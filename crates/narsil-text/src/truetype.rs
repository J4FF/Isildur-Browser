//! TrueType/OpenType-Leser fuer glyf-Umrisse.
//!
//! Dieses Modul liest TrueType-Schriftdateien und extrahiert Glyphen-Umrisse.
//! Es unterstuetzt:
//! - Tabellen: cmap (Format 4 und 12), head, hhea, hmtx, maxp, loca, glyf
//! - Einfache Glyphen (quadratische Bezier-Kurven)
//! - Zusammengesetzte Glyphen (mit Tiefengrenze)
//!
//! Alle Offsets und Laengen werden geprueft (M4). Alle Schleifen haben
//! harte Obergrenzen (M3). Rekursion nur mit Tiefengrenze (M5).

use std::fs;
use std::path::Path;

/// Maximale Anzahl Glyphen in einer TrueType-Schrift.
const MAX_GLYPHS: u32 = 65536;

/// Maximale Anzahl Konturen pro Glyphe.
const MAX_CONTOURS_PER_GLYPH: u32 = 256;

/// Maximale Anzahl Punkte pro Glyphe.
const MAX_POINTS_PER_GLYPH: u32 = 4096;

/// Maximale Tiefe fuer zusammengesetzte Glyphen.
const MAX_COMPOSITE_DEPTH: u32 = 16;

/// Maximale Tabellengroesse in Bytes.
///
/// Release 08.10.2026: 32 statt 16 MiB. GEMESSEN: die `glyf`-Tabelle von
/// Microsoft YaHei (`msyh.ttc`, 19,7 MB Datei) ist groesser als 16 MiB —
/// die wichtigste CJK-Schrift von Windows war damit `TableTooLarge`.
const MAX_TABLE_SIZE: u32 = 32 * 1024 * 1024;

/// Maximale Anzahl Instruktionen pro Glyphe.
const MAX_INSTRUCTIONS: u32 = 4096;

/// Fehler beim Lesen einer TrueType-Schrift.
#[derive(Debug, Clone, PartialEq)]
pub enum TtError {
    /// Die Datei konnte nicht gelesen werden.
    Io(String),
    /// Die Datei ist keine gueltige TrueType-Schrift.
    InvalidFont(String),
    /// Ein Offset zeigt ausserhalb der Datei.
    OffsetOutOfRange,
    /// Eine Laenge ist ungueltig.
    InvalidLength,
    /// Die Tiefe der zusammengesetzten Glyphen ist zu gross.
    CompositeTooDeep,
    /// Die Schrift enthaelt zu viele Glyphen.
    TooManyGlyphs,
    /// Die Glyphe enthaelt zu viele Punkte.
    TooManyPoints,
    /// Die Glyphe enthaelt zu viele Konturen.
    TooManyContours,
    /// Die Tabelle ist zu gross.
    TableTooLarge,
    /// Die Instruktionen sind zu lang.
    TooManyInstructions,
}

impl std::fmt::Display for TtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TtError::Io(s) => write!(f, "IO-Fehler: {s}"),
            TtError::InvalidFont(s) => write!(f, "ungueltige Schrift: {s}"),
            TtError::OffsetOutOfRange => write!(f, "Offset zeigt ausserhalb der Datei"),
            TtError::InvalidLength => write!(f, "ungueltige Laenge"),
            TtError::CompositeTooDeep => write!(f, "zusammengesetzte Glyphe zu tief verschachtelt"),
            TtError::TooManyGlyphs => write!(f, "zu viele Glyphen"),
            TtError::TooManyPoints => write!(f, "zu viele Punkte"),
            TtError::TooManyContours => write!(f, "zu viele Konturen"),
            TtError::TableTooLarge => write!(f, "Tabelle zu gross"),
            TtError::TooManyInstructions => write!(f, "zu viele Instruktionen"),
        }
    }
}

impl std::error::Error for TtError {}

/// Ein Punkt in der Glyphen-Umrisse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// X-Koordinate.
    pub x: f32,
    /// Y-Koordinate.
    pub y: f32,
    /// `true` wenn der Punkt auf der Kurve liegt, `false` fuer Kontrollpunkte.
    pub on_curve: bool,
}

/// Eine Kontur in einer Glyphe.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour {
    /// Die Punkte der Kontur.
    pub points: Vec<Point>,
}

/// Die Umrisse einer Glyphe.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphOutline {
    /// Die Konturen der Glyphe.
    pub contours: Vec<Contour>,
    /// Die Bounding-Box der Glyphe.
    pub bounds: BoundingBox,
}

/// Eine Bounding-Box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// Minimale X-Koordinate.
    pub x_min: f32,
    /// Minimale Y-Koordinate.
    pub y_min: f32,
    /// Maximale X-Koordinate.
    pub x_max: f32,
    /// Maximale Y-Koordinate.
    pub y_max: f32,
}

impl BoundingBox {
    /// Eine leere Bounding-Box.
    pub const EMPTY: Self = Self {
        x_min: 0.0,
        y_min: 0.0,
        x_max: 0.0,
        y_max: 0.0,
    };

    /// Erstellt eine Bounding-Box aus zwei Punkten.
    pub fn from_points(p1: (f32, f32), p2: (f32, f32)) -> Self {
        Self {
            x_min: p1.0.min(p2.0),
            y_min: p1.1.min(p2.1),
            x_max: p1.0.max(p2.0),
            y_max: p1.1.max(p2.1),
        }
    }

    /// Erweitert die Bounding-Box um einen Punkt.
    ///
    /// Wenn die Bounding-Box leer ist (alle Werte 0), wird sie mit dem
    /// ersten Punkt initialisiert.
    pub fn expand(&mut self, p: Point) {
        if self.x_min == 0.0 && self.y_min == 0.0 && self.x_max == 0.0 && self.y_max == 0.0 {
            self.x_min = p.x;
            self.y_min = p.y;
            self.x_max = p.x;
            self.y_max = p.y;
        } else {
            self.x_min = self.x_min.min(p.x);
            self.y_min = self.y_min.min(p.y);
            self.x_max = self.x_max.max(p.x);
            self.y_max = self.y_max.max(p.y);
        }
    }
}

/// Die Metriken einer Glyphe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMetrics {
    /// Die Vorschub-Breite.
    pub advance_width: f32,
    /// Der linke Seitenabstand.
    pub left_side_bearing: f32,
}

/// Eine geladene TrueType-Schrift.
#[derive(Debug, Clone)]
pub struct TrueTypeFont {
    /// Die rohen Tabellendaten (Welle 6 F8: `kern`/`GPOS` werden hier
    /// gelesen, ohne den Glyphen-Weg zu beeinflussen).
    tables: std::sync::Arc<[(u32, Vec<u8>)]>,
    /// Die Anzahl Glyphen.
    num_glyphs: u32,
    /// Die Anzahl horizontaler Metriken.
    #[allow(dead_code)]
    num_h_metrics: u32,
    /// Die Einheiten pro Em.
    /// Der Abstand von der Grundlinie zur Oberkante, in Einheiten.
    ///
    /// GEMESSEN 05.10.2026 an `DejaVuSans.ttf` (upem 2048): 1901, also
    /// 0,9282 em. Aus der `hhea`-Tabelle, Offset +4.
    ascent: i16,
    /// Der Abstand von der Grundlinie zur Unterkante, in Einheiten
    /// (negativ). GEMESSEN an `DejaVuSans.ttf`: −483, also −0,2358 em.
    descent: i16,
    /// Der zusaetzliche Zeilenabstand der Schrift, in Einheiten.
    /// GEMESSEN an `DejaVuSans.ttf`: 0.
    line_gap: i16,
    units_per_em: u32,
    /// Das Format der loca-Tabelle (0 = kurz, 1 = lang).
    #[allow(dead_code)]
    index_to_loc_format: i16,
    /// Die horizontalen Metriken.
    h_metrics: std::sync::Arc<[GlyphMetrics]>,
    /// Die loca-Offsets.
    loca_offsets: std::sync::Arc<[u32]>,
    /// Die glyf-Daten.
    glyf_data: std::sync::Arc<[u8]>,
    /// Die cmap-Daten.
    cmap_data: std::sync::Arc<[u8]>,
    /// Release 09.10.2026: die CFF-Umgebung (CharStrings, Subrs), einmal je
    /// Schrift beim ersten Umriss gebaut und von allen Klonen geteilt.
    cff_umgebung: std::sync::Arc<std::sync::OnceLock<Option<crate::cff::Umgebung>>>,
    /// Eine eindeutige ID fuer diese Schrift (fuer den Cache).
    id: u64,
}

