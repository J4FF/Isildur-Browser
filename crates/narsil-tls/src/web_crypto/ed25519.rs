//! Ed25519 (RFC 8032 section 5.1) and X25519 (RFC 7748, through the TLS
//! implementation of this crate).

use super::hash::{self, HashAlg};
use super::num;
use crate::zahl::Zahl;

type R<T> = Result<T, &'static str>;

fn hx(s: &str) -> Zahl {
    let b: Vec<u8> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0)).collect();
    num::from_bytes(&b).unwrap_or_else(|_| Zahl::null())
}

struct Params {
    p: Zahl,
    d: Zahl,
    l: Zahl,
    sqrt_m1: Zahl,
    bx: Zahl,
    by: Zahl,
}

fn params() -> Params {
    Params {
        p: hx("7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffed"),
        d: hx("52036cee2b6ffe738cc740797779e89800700a4d4141d8ab75eb4dca135978a3"),
        l: hx("1000000000000000000000000000000014def9dea2f79cd65812631a5cf5d3ed"),
        sqrt_m1: hx("2b8324804fc1df0b2b4d00993dfbd7a72f431806ad2fe478c4ee1b274a0ea0b0"),
        bx: hx("216936d3cd6e53fec0a4e231fdd6dc5c692cc7609525a7b2c9562d608f25d51a"),
        by: hx("6666666666666666666666666666666666666666666666666666666666666658"),
    }
}

/// Extended coordinates (X : Y : Z : T), x = X/Z, y = Y/Z, xy = T/Z.
#[derive(Clone)]
struct Pt {
    x: Zahl,
    y: Zahl,
    z: Zahl,
    t: Zahl,
}

fn m(a: &Zahl, b: &Zahl, p: &Zahl) -> R<Zahl> {
    num::mul_mod(a, b, p)
}

fn pt_add(k: &Params, a: &Pt, b: &Pt) -> R<Pt> {
    let p = &k.p;
    let a1 = m(&num::sub_mod(&a.y, &a.x, p)?, &num::sub_mod(&b.y, &b.x, p)?, p)?;
    let b1 = m(&num::add_mod(&a.y, &a.x, p)?, &num::add_mod(&b.y, &b.x, p)?, p)?;
    let d2 = num::add_mod(&k.d, &k.d, p)?;
    let c = m(&m(&a.t, &d2, p)?, &b.t, p)?;
    let zz = m(&a.z, &b.z, p)?;
    let d = num::add_mod(&zz, &zz, p)?;
    let e = num::sub_mod(&b1, &a1, p)?;
    let f = num::sub_mod(&d, &c, p)?;
    let g = num::add_mod(&d, &c, p)?;
    let h = num::add_mod(&b1, &a1, p)?;
    Ok(Pt { x: m(&e, &f, p)?, y: m(&g, &h, p)?, t: m(&e, &h, p)?, z: m(&f, &g, p)? })
}

fn pt_double(k: &Params, a: &Pt) -> R<Pt> {
    let p = &k.p;
    let aa = m(&a.x, &a.x, p)?;
    let bb = m(&a.y, &a.y, p)?;
    let z2 = m(&a.z, &a.z, p)?;
    let c = num::add_mod(&z2, &z2, p)?;
    let h = num::add_mod(&aa, &bb, p)?;
    let xy = num::add_mod(&a.x, &a.y, p)?;
    let e = num::sub_mod(&h, &m(&xy, &xy, p)?, p)?;
    let g = num::sub_mod(&aa, &bb, p)?;
    let f = num::add_mod(&c, &g, p)?;
    Ok(Pt { x: m(&e, &f, p)?, y: m(&g, &h, p)?, t: m(&e, &h, p)?, z: m(&f, &g, p)? })
}

fn identity() -> Pt {
    Pt { x: Zahl::null(), y: Zahl::eins(), z: Zahl::eins(), t: Zahl::null() }
}

