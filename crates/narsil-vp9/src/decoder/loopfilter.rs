//! The loop filter (VP9 specification §8.8; the edge masks, the order of
//! the passes and the filters of libvpx `vp9_loopfilter.c` and
//! `vpx_dsp/loopfilter.c`): per 64x64 superblock, all vertical edges of a
//! plane, then all horizontal edges.

use super::frame::{FrameBuf, ModeInfo};
use super::header::{LoopFilterParams, Segmentation};
use super::tables::*;

#[derive(Clone, Copy, Default)]
struct Thresh {
    mblim: u8,
    lim: u8,
    hev: u8,
}

struct Mask {
    left_y: [u64; 4],
    above_y: [u64; 4],
    int_4x4_y: u64,
    left_uv: [u16; 4],
    above_uv: [u16; 4],
    int_4x4_uv: u16,
    lfl_y: [u8; 64],
}

impl Default for Mask {
    fn default() -> Self {
        Mask { left_y: [0; 4], above_y: [0; 4], int_4x4_y: 0, left_uv: [0; 4], above_uv: [0; 4], int_4x4_uv: 0, lfl_y: [0; 64] }
    }
}

const LEFT_TX_Y: [u64; 4] = [u64::MAX, u64::MAX, 0x5555_5555_5555_5555, 0x1111_1111_1111_1111];
const ABOVE_TX_Y: [u64; 4] = [u64::MAX, u64::MAX, 0x00ff_00ff_00ff_00ff, 0x0000_00ff_0000_00ff];
const LEFT_TX_UV: [u16; 4] = [0xffff, 0xffff, 0x5555, 0x1111];
const ABOVE_TX_UV: [u16; 4] = [0xffff, 0xffff, 0x0f0f, 0x000f];
const LEFT_BORDER: u64 = 0x1111_1111_1111_1111;
const ABOVE_BORDER: u64 = 0x0000_00ff_0000_00ff;
const LEFT_BORDER_UV: u16 = 0x1111;
const ABOVE_BORDER_UV: u16 = 0x000f;

fn left_pred_y(bs: usize) -> u64 {
    (0..NUM_8X8_H[bs] as u64).fold(0, |m, r| m | 1 << (r * 8))
}
fn above_pred_y(bs: usize) -> u64 {
    (1u64 << NUM_8X8_W[bs]) - 1
}
fn size_y(bs: usize) -> u64 {
    (0..NUM_8X8_H[bs] as u64).fold(0, |m, r| m | above_pred_y(bs) << (r * 8))
}
fn uv_dims(bs: usize) -> (u32, u32) {
    (((NUM_8X8_W[bs] / 2).max(1)) as u32, ((NUM_8X8_H[bs] / 2).max(1)) as u32)
}
fn left_pred_uv(bs: usize) -> u16 {
    let (_, h) = uv_dims(bs);
    (0..h).fold(0, |m, r| m | 1 << (r * 4))
}
fn above_pred_uv(bs: usize) -> u16 {
    let (w, _) = uv_dims(bs);
    (1u16 << w) - 1
}
fn size_uv(bs: usize) -> u16 {
    let (_, h) = uv_dims(bs);
    (0..h).fold(0, |m, r| m | above_pred_uv(bs) << (r * 4))
}

/// The loop filter of one frame.
pub struct LoopFilter {
    lvl: [[[u8; 2]; 4]; 8],
    thresh: [Thresh; 64],
    ss: (bool, bool),
}

