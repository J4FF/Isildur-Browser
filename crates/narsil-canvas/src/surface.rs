//! The canvas bitmap (premultiplied RGBA8) and the compositing pipeline that
//! paints rasterized shapes into it.

use crate::blur::gaussian_blur;
use crate::composite::{composite, CompositeOp};
use crate::geom::{Bounds, IRect};
use crate::paint::{Image, Shader};
use crate::raster::{FillRule, Line, Rasterizer};
use std::sync::Arc;

/// Largest bitmap the canvas allocates (pixels).
pub const MAX_PIXELS: usize = 16_777_216;

/// An anti-aliased clip mask covering the whole surface it was made on
/// (`width` wide, device origin `x`, `y`).
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub mask: Arc<Vec<u8>>,
    pub bounds: IRect,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    /// The masks of further tiles of a layer that covers separate areas
    /// (see `Context2D` tiles); each part answers for its own rectangle.
    pub parts: Vec<Clip>,
}

impl Clip {
    /// Clip coverage (`0..=1`) at a device pixel; zero outside the masks.
    #[inline]
    #[must_use]
    pub fn at(&self, x: i32, y: i32) -> f32 {
        let (lx, ly) = (x - self.x, y - self.y);
        let rows = if self.width == 0 { 0 } else { self.mask.len() / self.width as usize };
        if lx >= 0 && ly >= 0 && lx < self.width as i32 && (ly as usize) < rows {
            return self.mask[ly as usize * self.width as usize + lx as usize] as f32 / 255.0;
        }
        for p in &self.parts {
            let (px, py) = (x - p.x, y - p.y);
            let prow = if p.width == 0 { 0 } else { p.mask.len() / p.width as usize };
            if px >= 0 && py >= 0 && px < p.width as i32 && (py as usize) < prow {
                return p.mask[py as usize * p.width as usize + px as usize] as f32 / 255.0;
            }
        }
        0.0
    }
}

/// Shadow parameters in device space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// Premultiplied shadow color.
    pub color: [f32; 4],
    pub offset_x: f64,
    pub offset_y: f64,
    /// Gaussian standard deviation (`shadowBlur / 2`).
    pub sigma: f64,
}

/// How a draw is composited.
pub struct Composite<'a> {
    pub op: CompositeOp,
    pub global_alpha: f32,
    pub clip: Option<&'a Clip>,
    pub shadow: Option<Shadow>,
    /// `ctx.filter`: the drawing goes through a layer first.
    pub filter: Option<&'a crate::filter::Filter>,
}

/// A premultiplied f32 RGBA image over a device rectangle (filters,
/// `beginLayer`).
#[derive(Clone, Debug)]
pub struct Layer {
    pub rect: IRect,
    pub data: Vec<f32>,
}

impl Layer {
    #[must_use]
    pub fn new(rect: IRect) -> Layer {
        let n = rect.width().max(0) as usize * rect.height().max(0) as usize;
        Layer { rect, data: vec![0.0; n * 4] }
    }

    /// Whether the device pixel lies in the layer.
    #[inline]
    #[must_use]
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.rect.x0 && y >= self.rect.y0 && x < self.rect.x1 && y < self.rect.y1
    }

    /// Alpha at a device pixel (zero outside).
    #[inline]
    #[must_use]
    pub fn alpha_at(&self, x: i32, y: i32) -> f32 {
        self.at(x, y)[3]
    }

    #[inline]
    fn at(&self, x: i32, y: i32) -> [f32; 4] {
        if x < self.rect.x0 || y < self.rect.y0 || x >= self.rect.x1 || y >= self.rect.y1 {
            return [0.0; 4];
        }
        let i = ((y - self.rect.y0) as usize * self.rect.width() as usize + (x - self.rect.x0) as usize) * 4;
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }
}

#[derive(Clone, Debug)]
pub struct Surface {
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA8, row-major.
    pub data: Arc<Vec<u8>>,
    /// `alpha: false` contexts: every pixel stays opaque.
    pub opaque: bool,
    /// Device position of the top-left pixel: the bitmap is at (0, 0),
    /// layers may extend beyond it (filters that pull content in from
    /// outside the canvas).
    pub x: i32,
    pub y: i32,
}

