//! RSA for Web Crypto (RFC 8017): key generation, RSASSA-PKCS1-v1_5,
//! RSASSA-PSS and RSAES-OAEP with any of the four hashes.

use super::hash::{self, HashAlg};
use super::num;
use crate::zahl::Zahl;

/// A public key.
#[derive(Clone, Debug)]
pub struct RsaPublic {
    pub n: Zahl,
    pub e: Zahl,
}

/// A private key with CRT parameters (all big-endian when exported).
#[derive(Clone, Debug)]
pub struct RsaPrivate {
    pub n: Zahl,
    pub e: Zahl,
    pub d: Zahl,
    pub p: Zahl,
    pub q: Zahl,
    pub dp: Zahl,
    pub dq: Zahl,
    pub qi: Zahl,
}

impl RsaPublic {
    pub fn size(&self) -> usize {
        num::bits(&self.n).div_ceil(8)
    }
}

impl RsaPrivate {
    pub fn public(&self) -> RsaPublic {
        RsaPublic { n: self.n.clone(), e: self.e.clone() }
    }

    pub fn size(&self) -> usize {
        num::bits(&self.n).div_ceil(8)
    }

    /// Build from the eight components (JWK/PKCS#8 import); checks that
    /// they belong together.
    pub fn from_parts(parts: [&[u8]; 8]) -> Result<RsaPrivate, &'static str> {
        let z = |b: &[u8]| num::from_bytes(b);
        let k = RsaPrivate {
            n: z(parts[0])?,
            e: z(parts[1])?,
            d: z(parts[2])?,
            p: z(parts[3])?,
            q: z(parts[4])?,
            dp: z(parts[5])?,
            dq: z(parts[6])?,
            qi: z(parts[7])?,
        };
        if !num::eq(&num::mul(&k.p, &k.q), &k.n) {
            return Err("RSA key: p * q != n");
        }
        Ok(k)
    }
}

/// Generate a key of `bits` bits with public exponent `e` (big-endian).
pub fn generate(bits: usize, e: &[u8]) -> Result<RsaPrivate, &'static str> {
    if !(256..=16384).contains(&bits) || bits % 8 != 0 {
        return Err("RSA modulus length is not supported");
    }
    let e = num::from_bytes(e)?;
    if num::bits(&e) < 2 || !crate::zahl::ist_ungerade(&e) {
        return Err("RSA public exponent is not usable");
    }
    let one = Zahl::eins();
    for _ in 0..100 {
        let p = num::random_prime(bits / 2, &e)?;
        let q = num::random_prime(bits - bits / 2, &e)?;
        if num::eq(&p, &q) {
            continue;
        }
        let (p, q) = if num::ge(&p, &q) { (p, q) } else { (q, p) };
        let n = num::mul(&p, &q);
        if num::bits(&n) != bits {
            continue;
        }
        let pm1 = num::sub(&p, &one);
        let qm1 = num::sub(&q, &one);
        // d = e^-1 mod lcm(p-1, q-1)
        let g = num::gcd(&pm1, &qm1);
        let lambda = num::div_rem(&num::mul(&pm1, &qm1), &g)?.0;
        let Some(d) = num::inv_mod(&e, &lambda) else { continue };
        let dp = num::rem(&d, &pm1)?;
        let dq = num::rem(&d, &qm1)?;
        let Some(qi) = num::inv_mod(&q, &p) else { continue };
        return Ok(RsaPrivate { n, e, d, p, q, dp, dq, qi });
    }
    Err("RSA key generation failed")
}

fn public_op(k: &RsaPublic, m: &[u8]) -> Result<Vec<u8>, &'static str> {
    let x = num::from_bytes(m)?;
    if num::ge(&x, &k.n) {
        return Err("RSA input out of range");
    }
    Ok(num::pow_mod(&x, &k.e, &k.n)?.nach_bytes_gross(k.size()))
}

fn private_op(k: &RsaPrivate, c: &[u8]) -> Result<Vec<u8>, &'static str> {
    let x = num::from_bytes(c)?;
    if num::ge(&x, &k.n) {
        return Err("RSA input out of range");
    }
    // CRT: m1 = c^dp mod p, m2 = c^dq mod q, h = qi (m1 - m2) mod p.
    let mp = num::Mont::new(&k.p).ok_or("RSA prime p is even")?;
    let mq = num::Mont::new(&k.q).ok_or("RSA prime q is even")?;
    let m1 = mp.pow(&x, &k.dp);
    let m2 = mq.pow(&x, &k.dq);
    let m2p = num::rem(&m2, &k.p)?;
    let h = num::mul_mod(&k.qi, &num::sub_mod(&m1, &m2p, &k.p)?, &k.p)?;
    let m = num::add(&m2, &num::mul(&h, &k.q));
    // Check against the public operation (fault attacks, wrong imports).
    let back = num::pow_mod(&m, &k.e, &k.n)?;
    if !num::eq(&back, &num::rem(&x, &k.n)?) {
        return Err("RSA private key is inconsistent");
    }
    Ok(m.nach_bytes_gross(k.size()))
}

