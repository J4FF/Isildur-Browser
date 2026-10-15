// GEMESSEN aus libvpx vp9/common/vp9_entropy.c, unveraendert abgelesen.
// Layout: [PLANE_TYPES=2][REF_TYPES=2][COEF_BANDS=6][BAND_COEFF_CONTEXTS][UNCONSTRAINED_NODES=3]
// Band 0 hat 3 Kontexte, die Baender 1 bis 5 je 6: 2*2*(3+6*5)*3 = 396.

/// default_coef_probs_4x4
pub const DEFAULT_COEF_4X4: [u8; 396] = [
    195, 29, 183, 84, 49, 136, 8, 42, 71, 31, 107, 169,
    35, 99, 159, 17, 82, 140, 8, 66, 114, 2, 44, 76,
    1, 19, 32, 40, 132, 201, 29, 114, 187, 13, 91, 157,
    7, 75, 127, 3, 58, 95, 1, 28, 47, 69, 142, 221,
    42, 122, 201, 15, 91, 159, 6, 67, 121, 1, 42, 77,
    1, 17, 31, 102, 148, 228, 67, 117, 204, 17, 82, 154,
    6, 59, 114, 2, 39, 75, 1, 15, 29, 156, 57, 233,
    119, 57, 212, 58, 48, 163, 29, 40, 124, 12, 30, 81,
    3, 12, 31, 191, 107, 226, 124, 117, 204, 25, 99, 155,
    29, 148, 210, 37, 126, 194, 8, 93, 157, 2, 68, 118,
    1, 39, 69, 1, 17, 33, 41, 151, 213, 27, 123, 193,
    3, 82, 144, 1, 58, 105, 1, 32, 60, 1, 13, 26,
    59, 159, 220, 23, 126, 198, 4, 88, 151, 1, 66, 114,
    1, 38, 71, 1, 18, 34, 114, 136, 232, 51, 114, 207,
    11, 83, 155, 3, 56, 105, 1, 33, 65, 1, 17, 34,
    149, 65, 234, 121, 57, 215, 61, 49, 166, 28, 36, 114,
    12, 25, 76, 3, 16, 42, 214, 49, 220, 132, 63, 188,
    42, 65, 137, 85, 137, 221, 104, 131, 216, 49, 111, 192,
    21, 87, 155, 2, 49, 87, 1, 16, 28, 89, 163, 230,
    90, 137, 220, 29, 100, 183, 10, 70, 135, 2, 42, 81,
    1, 17, 33, 108, 167, 237, 55, 133, 222, 15, 97, 179,
    4, 72, 135, 1, 45, 85, 1, 19, 38, 124, 146, 240,
    66, 124, 224, 17, 88, 175, 4, 58, 122, 1, 36, 75,
    1, 18, 37, 141, 79, 241, 126, 70, 227, 66, 58, 182,
    30, 44, 136, 12, 34, 96, 2, 20, 47, 229, 99, 249,
    143, 111, 235, 46, 109, 192, 82, 158, 236, 94, 146, 224,
    25, 117, 191, 9, 87, 149, 3, 56, 99, 1, 33, 57,
    83, 167, 237, 68, 145, 222, 10, 103, 177, 2, 72, 131,
    1, 41, 79, 1, 20, 39, 99, 167, 239, 47, 141, 224,
    10, 104, 178, 2, 73, 133, 1, 44, 85, 1, 22, 47,
    127, 145, 243, 71, 129, 228, 17, 93, 177, 3, 61, 124,
    1, 41, 84, 1, 21, 52, 157, 78, 244, 140, 72, 231,
    69, 58, 184, 31, 44, 137, 14, 38, 105, 8, 23, 61
];

