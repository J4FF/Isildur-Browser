//! Eingebettete Bitmap-Schrift: Glyphen-Bitmaps und Vorschub.
//!
//! Die Schrift ist eine klassische 5x7-ASCII-Bitmap, vollstaendig im
//! Quellcode eingebettet. Es gibt keine Schriftdateien, keine
//! Systemfonts und keine Umgebungsvariablen. `BitmapFont::builtin()`
//! arbeitet daher auch ohne Dateisystemzugriff.
//!
//! # Datenformat
//!
//! Pro Zeichen liegen 5 Bytes vor — je ein Byte pro Spalte. Bit 0 ist die
//! oberste Zeile, Bit 6 die unterste (7 Zeilen). Gespeichert wird
//! ausschliesslich die Tinte; der Seitenabstand steht in [`CELL_COLUMNS`].
//!
//! # Zelle und Vorschub
//!
//! Die Schrift ist Monospace. Eine Zeichenzelle ist [`CELL_COLUMNS`]
//! (5 Tinte + 1 Abstandsspalte) breit und [`BASE_ROWS`] Zeilen hoch.
//! `rasterize_glyph` liefert die **komplette Zelle** inklusive des
//! Abstandsspalte, und `advance` liefert exakt deren Breite. Dadurch gilt
//! immer `advance == bitmap.width`: der Text klafft nicht auseinander und
//! ueberlappt auch nicht.
//!
//! # Skalierung
//!
//! Die Basisglyphe wird nearest-neighbour auf die angeforderte
//! Schriftgroesse gezogen. Eine Basespalte `c` belegt im Ziel die
//! Pixelhalb-offen `[round(c * s), round((c + 1) * s))` mit
//! `s = font_size / BASE_ROWS`. Weil die Intervallgrenzen ganzzahlig
//! gerundet werden, zerlegen sich die Spalten und Zeilen lueckenlos auf
//! die Zelle — es entstehen weder Luecken noch Ueberlaenge.

/// Breite der Zeichenzelle in Basespalten (5 Tinte + 1 Seitenabstand).
pub const CELL_COLUMNS: u32 = 6;

/// Hoehe der Zeichenzelle in Basiszeilen.
pub const BASE_ROWS: u32 = 7;

/// Anzahl der Spalten mit tatsaechlicher Tinte.
const INK_COLUMNS: u32 = 5;

/// Zeichnet den .notdef-Platzhalter in eine bereits belegte Zelle.
///
/// Ein Rechteck-Rahmen: die aeusseren Pixel sind gedeckt, die Mitte
/// bleibt frei. Das unterscheidet ihn von einem Fuellblock und macht
/// "zwei Kisten nebeneinander" sichtbar als zwei Rahmen.
///
/// Bei sehr kleinen Zellen (1x1) wird das eine einzelne gedeckte Pixel —
/// ein 1x1-Rahmen waere unsichtbar.
fn draw_notdef(alpha: &mut [u8], width: u32, height: u32) {
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 || alpha.len() < w * h {
        return;
    }

    // Ein-Pixel-Zellen: ein Punkt ist das kleinste Sichtbare.
    if w == 1 || h == 1 {
        alpha[0] = 255;
        return;
    }

    let setze = |alpha: &mut [u8], x: usize, y: usize| {
        if let Some(p) = alpha.get_mut(y * w + x) {
            *p = 255;
        }
    };

    // Oben und unten: volle Zeile.
    for x in 0..w {
        setze(alpha, x, 0);
        setze(alpha, x, h - 1);
    }
    // Links und rechts: volle Spalte, ausser den Ecken sind die
    // Horizontalen schon gesetzt — das ist Absicht, ein Rahmen.
    for y in 0..h {
        setze(alpha, 0, y);
        setze(alpha, w - 1, y);
    }
}

/// Erstes Zeichen der Schrift: Space (U+0020).
const FIRST_CHAR: char = ' ';

/// Letztes Zeichen der Schrift: Tilde (U+007E).
const LAST_CHAR: char = '~';

/// Kleinste erlaubte Schriftgroesse. Alles darunter wird auf 1 Pixel
/// gerundet, damit niemals eine 0x0-Glyphe entsteht.
const MIN_SIDE: u32 = 1;

