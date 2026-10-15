//! Probability contexts, symbol counts, the compressed header's
//! probability updates and the backward adaptation after each frame.

use super::bits::BoolDecoder;
use super::tables::*;

pub type CoefProbs = [[[[[[u8; 3]; 6]; 6]; 2]; 2]; 4];

#[derive(Clone)]
pub struct MvComponentProbs {
    pub sign: u8,
    pub classes: [u8; 10],
    pub class0: [u8; 1],
    pub bits: [u8; 10],
    pub class0_fp: [[u8; 3]; 2],
    pub fp: [u8; 3],
    pub class0_hp: u8,
    pub hp: u8,
}

/// One frame context (`FRAME_CONTEXT`).
#[derive(Clone)]
pub struct FrameContext {
    pub y_mode: [[u8; 9]; 4],
    pub uv_mode: [[u8; 9]; 10],
    pub partition: [[u8; 3]; 16],
    pub coef: CoefProbs,
    pub switchable_interp: [[u8; 2]; 4],
    pub inter_mode: [[u8; 3]; 7],
    pub intra_inter: [u8; 4],
    pub comp_inter: [u8; 5],
    pub single_ref: [[u8; 2]; 5],
    pub comp_ref: [u8; 5],
    pub tx8: [[u8; 1]; 2],
    pub tx16: [[u8; 2]; 2],
    pub tx32: [[u8; 3]; 2],
    pub skip: [u8; 3],
    pub mv_joints: [u8; 3],
    pub mv: [MvComponentProbs; 2],
}

impl Default for FrameContext {
    fn default() -> Self {
        let comp = |i: usize| MvComponentProbs {
            sign: DEFAULT_MV_SIGN[i],
            classes: DEFAULT_MV_CLASSES[i],
            class0: [DEFAULT_MV_CLASS0[i]],
            bits: DEFAULT_MV_BITS[i],
            class0_fp: DEFAULT_MV_CLASS0_FP[i],
            fp: DEFAULT_MV_FP[i],
            class0_hp: DEFAULT_MV_CLASS0_HP[i],
            hp: DEFAULT_MV_HP[i],
        };
        FrameContext {
            y_mode: DEFAULT_Y_MODE_PROBS,
            uv_mode: DEFAULT_UV_MODE_PROBS,
            partition: DEFAULT_PARTITION_PROBS,
            coef: default_coef_probs(),
            switchable_interp: DEFAULT_SWITCHABLE_INTERP_PROBS,
            inter_mode: DEFAULT_INTER_MODE_PROBS,
            intra_inter: DEFAULT_INTRA_INTER_PROBS,
            comp_inter: DEFAULT_COMP_INTER_PROBS,
            single_ref: DEFAULT_SINGLE_REF_PROBS,
            comp_ref: DEFAULT_COMP_REF_PROBS,
            tx8: DEFAULT_TX8_PROBS,
            tx16: DEFAULT_TX16_PROBS,
            tx32: DEFAULT_TX32_PROBS,
            skip: DEFAULT_SKIP_PROBS,
            mv_joints: DEFAULT_MV_JOINT_PROBS,
            mv: [comp(0), comp(1)],
        }
    }
}

#[derive(Clone, Default)]
pub struct MvComponentCounts {
    pub sign: [u32; 2],
    pub classes: [u32; 11],
    pub class0: [u32; 2],
    pub bits: [[u32; 2]; 10],
    pub class0_fp: [[u32; 4]; 2],
    pub fp: [u32; 4],
    pub class0_hp: [u32; 2],
    pub hp: [u32; 2],
}

/// Symbol counts of one frame (`FRAME_COUNTS`).
#[derive(Clone)]
pub struct Counts {
    pub y_mode: [[u32; 10]; 4],
    pub uv_mode: [[u32; 10]; 10],
    pub partition: [[u32; 4]; 16],
    pub coef: [[[[[[u32; 4]; 6]; 6]; 2]; 2]; 4],
    pub eob_branch: [[[[[u32; 6]; 6]; 2]; 2]; 4],
    pub switchable_interp: [[u32; 3]; 4],
    pub inter_mode: [[u32; 4]; 7],
    pub intra_inter: [[u32; 2]; 4],
    pub comp_inter: [[u32; 2]; 5],
    pub single_ref: [[[u32; 2]; 2]; 5],
    pub comp_ref: [[u32; 2]; 5],
    pub tx32: [[u32; 4]; 2],
    pub tx16: [[u32; 3]; 2],
    pub tx8: [[u32; 2]; 2],
    pub skip: [[u32; 2]; 3],
    pub mv_joints: [u32; 4],
    pub mv: [MvComponentCounts; 2],
}

