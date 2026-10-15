//! Paint sources: solid colors, gradients, patterns and image sampling.

use crate::geom::Matrix;
use std::sync::Arc;

/// A non-premultiplied sRGB color. Channels are gamma-encoded and may lie
/// outside `0..=1` for wide-gamut colors (extended sRGB); drawing clamps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
    /// The CSS form of colors outside the legacy sRGB syntaxes
    /// (`color()`, `lab()`, `color-mix()`, ...), kept for serialization.
    pub css: Option<CssForm>,
}

/// Color spaces of CSS colors (CSS Color 4 section 10 and the polar forms).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssSpace {
    Srgb,
    SrgbLinear,
    DisplayP3,
    A98Rgb,
    ProphotoRgb,
    Rec2020,
    XyzD50,
    XyzD65,
    Lab,
    Lch,
    Oklab,
    Oklch,
    Hsl,
    Hwb,
}

/// A CSS color's own channels: `none` bits 0..=2 mark missing channels,
/// bit 3 a missing alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CssForm {
    pub space: CssSpace,
    pub c: [f32; 3],
    pub none: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Color = Color::rgba(0.0, 0.0, 0.0, 1.0);

    /// A legacy sRGB color from channels in `0..=1`.
    #[must_use]
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a, css: None }
    }

    #[must_use]
    pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color::rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0)
    }

    /// Whether the color uses a legacy sRGB syntax (gradients between
    /// legacy colors interpolate in sRGB, others in OKLab).
    #[must_use]
    pub fn is_legacy(&self) -> bool {
        self.css.is_none()
    }

    /// Channels quantized to 8 bits, as the canvas stores and serializes
    /// colors.
    #[must_use]
    pub fn to_rgba8(&self) -> [u8; 4] {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [q(self.r), q(self.g), q(self.b), q(self.a)]
    }

    /// The premultiplied color in a canvas storage space, quantized to
    /// 8-bit channels like the bitmap.
    #[must_use]
    pub fn premultiplied_in(&self, space: crate::colorspace::ColorSpace) -> [f32; 4] {
        if space == crate::colorspace::ColorSpace::Srgb {
            return self.premultiplied();
        }
        let c = crate::colorspace::from_extended_srgb([self.r as f64, self.g as f64, self.b as f64], space);
        let q = |v: f64| (v * 255.0).round() / 255.0;
        let a = (self.a.clamp(0.0, 1.0) * 255.0).round() / 255.0;
        [q(c[0]) as f32 * a, q(c[1]) as f32 * a, q(c[2]) as f32 * a, a]
    }

    /// The premultiplied color after quantizing to 8-bit channels.
    #[must_use]
    pub fn premultiplied(&self) -> [f32; 4] {
        let [r, g, b, a] = self.to_rgba8();
        let a = a as f32 / 255.0;
        [r as f32 / 255.0 * a, g as f32 / 255.0 * a, b as f32 / 255.0 * a, a]
    }

    /// HTML "serialization of a color" for canvas attributes: `#rrggbb`
    /// for opaque legacy colors, `rgba(r, g, b, a)` for translucent ones,
    /// and the CSS serialization for other color forms.
    #[must_use]
    pub fn serialize(&self) -> String {
        if let Some(f) = &self.css {
            return crate::css_color::serialize_css(f, self.a.clamp(0.0, 1.0));
        }
        let [r, g, b, _] = self.to_rgba8();
        let a = self.a.clamp(0.0, 1.0);
        if a >= 1.0 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("rgba({r}, {g}, {b}, {})", format_alpha(a))
        }
    }
}

/// Formats an alpha value the way browsers do: the shortest decimal that
/// round-trips the 8-bit alpha (at most three fraction digits), `0` for
/// fully transparent.
#[must_use]
pub fn format_alpha(a: f32) -> String {
    if a <= 0.0 {
        return "0".into();
    }
    let a8 = (a * 255.0).round();
    // Prefer two decimals when they map back to the same 8-bit value.
    for digits in 1..=3 {
        let p = 10f32.powi(digits);
        let v = (a * p).round() / p;
        if digits == 3 || ((v * 255.0).round() == a8 && (v - a).abs() < 0.5 / 255.0 + 1e-6) {
            let mut s = format!("{v:.*}", digits as usize);
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
            return s;
        }
    }
    format!("{a}")
}

