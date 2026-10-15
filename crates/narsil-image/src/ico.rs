//! ICO LESEN: Windows-Icon (Favicons).
//!
//! Ein Favicon ist keine Datei, sondern ein Behaelter: die `.ico` enthaelt
//! mehrere Bilder in verschiedenen Groessen, und der Browser nimmt das, was
//! zu seiner Anzeigegroesse passt (16x16 im Tab, 32x32 im Lesezeichen,
//! 64x64 oder 256x256 auf dem Desktop).
//!
//! Zwei Formen kommen im echten Web vor:
//!
//! 1. **BMP/DIB** (ueberall): ein `BITMAPINFOHEADER` mit doppelter Hoehe
//!    (Bild + AND-Maske), darunter die Bilddaten BGRA in Zeilen von unten
//!    nach oben, darunter die Maske mit 1 Bit je Pixel.
//! 2. **PNG** (ab Windows Vista): der Eintrag enthaelt eine vollstaendige
//!    PNG-Datei. GEMESSEN 30.09.2026: Pillow 11.1.0 schreibt in ein `.ico`
//!    grundsaetzlich PNG-Eintraege — auch mit `format="ICO"`.
//!
//! Beide werden hier gelesen. Fuer PNG wird der vorhandene PNG-Leser
//! benutzt ([`crate::png_lesen`]) — das ist eine echte Wiederverwendung und
//! kein zweiter PNG-Leser.
//!
//! # Grenzen (M3, M4)
//!
//! * [`MAX_EINTRAEGE`] = 64 Bilder je Datei. GEMESSEN: die groessten
//!   Icons der Welt (Windows Vista) haben 6.
//! * Alle Groessen kommen aus `u8` im Verzeichnis (oder 256 fuer 0), die
//!   Gesamtgroesse also hoechstens 65.536 Punkte je Bild — aber die
//!   Bilddaten muessen wirklich in der Datei stehen, das wird geprueft.
use crate::{Bild, BildFehler, MAX_KANTE};

/// Hoechste Zahl Eintraege in einer ICO-Datei (M3).
pub const MAX_EINTRAEGE: usize = 64;

/// Was fuer einen einzelnen Eintrag in der Datei steht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EintragsArt {
    /// Ein PNG-Bild direkt im Eintrag.
    Png,
    /// Ein DIB: BITMAPINFOHEADER + BGRA + AND-Maske.
    Dib,
}

/// Ein Eintrag im Verzeichnis einer ICO-Datei.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Eintrag {
    /// Angeforderte Breite (256, wenn das Byte 0 ist).
    pub breite: u32,
    /// Angeforderte Hoehe.
    pub hoehe: u32,
    /// Anzahl Farben aus dem Verzeichnis (`0` = mehr als 256 / keine Angabe).
    pub farben: u16,
    /// Groesse der Bilddaten in Byte.
    pub daten_groesse: u32,
    /// Wo die Bilddaten in der Datei anfangen.
    pub versatz: u32,
    /// Was in den Bilddaten steht.
    pub art: EintragsArt,
}

impl Eintrag {
    /// Die Flaeche des Eintrags in Punkten.
    pub fn punkte(&self) -> u64 {
        self.breite as u64 * self.hoehe as u64
    }
}

