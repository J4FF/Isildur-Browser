//! A VP9 decoder (2026-10-15), written from the VP9 bitstream
//! specification and the conventions of the reference decoder: profile 0
//! (8-bit 4:2:0) first; 4:4:4 decodes too (profile 1 without 4:2:2/4:4:0).
//!
//! Use [`Decoder::decode`] per compressed packet (a WebM block or an MP4
//! sample); it returns the frames to show (a superframe can carry hidden
//! frames). [`FrameBuf::to_rgba`] converts a frame for display.

pub mod bits;
pub mod frame;
pub mod header;
pub mod idct;
pub mod loopfilter;
pub mod probs;
pub mod recon;
pub mod tables;

use std::sync::Arc;

pub use frame::FrameBuf;
use frame::{FrameDec, MvRef, Scale};
use header::{ColorConfig, FrameHeader, HeaderState, LoopFilterParams, Segmentation};
use probs::FrameContext;

/// Decoder state between frames.
pub struct Decoder {
    refs: [Option<Arc<FrameBuf>>; 8],
    contexts: [FrameContext; 4],
    lf: LoopFilterParams,
    seg: Segmentation,
    prev_seg: Vec<u8>,
    last_color: ColorConfig,
    last_size: (u32, u32),
    last_show_frame: bool,
    last_intra_only: bool,
    last_was_key: bool,
    prev_frame: Option<Arc<FrameBuf>>,
    /// The color configuration of the last decoded frame.
    pub color: ColorConfig,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Splits a packet into frames (the superframe index, VP9 spec Annex B).
pub fn split_superframe(data: &[u8]) -> Vec<&[u8]> {
    if let Some(&marker) = data.last()
        && marker & 0xe0 == 0xc0
    {
        let frames = (marker & 7) as usize + 1;
        let mag = ((marker >> 3) & 3) as usize + 1;
        let index = 2 + mag * frames;
        if data.len() >= index && data[data.len() - index] == marker {
            let mut out = Vec::new();
            let mut pos = 0usize;
            let mut p = data.len() - index + 1;
            for _ in 0..frames {
                let mut size = 0usize;
                for b in 0..mag {
                    size |= (data[p + b] as usize) << (8 * b);
                }
                p += mag;
                if size == 0 || pos + size > data.len() - index {
                    break;
                }
                out.push(&data[pos..pos + size]);
                pos += size;
            }
            return out;
        }
    }
    vec![data]
}

impl Decoder {
    pub fn new() -> Self {
        Decoder {
            refs: Default::default(),
            contexts: [FrameContext::default(), FrameContext::default(), FrameContext::default(), FrameContext::default()],
            lf: LoopFilterParams { ref_deltas: [1, 0, -1, -1], ..Default::default() },
            seg: Segmentation::default(),
            prev_seg: Vec::new(),
            last_color: ColorConfig { bit_depth: 8, subsampling_x: true, subsampling_y: true, ..Default::default() },
            last_size: (0, 0),
            last_show_frame: false,
            last_intra_only: false,
            last_was_key: false,
            prev_frame: None,
            color: ColorConfig::default(),
        }
    }

    /// Decodes one packet; returns the frames to show.
    pub fn decode(&mut self, packet: &[u8]) -> Result<Vec<Arc<FrameBuf>>, String> {
        let mut shown = Vec::new();
        for f in split_superframe(packet) {
            if let Some(fr) = self.decode_frame(f)? {
                shown.push(fr);
            }
        }
        Ok(shown)
    }

