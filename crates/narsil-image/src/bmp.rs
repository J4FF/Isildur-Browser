//! BMP LESEN: Windows-Bitmap, unkomprimiert (BI_RGB), 24 und 32 Bit.
//!
//! Aufbau nach der Microsoft-Spezifikation:
//!
//! ```text
//! BITMAPFILEHEADER  14 Byte: 'BM' Groesse(4) reserviert(4) Datenversatz(4)
//! BITMAPINFOHEADER  40 Byte: Groesse(4) Breite(4) Hoehe(4) Ebenen(2)
//!                              Bits(2) Kompression(4) Bildgroesse(4) ...
//! Pixelzeilen, jede auf 4 Byte ausgerichtet
//! ```
//!
//! Drei Fallen, alle drei eingebaut:
//!   * **Negative Hoehe** bedeutet „von oben nach unten". Das ist die
//!     haeufigere Variante und die einzige, bei der die erste Zeile oben
//!     liegt. Positive Hoehe speichert das Bild auf dem Kopf.
//!   * **Zeilenpolsterung** auf 4 Byte. Ein 3-Byte-Pixel auf ungerader
//!     Breite ergibt 1 Fuellbyte je Zeile; ohne sie zerfaellt das Bild
//!     zeilenweise nach rechts.
//!   * **24 Bit ist BGR**, nicht RGB.

use crate::{Bild, BildFehler};

/// Groesster DIB-Header, den wir als „bekannt" akzeptieren (BITMAPV5).
const INFOHEADER_MAX: usize = 124;
/// Grossster DIB-Header, den wir ueberhaupt lesen (Core-Header: 12).
const INFOHEADER_MIN: usize = 12;

/// Liest eine Windows-Bitmap. Kein Panic, egal was in den Bytes steht.
pub fn bmp_lesen(daten: &[u8]) -> Result<Bild, BildFehler> {
    if daten.len() < 14 {
        return Err(BildFehler::ZuKurz {
            vorhanden: daten.len(),
            gebraucht: 14,
        });
    }
    if &daten[0..2] != b"BM" {
        return Err(BildFehler::BmpKopfFalsch {
            grund: "kein 'BM' am Anfang",
        });
    }
    let datenversatz = u32le(daten, 10)? as usize;
    let info_groesse = u32le(daten, 14)? as usize;

    if !(INFOHEADER_MIN..=INFOHEADER_MAX).contains(&info_groesse) {
        return Err(BildFehler::BmpKopfFalsch {
            grund: "DIB-Header unbekannt",
        });
    }
    if daten.len() < 14 + info_groesse {
        return Err(BildFehler::Abgeschnitten {
            position: 14,
            gebraucht: info_groesse,
        });
    }

    // BITMAPCOREHEADER (12 Byte) hat Breite/Hoehe als u16 und keine
    // Kompressionsangabe. Alles andere liest wie BITMAPINFOHEADER.
    let (breite_i, hoehe_i, bit_tiefe, kompression) = if info_groesse == 12 {
        (
            u16le(daten, 18)? as i32,
            u16le(daten, 20)? as i32,
            u16le(daten, 24)? as u16,
            0u32,
        )
    } else {
        (
            i32le(daten, 18)?,
            i32le(daten, 22)?,
            u16le(daten, 28)? as u16,
            u32le(daten, 30)?,
        )
    };

    if bit_tiefe != 24 && bit_tiefe != 32 {
        return Err(BildFehler::BmpVariante {
            grund: "nur BI_RGB mit 24 oder 32 Bit",
        });
    }
    if kompression != 0 {
        return Err(BildFehler::BmpVariante {
            grund: "nur BI_RGB, keine RLE- oder Bitfeld-Kompression",
        });
    }
    if breite_i <= 0 {
        return Err(BildFehler::BmpKopfFalsch {
            grund: "Breite nicht positiv",
        });
    }
    // `hoehe_i` kann `i32::MIN` sein; `.abs()` wuerde damit panicen. Der
    // Weg ueber `i64` ist der Grund, warum hier kein `unwrap` steht.
    let hoehe_betrag = (hoehe_i as i64).unsigned_abs();
    if hoehe_betrag == 0 {
        return Err(BildFehler::GroesseNull);
    }
    let breite = breite_i as u32;
    let hoehe = hoehe_betrag.min(u32::MAX as u64) as u32;

    if breite > crate::MAX_KANTE || hoehe > crate::MAX_KANTE {
        return Err(BildFehler::ZuGross {
            breite,
            hoehe,
            kante: crate::MAX_KANTE,
        });
    }
    if breite as u64 * hoehe as u64 > crate::MAX_BILDPUNKTE {
        return Err(BildFehler::ZuGross {
            breite,
            hoehe,
            kante: crate::MAX_BILDPUNKTE as u32,
        });
    }

    let bytes_pro_pixel = (bit_tiefe / 8) as usize;
    let ungepolstert = breite as usize * bytes_pro_pixel;
    // Auf 4 Byte auffuellen: das Bitmuster 0b...11 loescht die unteren
    // Bits und laesst die Zeile an einer Vierergrenze enden.
    let zeilenbreite = (ungepolstert + 3) & !3usize;
    let pixel_start = if datenversatz == 0 {
        14 + info_groesse
    } else {
        datenversatz
    };
    // Die Datei muss die Zeilen wirklich tragen. Ohne diese Pruefung laeuft
    // die letzte Zeile ins Leere.
    let benoetigt = pixel_start + zeilenbreite * hoehe as usize;
    if benoetigt > daten.len() {
        return Err(BildFehler::Abgeschnitten {
            position: daten.len(),
            gebraucht: benoetigt - daten.len(),
        });
    }

    let von_oben = hoehe_i < 0;
    let mut bild = Bild::new(breite, hoehe);
    for y_zeile in 0..hoehe as usize {
        // Positive Hoehe heisst: die Datei speichert die unterste Zeile
        // zuerst. Also von hinten in das Bild schreiben.
        let ziel_y = if von_oben {
            y_zeile
        } else {
            hoehe as usize - 1 - y_zeile
        };
        let start = pixel_start + y_zeile * zeilenbreite;
        for x in 0..breite as usize {
            let i = start + x * bytes_pro_pixel;
            let b = *daten.get(i).unwrap_or(&0);
            let g = *daten.get(i + 1).unwrap_or(&0);
            let r = *daten.get(i + 2).unwrap_or(&0);
            // Bei 32 Bit ist das vierte Byte laut Spezifikation unbenutzt.
            // GDI laesst dort eine Null stehen — als Alpha gelesen waere
            // jedes 32-Bit-Bild unsichtbar. Also wird Alpha 255 gesetzt.
            bild.setze_pixel(x as u32, ziel_y as u32, [r, g, b, 255]);
        }
    }
    Ok(bild)
}

