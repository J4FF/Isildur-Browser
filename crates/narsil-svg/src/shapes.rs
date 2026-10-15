//! CSS basic shapes (CSS Shapes 1 §3: `inset()`, `xywh()`, `rect()`,
//! `circle()`, `ellipse()`, `polygon()`, `path()`) with their reference
//! box, as device paths; shared by SVG `clip-path` and `clip-path` on CSS
//! boxes.

use crate::render::BBox;
use narsil_canvas::{FillRule, Matrix, Path};

/// The reference boxes a shape may name (user space of the element).
#[derive(Clone, Copy, Debug)]
pub struct RefBoxes {
    pub content: BBox,
    pub padding: BBox,
    pub border: BBox,
    pub margin: BBox,
    pub fill: BBox,
    pub stroke: BBox,
    pub view: BBox,
    /// The box used when the value names none.
    pub default: BBox,
    /// `em` and `rem` units.
    pub font_size: f64,
    pub root_font_size: f64,
    /// The viewport for `vw`/`vh`.
    pub viewport: (f64, f64),
    /// Release 15.10.2026 (Agent F): the border box corner radii (rx, ry)
    /// of a CSS box, top-left, top-right, bottom-right, bottom-left: a box
    /// alone (`clip-path: border-box`) has these rounded corners.
    pub border_radii: [(f64, f64); 4],
}

impl RefBoxes {
    /// Boxes of an SVG element: fill box and stroke box, the CSS boxes map
    /// onto them (CSS Masking 1 §6.1).
    #[must_use]
    pub fn svg(fill: BBox, stroke: BBox, view: BBox, font_size: f64) -> RefBoxes {
        RefBoxes { content: fill, padding: fill, border: stroke, margin: stroke, fill, stroke, view, default: stroke, font_size, root_font_size: 16.0, viewport: (view.2 - view.0, view.3 - view.1), border_radii: [(0.0, 0.0); 4] }
    }

    /// Boxes of a CSS box: fill box is the content box, stroke and view box
    /// the border box.
    #[must_use]
    pub fn css(content: BBox, padding: BBox, border: BBox, margin: BBox, font_size: f64, viewport: (f64, f64)) -> RefBoxes {
        RefBoxes { content, padding, border, margin, fill: content, stroke: border, view: border, default: border, font_size, root_font_size: 16.0, viewport, border_radii: [(0.0, 0.0); 4] }
    }

    fn named(&self, kw: &str) -> Option<BBox> {
        Some(match kw {
            "content-box" => self.content,
            "padding-box" => self.padding,
            "border-box" => self.border,
            "margin-box" => self.margin,
            "fill-box" => self.fill,
            "stroke-box" => self.stroke,
            "view-box" => self.view,
            _ => return None,
        })
    }

    /// The corner radii of box `b` (one of the CSS boxes): the border box
    /// radii less the distance of each edge from the border edge (more for
    /// the margin box), CSS Backgrounds 3 §5.2.
    fn radii_of(&self, b: BBox) -> Option<[(f64, f64); 4]> {
        let r = self.border_radii;
        if !r.iter().any(|c| c.0 > 0.0 && c.1 > 0.0) {
            return None;
        }
        let o = self.border;
        let (l, t, rt, bt) = (b.0 - o.0, b.1 - o.1, o.2 - b.2, o.3 - b.3);
        let c = |(x, y): (f64, f64), dx: f64, dy: f64| if x > 0.0 && y > 0.0 { ((x - dx).max(0.0), (y - dy).max(0.0)) } else { (0.0, 0.0) };
        Some([c(r[0], l, t), c(r[1], rt, t), c(r[2], rt, bt), c(r[3], l, bt)])
    }
}

/// Splits a value into its shape function (name, arguments) and its
/// reference box keyword.
fn split(v: &str) -> Option<(Option<(String, String)>, Option<String>)> {
    let v = v.trim();
    let mut func = None;
    let mut rest = String::new();
    if let Some(open) = v.find('(') {
        let close = v.rfind(')')?;
        if close < open {
            return None;
        }
        let head = v[..open].trim();
        let (before, name) = match head.rsplit_once(char::is_whitespace) {
            Some((b, n)) => (b.trim(), n.trim()),
            None => ("", head),
        };
        func = Some((name.to_ascii_lowercase(), v[open + 1..close].trim().to_string()));
        rest.push_str(before);
        rest.push(' ');
        rest.push_str(&v[close + 1..]);
    } else {
        rest.push_str(v);
    }
    let words: Vec<String> = rest.split_whitespace().map(str::to_ascii_lowercase).collect();
    if words.len() > 1 {
        return None;
    }
    Some((func, words.into_iter().next()))
}

