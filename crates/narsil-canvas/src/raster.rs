//! Anti-aliased scanline rasterizer.
//!
//! Exact-area coverage accumulation: every edge deposits the signed area it
//! sweeps into a per-row accumulation buffer, and a prefix sum over a row
//! yields the winding-weighted coverage of each pixel (the approach used by
//! font rasterizers such as FreeType's "gray" raster and font-rs). The work
//! happens in bands of rows so that memory stays bounded by the band width
//! instead of the path's bounding box; there are no per-pixel allocations.

use crate::geom::IRect;

/// Canvas fill rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}

/// A directed edge in device space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Line {
    #[must_use]
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Line {
        Line { x0: x0 as f32, y0: y0 as f32, x1: x1 as f32, y1: y1 as f32 }
    }
}

/// Rows processed together; the accumulation buffer is `BAND` rows high.
const BAND: usize = 16;

/// Reusable rasterizer scratch space.
#[derive(Default)]
pub struct Rasterizer {
    acc: Vec<f32>,
    cov: Vec<f32>,
    lines: Vec<Line>,
    active: Vec<usize>,
}

impl Rasterizer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Rasterizes `input` inside `region` (device pixels).
    ///
    /// `emit(y, x, coverage)` receives runs of coverage values in `0..=1`.
    /// With `all_rows` every row of `region` is reported in full (needed by
    /// composite operators that also affect pixels outside the shape);
    /// otherwise only the part of each row that may be covered.
    pub fn rasterize(
        &mut self,
        input: &[Line],
        rule: FillRule,
        region: IRect,
        all_rows: bool,
        emit: &mut dyn FnMut(i32, i32, &[f32]),
    ) {
        if region.is_empty() {
            return;
        }
        let rw = region.width() as usize;
        let rh = region.height() as usize;
        let stride = rw + 2;
        self.prepare_lines(input, region);
        let lines = std::mem::take(&mut self.lines);
        let mut active = std::mem::take(&mut self.active);
        active.clear();
        if self.acc.len() < stride * BAND {
            self.acc.resize(stride * BAND, 0.0);
        }
        if self.cov.len() < rw {
            self.cov.resize(rw, 0.0);
        }
        let mut next = 0usize;
        let mut band_y0 = 0usize;
        while band_y0 < rh {
            let band_y1 = (band_y0 + BAND).min(rh);
            let (fy0, fy1) = (band_y0 as f32, band_y1 as f32);
            while next < lines.len() && lines[next].y0.min(lines[next].y1) < fy1 {
                active.push(next);
                next += 1;
            }
            active.retain(|&i| lines[i].y0.max(lines[i].y1) > fy0);
            if active.is_empty() {
                if all_rows {
                    self.cov[..rw].fill(0.0);
                    for row in band_y0..band_y1 {
                        emit(region.y0 + row as i32, region.x0, &self.cov[..rw]);
                    }
                } else if next >= lines.len() {
                    break;
                }
                band_y0 = band_y1;
                continue;
            }
            let rows = band_y1 - band_y0;
            self.acc[..stride * rows].fill(0.0);
            for &i in &active {
                deposit(&mut self.acc, stride, rw as f32, &lines[i], fy0, fy1);
            }
            for row in 0..rows {
                let acc = &self.acc[row * stride..row * stride + rw];
                let cov = &mut self.cov[..rw];
                let mut sum = 0.0f32;
                let mut first = usize::MAX;
                let mut last = 0usize;
                match rule {
                    FillRule::NonZero => {
                        for (i, a) in acc.iter().enumerate() {
                            sum += a;
                            let c = sum.abs().min(1.0);
                            cov[i] = c;
                            if c > 1.0 / 1024.0 {
                                if first == usize::MAX {
                                    first = i;
                                }
                                last = i;
                            }
                        }
                    }
                    FillRule::EvenOdd => {
                        for (i, a) in acc.iter().enumerate() {
                            sum += a;
                            let m = sum.abs() % 2.0;
                            let c = if m > 1.0 { 2.0 - m } else { m };
                            cov[i] = c;
                            if c > 1.0 / 1024.0 {
                                if first == usize::MAX {
                                    first = i;
                                }
                                last = i;
                            }
                        }
                    }
                }
                let y = region.y0 + (band_y0 + row) as i32;
                if all_rows {
                    emit(y, region.x0, &self.cov[..rw]);
                } else if first != usize::MAX {
                    emit(y, region.x0 + first as i32, &self.cov[first..=last]);
                }
            }
            band_y0 = band_y1;
        }
        self.lines = lines;
        self.active = active;
    }

    /// Converts edges to region-relative coordinates, folds everything left
    /// of the region onto its left border (it still contributes winding),
    /// drops everything right of it, and sorts by top edge.
    fn prepare_lines(&mut self, input: &[Line], region: IRect) {
        self.lines.clear();
        let ox = region.x0 as f32;
        let oy = region.y0 as f32;
        let w = region.width() as f32;
        let h = region.height() as f32;
        for l in input {
            let (x0, y0, x1, y1) = (l.x0 - ox, l.y0 - oy, l.x1 - ox, l.y1 - oy);
            if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
                continue;
            }
            if y0 == y1 || (y0 <= 0.0 && y1 <= 0.0) || (y0 >= h && y1 >= h) {
                continue;
            }
            if x0 >= w && x1 >= w {
                continue;
            }
            // Clip vertically to the region so that the x math stays small.
            let (mut ax, mut ay, mut bx, mut by) = (x0, y0, x1, y1);
            let clip_y = |ax: &mut f32, ay: &mut f32, bx: f32, by: f32, edge: f32| {
                let t = (edge - *ay) / (by - *ay);
                *ax += (bx - *ax) * t;
                *ay = edge;
            };
            if ay < 0.0 {
                clip_y(&mut ax, &mut ay, bx, by, 0.0);
            } else if ay > h {
                clip_y(&mut ax, &mut ay, bx, by, h);
            }
            if by < 0.0 {
                clip_y(&mut bx, &mut by, ax, ay, 0.0);
            } else if by > h {
                clip_y(&mut bx, &mut by, ax, ay, h);
            }
            if ay == by {
                continue;
            }
            self.push_x_clipped(ax, ay, bx, by, w);
        }
        self.lines.sort_by(|a, b| a.y0.min(a.y1).total_cmp(&b.y0.min(b.y1)));
    }

    fn push_x_clipped(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32) {
        // Split at x = 0 and x = w.
        let mut pts: [(f32, f32); 4] = [(x0, y0); 4];
        let mut n = 1;
        let mut ts: [f32; 2] = [2.0, 2.0];
        let mut k = 0;
        for edge in [0.0f32, w] {
            if (x0 < edge && x1 > edge) || (x0 > edge && x1 < edge) {
                ts[k] = (edge - x0) / (x1 - x0);
                k += 1;
            }
        }
        if k == 2 && ts[0] > ts[1] {
            ts.swap(0, 1);
        }
        for &t in &ts[..k] {
            pts[n] = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
            n += 1;
        }
        pts[n] = (x1, y1);
        n += 1;
        for i in 0..n - 1 {
            let (ax, ay) = pts[i];
            let (bx, by) = pts[i + 1];
            if ay == by {
                continue;
            }
            let mid = (ax + bx) * 0.5;
            if mid >= w {
                continue;
            }
            if mid <= 0.0 {
                self.lines.push(Line { x0: 0.0, y0: ay, x1: 0.0, y1: by });
            } else {
                self.lines.push(Line { x0: ax.clamp(0.0, w), y0: ay, x1: bx.clamp(0.0, w), y1: by });
            }
        }
    }
}

