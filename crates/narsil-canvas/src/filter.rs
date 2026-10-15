//! `ctx.filter` (Filter Effects Module Level 1 shorthand functions) applied
//! to premultiplied RGBA layers.

use crate::blur::gaussian_blur;

/// One filter primitive.
#[derive(Clone, Debug, PartialEq)]
pub enum FilterOp {
    /// Gaussian blur with this standard deviation (x, y) in pixels.
    Blur(f64, f64),
    /// A 4x5 color matrix on un-premultiplied RGBA (row-major, the last
    /// column is the offset), results clamped to `0..=1`.
    ColorMatrix([f32; 20]),
    /// Per-channel linear transfer `c * slope + intercept` (RGB only).
    Linear { slope: f32, intercept: f32 },
    /// `opacity()`: alpha times this.
    Opacity(f32),
    /// `drop-shadow()`: offset, standard deviations (x, y) and
    /// premultiplied color.
    DropShadow { dx: f64, dy: f64, sigma: f64, sigma_y: f64, color: [f32; 4] },
    /// Per-channel component transfer tables (`feComponentTransfer`, table
    /// type), un-premultiplied; empty tables leave a channel unchanged.
    Table([Vec<f32>; 4]),
    /// `feComponentTransfer` with every function type (CanvasFilter
    /// `componentTransfer`), un-premultiplied RGBA.
    Transfer(Box<[TransferFn; 4]>),
    /// `feConvolveMatrix` (CanvasFilter `convolveMatrix`).
    Convolve(Box<Convolution>),
    /// `feTurbulence` (CanvasFilter `turbulence`): replaces the input.
    Turbulence(Turbulence),
}

/// One `feFuncX` transfer function.
#[derive(Clone, Debug, PartialEq)]
pub enum TransferFn {
    Identity,
    Table(Vec<f32>),
    Discrete(Vec<f32>),
    Linear { slope: f32, intercept: f32 },
    Gamma { amplitude: f32, exponent: f32, offset: f32 },
}

impl TransferFn {
    pub fn apply(&self, c: f32) -> f32 {
        match self {
            TransferFn::Identity => c,
            TransferFn::Table(t) => table(t, c),
            TransferFn::Discrete(t) => {
                let n = t.len();
                if n == 0 {
                    return c;
                }
                let k = if c >= 1.0 { n - 1 } else { ((c.max(0.0) * n as f32).floor() as usize).min(n - 1) };
                t[k].clamp(0.0, 1.0)
            }
            TransferFn::Linear { slope, intercept } => (slope * c + intercept).clamp(0.0, 1.0),
            TransferFn::Gamma { amplitude, exponent, offset } => (amplitude * c.max(0.0).powf(*exponent) + offset).clamp(0.0, 1.0),
        }
    }
}

/// Edge handling of a convolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeMode {
    Duplicate,
    Wrap,
    None,
}

/// `feConvolveMatrix` parameters; `kernel` is row-major, `order_x` wide.
#[derive(Clone, Debug, PartialEq)]
pub struct Convolution {
    pub order_x: usize,
    pub order_y: usize,
    pub kernel: Vec<f32>,
    pub divisor: f32,
    pub bias: f32,
    pub target_x: i64,
    pub target_y: i64,
    pub edge: EdgeMode,
    pub preserve_alpha: bool,
}

/// `feTurbulence` parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turbulence {
    pub base_x: f64,
    pub base_y: f64,
    pub octaves: u32,
    pub seed: f64,
    pub stitch: bool,
    pub fractal: bool,
}

/// A parsed filter list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    pub ops: Vec<FilterOp>,
}

