//! QUIC-Paketkoepfe (RFC 9000, Abschnitt 17): Long Header und Short Header.
//!
//! Ein Kopf wird hier in zwei Richtungen gebraucht:
//!   * Lesen (`lange_lesen`, `kurze_lesen`): um den Offset der Paketnummer
//!     (`pn_offset`) zu bestimmen, von dem aus der Header-Schutz (RFC 9001
//!     Abschnitt 5.4.2) sein 16-Byte-Sample nimmt. Alle Felder bis auf die
//!     Paketnummer stehen im Klartext, deshalb laesst sich `pn_offset` ohne
//!     den Header-Schutz bestimmen.
//!   * Schreiben (`lange_schreiben`, `kurze_schreiben`): um ein Paket zu
//!     bauen, dessen ungeschuetzter Kopf der AEAD als zusaetzliche Daten
//!     (AAD) dient.
//!
//! M3/M4: Connection-IDs und Token sind in ihrer Laenge gedeckelt, bevor
//! allokiert wird. Der Leser prueft jede Laenge gegen den Puffer, BEVOR er
//! einen Ausschnitt nimmt.

use crate::varint::{self, VarintFehler};

/// Groesste erlaubte Connection-ID in Byte (RFC 9000, Abschnitt 5.1:
/// 0..20 Byte).
pub const MAX_CID_LAENGE: usize = 20;
/// Groesstes Token in Byte. Ein Initial-Token kommt aus einem Retry und ist
/// damit Teil der Eingabe; diese harte Grenze (M4) verhindert, dass ein
/// boesartiges Token den Speicher sprengt. RFC 9000 begrenzt das Token nur
/// implizit ueber die Datagrammgroesse.
pub const MAX_TOKEN_LAENGE: usize = 4096;

/// Long-Packet-Typen (RFC 9000, Abschnitt 17.2).
pub const TYP_INITIAL: u8 = 0;
pub const TYP_ZERO_RTT: u8 = 1;
pub const TYP_HANDSHAKE: u8 = 2;
pub const TYP_RETRY: u8 = 3;

/// Die QUIC-Version 1 (RFC 9000 6.1).
///
/// BISHER STAND HIER ÜBERALL EIN LITERAL `1` — an mindestens fuenf Stellen.
/// Das ist kein Fehler, aber es ist eine Ziffer, die man an fuenf Orten
/// aendern muss, wenn sie sich aendert; und Q1 verlangt, die Versions-
/// verhandlung zu **messen**. Messen heisst: sie muss einen Namen haben.
pub const QUIC_VERSION_1: u32 = 0x0000_0001;

/// Wie viele Versionen ein Version-Negotiation-Paket anbieten darf (Q1).
///
/// RFC 9000 6.2 nennt keine Grenze; ein Datagramm hat hoechstens
/// `MAX_DATAGRAMM` Byte, und jedes Angebot braucht 4 Byte. Die Zahl ist
/// trotzdem da, weil eine Liste aus fremden Bytes eine Liste bleibt, die man
/// abtraegt.
pub const MAX_VERSIONEN: usize = 64;

/// Ist dieses Datagramm ein **Version-Negotiation**-Paket? (RFC 9000 17.2.1)
///
/// DIE REGEL: Long Header (Bit 7 gesetzt) **und** Version-Feld gleich null.
/// Genau null, nicht "irgendeine": ein Paket mit Version 0 ist per Norm ein
/// Versionsverhandlungspaket.
///
/// **WARUM DAS FEHLTE, UND WAS ES KOSTETE:** ohne diese Pruefung fiel ein
/// solches Paket in `zerlegen` hinein, wurde als Initial mit Typ 0
/// eingeordnet, scheiterte an der Entschluesselung (ein VN-Paket ist **nicht**
/// verschluesselt) — und der Client lief bis zum Zeitlimit. Die Meldung, die
/// dann kam, war `Zeitueberschreitung: keine Antwort nach dem
/// Idle-Zeitlimit`: man haelt den Server fuer stumm, dabei hat er sich
/// **offiziell fuer die Version entschieden**.
pub fn ist_versionsverhandlung(d: &[u8]) -> bool {
    d.len() >= 5 && d[0] & 0x80 != 0 && u32::from_be_bytes([d[1], d[2], d[3], d[4]]) == 0
}

