//! Constant tables of the VP9 decoder (VP9 bitstream specification /
//! libvpx `vp9_entropymode.c`, `vp9_entropymv.c`, `vp9_scan.c`,
//! `vp9_filter.c`, `vp9_common_data.c`). The coefficient probabilities,
//! the Pareto table, the quantizer tables, the key frame mode
//! probabilities and the default scans come from the crate's existing,
//! machine-extracted tables (`dprob.rs`, `pareto.rs`, `dequant.rs`,
//! `tile.rs`, `scan.rs`); the neighbour tables are derived from the scans.

use std::sync::OnceLock;

// Block sizes.
pub const BLOCK_4X4: u8 = 0;
pub const BLOCK_4X8: u8 = 1;
pub const BLOCK_8X4: u8 = 2;
pub const BLOCK_8X8: u8 = 3;
pub const BLOCK_64X64: u8 = 12;
pub const BLOCK_INVALID: u8 = 13;

// Prediction modes.
pub const DC_PRED: u8 = 0;
pub const V_PRED: u8 = 1;
pub const H_PRED: u8 = 2;
pub const D45_PRED: u8 = 3;
pub const D135_PRED: u8 = 4;
pub const D117_PRED: u8 = 5;
pub const D153_PRED: u8 = 6;
pub const D207_PRED: u8 = 7;
pub const D63_PRED: u8 = 8;
pub const TM_PRED: u8 = 9;
pub const NEARESTMV: u8 = 10;
pub const NEARMV: u8 = 11;
pub const ZEROMV: u8 = 12;
pub const NEWMV: u8 = 13;

// Reference frames.
pub const INTRA_FRAME: i8 = 0;
pub const LAST_FRAME: i8 = 1;
pub const GOLDEN_FRAME: i8 = 2;
pub const ALTREF_FRAME: i8 = 3;
pub const NONE_FRAME: i8 = -1;

// Transform sizes and types.
pub const TX_4X4: u8 = 0;
pub const TX_8X8: u8 = 1;
pub const TX_16X16: u8 = 2;
pub const TX_32X32: u8 = 3;
pub const TX_MODE_SELECT: u8 = 4;
pub const DCT_DCT: u8 = 0;
pub const ADST_DCT: u8 = 1;
pub const DCT_ADST: u8 = 2;
pub const ADST_ADST: u8 = 3;

// Partitions.
pub const PARTITION_NONE: u8 = 0;
pub const PARTITION_HORZ: u8 = 1;
pub const PARTITION_VERT: u8 = 2;
pub const PARTITION_SPLIT: u8 = 3;

// Interpolation filters.
pub const EIGHTTAP: u8 = 0;
pub const EIGHTTAP_SMOOTH: u8 = 1;
pub const EIGHTTAP_SHARP: u8 = 2;
pub const BILINEAR: u8 = 3;
pub const SWITCHABLE: u8 = 4;

// Segmentation features.
pub const SEG_LVL_ALT_Q: usize = 0;
pub const SEG_LVL_ALT_LF: usize = 1;
pub const SEG_LVL_REF_FRAME: usize = 2;
pub const SEG_LVL_SKIP: usize = 3;
pub const SEG_FEATURE_BITS: [u32; 4] = [8, 6, 2, 0];
pub const SEG_FEATURE_SIGNED: [bool; 4] = [true, true, false, false];

pub const B_WIDTH_LOG2: [u8; 13] = [0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4];
pub const B_HEIGHT_LOG2: [u8; 13] = [0, 1, 0, 1, 2, 1, 2, 3, 2, 3, 4, 3, 4];
pub const NUM_4X4_W: [u8; 13] = [1, 1, 2, 2, 2, 4, 4, 4, 8, 8, 8, 16, 16];
pub const NUM_4X4_H: [u8; 13] = [1, 2, 1, 2, 4, 2, 4, 8, 4, 8, 16, 8, 16];
pub const NUM_8X8_W: [u8; 13] = [1, 1, 1, 1, 1, 2, 2, 2, 4, 4, 4, 8, 8];
pub const NUM_8X8_H: [u8; 13] = [1, 1, 1, 1, 2, 1, 2, 4, 2, 4, 8, 4, 8];
pub const MI_WIDTH_LOG2: [u8; 13] = [0, 0, 0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3];
pub const SIZE_GROUP: [u8; 13] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 3];
pub const MAX_TXSIZE: [u8; 13] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 3];
pub const TX_MODE_TO_BIGGEST: [u8; 5] = [0, 1, 2, 3, 3];