    fn decode_frame(&mut self, data: &[u8]) -> Result<Option<Arc<FrameBuf>>, String> {
        let mut ref_sizes = [(0u32, 0u32); 8];
        for (i, r) in self.refs.iter().enumerate() {
            if let Some(r) = r {
                ref_sizes[i] = (r.width, r.height);
            }
        }
        let mut st = HeaderState { lf: &mut self.lf, seg: &mut self.seg, last_color: self.last_color, ref_sizes };
        let mut reset_maps = false;
        let h = header::parse(data, &mut st, |_h, lf, seg| {
            // setup_past_independence: segmentation features and loop
            // filter deltas back to their defaults.
            seg.feature_enabled = [[false; 4]; 8];
            seg.feature_data = [[0; 4]; 8];
            seg.abs_delta = false;
            lf.ref_deltas = [1, 0, -1, -1];
            lf.mode_deltas = [0, 0];
            reset_maps = true;
        })?;
        if h.show_existing_frame {
            let f = self.refs[h.existing_frame_idx as usize].clone().ok_or("VP9: show_existing_frame of an empty slot")?;
            return Ok(Some(f));
        }
        if h.color.bit_depth != 8 {
            return Err(format!("VP9: {}-bit video is not supported yet", h.color.bit_depth));
        }
        let intra = h.frame_is_intra();
        if !intra {
            for i in 0..3 {
                if self.refs[h.ref_frame_idx[i] as usize].is_none() {
                    return Err("VP9: missing reference frame".into());
                }
            }
        }
        // Frame contexts.
        let mut ctx_idx = h.frame_context_idx as usize;
        if intra || h.error_resilient {
            let d = FrameContext::default();
            if h.key_frame || h.error_resilient || h.reset_frame_context == 3 {
                self.contexts = [d.clone(), d.clone(), d.clone(), d];
            } else if h.reset_frame_context == 2 {
                self.contexts[ctx_idx] = d;
            }
            ctx_idx = 0;
        }
        let mi_cols = (h.width as usize).div_ceil(8);
        let mi_rows = (h.height as usize).div_ceil(8);
        if reset_maps || self.prev_seg.len() != mi_cols * mi_rows {
            self.prev_seg = vec![0; mi_cols * mi_rows];
        }
        let mut fc = self.contexts[ctx_idx].clone();
        let start = h.uncompressed_size;
        let end = start + h.header_size;
        if end > data.len() {
            return Err("VP9: truncated compressed header".into());
        }
        let compound_allowed = !intra && (h.sign_bias[2] != h.sign_bias[1] || h.sign_bias[3] != h.sign_bias[1]);
        let ch = {
            let mut r = bits::BoolDecoder::new(&data[start..end]).map_err(|e| format!("VP9: {e}"))?;
            probs::read_compressed_header(&mut r, &mut fc, h.lossless, intra, h.interp_filter, h.allow_high_precision_mv, compound_allowed)
        };
        let ss = (h.color.subsampling_x, h.color.subsampling_y);
        let cur = FrameBuf::new(h.width, h.height, ss.0, ss.1);
        // References and their scale factors.
        let mut ref_arcs: [Option<Arc<FrameBuf>>; 3] = [None, None, None];
        if !intra {
            for i in 0..3 {
                ref_arcs[i] = self.refs[h.ref_frame_idx[i] as usize].clone();
            }
        }
        let mut refs: [Option<(&FrameBuf, Scale)>; 3] = [None, None, None];
        for i in 0..3 {
            if let Some(r) = ref_arcs[i].as_deref() {
                let sc = Scale::new(r.width, r.height, h.width, h.height).ok_or("VP9: reference frame has invalid dimensions")?;
                refs[i] = Some((r, sc));
            }
        }
        let use_prev = !h.error_resilient && self.last_size == (h.width, h.height) && !self.last_intra_only && self.last_show_frame;
        let prev = self.prev_frame.clone();
        let prev_mvs: Option<&[MvRef]> = if use_prev { prev.as_deref().map(|p| p.mvs.as_slice()) } else { None };
        let counting = !h.error_resilient && !h.frame_parallel;
        let seg = self.seg.clone();
        let mut fd = FrameDec::new(&h, &fc, &seg, cur, refs, prev_mvs, &self.prev_seg, ch.tx_mode, ch.reference_mode, counting);
        fd.decode_tiles(&data[end..])?;
        if self.lf.level > 0 {
            let lf = loopfilter::LoopFilter::new(&self.lf, &seg, ss);
            lf.filter_frame(&mut fd.cur, &fd.mi);
        }
        let counts = fd.counts.take();
        let FrameDec { cur, seg_map, .. } = fd;
        // Backward adaptation.
        if let Some(counts) = counts {
            let pre = &self.contexts[ctx_idx];
            probs::adapt_coef_probs(&mut fc, pre, &counts, intra, self.last_was_key);
            if !intra {
                probs::adapt_mode_probs(&mut fc, pre, &counts, h.interp_filter, ch.tx_mode, h.allow_high_precision_mv);
            }
        }
        if h.refresh_frame_context {
            self.contexts[ctx_idx] = fc;
        }
        if seg.enabled {
            self.prev_seg = seg_map;
        }
        let frame = Arc::new(cur);
        for i in 0..8 {
            if h.refresh_frame_flags & (1 << i) != 0 {
                self.refs[i] = Some(frame.clone());
            }
        }
        self.last_color = h.color;
        self.color = h.color;
        self.last_size = (h.width, h.height);
        self.last_show_frame = h.show_frame;
        self.last_intra_only = h.intra_only;
        self.last_was_key = h.key_frame;
        self.prev_frame = Some(frame.clone());
        Ok(h.show_frame.then_some(frame))
    }
}

impl FrameBuf {
    /// RGBA pixels of the visible frame (BT.601 or BT.709, studio or full range).
    pub fn to_rgba(&self, color: &ColorConfig) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut out = vec![0u8; w * h * 4];
        let bt709 = color.color_space == 2;
        let full = color.color_range;
        let (y, u, v) = (&self.planes[0], &self.planes[1], &self.planes[2]);
        let (sx, sy) = (self.ss_x as usize, self.ss_y as usize);
        if color.color_space == 7 {
            // RGB: the planes hold G, B and R.
            for row in 0..h {
                for col in 0..w {
                    let o = (row * w + col) * 4;
                    out[o] = v.data[(row >> sy) * v.stride + (col >> sx)];
                    out[o + 1] = y.data[row * y.stride + col];
                    out[o + 2] = u.data[(row >> sy) * u.stride + (col >> sx)];
                    out[o + 3] = 255;
                }
            }
            return out;
        }
        for row in 0..h {
            for col in 0..w {
                let yy = y.data[row * y.stride + col] as i32;
                let d = u.data[(row >> sy) * u.stride + (col >> sx)] as i32 - 128;
                let e = v.data[(row >> sy) * v.stride + (col >> sx)] as i32 - 128;
                let c = if full { yy * 256 } else { (yy - 16) * 298 };
                let (r, g, b) = match (bt709, full) {
                    (true, true) => (c + 403 * e, c - 48 * d - 120 * e, c + 475 * d),
                    (true, false) => (c + 459 * e, c - 55 * d - 136 * e, c + 541 * d),
                    (false, true) => (c + 359 * e, c - 88 * d - 183 * e, c + 454 * d),
                    (false, false) => (c + 409 * e, c - 100 * d - 208 * e, c + 516 * d),
                };
                let o = (row * w + col) * 4;
                out[o] = ((r + 128) >> 8).clamp(0, 255) as u8;
                out[o + 1] = ((g + 128) >> 8).clamp(0, 255) as u8;
                out[o + 2] = ((b + 128) >> 8).clamp(0, 255) as u8;
                out[o + 3] = 255;
            }
        }
        out
    }
}

/// The uncompressed header of a frame without decoding it.
pub fn peek_header(data: &[u8]) -> Result<FrameHeader, String> {
    let mut lf = LoopFilterParams::default();
    let mut seg = Segmentation::default();
    let mut st = HeaderState { lf: &mut lf, seg: &mut seg, last_color: ColorConfig::default(), ref_sizes: [(0, 0); 8] };
    header::parse(data, &mut st, |_, _, _| {})
}
