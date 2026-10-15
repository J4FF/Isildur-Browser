//! Affine 2D geometry used by the canvas: matrices and points.

/// A 2D affine transform in the canvas convention `[a b c d e f]`:
///
/// ```text
/// x' = a*x + c*y + e
/// y' = b*x + d*y + f
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Matrix {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Matrix {
    pub const IDENTITY: Matrix = Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    #[must_use]
    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Matrix { a, b, c, d, e, f }
    }

    #[must_use]
    pub fn translate(x: f64, y: f64) -> Self {
        Matrix::new(1.0, 0.0, 0.0, 1.0, x, y)
    }

    #[must_use]
    pub fn scale(x: f64, y: f64) -> Self {
        Matrix::new(x, 0.0, 0.0, y, 0.0, 0.0)
    }

    #[must_use]
    pub fn rotate(angle: f64) -> Self {
        let (s, c) = angle.sin_cos();
        Matrix::new(c, s, -s, c, 0.0, 0.0)
    }

    #[must_use]
    pub fn as_array(&self) -> [f64; 6] {
        [self.a, self.b, self.c, self.d, self.e, self.f]
    }

    #[inline]
    #[must_use]
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    /// Applies the linear part only (no translation).
    #[inline]
    #[must_use]
    pub fn apply_vector(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y, self.b * x + self.d * y)
    }

    /// `self * other`: the result maps a point first through `other`, then
    /// through `self` (the canvas `transform()` composition).
    #[must_use]
    pub fn multiply(&self, o: &Matrix) -> Matrix {
        Matrix {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }

    #[must_use]
    pub fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.as_array().iter().all(|v| v.is_finite())
    }

    #[must_use]
    pub fn is_invertible(&self) -> bool {
        let det = self.determinant();
        det != 0.0 && det.is_finite() && self.is_finite()
    }

    #[must_use]
    pub fn invert(&self) -> Option<Matrix> {
        let det = self.determinant();
        if det == 0.0 || !det.is_finite() || !self.is_finite() {
            return None;
        }
        let inv = 1.0 / det;
        let m = Matrix {
            a: self.d * inv,
            b: -self.b * inv,
            c: -self.c * inv,
            d: self.a * inv,
            e: (self.c * self.f - self.d * self.e) * inv,
            f: (self.b * self.e - self.a * self.f) * inv,
        };
        m.is_finite().then_some(m)
    }

    /// The largest factor by which the transform stretches a unit vector
    /// (used to pick curve flattening densities in device space).
    #[must_use]
    pub fn max_scale(&self) -> f64 {
        // Singular values of the linear part.
        let (a, b, c, d) = (self.a, self.b, self.c, self.d);
        let s1 = a * a + b * b + c * c + d * d;
        let s2 = ((a * a + b * b - c * c - d * d).powi(2) + 4.0 * (a * c + b * d).powi(2)).sqrt();
        let v = ((s1 + s2) / 2.0).sqrt();
        if v.is_finite() { v } else { 1.0 }
    }

    #[must_use]
    pub fn is_identity(&self) -> bool {
        *self == Matrix::IDENTITY
    }

    /// True when the transform maps axis-aligned rectangles to axis-aligned
    /// rectangles (no rotation or skew).
    #[must_use]
    pub fn is_axis_aligned(&self) -> bool {
        self.b == 0.0 && self.c == 0.0
    }
}

/// An axis-aligned integer pixel rectangle `[x0, x1) x [y0, y1)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct IRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl IRect {
    #[must_use]
    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        IRect { x0, y0, x1, y1 }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    #[must_use]
    pub fn width(&self) -> i32 {
        (self.x1 - self.x0).max(0)
    }

    #[must_use]
    pub fn height(&self) -> i32 {
        (self.y1 - self.y0).max(0)
    }

    #[must_use]
    pub fn intersect(&self, o: &IRect) -> IRect {
        IRect {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        }
    }

    #[must_use]
    pub fn union(&self, o: &IRect) -> IRect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        IRect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    /// The smallest pixel rectangle covering the float bounds, clamped to a
    /// sane range so that huge coordinates cannot overflow.
    #[must_use]
    pub fn from_bounds(x0: f64, y0: f64, x1: f64, y1: f64) -> IRect {
        let clamp = |v: f64| v.clamp(-1.0e8, 1.0e8);
        if !(x0 <= x1 && y0 <= y1) {
            return IRect::default();
        }
        IRect {
            x0: clamp(x0).floor() as i32,
            y0: clamp(y0).floor() as i32,
            x1: clamp(x1).ceil() as i32,
            y1: clamp(y1).ceil() as i32,
        }
    }

    #[must_use]
    pub fn outset(&self, n: i32) -> IRect {
        IRect { x0: self.x0.saturating_sub(n), y0: self.y0.saturating_sub(n), x1: self.x1.saturating_add(n), y1: self.y1.saturating_add(n) }
    }

    #[must_use]
    pub fn offset(&self, dx: i32, dy: i32) -> IRect {
        IRect { x0: self.x0.saturating_add(dx), y0: self.y0.saturating_add(dy), x1: self.x1.saturating_add(dx), y1: self.y1.saturating_add(dy) }
    }
}

/// Float bounding box accumulator.
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Default for Bounds {
    fn default() -> Self {
        Bounds { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY }
    }
}

impl Bounds {
    #[inline]
    pub fn add(&mut self, x: f64, y: f64) {
        self.x0 = self.x0.min(x);
        self.y0 = self.y0.min(y);
        self.x1 = self.x1.max(x);
        self.y1 = self.y1.max(y);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        !(self.x0 <= self.x1 && self.y0 <= self.y1)
    }

    #[must_use]
    pub fn to_irect(&self) -> IRect {
        if self.is_empty() {
            return IRect::default();
        }
        IRect::from_bounds(self.x0, self.y0, self.x1, self.y1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiply_matches_canvas_transform_order() {
        let m = Matrix::translate(10.0, 0.0).multiply(&Matrix::scale(2.0, 2.0));
        assert_eq!(m.apply(1.0, 1.0), (12.0, 2.0));
    }

    #[test]
    fn invert_round_trips() {
        let m = Matrix::new(2.0, 1.0, -1.0, 3.0, 5.0, 7.0);
        let i = m.invert().unwrap();
        let (x, y) = m.apply(3.0, 4.0);
        let (u, v) = i.apply(x, y);
        assert!((u - 3.0).abs() < 1e-9 && (v - 4.0).abs() < 1e-9);
        assert!(Matrix::new(1.0, 2.0, 2.0, 4.0, 0.0, 0.0).invert().is_none());
    }

    #[test]
    fn max_scale_of_rotation_is_one() {
        assert!((Matrix::rotate(0.7).max_scale() - 1.0).abs() < 1e-9);
        assert!((Matrix::scale(3.0, 0.5).max_scale() - 3.0).abs() < 1e-9);
    }
}