/// `subsize_lookup[partition][bsize]`.
pub const SUBSIZE: [[u8; 13]; 4] = crate::tile::SUBSIZE_LOOKUP;

/// `ss_size_lookup[bsize][ss_x][ss_y]`.
pub const SS_SIZE: [[[u8; 2]; 2]; 13] = [
    [[0, 13], [13, 13]],
    [[1, 0], [13, 13]],
    [[2, 13], [0, 13]],
    [[3, 2], [1, 0]],
    [[4, 3], [13, 1]],
    [[5, 13], [3, 2]],
    [[6, 5], [4, 3]],
    [[7, 6], [13, 4]],
    [[8, 13], [6, 5]],
    [[9, 8], [7, 6]],
    [[10, 9], [13, 7]],
    [[11, 13], [9, 8]],
    [[12, 11], [10, 9]],
];

/// `partition_context_lookup[bsize] = (above, left)`.
pub const PARTITION_CONTEXT: [(u8, u8); 13] = crate::tile::PARTITION_KONTEXT;

pub const INTRA_MODE_TO_TX_TYPE: [u8; 10] =
    [DCT_DCT, ADST_DCT, DCT_ADST, DCT_DCT, ADST_ADST, ADST_DCT, DCT_ADST, DCT_ADST, ADST_DCT, ADST_ADST];

// Trees (libvpx `vpx_tree_index` layout).
pub const INTRA_MODE_TREE: [i8; 18] = [
    -(DC_PRED as i8), 2, -(TM_PRED as i8), 4, -(V_PRED as i8), 6, 8, 12, -(H_PRED as i8), 10,
    -(D135_PRED as i8), -(D117_PRED as i8), -(D45_PRED as i8), 14, -(D63_PRED as i8), 16,
    -(D153_PRED as i8), -(D207_PRED as i8),
];
pub const SEGMENT_TREE: [i8; 14] = [2, 4, 6, 8, 10, 12, 0, -1, -2, -3, -4, -5, -6, -7];
pub const PARTITION_TREE: [i8; 6] = [0, 2, -1, 4, -2, -3];
/// Leaves are `mode - NEARESTMV`: ZEROMV (2), NEARESTMV (0), NEARMV (1), NEWMV (3).
pub const INTER_MODE_TREE: [i8; 6] = [-2, 2, 0, 4, -1, -3];
pub const SWITCHABLE_INTERP_TREE: [i8; 4] = [-(EIGHTTAP as i8), 2, -(EIGHTTAP_SMOOTH as i8), -(EIGHTTAP_SHARP as i8)];
pub const MV_JOINT_TREE: [i8; 6] = [0, 2, -1, 4, -2, -3];
pub const MV_CLASS_TREE: [i8; 20] = [0, 2, -1, 4, 6, 8, -2, -3, 10, 12, -4, -5, -6, 14, 16, 18, -7, -8, -9, -10];
pub const MV_CLASS0_TREE: [i8; 2] = [0, -1];
pub const MV_FP_TREE: [i8; 6] = [0, 2, -1, 4, -2, -3];

// Default probabilities (frame context).
pub const KF_Y_MODE_PROBS: [[[u8; 9]; 10]; 10] = crate::tile::KF_Y_MODUS;
pub const KF_UV_MODE_PROBS: [[u8; 9]; 10] = crate::tile::KF_UV_MODUS;
pub const KF_PARTITION_PROBS: [[u8; 3]; 16] = crate::tile::KF_PARTITION;