impl Filter {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// How far (pixels) the filter can move content: `(left, top, right,
    /// bottom)` margins a layer needs so that nothing outside it can reach
    /// the visible area.
    #[must_use]
    pub fn reach(&self) -> (f64, f64, f64, f64) {
        let (mut l, mut t, mut r, mut b) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for op in &self.ops {
            match op {
                FilterOp::Blur(sx, sy) => {
                    l += 3.0 * sx;
                    r += 3.0 * sx;
                    t += 3.0 * sy;
                    b += 3.0 * sy;
                }
                FilterOp::DropShadow { dx, dy, sigma, sigma_y, .. } => {
                    let (sx, sy) = (3.0 * sigma.max(0.0), 3.0 * sigma_y.max(0.0));
                    l = l.max(sx + dx.max(0.0));
                    r = r.max(sx - dx.min(0.0));
                    t = t.max(sy + dy.max(0.0));
                    b = b.max(sy - dy.min(0.0));
                }
                FilterOp::Convolve(c) => {
                    l += c.target_x.max(0) as f64;
                    r += (c.order_x as i64 - 1 - c.target_x).max(0) as f64;
                    t += c.target_y.max(0) as f64;
                    b += (c.order_y as i64 - 1 - c.target_y).max(0) as f64;
                }
                _ => {}
            }
        }
        (l.ceil(), t.ceil(), r.ceil(), b.ceil())
    }

    /// True when transparent pixels can become visible (the filter must
    /// then run over the whole layer, not only over the drawn area).
    #[must_use]
    pub fn affects_transparent(&self) -> bool {
        self.ops.iter().any(|op| match op {
            FilterOp::ColorMatrix(m) => m[19] != 0.0,
            FilterOp::Table(t) => t[3].first().is_some_and(|v| *v != 0.0),
            FilterOp::Transfer(f) => f[3].apply(0.0) != 0.0,
            FilterOp::Convolve(c) => c.bias != 0.0,
            FilterOp::Turbulence(_) => true,
            _ => false,
        })
    }

    /// Applies every primitive in order to a premultiplied RGBA layer.
    pub fn apply(&self, buf: &mut [f32], w: usize, h: usize) {
        for op in &self.ops {
            match op {
                FilterOp::Blur(sx, sy) => blur_rgba_xy(buf, w, h, *sx, *sy),
                FilterOp::ColorMatrix(m) => per_pixel(buf, |c| {
                    let mut o = [0.0f32; 4];
                    for (r, out) in o.iter_mut().enumerate() {
                        let row = &m[r * 5..r * 5 + 5];
                        *out = (row[0] * c[0] + row[1] * c[1] + row[2] * c[2] + row[3] * c[3] + row[4]).clamp(0.0, 1.0);
                    }
                    o
                }),
                FilterOp::Linear { slope, intercept } => per_pixel(buf, |c| {
                    [(c[0] * slope + intercept).clamp(0.0, 1.0), (c[1] * slope + intercept).clamp(0.0, 1.0), (c[2] * slope + intercept).clamp(0.0, 1.0), c[3]]
                }),
                FilterOp::Opacity(a) => {
                    let a = a.clamp(0.0, 1.0);
                    for v in buf.iter_mut() {
                        *v *= a;
                    }
                }
                FilterOp::Table(t) => per_pixel(buf, |c| {
                    let mut o = c;
                    for k in 0..4 {
                        o[k] = table(&t[k], c[k]);
                    }
                    o
                }),
                FilterOp::DropShadow { dx, dy, sigma, sigma_y, color } => drop_shadow(buf, w, h, *dx, *dy, (*sigma, *sigma_y), *color),
                FilterOp::Transfer(f) => per_pixel(buf, |c| [f[0].apply(c[0]), f[1].apply(c[1]), f[2].apply(c[2]), f[3].apply(c[3])]),
                FilterOp::Convolve(c) => convolve(buf, w, h, c),
                FilterOp::Turbulence(t) => turbulence(buf, w, h, t),
            }
        }
    }