fn scalar_mul(k: &Params, s: &Zahl, pt: &Pt) -> R<Pt> {
    let mut acc = identity();
    let bits = num::bits(s);
    let g = s.glieder();
    for i in (0..bits).rev() {
        acc = pt_double(k, &acc)?;
        if (g[i / 32] >> (i % 32)) & 1 == 1 {
            acc = pt_add(k, &acc, pt)?;
        }
    }
    Ok(acc)
}

fn base(k: &Params) -> R<Pt> {
    Ok(Pt { x: k.bx.clone(), y: k.by.clone(), z: Zahl::eins(), t: m(&k.bx, &k.by, &k.p)? })
}

fn le(b: &[u8]) -> R<Zahl> {
    let mut v = b.to_vec();
    v.reverse();
    num::from_bytes(&v)
}

fn to_le(z: &Zahl, n: usize) -> Vec<u8> {
    let mut v = z.nach_bytes_gross(n);
    v.reverse();
    v
}

fn encode(k: &Params, q: &Pt) -> R<[u8; 32]> {
    let p = &k.p;
    let pm2 = num::sub(p, &num::from_u64(2));
    let zi = num::pow_mod(&q.z, &pm2, p)?;
    let x = m(&q.x, &zi, p)?;
    let y = m(&q.y, &zi, p)?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&to_le(&y, 32));
    if crate::zahl::ist_ungerade(&x) {
        out[31] |= 0x80;
    }
    Ok(out)
}

fn decode(k: &Params, b: &[u8]) -> R<Pt> {
    if b.len() != 32 {
        return Err("Ed25519 point must be 32 bytes");
    }
    let p = &k.p;
    let sign = b[31] >> 7;
    let mut yb = b.to_vec();
    yb[31] &= 0x7f;
    let y = le(&yb)?;
    if num::ge(&y, p) {
        return Err("Ed25519 point is not canonical");
    }
    let one = Zahl::eins();
    let y2 = m(&y, &y, p)?;
    let u = num::sub_mod(&y2, &one, p)?;
    let v = num::add_mod(&m(&k.d, &y2, p)?, &one, p)?;
    // x = u v^3 (u v^7)^((p-5)/8)
    let v3 = m(&m(&v, &v, p)?, &v, p)?;
    let v7 = m(&m(&v3, &v3, p)?, &v, p)?;
    let e = num::div_rem(&num::sub(p, &num::from_u64(5)), &num::from_u64(8))?.0;
    let mut x = m(&m(&u, &v3, p)?, &num::pow_mod(&m(&u, &v7, p)?, &e, p)?, p)?;
    let vx2 = m(&v, &m(&x, &x, p)?, p)?;
    if !num::eq(&vx2, &u) {
        let neg_u = num::sub_mod(&Zahl::null(), &u, p)?;
        if num::eq(&vx2, &neg_u) {
            x = m(&x, &k.sqrt_m1, p)?;
        } else {
            return Err("Ed25519 point is not on the curve");
        }
    }
    if x.ist_null() && sign == 1 {
        return Err("Ed25519 point is not canonical");
    }
    if (crate::zahl::ist_ungerade(&x) as u8) != sign {
        x = num::sub(p, &x);
    }
    let t = m(&x, &y, p)?;
    Ok(Pt { x, y, z: one, t })
}

fn expand(seed: &[u8]) -> R<(Zahl, Vec<u8>)> {
    if seed.len() != 32 {
        return Err("Ed25519 private key must be 32 bytes");
    }
    let h = hash::digest(HashAlg::Sha512, seed);
    let mut a = h[..32].to_vec();
    a[0] &= 248;
    a[31] &= 127;
    a[31] |= 64;
    Ok((le(&a)?, h[32..].to_vec()))
}

/// The public key of a 32-byte private key (seed).
pub fn public_key(seed: &[u8]) -> R<[u8; 32]> {
    let k = params();
    let (a, _) = expand(seed)?;
    encode(&k, &scalar_mul(&k, &a, &base(&k)?)?)
}

