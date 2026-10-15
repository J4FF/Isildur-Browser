//! T3 — HKDF (RFC 5869) und HKDF-Expand-Label (RFC 8446 Abschnitt 7.1).
//!
//! HKDF hat zwei Haelften: EXTRACT macht aus (salt, ikm) einen
//! Pseudozufallsschluessel PRK, EXPAND zieht daraus eine beliebig lange
//! Ausgabe. TLS 1.3 benutzt EXPAND-LABEL, bei dem das `info`-Feld selbst
//! aus einem TLS-Label gebaut wird.
//!
//! M3/M4 — DIE GRENZEN, und sie sind Pflicht, nicht Formsache:
//!
//!   * Die Ausgabelaenge `L` kommt von AUSSEN (der TLS-Schluesselplan
//!     fragt nach 32 Byte, ein Angreifer im Record-Layer vielleicht nach
//!     2^31). `L > 255 * Hashlaenge` ist nach RFC 5869 ein Fehler, und der
//!     Fehler wird VOR der Allokation geworfen. Wer das prueft, braucht
//!     keine `Vec::with_capacity(L)`.
//!   * Die Ausgabe wird in `MAX_AUSGABE_SCHRITTE` Schritten erzeugt, jeder
//!     Schritt ist ein eigener Block. Die Schleife kann also nicht laenger
//!     laufen als die Grenze erlaubt.

use crate::hmac::{HmacSha256, HmacSha384};

/// Laenge des Pseudozufallsschluessels von SHA-256.
pub const PRK256: usize = 32;
/// Laenge des Pseudozufallsschluessels von SHA-384.
pub const PRK384: usize = 48;

/// Die groesste Ausgabe, die der Code fuer SHA-256 erzeugt.
///
/// 255 Bloecke a 32 Byte sind nach RFC 5869 das Maximum (die Zaehlung der
/// Iteration geht bis 255). GEMESSEN/Korrektheit: RFC 5869 Abschnitt 2.3
/// sagt "L <= 255 * HashLen".
pub const MAX_AUSGABE256: usize = 255 * PRK256;
/// Dasselbe fuer SHA-384.
pub const MAX_AUSGABE384: usize = 255 * PRK384;

/// Obergrenze fuer die Schrittzahl. Deckt sich mit `MAX_AUSGABE*`, aber
/// als EIGENE Konstante, damit die Schleife beim Lesen begrenzt aussieht
/// (M3) und nicht erst durch eine Rechnung.
const MAX_SCHRITTE256: usize = 255;
const MAX_SCHRITTE384: usize = 255;

/// Warum eine Ausgabe nicht moeglich ist. BENANNTE Varianten, kein String
/// mit "irgendwas" — der Aufrufer soll entscheiden, nicht raten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fehler {
    /// Gewollte Laenge war 0. HKDF mit L = 0 ist nicht definiert.
    LaengeNull,
    /// Gewollte Laenge lag ueber dem Maximum (255 * Hashlaenge).
    ZuLang { gewollt: usize, maximum: usize },
    /// Der Pseudozufallsschluessel hatte die falsche Laenge.
    PrkFalsch { ist: usize, soll: usize },
    /// added 02.10.2026 03:5x: `expand_label_*` lieferte eine andere Laenge als
    /// die feste, die im Typ stand. GEMESSEN unerreichbar (die Laenge ist `N`),
    /// aber benannt statt behauptet — die Form ist dieselbe wie in
    /// `PrkFalsch`.
    FesteLaengeFalsch { gewollt: usize, bekommen: usize },
}

impl std::fmt::Display for Fehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fehler::LaengeNull => write!(f, "geforderte Laenge ist 0"),
            Fehler::ZuLang { gewollt, maximum } => {
                write!(f, "{} Byte verlangt, erlaubt sind {maximum}", gewollt)
            }
            Fehler::FesteLaengeFalsch { gewollt, bekommen } => write!(
                f,
                "die feste Laenge war {gewollt}, HKDF lieferte {bekommen}"
            ),
            Fehler::PrkFalsch { ist, soll } => {
                write!(f, "Pseudozufallsschluessel ist {ist} Byte, erwartet {soll}")
            }
        }
    }
}

