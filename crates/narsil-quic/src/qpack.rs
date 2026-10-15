//! QPACK — Feldkompression fuer HTTP/3 (RFC 9204).
//!
//! Fuer Q5 zunaechst nur der Teil ohne dynamische Tabelle (RFC 9204
//! Abschnitt 3.1): die STATISCHE Tabelle (Anhang A, 99 Eintraege) plus
//! Literal-Darstellungen ohne Huffman. Das deckt die häufigsten
//! Request-/Response-Koepfe ab und ist deterministisch testbar.
//!
//! Darstellungen, die hier kodiert/dekodiert werden:
//!   * Indexed Field Line (statisch; `1 1 index<6>`),
//!   * Literal Field Line with Name Reference (statisch; `01 N T index<4>`),
//!   * Literal Field Line with Literal Name (`001 N H laenge<3>`).
//!
//! Referenzen auf die dynamische Tabelle und Huffman sind (noch) ein
//! benannter Fehler, kein Panic.
//!
//! M3/M4: jede Laenge wird vor dem Lesen geprueft; die Ganzzahl-Kodierung
//! ist gegen Ueberlauf begrenzt; die Zahl der Felder ist begrenzt.

/// Eine Feldzeile (Name-Wert-Paar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeldZeile {
    pub name: Vec<u8>,
    pub wert: Vec<u8>,
}

impl FeldZeile {
    pub fn neu(name: &str, wert: &str) -> Self {
        FeldZeile {
            name: name.as_bytes().to_vec(),
            wert: wert.as_bytes().to_vec(),
        }
    }
}

/// Die statische Tabelle (RFC 9204, Anhang A), woertlich uebernommen.
pub const STATISCHE_TABELLE: [(&str, &str); 99] = [
    (":authority", ""),
    (":path", "/"),
    ("age", "0"),
    ("content-disposition", ""),
    ("content-length", "0"),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("referer", ""),
    ("set-cookie", ""),
    (":method", "CONNECT"),
    (":method", "DELETE"),
    (":method", "GET"),
    (":method", "HEAD"),
    (":method", "OPTIONS"),
    (":method", "POST"),
    (":method", "PUT"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "103"),
    (":status", "200"),
    (":status", "304"),
    (":status", "404"),
    (":status", "503"),
    ("accept", "*/*"),
    ("accept", "application/dns-message"),
    ("accept-encoding", "gzip, deflate, br"),
    ("accept-ranges", "bytes"),
    ("access-control-allow-headers", "cache-control"),
    ("access-control-allow-headers", "content-type"),
    ("access-control-allow-origin", "*"),
    ("cache-control", "max-age=0"),
    ("cache-control", "max-age=2592000"),
    ("cache-control", "max-age=604800"),
    ("cache-control", "no-cache"),
    ("cache-control", "no-store"),
    ("cache-control", "public, max-age=31536000"),
    ("content-encoding", "br"),
    ("content-encoding", "gzip"),
    ("content-type", "application/dns-message"),
    ("content-type", "application/javascript"),
    ("content-type", "application/json"),
    ("content-type", "application/x-www-form-urlencoded"),
    ("content-type", "image/gif"),
    ("content-type", "image/jpeg"),
    ("content-type", "image/png"),
    ("content-type", "text/css"),
    ("content-type", "text/html; charset=utf-8"),
    ("content-type", "text/plain"),
    ("content-type", "text/plain;charset=utf-8"),
    ("range", "bytes=0-"),
    ("strict-transport-security", "max-age=31536000"),
    (
        "strict-transport-security",
        "max-age=31536000; includesubdomains",
    ),
    (
        "strict-transport-security",
        "max-age=31536000; includesubdomains; preload",
    ),
    ("vary", "accept-encoding"),
    ("vary", "origin"),
    ("x-content-type-options", "nosniff"),
    ("x-xss-protection", "1; mode=block"),
    (":status", "100"),
    (":status", "204"),
    (":status", "206"),
    (":status", "302"),
    (":status", "400"),
    (":status", "403"),
    (":status", "421"),
    (":status", "425"),
    (":status", "500"),
    ("accept-language", ""),
    ("access-control-allow-credentials", "FALSE"),
    ("access-control-allow-credentials", "TRUE"),
    ("access-control-allow-headers", "*"),
    ("access-control-allow-methods", "get"),
    ("access-control-allow-methods", "get, post, options"),
    ("access-control-allow-methods", "options"),
    ("access-control-expose-headers", "content-length"),
    ("access-control-request-headers", "content-type"),
    ("access-control-request-method", "get"),
    ("access-control-request-method", "post"),
    ("alt-svc", "clear"),
    ("authorization", ""),
    (
        "content-security-policy",
        "script-src 'none'; object-src 'none'; base-uri 'none'",
    ),
    ("early-data", "1"),
    ("expect-ct", ""),
    ("forwarded", ""),
    ("if-range", ""),
    ("origin", ""),
    ("purpose", "prefetch"),
    ("server", ""),
    ("timing-allow-origin", "*"),
    ("upgrade-insecure-requests", "1"),
    ("user-agent", ""),
    ("x-forwarded-for", ""),
    ("x-frame-options", "deny"),
    ("x-frame-options", "sameorigin"),
];

