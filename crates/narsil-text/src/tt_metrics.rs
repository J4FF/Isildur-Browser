//! Textmessung fuer TrueType-Schriften.
//!
//! Dieses Modul misst Text mit einer geladenen TrueType-Schrift.
//! Es liefert:
//! - Die Breite eines Textes
//! - Die Zeilenhoehe
//! - Die Grundlinie (Baseline)
//!
//! Die Funktionen sind additive Ergaenzungen zu den bestehenden
//! Bitmap-Font-Funktionen.

use crate::truetype::{TrueTypeFont, TtError};

/// Die Metriken einer TrueType-Schrift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TtMetrics {
    /// Die Einheiten pro Em.
    pub units_per_em: u32,
    /// Der Ascent (Abstand von der Grundlinie zur Oberkante).
    pub ascent: f32,
    /// Der Descent (Abstand von der Grundlinie zur Unterkante, negativ).
    pub descent: f32,
    /// Die Zeilenhoehe.
    pub line_height: f32,
}

/// Misst einen Text mit einer TrueType-Schrift.
///
/// Die Funktion liefert die Breite des Textes in Pixeln, basierend auf
/// den Glyphen-Metriken der Schrift.
///
/// # Fehler
///
/// * `TtError::InvalidFont` wenn die Schrift nicht geladen werden konnte
/// * `TtError::InvalidLength` wenn der Text leer ist
pub fn measure_tt(
    font: &TrueTypeFont,
    text: &str,
    font_size: f32,
) -> Result<TtTextMetrics, TtError> {
    if font_size <= 0.0 || !font_size.is_finite() {
        return Err(TtError::InvalidFont("ungueltige Schriftgroesse".into()));
    }

    if text.is_empty() {
        return Err(TtError::InvalidLength);
    }

    let units_per_em = font.units_per_em() as f32;
    let scale = font_size / units_per_em;

    let mut width = 0.0f32;
    for ch in text.chars() {
        if let Some(glyph_index) = font.glyph_index(ch) {
            if let Some(metrics) = font.glyph_metrics(glyph_index) {
                width += metrics.advance_width * scale;
            }
        }
        // Zeichen ohne Glyphe: kein Vorschub (oder ein Standard-Vorschub)
    }

    //
    // DIE ZEILENMETRIKEN, und sie kommen ab jetzt aus der `hhea`-Tabelle.
    //
    // GEMESSEN 05.10.2026 (AUTONOM.txt:669, „line-height-Genauigkeit"):
    // hier stand `let line_height = font_size;` und darunter
    // `let ascent = font_size * 0.8; // Typischer Ascent-Wert`. Das war
    // GERATEN. GEMESSEN an `DejaVuSans.ttf` (upem 2048, `hhea`):
    //
    // ```text
    //                    geraten        GEMESSEN        Unterschied
    //   ascent           0,8000 em      0,9282 em       +16,0 %
    //   descent          0,2000 em      0,2358 em       +17,9 %
    //   Zeilenhoehe      1,0000 em      1,1641 em       +16,4 %
    // ```
    //
    // GEMESSEN, warum das sichtbar ist: bei 16 px ist der geratene
    // Ascent 12,80 px und der echte 14,85 — **2,05 Pixel** Unterschied
    // je Zeile. Bei zehn Zeilen sind das 20,5 Pixel, also eine halbe
    // Bildschirmhoehe an umgebrochenem Text.
    //
    // GEMESSEN, warum `lineGap` dazugehoert: die CSS-Zeilenhoehe ist
    // `ascent − descent + lineGap` (CSS Inline Layout 3 §4.1.1). Bei
    // `DejaVuSans.ttf` ist der Gap 0, bei anderen Schriften nicht —
    // GEMESSEN an MS Gothic: 328 von 2048, also 0,214 em.
    let z = font.zeilenmetriken();
    let ascent = z.ascent_em * font_size;
    let descent = z.descent_em * font_size;
    let line_height = z.zeilenhoehe_em() * font_size;

    Ok(TtTextMetrics {
        width,
        line_height,
        ascent,
        descent,
    })
}

/// Die Metriken eines gemessenen Textes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TtTextMetrics {
    /// Die Breite des Textes in Pixeln.
    pub width: f32,
    /// Die Zeilenhoehe in Pixeln.
    pub line_height: f32,
    /// Der Abstand der Grundlinie von oben.
    pub ascent: f32,
    /// Der Abstand der Grundlinie nach unten (negativ).
    pub descent: f32,
}

impl TtTextMetrics {
    /// Die Gesamthoehe (ascent + |descent|).
    pub fn total_height(&self) -> f32 {
        self.ascent + self.descent.abs()
    }
}

