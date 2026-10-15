//! Inverse transforms (VP9 specification §8.7.1; the butterfly structure
//! and rounding of libvpx `vpx_dsp/inv_txfm.c`, which conforming streams
//! are encoded against): DCT and ADST 4/8/16, DCT 32, the lossless WHT.

const COSPI: [i64; 32] = [
    16384, 16364, 16305, 16207, 16069, 15893, 15679, 15426, 15137, 14811, 14449, 14053, 13623, 13160, 12665, 12140, 11585,
    11003, 10394, 9760, 9102, 8423, 7723, 7005, 6270, 5520, 4756, 3981, 3196, 2404, 1606, 804,
];
const SINPI_1_9: i64 = 5283;
const SINPI_2_9: i64 = 9929;
const SINPI_3_9: i64 = 13377;
const SINPI_4_9: i64 = 15212;

#[inline(always)]
fn c(k: usize) -> i64 {
    COSPI[k]
}

#[inline(always)]
fn rnd(x: i64) -> i32 {
    ((x + (1 << 13)) >> 14) as i32
}

pub fn idct4(i: &[i32], o: &mut [i32]) {
    let (i0, i1, i2, i3) = (i[0] as i64, i[1] as i64, i[2] as i64, i[3] as i64);
    let s0 = rnd((i0 + i2) * c(16));
    let s1 = rnd((i0 - i2) * c(16));
    let s2 = rnd(i1 * c(24) - i3 * c(8));
    let s3 = rnd(i1 * c(8) + i3 * c(24));
    o[0] = s0 + s3;
    o[1] = s1 + s2;
    o[2] = s1 - s2;
    o[3] = s0 - s3;
}

pub fn iadst4(i: &[i32], o: &mut [i32]) {
    let (x0, x1, x2, x3) = (i[0] as i64, i[1] as i64, i[2] as i64, i[3] as i64);
    if x0 | x1 | x2 | x3 == 0 {
        o[..4].fill(0);
        return;
    }
    let mut s0 = SINPI_1_9 * x0;
    let mut s1 = SINPI_2_9 * x0;
    let mut s2 = SINPI_3_9 * x1;
    let mut s3 = SINPI_4_9 * x2;
    let s4 = SINPI_1_9 * x2;
    let s5 = SINPI_2_9 * x3;
    let s6 = SINPI_4_9 * x3;
    let s7 = x0 - x2 + x3;
    s0 = s0 + s3 + s5;
    s1 = s1 - s4 - s6;
    s3 = s2;
    s2 = SINPI_3_9 * s7;
    o[0] = rnd(s0 + s3);
    o[1] = rnd(s1 + s3);
    o[2] = rnd(s2);
    o[3] = rnd(s0 + s1 - s3);
}

pub fn idct8(i: &[i32], o: &mut [i32]) {
    let mut s1 = [0i32; 8];
    let mut s2 = [0i32; 8];
    let x = |k: usize| i[k] as i64;
    // stage 1
    let even = [i[0], i[2], i[4], i[6]];
    s1[4] = rnd(x(1) * c(28) - x(7) * c(4));
    s1[7] = rnd(x(1) * c(4) + x(7) * c(28));
    s1[5] = rnd(x(5) * c(12) - x(3) * c(20));
    s1[6] = rnd(x(5) * c(20) + x(3) * c(12));
    // stage 2
    let mut e = [0i32; 4];
    idct4(&even, &mut e);
    s1[..4].copy_from_slice(&e);
    s2[4] = s1[4] + s1[5];
    s2[5] = s1[4] - s1[5];
    s2[6] = -s1[6] + s1[7];
    s2[7] = s1[6] + s1[7];
    // stage 3
    let a = s2[5] as i64;
    let b = s2[6] as i64;
    s1[5] = rnd((b - a) * c(16));
    s1[6] = rnd((a + b) * c(16));
    s1[4] = s2[4];
    s1[7] = s2[7];
    // stage 4
    for k in 0..4 {
        o[k] = s1[k] + s1[7 - k];
        o[7 - k] = s1[k] - s1[7 - k];
    }
}

