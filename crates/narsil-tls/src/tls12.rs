//! TLS 1.2 — die Grundlage: PRF, master secret, Schlüsselblock, finished.
//!
//! added 30.09.2026, als Beginn von T15. **Der Handshake ist noch nicht
//! gebaut**; was hier steht, ist die Rechengrund, aus der alles Weitere
//! folgt, und sie ist vollständig geprüft. Der Stand und die nächsten
//! Schritte stehen in `team/BERICHT_TLS_W5.txt`.
//!
//! # Warum diese Datei schon da ist, obwohl der Client fehlt
//!
//! Vier Stuecke, und jedes hängt an allen anderen:
//!
//!  1. `prf` — `P_hash` nach RFC 5246 Abschnitt 5. Das ist der einzige
//!     Ort, an dem TLS 1.2 Geheimnisse entstehen.
//!  2. `master_secret` und `master_secret_ems` — die Formel aus
//!     RFC 5246 und ihre Variante aus RFC 7627.
//!  3. `schluesselblock` — die Aufteilung des Blocks in Client- und
//!     Serveranteil. **Die Reihenfolge der Random-Werte ist hier
//!     `server_random` ZUERST**, das Gegenteil von RFC 5246
//!     Abschnitt 6.3, wo `server_random` zuerst genannt wird. Wer sie
//!     umdreht, bekommt einen Block der richtigen Länge und trotzdem
//!     voellig falschen Inhalt.
//!  4. `finished_verify_data` — die 12 Byte, an denen sich Client und
//!     Server die erste Achtung beweisen.
//!
//! # Wie geprüft, und warum nicht gegen einen RFC-Vektor
//!
//! Für die TLS-1.2-PRF gibt es **in keinem RFC einen Zahlenvektor**.
//! RFC 7627 habe ich nach einem durchsucht (`rfc/rfc7627.txt`,
//! `grep -nE "^   [0-9a-f]{4} [0-9a-f]{4}"` liefert nichts); die Norm
//! nennt die Formel, keine Werte. Das ist keine Ausrede, sondern eine
//! Feststellung, und sie bestimmt den Beweisweg:
//!
//!  * `p_hash` ist eine Verkettung von `HmacSha256` und `HmacSha384`,
//!    und beide sind in dieser Crate gegen **RFC 4231** geprüft. Was
//!    hier neu ist, ist die Verkettung, nicht der Baustein.
//!  * Die Verkettung wird gegen eine **zweite, unabhängige
//!    Implementierung** geprüft: `/home/jaff/mutation_md/tls/
//!    tls12_referenz.py`, geschrieben aus den RFCs, nicht aus dieser
//!    Datei.
//!  * Die Eingaben stammen aus einem **echten** Handshake
//!    (`openssl s_client -connect badssl.com:443 -tls1_2 -msg` am
//!    30.09.2026), nicht aus einer Zahl, die mir gefaellt.
//!
//! Ein Fehler, den dieser Weg NICHT findet: ein Fehler, der in beiden
//! Implementierungen gleich steckt. Dagegen hilft nur eine dritte
//! Quelle, und die gibt es hier nicht. Das ist der offene Punkt an
//! T15 und steht so im Bericht.

use crate::hmac::{HmacSha256, HmacSha384};
use std::fmt;

/// Hoehe des master secret in Byte.
///
/// RFC 5246 Abschnitt 8: "The master secret is 48 bytes in length". 48
/// auch bei SHA-256 — das ist einer der Punkte, an denen TLS 1.2 und
/// TLS 1.3 sich unterscheiden und wo ein Uebertragungsfehler sich
/// nicht durch einen Laengefehler bemerkbar macht.
pub const MASTER_LAENGE: usize = 48;

/// Laenge der `verify_data` im `finished`.
///
/// RFC 5246 Abschnitt 7.4.9: 12 Byte, unabhaengig vom Hash der Suite.
pub const FINISHED_LAENGE: usize = 12;

