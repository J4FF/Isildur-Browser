//! Hash functions, HMAC (RFC 2104), HKDF (RFC 5869) and PBKDF2 (RFC 8018)
//! over SHA-1/SHA-256/SHA-384/SHA-512.

use crate::sha2::{Sha1, Sha256, Sha384, Sha512};

/// The hash algorithms of Web Crypto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlg {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlg {
    pub fn from_name(name: &str) -> Option<HashAlg> {
        Some(match name {
            "SHA-1" => HashAlg::Sha1,
            "SHA-256" => HashAlg::Sha256,
            "SHA-384" => HashAlg::Sha384,
            "SHA-512" => HashAlg::Sha512,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            HashAlg::Sha1 => "SHA-1",
            HashAlg::Sha256 => "SHA-256",
            HashAlg::Sha384 => "SHA-384",
            HashAlg::Sha512 => "SHA-512",
        }
    }

    /// Output length in bytes.
    pub fn out_len(self) -> usize {
        match self {
            HashAlg::Sha1 => 20,
            HashAlg::Sha256 => 32,
            HashAlg::Sha384 => 48,
            HashAlg::Sha512 => 64,
        }
    }

    /// Block length in bytes (the HMAC default key length).
    pub fn block_len(self) -> usize {
        match self {
            HashAlg::Sha1 | HashAlg::Sha256 => 64,
            HashAlg::Sha384 | HashAlg::Sha512 => 128,
        }
    }
}

/// An incremental hash of any of the four algorithms.
#[derive(Clone)]
pub enum Hasher {
    Sha1(Sha1),
    Sha256(Sha256),
    Sha384(Sha384),
    Sha512(Sha512),
}

impl Hasher {
    pub fn new(alg: HashAlg) -> Hasher {
        match alg {
            HashAlg::Sha1 => Hasher::Sha1(Sha1::neu()),
            HashAlg::Sha256 => Hasher::Sha256(Sha256::neu()),
            HashAlg::Sha384 => Hasher::Sha384(Sha384::neu()),
            HashAlg::Sha512 => Hasher::Sha512(Sha512::neu()),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Sha1(h) => h.aufnehmen(data),
            Hasher::Sha256(h) => h.aufnehmen(data),
            Hasher::Sha384(h) => h.aufnehmen(data),
            Hasher::Sha512(h) => h.aufnehmen(data),
        }
    }

    pub fn finish(self) -> Vec<u8> {
        match self {
            Hasher::Sha1(h) => h.fertig().to_vec(),
            Hasher::Sha256(h) => h.fertig().to_vec(),
            Hasher::Sha384(h) => h.fertig().to_vec(),
            Hasher::Sha512(h) => h.fertig().to_vec(),
        }
    }
}

/// One-shot hash.
pub fn digest(alg: HashAlg, data: &[u8]) -> Vec<u8> {
    let mut h = Hasher::new(alg);
    h.update(data);
    h.finish()
}

/// Hash of several pieces.
pub fn digest_parts(alg: HashAlg, parts: &[&[u8]]) -> Vec<u8> {
    let mut h = Hasher::new(alg);
    for p in parts {
        h.update(p);
    }
    h.finish()
}

/// A keyed HMAC state (inner and outer pad already absorbed).
#[derive(Clone)]
pub struct Hmac {
    inner: Hasher,
    outer: Hasher,
}

impl Hmac {
    pub fn new(alg: HashAlg, key: &[u8]) -> Hmac {
        let b = alg.block_len();
        let mut k = if key.len() > b { digest(alg, key) } else { key.to_vec() };
        k.resize(b, 0);
        let ipad: Vec<u8> = k.iter().map(|x| x ^ 0x36).collect();
        let opad: Vec<u8> = k.iter().map(|x| x ^ 0x5c).collect();
        let mut inner = Hasher::new(alg);
        inner.update(&ipad);
        let mut outer = Hasher::new(alg);
        outer.update(&opad);
        Hmac { inner, outer }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    pub fn finish(self) -> Vec<u8> {
        let i = self.inner.finish();
        let mut o = self.outer;
        o.update(&i);
        o.finish()
    }
}

/// One-shot HMAC.
pub fn hmac(alg: HashAlg, key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut m = Hmac::new(alg, key);
    m.update(data);
    m.finish()
}

/// HKDF extract + expand (RFC 5869). `len` at most 255 hash lengths.
pub fn hkdf(alg: HashAlg, ikm: &[u8], salt: &[u8], info: &[u8], len: usize) -> Result<Vec<u8>, &'static str> {
    let n = alg.out_len();
    if len > 255 * n {
        return Err("HKDF output too long");
    }
    let salt_default;
    let salt = if salt.is_empty() {
        salt_default = vec![0u8; n];
        &salt_default[..]
    } else {
        salt
    };
    let prk = hmac(alg, salt, ikm);
    let keyed = Hmac::new(alg, &prk);
    let mut out = Vec::with_capacity(len);
    let mut t: Vec<u8> = Vec::new();
    let mut i = 1u8;
    while out.len() < len {
        let mut m = keyed.clone();
        m.update(&t);
        m.update(info);
        m.update(&[i]);
        t = m.finish();
        let take = (len - out.len()).min(n);
        out.extend_from_slice(&t[..take]);
        i = i.wrapping_add(1);
    }
    Ok(out)
}

/// PBKDF2 with HMAC (RFC 8018 section 5.2).
pub fn pbkdf2(alg: HashAlg, password: &[u8], salt: &[u8], iterations: u32, len: usize) -> Result<Vec<u8>, &'static str> {
    if iterations == 0 {
        return Err("PBKDF2 needs at least one iteration");
    }
    let n = alg.out_len();
    let keyed = Hmac::new(alg, password);
    let mut out = Vec::with_capacity(len);
    let mut block = 1u32;
    while out.len() < len {
        let mut m = keyed.clone();
        m.update(salt);
        m.update(&block.to_be_bytes());
        let mut u = m.finish();
        let mut t = u.clone();
        for _ in 1..iterations {
            let mut m = keyed.clone();
            m.update(&u);
            u = m.finish();
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= b;
            }
        }
        let take = (len - out.len()).min(n);
        out.extend_from_slice(&t[..take]);
        block = block.wrapping_add(1);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha512_and_sha1_vectors() {
        assert_eq!(
            hex(&digest(HashAlg::Sha512, b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(hex(&digest(HashAlg::Sha1, b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn hmac_rfc4231_case_2() {
        let m = hmac(HashAlg::Sha512, b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex(&m),
            "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea250554\
             9758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737"
        );
    }

    #[test]
    fn hkdf_rfc5869_case_1() {
        let ikm = [0x0bu8; 22];
        let salt: Vec<u8> = (0u8..=0x0c).collect();
        let info: Vec<u8> = (0xf0u8..=0xf9).collect();
        let okm = hkdf(HashAlg::Sha256, &ikm, &salt, &info, 42).unwrap();
        assert_eq!(
            hex(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    #[test]
    fn pbkdf2_rfc6070() {
        let dk = pbkdf2(HashAlg::Sha1, b"password", b"salt", 2, 20).unwrap();
        assert_eq!(hex(&dk), "ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957");
    }
}
