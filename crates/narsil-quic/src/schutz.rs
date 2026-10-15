//! QUIC-Paketschutz nach RFC 9001: Initial-Schluessel, Header-Schutz, AEAD.
//!
//! Was hier gebaut wird (die Reihenfolge entspricht dem Auftrag Q1):
//!   * Initial-Geheimnis aus der Destination-Connection-ID (HKDF-Extract),
//!   * Richtungsgeheimnisse "client in"/"server in" (HKDF-Expand-Label),
//!   * Paket-Schluessel "quic key"/"quic iv"/"quic hp" (HKDF-Expand-Label),
//!   * AEAD-Nonce aus IV und Paketnummer (RFC 9001, Abschnitt 5.3),
//!   * Header-Schutz-Maske: AES-ECB oder ChaCha20 (Abschnitt 5.4),
//!   * Retry-Integritaet (Abschnitt 5.8),
//!   * und die beiden Ganzpaket-Funktionen `*_schuetzen`/`entschuetzen`.
//!
//! Die Kryptografie kommt aus `narsil-tls` (AEAD, HKDF, AES, ChaCha20). Hier
//! liegt nur die QUIC-Verdrahtung. Jeder Wert ist gegen RFC 9001 Anhang A
//! byte-genau geprueft (tests/rfc9001_anhang_a.rs).

use crate::kopf::{
    KopfFehler, MAX_CID_LAENGE, MAX_TOKEN_LAENGE, kurze_schreiben, lange_schreiben,
    paketnummer_rekonstruieren,
};
use narsil_tls::aes::{Aes, GcmFehler};
use narsil_tls::chacha::{AeadFehler, ChaCha20};
use narsil_tls::hkdf::{Fehler as HkdfFehler, extract_sha256};
use narsil_tls::{aead_chiffre, aead_dechiffre, expand_label_sha256, gcm_chiffre, gcm_dechiffre};

/// Initial-Salz aus RFC 9001, Abschnitt 5.2.
pub const INITIAL_SALZ: [u8; 20] = [
    0x38, 0x76, 0x2c, 0xf7, 0xf5, 0x59, 0x34, 0xb3, 0x4d, 0x17, 0x9a, 0xe6, 0xa4, 0xc8, 0x0c, 0xad,
    0xcc, 0xbb, 0x7f, 0x0a,
];

/// Das feste Retry-Geheimnis aus RFC 9001, Abschnitt 5.8.
pub const RETRY_GEHEIMNIS: [u8; 32] = [
    0xd9, 0xc9, 0x94, 0x3e, 0x61, 0x01, 0xfd, 0x20, 0x00, 0x21, 0x50, 0x6b, 0xcc, 0x02, 0x81, 0x4c,
    0x73, 0x03, 0x0f, 0x25, 0xc7, 0x9d, 0x71, 0xce, 0x87, 0x6e, 0xca, 0x87, 0x6e, 0x6f, 0xca, 0x8e,
];

/// Die von QUIC benutzten AEAD-Verfahren. AES-128-GCM schuetzt Initial-,
/// Handshake- und 1-RTT-Pakete, ChaCha20-Poly1305 ist die Alternative fuer
/// 1-RTT (RFC 9001, Abschnitt 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suite {
    Aes128Gcm,
    ChaCha20Poly1305,
}

impl Suite {
    /// Laenge des AEAD-Schluessels in Byte.
    pub fn schluessel_laenge(self) -> usize {
        match self {
            Suite::Aes128Gcm => 16,
            Suite::ChaCha20Poly1305 => 32,
        }
    }

    /// Laenge des Header-Schutz-Schluessels in Byte.
    pub fn hp_laenge(self) -> usize {
        match self {
            Suite::Aes128Gcm => 16,
            Suite::ChaCha20Poly1305 => 32,
        }
    }
}

/// Ein abgeleiteter Satz Paket-Schluessel. Die Felder sind fest gross
/// (M4): AES nutzt die ersten 16 Byte, ChaCha20 alle 32.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaketSchluessel {
    pub suite: Suite,
    /// AEAD-Schluessel (16 oder 32 Byte genutzt).
    pub schluessel: [u8; 32],
    /// AEAD-IV (12 Byte, RFC 9001 Abschnitt 5.1).
    pub iv: [u8; 12],
    /// Header-Schutz-Schluessel (16 oder 32 Byte genutzt).
    pub hp: [u8; 32],
}