struct Len<'b> {
    boxes: &'b RefBoxes,
    w: f64,
    h: f64,
}

impl Len<'_> {
    /// A length or percentage of `base`.
    fn of(&self, s: &str, base: f64) -> Option<f64> {
        let s = s.trim();
        if let Some(inner) = s.strip_prefix("calc(").and_then(|x| x.strip_suffix(')')) {
            return self.calc(inner, base);
        }
        let (v, unit) = crate::units::split_number(s)?;
        Some(match unit.to_ascii_lowercase().as_str() {
            "%" => v / 100.0 * base,
            "" if v == 0.0 => 0.0,
            "px" => v,
            "em" => v * self.boxes.font_size,
            "rem" => v * self.boxes.root_font_size,
            "ex" | "ch" => v * self.boxes.font_size * 0.5,
            "vw" => v * self.boxes.viewport.0 / 100.0,
            "vh" => v * self.boxes.viewport.1 / 100.0,
            "vmin" => v * self.boxes.viewport.0.min(self.boxes.viewport.1) / 100.0,
            "vmax" => v * self.boxes.viewport.0.max(self.boxes.viewport.1) / 100.0,
            "pt" => v * 4.0 / 3.0,
            "pc" => v * 16.0,
            "in" => v * 96.0,
            "cm" => v * 96.0 / 2.54,
            "mm" => v * 96.0 / 25.4,
            "q" => v * 96.0 / 101.6,
            _ => return None,
        })
    }

    /// `calc()` with `+` and `-` between terms (and `*`/`/` by numbers).
    fn calc(&self, inner: &str, base: f64) -> Option<f64> {
        let mut total = 0.0;
        let mut sign = 1.0;
        for tok in inner.split_whitespace() {
            match tok {
                "+" => sign = 1.0,
                "-" => sign = -1.0,
                t => {
                    let v = if let Some((a, b)) = t.split_once('*') {
                        self.of(a, base)? * b.parse::<f64>().ok()?
                    } else if let Some((a, b)) = t.split_once('/') {
                        self.of(a, base)? / b.parse::<f64>().ok()?
                    } else {
                        self.of(t, base)?
                    };
                    total += sign * v;
                    sign = 1.0;
                }
            }
        }
        Some(total)
    }

    fn x(&self, s: &str) -> Option<f64> {
        self.of(s, self.w)
    }

    fn y(&self, s: &str) -> Option<f64> {
        self.of(s, self.h)
    }
}

/// Words at parenthesis depth zero.
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

