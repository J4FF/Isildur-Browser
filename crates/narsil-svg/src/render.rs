//! Painting the document tree onto a `narsil_canvas::Context2D`: the
//! structure elements, shapes, paint servers, strokes, markers, images,
//! text, and the group effects (opacity, clip-path, mask, filter,
//! mix-blend-mode) through offscreen surfaces.

use crate::color;
use crate::style::{Computed, Doc};
use crate::transform;
use crate::units::{self, Axis, Ctx};
use narsil_canvas::paint::ColorStop;
use narsil_canvas::{Color, CompositeOp, Context2D, FillRule, Gradient, GradientKind, LineCap, LineJoin, Matrix, Path, Pattern, Repetition, Spread, Style};
use std::sync::Arc;

/// The viewport percentages resolve against.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub w: f64,
    pub h: f64,
}

/// User-space bounding box (x0, y0, x1, y1).
pub type BBox = (f64, f64, f64, f64);

/// How deep `use`, patterns, markers and masks may nest.
const MAX_DEPTH: usize = 24;

pub struct Renderer<'d, 'a> {
    pub doc: &'d Doc<'a>,
    pub w: u32,
    pub h: u32,
    active: Vec<usize>,
    /// Paints of the shape a marker belongs to (`context-fill`).
    /// With the transform of the context element they were made for.
    context_paint: Vec<(Style, Style, Matrix)>,
    /// Styles of the `use` instance trees being drawn.
    instances: Vec<std::collections::HashMap<usize, Computed>>,
    /// Group opacity folded into a leaf's single paint operation.
    alpha_scale: f64,
}

#[derive(Clone, Debug)]
enum PaintRef {
    None,
    Color(Color),
    Server(usize, Option<Color>),
    ContextFill,
    ContextStroke,
}

impl<'d, 'a> Renderer<'d, 'a> {
    pub fn new(doc: &'d Doc<'a>, w: u32, h: u32) -> Self {
        Renderer { doc, w, h, active: Vec::new(), context_paint: Vec::new(), instances: Vec::new(), alpha_scale: 1.0 }
    }

    fn ctx_for(&self, st: &Computed, view: View) -> Ctx {
        Ctx { vw: view.w, vh: view.h, font_size: font_size(st, view) }
    }

    /// The computed style of element `i`, as inherited in the current
    /// `use` instance tree.
    pub(crate) fn style(&self, i: usize) -> &Computed {
        for map in self.instances.iter().rev() {
            if let Some(s) = map.get(&i) {
                return s;
            }
        }
        &self.doc.items[i].style
    }

    fn name(&self, i: usize) -> &str {
        &self.doc.items[i].el.name
    }

    fn attr(&self, i: usize, name: &str) -> Option<&str> {
        self.doc.items[i].el.attr(name)
    }

    /// A geometry value: the CSS property when set, else the attribute.
    fn geometry(&self, i: usize, name: &str) -> Option<String> {
        let st = self.style(i);
        // `width`/`height` on a nested `<svg>` are attributes only (the
        // CSS properties do not size it; svgwg pull request 1059).
        let attribute_only = self.name(i) == "svg" && matches!(name, "width" | "height");
        if st.is_set(name) && !attribute_only {
            let v = st.get(name);
            if !v.eq_ignore_ascii_case("auto") {
                return Some(v.to_string());
            }
        }
        self.attr(i, name).map(str::to_string)
    }

    fn len(&self, i: usize, name: &str, axis: Axis, view: View, default: f64) -> f64 {
        let c = self.ctx_for(self.style(i), view);
        self.geometry(i, name).and_then(|v| units::length(&v, axis, &c)).unwrap_or(default)
    }

    /// The element's own transform (attribute or CSS property).
    fn own_transform(&self, i: usize, view: View) -> Matrix {
        let st = self.style(i);
        let text = if st.is_set("transform") { st.get("transform").to_string() } else { self.attr(i, "transform").unwrap_or("").to_string() };
        let m = transform::parse(&text).unwrap_or(Matrix::IDENTITY);
        if m.is_identity() || !st.is_set("transform-origin") {
            return m;
        }
        let c = self.ctx_for(st, view);
        let parts: Vec<&str> = st.get("transform-origin").split_whitespace().collect();
        let ox = parts.first().and_then(|p| origin_value(p, Axis::X, &c)).unwrap_or(0.0);
        let oy = parts.get(1).and_then(|p| origin_value(p, Axis::Y, &c)).unwrap_or(0.0);
        Matrix::translate(ox, oy).multiply(&m).multiply(&Matrix::translate(-ox, -oy))
    }

    // ---------------------------------------------------------------- tree

    pub fn draw_children(&mut self, ctx: &mut Context2D, i: usize, m: Matrix, view: View) {
        let kids = self.doc.items[i].children.clone();
        for c in kids {
            self.draw(ctx, c, m, view);
        }
    }

    /// Draws element `i` under `ctm`.
    pub fn draw(&mut self, ctx: &mut Context2D, i: usize, ctm: Matrix, view: View) {
        if self.active.len() > MAX_DEPTH * 8 {
            return;
        }
        let name = self.name(i).to_string();
        let st = self.style(i);
        if st.get("display").eq_ignore_ascii_case("none") {
            return;
        }
        if !conditions_pass(self.doc.items[i].el) {
            return;
        }
        let renderable = matches!(
            name.as_str(),
            "svg" | "g" | "a" | "switch" | "use" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "path" | "text" | "image"
        );
        if !renderable {
            return;
        }
        let m = ctm.multiply(&self.own_transform(i, view));
        if !m.is_invertible() {
            return;
        }
        // A shape without geometry renders nothing, filters included.
        if matches!(name.as_str(), "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "path") && self.shape_path(i, view).is_none() {
            return;
        }
        if name == "image" && (self.geometry(i, "width").is_some_and(|w| units::number(&w) == Some(0.0)) || self.geometry(i, "height").is_some_and(|h| units::number(&h) == Some(0.0))) {
            return;
        }
        let effects = self.has_effects(i);
        if !effects {
            self.draw_content(ctx, i, m, view);
            return;
        }
        // Opacity on a leaf with one paint operation needs no layer.
        if let Some(k) = self.leaf_opacity(i, view) {
            let saved = self.alpha_scale;
            self.alpha_scale *= k;
            self.draw_content(ctx, i, m, view);
            self.alpha_scale = saved;
            return;
        }
        // The layer covers what the element can paint, not the canvas.
        let full = (0.0, 0.0, f64::from(self.w), f64::from(self.h));
        let area = match self.paint_bounds(i, m, view) {
            Some(b) => (b.0.max(0.0).floor(), b.1.max(0.0).floor(), b.2.min(full.2).ceil(), b.3.min(full.3).ceil()),
            None => full,
        };
        if area.2 <= area.0 || area.3 <= area.1 {
            return;
        }
        let (lw, lh) = ((area.2 - area.0) as u32, (area.3 - area.1) as u32);
        let mut layer = match Context2D::new(lw, lh, false) {
            Some(c) => c,
            None => return,
        };
        let lm = Matrix::translate(-area.0, -area.1).multiply(&m);
        let (ow, oh) = (self.w, self.h);
        let saved_alpha = self.alpha_scale;
        self.alpha_scale = 1.0;
        self.w = lw;
        self.h = lh;
        self.draw_content(&mut layer, i, lm, view);
        let composed = self.effects_layer(layer, i, lm, view);
        self.w = ow;
        self.h = oh;
        self.alpha_scale = saved_alpha;
        if let Some((img, opacity, op)) = composed {
            ctx.save();
            ctx.state.transform = Matrix::IDENTITY;
            ctx.state.global_alpha = opacity * saved_alpha;
            ctx.state.op = op;
            ctx.state.smoothing = false;
            let (w, h) = (f64::from(img.width), f64::from(img.height));
            ctx.draw_image(&img, 0.0, 0.0, w, h, area.0, area.1, w, h);
            let _ = ctx.restore();
        }
    }