    /// Builds a filter from the flat CanvasFilter description the bindings
    /// produce: `[op, params...]` per primitive (see `canvas2d.js`).
    #[must_use]
    pub fn from_spec(spec: &[f64]) -> Option<Filter> {
        let mut ops = Vec::new();
        let mut i = 0usize;
        let take = |i: &mut usize, n: usize| -> Option<&[f64]> {
            let s = spec.get(*i..*i + n)?;
            *i += n;
            Some(s)
        };
        while i < spec.len() {
            let op = spec[i] as u32;
            i += 1;
            match op {
                1 => {
                    let p = take(&mut i, 2)?;
                    ops.push(FilterOp::Blur(p[0].max(0.0), p[1].max(0.0)));
                }
                2 => {
                    let p = take(&mut i, 8)?;
                    let a = p[7].clamp(0.0, 1.0) as f32;
                    let ch = |v: f64| v.clamp(0.0, 1.0) as f32 * a;
                    ops.push(FilterOp::DropShadow { dx: p[0], dy: p[1], sigma: p[2].max(0.0), sigma_y: p[3].max(0.0), color: [ch(p[4]), ch(p[5]), ch(p[6]), a] });
                }
                3 => {
                    let kind = take(&mut i, 1)?[0] as u32;
                    let m = match kind {
                        0 => {
                            let p = take(&mut i, 20)?;
                            let mut m = [0.0f32; 20];
                            for (o, v) in m.iter_mut().zip(p) {
                                *o = *v as f32;
                            }
                            m
                        }
                        1 => saturate(take(&mut i, 1)?[0] as f32),
                        2 => hue_rotate(take(&mut i, 1)?[0]),
                        _ => luminance_to_alpha(),
                    };
                    ops.push(FilterOp::ColorMatrix(m));
                }
                4 => {
                    let mut fns: Vec<TransferFn> = Vec::with_capacity(4);
                    for _ in 0..4 {
                        let head = take(&mut i, 2)?;
                        let (kind, n) = (head[0] as u32, head[1] as usize);
                        let p: Vec<f32> = take(&mut i, n)?.iter().map(|v| *v as f32).collect();
                        fns.push(match (kind, p.as_slice()) {
                            (1, _) => TransferFn::Table(p),
                            (2, _) => TransferFn::Discrete(p),
                            (3, [s, b]) => TransferFn::Linear { slope: *s, intercept: *b },
                            (4, [a, e, o]) => TransferFn::Gamma { amplitude: *a, exponent: *e, offset: *o },
                            _ => TransferFn::Identity,
                        });
                    }
                    let fns: [TransferFn; 4] = fns.try_into().ok()?;
                    ops.push(FilterOp::Transfer(Box::new(fns)));
                }
                5 => {
                    let p = take(&mut i, 8)?;
                    let (ox, oy) = (p[0] as usize, p[1] as usize);
                    let n = ox.checked_mul(oy)?;
                    if n > 4096 {
                        return None;
                    }
                    let kernel: Vec<f32> = take(&mut i, n)?.iter().map(|v| *v as f32).collect();
                    if n == 0 {
                        continue;
                    }
                    let edge = match p[6] as u32 {
                        0 => EdgeMode::Duplicate,
                        1 => EdgeMode::Wrap,
                        _ => EdgeMode::None,
                    };
                    let (tx, ty) = (p[4].floor() as i64, p[5].floor() as i64);
                    if tx < 0 || ty < 0 || tx >= ox as i64 || ty >= oy as i64 {
                        continue;
                    }
                    ops.push(FilterOp::Convolve(Box::new(Convolution {
                        order_x: ox,
                        order_y: oy,
                        kernel,
                        divisor: if p[2] == 0.0 { 1.0 } else { p[2] as f32 },
                        bias: p[3] as f32,
                        target_x: tx,
                        target_y: ty,
                        edge,
                        preserve_alpha: p[7] != 0.0,
                    })));
                }
                6 => {
                    let p = take(&mut i, 6)?;
                    ops.push(FilterOp::Turbulence(Turbulence {
                        base_x: p[0].max(0.0),
                        base_y: p[1].max(0.0),
                        octaves: p[2].clamp(0.0, 16.0) as u32,
                        seed: p[3],
                        stitch: p[4] != 0.0,
                        fractal: p[5] != 0.0,
                    }));
                }
                _ => return None,
            }
        }
        Some(Filter { ops })
    }
}