/// Groesste erlaubte Schriftgroesse in Pixeln.
///
/// Begrenzt die Zellenausdehnung auf 4096 Pixel je Achse.
///
/// DAS IST EINE KOSTENGRENZE, KEINE SICHERHEITSGRENZE.
///
/// GEMESSEN am 29.09.2026 (`projekt/ENTSCHEIDUNG_E2.txt`). Die frueher
/// hier stehende Begruendung lautete: "Ohne diese Grenze wuerde ein
/// absurder Wert wie `f32::MAX` eine Allokation von mehreren Gigabyte
/// anfordern und damit panicen." Beide Haelfte waren falsch:
///
///   * `f32::MAX` alloziert gar nichts und paniced nicht -- es liefert
///     `None`, weil `is_valid_font_size` es eine Zeile weiter oben
///     ablehnt.
///   * Der Absturz liegt nicht bei 4096, sondern beim verfuegbaren
///     Speicher. In einer Kopie dieser Crate mit entfernter Grenze:
///     `font-size` 1e5 alloziert 8,0 GiB und gelingt, 1e6 fordert
///     818,0 GiB und bricht den Prozess ab. Ein solcher Abbruch ist ein
///     `abort`, kein `panic` -- `catch_unwind` faengt ihn nicht.
///
/// Der Preis dieser Grenze, ebenfalls gemessen: bei `font-size: 4096` ist
/// EINE Zelle 13,7 MiB gross (3511 x 4096 Byte), alle 95 Zeichen der
/// eingebetteten Schrift waeren 1,27 GiB.
///
/// DIE Zahl SELBST IST NICHT ABGELEITET, sondern gesetzt: 4096 = 2^12.
/// 1024 oder 8192 waeren genauso begruendbar gewesen. Sie ist eine
/// Entscheidung und umkehrbar (Leitungsregel 4). Praktische
/// Schriftgroessen liegen weit darunter.
pub const MAX_FONT_SIZE: f32 = 4096.0;

/// `true`, wenn `font_size` von [`crate::rasterize_glyph`] und
/// [`crate::advance`] verarbeitet werden kann.
///
/// Gueltig heisst: endlich, groesser als 0 und hoechstens
/// [`MAX_FONT_SIZE`].
pub fn is_valid_font_size(font_size: f32) -> bool {
    font_size.is_finite() && font_size > 0.0 && font_size <= MAX_FONT_SIZE
}