    /// The opacity of a leaf whose content is a single paint operation
    /// (an image, or a shape with only a fill or only a stroke and no
    /// markers), when opacity is its only effect.
    fn leaf_opacity(&self, i: usize, view: View) -> Option<f64> {
        let st = self.style(i);
        if !st.get("clip-path").eq_ignore_ascii_case("none")
            || !st.get("mask").eq_ignore_ascii_case("none")
            || !st.get("filter").eq_ignore_ascii_case("none")
            || !st.get("mix-blend-mode").eq_ignore_ascii_case("normal")
        {
            return None;
        }
        let k = units::fraction(st.get("opacity")).unwrap_or(1.0).clamp(0.0, 1.0);
        match self.name(i) {
            "image" => Some(k),
            "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "path" => {
                let has_markers = ["marker-start", "marker-mid", "marker-end"].iter().any(|p| !st.get(p).eq_ignore_ascii_case("none"));
                let fill = !st.get("fill").eq_ignore_ascii_case("none");
                let c = self.ctx_for(st, view);
                let stroke = !st.get("stroke").eq_ignore_ascii_case("none") && units::length(st.get("stroke-width"), Axis::Diagonal, &c).unwrap_or(1.0) > 0.0;
                (!has_markers && !(fill && stroke)).then_some(k)
            }
            _ => None,
        }
    }

    /// A conservative device-space box of everything element `i` paints
    /// under `m`; `None` when it cannot be bounded (filters, markers).
    pub(crate) fn paint_bounds(&self, i: usize, m: Matrix, view: View) -> Option<BBox> {
        let st = self.style(i);
        if !st.get("filter").eq_ignore_ascii_case("none") {
            return None;
        }
        let name = self.name(i);
        let stroke_pad = |st: &Computed| -> f64 {
            if st.get("stroke").eq_ignore_ascii_case("none") {
                return 0.0;
            }
            let c = self.ctx_for(st, view);
            let w = units::length(st.get("stroke-width"), Axis::Diagonal, &c).unwrap_or(1.0).max(0.0);
            let miter = if st.get("stroke-linejoin").eq_ignore_ascii_case("miter") || st.get("stroke-linejoin").eq_ignore_ascii_case("miter-clip") {
                units::number(st.get("stroke-miterlimit")).unwrap_or(4.0).max(1.0)
            } else {
                1.0
            };
            let scale = if st.get("vector-effect").eq_ignore_ascii_case("non-scaling-stroke") { 1.0 } else { m.max_scale() };
            w / 2.0 * miter.max(std::f64::consts::SQRT_2) * scale
        };
        let grow = |b: BBox, d: f64| (b.0 - d - 1.0, b.1 - d - 1.0, b.2 + d + 1.0, b.3 + d + 1.0);
        match name {
            "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "path" => {
                if ["marker-start", "marker-mid", "marker-end"].iter().any(|p| !st.get(p).eq_ignore_ascii_case("none")) {
                    return None;
                }
                let segs = self.shape_path(i, view)?;
                let b = build_path(&segs, &m).bounds();
                b.x0.is_finite().then(|| grow((b.x0, b.y0, b.x1, b.y1), stroke_pad(st)))
            }
            "text" => {
                let b = crate::text::text_bbox(self, i, m, view)?;
                let em = font_size(st, view) * m.max_scale();
                Some(grow(b, stroke_pad(st) + em))
            }
            "image" => self.bbox(i, m, view).map(|b| grow(b, 0.0)),
            "g" | "a" | "switch" | "svg" => {
                let mut acc: Option<BBox> = None;
                for &c in &self.doc.items[i].children {
                    if self.style(c).get("display").eq_ignore_ascii_case("none") || !self.renders(c) {
                        continue;
                    }
                    let cm = if name == "svg" { return None } else { m.multiply(&self.own_transform(c, view)) };
                    let b = self.paint_bounds(c, cm, view)?;
                    acc = Some(match acc {
                        None => b,
                        Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
                    });
                }
                Some(acc.unwrap_or((0.0, 0.0, 0.0, 0.0)))
            }
            _ => None,
        }
    }

    /// Elements that paint (the rest is skipped by `paint_bounds`).
    fn renders(&self, i: usize) -> bool {
        matches!(self.name(i), "svg" | "g" | "a" | "switch" | "use" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "path" | "text" | "image")
    }

    fn has_effects(&self, i: usize) -> bool {
        let st = self.style(i);
        let opacity = units::fraction(st.get("opacity")).unwrap_or(1.0);
        opacity < 1.0
            || !st.get("clip-path").eq_ignore_ascii_case("none")
            || !st.get("mask").eq_ignore_ascii_case("none")
            || !st.get("filter").eq_ignore_ascii_case("none")
            || !st.get("mix-blend-mode").eq_ignore_ascii_case("normal")
    }

    fn draw_content(&mut self, ctx: &mut Context2D, i: usize, m: Matrix, view: View) {
        let name = self.name(i).to_string();
        match name.as_str() {
            "svg" => self.draw_svg(ctx, i, m, view),
            "g" | "a" => self.draw_children(ctx, i, m, view),
            "switch" => {
                let kids = self.doc.items[i].children.clone();
                if let Some(c) = kids.into_iter().find(|c| conditions_pass(self.doc.items[*c].el) && !self.style(*c).get("display").eq_ignore_ascii_case("none")) {
                    self.draw(ctx, c, m, view);
                }
            }
            "use" => self.draw_use(ctx, i, m, view),
            "image" => self.draw_image(ctx, i, m, view),
            "text" => crate::text::draw_text(self, ctx, i, m, view),
            _ => self.draw_shape(ctx, i, m, view),
        }
    }

    /// A nested `<svg>`: its viewport, `viewBox` and clip.
    fn draw_svg(&mut self, ctx: &mut Context2D, i: usize, m: Matrix, view: View) {
        let is_root = self.doc.items[i].parent.is_none();
        // The root's viewport is the one the document is rendered into.
        let (x, y, w, h) = if is_root {
            (0.0, 0.0, view.w, view.h)
        } else {
            (self.len(i, "x", Axis::X, view, 0.0), self.len(i, "y", Axis::Y, view, 0.0), self.len(i, "width", Axis::X, view, view.w), self.len(i, "height", Axis::Y, view, view.h))
        };
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.draw_viewport(ctx, i, m, x, y, w, h, i);
    }

    /// Children of `content` in a viewport (x, y, w, h) with the
    /// `viewBox`/`preserveAspectRatio` of element `vb_from`.
    #[allow(clippy::too_many_arguments)]
    fn draw_viewport(&mut self, ctx: &mut Context2D, content: usize, m: Matrix, x: f64, y: f64, w: f64, h: f64, vb_from: usize) {
        if transform::view_box_disables(self.attr(vb_from, "viewBox")) {
            return;
        }
        let par = self.attr(vb_from, "preserveAspectRatio").unwrap_or("xMidYMid meet").to_string();
        let (inner, view) = match self.attr(vb_from, "viewBox").and_then(transform::view_box) {
            Some(vb) => (transform::view_box_transform(vb, &par, x, y, w, h), View { w: vb[2], h: vb[3] }),
            None => (Matrix::translate(x, y), View { w, h }),
        };
        let clip = overflow_clips(self.style(vb_from)) && self.doc.items[vb_from].parent.is_some();
        if clip {
            ctx.save();
            let mut p = Path::new();
            p.rect(&m, x, y, w, h);
            ctx.clip(Some(&p), FillRule::NonZero);
        }
        self.draw_children(ctx, content, m.multiply(&inner), view);
        if clip {
            let _ = ctx.restore();
        }
    }

    fn draw_use(&mut self, ctx: &mut Context2D, i: usize, m: Matrix, view: View) {
        let Some(target) = self.attr(i, "href").and_then(|r| self.doc.lookup(r)) else { return };
        if self.active.contains(&target) || self.active.len() >= MAX_DEPTH || self.is_ancestor(target, i) {
            return;
        }
        let x = self.len(i, "x", Axis::X, view, 0.0);
        let y = self.len(i, "y", Axis::Y, view, 0.0);
        // The `use` element is the context element of its instance tree
        // (`context-fill` / `context-stroke`).
        let needs_box = self.style(i).get("fill").contains("url(") || self.style(i).get("stroke").contains("url(");
        let ubox = if needs_box { self.bbox(i, Matrix::IDENTITY, view) } else { None };
        let cf = self.paint_style(i, true, ubox, m, view).map(|p| p.0);
        let cs = self.paint_style(i, false, ubox, m, view).map(|p| p.0);
        self.push_context_paint(cf, cs, m);
        let m = m.multiply(&Matrix::translate(x, y));
        self.active.push(target);
        let restyled = self.doc.restyle(target, self.style(i));
        self.instances.push(restyled);
        match self.name(target) {
            "symbol" => {
                let w = self.geometry(i, "width").and_then(|v| units::length(&v, Axis::X, &self.ctx_for(self.style(i), view))).or_else(|| self.geometry(target, "width").and_then(|v| units::length(&v, Axis::X, &self.ctx_for(self.style(target), view)))).unwrap_or(view.w);
                let h = self.geometry(i, "height").and_then(|v| units::length(&v, Axis::Y, &self.ctx_for(self.style(i), view))).or_else(|| self.geometry(target, "height").and_then(|v| units::length(&v, Axis::Y, &self.ctx_for(self.style(target), view)))).unwrap_or(view.h);
                let (sx, sy) = (self.len(target, "x", Axis::X, view, 0.0), self.len(target, "y", Axis::Y, view, 0.0));
                if w > 0.0 && h > 0.0 {
                    self.draw_symbol(ctx, target, m, sx, sy, w, h, view);
                }
            }
            "svg" => {
                let w = self.geometry(i, "width").and_then(|v| units::length(&v, Axis::X, &self.ctx_for(self.style(i), view))).unwrap_or_else(|| self.len(target, "width", Axis::X, view, view.w));
                let h = self.geometry(i, "height").and_then(|v| units::length(&v, Axis::Y, &self.ctx_for(self.style(i), view))).unwrap_or_else(|| self.len(target, "height", Axis::Y, view, view.h));
                let (sx, sy) = (self.len(target, "x", Axis::X, view, 0.0), self.len(target, "y", Axis::Y, view, 0.0));
                if w > 0.0 && h > 0.0 && !self.style(target).get("display").eq_ignore_ascii_case("none") {
                    let tm = m.multiply(&self.own_transform(target, view));
                    if self.has_effects(target) {
                        if let Some(mut layer) = Context2D::new(self.w, self.h, false) {
                            self.draw_viewport(&mut layer, target, tm, sx, sy, w, h, target);
                            self.apply_effects(ctx, layer, target, tm, view);
                        }
                    } else {
                        self.draw_viewport(ctx, target, tm, sx, sy, w, h, target);
                    }
                }
            }
            _ => self.draw(ctx, target, m, view),
        }
        self.instances.pop();
        self.pop_context_paint();
        self.active.pop();
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_symbol(&mut self, ctx: &mut Context2D, sym: usize, m: Matrix, x: f64, y: f64, w: f64, h: f64, _view: View) {
        let m = m.multiply(&self.own_transform(sym, _view));
        if self.has_effects(sym) {
            if let Some(mut layer) = Context2D::new(self.w, self.h, false) {
                self.draw_viewport(&mut layer, sym, m, x, y, w, h, sym);
                self.apply_effects(ctx, layer, sym, m, _view);
            }
        } else {
            self.draw_viewport(ctx, sym, m, x, y, w, h, sym);
        }
    }

    fn is_ancestor(&self, a: usize, mut b: usize) -> bool {
        loop {
            if a == b {
                return true;
            }
            match self.doc.items[b].parent {
                Some(p) => b = p,
                None => return false,
            }
        }
    }

    // ---------------------------------------------------------------- shapes

    /// The shape outline in user space as a command list (`None` when the
    /// element does not render).
    pub fn shape_path(&self, i: usize, view: View) -> Option<Vec<Seg>> {
        let name = self.name(i);
        let mut out = Vec::new();
        match name {
            "rect" => {
                let x = self.len(i, "x", Axis::X, view, 0.0);
                let y = self.len(i, "y", Axis::Y, view, 0.0);
                let w = self.len(i, "width", Axis::X, view, 0.0);
                let h = self.len(i, "height", Axis::Y, view, 0.0);
                if w <= 0.0 || h <= 0.0 {
                    return None;
                }
                let c = self.ctx_for(self.style(i), view);
                let rx_v = self.geometry(i, "rx").and_then(|v| units::length(&v, Axis::X, &c)).filter(|v| *v >= 0.0);
                let ry_v = self.geometry(i, "ry").and_then(|v| units::length(&v, Axis::Y, &c)).filter(|v| *v >= 0.0);
                let (mut rx, mut ry) = match (rx_v, ry_v) {
                    (Some(a), Some(b)) => (a, b),
                    (Some(a), None) => (a, a),
                    (None, Some(b)) => (b, b),
                    (None, None) => (0.0, 0.0),
                };
                rx = rx.min(w / 2.0);
                ry = ry.min(h / 2.0);
                if rx > 0.0 && ry > 0.0 {
                    // SVG 2 §10.2: start at (x + rx, y), clockwise, with
                    // elliptical arcs at the corners.
                    use std::f64::consts::{FRAC_PI_2, PI};
                    out.push(Seg::Move(x + rx, y));
                    out.push(Seg::Line(x + w - rx, y));
                    out.push(Seg::Arc(x + w - rx, y + ry, rx, ry, 0.0, -FRAC_PI_2, 0.0, false));
                    out.push(Seg::Line(x + w, y + h - ry));
                    out.push(Seg::Arc(x + w - rx, y + h - ry, rx, ry, 0.0, 0.0, FRAC_PI_2, false));
                    out.push(Seg::Line(x + rx, y + h));
                    out.push(Seg::Arc(x + rx, y + h - ry, rx, ry, 0.0, FRAC_PI_2, PI, false));
                    out.push(Seg::Line(x, y + ry));
                    out.push(Seg::Arc(x + rx, y + ry, rx, ry, 0.0, PI, PI + FRAC_PI_2, false));
                    out.push(Seg::Close);
                } else {
                    out.extend([Seg::Move(x, y), Seg::Line(x + w, y), Seg::Line(x + w, y + h), Seg::Line(x, y + h), Seg::Close]);
                }
            }
            "circle" | "ellipse" => {
                let cx = self.len(i, "cx", Axis::X, view, 0.0);
                let cy = self.len(i, "cy", Axis::Y, view, 0.0);
                let (rx, ry) = if name == "circle" {
                    let r = self.len(i, "r", Axis::Diagonal, view, 0.0);
                    (r, r)
                } else {
                    let c = self.ctx_for(self.style(i), view);
                    let rx = self.geometry(i, "rx").and_then(|v| units::length(&v, Axis::X, &c));
                    let ry = self.geometry(i, "ry").and_then(|v| units::length(&v, Axis::Y, &c));
                    match (rx, ry) {
                        (Some(a), Some(b)) => (a, b),
                        (Some(a), None) => (a, a),
                        (None, Some(b)) => (b, b),
                        _ => (0.0, 0.0),
                    }
                };
                if rx <= 0.0 || ry <= 0.0 {
                    return None;
                }
                // SVG 2 §10.3: start at (cx + rx, cy), clockwise; four
                // quarter arcs so that markers and text paths see the
                // quadrant points.
                use std::f64::consts::FRAC_PI_2;
                out.push(Seg::Move(cx + rx, cy));
                for k in 0..4 {
                    let a0 = f64::from(k) * FRAC_PI_2;
                    out.push(Seg::Arc(cx, cy, rx, ry, 0.0, a0, a0 + FRAC_PI_2, false));
                }
                out.push(Seg::Close);
            }
            "line" => {
                let x1 = self.len(i, "x1", Axis::X, view, 0.0);
                let y1 = self.len(i, "y1", Axis::Y, view, 0.0);
                let x2 = self.len(i, "x2", Axis::X, view, 0.0);
                let y2 = self.len(i, "y2", Axis::Y, view, 0.0);
                out.push(Seg::Move(x1, y1));
                out.push(Seg::Line(x2, y2));
            }
            "polyline" | "polygon" => {
                let pts = units::number_list(self.attr(i, "points").unwrap_or(""));
                if pts.len() < 2 {
                    return None;
                }
                out.push(Seg::Move(pts[0], pts[1]));
                for p in pts[2..].chunks_exact(2) {
                    out.push(Seg::Line(p[0], p[1]));
                }
                if name == "polygon" {
                    out.push(Seg::Close);
                }
            }
            "path" => {
                let d = self.geometry(i, "d")?;
                let d = d.trim();
                let d = d.strip_prefix("path(").map(|x| x.trim_end_matches(')').trim().trim_matches(|c| c == '"' || c == '\'')).unwrap_or(d);
                out = segments_from_commands(&narsil_canvas::commands::parse_svg_path(d));
                if out.is_empty() {
                    return None;
                }
            }
            _ => return None,
        }
        Some(out)
    }

    fn draw_shape(&mut self, ctx: &mut Context2D, i: usize, m: Matrix, view: View) {
        let Some(segs) = self.shape_path(i, view) else { return };
        if self.style(i).get("visibility").to_ascii_lowercase() != "visible" {
            return;
        }
        let bbox = segs_bbox(&segs);
        let path = build_path(&segs, &m);
        let order = paint_order(self.style(i).get("paint-order"));
        let (fill_style, stroke_style) = (self.paint_style(i, true, bbox, m, view), self.paint_style(i, false, bbox, m, view));
        let dash_scale = self.dash_scale(i, &segs);
        for step in order {
            match step {
                'f' => self.fill_path(ctx, i, &path, m, fill_style.clone()),
                's' => self.stroke_path(ctx, i, &path, m, view, stroke_style.clone(), dash_scale),
                _ => {
                    if matches!(self.name(i), "path" | "line" | "polyline" | "polygon") {
                        let (fs, ss) = (fill_style.clone().map(|f| f.0), stroke_style.clone().map(|s| s.0));
                        crate::marker::draw_markers(self, ctx, i, &segs, m, view, fs, ss);
                    }
                }
            }
        }
    }

    /// The fill or stroke paint as a canvas style with its alpha.
    pub fn paint_style(&mut self, i: usize, fill: bool, bbox: Option<BBox>, m: Matrix, view: View) -> Option<(Style, f64)> {
        let st = self.style(i).clone();
        let (prop, op) = if fill { ("fill", "fill-opacity") } else { ("stroke", "stroke-opacity") };
        let opacity = units::fraction(st.get(op)).unwrap_or(1.0).clamp(0.0, 1.0);
        let current = color::parse(st.get("color"), Color::BLACK).unwrap_or(Color::BLACK);
        match self.paint_ref(st.get(prop), current) {
            PaintRef::None => None,
            PaintRef::Color(c) => Some((Style::Color(c), opacity)),
            PaintRef::ContextFill => self.context_paint.last().map(|(f, _, from)| (rebase(f.clone(), from, &m), opacity)),
            PaintRef::ContextStroke => self.context_paint.last().map(|(_, s, from)| (rebase(s.clone(), from, &m), opacity)),
            PaintRef::Server(server, fallback) => match self.server_style(server, bbox, m, view) {
                Some(s) => s.map(|s| (s, opacity)),
                None => fallback.map(|c| (Style::Color(c), opacity)),
            },
        }
    }

    fn paint_ref(&self, v: &str, current: Color) -> PaintRef {
        let v = v.trim();
        let lower = v.to_ascii_lowercase();
        if lower == "none" || lower.is_empty() {
            return PaintRef::None;
        }
        if lower == "context-fill" {
            return PaintRef::ContextFill;
        }
        if lower == "context-stroke" {
            return PaintRef::ContextStroke;
        }
        if lower.starts_with("url(") {
            let close = v.find(')').unwrap_or(v.len() - 1);
            let reference = &v[..=close.min(v.len() - 1)];
            let rest = v[close + 1..].trim();
            let fallback = if rest.is_empty() { None } else if rest.eq_ignore_ascii_case("none") { Some(None) } else { color::parse(rest, current).map(Some) };
            return match self.doc.lookup(reference) {
                Some(s) => PaintRef::Server(s, fallback.flatten()),
                None => match fallback {
                    Some(Some(c)) => PaintRef::Color(c),
                    _ => PaintRef::None,
                },
            };
        }
        match color::parse(v, current) {
            Some(c) => PaintRef::Color(c),
            None => PaintRef::None,
        }
    }

    pub(crate) fn fill_path(&mut self, ctx: &mut Context2D, i: usize, path: &Path, m: Matrix, paint: Option<(Style, f64)>) {
        let Some((style, alpha)) = paint else { return };
        let rule = if self.style(i).get("fill-rule").eq_ignore_ascii_case("evenodd") { FillRule::EvenOdd } else { FillRule::NonZero };
        ctx.save();
        ctx.state.transform = m;
        ctx.state.fill = style;
        ctx.state.global_alpha *= alpha * self.alpha_scale;
        ctx.fill(Some(path), rule);
        let _ = ctx.restore();
    }

    #[allow(clippy::too_many_arguments)]
    /// How `pathLength` scales distances along the outline of shape `i`
    /// (`INFINITY` for `pathLength="0"`).
    pub(crate) fn dash_scale(&self, i: usize, segs: &[Seg]) -> f64 {
        match self.attr(i, "pathLength").and_then(units::number) {
            Some(p) if p > 0.0 => path_length(segs) / p,
            Some(p) if p == 0.0 => f64::INFINITY,
            _ => 1.0,
        }
    }

    /// Strokes `path`; `dash_scale` maps dash lengths through `pathLength`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn stroke_path(&mut self, ctx: &mut Context2D, i: usize, path: &Path, m: Matrix, view: View, paint: Option<(Style, f64)>, dash_scale: f64) {
        let Some((style, alpha)) = paint else { return };
        let st = self.style(i).clone();
        let c = self.ctx_for(&st, view);
        let width = units::length(st.get("stroke-width"), Axis::Diagonal, &c).unwrap_or(1.0);
        if width <= 0.0 {
            return;
        }
        ctx.save();
        let non_scaling = st.get("vector-effect").eq_ignore_ascii_case("non-scaling-stroke");
        ctx.state.transform = if non_scaling { Matrix::IDENTITY } else { m };
        if non_scaling {
            // The paint keeps its user-space mapping through the gradient
            // transform.
            ctx.state.stroke = match style {
                Style::Gradient(g) => {
                    let mut g2 = (*g).clone();
                    g2.transform = m.multiply(&g2.transform);
                    Style::Gradient(Arc::new(g2))
                }
                other => other,
            };
        } else {
            ctx.state.stroke = style;
        }
        ctx.state.global_alpha *= alpha * self.alpha_scale;
        let line = &mut ctx.state.line;
        line.width = width;
        line.cap = match st.get("stroke-linecap").to_ascii_lowercase().as_str() {
            "round" => LineCap::Round,
            "square" => LineCap::Square,
            _ => LineCap::Butt,
        };
        line.join = match st.get("stroke-linejoin").to_ascii_lowercase().as_str() {
            "round" => LineJoin::Round,
            "bevel" => LineJoin::Bevel,
            _ => LineJoin::Miter,
        };
        line.miter_limit = units::number(st.get("stroke-miterlimit")).filter(|v| *v >= 1.0).unwrap_or(4.0);
        let dash = st.get("stroke-dasharray");
        line.dash.clear();
        if !dash.eq_ignore_ascii_case("none") {
            if let Some(list) = units::length_list(dash, Axis::Diagonal, &c) {
                if list.iter().all(|v| *v >= 0.0) && list.iter().any(|v| *v > 0.0) {
                    let mut l = list;
                    if l.len() % 2 == 1 {
                        let copy = l.clone();
                        l.extend(copy);
                    }
                    line.dash = l;
                }
            }
        }
        line.dash_offset = units::length(st.get("stroke-dashoffset"), Axis::Diagonal, &c).unwrap_or(0.0);
        if dash_scale.is_infinite() {
            line.dash.clear();
        } else if dash_scale != 1.0 && dash_scale > 0.0 {
            for v in &mut line.dash {
                *v *= dash_scale;
            }
            line.dash_offset *= dash_scale;
        }
        ctx.stroke(Some(path));
        let _ = ctx.restore();
    }

    // ---------------------------------------------------------------- servers

    /// A gradient or pattern as a canvas style for a shape with user-space
    /// bounding box `bbox`.
    /// `None`: the reference is invalid (the fallback applies);
    /// `Some(None)`: the server paints nothing (a gradient without stops,
    /// an empty pattern).
    fn server_style(&mut self, s: usize, bbox: Option<BBox>, m: Matrix, view: View) -> Option<Option<Style>> {
        match self.name(s) {
            "linearGradient" | "radialGradient" => {
                if self.gradient_stops_from(s).is_none() {
                    return Some(None);
                }
                self.gradient(s, bbox, view).map(|g| Some(Style::Gradient(Arc::new(g))))
            }
            "pattern" => Some(self.pattern(s, bbox, m, view).map(|p| Style::Pattern(Arc::new(p)))),
            _ => None,
        }
    }

    /// The element of a gradient's `href` chain whose stops it uses;
    /// `None` when no element of the chain has stops. Templates inside
    /// text content are not in the rendering tree and are skipped.
    fn gradient_stops_from(&self, s: usize) -> Option<usize> {
        let mut cur = s;
        for _ in 0..16 {
            if self.doc.items[cur].children.iter().any(|c| self.name(*c) == "stop") {
                return Some(cur);
            }
            match self.attr(cur, "href").and_then(|r| self.doc.lookup(r)) {
                Some(n) if matches!(self.name(n), "linearGradient" | "radialGradient") && !self.in_text(n) => cur = n,
                _ => return None,
            }
        }
        None
    }

    fn in_text(&self, mut i: usize) -> bool {
        while let Some(p) = self.doc.items[i].parent {
            if matches!(self.name(p), "text" | "tspan" | "textPath") {
                return true;
            }
            i = p;
        }
        false
    }

    /// A transform attribute through the `href` chain: an empty or invalid
    /// value is not specified, so the template's applies.
    fn inherited_transform(&self, s: usize, name: &str) -> Matrix {
        let mut cur = s;
        for _ in 0..16 {
            if let Some(m) = self.attr(cur, name).filter(|v| !v.trim().is_empty()).and_then(transform::parse) {
                return m;
            }
            match self.attr(cur, "href").and_then(|r| self.doc.lookup(r)) {
                Some(n) => cur = n,
                None => break,
            }
        }
        Matrix::IDENTITY
    }

    /// Attribute lookup through the `href` chain of gradients/patterns.
    fn inherited_attr(&self, s: usize, name: &str) -> Option<String> {
        let mut cur = s;
        for _ in 0..16 {
            if let Some(v) = self.attr(cur, name) {
                return Some(v.to_string());
            }
            cur = self.attr(cur, "href").and_then(|r| self.doc.lookup(r))?;
        }
        None
    }

    fn gradient(&mut self, s: usize, bbox: Option<BBox>, view: View) -> Option<Gradient> {
        // Stops come from the first element in the href chain that has any.
        let stops_from = self.gradient_stops_from(s)?;
        let object_bbox = self.inherited_attr(s, "gradientUnits").as_deref() != Some("userSpaceOnUse");
        if object_bbox {
            let (x0, y0, x1, y1) = bbox?;
            if x1 - x0 <= 0.0 || y1 - y0 <= 0.0 {
                return None;
            }
        }
        let c = Ctx { vw: view.w, vh: view.h, font_size: 16.0 };
        let coord = |name: &str, default: &str, axis: Axis| -> f64 {
            let v = self.inherited_attr(s, name).unwrap_or_else(|| default.to_string());
            if object_bbox {
                units::fraction(&v).unwrap_or(0.0)
            } else {
                units::length(&v, axis, &c).unwrap_or(0.0)
            }
        };
        let kind = if self.name(s) == "linearGradient" {
            GradientKind::Linear { x0: coord("x1", "0%", Axis::X), y0: coord("y1", "0%", Axis::Y), x1: coord("x2", "100%", Axis::X), y1: coord("y2", "0%", Axis::Y) }
        } else {
            let cx = coord("cx", "50%", Axis::X);
            let cy = coord("cy", "50%", Axis::Y);
            let r = coord("r", "50%", Axis::Diagonal);
            let fx = if self.inherited_attr(s, "fx").is_some() { coord("fx", "50%", Axis::X) } else { cx };
            let fy = if self.inherited_attr(s, "fy").is_some() { coord("fy", "50%", Axis::Y) } else { cy };
            let fr = coord("fr", "0%", Axis::Diagonal);
            GradientKind::Radial { x0: fx, y0: fy, r0: fr, x1: cx, y1: cy, r1: r }
        };
        let mut g = Gradient::new(kind);
        let mut last = 0.0f32;
        let kids = self.doc.items[stops_from].children.clone();
        for k in kids {
            if self.name(k) != "stop" {
                continue;
            }
            let off = self.attr(k, "offset").and_then(units::fraction).unwrap_or(0.0).clamp(0.0, 1.0) as f32;
            let off = off.max(last);
            last = off;
            let st = self.style(k);
            let current = color::parse(st.get("color"), Color::BLACK).unwrap_or(Color::BLACK);
            let mut col = color::parse(st.get("stop-color"), current).unwrap_or(Color::BLACK);
            col.a *= units::fraction(st.get("stop-opacity")).unwrap_or(1.0).clamp(0.0, 1.0) as f32;
            col.css = None;
            g.stops.push(ColorStop { offset: off, color: col });
        }
        if g.stops.is_empty() {
            return None;
        }
        if g.stops.len() == 1 {
            let only = g.stops[0];
            g.stops.push(ColorStop { offset: 1.0, color: only.color });
            g.stops[0].offset = 0.0;
        }
        if self.style(s).get("color-interpolation").eq_ignore_ascii_case("linearrgb") {
            g.use_linear_rgb();
        }
        g.spread = match self.inherited_attr(s, "spreadMethod").as_deref() {
            Some("reflect") => Spread::Reflect,
            Some("repeat") => Spread::Repeat,
            _ => Spread::Pad,
        };
        let gt = self.inherited_transform(s, "gradientTransform");
        let units_m = match (object_bbox, bbox) {
            (true, Some((x0, y0, x1, y1))) => Matrix::new(x1 - x0, 0.0, 0.0, y1 - y0, x0, y0),
            _ => Matrix::IDENTITY,
        };
        g.transform = units_m.multiply(&gt);
        Some(g)
    }

    fn pattern(&mut self, s: usize, bbox: Option<BBox>, m: Matrix, view: View) -> Option<Pattern> {
        if self.active.contains(&s) || self.active.len() >= MAX_DEPTH {
            return None;
        }
        // Content from the first pattern in the href chain with children.
        let mut content = s;
        for _ in 0..16 {
            if !self.doc.items[content].children.is_empty() {
                break;
            }
            match self.attr(content, "href").and_then(|r| self.doc.lookup(r)) {
                Some(n) if self.name(n) == "pattern" => content = n,
                _ => break,
            }
        }
        let units_bbox = self.inherited_attr(s, "patternUnits").as_deref() != Some("userSpaceOnUse");
        let content_bbox = self.inherited_attr(s, "patternContentUnits").as_deref() == Some("objectBoundingBox");
        let c = Ctx { vw: view.w, vh: view.h, font_size: 16.0 };
        let val = |name: &str, default: &str, axis: Axis| -> f64 {
            let v = self.inherited_attr(s, name).unwrap_or_else(|| default.to_string());
            if units_bbox {
                units::fraction(&v).unwrap_or(0.0)
            } else {
                units::length(&v, axis, &c).unwrap_or(0.0)
            }
        };
        let (mut x, mut y, mut w, mut h) = (val("x", "0", Axis::X), val("y", "0", Axis::Y), val("width", "0", Axis::X), val("height", "0", Axis::Y));
        let bb = bbox.unwrap_or((0.0, 0.0, 0.0, 0.0));
        if units_bbox {
            let (bw, bh) = (bb.2 - bb.0, bb.3 - bb.1);
            x = bb.0 + x * bw;
            y = bb.1 + y * bh;
            w *= bw;
            h *= bh;
        }
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        if transform::view_box_disables(self.inherited_attr(s, "viewBox").as_deref()) {
            return None;
        }
        let pt = self.inherited_transform(s, "patternTransform");
        // Tile resolution: the device scale of the pattern space.
        let dev = m.multiply(&pt);
        let scale = dev.max_scale().clamp(0.01, 64.0);
        let (tw, th) = ((w * scale).ceil().clamp(1.0, 4096.0), (h * scale).ceil().clamp(1.0, 4096.0));
        let (kx, ky) = (tw / w, th / h);
        let mut tile = Context2D::new(tw as u32, th as u32, false)?;
        let mut content_m = Matrix::new(kx, 0.0, 0.0, ky, 0.0, 0.0);
        let vb = self.inherited_attr(s, "viewBox").and_then(|v| transform::view_box(&v));
        let tile_view;
        if let Some(vb) = vb {
            let par = self.inherited_attr(s, "preserveAspectRatio").unwrap_or_else(|| "xMidYMid meet".into());
            content_m = content_m.multiply(&transform::view_box_transform(vb, &par, 0.0, 0.0, w, h));
            tile_view = View { w: vb[2], h: vb[3] };
        } else if content_bbox {
            content_m = content_m.multiply(&Matrix::new(bb.2 - bb.0, 0.0, 0.0, bb.3 - bb.1, 0.0, 0.0));
            tile_view = view;
        } else {
            content_m = content_m.multiply(&Matrix::translate(-x, -y));
            tile_view = view;
        }
        // Patterns render with their own pixel size; keep the renderer's.
        let (ow, oh) = (self.w, self.h);
        self.w = tw as u32;
        self.h = th as u32;
        self.active.push(s);
        self.draw_children(&mut tile, content, content_m, tile_view);
        self.active.pop();
        self.w = ow;
        self.h = oh;
        let image = tile.surface.snapshot();
        let transform = pt.multiply(&Matrix::new(1.0 / kx, 0.0, 0.0, 1.0 / ky, x, y));
        Some(Pattern { image, repetition: Repetition::Repeat, transform })
    }

    // ---------------------------------------------------------------- image

    fn draw_image(&mut self, ctx: &mut Context2D, i: usize, m: Matrix, view: View) {
        if self.style(i).get("visibility").to_ascii_lowercase() != "visible" {
            return;
        }
        let Some(href) = self.attr(i, "href") else { return };
        let Some(img) = crate::image::decode_href(href) else { return };
        let x = self.len(i, "x", Axis::X, view, 0.0);
        let y = self.len(i, "y", Axis::Y, view, 0.0);
        let (iw, ih) = (img.width as f64, img.height as f64);
        let c = self.ctx_for(self.style(i), view);
        let w0 = self.geometry(i, "width").and_then(|v| units::length(&v, Axis::X, &c));
        let h0 = self.geometry(i, "height").and_then(|v| units::length(&v, Axis::Y, &c));
        let (w, h) = match (w0, h0) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * ih / iw.max(1.0)),
            (None, Some(h)) => (h * iw / ih.max(1.0), h),
            (None, None) => (iw, ih),
        };
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let par = self.attr(i, "preserveAspectRatio").unwrap_or("xMidYMid meet").to_string();
        let fit = transform::view_box_transform([0.0, 0.0, iw, ih], &par, x, y, w, h);
        ctx.save();
        let mut clip = Path::new();
        clip.rect(&m, x, y, w, h);
        ctx.clip(Some(&clip), FillRule::NonZero);
        ctx.state.transform = m.multiply(&fit);
        ctx.state.smoothing = !matches!(self.style(i).get("image-rendering").to_ascii_lowercase().as_str(), "pixelated" | "optimizespeed" | "crisp-edges");
        ctx.state.global_alpha *= self.alpha_scale;
        ctx.draw_image(&img, 0.0, 0.0, iw, ih, 0.0, 0.0, iw, ih);
        let _ = ctx.restore();
    }

    // ---------------------------------------------------------------- effects

    /// Composites an offscreen `layer` with the element's clip-path, mask,
    /// filter, opacity and blend mode.
    fn apply_effects(&mut self, ctx: &mut Context2D, layer: Context2D, i: usize, m: Matrix, view: View) {
        if let Some((img, opacity, op)) = self.effects_layer(layer, i, m, view) {
            ctx.save();
            ctx.state.transform = Matrix::IDENTITY;
            ctx.state.global_alpha = opacity;
            ctx.state.op = op;
            ctx.state.smoothing = false;
            let (w, h) = (f64::from(img.width), f64::from(img.height));
            ctx.draw_image(&img, 0.0, 0.0, w, h, 0.0, 0.0, w, h);
            let _ = ctx.restore();
        }
    }

    /// Runs filter, clip-path and mask over `layer`; the image to composite
    /// with its opacity and blend mode (`None`: nothing is rendered).
    fn effects_layer(&mut self, mut layer: Context2D, i: usize, m: Matrix, view: View) -> Option<(narsil_canvas::Image, f64, CompositeOp)> {
        let st = self.style(i).clone();
        let bbox = self.bbox(i, Matrix::IDENTITY, view);
        let filter = st.get("filter").to_string();
        if !filter.eq_ignore_ascii_case("none") {
            match crate::filter::apply(self, &mut layer, &filter, i, m, view, bbox) {
                crate::filter::Outcome::Applied => {}
                // An empty filter region disables rendering.
                crate::filter::Outcome::Hide => return None,
                crate::filter::Outcome::Ignore => {}
            }
        }
        let clip = st.get("clip-path").to_string();
        if !clip.eq_ignore_ascii_case("none") {
            match crate::clip::coverage(self, &clip, i, m, view, bbox) {
                Some(mask) => multiply_alpha(&mut layer, &mask, false),
                None => {
                    if clip.to_ascii_lowercase().starts_with("url(") {
                        // A missing clip path reference: the element is
                        // rendered unclipped (SVG 2 / CSS Masking).
                    }
                }
            }
        }
        let mask = st.get("mask").to_string();
        if !mask.eq_ignore_ascii_case("none") {
            if let Some((mk, luminance)) = crate::clip::mask(self, &mask, i, m, view, bbox) {
                multiply_alpha(&mut layer, &mk, luminance);
            }
        }
        let opacity = units::fraction(st.get("opacity")).unwrap_or(1.0).clamp(0.0, 1.0);
        let op = match st.get("mix-blend-mode").to_ascii_lowercase().as_str() {
            "normal" => CompositeOp::SourceOver,
            other => CompositeOp::parse(other).filter(|o| !matches!(o, CompositeOp::Copy | CompositeOp::Xor)).unwrap_or(CompositeOp::SourceOver),
        };
        Some((layer.surface.snapshot(), opacity, op))
    }

    // ---------------------------------------------------------------- bbox

    /// The object bounding box of element `i` in the coordinate system `m`
    /// maps its user space into (fill geometry only).
    pub fn bbox(&self, i: usize, m: Matrix, view: View) -> Option<BBox> {
        let name = self.name(i);
        match name {
            "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "path" => {
                let segs = self.shape_path(i, view)?;
                let p = build_path(&segs, &m);
                let b = p.bounds();
                (b.x0.is_finite()).then_some((b.x0, b.y0, b.x1, b.y1))
            }
            "g" | "a" | "svg" | "switch" | "symbol" | "use" | "text" | "tspan" | "clipPath" | "mask" | "marker" => {
                if name == "use" {
                    let target = self.attr(i, "href").and_then(|r| self.doc.lookup(r))?;
                    if self.is_ancestor(target, i) {
                        return None;
                    }
                    let x = self.len(i, "x", Axis::X, view, 0.0);
                    let y = self.len(i, "y", Axis::Y, view, 0.0);
                    let tm = m.multiply(&Matrix::translate(x, y)).multiply(&self.own_transform(target, view));
                    return if matches!(self.name(target), "symbol" | "svg") { self.children_bbox(target, tm, view) } else { self.bbox(target, tm, view) };
                }
                if name == "text" || name == "tspan" {
                    return crate::text::text_bbox(self, i, m, view);
                }
                self.children_bbox(i, m, view)
            }
            "image" => {
                let x = self.len(i, "x", Axis::X, view, 0.0);
                let y = self.len(i, "y", Axis::Y, view, 0.0);
                let w = self.len(i, "width", Axis::X, view, 0.0);
                let h = self.len(i, "height", Axis::Y, view, 0.0);
                let pts = [m.apply(x, y), m.apply(x + w, y), m.apply(x, y + h), m.apply(x + w, y + h)];
                Some(pts.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p.0), a.1.min(p.1), a.2.max(p.0), a.3.max(p.1))))
            }
            _ => None,
        }
    }

    fn children_bbox(&self, i: usize, m: Matrix, view: View) -> Option<BBox> {
        let mut acc: Option<BBox> = None;
        for &c in &self.doc.items[i].children {
            if self.style(c).get("display").eq_ignore_ascii_case("none") {
                continue;
            }
            let cm = m.multiply(&self.own_transform(c, view));
            if let Some(b) = self.bbox(c, cm, view) {
                acc = Some(match acc {
                    None => b,
                    Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
                });
            }
        }
        acc
    }

    pub(crate) fn push_context_paint(&mut self, fill: Option<Style>, stroke: Option<Style>, m: Matrix) {
        let none = Style::Color(Color::TRANSPARENT);
        self.context_paint.push((fill.unwrap_or_else(|| none.clone()), stroke.unwrap_or(none), m));
    }

    pub(crate) fn pop_context_paint(&mut self) {
        self.context_paint.pop();
    }

    pub(crate) fn enter(&mut self, i: usize) -> bool {
        if self.active.contains(&i) || self.active.len() >= MAX_DEPTH {
            return false;
        }
        self.active.push(i);
        true
    }

    pub(crate) fn leave(&mut self) {
        self.active.pop();
    }
}

