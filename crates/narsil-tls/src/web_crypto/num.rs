//! Big-number helpers on top of `crate::zahl::Zahl` for RSA and elliptic
//! curves: bit length, modular inverse, random numbers and primes.

use crate::zahl::{self, Zahl};

pub fn from_u64(v: u64) -> Zahl {
    Zahl::aus_gliedern(vec![v as u32, (v >> 32) as u32])
}

pub fn from_bytes(b: &[u8]) -> Result<Zahl, &'static str> {
    Zahl::aus_bytes_gross(b).map_err(|_| "number too large")
}

/// Big-endian bytes without leading zeros (at least one byte).
pub fn to_bytes_min(z: &Zahl) -> Vec<u8> {
    let n = bits(z).div_ceil(8).max(1);
    z.nach_bytes_gross(n)
}

pub fn bits(z: &Zahl) -> usize {
    let g = z.glieder();
    for i in (0..g.len()).rev() {
        if g[i] != 0 {
            return i * 32 + (32 - g[i].leading_zeros() as usize);
        }
    }
    0
}

pub fn is_zero(z: &Zahl) -> bool {
    z.ist_null()
}

pub fn eq(a: &Zahl, b: &Zahl) -> bool {
    a.cmp(b) == std::cmp::Ordering::Equal
}

pub fn ge(a: &Zahl, b: &Zahl) -> bool {
    zahl::ist_groesser_gleich(a, b)
}

pub fn add(a: &Zahl, b: &Zahl) -> Zahl {
    zahl::plus(a, b)
}

/// a - b for a >= b.
pub fn sub(a: &Zahl, b: &Zahl) -> Zahl {
    zahl::minus(a, b)
}

pub fn mul(a: &Zahl, b: &Zahl) -> Zahl {
    zahl::mal(a, b)
}

pub fn rem(a: &Zahl, m: &Zahl) -> Result<Zahl, &'static str> {
    zahl::modulo(a, m).map_err(|_| "modulus")
}

pub fn div_rem(a: &Zahl, b: &Zahl) -> Result<(Zahl, Zahl), &'static str> {
    zahl::teile(a, b).map_err(|_| "division")
}

pub fn mul_mod(a: &Zahl, b: &Zahl, m: &Zahl) -> Result<Zahl, &'static str> {
    zahl::mal_modulo(a, b, m).map_err(|_| "modulus")
}

pub fn add_mod(a: &Zahl, b: &Zahl, m: &Zahl) -> Result<Zahl, &'static str> {
    zahl::plus_modulo(a, b, m).map_err(|_| "modulus")
}

pub fn sub_mod(a: &Zahl, b: &Zahl, m: &Zahl) -> Result<Zahl, &'static str> {
    zahl::minus_modulo(a, b, m).map_err(|_| "modulus")
}

pub fn pow_mod(b: &Zahl, e: &Zahl, m: &Zahl) -> Result<Zahl, &'static str> {
    zahl::potenz_modulo(b, e, m).map_err(|_| "modulus")
}

/// a^-1 mod m (extended Euclid; `None` when gcd(a, m) != 1).
pub fn inv_mod(a: &Zahl, m: &Zahl) -> Option<Zahl> {
    // Invariants: r0 = s0 * a (mod m), r1 = s1 * a (mod m); the
    // coefficients are kept as (magnitude, negative).
    let mut r0 = m.clone();
    let mut r1 = rem(a, m).ok()?;
    let mut s0 = (Zahl::null(), false);
    let mut s1 = (Zahl::eins(), false);
    while !r1.ist_null() {
        let (q, r) = div_rem(&r0, &r1).ok()?;
        // s2 = s0 - q * s1
        let qs1 = (mul(&q, &s1.0), s1.1);
        let s2 = signed_sub(&s0, &qs1);
        r0 = r1;
        r1 = r;
        s0 = s1;
        s1 = s2;
    }
    if !eq(&r0, &Zahl::eins()) {
        return None;
    }
    let mag = rem(&s0.0, m).ok()?;
    if s0.1 && !mag.ist_null() {
        Some(sub(m, &mag))
    } else {
        Some(mag)
    }
}