/// Klein-Endian-u32 an Position `i`, ohne Panic.
fn u32le(d: &[u8], i: usize) -> Result<u32, BildFehler> {
    let s = d.get(i..i + 4).ok_or(BildFehler::ZuKurz {
        vorhanden: d.len(),
        gebraucht: i + 4,
    })?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Klein-Endian-u16 an Position `i`, ohne Panic.
fn u16le(d: &[u8], i: usize) -> Result<u16, BildFehler> {
    let s = d.get(i..i + 2).ok_or(BildFehler::ZuKurz {
        vorhanden: d.len(),
        gebraucht: i + 2,
    })?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

/// Klein-Endian-i32 an Position `i`, ohne Panic.
fn i32le(d: &[u8], i: usize) -> Result<i32, BildFehler> {
    let s = d.get(i..i + 4).ok_or(BildFehler::ZuKurz {
        vorhanden: d.len(),
        gebraucht: i + 4,
    })?;
    Ok(i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Erzeugt eine minimale 24-Bit-BMP mit `breite` x `hoehe` und einem
    /// Farbverlauf. Nur fuer die eigenen Tests.
    #[cfg_attr(not(test), allow(dead_code))]
    fn baue_bmp(breite: u32, hoehe: u32, negativ: bool) -> Vec<u8> {
        let bpp = 3usize;
        let z = (breite as usize * bpp + 3) & !3usize;
        let pixel = z * hoehe as usize;
        let versatz = 54usize;
        let mut v = Vec::new();
        v.extend_from_slice(b"BM");
        v.extend_from_slice(&((versatz + pixel) as u32).to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&(versatz as u32).to_le_bytes());
        v.extend_from_slice(&40u32.to_le_bytes());
        v.extend_from_slice(&(breite as i32).to_le_bytes());
        let h = if negativ {
            -(hoehe as i32)
        } else {
            hoehe as i32
        };
        v.extend_from_slice(&h.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // Ebenen
        v.extend_from_slice(&24u16.to_le_bytes()); // Bits
        v.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
        v.extend_from_slice(&(pixel as u32).to_le_bytes());
        v.extend_from_slice(&[0u8; 16]); // Rest des INFOHEADER
        for y in 0..hoehe as usize {
            for x in 0..breite as usize {
                v.push((x * 60) as u8);
                v.push((y * 60) as u8);
                v.push(200);
            }
            v.resize(v.len() + (z - breite as usize * bpp), 0);
        }
        v
    }

    #[test]
    fn negative_hoehe_liest_von_oben() {
        let b = baue_bmp(3, 2, true);
        let bild = bmp_lesen(&b).expect("sollte lesen");
        assert_eq!((bild.breite, bild.hoehe), (3, 2));
        // Erste Zeile ist oben: bei y=0 ist der G-Kanal 0.
        assert_eq!(bild.pixel(0, 0), [200, 0, 0, 255]);
        assert_eq!(bild.pixel(0, 1), [200, 60, 0, 255]);
    }

    #[test]
    fn positive_hoehe_liest_gespiegelt() {
        let b = baue_bmp(3, 2, false);
        let bild = bmp_lesen(&b).expect("sollte lesen");
        // Datei enthaelt unten zuerst, also ist im Bild y=0 die UNTERE Zeile.
        assert_eq!(bild.pixel(0, 1), [200, 0, 0, 255]);
    }

    #[test]
    fn zu_kurz_ist_ein_benannter_fehler() {
        let mut b = baue_bmp(3, 2, true);
        b.truncate(60);
        let e = bmp_lesen(&b).expect_err("darf nicht lesen");
        assert_eq!(e.name(), "Abgeschnitten");
    }
}
