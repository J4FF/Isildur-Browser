//! SHA-1 in reinem Rust (FIPS 180-4).
//!
//! NUR fuer den WebSocket-Handshake (RFC 6455, Abschnitt 1.3): der
//! `Sec-WebSocket-Accept`-Wert ist `Base64(SHA1(Key + GUID))`.
//! SHA-1 ist als Passwort-Hash oder Integritaetsnachweis VERBOTEN
//! (kollisionsanfaellig seit 2017) — hier steht es, weil das Protokoll
//! es vorschreibt, nicht weil es sicher waere. Wer Inhalte pruefen
//! will, nutzt kein SHA-1 (siehe [`crate::tls`]).

/// Berechnet den SHA-1-Hash von Bytes (20 Bytes).
pub fn sha1(bytes: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];

    // Padding: 0x80, dann Nullen, dann 64-Bit-Laenge (Big-Endian).
    let bitlen = (bytes.len() as u64).wrapping_mul(8);
    let mut nachricht = bytes.to_vec();
    nachricht.push(0x80);
    while nachricht.len() % 64 != 56 {
        nachricht.push(0);
    }
    nachricht.extend_from_slice(&bitlen.to_be_bytes());

    for block in nachricht.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        // `i` ist hier die RUNDE, nicht ein Zaehler ueber einer
        // Folge: der Korper braucht `i` selbst (Grenzen bei 20/40/60)
        // und greift auf w[i-3]..w[i-16] zu — ein Iterator muesste
        // diese Zahlen trotzdem hervorholen. Bewusst erlaubt.
        #[allow(clippy::needless_range_loop)]
        for i in 0..80 {
            let (f, k) = if i < 20 {
                ((b & c) | ((!b) & d), 0x5A827999)
            } else if i < 40 {
                (b ^ c ^ d, 0x6ED9EBA1)
            } else if i < 60 {
                ((b & c) | (b & d) | (c & d), 0x8F1BBCDC)
            } else {
                (b ^ c ^ d, 0xCA62C1D6)
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut raus = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        raus[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    raus
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn fips_vektoren() {
        // FIPS 180-4, Abschnitt 8 (geprueft gegen Python hashlib).
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn websocket_guid_vektor() {
        // RFC 6455, Abschnitt 1.3: Key + GUID -> Accept.
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let guid = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
        let hash = sha1(format!("{key}{guid}").as_bytes());
        assert_eq!(
            crate::base64::kodieren(&hash),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }
}