/// A paint server style made for the user space `from`, for use in `to`.
fn rebase(style: Style, from: &Matrix, to: &Matrix) -> Style {
    if from == to {
        return style;
    }
    let Some(inv) = to.invert() else { return style };
    let k = inv.multiply(from);
    match style {
        Style::Gradient(g) => {
            let mut g2 = (*g).clone();
            g2.transform = k.multiply(&g2.transform);
            Style::Gradient(Arc::new(g2))
        }
        Style::Pattern(p) => {
            let mut p2 = (*p).clone();
            p2.transform = k.multiply(&p2.transform);
            Style::Pattern(Arc::new(p2))
        }
        other => other,
    }
}

/// Multiplies the layer's pixels by a coverage (alpha) or luminance mask.
fn multiply_alpha(layer: &mut Context2D, mask: &Context2D, luminance: bool) {
    let src = mask.surface.data.clone();
    let data = Arc::make_mut(&mut layer.surface.data);
    for (px, m) in data.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let k = if luminance {
            // Premultiplied channels: luminance already includes alpha.
            (0.2125 * m[0] as f32 + 0.7154 * m[1] as f32 + 0.0721 * m[2] as f32) / 255.0
        } else {
            m[3] as f32 / 255.0
        };
        for v in px.iter_mut() {
            *v = (*v as f32 * k + 0.5) as u8;
        }
    }
    layer.dirty = true;
}