fn signed_sub(a: &(Zahl, bool), b: &(Zahl, bool)) -> (Zahl, bool) {
    // a - b = a + (-b)
    let nb = (b.0.clone(), !b.1);
    signed_add(a, &nb)
}

fn signed_add(a: &(Zahl, bool), b: &(Zahl, bool)) -> (Zahl, bool) {
    if a.1 == b.1 {
        return (add(&a.0, &b.0), a.1);
    }
    if ge(&a.0, &b.0) {
        (sub(&a.0, &b.0), a.1)
    } else {
        (sub(&b.0, &a.0), b.1)
    }
}

pub fn gcd(a: &Zahl, b: &Zahl) -> Zahl {
    let mut x = a.clone();
    let mut y = b.clone();
    while !y.ist_null() {
        let r = rem(&x, &y).unwrap_or_else(|_| Zahl::null());
        x = y;
        y = r;
    }
    x
}

/// A uniformly random number in 1 .. n-1.
pub fn random_below(n: &Zahl) -> Result<Zahl, &'static str> {
    let nb = bits(n);
    if nb < 2 {
        return Err("range too small");
    }
    let len = nb.div_ceil(8);
    let extra = len * 8 - nb;
    for _ in 0..1000 {
        let mut b = super::random(len)?;
        b[0] &= 0xffu8 >> extra;
        let z = from_bytes(&b)?;
        if !z.ist_null() && !ge(&z, n) {
            return Ok(z);
        }
    }
    Err("no random number found")
}

/// Montgomery arithmetic with 64-bit limbs for one odd modulus — the hot
/// path of RSA (private operation, prime search). About four times fewer
/// limb operations than the generic 32-bit path, no allocation per
/// multiplication, and a 4-bit window for exponentiation.
pub struct Mont {
    m: Vec<u64>,
    n0: u64,
    r2: Vec<u64>,
}

fn limbs64(z: &Zahl, k: usize) -> Vec<u64> {
    let g = z.glieder();
    let mut out = vec![0u64; k];
    for (i, w) in g.iter().enumerate() {
        if i / 2 < k {
            out[i / 2] |= (*w as u64) << (32 * (i % 2));
        }
    }
    out
}

fn from_limbs64(v: &[u64]) -> Zahl {
    let mut g = Vec::with_capacity(v.len() * 2);
    for w in v {
        g.push(*w as u32);
        g.push((*w >> 32) as u32);
    }
    Zahl::aus_gliedern(g)
}

impl Mont {
    pub fn new(m: &Zahl) -> Option<Mont> {
        if !crate::zahl::ist_ungerade(m) {
            return None;
        }
        let k = bits(m).div_ceil(64).max(1);
        let ml = limbs64(m, k);
        let mut inv: u64 = 1;
        for _ in 0..6 {
            inv = inv.wrapping_mul(2u64.wrapping_sub(ml[0].wrapping_mul(inv)));
        }
        // R^2 mod m by doubling 1 (2 * 64k) times.
        let mut x = vec![0u64; k];
        x[0] = 1;
        for _ in 0..128 * k {
            let mut carry = 0u64;
            for w in x.iter_mut() {
                let nc = *w >> 63;
                *w = (*w << 1) | carry;
                carry = nc;
            }
            if carry == 1 || !lt(&x, &ml) {
                sub_in(&mut x, &ml);
            }
        }
        Some(Mont { m: ml, n0: inv.wrapping_neg(), r2: x })
    }

    fn k(&self) -> usize {
        self.m.len()
    }

    /// out = a * b * R^-1 mod m (CIOS); `t` is scratch of k + 2 limbs.
    fn mul(&self, a: &[u64], b: &[u64], out: &mut [u64], t: &mut [u64]) {
        let k = self.k();
        let m = &self.m;
        for x in t.iter_mut() {
            *x = 0;
        }
        for &bi in b.iter().take(k) {
            let mut c: u128 = 0;
            for j in 0..k {
                let s = t[j] as u128 + a[j] as u128 * bi as u128 + c;
                t[j] = s as u64;
                c = s >> 64;
            }
            let s = t[k] as u128 + c;
            t[k] = s as u64;
            t[k + 1] = (s >> 64) as u64;
            let mf = t[0].wrapping_mul(self.n0);
            let mut c = (t[0] as u128 + m[0] as u128 * mf as u128) >> 64;
            for j in 1..k {
                let s = t[j] as u128 + m[j] as u128 * mf as u128 + c;
                t[j - 1] = s as u64;
                c = s >> 64;
            }
            let s = t[k] as u128 + c;
            t[k - 1] = s as u64;
            t[k] = t[k + 1] + (s >> 64) as u64;
            t[k + 1] = 0;
        }
        out.copy_from_slice(&t[..k]);
        if t[k] != 0 || !lt(out, m) {
            sub_in(out, m);
        }
    }