/// Liest das Verzeichnis einer ICO-Datei, ohne ein Bild zu dekodieren.
///
/// Das ist der billige Teil: 6 Byte plus 16 Byte je Eintrag. Ein Browser
/// braucht das, um zu entscheiden, welches Bild er ueberhaupt liest.
pub fn ico_verzeichnis(daten: &[u8]) -> Result<Vec<Eintrag>, BildFehler> {
    if daten.len() < 6 {
        return Err(BildFehler::ZuKurz {
            vorhanden: daten.len(),
            gebraucht: 6,
        });
    }
    let reserviert = u16::from_le_bytes([daten[0], daten[1]]);
    let typ = u16::from_le_bytes([daten[2], daten[3]]);
    let anzahl = u16::from_le_bytes([daten[4], daten[5]]) as usize;

    // GEMESSEN: reserviert 0 und Typ 1 (Icon). Typ 2 ist ein Cursor
    // (.cur), das ist ein anderes Format mit einem 4-Byte-Hotspot.
    if reserviert != 0 {
        return Err(BildFehler::IcoKopfFalsch {
            grund: "reserved ist nicht 0",
        });
    }
    if typ != 1 {
        return Err(BildFehler::IcoKopfFalsch {
            grund: "typ ist nicht 1 (Cursor wird nicht gelesen)",
        });
    }
    if anzahl == 0 {
        return Err(BildFehler::IcoKopfFalsch {
            grund: "keine Eintraege",
        });
    }
    // M3: Grenze VOR dem Lesen des Verzeichnisses.
    if anzahl > MAX_EINTRAEGE {
        return Err(BildFehler::IcoKopfFalsch {
            grund: "mehr Eintraege als MAX_EINTRAEGE",
        });
    }
    // M4: das Verzeichnis muss wirklich in der Datei stehen.
    if 6 + anzahl * 16 > daten.len() {
        return Err(BildFehler::Abgeschnitten {
            position: 6,
            gebraucht: anzahl * 16,
        });
    }

    let mut raus = Vec::with_capacity(anzahl);
    for i in 0..anzahl {
        let p = 6 + i * 16;
        let b = daten[p];
        let h = daten[p + 1];
        let farben = daten[p + 2];
        let daten_groesse =
            u32::from_le_bytes([daten[p + 8], daten[p + 9], daten[p + 10], daten[p + 11]]);
        let versatz =
            u32::from_le_bytes([daten[p + 12], daten[p + 13], daten[p + 14], daten[p + 15]]);

        // GEMESSEN 30.09.2026: das Byte 0 steht fuer 256, nicht fuer 0.
        // Ohne diese Regel waere ein 256x256-Icon ein 0x0-Bild.
        let breite = if b == 0 { 256 } else { b as u32 };
        let hoehe = if h == 0 { 256 } else { h as u32 };

        // M4: der Eintrag muss in der Datei liegen.
        let ende = versatz as u64 + daten_groesse as u64;
        if ende > daten.len() as u64 {
            return Err(BildFehler::Abgeschnitten {
                position: versatz as usize,
                gebraucht: daten_groesse as usize,
            });
        }
        if breite > MAX_KANTE || hoehe > MAX_KANTE {
            return Err(BildFehler::ZuGross {
                breite,
                hoehe,
                kante: MAX_KANTE,
            });
        }

        // Die Art steht nicht im Verzeichnis; sie ergibt sich aus den
        // ersten acht Byte der Bilddaten (PNG-Signatur oder
        // BITMAPINFOHEADER-Groesse 40).
        let art = if ende - versatz as u64 >= 8
            && daten[versatz as usize..versatz as usize + 8]
                == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        {
            EintragsArt::Png
        } else if ende - versatz as u64 >= 4
            && u32::from_le_bytes([
                daten[versatz as usize],
                daten[versatz as usize + 1],
                daten[versatz as usize + 2],
                daten[versatz as usize + 3],
            ]) == 40
        {
            EintragsArt::Dib
        } else {
            return Err(BildFehler::IcoKopfFalsch {
                grund: "Bilddaten sind weder PNG noch BITMAPINFOHEADER",
            });
        };

        raus.push(Eintrag {
            breite,
            hoehe,
            farben: farben as u16,
            daten_groesse,
            versatz,
            art,
        });
    }
    Ok(raus)
}

/// Liest das GROESSTE Bild einer ICO-Datei.
///
/// Das ist die Form, die die Crate schon hatte. Ein Browser, der ein
/// Favicon in einem Tab anzeigt, braucht meist genau das; wer eine
/// bestimmte Groesse will, nimmt [`ico_lesen_groesse`].
///
/// GEMESSEN: „groesstes Bild" heisst groesste Flaeche, nicht groesste
/// Kantenlaenge — ein 32x32-Bild hat mehr Punkte als ein 64x64-Bild, das
/// nicht existiert, aber ein 256x1 waere ein Streifen. Beide Kriterien
/// werden nebeneinander geprueft: Flaeche zuerst, dann Kante.
pub fn ico_lesen(daten: &[u8]) -> Result<Bild, BildFehler> {
    let eintraege = ico_verzeichnis(daten)?;
    // Das groesste Bild zuerst probieren: ein Fehler in einem kleinen
    // Eintrag darf den grossen nicht verhindern.
    let mut sortiert = eintraege.clone();
    sortiert.sort_by_key(|e| (e.punkte(), e.breite + e.hoehe));
    sortiert.reverse();
    let mut letzter: Option<BildFehler> = None;
    for e in &sortiert {
        match ico_eintrag_lesen(daten, e) {
            Ok(b) => return Ok(b),
            Err(f) => letzter = Some(f),
        }
    }
    Err(letzter.unwrap_or(BildFehler::IcoKopfFalsch {
        grund: "kein lesbarer Eintrag",
    }))
}