/// Whether a viewport-establishing element (nested `svg`, `symbol`,
/// `marker`, `pattern`, `image`) clips: the UA style sheet sets
/// `overflow: hidden` on them, and `visible`/`auto` turn the clip off.
pub(crate) fn overflow_clips(st: &Computed) -> bool {
    if !st.is_set("overflow") {
        return true;
    }
    !matches!(st.get("overflow").to_ascii_lowercase().as_str(), "visible" | "auto")
}

fn origin_value(p: &str, axis: Axis, c: &Ctx) -> Option<f64> {
    match p.to_ascii_lowercase().as_str() {
        "left" | "top" => Some(0.0),
        "center" => Some(match axis {
            Axis::X => c.vw / 2.0,
            _ => c.vh / 2.0,
        }),
        "right" => Some(c.vw),
        "bottom" => Some(c.vh),
        _ => units::length(p, axis, c),
    }
}

/// `requiredExtensions` / `systemLanguage` conditional processing.
fn conditions_pass(el: &crate::xml::Element) -> bool {
    if el.attr("requiredExtensions").is_some_and(|v| !v.trim().is_empty()) {
        return false;
    }
    if let Some(langs) = el.attr("systemLanguage") {
        return langs.split(',').any(|l| {
            let l = l.trim().to_ascii_lowercase();
            l == "en" || l.starts_with("en-")
        });
    }
    true
}

