//! CRC32 (PNG, RFC 2083) und Adler32 (zlib, RFC 1950).
//!
//! Zwei winzige Pruefsummen, aber sie sind der Grund, warum ein Bilddecoder
//! ueberhaupt weiss, dass er nicht am Ende noch Unsinn liest:
//!   * CRC32 haengt an JEDEM PNG-Chunk und deckt Typ und Daten ab. Wer das
//!     entfernt, akzeptiert jedes verdrehte Bild.
//!   * Adler32 haengt am zlib-Stream und deckt die ENTPACKTEN Daten ab.
//!
//! Beide sind hier nachgerechnet, nicht aus einer Crate importiert (ADR-030).

/// Die CRC32-Tabelle: `t[n] = CRC32 des Einzelbytes n`.
///
/// Wird zur Compile-Zeit gebaut (`const fn`), damit im Betrieb keine
/// Initialisierung und keine_threadsynchronisation noetig ist. Das Muster
/// ist genau das, was die Spezifikation vorschreibt: Polynom 0xEDB88320
/// (gespiegelt), Initialwert 0xFFFFFFFF, Endbelegung invertiert.
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

/// CRC32 wie in PNG: Anfang `0xFFFFFFFF`, Tabellenschritt, am Ende invertiert.
///
/// Beweis aus der Spezifikation: CRC32 ueber die acht Bytes der PNG-Signatur
/// `89 50 4E 47 0D 0A 1A 0A` ist `0xAE426082`. Genau das steht im
/// `test_crc32_der_signatur` gegen eine fest verdrahtete Zahl gerechnet —
/// nicht gegen sich selbst.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        c = CRC_TABELLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// CRC32 fortsetzen, ohne den Startwert zu verlieren.
///
/// Noetig, weil die IDAT-Daten in einem oder mehreren Chunks ankommen und
/// der CRC eines Chunks ueber Typ+DiesesChunk-Daten laeuft — nicht ueber
/// alle IDAT-Chunks hinweg.
pub fn crc32_begin() -> u32 {
    0xFFFF_FFFF
}

/// CRC32-Schritt fuer einen laufenden Wert. `start` ist das Ergebnis von
/// [`crc32_begin`]; das Ergebnis ist der fertige CRC-Wert.
pub fn crc32_schritt(start: u32, bytes: &[u8]) -> u32 {
    let mut c = start;
    for &b in bytes {
        c = CRC_TABELLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c
}

/// CRC32 ueber zwei Stuecke, als waeren sie eines (fuer Chunk-Typ + Daten).
///
/// Der PNG-Chunk-CRC laeuft ueber die vier Typbytes UND die Daten, in dieser
/// Reihenfolge, und das sind zwei verschiedene Speicherstuecke. Deshalb
/// dieser Umweg — sonst muesste der Aufrufer erst einen Puffer bauen, nur um
/// vier Bytes und Daten zusammenzufuehren.
pub fn crc32_zwei(erstes: &[u8], zweites: &[u8]) -> u32 {
    let start = crc32_schritt(crc32_begin(), erstes);
    crc32_schritt(start, zweites) ^ 0xFFFF_FFFF
}

/// Adler32 nach RFC 1950 Abschnitt 9: `s1 = 1`, `s2 = 0`,
/// `s1 = (s1 + byte) % 65521`, `s2 = (s2 + s1) % 65521`,
/// Ergebnis `(s2 << 16) | s1`.
///
/// Das `% 65521` ist das primitive Polynom von Adler32, deshalb heisst es
/// nicht 65535. Wer 65535 nimmt, erzeugt einen Strom, den `zlib` ablehnt.
pub fn adler32(bytes: &[u8]) -> u32 {
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    // Grosse Bloecke: 5552 Bytes sind das groesste Stueck, bei dem s2
    // garantiert unter 2^32 bleibt. Das haelt die Schleife kurz, ohne dass
    // pro Byte eine Modulo-Operation faellt.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Feste Zahl aus der Spezifikation, nicht aus dem eigenen Code.
    /// Quelle: RFC 1952 Anhang B / die Standardpruefung „123456789".
    #[test]
    fn crc32_der_scheckre() {
        // crc32("123456789") == 0xCBF43926. Das ist DER Wert, an dem man
        // CRC32 erkennt; steht in RFC 1952 Anhang B.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(&[0u8; 32]), 0x190A_55AD);
    }

    /// Das Gegenstueck: Adlers "scheckre" ist 0x091E01DE.
    #[test]
    fn adler32_der_scheckre() {
        assert_eq!(adler32(b"123456789"), 0x091E_01DE);
        assert_eq!(adler32(b""), 0x0000_0001);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    /// Die Tabelle ist zur Compile-Zeit gebaut — dieser Test belegt, dass
    /// sie dann auch benutzt wird und nicht etwa ein zweiter, abweichender
    /// Algorithmus danebenlaeuft.
    #[test]
    fn tabelle_stimmt_mit_dem_rechnen_ueberein() {
        assert_eq!(CRC_TABELLE[1], 0x7707_3096);
        assert_eq!(CRC_TABELLE[0], 0x0000_0000);
    }

    /// `crc32_zwei` ist nur eine Bequemlichkeit; sie MUSS dasselbe ergeben
    /// wie `crc32` ueber die Verkettung. Sonst waere der Chunk-CRC anders
    /// als der Datei-CRC und genau das faellt niemandem auf.
    #[test]
    fn crc32_zwei_ist_gleich_wie_crc32_ueber_alles() {
        assert_eq!(
            crc32_zwei(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]),
            crc32(b"IHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x00\x00\x00\x00")
        );
    }

    /// GEGENBEWEIS-VORBEREITUNG: Wenn jemand die Endbelegung `^ 0xFFFFFFFF`
    /// wegnimmt, wird `crc32_der_scheckre` rot. Ohne Endbelegung waere das
    /// Ergebnis `0x340BC6D9` statt `0xCBF43926`.
    #[test]
    fn crc32_ohne_endbelegung_waere_ein_anderer_wert() {
        let mut c: u32 = 0xFFFF_FFFF;
        for &b in b"123456789" {
            c = CRC_TABELLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
        assert_eq!(c ^ 0xFFFF_FFFF, 0xCBF4_3926);
        assert_eq!(c, 0x340B_C6D9); // die Endbelegung fehlt -> anderer Wert
    }
}