/// default_coef_probs_8x8
pub const DEFAULT_COEF_8X8: [u8; 396] = [
    125, 34, 187, 52, 41, 133, 6, 31, 56, 37, 109, 153,
    51, 102, 147, 23, 87, 128, 8, 67, 101, 1, 41, 63,
    1, 19, 29, 31, 154, 185, 17, 127, 175, 6, 96, 145,
    2, 73, 114, 1, 51, 82, 1, 28, 45, 23, 163, 200,
    10, 131, 185, 2, 93, 148, 1, 67, 111, 1, 41, 69,
    1, 14, 24, 29, 176, 217, 12, 145, 201, 3, 101, 156,
    1, 69, 111, 1, 39, 63, 1, 14, 23, 57, 192, 233,
    25, 154, 215, 6, 109, 167, 3, 78, 118, 1, 48, 69,
    1, 21, 29, 202, 105, 245, 108, 106, 216, 18, 90, 144,
    33, 172, 219, 64, 149, 206, 14, 117, 177, 5, 90, 141,
    2, 61, 95, 1, 37, 57, 33, 179, 220, 11, 140, 198,
    1, 89, 148, 1, 60, 104, 1, 33, 57, 1, 12, 21,
    30, 181, 221, 8, 141, 198, 1, 87, 145, 1, 58, 100,
    1, 31, 55, 1, 12, 20, 32, 186, 224, 7, 142, 198,
    1, 86, 143, 1, 58, 100, 1, 31, 55, 1, 12, 22,
    57, 192, 227, 20, 143, 204, 3, 96, 154, 1, 68, 112,
    1, 42, 69, 1, 19, 32, 212, 35, 215, 113, 47, 169,
    29, 48, 105, 74, 129, 203, 106, 120, 203, 49, 107, 178,
    19, 84, 144, 4, 50, 84, 1, 15, 25, 71, 172, 217,
    44, 141, 209, 15, 102, 173, 6, 76, 133, 2, 51, 89,
    1, 24, 42, 64, 185, 231, 31, 148, 216, 8, 103, 175,
    3, 74, 131, 1, 46, 81, 1, 18, 30, 65, 196, 235,
    25, 157, 221, 5, 105, 174, 1, 67, 120, 1, 38, 69,
    1, 15, 30, 65, 204, 238, 30, 156, 224, 7, 107, 177,
    2, 70, 124, 1, 42, 73, 1, 18, 34, 225, 86, 251,
    144, 104, 235, 42, 99, 181, 85, 175, 239, 112, 165, 229,
    29, 136, 200, 12, 103, 162, 6, 77, 123, 2, 53, 84,
    75, 183, 239, 30, 155, 221, 3, 106, 171, 1, 74, 128,
    1, 44, 76, 1, 17, 28, 73, 185, 240, 27, 159, 222,
    2, 107, 172, 1, 75, 127, 1, 42, 73, 1, 17, 29,
    62, 190, 238, 21, 159, 222, 2, 107, 172, 1, 72, 122,
    1, 40, 71, 1, 18, 32, 61, 199, 240, 27, 161, 226,
    4, 113, 180, 1, 76, 129, 1, 46, 80, 1, 23, 41
];

/// default_coef_probs_16x16
pub const DEFAULT_COEF_16X16: [u8; 396] = [
    7, 27, 153, 5, 30, 95, 1, 16, 30, 50, 75, 127,
    57, 75, 124, 27, 67, 108, 10, 54, 86, 1, 33, 52,
    1, 12, 18, 43, 125, 151, 26, 108, 148, 7, 83, 122,
    2, 59, 89, 1, 38, 60, 1, 17, 27, 23, 144, 163,
    13, 112, 154, 2, 75, 117, 1, 50, 81, 1, 31, 51,
    1, 14, 23, 18, 162, 185, 6, 123, 171, 1, 78, 125,
    1, 51, 86, 1, 31, 54, 1, 14, 23, 15, 199, 227,
    3, 150, 204, 1, 91, 146, 1, 55, 95, 1, 30, 53,
    1, 11, 20, 19, 55, 240, 19, 59, 196, 3, 52, 105,
    41, 166, 207, 104, 153, 199, 31, 123, 181, 14, 101, 152,
    5, 72, 106, 1, 36, 52, 35, 176, 211, 12, 131, 190,
    2, 88, 144, 1, 60, 101, 1, 36, 60, 1, 16, 28,
    28, 183, 213, 8, 134, 191, 1, 86, 142, 1, 56, 96,
    1, 30, 53, 1, 12, 20, 20, 190, 215, 4, 135, 192,
    1, 84, 139, 1, 53, 91, 1, 28, 49, 1, 11, 20,
    13, 196, 216, 2, 137, 192, 1, 86, 143, 1, 57, 99,
    1, 32, 56, 1, 13, 24, 211, 29, 217, 96, 47, 156,
    22, 43, 87, 78, 120, 193, 111, 116, 186, 46, 102, 164,
    15, 80, 128, 2, 49, 76, 1, 18, 28, 71, 161, 203,
    42, 132, 192, 10, 98, 150, 3, 69, 109, 1, 44, 70,
    1, 18, 29, 57, 186, 211, 30, 140, 196, 4, 93, 146,
    1, 62, 102, 1, 38, 65, 1, 16, 27, 47, 199, 217,
    14, 145, 196, 1, 88, 142, 1, 57, 98, 1, 36, 62,
    1, 15, 26, 26, 219, 229, 5, 155, 207, 1, 94, 151,
    1, 60, 104, 1, 36, 62, 1, 16, 28, 233, 29, 248,
    146, 47, 220, 43, 52, 140, 100, 163, 232, 179, 161, 222,
    63, 142, 204, 37, 113, 174, 26, 89, 137, 18, 68, 97,
    85, 181, 230, 32, 146, 209, 7, 100, 164, 3, 71, 121,
    1, 45, 77, 1, 18, 30, 65, 187, 230, 20, 148, 207,
    2, 97, 159, 1, 68, 116, 1, 40, 70, 1, 14, 29,
    40, 194, 227, 8, 147, 204, 1, 94, 155, 1, 65, 112,
    1, 39, 66, 1, 14, 26, 16, 208, 228, 3, 151, 207,
    1, 98, 160, 1, 67, 117, 1, 41, 74, 1, 17, 31
];