/// Bitdaten der eingebetteten Schrift, 5 Bytes pro Zeichen,
/// fuer [`FIRST_CHAR`]..=[`LAST_CHAR`].
#[rustfmt::skip]
const GLYPHS: [[u8; INK_COLUMNS as usize]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], // ' '
    [0x00, 0x00, 0x5F, 0x00, 0x00], // '!'
    [0x00, 0x07, 0x00, 0x07, 0x00], // '"'
    [0x14, 0x7F, 0x14, 0x7F, 0x14], // '#'
    [0x24, 0x2A, 0x7F, 0x2A, 0x12], // '$'
    [0x23, 0x13, 0x08, 0x64, 0x62], // '%'
    [0x36, 0x49, 0x55, 0x22, 0x50], // '&'
    [0x00, 0x05, 0x03, 0x00, 0x00], // '\''
    [0x00, 0x1C, 0x22, 0x41, 0x00], // '('
    [0x00, 0x41, 0x22, 0x1C, 0x00], // ')'
    [0x14, 0x08, 0x3E, 0x08, 0x14], // '*'
    [0x08, 0x08, 0x3E, 0x08, 0x08], // '+'
    [0x00, 0x50, 0x30, 0x00, 0x00], // ','
    [0x08, 0x08, 0x08, 0x08, 0x08], // '-'
    [0x00, 0x60, 0x60, 0x00, 0x00], // '.'
    [0x20, 0x10, 0x08, 0x04, 0x02], // '/'
    [0x3E, 0x51, 0x49, 0x45, 0x3E], // '0'
    [0x00, 0x42, 0x7F, 0x40, 0x00], // '1'
    [0x42, 0x61, 0x51, 0x49, 0x46], // '2'
    [0x21, 0x41, 0x45, 0x4B, 0x31], // '3'
    [0x18, 0x14, 0x12, 0x7F, 0x10], // '4'
    [0x27, 0x45, 0x45, 0x45, 0x39], // '5'
    [0x3C, 0x4A, 0x49, 0x49, 0x30], // '6'
    [0x01, 0x71, 0x09, 0x05, 0x03], // '7'
    [0x36, 0x49, 0x49, 0x49, 0x36], // '8'
    [0x06, 0x49, 0x49, 0x29, 0x1E], // '9'
    [0x00, 0x36, 0x36, 0x00, 0x00], // ':'
    [0x00, 0x56, 0x36, 0x00, 0x00], // ';'
    [0x00, 0x08, 0x14, 0x22, 0x41], // '<'
    [0x14, 0x14, 0x14, 0x14, 0x14], // '='
    [0x41, 0x22, 0x14, 0x08, 0x00], // '>'
    [0x02, 0x01, 0x51, 0x09, 0x06], // '?'
    [0x32, 0x49, 0x79, 0x41, 0x3E], // '@'
    [0x7E, 0x11, 0x11, 0x11, 0x7E], // 'A'
    [0x7F, 0x49, 0x49, 0x49, 0x36], // 'B'
    [0x3E, 0x41, 0x41, 0x41, 0x22], // 'C'
    [0x7F, 0x41, 0x41, 0x22, 0x1C], // 'D'
    [0x7F, 0x49, 0x49, 0x49, 0x41], // 'E'
    [0x7F, 0x09, 0x09, 0x09, 0x01], // 'F'
    [0x3E, 0x41, 0x49, 0x49, 0x7A], // 'G'
    [0x7F, 0x08, 0x08, 0x08, 0x7F], // 'H'
    [0x00, 0x41, 0x7F, 0x41, 0x00], // 'I'
    [0x20, 0x40, 0x41, 0x3F, 0x01], // 'J'
    [0x7F, 0x08, 0x14, 0x22, 0x41], // 'K'
    [0x7F, 0x40, 0x40, 0x40, 0x40], // 'L'
    [0x7F, 0x02, 0x0C, 0x02, 0x7F], // 'M'
    [0x7F, 0x04, 0x08, 0x10, 0x7F], // 'N'
    [0x3E, 0x41, 0x41, 0x41, 0x3E], // 'O'
    [0x7F, 0x09, 0x09, 0x09, 0x06], // 'P'
    [0x3E, 0x41, 0x51, 0x21, 0x5E], // 'Q'
    [0x7F, 0x09, 0x19, 0x29, 0x46], // 'R'
    [0x46, 0x49, 0x49, 0x49, 0x31], // 'S'
    [0x01, 0x01, 0x7F, 0x01, 0x01], // 'T'
    [0x3F, 0x40, 0x40, 0x40, 0x3F], // 'U'
    [0x1F, 0x20, 0x40, 0x20, 0x1F], // 'V'
    [0x3F, 0x40, 0x38, 0x40, 0x3F], // 'W'
    [0x63, 0x14, 0x08, 0x14, 0x63], // 'X'
    [0x07, 0x08, 0x70, 0x08, 0x07], // 'Y'
    [0x61, 0x51, 0x49, 0x45, 0x43], // 'Z'
    [0x00, 0x7F, 0x41, 0x41, 0x00], // '['
    [0x02, 0x04, 0x08, 0x10, 0x20], // '\\'
    [0x00, 0x41, 0x41, 0x7F, 0x00], // ']'
    [0x04, 0x02, 0x01, 0x02, 0x04], // '^'
    [0x40, 0x40, 0x40, 0x40, 0x40], // '_'
    [0x00, 0x01, 0x02, 0x04, 0x00], // '`'
    [0x20, 0x54, 0x54, 0x54, 0x78], // 'a'
    [0x7F, 0x48, 0x44, 0x44, 0x38], // 'b'
    [0x38, 0x44, 0x44, 0x44, 0x20], // 'c'
    [0x38, 0x44, 0x44, 0x48, 0x7F], // 'd'
    [0x38, 0x54, 0x54, 0x54, 0x18], // 'e'
    [0x08, 0x7E, 0x09, 0x01, 0x02], // 'f'
    [0x0C, 0x52, 0x52, 0x52, 0x3E], // 'g'
    [0x7F, 0x08, 0x04, 0x04, 0x78], // 'h'
    [0x00, 0x44, 0x7D, 0x40, 0x00], // 'i'
    [0x20, 0x40, 0x44, 0x3D, 0x00], // 'j'
    [0x7F, 0x10, 0x28, 0x44, 0x00], // 'k'
    [0x00, 0x41, 0x7F, 0x40, 0x00], // 'l'
    [0x7C, 0x04, 0x18, 0x04, 0x78], // 'm'
    [0x7C, 0x08, 0x04, 0x04, 0x78], // 'n'
    [0x38, 0x44, 0x44, 0x44, 0x38], // 'o'
    [0x7C, 0x14, 0x14, 0x14, 0x08], // 'p'
    [0x08, 0x14, 0x14, 0x18, 0x7C], // 'q'
    [0x7C, 0x08, 0x04, 0x04, 0x08], // 'r'
    [0x48, 0x54, 0x54, 0x54, 0x20], // 's'
    [0x04, 0x3F, 0x44, 0x40, 0x20], // 't'
    [0x3C, 0x40, 0x40, 0x20, 0x7C], // 'u'
    [0x1C, 0x20, 0x40, 0x20, 0x1C], // 'v'
    [0x3C, 0x40, 0x30, 0x40, 0x3C], // 'w'
    [0x44, 0x28, 0x10, 0x28, 0x44], // 'x'
    [0x0C, 0x50, 0x50, 0x50, 0x3C], // 'y'
    [0x44, 0x64, 0x54, 0x4C, 0x44], // 'z'
    [0x00, 0x08, 0x36, 0x41, 0x00], // '{'
    [0x00, 0x00, 0x7F, 0x00, 0x00], // '|'
    [0x00, 0x41, 0x36, 0x08, 0x00], // '}'
    [0x08, 0x04, 0x08, 0x10, 0x08], // '~'
];

/// Schwarze/weisse Rastermaske eines einzelnen Zeichens.
///
/// Kanaele: alpha als `u8` (0 oder 255 — fuer das MVP ist keine
/// Graustufenumsetzung noetig). Die Daten liegen row-major vor, die
/// Laenge betraegt `width * height`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphBitmap {
    /// Breite in Pixeln.
    pub width: u32,
    /// Hoehe in Pixeln.
    pub height: u32,
    /// Row-major, Laenge = `width * height`, 0 = transparent, 255 = deckend.
    pub alpha: Vec<u8>,
}

