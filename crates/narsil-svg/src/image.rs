//! `<image>` and `<feImage>` sources: `data:` URLs with raster images or
//! nested SVG documents. Other URLs need the network and are not loaded
//! from inside an SVG image (as in browsers, SVG images are isolated).

use narsil_canvas::Image;
use std::sync::Arc;

/// How deep SVG documents may embed further SVG documents.
const MAX_NESTING: usize = 4;

thread_local! {
    static NESTING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Decodes a `data:` URL into a premultiplied image.
#[must_use]
pub fn decode_href(href: &str) -> Option<Image> {
    let (mime, bytes) = parse_data_url(href.trim())?;
    let looks_svg = mime.contains("svg") || {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]).to_ascii_lowercase();
        head.trim_start().starts_with("<svg") || head.contains("<svg")
    };
    if looks_svg {
        let depth = NESTING.with(std::cell::Cell::get);
        if depth >= MAX_NESTING {
            return None;
        }
        NESTING.with(|n| n.set(depth + 1));
        let out = std::str::from_utf8(&bytes).ok().and_then(|s| crate::render(s, None).ok());
        NESTING.with(|n| n.set(depth));
        return out.map(|r| r.image);
    }
    let bild = narsil_image::lesen(&bytes).ok()?;
    if bild.breite == 0 || bild.hoehe == 0 || bild.rgba.len() != bild.breite as usize * bild.hoehe as usize * 4 {
        return None;
    }
    let mut data = bild.rgba;
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * a + 127) / 255) as u8;
            }
        }
    }
    Some(Image { width: bild.breite, height: bild.hoehe, data: Arc::new(data) })
}

/// Splits a `data:` URL into its media type and decoded bytes.
#[must_use]
pub fn parse_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.get(..5).filter(|p| p.eq_ignore_ascii_case("data:")).map(|_| &url[5..])?;
    let comma = rest.find(',')?;
    let (head, payload) = (&rest[..comma], &rest[comma + 1..]);
    let mut parts = head.split(';');
    let mime = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let base64 = head.split(';').any(|p| p.trim().eq_ignore_ascii_case("base64"));
    let raw = percent_decode(payload);
    let bytes = if base64 { base64_decode(&raw)? } else { raw };
    Some((mime, bytes))
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in input {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == b'=' {
            break;
        }
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_urls_decode() {
        let (m, b) = parse_data_url("data:text/plain;base64,aGVsbG8=").unwrap();
        assert_eq!((m.as_str(), b.as_slice()), ("text/plain", &b"hello"[..]));
        let (_, b) = parse_data_url("data:,a%20b").unwrap();
        assert_eq!(b, b"a b");
    }
}
