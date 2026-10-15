//! ECDSA (FIPS 186-4, signatures as `r || s`) and ECDH on the NIST curves
//! P-256, P-384 and P-521 (all of the form y^2 = x^3 - 3x + b).
//!
//! Points are computed in Jacobian coordinates with the generic big-number
//! type; the scalar multiplication is a plain double-and-add (not constant
//! time — the inputs are the page's own keys inside its own process).

use super::hash::{self, HashAlg};
use super::num;
use crate::zahl::Zahl;

/// The named curves of Web Crypto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveId {
    P256,
    P384,
    P521,
}

impl CurveId {
    pub fn from_name(name: &str) -> Option<CurveId> {
        Some(match name {
            "P-256" => CurveId::P256,
            "P-384" => CurveId::P384,
            "P-521" => CurveId::P521,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            CurveId::P256 => "P-256",
            CurveId::P384 => "P-384",
            CurveId::P521 => "P-521",
        }
    }

    /// Field element / scalar length in bytes.
    pub fn len(self) -> usize {
        match self {
            CurveId::P256 => 32,
            CurveId::P384 => 48,
            CurveId::P521 => 66,
        }
    }

    pub fn bits(self) -> usize {
        match self {
            CurveId::P256 => 256,
            CurveId::P384 => 384,
            CurveId::P521 => 521,
        }
    }
}

/// Domain parameters.
pub struct Curve {
    pub id: CurveId,
    pub p: Zahl,
    pub b: Zahl,
    pub n: Zahl,
    pub gx: Zahl,
    pub gy: Zahl,
}

fn h(s: &str) -> Zahl {
    let b: Vec<u8> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0)).collect();
    num::from_bytes(&b).unwrap_or_else(|_| Zahl::null())
}