pub const DEFAULT_Y_MODE_PROBS: [[u8; 9]; 4] = [
    [65, 32, 18, 144, 162, 194, 41, 51, 98],
    [132, 68, 18, 165, 217, 196, 45, 40, 78],
    [173, 80, 19, 176, 240, 193, 64, 35, 46],
    [221, 135, 38, 194, 248, 121, 96, 85, 29],
];
pub const DEFAULT_UV_MODE_PROBS: [[u8; 9]; 10] = [
    [120, 7, 76, 176, 208, 126, 28, 54, 103],
    [48, 12, 154, 155, 139, 90, 34, 117, 119],
    [67, 6, 25, 204, 243, 158, 13, 21, 96],
    [97, 5, 44, 131, 176, 139, 48, 68, 97],
    [83, 5, 42, 156, 111, 152, 26, 49, 152],
    [80, 5, 58, 178, 74, 83, 33, 62, 145],
    [86, 5, 32, 154, 192, 168, 14, 22, 163],
    [85, 5, 32, 156, 216, 148, 19, 29, 73],
    [77, 7, 64, 116, 132, 122, 37, 126, 120],
    [101, 21, 107, 181, 192, 103, 19, 67, 125],
];
pub const DEFAULT_PARTITION_PROBS: [[u8; 3]; 16] = [
    [199, 122, 141],
    [147, 63, 159],
    [148, 133, 118],
    [121, 104, 114],
    [174, 73, 87],
    [92, 41, 83],
    [82, 99, 50],
    [53, 39, 39],
    [177, 58, 59],
    [68, 26, 63],
    [52, 79, 25],
    [17, 14, 12],
    [222, 34, 30],
    [72, 16, 44],
    [58, 32, 12],
    [10, 7, 6],
];
pub const DEFAULT_INTER_MODE_PROBS: [[u8; 3]; 7] =
    [[2, 173, 34], [7, 145, 85], [7, 166, 63], [7, 94, 66], [8, 64, 46], [17, 81, 31], [25, 29, 30]];
pub const DEFAULT_INTRA_INTER_PROBS: [u8; 4] = [9, 102, 187, 225];
pub const DEFAULT_COMP_INTER_PROBS: [u8; 5] = [239, 183, 119, 96, 41];
pub const DEFAULT_COMP_REF_PROBS: [u8; 5] = [50, 126, 123, 221, 226];
pub const DEFAULT_SINGLE_REF_PROBS: [[u8; 2]; 5] = [[33, 16], [77, 74], [142, 142], [172, 170], [238, 247]];
pub const DEFAULT_SWITCHABLE_INTERP_PROBS: [[u8; 2]; 4] = [[235, 162], [36, 255], [34, 3], [149, 144]];
pub const DEFAULT_SKIP_PROBS: [u8; 3] = [192, 128, 64];
pub const DEFAULT_TX8_PROBS: [[u8; 1]; 2] = [[100], [66]];
pub const DEFAULT_TX16_PROBS: [[u8; 2]; 2] = [[20, 152], [15, 101]];
pub const DEFAULT_TX32_PROBS: [[u8; 3]; 2] = [[3, 136, 37], [5, 52, 13]];

pub const DEFAULT_MV_JOINT_PROBS: [u8; 3] = [32, 64, 96];
/// Per component (vertical, horizontal): sign, classes[10], class0[1],
/// bits[10], class0_fp[2][3], fp[3], class0_hp, hp.
pub const DEFAULT_MV_SIGN: [u8; 2] = [128, 128];
pub const DEFAULT_MV_CLASSES: [[u8; 10]; 2] =
    [[224, 144, 192, 168, 192, 176, 192, 198, 198, 245], [216, 128, 176, 160, 176, 176, 192, 198, 198, 208]];
pub const DEFAULT_MV_CLASS0: [u8; 2] = [216, 208];
pub const DEFAULT_MV_BITS: [[u8; 10]; 2] =
    [[136, 140, 148, 160, 176, 192, 224, 234, 234, 240], [136, 140, 148, 160, 176, 192, 224, 234, 234, 240]];
