//! Intra and inter prediction (8-bit).

use super::tables::*;

#[inline(always)]
fn avg2(a: u8, b: u8) -> u8 {
    ((a as u32 + b as u32 + 1) >> 1) as u8
}

#[inline(always)]
fn avg3(a: u8, b: u8, c: u8) -> u8 {
    ((a as u32 + 2 * b as u32 + c as u32 + 2) >> 2) as u8
}

/// Intra prediction of one transform block at plane pixel `(x0, y0)`.
/// `fw`/`fh`: the plane's decoded (8-aligned) size; edges beyond it
/// repeat the last pixel. Writes the prediction into `p`.
#[allow(clippy::too_many_arguments)]
pub fn predict_intra(
    p: &mut [u8],
    stride: usize,
    x0: usize,
    y0: usize,
    fw: usize,
    fh: usize,
    mode: u8,
    tx: u8,
    have_top: bool,
    have_left: bool,
    have_right: bool,
) {
    let bs = 4usize << tx;
    let need_left = matches!(mode, DC_PRED | H_PRED | D135_PRED | D117_PRED | D153_PRED | D207_PRED | TM_PRED);
    let need_above = matches!(mode, DC_PRED | V_PRED | D135_PRED | D117_PRED | D153_PRED | TM_PRED);
    let need_above_right = matches!(mode, D45_PRED | D63_PRED);
    let mut left = [129u8; 32];
    // above[0] is the top-left pixel, above[1..] the row above.
    let mut above = [127u8; 65];
    if need_left && have_left {
        let n = if y0 + bs <= fh { bs } else { fh.saturating_sub(y0).max(1) };
        for (i, l) in left.iter_mut().enumerate().take(n) {
            *l = p[(y0 + i) * stride + x0 - 1];
        }
        let last = left[n - 1];
        for l in left.iter_mut().take(bs).skip(n) {
            *l = last;
        }
    }
    if need_above || need_above_right {
        if have_top {
            let row = (y0 - 1) * stride;
            let avail = fw.saturating_sub(x0).max(1);
            let want = if need_above_right { 2 * bs } else { bs };
            let copy = if need_above_right && !(bs == 4 && have_right) { bs.min(avail) } else { want.min(avail) };
            above[1..1 + copy].copy_from_slice(&p[row + x0..row + x0 + copy]);
            let last = above[copy];
            for a in above.iter_mut().take(want + 1).skip(copy + 1) {
                *a = last;
            }
            above[0] = if have_left { p[row + x0 - 1] } else { 129 };
        } else {
            above[0] = 127;
        }
    }
    let a = &above[1..];
    let tl = above[0];
    let at = |r: usize, c: usize| (y0 + r) * stride + x0 + c;
    match mode {
        DC_PRED => {
            let v = match (have_left, have_top) {
                (true, true) => {
                    let s: u32 = a[..bs].iter().map(|v| *v as u32).sum::<u32>() + left[..bs].iter().map(|v| *v as u32).sum::<u32>();
                    ((s + bs as u32) / (2 * bs as u32)) as u8
                }
                (true, false) => ((left[..bs].iter().map(|v| *v as u32).sum::<u32>() + bs as u32 / 2) / bs as u32) as u8,
                (false, true) => ((a[..bs].iter().map(|v| *v as u32).sum::<u32>() + bs as u32 / 2) / bs as u32) as u8,
                (false, false) => 128,
            };
            for r in 0..bs {
                p[at(r, 0)..at(r, 0) + bs].fill(v);
            }
        }
        V_PRED => {
            for r in 0..bs {
                p[at(r, 0)..at(r, 0) + bs].copy_from_slice(&a[..bs]);
            }
        }
        H_PRED => {
            for (r, &l) in left.iter().enumerate().take(bs) {
                p[at(r, 0)..at(r, 0) + bs].fill(l);
            }
        }
        TM_PRED => {
            for r in 0..bs {
                for c in 0..bs {
                    p[at(r, c)] = (left[r] as i32 + a[c] as i32 - tl as i32).clamp(0, 255) as u8;
                }
            }
        }
        D45_PRED => {
            for r in 0..bs {
                for c in 0..bs {
                    p[at(r, c)] = if r + c + 2 < bs * 2 { avg3(a[r + c], a[r + c + 1], a[r + c + 2]) } else { a[bs * 2 - 1] };
                }
            }
        }
        D63_PRED => {
            for r in 0..bs {
                for c in 0..bs {
                    let k = r / 2 + c;
                    p[at(r, c)] = if r & 1 != 0 { avg3(a[k], a[k + 1], a[k + 2]) } else { avg2(a[k], a[k + 1]) };
                }
            }
        }
        D117_PRED => {
            let mut b = [[0u8; 32]; 32];
            for c in 0..bs {
                b[0][c] = avg2(if c == 0 { tl } else { a[c - 1] }, a[c]);
            }
            b[1][0] = avg3(left[0], tl, a[0]);
            for c in 1..bs {
                b[1][c] = avg3(if c == 1 { tl } else { a[c - 2] }, a[c - 1], a[c]);
            }
            if bs > 2 {
                b[2][0] = avg3(tl, left[0], left[1]);
                for r in 3..bs {
                    b[r][0] = avg3(left[r - 3], left[r - 2], left[r - 1]);
                }
            }
            for r in 2..bs {
                for c in 1..bs {
                    b[r][c] = b[r - 2][c - 1];
                }
            }
            for r in 0..bs {
                p[at(r, 0)..at(r, 0) + bs].copy_from_slice(&b[r][..bs]);
            }
        }
        D135_PRED => {
            let mut b = [[0u8; 32]; 32];
            b[0][0] = avg3(left[0], tl, a[0]);
            for c in 1..bs {
                b[0][c] = avg3(if c == 1 { tl } else { a[c - 2] }, a[c - 1], a[c]);
            }
            b[1][0] = avg3(tl, left[0], left[1]);
            for r in 2..bs {
                b[r][0] = avg3(left[r - 2], left[r - 1], left[r]);
            }
            for r in 1..bs {
                for c in 1..bs {
                    b[r][c] = b[r - 1][c - 1];
                }
            }
            for r in 0..bs {
                p[at(r, 0)..at(r, 0) + bs].copy_from_slice(&b[r][..bs]);
            }
        }
        D153_PRED => {
            let mut b = [[0u8; 32]; 32];
            b[0][0] = avg2(tl, left[0]);
            for r in 1..bs {
                b[r][0] = avg2(left[r - 1], left[r]);
            }
            b[0][1] = avg3(left[0], tl, a[0]);
            b[1][1] = avg3(tl, left[0], left[1]);
            for r in 2..bs {
                b[r][1] = avg3(left[r - 2], left[r - 1], left[r]);
            }
            for c in 0..bs - 2 {
                b[0][c + 2] = avg3(if c == 0 { tl } else { a[c - 1] }, a[c], a[c + 1]);
            }
            for r in 1..bs {
                for c in 0..bs - 2 {
                    b[r][c + 2] = b[r - 1][c];
                }
            }
            for r in 0..bs {
                p[at(r, 0)..at(r, 0) + bs].copy_from_slice(&b[r][..bs]);
            }
        }
        _ => {
            // D207
            let mut b = [[0u8; 32]; 32];
            for r in 0..bs - 1 {
                b[r][0] = avg2(left[r], left[r + 1]);
            }
            b[bs - 1][0] = left[bs - 1];
            for r in 0..bs - 2 {
                b[r][1] = avg3(left[r], left[r + 1], left[r + 2]);
            }
            b[bs - 2][1] = avg3(left[bs - 2], left[bs - 1], left[bs - 1]);
            b[bs - 1][1] = left[bs - 1];
            for c in 0..bs - 2 {
                b[bs - 1][c + 2] = left[bs - 1];
            }
            for r in (0..bs - 1).rev() {
                for c in 0..bs - 2 {
                    b[r][c + 2] = b[r + 1][c];
                }
            }
            for r in 0..bs {
                p[at(r, 0)..at(r, 0) + bs].copy_from_slice(&b[r][..bs]);
            }
        }
    }
}

