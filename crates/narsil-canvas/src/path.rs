//! Canvas paths (HTML §4.12.5.1.10 "Building paths").
//!
//! A [`Path`] is a list of subpaths whose points are already in device space
//! (the current transform is applied when a point is added, as the spec
//! requires). Curves are flattened at that moment with a device-space
//! tolerance, so later fills and strokes only see polylines.

use crate::geom::{Bounds, Matrix};
use crate::raster::Line;

/// Flattening tolerance in device pixels.
pub const TOLERANCE: f64 = 0.1;
/// Upper bound for the number of points in one path (memory guard).
pub const MAX_POINTS: usize = 4_000_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubPath {
    pub points: Vec<(f64, f64)>,
    /// Per point: the exact tangent direction (device space, radians) for
    /// the interior points of a flattened curve, NaN for corners. Strokes
    /// sweep along these normals instead of joining the chords.
    pub tangent: Vec<f32>,
    pub closed: bool,
}

impl SubPath {
    fn single(p: (f64, f64)) -> SubPath {
        SubPath { points: vec![bounded(p)], tangent: vec![f32::NAN], closed: false }
    }
}

/// Device coordinates that overflowed (a `Number.MAX_VALUE` scale) are
/// kept huge but finite, so the path still covers what it should
/// (WPT 2d.path.isPointInPath.basic).
#[inline]
fn bounded(p: (f64, f64)) -> (f64, f64) {
    const LIMIT: f64 = 1.0e30;
    let b = |v: f64| if v.is_infinite() { LIMIT.copysign(v) } else { v };
    (b(p.0), b(p.1))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub subpaths: Vec<SubPath>,
    points: usize,
}

/// Errors the path methods can raise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathError {
    /// `IndexSizeError`: a negative radius.
    NegativeRadius,
}

impl Path {
    #[must_use]
    pub fn new() -> Path {
        Path::default()
    }

    pub fn clear(&mut self) {
        self.subpaths.clear();
        self.points = 0;
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.subpaths.is_empty()
    }

    #[must_use]
    pub fn point_count(&self) -> usize {
        self.points
    }

    /// The last point of the last subpath (device space).
    #[must_use]
    pub fn last_point(&self) -> Option<(f64, f64)> {
        self.subpaths.last().and_then(|s| s.points.last().copied())
    }

    fn push_point(&mut self, p: (f64, f64)) {
        self.push_point_t(p, None);
    }

    /// Adds a point; `tangent` is the curve direction at an interior curve
    /// point (device space), `None` for a corner.
    fn push_point_t(&mut self, p: (f64, f64), tangent: Option<(f64, f64)>) {
        if self.points >= MAX_POINTS {
            return;
        }
        if let Some(s) = self.subpaths.last_mut() {
            s.points.push(bounded(p));
            s.tangent.push(match tangent {
                Some((x, y)) if x != 0.0 || y != 0.0 => y.atan2(x) as f32,
                _ => f32::NAN,
            });
            self.points += 1;
        }
    }

    /// Curve parameters for flattening into `n` segments, refined next to
    /// both ends so that the first and last chords follow the true end
    /// tangents (strokes take their caps and joins from them).
    fn curve_params(n: usize) -> Vec<f64> {
        let mut ts = Vec::with_capacity(n + 2);
        let edge = 0.02 / n as f64;
        ts.push(edge);
        for i in 1..n {
            ts.push(i as f64 / n as f64);
        }
        ts.push(1.0 - edge);
        ts.push(1.0);
        ts
    }

    fn new_subpath(&mut self, p: (f64, f64)) {
        if self.points >= MAX_POINTS {
            return;
        }
        // A lone point (e.g. the subpath after closePath) is replaced by a
        // new subpath instead of piling up.
        if let Some(s) = self.subpaths.last_mut() {
            if s.points.len() == 1 && !s.closed {
                s.points[0] = bounded(p);
                s.tangent[0] = f32::NAN;
                return;
            }
        }
        self.subpaths.push(SubPath::single(p));
        self.points += 1;
    }

    /// "Ensure there is a subpath for (x, y)".
    fn ensure_subpath(&mut self, m: &Matrix, x: f64, y: f64) {
        if self.subpaths.is_empty() {
            self.new_subpath(m.apply(x, y));
        }
    }