/// A `<position>` (CSS Values 4 §9) in the box.
fn position(ws: &[String], l: &Len) -> Option<(f64, f64)> {
    let (w, h) = (l.w, l.h);
    let kw_x = |k: &str| match k {
        "left" => Some(0.0),
        "center" => Some(w / 2.0),
        "right" => Some(w),
        _ => None,
    };
    let kw_y = |k: &str| match k {
        "top" => Some(0.0),
        "center" => Some(h / 2.0),
        "bottom" => Some(h),
        _ => None,
    };
    let ws: Vec<String> = ws.iter().map(|w| w.to_ascii_lowercase()).collect();
    match ws.len() {
        0 => Some((w / 2.0, h / 2.0)),
        1 => {
            let a = ws[0].as_str();
            if matches!(a, "top" | "bottom") {
                Some((w / 2.0, kw_y(a)?))
            } else {
                Some((kw_x(a).or_else(|| l.x(a))?, h / 2.0))
            }
        }
        2 => {
            let (a, b) = (ws[0].as_str(), ws[1].as_str());
            if matches!(a, "top" | "bottom") || matches!(b, "left" | "right") {
                Some((kw_x(b).or_else(|| l.x(b))?, kw_y(a).or_else(|| l.y(a))?))
            } else {
                Some((kw_x(a).or_else(|| l.x(a))?, kw_y(b).or_else(|| l.y(b))?))
            }
        }
        3 | 4 => {
            // Keyword + offset pairs (`right 10px bottom 20%`).
            let mut x = None;
            let mut y = None;
            let mut i = 0;
            while i < ws.len() {
                let k = ws[i].as_str();
                let off = ws.get(i + 1).filter(|n| !matches!(n.as_str(), "left" | "right" | "top" | "bottom" | "center"));
                let step = if off.is_some() { 2 } else { 1 };
                match k {
                    "left" | "right" => {
                        let d = off.map_or(Some(0.0), |o| l.x(o))?;
                        x = Some(if k == "left" { d } else { w - d });
                    }
                    "top" | "bottom" => {
                        let d = off.map_or(Some(0.0), |o| l.y(o))?;
                        y = Some(if k == "top" { d } else { h - d });
                    }
                    "center" => {
                        if x.is_none() {
                            x = Some(w / 2.0);
                        } else {
                            y = Some(h / 2.0);
                        }
                    }
                    _ => return None,
                }
                i += step;
            }
            Some((x.unwrap_or(w / 2.0), y.unwrap_or(h / 2.0)))
        }
        _ => None,
    }
}

/// `border-radius` values after `round`: per corner (rx, ry) in the order
/// top-left, top-right, bottom-right, bottom-left.
fn radii(s: &str, l: &Len) -> Option<[(f64, f64); 4]> {
    let (hs, vs) = match s.split_once('/') {
        Some((a, b)) => (a.trim(), b.trim()),
        None => (s.trim(), s.trim()),
    };
    let expand = |t: &str, horizontal: bool| -> Option<[f64; 4]> {
        let v: Vec<f64> = words(t).iter().map(|w| if horizontal { l.x(w) } else { l.y(w) }).collect::<Option<Vec<f64>>>()?;
        Some(match v.as_slice() {
            [a] => [*a; 4],
            [a, b] => [*a, *b, *a, *b],
            [a, b, c] => [*a, *b, *c, *b],
            [a, b, c, d] => [*a, *b, *c, *d],
            _ => return None,
        })
    };
    let (h, v) = (expand(hs, true)?, expand(vs, false)?);
    Some([(h[0], v[0]), (h[1], v[1]), (h[2], v[2]), (h[3], v[3])])
}

fn rect_path(m: &Matrix, x: f64, y: f64, w: f64, h: f64, round: Option<[(f64, f64); 4]>) -> Path {
    let mut p = Path::new();
    let (w, h) = (w.max(0.0), h.max(0.0));
    match round {
        Some(r) if r.iter().any(|c| c.0 > 0.0 && c.1 > 0.0) => {
            // Overlapping radii scale down together (CSS Backgrounds 3 §5.5).
            let f = [w / (r[0].0 + r[1].0), w / (r[3].0 + r[2].0), h / (r[0].1 + r[3].1), h / (r[1].1 + r[2].1)].into_iter().filter(|v| v.is_finite()).fold(1.0f64, f64::min);
            let r = r.map(|(a, b)| (a.max(0.0) * f, b.max(0.0) * f));
            p.round_rect(m, x, y, w, h, r);
        }
        _ => p.rect(m, x, y, w, h),
    }
    p
}

/// Release 15.10.2026 (Agent F): a basic shape that has an exact
/// coverage: an ellipse or a (rounded) rectangle in the user space of `m`.
enum Analytic {
    Ellipse { m: Matrix, cx: f64, cy: f64, rx: f64, ry: f64 },
    RoundRect { m: Matrix, x: f64, y: f64, w: f64, h: f64, radii: Option<[(f64, f64); 4]> },
}

/// A basic shape value (with an optional reference box, or a box alone) as
/// a device path under `m`, with its fill rule. `None` for an invalid
/// value or `none`.
#[must_use]
pub fn basic_shape(v: &str, boxes: &RefBoxes, m: &Matrix) -> Option<(Path, FillRule)> {
    shape_with_analytic(v, boxes, m, &mut None)
}