/// A reference plane for motion compensation: pixels outside
/// `w` x `h` repeat the nearest edge pixel.
pub struct RefPlane<'a> {
    pub data: &'a [u8],
    pub stride: usize,
    pub w: i32,
    pub h: i32,
}

impl RefPlane<'_> {
    #[inline(always)]
    fn at(&self, x: i32, y: i32) -> u8 {
        let x = x.clamp(0, self.w - 1) as usize;
        let y = y.clamp(0, self.h - 1) as usize;
        self.data[y * self.stride + x]
    }
}

/// The 8-tap sub-pixel prediction of a `w` x `h` block (libvpx
/// `vpx_convolve8` with steps; the horizontal pass rounds and clips to
/// 8 bits). `x0`/`y0`: the integer top-left position in the reference,
/// `fx`/`fy`: the 1/16 phase, `xs`/`ys`: the 1/16 steps (16 unscaled).
/// With `avg` the result is averaged into `dst` (compound prediction).
#[allow(clippy::too_many_arguments)]
pub fn predict_inter(
    r: &RefPlane,
    dst: &mut [u8],
    doff: usize,
    dstride: usize,
    x0: i32,
    y0: i32,
    fx: i32,
    fy: i32,
    xs: i32,
    ys: i32,
    w: usize,
    h: usize,
    filter: &[[i16; 8]; 16],
    avg: bool,
) {
    if xs == 16 && ys == 16 {
        predict_unscaled(r, dst, doff, dstride, x0, y0, fx, fy, w, h, filter, avg);
    } else {
        predict_two_pass(r, dst, doff, dstride, x0, y0, fx, fy, xs, ys, w, h, filter, avg);
    }
}