pub fn iadst8(inp: &[i32], o: &mut [i32]) {
    let mut x0 = inp[7] as i64;
    let mut x1 = inp[0] as i64;
    let mut x2 = inp[5] as i64;
    let mut x3 = inp[2] as i64;
    let mut x4 = inp[3] as i64;
    let mut x5 = inp[4] as i64;
    let mut x6 = inp[1] as i64;
    let mut x7 = inp[6] as i64;
    if x0 | x1 | x2 | x3 | x4 | x5 | x6 | x7 == 0 {
        o[..8].fill(0);
        return;
    }
    // stage 1
    let s0 = c(2) * x0 + c(30) * x1;
    let s1 = c(30) * x0 - c(2) * x1;
    let s2 = c(10) * x2 + c(22) * x3;
    let s3 = c(22) * x2 - c(10) * x3;
    let s4 = c(18) * x4 + c(14) * x5;
    let s5 = c(14) * x4 - c(18) * x5;
    let s6 = c(26) * x6 + c(6) * x7;
    let s7 = c(6) * x6 - c(26) * x7;
    x0 = rnd(s0 + s4) as i64;
    x1 = rnd(s1 + s5) as i64;
    x2 = rnd(s2 + s6) as i64;
    x3 = rnd(s3 + s7) as i64;
    x4 = rnd(s0 - s4) as i64;
    x5 = rnd(s1 - s5) as i64;
    x6 = rnd(s2 - s6) as i64;
    x7 = rnd(s3 - s7) as i64;
    // stage 2
    let s0 = x0;
    let s1 = x1;
    let s2 = x2;
    let s3 = x3;
    let s4 = c(8) * x4 + c(24) * x5;
    let s5 = c(24) * x4 - c(8) * x5;
    let s6 = -c(24) * x6 + c(8) * x7;
    let s7 = c(8) * x6 + c(24) * x7;
    x0 = s0 + s2;
    x1 = s1 + s3;
    x2 = s0 - s2;
    x3 = s1 - s3;
    x4 = rnd(s4 + s6) as i64;
    x5 = rnd(s5 + s7) as i64;
    x6 = rnd(s4 - s6) as i64;
    x7 = rnd(s5 - s7) as i64;
    // stage 3
    let s2 = c(16) * (x2 + x3);
    let s3 = c(16) * (x2 - x3);
    let s6 = c(16) * (x6 + x7);
    let s7 = c(16) * (x6 - x7);
    x2 = rnd(s2) as i64;
    x3 = rnd(s3) as i64;
    x6 = rnd(s6) as i64;
    x7 = rnd(s7) as i64;
    o[0] = x0 as i32;
    o[1] = -x4 as i32;
    o[2] = x6 as i32;
    o[3] = -x2 as i32;
    o[4] = x3 as i32;
    o[5] = -x7 as i32;
    o[6] = x5 as i32;
    o[7] = -x1 as i32;
}

