//! QUIC-Varint (RFC 9000, Abschnitt 16).
//!
//! QUIC kodiert ganze Zahlen bis 2^62 - 1 mit einer Laenge von 1, 2, 4 oder
//! 8 Byte. Die zwei hoechstwertigen Bits des ersten Bytes geben die Laenge an:
//!   00 -> 1 Byte (Wert in den unteren 6 Bit),
//!   01 -> 2 Byte (14 Bit),
//!   10 -> 4 Byte (30 Bit),
//!   11 -> 8 Byte (62 Bit).
//!
//! M3/M4: Lesen und Schreiben laufen in einer Schleife, deren Laufzahl von der
//! VORHER festgestellten Laenge (1, 2, 4 oder 8) abhaengt, nie von einer Zahl
//! aus der Eingabe. Ein Wert ueber 2^62 - 1 wird VOR dem Schreiben abgelehnt
//! (der Fehler ist ein Wert, kein Panic).

/// Groesster darstellbarer Wert: 2^62 - 1 (RFC 9000, Abschnitt 16).
pub const MAX_WERT: u64 = (1u64 << 62) - 1;

/// Warum ein Varint nicht gelesen oder geschrieben werden kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarintFehler {
    /// Der Wert passt nicht in 62 Bit.
    ZuGross { wert: u64 },
    /// Der Puffer endet vor dem Ende des Varints.
    Abgeschnitten { braucht: usize, da: usize },
}

impl std::fmt::Display for VarintFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VarintFehler::ZuGross { wert } => {
                write!(f, "Wert {wert} passt nicht in 62 Bit")
            }
            VarintFehler::Abgeschnitten { braucht, da } => {
                write!(f, "Varint braucht {braucht} Byte, da sind {da}")
            }
        }
    }
}

impl std::error::Error for VarintFehler {}

/// Die Laenge des Varints fuer einen Wert. Der Wert wird VOR der Rueckgabe
/// gegen `MAX_WERT` geprueft (M4).
pub fn laenge(wert: u64) -> Result<usize, VarintFehler> {
    if wert > MAX_WERT {
        return Err(VarintFehler::ZuGross { wert });
    }
    if wert < (1u64 << 6) {
        Ok(1)
    } else if wert < (1u64 << 14) {
        Ok(2)
    } else if wert < (1u64 << 30) {
        Ok(4)
    } else {
        Ok(8)
    }
}

/// Schreibt einen Varint in einen Puffer.
pub fn schreibe(wert: u64, aus: &mut Vec<u8>) -> Result<(), VarintFehler> {
    match laenge(wert)? {
        1 => aus.push(wert as u8),
        2 => {
            aus.push(0x40 | ((wert >> 8) as u8));
            aus.push((wert & 0xff) as u8);
        }
        4 => {
            aus.push(0x80 | ((wert >> 24) as u8));
            aus.push(((wert >> 16) & 0xff) as u8);
            aus.push(((wert >> 8) & 0xff) as u8);
            aus.push((wert & 0xff) as u8);
        }
        8 => {
            aus.push(0xc0 | ((wert >> 56) as u8));
            for i in 1..8 {
                aus.push(((wert >> (56 - 8 * i)) & 0xff) as u8);
            }
        }
        // `laenge` liefert nur 1, 2, 4 oder 8.
        _ => unreachable!(),
    }
    Ok(())
}

