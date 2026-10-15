//! JPEG LESEN: Baseline (ITU T.81) und progressiv.
//!
//! Diese Datei implementiert den JPEG-Dekoder. Reihenfolge der Arbeit:
//!
//!   1. Segmente lesen: DQT, DHT, SOF0/SOF1/SOF2, DRI, APPn, SOS.
//!   2. Die entropiekodierten Daten jedes Scans entstufen (0xFF00 wird zu
//!      0xFF) und an den Restart-Markern als POSITIONEN abgrenzen.
//!   3. Huffman dekodieren -> rohe Koeffizienten (Vorzeichen erweitert,
//!      in natuerlicher Reihenfolge ueber die Zigzag-Tabelle abgelegt).
//!   4. Dequantisierung + IDCT -> eine Musterebene je Komponente.
//!   5. YCbCr -> RGB (oder RGB direkt, wenn APP14/Adobe "transform 0"
//!      sagt), EXIF-Orientierung ist ueber [`jpeg_orientierung`] lesbar.
//!
//! Unterstuetzt: Baseline (SOF0/SOF1), Progressiv (SOF2) mit
//! Spektralauswahl und sukzessiver Approximation, Subsampling 4:4:4,
//! 4:2:2 und 4:2:0, Restart-Marker (DRI). 1 und 3 Komponenten;
//! CMYK/YCCK (4 Komponenten) wird mit einem benannten Fehler
//! abgelehnt, weil die Farbrechnung dort nicht definiert ist.
//!
//! Alle Grenzen werden VOR der Allokation geprueft: Groesse (MAX_KANTE,
//! MAX_BILDPUNKTE), Arbeitsspeicher (MAX_SPEICHER fuer Koeffizienten +
//! Musterebenen + Zielbild), Huffman-Kraft (kein ueberbesetztes
//! Codebaum), Scans und Segmente.

use crate::{Bild, BildFehler, MAX_BILDPUNKTE, MAX_KANTE, MAX_SPEICHER};

use super::{
    KOEFFIZIENTEN_PRO_BLOCK, MAX_BLOECKE_PRO_SPALTE, MAX_BLOECKE_PRO_ZEILE, MAX_HUFFMAN_LAENGE,
    MAX_HUFFMAN_TABELLEN, MAX_HUFFMAN_WERTE, MAX_KOMPONENTEN, MAX_QUANT_TABELLEN, MAX_QUANT_WERTE,
    MAX_SCANS, MAX_SEGMENTE,
};

// ---------------------------------------------------------------------------
// Marker
// ---------------------------------------------------------------------------

/// JPEG-Marker-Konstanten (das zweite Byte nach 0xFF).
mod marker {
    pub const SOI: u8 = 0xD8; // Start of Image
    pub const EOI: u8 = 0xD9; // End of Image
    pub const SOS: u8 = 0xDA; // Start of Scan
    pub const DQT: u8 = 0xDB; // Define Quantization Table
    pub const DHT: u8 = 0xC4; // Define Huffman Table
    pub const DRI: u8 = 0xDD; // Define Restart Interval
    pub const SOF0: u8 = 0xC0; // Start of Frame, Baseline
    pub const SOF1: u8 = 0xC1; // Start of Frame, erweitert sequentiell (8 Bit)
    pub const SOF2: u8 = 0xC2; // Start of Frame, Progressiv
    pub const APP0: u8 = 0xE0; // Application Segment 0 (JFIF)
    pub const APP1: u8 = 0xE1; // Application Segment 1 (EXIF)
    pub const APP14: u8 = 0xEE; // Application Segment 14 (Adobe)
    pub const COM: u8 = 0xFE; // Comment
}

/// Zigzag-Reihenfolge: ZIGZAG[z] ist die natuerliche Position des
/// Koeffizienten an Zigzag-Stelle z. Steht so auch im JPEG-Kopf (DQT,
/// SOS-Bandgrenzen), deshalb wird beim Ablegen umsortiert und beim
/// Dequantisieren nur noch z-weise multipliziert.
const ZIGZAG: [usize; KOEFFIZIENTEN_PRO_BLOCK] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

// ---------------------------------------------------------------------------
// Bit-Reader
// ---------------------------------------------------------------------------