/// Hoechste Zahl von Feldzeilen in einem Abschnitt (M3).
pub const MAX_FELDER: usize = 128;
/// Groesste Laenge eines Namens oder Werts nach der Huffman-Dekodierung (M4).
///
/// Begrenzt die Ausgabe der Huffman-Dekodierung: ein boesartiger, kleiner
/// Huffman-Strom darf nicht zu einem beliebig grossen Feld aufblaest.
pub const MAX_FELD_LAENGE: usize = 32767;

/// Warum eine QPACK-Dekodierung scheitert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QpackFehler {
    Abgeschnitten {
        braucht: usize,
        da: usize,
    },
    ZuLang,
    ZuVieleFelder {
        ist: usize,
        maximum: usize,
    },
    /// Ein Feld ist laenger als `MAX_FELD_LAENGE`.
    FeldZuLang {
        ist: usize,
        maximum: usize,
    },
    /// Referenz auf die dynamische Tabelle (im Nur-Statik-Dekoder).
    DynamischNichtUnterstuetzt,
    /// Ein Index in die dynamische Tabelle ist ungueltig.
    UngueltigerDynamikIndex {
        index: u64,
    },
    /// Die Kapazitaet der dynamischen Tabelle uebersteigt das Maximum.
    KapazitaetZuGross {
        ist: u64,
        maximum: u64,
    },
    /// Zu viele blockierte Stroeme (RFC 9204 Abschnitt 2.1.2).
    ZuVieleBlockierteStroeme,
    /// Der Required Insert Count ist ungueltig (RFC 9204 Abschnitt 4.5.1.1).
    RicUngueltig {
        ist: u64,
    },
    /// Der Huffman-Strom enthaelt das EOS-Symbol 256 (RFC 7541 Abschnitt 5.2).
    HuffmanEos,
    /// Die Huffman-Nachfuellung ist ungueltig (laenger als 7 Bit oder keine Einsen).
    HuffmanPadding,
    /// Ein Huffman-Code ist unbekannt (defensiv; mit der korrekten Tabelle
    /// unerreichbar).
    HuffmanUnbekannt,
    /// Unbekannte Darstellung.
    UnbekannteDarstellung,
    /// Ungueltiger Statik-Index.
    UngueltigerIndex {
        index: u64,
    },
}

impl std::fmt::Display for QpackFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QpackFehler::Abgeschnitten { braucht, da } => {
                write!(f, "braucht {braucht} Byte, da sind {da}")
            }
            QpackFehler::ZuLang => write!(f, "Ganzzahl ueberlaueft"),
            QpackFehler::ZuVieleFelder { ist, maximum } => {
                write!(f, "{ist} Felder, erlaubt sind {maximum}")
            }
            QpackFehler::FeldZuLang { ist, maximum } => {
                write!(f, "Feld hat {ist} Byte, erlaubt sind {maximum}")
            }
            QpackFehler::DynamischNichtUnterstuetzt => {
                write!(f, "dynamische Tabelle ist nicht implementiert")
            }
            QpackFehler::UngueltigerDynamikIndex { index } => {
                write!(f, "ungueltiger Dynamik-Index {index}")
            }
            QpackFehler::KapazitaetZuGross { ist, maximum } => {
                write!(f, "Kapazitaet {ist} ueber dem Maximum {maximum}")
            }
            QpackFehler::ZuVieleBlockierteStroeme => {
                write!(f, "zu viele blockierte Stroeme")
            }
            QpackFehler::RicUngueltig { ist } => {
                write!(f, "Required Insert Count {ist} ist ungueltig")
            }
            QpackFehler::HuffmanEos => {
                write!(f, "der Huffman-Strom enthaelt das EOS-Symbol 256")
            }
            QpackFehler::HuffmanPadding => write!(f, "ungueltige Huffman-Nachfuellung"),
            QpackFehler::HuffmanUnbekannt => write!(f, "unbekannter Huffman-Code"),
            QpackFehler::UnbekannteDarstellung => write!(f, "unbekannte Darstellung"),
            QpackFehler::UngueltigerIndex { index } => {
                write!(f, "ungueltiger Statik-Index {index}")
            }
        }
    }
}

