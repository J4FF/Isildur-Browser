//! The 2D context: drawing state, the state stack, the current default path
//! and every drawing operation of `CanvasRenderingContext2D` that touches
//! pixels.

use crate::composite::CompositeOp;
use crate::geom::{IRect, Matrix};
use crate::paint::{Color, Gradient, Image, ImageSource, Paint, Pattern, Shader, Smoothing};
use crate::path::Path;
use crate::raster::{FillRule, Line, Rasterizer};
use crate::stroke::{Polyline, StrokeStyle, Stroker};
use crate::surface::{Clip, Composite, Shadow, Surface};
use std::sync::Arc;

/// Maximum depth of the `save()` stack (memory guard).
pub const MAX_STACK: usize = 4096;

/// Pixels a layer may cover beyond the canvas size (filters that pull
/// content in from outside the canvas).
const LAYER_PIXELS: u64 = 2_500_000;

/// A fill or stroke style.
#[derive(Clone, Debug, PartialEq)]
pub enum Style {
    Color(Color),
    Gradient(Arc<Gradient>),
    Pattern(Arc<Pattern>),
}

impl Default for Style {
    fn default() -> Self {
        Style::Color(Color::BLACK)
    }
}

/// `imageSmoothingQuality`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SmoothingQuality {
    #[default]
    Low,
    Medium,
    High,
}

impl SmoothingQuality {
    #[must_use]
    pub fn parse(s: &str) -> Option<SmoothingQuality> {
        Some(match s {
            "low" => SmoothingQuality::Low,
            "medium" => SmoothingQuality::Medium,
            "high" => SmoothingQuality::High,
            _ => return None,
        })
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            SmoothingQuality::Low => "low",
            SmoothingQuality::Medium => "medium",
            SmoothingQuality::High => "high",
        }
    }
}

/// The text attributes of the drawing state. The strings are the values
/// the attributes return; the bridge parses them.
#[derive(Clone, Debug, PartialEq)]
pub struct TextState {
    pub font: String,
    pub text_align: String,
    pub text_baseline: String,
    pub direction: String,
    pub letter_spacing: String,
    pub letter_spacing_px: f64,
    pub word_spacing: String,
    pub word_spacing_px: f64,
    pub font_kerning: String,
    pub font_stretch: String,
    pub font_variant_caps: String,
    pub text_rendering: String,
    pub lang: String,
}

impl Default for TextState {
    fn default() -> Self {
        TextState {
            font: "10px sans-serif".into(),
            text_align: "start".into(),
            text_baseline: "alphabetic".into(),
            direction: "inherit".into(),
            letter_spacing: "0px".into(),
            letter_spacing_px: 0.0,
            word_spacing: "0px".into(),
            word_spacing_px: 0.0,
            font_kerning: "auto".into(),
            font_stretch: "normal".into(),
            font_variant_caps: "normal".into(),
            text_rendering: "auto".into(),
            lang: "inherit".into(),
        }
    }
}

/// The drawing state (HTML §4.12.5.1.2): everything `save()` stores.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub transform: Matrix,
    pub fill: Style,
    pub stroke: Style,
    pub global_alpha: f64,
    pub op: CompositeOp,
    pub line: StrokeStyle,
    pub shadow_offset_x: f64,
    pub shadow_offset_y: f64,
    pub shadow_blur: f64,
    pub shadow_color: Color,
    pub smoothing: bool,
    pub smoothing_quality: SmoothingQuality,
    pub filter: String,
    /// The parsed `filter` (`None` for `none`).
    pub filter_ops: Option<Arc<crate::filter::Filter>>,
    pub text: TextState,
    pub clip: Option<Arc<Clip>>,
}

impl Default for State {
    fn default() -> Self {
        State {
            transform: Matrix::IDENTITY,
            fill: Style::default(),
            stroke: Style::default(),
            global_alpha: 1.0,
            op: CompositeOp::SourceOver,
            line: StrokeStyle::default(),
            shadow_offset_x: 0.0,
            shadow_offset_y: 0.0,
            shadow_blur: 0.0,
            shadow_color: Color::TRANSPARENT,
            smoothing: true,
            smoothing_quality: SmoothingQuality::Low,
            filter: "none".into(),
            filter_ops: None,
            text: TextState::default(),
            clip: None,
        }
    }
}

/// An open `beginLayer()`: the bitmap underneath and the state that
/// composites the layer at `endLayer()`.
struct LayerFrame {
    parent: Surface,
    /// The parent's further tiles (a layer opened inside a tiled layer).
    parent_tiles: Vec<Surface>,
    state: State,
    filter: Option<Arc<crate::filter::Filter>>,
    stack_len: usize,
    /// Opened under a non-invertible transform: nothing of it is drawn.
    hidden: bool,
}

/// Errors of the state stack operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackError {
    /// `InvalidStateError`: a layer is in the way, or there is none.
    InvalidState,
}

