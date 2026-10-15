//! Textmessung: Breite, Hoehe und Grundlinie einer Textzeile.

use narsil_core::geometry::Size;

use crate::TextError;
use crate::font::{BitmapFont, is_valid_font_size};

/// Anteil der Zeilenhoehe, der oberhalb der Grundlinie liegt.
///
/// Die 5x7-Basisglyphe nutzt die Zeilen 0..=4 fuer Ober- und Kleinbuchstaben,
/// die Zeilen 5..=6 fuer Descender (`g`, `j`, `p`, `q`, `y`). Die Grundlinie
/// sitzt daher 5 von 7 Zeilen unterhalb der Oberkante.
pub const ASCENT_RATIO: f32 = 5.0 / 7.0;

/// DIE ZEILENHOEHE VON `line-height: normal` — **die eine Quelle**.
///
/// added 04.10.2026, Rolle SERVER (SERVER_12).
///
/// # WARUM HIER UND NICHT IM LAYOUT
///
/// Es gab in diesem Projekt **zwei** Zahlen fuer dieselbe Sache:
/// `narsil_layout_engine::text` rechnete `font_size` (1,0 em), und
/// `narsil_page::ZEILENHOEHE` war `1.2` — mit dem Kommentar, *beide* Stellen
/// muessen denselben Wert haben. Sie hatten nicht denselben.
///
/// GEMESSEN an Firefox 157 sind **beide** falsch. Diese Funktion ist die
/// Regel, die alle 20 gemessenen Schriftgroessen exakt trifft.
///
/// **WARUM `narsil-text`:** das ist eine **Schriftmetrik**, und
/// `narsil-text` ist die Crate, die Schriftmetriken liest. Ausserdem haengen
/// **beide** Crates ohnehin daran — `narsil-page` und
/// `narsil-layout-engine` haben es in `[dependencies]`. In der Engine waere
/// es nicht erreichbar gewesen: dort ist die Engine nur eine
/// **dev**-Abhängigkeit von `narsil-page`, und eine Bibliothek darf ihre
/// dev-Abhängigkeit nicht benutzen (gemessen 04.10., E0432).
///
/// # DIE REGEL
///
/// ```text
/// lh = max( round(fs * 1229/1024),            <- die Proportion der Schrift
///            ceil(fs * 1901/2048) + ceil(fs * 483/2048) )   <- die Rasterung
/// ```
///
/// **Beide Terme werden auf ganze Pixel gerundet, erst dann kommt das
/// `max`.** Die Zahlen stammen aus `DejaVuSans.ttf` (das liefert
/// `fc-match Verdana`, die Schrift der Hacker-News-Liste):
///
/// | Zahl | Herkunft | Wert |
/// |---|---|---|
/// | 1229/1024 | OS/2 `sTypoAscender+Descender+LineGap` = (1556+492+410)/2048 | 1,200195 em |
/// | 1901/2048 | `hhea` ascender | 0,928223 em |
/// | 483/2048 | `hhea` descender | 0,235840 em |
///
/// Der `max` ist kein Kosmetik: bei 13,33 px gewinnt die Rasterung
/// (13 + 4 = 17 gegen 16), bei 100 px gewinnt die Proportion (120 gegen
/// 93 + 24 = 117).
///
/// GEMESSENE TREFFERQUOTE: **20 von 20** Schriftgroessen exakt.
/// ```text
///   Schrift  ist    round(em)   ceil(A)+ceil(D)    max
///    9.33     12        11             12         12   <- Rasterung
///   13.00     17        16             17         17
///   13.33     17        16             17         17
///   16.00     19        19             19         19   <- beide gleich
///   17.00     21        20             21         21
///  100.00    120       120            117        120   <- Proportion
/// ```
/// `ceil(1.2002 em)` allein trifft **7 von 20**; `round` allein 9; die
/// Teile allein 14. Erst das `max` **nach** dem Runden trifft alle.
///
/// **DIE IM AUFTRAG GENANNTE ZAHL 1.1641 em IST WIDERLEGT:** sie ergibt
/// bei 13,33 px nur 15,52 px, gemessen sind 17,00 px.
pub fn zeilenhoehe_normal(font_size: f32) -> f32 {
    if !font_size.is_finite() || font_size <= 0.0 {
        // Kein Text, keine Zeile. `font_size` zurueckgeben waere eine Zahl
        // ohne Aussage; 0.0 sagt „keine Zeile“.
        return 0.0;
    }
    // Term 1: die Proportion der Schrift, auf GANZE Pixel gerundet.
    //   1229/1024 = 1.200195 em (OS/2 typo-Metriken, auf 1024 gekuerzt,
    //   damit die Konstante ganzzahlig bleibt).
    let em = (font_size * 1229.0 / 1024.0 + 0.5).floor();

    // Term 2: die Rasterung. Auf- und Abstand einzeln auf ganze Pixel,
    //   jedes mit mindestens 1 px — sonst bekaeme ein 2-px-Font eine Zeile
    //   der Hoehe 0.
    let auf = (font_size * 1901.0 / 2048.0).ceil().max(1.0);
    let ab = (font_size * 483.0 / 2048.0).ceil().max(1.0);

    em.max(auf + ab)
}