impl std::error::Error for QpackFehler {}

/// Exakter Treffer in der statischen Tabelle (Name UND Wert).
pub fn statisch_index(name: &[u8], wert: &[u8]) -> Option<usize> {
    STATISCHE_TABELLE
        .iter()
        .position(|(n, w)| n.as_bytes() == name && w.as_bytes() == wert)
}

/// Treffer nur ueber den Namen (Wert wird literal kodiert).
pub fn statisch_name_index(name: &[u8]) -> Option<usize> {
    STATISCHE_TABELLE
        .iter()
        .position(|(n, _)| n.as_bytes() == name)
}

/// Schreibt eine Ganzzahl mit `praefix` Bits; die oberen (8 - praefix) Bits
/// des ersten Bytes sind `vorsatz` (die Repraesentations-Bits).
/// RFC 7541 Abschnitt 5.1.
pub(crate) fn ganzzahl_mit_vorsatz(mut wert: u64, praefix: u8, vorsatz: u8, aus: &mut Vec<u8>) {
    let maximum = (1u64 << praefix) - 1;
    if wert < maximum {
        aus.push(vorsatz | (wert as u8));
        return;
    }
    aus.push(vorsatz | (maximum as u8));
    wert -= maximum;
    while wert >= 128 {
        aus.push((wert % 128) as u8 | 0x80);
        wert /= 128;
    }
    aus.push(wert as u8);
}

/// Liest eine Ganzzahl mit `praefix` Bits. Liefert `(wert, verbraucht)`.
pub(crate) fn ganzzahl_lesen(daten: &[u8], praefix: u8) -> Result<(u64, usize), QpackFehler> {
    if daten.is_empty() {
        return Err(QpackFehler::Abgeschnitten { braucht: 1, da: 0 });
    }
    let maximum = (1u64 << praefix) - 1;
    let mut wert = (daten[0] & (maximum as u8)) as u64;
    let mut verbraucht = 1;
    if wert == maximum {
        let mut m = 0u32;
        loop {
            if verbraucht >= daten.len() {
                return Err(QpackFehler::Abgeschnitten {
                    braucht: verbraucht + 1,
                    da: daten.len(),
                });
            }
            let b = daten[verbraucht];
            verbraucht += 1;
            // Gepruefte Addition (M3): ein boesartiger Strom mit vielen
            // Fortsetzungsbytes darf nicht ueberlaufen.
            let summand = ((b & 0x7f) as u64)
                .checked_shl(m)
                .ok_or(QpackFehler::ZuLang)?;
            wert = wert.checked_add(summand).ok_or(QpackFehler::ZuLang)?;
            if b & 0x80 == 0 {
                break;
            }
            m += 7;
        }
    }
    Ok((wert, verbraucht))
}

/// Huffman-kodiert Bytes (RFC 7541, Anhang B). Die Ausgabe ist mit Einsen
/// (EOS-Praefix) auf die naechste Byte-Grenze aufgefuellt (Abschnitt 5.2).
pub fn huffman_kodiere(daten: &[u8]) -> Vec<u8> {
    let mut aus = Vec::new();
    let mut akku = 0u16;
    let mut akku_bits = 0u32;
    for &b in daten {
        let (code, laenge) = crate::huffman::HUFFMAN[b as usize];
        for stufe in (0..laenge).rev() {
            akku = (akku << 1) | (((code >> stufe) & 1) as u16);
            akku_bits += 1;
            if akku_bits == 8 {
                aus.push(akku as u8);
                akku = 0;
                akku_bits = 0;
            }
        }
    }
    if akku_bits > 0 {
        let fuellung = 8 - akku_bits;
        let wert = (akku << fuellung) | ((1u16 << fuellung) - 1);
        aus.push((wert & 0xff) as u8);
    }
    aus
}