pub fn idct16(i: &[i32], o: &mut [i32]) {
    let mut s1 = [0i32; 16];
    let mut s2 = [0i32; 16];
    // stage 1
    s1[0] = i[0];
    s1[1] = i[8];
    s1[2] = i[4];
    s1[3] = i[12];
    s1[4] = i[2];
    s1[5] = i[10];
    s1[6] = i[6];
    s1[7] = i[14];
    s1[8] = i[1];
    s1[9] = i[9];
    s1[10] = i[5];
    s1[11] = i[13];
    s1[12] = i[3];
    s1[13] = i[11];
    s1[14] = i[7];
    s1[15] = i[15];
    let v = |a: &[i32; 16], k: usize| a[k] as i64;
    // stage 2
    s2[..8].copy_from_slice(&s1[..8]);
    s2[8] = rnd(v(&s1, 8) * c(30) - v(&s1, 15) * c(2));
    s2[15] = rnd(v(&s1, 8) * c(2) + v(&s1, 15) * c(30));
    s2[9] = rnd(v(&s1, 9) * c(14) - v(&s1, 14) * c(18));
    s2[14] = rnd(v(&s1, 9) * c(18) + v(&s1, 14) * c(14));
    s2[10] = rnd(v(&s1, 10) * c(22) - v(&s1, 13) * c(10));
    s2[13] = rnd(v(&s1, 10) * c(10) + v(&s1, 13) * c(22));
    s2[11] = rnd(v(&s1, 11) * c(6) - v(&s1, 12) * c(26));
    s2[12] = rnd(v(&s1, 11) * c(26) + v(&s1, 12) * c(6));
    // stage 3
    s1[..4].copy_from_slice(&s2[..4]);
    s1[4] = rnd(v(&s2, 4) * c(28) - v(&s2, 7) * c(4));
    s1[7] = rnd(v(&s2, 4) * c(4) + v(&s2, 7) * c(28));
    s1[5] = rnd(v(&s2, 5) * c(12) - v(&s2, 6) * c(20));
    s1[6] = rnd(v(&s2, 5) * c(20) + v(&s2, 6) * c(12));
    s1[8] = s2[8] + s2[9];
    s1[9] = s2[8] - s2[9];
    s1[10] = -s2[10] + s2[11];
    s1[11] = s2[10] + s2[11];
    s1[12] = s2[12] + s2[13];
    s1[13] = s2[12] - s2[13];
    s1[14] = -s2[14] + s2[15];
    s1[15] = s2[14] + s2[15];
    // stage 4
    s2[0] = rnd((v(&s1, 0) + v(&s1, 1)) * c(16));
    s2[1] = rnd((v(&s1, 0) - v(&s1, 1)) * c(16));
    s2[2] = rnd(v(&s1, 2) * c(24) - v(&s1, 3) * c(8));
    s2[3] = rnd(v(&s1, 2) * c(8) + v(&s1, 3) * c(24));
    s2[4] = s1[4] + s1[5];
    s2[5] = s1[4] - s1[5];
    s2[6] = -s1[6] + s1[7];
    s2[7] = s1[6] + s1[7];
    s2[8] = s1[8];
    s2[15] = s1[15];
    s2[9] = rnd(-v(&s1, 9) * c(8) + v(&s1, 14) * c(24));
    s2[14] = rnd(v(&s1, 9) * c(24) + v(&s1, 14) * c(8));
    s2[10] = rnd(-v(&s1, 10) * c(24) - v(&s1, 13) * c(8));
    s2[13] = rnd(-v(&s1, 10) * c(8) + v(&s1, 13) * c(24));
    s2[11] = s1[11];
    s2[12] = s1[12];
    // stage 5
    s1[0] = s2[0] + s2[3];
    s1[1] = s2[1] + s2[2];
    s1[2] = s2[1] - s2[2];
    s1[3] = s2[0] - s2[3];
    s1[4] = s2[4];
    s1[5] = rnd((v(&s2, 6) - v(&s2, 5)) * c(16));
    s1[6] = rnd((v(&s2, 5) + v(&s2, 6)) * c(16));
    s1[7] = s2[7];
    s1[8] = s2[8] + s2[11];
    s1[9] = s2[9] + s2[10];
    s1[10] = s2[9] - s2[10];
    s1[11] = s2[8] - s2[11];
    s1[12] = -s2[12] + s2[15];
    s1[13] = -s2[13] + s2[14];
    s1[14] = s2[13] + s2[14];
    s1[15] = s2[12] + s2[15];
    // stage 6
    for k in 0..4 {
        s2[k] = s1[k] + s1[7 - k];
        s2[7 - k] = s1[k] - s1[7 - k];
    }
    s2[8] = s1[8];
    s2[9] = s1[9];
    s2[10] = rnd((-v(&s1, 10) + v(&s1, 13)) * c(16));
    s2[13] = rnd((v(&s1, 10) + v(&s1, 13)) * c(16));
    s2[11] = rnd((-v(&s1, 11) + v(&s1, 12)) * c(16));
    s2[12] = rnd((v(&s1, 11) + v(&s1, 12)) * c(16));
    s2[14] = s1[14];
    s2[15] = s1[15];
    // stage 7
    for k in 0..8 {
        o[k] = s2[k] + s2[15 - k];
        o[15 - k] = s2[k] - s2[15 - k];
    }
}

