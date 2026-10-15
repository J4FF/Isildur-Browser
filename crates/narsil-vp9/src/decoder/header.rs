//! The uncompressed frame header (VP9 specification §6.2).

use super::bits::BitReader;
use super::tables::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColorConfig {
    pub bit_depth: u8,
    pub color_space: u8,
    pub color_range: bool,
    pub subsampling_x: bool,
    pub subsampling_y: bool,
}

#[derive(Clone, Debug, Default)]
pub struct LoopFilterParams {
    pub level: u8,
    pub sharpness: u8,
    pub delta_enabled: bool,
    pub ref_deltas: [i8; 4],
    pub mode_deltas: [i8; 2],
}

#[derive(Clone, Debug, Default)]
pub struct Segmentation {
    pub enabled: bool,
    pub update_map: bool,
    pub temporal_update: bool,
    pub abs_delta: bool,
    pub tree_probs: [u8; 7],
    pub pred_probs: [u8; 3],
    pub feature_enabled: [[bool; 4]; 8],
    pub feature_data: [[i16; 4]; 8],
}

impl Segmentation {
    pub fn active(&self, seg: u8, feature: usize) -> bool {
        self.enabled && self.feature_enabled[seg as usize][feature]
    }
    pub fn data(&self, seg: u8, feature: usize) -> i16 {
        self.feature_data[seg as usize][feature]
    }
}

#[derive(Clone, Debug, Default)]
pub struct FrameHeader {
    pub profile: u8,
    pub show_existing_frame: bool,
    pub existing_frame_idx: u8,
    pub key_frame: bool,
    pub show_frame: bool,
    pub error_resilient: bool,
    pub intra_only: bool,
    pub reset_frame_context: u8,
    pub color: ColorConfig,
    pub width: u32,
    pub height: u32,
    pub render_width: u32,
    pub render_height: u32,
    pub refresh_frame_flags: u8,
    pub ref_frame_idx: [u8; 3],
    /// `ref_frame_sign_bias[LAST..=ALTREF]` at indices 1..=3.
    pub sign_bias: [bool; 4],
    /// For inter frames: the size comes from reference `i` (found_ref).
    pub size_from_ref: Option<usize>,
    pub allow_high_precision_mv: bool,
    pub interp_filter: u8,
    pub refresh_frame_context: bool,
    pub frame_parallel: bool,
    pub frame_context_idx: u8,
    pub base_q_idx: u8,
    pub delta_q_y_dc: i8,
    pub delta_q_uv_dc: i8,
    pub delta_q_uv_ac: i8,
    pub lossless: bool,
    pub tile_cols_log2: u8,
    pub tile_rows_log2: u8,
    pub header_size: usize,
    /// Bytes of the uncompressed header.
    pub uncompressed_size: usize,
}

impl FrameHeader {
    pub fn frame_is_intra(&self) -> bool {
        self.key_frame || self.intra_only
    }
}

fn read_delta_q(r: &mut BitReader) -> i8 {
    if r.bit() != 0 { r.signed(4) as i8 } else { 0 }
}

fn color_config(r: &mut BitReader, profile: u8) -> Result<ColorConfig, String> {
    let mut c = ColorConfig { bit_depth: 8, ..Default::default() };
    if profile >= 2 {
        c.bit_depth = if r.bit() != 0 { 12 } else { 10 };
    }
    c.color_space = r.bits(3) as u8;
    if c.color_space != 7 {
        c.color_range = r.bit() != 0;
        if profile == 1 || profile == 3 {
            c.subsampling_x = r.bit() != 0;
            c.subsampling_y = r.bit() != 0;
            if r.bit() != 0 {
                return Err("VP9: reserved bit set".into());
            }
        } else {
            c.subsampling_x = true;
            c.subsampling_y = true;
        }
    } else {
        c.color_range = true;
        if profile == 1 || profile == 3 {
            if r.bit() != 0 {
                return Err("VP9: reserved bit set".into());
            }
        } else {
            return Err("VP9: RGB is not allowed in profile 0".into());
        }
    }
    Ok(c)
}

