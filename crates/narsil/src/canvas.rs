//! Bridge between the script-facing Canvas API (`canvas2d.js`) and the
//! renderer in `narsil-canvas`.
//!
//! The script side keeps only object identity (contexts, gradients,
//! patterns, `Path2D` command lists); every drawing state and every pixel
//! lives here, keyed by a surface id: the host id of a `<canvas>` element,
//! or an id from `OFFSCREEN_BASE` upwards for `OffscreenCanvas`. One host
//! function, `__narsilC2d(op, id, ...)`, carries all operations; the op
//! numbers are shared with `canvas2d.js` (see `op` below).

use narsil_canvas as nc;
/// The renderer, for tests and tools.
pub use narsil_canvas as engine;
use narsil_jsvm::Wert;
use std::collections::HashMap;
use std::sync::Arc;

/// Surface ids for `OffscreenCanvas` (element canvases use their host id).
pub const OFFSCREEN_BASE: u32 = 0x4000_0000;
/// All bitmaps of a page together (pixels) — a memory guard on top of the
/// per-surface limit.
pub const MAX_TOTAL_PIXELS: usize = 96 * 1024 * 1024;
/// Largest single bitmap (pixels), as in `narsil_canvas::surface`.
pub const MAX_PIXELS: usize = nc::surface::MAX_PIXELS;
/// Byte budget of cached decoded images, patterns and bitmaps.
const CACHE_BUDGET: usize = 256 * 1024 * 1024;

/// The script half of the API, evaluated on first use.
pub const SCRIPT: &str = include_str!("canvas2d.js");

/// Operation numbers (keep in sync with `canvas2d.js`).
pub mod op {
    pub const GET_CONTEXT: u32 = 1;
    pub const CREATE_OFFSCREEN: u32 = 2;
    pub const RESIZE: u32 = 3;
    pub const SIZE: u32 = 4;
    pub const TO_DATA_URL: u32 = 5;
    pub const CONTEXT_ATTRS: u32 = 6;
    pub const SNAPSHOT: u32 = 7;
    pub const ENCODE: u32 = 8;
    pub const DECODE: u32 = 9;
    pub const SAVE: u32 = 10;
    pub const RESTORE: u32 = 11;
    pub const RESET: u32 = 12;
    pub const GET: u32 = 13;
    pub const SET: u32 = 14;
    pub const SET_LINE_DASH: u32 = 15;
    pub const GET_LINE_DASH: u32 = 16;
    pub const SET_GRADIENT: u32 = 17;
    pub const SET_PATTERN: u32 = 18;
    pub const PARSE_COLOR: u32 = 19;
    pub const TRANSLATE: u32 = 20;
    pub const SCALE: u32 = 21;
    pub const ROTATE: u32 = 22;
    pub const TRANSFORM: u32 = 23;
    pub const SET_TRANSFORM: u32 = 24;
    pub const RESET_TRANSFORM: u32 = 25;
    pub const GET_TRANSFORM: u32 = 26;
    pub const BEGIN_PATH: u32 = 30;
    pub const MOVE_TO: u32 = 31;
    pub const LINE_TO: u32 = 32;
    pub const QUAD_TO: u32 = 33;
    pub const CUBIC_TO: u32 = 34;
    pub const ARC_TO: u32 = 35;
    pub const ARC: u32 = 36;
    pub const ELLIPSE: u32 = 37;
    pub const RECT: u32 = 38;
    pub const ROUND_RECT: u32 = 39;
    pub const CLOSE_PATH: u32 = 40;
    pub const FILL: u32 = 41;
    pub const STROKE: u32 = 42;
    pub const CLIP: u32 = 43;
    pub const IN_PATH: u32 = 44;
    pub const IN_STROKE: u32 = 45;
    pub const FILL_RECT: u32 = 46;
    pub const STROKE_RECT: u32 = 47;
    pub const CLEAR_RECT: u32 = 48;
    pub const DRAW_IMAGE: u32 = 50;
    pub const GET_IMAGE_DATA: u32 = 51;
    pub const PUT_IMAGE_DATA: u32 = 52;
    pub const SOURCE_INFO: u32 = 53;
    pub const UPLOAD_BITMAP: u32 = 54;
    pub const PARSE_SVG_PATH: u32 = 60;
    pub const BEGIN_LAYER: u32 = 61;
    pub const SET_FILTER_SPEC: u32 = 63;
    pub const LINK_PLACEHOLDER: u32 = 64;
    pub const END_LAYER: u32 = 62;
    pub const FILL_TEXT: u32 = 70;
    pub const STROKE_TEXT: u32 = 71;
    pub const MEASURE_TEXT: u32 = 72;
}

/// `getContext` options that the context remembers.
#[derive(Clone, Debug, PartialEq)]
pub struct Attributes {
    pub alpha: bool,
    pub color_space: String,
    /// `toneMapping.mode` (`standard` or `extended`).
    pub tone_mapping: String,
    pub desynchronized: bool,
    pub will_read_frequently: bool,
    pub color_type: String,
}

impl Default for Attributes {
    fn default() -> Self {
        Attributes { alpha: true, color_space: "srgb".into(), tone_mapping: "standard".into(), desynchronized: false, will_read_frequently: false, color_type: "unorm8".into() }
    }
}

/// One drawing surface with its 2D context.
pub struct Surface {
    pub ctx: nc::Context2D,
    /// The `<canvas>` element (host id), `None` for `OffscreenCanvas`.
    pub element: Option<u32>,
    pub attrs: Attributes,
    /// The script asked for a 2D context (an `OffscreenCanvas` without one
    /// still owns a transparent bitmap).
    pub has_context: bool,
    /// Tokens of the gradient/pattern objects the state references
    /// (fill, stroke) — the script maps them back to objects.
    pub fill_token: f64,
    pub stroke_token: f64,
    saved_tokens: Vec<(f64, f64)>,
    /// For an `OffscreenCanvas` from `transferControlToOffscreen()`: the
    /// placeholder `<canvas>` that shows its bitmap and lends it `lang`
    /// and `direction`.
    pub placeholder: Option<u32>,
}

impl Surface {
    /// The element whose styles text inherits (`lang`, `direction`).
    #[must_use]
    pub fn style_element(&self) -> Option<u32> {
        self.element.or(self.placeholder)
    }
}

/// A decoded `<img>` (or other source) in premultiplied form.
#[derive(Clone)]
struct CachedImage {
    image: nc::Image,
    /// Natural size the script sees (the buffer may be smaller).
    natural: (u32, u32),
}

/// Every canvas of a page.
#[derive(Default)]
pub struct Canvases {
    pub surfaces: HashMap<u32, Surface>,
    next_offscreen: u32,
    /// `src` -> premultiplied decoded image.
    images: HashMap<String, CachedImage>,
    /// Pattern and bitmap pixels by script token.
    bitmaps: HashMap<u64, CachedImage>,
    /// Least recently used first.
    lru: Vec<(u8, String, u64)>,
    cache_bytes: usize,
    /// Parsed color strings (`fillStyle` is set very often).
    colors: HashMap<String, Option<nc::Color>>,
}

impl std::fmt::Debug for Canvases {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Canvases").field("surfaces", &self.surfaces.len()).finish()
    }
}

impl Canvases {
    fn total_pixels(&self) -> usize {
        self.surfaces.values().map(|s| s.ctx.width() as usize * s.ctx.height() as usize).sum()
    }

    /// An element's width/height attribute changed: recreate its bitmap.
    pub fn element_resized(&mut self, element: u32, width: u32, height: u32) {
        let others = self.total_pixels().saturating_sub(self.surfaces.get(&element).map_or(0, |s| s.ctx.width() as usize * s.ctx.height() as usize));
        if let Some(s) = self.surfaces.get_mut(&element) {
            let (w, h) = fit(width, height, others);
            s.ctx.resize(w, h);
            s.fill_token = 0.0;
            s.stroke_token = 0.0;
            s.saved_tokens.clear();
        }
    }

    /// Display updates for changed element canvases: `(host, w, h, rgba)`.
    /// Element canvases for which `shown(element)` is false stay dirty.
    pub fn take_dirty(&mut self, shown: impl Fn(u32) -> bool) -> Vec<(u32, u32, u32, Arc<Vec<u8>>)> {
        let mut out = Vec::new();
        for s in self.surfaces.values_mut() {
            if !s.ctx.dirty {
                continue;
            }
            let shows = s.element.or(s.placeholder);
            if shows.is_some_and(|el| !shown(el)) {
                continue;
            }
            s.ctx.dirty = false;
            if let Some(el) = shows {
                // The content of open layers is not presented.
                let mut px = s.ctx.presented().to_unpremultiplied();
                nc::colorspace::convert(&mut px, s.ctx.color_space, nc::colorspace::ColorSpace::Srgb);
                out.push((el, s.ctx.width(), s.ctx.height(), Arc::new(px)));
            }
        }
        out
    }

