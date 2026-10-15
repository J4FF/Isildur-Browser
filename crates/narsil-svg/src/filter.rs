//! The `filter` property: CSS filter functions and `<filter>` elements
//! with their primitive graph (Filter Effects Module Level 1), computed in
//! device pixels over the filter region.

use crate::color;
use crate::render::{BBox, Renderer, View};
use crate::transform;
use crate::units::{self, Axis, Ctx};
use narsil_canvas::filter::{self as cf, Convolution, EdgeMode, FilterOp, TransferFn, Turbulence};
use narsil_canvas::{Color, Context2D, IRect, Matrix};
use std::collections::HashMap;
use std::sync::Arc;

/// What applying a filter did.
pub enum Outcome {
    Applied,
    /// The element is not rendered (an empty filter region).
    Hide,
    /// The filter is invalid and does not apply.
    Ignore,
}

/// Primitives one filter may run.
const MAX_PRIMITIVES: usize = 200;
/// Largest morphology radius in device pixels.
const MAX_RADIUS: i64 = 256;

/// A filter function or reference of the `filter` property.
enum Item {
    /// A filter function; `true` when it is tainted (a `drop-shadow()` in
    /// `currentcolor`, Filter Effects 1 §tainted filter primitives).
    Css(FilterOp, bool),
    Url(String),
}

fn split_functions(v: &str) -> Option<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut rest = v.trim();
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = rest[..open].trim().to_ascii_lowercase();
        let mut depth = 0;
        let mut close = None;
        for (k, c) in rest[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + k);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close?;
        out.push((name, rest[open + 1..close].trim().to_string()));
        rest = rest[close + 1..].trim_start();
    }
    Some(out)
}

fn amount(arg: &str) -> Option<f32> {
    if arg.is_empty() {
        return Some(1.0);
    }
    let v = units::fraction(arg)?;
    (v >= 0.0).then_some(v as f32)
}

/// Words at parenthesis depth 0.
fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Parses the `filter` value; `None` when invalid.
fn parse(v: &str, current: Color, scale: (f64, f64), c: &Ctx) -> Option<Vec<Item>> {
    let mut items = Vec::new();
    let avg = ((scale.0 * scale.0 + scale.1 * scale.1) / 2.0).sqrt();
    for (name, arg) in split_functions(v)? {
        let op = match name.as_str() {
            "url" => {
                items.push(Item::Url(format!("url({arg})")));
                continue;
            }
            "blur" => {
                let r = if arg.is_empty() { 0.0 } else { units::length(&arg, Axis::Diagonal, c)? };
                if r < 0.0 {
                    return None;
                }
                FilterOp::Blur(r * scale.0, r * scale.1)
            }
            "brightness" => FilterOp::Linear { slope: amount(&arg)?, intercept: 0.0 },
            "contrast" => {
                let a = amount(&arg)?;
                FilterOp::Linear { slope: a, intercept: 0.5 - 0.5 * a }
            }
            "grayscale" => FilterOp::ColorMatrix(cf::grayscale(amount(&arg)?.min(1.0))),
            "sepia" => FilterOp::ColorMatrix(cf::sepia(amount(&arg)?.min(1.0))),
            "saturate" => FilterOp::ColorMatrix(cf::saturate(amount(&arg)?)),
            "invert" => {
                let a = amount(&arg)?.min(1.0);
                FilterOp::Linear { slope: 1.0 - 2.0 * a, intercept: a }
            }
            "opacity" => FilterOp::Opacity(amount(&arg)?.min(1.0)),
            "hue-rotate" => FilterOp::ColorMatrix(cf::hue_rotate(if arg.is_empty() { 0.0 } else { units::angle(&arg)? })),
            "drop-shadow" => {
                let mut lengths = Vec::new();
                let mut col = None;
                for w in words(&arg) {
                    if let Some(l) = units::length(&w, Axis::Diagonal, c) {
                        lengths.push(l);
                    } else if col.is_none() {
                        col = Some(color::parse(&w, current)?);
                    } else {
                        return None;
                    }
                }
                if !(2..=3).contains(&lengths.len()) || lengths.get(2).is_some_and(|b| *b < 0.0) {
                    return None;
                }
                let sigma = lengths.get(2).copied().unwrap_or(0.0) / 2.0 * avg;
                let col = col.unwrap_or(current);
                FilterOp::DropShadow { dx: lengths[0] * scale.0, dy: lengths[1] * scale.1, sigma, sigma_y: sigma, color: premultiplied(col) }
            }
            _ => return None,
        };
        let tainted = name == "drop-shadow" && (arg.to_ascii_lowercase().contains("currentcolor") || !words(&arg).iter().any(|w| units::length(w, Axis::Diagonal, c).is_none()));
        items.push(Item::Css(op, tainted));
    }
    Some(items)
}

fn premultiplied(c: Color) -> [f32; 4] {
    let [r, g, b, a] = c.to_rgba8();
    let a = f32::from(a) / 255.0;
    [f32::from(r) / 255.0 * a, f32::from(g) / 255.0 * a, f32::from(b) / 255.0 * a, a]
}

fn device_scale(m: &Matrix) -> (f64, f64) {
    let [a, b, c, d, _, _] = m.as_array();
    (a.hypot(b).max(1e-9), c.hypot(d).max(1e-9))
}

/// Applies the `filter` value to the element's offscreen `layer`.
pub fn apply(r: &mut Renderer, layer: &mut Context2D, value: &str, i: usize, m: Matrix, view: View, bbox: Option<BBox>) -> Outcome {
    let st = r.style(i).clone();
    let current = color::parse(st.get("color"), Color::BLACK).unwrap_or(Color::BLACK);
    let c = Ctx { vw: view.w, vh: view.h, font_size: crate::render::font_size(&st, view) };
    let Some(items) = parse(value, current, device_scale(&m), &c) else { return Outcome::Ignore };
    // A reference to anything but a filter element voids the whole list.
    for it in &items {
        if let Item::Url(u) = it {
            if r.doc.lookup(u).filter(|f| r.doc.items[*f].el.name == "filter").is_none() {
                return Outcome::Ignore;
            }
        }
    }
    let (w, h) = (layer.surface.width as usize, layer.surface.height as usize);
    // The result of a tainted function is a tainted source for the rest.
    let mut tainted = false;
    for it in items {
        match it {
            Item::Css(op, t) => {
                tainted |= t;
                let mut buf = to_f32(&layer.surface.data, 0, 0, w, h, w);
                cf::Filter { ops: vec![op] }.apply(&mut buf, w, h);
                write_back(layer, &buf, IRect { x0: 0, y0: 0, x1: w as i32, y1: h as i32 });
            }
            Item::Url(u) => {
                let Some(f) = r.doc.lookup(&u) else { continue };
                if !r.enter(f) {
                    continue;
                }
                let ok = svg_filter(r, layer, f, i, m, view, bbox, tainted);
                r.leave();
                if !ok {
                    return Outcome::Hide;
                }
            }
        }
    }
    Outcome::Applied
}

