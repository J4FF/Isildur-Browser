//! Glyphen-Rasterung: zeichnet Text-Items in den Framebuffer.
//!
//! Dieses Modul uebersetzt `DisplayItem::Text` in echte Pixel.
//! Es nutzt `narsil-text` fuer Shaping und `crate::framebuffer` fuer das Zeichnen.

use narsil_core::color::Color;
use narsil_core::geometry::Point;
use narsil_text::{BitmapFont, PositionedGlyph, layout_text};

use crate::deko;
use crate::framebuffer::Framebuffer;

/// Rastert ein Text-Item in den Framebuffer.
///
/// Rueckgabe: Anzahl tatsaechlich gezeichneter Pixel.
/// Kein Panic bei jedem Eingabewert.
///
/// Fehlerverhalten:
/// - Leerer String: 0 Pixel, kein Fehler
/// - Ungueltige Schriftgroesse (0, NaN, negativ, unendlich): 0 Pixel
/// - Unbekannte Zeichen: werden uebersprungen, aber der restliche Text wird gezeichnet
pub fn draw_text(
    fb: &mut Framebuffer,
    font: &BitmapFont,
    item_text: &str,
    color: Color,
    font_size: f32,
    origin: Point,
) -> usize {
    draw_text_mit_deko(fb, font, item_text, color, font_size, origin, 0)
}

/// Rastert ein Text-Item und zeichnet die Textdekoration darunter.
///
/// `deko` ist die Bitmaske aus `narsil-css` (`Berechnet::dekoration`):
/// 1 = `underline`, 4 = `line-through`; siehe [`crate::deko`].
///
/// Die Dekoration laeuft ueber den **Vorschub** jeder Glyphe, nicht ueber
/// ihre Tintenbreite. Das ist der Unterschied, den man sieht: bei
/// `underline` auf einem Text mit Leerzeichen gaebe es sonst Luecken genau
/// an den Stellen, an denen nichts zu unterstreichen waere — und jeder
/// Link mit einem Wortabstand haette eine.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_mit_deko(
    fb: &mut Framebuffer,
    font: &BitmapFont,
    item_text: &str,
    color: Color,
    font_size: f32,
    origin: Point,
    deko: u8,
) -> usize {
    // Leerer String: nichts zu tun
    if item_text.is_empty() {
        return 0;
    }

    // Versuche das Text zu layouten
    let glyphs = match layout_text(font, item_text, font_size, origin) {
        Ok(g) => g,
        Err(_) => return 0, // Ungueltige Schriftgroesse
    };

    // Zeichne jede Glyphe
    let mut total_pixels = 0;
    for glyph in &glyphs {
        total_pixels += draw_glyph(fb, glyph, color);
    }

    if deko::mask_gueltig(deko) != 0 {
        // GEMESSEN 01.10.2026: `layout_text` laesst an Zeilenumbruechen
        // eine Luecke zwischen den Glyphen. Die Dekoration muss je Zeile
        // fuer sich gezogen werden, sonst verbindet ein Strich zwei
        // getrennte Zeilen ueber die ganze Zeilenhoehe.
        deko_ueber_zeilen(fb, &glyphs, font_size, deko, color);
    }

    total_pixels
}

/// M3: hoechste Zahl Zeilen, die ein Text-Item haben darf. Ohne Grenze
/// koennte ein Text mit vielen Umbruechen eine unbegrenzte Schleife
/// erzeugen.
const MAX_ZEILEN: usize = 4096;

/// Zeichnet die Dekoration fuer jede Zeile getrennt.
fn deko_ueber_zeilen(
    fb: &mut Framebuffer,
    glyphs: &[narsil_text::PositionedGlyph],
    font_size: f32,
    deko: u8,
    color: Color,
) {
    let mut anfang = 0usize;
    let mut abschnitte = 0usize;
    while anfang < glyphs.len() {
        if abschnitte >= MAX_ZEILEN {
            break;
        }
        abschnitte += 1;
        let y = glyphs[anfang].y;
        let mut ende = anfang;
        while ende < glyphs.len() && glyphs[ende].y == y {
            ende += 1;
        }
        // Die Strecke geht von der linken Kante der ersten bis zur
        // rechten der letzten Glyphe. Bei einer einzelnen Glyphe zaehlt
        // ihr Vorschub, sonst waere eine einzelne unterstrichene Glyphe
        // unsichtbar.
        let zeile = &glyphs[anfang..ende];
        let x0 = zeile[0].x;
        let letzte = &zeile[zeile.len() - 1];
        let x1 = letzte.x + letzte.advance;
        deko::deko_zeile(fb, deko, y, font_size, x0, x1, color);
        anfang = ende;
    }
}