    /// base^exp mod m.
    pub fn pow(&self, base: &Zahl, exp: &Zahl) -> Zahl {
        let k = self.k();
        let mut t = vec![0u64; k + 2];
        let b = rem(base, &from_limbs64(&self.m)).unwrap_or_else(|_| Zahl::null());
        let bl = limbs64(&b, k);
        let mut one = vec![0u64; k];
        one[0] = 1;
        // Table of base^i in Montgomery form, i = 0..16.
        let mut table = vec![vec![0u64; k]; 16];
        let mut tmp = vec![0u64; k];
        self.mul(&one, &self.r2, &mut tmp, &mut t);
        table[0] = tmp.clone();
        self.mul(&bl, &self.r2, &mut tmp, &mut t);
        table[1] = tmp.clone();
        for i in 2..16 {
            let (lo, hi) = table.split_at_mut(i);
            self.mul(&lo[i - 1], &lo[1], &mut hi[0], &mut t);
        }
        let mut acc = table[0].clone();
        let eb = bits(exp);
        let g = exp.glieder();
        let bit = |i: usize| -> usize { ((g[i / 32] >> (i % 32)) & 1) as usize };
        let mut i = eb.div_ceil(4) * 4;
        let mut sq = vec![0u64; k];
        while i > 0 {
            i -= 4;
            for _ in 0..4 {
                self.mul(&acc, &acc, &mut sq, &mut t);
                std::mem::swap(&mut acc, &mut sq);
            }
            let mut w = 0usize;
            for j in (0..4).rev() {
                let idx = i + j;
                w = (w << 1) | if idx < eb { bit(idx) } else { 0 };
            }
            if w != 0 {
                self.mul(&acc, &table[w], &mut sq, &mut t);
                std::mem::swap(&mut acc, &mut sq);
            }
        }
        self.mul(&acc, &one, &mut sq, &mut t);
        from_limbs64(&sq)
    }
}

/// Limbs of a field element (enough for P-521: 9 x 64 = 576 bits).
pub const FE_LIMBS: usize = 9;
/// A field element in Montgomery form (unused high limbs are zero).
pub type Fe = [u64; FE_LIMBS];

/// Prime-field arithmetic for the elliptic curves: Montgomery form with
/// fixed-size limb arrays (no allocation per operation).
pub struct Fp {
    m: Fe,
    n0: u64,
    r2: Fe,
    k: usize,
}

impl Fp {
    pub fn new(p: &Zahl) -> Option<Fp> {
        let mont = Mont::new(p)?;
        let k = mont.m.len();
        if k > FE_LIMBS {
            return None;
        }
        let mut m = [0u64; FE_LIMBS];
        m[..k].copy_from_slice(&mont.m);
        let mut r2 = [0u64; FE_LIMBS];
        r2[..k].copy_from_slice(&mont.r2);
        Some(Fp { m, n0: mont.n0, r2, k })
    }