    /// The bitmap of an element canvas (for tests and readback).
    #[must_use]
    pub fn element_pixels(&self, element: u32) -> Option<(u32, u32, Vec<u8>)> {
        let s = self.surfaces.get(&element)?;
        Some((s.ctx.width(), s.ctx.height(), export_srgb(&s.ctx)))
    }

    fn touch(&mut self, kind: u8, key: &str, token: u64) {
        if let Some(i) = self.lru.iter().position(|(k, s, t)| *k == kind && s == key && *t == token) {
            let e = self.lru.remove(i);
            self.lru.push(e);
        }
    }

    fn insert_cached(&mut self, kind: u8, key: String, token: u64, img: CachedImage) {
        let bytes = img.image.data.len();
        while self.cache_bytes + bytes > CACHE_BUDGET && !self.lru.is_empty() {
            let (k, s, t) = self.lru.remove(0);
            let freed = if k == 0 { self.images.remove(&s) } else { self.bitmaps.remove(&t) };
            if let Some(f) = freed {
                self.cache_bytes = self.cache_bytes.saturating_sub(f.image.data.len());
            }
        }
        self.cache_bytes += bytes;
        self.lru.push((kind, key.clone(), token));
        if kind == 0 {
            if let Some(old) = self.images.insert(key, img) {
                self.cache_bytes = self.cache_bytes.saturating_sub(old.image.data.len());
            }
        } else if let Some(old) = self.bitmaps.insert(token, img) {
            self.cache_bytes = self.cache_bytes.saturating_sub(old.image.data.len());
        }
    }
}

/// Clamps a bitmap so that all surfaces together stay within budget.
fn fit(width: u32, height: u32, others: usize) -> (u32, u32) {
    let n = width as usize * height as usize;
    if n > MAX_PIXELS || others + n > MAX_TOTAL_PIXELS { (0, 0) } else { (width, height) }
}

// --- argument helpers ------------------------------------------------------

