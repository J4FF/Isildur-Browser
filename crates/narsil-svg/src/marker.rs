//! Markers (SVG 2 §11.6): vertices and directions of a path, `orient`,
//! `markerUnits`, `refX`/`refY`, the marker viewport and its clip.

use crate::render::{Renderer, Seg, View};
use crate::transform;
use crate::units::{self, Axis};
use narsil_canvas::{Context2D, FillRule, Matrix, Path, Style};

/// A vertex with the directions of the segments entering and leaving it.
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    pub dir_in: Option<(f64, f64)>,
    pub dir_out: Option<(f64, f64)>,
}

fn nonzero(v: (f64, f64)) -> Option<(f64, f64)> {
    (v.0.abs() > 1e-12 || v.1.abs() > 1e-12).then_some(v)
}

fn first_nonzero(list: &[(f64, f64)]) -> Option<(f64, f64)> {
    list.iter().copied().find_map(nonzero)
}

/// The marker vertices of a segment list.
#[must_use]
pub fn vertices(segs: &[Seg]) -> Vec<Vertex> {
    let mut out: Vec<Vertex> = Vec::new();
    let mut start = 0usize;
    let (mut cx, mut cy) = (0.0, 0.0);
    let push = |out: &mut Vec<Vertex>, x: f64, y: f64, dir: Option<(f64, f64)>| {
        out.push(Vertex { x, y, dir_in: dir, dir_out: None });
    };
    for s in segs {
        match *s {
            Seg::Move(x, y) => {
                start = out.len();
                push(&mut out, x, y, None);
                (cx, cy) = (x, y);
            }
            Seg::Line(x, y) => {
                let d = nonzero((x - cx, y - cy));
                if let Some(last) = out.last_mut() {
                    last.dir_out = d;
                }
                push(&mut out, x, y, d);
                (cx, cy) = (x, y);
            }
            Seg::Quad(a, b, x, y) => {
                let d0 = first_nonzero(&[(a - cx, b - cy), (x - cx, y - cy)]);
                let d1 = first_nonzero(&[(x - a, y - b), (x - cx, y - cy)]);
                if let Some(last) = out.last_mut() {
                    last.dir_out = d0;
                }
                push(&mut out, x, y, d1);
                (cx, cy) = (x, y);
            }
            Seg::Cubic(a, b, c, d, x, y) => {
                let d0 = first_nonzero(&[(a - cx, b - cy), (c - cx, d - cy), (x - cx, y - cy)]);
                let d1 = first_nonzero(&[(x - c, y - d), (x - a, y - b), (x - cx, y - cy)]);
                if let Some(last) = out.last_mut() {
                    last.dir_out = d0;
                }
                push(&mut out, x, y, d1);
                (cx, cy) = (x, y);
            }
            Seg::Arc(ecx, ecy, rx, ry, rot, a0, a1, ccw) => {
                let (sr, cr) = rot.sin_cos();
                let point = |t: f64| {
                    let (ex, ey) = (rx * t.cos(), ry * t.sin());
                    (ecx + ex * cr - ey * sr, ecy + ex * sr + ey * cr)
                };
                let sign = if ccw { -1.0 } else { 1.0 };
                let tangent = |t: f64| {
                    let (ex, ey) = (-rx * t.sin() * sign, ry * t.cos() * sign);
                    (ex * cr - ey * sr, ex * sr + ey * cr)
                };
                let (sx, sy) = point(a0);
                if out.is_empty() {
                    start = 0;
                    push(&mut out, sx, sy, None);
                } else if (sx - cx).abs() > 1e-9 || (sy - cy).abs() > 1e-9 {
                    let d = nonzero((sx - cx, sy - cy));
                    if let Some(last) = out.last_mut() {
                        last.dir_out = d;
                    }
                    push(&mut out, sx, sy, d);
                }
                if let Some(last) = out.last_mut() {
                    last.dir_out = nonzero(tangent(a0));
                }
                let (ex, ey) = point(a1);
                push(&mut out, ex, ey, nonzero(tangent(a1)));
                (cx, cy) = (ex, ey);
            }
            Seg::Close => {
                let Some(first) = out.get(start).copied() else { continue };
                let d = nonzero((first.x - cx, first.y - cy));
                if d.is_some() {
                    if let Some(last) = out.last_mut() {
                        last.dir_out = d;
                    }
                    push(&mut out, first.x, first.y, d);
                }
                // The closing vertex continues into the first segment, and
                // the start vertex is entered by the closing segment.
                let end = out.len() - 1;
                let first_out = out[start].dir_out;
                let closing_in = out[end].dir_in;
                out[end].dir_out = first_out;
                out[start].dir_in = closing_in;
                (cx, cy) = (first.x, first.y);
            }
        }
    }
    // Zero-length segments take the directions of their neighbours.
    for v in &mut out {
        if v.dir_in.is_none() {
            v.dir_in = v.dir_out;
        }
        if v.dir_out.is_none() {
            v.dir_out = v.dir_in;
        }
    }
    out
}

/// The bisector angle (radians) at a vertex.
fn auto_angle(v: &Vertex) -> f64 {
    match (v.dir_in, v.dir_out) {
        (Some(a), Some(b)) => {
            let ai = a.1.atan2(a.0);
            let ao = b.1.atan2(b.0);
            let mut d = ao - ai;
            while d > std::f64::consts::PI {
                d -= 2.0 * std::f64::consts::PI;
            }
            while d <= -std::f64::consts::PI {
                d += 2.0 * std::f64::consts::PI;
            }
            ai + d / 2.0
        }
        (Some(a), None) | (None, Some(a)) => a.1.atan2(a.0),
        (None, None) => 0.0,
    }
}