impl Default for Counts {
    fn default() -> Self {
        Counts {
            y_mode: [[0; 10]; 4],
            uv_mode: [[0; 10]; 10],
            partition: [[0; 4]; 16],
            coef: [[[[[[0; 4]; 6]; 6]; 2]; 2]; 4],
            eob_branch: [[[[[0; 6]; 6]; 2]; 2]; 4],
            switchable_interp: [[0; 3]; 4],
            inter_mode: [[0; 4]; 7],
            intra_inter: [[0; 2]; 4],
            comp_inter: [[0; 2]; 5],
            single_ref: [[[0; 2]; 2]; 5],
            comp_ref: [[0; 2]; 5],
            tx32: [[0; 4]; 2],
            tx16: [[0; 3]; 2],
            tx8: [[0; 2]; 2],
            skip: [[0; 2]; 3],
            mv_joints: [0; 4],
            mv: [MvComponentCounts::default(), MvComponentCounts::default()],
        }
    }
}

// ---------------------------------------------------------------------
// Compressed header probability updates
// ---------------------------------------------------------------------

fn inv_recenter_nonneg(v: i32, m: i32) -> i32 {
    if v > 2 * m {
        v
    } else if v & 1 != 0 {
        m - ((v + 1) >> 1)
    } else {
        m + (v >> 1)
    }
}

fn decode_uniform(r: &mut BoolDecoder) -> i32 {
    let m = (1 << 8) - 191;
    let v = r.literal(7) as i32;
    if v < m { v } else { (v << 1) - m + r.bit() as i32 }
}

fn decode_term_subexp(r: &mut BoolDecoder) -> i32 {
    if r.bit() == 0 {
        return r.literal(4) as i32;
    }
    if r.bit() == 0 {
        return r.literal(4) as i32 + 16;
    }
    if r.bit() == 0 {
        return r.literal(5) as i32 + 32;
    }
    decode_uniform(r) + 64
}

fn inv_remap_prob(v: i32, m: u8) -> u8 {
    let v = crate::dprob::INV_MAP_TABLE[v.clamp(0, 254) as usize] as i32;
    let m = m as i32 - 1;
    if (m << 1) <= 255 { (1 + inv_recenter_nonneg(v, m)) as u8 } else { (255 - inv_recenter_nonneg(v, 255 - 1 - m)) as u8 }
}

/// `diff_update_prob`.
pub fn diff_update(r: &mut BoolDecoder, p: &mut u8) {
    if r.read(252) {
        let d = decode_term_subexp(r);
        *p = inv_remap_prob(d, *p);
    }
}

/// `update_mv_prob`.
fn update_mv(r: &mut BoolDecoder, p: &mut u8) {
    if r.read(252) {
        *p = ((r.literal(7) << 1) | 1) as u8;
    }
}

/// What the compressed header declares besides probabilities.
pub struct CompressedHeader {
    pub tx_mode: u8,
    pub reference_mode: u8,
}

pub const SINGLE_REFERENCE: u8 = 0;
pub const COMPOUND_REFERENCE: u8 = 1;
pub const REFERENCE_MODE_SELECT: u8 = 2;