    pub fn mul(&self, a: &Fe, b: &Fe) -> Fe {
        let k = self.k;
        let m = &self.m;
        let mut t = [0u64; FE_LIMBS + 2];
        for &bi in b.iter().take(k) {
            let mut c: u128 = 0;
            for j in 0..k {
                let s = t[j] as u128 + a[j] as u128 * bi as u128 + c;
                t[j] = s as u64;
                c = s >> 64;
            }
            let s = t[k] as u128 + c;
            t[k] = s as u64;
            t[k + 1] = (s >> 64) as u64;
            let mf = t[0].wrapping_mul(self.n0);
            let mut c = (t[0] as u128 + m[0] as u128 * mf as u128) >> 64;
            for j in 1..k {
                let s = t[j] as u128 + m[j] as u128 * mf as u128 + c;
                t[j - 1] = s as u64;
                c = s >> 64;
            }
            let s = t[k] as u128 + c;
            t[k - 1] = s as u64;
            t[k] = t[k + 1] + (s >> 64) as u64;
            t[k + 1] = 0;
        }
        let mut out = [0u64; FE_LIMBS];
        out[..k].copy_from_slice(&t[..k]);
        if t[k] != 0 || !lt(&out[..k], &m[..k]) {
            sub_in(&mut out[..k], &m[..k]);
        }
        out
    }

    pub fn add(&self, a: &Fe, b: &Fe) -> Fe {
        let k = self.k;
        let mut out = [0u64; FE_LIMBS];
        let mut carry = 0u64;
        for i in 0..k {
            let (s1, o1) = a[i].overflowing_add(b[i]);
            let (s2, o2) = s1.overflowing_add(carry);
            out[i] = s2;
            carry = (o1 || o2) as u64;
        }
        if carry == 1 || !lt(&out[..k], &self.m[..k]) {
            sub_in(&mut out[..k], &self.m[..k]);
        }
        out
    }

    pub fn sub(&self, a: &Fe, b: &Fe) -> Fe {
        let k = self.k;
        let mut out = [0u64; FE_LIMBS];
        let mut borrow = 0u64;
        for i in 0..k {
            let (d1, o1) = a[i].overflowing_sub(b[i]);
            let (d2, o2) = d1.overflowing_sub(borrow);
            out[i] = d2;
            borrow = (o1 || o2) as u64;
        }
        if borrow == 1 {
            let mut carry = 0u64;
            for i in 0..k {
                let (s1, o1) = out[i].overflowing_add(self.m[i]);
                let (s2, o2) = s1.overflowing_add(carry);
                out[i] = s2;
                carry = (o1 || o2) as u64;
            }
        }
        out
    }

    /// Into Montgomery form (the integer must be below p).
    pub fn to_mont(&self, z: &Zahl) -> Fe {
        let l = limbs64(z, self.k);
        let mut a = [0u64; FE_LIMBS];
        a[..self.k].copy_from_slice(&l);
        self.mul(&a, &self.r2)
    }

    pub fn from_mont(&self, a: &Fe) -> Zahl {
        let mut one = [0u64; FE_LIMBS];
        one[0] = 1;
        let r = self.mul(a, &one);
        from_limbs64(&r[..self.k])
    }

    pub fn is_zero(a: &Fe) -> bool {
        a.iter().all(|&x| x == 0)
    }
}

fn lt(a: &[u64], b: &[u64]) -> bool {
    for i in (0..a.len()).rev() {
        if a[i] != b[i] {
            return a[i] < b[i];
        }
    }
    false
}

fn sub_in(a: &mut [u64], b: &[u64]) {
    let mut borrow = 0u64;
    for i in 0..a.len() {
        let (d1, o1) = a[i].overflowing_sub(b[i]);
        let (d2, o2) = d1.overflowing_sub(borrow);
        a[i] = d2;
        borrow = (o1 || o2) as u64;
    }
}

/// z mod p for a small p (no allocation).
pub fn rem_u32(z: &Zahl, p: u32) -> u32 {
    let mut r: u64 = 0;
    for w in z.glieder().iter().rev() {
        r = ((r << 32) | *w as u64) % p as u64;
    }
    r as u32
}

/// The odd primes below 4096 (sieve of Eratosthenes, computed once).
fn small_primes() -> &'static [u32] {
    use std::sync::OnceLock;
    static P: OnceLock<Vec<u32>> = OnceLock::new();
    P.get_or_init(|| {
        let n = 4096usize;
        let mut sieve = vec![true; n];
        let mut out = Vec::new();
        for i in 2..n {
            if sieve[i] {
                if i > 2 {
                    out.push(i as u32);
                }
                let mut j = i * i;
                while j < n {
                    sieve[j] = false;
                    j += i;
                }
            }
        }
        out
    })
}