impl Surface {
    /// A transparent (or opaque black) surface; `None` when too large.
    #[must_use]
    pub fn new(width: u32, height: u32, opaque: bool) -> Option<Surface> {
        let n = (width as usize).checked_mul(height as usize).filter(|n| *n <= MAX_PIXELS)?;
        let mut data = vec![0u8; n * 4];
        if opaque {
            for px in data.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
        Some(Surface { width, height, data: Arc::new(data), opaque, x: 0, y: 0 })
    }

    /// A transparent surface covering the device rectangle `r`.
    #[must_use]
    pub fn covering(r: IRect) -> Option<Surface> {
        let mut s = Surface::new(r.width().max(0) as u32, r.height().max(0) as u32, false)?;
        s.x = r.x0;
        s.y = r.y0;
        Some(s)
    }

    #[must_use]
    pub fn rect(&self) -> IRect {
        IRect::new(self.x, self.y, self.x + self.width as i32, self.y + self.height as i32)
    }

    /// Index of a device pixel inside the surface (the caller checked it).
    #[inline]
    fn index(&self, x: i32, y: i32) -> usize {
        (y - self.y) as usize * self.width as usize + (x - self.x) as usize
    }

    /// A shared, copy-on-write snapshot of the bitmap.
    #[must_use]
    pub fn snapshot(&self) -> Image {
        Image { width: self.width, height: self.height, data: self.data.clone() }
    }

    /// Clears the whole bitmap to transparent black (opaque black when the
    /// context has no alpha).
    pub fn clear_all(&mut self) {
        let data = Arc::make_mut(&mut self.data);
        if self.opaque {
            for px in data.chunks_exact_mut(4) {
                px.copy_from_slice(&[0, 0, 0, 255]);
            }
        } else {
            data.fill(0);
        }
    }

    /// Paints a shape: `lines` filled with `rule`, colored by `shader`.
    pub fn fill(&mut self, raster: &mut Rasterizer, lines: &[Line], rule: FillRule, shader: &Shader<'_>, c: &Composite<'_>) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        let mut bounds = Bounds::default();
        for l in lines {
            bounds.add(l.x0 as f64, l.y0 as f64);
            bounds.add(l.x1 as f64, l.y1 as f64);
        }
        if let Some(f) = c.filter.filter(|f| !f.is_empty()) {
            // HTML drawing model: image A (the shape), then the filter
            // (image B), then shadow and compositing from B.
            // `reach` holds the input margins the filter needs around the
            // visible area; content spreads the opposite way.
            let (l, t, r, b) = f.reach();
            let (l, t, r, b) = (l as i32, t as i32, r as i32, b as i32);
            let s = self.rect();
            let reach = IRect::new(s.x0 - l, s.y0 - t, s.x1 + r, s.y1 + b);
            let rect = if f.affects_transparent() {
                reach
            } else {
                let shape = bounds.to_irect().outset(1);
                IRect::new(shape.x0 - r, shape.y0 - b, shape.x1 + l, shape.y1 + t).intersect(&reach)
            };
            let mut layer = Layer::new(rect);
            Self::render_into(raster, lines, rule, shader, &mut layer);
            // globalAlpha applies to the drawing before the filter: a
            // drop-shadow of a half-transparent shape is half transparent
            // (WPT 2d.filter.drop-shadow-globalAlpha).
            if c.global_alpha < 1.0 {
                for v in &mut layer.data {
                    *v *= c.global_alpha;
                }
            }
            // The source image is 8-bit like the bitmap (browsers filter
            // 8-bit layers): the same rounding as two plain draws.
            quantize(&mut layer.data);
            if !layer.data.is_empty() {
                f.apply(&mut layer.data, rect.width() as usize, rect.height() as usize);
            }
            let plain = Composite { op: c.op, global_alpha: 1.0, clip: c.clip, shadow: c.shadow, filter: None };
            self.composite_layer(&layer, &plain);
            return;
        }
        if let Some(sh) = c.shadow {
            self.fill_shadow(raster, lines, rule, &bounds, shader, c, sh);
        }
        let unbounded = c.op.is_unbounded();
        let mut region = if unbounded { self.rect() } else { bounds.to_irect().intersect(&self.rect()) };
        if let Some(clip) = c.clip {
            region = region.intersect(&clip.bounds);
        }
        if region.is_empty() {
            return;
        }
        let w = self.width as usize;
        let (ox, oy) = (self.x, self.y);
        let opaque = self.opaque;
        let op = c.op;
        let ga = c.global_alpha;
        let clip = c.clip;
        let data = Arc::make_mut(&mut self.data);
        let mut colors: Vec<[f32; 4]> = vec![[0.0; 4]; region.width() as usize];
        raster.rasterize(lines, rule, region, unbounded, &mut |y, x0, cov| {
            let n = cov.len();
            shader.shade(x0, y, &mut colors[..n]);
            let row = (y - oy) as usize * w;
            for i in 0..n {
                let x = (x0 - ox) as usize + i;
                let k = clip.map_or(1.0, |cl| cl.at(x0 + i as i32, y));
                if k <= 0.0 {
                    continue;
                }
                let a = cov[i] * ga;
                if a <= 0.0 && !unbounded {
                    continue;
                }
                let s = colors[i];
                let s = [s[0] * a, s[1] * a, s[2] * a, s[3] * a];
                let px = &mut data[(row + x) * 4..(row + x) * 4 + 4];
                write_pixel(px, op, s, k, opaque);
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn fill_shadow(&mut self, raster: &mut Rasterizer, lines: &[Line], rule: FillRule, bounds: &Bounds, shader: &Shader<'_>, c: &Composite<'_>, sh: Shadow) {
        if bounds.is_empty() || sh.color[3] <= 0.0 {
            return;
        }
        let margin = (sh.sigma * 3.0).ceil() as i32 + 1;
        // The part of the shadow that can land on the surface.
        let target = self.rect().intersect(&c.clip.map_or(self.rect(), |cl| cl.bounds));
        let shifted = Bounds { x0: bounds.x0 + sh.offset_x, y0: bounds.y0 + sh.offset_y, x1: bounds.x1 + sh.offset_x, y1: bounds.y1 + sh.offset_y };
        let layer_rect = shifted.to_irect().outset(margin).intersect(&target.outset(margin));
        if layer_rect.is_empty() {
            if !c.op.is_unbounded() {
                return;
            }
        }
        let (lw, lh) = (layer_rect.width().max(0) as usize, layer_rect.height().max(0) as usize);
        let mut alpha = vec![0.0f32; lw * lh];
        if lw > 0 && lh > 0 {
            let moved: Vec<Line> = lines
                .iter()
                .map(|l| Line { x0: l.x0 + sh.offset_x as f32, y0: l.y0 + sh.offset_y as f32, x1: l.x1 + sh.offset_x as f32, y1: l.y1 + sh.offset_y as f32 })
                .collect();
            let (ox, oy) = (sh.offset_x.round() as i32, sh.offset_y.round() as i32);
            let mut colors: Vec<[f32; 4]> = vec![[0.0; 4]; lw];
            raster.rasterize(&moved, rule, layer_rect, false, &mut |y, x0, cov| {
                let n = cov.len();
                shader.shade(x0 - ox, y - oy, &mut colors[..n]);
                let row = (y - layer_rect.y0) as usize * lw;
                for i in 0..n {
                    alpha[row + (x0 - layer_rect.x0) as usize + i] = cov[i] * colors[i][3];
                }
            });
            gaussian_blur(&mut alpha, lw, lh, sh.sigma);
        }
        let unbounded = c.op.is_unbounded();
        let region = if unbounded { target } else { layer_rect.intersect(&target) };
        if region.is_empty() {
            return;
        }
        let opaque = self.opaque;
        let index: Vec<usize> = (region.y0..region.y1).map(|y| self.index(region.x0, y)).collect();
        let data = Arc::make_mut(&mut self.data);
        for y in region.y0..region.y1 {
            let row = index[(y - region.y0) as usize];
            for x in region.x0..region.x1 {
                let inside = layer_rect.x0 <= x && x < layer_rect.x1 && layer_rect.y0 <= y && y < layer_rect.y1;
                let a = if inside { alpha[(y - layer_rect.y0) as usize * lw + (x - layer_rect.x0) as usize] } else { 0.0 };
                if a <= 0.0 && !unbounded {
                    continue;
                }
                let i = row + (x - region.x0) as usize;
                let k = c.clip.map_or(1.0, |cl| cl.at(x, y));
                if k <= 0.0 {
                    continue;
                }
                let f = a * c.global_alpha;
                let s = [sh.color[0] * f, sh.color[1] * f, sh.color[2] * f, sh.color[3] * f];
                write_pixel(&mut data[i * 4..i * 4 + 4], c.op, s, k, opaque);
            }
        }
    }

    /// Renders `lines` with `shader` into `layer` (paint times coverage).
    pub fn render_into(raster: &mut Rasterizer, lines: &[Line], rule: FillRule, shader: &Shader<'_>, layer: &mut Layer) {
        let rect = layer.rect;
        if rect.is_empty() {
            return;
        }
        let lw = rect.width() as usize;
        let mut colors: Vec<[f32; 4]> = vec![[0.0; 4]; lw];
        let data = &mut layer.data;
        raster.rasterize(lines, rule, rect, false, &mut |y, x0, cov| {
            let n = cov.len();
            shader.shade(x0, y, &mut colors[..n]);
            let row = (y - rect.y0) as usize * lw + (x0 - rect.x0) as usize;
            for i in 0..n {
                let k = cov[i];
                let o = (row + i) * 4;
                for j in 0..4 {
                    data[o + j] = colors[i][j] * k;
                }
            }
        });
    }

    /// Composites a premultiplied layer onto the bitmap: its shadow first,
    /// then the layer itself, both with `globalAlpha`, the operator and the
    /// clip (HTML drawing model steps for images B and C).
    pub fn composite_layer(&mut self, layer: &Layer, c: &Composite<'_>) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        let target = self.rect().intersect(&c.clip.map_or(self.rect(), |cl| cl.bounds));
        if let Some(sh) = c.shadow.filter(|s| s.color[3] > 0.0) {
            let margin = (sh.sigma * 3.0).ceil() as i32 + 1;
            let (ox, oy) = (sh.offset_x.round() as i32, sh.offset_y.round() as i32);
            let srect = layer.rect.offset(ox, oy).outset(margin).intersect(&target.outset(margin));
            let (sw, shh) = (srect.width().max(0) as usize, srect.height().max(0) as usize);
            let mut alpha = vec![0.0f32; sw * shh];
            for y in 0..shh {
                for x in 0..sw {
                    alpha[y * sw + x] = layer.at(srect.x0 + x as i32 - ox, srect.y0 + y as i32 - oy)[3];
                }
            }
            gaussian_blur(&mut alpha, sw, shh, sh.sigma);
            let shadow_layer = Layer {
                rect: srect,
                data: alpha.iter().flat_map(|a| [sh.color[0] * a, sh.color[1] * a, sh.color[2] * a, sh.color[3] * a]).collect(),
            };
            self.blit_layer(&shadow_layer, c, target);
        }
        self.blit_layer(layer, c, target);
    }

    fn blit_layer(&mut self, layer: &Layer, c: &Composite<'_>, target: IRect) {
        let unbounded = c.op.is_unbounded();
        let region = if unbounded { target } else { layer.rect.intersect(&target) };
        if region.is_empty() {
            return;
        }
        let opaque = self.opaque;
        let index: Vec<usize> = (region.y0..region.y1).map(|y| self.index(region.x0, y)).collect();
        let data = Arc::make_mut(&mut self.data);
        for y in region.y0..region.y1 {
            let row = index[(y - region.y0) as usize];
            for x in region.x0..region.x1 {
                let s = layer.at(x, y);
                if s[3] <= 0.0 && !unbounded && s[0] <= 0.0 && s[1] <= 0.0 && s[2] <= 0.0 {
                    continue;
                }
                let i = row + (x - region.x0) as usize;
                let k = c.clip.map_or(1.0, |cl| cl.at(x, y));
                if k <= 0.0 {
                    continue;
                }
                let ga = c.global_alpha;
                let s = [s[0] * ga, s[1] * ga, s[2] * ga, s[3] * ga];
                write_pixel(&mut data[i * 4..i * 4 + 4], c.op, s, k, opaque);
            }
        }
    }

    /// The bitmap as a premultiplied f32 layer (for `endLayer`).
    #[must_use]
    pub fn to_layer(&self) -> Layer {
        Layer { rect: self.rect(), data: self.data.iter().map(|v| *v as f32 / 255.0).collect() }
    }

    /// `clearRect`: clears the covered pixels within the clip.
    pub fn clear(&mut self, raster: &mut Rasterizer, lines: &[Line], clip: Option<&Clip>) {
        let mut bounds = Bounds::default();
        for l in lines {
            bounds.add(l.x0 as f64, l.y0 as f64);
            bounds.add(l.x1 as f64, l.y1 as f64);
        }
        let mut region = bounds.to_irect().intersect(&self.rect());
        if let Some(cl) = clip {
            region = region.intersect(&cl.bounds);
        }
        if region.is_empty() {
            return;
        }
        let w = self.width as usize;
        let (ox, oy) = (self.x, self.y);
        let opaque = self.opaque;
        let data = Arc::make_mut(&mut self.data);
        raster.rasterize(lines, FillRule::NonZero, region, false, &mut |y, x0, cov| {
            let row = (y - oy) as usize * w;
            for (i, c) in cov.iter().enumerate() {
                let x = (x0 - ox) as usize + i;
                let k = clip.map_or(1.0, |cl| cl.at(x0 + i as i32, y)) * c;
                if k <= 0.0 {
                    continue;
                }
                let px = &mut data[(row + x) * 4..(row + x) * 4 + 4];
                let target = if opaque { [0.0, 0.0, 0.0, 1.0] } else { [0.0; 4] };
                for j in 0..4 {
                    let d = px[j] as f32 / 255.0;
                    px[j] = to_u8(d + (target[j] - d) * k);
                }
            }
        });
    }

    /// Un-premultiplied RGBA8 copy of a rectangle; pixels outside the
    /// bitmap are transparent black.
    #[must_use]
    pub fn read(&self, sx: i64, sy: i64, w: u32, h: u32) -> Vec<u8> {
        let (sx, sy) = (sx - self.x as i64, sy - self.y as i64);
        let mut out = vec![0u8; w as usize * h as usize * 4];
        let (cw, ch) = (self.width as i64, self.height as i64);
        let x0 = sx.max(0);
        let x1 = (sx + w as i64).min(cw);
        if x1 <= x0 {
            return out;
        }
        for row in 0..h as i64 {
            let y = sy + row;
            if y < 0 || y >= ch {
                continue;
            }
            for x in x0..x1 {
                let s = ((y * cw + x) * 4) as usize;
                let d = ((row * w as i64 + (x - sx)) * 4) as usize;
                let p = &self.data[s..s + 4];
                out[d..d + 4].copy_from_slice(&unpremultiply(p));
            }
        }
        out
    }

    /// Writes un-premultiplied RGBA8 pixels (`putImageData`), unaffected by
    /// clip, alpha and compositing. `rect` is the dirty rectangle within the
    /// source image.
    pub fn write(&mut self, src: &[u8], src_w: u32, dx: i64, dy: i64, rect: IRect) {
        let (dx, dy) = (dx - self.x as i64, dy - self.y as i64);
        let (cw, ch) = (self.width as i64, self.height as i64);
        let opaque = self.opaque;
        let data = Arc::make_mut(&mut self.data);
        for y in rect.y0 as i64..rect.y1 as i64 {
            let ty = dy + y;
            if ty < 0 || ty >= ch {
                continue;
            }
            for x in rect.x0 as i64..rect.x1 as i64 {
                let tx = dx + x;
                if tx < 0 || tx >= cw {
                    continue;
                }
                let s = ((y * src_w as i64 + x) * 4) as usize;
                let d = ((ty * cw + tx) * 4) as usize;
                let mut p = premultiply(&src[s..s + 4]);
                if opaque {
                    p[3] = 255;
                }
                data[d..d + 4].copy_from_slice(&p);
            }
        }
    }

    /// The bitmap as un-premultiplied RGBA8 (display, PNG export).
    #[must_use]
    pub fn to_unpremultiplied(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.data.len()];
        for (o, p) in out.chunks_exact_mut(4).zip(self.data.chunks_exact(4)) {
            o.copy_from_slice(&unpremultiply(p));
        }
        out
    }

    /// Intersects `clip` (or the whole surface) with the coverage of a path.
    #[must_use]
    pub fn clip_with(&self, raster: &mut Rasterizer, lines: &[Line], rule: FillRule, clip: Option<&Clip>) -> Clip {
        let w = self.width as usize;
        let mut mask = vec![0u8; w * self.height as usize];
        let mut region = self.rect();
        if let Some(cl) = clip {
            region = region.intersect(&cl.bounds);
        }
        let mut bounds = IRect::default();
        let (ox, oy) = (self.x, self.y);
        if !region.is_empty() {
            raster.rasterize(lines, rule, region, false, &mut |y, x0, cov| {
                let row = (y - oy) as usize * w;
                let mut any = false;
                for (i, c) in cov.iter().enumerate() {
                    let x = (x0 - ox) as usize + i;
                    let prev = clip.map_or(255.0, |cl| (cl.at(x0 + i as i32, y) * 255.0).round());
                    let v = (prev * c).round().clamp(0.0, 255.0) as u8;
                    mask[row + x] = v;
                    any |= v > 0;
                }
                if any {
                    bounds = bounds.union(&IRect::new(x0, y, x0 + cov.len() as i32, y + 1));
                }
            });
        }
        Clip { mask: Arc::new(mask), bounds, x: self.x, y: self.y, width: self.width, parts: Vec::new() }
    }
}

/// Rounds premultiplied f32 channels to 8-bit steps.
pub fn quantize(data: &mut [f32]) {
    for v in data {
        *v = (*v * 255.0).round().clamp(0.0, 255.0) / 255.0;
    }
}

#[inline]
fn to_u8(v: f32) -> u8 {
    (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

#[inline]
fn write_pixel(px: &mut [u8], op: CompositeOp, s: [f32; 4], k: f32, opaque: bool) {
    let d = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0, px[3] as f32 / 255.0];
    let mut r = composite(op, s, d);
    if k < 1.0 {
        for j in 0..4 {
            r[j] = d[j] + (r[j] - d[j]) * k;
        }
    }
    if opaque {
        r[3] = 1.0;
    } else {
        // Keep premultiplied colors valid after rounding.
        let a = r[3].clamp(0.0, 1.0);
        for v in r.iter_mut().take(3) {
            *v = v.clamp(0.0, a);
        }
    }
    px[0] = to_u8(r[0]);
    px[1] = to_u8(r[1]);
    px[2] = to_u8(r[2]);
    px[3] = to_u8(r[3]);
}

/// Premultiplies an RGBA8 pixel (rounding like Skia's `SkMulDiv255Round`).
#[inline]
#[must_use]
pub fn premultiply(p: &[u8]) -> [u8; 4] {
    let a = p[3] as u32;
    let m = |c: u8| {
        let prod = c as u32 * a + 128;
        ((prod + (prod >> 8)) >> 8) as u8
    };
    [m(p[0]), m(p[1]), m(p[2]), p[3]]
}

/// Un-premultiplies an RGBA8 pixel.
#[inline]
#[must_use]
pub fn unpremultiply(p: &[u8]) -> [u8; 4] {
    let a = p[3] as u32;
    if a == 0 {
        return [0, 0, 0, 0];
    }
    if a == 255 {
        return [p[0], p[1], p[2], 255];
    }
    let u = |c: u8| ((c as u32 * 255 + a / 2) / a).min(255) as u8;
    [u(p[0]), u(p[1]), u(p[2]), p[3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_round_trip_is_stable() {
        // getImageData -> putImageData -> getImageData must not drift.
        for a in 0..=255u32 {
            for c in 0..=255u32 {
                let p = premultiply(&[c as u8, 0, 0, a as u8]);
                let u = unpremultiply(&p);
                let p2 = premultiply(&u);
                let u2 = unpremultiply(&p2);
                assert_eq!(u, u2, "c={c} a={a}");
            }
        }
    }

    #[test]
    fn opaque_surface_starts_black() {
        let s = Surface::new(2, 1, true).unwrap();
        assert_eq!(s.read(0, 0, 2, 1), vec![0, 0, 0, 255, 0, 0, 0, 255]);
    }
}