/// Warum der Paketschutz scheitert. Benannte Varianten, keine Panics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchutzFehler {
    /// Das Paket ist zu kurz fuer Kopf, Paketnummer oder Sample.
    PaketZuKurz { braucht: usize, da: usize },
    /// Die Paketnummer passt nicht in 62 Bit.
    PnZuGross { pn: u64 },
    /// Der Puffer enthaelt **mehr** als ein Paket (N41).
    ///
    /// DAS IST EINE EIGENE FEHLERART, weil das sonst ein "falscher Tag"
    /// aussieht. Ein Datagramm kann nach RFC 9000 12.2 mehrere Paketlisten
    /// enthalten; fuettert man der Entschluesselung das ganze Datagramm,
    /// rechnet sie mit Bytes hinter dem 16-Byte-Tag — und der Tag stimmt
    /// nicht. Man haelt das dann fuer ein Schluesselproblem, denn
    /// "Initial-Schluessel passen nicht" sieht man, "Paketliste nicht
    /// getrennt" sieht man **nicht**.
    ///
    /// GEMESSEN (N41.2): von vier Initial-Paketen einer echten
    /// Cloudflare-Verbindung gehen geteilt **4 von 4** auf, ungeteilt **1 von
    /// 4**. Die drei uebrigen waren jeweils der erste einer Paketliste
    /// (1200-Byte-Datagramm, Paketende bei Byte 52, 52 und 81).
    DatagrammZuLang { paket: usize, da: usize },
    /// Das Laenge-Feld nennt weniger, als ein Paket braucht (N41).
    LaengeFeldZuKurz { laenge: u64, braucht: u64 },
    /// HKDF meldete einen Fehler.
    Hkdf(HkdfFehler),
    /// Der Kopf war kaputt (einschliesslich CID- und Token-Grenzen).
    Kopf(KopfFehler),
    /// AES-GCM meldete einen Fehler.
    Gcm(GcmFehler),
    /// ChaCha20-Poly1305 meldete einen Fehler.
    Aead(AeadFehler),
}

impl std::fmt::Display for SchutzFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchutzFehler::PaketZuKurz { braucht, da } => {
                write!(f, "Paket braucht {braucht} Byte, da sind {da}")
            }
            SchutzFehler::PnZuGross { pn } => write!(f, "Paketnummer {pn} passt nicht in 62 Bit"),
            SchutzFehler::DatagrammZuLang { paket, da } => write!(
                f,
                "Datagramm hat {da} Byte, das Paket nur {paket} — Paketliste nicht getrennt?"
            ),
            SchutzFehler::LaengeFeldZuKurz { laenge, braucht } => write!(
                f,
                "Laenge-Feld nennt {laenge} Byte, ein Paket braucht mindestens {braucht}"
            ),
            SchutzFehler::Hkdf(e) => write!(f, "HKDF: {e}"),
            SchutzFehler::Kopf(e) => write!(f, "Kopf: {e}"),
            SchutzFehler::Gcm(e) => write!(f, "AES-GCM: {e}"),
            SchutzFehler::Aead(e) => write!(f, "ChaCha20-Poly1305: {e}"),
        }
    }
}

impl std::error::Error for SchutzFehler {}

impl From<HkdfFehler> for SchutzFehler {
    fn from(e: HkdfFehler) -> Self {
        SchutzFehler::Hkdf(e)
    }
}

impl From<KopfFehler> for SchutzFehler {
    fn from(e: KopfFehler) -> Self {
        SchutzFehler::Kopf(e)
    }
}

/// Hoechstgrenze fuer ein UDP-Datagramm, als **benannte** Grenze fuer
/// Laengen aus fremden Bytes (M7, N41). 65527 ist die groesste UDP-Nutzlast,
/// die in einem IPv4-Datagramm ueberhaupt Platz hat (65535 abzueglich 8 Byte
/// UDP-Kopf).
pub const MAX_DATAGRAMM: usize = 65_527;

/// Das Initial-Geheimnis: HKDF-Extract(INITIAL_SALZ, DCID) (RFC 9001,
/// Abschnitt 5.2). Die DCID ist die Destination-Connection-ID aus dem
/// ersten Initial-Paket des Clients.
pub fn initial_geheimnis(dcid: &[u8]) -> Result<[u8; 32], SchutzFehler> {
    if dcid.len() > MAX_CID_LAENGE {
        return Err(SchutzFehler::Kopf(KopfFehler::CidZuLang {
            ist: dcid.len(),
            maximum: MAX_CID_LAENGE,
        }));
    }
    Ok(extract_sha256(&INITIAL_SALZ, dcid))
}

/// Ein Richtungsgeheimnis: HKDF-Expand-Label(initial, label, "", 32). Die
/// Labels sind "client in" und "server in" (RFC 9001, Abschnitt 5.2).
pub fn richtungs_geheimnis(initial: &[u8; 32], label: &[u8]) -> Result<[u8; 32], SchutzFehler> {
    let bytes = expand_label_sha256(initial, label, b"", 32)?;
    let mut aus = [0u8; 32];
    aus.copy_from_slice(&bytes);
    Ok(aus)
}