/// A gradient color stop (non-premultiplied color).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorStop {
    pub offset: f32,
    pub color: Color,
}

/// Gradient geometry in user space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    Linear { x0: f64, y0: f64, x1: f64, y1: f64 },
    Radial { x0: f64, y0: f64, r0: f64, x1: f64, y1: f64, r1: f64 },
    Conic { angle: f64, x: f64, y: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    /// Sorted by offset; stops with equal offsets keep insertion order.
    pub stops: Vec<ColorStop>,
    /// Premultiplied OKLab stops when the gradient interpolates in OKLab
    /// (some stop is not a legacy sRGB color, as with CSS gradients).
    pub oklab: Option<Vec<[f32; 4]>>,
    /// Storage space of the canvas it paints into.
    pub output: crate::colorspace::ColorSpace,
    /// Gradient space to user space (SVG `gradientUnits` and
    /// `gradientTransform`); identity for canvas gradients.
    pub transform: Matrix,
    /// What happens beyond the end stops (SVG `spreadMethod`).
    pub spread: Spread,
    /// Premultiplied linear-light sRGB stops when the gradient
    /// interpolates in linear RGB (SVG `color-interpolation: linearRGB`).
    pub linear: Option<Vec<[f32; 4]>>,
}

/// SVG `spreadMethod`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Spread {
    #[default]
    Pad,
    Reflect,
    Repeat,
}

impl Gradient {
    #[must_use]
    pub fn new(kind: GradientKind) -> Gradient {
        Gradient { kind, stops: Vec::new(), oklab: None, output: crate::colorspace::ColorSpace::Srgb, transform: Matrix::IDENTITY, spread: Spread::Pad, linear: None }
    }

    /// The gradient for a canvas that stores `space`: stop colors converted
    /// (sRGB interpolation happens in the storage encoding, as browsers do).
    #[must_use]
    pub fn in_space(&self, space: crate::colorspace::ColorSpace) -> Gradient {
        let mut g = self.clone();
        g.output = space;
        if g.oklab.is_none() {
            for s in &mut g.stops {
                let c = crate::colorspace::from_extended_srgb([s.color.r as f64, s.color.g as f64, s.color.b as f64], space);
                s.color = Color::rgba(c[0] as f32, c[1] as f32, c[2] as f32, s.color.a);
            }
        }
        g
    }

    /// Chooses the interpolation space after the stops are in place:
    /// OKLab as soon as one stop is not a legacy sRGB color.
    pub fn choose_interpolation(&mut self, any_non_legacy: bool) {
        self.oklab = any_non_legacy.then(|| {
            self.stops
                .iter()
                .map(|s| {
                    let a = s.color.a.clamp(0.0, 1.0);
                    let lab = crate::css_color::srgb_to_oklab([s.color.r as f64, s.color.g as f64, s.color.b as f64]);
                    [lab[0] as f32 * a, lab[1] as f32 * a, lab[2] as f32 * a, a]
                })
                .collect()
        });
    }

    /// OKLab interpolation between stops `i - 1` and `i` (premultiplied),
    /// returned as premultiplied sRGB.
    fn oklab_at(lab: &[[f32; 4]], i: usize, f: f32, output: crate::colorspace::ColorSpace) -> [f32; 4] {
        let (p, q) = (lab[i.saturating_sub(1)], lab[i.min(lab.len() - 1)]);
        let a = p[3] + (q[3] - p[3]) * f;
        if a <= 0.0 {
            return [0.0; 4];
        }
        let ch = |k: usize| ((p[k] + (q[k] - p[k]) * f) / a) as f64;
        let lin = crate::css_color::oklab_to_linear_srgb([ch(0), ch(1), ch(2)]);
        if output != crate::colorspace::ColorSpace::Srgb {
            let c = crate::colorspace::from_linear_srgb(lin, output);
            return [c[0] as f32 * a, c[1] as f32 * a, c[2] as f32 * a, a];
        }
        let e = |v: f64| encode_srgb(v as f32) * a;
        [e(lin[0]), e(lin[1]), e(lin[2]), a]
    }