impl std::error::Error for Fehler {}

// ---------------------------------------------------------------------------
// SHA-256
// ---------------------------------------------------------------------------

/// EXTRACT mit SHA-256. Ist `salt` leer, wird ein Block aus Nullen benutzt
/// (RFC 5869 Abschnitt 2.2).
pub fn extract_sha256(salt: &[u8], ikm: &[u8]) -> [u8; PRK256] {
    HmacSha256::berechne(salt, ikm)
}

/// EXPAND mit SHA-256.
///
/// `laenge` wird VOR der Allokation geprueft (M4). Die Schleife laeuft
/// hoechstens `MAX_SCHRITTE256` Mal (M3).
pub fn expand_sha256(prk: &[u8], info: &[u8], laenge: usize) -> Result<Vec<u8>, Fehler> {
    if laenge == 0 {
        return Err(Fehler::LaengeNull);
    }
    if laenge > MAX_AUSGABE256 {
        return Err(Fehler::ZuLang {
            gewollt: laenge,
            maximum: MAX_AUSGABE256,
        });
    }
    if prk.len() != PRK256 {
        return Err(Fehler::PrkFalsch {
            ist: prk.len(),
            soll: PRK256,
        });
    }

    let mut raus: Vec<u8> = Vec::with_capacity(laenge.min(MAX_AUSGABE256));
    // T(0) ist die leere Folge (RFC 5869 Abschnitt 2.3).
    let mut vorher: Vec<u8> = Vec::new();
    let mut schritt: u8 = 1;
    let mut schritte = 0usize;

    while raus.len() < laenge {
        schritte += 1;
        if schritte > MAX_SCHRITTE256 {
            // Erreichbar nur, wenn die Laengenrechnung oben falsch waere.
            // Lieber ein Fehler als eine Endlosschleife (M2/M3).
            return Err(Fehler::ZuLang {
                gewollt: laenge,
                maximum: MAX_AUSGABE256,
            });
        }
        // T(i) = HMAC(PRK, T(i-1) | info | i)
        let mut mac = HmacSha256::neu(prk);
        mac.aufnehmen(&vorher);
        mac.aufnehmen(info);
        mac.aufnehmen(&[schritt]);
        let block = mac.fertig();

        let nimm = block.len().min(laenge - raus.len());
        raus.extend_from_slice(&block[..nimm]);
        vorher.clear();
        vorher.extend_from_slice(&block);
        schritt = schritt.wrapping_add(1);
    }
    Ok(raus)
}

/// EXTRACT und EXPAND zusammen.
pub fn hkdf_sha256(salt: &[u8], ikm: &[u8], info: &[u8], laenge: usize) -> Result<Vec<u8>, Fehler> {
    let prk = extract_sha256(salt, ikm);
    expand_sha256(&prk, info, laenge)
}

// ---------------------------------------------------------------------------
// SHA-384
// ---------------------------------------------------------------------------

/// EXTRACT mit SHA-384.
pub fn extract_sha384(salt: &[u8], ikm: &[u8]) -> [u8; PRK384] {
    HmacSha384::berechne(salt, ikm)
}

/// EXPAND mit SHA-384. Dieselben Grenzen wie bei SHA-256, mit den Werten
/// fuer SHA-384.
pub fn expand_sha384(prk: &[u8], info: &[u8], laenge: usize) -> Result<Vec<u8>, Fehler> {
    if laenge == 0 {
        return Err(Fehler::LaengeNull);
    }
    if laenge > MAX_AUSGABE384 {
        return Err(Fehler::ZuLang {
            gewollt: laenge,
            maximum: MAX_AUSGABE384,
        });
    }
    if prk.len() != PRK384 {
        return Err(Fehler::PrkFalsch {
            ist: prk.len(),
            soll: PRK384,
        });
    }

    let mut raus: Vec<u8> = Vec::with_capacity(laenge.min(MAX_AUSGABE384));
    let mut vorher: Vec<u8> = Vec::new();
    let mut schritt: u8 = 1;
    let mut schritte = 0usize;

    while raus.len() < laenge {
        schritte += 1;
        if schritte > MAX_SCHRITTE384 {
            return Err(Fehler::ZuLang {
                gewollt: laenge,
                maximum: MAX_AUSGABE384,
            });
        }
        let mut mac = HmacSha384::neu(prk);
        mac.aufnehmen(&vorher);
        mac.aufnehmen(info);
        mac.aufnehmen(&[schritt]);
        let block = mac.fertig();

        let nimm = block.len().min(laenge - raus.len());
        raus.extend_from_slice(&block[..nimm]);
        vorher.clear();
        vorher.extend_from_slice(&block);
        schritt = schritt.wrapping_add(1);
    }
    Ok(raus)
}