/// Leitet aus einem 32-Byte-Geheimnis die Paket-Schluessel ab
/// ("quic key", "quic iv", "quic hp"; RFC 9001, Abschnitt 5.1).
pub fn schluessel_ableiten(
    geheimnis: &[u8; 32],
    suite: Suite,
) -> Result<PaketSchluessel, SchutzFehler> {
    let schluessel_laenge = suite.schluessel_laenge();
    let hp_laenge = suite.hp_laenge();

    let schluessel_bytes = expand_label_sha256(geheimnis, b"quic key", b"", schluessel_laenge)?;
    let iv_bytes = expand_label_sha256(geheimnis, b"quic iv", b"", 12)?;
    let hp_bytes = expand_label_sha256(geheimnis, b"quic hp", b"", hp_laenge)?;

    let mut schluessel = [0u8; 32];
    schluessel[..schluessel_laenge].copy_from_slice(&schluessel_bytes);
    let mut iv = [0u8; 12];
    iv.copy_from_slice(&iv_bytes);
    let mut hp = [0u8; 32];
    hp[..hp_laenge].copy_from_slice(&hp_bytes);

    Ok(PaketSchluessel {
        suite,
        schluessel,
        iv,
        hp,
    })
}

/// Baut die AEAD-Nonce: IV XOR Paketnummer (links mit Nullen auf 12 Byte
/// aufgefuellt; RFC 9001, Abschnitt 5.3). Die Paketnummer ist ein 62-Bit-
/// Wert, die oberen zwei Bit von `pn` sind also immer 0.
pub fn nonce_berechnen(iv: &[u8; 12], pn: u64) -> [u8; 12] {
    let mut nonce = *iv;
    let pn_bytes = pn.to_be_bytes();
    // Paketnummer in den letzten 8 Byte der Nonce.
    for i in 0..8 {
        nonce[4 + i] ^= pn_bytes[i];
    }
    nonce
}

/// Berechnet die 5-Byte-Header-Schutz-Maske (RFC 9001, Abschnitt 5.4).
/// `sample` sind mindestens 16 Byte Chiffrat ab `pn_offset + 4`.
pub fn maske_berechnen(suite: Suite, hp: &[u8], sample: &[u8]) -> Result<[u8; 5], SchutzFehler> {
    if sample.len() < 16 {
        return Err(SchutzFehler::PaketZuKurz {
            braucht: 16,
            da: sample.len(),
        });
    }
    let s16 = &sample[..16];
    match suite {
        Suite::Aes128Gcm => {
            if hp.len() < 16 {
                return Err(SchutzFehler::PaketZuKurz {
                    braucht: 16,
                    da: hp.len(),
                });
            }
            let aes = Aes::neu(&hp[..16]).ok_or(SchutzFehler::PaketZuKurz {
                braucht: 16,
                da: hp.len(),
            })?;
            let mut block = [0u8; 16];
            block.copy_from_slice(s16);
            aes.verschluessel(&mut block);
            Ok([block[0], block[1], block[2], block[3], block[4]])
        }
        Suite::ChaCha20Poly1305 => {
            if hp.len() < 32 {
                return Err(SchutzFehler::PaketZuKurz {
                    braucht: 32,
                    da: hp.len(),
                });
            }
            let mut schluessel = [0u8; 32];
            schluessel.copy_from_slice(&hp[..32]);
            let mut nonce = [0u8; 12];
            nonce.copy_from_slice(&s16[4..16]);
            // Die ersten 4 Byte des Samples sind der Blockzaehler,
            // little-endian (RFC 9001, Abschnitt 5.4.4).
            let zaehler = u32::from_le_bytes([s16[0], s16[1], s16[2], s16[3]]);
            let mut chacha = ChaCha20::neu(&schluessel, &nonce, zaehler);
            let mut null = [0u8; 5];
            chacha.anwenden(&mut null);
            Ok(null)
        }
    }
}

/// AEAD-Versiegelung auf QUIC-Art: `Chiffrat || Tag` (RFC 9001, Abschnitt 5.3).
fn aead_versiegeln(
    s: &PaketSchluessel,
    nonce: &[u8; 12],
    aad: &[u8],
    klartext: &[u8],
) -> Result<Vec<u8>, SchutzFehler> {
    match s.suite {
        Suite::Aes128Gcm => {
            let (chiffrat, tag) = gcm_chiffre(&s.schluessel[..16], nonce, aad, klartext)
                .map_err(SchutzFehler::Gcm)?;
            let mut aus = chiffrat;
            aus.extend_from_slice(&tag);
            Ok(aus)
        }
        Suite::ChaCha20Poly1305 => {
            aead_chiffre(&s.schluessel, nonce, aad, klartext).map_err(SchutzFehler::Aead)
        }
    }
}