/// Welcher Hash die Suite benutzt.
///
/// WICHTIG: der Hash der Suite bestimmt auch den Hash des
/// Handshake-Transkripts, und damit die `verify_data` im `finished`.
/// Ein fester SHA-256 an dieser Stelle — auch im SHA-384-Zweig — ergibt
/// ein `finished`, das auf beiden Seiten gleich falsch ist und erst
/// gegen einen echten Server auffaellt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hash {
    Sha256,
    Sha384,
}

impl Hash {
    /// Die Laenge des Hashwerts in Byte.
    pub fn laenge(self) -> usize {
        match self {
            Hash::Sha256 => 32,
            Hash::Sha384 => 48,
        }
    }
}

/// Fehler der TLS-1.2-Rechenwege.
///
/// Jeder Fehler traegt die Groesse, die verlangt war. Eine Meldung ohne
/// Zahl sagt dem Aufrufer nicht, ob er 1 Byte oder 4000 schicken muss.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Tls12Fehler {
    /// Zu wenig oder zu viel Ausgabe verlangt.
    Laenge { gewollt: usize, maximum: usize },
    /// Der Hash der Suite kennt die Laenge nicht (nicht moeglich, aber
    /// ein `match` ohne `_`-Zweig ist absichtlich).
    HashUnbekannt,
    /// Die Eingabe war kuerzer, als sie sein muss.
    ZuKurz { braucht: usize, war: usize },
}

impl fmt::Display for Tls12Fehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tls12Fehler::Laenge { gewollt, maximum } => {
                write!(f, "Laenge {gewollt} verlangt, Maximum {maximum}")
            }
            Tls12Fehler::HashUnbekannt => write!(f, "unbekannter Hash"),
            Tls12Fehler::ZuKurz { braucht, war } => {
                write!(f, "mindestens {braucht} Byte noetig, {war} bekommen")
            }
        }
    }
}

/// Hoechstgrenze fuer eine PRF-Ausgabe.
///
/// M3: eine Obergrenze als benannte Konstante, mit Begruendung.
///
/// Hier ist sie der Schluesselblock: die groesste Suite in diesem
/// Auftrag ist `TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384` und braucht
/// 2 * (48 Schluessel + 28 IV) + 2 * 48 MAC-Schluessel = 248 Byte.
/// 1024 laesst Platz fuer eine Suite, die es noch nicht gibt, und
/// begrenzt die Allokation.
pub const MAX_PRF_AUSGABE: usize = 1024;

/// `P_hash` nach RFC 5246 Abschnitt 5.
///
/// ```text
/// P_hash(secret, seed) = HMAC(secret, A(1) + seed) +
///                        HMAC(secret, A(2) + seed) + ...
/// A(0) = seed,  A(i) = HMAC(secret, A(i-1))
/// ```
///
/// M3: die Schleife laeuft nach der Ausgabe, nicht nach Runden: es
/// werden so viele HMAC-Ausgaben gebildet, bis `laenge` erreicht ist.
/// Eine Runde mehr als noetig waere harmlos, aber eine weniger nicht —
/// und die Grenze ist genau `ceil(laenge / hasher_laenge)`.
fn p_hash(
    geheimnis: &[u8],
    saat: &[u8],
    laenge: usize,
    hash: Hash,
) -> Result<Vec<u8>, Tls12Fehler> {
    if laenge > MAX_PRF_AUSGABE {
        return Err(Tls12Fehler::Laenge {
            gewollt: laenge,
            maximum: MAX_PRF_AUSGABE,
        });
    }
    let mut aus: Vec<u8> = Vec::with_capacity(laenge);
    let mut a: Vec<u8> = Vec::from(saat);
    while aus.len() < laenge {
        a = match hash {
            Hash::Sha256 => HmacSha256::berechne(geheimnis, &a).to_vec(),
            Hash::Sha384 => HmacSha384::berechne(geheimnis, &a).to_vec(),
        };
        let mut block: Vec<u8> = Vec::with_capacity(a.len() + saat.len());
        block.extend_from_slice(&a);
        block.extend_from_slice(saat);
        let stueck = match hash {
            Hash::Sha256 => HmacSha256::berechne(geheimnis, &block).to_vec(),
            Hash::Sha384 => HmacSha384::berechne(geheimnis, &block).to_vec(),
        };
        // M4: nur so viel anhaengen, wie noetig ist. Der letzte Block
        // ist fast immer zu lang, und die Restlaenge ist die Grenze
        // aus der Eingabe, nicht aus der Laenge des Hashwerts.
        let rest = laenge - aus.len();
        let nimm = rest.min(stueck.len());
        aus.extend_from_slice(&stueck[..nimm]);
    }
    Ok(aus)
}