/// `feColorMatrix type="luminanceToAlpha"`.
#[must_use]
pub fn luminance_to_alpha() -> [f32; 20] {
    [
        0.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 0.0, 0.0,
        0.2125, 0.7154, 0.0721, 0.0, 0.0,
    ]
}

/// `feConvolveMatrix` on a premultiplied layer (SVG 1.1 section 15.13).
pub fn convolve(buf: &mut [f32], w: usize, h: usize, c: &Convolution) {
    if w == 0 || h == 0 {
        return;
    }
    let src = buf.to_vec();
    let fetch = |x: i64, y: i64| -> Option<usize> {
        let (wi, hi) = (w as i64, h as i64);
        let (x, y) = match c.edge {
            EdgeMode::Duplicate => (x.clamp(0, wi - 1), y.clamp(0, hi - 1)),
            EdgeMode::Wrap => (x.rem_euclid(wi), y.rem_euclid(hi)),
            EdgeMode::None => {
                if x < 0 || y < 0 || x >= wi || y >= hi {
                    return None;
                }
                (x, y)
            }
        };
        Some((y as usize * w + x as usize) * 4)
    };
    let unpremul = |i: usize| -> [f32; 4] {
        let a = src[i + 3];
        if a > 0.0 { [src[i] / a, src[i + 1] / a, src[i + 2] / a, a] } else { [0.0; 4] }
    };
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0.0f32; 4];
            for ky in 0..c.order_y {
                for kx in 0..c.order_x {
                    let sx = x as i64 - c.target_x + kx as i64;
                    let sy = y as i64 - c.target_y + ky as i64;
                    let Some(i) = fetch(sx, sy) else { continue };
                    // The kernel is applied rotated by 180 degrees.
                    let k = c.kernel[(c.order_y - 1 - ky) * c.order_x + (c.order_x - 1 - kx)];
                    let p = if c.preserve_alpha { unpremul(i) } else { [src[i], src[i + 1], src[i + 2], src[i + 3]] };
                    for j in 0..4 {
                        sum[j] += p[j] * k;
                    }
                }
            }
            let o = (y * w + x) * 4;
            if c.preserve_alpha {
                let a = src[o + 3];
                for j in 0..3 {
                    buf[o + j] = (sum[j] / c.divisor + c.bias).clamp(0.0, 1.0) * a;
                }
                buf[o + 3] = a;
            } else {
                let a = (sum[3] / c.divisor + c.bias).clamp(0.0, 1.0);
                for j in 0..3 {
                    buf[o + j] = (sum[j] / c.divisor + c.bias * a).clamp(0.0, a);
                }
                buf[o + 3] = a;
            }
        }
    }
}

// --- feTurbulence (SVG 1.1 section 15.24 reference implementation) ---------

const B_SIZE: usize = 0x100;
const B_MASK: i64 = 0xff;
const PERLIN_N: f64 = 4096.0;

struct Lattice {
    selector: Vec<usize>,
    gradient: Vec<[[f64; 2]; 4]>,
}

fn random(seed: i64) -> i64 {
    const M: i64 = 2_147_483_647;
    const A: i64 = 16807;
    const Q: i64 = 127_773;
    const R: i64 = 2836;
    let mut r = A * (seed % Q) - R * (seed / Q);
    if r <= 0 {
        r += M;
    }
    r
}

