//! JPEG LESEN: Baseline (ITU T.81) und progressiv.
//!
//! Aufbau einer JPEG-Datei:
//!
//! ```text
//! SOI  FFD8
//!      Segmente (APPn, DQT, SOF0/2, DHT, DRI, SOS, ...)
//! SOS  FFDA ... Entropiekodierte Daten ... FFD9
//! EOI  FFD9
//! ```
//!
//! Die entropiekodierten Daten zwischen SOS und EOI sind eine Bitfolge, die
//! in 8-Bit-Bloecke aufgeteilt ist. Ein Byte 0xFF in den Daten ist immer ein
//! Marker-Prefix: das naechste Byte sagt, was es fuer ein Marker ist.
//! 0xFF00 ist ein gestrecktes 0xFF (Stuffing), 0xFFD0-0xFFD7 sind
//! Restart-Marker.
//!
//! Diese Crate implementiert:
//!   * Baseline (SOF0): Huffman, 8-Bit-Abtastwerte, bis zu 4 Komponenten
//!   * Progressiv (SOF2): Spektralauswahl und sukzessive Approximation
//!   * Subsampling 4:4:4, 4:2:2, 4:2:0
//!   * Restart-Marker
//!   * EXIF-Orientierung lesen (nur lesen, nicht anwenden)

pub mod lesen;
pub mod schreiben;

pub use lesen::{Verkleinerung, jpeg_lesen, jpeg_lesen_mit, jpeg_masse, jpeg_orientierung};
pub use schreiben::jpeg_schreiben;

/// JPEG-Signatur: Start of Image (SOI).
pub const SIGNATUR: [u8; 2] = [0xFF, 0xD8];

/// Maximale Anzahl Komponenten in einem Frame.
pub const MAX_KOMPONENTEN: usize = 4;

/// Maximale Anzahl Huffman-Tabellen je Kategorie (DC/AC).
pub const MAX_HUFFMAN_TABELLEN: usize = 4;

/// Maximale Anzahl Quantisierungstabellen.
pub const MAX_QUANT_TABELLEN: usize = 4;

/// Maximale Laenge eines Huffman-Codes in Bits.
pub const MAX_HUFFMAN_LAENGE: usize = 16;

/// Maximale Anzahl Koeffizienten je Block (8x8 = 64).
pub const KOEFFIZIENTEN_PRO_BLOCK: usize = 64;

/// Maximale Anzahl Bloecke je Zeile (fuer Grenzpruefung).
pub const MAX_BLOECKE_PRO_ZEILE: usize = 4096;

/// Maximale Anzahl Bloecke je Spalte (fuer Grenzpruefung).
pub const MAX_BLOECKE_PRO_SPALTE: usize = 4096;

/// Maximale Anzahl Scans (fuer Grenzpruefung).
///
/// 64, weil eine progressiv gespeicherte Foto-Datei leicht 10 bis 15
/// Scans hat (DC plus mehrere Band-Spektren je Komponente) und ein
/// Enkoder jedes Band einzeln schreiben darf. Mehr als 64 Scans ist
/// kein Bild mehr, das jemand gemeint hat.
pub const MAX_SCANS: usize = 64;

/// Maximale Anzahl Segmente (fuer Grenzpruefung).
pub const MAX_SEGMENTE: usize = 1024;

/// Maximale Laenge eines Segments in Byte (fuer Grenzpruefung).
pub const MAX_SEGMENT_LAENGE: usize = 65533;

/// Maximale Anzahl Werte in einer Huffman-Tabelle.
pub const MAX_HUFFMAN_WERTE: usize = 256;

/// Maximale Anzahl Quantisierungswerte.
pub const MAX_QUANT_WERTE: usize = 64;