/// A new key pair: (seed, public key).
pub fn generate() -> R<(Vec<u8>, [u8; 32])> {
    let seed = super::random(32)?;
    let pk = public_key(&seed)?;
    Ok((seed, pk))
}

/// Signature (64 bytes).
pub fn sign(seed: &[u8], msg: &[u8]) -> R<Vec<u8>> {
    let k = params();
    let (a, prefix) = expand(seed)?;
    let b = base(&k)?;
    let pk = encode(&k, &scalar_mul(&k, &a, &b)?)?;
    let r = num::rem(&le(&hash::digest_parts(HashAlg::Sha512, &[&prefix, msg]))?, &k.l)?;
    let rb = encode(&k, &scalar_mul(&k, &r, &b)?)?;
    let hk = num::rem(&le(&hash::digest_parts(HashAlg::Sha512, &[&rb, &pk, msg]))?, &k.l)?;
    let s = num::add_mod(&r, &num::mul_mod(&hk, &num::rem(&a, &k.l)?, &k.l)?, &k.l)?;
    let mut out = rb.to_vec();
    out.extend(to_le(&s, 32));
    Ok(out)
}

/// Verification (cofactorless, as RFC 8032 section 5.1.7 with
/// `[S]B = R + [k]A`).
pub fn verify(public: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let k = params();
    if sig.len() != 64 || public.len() != 32 {
        return false;
    }
    let f = || -> R<bool> {
        let a = decode(&k, public)?;
        let r = decode(&k, &sig[..32])?;
        // A small-order public key or R never verifies (as in the WPT
        // suite's expectations for Web Crypto).
        if small_order(&k, &a)? || small_order(&k, &r)? {
            return Ok(false);
        }
        let s = le(&sig[32..])?;
        if num::ge(&s, &k.l) {
            return Ok(false);
        }
        let hk = num::rem(&le(&hash::digest_parts(HashAlg::Sha512, &[&sig[..32], public, msg]))?, &k.l)?;
        let lhs = encode(&k, &scalar_mul(&k, &s, &base(&k)?)?)?;
        let rhs = encode(&k, &pt_add(&k, &r, &scalar_mul(&k, &hk, &a)?)?)?;
        Ok(lhs == rhs)
    };
    f().unwrap_or(false)
}

/// Does 8 * pt give the identity (a point of order 1, 2, 4 or 8)?
fn small_order(k: &Params, pt: &Pt) -> R<bool> {
    let mut q = pt.clone();
    for _ in 0..3 {
        q = pt_double(k, &q)?;
    }
    // Identity: X = 0 and Y = Z.
    Ok(q.x.ist_null() && num::eq(&num::rem(&q.y, &k.p)?, &num::rem(&q.z, &k.p)?))
}

/// Is `public` a valid encoded point?
pub fn valid_public(public: &[u8]) -> bool {
    decode(&params(), public).is_ok()
}

/// X25519 public key of a 32-byte private key.
pub fn x25519_public(private: &[u8]) -> R<[u8; 32]> {
    let k: [u8; 32] = private.try_into().map_err(|_| "X25519 private key must be 32 bytes")?;
    Ok(crate::x25519::oeffentlich(&k))
}

/// X25519 shared secret; an all-zero result (small-order point) is an
/// error.
pub fn x25519(private: &[u8], public: &[u8]) -> R<[u8; 32]> {
    let k: [u8; 32] = private.try_into().map_err(|_| "X25519 private key must be 32 bytes")?;
    let p: [u8; 32] = public.try_into().map_err(|_| "X25519 public key must be 32 bytes")?;
    crate::x25519::gemeinsam(&k, &p).map_err(|_| "X25519 result is all zero")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn rfc8032_test_1() {
        let seed = unhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let pk = public_key(&seed).unwrap();
        assert_eq!(pk.to_vec(), unhex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"));
        let sig = sign(&seed, b"").unwrap();
        assert_eq!(
            sig,
            unhex(
                "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
            )
        );
        assert!(verify(&pk, b"", &sig));
        assert!(!verify(&pk, b"x", &sig));
    }
}
