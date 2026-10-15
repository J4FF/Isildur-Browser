//! Path stroking (HTML §4.12.5.1.5 "trace a path"): joins, caps and dashes.
//!
//! The stroke is built in user space (so non-uniform transforms shear the
//! pen like the spec requires) as a union of convex pieces — one quad per
//! segment plus join and cap pieces. Every piece is emitted with the same
//! orientation in device space, so a non-zero fill of all pieces is their
//! union.

use crate::geom::Matrix;
use crate::raster::Line;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

impl LineCap {
    #[must_use]
    pub fn parse(s: &str) -> Option<LineCap> {
        Some(match s {
            "butt" => LineCap::Butt,
            "round" => LineCap::Round,
            "square" => LineCap::Square,
            _ => return None,
        })
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            LineCap::Butt => "butt",
            LineCap::Round => "round",
            LineCap::Square => "square",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

impl LineJoin {
    #[must_use]
    pub fn parse(s: &str) -> Option<LineJoin> {
        Some(match s {
            "miter" => LineJoin::Miter,
            "round" => LineJoin::Round,
            "bevel" => LineJoin::Bevel,
            _ => return None,
        })
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            LineJoin::Miter => "miter",
            LineJoin::Round => "round",
            LineJoin::Bevel => "bevel",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStyle {
    pub width: f64,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f64,
    pub dash: Vec<f64>,
    pub dash_offset: f64,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        StrokeStyle { width: 1.0, cap: LineCap::Butt, join: LineJoin::Miter, miter_limit: 10.0, dash: Vec::new(), dash_offset: 0.0 }
    }
}

type P = (f64, f64);

/// One polyline of the path in user space.
pub struct Polyline<'a> {
    pub points: &'a [P],
    /// Per point: the unit tangent of an interior point of a flattened
    /// curve (user space), `None` (or NaN) for corners.
    pub tangents: Option<&'a [P]>,
    pub closed: bool,
}

/// Collects stroke pieces as device-space edges.
pub struct Stroker<'a> {
    style: &'a StrokeStyle,
    m: &'a Matrix,
    hw: f64,
    /// Device-space flattening step for round pieces (radians).
    round_step: f64,
    pub lines: Vec<Line>,
    scratch: Vec<P>,
}

impl<'a> Stroker<'a> {
    #[must_use]
    pub fn new(style: &'a StrokeStyle, m: &'a Matrix) -> Stroker<'a> {
        let hw = style.width / 2.0;
        let r_dev = hw * m.max_scale();
        let tol = crate::path::TOLERANCE;
        let round_step = if r_dev > tol { 2.0 * (1.0 - tol / r_dev).clamp(-1.0, 1.0).acos() } else { std::f64::consts::FRAC_PI_2 };
        Stroker { style, m, hw, round_step: round_step.clamp(0.01, std::f64::consts::FRAC_PI_2), lines: Vec::new(), scratch: Vec::new() }
    }

    /// Strokes one subpath (user-space points).
    pub fn stroke(&mut self, poly: Polyline<'_>) {
        if !(self.hw > 0.0) || !self.hw.is_finite() {
            return;
        }
        let mut pts: Vec<P> = Vec::with_capacity(poly.points.len());
        let mut smooth: Vec<Option<P>> = Vec::with_capacity(poly.points.len());
        for (i, &p) in poly.points.iter().enumerate() {
            if !(p.0.is_finite() && p.1.is_finite()) {
                continue;
            }
            let s = poly.tangents.and_then(|f| f.get(i).copied()).filter(|t| t.0.is_finite() && t.1.is_finite());
            if pts.last().is_none_or(|q: &P| !same(*q, p)) {
                pts.push(p);
                smooth.push(s);
            } else if let Some(last) = smooth.last_mut() {
                // A duplicate point keeps the corner if either was one.
                if s.is_none() {
                    *last = None;
                }
            }
        }
        let closed = poly.closed;
        if closed && pts.len() > 1 && same(pts[0], pts[pts.len() - 1]) {
            pts.pop();
            smooth.pop();
        }
        if pts.len() < 2 {
            return;
        }
        smooth[0] = None;
        let dash_total: f64 = self.style.dash.iter().sum();
        if self.style.dash.is_empty() || !(dash_total > 0.0) || !dash_total.is_finite() {
            self.stroke_polyline(&pts, &smooth, closed);
            return;
        }
        self.stroke_dashed(&pts, &smooth, closed, dash_total);
    }