/// Persistent state the header parser updates (it carries over between
/// frames: loop filter deltas and segmentation).
pub struct HeaderState<'a> {
    pub lf: &'a mut LoopFilterParams,
    pub seg: &'a mut Segmentation,
    pub last_color: ColorConfig,
    /// (width, height) of each reference slot.
    pub ref_sizes: [(u32, u32); 8],
}

/// Parses the uncompressed header. `reset` is called (with the header
/// read so far) when the frame needs `setup_past_independence` before the
/// loop filter and segmentation fields are read.
pub fn parse(data: &[u8], st: &mut HeaderState, mut reset: impl FnMut(&FrameHeader, &mut LoopFilterParams, &mut Segmentation)) -> Result<FrameHeader, String> {
    let mut r = BitReader::new(data);
    let mut h = FrameHeader::default();
    if r.bits(2) != 2 {
        return Err("VP9: invalid frame marker".into());
    }
    let lo = r.bit();
    let hi = r.bit();
    h.profile = ((hi << 1) | lo) as u8;
    if h.profile == 3 && r.bit() != 0 {
        return Err("VP9: reserved bit set".into());
    }
    h.show_existing_frame = r.bit() != 0;
    if h.show_existing_frame {
        h.existing_frame_idx = r.bits(3) as u8;
        h.uncompressed_size = r.byte_pos();
        return Ok(h);
    }
    h.key_frame = r.bit() == 0;
    h.show_frame = r.bit() != 0;
    h.error_resilient = r.bit() != 0;
    let sync = |r: &mut BitReader| -> Result<(), String> {
        if r.bits(8) != 0x49 || r.bits(8) != 0x83 || r.bits(8) != 0x42 {
            return Err("VP9: invalid frame sync code".into());
        }
        Ok(())
    };
    let frame_size = |r: &mut BitReader, h: &mut FrameHeader| {
        h.width = r.bits(16) + 1;
        h.height = r.bits(16) + 1;
    };
    let render_size = |r: &mut BitReader, h: &mut FrameHeader| {
        if r.bit() != 0 {
            h.render_width = r.bits(16) + 1;
            h.render_height = r.bits(16) + 1;
        } else {
            h.render_width = h.width;
            h.render_height = h.height;
        }
    };
    if h.key_frame {
        sync(&mut r)?;
        h.color = color_config(&mut r, h.profile)?;
        frame_size(&mut r, &mut h);
        render_size(&mut r, &mut h);
        h.refresh_frame_flags = 0xff;
    } else {
        h.intra_only = if h.show_frame { false } else { r.bit() != 0 };
        h.reset_frame_context = if h.error_resilient { 0 } else { r.bits(2) as u8 };
        if h.intra_only {
            sync(&mut r)?;
            if h.profile > 0 {
                h.color = color_config(&mut r, h.profile)?;
            } else {
                h.color = ColorConfig { bit_depth: 8, color_space: 1, color_range: false, subsampling_x: true, subsampling_y: true };
            }
            h.refresh_frame_flags = r.bits(8) as u8;
            frame_size(&mut r, &mut h);
            render_size(&mut r, &mut h);
        } else {
            h.color = st.last_color;
            h.refresh_frame_flags = r.bits(8) as u8;
            for i in 0..3 {
                h.ref_frame_idx[i] = r.bits(3) as u8;
                h.sign_bias[1 + i] = r.bit() != 0;
            }
            let mut found = None;
            for i in 0..3 {
                if r.bit() != 0 {
                    found = Some(i);
                    break;
                }
            }
            match found {
                Some(i) => {
                    let (w, hh) = st.ref_sizes[h.ref_frame_idx[i] as usize];
                    h.width = w;
                    h.height = hh;
                    h.size_from_ref = Some(i);
                }
                None => frame_size(&mut r, &mut h),
            }
            render_size(&mut r, &mut h);
            h.allow_high_precision_mv = r.bit() != 0;
            h.interp_filter = if r.bit() != 0 { SWITCHABLE } else { [EIGHTTAP_SMOOTH, EIGHTTAP, EIGHTTAP_SHARP, BILINEAR][r.bits(2) as usize] };
        }
    }
    if h.width == 0 || h.height == 0 {
        return Err("VP9: zero frame size".into());
    }
    if !h.error_resilient {
        h.refresh_frame_context = r.bit() != 0;
        h.frame_parallel = r.bit() != 0;
    } else {
        h.refresh_frame_context = false;
        h.frame_parallel = true;
    }
    h.frame_context_idx = r.bits(2) as u8;
    if h.frame_is_intra() || h.error_resilient {
        reset(&h, st.lf, st.seg);
    }
    // Loop filter.
    let lf = &mut *st.lf;
    lf.level = r.bits(6) as u8;
    lf.sharpness = r.bits(3) as u8;
    lf.delta_enabled = r.bit() != 0;
    if lf.delta_enabled && r.bit() != 0 {
        for i in 0..4 {
            if r.bit() != 0 {
                lf.ref_deltas[i] = r.signed(6) as i8;
            }
        }
        for i in 0..2 {
            if r.bit() != 0 {
                lf.mode_deltas[i] = r.signed(6) as i8;
            }
        }
    }
    // Quantization.
    h.base_q_idx = r.bits(8) as u8;
    h.delta_q_y_dc = read_delta_q(&mut r);
    h.delta_q_uv_dc = read_delta_q(&mut r);
    h.delta_q_uv_ac = read_delta_q(&mut r);
    h.lossless = h.base_q_idx == 0 && h.delta_q_y_dc == 0 && h.delta_q_uv_dc == 0 && h.delta_q_uv_ac == 0;
    // Segmentation.
    let seg = &mut *st.seg;
    seg.update_map = false;
    seg.temporal_update = false;
    seg.enabled = r.bit() != 0;
    if seg.enabled {
        seg.update_map = r.bit() != 0;
        if seg.update_map {
            for i in 0..7 {
                seg.tree_probs[i] = if r.bit() != 0 { r.bits(8) as u8 } else { 255 };
            }
            seg.temporal_update = r.bit() != 0;
            for i in 0..3 {
                seg.pred_probs[i] = if seg.temporal_update && r.bit() != 0 { r.bits(8) as u8 } else { 255 };
            }
        }
        if r.bit() != 0 {
            seg.abs_delta = r.bit() != 0;
            for i in 0..8 {
                for j in 0..4 {
                    let mut v = 0i16;
                    let enabled = r.bit() != 0;
                    seg.feature_enabled[i][j] = enabled;
                    if enabled {
                        v = r.bits(SEG_FEATURE_BITS[j]) as i16;
                        if SEG_FEATURE_SIGNED[j] && r.bit() != 0 {
                            v = -v;
                        }
                    }
                    seg.feature_data[i][j] = v;
                }
            }
        }
    }
    // Tiles.
    let mi_cols = h.width.div_ceil(8);
    let sb64_cols = mi_cols.div_ceil(8);
    let mut min_log2 = 0;
    while (64u32 << min_log2) < sb64_cols {
        min_log2 += 1;
    }
    let mut max_log2 = 1;
    while (sb64_cols >> max_log2) >= 4 {
        max_log2 += 1;
    }
    max_log2 -= 1;
    h.tile_cols_log2 = min_log2 as u8;
    while (h.tile_cols_log2 as u32) < max_log2 {
        if r.bit() != 0 {
            h.tile_cols_log2 += 1;
        } else {
            break;
        }
    }
    h.tile_rows_log2 = r.bit() as u8;
    if h.tile_rows_log2 != 0 {
        h.tile_rows_log2 += r.bit() as u8;
    }
    h.header_size = r.bits(16) as usize;
    if r.overrun() {
        return Err("VP9: truncated frame header".into());
    }
    h.uncompressed_size = r.byte_pos();
    if h.header_size == 0 {
        return Err("VP9: invalid compressed header size".into());
    }
    Ok(h)
}