/// Liest ein Bild einer bestimmten Kantenlaenge.
pub fn ico_lesen_groesse(daten: &[u8], kante: u32) -> Result<Bild, BildFehler> {
    let eintraege = ico_verzeichnis(daten)?;
    for e in &eintraege {
        if e.breite == kante && e.hoehe == kante {
            return ico_eintrag_lesen(daten, e);
        }
    }
    Err(BildFehler::IcoKopfFalsch {
        grund: "kein Eintrag mit dieser Groesse",
    })
}

/// Liest einen bestimmten Eintrag.
pub fn ico_eintrag_lesen(daten: &[u8], eintrag: &Eintrag) -> Result<Bild, BildFehler> {
    let von = eintrag.versatz as usize;
    let bis = von + eintrag.daten_groesse as usize;
    let blob = daten.get(von..bis).ok_or(BildFehler::Abgeschnitten {
        position: von,
        gebraucht: eintrag.daten_groesse as usize,
    })?;
    match eintrag.art {
        EintragsArt::Png => {
            // Wiederverwendung: der PNG-Leser dieser Crate. Kein zweiter.
            let mut b = crate::png_lesen(blob)?;
            // Das Verzeichnis kann eine andere Groesse behaupten als das
            // PNG enthaelt; das PNG gewinnt, weil es die echten Punkte hat.
            if b.breite == 0 || b.hoehe == 0 {
                b = Bild::new(eintrag.breite, eintrag.hoehe);
            }
            Ok(b)
        }
        EintragsArt::Dib => dib_lesen(blob, eintrag.breite, eintrag.hoehe),
    }
}