/// default_coef_probs_32x32
pub const DEFAULT_COEF_32X32: [u8; 396] = [
    17, 38, 140, 7, 34, 80, 1, 17, 29, 37, 75, 128,
    41, 76, 128, 26, 66, 116, 12, 52, 94, 2, 32, 55,
    1, 10, 16, 50, 127, 154, 37, 109, 152, 16, 82, 121,
    5, 59, 85, 1, 35, 54, 1, 13, 20, 40, 142, 167,
    17, 110, 157, 2, 71, 112, 1, 44, 72, 1, 27, 45,
    1, 11, 17, 30, 175, 188, 9, 124, 169, 1, 74, 116,
    1, 48, 78, 1, 30, 49, 1, 11, 18, 10, 222, 223,
    2, 150, 194, 1, 83, 128, 1, 48, 79, 1, 27, 45,
    1, 11, 17, 36, 41, 235, 29, 36, 193, 10, 27, 111,
    85, 165, 222, 177, 162, 215, 110, 135, 195, 57, 113, 168,
    23, 83, 120, 10, 49, 61, 85, 190, 223, 36, 139, 200,
    5, 90, 146, 1, 60, 103, 1, 38, 65, 1, 18, 30,
    72, 202, 223, 23, 141, 199, 2, 86, 140, 1, 56, 97,
    1, 36, 61, 1, 16, 27, 55, 218, 225, 13, 145, 200,
    1, 86, 141, 1, 57, 99, 1, 35, 61, 1, 13, 22,
    15, 235, 212, 1, 132, 184, 1, 84, 139, 1, 57, 97,
    1, 34, 56, 1, 14, 23, 181, 21, 201, 61, 37, 123,
    10, 38, 71, 47, 106, 172, 95, 104, 173, 42, 93, 159,
    18, 77, 131, 4, 50, 81, 1, 17, 23, 62, 147, 199,
    44, 130, 189, 28, 102, 154, 18, 75, 115, 2, 44, 65,
    1, 12, 19, 55, 153, 210, 24, 130, 194, 3, 93, 146,
    1, 61, 97, 1, 31, 50, 1, 10, 16, 49, 186, 223,
    17, 148, 204, 1, 96, 142, 1, 53, 83, 1, 26, 44,
    1, 11, 17, 13, 217, 212, 2, 136, 180, 1, 78, 124,
    1, 50, 83, 1, 29, 49, 1, 14, 23, 197, 13, 247,
    82, 17, 222, 25, 17, 162, 126, 186, 247, 234, 191, 243,
    176, 177, 234, 104, 158, 220, 66, 128, 186, 55, 90, 137,
    111, 197, 242, 46, 158, 219, 9, 104, 171, 2, 65, 125,
    1, 44, 80, 1, 17, 91, 104, 208, 245, 39, 168, 224,
    3, 109, 162, 1, 79, 124, 1, 50, 102, 1, 43, 102,
    84, 220, 246, 31, 177, 231, 2, 115, 180, 1, 79, 134,
    1, 55, 77, 1, 60, 79, 43, 243, 240, 8, 180, 217,
    1, 115, 166, 1, 84, 121, 1, 51, 67, 1, 16, 6
];

use crate::tile::BoolQuelle;
use crate::vp9_bool::Vp9Fehler;

/// `DIFF_UPDATE_PROB` — GEMESSEN `vp9_entropy.h:24`.
pub const DIFF_UPDATE_PROB: u8 = 252;
/// `SKIP_CONTEXTS` — GEMESSEN `vp9_enums.h:131`.
pub const SKIP_CONTEXTS: usize = 3;
/// `COEF_BANDS` — GEMESSEN `vp9_entropy.h:91`.
pub const COEF_BANDS: usize = 6;
/// `UNCONSTRAINED_NODES` — GEMESSEN `vp9_entropy.h:144`.
pub const UNCONSTRAINED_NODES: usize = 3;
/// `BAND_COEFF_CONTEXTS(0) = 3`, sonst `COEFF_CONTEXTS = 6` — GEMESSEN
/// `vp9_entropy.h:109/110`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Funktion und keine Konstante ist:
/// die Zahl der Kontexte haengt vom **Band** ab, und ein flacher Index
/// `band * 6` laesst Band 0 sechs Kontexte lesen, von denen nur drei
/// existieren. Der Index verschiebt sich dann um **drei Eintraege** je
/// Band — und `base_q_idx` stimmt trotzdem, weil Boolean-Bits sich nicht
/// an Grenzen stoeren. Genau die Sorte Fehler, die erst spät auffaellt.
pub fn band_kontexte(band: usize) -> usize {
    if band == 0 { 3 } else { 6 }
}

/// Der Versatz des Bandes `band` innerhalb von `[Ebene][Referenz]`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **aufsummiert** und nicht
/// `band * 18`: Band 0 hat 3 Kontexte, die anderen je 6, also
/// `0*9 + 3*3 = 9`, dann `9 + 18 = 27`, und so weiter.
fn band_versatz(band: usize) -> usize {
    let mut v = 0;
    for b in 0..band {
        v += band_kontexte(b) * UNCONSTRAINED_NODES;
    }
    v
}

/// `default_skip_probs` — GEMESSEN `vp9_entropymode.c:313`.
pub const DEFAULT_SKIP_PROBS: [u8; SKIP_CONTEXTS] = [192, 128, 64];