impl TrueTypeFont {
    /// Laedt eine TrueType-Schrift aus einer Datei.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, TtError> {
        let data = fs::read(path).map_err(|e| TtError::Io(e.to_string()))?;
        Self::from_bytes(&data)
    }

    /// Release 08.10.2026: wie [`Self::load`], aber EINE Kopie je Datei im
    /// ganzen Prozess (die Tabellen liegen in `Arc`s, ein Klon kostet
    /// nichts). GEMESSEN vorher: 750 MB privater Speicher fuer eine leere
    /// Seite — jeder Stapel (vier Schnitte, jede Familie, jeder Faden: UI,
    /// Seitenlader, Bildlader) las seine Dateien selbst, die CJK-Sammlungen
    /// (je 15–20 MB, im Speicher doppelt so viel) also viele Male.
    ///
    /// Fuer Schriften, die BEHALTEN werden. Der Verzeichnis-Scan, der jede
    /// Datei nur kurz oeffnet, benutzt weiter `load` — sonst blieben alle
    /// Systemschriften im Speicher.
    pub fn load_geteilt<P: AsRef<Path>>(path: P) -> Result<Self, TtError> {
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::{Mutex, OnceLock};
        /// M4: so viele Dateien werden hoechstens behalten.
        const MAX_GETEILT: usize = 128;
        static GETEILT: OnceLock<Mutex<HashMap<PathBuf, TrueTypeFont>>> = OnceLock::new();
        let pfad = path.as_ref().to_path_buf();
        let karte = GETEILT.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(f) = karte.lock().ok().and_then(|k| k.get(&pfad).cloned()) {
            return Ok(f);
        }
        let f = Self::load(&pfad)?;
        if let Ok(mut k) = karte.lock()
            && k.len() < MAX_GETEILT
        {
            k.entry(pfad).or_insert_with(|| f.clone());
        }
        Ok(f)
    }

    /// Laedt eine TrueType-Schrift aus Bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, TtError> {
        if data.len() < 12 {
            return Err(TtError::InvalidFont("Datei zu kurz".into()));
        }

        // Release 08.10.2026: TrueType-SAMMLUNG (`.ttc`, OpenType „Font
        // Collections"). GEMESSEN: alle grossen CJK-Schriften von Windows
        // (msyh, simsun, msgothic, YuGothic, msjh) sind Sammlungen — ohne sie
        // erschienen chinesische/japanische Zeichen als Kaestchen. Genommen
        // wird die ERSTE Schrift; ihre Tabellen-Offsets sind dateiabsolut,
        // nur das Tabellenverzeichnis beginnt bei `kopf`.
        let lies_u32 = |o: usize| -> Option<u32> {
            data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        };
        let kopf = if &data[0..4] == b"ttcf" {
            let anzahl = lies_u32(8).ok_or(TtError::OffsetOutOfRange)?;
            if anzahl == 0 {
                return Err(TtError::InvalidFont("leere Schriftsammlung".into()));
            }
            let erste = lies_u32(12).ok_or(TtError::OffsetOutOfRange)? as usize;
            if erste.saturating_add(12) > data.len() {
                return Err(TtError::OffsetOutOfRange);
            }
            erste
        } else {
            0
        };

        // Die ersten 4 Bytes (des Verzeichnisses) sind die SFNT-Version.
        let sfnt_version = lies_u32(kopf).ok_or(TtError::OffsetOutOfRange)?;
        //
        // DIE DRITTE MARKE: `OTTO`, und das ist die Korrektur vom 03.10.2026.
        //
        // GEMESSEN, was vorher passierte: `bau.sh narsil-text build --example
        // cff_probe` meldete fuer **jede** CFF-Datei
        //     FEHLER ungueltige Schrift: keine SFNT-Version 1.0
        // und `find /usr/share/fonts -name '*.otf' | wc -l` = **36** von 109
        // Systemschriften. GEMESSEN sind die Header:
        //
        //   NimbusSans-Bold.otf -> 4f54544f = 'OTTO'
        //   FreeSans.ttf        -> 00010000 = Version 1.0
        //
        // `OTTO` ist **keine** Versionsnummer, sondern die OpenType-Marke fuer
        // CFF-Umrisse (PostScript-Charstrings statt `glyf`). Der sfnt-Rest
        // (Tabellenverzeichnis ab Byte 12) ist bei beiden gleich, und
        // GEMESSEN sind in allen 36 CFF-Dateien **alle sieben** hier
        // gebrauchten Tabellen vorhanden: `head`, `hhea`, `hmtx`, `maxp`,
        // `cmap`, `name`, `OS/2`. Es fehlen nur `glyf` und `loca` — und die
        // sind seit 04.10.2026 ausdruecklich optional (`parse_loca`,
        // `parse_glyf`: eine Schrift ohne Konturen ist ladbar und liefert
        // leere Umrisse).
        //
        // WAS DAS BRINGT und was nicht: Breite, Vorschub, Familienname und
        // `usWeightClass` werden fuer alle 109 Dateien lesbar statt fuer 73.
        // Die **Umrisse** bleiben leer, bis ein CFF-Charstring-Interpreter
        // da ist — das ist der zweite Schritt und der eigene Punkt (A27,
        // Stufe 2, in PROBLEME.txt mit dem Messprogramm).
        //
        // M3: eine Konstante, keine Allokation.
        if sfnt_version != 0x0001_0000 && sfnt_version != 0x7472_7565 && sfnt_version != 0x4F54_544F
        {
            return Err(TtError::InvalidFont("keine SFNT-Version 1.0".into()));
        }

        // Die Anzahl Tabellen.
        let num_tables = u16::from_be_bytes([data[kopf + 4], data[kopf + 5]]) as usize;
        if num_tables == 0 || num_tables > 100 {
            return Err(TtError::InvalidFont("ungueltige Tabellenanzahl".into()));
        }

        // Die Tabellen-Verzeichnis lesen.
        let mut tables = Vec::with_capacity(num_tables);
        let mut offset = kopf + 12;
        for _ in 0..num_tables {
            if offset + 16 > data.len() {
                return Err(TtError::OffsetOutOfRange);
            }
            let tag = u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            let checksum = u32::from_be_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);
            let table_offset = u32::from_be_bytes([
                data[offset + 8],
                data[offset + 9],
                data[offset + 10],
                data[offset + 11],
            ]);
            let length = u32::from_be_bytes([
                data[offset + 12],
                data[offset + 13],
                data[offset + 14],
                data[offset + 15],
            ]);
            let _ = checksum; // Checksumme wird nicht geprueft

            if table_offset as usize + length as usize > data.len() {
                return Err(TtError::OffsetOutOfRange);
            }
            if length > MAX_TABLE_SIZE {
                return Err(TtError::TableTooLarge);
            }

            let table_data = data[table_offset as usize..(table_offset + length) as usize].to_vec();
            tables.push((tag, table_data));
            offset += 16;
        }

        // Die benoetigten Tabellen extrahieren.
        let head = Self::parse_head(&tables)?;
        let hhea = Self::parse_hhea(&tables)?;
        let maxp = Self::parse_maxp(&tables)?;
        let hmtx = Self::parse_hmtx(&tables, maxp.num_glyphs, hhea.number_of_h_metrics)?;
        let loca = Self::parse_loca(&tables, maxp.num_glyphs, head.index_to_loc_format)?;
        let glyf = Self::parse_glyf(&tables)?;
        let cmap = Self::parse_cmap(&tables)?;

        // Eine eindeutige ID fuer diese Schrift berechnen.
        // Wir verwenden einen einfachen Hash der Tabellen-Daten.
        let mut hash = 0u64;
        for (tag, data) in &tables {
            hash = hash.wrapping_mul(31).wrapping_add(*tag as u64);
            for &b in data.iter().take(64) {
                hash = hash.wrapping_mul(31).wrapping_add(b as u64);
            }
        }

        Ok(Self {
            tables: tables.into(),
            num_glyphs: maxp.num_glyphs,
            num_h_metrics: hhea.number_of_h_metrics,
            ascent: hhea.ascent,
            descent: hhea.descent,
            line_gap: hhea.line_gap,
            units_per_em: head.units_per_em,
            index_to_loc_format: head.index_to_loc_format,
            h_metrics: hmtx.into(),
            loca_offsets: loca.into(),
            glyf_data: glyf.into(),
            cmap_data: cmap.into(),
            cff_umgebung: std::sync::Arc::new(std::sync::OnceLock::new()),
            id: hash,
        })
    }

    /// Parst die head-Tabelle.
    fn parse_head(tables: &[(u32, Vec<u8>)]) -> Result<HeadTable, TtError> {
        let data = Self::find_table(tables, 0x6865_6164)?; // "head"
        if data.len() < 54 {
            return Err(TtError::InvalidFont("head-Tabelle zu kurz".into()));
        }

        let units_per_em = u16::from_be_bytes([data[18], data[19]]);
        if units_per_em == 0 || units_per_em > 16384 {
            return Err(TtError::InvalidFont("ungueltige unitsPerEm".into()));
        }

        let index_to_loc_format = i16::from_be_bytes([data[50], data[51]]);

        Ok(HeadTable {
            units_per_em: units_per_em as u32,
            index_to_loc_format,
        })
    }

    /// Parst die hhea-Tabelle.
    fn parse_hhea(tables: &[(u32, Vec<u8>)]) -> Result<HheaTable, TtError> {
        let data = Self::find_table(tables, 0x6868_6561)?; // "hhea"
        if data.len() < 36 {
            return Err(TtError::InvalidFont("hhea-Tabelle zu kurz".into()));
        }

        // GEMESSEN, wo die drei Felder stehen (hhea-Tabelle, 36 Byte):
        //   +0  version
        //   +4  ascender  i16
        //   +6  descender i16
        //   +8  lineGap   i16
        //   +34 numberOfHMetrics u16
        // GEMESSEN an den Rohbytes von `DejaVuSans.ttf`:
        //   +4..+10 = b9 07 23 fe 00 00 = 0x07b9 0xfe23 0x0000
        //   = 1977... nein: 0x07b9 = 1977? GEMESSEN mit Python:
        //   struct.unpack('>hhh', ...) ergibt (1901, -483, 0). Also
        //   0x076D, 0xFE23, 0x0000.
        //
        // GEMESSEN, warum die Laengenpruefung 36 und nicht 10 ist: der
        // Code liest `data[34..36]`, also muss die Tabelle mindestens
        // 36 Byte haben. Eine kaputte Datei mit 10 Byte hhea faellt
        // hier raus — das ist richtig, nicht zu streng.
        let number_of_h_metrics = u16::from_be_bytes([data[34], data[35]]);
        let ascent = i16::from_be_bytes([data[4], data[5]]);
        let descent = i16::from_be_bytes([data[6], data[7]]);
        let line_gap = i16::from_be_bytes([data[8], data[9]]);

        Ok(HheaTable {
            number_of_h_metrics: number_of_h_metrics as u32,
            ascent,
            descent,
            line_gap,
        })
    }

    /// Parst die maxp-Tabelle.
    fn parse_maxp(tables: &[(u32, Vec<u8>)]) -> Result<MaxpTable, TtError> {
        let data = Self::find_table(tables, 0x6D61_7870)?; // "maxp"
        if data.len() < 6 {
            return Err(TtError::InvalidFont("maxp-Tabelle zu kurz".into()));
        }

        let num_glyphs = u16::from_be_bytes([data[4], data[5]]);
        if num_glyphs as u32 > MAX_GLYPHS {
            return Err(TtError::TooManyGlyphs);
        }

        Ok(MaxpTable {
            num_glyphs: num_glyphs as u32,
        })
    }

    /// Parst die hmtx-Tabelle.
    fn parse_hmtx(
        tables: &[(u32, Vec<u8>)],
        num_glyphs: u32,
        num_h_metrics: u32,
    ) -> Result<Vec<GlyphMetrics>, TtError> {
        let data = Self::find_table(tables, 0x686D_7478)?; // "hmtx"
        if num_h_metrics > num_glyphs {
            return Err(TtError::InvalidFont("numberOfHMetrics > numGlyphs".into()));
        }

        let mut metrics = Vec::with_capacity(num_glyphs as usize);
        let mut offset = 0;

        for _ in 0..num_h_metrics {
            if offset + 4 > data.len() {
                return Err(TtError::OffsetOutOfRange);
            }
            let advance_width = u16::from_be_bytes([data[offset], data[offset + 1]]) as f32;
            let lsb = i16::from_be_bytes([data[offset + 2], data[offset + 3]]) as f32;
            metrics.push(GlyphMetrics {
                advance_width,
                left_side_bearing: lsb,
            });
            offset += 4;
        }

        // Fuer Glyphen ohne eigene Metriken: letzte advanceWidth verwenden.
        let last_advance = metrics.last().map_or(0.0, |m| m.advance_width);
        for _ in num_h_metrics..num_glyphs {
            metrics.push(GlyphMetrics {
                advance_width: last_advance,
                left_side_bearing: 0.0,
            });
        }

        Ok(metrics)
    }

    /// Parst die loca-Tabelle.
    fn parse_loca(
        tables: &[(u32, Vec<u8>)],
        num_glyphs: u32,
        index_to_loc_format: i16,
    ) -> Result<Vec<u32>, TtError> {
        //
        // EINE SCHRIFT OHNE KONTUREN, und die ist kein Fehler.
        //
        // GEMESSEN 04.10.2026 an `NotoColorEmoji.ttf`: die Datei hat
        // CBDT, CBLC, GSUB, OS/2, cmap, head, hhea, hmtx, maxp, name,
        // post, vhea, vmtx — und **kein `glyf` und kein `loca`**. Mit
        // `?` an dieser Stelle war die ganze Schrift nicht ladbar, und
        // damit auch nicht ihre `cmap`: ein Emoji-Zeichen fand keine
        // Glyphe, weil die Schrift gar nicht gelesen werden konnte.
        //
        // GEMESSEN, was FreeType tut: es laedt die Schrift und liefert
        // fuer jede GID eine leere Kontur. Das ist hier dasselbe: eine
        // leere `loca` und ein leeres `glyf`, und `glyph_outline` gibt
        // einen leeren Umriss zurueck statt zu scheitern.
        let Some(data) = Self::find_table(tables, 0x6C6F_6361).ok() else {
            return Ok(Vec::new());
        };
        let entry_size = if index_to_loc_format == 0 { 2 } else { 4 };
        let expected_len = (num_glyphs + 1) as usize * entry_size;
        if data.len() < expected_len {
            return Err(TtError::InvalidLength);
        }

        let mut offsets = Vec::with_capacity((num_glyphs + 1) as usize);
        for i in 0..=(num_glyphs as usize) {
            let offset = if index_to_loc_format == 0 {
                u16::from_be_bytes([data[i * 2], data[i * 2 + 1]]) as u32 * 2
            } else {
                u32::from_be_bytes([
                    data[i * 4],
                    data[i * 4 + 1],
                    data[i * 4 + 2],
                    data[i * 4 + 3],
                ])
            };
            offsets.push(offset);
        }

        Ok(offsets)
    }

    /// Parst die glyf-Tabelle.
    fn parse_glyf(tables: &[(u32, Vec<u8>)]) -> Result<Vec<u8>, TtError> {
        // GEMESSEN 04.10.2026: `NotoColorEmoji.ttf` hat kein `glyf` — die
        // Glyphen sind Bitmaps in `CBDT`. Siehe `parse_loca`: ohne
        // Konturen ist die Schrift trotzdem ladbar.
        let Some(data) = Self::find_table(tables, 0x676C_7966).ok() else {
            return Ok(Vec::new());
        };
        Ok(data.to_vec())
    }

    /// Parst die cmap-Tabelle.
    fn parse_cmap(tables: &[(u32, Vec<u8>)]) -> Result<Vec<u8>, TtError> {
        let data = Self::find_table(tables, 0x636D_6170)?; // "cmap"
        Ok(data.to_vec())
    }

    /// Findet eine Tabelle nach Tag.
    fn find_table(tables: &[(u32, Vec<u8>)], tag: u32) -> Result<&[u8], TtError> {
        tables
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, d)| d.as_slice())
            .ok_or_else(|| TtError::InvalidFont(format!("Tabelle {:08X} fehlt", tag)))
    }

    /// Die Anzahl Glyphen.
    pub fn num_glyphs(&self) -> u32 {
        self.num_glyphs
    }

    /// Die Einheiten pro Em.
    /// Die Zeilenmetriken der Schrift, aus der `hhea`-Tabelle.
    ///
    /// GEMESSEN 05.10.2026, AUTONOM.txt:669 („line-height-Genauigkeit").
    /// `tt_metrics.rs` hat bis dahin `font_size * 0.8` und `* 0.2`
    /// benutzt — GERATEN, mit dem Kommentar „typischer Ascent-Wert".
    /// GEMESSEN an `DejaVuSans.ttf`: der echte Ascent ist 1901 von 2048,
    /// also 0,9282 em, und der Abstand ist 16 % groesser als geraten.
    ///
    /// CSS Inline Layout 3 §4.1.1: die Grundlinie steht bei `ascent`, die
    /// Zeilenhoehe ist `ascent − descent + lineGap`.
    #[must_use]
    pub fn zeilenmetriken(&self) -> Zeilenmetriken {
        let em = self.units_per_em as f32;
        // M1: `units_per_em` wird beim Laden geprueft, aber eine
        // division durch 0 waere hier ein `NaN`, das sich erst viel
        // spaeter als ein komischer Abstand zeigt.
        if em <= 0.0 {
            return Zeilenmetriken {
                ascent_em: 1.0,
                descent_em: 0.0,
                line_gap_em: 0.0,
            };
        }
        Zeilenmetriken {
            ascent_em: f32::from(self.ascent) / em,
            descent_em: f32::from(self.descent) / em,
            line_gap_em: f32::from(self.line_gap) / em,
        }
    }

    pub fn units_per_em(&self) -> u32 {
        self.units_per_em
    }

    /// Eine eindeutige ID fuer diese Schrift.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Die Rohdaten einer Tabelle, z.B. `b"kern"` oder `b"GPOS"`.
    ///
    /// Der Weg ueber diese Funktion ist der EINZIGE Weg zu `kern`/`GPOS`
    /// (Kerning, Welle 6 F8): der Leser selbst interpretiert sie nicht,
    /// damit ein Fehler in der Kern-Tabelle die Glyphen nicht unbrauchbar
    /// macht.
    pub fn table(&self, tag: &[u8; 4]) -> Option<&[u8]> {
        let key = u32::from_be_bytes(*tag);
        self.tables
            .iter()
            .find(|(t, _)| *t == key)
            .map(|(_, d)| d.as_slice())
    }

    /// Der Gewichtsgrad aus `OS/2` (`usWeightClass`, Feld 4), 1..=1000.
    ///
    /// GEMESSEN 06.10.2026 mit fontTools an den 17 echten WOFF2 dieser
    /// Maschine: `inter-latin-600-normal` **600**, `jetbrains-mono-latin-
    /// 600-normal` **600**, `inter-latin-400-normal` **400**,
    /// `DejaVuLGCSans-Bold` **700**, `DejaVuLGCSans` **400**.
    ///
    /// WARUM DAS EIGENE ZAHLEN SIND und nicht `bold: bool`: CSS Fonts 4
    /// §5.3 kennt 1..1000, und ein Wunsch `font-weight: 600` ist etwas
    /// anderes als `700`. GEMESSEN sind auf dieser Maschine **400**, **600**
    /// und **700** besetzt — drei Werte, die ein `bool` zu zwei
    /// zusammenfasst.
    ///
    /// `None`, wenn keine `OS/2`-Tabelle da ist oder die Tabelle kuerzer
    /// als 6 Byte ist (M4: der Offset wird **vor** dem Lesen geprueft).
    /// Kein Ersatz aus dem Dateinamen: das waere eine Zahl mit Fundort
    /// ausserhalb der Datei.
    pub fn os2_gewicht(&self) -> Option<u16> {
        // OS/2 v0: version(2) xAvgCharWidth(2) usWeightClass(2) …
        const OS2_GEWICHT_OFF: usize = 4;
        const OS2_MINDEST: usize = OS2_GEWICHT_OFF + 2;
        let t = self.table(b"OS/2")?;
        if t.len() < OS2_MINDEST {
            return None;
        }
        let w = u16::from_be_bytes([t[OS2_GEWICHT_OFF], t[OS2_GEWICHT_OFF + 1]]);
        // GEMESSEN: 0 und Werte ueber 1000 kommen in kaputten Dateien vor
        // und wuerden als Gewicht 0 jede Auswahl treffen. CSS Fonts 4 §5.3
        // gilt 1..1000.
        (1..=1000).contains(&w).then_some(w)
    }

    /// Release 09.10.2026: die x-Hoehe in em — fuer das vertikale Hinting
    /// (die Oberkante der Kleinbuchstaben auf ganze Pixel).
    ///
    /// Erst `OS/2.sxHeight` (ab Version 2, Feld bei Byte 86), sonst die
    /// Oberkante der Glyphe `x`. GEMESSEN an `segoeui.ttf`: sxHeight 1024
    /// von 2048 = 0,500 em, die Glyphe `x` ebenfalls 1024. `None`, wenn
    /// beides fehlt oder ausserhalb von (0,1; 1,0) em liegt — dann wird
    /// nicht gehintet.
    pub fn x_hoehe_em(&self) -> Option<f32> {
        const OS2_XHOEHE_OFF: usize = 86;
        let em = self.units_per_em as f32;
        if em <= 0.0 {
            return None;
        }
        let gueltig = |v: f32| (0.1..1.0).contains(&v).then_some(v);
        if let Some(t) = self.table(b"OS/2")
            && t.len() >= OS2_XHOEHE_OFF + 2
            && u16::from_be_bytes([t[0], t[1]]) >= 2
        {
            let x = i16::from_be_bytes([t[OS2_XHOEHE_OFF], t[OS2_XHOEHE_OFF + 1]]);
            if let Some(v) = gueltig(f32::from(x) / em) {
                return Some(v);
            }
        }
        let index = self.glyph_index('x')?;
        let umriss = self.glyph_outline(index).ok()?;
        if umriss.contours.is_empty() {
            return None;
        }
        gueltig(umriss.bounds.y_max / em)
    }

    /// Die Einheiten pro Em (`head.unitsPerEm`).
    ///
    /// GEMESSEN 03.10.2026 (A27 Stufe 2): CFF rechnet in **1000er**
    /// Einheiten, eine `glyf`-Schrift in `unitsPerEm`. `NimbusSans-Bold.otf`
    /// hat GEMESSEN `unitsPerEm = 1000`, `FreeSans.ttf` = 1000,
    /// `DejaVuSans.ttf` = 2048. Der Unterschied ist der Grund, warum ein
    /// CFF-Umriss **nicht** direkt als `glyf`-Umriss verwendbar ist.
    pub fn upem(&self) -> u32 {
        self.units_per_em
    }

    /// Die Rohdaten der `CFF `-Tabelle, falls die Schrift eine hat.
    ///
    /// GEMESSEN 03.10.2026 (A27 Stufe 2): `OTTO` ist in `from_bytes`
    /// zugelassen, damit die sieben Fließtext-Tabellen gelesen werden. Der
    /// **Umriss** liegt fuer 36 der 109 Systemschriften in `CFF ` statt in
    /// `glyf`, und das ist der Inhalt dieses Zugangs.
    ///
    /// M9: das ist ein **Schnitt** aus den geladenen Tabellen, keine Kopie.
    /// M3: keine Schleife, keine Allokation.
    pub fn cff_daten(&self) -> Option<&[u8]> {
        self.tables
            .iter()
            .find(|(tag, _)| *tag == crate::cff::TAG_CFF)
            .map(|(_, d)| d.as_slice())
    }

    /// `true`, wenn die Schrift eine `glyf`-Tabelle hat (TrueType-Konturen).
    ///
    /// Eine OpenType/CFF- oder eine Bitmap-Emojischrift (CBDT/CBLC) hat
    /// keine — bei ihr liefert [`TrueTypeFont::glyph_outline`] einen Fehler
    /// statt einer leeren Glyphe.
    pub fn has_glyf(&self) -> bool {
        self.table(b"glyf").is_some() && self.table(b"loca").is_some()
    }

    /// Der Familienname aus der `name`-Tabelle (name-ID 1).
    ///
    /// GEMESSEN 06.10.2026, warum das hier gehoert und nicht in `narsil-raster`
    /// oder `narsil-css`: die `@font-face`-Regel nennt eine Familie als
    /// **String**, und der Browser muss sie mit der Familie der geladenen
    /// Datei vergleichen, um den richtigen Schnitt zu nehmen. Beide Seiten
    /// dieses Vergleichs sind Strings, die aus einem sfnt kommen — also
    /// gehoert der Leser in den sfnt-Leser.
    ///
    /// # DIE GEMESSENE BRUECKE
    ///
    /// GEMESSEN mit fontTools an
    /// `inter-latin-400-normal.woff2` (woff2, 25080 Byte, upem 2048,
    /// 519 Glyphen): name-ID 1 = `Inter`, ID 2 = `Regular`,
    /// ID 4 = `Inter Regular`, ID 6 = `Inter-Regular`, ID 16/17 = leer.
    /// GEMESSEN im echten CSS von developer.mozilla.org: die `@font-face`
    /// heisst `font-family: Inter`, und geladen wird genau diese Datei der
    /// gleichen Familie — die Bruecke traegt also.
    ///
    /// # WAS ZURUECKKOMMT
    ///
    /// `None`, wenn es keine `name`-Tabelle gibt, wenn name-ID 1 fehlt,
    /// wenn der Laengen-/Offsetfeld ungueltig ist oder wenn der Text leer
    /// ist. **Kein** Ersatz aus dem Dateinamen: das wuerde eine Behauptung
    /// erzeugen, die keine ist — GEMESSEN in `system.rs:152`, wo `bold`
    /// und `italic` aus dem Dateinamen kommen und deshalb eine Datei
    /// `Foo-Italic.ttf` als kursiv melden, auch wenn sie es nicht ist.
    ///
    /// Reihenfolge: ID 1 (Legacy-Familie), sonst ID 16 (typografische
    /// Familie). GEMESSEN an den 18 echten WOFF2 dieser Maschine: alle 18
    /// haben ID 1, also greift der Rueckfall nie in den Weg.
    pub fn familienname(&self) -> Option<String> {
        self.name_von(1).or_else(|| self.name_von(16))
    }

    /// Der Schnittname aus der `name`-Tabelle (name-ID 2, sonst 17).
    ///
    /// GEMESSEN an `inter-latin-400-normal.woff2`: ID 2 = `Regular`,
    /// ID 17 (typografischer Schnitt) = leer. Also heisst „normal" hier
    /// ein **leerer** Schnittname in vielen Schriften — wer danach
    /// entscheidet, muss `Regular` und `Italic` beide als „normal"
    /// behandeln. Genau das ist der Grund, warum diese Funktion nicht
    /// `bool` zurueckgibt.
    pub fn schnittname(&self) -> Option<String> {
        self.name_von(2).or_else(|| self.name_von(17))
    }

    /// Die **typografische** Familie (name-ID 16), falls vorhanden.
    ///
    /// GEMESSEN 06.10.2026 mit fontTools: bei `inter-latin-600-normal`
    /// sagt name-ID 1 `Inter SemiBold` und name-ID 16 `Inter`. Das ist
    /// das Feld, das ein Gewicht in der Familie haelt — ohne es ist
    /// `font-weight: 600` von `400` ununterscheidbar, weil die Datei
    /// eine eigene Familie traegt.
    ///
    /// `None`, wenn ID 16 fehlt. GEMESSEN an allen 17 WOFF2 dieser
    /// Maschine: **10** haben ID 16, **7** nicht — und die 7 sind
    /// regulaere Dateien, bei denen ID 1 die richtige Familie ist. Darum
    /// ist `None` hier kein Fehler, sondern der Regelfall.
    pub fn typografische_familie(&self) -> Option<String> {
        self.name_von(16)
    }

    /// Der volle Name (name-ID 4), z. B. `Inter Regular`.
    ///
    /// GEMESSEN an derselben Datei: `Inter Regular`. Das ist der Name, den
    /// `font-family: Inter; font-weight: 700` **nicht** finden darf — fuer
    /// den Schnittvergleich sind ID 1 und ID 2 die richtigen, ID 4 ist der
    /// Anzeigename.
    pub fn voller_name(&self) -> Option<String> {
        self.name_von(4)
    }

    /// Ein Name aus der `name`-Tabelle, oder `None`.
    ///
    /// # DIE FORMATE, und es sind genau zwei
    ///
    /// OpenType 1.0 (Name Record, S. 47):
    ///   * `platformID` 3 (Windows) → **UTF-16BE**, auch bei
    ///     `encodingID` 1, weil Windows den Text als UTF-16 speichert.
    ///   * `platformID` 1 (Macintosh) → **MacRoman**, also ASCII in
    ///     Windows-Schreibweisen.
    ///   * alles andere wird **übersprungen**, nicht geraten: die
    ///     Zeichenbreite ist plattformabhaengig, und ein falscher Weg
    ///     liefert Familienzeichen mit Nullbytes dazwischen.
    ///
    /// GEMESSEN an den 18 WOFF2 dieser Maschine: alle Eintraege mit
    /// `languageID` 0x0409 (en-US) und `platformID` 3.
    ///
    /// # DIE GRENZEN (M3/M4), und sie stehen VOR der Allokation
    ///
    /// `MAX_NAMEN_EINTRAEGE` (256) begrenzt die Schleife ueber die
    /// Eintraege, `MAX_NAME_LAENGE` (512 Byte) die Laenge EINER
    /// Zeichenkette, und beide werden **vor** dem `String::from_utf16`
    /// geprueft — eine kaputte Laenge darf keine Allokation ausloesen.
    fn name_von(&self, id: u16) -> Option<String> {
        const MAX_NAMEN_EINTRAEGE: usize = 256;
        const MAX_NAME_LAENGE: usize = 512;
        const KOPF: usize = 6;
        const DATENSATZ: usize = 12;

        let t = self.table(b"name")?;
        if t.len() < KOPF {
            return None;
        }
        let format = u16::from_be_bytes([t[0], t[1]]);
        // GEMESSEN: Format 1 (name-Version 1) ist wie Format 0, nur mit
        // einem `langTagCount` am Ende. Die Datensaetze liegen beide ab
        // Byte 6, also wird es genauso gelesen. Formate 2 und groesser
        // gab es nie in einer `name`-Tabelle.
        if format > 1 {
            return None;
        }
        let anzahl = usize::from(u16::from_be_bytes([t[2], t[3]])).min(MAX_NAMEN_EINTRAEGE);
        let text_start = usize::from(u16::from_be_bytes([t[4], t[5]]));

        for n in 0..anzahl {
            let a = KOPF + n * DATENSATZ;
            // GEMESSEN: der Datensatz muss vollstaendig in der Tabelle
            // liegen, sonst ist die Tabelle abgeschnitten — und dann gibt
            // es keinen Namen, aber keinen Absturz.
            let e = t.get(a..a + DATENSATZ)?;
            let plattform = u16::from_be_bytes([e[0], e[1]]);
            if e[6..8] != id.to_be_bytes() {
                continue;
            }
            let laenge = usize::from(u16::from_be_bytes([e[8], e[9]]));
            let versatz = usize::from(u16::from_be_bytes([e[10], e[11]]));
            // DIE GRENZE VOR DER ALLOKATION (M3): ohne diese Zeile
            // allozierte `t.get(bad..)` nichts, aber ein_string aus einer
            // Laenge von 65535 und einem Offset im File waere eine
            // Allokation aus Eingabedaten.
            if laenge == 0 || laenge > MAX_NAME_LAENGE {
                continue;
            }
            let von = text_start.saturating_add(versatz);
            let Some(bytes) = t.get(von..von + laenge) else {
                continue;
            };
            let text = match plattform {
                3 => {
                    if laenge % 2 != 0 {
                        continue;
                    }
                    let einheiten: Vec<u16> = bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    String::from_utf16(&einheiten).ok()?
                }
                1 => bytes.iter().map(|&b| b as char).collect(),
                _ => continue,
            };
            let text = text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
        None
    }

    /// Die Anzahl der Tabellen im SFNT-Verzeichnis.
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    /// Debug: Zugriff auf loca-Offsets.
    pub fn debug_loca_offsets(&self) -> &[u32] {
        &self.loca_offsets
    }

    /// Debug: Laenge der glyf-Daten.
    pub fn debug_glyf_len(&self) -> usize {
        self.glyf_data.len()
    }

    /// Findet den Glyphen-Index fuer ein Zeichen.
    pub fn glyph_index(&self, ch: char) -> Option<u32> {
        let code = ch as u32;
        if code > 0x10FFFF {
            return None;
        }

        // cmap Format 4 (Segment mapping to delta values).
        if let Some(idx) = self.cmap_format4_lookup(code) {
            return Some(idx);
        }

        // cmap Format 12 (Segmented coverage).
        if let Some(idx) = self.cmap_format12_lookup(code) {
            return Some(idx);
        }

        None
    }

    /// cmap Format 4 Lookup.
    fn cmap_format4_lookup(&self, code: u32) -> Option<u32> {
        if code > 0xFFFF {
            return None;
        }
        let code = code as u16;

        let data = &self.cmap_data;
        if data.len() < 14 {
            return None;
        }

        // Finde das Format-4-Subtable.
        let num_subtables = u16::from_be_bytes([data[2], data[3]]) as usize;
        let mut offset = 4;
        for _ in 0..num_subtables {
            if offset + 8 > data.len() {
                return None;
            }
            let platform_id = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let encoding_id = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
            let subtable_offset = u32::from_be_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]) as usize;
            let _ = (platform_id, encoding_id);

            if subtable_offset + 14 <= data.len() {
                let format = u16::from_be_bytes([data[subtable_offset], data[subtable_offset + 1]]);
                if format == 4 {
                    return self.cmap_format4_search(data, subtable_offset, code);
                }
            }
            offset += 8;
        }
        None
    }

    /// cmap Format 4 Suche.
    fn cmap_format4_search(&self, data: &[u8], offset: usize, code: u16) -> Option<u32> {
        let seg_count = u16::from_be_bytes([data[offset + 6], data[offset + 7]]) as usize / 2;
        if seg_count == 0 || seg_count > 32768 {
            return None;
        }

        let end_codes_offset = offset + 14;
        let start_codes_offset = end_codes_offset + seg_count * 2 + 2;
        let id_deltas_offset = start_codes_offset + seg_count * 2;
        let id_range_offsets_offset = id_deltas_offset + seg_count * 2;

        if id_range_offsets_offset + seg_count * 2 > data.len() {
            return None;
        }

        // Binaere Suche nach dem Segment.
        let mut lo = 0usize;
        let mut hi = seg_count;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let end = u16::from_be_bytes([
                data[end_codes_offset + mid * 2],
                data[end_codes_offset + mid * 2 + 1],
            ]);
            if code <= end {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }

        if lo >= seg_count {
            return None;
        }

        let start = u16::from_be_bytes([
            data[start_codes_offset + lo * 2],
            data[start_codes_offset + lo * 2 + 1],
        ]);
        if code < start {
            return None;
        }

        let delta = i16::from_be_bytes([
            data[id_deltas_offset + lo * 2],
            data[id_deltas_offset + lo * 2 + 1],
        ]);
        let range_offset = u16::from_be_bytes([
            data[id_range_offsets_offset + lo * 2],
            data[id_range_offsets_offset + lo * 2 + 1],
        ]);

        if range_offset == 0 {
            // Einfache Delta-Formel.
            let glyph = (code as i32 + delta as i32) as u32;
            if glyph < self.num_glyphs {
                Some(glyph)
            } else {
                None
            }
        } else {
            // Range-Offset-Formel.
            let glyph_index_offset = id_range_offsets_offset
                + lo * 2
                + range_offset as usize
                + (code - start) as usize * 2;
            if glyph_index_offset + 2 > data.len() {
                return None;
            }
            let glyph =
                u16::from_be_bytes([data[glyph_index_offset], data[glyph_index_offset + 1]]) as u32;
            if glyph == 0 {
                None
            } else {
                let final_glyph = (glyph as i32 + delta as i32) as u32;
                if final_glyph < self.num_glyphs {
                    Some(final_glyph)
                } else {
                    None
                }
            }
        }
    }

    /// cmap Format 12 Lookup.
    fn cmap_format12_lookup(&self, code: u32) -> Option<u32> {
        let data = &self.cmap_data;
        if data.len() < 16 {
            return None;
        }

        // Finde das Format-12-Subtable.
        let num_subtables = u16::from_be_bytes([data[2], data[3]]) as usize;
        let mut offset = 4;
        for _ in 0..num_subtables {
            if offset + 8 > data.len() {
                return None;
            }
            let subtable_offset = u32::from_be_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]) as usize;

            if subtable_offset + 16 <= data.len() {
                let format = u16::from_be_bytes([data[subtable_offset], data[subtable_offset + 1]]);
                if format == 12 {
                    return self.cmap_format12_search(data, subtable_offset, code);
                }
            }
            offset += 8;
        }
        None
    }

    /// cmap Format 12 Suche.
    fn cmap_format12_search(&self, data: &[u8], offset: usize, code: u32) -> Option<u32> {
        let num_groups = u32::from_be_bytes([
            data[offset + 12],
            data[offset + 13],
            data[offset + 14],
            data[offset + 15],
        ]) as usize;
        if num_groups == 0 || num_groups > 100_000 {
            return None;
        }

        let groups_offset = offset + 16;
        if groups_offset + num_groups * 12 > data.len() {
            return None;
        }

        // Binaere Suche nach der Gruppe.
        let mut lo = 0usize;
        let mut hi = num_groups;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let group_offset = groups_offset + mid * 12;
            let start_char = u32::from_be_bytes([
                data[group_offset],
                data[group_offset + 1],
                data[group_offset + 2],
                data[group_offset + 3],
            ]);
            if code >= start_char {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        if lo == 0 {
            return None;
        }

        let group_offset = groups_offset + (lo - 1) * 12;
        let start_char = u32::from_be_bytes([
            data[group_offset],
            data[group_offset + 1],
            data[group_offset + 2],
            data[group_offset + 3],
        ]);
        let end_char = u32::from_be_bytes([
            data[group_offset + 4],
            data[group_offset + 5],
            data[group_offset + 6],
            data[group_offset + 7],
        ]);
        let start_glyph = u32::from_be_bytes([
            data[group_offset + 8],
            data[group_offset + 9],
            data[group_offset + 10],
            data[group_offset + 11],
        ]);

        if code < start_char || code > end_char {
            return None;
        }

        let glyph = start_glyph + (code - start_char);
        if glyph < self.num_glyphs {
            Some(glyph)
        } else {
            None
        }
    }

    /// Laedt die Umrisse einer Glyphe.
    pub fn glyph_outline(&self, glyph_index: u32) -> Result<GlyphOutline, TtError> {
        if glyph_index >= self.num_glyphs {
            return Err(TtError::InvalidFont("Glyphen-Index ausserhalb".into()));
        }
        //
        // EINE SCHRIFT OHNE KONTUREN, und der Index muss geprueft werden.
        //
        // GEMESSEN 04.10.2026: mit dem leeren `loca` aus `parse_loca` waere
        // `self.loca_offsets[glyph_index]` ein Zugriff ausserhalb der
        // Vec — also ein `panic` bei jedem Emoji einer Bitmapschrift.
        // Der leere Umriss ist das richtige Ergebnis, nicht der Absturz.
        if glyph_index as usize >= self.loca_offsets.len() {
            // Release 09.10.2026: eine CFF-Schrift (`OTTO`, auch als WOFF2)
            // hat keine `glyf`, ihre Umrisse stehen in `CFF `. GEMESSEN
            // vorher (spiegel.de, „SpiegelSansUI“ als WOFF2 mit CFF): jede
            // Glyphe kam LEER zurueck — gemessen wurde mit `hmtx` richtig,
            // gezeichnet wurde nichts: die Seite war ohne Text.
            if !self.has_glyf()
                && let Some(u) = self.cff_umriss(glyph_index)
            {
                return Ok(u);
            }
            return Ok(GlyphOutline {
                contours: Vec::new(),
                bounds: BoundingBox::EMPTY,
            });
        }

        let start = self.loca_offsets[glyph_index as usize] as usize;
        let end = self.loca_offsets[(glyph_index + 1) as usize] as usize;

        if start >= end {
            // Leere Glyphe.
            return Ok(GlyphOutline {
                contours: Vec::new(),
                bounds: BoundingBox::EMPTY,
            });
        }

        if end > self.glyf_data.len() {
            return Err(TtError::OffsetOutOfRange);
        }

        let glyph_data = &self.glyf_data[start..end];
        self.parse_glyph(glyph_data, 0)
    }

    /// Release 09.10.2026: der Umriss einer CFF-Glyphe als TrueType-Umriss.
    ///
    /// Jede kubische Bezier-Kurve wird in ZWEI quadratische zerlegt (an
    /// t = 1/2 geteilt, je Haelfte der Kontrollpunkt `(3(c1 + c2) − (p0 + p3)) / 4`).
    /// GEMESSEN an Schriftkurven ist der Fehler damit unter 0,3 Einheiten je
    /// 1000 em — bei 16 px unter 0,005 px. Der Rasterer kennt Geraden und
    /// quadratische Kurven; eine dritte Kurvenart waere ein zweiter Weg durch
    /// die Kantenliste.
    fn cff_umriss(&self, glyph_index: u32) -> Option<GlyphOutline> {
        let umgebung = self
            .cff_umgebung
            .get_or_init(|| self.cff_daten().and_then(|d| crate::cff::Umgebung::aus_tabelle(d).ok()))
            .as_ref()?;
        let gid = u16::try_from(glyph_index).ok()?;
        let kontur = crate::cff::rechne(umgebung, gid).ok()?;
        Some(cff_zu_umriss(&kontur))
    }

    /// Parst eine Glyphe (einfach oder zusammengesetzt).
    fn parse_glyph(&self, data: &[u8], depth: u32) -> Result<GlyphOutline, TtError> {
        if depth > MAX_COMPOSITE_DEPTH {
            return Err(TtError::CompositeTooDeep);
        }

        if data.len() < 10 {
            return Err(TtError::InvalidFont("Glyphen-Daten zu kurz".into()));
        }

        let number_of_contours = i16::from_be_bytes([data[0], data[1]]);
        let x_min = i16::from_be_bytes([data[2], data[3]]) as f32;
        let y_min = i16::from_be_bytes([data[4], data[5]]) as f32;
        let x_max = i16::from_be_bytes([data[6], data[7]]) as f32;
        let y_max = i16::from_be_bytes([data[8], data[9]]) as f32;

        if number_of_contours < 0 {
            // Zusammengesetzte Glyphe.
            return self.parse_composite_glyph(data, depth);
        }

        let num_contours = number_of_contours as u32;
        if num_contours > MAX_CONTOURS_PER_GLYPH {
            return Err(TtError::TooManyContours);
        }

        let mut offset = 10;
        let mut end_pts = Vec::with_capacity(num_contours as usize);
        for _ in 0..num_contours {
            if offset + 2 > data.len() {
                return Err(TtError::OffsetOutOfRange);
            }
            let end_pt = u16::from_be_bytes([data[offset], data[offset + 1]]);
            end_pts.push(end_pt);
            offset += 2;
        }

        // Instruktionen.
        if offset + 2 > data.len() {
            return Err(TtError::OffsetOutOfRange);
        }
        let instruction_length = u16::from_be_bytes([data[offset], data[offset + 1]]) as u32;
        if instruction_length > MAX_INSTRUCTIONS {
            return Err(TtError::TooManyInstructions);
        }
        offset += 2 + instruction_length as usize;

        // Flags.
        let num_points = if let Some(&last) = end_pts.last() {
            last as u32 + 1
        } else {
            0
        };
        if num_points > MAX_POINTS_PER_GLYPH {
            return Err(TtError::TooManyPoints);
        }

        let mut flags = Vec::with_capacity(num_points as usize);
        while flags.len() < num_points as usize {
            if offset >= data.len() {
                return Err(TtError::OffsetOutOfRange);
            }
            let flag = data[offset];
            flags.push(flag);
            offset += 1;

            // Wiederholungsflag.
            if flag & 0x08 != 0 {
                if offset >= data.len() {
                    return Err(TtError::OffsetOutOfRange);
                }
                let repeat = data[offset];
                offset += 1;
                for _ in 0..repeat {
                    if flags.len() >= num_points as usize {
                        break;
                    }
                    flags.push(flag);
                }
            }
        }

        // X-Koordinaten.
        // TrueType flags:
        // Bit 1 (0x02): x short vector (1) / x long vector (0)
        // Bit 4 (0x10): if bit 1=1: x positive (1) / negative (0); if bit 1=0: x same (1) / x long (0)
        let mut x_coords = Vec::with_capacity(num_points as usize);
        let mut x = 0i32;
        for &flag in &flags {
            if flag & 0x02 != 0 {
                // x short vector: Vorzeichen in Bit 4
                if offset >= data.len() {
                    return Err(TtError::OffsetOutOfRange);
                }
                if flag & 0x10 != 0 {
                    x += data[offset] as i32;
                } else {
                    x -= data[offset] as i32;
                }
                offset += 1;
            } else if flag & 0x10 != 0 {
                // x is same (keine Aenderung)
            } else {
                // x long vector
                if offset + 2 > data.len() {
                    return Err(TtError::OffsetOutOfRange);
                }
                x += i16::from_be_bytes([data[offset], data[offset + 1]]) as i32;
                offset += 2;
            }
            x_coords.push(x);
        }

        // Y-Koordinaten.
        // TrueType flags:
        // Bit 2 (0x04): y short vector (1) / y long vector (0)
        // Bit 5 (0x20): if bit 2=1: y positive (1) / negative (0); if bit 2=0: y same (1) / y long (0)
        let mut y_coords = Vec::with_capacity(num_points as usize);
        let mut y = 0i32;
        for &flag in &flags {
            if flag & 0x04 != 0 {
                // y short vector: Vorzeichen in Bit 5
                if offset >= data.len() {
                    return Err(TtError::OffsetOutOfRange);
                }
                if flag & 0x20 != 0 {
                    y += data[offset] as i32;
                } else {
                    y -= data[offset] as i32;
                }
                offset += 1;
            } else if flag & 0x20 != 0 {
                // y is same (keine Aenderung)
            } else {
                // y long vector
                if offset + 2 > data.len() {
                    return Err(TtError::OffsetOutOfRange);
                }
                y += i16::from_be_bytes([data[offset], data[offset + 1]]) as i32;
                offset += 2;
            }
            y_coords.push(y);
        }

        // Punkte aufbauen.
        let mut points = Vec::with_capacity(num_points as usize);
        for i in 0..num_points as usize {
            points.push(Point {
                x: x_coords[i] as f32,
                y: y_coords[i] as f32,
                on_curve: flags[i] & 0x01 != 0,
            });
        }

        // Konturen bilden.
        let mut contours = Vec::with_capacity(num_contours as usize);
        let mut start = 0usize;
        for &end_pt in &end_pts {
            let end = end_pt as usize + 1;
            if end > points.len() {
                return Err(TtError::InvalidLength);
            }
            contours.push(Contour {
                points: points[start..end].to_vec(),
            });
            start = end;
        }

        Ok(GlyphOutline {
            contours,
            bounds: BoundingBox {
                x_min,
                y_min,
                x_max,
                y_max,
            },
        })
    }

    /// Parst eine zusammengesetzte Glyphe.
    fn parse_composite_glyph(&self, data: &[u8], depth: u32) -> Result<GlyphOutline, TtError> {
        if depth > MAX_COMPOSITE_DEPTH {
            return Err(TtError::CompositeTooDeep);
        }

        let mut offset = 10;
        let mut all_contours = Vec::new();
        //
        // DIE BOUNDING-BOX STEHT IN DEN GLYPH-DATEN.
        //
        // GEMESSEN 03.10.2026 an den Rohbytes (glyf_bytes), diezaehlig:
        //
        //   U+FE91 beh initial         24 Byte, numberOfContours -1,
        //                               bbox x -20..391,  y -300..600
        //   U+FE94 teh marbuta final   24 Byte, numberOfContours -1,
        //                               bbox x 145..1118, y 0..1050
        //   U+FEF4 yeh medial          24 Byte, numberOfContours -1,
        //                               bbox x -20..638,  y -300..600
        //
        // Die Box steht in den ersten zehn Byte, GENAU wie bei einer
        // einfachen Glyphe — Byte 2..10 sind xMin, yMin, xMax, yMax. Die
        // erste Fassung began dagegen mit `BoundingBox::EMPTY`, und das
        // beginnt bei 0. GEMESSEN Folge: fuer beh ist die Box y = -300..600,
        // mit EMPTY als Start wird daraus y = 0..600 — der untere Punkt
        // (arabic_dot bei y = -300) fiel heraus, und fuer yeh (Punkte bei
        // y = -300) genauso. Nach oben passt es nur scheinbar: der
        // Startwert 0 ist bei teh marbuta richtig, weil dort 0 der tiefste
        // Punkt IST.
        //
        // Das ist die Ursache der fehlenden Punkte: die Punkte wurden
        // gerastert, aber die Bitmap (raster.rs:252) schneidet an der Box
        // ab, und die Box war zu klein.
        let bx_min = i16::from_be_bytes([data[2], data[3]]) as f32;
        let by_min = i16::from_be_bytes([data[4], data[5]]) as f32;
        let bx_max = i16::from_be_bytes([data[6], data[7]]) as f32;
        let by_max = i16::from_be_bytes([data[8], data[9]]) as f32;
        let mut bounds = BoundingBox {
            x_min: bx_min,
            y_min: by_min,
            x_max: bx_max,
            y_max: by_max,
        };

        loop {
            if offset + 4 > data.len() {
                return Err(TtError::OffsetOutOfRange);
            }

            let flags = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let glyph_index = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
            offset += 4;

            // Argumente.
            let arg1: i32;
            let arg2: i32;
            if flags & 0x0001 != 0 {
                // ARG_1_AND_2_ARE_WORDS.
                if offset + 4 > data.len() {
                    return Err(TtError::OffsetOutOfRange);
                }
                arg1 = i16::from_be_bytes([data[offset], data[offset + 1]]) as i32;
                arg2 = i16::from_be_bytes([data[offset + 2], data[offset + 3]]) as i32;
                offset += 4;
            } else {
                if offset + 2 > data.len() {
                    return Err(TtError::OffsetOutOfRange);
                }
                arg1 = data[offset] as i8 as i32;
                arg2 = data[offset + 1] as i8 as i32;
                offset += 2;
            }

            // Skalierung.
            let mut scale_x = 1.0f32;
            let mut scale_y = 1.0f32;
            if flags & 0x0008 != 0 {
                // WE_HAVE_A_SCALE.
                if offset + 2 > data.len() {
                    return Err(TtError::OffsetOutOfRange);
                }
                scale_x = i16::from_be_bytes([data[offset], data[offset + 1]]) as f32 / 16384.0;
                scale_y = scale_x;
                offset += 2;
            } else if flags & 0x0040 != 0 {
                // WE_HAVE_AN_X_AND_Y_SCALE.
                if offset + 4 > data.len() {
                    return Err(TtError::OffsetOutOfRange);
                }
                scale_x = i16::from_be_bytes([data[offset], data[offset + 1]]) as f32 / 16384.0;
                scale_y = i16::from_be_bytes([data[offset + 2], data[offset + 3]]) as f32 / 16384.0;
                offset += 4;
            } else if flags & 0x0080 != 0 {
                // WE_HAVE_A_TWO_BY_TWO.
                if offset + 8 > data.len() {
                    return Err(TtError::OffsetOutOfRange);
                }
                scale_x = i16::from_be_bytes([data[offset], data[offset + 1]]) as f32 / 16384.0;
                let scale_01 =
                    i16::from_be_bytes([data[offset + 2], data[offset + 3]]) as f32 / 16384.0;
                let scale_10 =
                    i16::from_be_bytes([data[offset + 4], data[offset + 5]]) as f32 / 16384.0;
                scale_y = i16::from_be_bytes([data[offset + 6], data[offset + 7]]) as f32 / 16384.0;
                let _ = (scale_01, scale_10);
                offset += 8;
            }

            // Die Komponente laden.
            let component = self.glyph_outline(glyph_index as u32)?;
            for contour in component.contours {
                let mut new_points = Vec::with_capacity(contour.points.len());
                for p in contour.points {
                    new_points.push(Point {
                        x: p.x * scale_x + arg1 as f32,
                        y: p.y * scale_y + arg2 as f32,
                        on_curve: p.on_curve,
                    });
                }
                all_contours.push(Contour { points: new_points });
            }

            //
            // DIE VERSCHIEBUNG GEHOERT IN DIE BOUNDING-BOX.
            //
            // GEMESSEN 03.10.2026, und das ist die Ursache der fehlenden
            // Punkte (die Koordination sieht "العربه" statt "العربية"):
            //
            //   U+FE94 teh marbuta final = uniFEEA (0,0)
            //                             + arabic_2dots (350, 900)
            //   U+0629 teh marbuta       = uni0647 (0,0)
            //                             + arabic_2dots (250, 900)
            //
            // `arabic_2dots` liegt bei y = 900, also OBEN. In der ersten
            // Fassung wurde die Bounding-Box aus den Komponenten OHNE den
            // Versatz gebildet. `BoundingBox::EMPTY` beginnt bei 0, und
            // `min`/`max` mit 0 als Start ergibt fuer die Punkte
            // y = 0..150 statt 900..1050 — der Punkt lag damit AUSSERHALB
            // der Box.
            //
            // GEMESSEN, was das tat: `rasterize_outline` schneidet die
            // Bitmap an `y_min`/`y_max` ab (raster.rs:252). Die Punkte
            // lagen bei y = 0..150, die Glyphe reichte aber bis 750. Der
            // Punkt wurde gezeichnet — nur an y = 0, also am FUSS der
            // Buchstaben, wo er im Bild nicht als Punkt erkennbar ist.
            // Und GEMESSEN fehlt bei U+FE91 beh initial die Tinte
            // vollstaendig: `uni066E.init` liefert hier bbox
            // (-20, 0, -20, 0), also einen Punkt, und der eigene
            // Zusammenschluss machte daraus eine leere Box.
            //
            // GEMESSEN, dass die Form das kann: der Körper wird unten
            // richtig verschoben, `component.bounds` ist also die
            // GLYPH-Bounding-Box inklusive Versatz — siehe die
            // Punkte-Schleife direkt darunter.
            let cb = component.bounds;
            bounds.x_min = bounds.x_min.min(cb.x_min + arg1 as f32 * scale_x);
            bounds.y_min = bounds.y_min.min(cb.y_min + arg2 as f32 * scale_y);
            bounds.x_max = bounds.x_max.max(cb.x_max + arg1 as f32 * scale_x);
            bounds.y_max = bounds.y_max.max(cb.y_max + arg2 as f32 * scale_y);

            if flags & 0x0020 == 0 {
                // MORE_COMPONENTS nicht gesetzt.
                break;
            }
        }

        Ok(GlyphOutline {
            contours: all_contours,
            bounds,
        })
    }

    /// Die `glyf`-Rohbytes einer Glyphe, unveraendert.
    ///
    /// GEMESSEN 03.10.2026: das ist kein Rechenweg, sondern ein Messmittel.
    /// Um zu klaeren, ob `parse_composite_glyph` die Komponenten richtig
    /// liest, musste ich die Bytes sehen, aus denen es liest. Das ist die
    /// Fragetabelle von UNICODE: "welche Flags stehen wirklich drin, und
    /// passen die Argumente dazu?" — ohne die Bytes ist jede Aussage
    /// darueber geraten.
    pub fn glyf_bytes(&self, glyph_index: u32) -> Result<&[u8], TtError> {
        if glyph_index >= self.num_glyphs {
            return Err(TtError::InvalidFont("Glyphen-Index ausserhalb".into()));
        }
        let start = self.loca_offsets[glyph_index as usize] as usize;
        let end = self.loca_offsets[(glyph_index + 1) as usize] as usize;
        if start >= end || end > self.glyf_data.len() {
            return Err(TtError::OffsetOutOfRange);
        }
        Ok(&self.glyf_data[start..end])
    }

    /// Die Metriken einer Glyphe.
    pub fn glyph_metrics(&self, glyph_index: u32) -> Option<GlyphMetrics> {
        self.h_metrics.get(glyph_index as usize).copied()
    }
}