impl LoopFilter {
    pub fn new(lf: &LoopFilterParams, seg: &Segmentation, ss: (bool, bool)) -> Self {
        let mut thresh = [Thresh::default(); 64];
        let sharp = lf.sharpness as i32;
        for (lvl, t) in thresh.iter_mut().enumerate() {
            let mut inside = (lvl as i32) >> ((sharp > 0) as i32 + (sharp > 4) as i32);
            if sharp > 0 && inside > 9 - sharp {
                inside = 9 - sharp;
            }
            inside = inside.max(1);
            *t = Thresh { lim: inside as u8, mblim: (2 * (lvl as i32 + 2) + inside) as u8, hev: (lvl >> 4) as u8 };
        }
        let mut lvl = [[[0u8; 2]; 4]; 8];
        let base = lf.level as i32;
        let scale = 1 << (base >> 5);
        for (s, l) in lvl.iter_mut().enumerate() {
            let mut lvl_seg = base;
            if seg.active(s as u8, SEG_LVL_ALT_LF) {
                let d = seg.data(s as u8, SEG_LVL_ALT_LF) as i32;
                lvl_seg = (if seg.abs_delta { d } else { base + d }).clamp(0, 63);
            }
            if !lf.delta_enabled {
                *l = [[lvl_seg as u8; 2]; 4];
            } else {
                l[0][0] = (lvl_seg + lf.ref_deltas[0] as i32 * scale).clamp(0, 63) as u8;
                for rf in 1..4 {
                    for m in 0..2 {
                        l[rf][m] = (lvl_seg + lf.ref_deltas[rf] as i32 * scale + lf.mode_deltas[m] as i32 * scale).clamp(0, 63) as u8;
                    }
                }
            }
        }
        LoopFilter { lvl, thresh, ss }
    }

    fn level(&self, mi: &ModeInfo) -> u8 {
        self.lvl[mi.segment_id as usize][mi.ref_frame[0].max(0) as usize][MODE_LF_LUT[mi.mode as usize] as usize]
    }

    fn build_masks(&self, mi: &ModeInfo, shift_y: u32, shift_uv: u32, m: &mut Mask, with_uv: bool) {
        let bs = mi.sb_type as usize;
        let tx_y = mi.tx_size as usize;
        let tx_uv = if mi.sb_type < BLOCK_8X8 {
            0
        } else {
            let uvb = SS_SIZE[bs][self.ss.0 as usize][self.ss.1 as usize];
            tx_y.min(MAX_TXSIZE[uvb as usize] as usize)
        };
        let level = self.level(mi);
        if level == 0 {
            return;
        }
        let (w, h) = (NUM_8X8_W[bs] as usize, NUM_8X8_H[bs] as usize);
        for i in 0..h {
            let idx = shift_y as usize + i * 8;
            for v in m.lfl_y.iter_mut().skip(idx).take(w) {
                *v = level;
            }
        }
        m.above_y[tx_y] |= above_pred_y(bs) << shift_y;
        m.left_y[tx_y] |= left_pred_y(bs) << shift_y;
        if with_uv {
            m.above_uv[tx_uv] |= above_pred_uv(bs) << shift_uv;
            m.left_uv[tx_uv] |= left_pred_uv(bs) << shift_uv;
        }
        if mi.skip && mi.is_inter() {
            return;
        }
        m.above_y[tx_y] |= (size_y(bs) & ABOVE_TX_Y[tx_y]) << shift_y;
        m.left_y[tx_y] |= (size_y(bs) & LEFT_TX_Y[tx_y]) << shift_y;
        if with_uv {
            m.above_uv[tx_uv] |= (size_uv(bs) & ABOVE_TX_UV[tx_uv]) << shift_uv;
            m.left_uv[tx_uv] |= (size_uv(bs) & LEFT_TX_UV[tx_uv]) << shift_uv;
        }
        if tx_y == 0 {
            m.int_4x4_y |= size_y(bs) << shift_y;
        }
        if with_uv && tx_uv == 0 {
            m.int_4x4_uv |= size_uv(bs) << shift_uv;
        }
    }