impl GlyphBitmap {
    /// Leere Glyphe (Breite/Hoehe 0) fuer Zeichen, die es nicht gibt.
    pub const fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            alpha: Vec::new(),
        }
    }

    /// `true`, wenn die Glyphe keine sichtbaren Pixel hat.
    ///
    /// Das gilt fuer [`GlyphBitmap::empty`] ebenso wie fuer eine gerasterte
    /// Glyphe, die vollstaendig aus transparenten Pixeln besteht
    /// (etwa das Leerzeichen).
    pub fn is_empty(&self) -> bool {
        if self.alpha.is_empty() {
            return true;
        }
        !self.alpha.iter().any(|&a| a != 0)
    }

    /// Alpha eines Pixels, 0 wenn ausserhalb (kein Panic).
    pub fn alpha_at(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        // width * height kann im Extremfall ueberlaufen; mit u64 gerechnet
        // ist der Index immer sicher.
        let index = u64::from(y) * u64::from(self.width) + u64::from(x);
        match usize::try_from(index) {
            Ok(i) => self.alpha.get(i).copied().unwrap_or(0),
            Err(_) => 0,
        }
    }
}

/// Eingebettete Bitmap-Schrift. Keine externen Dateien.
///
/// Die Schrift ist Monospace und deckt das druckbare ASCII
/// (U+0020..=U+007E) ab. Alle Instanzen sind funktional gleichwertig;
/// `builtin()` ist der einzige Konstruktor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitmapFont {
    /// Reserviert fuer spaetere Erweiterungen (z. B. weitere Schnitte).
    /// Die Daten liegen bewusst als Konstanten in diesem Modul.
    _private: (),
}

impl BitmapFont {
    /// Die eingebaute Standardschrift des Projekts.
    ///
    /// Arbeitet vollstaendig im Speicher: kein Dateisystem, keine
    /// Konfiguration, keine Abhaengigkeit von der Umgebung. Das Ergebnis
    /// ist bei jedem Aufruf identisch.
    pub fn builtin() -> BitmapFont {
        BitmapFont { _private: () }
    }

    /// `true`, wenn die Schrift eine Glyphe fuer `ch` enthaelt.
    pub fn has_glyph(&self, ch: char) -> bool {
        base_index(ch).is_some()
    }

    /// Rastert EIN Zeichen auf die angeforderte Schriftgroesse.
    ///
    /// # Skalierungsregel
    ///
    /// Die eingebaute 5x7-Basisglyphe wird nearest-neighbour auf
    /// `font_size` Pixel Zeilenhoehe gezogen. Mit `s = font_size / 7`
    /// belegt die Basespalte `c` im Ziel die Pixel
    /// `[round(c * s), round((c + 1) * s))`. Beide Achsen werden auf
    /// mindestens [`MIN_SIDE`] (1) Pixel gerundet, sodass auch sehr kleine
    /// Schriftgroessen eine sichtbare 1x1-Glyphe ergeben und nie eine
    /// 0x0-Glyphe entsteht.
    ///
    /// Zurueckgegeben wird die **komplette Zeichenzelle** aus
    /// [`CELL_COLUMNS`] x `round(font_size)` Pixeln, inklusive der
    /// transparenten Abstandsspalte. Genau diese Breite liefert
    /// [`BitmapFont::advance`], damit Text weder klafft noch ueberlappt.
    ///
    /// # Platzhalter fuer unbekannte Zeichen (TASK_0017)
    ///
    /// Ein Zeichen ausserhalb U+0020..=U+007E erhaelt einen sichtbaren
    /// **.notdef**-Platzhalter: ein Rechteck-Rahmen in der vollen
    /// Zellbreite, mit leerer Mitte. Es gibt hier NIE eine 0x0-Glyphe.
    ///
    /// Warum hier und nicht im Rasterizer: die Breite des Platzhalters
    /// ist die Zellbreite, und die kennt nur diese eine Stelle. Staende
    /// sie auch in `narsil-raster`, waeren es zwei Orte fuer eine Zahl —
    /// genau der Fehler, an dem `DisplayList::bounds()` mit 0.6 em
    /// gescheitert ist, waehrend die Schrift 0,875 em braucht.
    ///
    /// Warum sichtbar und nicht "Breite abziehen": ein Browser, der
    /// Zeichen verschluckt, ohne es zu sagen, ist schlechter als einer,
    /// der ehrlich ein Kästchen zeigt. `text "Grüße"` liefert jetzt
    /// "Gr[ü]ß" mit zwei Kästen statt "Gr  e" mit zwei Löchern.
    ///
    /// Ausnahme: das Leerzeichen und der Tabulator sind per Definition
    /// unsichtbar. Sie sind IN der Schrift, deshalb ist ihr Platzhalter
    /// der Leerraum aus der Tabelle, nicht der Rahmen.
    ///
    /// # Rueckgabe
    ///
    /// `None`, wenn `font_size` nicht endlich oder nicht positiv ist
    /// (dann gibt es keine Zelle, in der ein Platzhalter Platz haette).
    ///
    /// Fuer `ch` ausserhalb der Schrift wird es `None` sein: `None`
    /// bedeutet hier "keine gueltige Schriftgroesse". Ob ein Zeichen
    /// bekannt ist, steht in [`BitmapFont::has_glyph`].
    pub fn rasterize_glyph(&self, ch: char, font_size: f32) -> Option<GlyphBitmap> {
        let scale = cell_scale(font_size)?;
        let width = scaled_extent(CELL_COLUMNS, scale);
        let height = scaled_extent(BASE_ROWS, scale);

        let mut alpha = vec![0u8; (width as usize) * (height as usize)];

        // Unbekanntes Zeichen: sichtbarer .notdef-Rahmen. Die Tabelle
        // wird nicht angefasst, die Zelle ist gleich gross wie sonst.
        let columns = match base_index(ch) {
            Some(i) => i,
            None => {
                draw_notdef(&mut alpha, width, height);
                return Some(GlyphBitmap {
                    width,
                    height,
                    alpha,
                });
            }
        };
        let source = &GLYPHS[columns];

        for c in 0..INK_COLUMNS {
            let x0 = scaled_offset(c, scale);
            let x1 = scaled_offset(c + 1, scale);
            let bits = source[c as usize];
            if bits == 0 {
                continue;
            }
            for x in x0..x1 {
                for r in 0..BASE_ROWS {
                    // Bit 0 = oberste Zeile.
                    if bits & (1 << r) == 0 {
                        continue;
                    }
                    let y0 = scaled_offset(r, scale) as usize;
                    let y1 = scaled_offset(r + 1, scale) as usize;
                    for y in y0..y1 {
                        alpha[y * width as usize + x as usize] = 255;
                    }
                }
            }
        }

        Some(GlyphBitmap {
            width,
            height,
            alpha,
        })
    }