/// Die angebotenen Versionen eines Version-Negotiation-Pakets.
///
/// RFC 9000 6.1: der Server nennt die Versionen, die er unterstuetzt, jeweils
/// als 4 Byte; der Client soll eine davon nehmen. Wir kennen **eine**, und
/// die lesen wir aus der Liste statt sie zu raten — sonst wuerde ein Paket, das
/// Version 1 anbietet, als "keine bekannt" abgewiesen.
pub fn versionen_lesen(d: &[u8]) -> Result<Vec<u32>, KopfFehler> {
    if !ist_versionsverhandlung(d) {
        return Err(KopfFehler::Abgeschnitten {
            braucht: 7,
            da: d.len(),
        });
    }
    let mut raus: Vec<u32> = Vec::new();
    let mut i = 5usize;
    // Die Connection-ID-Laengen stehen **vor** der Liste (17.2.1): der Server
    // wiederholt in DCID die SCID des ausloesenden Pakets und in SCID dessen
    // DCID. Wer sie ueberspringt, liest die Liste an der falschen Stelle.
    let dl = *d.get(i).ok_or(KopfFehler::Abgeschnitten {
        braucht: 6,
        da: d.len(),
    })? as usize;
    i += 1 + dl;
    let sl = *d.get(i).ok_or(KopfFehler::Abgeschnitten {
        braucht: i + 1,
        da: d.len(),
    })? as usize;
    i += 1 + sl;
    while i + 4 <= d.len() {
        if raus.len() >= MAX_VERSIONEN {
            break;
        }
        raus.push(u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]));
        i += 4;
    }
    if raus.is_empty() {
        return Err(KopfFehler::Abgeschnitten {
            braucht: i + 4,
            da: d.len(),
        });
    }
    Ok(raus)
}

/// Warum ein Kopf nicht lesbar oder schreibbar ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KopfFehler {
    /// Der Puffer endet mitten im Kopf.
    Abgeschnitten { braucht: usize, da: usize },
    /// Eine Connection-ID war laenger als 20 Byte.
    CidZuLang { ist: usize, maximum: usize },
    /// Ein Token war laenger als die harte Grenze.
    TokenZuLang { ist: usize, maximum: usize },
    /// Die Paketnummernlaenge ist nicht 1..=4.
    Paketnummernlaenge { ist: usize },
    /// Ein Varint-Feld war kaputt.
    Varint(VarintFehler),
    /// Das Length-Feld kündigt mehr an, als der Puffer hergibt.
    LaengeZuGross { angegeben: u64, da: usize },
    /// Ein reserviertes Bit war nach dem Kopfschutz **nicht** null
    /// (RFC 9000 Abschnitt 17.2).
    ///
    /// WO DIE PRUEFUNG STEHEN MUSS, und nicht in `kurze_lesen`/`lange_lesen`:
    /// **in `schutz::entschuetzen`, nach dem Entmasken.** Grund: die
    /// Kopf-Leser werden mit **geschuetzten** Bytes aufgerufen — sie muessen
    /// es, denn sie liefern das `pn_offset`, das man fuer das Sample braucht,
    /// und das Sample liegt hinter der Entschluesselung. Eine Pruefung dort
    /// haette **jedes** Paket verworfen; genau das ist beim ersten Versuch
    /// passiert (`test die_pruefung_gehoert_nach_den_kopfschutz` hat es
    /// gemeldet, und der Test ist deshalb geblieben).
    ///
    /// DER FEHLER, DEN DIESE VARIANTE FUHRT (U2-Rest, 01.10.2026): die
    /// Dekodierer haben diese Bits nie geprueft. Wir haben unsere **eigenen**
    /// Pakete geprueft (die stimmen), aber ein **Server** prueft sie bei jedem
    /// empfangenen Paket und schliesst die Verbindung mit
    /// PROTOCOL_VIOLATION. Ein Dekodierer, der sie nicht prueft, nimmt also
    /// Pakete an, die kein Server akzeptieren wuerde.
    ///
    /// DIE BITLAGEN, woertlich aus RFC 9000. Und sie sind **verschieden**:
    ///
    ///   Abschnitt 17.3 (Short): "Reserved Bits: Two bits (those with a mask
    ///   of 0x18) of byte 0 are reserved for future use."
    ///
    ///   Abschnitt 17.2 (Long):  "Reserved Bits: Two bits (those with a mask
    ///   of 0x0c) of byte 0 are reserved across multiple packet types."
    ///
    ///   Also  Short 0x18,  Long **0x0c**.
    ///
    /// DER FEHLER, DEN I HIER GEMACHT HABE: Ich hatte zuerst 0x18 fuer
    /// **beide** gesetzt. Der Test `eigene_koepfe_haben_keine_reservierten_bits`
    /// hat es sofort gemeldet, und zwar an `typ=1, pn_laenge=1`, also an
    /// 0-RTT:  `0xc0 | (1 << 4) | 0 = 0xd0`, und `0xd0 & 0x18 = 0x10`.
    ///
    /// Der Schreibweg war **richtig** — `0xc0 | (typ << 4) | (pn_laenge - 1)`
    /// setzt die Typbits auf 0x30 und laesst 0x0c frei. Nur meine Pruefung war
    /// falsch, und sie haette **jedes** 0-RTT-Paket einesServers abgewiesen.
    ///
    /// DIE REGEL: Wer die beiden Masken verwechselt, prueft die falschen Bits
    /// und haelt sich fuer konform. Genau das ist in meinem eigenen
    /// Testkommentar vorhergesagt worden — und dann passiert.
    ///
    /// `wert` ist der **beobachtete** Anteil, nicht das ganze Byte — sonst
    /// muesste man im Fehlertext nachrechnen, welches Bit es war.
    ReserviertesBit { bit: u8, wert: u8 },
}

