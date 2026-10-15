//! Compression Streams (https://compression.spec.whatwg.org/): the Rust
//! half. `web_compression.js` builds CompressionStream/DecompressionStream
//! on the preamble's TransformStream and calls these two natives with the
//! collected input; the codecs are narsil-compress (DEFLATE, zlib, gzip,
//! Brotli decoding).

use crate::heap::GVal;
use crate::interp::{Engine, Stoerung};
use crate::web_platform::PlatformOp;

const GLUE: &str = include_str!("web_compression.js");

/// Output limit of one decompression (the engine's other decoders use the
/// same 64 MiB).
const MAX_OUTPUT: usize = narsil_compress::STANDARD_GRENZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompressionOp {
    Compress,
    Decompress,
}

const NATIVES: &[(&str, CompressionOp, u32)] = &[
    ("compress", CompressionOp::Compress, 2),
    ("decompress", CompressionOp::Decompress, 3),
];

/// Result of one decompression attempt over all input so far.
enum Decoded {
    /// The stream is complete and nothing follows it.
    Done(Vec<u8>),
    /// The stream is complete, but more bytes follow it (an error after
    /// the output).
    Trailing(Vec<u8>),
    /// More input is needed.
    Incomplete,
}

fn incomplete(e: &narsil_compress::FehlerZlib) -> bool {
    matches!(e, narsil_compress::FehlerZlib::DatenZuEnde { .. })
}

fn raw(data: &[u8], out: &mut Vec<u8>) -> Result<Option<usize>, &'static str> {
    match narsil_compress::deflate_entpacke_laenge(data, out, MAX_OUTPUT) {
        Ok(n) => Ok(Some(n)),
        Err(e) if incomplete(&e) => Ok(None),
        Err(_) => Err("invalid DEFLATE data"),
    }
}

fn finish(out: Vec<u8>, used: usize, total: usize) -> Decoded {
    if used < total { Decoded::Trailing(out) } else { Decoded::Done(out) }
}

/// Brotli (RFC 7932) with uncompressed meta-blocks only: a valid stream
/// every decoder reads, without a compressor (the engine has a Brotli
/// decoder but no encoder yet).
fn brotli_stored(data: &[u8]) -> Vec<u8> {
    struct Bits {
        out: Vec<u8>,
        acc: u64,
        n: u32,
    }
    impl Bits {
        fn put(&mut self, v: u64, bits: u32) {
            self.acc |= v << self.n;
            self.n += bits;
            while self.n >= 8 {
                self.out.push(self.acc as u8);
                self.acc >>= 8;
                self.n -= 8;
            }
        }
        fn align(&mut self) {
            if self.n > 0 {
                self.out.push(self.acc as u8);
                self.acc = 0;
                self.n = 0;
            }
        }
    }
    let mut b = Bits { out: Vec::with_capacity(data.len() + 16), acc: 0, n: 0 };
    b.put(0, 1); // WBITS = 16
    for chunk in data.chunks(1 << 16) {
        b.put(0, 1); // ISLAST = 0
        let mlen = chunk.len() as u64 - 1;
        b.put(0, 2); // MNIBBLES = 4 (up to 65536 bytes)
        b.put(mlen, 16);
        b.put(1, 1); // ISUNCOMPRESSED
        b.align();
        b.out.extend_from_slice(chunk);
    }
    b.put(1, 1); // ISLAST
    b.put(1, 1); // ISLASTEMPTY
    b.align();
    b.out
}

