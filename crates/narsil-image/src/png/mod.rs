//! PNG nach RFC 2083 — Container, Leser und Enkoder.
//!
//! Die Datei ist eine Kette von Chunks:
//!
//! ```text
//! Signatur 89 50 4E 47 0D 0A 1A 0A            8 Byte
//! je Chunk: Laenge(4) Typ(4) Daten(Laenge) CRC(4)
//! ```
//!
//! `CRC` laeuft ueber TYP und DATEN, nicht ueber die Laenge — das ist die
//! eine Stelle, an der fast jeder beim ersten Mal danebenliegt, und der
//! Test `chunk_crc_laeuft_ueber_typ_und_daten` rechnet es beide Varianten
//! aus.

pub mod lesen;
pub mod schreiben;

pub use lesen::png_lesen;
pub use schreiben::{png_schreiben, png_schreiben_modus, zlib_ohne_kompression};

/// Wie der Enkoder das Bild kodiert. Liegt hier, damit Aufrufer von
/// `narsil_image::png_schreiben_modus` den Typ direkt bei der Hand haben.
pub use crate::Farbmodus as Farbwahl;

/// Die acht Byte, die eine PNG-Datei eröffnen. Aus RFC 2083 Abschnitt 5.1.
pub const SIGNATUR: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Farbtyp 0: Graustufen.
pub const FARBTYP_GRAU: u8 = 0;
/// Farbtyp 2: Truecolor (RGB).
pub const FARBTYP_RGB: u8 = 2;
/// Farbtyp 3: Indexpalette.
pub const FARBTYP_PALETTE: u8 = 3;
/// Farbtyp 4: Graustufen mit Alpha.
pub const FARBTYP_GRAU_ALPHA: u8 = 4;
/// Farbtyp 6: Truecolor mit Alpha.
pub const FARBTYP_RGBA: u8 = 6;