fn lattice(seed: f64) -> Lattice {
    const M: i64 = 2_147_483_647;
    let mut s = seed.round() as i64;
    if s <= 0 {
        s = -(s % (M - 1)) + 1;
    }
    if s > M - 1 {
        s = M - 1;
    }
    let n = B_SIZE + B_SIZE + 2;
    let mut selector = vec![0usize; n];
    let mut gradient = vec![[[0.0f64; 2]; 4]; n];
    for k in 0..4 {
        for i in 0..B_SIZE {
            selector[i] = i;
            for j in 0..2 {
                s = random(s);
                gradient[i][k][j] = ((s % (2 * B_SIZE as i64)) - B_SIZE as i64) as f64 / B_SIZE as f64;
            }
            let g = gradient[i][k];
            let len = (g[0] * g[0] + g[1] * g[1]).sqrt();
            if len > 0.0 {
                gradient[i][k] = [g[0] / len, g[1] / len];
            }
        }
    }
    for i in (1..B_SIZE).rev() {
        let k = selector[i];
        s = random(s);
        let j = (s % B_SIZE as i64) as usize;
        selector[i] = selector[j];
        selector[j] = k;
    }
    for i in 0..B_SIZE + 2 {
        selector[B_SIZE + i] = selector[i];
        gradient[B_SIZE + i] = gradient[i];
    }
    Lattice { selector, gradient }
}

#[derive(Clone, Copy)]
struct Stitch {
    width: i64,
    height: i64,
    wrap_x: i64,
    wrap_y: i64,
}

fn noise2(l: &Lattice, channel: usize, vx: f64, vy: f64, stitch: Option<&Stitch>) -> f64 {
    let t = vx + PERLIN_N;
    let mut bx0 = t as i64 & B_MASK;
    let mut bx1 = (bx0 + 1) & B_MASK;
    let rx0 = t - (t as i64) as f64;
    let rx1 = rx0 - 1.0;
    let t = vy + PERLIN_N;
    let mut by0 = t as i64 & B_MASK;
    let mut by1 = (by0 + 1) & B_MASK;
    let ry0 = t - (t as i64) as f64;
    let ry1 = ry0 - 1.0;
    if let Some(s) = stitch {
        if bx0 >= s.wrap_x {
            bx0 -= s.width;
        }
        if bx1 >= s.wrap_x {
            bx1 -= s.width;
        }
        if by0 >= s.wrap_y {
            by0 -= s.height;
        }
        if by1 >= s.wrap_y {
            by1 -= s.height;
        }
    }
    let (bx0, bx1, by0, by1) = ((bx0 & B_MASK) as usize, (bx1 & B_MASK) as usize, (by0 & B_MASK) as usize, (by1 & B_MASK) as usize);
    let i = l.selector[bx0];
    let j = l.selector[bx1];
    let b00 = l.selector[i + by0];
    let b10 = l.selector[j + by0];
    let b01 = l.selector[i + by1];
    let b11 = l.selector[j + by1];
    let curve = |t: f64| t * t * (3.0 - 2.0 * t);
    let lerp = |t: f64, a: f64, b: f64| a + t * (b - a);
    let (sx, sy) = (curve(rx0), curve(ry0));
    let q = l.gradient[b00][channel];
    let u = rx0 * q[0] + ry0 * q[1];
    let q = l.gradient[b10][channel];
    let v = rx1 * q[0] + ry0 * q[1];
    let a = lerp(sx, u, v);
    let q = l.gradient[b01][channel];
    let u = rx0 * q[0] + ry1 * q[1];
    let q = l.gradient[b11][channel];
    let v = rx1 * q[0] + ry1 * q[1];
    let b = lerp(sx, u, v);
    lerp(sy, a, b)
}

fn turbulence(buf: &mut [f32], w: usize, h: usize, t: &Turbulence) {
    turbulence_mapped(buf, w, h, t, (0.0, 0.0, w as f64, h as f64), &|x, y| (x as f64, y as f64));
}

