//! Gemeinsame Werttypen der Narsil-Pipeline.
//!
//! Dieser Crate enthaelt KEINE Engine-Logik. Er definiert die Typen und
//! Konventionen, die mehrere Phasen miteinander teilen (Parser, Layout,
//! Display-List, Rasterizer). Dadurch koennen Agent2 und Agent3 unabhaengig
//! voneinander gegen exakt dieselben Definitionen arbeiten.
//!
//! Konventionen (gelten im gesamten Projekt):
//!   * Koordinatensystem: x nach rechts, y nach unten, Ursprung oben links.
//!   * Einheit: logische Pixel, Typ `f32`.
//!   * Rechtecke sind halb-offen: x liegt im Bereich [x, x + width).
//!   * Farben sind `u8`-Kanaele, alpha-premultiplied ist NICHT der Standard;
//!     `a` ist reine Deckkraft (source-over).

pub mod color;
pub mod geometry;
pub mod length;
pub mod style;
// BILD_44, Station 1: der NEUTRALE Verlaufstyp, nur Daten. `narsil-raster` darf
// nicht an `narsil-css` haengen, braucht aber Werte — deshalb liegen sie hier.
pub mod verlauf_typ;

pub use color::{Color, ColorParseError, ColorParseErrorReason};
pub use geometry::{Edges, Point, Rect, Size};
pub use length::{Dimension, Length, Spacing};
pub use style::{Direction, Style};