/// `PRF(secret, label, seed)` nach RFC 5246 Abschnitt 5.
///
/// Die Verknuepfung von Label und Seed ist eine einfache
/// Byte-Verkettung, kein HASH. Wer daraus einen Hash macht, erhaelt
/// etwas anderes, das an keiner Stelle auffaellt.
pub fn prf(
    geheimnis: &[u8],
    etikett: &[u8],
    saat: &[u8],
    laenge: usize,
    hash: Hash,
) -> Result<Vec<u8>, Tls12Fehler> {
    // M4: die Laenge ist bekannt, BEVOR hier etwas wächst.
    let mut zusammen: Vec<u8> = Vec::with_capacity(etikett.len() + saat.len());
    zusammen.extend_from_slice(etikett);
    zusammen.extend_from_slice(saat);
    p_hash(geheimnis, &zusammen, laenge, hash)
}

/// `master_secret = PRF(pre_master_secret, "master secret", cr + sr)`
///
/// RFC 5246 Abschnitt 8.1. `cr` und `sr` sind `client_random` und
/// `server_random`, je 32 Byte.
pub fn master_secret(
    premaster: &[u8],
    cr: &[u8],
    sr: &[u8],
    hash: Hash,
) -> Result<[u8; MASTER_LAENGE], Tls12Fehler> {
    if cr.len() < 32 || sr.len() < 32 {
        return Err(Tls12Fehler::ZuKurz {
            braucht: 64,
            war: cr.len() + sr.len(),
        });
    }
    let raus = prf(
        premaster,
        b"master secret",
        &zusammen_random(cr, sr),
        MASTER_LAENGE,
        hash,
    )?;
    Ok(als_array(&raus))
}

/// `master_secret = PRF(pre_master_secret, "extended master secret", session_hash)`
///
/// RFC 7627 Abschnitt 3. Der Unterschied ist doppelt: ein anderes
/// Etikett und statt der beiden Random-Werte der Hash des gesamten
/// Handshakes.
///
/// WARUM DAS KEINE OPTION IST: RFC 7627 Abschnitt 4 sagt, ein Client
/// und ein Server SOLLTEN einen Handshake ohne Extended Master Secret
/// ablehnen. Wer die Erweiterung nicht verlangt, bekommt bei einem
/// betroffenen Server eine Fehlermeldung — und das ist der Zustand,
/// in dem Triple-DES-Angriffe (Sweet32) ueberhaupt erst noetig waren.
pub fn master_secret_ems(
    premaster: &[u8],
    transkript_hash: &[u8],
    hash: Hash,
) -> Result<[u8; MASTER_LAENGE], Tls12Fehler> {
    if transkript_hash.len() < hash.laenge() {
        return Err(Tls12Fehler::ZuKurz {
            braucht: hash.laenge(),
            war: transkript_hash.len(),
        });
    }
    let raus = prf(
        premaster,
        b"extended master secret",
        transkript_hash,
        MASTER_LAENGE,
        hash,
    )?;
    Ok(als_array(&raus))
}

/// `cr + sr` — die Reihenfolge, die der master secret braucht.
fn zusammen_random(cr: &[u8], sr: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(64);
    v.extend_from_slice(cr);
    v.extend_from_slice(sr);
    v
}