pub struct Context2D {
    pub surface: Surface,
    pub state: State,
    stack: Vec<State>,
    layers: Vec<LayerFrame>,
    pub path: Path,
    raster: Rasterizer,
    /// Bumped whenever the bitmap changes (display refresh).
    pub dirty: bool,
    /// The color space the bitmap stores (`colorSpace` context setting);
    /// colors are converted into it when they are painted.
    pub color_space: crate::colorspace::ColorSpace,
    /// Further areas of the current layer: a layer whose drop-shadow pulls
    /// content in from far outside the canvas covers the canvas and the
    /// area the shadow comes from, instead of everything in between.
    /// Every draw goes to `surface` and to these tiles.
    tiles: Vec<Surface>,
}

impl Context2D {
    #[must_use]
    pub fn new(width: u32, height: u32, opaque: bool) -> Option<Context2D> {
        Some(Context2D {
            surface: Surface::new(width, height, opaque)?,
            state: State::default(),
            stack: Vec::new(),
            layers: Vec::new(),
            path: Path::new(),
            raster: Rasterizer::new(),
            dirty: true,
            color_space: crate::colorspace::ColorSpace::Srgb,
            tiles: Vec::new(),
        })
    }

    /// The bitmap's size (open layers may cover a different area).
    #[must_use]
    pub fn width(&self) -> u32 {
        self.layers.first().map_or(self.surface.width, |f| f.parent.width)
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.layers.first().map_or(self.surface.height, |f| f.parent.height)
    }

    /// The device area a new layer must cover: where its result can land
    /// (the current target within the clip), widened by how far the
    /// layer's filters and shadow pull content in from outside.
    fn layer_domain(&self, filter: Option<&crate::filter::Filter>) -> (IRect, Vec<IRect>) {
        let parent = self.surface.rect();
        let mut target = parent;
        if let Some(cl) = &self.state.clip {
            target = target.intersect(&cl.bounds);
        }
        let (mut l, mut t, mut r, mut b) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for f in [filter, self.state.filter_ops.as_deref()].into_iter().flatten() {
            let (fl, ft, fr, fb) = f.reach();
            l += fl;
            t += ft;
            r += fr;
            b += fb;
        }
        if let Some(sh) = Self::shadow_state(&self.state, self.color_space) {
            let s = (sh.sigma * 3.0).ceil() + 1.0;
            l += s + sh.offset_x.max(0.0);
            r += s - sh.offset_x.min(0.0);
            t += s + sh.offset_y.max(0.0);
            b += s - sh.offset_y.min(0.0);
        }
        if l == 0.0 && t == 0.0 && r == 0.0 && b == 0.0 {
            return (parent, Vec::new());
        }
        let m = |v: f64| v.min(1.0e7) as i32;
        let domain = IRect::new(target.x0.saturating_sub(m(l)), target.y0.saturating_sub(m(t)), target.x1.saturating_add(m(r)), target.y1.saturating_add(m(b)));
        let area = domain.width().max(0) as u64 * domain.height().max(0) as u64;
        let budget = (parent.width().max(0) as u64 * parent.height().max(0) as u64).max(LAYER_PIXELS);
        if area <= budget.min(crate::surface::MAX_PIXELS as u64) {
            return (domain, Vec::new());
        }
        // Content pulled in from very far (a shadow offset of 10000px)
        // would need a huge layer. A single drop-shadow gets a second tile
        // where its content comes from; anything else keeps the canvas.
        if let Some((dx, dy, sx, sy)) = Self::single_drop_shadow(filter, self.state.filter_ops.as_deref()) {
            if Self::shadow_state(&self.state, self.color_space).is_none() {
                let (mx, my) = ((3.0 * sx).ceil() as i32 + 1, (3.0 * sy).ceil() as i32 + 1);
                let (ox, oy) = (dx.round() as i32, dy.round() as i32);
                let source = IRect::new(target.x0 - ox - mx, target.y0 - oy - my, target.x1 - ox + mx, target.y1 - oy + my);
                return (target, vec![source]);
            }
        }
        (parent, Vec::new())
    }

    /// `(dx, dy, sigma_x, sigma_y)` when the layer's filters are exactly
    /// one drop-shadow.
    fn single_drop_shadow(a: Option<&crate::filter::Filter>, b: Option<&crate::filter::Filter>) -> Option<(f64, f64, f64, f64)> {
        let ops: Vec<&crate::filter::FilterOp> = [a, b].into_iter().flatten().flat_map(|f| f.ops.iter()).collect();
        match ops.as_slice() {
            [crate::filter::FilterOp::DropShadow { dx, dy, sigma, sigma_y, .. }] => Some((*dx, *dy, *sigma, *sigma_y)),
            _ => None,
        }
    }

    /// "Reset the rendering context to its default state": clear the
    /// bitmap, the path, the state and the stack.
    pub fn reset(&mut self) {
        self.drop_layers();
        self.surface.clear_all();
        self.state = State::default();
        self.stack.clear();
        self.path.clear();
        self.dirty = true;
    }

