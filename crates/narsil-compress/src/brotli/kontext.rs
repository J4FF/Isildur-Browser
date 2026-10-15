//! Die Kontexttabellen und -arten aus **RFC 7932 Abschnitt 7.1**.
//!
//! ERZEUGT, NICHT ABGETIPPT: `mutation_md/tls/brotli_dict_generator.py`
//! liest die drei Tabellen aus `rfc/rfc7932.txt` und prueft ihre
//! CRC-32 gegen die Tabelle `Table Length CRC-32` im selben
//! Abschnitt (Lut0 0x8e91efb7, Lut1 0xd01a32f4, Lut2 0x0dd7a0d6).
//!
//! WARUM DIE CRC-PRUEFUNG HIER EHER NOTWENDIG IST als bei den
//! anderen Tabellen: die drei Luts stehen als Dezimalzahlen im
//! Fliesstext, ein Tippfehler faellt nicht als Bytefehler auf,
//! sondern als **falsches但 korrekt gebautes** Kontextverzeichnis —
//! der Decoder laeuft dann ohne Fehler und liefert falsche Bytes.

/// Lut0: 256 Byte, Index ist das letzte entpackte Byte `p1`
/// bzw. das vorletzte `p2`.
pub const LUT0: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    8, 12, 16, 12, 12, 20, 12, 16, 24, 28, 12, 12, 32, 12, 36, 12, 44, 44, 44, 44, 44, 44, 44, 44,
    44, 44, 32, 32, 24, 40, 28, 12, 12, 48, 52, 52, 52, 48, 52, 52, 52, 48, 52, 52, 52, 52, 52, 48,
    52, 52, 52, 52, 52, 48, 52, 52, 52, 52, 52, 24, 12, 28, 12, 12, 12, 56, 60, 60, 60, 56, 60, 60,
    60, 56, 60, 60, 60, 60, 60, 56, 60, 60, 60, 60, 60, 56, 60, 60, 60, 60, 60, 24, 12, 28, 12, 0,
    0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1,
    0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1,
    2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3,
    2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3,
];

/// Lut1: 256 Byte, Index ist das letzte entpackte Byte `p1`
/// bzw. das vorletzte `p2`.
// KORREKTUR 30.09. (Koordination): Bytes 192..=223 (UTF-8-Startbytes) sind 2,
// nicht 0 - RFC 7932 7.1: 128..=191 -> 0, 192..=255 -> 2.
pub const LUT1: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1,
    1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1,
    1, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 1, 1, 1, 1, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
];

/// Lut2: 256 Byte, Index ist das letzte entpackte Byte `p1`
/// bzw. das vorletzte `p2`.
pub const LUT2: [u8; 256] = [
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 7,
];

/// Die vier Kontextarten, in der Nummerierung des RFC (Abschnitt 7.1).
///
/// DER WERT DER ARBEIT: "The context modes LSB6, MSB6, UTF8, and
/// Signed are denoted by integers 0, 1, 2, 3." Das sind zwei Bit im
/// Meta-Block-Header je Literalblocktyp, und sie stehen in
/// `meta_block.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KontextArt {
    /// `Context ID = p1 & 0x3f`
    Lsb6,
    /// `Context ID = p1 >> 2`
    Msb6,
    /// `Context ID = Lut0[p1] | Lut1[p2]`
    Utf8,
    /// `Context ID = (Lut2[p1] << 3) | Lut2[p2]`
    Signed,
}

impl KontextArt {
    /// Aus den zwei Bits im Header, woertlich "denoted by integers
    /// 0, 1, 2, 3" (Abschnitt 7.1).
    pub fn aus_zwei_bits(bits: u32) -> Option<Self> {
        match bits {
            0 => Some(KontextArt::Lsb6),
            1 => Some(KontextArt::Msb6),
            2 => Some(KontextArt::Utf8),
            3 => Some(KontextArt::Signed),
            _ => None,
        }
    }

    /// Die Kontext-ID fuer die letzten beiden entpackten Bytes.
    ///
    /// DER BEREICH: "context IDs for literals are in the range of
    /// 0..63" (Abschnitt 7.1) — 64 Kontexte, und die Context Map
    /// im Header hat hoechstens 64 Eintraege.
    pub fn id(&self, p1: u8, p2: u8) -> u8 {
        match self {
            KontextArt::Lsb6 => p1 & 0x3f,
            KontextArt::Msb6 => p1 >> 2,
            KontextArt::Utf8 => LUT0[p1 as usize] | LUT1[p2 as usize],
            KontextArt::Signed => (LUT2[p1 as usize] << 3) | LUT2[p2 as usize],
        }
    }
}