/// Ein Bit-Reader ueber ENTSTUFPTE Scan-Daten.
///
/// Das Stuffing (0xFF00) und die Restart-Marker sind schon in
/// [`lese_scan_daten`] herausgearbeitet: hier steht eine reine
/// Bitfolge, Byte fuer Byte, ohne Markerbedeutung. Deshalb gibt es
/// hier auch keine Marker-Erkennung — ein 0xFF im Datenstrom ist ein
/// Datenbyte wie jedes andere.
struct BitReader<'a> {
    daten: &'a [u8],
    pos: usize,
    /// Welches Bit des aktuellen Bytes als naechstes kommt (0..8).
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    fn new(daten: &'a [u8]) -> Self {
        BitReader {
            daten,
            pos: 0,
            bit_pos: 0,
        }
    }

    /// Liest ein Bit. `None` heisst Datenende.
    fn bit(&mut self) -> Option<u8> {
        if self.pos >= self.daten.len() {
            return None;
        }
        let byte = self.daten[self.pos];
        let b = (byte >> (7 - self.bit_pos)) & 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.pos += 1;
        }
        Some(b)
    }

    /// Liest `n` Bits als Zahl (hoechstens 16, weil Kategorien 1..=15
    /// und Verfeinerungsbitfelder so breit sind). `None` = Datenende.
    fn bits(&mut self, n: u8) -> Option<u16> {
        let mut wert: u16 = 0;
        for _ in 0..n {
            let b = self.bit()?;
            wert = (wert << 1) | b as u16;
        }
        Some(wert)
    }

    /// Laesst Restbits der aktuellen Zeile liegen und rueckt auf die
    /// naechste Bytegrenze vor. Genau das passiert vor jedem
    /// Restart-Marker: der Enkoder fuellt auf Bytegrenze auf.
    fn aligiere(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.pos += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Vorzeichen erweitern (ITU T.81, "EXTEND")
// ---------------------------------------------------------------------------

/// Verwandelt ein `kategorie`-bit-Muster in einen signierten Wert.
///
/// Kategorie 1 heisst: 0 -> -1, 1 -> +1. Kategorie s heisst fuer
/// Werte unterhalb der Haelfte: `wert - 2^s + 1`. Gesamtdefiniert,
/// ohne Shift-Ueberlauf: Kategorie 0 liefert 0, alles ueber 15 wird
/// verworfen (Kategorien sind Nibbles, also hoechstens 15).
fn strecke(wert: u16, kategorie: u8) -> i32 {
    if kategorie == 0 || kategorie > 15 {
        return 0;
    }
    let w = wert as i32;
    let halb = 1i32 << (kategorie - 1);
    if w < halb {
        w - (1i32 << kategorie) + 1
    } else {
        w
    }
}

// ---------------------------------------------------------------------------
// Huffman-Tabelle
// ---------------------------------------------------------------------------

/// Eine Huffman-Tabelle mit abgeleiteten Suchfeldern.
///
/// Gespeichert sind die Anzahlen je Laenge und die Symbole in
/// kanonischer Reihenfolge (ITU T.81 Anhang F). Daraus leitet
/// [`HuffmanTabelle::ableiten`] die drei Felder ab, die das Dekodieren
/// braucht: `mincode`, `maxcode`, `valptr` je Laenge. Ein Symbol wird
/// dann in hoechstens 16 Schritten gefunden, ohne pro Schritt die
/// Tabelle neu zu durchlaufen.
#[derive(Debug, Clone)]
struct HuffmanTabelle {
    /// Anzahl der Codes je Laenge (Index 0 = Laenge 1).
    anzahlen: [u8; MAX_HUFFMAN_LAENGE],
    /// Die Symbole in kanonischer Reihenfolge.
    symbole: [u8; MAX_HUFFMAN_WERTE],
    anzahl_symbole: usize,
    /// Kleinstes Code-Muster je Laenge (Index 1..=16), signiert.
    mincode: [i32; MAX_HUFFMAN_LAENGE + 1],
    groesstes: [i32; MAX_HUFFMAN_LAENGE + 1],
    /// Index des ersten Symbols je Laenge (Index 1..=16).
    valptr: [usize; MAX_HUFFMAN_LAENGE + 1],
}

impl HuffmanTabelle {
    fn new() -> Self {
        HuffmanTabelle {
            anzahlen: [0; MAX_HUFFMAN_LAENGE],
            symbole: [0; MAX_HUFFMAN_WERTE],
            anzahl_symbole: 0,
            mincode: [0; MAX_HUFFMAN_LAENGE + 1],
            groesstes: [-1; MAX_HUFFMAN_LAENGE + 1],
            valptr: [0; MAX_HUFFMAN_LAENGE + 1],
        }
    }

    /// Liest eine Huffman-Tabelle aus den Daten.
    ///
    /// Format: 1 Byte Tc/Th (Tc = 0 DC, 1 AC; Th = Tabelle 0..3),
    /// 16 Byte Anzahl je Laenge 1..16, dann n Symbole mit
    /// n = Summe der Anzahlen. Liefert (Tc, Th, Tabelle, Position).
    fn lesen(daten: &[u8], pos: &mut usize) -> Result<(u8, u8, Self, usize), BildFehler> {
        if *pos >= daten.len() {
            return Err(BildFehler::Abgeschnitten {
                position: *pos,
                gebraucht: 1,
            });
        }
        let tc_th = daten[*pos];
        *pos += 1;
        let tc = (tc_th >> 4) & 0x0F;
        let th = tc_th & 0x0F;
        if tc > 1 || th as usize >= MAX_HUFFMAN_TABELLEN {
            return Err(BildFehler::JpegHuffmanFalsch {
                grund: "Tc/Th ungueltig",
            });
        }

        if *pos + MAX_HUFFMAN_LAENGE > daten.len() {
            return Err(BildFehler::Abgeschnitten {
                position: *pos,
                gebraucht: MAX_HUFFMAN_LAENGE,
            });
        }
        let mut tabelle = HuffmanTabelle::new();
        let mut gesamt = 0usize;
        for i in 0..MAX_HUFFMAN_LAENGE {
            tabelle.anzahlen[i] = daten[*pos + i];
            gesamt += tabelle.anzahlen[i] as usize;
        }
        *pos += MAX_HUFFMAN_LAENGE;

        if gesamt == 0 {
            return Err(BildFehler::JpegHuffmanFalsch {
                grund: "leere Huffman-Tabelle",
            });
        }
        if gesamt > MAX_HUFFMAN_WERTE {
            return Err(BildFehler::JpegHuffmanFalsch {
                grund: "zu viele Symbole",
            });
        }

        // Kraft-Gleichung: der Codebaum darf nicht ueberbesetzt sein,
        // sonst koennte man mit 16 Bits nie alle Symbole erreichen.
        // Ueberzaehlig waere das Gegenstueck (unvollstaendig ist erlaubt).
        let mut summe = 0u32;
        for laenge in 0..MAX_HUFFMAN_LAENGE {
            summe += tabelle.anzahlen[laenge] as u32;
            if summe > (1u32 << (laenge + 1)) {
                return Err(BildFehler::JpegHuffmanFalsch {
                    grund: "Codebaum ueberbesetzt",
                });
            }
            summe <<= 1;
        }

        if *pos + gesamt > daten.len() {
            return Err(BildFehler::Abgeschnitten {
                position: *pos,
                gebraucht: gesamt,
            });
        }
        for i in 0..gesamt {
            tabelle.symbole[i] = daten[*pos + i];
        }
        let neue_pos = *pos + gesamt;
        tabelle.anzahl_symbole = gesamt;
        tabelle.ableiten();
        Ok((tc, th, tabelle, neue_pos))
    }

    /// Leitet die Suchfelder aus den Anzahlen ab (kanonische Codes).
    fn ableiten(&mut self) {
        let mut code: i32 = 0;
        let mut symbol = 0usize;
        for laenge in 1..=MAX_HUFFMAN_LAENGE {
            self.mincode[laenge] = code;
            self.valptr[laenge] = symbol;
            let n = self.anzahlen[laenge - 1] as i32;
            if n > 0 {
                self.groesstes[laenge] = code + n - 1;
                code += n;
                symbol += n as usize;
            } else {
                self.groesstes[laenge] = -1;
            }
            code <<= 1;
        }
    }

    /// Dekodiert ein Symbol aus dem Bit-Stream.
    ///
    /// `None` heisst „Datenende vor dem naechsten Bit" — der Aufrufer
    /// meldet das als abgeschnittene Scan-Daten. Ein Fehler heisst: die
    /// 16 Bits passen in keinen Code der Tabelle.
    fn dekodiere(&self, reader: &mut BitReader) -> Result<Option<u8>, BildFehler> {
        let mut code: i32 = 0;
        for laenge in 1..=MAX_HUFFMAN_LAENGE {
            let b = match reader.bit() {
                Some(b) => b,
                None => return Ok(None),
            };
            code = (code << 1) | b as i32;
            if code <= self.groesstes[laenge] && code >= self.mincode[laenge] {
                let idx = self.valptr[laenge] + (code - self.mincode[laenge]) as usize;
                if idx >= self.anzahl_symbole {
                    return Err(BildFehler::JpegHuffmanFalsch {
                        grund: "Symbolindex ausserhalb der Tabelle",
                    });
                }
                return Ok(Some(self.symbole[idx]));
            }
        }
        Err(BildFehler::JpegHuffmanFalsch {
            grund: "kein Code der Tabelle passt zu 16 Bits",
        })
    }
}

// ---------------------------------------------------------------------------
// Quantisierungstabelle
// ---------------------------------------------------------------------------

/// Eine Quantisierungstabelle (64 Werte, in Zigzag-Reihenfolge wie im
/// DQT-Segment gespeichert).
struct QuantTabelle {
    werte: [u16; MAX_QUANT_WERTE],
}

impl QuantTabelle {
    fn new() -> Self {
        QuantTabelle {
            werte: [1; MAX_QUANT_WERTE],
        }
    }

    /// Liest eine Quantisierungstabelle. Liefert (Tq, Tabelle, Pos).
    fn lesen(daten: &[u8], pos: &mut usize) -> Result<(u8, Self, usize), BildFehler> {
        if *pos >= daten.len() {
            return Err(BildFehler::Abgeschnitten {
                position: *pos,
                gebraucht: 1,
            });
        }
        let pq_tq = daten[*pos];
        *pos += 1;
        let pq = (pq_tq >> 4) & 0x0F;
        let tq = pq_tq & 0x0F;
        if pq > 1 || tq as usize >= MAX_QUANT_TABELLEN {
            return Err(BildFehler::JpegQuantFalsch {
                grund: "Pq/Tq ungueltig",
            });
        }

        let anzahl = if pq == 0 { 64 } else { 128 };
        if *pos + anzahl > daten.len() {
            return Err(BildFehler::Abgeschnitten {
                position: *pos,
                gebraucht: anzahl,
            });
        }
        let mut tabelle = QuantTabelle::new();
        if pq == 0 {
            for i in 0..64 {
                tabelle.werte[i] = daten[*pos + i] as u16;
            }
        } else {
            for i in 0..64 {
                tabelle.werte[i] =
                    u16::from_be_bytes([daten[*pos + i * 2], daten[*pos + i * 2 + 1]]);
            }
        }
        let neue_pos = *pos + anzahl;
        Ok((tq, tabelle, neue_pos))
    }
}

// ---------------------------------------------------------------------------
// Komponente und Frame
// ---------------------------------------------------------------------------

/// Eine Komponente (Kanal) in einem Frame.
struct Komponente {
    id: u8,
    /// Horizontaler Abtastfaktor (1..4).
    h: u8,
    /// Vertikaler Abtastfaktor (1..4).
    v: u8,
    /// Quantisierungstabelle (0..3).
    tq: u8,
    /// Groesster Abtastfaktor im Frame ( fuer die Musterebene ).
    hmax: u8,
    vmax: u8,
    /// DC-Differenz-Akkumulator (Prediktor) des aktuellen Scans.
    dc: i32,
    /// Anzahl Bloecke je Richtung (auf das MCU-Raster aufgezoegen).
    ///
    /// Das ist das Raster des SPEICHERS: ein interlizierter Scan
    /// fuellt alle Spalten, auch die Padding-Spalte am rechten Rand.
    bloecke_x: u32,
    bloecke_y: u32,
    /// Bloecke, die das Bild wirklich braucht (ohne Padding).
    ///
    /// Ein Scan mit EINER Komponente zaehlt nur diese Bloecke: der
    /// Encoder schreibt die Randbloecke des MCU-Rasters dann ueber-
    /// haupt nicht (ITU T.81 A.2.1.2, `width_in_blocks`).
    nutz_x: u32,
    nutz_y: u32,
    /// Rohe Koeffizienten aller Bloecke, in Reihenfolge des Scans
    /// abgelegt (Zigzag -> natuerlich beim Ablegen).
    koeffizienten: Vec<[i16; KOEFFIZIENTEN_PRO_BLOCK]>,
    /// B9: nur die Gleichstromkoeffizienten, ein Wert je Block.
    ///
    /// GEMESSENE Groessenordnung an einem 2400x1800-Bild mit 3 Komponenten
    /// (4:4:0, also 1h x 2v):
    /// `koeffizienten` braucht 8 mal 64 Byte je Block, das sind **2 Byte
    /// je Bildpunkt je Komponente** — bei drei Komponenten rund 13 MB,
    /// ohne dass ein einziger Wechselstromkoeffizient gebraucht wird.
    /// `dc_werte` braucht 2 Byte je Block, also ein Vierundsechzigstel
    /// davon, rund 0,2 MB.
    ///
    /// Das Feld wird nur benutzt, wenn `Frame::nur_dc` gesetzt ist; im
    /// normalen Weg bleibt es leer, damit sich der Speicherbedarf des
    /// Volldekoders nicht aendert.
    dc_werte: Vec<i16>,
    /// Fertige Musterebene: `bloecke_x*8` mal `bloecke_y*8` Byte.
    muster: Vec<u8>,
    muster_breite: u32,
}

impl Komponente {
    fn new(id: u8, h: u8, v: u8, tq: u8) -> Self {
        Komponente {
            id,
            h,
            v,
            tq,
            hmax: 1,
            vmax: 1,
            dc: 0,
            bloecke_x: 0,
            bloecke_y: 0,
            nutz_x: 0,
            nutz_y: 0,
            koeffizienten: Vec::new(),
            dc_werte: Vec::new(),
            muster: Vec::new(),
            muster_breite: 0,
        }
    }

    /// Ein Sample der Musterebene fuer die Bildposition (x, y).
    ///
    /// Die Musterebene ist bereits auf Bildgroesse hochgerechnet (siehe
    /// `hochskaliere`), deshalb ist der Index hier direkt `x + y*Breite`.
    fn musterpixel(&self, x: u32, y: u32) -> u8 {
        if self.muster_breite == 0 {
            return 128;
        }
        let i = y as usize * self.muster_breite as usize + x as usize;
        match self.muster.get(i) {
            Some(&w) => w,
            None => 128,
        }
    }

    /// Hochrechnet die Chromaebenen auf die Bildgroesse.
    ///
    /// GEMESSEN 01.10.2026, 03:50 und 04:10: mit blossem „naechstes
    /// Sample nehmen" (naemlich `x * h / hmax`) war die Abweichung zum
    /// Original **7 bis 9** bei 4:2:0, aber nur **3** bei 4:4:4 und **1**
    /// bei Graustufen. Die Abweichung waechst also mit dem
    /// Chroma-Subsampling, und das ist der Beweis, dass nicht der
    /// progressive Weg, sondern das Hochrechnen schuld war.
    ///
    /// libjpeg rechnet standardmaessig „fancy" hoch (`jdmaster.c`,
    /// `do_fancy_upsampling = TRUE`): ein Dreiecksfilter, 3/4 des
    /// naeheren und 1/4 des weiteren Samples. Die drei Faelle stehen hier
    /// einzeln, mit den **exakt verschiedenen Rundungskonstanten** aus
    /// `jdsample.c` — die sind nicht symmetrisch, und eine
    /// symmetrische Form kostete GEMESSEN genau einen Punkt Abweichung
    /// (4:2:2 von 5 auf 6):
    ///
    /// | Fall         | libjpeg-Funktion        | Konstanten           |
    /// |--------------|-------------------------|----------------------|
    /// | 2x1, 1x2     | `h2v1_fancy_upsample`   | `+1` ungerade, `+2` gerade |
    /// | 2x2          | `h2v2_fancy_upsample`   | `+8` gerade, `+7` ungerade |
    /// | sonst        | separierbar 9/16/3/16/1/16| `+8`                 |
    ///
    /// Der Rand wird geklemmt, nicht abgeschnitten: libjpeg laesst die
    /// Randzeile/-spalte doppelt zaehlen (3x naeheres Sample + 1x
    /// weiteres Sample ist dort dasselbe Sample). Genau das macht dieses
    /// `clamp` nach, und es ist der Grund, warum die Randpunkte stimmen.
    /// Hochrechnen mit auswaehlbarem Filter.
    ///
    /// B9, GEMESSEN: im **Achtelweg** ist `fancy` falsch, und der
    /// Unterschied ist gross. Bei 3 Chromawerten und 5 Ausgabespalten
    /// liefert libjpeg (also PIL nach `draft`) die Folge
    /// `[c0, c0, c1, c1, c2]` — jedes Sample wird `sx` mal
    /// **vervielfaeltigt**. Nachgerechnet an
    /// `tests/daten/farbverlauf_422.jpg`, Zeile 0, Kanal Rot:
    ///
    /// ```text
    /// x        0     1     2     3     4
    /// Y       25    42    59    76    94      (unsere Gleichstromebene)
    /// Cr     136   136   180   180   218      (vervielfaeltigt)
    /// R = Y + 1.402*(Cr-128)
    ///        36.2  53.2 131.9 148.9 220.2  -> gerundet 36 53 132 149 220
    /// PIL      36    53   132   149   220     <- alle fuenf gleich
    /// ```
    ///
    /// Mit dem fancy-Filter (3/4 naeheres + 1/4 weiteres Sample) kamen
    /// statt dessen 36, 69, 116, 163, 206 heraus — **maximale Abweichung
    /// 16** an der Zeile. Die Begründung des Herstellers ist dieselbe,
    /// die auch im Kommentar oben steht: der fancy-Filter ist ein
    /// **Hoch**rechnungsfilter. Beim Achtelweg ist jedes Chromasample
    /// schon genau so breit wie der Bildpunkt, den es versorgt, es gibt
    /// also nichts zu interpolieren — nur zu vervielfaeltigen.
    ///
    /// Im Volldekoder bleibt `fancy` bei `true`, dort ist das Hochrechnen
    /// ein echtes Hochrechnen (2x, 4x) und der Filter gehoert dorthin
    /// (`jdsample.c`).
    fn hochskaliere_mit(&mut self, breite: u32, hoehe: u32, fancy: bool) {
        let sx = (self.hmax / self.h.max(1)) as i64;
        let sy = (self.vmax / self.v.max(1)) as i64;
        if sx <= 1 && sy <= 1 {
            return; // 4:4:4 oder 4:4:0: nichts zu tun
        }
        let qb = self.muster_breite as usize;
        if qb == 0 || self.muster.is_empty() {
            return;
        }
        let qh = (self.muster.len() / qb) as i64;
        let ziel_breite = ((self.muster_breite as i64) * sx).min(breite.max(1) as i64) as u32;
        let ziel_hoehe = (qh * sy).min(hoehe.max(1) as i64) as u32;
        let mut raus = vec![128u8; (ziel_breite as usize) * (ziel_hoehe as usize)];

        let hol = |b: &[u8], x: i64, y: i64| -> i32 {
            let xx = x.clamp(0, qb as i64 - 1) as usize;
            let yy = y.clamp(0, qh - 1) as usize;
            b.get(yy * qb + xx).copied().unwrap_or(128) as i32
        };

        // GEMESSEN 01.10.2026: die Chromaebene ist auf ganze Bloecke
        // aufgerundet, ein 40 Pixel breites 4:2:0-Bild hat also 24 Chroma-
        // Spalten und nach dem Hochrechnen 48 — mehr als das Bild breit
        // ist. Ohne diese Pruefung schreibt der Filter ueber den Puffer
        // hinaus (panic bei `index out of bounds: len 1200, index 1200`).
        let zb = ziel_breite as usize;
        let zh = ziel_hoehe as usize;
        let set = |r: &mut Vec<u8>, x: i64, y: i64, wert: i32| {
            if x < 0 || y < 0 || x >= zb as i64 || y >= zh as i64 {
                return; // Padding-Spalte/-zeile: nicht Teil des Bildes
            }
            r[y as usize * zb + x as usize] = wert as u8;
        };

        // B9, Achtelweg: vervielfaeltigen statt interpolieren.
        if !fancy {
            let sx_u = sx.max(1) as u32;
            let sy_u = sy.max(1) as u32;
            for y in 0..zh as u32 {
                // M3/M4: `y / sy_u` und `min` halten den Index in der
                // Ebene, auch wenn das Zielgitter groesser ist als das
                // Quellgitter (Randbloecke).
                let qy = (y / sy_u).min(qh.saturating_sub(1) as u32) as usize;
                for x in 0..zb as u32 {
                    let qx = (x / sx_u).min(qb.saturating_sub(1) as u32) as usize;
                    raus[y as usize * zb + x as usize] = self.muster[qy * qb + qx];
                }
            }
            self.muster = raus;
            self.muster_breite = ziel_breite;
            return;
        }

        match (sx, sy) {
            // Nur horizontal: `h2v1_fancy_upsample`, Zeile fuer Zeile.
            (2, 1) => {
                for y in 0..ziel_hoehe as i64 {
                    for i in 0..qb as i64 {
                        let a = hol(&self.muster, i, y);
                        let b = hol(&self.muster, i + 1, y);
                        let links = hol(&self.muster, i - 1, y);
                        // (3*naeher + weiter + 1) >> 2 fuer die gerade
                        // Ausgabe, (3*naeher + weiter + 2) >> 2 fuer die
                        // ungerade. Genau die beiden Konstanten aus
                        // `jdsample.c`.
                        set(&mut raus, 2 * i, y, (3 * a + links + 1) >> 2);
                        set(&mut raus, 2 * i + 1, y, (3 * a + b + 2) >> 2);
                    }
                }
            }
            // Nur vertikal: dasselbe untereinander.
            (1, 2) => {
                for y in 0..ziel_hoehe as i64 {
                    let ny = y / 2;
                    let wy = if y % 2 == 0 { ny - 1 } else { ny + 1 };
                    for x in 0..ziel_breite as i64 {
                        let a = hol(&self.muster, x, ny);
                        let b = hol(&self.muster, x, wy);
                        let oben = hol(&self.muster, x, ny - 1);
                        // gerade Zeile: (3*naeher + weiter + 1) >> 2
                        set(&mut raus, x, y, (3 * a + oben + 1) >> 2);
                        // ungerade Zeile: (naeher + 3*weiter + 2) >> 2
                        set(&mut raus, x, y + 1, (a + 3 * b + 2) >> 2);
                    }
                }
            }
            // Beide Richtungen: `h2v2_fancy_upsample`.
            (2, 2) => {
                for y in 0..ziel_hoehe as i64 {
                    let ny = y / 2;
                    let wy = if y % 2 == 0 { ny - 1 } else { ny + 1 };
                    // thiscolsum[i] = 3*naeher + weiter, je Spalte.
                    let cs: Vec<i32> = (0..qb as i64)
                        .map(|i| 3 * hol(&self.muster, i, ny) + hol(&self.muster, i, wy))
                        .collect();
                    for i in 0..qb as i64 {
                        let c = cs[i as usize];
                        let vorher = if i == 0 { cs[0] } else { cs[(i - 1) as usize] };
                        let nachher = if i + 1 >= qb as i64 {
                            cs[qb - 1]
                        } else {
                            cs[(i + 1) as usize]
                        };
                        set(&mut raus, 2 * i, y, (c * 3 + vorher + 8) >> 4);
                        set(&mut raus, 2 * i + 1, y, (c * 3 + nachher + 7) >> 4);
                    }
                }
            }
            // Alles andere (z. B. 4:1:1): separierbar 9/16, 3/16, 1/16.
            _ => {
                for y in 0..ziel_hoehe as i64 {
                    let ny = y / sy;
                    let wy = if y % sy == 0 { ny - 1 } else { ny + 1 };
                    for x in 0..ziel_breite as i64 {
                        let nx = x / sx;
                        let wx = if x % sx == 0 { nx - 1 } else { nx + 1 };
                        let sum = 9 * hol(&self.muster, nx, ny)
                            + 3 * hol(&self.muster, wx, ny)
                            + 3 * hol(&self.muster, nx, wy)
                            + hol(&self.muster, wx, wy);
                        set(&mut raus, x, y, (sum + 8) >> 4);
                    }
                }
            }
        }
        self.muster = raus;
        self.muster_breite = ziel_breite;
    }
}

/// Ein Frame (SOF-Segment) mit seinem MCU-Raster.
struct Frame {
    breite: u32,
    hoehe: u32,
    komponenten: Vec<Komponente>,
    progressiv: bool,
    hmax: u8,
    vmax: u8,
    /// Anzahl der Makrobausteine je Richtung.
    mcus_x: u32,
    mcus_y: u32,
    /// B9: nur die Gleichstromkoeffizienten lesen und daraus ein Bild in
    /// Achtelgroesse bauen. Das ist die DCT-Skalierung, die libjpeg unter
    /// dem Namen `scale N/8` anbietet und die PIL ueber `draft()` benutzt —
    /// dieselbe Rechnung, damit ist PIL das Orakel fuer diesen Weg.
    ///
    /// GEMESSEN: bei `true` sind `ziel_breite`/`ziel_hoehe` eine Achtel der
    /// Bildgroesse (aufgerundet), im normalen Weg sind beide Zahlen genau
    /// `breite`/`hoehe`.
    nur_dc: bool,
    /// Release 08.10.2026: der Teiler der Kante im Weg MIT Wechselstrom
    /// (1 = voll, 2 = halb, 4 = viertel); der Achtelweg ist `nur_dc`.
    teiler: u32,
    /// Breite des Bildes, das am Ende herauskommt.
    ziel_breite: u32,
    /// Hoehe des Bildes, das am Ende herauskommt.
    ziel_hoehe: u32,
}

impl Frame {
    fn new(breite: u32, hoehe: u32, progressiv: bool) -> Self {
        Frame {
            breite,
            hoehe,
            komponenten: Vec::new(),
            progressiv,
            hmax: 1,
            vmax: 1,
            mcus_x: 0,
            mcus_y: 0,
            nur_dc: false,
            teiler: 1,
            ziel_breite: breite,
            ziel_hoehe: hoehe,
        }
    }
}

// ---------------------------------------------------------------------------
// Scans
// ---------------------------------------------------------------------------

/// Was ein Scan zu liefern hat (ITU T.81 Tabelle B.3: Ss, Se, Ah, Al).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanArt {
    /// Baseline: DC und AC in einem Durchgang, volles Band 0..63.
    Basis,
    /// Progressiv: nur DC (Ss = Se = 0).
    Dc,
    /// Progressiv: nur AC (Ss >= 1).
    Ac,
}

/// Ein gelesener SOS-Kopf, bevor die IDs in Frame-Indizes aufgeloest
/// sind. `komponenten` ist (Komponenten-ID, Td, Ta).
struct SosRoh {
    komponenten: Vec<(u8, u8, u8)>,
    ss: u8,
    se: u8,
    ah: u8,
    al: u8,
}