/// `feTurbulence` over a `w` x `h` premultiplied buffer: `point` maps a
/// pixel to its point in the noise (filter primitive) space, and `tile` is
/// the stitching tile (x, y, width, height) in that space.
pub fn turbulence_mapped(buf: &mut [f32], w: usize, h: usize, t: &Turbulence, tile: (f64, f64, f64, f64), point: &dyn Fn(usize, usize) -> (f64, f64)) {
    let l = lattice(t.seed);
    let (mut fx, mut fy) = (t.base_x, t.base_y);
    let (tx, ty, tw, th) = tile;
    let mut stitch = None;
    if t.stitch {
        if fx != 0.0 {
            let lo = (tw * fx).floor() / tw;
            let hi = (tw * fx).ceil() / tw;
            fx = if lo > 0.0 && fx / lo < hi / fx { lo } else { hi };
        }
        if fy != 0.0 {
            let lo = (th * fy).floor() / th;
            let hi = (th * fy).ceil() / th;
            fy = if lo > 0.0 && fy / lo < hi / fy { lo } else { hi };
        }
        let width = (tw * fx + 0.5) as i64;
        let height = (th * fy + 0.5) as i64;
        stitch = Some(Stitch { width, height, wrap_x: (tx * fx) as i64 + PERLIN_N as i64 + width, wrap_y: (ty * fy) as i64 + PERLIN_N as i64 + height });
    }
    for y in 0..h {
        for x in 0..w {
            let mut px = [0.0f32; 4];
            let (ux, uy) = point(x, y);
            for (channel, out) in px.iter_mut().enumerate() {
                let (mut vx, mut vy) = (ux * fx, uy * fy);
                let mut sum = 0.0;
                let mut ratio = 1.0;
                let mut st = stitch;
                for _ in 0..t.octaves {
                    let n = noise2(&l, channel, vx, vy, st.as_ref());
                    sum += if t.fractal { n } else { n.abs() } / ratio;
                    vx *= 2.0;
                    vy *= 2.0;
                    ratio *= 2.0;
                    if let Some(s) = st.as_mut() {
                        s.width *= 2;
                        s.wrap_x = 2 * s.wrap_x - PERLIN_N as i64;
                        s.height *= 2;
                        s.wrap_y = 2 * s.wrap_y - PERLIN_N as i64;
                    }
                }
                let v = if t.fractal { (sum + 1.0) / 2.0 } else { sum };
                *out = v.clamp(0.0, 1.0) as f32;
            }
            let o = (y * w + x) * 4;
            let a = px[3];
            buf[o] = px[0] * a;
            buf[o + 1] = px[1] * a;
            buf[o + 2] = px[2] * a;
            buf[o + 3] = a;
        }
    }
}

fn table(t: &[f32], c: f32) -> f32 {
    let n = t.len();
    if n == 0 {
        return c;
    }
    if n == 1 {
        return t[0].clamp(0.0, 1.0);
    }
    let c = c.clamp(0.0, 1.0);
    let k = ((c * (n - 1) as f32).floor() as usize).min(n - 2);
    let lo = k as f32 / (n - 1) as f32;
    let v = t[k] + (c - lo) * (n - 1) as f32 * (t[k + 1] - t[k]);
    v.clamp(0.0, 1.0)
}

/// Runs `f` on every pixel's un-premultiplied color.
fn per_pixel(buf: &mut [f32], f: impl Fn([f32; 4]) -> [f32; 4]) {
    for px in buf.chunks_exact_mut(4) {
        let a = px[3];
        let c = if a > 0.0 { [px[0] / a, px[1] / a, px[2] / a, a] } else { [0.0, 0.0, 0.0, 0.0] };
        let o = f(c);
        let a2 = o[3].clamp(0.0, 1.0);
        px[0] = o[0] * a2;
        px[1] = o[1] * a2;
        px[2] = o[2] * a2;
        px[3] = a2;
    }
}

