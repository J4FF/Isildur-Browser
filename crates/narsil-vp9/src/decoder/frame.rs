//! Decoding of one frame's tiles: partitions, mode info, motion vector
//! prediction, coefficient tokens and reconstruction.

use super::bits::BoolDecoder;
use super::header::{FrameHeader, Segmentation};
use super::idct;
use super::probs::{COMPOUND_REFERENCE, Counts, FrameContext, REFERENCE_MODE_SELECT};
use super::recon::{RefPlane, predict_inter, predict_intra};
use super::tables::*;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Mv {
    pub row: i16,
    pub col: i16,
}

#[derive(Clone, Copy, Debug)]
pub struct MvRef {
    pub ref_frame: [i8; 2],
    pub mv: [Mv; 2],
}

impl Default for MvRef {
    fn default() -> Self {
        MvRef { ref_frame: [INTRA_FRAME, NONE_FRAME], mv: [Mv::default(); 2] }
    }
}

/// Mode info of one block, replicated into every 8x8 cell it covers.
#[derive(Clone, Copy, Debug)]
pub struct ModeInfo {
    pub sb_type: u8,
    pub mode: u8,
    pub uv_mode: u8,
    pub sub_modes: [u8; 4],
    pub ref_frame: [i8; 2],
    pub mv: [Mv; 2],
    pub bmv: [[Mv; 2]; 4],
    pub interp_filter: u8,
    pub tx_size: u8,
    pub skip: bool,
    pub segment_id: u8,
    pub seg_id_predicted: bool,
}

impl Default for ModeInfo {
    fn default() -> Self {
        ModeInfo {
            sb_type: BLOCK_8X8,
            mode: DC_PRED,
            uv_mode: DC_PRED,
            sub_modes: [DC_PRED; 4],
            ref_frame: [INTRA_FRAME, NONE_FRAME],
            mv: [Mv::default(); 2],
            bmv: [[Mv::default(); 2]; 4],
            interp_filter: 0,
            tx_size: 0,
            skip: false,
            segment_id: 0,
            seg_id_predicted: false,
        }
    }
}

impl ModeInfo {
    pub fn is_inter(&self) -> bool {
        self.ref_frame[0] > INTRA_FRAME
    }
    pub fn has_second_ref(&self) -> bool {
        self.ref_frame[1] > INTRA_FRAME
    }
}

/// One plane of a frame buffer. `data` covers the frame rounded up to
/// whole superblocks so that blocks at the edges can be written whole.
#[derive(Clone)]
pub struct Plane {
    pub data: Vec<u8>,
    pub stride: usize,
    /// Visible size.
    pub w: usize,
    pub h: usize,
    /// Decoded size (8-aligned in luma).
    pub aw: usize,
    pub ah: usize,
}

/// A decoded frame (also a reference frame).
#[derive(Clone)]
pub struct FrameBuf {
    pub width: u32,
    pub height: u32,
    pub ss_x: bool,
    pub ss_y: bool,
    pub planes: [Plane; 3],
    pub mi_cols: usize,
    pub mi_rows: usize,
    pub mvs: Vec<MvRef>,
}

impl FrameBuf {
    pub fn new(width: u32, height: u32, ss_x: bool, ss_y: bool) -> Self {
        let mi_cols = (width as usize).div_ceil(8);
        let mi_rows = (height as usize).div_ceil(8);
        let sb_w = mi_cols.div_ceil(8) * 64;
        let sb_h = mi_rows.div_ceil(8) * 64;
        let mk = |sx: bool, sy: bool, w: usize, h: usize| {
            let stride = (sb_w >> sx as usize) + 32;
            let rows = (sb_h >> sy as usize) + 32;
            Plane {
                data: vec![0u8; stride * rows],
                stride,
                w: (w + sx as usize) >> sx as usize,
                h: (h + sy as usize) >> sy as usize,
                aw: (mi_cols * 8) >> sx as usize,
                ah: (mi_rows * 8) >> sy as usize,
            }
        };
        let (w, h) = (width as usize, height as usize);
        FrameBuf {
            width,
            height,
            ss_x,
            ss_y,
            planes: [mk(false, false, w, h), mk(ss_x, ss_y, w, h), mk(ss_x, ss_y, w, h)],
            mi_cols,
            mi_rows,
            mvs: vec![MvRef::default(); mi_cols * mi_rows],
        }
    }
}

/// Scale factors of a reference (libvpx `struct scale_factors`).
#[derive(Clone, Copy)]
pub struct Scale {
    pub x_fp: i64,
    pub y_fp: i64,
    pub x_step: i32,
    pub y_step: i32,
    pub scaled: bool,
}

impl Scale {
    pub fn new(other_w: u32, other_h: u32, this_w: u32, this_h: u32) -> Option<Scale> {
        let valid = 2 * this_w >= other_w && 2 * this_h >= other_h && this_w <= 16 * other_w && this_h <= 16 * other_h;
        if !valid {
            return None;
        }
        let x_fp = ((other_w as i64) << 14) / this_w as i64;
        let y_fp = ((other_h as i64) << 14) / this_h as i64;
        Some(Scale {
            x_fp,
            y_fp,
            x_step: ((16 * x_fp) >> 14) as i32,
            y_step: ((16 * y_fp) >> 14) as i32,
            scaled: x_fp != 1 << 14 || y_fp != 1 << 14,
        })
    }
    fn sx(&self, v: i64) -> i64 {
        (v * self.x_fp) >> 14
    }
    fn sy(&self, v: i64) -> i64 {
        (v * self.y_fp) >> 14
    }
}

pub struct TileInfo {
    pub mi_col_start: usize,
    pub mi_col_end: usize,
    pub mi_row_start: usize,
    pub mi_row_end: usize,
}