pub fn iadst16(inp: &[i32], o: &mut [i32]) {
    let mut x = [0i64; 16];
    let order = [15, 0, 13, 2, 11, 4, 9, 6, 7, 8, 5, 10, 3, 12, 1, 14];
    let mut any = 0;
    for k in 0..16 {
        x[k] = inp[order[k]] as i64;
        any |= x[k];
    }
    if any == 0 {
        o[..16].fill(0);
        return;
    }
    let mut s = [0i64; 16];
    // stage 1
    s[0] = x[0] * c(1) + x[1] * c(31);
    s[1] = x[0] * c(31) - x[1] * c(1);
    s[2] = x[2] * c(5) + x[3] * c(27);
    s[3] = x[2] * c(27) - x[3] * c(5);
    s[4] = x[4] * c(9) + x[5] * c(23);
    s[5] = x[4] * c(23) - x[5] * c(9);
    s[6] = x[6] * c(13) + x[7] * c(19);
    s[7] = x[6] * c(19) - x[7] * c(13);
    s[8] = x[8] * c(17) + x[9] * c(15);
    s[9] = x[8] * c(15) - x[9] * c(17);
    s[10] = x[10] * c(21) + x[11] * c(11);
    s[11] = x[10] * c(11) - x[11] * c(21);
    s[12] = x[12] * c(25) + x[13] * c(7);
    s[13] = x[12] * c(7) - x[13] * c(25);
    s[14] = x[14] * c(29) + x[15] * c(3);
    s[15] = x[14] * c(3) - x[15] * c(29);
    for k in 0..8 {
        x[k] = rnd(s[k] + s[k + 8]) as i64;
        x[k + 8] = rnd(s[k] - s[k + 8]) as i64;
    }
    // stage 2
    s[..8].copy_from_slice(&x[..8]);
    s[8] = x[8] * c(4) + x[9] * c(28);
    s[9] = x[8] * c(28) - x[9] * c(4);
    s[10] = x[10] * c(20) + x[11] * c(12);
    s[11] = x[10] * c(12) - x[11] * c(20);
    s[12] = -x[12] * c(28) + x[13] * c(4);
    s[13] = x[12] * c(4) + x[13] * c(28);
    s[14] = -x[14] * c(12) + x[15] * c(20);
    s[15] = x[14] * c(20) + x[15] * c(12);
    for k in 0..4 {
        x[k] = s[k] + s[k + 4];
        x[k + 4] = s[k] - s[k + 4];
    }
    for k in 8..12 {
        x[k] = rnd(s[k] + s[k + 4]) as i64;
        x[k + 4] = rnd(s[k] - s[k + 4]) as i64;
    }
    // stage 3
    s[0] = x[0];
    s[1] = x[1];
    s[2] = x[2];
    s[3] = x[3];
    s[4] = x[4] * c(8) + x[5] * c(24);
    s[5] = x[4] * c(24) - x[5] * c(8);
    s[6] = -x[6] * c(24) + x[7] * c(8);
    s[7] = x[6] * c(8) + x[7] * c(24);
    s[8] = x[8];
    s[9] = x[9];
    s[10] = x[10];
    s[11] = x[11];
    s[12] = x[12] * c(8) + x[13] * c(24);
    s[13] = x[12] * c(24) - x[13] * c(8);
    s[14] = -x[14] * c(24) + x[15] * c(8);
    s[15] = x[14] * c(8) + x[15] * c(24);
    x[0] = s[0] + s[2];
    x[1] = s[1] + s[3];
    x[2] = s[0] - s[2];
    x[3] = s[1] - s[3];
    x[4] = rnd(s[4] + s[6]) as i64;
    x[5] = rnd(s[5] + s[7]) as i64;
    x[6] = rnd(s[4] - s[6]) as i64;
    x[7] = rnd(s[5] - s[7]) as i64;
    x[8] = s[8] + s[10];
    x[9] = s[9] + s[11];
    x[10] = s[8] - s[10];
    x[11] = s[9] - s[11];
    x[12] = rnd(s[12] + s[14]) as i64;
    x[13] = rnd(s[13] + s[15]) as i64;
    x[14] = rnd(s[12] - s[14]) as i64;
    x[15] = rnd(s[13] - s[15]) as i64;
    // stage 4
    let s2 = -c(16) * (x[2] + x[3]);
    let s3 = c(16) * (x[2] - x[3]);
    let s6 = c(16) * (x[6] + x[7]);
    let s7 = c(16) * (-x[6] + x[7]);
    let s10 = c(16) * (x[10] + x[11]);
    let s11 = c(16) * (-x[10] + x[11]);
    let s14 = -c(16) * (x[14] + x[15]);
    let s15 = c(16) * (x[14] - x[15]);
    x[2] = rnd(s2) as i64;
    x[3] = rnd(s3) as i64;
    x[6] = rnd(s6) as i64;
    x[7] = rnd(s7) as i64;
    x[10] = rnd(s10) as i64;
    x[11] = rnd(s11) as i64;
    x[14] = rnd(s14) as i64;
    x[15] = rnd(s15) as i64;
    let out = [x[0], -x[8], x[12], -x[4], x[6], x[14], x[10], x[2], x[3], x[11], x[15], x[7], x[5], -x[13], x[9], -x[1]];
    for k in 0..16 {
        o[k] = out[k] as i32;
    }
}