    /// `vp9_setup_mask` for the superblock at (mi_row, mi_col).
    fn setup_mask(&self, mis: &[ModeInfo], mi_cols: usize, mi_rows: usize, mi_row: usize, mi_col: usize) -> Mask {
        let mut m = Mask::default();
        let at = |r: usize, c: usize| &mis[(mi_row + r) * mi_cols + mi_col + c];
        let max_rows = (mi_rows - mi_row).min(8);
        let max_cols = (mi_cols - mi_col).min(8);
        let shift_32_y = [0, 4, 32, 36];
        let shift_16_y = [0, 2, 16, 18];
        let shift_8_y = [0, 1, 8, 9];
        let shift_32_uv = [0, 2, 8, 10];
        let shift_16_uv = [0, 1, 4, 5];
        let top = at(0, 0);
        match top.sb_type {
            12 => self.build_masks(top, 0, 0, &mut m, true),
            11 => {
                self.build_masks(top, 0, 0, &mut m, true);
                if 4 < max_rows {
                    self.build_masks(at(4, 0), 32, 8, &mut m, true);
                }
            }
            10 => {
                self.build_masks(top, 0, 0, &mut m, true);
                if 4 < max_cols {
                    self.build_masks(at(0, 4), 4, 2, &mut m, true);
                }
            }
            _ => {
                for i32 in 0..4 {
                    let c32 = (i32 & 1) << 2;
                    let r32 = (i32 >> 1) << 2;
                    if c32 >= max_cols || r32 >= max_rows {
                        continue;
                    }
                    let (sy, suv) = (shift_32_y[i32], shift_32_uv[i32]);
                    let b32 = at(r32, c32);
                    match b32.sb_type {
                        9 => self.build_masks(b32, sy, suv, &mut m, true),
                        8 => {
                            self.build_masks(b32, sy, suv, &mut m, true);
                            if r32 + 2 < max_rows {
                                self.build_masks(at(r32 + 2, c32), sy + 16, suv + 4, &mut m, true);
                            }
                        }
                        7 => {
                            self.build_masks(b32, sy, suv, &mut m, true);
                            if c32 + 2 < max_cols {
                                self.build_masks(at(r32, c32 + 2), sy + 2, suv + 1, &mut m, true);
                            }
                        }
                        _ => {
                            for i16 in 0..4 {
                                let c16 = c32 + ((i16 & 1) << 1);
                                let r16 = r32 + ((i16 >> 1) << 1);
                                if c16 >= max_cols || r16 >= max_rows {
                                    continue;
                                }
                                let sy16 = sy + shift_16_y[i16];
                                let suv16 = suv + shift_16_uv[i16];
                                let b16 = at(r16, c16);
                                match b16.sb_type {
                                    6 => self.build_masks(b16, sy16, suv16, &mut m, true),
                                    5 => {
                                        self.build_masks(b16, sy16, suv16, &mut m, true);
                                        if r16 + 1 < max_rows {
                                            self.build_masks(at(r16 + 1, c16), sy16 + 8, 0, &mut m, false);
                                        }
                                    }
                                    4 => {
                                        self.build_masks(b16, sy16, suv16, &mut m, true);
                                        if c16 + 1 < max_cols {
                                            self.build_masks(at(r16, c16 + 1), sy16 + 1, 0, &mut m, false);
                                        }
                                    }
                                    _ => {
                                        self.build_masks(b16, sy16 + shift_8_y[0], suv16, &mut m, true);
                                        for i8 in 1..4 {
                                            let c8 = c16 + (i8 & 1);
                                            let r8 = r16 + (i8 >> 1);
                                            if c8 >= max_cols || r8 >= max_rows {
                                                continue;
                                            }
                                            self.build_masks(at(r8, c8), sy16 + shift_8_y[i8], 0, &mut m, false);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // The 16x16 filter also serves 32x32 transforms.
        m.left_y[2] |= m.left_y[3];
        m.above_y[2] |= m.above_y[3];
        m.left_uv[2] |= m.left_uv[3];
        m.above_uv[2] |= m.above_uv[3];
        // At least the 8-tap filter on every 32x32 border.
        m.left_y[1] |= m.left_y[0] & LEFT_BORDER;
        m.left_y[0] &= !LEFT_BORDER;
        m.above_y[1] |= m.above_y[0] & ABOVE_BORDER;
        m.above_y[0] &= !ABOVE_BORDER;
        m.left_uv[1] |= m.left_uv[0] & LEFT_BORDER_UV;
        m.left_uv[0] &= !LEFT_BORDER_UV;
        m.above_uv[1] |= m.above_uv[0] & ABOVE_BORDER_UV;
        m.above_uv[0] &= !ABOVE_BORDER_UV;
        if mi_row + 8 > mi_rows {
            let rows = (mi_rows - mi_row) as u64;
            let mask_y = (1u64 << (rows << 3)) - 1;
            let mask_uv = ((1u32 << (((rows + 1) >> 1) << 2)) - 1) as u16;
            for i in 0..3 {
                m.left_y[i] &= mask_y;
                m.above_y[i] &= mask_y;
                m.left_uv[i] &= mask_uv;
                m.above_uv[i] &= mask_uv;
            }
            m.int_4x4_y &= mask_y;
            m.int_4x4_uv &= mask_uv;
            if rows == 1 {
                m.above_uv[1] |= m.above_uv[2];
                m.above_uv[2] = 0;
            }
            if rows == 5 {
                m.above_uv[1] |= m.above_uv[2] & 0xff00;
                m.above_uv[2] &= !(m.above_uv[2] & 0xff00);
            }
        }
        if mi_col + 8 > mi_cols {
            let cols = (mi_cols - mi_col) as u64;
            let mask_y = ((1u64 << cols) - 1) * 0x0101_0101_0101_0101;
            let mask_uv = (((1u32 << ((cols + 1) >> 1)) - 1) * 0x1111) as u16;
            let mask_uv_int = (((1u32 << (cols >> 1)) - 1) * 0x1111) as u16;
            for i in 0..3 {
                m.left_y[i] &= mask_y;
                m.above_y[i] &= mask_y;
                m.left_uv[i] &= mask_uv;
                m.above_uv[i] &= mask_uv;
            }
            m.int_4x4_y &= mask_y;
            m.int_4x4_uv &= mask_uv_int;
            if cols == 1 {
                m.left_uv[1] |= m.left_uv[2];
                m.left_uv[2] = 0;
            }
            if cols == 5 {
                m.left_uv[1] |= m.left_uv[2] & 0xcccc;
                m.left_uv[2] &= !(m.left_uv[2] & 0xcccc);
            }
        }
        if mi_col == 0 {
            for i in 0..3 {
                m.left_y[i] &= 0xfefe_fefe_fefe_fefe;
                m.left_uv[i] &= 0xeeee;
            }
        }
        m
    }

    /// Filters the whole frame.
    pub fn filter_frame(&self, f: &mut FrameBuf, mis: &[ModeInfo]) {
        let (mi_cols, mi_rows) = (f.mi_cols, f.mi_rows);
        let s420 = self.ss == (true, true);
        let s444 = self.ss == (false, false);
        for mi_row in (0..mi_rows).step_by(8) {
            for mi_col in (0..mi_cols).step_by(8) {
                let m = self.setup_mask(mis, mi_cols, mi_rows, mi_row, mi_col);
                let pl = &mut f.planes[0];
                let off = mi_row * 8 * pl.stride + mi_col * 8;
                self.plane_ss00(&mut pl.data, off, pl.stride, mi_row, mi_rows, &m);
                for p in 1..3 {
                    let pl = &mut f.planes[p];
                    if s420 {
                        let off = mi_row * 4 * pl.stride + mi_col * 4;
                        self.plane_ss11(&mut pl.data, off, pl.stride, mi_row, mi_rows, &m);
                    } else if s444 {
                        let off = mi_row * 8 * pl.stride + mi_col * 8;
                        self.plane_ss00(&mut pl.data, off, pl.stride, mi_row, mi_rows, &m);
                    }
                }
            }
        }
    }

    fn plane_ss00(&self, d: &mut [u8], off: usize, stride: usize, mi_row: usize, mi_rows: usize, m: &Mask) {
        let (mut m16, mut m8, mut m4, mut mi4) = (m.left_y[2], m.left_y[1], m.left_y[0], m.int_4x4_y);
        let mut o = off;
        let mut r = 0;
        while r < 8 && mi_row + r < mi_rows {
            self.vert_row2(d, o, stride, false, m16 as u32, m8 as u32, m4 as u32, mi4 as u32, &m.lfl_y[r << 3..]);
            o += 16 * stride;
            m16 >>= 16;
            m8 >>= 16;
            m4 >>= 16;
            mi4 >>= 16;
            r += 2;
        }
        let (mut m16, mut m8, mut m4, mut mi4) = (m.above_y[2], m.above_y[1], m.above_y[0], m.int_4x4_y);
        let mut o = off;
        let mut r = 0;
        while r < 8 && mi_row + r < mi_rows {
            let (a16, a8, a4) = if mi_row + r == 0 { (0, 0, 0) } else { ((m16 & 0xff) as u32, (m8 & 0xff) as u32, (m4 & 0xff) as u32) };
            self.horiz(d, o, stride, a16, a8, a4, (mi4 & 0xff) as u32, &m.lfl_y[r << 3..]);
            o += 8 * stride;
            m16 >>= 8;
            m8 >>= 8;
            m4 >>= 8;
            mi4 >>= 8;
            r += 1;
        }
    }

    fn plane_ss11(&self, d: &mut [u8], off: usize, stride: usize, mi_row: usize, mi_rows: usize, m: &Mask) {
        let mut lfl_uv = [0u8; 16];
        let (mut m16, mut m8, mut m4, mut mi4) = (m.left_uv[2], m.left_uv[1], m.left_uv[0], m.int_4x4_uv);
        let mut o = off;
        let mut r = 0;
        while r < 8 && mi_row + r < mi_rows {
            for c in 0..4 {
                lfl_uv[(r << 1) + c] = m.lfl_y[(r << 3) + (c << 1)];
                lfl_uv[((r + 2) << 1) + c] = m.lfl_y[((r + 2) << 3) + (c << 1)];
            }
            self.vert_row2(d, o, stride, true, (m16 & 0xff) as u32, (m8 & 0xff) as u32, (m4 & 0xff) as u32, (mi4 & 0xff) as u32, &lfl_uv[r << 1..]);
            o += 16 * stride;
            m16 >>= 8;
            m8 >>= 8;
            m4 >>= 8;
            mi4 >>= 8;
            r += 4;
        }
        let (mut m16, mut m8, mut m4, mut mi4) = (m.above_uv[2], m.above_uv[1], m.above_uv[0], m.int_4x4_uv);
        let mut o = off;
        let mut r = 0;
        while r < 8 && mi_row + r < mi_rows {
            let skip_border = mi_row + r == mi_rows - 1;
            let int_r = if skip_border { 0 } else { (mi4 & 0xf) as u32 };
            let (a16, a8, a4) = if mi_row + r == 0 { (0, 0, 0) } else { ((m16 & 0xf) as u32, (m8 & 0xf) as u32, (m4 & 0xf) as u32) };
            self.horiz(d, o, stride, a16, a8, a4, int_r, &lfl_uv[r << 1..]);
            o += 8 * stride;
            m16 >>= 4;
            m8 >>= 4;
            m4 >>= 4;
            mi4 >>= 4;
            r += 2;
        }
    }

    /// `filter_selectively_vert_row2`: two rows of 8x8 blocks.
    #[allow(clippy::too_many_arguments)]
    fn vert_row2(&self, d: &mut [u8], off: usize, stride: usize, uv: bool, mut m16: u32, mut m8: u32, mut m4: u32, mut mi4: u32, lfl: &[u8]) {
        let cutoff = if uv { 0xff } else { 0xffff };
        let fwd = if uv { 4 } else { 8 };
        let dual = 1u32 | (1 << fwd);
        let mut mask = (m16 | m8 | m4 | mi4) & cutoff;
        let mut s0 = off;
        let mut li = 0usize;
        while mask != 0 {
            let t0 = self.thresh[lfl[li] as usize];
            let t1 = self.thresh[lfl[li + fwd] as usize];
            let s1 = s0 + 8 * stride;
            if mask & dual != 0 {
                if m16 & dual != 0 {
                    if m16 & dual == dual {
                        lpf16(d, s0, 1, stride, 16, t0);
                    } else {
                        let (s, t) = if m16 & 1 != 0 { (s0, t0) } else { (s1, t1) };
                        lpf16(d, s, 1, stride, 8, t);
                    }
                }
                if m8 & dual != 0 {
                    if m8 & 1 != 0 {
                        lpf8(d, s0, 1, stride, 8, t0);
                    }
                    if m8 & (1 << fwd) != 0 {
                        lpf8(d, s1, 1, stride, 8, t1);
                    }
                }
                if m4 & dual != 0 {
                    if m4 & 1 != 0 {
                        lpf4(d, s0, 1, stride, 8, t0);
                    }
                    if m4 & (1 << fwd) != 0 {
                        lpf4(d, s1, 1, stride, 8, t1);
                    }
                }
                if mi4 & dual != 0 {
                    if mi4 & 1 != 0 {
                        lpf4(d, s0 + 4, 1, stride, 8, t0);
                    }
                    if mi4 & (1 << fwd) != 0 {
                        lpf4(d, s1 + 4, 1, stride, 8, t1);
                    }
                }
            }
            s0 += 8;
            li += 1;
            m16 >>= 1;
            m8 >>= 1;
            m4 >>= 1;
            mi4 >>= 1;
            mask = (mask & !dual) >> 1;
        }
    }

    /// `filter_selectively_horiz`.
    #[allow(clippy::too_many_arguments)]
    fn horiz(&self, d: &mut [u8], off: usize, stride: usize, mut m16: u32, mut m8: u32, mut m4: u32, mut mi4: u32, lfl: &[u8]) {
        let mut mask = m16 | m8 | m4 | mi4;
        let mut s = off;
        let mut li = 0usize;
        while mask != 0 {
            let t = self.thresh[lfl[li] as usize];
            let mut count = 1;
            if mask & 1 != 0 {
                if m16 & 1 != 0 {
                    if m16 & 3 == 3 {
                        lpf16(d, s, stride, 1, 16, t);
                        count = 2;
                    } else {
                        lpf16(d, s, stride, 1, 8, t);
                    }
                } else if m8 & 1 != 0 {
                    if m8 & 3 == 3 {
                        let tn = self.thresh[lfl[li + 1] as usize];
                        lpf8(d, s, stride, 1, 8, t);
                        lpf8(d, s + 8, stride, 1, 8, tn);
                        if mi4 & 1 != 0 {
                            lpf4(d, s + 4 * stride, stride, 1, 8, t);
                        }
                        if mi4 & 2 != 0 {
                            lpf4(d, s + 8 + 4 * stride, stride, 1, 8, tn);
                        }
                        count = 2;
                    } else {
                        lpf8(d, s, stride, 1, 8, t);
                        if mi4 & 1 != 0 {
                            lpf4(d, s + 4 * stride, stride, 1, 8, t);
                        }
                    }
                } else if m4 & 1 != 0 {
                    if m4 & 3 == 3 {
                        let tn = self.thresh[lfl[li + 1] as usize];
                        lpf4(d, s, stride, 1, 8, t);
                        lpf4(d, s + 8, stride, 1, 8, tn);
                        if mi4 & 1 != 0 {
                            lpf4(d, s + 4 * stride, stride, 1, 8, t);
                        }
                        if mi4 & 2 != 0 {
                            lpf4(d, s + 8 + 4 * stride, stride, 1, 8, tn);
                        }
                        count = 2;
                    } else {
                        lpf4(d, s, stride, 1, 8, t);
                        if mi4 & 1 != 0 {
                            lpf4(d, s + 4 * stride, stride, 1, 8, t);
                        }
                    }
                } else {
                    lpf4(d, s + 4 * stride, stride, 1, 8, t);
                }
            }
            s += 8 * count;
            li += count;
            m16 >>= count;
            m8 >>= count;
            m4 >>= count;
            mi4 >>= count;
            mask >>= count;
        }
    }
}

// ---------------------------------------------------------------------
// Edge filters. `q0` is the index of the first pixel past the edge,
// `step` the distance across the edge (1 for vertical edges, the stride
// for horizontal ones), `along` the distance along the edge.
// ---------------------------------------------------------------------

#[inline(always)]
fn filter_mask(lim: u8, blim: u8, p: &[i32; 4], q: &[i32; 4]) -> bool {
    let l = lim as i32;
    (p[3] - p[2]).abs() <= l
        && (p[2] - p[1]).abs() <= l
        && (p[1] - p[0]).abs() <= l
        && (q[1] - q[0]).abs() <= l
        && (q[2] - q[1]).abs() <= l
        && (q[3] - q[2]).abs() <= l
        && (p[0] - q[0]).abs() * 2 + (p[1] - q[1]).abs() / 2 <= blim as i32
}

#[inline(always)]
fn flat4(p: &[i32; 4], q: &[i32; 4]) -> bool {
    (p[1] - p[0]).abs() <= 1 && (q[1] - q[0]).abs() <= 1 && (p[2] - p[0]).abs() <= 1 && (q[2] - q[0]).abs() <= 1 && (p[3] - p[0]).abs() <= 1 && (q[3] - q[0]).abs() <= 1
}

#[inline(always)]
fn sclamp(v: i32) -> i32 {
    v.clamp(-128, 127)
}

/// `filter4` on the pixels at `q0 - 2*step .. q0 + 2*step`.
#[inline(always)]
fn filter4(d: &mut [u8], q0: usize, step: usize, hev_thr: u8) {
    let ps1 = d[q0 - 2 * step] as i32 - 128;
    let ps0 = d[q0 - step] as i32 - 128;
    let qs0 = d[q0] as i32 - 128;
    let qs1 = d[q0 + step] as i32 - 128;
    let hev = (ps1 - ps0).abs() > hev_thr as i32 || (qs1 - qs0).abs() > hev_thr as i32;
    let mut f = if hev { sclamp(ps1 - qs1) } else { 0 };
    f = sclamp(f + 3 * (qs0 - ps0));
    let f1 = sclamp(f + 4) >> 3;
    let f2 = sclamp(f + 3) >> 3;
    d[q0] = (sclamp(qs0 - f1) + 128) as u8;
    d[q0 - step] = (sclamp(ps0 + f2) + 128) as u8;
    if !hev {
        let f = (f1 + 1) >> 1;
        d[q0 + step] = (sclamp(qs1 - f) + 128) as u8;
        d[q0 - 2 * step] = (sclamp(ps1 + f) + 128) as u8;
    }
}

fn load(d: &[u8], q0: usize, step: usize) -> ([i32; 4], [i32; 4]) {
    let p = [d[q0 - step] as i32, d[q0 - 2 * step] as i32, d[q0 - 3 * step] as i32, d[q0 - 4 * step] as i32];
    let q = [d[q0] as i32, d[q0 + step] as i32, d[q0 + 2 * step] as i32, d[q0 + 3 * step] as i32];
    (p, q)
}

fn lpf4(d: &mut [u8], at: usize, step: usize, along: usize, count: usize, t: Thresh) {
    for i in 0..count {
        let q0 = at + i * along;
        let (p, q) = load(d, q0, step);
        if filter_mask(t.lim, t.mblim, &p, &q) {
            filter4(d, q0, step, t.hev);
        }
    }
}

fn lpf8(d: &mut [u8], at: usize, step: usize, along: usize, count: usize, t: Thresh) {
    for i in 0..count {
        let q0 = at + i * along;
        let (p, q) = load(d, q0, step);
        if !filter_mask(t.lim, t.mblim, &p, &q) {
            continue;
        }
        if flat4(&p, &q) {
            let (p3, p2, p1, p0, q0v, q1, q2, q3) = (p[3], p[2], p[1], p[0], q[0], q[1], q[2], q[3]);
            let w = |v: i32| ((v + 4) >> 3) as u8;
            d[q0 - 3 * step] = w(p3 + p3 + p3 + 2 * p2 + p1 + p0 + q0v);
            d[q0 - 2 * step] = w(p3 + p3 + p2 + 2 * p1 + p0 + q0v + q1);
            d[q0 - step] = w(p3 + p2 + p1 + 2 * p0 + q0v + q1 + q2);
            d[q0] = w(p2 + p1 + p0 + 2 * q0v + q1 + q2 + q3);
            d[q0 + step] = w(p1 + p0 + q0v + 2 * q1 + q2 + q3 + q3);
            d[q0 + 2 * step] = w(p0 + q0v + q1 + 2 * q2 + q3 + q3 + q3);
        } else {
            filter4(d, q0, step, t.hev);
        }
    }
}

fn lpf16(d: &mut [u8], at: usize, step: usize, along: usize, count: usize, t: Thresh) {
    for i in 0..count {
        let q0i = at + i * along;
        let (p, q) = load(d, q0i, step);
        if !filter_mask(t.lim, t.mblim, &p, &q) {
            continue;
        }
        if !flat4(&p, &q) {
            filter4(d, q0i, step, t.hev);
            continue;
        }
        let px = |k: usize| d[q0i - (k + 1) * step] as i32;
        let qx = |k: usize| d[q0i + k * step] as i32;
        let (p4, p5, p6, p7) = (px(4), px(5), px(6), px(7));
        let (q4, q5, q6, q7) = (qx(4), qx(5), qx(6), qx(7));
        let flat2 = (p4 - p[0]).abs() <= 1
            && (q4 - q[0]).abs() <= 1
            && (p5 - p[0]).abs() <= 1
            && (q5 - q[0]).abs() <= 1
            && (p6 - p[0]).abs() <= 1
            && (q6 - q[0]).abs() <= 1
            && (p7 - p[0]).abs() <= 1
            && (q7 - q[0]).abs() <= 1;
        let (p0, p1, p2, p3) = (p[0], p[1], p[2], p[3]);
        let (q0, q1, q2, q3) = (q[0], q[1], q[2], q[3]);
        if flat2 {
            let w = |v: i32| ((v + 8) >> 4) as u8;
            let out = [
                w(p7 * 7 + p6 * 2 + p5 + p4 + p3 + p2 + p1 + p0 + q0),
                w(p7 * 6 + p6 + p5 * 2 + p4 + p3 + p2 + p1 + p0 + q0 + q1),
                w(p7 * 5 + p6 + p5 + p4 * 2 + p3 + p2 + p1 + p0 + q0 + q1 + q2),
                w(p7 * 4 + p6 + p5 + p4 + p3 * 2 + p2 + p1 + p0 + q0 + q1 + q2 + q3),
                w(p7 * 3 + p6 + p5 + p4 + p3 + p2 * 2 + p1 + p0 + q0 + q1 + q2 + q3 + q4),
                w(p7 * 2 + p6 + p5 + p4 + p3 + p2 + p1 * 2 + p0 + q0 + q1 + q2 + q3 + q4 + q5),
                w(p7 + p6 + p5 + p4 + p3 + p2 + p1 + p0 * 2 + q0 + q1 + q2 + q3 + q4 + q5 + q6),
                w(p6 + p5 + p4 + p3 + p2 + p1 + p0 + q0 * 2 + q1 + q2 + q3 + q4 + q5 + q6 + q7),
                w(p5 + p4 + p3 + p2 + p1 + p0 + q0 + q1 * 2 + q2 + q3 + q4 + q5 + q6 + q7 * 2),
                w(p4 + p3 + p2 + p1 + p0 + q0 + q1 + q2 * 2 + q3 + q4 + q5 + q6 + q7 * 3),
                w(p3 + p2 + p1 + p0 + q0 + q1 + q2 + q3 * 2 + q4 + q5 + q6 + q7 * 4),
                w(p2 + p1 + p0 + q0 + q1 + q2 + q3 + q4 * 2 + q5 + q6 + q7 * 5),
                w(p1 + p0 + q0 + q1 + q2 + q3 + q4 + q5 * 2 + q6 + q7 * 6),
                w(p0 + q0 + q1 + q2 + q3 + q4 + q5 + q6 * 2 + q7 * 7),
            ];
            // out[0] is p6 ... out[6] is p0, out[7] q0 ... out[13] q6.
            for k in 0..7 {
                d[q0i - (7 - k) * step] = out[k];
            }
            for k in 0..7 {
                d[q0i + k * step] = out[7 + k];
            }
        } else {
            let w = |v: i32| ((v + 4) >> 3) as u8;
            d[q0i - 3 * step] = w(p3 + p3 + p3 + 2 * p2 + p1 + p0 + q0);
            d[q0i - 2 * step] = w(p3 + p3 + p2 + 2 * p1 + p0 + q0 + q1);
            d[q0i - step] = w(p3 + p2 + p1 + 2 * p0 + q0 + q1 + q2);
            d[q0i] = w(p2 + p1 + p0 + 2 * q0 + q1 + q2 + q3);
            d[q0i + step] = w(p1 + p0 + q0 + 2 * q1 + q2 + q3 + q3);
            d[q0i + 2 * step] = w(p0 + q0 + q1 + 2 * q2 + q3 + q3 + q3);
        }
    }
}