/// The general two-pass filter for any step (scaled references).
#[allow(clippy::too_many_arguments)]
fn predict_two_pass(
    r: &RefPlane,
    dst: &mut [u8],
    doff: usize,
    dstride: usize,
    x0: i32,
    y0: i32,
    fx: i32,
    fy: i32,
    xs: i32,
    ys: i32,
    w: usize,
    h: usize,
    filter: &[[i16; 8]; 16],
    avg: bool,
) {
    let inter_h = ((((h as i32 - 1) * ys + fy) >> 4) + 8) as usize;
    let mut tmp = [0u8; 64 * 135];
    // Is the whole source area inside the plane? Then index directly.
    let x_last = x0 + (((w as i32 - 1) * xs + fx) >> 4) + 4;
    let inside = x0 - 3 >= 0 && y0 - 3 >= 0 && x_last < r.w && y0 - 3 + inter_h as i32 <= r.h;
    for ty in 0..inter_h {
        let sy = y0 - 3 + ty as i32;
        let mut xq = fx;
        for tx in 0..w {
            let sx = x0 + (xq >> 4);
            let k = &filter[(xq & 15) as usize];
            let mut sum = 0i32;
            if inside {
                let base = sy as usize * r.stride + (sx - 3) as usize;
                let row = &r.data[base..base + 8];
                for t in 0..8 {
                    sum += row[t] as i32 * k[t] as i32;
                }
            } else {
                for t in 0..8 {
                    sum += r.at(sx - 3 + t as i32, sy) as i32 * k[t] as i32;
                }
            }
            tmp[ty * w + tx] = ((sum + 64) >> 7).clamp(0, 255) as u8;
            xq += xs;
        }
    }
    let mut yq = fy;
    for oy in 0..h {
        let ty = (yq >> 4) as usize;
        let k = &filter[(yq & 15) as usize];
        let drow = doff + oy * dstride;
        for ox in 0..w {
            let mut sum = 0i32;
            for t in 0..8 {
                sum += tmp[(ty + t) * w + ox] as i32 * k[t] as i32;
            }
            let v = ((sum + 64) >> 7).clamp(0, 255) as u8;
            let d = &mut dst[drow + ox];
            *d = if avg { ((*d as u32 + v as u32 + 1) >> 1) as u8 } else { v };
        }
        yq += ys;
    }
}

/// Writes `v` to `d`, or averages it in (compound prediction).
#[inline(always)]
fn put(d: &mut u8, v: u8, avg: bool) {
    *d = if avg { ((*d as u32 + v as u32 + 1) >> 1) as u8 } else { v };
}

#[inline(always)]
fn tap8(src: &[u8], o: usize, step: usize, k: &[i16; 8]) -> u8 {
    let mut sum = 64i32;
    for t in 0..8 {
        sum += src[o + t * step] as i32 * k[t] as i32;
    }
    (sum >> 7).clamp(0, 255) as u8
}