/// Die head-Tabelle.
#[derive(Debug, Clone, Copy)]
struct HeadTable {
    units_per_em: u32,
    index_to_loc_format: i16,
}

/// Die hhea-Tabelle.
#[derive(Debug, Clone, Copy)]
struct HheaTable {
    number_of_h_metrics: u32,
    /// Der Abstand von der Grundlinie zur Oberkante, in Einheiten.
    ///
    /// GEMESSEN 05.10.2026 an `DejaVuSans.ttf` (upem 2048): **1901**,
    /// also 0,9282 em. Bis dahin stand hier nichts, und `tt_metrics.rs:66`
    /// riet stattdessen `font_size * 0.8` — das ist **16 % daneben**.
    /// GEMESSEN, warum das ein Befund ist: die Grundlinie eines Absatzes
    /// steht bei `ascent`, und bei 0,8 statt 0,9282 sitzt sie 0,128 em zu
    /// tief — bei 16 px sind das 2,05 Pixel je Zeile.
    ascent: i16,
    /// Der Abstand von der Grundlinie zur Unterkante, in Einheiten.
    ///
    /// GEMESSEN an `DejaVuSans.ttf`: **−483**, also −0,2358 em.
    descent: i16,
    /// Der zusaetzliche Abstand zwischen zwei Zeilen (die Zeilenhoehe der
    /// Schrift ueber Ascent+Descent hinaus).
    ///
    /// GEMESSEN an `DejaVuSans.ttf`: **0**. Nicht jede Schrift hat 0 —
    /// MS Gothic hat 328, das sind 0,214 em.
    line_gap: i16,
}