fn digest_info(h: HashAlg) -> &'static [u8] {
    match h {
        HashAlg::Sha1 => &[0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14],
        HashAlg::Sha256 => &[
            0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20,
        ],
        HashAlg::Sha384 => &[
            0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05, 0x00, 0x04, 0x30,
        ],
        HashAlg::Sha512 => &[
            0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40,
        ],
    }
}

fn pkcs1_encode(h: HashAlg, msg: &[u8], k: usize) -> Result<Vec<u8>, &'static str> {
    let di = digest_info(h);
    let d = hash::digest(h, msg);
    let t_len = di.len() + d.len();
    if k < t_len + 11 {
        return Err("RSA key too short for the hash");
    }
    let mut em = vec![0u8, 1u8];
    em.extend(std::iter::repeat_n(0xffu8, k - t_len - 3));
    em.push(0);
    em.extend_from_slice(di);
    em.extend_from_slice(&d);
    Ok(em)
}

/// RSASSA-PKCS1-v1_5 signature.
pub fn pkcs1_sign(k: &RsaPrivate, h: HashAlg, msg: &[u8]) -> Result<Vec<u8>, &'static str> {
    let em = pkcs1_encode(h, msg, k.size())?;
    private_op(k, &em)
}

pub fn pkcs1_verify(k: &RsaPublic, h: HashAlg, msg: &[u8], sig: &[u8]) -> bool {
    if sig.len() != k.size() {
        return false;
    }
    let Ok(em) = public_op(k, sig) else { return false };
    match pkcs1_encode(h, msg, k.size()) {
        Ok(expected) => super::equal(&em, &expected),
        Err(_) => false,
    }
}

fn mgf1(h: HashAlg, seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut c = 0u32;
    while out.len() < len {
        let d = hash::digest_parts(h, &[seed, &c.to_be_bytes()]);
        let take = (len - out.len()).min(d.len());
        out.extend_from_slice(&d[..take]);
        c += 1;
    }
    out
}

/// RSASSA-PSS signature with MGF1(h) and `salt_len` bytes of salt.
pub fn pss_sign(k: &RsaPrivate, h: HashAlg, msg: &[u8], salt_len: usize) -> Result<Vec<u8>, &'static str> {
    let mod_bits = num::bits(&k.n) - 1;
    let em_len = mod_bits.div_ceil(8);
    let hl = h.out_len();
    if em_len < hl + salt_len + 2 {
        return Err("RSA-PSS salt too long for the key");
    }
    let m_hash = hash::digest(h, msg);
    let salt = super::random(salt_len)?;
    let hh = hash::digest_parts(h, &[&[0u8; 8], &m_hash, &salt]);
    let mut db = vec![0u8; em_len - salt_len - hl - 2];
    db.push(1);
    db.extend_from_slice(&salt);
    let mask = mgf1(h, &hh, db.len());
    for (a, b) in db.iter_mut().zip(mask.iter()) {
        *a ^= b;
    }
    let zero_bits = 8 * em_len - mod_bits;
    db[0] &= 0xffu8 >> zero_bits;
    let mut em = db;
    em.extend_from_slice(&hh);
    em.push(0xbc);
    // Left-pad to the modulus size for the private operation.
    let mut full = vec![0u8; k.size() - em.len()];
    full.extend_from_slice(&em);
    private_op(k, &full)
}

pub fn pss_verify(k: &RsaPublic, h: HashAlg, msg: &[u8], sig: &[u8], salt_len: usize) -> bool {
    if sig.len() != k.size() {
        return false;
    }
    let Ok(m) = public_op(k, sig) else { return false };
    let mod_bits = num::bits(&k.n) - 1;
    let em_len = mod_bits.div_ceil(8);
    let hl = h.out_len();
    if em_len < hl + salt_len + 2 || m.len() < em_len {
        return false;
    }
    if m[..m.len() - em_len].iter().any(|&b| b != 0) {
        return false;
    }
    let em = &m[m.len() - em_len..];
    if em[em_len - 1] != 0xbc {
        return false;
    }
    let (masked, rest) = em.split_at(em_len - hl - 1);
    let hh = &rest[..hl];
    let zero_bits = 8 * em_len - mod_bits;
    if masked[0] & !(0xffu8 >> zero_bits) != 0 {
        return false;
    }
    let mask = mgf1(h, hh, masked.len());
    let mut db: Vec<u8> = masked.iter().zip(mask.iter()).map(|(a, b)| a ^ b).collect();
    db[0] &= 0xffu8 >> zero_bits;
    let ps_len = em_len - hl - salt_len - 2;
    if db[..ps_len].iter().any(|&b| b != 0) || db[ps_len] != 1 {
        return false;
    }
    let salt = &db[db.len() - salt_len..];
    let m_hash = hash::digest(h, msg);
    let expected = hash::digest_parts(h, &[&[0u8; 8], &m_hash, salt]);
    super::equal(&expected, hh)
}