/// Dekodiert einen Huffman-Bitstrom (RFC 7541, Abschnitt 5.2).
///
/// Die Ausgabe ist begrenzt (M4): hoechstens `8 * Eingabe` und hoechstens
/// `MAX_FELD_LAENGE` — gegen Aufblasen durch boesartige, kleine Stroeme.
/// EOS und ungueltige Nachfuellung sind benannte Fehler.
pub fn huffman_dekodiere(daten: &[u8]) -> Result<Vec<u8>, QpackFehler> {
    // Baum aus der Tabelle bauen (klein, deterministisch, 257 Symbole).
    // Knoten k hat Kinder an 2*k (Bit 0) und 2*k+1 (Bit 1). `blatt` traegt das
    // Symbol, `kinder` den Folgeknoten.
    let mut kinder = vec![0u16; 2 * 2 * 257];
    let mut blatt = vec![0xFFFFu16; 2 * 2 * 257];
    let mut naechster = 1usize;
    for sym in 0..257usize {
        let (code, laenge) = crate::huffman::HUFFMAN[sym];
        let mut knoten = 0usize;
        for stufe in 0..laenge {
            let bit = ((code >> (laenge - 1 - stufe)) & 1) as usize;
            if stufe + 1 == laenge {
                blatt[knoten * 2 + bit] = sym as u16;
            } else {
                if kinder[knoten * 2 + bit] == 0 {
                    kinder[knoten * 2 + bit] = naechster as u16;
                    naechster += 1;
                }
                knoten = kinder[knoten * 2 + bit] as usize;
            }
        }
    }

    let maximum = daten.len().saturating_mul(8).min(MAX_FELD_LAENGE);
    let mut aus = Vec::new();
    let mut knoten = 0usize;
    let mut haengend_bits = 0u32;
    let mut haengend_alles_eins = true;

    for &byte in daten {
        for stufe in (0..8).rev() {
            let bit = ((byte >> stufe) & 1) as usize;
            let pos = knoten * 2 + bit;
            if blatt[pos] != 0xFFFF {
                let sym = blatt[pos] as usize;
                if sym == 256 {
                    return Err(QpackFehler::HuffmanEos);
                }
                aus.push(sym as u8);
                if aus.len() > maximum {
                    return Err(QpackFehler::FeldZuLang {
                        ist: aus.len(),
                        maximum: MAX_FELD_LAENGE,
                    });
                }
                knoten = 0;
                haengend_bits = 0;
                haengend_alles_eins = true;
            } else if kinder[pos] != 0 {
                knoten = kinder[pos] as usize;
                haengend_bits += 1;
                if bit == 0 {
                    haengend_alles_eins = false;
                }
            } else {
                return Err(QpackFehler::HuffmanUnbekannt);
            }
        }
    }
    // Nachfuellungs-Pruefung (nur wenn am Ende ein Code haengt, RFC 7541 5.2).
    if knoten != 0 && (haengend_bits > 7 || !haengend_alles_eins) {
        return Err(QpackFehler::HuffmanPadding);
    }
    Ok(aus)
}

/// Liest ein String-Literal mit `praefix` Bits fuer die Laenge; `huffman`
/// gibt an, ob der Inhalt Huffman-kodiert ist. Liefert `(bytes, verbraucht)`.
pub(crate) fn string_literal_lesen(
    daten: &[u8],
    huffman: bool,
    praefix: u8,
) -> Result<(Vec<u8>, usize), QpackFehler> {
    let (laenge, verbraucht) = ganzzahl_lesen(daten, praefix)?;
    let ende = verbraucht + laenge as usize;
    if daten.len() < ende {
        return Err(QpackFehler::Abgeschnitten {
            braucht: ende,
            da: daten.len(),
        });
    }
    let roh = &daten[verbraucht..ende];
    let inhalt = if huffman {
        huffman_dekodiere(roh)?
    } else {
        roh.to_vec()
    };
    Ok((inhalt, ende))
}