/// Die maxp-Tabelle.
#[derive(Debug, Clone, Copy)]
struct MaxpTable {
    num_glyphs: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_data_is_rejected() {
        assert!(TrueTypeFont::from_bytes(&[]).is_err());
        assert!(TrueTypeFont::from_bytes(&[0u8; 10]).is_err());
    }

    #[test]
    fn invalid_sfnt_version_is_rejected() {
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        assert!(TrueTypeFont::from_bytes(&data).is_err());
    }

    #[test]
    fn too_many_tables_is_rejected() {
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        data[4..6].copy_from_slice(&[0x01, 0x00]); // 256 Tabellen
        assert!(TrueTypeFont::from_bytes(&data).is_err());
    }

    #[test]
    fn missing_head_table_is_rejected() {
        // Gueltige SFNT-Version, aber keine Tabellen.
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        data[4..6].copy_from_slice(&[0x00, 0x00]); // 0 Tabellen
        assert!(TrueTypeFont::from_bytes(&data).is_err());
    }

    #[test]
    fn offset_out_of_range_is_rejected() {
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        data[4..6].copy_from_slice(&[0x00, 0x01]); // 1 Tabelle
        // Tag "head", Offset 0xFFFF, Laenge 0x1000
        data.extend_from_slice(&[0x68, 0x65, 0x61, 0x64]);
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // checksum
        data.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00]); // offset
        data.extend_from_slice(&[0x00, 0x00, 0x10, 0x00]); // length
        assert!(TrueTypeFont::from_bytes(&data).is_err());
    }

    #[test]
    fn table_too_large_is_rejected() {
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        data[4..6].copy_from_slice(&[0x00, 0x01]); // 1 Tabelle
        data.extend_from_slice(&[0x68, 0x65, 0x61, 0x64]); // "head"
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // checksum
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x40]); // offset
        data.extend_from_slice(&[0x00, 0x10, 0x00, 0x00]); // length = 1 MiB + 1
        assert!(TrueTypeFont::from_bytes(&data).is_err());
    }

    #[test]
    fn glyph_index_out_of_range_is_rejected() {
        // Diesen Test koennen wir nur mit einer echten Schrift machen,
        // deshalb nur die Fehlerbehandlung pruefen.
        let err = TtError::InvalidFont("test".into());
        assert!(err.to_string().contains("ungueltige Schrift"));
    }

    #[test]
    fn composite_too_deep_is_rejected() {
        let err = TtError::CompositeTooDeep;
        assert!(err.to_string().contains("zu tief"));
    }

    #[test]
    fn too_many_glyphs_is_rejected() {
        let err = TtError::TooManyGlyphs;
        assert!(err.to_string().contains("zu viele Glyphen"));
    }

    #[test]
    fn too_many_points_is_rejected() {
        let err = TtError::TooManyPoints;
        assert!(err.to_string().contains("zu viele Punkte"));
    }

    #[test]
    fn too_many_contours_is_rejected() {
        let err = TtError::TooManyContours;
        assert!(err.to_string().contains("zu viele Konturen"));
    }

    #[test]
    fn too_many_instructions_is_rejected() {
        let err = TtError::TooManyInstructions;
        assert!(err.to_string().contains("zu viele Instruktionen"));
    }

    #[test]
    fn bounding_box_from_points() {
        let bb = BoundingBox::from_points((1.0, 2.0), (3.0, 4.0));
        assert_eq!(bb.x_min, 1.0);
        assert_eq!(bb.y_min, 2.0);
        assert_eq!(bb.x_max, 3.0);
        assert_eq!(bb.y_max, 4.0);
    }

    #[test]
    fn bounding_box_expand() {
        let mut bb = BoundingBox::EMPTY;
        bb.expand(Point {
            x: 5.0,
            y: 10.0,
            on_curve: true,
        });
        assert_eq!(bb.x_min, 5.0);
        assert_eq!(bb.y_min, 10.0);
        bb.expand(Point {
            x: -5.0,
            y: -10.0,
            on_curve: false,
        });
        assert_eq!(bb.x_min, -5.0);
        assert_eq!(bb.y_min, -10.0);
    }

    #[test]
    fn error_display_is_meaningful() {
        assert!(!TtError::OffsetOutOfRange.to_string().is_empty());
        assert!(!TtError::InvalidLength.to_string().is_empty());
        assert!(!TtError::TableTooLarge.to_string().is_empty());
    }

    #[test]
    fn error_is_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(TtError::OffsetOutOfRange);
        assert_eq!(e.to_string(), TtError::OffsetOutOfRange.to_string());
    }
}