pub fn idct32(inp: &[i32], o: &mut [i32]) {
    let mut s1 = [0i32; 32];
    let mut s2 = [0i32; 32];
    let x = |k: usize| inp[k] as i64;
    let v = |a: &[i32; 32], k: usize| a[k] as i64;
    // stage 1
    let even = [0, 16, 8, 24, 4, 20, 12, 28, 2, 18, 10, 26, 6, 22, 14, 30];
    for k in 0..16 {
        s1[k] = inp[even[k]];
    }
    let rot = |a: usize, b: usize, ca: usize, cb: usize| -> (i32, i32) {
        (rnd(x(a) * c(ca) - x(b) * c(cb)), rnd(x(a) * c(cb) + x(b) * c(ca)))
    };
    (s1[16], s1[31]) = rot(1, 31, 31, 1);
    (s1[17], s1[30]) = rot(17, 15, 15, 17);
    (s1[18], s1[29]) = rot(9, 23, 23, 9);
    (s1[19], s1[28]) = rot(25, 7, 7, 25);
    (s1[20], s1[27]) = rot(5, 27, 27, 5);
    (s1[21], s1[26]) = rot(21, 11, 11, 21);
    (s1[22], s1[25]) = rot(13, 19, 19, 13);
    (s1[23], s1[24]) = rot(29, 3, 3, 29);
    // stage 2
    s2[..8].copy_from_slice(&s1[..8]);
    s2[8] = rnd(v(&s1, 8) * c(30) - v(&s1, 15) * c(2));
    s2[15] = rnd(v(&s1, 8) * c(2) + v(&s1, 15) * c(30));
    s2[9] = rnd(v(&s1, 9) * c(14) - v(&s1, 14) * c(18));
    s2[14] = rnd(v(&s1, 9) * c(18) + v(&s1, 14) * c(14));
    s2[10] = rnd(v(&s1, 10) * c(22) - v(&s1, 13) * c(10));
    s2[13] = rnd(v(&s1, 10) * c(10) + v(&s1, 13) * c(22));
    s2[11] = rnd(v(&s1, 11) * c(6) - v(&s1, 12) * c(26));
    s2[12] = rnd(v(&s1, 11) * c(26) + v(&s1, 12) * c(6));
    for k in (16..32).step_by(4) {
        s2[k] = s1[k] + s1[k + 1];
        s2[k + 1] = s1[k] - s1[k + 1];
        s2[k + 2] = -s1[k + 2] + s1[k + 3];
        s2[k + 3] = s1[k + 2] + s1[k + 3];
    }
    // stage 3
    s1[..4].copy_from_slice(&s2[..4]);
    s1[4] = rnd(v(&s2, 4) * c(28) - v(&s2, 7) * c(4));
    s1[7] = rnd(v(&s2, 4) * c(4) + v(&s2, 7) * c(28));
    s1[5] = rnd(v(&s2, 5) * c(12) - v(&s2, 6) * c(20));
    s1[6] = rnd(v(&s2, 5) * c(20) + v(&s2, 6) * c(12));
    s1[8] = s2[8] + s2[9];
    s1[9] = s2[8] - s2[9];
    s1[10] = -s2[10] + s2[11];
    s1[11] = s2[10] + s2[11];
    s1[12] = s2[12] + s2[13];
    s1[13] = s2[12] - s2[13];
    s1[14] = -s2[14] + s2[15];
    s1[15] = s2[14] + s2[15];
    s1[16] = s2[16];
    s1[31] = s2[31];
    s1[17] = rnd(-v(&s2, 17) * c(4) + v(&s2, 30) * c(28));
    s1[30] = rnd(v(&s2, 17) * c(28) + v(&s2, 30) * c(4));
    s1[18] = rnd(-v(&s2, 18) * c(28) - v(&s2, 29) * c(4));
    s1[29] = rnd(-v(&s2, 18) * c(4) + v(&s2, 29) * c(28));
    s1[19] = s2[19];
    s1[20] = s2[20];
    s1[21] = rnd(-v(&s2, 21) * c(20) + v(&s2, 26) * c(12));
    s1[26] = rnd(v(&s2, 21) * c(12) + v(&s2, 26) * c(20));
    s1[22] = rnd(-v(&s2, 22) * c(12) - v(&s2, 25) * c(20));
    s1[25] = rnd(-v(&s2, 22) * c(20) + v(&s2, 25) * c(12));
    s1[23] = s2[23];
    s1[24] = s2[24];
    s1[27] = s2[27];
    s1[28] = s2[28];
    // stage 4
    s2[0] = rnd((v(&s1, 0) + v(&s1, 1)) * c(16));
    s2[1] = rnd((v(&s1, 0) - v(&s1, 1)) * c(16));
    s2[2] = rnd(v(&s1, 2) * c(24) - v(&s1, 3) * c(8));
    s2[3] = rnd(v(&s1, 2) * c(8) + v(&s1, 3) * c(24));
    s2[4] = s1[4] + s1[5];
    s2[5] = s1[4] - s1[5];
    s2[6] = -s1[6] + s1[7];
    s2[7] = s1[6] + s1[7];
    s2[8] = s1[8];
    s2[15] = s1[15];
    s2[9] = rnd(-v(&s1, 9) * c(8) + v(&s1, 14) * c(24));
    s2[14] = rnd(v(&s1, 9) * c(24) + v(&s1, 14) * c(8));
    s2[10] = rnd(-v(&s1, 10) * c(24) - v(&s1, 13) * c(8));
    s2[13] = rnd(-v(&s1, 10) * c(8) + v(&s1, 13) * c(24));
    s2[11] = s1[11];
    s2[12] = s1[12];
    s2[16] = s1[16] + s1[19];
    s2[17] = s1[17] + s1[18];
    s2[18] = s1[17] - s1[18];
    s2[19] = s1[16] - s1[19];
    s2[20] = -s1[20] + s1[23];
    s2[21] = -s1[21] + s1[22];
    s2[22] = s1[21] + s1[22];
    s2[23] = s1[20] + s1[23];
    s2[24] = s1[24] + s1[27];
    s2[25] = s1[25] + s1[26];
    s2[26] = s1[25] - s1[26];
    s2[27] = s1[24] - s1[27];
    s2[28] = -s1[28] + s1[31];
    s2[29] = -s1[29] + s1[30];
    s2[30] = s1[29] + s1[30];
    s2[31] = s1[28] + s1[31];
    // stage 5
    s1[0] = s2[0] + s2[3];
    s1[1] = s2[1] + s2[2];
    s1[2] = s2[1] - s2[2];
    s1[3] = s2[0] - s2[3];
    s1[4] = s2[4];
    s1[5] = rnd((v(&s2, 6) - v(&s2, 5)) * c(16));
    s1[6] = rnd((v(&s2, 5) + v(&s2, 6)) * c(16));
    s1[7] = s2[7];
    s1[8] = s2[8] + s2[11];
    s1[9] = s2[9] + s2[10];
    s1[10] = s2[9] - s2[10];
    s1[11] = s2[8] - s2[11];
    s1[12] = -s2[12] + s2[15];
    s1[13] = -s2[13] + s2[14];
    s1[14] = s2[13] + s2[14];
    s1[15] = s2[12] + s2[15];
    s1[16] = s2[16];
    s1[17] = s2[17];
    s1[18] = rnd(-v(&s2, 18) * c(8) + v(&s2, 29) * c(24));
    s1[29] = rnd(v(&s2, 18) * c(24) + v(&s2, 29) * c(8));
    s1[19] = rnd(-v(&s2, 19) * c(8) + v(&s2, 28) * c(24));
    s1[28] = rnd(v(&s2, 19) * c(24) + v(&s2, 28) * c(8));
    s1[20] = rnd(-v(&s2, 20) * c(24) - v(&s2, 27) * c(8));
    s1[27] = rnd(-v(&s2, 20) * c(8) + v(&s2, 27) * c(24));
    s1[21] = rnd(-v(&s2, 21) * c(24) - v(&s2, 26) * c(8));
    s1[26] = rnd(-v(&s2, 21) * c(8) + v(&s2, 26) * c(24));
    s1[22] = s2[22];
    s1[23] = s2[23];
    s1[24] = s2[24];
    s1[25] = s2[25];
    s1[30] = s2[30];
    s1[31] = s2[31];
    // stage 6
    for k in 0..4 {
        s2[k] = s1[k] + s1[7 - k];
        s2[7 - k] = s1[k] - s1[7 - k];
    }
    s2[8] = s1[8];
    s2[9] = s1[9];
    s2[10] = rnd((-v(&s1, 10) + v(&s1, 13)) * c(16));
    s2[13] = rnd((v(&s1, 10) + v(&s1, 13)) * c(16));
    s2[11] = rnd((-v(&s1, 11) + v(&s1, 12)) * c(16));
    s2[12] = rnd((v(&s1, 11) + v(&s1, 12)) * c(16));
    s2[14] = s1[14];
    s2[15] = s1[15];
    for k in 0..4 {
        s2[16 + k] = s1[16 + k] + s1[23 - k];
        s2[23 - k] = s1[16 + k] - s1[23 - k];
        s2[24 + k] = -s1[24 + k] + s1[31 - k];
        s2[31 - k] = s1[24 + k] + s1[31 - k];
    }
    // stage 7
    for k in 0..8 {
        s1[k] = s2[k] + s2[15 - k];
        s1[15 - k] = s2[k] - s2[15 - k];
    }
    s1[16] = s2[16];
    s1[17] = s2[17];
    s1[18] = s2[18];
    s1[19] = s2[19];
    for (a, b) in [(20, 27), (21, 26), (22, 25), (23, 24)] {
        s1[a] = rnd((-v(&s2, a) + v(&s2, b)) * c(16));
        s1[b] = rnd((v(&s2, a) + v(&s2, b)) * c(16));
    }
    s1[28] = s2[28];
    s1[29] = s2[29];
    s1[30] = s2[30];
    s1[31] = s2[31];
    // final stage
    for k in 0..16 {
        o[k] = s1[k] + s1[31 - k];
        o[31 - k] = s1[k] - s1[31 - k];
    }
}