/// EXTRACT und EXPAND zusammen, SHA-384.
pub fn hkdf_sha384(salt: &[u8], ikm: &[u8], info: &[u8], laenge: usize) -> Result<Vec<u8>, Fehler> {
    let prk = extract_sha384(salt, ikm);
    expand_sha384(&prk, info, laenge)
}

// ---------------------------------------------------------------------------
// TLS 1.3 HKDF-Expand-Label (RFC 8446 Abschnitt 7.1)
// ---------------------------------------------------------------------------

/// Das Label, das TLS 1.3 jedem Expand-Label voranstellt.
pub const LABEL_PRAEFIX: &[u8; 6] = b"tls13 ";

/// Obergrenze fuer die Laenge des Labels. RFC 8446 erlaubt 1..=249 Byte
/// fuer das Gesamtfeld `opaque label<7..255>`, davon sind 6 der Praefix.
const MAX_LABEL: usize = 249 - 6;
/// Obergrenze fuer den Kontext. RFC 8446 erlaubt `opaque context<0..255>`.
const MAX_KONTEXT: usize = 255;

/// Baut das `info`-Feld fuer EXPAND-LABEL.
///
/// Die Binaerstruktur nach RFC 8446:
///     uint16 length;          Laenge der Ausgabe in Byte
///     opaque label<7..255>;   "tls13 " + Label
///     opaque context<0..255>; Kontext, meist leer
/// mit dem Namen HkdfLabel.
///
/// M3/M4: Label und Kontext kommen von aussen (aus dem Schluesselplan, im
/// TLS-Client aus dem Server-Namen abgeleitet). Beide werden geprueft, BEVOR
/// hier ein `Vec` entsteht.
pub fn label_info(label: &[u8], kontext: &[u8]) -> Result<Vec<u8>, Fehler> {
    if label.len() > MAX_LABEL {
        return Err(Fehler::ZuLang {
            gewollt: label.len(),
            maximum: MAX_LABEL,
        });
    }
    if kontext.len() > MAX_KONTEXT {
        return Err(Fehler::ZuLang {
            gewollt: kontext.len(),
            maximum: MAX_KONTEXT,
        });
    }
    // M4: die Groesse ist jetzt bekannt und geprueft. 2 + 1 + 6 + label + 1 + kontext
    let groesse = 2 + 1 + LABEL_PRAEFIX.len() + label.len() + 1 + kontext.len();
    let mut raus = Vec::with_capacity(groesse);
    // `length` wird spaeter von `expand_label_*` gefuellt; hier steht der
    // Platzhalter 0, damit die Struktur stimmt.
    raus.extend_from_slice(&[0, 0]);
    raus.push((LABEL_PRAEFIX.len() + label.len()) as u8);
    raus.extend_from_slice(LABEL_PRAEFIX);
    raus.extend_from_slice(label);
    raus.push(kontext.len() as u8);
    raus.extend_from_slice(kontext);
    Ok(raus)
}

/// EXPAND-LABEL mit SHA-256.
pub fn expand_label_sha256(
    geheimnis: &[u8],
    label: &[u8],
    kontext: &[u8],
    laenge: usize,
) -> Result<Vec<u8>, Fehler> {
    let mut info = label_info(label, kontext)?;
    if laenge > 0xFFFF {
        return Err(Fehler::ZuLang {
            gewollt: laenge,
            maximum: 0xFFFF,
        });
    }
    info[0] = (laenge >> 8) as u8;
    info[1] = laenge as u8;
    expand_sha256(geheimnis, &info, laenge)
}

