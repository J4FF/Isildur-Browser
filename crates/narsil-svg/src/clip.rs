//! `clip-path` (a `<clipPath>` reference or a CSS basic shape) and `mask`
//! (a `<mask>` reference) rendered into coverage layers.

use crate::render::{build_path, BBox, Renderer, View};
use crate::transform;
use crate::units::{self, Axis, Ctx};
use narsil_canvas::{Color, Context2D, FillRule, Matrix, Path, Style};

/// The coverage of a `clip-path` value as an alpha layer; `None` when the
/// value does not clip (`none`, or a reference to a missing element).
pub fn coverage(r: &mut Renderer, value: &str, i: usize, m: Matrix, view: View, bbox: Option<BBox>) -> Option<Context2D> {
    let v = value.trim();
    if v.to_ascii_lowercase().starts_with("url(") {
        let cp = r.doc.lookup(v).filter(|c| r.doc.items[*c].el.name == "clipPath")?;
        return clip_path_layer(r, cp, m, view, bbox);
    }
    let (path, rule) = basic_shape(r, v, i, m, view, bbox)?;
    let mut layer = Context2D::new(r.w, r.h, false)?;
    layer.state.fill = Style::Color(Color::BLACK);
    layer.fill(Some(&path), rule);
    Some(layer)
}

/// A `<clipPath>` element's coverage under `m` for a referencing element
/// with bounding box `bbox`.
pub(crate) fn clip_path_layer(r: &mut Renderer, cp: usize, m: Matrix, view: View, bbox: Option<BBox>) -> Option<Context2D> {
    let mut layer = Context2D::new(r.w, r.h, false)?;
    if !r.enter(cp) {
        // A clip path that references itself clips everything away.
        return Some(layer);
    }
    let el = r.doc.items[cp].el;
    let own = transform::parse(el.attr("transform").unwrap_or("")).unwrap_or(Matrix::IDENTITY);
    let mut cm = m.multiply(&own);
    if el.attr("clipPathUnits") == Some("objectBoundingBox") {
        let Some((x0, y0, x1, y1)) = bbox else {
            r.leave();
            return Some(layer);
        };
        cm = cm.multiply(&Matrix::new(x1 - x0, 0.0, 0.0, y1 - y0, x0, y0));
    }
    layer.state.fill = Style::Color(Color::BLACK);
    let kids = r.doc.items[cp].children.clone();
    for c in kids {
        let Some((path, rule)) = child_outline(r, c, cm, view) else { continue };
        let child_clip = r.style(c).get("clip-path").to_string();
        if child_clip.eq_ignore_ascii_case("none") {
            layer.fill(Some(&path), rule);
        } else {
            // A child's own clip-path clips that child.
            let Some(mut part) = Context2D::new(r.w, r.h, false) else { continue };
            part.state.fill = Style::Color(Color::BLACK);
            part.fill(Some(&path), rule);
            let cbox = r.bbox(c, Matrix::IDENTITY, view);
            let child_m = cm.multiply(&own_transform(r, c));
            if let Some(cc) = coverage(r, &child_clip, c, child_m, view, cbox) {
                multiply(&mut part, &cc);
            }
            union(&mut layer, &part);
        }
    }
    // The clip path's own clip-path intersects.
    let outer = r.style(cp).get("clip-path").to_string();
    if !outer.eq_ignore_ascii_case("none") {
        if let Some(cc) = coverage(r, &outer, cp, m, view, bbox) {
            multiply(&mut layer, &cc);
        }
    }
    r.leave();
    Some(layer)
}

fn own_transform(r: &Renderer, i: usize) -> Matrix {
    let st = &r.style(i);
    let t = if st.is_set("transform") { st.get("transform").to_string() } else { r.doc.items[i].el.attr("transform").unwrap_or("").to_string() };
    transform::parse(&t).unwrap_or(Matrix::IDENTITY)
}

/// The device outline of a clip path child (shapes, text, and `use` of
/// those) with its clip rule.
fn child_outline(r: &mut Renderer, c: usize, cm: Matrix, view: View) -> Option<(Path, FillRule)> {
    let st = &r.style(c);
    if st.get("display").eq_ignore_ascii_case("none") || st.get("visibility").to_ascii_lowercase() != "visible" {
        return None;
    }
    let rule = if st.get("clip-rule").eq_ignore_ascii_case("evenodd") { FillRule::EvenOdd } else { FillRule::NonZero };
    let m = cm.multiply(&own_transform(r, c));
    match r.doc.items[c].el.name.as_str() {
        "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "path" => {
            let segs = r.shape_path(c, view)?;
            Some((build_path(&segs, &m), rule))
        }
        "text" => Some((crate::text::text_outline(r, c, m, view), rule)),
        "use" => {
            let target = r.doc.items[c].el.attr("href").and_then(|h| r.doc.lookup(h))?;
            if !matches!(r.doc.items[target].el.name.as_str(), "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "path" | "text") {
                return None;
            }
            let ctx = Ctx { vw: view.w, vh: view.h, font_size: 16.0 };
            let x = r.doc.items[c].el.attr("x").and_then(|v| units::length(v, Axis::X, &ctx)).unwrap_or(0.0);
            let y = r.doc.items[c].el.attr("y").and_then(|v| units::length(v, Axis::Y, &ctx)).unwrap_or(0.0);
            let um = m.multiply(&Matrix::translate(x, y));
            if !r.enter(target) {
                return None;
            }
            // The referenced element's own clip-rule applies to it.
            let out = child_outline(r, target, um, view);
            r.leave();
            out
        }
        _ => None,
    }
}