#[inline(always)]
fn clip_add(d: u8, r: i32) -> u8 {
    (d as i32 + r).clamp(0, 255) as u8
}

type Tx1d = fn(&[i32], &mut [i32]);

/// Adds the inverse transform of `coefs` (raster, n x n) to `dst`.
/// `tx_type`: 0 DCT/DCT, 1 ADST/DCT (vertical ADST), 2 DCT/ADST, 3 ADST/ADST.
pub fn inverse_add(coefs: &[i32], dst: &mut [u8], off: usize, stride: usize, tx_size: u8, tx_type: u8, lossless: bool) {
    if lossless {
        iwht4x4_add(coefs, dst, off, stride);
        return;
    }
    let n = 4usize << tx_size;
    let (cols, rows): (Tx1d, Tx1d) = match (tx_size, tx_type) {
        (0, 1) => (iadst4, idct4),
        (0, 2) => (idct4, iadst4),
        (0, 3) => (iadst4, iadst4),
        (0, _) => (idct4, idct4),
        (1, 1) => (iadst8, idct8),
        (1, 2) => (idct8, iadst8),
        (1, 3) => (iadst8, iadst8),
        (1, _) => (idct8, idct8),
        (2, 1) => (iadst16, idct16),
        (2, 2) => (idct16, iadst16),
        (2, 3) => (iadst16, iadst16),
        (2, _) => (idct16, idct16),
        _ => (idct32, idct32),
    };
    let shift = match tx_size {
        0 => 4,
        1 => 5,
        _ => 6,
    };
    let mut tmp = [0i32; 1024];
    for r in 0..n {
        let row = &coefs[r * n..r * n + n];
        if row.iter().all(|v| *v == 0) {
            continue;
        }
        rows(row, &mut tmp[r * n..r * n + n]);
    }
    let mut col_in = [0i32; 32];
    let mut col_out = [0i32; 32];
    let round = 1 << (shift - 1);
    for cidx in 0..n {
        for r in 0..n {
            col_in[r] = tmp[r * n + cidx];
        }
        cols(&col_in[..n], &mut col_out[..n]);
        for r in 0..n {
            let p = off + r * stride + cidx;
            dst[p] = clip_add(dst[p], (col_out[r] + round) >> shift);
        }
    }
}