/// Ein aufgeloester Scan: Komponenten-Index + Tabellen, Bandgrenzen.
struct ScanKopf {
    komponenten: Vec<(usize, u8, u8)>,
    ss: u8,
    se: u8,
    ah: u8,
    al: u8,
    art: ScanArt,
}

/// Die entropiekodierten Daten eines Scans.
///
/// `restarts` enthaelt die Positionen, an denen im ENSTUFPSTEN Strom
/// ein Restart-Marker sass. Die Marker selbst sind raus, weil ihre
/// Bytes die Bitfolge verschieben wuerden; an diesen Positionen muss
/// der Dekoder nach dem Auffuellen auf Bytegrenze genau sein.
struct ScanDaten {
    bytes: Vec<u8>,
    restarts: Vec<usize>,
}

/// Der Huffman-Tabellenstand ZUM ZEITPUNKT, an dem der Scan gelesen
/// wurde: (DC, AC).
///
/// Optimierte progressive Dateien definieren die AC-Tabelle zwischen
/// den Scans neu — in `fortschrittlich.jpg` acht Mal, jedes Mal unter
/// derselben Tabellennummer. Wer alle Scans erst am Ende der Datei
/// dekodiert, benutzt dann die LETZTE Tabelle fuer alle vorherigen
/// Scans, und der erste AC-Scan laeuft ueber das Band hinaus. Jeder
/// Scan bekommt deshalb seinen eigenen, eingefrorenen Stand.
type Tabellenstand = (Vec<Option<HuffmanTabelle>>, Vec<Option<HuffmanTabelle>>);

// ---------------------------------------------------------------------------
// IDCT
// ---------------------------------------------------------------------------

/// Inverse Diskrete Cosinus-Transformation mit Dequantisierung.
///
/// `block` kommt mit rohen Koeffizienten in natuerlicher Reihenfolge
/// und geht als 0..255-Muster zurueck. Zwei Schritte (Zeilen dann
/// Spalten) mit der direkten Formel, Faktor 0.5 je Schritt, am Ende
/// +128 fuer die Negative-Offset-Konvention von JPEG.
fn idct(block: &mut [i16; KOEFFIZIENTEN_PRO_BLOCK], quant: &QuantTabelle) {
    // Dequantisierung: der Wert im DQT-Segment steht in Zigzag-
    // Reihenfolge, der Koeffizient liegt natuerlich vor.
    let mut f = [0f32; KOEFFIZIENTEN_PRO_BLOCK];
    for z in 0..KOEFFIZIENTEN_PRO_BLOCK {
        f[ZIGZAG[z]] = block[ZIGZAG[z]] as f32 * quant.werte[z] as f32;
    }

    // Zeilentransformation
    let mut tmp = [0f32; KOEFFIZIENTEN_PRO_BLOCK];
    for x in 0..8 {
        for y in 0..8 {
            let mut summe = 0f32;
            for v in 0..8 {
                let cv = if v == 0 {
                    std::f32::consts::FRAC_1_SQRT_2
                } else {
                    1f32
                };
                summe += cv * f[x * 8 + v] * COS_TABLE[y][v];
            }
            tmp[x * 8 + y] = summe * 0.5f32;
        }
    }

    // Spaltentransformation
    for x in 0..8 {
        for y in 0..8 {
            let mut summe = 0f32;
            for u in 0..8 {
                let cu = if u == 0 {
                    std::f32::consts::FRAC_1_SQRT_2
                } else {
                    1f32
                };
                summe += cu * tmp[u * 8 + y] * COS_TABLE[x][u];
            }
            let wert = (summe * 0.5f32 + 128f32).round() as i32;
            block[x * 8 + y] = wert.clamp(0, 255) as i16;
        }
    }
}

/// Cosinus-Tabelle: COS_TABLE[x][u] = cos((2x+1)*u*pi/16).
const COS_TABLE: [[f32; 8]; 8] = [
    [
        1.0,
        0.98078528,
        0.9238795,
        0.8314696,
        std::f32::consts::FRAC_1_SQRT_2,
        0.55557023,
        0.38268343,
        0.19509032,
    ],
    [
        1.0,
        0.8314696,
        0.38268343,
        -0.19509032,
        -std::f32::consts::FRAC_1_SQRT_2,
        -0.98078528,
        -0.9238795,
        -0.55557023,
    ],
    [
        1.0,
        0.55557023,
        -0.38268343,
        -0.98078528,
        -std::f32::consts::FRAC_1_SQRT_2,
        0.19509032,
        0.9238795,
        0.8314696,
    ],
    [
        1.0,
        0.19509032,
        -0.9238795,
        -0.55557023,
        std::f32::consts::FRAC_1_SQRT_2,
        0.8314696,
        -0.38268343,
        -0.98078528,
    ],
    [
        1.0,
        -0.19509032,
        -0.9238795,
        0.55557023,
        std::f32::consts::FRAC_1_SQRT_2,
        -0.8314696,
        -0.38268343,
        0.98078528,
    ],
    [
        1.0,
        -0.55557023,
        -0.38268343,
        0.98078528,
        -std::f32::consts::FRAC_1_SQRT_2,
        -0.19509032,
        0.9238795,
        -0.8314696,
    ],
    [
        1.0,
        -0.8314696,
        0.38268343,
        0.19509032,
        -std::f32::consts::FRAC_1_SQRT_2,
        0.98078528,
        -0.9238795,
        0.55557023,
    ],
    [
        1.0,
        -0.98078528,
        0.9238795,
        -0.8314696,
        std::f32::consts::FRAC_1_SQRT_2,
        -0.55557023,
        0.38268343,
        -0.19509032,
    ],
];

// ---------------------------------------------------------------------------
// YCbCr -> RGB
// ---------------------------------------------------------------------------

/// Wandelt YCbCr in RGB um (ITU T.81 Anhang K, auf 0..255 geklemmt).
fn ycbcr_zu_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let y = y as f32;
    let cb = cb as f32 - 128f32;
    let cr = cr as f32 - 128f32;
    let r = (y + 1.402f32 * cr).round().clamp(0f32, 255f32) as u8;
    let g = (y - 0.344136f32 * cb - 0.714136f32 * cr)
        .round()
        .clamp(0f32, 255f32) as u8;
    let b = (y + 1.772f32 * cb).round().clamp(0f32, 255f32) as u8;
    [r, g, b]
}

// ---------------------------------------------------------------------------
// Hauptfunktion
// ---------------------------------------------------------------------------

/// Liest eine JPEG-Datei (Baseline oder progressiv) als RGBA-Bild.
/// Wie stark verkleinert gelesen wird (B9).
///
/// `Achtel` ist die DCT-Skalierung 1/8: je 8x8-Block entsteht genau ein
/// Bildpunkt aus dem Gleichstromkoeffizienten. Das ist keine Naeherung von
/// aussen, sondern das, was libjpeg bei `scale 1/8` rechnet
/// (`jidctred.c`, `jpeg_idct_1x1`: `DESCALE(dc, 3)` auf den
/// dequantisierten Gleichstrom) und was PIL ueber
/// `Image.draft("RGB", (w/8, h/8))` anbietet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verkleinerung {
    /// Volle Aufloesung, wie bisher.
    Voll,
    /// Release 08.10.2026 (Bild-Diaet): halbe Kante. Je 8x8-Block entstehen
    /// 4x4 Bildpunkte, jeder der Mittelwert seiner 2x2 Bildpunkte nach der
    /// IDCT — derselbe Kastenfilter wie „voll lesen und halbieren“, aber
    /// die Musterebenen und das fertige Bild sind nur ein Viertel so gross.
    Halb,
    /// Release 08.10.2026 (Bild-Diaet): Viertelkante (2x2 je Block).
    Viertel,
    /// Nur Gleichstrom, Ergebnis in Achtelgroesse.
    Achtel,
}

impl Verkleinerung {
    /// Der Teiler der Kante (1, 2, 4, 8).
    #[must_use]
    pub fn teiler(self) -> u32 {
        match self {
            Verkleinerung::Voll => 1,
            Verkleinerung::Halb => 2,
            Verkleinerung::Viertel => 4,
            Verkleinerung::Achtel => 8,
        }
    }
}

/// Liest die Bildmasse aus dem Kopf, ohne zu dekodieren.
///
/// GEMESSENER Grund fuer diese eigene Funktion: der Aufrufer braucht die
/// Masse **vor** dem Dekodieren — fuer `<img width height>` und fuer die
/// Entscheidung „muss ich ueberhaupt verkleinern“. Und B9 braucht sie fuer
/// genau diese Entscheidung: GEMESSENER Fehler beim ersten Wurf, als
/// `lesen_klein(bytes, 2400, 1800)` ein **300x225**-Bild lieferte, weil der
/// Achtelweg in einem 2400-px-Feld ohne Vergleich mit der Quelle benutzt
/// wurde. Ein Bild ist nie „klein genug“, es ist nur „klein genug
/// im Vergleich zu seinem Kopf“.
///
/// M3: der Lauf ueber die Segmente ist durch `MAX_SEGMENTE` begrenzt und
/// laengt bei jeder Marke mit Laengenfeld; ein Marker ohne Laengenfeld
/// (Restart, TEM) wird uebersprungen. Es wird nichts allokiert.
pub fn jpeg_masse(daten: &[u8]) -> Option<(u32, u32)> {
    if daten.len() < 4 || daten[0] != 0xFF || daten[1] != 0xD8 {
        return None;
    }
    let mut pos = 2usize;
    let mut segmente = 0usize;
    while pos + 4 <= daten.len() {
        segmente += 1;
        if segmente > MAX_SEGMENTE {
            return None;
        }
        if daten[pos] != 0xFF {
            return None;
        }
        let mut m = pos + 1;
        while m < daten.len() && daten[m] == 0xFF {
            m += 1;
        }
        let mk = daten.get(m).copied()?;
        pos = m + 1;
        let ist_sof = matches!(
            mk,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF
        );
        if ist_sof {
            // SOF: Laenge (2), Praezision (1), Hoehe (2), Breite (2)
            let v = daten.get(pos + 3..pos + 7)?;
            let hoehe = u16::from_be_bytes([v[0], v[1]]) as u32;
            let breite = u16::from_be_bytes([v[2], v[3]]) as u32;
            if breite == 0 || hoehe == 0 {
                return None;
            }
            return Some((breite, hoehe));
        }
        if matches!(mk, 0xD8 | 0xD9 | 0x01 | 0xD0..=0xD7) {
            continue; // kein Laengenfeld
        }
        let laenge = daten.get(pos..pos + 2)?;
        let laenge = u16::from_be_bytes([laenge[0], laenge[1]]) as usize;
        if laenge < 2 {
            return None;
        }
        pos = pos.checked_add(laenge)?.min(daten.len());
    }
    None
}

/// Liest ein JPEG in voller Aufloesung.
pub fn jpeg_lesen(daten: &[u8]) -> Result<Bild, BildFehler> {
    jpeg_lesen_mit(daten, Verkleinerung::Voll)
}