/// `tx_mode_to_biggest_tx_size` — GEMESSEN `vp9_common_data.c:102`.
/// **`TX_MODE_SELECT = 4` ist nicht 3**, sondern der **fünfte** Wert, und
/// er zeigt auf `TX_32X32`. GEMESSENE ENTSCHEIDUNG, warum das eine
/// Falle ist: `read_tx_mode` liest `literal(2)` = 0..3 und addiert bei 3
/// noch ein Bit → **4**. Wer die Tabelle auf vier Eintraege kuerzt, laesst
/// den letzten Index ins Leere greifen.
pub const TX_MODE_GROESSTE: [u8; 5] = [0, 1, 2, 3, 3];

/// `inv_recenter_nonneg` — wortgleich `vp9_dsubexp.c`.
///
/// ```c
/// static int inv_recenter_nonneg(int v, int m) {
///   if (v > 2 * m) return v;
///   return (v & 1) ? m - ((v + 1) >> 1) : m + (v >> 1);
/// }
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum `v > 2 * m` und **nicht** `v >=`:
/// GEMESSEN in der Referenz ist es `>`. Mit `>=` faellt der Wert
/// `v == 2m` in den zweiten Zweig und ergibt `m + m = 2m` statt `2m + 1`
/// — ein **halbes** Bit daneben bei genau einem Wert von 4.
pub fn inv_recenter_nonneg(v: i32, m: i32) -> i32 {
    if v > 2 * m { v } else if v & 1 != 0 { m - ((v + 1) >> 1) } else { m + (v >> 1) }
}

/// `decode_uniform` — wortgleich.
///
/// ```c
/// static int decode_uniform(vpx_reader *r) {
///   const int l = 8;
///   const int m = (1 << l) - 191;          // = 65
///   const int v = vpx_read_literal(r, l - 1);   // 7 Bit, 0-basiert
///   return v < m ? v : (v << 1) - m + vpx_read_bit(r);
/// }
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum `l = 8` und dann **7** Bit gelesen
/// werden: das ist **kein** Tippfehler. `m = 65`, und `v` laeuft 0..127.
/// Fuer `v < 65` ist der Wert fertig; sonst wird ein **Extra-Bit**
/// gelesen und der Wert ist `(v<<1) - 65 + bit`. Die Verteilung ist
/// bewusst **nicht** gleich — sie ist so gebaut, dass 0..64 genau einmal
/// vorkommen und 65..127 je zweimal, zusammen 255 Werte fuer eine
/// Wahrscheinlichkeit.
fn decode_uniform(b: &mut impl BoolQuelle) -> i32 {
    const L: i32 = 8;
    const M: i32 = (1 << L) - 191;
    // GEMESSENER FEHLER, VIDEO_35: hier stand
    // `b.lies_flag_mit_prob(128)` — **ein** Bit. GEMESSEN ist aber
    // `vpx_read_literal(r, l - 1)`, also **sieben** Bit bei `l = 8`.
    // GEMESSENE ENTSCHEIDUNG, warum das so **laut** und trotzdem so spät war:
    // GEMESSEN ist `m = (1 << 8) - 191 = 65`, und mit **einem** Bit ist
    // `v ∈ {0, 1}` — also ist `v < 65` **immer** wahr. Damit wurde der ganze
    // Zweig `(v << 1) - m + vpx_read_bit(r)` **nie** genommen, und
    // `decode_uniform` lieferte nur **0 oder 1** statt **0 bis 127**.
    // GEMESSENE FOLGE: `delp` war `64 + 0` oder `64 + 1` statt des echten
    // Werts, also `INV_MAP_TABLE[64]` statt des richtigen Eintrags — und
    // GEMESSEN sind die ersten beiden Aenderungen **identisch**, weil sie
    // ueber den 4-Bit-Zweig gingen, wo `roh(4)` stimmte.
    let mut v = 0;
    for _ in 0..(L - 1) {
        v = (v << 1) | b.lies_flag_mit_prob(128) as i32;
    }
    if v < M {
        v
    } else {
        (v << 1) - M + b.lies_flag_mit_prob(128) as i32
    }
}

/// `decode_term_subexp` — wortgleich `vp9_dsubexp.c`.
///
/// ```c
/// static int decode_term_subexp(vpx_reader *r) {
///   if (!vpx_read_bit(r)) return vpx_read_literal(r, 4);
///   if (!vpx_read_bit(r)) return vpx_read_literal(r, 4) + 16;
///   if (!vpx_read_bit(r)) return vpx_read_literal(r, 5) + 32;
///   return decode_uniform(r) + 64;
/// }
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum **nullbasiert** und nicht VP9s
/// `read_literal`: GEMESSEN in `vpx_dsp/bitreader.h` ist
/// `vpx_read_literal(r, n)` ein n-stelliger **Gleichstand** ohne
/// Abziehen. Das „plus 1" aus VIDEO_24 gilt fuer
/// `vpx_read_bit_buffer` (dem Rohbit-Puffer), **nicht** fuer den
/// `vpx_reader`. Zwei gleichnamige Funktionen, zwei Bedeutungen.
fn decode_term_subexp(b: &mut impl BoolQuelle) -> i32 {
    // GEMESSEN: `vpx_read_bit(r)` — drei Mal bedingte Bit, dann der Wert.
    // GEMESSENE ENTSCHEIDUNG, warum **nullbasiert**: GEMESSEN ist
    // `vpx_read_literal`, und das ist ein Gleichstand ohne Abziehen.
    let roh = |b: &mut dyn FnMut() -> u32, n: u32| -> u32 {
        let mut v = 0;
        for _ in 0..n { v = (v << 1) | b(); }
        v
    };
    if b.lies_flag_mit_prob(128) == 0 { return roh(&mut || b.lies_flag_mit_prob(128), 4) as i32; }
    if b.lies_flag_mit_prob(128) == 0 { return roh(&mut || b.lies_flag_mit_prob(128), 4) as i32 + 16; }
    if b.lies_flag_mit_prob(128) == 0 { return roh(&mut || b.lies_flag_mit_prob(128), 5) as i32 + 32; }
    decode_uniform(b) + 64
}