impl std::fmt::Display for KopfFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KopfFehler::Abgeschnitten { braucht, da } => {
                write!(f, "Kopf braucht {braucht} Byte, da sind {da}")
            }
            KopfFehler::CidZuLang { ist, maximum } => {
                write!(f, "Connection-ID hat {ist} Byte, erlaubt sind {maximum}")
            }
            KopfFehler::TokenZuLang { ist, maximum } => {
                write!(f, "Token hat {ist} Byte, erlaubt sind {maximum}")
            }
            KopfFehler::Paketnummernlaenge { ist } => {
                write!(f, "Paketnummernlaenge {ist} ist nicht 1..=4")
            }
            KopfFehler::Varint(e) => write!(f, "Varint im Kopf: {e}"),
            KopfFehler::LaengeZuGross { angegeben, da } => {
                write!(f, "Length-Feld sagt {angegeben} Byte, da sind {da}")
            }
            KopfFehler::ReserviertesBit { bit, wert } => {
                write!(
                    f,
                    "reservierte Bits 0x{bit:02x} sind 0x{wert:02x}, muessen 0 sein"
                )
            }
        }
    }
}

impl std::error::Error for KopfFehler {}

impl From<VarintFehler> for KopfFehler {
    fn from(e: VarintFehler) -> Self {
        KopfFehler::Varint(e)
    }
}

/// Ein geparster Long Header (Initial, 0-RTT oder Handshake).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangerKopf {
    /// Long-Packet-Typ (0, 1 oder 2). Retry (3) wird separat behandelt.
    pub typ: u8,
    pub version: u32,
    pub dcid: Vec<u8>,
    pub scid: Vec<u8>,
    /// Token, nur bei Initial-Paketen gesetzt.
    pub token: Vec<u8>,
    /// Der Wert des Length-Feldes (Paketnummer + Nutzlast).
    pub laenge: u64,
    /// Paketnummernlaenge in Byte (1..=4).
    pub pn_laenge: usize,
    /// Offset der Paketnummer im Datagramm.
    pub pn_offset: usize,
    /// Bytes bis einschliesslich Paketnummer (Laenge des AAD).
    pub header_laenge: usize,
}

/// Ein geparster Short Header (1-RTT).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KurzerKopf {
    pub spin: bool,
    pub key_phase: bool,
    pub pn_laenge: usize,
    /// Laenge der Connection-ID (aus dem Verbindungszustand, nicht im Kopf).
    pub dcid_laenge: usize,
    pub pn_offset: usize,
    pub header_laenge: usize,
}

/// Baut das erste Byte eines Long Headers (Header-Form 1, Fixed 1, Typ,
/// reservierte Bits 0, Paketnummernlaenge).
fn langes_erstes_byte(typ: u8, pn_laenge: usize) -> Result<u8, KopfFehler> {
    if !(1..=4).contains(&pn_laenge) {
        return Err(KopfFehler::Paketnummernlaenge { ist: pn_laenge });
    }
    Ok(0xc0 | ((typ & 0x03) << 4) | ((pn_laenge - 1) as u8))
}