fn to_f32(data: &[u8], x0: usize, y0: usize, w: usize, h: usize, stride: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h * 4];
    for y in 0..h {
        let src = &data[((y0 + y) * stride + x0) * 4..((y0 + y) * stride + x0 + w) * 4];
        for (o, s) in out[y * w * 4..(y + 1) * w * 4].iter_mut().zip(src) {
            *o = f32::from(*s) / 255.0;
        }
    }
    out
}

/// Replaces the layer with `buf` placed at `rect` (transparent elsewhere).
fn write_back(layer: &mut Context2D, buf: &[f32], rect: IRect) {
    let stride = layer.surface.width as usize;
    let data = Arc::make_mut(&mut layer.surface.data);
    data.iter_mut().for_each(|v| *v = 0);
    let w = rect.width().max(0) as usize;
    for y in 0..rect.height().max(0) as usize {
        let row = (rect.y0 as usize + y) * stride + rect.x0 as usize;
        for x in 0..w {
            let s = &buf[(y * w + x) * 4..(y * w + x) * 4 + 4];
            let a = s[3].clamp(0.0, 1.0);
            let d = &mut data[(row + x) * 4..(row + x) * 4 + 4];
            for k in 0..3 {
                d[k] = (s[k].clamp(0.0, a) * 255.0 + 0.5) as u8;
            }
            d[3] = (a * 255.0 + 0.5) as u8;
        }
    }
    layer.dirty = true;
}

// --------------------------------------------------------------- color space

fn lut_to_linear() -> &'static [f32; 4096] {
    static T: std::sync::OnceLock<Box<[f32; 4096]>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = Box::new([0.0f32; 4096]);
        for (k, v) in t.iter_mut().enumerate() {
            let c = k as f32 / 4095.0;
            *v = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        }
        t
    })
}

fn lut_to_srgb() -> &'static [f32; 4096] {
    static T: std::sync::OnceLock<Box<[f32; 4096]>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = Box::new([0.0f32; 4096]);
        for (k, v) in t.iter_mut().enumerate() {
            let c = k as f32 / 4095.0;
            *v = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        }
        t
    })
}

fn convert(buf: &mut [f32], to_linear: bool) {
    let t = if to_linear { lut_to_linear() } else { lut_to_srgb() };
    for px in buf.chunks_exact_mut(4) {
        let a = px[3];
        if a <= 0.0 {
            continue;
        }
        for v in &mut px[..3] {
            let c = (*v / a).clamp(0.0, 1.0);
            *v = t[(c * 4095.0 + 0.5) as usize] * a;
        }
    }
}

fn color_in(c: [f32; 4], linear: bool) -> [f32; 4] {
    if !linear || c[3] <= 0.0 {
        return c;
    }
    let mut px = c;
    convert(&mut px, true);
    px
}

// --------------------------------------------------------------- the graph

/// A primitive result: pixels over the filter region, its color space and
/// its subregion (region-local pixels, and in user space).
#[derive(Clone)]
struct Res {
    data: Vec<f32>,
    linear: bool,
    sub: IRect,
    user: (f64, f64, f64, f64),
    /// Derived from `currentcolor` or the fill/stroke paint (Filter
    /// Effects 1 §tainted filter primitives).
    tainted: bool,
}

struct Ctx2<'m> {
    /// Region-local device rect of the filter region (in layer pixels).
    reg: IRect,
    rw: usize,
    rh: usize,
    m: &'m Matrix,
    inv: Matrix,
    scale: (f64, f64),
    /// primitiveUnits="objectBoundingBox".
    obb: Option<BBox>,
    view: View,
    region_user: (f64, f64, f64, f64),
}

impl Ctx2<'_> {
    fn device_rect(&self, u: (f64, f64, f64, f64)) -> IRect {
        let (x, y, w, h) = u;
        let pts = [self.m.apply(x, y), self.m.apply(x + w, y), self.m.apply(x, y + h), self.m.apply(x + w, y + h)];
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (px, py) in pts {
            x0 = x0.min(px);
            y0 = y0.min(py);
            x1 = x1.max(px);
            y1 = y1.max(py);
        }
        // Edges snap to the nearest pixel (subregions are pixel aligned).
        let r = IRect { x0: (x0 - 1e-6).round() as i32 - self.reg.x0, y0: (y0 - 1e-6).round() as i32 - self.reg.y0, x1: (x1 + 1e-6).round() as i32 - self.reg.x0, y1: (y1 + 1e-6).round() as i32 - self.reg.y0 };
        r.intersect(&IRect { x0: 0, y0: 0, x1: self.rw as i32, y1: self.rh as i32 })
    }

    /// A primitive length (`stdDeviation`, `dx`, ...) in user units.
    fn user_len(&self, v: f64, axis: Axis) -> f64 {
        match (self.obb, axis) {
            (Some(b), Axis::X) => v * (b.2 - b.0),
            (Some(b), Axis::Y) => v * (b.3 - b.1),
            (Some(b), Axis::Diagonal) => v * (((b.2 - b.0).powi(2) + (b.3 - b.1).powi(2)) / 2.0).sqrt(),
            (None, _) => v,
        }
    }

    fn new_buf(&self) -> Vec<f32> {
        vec![0.0; self.rw * self.rh * 4]
    }
}

fn clip_to(data: &mut [f32], w: usize, sub: IRect) {
    for (k, px) in data.chunks_exact_mut(4).enumerate() {
        let (x, y) = ((k % w) as i32, (k / w) as i32);
        if x < sub.x0 || x >= sub.x1 || y < sub.y0 || y >= sub.y1 {
            px.iter_mut().for_each(|v| *v = 0.0);
        }
    }
}

fn attr_num(el: &crate::xml::Element, name: &str, default: f64) -> f64 {
    el.attr(name).and_then(units::number).unwrap_or(default)
}

fn attr_pair(el: &crate::xml::Element, name: &str, default: f64) -> Option<(f64, f64)> {
    match el.attr(name) {
        None => Some((default, default)),
        Some(s) => {
            let v = units::number_list(s);
            match v.as_slice() {
                [a] => Some((*a, *a)),
                [a, b] => Some((*a, *b)),
                _ => None,
            }
        }
    }
}