/// AEAD-Oeffnung auf QUIC-Art: `Chiffrat || Tag` -> Klartext.
fn aead_oeffnen(
    s: &PaketSchluessel,
    nonce: &[u8; 12],
    aad: &[u8],
    chiffrat: &[u8],
) -> Result<Vec<u8>, SchutzFehler> {
    match s.suite {
        Suite::Aes128Gcm => {
            if chiffrat.len() < 16 {
                return Err(SchutzFehler::PaketZuKurz {
                    braucht: 16,
                    da: chiffrat.len(),
                });
            }
            let (koerper, tag) = chiffrat.split_at(chiffrat.len() - 16);
            let mut t = [0u8; 16];
            t.copy_from_slice(tag);
            gcm_dechiffre(&s.schluessel[..16], nonce, aad, koerper, &t).map_err(SchutzFehler::Gcm)
        }
        Suite::ChaCha20Poly1305 => {
            aead_dechiffre(&s.schluessel, nonce, aad, chiffrat).map_err(SchutzFehler::Aead)
        }
    }
}

/// Schuetzt ein Long-Header-Paket (Initial, 0-RTT oder Handshake) und gibt
/// das vollstaendige, geschuetzte Paket zurueck.
///
/// Viele Argumente sind Absicht: sie bilden die Skalarfelder des QUIC-Kopfes
/// eins zu eins ab (siehe `lange_schreiben`).
#[allow(clippy::too_many_arguments)]
pub fn initial_schuetzen(
    schluessel: &PaketSchluessel,
    typ: u8,
    version: u32,
    dcid: &[u8],
    scid: &[u8],
    token: &[u8],
    pn: u64,
    pn_laenge: usize,
    klartext: &[u8],
) -> Result<Vec<u8>, SchutzFehler> {
    if pn >= (1u64 << 62) {
        return Err(SchutzFehler::PnZuGross { pn });
    }
    // KEINE Auffuellung hier — siehe `mit_auffuellung`. Ein zu kurzes Paket
    // ist ein benannter Fehler des Aufrufers, keine Sache dieser Funktion.
    let (kopf, pn_offset) = lange_schreiben(
        typ,
        version,
        dcid,
        scid,
        token,
        pn,
        pn_laenge,
        klartext.len(),
    )?;
    let nonce = nonce_berechnen(&schluessel.iv, pn);
    let chiffrat = aead_versiegeln(schluessel, &nonce, &kopf, klartext)?;

    // Sample beginnt 4 Byte nach der Paketnummer, also bei
    // pn_offset + 4 - header_laenge = 4 - pn_laenge im Chiffrat.
    let versatz = 4usize.saturating_sub(pn_laenge);
    if chiffrat.len() < versatz + 16 {
        return Err(SchutzFehler::PaketZuKurz {
            braucht: versatz + 16,
            da: chiffrat.len(),
        });
    }
    let sample = &chiffrat[versatz..versatz + 16];
    let mask = maske_berechnen(schluessel.suite, &schluessel.hp, sample)?;

    let mut kopf = kopf;
    kopf[0] ^= mask[0] & 0x0f;
    for i in 0..pn_laenge {
        kopf[pn_offset + i] ^= mask[1 + i];
    }
    let mut paket = kopf;
    paket.extend_from_slice(&chiffrat);
    Ok(paket)
}

/// Die kleinste Nutzlast, bei der das Header-Schutz-Sample vollstaendig im
/// Chiffrat liegt.
///
/// **DER GRUND, DASS ES DIE KONSTANTE GIBT:** RFC 9001 Abschnitt 5.4.2 liest
/// das Sample 4 Byte **nach** dem Ende der Paketnummer ab. Bei einer
/// Paketnummer von `n` Byte beginnt das Sample also bei Chiffrat-Byte `4 - n`
/// und ist 16 Byte lang. Das Chiffrat ist `Klartext + 16` Tag, also muss der
/// Klartext mindestens `4 - n` Byte haben — bei 2 Byte Paketnummer also 2.
///
/// **WARUM SIE NICHT IN DEN PROTECT-FUNKTIONEN STECKT, sondern hier exportiert
/// wird:** sie hat zuerst dort gesteckt, und der Fehler war ein *anderer* als
/// gedacht. `initial_schuetzen(&s, .., 0, 4, b"")` lieferte daraufhin ein
/// Paket, das vier PADDING-Byte mehr im Klartext hatte als die Eingabe — und
/// der Test `rundlauf_vieler_laengen` in `tests/bomben.rs` vergleicht den
/// Klartext mit der Eingabe. Er fiel durch und diagnostizierte richtig: eine
/// Protect-Funktion, die den Klartext veraendert, ist keine Protect-Funktion.
/// Wer `entschuetzen(entschuetzen(x)) == x` braucht (und das ist die
/// Zusicherung dieser Crate), darf keine stillen Bytes einruecken.
///
/// Der Aufrufer fuellt auf — dort, wo PADDING hingehört: zwischen echten
/// Frames eines QUIC-Pakets.
pub const MIN_NUTZLAST: usize = 4;