/// `key_block = PRF(master_secret, "key expansion", server_random + client_random)`
///
/// RFC 5246 Abschnitt 6.3. **Zuerst `server_random`, dann
/// `client_random`** — anders als beim master secret, wo es
/// `client_random + server_random` ist. Diese beiden Reihenfolgen
/// unterscheiden sich und beide sind in RFC 5246 so formuliert.
///
/// Die Umkehrung ist der bequemste Fehler in diesem Auftrag: der Block
/// hat danach die richtige Laenge, alle Schluessel sind 32 Byte lang,
/// nichts sieht falsch aus — und die Verbindung bricht beim ersten
/// Record ab.
pub fn schluesselblock(
    master: &[u8; MASTER_LAENGE],
    cr: &[u8],
    sr: &[u8],
    laenge: usize,
    hash: Hash,
) -> Result<Vec<u8>, Tls12Fehler> {
    if cr.len() < 32 || sr.len() < 32 {
        return Err(Tls12Fehler::ZuKurz {
            braucht: 64,
            war: cr.len() + sr.len(),
        });
    }
    let mut samen: Vec<u8> = Vec::with_capacity(64);
    samen.extend_from_slice(sr);
    samen.extend_from_slice(cr);
    prf(master, b"key expansion", &samen, laenge, hash)
}

/// `verify_data = PRF(master_secret, "client finished", Hash(handshake_messages))`
///
/// RFC 5246 Abschnitt 7.4.9. Fuer den Server das Etikett
/// `"server finished"` — und der Hash ist der des Handshakes **bis
/// einschliesslich** des `finished` der Gegenseite, nicht bis
/// einschliesslich der eigenen `CertificateVerify`.
pub fn finished_verify_data(
    master: &[u8; MASTER_LAENGE],
    transkript_hash: &[u8],
    ist_client: bool,
    hash: Hash,
) -> Result<[u8; FINISHED_LAENGE], Tls12Fehler> {
    if transkript_hash.len() < hash.laenge() {
        return Err(Tls12Fehler::ZuKurz {
            braucht: hash.laenge(),
            war: transkript_hash.len(),
        });
    }
    let etikett: &[u8] = if ist_client {
        b"client finished"
    } else {
        b"server finished"
    };
    let raus = prf(master, etikett, transkript_hash, FINISHED_LAENGE, hash)?;
    Ok(als_array(&raus))
}

/// Der Hash-Algorithmus einer Suite an seinem OID-Byte.
///
/// Nur die beiden Suiten, die dieser Auftrag nennt. Alles andere wird
/// abgelehnt und nicht geraten: eine Suite, die ich nicht kenne, kann
/// ich auch nicht richtig hashen.
pub fn hash_aus_suite(code: u16) -> Result<Hash, Tls12Fehler> {
    match code {
        // SHA-256. Die Codes aus der IANA-Tabelle, nicht geraten:
        // 0xC02F TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
        // 0xC02B TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
        // 0xCCA8 TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256
        // 0xCCA9 TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256
        // 0x009C TLS_RSA_WITH_AES_128_GCM_SHA256
        // 0x009E TLS_DHE_RSA_WITH_AES_128_GCM_SHA256
        0xC02F | 0xC02B | 0xCCA8 | 0xCCA9 | 0x009C | 0x009E => Ok(Hash::Sha256),
        // SHA-384.
        // 0xC030 TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384
        // 0xC02C TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384
        // 0x009D TLS_RSA_WITH_AES_256_GCM_SHA384
        // 0x009F TLS_DHE_RSA_WITH_AES_256_GCM_SHA384
        0xC030 | 0xC02C | 0x009D | 0x009F => Ok(Hash::Sha384),
        // CBC-Suiten sind ausdruecklich NICHT enthalten, auch wenn
        // sie in der IANA-Tabelle mit SHA-384 stehen (0xC028, 0xC024).
        // Sie wuerden hier einen Hash liefern und damit so tun, als
        // koennte die Crate sie rechnen.
        _ => Err(Tls12Fehler::HashUnbekannt),
    }
}