fn iwht4x4_add(input: &[i32], dst: &mut [u8], off: usize, stride: usize) {
    let mut out = [0i32; 16];
    for i in 0..4 {
        let ip = &input[i * 4..i * 4 + 4];
        let mut a1 = ip[0] >> 2;
        let mut c1 = ip[1] >> 2;
        let mut d1 = ip[2] >> 2;
        let mut b1 = ip[3] >> 2;
        a1 += c1;
        d1 -= b1;
        let e1 = (a1 - d1) >> 1;
        b1 = e1 - b1;
        c1 = e1 - c1;
        a1 -= b1;
        d1 += c1;
        out[i * 4] = a1;
        out[i * 4 + 1] = b1;
        out[i * 4 + 2] = c1;
        out[i * 4 + 3] = d1;
    }
    for i in 0..4 {
        let mut a1 = out[i];
        let mut c1 = out[4 + i];
        let mut d1 = out[8 + i];
        let mut b1 = out[12 + i];
        a1 += c1;
        d1 -= b1;
        let e1 = (a1 - d1) >> 1;
        b1 = e1 - b1;
        c1 = e1 - c1;
        a1 -= b1;
        d1 += c1;
        for (r, v) in [a1, b1, c1, d1].into_iter().enumerate() {
            let p = off + r * stride + i;
            dst[p] = clip_add(dst[p], v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(f: Tx1d, n: usize) -> Vec<Vec<f64>> {
        // Column k = transform of the k-th unit vector (scaled up).
        let mut m = vec![vec![0.0; n]; n];
        for k in 0..n {
            let mut i = vec![0i32; n];
            i[k] = 1 << 12;
            let mut o = vec![0i32; n];
            f(&i, &mut o);
            for r in 0..n {
                m[r][k] = o[r] as f64 / 4096.0;
            }
        }
        m
    }

    fn check_dct(f: Tx1d, n: usize) {
        let m = matrix(f, n);
        for k in 0..n {
            let s = if k == 0 { std::f64::consts::FRAC_1_SQRT_2 } else { 1.0 };
            for r in 0..n {
                let want = s * ((2 * r + 1) as f64 * k as f64 * std::f64::consts::PI / (2 * n) as f64).cos();
                assert!((m[r][k] - want).abs() < 0.01, "n={n} r={r} k={k}: {} vs {want}", m[r][k]);
            }
        }
    }

    fn check_orthogonal(f: Tx1d, n: usize) {
        let m = matrix(f, n);
        let mut g = vec![vec![0.0; n]; n];
        for a in 0..n {
            for b in 0..n {
                g[a][b] = (0..n).map(|r| m[r][a] * m[r][b]).sum();
            }
        }
        let d = g[0][0];
        for a in 0..n {
            for b in 0..n {
                let want = if a == b { d } else { 0.0 };
                assert!((g[a][b] - want).abs() < 0.02 * d, "n={n} ({a},{b}) {} vs {want}", g[a][b]);
            }
        }
    }

    #[test]
    fn dct_matches_the_cosine_basis() {
        check_dct(idct4, 4);
        check_dct(idct8, 8);
        check_dct(idct16, 16);
        check_dct(idct32, 32);
    }

    #[test]
    fn adst_is_orthogonal_and_sine_shaped() {
        check_orthogonal(iadst4, 4);
        check_orthogonal(iadst8, 8);
        check_orthogonal(iadst16, 16);
        // ADST 8/16: x_r = sum_k in_k sin(pi (2r+1)(2k+1) / 4n).
        for (f, n) in [(iadst8 as Tx1d, 8usize), (iadst16 as Tx1d, 16)] {
            let m = matrix(f, n);
            for k in 0..n {
                for r in 0..n {
                    let want = (std::f64::consts::PI * ((2 * r + 1) * (2 * k + 1)) as f64 / (4 * n) as f64).sin();
                    assert!((m[r][k] - want).abs() < 0.01, "n={n} r={r} k={k}: {} vs {want}", m[r][k]);
                }
            }
        }
    }
}
