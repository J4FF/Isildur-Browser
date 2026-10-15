//! GIF LESEN: LZW, Interlacing, Transparenz, Animation.
//!
//! # Die LZW-Regel, die hier gilt (GEMESSEN 30.09.2026, 00:20)
//!
//! Der Enkoder haengt nach dem Clear an JEDEN gesendeten Code einen neuen
//! Eintrag an — auch an den ersten. Der Dekoder muss es genau so
//! machen: den Eintrag anlegen und den Zaehler erhoehen, und zwar in
//! **beiden** Faellen (normaler Code und „KwKwK"). Ein Dekoder, der den
//! Eintrag nur im Sonderfall anlegt, hat ab dem 258. Code ein kleineres
//! Woerterbuch als der Enkoder und liest ab da mit der falschen
//! Codebreite — bei einem 16x16-Bild also genau ab der Mitte Unsinn.
//!
//! Zwei Zaehlregeln sind denkbar und GEMESSEN an
//! `tests/daten/gif_einfach.gif` (256 Pixel, Pillow 11.1.0 als Orakel):
//!
//! | Regel                                  | Ergebnis                    |
//! |----------------------------------------|-----------------------------|
//! | Zaehler nur beim Anlegen des Eintrags   | alle 256 Indizes RICHTIG    |
//! | Zaehler bei jedem gelesenen Code        | Index 255 falsch (4 statt 0)|
//!
//! Es gilt also die erste Regel. Beide sind hier als Messung festgehalten,
//! damit sie niemand "symmetrischer" umbaut.
//!
//! # Grenzen (M3, M4)
//!
//! * [`MAX_FRAMES`] = 1.000 Bilder je Datei.
//! * [`MAX_PIXEL_ALLE_BILDER`] begrenzt die Summe aller Bildpunkte, weil
//!   ein Frame `breite * hoehe * 4` Byte ist: 1.000 Frames a 500x500 waeren
//!   ohne diese Grenze 1 GB.
//! * [`MAX_LZW_CODES`] = 4.096 (Spezifikation: 4.095 nutzbare Codes).
//! * [`MAX_LZW_DATEN`] = 16 MiB je Bild.
//! * [`MAX_PALETTE`] = 768 Byte (256 * 3).
//! * Jede Laenge aus der Datei wird VOR der Allokation geprueft.

use crate::{Bild, BildFehler, MAX_BILDPUNKTE, MAX_KANTE};

/// Hoechste Zahl Bilder (Frames) in einer GIF-Datei (M3).
pub const MAX_FRAMES: usize = 1_000;

/// Hoechste Summe Bildpunkte ueber ALLE Bilder einer Datei (M4).
///
/// 16.777.216 Punkte sind 64 MiB RGBA — das passt in
/// [`crate::MAX_SPEICHER`] und laesst genug Platz fuer mehrere Bilder.
pub const MAX_PIXEL_ALLE_BILDER: u64 = 16_777_216;

/// Hoechste Zahl LZW-Codes.
pub const MAX_LZW_CODES: usize = 4_096;

/// Sentinel in der Praefixliste: "dieser Code hat keinen Praefix".
const KEIN_PRAEFIX: u16 = 0xFFFF;

/// Hoechste Groesse der LZW-Daten je Bild in Byte.
pub const MAX_LZW_DATEN: usize = 16 * 1024 * 1024;

/// Hoechste Groesse einer Farbtabelle in Byte.
pub const MAX_PALETTE: usize = 768;

/// Was mit dem Bildschirm vor dem Zeichnen eines Bildes passiert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entsorgung {
    /// 0: nichts.
    Behalten,
    /// 1: vor dem Zeichnen auf die Hintergrundfarbe zuruecksetzen.
    Aufraeumen,
    /// 2: vor dem naechsten Bild wieder entfernen.
    Zuruecknehmen,
    /// 3: wie 1, aber mit der Frame-Ecke links oben als Hintergrund.
    Vormerken,
    /// 4..7 sind reserviert und werden wie 0 behandelt.
    Unbekannt,
}