    fn stroke_dashed(&mut self, pts: &[P], smooth: &[Option<P>], closed: bool, total: f64) {
        let dash = self.style.dash.clone();
        let mut ring: Vec<P> = pts.to_vec();
        let mut ring_s: Vec<Option<P>> = smooth.to_vec();
        if closed {
            ring.push(pts[0]);
            ring_s.push(None);
        }
        // Position inside the pattern at the start of the subpath.
        let mut offset = self.style.dash_offset % total;
        if offset < 0.0 {
            offset += total;
        }
        let mut index = 0usize;
        while offset >= dash[index] && dash[index] >= 0.0 {
            offset -= dash[index];
            index = (index + 1) % dash.len();
            if offset <= 0.0 {
                break;
            }
        }
        let mut remaining = dash[index] - offset;
        let mut on = index % 2 == 0;
        let mut current: Vec<P> = Vec::new();
        let mut current_s: Vec<Option<P>> = Vec::new();
        if on {
            current.push(ring[0]);
            current_s.push(None);
        }
        let mut guard = 0usize;
        for i in 0..ring.len() - 1 {
            let (a, b) = (ring[i], ring[i + 1]);
            let len = dist(a, b);
            let mut t0 = 0.0;
            while len - t0 > remaining {
                guard += 1;
                if guard > 1_000_000 {
                    return;
                }
                t0 += remaining;
                let p = lerp(a, b, t0 / len);
                if on {
                    current.push(p);
                    current_s.push(None);
                    self.emit_dash(&mut current, &mut current_s, (b.0 - a.0, b.1 - a.1));
                } else {
                    current.clear();
                    current_s.clear();
                    current.push(p);
                    current_s.push(None);
                }
                on = !on;
                index = (index + 1) % dash.len();
                remaining = dash[index];
            }
            remaining -= len - t0;
            if on {
                current.push(b);
                current_s.push(ring_s[i + 1]);
            }
        }
        if on && current.len() > 1 {
            let d = {
                let n = ring.len();
                (ring[n - 1].0 - ring[n - 2].0, ring[n - 1].1 - ring[n - 2].1)
            };
            if let Some(l) = current_s.last_mut() {
                *l = None;
            }
            self.emit_dash(&mut current, &mut current_s, d);
        }
    }

    fn emit_dash(&mut self, current: &mut Vec<P>, current_s: &mut Vec<Option<P>>, dir: P) {
        let mut pts: Vec<P> = Vec::with_capacity(current.len());
        let mut sm: Vec<Option<P>> = Vec::with_capacity(current.len());
        for (i, &p) in current.iter().enumerate() {
            if pts.last().is_none_or(|q: &P| !same(*q, p)) {
                pts.push(p);
                sm.push(current_s.get(i).copied().flatten());
            }
        }
        if pts.len() >= 2 {
            sm[0] = None;
            let last = sm.len() - 1;
            sm[last] = None;
            self.stroke_polyline(&pts, &sm, false);
        } else if pts.len() == 1 && self.style.cap != LineCap::Butt {
            // A zero-length dash still gets its caps (dotted lines).
            let l = (dir.0 * dir.0 + dir.1 * dir.1).sqrt();
            if l > 0.0 {
                let d = (dir.0 / l, dir.1 / l);
                self.cap(pts[0], d);
                self.cap(pts[0], (-d.0, -d.1));
            }
        }
        current.clear();
        current_s.clear();
    }