/// Multiplies `layer` by the coverage (alpha) of `by`.
pub fn multiply(layer: &mut Context2D, by: &Context2D) {
    let src = by.surface.data.clone();
    let data = std::sync::Arc::make_mut(&mut layer.surface.data);
    for (px, m) in data.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let k = m[3] as u32;
        for v in px.iter_mut() {
            *v = ((*v as u32 * k + 127) / 255) as u8;
        }
    }
    layer.dirty = true;
}

/// `layer = layer ∪ add` for coverage layers (source-over of alpha).
fn union(layer: &mut Context2D, add: &Context2D) {
    let src = add.surface.data.clone();
    let data = std::sync::Arc::make_mut(&mut layer.surface.data);
    for (px, a) in data.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        for k in 0..4 {
            let d = px[k] as u32;
            let s = a[k] as u32;
            px[k] = (s + (d * (255 - a[3] as u32) + 127) / 255).min(255) as u8;
        }
    }
    layer.dirty = true;
}

/// A CSS basic shape on an SVG element: the fill box is the object
/// bounding box, the stroke box adds half the stroke width.
fn basic_shape(r: &Renderer, v: &str, i: usize, m: Matrix, view: View, bbox: Option<BBox>) -> Option<(Path, FillRule)> {
    let fill = bbox.unwrap_or((0.0, 0.0, 0.0, 0.0));
    let st = r.style(i);
    let c = Ctx { vw: view.w, vh: view.h, font_size: crate::render::font_size(st, view) };
    let half = if st.get("stroke").eq_ignore_ascii_case("none") { 0.0 } else { units::length(st.get("stroke-width"), Axis::Diagonal, &c).unwrap_or(1.0) / 2.0 };
    let stroke = (fill.0 - half, fill.1 - half, fill.2 + half, fill.3 + half);
    let boxes = crate::shapes::RefBoxes::svg(fill, stroke, (0.0, 0.0, view.w, view.h), c.font_size);
    crate::shapes::basic_shape(v, &boxes, &m)
}

/// The rendered content of a `mask` reference and whether it masks by
/// luminance (`false`: by alpha).
pub fn mask(r: &mut Renderer, value: &str, _i: usize, m: Matrix, view: View, bbox: Option<BBox>) -> Option<(Context2D, bool)> {
    let v = value.trim();
    let mk = r.doc.lookup(v).filter(|c| r.doc.items[*c].el.name == "mask")?;
    let mut layer = Context2D::new(r.w, r.h, false)?;
    if !r.enter(mk) {
        return Some((layer, false));
    }
    let el = r.doc.items[mk].el;
    let object_units = el.attr("maskUnits") != Some("userSpaceOnUse");
    let bb = bbox.unwrap_or((0.0, 0.0, 0.0, 0.0));
    let (bw, bh) = (bb.2 - bb.0, bb.3 - bb.1);
    let c = Ctx { vw: view.w, vh: view.h, font_size: 16.0 };
    let get = |name: &str, default: &str, axis: Axis| -> f64 {
        let s = el.attr(name).unwrap_or(default);
        if object_units {
            let f = units::fraction(s).unwrap_or(0.0);
            match axis {
                Axis::X => f * bw,
                _ => f * bh,
            }
        } else {
            units::length(s, axis, &c).unwrap_or(0.0)
        }
    };
    let (mut x, mut y) = (get("x", "-10%", Axis::X), get("y", "-10%", Axis::Y));
    let (w, h) = (get("width", "120%", Axis::X), get("height", "120%", Axis::Y));
    if object_units {
        x += bb.0;
        y += bb.1;
        if bbox.is_none() || bw <= 0.0 || bh <= 0.0 {
            r.leave();
            return Some((layer, false));
        }
    }
    if w <= 0.0 || h <= 0.0 {
        r.leave();
        return Some((layer, false));
    }
    let mut clip = Path::new();
    clip.rect(&m, x, y, w, h);
    layer.clip(Some(&clip), FillRule::NonZero);
    let mut cm = m;
    if el.attr("maskContentUnits") == Some("objectBoundingBox") {
        cm = cm.multiply(&Matrix::new(bw, 0.0, 0.0, bh, bb.0, bb.1));
    }
    let luminance = !r.style(mk).get("mask-type").eq_ignore_ascii_case("alpha");
    r.draw_children(&mut layer, mk, cm, view);
    r.leave();
    Some((layer, luminance))
}