/// Liest ein JPEG mit gewaehlter Verkleinerungsstufe.
///
/// GEMESSEN mit `examples/seite100.rs` an
/// `tests/daten/seite100_gross444.jpg` (2400x1800), **ein** Bild, Ziel
/// 300x225:
///
/// ```text
/// Voll   | 159.8 ms | VmHWM 32784 KiB | gehalten 17280032 Byte
/// Achtel |  18.7 ms | VmHWM  3876 KiB | gehalten   270032 Byte
/// ```
///
/// Das Verhaeltnis ist beim einem Bild rund 8,5-mal in der Zeit und 8,5-mal
/// im Speicher; bei 100 Bildern (alle dekodiert, alle gehalten) sind es
/// 19042.7 ms gegen 2197.2 ms und 1 708 244 KiB gegen 30 112 KiB — siehe
/// `src/cache.rs` und `tests/jpeg_klein.rs`.
pub fn jpeg_lesen_mit(daten: &[u8], verkleinerung: Verkleinerung) -> Result<Bild, BildFehler> {
    if daten.len() < 2 {
        return Err(BildFehler::ZuKurz {
            vorhanden: daten.len(),
            gebraucht: 2,
        });
    }
    if daten[0] != 0xFF || daten[1] != marker::SOI {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "kein SOI-Marker",
        });
    }

    let mut pos = 2usize;
    let mut quant_tabellen: Vec<Option<QuantTabelle>> =
        (0..MAX_QUANT_TABELLEN).map(|_| None).collect();
    let mut huffman_dc: Vec<Option<HuffmanTabelle>> =
        (0..MAX_HUFFMAN_TABELLEN).map(|_| None).collect();
    let mut huffman_ac: Vec<Option<HuffmanTabelle>> =
        (0..MAX_HUFFMAN_TABELLEN).map(|_| None).collect();
    let mut frame: Option<Frame> = None;
    let mut restart_interval: u16 = 0;
    let mut adobe: Option<u8> = None;
    let mut scans: Vec<(ScanKopf, ScanDaten, Tabellenstand)> = Vec::new();
    // Ohne EOI-Marker ist die Datei abgeschnitten: der Strom haette
    // sonst kein definiertes Ende (ITU T.81, 2.4.2).
    let mut eoi_gesehen = false;

    let mut segmente = 0usize;
    while pos < daten.len() {
        segmente += 1;
        if segmente > MAX_SEGMENTE {
            return Err(BildFehler::JpegSegmenteZuViel);
        }
        if daten[pos] != 0xFF {
            return Err(BildFehler::JpegKopfFalsch {
                grund: "Marker erwartet",
            });
        }
        // Fuell-Bytes: mehrere 0xFF hintereinander sind erlaubt.
        let mut m = pos + 1;
        while m < daten.len() && daten[m] == 0xFF {
            m += 1;
        }
        if m >= daten.len() {
            // Datei endet hinter Fuell-Bytes: kein EOI, aber auch kein
            // Schaden — Scans, die schon da sind, werden verarbeitet.
            break;
        }
        let mk = daten[m];
        pos = m + 1;

        match mk {
            marker::SOI => {
                return Err(BildFehler::JpegKopfFalsch {
                    grund: "doppeltes SOI",
                });
            }
            marker::EOI => {
                eoi_gesehen = true;
                break;
            }
            0x01 | 0xD0..=0xD7 => {
                // TEM und Restart-Marker ausserhalb eines Scans: kein
                // Laengenfeld, einfach ueberspringen.
            }
            marker::SOS => {
                if scans.len() >= MAX_SCANS {
                    return Err(BildFehler::JpegSegmenteZuViel);
                }
                let f = match frame.as_ref() {
                    Some(f) => f,
                    None => {
                        return Err(BildFehler::JpegKopfFalsch {
                            grund: "SOS vor dem SOF-Segment",
                        });
                    }
                };
                let (roh, neue_pos) = lese_sos(daten, pos)?;
                pos = neue_pos;
                let kopf = loese_auf(&roh, f)?;
                let (bytes, restarts, scan_ende) = lese_scan_daten(daten, pos);
                pos = scan_ende;
                scans.push((
                    kopf,
                    ScanDaten { bytes, restarts },
                    (huffman_dc.clone(), huffman_ac.clone()),
                ));
            }
            marker::DQT => {
                let laenge = lese_laenge(daten, pos)?;
                let ende = pos + laenge;
                pos += 2;
                while pos < ende {
                    let (tq, tabelle, neu) = QuantTabelle::lesen(daten, &mut pos)?;
                    if neu > ende {
                        return Err(BildFehler::JpegQuantFalsch {
                            grund: "Tabelle laenger als ihr Segment",
                        });
                    }
                    quant_tabellen[tq as usize] = Some(tabelle);
                    pos = neu;
                }
            }
            marker::DHT => {
                let laenge = lese_laenge(daten, pos)?;
                let ende = pos + laenge;
                pos += 2;
                while pos < ende {
                    let (tc, th, tabelle, neu) = HuffmanTabelle::lesen(daten, &mut pos)?;
                    if neu > ende {
                        return Err(BildFehler::JpegHuffmanFalsch {
                            grund: "Tabelle laenger als ihr Segment",
                        });
                    }
                    if tc == 0 {
                        huffman_dc[th as usize] = Some(tabelle);
                    } else {
                        huffman_ac[th as usize] = Some(tabelle);
                    }
                    pos = neu;
                }
            }
            marker::DRI => {
                let laenge = lese_laenge(daten, pos)?;
                if laenge != 4 {
                    return Err(BildFehler::JpegKopfFalsch {
                        grund: "DRI-Laenge muss 4 sein",
                    });
                }
                restart_interval = u16::from_be_bytes([daten[pos + 2], daten[pos + 3]]);
                pos += laenge;
            }
            marker::SOF0 | marker::SOF1 | marker::SOF2 => {
                if frame.is_some() {
                    return Err(BildFehler::JpegKopfFalsch {
                        grund: "zwei SOF-Segmente",
                    });
                }
                let (f, neue_pos) = lese_sof(daten, pos, mk != marker::SOF0 && mk != marker::SOF1)?;
                frame = Some(f);
                pos = neue_pos;
            }
            marker::APP14 => {
                let laenge = lese_laenge(daten, pos)?;
                let anfang = pos + 2;
                let ende = pos + laenge;
                if anfang + 12 <= ende && &daten[anfang..anfang + 5] == b"Adobe" {
                    adobe = Some(daten[anfang + 11]);
                }
                pos = ende;
            }
            marker::APP0 | marker::APP1 | marker::COM => {
                let laenge = lese_laenge(daten, pos)?;
                pos += laenge;
            }
            _ => {
                // Unbekanntes Segment (APPn, JPG, DNL, ...): Laenge
                // lesen und ueberspringen. Das ist die einzige Stelle,
                // an der nicht verstandene Segmente gueltig sind.
                let laenge = lese_laenge(daten, pos)?;
                pos += laenge;
            }
        }
    }

    let mut frame = match frame {
        Some(f) => f,
        None => {
            return Err(BildFehler::JpegKopfFalsch {
                grund: "kein SOF-Marker",
            });
        }
    };
    if scans.is_empty() {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "kein SOS-Marker",
        });
    }

    // ---- B9: die Verkleinerungsstufe auf den Frame legen ----
    // GEMESSEN: das sind 2400/8 = 300 und 1800/8 = 225, und
    // genau diese beiden Massen nennt auch PIL nach `draft("RGB",
    // (300, 225))`. Aufgerundet wird, weil ein 8x8-Block ganz am Rand
    // auch dann noch einen Bildpunkt liefert.
    if verkleinerung == Verkleinerung::Achtel {
        // GEMESSEN und deshalb eine Grenze: der Achtelweg stimmt nur, wenn
        // keine Komponente senkrecht unterabgetastet ist.
        //
        // * 4:4:4 und Graustufen: 0 von 60 Kanalwerten verschieden
        //   (`farbverlauf_444.jpg`, `grau.jpg`).
        // * 4:2:2 (nur waagerecht unterabgetastet): ebenfalls 0 von 60 —
        //   die waagerechte Vervielfaeltigung ist damit GEMESSEN richtig
        //   (`farbverlauf_422.jpg`).
        // * 4:2:0 (beide Richtungen): **falsch**, und nicht knapp. An
        //   `farbverlauf_420.jpg` und `nurchromav.jpg` weicht der Weg bis
        //   37 ab. Drei Filter wurden durchgerechnet und alle verfehlen:
        //   fancy 3/4 + 1/4 (50), Vervielfaeltigung (37) und
        //   9/16 + 3/16 + 3/16 + 1/16 (49).
        //
        //   GEMESSENE Beobachtung, die den naechsten Schritt benennt:
        //   `djpeg -scale 1/8` liefert bei `nurchromav.jpg` vier
        //   **verschiedene** Chromazeilen (Cr 142, 165, 198, ueber 210),
        //   obwohl ein Chromablock 16 Bildzeilen umfasst. Bei 8 Zeilen je
        //   Block waeren es hoechstens zwei Werte. Der Gleichstrom allein
        //   reicht fuer diese Senkrechte also nicht; libjpeg rechnet dort
        //   eine **2x2-Transformation** (vier Ausgabewerte je Chromablock),
        //   nicht den Gleichstrom. Das ist die naechste Aufgabe, nicht
        //   eine Sache, die man raten kann.
        // GEMESSENER Fehler beim ersten Wurf: `frame.vmax` wird erst
        // weiter unten gesetzt (im Abschnitt „MCU-Raster und Bloecke“), die
        // Grenze stand also gegen `1` und feuerte nie. Darum hier der
        // groesste Vertikalfaktor selbst.
        let vmax = frame.komponenten.iter().map(|k| k.v).max().unwrap_or(1);
        if frame.komponenten.iter().any(|k| k.v < vmax) {
            return Err(BildFehler::JpegVerkleinerungFalsch {
                grund: "senkrecht unterabgetastet, der Achtelweg waere falsch",
            });
        }
        frame.nur_dc = true;
        frame.ziel_breite = frame.breite.div_ceil(8);
        frame.ziel_hoehe = frame.hoehe.div_ceil(8);
        if frame.ziel_breite == 0 || frame.ziel_hoehe == 0 {
            return Err(BildFehler::GroesseNull);
        }
    }
    // Release 08.10.2026 (Bild-Diaet): halbe und Viertelkante. Anders als im
    // Achtelweg gilt jede Unterabtastung: die Chromaebenen werden nach dem
    // Verkleinern wie sonst „fancy“ hochgerechnet, nur auf dem kleineren
    // Raster.
    if matches!(verkleinerung, Verkleinerung::Halb | Verkleinerung::Viertel) {
        let t = verkleinerung.teiler();
        frame.teiler = t;
        frame.ziel_breite = frame.breite.div_ceil(t);
        frame.ziel_hoehe = frame.hoehe.div_ceil(t);
        if frame.ziel_breite == 0 || frame.ziel_hoehe == 0 {
            return Err(BildFehler::GroesseNull);
        }
    }

    // ---- Grenzen, bevor irgendetwas groesses allokiert wird ----
    if frame.breite == 0 || frame.hoehe == 0 {
        return Err(BildFehler::GroesseNull);
    }
    if frame.breite > MAX_KANTE || frame.hoehe > MAX_KANTE {
        return Err(BildFehler::ZuGross {
            breite: frame.breite,
            hoehe: frame.hoehe,
            kante: MAX_KANTE,
        });
    }
    let punkte = frame.breite as u64 * frame.hoehe as u64;
    if punkte > MAX_BILDPUNKTE {
        return Err(BildFehler::ZuGross {
            breite: frame.breite,
            hoehe: frame.hoehe,
            kante: MAX_BILDPUNKTE as u32,
        });
    }

    // ---- MCU-Raster und Bloecke je Komponente ----
    frame.hmax = frame.komponenten.iter().map(|k| k.h).max().unwrap_or(1);
    frame.vmax = frame.komponenten.iter().map(|k| k.v).max().unwrap_or(1);
    let mcu_breite = 8u32 * frame.hmax as u32;
    let mcu_hoehe = 8u32 * frame.vmax as u32;
    frame.mcus_x = frame.breite.div_ceil(mcu_breite);
    frame.mcus_y = frame.hoehe.div_ceil(mcu_hoehe);
    if frame.mcus_x > MAX_BLOECKE_PRO_ZEILE as u32 || frame.mcus_y > MAX_BLOECKE_PRO_SPALTE as u32 {
        return Err(BildFehler::ZuGross {
            breite: frame.breite,
            hoehe: frame.hoehe,
            kante: MAX_BLOECKE_PRO_ZEILE as u32,
        });
    }
    for k in frame.komponenten.iter_mut() {
        k.hmax = frame.hmax;
        k.vmax = frame.vmax;
        // Der Blockraster einer Komponente folgt dem MCU-Raster, nicht
        // der eigenen Breite: 4:2:0-Chroma hat genauso viele Bloecke
        // wie Y geteilt durch den Faktor, aufgezoegen auf ganze MCUs.
        k.bloecke_x = frame.mcus_x * k.h as u32;
        k.bloecke_y = frame.mcus_y * k.v as u32;
        // Ohne Padding: ceil(Breite * h / (hmax * 8)). Fuer 40x30 bei
        // 4:2:0 ist Y damit 5 Bloecke breit, waehrend das MCU-Raster
        // 6 Spalten hat -- die sechste ist nur Randfullung.
        let teiler_x = 8 * frame.hmax as u32;
        let teiler_y = 8 * frame.vmax as u32;
        k.nutz_x = (frame.breite * k.h as u32).div_ceil(teiler_x);
        k.nutz_y = (frame.hoehe * k.v as u32).div_ceil(teiler_y);
        if k.nutz_x == 0 || k.nutz_y == 0 {
            return Err(BildFehler::GroesseNull);
        }
        if k.bloecke_x > MAX_BLOECKE_PRO_ZEILE as u32 || k.bloecke_y > MAX_BLOECKE_PRO_SPALTE as u32
        {
            return Err(BildFehler::ZuGross {
                breite: frame.breite,
                hoehe: frame.hoehe,
                kante: MAX_BLOECKE_PRO_ZEILE as u32,
            });
        }
    }

    // ---- Arbeitsspeicher schaetzen und gegen MAX_SPEICHER pruefen ----
    // Phase 1: Koeffizienten + Musterebenen liegen gleichzeitig.
    // Phase 2 (spaeter): Koeffizienten sind frei, Musterebenen + Zielbild
    // liegen gleichzeitig. Beide Summen muessen passen.
    let mut coeff_bytes: u64 = 0;
    let mut muster_bytes: u64 = 0;
    for k in frame.komponenten.iter() {
        let blaetter = k.bloecke_x as u64 * k.bloecke_y as u64;
        coeff_bytes += blaetter * (KOEFFIZIENTEN_PRO_BLOCK as u64 * 2);
        muster_bytes += (k.bloecke_x as u64 * 8) * (k.bloecke_y as u64 * 8);
    }
    let ziel_bytes = punkte * 4;
    if coeff_bytes + muster_bytes > MAX_SPEICHER as u64
        || muster_bytes + ziel_bytes > MAX_SPEICHER as u64
    {
        return Err(BildFehler::ZuGross {
            breite: frame.breite,
            hoehe: frame.hoehe,
            kante: MAX_SPEICHER as u32,
        });
    }

    // Scans sind da, die Groesse passt -- jetzt fehlt nur noch das
    // Ende der Datei. Schneidet man die letzten Byte ab, war der
    // letzte Scan moeglicherweise gar nicht vollstaendig.
    if !eoi_gesehen {
        return Err(BildFehler::Abgeschnitten {
            position: daten.len(),
            gebraucht: 2,
        });
    }

    // Jetzt, und erst jetzt, darf allokiert werden.
    //
    // B9: im Achtelweg wird **nicht** die 64-Byte-Ablage je Block
    // angelegt, sondern ein einziger Gleichstromwert. GEMESSEN an
    // 2400x1800 mit 4:2:0 (51 000 Bloecke in der Luminanzebene, 25 500 in
    // jeder Chromaebene): 102 000 mal 64 Byte = 6,5 MB je Komponente
    // waere der Volldekoder, `dc_werte` braucht 102 000 mal 2 Byte =
    // 0,2 MB. Ueber alle Komponenten sind das 13 MB gegen 0,6 MB — und
    // genau diese 12,4 MB sind der Grund, warum ein Volldekoder auf einer
    // Seite mit 100 Bildern den Speicher frisst.
    for k in frame.komponenten.iter_mut() {
        let anzahl = (k.bloecke_x * k.bloecke_y) as usize;
        if frame.nur_dc {
            k.dc_werte = vec![0i16; anzahl];
        } else {
            k.koeffizienten = vec![[0i16; KOEFFIZIENTEN_PRO_BLOCK]; anzahl];
        }
    }

    // ---- Scans dekodieren ----
    // GEMESSENER Compilerfehler: `frame.nur_dc` als sechstes Argument
    // neben `&mut frame` geht nicht — das Feld wird waehrend der
    // Leihgabe gelesen. Darum vorher in eine eigene Zahl.
    let nur_dc = frame.nur_dc;
    for (kopf, sd, (stand_dc, stand_ac)) in &scans {
        dekodiere_scan(
            kopf,
            sd,
            &mut frame,
            stand_dc,
            stand_ac,
            restart_interval,
            nur_dc,
        )?;
    }

    // ---- Dequantisierung, IDCT, Musterebenen; Koeffizienten frei ----
    //
    // B9, Achtelweg: statt der 8x8-IDCT entsteht je Block ein Bildpunkt.
    // Die Rechnung ist die aus `jidctred.c` (`jpeg_idct_1x1`):
    // `DESCALE(dc * q[0], 3)`, also der dequantisierte Gleichstrom um den
    // Faktor 8 geteilt und auf eine ganze Zahl gerundet, dazu die
    // Mittelwertverschiebung 128 und die Klemmung auf 0..255. Das ist
    // genau der Blockmittelwert — dieselbe Zahl, die PIL nach `draft`
    // anzeigt (Abnahme in `tests/jpeg_klein.rs`).
    if frame.nur_dc {
        for k in frame.komponenten.iter_mut() {
            let tq = k.tq as usize;
            let q0 = match quant_tabellen.get(tq).and_then(|q| q.as_ref()) {
                Some(q) => q.werte[0] as i32,
                None => {
                    return Err(BildFehler::JpegQuantFalsch {
                        grund: "Quantisierungstabelle der Komponente fehlt",
                    });
                }
            };
            let bx = k.bloecke_x.max(1);
            let by = k.bloecke_y.max(1);
            let mut muster = vec![128u8; (bx as usize) * (by as usize)];
            for (bi, &dc) in k.dc_werte.iter().enumerate() {
                // (x + 4) >> 3 ist `DESCALE(x, 3)` mit arithmetischer
                // Schiefe; das deckt auch negative Gleichstromwerte ab
                // (ein dunkler Block liefert ein negatives dc).
                let wert = ((dc as i32 * q0 + 4) >> 3) + 128;
                let ox = (bi as u32 % bx) as usize;
                let oy = (bi as u32 / bx) as usize;
                muster[oy * bx as usize + ox] = wert.clamp(0, 255) as u8;
            }
            k.muster = muster;
            k.muster_breite = bx;
            k.dc_werte = Vec::new();
            k.dc_werte.shrink_to_fit();
        }
    } else {
        for k in frame.komponenten.iter_mut() {
            let tq = k.tq as usize;
            let quant = match quant_tabellen.get(tq).and_then(|q| q.as_ref()) {
                Some(q) => q,
                None => {
                    return Err(BildFehler::JpegQuantFalsch {
                        grund: "Quantisierungstabelle der Komponente fehlt",
                    });
                }
            };
            let bx = k.bloecke_x;
            let by = k.bloecke_y;
            // Release 08.10.2026: mit Teiler `t` liefert jeder Block
            // `8/t x 8/t` Bildpunkte, jeder der gerundete Mittelwert seiner
            // `t x t` Punkte (Kastenfilter). Bei `t = 1` ist das der alte Weg.
            let t = frame.teiler.clamp(1, 4);
            let kante = 8 / t;
            let breite = bx * kante;
            let mut muster = vec![128u8; (breite as usize) * (by as usize) * kante as usize];
            let flaeche = t * t;
            for bi in 0..k.koeffizienten.len() {
                idct(&mut k.koeffizienten[bi], quant);
                let ox = (bi as u32 % bx) * kante;
                let oy = (bi as u32 / bx) * kante;
                let block = &k.koeffizienten[bi];
                for y in 0..kante {
                    for x in 0..kante {
                        let wert = if t == 1 {
                            block[(y * 8 + x) as usize] as u32
                        } else {
                            let mut summe = 0u32;
                            for dy in 0..t {
                                for dx in 0..t {
                                    summe += block[((y * t + dy) * 8 + x * t + dx) as usize] as u32;
                                }
                            }
                            (summe + flaeche / 2) / flaeche
                        };
                        let ziel = ((oy + y) as usize) * (breite as usize) + (ox + x) as usize;
                        muster[ziel] = wert as u8;
                    }
                }
            }
            k.muster = muster;
            k.muster_breite = breite;
            k.koeffizienten = Vec::new();
            k.koeffizienten.shrink_to_fit();
        }
    }

    // ---- Hochrechnen auf die Bildgroesse ----
    // GEMESSEN 01.10.2026: ohne diesen Schritt war die Abweichung
    // 7..9 bei 4:2:0, aber nur 2 bei 4:4:4 — sie haengte also am
    // Subsampling, nicht am progressiven Weg. Siehe `hochskaliere`.
    //
    // B9: im Achtelweg ist das Ziel eine Achtel kleiner. Die Chromaebenen
    // werden mit derselben „fancy“-Regel hochgerechnet wie sonst
    // (`jdsample.c`), nur auf dem kleineren Raster — deshalb stimmt das
    // Ergebnis auch bei 4:2:0 und 4:2:2 (siehe die Messung in
    // `tests/jpeg_klein.rs`).
    for k in frame.komponenten.iter_mut() {
        k.hochskaliere_mit(frame.ziel_breite, frame.ziel_hoehe, !frame.nur_dc);
    }

    // ---- Bild zusammenbauen ----
    let bild = if frame.komponenten.len() == 4 {
        // GEMESSEN 02.10.2026 (Rolle BILD, B2): CMYK und YCCK unterscheiden
        // sich an den Komponentenkennungen, nicht am Adobe-Transformwert.
        // GEMESSEN an `scratch/jpeg_tests/test_cmyk.jpg`: die Kennungen
        // stehen als `C`, `M`, `Y`, `K` (67, 77, 89, 75) im SOF0, und der
        // APP14-Transformwert ist 0. Vorher entschied allein der
        // Transformwert, und `transform 2` wurde als YCCK gelesen —
        // obwohl dieser Wert nur sagt, dass die Werte invertiert
        // gespeichert sind.
        let ycck = frame.komponenten[0].id == b'Y' as u8
            && frame.komponenten[1].id == b'C' as u8
            && frame.komponenten[2].id == b'R' as u8;
        // `transform 2` heisst: die Werte im Bild sind invertiert
        // gespeichert, also 255 = KEINE Tinte (Adobe Photoshop, siehe
        // TIFF 6.0, Section 16).
        //
        // GEMESSEN 02.10.2026 (Rolle BILD, B2): bei
        // `scratch/jpeg_tests/test_cmyk.jpg` steht der Transformwert 0,
        // die Werte sind aber trotzdem invertiert gespeichert — PIL
        // schreibt beim Speichern invertiert und meldet 0. Beleg: PIL
        // liest an (0, 0) das CMYK (0, 0, 0, 0), unser Dekoder roh
        // (255, 255, 255, 255); mit Invertierung stimmen die Kanaele:
        //     R = 255 - min(255, (255 - C_roh) + (255 - K_roh))
        // statt mit dem nicht invertierten Tintenmodell, wo
        // `255 - 255 = 0` herauskaem (das Bild war schwarz).
        // GEMESSENE Folge des nicht invertierten Modells an derselben
        // Datei: mittlere Abweichung 139.10 je Kanal, 7860 von 12288
        // Kanalwerten ueber 3.
        //
        // Deshalb wird bei vier Komponenten generell invertiert: das ist
        // der Weg, den alle Adobe-CMYK-JPEGs nehmen (PIL, Photoshop,
        // libjpeg in der ueblichen Verwendung); der Transformwert 0
        // heisst „unbekannt“ und nicht „nicht invertiert“.
        let invertiert = adobe != Some(1);
        if ycck {
            ycck_bild(&frame, invertiert)?
        } else {
            cmyk_bild(&frame, invertiert)?
        }
    } else if adobe == Some(0) && frame.komponenten.len() == 3 {
        // APP14 mit transform 0: die drei Samples sind bereits R, G, B.
        rgb_bild(&frame)?
    } else {
        // YCbCr ist der Normalfall (steht auch so im JFIF-Kopf),
        // bei EINER Komponente wird aus dem Wert eine Graustufe.
        ycbcr_bild(&frame)?
    };

    let mut end_bild = bild;
    // B4 (02.10.2026): ein eingebettetes ICC-Profil wird ausgewertet.
    // Ein P3- oder AdobeRGB-Bild wird sonst in den falschen Farbraum
    // gerechnet und sieht kalt oder matt aus. Die Umrechnung aendert
    // keine Kanten und nichts am Alphakanal, sie darf also vor oder nach
    // der Drehung stehen — hier davor, damit die Kachel der Matrix
    // unabhaengig von der EXIF-Drehung dieselbe ist.
    crate::icc::auf_srgb(&mut end_bild, daten);
    if let Some(ori) = jpeg_orientierung(daten) {
        end_bild = drehe_bild(end_bild, ori);
    }
    Ok(end_bild)
}