    pub fn move_to(&mut self, m: &Matrix, x: f64, y: f64) {
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        self.new_subpath(m.apply(x, y));
    }

    pub fn line_to(&mut self, m: &Matrix, x: f64, y: f64) {
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        if self.subpaths.is_empty() {
            self.new_subpath(m.apply(x, y));
        } else {
            self.push_point(m.apply(x, y));
        }
    }

    pub fn quad_to(&mut self, m: &Matrix, cx: f64, cy: f64, x: f64, y: f64) {
        if ![cx, cy, x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.ensure_subpath(m, cx, cy);
        let p0 = self.last_point().unwrap_or_else(|| m.apply(cx, cy));
        let p1 = m.apply(cx, cy);
        let p2 = m.apply(x, y);
        let dd = ((p0.0 - 2.0 * p1.0 + p2.0).powi(2) + (p0.1 - 2.0 * p1.1 + p2.1).powi(2)).sqrt();
        let n = ((dd / (4.0 * TOLERANCE)).sqrt().ceil() as usize).clamp(1, 1000);
        let ts = Self::curve_params(n);
        let last = ts.len() - 1;
        for (i, t) in ts.into_iter().enumerate() {
            let mt = 1.0 - t;
            let a = mt * mt;
            let b = 2.0 * mt * t;
            let c = t * t;
            let d = (2.0 * mt * (p1.0 - p0.0) + 2.0 * t * (p2.0 - p1.0), 2.0 * mt * (p1.1 - p0.1) + 2.0 * t * (p2.1 - p1.1));
            self.push_point_t((a * p0.0 + b * p1.0 + c * p2.0, a * p0.1 + b * p1.1 + c * p2.1), (i < last).then_some(d));
        }
    }

    pub fn cubic_to(&mut self, m: &Matrix, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        if ![c1x, c1y, c2x, c2y, x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.ensure_subpath(m, c1x, c1y);
        let p0 = self.last_point().unwrap_or_else(|| m.apply(c1x, c1y));
        let p1 = m.apply(c1x, c1y);
        let p2 = m.apply(c2x, c2y);
        let p3 = m.apply(x, y);
        let d1 = ((p0.0 - 2.0 * p1.0 + p2.0).powi(2) + (p0.1 - 2.0 * p1.1 + p2.1).powi(2)).sqrt();
        let d2 = ((p1.0 - 2.0 * p2.0 + p3.0).powi(2) + (p1.1 - 2.0 * p2.1 + p3.1).powi(2)).sqrt();
        let dd = d1.max(d2);
        let n = ((0.75 * dd / TOLERANCE).sqrt().ceil() as usize).clamp(1, 1000);
        let ts = Self::curve_params(n);
        let last = ts.len() - 1;
        for (i, t) in ts.into_iter().enumerate() {
            let mt = 1.0 - t;
            let a = mt * mt * mt;
            let b = 3.0 * mt * mt * t;
            let c = 3.0 * mt * t * t;
            let d = t * t * t;
            let dt = (
                3.0 * mt * mt * (p1.0 - p0.0) + 6.0 * mt * t * (p2.0 - p1.0) + 3.0 * t * t * (p3.0 - p2.0),
                3.0 * mt * mt * (p1.1 - p0.1) + 6.0 * mt * t * (p2.1 - p1.1) + 3.0 * t * t * (p3.1 - p2.1),
            );
            self.push_point_t(
                (a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1),
                (i < last).then_some(dt),
            );
        }
    }

    /// `arcTo(x1, y1, x2, y2, radius)`.
    pub fn arc_to(&mut self, m: &Matrix, x1: f64, y1: f64, x2: f64, y2: f64, r: f64) -> Result<(), PathError> {
        if ![x1, y1, x2, y2, r].iter().all(|v| v.is_finite()) {
            return Ok(());
        }
        self.ensure_subpath(m, x1, y1);
        if r < 0.0 {
            return Err(PathError::NegativeRadius);
        }
        let Some(inv) = m.invert() else {
            self.line_to(m, x1, y1);
            return Ok(());
        };
        let last = self.last_point().unwrap_or_else(|| m.apply(x1, y1));
        let (x0, y0) = inv.apply(last.0, last.1);
        let same = |ax: f64, ay: f64, bx: f64, by: f64| (ax - bx).abs() < 1e-9 && (ay - by).abs() < 1e-9;
        if same(x0, y0, x1, y1) || same(x1, y1, x2, y2) || r == 0.0 {
            self.line_to(m, x1, y1);
            return Ok(());
        }
        let (v1x, v1y) = (x0 - x1, y0 - y1);
        let (v2x, v2y) = (x2 - x1, y2 - y1);
        let l1 = (v1x * v1x + v1y * v1y).sqrt();
        let l2 = (v2x * v2x + v2y * v2y).sqrt();
        let cross = v1x * v2y - v1y * v2x;
        if cross.abs() <= 1e-12 * l1 * l2 {
            self.line_to(m, x1, y1);
            return Ok(());
        }
        let (u1x, u1y) = (v1x / l1, v1y / l1);
        let (u2x, u2y) = (v2x / l2, v2y / l2);
        let cos = (u1x * u2x + u1y * u2y).clamp(-1.0, 1.0);
        let theta = cos.acos();
        let dist = r / (theta / 2.0).tan();
        let (t1x, t1y) = (x1 + u1x * dist, y1 + u1y * dist);
        let (t2x, t2y) = (x1 + u2x * dist, y1 + u2y * dist);
        let (bx, by) = (u1x + u2x, u1y + u2y);
        let bl = (bx * bx + by * by).sqrt();
        let cd = r / (theta / 2.0).sin();
        let (cx, cy) = (x1 + bx / bl * cd, y1 + by / bl * cd);
        let a1 = (t1y - cy).atan2(t1x - cx);
        let a2 = (t2y - cy).atan2(t2x - cx);
        self.line_to(m, t1x, t1y);
        let ccw = cross > 0.0;
        self.arc_points(m, cx, cy, r, r, 0.0, a1, short_sweep(a1, a2, ccw));
        Ok(())
    }

    /// `ellipse(...)`; `arc()` is the circular case.
    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(
        &mut self,
        m: &Matrix,
        x: f64,
        y: f64,
        rx: f64,
        ry: f64,
        rotation: f64,
        start: f64,
        end: f64,
        ccw: bool,
    ) -> Result<(), PathError> {
        if ![x, y, rx, ry, rotation, start, end].iter().all(|v| v.is_finite()) {
            return Ok(());
        }
        if rx < 0.0 || ry < 0.0 {
            return Err(PathError::NegativeRadius);
        }
        let sweep = arc_sweep(start, end, ccw);
        let (sx, sy) = ellipse_point(x, y, rx, ry, rotation, start);
        if self.subpaths.is_empty() {
            self.new_subpath(m.apply(sx, sy));
        } else {
            self.push_point(m.apply(sx, sy));
        }
        self.arc_points(m, x, y, rx, ry, rotation, start, sweep);
        Ok(())
    }

    /// Adds the points of an elliptical arc after its start point.
    #[allow(clippy::too_many_arguments)]
    fn arc_points(&mut self, m: &Matrix, x: f64, y: f64, rx: f64, ry: f64, rotation: f64, start: f64, sweep: f64) {
        if sweep == 0.0 {
            return;
        }
        let r_dev = rx.max(ry) * m.max_scale();
        let step = if r_dev > TOLERANCE { 2.0 * (1.0 - TOLERANCE / r_dev).clamp(-1.0, 1.0).acos() } else { sweep.abs() };
        let n = ((sweep.abs() / step.max(1e-6)).ceil() as usize).clamp(1, 8192);
        let ts = Self::curve_params(n);
        let last = ts.len() - 1;
        for (i, t) in ts.into_iter().enumerate() {
            let a = start + sweep * t;
            let (px, py) = ellipse_point(x, y, rx, ry, rotation, a);
            // d/da of the ellipse point, oriented along the sweep.
            let (s, c) = a.sin_cos();
            let (rs, rc) = rotation.sin_cos();
            let (ex, ey) = (-rx * s, ry * c);
            let k = sweep.signum();
            let d = m.apply_vector((ex * rc - ey * rs) * k, (ex * rs + ey * rc) * k);
            self.push_point_t(m.apply(px, py), (i < last).then_some(d));
        }
    }

    pub fn rect(&mut self, m: &Matrix, x: f64, y: f64, w: f64, h: f64) {
        if ![x, y, w, h].iter().all(|v| v.is_finite()) {
            return;
        }
        if self.points + 5 > MAX_POINTS {
            return;
        }
        self.subpaths.push(SubPath {
            points: vec![bounded(m.apply(x, y)), bounded(m.apply(x + w, y)), bounded(m.apply(x + w, y + h)), bounded(m.apply(x, y + h))],
            tangent: vec![f32::NAN; 4],
            closed: true,
        });
        self.points += 4;
        self.subpaths.push(SubPath::single(m.apply(x, y)));
        self.points += 1;
    }

    /// `roundRect()` with already validated, normalized corner radii in the
    /// order upper-left, upper-right, lower-right, lower-left (`(rx, ry)`).
    pub fn round_rect(&mut self, m: &Matrix, x: f64, y: f64, w: f64, h: f64, radii: [(f64, f64); 4]) {
        if ![x, y, w, h].iter().all(|v| v.is_finite()) {
            return;
        }
        let [mut ul, mut ur, mut lr, mut ll] = radii;
        // Negative sizes mirror the rectangle; the corners swap with it, and
        // one mirrored axis reverses the winding (WPT 2d.path.roundrect.winding).
        let mirrored = (w < 0.0) != (h < 0.0);
        let (mut x, mut y, mut w, mut h) = (x, y, w, h);
        if w < 0.0 {
            x += w;
            w = -w;
            std::mem::swap(&mut ul, &mut ur);
            std::mem::swap(&mut ll, &mut lr);
        }
        if h < 0.0 {
            y += h;
            h = -h;
            std::mem::swap(&mut ul, &mut ll);
            std::mem::swap(&mut ur, &mut lr);
        }
        let top = ul.0 + ur.0;
        let right = ur.1 + lr.1;
        let bottom = lr.0 + ll.0;
        let left = ul.1 + ll.1;
        let mut scale = f64::INFINITY;
        for (len, sum) in [(w, top), (h, right), (w, bottom), (h, left)] {
            if sum > 0.0 {
                scale = scale.min(len / sum);
            }
        }
        if scale < 1.0 {
            for r in [&mut ul, &mut ur, &mut lr, &mut ll] {
                r.0 *= scale;
                r.1 *= scale;
            }
        }
        use std::f64::consts::{FRAC_PI_2, PI};
        self.new_subpath(m.apply(x + ul.0, y));
        self.push_point(m.apply(x + w - ur.0, y));
        self.arc_points(m, x + w - ur.0, y + ur.1, ur.0, ur.1, 0.0, -FRAC_PI_2, FRAC_PI_2);
        self.push_point(m.apply(x + w, y + h - lr.1));
        self.arc_points(m, x + w - lr.0, y + h - lr.1, lr.0, lr.1, 0.0, 0.0, FRAC_PI_2);
        self.push_point(m.apply(x + ll.0, y + h));
        self.arc_points(m, x + ll.0, y + h - ll.1, ll.0, ll.1, 0.0, FRAC_PI_2, FRAC_PI_2);
        self.push_point(m.apply(x, y + ul.1));
        self.arc_points(m, x + ul.0, y + ul.1, ul.0, ul.1, 0.0, PI, FRAC_PI_2);
        if let Some(s) = self.subpaths.last_mut() {
            s.closed = true;
            if mirrored {
                s.points.reverse();
                s.tangent.reverse();
                for t in &mut s.tangent {
                    if !t.is_nan() {
                        *t += std::f32::consts::PI;
                    }
                }
            }
        }
        self.subpaths.push(SubPath::single(m.apply(x, y)));
        self.points += 1;
    }

    pub fn close(&mut self) {
        let Some(s) = self.subpaths.last_mut() else { return };
        s.closed = true;
        let first = s.points[0];
        self.subpaths.push(SubPath::single(first));
        self.points += 1;
    }

    /// Appends the subpaths of `other` (device space already), then starts
    /// a new subpath at the previous last point (`Path2D.addPath`).
    pub fn append(&mut self, other: &Path) {
        let last = self.last_point();
        for s in &other.subpaths {
            if self.points + s.points.len() > MAX_POINTS {
                break;
            }
            self.points += s.points.len();
            self.subpaths.push(s.clone());
        }
        if let Some(p) = last {
            self.subpaths.push(SubPath::single(p));
            self.points += 1;
        }
    }

    /// The fill edges: every subpath is implicitly closed.
    #[must_use]
    pub fn fill_lines(&self) -> Vec<Line> {
        let mut out = Vec::with_capacity(self.points + self.subpaths.len());
        for s in &self.subpaths {
            let p = &s.points;
            if p.len() < 2 {
                continue;
            }
            for i in 0..p.len() {
                let a = p[i];
                let b = p[(i + 1) % p.len()];
                if a.1 != b.1 {
                    out.push(Line::new(a.0, a.1, b.0, b.1));
                }
            }
        }
        out
    }

    #[must_use]
    pub fn bounds(&self) -> Bounds {
        let mut b = Bounds::default();
        for s in &self.subpaths {
            for p in &s.points {
                b.add(p.0, p.1);
            }
        }
        b
    }

    /// Maps every point through `m` (used to move a path between spaces).
    #[must_use]
    pub fn transformed(&self, m: &Matrix) -> Path {
        Path {
            subpaths: self
                .subpaths
                .iter()
                .map(|s| SubPath { points: s.points.iter().map(|p| m.apply(p.0, p.1)).collect(), tangent: s.tangent.iter().map(|a| transform_angle(m, *a)).collect(), closed: s.closed })
                .collect(),
            points: self.points,
        }
    }

    /// Point-in-path test for the fill (the boundary counts as inside).
    #[must_use]
    pub fn contains(&self, x: f64, y: f64, even_odd: bool) -> bool {
        let mut winding = 0i32;
        for s in &self.subpaths {
            let p = &s.points;
            if p.len() < 2 {
                continue;
            }
            for i in 0..p.len() {
                let a = p[i];
                let b = p[(i + 1) % p.len()];
                if on_segment(a, b, x, y) {
                    return true;
                }
                if (a.1 <= y && b.1 > y) || (b.1 <= y && a.1 > y) {
                    let t = (y - a.1) / (b.1 - a.1);
                    let cx = a.0 + t * (b.0 - a.0);
                    if cx > x {
                        winding += if b.1 > a.1 { 1 } else { -1 };
                    }
                }
            }
        }
        if even_odd { winding % 2 != 0 } else { winding != 0 }
    }
}

fn on_segment(a: (f64, f64), b: (f64, f64), x: f64, y: f64) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx * dx + dy * dy;
    let eps = 1e-6;
    if len == 0.0 {
        return (x - a.0).abs() < eps && (y - a.1).abs() < eps;
    }
    let t = ((x - a.0) * dx + (y - a.1) * dy) / len;
    if !(-1e-9..=1.0 + 1e-9).contains(&t) {
        return false;
    }
    let (px, py) = (a.0 + t * dx, a.1 + t * dy);
    (px - x).abs() < eps && (py - y).abs() < eps
}

/// Maps a tangent angle through the linear part of `m` (NaN stays NaN).
fn transform_angle(m: &Matrix, a: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    let (s, c) = (a as f64).sin_cos();
    let (x, y) = m.apply_vector(c, s);
    if x == 0.0 && y == 0.0 { f32::NAN } else { y.atan2(x) as f32 }
}

#[must_use]
pub fn ellipse_point(x: f64, y: f64, rx: f64, ry: f64, rotation: f64, angle: f64) -> (f64, f64) {
    let (s, c) = angle.sin_cos();
    let (rs, rc) = rotation.sin_cos();
    let (ex, ey) = (rx * c, ry * s);
    (x + ex * rc - ey * rs, y + ex * rs + ey * rc)
}

/// The signed sweep of an arc per the spec's angle rules. Where the spec is
/// silent (`arc(x, y, r, 0, 2 * PI, true)`: start and end point coincide),
/// the whole circle is drawn, as every browser does.
#[must_use]
pub fn arc_sweep(start: f64, end: f64, ccw: bool) -> f64 {
    let tau = std::f64::consts::TAU;
    if !ccw && end - start >= tau {
        tau
    } else if ccw && start - end >= tau {
        -tau
    } else if !ccw && start > end {
        tau - (start - end) % tau
    } else if ccw && start < end {
        -(tau - (end - start) % tau)
    } else {
        end - start
    }
}

fn short_sweep(a1: f64, a2: f64, ccw: bool) -> f64 {
    let tau = std::f64::consts::TAU;
    if ccw { -(a1 - a2).rem_euclid(tau) } else { (a2 - a1).rem_euclid(tau) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrored_round_rects_wind_the_other_way() {
        // WPT 2d.path.roundrect.winding: one negative size reverses the winding.
        let m = Matrix::IDENTITY;
        let mut p = Path::new();
        p.round_rect(&m, 0.0, 0.0, 50.0, 50.0, [(0.0, 0.0); 4]);
        p.round_rect(&m, 0.0, 25.0, 100.0, -25.0, [(0.0, 0.0); 4]);
        assert!(!p.contains(25.0, 12.0, false));
        assert!(p.contains(75.0, 12.0, false));
        assert!(p.contains(25.0, 37.0, false));
    }

    #[test]
    fn line_to_without_subpath_acts_as_move_to() {
        let mut p = Path::new();
        p.line_to(&Matrix::IDENTITY, 5.0, 5.0);
        assert_eq!(p.subpaths.len(), 1);
        assert_eq!(p.subpaths[0].points, vec![(5.0, 5.0)]);
    }

    #[test]
    fn close_path_starts_a_new_subpath_at_the_first_point() {
        let mut p = Path::new();
        let m = Matrix::IDENTITY;
        p.move_to(&m, 1.0, 2.0);
        p.line_to(&m, 5.0, 2.0);
        p.close();
        assert_eq!(p.last_point(), Some((1.0, 2.0)));
        assert!(p.subpaths[0].closed);
    }

    #[test]
    fn transform_is_applied_when_points_are_added() {
        let mut p = Path::new();
        p.move_to(&Matrix::IDENTITY, 0.0, 0.0);
        p.line_to(&Matrix::scale(2.0, 2.0), 10.0, 10.0);
        assert_eq!(p.last_point(), Some((20.0, 20.0)));
    }

    #[test]
    fn full_circle_contains_its_center() {
        let mut p = Path::new();
        p.ellipse(&Matrix::IDENTITY, 50.0, 25.0, 20.0, 20.0, 0.0, 0.0, std::f64::consts::TAU, false).unwrap();
        assert!(p.contains(50.0, 25.0, false));
        assert!(!p.contains(75.0, 25.0, false));
        assert!(p.ellipse(&Matrix::IDENTITY, 0.0, 0.0, -1.0, 1.0, 0.0, 0.0, 1.0, false).is_err());
    }

    #[test]
    fn arc_sweep_follows_the_spec() {
        use std::f64::consts::PI;
        assert!((arc_sweep(0.0, PI / 2.0, false) - PI / 2.0).abs() < 1e-12);
        assert!((arc_sweep(0.0, -PI / 2.0, false) - 1.5 * PI).abs() < 1e-12);
        assert!((arc_sweep(0.0, 7.0, false) - std::f64::consts::TAU).abs() < 1e-12);
        assert!((arc_sweep(0.0, PI / 2.0, true) + 1.5 * PI).abs() < 1e-12);
        assert_eq!(arc_sweep(1.0, 1.0, true), 0.0);
        assert!((arc_sweep(0.0, 2.0 * PI, true) + 2.0 * PI).abs() < 1e-12);
    }

    #[test]
    fn arc_to_rounds_a_corner() {
        let mut p = Path::new();
        let m = Matrix::IDENTITY;
        p.move_to(&m, 0.0, 0.0);
        p.arc_to(&m, 10.0, 0.0, 10.0, 10.0, 5.0).unwrap();
        let last = p.last_point().unwrap();
        assert!((last.0 - 10.0).abs() < 1e-9 && (last.1 - 5.0).abs() < 1e-9, "{last:?}");
    }
}
