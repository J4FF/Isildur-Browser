//! PPM LESEN: P1-P6.
//!
//! Eine kleine, abgeschlossene Spezifikation (die Netpbm-Familie) und ein
//! Format, das in freier Wildbahn vorkommt. Zwei Besonderheiten, an denen
//! man sich sonst festbeisst:
//!
//!   * P1 (ASCII-Bitmap) hat **umgekehrte** Bedeutung: 1 ist SCHWARZ, 0
//!     ist WEISS. In P2/P3 ist 0 schwarz. Das ist historisch und nicht
//!     konfigurierbar.
//!   * Kommentare beginnen mit `#` und laufen bis zum Zeilenende — auch
//!     mitten in der Zahlenfolge zwischen zwei Werten.
//!
//! Die binaeren Formen P4/P5/P6 haben nach dem letzten Headerwert GENAU EIN
//! Leerzeichen und dann sofort die Rohbytes. Wer dieses Leerzeichen auch
//! noch frisst, verliert ein Byte pro Zeile und sieht ab der zweiten Zeile
//! Farbstreifen.

use crate::{Bild, BildFehler};

/// Liest ein PPM/PGM/PBM in jeder der sechs Formen.
pub fn ppm_lesen(daten: &[u8]) -> Result<Bild, BildFehler> {
    if daten.len() < 2 {
        return Err(BildFehler::ZuKurz {
            vorhanden: daten.len(),
            gebraucht: 2,
        });
    }
    if daten[0] != b'P' {
        return Err(BildFehler::PpmKopfFalsch { gefunden: daten[0] });
    }
    let form = daten[1];
    let ascii = match form {
        b'1' | b'2' | b'3' => true,
        b'4' | b'5' | b'6' => false,
        _ => {
            return Err(BildFehler::PpmKopfFalsch { gefunden: form });
        }
    };

    let mut l = Lexer { daten, pos: 2 };
    let breite = l.zahl()?;
    let hoehe = l.zahl()?;

    // P1/P4 (Bitmap) haben kein maxval. Es gibt dann nur 0 und 1.
    let maxval = match form {
        b'1' | b'4' => 1u32,
        _ => l.zahl()?,
    };
    if maxval == 0 || maxval > 65535 {
        return Err(BildFehler::PpmMaxvalFalsch { maxval });
    }
    if breite == 0 || hoehe == 0 {
        return Err(BildFehler::GroesseNull);
    }
    if breite > crate::MAX_KANTE || hoehe > crate::MAX_KANTE {
        return Err(BildFehler::ZuGross {
            breite,
            hoehe,
            kante: crate::MAX_KANTE,
        });
    }
    let punkte = breite as u64 * hoehe as u64;
    if punkte > crate::MAX_BILDPUNKTE {
        return Err(BildFehler::ZuGross {
            breite,
            hoehe,
            kante: crate::MAX_BILDPUNKTE as u32,
        });
    }

    let mut bild = Bild::new(breite, hoehe);
    // P3 hat drei Kanäle, P2/P5 einen, P1/P4 ebenfalls einen.
    let kanaele: usize = match form {
        b'3' | b'6' => 3,
        _ => 1,
    };
    let bytes_pro_wert = if maxval > 255 { 2 } else { 1 };

    if ascii {
        for y in 0..hoehe {
            for x in 0..breite {
                let mut v = [0u8; 3];
                for k in 0..kanaele {
                    let wert = l.zahl()?;
                    v[k] = skaliere(wert, maxval);
                }
                if kanaele == 1 {
                    // P1: 1 heisst SCHWARZ. P2: 0 heisst schwarz.
                    let g = if form == b'1' {
                        if v[0] > 0 { 0 } else { 255 }
                    } else {
                        v[0]
                    };
                    bild.setze_pixel(x, y, [g, g, g, 255]);
                } else {
                    bild.setze_pixel(x, y, [v[0], v[1], v[2], 255]);
                }
            }
        }
    } else {
        // Nach dem Headerwert folgt bei P4/P5/P6 genau ein Leerzeichen.
        l.trenne_binardaten();
        // NUR P4 ist bitgepackt (ein Bit je Pixel). P5 und P6 sind
        // BYTE-ausgerichtet: eine Zeile aus 2 Pixeln in P6 ist 2*3 = 6
        // Byte, nicht 6/8.
        //
        // GEFUNDEN VOM TEST `ppm_zu_kurz_ergibt_abgeschnitten`: mit dem
        // pauschalen `/8` wurde aus 6 Byte der Zeilenlaenge 1 Byte, der
        // Leser las weiter und PANIKTE mit Index out of bounds. Genau die
        // Sorte Fehler, die dieser Test suchen sollte — eine fremde Datei,
        // die den Tab beendet.
        let zeilenbyte = if form == b'4' {
            ((breite as usize * kanaele + 7) / 8) as usize
        } else {
            breite as usize * kanaele * bytes_pro_wert
        };
        for y in 0..hoehe {
            let zeile = l.byte_slice(zeilenbyte)?;
            for x in 0..breite as usize {
                let px = pixel_aus_zeile(zeile, x, kanaele, bytes_pro_wert, maxval);
                if kanaele == 1 {
                    let g = if form == b'4' {
                        // Wieder die Umkehrung, diesmal binaer: Bit 1 = schwarz.
                        if px[0] != 0 { 0 } else { 255 }
                    } else {
                        px[0]
                    };
                    bild.setze_pixel(x as u32, y, [g, g, g, 255]);
                } else {
                    bild.setze_pixel(x as u32, y, [px[0], px[1], px[2], 255]);
                }
            }
        }
    }
    Ok(bild)
}