    /// Fortschritt dieses Zeichens in Pixeln (Vorschub).
    ///
    /// Gleich der Breite der mit [`BitmapFont::rasterize_glyph`] erzeugten
    /// Glyphe, damit aufeinanderfolgende Glyphen lueckenlos aneinander
    /// anschliessen. `None` wenn Zeichen nicht enthalten oder
    /// `font_size` ungueltig ist.
    ///
    /// # DER VORSCHUB IST ZEICHENUNABHAENGIG (TASK_0017)
    ///
    /// Alle Zeichen haben denselben Vorschub: `round(font_size * 6/7)`.
    /// Auch unbekannte Zeichen und der Platzhalter. Gemessen bei
    /// `font_size` 16: `'a'` = 14, `'ü'` = 14, `'€'` = 14, `'😀'` = 14.
    ///
    /// Das ist Absicht und keine Vereinfachung. Ein Platzhalter mit
    /// eigenem Vorschub wuerde die Breite des Zeichens veraendern, das
    /// er ersetzt — das waere ein stiller Layout-Effekt, den niemand
    /// sieht. Die Schrift ist Monospace, und Monospace heisst: ein
    /// Zeichen belegt eine Zelle, unabhaengig davon, was da steht.
    ///
    /// `None` bedeutet ausschliesslich "die Schrift kennt dieses Zeichen
    /// nicht" — die Frage "wie sieht es aus" beantwortet
    /// `rasterize_glyph`, das dafuer einen Platzhalter liefert. Die
    /// Asymmetrie ist beabsichtigt: `advance` ist eine Aussage ueber die
    /// Tabelle, `rasterize_glyph` ueber die Darstellung.
    pub fn advance(&self, ch: char, font_size: f32) -> Option<f32> {
        base_index(ch)?;
        let width = cell_width(font_size)?;
        Some(width as f32)
    }

    /// Vorschub fuer jedes Zeichen, auch fuer solche ausserhalb der
    /// Schrift.
    ///
    /// Fehlende Glyphen erhalten den vollen Zellvorschub, damit der
    /// nachfolgende Text nicht zusammenrueckt. Wird von `measure` und
    /// `layout_text` benutzt, damit beide exakt dieselbe Breite rechnen.
    pub(crate) fn advance_or_fallback(&self, ch: char, font_size: f32) -> f32 {
        match self.advance(ch, font_size) {
            Some(a) => a,
            // Ungueltige Schriftgroesse kann hier nicht auftreten, weil
            // beide Aufrufer vorher validieren; die 1.0 ist nur ein
            // sicherer Ausgangswert.
            None => cell_width(font_size).map_or(1.0, |w| w as f32),
        }
    }
}

/// Index in [`GLYPHS`], falls `ch` zur Schrift gehoert.
fn base_index(ch: char) -> Option<usize> {
    if ch < FIRST_CHAR || ch > LAST_CHAR {
        return None;
    }
    // Der Abstand zweier ASCII-Codes entspricht genau dem Index.
    Some(ch as usize - FIRST_CHAR as usize)
}

/// Skalierungsfaktor fuer `font_size`, `None` wenn ungueltig.
fn cell_scale(font_size: f32) -> Option<f32> {
    if !is_valid_font_size(font_size) {
        return None;
    }
    Some(font_size / BASE_ROWS as f32)
}