/// Fuellt eine zu kurze Nutzlast mit PADDING-Frames (0x00) auf.
///
/// `0x00` ist der PADDING-Frame-Typ (RFC 9000 Abschnitt 12.1) und ueberall
/// erlaubt, also auch zwischen echten Frames.
///
/// M3: die Schleife laeuft ueber `MIN_NUTZLAST`, eine Konstante — nicht ueber
/// die Laenge der Eingabe. Ein Aufruf mit 1 Byte Nutzlast gibt 4 Byte zurueck,
/// einer mit 1 MiB gibt 1 MiB unveraendert zurueck (nur kopiert).
pub fn mit_auffuellung(klartext: &[u8], pn_laenge: usize) -> Vec<u8> {
    let versatz = 4usize.saturating_sub(pn_laenge.min(4));
    let braucht = MIN_NUTZLAST.max(versatz);
    if klartext.len() >= braucht {
        return klartext.to_vec();
    }
    let mut raus = klartext.to_vec();
    raus.resize(braucht, 0x00);
    raus
}

/// Schuetzt ein Short-Header-Paket (1-RTT) und gibt das vollstaendige,
/// geschuetzte Paket zurueck.
pub fn kurzes_schuetzen(
    schluessel: &PaketSchluessel,
    spin: bool,
    key_phase: bool,
    dcid: &[u8],
    pn: u64,
    pn_laenge: usize,
    klartext: &[u8],
) -> Result<Vec<u8>, SchutzFehler> {
    if pn >= (1u64 << 62) {
        return Err(SchutzFehler::PnZuGross { pn });
    }
    // KEINE Auffuellung hier — siehe `mit_auffuellung`.
    let (kopf, pn_offset) = kurze_schreiben(spin, key_phase, dcid, pn, pn_laenge)?;
    let nonce = nonce_berechnen(&schluessel.iv, pn);
    let chiffrat = aead_versiegeln(schluessel, &nonce, &kopf, klartext)?;

    let versatz = 4usize.saturating_sub(pn_laenge);
    if chiffrat.len() < versatz + 16 {
        return Err(SchutzFehler::PaketZuKurz {
            braucht: versatz + 16,
            da: chiffrat.len(),
        });
    }
    let sample = &chiffrat[versatz..versatz + 16];
    let mask = maske_berechnen(schluessel.suite, &schluessel.hp, sample)?;

    let mut kopf = kopf;
    kopf[0] ^= mask[0] & 0x1f;
    for i in 0..pn_laenge {
        kopf[pn_offset + i] ^= mask[1 + i];
    }
    let mut paket = kopf;
    paket.extend_from_slice(&chiffrat);
    Ok(paket)
}

/// Soll `entschuessen` die reservierten Bits des Kopfs ablehnen
/// (RFC 9000 17.2 Long / 17.3 Short)?
///
/// **STAND 01.10.2026: `true`.** Die Pruefung ist aktiv.
///
/// DIE GESCHICHTE, in drei Absaetzen, weil sie kuerzer nicht geht:
///
///  1. N11 meldete: "bei 5 von 8 eigenen Nutzlastlaengen stellt `entschuetzen`
///     ein falsches erstes Byte wieder her". Auf dieser Zahl beruhte zwei
///     Berichte lang die Aussage, U2-Rest sei "durch einen eigenen Fehler
///     blockiert".
///  2. N15: **die Zahl war falsch.** Sie stammte aus einer Pruefung, die in
///     `lange_lesen` stand — und `lange_lesen` wird mit **geschuetzten** Bytes
///     aufgerufen. Die "falschen Erstbytes" waren die maskierten Bits eines
///     Pakets, das nie entschluesselt war. Der Fehler war meiner, nicht der des
///     Dekoders.
///  3. N18, die Messung: mit der Pruefung an der richtigen Stelle
///     (**nach** der AEAD, N16) liefern alle acht Nutzlastlaengen
///     `erstes_byte = 0xc3` und reserviert `0x00`. Der Dekoder war die ganze
///     Zeit richtig.
///
/// WAS ICH DARAUS GELERNT HABE und warum es hier steht: Ein Test, der eine
/// Zahl meldet, muss die Zahl **zwei** Mal aus zwei Wegen bestaetigen, bevor
/// sie einen Bericht trägt. N11 stand in zwei Berichten und war eine
/// Fehlmessung.
///
/// DIE PRUEFUNG LIEGT NACH DER AEAD, nicht davor. Das ist der eigentliche
/// Punkt, und er bleibt unabhaengig von N11: ein Paket mit falschem Tag ist
/// unauthentifiziert, und ueber seine Bytes etwas zu behaupten heisst, sich auf
/// Daten zu berufen, die jeder im Netzpfad frei waehlen darf.
///
/// `tests/bomben.rs` und `tests/kopf_reservierte_bits.rs` halten die Zahlen
/// fest. Siehe `team/BERICHT_QUIC_U2.txt` N11, N13 und N15.
pub const PRUEFE_RESERVIERTE_BITS: bool = true;

