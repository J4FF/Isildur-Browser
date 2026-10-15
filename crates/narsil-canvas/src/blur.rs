//! Separable Gaussian blur for shadows and filters.

/// Blurs a single-channel `width x height` buffer in place with standard
/// deviation `sigma` (in pixels). Uses an exact separable kernel for small
/// sigmas and three box passes (a close Gaussian approximation) for large
/// ones, so the cost stays linear in the area.
pub fn gaussian_blur(buf: &mut [f32], width: usize, height: usize, sigma: f64) {
    if !(sigma > 0.0) || width == 0 || height == 0 {
        return;
    }
    let mut tmp = vec![0.0f32; buf.len()];
    if sigma < 8.0 {
        let kernel = kernel(sigma);
        convolve_rows(buf, &mut tmp, width, height, &kernel);
        convolve_cols(&tmp, buf, width, height, &kernel);
    } else {
        for r in box_radii(sigma) {
            box_rows(buf, &mut tmp, width, height, r);
            box_cols(&tmp, buf, width, height, r);
        }
    }
}

/// Blurs a single-channel buffer with separate standard deviations along
/// x and y (either may be zero: that axis stays sharp).
pub fn gaussian_blur_xy(buf: &mut [f32], width: usize, height: usize, sx: f64, sy: f64) {
    if width == 0 || height == 0 {
        return;
    }
    let mut tmp = vec![0.0f32; buf.len()];
    if sx > 0.0 {
        if sx < 8.0 {
            convolve_rows(buf, &mut tmp, width, height, &kernel(sx));
            buf.copy_from_slice(&tmp);
        } else {
            for r in box_radii(sx) {
                box_rows(buf, &mut tmp, width, height, r);
                buf.copy_from_slice(&tmp);
            }
        }
    }
    if sy > 0.0 {
        if sy < 8.0 {
            convolve_cols(buf, &mut tmp, width, height, &kernel(sy));
            buf.copy_from_slice(&tmp);
        } else {
            for r in box_radii(sy) {
                box_cols(buf, &mut tmp, width, height, r);
                buf.copy_from_slice(&tmp);
            }
        }
    }
}

/// Blurs four interleaved channels (premultiplied RGBA) in place.
pub fn gaussian_blur_rgba(buf: &mut [f32], width: usize, height: usize, sigma: f64) {
    if !(sigma > 0.0) {
        return;
    }
    let n = width * height;
    let mut chan = vec![0.0f32; n];
    for c in 0..4 {
        for i in 0..n {
            chan[i] = buf[i * 4 + c];
        }
        gaussian_blur(&mut chan, width, height, sigma);
        for i in 0..n {
            buf[i * 4 + c] = chan[i];
        }
    }
}

fn kernel(sigma: f64) -> Vec<f32> {
    let r = (sigma * 3.0).ceil() as usize;
    let mut k: Vec<f64> = (0..=2 * r).map(|i| {
        let x = i as f64 - r as f64;
        (-(x * x) / (2.0 * sigma * sigma)).exp()
    }).collect();
    let sum: f64 = k.iter().sum();
    for v in &mut k {
        *v /= sum;
    }
    k.into_iter().map(|v| v as f32).collect()
}

fn convolve_rows(src: &[f32], dst: &mut [f32], w: usize, h: usize, k: &[f32]) {
    let r = k.len() / 2;
    for y in 0..h {
        let row = &src[y * w..y * w + w];
        let out = &mut dst[y * w..y * w + w];
        for (x, o) in out.iter_mut().enumerate() {
            let mut s = 0.0f32;
            let lo = x.saturating_sub(r);
            let hi = (x + r).min(w - 1);
            for xi in lo..=hi {
                s += row[xi] * k[xi + r - x];
            }
            *o = s;
        }
    }
}

fn convolve_cols(src: &[f32], dst: &mut [f32], w: usize, h: usize, k: &[f32]) {
    let r = k.len() / 2;
    for y in 0..h {
        let lo = y.saturating_sub(r);
        let hi = (y + r).min(h - 1);
        let out = &mut dst[y * w..y * w + w];
        out.fill(0.0);
        for yi in lo..=hi {
            let f = k[yi + r - y];
            let row = &src[yi * w..yi * w + w];
            for (o, v) in out.iter_mut().zip(row) {
                *o += v * f;
            }
        }
    }
}

/// Box radii for three passes approximating a Gaussian of `sigma`.
fn box_radii(sigma: f64) -> [usize; 3] {
    let n = 3.0;
    let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m_ideal = (12.0 * sigma * sigma - n * (wl * wl) as f64 - 4.0 * n * wl as f64 - 3.0 * n) / (-4.0 * wl as f64 - 4.0);
    let m = m_ideal.round() as i64;
    let mut out = [0usize; 3];
    for (i, o) in out.iter_mut().enumerate() {
        let size = if (i as i64) < m { wl } else { wu };
        *o = ((size - 1) / 2).max(0) as usize;
    }
    out
}

fn box_rows(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let scale = 1.0 / (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..y * w + w];
        let out = &mut dst[y * w..y * w + w];
        let mut acc = 0.0f32;
        for xi in 0..=r.min(w - 1) {
            acc += row[xi];
        }
        for x in 0..w {
            out[x] = acc * scale;
            let add = x + r + 1;
            if add < w {
                acc += row[add];
            }
            if x >= r {
                acc -= row[x - r];
            }
        }
    }
}

fn box_cols(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let scale = 1.0 / (2 * r + 1) as f32;
    let mut acc = vec![0.0f32; w];
    for yi in 0..=r.min(h - 1) {
        for (a, v) in acc.iter_mut().zip(&src[yi * w..yi * w + w]) {
            *a += v;
        }
    }
    for y in 0..h {
        for x in 0..w {
            dst[y * w + x] = acc[x] * scale;
        }
        let add = y + r + 1;
        if add < h {
            for (a, v) in acc.iter_mut().zip(&src[add * w..add * w + w]) {
                *a += v;
            }
        }
        if y >= r {
            let sub = y - r;
            for (a, v) in acc.iter_mut().zip(&src[sub * w..sub * w + w]) {
                *a -= v;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_preserves_mass_away_from_borders() {
        let (w, h) = (160, 160);
        for sigma in [1.5, 12.0] {
            let mut b = vec![0.0f32; w * h];
            b[80 * w + 80] = 1.0;
            gaussian_blur(&mut b, w, h, sigma);
            let s: f32 = b.iter().sum();
            assert!((s - 1.0).abs() < 1e-3, "{sigma}: {s}");
            assert!(b[80 * w + 80] < 0.5);
        }
    }
}