/// Reads the compressed header into `fc`.
pub fn read_compressed_header(
    r: &mut BoolDecoder,
    fc: &mut FrameContext,
    lossless: bool,
    intra: bool,
    interp_filter: u8,
    allow_hp: bool,
    compound_allowed: bool,
) -> CompressedHeader {
    // tx_mode
    let tx_mode = if lossless {
        0
    } else {
        let mut t = r.literal(2) as u8;
        if t == 3 {
            t += r.literal(1) as u8;
        }
        t
    };
    if tx_mode == TX_MODE_SELECT {
        for i in 0..2 {
            diff_update(r, &mut fc.tx8[i][0]);
        }
        for i in 0..2 {
            for j in 0..2 {
                diff_update(r, &mut fc.tx16[i][j]);
            }
        }
        for i in 0..2 {
            for j in 0..3 {
                diff_update(r, &mut fc.tx32[i][j]);
            }
        }
    }
    // coefficient probabilities
    let max_tx = TX_MODE_TO_BIGGEST[tx_mode as usize];
    for t in 0..=max_tx as usize {
        if r.literal(1) != 0 {
            for i in 0..2 {
                for j in 0..2 {
                    for k in 0..6 {
                        let ctxs = if k == 0 { 3 } else { 6 };
                        for l in 0..ctxs {
                            for m in 0..3 {
                                diff_update(r, &mut fc.coef[t][i][j][k][l][m]);
                            }
                        }
                    }
                }
            }
        }
    }
    for i in 0..3 {
        diff_update(r, &mut fc.skip[i]);
    }
    let mut reference_mode = SINGLE_REFERENCE;
    if !intra {
        for i in 0..7 {
            for j in 0..3 {
                diff_update(r, &mut fc.inter_mode[i][j]);
            }
        }
        if interp_filter == SWITCHABLE {
            for j in 0..4 {
                for i in 0..2 {
                    diff_update(r, &mut fc.switchable_interp[j][i]);
                }
            }
        }
        for i in 0..4 {
            diff_update(r, &mut fc.intra_inter[i]);
        }
        if compound_allowed {
            if r.literal(1) != 0 {
                reference_mode = if r.literal(1) != 0 { REFERENCE_MODE_SELECT } else { COMPOUND_REFERENCE };
            }
        }
        if reference_mode == REFERENCE_MODE_SELECT {
            for i in 0..5 {
                diff_update(r, &mut fc.comp_inter[i]);
            }
        }
        if reference_mode != COMPOUND_REFERENCE {
            for i in 0..5 {
                diff_update(r, &mut fc.single_ref[i][0]);
                diff_update(r, &mut fc.single_ref[i][1]);
            }
        }
        if reference_mode != SINGLE_REFERENCE {
            for i in 0..5 {
                diff_update(r, &mut fc.comp_ref[i]);
            }
        }
        for i in 0..4 {
            for j in 0..9 {
                diff_update(r, &mut fc.y_mode[i][j]);
            }
        }
        for i in 0..16 {
            for j in 0..3 {
                diff_update(r, &mut fc.partition[i][j]);
            }
        }
        // motion vector probabilities
        for j in 0..3 {
            update_mv(r, &mut fc.mv_joints[j]);
        }
        for i in 0..2 {
            let c = &mut fc.mv[i];
            update_mv(r, &mut c.sign);
            for j in 0..10 {
                update_mv(r, &mut c.classes[j]);
            }
            update_mv(r, &mut c.class0[0]);
            for j in 0..10 {
                update_mv(r, &mut c.bits[j]);
            }
        }
        for i in 0..2 {
            let c = &mut fc.mv[i];
            for j in 0..2 {
                for k in 0..3 {
                    update_mv(r, &mut c.class0_fp[j][k]);
                }
            }
            for k in 0..3 {
                update_mv(r, &mut c.fp[k]);
            }
        }
        if allow_hp {
            for i in 0..2 {
                let c = &mut fc.mv[i];
                update_mv(r, &mut c.class0_hp);
                update_mv(r, &mut c.hp);
            }
        }
    }
    CompressedHeader { tx_mode, reference_mode }
}

// ---------------------------------------------------------------------
// Backward adaptation
// ---------------------------------------------------------------------

fn get_prob(num: u32, den: u32) -> u8 {
    if den == 0 {
        return 128;
    }
    let p = ((num as u64 * 256 + (den as u64 >> 1)) / den as u64) as i64;
    p.clamp(1, 255) as u8
}

fn weighted_prob(a: u8, b: u8, factor: u32) -> u8 {
    ((a as u32 * (256 - factor) + b as u32 * factor + 128) >> 8) as u8
}

fn merge_probs(pre: u8, ct: [u32; 2], count_sat: u32, max_update: u32) -> u8 {
    let prob = get_prob(ct[0], ct[0] + ct[1]);
    let count = (ct[0] + ct[1]).min(count_sat);
    let factor = max_update * count / count_sat;
    weighted_prob(pre, prob, factor)
}

fn mode_mv_merge(pre: u8, ct: [u32; 2]) -> u8 {
    let den = ct[0] + ct[1];
    if den == 0 {
        return pre;
    }
    let count = den.min(20);
    let factor = COUNT_TO_UPDATE_FACTOR[count as usize];
    weighted_prob(pre, get_prob(ct[0], den), factor)
}

fn tree_merge_impl(i: usize, tree: &[i8], pre: &[u8], counts: &[u32], out: &mut [u8]) -> u32 {
    let l = tree[i];
    let left = if l <= 0 { counts[(-l) as usize] } else { tree_merge_impl(l as usize, tree, pre, counts, out) };
    let r = tree[i + 1];
    let right = if r <= 0 { counts[(-r) as usize] } else { tree_merge_impl(r as usize, tree, pre, counts, out) };
    out[i >> 1] = mode_mv_merge(pre[i >> 1], [left, right]);
    left + right
}

fn tree_merge(tree: &[i8], pre: &[u8], counts: &[u32], out: &mut [u8]) {
    tree_merge_impl(0, tree, pre, counts, out);
}

