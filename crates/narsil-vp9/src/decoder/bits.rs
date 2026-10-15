//! Bit readers: the plain MSB-first reader of the uncompressed header and
//! the boolean (arithmetic) decoder of the compressed header and tiles.

/// MSB-first reader over a byte slice (`f(n)` in the specification).
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0 }
    }

    pub fn bit(&mut self) -> u32 {
        let byte = self.data.get(self.pos >> 3).copied().unwrap_or(0);
        let b = (byte >> (7 - (self.pos & 7))) & 1;
        self.pos += 1;
        b as u32
    }

    pub fn bits(&mut self, n: u32) -> u32 {
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | self.bit();
        }
        v
    }

    /// `su(n)`: n bits of magnitude, then the sign.
    pub fn signed(&mut self, n: u32) -> i32 {
        let v = self.bits(n) as i32;
        if self.bit() != 0 { -v } else { v }
    }

    /// Whole bytes consumed after aligning to a byte boundary.
    pub fn byte_pos(&self) -> usize {
        self.pos.div_ceil(8)
    }

    pub fn overrun(&self) -> bool {
        self.pos > self.data.len() * 8
    }
}

/// The boolean decoder (libvpx `vpx_reader`, 64-bit window).
pub struct BoolDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    value: u64,
    count: i32,
    range: u32,
}

const LOTS_OF_BITS: i32 = 0x4000_0000;

impl<'a> BoolDecoder<'a> {
    /// Starts a decoder and reads the marker bit (which must be 0).
    pub fn new(data: &'a [u8]) -> Result<Self, &'static str> {
        if data.is_empty() {
            return Err("empty boolean-coded partition");
        }
        let mut d = BoolDecoder { data, pos: 0, value: 0, count: -8, range: 255 };
        d.fill();
        if d.read(128) {
            return Err("invalid marker bit");
        }
        Ok(d)
    }

    #[inline]
    fn fill(&mut self) {
        let mut shift = 64 - 8 - (self.count + 8);
        while shift >= 0 {
            if self.pos >= self.data.len() {
                self.count += LOTS_OF_BITS;
                break;
            }
            self.count += 8;
            self.value |= (self.data[self.pos] as u64) << shift;
            self.pos += 1;
            shift -= 8;
        }
    }

    #[inline]
    pub fn read(&mut self, prob: u8) -> bool {
        let split = (self.range * prob as u32 + (256 - prob as u32)) >> 8;
        if self.count < 0 {
            self.fill();
        }
        let bigsplit = (split as u64) << 56;
        let bit = if self.value >= bigsplit {
            self.range -= split;
            self.value -= bigsplit;
            true
        } else {
            self.range = split;
            false
        };
        let shift = self.range.leading_zeros().saturating_sub(24);
        self.range <<= shift;
        self.value <<= shift;
        self.count -= shift as i32;
        bit
    }

    #[inline]
    pub fn bit(&mut self) -> u32 {
        self.read(128) as u32
    }

    /// `L(n)`: n equiprobable bits, MSB first.
    pub fn literal(&mut self, n: u32) -> u32 {
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | self.bit();
        }
        v
    }

    /// Reads a tree-coded value (libvpx `vpx_read_tree`).
    #[inline]
    pub fn tree(&mut self, tree: &[i8], probs: &[u8]) -> u8 {
        let mut i = 0i32;
        loop {
            i = tree[(i + self.read(probs[(i >> 1) as usize]) as i32) as usize] as i32;
            if i <= 0 {
                return (-i) as u8;
            }
        }
    }

    /// Bits consumed beyond the data (a corrupt partition).
    pub fn overrun(&self) -> bool {
        self.count > 64 && self.count < LOTS_OF_BITS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_bits_round_trip_through_an_ideal_encoder() {
        // A minimal boolean encoder (libvpx vpx_writer) to round-trip values.
        struct Enc {
            low: u32,
            range: u32,
            count: i32,
            out: Vec<u8>,
        }
        impl Enc {
            fn put(&mut self, bit: bool, prob: u8) {
                let split = 1 + (((self.range - 1) * prob as u32) >> 8);
                let mut range = split;
                let mut low = self.low;
                if bit {
                    low += split;
                    range = self.range - split;
                }
                let mut shift = range.leading_zeros() as i32 - 24;
                range <<= shift;
                self.count += shift;
                if self.count >= 0 {
                    let offset = shift - self.count;
                    if (low << (offset - 1)) & 0x8000_0000 != 0 {
                        let mut x = self.out.len() as i32 - 1;
                        while x >= 0 && self.out[x as usize] == 0xff {
                            self.out[x as usize] = 0;
                            x -= 1;
                        }
                        self.out[x as usize] += 1;
                    }
                    self.out.push((low >> (24 - offset)) as u8);
                    low <<= offset;
                    shift = self.count;
                    low &= 0xff_ffff;
                    self.count -= 8;
                }
                low <<= shift;
                self.low = low;
                self.range = range;
            }
        }
        let mut e = Enc { low: 0, range: 255, count: -24, out: Vec::new() };
        e.put(false, 128);
        let vals: Vec<(bool, u8)> = (0..200).map(|i| ((i * 7 % 3) == 0, (i * 37 % 254 + 1) as u8)).collect();
        for &(b, p) in &vals {
            e.put(b, p);
        }
        for _ in 0..32 {
            e.put(false, 128);
        }
        let mut d = BoolDecoder::new(&e.out).expect("marker");
        for &(b, p) in &vals {
            assert_eq!(d.read(p), b);
        }
    }
}