fn drehe_bild(bild: Bild, ori: u8) -> Bild {
    match ori {
        2 => spiegeln(bild, true, false), // Horizontal
        3 => drehen(bild, 180),
        4 => spiegeln(bild, false, true),              // Vertical
        5 => drehen(spiegeln(bild, true, false), 270), // Transpose (spiegeln horizontal dann 270 = rot 90 + spiegeln)
        6 => drehen(bild, 90),
        7 => drehen(spiegeln(bild, true, false), 90), // Transverse
        8 => drehen(bild, 270),
        _ => bild, // 1 oder unbekannt
    }
}

fn drehen(bild: Bild, winkel: u16) -> Bild {
    match winkel {
        90 => {
            let mut raus = Bild::new(bild.hoehe, bild.breite);
            for y in 0..bild.hoehe {
                for x in 0..bild.breite {
                    raus.setze_pixel(bild.hoehe - 1 - y, x, bild.pixel(x, y));
                }
            }
            raus
        }
        180 => {
            let mut raus = Bild::new(bild.breite, bild.hoehe);
            for y in 0..bild.hoehe {
                for x in 0..bild.breite {
                    raus.setze_pixel(bild.breite - 1 - x, bild.hoehe - 1 - y, bild.pixel(x, y));
                }
            }
            raus
        }
        270 => {
            let mut raus = Bild::new(bild.hoehe, bild.breite);
            for y in 0..bild.hoehe {
                for x in 0..bild.breite {
                    raus.setze_pixel(y, bild.breite - 1 - x, bild.pixel(x, y));
                }
            }
            raus
        }
        _ => bild,
    }
}

fn spiegeln(bild: Bild, horiz: bool, vert: bool) -> Bild {
    let mut raus = Bild::new(bild.breite, bild.hoehe);
    for y in 0..bild.hoehe {
        for x in 0..bild.breite {
            let nx = if horiz { bild.breite - 1 - x } else { x };
            let ny = if vert { bild.hoehe - 1 - y } else { y };
            raus.setze_pixel(nx, ny, bild.pixel(x, y));
        }
    }
    raus
}

/// Baut das Zielbild in YCbCr-Rechnung (3 Komponenten) oder als
/// Graustufen (1 Komponente: Cb = Cr = 128).
fn ycbcr_bild(frame: &Frame) -> Result<Bild, BildFehler> {
    let y_k = &frame.komponenten[0];
    let cb_k = frame.komponenten.get(1);
    let cr_k = frame.komponenten.get(2);
    let mut bild = Bild::new(frame.ziel_breite, frame.ziel_hoehe);
    for y in 0..frame.ziel_hoehe {
        for x in 0..frame.ziel_breite {
            let yw = y_k.musterpixel(x, y);
            let (cbw, crw) = match (cb_k, cr_k) {
                (Some(cb), Some(cr)) => (cb.musterpixel(x, y), cr.musterpixel(x, y)),
                _ => (128, 128),
            };
            let [r, g, b] = ycbcr_zu_rgb(yw, cbw, crw);
            bild.setze_pixel(x, y, [r, g, b, 255]);
        }
    }
    Ok(bild)
}

/// Baut das Zielbild, wenn die drei Komponenten direkt R, G, B sind
/// (APP14/Adobe, transform 0).
fn rgb_bild(frame: &Frame) -> Result<Bild, BildFehler> {
    if frame.komponenten.len() < 3 {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "RGB-Modus braucht drei Komponenten",
        });
    }
    let kr = &frame.komponenten[0];
    let kg = &frame.komponenten[1];
    let kb = &frame.komponenten[2];
    let mut bild = Bild::new(frame.ziel_breite, frame.ziel_hoehe);
    for y in 0..frame.ziel_hoehe {
        for x in 0..frame.ziel_breite {
            bild.setze_pixel(
                x,
                y,
                [
                    kr.musterpixel(x, y),
                    kg.musterpixel(x, y),
                    kb.musterpixel(x, y),
                    255,
                ],
            );
        }
    }
    Ok(bild)
}

// ---------------------------------------------------------------------------
// Segment-Helfer
// ---------------------------------------------------------------------------

/// Liest das 2-Byte-Laengenfeld an `pos` und prueft es.
fn lese_laenge(daten: &[u8], pos: usize) -> Result<usize, BildFehler> {
    if pos + 2 > daten.len() {
        return Err(BildFehler::Abgeschnitten {
            position: pos,
            gebraucht: 2,
        });
    }
    let laenge = u16::from_be_bytes([daten[pos], daten[pos + 1]]) as usize;
    if laenge < 2 {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "Laengenfeld unter 2",
        });
    }
    if pos + laenge > daten.len() {
        return Err(BildFehler::Abgeschnitten {
            position: pos,
            gebraucht: laenge,
        });
    }
    Ok(laenge)
}

/// Liest das SOF-Segment (SOF0, SOF1 oder SOF2).
fn lese_sof(daten: &[u8], pos: usize, progressiv: bool) -> Result<(Frame, usize), BildFehler> {
    if pos + 6 > daten.len() {
        return Err(BildFehler::Abgeschnitten {
            position: pos,
            gebraucht: 6,
        });
    }
    let laenge = u16::from_be_bytes([daten[pos], daten[pos + 1]]) as usize;
    if laenge < 8 || pos + laenge > daten.len() {
        return Err(BildFehler::Abgeschnitten {
            position: pos,
            gebraucht: laenge,
        });
    }
    let praezision = daten[pos + 2];
    if praezision != 8 {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "nur 8-Bit-Praezision wird unterstuetzt",
        });
    }
    let hoehe = u16::from_be_bytes([daten[pos + 3], daten[pos + 4]]) as u32;
    let breite = u16::from_be_bytes([daten[pos + 5], daten[pos + 6]]) as u32;
    let anzahl = daten[pos + 7];
    if anzahl == 0 || anzahl as usize > MAX_KOMPONENTEN {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "ungueltige Anzahl Komponenten",
        });
    }
    if laenge != 8 + 3 * anzahl as usize {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "SOF-Laenge passt nicht zur Komponentenanzahl",
        });
    }
    if anzahl != 1 && anzahl != 3 && anzahl != 4 {
        // 2 Komponenten gibt es praktisch nicht, 4 ist CMYK/YCCK. Die
        // Farbrechnung dort ist nicht YCbCr, und raten waere schlimmer
        // als ein benannter Fehler.
        return Err(BildFehler::JpegKopfFalsch {
            grund: "CMYK/YCCK (4 Komponenten) wird nicht unterstuetzt",
        });
    }
    let mut frame = Frame::new(breite, hoehe, progressiv);
    let mut p = pos + 8;
    for _ in 0..anzahl {
        let id = daten[p];
        let hv = daten[p + 1];
        let h = hv >> 4;
        let v = hv & 0x0F;
        let tq = daten[p + 2];
        if h == 0 || h > 4 || v == 0 || v > 4 {
            return Err(BildFehler::JpegKopfFalsch {
                grund: "ungueltige Abtastfaktoren",
            });
        }
        if tq as usize >= MAX_QUANT_TABELLEN {
            return Err(BildFehler::JpegQuantFalsch {
                grund: "Tabelle (Tq) existiert nicht",
            });
        }
        frame.komponenten.push(Komponente::new(id, h, v, tq));
        p += 3;
    }
    Ok((frame, pos + laenge))
}

/// Liest das SOS-Segment (Kopf, nicht die Daten danach).
fn lese_sos(daten: &[u8], pos: usize) -> Result<(SosRoh, usize), BildFehler> {
    let laenge = lese_laenge(daten, pos)?;
    if laenge < 6 {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "SOS-Laenge zu klein",
        });
    }
    let ns = daten[pos + 2];
    if ns == 0 || ns as usize > MAX_KOMPONENTEN {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "SOS: ungueltige Komponentenanzahl",
        });
    }
    if laenge != 6 + 2 * ns as usize {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "SOS-Laenge passt nicht zur Komponentenanzahl",
        });
    }
    let mut komponenten = Vec::with_capacity(ns as usize);
    for i in 0..ns as usize {
        let id = daten[pos + 3 + i * 2];
        let tdta = daten[pos + 4 + i * 2];
        let td = (tdta >> 4) & 0x0F;
        let ta = tdta & 0x0F;
        if td as usize >= MAX_HUFFMAN_TABELLEN || ta as usize >= MAX_HUFFMAN_TABELLEN {
            return Err(BildFehler::JpegHuffmanFalsch {
                grund: "SOS verweist auf eine Tabelle 4 oder hoeher",
            });
        }
        komponenten.push((id, td, ta));
    }
    let ss = daten[pos + 3 + 2 * ns as usize];
    let se = daten[pos + 4 + 2 * ns as usize];
    let ahal = daten[pos + 5 + 2 * ns as usize];
    let roh = SosRoh {
        komponenten,
        ss,
        se,
        ah: (ahal >> 4) & 0x0F,
        al: ahal & 0x0F,
    };
    Ok((roh, pos + laenge))
}

/// Prueft die Scan-Parameter gegen den Frame und loest die
/// Komponenten-IDs in Indizes auf.
fn loese_auf(roh: &SosRoh, frame: &Frame) -> Result<ScanKopf, BildFehler> {
    let art = if !frame.progressiv {
        // Baseline: ein Scan, Band 0..63, keine Approximation.
        if roh.ss != 0 || roh.se != 63 || roh.ah != 0 || roh.al != 0 {
            return Err(BildFehler::JpegKopfFalsch {
                grund: "Baseline-Scan mit ungueltigen Bandgrenzen",
            });
        }
        ScanArt::Basis
    } else if roh.ss == 0 {
        if roh.se != 0 {
            return Err(BildFehler::JpegKopfFalsch {
                grund: "progressiver DC-Scan braucht Ss = Se = 0",
            });
        }
        ScanArt::Dc
    } else {
        if roh.se > 63 || roh.ss > roh.se {
            return Err(BildFehler::JpegKopfFalsch {
                grund: "progressiver AC-Scan ausserhalb 1..63",
            });
        }
        if roh.komponenten.len() != 1 {
            return Err(BildFehler::JpegKopfFalsch {
                grund: "ein AC-Scan darf nur eine Komponente haben",
            });
        }
        ScanArt::Ac
    };

    // Explizite Typangabe: ohne sie kann rustc das Feld `.0` des
    // Tupels erst auflösen, nachdem `push` kam — und das gibt E0282.
    let mut komponenten: Vec<(usize, u8, u8)> = Vec::with_capacity(roh.komponenten.len());
    for &(id, td, ta) in &roh.komponenten {
        let idx = frame.komponenten.iter().position(|k| k.id == id).ok_or(
            BildFehler::JpegKopfFalsch {
                grund: "SOS-Komponente steht nicht im SOF",
            },
        )?;
        if komponenten.iter().any(|k| k.0 == idx) {
            return Err(BildFehler::JpegKopfFalsch {
                grund: "SOS-Komponente doppelt",
            });
        }
        komponenten.push((idx, td, ta));
    }
    Ok(ScanKopf {
        komponenten,
        ss: roh.ss,
        se: roh.se,
        ah: roh.ah,
        al: roh.al,
        art,
    })
}