/// Ein Bild einer Animation.
#[derive(Debug, Clone, PartialEq)]
pub struct GifFrame {
    /// Laufende Nummer dieses Bildes.
    pub nummer: usize,
    /// Das dekodierte Bild in RGBA8.
    pub bild: Bild,
    /// Verzoegerung in Millisekunden (GCE-Feld mal 10).
    pub verzoegerung_ms: u16,
    /// Disposal Method fuer dieses Bild.
    pub entsorgung: Entsorgung,
    /// Position links oben im Logical Screen.
    pub versatz_x: u16,
    pub versatz_y: u16,
}

/// Alles, was eine GIF-Datei enthaelt.
#[derive(Debug, Clone, PartialEq)]
pub struct Gif {
    /// Logical Screen Descriptor.
    pub breite: u16,
    pub hoehe: u16,
    /// Alle Bilder in Reihenfolge.
    pub frames: Vec<GifFrame>,
    /// 0 = endlos wiederholen.
    pub wiederholungen: u32,
    /// `true`, wenn ein Bild Transparenz hat.
    pub hat_transparenz: bool,
}

impl Gif {
    /// Das erste Bild.
    pub fn erstes_bild(&self) -> Option<&Bild> {
        self.frames.first().map(|f| &f.bild)
    }

    /// Die Verzoegerungen aller Bilder in Millisekunden.
    pub fn verzoegerungen(&self) -> Vec<u16> {
        self.frames.iter().map(|f| f.verzoegerung_ms).collect()
    }

    /// Dauer EINER Runde in Millisekunden.
    pub fn dauer_runde_ms(&self) -> u64 {
        self.frames.iter().map(|f| f.verzoegerung_ms as u64).sum()
    }
}

// ---------------------------------------------------------------------------
// LZW
// ---------------------------------------------------------------------------

/// Zerlegt den LZW-Strom in Symbole.
///
/// Siehe Modulkommentar: der Eintrag wird in BEIDEN Zweigen abgelegt.
struct Lzw {
    daten: Vec<u8>,
    byte: usize,
    puffer: u32,
    bits_im_puffer: u8,
    breite: u8,
    naechster: usize,
    praefix: Vec<u16>,
    zeichen: Vec<u8>,
}

impl Lzw {
    fn neu(min_code: u8) -> Lzw {
        let clear = 1usize << min_code;
        Lzw {
            daten: Vec::new(),
            byte: 0,
            puffer: 0,
            bits_im_puffer: 0,
            breite: min_code + 1,
            naechster: clear + 2,
            praefix: vec![KEIN_PRAEFIX; MAX_LZW_CODES],
            zeichen: vec![0; MAX_LZW_CODES],
        }
    }

    /// Setzt das Woerterbuch auf „nur die Literale" zurueck.
    fn zuruecksetzen(&mut self, min_code: u8) {
        let clear = 1usize << min_code;
        self.breite = min_code + 1;
        self.naechster = clear + 2;
        self.praefix.iter_mut().for_each(|p| *p = KEIN_PRAEFIX);
        self.zeichen.iter_mut().enumerate().for_each(|(i, z)| {
            if i < clear {
                *z = i as u8;
            }
        });
    }

    /// Liest `n` Bit (GIF ist LSB zuerst). `None` am Datenende.
    fn bits(&mut self, n: u8) -> Option<u32> {
        while self.bits_im_puffer < n {
            let b = *self.daten.get(self.byte)? as u32;
            self.byte += 1;
            self.puffer |= b << self.bits_im_puffer;
            self.bits_im_puffer += 8;
        }
        let wert = self.puffer & ((1u32 << n) - 1);
        self.puffer >>= n;
        self.bits_im_puffer -= n;
        Some(wert)
    }

