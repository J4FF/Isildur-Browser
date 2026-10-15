//! JPEG WRITING (Release 15.10.2026): baseline sequential DCT (ITU T.81),
//! 4:2:0 chroma subsampling, the example Huffman tables of Annex K and the
//! IJG quality scaling of the Annex K quantization tables.
//!
//! Used by `canvas.toDataURL('image/jpeg', quality)` / `toBlob`. Alpha is
//! composited onto opaque black first, as HTML requires for formats without
//! an alpha channel.

use crate::Bild;

const LUMA_Q: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51, 87, 80, 62, 18, 22,
    37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const CHROMA_Q: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57,
    50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

// Annex K.3 Huffman tables: code counts per length 1..16 and values.
const DC_LUMA_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_LUMA_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const DC_CHROMA_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_CHROMA_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_LUMA_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_LUMA_VALS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91,
    0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a,
    0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53,
    0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79,
    0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9,
    0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2,
    0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];
const AC_CHROMA_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_CHROMA_VALS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14,
    0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17,
    0x18, 0x19, 0x1a, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a,
    0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78,
    0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3,
    0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
    0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2,
    0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];

/// Canonical Huffman codes `(code, length)` indexed by symbol.
fn huffman_codes(bits: &[u8; 16], vals: &[u8]) -> [(u16, u8); 256] {
    let mut table = [(0u16, 0u8); 256];
    let mut code = 0u16;
    let mut k = 0;
    for (len, &count) in bits.iter().enumerate() {
        for _ in 0..count {
            table[vals[k] as usize] = (code, len as u8 + 1);
            code += 1;
            k += 1;
        }
        code <<= 1;
    }
    table
}

struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl BitWriter {
    fn put(&mut self, code: u16, len: u8) {
        if len == 0 {
            return;
        }
        self.acc = (self.acc << len) | u32::from(code) & ((1u32 << len) - 1);
        self.n += u32::from(len);
        while self.n >= 8 {
            let byte = (self.acc >> (self.n - 8)) as u8;
            self.out.push(byte);
            if byte == 0xFF {
                self.out.push(0);
            }
            self.n -= 8;
        }
        self.acc &= (1u32 << self.n) - 1;
    }

    fn flush(&mut self) {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put(((1u32 << pad) - 1) as u16, pad as u8);
        }
    }
}

fn scaled_table(base: &[u16; 64], quality: u8) -> [u8; 64] {
    let q = u32::from(quality.clamp(1, 100));
    let scale = if q < 50 { 5000 / q } else { 200 - q * 2 };
    let mut t = [0u8; 64];
    for i in 0..64 {
        t[i] = ((u32::from(base[i]) * scale + 50) / 100).clamp(1, 255) as u8;
    }
    t
}

/// Forward 8x8 DCT (separable, double precision) of samples centered on 0.
fn fdct(block: &[f32; 64]) -> [f32; 64] {
    let mut cos = [[0.0f32; 8]; 8];
    for (u, row) in cos.iter_mut().enumerate() {
        for (x, c) in row.iter_mut().enumerate() {
            *c = (((2 * x + 1) as f32 * u as f32 * std::f32::consts::PI) / 16.0).cos();
        }
    }
    let mut tmp = [0.0f32; 64];
    for y in 0..8 {
        for u in 0..8 {
            let mut s = 0.0;
            for x in 0..8 {
                s += block[y * 8 + x] * cos[u][x];
            }
            tmp[y * 8 + u] = s * if u == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 } / 2.0;
        }
    }
    let mut out = [0.0f32; 64];
    for u in 0..8 {
        for v in 0..8 {
            let mut s = 0.0;
            for y in 0..8 {
                s += tmp[y * 8 + u] * cos[v][y];
            }
            out[v * 8 + u] = s * if v == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 } / 2.0;
        }
    }
    out
}

fn bit_size(v: i32) -> u8 {
    (32 - v.unsigned_abs().leading_zeros()) as u8
}

fn encode_block(w: &mut BitWriter, block: &[f32; 64], q: &[u8; 64], prev_dc: &mut i32, dc: &[(u16, u8); 256], ac: &[(u16, u8); 256]) {
    let coef = fdct(block);
    let mut zz = [0i32; 64];
    for (i, &z) in ZIGZAG.iter().enumerate() {
        zz[i] = (coef[z] / f32::from(q[i])).round() as i32;
    }
    let diff = zz[0] - *prev_dc;
    *prev_dc = zz[0];
    let size = bit_size(diff);
    let (c, l) = dc[size as usize];
    w.put(c, l);
    if size > 0 {
        let bits = if diff < 0 { diff - 1 } else { diff };
        w.put((bits as u32 & ((1u32 << size) - 1)) as u16, size);
    }
    let mut run = 0;
    for &v in &zz[1..] {
        if v == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            let (c, l) = ac[0xF0];
            w.put(c, l);
            run -= 16;
        }
        let size = bit_size(v);
        let (c, l) = ac[(run << 4 | size) as usize];
        w.put(c, l);
        let bits = if v < 0 { v - 1 } else { v };
        w.put((bits as u32 & ((1u32 << size) - 1)) as u16, size);
        run = 0;
    }
    if run > 0 {
        let (c, l) = ac[0x00];
        w.put(c, l);
    }
}