/// `inv_map_table` — wortgleich `vp9_dsubexp.c`, **255 Eintraege**.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine **Tabelle** ist und keine
/// Formel: sie ist im C-Code eine Tabelle, und sie ist **nicht**
/// berechenbar — die Werte springen (7, 20, 33 … 254, **1**, 2, 3 …).
/// GEMESSEN ist auch, dass die **letzten zwei** Eintraege gleich `253`
/// sind. Das ist kein Tippfehler der Quelle, sondern so in libvpx — und
/// eine „bereinigte" Tabelle waere eine andere.
#[rustfmt::skip]
pub const INV_MAP_TABLE: [u8; 255] = [
    7, 20, 33, 46, 59, 72, 85, 98, 111, 124, 137, 150, 163, 176, 189,
    202, 215, 228, 241, 254, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11,
    12, 13, 14, 15, 16, 17, 18, 19, 21, 22, 23, 24, 25, 26, 27,
    28, 29, 30, 31, 32, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43,
    44, 45, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 60,
    61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 73, 74, 75, 76,
    77, 78, 79, 80, 81, 82, 83, 84, 86, 87, 88, 89, 90, 91, 92,
    93, 94, 95, 96, 97, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108,
    109, 110, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 125,
    126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 138, 139, 140, 141,
    142, 143, 144, 145, 146, 147, 148, 149, 151, 152, 153, 154, 155, 156, 157,
    158, 159, 160, 161, 162, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173,
    174, 175, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 190,
    191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 203, 204, 205, 206,
    207, 208, 209, 210, 211, 212, 213, 214, 216, 217, 218, 219, 220, 221, 222,
    223, 224, 225, 226, 227, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238,
    239, 240, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 253
 ];

/// `inv_remap_prob` — wortgleich `vp9_dsubexp.c`.
pub fn inv_remap_prob(v: usize, m: u8) -> u8 {
    let wert = INV_MAP_TABLE[v] as i32;
    let m = i32::from(m) - 1;
    if (m << 1) <= 255 {
        (1 + inv_recenter_nonneg(wert, m)) as u8
    } else {
        (255 - inv_recenter_nonneg(wert, 255 - 1 - m)) as u8
    }
}

/// `vp9_diff_update_prob` — wortgleich.
///
/// ```c
/// void vp9_diff_update_prob(vpx_reader *r, vpx_prob *p) {
///   if (vpx_read(r, DIFF_UPDATE_PROB)) {
///     const int delp = decode_term_subexp(r);
///     *p = (vpx_prob)inv_remap_prob(delp, *p);
///   }
/// }
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum `vpx_read(r, 252)` und **nicht** ein
/// Gleichstands-Bit: das Vorschaltbit ist selbst eine **Wahrschein-
/// lichkeit**, und bei 252 ist es fast immer wahr. Wer es als `bool()`
/// liest, verliert die Anpassung und damit **alle** folgenden Felder.
pub fn diff_update_prob(b: &mut impl BoolQuelle, p: &mut u8) -> bool {
    if b.lies_flag_mit_prob(DIFF_UPDATE_PROB) == 0 {
        return false;
    }
    let delp = decode_term_subexp(b) as usize;
    *p = inv_remap_prob(delp, *p);
    true
}

/// `read_tx_mode` — wortgleich.
///
/// ```c
/// static TX_MODE read_tx_mode(vpx_reader *r) {
///   TX_MODE tx_mode = vpx_read_literal(r, 2);
///   if (tx_mode == ALLOW_32X32) tx_mode += vpx_read_bit(r);
///   return tx_mode;
/// }
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum `vpx_read_literal(r, 2)` hier
/// **nullbasiert** ist: es ist ein `vpx_reader`, nicht der
/// `vpx_read_bit_buffer` aus VIDEO_24. Das „plus 1" gilt dort **nicht**.
pub fn read_tx_mode(b: &mut impl BoolQuelle) -> u32 {
    // GEMESSEN: `vpx_read_literal(r, 2)`, also **nullbasiert** und
    // Gleichstand. VIDEO_31 hatte `Leser::roh(2)` benutzt, und der neue
    // Leser gibt `3` statt `1` — das war das fehlende Marker-Bit.
    let mut m = b.lies_flag_mit_prob(128) << 1;
    m |= b.lies_flag_mit_prob(128);
    if m == 3 {
        m += b.lies_flag_mit_prob(128);
    }
    m
}