    /// Interpolates the (sRGB) stops in linear-light RGB.
    pub fn use_linear_rgb(&mut self) {
        self.linear = Some(
            self.stops
                .iter()
                .map(|s| {
                    let a = s.color.a.clamp(0.0, 1.0);
                    let l = |v: f32| crate::css_color::srgb_to_linear(f64::from(v.clamp(0.0, 1.0))) as f32 * a;
                    [l(s.color.r), l(s.color.g), l(s.color.b), a]
                })
                .collect(),
        );
    }

    /// Inserts a stop after all stops with an offset `<=` the new one.
    pub fn add_stop(&mut self, offset: f32, color: Color) {
        let i = self.stops.partition_point(|s| s.offset <= offset);
        self.stops.insert(i, ColorStop { offset, color });
    }

    /// The premultiplied color at gradient position `t`.
    #[must_use]
    pub fn color_at(&self, t: f32) -> [f32; 4] {
        let t = match self.spread {
            Spread::Pad => t,
            Spread::Repeat => t - t.floor(),
            Spread::Reflect => {
                let m = t.rem_euclid(2.0);
                if m > 1.0 { 2.0 - m } else { m }
            }
        };
        let s = &self.stops;
        if let Some(lin) = self.linear.as_deref().filter(|l| l.len() == s.len() && !l.is_empty()) {
            let (p, q, f) = if t <= s[0].offset {
                (lin[0], lin[0], 0.0)
            } else if t >= s[s.len() - 1].offset {
                (lin[s.len() - 1], lin[s.len() - 1], 0.0)
            } else {
                let i = s.partition_point(|st| st.offset <= t);
                let span = s[i].offset - s[i - 1].offset;
                (lin[i - 1], lin[i], if span > 0.0 { (t - s[i - 1].offset) / span } else { 1.0 })
            };
            let a = p[3] + (q[3] - p[3]) * f;
            if a <= 0.0 {
                return [0.0; 4];
            }
            let e = |k: usize| encode_srgb((p[k] + (q[k] - p[k]) * f) / a) * a;
            return [e(0), e(1), e(2), a];
        }
        if let Some(lab) = self.oklab.as_deref().filter(|l| l.len() == s.len() && !l.is_empty()) {
            let out = self.output;
            return if t <= s[0].offset {
                Gradient::oklab_at(lab, 0, 0.0, out)
            } else if t >= s[s.len() - 1].offset {
                Gradient::oklab_at(lab, s.len() - 1, 1.0, out)
            } else {
                let i = s.partition_point(|st| st.offset <= t);
                let span = s[i].offset - s[i - 1].offset;
                Gradient::oklab_at(lab, i, if span > 0.0 { (t - s[i - 1].offset) / span } else { 1.0 }, out)
            };
        }
        let c = if s.is_empty() {
            return [0.0; 4];
        } else if t <= s[0].offset {
            s[0].color
        } else if t >= s[s.len() - 1].offset {
            s[s.len() - 1].color
        } else {
            let i = s.partition_point(|st| st.offset <= t);
            let (a, b) = (&s[i - 1], &s[i]);
            let span = b.offset - a.offset;
            let f = if span > 0.0 { (t - a.offset) / span } else { 1.0 };
            let l = |p: f32, q: f32| p + (q - p) * f;
            Color::rgba(l(a.color.r, b.color.r), l(a.color.g, b.color.g), l(a.color.b, b.color.b), l(a.color.a, b.color.a))
        };
        let a = c.a.clamp(0.0, 1.0);
        [c.r.clamp(0.0, 1.0) * a, c.g.clamp(0.0, 1.0) * a, c.b.clamp(0.0, 1.0) * a, a]
    }