/// Miller-Rabin with the given bases (`n` odd, larger than the bases).
pub fn miller_rabin(n: &Zahl, bases: &[u32]) -> bool {
    let Some(mont) = Mont::new(n) else { return false };
    let one = Zahl::eins();
    let nm1 = sub(n, &one);
    let mut d = nm1.clone();
    let mut s = 0usize;
    while !crate::zahl::ist_ungerade(&d) {
        d = div_rem(&d, &from_u64(2)).map(|x| x.0).unwrap_or_else(|_| Zahl::null());
        s += 1;
        if s > 8192 {
            return false;
        }
    }
    let two = from_u64(2);
    'bases: for &b in bases {
        let mut x = mont.pow(&from_u64(b as u64), &d);
        if eq(&x, &one) || eq(&x, &nm1) {
            continue;
        }
        for _ in 1..s {
            x = mont.pow(&x, &two);
            if eq(&x, &nm1) {
                continue 'bases;
            }
            if eq(&x, &one) {
                return false;
            }
        }
        return false;
    }
    true
}

/// A random prime of exactly `bits_wanted` bits with the top two bits set
/// (so that the product of two such primes has twice the bits), with
/// gcd(p - 1, e) = 1. Candidates come from a random start and its odd
/// successors, sieved with the small primes before any Miller-Rabin test.
pub fn random_prime(bits_wanted: usize, e: &Zahl) -> Result<Zahl, &'static str> {
    if bits_wanted < 16 {
        return Err("prime too small");
    }
    let len = bits_wanted.div_ceil(8);
    let extra = len * 8 - bits_wanted;
    let one = Zahl::eins();
    let primes = small_primes();
    // Rounds for random candidates (FIPS 186-5 appendix B.3 needs fewer
    // for these sizes); the bases are the first primes.
    let bases: &[u32] = if bits_wanted >= 1024 { &[2, 3, 5, 7, 11] } else { &[2, 3, 5, 7, 11, 13, 17, 19, 23, 29] };
    for _ in 0..10_000 {
        let mut b = super::random(len)?;
        b[0] &= 0xffu8 >> extra;
        let top = 7 - extra;
        b[0] |= 1 << top;
        if top > 0 {
            b[0] |= 1 << (top - 1);
        } else if len > 1 {
            b[1] |= 0x80;
        }
        let last = len - 1;
        b[last] |= 1;
        let start = from_bytes(&b)?;
        let residues: Vec<u32> = primes.iter().map(|&p| rem_u32(&start, p)).collect();
        let mut delta: u32 = 0;
        while delta < 1 << 16 {
            let composite = primes
                .iter()
                .zip(residues.iter())
                .any(|(&p, &r)| (r as u64 + delta as u64) % p as u64 == 0);
            if !composite {
                let p = add(&start, &from_u64(delta as u64));
                if bits(&p) != bits_wanted {
                    break;
                }
                let pm1 = sub(&p, &one);
                if eq(&gcd(&pm1, e), &one) && miller_rabin(&p, bases) {
                    return Ok(p);
                }
            }
            delta += 2;
        }
    }
    Err("no prime found")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_and_bits() {
        let a = from_u64(17);
        let m = from_u64(3120);
        let i = inv_mod(&a, &m).unwrap();
        assert!(eq(&i, &from_u64(2753)));
        assert_eq!(bits(&from_u64(0x8000)), 16);
        assert!(inv_mod(&from_u64(6), &from_u64(9)).is_none());
    }

    #[test]
    fn montgomery_matches_the_generic_path() {
        let m = from_bytes(&[0xc3, 0x5f, 0x11, 0x02, 0x99, 0x01, 0xab, 0xcd, 0xef, 0x13, 0x37, 0x42, 0x01, 0x07]).unwrap();
        let b = from_u64(0x1234_5678_9abc);
        let e = from_u64(0xdead_beef_1234);
        let mont = Mont::new(&m).unwrap();
        assert!(eq(&mont.pow(&b, &e), &pow_mod(&b, &e, &m).unwrap()));
        assert!(miller_rabin(&from_u64(2_147_483_647), &[2, 3, 5, 7]));
        assert!(!miller_rabin(&from_u64(2_147_483_649), &[2, 3, 5, 7]));
    }
}