/// Liest ein DIB: BITMAPINFOHEADER + BGRA + AND-Maske.
///
/// GEMESSEN 30.09.2026 an `ico_bmp.ico`, von Pillow gebaut:
///
/// | Groesse | Header | BITMAPINFOHEADER | Bildbytes | Maskenbytes |
/// |---------|--------|------------------|------------|-------------|
/// |   16x16 |     40 |               1128 |         512 |           64 |
/// |   32x32 |     40 |               4264 |        4096 |          128 |
///
/// Zwei Faelle, die man leicht falsch macht:
/// * Die **Hoehe im Header ist doppelt** (Bild + Maske). Wer sie als
///   Bildhoehe nimmt, liest Maskenbytes als Pixel.
/// * Die Bildzeilen laufen **von unten nach oben**, die Maske ebenso.
fn dib_lesen(blob: &[u8], breite: u32, hoehe: u32) -> Result<Bild, BildFehler> {
    if blob.len() < 40 {
        return Err(BildFehler::Abgeschnitten {
            position: 0,
            gebraucht: 40,
        });
    }
    let header_groesse = u32::from_le_bytes([blob[0], blob[1], blob[2], blob[3]]);
    if header_groesse != 40 {
        return Err(BildFehler::IcoKopfFalsch {
            grund: "BITMAPINFOHEADER ist nicht 40 Byte",
        });
    }
    let bmp_breite = i32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]);
    let bmp_hoehe = i32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]);
    let bit_tiefe = u16::from_le_bytes([blob[14], blob[15]]);
    let kompression = u32::from_le_bytes([blob[16], blob[17], blob[18], blob[19]]);

    if kompression != 0 {
        return Err(BildFehler::IcoKopfFalsch {
            grund: "DIB mit Kompression wird nicht gelesen",
        });
    }
    if bit_tiefe != 32 {
        return Err(BildFehler::IcoKopfFalsch {
            grund: "nur 32 Bit je Pixel im DIB",
        });
    }
    // Doppelte Hoehe: die untere Haelfte ist die AND-Maske.
    if bmp_hoehe % 2 != 0 || bmp_breite <= 0 || bmp_hoehe < 2 {
        return Err(BildFehler::IcoKopfFalsch {
            grund: "DIB-Hoehe ist nicht gerade (Bild + Maske)",
        });
    }
    let bild_hoehe = (bmp_hoehe / 2) as u32;
    let w = bmp_breite as u32;
    if bild_hoehe != hoehe || w != breite {
        return Err(BildFehler::IcoKopfFalsch {
            grund: "DIB-Groesse passt nicht zum Verzeichnis",
        });
    }

    // M4: Bildbytes und Maskenbytes muessen in den Blob passen.
    let bild_bytes = w as usize * bild_hoehe as usize * 4;
    // Die AND-Maske hat 1 Bit je Pixel, aber ihre Zeilen muessen auf
    // 4 Byte ausgerichtet sein (Padding) — sonst waeren die Zeilen
    // versetzt. GEMESSEN: 16 -> 4 Byte, 32 -> 4 Byte, 1 -> 4 Byte.
    let maske_zeilen = 4 * (w as usize).div_ceil(32);
    let maske_bytes = maske_zeilen * bild_hoehe as usize;
    let xor_start = 40usize;
    let maske_start = xor_start + bild_bytes;
    if maske_start + maske_bytes > blob.len() {
        return Err(BildFehler::Abgeschnitten {
            position: maske_start,
            gebraucht: maske_bytes,
        });
    }

    let mut bild = Bild::new(w, bild_hoehe);
    for y in 0..bild_hoehe {
        // Von unten nach oben.
        let ziel_y = bild_hoehe - 1 - y;
        for x in 0..w {
            let p = xor_start + (y as usize * w as usize + x as usize) * 4;
            let b = blob[p];
            let g = blob[p + 1];
            let r = blob[p + 2];
            let a = blob[p + 3];

            // Die AND-Maske zaehlt nur dann, wenn der Alphakanal 0 ist.
            // GEMESSEN: Pillow schreibt bei einem transparenten Rand Alpha
            // 0 UND ein gesetztes Maskenbit — es gibt also beides, und die
            // Maske darf ein vorhandenes Alpha nicht ueberschreiben.
            let mut alpha = a;
            if alpha == 0 {
                // Die Maske liegt unter dem Bild, ebenfalls von unten
                // nach oben, 1 Bit je Pixel, Zeilen auf 4 Byte
                // ausgerichtet. `y` ist hier schon der Index von unten,
                // also direkt die Maskenzeile.
                let zeile = y as usize * maske_zeilen;
                let bit = maske_start + zeile + (x as usize / 8);
                if bit < blob.len() && (blob[bit] & (0x80 >> (x % 8))) != 0 {
                    alpha = 255;
                }
            }
            bild.setze_pixel(x, ziel_y, [r, g, b, alpha]);
        }
    }
    Ok(bild)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verzeichnis-Byte-Kopf: reserved=0, typ=1, anzahl.
    fn kopf(anzahl: u16) -> Vec<u8> {
        let mut v = vec![0, 0, 1, 0];
        v.extend_from_slice(&anzahl.to_le_bytes());
        v
    }

    /// Ein Verzeichniseintrag.
    fn eintrag(w: u8, h: u8, farben: u8, groesse: u32, versatz: u32) -> Vec<u8> {
        let mut v = vec![w, h, farben, 0];
        v.extend_from_slice(&1u16.to_le_bytes()); // planes
        v.extend_from_slice(&32u16.to_le_bytes()); // bit count
        v.extend_from_slice(&groesse.to_le_bytes());
        v.extend_from_slice(&versatz.to_le_bytes());
        v
    }

    /// Ein minimales, gueltiges PNG (1x1, rot).
    fn mini_png() -> Vec<u8> {
        crate::png_schreiben_modus(
            &Bild::aus_farbe(1, 1, [255, 0, 0, 255]),
            crate::Farbwahl::Rgba,
        )
    }

    #[test]
    fn leerer_verzeichnis_fehler() {
        assert_eq!(ico_verzeichnis(&[]).unwrap_err().name(), "ZuKurz");
    }

    #[test]
    fn reserviert_nicht_null_ist_fehler() {
        let mut d = kopf(1);
        d[0] = 1;
        assert_eq!(ico_verzeichnis(&d).unwrap_err().name(), "IcoKopfFalsch");
    }

    #[test]
    fn cursor_ist_fehler() {
        let mut d = kopf(1);
        d[3] = 2;
        assert_eq!(ico_verzeichnis(&d).unwrap_err().name(), "IcoKopfFalsch");
    }

    #[test]
    fn null_eintraege_ist_fehler() {
        assert_eq!(
            ico_verzeichnis(&kopf(0)).unwrap_err().name(),
            "IcoKopfFalsch"
        );
    }

    #[test]
    fn zu_viele_eintraege_ist_fehler() {
        // M3: MAX_EINTRAEGE + 1 wird abgewiesen, BEVOR gelesen wird.
        let mut d = kopf((MAX_EINTRAEGE + 1) as u16);
        d.extend(std::iter::repeat(0).take((MAX_EINTRAEGE + 1) * 16));
        assert_eq!(ico_verzeichnis(&d).unwrap_err().name(), "IcoKopfFalsch");
        assert_eq!(MAX_EINTRAEGE, 64);
    }

    #[test]
    fn abgeschnittenes_verzeichnis_ist_fehler() {
        let mut d = kopf(2);
        d.extend_from_slice(&eintrag(16, 16, 0, 10, 40));
        // Das zweite Eintrag fehlt.
        assert_eq!(ico_verzeichnis(&d).unwrap_err().name(), "Abgeschnitten");
    }

    #[test]
    fn eintrag_ragt_ueber_die_datei() {
        let png = mini_png();
        let mut d = kopf(1);
        d.extend_from_slice(&eintrag(1, 1, 0, 10_000, 22));
        d.extend_from_slice(&png);
        assert_eq!(ico_verzeichnis(&d).unwrap_err().name(), "Abgeschnitten");
    }

    #[test]
    fn byte_null_bedeutet_256() {
        // GEMESSEN: das Verzeichnis speichert 256 als Byte 0.
        let png = mini_png();
        let mut d = kopf(1);
        d.extend_from_slice(&eintrag(0, 0, 0, png.len() as u32, 22));
        d.extend_from_slice(&png);
        let v = ico_verzeichnis(&d).expect("liest");
        assert_eq!((v[0].breite, v[0].hoehe), (256, 256));
    }

    #[test]
    fn png_eintrag_wird_erkannt() {
        let png = mini_png();
        let mut d = kopf(1);
        d.extend_from_slice(&eintrag(1, 1, 0, png.len() as u32, 22));
        d.extend_from_slice(&png);
        let v = ico_verzeichnis(&d).expect("liest");
        assert_eq!(v[0].art, EintragsArt::Png);
        let b = ico_lesen(&d).expect("liest");
        assert_eq!((b.breite, b.hoehe), (1, 1));
        assert_eq!(b.pixel(0, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn dib_kopf_muss_40_sein() {
        let mut dib = vec![0u8; 40];
        dib[0] = 12; // BITMAPCOREHEADER statt BITMAPINFOHEADER
        let mut d = kopf(1);
        d.extend_from_slice(&eintrag(1, 1, 0, 40, 22));
        d.extend_from_slice(&dib);
        assert_eq!(ico_lesen(&d).unwrap_err().name(), "IcoKopfFalsch");
    }

    #[test]
    fn dib_mit_kompression_ist_fehler() {
        let mut dib = vec![0u8; 44];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[16..20].copy_from_slice(&1u32.to_le_bytes()); // BI_RLE8
        let mut d = kopf(1);
        d.extend_from_slice(&eintrag(1, 1, 0, 44, 22));
        d.extend_from_slice(&dib);
        let e = ico_lesen(&d).unwrap_err();
        assert_eq!(e.name(), "IcoKopfFalsch");
    }

    #[test]
    fn dib_1x1_rot() {
        // BITMAPINFOHEADER + 1x1 BGRA + 4 Byte AND-Maske
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&1i32.to_le_bytes()); // Breite
        dib.extend_from_slice(&2i32.to_le_bytes()); // Hoehe = Bild + Maske
        dib.extend_from_slice(&1u16.to_le_bytes()); // planes
        dib.extend_from_slice(&32u16.to_le_bytes()); // bit count
        dib.extend_from_slice(&0u32.to_le_bytes()); // compression
        dib.extend_from_slice(&8u32.to_le_bytes()); // bildbytes
        dib.extend_from_slice(&[0u8; 16]);
        dib.extend_from_slice(&[0, 128, 0, 255]); // BGRA: gruen, alpha 255
        dib.extend_from_slice(&[0, 0, 0, 0]); // AND-Maske: nichts gesetzt
        let mut d = kopf(1);
        d.extend_from_slice(&eintrag(1, 1, 0, dib.len() as u32, 22));
        d.extend_from_slice(&dib);
        let b = ico_lesen(&d).expect("liest");
        assert_eq!(b.pixel(0, 0), [0, 128, 0, 255]);
    }

    #[test]
    fn dib_1x1_transparent() {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&1i32.to_le_bytes());
        dib.extend_from_slice(&2i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&8u32.to_le_bytes());
        dib.extend_from_slice(&[0u8; 16]);
        dib.extend_from_slice(&[255, 0, 0, 0]); // BGRA: rot, ALPHA 0
        dib.extend_from_slice(&[0, 0, 0, 0]); // Maske: nichts
        let mut d = kopf(1);
        d.extend_from_slice(&eintrag(1, 1, 0, dib.len() as u32, 22));
        d.extend_from_slice(&dib);
        let b = ico_lesen(&d).expect("liest");
        assert_eq!(b.pixel(0, 0)[3], 0, "alpha 0 heisst durchsichtig");
    }

    #[test]
    fn dib_maske_macht_undurchsichtig() {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&1i32.to_le_bytes());
        dib.extend_from_slice(&2i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&8u32.to_le_bytes());
        dib.extend_from_slice(&[0u8; 16]);
        dib.extend_from_slice(&[255, 0, 0, 0]); // Alpha 0
        dib.extend_from_slice(&[0x80, 0, 0, 0]); // Maskenbit 0 gesetzt
        let mut d = kopf(1);
        d.extend_from_slice(&eintrag(1, 1, 0, dib.len() as u32, 22));
        d.extend_from_slice(&dib);
        let b = ico_lesen(&d).expect("liest");
        assert_eq!(b.pixel(0, 0)[3], 255, "Maskenbit macht undurchsichtig");
    }

    #[test]
    fn dib_abgeschnitten_ist_fehler() {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&8i32.to_le_bytes()); // 8x8
        dib.extend_from_slice(&16i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&[0u8; 16]);
        // Bildbytes fehlen.
        let mut d = kopf(1);
        d.extend_from_slice(&eintrag(8, 8, 0, dib.len() as u32, 22));
        d.extend_from_slice(&dib);
        assert_eq!(ico_lesen(&d).unwrap_err().name(), "Abgeschnitten");
    }

    #[test]
    fn groesstes_bild_wird_genommen() {
        // Zwei PNG-Eintraege: klein und gross. `ico_lesen` liefert das
        // groessere.
        let klein = crate::png_schreiben_modus(
            &Bild::aus_farbe(4, 4, [1, 2, 3, 255]),
            crate::Farbwahl::Rgba,
        );
        let gross = crate::png_schreiben_modus(
            &Bild::aus_farbe(16, 16, [9, 8, 7, 255]),
            crate::Farbwahl::Rgba,
        );
        let mut d = kopf(2);
        let p1 = 6 + 32;
        d.extend_from_slice(&eintrag(4, 4, 0, klein.len() as u32, p1 as u32));
        d.extend_from_slice(&eintrag(
            16,
            16,
            0,
            gross.len() as u32,
            (p1 + klein.len()) as u32,
        ));
        d.extend_from_slice(&klein);
        d.extend_from_slice(&gross);
        let b = ico_lesen(&d).expect("liest");
        assert_eq!((b.breite, b.hoehe), (16, 16), "das groessere Bild");
        // Und gezielt lesen geht auch.
        let klein_b = ico_lesen_groesse(&d, 4).expect("liest 4x4");
        assert_eq!((klein_b.breite, klein_b.hoehe), (4, 4));
    }

    #[test]
    fn groesse_nicht_vorhanden_ist_fehler() {
        let png = mini_png();
        let mut d = kopf(1);
        d.extend_from_slice(&eintrag(1, 1, 0, png.len() as u32, 22));
        d.extend_from_slice(&png);
        assert_eq!(
            ico_lesen_groesse(&d, 48).unwrap_err().name(),
            "IcoKopfFalsch"
        );
    }

    #[test]
    fn unbekannte_bilddaten_ist_fehler() {
        // Weder PNG-Signatur noch Header 40.
        let mut d = kopf(1);
        d.extend_from_slice(&eintrag(1, 1, 0, 16, 22));
        d.extend_from_slice(&[0xAAu8; 16]);
        assert_eq!(ico_lesen(&d).unwrap_err().name(), "IcoKopfFalsch");
    }
}