fn num(args: &[Wert], i: usize) -> f64 {
    match args.get(i) {
        Some(Wert::Zahl(n)) => *n,
        Some(Wert::Wahr(b)) => f64::from(u8::from(*b)),
        Some(Wert::Null) => 0.0,
        Some(Wert::Text(t)) => {
            let t = t.trim();
            if t.is_empty() { 0.0 } else { t.parse().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}

fn text(args: &[Wert], i: usize) -> String {
    args.get(i).map(|v| v.als_text()).unwrap_or_default()
}

fn boolean(args: &[Wert], i: usize) -> bool {
    match args.get(i) {
        Some(Wert::Wahr(b)) => *b,
        Some(Wert::Zahl(n)) => *n != 0.0 && !n.is_nan(),
        Some(Wert::Text(t)) => !t.is_empty(),
        Some(Wert::Undefiniert | Wert::Null) | None => false,
        Some(_) => true,
    }
}

fn numbers(v: Option<&Wert>) -> Option<Vec<f64>> {
    match v {
        Some(Wert::Feld(items)) => Some(
            items
                .iter()
                .map(|x| match x {
                    Wert::Zahl(n) => *n,
                    Wert::Wahr(b) => f64::from(u8::from(*b)),
                    _ => f64::NAN,
                })
                .collect(),
        ),
        _ => None,
    }
}

fn rule(args: &[Wert], i: usize) -> nc::FillRule {
    if text(args, i) == "evenodd" { nc::FillRule::EvenOdd } else { nc::FillRule::NonZero }
}

fn path_error(e: nc::PathError) -> String {
    match e {
        nc::PathError::NegativeRadius => "IndexSizeError: The radius provided is negative.".into(),
    }
}

/// Parses a CSS color for the canvas; `currentcolor` is opaque black
/// (gradient stops, contexts without an element).
#[must_use]
pub fn parse_css_color(t: &str) -> Option<nc::Color> {
    parse_css_color_with(t, nc::Color::BLACK)
}

/// Parses a CSS color with `current` as the value of `currentcolor`.
#[must_use]
pub fn parse_css_color_with(t: &str, current: nc::Color) -> Option<nc::Color> {
    nc::css_color::parse_color(t, &|k| keyword_color(k, current))
}

/// Keywords and hex colors for the functional-notation parser.
fn keyword_color(k: &str, current: nc::Color) -> Option<nc::Color> {
    if k == "currentcolor" {
        return Some(current);
    }
    // The CSS engine's values first (system colors as the page shows
    // them), then the deprecated system colors it does not know.
    if let Some(f) = narsil_css::farbe_aus(k) {
        return Some(nc::Color::from_rgba8(f.r, f.g, f.b, f.a));
    }
    if let Some(c) = system_color(k) {
        return Some(c);
    }
    narsil_css::inline_lesen(&format!("color:{k}")).into_iter().find_map(|d| match d.wert {
        narsil_css::Wert::Farbe(c) => Some(nc::Color::from_rgba8(c.r, c.g, c.b, c.a)),
        _ => None,
    })
}

/// Deprecated CSS system colors (CSS Color 4 §6.2), mapped to their
/// modern equivalents' light-scheme values.
fn system_color(s: &str) -> Option<nc::Color> {
    let rgb = |r, g, b| Some(nc::Color::from_rgba8(r, g, b, 255));
    match s {
        "activeborder" | "inactiveborder" | "threeddarkshadow" | "threedshadow" | "threedhighlight" | "threedlightshadow" | "threedface" | "windowframe" | "buttonborder" => rgb(118, 118, 118),
        "activecaption" | "inactivecaption" | "appworkspace" | "background" | "canvas" | "window" | "menu" | "infobackground" => rgb(255, 255, 255),
        "buttonface" | "buttonhighlight" | "buttonshadow" => rgb(239, 239, 239),
        "buttontext" | "captiontext" | "inactivecaptiontext" | "canvastext" | "windowtext" | "menutext" | "infotext" | "fieldtext" => rgb(0, 0, 0),
        "graytext" => rgb(109, 109, 109),
        "highlight" | "selecteditem" => rgb(0, 117, 255),
        "highlighttext" | "selecteditemtext" => rgb(255, 255, 255),
        "linktext" => rgb(0, 0, 238),
        "visitedtext" => rgb(85, 26, 139),
        "activetext" => rgb(255, 0, 0),
        "field" => rgb(255, 255, 255),
        "mark" => rgb(255, 255, 0),
        "marktext" => rgb(0, 0, 0),
        "accentcolor" => rgb(0, 117, 255),
        "accentcolortext" => rgb(255, 255, 255),
        _ => None,
    }
}

/// The canvas element's computed `color` (for `currentColor`); opaque
/// black when the canvas is not in a document.
fn current_color(w: &mut crate::dom::Stand, id: u32) -> nc::Color {
    let Some(el) = w.canvas.surfaces.get(&id).and_then(|s| s.element) else { return nc::Color::BLACK };
    let connected = w.lebend.as_ref().is_some_and(|l| l.knoten(el).is_some_and(|k| l.ist_verbunden(k)));
    if !connected {
        return nc::Color::BLACK;
    }
    crate::dom::computed_value(w, el, "color").and_then(|c| parse_css_color(&c)).unwrap_or(nc::Color::BLACK)
}

fn color_for(w: &mut crate::dom::Stand, id: u32, value: &str) -> Option<nc::Color> {
    if value.to_ascii_lowercase().contains("currentcolor") {
        let current = current_color(w, id);
        return parse_css_color_with(value, current);
    }
    if let Some(c) = w.canvas.colors.get(value) {
        return *c;
    }
    let c = parse_css_color(value);
    if w.canvas.colors.len() > 512 {
        w.canvas.colors.clear();
    }
    w.canvas.colors.insert(value.to_string(), c);
    c
}

/// `[r, g, b, a, legacy]` for gradient stops kept in JavaScript.
fn color_value(c: nc::Color) -> Wert {
    let legacy = if c.is_legacy() { 1.0 } else { 0.0 };
    Wert::Feld(vec![Wert::Zahl(c.r as f64), Wert::Zahl(c.g as f64), Wert::Zahl(c.b as f64), Wert::Zahl(c.a as f64), Wert::Zahl(legacy)])
}

/// HTML "rules for parsing non-negative integers" for canvas sizes.
#[must_use]
pub fn parse_dimension(v: &str, fallback: u32) -> u32 {
    let t = v.trim_start_matches([' ', '\t', '\n', '\x0c', '\r']);
    let t = t.strip_prefix('+').unwrap_or(t);
    let digits: &str = &t[..t.bytes().take_while(u8::is_ascii_digit).count()];
    if digits.is_empty() {
        return fallback;
    }
    match digits.parse::<u64>() {
        Ok(n) if n <= i32::MAX as u64 => n as u32,
        _ => fallback,
    }
}

// --- image sources ---------------------------------------------------------

/// Where an image argument comes from.
enum SourceRef {
    Element(u32),
    Surface(u32),
    Bitmap(u64),
}

fn source_ref(args: &[Wert], kind_at: usize) -> Option<SourceRef> {
    let kind = num(args, kind_at);
    let r = args.get(kind_at + 1)?;
    match kind as u32 {
        0 => narsil_jsvm::host_id(r).map(SourceRef::Element),
        1 => Some(SourceRef::Surface(num(args, kind_at + 1) as u32)),
        2 => Some(SourceRef::Bitmap(num(args, kind_at + 1) as u64)),
        _ => None,
    }
}

/// The local name of an element (living DOM first: elements created by
/// scripts are only known there).
pub(crate) fn element_tag(w: &crate::dom::Stand, el: u32) -> Option<String> {
    if let Some(l) = w.lebend.as_ref()
        && let Some(k) = l.knoten(el)
        && let Some(n) = l.dok.element_name(k)
    {
        return Some(n.to_ascii_lowercase());
    }
    w.tag.get(&el).map(|t| t.to_ascii_lowercase())
}

/// Outcome of "check the usability of the image argument".
/// The un-premultiplied bitmap converted to sRGB (display, encoding).
fn export_srgb(ctx: &nc::Context2D) -> Vec<u8> {
    let mut px = ctx.surface.to_unpremultiplied();
    nc::colorspace::convert(&mut px, ctx.color_space, nc::colorspace::ColorSpace::Srgb);
    px
}

enum Usable {
    /// Premultiplied pixels, natural size and the color space they are in.
    Good(nc::Image, (u32, u32), nc::colorspace::ColorSpace),
    /// Not (yet) decodable: draw nothing.
    Bad,
    /// Script must upload the bitmap first.
    NeedData,
}

fn usable_source(w: &mut crate::dom::Stand, src: &SourceRef) -> Result<Usable, String> {
    match *src {
        SourceRef::Surface(id) => {
            let s = w.canvas.surfaces.get(&id).ok_or("InvalidStateError: The canvas has been detached.")?;
            if s.ctx.layer_depth() > 0 {
                return Err("InvalidStateError: The canvas has open layers.".into());
            }
            if s.ctx.width() == 0 || s.ctx.height() == 0 {
                return Err("InvalidStateError: The image argument is a canvas element with a width or height of 0.".into());
            }
            Ok(Usable::Good(s.ctx.surface.snapshot(), (s.ctx.width(), s.ctx.height()), s.ctx.color_space))
        }
        SourceRef::Bitmap(token) => match w.canvas.bitmaps.get(&token).cloned() {
            Some(c) => {
                w.canvas.touch(1, "", token);
                Ok(Usable::Good(c.image, c.natural, nc::colorspace::ColorSpace::Srgb))
            }
            None => Ok(Usable::NeedData),
        },
        SourceRef::Element(el) => {
            let tag = element_tag(w, el).unwrap_or_default();
            match tag.as_str() {
                "canvas" => {
                    if let Some(s) = w.canvas.surfaces.get(&el) {
                        if s.ctx.layer_depth() > 0 {
                            return Err("InvalidStateError: The canvas has open layers.".into());
                        }
                        if s.ctx.width() == 0 || s.ctx.height() == 0 {
                            return Err("InvalidStateError: The image argument is a canvas element with a width or height of 0.".into());
                        }
                        return Ok(Usable::Good(s.ctx.surface.snapshot(), (s.ctx.width(), s.ctx.height()), s.ctx.color_space));
                    }
                    let (cw, ch) = crate::dom::canvas_size(w, el);
                    if cw == 0 || ch == 0 {
                        return Err("InvalidStateError: The image argument is a canvas element with a width or height of 0.".into());
                    }
                    if cw as usize * ch as usize > MAX_PIXELS {
                        return Ok(Usable::Bad);
                    }
                    let blank = nc::Image { width: cw, height: ch, data: Arc::new(vec![0; cw as usize * ch as usize * 4]) };
                    Ok(Usable::Good(blank, (cw, ch), nc::colorspace::ColorSpace::Srgb))
                }
                "img" | "image" => image_element(w, el),
                _ => Ok(Usable::Bad),
            }
        }
    }
}

/// The finished source of an `<img>`: `(url, natural width, height)`, the
/// same resolution `img.complete` uses (reported load, else the cache of
/// finished sources by `src`, then `srcset` candidates).
fn image_source(w: &crate::dom::Stand, el: u32) -> Option<(String, u32, u32)> {
    let l = w.lebend.as_ref();
    let k = l.and_then(|l| l.knoten(el));
    let attr = |name: &str| -> Option<String> {
        match (l, k) {
            (Some(l), Some(k)) => l.dok.attribut(k, name).map(str::to_string),
            _ => w.attribute.get(&el).and_then(|a| a.get(name)).cloned(),
        }
    };
    let src = attr("src").unwrap_or_default().trim().to_string();
    let srcset = attr("srcset").filter(|s| !s.trim().is_empty());
    let quelle = narsil_url::loese_auf(&w.adresse, &src).unwrap_or_else(|_| src.clone());
    if let Some(g) = w.bild_gemeldet.get(&el).filter(|g| srcset.is_some() || g.0 == quelle) {
        return Some(g.clone());
    }
    if !src.is_empty() {
        if let Some(g) = w.bilder_fertig.get(&quelle) {
            return Some((quelle, g.0, g.1));
        }
    }
    for teil in srcset.as_deref().unwrap_or("").split(',') {
        let Some(u) = teil.split_whitespace().next() else { continue };
        let u = narsil_url::loese_auf(&w.adresse, u).unwrap_or_else(|_| u.to_string());
        if let Some(g) = w.bilder_fertig.get(&u) {
            return Some((u, g.0, g.1));
        }
    }
    None
}

fn image_element(w: &mut crate::dom::Stand, el: u32) -> Result<Usable, String> {
    let Some((src, nw, nh)) = image_source(w, el) else { return Ok(Usable::Bad) };
    if nw == 0 || nh == 0 {
        return Err("InvalidStateError: The HTMLImageElement provided is in the 'broken' state.".into());
    }
    if let Some(c) = w.canvas.images.get(&src).cloned() {
        w.canvas.touch(0, &src, 0);
        return Ok(Usable::Good(c.image, c.natural, nc::colorspace::ColorSpace::Srgb));
    }
    let Some(px) = w.bild_pixel.get(&src).cloned() else { return Ok(Usable::Bad) };
    let Some(data) = px.pixels.upgrade() else { return Ok(Usable::Bad) };
    if data.len() < px.width as usize * px.height as usize * 4 || px.width == 0 || px.height == 0 {
        return Ok(Usable::Bad);
    }
    let mut pre = vec![0u8; px.width as usize * px.height as usize * 4];
    for (o, p) in pre.chunks_exact_mut(4).zip(data.chunks_exact(4)) {
        o.copy_from_slice(&nc::surface::premultiply(p));
    }
    let img = CachedImage { image: nc::Image { width: px.width, height: px.height, data: Arc::new(pre) }, natural: (nw, nh) };
    w.canvas.insert_cached(0, src, 0, img.clone());
    Ok(Usable::Good(img.image, img.natural, nc::colorspace::ColorSpace::Srgb))
}

/// Script-side view of decoded image pixels for `drawImage` on `<img>`.
#[derive(Clone, Debug)]
pub struct ImagePixels {
    pub width: u32,
    pub height: u32,
    pub pixels: std::sync::Weak<Vec<u8>>,
}

// --- the host function -----------------------------------------------------

pub(crate) fn surface<'a>(w: &'a mut crate::dom::Stand, id: u32) -> Result<&'a mut Surface, String> {
    w.canvas.surfaces.get_mut(&id).ok_or_else(|| "TypeError: Illegal invocation".to_string())
}

/// `__narsilC2d(op, id, ...)`.
pub fn host_call(w: &mut crate::dom::Stand, args: &[Wert]) -> Result<Wert, String> {
    let code = num(args, 0) as u32;
    let a = &args[1.min(args.len())..];
    match code {
        op::GET_CONTEXT => get_context(w, a),
        op::CREATE_OFFSCREEN => {
            let (width, height) = (num(a, 0), num(a, 1));
            let width = if width.is_finite() && width >= 0.0 { width.min(u32::MAX as f64) as u32 } else { 0 };
            let height = if height.is_finite() && height >= 0.0 { height.min(u32::MAX as f64) as u32 } else { 0 };
            if w.canvas.next_offscreen >= u32::MAX - OFFSCREEN_BASE - 1 {
                return Err("RangeError: too many OffscreenCanvas objects".into());
            }
            w.canvas.next_offscreen += 1;
            let id = OFFSCREEN_BASE + w.canvas.next_offscreen;
            let (fw, fh) = fit(width, height, w.canvas.total_pixels());
            let ctx = nc::Context2D::new(fw, fh, false).ok_or("RangeError: canvas too large")?;
            w.canvas.surfaces.insert(id, Surface { ctx, element: None, attrs: Attributes::default(), has_context: false, fill_token: 0.0, stroke_token: 0.0, saved_tokens: Vec::new(), placeholder: None });
            Ok(Wert::Zahl(id as f64))
        }
        op::RESIZE => {
            let id = num(a, 0) as u32;
            let (width, height) = (num(a, 1).max(0.0) as u32, num(a, 2).max(0.0) as u32);
            let others = w.canvas.total_pixels().saturating_sub(w.canvas.surfaces.get(&id).map_or(0, |s| s.ctx.width() as usize * s.ctx.height() as usize));
            let s = surface(w, id)?;
            let (fw, fh) = fit(width, height, others);
            s.ctx.resize(fw, fh);
            s.fill_token = 0.0;
            s.stroke_token = 0.0;
            s.saved_tokens.clear();
            Ok(Wert::Undefiniert)
        }
        op::SIZE => {
            let s = surface(w, num(a, 0) as u32)?;
            Ok(Wert::Feld(vec![Wert::Zahl(s.ctx.width() as f64), Wert::Zahl(s.ctx.height() as f64)]))
        }
        op::TO_DATA_URL => to_data_url(w, a),
        op::CONTEXT_ATTRS => {
            let s = surface(w, num(a, 0) as u32)?;
            let at = &s.attrs;
            Ok(Wert::Gegenstand(vec![
                ("alpha".into(), Wert::Wahr(at.alpha)),
                ("colorSpace".into(), Wert::Text(at.color_space.clone())),
                ("colorType".into(), Wert::Text(at.color_type.clone())),
                ("desynchronized".into(), Wert::Wahr(at.desynchronized)),
                ("willReadFrequently".into(), Wert::Wahr(at.will_read_frequently)),
                ("toneMappingMode".into(), Wert::Text(at.tone_mapping.clone())),
            ]))
        }
        op::SNAPSHOT => snapshot(w, a),
        op::ENCODE => {
            // (kind, ref, type, quality) -> [mime, bytes] | null for empty bitmaps
            let (width, height, px) = match source_ref(a, 0) {
                Some(SourceRef::Surface(id)) => {
                    let s = surface(w, id)?;
                    if s.ctx.layer_depth() > 0 {
                        return Err("InvalidStateError: The canvas has open layers.".into());
                    }
                    (s.ctx.width(), s.ctx.height(), export_srgb(&s.ctx))
                }
                Some(SourceRef::Element(el)) => match w.canvas.surfaces.get(&el) {
                    Some(s) if s.ctx.layer_depth() > 0 => return Err("InvalidStateError: The canvas has open layers.".into()),
                    Some(s) => (s.ctx.width(), s.ctx.height(), export_srgb(&s.ctx)),
                    None => {
                        let (cw, ch) = crate::dom::canvas_size(w, el);
                        if cw as usize * ch as usize > MAX_PIXELS {
                            return Ok(Wert::Null);
                        }
                        (cw, ch, vec![0; cw as usize * ch as usize * 4])
                    }
                },
                _ => return Err("TypeError: Illegal invocation".into()),
            };
            if width == 0 || height == 0 {
                return Ok(Wert::Null);
            }
            let (mime, bytes) = encode(width, height, px, &text(a, 2).to_ascii_lowercase(), num(a, 3));
            Ok(Wert::Feld(vec![Wert::Text(mime.into()), Wert::ByteFeld { daten: bytes, geklemmt: false }]))
        }
        op::DECODE => {
            let Some(Wert::ByteFeld { daten, .. }) = a.first() else { return Ok(Wert::Null) };
            match narsil_image::lesen(daten) {
                Ok(bild) if bild.breite > 0 && bild.hoehe > 0 && (bild.breite as usize * bild.hoehe as usize) <= MAX_PIXELS => {
                    let mut pre = vec![0u8; bild.rgba.len()];
                    for (o, p) in pre.chunks_exact_mut(4).zip(bild.rgba.chunks_exact(4)) {
                        o.copy_from_slice(&nc::surface::premultiply(p));
                    }
                    Ok(Wert::Feld(vec![Wert::Zahl(bild.breite as f64), Wert::Zahl(bild.hoehe as f64), Wert::ByteFeld { daten: pre, geklemmt: true }]))
                }
                _ => Ok(Wert::Null),
            }
        }
        op::SOURCE_INFO => {
            let src = source_ref(a, 0).ok_or("TypeError: invalid image source")?;
            match usable_source(w, &src)? {
                Usable::Good(_, (nw, nh), _) => Ok(Wert::Feld(vec![Wert::Zahl(nw as f64), Wert::Zahl(nh as f64)])),
                Usable::Bad => Ok(Wert::Null),
                Usable::NeedData => Ok(Wert::Text("need".into())),
            }
        }
        op::UPLOAD_BITMAP => {
            let token = num(a, 0) as u64;
            let (bw, bh) = (num(a, 1) as u32, num(a, 2) as u32);
            let Some(Wert::ByteFeld { daten, .. }) = a.get(3) else { return Err("TypeError: bitmap data missing".into()) };
            if daten.len() != bw as usize * bh as usize * 4 {
                return Err("TypeError: bitmap data has the wrong length".into());
            }
            let img = CachedImage { image: nc::Image { width: bw, height: bh, data: Arc::new(daten.clone()) }, natural: (bw, bh) };
            w.canvas.insert_cached(1, String::new(), token, img);
            Ok(Wert::Undefiniert)
        }
        op::PARSE_SVG_PATH => {
            // (unused id, path data)
            let cmds = nc::commands::parse_svg_path(&text(a, 1));
            Ok(Wert::Feld(cmds.into_iter().map(Wert::Zahl).collect()))
        }
        op::PARSE_COLOR => {
            let id = num(a, 0) as u32;
            let t = text(a, 1);
            let current = if w.canvas.surfaces.contains_key(&id) && t.to_ascii_lowercase().contains("currentcolor") { current_color(w, id) } else { nc::Color::BLACK };
            let c = parse_css_color_with(&t, current);
            Ok(c.map_or(Wert::Null, color_value))
        }
        _ => context_call(w, code, a),
    }
}

/// The space the bitmap is stored in. 8-bit canvases store their
/// `colorSpace`; float16 canvases keep 8-bit sRGB storage (there is no
/// float storage yet, and sRGB keeps sRGB content exact).
fn storage_space(attrs: &Attributes) -> nc::colorspace::ColorSpace {
    if attrs.color_type == "float16" {
        return nc::colorspace::ColorSpace::Srgb;
    }
    nc::colorspace::ColorSpace::parse(&attrs.color_space).unwrap_or_default()
}

fn get_context(w: &mut crate::dom::Stand, a: &[Wert]) -> Result<Wert, String> {
    // (element | surface id, alpha, colorSpace, willReadFrequently, desynchronized, colorType)
    let attrs = Attributes {
        alpha: !matches!(a.get(1), Some(Wert::Wahr(false))),
        color_space: nc::colorspace::ColorSpace::parse(&text(a, 2)).unwrap_or_default().name().into(),
        will_read_frequently: boolean(a, 3),
        desynchronized: boolean(a, 4),
        color_type: if text(a, 5) == "float16" { "float16".into() } else { "unorm8".into() },
        tone_mapping: if text(a, 6) == "extended" { "extended".into() } else { "standard".into() },
    };
    let id = match a.first() {
        Some(Wert::Zahl(n)) => *n as u32,
        Some(v) => {
            let el = narsil_jsvm::host_id(v).ok_or("TypeError: Illegal invocation")?;
            if element_tag(w, el).as_deref() != Some("canvas") {
                return Err("TypeError: Illegal invocation".into());
            }
            if !w.canvas.surfaces.contains_key(&el) {
                let (cw, ch) = crate::dom::canvas_size(w, el);
                let (fw, fh) = fit(cw, ch, w.canvas.total_pixels());
                let mut ctx = nc::Context2D::new(fw, fh, !attrs.alpha).ok_or("RangeError: canvas too large")?;
                ctx.color_space = storage_space(&attrs);
                w.canvas.surfaces.insert(el, Surface { ctx, element: Some(el), attrs: attrs.clone(), has_context: true, fill_token: 0.0, stroke_token: 0.0, saved_tokens: Vec::new(), placeholder: None });
            }
            el
        }
        None => return Err("TypeError: Illegal invocation".into()),
    };
    let s = surface(w, id)?;
    if !s.has_context {
        s.has_context = true;
        if !attrs.alpha {
            let (cw, ch) = (s.ctx.width(), s.ctx.height());
            if let Some(c) = nc::Context2D::new(cw, ch, true) {
                s.ctx = c;
            }
        }
        s.ctx.color_space = storage_space(&attrs);
        s.attrs = attrs;
    }
    Ok(Wert::Zahl(id as f64))
}

fn snapshot(w: &mut crate::dom::Stand, a: &[Wert]) -> Result<Wert, String> {
    // (kind, ref, sx, sy, sw, sh, premultiplied?) -> [w, h, bytes]
    let src = source_ref(a, 0).ok_or("TypeError: invalid image source")?;
    let (img, natural) = match usable_source(w, &src)? {
        Usable::Good(i, n, space) => (in_space(i, space, nc::colorspace::ColorSpace::Srgb), n),
        Usable::Bad => return Ok(Wert::Null),
        Usable::NeedData => return Ok(Wert::Text("need".into())),
    };
    let premul = boolean(a, 6);
    let (mut sx, mut sy, mut sw, mut sh) = (num(a, 2), num(a, 3), num(a, 4), num(a, 5));
    if !sw.is_finite() || !sh.is_finite() || !sx.is_finite() || !sy.is_finite() {
        sx = 0.0;
        sy = 0.0;
        sw = natural.0 as f64;
        sh = natural.1 as f64;
    }
    let (ow, oh) = (sw.abs().round() as u32, sh.abs().round() as u32);
    if ow as usize * oh as usize > MAX_PIXELS {
        return Err("RangeError: bitmap too large".into());
    }
    if sw < 0.0 {
        sx += sw;
    }
    if sh < 0.0 {
        sy += sh;
    }
    let mut ctx = nc::Context2D::new(ow, oh, false).ok_or("RangeError: bitmap too large")?;
    ctx.state.op = nc::CompositeOp::Copy;
    ctx.state.smoothing = false;
    let (kx, ky) = (img.width as f64 / natural.0.max(1) as f64, img.height as f64 / natural.1.max(1) as f64);
    ctx.draw_image(&img, sx * kx, sy * ky, ow as f64 * kx, oh as f64 * ky, 0.0, 0.0, ow as f64, oh as f64);
    let bytes = if premul { ctx.surface.data.as_ref().clone() } else { ctx.surface.to_unpremultiplied() };
    Ok(Wert::Feld(vec![Wert::Zahl(ow as f64), Wert::Zahl(oh as f64), Wert::ByteFeld { daten: bytes, geklemmt: true }]))
}

fn to_data_url(w: &mut crate::dom::Stand, a: &[Wert]) -> Result<Wert, String> {
    // (kind 0 element | 1 surface, ref, type, quality)
    let (width, height, mut px) = match source_ref(a, 0) {
        Some(SourceRef::Surface(id)) => {
            let s = surface(w, id)?;
            if s.ctx.layer_depth() > 0 {
                return Err("InvalidStateError: The canvas has open layers.".into());
            }
            (s.ctx.width(), s.ctx.height(), export_srgb(&s.ctx))
        }
        Some(SourceRef::Element(el)) => match w.canvas.surfaces.get(&el) {
            Some(s) if s.ctx.layer_depth() > 0 => return Err("InvalidStateError: The canvas has open layers.".into()),
            Some(s) => (s.ctx.width(), s.ctx.height(), export_srgb(&s.ctx)),
            None => {
                let (cw, ch) = crate::dom::canvas_size(w, el);
                if cw as usize * ch as usize > MAX_PIXELS {
                    return Ok(Wert::Text("data:,".into()));
                }
                (cw, ch, vec![0; cw as usize * ch as usize * 4])
            }
        },
        _ => return Err("TypeError: Illegal invocation".into()),
    };
    if width == 0 || height == 0 {
        return Ok(Wert::Text("data:,".into()));
    }
    // Release 08.10.2026 (feature 3): a noised copy is encoded, the canvas
    // itself stays unchanged.
    if crate::features::netz::rauschen_an() {
        let host = crate::features::host_von(&w.adresse);
        crate::features::netz::verrauschen(&mut px, width, 0, 0, &host);
    }
    let kind = text(a, 2).to_ascii_lowercase();
    let quality = num(a, 3);
    let (mime, bytes) = encode(width, height, px, &kind, quality);
    Ok(Wert::Text(data_url(mime, &bytes)))
}

/// Encodes un-premultiplied pixels; unsupported types fall back to PNG.
#[must_use]
pub fn encode(width: u32, height: u32, rgba: Vec<u8>, kind: &str, quality: f64) -> (&'static str, Vec<u8>) {
    let bild = narsil_image::Bild { breite: width, hoehe: height, rgba };
    if kind == "image/jpeg" && width <= 65535 && height <= 65535 {
        // HTML: a quality outside 0..=1 (or not a number) means the default.
        let q = if (0.0..=1.0).contains(&quality) { quality } else { 0.92 };
        return ("image/jpeg", narsil_image::jpeg_schreiben(&bild, (q * 100.0).round().clamp(1.0, 100.0) as u8));
    }
    ("image/png", narsil_image::png_schreiben(&bild))
}

#[must_use]
pub fn data_url(mime: &str, bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len() * 4 / 3 + 32);
    result.push_str("data:");
    result.push_str(mime);
    result.push_str(";base64,");
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (chunk.get(1).copied().unwrap_or(0) as u32) << 8 | chunk.get(2).copied().unwrap_or(0) as u32;
        result.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        result.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        result.push(if chunk.len() > 1 { ALPHABET[((n >> 6) & 63) as usize] as char } else { '=' });
        result.push(if chunk.len() > 2 { ALPHABET[(n & 63) as usize] as char } else { '=' });
    }
    result
}