    /// Strokes a polyline. Interior points of flattened curves (`smooth`)
    /// share one vertex normal between their two segments, so the body
    /// follows the curve's normals exactly (no slivers past the caps of
    /// thick strokes); real corners get joins.
    fn stroke_polyline(&mut self, pts: &[P], smooth: &[Option<P>], closed: bool) {
        let n = pts.len();
        let seg_count = if closed { n } else { n - 1 };
        let seg_nrm = |i: usize| {
            let d = unit(pts[i], pts[(i + 1) % n]);
            (-d.1, d.0)
        };
        // Vertex normals at smooth vertices (None = a corner).
        let mut vn: Vec<Option<P>> = vec![None; n];
        for i in 0..n {
            let interior = closed || (i > 0 && i < n - 1);
            let Some(t) = smooth[i].filter(|_| interior) else { continue };
            let a = seg_nrm((i + n - 1) % n);
            let b = seg_nrm(i);
            let nrm = (-t.1, t.0);
            // The exact curve normal, unless the chords disagree with it
            // (degenerate flattening): then the vertex is a corner.
            if nrm.0 * a.0 + nrm.1 * a.1 > 0.5 && nrm.0 * b.0 + nrm.1 * b.1 > 0.5 {
                vn[i] = Some(nrm);
            }
        }
        for i in 0..seg_count {
            let j = (i + 1) % n;
            let (a, b) = (pts[i], pts[j]);
            let sn = seg_nrm(i);
            let na = vn[i].unwrap_or(sn);
            let nb = vn[j].unwrap_or(sn);
            self.half(a, b, na, nb, self.hw);
            self.half(a, b, na, nb, -self.hw);
        }
        let joins: Box<dyn Iterator<Item = usize>> = if closed { Box::new(0..n) } else { Box::new(1..n - 1) };
        for i in joins {
            if vn[i].is_some() {
                continue;
            }
            let prev = pts[(i + n - 1) % n];
            let cur = pts[i];
            let next = pts[(i + 1) % n];
            self.join(cur, unit(prev, cur), unit(cur, next));
        }
        if !closed {
            let d0 = unit(pts[1], pts[0]);
            self.cap(pts[0], d0);
            let d1 = unit(pts[n - 2], pts[n - 1]);
            self.cap(pts[n - 1], d1);
        }
    }

    /// One side of a segment piece: the region swept by the normals from
    /// `a + h*na` to `b + h*nb`. When the two normals cross (a curve tighter
    /// than the half width), the piece is the two triangles on either side
    /// of the crossing point instead of a self-intersecting quad.
    fn half(&mut self, a: P, b: P, na: P, nb: P, h: f64) {
        let qa = (a.0 + na.0 * h, a.1 + na.1 * h);
        let qb = (b.0 + nb.0 * h, b.1 + nb.1 * h);
        if let Some(x) = segment_intersection(a, qa, b, qb) {
            self.emit(&[a, b, x]);
            self.emit(&[x, qb, qa]);
        } else {
            self.emit(&[a, b, qb, qa]);
        }
    }

    fn join(&mut self, p: P, d0: P, d1: P) {
        let hw = self.hw;
        let cross = d0.0 * d1.1 - d0.1 * d1.0;
        let dot = d0.0 * d1.0 + d0.1 * d1.1;
        if cross.abs() < 1e-12 {
            if dot > 0.0 {
                return;
            }
            // A full reversal: only a round join adds anything.
            if self.style.join == LineJoin::Round {
                self.cap_round(p, d0);
            }
            return;
        }
        let s = if cross > 0.0 { -1.0 } else { 1.0 };
        let n0 = (-d0.1 * s, d0.0 * s);
        let n1 = (-d1.1 * s, d1.0 * s);
        let a = (p.0 + n0.0 * hw, p.1 + n0.1 * hw);
        let b = (p.0 + n1.0 * hw, p.1 + n1.1 * hw);
        match self.style.join {
            LineJoin::Bevel => self.emit(&[p, a, b]),
            LineJoin::Miter => {
                // cos(theta/2) with theta the turn angle.
                let half_cos = ((1.0 + dot) / 2.0).max(0.0).sqrt();
                if half_cos > 0.0 && 1.0 / half_cos <= self.style.miter_limit {
                    let (bx, by) = (n0.0 + n1.0, n0.1 + n1.1);
                    let bl = (bx * bx + by * by).sqrt();
                    let len = hw / half_cos;
                    let mp = (p.0 + bx / bl * len, p.1 + by / bl * len);
                    self.emit(&[p, a, mp, b]);
                } else {
                    self.emit(&[p, a, b]);
                }
            }
            LineJoin::Round => {
                let a0 = n0.1.atan2(n0.0);
                let mut a1 = n1.1.atan2(n1.0);
                // Sweep the short way from n0 to n1.
                let mut sweep = a1 - a0;
                while sweep > std::f64::consts::PI {
                    sweep -= std::f64::consts::TAU;
                }
                while sweep < -std::f64::consts::PI {
                    sweep += std::f64::consts::TAU;
                }
                a1 = a0 + sweep;
                let mut pts = std::mem::take(&mut self.scratch);
                pts.clear();
                pts.push(p);
                let steps = ((sweep.abs() / self.round_step).ceil() as usize).max(1);
                for k in 0..=steps {
                    let t = a0 + (a1 - a0) * k as f64 / steps as f64;
                    pts.push((p.0 + hw * t.cos(), p.1 + hw * t.sin()));
                }
                self.emit(&pts);
                self.scratch = pts;
            }
        }
    }