pub const DEFAULT_MV_CLASS0_FP: [[[u8; 3]; 2]; 2] = [[[128, 128, 64], [96, 112, 64]], [[128, 128, 64], [96, 112, 64]]];
pub const DEFAULT_MV_FP: [[u8; 3]; 2] = [[64, 96, 64], [64, 96, 64]];
pub const DEFAULT_MV_CLASS0_HP: [u8; 2] = [160, 160];
pub const DEFAULT_MV_HP: [u8; 2] = [128, 128];

/// The coefficient probabilities per transform size, unpacked to
/// `[plane type][ref][band][context][node]` (band 0 has 3 contexts).
pub fn default_coef_probs() -> [[[[[[u8; 3]; 6]; 6]; 2]; 2]; 4] {
    let mut out = [[[[[[0u8; 3]; 6]; 6]; 2]; 2]; 4];
    let srcs: [&[u8; 396]; 4] = [
        &crate::dprob::DEFAULT_COEF_4X4,
        &crate::dprob::DEFAULT_COEF_8X8,
        &crate::dprob::DEFAULT_COEF_16X16,
        &crate::dprob::DEFAULT_COEF_32X32,
    ];
    for (t, src) in srcs.iter().enumerate() {
        let mut i = 0;
        for plane in 0..2 {
            for r in 0..2 {
                for band in 0..6 {
                    let ctxs = if band == 0 { 3 } else { 6 };
                    for ctx in 0..ctxs {
                        for node in 0..3 {
                            out[t][plane][r][band][ctx][node] = src[i];
                            i += 1;
                        }
                    }
                }
            }
        }
    }
    out
}

pub const PARETO8: [[u8; 8]; 255] = crate::pareto::PARETO8;
pub const DC_QLOOKUP: [i16; 256] = crate::dequant::DC_QLOOKUP;
pub const AC_QLOOKUP: [i16; 256] = crate::dequant::AC_QLOOKUP;
pub const COEFBAND_4X4: [u8; 16] = crate::coefband::COEFBAND_TRANS_4X4;
pub const COEFBAND_8X8PLUS: [u8; 1024] = crate::coefband::COEFBAND_TRANS_8X8PLUS;
pub const CAT1_PROB: [u8; 1] = [159];
pub const CAT2_PROB: [u8; 2] = [165, 145];
pub const CAT3_PROB: [u8; 3] = [173, 148, 140];
pub const CAT4_PROB: [u8; 4] = [176, 155, 140, 135];
pub const CAT5_PROB: [u8; 5] = [180, 157, 141, 134, 130];
pub const CAT6_PROB: [u8; 14] = [254, 254, 254, 252, 249, 243, 230, 196, 177, 153, 140, 133, 130, 129];