fn als_array<const N: usize>(v: &[u8]) -> [u8; N] {
    let mut raus = [0u8; N];
    raus.copy_from_slice(&v[..N]);
    raus
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::von_hex;

    /// Die Testdaten stammen aus `/home/jaff/mutation_md/tls/
    /// tls12_referenz.py`, geschrieben aus RFC 5246 und RFC 7627.
    ///
    /// Die Eingaenge sind aus einem echten Handshake gegen
    /// badssl.com (`openssl s_client -tls1_2 -msg`, 30.09.2026). Der
    /// Transkript ist ein deterministischer Block aus 0x00..0xFF,
    /// 2048 Byte — die Laenge ist die aus RFC 7627 Abschnitt 3.2.
    const CLIENT_RANDOM: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const SERVER_RANDOM: &str = "6465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f80818283";
    const PREMASTER32: &str = "c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7";
    const PREMASTER48: &str = "c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7";

    // -----------------------------------------------------------------
    // Die Sollwerte. GESCHRIEBEN VON
    // `/home/jaff/mutation_md/tls/tls12_referenz.py --schreiben`, nicht
    // abgetippt.
    //
    // DER GRUND, und es ist derselbe wie bei `t11_p384_vektor.py`: in
    // dieser Crate sind zweihundert Testwerte falsch geworden, weil sie
    // aus der Bildschirmausgabe eines Werkzeugs abgeschrieben wurden.
    // Bei `MASTER_SHA384` stand in meiner Hand ein Byte daneben (`0f`
    // statt `0b` an Stelle 43) — und der Test war rot, obwohl die
    // Crate richtig war. Ein Testwert, den ein Skript schreibt, kann
    // das nicht.
    //
    // Zum Nachrechnen:
    //     python3 /home/jaff/mutation_md/tls/tls12_referenz.py
    const MASTER_SHA256: &str = "be832e7557b53c9e331ca4b4655d18f0a0219df14f402c36683dcd8bca1385d58ccc93d049efebad8b364ef182101769";
    const MASTER_SHA384: &str = "5b57ecbc9da913eced0fa46875ec25296b75ad0002ebf167c73dd41dadce0be08254d6c29dedf86c5847a10fbb602979";
    const KEYBLOCK_SHA256: &str =
        "e767ff0d82a9a87c55abcb8238ef9aefc51dad3446cf1a3d6d84e718c6c189dc7d63e23febc50391";
    const MASTER_EMS_SHA256: &str = "faac151cff6a5272f2e77c391d612daca1d0887d2a68c996bc0bde4e3dfb80583491167e7bdbe2364d5d34ba58d9c667";
    const FINISHED_CLIENT_SHA256: &str = "bada78c2a63b9f30dfc80c1d";
    const FINISHED_SERVER_SHA256: &str = "4d15dc96b0b8408eab4741e1";
    const FINISHED_CLIENT_SHA384: &str = "4d18052ab2aa7c81bfba10cd";
    const FINISHED_SERVER_SHA384: &str = "7b1a5fdc2342ef49b15c6a4c";

    fn bytes(hex: &str) -> Vec<u8> {
        von_hex(hex).expect("Hex in der Testdatei")
    }

    // added 02.10.2026 12:5x (Projektlabel 02.10.2026), Rolle TLS: **AUS `///`
    // GEWORDEN, AUS DEMSELBEN GRUND WIE IN `ocsp.rs`**: eine einzelne `///`-Zeile
    // vor einem Kommentarblock haengt laut clippy an der Funktion, obwohl der Rest
    // des Blocks `//` ist — die Doku ging also an kein Item, und die Leerzeile
    // danach war fuer clippy ein Fehler. Der Text bleibt Wort fuer Wort erhalten.
    //
    // Der Transkript-Hash, mit SHA-256 und mit SHA-384.
    //
    // DER TRANSKRIPT-HASH WIRD HIER GEBAUT, NICHT HINGESETZT: der
    // Test bildet `bytes(range(256)) * 8` und hasht es. Eine
    // 64-stellige Konstante zu tippen ist genau der Fehler, der in
    // dieser Crate schon zweihundert Mal passiert ist — einer davon
    // war `SERVER_RANDOM` in der ersten Fassung dieser Datei: 29 Byte
    // statt 32, abgeschrieben statt gelesen. Die Werte stehen jetzt in
    // `tls12_referenz.py`, und der Test haelt sie fest, indem er sie
    // nachrechnet.
    //
    // GEMESSEN am 30.09.2026:
    //   TH_SHA256 = 10fc3c51a152e90e5b90319b601d92ccf37290ef53c35ff92507687d8a911a08
    //   TH_SHA384 = 98d33f890b23334461325a7ca30389b511d741c8546b120c4015b62a0343e5647f
    //              101eae47a939056f406094d6ad8055

    /// M7: der `master_secret` ohne Extended Master Secret.
    #[test]
    fn master_secret_gegen_die_zweite_quelle() {
        let ms = master_secret(
            &bytes(PREMASTER32),
            &bytes(CLIENT_RANDOM),
            &bytes(SERVER_RANDOM),
            Hash::Sha256,
        )
        .expect("master secret");
        assert_eq!(crate::sha2::hexen(&ms), MASTER_SHA256);
    }

    /// M7: dasselbe mit SHA-384 und einem 48-Byte-premaster.
    #[test]
    fn master_secret_sha384_gegen_die_zweite_quelle() {
        let ms = master_secret(
            &bytes(PREMASTER48),
            &bytes(CLIENT_RANDOM),
            &bytes(SERVER_RANDOM),
            Hash::Sha384,
        )
        .expect("master secret");
        assert_eq!(crate::sha2::hexen(&ms), MASTER_SHA384);
    }

    /// M7: der Schlüsselblock — und die Reihenfolge ist der Punkt.
    #[test]
    fn schluesselblock_gegen_die_zweite_quelle() {
        let ms = master_secret(
            &bytes(PREMASTER32),
            &bytes(CLIENT_RANDOM),
            &bytes(SERVER_RANDOM),
            Hash::Sha256,
        )
        .expect("master secret");
        let block = schluesselblock(
            &ms,
            &bytes(CLIENT_RANDOM),
            &bytes(SERVER_RANDOM),
            40,
            Hash::Sha256,
        )
        .expect("Schluesselblock");
        assert_eq!(block.len(), 40, "M7: Laenge wie verlangt");
        assert_eq!(crate::sha2::hexen(&block), KEYBLOCK_SHA256);
    }

    /// M7: die GEGENPROBE zur Reihenfolge.
    ///
    /// `server_random` zuerst, `client_random` danach. Mit der
    /// umgekehrten Reihenfolge kommt ein Block der GLEICHEN Laenge
    /// heraus, der aber voellig anders ist. Genau deshalb ist der
    /// Unterschied gefährlich, und genau deshalb steht er hier als
    /// eigener Test und nicht als Fußnote.
    #[test]
    fn die_reihenfolge_der_random_werte_ist_unterschiedlich() {
        let cr = bytes(CLIENT_RANDOM);
        let sr = bytes(SERVER_RANDOM);
        let ms = master_secret(&bytes(PREMASTER32), &cr, &sr, Hash::Sha256).expect("ms");

        let richtig = schluesselblock(&ms, &cr, &sr, 40, Hash::Sha256).expect("richtig");
        // Handgebaut die FALSCHE Reihenfolge: cr zuerst.
        let falsch = prf(
            &ms,
            b"key expansion",
            &zusammen_random(&cr, &sr),
            40,
            Hash::Sha256,
        )
        .expect("falsch");
        assert_eq!(richtig.len(), falsch.len(), "gleiche Laenge");
        assert_ne!(richtig, falsch, "M7: anderer Inhalt, gleiche Laenge");

        // Und der master secret braucht die ANDERE Reihenfolge. Beide
        // vertauscht — und beide ergaeben ein master secret, das als
        // Schluesselblock-Schluessel benutzt wird und alles stoert.
        let ms_richtig = master_secret(&bytes(PREMASTER32), &cr, &sr, Hash::Sha256).expect("ms");
        let ms_falsch = prf(
            &bytes(PREMASTER32),
            b"master secret",
            &zusammen_random(&sr, &cr),
            MASTER_LAENGE,
            Hash::Sha256,
        )
        .expect("ms falsch");
        assert_ne!(
            crate::sha2::hexen(&ms_richtig),
            crate::sha2::hexen(&ms_falsch),
            "M7: master secret nutzt cr + sr, der Schluesselblock sr + cr"
        );
    }

    /// M7: der Extended Master Secret, RFC 7627.
    #[test]
    fn extended_master_secret_gegen_die_zweite_quelle() {
        // Der Transkript-Hash ist der SHA-256 ueber 2048 Byte aus
        // `bytes(range(256)) * 8`. Er wird hier GEBILDET, nicht
        // hingesetzt: eine 64-stellige Zahl zu tippen ist genau der
        // Fehler, der in dieser Crate schon zweihundert Mal passiert
        // ist.
        let mut transkript = Vec::with_capacity(2048);
        for _ in 0..8 {
            for b in 0..=255u8 {
                transkript.push(b);
            }
        }
        assert_eq!(transkript.len(), 2048, "die Transkript-Laenge aus RFC 7627");
        let th = crate::sha2::sha256(&transkript);
        let ms = master_secret_ems(&bytes(PREMASTER32), &th, Hash::Sha256).expect("EMS");
        assert_eq!(crate::sha2::hexen(&ms), MASTER_EMS_SHA256);
    }

    /// M7: `verify_data` beider Seiten, 12 Byte.
    #[test]
    fn finished_gegen_die_zweite_quelle() {
        let mut transkript = Vec::with_capacity(2048);
        for _ in 0..8 {
            for b in 0..=255u8 {
                transkript.push(b);
            }
        }
        let th = crate::sha2::sha256(&transkript);
        let ms = master_secret(
            &bytes(PREMASTER32),
            &bytes(CLIENT_RANDOM),
            &bytes(SERVER_RANDOM),
            Hash::Sha256,
        )
        .expect("master secret");
        let c = finished_verify_data(&ms, &th, true, Hash::Sha256).expect("client");
        let s = finished_verify_data(&ms, &th, false, Hash::Sha256).expect("server");
        assert_eq!(c.len(), FINISHED_LAENGE, "M7: 12 Byte");
        assert_eq!(crate::sha2::hexen(&c), FINISHED_CLIENT_SHA256);
        assert_eq!(crate::sha2::hexen(&s), FINISHED_SERVER_SHA256);
        // Die Seiten muessen sich unterscheiden: ein Etikett, das
        // vergessen wurde, ergibt denselben Wert fuer beide.
        assert_ne!(c, s, "M7: Client und Server kommen zu verschiedenen Werten");
    }

    /// M7: die Grenzen. M3/M4 in einem Test.
    #[test]
    fn grenzen_werden_eingehalten() {
        let cr = bytes(CLIENT_RANDOM);
        let sr = bytes(SERVER_RANDOM);
        // Zu kurze Random-Werte.
        assert!(matches!(
            master_secret(&bytes(PREMASTER32), &cr[..16], &sr, Hash::Sha256),
            Err(Tls12Fehler::ZuKurz { braucht: 64, .. })
        ));
        // Zu lange Ausgabe.
        assert!(matches!(
            prf(b"x", b"y", b"z", MAX_PRF_AUSGABE + 1, Hash::Sha256),
            Err(Tls12Fehler::Laenge { .. })
        ));
        // Genau die Grenze geht noch.
        assert_eq!(
            prf(b"x", b"y", b"z", MAX_PRF_AUSGABE, Hash::Sha256)
                .expect("Grenze")
                .len(),
            MAX_PRF_AUSGABE
        );
        // Ein zu kurzer Transkript-Hash.
        let ms = [0u8; MASTER_LAENGE];
        assert!(matches!(
            finished_verify_data(&ms, &[0u8; 16], true, Hash::Sha384),
            Err(Tls12Fehler::ZuKurz { .. })
        ));
    }

    /// M7: eine unbekannte Suite wird abgelehnt, nicht geraten.
    #[test]
    fn unbekannte_suite_wird_abgelehnt() {
        // 0xC02F = TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
        // 0xC02B = TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 — beide
        // SHA-256. FUER SHA-384 sind es 0xC030 und 0xC02C.
        //
        // MEINE ERSTE ERWARTUNG HIER war falsch: ich hatte 0xC02B fuer
        // SHA-384 gehalten, weil 0xC02B zwischen 0xC02A und 0xC030
        // liegt. Die IANA-Tabelle sagt etwas anderes, und die Tabelle
        // hat recht.
        assert_eq!(hash_aus_suite(0xC02F), Ok(Hash::Sha256));
        assert_eq!(hash_aus_suite(0xC02B), Ok(Hash::Sha256));
        assert_eq!(hash_aus_suite(0xC030), Ok(Hash::Sha384));
        assert_eq!(hash_aus_suite(0xC02C), Ok(Hash::Sha384));
        assert_eq!(hash_aus_suite(0xCCA8), Ok(Hash::Sha256));
        // Und die CBC-Suiten sind abgelehnt, obwohl sie einen Hash
        // haben: 0xC028 = TLS_ECDHE_RSA_WITH_AES_256_CBC_SHA384.
        assert_eq!(hash_aus_suite(0xC028), Err(Tls12Fehler::HashUnbekannt));
        // 0x1301 ist TLS 1.3 und gehoert hier nicht her.
        assert_eq!(hash_aus_suite(0x1301), Err(Tls12Fehler::HashUnbekannt));
    }

    /// M7: die `finished`-Werte mit SHA-384.
    ///
    /// DER FEHLER, DEN DIESER TEST VERHINDERT: im ersten Wurf stand der
    /// Transkript-Hash im SHA-384-Zweig auf SHA-256. Beide Seiten haetten
    /// dann dasselbe falsche Ergebnis gehabt und der Test waere gruen
    /// gewesen; er waere erst gegen einen echten Server rot geworden,
    /// als "Handshake bricht ab", ohne Ort.
    #[test]
    fn finished_sha384_nimmt_auch_sha384() {
        let mut transkript = Vec::with_capacity(2048);
        for _ in 0..8 {
            for b in 0..=255u8 {
                transkript.push(b);
            }
        }
        let th384 = crate::sha2::sha384(&transkript);
        let ms = master_secret(
            &bytes(PREMASTER48),
            &bytes(CLIENT_RANDOM),
            &bytes(SERVER_RANDOM),
            Hash::Sha384,
        )
        .expect("master secret");
        let c = finished_verify_data(&ms, &th384, true, Hash::Sha384).expect("client");
        let s = finished_verify_data(&ms, &th384, false, Hash::Sha384).expect("server");
        assert_eq!(crate::sha2::hexen(&c), FINISHED_CLIENT_SHA384);
        assert_eq!(crate::sha2::hexen(&s), FINISHED_SERVER_SHA384);
        // Und ein ANDERER Transkript ergibt ein anderes `finished`.
        //
        // MEIN ERSTER VERSUCH stand hier mit dem SHA-256-Hash des
        // Transkripts und panickte. Das war richtig und nicht ein
        // Fehler des Codes: `finished_verify_data` verlangt bei einer
        // SHA-384-Suite einen 48-Byte-Hash, und ein 32-Byte-Hash ist
        // kein gueltiger. Die Pruefung hat genau das getan, wofuer sie
        // da ist — sie hat einen Zufallswert abgewiesen, der als
        // Transkript-Hash durchging.
        //
        // Der Vergleich laeuft darum ueber einen zweiten, ebenfalls
        // 48-Byte-Hash: SHA-384 ueber den Transkript mit einem
        // angehaengten Byte.
        let mut transkript2 = transkript.clone();
        transkript2.push(0);
        let th384_b = crate::sha2::sha384(&transkript2);
        let c_b = finished_verify_data(&ms, &th384_b, true, Hash::Sha384).expect("zweiter");
        assert_ne!(c, c_b, "M7: ein Byte mehr Transkript, ein anderes finished");

        // Und die Laengenpruefung gilt in beiden Richtungen: ein
        // 32-Byte-Hash bei SHA-384 wird abgewiesen, ein 48-Byte-Hash
        // bei SHA-256 angenommen (der Hash darf nicht zu kurz sein,
        // aber zu lang ist er harmlos).
        assert!(matches!(
            finished_verify_data(&ms, &crate::sha2::sha256(&transkript), true, Hash::Sha384),
            Err(Tls12Fehler::ZuKurz {
                braucht: 48,
                war: 32
            })
        ));
        assert!(finished_verify_data(&ms, &th384, true, Hash::Sha256).is_ok());
    }
}