/// Replays a `Path2D` command list (if given) under the current transform.
fn script_path(s: &Surface, v: Option<&Wert>) -> Result<Option<nc::Path>, String> {
    let Some(cmds) = numbers(v) else { return Ok(None) };
    let mut p = nc::Path::new();
    nc::commands::replay(&cmds, &s.ctx.state.transform, &mut p).map_err(path_error)?;
    Ok(Some(p))
}

fn context_call(w: &mut crate::dom::Stand, code: u32, a: &[Wert]) -> Result<Wert, String> {
    let id = match a.first() {
        Some(Wert::Zahl(n)) if *n >= 0.0 => *n as u32,
        _ => return Err("TypeError: Illegal invocation".into()),
    };
    let n = |i: usize| num(a, i + 1);
    // Values that need the whole state (colors, sources) first.
    match code {
        op::SET => return set_attribute(w, id, &text(a, 1), a.get(2)),
        op::DRAW_IMAGE => return draw_image(w, id, a),
        op::SET_PATTERN => return set_pattern(w, id, a),
        op::BEGIN_LAYER => {
            // (id, filter string | undefined, CanvasFilter spec | undefined)
            surface(w, id)?;
            let filter = match (a.get(1), numbers(a.get(2))) {
                (_, Some(spec)) => nc::filter::Filter::from_spec(&spec).filter(|f| !f.is_empty()).map(Arc::new),
                (Some(Wert::Text(t)), None) => {
                    let color = current_color(w, id);
                    crate::canvas_text::parse_filter(t, color).flatten().map(Arc::new)
                }
                _ => None,
            };
            let s = surface(w, id)?;
            s.ctx.begin_layer(filter);
            s.saved_tokens.push((s.fill_token, s.stroke_token));
            return Ok(Wert::Undefiniert);
        }
        op::LINK_PLACEHOLDER => {
            // (offscreen id, placeholder canvas element)
            let el = a.get(1).and_then(narsil_jsvm::host_id);
            let s = surface(w, id)?;
            s.placeholder = el;
            s.ctx.dirty = true;
            return Ok(Wert::Undefiniert);
        }
        op::SET_FILTER_SPEC => {
            // (id, token, spec): `ctx.filter = new CanvasFilter(...)`.
            let token = num(a, 1);
            let filter = numbers(a.get(2)).and_then(|spec| nc::filter::Filter::from_spec(&spec)).filter(|f| !f.is_empty());
            let s = surface(w, id)?;
            s.ctx.state.filter = format!("\u{1}{token}");
            s.ctx.state.filter_ops = filter.map(Arc::new);
            return Ok(Wert::Undefiniert);
        }
        op::FILL_TEXT | op::STROKE_TEXT => return crate::canvas_text::draw_text(w, id, a, code == op::STROKE_TEXT),
        op::MEASURE_TEXT => return crate::canvas_text::measure_text(w, id, &text(a, 1)),
        _ => {}
    }
    let rauschen = code == op::GET_IMAGE_DATA && crate::features::netz::rauschen_an();
    let adresse = if rauschen { w.adresse.clone() } else { String::new() };
    let s = surface(w, id)?;
    let ctx = &mut s.ctx;
    let m = ctx.state.transform;
    match code {
        op::SAVE => {
            ctx.save();
            s.saved_tokens.push((s.fill_token, s.stroke_token));
        }
        op::RESTORE => {
            let popped = ctx.restore().map_err(|_| "InvalidStateError: Cannot restore the state of an open layer; call endLayer() first.".to_string())?;
            if popped {
                if let Some((f, st)) = s.saved_tokens.pop() {
                    s.fill_token = f;
                    s.stroke_token = st;
                }
            }
        }
        op::END_LAYER => {
            s.ctx.end_layer().map_err(|_| "InvalidStateError: No layer to end, or unmatched save() calls inside the layer.".to_string())?;
            if let Some((f, st)) = s.saved_tokens.pop() {
                s.fill_token = f;
                s.stroke_token = st;
            }
        }
        op::RESET => {
            ctx.reset();
            s.fill_token = 0.0;
            s.stroke_token = 0.0;
            s.saved_tokens.clear();
        }
        op::GET => return Ok(get_attribute(s, &text(a, 1))),
        op::SET_LINE_DASH => {
            let Some(list) = numbers(a.get(1)) else { return Ok(Wert::Undefiniert) };
            if list.iter().any(|v| !v.is_finite() || *v < 0.0) {
                return Ok(Wert::Undefiniert);
            }
            let mut list = list;
            if list.len() % 2 == 1 {
                let copy = list.clone();
                list.extend(copy);
            }
            ctx.state.line.dash = list;
        }
        op::GET_LINE_DASH => return Ok(Wert::Feld(ctx.state.line.dash.iter().map(|v| Wert::Zahl(*v)).collect())),
        op::SET_GRADIENT => {
            // (which, token, kind, c0..c5, stops[offset r g b a legacy ...])
            let which = n(0) as u32;
            let token = n(1);
            let kind = match n(2) as u32 {
                0 => nc::GradientKind::Linear { x0: n(3), y0: n(4), x1: n(5), y1: n(6) },
                1 => nc::GradientKind::Radial { x0: n(3), y0: n(4), r0: n(5), x1: n(6), y1: n(7), r1: n(8) },
                _ => nc::GradientKind::Conic { angle: n(3), x: n(4), y: n(5) },
            };
            let mut g = nc::Gradient::new(kind);
            let mut non_legacy = false;
            if let Some(stops) = numbers(a.get(10)) {
                for c in stops.chunks_exact(6) {
                    g.stops.push(nc::paint::ColorStop { offset: c[0] as f32, color: nc::Color::rgba(c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32) });
                    non_legacy |= c[5] == 0.0;
                }
            }
            g.choose_interpolation(non_legacy);
            let style = nc::Style::Gradient(Arc::new(g));
            if which == 0 {
                ctx.state.fill = style;
                s.fill_token = token;
            } else {
                ctx.state.stroke = style;
                s.stroke_token = token;
            }
        }
        op::TRANSLATE => ctx.transform(nc::Matrix::translate(n(0), n(1))),
        op::SCALE => ctx.transform(nc::Matrix::scale(n(0), n(1))),
        op::ROTATE => {
            if n(0).is_finite() {
                ctx.transform(nc::Matrix::rotate(n(0)));
            }
        }
        op::TRANSFORM => ctx.transform(nc::Matrix::new(n(0), n(1), n(2), n(3), n(4), n(5))),
        op::SET_TRANSFORM => ctx.set_transform(nc::Matrix::new(n(0), n(1), n(2), n(3), n(4), n(5))),
        op::RESET_TRANSFORM => ctx.state.transform = nc::Matrix::IDENTITY,
        op::GET_TRANSFORM => return Ok(Wert::Feld(m.as_array().iter().map(|v| Wert::Zahl(*v)).collect())),
        op::BEGIN_PATH => ctx.begin_path(),
        op::MOVE_TO => ctx.path.move_to(&m, n(0), n(1)),
        op::LINE_TO => ctx.path.line_to(&m, n(0), n(1)),
        op::QUAD_TO => ctx.path.quad_to(&m, n(0), n(1), n(2), n(3)),
        op::CUBIC_TO => ctx.path.cubic_to(&m, n(0), n(1), n(2), n(3), n(4), n(5)),
        op::ARC_TO => ctx.path.arc_to(&m, n(0), n(1), n(2), n(3), n(4)).map_err(path_error)?,
        op::ARC => ctx.path.ellipse(&m, n(0), n(1), n(2), n(2), 0.0, n(3), n(4), boolean(a, 6)).map_err(path_error)?,
        op::ELLIPSE => ctx.path.ellipse(&m, n(0), n(1), n(2), n(3), n(4), n(5), n(6), boolean(a, 8)).map_err(path_error)?,
        op::RECT => ctx.path.rect(&m, n(0), n(1), n(2), n(3)),
        op::ROUND_RECT => {
            let r = numbers(a.get(5)).unwrap_or_default();
            if r.len() == 8 {
                ctx.path.round_rect(&m, n(0), n(1), n(2), n(3), [(r[0], r[1]), (r[2], r[3]), (r[4], r[5]), (r[6], r[7])]);
            }
        }
        op::CLOSE_PATH => ctx.path.close(),
        op::FILL => {
            let p = script_path(s, a.get(2))?;
            s.ctx.fill(p.as_ref(), rule(a, 1));
        }
        op::STROKE => {
            let p = script_path(s, a.get(1))?;
            s.ctx.stroke(p.as_ref());
        }
        op::CLIP => {
            let p = script_path(s, a.get(2))?;
            s.ctx.clip(p.as_ref(), rule(a, 1));
        }
        op::IN_PATH => {
            let p = script_path(s, a.get(4))?;
            return Ok(Wert::Wahr(s.ctx.is_point_in_path(p.as_ref(), n(0), n(1), rule(a, 3))));
        }
        op::IN_STROKE => {
            let p = script_path(s, a.get(3))?;
            return Ok(Wert::Wahr(s.ctx.is_point_in_stroke(p.as_ref(), n(0), n(1))));
        }
        op::FILL_RECT => ctx.fill_rect(n(0), n(1), n(2), n(3)),
        op::STROKE_RECT => ctx.stroke_rect(n(0), n(1), n(2), n(3)),
        op::CLEAR_RECT => ctx.clear_rect(n(0), n(1), n(2), n(3)),
        op::GET_IMAGE_DATA => {
            if ctx.layer_depth() > 0 {
                return Err("InvalidStateError: getImageData() cannot be called while layers are open.".into());
            }
            let (sx, sy, sw, sh) = (n(0), n(1), n(2), n(3));
            let (mut x, mut y) = (sx as i64, sy as i64);
            if sw < 0.0 {
                x += sw as i64;
            }
            if sh < 0.0 {
                y += sh as i64;
            }
            let (gw, gh) = (sw.abs() as u64, sh.abs() as u64);
            if gw == 0 || gh == 0 {
                return Err("IndexSizeError: The source width or height is 0.".into());
            }
            if gw.saturating_mul(gh) > MAX_PIXELS as u64 {
                return Err("RangeError: Out of memory at ImageData creation".into());
            }
            let mut data = ctx.get_image_data(x, y, gw as u32, gh as u32);
            // (id, sx, sy, sw, sh, colorSpace): from the storage space.
            if let Some(cs) = nc::colorspace::ColorSpace::parse(&text(a, 5)) {
                nc::colorspace::convert(&mut data, ctx.color_space, cs);
            }
            // Release 08.10.2026 (feature 3): the same fixed noise as toDataURL.
            if rauschen {
                let host = crate::features::host_von(&adresse);
                crate::features::netz::verrauschen(&mut data, gw as u32, x, y, &host);
            }
            return Ok(Wert::ByteFeld { daten: data, geklemmt: true });
        }
        op::PUT_IMAGE_DATA => {
            // (bytes, w, h, dx, dy, dirtyX, dirtyY, dirtyW, dirtyH)
            if ctx.layer_depth() > 0 {
                return Err("InvalidStateError: putImageData() cannot be called while layers are open.".into());
            }
            let Some(Wert::ByteFeld { daten, .. }) = a.get(1) else { return Err("TypeError: ImageData expected".into()) };
            let (iw, ih) = (n(1).max(0.0) as u32, n(2).max(0.0) as u32);
            if daten.len() < iw as usize * ih as usize * 4 {
                return Err("InvalidStateError: The ImageData has been detached.".into());
            }
            let (dx, dy) = (n(3) as i64, n(4) as i64);
            let has_dirty = matches!(a.get(6), Some(Wert::Zahl(_)));
            let (mut x, mut y, mut dw, mut dh) = if has_dirty { (n(5) as i64, n(6) as i64, n(7) as i64, n(8) as i64) } else { (0, 0, iw as i64, ih as i64) };
            if dw < 0 {
                x += dw;
                dw = -dw;
            }
            if dh < 0 {
                y += dh;
                dh = -dh;
            }
            if x < 0 {
                dw += x;
                x = 0;
            }
            if y < 0 {
                dh += y;
                y = 0;
            }
            if x + dw > iw as i64 {
                dw = iw as i64 - x;
            }
            if y + dh > ih as i64 {
                dh = ih as i64 - y;
            }
            if dw > 0 && dh > 0 {
                let rect = nc::IRect::new(x as i32, y as i32, (x + dw) as i32, (y + dh) as i32);
                let storage = ctx.color_space;
                match nc::colorspace::ColorSpace::parse(&text(a, 10)).filter(|cs| *cs != storage) {
                    Some(cs) => {
                        let mut copy = daten.clone();
                        nc::colorspace::convert(&mut copy, cs, storage);
                        ctx.put_image_data(&copy, iw, dx, dy, rect);
                    }
                    None => ctx.put_image_data(daten, iw, dx, dy, rect),
                }
            }
        }
        _ => return Err(format!("TypeError: unknown canvas operation {code}")),
    }
    Ok(Wert::Undefiniert)
}