    /// A cap at `p` facing direction `d` (unit, pointing away from the line).
    fn cap(&mut self, p: P, d: P) {
        match self.style.cap {
            LineCap::Butt => {}
            LineCap::Square => {
                let hw = self.hw;
                let n = (-d.1 * hw, d.0 * hw);
                let e = (d.0 * hw, d.1 * hw);
                self.emit(&[
                    (p.0 + n.0, p.1 + n.1),
                    (p.0 + n.0 + e.0, p.1 + n.1 + e.1),
                    (p.0 - n.0 + e.0, p.1 - n.1 + e.1),
                    (p.0 - n.0, p.1 - n.1),
                ]);
            }
            LineCap::Round => self.cap_round(p, d),
        }
    }

    fn cap_round(&mut self, p: P, d: P) {
        let hw = self.hw;
        let start = d.1.atan2(d.0) - std::f64::consts::FRAC_PI_2;
        let steps = ((std::f64::consts::PI / self.round_step).ceil() as usize).max(2);
        let mut pts = std::mem::take(&mut self.scratch);
        pts.clear();
        for k in 0..=steps {
            let t = start + std::f64::consts::PI * k as f64 / steps as f64;
            pts.push((p.0 + hw * t.cos(), p.1 + hw * t.sin()));
        }
        self.emit(&pts);
        self.scratch = pts;
    }

    /// Emits a convex polygon (user space) with positive device orientation.
    fn emit(&mut self, pts: &[P]) {
        let m = self.m;
        let n = pts.len();
        let mut area = 0.0;
        let dev: Vec<P> = pts.iter().map(|p| m.apply(p.0, p.1)).collect();
        for i in 0..n {
            let (a, b) = (dev[i], dev[(i + 1) % n]);
            area += a.0 * b.1 - b.0 * a.1;
        }
        if !area.is_finite() || area == 0.0 {
            return;
        }
        for i in 0..n {
            let (a, b) = if area > 0.0 { (dev[i], dev[(i + 1) % n]) } else { (dev[(i + 1) % n], dev[i]) };
            if a.1 != b.1 {
                self.lines.push(Line::new(a.0, a.1, b.0, b.1));
            }
        }
    }
}

/// The crossing point of segments `p1-p2` and `p3-p4`, strictly inside
/// both (touching ends do not count).
fn segment_intersection(p1: P, p2: P, p3: P, p4: P) -> Option<P> {
    let d1 = (p2.0 - p1.0, p2.1 - p1.1);
    let d2 = (p4.0 - p3.0, p4.1 - p3.1);
    let den = d1.0 * d2.1 - d1.1 * d2.0;
    if den.abs() < 1e-12 {
        return None;
    }
    let w = (p3.0 - p1.0, p3.1 - p1.1);
    let t = (w.0 * d2.1 - w.1 * d2.0) / den;
    let u = (w.0 * d1.1 - w.1 * d1.0) / den;
    let eps = 1e-9;
    (t > eps && t < 1.0 - eps && u > eps && u < 1.0 - eps).then(|| (p1.0 + d1.0 * t, p1.1 + d1.1 * t))
}

/// Equal up to rounding noise (flattening a degenerate curve produces
/// points that differ in the last bits; they must still be pruned).
fn same(a: P, b: P) -> bool {
    let eps = |u: f64, v: f64| (u - v).abs() <= 1e-9 * (1.0 + u.abs().max(v.abs()));
    eps(a.0, b.0) && eps(a.1, b.1)
}

fn dist(a: P, b: P) -> f64 {
    ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
}