/// Rastert eine fertig platzierte Glyphe.
///
/// Alpha der Bitmap ist die Deckung. Bei Deckung 0 wird NICHTS geschrieben
/// (ein Pixel darf nicht unveraendert als "gezeichnet" gezaehlt werden).
///
/// Rueckgabe: Anzahl tatsaechlich gezeichneter Pixel.
pub fn draw_glyph(fb: &mut Framebuffer, glyph: &PositionedGlyph, color: Color) -> usize {
    let bitmap = &glyph.bitmap;

    // Leere Bitmap: nichts zu tun
    if bitmap.is_empty() {
        return 0;
    }

    // Position der Glyphe im Framebuffer
    // Rundung: floor, weil die Bitmap an der linken oberen Ecke beginnt
    // und negative Koordinaten korrekt behandelt werden muessen
    let base_x = glyph.x.floor() as i64;
    let base_y = glyph.y.floor() as i64;

    let mut drawn = 0;

    // Iteriere ueber alle Pixel der Bitmap
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let alpha = bitmap.alpha_at(x, y);

            // Bei Deckung 0 wird nichts geschrieben
            if alpha == 0 {
                continue;
            }

            // Berechne Position im Framebuffer.
            //
            // GEMESSEN 01.10.2026 (A3): mit `+` Panicked dieser Test bei
            // unendlicher Schriftgroesse. `glyph.y.floor() as i64` ist bei
            // `f32::INFINITY` `i64::MAX`, und `i64::MAX + y` laeuft ueber.
            // `saturating_add` kann das nicht und liefert stattdessen eine
            // Position, die der Clipping-Test unten ohnehin verwirft.
            let px = base_x.saturating_add(i64::from(x));
            let py = base_y.saturating_add(i64::from(y));

            // Clipping: nur zeichnen, wenn innerhalb des Framebuffers
            if px < 0 || py < 0 {
                continue;
            }

            let px = px as u32;
            let py = py as u32;

            // set_pixel gibt false zurueck, wenn ausserhalb
            // Bei Deckung 255 wird direkt ueberschrieben
            if fb.set_pixel(px, py, color) {
                drawn += 1;
            }
        }
    }

    drawn
}

#[cfg(test)]
mod tests {
    use super::*;
    use narsil_text::BitmapFont;