/// Deposits the signed area of one edge into the rows `[by0, by1)` of the
/// band buffer.
fn deposit(acc: &mut [f32], stride: usize, w: f32, l: &Line, by0: f32, by1: f32) {
    let (dir, x0, y0, x1, y1) = if l.y0 < l.y1 { (1.0f32, l.x0, l.y0, l.x1, l.y1) } else { (-1.0f32, l.x1, l.y1, l.x0, l.y0) };
    if y1 <= by0 || y0 >= by1 {
        return;
    }
    let dxdy = (x1 - x0) / (y1 - y0);
    let ys = y0.max(by0);
    let ye = y1.min(by1);
    let mut x = x0 + (ys - y0) * dxdy;
    let mut y = ys;
    let mut row = (ys - by0).floor() as usize;
    while y < ye {
        let row_bottom = by0 + (row + 1) as f32;
        let yn = ye.min(row_bottom);
        let dy = yn - y;
        let xnext = (x + dxdy * dy).clamp(0.0, w);
        let d = dy * dir;
        let line = &mut acc[row * stride..row * stride + stride];
        let (xa, xb) = if x < xnext { (x, xnext) } else { (xnext, x) };
        let x0floor = xa.floor();
        let x0i = x0floor as i32;
        let x1ceil = xb.ceil();
        let x1i = x1ceil as i32;
        if x1i <= x0i + 1 {
            let xmf = 0.5 * (x + xnext) - x0floor;
            let i = x0i.max(0) as usize;
            line[i] += d - d * xmf;
            line[i + 1] += d * xmf;
        } else {
            let s = (xb - xa).recip();
            let x0f = xa - x0floor;
            let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
            let x1f = xb - x1ceil + 1.0;
            let am = 0.5 * s * x1f * x1f;
            let i0 = x0i.max(0) as usize;
            line[i0] += d * a0;
            if x1i == x0i + 2 {
                line[i0 + 1] += d * (1.0 - a0 - am);
            } else {
                let a1 = s * (1.5 - x0f);
                line[i0 + 1] += d * (a1 - a0);
                for xi in (x0i + 2)..(x1i - 1) {
                    line[xi as usize] += d * s;
                }
                let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                line[(x1i - 1) as usize] += d * (1.0 - a2 - am);
            }
            line[x1i as usize] += d * am;
        }
        x = xnext;
        y = yn;
        row += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poly(pts: &[(f64, f64)]) -> Vec<Line> {
        let mut v = Vec::new();
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            v.push(Line::new(a.0, a.1, b.0, b.1));
        }
        v
    }

    fn coverage(lines: &[Line], rule: FillRule, w: i32, h: i32) -> Vec<f32> {
        let mut out = vec![0.0f32; (w * h) as usize];
        let mut r = Rasterizer::new();
        r.rasterize(lines, rule, IRect::new(0, 0, w, h), false, &mut |y, x, c| {
            for (i, v) in c.iter().enumerate() {
                out[(y * w + x) as usize + i] = *v;
            }
        });
        out
    }

    #[test]
    fn integer_rectangle_is_exact() {
        let c = coverage(&poly(&[(2.0, 1.0), (6.0, 1.0), (6.0, 4.0), (2.0, 4.0)]), FillRule::NonZero, 8, 6);
        for y in 0..6 {
            for x in 0..8 {
                let inside = (2..6).contains(&x) && (1..4).contains(&y);
                assert_eq!(c[(y * 8 + x) as usize], if inside { 1.0 } else { 0.0 }, "{x},{y}");
            }
        }
    }

    #[test]
    fn half_pixel_edges_give_half_coverage() {
        let c = coverage(&poly(&[(1.5, 0.0), (3.0, 0.0), (3.0, 2.0), (1.5, 2.0)]), FillRule::NonZero, 4, 2);
        assert!((c[1] - 0.5).abs() < 1e-6);
        assert_eq!(c[2], 1.0);
        assert_eq!(c[3], 0.0);
    }

    #[test]
    fn triangle_area_matches() {
        let c = coverage(&poly(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]), FillRule::NonZero, 10, 10);
        let sum: f32 = c.iter().sum();
        assert!((sum - 50.0).abs() < 1e-3, "{sum}");
    }

    #[test]
    fn shapes_beyond_the_left_border_still_fill() {
        let c = coverage(&poly(&[(-20.0, 0.0), (3.0, 0.0), (3.0, 2.0), (-20.0, 2.0)]), FillRule::NonZero, 6, 2);
        assert_eq!(&c[0..6], &[1.0, 1.0, 1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn even_odd_cuts_holes() {
        let mut l = poly(&[(0.0, 0.0), (6.0, 0.0), (6.0, 6.0), (0.0, 6.0)]);
        l.extend(poly(&[(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0)]));
        let eo = coverage(&l, FillRule::EvenOdd, 6, 6);
        let nz = coverage(&l, FillRule::NonZero, 6, 6);
        assert_eq!(eo[3 * 6 + 3], 0.0);
        assert_eq!(nz[3 * 6 + 3], 1.0);
        assert_eq!(eo[0], 1.0);
    }

    #[test]
    fn diagonal_spanning_many_bands_and_columns() {
        let c = coverage(&poly(&[(0.0, 0.0), (100.0, 37.0), (0.0, 37.0)]), FillRule::NonZero, 100, 37);
        let sum: f32 = c.iter().sum();
        assert!((sum - 1850.0).abs() < 0.05, "{sum}");
    }
}