/// Baut das erste Byte eines Short Headers (Header-Form 0, Fixed 1, Spin,
/// reservierte Bits 0, Key Phase, Paketnummernlaenge).
fn kurzes_erstes_byte(spin: bool, key_phase: bool, pn_laenge: usize) -> Result<u8, KopfFehler> {
    if !(1..=4).contains(&pn_laenge) {
        return Err(KopfFehler::Paketnummernlaenge { ist: pn_laenge });
    }
    let mut b = 0x40u8;
    if spin {
        b |= 0x20;
    }
    if key_phase {
        b |= 0x04;
    }
    b |= (pn_laenge - 1) as u8;
    Ok(b)
}

/// Schiebt die unteren `pn_laenge` Byte der Paketnummer (big-endian) in den
/// Puffer — genau so, wie QUIC die Paketnummer kuerzt.
fn paketnummer_schreiben(aus: &mut Vec<u8>, pn: u64, pn_laenge: usize) {
    let pn_bytes = pn.to_be_bytes();
    aus.extend_from_slice(&pn_bytes[8 - pn_laenge..]);
}

/// Schreibt einen Long Header (ohne Nutzlast). Liefert `(bytes, pn_offset)`.
///
/// `nutzlast_laenge` ist die Klartextlaenge der Nutzlast (ohne Tag); das
/// Length-Feld zaehlt Paketnummer + Chiffrat + Tag.
///
/// Viele Argumente sind Absicht: ein QUIC-Kopf hat genau diese Skalarfelder,
/// und sie gebuendelt zu uebergeben wuerde nur eine zusaetzliche Struktur
/// ohne eigenen Sinn erzeugen.
#[allow(clippy::too_many_arguments)]
pub fn lange_schreiben(
    typ: u8,
    version: u32,
    dcid: &[u8],
    scid: &[u8],
    token: &[u8],
    pn: u64,
    pn_laenge: usize,
    nutzlast_laenge: usize,
) -> Result<(Vec<u8>, usize), KopfFehler> {
    if dcid.len() > MAX_CID_LAENGE {
        return Err(KopfFehler::CidZuLang {
            ist: dcid.len(),
            maximum: MAX_CID_LAENGE,
        });
    }
    if scid.len() > MAX_CID_LAENGE {
        return Err(KopfFehler::CidZuLang {
            ist: scid.len(),
            maximum: MAX_CID_LAENGE,
        });
    }
    if typ != TYP_INITIAL && !token.is_empty() {
        // Nur Initial traegt ein Token; hier nie ein leeres erzwingen.
        return Err(KopfFehler::TokenZuLang {
            ist: token.len(),
            maximum: 0,
        });
    }
    if token.len() > MAX_TOKEN_LAENGE {
        return Err(KopfFehler::TokenZuLang {
            ist: token.len(),
            maximum: MAX_TOKEN_LAENGE,
        });
    }

    let mut kopf = Vec::new();
    kopf.push(langes_erstes_byte(typ, pn_laenge)?);
    kopf.extend_from_slice(&version.to_be_bytes());
    kopf.push(dcid.len() as u8);
    kopf.extend_from_slice(dcid);
    kopf.push(scid.len() as u8);
    kopf.extend_from_slice(scid);

    if typ == TYP_INITIAL {
        varint::schreibe(token.len() as u64, &mut kopf)?;
        kopf.extend_from_slice(token);
    }

    // Length-Feld: Paketnummer + Chiffrat (Klartext + 16 Byte Tag).
    let laenge = (pn_laenge as u64) + (nutzlast_laenge as u64) + 16;
    varint::schreibe(laenge, &mut kopf)?;

    let pn_offset = kopf.len();
    paketnummer_schreiben(&mut kopf, pn, pn_laenge);
    Ok((kopf, pn_offset))
}

