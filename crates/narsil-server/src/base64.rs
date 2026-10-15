//! Base64 in reinem Rust (RFC 4648, Standard-Alphabet).
//!
//! Nur fuer den WebSocket-Handshake ([`crate::ws`]): `Sec-WebSocket-Key`
//! wird mit SHA-1 gehasht und Base64-kodiert. Kein Padding-Raten, kein
//! URL-Alphabet — was nicht zum Standard gehoert, ist ein Fehler.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Kodiert Bytes als Base64-String (mit `=`-Padding).
pub fn kodieren(bytes: &[u8]) -> String {
    let mut raus = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for stueck in bytes.chunks(3) {
        let (b0, b1, b2) = (
            stueck[0],
            *stueck.get(1).unwrap_or(&0),
            *stueck.get(2).unwrap_or(&0),
        );
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        raus.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        raus.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        if stueck.len() > 1 {
            raus.push(ALPHABET[((n >> 6) & 63) as usize] as char);
        } else {
            raus.push('=');
        }
        if stueck.len() > 2 {
            raus.push(ALPHABET[(n & 63) as usize] as char);
        } else {
            raus.push('=');
        }
    }
    raus
}

/// Dekodiert einen Base64-String. Whitespace wird ignoriert, alles andere
/// ausser Alphabet + `=` ist ein Fehler mit Position.
pub fn dekodieren(s: &str) -> Result<Vec<u8>, String> {
    let mut werte: Vec<u8> = Vec::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_whitespace() {
            continue;
        }
        if c == '=' {
            werte.push(64); // Padding-Marker
            continue;
        }
        let v = ALPHABET
            .iter()
            .position(|&a| a as char == c)
            .ok_or_else(|| format!("Base64: Zeichen {c:?} bei Position {i}"))?;
        werte.push(v as u8);
    }
    if werte.len() % 4 != 0 {
        return Err(format!(
            "Base64: Laenge {} kein Vielfaches von 4",
            werte.len()
        ));
    }
    let mut raus = Vec::with_capacity(werte.len() / 4 * 3);
    for stueck in werte.chunks(4) {
        // Padding nur am Ende der LETZTEN Gruppe — sonst ist es kein
        // Padding, sondern ein abgeschnittener Strom.
        let [a, b, c, d] = [stueck[0], stueck[1], stueck[2], stueck[3]];
        if a == 64 || b == 64 {
            return Err("Base64: Padding an falscher Stelle".to_string());
        }
        let n = ((a as u32) << 18) | ((b as u32) << 12);
        raus.push((n >> 16) as u8);
        if c != 64 {
            let n = n | ((c as u32) << 6);
            raus.push((n >> 8) as u8);
            if d != 64 {
                let n = n | (d as u32);
                raus.push(n as u8);
            }
        } else if d != 64 {
            return Err("Base64: Padding an falscher Stelle".to_string());
        }
    }
    Ok(raus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4648_vektoren() {
        // RFC 4648, Abschnitt 10.
        assert_eq!(kodieren(b""), "");
        assert_eq!(kodieren(b"f"), "Zg==");
        assert_eq!(kodieren(b"fo"), "Zm8=");
        assert_eq!(kodieren(b"foo"), "Zm9v");
        assert_eq!(kodieren(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn runde() {
        let bytes: Vec<u8> = (0..255).collect();
        assert_eq!(dekodieren(&kodieren(&bytes)).unwrap(), bytes);
    }

    #[test]
    fn fehler_hat_position() {
        assert!(dekodieren("!!!").is_err());
        assert!(dekodieren("ABC").is_err());
    }

    #[test]
    fn websocket_schluessel_runde() {
        // Der RFC-6455-Beispielschluessel, dekodiert 16 Bytes.
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        assert_eq!(dekodieren(key).unwrap().len(), 16);
    }
}