/// Runs a `<filter>` element over the layer; `false` when the element is
/// not rendered.
#[allow(clippy::too_many_lines)]
#[allow(clippy::too_many_arguments)]
fn svg_filter(r: &mut Renderer, layer: &mut Context2D, f: usize, i: usize, m: Matrix, view: View, bbox: Option<BBox>, source_tainted: bool) -> bool {
    // Attributes and primitives through the `href` chain (templates).
    let chain = filter_chain(r, f);
    let attr = |name: &str| chain.iter().find_map(|&k| r.doc.items[k].el.attr(name));
    let obb_units = attr("filterUnits") != Some("userSpaceOnUse");
    let bb = bbox.unwrap_or((0.0, 0.0, 0.0, 0.0));
    let (bw, bh) = (bb.2 - bb.0, bb.3 - bb.1);
    let c = Ctx { vw: view.w, vh: view.h, font_size: 16.0 };
    let region_val = |name: &str, default: &str, axis: Axis| -> f64 {
        let s = attr(name).unwrap_or(default);
        if obb_units {
            let v = units::fraction(s).or_else(|| units::length(s, axis, &c)).unwrap_or(0.0);
            match axis {
                Axis::X => bb.0 + v * bw,
                _ => bb.1 + v * bh,
            }
        } else {
            units::length(s, axis, &c).unwrap_or(0.0)
        }
    };
    let size_val = |name: &str, default: &str, axis: Axis| -> f64 {
        let s = attr(name).unwrap_or(default);
        if obb_units {
            let v = units::fraction(s).or_else(|| units::length(s, axis, &c)).unwrap_or(0.0);
            match axis {
                Axis::X => v * bw,
                _ => v * bh,
            }
        } else {
            units::length(s, axis, &c).unwrap_or(0.0)
        }
    };
    if obb_units && (bbox.is_none() || bw <= 0.0 || bh <= 0.0) {
        return false;
    }
    let fx = region_val("x", "-10%", Axis::X);
    let fy = region_val("y", "-10%", Axis::Y);
    let fw = size_val("width", "120%", Axis::X);
    let fh = size_val("height", "120%", Axis::Y);
    if fw <= 0.0 || fh <= 0.0 {
        return false;
    }
    let (w, h) = (layer.surface.width as i32, layer.surface.height as i32);
    let pts = [m.apply(fx, fy), m.apply(fx + fw, fy), m.apply(fx, fy + fh), m.apply(fx + fw, fy + fh)];
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (px, py) in pts {
        x0 = x0.min(px);
        y0 = y0.min(py);
        x1 = x1.max(px);
        y1 = y1.max(py);
    }
    let reg = IRect { x0: (x0 - 1e-6).round() as i32, y0: (y0 - 1e-6).round() as i32, x1: (x1 + 1e-6).round() as i32, y1: (y1 + 1e-6).round() as i32 }.intersect(&IRect { x0: 0, y0: 0, x1: w, y1: h });
    if reg.is_empty() {
        write_back(layer, &[], IRect { x0: 0, y0: 0, x1: 0, y1: 0 });
        return true;
    }
    let (rw, rh) = (reg.width() as usize, reg.height() as usize);
    let obb_prims = attr("primitiveUnits") == Some("objectBoundingBox");
    let cx = Ctx2 { reg, rw, rh, m: &m, inv: m.invert().unwrap_or(Matrix::IDENTITY), scale: device_scale(&m), obb: if obb_prims { Some(bb) } else { None }, view, region_user: (fx, fy, fw, fh) };
    let full = IRect { x0: 0, y0: 0, x1: rw as i32, y1: rh as i32 };
    let source = Res { data: to_f32(&layer.surface.data, reg.x0 as usize, reg.y0 as usize, rw, rh, w as usize), linear: false, sub: full, user: cx.region_user, tainted: source_tainted };
    let mut named: HashMap<String, Res> = HashMap::new();
    let mut last: Option<Res> = None;
    let st_el = r.style(i).clone();
    let element_color = color::parse(st_el.get("color"), Color::BLACK).unwrap_or(Color::BLACK);
    let with_primitives = chain.iter().copied().find(|&k| r.doc.items[k].children.iter().any(|&c| r.doc.items[c].el.name.starts_with("fe"))).unwrap_or(f);
    let kids: Vec<usize> = r.doc.items[with_primitives].children.clone();
    for (count, p) in kids.into_iter().enumerate() {
        if count >= MAX_PRIMITIVES {
            break;
        }
        let pel = r.doc.items[p].el;
        if !pel.name.starts_with("fe") {
            continue;
        }
        let pst = r.style(p).clone();
        let linear = !pst.get("color-interpolation-filters").eq_ignore_ascii_case("srgb");
        let input = |name: Option<&str>, named: &HashMap<String, Res>, last: &Option<Res>| resolve(name, named, last, &source, &cx);
        let in1 = input(pel.attr("in"), &named, &last);
        let in2 = input(pel.attr("in2"), &named, &last);
        let uses_in2 = matches!(pel.name.as_str(), "feComposite" | "feBlend" | "feDisplacementMap");
        let no_input = matches!(pel.name.as_str(), "feFlood" | "feTurbulence" | "feImage");
        // The default subregion: the union of the inputs' subregions, or
        // the filter region for primitives without inputs.
        let standard = |n: Option<&str>| matches!(n.map(str::trim), Some("SourceGraphic" | "SourceAlpha" | "BackgroundImage" | "BackgroundAlpha" | "FillPaint" | "StrokePaint"));
        let mut def_user = if no_input || (standard(pel.attr("in")) && (!uses_in2 || standard(pel.attr("in2")))) {
            cx.region_user
        } else if uses_in2 {
            union_user(in1.user, in2.user)
        } else {
            in1.user
        };
        if pel.name == "feMerge" {
            let mut u: Option<(f64, f64, f64, f64)> = None;
            for &k in &r.doc.items[p].children {
                if r.doc.items[k].el.name == "feMergeNode" {
                    let src = r.doc.items[k].el.attr("in");
                    let res = if standard(src) { cx.region_user } else { input(src, &named, &last).user };
                    u = Some(u.map_or(res, |a| union_user(a, res)));
                }
            }
            if let Some(u) = u {
                def_user = u;
            }
        }
        let pc = Ctx { vw: view.w, vh: view.h, font_size: 16.0 };
        let sub_val = |name: &str, axis: Axis, def: f64| -> f64 {
            match pel.attr(name) {
                None => def,
                Some(s) => match cx.obb {
                    Some(b) => {
                        let v = units::fraction(s).unwrap_or(0.0);
                        match (name, axis) {
                            ("x", _) => b.0 + v * (b.2 - b.0),
                            ("y", _) => b.1 + v * (b.3 - b.1),
                            (_, Axis::X) => v * (b.2 - b.0),
                            _ => v * (b.3 - b.1),
                        }
                    }
                    None => units::length(s, axis, &pc).unwrap_or(def),
                },
            }
        };
        let user = (sub_val("x", Axis::X, def_user.0), sub_val("y", Axis::Y, def_user.1), sub_val("width", Axis::X, def_user.2), sub_val("height", Axis::Y, def_user.3));
        if user.2 < 0.0 || user.3 < 0.0 {
            return false;
        }
        let sub = cx.device_rect(user).intersect(&cx.device_rect(cx.region_user));
        let mut out = match run_primitive(r, p, &cx, &in1, &in2, linear, sub, user, element_color, &named, &last, &source) {
            Some(o) => o,
            None => Res { data: cx.new_buf(), linear, sub, user, tainted: false },
        };
        out.sub = sub;
        out.user = user;
        // Taint flows from the inputs and from `currentcolor` paints.
        let own = match pel.name.as_str() {
            "feFlood" | "feDropShadow" => pst.get("flood-color").to_ascii_lowercase().contains("currentcolor"),
            "feDiffuseLighting" | "feSpecularLighting" => pst.get("lighting-color").to_ascii_lowercase().contains("currentcolor"),
            _ => false,
        };
        let merged = pel.name == "feMerge"
            && r.doc.items[p].children.iter().any(|&k| r.doc.items[k].el.name == "feMergeNode" && input(r.doc.items[k].el.attr("in"), &named, &last).tainted);
        out.tainted = out.tainted || own || merged || (!no_input && in1.tainted) || (uses_in2 && in2.tainted);
        clip_to(&mut out.data, rw, sub);
        if let Some(name) = pel.attr("result").map(str::trim).filter(|n| !n.is_empty()) {
            named.insert(name.to_string(), out.clone());
        }
        last = Some(out);
    }
    let mut result = last.unwrap_or(source);
    if result.linear {
        convert(&mut result.data, false);
    }
    write_back(layer, &result.data, reg);
    true
}