/// Liest einen Long Header ab Byte 0. Der Header-Schutz darf noch NICHT
/// entfernt sein — die Paketnummernlaenge steht hier in den geschuetzten
/// Bits, wird aber erst nach dem Entschuetzen gebraucht. `pn_offset` und
/// `header_laenge` werden trotzdem korrekt geliefert, weil sie nicht von der
/// Paketnummernlaenge abhaengen.
pub fn lange_lesen(daten: &[u8]) -> Result<LangerKopf, KopfFehler> {
    // Erstes Byte + Version + DCID-Len + SCID-Len = 1 + 4 + 1 + 1 = 7.
    if daten.len() < 7 {
        return Err(KopfFehler::Abgeschnitten {
            braucht: 7,
            da: daten.len(),
        });
    }
    let erstes = daten[0];
    if erstes & 0x80 == 0 {
        return Err(KopfFehler::Abgeschnitten { braucht: 1, da: 0 });
    }
    // DIE RESERVIERTEN BITS WERDEN HIER **NICHT** GEPRUEFT, und das ist
    // richtig so.
    //
    // DER GRUND: `lange_lesen` wird von `schutz::entschuetzen` mit
    // **geschuetzten** Bytes aufgerufen — es liefert das `pn_offset`, ohne das
    // das Header-Schutz-Sample nicht zu finden ist. Vor dem Entschluesselen
    // sind die Bits maskiert, also zufaellig.
    //
    // GEMESSEN, als die Pruefung hier stand (U2-Bericht N10, "Falle 2"):
    // `entschuetzen` meldete `ReserviertesBit` fuer **jedes** eigene Paket und
    // fuer ein **echtes** Paket von cloudflare-quic.com. Sie steht jetzt in
    // `entschuetzen` selbst, **nach** dem Entmasken, hinter der Konstante
    // `schutz::PRUEFE_RESERVIERTE_BITS` (derzeit `false`, aus einem
    // gemessenen Grund — siehe dort).
    //
    // Die Masken fuer sich stehen bei `KopfFehler::ReserviertesBit`:
    // Long 0x0c (17.2), Short 0x18 (17.3).
    let typ = (erstes >> 4) & 0x03;
    let version = u32::from_be_bytes([daten[1], daten[2], daten[3], daten[4]]);

    let dcid_len = daten[5] as usize;
    if dcid_len > MAX_CID_LAENGE {
        return Err(KopfFehler::CidZuLang {
            ist: dcid_len,
            maximum: MAX_CID_LAENGE,
        });
    }
    let mut pos = 6usize;
    let dcid_ende = pos + dcid_len;
    if dcid_ende > daten.len() {
        return Err(KopfFehler::Abgeschnitten {
            braucht: dcid_ende,
            da: daten.len(),
        });
    }
    let dcid = daten[pos..dcid_ende].to_vec();
    pos = dcid_ende;

    if pos >= daten.len() {
        return Err(KopfFehler::Abgeschnitten {
            braucht: pos + 1,
            da: daten.len(),
        });
    }
    let scid_len = daten[pos] as usize;
    pos += 1;
    if scid_len > MAX_CID_LAENGE {
        return Err(KopfFehler::CidZuLang {
            ist: scid_len,
            maximum: MAX_CID_LAENGE,
        });
    }
    let scid_ende = pos + scid_len;
    if scid_ende > daten.len() {
        return Err(KopfFehler::Abgeschnitten {
            braucht: scid_ende,
            da: daten.len(),
        });
    }
    let scid = daten[pos..scid_ende].to_vec();
    pos = scid_ende;

    // Token (nur Initial), dann Length-Feld.
    let mut token = Vec::new();
    if typ == TYP_INITIAL {
        let (token_len, token_feld_laenge) = varint::lese(&daten[pos..])?;
        if token_len as usize > MAX_TOKEN_LAENGE {
            return Err(KopfFehler::TokenZuLang {
                ist: token_len as usize,
                maximum: MAX_TOKEN_LAENGE,
            });
        }
        pos += token_feld_laenge;
        let token_ende = pos + token_len as usize;
        if token_ende > daten.len() {
            return Err(KopfFehler::Abgeschnitten {
                braucht: token_ende,
                da: daten.len(),
            });
        }
        token = daten[pos..token_ende].to_vec();
        pos = token_ende;
    }

    let (laenge, laenge_feld_laenge) = varint::lese(&daten[pos..])?;

    let pn_offset = pos + laenge_feld_laenge;
    // Paketnummernlaenge ist geschuetzt; hier nur ein Platzhalter (1), der
    // nach dem Entschuetzen ueberschrieben wird.
    let pn_laenge = ((erstes & 0x03) as usize) + 1;
    let header_laenge = pn_offset + pn_laenge;

    Ok(LangerKopf {
        typ,
        version,
        dcid,
        scid,
        token,
        laenge,
        pn_laenge,
        pn_offset,
        header_laenge,
    })
}

/// Schreibt einen Short Header. Liefert `(bytes, pn_offset)`.
pub fn kurze_schreiben(
    spin: bool,
    key_phase: bool,
    dcid: &[u8],
    pn: u64,
    pn_laenge: usize,
) -> Result<(Vec<u8>, usize), KopfFehler> {
    if dcid.len() > MAX_CID_LAENGE {
        return Err(KopfFehler::CidZuLang {
            ist: dcid.len(),
            maximum: MAX_CID_LAENGE,
        });
    }
    let mut kopf = Vec::new();
    kopf.push(kurzes_erstes_byte(spin, key_phase, pn_laenge)?);
    kopf.extend_from_slice(dcid);
    let pn_offset = kopf.len();
    paketnummer_schreiben(&mut kopf, pn, pn_laenge);
    Ok((kopf, pn_offset))
}