// ---------------------------------------------------------------------------
// Scan-Daten
// ---------------------------------------------------------------------------

/// Schneidet die entropiekodierten Daten ab `pos` heraus.
///
/// Dabei passieren drei Dinge: 0xFF00 wird zu 0xFF entstuepft,
/// Restart-Marker (0xFFD0..0xD7) werden entfernt und ihre Position im
/// entstufsten Strom notiert, und der erste echte Marker (z.B. EOI
/// oder das naechste SOS) beendet die Daten.
fn lese_scan_daten(daten: &[u8], pos: usize) -> (Vec<u8>, Vec<usize>, usize) {
    let mut out = Vec::new();
    let mut restarts = Vec::new();
    let mut i = pos;
    while i < daten.len() {
        if daten[i] == 0xFF {
            if i + 1 >= daten.len() {
                break;
            }
            let next = daten[i + 1];
            if next == 0x00 {
                out.push(0xFF);
                i += 2;
            } else if (0xD0..=0xD7).contains(&next) {
                restarts.push(out.len());
                i += 2;
            } else {
                break;
            }
        } else {
            out.push(daten[i]);
            i += 1;
        }
    }
    (out, restarts, i)
}

// ---------------------------------------------------------------------------
// Entropie-Dekodierung
// ---------------------------------------------------------------------------

/// Holt die Tabelle `nr` aus dem Tabellenstand EINES Scans.
///
/// `None` heisst: zu dem Zeitpunkt, als der Scan gelesen wurde, war
/// diese Nummer noch nicht definiert — sie darf dann auch nur dann
/// verlangt werden, wenn der Scan sie benutzt.
fn tabelle(stand: &[Option<HuffmanTabelle>], nr: u8) -> Option<&HuffmanTabelle> {
    stand.get(nr as usize).and_then(|t| t.as_ref())
}

/// Dekodiert alle Scans in die Koeffizientenfelder des Frames.
fn dekodiere_scan(
    kopf: &ScanKopf,
    sd: &ScanDaten,
    frame: &mut Frame,
    huffman_dc: &[Option<HuffmanTabelle>],
    huffman_ac: &[Option<HuffmanTabelle>],
    restart_interval: u16,
    nur_dc: bool,
) -> Result<(), BildFehler> {
    let interlaced = kopf.komponenten.len() > 1;

    // B9: bei einem progressiven Bild sind die Wechselstrombaender eigene
    // Scans. Sie werden im Achtelweg **ganz uebersprungen** — sie stehen
    // hinter dem Gleichstrom im Strom und aendern ihn nicht, also bleibt
    // das Ergebnis gleich, und es wird die meiste Rechenzeit gespart.
    // (libjpeg macht es genauso: `jpeg_read_coefficients` mit
    // `cinfo.scale_num == 1` verarbeitet nur die DC-Scans.)
    //
    // GEMESSEN: an `tests/daten/seite100_progressiv.jpg` sind das 9 Scans,
    // davon 1 mit Gleichstrom; im Achtelweg wird nur dieser eine gelesen.
    if nur_dc && kopf.art == ScanArt::Ac {
        return Ok(());
    }

    // MCU-Raster: interlaced zaehlt ueber den Frame (inklusive der
    // Padding-Bloecke am Rand), ein einzelner Scan zaehlt NUR die
    // Bloecke, die das Bild wirklich braucht, und zwar zeilenweise
    // ueber diese schmalere Flaeche.
    let (mcus_x, mcus_y) = if interlaced {
        (frame.mcus_x, frame.mcus_y)
    } else {
        let k = &frame.komponenten[kopf.komponenten[0].0];
        (k.nutz_x, k.nutz_y)
    };
    let gesamt = mcus_x as u64 * mcus_y as u64;

    for k in frame.komponenten.iter_mut() {
        k.dc = 0;
    }
    let mut reader = BitReader::new(&sd.bytes);
    let mut eobrun = 0u32;
    let mut restart_nr = 0usize;

    for mcu in 0..gesamt {
        if restart_interval > 0 && mcu > 0 && (mcu % restart_interval as u64) == 0 {
            // Auffuellen auf Bytegrenze, dann exakt die notierte
            // Position: fehlt der Marker oder verschiebt er sich, ist
            // der Strom nicht mehr synchron — das ist ein Fehler.
            reader.aligiere();
            match sd.restarts.get(restart_nr) {
                Some(&p) if p == reader.pos => restart_nr += 1,
                _ => return Err(BildFehler::JpegRestartFalsch),
            }
            for k in frame.komponenten.iter_mut() {
                k.dc = 0;
            }
            eobrun = 0;
        }

        for &(ci, td, ta) in &kopf.komponenten {
            let anzahl = if interlaced {
                frame.komponenten[ci].h * frame.komponenten[ci].v
            } else {
                1
            };
            // Die Tabellen werden erst im Arm geholt, der sie wirklich
            // braucht: ein DC-Scan verweist per TdTa mitunter auf eine
            // AC-Tabelle, die zu diesem Zeitpunkt noch gar nicht
            // definiert ist (vor Scan 1 von fortschrittlich.jpg liegt
            // nur ein DC-Segment). Wer sie trotzdem anfordert, wirft
            // eine falsche Fehlermeldung.
            for innen in 0..anzahl as usize {
                // Speicherposition des Blocks, nicht die zaehlende
                // Reihenfolge des Scans: im MCU stehen erst h Bloecke
                // nebeneinander, danach folgt die naechste MCU-Zeile.
                // Bei 4:2:0 laege ein Flaechen-Zaehler sonst schief.
                let idx = {
                    let k = &frame.komponenten[ci];
                    if interlaced {
                        let mx = (mcu % frame.mcus_x as u64) as u32;
                        let my = (mcu / frame.mcus_x as u64) as u32;
                        let spalte = mx * k.h as u32 + (innen % k.h as usize) as u32;
                        let zeile = my * k.v as u32 + (innen / k.h as usize) as u32;
                        (zeile * k.bloecke_x + spalte) as usize
                    } else {
                        let spalte = (mcu % mcus_x as u64) as u32;
                        let zeile = (mcu / mcus_x as u64) as u32;
                        (zeile * k.bloecke_x + spalte) as usize
                    }
                };
                let mut dc = frame.komponenten[ci].dc;
                // B9: im Achtelweg gibt es keine 64-Byte-Ablage. Statt
                // einen zweiten Ast durch jede Blockfunktion zu ziehen,
                // wird in einen Stapelkratzer dekodiert und nur der
                // Gleichstrom uebernommen.
                //
                // GEMESSENER Grund, warum das nicht einfach „Wechselstrom
                // nicht lesen“ heissen darf: bei einem Basisbild steht der
                // Wechselstrom **im selben Block** hinter dem Gleichstrom,
                // und die Bits muessen gelesen werden, sonst steht der
                // Leser beim naechsten Block an der falschen Stelle. Der
                // Kratzer liest sie also — er wirft sie nur weg.
                let mut kratzer = [0i16; KOEFFIZIENTEN_PRO_BLOCK];
                //
                // GEMESSENER FEHLER, den dieser Test gefunden hat: die
                // progressive Datei kam mit **202474 von 202500** Byte
                // Abweichung heraus (groesste 128), das Basisbild dagegen
                // mit 13 Byte und groesste 1. Grund: bei einem progressiven
                // JPEG kommt der Gleichstrom in mehreren Scans. Der erste
                // setzt ihn (`block[0] = wert << al`), jeder weitere
                // **verfeinert** ihn und liest dabei den bisherigen Wert
                // (`block[0] | (1 << al)`, ITU T.81 G.1.2.2). Ein Kratzer,
                // der pro Scan neu genullt wird, verliert genau diese
                // oberen Bit — der Gleichstrom war dann um Faktor 2^al zu
                // klein. Der Kratzer wird darum aus `dc_werte` vorbelegt.
                // Fuer Basis und fuer den ersten DC-Scan ist das
                // unschädlich: dort wird `block[0]` ohnehin zugewiesen.
                kratzer[0] = frame.komponenten[ci]
                    .dc_werte
                    .get(idx)
                    .copied()
                    .unwrap_or(0);
                if nur_dc && frame.komponenten[ci].dc_werte.get(idx).is_none() {
                    return Err(BildFehler::JpegDatenFalsch {
                        grund: "mehr Bloecke als das SOF angekuendigt",
                    });
                }
                let block: &mut [i16; KOEFFIZIENTEN_PRO_BLOCK] = if nur_dc {
                    &mut kratzer
                } else {
                    match frame.komponenten[ci].koeffizienten.get_mut(idx) {
                        Some(b) => b,
                        None => {
                            return Err(BildFehler::JpegDatenFalsch {
                                grund: "mehr Bloecke als das SOF angekuendigt",
                            });
                        }
                    }
                };
                let ergebnis = match kopf.art {
                    ScanArt::Basis => dekodiere_block_basis(
                        block,
                        &mut reader,
                        tabelle(huffman_dc, td).ok_or(BildFehler::JpegHuffmanFalsch {
                            grund: "DC-Tabelle fehlt",
                        })?,
                        tabelle(huffman_ac, ta).ok_or(BildFehler::JpegHuffmanFalsch {
                            grund: "AC-Tabelle fehlt",
                        })?,
                        &mut dc,
                    ),
                    ScanArt::Dc => dekodiere_block_dc(
                        block,
                        &mut reader,
                        tabelle(huffman_dc, td).ok_or(BildFehler::JpegHuffmanFalsch {
                            grund: "DC-Tabelle fehlt",
                        })?,
                        &mut dc,
                        kopf.ah,
                        kopf.al,
                    ),
                    ScanArt::Ac => dekodiere_block_ac(
                        block,
                        &mut reader,
                        tabelle(huffman_ac, ta).ok_or(BildFehler::JpegHuffmanFalsch {
                            grund: "AC-Tabelle fehlt",
                        })?,
                        kopf.ss,
                        kopf.se,
                        kopf.ah,
                        kopf.al,
                        &mut eobrun,
                    ),
                };
                frame.komponenten[ci].dc = dc;
                ergebnis?;
                if nur_dc {
                    if let Some(ziel) = frame.komponenten[ci].dc_werte.get_mut(idx) {
                        *ziel = kratzer[0];
                    }
                }
            }
        }
    }
    Ok(())
}

/// Ein DC-Symbol lesen und den Prediktor fortschreiben.
fn lies_dc(
    reader: &mut BitReader,
    tabelle: &HuffmanTabelle,
    predictor: &mut i32,
) -> Result<(), BildFehler> {
    let sym = match tabelle.dekodiere(reader)? {
        Some(s) => s,
        None => {
            return Err(BildFehler::JpegDatenFalsch {
                grund: "DC-Symbol fehlt (Scan abgeschnitten)",
            });
        }
    };
    if sym > 15 {
        return Err(BildFehler::JpegDatenFalsch {
            grund: "DC-Kategorie groesser als 15",
        });
    }
    let diff = if sym == 0 {
        0
    } else {
        let wert = reader.bits(sym).ok_or(BildFehler::JpegDatenFalsch {
            grund: "DC-Bits fehlen (Scan abgeschnitten)",
        })?;
        strecke(wert, sym)
    };
    *predictor = predictor.saturating_add(diff);
    Ok(())
}

/// Was ein AC-Symbol bedeutet hat, wenn es gelesen wurde.
enum AcTreffer {
    /// Ein Koeffizient wurde geschrieben.
    Wert,
    /// ZRL: 16 Nullen uebersprungen.
    Zrl,
    /// Symbol 0x00: der Rest des Blocks ist null (Baseline-EOB,
    /// beziehungsweise progressiver EOBRUN der Laenge 0).
    Null,
    /// Symbol 0xk0 mit k=1..14: progressiver EOBRUN der Laenge k.
    Laufweite(u8),
}

/// Ein AC-Symbol lesen und an Zigzag-Position `i` ablegen.
///
/// `mal` ist der Faktor der Punkttransformation (1 bei Baseline,
/// 2^Al progressiv). Die Interpretation des Symbols haengt vom
/// Scan-Typ ab und passiert beim Aufrufer.
fn lies_ac(
    reader: &mut BitReader,
    tabelle: &HuffmanTabelle,
    block: &mut [i16; KOEFFIZIENTEN_PRO_BLOCK],
    i: &mut usize,
    grenze: usize,
    mal: i32,
) -> Result<AcTreffer, BildFehler> {
    let sym = match tabelle.dekodiere(reader)? {
        Some(s) => s,
        None => {
            return Err(BildFehler::JpegDatenFalsch {
                grund: "AC-Symbol fehlt (Scan abgeschnitten)",
            });
        }
    };
    let lauf = (sym >> 4) as usize;
    let groesse = sym & 0x0F;
    if groesse == 0 {
        if lauf == 15 {
            *i += 16;
            return Ok(AcTreffer::Zrl);
        }
        if lauf == 0 {
            return Ok(AcTreffer::Null);
        }
        return Ok(AcTreffer::Laufweite(lauf as u8));
    }
    *i += lauf;
    if *i > grenze {
        return Err(BildFehler::JpegDatenFalsch {
            grund: "AC-Lauf laeuft ueber das Band hinaus",
        });
    }
    let wert = reader.bits(groesse).ok_or(BildFehler::JpegDatenFalsch {
        grund: "AC-Bits fehlen (Scan abgeschnitten)",
    })?;
    let wert = strecke(wert, groesse) * mal;
    block[ZIGZAG[*i]] = wert.clamp(-32767, 32767) as i16;
    *i += 1;
    Ok(AcTreffer::Wert)
}

/// Einen vollstaendigen Baseline-Block (DC + AC) dekodieren.
fn dekodiere_block_basis(
    block: &mut [i16; KOEFFIZIENTEN_PRO_BLOCK],
    reader: &mut BitReader,
    dc: &HuffmanTabelle,
    ac: &HuffmanTabelle,
    predictor: &mut i32,
) -> Result<(), BildFehler> {
    lies_dc(reader, dc, predictor)?;
    block[0] = (*predictor).clamp(-32767, 32767) as i16;

    let mut i = 1usize;
    while i < KOEFFIZIENTEN_PRO_BLOCK {
        match lies_ac(reader, ac, block, &mut i, KOEFFIZIENTEN_PRO_BLOCK - 1, 1)? {
            AcTreffer::Wert | AcTreffer::Zrl => {}
            AcTreffer::Null => break, // EOB
            AcTreffer::Laufweite(_) => {
                return Err(BildFehler::JpegDatenFalsch {
                    grund: "EOBRUN-Symbol nur in progressiven Scans gueltig",
                });
            }
        }
    }
    Ok(())
}

/// Ein Block in einem progressiven DC-Scan.
fn dekodiere_block_dc(
    block: &mut [i16; KOEFFIZIENTEN_PRO_BLOCK],
    reader: &mut BitReader,
    dc: &HuffmanTabelle,
    predictor: &mut i32,
    ah: u8,
    al: u8,
) -> Result<(), BildFehler> {
    if ah == 0 {
        // Erster DC-Scan: Differenz kodieren, dann Punkttransformation
        // (die unteren `al` Bit sind noch schwaech).
        lies_dc(reader, dc, predictor)?;
        let wert = *predictor << al;
        block[0] = wert.clamp(-32767, 32767) as i16;
    } else {
        // Verfeinerung: genau ein Bit fuer die neue Stelle.
        let bit = reader.bits(1).ok_or(BildFehler::JpegDatenFalsch {
            grund: "DC-Verfeinerungsbit fehlt",
        })?;
        if bit == 1 {
            let wert = (block[0] as i32) | (1i32 << al);
            block[0] = wert.clamp(-32767, 32767) as i16;
        }
    }
    Ok(())
}