// Scans. The default scans are the crate's extracted tables; the column
// and row scans (used by ADST/DCT and DCT/ADST transforms) follow.
pub const COL_SCAN_4X4: [i16; 16] = [0, 4, 8, 1, 12, 5, 9, 2, 13, 6, 10, 3, 7, 14, 11, 15];
pub const ROW_SCAN_4X4: [i16; 16] = [0, 1, 4, 2, 5, 3, 6, 8, 9, 7, 12, 10, 13, 11, 14, 15];
#[rustfmt::skip]
pub const COL_SCAN_8X8: [i16; 64] = [
    0, 8, 16, 1, 24, 9, 32, 17, 2, 40, 25, 10, 33, 18, 48, 3, 26, 41, 11, 56, 19, 34, 4, 49, 27, 42, 12, 35, 20, 57, 50, 28,
    5, 43, 13, 36, 58, 51, 21, 44, 6, 29, 59, 37, 14, 52, 22, 7, 45, 60, 30, 15, 38, 53, 23, 46, 31, 61, 39, 54, 47, 62, 55, 63,
];
#[rustfmt::skip]
pub const ROW_SCAN_8X8: [i16; 64] = [
    0, 1, 2, 8, 9, 3, 16, 10, 4, 17, 11, 24, 5, 18, 25, 12, 19, 26, 32, 6, 13, 20, 33, 27, 7, 34, 40, 21, 28, 41, 14, 35,
    48, 42, 29, 36, 49, 22, 43, 15, 56, 37, 50, 44, 30, 57, 23, 51, 58, 45, 38, 52, 31, 59, 53, 46, 60, 39, 61, 47, 54, 55, 62, 63,
];
#[rustfmt::skip]
pub const COL_SCAN_16X16: [i16; 256] = [
    0, 16, 32, 48, 1, 64, 17, 80, 33, 96, 49, 2, 65, 112, 18, 81, 34, 128, 50, 97, 3, 66, 144, 19, 113, 35, 82, 160, 98, 51,
    129, 4, 67, 176, 20, 114, 145, 83, 36, 99, 130, 52, 192, 5, 161, 68, 115, 21, 146, 84, 208, 177, 37, 131, 100, 53, 162, 224, 69, 6,
    116, 193, 147, 85, 22, 240, 132, 38, 178, 101, 163, 54, 209, 117, 70, 7, 148, 194, 86, 179, 225, 23, 133, 39, 164, 8, 102, 210, 241, 55,
    195, 118, 149, 71, 180, 24, 87, 226, 134, 165, 211, 40, 103, 56, 72, 150, 196, 242, 119, 9, 181, 227, 88, 166, 25, 135, 41, 104, 212, 57,
    151, 197, 120, 73, 243, 182, 136, 167, 213, 89, 10, 228, 105, 152, 198, 26, 42, 121, 183, 244, 168, 58, 137, 229, 74, 214, 90, 153, 199, 184,
    11, 106, 245, 27, 122, 230, 169, 43, 215, 59, 200, 138, 185, 246, 75, 12, 91, 154, 216, 231, 107, 28, 44, 201, 123, 170, 60, 247, 232, 76,
    139, 13, 92, 217, 186, 248, 155, 108, 29, 124, 45, 202, 233, 171, 61, 14, 77, 140, 15, 249, 93, 30, 187, 156, 218, 46, 109, 125, 62, 172,
    78, 203, 31, 141, 234, 94, 47, 188, 63, 157, 110, 250, 219, 79, 126, 204, 173, 142, 95, 189, 111, 235, 158, 220, 251, 127, 174, 143, 205, 236,
    159, 190, 221, 252, 175, 206, 237, 191, 253, 222, 238, 207, 254, 223, 239, 255,
];
#[rustfmt::skip]
pub const ROW_SCAN_16X16: [i16; 256] = [
    0, 1, 2, 16, 3, 17, 4, 18, 32, 5, 33, 19, 6, 34, 48, 20, 49, 7, 35, 21, 50, 64, 8, 36, 65, 22, 51, 37, 80, 9,
    66, 52, 23, 38, 81, 67, 10, 53, 24, 82, 68, 96, 39, 11, 54, 83, 97, 69, 25, 98, 84, 40, 112, 55, 12, 70, 99, 113, 85, 26,
    41, 56, 114, 100, 13, 71, 128, 86, 27, 115, 101, 129, 42, 57, 72, 116, 14, 87, 130, 102, 144, 73, 131, 117, 28, 58, 15, 88, 43, 145,
    103, 132, 146, 118, 74, 160, 89, 133, 104, 29, 59, 147, 119, 44, 161, 148, 90, 105, 134, 162, 120, 176, 75, 135, 149, 30, 60, 163, 177, 45,
    121, 91, 106, 164, 178, 150, 192, 136, 165, 179, 31, 151, 193, 76, 122, 61, 137, 194, 107, 152, 180, 208, 46, 166, 167, 195, 92, 181, 138, 209,
    123, 153, 224, 196, 77, 168, 210, 182, 240, 108, 197, 62, 154, 225, 183, 169, 211, 47, 139, 93, 184, 226, 212, 241, 198, 170, 124, 155, 199, 78,
    213, 185, 109, 227, 200, 63, 228, 242, 140, 214, 171, 186, 156, 229, 243, 125, 94, 201, 244, 215, 216, 230, 141, 187, 202, 79, 172, 110, 157, 245,
    217, 231, 95, 246, 232, 126, 203, 247, 233, 173, 218, 142, 111, 158, 188, 248, 127, 234, 219, 249, 189, 204, 143, 174, 159, 250, 235, 205, 220, 175,
    190, 251, 221, 191, 206, 236, 207, 237, 252, 222, 253, 223, 238, 239, 254, 255,
];

