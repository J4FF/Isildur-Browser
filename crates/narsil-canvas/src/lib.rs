//! `narsil-canvas`: the software renderer behind `CanvasRenderingContext2D`
//! and `OffscreenCanvasRenderingContext2D`.
//!
//! Everything here is independent of the DOM and the JavaScript engine: the
//! browser crate owns the bindings (attribute parsing, WebIDL conversions,
//! image sources) and drives a [`context::Context2D`].
//!
//! Pixels are stored premultiplied (RGBA8), shapes are rasterized with exact
//! area coverage (anti-aliased), compositing implements every
//! `globalCompositeOperation`, and strokes, dashes, gradients, patterns,
//! shadows, clipping and image drawing follow HTML §4.12.5.

pub mod blur;
pub mod colorspace;
pub mod commands;
pub mod composite;
pub mod context;
pub mod css_color;
pub mod filter;
pub mod geom;
pub mod paint;
pub mod path;
pub mod raster;
pub mod stroke;
pub mod surface;

pub use composite::CompositeOp;
pub use context::{Context2D, SmoothingQuality, State, Style};
pub use geom::{IRect, Matrix};
pub use paint::{Color, Gradient, GradientKind, Image, Pattern, Repetition, Spread};
pub use path::{Path, PathError};
pub use raster::FillRule;
pub use stroke::{LineCap, LineJoin};