/// Ergebnis der Textmessung, in Pixeln.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    /// Gesamtbreite des Textes, inklusive aller Zwischenraeume.
    pub width: f32,
    /// Zeilenhoehe fuer `font_size`.
    pub height: f32,
    /// Abstand der Grundlinie von oben.
    pub ascent: f32,
}

impl TextMetrics {
    /// Hoehe unterhalb der Grundlinie (Descender-Anteil).
    pub fn descent(&self) -> f32 {
        self.height - self.ascent
    }

    /// Gesamtgroesse als [`Size`].
    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}

/// Misst eine komplette Textzeile.
///
/// # Regeln
///
/// * `width` ist die Summe aller Vorschuebe. Zeichen ausserhalb der Schrift
///   erhalten den vollen Zellvorschub, damit der Textfluss nicht
///   zusammenrueckt.
/// * `height` ist `font_size` (die Zeilenhoehe).
/// * `ascent` ist `font_size * 5/7`, also der Anteil oberhalb der
///   Grundlinie.
///
/// # Fehler
///
/// * `font_size` nicht endlich oder `<= 0` -> [`TextError::InvalidFontSize`]
/// * leere Zeichenkette -> [`TextError::EmptyText`]
///
/// Die Schriftgroesse wird **vor** dem Inhalt geprueft, damit
/// `measure("", 0.0)` reproduzierbar `InvalidFontSize` liefert.
pub fn measure(font: &BitmapFont, text: &str, font_size: f32) -> Result<TextMetrics, TextError> {
    validate_font_size(font_size)?;
    if text.is_empty() {
        return Err(TextError::EmptyText);
    }

    let mut width = 0.0f32;
    for ch in text.chars() {
        width += font.advance_or_fallback(ch, font_size);
    }

    Ok(TextMetrics {
        width,
        height: font_size,
        ascent: font_size * ASCENT_RATIO,
    })
}

