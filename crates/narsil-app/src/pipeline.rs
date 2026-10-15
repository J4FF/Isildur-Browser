//! Render-Pipeline als Steckdose.
//!
//! Dies ist die STECKDOSE. Wenn Parser, Layout und Rasterizer fertig sind,
//! implementiert ein Folge-Task (TASK_0009) dieses Trait mit echter Logik.
//! An diesem Crate aendert sich dann nichts — nur der Implementierer wird
//! ausgetauscht.
//!
//! Die Quelle ist bewusst ein eingelesener String, kein Pfad: `run` liest
//! die Datei, die Pipeline sieht nur Text. So braucht die Pipeline keinen
//! Dateizugriff, und `SourceMap` kommt ohne Pfad aus. In TASK_0009 nicht
//! umbauen.

use narsil_core::Length;
use narsil_core::geometry::Size;
use narsil_dom::{Document, Span};

use crate::error::Stage;

/// Was eine Render-Pipeline koennen muss.
///
/// Rendert eine Quelldatei in RGBA8-Pixeldaten. `width`/`height` kommen aus
/// der CLI. `None` heisst: aus dem Dokument bestimmen.
///
/// Fehler als [`PipelineError`] statt String: Ein Implementierer, der den
/// `Span` kennt, gibt ihn strukturiert zurueck, statt ihn in Text
/// umzuwandeln — sonst muesste das jeder Implementierer doppelt tun und
/// die Position waere nachher nicht mehr auswertbar.
pub trait Pipeline {
    /// Rendert eine Quelldatei in RGBA8-Pixeldaten.
    ///
    /// Rueckgabe: (breite, hoehe, rgba-bytes in Reihenfolge R, G, B, A).
    fn render(
        &self,
        source: &str,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<RenderedImage, PipelineError>;
}

/// Fehler einer Pipeline-Stufe, mit Ort im Quelltext.
///
/// Strukturiert statt String, damit `run` daraus `AppError::Source` baut
/// und die Position (Zeile, Spalte, Auszug) erst bei der Anzeige entsteht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineError {
    /// Welche Stufe.
    pub stage: Stage,
    /// Das Problem in einem Satz.
    pub message: String,
    /// Wo im Quelltext, wenn die Stufe das weiss.
    pub span: Option<Span>,
}

impl PipelineError {
    /// Baut einen Stufenfehler mit Ort.
    pub fn new(stage: Stage, message: String, span: Option<Span>) -> Self {
        Self {
            stage,
            message,
            span,
        }
    }
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.span {
            Some(s) => write!(
                f,
                "{} [Byte {}+{}]: {}",
                self.stage, s.start, s.len, self.message
            ),
            None => write!(f, "{}: {}", self.stage, self.message),
        }
    }
}

impl std::error::Error for PipelineError {}

/// Die echte MVP-0-Pipeline: parsen, Viewport, Layout, Display-Liste,
/// Rastern. Sechs Aufrufe, keine Abkuerzung:
///
/// 1. `narsil_parser::parse` — Quelle wird Dokument.
/// 2. Viewport aus CLI (`--width`/`--height` gewinnen) oder `page`-Knoten.
/// 3. `narsil_layout_engine::layout` — Dokument wird Layoutbaum.
/// 4. `narsil_layout_engine::build_display_list` — Baum plus Dokument wird
///    Liste (zwei Argumente: die Farbe steht nur im Dokument).
/// 5. `narsil_raster::rasterize` — Liste wird Framebuffer, in Viewport-Groesse.
/// 6. `write_ppm` — NICHT hier, das macht `run` (kein Doppel-Schreiben).
///
/// Text laeuft mit, wird aber noch uebersprungen (`skipped > 0`), bis
/// Agent3 die Glyphenzeichnung liefert (TASK_0011). Das ist erwartet.
pub struct RealPipeline;