/// Release 15.10.2026 (Agent F): the coverage of `circle()`, `ellipse()`,
/// `inset()`, `xywh()`, `rect()` and a box alone as alpha bytes of `size`
/// pixels whose top left pixel is at device `origin`: the signed distance
/// at each pixel centre with a one-pixel soft edge, the same coverage the
/// rasterizer gives a box with `border-radius` (so `circle(50%)` and a
/// round box match). `None` for other shapes (polygons, paths) and for a
/// rotating or skewing `m`: those are filled as paths.
#[must_use]
pub fn basic_shape_coverage(v: &str, boxes: &RefBoxes, m: &Matrix, origin: (f64, f64), size: (u32, u32)) -> Option<Vec<u8>> {
    let mut a = None;
    shape_with_analytic(v, boxes, m, &mut a)?;
    let a = a?;
    let mm = match &a {
        Analytic::Ellipse { m, .. } | Analytic::RoundRect { m, .. } => *m,
    };
    if mm.b != 0.0 || mm.c != 0.0 || mm.a <= 0.0 || (mm.a - mm.d).abs() > 1e-9 {
        return None;
    }
    let s = mm.a;
    let (w, h) = (size.0 as usize, size.1 as usize);
    let mut out = vec![0u8; w * h];
    let cover = |d: f64| ((0.5 - d).clamp(0.0, 1.0) * 255.0).round() as u8;
    match a {
        Analytic::Ellipse { cx, cy, rx, ry, .. } => {
            let (cx, cy, rx, ry) = (s * cx + mm.e, s * cy + mm.f, s * rx, s * ry);
            if rx <= 0.0 || ry <= 0.0 {
                return Some(out);
            }
            for y in 0..h {
                let py = origin.1 + y as f64 + 0.5 - cy;
                for x in 0..w {
                    let px = origin.0 + x as f64 + 0.5 - cx;
                    out[y * w + x] = cover(ellipse_distance(px, py, rx, ry));
                }
            }
        }
        Analytic::RoundRect { x, y, w: rw, h: rh, radii, .. } => {
            let (rx0, ry0, rw, rh) = (s * x + mm.e, s * y + mm.f, s * rw.max(0.0), s * rh.max(0.0));
            // Overlapping radii scale down together, as in `rect_path`.
            let radii = radii.filter(|r| r.iter().any(|c| c.0 > 0.0 && c.1 > 0.0)).map(|r| {
                let f = [rw / (s * (r[0].0 + r[1].0)), rw / (s * (r[3].0 + r[2].0)), rh / (s * (r[0].1 + r[3].1)), rh / (s * (r[1].1 + r[2].1))]
                    .into_iter()
                    .filter(|v| v.is_finite())
                    .fold(1.0f64, f64::min);
                r.map(|(a, b)| (a.max(0.0) * f * s, b.max(0.0) * f * s))
            });
            for yy in 0..h {
                let py = origin.1 + yy as f64 + 0.5;
                for xx in 0..w {
                    let px = origin.0 + xx as f64 + 0.5;
                    out[yy * w + xx] = cover(round_rect_distance(px, py, (rx0, ry0, rw, rh), radii));
                }
            }
        }
    }
    Some(out)
}

/// Signed distance to the ellipse with radii `rx`, `ry` around the origin
/// (exact for a circle, the gradient approximation otherwise).
fn ellipse_distance(px: f64, py: f64, rx: f64, ry: f64) -> f64 {
    if (rx - ry).abs() < 1e-9 {
        return (px * px + py * py).sqrt() - rx;
    }
    let (nx, ny) = (px / rx, py / ry);
    let k0 = (nx * nx + ny * ny).sqrt();
    let (gx, gy) = (px / (rx * rx), py / (ry * ry));
    let k1 = (gx * gx + gy * gy).sqrt();
    if k1 > 0.0 { k0 * (k0 - 1.0) / k1 } else { -rx.min(ry) }
}