/// added 02.10.2026 03:5x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// EXPAND-LABEL mit SHA-256 und **fester** Laenge — `N` steht im Typ, nicht
/// im Aufruf.
///
/// DER GRUND, und es ist eine Regelverletzung, keine Formalie: in
/// `record.rs` stand an **15** Stellen `expand_label_sha256(…, 32)
/// .expect("32 Byte sind erlaubt")`. GEMESSEN: alle 15 übergeben die 32 und
/// kommen aus `Schluesselplan`; `expand_label_sha256` scheitert an genau zwei
/// Dingen (`label_info` bei einem zu langen Label, `laenge > 0xFFFF`). Mit
/// `N` im Typ ist die Länge nicht mehr ein Argument, also kann der zweite Fall
/// gar nicht mehr eintreten — und der erste bleibt ein `Result`, weil er
/// wirklich möglich ist.
///
/// added 02.10.2026 03:5x: DIE UMWANDLUNG ist **kein** `expect`, sondern ein
/// benannter Fehler (`FesteLaengeFalsch`), obwohl sie hier unerreichbar ist:
/// genau wie in `ticket.rs::finished_key_aus_binder_key` (Zeile 1391) wird
/// der Fall benannt statt behauptet.
pub fn expand_label_sha256_fest<const N: usize>(
    geheimnis: &[u8],
    label: &[u8],
    kontext: &[u8],
) -> Result<[u8; N], Fehler> {
    let v = expand_label_sha256(geheimnis, label, kontext, N)?;
    let arr: [u8; N] = v
        .try_into()
        .map_err(|v: Vec<u8>| Fehler::FesteLaengeFalsch {
            gewollt: N,
            bekommen: v.len(),
        })?;
    Ok(arr)
}