    /// Die Kette von `code`, in Leserichtung.
    fn kette(&self, code: usize) -> Vec<u8> {
        let mut raus = Vec::new();
        let mut c = code;
        let mut n = 0usize;
        // `get` statt `[]`: ein Code, der nicht im Woerterbuch steht,
        // zeigt ins Nichts und beendet die Kette hier, statt den
        // Speicher zu lesen, der nicht dazugehoert.
        while let Some(&z) = self.zeichen.get(c) {
            raus.push(z);
            n += 1;
            if n > MAX_LZW_CODES {
                break; // zyklisch: abbrechen statt den Stapel zu sprengen (M5)
            }
            let p = self.praefix[c];
            if p == KEIN_PRAEFIX {
                break;
            }
            c = p as usize;
        }
        raus.reverse();
        raus
    }

    /// Dekodiert bis `ziel` Symbole.
    fn dekodiere(
        &mut self,
        ziel: usize,
        min_code: u8,
        aus: &mut Vec<u8>,
    ) -> Result<(), BildFehler> {
        let clear = 1usize << min_code;
        let eoi = clear + 1;
        self.zuruecksetzen(min_code);
        aus.clear();
        // M4: die Ausgabe ist durch `ziel` beschraenkt, das ist die
        // einzige Allokation und sie wird nicht ueberschritten.
        aus.reserve(ziel);

        let mut vorher: Option<usize> = None;
        let mut unterbrechung = false;

        while aus.len() < ziel {
            let code = match self.bits(self.breite) {
                Some(c) => c as usize,
                None => {
                    unterbrechung = true;
                    break;
                }
            };
            if code == eoi {
                unterbrechung = true;
                break;
            }
            if code == clear {
                self.zuruecksetzen(min_code);
                vorher = None;
                continue;
            }

            // „KwKwK": der Code ist genau der eben eingetragene. Dann ist
            // seine Kette die Kette des Vorcodes plus dessen erstes Zeichen.
            let kette = if code >= self.naechster {
                let basis = match vorher {
                    Some(b) => b,
                    None => {
                        unterbrechung = true;
                        break;
                    }
                };
                let mut k = self.kette(basis);
                if let Some(&erstes) = k.first() {
                    k.push(erstes);
                } else {
                    k.push(0);
                }
                k
            } else {
                self.kette(code)
            };

            for &b in &kette {
                if aus.len() >= ziel {
                    break;
                }
                aus.push(b);
            }

            // DER EINE WICHTIGE SCHRITT (GEMESSEN, siehe Modulkommentar):
            // der Eintrag entsteht in BEIDEN Zweigen.
            if let (Some(p), Some(&erstes)) = (vorher, kette.first()) {
                if self.naechster < MAX_LZW_CODES {
                    self.praefix[self.naechster] = p as u16;
                    self.zeichen[self.naechster] = erstes;
                    self.naechster += 1;
                    if self.naechster == (1usize << self.breite) && self.breite < 12 {
                        self.breite += 1;
                    }
                }
            }

            vorher = Some(code);
        }

        if aus.len() < ziel {
            return Err(BildFehler::ZuWenigDaten {
                erwartet: ziel,
                gefunden: aus.len(),
            });
        }

        // GEMESSEN 30.09.2026, 00:35: „Genau genug Symbole" ist NORMAL —
        // der End-of-Information-Code steht direkt hinter dem letzten
        // Pixel. Falsch war die erste Fassung, die das schon als
        // „zu viele" wertete und jede korrekte Datei ablehnte. Richtig ist
        // EINEN weiteren Code lesen: ist er EOI oder Clear, passt die
        // Datei genau.
        if !unterbrechung {
            if let Some(c) = self.bits(self.breite) {
                let c = c as usize;
                if c != eoi && c != clear {
                    return Err(BildFehler::EntpacktZuViel { erwartet: ziel });
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Lesen
// ---------------------------------------------------------------------------

/// Liest eine GIF-Datei mit ALLEN Bildern.
pub fn gif_lesen_alle(daten: &[u8]) -> Result<Gif, BildFehler> {
    if daten.len() < 13 {
        return Err(BildFehler::ZuKurz {
            vorhanden: daten.len(),
            gebraucht: 13,
        });
    }
    if &daten[..6] != b"GIF87a" && &daten[..6] != b"GIF89a" {
        return Err(BildFehler::GifDatenFalsch {
            grund: "kein GIF87a/GIF89a",
        });
    }

    let breite = u16::from_le_bytes([daten[6], daten[7]]);
    let hoehe = u16::from_le_bytes([daten[8], daten[9]]);
    let pack = daten[10];
    let gct_groesse = 1usize << ((pack & 0x07) + 1);

    let mut pos = 13usize;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    if pack & 0x80 != 0 {
        // M4: Hoechstens 768 Byte, aber VOR dem Lesen geprueft.
        if gct_groesse * 3 > MAX_PALETTE || pos + gct_groesse * 3 > daten.len() {
            return Err(BildFehler::GifDatenFalsch {
                grund: "globale Farbtabelle abgeschnitten",
            });
        }
        palette = daten[pos..pos + gct_groesse * 3]
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        pos += gct_groesse * 3;
    }

    let mut frames: Vec<GifFrame> = Vec::new();
    let mut wiederholungen = 0u32;
    let mut hat_transparenz = false;

    // GCE-Vorgaben gelten fuer das NAECHSTE Bild.
    let mut gce_transparenz: Option<u8> = None;
    let mut gce_verzoegerung = 0u16;
    let mut gce_entsorgung = Entsorgung::Behalten;

    let mut pixel_gezaehlt: u64 = 0;

    while pos < daten.len() {
        match daten[pos] {
            0x3B => break,    // Trailer
            0x00 => pos += 1, // Fuellbyte
            0x21 => {
                // Extension
                if pos + 2 > daten.len() {
                    return Err(BildFehler::Abgeschnitten {
                        position: pos,
                        gebraucht: 2,
                    });
                }
                let kennung = daten[pos + 1];
                let (start, ende) = lies_block_kette(daten, pos + 2)?;
                // `saturating_sub`: siehe `lies_block_kette`. Bei einer
                // leeren Kette (die mit einem Nullbyte beginnt) ist
                // `ende < start`; die alte Subtraktion unterlief und
                // panikte, `saturating_sub` liefert 0, und ein Block mit
                // 0 Nutzbytes wird uebersprungen.
                if kennung == 0xF9 && ende.saturating_sub(start) >= 4 {
                    let f = daten[start];
                    gce_entsorgung = entsorgung_aus((f >> 2) & 0x07);
                    // GEMESSEN 30.09.2026: Pillow schreibt `duration=10` als
                    // 1 in das GCE-Feld, weil es HUNDERSTELSEKUNDEN sind.
                    // Ohne den Faktor 10 kam 1 statt 10 heraus.
                    // GEMESSEN 02.10.2026 (Rolle BILD, B5) mit
                    // `examples/bomben_stress.rs`: hier stand `10 * ...`
                    // ohne Pruefung. Das GCE-Feld ist 16 Bit breit und
                    // traegt HUNDERSTELSEKUNDEN; ab 6554 laeuft die
                    // Zehnfachechnung ueber — der Fuzzer fand
                    // `attempt to multiply with overflow` (Zeile 380).
                    // `saturating_mul` haelt den Wert an der Obergrenze
                    // statt zu paniken; 655350 ms sind eine Minute, was
                    // in einem GIF zwar Unsinn ist, aber kein Absturz.
                    gce_verzoegerung = 10u16
                        .saturating_mul(u16::from_le_bytes([daten[start + 1], daten[start + 2]]));
                    if f & 0x01 != 0 {
                        gce_transparenz = Some(daten[start + 3]);
                        hat_transparenz = true;
                    }
                } else if kennung == 0xFF
                    && ende.saturating_sub(start) >= 14
                    && &daten[start..start + 11] == b"NETSCAPE2.0"
                    && daten[start + 11] == 1
                {
                    wiederholungen =
                        u16::from_le_bytes([daten[start + 12], daten[start + 13]]) as u32;
                }
                pos = ende + 1;
            }
            0x2C => {
                if frames.len() >= MAX_FRAMES {
                    return Err(BildFehler::GifDatenFalsch {
                        grund: "mehr Bilder als MAX_FRAMES",
                    });
                }
                if pos + 10 > daten.len() {
                    return Err(BildFehler::Abgeschnitten {
                        position: pos,
                        gebraucht: 10,
                    });
                }
                let ix = u16::from_le_bytes([daten[pos + 1], daten[pos + 2]]);
                let iy = u16::from_le_bytes([daten[pos + 3], daten[pos + 4]]);
                let iw = u16::from_le_bytes([daten[pos + 5], daten[pos + 6]]);
                let ih = u16::from_le_bytes([daten[pos + 7], daten[pos + 8]]);
                let ipack = daten[pos + 9];
                let interlaced = ipack & 0x40 != 0;
                let lct_groesse = 1usize << ((ipack & 0x07) + 1);

                let mut bild_palette = palette.clone();
                let mut p = pos + 10;
                if ipack & 0x80 != 0 {
                    if lct_groesse * 3 > MAX_PALETTE || p + lct_groesse * 3 > daten.len() {
                        return Err(BildFehler::GifDatenFalsch {
                            grund: "lokale Farbtabelle abgeschnitten",
                        });
                    }
                    bild_palette = daten[p..p + lct_groesse * 3]
                        .chunks_exact(3)
                        .map(|c| [c[0], c[1], c[2]])
                        .collect();
                    p += lct_groesse * 3;
                }

                // M3/M4: vor der Allokation.
                if iw == 0 || ih == 0 {
                    return Err(BildFehler::GroesseNull);
                }
                if iw as u32 > MAX_KANTE || ih as u32 > MAX_KANTE {
                    return Err(BildFehler::ZuGross {
                        breite: iw as u32,
                        hoehe: ih as u32,
                        kante: MAX_KANTE,
                    });
                }
                let ziel = iw as usize * ih as usize;
                if ziel as u64 > MAX_BILDPUNKTE {
                    return Err(BildFehler::ZuGross {
                        breite: iw as u32,
                        hoehe: ih as u32,
                        kante: MAX_BILDPUNKTE as u32,
                    });
                }
                pixel_gezaehlt += ziel as u64;
                if pixel_gezaehlt > MAX_PIXEL_ALLE_BILDER {
                    return Err(BildFehler::ZuGross {
                        breite: breite as u32,
                        hoehe: hoehe as u32,
                        kante: MAX_PIXEL_ALLE_BILDER as u32,
                    });
                }

                if p >= daten.len() {
                    return Err(BildFehler::Abgeschnitten {
                        position: p,
                        gebraucht: 1,
                    });
                }
                let min_code = daten[p];
                if !(2..=8).contains(&min_code) {
                    return Err(BildFehler::GifDatenFalsch {
                        grund: "LZW-Codegroesse ausserhalb 2..=8",
                    });
                }
                p += 1;
                // M4: die LZW-Nutzlast wird gesammelt, nicht geschnitten.
                let (nutzlast, lzw_ende) = lies_nutzlast(daten, p)?;
                if nutzlast.len() > MAX_LZW_DATEN {
                    return Err(BildFehler::ZuGross {
                        breite: nutzlast.len() as u32,
                        hoehe: 0,
                        kante: MAX_LZW_DATEN as u32,
                    });
                }

                let mut lzw = Lzw::neu(min_code);
                lzw.daten = nutzlast;
                let mut indizes = Vec::with_capacity(ziel);
                lzw.dekodiere(ziel, min_code, &mut indizes)?;

                frames.push(GifFrame {
                    nummer: frames.len(),
                    bild: baue_bild(iw, ih, &indizes, &bild_palette, gce_transparenz, interlaced),
                    verzoegerung_ms: gce_verzoegerung,
                    entsorgung: gce_entsorgung,
                    versatz_x: ix,
                    versatz_y: iy,
                });

                gce_transparenz = None;
                gce_verzoegerung = 0;
                gce_entsorgung = Entsorgung::Behalten;
                pos = lzw_ende + 1;
            }
            _ => {
                return Err(BildFehler::GifDatenFalsch {
                    grund: "unbekannter GIF-Block",
                });
            }
        }
    }

    if frames.is_empty() {
        return Err(BildFehler::GifDatenFalsch {
            grund: "GIF ohne Bild",
        });
    }
    Ok(Gif {
        breite,
        hoehe,
        frames,
        wiederholungen,
        hat_transparenz,
    })
}

/// Liest eine Kette `len byte daten`.
///
/// Liefert `(nutzlast_start, ende)`; `ende` zeigt auf das Null-Byte.
///
/// GEMESSEN 30.09.2026: `nutzlast_start` ist `pos + 1`. Mit `pos` las der
/// LZW-Dekoder die Laengenangabe als erstes Datenbyte und verpasste den
/// Clear-Code — das Bild wurde einfarbig.
fn lies_nutzlast(daten: &[u8], pos: usize) -> Result<(Vec<u8>, usize), BildFehler> {
    let mut raus: Vec<u8> = Vec::new();
    let mut p = pos;
    let mut bloecke = 0usize;
    // M3: 255 Bloecke a 255 Byte = 65.025 Byte je Kette.
    while p < daten.len() {
        let laenge = daten[p] as usize;
        p += 1;
        if laenge == 0 {
            return Ok((raus, p - 1));
        }
        if p + laenge > daten.len() {
            return Err(BildFehler::Abgeschnitten {
                position: p,
                gebraucht: laenge,
            });
        }
        // M4: erst pruefen, dann anhaengen.
        if raus.len() + laenge > MAX_LZW_DATEN {
            return Err(BildFehler::ZuGross {
                breite: (raus.len() + laenge) as u32,
                hoehe: 0,
                kante: MAX_LZW_DATEN as u32,
            });
        }
        raus.extend_from_slice(&daten[p..p + laenge]);
        p += laenge;
        bloecke += 1;
        if bloecke > 255 {
            return Err(BildFehler::GifDatenFalsch {
                grund: "zu viele Datenbloecke in einer Kette",
            });
        }
    }
    Err(BildFehler::Abgeschnitten {
        position: p,
        gebraucht: 1,
    })
}

/// Liest die Nutzlast EINER Kette und gibt sie als Schnitt zurueck.
///
/// Wird fuer die kurzen Erweiterungen (GCE, Application) benutzt, wo die
/// Nutzlast hoechstens 255 Byte hat und ein Schnitt genuegt.
/// Liest eine GIF-Datenblockkette.
///
/// Liefert `(start, ende)`: `start` ist die erste Nutzbyte-Stellung, und
/// `ende` ist die Stellung des **abschliessenden Nullbytes** — der
/// Aufrufer macht mit `pos = ende + 1` weiter, das ist die erste Stellung
/// hinter der Kette.
///
/// GEMESSEN 02.10.2026 (Rolle BILD, B5) mit `examples/bomben_stress.rs`:
/// bei einer Kette, die mit einem Nullbyte beginnt (also `laenge == 0`
/// im ersten Durchgang), ist `ende = pos` und `start = pos + 1` — dann
/// ist `ende KLEINER als start`, und die Aufrufer rechnen `ende - start`:
/// „attempt to subtract with overflow“ (Zeile 373). `ende` darf diese
/// Stellung nicht verruecken, weil `ende + 1` der Aufruf weiterspringt;
/// gesichert sind deshalb die Aufrufer mit `saturating_sub`.
fn lies_block_kette(daten: &[u8], pos: usize) -> Result<(usize, usize), BildFehler> {
    let mut p = pos;
    let mut bloecke = 0usize;
    // M3: hoechstens 255 Bloecke a 255 Byte = 65.025 Byte je Kette.
    while p < daten.len() {
        let laenge = daten[p] as usize;
        p += 1;
        if laenge == 0 {
            return Ok((pos + 1, p - 1));
        }
        if p + laenge > daten.len() {
            return Err(BildFehler::Abgeschnitten {
                position: p,
                gebraucht: laenge,
            });
        }
        p += laenge;
        bloecke += 1;
        if bloecke > 255 {
            return Err(BildFehler::GifDatenFalsch {
                grund: "zu viele Datenbloecke in einer Kette",
            });
        }
    }
    Err(BildFehler::Abgeschnitten {
        position: p,
        gebraucht: 1,
    })
}

/// Disposal Method aus der 3-Bit-Zahl.
fn entsorgung_aus(wert: u8) -> Entsorgung {
    match wert {
        0 => Entsorgung::Behalten,
        1 => Entsorgung::Aufraeumen,
        2 => Entsorgung::Zuruecknehmen,
        3 => Entsorgung::Vormerken,
        _ => Entsorgung::Unbekannt,
    }
}

/// Die Zeilenreihenfolge eines interlaced GIF (Spezifikation Abschnitt 20).
const INTERLACE_PASS: [(u32, u32); 4] = [(0, 8), (4, 8), (2, 4), (1, 2)];

/// Baut aus den Indizes ein RGBA-Bild.
fn baue_bild(
    iw: u16,
    ih: u16,
    indizes: &[u8],
    palette: &[[u8; 3]],
    transparenz: Option<u8>,
    interlaced: bool,
) -> Bild {
    let w = iw as u32;
    let h = ih as u32;
    let mut bild = Bild::new(w, h);
    if indizes.len() < (w as usize) * (h as usize) {
        return bild; // kann nicht vorkommen (dekodiere prueft), aber ohne Panic
    }

    let mut reihen = Vec::with_capacity(h as usize);
    if interlaced {
        for (start, schritt) in INTERLACE_PASS {
            let mut y = start;
            while y < h {
                reihen.push(y);
                y += schritt;
            }
        }
        if reihen.len() != h as usize {
            reihen.clear();
            for y in 0..h {
                reihen.push(y);
            }
        }
    } else {
        for y in 0..h {
            reihen.push(y);
        }
    }

    for zeile in 0..h as usize {
        let y = reihen[zeile];
        for x in 0..w as usize {
            let idx = indizes[zeile * w as usize + x];
            let p = match palette.get(idx as usize) {
                Some(p) => *p,
                None => continue, // Index ausserhalb: durchsichtig
            };
            let a = if transparenz == Some(idx) { 0 } else { 255 };
            bild.setze_pixel(x as u32, y, [p[0], p[1], p[2], a]);
        }
    }
    bild
}

/// Liest das ERSTE Bild einer GIF-Datei.
pub fn gif_lesen(daten: &[u8]) -> Result<Bild, BildFehler> {
    match gif_lesen_alle(daten)?.erstes_bild() {
        Some(b) => Ok(b.clone()),
        None => Err(BildFehler::GifDatenFalsch {
            grund: "GIF ohne Bild",
        }),
    }
}

/// Die Hintergrundfarbe aus der globalen Farbtabelle.
pub fn gif_hintergrund(daten: &[u8]) -> Option<[u8; 3]> {
    if daten.len() < 13 || (&daten[..6] != b"GIF87a" && &daten[..6] != b"GIF89a") {
        return None;
    }
    if daten[10] & 0x80 == 0 {
        return None;
    }
    let groesse = 1usize << ((daten[10] & 0x07) + 1);
    let idx = daten[11] as usize;
    if idx >= groesse {
        return None;
    }
    let p = 13 + idx * 3;
    if p + 3 > daten.len() {
        return None;
    }
    Some([daten[p], daten[p + 1], daten[p + 2]])
}

/// Die Dauer einer Animationsrunde in Millisekunden.
pub fn gif_gesamtdauer_ms(daten: &[u8]) -> Option<u64> {
    match gif_lesen_alle(daten) {
        Ok(g) if g.frames.len() > 1 => Some(g.dauer_runde_ms()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Interlace-Reihenfolge nach GIF89a Abschnitt 20.
    ///
    /// GEMESSEN 30.09.2026: an `gif_interlaced.gif` liefert der Decoder
    /// genau diese Permutation. Deshalb wird sie hier festgeschrieben
    /// statt aus einer Datei gelesen zu werden — sie ist die Definition,
    /// keine Eingabe.
    #[test]
    fn interlace_reihenfolge_ist_die_spezifikationsreihenfolge() {
        let mut reihen = Vec::new();
        for (start, schritt) in INTERLACE_PASS {
            let mut y = start;
            while y < 16 {
                reihen.push(y);
                y += schritt;
            }
        }
        // GEMESSEN 30.09.2026: die Schleife erzeugt genau die
        // Spezifikationsreihenfolge. Der Wert stand hier vorher falsch
        // (2,10,6,14 statt 2,6,10,14) — der Test hat seinen eigenen
        // Sollwert gerettet, nicht den Code.
        assert_eq!(
            reihen,
            vec![0, 8, 4, 12, 2, 6, 10, 14, 1, 3, 5, 7, 9, 11, 13, 15]
        );
    }

    /// GEMESSEN 30.09.2026: `baue_bild` legt Stromzeile `k` auf Bildzeile
    /// `reihen[k]`. Mit einem Muster, in dem jede Bildzeile ihre Nummer
    /// traegt, ist das ohne Bilddatei nachrechenbar.
    #[test]
    fn interlaced_baut_die_zeilen_um() {
        // GEMESSEN: nur wenn die Palette jeden Index mit einer eigenen
        // Zahl belegt, ist ueberhaupt sichtbar, welche Zeile gelandet ist.
        let mut palette = [[0u8, 0, 0]; 256];
        for (i, e) in palette.iter_mut().enumerate() {
            e[0] = i as u8;
        }
        let mut indizes = vec![0u8; 16 * 16];
        for y in 0..16u32 {
            for x in 0..16usize {
                // Index 0..15 traegt die Zeilennummer.
                indizes[y as usize * 16 + x] = y as u8;
            }
        }
        let bild = baue_bild(16, 16, &indizes, &palette, None, true);
        // GEMESSEN an `gif_interlaced.gif`: Bildzeile 1 bekommt die
        // Stromzeile 8. Hier ist der Rot-Kanal die Zeilennummer. Die
        // Zuordnung folgt `reihen[k]`: Bildzeile 3 ist Stromzeile 9
        // (GEMESSEN — die Zuordnung 3 -> 6 war mein Rechenfehler, nicht
        // der Code; der Test stand zuerst auf 6 und war rot).
        assert_eq!(bild.pixel(0, 1)[0], 8, "Bildzeile 1 <- Stromzeile 8");
        assert_eq!(bild.pixel(0, 3)[0], 9, "Bildzeile 3 <- Stromzeile 9");
        assert_eq!(bild.pixel(0, 8)[0], 1, "Bildzeile 8 <- Stromzeile 1");
        // Jede Zeilennummer kommt genau einmal vor.
        let mut gesehen = [false; 16];
        for y in 0..16u32 {
            gesehen[bild.pixel(0, y)[0] as usize] = true;
        }
        assert!(gesehen.iter().all(|&g| g), "Permutation ist vollstaendig");
    }
}