/// A primitive input: a standard keyword, a named result, or the previous
/// result (the source graphic for the first primitive).
fn resolve(name: Option<&str>, named: &HashMap<String, Res>, last: &Option<Res>, source: &Res, cx: &Ctx2) -> Res {
    match name.map(str::trim) {
        Some("SourceGraphic") => source.clone(),
        Some("SourceAlpha") => {
            let mut s = source.clone();
            for px in s.data.chunks_exact_mut(4) {
                px[0] = 0.0;
                px[1] = 0.0;
                px[2] = 0.0;
            }
            s
        }
        Some(n @ ("BackgroundImage" | "BackgroundAlpha" | "FillPaint" | "StrokePaint")) => Res { data: cx.new_buf(), linear: false, sub: source.sub, user: cx.region_user, tainted: n.ends_with("Paint") },
        Some(n) if !n.is_empty() => named.get(n).cloned().or_else(|| last.clone()).unwrap_or_else(|| source.clone()),
        _ => last.clone().unwrap_or_else(|| source.clone()),
    }
}

/// A filter element and the filters its `href` chain names.
fn filter_chain(r: &Renderer, f: usize) -> Vec<usize> {
    let mut out = vec![f];
    let mut cur = f;
    while out.len() < 16 {
        match r.doc.items[cur].el.attr("href").and_then(|h| r.doc.lookup(h)) {
            Some(n) if r.doc.items[n].el.name == "filter" && !out.contains(&n) => {
                out.push(n);
                cur = n;
            }
            _ => break,
        }
    }
    out
}

fn union_user(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    let x0 = a.0.min(b.0);
    let y0 = a.1.min(b.1);
    let x1 = (a.0 + a.2).max(b.0 + b.2);
    let y1 = (a.1 + a.3).max(b.1 + b.3);
    (x0, y0, x1 - x0, y1 - y0)
}