/// Liest ein String-Literal (H-Bit + 7-Bit-Laenge + Bytes) und gibt
/// `(bytes, verbraucht)` zurueck. Ist das H-Bit gesetzt, wird Huffman
/// dekodiert (RFC 7541 Abschnitt 5.2).
pub(crate) fn zeichenkette_lesen(daten: &[u8]) -> Result<(Vec<u8>, usize), QpackFehler> {
    if daten.is_empty() {
        return Err(QpackFehler::Abgeschnitten { braucht: 1, da: 0 });
    }
    string_literal_lesen(daten, daten[0] & 0x80 != 0, 7)
}

/// Kodiert einen ganzen Feldabschnitt (mit Praefix, ohne dynamische Tabelle).
pub fn feldabschnitt_kodieren(felder: &[FeldZeile]) -> Vec<u8> {
    let mut aus = Vec::new();
    // Praefix: Required Insert Count = 0 (8 Bit), Sign = 0, Delta Base = 0.
    ganzzahl_mit_vorsatz(0, 8, 0, &mut aus);
    ganzzahl_mit_vorsatz(0, 7, 0, &mut aus);
    for feld in felder {
        feld_kodieren(feld, &mut aus);
    }
    aus
}

fn feld_kodieren(feld: &FeldZeile, aus: &mut Vec<u8>) {
    // 1) Exakter Treffer -> Indexed Field Line (statisch).
    if let Some(i) = statisch_index(&feld.name, &feld.wert) {
        // `1 1 index<6>`.
        ganzzahl_mit_vorsatz(i as u64, 6, 0xc0, aus);
        return;
    }
    // 2) Name in der Tabelle -> Literal Field Line with Name Reference.
    if let Some(i) = statisch_name_index(&feld.name) {
        // `01 N=0 T=1 index<4>`.
        ganzzahl_mit_vorsatz(i as u64, 4, 0x50, aus);
        ganzzahl_mit_vorsatz(feld.wert.len() as u64, 7, 0, aus);
        aus.extend_from_slice(&feld.wert);
        return;
    }
    // 3) Neuer Name -> Literal Field Line with Literal Name.
    // `001 N=0 H=0 laenge<3>`.
    ganzzahl_mit_vorsatz(feld.name.len() as u64, 3, 0x20, aus);
    aus.extend_from_slice(&feld.name);
    ganzzahl_mit_vorsatz(feld.wert.len() as u64, 7, 0, aus);
    aus.extend_from_slice(&feld.wert);
}

/// Dekodiert einen ganzen Feldabschnitt (ohne dynamische Tabelle).
pub fn feldabschnitt_dekodieren(daten: &[u8]) -> Result<Vec<FeldZeile>, QpackFehler> {
    let (ric, n1) = ganzzahl_lesen(daten, 8)?;
    if ric != 0 {
        return Err(QpackFehler::DynamischNichtUnterstuetzt);
    }
    let rest = &daten[n1..];
    // Sign (Bit 7) und Delta Base (7-Bit-Praefix).
    if rest.is_empty() {
        return Err(QpackFehler::Abgeschnitten { braucht: 1, da: 0 });
    }
    let sign = rest[0] & 0x80 != 0;
    let (delta_base, n2) = ganzzahl_lesen(rest, 7)?;
    if sign || delta_base != 0 {
        return Err(QpackFehler::DynamischNichtUnterstuetzt);
    }
    let mut rest = &rest[n2..];

    let mut felder = Vec::new();
    while !rest.is_empty() {
        if felder.len() >= MAX_FELDER {
            return Err(QpackFehler::ZuVieleFelder {
                ist: felder.len() + 1,
                maximum: MAX_FELDER,
            });
        }
        let erste = rest[0];
        let (feld, verbraucht) = if erste & 0x80 != 0 {
            // Indexed Field Line: `1 T index<6>`.
            if erste & 0x40 == 0 {
                return Err(QpackFehler::DynamischNichtUnterstuetzt);
            }
            let (index, n) = ganzzahl_lesen(rest, 6)?;
            let (name, wert) = statik_eintrag(index)?;
            (
                FeldZeile {
                    name: name.to_vec(),
                    wert: wert.to_vec(),
                },
                n,
            )
        } else if erste & 0x40 != 0 {
            // Literal Field Line with Name Reference: `01 N T index<4>`.
            if erste & 0x10 == 0 {
                return Err(QpackFehler::DynamischNichtUnterstuetzt);
            }
            let (index, n) = ganzzahl_lesen(rest, 4)?;
            let (name, _) = statik_eintrag(index)?;
            let (wert, n2) = zeichenkette_lesen(&rest[n..])?;
            (
                FeldZeile {
                    name: name.to_vec(),
                    wert,
                },
                n + n2,
            )
        } else if erste & 0x20 != 0 {
            // Literal Field Line with Literal Name: `001 N H laenge<3>`.
            let name_huffman = erste & 0x08 != 0;
            let (name_len, n) = ganzzahl_lesen(rest, 3)?;
            let name_ende = n + name_len as usize;
            if rest.len() < name_ende {
                return Err(QpackFehler::Abgeschnitten {
                    braucht: name_ende,
                    da: rest.len(),
                });
            }
            let roh = &rest[n..name_ende];
            let name = if name_huffman {
                huffman_dekodiere(roh)?
            } else {
                roh.to_vec()
            };
            let (wert, n2) = zeichenkette_lesen(&rest[name_ende..])?;
            (FeldZeile { name, wert }, name_ende + n2)
        } else {
            // Post-Base- oder unbekannte Darstellung.
            return Err(QpackFehler::UnbekannteDarstellung);
        };
        felder.push(feld);
        rest = &rest[verbraucht..];
    }
    Ok(felder)
}