/// Draws the markers of shape `i`.
#[allow(clippy::too_many_arguments)]
pub fn draw_markers(r: &mut Renderer, ctx: &mut Context2D, i: usize, segs: &[Seg], m: Matrix, view: View, fill: Option<Style>, stroke: Option<Style>) {
    let st = r.style(i).clone();
    let props = [st.get("marker-start").to_string(), st.get("marker-mid").to_string(), st.get("marker-end").to_string()];
    if props.iter().all(|p| p.eq_ignore_ascii_case("none")) {
        return;
    }
    let verts = vertices(segs);
    if verts.is_empty() {
        return;
    }
    let c = units::Ctx { vw: view.w, vh: view.h, font_size: crate::render::font_size(&st, view) };
    let mut stroke_width = units::length(st.get("stroke-width"), Axis::Diagonal, &c).unwrap_or(1.0);
    // A non-scaling stroke has its width on the screen: in user units the
    // markers shrink by the transform's scale.
    if st.get("vector-effect").eq_ignore_ascii_case("non-scaling-stroke") {
        let scale = m.determinant().abs().sqrt();
        if scale > 0.0 {
            stroke_width /= scale;
        }
    }
    let last = verts.len() - 1;
    for (k, v) in verts.iter().enumerate() {
        let which = if k == 0 {
            0
        } else if k == last {
            2
        } else {
            1
        };
        let mut draw = |slot: usize| {
            if let Some(mk) = r.doc.lookup(&props[slot]).filter(|mk| r.doc.items[*mk].el.name == "marker") {
                draw_marker(r, ctx, mk, v, slot == 0, stroke_width, m, view, fill.clone(), stroke.clone());
            }
        };
        draw(which);
        // A path with one vertex gets start and end markers on it.
        if last == 0 {
            draw(2);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_marker(r: &mut Renderer, ctx: &mut Context2D, mk: usize, v: &Vertex, is_start: bool, stroke_width: f64, m: Matrix, view: View, fill: Option<Style>, stroke: Option<Style>) {
    if !r.enter(mk) {
        return;
    }
    let el = r.doc.items[mk].el;
    let c = units::Ctx { vw: view.w, vh: view.h, font_size: 16.0 };
    let mw = el.attr("markerWidth").and_then(|s| units::length(s, Axis::X, &c)).unwrap_or(3.0);
    let mh = el.attr("markerHeight").and_then(|s| units::length(s, Axis::Y, &c)).unwrap_or(3.0);
    if mw <= 0.0 || mh <= 0.0 {
        r.leave();
        return;
    }
    if transform::view_box_disables(el.attr("viewBox")) {
        r.leave();
        return;
    }
    let orient = el.attr("orient").unwrap_or("0").trim().to_string();
    let angle = match orient.as_str() {
        "auto" => auto_angle(v),
        "auto-start-reverse" => auto_angle(v) + if is_start { std::f64::consts::PI } else { 0.0 },
        other => units::angle(other).unwrap_or(0.0).to_radians(),
    };
    let scale = if el.attr("markerUnits") == Some("userSpaceOnUse") { 1.0 } else { stroke_width };
    let vb_view;
    let vb_m = match el.attr("viewBox").and_then(transform::view_box) {
        Some(vb) => {
            vb_view = View { w: vb[2], h: vb[3] };
            transform::view_box_transform(vb, el.attr("preserveAspectRatio").unwrap_or("xMidYMid meet"), 0.0, 0.0, mw, mh)
        }
        None => {
            vb_view = View { w: mw, h: mh };
            Matrix::IDENTITY
        }
    };
    let ref_len = |name: &str, axis: Axis, w: f64| -> f64 {
        match el.attr(name).map(str::trim) {
            Some("left") | Some("top") => 0.0,
            Some("center") => w / 2.0,
            Some("right") | Some("bottom") => w,
            Some(s) => units::length(s, axis, &units::Ctx { vw: vb_view.w, vh: vb_view.h, font_size: 16.0 }).unwrap_or(0.0),
            None => 0.0,
        }
    };
    let (rx, ry) = (ref_len("refX", Axis::X, vb_view.w), ref_len("refY", Axis::Y, vb_view.h));
    let (rmx, rmy) = vb_m.apply(rx, ry);
    let base = m.multiply(&Matrix::translate(v.x, v.y)).multiply(&Matrix::rotate(angle)).multiply(&Matrix::scale(scale, scale)).multiply(&Matrix::translate(-rmx, -rmy));
    let clip = crate::render::overflow_clips(&r.style(mk));
    ctx.save();
    if clip {
        let (x0, y0, w, h) = match el.attr("viewBox").and_then(transform::view_box) {
            // Clip to the viewport (the viewBox may not fill it under `meet`).
            Some(_) => (0.0, 0.0, mw, mh),
            None => (0.0, 0.0, mw, mh),
        };
        let mut p = Path::new();
        p.rect(&base, x0, y0, w, h);
        ctx.clip(Some(&p), FillRule::NonZero);
    }
    r.push_context_paint(fill, stroke, m);
    r.draw_children(ctx, mk, base.multiply(&vb_m), vb_view);
    r.pop_context_paint();
    let _ = ctx.restore();
    r.leave();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertices_of_a_closed_triangle() {
        let segs = [Seg::Move(0.0, 0.0), Seg::Line(10.0, 0.0), Seg::Line(10.0, 10.0), Seg::Close];
        let v = vertices(&segs);
        assert_eq!(v.len(), 4);
        // The start vertex is entered by the closing segment (pointing up-left).
        let d = v[0].dir_in.unwrap();
        assert!(d.0 < 0.0 && d.1 < 0.0);
        assert!((auto_angle(&v[1]) - std::f64::consts::FRAC_PI_4).abs() < 1e-9);
    }
}