/// Signed distance to a rectangle (x, y, w, h) with the (fitted) corner
/// radii, top-left, top-right, bottom-right, bottom-left — the rasterizer's
/// rounded box distance (`glas::abstand`, `formen::distance_xy`).
fn round_rect_distance(px: f64, py: f64, r: (f64, f64, f64, f64), radii: Option<[(f64, f64); 4]>) -> f64 {
    let (hb, hh) = (r.2 / 2.0, r.3 / 2.0);
    let (mx, my) = (r.0 + hb, r.1 + hh);
    let (qx, qy) = ((px - mx).abs(), (py - my).abs());
    let i = match (px > mx, py > my) {
        (false, false) => 0,
        (true, false) => 1,
        (true, true) => 2,
        (false, true) => 3,
    };
    let (ax, ay) = radii.map_or((0.0, 0.0), |c| c[i]);
    let (ax, ay) = (ax.clamp(0.0, hb), ay.clamp(0.0, hh));
    if ax > 0.0 && ay > 0.0 {
        if (ax - ay).abs() < 1e-9 {
            let rad = ax.min(hb.min(hh));
            let ex = qx - (hb - rad);
            let ey = qy - (hh - rad);
            return (ex.max(0.0).powi(2) + ey.max(0.0).powi(2)).sqrt() + ex.max(ey).min(0.0) - rad;
        }
        let (dx, dy) = (qx - (hb - ax), qy - (hh - ay));
        if dx > 0.0 && dy > 0.0 {
            return ellipse_distance(dx, dy, ax, ay);
        }
    }
    let (ex, ey) = (qx - hb, qy - hh);
    (ex.max(0.0).powi(2) + ey.max(0.0).powi(2)).sqrt() + ex.max(ey).min(0.0)
}