/// Der Klartext und die Paketnummer eines entschluesselten Pakets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entschuetzt {
    pub klartext: Vec<u8>,
    pub paketnummer: u64,
    /// `true` bei einem Long Header (Initial/0-RTT/Handshake).
    pub lang: bool,
    /// Long-Paket-Typ, nur sinnvoll wenn `lang` gesetzt ist.
    pub typ: u8,
    /// Das erste Byte **nach** dem Entmasken, also der wiederhergestellte
    /// Kopf (U2-Bericht N13).
    ///
    /// WARUM ES HIER RAUSKOMMT: um den wiederhergestellten Kopf zu pruefen,
    /// muss man ihn sehen koennen. Vorher gab es nur den Klartext, und der
    /// stimmt auch dann, wenn der Kopf falsch ist — der AEAD-Tag laeuft ueber
    /// die **geschuetzten** Bytes. Genau deshalb hat der Fehler aus N11 so
    /// lange unbemerkt bleiben koennen: es gab keinen Weg, ihn zu sehen, ausser
    /// man baut sich eine eigene Entschluesselung.
    pub erstes_byte: u8,
    /// Wie viele Byte von `daten` dieses Paket belegt (N41).
    ///
    /// Bei einem Long Header ist das `pn_offset + Laenge-Feld`; bei einem
    /// Short Header der Rest des Puffers. Wer ein Datagramm selbst durchgeht,
    /// braucht diese Zahl, um beim naechsten Paket weiterzulesen.
    pub paket_ende: usize,
}