fn style_value(style: &nc::Style, token: f64) -> Wert {
    match style {
        nc::Style::Color(c) => Wert::Text(c.serialize()),
        _ => Wert::Zahl(token),
    }
}

fn get_attribute(s: &Surface, name: &str) -> Wert {
    let st = &s.ctx.state;
    match name {
        "fillStyle" => style_value(&st.fill, s.fill_token),
        "strokeStyle" => style_value(&st.stroke, s.stroke_token),
        "globalAlpha" => Wert::Zahl(st.global_alpha),
        "globalCompositeOperation" => Wert::Text(st.op.name().into()),
        "lineWidth" => Wert::Zahl(st.line.width),
        "lineCap" => Wert::Text(st.line.cap.name().into()),
        "lineJoin" => Wert::Text(st.line.join.name().into()),
        "miterLimit" => Wert::Zahl(st.line.miter_limit),
        "lineDashOffset" => Wert::Zahl(st.line.dash_offset),
        "shadowOffsetX" => Wert::Zahl(st.shadow_offset_x),
        "shadowOffsetY" => Wert::Zahl(st.shadow_offset_y),
        "shadowBlur" => Wert::Zahl(st.shadow_blur),
        "shadowColor" => Wert::Text(st.shadow_color.serialize()),
        "imageSmoothingEnabled" => Wert::Wahr(st.smoothing),
        "imageSmoothingQuality" => Wert::Text(st.smoothing_quality.name().into()),
        "filter" => Wert::Text(st.filter.clone()),
        "font" => Wert::Text(st.text.font.clone()),
        "textAlign" => Wert::Text(st.text.text_align.clone()),
        "textBaseline" => Wert::Text(st.text.text_baseline.clone()),
        "direction" => Wert::Text(st.text.direction.clone()),
        "letterSpacing" => Wert::Text(st.text.letter_spacing.clone()),
        "wordSpacing" => Wert::Text(st.text.word_spacing.clone()),
        "fontKerning" => Wert::Text(st.text.font_kerning.clone()),
        "fontStretch" => Wert::Text(st.text.font_stretch.clone()),
        "fontVariantCaps" => Wert::Text(st.text.font_variant_caps.clone()),
        "textRendering" => Wert::Text(st.text.text_rendering.clone()),
        "lang" => Wert::Text(st.text.lang.clone()),
        "width" => Wert::Zahl(s.ctx.width() as f64),
        "height" => Wert::Zahl(s.ctx.height() as f64),
        _ => Wert::Undefiniert,
    }
}