/// The computed font size in user units.
pub fn font_size(st: &Computed, view: View) -> f64 {
    let c = Ctx { vw: view.w, vh: view.h, font_size: 16.0 };
    let v = st.get("font-size").to_ascii_lowercase();
    match v.as_str() {
        "xx-small" => 9.0,
        "x-small" => 10.0,
        "small" => 13.0,
        "medium" => 16.0,
        "large" => 18.0,
        "x-large" => 24.0,
        "xx-large" => 32.0,
        _ => units::length(&v, Axis::Y, &c).filter(|v| *v >= 0.0).unwrap_or(16.0),
    }
}

pub(crate) fn paint_order(v: &str) -> Vec<char> {
    let mut out = Vec::new();
    for w in v.split_whitespace() {
        let c = match w.to_ascii_lowercase().as_str() {
            "fill" => 'f',
            "stroke" => 's',
            "markers" => 'm',
            _ => continue,
        };
        if !out.contains(&c) {
            out.push(c);
        }
    }
    for c in ['f', 's', 'm'] {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// A path segment in user space (absolute coordinates).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(f64, f64),
    Line(f64, f64),
    Quad(f64, f64, f64, f64),
    Cubic(f64, f64, f64, f64, f64, f64),
    /// Center, radii, rotation, start angle, end angle, anticlockwise.
    Arc(f64, f64, f64, f64, f64, f64, f64, bool),
    Close,
}

/// Converts canvas command lists (from the SVG path parser) to segments.
#[must_use]
pub fn segments_from_commands(cmds: &[f64]) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < cmds.len() {
        let op = cmds[i];
        let Some(n) = narsil_canvas::commands::arity(op) else { break };
        if i + 1 + n > cmds.len() {
            break;
        }
        let a = &cmds[i + 1..i + 1 + n];
        i += 1 + n;
        out.push(match op as u32 {
            0 => Seg::Move(a[0], a[1]),
            1 => Seg::Line(a[0], a[1]),
            2 => Seg::Quad(a[0], a[1], a[2], a[3]),
            3 => Seg::Cubic(a[0], a[1], a[2], a[3], a[4], a[5]),
            5 => Seg::Arc(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7] != 0.0),
            8 => Seg::Close,
            _ => continue,
        });
    }
    out
}