fn pixel_aus_zeile(zeile: &[u8], x: usize, kanaele: usize, b: usize, maxval: u32) -> [u8; 3] {
    if kanaele == 1 {
        if maxval == 1 {
            // P4: ein Bit je Pixel, die Zeile ist auf Bytes gepolstert.
            let bit = (zeile[x / 8] >> (7 - (x % 8))) & 1;
            [bit * 255, bit * 255, bit * 255]
        } else if b == 1 {
            // ACHTUNG, HIER LAG EIN FEHLER (gefunden vom Test
            // `p5_mit_kleinem_maxval`): der Rohwert wurde unveraendert
            // zurueckgegeben. Bei maxval 15 ergibt 0x0F dann 15 statt 255,
            // und ein Grauverlauf ist um den Faktor 17 zu dunkel. Die
            // Skalierung auf maxval ist Pflicht, nicht Kosmetik.
            let g = skaliere(zeile[x] as u32, maxval);
            [g, g, g]
        } else {
            let g = skaliere(be16(&zeile[x * 2..x * 2 + 2]), maxval);
            [g, g, g]
        }
    } else if b == 1 {
        // P6 mit maxval < 255: ebenfalls skalieren, aus demselben Grund.
        [
            skaliere(zeile[x * 3] as u32, maxval),
            skaliere(zeile[x * 3 + 1] as u32, maxval),
            skaliere(zeile[x * 3 + 2] as u32, maxval),
        ]
    } else {
        // 16 Bit pro Kanal, GROSSES Byte zuerst (Netpbm ist Big-Endian),
        // und dann wirklich skaliert statt „oberes Byte nehmen".
        //
        // ACHTUNG, HIER LAG EIN FEHLER (gefunden vom Test
        // `p6_16bit_nimmt_die_oberen_byte`): es wurden die KLEINEN Byte
        // genommen (x*6+1 statt x*6+0). 0x1234 wurde als 0x34 = 52 gelesen
        // statt als 0x12 = 18 — das Bild sah aus wie ein Negativ mit
        // vertauschten Kanaelen. Das Skalieren statt Abschneiden ist
        // ausserdem fuer JEDES maxval richtig, nicht nur fuer 65535.
        [
            skaliere(be16(&zeile[x * 6..x * 6 + 2]), maxval),
            skaliere(be16(&zeile[x * 6 + 2..x * 6 + 4]), maxval),
            skaliere(be16(&zeile[x * 6 + 4..x * 6 + 6]), maxval),
        ]
    }
}

/// Liest zwei Byte gross-endian. Ohne `unwrap`, weil `zeile` aus einem
/// fremden Format kommt — aber die Laenge ist oben bereits geprueft.
fn be16(b: &[u8]) -> u32 {
    match (b.first(), b.get(1)) {
        (Some(&a), Some(&c)) => u16::from_be_bytes([a, c]) as u32,
        _ => 0,
    }
}

/// Skaliert einen Wert aus `0..=maxval` auf `0..=255`.
///
/// Fuer maxval 255 ist das der Identitaetsfall, fuer maxval 1 der
/// Extremfall — und genau die zwei, bei denen sich die meisten Leser
/// verrechnen.
fn skaliere(wert: u32, maxval: u32) -> u8 {
    if maxval == 255 {
        return wert.min(255) as u8;
    }
    ((wert.min(maxval) * 255 + maxval / 2) / maxval) as u8
}

/// Ein Token-Leser fuer die PPM-Formate.
///
/// Warum ein eigener Lexer: Kommentare duerfen UEBERALL zwischen zwei
/// Werten stehen, nicht nur am Zeilenanfang. Wer `split_whitespace` nimmt,
/// scheitert an `P3\n# ein Kommentar\n2 1`. Und nach dem letzten
/// Headerwert der binaeren Formen muss der Leser an einer exakten Byte-
/// Position weiterarbeiten koennen — da hilft nur ein Index.
struct Lexer<'a> {
    daten: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// Ueberspringt Leerzeichen und Kommentare.
    fn springe(&mut self) {
        loop {
            while self.pos < self.daten.len() && self.daten[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if self.pos < self.daten.len() && self.daten[self.pos] == b'#' {
                while self.pos < self.daten.len() && self.daten[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else {
                return;
            }
        }
    }

    fn zahl(&mut self) -> Result<u32, BildFehler> {
        self.springe();
        let start = self.pos;
        let mut wert: u64 = 0;
        let mut ziffern = 0usize;
        while self.pos < self.daten.len() && self.daten[self.pos].is_ascii_digit() {
            wert = wert
                .saturating_mul(10)
                .saturating_add((self.daten[self.pos] - b'0') as u64);
            // Ueber fuenf Stellen hinaus ist der Wert in jedem Fall kaputt
            // (65535 hat fuenf Ziffern). Wir melden es, statt weiterzurechnen.
            if ziffern >= 9 {
                return Err(BildFehler::PpmMaxvalFalsch {
                    maxval: wert.min(u32::MAX as u64) as u32,
                });
            }
            ziffern += 1;
            self.pos += 1;
        }
        if ziffern == 0 {
            return Err(BildFehler::PpmZahlFalsch { position: start });
        }
        if wert > u32::MAX as u64 {
            return Err(BildFehler::PpmMaxvalFalsch { maxval: u32::MAX });
        }
        Ok(wert as u32)
    }

    /// Springt ueber das EIN Leerzeichen, das nach dem letzten Headerwert
    /// folgt, und merkt sich, wo die Rohdaten beginnen.
    fn trenne_binardaten(&mut self) {
        if self.pos < self.daten.len() && self.daten[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn byte_slice(&mut self, n: usize) -> Result<&'a [u8], BildFehler> {
        if self.pos + n > self.daten.len() {
            return Err(BildFehler::Abgeschnitten {
                position: self.pos,
                gebraucht: n,
            });
        }
        let s = &self.daten[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}