/// A scan with its neighbour table (raster positions of the above and
/// left coefficients already decoded, `2 * (n + 1)` entries).
pub struct ScanOrder {
    pub scan: &'static [i16],
    pub neighbors: Vec<u16>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanKind {
    Default,
    Col,
    Row,
}

fn neighbors(scan: &[i16], l: usize, kind: ScanKind) -> Vec<u16> {
    let n = l * l;
    let mut out = vec![0u16; 2 * (n + 1)];
    for (c, &rc) in scan.iter().enumerate().skip(1) {
        let rc = rc as usize;
        let (i, j) = (rc / l, rc % l);
        let (a, b) = if i > 0 && j > 0 {
            match kind {
                ScanKind::Col => ((i - 1) * l + j, (i - 1) * l + j),
                ScanKind::Row => (i * l + j - 1, i * l + j - 1),
                ScanKind::Default => ((i - 1) * l + j, i * l + j - 1),
            }
        } else if i > 0 {
            ((i - 1) * l + j, (i - 1) * l + j)
        } else {
            (i * l + j - 1, i * l + j - 1)
        };
        out[2 * c] = a as u16;
        out[2 * c + 1] = b as u16;
    }
    out
}

/// `[tx size][tx type]` scan orders (32x32 always uses the default scan).
pub fn scan_orders() -> &'static [[ScanOrder; 4]; 4] {
    static ORDERS: OnceLock<[[ScanOrder; 4]; 4]> = OnceLock::new();
    ORDERS.get_or_init(|| {
        let make = |scan: &'static [i16], l: usize, kind: ScanKind| ScanOrder { scan, neighbors: neighbors(scan, l, kind) };
        let set = |d: &'static [i16], c: &'static [i16], r: &'static [i16], l: usize| -> [ScanOrder; 4] {
            [
                make(d, l, ScanKind::Default),
                make(r, l, ScanKind::Row),
                make(c, l, ScanKind::Col),
                make(d, l, ScanKind::Default),
            ]
        };
        [
            set(&crate::scan::SCAN_4X4, &COL_SCAN_4X4, &ROW_SCAN_4X4, 4),
            set(&crate::scan::SCAN_8X8, &COL_SCAN_8X8, &ROW_SCAN_8X8, 8),
            set(&crate::scan::SCAN_16X16, &COL_SCAN_16X16, &ROW_SCAN_16X16, 16),
            set(&crate::scan::SCAN_32X32, &crate::scan::SCAN_32X32, &crate::scan::SCAN_32X32, 32),
        ]
    })
}