fn blur_rgba_xy(buf: &mut [f32], w: usize, h: usize, sx: f64, sy: f64) {
    if sx <= 0.0 && sy <= 0.0 {
        return;
    }
    let n = w * h;
    let mut chan = vec![0.0f32; n];
    for c in 0..4 {
        for i in 0..n {
            chan[i] = buf[i * 4 + c];
        }
        blur_channel_xy(&mut chan, w, h, sx, sy);
        for i in 0..n {
            buf[i * 4 + c] = chan[i];
        }
    }
}

/// Gaussian blur of one channel with separate standard deviations.
fn blur_channel_xy(chan: &mut [f32], w: usize, h: usize, sx: f64, sy: f64) {
    let (sx, sy) = (sx.max(0.0), sy.max(0.0));
    if (sx - sy).abs() < 1e-9 {
        gaussian_blur(chan, w, h, sx);
    } else {
        crate::blur::gaussian_blur_xy(chan, w, h, sx, sy);
    }
}

fn drop_shadow(buf: &mut [f32], w: usize, h: usize, dx: f64, dy: f64, sigma: (f64, f64), color: [f32; 4]) {
    let n = w * h;
    let mut alpha = vec![0.0f32; n];
    let (ox, oy) = (dx.round() as i64, dy.round() as i64);
    for y in 0..h as i64 {
        let sy = y - oy;
        if sy < 0 || sy >= h as i64 {
            continue;
        }
        for x in 0..w as i64 {
            let sx = x - ox;
            if sx < 0 || sx >= w as i64 {
                continue;
            }
            alpha[(y as usize) * w + x as usize] = buf[((sy as usize) * w + sx as usize) * 4 + 3];
        }
    }
    blur_channel_xy(&mut alpha, w, h, sigma.0, sigma.1);
    for (i, px) in buf.chunks_exact_mut(4).enumerate() {
        let s = alpha[i];
        let sh = [color[0] * s, color[1] * s, color[2] * s, color[3] * s];
        let a = px[3];
        for k in 0..4 {
            px[k] += sh[k] * (1.0 - a);
        }
    }
}

/// Color matrices of the shorthand functions (Filter Effects 1 §13).
#[must_use]
pub fn grayscale(a: f32) -> [f32; 20] {
    let s = 1.0 - a.clamp(0.0, 1.0);
    [
        0.2126 + 0.7874 * s, 0.7152 - 0.7152 * s, 0.0722 - 0.0722 * s, 0.0, 0.0,
        0.2126 - 0.2126 * s, 0.7152 + 0.2848 * s, 0.0722 - 0.0722 * s, 0.0, 0.0,
        0.2126 - 0.2126 * s, 0.7152 - 0.7152 * s, 0.0722 + 0.9278 * s, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ]
}

#[must_use]
pub fn sepia(a: f32) -> [f32; 20] {
    let s = 1.0 - a.clamp(0.0, 1.0);
    [
        0.393 + 0.607 * s, 0.769 - 0.769 * s, 0.189 - 0.189 * s, 0.0, 0.0,
        0.349 - 0.349 * s, 0.686 + 0.314 * s, 0.168 - 0.168 * s, 0.0, 0.0,
        0.272 - 0.272 * s, 0.534 - 0.534 * s, 0.131 + 0.869 * s, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ]
}

#[must_use]
pub fn saturate(s: f32) -> [f32; 20] {
    [
        0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s, 0.0, 0.0,
        0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s, 0.0, 0.0,
        0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ]
}

