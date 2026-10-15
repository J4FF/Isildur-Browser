//! Software-Rasterizer: DisplayList -> RGBA8-Framebuffer.
//!
//! `narsil-raster` rasterisiert eine `narsil_display::DisplayList` in einen
//! RGBA8-Framebuffer und kann ihn als PPM schreiben.
//!
//! Oeffentliche API:
//!   - `Framebuffer`: RGBA8-Framebuffer mit Pixelzugriff und Alpha-Blending
//!   - `RenderResult`: Ergebnis eines Rastervorgangs
//!   - `rasterize`: Rastert eine komplette Display-List
//!   - `rasterize_with_font`: Rastert mit eingebundener Schrift
//!   - `draw_text`: Zeichnet ein Text-Item
//!   - `draw_glyph`: Zeichnet eine einzelne Glyphe

pub mod bilder;
pub mod deko;
pub mod formen;
pub mod framebuffer;
pub mod glyphs;
pub mod ppm;
pub mod rahmen;
pub mod rasterizer;
pub mod schrift;
pub mod synthetisch;
// Release 09.10.2026: gamma-korrekte Textueberblendung, Subpixel-Text.
pub mod textgamma;
// Release 09.10.2026: Liquid Glass (Hinterglas) und Gruppen-Deckkraft.
pub mod glas;
// BILD_44, Station 3: der lineare Farbverlauf (zwei Stopps). Nimmt den
// neutralen Typ aus `narsil-core` — ohne Umweg ueber `narsil-css`, der hier
// nicht anhaengt.
pub mod verlauf;

// I3: Bilder skalieren und zeichnen.
pub use bilder::{draw_bild, draw_bild_item, draw_platzhalter};
pub use framebuffer::{Framebuffer, blend_source_over};
pub use glyphs::{draw_glyph, draw_text};
pub use ppm::{parse_ppm_header, write_ppm, write_ppm_with_white_background};
pub use rasterizer::{
    RenderResult, rasterize, rasterize_bereich, rasterize_mit_schrift, rasterize_with_font,
};
// I4: die Schriftwahl (TrueType mit Bitmap-Fallback).
// GEMESSEN 03.10.2026: `aus_wunsch` ist der Weg von einem CSS-Wunsch zur
// Schrift in EINEM Aufruf. Ohne diesen `pub use` waere sie nur ueber
// `narsil_raster::schrift::aus_wunsch` erreichbar — und GEMESSEN hat der
// Test `aus_wunsch.rs` genau danach gefragt und E0425 bekommen.
pub use schrift::{Schrift, SchriftQuelle, aus_wunsch};
pub use textgamma::{LcdOrdnung, setze_lcd};