fn finite(v: Option<&Wert>) -> Option<f64> {
    match v {
        Some(Wert::Zahl(n)) if n.is_finite() => Some(*n),
        _ => None,
    }
}

fn set_attribute(w: &mut crate::dom::Stand, id: u32, name: &str, value: Option<&Wert>) -> Result<Wert, String> {
    let t = value.map(|v| v.als_text()).unwrap_or_default();
    // Colors may need the element's computed style.
    let color = match name {
        "fillStyle" | "strokeStyle" | "shadowColor" => {
            surface(w, id)?;
            color_for(w, id, &t)
        }
        _ => None,
    };
    // Relative font sizes resolve against a connected canvas element's
    // font size, otherwise against the default 10px.
    let font = if name == "font" {
        let el = surface(w, id)?.element;
        let connected = el.is_some_and(|e| w.lebend.as_ref().is_some_and(|l| l.knoten(e).is_some_and(|k| l.ist_verbunden(k))));
        let px = |v: Option<String>| v.and_then(|v| v.trim().strip_suffix("px").and_then(|n| n.trim().parse::<f64>().ok()));
        let (parent, parent_lh) = match el.filter(|_| connected) {
            Some(e) => {
                let size = px(crate::dom::computed_value(w, e, "font-size")).unwrap_or(16.0);
                let lh = px(crate::dom::computed_value(w, e, "line-height")).unwrap_or(size * 1.2);
                (size, lh)
            }
            None => (10.0, 12.0),
        };
        crate::canvas_text::parse_font_in(&t, parent, parent_lh)
    } else {
        None
    };
    let filter = if name == "filter" {
        surface(w, id)?;
        let cc = current_color(w, id);
        crate::canvas_text::parse_filter(&t, cc)
    } else {
        None
    };
    let s = surface(w, id)?;
    let st = &mut s.ctx.state;
    match name {
        "fillStyle" => {
            if let Some(c) = color {
                st.fill = nc::Style::Color(c);
                s.fill_token = 0.0;
            }
        }
        "strokeStyle" => {
            if let Some(c) = color {
                st.stroke = nc::Style::Color(c);
                s.stroke_token = 0.0;
            }
        }
        "shadowColor" => {
            if let Some(c) = color {
                st.shadow_color = c;
            }
        }
        "globalAlpha" => {
            if let Some(v) = finite(value).filter(|v| (0.0..=1.0).contains(v)) {
                st.global_alpha = v;
            }
        }
        "globalCompositeOperation" => {
            if let Some(o) = nc::CompositeOp::parse(&t) {
                st.op = o;
            }
        }
        "lineWidth" => {
            if let Some(v) = finite(value).filter(|v| *v > 0.0) {
                st.line.width = v;
            }
        }
        "miterLimit" => {
            if let Some(v) = finite(value).filter(|v| *v > 0.0) {
                st.line.miter_limit = v;
            }
        }
        "lineDashOffset" => {
            if let Some(v) = finite(value) {
                st.line.dash_offset = v;
            }
        }
        "lineCap" => {
            if let Some(c) = nc::LineCap::parse(&t) {
                st.line.cap = c;
            }
        }
        "lineJoin" => {
            if let Some(j) = nc::LineJoin::parse(&t) {
                st.line.join = j;
            }
        }
        "shadowOffsetX" => {
            if let Some(v) = finite(value) {
                st.shadow_offset_x = v;
            }
        }
        "shadowOffsetY" => {
            if let Some(v) = finite(value) {
                st.shadow_offset_y = v;
            }
        }
        "shadowBlur" => {
            if let Some(v) = finite(value).filter(|v| *v >= 0.0) {
                st.shadow_blur = v;
            }
        }
        "imageSmoothingEnabled" => st.smoothing = boolean(value.map(std::slice::from_ref).unwrap_or(&[]), 0),
        "imageSmoothingQuality" => {
            if let Some(q) = nc::SmoothingQuality::parse(&t) {
                st.smoothing_quality = q;
            }
        }
        "textAlign" => {
            if matches!(t.as_str(), "start" | "end" | "left" | "right" | "center") {
                st.text.text_align = t;
            }
        }
        "textBaseline" => {
            if matches!(t.as_str(), "top" | "hanging" | "middle" | "alphabetic" | "ideographic" | "bottom") {
                st.text.text_baseline = t;
            }
        }
        "direction" => {
            if matches!(t.as_str(), "ltr" | "rtl" | "inherit") {
                st.text.direction = t;
            }
        }
        "fontKerning" => {
            if matches!(t.as_str(), "auto" | "normal" | "none") {
                st.text.font_kerning = t;
            }
        }
        "fontStretch" => {
            if matches!(
                t.as_str(),
                "ultra-condensed" | "extra-condensed" | "condensed" | "semi-condensed" | "normal" | "semi-expanded" | "expanded" | "extra-expanded" | "ultra-expanded"
            ) {
                st.text.font_stretch = t;
            }
        }
        "fontVariantCaps" => {
            if matches!(t.as_str(), "normal" | "small-caps" | "all-small-caps" | "petite-caps" | "all-petite-caps" | "unicase" | "titling-caps") {
                st.text.font_variant_caps = t;
            }
        }
        "textRendering" => {
            if matches!(t.as_str(), "auto" | "optimizeSpeed" | "optimizeLegibility" | "geometricPrecision") {
                st.text.text_rendering = t;
            }
        }
        "lang" => st.text.lang = t,
        "letterSpacing" | "wordSpacing" => {
            if let Some((px, ser)) = spacing(&t) {
                if name == "letterSpacing" {
                    st.text.letter_spacing = ser;
                    st.text.letter_spacing_px = px;
                } else {
                    st.text.word_spacing = ser;
                    st.text.word_spacing_px = px;
                }
            }
        }
        "font" => {
            // The shorthand also sets font-variant-caps and font-stretch.
            if let Some(f) = font {
                st.text.font = f.serialize();
                st.text.font_variant_caps = f.variant;
                st.text.font_stretch = f.stretch;
            }
        }
        "filter" => {
            if let Some(f) = filter {
                st.filter = t;
                st.filter_ops = f.map(Arc::new);
            }
        }
        _ => {}
    }
    Ok(Wert::Undefiniert)
}