/// `default_tx_probs` — GEMESSEN `vp9_entropymode.c:281`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das die letzte Falle in dieser Tabelle
/// ist: GEMESSEN ist
/// ```c
/// struct tx_probs {
///   vpx_prob p32x32[TX_SIZE_CONTEXTS][TX_SIZES - 1];   // zuerst
///   vpx_prob p16x16[TX_SIZE_CONTEXTS][TX_SIZES - 2];
///   vpx_prob p8x8  [TX_SIZE_CONTEXTS][TX_SIZES - 3];
/// };
/// static const struct tx_probs default_tx_probs = {
///   { { 3, 136, 37 }, { 5, 52, 13 } },   // p32x32
///   { { 20, 152 },     { 15, 101 } },   // p16x16
///   { { 100 },         { 66 } } };      // p8x8
/// ```
/// Der Initialisierer steht in **Deklarationsreihenfolge** — `p32x32`
/// zuerst. GEMESSEN ist aber die **Lese**reihenfolge in
/// `read_tx_mode_probs`: `p8x8`, dann `p16x16`, dann `p32x32`.
/// GEMESSENE FOLGE, warum das gefaehrlich ist: die Startwerte sind alle
/// zweistellig (100, 20, 3 …), ein Vertauschen faellt **nicht** auf — der
/// Decoder liest weiter und meldet nichts.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier **zwoelf** Updates stehen und nicht
/// die **21**, die VIDEO_31 notiert hat: GEMESSEN ist
/// `TX_SIZE_CONTEXTS = 2` und `TX_SIZES = 4`, also
/// `2 * (4 - 3) + 2 * (4 - 2) + 2 * (4 - 1) = 2 + 4 + 6 = 12`.
/// VIDEO_31 hatte `4 + 6 + 6 + 5` notiert — geraten, und falsch.
#[rustfmt::skip]
pub const DEFAULT_TX_PROBS: TxProbs = TxProbs {
    p8x8: [[100], [66]],
    p16x16: [[20, 152], [15, 101]],
    p32x32: [[3, 136, 37], [5, 52, 13]],
};

/// `struct tx_probs` — GEMESSEN `vp9_entropymode.h:31`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxProbs {
    /// `p8x8[2][1]` — GEMESSEN `[TX_SIZE_CONTEXTS][TX_SIZES - 3]`.
    pub p8x8: [[u8; 1]; 2],
    /// `p16x16[2][2]` — GEMESSEN `[TX_SIZE_CONTEXTS][TX_SIZES - 2]`.
    pub p16x16: [[u8; 2]; 2],
    /// `p32x32[2][3]` — GEMESSEN `[TX_SIZE_CONTEXTS][TX_SIZES - 1]`.
    pub p32x32: [[u8; 3]; 2],
}

/// `read_tx_mode_probs` — GEMESSEN `vp9_decodeframe.c`.
///
/// GEMESSENE ENTSCHEIDUNG, warum `TX_MODE_SELECT` ueberhaupt erst
/// aufgetreten ist: VIDEO_31 hat `tx_mode = 1` gemessen und daraus
/// geschlossen, dieser Zweig sei fuer dieses Bild unmoeglich. GEMESSEN
/// ist `tx_mode = 4`, und **er** ist der Grund, warum die 12 Updates
/// kommen, warum die erste `partition` SPLIT ist und warum die erste
/// `tx_size` **32x32** wird. Ein Feld, das man fuer unmoeglich haelt,
/// verschiebt alles danach.
pub fn read_tx_mode_probs(b: &mut impl BoolQuelle, p: &mut TxProbs) -> u32 {
    let mut anzahl = 0;
    for i in 0..2 {
        for j in 0..1 {
            if diff_update_prob(b, &mut p.p8x8[i][j]) { anzahl += 1; }
        }
    }
    for i in 0..2 {
        for j in 0..2 {
            if diff_update_prob(b, &mut p.p16x16[i][j]) { anzahl += 1; }
        }
    }
    for i in 0..2 {
        for j in 0..3 {
            if diff_update_prob(b, &mut p.p32x32[i][j]) { anzahl += 1; }
        }
    }
    anzahl
}