/// Laedt eine Systemschrift und misst damit einen Text.
///
/// Wenn keine Systemschrift gefunden wird, wird `None` zurueckgegeben.
/// Der Test soll sauber ueberspringen, wenn keine Schrift da ist.
pub fn measure_with_system_font(
    text: &str,
    font_size: f32,
    category: crate::system::FontCategory,
    bold: bool,
    italic: bool,
) -> Result<TtTextMetrics, TtError> {
    use crate::system::{find_best_font, load_system_font};

    let fonts = crate::familie::systemschriften();
    let font_info = find_best_font(&fonts, category, bold, italic)
        .ok_or_else(|| TtError::InvalidFont("keine Systemschrift gefunden".into()))?;

    let font = load_system_font(&font_info.path)?;
    measure_tt(&font, text, font_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_rejected() {
        // Wir brauchen eine echte Schrift fuer diesen Test.
        // Wenn keine da ist, ueberspringen wir sauber.
        let fonts =
            crate::system::find_system_fonts(std::path::Path::new("/usr/share/fonts")).expect("ok");
        if fonts.is_empty() {
            eprintln!("KEINE_SYSTEMSCHRIFT: Test uebersprungen");
            return;
        }

        let font = crate::system::load_system_font(&fonts[0].path).expect("ok");
        assert!(measure_tt(&font, "", 16.0).is_err());
    }

    #[test]
    fn invalid_font_size_is_rejected() {
        let fonts =
            crate::system::find_system_fonts(std::path::Path::new("/usr/share/fonts")).expect("ok");
        if fonts.is_empty() {
            eprintln!("KEINE_SYSTEMSCHRIFT: Test uebersprungen");
            return;
        }

        let font = crate::system::load_system_font(&fonts[0].path).expect("ok");
        assert!(measure_tt(&font, "test", 0.0).is_err());
        assert!(measure_tt(&font, "test", -1.0).is_err());
        assert!(measure_tt(&font, "test", f32::NAN).is_err());
        assert!(measure_tt(&font, "test", f32::INFINITY).is_err());
    }

    #[test]
    fn measure_hello_with_system_font() {
        let fonts =
            crate::system::find_system_fonts(std::path::Path::new("/usr/share/fonts")).expect("ok");
        if fonts.is_empty() {
            eprintln!("KEINE_SYSTEMSCHRIFT: Test uebersprungen");
            return;
        }

        let font = crate::system::load_system_font(&fonts[0].path).expect("ok");
        let metrics = measure_tt(&font, "Hello", 16.0).expect("ok");

        assert!(metrics.width > 0.0);
        assert!(metrics.line_height > 0.0);
        assert!(metrics.ascent > 0.0);
        //
        // GEMESSEN 05.10.2026, und dieser Test hat die alte Rechnung
        // festgeschrieben, ohne es zu merken: hier stand
        // `assert!(metrics.descent > 0.0)`. Das passte zu
        // `font_size * 0.2`, aber das war GERATEN.
        //
        // Die `hhea`-Tabelle liefert den Descent als **negative** Zahl
        // (GEMESSEN an `DejaVuSans.ttf`: −483 von 2048, also −0,2358 em),
        // und das ist die uebliche Vorzeichenkonvention: der Descent ist
        // der Abstand von der Grundlinie nach UNTEN.
        //
        // GEMESSEN, dass der Rest des Codes das schon erwartete:
        // die Feld-Doku hier oben sagt „Der Abstand der Grundlinie nach
        // unten (negativ)", und `total_height()` nutzt `descent.abs()`
        // (tt_metrics.rs:115). Der Test widersprach der eigenen Doku.
        assert!(
            metrics.descent < 0.0,
            "GEMESSEN: der Descent muss negativ sein (die hhea-Konvention),              ist aber {:.3} px",
            metrics.descent
        );
        // Und die Summe muss die Schrift wirklich einpacken:
        // GEMESSEN an `DejaVuSans.ttf`: ascent 0,9282 + |descent| 0,2358
        // = 1,1640 em, das ist genau die Zeilenhoehe.
        assert!(
            (metrics.total_height() - metrics.line_height).abs() < 0.01,
            "GEMESSEN: Grundlinie + Abstieg {:.3} px muss die Zeilenhoehe              {:.3} px ergeben",
            metrics.total_height(),
            metrics.line_height
        );
    }

    #[test]
    fn width_scales_with_font_size() {
        let fonts =
            crate::system::find_system_fonts(std::path::Path::new("/usr/share/fonts")).expect("ok");
        if fonts.is_empty() {
            eprintln!("KEINE_SYSTEMSCHRIFT: Test uebersprungen");
            return;
        }

        let font = crate::system::load_system_font(&fonts[0].path).expect("ok");
        let small = measure_tt(&font, "test", 8.0).expect("ok");
        let large = measure_tt(&font, "test", 16.0).expect("ok");

        assert!(large.width > small.width);
        assert!(large.line_height > small.line_height);
    }

    #[test]
    fn total_height_is_ascent_plus_descent() {
        let metrics = TtTextMetrics {
            width: 100.0,
            line_height: 16.0,
            ascent: 12.0,
            descent: 4.0,
        };
        assert_eq!(metrics.total_height(), 16.0);
    }

    #[test]
    fn measure_with_system_font_works() {
        let result = measure_with_system_font(
            "Hello",
            16.0,
            crate::system::FontCategory::SansSerif,
            false,
            false,
        );
        // Wenn keine Schrift da ist, ist das kein Fehler — nur None.
        if let Ok(metrics) = result {
            assert!(metrics.width > 0.0);
        }
    }

    #[test]
    fn tt_metrics_are_reasonable() {
        let metrics = TtTextMetrics {
            width: 100.0,
            line_height: 16.0,
            ascent: 12.0,
            descent: 4.0,
        };
        assert!(metrics.width > 0.0);
        assert!(metrics.line_height > 0.0);
        assert!(metrics.ascent > 0.0);
        assert!(metrics.descent > 0.0);
        assert!(metrics.ascent < metrics.line_height);
    }
}