/// Entfernt Header-Schutz und AEAD von einem Paket. `dcid_laenge` wird nur
/// fuer Short Header gebraucht (dort steht die Connection-ID nicht im Kopf).
/// `groesste_pn` ist die groesste bisher empfangene Paketnummer im selben
/// Nummernraum (fuer die Rekonstruktion der gekuerzten Paketnummer).
pub fn entschuetzen(
    schluessel: &PaketSchluessel,
    daten: &[u8],
    dcid_laenge: usize,
    groesste_pn: u64,
) -> Result<Entschuetzt, SchutzFehler> {
    if daten.is_empty() {
        return Err(SchutzFehler::PaketZuKurz { braucht: 1, da: 0 });
    }
    let lang = daten[0] & 0x80 != 0;

    let (typ, pn_offset, laenge_feld) = if lang {
        let kopf = crate::kopf::lange_lesen(daten)?;
        (kopf.typ, kopf.pn_offset, Some(kopf.laenge))
    } else {
        let kopf = crate::kopf::kurze_lesen(daten, dcid_laenge)?;
        (0, kopf.pn_offset, None)
    };

    // Das Sample braucht 16 Byte ab pn_offset + 4.
    let sample_ende = pn_offset + 4 + 16;
    if daten.len() < sample_ende {
        return Err(SchutzFehler::PaketZuKurz {
            braucht: sample_ende,
            da: daten.len(),
        });
    }
    let sample = &daten[pn_offset + 4..sample_ende];
    let mask = maske_berechnen(schluessel.suite, &schluessel.hp, sample)?;

    // Erst das erste Byte entmasken, um die Paketnummernlaenge zu erhalten.
    let erstes = daten[0] ^ (mask[0] & if lang { 0x0f } else { 0x1f });

    // DIE RESERVIERTEN BITS, und **hier** — nicht in `kurze_lesen` und nicht
    // in `lange_lesen`.
    //
    // DER GRUND: die Kopf-Leser werden mit **geschuetzten** Bytes aufgerufen
    // (siehe die beiden Aufrufe weiter oben). Sie muessen es, denn sie
    // liefern das `pn_offset`, ohne das das Sample nicht zu finden ist. Eine
    // Pruefung der reservierten Bits in den Lesern wuerde daher jedes Paket
    // verwerfen, weil die Bits im geschuetzten Zustand zufaellig sind. Der
    // erste Versuch stand dort, und der Test
    // `die_pruefung_gehoert_nach_den_kopfschutz` in
    // `tests/kopf_reservierte_bits.rs` hat es gemeldet.
    //
    // DIE ZAHLEN, und sie sind nach RFC 9000 **verschieden**:
    //   Abschnitt 17.3 (Short): "Two bits (those with a mask of 0x18)"
    //   Abschnitt 17.2 (Long):  "Two bits (those with a mask of 0x0c)"
    //
    // Kurz gesagt: die Bits **zwischen** Spin und Key Phase. Beim Short Header
    // liegen sie bei 0x18, beim Long Header bei 0x0c, weil dort das Typfeld
    // ein Bit tiefer sitzt.
    let pn_laenge = ((erstes & 0x03) as usize) + 1;
    let header_laenge = pn_offset + pn_laenge;

    // N41: DAS LAENGE-FELD GIBT AN, WO DAS PAKET AUFHOERT — und ein Long
    // Header **muss** es haben (RFC 9000 17.2). Es deckt Paketnummer und
    // Nutzlast, also endet das Paket bei `pn_offset + laenge`, unabhaengig von
    // der Paketnummernlaenge.
    //
    // OHNE DIESE PRUEFUNG passiert genau das, was in N41 gemessen wurde: man
    // fuettert ein 1200-Byte-Datagramm an, das eine Paketliste ist, und der
    // AEAD-Oeffner rechnet mit 1148 Byte hinter dem Tag. Der Tag stimmt nicht,
    // die Meldung lautet "AES-GCM: der Tag stimmt nicht", und man schliesst auf
    // die Schluessel. **Beides ist eine andere Sache.**
    let mut paket_ende = daten.len();
    if let Some(lf) = laenge_feld {
        // Mindestinhalt: Paketnummer (1) plus Tag (16). Weniger geht nicht.
        let braucht = pn_laenge as u64 + 16;
        if lf < braucht {
            return Err(SchutzFehler::LaengeFeldZuKurz {
                laenge: lf,
                braucht,
            });
        }
        // `pn_offset + laenge` kann ueberlaufen; das wird als Laenge-Fehler
        // benannt, nicht als Panik (M7: fremde Bytes duerfen nichts reissen).
        let ende = (pn_offset as u64)
            .checked_add(lf)
            .ok_or(SchutzFehler::LaengeFeldZuKurz {
                laenge: lf,
                braucht: braucht.max(u64::MAX / 2),
            })?;
        // Grenze aus fremden Daten: ein Datagramm ist kleiner als 64 KiB in
        // jedem erdenklichen Fall, und `MAX_DATAGRAMM` steht als Name dafuer.
        if ende > MAX_DATAGRAMM as u64 {
            return Err(SchutzFehler::PaketZuKurz {
                braucht: MAX_DATAGRAMM,
                da: daten.len(),
            });
        }
        paket_ende = ende as usize;
        // ZUERST das Fehlende, DANN das Zuviel — in dieser Reihenfolge, und
        // das ist nicht kosmetisch.
        //
        // DER GRUND, und ich habe ihn selbst hereingebracht: Ein
        // Laenge-Feld, das **ueber** den Puffer hinausweist (der Test
        // `laenge_feld_ueber_puffer`), liess `&daten[header_laenge..paket_ende]`
        // auf einen Bereich laufen, der laenger ist als der Puffer — **eine
        // Panik an fremden Bytes**, eingefuehrt von der Härtung selbst. Der
        // Bombentest hat sie im selben Lauf gefunden, in dem ich sie gebaut
        // habe. Vorher stand nur `&daten[header_laenge..]`, und das konnte
        // nicht ueberlaufen.
        if daten.len() < paket_ende {
            return Err(SchutzFehler::PaketZuKurz {
                braucht: paket_ende,
                da: daten.len(),
            });
        }
        // UND JETZT DIE EIGENTLICHE WARNUNG, mit Namen: mehr Bytes als das
        // Paket braucht heisst "du hast nicht getrennt".
        if daten.len() > paket_ende {
            return Err(SchutzFehler::DatagrammZuLang {
                paket: paket_ende,
                da: daten.len(),
            });
        }
    }
    if daten.len() < header_laenge + 16 {
        return Err(SchutzFehler::PaketZuKurz {
            braucht: header_laenge + 16,
            da: daten.len(),
        });
    }

    // Paketnummer entmasken.
    let mut pn_bytes = [0u8; 4];
    for i in 0..pn_laenge {
        pn_bytes[4 - pn_laenge + i] = daten[pn_offset + i] ^ mask[1 + i];
    }
    let abgeschnitten = u32::from_be_bytes(pn_bytes) as u64;
    let pn = paketnummer_rekonstruieren(abgeschnitten, pn_laenge, groesste_pn);

    // AAD = ungeschuetzter Kopf bis einschliesslich Paketnummer.
    let mut aad = Vec::with_capacity(header_laenge);
    aad.push(erstes);
    aad.extend_from_slice(&daten[1..pn_offset]);
    aad.extend_from_slice(&pn_bytes[4 - pn_laenge..]);

    let nonce = nonce_berechnen(&schluessel.iv, pn);
    let klartext = aead_oeffnen(schluessel, &nonce, &aad, &daten[header_laenge..paket_ende])?;

    // DIE RESERVIERTEN BITS — und zwar HIER, **nach** der AEAD.
    //
    // DER ORT IST DIE GANZE FRAGE, und er ist mir zuerst falsch geraten.
    //
    // Zuerst stand die Pruefung VOR der Entschluesselung, mit der Begruendung
    // "die Leser sehen geschuetzte Bytes". Das war richtig gedacht und falsch
    // gemacht: `erstes` ist auch **vor** der AEAD schon entmaskt, die
    // Pruefungsassessenz war also nicht die. Der wahre Grund ist ein anderer
    // und viel schlimmerer:
    //
    // **Ein Paket, dessen Tag nicht stimmt, ist unauthentifiziert.** Ueber
    // seine Bytes zu sagen "die reservierten Bits sind gesetzt" heisst, sich
    // auf Daten zu berufen, die jeder im Netzpfad frei gewaehlt hat. Das ist
    // die Umkehrung von RFC 9001 Abschnitt 5.5: nicht der Emphaenger soll
    // schweigen, sondern man soll nichts behaupten, was man nicht geprueft
    // hat.
    //
    // GEMESSEN, wie das schiefgeht: `tests/cloudflare_mitschnitt.rs` meldete
    // `ReserviertesBit { bit: 12, wert: 8 }` fuer einen **echten** Cloudflare-
    // Initial. Die **Wahrheit** ist, dass dieser Initial bei uns gar nicht
    // entschluesselt:
    //
    //     Teil 0 (Initial): FEHLER AES-GCM: der Tag stimmt nicht
    //
    // Die Paketnummer ist 0, und unser Initial-Geheimnis aus
    // `cloudflare_dcid.bin` passt offenbar nicht zu diesem Paket. Der
    // Header war also nie aussagekraeftig, und meine Schlussfolgerung "unser
    // Dekoder ist bei konformen Serverpaketen falsch" war **falsch**. Sie ist
    // hiermit zurueckgenommen; sie steht im Bericht als Korrektur.
    //
    // NACH der AEAD ist die Pruefung harmlos: sie sieht nur Bytes, die der
    // Absender signiert hat.
    if PRUEFE_RESERVIERTE_BITS {
        let reserviert = if lang { 0x0c } else { 0x18 };
        if erstes & reserviert != 0 {
            return Err(SchutzFehler::Kopf(KopfFehler::ReserviertesBit {
                bit: reserviert,
                wert: erstes & reserviert,
            }));
        }
    }

    Ok(Entschuetzt {
        klartext,
        paketnummer: pn,
        lang,
        typ,
        erstes_byte: erstes,
        paket_ende,
    })
}