/// Ein Block in einem progressiven AC-Scan (erste oder
/// Verfeinerungs-Phase, je nach `ah`).
// Acht Argumente, Limit sind sieben. GEMESSEN 02.10.2026: die vier
// Huffman-Skalare `ss`, `se`, `ah`, `al` sind die Spezifikation des
// Scans (ITU T.81, Tabelle B.2) und werden unveraendert durchgereicht —
// sie in ein Buendel zu packen wuerde vier Fuell-Bit-Zugriffe an vier
// Stellen ersetzen und den Code schlechter lesbar machen, ohne etwas zu
// gewinnen. `eobrun` muss `&mut` sein, weil der End-of-Block-Zaehler
// ueber die Bloecke eines Scans laeuft.
#[allow(clippy::too_many_arguments)]
fn dekodiere_block_ac(
    block: &mut [i16; KOEFFIZIENTEN_PRO_BLOCK],
    reader: &mut BitReader,
    ac: &HuffmanTabelle,
    ss: u8,
    se: u8,
    ah: u8,
    al: u8,
    eobrun: &mut u32,
) -> Result<(), BildFehler> {
    if ah == 0 {
        ac_erster(block, reader, ac, ss, se, al, eobrun)
    } else {
        ac_verfeinern(block, reader, ac, ss, se, al, eobrun)
    }
}