fn lerp(a: P, b: P, t: f64) -> P {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

fn unit(a: P, b: P) -> P {
    let l = dist(a, b);
    if l == 0.0 { (1.0, 0.0) } else { ((b.0 - a.0) / l, (b.1 - a.1) / l) }
}

/// Point-in-stroke test in user space: the stroke pieces are convex, so a
/// point is inside when it is inside any piece (boundary included).
#[must_use]
pub fn stroke_contains(lines: &[Line], x: f64, y: f64) -> bool {
    // Non-zero winding over all pieces plus an on-edge check.
    let (x, y) = (x as f32, y as f32);
    let mut winding = 0;
    for l in lines {
        let (ax, ay, bx, by) = (l.x0, l.y0, l.x1, l.y1);
        let (dx, dy) = (bx - ax, by - ay);
        let len = dx * dx + dy * dy;
        if len > 0.0 {
            let t = ((x - ax) * dx + (y - ay) * dy) / len;
            if (0.0..=1.0).contains(&t) {
                let (px, py) = (ax + t * dx, ay + t * dy);
                if (px - x).abs() < 1e-4 && (py - y).abs() < 1e-4 {
                    return true;
                }
            }
        }
        if (ay <= y && by > y) || (by <= y && ay > y) {
            let t = (y - ay) / (by - ay);
            if ax + t * (bx - ax) > x {
                winding += if by > ay { 1 } else { -1 };
            }
        }
    }
    winding != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::IRect;
    use crate::raster::{FillRule, Rasterizer};

    fn render(lines: &[Line], w: i32, h: i32) -> Vec<f32> {
        let mut out = vec![0.0f32; (w * h) as usize];
        Rasterizer::new().rasterize(lines, FillRule::NonZero, IRect::new(0, 0, w, h), false, &mut |y, x, c| {
            for (i, v) in c.iter().enumerate() {
                out[(y * w + x) as usize + i] = *v;
            }
        });
        out
    }

    #[test]
    fn horizontal_line_of_width_two() {
        let style = StrokeStyle { width: 2.0, ..StrokeStyle::default() };
        let m = Matrix::IDENTITY;
        let mut s = Stroker::new(&style, &m);
        s.stroke(Polyline { points: &[(1.0, 5.0), (9.0, 5.0)], tangents: None, closed: false });
        let c = render(&s.lines, 10, 10);
        assert_eq!(c[4 * 10 + 5], 1.0);
        assert_eq!(c[5 * 10 + 5], 1.0);
        assert_eq!(c[3 * 10 + 5], 0.0);
        assert_eq!(c[5 * 10 + 0], 0.0);
    }

    #[test]
    fn closed_square_has_filled_miter_corners() {
        let style = StrokeStyle { width: 4.0, ..StrokeStyle::default() };
        let m = Matrix::IDENTITY;
        let mut s = Stroker::new(&style, &m);
        s.stroke(Polyline { points: &[(4.0, 4.0), (16.0, 4.0), (16.0, 16.0), (4.0, 16.0)], tangents: None, closed: true });
        let c = render(&s.lines, 20, 20);
        assert_eq!(c[2 * 20 + 2], 1.0, "outer corner");
        assert_eq!(c[10 * 20 + 10], 0.0, "inside");
    }

    #[test]
    fn dashes_leave_gaps() {
        let style = StrokeStyle { width: 2.0, dash: vec![4.0, 4.0], ..StrokeStyle::default() };
        let m = Matrix::IDENTITY;
        let mut s = Stroker::new(&style, &m);
        s.stroke(Polyline { points: &[(0.0, 5.0), (16.0, 5.0)], tangents: None, closed: false });
        let c = render(&s.lines, 16, 10);
        assert_eq!(c[5 * 16 + 1], 1.0);
        assert_eq!(c[5 * 16 + 5], 0.0);
        assert_eq!(c[5 * 16 + 9], 1.0);
    }

    #[test]
    fn degenerate_curves_are_pruned() {
        // WPT 2d.path.stroke.prune.curve.
        let style = StrokeStyle { width: 100.0, cap: LineCap::Round, join: LineJoin::Round, ..StrokeStyle::default() };
        let m = Matrix::IDENTITY;
        let mut p = crate::path::Path::new();
        p.move_to(&m, 50.0, 25.0);
        p.quad_to(&m, 50.0, 25.0, 50.0, 25.0);
        p.move_to(&m, 50.0, 25.0);
        p.cubic_to(&m, 50.0, 25.0, 50.0, 25.0, 50.0, 25.0);
        let mut s = Stroker::new(&style, &m);
        for sp in &p.subpaths {
            s.stroke(Polyline { points: &sp.points, tangents: None, closed: false });
        }
        assert!(s.lines.is_empty());
    }

    #[test]
    fn zero_length_subpaths_are_pruned() {
        let style = StrokeStyle { width: 10.0, cap: LineCap::Round, ..StrokeStyle::default() };
        let m = Matrix::IDENTITY;
        let mut s = Stroker::new(&style, &m);
        s.stroke(Polyline { points: &[(5.0, 5.0), (5.0, 5.0)], tangents: None, closed: false });
        assert!(s.lines.is_empty());
    }
}