/// Berechnet den Retry-Integritaets-Tag (RFC 9001, Abschnitt 5.8).
///
/// `odcid` ist die urspruengliche Destination-Connection-ID aus dem
/// Initial-Paket, auf das der Retry antwortet. `erstes_byte` ist das erste
/// Byte des uebertragenen Retry-Pakets (hier 0xf0 plus "Unused"-Bits).
pub fn retry_tag(
    odcid: &[u8],
    erstes_byte: u8,
    version: u32,
    dcid: &[u8],
    scid: &[u8],
    token: &[u8],
) -> Result<[u8; 16], SchutzFehler> {
    for feld in [odcid, dcid, scid] {
        if feld.len() > MAX_CID_LAENGE {
            return Err(SchutzFehler::Kopf(KopfFehler::CidZuLang {
                ist: feld.len(),
                maximum: MAX_CID_LAENGE,
            }));
        }
    }
    if token.len() > MAX_TOKEN_LAENGE {
        return Err(SchutzFehler::Kopf(KopfFehler::TokenZuLang {
            ist: token.len(),
            maximum: MAX_TOKEN_LAENGE,
        }));
    }

    // Das Retry-Pseudo-Paket (RFC 9001, Abbildung 8).
    let mut pseudo = Vec::new();
    pseudo.push(odcid.len() as u8);
    pseudo.extend_from_slice(odcid);
    pseudo.push(erstes_byte);
    pseudo.extend_from_slice(&version.to_be_bytes());
    pseudo.push(dcid.len() as u8);
    pseudo.extend_from_slice(dcid);
    pseudo.push(scid.len() as u8);
    pseudo.extend_from_slice(scid);
    pseudo.extend_from_slice(token);

    let schluessel = schluessel_ableiten(&RETRY_GEHEIMNIS, Suite::Aes128Gcm)?;
    let (_, tag) = gcm_chiffre(&schluessel.schluessel[..16], &schluessel.iv, &pseudo, b"")
        .map_err(SchutzFehler::Gcm)?;
    Ok(tag)
}