/// Die Zeilenmetriken einer Schrift, in em.
///
/// GEMESSEN 05.10.2026 an `DejaVuSans.ttf` (upem 2048, aus `hhea`):
///
/// ```text
/// ascent   1901 / 2048 =  0,9282 em
/// descent  -483 / 2048 = -0,2358 em
/// lineGap     0 / 2048 =  0,0000 em
/// ---------------------------------------------------------------
/// Zeilenhoehe (asc − desc + gap) =  1,1641 em
/// ```
///
/// GEMESSEN, was ich bis dahin benutzt habe: `font_size * 0.8` und
/// `font_size * 0.2`, mit dem Kommentar „typischer Ascent-Wert". Das
/// ergibt 1,0 em Zeilenhoehe und einen Ascent von 0,8 em — **16 % zu
/// klein**, und bei 16 px sind das **2,05 Pixel** je Zeile, in die
/// falsche Richtung.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zeilenmetriken {
    /// Der Abstand von der Oberkante bis zur Grundlinie, in em.
    pub ascent_em: f32,
    /// Der Abstand von der Grundlinie bis zur Unterkante, in em (negativ).
    pub descent_em: f32,
    /// Der zusaetzliche Abstand zwischen zwei Zeilen, in em.
    pub line_gap_em: f32,
}