/// Gerundete Zellbreite in Pixeln, mindestens 1. `None` wenn ungueltig.
fn cell_width(font_size: f32) -> Option<u32> {
    cell_scale(font_size).map(|s| scaled_extent(CELL_COLUMNS, s))
}

/// Pixelgrenze des Basisindexes `index` nach der Skalierung.
///
/// Weil die Grenzen ganzzahlig gerundet sind, zerlegen aufeinanderfolgende
/// Aufrufe das Ziel in lueckenlose, ueberlappungsfreie Abschnitte.
///
/// Wichtig: Der Offset wird **nicht** auf [`MIN_SIDE`] gehoben. Ein
/// verschobener Start liesse die erste Spalte oder Zeile leer. Die
/// Untergrenze sichert allein [`scaled_extent`], damit nie eine 0x0-Glyphe
/// entsteht.
fn scaled_offset(index: u32, scale: f32) -> u32 {
    let raw = (index as f32 * scale).round();
    // `index >= 0` und `scale > 0`, also ist `raw` immer endlich und >= 0.
    raw as u32
}

/// Gesamtausdehnung von `count` Basiseinheiten, mindestens [`MIN_SIDE`].
///
/// Hier — und nur hier — greift die Untergrenze: bei winzigen
/// Schriftgroessen wuerden alle Offsets auf 0 fallen und die Glyphe waere
/// 0x0.
fn scaled_extent(count: u32, scale: f32) -> u32 {
    let extent = scaled_offset(count, scale);
    if extent < MIN_SIDE { MIN_SIDE } else { extent }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_is_deterministic() {
        // Gleiche Eingabe -> gleiche Glyphe.
        let f = BitmapFont::builtin();
        for ch in ['A', 'z', '0', '~', ' '] {
            let a = f.rasterize_glyph(ch, 16.0);
            let b = f.rasterize_glyph(ch, 16.0);
            assert_eq!(a, b);
        }
    }

    #[test]
    fn glyph_table_covers_printable_ascii() {
        for code in 0x20u8..=0x7E {
            let ch = char::from(code);
            assert!(BitmapFont::builtin().has_glyph(ch), "fehlt: {ch:?}");
        }
        assert!(!BitmapFont::builtin().has_glyph('\u{1F600}'));
    }

    #[test]
    fn glyph_data_uses_only_seven_rows() {
        // Bit 7 (und hoeher) darf nie gesetzt sein, sonst waere die
        // Basisglyphe hoeher als 7 Zeilen.
        for (i, glyph) in GLYPHS.iter().enumerate() {
            for &byte in glyph {
                assert_eq!(byte & 0x80, 0, "Zeichenindex {i} verletzt 7 Zeilen");
            }
        }
    }

    #[test]
    fn every_visible_ascii_glyph_has_ink() {
        let f = BitmapFont::builtin();
        for code in 0x21u8..=0x7E {
            let ch = char::from(code);
            let g = f.rasterize_glyph(ch, 16.0).expect("Rasterung fehlt");
            assert!(!g.is_empty(), "keine Tinte fuer {ch:?}");
        }
    }

    #[test]
    fn space_has_advance_but_no_ink() {
        let f = BitmapFont::builtin();
        let g = f
            .rasterize_glyph(' ', 16.0)
            .expect("Space muss belegt sein");
        assert!(g.is_empty());
        assert!(f.advance(' ', 16.0).is_some_and(|a| a > 0.0));
    }

    #[test]
    fn rasterization_never_produces_empty_bitmap() {
        let f = BitmapFont::builtin();
        for code in 0x20u8..=0x7E {
            let g = f.rasterize_glyph(char::from(code), 1.0).expect("fehlt");
            assert!(g.width >= 1 && g.height >= 1);
        }
    }

    #[test]
    fn alpha_is_binary() {
        let f = BitmapFont::builtin();
        let g = f.rasterize_glyph('A', 16.0).expect("fehlt");
        assert!(g.alpha.iter().all(|&a| a == 0 || a == 255));
    }

    #[test]
    fn advance_matches_rasterized_width() {
        let f = BitmapFont::builtin();
        for size in [1.0, 4.0, 7.0, 11.5, 16.0, 33.0] {
            for code in 0x20u8..=0x7E {
                let ch = char::from(code);
                let a = f.advance(ch, size).expect("fehlt");
                let w = f.rasterize_glyph(ch, size).expect("fehlt").width as f32;
                assert!(
                    (a - w).abs() < 1.0,
                    "{ch:?} bei {size}: advance {a} vs breite {w}"
                );
                assert!((a - w).abs() < f32::EPSILON, "muss exakt gleich sein");
            }
        }
    }

    #[test]
    fn columns_and_rows_tile_without_gaps() {
        // Die Skalierungsgrenzen muessen lueckenlos und exakt die
        // Zellenausdehnung ergeben: die Offsets sind monoton steigend, und
        // die Gesamtausdehnung ist der letzte Offset — mit einer Ausnahme:
        // unterhalb einer Basiszeile fallen alle Offsets auf 0, erst
        // `scaled_extent` hebt die Zelle dann auf 1 Pixel.
        for size in [0.1, 1.0, 3.0, 7.0, 16.0, 21.0, 100.0] {
            let s = cell_scale(size).expect("gueltig");
            assert_eq!(scaled_offset(0, s), 0, "Start muss 0 sein, nicht 1");

            for count in [CELL_COLUMNS, BASE_ROWS] {
                let mut previous = 0;
                for index in 1..=count {
                    let offset = scaled_offset(index, s);
                    assert!(
                        offset >= previous,
                        "Offset {index}/{count} bei {size}: {offset} < {previous}"
                    );
                    previous = offset;
                }
                // Gesamtausdehnung = letzter Offset, mindestens MIN_SIDE.
                assert_eq!(scaled_extent(count, s), previous.max(MIN_SIDE));
            }
            assert_eq!(
                scaled_extent(CELL_COLUMNS, s),
                cell_width(size).expect("ok")
            );
        }
    }

    #[test]
    fn base_size_maps_one_to_one() {
        // Bei Schriftgroesse = Basishoehe ist der Skalierungsfaktor 1: die
        // Glyphe ist exakt 6x7 Pixel gross.
        let s = cell_scale(BASE_ROWS as f32).expect("gueltig");
        assert_eq!(s, 1.0);
        for index in 0..=CELL_COLUMNS {
            assert_eq!(scaled_offset(index, s), index);
        }
        let g = BitmapFont::builtin()
            .rasterize_glyph('A', BASE_ROWS as f32)
            .expect("fehlt");
        assert_eq!(g.width, CELL_COLUMNS);
        assert_eq!(g.height, BASE_ROWS);
    }

    #[test]
    fn tiny_sizes_never_produce_a_zero_sized_cell() {
        // Bei sehr kleinen Groessen fallen alle Offsets auf 0. Die
        // Untergrenze in `scaled_extent` verhindert eine 0x0-Glyphe.
        for size in [0.01, 0.1, 0.3, 0.6] {
            let s = cell_scale(size).expect("gueltig");
            assert_eq!(scaled_extent(CELL_COLUMNS, s), MIN_SIDE);
            assert_eq!(scaled_extent(BASE_ROWS, s), MIN_SIDE);
        }
    }

    #[test]
    fn max_font_size_ist_4096() {
        // Diese Zahl wird hier AUSGESCHRIEBEN und nicht ueber die
        // Konstante gestellt.
        //
        // GEMESSEN 29.09.2026 mit `werkzeuge/gegenbeweis.sh`:
        //     BLIND  MAX_FONT_SIZE 4096 -> 4097   0/111
        // Null von 111 Tests dieser Crate werden rot, wenn man die Zahl
        // aendert -- und diese Crate besitzt sie. Der Grund ist nicht
        // Vergessen: `oversized_font_size_is_rejected` unten benutzt die
        // Konstante in ihren EINGABEN (`MAX_FONT_SIZE + 1.0`), also ist
        // 4098 genauso gut abgelehnt wie 4097. Dieser Test prueft die
        // Beziehung der Konstante zu sich selbst, nicht ihren Wert.
        //
        // Einzige Stelle im Workspace, die 4096 ausschrieb, war
        // crates/narsil-parser/tests/fontsizes.rs:34 -- eine andere Crate.
        // Das ist die Umkehrung von FERTIG_0064: dort gehoerte der Test
        // zur falschen Crate, hier gehoert er zur richtigen und prueft
        // trotzdem nichts.
        assert_eq!(
            MAX_FONT_SIZE, 4096.0,
            "die Grenze ist eine Entscheidung, kein Zufall -- wer sie aendert, \
             aendert diese drei Zeilen mit"
        );
        assert!(is_valid_font_size(4096.0), "die Grenze selbst ist erlaubt");
        assert!(
            !is_valid_font_size(4097.0),
            "die naechste Zahl DARUEBER nicht"
        );
    }

    #[test]
    fn oversized_font_size_is_rejected() {
        // Ohne diese Grenze wuerde die Rasterung eineriesige Allokation
        // anfordern und panicen.
        let f = BitmapFont::builtin();
        for size in [MAX_FONT_SIZE + 1.0, 1e6, f32::MAX, 1e30] {
            assert!(!is_valid_font_size(size), "{size} muss abgelehnt werden");
            assert!(f.rasterize_glyph('A', size).is_none());
            assert!(f.advance('A', size).is_none());
        }
        assert!(is_valid_font_size(MAX_FONT_SIZE));
    }

    #[test]
    fn oversized_glyph_never_panics() {
        // Das Sicherheitsnetz gegen den Capacity-Overflow: der Wert, der
        // vorher panicte, muss jetzt sauber zu None fuehren.
        let f = BitmapFont::builtin();
        assert!(f.rasterize_glyph('A', f32::MAX).is_none());
        assert!(f.rasterize_glyph('A', f32::INFINITY).is_none());
        assert!(f.rasterize_glyph('A', 1e20).is_none());
    }

    #[test]
    fn invalid_font_size_yields_none() {
        let f = BitmapFont::builtin();
        for size in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(f.rasterize_glyph('A', size).is_none());
            assert!(f.advance('A', size).is_none());
        }
    }

    #[test]
    fn out_of_range_chars_get_a_notdef_placeholder() {
        // TASK_0017 hat das Verhalten geaendert: ein Zeichen, das die
        // Schrift nicht kennt, erhaelt jetzt einen sichtbaren .notdef-
        // Platzhalter statt einer 0x0-Glyphe. `has_glyph` beantwortet
        // weiterhin die Frage "kennt die Schrift das Zeichen?".
        let f = BitmapFont::builtin();
        for ch in ['\u{00DF}', 'é', '\u{1F389}', 'ü'] {
            assert!(!f.has_glyph(ch), "{ch:?} ist nicht in der Schrift");

            // Vorher None, jetzt Some mit voller Zellbreite.
            let g = f
                .rasterize_glyph(ch, 16.0)
                .expect("TASK_0017: ein Platzhalter ist besser als ein Loch");
            assert!(g.width > 0, "{ch:?} hat Breite 0");
            assert!(!g.is_empty(), "{ch:?} ist unsichtbar");

            // Der Vorschub bleibt None — die Frage "ist das ein
            // bekanntes Zeichen" ist damit weiterhin beantwortbar.
            assert!(
                f.advance(ch, 16.0).is_none(),
                "advance() bleibt None fuer unbekannte Zeichen"
            );
        }
    }

    #[test]
    fn ungueltige_schriftgroesse_bleibt_none() {
        // `None` bedeutet jetzt ausschliesslich "keine gueltige
        // Schriftgroesse" — nicht "Zeichen unbekannt".
        let f = BitmapFont::builtin();
        for ch in ['A', 'ü'] {
            for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
                assert!(
                    f.rasterize_glyph(ch, size).is_none(),
                    "'{ch}' bei {size} muss None liefern"
                );
            }
        }
    }

    #[test]
    fn notdef_ist_ein_rahmen_mit_leerer_mitte() {
        let f = BitmapFont::builtin();
        let g = f.rasterize_glyph('ü', 24.0).expect("ok");
        let w = g.width;
        let h = g.height;
        assert!(w >= 2 && h >= 2, "bei 24px ist die Zelle gross genug");
        // Mitte frei
        assert_eq!(g.alpha_at(w / 2, h / 2), 0, "Mitte ist leer");
        // Rand gedeckt
        assert_eq!(g.alpha_at(0, 0), 255);
        assert_eq!(g.alpha_at(w - 1, 0), 255);
        assert_eq!(g.alpha_at(0, h - 1), 255);
        assert_eq!(g.alpha_at(w - 1, h - 1), 255);
    }

    #[test]
    fn notdef_ist_ein_punkt_in_ein_pixelzelle() {
        // Bei kleinster Schriftgroesse ist ein Rahmen unsichtbar —
        // ein einzelnes Pixel ist das kleinste Sichtbare.
        let f = BitmapFont::builtin();
        let g = f.rasterize_glyph('ü', 1.0).expect("ok");
        assert_eq!((g.width, g.height), (1, 1));
        assert_eq!(g.alpha_at(0, 0), 255, "1x1 wird zu einem Punkt");
    }

    #[test]
    fn empty_bitmap_is_empty_and_bounds_safe() {
        let g = GlyphBitmap::empty();
        assert!(g.is_empty());
        assert_eq!(g.alpha_at(0, 0), 0);
        assert_eq!(g.alpha_at(u32::MAX, u32::MAX), 0);
        assert!(g.alpha.is_empty());
    }

    #[test]
    fn alpha_at_outside_returns_zero() {
        let f = BitmapFont::builtin();
        let g = f.rasterize_glyph('A', 16.0).expect("fehlt");
        assert_eq!(g.alpha_at(g.width, 0), 0);
        assert_eq!(g.alpha_at(0, g.height), 0);
        assert_eq!(g.alpha_at(g.width + 100, g.height + 100), 0);
    }

    #[test]
    fn alpha_length_matches_area() {
        let f = BitmapFont::builtin();
        for size in [1.0, 7.0, 16.0, 40.0] {
            let g = f.rasterize_glyph('M', size).expect("fehlt");
            assert_eq!(g.alpha.len(), (g.width * g.height) as usize);
        }
    }

    #[test]
    fn advance_scales_with_size() {
        let f = BitmapFont::builtin();
        let small = f.advance('A', 7.0).expect("fehlt");
        let large = f.advance('A', 14.0).expect("fehlt");
        assert!(large > small);
        // 14/7 = 2 -> exakt doppelter Vorschub.
        assert!((large - small * 2.0).abs() < 0.5);
    }

    #[test]
    fn tiny_font_size_is_clamped_to_one_pixel() {
        let f = BitmapFont::builtin();
        let g = f.rasterize_glyph('A', 0.1).expect("muss noch klappen");
        assert_eq!(g.width, 1);
        assert_eq!(g.height, 1);
        assert_eq!(f.advance('A', 0.1), Some(1.0));
    }
}
