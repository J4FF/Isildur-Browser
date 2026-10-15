//! Schriftrasterung und Textmessung fuer die Display-List.
//!
//! Dieses Crate uebersetzt Zeichenketten in Glyphen-Bitmaps, misst Text und
//! positioniert Glyphen auf einer Textlinie. Es kennt **keinen** Framebuffer
//! und **keine** Display-List: gezeichnet wird hier nicht. Die Integration —
//! der Rasterizer malt diese Glyphen spaeter in seinen Puffer — gehoert zu
//! `narsil-raster`.
//!
//! # Schnellstart
//!
//! ```
//! use narsil_text::{BitmapFont, layout_text, measure};
//!
//! let font = BitmapFont::builtin();
//! let metrics = measure(&font, "Hello", 16.0).expect("gueltige Eingabe");
//! assert!(metrics.width > 0.0);
//!
//! let glyphs = layout_text(&font, "Hello", 16.0, narsil_core::geometry::Point::new(0.0, 0.0))
//!     .expect("gueltige Eingabe");
//! assert_eq!(glyphs.len(), 5);
//! ```
//!
//! # Eigenschaften
//!
//! * Die Schrift ist vollstaendig eingebettet (5x7-ASCII, U+0020..=U+007E).
//!   Keine Dateien, keine Systemfonts, keine externen Crates.
//! * Die Schrift ist Monospace. `advance` liefert exakt die Breite der
//!   gerasterten Glyphe, damit Text weder klafft noch ueberlappt.
//! * Keine Panics: ungueltige Eingaben liefern [`TextError`].

// GEMESSEN 03.10.2026 (A27 Stufe 2): `cff` liest PostScript-Charstrings,
// damit die 36 CFF-Systemschriften dieser Maschine Umrisse bekommen.
// `rasterize_outline` liest `glyf`, und eine CFF-Schrift hat statt dessen
// `CFF `. GEMESSEN der Umfang: 35 CFF1 (keine CID), 1 CFF2, 14 Operatoren.
pub mod arabisch;
pub mod bidi;
pub mod cff;
pub mod devanagari;
/// RUNDE 3 S4: Farbige Glyphen aus CBDT/CBLC.
pub mod emoji;
mod font;
pub mod graphem;
/// Release 08.10.2026: kombinierende Zeichen zusammensetzen (NFC, lateinisch).
pub mod komposition;
pub mod hebraeisch;
mod kerning;
mod linebreak;
mod metrics;
pub mod raster;
mod shaping;
mod stapel;
mod system;
pub mod truetype;
mod tt_metrics;
pub mod ueberlauf;
pub mod umbruch;
pub mod unicode_daten;
pub mod woff2;
/// Release 09.10.2026: angemeldete Webschriften (`@font-face`).
pub mod webschrift;

pub use font::{
    BASE_ROWS, BitmapFont, CELL_COLUMNS, GlyphBitmap, MAX_FONT_SIZE, is_valid_font_size,
};
pub use kerning::{KernPaare, MAX_PAARE, kern_paare_lesen};
pub use linebreak::{
    BreakOpportunity, LineBreakClass, MAX_MESSUNGEN, MAX_ZEILEN, can_break_before,
    find_break_opportunities, line_break_class, wrap_text, wrap_text_mit,
};
pub use metrics::{ASCENT_RATIO, TextMetrics, measure, zeilenhoehe_normal};
pub use raster::{GlyphCache, rasterize_outline};
pub use shaping::{
    MAX_LAUFWEITE, MAX_TAB_SIZE, MAX_TEXTLAENGE, MIN_LAUFWEITE, PositionedGlyph, TAB_SIZE_STANDARD,
    TRENNSTRICH, Transform, WEICHES_TRENNZEICHEN, ZeilenLayout, laufweite, layout_text,
    layout_text_mit_breite, layout_text_mit_laufweite, layout_text_mit_tabs,
    layout_text_mit_tabs_mit_breite, layout_zeile_mit_shy, layout_zeile_mit_shy_mit_breite,
    text_transform,
};
mod familie;
pub use familie::{datei_fuer_familie, datei_fuer_familie_mit_gewicht, stapel_fuer_familie, systemschrift_wurzeln, systemschriften};
pub use stapel::{
    BreitenTabelle, GlyphQuelle, Glyphenlauf, MAX_BREITEN_ZEICHEN, MAX_GESUCHTE_DATEIEN,
    MAX_LAEUFE, MAX_SCHRIFTEN, Schriftstapel, StapelFehler, Stapelschrift, Stilauswahl,
    ersatz_fuer, ersatzschriften_waehlen, familie_waehlen, generische_familie, gewicht_ist_fett,
    ist_ersatzschrift, schnitte_aus_css, stil_waehlen, system_stapel,
};
pub use system::{
    FontCategory, SchriftWunsch, SystemFont, find_best_font, find_system_fonts, load_system_font,
    waehle_schrift,
};
pub use truetype::{
    BoundingBox, Contour, GlyphMetrics, GlyphOutline, Point, TrueTypeFont, TtError,
};
pub use tt_metrics::{TtMetrics, TtTextMetrics, measure_tt, measure_with_system_font};