/// Der Zustand des komprimierten Kopfs, **nach jedem Feld**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zustaende {
    /// `tx_mode`.
    pub tx_mode: u32,
    /// `tx_mode_to_biggest_tx_size[tx_mode]`.
    pub groesste_tx: u8,
    /// Ob `TX_MODE_SELECT` gelesen wurde.
    pub tx_mode_select: bool,
    /// Wie viele der **zwoelf** `read_tx_mode_probs`-Updates etwas
    /// geaendert haben. GEMESSEN: `TX_SIZE_CONTEXTS = 2`, `TX_SIZES = 4`.
    pub tx_probs_aenderungen: u32,
    /// `tx_probs` nach den Updates.
    pub tx_probs: TxProbs,
    /// Fuer jede der vier `TX_SIZE`: ob `read_coef_probs_common` ein
    /// Vorschaltbit **1** gelesen hat und wie viele Updates folgten.
    pub coef_aktualisiert: [bool; 4],
    /// Wie viele `vp9_diff_update_prob` je `TX_SIZE` wirklich etwas
    /// geaendert haben.
    pub coef_aenderungen: [u32; 4],
    /// **Bit-Bilanz** je `TX_SIZE` — GEMESSEN in libvpx als
    /// `VPX_MESSEN coef_common bits=`. GEMESSENE ENTSCHEIDUNG, warum das
    /// eine eigene Zahl ist und nicht `coef_aenderungen`: die Anzahl der
    /// *Aenderungen* ist nicht die Anzahl der *Bits* — ein Update kann 1 Bit
    /// (das Vorschaltbit) oder 14 Bits (Vorschaltbit plus
    /// `decode_term_subexp` plus `decode_uniform`) kosten. Nur die
    /// **Bit-Zahl** ist mit dem Orakel vergleichbar.
    pub coef_bits: [u32; 4],
    /// Die **ersten zwoelf** Aenderungen als
    /// `[ebene, referenz, band, kontext, node, neuer_wert]` — GEMESSEN zum
    /// Vergleich mit libvpx `VPX_MESSEN AENDERUNG`.
    pub coef_positionen: Vec<[u8; 6]>,
    /// Die **angepassten** `coef_probs[tx_size]` nach `read_coef_probs`.
    ///
    /// GEMESSENER FEHLER (VIDEO_38), der hier zuerst stand: es gab kein Feld
    /// dafuer. `let mut p = tabellen(tx);` machte eine **lokale Kopie**, wurde
    /// in-place veraendert und am Ende der Schleife **verworfen** — und
    /// `decode_coefs` bekam nichts als die **Default**-Tabelle zurueck.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **nicht** auffaellt und trotzdem alles
    /// verschiebt: GEMESSEN ist in `vp9_detokenize.c:125`
    /// ```c
    /// const vpx_prob(*coef_probs)[COEFF_CONTEXTS][UNCONSTRAINED_NODES] =
    ///     fc->coef_probs[tx_size][type][ref];
    /// ```
    /// und GEMESSEN ist, dass `fc->coef_probs` in `read_compressed_header`
    /// **in-place** aktualisiert wird. Ein Dekoder, der stattdessen die Defaults
    /// benutzt, liest **jedes** Boolean mit einer anderen Wahrscheinlichkeit —
    /// der Ablauf bricht nicht ab, er liest nur ab der **falschen** Stelle
    /// weiter. GEMESSEN ist der Preis dafuer: **159** statt **1** Token beim
    /// ersten Luma-Block, ohne Ueberlauf.
    pub coef_probs: [[u8; 396]; 4],
    /// `skip_probs` nach den drei Updates.
    pub skip_probs: [u8; SKIP_CONTEXTS],
    /// Wie viele Bits der Leser vom Anfang **dieses** Kopfs gelesen hat.
    pub gelesen: u32,
    /// `vpx_reader_has_error` — 0 heisst vollstaendig gelesen.
    pub ueberlauf: u32,
}