fn shape_with_analytic(v: &str, boxes: &RefBoxes, m: &Matrix, analytic: &mut Option<Analytic>) -> Option<(Path, FillRule)> {
    let (func, kw) = split(v)?;
    let b = match kw.as_deref() {
        Some(k) => boxes.named(k)?,
        None => boxes.default,
    };
    let (bx, by, bw, bh) = (b.0, b.1, b.2 - b.0, b.3 - b.1);
    let base = m.multiply(&Matrix::translate(bx, by));
    let Some((name, args)) = func else {
        // A box alone clips to that box, with its rounded corners.
        *analytic = kw.as_ref().map(|_| Analytic::RoundRect { m: *m, x: bx, y: by, w: bw, h: bh, radii: boxes.radii_of(b) });
        return kw.map(|_| (rect_path(m, bx, by, bw, bh, boxes.radii_of(b)), FillRule::NonZero));
    };
    let l = Len { boxes, w: bw, h: bh };
    let split_round = |a: &str| -> (String, Option<String>) {
        let ws = words(a);
        match ws.iter().position(|w| w.eq_ignore_ascii_case("round")) {
            Some(k) => (ws[..k].join(" "), Some(ws[k + 1..].join(" "))),
            None => (ws.join(" "), None),
        }
    };
    let rule_prefix = |a: &str| -> (FillRule, String) {
        if let Some((first, rest)) = a.split_once(',') {
            match first.trim().to_ascii_lowercase().as_str() {
                "evenodd" => return (FillRule::EvenOdd, rest.to_string()),
                "nonzero" => return (FillRule::NonZero, rest.to_string()),
                _ => {}
            }
        }
        (FillRule::NonZero, a.to_string())
    };
    match name.as_str() {
        "inset" => {
            let (main, round) = split_round(&args);
            let ws = words(&main);
            // Top and bottom are heights, left and right widths.
            let (t, r, bo, le) = match ws.as_slice() {
                [a] => (a, a, a, a),
                [a, b] => (a, b, a, b),
                [a, b, c] => (a, b, c, b),
                [a, b, c, d] => (a, b, c, d),
                _ => return None,
            };
            let (t, r, bo, le) = (l.y(t)?, l.x(r)?, l.y(bo)?, l.x(le)?);
            // Insets that add up to more than the box leave nothing.
            let (w, h) = ((bw - le - r).max(0.0), (bh - t - bo).max(0.0));
            // Radius percentages refer to the reference box; radii that do
            // not fit scale down together (WPT clip-path-inset-round-percent,
            // -rendering).
            let round = match round {
                Some(rs) => Some(radii(&rs, &l)?),
                None => None,
            };
            *analytic = Some(Analytic::RoundRect { m: base, x: le, y: t, w, h, radii: round });
            Some((rect_path(&base, le, t, w, h, round), FillRule::NonZero))
        }
        "xywh" => {
            let (main, round) = split_round(&args);
            let ws = words(&main);
            if ws.len() != 4 {
                return None;
            }
            let (x, y, w, h) = (l.x(&ws[0])?, l.y(&ws[1])?, l.x(&ws[2])?, l.y(&ws[3])?);
            if w < 0.0 || h < 0.0 {
                return None;
            }
            let round = match round {
                Some(rs) => Some(radii(&rs, &l)?),
                None => None,
            };
            *analytic = Some(Analytic::RoundRect { m: base, x, y, w, h, radii: round });
            Some((rect_path(&base, x, y, w, h, round), FillRule::NonZero))
        }
        "rect" => {
            let (main, round) = split_round(&args);
            let ws = words(&main);
            if ws.len() != 4 {
                return None;
            }
            let auto = |s: &str, d: f64, f: &dyn Fn(&str) -> Option<f64>| if s.eq_ignore_ascii_case("auto") { Some(d) } else { f(s) };
            let t = auto(&ws[0], 0.0, &|s| l.y(s))?;
            let r = auto(&ws[1], bw, &|s| l.x(s))?;
            let bo = auto(&ws[2], bh, &|s| l.y(s))?;
            let le = auto(&ws[3], 0.0, &|s| l.x(s))?;
            let round = match round {
                Some(rs) => Some(radii(&rs, &l)?),
                None => None,
            };
            *analytic = Some(Analytic::RoundRect { m: base, x: le, y: t, w: (r - le).max(0.0), h: (bo - t).max(0.0), radii: round });
            Some((rect_path(&base, le, t, (r - le).max(0.0), (bo - t).max(0.0), round), FillRule::NonZero))
        }
        "circle" | "ellipse" => {
            let ws = words(&args);
            let at = ws.iter().position(|w| w.eq_ignore_ascii_case("at"));
            let (rs, ps) = match at {
                Some(k) => (&ws[..k], &ws[k + 1..]),
                None => (&ws[..], &ws[ws.len()..]),
            };
            let (cx, cy) = position(ps, &l)?;
            let (dx0, dx1, dy0, dy1) = (cx.abs(), (bw - cx).abs(), cy.abs(), (bh - cy).abs());
            let corner = |far: bool| {
                let cs = [(0.0, 0.0), (bw, 0.0), (0.0, bh), (bw, bh)].map(|(x, y): (f64, f64)| (x - cx).hypot(y - cy));
                if far { cs.into_iter().fold(0.0, f64::max) } else { cs.into_iter().fold(f64::INFINITY, f64::min) }
            };
            let (rx, ry) = if name == "circle" {
                let r = match rs.first().map(|s| s.to_ascii_lowercase()) {
                    None => dx0.min(dx1).min(dy0).min(dy1),
                    Some(s) if s == "closest-side" => dx0.min(dx1).min(dy0).min(dy1),
                    Some(s) if s == "farthest-side" => dx0.max(dx1).max(dy0).max(dy1),
                    Some(s) if s == "closest-corner" => corner(false),
                    Some(s) if s == "farthest-corner" => corner(true),
                    Some(s) => l.of(&s, (bw.hypot(bh)) / std::f64::consts::SQRT_2)?,
                };
                if rs.len() > 1 || r < 0.0 {
                    return None;
                }
                (r, r)
            } else {
                let one = |s: Option<&String>, horizontal: bool| -> Option<f64> {
                    let (d0, d1) = if horizontal { (dx0, dx1) } else { (dy0, dy1) };
                    match s.map(|s| s.to_ascii_lowercase()) {
                        None => Some(d0.min(d1)),
                        Some(s) if s == "closest-side" => Some(d0.min(d1)),
                        Some(s) if s == "farthest-side" => Some(d0.max(d1)),
                        Some(s) if s == "closest-corner" || s == "farthest-corner" => {
                            // The corner ellipse keeps the side ratio.
                            let (sx, sy) = if s == "closest-corner" { (dx0.min(dx1), dy0.min(dy1)) } else { (dx0.max(dx1), dy0.max(dy1)) };
                            Some(if horizontal { sx * std::f64::consts::SQRT_2 } else { sy * std::f64::consts::SQRT_2 })
                        }
                        Some(s) => {
                            if horizontal {
                                l.x(&s)
                            } else {
                                l.y(&s)
                            }
                        }
                    }
                };
                if rs.len() == 1 || rs.len() > 2 {
                    return None;
                }
                (one(rs.first(), true)?, one(rs.get(1), false)?)
            };
            let mut p = Path::new();
            if rx > 0.0 && ry > 0.0 {
                p.ellipse(&base, cx, cy, rx, ry, 0.0, 0.0, std::f64::consts::TAU, false).ok()?;
                p.close();
            }
            *analytic = Some(Analytic::Ellipse { m: base, cx, cy, rx, ry });
            Some((p, FillRule::NonZero))
        }
        "polygon" => {
            let (rule, pts) = rule_prefix(&args);
            let mut p = Path::new();
            let mut first = true;
            for pair in pts.split(',') {
                let ws = words(pair);
                if ws.len() != 2 {
                    return None;
                }
                let (x, y) = (l.x(&ws[0])?, l.y(&ws[1])?);
                if first {
                    p.move_to(&base, x, y);
                    first = false;
                } else {
                    p.line_to(&base, x, y);
                }
            }
            p.close();
            Some((p, rule))
        }
        "path" => {
            let (rule, d) = rule_prefix(&args);
            let d = d.trim().trim_matches(|c| c == '"' || c == '\'');
            let segs = crate::render::segments_from_commands(&narsil_canvas::commands::parse_svg_path(d));
            // `path()` coordinates are relative to the box origin.
            Some((crate::render::build_path(&segs, &base), rule))
        }
        "shape" => {
            // `shape([<fill-rule>]? from ..., ...)`: no comma after the rule.
            let a = args.trim_start();
            let lower = a.to_ascii_lowercase();
            let (rule, cmds) = if lower.starts_with("evenodd ") {
                (FillRule::EvenOdd, &a[8..])
            } else if lower.starts_with("nonzero ") {
                (FillRule::NonZero, &a[8..])
            } else {
                (FillRule::NonZero, a)
            };
            let d = shape_to_path_data(cmds, &l)?;
            let segs = crate::render::segments_from_commands(&narsil_canvas::commands::parse_svg_path(&d));
            Some((crate::render::build_path(&segs, &base), rule))
        }
        _ => None,
    }
}