/// The unscaled prediction (steps of 16): a copy for whole-pixel motion,
/// one filter pass when only one phase is fractional, both passes
/// otherwise. Filter phase 0 is the identity, so every path gives the
/// bytes of the two-pass filter. Blocks that reach outside the plane are
/// first copied with clamped edges.
#[allow(clippy::too_many_arguments)]
fn predict_unscaled(
    r: &RefPlane,
    dst: &mut [u8],
    doff: usize,
    dstride: usize,
    x0: i32,
    y0: i32,
    fx: i32,
    fy: i32,
    w: usize,
    h: usize,
    filter: &[[i16; 8]; 16],
    avg: bool,
) {
    // The source area with the filter taps: (w + 7) x (h + 7) from
    // (x0 - 3, y0 - 3).
    let (sw, sh) = (w + 7, h + 7);
    let mut edge = [0u8; 71 * 71];
    let inside = x0 - 3 >= 0 && y0 - 3 >= 0 && x0 + w as i32 + 4 <= r.w && y0 + h as i32 + 4 <= r.h;
    let (src, base, stride): (&[u8], usize, usize) = if inside {
        (r.data, (y0 - 3) as usize * r.stride + (x0 - 3) as usize, r.stride)
    } else {
        for ty in 0..sh {
            for tx in 0..sw {
                edge[ty * sw + tx] = r.at(x0 - 3 + tx as i32, y0 - 3 + ty as i32);
            }
        }
        (&edge[..], 0, sw)
    };
    // The block's own top-left pixel.
    let org = base + 3 * stride + 3;
    match (fx != 0, fy != 0) {
        (false, false) => {
            for y in 0..h {
                let s = &src[org + y * stride..org + y * stride + w];
                let d = &mut dst[doff + y * dstride..doff + y * dstride + w];
                if avg {
                    for (d, &v) in d.iter_mut().zip(s) {
                        put(d, v, true);
                    }
                } else {
                    d.copy_from_slice(s);
                }
            }
        }
        (true, false) => {
            let k = &filter[fx as usize];
            for y in 0..h {
                let row = org + y * stride - 3;
                for x in 0..w {
                    let v = tap8(src, row + x, 1, k);
                    put(&mut dst[doff + y * dstride + x], v, avg);
                }
            }
        }
        (false, true) => {
            let k = &filter[fy as usize];
            for y in 0..h {
                let col = org + y * stride - 3 * stride;
                for x in 0..w {
                    let v = tap8(src, col + x, stride, k);
                    put(&mut dst[doff + y * dstride + x], v, avg);
                }
            }
        }
        (true, true) => {
            let kx = &filter[fx as usize];
            let ky = &filter[fy as usize];
            let mut tmp = [0u8; 64 * 71];
            for ty in 0..sh {
                let row = base + ty * stride;
                for x in 0..w {
                    tmp[ty * w + x] = tap8(src, row + x, 1, kx);
                }
            }
            for y in 0..h {
                for x in 0..w {
                    let v = tap8(&tmp, y * w + x, w, ky);
                    put(&mut dst[doff + y * dstride + x], v, avg);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The unscaled fast paths give the bytes of the general two-pass
    /// filter, inside the plane and across its edges, for every phase.
    #[test]
    fn unscaled_paths_match_the_two_pass_filter() {
        let (pw, ph) = (40usize, 36usize);
        let mut seed = 0x1234_5678u32;
        let data: Vec<u8> = (0..pw * ph)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let r = RefPlane { data: &data, stride: pw, w: 37, h: 33 };
        let filter = &crate::decoder::tables::FILTERS[0];
        for &(w, h) in &[(4usize, 4usize), (8, 4), (16, 16), (32, 8)] {
            for &(x0, y0) in &[(10i32, 9i32), (-5, 3), (30, 28), (-2, -6)] {
                for phase in 0..16 * 16 {
                    let (fx, fy) = (phase % 16, phase / 16);
                    for avg in [false, true] {
                        let mut a = vec![77u8; 64 * 64];
                        let mut b = a.clone();
                        predict_unscaled(&r, &mut a, 0, 64, x0, y0, fx, fy, w, h, filter, avg);
                        predict_two_pass(&r, &mut b, 0, 64, x0, y0, fx, fy, 16, 16, w, h, filter, avg);
                        assert!(a == b, "{w}x{h} at ({x0},{y0}) phase ({fx},{fy}) avg {avg}");
                    }
                }
            }
        }
    }
}