/// A device-space path from segments under `m`.
#[must_use]
pub fn build_path(segs: &[Seg], m: &Matrix) -> Path {
    let mut p = Path::new();
    for s in segs {
        match *s {
            Seg::Move(x, y) => p.move_to(m, x, y),
            Seg::Line(x, y) => p.line_to(m, x, y),
            Seg::Quad(a, b, x, y) => p.quad_to(m, a, b, x, y),
            Seg::Cubic(a, b, c, d, x, y) => p.cubic_to(m, a, b, c, d, x, y),
            Seg::Arc(cx, cy, rx, ry, rot, s0, s1, ccw) => {
                let _ = p.ellipse(m, cx, cy, rx, ry, rot, s0, s1, ccw);
            }
            Seg::Close => p.close(),
        }
    }
    p
}

/// Segments flattened to polylines: (points, closed) per subpath.
#[must_use]
pub fn flatten(segs: &[Seg]) -> Vec<(Vec<(f64, f64)>, bool)> {
    let mut out: Vec<(Vec<(f64, f64)>, bool)> = Vec::new();
    let (mut cx, mut cy) = (0.0, 0.0);
    let (mut sx, mut sy) = (0.0, 0.0);
    let steps = |len: f64| (len / 2.0).ceil().clamp(4.0, 256.0) as usize;
    for s in segs {
        match *s {
            Seg::Move(x, y) => {
                out.push((vec![(x, y)], false));
                (cx, cy, sx, sy) = (x, y, x, y);
            }
            Seg::Line(x, y) => {
                if out.is_empty() {
                    out.push((vec![(cx, cy)], false));
                }
                out.last_mut().unwrap().0.push((x, y));
                (cx, cy) = (x, y);
            }
            Seg::Quad(a, b, x, y) => {
                if out.is_empty() {
                    out.push((vec![(cx, cy)], false));
                }
                let n = steps((a - cx).hypot(b - cy) + (x - a).hypot(y - b));
                let pts = &mut out.last_mut().unwrap().0;
                for k in 1..=n {
                    let t = k as f64 / n as f64;
                    let u = 1.0 - t;
                    pts.push((u * u * cx + 2.0 * u * t * a + t * t * x, u * u * cy + 2.0 * u * t * b + t * t * y));
                }
                (cx, cy) = (x, y);
            }
            Seg::Cubic(a, b, c, d, x, y) => {
                if out.is_empty() {
                    out.push((vec![(cx, cy)], false));
                }
                let n = steps((a - cx).hypot(b - cy) + (c - a).hypot(d - b) + (x - c).hypot(y - d));
                let pts = &mut out.last_mut().unwrap().0;
                for k in 1..=n {
                    let t = k as f64 / n as f64;
                    let u = 1.0 - t;
                    let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    pts.push((w0 * cx + w1 * a + w2 * c + w3 * x, w0 * cy + w1 * b + w2 * d + w3 * y));
                }
                (cx, cy) = (x, y);
            }
            Seg::Arc(ecx, ecy, rx, ry, rot, a0, a1, ccw) => {
                let sweep = narsil_canvas::path::arc_sweep(a0, a1, ccw);
                let n = steps(sweep.abs() * rx.max(ry));
                let (sr, cr) = rot.sin_cos();
                let pt = |t: f64| {
                    let (ex, ey) = (rx * t.cos(), ry * t.sin());
                    (ecx + ex * cr - ey * sr, ecy + ex * sr + ey * cr)
                };
                if out.is_empty() {
                    out.push((vec![pt(a0)], false));
                }
                let pts = &mut out.last_mut().unwrap().0;
                pts.push(pt(a0));
                for k in 1..=n {
                    pts.push(pt(a0 + sweep * k as f64 / n as f64));
                }
                (cx, cy) = pt(a0 + sweep);
            }
            Seg::Close => {
                if let Some(last) = out.last_mut() {
                    last.0.push((sx, sy));
                    last.1 = true;
                }
                (cx, cy) = (sx, sy);
            }
        }
    }
    out
}

/// Segments from flattened polylines under `t`.
#[must_use]
pub fn segments_from_polyline(sub: &[(Vec<(f64, f64)>, bool)], t: &Matrix) -> Vec<Seg> {
    let mut out = Vec::new();
    for (pts, closed) in sub {
        for (k, p) in pts.iter().enumerate() {
            let (x, y) = t.apply(p.0, p.1);
            out.push(if k == 0 { Seg::Move(x, y) } else { Seg::Line(x, y) });
        }
        if *closed {
            out.push(Seg::Close);
        }
    }
    out
}

/// The length of the outline (all subpaths).
#[must_use]
pub fn path_length(segs: &[Seg]) -> f64 {
    flatten(segs).iter().map(|(p, _)| p.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum::<f64>()).sum()
}

/// The user-space bounding box of segments (through a flattened path).
#[must_use]
pub fn segs_bbox(segs: &[Seg]) -> Option<BBox> {
    let b = build_path(segs, &Matrix::IDENTITY).bounds();
    b.x0.is_finite().then_some((b.x0, b.y0, b.x1, b.y1))
}