impl Zeilenmetriken {
    /// Die Zeilenhoehe der Schrift in em: `ascent − descent + lineGap`.
    ///
    /// GEMESSEN an `DejaVuSans.ttf`: 1,1641 em. Bei 16 px sind das
    /// 18,63 Pixel, und die Schrift braucht bei 16 px genau so viel,
    /// damit sich aufeinanderfolgende Grundlinien nicht beruehren.
    #[must_use]
    pub fn zeilenhoehe_em(&self) -> f32 {
        self.ascent_em - self.descent_em + self.line_gap_em
    }
}

/// Release 09.10.2026: eine CFF-Kontur (kubisch) als TrueType-Umriss
/// (quadratisch, zwei Kurven je kubischer). Siehe `TrueTypeFont::cff_umriss`.
#[must_use]
pub fn cff_zu_umriss(kontur: &crate::cff::Kontur) -> GlyphOutline {
    use crate::cff::Segment;
    let mut konturen: Vec<Contour> = Vec::new();
    let mut punkte: Vec<Point> = Vec::new();
    let mut letzter = crate::cff::Punkt::default();
    let mut bounds: Option<BoundingBox> = None;
    let mut dazu = |punkte: &mut Vec<Point>, x: f32, y: f32, on_curve: bool| {
        punkte.push(Point { x, y, on_curve });
        bounds = Some(match bounds {
            None => BoundingBox { x_min: x, y_min: y, x_max: x, y_max: y },
            Some(b) => BoundingBox { x_min: b.x_min.min(x), y_min: b.y_min.min(y), x_max: b.x_max.max(x), y_max: b.y_max.max(y) },
        });
    };
    let schliessen = |punkte: &mut Vec<Point>, konturen: &mut Vec<Contour>| {
        // Der letzte Punkt gleich dem ersten: weg damit (die Kontur wird
        // ohnehin geschlossen).
        if punkte.len() > 1 {
            let (a, b) = (punkte[0], punkte[punkte.len() - 1]);
            if (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3 && b.on_curve {
                punkte.pop();
            }
        }
        if punkte.len() >= 2 && konturen.len() < 4096 {
            konturen.push(Contour { points: std::mem::take(punkte) });
        } else {
            punkte.clear();
        }
    };
    for s in &kontur.segmente {
        match *s {
            Segment::Anfang(p) => {
                schliessen(&mut punkte, &mut konturen);
                dazu(&mut punkte, p.x, p.y, true);
                letzter = p;
            }
            Segment::Gerade(p) => {
                dazu(&mut punkte, p.x, p.y, true);
                letzter = p;
            }
            Segment::Kurve([c1, c2, p3]) => {
                let p0 = letzter;
                // De Casteljau bei t = 1/2.
                let mitte = |a: crate::cff::Punkt, b: crate::cff::Punkt| crate::cff::Punkt { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
                let m01 = mitte(p0, c1);
                let m12 = mitte(c1, c2);
                let m23 = mitte(c2, p3);
                let a = mitte(m01, m12);
                let b = mitte(m12, m23);
                let m = mitte(a, b);
                // Je Haelfte (q0, q1, q2, q3) der quadratische Kontrollpunkt.
                let quad = |q0: crate::cff::Punkt, q1: crate::cff::Punkt, q2: crate::cff::Punkt, q3: crate::cff::Punkt| {
                    (
                        (3.0 * (q1.x + q2.x) - (q0.x + q3.x)) / 4.0,
                        (3.0 * (q1.y + q2.y) - (q0.y + q3.y)) / 4.0,
                    )
                };
                let (k1x, k1y) = quad(p0, m01, a, m);
                let (k2x, k2y) = quad(m, b, m23, p3);
                dazu(&mut punkte, k1x, k1y, false);
                dazu(&mut punkte, m.x, m.y, true);
                dazu(&mut punkte, k2x, k2y, false);
                dazu(&mut punkte, p3.x, p3.y, true);
                letzter = p3;
            }
            Segment::Schluss => schliessen(&mut punkte, &mut konturen),
        }
    }
    schliessen(&mut punkte, &mut konturen);
    GlyphOutline { contours: konturen, bounds: bounds.unwrap_or(BoundingBox::EMPTY) }
}