    /// New bitmap dimensions (setting `canvas.width`/`height`): the bitmap
    /// is recreated and the context reset. Returns `false` when too large
    /// (the context then keeps a 0x0 bitmap).
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        self.drop_layers();
        let opaque = self.surface.opaque;
        let ok = match Surface::new(width, height, opaque) {
            Some(s) => {
                self.surface = s;
                true
            }
            None => {
                self.surface = Surface::new(0, 0, opaque).expect("empty surface");
                false
            }
        };
        self.state = State::default();
        self.stack.clear();
        self.path.clear();
        self.dirty = true;
        ok
    }

    pub fn save(&mut self) {
        if self.stack.len() < MAX_STACK {
            self.stack.push(self.state.clone());
        }
    }

    /// Returns `Ok(false)` when the stack was empty; a layer on top of the
    /// stack cannot be restored (`endLayer` closes it).
    pub fn restore(&mut self) -> Result<bool, StackError> {
        if self.layers.last().is_some_and(|l| l.stack_len == self.stack.len()) {
            return Err(StackError::InvalidState);
        }
        Ok(match self.stack.pop() {
            Some(s) => {
                self.state = s;
                true
            }
            None => false,
        })
    }

    /// `beginLayer()`: drawing goes to a fresh transparent bitmap until
    /// `endLayer()` composites it with the current alpha, operator, shadow
    /// and filter (which are reset inside the layer).
    pub fn begin_layer(&mut self, filter: Option<Arc<crate::filter::Filter>>) {
        if self.layers.len() >= 64 || self.stack.len() >= MAX_STACK {
            return;
        }
        self.stack.push(self.state.clone());
        let (domain, extra) = self.layer_domain(filter.as_deref());
        let fresh = Surface::covering(domain)
            .or_else(|| Surface::covering(self.surface.rect()))
            .unwrap_or_else(|| Surface::new(0, 0, false).expect("empty surface"));
        let tiles: Vec<Surface> = extra.into_iter().filter_map(Surface::covering).collect();
        let parent = std::mem::replace(&mut self.surface, fresh);
        let parent_tiles = std::mem::replace(&mut self.tiles, tiles);
        let hidden = !self.state.transform.is_invertible();
        self.layers.push(LayerFrame { parent, parent_tiles, state: self.state.clone(), filter, stack_len: self.stack.len(), hidden });
        let s = &mut self.state;
        // The clip applies when the layer is composited, not inside it.
        s.clip = None;
        s.global_alpha = 1.0;
        s.op = CompositeOp::SourceOver;
        s.shadow_offset_x = 0.0;
        s.shadow_offset_y = 0.0;
        s.shadow_blur = 0.0;
        s.shadow_color = Color::TRANSPARENT;
        s.filter = "none".into();
        s.filter_ops = None;
    }

    /// `endLayer()`.
    pub fn end_layer(&mut self) -> Result<(), StackError> {
        let Some(frame) = self.layers.pop() else { return Err(StackError::InvalidState) };
        if frame.stack_len != self.stack.len() {
            self.layers.push(frame);
            return Err(StackError::InvalidState);
        }
        self.state = self.stack.pop().unwrap_or_default();
        let mut frame = frame;
        let placeholder = Surface::new(0, 0, false).expect("empty surface");
        let parent = std::mem::replace(&mut frame.parent, placeholder);
        let content = std::mem::replace(&mut self.surface, parent);
        let content_tiles = std::mem::replace(&mut self.tiles, std::mem::take(&mut frame.parent_tiles));
        if frame.hidden {
            return Ok(());
        }
        if content_tiles.is_empty() {
            for t in &mut self.tiles {
                Self::composite_frame(t, content.clone(), &frame, self.color_space);
            }
            Self::composite_frame(&mut self.surface, content, &frame, self.color_space);
        } else {
            Self::composite_tiled(&mut self.surface, &content, &content_tiles, &frame);
        }
        self.dirty = true;
        Ok(())
    }

    /// Composites a tiled layer: its single drop-shadow takes the shadow
    /// from the tile its content comes from (see `layer_domain`).
    fn composite_tiled(target: &mut Surface, content: &Surface, tiles: &[Surface], frame: &LayerFrame) {
        let st = &frame.state;
        let Some(op) = [frame.filter.as_deref(), st.filter_ops.as_deref()].into_iter().flatten().flat_map(|f| f.ops.iter()).next() else { return };
        let crate::filter::FilterOp::DropShadow { dx, dy, sigma, sigma_y, color } = op else { return };
        let ga = st.global_alpha as f32;
        let scale = |data: &mut Vec<f32>| {
            if ga < 1.0 {
                for v in data.iter_mut() {
                    *v *= ga;
                }
                crate::surface::quantize(data);
            }
        };
        let mut layer = content.to_layer();
        scale(&mut layer.data);
        let (ox, oy) = (dx.round() as i32, dy.round() as i32);
        let out = layer.rect;
        let (w, h) = (out.width().max(0) as usize, out.height().max(0) as usize);
        // Shadow alpha over the output area, from where the content was.
        let margin_x = (3.0 * sigma).ceil() as i32 + 1;
        let margin_y = (3.0 * sigma_y).ceil() as i32 + 1;
        let area = IRect::new(out.x0 - margin_x, out.y0 - margin_y, out.x1 + margin_x, out.y1 + margin_y);
        let (aw, ah) = (area.width().max(0) as usize, area.height().max(0) as usize);
        let mut alpha = vec![0.0f32; aw * ah];
        let sources: Vec<crate::surface::Layer> = tiles
            .iter()
            .map(|t| {
                let mut l = t.to_layer();
                scale(&mut l.data);
                l
            })
            .collect();
        for y in 0..ah {
            for x in 0..aw {
                let (sx, sy) = (area.x0 + x as i32 - ox, area.y0 + y as i32 - oy);
                let a = sources.iter().find(|l| l.contains(sx, sy)).map_or_else(|| layer.alpha_at(sx, sy), |l| l.alpha_at(sx, sy));
                alpha[y * aw + x] = a;
            }
        }
        crate::blur::gaussian_blur_xy(&mut alpha, aw, ah, sigma.max(0.0), sigma_y.max(0.0));
        for y in 0..h {
            for x in 0..w {
                let s = alpha[(y + margin_y as usize) * aw + x + margin_x as usize];
                let i = (y * w + x) * 4;
                let a = layer.data[i + 3];
                for k in 0..4 {
                    layer.data[i + k] += color[k] * s * (1.0 - a);
                }
            }
        }
        let comp = Composite { op: st.op, global_alpha: 1.0, clip: st.clip.as_deref(), shadow: None, filter: None };
        target.composite_layer(&layer, &comp);
    }

    /// Composites a layer's content onto `target` with the frame's state.
    fn composite_frame(target: &mut Surface, content: Surface, frame: &LayerFrame, space: crate::colorspace::ColorSpace) {
        let mut layer = content.to_layer();
        let (w, h) = (layer.rect.width() as usize, layer.rect.height() as usize);
        let st = &frame.state;
        let filtered = frame.filter.is_some() || st.filter_ops.is_some();
        // With a filter, globalAlpha applies before it, as for a single
        // draw with ctx.filter (WPT 2d.layer.global-states.ctx-filter).
        let mut alpha = st.global_alpha as f32;
        if filtered && alpha < 1.0 {
            for v in &mut layer.data {
                *v *= alpha;
            }
            alpha = 1.0;
            crate::surface::quantize(&mut layer.data);
        }
        if let Some(f) = frame.filter.as_deref() {
            f.apply(&mut layer.data, w, h);
        }
        if let Some(f) = st.filter_ops.as_deref() {
            f.apply(&mut layer.data, w, h);
        }
        let shadow = Self::shadow_state(st, space);
        let comp = Composite { op: st.op, global_alpha: alpha, clip: st.clip.as_deref(), shadow, filter: None };
        target.composite_layer(&layer, &comp);
    }

    /// The bitmap as it is presented: what is drawn into layers that are
    /// still open does not show (WPT manual/layers/unclosed-layers).
    #[must_use]
    pub fn presented(&self) -> &Surface {
        self.layers.first().map_or(&self.surface, |f| &f.parent)
    }

    /// Open layers (drawing reads of this canvas are refused meanwhile).
    #[must_use]
    pub fn layer_depth(&self) -> usize {
        self.layers.len()
    }

    fn drop_layers(&mut self) {
        if !self.layers.is_empty() {
            let root = self.layers.remove(0).parent;
            self.layers.clear();
            self.surface = root;
            self.tiles.clear();
        }
    }

    #[must_use]
    pub fn stack_depth(&self) -> usize {
        self.stack.len()
    }

    /// `transform(a, b, c, d, e, f)`.
    pub fn transform(&mut self, m: Matrix) {
        if m.is_finite() {
            self.state.transform = self.state.transform.multiply(&m);
        }
    }

    pub fn set_transform(&mut self, m: Matrix) {
        if m.is_finite() {
            self.state.transform = m;
        }
    }

    fn shadow(&self) -> Option<Shadow> {
        self.shadow_of(&self.state)
    }

    fn shadow_of(&self, s: &State) -> Option<Shadow> {
        Self::shadow_state(s, self.color_space)
    }

    fn shadow_state(s: &State, space: crate::colorspace::ColorSpace) -> Option<Shadow> {
        let c = s.shadow_color.premultiplied_in(space);
        if c[3] <= 0.0 || (s.shadow_blur == 0.0 && s.shadow_offset_x == 0.0 && s.shadow_offset_y == 0.0) {
            return None;
        }
        Some(Shadow { color: c, offset_x: s.shadow_offset_x, offset_y: s.shadow_offset_y, sigma: s.shadow_blur / 2.0 })
    }

    fn paint_for(&self, style: &Style) -> Option<(Paint, Matrix)> {
        let ctm = self.state.transform;
        match style {
            Style::Color(c) => Some((Paint::Solid(c.premultiplied_in(self.color_space)), Matrix::IDENTITY)),
            Style::Gradient(g) if g.output != self.color_space => Some((Paint::Gradient(Arc::new(g.in_space(self.color_space))), ctm.multiply(&g.transform).invert()?)),
            Style::Gradient(g) => Some((Paint::Gradient(g.clone()), ctm.multiply(&g.transform).invert()?)),
            Style::Pattern(p) => Some((Paint::Pattern(p.clone()), ctm.multiply(&p.transform).invert()?)),
        }
    }

    fn smoothing(&self) -> Smoothing {
        if self.state.smoothing { Smoothing::Bilinear } else { Smoothing::Nearest }
    }

    /// Paints `lines` with `style` using the current compositing state.
    fn paint_lines(&mut self, lines: &[Line], rule: FillRule, style: &Style) {
        let Some((paint, inverse)) = self.paint_for(style) else { return };
        self.paint_lines_with(lines, rule, &paint, inverse);
    }

    fn paint_lines_with(&mut self, lines: &[Line], rule: FillRule, paint: &Paint, inverse: Matrix) {
        if lines.is_empty() && !self.state.op.is_unbounded() {
            return;
        }
        let shader = Shader { paint, inverse, smoothing: self.smoothing() };
        let clip = self.state.clip.clone();
        let filter = self.state.filter_ops.clone();
        let comp = Composite { op: self.state.op, global_alpha: self.state.global_alpha as f32, clip: clip.as_deref(), shadow: self.shadow(), filter: filter.as_deref() };
        self.surface.fill(&mut self.raster, lines, rule, &shader, &comp);
        for t in &mut self.tiles {
            t.fill(&mut self.raster, lines, rule, &shader, &comp);
        }
        self.dirty = true;
    }

    fn rect_lines(&self, x: f64, y: f64, w: f64, h: f64) -> Vec<Line> {
        let m = &self.state.transform;
        let p = [m.apply(x, y), m.apply(x + w, y), m.apply(x + w, y + h), m.apply(x, y + h)];
        let mut lines = Vec::with_capacity(4);
        for i in 0..4 {
            let (a, b) = (p[i], p[(i + 1) % 4]);
            lines.push(Line::new(a.0, a.1, b.0, b.1));
        }
        lines
    }

    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        if ![x, y, w, h].iter().all(|v| v.is_finite()) || w == 0.0 || h == 0.0 {
            return;
        }
        let lines = self.rect_lines(x, y, w, h);
        let style = self.state.fill.clone();
        self.paint_lines(&lines, FillRule::NonZero, &style);
    }

    pub fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        if ![x, y, w, h].iter().all(|v| v.is_finite()) {
            return;
        }
        if w == 0.0 && h == 0.0 {
            return;
        }
        // A degenerate rectangle is a closed path there and back: no caps,
        // joins at both ends (WPT 2d.strokeRect.zero.4 / zero.5).
        let pts: Vec<(f64, f64)> = if w == 0.0 || h == 0.0 { vec![(x, y), (x + w, y + h)] } else { vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)] };
        let closed = true;
        let m = self.state.transform;
        let style = self.state.line.clone();
        let mut stroker = Stroker::new(&style, &m);
        stroker.stroke(Polyline { points: &pts, tangents: None, closed });
        let lines = std::mem::take(&mut stroker.lines);
        let st = self.state.stroke.clone();
        self.paint_lines(&lines, FillRule::NonZero, &st);
    }

    pub fn clear_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        if ![x, y, w, h].iter().all(|v| v.is_finite()) || w == 0.0 || h == 0.0 {
            return;
        }
        let lines = self.rect_lines(x, y, w, h);
        let clip = self.state.clip.clone();
        self.surface.clear(&mut self.raster, &lines, clip.as_deref());
        for t in &mut self.tiles {
            t.clear(&mut self.raster, &lines, clip.as_deref());
        }
        self.dirty = true;
    }

    pub fn begin_path(&mut self) {
        self.path.clear();
    }

    /// `fill()` / `fill(path)`.
    pub fn fill(&mut self, path: Option<&Path>, rule: FillRule) {
        let lines = path.unwrap_or(&self.path).fill_lines();
        let style = self.state.fill.clone();
        self.paint_lines(&lines, rule, &style);
    }

    /// The stroke outline of a device-space path as device-space edges.
    fn stroke_lines(&self, path: &Path) -> Vec<Line> {
        let m = self.state.transform;
        let Some(inv) = m.invert() else { return Vec::new() };
        let style = &self.state.line;
        let mut stroker = Stroker::new(style, &m);
        let mut user: Vec<(f64, f64)> = Vec::new();
        let mut tangents: Vec<(f64, f64)> = Vec::new();
        for s in &path.subpaths {
            user.clear();
            user.extend(s.points.iter().map(|p| inv.apply(p.0, p.1)));
            tangents.clear();
            tangents.extend(s.tangent.iter().map(|a| {
                if a.is_nan() {
                    return (f64::NAN, f64::NAN);
                }
                let (sn, cs) = (*a as f64).sin_cos();
                let (x, y) = inv.apply_vector(cs, sn);
                let l = (x * x + y * y).sqrt();
                if l > 0.0 { (x / l, y / l) } else { (f64::NAN, f64::NAN) }
            }));
            stroker.stroke(Polyline { points: &user, tangents: Some(&tangents), closed: s.closed });
        }
        stroker.lines
    }

    pub fn stroke(&mut self, path: Option<&Path>) {
        let lines = self.stroke_lines(path.unwrap_or(&self.path));
        let style = self.state.stroke.clone();
        self.paint_lines(&lines, FillRule::NonZero, &style);
    }

    pub fn clip(&mut self, path: Option<&Path>, rule: FillRule) {
        let lines = path.unwrap_or(&self.path).fill_lines();
        let mut clip = self.surface.clip_with(&mut self.raster, &lines, rule, self.state.clip.as_deref());
        for t in &self.tiles {
            let part = t.clip_with(&mut self.raster, &lines, rule, self.state.clip.as_deref());
            clip.bounds = clip.bounds.union(&part.bounds);
            clip.parts.push(part);
        }
        self.state.clip = Some(Arc::new(clip));
    }

    #[must_use]
    pub fn is_point_in_path(&self, path: Option<&Path>, x: f64, y: f64, rule: FillRule) -> bool {
        // A singular transform maps no point into user space (a huge but
        // regular one, such as a Number.MAX_VALUE scale, still does).
        if !(x.is_finite() && y.is_finite()) || self.state.transform.determinant() == 0.0 {
            return false;
        }
        path.unwrap_or(&self.path).contains(x, y, rule == FillRule::EvenOdd)
    }

    #[must_use]
    pub fn is_point_in_stroke(&self, path: Option<&Path>, x: f64, y: f64) -> bool {
        if !(x.is_finite() && y.is_finite()) {
            return false;
        }
        let lines = self.stroke_lines(path.unwrap_or(&self.path));
        crate::stroke::stroke_contains(&lines, x, y)
    }

    /// `drawImage` with a normalized, non-empty source and destination.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image(&mut self, image: &Image, sx: f64, sy: f64, sw: f64, sh: f64, dx: f64, dy: f64, dw: f64, dh: f64) {
        if ![sx, sy, sw, sh, dx, dy, dw, dh].iter().all(|v| v.is_finite()) || sw == 0.0 || sh == 0.0 || dw == 0.0 || dh == 0.0 {
            return;
        }
        if image.width == 0 || image.height == 0 {
            return;
        }
        let (mut sx, mut sy, mut sw, mut sh) = norm(sx, sy, sw, sh);
        let (mut dx, mut dy, mut dw, mut dh) = norm(dx, dy, dw, dh);
        // Clip the source to the image and the destination proportionally.
        let (iw, ih) = (image.width as f64, image.height as f64);
        let (kx, ky) = (dw / sw, dh / sh);
        if sx < 0.0 {
            dx -= sx * kx;
            dw += sx * kx;
            sw += sx;
            sx = 0.0;
        }
        if sy < 0.0 {
            dy -= sy * ky;
            dh += sy * ky;
            sh += sy;
            sy = 0.0;
        }
        if sx + sw > iw {
            let cut = sx + sw - iw;
            sw -= cut;
            dw -= cut * kx;
        }
        if sy + sh > ih {
            let cut = sy + sh - ih;
            sh -= cut;
            dh -= cut * ky;
        }
        if sw <= 0.0 || sh <= 0.0 || dw <= 0.0 || dh <= 0.0 {
            return;
        }
        let Some(inv) = self.state.transform.invert() else { return };
        let local = Matrix::new(sw / dw, 0.0, 0.0, sh / dh, sx - dx * sw / dw, sy - dy * sh / dh);
        let to_image = local.multiply(&inv);
        let source = Arc::new(ImageSource { image: image.clone(), sx, sy, sw, sh });
        let paint = Paint::Image { source, to_image };
        let lines = self.rect_lines(dx, dy, dw, dh);
        self.paint_lines_with(&lines, FillRule::NonZero, &paint, Matrix::IDENTITY);
    }

    /// `getImageData` (un-premultiplied RGBA8).
    #[must_use]
    pub fn get_image_data(&self, sx: i64, sy: i64, w: u32, h: u32) -> Vec<u8> {
        self.surface.read(sx, sy, w, h)
    }

    /// `putImageData` with the dirty rectangle already normalized (spec
    /// steps 1–4); `rect` lies within the image data.
    pub fn put_image_data(&mut self, data: &[u8], width: u32, dx: i64, dy: i64, rect: IRect) {
        if rect.is_empty() {
            return;
        }
        self.surface.write(data, width, dx, dy, rect);
        for t in &mut self.tiles {
            t.write(data, width, dx, dy, rect);
        }
        self.dirty = true;
    }
}