/// The input in the color space the primitive works in.
fn in_space(res: &Res, linear: bool) -> Vec<f32> {
    let mut d = res.data.clone();
    if res.linear != linear {
        convert(&mut d, linear);
    }
    d
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn run_primitive(r: &mut Renderer, p: usize, cx: &Ctx2, in1: &Res, in2: &Res, linear: bool, sub: IRect, user: (f64, f64, f64, f64), element_color: Color, named: &HashMap<String, Res>, last: &Option<Res>, source: &Res) -> Option<Res> {
    let el = r.doc.items[p].el;
    let st = r.style(p).clone();
    let (rw, rh) = (cx.rw, cx.rh);
    let res = |data: Vec<f32>, linear: bool| Some(Res { data, linear, sub, user, tainted: false });
    let current = color::parse(st.get("color"), element_color).unwrap_or(element_color);
    let flood = || -> [f32; 4] {
        let mut col = color::parse(st.get("flood-color"), current).unwrap_or(Color::BLACK);
        col.a *= units::fraction(st.get("flood-opacity")).unwrap_or(1.0).clamp(0.0, 1.0) as f32;
        color_in(premultiplied(col), linear)
    };
    match el.name.as_str() {
        "feFlood" => {
            let col = flood();
            let mut d = cx.new_buf();
            for px in d.chunks_exact_mut(4) {
                px.copy_from_slice(&col);
            }
            res(d, linear)
        }
        "feOffset" => {
            let dx = cx.user_len(attr_num(el, "dx", 0.0), Axis::X);
            let dy = cx.user_len(attr_num(el, "dy", 0.0), Axis::Y);
            let (ddx, ddy) = cx.m.apply_vector(dx, dy);
            let d = shift(&in1.data, rw, rh, ddx.round() as i64, ddy.round() as i64);
            res(d, in1.linear)
        }
        "feGaussianBlur" => {
            // Negative deviations count as zero.
            let (sx, sy) = attr_pair(el, "stdDeviation", 0.0)?;
            let (sx, sy) = (sx.max(0.0), sy.max(0.0));
            let mut d = in_space(in1, linear);
            let (dsx, dsy) = (cx.user_len(sx, Axis::X) * cx.scale.0, cx.user_len(sy, Axis::Y) * cx.scale.1);
            if dsx > 0.0 || dsy > 0.0 {
                cf::Filter { ops: vec![FilterOp::Blur(dsx, dsy)] }.apply(&mut d, rw, rh);
            }
            res(d, linear)
        }
        "feColorMatrix" => {
            let ty = el.attr("type").unwrap_or("matrix").trim();
            let vals = el.attr("values").map(units::number_list);
            let mat: [f32; 20] = match ty {
                "saturate" => cf::saturate(vals.as_ref().and_then(|v| v.first().copied()).unwrap_or(1.0) as f32),
                "hueRotate" => cf::hue_rotate(vals.as_ref().and_then(|v| v.first().copied()).unwrap_or(0.0)),
                "luminanceToAlpha" => cf::luminance_to_alpha(),
                _ => match vals {
                    Some(v) if v.len() == 20 => {
                        let mut a = [0.0f32; 20];
                        for (o, s) in a.iter_mut().zip(&v) {
                            *o = *s as f32;
                        }
                        a
                    }
                    _ => [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                },
            };
            let mut d = in_space(in1, linear);
            cf::Filter { ops: vec![FilterOp::ColorMatrix(mat)] }.apply(&mut d, rw, rh);
            res(d, linear)
        }
        "feComponentTransfer" => {
            let mut funcs = [TransferFn::Identity, TransferFn::Identity, TransferFn::Identity, TransferFn::Identity];
            for &k in &r.doc.items[p].children {
                let fe = r.doc.items[k].el;
                let slot = match fe.name.as_str() {
                    "feFuncR" => 0,
                    "feFuncG" => 1,
                    "feFuncB" => 2,
                    "feFuncA" => 3,
                    _ => continue,
                };
                let table: Vec<f32> = fe.attr("tableValues").map(units::number_list).unwrap_or_default().into_iter().map(|v| v as f32).collect();
                funcs[slot] = match fe.attr("type").unwrap_or("").trim() {
                    "table" if !table.is_empty() => TransferFn::Table(table),
                    "discrete" if !table.is_empty() => TransferFn::Discrete(table),
                    "linear" => TransferFn::Linear { slope: attr_num(fe, "slope", 1.0) as f32, intercept: attr_num(fe, "intercept", 0.0) as f32 },
                    "gamma" => TransferFn::Gamma { amplitude: attr_num(fe, "amplitude", 1.0) as f32, exponent: attr_num(fe, "exponent", 1.0) as f32, offset: attr_num(fe, "offset", 0.0) as f32 },
                    _ => TransferFn::Identity,
                };
            }
            let mut d = in_space(in1, linear);
            cf::Filter { ops: vec![FilterOp::Transfer(Box::new(funcs))] }.apply(&mut d, rw, rh);
            res(d, linear)
        }
        "feComposite" => {
            let a = in_space(in1, linear);
            let b = in_space(in2, linear);
            let op = el.attr("operator").unwrap_or("over").trim().to_string();
            let k = [attr_num(el, "k1", 0.0) as f32, attr_num(el, "k2", 0.0) as f32, attr_num(el, "k3", 0.0) as f32, attr_num(el, "k4", 0.0) as f32];
            let mut d = cx.new_buf();
            for ((o, s), t) in d.chunks_exact_mut(4).zip(a.chunks_exact(4)).zip(b.chunks_exact(4)) {
                let (sa, ba) = (s[3], t[3]);
                for j in 0..4 {
                    o[j] = match op.as_str() {
                        "in" => s[j] * ba,
                        "out" => s[j] * (1.0 - ba),
                        "atop" => s[j] * ba + t[j] * (1.0 - sa),
                        "xor" => s[j] * (1.0 - ba) + t[j] * (1.0 - sa),
                        "lighter" => (s[j] + t[j]).min(1.0),
                        "arithmetic" => k[0] * s[j] * t[j] + k[1] * s[j] + k[2] * t[j] + k[3],
                        _ => s[j] + t[j] * (1.0 - sa),
                    };
                }
                o[3] = o[3].clamp(0.0, 1.0);
                for j in 0..3 {
                    o[j] = o[j].clamp(0.0, o[3]);
                }
            }
            res(d, linear)
        }
        "feBlend" => {
            let a = in_space(in1, linear);
            let b = in_space(in2, linear);
            let mode = el.attr("mode").unwrap_or("normal").trim().to_ascii_lowercase();
            let mut d = cx.new_buf();
            for ((o, s), t) in d.chunks_exact_mut(4).zip(a.chunks_exact(4)).zip(b.chunks_exact(4)) {
                blend(&mode, s, t, o);
            }
            res(d, linear)
        }
        "feMerge" => {
            let mut d = cx.new_buf();
            for &k in &r.doc.items[p].children {
                let node = r.doc.items[k].el;
                if node.name != "feMergeNode" {
                    continue;
                }
                let src = resolve(node.attr("in"), named, last, source, cx);
                let mut s = in_space(&src, linear);
                clip_to(&mut s, rw, src.sub);
                for (o, v) in d.chunks_exact_mut(4).zip(s.chunks_exact(4)) {
                    let ia = 1.0 - v[3];
                    for j in 0..4 {
                        o[j] = v[j] + o[j] * ia;
                    }
                }
            }
            res(d, linear)
        }
        "feMorphology" => {
            let (rx, ry) = attr_pair(el, "radius", 0.0)?;
            let erode = el.attr("operator").map(str::trim) != Some("dilate");
            let d = in_space(in1, linear);
            // A negative or zero radius disables the primitive: the result
            // is its input.
            if rx <= 0.0 || ry <= 0.0 {
                return res(d, linear);
            }
            let drx = (cx.user_len(rx, Axis::X) * cx.scale.0).round().min(MAX_RADIUS as f64) as i64;
            let dry = (cx.user_len(ry, Axis::Y) * cx.scale.1).round().min(MAX_RADIUS as f64) as i64;
            if drx <= 0 && dry <= 0 {
                return res(d, linear);
            }
            res(morphology(&d, rw, rh, drx.max(0), dry.max(0), erode), linear)
        }
        "feTile" => {
            let src = in_space(in1, linear);
            let t = in1.sub;
            let (tw, th) = (t.width(), t.height());
            let mut d = cx.new_buf();
            if tw > 0 && th > 0 {
                for y in sub.y0.max(0)..sub.y1.min(rh as i32) {
                    let sy = t.y0 + (y - t.y0).rem_euclid(th);
                    for x in sub.x0.max(0)..sub.x1.min(rw as i32) {
                        let sx = t.x0 + (x - t.x0).rem_euclid(tw);
                        let (o, s) = (((y as usize) * rw + x as usize) * 4, ((sy as usize) * rw + sx as usize) * 4);
                        d[o..o + 4].copy_from_slice(&src[s..s + 4]);
                    }
                }
            }
            res(d, linear)
        }
        "feImage" => fe_image(r, p, cx, sub, user, linear),
        "feTurbulence" => {
            let (bfx, bfy) = attr_pair(el, "baseFrequency", 0.0)?;
            if bfx < 0.0 || bfy < 0.0 {
                return res(cx.new_buf(), linear);
            }
            let t = Turbulence {
                base_x: bfx,
                base_y: bfy,
                octaves: attr_num(el, "numOctaves", 1.0).max(0.0) as u32,
                seed: attr_num(el, "seed", 0.0),
                stitch: el.attr("stitchTiles").map(str::trim) == Some("stitch"),
                fractal: el.attr("type").map(str::trim) == Some("fractalNoise"),
            };
            let mut d = cx.new_buf();
            let (ox, oy) = (cx.reg.x0 as f64, cx.reg.y0 as f64);
            let inv = cx.inv;
            cf::turbulence_mapped(&mut d, rw, rh, &t, user, &|x, y| inv.apply(ox + x as f64, oy + y as f64));
            res(d, linear)
        }
        "feConvolveMatrix" => {
            let order = el.attr("order").map(units::number_list).unwrap_or_else(|| vec![3.0]);
            let (ox, oy) = match order.as_slice() {
                [a] => (*a, *a),
                [a, b] => (*a, *b),
                _ => return res(cx.new_buf(), linear),
            };
            if ox < 1.0 || oy < 1.0 || ox.fract() != 0.0 || oy.fract() != 0.0 || ox * oy > 4096.0 {
                return res(cx.new_buf(), linear);
            }
            let (ox, oy) = (ox as usize, oy as usize);
            let kernel: Vec<f32> = el.attr("kernelMatrix").map(units::number_list).unwrap_or_default().into_iter().map(|v| v as f32).collect();
            if kernel.len() != ox * oy {
                return res(cx.new_buf(), linear);
            }
            let sum: f32 = kernel.iter().sum();
            let divisor = el.attr("divisor").and_then(units::number).map(|v| v as f32).filter(|v| *v != 0.0).unwrap_or(if sum == 0.0 { 1.0 } else { sum });
            let tx = el.attr("targetX").and_then(units::number).map_or((ox / 2) as i64, |v| v as i64);
            let ty = el.attr("targetY").and_then(units::number).map_or((oy / 2) as i64, |v| v as i64);
            if tx < 0 || ty < 0 || tx >= ox as i64 || ty >= oy as i64 {
                return res(cx.new_buf(), linear);
            }
            let edge = match el.attr("edgeMode").map(str::trim) {
                Some("wrap") => EdgeMode::Wrap,
                Some("none") => EdgeMode::None,
                _ => EdgeMode::Duplicate,
            };
            let conv = Convolution { order_x: ox, order_y: oy, kernel, divisor, bias: attr_num(el, "bias", 0.0) as f32, target_x: tx, target_y: ty, edge, preserve_alpha: el.attr("preserveAlpha").map(str::trim) == Some("true") };
            // The convolution sees only the input's subregion.
            let mut d = in_space(in1, linear);
            let s = in1.sub;
            let (sw, sh) = (s.width().max(0) as usize, s.height().max(0) as usize);
            if sw > 0 && sh > 0 {
                let mut part = vec![0.0f32; sw * sh * 4];
                for y in 0..sh {
                    let src = ((s.y0 as usize + y) * rw + s.x0 as usize) * 4;
                    part[y * sw * 4..(y + 1) * sw * 4].copy_from_slice(&d[src..src + sw * 4]);
                }
                cf::convolve(&mut part, sw, sh, &conv);
                for y in 0..sh {
                    let dst = ((s.y0 as usize + y) * rw + s.x0 as usize) * 4;
                    d[dst..dst + sw * 4].copy_from_slice(&part[y * sw * 4..(y + 1) * sw * 4]);
                }
            }
            res(d, linear)
        }
        "feDisplacementMap" => {
            let scale = cx.user_len(attr_num(el, "scale", 0.0), Axis::Diagonal);
            let (sx, sy) = (scale * cx.scale.0, scale * cx.scale.1);
            let pick = |name: &str| match el.attr(name).map(str::trim) {
                Some("R") => 0,
                Some("G") => 1,
                Some("B") => 2,
                _ => 3,
            };
            let (xc, yc) = (pick("xChannelSelector"), pick("yChannelSelector"));
            // A tainted displacement map is not used (Filter Effects 1
            // §feDisplacementMap restrictions).
            if in2.tainted {
                return Some(Res { data: in1.data.clone(), linear: in1.linear, sub, user, tainted: in1.tainted });
            }
            let src = in1.data.clone();
            let map = in_space(in2, linear);
            let mut d = cx.new_buf();
            for y in 0..rh {
                for x in 0..rw {
                    let o = (y * rw + x) * 4;
                    let mp = &map[o..o + 4];
                    let a = mp[3];
                    let ch = |c: usize| if c == 3 { a } else if a > 0.0 { mp[c] / a } else { 0.0 };
                    let px = x as f64 + sx * (f64::from(ch(xc)) - 0.5);
                    let py = y as f64 + sy * (f64::from(ch(yc)) - 0.5);
                    let (ix, iy) = (px.floor() as i64, py.floor() as i64);
                    if ix >= 0 && iy >= 0 && (ix as usize) < rw && (iy as usize) < rh {
                        let s = ((iy as usize) * rw + ix as usize) * 4;
                        d[o..o + 4].copy_from_slice(&src[s..s + 4]);
                    }
                }
            }
            res(d, in1.linear)
        }
        "feDropShadow" => {
            let (sdx, sdy) = attr_pair(el, "stdDeviation", 2.0)?;
            let (sdx, sdy) = (sdx.max(0.0), sdy.max(0.0));
            let dx = cx.user_len(attr_num(el, "dx", 2.0), Axis::X);
            let dy = cx.user_len(attr_num(el, "dy", 2.0), Axis::Y);
            let (ddx, ddy) = cx.m.apply_vector(dx, dy);
            let src = in_space(in1, linear);
            let col = flood();
            let mut shadow = cx.new_buf();
            for (o, s) in shadow.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                for j in 0..4 {
                    o[j] = col[j] * s[3];
                }
            }
            let (bx, by) = (cx.user_len(sdx, Axis::X) * cx.scale.0, cx.user_len(sdy, Axis::Y) * cx.scale.1);
            if bx > 0.0 || by > 0.0 {
                cf::Filter { ops: vec![FilterOp::Blur(bx, by)] }.apply(&mut shadow, rw, rh);
            }
            let mut d = shift(&shadow, rw, rh, ddx.round() as i64, ddy.round() as i64);
            for (o, s) in d.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                let ia = 1.0 - s[3];
                for j in 0..4 {
                    o[j] = s[j] + o[j] * ia;
                }
            }
            res(d, linear)
        }
        "feDiffuseLighting" | "feSpecularLighting" => lighting(r, p, cx, in1, linear, current, sub, user),
        _ => None,
    }
}