fn segment(out: &mut Vec<u8>, marker: u8, body: &[u8]) {
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(body);
}

fn dht(out: &mut Vec<u8>, class_id: u8, bits: &[u8; 16], vals: &[u8]) {
    let mut body = vec![class_id];
    body.extend_from_slice(bits);
    body.extend_from_slice(vals);
    segment(out, 0xC4, &body);
}

/// Encodes `bild` as a baseline JPEG with `quality` (1..=100).
#[must_use]
pub fn jpeg_schreiben(bild: &Bild, quality: u8) -> Vec<u8> {
    let (w, h) = (bild.breite as usize, bild.hoehe as usize);
    let mut out = vec![0xFF, 0xD8];
    segment(&mut out, 0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
    let ql = scaled_table(&LUMA_Q, quality);
    let qc = scaled_table(&CHROMA_Q, quality);
    for (id, t) in [(0u8, &ql), (1u8, &qc)] {
        let mut body = vec![id];
        body.extend_from_slice(t);
        segment(&mut out, 0xDB, &body);
    }
    let mut sof = vec![8];
    sof.extend_from_slice(&(h as u16).to_be_bytes());
    sof.extend_from_slice(&(w as u16).to_be_bytes());
    sof.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    segment(&mut out, 0xC0, &sof);
    dht(&mut out, 0x00, &DC_LUMA_BITS, &DC_LUMA_VALS);
    dht(&mut out, 0x10, &AC_LUMA_BITS, &AC_LUMA_VALS);
    dht(&mut out, 0x01, &DC_CHROMA_BITS, &DC_CHROMA_VALS);
    dht(&mut out, 0x11, &AC_CHROMA_BITS, &AC_CHROMA_VALS);
    segment(&mut out, 0xDA, &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]);
    let dcl = huffman_codes(&DC_LUMA_BITS, &DC_LUMA_VALS);
    let acl = huffman_codes(&AC_LUMA_BITS, &AC_LUMA_VALS);
    let dcc = huffman_codes(&DC_CHROMA_BITS, &DC_CHROMA_VALS);
    let acc = huffman_codes(&AC_CHROMA_BITS, &AC_CHROMA_VALS);
    // Color conversion (alpha over black), edge pixels replicated.
    let ycc = |x: usize, y: usize| -> [f32; 3] {
        let (x, y) = (x.min(w.saturating_sub(1)), y.min(h.saturating_sub(1)));
        let i = (y * w + x) * 4;
        let p = bild.rgba.get(i..i + 4).unwrap_or(&[0, 0, 0, 0]);
        let a = f32::from(p[3]) / 255.0;
        let (r, g, b) = (f32::from(p[0]) * a, f32::from(p[1]) * a, f32::from(p[2]) * a);
        [
            0.299 * r + 0.587 * g + 0.114 * b,
            -0.168_736 * r - 0.331_264 * g + 0.5 * b + 128.0,
            0.5 * r - 0.418_688 * g - 0.081_312 * b + 128.0,
        ]
    };
    let mut bw = BitWriter { out: Vec::new(), acc: 0, n: 0 };
    let (mut py, mut pcb, mut pcr) = (0i32, 0i32, 0i32);
    for my in (0..h.max(1)).step_by(16) {
        for mx in (0..w.max(1)).step_by(16) {
            for by in 0..2 {
                for bx in 0..2 {
                    let mut blk = [0.0f32; 64];
                    for y in 0..8 {
                        for x in 0..8 {
                            blk[y * 8 + x] = ycc(mx + bx * 8 + x, my + by * 8 + y)[0] - 128.0;
                        }
                    }
                    encode_block(&mut bw, &blk, &ql, &mut py, &dcl, &acl);
                }
            }
            for (k, prev) in [(1usize, &mut pcb), (2usize, &mut pcr)] {
                let mut blk = [0.0f32; 64];
                for y in 0..8 {
                    for x in 0..8 {
                        let mut s = 0.0;
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            s += ycc(mx + x * 2 + dx, my + y * 2 + dy)[k];
                        }
                        blk[y * 8 + x] = s / 4.0 - 128.0;
                    }
                }
                encode_block(&mut bw, &blk, &qc, prev, &dcc, &acc);
            }
        }
    }
    bw.flush();
    out.extend_from_slice(&bw.out);
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_decoder() {
        let mut bild = Bild::new(20, 12);
        for y in 0..12 {
            for x in 0..20 {
                let i = (y * 20 + x) * 4;
                let c = if x < 10 { [255, 0, 0, 255] } else { [0, 0, 255, 255] };
                bild.rgba[i..i + 4].copy_from_slice(&c);
            }
        }
        let data = jpeg_schreiben(&bild, 90);
        let back = crate::jpeg_lesen(&data).expect("decodes");
        assert_eq!((back.breite, back.hoehe), (20, 12));
        let px = &back.rgba[(6 * 20 + 2) * 4..(6 * 20 + 2) * 4 + 4];
        assert!(px[0] > 230 && px[1] < 30 && px[2] < 30, "{px:?}");
        let px = &back.rgba[(6 * 20 + 17) * 4..(6 * 20 + 17) * 4 + 4];
        assert!(px[2] > 230 && px[0] < 30, "{px:?}");
    }
}