impl Pipeline for RealPipeline {
    fn render(
        &self,
        source: &str,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<RenderedImage, PipelineError> {
        // (1) PARSEN. Alternative verworfen: Tokens vorab pruefen —
        // parse() validiert selbst und liefert Span mit.
        let document = narsil_parser::parse(source)
            .map_err(|e| PipelineError::new(Stage::Parse, e.message, Some(e.span)))?;
        // (2) VIEWPORT. Reihenfolge: CLI, dann page, sonst Fehler.
        // Alternative verworfen: page zuerst — die CLI ist explizite
        // Render-Absicht und muss gewinnen (siehe FERTIG_0009, Frage 4).
        let (w, h) = viewport(&document, width, height)?;
        // (3) LAYOUT. Alternative verworfen: layout_with_measurer —
        // Standard-Messung (Bitmap-Schrift) ist der MVP-Weg.
        let tree = narsil_layout_engine::layout(&document, Size::new(w as f32, h as f32))
            .map_err(|e| PipelineError::new(Stage::Layout, e.to_string(), None))?;
        // (4) DISPLAY-LISTE. Kein Fehler, keine Option — immer aufrufen.
        let list = narsil_layout_engine::build_display_list(&tree, &document);
        // (5) RASTERN mit eingebundener Schrift (DRINGEND_0009_SIGNATUR:
        // rasterize() allein hiesse font=None = Text wird uebersprungen).
        // Groesse aus (2), nie bounds(). Text liegt automatisch richtig,
        // weil layout_text und draw_glyph dieselbe Bitmap benutzen —
        // Ascent NICHT addieren.
        let font = narsil_text::BitmapFont::builtin();
        let result = narsil_raster::rasterize_with_font(&list, Some(&font), w, h);
        // Framebuffer liefert RGBA8 direkt (Zeilen von oben, R,G,B,A) —
        // keine Umwandlung, nur Kopie. `new` prueft die Laenge.
        RenderedImage::new(w, h, result.framebuffer.pixels().to_vec())
            .map_err(|message| PipelineError::new(Stage::Raster, message, None))
    }
}

/// Bestimmt den Viewport in Pixeln.
///
/// CLI gewinnt gegen `page` (Ueberschreibung, keine Vorgabe). Fehlt beides
/// oder ist `page` ohne gueltige Breite/Hoehe (`auto` = 0x0 laut Spec
/// Abschnitt 6), gibt es keinen stillen Default, sondern einen Fehler der
/// das sagt: unvollstaendiges Dokument, kein "ungueltige Breite".
fn viewport(
    document: &Document,
    cli_width: Option<u32>,
    cli_height: Option<u32>,
) -> Result<(u32, u32), PipelineError> {
    let fail = |dim: &str| {
        PipelineError::new(
            Stage::Layout,
            std::format!(
                "kein Viewport: weder --{dim} gesetzt noch gueltige page-{dim} im Dokument (unvollstaendiges Dokument, auto ist 0x0)"
            ),
            None,
        )
    };
    let page_size = document
        .node(document.root())
        .map(|n| n.style.size)
        .map_err(|e| PipelineError::new(Stage::Layout, e.to_string(), None))?;
    let w = match cli_width {
        Some(v) => Some(v),
        None => match page_size.width {
            Length::Px(v) => to_pixels(v),
            _ => None,
        },
    };
    let h = match cli_height {
        Some(v) => Some(v),
        None => match page_size.height {
            Length::Px(v) => to_pixels(v),
            _ => None,
        },
    };
    match (w, h) {
        (Some(w), Some(h)) => Ok((w, h)),
        (None, _) => Err(fail("width")),
        (_, None) => Err(fail("height")),
    }
}

/// f32-Laenge in ganze Pixel, abgerundet. `None` bei NaN, unendlich,
/// <= 0 oder groesser als `u32::MAX`. Kein Panic, kein Wrap.
fn to_pixels(v: f32) -> Option<u32> {
    if !v.is_finite() || v <= 0.0 || v > u32::MAX as f32 {
        None
    } else {
        Some(v as u32)
    }
}

/// Ergebnis eines Render-Vorgangs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedImage {
    /// Breite in Pixeln.
    pub width: u32,
    /// Hoehe in Pixeln.
    pub height: u32,
    /// Laenge ist immer `width * height * 4` (RGBA8).
    pub rgba: Vec<u8>,
}

impl RenderedImage {
    /// Baut ein Render-Ergebnis und prueft die Laenge.
    ///
    /// Rechnet in `u128`, damit `u32::MAX x u32::MAX` nicht ueberlaeuft:
    /// Ueberlauf gibt `Err` statt Panic. `rgba.len()` muss exakt
    /// `width * height * 4` sein, sonst `Err` mit konkreten Zahlen.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        let want = width as u128 * height as u128 * 4_u128;
        if want != rgba.len() as u128 {
            return Err(std::format!(
                "ungueltiges Render-Ergebnis: {width}x{height} braucht {want} Bytes, erhalten {}",
                rgba.len()
            ));
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// Anzahl der Pixel. Rechnet aus dem gespeicherten Puffer (`len / 4`),
    /// damit kein Ueberlauf moeglich ist: `new` garantiert bereits
    /// `len == width * height * 4`.
    pub fn pixel_count(&self) -> usize {
        self.rgba.len() / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_matching_buffer() {
        let created = RenderedImage::new(2, 2, vec![0; 16]);
        assert!(created.is_ok());
        if let Ok(img) = created {
            assert_eq!(img.pixel_count(), 4);
        }
    }

    #[test]
    fn new_rejects_wrong_length_without_panic() {
        assert!(RenderedImage::new(2, 2, vec![0; 15]).is_err());
        assert!(RenderedImage::new(u32::MAX, u32::MAX, Vec::new()).is_err());
    }
}