/// Prueft eine Schriftgroesse und liefert sie bei Gueltigkeit unveraendert
/// zurueck.
///
/// Abgelehnt werden `NaN`, `INFINITY`, Werte `<= 0` sowie Werte oberhalb von
/// [`crate::MAX_FONT_SIZE`] — letztere wuerden bei der Rasterung eine
/// unbrauchbar grosse Allokation erzeugen.
pub(crate) fn validate_font_size(font_size: f32) -> Result<f32, TextError> {
    if !is_valid_font_size(font_size) {
        return Err(TextError::InvalidFontSize(font_size));
    }
    Ok(font_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::MAX_FONT_SIZE;

    fn font() -> BitmapFont {
        BitmapFont::builtin()
    }

    #[test]
    fn measure_hello() {
        let m = measure(&font(), "Hello", 16.0).expect("sollte klappen");
        assert!(m.width > 0.0);
        assert_eq!(m.height, 16.0);
        assert!((m.ascent - 16.0 * 5.0 / 7.0).abs() < 1e-6);
    }

    #[test]
    fn measure_five_times_single_char() {
        let m5 = measure(&font(), "Hello", 16.0).expect("ok");
        let m1 = measure(&font(), "H", 16.0).expect("ok");
        assert!(m5.width > 0.0);
        // Monospace: fuenf Zeichen sind genau fuenffel so breit.
        assert!((m5.width - 5.0 * m1.width).abs() < 1e-3);
    }

    #[test]
    fn measure_empty_text_is_error() {
        assert_eq!(measure(&font(), "", 16.0), Err(TextError::EmptyText));
    }

    #[test]
    fn measure_invalid_size_is_error() {
        for size in [
            0.0,
            -5.0,
            -0.001,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            // NaN != NaN, deshalb nicht mit assert_eq! vergleichen, sondern
            // gezielt auf die Fehlervariante pruefen.
            match measure(&font(), "x", size) {
                Err(TextError::InvalidFontSize(reported)) => assert!(
                    reported.is_nan() == size.is_nan() && (reported.is_nan() || reported == size),
                    "Groesse {size} wurde als {reported} gemeldet"
                ),
                other => panic!("Groesse {size} muss abgelehnt werden, war {other:?}"),
            }
        }
    }

    #[test]
    fn measure_rejects_oversized_font() {
        // Zu grosse Schriftgroessen wuerden eine unbrauchbare Allokation
        // erzeugen und werden deshalb abgewiesen.
        assert_eq!(
            measure(&font(), "x", MAX_FONT_SIZE + 1.0),
            Err(TextError::InvalidFontSize(MAX_FONT_SIZE + 1.0))
        );
        assert!(measure(&font(), "x", MAX_FONT_SIZE).is_ok());
        assert!(!is_valid_font_size(f32::MAX));
    }

    #[test]
    fn invalid_size_beats_empty_text() {
        // Reihenfolge ist dokumentiert: Schriftgroesse zuerst.
        assert_eq!(
            measure(&font(), "", 0.0),
            Err(TextError::InvalidFontSize(0.0))
        );
    }

    #[test]
    fn width_is_sum_of_advances() {
        let f = font();
        let m = measure(&f, "abc", 16.0).expect("ok");
        let expected = f.advance('a', 16.0).expect("ok")
            + f.advance('b', 16.0).expect("ok")
            + f.advance('c', 16.0).expect("ok");
        assert!((m.width - expected).abs() < 1e-4);
    }

    #[test]
    fn descent_is_remainder_of_height() {
        let m = measure(&font(), "x", 20.0).expect("ok");
        assert!((m.ascent + m.descent() - m.height).abs() < 1e-4);
        assert!(m.descent() > 0.0);
    }

    #[test]
    fn size_matches_metrics() {
        let m = measure(&font(), "hi", 12.0).expect("ok");
        assert_eq!(m.size(), Size::new(m.width, m.height));
    }

    #[test]
    fn measure_handles_spaces() {
        let m = measure(&font(), "a b", 16.0).expect("ok");
        let one = measure(&font(), "a", 16.0).expect("ok");
        // Leerzeichen belegt eine volle Zelle.
        assert!((m.width - 3.0 * one.width).abs() < 1e-3);
    }

    #[test]
    fn measure_handles_non_ascii_without_panic() {
        let m = measure(&font(), "Größe", 16.0).expect("ok");
        assert!(m.width > 0.0);
    }

    #[test]
    fn validate_font_size_passes_valid() {
        assert_eq!(validate_font_size(0.5), Ok(0.5));
        assert_eq!(validate_font_size(100.0), Ok(100.0));
    }
}