pub(crate) fn statik_eintrag(index: u64) -> Result<(&'static [u8], &'static [u8]), QpackFehler> {
    let (name, wert) = STATISCHE_TABELLE
        .get(index as usize)
        .ok_or(QpackFehler::UngueltigerIndex { index })?;
    Ok((name.as_bytes(), wert.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9204, Anhang B.1: ein Literal mit statischem Namensverweis,
    /// byte-genau (`:path=/index.html`).
    #[test]
    fn rfc9204_b1_literal_name_reference() {
        let felder = vec![FeldZeile::neu(":path", "/index.html")];
        let kodiert = feldabschnitt_kodieren(&felder);
        assert_eq!(
            narsil_tls::sha2::hexen(&kodiert),
            "0000510b2f696e6465782e68746d6c",
            "M7: RFC 9204 B.1 byte-genau"
        );
        let gelesen = feldabschnitt_dekodieren(&kodiert).expect("dekodieren");
        assert_eq!(gelesen, felder, "M7: Rundlauf");
    }

    /// Ein vollstaendiger GET-Request-Kopfblock, kodiert und dekodiert.
    #[test]
    fn get_request_rundlauf() {
        let felder = vec![
            FeldZeile::neu(":method", "GET"),
            FeldZeile::neu(":scheme", "https"),
            FeldZeile::neu(":authority", "cloudflare-quic.com"),
            FeldZeile::neu(":path", "/"),
            FeldZeile::neu("user-agent", "narsil"),
        ];
        let kodiert = feldabschnitt_kodieren(&felder);
        // Praefix 0000, dann Indexed: :method GET (17 -> d1), :scheme https
        // (23 -> d7).
        assert_eq!(
            narsil_tls::sha2::hexen(&kodiert[..4]),
            "0000d1d7",
            "M7: Prefix + zwei Indexed-Statik-Eintraege"
        );
        let gelesen = feldabschnitt_dekodieren(&kodiert).expect("dekodieren");
        assert_eq!(gelesen, felder, "M7: GET-Request-Rundlauf");
    }

    /// Eine Response mit :status 200 (statisch) und content-length.
    #[test]
    fn response_rundlauf() {
        let felder = vec![
            FeldZeile::neu(":status", "200"),
            FeldZeile::neu("content-length", "12"),
            FeldZeile::neu("content-type", "text/plain"),
        ];
        let kodiert = feldabschnitt_kodieren(&felder);
        let gelesen = feldabschnitt_dekodieren(&kodiert).expect("dekodieren");
        assert_eq!(gelesen, felder, "M7: Response-Rundlauf");
    }

    /// M4: ein abgeschnittenes String-Literal ist ein benannter Fehler.
    #[test]
    fn abgeschnitten_ist_ein_benannter_fehler() {
        // Prefix ok, dann Literal-Name-Ref mit Wertlaenge 20 (0x14), aber nur
        // 2 Byte Wert (da=3: 1 Laengenbyte + 2 Wertbytes).
        let daten = narsil_tls::sha2::von_hex("000051140001").expect("hex");
        assert!(matches!(
            feldabschnitt_dekodieren(&daten),
            Err(QpackFehler::Abgeschnitten { braucht: 21, da: 3 })
        ));
    }

    /// RFC 7541, Anhang C.6: Huffman-Kodierung byte-genau (hin und zurueck).
    #[test]
    fn huffman_c6_byte_genau() {
        // (Klartext, Huffman-Hex) — woertlich aus RFC 7541 C.6.
        let faelle = [
            ("www.example.com", "f1e3c2e5f23a6ba0ab90f4ff"),
            ("no-cache", "a8eb10649cbf"),
            ("custom-key", "25a849e95ba97d7f"),
            ("custom-value", "25a849e95bb8e8b4bf"),
        ];
        for (klar, hex) in faelle {
            let kodiert = huffman_kodiere(klar.as_bytes());
            assert_eq!(
                narsil_tls::sha2::hexen(&kodiert),
                hex,
                "M7: Huffman-Kodierung von {klar}"
            );
            let dekodiert =
                huffman_dekodiere(&narsil_tls::sha2::von_hex(hex).unwrap()).expect("dekodieren");
            assert_eq!(
                dekodiert,
                klar.as_bytes(),
                "M7: Huffman-Dekodierung von {klar}"
            );
        }
    }

    /// Ein Feldabschnitt mit Huffman-kodiertem Wert laesst sich dekodieren.
    #[test]
    fn feldabschnitt_mit_huffman_wert() {
        // Literal Name Ref (Index 1 = :path), Wert "www.example.com" Huffman:
        // `01 0 1 0001` = 0x51, dann H=1 + Laenge 12 = 0x8c, dann f1e3...
        let mut daten = narsil_tls::sha2::von_hex("0000518c").expect("hex");
        daten.extend_from_slice(&narsil_tls::sha2::von_hex("f1e3c2e5f23a6ba0ab90f4ff").unwrap());
        let felder = feldabschnitt_dekodieren(&daten).expect("dekodieren");
        assert_eq!(
            felder,
            vec![FeldZeile::neu(":path", "www.example.com")],
            "M7: Huffman-Wert dekodiert"
        );
    }

    /// Die dynamische Tabelle ist (noch) ein benannter Fehler.
    #[test]
    fn dynamisch_ist_benannt() {
        let daten = narsil_tls::sha2::von_hex("010000").expect("hex");
        assert!(matches!(
            feldabschnitt_dekodieren(&daten),
            Err(QpackFehler::DynamischNichtUnterstuetzt)
        ));
    }

    /// RFC 7541 Abschnitt 5.2: EOS und ungueltige Nachfuellung sind benannte
    /// Fehler.
    #[test]
    fn huffman_eos_und_padding_sind_benannt() {
        // 32 Einsen: bei Bit 30 steht das EOS-Symbol (256).
        assert!(matches!(
            huffman_dekodiere(&[0xff, 0xff, 0xff, 0xff]),
            Err(QpackFehler::HuffmanEos)
        ));
        // "custom-key" endet mit 0x7f; kippt man das letzte Fuellbit auf 0,
        // ist die Nachfuellung ungueltig.
        let mut kaputt = narsil_tls::sha2::von_hex("25a849e95ba97d7f").expect("hex");
        let l = kaputt.len();
        kaputt[l - 1] ^= 0x01;
        assert!(matches!(
            huffman_dekodiere(&kaputt),
            Err(QpackFehler::HuffmanPadding) | Err(QpackFehler::HuffmanUnbekannt)
        ));
    }

    /// M3: Zufallsmuell darf nicht paniken.
    #[test]
    fn zufallsmuell_panikt_nicht() {
        let mut z = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..4096 {
            z ^= z << 13;
            z ^= z >> 7;
            z ^= z << 17;
            let laenge = (z as usize) % 64;
            let daten: Vec<u8> = (0..laenge)
                .map(|k| ((z >> ((k % 8) * 8)) & 0xff) as u8)
                .collect();
            let _ = feldabschnitt_dekodieren(&daten);
        }
    }
}