    /// Gradient parameter for a point in gradient space, `None` where the
    /// gradient paints nothing.
    #[must_use]
    pub fn position(&self, x: f64, y: f64) -> Option<f32> {
        match self.kind {
            GradientKind::Linear { x0, y0, x1, y1 } => {
                let (dx, dy) = (x1 - x0, y1 - y0);
                let len = dx * dx + dy * dy;
                if len == 0.0 {
                    return None;
                }
                Some((((x - x0) * dx + (y - y0) * dy) / len) as f32)
            }
            GradientKind::Radial { x0, y0, r0, x1, y1, r1 } => {
                if x0 == x1 && y0 == y1 && r0 == r1 {
                    return None;
                }
                let (cdx, cdy, dr) = (x1 - x0, y1 - y0, r1 - r0);
                let (pdx, pdy) = (x - x0, y - y0);
                let a = cdx * cdx + cdy * cdy - dr * dr;
                let b = pdx * cdx + pdy * cdy + r0 * dr;
                let c = pdx * pdx + pdy * pdy - r0 * r0;
                let valid = |w: f64| r0 + w * dr >= 0.0;
                if a.abs() < 1e-12 {
                    if b == 0.0 {
                        return None;
                    }
                    let w = c / (2.0 * b);
                    return valid(w).then_some(w as f32);
                }
                let disc = b * b - a * c;
                if disc < 0.0 {
                    return None;
                }
                let sq = disc.sqrt();
                let (w1, w2) = ((b + sq) / a, (b - sq) / a);
                let (hi, lo) = if w1 > w2 { (w1, w2) } else { (w2, w1) };
                if valid(hi) {
                    Some(hi as f32)
                } else if valid(lo) {
                    Some(lo as f32)
                } else {
                    None
                }
            }
            GradientKind::Conic { angle, x: cx, y: cy } => {
                let a = (y - cy).atan2(x - cx) - angle;
                let t = a / std::f64::consts::TAU;
                Some((t - t.floor()) as f32)
            }
        }
    }
}

/// Linear light to gamma-encoded sRGB through a table (gradients in OKLab
/// convert every pixel); input clamped to `0..=1`.
fn encode_srgb(v: f32) -> f32 {
    const N: usize = 4096;
    static TABLE: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| (0..=N).map(|i| crate::css_color::linear_to_srgb(i as f64 / N as f64) as f32).collect());
    let x = v.clamp(0.0, 1.0) * N as f32;
    let i = (x as usize).min(N - 1);
    let f = x - i as f32;
    t[i] + (t[i + 1] - t[i]) * f
}

/// Pattern repetition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Repetition {
    #[default]
    Repeat,
    RepeatX,
    RepeatY,
    NoRepeat,
}

impl Repetition {
    #[must_use]
    pub fn parse(s: &str) -> Option<Repetition> {
        Some(match s {
            "" | "repeat" => Repetition::Repeat,
            "repeat-x" => Repetition::RepeatX,
            "repeat-y" => Repetition::RepeatY,
            "no-repeat" => Repetition::NoRepeat,
            _ => return None,
        })
    }
}

/// A premultiplied RGBA8 image shared between surfaces, patterns and
/// bitmaps (copy-on-write through `Arc`).
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub data: Arc<Vec<u8>>,
}

impl Image {
    #[inline]
    fn texel(&self, x: i64, y: i64) -> [f32; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = &self.data[i..i + 4];
        [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, p[3] as f32 / 255.0]
    }
}

/// Image smoothing quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Smoothing {
    Nearest,
    #[default]
    Bilinear,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub image: Image,
    pub repetition: Repetition,
    pub transform: Matrix,
}

/// The source of an image draw: a sub-rectangle of an image that must not
/// be sampled outside its bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageSource {
    pub image: Image,
    /// Source rectangle in image pixels (already clipped to the image).
    pub sx: f64,
    pub sy: f64,
    pub sw: f64,
    pub sh: f64,
}

/// What a draw paints with.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    /// Premultiplied color.
    Solid([f32; 4]),
    Gradient(Arc<Gradient>),
    Pattern(Arc<Pattern>),
    /// `drawImage`: maps device space to source image pixels via `to_image`.
    Image { source: Arc<ImageSource>, to_image: Matrix },
}

