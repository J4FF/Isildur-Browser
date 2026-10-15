//! Begrenzte data:-Bildquellen; keine Netzverbindung erforderlich.
use crate::bilder::{BildFehler, MAX_BILDBYTE};

pub(crate) fn lesen(url: &str) -> Result<Vec<u8>, BildFehler> {
    let (kopf, daten) = url
        .strip_prefix("data:")
        .and_then(|s| s.split_once(','))
        .ok_or_else(|| BildFehler::Format("data-URL ohne Komma".into()))?;
    if daten.len() > MAX_BILDBYTE * 3 {
        return Err(BildFehler::ZuVielByte(daten.len()));
    }
    let mut roh = Vec::new();
    let b = daten.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = |c: u8| (c as char).to_digit(16).map(|v| v as u8);
            let wert = b
                .get(i + 1)
                .and_then(|a| hex(*a))
                .zip(b.get(i + 2).and_then(|a| hex(*a)));
            let Some((a, c)) = wert else {
                return Err(BildFehler::Format(
                    "ungueltiges Prozent-Escape in data-URL".into(),
                ));
            };
            roh.push(a * 16 + c);
            i += 3;
        } else {
            roh.push(b[i]);
            i += 1;
        }
        if roh.len() > MAX_BILDBYTE {
            return Err(BildFehler::ZuVielByte(roh.len()));
        }
    }
    if !kopf.split(';').any(|s| s.eq_ignore_ascii_case("base64")) {
        return Ok(roh);
    }
    let mut aus = Vec::new();
    let mut akku = 0u32;
    let mut bits = 0;
    let mut ende = false;
    for c in roh {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == b'=' {
            ende = true;
            continue;
        }
        if ende {
            return Err(BildFehler::Format("Base64-Daten nach Padding".into()));
        }
        let n = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(BildFehler::Format("ungueltiges Base64".into())),
        };
        akku = (akku << 6) | u32::from(n);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            aus.push((akku >> bits) as u8);
        }
    }
    if bits == 6 || (bits > 0 && akku & ((1 << bits) - 1) != 0) {
        return Err(BildFehler::Format("unvollstaendiges Base64".into()));
    }
    Ok(aus)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quelltext_und_base64() {
        assert_eq!(
            lesen("data:image/svg+xml,%3Csvg%3E%2B%3C%2Fsvg%3E").unwrap(),
            b"<svg>+</svg>"
        );
        assert_eq!(
            lesen("data:image/png;base64,AAECA%2F8=").unwrap(),
            [0, 1, 2, 3, 255]
        );
        assert!(lesen("data:image/png;base64,A").is_err());
        assert!(lesen("data:image/svg+xml,%XX").is_err());
    }
}
