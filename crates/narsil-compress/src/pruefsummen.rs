//! Prüfsummen: CRC32 (PNG, RFC 2083) und Adler32 (zlib, RFC 1950).
//
// W5F, 30.09.2026: `use crate::FehlerZlib;` entfernt — in dieser Datei
// wird der Typ nicht benutzt. Der Import war der dritte Beweis, dass
// hier jemand vor laenger als einmal vorbeigegangen ist, ohne zu lesen.

/// CRC32-Tabelle (Polynom 0xEDB88320, gespiegelt).
const fn crc_tabelle() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut n = 0usize;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
}

static CRC_TABELLE: [u32; 256] = crc_tabelle();

/// CRC32 wie in PNG/gzip: Anfang 0xFFFFFFFF, Tabellenschritt, am Ende invertiert.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        c = CRC_TABELLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// CRC32 fortsetzen.
pub fn crc32_begin() -> u32 {
    0xFFFF_FFFF
}

pub fn crc32_schritt(start: u32, bytes: &[u8]) -> u32 {
    let mut c = start;
    for &b in bytes {
        c = CRC_TABELLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c
}

pub fn crc32_zwei(erstes: &[u8], zweites: &[u8]) -> u32 {
    let start = crc32_schritt(crc32_begin(), erstes);
    crc32_schritt(start, zweites) ^ 0xFFFF_FFFF
}

/// Adler32 nach RFC 1950.
pub fn adler32(bytes: &[u8]) -> u32 {
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for block in bytes.chunks(5552) {
        for &b in block {
            s1 += b as u32;
            s2 += s1;
        }
        s1 %= 65521;
        s2 %= 65521;
    }
    (s2 << 16) | s1
}

pub fn adler32_schritt(start: (u32, u32), bytes: &[u8]) -> (u32, u32) {
    let (mut s1, mut s2) = start;
    for block in bytes.chunks(5552) {
        for &b in block {
            s1 += b as u32;
            s2 += s1;
        }
        s1 %= 65521;
        s2 %= 65521;
    }
    (s1, s2)
}