/// Liest einen Short Header ab Byte 0. `dcid_laenge` kommt aus dem
/// Verbindungszustand (der Short Header traegt sie nicht).
pub fn kurze_lesen(daten: &[u8], dcid_laenge: usize) -> Result<KurzerKopf, KopfFehler> {
    if daten.is_empty() {
        return Err(KopfFehler::Abgeschnitten { braucht: 1, da: 0 });
    }
    if dcid_laenge > MAX_CID_LAENGE {
        return Err(KopfFehler::CidZuLang {
            ist: dcid_laenge,
            maximum: MAX_CID_LAENGE,
        });
    }
    let erstes = daten[0];
    let pn_laenge = ((erstes & 0x03) as usize) + 1;
    let pn_offset = 1 + dcid_laenge;
    let header_laenge = pn_offset + pn_laenge;
    Ok(KurzerKopf {
        spin: erstes & 0x20 != 0,
        key_phase: erstes & 0x04 != 0,
        pn_laenge,
        dcid_laenge,
        pn_offset,
        header_laenge,
    })
}

/// Rekonstruiert die volle Paketnummer aus dem gekuerzten Feld
/// (RFC 9000, Anhang A.3, "Sample Packet Number Decoding Algorithm").
///
/// `abgeschnitten` ist der Wert aus den `pn_laenge` Byte des Kopfes,
/// `groesste` die groesste bisher empfangene Paketnummer in demselben
/// Nummernraum.
///
/// ## DIE DREI UNTERLAEUFE, DIE HIER STANDEN (01.10.2026, U1)
///
/// Der Kommentar an dieser Funktion behauptete "die Arithmetik ist so
/// formuliert, dass sie nie unterlaeuft". Das war **falsch**, und der Test
/// `paketnummer_rekonstruktion_haelt_die_grenzen_ein` in
/// `tests/groesse_und_fluss.rs` hat alle drei gefunden — mit `abgeschnitten =
/// u32::MAX` und `pn_laenge = 4`:
///
/// 1. `kandidat + halb` bei `kandidat` nahe `u64::MAX`,
/// 2. `(1u64 << 62) - fenster` ist unmoeglich, wenn `fenster > 1u64 << 62`,
///    also sobald `pn_laenge > 7` — und `pn_laenge` kommt aus einem
///    geschuetzten Bitfeld, ist also **nicht** auf 1..=4 begrenzt, sobald man
///    diese Funktion direkt aufruft,
/// 3. `erwartet + halb` mit `erwartet` aus `saturating_add`, also `u64::MAX`.
///
/// `groesste` und `abgeschnitten` sind **fremde Daten**: die Paketnummer steht
/// in einem Feld, das ein Angreifer mit dem richtigen Schluessel selbst
/// waehlen darf. Jede Panik hier ist ein Weg, den Client abzustuerzen (M3).
///
/// ## DIE GRENZEN, JETZT BENANNT
///
/// * `pn_laenge` wird auf 1..=4 begrenzt. Das ist die Vorgabe der Norm
///   (Abschnitt 17.2: "one to four bytes"); ein laengeres Feld ist kein
///   gueltiges Paket.
/// * `fenster` kann damit nie groesser als 2^32 sein.
/// * `erwartet` wird auf 2^62-1 begrenzt: die Paketnummer ist nach
///   Abschnitt 12.3 ein 62-Bit-Wert, und alles darueber ist bedeutungslos.
///   Ein Wert ausserhalb wird auf das groesste darunter zurueckgesetzt, was
///   eine **fortlaufende** Folge ergibt statt eines Sprungs.
pub fn paketnummer_rekonstruieren(abgeschnitten: u64, pn_laenge: usize, groesste: u64) -> u64 {
    // M3: `pn_laenge` aus fremden Daten auf den Normbereich begrenzen.
    let pn_laenge = pn_laenge.clamp(1, 4);
    let bits = pn_laenge * 8;
    let fenster = 1u64 << bits;
    let halb = fenster >> 1;
    let maske = fenster - 1;
    // M3: `groesste` ist eine 62-Bit-Paketnummer. Alles darueber ist nach
    // Abschnitt 12.3 kein gueltiger Wert mehr, und `saturating_add` wuerde
    // daraus `u64::MAX` machen — dann laeuft `erwartet + halb` ueber.
    let grenze = (1u64 << 62) - 1;
    let groesste = groesste.min(grenze);
    let erwartet = groesste.saturating_add(1).min(grenze);
    let mut kandidat = (erwartet & !maske) | (abgeschnitten & maske);
    //
    // DIE BEIDEN BEDINGUNGEN, ohne Ueberlauf (RFC 9000 Anhang A.3):
    //
    //   1. `candidate_pn <= expected_pn - pn_hwin`   — der Kandidat ist zu klein,
    //   2. `candidate_pn >  expected_pn + pn_hwin`   — der Kandidat ist zu gross.
    //
    // Geschrieben wird jede als **Differenz** mit `saturating_sub`, und jede in
    // der richtigen Richtung:
    //
    // * Bedingung 1 braucht `erwartet - kandidat >= halb` (nicht umgekehrt!).
    // * Bedingung 2 braucht `kandidat - erwartet >  halb`.
    //
    // DER FEHLER, DEN DER TEST HIER GEFANGEN HAT: zuerst stand
    // `differenz = kandidat - erwartet` und dann `differenz <= halb` fuer
    // Bedingung 1. Das ist die **falsche Richtung** und bricht das
    // Pflichtbeispiel aus RFC 9000 Anhang A.3: bei `kandidat = 0xa82f9b32` und
    // `erwartet = 0xa82f30eb` ist `kandidat - erwartet = 0x6a47`, was `<= halb`
    // erfuellt, also sprang der Kandidat ein Fenster **nach oben** und das
    // Ergebnis war `0xa82f3532` statt `0xa82f9b32`. Der Test in
    // `kopf.rs::tests::paketnummer_rekonstruktion` hat es gemeldet, weil er das
    // Pflichtbeispiel aus dem RFC enthaelt.
    //
    // Die Richtung ist nicht Geschmack: Bedingung 1 sagt "der Kandidat liegt
    // **unter** der Erwartung, um mehr als ein halbes Fenster" — dafuer muss man
    // von der Erwartung abwaerts messen.
    let unter_erwartung = erwartet.saturating_sub(kandidat);
    let ueber_erwartung = kandidat.saturating_sub(erwartet);
    // `kandidat >= fenster` ist aequivalent zu "es gibt ein Fenster darunter,
    // um herunterzugehen" — und braucht keine Subtraktion.
    if unter_erwartung >= halb && kandidat < grenze.saturating_sub(fenster) {
        kandidat += fenster;
    } else if ueber_erwartung > halb && kandidat >= fenster {
        // `kandidat -= fenster` ist jetzt sicher: `kandidat >= fenster` ist
        // oben geprueft, also ist die Subtraktion nicht negativ.
        kandidat -= fenster;
    }
    kandidat.min(grenze)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9001, Anhang A.2: der Client-Initial-Kopf, ungeschuetzt und
    /// geschuetzt, byte-genau.
    #[test]
    fn client_initial_kopf_aus_rfc9001_a2() {
        let (kopf, pn_offset) = lange_schreiben(
            TYP_INITIAL,
            1,
            &[0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08],
            &[],
            &[],
            2,
            4,
            1162,
        )
        .expect("Kopf bauen");
        // c3 00000001 08 <dcid> 00 00 449e 00000002
        let mut erwartet = hex("c300000001088394c8f03e5157080000449e");
        erwartet.extend_from_slice(&[0x00, 0x00, 0x00, 0x02]);
        assert_eq!(kopf, erwartet, "M7: ungeschuetzter Kopf");
        assert_eq!(pn_offset, 18, "M7: pn_offset");

        // Und zurueck: lesen liefert dieselben Felder.
        let g = lange_lesen(&kopf).expect("Kopf lesen");
        assert_eq!(g.typ, TYP_INITIAL);
        assert_eq!(g.version, 1);
        assert_eq!(g.dcid, vec![0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08]);
        assert!(g.scid.is_empty());
        assert!(g.token.is_empty());
        assert_eq!(g.laenge, 1182);
        assert_eq!(g.pn_offset, 18);
        assert_eq!(g.pn_laenge, 4);
    }

    /// RFC 9001, Anhang A.3: der Server-Initial-Kopf.
    #[test]
    fn server_initial_kopf_aus_rfc9001_a3() {
        let (kopf, pn_offset) = lange_schreiben(
            TYP_INITIAL,
            1,
            &[],
            &[0xf0, 0x67, 0xa5, 0x50, 0x2a, 0x42, 0x62, 0xb5],
            &[],
            1,
            2,
            99,
        )
        .expect("Kopf bauen");
        let mut erwartet = hex("c1000000010008f067a5502a4262b5004075");
        erwartet.extend_from_slice(&[0x00, 0x01]);
        assert_eq!(kopf, erwartet, "M7: ungeschuetzter Kopf");
        assert_eq!(pn_offset, 18, "M7: pn_offset");
    }

    /// RFC 9001, Anhang A.5: der Short-Header (ChaCha20, Paketnummer 3 Byte).
    #[test]
    fn short_header_aus_rfc9001_a5() {
        let (kopf, pn_offset) = kurze_schreiben(false, false, &[], 654_360_564, 3).expect("Kopf");
        assert_eq!(
            narsil_tls::sha2::hexen(&kopf),
            "4200bff4",
            "M7: ungeschuetzter Kopf 4200bff4"
        );
        assert_eq!(pn_offset, 1, "M7: pn_offset");

        let g = kurze_lesen(&kopf, 0).expect("Kopf lesen");
        assert_eq!(g.pn_laenge, 3);
        assert_eq!(g.pn_offset, 1);
        assert!(!g.spin && !g.key_phase);
    }

    /// M3/M4: ein Kopf mit einer Connection-ID von 21 Byte wird abgelehnt.
    #[test]
    fn cid_ueber_20_byte_wird_abgelehnt() {
        let lang = [0xabu8; 21];
        assert!(matches!(
            lange_schreiben(TYP_INITIAL, 1, &lang, &[], &[], 0, 1, 0),
            Err(KopfFehler::CidZuLang {
                ist: 21,
                maximum: 20
            })
        ));
        assert!(matches!(
            kurze_schreiben(false, false, &lang, 0, 1),
            Err(KopfFehler::CidZuLang {
                ist: 21,
                maximum: 20
            })
        ));
        // Leseseite: ein 21-Byte-Laengenfeld.
        let mut daten = vec![0xc3, 0, 0, 0, 1, 21];
        daten.extend_from_slice(&lang);
        assert!(matches!(
            lange_lesen(&daten),
            Err(KopfFehler::CidZuLang {
                ist: 21,
                maximum: 20
            })
        ));
    }

    /// M4: abgeschnittene Koepfe enden mit benanntem Fehler, nie mit Panic.
    #[test]
    fn abgeschnittene_koepfe() {
        // Weniger als 7 Byte.
        for n in 0..7 {
            assert!(
                matches!(
                    lange_lesen(&vec![0xc3u8; n]),
                    Err(KopfFehler::Abgeschnitten { .. })
                ),
                "Long-Header mit {n} Byte"
            );
        }
        // Leerer Short-Header.
        assert!(matches!(
            kurze_lesen(&[], 0),
            Err(KopfFehler::Abgeschnitten { .. })
        ));
    }

    /// M3/M7: die Paketnummern-Rekonstruktion aus RFC 9000, Anhang A.3.
    #[test]
    fn paketnummer_rekonstruktion() {
        // Woertlich aus RFC 9000, Anhang A.3: "if the highest successfully
        // authenticated packet had a packet number of 0xa82f30ea, then a
        // packet containing a 16-bit value of 0x9b32 will be decoded as
        // 0xa82f9b32."
        let ergebnis = paketnummer_rekonstruieren(0x9b32, 2, 0xa82f30ea);
        assert_eq!(ergebnis, 0xa82f9b32, "M7: RFC-9000-A.3-Beispiel");

        // Triviale Faelle: groesste = 0, kleiner Wert.
        assert_eq!(paketnummer_rekonstruieren(2, 4, 0), 2);
        // Wickel ueber die Fenstergrenze (nach oben): 0xab, dann 0x00 ->
        // 0x100, weil 0 unter der halben Fenstergroesse liegt.
        assert_eq!(paketnummer_rekonstruieren(0, 1, 0xab), 0x100);
        // Grenze: gekuerzter Wert direkt beim erwarteten.
        assert_eq!(paketnummer_rekonstruieren(5, 1, 4), 5);
    }

    fn hex(s: &str) -> Vec<u8> {
        narsil_tls::sha2::von_hex(s).expect("Hex im Test")
    }
}