fn decode(format: &str, data: &[u8], last: bool) -> Result<Decoded, &'static str> {
    let mut out = Vec::new();
    match format {
        "deflate-raw" => match raw(data, &mut out)? {
            Some(n) => Ok(finish(out, n, data.len())),
            None => Ok(Decoded::Incomplete),
        },
        "deflate" => {
            if data.len() < 2 {
                return Ok(Decoded::Incomplete);
            }
            let (cmf, flg) = (data[0], data[1]);
            if cmf & 0x0f != 8 || cmf >> 4 > 7 || ((cmf as u32) << 8 | flg as u32) % 31 != 0 || flg & 0x20 != 0 {
                return Err("invalid zlib header");
            }
            let Some(n) = raw(&data[2..], &mut out)? else { return Ok(Decoded::Incomplete) };
            let end = 2 + n;
            if data.len() < end + 4 {
                return Ok(Decoded::Incomplete);
            }
            let want = u32::from_be_bytes([data[end], data[end + 1], data[end + 2], data[end + 3]]);
            if narsil_compress::adler32(&out) != want {
                return Err("zlib checksum mismatch");
            }
            Ok(finish(out, end + 4, data.len()))
        }
        "gzip" => {
            if data.len() < 10 {
                return Ok(Decoded::Incomplete);
            }
            if data[0] != 0x1f || data[1] != 0x8b || data[2] != 8 {
                return Err("invalid gzip header");
            }
            let flags = data[3];
            if flags & 0xe0 != 0 {
                return Err("invalid gzip flags");
            }
            let mut pos = 10;
            if flags & 0x04 != 0 {
                if data.len() < pos + 2 {
                    return Ok(Decoded::Incomplete);
                }
                pos += 2 + u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            }
            for flag in [0x08u8, 0x10] {
                if flags & flag != 0 {
                    while pos < data.len() && data[pos] != 0 {
                        pos += 1;
                    }
                    pos += 1;
                }
            }
            if flags & 0x02 != 0 {
                pos += 2;
            }
            if pos > data.len() {
                return Ok(Decoded::Incomplete);
            }
            let Some(n) = raw(&data[pos..], &mut out)? else { return Ok(Decoded::Incomplete) };
            let end = pos + n;
            if data.len() < end + 8 {
                return Ok(Decoded::Incomplete);
            }
            let crc = u32::from_le_bytes([data[end], data[end + 1], data[end + 2], data[end + 3]]);
            let size = u32::from_le_bytes([data[end + 4], data[end + 5], data[end + 6], data[end + 7]]);
            if narsil_compress::crc32(&out) != crc || size != out.len() as u32 {
                return Err("gzip checksum mismatch");
            }
            Ok(finish(out, end + 8, data.len()))
        }
        // The Brotli decoder does not tell "truncated" from "invalid":
        // before the end of the input every failure means "wait for more".
        "brotli" => match narsil_compress::brotli_entpacken(data, &mut out, MAX_OUTPUT) {
            Ok(()) => {
                // Trailing data: the decoder stops at ISLAST without saying
                // where. The last byte of a Brotli stream holds its final
                // bits, so if the input without its last byte still decodes,
                // something follows the stream.
                let mut probe = Vec::new();
                let shorter = data.len() > 1
                    && narsil_compress::brotli_entpacken(&data[..data.len() - 1], &mut probe, MAX_OUTPUT).is_ok();
                Ok(if shorter { Decoded::Trailing(out) } else { Decoded::Done(out) })
            }
            Err(e) if incomplete(&e) || !last => Ok(Decoded::Incomplete),
            Err(_) => Err("invalid Brotli data"),
        },
        _ => Err("unknown format"),
    }
}

fn encode(format: &str, data: &[u8]) -> Result<Vec<u8>, &'static str> {
    Ok(match format {
        "deflate-raw" => narsil_compress::deflate(data),
        "deflate" => {
            // zlib frame (RFC 1950): CMF/FLG 0x78 0x9c, DEFLATE, Adler-32.
            let mut out = vec![0x78, 0x9c];
            out.extend(narsil_compress::deflate(data));
            out.extend(narsil_compress::adler32(data).to_be_bytes());
            out
        }
        "gzip" => narsil_compress::gzip_komprimiere(data),
        "brotli" => brotli_stored(data),
        _ => return Err("unsupported format"),
    })
}

impl Engine {
    pub(crate) fn compression_glue_laden(&mut self) -> Result<(), Stoerung> {
        let list: Vec<(&str, PlatformOp, u32)> =
            NATIVES.iter().map(|(n, o, l)| (*n, PlatformOp::Compression(*o), *l)).collect();
        let natives = self.platform_natives(&list)?;
        self.platform_run_glue(GLUE, natives)
    }

    /// `compress(format, bytes)` -> ArrayBuffer;
    /// `decompress(format, bytes)` -> `[state, ArrayBuffer]` with state 0 =
    /// complete, 1 = more input needed, 2 = complete with trailing data.
    /// Invalid data throws a TypeError (the spec's error for both streams).
    pub(crate) fn compression_native(&mut self, op: CompressionOp, args: &[GVal]) -> Result<GVal, Stoerung> {
        let format = self.zu_text(self.arg(args, 0))?;
        let data = self.buffer_source_bytes(self.arg(args, 1))?;
        match op {
            CompressionOp::Compress => match encode(&format, &data) {
                Ok(b) => self.new_array_buffer(b),
                Err(e) => Err(self.type_error(e)),
            },
            CompressionOp::Decompress => {
                let last = self.zu_bool(self.arg(args, 2));
                let (state, bytes) = match decode(&format, &data, last) {
                    Ok(Decoded::Done(b)) => (0, b),
                    Ok(Decoded::Incomplete) => (1, Vec::new()),
                    Ok(Decoded::Trailing(b)) => (2, b),
                    Err(e) => return Err(self.type_error(e)),
                };
                let marke = self.wurzel_merke();
                let b = self.new_array_buffer(bytes)?;
                self.wurzeln.push(b);
                let r = self.array_of(&[GVal::Zahl(state as f64), b]);
                self.wurzel_zurueck(marke);
                r
            }
        }
    }
}
