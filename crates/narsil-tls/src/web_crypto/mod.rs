//! Primitives for the Web Crypto API (`crypto.subtle` in narsil-jsvm).
//!
//! Added 2026-10-15. Builds on the TLS primitives of this crate (SHA-2,
//! AES block cipher, big numbers, X25519) and adds what Web Crypto needs on
//! top: SHA-512, generic HMAC/HKDF/PBKDF2, AES-CBC/CTR/GCM (any IV and tag
//! length)/KW, RSA (key generation, PKCS#1 v1.5, PSS, OAEP), ECDSA/ECDH on
//! P-256/P-384/P-521, Ed25519.
//!
//! Every function is pure (bytes in, bytes out) and returns `Err` with a
//! short reason instead of panicking on bad input; the JS side maps errors
//! to the DOMException the spec names.

pub mod aes;
pub mod ec;
pub mod ed25519;
pub mod hash;
pub mod num;
pub mod rsa;

/// `n` bytes from the operating system's CSPRNG.
pub fn random(n: usize) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let k = (n - out.len()).min(crate::zufall::MAX_BLOCK);
        let b = crate::zufall::bytes(k).map_err(|_| "no randomness available")?;
        out.extend_from_slice(&b);
    }
    Ok(out)
}

/// Constant-time comparison (lengths are public).
pub fn equal(a: &[u8], b: &[u8]) -> bool {
    crate::hmac::vergleich_gleich(a, b)
}