fn shift(src: &[f32], w: usize, h: usize, dx: i64, dy: i64) -> Vec<f32> {
    let mut d = vec![0.0f32; src.len()];
    for y in 0..h as i64 {
        let sy = y - dy;
        if sy < 0 || sy >= h as i64 {
            continue;
        }
        for x in 0..w as i64 {
            let sx = x - dx;
            if sx < 0 || sx >= w as i64 {
                continue;
            }
            let (o, s) = (((y as usize) * w + x as usize) * 4, ((sy as usize) * w + sx as usize) * 4);
            d[o..o + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    d
}

fn morphology(src: &[f32], w: usize, h: usize, rx: i64, ry: i64, erode: bool) -> Vec<f32> {
    let pick = |a: f32, b: f32| if erode { a.min(b) } else { a.max(b) };
    let init = if erode { f32::INFINITY } else { f32::NEG_INFINITY };
    let mut tmp = vec![0.0f32; src.len()];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let mut v = init;
                for k in -rx..=rx {
                    let sx = x as i64 + k;
                    // Outside the image counts as transparent black.
                    let s = if sx < 0 || sx >= w as i64 { 0.0 } else { src[(y * w + sx as usize) * 4 + c] };
                    v = pick(v, s);
                }
                tmp[(y * w + x) * 4 + c] = v;
            }
        }
    }
    let mut out = vec![0.0f32; src.len()];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let mut v = init;
                for k in -ry..=ry {
                    let sy = y as i64 + k;
                    let s = if sy < 0 || sy >= h as i64 { 0.0 } else { tmp[(sy as usize * w + x) * 4 + c] };
                    v = pick(v, s);
                }
                out[(y * w + x) * 4 + c] = v;
            }
        }
    }
    out
}