/// Coefficient adaptation (`vp9_adapt_coef_probs`).
pub fn adapt_coef_probs(fc: &mut FrameContext, pre: &FrameContext, counts: &Counts, intra: bool, last_was_key: bool) {
    let (count_sat, update) = if intra {
        (24, 112)
    } else if last_was_key {
        (24, 128)
    } else {
        (24, 112)
    };
    for t in 0..4 {
        for i in 0..2 {
            for j in 0..2 {
                for k in 0..6 {
                    let ctxs = if k == 0 { 3 } else { 6 };
                    for l in 0..ctxs {
                        let c = &counts.coef[t][i][j][k][l];
                        let (n0, n1, n2, neob) = (c[0], c[1], c[2], c[3]);
                        let eob = counts.eob_branch[t][i][j][k][l];
                        let branch = [[neob, eob.saturating_sub(neob)], [n0, n1 + n2], [n1, n2]];
                        for m in 0..3 {
                            fc.coef[t][i][j][k][l][m] = merge_probs(pre.coef[t][i][j][k][l][m], branch[m], count_sat, update);
                        }
                    }
                }
            }
        }
    }
}

/// Mode and motion vector adaptation (`vp9_adapt_mode_probs`,
/// `vp9_adapt_mv_probs`).
pub fn adapt_mode_probs(fc: &mut FrameContext, pre: &FrameContext, counts: &Counts, interp_filter: u8, tx_mode: u8, allow_hp: bool) {
    for i in 0..4 {
        fc.intra_inter[i] = mode_mv_merge(pre.intra_inter[i], counts.intra_inter[i]);
    }
    for i in 0..5 {
        fc.comp_inter[i] = mode_mv_merge(pre.comp_inter[i], counts.comp_inter[i]);
        fc.comp_ref[i] = mode_mv_merge(pre.comp_ref[i], counts.comp_ref[i]);
        for j in 0..2 {
            fc.single_ref[i][j] = mode_mv_merge(pre.single_ref[i][j], counts.single_ref[i][j]);
        }
    }
    for i in 0..7 {
        tree_merge(&INTER_MODE_TREE, &pre.inter_mode[i], &counts.inter_mode[i], &mut fc.inter_mode[i]);
    }
    for i in 0..4 {
        tree_merge(&INTRA_MODE_TREE, &pre.y_mode[i], &counts.y_mode[i], &mut fc.y_mode[i]);
    }
    for i in 0..10 {
        tree_merge(&INTRA_MODE_TREE, &pre.uv_mode[i], &counts.uv_mode[i], &mut fc.uv_mode[i]);
    }
    for i in 0..16 {
        tree_merge(&PARTITION_TREE, &pre.partition[i], &counts.partition[i], &mut fc.partition[i]);
    }
    if interp_filter == SWITCHABLE {
        for i in 0..4 {
            tree_merge(&SWITCHABLE_INTERP_TREE, &pre.switchable_interp[i], &counts.switchable_interp[i], &mut fc.switchable_interp[i]);
        }
    }
    if tx_mode == TX_MODE_SELECT {
        for i in 0..2 {
            let c8 = counts.tx8[i];
            fc.tx8[i][0] = mode_mv_merge(pre.tx8[i][0], [c8[0], c8[1]]);
            let c16 = counts.tx16[i];
            fc.tx16[i][0] = mode_mv_merge(pre.tx16[i][0], [c16[0], c16[1] + c16[2]]);
            fc.tx16[i][1] = mode_mv_merge(pre.tx16[i][1], [c16[1], c16[2]]);
            let c32 = counts.tx32[i];
            fc.tx32[i][0] = mode_mv_merge(pre.tx32[i][0], [c32[0], c32[1] + c32[2] + c32[3]]);
            fc.tx32[i][1] = mode_mv_merge(pre.tx32[i][1], [c32[1], c32[2] + c32[3]]);
            fc.tx32[i][2] = mode_mv_merge(pre.tx32[i][2], [c32[2], c32[3]]);
        }
    }
    for i in 0..3 {
        fc.skip[i] = mode_mv_merge(pre.skip[i], counts.skip[i]);
    }
    // motion vectors
    tree_merge(&MV_JOINT_TREE, &pre.mv_joints, &counts.mv_joints, &mut fc.mv_joints);
    for i in 0..2 {
        let (p, c) = (&pre.mv[i], &counts.mv[i]);
        let f = &mut fc.mv[i];
        f.sign = mode_mv_merge(p.sign, c.sign);
        tree_merge(&MV_CLASS_TREE, &p.classes, &c.classes, &mut f.classes);
        tree_merge(&MV_CLASS0_TREE, &p.class0, &c.class0, &mut f.class0);
        for j in 0..10 {
            f.bits[j] = mode_mv_merge(p.bits[j], c.bits[j]);
        }
        for j in 0..2 {
            tree_merge(&MV_FP_TREE, &p.class0_fp[j], &c.class0_fp[j], &mut f.class0_fp[j]);
        }
        tree_merge(&MV_FP_TREE, &p.fp, &c.fp, &mut f.fp);
        if allow_hp {
            f.class0_hp = mode_mv_merge(p.class0_hp, c.class0_hp);
            f.hp = mode_mv_merge(p.hp, c.hp);
        }
    }
}