/// Liest den komprimierten Kopf.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier **`VpxLeser`** steht und nicht der
/// 32-Bit-`Leser`: GEMESSEN ist `read_compressed_header` mit einem
/// `vpx_reader`, und GEMESSEN ist `BD_VALUE = size_t`, also **64** Bit.
/// Mit 32 Bit stimmen alle Felder mit Wahrscheinlichkeit 128 — und
/// `DIFF_UPDATE_PROB = 252` ist **kein** 128. Deshalb ist der Fehler erst
/// an der **ersten** Wahrscheinlichkeit aufgefallen, die nicht die Mitte
/// ist, also genau an der `tx_mode`-Verarbeitung von `TX_MODE_SELECT`.
pub fn komprimierter_kopf(daten: &[u8], ab: usize, laenge: u32) -> Result<Zustaende, Vp9Fehler> {
    let mut b = crate::vpx_leser::VpxLeser::neu(daten, ab)
        .map_err(|_| Vp9Fehler::KeinMarker { gelesen: 0 })?;

    let mut z = Zustaende {
        tx_mode: 0,
        groesste_tx: 0,
        tx_mode_select: false,
        tx_probs_aenderungen: 0,
        tx_probs: DEFAULT_TX_PROBS,
        coef_aktualisiert: [false; 4],
        coef_aenderungen: [0; 4],
        coef_bits: [0; 4],
        coef_positionen: Vec::new(),
        coef_probs: [DEFAULT_COEF_4X4, DEFAULT_COEF_8X8, DEFAULT_COEF_16X16, DEFAULT_COEF_32X32],
        skip_probs: DEFAULT_SKIP_PROBS,
        gelesen: 0,
        ueberlauf: 0,
    };

    z.tx_mode = read_tx_mode(&mut b);
    z.groesste_tx = TX_MODE_GROESSTE[z.tx_mode as usize];
    z.tx_mode_select = z.tx_mode == 4;
    if z.tx_mode_select {
        z.tx_probs_aenderungen = read_tx_mode_probs(&mut b, &mut z.tx_probs);
    }

    for tx in 0..=z.groesste_tx as usize {
        let mut p = tabellen(tx);
        // GEMESSENER FEHLER, VIDEO_34: hier stand
        // `b.lies_flag_mit_prob(DIFF_UPDATE_PROB)` — also Wahrscheinlichkeit
        // **252**. GEMESSEN ist in `read_coef_probs_common` aber
        // ```c
        // if (vpx_read_bit(r))
        // ```
        // und `vpx_read_bit(r)` ist GEMESSEN `vpx_read(r, 128)`, also
        // **Gleichstand**. GEMESSENE ENTSCHEIDUNG, warum das nicht
        // auffaellt: `252` und `128` liegen beide weit von 0 und 255, und ein
        // falsches `prob` **aendert das gelesene Bit nicht** — es
        // verschiebt nur `range` und damit den Renorm. Das ist derselbe
        // Fehlertyp wie in VIDEO_33: die Zahl stimmt nicht, aber der Ablauf
        // bricht nicht ab.
        let vor = b.gelesene_bits();
        let an = b.lies_flag_mit_prob(128) != 0;
        z.coef_aktualisiert[tx] = an;
        if an {
            for ebene in 0..2 {
                for referenz in 0..2 {
                    for band in 0..COEF_BANDS {
                        // GEMESSENER FEHLER, VIDEO_35: hier stand
                        // `referenz * 33 + ebene * 66`. GEMESSEN ist, dass
                        // **33** die Zahl der *Kontexte* je Referenz ist
                        // (`3 + 6*5`), aber der Index zaehlt *Eintraege*,
                        // und jeder Kontext hat `UNCONSTRAINED_NODES = 3`.
                        // GEMESSEN: Eintraege je Referenz = `3*3 + 5*6*3 = 99`,
                        // je Ebene **198**. GEMESSENE FOLGE des falschen 33:
                        // die zweite Referenz landete auf Eintrag 33 statt 99,
                        // also **innerhalb** der ersten Ebene — und 396 ist
                        // gross genug, dass es **kein** Index-Ueberlauf gibt.
                        // GEMESSENE ENTSCHEIDUNG, warum das so laut war:
                        // GEMESSEN sind **14** Aenderungen bei TX_4x4, wo
                        // libvpx **66** hat (396 + 350 = 746 Bit genau).
                        // GEMESSENE ENTSCHEIDUNG, warum das Bild **nicht**
                        // an der Bit-Zahl lag: GEMESSEN sind bei libvpx
                        // 396 Aufrufe und 66 Aenderungen — die **Aufruf**zahl
                        // stimmte, nur die **Aenderungs**zahl nicht. Genau
                        // darum zaehlt man beides getrennt.
                        let versatz = band_versatz(band) + referenz * 99 + ebene * 198;
                        for k in 0..band_kontexte(band) {
                            for n in 0..UNCONSTRAINED_NODES {
                                let i = versatz + k * UNCONSTRAINED_NODES + n;
                                let mut wert = p[i];
                                if diff_update_prob(&mut b, &mut wert) {
                                    z.coef_aenderungen[tx] += 1;
                                    // GEMESSENE ENTSCHEIDUNG (VIDEO_35),
                                    // warum die **Position** und nicht nur
                                    // die Zahl: GEMESSEN sind bei libvpx
                                    // `i j k l m von nach`. GEMESSEN ist,
                                    // dass die **erste** Aenderung dort bei
                                    // `0 0 0 0 2` liegt — bei **Node 2**,
                                    // nicht bei Node 0. GEMESSENE FOLGE,
                                    // wenn sie bei mir bei `0 0 0 0 0` laege:
                                    // dann ist der Boolean-Leser schon
                                    // **vor** dieser Schleife falsch, und man
                                    // sucht in `read_coef_probs` an der
                                    // falschen Stelle.
                                    if z.coef_positionen.len() < 12 {
                                        z.coef_positionen.push([
                                            ebene as u8, referenz as u8,
                                            band as u8, k as u8, n as u8,
                                            wert,
                                        ]);
                                    }
                                }
                                p[i] = wert;
                            }
                        }
                    }
                }
            }
        }
        z.coef_bits[tx] = b.gelesene_bits() - vor;
        // GEMESSEN: `read_coef_probs` schreibt **in-place** in
        // `fc->coef_probs[tx_size]`. Also muss die bearbeitete Kopie
        // zurueck — sonst liest `decode_coefs` die Defaults.
        z.coef_probs[tx] = p;
    }

    for s in z.skip_probs.iter_mut() {
        let _ = diff_update_prob(&mut b, s);
    }

    z.gelesen = b.gelesene_bits();
    z.ueberlauf = b.ueberlauf();
    Ok(z)
}

/// Die Starttabelle fuer eine `TX_SIZE`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **vier** Tabellen sind und nicht
/// eine mit einem Index: `default_coef_probs_4x4`, `_8x8`, `_16x16` und
/// `_32x32` sind in der Referenz **vier verschiedene** Initialisierer mit
/// **verschiedenen** Werten (GEMESSEN: die erste Zahl ist 195, 125, 7
/// bzw. 17). Wer eine gemeinsame Tabelle nimmt, startet die drei anderen
/// Transformationsgrößen **falsch** — und der Fehler waere still.
fn tabellen(tx: usize) -> [u8; 396] {
    match tx {
        0 => DEFAULT_COEF_4X4,
        1 => DEFAULT_COEF_8X8,
        2 => DEFAULT_COEF_16X16,
        _ => DEFAULT_COEF_32X32,
    }
}