fn norm(x: f64, y: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let (x, w) = if w < 0.0 { (x + w, -w) } else { (x, w) };
    let (y, h) = if h < 0.0 { (y + h, -h) } else { (y, h) };
    (x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(c: &Context2D, x: i64, y: i64) -> [u8; 4] {
        let d = c.get_image_data(x, y, 1, 1);
        [d[0], d[1], d[2], d[3]]
    }

    #[test]
    fn fill_rect_paints_solid_green() {
        let mut c = Context2D::new(100, 50, false).unwrap();
        c.state.fill = Style::Color(Color::from_rgba8(0, 255, 0, 255));
        c.fill_rect(0.0, 0.0, 100.0, 50.0);
        assert_eq!(px(&c, 50, 25), [0, 255, 0, 255]);
    }

    #[test]
    fn copy_operator_clears_uncovered_pixels() {
        let mut c = Context2D::new(100, 50, false).unwrap();
        c.state.fill = Style::Color(Color::rgba(0.0, 1.0, 0.0, 0.5));
        c.fill_rect(0.0, 0.0, 100.0, 50.0);
        c.state.op = CompositeOp::Copy;
        c.state.fill = Style::Color(Color::rgba(0.0, 0.0, 1.0, 0.75));
        c.transform(Matrix::translate(0.0, 25.0));
        c.fill_rect(0.0, 50.0, 100.0, 50.0);
        assert_eq!(px(&c, 50, 25), [0, 0, 0, 0]);
    }

    #[test]
    fn clip_limits_painting() {
        let mut c = Context2D::new(100, 50, false).unwrap();
        let m = c.state.transform;
        c.path.rect(&m, 0.0, 0.0, 50.0, 50.0);
        c.clip(None, FillRule::NonZero);
        c.state.fill = Style::Color(Color::from_rgba8(255, 0, 0, 255));
        c.fill_rect(0.0, 0.0, 100.0, 50.0);
        assert_eq!(px(&c, 25, 25), [255, 0, 0, 255]);
        assert_eq!(px(&c, 75, 25), [0, 0, 0, 0]);
    }

    #[test]
    fn stroke_rect_draws_the_outline_only() {
        let mut c = Context2D::new(100, 50, false).unwrap();
        c.state.stroke = Style::Color(Color::from_rgba8(0, 255, 0, 255));
        c.state.line.width = 10.0;
        c.stroke_rect(25.0, 12.5, 50.0, 25.0);
        assert_eq!(px(&c, 25, 12), [0, 255, 0, 255]);
        assert_eq!(px(&c, 50, 25), [0, 0, 0, 0]);
    }

    #[test]
    fn shadow_is_drawn_offset() {
        let mut c = Context2D::new(100, 50, false).unwrap();
        c.state.shadow_color = Color::from_rgba8(0, 255, 0, 255);
        c.state.shadow_offset_x = 50.0;
        c.state.fill = Style::Color(Color::from_rgba8(255, 0, 0, 255));
        c.fill_rect(0.0, 0.0, 50.0, 50.0);
        assert_eq!(px(&c, 25, 25), [255, 0, 0, 255]);
        assert_eq!(px(&c, 75, 25), [0, 255, 0, 255]);
    }

    fn green_canvas() -> Context2D {
        let mut c = Context2D::new(100, 50, false).unwrap();
        c.state.fill = Style::Color(Color::from_rgba8(0, 255, 0, 255));
        c.fill_rect(0.0, 0.0, 100.0, 50.0);
        c.state.stroke = Style::Color(Color::from_rgba8(255, 0, 0, 255));
        c
    }

    #[test]
    fn open_layers_are_not_presented_and_singular_transforms_hit_nothing() {
        let mut c = Context2D::new(20, 20, false).unwrap();
        c.state.fill = Style::Color(Color::rgba(1.0, 0.0, 0.0, 1.0));
        c.begin_layer(None);
        c.fill_rect(0.0, 0.0, 10.0, 10.0);
        assert_eq!(c.presented().read(5, 5, 1, 1), vec![0, 0, 0, 0]);
        c.end_layer().unwrap();
        assert_eq!(c.presented().read(5, 5, 1, 1), vec![255, 0, 0, 255]);
        // WPT 2d.path.isPointInPath.basic: a huge scale still hits, a
        // singular one never does.
        let mut p = Path::new();
        let big = Matrix::new(f64::MAX, 0.0, 0.0, f64::MAX, 0.0, 0.0);
        p.rect(&big, -10.0, -10.0, 20.0, 20.0);
        c.set_transform(big);
        assert!(c.is_point_in_path(Some(&p), 0.0, 0.0, FillRule::NonZero));
        c.set_transform(Matrix::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        assert!(!c.is_point_in_path(Some(&p), 0.0, 0.0, FillRule::NonZero));
    }

    #[test]
    fn filtered_draws_round_like_two_plain_draws() {
        // WPT 2d.filter.drop-shadow-globalAlpha: shadow and shape at alpha
        // 0.5 overlap exactly as two 8-bit draws would.
        use crate::filter::{Filter, FilterOp};
        let mut c = Context2D::new(4, 4, false).unwrap();
        let shadow = FilterOp::DropShadow { dx: 1.0, dy: 0.0, sigma: 0.0, sigma_y: 0.0, color: [1.0, 165.0 / 255.0, 0.0, 1.0] };
        c.state.filter_ops = Some(Arc::new(Filter { ops: vec![shadow] }));
        c.state.global_alpha = 0.5;
        c.state.fill = Style::Color(Color::from_rgba8(128, 0, 128, 255));
        c.fill_rect(0.0, 0.0, 2.0, 4.0);
        assert_eq!(px(&c, 1, 1), [170, 54, 85, 192]);
    }

    #[test]
    fn far_shadows_come_from_a_second_tile() {
        // WPT 2d.layer.shadow-from-outside-canvas.long-distance: the
        // content is 10200 px away; the layer covers only two tiles.
        use crate::filter::{Filter, FilterOp};
        let mut c = Context2D::new(200, 200, false).unwrap();
        let shadow = FilterOp::DropShadow { dx: -10200.0, dy: -10200.0, sigma: 0.0, sigma_y: 0.0, color: [0.0, 0.5, 0.0, 1.0] };
        c.state.filter_ops = Some(Arc::new(Filter { ops: vec![shadow] }));
        c.begin_layer(None);
        c.state.fill = Style::Color(Color::rgba(1.0, 0.0, 0.0, 1.0));
        c.fill_rect(10200.0, 10200.0, 100.0, 100.0);
        c.end_layer().unwrap();
        assert_eq!(px(&c, 50, 50), [0, 128, 0, 255]);
        assert_eq!(px(&c, 150, 150), [0, 0, 0, 0]);
    }

    #[test]
    fn layer_filters_pull_content_from_outside_the_canvas() {
        // WPT 2d.layer.shadow-from-outside-canvas.short-distance.ctx-filter.
        use crate::filter::{Filter, FilterOp};
        let mut c = Context2D::new(200, 200, false).unwrap();
        let shadow = FilterOp::DropShadow { dx: -201.0, dy: -201.0, sigma: 0.0, sigma_y: 0.0, color: [0.0, 0.5, 0.0, 1.0] };
        c.state.filter_ops = Some(Arc::new(Filter { ops: vec![shadow] }));
        c.begin_layer(None);
        c.state.fill = Style::Color(Color::rgba(1.0, 0.0, 0.0, 1.0));
        c.fill_rect(201.0, 201.0, 100.0, 100.0);
        c.end_layer().unwrap();
        assert_eq!(px(&c, 50, 50), [0, 128, 0, 255]);
        assert_eq!(px(&c, 150, 150), [0, 0, 0, 0]);
        assert_eq!((c.width(), c.height()), (200, 200));
    }

    #[test]
    fn thick_arc_strokes_end_exactly_at_their_caps() {
        // WPT 2d.path.arc.shape.1: a half circle below the canvas.
        let mut c = green_canvas();
        c.state.line.width = 50.0;
        let m = c.state.transform;
        c.path.ellipse(&m, 50.0, 50.0, 50.0, 50.0, 0.0, 0.0, std::f64::consts::PI, false).unwrap();
        c.stroke(None);
        for (x, y) in [(50, 25), (1, 1), (98, 1), (1, 48), (20, 48), (98, 48)] {
            assert_eq!(px(&c, x, y), [0, 255, 0, 255], "({x},{y})");
        }
        // WPT 2d.path.arc.selfintersect.1: lines wider than the radius.
        let mut c = green_canvas();
        c.state.line.width = 200.0;
        let m = c.state.transform;
        c.path.ellipse(&m, 100.0, 50.0, 25.0, 25.0, 0.0, 0.0, -std::f64::consts::FRAC_PI_2, true).unwrap();
        c.stroke(None);
        c.begin_path();
        c.path.ellipse(&m, 0.0, 0.0, 25.0, 25.0, 0.0, 0.0, -std::f64::consts::FRAC_PI_2, true).unwrap();
        c.stroke(None);
        assert_eq!(px(&c, 1, 1), [0, 255, 0, 255]);
        assert_eq!(px(&c, 50, 25), [0, 255, 0, 255]);
        // WPT 2d.path.arc.shape.3.
        let mut c = green_canvas();
        c.state.line.width = 100.0;
        let m = c.state.transform;
        c.path.ellipse(&m, 0.0, 50.0, 50.0, 50.0, 0.0, 0.0, -std::f64::consts::FRAC_PI_2, false).unwrap();
        c.stroke(None);
        for (x, y) in [(50, 25), (1, 1), (98, 1), (1, 48), (98, 48)] {
            assert_eq!(px(&c, x, y), [0, 255, 0, 255], "({x},{y})");
        }
    }

    #[test]
    fn draw_image_copies_pixels_exactly() {
        let mut src = Context2D::new(4, 4, false).unwrap();
        src.state.fill = Style::Color(Color::from_rgba8(10, 20, 30, 255));
        src.fill_rect(0.0, 0.0, 2.0, 2.0);
        let img = src.surface.snapshot();
        let mut c = Context2D::new(10, 10, false).unwrap();
        c.draw_image(&img, 0.0, 0.0, 4.0, 4.0, 3.0, 3.0, 4.0, 4.0);
        assert_eq!(px(&c, 3, 3), [10, 20, 30, 255]);
        assert_eq!(px(&c, 5, 5), [0, 0, 0, 0]);
    }
}