/// A paint prepared for one draw: device pixel centers map into paint space
/// through `inverse`.
pub struct Shader<'a> {
    pub paint: &'a Paint,
    pub inverse: Matrix,
    pub smoothing: Smoothing,
}

impl Shader<'_> {
    /// Premultiplied colors of pixels `x0..x0+out.len()` in row `y`.
    pub fn shade(&self, x0: i32, y: i32, out: &mut [[f32; 4]]) {
        match self.paint {
            Paint::Solid(c) => out.fill(*c),
            Paint::Gradient(g) => {
                let m = &self.inverse;
                let py = y as f64 + 0.5;
                for (i, o) in out.iter_mut().enumerate() {
                    let px = (x0 + i as i32) as f64 + 0.5;
                    let (u, v) = m.apply(px, py);
                    *o = match g.position(u, v) {
                        Some(t) if t.is_finite() => g.color_at(t),
                        _ => [0.0; 4],
                    };
                }
            }
            Paint::Pattern(p) => {
                let m = &self.inverse;
                let py = y as f64 + 0.5;
                for (i, o) in out.iter_mut().enumerate() {
                    let px = (x0 + i as i32) as f64 + 0.5;
                    let (u, v) = m.apply(px, py);
                    *o = sample_pattern(p, u, v, self.smoothing);
                }
            }
            Paint::Image { source, to_image } => {
                let m = to_image;
                let py = y as f64 + 0.5;
                for (i, o) in out.iter_mut().enumerate() {
                    let px = (x0 + i as i32) as f64 + 0.5;
                    let (u, v) = m.apply(px, py);
                    *o = sample_clamped(source, u, v, self.smoothing);
                }
            }
        }
    }
}

fn wrap(v: i64, n: i64) -> i64 {
    v.rem_euclid(n)
}

fn sample_pattern(p: &Pattern, u: f64, v: f64, smoothing: Smoothing) -> [f32; 4] {
    let img = &p.image;
    let (w, h) = (img.width as i64, img.height as i64);
    if w == 0 || h == 0 || !u.is_finite() || !v.is_finite() {
        return [0.0; 4];
    }
    let rx = matches!(p.repetition, Repetition::Repeat | Repetition::RepeatX);
    let ry = matches!(p.repetition, Repetition::Repeat | Repetition::RepeatY);
    let fetch = |x: i64, y: i64| -> [f32; 4] {
        let x = if rx { wrap(x, w) } else if x < 0 || x >= w { return [0.0; 4] } else { x };
        let y = if ry { wrap(y, h) } else if y < 0 || y >= h { return [0.0; 4] } else { y };
        img.texel(x, y)
    };
    let (u, v) = (u.clamp(-1.0e9, 1.0e9), v.clamp(-1.0e9, 1.0e9));
    match smoothing {
        Smoothing::Nearest => fetch(u.floor() as i64, v.floor() as i64),
        Smoothing::Bilinear => {
            let (fu, fv) = (u - 0.5, v - 0.5);
            let (x0, y0) = (fu.floor() as i64, fv.floor() as i64);
            let (tx, ty) = ((fu - fu.floor()) as f32, (fv - fv.floor()) as f32);
            bilerp(fetch(x0, y0), fetch(x0 + 1, y0), fetch(x0, y0 + 1), fetch(x0 + 1, y0 + 1), tx, ty)
        }
    }
}

fn sample_clamped(s: &ImageSource, u: f64, v: f64, smoothing: Smoothing) -> [f32; 4] {
    let img = &s.image;
    if img.width == 0 || img.height == 0 || !u.is_finite() || !v.is_finite() {
        return [0.0; 4];
    }
    let xmin = s.sx.floor().max(0.0) as i64;
    let ymin = s.sy.floor().max(0.0) as i64;
    let xmax = ((s.sx + s.sw).ceil() as i64 - 1).clamp(xmin, img.width as i64 - 1);
    let ymax = ((s.sy + s.sh).ceil() as i64 - 1).clamp(ymin, img.height as i64 - 1);
    let fetch = |x: i64, y: i64| img.texel(x.clamp(xmin, xmax), y.clamp(ymin, ymax));
    let (u, v) = (u.clamp(-1.0e9, 1.0e9), v.clamp(-1.0e9, 1.0e9));
    match smoothing {
        Smoothing::Nearest => fetch(u.floor() as i64, v.floor() as i64),
        Smoothing::Bilinear => {
            let (fu, fv) = (u - 0.5, v - 0.5);
            let (x0, y0) = (fu.floor() as i64, fv.floor() as i64);
            let (tx, ty) = ((fu - fu.floor()) as f32, (fv - fv.floor()) as f32);
            bilerp(fetch(x0, y0), fetch(x0 + 1, y0), fetch(x0, y0 + 1), fetch(x0 + 1, y0 + 1), tx, ty)
        }
    }
}

