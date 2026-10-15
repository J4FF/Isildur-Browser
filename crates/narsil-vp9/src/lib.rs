//! VP9 — der Decoder, **in Arbeit**, in einem eigenen Crate.
//!
//! VIDEO_25, 04.10.2026 (Anordnung der Leitung): „Neue grosse Dekoder
//! gehoeren NICHT in ein von allen genutztes Crate, solange sie in Arbeit
//! sind." GEMESSENER GRUND: `narsil-image` wird von `narsil-layout-engine`,
//! `narsil-page`, `narsil-raster`, `narsil-probe` und `narsil` benutzt — ein
//! Dekoder, der in Arbeit ist, macht jeden Testlauf in **allen** diesen
//! Crates rot, und nicht an seinem eigenen Fehler.
//!
//! | Teil | Stand |
//! |---|---|
//! | Bit-Leser (`f(n)`, MSB zuerst) | gebaut, an der echten Datei gemessen |
//! | Uncompressed-Header (Profil 0, Schluesselbild) | gebaut, **69 Bit** nachgerechnet, `ffprobe` stimmt |
//! | Boolean-Decoder | der **vorhandene** `narsil-image` (RFC 6386 §7.3) |
//! | `base_q_idx` | **9** bei Byte 9 (VIDEO_24) |
//! | Komprimierter Kopf | gebaut, **Selbsttest steht aus** |
//! | Bildsignal (tile, coefficient, mode) | **nicht gebaut** |
//! | Intra / Inter / Compound / Loopfilter | **nicht gebaut** |

pub mod vp9;
pub mod dprob;
pub mod vp9_bool;
pub mod vpx_leser;
pub mod tile;
pub mod scan;
pub mod tabellen;
pub mod dequant;
pub mod coefband;
pub mod pareto;
pub mod coefs;
pub mod modusinfo;
/// The decoder (2026-10-15): headers, modes, motion vectors, tokens,
/// prediction, transforms and the loop filter.
pub mod decoder;

pub use vp9_bool::Vp9Fehler;