/// Separable and non-separable blend modes on premultiplied colors
/// (Compositing and Blending Level 1); `s` is the top (`in`), `b` the
/// backdrop (`in2`).
fn blend(mode: &str, s: &[f32], b: &[f32], o: &mut [f32]) {
    let (sa, ba) = (s[3], b[3]);
    let un = |p: &[f32], a: f32| if a > 0.0 { [p[0] / a, p[1] / a, p[2] / a] } else { [0.0; 3] };
    let (cs, cb) = (un(s, sa), un(b, ba));
    let sep = |f: &dyn Fn(f32, f32) -> f32| [f(cb[0], cs[0]), f(cb[1], cs[1]), f(cb[2], cs[2])];
    let mixed: [f32; 3] = match mode {
        "multiply" => sep(&|b, s| b * s),
        "screen" => sep(&|b, s| b + s - b * s),
        "darken" => sep(&|b, s| b.min(s)),
        "lighten" => sep(&|b, s| b.max(s)),
        "overlay" => sep(&|b, s| hard_light(s, b)),
        "color-dodge" => sep(&|b, s| if b == 0.0 { 0.0 } else if s >= 1.0 { 1.0 } else { (b / (1.0 - s)).min(1.0) }),
        "color-burn" => sep(&|b, s| if b >= 1.0 { 1.0 } else if s <= 0.0 { 0.0 } else { 1.0 - ((1.0 - b) / s).min(1.0) }),
        "hard-light" => sep(&|b, s| hard_light(b, s)),
        "soft-light" => sep(&|b, s| {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }),
        "difference" => sep(&|b, s| (b - s).abs()),
        "exclusion" => sep(&|b, s| b + s - 2.0 * b * s),
        "hue" => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        "saturation" => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        "color" => set_lum(cs, lum(cb)),
        "luminosity" => set_lum(cb, lum(cs)),
        _ => cs,
    };
    let a = sa + ba - sa * ba;
    for j in 0..3 {
        o[j] = ((1.0 - ba) * s[j] + (1.0 - sa) * b[j] + sa * ba * mixed[j]).clamp(0.0, a);
    }
    o[3] = a.clamp(0.0, 1.0);
}

fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 { b * 2.0 * s } else { let t = 2.0 * s - 1.0; b + t - b * t }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut o = c;
    if n < 0.0 {
        for v in &mut o {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 {
        for v in &mut o {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    o
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|a, b| c[*a].partial_cmp(&c[*b]).unwrap_or(std::cmp::Ordering::Equal));
    let (mn, md, mx) = (idx[0], idx[1], idx[2]);
    let mut o = [0.0f32; 3];
    if c[mx] > c[mn] {
        o[md] = (c[md] - c[mn]) * s / (c[mx] - c[mn]);
        o[mx] = s;
    }
    o[mn] = 0.0;
    o
}

fn fe_image(r: &mut Renderer, p: usize, cx: &Ctx2, sub: IRect, user: (f64, f64, f64, f64), linear: bool) -> Option<Res> {
    let el = r.doc.items[p].el;
    let href = el.attr("href")?.trim().to_string();
    let (w, h) = (r.w, r.h);
    let mut layer = Context2D::new(w, h, false)?;
    if href.starts_with('#') {
        let target = r.doc.lookup(&href)?;
        if !r.enter(target) {
            return None;
        }
        r.draw(&mut layer, target, *cx.m, cx.view);
        r.leave();
    } else {
        let img = crate::image::decode_href(&href)?;
        let (iw, ih) = (f64::from(img.width), f64::from(img.height));
        let par = el.attr("preserveAspectRatio").unwrap_or("xMidYMid meet");
        let fit = transform::view_box_transform([0.0, 0.0, iw, ih], par, user.0, user.1, user.2, user.3);
        let mut clip = narsil_canvas::Path::new();
        clip.rect(cx.m, user.0, user.1, user.2, user.3);
        layer.clip(Some(&clip), narsil_canvas::FillRule::NonZero);
        layer.state.transform = cx.m.multiply(&fit);
        layer.draw_image(&img, 0.0, 0.0, iw, ih, 0.0, 0.0, iw, ih);
    }
    let mut d = to_f32(&layer.surface.data, cx.reg.x0 as usize, cx.reg.y0 as usize, cx.rw, cx.rh, w as usize);
    if linear {
        convert(&mut d, true);
    }
    let _ = h;
    Some(Res { data: d, linear, sub, user, tainted: false })
}

#[allow(clippy::too_many_arguments)]
fn lighting(r: &Renderer, p: usize, cx: &Ctx2, in1: &Res, linear: bool, current: Color, sub: IRect, user: (f64, f64, f64, f64)) -> Option<Res> {
    let el = r.doc.items[p].el;
    let st = &r.style(p);
    let specular = el.name == "feSpecularLighting";
    let surface_scale = attr_num(el, "surfaceScale", 1.0) as f32;
    let kd = attr_num(el, "diffuseConstant", 1.0) as f32;
    let ks = attr_num(el, "specularConstant", 1.0) as f32;
    let exp = attr_num(el, "specularExponent", 1.0) as f32;
    if (!specular && kd < 0.0) || (specular && !(1.0..=128.0).contains(&exp)) {
        return Some(Res { data: cx.new_buf(), linear, sub, user, tainted: false });
    }
    let lc = color::parse(st.get("lighting-color"), current).unwrap_or(Color::rgba(1.0, 1.0, 1.0, 1.0));
    let lcp = color_in(premultiplied(Color { a: 1.0, ..lc }), linear);
    let light = r.doc.items[p].children.iter().copied().find(|k| matches!(r.doc.items[*k].el.name.as_str(), "feDistantLight" | "fePointLight" | "feSpotLight"));
    let Some(light) = light else { return Some(Res { data: cx.new_buf(), linear, sub, user, tainted: false }) };
    let lel = r.doc.items[light].el;
    let avg_scale = ((cx.scale.0 + cx.scale.1) / 2.0) as f32;
    enum Light {
        Distant([f32; 3]),
        Point([f32; 3]),
        Spot { pos: [f32; 3], dir: [f32; 3], exp: f32, cos_cone: Option<f32> },
    }
    let dev_point = |x: f64, y: f64, z: f64| -> [f32; 3] {
        let (ux, uy) = (cx.user_len(x, Axis::X), cx.user_len(y, Axis::Y));
        let (dx, dy) = cx.m.apply(ux, uy);
        [(dx - f64::from(cx.reg.x0)) as f32, (dy - f64::from(cx.reg.y0)) as f32, (cx.user_len(z, Axis::Diagonal) as f32) * avg_scale]
    };
    let num = |n: &str| attr_num(lel, n, 0.0);
    let src = match lel.name.as_str() {
        "feDistantLight" => {
            let (az, elv) = (num("azimuth").to_radians(), num("elevation").to_radians());
            Light::Distant([(az.cos() * elv.cos()) as f32, (az.sin() * elv.cos()) as f32, elv.sin() as f32])
        }
        "fePointLight" => Light::Point(dev_point(num("x"), num("y"), num("z"))),
        _ => {
            let pos = dev_point(num("x"), num("y"), num("z"));
            let at = dev_point(num("pointsAtX"), num("pointsAtY"), num("pointsAtZ"));
            let d = [at[0] - pos[0], at[1] - pos[1], at[2] - pos[2]];
            let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-9);
            let cone = lel.attr("limitingConeAngle").and_then(units::number).map(|a| (a.abs().to_radians().cos()) as f32);
            Light::Spot { pos, dir: [d[0] / n, d[1] / n, d[2] / n], exp: attr_num(lel, "specularExponent", 1.0) as f32, cos_cone: cone }
        }
    };
    let (w, h) = (cx.rw, cx.rh);
    let alpha = |x: i64, y: i64| -> f32 {
        let s = in1.sub;
        let x = x.clamp(i64::from(s.x0.max(0)), i64::from((s.x1 - 1).max(0)));
        let y = y.clamp(i64::from(s.y0.max(0)), i64::from((s.y1 - 1).max(0)));
        in1.data[(y as usize * w + x as usize) * 4 + 3]
    };
    let mut d = vec![0.0f32; w * h * 4];
    for y in sub.y0.max(0)..sub.y1.min(h as i32) {
        for x in sub.x0.max(0)..sub.x1.min(w as i32) {
            let (xi, yi) = (i64::from(x), i64::from(y));
            let a = |dx: i64, dy: i64| alpha(xi + dx, yi + dy);
            let nx = -surface_scale / 4.0 * ((a(1, -1) + 2.0 * a(1, 0) + a(1, 1)) - (a(-1, -1) + 2.0 * a(-1, 0) + a(-1, 1)));
            let ny = -surface_scale / 4.0 * ((a(-1, 1) + 2.0 * a(0, 1) + a(1, 1)) - (a(-1, -1) + 2.0 * a(0, -1) + a(1, -1)));
            let nn = (nx * nx + ny * ny + 1.0).sqrt();
            let n = [nx / nn, ny / nn, 1.0 / nn];
            let z = surface_scale * a(0, 0);
            let (l, color) = match &src {
                Light::Distant(v) => (*v, [lcp[0], lcp[1], lcp[2]]),
                Light::Point(pp) => {
                    let v = [pp[0] - x as f32, pp[1] - y as f32, pp[2] - z];
                    let k = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
                    ([v[0] / k, v[1] / k, v[2] / k], [lcp[0], lcp[1], lcp[2]])
                }
                Light::Spot { pos, dir, exp, cos_cone } => {
                    let v = [pos[0] - x as f32, pos[1] - y as f32, pos[2] - z];
                    let k = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
                    let l = [v[0] / k, v[1] / k, v[2] / k];
                    let minus_l_dot_s = -(l[0] * dir[0] + l[1] * dir[1] + l[2] * dir[2]);
                    let f = if minus_l_dot_s <= 0.0 || cos_cone.is_some_and(|c| minus_l_dot_s < c) { 0.0 } else { minus_l_dot_s.powf(*exp) };
                    (l, [lcp[0] * f, lcp[1] * f, lcp[2] * f])
                }
            };
            let o = (y as usize * w + x as usize) * 4;
            if specular {
                let hv = [l[0], l[1], l[2] + 1.0];
                let hn = (hv[0] * hv[0] + hv[1] * hv[1] + hv[2] * hv[2]).sqrt().max(1e-9);
                let ndoth = (n[0] * hv[0] + n[1] * hv[1] + n[2] * hv[2]) / hn;
                let f = ks * ndoth.max(0.0).powf(exp);
                let rgb = [(color[0] * f).clamp(0.0, 1.0), (color[1] * f).clamp(0.0, 1.0), (color[2] * f).clamp(0.0, 1.0)];
                let a = rgb[0].max(rgb[1]).max(rgb[2]);
                d[o..o + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
            } else {
                let f = kd * (n[0] * l[0] + n[1] * l[1] + n[2] * l[2]).max(0.0);
                d[o..o + 4].copy_from_slice(&[(color[0] * f).clamp(0.0, 1.0), (color[1] * f).clamp(0.0, 1.0), (color[2] * f).clamp(0.0, 1.0), 1.0]);
            }
        }
    }
    Some(Res { data: d, linear, sub, user, tainted: false })
}