pub fn curve(id: CurveId) -> Curve {
    match id {
        CurveId::P256 => Curve {
            id,
            p: h("ffffffff00000001000000000000000000000000ffffffffffffffffffffffff"),
            b: h("5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b"),
            n: h("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551"),
            gx: h("6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296"),
            gy: h("4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5"),
        },
        CurveId::P384 => Curve {
            id,
            p: h("fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffeffffffff0000000000000000ffffffff"),
            b: h("b3312fa7e23ee7e4988e056be3f82d19181d9c6efe8141120314088f5013875ac656398d8a2ed19d2a85c8edd3ec2aef"),
            n: h("ffffffffffffffffffffffffffffffffffffffffffffffffc7634d81f4372ddf581a0db248b0a77aecec196accc52973"),
            gx: h("aa87ca22be8b05378eb1c71ef320ad746e1d3b628ba79b9859f741e082542a385502f25dbf55296c3a545e3872760ab7"),
            gy: h("3617de4a96262c6f5d9e98bf9292dc29f8f41dbd289a147ce9da3113b5f0b8c00a60b1ce1d7e819d7a431d7c90ea0e5f"),
        },
        CurveId::P521 => Curve {
            id,
            p: h("01ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
            b: h("0051953eb9618e1c9a1f929a21a0b68540eea2da725b99b315f3b8b489918ef109e156193951ec7e937b1652c0bd3bb1bf073573df883d2c34f1ef451fd46b503f00"),
            n: h("01fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffa51868783bf2f966b7fcc0148f709a5d03bb5c9b8899c47aebb6fb71e91386409"),
            gx: h("00c6858e06b70404e9cd9e3ecb662395b4429c648139053fb521f828af606b4d3dbaa14b5e77efe75928fe1dc127a2ffa8de3348b3c1856a429bf97e7e31c2e5bd66"),
            gy: h("011839296a789a3bc0045c8a5fb42c7d1bd998f54449579b446817afbd17273e662c97ee72995ef42640c550b9013fad0761353c7086a272c24088be94769fd16650"),
        },
    }
}

/// A point in Jacobian coordinates (`z == 0` is the point at infinity).
#[derive(Clone)]
struct Jac {
    x: Zahl,
    y: Zahl,
    z: Zahl,
}

type R<T> = Result<T, &'static str>;

fn mm(a: &Zahl, b: &Zahl, p: &Zahl) -> R<Zahl> {
    num::mul_mod(a, b, p)
}

fn double(c: &Curve, q: &Jac) -> R<Jac> {
    let p = &c.p;
    if q.z.ist_null() || q.y.ist_null() {
        return Ok(Jac { x: Zahl::eins(), y: Zahl::eins(), z: Zahl::null() });
    }
    let delta = mm(&q.z, &q.z, p)?;
    let gamma = mm(&q.y, &q.y, p)?;
    let beta = mm(&q.x, &gamma, p)?;
    let t1 = num::sub_mod(&q.x, &delta, p)?;
    let t2 = num::add_mod(&q.x, &delta, p)?;
    let t3 = mm(&t1, &t2, p)?;
    let alpha = num::add_mod(&num::add_mod(&t3, &t3, p)?, &t3, p)?;
    let beta4 = {
        let b2 = num::add_mod(&beta, &beta, p)?;
        num::add_mod(&b2, &b2, p)?
    };
    let beta8 = num::add_mod(&beta4, &beta4, p)?;
    let x3 = num::sub_mod(&mm(&alpha, &alpha, p)?, &beta8, p)?;
    let yz = num::add_mod(&q.y, &q.z, p)?;
    let z3 = num::sub_mod(&num::sub_mod(&mm(&yz, &yz, p)?, &gamma, p)?, &delta, p)?;
    let g2 = mm(&gamma, &gamma, p)?;
    let g8 = {
        let a = num::add_mod(&g2, &g2, p)?;
        let b = num::add_mod(&a, &a, p)?;
        num::add_mod(&b, &b, p)?
    };
    let y3 = num::sub_mod(&mm(&alpha, &num::sub_mod(&beta4, &x3, p)?, p)?, &g8, p)?;
    Ok(Jac { x: x3, y: y3, z: z3 })
}

fn add(c: &Curve, a: &Jac, b: &Jac) -> R<Jac> {
    let p = &c.p;
    if a.z.ist_null() {
        return Ok(b.clone());
    }
    if b.z.ist_null() {
        return Ok(a.clone());
    }
    let z1z1 = mm(&a.z, &a.z, p)?;
    let z2z2 = mm(&b.z, &b.z, p)?;
    let u1 = mm(&a.x, &z2z2, p)?;
    let u2 = mm(&b.x, &z1z1, p)?;
    let s1 = mm(&mm(&a.y, &b.z, p)?, &z2z2, p)?;
    let s2 = mm(&mm(&b.y, &a.z, p)?, &z1z1, p)?;
    if num::eq(&u1, &u2) {
        if num::eq(&s1, &s2) {
            return double(c, a);
        }
        return Ok(Jac { x: Zahl::eins(), y: Zahl::eins(), z: Zahl::null() });
    }
    let hh = num::sub_mod(&u2, &u1, p)?;
    let r = num::sub_mod(&s2, &s1, p)?;
    let h2 = mm(&hh, &hh, p)?;
    let h3 = mm(&h2, &hh, p)?;
    let u1h2 = mm(&u1, &h2, p)?;
    let x3 = num::sub_mod(&num::sub_mod(&mm(&r, &r, p)?, &h3, p)?, &num::add_mod(&u1h2, &u1h2, p)?, p)?;
    let y3 = num::sub_mod(&mm(&r, &num::sub_mod(&u1h2, &x3, p)?, p)?, &mm(&s1, &h3, p)?, p)?;
    let z3 = mm(&mm(&hh, &a.z, p)?, &b.z, p)?;
    Ok(Jac { x: x3, y: y3, z: z3 })
}

/// A point in Jacobian coordinates over the fast Montgomery field.
#[derive(Clone, Copy)]
struct FJac {
    x: num::Fe,
    y: num::Fe,
    z: num::Fe,
}

fn f_double(f: &num::Fp, q: &FJac) -> FJac {
    if num::Fp::is_zero(&q.z) || num::Fp::is_zero(&q.y) {
        return FJac { x: q.x, y: q.y, z: [0; num::FE_LIMBS] };
    }
    // a = -3: alpha = 3 (X - Z^2)(X + Z^2).
    let delta = f.mul(&q.z, &q.z);
    let gamma = f.mul(&q.y, &q.y);
    let beta = f.mul(&q.x, &gamma);
    let t3 = f.mul(&f.sub(&q.x, &delta), &f.add(&q.x, &delta));
    let alpha = f.add(&f.add(&t3, &t3), &t3);
    let b2 = f.add(&beta, &beta);
    let beta4 = f.add(&b2, &b2);
    let beta8 = f.add(&beta4, &beta4);
    let x3 = f.sub(&f.mul(&alpha, &alpha), &beta8);
    let yz = f.add(&q.y, &q.z);
    let z3 = f.sub(&f.sub(&f.mul(&yz, &yz), &gamma), &delta);
    let g2 = f.mul(&gamma, &gamma);
    let g2x2 = f.add(&g2, &g2);
    let g2x4 = f.add(&g2x2, &g2x2);
    let g8 = f.add(&g2x4, &g2x4);
    let y3 = f.sub(&f.mul(&alpha, &f.sub(&beta4, &x3)), &g8);
    FJac { x: x3, y: y3, z: z3 }
}

fn f_add(f: &num::Fp, a: &FJac, b: &FJac) -> FJac {
    if num::Fp::is_zero(&a.z) {
        return *b;
    }
    if num::Fp::is_zero(&b.z) {
        return *a;
    }
    let z1z1 = f.mul(&a.z, &a.z);
    let z2z2 = f.mul(&b.z, &b.z);
    let u1 = f.mul(&a.x, &z2z2);
    let u2 = f.mul(&b.x, &z1z1);
    let s1 = f.mul(&f.mul(&a.y, &b.z), &z2z2);
    let s2 = f.mul(&f.mul(&b.y, &a.z), &z1z1);
    if u1 == u2 {
        if s1 == s2 {
            return f_double(f, a);
        }
        return FJac { x: a.x, y: a.y, z: [0; num::FE_LIMBS] };
    }
    let h = f.sub(&u2, &u1);
    let r = f.sub(&s2, &s1);
    let h2 = f.mul(&h, &h);
    let h3 = f.mul(&h2, &h);
    let u1h2 = f.mul(&u1, &h2);
    let x3 = f.sub(&f.sub(&f.mul(&r, &r), &h3), &f.add(&u1h2, &u1h2));
    let y3 = f.sub(&f.mul(&r, &f.sub(&u1h2, &x3)), &f.mul(&s1, &h3));
    let z3 = f.mul(&f.mul(&h, &a.z), &b.z);
    FJac { x: x3, y: y3, z: z3 }
}

/// k * pt with a 4-bit fixed window over the fast field.
fn scalar_mul(c: &Curve, k: &Zahl, pt: &Jac) -> R<Jac> {
    let f = num::Fp::new(&c.p).ok_or("curve field")?;
    let p = FJac { x: f.to_mont(&pt.x), y: f.to_mont(&pt.y), z: f.to_mont(&pt.z) };
    let inf = FJac { x: p.x, y: p.y, z: [0; num::FE_LIMBS] };
    let mut table = [inf; 16];
    table[1] = p;
    for i in 2..16 {
        table[i] = f_add(&f, &table[i - 1], &p);
    }
    let bits = num::bits(k);
    let g = k.glieder();
    let bit = |i: usize| -> usize { if i < bits { ((g[i / 32] >> (i % 32)) & 1) as usize } else { 0 } };
    let mut acc = inf;
    let mut i = bits.div_ceil(4) * 4;
    while i > 0 {
        i -= 4;
        for _ in 0..4 {
            acc = f_double(&f, &acc);
        }
        let w = (bit(i + 3) << 3) | (bit(i + 2) << 2) | (bit(i + 1) << 1) | bit(i);
        if w != 0 {
            acc = f_add(&f, &acc, &table[w]);
        }
    }
    if num::Fp::is_zero(&acc.z) {
        return Ok(Jac { x: Zahl::eins(), y: Zahl::eins(), z: Zahl::null() });
    }
    Ok(Jac { x: f.from_mont(&acc.x), y: f.from_mont(&acc.y), z: f.from_mont(&acc.z) })
}

fn affine(c: &Curve, q: &Jac) -> R<Option<(Zahl, Zahl)>> {
    if q.z.ist_null() {
        return Ok(None);
    }
    let p = &c.p;
    let pm2 = num::sub(p, &num::from_u64(2));
    let zi = num::pow_mod(&q.z, &pm2, p)?;
    let zi2 = mm(&zi, &zi, p)?;
    let zi3 = mm(&zi2, &zi, p)?;
    Ok(Some((mm(&q.x, &zi2, p)?, mm(&q.y, &zi3, p)?)))
}

fn jac(x: &Zahl, y: &Zahl) -> Jac {
    Jac { x: x.clone(), y: y.clone(), z: Zahl::eins() }
}

/// y^2 == x^3 - 3x + b (mod p), coordinates in range.
pub fn on_curve(c: &Curve, x: &Zahl, y: &Zahl) -> bool {
    if num::ge(x, &c.p) || num::ge(y, &c.p) {
        return false;
    }
    let p = &c.p;
    let f = || -> R<bool> {
        let y2 = mm(y, y, p)?;
        let x3 = mm(&mm(x, x, p)?, x, p)?;
        let three_x = num::add_mod(&num::add_mod(x, x, p)?, x, p)?;
        let rhs = num::add_mod(&num::sub_mod(&x3, &three_x, p)?, &c.b, p)?;
        Ok(num::eq(&y2, &rhs))
    };
    f().unwrap_or(false)
}

/// Decode an uncompressed (0x04) or compressed (0x02/0x03) point.
pub fn decode_point(c: &Curve, data: &[u8]) -> R<(Zahl, Zahl)> {
    let l = c.id.len();
    match data.first() {
        Some(4) if data.len() == 1 + 2 * l => {
            let x = num::from_bytes(&data[1..1 + l])?;
            let y = num::from_bytes(&data[1 + l..])?;
            if !on_curve(c, &x, &y) {
                return Err("point is not on the curve");
            }
            Ok((x, y))
        }
        Some(&t) if (t == 2 || t == 3) && data.len() == 1 + l => {
            let x = num::from_bytes(&data[1..])?;
            if num::ge(&x, &c.p) {
                return Err("point is not on the curve");
            }
            let p = &c.p;
            let x3 = mm(&mm(&x, &x, p)?, &x, p)?;
            let three_x = num::add_mod(&num::add_mod(&x, &x, p)?, &x, p)?;
            let rhs = num::add_mod(&num::sub_mod(&x3, &three_x, p)?, &c.b, p)?;
            // p = 3 (mod 4) for all three curves: sqrt = rhs^((p+1)/4).
            let e = num::div_rem(&num::add(p, &Zahl::eins()), &num::from_u64(4))?.0;
            let mut y = num::pow_mod(&rhs, &e, p)?;
            if !num::eq(&mm(&y, &y, p)?, &rhs) {
                return Err("point is not on the curve");
            }
            if (crate::zahl::ist_ungerade(&y) as u8) != (t & 1) {
                y = num::sub(p, &y);
            }
            Ok((x, y))
        }
        _ => Err("point encoding is invalid"),
    }
}

/// Compressed encoding 0x02/0x03 || x.
pub fn encode_point_compressed(c: &Curve, x: &Zahl, y: &Zahl) -> Vec<u8> {
    let mut out = vec![if crate::zahl::ist_ungerade(y) { 3u8 } else { 2u8 }];
    out.extend(x.nach_bytes_gross(c.id.len()));
    out
}

/// A key pair: (private scalar bytes, public x, public y).
pub fn generate(id: CurveId) -> R<(Vec<u8>, Zahl, Zahl)> {
    let c = curve(id);
    let d = num::random_below(&c.n)?;
    let (x, y) = public_from_private(&c, &d)?;
    Ok((d.nach_bytes_gross(id.len()), x, y))
}

pub fn public_from_private(c: &Curve, d: &Zahl) -> R<(Zahl, Zahl)> {
    if d.ist_null() || num::ge(d, &c.n) {
        return Err("private key out of range");
    }
    let q = scalar_mul(c, d, &jac(&c.gx, &c.gy))?;
    affine(c, &q)?.ok_or("private key gives the point at infinity")
}

/// The hash as an integer: its leftmost `bits(n)` bits.
fn hash_int(c: &Curve, digest: &[u8]) -> R<Zahl> {
    let nbits = num::bits(&c.n);
    let mut e = num::from_bytes(digest)?;
    let hbits = digest.len() * 8;
    if hbits > nbits {
        let shift = hbits - nbits;
        let two = num::from_u64(1u64 << (shift % 32).min(31));
        // Shift right by `shift` bits: divide by 2^shift.
        let mut q = e;
        for _ in 0..shift / 32 {
            q = num::div_rem(&q, &num::from_u64(1u64 << 32))?.0;
        }
        if shift % 32 != 0 {
            q = num::div_rem(&q, &two)?.0;
        }
        e = q;
    }
    Ok(e)
}

/// ECDSA signature `r || s` over `msg` hashed with `hash_alg`.
pub fn sign(id: CurveId, d: &[u8], hash_alg: HashAlg, msg: &[u8]) -> R<Vec<u8>> {
    let c = curve(id);
    let d = num::from_bytes(d)?;
    if d.ist_null() || num::ge(&d, &c.n) {
        return Err("private key out of range");
    }
    let e = hash_int(&c, &hash::digest(hash_alg, msg))?;
    let g = jac(&c.gx, &c.gy);
    for _ in 0..64 {
        let k = num::random_below(&c.n)?;
        let Some((x1, _)) = affine(&c, &scalar_mul(&c, &k, &g)?)? else { continue };
        let r = num::rem(&x1, &c.n)?;
        if r.ist_null() {
            continue;
        }
        let Some(kinv) = num::inv_mod(&k, &c.n) else { continue };
        let rd = num::mul_mod(&r, &d, &c.n)?;
        let s = num::mul_mod(&kinv, &num::add_mod(&num::rem(&e, &c.n)?, &rd, &c.n)?, &c.n)?;
        if s.ist_null() {
            continue;
        }
        let mut out = r.nach_bytes_gross(id.len());
        out.extend(s.nach_bytes_gross(id.len()));
        return Ok(out);
    }
    Err("ECDSA signing failed")
}

/// ECDSA verification of `r || s`.
pub fn verify(id: CurveId, qx: &Zahl, qy: &Zahl, hash_alg: HashAlg, msg: &[u8], sig: &[u8]) -> bool {
    let c = curve(id);
    let l = id.len();
    if sig.len() != 2 * l {
        return false;
    }
    let f = || -> R<bool> {
        let r = num::from_bytes(&sig[..l])?;
        let s = num::from_bytes(&sig[l..])?;
        if r.ist_null() || s.ist_null() || num::ge(&r, &c.n) || num::ge(&s, &c.n) {
            return Ok(false);
        }
        let e = hash_int(&c, &hash::digest(hash_alg, msg))?;
        let w = num::inv_mod(&s, &c.n).ok_or("inverse")?;
        let u1 = num::mul_mod(&num::rem(&e, &c.n)?, &w, &c.n)?;
        let u2 = num::mul_mod(&r, &w, &c.n)?;
        let p1 = scalar_mul(&c, &u1, &jac(&c.gx, &c.gy))?;
        let p2 = scalar_mul(&c, &u2, &jac(qx, qy))?;
        let Some((x, _)) = affine(&c, &add(&c, &p1, &p2)?)? else { return Ok(false) };
        Ok(num::eq(&num::rem(&x, &c.n)?, &r))
    };
    f().unwrap_or(false)
}

/// ECDH: the x coordinate of d * Q.
pub fn ecdh(id: CurveId, d: &[u8], qx: &Zahl, qy: &Zahl) -> R<Vec<u8>> {
    let c = curve(id);
    let d = num::from_bytes(d)?;
    if !on_curve(&c, qx, qy) {
        return Err("public key is not on the curve");
    }
    let s = scalar_mul(&c, &d, &jac(qx, qy))?;
    let (x, _) = affine(&c, &s)?.ok_or("ECDH result is the point at infinity")?;
    Ok(x.nach_bytes_gross(id.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generators_are_on_their_curves() {
        for id in [CurveId::P256, CurveId::P384, CurveId::P521] {
            let c = curve(id);
            assert!(on_curve(&c, &c.gx, &c.gy), "{}", id.name());
        }
    }

    #[test]
    fn sign_verify_and_ecdh_all_curves() {
        for id in [CurveId::P256, CurveId::P384, CurveId::P521] {
            let (d, x, y) = generate(id).unwrap();
            let s = sign(id, &d, HashAlg::Sha256, b"msg").unwrap();
            assert!(verify(id, &x, &y, HashAlg::Sha256, b"msg", &s), "{}", id.name());
            assert!(!verify(id, &x, &y, HashAlg::Sha256, b"msh", &s));
            let (d2, x2, y2) = generate(id).unwrap();
            assert_eq!(ecdh(id, &d, &x2, &y2).unwrap(), ecdh(id, &d2, &x, &y).unwrap());
            let c = curve(id);
            let comp = encode_point_compressed(&c, &x, &y);
            let (xd, yd) = decode_point(&c, &comp).unwrap();
            assert!(num::eq(&xd, &x) && num::eq(&yd, &y));
        }
    }
}