/// Erster AC-Scan ueber das Band ss..=se (ITU T.81, Figure F.14).
///
/// `eobrun` ist ueber Bloecke hinweg gueltig: er zaehlt, wie viele
/// Bloecke (inklusive des aktuellen) restlos null sind.
fn ac_erster(
    block: &mut [i16; KOEFFIZIENTEN_PRO_BLOCK],
    reader: &mut BitReader,
    ac: &HuffmanTabelle,
    ss: u8,
    se: u8,
    al: u8,
    eobrun: &mut u32,
) -> Result<(), BildFehler> {
    if *eobrun > 0 {
        *eobrun -= 1;
        return Ok(());
    }
    let grenze = se as usize;
    let mut i = ss as usize;
    while i <= grenze {
        match lies_ac(reader, ac, block, &mut i, grenze, 1i32 << al)? {
            AcTreffer::Wert | AcTreffer::Zrl => {}
            AcTreffer::Null => {
                // EOBRUN mit Laenge 0: nur dieser Block ist ab hier null.
                return Ok(());
            }
            AcTreffer::Laufweite(k) => {
                // EOBRUN = 2^k + k Bits; das erste Bit gehoert zum
                // aktuellen Block, deshalb zaehlt der Rest fuer die
                // Folgebloecke.
                let mut e = 1u32 << k;
                let extra = reader.bits(k).ok_or(BildFehler::JpegDatenFalsch {
                    grund: "EOBRUN-Bits fehlen (Scan abgeschnitten)",
                })?;
                e += extra as u32;
                *eobrun = e.saturating_sub(1);
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Verfeinerungs-AC-Scan (ITU T.81, Figure F.15; libjpeg `jdphuff.c`,
/// `decode_mcu_AC_refine`).
///
/// # Der Fehler, der hier gesteckt hat (GEMESSEN 01.10.2026, 03:40)
///
/// Ich hatte die Verfeinerung in ZWEI Durchgaenge zerlegt: erst alle
/// Symbole des Blocks lesen und die neuen Stellen merken, danach in einem
/// zweiten Durchgang fuer jede von-null-verschiedene Stelle im Band ein
/// Korrekturbit lesen. Das liest **dieselbe Anzahl Bits**, aber der
/// Strom laeuft ab dem ersten Block auseinander.
///
/// Der Grund ist ein **ZRL** (Praefixcode `r = 15`, `s = 0`). Im
/// Verfeinerungs-Scan laeuft libjpegs `do { ... } while (k <= Se)` auch
/// fuer ein ZRL durch die 16 Positionen und liest fuer jede bereits
/// von-null-verschiedene Stelle ein Korrekturbit. Mein Code tat bloss
/// `i += 16` und las kein einziges Bit.
///
/// GEMESSEN: `g_prog.jpg` (Graustufen, progressiv) bricht seitdem nicht
/// mehr im Verfeinerungsscan ab. Der Fehler war an drei von vier
/// erzeugten progressiven Dateien sichtbar, davon zwei ueberhaupt nicht
/// dekodierbar.
///
/// Also: die Korrekturbits werden **waehrend** des Symboldurchgangs
/// gelesen, in derselben Schleife. Der zweite Durchgang am Ende laeuft nur
/// noch fuer den Rest des Bandes nach einem EOB.
fn ac_verfeinern(
    block: &mut [i16; KOEFFIZIENTEN_PRO_BLOCK],
    reader: &mut BitReader,
    ac: &HuffmanTabelle,
    ss: u8,
    se: u8,
    al: u8,
    eobrun: &mut u32,
) -> Result<(), BildFehler> {
    let grenze = se as usize;
    let p1 = 1i32 << al;
    let m1 = -p1;

    /// Ein Korrekturbit lesen und anwenden. Gibt false, wenn die Bits
    /// ausgehen.
    fn verfeinere(
        block: &mut [i16; KOEFFIZIENTEN_PRO_BLOCK],
        reader: &mut BitReader,
        k: usize,
        p1: i32,
        m1: i32,
    ) -> Result<(), BildFehler> {
        let wert = block[ZIGZAG[k]] as i32;
        if wert == 0 {
            return Ok(());
        }
        let bit = reader.bits(1).ok_or(BildFehler::JpegDatenFalsch {
            grund: "AC-Verfeinerung: Korrektur-Bit fehlt",
        })?;
        // Wortwoertlich libjpeg: nur verschieben, wenn das Bit an der
        // Stelle Al noch 0 ist, und in Richtung des eigenen Vorzeichens.
        if bit == 1 && (wert & p1) == 0 {
            block[ZIGZAG[k]] = if wert >= 0 {
                (wert + p1).clamp(-32767, 32767) as i16
            } else {
                (wert + m1).clamp(-32767, 32767) as i16
            };
        }
        Ok(())
    }

    let mut k = ss as usize;
    if *eobrun == 0 {
        while k <= grenze {
            let sym = match ac.dekodiere(reader)? {
                Some(s) => s,
                None => {
                    return Err(BildFehler::JpegDatenFalsch {
                        grund: "AC-Verfeinerung: Symbol fehlt",
                    });
                }
            };
            let mut r = (sym >> 4) as i32;
            let mut s = (sym & 0x0F) as i32;
            if s != 0 {
                // libjpeg warnt nur, wenn s != 1, und macht weiter. Wir
                // warnen nicht (das gibt es hier nicht), setzen aber
                // ebenfalls nur ein Vorzeichenbit — eine Groesse > 1
                // gibt es in einer Verfeinerung nicht.
                let bit = reader.bits(1).ok_or(BildFehler::JpegDatenFalsch {
                    grund: "AC-Verfeinerung: Vorzeichen fehlt",
                })?;
                s = if bit == 1 { p1 } else { m1 };
            } else if r != 15 {
                // EOB: der Rest des Bandes und `eobrun` weitere Bloecke
                // bekommen keine neuen Stellen, aber sehr wohl
                // Korrekturbits.
                let mut e = 1u32 << r;
                if r > 0 {
                    let extra = reader.bits(r as u8).ok_or(BildFehler::JpegDatenFalsch {
                        grund: "AC-Verfeinerung: EOBRUN-Bits fehlen",
                    })?;
                    e += extra as u32;
                }
                // WICHTIG: hier wird NICHT heruntergezaehlt. libjpeg
                // setzt EOBRUN, bricht ab, und der abschliessende
                // `if (EOBRUN > 0)`-Block erledigt den Rest des Bandes UND
                // zieht einmal ab. Ein zweites Abziehen (so wie zuerst
                // hier) zaehlt einen Block zu viel leer und der Strom
                // laeuft ab dem Folgeloecke auseinander.
                *eobrun = e;
                break;
            }
            // ZRL: r = 15, s = 0. Die Schleife unten laeuft fuer ein ZRL
            // mit und liest die Korrekturbits der 16 uebersprungenen
            // Stellen mit.

            // Ueber bereits von-null-verschiedene und `r` noch-null
            // Koeffizienten hinweggehen und die Korrekturbits anhaengen.
            loop {
                if k > grenze {
                    break;
                }
                if block[ZIGZAG[k]] != 0 {
                    verfeinere(block, reader, k, p1, m1)?;
                } else {
                    r -= 1;
                    if r < 0 {
                        break; // Ziel-Stelle erreicht
                    }
                }
                k += 1;
            }
            if s != 0 {
                if k > grenze {
                    return Err(BildFehler::JpegDatenFalsch {
                        grund: "AC-Verfeinerung: neue Stelle hinter dem Band",
                    });
                }
                block[ZIGZAG[k]] = s.clamp(-32767, 32767) as i16;
            }
            k += 1; // die `for`-Schleife von libjpeg zaehlt hier hoch
        }
    }

    if *eobrun > 0 {
        // Der Rest des Bandes nach dem EOB.
        while k <= grenze {
            verfeinere(block, reader, k, p1, m1)?;
            k += 1;
        }
        *eobrun -= 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// EXIF-Orientierung
// ---------------------------------------------------------------------------

/// Liest die EXIF-Orientierung (Tag 0x0112) aus einem JPEG.
///
/// „Lesen" heisst hier bewusst: die Funktion gibt die Zahl zurueck und
/// das Bild wird NICHT gedreht. Wer das Bild drehen will, tut das nach
/// dem Dekodieren selbst — eine stille Drehung waere eine
/// Groessen-Aenderung, die niemand bestellt hat.
pub fn jpeg_orientierung(daten: &[u8]) -> Option<u8> {
    if daten.len() < 4 || daten[0] != 0xFF || daten[1] != marker::SOI {
        return None;
    }
    let mut pos = 2usize;
    while pos + 4 <= daten.len() {
        if daten[pos] != 0xFF {
            return None;
        }
        let mut m = pos + 1;
        while m < daten.len() && daten[m] == 0xFF {
            m += 1;
        }
        if m >= daten.len() {
            return None;
        }
        let mk = daten[m];
        if mk == marker::EOI || mk == marker::SOS {
            return None; // APP1 muss vor dem Scan stehen
        }
        if mk == 0x01 || (0xD0..=0xD7).contains(&mk) {
            pos = m + 1;
            continue;
        }
        if m + 3 > daten.len() {
            return None;
        }
        let laenge = u16::from_be_bytes([daten[m + 1], daten[m + 2]]) as usize;
        if laenge < 2 || m + 1 + laenge > daten.len() {
            return None;
        }
        if mk == marker::APP1 {
            if let Some(o) = lese_exif_orientation(&daten[m + 3..m + 1 + laenge]) {
                return Some(o);
            }
        }
        pos = m + 1 + laenge;
    }
    None
}

/// Liest die Orientation aus dem NUTZLAST-Teil eines APP1-Segments
/// (also ohne das Laengenfeld): "Exif\0\0" plus TIFF-Struktur.
fn lese_exif_orientation(daten: &[u8]) -> Option<u8> {
    if daten.len() < 14 || &daten[..6] != b"Exif\0\0" {
        return None;
    }
    let tiff = 6usize;
    let little = &daten[tiff..tiff + 2] == b"II";
    let u16_an = |a: usize| -> u16 {
        if little {
            u16::from_le_bytes([daten[a], daten[a + 1]])
        } else {
            u16::from_be_bytes([daten[a], daten[a + 1]])
        }
    };
    let u32_an = |a: usize| -> u32 {
        if little {
            u32::from_le_bytes([daten[a], daten[a + 1], daten[a + 2], daten[a + 3]])
        } else {
            u32::from_be_bytes([daten[a], daten[a + 1], daten[a + 2], daten[a + 3]])
        }
    };
    if u16_an(tiff + 2) != 42 {
        return None; // TIFF-Magic
    }
    // IFD-Offset ist RELATIV zum TIFF-Kopf, nicht zur Datei.
    let ifd = tiff + u32_an(tiff + 4) as usize;
    if ifd + 2 > daten.len() {
        return None;
    }
    let anzahl = u16_an(ifd);
    let mut p = ifd + 2;
    for _ in 0..anzahl {
        if p + 12 > daten.len() {
            return None;
        }
        let tag = u16_an(p);
        if tag == 0x0112 {
            // Orientation ist SHORT: der Wert steht in den ersten
            // zwei Byte des Wertfeldes, in Datei-Reihenfolge.
            return Some(u16_an(p + 8) as u8);
        }
        p += 12;
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Handkodiertes 8x8-Baseline-JPEG, drei Komponenten, 4:4:4,
    /// Quantisierungstabelle ueberall 1, nur DC-Koeffizienten.
    ///
    /// Erwartete Farbe: Y=200, Cb=100, Cr=150 -> nach der Umrechnung
    /// (231, 194, 150), weil eine konstante Flaeche exakt ueber den
    /// DC-Koeffizienten 8*(wert-128) kommt.
    fn mini_farbe() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&[0xFF, 0xD8]); // SOI
        // DQT: Tabelle 0, alle Werte 1
        v.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        v.extend(std::iter::repeat(1u8).take(64));
        // SOF0: 8x8, 3 Komponenten, alle 1:1, Tq=0
        v.extend_from_slice(&[
            0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x08, 0x00, 0x08, 0x03, 0x01, 0x11, 0x00, 0x02,
            0x11, 0x00, 0x03, 0x11, 0x00,
        ]);
        // DHT: DC-Tabelle 0, Symbole 7,8,9,10 an Laenge 2
        v.extend_from_slice(&[
            0xFF, 0xC4, 0x00, 0x17, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x08, 0x09, 0x0A,
        ]);
        // DHT: AC-Tabelle 0, nur EOB (0x00) an Laenge 1
        v.extend_from_slice(&[
            0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]);
        // SOS: 3 Komponenten, Band 0..63
        v.extend_from_slice(&[
            0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x00, 0x3F, 0x00,
        ]);
        // Entropie: Y(DC 576, Kategorie 10) Cb(DC -224, Kat 8) Cr(DC 176, Kat 8)
        v.extend_from_slice(&[0xE4, 0x02, 0x3E, 0x6C, 0x00]);
        v.extend_from_slice(&[0xFF, 0xD9]); // EOI
        v
    }

    /// Dasselbe Bild, aber 16x8 (zwei MCUs) mit DRI=1 und einem
    /// Restart-Marker dazwischen. Zweite Haelfte: Y=80, Cb=140,
    /// Cr=120 -> (69, 82, 101).
    fn mini_restart() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&[0xFF, 0xD8]);
        v.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        v.extend(std::iter::repeat(1u8).take(64));
        v.extend_from_slice(&[
            0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x08, 0x00, 0x10, 0x03, 0x01, 0x11, 0x00, 0x02,
            0x11, 0x00, 0x03, 0x11, 0x00,
        ]);
        v.extend_from_slice(&[
            0xFF, 0xC4, 0x00, 0x17, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x08, 0x09, 0x0A,
        ]);
        v.extend_from_slice(&[
            0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]);
        // DRI: Restartintervall 1 MCU
        v.extend_from_slice(&[0xFF, 0xDD, 0x00, 0x04, 0x00, 0x01]);
        v.extend_from_slice(&[
            0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x00, 0x3F, 0x00,
        ]);
        v.extend_from_slice(&[0xE4, 0x02, 0x3E, 0x6C, 0x00]); // MCU 0
        v.extend_from_slice(&[0xFF, 0xD0]); // Restart-Marker RST0
        v.extend_from_slice(&[0x8F, 0xE3, 0x00, 0x7E]); // MCU 1
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }

    fn nahe(fund: [u8; 4], erwartet: [u8; 3], tol: u8) {
        assert_eq!(fund[3], 255, "Alpha muss 255 sein");
        for i in 0..3 {
            let d = (fund[i] as i32 - erwartet[i] as i32).abs();
            assert!(
                d <= tol as i32,
                "Kanal {i}: {} statt {} (+-{})",
                fund[i],
                erwartet[i],
                tol
            );
        }
    }

    #[test]
    fn jpeg_8x8_gibt_die_farben() {
        let d = mini_farbe();
        let bild = jpeg_lesen(&d).expect("muss lesen");
        assert_eq!((bild.breite, bild.hoehe), (8, 8));
        for y in 0..8 {
            for x in 0..8 {
                nahe(bild.pixel(x, y), [231, 194, 150], 2);
            }
        }
    }

    #[test]
    fn jpeg_restart_setzt_den_prediktor_zurueck() {
        let d = mini_restart();
        let bild = jpeg_lesen(&d).expect("muss lesen");
        assert_eq!((bild.breite, bild.hoehe), (16, 8));
        // Ohne den Reset waeren die Koeffizienten der zweiten Haelfte
        // die Differenz zur ersten — es muesste also daneben liegen.
        nahe(bild.pixel(0, 0), [231, 194, 150], 2);
        nahe(bild.pixel(7, 7), [231, 194, 150], 2);
        nahe(bild.pixel(8, 0), [69, 82, 101], 2);
        nahe(bild.pixel(15, 7), [69, 82, 101], 2);
    }

    #[test]
    fn jpeg_ohne_soi_ist_falsch() {
        let d = vec![0x00, 0x01, 0x02, 0x03];
        let e = jpeg_lesen(&d).expect_err("darf nicht lesen");
        assert_eq!(e.name(), "JpegKopfFalsch");
    }

    #[test]
    fn jpeg_zu_kurz_ist_falsch() {
        let d = vec![0xFF];
        let e = jpeg_lesen(&d).expect_err("darf nicht lesen");
        assert_eq!(e.name(), "ZuKurz");
    }

    #[test]
    fn jpeg_grosse_kopfzahlen_ist_falsch() {
        // 65535x65535 im SOF: erst die Grenze, dann die Allokation.
        let mut d = mini_farbe();
        // SOF-Hoehe/Breite in der Datei finden und ueberschreiben.
        let p = d
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("SOF drin");
        d[p + 5] = 0xFF;
        d[p + 6] = 0xFF; // Hoehe 65535
        d[p + 7] = 0xFF;
        d[p + 8] = 0xFF; // Breite 65535
        let e = jpeg_lesen(&d).expect_err("darf nicht lesen");
        assert_eq!(e.name(), "ZuGross");
    }

    #[test]
    fn strecke_macht_vorzeichen_richtig() {
        assert_eq!(strecke(0, 1), -1);
        assert_eq!(strecke(1, 1), 1);
        assert_eq!(strecke(0, 2), -3);
        assert_eq!(strecke(1, 2), -2);
        assert_eq!(strecke(2, 2), 2);
        assert_eq!(strecke(3, 2), 3);
        assert_eq!(strecke(31, 8), -224);
        assert_eq!(strecke(176, 8), 176);
        assert_eq!(strecke(127, 9), -384);
        // Kategorie 0 oder Unsinn: kein Shift-Umbruch, sondern 0.
        assert_eq!(strecke(0, 0), 0);
        assert_eq!(strecke(99, 99), 0);
    }

    #[test]
    fn idct_dc_ergibt_konstante() {
        let mut block = [0i16; KOEFFIZIENTEN_PRO_BLOCK];
        block[0] = 576; // 8*(200-128)
        let q = QuantTabelle::new();
        idct(&mut block, &q);
        for (i, w) in block.iter().enumerate() {
            assert_eq!(*w, 200, "Stelle {i}");
        }

        let mut block = [0i16; KOEFFIZIENTEN_PRO_BLOCK];
        block[0] = -384; // 8*(80-128)
        idct(&mut block, &q);
        assert_eq!(block[63], 80);
    }

    #[test]
    fn huffman_ueberbesetzter_codebaum_ist_falsch() {
        // Zwei 1-Bit- und zwei 2-Bit-Codes: Kraft = 1.5 > 1.
        let bytes = [
            0x00, // Tc=0, Th=0
            0x02, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, // 16 Anzahlen
            0x00, 0x01, 0x02, 0x03, // Symbole
        ];
        let mut pos = 0usize;
        let e = HuffmanTabelle::lesen(&bytes, &mut pos).expect_err("zu viel");
        assert_eq!(e.name(), "JpegHuffmanFalsch");
    }

    #[test]
    fn huffman_leere_tabelle_ist_falsch() {
        let mut bytes = vec![0x00];
        bytes.extend(std::iter::repeat(0u8).take(16));
        let mut pos = 0usize;
        let e = HuffmanTabelle::lesen(&bytes, &mut pos).expect_err("leer");
        assert_eq!(e.name(), "JpegHuffmanFalsch");
    }

    #[test]
    fn exif_orientierung_wird_gelesen() {
        // APP1-Nutzlast: Exif + TIFF (II, Magic 42) + ein Eintrag 0x0112.
        let mut p = Vec::new();
        p.extend_from_slice(b"Exif\0\0");
        p.extend_from_slice(b"II");
        p.extend_from_slice(&42u16.to_le_bytes());
        p.extend_from_slice(&8u32.to_le_bytes()); // IFD-Offset ab TIFF-Kopf
        p.extend_from_slice(&1u16.to_le_bytes()); // ein Eintrag
        p.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
        p.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        p.extend_from_slice(&1u32.to_le_bytes()); // ein Wert
        p.extend_from_slice(&6u16.to_le_bytes()); // Orientation = 6
        p.extend_from_slice(&0u16.to_le_bytes()); // Rest des Wertfeldes
        p.extend_from_slice(&0u32.to_le_bytes()); // kein weiteres IFD
        assert_eq!(lese_exif_orientation(&p), Some(6));

        // Grossbuchstabig (MM) geht auch.
        let mut q = Vec::new();
        q.extend_from_slice(b"Exif\0\0");
        q.extend_from_slice(b"MM");
        q.extend_from_slice(&42u16.to_be_bytes());
        q.extend_from_slice(&8u32.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q.extend_from_slice(&0x0112u16.to_be_bytes());
        q.extend_from_slice(&3u16.to_be_bytes());
        q.extend_from_slice(&1u32.to_be_bytes());
        q.extend_from_slice(&3u16.to_be_bytes());
        q.extend_from_slice(&0u16.to_be_bytes());
        q.extend_from_slice(&0u32.to_be_bytes());
        assert_eq!(lese_exif_orientation(&q), Some(3));

        // Kaputtes EXIF gibt None, keinen Panic.
        assert_eq!(lese_exif_orientation(b"Exif\0\0II"), None);
        assert_eq!(lese_exif_orientation(b"keins"), None);
    }

    #[test]
    fn scan_daten_entstuft_und_notiert_restarts() {
        // 0xFF00 wird zu 0xFF, ein Restart-Marker wird zur Naht.
        let roh = [0x11, 0xFF, 0x00, 0x22, 0xFF, 0xD3, 0x33, 0xFF, 0xD9, 0x44];
        let (bytes, restarts, ende) = lese_scan_daten(&roh, 0);
        assert_eq!(bytes, vec![0x11, 0xFF, 0x22, 0x33]);
        assert_eq!(restarts, vec![3]);
        assert_eq!(ende, 7); // steht auf dem 0xFF des EOI
    }

    #[test]
    fn progressiver_dc_scan_muss_band_null_haben() {
        // SOF2 mit einem AC-artigen Scan: das darf Baseline heissen,
        // progressiv mit Ss=0/Se!=0 ist aber kaputt.
        let mut d = Vec::new();
        d.extend_from_slice(&[0xFF, 0xD8]);
        d.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        d.extend(std::iter::repeat(1u8).take(64));
        // SOF2 (progressiv), 8x8, 1 Komponente
        d.extend_from_slice(&[
            0xFF, 0xC2, 0x00, 0x0B, 0x08, 0x00, 0x08, 0x00, 0x08, 0x01, 0x01, 0x11, 0x00,
        ]);
        d.extend_from_slice(&[
            0xFF, 0xC4, 0x00, 0x17, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x08, 0x09, 0x0A,
        ]);
        // SOS mit Ss=0, Se=63 -> im progressiven Frame ungueltig
        d.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
        d.extend_from_slice(&[0x00, 0xFF, 0xD9]);
        let e = jpeg_lesen(&d).expect_err("darf nicht lesen");
        assert_eq!(e.name(), "JpegKopfFalsch");
    }

    /// GEMESSEN 02.10.2026: die Cosinus-Tabelle wird bitweise gegen die
    /// Formel geprueft, aus der sie stammt.
    ///
    /// Anlass: clippy meldete 16x "float has excessive precision" — die
    /// Literale hatten mehr Nachkommastellen, als `f32` fassen kann.
    /// RUNDEN AUF DIE KURZESTE f32-FORM KANN DAS ERGEBNIS NICHT AENDERN,
    /// wenn die f32-Bits gleich bleiben. Genau das behauptet dieser Test,
    /// und er behauptet es fuer alle 64 Eintraege statt fuer zwei Zahlen.
    ///
    /// Er haelt auch fest, dass die Tabelle ueberhaupt die Formel traegt:
    /// ein Tippfehler in einer Zelle faellt hier auf und nicht erst als
    /// falsches Bild.
    #[test]
    fn die_cosinus_tabelle_stimmt_bitweise_mit_ihrer_formel_ueberein() {
        const PI: f64 = std::f64::consts::PI;
        for x in 0..8usize {
            for u in 0..8usize {
                let soll = ((2 * x + 1) as f64 * u as f64 * PI / 16.0).cos();
                // `cos` in f64, dann der naechste f32-Wert.
                let erwartet = soll as f32;
                assert_eq!(
                    COS_TABLE[x][u].to_bits(),
                    erwartet.to_bits(),
                    "COS_TABLE[{x}][{u}] = {} ist nicht cos((2*{x}+1)*{u}*pi/16) = {soll}",
                    COS_TABLE[x][u]
                );
            }
        }
    }
}

/// CMYK nach RGB.
///
/// GEMESSEN 02.10.2026 (Rolle BILD, B2), mit
/// `examples/jpeg_cmyk_messung.rs` gegen
/// `scratch/jpeg_tests/test_base.jpg` und `test_cmyk.jpg`:
///
/// ```text
/// vorher: groesste Abweichung 148 bei (30, 44), mittlere Abweichung
///         3.57 je Kanal, 2583 von 12288 Kanalwerten ueber 3
/// ```
///
/// Grund: hier stand `r = c * k / 255`. Das ist kein Tintenmodell — es
/// multipliziert zwei Werte, die gar nicht miteinander verrechnet werden
/// duerfen, und es ignores die Invertierung der Kanaele. Bei
/// `test_cmyk.jpg` ist K ueberall 0, das Ergebnis war also `0` fuer R, G
/// und B statt `255 - C`.
///
/// Die Regel, die die Datei trifft: im CMYK-Signal bedeutet 255 volle
/// Tinte (ausser der Adobe-Transformwert 2, dann ist es umgekehrt). Auf
/// einen Farbkanal wirkt die Summe aus eigener Tinte und Schwarz, und
/// 255 minus dieser Summe ist der Kanalwert:
///
/// ```text
/// R = 255 - min(255, C + K)
/// G = 255 - min(255, M + K)
/// B = 255 - min(255, Y + K)
/// ```
///
/// GEMESSEN an der Testdatei, die PIL erzeugt hat
/// (`scratch/gen_jpeg_tests.py`, `img.convert("CMYK")`):
///
/// ```text
/// RGB (19, 16, 107)   ->  CMYK (236, 239, 148, 0)
/// RGB (255, 0, 0)     ->  CMYK (0, 255, 255, 0)
/// RGB (255, 255, 255) ->  CMYK (0, 0, 0, 0)
/// ```
///
/// wonach `255 - C = 19`, `255 - M = 16`, `255 - Y = 107` gilt. Genau
/// das ist zugleich die uebliche Formel fuer CMYK ohne ICC-Profil (sie
/// steht so bei ImageMagick in `colorspace.c` und in den meisten
/// CMYK-Decodern); ein Profil waere genauer, ist aber hier nicht
/// vorhanden und waere auch nicht pruefbar.
///
/// `invertiert` steht fuer den Adobe-Transformwert 2: dann sind die
/// Werte im Bild invertiert gespeichert (255 = keine Tinte) und werden
/// vor der Rechnung umgedreht.
fn cmyk_bild(frame: &Frame, invertiert: bool) -> Result<Bild, BildFehler> {
    if frame.komponenten.len() < 4 {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "CMYK braucht vier Komponenten",
        });
    }
    let mut bild = Bild::new(frame.ziel_breite, frame.ziel_hoehe);
    let kc = &frame.komponenten[0];
    let km = &frame.komponenten[1];
    let ky = &frame.komponenten[2];
    let kk = &frame.komponenten[3];

    for y in 0..frame.ziel_hoehe {
        for x in 0..frame.ziel_breite {
            let hol = |k: &Komponente| -> u32 {
                let w = k.musterpixel(x, y) as u32;
                if invertiert { 255 - w } else { w }
            };
            let c = hol(kc);
            let m = hol(km);
            let yy = hol(ky);
            let k = hol(kk);
            let r = 255 - (c + k).min(255);
            let g = 255 - (m + k).min(255);
            let b = 255 - (yy + k).min(255);
            bild.setze_pixel(x, y, [r as u8, g as u8, b as u8, 255]);
        }
    }
    Ok(bild)
}

/// YCCK nach RGB: erst YCbCr, dann die Tinte des Schwarzkanals.
///
/// `invertiert` bedeutet: der Adobe-Transformwert ist 2, die Werte im
/// Bild sind also invertiert gespeichert (255 = keine Tinte) und werden
/// vor der Rechnung umgedreht. Siehe `cmyk_bild` fuer die Messung, die
/// die Regel fuer CMYK belegt.
fn ycck_bild(frame: &Frame, invertiert: bool) -> Result<Bild, BildFehler> {
    if frame.komponenten.len() < 4 {
        return Err(BildFehler::JpegKopfFalsch {
            grund: "YCCK braucht vier Komponenten",
        });
    }
    let mut bild = Bild::new(frame.ziel_breite, frame.ziel_hoehe);
    let ky = &frame.komponenten[0];
    let kcb = &frame.komponenten[1];
    let kcr = &frame.komponenten[2];
    let kk = &frame.komponenten[3];

    for y in 0..frame.ziel_hoehe {
        for x in 0..frame.ziel_breite {
            let dreh = |w: u8| -> u8 { if invertiert { 255 - w } else { w } };
            let yy = dreh(ky.musterpixel(x, y));
            let cb = dreh(kcb.musterpixel(x, y));
            let cr = dreh(kcr.musterpixel(x, y));
            let k = dreh(kk.musterpixel(x, y)) as u32;

            let [r, g, b] = ycbcr_zu_rgb(yy, cb, cr);
            // 255 = keine Tinte: der Schwarzkanal dunkelt ab, er
            // multipliziert nicht (siehe `cmyk_bild`).
            let rf = ((r as u32 + k).min(255)) as u8;
            let gf = ((g as u32 + k).min(255)) as u8;
            let bf = ((b as u32 + k).min(255)) as u8;

            bild.setze_pixel(x, y, [rf as u8, gf as u8, bf as u8, 255]);
        }
    }
    Ok(bild)
}