/// `hue-rotate(deg)`.
#[must_use]
pub fn hue_rotate(deg: f64) -> [f32; 20] {
    let (s, c) = deg.to_radians().sin_cos();
    let (s, c) = (s as f32, c as f32);
    [
        0.213 + c * 0.787 - s * 0.213, 0.715 - c * 0.715 - s * 0.715, 0.072 - c * 0.072 + s * 0.928, 0.0, 0.0,
        0.213 - c * 0.213 + s * 0.143, 0.715 + c * 0.285 + s * 0.140, 0.072 - c * 0.072 - s * 0.283, 0.0, 0.0,
        0.213 - c * 0.213 - s * 0.787, 0.715 - c * 0.715 + s * 0.715, 0.072 + c * 0.928 + s * 0.072, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grayscale_of_red() {
        let f = Filter { ops: vec![FilterOp::ColorMatrix(grayscale(1.0))] };
        let mut px = vec![1.0, 0.0, 0.0, 1.0];
        f.apply(&mut px, 1, 1);
        assert!((px[0] - 0.2126).abs() < 1e-4 && (px[1] - 0.2126).abs() < 1e-4);
    }

    #[test]
    fn discrete_transfer_matches_svg() {
        // WPT 2d.filter.canvasFilterObject.componentTransfer.discrete.
        let f = TransferFn::Discrete(vec![0.0, 0.0, 1.0, 1.0]);
        assert_eq!(f.apply(127.0 / 255.0), 0.0);
        assert_eq!(f.apply(1.0), 1.0);
        let g = TransferFn::Discrete(vec![2.0, 0.0, 0.5, 3.0]);
        assert_eq!(g.apply(186.0 / 255.0), 0.5);
    }

    #[test]
    fn spec_round_trip_builds_primitives() {
        let f = Filter::from_spec(&[1.0, 4.0, 0.0, 3.0, 3.0, 4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]).expect("spec");
        assert_eq!(f.ops.len(), 3);
        assert!(matches!(f.ops[0], FilterOp::Blur(sx, sy) if sx == 4.0 && sy == 0.0));
        assert!(Filter::from_spec(&[1.0]).is_none());
    }

    #[test]
    fn identity_convolution_keeps_pixels() {
        let c = Convolution { order_x: 3, order_y: 3, kernel: vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0], divisor: 1.0, bias: 0.0, target_x: 1, target_y: 1, edge: EdgeMode::Duplicate, preserve_alpha: false };
        let mut px: Vec<f32> = (0..16).map(|i| (i % 4) as f32 / 4.0).collect();
        let before = px.clone();
        convolve(&mut px, 2, 2, &c);
        assert_eq!(px, before);
    }

    #[test]
    fn turbulence_is_deterministic_and_bounded() {
        let t = Turbulence { base_x: 0.05, base_y: 0.05, octaves: 2, seed: 1.0, stitch: false, fractal: true };
        let mut a = vec![0.0f32; 8 * 8 * 4];
        let mut b = a.clone();
        turbulence(&mut a, 8, 8, &t);
        turbulence(&mut b, 8, 8, &t);
        assert_eq!(a, b);
        assert!(a.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn one_axis_blur_keeps_mass() {
        // WPT 2d.layer.anisotropic-blur.x-only: stdDeviation [4, 0].
        let (w, h) = (40usize, 5usize);
        let mut px = vec![0.0f32; w * h * 4];
        for y in 0..h {
            for x in 15..25 {
                px[(y * w + x) * 4 + 3] = 1.0;
            }
        }
        Filter { ops: vec![FilterOp::Blur(4.0, 0.0)] }.apply(&mut px, w, h);
        let centre = px[(2 * w + 20) * 4 + 3];
        assert!(centre > 0.75, "{centre}");
        let row: f32 = (0..w).map(|x| px[(2 * w + x) * 4 + 3]).sum();
        assert!((row - 10.0).abs() < 0.05, "{row}");
    }

    #[test]
    fn drop_shadow_lands_offset() {
        let f = Filter { ops: vec![FilterOp::DropShadow { dx: 2.0, dy: 0.0, sigma: 0.0, sigma_y: 0.0, color: [0.0, 1.0, 0.0, 1.0] }] };
        let mut px = vec![0.0f32; 4 * 4];
        px[3] = 1.0;
        px[0] = 1.0;
        f.apply(&mut px, 4, 1);
        assert_eq!(&px[8..12], &[0.0, 1.0, 0.0, 1.0]);
        assert_eq!(&px[0..4], &[1.0, 0.0, 0.0, 1.0]);
    }
}