#[inline]
fn bilerp(a: [f32; 4], b: [f32; 4], c: [f32; 4], d: [f32; 4], tx: f32, ty: f32) -> [f32; 4] {
    let mut o = [0.0f32; 4];
    for i in 0..4 {
        let top = a[i] + (b[i] - a[i]) * tx;
        let bot = c[i] + (d[i] - c[i]) * tx;
        o[i] = top + (bot - top) * ty;
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization_matches_browsers() {
        assert_eq!(Color::from_rgba8(255, 0, 0, 255).serialize(), "#ff0000");
        assert_eq!(Color::rgba(1.0, 1.0, 1.0, 0.5).serialize(), "rgba(255, 255, 255, 0.5)");
        assert_eq!(Color::rgba(0.0, 0.0, 0.0, 0.0).serialize(), "rgba(0, 0, 0, 0)");
        let s = Color::rgba(1.0, 1.0, 1.0, 0.45).serialize();
        assert!(s.starts_with("rgba(255, 255, 255, 0.4"), "{s}");
    }

    #[test]
    fn gradient_interpolates_unpremultiplied() {
        let mut g = Gradient::new(GradientKind::Linear { x0: 0.0, y0: 0.0, x1: 100.0, y1: 0.0 });
        g.add_stop(0.0, Color::rgba(1.0, 1.0, 0.0, 0.0));
        g.add_stop(1.0, Color::rgba(0.0, 0.0, 1.0, 1.0));
        let c = g.color_at(0.25);
        let a = c[3];
        assert!((c[0] / a - 0.75).abs() < 1e-5 && (a - 0.25).abs() < 1e-6);
    }

    #[test]
    fn non_legacy_stops_interpolate_in_oklab() {
        // WPT 2d.gradient.colormix: #f00 to color(srgb 0 0.5 0.5).
        let mut g = Gradient::new(GradientKind::Linear { x0: 0.0, y0: 0.0, x1: 100.0, y1: 0.0 });
        g.add_stop(0.0, Color::from_rgba8(255, 0, 0, 255));
        g.add_stop(1.0, Color::rgba(0.0, 0.5, 0.5, 1.0));
        g.choose_interpolation(true);
        let c = g.color_at(0.255);
        let px = [c[0] * 255.0, c[1] * 255.0, c[2] * 255.0];
        assert!((px[0] - 212.0).abs() < 3.0 && (px[1] - 81.0).abs() < 3.0 && (px[2] - 61.0).abs() < 3.0, "{px:?}");
    }

    #[test]
    fn equal_offsets_keep_insertion_order() {
        let mut g = Gradient::new(GradientKind::Linear { x0: 0.0, y0: 0.0, x1: 1.0, y1: 0.0 });
        g.add_stop(0.5, Color::from_rgba8(255, 0, 0, 255));
        g.add_stop(0.5, Color::from_rgba8(0, 255, 0, 255));
        assert_eq!(g.color_at(0.4)[0], 1.0);
        assert_eq!(g.color_at(0.6)[1], 1.0);
    }

    #[test]
    fn radial_gradient_picks_largest_omega() {
        let g = Gradient::new(GradientKind::Radial { x0: 0.0, y0: 0.0, r0: 0.0, x1: 0.0, y1: 0.0, r1: 10.0 });
        assert!((g.position(5.0, 0.0).unwrap() - 0.5).abs() < 1e-6);
    }
}
