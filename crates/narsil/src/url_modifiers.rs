//! Release 15.10.2026 (Agent C): the request URL modifiers of a CSS `url()`
//! for images (CSS Values 5 §4.5.4): `cross-origin(anonymous |
//! use-credentials)` asks for a CORS response, `integrity("sha256-…")` for
//! a body with that hash (Subresource Integrity §3.3). The style parser keeps
//! them behind the address as a fragment (`#narsil-mod&cors=…&sri=…`, see
//! `narsil_css::parse::url_innern`); the fragment never goes on the wire.
//!
//! A response that fails them is no image: the box shows what it has
//! without it (its background color).

use std::collections::HashMap;

/// The fragment the style parser appends (with `&key=value` pairs).
pub(crate) const MARK: &str = "#narsil-mod";

/// The modifiers of one image address.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Modifiers {
    /// `anonymous` or `use-credentials`.
    pub cors: Option<String>,
    /// Integrity metadata (`sha256-…`, `sha384-…`).
    pub integrity: Vec<String>,
}

/// The address without the modifier fragment, and the modifiers.
pub(crate) fn split(address: &str) -> (String, Option<Modifiers>) {
    let Some(i) = address.find(MARK) else {
        return (address.to_string(), None);
    };
    let mut m = Modifiers::default();
    for pair in address[i + MARK.len()..].split('&') {
        match pair.split_once('=') {
            Some(("cors", v)) => m.cors = Some(v.to_ascii_lowercase()),
            Some(("sri", v)) => m.integrity.extend(v.split(',').filter(|t| !t.is_empty()).map(str::to_string)),
            _ => {}
        }
    }
    (address[..i].to_string(), Some(m))
}

fn header<'a>(headers: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim())
}

/// Does the response satisfy the modifiers? `page_host` is the host of the
/// requesting document. `cross-origin()` always asks for the CORS headers:
/// `anonymous` accepts `*` or the page, `use-credentials` only the page plus
/// `Access-Control-Allow-Credentials: true` (Fetch §4.9 "CORS check").
pub(crate) fn check(m: &Modifiers, headers: &HashMap<String, String>, body: &[u8], page_host: &str) -> Result<(), String> {
    if let Some(mode) = &m.cors {
        let allow = header(headers, "access-control-allow-origin").unwrap_or("");
        let names_page = |v: &str| {
            let host = v.split("://").nth(1).unwrap_or(v);
            let host = host.split(['/', ':']).next().unwrap_or("");
            !page_host.is_empty() && host.eq_ignore_ascii_case(page_host)
        };
        let ok = if mode == "use-credentials" {
            names_page(allow) && header(headers, "access-control-allow-credentials").is_some_and(|v| v == "true")
        } else {
            allow == "*" || names_page(allow)
        };
        if !ok {
            return Err(format!("CORS check failed (Access-Control-Allow-Origin: {allow:?})"));
        }
    }
    if !m.integrity.is_empty() && !integrity_matches(&m.integrity, body) {
        return Err(String::from("integrity mismatch"));
    }
    Ok(())
}

/// Subresource Integrity §3.3.5: one matching token of a known algorithm
/// is enough; without any known algorithm the body passes.
fn integrity_matches(tokens: &[String], body: &[u8]) -> bool {
    let mut known = false;
    for t in tokens {
        let t = t.split('?').next().unwrap_or("");
        let Some((alg, digest)) = t.split_once('-') else { continue };
        let ours = match alg.to_ascii_lowercase().as_str() {
            "sha256" => base64(&narsil_tls::sha2::sha256(body)),
            "sha384" => base64(&narsil_tls::sha2::sha384(body)),
            _ => continue,
        };
        known = true;
        if ours == digest {
            return true;
        }
    }
    !known
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_split_and_check() {
        let (a, m) = split("http://h:1/x.png#narsil-mod&cors=anonymous&sri=sha256-AAAA");
        assert_eq!(a, "http://h:1/x.png");
        let m = m.expect("modifiers");
        assert_eq!(m.cors.as_deref(), Some("anonymous"));
        let mut h = HashMap::new();
        assert!(check(&m, &h, b"x", "h").is_err());
        h.insert(String::from("Access-Control-Allow-Origin"), String::from("*"));
        assert!(check(&Modifiers { cors: m.cors.clone(), integrity: Vec::new() }, &h, b"x", "h").is_ok());
        // sha256("abc") = ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=
        assert!(integrity_matches(&[String::from("sha256-ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=")], b"abc"));
        assert!(!integrity_matches(&[String::from("sha256-AAAA")], b"abc"));
        assert!(integrity_matches(&[String::from("md5-xyz")], b"abc"));
    }
}