/// Sub-pixel interpolation kernels `[filter][position][tap]` in filter
/// order regular, smooth, sharp, bilinear.
#[rustfmt::skip]
pub const FILTERS: [[[i16; 8]; 16]; 4] = [
    [
        [0, 0, 0, 128, 0, 0, 0, 0], [0, 1, -5, 126, 8, -3, 1, 0], [-1, 3, -10, 122, 18, -6, 2, 0], [-1, 4, -13, 118, 27, -9, 3, -1],
        [-1, 4, -16, 112, 37, -11, 4, -1], [-1, 5, -18, 105, 48, -14, 4, -1], [-1, 5, -19, 97, 58, -16, 5, -1], [-1, 6, -19, 88, 68, -18, 5, -1],
        [-1, 6, -19, 78, 78, -19, 6, -1], [-1, 5, -18, 68, 88, -19, 6, -1], [-1, 5, -16, 58, 97, -19, 5, -1], [-1, 4, -14, 48, 105, -18, 5, -1],
        [-1, 4, -11, 37, 112, -16, 4, -1], [-1, 3, -9, 27, 118, -13, 4, -1], [0, 2, -6, 18, 122, -10, 3, -1], [0, 1, -3, 8, 126, -5, 1, 0],
    ],
    [
        [0, 0, 0, 128, 0, 0, 0, 0], [-3, -1, 32, 64, 38, 1, -3, 0], [-2, -2, 29, 63, 41, 2, -3, 0], [-2, -2, 26, 63, 43, 4, -4, 0],
        [-2, -3, 24, 62, 46, 5, -4, 0], [-2, -3, 21, 60, 49, 7, -4, 0], [-1, -4, 18, 59, 51, 9, -4, 0], [-1, -4, 16, 57, 53, 12, -4, -1],
        [-1, -4, 14, 55, 55, 14, -4, -1], [-1, -4, 12, 53, 57, 16, -4, -1], [0, -4, 9, 51, 59, 18, -4, -1], [0, -4, 7, 49, 60, 21, -3, -2],
        [0, -4, 5, 46, 62, 24, -3, -2], [0, -4, 4, 43, 63, 26, -2, -2], [0, -3, 2, 41, 63, 29, -2, -2], [0, -3, 1, 38, 64, 32, -1, -3],
    ],
    [
        [0, 0, 0, 128, 0, 0, 0, 0], [-1, 3, -7, 127, 8, -3, 1, 0], [-2, 5, -13, 125, 17, -6, 3, -1], [-3, 7, -17, 121, 27, -10, 5, -2],
        [-4, 9, -20, 115, 37, -13, 6, -2], [-4, 10, -23, 108, 48, -16, 8, -3], [-4, 10, -24, 100, 59, -19, 9, -3], [-4, 11, -24, 90, 70, -21, 10, -4],
        [-4, 11, -23, 80, 80, -23, 11, -4], [-4, 10, -21, 70, 90, -24, 11, -4], [-3, 9, -19, 59, 100, -24, 10, -4], [-3, 8, -16, 48, 108, -23, 10, -4],
        [-2, 6, -13, 37, 115, -20, 9, -4], [-2, 5, -10, 27, 121, -17, 7, -3], [-1, 3, -6, 17, 125, -13, 5, -2], [0, 1, -3, 8, 127, -7, 3, -1],
    ],
    [
        [0, 0, 0, 128, 0, 0, 0, 0], [0, 0, 0, 120, 8, 0, 0, 0], [0, 0, 0, 112, 16, 0, 0, 0], [0, 0, 0, 104, 24, 0, 0, 0],
        [0, 0, 0, 96, 32, 0, 0, 0], [0, 0, 0, 88, 40, 0, 0, 0], [0, 0, 0, 80, 48, 0, 0, 0], [0, 0, 0, 72, 56, 0, 0, 0],
        [0, 0, 0, 64, 64, 0, 0, 0], [0, 0, 0, 56, 72, 0, 0, 0], [0, 0, 0, 48, 80, 0, 0, 0], [0, 0, 0, 40, 88, 0, 0, 0],
        [0, 0, 0, 32, 96, 0, 0, 0], [0, 0, 0, 24, 104, 0, 0, 0], [0, 0, 0, 16, 112, 0, 0, 0], [0, 0, 0, 8, 120, 0, 0, 0],
    ],
];