/// Fehler der Textverarbeitung.
///
/// Agent3 matcht auf diese Varianten, daher werden sie nicht umbenannt.
#[derive(Debug, Clone, PartialEq)]
pub enum TextError {
    /// Schriftgroesse war `NaN`, unendlich oder nicht positiv.
    InvalidFontSize(f32),
    /// Der Ursprung war `NaN` oder unendlich.
    ///
    /// GEMESSEN 03.10.2026: mit `origin.x = NaN` kam aus der neuen
    /// Endbreite `NaN` heraus, weil sie `x - origin.x` rechnet. Das ist
    /// nicht nur ein hässlicher Wert: der Aufrufer addiert die Breite zur
    /// Kastenbreite, und ein `NaN` dort macht die ganze Zeile unbrauchbar —
    /// ohne dass irgendwo ein Fehler entsteht.
    ///
    /// Die Glyphenpositionen sind dann ebenfalls `NaN`, also ist es nicht
    /// nur die Breite, die kaputt ist. Deshalb wird der Ursprung hier
    /// abgewiesen statt stillschweigend auf 0 gesetzt.
    UngueltigerUrsprung { x: f32, y: f32 },
    /// Der Text war leer; es gibt nichts zu messen oder zu layouten.
    EmptyText,
    /// Der Text war laenger als die gesetzte Grenze (M3/M4).
    ///
    /// GEMESSEN 03.10.2026 (RUNDE 3 S3): `text-transform: uppercase` legt
    /// eine Kopie an. Ohne Grenze waere ein 100-MB-Text ein Vec von
    /// 400 MB — der Fehler waere nicht im Ergebnis, sondern im Speicher,
    /// und faellt erst auf, wenn der Prozess stirbt.
    ZuLang,
}

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextError::InvalidFontSize(size) => {
                write!(
                    f,
                    "ungueltige Schriftgroesse: {size} (muss endlich und > 0 sein)"
                )
            }
            TextError::EmptyText => write!(f, "leere Zeichenkette"),
            TextError::ZuLang => write!(f, "Text laenger als MAX_TEXTLAENGE"),
            // GEMESSEN 03.10.2026: ein NaN-Ursprung macht jede Glyphen-
            // position unbrauchbar. Es wird abgewiesen, nicht auf 0
            // gesetzt — ein stilles 0 waere eine Erfindung.
            TextError::UngueltigerUrsprung { x, y } => write!(
                f,
                "ungueltiger Ursprung: ({x}, {y}) (beide muessen endlich sein)"
            ),
        }
    }
}

impl std::error::Error for TextError {}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use narsil_core::geometry::Point;

    use super::*;

    fn font() -> BitmapFont {
        BitmapFont::builtin()
    }

    #[test]
    fn error_display_is_meaningful() {
        assert!(!TextError::EmptyText.to_string().is_empty());
        assert!(!TextError::InvalidFontSize(-1.0).to_string().is_empty());
    }

    #[test]
    fn error_is_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(TextError::EmptyText);
        assert_eq!(e.to_string(), TextError::EmptyText.to_string());
    }

    #[test]
    fn constants_are_consistent() {
        assert_eq!(CELL_COLUMNS, 6);
        assert_eq!(BASE_ROWS, 7);
        // Ascent liegt ueber der Grundlinie, darunter bleibt Platz.
        // Compile-Zeit-Pruefung: die Ascent-Rate muss zwischen Grundlinie
        // und Oberkante liegen.
        const { assert!(ASCENT_RATIO > 0.0 && ASCENT_RATIO < 1.0) };
    }

    #[test]
    fn builtin_needs_no_filesystem() {
        // Reines Speicher-Objekt: kopierbar, vergleichbar, ohne I/O.
        let a = font();
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn no_panics_on_hostile_input() {
        let f = font();
        // Muss alles sauber beantworten, ohne zu panicken.
        for text in ["", "\n", "\u{0}", "\u{10FFFF}", "a\u{0}\u{10FFFF}b"] {
            let _ = layout_text(&f, text, 16.0, Point::new(0.0, 0.0));
            let _ = measure(&f, text, 16.0);
        }
        for size in [f32::MIN, f32::MIN_POSITIVE, f32::MAX, 0.0, -0.0, f32::NAN] {
            let _ = layout_text(&f, "x", size, Point::new(0.0, 0.0));
            let _ = measure(&f, "x", size);
        }
    }

    #[test]
    fn negative_zero_is_rejected() {
        assert_eq!(
            measure(&font(), "x", -0.0),
            Err(TextError::InvalidFontSize(-0.0))
        );
    }
}