/// `letterSpacing`/`wordSpacing`: a CSS `<length>` (no percentages).
fn spacing(t: &str) -> Option<(f64, String)> {
    // Validate with the default font size; the pixels are recomputed from
    // the stored string against the current font when text is shaped.
    let (v, unit) = crate::canvas_text::split_length(t)?;
    let px = crate::canvas_text::length_in_px(v, &unit, 10.0)?;
    let ser_unit = if unit.is_empty() { "px".to_string() } else { unit };
    Some((px, format!("{}{}", crate::canvas_text::css_number(v), ser_unit)))
}

fn set_pattern(w: &mut crate::dom::Stand, id: u32, a: &[Wert]) -> Result<Wert, String> {
    // (id, which, token, repetition, matrix[6], width, height, bytes?)
    let which = num(a, 1) as u32;
    let token = num(a, 2);
    let repetition = nc::Repetition::parse(&text(a, 3)).unwrap_or_default();
    let mt = numbers(a.get(4)).filter(|v| v.len() == 6).unwrap_or_else(|| vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let key = token as u64;
    let image = match a.get(7) {
        Some(Wert::ByteFeld { daten, .. }) => {
            let (pw, ph) = (num(a, 5) as u32, num(a, 6) as u32);
            if daten.len() != pw as usize * ph as usize * 4 {
                return Err("TypeError: pattern data has the wrong length".into());
            }
            let img = CachedImage { image: nc::Image { width: pw, height: ph, data: Arc::new(daten.clone()) }, natural: (pw, ph) };
            w.canvas.insert_cached(1, String::new(), key, img.clone());
            img.image
        }
        _ => match w.canvas.bitmaps.get(&key).cloned() {
            Some(c) => {
                w.canvas.touch(1, "", key);
                c.image
            }
            None => return Ok(Wert::Text("need".into())),
        },
    };
    let s = surface(w, id)?;
    let image = in_space(image, nc::colorspace::ColorSpace::Srgb, s.ctx.color_space);
    let pattern = nc::Pattern { image, repetition, transform: nc::Matrix::new(mt[0], mt[1], mt[2], mt[3], mt[4], mt[5]) };
    let style = nc::Style::Pattern(Arc::new(pattern));
    if which == 0 {
        s.ctx.state.fill = style;
        s.fill_token = token;
    } else {
        s.ctx.state.stroke = style;
        s.stroke_token = token;
    }
    Ok(Wert::Wahr(true))
}

/// Premultiplied pixels converted between canvas color spaces.
fn in_space(img: nc::Image, from: nc::colorspace::ColorSpace, to: nc::colorspace::ColorSpace) -> nc::Image {
    if from == to {
        return img;
    }
    let mut data = img.data.as_ref().clone();
    nc::colorspace::convert_premultiplied(&mut data, from, to);
    nc::Image { width: img.width, height: img.height, data: Arc::new(data) }
}

fn draw_image(w: &mut crate::dom::Stand, id: u32, a: &[Wert]) -> Result<Wert, String> {
    // (id, kind, ref, sx, sy, sw, sh, dx, dy, dw, dh, argc)
    let src = source_ref(a, 1).ok_or("TypeError: invalid image source")?;
    let (img, natural, space) = match usable_source(w, &src)? {
        Usable::Good(i, n, space) => (i, n, space),
        Usable::Bad => return Ok(Wert::Undefiniert),
        Usable::NeedData => return Ok(Wert::Text("need".into())),
    };
    let target = surface(w, id)?.ctx.color_space;
    let img = in_space(img, space, target);
    let g = |i: usize| num(a, i);
    let argc = g(11) as u32;
    let (nw, nh) = (natural.0 as f64, natural.1 as f64);
    let (sx, sy, sw, sh, dx, dy, dw, dh) = match argc {
        3 => (0.0, 0.0, nw, nh, g(3), g(4), nw, nh),
        5 => (0.0, 0.0, nw, nh, g(3), g(4), g(5), g(6)),
        _ => (g(3), g(4), g(5), g(6), g(7), g(8), g(9), g(10)),
    };
    // The buffer may be smaller than the natural size (image diet).
    let (kx, ky) = (img.width as f64 / nw.max(1.0), img.height as f64 / nh.max(1.0));
    let s = surface(w, id)?;
    s.ctx.draw_image(&img, sx * kx, sy * ky, sw * kx, sh * ky, dx, dy, dw, dh);
    Ok(Wert::Undefiniert)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn css(s: &str) -> Option<[u8; 4]> {
        parse_css_color(s).map(|c| c.to_rgba8())
    }

    #[test]
    fn legacy_color_syntax_is_strict() {
        assert_eq!(css("hsl(0, 100.%, 50%)"), None);
        assert_eq!(css("rgb(100%, 0, 0)"), None);
        assert_eq!(css("rgba(100%, 0, 0, 1)"), None);
        assert_eq!(css("rgba(255, 0, 0, 1.)"), None);
        assert_eq!(css("rgb(100%, 0%, 0%)"), Some([255, 0, 0, 255]));
        assert_eq!(css("rgb(.5e1, 0, 0)"), Some([5, 0, 0, 255]));
        assert_eq!(css("hsl(120, 100%, 50%)"), Some([0, 255, 0, 255]));
    }

    #[test]
    fn open_functions_close_at_the_end() {
        assert_eq!(css("rgb(0, 255, 0"), Some([0, 255, 0, 255]));
        assert_eq!(css("rgba(0, 255, 0, 1"), Some([0, 255, 0, 255]));
    }

    #[test]
    fn deprecated_system_colors_resolve() {
        assert_eq!(css("ThreeDDarkShadow"), Some([118, 118, 118, 255]));
        // Canvas colors match the page: system colors the CSS engine knows
        // come from its table (no canvas WPT test pins `Highlight`); the
        // canvas table only covers the ones it lacks.
        let page = narsil_css::farbe_aus("highlight").map(|f| [f.r, f.g, f.b, f.a]);
        assert_eq!(css("Highlight"), page.or(Some([0, 117, 255, 255])));
    }
}