/// Liest einen Varint. Liefert `(wert, verbrauchte_bytes)`, damit der
/// Aufrufer die Laenge des Feldes fuer Offsets kennt, ohne zweimal zu lesen.
pub fn lese(daten: &[u8]) -> Result<(u64, usize), VarintFehler> {
    if daten.is_empty() {
        return Err(VarintFehler::Abgeschnitten { braucht: 1, da: 0 });
    }
    let erste = daten[0];
    let len = match erste >> 6 {
        0 => 1usize,
        1 => 2,
        2 => 4,
        _ => 8,
    };
    if daten.len() < len {
        return Err(VarintFehler::Abgeschnitten {
            braucht: len,
            da: daten.len(),
        });
    }
    let wert = match len {
        1 => (erste & 0x3f) as u64,
        2 => (((erste & 0x3f) as u64) << 8) | daten[1] as u64,
        4 => {
            (((erste & 0x3f) as u64) << 24)
                | ((daten[1] as u64) << 16)
                | ((daten[2] as u64) << 8)
                | daten[3] as u64
        }
        8 => {
            let mut w = ((erste & 0x3f) as u64) << 56;
            for (i, &b) in daten[1..8].iter().enumerate() {
                w |= (b as u64) << (48 - 8 * i);
            }
            w
        }
        _ => unreachable!(),
    };
    Ok((wert, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9000, Abschnitt 16: die Beispielwerte samt ihrer Kodierung.
    #[test]
    fn rfc9000_beispielwerte() {
        // Wert -> erwartete Bytes.
        let faelle: &[(u64, &[u8])] = &[
            (0, &[0x00]),
            (63, &[0x3f]),
            (64, &[0x40, 0x40]),
            (
                151_288_809_941_952_652,
                &[0xc2, 0x19, 0x7c, 0x5e, 0xff, 0x14, 0xe8, 0x8c],
            ),
            (MAX_WERT, &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]),
        ];
        for (wert, bytes) in faelle {
            let mut p = Vec::new();
            schreibe(*wert, &mut p).expect("Schreiben");
            assert_eq!(&p[..], *bytes, "Wert {wert} kodiert");
            assert_eq!(laenge(*wert).unwrap(), bytes.len(), "Laenge von {wert}");
            let (gelesen, n) = lese(&p).expect("Lesen");
            assert_eq!(gelesen, *wert, "Rundlauf {wert}");
            assert_eq!(n, bytes.len(), "verbrauchte Bytes {wert}");
        }
    }

    /// M7: die vier Laengenklassen an ihren Grenzen (1, 2, 4, 8 Byte).
    #[test]
    fn laengenklassen_an_den_grenzen() {
        for (wert, soll) in [
            (0u64, 1usize),
            (63, 1),
            (64, 2),
            (16383, 2),
            (16384, 4),
            ((1 << 30) - 1, 4),
            (1 << 30, 8),
            (MAX_WERT, 8),
        ] {
            assert_eq!(laenge(wert).unwrap(), soll, "Laenge {wert}");
            let mut p = Vec::new();
            schreibe(wert, &mut p).unwrap();
            assert_eq!(p.len(), soll, "Kodierte Laenge {wert}");
            assert_eq!(lese(&p).unwrap(), (wert, soll), "Rundlauf {wert}");
        }
    }

    /// M4: Werte ueber 2^62 - 1 werden VOR der Allokation abgelehnt.
    #[test]
    fn zu_grosse_werte_sind_ein_benannter_fehler() {
        for wert in [(1u64 << 62), (1u64 << 63), u64::MAX] {
            assert_eq!(
                laenge(wert),
                Err(VarintFehler::ZuGross { wert }),
                "Wert {wert}"
            );
            let mut p = Vec::new();
            assert_eq!(schreibe(wert, &mut p), Err(VarintFehler::ZuGross { wert }));
            assert!(p.is_empty(), "M7: nichts geschrieben bei Fehler");
        }
    }

    /// M4: ein abgeschnittenes Varint an jeder moeglichen Stelle.
    #[test]
    fn abgeschnitten_ist_ein_benannter_fehler() {
        // 8-Byte-Varint, das mitten im Feld endet.
        let voll = [0xc2, 0x19, 0x7c, 0x5e, 0xff, 0x14, 0xe8, 0x8c];
        for kuerzen in 1..=8 {
            let r = lese(&voll[..8 - kuerzen]);
            assert!(
                r.is_err(),
                "M7: {kuerzen} Byte abgeschnitten muss ein Fehler sein, kam {r:?}"
            );
        }
        // Leerer Puffer.
        assert_eq!(
            lese(&[]),
            Err(VarintFehler::Abgeschnitten { braucht: 1, da: 0 })
        );
        // Ein-Byte-Varint, das auf ein 2-Byte-Varint zeigt.
        assert_eq!(
            lese(&[0x40]),
            Err(VarintFehler::Abgeschnitten { braucht: 2, da: 1 })
        );
    }
}