/// `mv_ref_blocks[bsize]`: candidate positions `(row, col)` in 8x8 units.
#[rustfmt::skip]
pub const MV_REF_BLOCKS: [[(i8, i8); 8]; 13] = [
    [(-1, 0), (0, -1), (-1, -1), (-2, 0), (0, -2), (-2, -1), (-1, -2), (-2, -2)],
    [(-1, 0), (0, -1), (-1, -1), (-2, 0), (0, -2), (-2, -1), (-1, -2), (-2, -2)],
    [(-1, 0), (0, -1), (-1, -1), (-2, 0), (0, -2), (-2, -1), (-1, -2), (-2, -2)],
    [(-1, 0), (0, -1), (-1, -1), (-2, 0), (0, -2), (-2, -1), (-1, -2), (-2, -2)],
    [(0, -1), (-1, 0), (1, -1), (-1, -1), (0, -2), (-2, 0), (-2, -1), (-1, -2)],
    [(-1, 0), (0, -1), (-1, 1), (-1, -1), (-2, 0), (0, -2), (-1, -2), (-2, -1)],
    [(-1, 0), (0, -1), (-1, 1), (1, -1), (-1, -1), (-3, 0), (0, -3), (-3, -3)],
    [(0, -1), (-1, 0), (2, -1), (-1, -1), (-1, 1), (0, -3), (-3, 0), (-3, -3)],
    [(-1, 0), (0, -1), (-1, 2), (-1, -1), (1, -1), (-3, 0), (0, -3), (-3, -3)],
    [(-1, 1), (1, -1), (-1, 2), (2, -1), (-1, -1), (-3, 0), (0, -3), (-3, -3)],
    [(0, -1), (-1, 0), (4, -1), (-1, 2), (-1, -1), (0, -3), (-3, 0), (2, -1)],
    [(-1, 0), (0, -1), (-1, 4), (2, -1), (-1, -1), (-3, 0), (0, -3), (-1, 2)],
    [(-1, 3), (3, -1), (-1, 4), (4, -1), (-1, -1), (-1, 0), (0, -1), (-1, 6)],
];

pub const MODE_2_COUNTER: [u8; 14] = [9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 0, 3, 1];
pub const COUNTER_TO_CONTEXT: [u8; 19] = [2, 3, 4, 1, 3, 9, 0, 9, 9, 5, 5, 9, 5, 9, 9, 9, 9, 9, 6];
pub const IDX_N_COLUMN_TO_SUBBLOCK: [[usize; 2]; 4] = [[1, 2], [1, 3], [3, 2], [3, 3]];

/// `mode_lf_lut`: 0 for intra modes and ZEROMV, 1 for the other inter modes.
pub const MODE_LF_LUT: [u8; 14] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1];

pub const COUNT_TO_UPDATE_FACTOR: [u32; 21] =
    [0, 6, 12, 19, 25, 32, 38, 44, 51, 57, 64, 70, 76, 83, 89, 96, 102, 108, 115, 121, 128];

#[cfg(test)]
mod tests {
    use super::*;

    fn permutation(s: &[i16]) -> bool {
        let mut seen = vec![false; s.len()];
        for &v in s {
            let v = v as usize;
            if v >= s.len() || seen[v] {
                return false;
            }
            seen[v] = true;
        }
        true
    }

    #[test]
    fn scans_are_permutations() {
        for s in [&COL_SCAN_4X4[..], &ROW_SCAN_4X4, &COL_SCAN_8X8, &ROW_SCAN_8X8, &COL_SCAN_16X16, &ROW_SCAN_16X16] {
            assert!(permutation(s), "{:?}", &s[..8]);
        }
    }

    #[test]
    fn derived_neighbors_match_the_extracted_default_tables() {
        let o = scan_orders();
        let nb4: Vec<u16> = crate::tabellen::NB_4X4.iter().map(|v| *v as u16).collect();
        let nb8: Vec<u16> = crate::tabellen::NB_8X8.iter().map(|v| *v as u16).collect();
        let nb16: Vec<u16> = crate::tabellen::NB_16X16.iter().map(|v| *v as u16).collect();
        let nb32: Vec<u16> = crate::tabellen::NB_32X32.iter().map(|v| *v as u16).collect();
        assert_eq!(o[0][0].neighbors, nb4);
        assert_eq!(o[1][0].neighbors, nb8);
        assert_eq!(o[2][0].neighbors, nb16);
        assert_eq!(o[3][0].neighbors, nb32);
    }

    #[test]
    fn filter_kernels_sum_to_128() {
        for f in &FILTERS {
            for k in f {
                assert_eq!(k.iter().map(|v| *v as i32).sum::<i32>(), 128);
            }
        }
    }
}