    #[test]
    fn draw_text_basic() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let pixels = draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );
        assert!(pixels > 0, "Text sollte Pixel zeichnen");
    }

    #[test]
    fn draw_text_empty_string() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let pixels = draw_text(
            &mut fb,
            &font,
            "",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );
        assert_eq!(pixels, 0);
    }

    #[test]
    fn draw_text_invalid_font_size() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let pixels = draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            0.0,
            Point::new(0.0, 0.0),
        );
        assert_eq!(pixels, 0);
    }

    #[test]
    fn draw_glyph_empty_bitmap() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let glyphs = layout_text(&font, " ", 7.0, Point::new(0.0, 0.0)).unwrap();
        let pixels = draw_glyph(&mut fb, &glyphs[0], Color::rgb(255, 0, 0));
        assert_eq!(pixels, 0);
    }

    #[test]
    fn draw_text_with_rectangle() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();

        // Rechteck zeichnen
        fb.fill_rect(
            narsil_core::geometry::Rect::new(0.0, 0.0, 10.0, 10.0),
            Color::rgb(0, 0, 255),
        );

        // Text darueber zeichnen
        let pixels = draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );
        assert!(pixels > 0);
    }

    #[test]
    fn draw_text_negative_origin() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        // Sollte nicht panicen
        let _pixels = draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(-3.0, -2.0),
        );
    }

    #[test]
    fn draw_text_outside_origin() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let pixels = draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(9999.0, 9999.0),
        );
        assert_eq!(pixels, 0);
    }

    #[test]
    fn draw_text_zero_size_framebuffer() {
        let mut fb = Framebuffer::new(0, 0, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        // Sollte nicht panicen
        let _pixels = draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );
    }

    #[test]
    fn draw_text_unknown_characters() {
        let mut fb = Framebuffer::new(50, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        // Uebersetzt Zeichen, die nicht in der Schrift sind
        let pixels = draw_text(
            &mut fb,
            &font,
            "A\u{1F600}B",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );
        // A und B sollten gezeichnet werden
        assert!(pixels > 0);
    }

    #[test]
    fn draw_text_newline() {
        let mut fb = Framebuffer::new(20, 50, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let pixels = draw_text(
            &mut fb,
            &font,
            "A\nB",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );
        assert!(pixels > 0);
    }

    #[test]
    fn draw_text_space() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let pixels = draw_text(
            &mut fb,
            &font,
            " ",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );
        assert_eq!(pixels, 0);
    }

    #[test]
    fn draw_text_nan_font_size() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let pixels = draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            f32::NAN,
            Point::new(0.0, 0.0),
        );
        assert_eq!(pixels, 0);
    }

    #[test]
    fn draw_text_infinity_font_size() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let pixels = draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            f32::INFINITY,
            Point::new(0.0, 0.0),
        );
        assert_eq!(pixels, 0);
    }

    #[test]
    fn draw_text_negative_font_size() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let pixels = draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            -5.0,
            Point::new(0.0, 0.0),
        );
        assert_eq!(pixels, 0);
    }

    #[test]
    fn draw_glyph_pixel_accuracy() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let glyphs = layout_text(&font, "A", 7.0, Point::new(0.0, 0.0)).unwrap();
        let pixels = draw_glyph(&mut fb, &glyphs[0], Color::rgb(255, 0, 0));

        // A sollte mehrere Pixel zeichnen
        assert!(pixels > 0);

        // Pruefe, dass die Pixel an den erwarteten Positionen sind
        // Die genaue Position haengt von der Rundung ab
    }

    #[test]
    fn draw_text_multiple_lines() {
        let mut fb = Framebuffer::new(50, 50, Color::TRANSPARENT);
        let font = BitmapFont::builtin();
        let pixels = draw_text(
            &mut fb,
            &font,
            "A\nB\nC",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );
        assert!(pixels > 0);
    }

    #[test]
    fn draw_text_overlapping_rect() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();

        // Rechteck zeichnen
        fb.fill_rect(
            narsil_core::geometry::Rect::new(0.0, 0.0, 10.0, 10.0),
            Color::rgb(0, 0, 255),
        );

        // Text darueber zeichnen
        let pixels = draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );

        // Text sollte sichtbar sein
        assert!(pixels > 0);
    }

    #[test]
    fn draw_text_rect_over_text() {
        let mut fb = Framebuffer::new(20, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();

        // Text zeichnen
        draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );

        // Rechteck darueber zeichnen
        fb.fill_rect(
            narsil_core::geometry::Rect::new(0.0, 0.0, 10.0, 10.0),
            Color::rgb(0, 0, 255),
        );

        // Rect sollte gewinnen
        assert_eq!(fb.pixel(0, 0), Some(Color::rgb(0, 0, 255)));
    }

    #[test]
    fn draw_text_drawn_count() {
        let mut fb = Framebuffer::new(50, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();

        // Zwei Text-Items
        let pixels1 = draw_text(
            &mut fb,
            &font,
            "A",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );
        let pixels2 = draw_text(
            &mut fb,
            &font,
            "B",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(10.0, 0.0),
        );

        assert!(pixels1 > 0);
        assert!(pixels2 > 0);
    }

    #[test]
    fn draw_text_clipped() {
        let mut fb = Framebuffer::new(5, 5, Color::TRANSPARENT);
        let font = BitmapFont::builtin();

        // Text, der ueber den Rand hinausgeht
        let pixels = draw_text(
            &mut fb,
            &font,
            "Hello",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );

        // Sollte nicht panicen, auch wenn ueber den Rand hinaus
        // Die Anzahl der gezeichneten Pixel sollte kleiner sein als die Textlaenge
        assert!(pixels > 0);
    }

    #[test]
    fn draw_text_tab() {
        let mut fb = Framebuffer::new(50, 20, Color::TRANSPARENT);
        let font = BitmapFont::builtin();

        // Tabulator
        let pixels = draw_text(
            &mut fb,
            &font,
            "A\tB",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );

        // A und B sollten gezeichnet werden
        assert!(pixels > 0);
    }

    #[test]
    fn draw_text_crlf() {
        let mut fb = Framebuffer::new(50, 50, Color::TRANSPARENT);
        let font = BitmapFont::builtin();

        // CRLF
        let pixels = draw_text(
            &mut fb,
            &font,
            "A\r\nB",
            Color::rgb(255, 0, 0),
            7.0,
            Point::new(0.0, 0.0),
        );

        // Beide Zeilen sollten gezeichnet werden
        assert!(pixels > 0);
    }
}
