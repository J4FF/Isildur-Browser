//! One decoder for the video packets of a WebM track (2026-10-15): VP8
//! through `narsil_image::vp8`, VP9 through `narsil_vp9::decoder`.
//!
//! The codec is taken from the first key frame after a reset, so callers
//! that only have packets (the player, the window) need no codec field:
//! a VP8 key frame carries the start code `9d 01 2a` at byte 3, a VP9
//! frame starts with the frame marker `0b10`.

use narsil_image::Bild;
use narsil_image::vp8::{Bildstrom, vp8_ebenen_im_strom, vp8_lesen_aus_strom};

/// What a packet gave.
pub enum Decoded {
    /// A picture to show.
    Frame(Bild),
    /// Decoded, but nothing to show (a hidden VP9 frame).
    Hidden,
}

enum State {
    /// Nothing decoded since the last reset.
    Empty,
    Vp8(Box<Bildstrom>),
    Vp9(Box<narsil_vp9::decoder::Decoder>),
}

/// The decoding state of one video track.
pub struct FrameDecoder {
    state: State,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// A VP8 key frame: key frame bit clear and the start code at byte 3.
pub fn is_vp8_key_frame(data: &[u8]) -> bool {
    data.len() >= 10 && data[0] & 1 == 0 && data[3..6] == [0x9d, 0x01, 0x2a]
}

/// A VP9 frame: the two-bit frame marker `0b10`.
pub fn is_vp9_frame(data: &[u8]) -> bool {
    data.first().is_some_and(|b| b >> 6 == 2)
}

impl FrameDecoder {
    pub fn new() -> Self {
        FrameDecoder { state: State::Empty }
    }

    /// Forgets all references; the next packet must be a key frame.
    pub fn reset(&mut self) {
        self.state = State::Empty;
    }

    /// Decodes one packet.
    pub fn decode(&mut self, data: &[u8]) -> Result<Decoded, String> {
        if matches!(self.state, State::Empty) {
            if is_vp8_key_frame(data) {
                self.state = State::Vp8(Box::new(Bildstrom::neu()));
            } else if is_vp9_frame(data) {
                self.state = State::Vp9(Box::new(narsil_vp9::decoder::Decoder::new()));
            } else {
                return Err("no key frame of a known codec".into());
            }
        }
        match &mut self.state {
            State::Empty => unreachable!(),
            State::Vp8(strom) => {
                let e = vp8_ebenen_im_strom(data, strom).map_err(|e| e.to_string())?;
                vp8_lesen_aus_strom(&e).map(Decoded::Frame).map_err(|e| e.to_string())
            }
            State::Vp9(d) => {
                let frames = d.decode(data)?;
                Ok(match frames.last() {
                    Some(f) => Decoded::Frame(Bild { breite: f.width, hoehe: f.height, rgba: f.to_rgba(&d.color) }),
                    None => Decoded::Hidden,
                })
            }
        }
    }
}