/// The `shape()` commands (CSS Shapes 2 §2.1) as SVG path data with
/// absolute coordinates in the reference box.
fn shape_to_path_data(cmds: &str, l: &Len) -> Option<String> {
    use std::fmt::Write;
    let parts: Vec<&str> = cmds.split(',').map(str::trim).collect();
    let first = words(parts.first()?);
    if first.first().map(|w| w.to_ascii_lowercase()).as_deref() != Some("from") {
        return None;
    }
    let (mut cx, mut cy) = position(&first[1..], l)?;
    let (sx, sy) = (cx, cy);
    let mut d = format!("M {cx} {cy}");
    let pair = |ws: &[String]| -> Option<(f64, f64)> {
        match ws {
            [a, b] => Some((l.x(a)?, l.y(b)?)),
            _ => None,
        }
    };
    for part in &parts[1..] {
        let ws = words(part);
        let Some(cmd) = ws.first().map(|w| w.to_ascii_lowercase()) else { continue };
        if cmd == "close" {
            d.push_str(" Z");
            (cx, cy) = (sx, sy);
            continue;
        }
        let rel = match ws.get(1).map(|w| w.to_ascii_lowercase()).as_deref() {
            Some("to") => false,
            Some("by") => true,
            _ => return None,
        };
        let rest = &ws[2..];
        let abs = |(x, y): (f64, f64)| if rel { (cx + x, cy + y) } else { (x, y) };
        match cmd.as_str() {
            "move" | "line" => {
                let (x, y) = abs(pair(rest.get(..2)?)?);
                let _ = write!(d, " {} {x} {y}", if cmd == "move" { "M" } else { "L" });
                (cx, cy) = (x, y);
            }
            "hline" | "vline" => {
                let v = rest.first()?;
                if cmd == "hline" {
                    let x = if rel { cx + l.x(v)? } else { l.x(v)? };
                    let _ = write!(d, " L {x} {cy}");
                    cx = x;
                } else {
                    let y = if rel { cy + l.y(v)? } else { l.y(v)? };
                    let _ = write!(d, " L {cx} {y}");
                    cy = y;
                }
            }
            "curve" | "smooth" => {
                let end = abs(pair(rest.get(..2)?)?);
                // `with` control points: one (quadratic) or two (cubic),
                // relative to the start point for `by`.
                let mut ctrl = Vec::new();
                if rest.get(2).is_some_and(|w| w.eq_ignore_ascii_case("with")) {
                    let tail = &rest[3..];
                    let groups: Vec<&[String]> = tail.split(|w| w == "/").collect();
                    for g in groups {
                        ctrl.push(abs(pair(g)?));
                    }
                }
                let (x, y) = end;
                match (cmd.as_str(), ctrl.as_slice()) {
                    ("curve", [c1]) => {
                        let _ = write!(d, " Q {} {} {x} {y}", c1.0, c1.1);
                    }
                    ("curve", [c1, c2]) => {
                        let _ = write!(d, " C {} {} {} {} {x} {y}", c1.0, c1.1, c2.0, c2.1);
                    }
                    ("smooth", []) => {
                        let _ = write!(d, " T {x} {y}");
                    }
                    ("smooth", [c2]) => {
                        let _ = write!(d, " S {} {} {x} {y}", c2.0, c2.1);
                    }
                    _ => return None,
                }
                (cx, cy) = end;
            }
            "arc" => {
                let (x, y) = abs(pair(rest.get(..2)?)?);
                if !rest.get(2).is_some_and(|w| w.eq_ignore_ascii_case("of")) {
                    return None;
                }
                let mut k = 3;
                let rx = l.x(rest.get(k)?)?;
                k += 1;
                let ry = match rest.get(k) {
                    Some(w) if l.y(w).is_some() => {
                        k += 1;
                        l.y(w)?
                    }
                    _ => rx,
                };
                let (mut sweep, mut large, mut rot) = (0, 0, 0.0);
                while let Some(w) = rest.get(k) {
                    match w.to_ascii_lowercase().as_str() {
                        "cw" => sweep = 1,
                        "ccw" => sweep = 0,
                        "large" => large = 1,
                        "small" => large = 0,
                        "rotate" => {
                            k += 1;
                            rot = crate::units::angle(rest.get(k)?)?;
                        }
                        _ => return None,
                    }
                    k += 1;
                }
                let _ = write!(d, " A {rx} {ry} {rot} {large} {sweep} {x} {y}");
                (cx, cy) = (x, y);
            }
            _ => return None,
        }
    }
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes() -> RefBoxes {
        let b = (10.0, 20.0, 110.0, 70.0);
        RefBoxes::css((20.0, 30.0, 100.0, 60.0), b, b, (0.0, 10.0, 120.0, 80.0), 16.0, (800.0, 600.0))
    }

    #[test]
    fn shapes_resolve_against_their_box() {
        let m = Matrix::IDENTITY;
        let (p, _) = basic_shape("inset(10px 20%)", &boxes(), &m).unwrap();
        let b = p.bounds();
        assert_eq!((b.x0, b.y0, b.x1, b.y1), (30.0, 30.0, 90.0, 60.0));
        let (p, _) = basic_shape("circle(10px at left top) content-box", &boxes(), &m).unwrap();
        let b = p.bounds();
        assert!((b.x0 - 10.0).abs() < 0.1 && (b.y1 - 40.0).abs() < 0.1, "{b:?}");
        let (_, rule) = basic_shape("polygon(evenodd, 0 0, 100% 0, 0 100%)", &boxes(), &m).unwrap();
        assert_eq!(rule, FillRule::EvenOdd);
        let (p, _) = basic_shape("margin-box", &boxes(), &m).unwrap();
        assert_eq!(p.bounds().x1, 120.0);
        assert!(basic_shape("circle(10px 20px)", &boxes(), &m).is_none());
        assert!(basic_shape("bogus(1px)", &boxes(), &m).is_none());
    }
}