/// Per-frame decoding state.
pub struct FrameDec<'a> {
    pub h: &'a FrameHeader,
    pub fc: &'a FrameContext,
    pub seg: &'a Segmentation,
    pub counts: Option<Box<Counts>>,
    pub tx_mode: u8,
    pub reference_mode: u8,
    pub comp_fixed_ref: i8,
    pub comp_var_ref: [i8; 2],
    pub mi_cols: usize,
    pub mi_rows: usize,
    pub mi: Vec<ModeInfo>,
    above_partition: Vec<u8>,
    above_nz: [Vec<u8>; 3],
    left_partition: [u8; 8],
    left_nz: [[u8; 16]; 3],
    pub cur: FrameBuf,
    pub refs: [Option<(&'a FrameBuf, Scale)>; 3],
    pub prev_mvs: Option<&'a [MvRef]>,
    pub prev_seg: &'a [u8],
    pub seg_map: Vec<u8>,
    dequant: [[[i32; 2]; 2]; 8],
    coef: Vec<i32>,
    /// The token contexts of the block being read (positions before the
    /// current one in scan order are always written first).
    token_cache: Box<[u8; 1024]>,
    tile: TileInfo,
    pub corrupted: bool,
}

/// The geometry of the block being decoded.
#[derive(Clone, Copy)]
struct Blk {
    r: usize,
    c: usize,
    bsize: u8,
    x_mis: usize,
    y_mis: usize,
    to_left: i32,
    to_right: i32,
    to_top: i32,
    to_bottom: i32,
    has_above: bool,
    has_left: bool,
}

fn lower_mv_precision(mv: &mut Mv, allow_hp: bool) {
    let use_hp = allow_hp && (mv.row.unsigned_abs() >> 3) < 8 && (mv.col.unsigned_abs() >> 3) < 8;
    if !use_hp {
        if mv.row & 1 != 0 {
            mv.row += if mv.row > 0 { -1 } else { 1 };
        }
        if mv.col & 1 != 0 {
            mv.col += if mv.col > 0 { -1 } else { 1 };
        }
    }
}

impl<'a> FrameDec<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        h: &'a FrameHeader,
        fc: &'a FrameContext,
        seg: &'a Segmentation,
        cur: FrameBuf,
        refs: [Option<(&'a FrameBuf, Scale)>; 3],
        prev_mvs: Option<&'a [MvRef]>,
        prev_seg: &'a [u8],
        tx_mode: u8,
        reference_mode: u8,
        counting: bool,
    ) -> Self {
        let mi_cols = cur.mi_cols;
        let mi_rows = cur.mi_rows;
        let mut dequant = [[[0i32; 2]; 2]; 8];
        for (s, dq) in dequant.iter_mut().enumerate() {
            let q = if seg.active(s as u8, SEG_LVL_ALT_Q) {
                let d = seg.data(s as u8, SEG_LVL_ALT_Q) as i32;
                if seg.abs_delta { d } else { h.base_q_idx as i32 + d }
            } else {
                h.base_q_idx as i32
            }
            .clamp(0, 255);
            let at = |t: &[i16; 256], d: i8| t[(q + d as i32).clamp(0, 255) as usize] as i32;
            dq[0] = [at(&DC_QLOOKUP, h.delta_q_y_dc), at(&AC_QLOOKUP, 0)];
            dq[1] = [at(&DC_QLOOKUP, h.delta_q_uv_dc), at(&AC_QLOOKUP, h.delta_q_uv_ac)];
        }
        // Compound reference setup.
        let sb = &h.sign_bias;
        let (fixed, var) = if sb[1] == sb[2] {
            (ALTREF_FRAME, [LAST_FRAME, GOLDEN_FRAME])
        } else if sb[1] == sb[3] {
            (GOLDEN_FRAME, [LAST_FRAME, ALTREF_FRAME])
        } else {
            (LAST_FRAME, [GOLDEN_FRAME, ALTREF_FRAME])
        };
        let nz_len = mi_cols * 2 + 32;
        FrameDec {
            h,
            fc,
            seg,
            counts: if counting { Some(Box::default()) } else { None },
            tx_mode,
            reference_mode,
            comp_fixed_ref: fixed,
            comp_var_ref: var,
            mi_cols,
            mi_rows,
            mi: vec![ModeInfo::default(); mi_cols * mi_rows],
            above_partition: vec![0; mi_cols + 16],
            above_nz: [vec![0; nz_len], vec![0; nz_len], vec![0; nz_len]],
            left_partition: [0; 8],
            left_nz: [[0; 16]; 3],
            cur,
            refs,
            prev_mvs,
            prev_seg,
            seg_map: vec![0; mi_cols * mi_rows],
            dequant,
            coef: vec![0; 1024],
            token_cache: Box::new([0; 1024]),
            tile: TileInfo { mi_col_start: 0, mi_col_end: 0, mi_row_start: 0, mi_row_end: 0 },
            corrupted: false,
        }
    }

    /// Decodes all tiles from `data` (the bytes after the compressed header).
    pub fn decode_tiles(&mut self, data: &[u8]) -> Result<(), String> {
        let tile_cols = 1usize << self.h.tile_cols_log2;
        let tile_rows = 1usize << self.h.tile_rows_log2;
        let mut pos = 0usize;
        let offset = |n: usize, mis: usize, log2: u8| -> usize {
            let sbs = mis.div_ceil(8);
            (((n * sbs) >> log2) << 3).min(mis)
        };
        for tr in 0..tile_rows {
            for tc in 0..tile_cols {
                let last = tr == tile_rows - 1 && tc == tile_cols - 1;
                let size = if last {
                    data.len().saturating_sub(pos)
                } else {
                    if pos + 4 > data.len() {
                        return Err("VP9: truncated tile size".into());
                    }
                    let s = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
                    pos += 4;
                    s
                };
                if pos + size > data.len() {
                    return Err("VP9: truncated tile".into());
                }
                let td = &data[pos..pos + size];
                pos += size;
                self.tile = TileInfo {
                    mi_col_start: offset(tc, self.mi_cols, self.h.tile_cols_log2),
                    mi_col_end: offset(tc + 1, self.mi_cols, self.h.tile_cols_log2),
                    mi_row_start: offset(tr, self.mi_rows, self.h.tile_rows_log2),
                    mi_row_end: offset(tr + 1, self.mi_rows, self.h.tile_rows_log2),
                };
                let mut r = BoolDecoder::new(td).map_err(|e| format!("VP9: {e}"))?;
                let mut mi_row = self.tile.mi_row_start;
                while mi_row < self.tile.mi_row_end {
                    self.left_partition = [0; 8];
                    self.left_nz = [[0; 16]; 3];
                    let mut mi_col = self.tile.mi_col_start;
                    while mi_col < self.tile.mi_col_end {
                        self.decode_partition(&mut r, mi_row, mi_col, BLOCK_64X64);
                        mi_col += 8;
                    }
                    mi_row += 8;
                }
                if r.overrun() {
                    self.corrupted = true;
                }
            }
        }
        Ok(())
    }

    fn decode_partition(&mut self, r: &mut BoolDecoder, mi_row: usize, mi_col: usize, bsize: u8) {
        if mi_row >= self.mi_rows || mi_col >= self.mi_cols {
            return;
        }
        let n8 = NUM_8X8_W[bsize as usize] as usize;
        let hbs = n8 >> 1;
        let has_rows = mi_row + hbs < self.mi_rows;
        let has_cols = mi_col + hbs < self.mi_cols;
        // Partition context.
        let bsl = MI_WIDTH_LOG2[bsize as usize];
        let above = (self.above_partition[mi_col] >> bsl) & 1;
        let left = (self.left_partition[mi_row & 7] >> bsl) & 1;
        let ctx = (left * 2 + above) as usize + bsl as usize * 4;
        let probs = if self.h.frame_is_intra() { &KF_PARTITION_PROBS[ctx] } else { &self.fc.partition[ctx] };
        let p = if has_rows && has_cols {
            r.tree(&PARTITION_TREE, probs)
        } else if has_cols {
            if r.read(probs[1]) { PARTITION_SPLIT } else { PARTITION_HORZ }
        } else if has_rows {
            if r.read(probs[2]) { PARTITION_SPLIT } else { PARTITION_VERT }
        } else {
            PARTITION_SPLIT
        };
        if !self.h.frame_is_intra()
            && let Some(c) = self.counts.as_mut()
        {
            c.partition[ctx][p as usize] += 1;
        }
        let sub = SUBSIZE[p as usize][bsize as usize];
        if sub < BLOCK_8X8 || p == PARTITION_NONE {
            self.decode_block(r, mi_row, mi_col, sub);
        } else if p == PARTITION_HORZ {
            self.decode_block(r, mi_row, mi_col, sub);
            if has_rows {
                self.decode_block(r, mi_row + hbs, mi_col, sub);
            }
        } else if p == PARTITION_VERT {
            self.decode_block(r, mi_row, mi_col, sub);
            if has_cols {
                self.decode_block(r, mi_row, mi_col + hbs, sub);
            }
        } else {
            self.decode_partition(r, mi_row, mi_col, sub);
            self.decode_partition(r, mi_row, mi_col + hbs, sub);
            self.decode_partition(r, mi_row + hbs, mi_col, sub);
            self.decode_partition(r, mi_row + hbs, mi_col + hbs, sub);
        }
        if bsize == BLOCK_8X8 || p != PARTITION_SPLIT {
            let (a, l) = PARTITION_CONTEXT[sub as usize];
            for i in 0..n8 {
                if mi_col + i < self.above_partition.len() {
                    self.above_partition[mi_col + i] = a;
                }
            }
            for i in 0..n8 {
                self.left_partition[(mi_row + i) & 7] = l;
            }
        }
    }

    fn above_mi(&self, b: &Blk) -> Option<&ModeInfo> {
        b.has_above.then(|| &self.mi[(b.r - 1) * self.mi_cols + b.c])
    }

    fn left_mi(&self, b: &Blk) -> Option<&ModeInfo> {
        b.has_left.then(|| &self.mi[b.r * self.mi_cols + b.c - 1])
    }

    fn decode_block(&mut self, r: &mut BoolDecoder, mi_row: usize, mi_col: usize, bsize: u8) {
        let bw = NUM_8X8_W[bsize as usize] as usize;
        let bh = NUM_8X8_H[bsize as usize] as usize;
        let b = Blk {
            r: mi_row,
            c: mi_col,
            bsize,
            x_mis: bw.min(self.mi_cols - mi_col),
            y_mis: bh.min(self.mi_rows - mi_row),
            to_left: -((mi_col * 64) as i32),
            to_right: (self.mi_cols as i32 - bw as i32 - mi_col as i32) * 64,
            to_top: -((mi_row * 64) as i32),
            to_bottom: (self.mi_rows as i32 - bh as i32 - mi_row as i32) * 64,
            has_above: mi_row > 0,
            has_left: mi_col > self.tile.mi_col_start,
        };
        let mut mi = if self.h.frame_is_intra() { self.intra_frame_mode_info(r, &b) } else { self.inter_frame_mode_info(r, &b) };
        mi.sb_type = bsize;
        // Store the mode info into every cell of the block.
        for y in 0..b.y_mis {
            let row = (mi_row + y) * self.mi_cols;
            for x in 0..b.x_mis {
                self.mi[row + mi_col + x] = mi;
            }
        }
        // Residual and reconstruction.
        if mi.skip {
            self.reset_skip_context(&b);
        }
        let eob_total = self.reconstruct(r, &b, &mi);
        if mi.is_inter() && bsize >= BLOCK_8X8 && eob_total == 0 && !mi.skip {
            mi.skip = true;
            for y in 0..b.y_mis {
                let row = (mi_row + y) * self.mi_cols;
                for x in 0..b.x_mis {
                    self.mi[row + mi_col + x].skip = true;
                }
            }
        }
        // Motion vectors for the next frame.
        for y in 0..b.y_mis {
            let row = (mi_row + y) * self.mi_cols;
            for x in 0..b.x_mis {
                self.cur.mvs[row + mi_col + x] = MvRef { ref_frame: mi.ref_frame, mv: mi.mv };
            }
        }
    }

    fn reset_skip_context(&mut self, b: &Blk) {
        for p in 0..3 {
            let (sx, sy) = if p == 0 { (0, 0) } else { (self.cur.ss_x as usize, self.cur.ss_y as usize) };
            let bs = b.bsize.max(BLOCK_8X8) as usize;
            let n4w = (NUM_4X4_W[bs] as usize >> sx).max(1);
            let n4h = (NUM_4X4_H[bs] as usize >> sy).max(1);
            let ax = (b.c * 2) >> sx;
            let ly = ((b.r & 7) * 2) >> sy;
            let a = &mut self.above_nz[p];
            for v in a.iter_mut().skip(ax).take(n4w) {
                *v = 0;
            }
            for v in self.left_nz[p].iter_mut().skip(ly).take(n4h) {
                *v = 0;
            }
        }
    }

    // ---------------------------------------------------------------
    // Mode info
    // ---------------------------------------------------------------

    fn read_segment_id(&self, r: &mut BoolDecoder) -> u8 {
        r.tree(&SEGMENT_TREE, &self.seg.tree_probs)
    }

    fn set_segment(&mut self, b: &Blk, id: u8) {
        for y in 0..b.y_mis {
            let row = (b.r + y) * self.mi_cols;
            for x in 0..b.x_mis {
                self.seg_map[row + b.c + x] = id;
            }
        }
    }

    fn copy_segment(&mut self, b: &Blk) {
        for y in 0..b.y_mis {
            let row = (b.r + y) * self.mi_cols;
            for x in 0..b.x_mis {
                let i = row + b.c + x;
                self.seg_map[i] = self.prev_seg.get(i).copied().unwrap_or(0);
            }
        }
    }

    fn predicted_segment(&self, b: &Blk) -> u8 {
        let mut m = 7u8;
        for y in 0..b.y_mis {
            let row = (b.r + y) * self.mi_cols;
            for x in 0..b.x_mis {
                m = m.min(self.prev_seg.get(row + b.c + x).copied().unwrap_or(0));
            }
        }
        m
    }

    fn read_skip(&mut self, r: &mut BoolDecoder, b: &Blk, seg_id: u8) -> bool {
        if self.seg.active(seg_id, SEG_LVL_SKIP) {
            return true;
        }
        let ctx = self.above_mi(b).map_or(0, |m| m.skip as usize) + self.left_mi(b).map_or(0, |m| m.skip as usize);
        let s = r.read(self.fc.skip[ctx]);
        if let Some(c) = self.counts.as_mut() {
            c.skip[ctx][s as usize] += 1;
        }
        s
    }

    fn read_tx_size(&mut self, r: &mut BoolDecoder, b: &Blk, allow_select: bool) -> u8 {
        let max_tx = MAX_TXSIZE[b.bsize as usize];
        if allow_select && self.tx_mode == TX_MODE_SELECT && b.bsize >= BLOCK_8X8 {
            let above = self.above_mi(b).map(|m| if m.skip { max_tx } else { m.tx_size });
            let left = self.left_mi(b).map(|m| if m.skip { max_tx } else { m.tx_size });
            let (mut a, mut l) = (above.unwrap_or(max_tx), left.unwrap_or(max_tx));
            if left.is_none() {
                l = a;
            }
            if above.is_none() {
                a = l;
            }
            let ctx = ((a as u32 + l as u32) > max_tx as u32) as usize;
            let mut tx = r.read(match max_tx {
                1 => self.fc.tx8[ctx][0],
                2 => self.fc.tx16[ctx][0],
                _ => self.fc.tx32[ctx][0],
            }) as u8;
            if tx != TX_4X4 && max_tx >= TX_16X16 {
                tx += r.read(if max_tx == 2 { self.fc.tx16[ctx][1] } else { self.fc.tx32[ctx][1] }) as u8;
                if tx != TX_8X8 && max_tx >= TX_32X32 {
                    tx += r.read(self.fc.tx32[ctx][2]) as u8;
                }
            }
            if let Some(c) = self.counts.as_mut() {
                match max_tx {
                    1 => c.tx8[ctx][tx as usize] += 1,
                    2 => c.tx16[ctx][tx as usize] += 1,
                    _ => c.tx32[ctx][tx as usize] += 1,
                }
            }
            tx
        } else {
            max_tx.min(TX_MODE_TO_BIGGEST[self.tx_mode as usize])
        }
    }

    fn intra_frame_mode_info(&mut self, r: &mut BoolDecoder, b: &Blk) -> ModeInfo {
        let mut mi = ModeInfo { sb_type: b.bsize, ..Default::default() };
        if self.seg.enabled {
            if self.seg.update_map {
                mi.segment_id = self.read_segment_id(r);
                self.set_segment(b, mi.segment_id);
            } else {
                self.copy_segment(b);
            }
        }
        mi.skip = self.read_skip(r, b, mi.segment_id);
        mi.tx_size = self.read_tx_size(r, b, true);
        let above = self.above_mi(b).copied();
        let left = self.left_mi(b).copied();
        let above_mode = |cur: &[u8; 4], blk: usize| -> u8 {
            if blk < 2 { above.filter(|m| !m.is_inter()).map_or(DC_PRED, |m| m.sub_modes[blk + 2]) } else { cur[blk - 2] }
        };
        let left_mode = |cur: &[u8; 4], blk: usize| -> u8 {
            if blk & 1 == 0 { left.filter(|m| !m.is_inter()).map_or(DC_PRED, |m| m.sub_modes[blk + 1]) } else { cur[blk - 1] }
        };
        if b.bsize >= BLOCK_8X8 {
            let a = above_mode(&mi.sub_modes, 0);
            let l = left_mode(&mi.sub_modes, 0);
            mi.mode = r.tree(&INTRA_MODE_TREE, &KF_Y_MODE_PROBS[a as usize][l as usize]);
            mi.sub_modes = [mi.mode; 4];
        } else {
            let w4 = NUM_4X4_W[b.bsize as usize] as usize;
            let h4 = NUM_4X4_H[b.bsize as usize] as usize;
            for idy in (0..2).step_by(h4) {
                for idx in (0..2).step_by(w4) {
                    let blk = idy * 2 + idx;
                    let a = above_mode(&mi.sub_modes, blk);
                    let l = left_mode(&mi.sub_modes, blk);
                    let m = r.tree(&INTRA_MODE_TREE, &KF_Y_MODE_PROBS[a as usize][l as usize]);
                    mi.sub_modes[blk] = m;
                    if h4 == 2 {
                        mi.sub_modes[blk + 2] = m;
                    }
                    if w4 == 2 {
                        mi.sub_modes[blk + 1] = m;
                    }
                }
            }
            mi.mode = mi.sub_modes[3];
        }
        mi.uv_mode = r.tree(&INTRA_MODE_TREE, &KF_UV_MODE_PROBS[mi.mode as usize]);
        mi
    }

    fn inter_frame_mode_info(&mut self, r: &mut BoolDecoder, b: &Blk) -> ModeInfo {
        let mut mi = ModeInfo { sb_type: b.bsize, ..Default::default() };
        // Segment id.
        if self.seg.enabled {
            let predicted = self.predicted_segment(b);
            if !self.seg.update_map {
                self.copy_segment(b);
                mi.segment_id = predicted;
            } else {
                if self.seg.temporal_update {
                    let ctx = self.above_mi(b).map_or(0, |m| m.seg_id_predicted as usize)
                        + self.left_mi(b).map_or(0, |m| m.seg_id_predicted as usize);
                    mi.seg_id_predicted = r.read(self.seg.pred_probs[ctx]);
                    mi.segment_id = if mi.seg_id_predicted { predicted } else { self.read_segment_id(r) };
                } else {
                    mi.segment_id = self.read_segment_id(r);
                }
                self.set_segment(b, mi.segment_id);
            }
        }
        mi.skip = self.read_skip(r, b, mi.segment_id);
        // Intra or inter.
        let is_inter = if self.seg.active(mi.segment_id, SEG_LVL_REF_FRAME) {
            self.seg.data(mi.segment_id, SEG_LVL_REF_FRAME) != INTRA_FRAME as i16
        } else {
            let a = self.above_mi(b).copied();
            let l = self.left_mi(b).copied();
            let ctx = match (a, l) {
                (Some(a), Some(l)) => {
                    let (ai, li) = (!a.is_inter(), !l.is_inter());
                    if ai && li { 3 } else { (ai || li) as usize }
                }
                (Some(e), None) | (None, Some(e)) => 2 * (!e.is_inter()) as usize,
                (None, None) => 0,
            };
            let v = r.read(self.fc.intra_inter[ctx]);
            if let Some(c) = self.counts.as_mut() {
                c.intra_inter[ctx][v as usize] += 1;
            }
            v
        };
        mi.tx_size = self.read_tx_size(r, b, !mi.skip || !is_inter);
        if is_inter {
            self.inter_block_mode_info(r, b, &mut mi);
        } else {
            self.intra_block_mode_info(r, b, &mut mi);
        }
        mi
    }

    fn read_y_mode(&mut self, r: &mut BoolDecoder, group: usize) -> u8 {
        let m = r.tree(&INTRA_MODE_TREE, &self.fc.y_mode[group]);
        if let Some(c) = self.counts.as_mut() {
            c.y_mode[group][m as usize] += 1;
        }
        m
    }

    fn intra_block_mode_info(&mut self, r: &mut BoolDecoder, b: &Blk, mi: &mut ModeInfo) {
        match b.bsize {
            BLOCK_4X4 => {
                for i in 0..4 {
                    mi.sub_modes[i] = self.read_y_mode(r, 0);
                }
            }
            BLOCK_4X8 => {
                let m = self.read_y_mode(r, 0);
                mi.sub_modes[0] = m;
                mi.sub_modes[2] = m;
                let m = self.read_y_mode(r, 0);
                mi.sub_modes[1] = m;
                mi.sub_modes[3] = m;
            }
            BLOCK_8X4 => {
                let m = self.read_y_mode(r, 0);
                mi.sub_modes[0] = m;
                mi.sub_modes[1] = m;
                let m = self.read_y_mode(r, 0);
                mi.sub_modes[2] = m;
                mi.sub_modes[3] = m;
            }
            _ => {
                let m = self.read_y_mode(r, SIZE_GROUP[b.bsize as usize] as usize);
                mi.sub_modes = [m; 4];
            }
        }
        mi.mode = mi.sub_modes[3];
        mi.uv_mode = r.tree(&INTRA_MODE_TREE, &self.fc.uv_mode[mi.mode as usize]);
        if let Some(c) = self.counts.as_mut() {
            c.uv_mode[mi.mode as usize][mi.uv_mode as usize] += 1;
        }
        mi.interp_filter = 3;
        mi.ref_frame = [INTRA_FRAME, NONE_FRAME];
    }

    // ---------------------------------------------------------------
    // Inter blocks
    // ---------------------------------------------------------------

    fn read_ref_frames(&mut self, r: &mut BoolDecoder, b: &Blk, seg_id: u8) -> [i8; 2] {
        if self.seg.active(seg_id, SEG_LVL_REF_FRAME) {
            return [self.seg.data(seg_id, SEG_LVL_REF_FRAME) as i8, NONE_FRAME];
        }
        let a = self.above_mi(b).copied();
        let l = self.left_mi(b).copied();
        let mode = if self.reference_mode == REFERENCE_MODE_SELECT {
            let ctx = self.comp_inter_ctx(a, l);
            let v = r.read(self.fc.comp_inter[ctx]);
            if let Some(c) = self.counts.as_mut() {
                c.comp_inter[ctx][v as usize] += 1;
            }
            v as u8
        } else {
            self.reference_mode
        };
        if mode == COMPOUND_REFERENCE {
            let idx = self.h.sign_bias[self.comp_fixed_ref as usize] as usize;
            let ctx = self.comp_ref_ctx(a, l);
            let bit = r.read(self.fc.comp_ref[ctx]);
            if let Some(c) = self.counts.as_mut() {
                c.comp_ref[ctx][bit as usize] += 1;
            }
            let mut rf = [NONE_FRAME; 2];
            rf[idx] = self.comp_fixed_ref;
            rf[1 - idx] = self.comp_var_ref[bit as usize];
            rf
        } else {
            let ctx0 = single_ref_p1(a, l);
            let bit0 = r.read(self.fc.single_ref[ctx0][0]);
            if let Some(c) = self.counts.as_mut() {
                c.single_ref[ctx0][0][bit0 as usize] += 1;
            }
            if bit0 {
                let ctx1 = single_ref_p2(a, l);
                let bit1 = r.read(self.fc.single_ref[ctx1][1]);
                if let Some(c) = self.counts.as_mut() {
                    c.single_ref[ctx1][1][bit1 as usize] += 1;
                }
                [if bit1 { ALTREF_FRAME } else { GOLDEN_FRAME }, NONE_FRAME]
            } else {
                [LAST_FRAME, NONE_FRAME]
            }
        }
    }

    fn comp_inter_ctx(&self, a: Option<ModeInfo>, l: Option<ModeInfo>) -> usize {
        let fixed = self.comp_fixed_ref;
        match (a, l) {
            (Some(a), Some(l)) => {
                if !a.has_second_ref() && !l.has_second_ref() {
                    ((a.ref_frame[0] == fixed) ^ (l.ref_frame[0] == fixed)) as usize
                } else if !a.has_second_ref() {
                    2 + (a.ref_frame[0] == fixed || !a.is_inter()) as usize
                } else if !l.has_second_ref() {
                    2 + (l.ref_frame[0] == fixed || !l.is_inter()) as usize
                } else {
                    4
                }
            }
            (Some(e), None) | (None, Some(e)) => {
                if !e.has_second_ref() {
                    (e.ref_frame[0] == fixed) as usize
                } else {
                    3
                }
            }
            (None, None) => 1,
        }
    }

    fn comp_ref_ctx(&self, a: Option<ModeInfo>, l: Option<ModeInfo>) -> usize {
        let fix_idx = self.h.sign_bias[self.comp_fixed_ref as usize] as usize;
        let var_idx = 1 - fix_idx;
        let var1 = self.comp_var_ref[1];
        let var0 = self.comp_var_ref[0];
        let fixed = self.comp_fixed_ref;
        match (a, l) {
            (Some(a), Some(l)) => {
                let (ai, li) = (!a.is_inter(), !l.is_inter());
                if ai && li {
                    2
                } else if ai || li {
                    let e = if ai { l } else { a };
                    if !e.has_second_ref() {
                        1 + 2 * (e.ref_frame[0] != var1) as usize
                    } else {
                        1 + 2 * (e.ref_frame[var_idx] != var1) as usize
                    }
                } else {
                    let l_sg = !l.has_second_ref();
                    let a_sg = !a.has_second_ref();
                    let vrfa = if a_sg { a.ref_frame[0] } else { a.ref_frame[var_idx] };
                    let vrfl = if l_sg { l.ref_frame[0] } else { l.ref_frame[var_idx] };
                    if vrfa == vrfl && var1 == vrfa {
                        0
                    } else if l_sg && a_sg {
                        if (vrfa == fixed && vrfl == var0) || (vrfl == fixed && vrfa == var0) {
                            4
                        } else if vrfa == vrfl {
                            3
                        } else {
                            1
                        }
                    } else if l_sg || a_sg {
                        let vrfc = if l_sg { vrfa } else { vrfl };
                        let rfs = if a_sg { vrfa } else { vrfl };
                        if vrfc == var1 && rfs != var1 {
                            1
                        } else if rfs == var1 && vrfc != var1 {
                            2
                        } else {
                            4
                        }
                    } else if vrfa == vrfl {
                        4
                    } else {
                        2
                    }
                }
            }
            (Some(e), None) | (None, Some(e)) => {
                if !e.is_inter() {
                    2
                } else if e.has_second_ref() {
                    4 * (e.ref_frame[var_idx] != var1) as usize
                } else {
                    3 * (e.ref_frame[0] != var1) as usize
                }
            }
            (None, None) => 2,
        }
    }

    fn is_inside(&self, b: &Blk, dr: i8, dc: i8) -> bool {
        let r = b.r as i32 + dr as i32;
        let c = b.c as i32 + dc as i32;
        !(r < 0 || c < self.tile.mi_col_start as i32 || r >= self.mi_rows as i32 || c >= self.tile.mi_col_end as i32)
    }

    fn mode_context(&self, b: &Blk) -> usize {
        let mut counter = 0usize;
        for &(dr, dc) in &MV_REF_BLOCKS[b.bsize as usize][..2] {
            if self.is_inside(b, dr, dc) {
                let m = &self.mi[(b.r as i32 + dr as i32) as usize * self.mi_cols + (b.c as i32 + dc as i32) as usize];
                counter += MODE_2_COUNTER[m.mode as usize] as usize;
            }
        }
        COUNTER_TO_CONTEXT[counter] as usize
    }

    /// `find_mv_refs` (with `block` = -1 for the whole block).
    fn find_mv_refs(&self, b: &Blk, ref_frame: i8, block: i32) -> [Mv; 2] {
        let mut list = [Mv::default(); 2];
        let mut count = 0;
        let sb = &self.h.sign_bias;
        let search = &MV_REF_BLOCKS[b.bsize as usize];
        let mut done = false;
        let add = |mv: Mv, list: &mut [Mv; 2], count: &mut usize, done: &mut bool| {
            if *done {
                return;
            }
            if *count > 0 {
                if mv != list[0] {
                    list[1] = mv;
                    *done = true;
                }
            } else {
                list[0] = mv;
                *count = 1;
            }
        };
        let scale = |mv: Mv, rf: i8| -> Mv {
            if sb[rf.max(0) as usize] != sb[ref_frame.max(0) as usize] { Mv { row: -mv.row, col: -mv.col } } else { mv }
        };
        let cell = |dr: i8, dc: i8| -> &ModeInfo { &self.mi[(b.r as i32 + dr as i32) as usize * self.mi_cols + (b.c as i32 + dc as i32) as usize] };
        let mut different_ref_found = false;
        for (i, &(dr, dc)) in search.iter().enumerate() {
            if done {
                break;
            }
            if !self.is_inside(b, dr, dc) {
                continue;
            }
            let cand = cell(dr, dc);
            different_ref_found = true;
            for which in 0..2 {
                if cand.ref_frame[which] == ref_frame {
                    let mv = if i < 2 && block >= 0 && cand.sb_type < BLOCK_8X8 {
                        cand.bmv[IDX_N_COLUMN_TO_SUBBLOCK[block as usize][(dc == 0) as usize]][which]
                    } else {
                        cand.mv[which]
                    };
                    add(mv, &mut list, &mut count, &mut done);
                    break;
                }
            }
        }
        let prev = if !done { self.prev_mvs.map(|p| p[b.r * self.mi_cols + b.c]) } else { None };
        if let Some(p) = prev {
            if p.ref_frame[0] == ref_frame {
                add(p.mv[0], &mut list, &mut count, &mut done);
            } else if p.ref_frame[1] == ref_frame {
                add(p.mv[1], &mut list, &mut count, &mut done);
            }
        }
        if different_ref_found && !done {
            for &(dr, dc) in search.iter() {
                if done {
                    break;
                }
                if !self.is_inside(b, dr, dc) {
                    continue;
                }
                let cand = cell(dr, dc);
                if cand.is_inter() {
                    if cand.ref_frame[0] != ref_frame {
                        add(scale(cand.mv[0], cand.ref_frame[0]), &mut list, &mut count, &mut done);
                    }
                    if cand.has_second_ref() && cand.ref_frame[1] != ref_frame && cand.mv[1] != cand.mv[0] {
                        add(scale(cand.mv[1], cand.ref_frame[1]), &mut list, &mut count, &mut done);
                    }
                }
            }
        }
        if let Some(p) = prev
            && !done
        {
            if p.ref_frame[0] != ref_frame && p.ref_frame[0] > INTRA_FRAME {
                add(scale(p.mv[0], p.ref_frame[0]), &mut list, &mut count, &mut done);
            }
            if p.ref_frame[1] > INTRA_FRAME && p.ref_frame[1] != ref_frame && p.mv[1] != p.mv[0] {
                add(scale(p.mv[1], p.ref_frame[1]), &mut list, &mut count, &mut done);
            }
        }
        // clamp_mv_ref
        for mv in list.iter_mut() {
            mv.col = (mv.col as i32).clamp(b.to_left - 128, b.to_right + 128) as i16;
            mv.row = (mv.row as i32).clamp(b.to_top - 128, b.to_bottom + 128) as i16;
        }
        list
    }

    fn read_mv_component(&mut self, r: &mut BoolDecoder, comp: usize, use_hp: bool) -> i16 {
        let p = &self.fc.mv[comp];
        let sign = r.read(p.sign);
        let class = r.tree(&MV_CLASS_TREE, &p.classes) as u32;
        let (d, mag0) = if class == 0 {
            (r.read(p.class0[0]) as u32, 0u32)
        } else {
            let n = class;
            let mut d = 0u32;
            for i in 0..n {
                d |= (r.read(p.bits[i as usize]) as u32) << i;
            }
            (d, 2 << (class + 2))
        };
        let fr = r.tree(&MV_FP_TREE, if class == 0 { &p.class0_fp[d as usize] } else { &p.fp }) as u32;
        let hp = if use_hp { r.read(if class == 0 { p.class0_hp } else { p.hp }) as u32 } else { 1 };
        let mag = mag0 + ((d << 3) | (fr << 1) | hp) + 1;
        if sign { -(mag as i32) as i16 } else { mag as i16 }
    }

    fn count_mv_component(c: &mut super::probs::MvComponentCounts, v: i16) {
        let s = (v < 0) as usize;
        c.sign[s] += 1;
        let z = v.unsigned_abs() as u32 - 1;
        let zz = z >> 3;
        let class = if z >= 2 * 4096 { 10 } else if zz == 0 { 0 } else { (31 - zz.leading_zeros()) as usize };
        let base = if class > 0 { 2u32 << (class + 2) } else { 0 };
        let o = z - base;
        c.classes[class] += 1;
        let d = (o >> 3) as usize;
        let f = ((o >> 1) & 3) as usize;
        let e = (o & 1) as usize;
        if class == 0 {
            c.class0[d] += 1;
            c.class0_fp[d][f] += 1;
            c.class0_hp[e] += 1;
        } else {
            for i in 0..class {
                c.bits[i][(d >> i) & 1] += 1;
            }
            c.fp[f] += 1;
            c.hp[e] += 1;
        }
    }

    fn read_mv(&mut self, r: &mut BoolDecoder, base: Mv) -> Mv {
        let joint = r.tree(&MV_JOINT_TREE, &self.fc.mv_joints);
        let use_hp = self.h.allow_high_precision_mv && (base.row.unsigned_abs() >> 3) < 8 && (base.col.unsigned_abs() >> 3) < 8;
        let mut diff = Mv::default();
        if joint == 2 || joint == 3 {
            diff.row = self.read_mv_component(r, 0, use_hp);
        }
        if joint == 1 || joint == 3 {
            diff.col = self.read_mv_component(r, 1, use_hp);
        }
        if let Some(c) = self.counts.as_mut() {
            let j = match (diff.row != 0, diff.col != 0) {
                (false, false) => 0,
                (false, true) => 1,
                (true, false) => 2,
                (true, true) => 3,
            };
            c.mv_joints[j] += 1;
            if diff.row != 0 {
                Self::count_mv_component(&mut c.mv[0], diff.row);
            }
            if diff.col != 0 {
                Self::count_mv_component(&mut c.mv[1], diff.col);
            }
        }
        let mv = Mv { row: base.row.wrapping_add(diff.row), col: base.col.wrapping_add(diff.col) };
        if !((mv.row as i32) > -(1 << 14) && (mv.row as i32) < (1 << 14) && (mv.col as i32) > -(1 << 14) && (mv.col as i32) < (1 << 14)) {
            self.corrupted = true;
        }
        mv
    }

    fn read_inter_mode(&mut self, r: &mut BoolDecoder, ctx: usize) -> u8 {
        let v = r.tree(&INTER_MODE_TREE, &self.fc.inter_mode[ctx]);
        if let Some(c) = self.counts.as_mut() {
            c.inter_mode[ctx][v as usize] += 1;
        }
        NEARESTMV + v
    }

    fn inter_block_mode_info(&mut self, r: &mut BoolDecoder, b: &Blk, mi: &mut ModeInfo) {
        mi.ref_frame = self.read_ref_frames(r, b, mi.segment_id);
        let compound = mi.has_second_ref();
        let ctx = self.mode_context(b);
        let allow_hp = self.h.allow_high_precision_mv;
        if self.seg.active(mi.segment_id, SEG_LVL_SKIP) {
            mi.mode = ZEROMV;
            if b.bsize < BLOCK_8X8 {
                self.corrupted = true;
            }
        } else if b.bsize >= BLOCK_8X8 {
            mi.mode = self.read_inter_mode(r, ctx);
        }
        mi.interp_filter = if self.h.interp_filter == SWITCHABLE {
            let a = self.above_mi(b).copied();
            let l = self.left_mi(b).copied();
            let lt = l.filter(|m| m.is_inter()).map_or(3, |m| m.interp_filter);
            let at = a.filter(|m| m.is_inter()).map_or(3, |m| m.interp_filter);
            let fctx = if lt == at {
                lt
            } else if lt == 3 {
                at
            } else if at == 3 {
                lt
            } else {
                3
            } as usize;
            let f = r.tree(&SWITCHABLE_INTERP_TREE, &self.fc.switchable_interp[fctx]);
            if let Some(c) = self.counts.as_mut() {
                c.switchable_interp[fctx][f as usize] += 1;
            }
            f
        } else {
            self.h.interp_filter
        };
        let nrefs = 1 + compound as usize;
        let mut best = [Mv::default(); 2];
        let mut near_nearest = [[Mv::default(); 2]; 2];
        if b.bsize < BLOCK_8X8 || mi.mode != ZEROMV {
            for rf in 0..nrefs {
                let mut l = self.find_mv_refs(b, mi.ref_frame[rf], -1);
                for mv in l.iter_mut() {
                    lower_mv_precision(mv, allow_hp);
                }
                best[rf] = l[0];
                near_nearest[rf] = l;
            }
        }
        if b.bsize < BLOCK_8X8 {
            let w4 = NUM_4X4_W[b.bsize as usize] as usize;
            let h4 = NUM_4X4_H[b.bsize as usize] as usize;
            let mut b_mode = ZEROMV;
            for idy in (0..2).step_by(h4) {
                for idx in (0..2).step_by(w4) {
                    let j = idy * 2 + idx;
                    b_mode = self.read_inter_mode(r, ctx);
                    let mut sub = [Mv::default(); 2];
                    if b_mode == NEARESTMV || b_mode == NEARMV {
                        for rf in 0..nrefs {
                            let (nearest, near) = self.sub8x8_mvs(b, mi, j, rf);
                            sub[rf] = if b_mode == NEARESTMV { nearest } else { near };
                        }
                    }
                    for rf in 0..nrefs {
                        mi.bmv[j][rf] = match b_mode {
                            NEWMV => self.read_mv(r, best[rf]),
                            NEARESTMV | NEARMV => sub[rf],
                            _ => Mv::default(),
                        };
                    }
                    if h4 == 2 {
                        mi.bmv[j + 2] = mi.bmv[j];
                    }
                    if w4 == 2 {
                        mi.bmv[j + 1] = mi.bmv[j];
                    }
                }
            }
            mi.mode = b_mode;
            mi.mv = mi.bmv[3];
        } else {
            for rf in 0..nrefs {
                mi.mv[rf] = match mi.mode {
                    NEWMV => self.read_mv(r, best[rf]),
                    NEARESTMV => near_nearest[rf][0],
                    NEARMV => near_nearest[rf][1],
                    _ => Mv::default(),
                };
            }
            mi.bmv = [mi.mv; 4];
        }
    }

    /// `append_sub8x8_mvs_for_idx`: (nearest, near) of sub-block `j`.
    fn sub8x8_mvs(&self, b: &Blk, mi: &ModeInfo, j: usize, rf: usize) -> (Mv, Mv) {
        let list = self.find_mv_refs(b, mi.ref_frame[rf], j as i32);
        match j {
            0 => (list[0], list[1]),
            1 | 2 => {
                let nearest = mi.bmv[0][rf];
                let near = list.iter().copied().find(|m| *m != nearest).unwrap_or_default();
                (nearest, near)
            }
            _ => {
                let nearest = mi.bmv[2][rf];
                let cands = [mi.bmv[1][rf], mi.bmv[0][rf], list[0], list[1]];
                let near = cands.iter().copied().find(|m| *m != nearest).unwrap_or_default();
                (nearest, near)
            }
        }
    }

    // ---------------------------------------------------------------
    // Residual and reconstruction
    // ---------------------------------------------------------------

    /// Prediction plus residual of all planes; returns the summed eobs.
    fn reconstruct(&mut self, r: &mut BoolDecoder, b: &Blk, mi: &ModeInfo) -> u32 {
        let bs8 = b.bsize.max(BLOCK_8X8);
        let inter = mi.is_inter();
        if inter {
            self.predict_inter_block(b, mi);
        }
        let mut eob_total = 0u32;
        for plane in 0..3 {
            let (sx, sy) = if plane == 0 { (0usize, 0usize) } else { (self.cur.ss_x as usize, self.cur.ss_y as usize) };
            let tx = if plane == 0 {
                mi.tx_size
            } else if b.bsize < BLOCK_8X8 {
                TX_4X4
            } else {
                let uvb = SS_SIZE[b.bsize as usize][sx][sy];
                mi.tx_size.min(MAX_TXSIZE[uvb as usize])
            };
            let n4w = (NUM_4X4_W[bs8 as usize] as usize >> sx).max(1);
            let n4h = (NUM_4X4_H[bs8 as usize] as usize >> sy).max(1);
            let step = 1usize << tx;
            let max_w = if b.to_right < 0 { (n4w as i32 + (b.to_right >> (5 + sx))) as usize } else { n4w };
            let max_h = if b.to_bottom < 0 { (n4h as i32 + (b.to_bottom >> (5 + sy))) as usize } else { n4h };
            let px = (b.c * 8) >> sx;
            let py = (b.r * 8) >> sy;
            let ax = (b.c * 2) >> sx;
            let ly = ((b.r & 7) * 2) >> sy;
            let plane_type = (plane > 0) as usize;
            for row in (0..max_h).step_by(step) {
                for col in (0..max_w).step_by(step) {
                    let x0 = px + col * 4;
                    let y0 = py + row * 4;
                    let mode = if plane == 0 {
                        if b.bsize < BLOCK_8X8 { mi.sub_modes[(row << 1) + col] } else { mi.mode }
                    } else {
                        mi.uv_mode
                    };
                    if !inter {
                        let pl = &mut self.cur.planes[plane];
                        let have_top = row > 0 || b.has_above;
                        let have_left = col > 0 || b.has_left;
                        let have_right = col + step < n4w;
                        let (aw, ah, stride) = (pl.aw, pl.ah, pl.stride);
                        predict_intra(&mut pl.data, stride, x0, y0, aw, ah, mode, tx, have_top, have_left, have_right);
                    }
                    if !mi.skip {
                        let tx_type = if plane > 0 || self.h.lossless || inter || tx == TX_32X32 {
                            DCT_DCT
                        } else {
                            INTRA_MODE_TO_TX_TYPE[mode as usize]
                        };
                        let eob = self.tokens(r, plane, plane_type, inter, tx, tx_type, ax + col, ly + row, max_w - col, max_h - row, mi.segment_id);
                        if eob > 0 {
                            eob_total += eob as u32;
                            let pl = &mut self.cur.planes[plane];
                            let off = y0 * pl.stride + x0;
                            let stride = pl.stride;
                            idct::inverse_add(&self.coef, &mut pl.data, off, stride, tx, tx_type, self.h.lossless);
                            // Clear the coefficients that were written.
                            let order = &scan_orders()[tx as usize][tx_type as usize];
                            for &rc in &order.scan[..eob] {
                                self.coef[rc as usize] = 0;
                            }
                        }
                    }
                }
            }
        }
        eob_total
    }

    /// Reads the tokens of one transform block into `self.coef`
    /// (dequantized, raster order). Returns the end of block.
    #[allow(clippy::too_many_arguments)]
    fn tokens(
        &mut self,
        r: &mut BoolDecoder,
        plane: usize,
        plane_type: usize,
        inter: bool,
        tx: u8,
        tx_type: u8,
        ax: usize,
        ly: usize,
        room_w: usize,
        room_h: usize,
        seg: u8,
    ) -> usize {
        let n4 = 1usize << tx;
        let above = &self.above_nz[plane][ax..ax + n4];
        let left = &self.left_nz[plane][ly..(ly + n4).min(16)];
        let ctx = above.iter().any(|v| *v != 0) as usize + left.iter().any(|v| *v != 0) as usize;
        let order = &scan_orders()[tx as usize][tx_type as usize];
        let scan = order.scan;
        let nb = &order.neighbors;
        let probs = &self.fc.coef[tx as usize][plane_type][inter as usize];
        let dq = self.dequant[seg as usize][plane_type];
        let max_eob = 16usize << (tx << 1);
        let band_of = |c: usize| -> usize { if tx == TX_4X4 { COEFBAND_4X4[c] as usize } else { COEFBAND_8X8PLUS[c] as usize } };
        let dq_shift = (tx == TX_32X32) as i32;
        let mut c = 0usize;
        let mut ctx = ctx;
        let mut dqv = dq[0];
        // Counts go straight into the frame counts (no lists per block).
        let (t, ii) = (tx as usize, inter as usize);
        let mut cnt = self.counts.as_deref_mut();
        let token_cache = &mut self.token_cache;
        while c < max_eob {
            let mut band = band_of(c);
            let mut prob = &probs[band][ctx];
            if let Some(cn) = cnt.as_mut() {
                cn.eob_branch[t][plane_type][ii][band][ctx] += 1;
            }
            if !r.read(prob[0]) {
                if let Some(cn) = cnt.as_mut() {
                    cn.coef[t][plane_type][ii][band][ctx][3] += 1;
                }
                break;
            }
            let mut ended = false;
            while !r.read(prob[1]) {
                if let Some(cn) = cnt.as_mut() {
                    cn.coef[t][plane_type][ii][band][ctx][0] += 1;
                }
                dqv = dq[1];
                token_cache[scan[c] as usize] = 0;
                c += 1;
                if c >= max_eob {
                    ended = true;
                    break;
                }
                ctx = ((1 + token_cache[nb[2 * c] as usize] as usize + token_cache[nb[2 * c + 1] as usize] as usize) >> 1) as usize;
                band = band_of(c);
                prob = &probs[band][ctx];
            }
            if ended {
                break;
            }
            let val: i32;
            if !r.read(prob[2]) {
                if let Some(cn) = cnt.as_mut() {
                    cn.coef[t][plane_type][ii][band][ctx][1] += 1;
                }
                token_cache[scan[c] as usize] = 1;
                val = 1;
            } else {
                if let Some(cn) = cnt.as_mut() {
                    cn.coef[t][plane_type][ii][band][ctx][2] += 1;
                }
                let p = &PARETO8[prob[2] as usize - 1];
                if !r.read(p[0]) {
                    if !r.read(p[1]) {
                        token_cache[scan[c] as usize] = 2;
                        val = 2;
                    } else {
                        token_cache[scan[c] as usize] = 3;
                        val = 3 + r.read(p[2]) as i32;
                    }
                } else if !r.read(p[3]) {
                    token_cache[scan[c] as usize] = 4;
                    if !r.read(p[4]) {
                        val = 5 + read_coeff(r, &CAT1_PROB);
                    } else {
                        val = 7 + read_coeff(r, &CAT2_PROB);
                    }
                } else {
                    token_cache[scan[c] as usize] = 5;
                    if !r.read(p[5]) {
                        if !r.read(p[6]) {
                            val = 11 + read_coeff(r, &CAT3_PROB);
                        } else {
                            val = 19 + read_coeff(r, &CAT4_PROB);
                        }
                    } else if !r.read(p[7]) {
                        val = 35 + read_coeff(r, &CAT5_PROB);
                    } else {
                        val = 67 + read_coeff(r, &CAT6_PROB);
                    }
                }
            }
            let v = (val * dqv) >> dq_shift;
            self.coef[scan[c] as usize] = if r.read(128) { -v } else { v };
            c += 1;
            if c < max_eob {
                ctx = ((1 + token_cache[nb[2 * c] as usize] as usize + token_cache[nb[2 * c + 1] as usize] as usize) >> 1) as usize;
            }
            dqv = dq[1];
        }
        // Entropy contexts (zero beyond the frame edge).
        let nz = (c > 0) as u8;
        for k in 0..n4 {
            self.above_nz[plane][ax + k] = if k < room_w { nz } else { 0 };
        }
        for k in 0..n4 {
            if ly + k < 16 {
                self.left_nz[plane][ly + k] = if k < room_h { nz } else { 0 };
            }
        }
        c
    }

    fn predict_inter_block(&mut self, b: &Blk, mi: &ModeInfo) {
        let filter = &FILTERS[mi.interp_filter.min(3) as usize];
        let bs8 = b.bsize.max(BLOCK_8X8);
        for rf in 0..(1 + mi.has_second_ref() as usize) {
            let idx = (mi.ref_frame[rf] - 1).clamp(0, 2) as usize;
            let Some((refbuf, sc)) = self.refs[idx] else {
                self.corrupted = true;
                continue;
            };
            for plane in 0..3 {
                let (sx, sy) = if plane == 0 { (0usize, 0usize) } else { (self.cur.ss_x as usize, self.cur.ss_y as usize) };
                let n4w = (NUM_4X4_W[bs8 as usize] as usize >> sx).max(1);
                let n4h = (NUM_4X4_H[bs8 as usize] as usize >> sy).max(1);
                let rp = &refbuf.planes[plane];
                let rplane = RefPlane { data: &rp.data, stride: rp.stride, w: rp.w as i32, h: rp.h as i32 };
                let bw = n4w * 4;
                let bh = n4h * 4;
                if b.bsize < BLOCK_8X8 {
                    let mut i = 0;
                    for y in 0..n4h {
                        for x in 0..n4w {
                            let mv = if sx == 0 && sy == 0 {
                                mi.bmv[i][rf]
                            } else if sx == 1 && sy == 1 {
                                let s = |f: fn(&Mv) -> i16| -> i16 {
                                    let v: i32 = (0..4).map(|k| f(&mi.bmv[k][rf]) as i32).sum();
                                    ((if v < 0 { v - 2 } else { v + 2 }) / 4) as i16
                                };
                                Mv { row: s(|m| m.row), col: s(|m| m.col) }
                            } else {
                                let other = if sx == 1 { i + 1 } else { i + 2 };
                                let avg = |a: i16, c: i16| -> i16 {
                                    let v = a as i32 + c as i32;
                                    ((if v < 0 { v - 1 } else { v + 1 }) / 2) as i16
                                };
                                let (m0, m1) = (mi.bmv[i][rf], mi.bmv[other.min(3)][rf]);
                                Mv { row: avg(m0.row, m1.row), col: avg(m0.col, m1.col) }
                            };
                            i += 1;
                            self.build_pred(b, plane, sx, sy, bw, bh, 4 * x, 4 * y, 4, 4, mv, &rplane, &sc, filter, rf == 1);
                        }
                    }
                } else {
                    self.build_pred(b, plane, sx, sy, bw, bh, 0, 0, bw, bh, mi.mv[rf], &rplane, &sc, filter, rf == 1);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_pred(
        &mut self,
        b: &Blk,
        plane: usize,
        sx: usize,
        sy: usize,
        bw: usize,
        bh: usize,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        mv: Mv,
        rp: &RefPlane,
        sc: &Scale,
        filter: &[[i16; 8]; 16],
        avg: bool,
    ) {
        let x_start = (-b.to_left) >> (3 + sx);
        let y_start = (-b.to_top) >> (3 + sy);
        let (x0, y0, mvx, mvy, xs, ys);
        if sc.scaled {
            // clamp_mv_to_umv_border_sb
            let spel_left = ((4 + bw as i32) << 4) as i32;
            let spel_right = spel_left - 16;
            let spel_top = (4 + bh as i32) << 4;
            let spel_bottom = spel_top - 16;
            let mut cr = mv.row as i32 * (1 << (1 - sy));
            let mut cc = mv.col as i32 * (1 << (1 - sx));
            cc = cc.clamp(b.to_left * (1 << (1 - sx)) - spel_left, b.to_right * (1 << (1 - sx)) + spel_right);
            cr = cr.clamp(b.to_top * (1 << (1 - sy)) - spel_top, b.to_bottom * (1 << (1 - sy)) + spel_bottom);
            let mi_x = (b.c * 8) as i64;
            let mi_y = (b.r * 8) as i64;
            let x_off = sc.sx((mi_x + x as i64) << 4) & 15;
            let y_off = sc.sy((mi_y + y as i64) << 4) & 15;
            mvy = (sc.sy(cr as i64) + y_off) as i32;
            mvx = (sc.sx(cc as i64) + x_off) as i32;
            x0 = sc.sx((x_start + x as i32) as i64) as i32;
            y0 = sc.sy((y_start + y as i32) as i64) as i32;
            xs = sc.x_step;
            ys = sc.y_step;
        } else {
            x0 = x_start + x as i32;
            y0 = y_start + y as i32;
            mvy = mv.row as i32 * (1 << (1 - sy));
            mvx = mv.col as i32 * (1 << (1 - sx));
            xs = 16;
            ys = 16;
        }
        let fx = mvx & 15;
        let fy = mvy & 15;
        let rx = x0 + (mvx >> 4);
        let ry = y0 + (mvy >> 4);
        let pl = &mut self.cur.planes[plane];
        let doff = (y_start as usize + y) * pl.stride + x_start as usize + x;
        let stride = pl.stride;
        predict_inter(rp, &mut pl.data, doff, stride, rx, ry, fx, fy, xs, ys, w, h, filter, avg);
    }
}

fn read_coeff(r: &mut BoolDecoder, probs: &[u8]) -> i32 {
    let mut v = 0i32;
    for &p in probs {
        v = (v << 1) | r.read(p) as i32;
    }
    v
}

fn single_ref_p1(a: Option<ModeInfo>, l: Option<ModeInfo>) -> usize {
    let last = LAST_FRAME;
    match (a, l) {
        (Some(a), Some(l)) => {
            let (ai, li) = (!a.is_inter(), !l.is_inter());
            if ai && li {
                2
            } else if ai || li {
                let e = if ai { l } else { a };
                if !e.has_second_ref() {
                    4 * (e.ref_frame[0] == last) as usize
                } else {
                    1 + (e.ref_frame[0] == last || e.ref_frame[1] == last) as usize
                }
            } else {
                let (a2, l2) = (a.has_second_ref(), l.has_second_ref());
                let (a0, a1, l0, l1) = (a.ref_frame[0], a.ref_frame[1], l.ref_frame[0], l.ref_frame[1]);
                if a2 && l2 {
                    1 + (a0 == last || a1 == last || l0 == last || l1 == last) as usize
                } else if a2 || l2 {
                    let rfs = if !a2 { a0 } else { l0 };
                    let crf1 = if a2 { a0 } else { l0 };
                    let crf2 = if a2 { a1 } else { l1 };
                    if rfs == last {
                        3 + (crf1 == last || crf2 == last) as usize
                    } else {
                        (crf1 == last || crf2 == last) as usize
                    }
                } else {
                    2 * (a0 == last) as usize + 2 * (l0 == last) as usize
                }
            }
        }
        (Some(e), None) | (None, Some(e)) => {
            if !e.is_inter() {
                2
            } else if !e.has_second_ref() {
                4 * (e.ref_frame[0] == last) as usize
            } else {
                1 + (e.ref_frame[0] == last || e.ref_frame[1] == last) as usize
            }
        }
        (None, None) => 2,
    }
}

fn single_ref_p2(a: Option<ModeInfo>, l: Option<ModeInfo>) -> usize {
    let (last, golden, alt) = (LAST_FRAME, GOLDEN_FRAME, ALTREF_FRAME);
    match (a, l) {
        (Some(a), Some(l)) => {
            let (ai, li) = (!a.is_inter(), !l.is_inter());
            if ai && li {
                2
            } else if ai || li {
                let e = if ai { l } else { a };
                if !e.has_second_ref() {
                    if e.ref_frame[0] == last { 3 } else { 4 * (e.ref_frame[0] == golden) as usize }
                } else {
                    1 + 2 * (e.ref_frame[0] == golden || e.ref_frame[1] == golden) as usize
                }
            } else {
                let (a2, l2) = (a.has_second_ref(), l.has_second_ref());
                let (a0, a1, l0, l1) = (a.ref_frame[0], a.ref_frame[1], l.ref_frame[0], l.ref_frame[1]);
                if a2 && l2 {
                    if a0 == l0 && a1 == l1 {
                        3 * (a0 == golden || a1 == golden || l0 == golden || l1 == golden) as usize
                    } else {
                        2
                    }
                } else if a2 || l2 {
                    let rfs = if !a2 { a0 } else { l0 };
                    let crf1 = if a2 { a0 } else { l0 };
                    let crf2 = if a2 { a1 } else { l1 };
                    if rfs == golden {
                        3 + (crf1 == golden || crf2 == golden) as usize
                    } else if rfs == alt {
                        (crf1 == golden || crf2 == golden) as usize
                    } else {
                        1 + 2 * (crf1 == golden || crf2 == golden) as usize
                    }
                } else if a0 == last && l0 == last {
                    3
                } else if a0 == last || l0 == last {
                    let edge0 = if a0 == last { l0 } else { a0 };
                    4 * (edge0 == golden) as usize
                } else {
                    2 * (a0 == golden) as usize + 2 * (l0 == golden) as usize
                }
            }
        }
        (Some(e), None) | (None, Some(e)) => {
            if !e.is_inter() || (e.ref_frame[0] == last && !e.has_second_ref()) {
                2
            } else if !e.has_second_ref() {
                4 * (e.ref_frame[0] == golden) as usize
            } else {
                3 * (e.ref_frame[0] == golden || e.ref_frame[1] == golden) as usize
            }
        }
        (None, None) => 2,
    }
}