/// RSAES-OAEP encryption with MGF1(h).
pub fn oaep_encrypt(k: &RsaPublic, h: HashAlg, msg: &[u8], label: &[u8]) -> Result<Vec<u8>, &'static str> {
    let size = k.size();
    let hl = h.out_len();
    if size < 2 * hl + 2 || msg.len() > size - 2 * hl - 2 {
        return Err("RSA-OAEP message too long for the key");
    }
    let l_hash = hash::digest(h, label);
    let mut db = l_hash;
    db.extend(std::iter::repeat_n(0u8, size - msg.len() - 2 * hl - 2));
    db.push(1);
    db.extend_from_slice(msg);
    let seed = super::random(hl)?;
    let db_mask = mgf1(h, &seed, db.len());
    for (a, b) in db.iter_mut().zip(db_mask.iter()) {
        *a ^= b;
    }
    let seed_mask = mgf1(h, &db, hl);
    let masked_seed: Vec<u8> = seed.iter().zip(seed_mask.iter()).map(|(a, b)| a ^ b).collect();
    let mut em = vec![0u8];
    em.extend_from_slice(&masked_seed);
    em.extend_from_slice(&db);
    public_op(k, &em)
}

pub fn oaep_decrypt(k: &RsaPrivate, h: HashAlg, ct: &[u8], label: &[u8]) -> Result<Vec<u8>, &'static str> {
    let size = k.size();
    let hl = h.out_len();
    if ct.len() != size || size < 2 * hl + 2 {
        return Err("RSA-OAEP decryption error");
    }
    let em = private_op(k, ct).map_err(|_| "RSA-OAEP decryption error")?;
    let (y, rest) = em.split_at(1);
    let (masked_seed, masked_db) = rest.split_at(hl);
    let seed_mask = mgf1(h, masked_db, hl);
    let seed: Vec<u8> = masked_seed.iter().zip(seed_mask.iter()).map(|(a, b)| a ^ b).collect();
    let db_mask = mgf1(h, &seed, masked_db.len());
    let db: Vec<u8> = masked_db.iter().zip(db_mask.iter()).map(|(a, b)| a ^ b).collect();
    let l_hash = hash::digest(h, label);
    let mut bad = y[0] != 0;
    bad |= !super::equal(&db[..hl], &l_hash);
    let mut idx = None;
    for (i, &b) in db[hl..].iter().enumerate() {
        if b == 1 {
            idx = Some(hl + i);
            break;
        }
        if b != 0 {
            bad = true;
            break;
        }
    }
    match (bad, idx) {
        (false, Some(i)) => Ok(db[i + 1..].to_vec()),
        _ => Err("RSA-OAEP decryption error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_sign_verify_encrypt_decrypt() {
        let k = generate(1024, &[1, 0, 1]).unwrap();
        assert_eq!(num::bits(&k.n), 1024);
        let pubk = k.public();
        let s = pkcs1_sign(&k, HashAlg::Sha256, b"hello").unwrap();
        assert!(pkcs1_verify(&pubk, HashAlg::Sha256, b"hello", &s));
        assert!(!pkcs1_verify(&pubk, HashAlg::Sha256, b"hellp", &s));
        let s = pss_sign(&k, HashAlg::Sha384, b"hello", 32).unwrap();
        assert!(pss_verify(&pubk, HashAlg::Sha384, b"hello", &s, 32));
        assert!(!pss_verify(&pubk, HashAlg::Sha384, b"hello", &s, 31));
        let c = oaep_encrypt(&pubk, HashAlg::Sha1, b"secret", b"lbl").unwrap();
        assert_eq!(oaep_decrypt(&k, HashAlg::Sha1, &c, b"lbl").unwrap(), b"secret");
        assert!(oaep_decrypt(&k, HashAlg::Sha1, &c, b"other").is_err());
    }
}