/// EXPAND-LABEL mit SHA-384.
pub fn expand_label_sha384(
    geheimnis: &[u8],
    label: &[u8],
    kontext: &[u8],
    laenge: usize,
) -> Result<Vec<u8>, Fehler> {
    let mut info = label_info(label, kontext)?;
    if laenge > 0xFFFF {
        return Err(Fehler::ZuLang {
            gewollt: laenge,
            maximum: 0xFFFF,
        });
    }
    info[0] = (laenge >> 8) as u8;
    info[1] = laenge as u8;
    expand_sha384(geheimnis, &info, laenge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::{hexen, von_hex};

    fn h(s: &str) -> Vec<u8> {
        von_hex(s).expect("Hex im Test")
    }

    /// RFC 5869 Testfall 1: Hash SHA-256, alle drei Felder belegt.
    #[test]
    fn rfc5869_fall_1() {
        let ikm = h("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
        let salt = h("000102030405060708090a0b0c");
        let info = h("f0f1f2f3f4f5f6f7f8f9");

        let prk = extract_sha256(&salt, &ikm);
        assert_eq!(
            hexen(&prk),
            "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5",
            "PRK aus RFC 5869 Testfall 1"
        );

        let okm = expand_sha256(&prk, &info, 42).expect("42 Byte sind erlaubt");
        assert_eq!(okm.len(), 42, "M7: Laenge gesichert");
        assert_eq!(
            hexen(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865",
            "OKM aus RFC 5869 Testfall 1"
        );
    }

    /// RFC 5869 Testfall 2: Salt und Info leer.
    ///
    /// WICHTIG UND GEMESSEN: die IKM hat hier ZWOELF Byte (0x01..0x0c).
    /// Ich hatte beim ersten Schreiben dreizehn Byte in der Hex-Zeile und
    /// verglich dann gegen einen Referenzwert, der ebenfalls dreizehn
    /// Byte hatte — die Implementierung sah "falsch" aus und war korrekt.
    /// Daraus die Regel fuer diese Crate: Sollwerte werden nicht von Hand
    /// eingetragen, sondern gegen eine unabhaengige Implementifikation
    /// gerechnet und die Rechnung im Kommentar festgehalten.
    #[test]
    fn rfc5869_fall_2() {
        let ikm = h("0102030405060708090a0b0c");
        assert_eq!(ikm.len(), 12, "M7: die Eingabe hat 12 Byte");
        let prk = extract_sha256(&[], &ikm);
        assert_eq!(
            hexen(&prk),
            "e0ed2d2d5267de13277523f44dbdd6664a174f302a9e652652bdc80df061fea2"
        );
        let okm = expand_sha256(&prk, &[], 42).expect("42 Byte sind erlaubt");
        assert_eq!(okm.len(), 42, "M7: Laenge");
        assert_eq!(
            hexen(&okm),
            "956bd278a5744de43d8621175bae74ab2d1f43c7ec64a6282453d2e67d9faf00fb02698387fa0e5974ad"
        );
    }

    /// RFC 5869 Testfall 3: gleiche IKM wie Fall 1, aber Salz und Info leer.
    ///
    /// Hier faellt auf, dass PRK und OKM sich unterscheiden — Extract und
    /// Expand sind verschiedene Schritte. Der Test sichert das ab.
    #[test]
    fn rfc5869_fall_3() {
        let ikm = h("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
        let prk = extract_sha256(&[], &ikm);
        assert_eq!(
            hexen(&prk),
            "19ef24a32c717b167f33a91d6f648bdf96596776afdb6377ac434c1c293ccb04"
        );
        let okm = expand_sha256(&prk, &[], 42).expect("42 Byte");
        assert_ne!(prk.to_vec(), okm, "PRK und OKM sind verschiedene Werte");
        assert_eq!(
            hexen(&okm),
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
        );
    }

    /// M4 — die Grenzen. Jede wird VOR der Allokation geprueft, und der
    /// Test sichert den FEHLER als Wert zu (M7), nicht die Panik.
    #[test]
    fn grenzen_werden_vor_der_allokation_geprueft() {
        let prk = [0u8; 32];
        // Laenge 0
        assert_eq!(
            expand_sha256(&prk, b"", 0),
            Err(Fehler::LaengeNull),
            "0 Byte ist nicht definiert"
        );
        // Genau auf der Grenze: erlaubt.
        let an = expand_sha256(&prk, b"", MAX_AUSGABE256).expect("genau am Maximum");
        assert_eq!(an.len(), MAX_AUSGABE256, "M7: volle Laenge");
        // Ein Byte darueber: abgelehnt.
        assert_eq!(
            expand_sha256(&prk, b"", MAX_AUSGABE256 + 1),
            Err(Fehler::ZuLang {
                gewollt: MAX_AUSGABE256 + 1,
                maximum: MAX_AUSGABE256
            })
        );
        // Und die ganz grossen Zahlen, die ein Angreifer schickt:
        for absurd in [usize::MAX, 1 << 40, 1 << 20] {
            match expand_sha256(&prk, b"", absurd) {
                Err(Fehler::ZuLang { .. }) => {}
                other => panic!("{absurd} Byte haetten abgelehnt werden muessen, kam {other:?}"),
            }
        }
        // Falsche PRK-Laenge.
        assert_eq!(
            expand_sha256(&[0u8; 31], b"", 32),
            Err(Fehler::PrkFalsch { ist: 31, soll: 32 })
        );
    }

    /// M7: die Ausgabe traegt genau die verlangte Laenge — auch wenn sie
    /// nicht auf den Block faellt (33 Byte = 32 + 1).
    #[test]
    fn ausgabe_hat_genau_die_verlangte_laenge() {
        let prk = [7u8; 32];
        for laenge in [1usize, 31, 32, 33, 64, 65, 100] {
            let o = expand_sha256(&prk, b"info", laenge).expect("erlaubt");
            assert_eq!(o.len(), laenge, "Laenge {laenge}");
        }
    }

    /// Das Label-Feld von TLS 1.3, byteweise nach RFC 8446 Abschnitt 7.1.
    ///
    /// Beispiel aus RFC 8448: Label "key", Kontext leer, Ausgabe 16 Byte.
    /// Das `length`-Feld wird von `expand_label_*` gefuellt, hier steht der
    /// Platzhalter 0.
    #[test]
    fn tls_label_feld_ist_richtig() {
        let info = label_info(b"key", b"").expect("kurzes Label");
        // byteweise: 0000 | 09 | "tls13 " | "key" | 00
        // Die Feldlaenge ist 6 (Praefix) + 3 ("key") = 9. Ich hatte hier
        // zuerst 7 eingetragen; 7 ist die UNTERGRENZE aus RFC 8446
        // (`opaque label<7..255>`), nicht der Wert fuer dieses Label.
        assert_eq!(
            hexen(&info),
            "000009746c73313320".to_owned() + "6b6579" + "00",
            "HkdfLabel-Struktur"
        );
        assert_eq!(info[2] as usize, LABEL_PRAEFIX.len() + 3, "Feldlaenge 9");
        assert_eq!(&info[3..9], LABEL_PRAEFIX, "Praefix tls13 ");
        assert_eq!(&info[9..12], b"key");
        assert_eq!(info[12], 0, "Kontext ist leer");
        assert_eq!(info.len(), 13, "M7: Gesamtfeldlaenge");
    }

    /// M3/M4: Label und Kontext von aussen sind begrenzt.
    #[test]
    fn tls_label_und_kontext_sind_begrenzt() {
        let zu_lang = vec![b'x'; MAX_LABEL + 1];
        assert!(
            label_info(&zu_lang, b"").is_err(),
            "ein Label ueber {} Byte wird abgelehnt",
            MAX_LABEL
        );
        assert!(
            label_info(&vec![b'x'; MAX_LABEL], b"").is_ok(),
            "am Maximum: erlaubt"
        );

        let kontext = vec![b'y'; MAX_KONTEXT + 1];
        assert!(label_info(b"key", &kontext).is_err(), "Kontext zu lang");
        assert!(
            label_info(b"key", &vec![b'y'; MAX_KONTEXT]).is_ok(),
            "am Maximum: erlaubt"
        );
    }

    /// M4: eine Ausgabe ueber 65535 Byte passt nicht in das `uint16` des
    /// Labels und wird abgelehnt, statt die Laenge zu beschneiden.
    #[test]
    fn expand_label_lehnt_ausgabe_ueber_65535_ab() {
        let r = expand_label_sha256(&[0u8; 32], b"key", b"", 65536);
        assert!(
            matches!(
                r,
                Err(Fehler::ZuLang {
                    gewollt: 65536,
                    maximum: 0xFFFF
                })
            ),
            "kam {r:?}"
        );
    }

    /// Die vier TLS-1.3-Labels aus RFC 8446 Abschnitt 7.1 muessen sich
    /// unterscheiden — sonst waere der Schluesselplan nicht trennbar.
    #[test]
    fn tls_labels_sind_voneinander_verschieden() {
        let g = [0x42u8; 32];
        let mut alle = Vec::new();
        for l in [b"key".as_slice(), b"iv", b"derived", b"finished"] {
            alle.push(expand_label_sha256(&g, l, b"", 32).expect("32 Byte"));
        }
        for i in 0..alle.len() {
            for j in i + 1..alle.len() {
                assert_ne!(alle[i], alle[j], "Label {i} und {j} sind gleich");
                assert_eq!(alle[i].len(), 32, "M7: Laenge");
            }
        }
    }

    /// SHA-384: die Grenzen gelten genauso, mit den SHA-384-Werten.
    #[test]
    fn sha384_grenzen() {
        let prk = [0u8; 48];
        assert_eq!(expand_sha384(&prk, b"", 0), Err(Fehler::LaengeNull));
        let an = expand_sha384(&prk, b"", MAX_AUSGABE384).expect("am Maximum");
        assert_eq!(an.len(), MAX_AUSGABE384);
        assert!(expand_sha384(&prk, b"", MAX_AUSGABE384 + 1).is_err());
        assert_eq!(
            expand_sha384(&[0u8; 32], b"", 32),
            Err(Fehler::PrkFalsch { ist: 32, soll: 48 })
        );
    }
}
