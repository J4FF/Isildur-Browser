//! AES modes of Web Crypto: CBC with PKCS#7 padding, CTR with a counter
//! of `length` bits, GCM with any IV length and tag lengths 32..128 bits
//! (NIST SP 800-38D), and AES Key Wrap (RFC 3394).

use crate::aes::Aes;

fn cipher(key: &[u8]) -> Result<Aes, &'static str> {
    Aes::neu_web(key).ok_or("AES key must be 16, 24 or 32 bytes")
}

/// AES-CBC encryption with PKCS#7 padding.
pub fn cbc_encrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let aes = cipher(key)?;
    if iv.len() != 16 {
        return Err("AES-CBC needs a 16-byte IV");
    }
    let pad = 16 - data.len() % 16;
    let mut out = Vec::with_capacity(data.len() + pad);
    let mut prev = [0u8; 16];
    prev.copy_from_slice(iv);
    let mut padded = data.to_vec();
    padded.extend(std::iter::repeat_n(pad as u8, pad));
    for chunk in padded.chunks(16) {
        let mut block = [0u8; 16];
        for i in 0..16 {
            block[i] = chunk[i] ^ prev[i];
        }
        aes.verschluessel(&mut block);
        out.extend_from_slice(&block);
        prev = block;
    }
    Ok(out)
}

/// AES-CBC decryption, checking and removing PKCS#7 padding.
pub fn cbc_decrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let aes = cipher(key)?;
    if iv.len() != 16 {
        return Err("AES-CBC needs a 16-byte IV");
    }
    if data.is_empty() || data.len() % 16 != 0 {
        return Err("AES-CBC ciphertext is not a multiple of 16 bytes");
    }
    let mut out = Vec::with_capacity(data.len());
    let mut prev = [0u8; 16];
    prev.copy_from_slice(iv);
    for chunk in data.chunks(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        let saved = block;
        aes.entschluessel(&mut block);
        for i in 0..16 {
            block[i] ^= prev[i];
        }
        out.extend_from_slice(&block);
        prev = saved;
    }
    let pad = *out.last().unwrap_or(&0) as usize;
    if pad == 0 || pad > 16 || out[out.len() - pad..].iter().any(|&b| b as usize != pad) {
        return Err("AES-CBC padding is invalid");
    }
    out.truncate(out.len() - pad);
    Ok(out)
}

/// AES-CTR (encryption and decryption are the same). The rightmost
/// `length` bits of `counter` count blocks; reusing a counter value is an
/// error.
pub fn ctr(key: &[u8], counter: &[u8], length: u32, data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let aes = cipher(key)?;
    if counter.len() != 16 {
        return Err("AES-CTR needs a 16-byte counter");
    }
    if length == 0 || length > 128 {
        return Err("AES-CTR counter length must be 1..128");
    }
    let blocks = data.len().div_ceil(16) as u128;
    if length < 128 && blocks > (1u128 << length) {
        return Err("AES-CTR counter would wrap");
    }
    let mask: u128 = if length == 128 { u128::MAX } else { (1u128 << length) - 1 };
    let start = u128::from_be_bytes(counter.try_into().map_err(|_| "counter")?);
    let fixed = start & !mask;
    let mut count = start & mask;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut block = (fixed | count).to_be_bytes();
        aes.verschluessel(&mut block);
        for (i, b) in chunk.iter().enumerate() {
            out.push(b ^ block[i]);
        }
        count = count.wrapping_add(1) & mask;
    }
    Ok(out)
}

/// Multiplication in GF(2^128) with the GCM polynomial.
fn gf_mul(x: u128, y: u128) -> u128 {
    const R: u128 = 0xe1 << 120;
    let mut z = 0u128;
    let mut v = y;
    for i in 0..128 {
        if (x >> (127 - i)) & 1 == 1 {
            z ^= v;
        }
        v = if v & 1 == 1 { (v >> 1) ^ R } else { v >> 1 };
    }
    z
}

fn ghash(h: u128, aad: &[u8], data: &[u8]) -> u128 {
    let mut y = 0u128;
    for part in [aad, data] {
        for chunk in part.chunks(16) {
            let mut block = [0u8; 16];
            block[..chunk.len()].copy_from_slice(chunk);
            y = gf_mul(y ^ u128::from_be_bytes(block), h);
        }
    }
    let lens = ((aad.len() as u128 * 8) << 64) | (data.len() as u128 * 8);
    gf_mul(y ^ lens, h)
}

fn gcm_setup(aes: &Aes, iv: &[u8]) -> (u128, u128) {
    let mut hb = [0u8; 16];
    aes.verschluessel(&mut hb);
    let h = u128::from_be_bytes(hb);
    let j0 = if iv.len() == 12 {
        let mut b = [0u8; 16];
        b[..12].copy_from_slice(iv);
        b[15] = 1;
        u128::from_be_bytes(b)
    } else {
        ghash(h, &[], iv)
    };
    (h, j0)
}

fn gcm_stream(aes: &Aes, j0: u128, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut ctr = j0;
    for chunk in data.chunks(16) {
        // inc32: only the low 32 bits count.
        ctr = (ctr & !0xffff_ffffu128) | ((ctr as u32).wrapping_add(1) as u128);
        let mut block = ctr.to_be_bytes();
        aes.verschluessel(&mut block);
        for (i, b) in chunk.iter().enumerate() {
            out.push(b ^ block[i]);
        }
    }
    out
}

fn gcm_tag(aes: &Aes, h: u128, j0: u128, aad: &[u8], ct: &[u8]) -> [u8; 16] {
    let s = ghash(h, aad, ct);
    let mut e = j0.to_be_bytes();
    aes.verschluessel(&mut e);
    (u128::from_be_bytes(e) ^ s).to_be_bytes()
}

/// AES-GCM encryption: ciphertext followed by the tag (`tag_bits / 8`
/// bytes).
pub fn gcm_encrypt(key: &[u8], iv: &[u8], aad: &[u8], data: &[u8], tag_bits: u32) -> Result<Vec<u8>, &'static str> {
    let aes = cipher(key)?;
    if iv.is_empty() {
        return Err("AES-GCM needs an IV");
    }
    if !matches!(tag_bits, 32 | 64 | 96 | 104 | 112 | 120 | 128) {
        return Err("AES-GCM tag length is not allowed");
    }
    let (h, j0) = gcm_setup(&aes, iv);
    let mut ct = gcm_stream(&aes, j0, data);
    let tag = gcm_tag(&aes, h, j0, aad, &ct);
    ct.extend_from_slice(&tag[..(tag_bits / 8) as usize]);
    Ok(ct)
}

/// AES-GCM decryption of ciphertext || tag.
pub fn gcm_decrypt(key: &[u8], iv: &[u8], aad: &[u8], data: &[u8], tag_bits: u32) -> Result<Vec<u8>, &'static str> {
    let aes = cipher(key)?;
    if iv.is_empty() {
        return Err("AES-GCM needs an IV");
    }
    if !matches!(tag_bits, 32 | 64 | 96 | 104 | 112 | 120 | 128) {
        return Err("AES-GCM tag length is not allowed");
    }
    let t = (tag_bits / 8) as usize;
    if data.len() < t {
        return Err("AES-GCM ciphertext is shorter than the tag");
    }
    let (ct, tag) = data.split_at(data.len() - t);
    let (h, j0) = gcm_setup(&aes, iv);
    let expected = gcm_tag(&aes, h, j0, aad, ct);
    if !super::equal(&expected[..t], tag) {
        return Err("AES-GCM tag does not match");
    }
    Ok(gcm_stream(&aes, j0, ct))
}

const KW_IV: u64 = 0xa6a6_a6a6_a6a6_a6a6;

/// AES Key Wrap (RFC 3394 section 2.2.1).
pub fn kw_wrap(kek: &[u8], data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let aes = cipher(kek)?;
    if data.len() < 16 || data.len() % 8 != 0 {
        return Err("AES-KW data must be a multiple of 8 bytes, at least 16");
    }
    let n = data.len() / 8;
    let mut a = KW_IV;
    let mut r: Vec<u64> = data.chunks(8).map(|c| u64::from_be_bytes(c.try_into().unwrap_or([0; 8]))).collect();
    for j in 0..6 {
        for (i, ri) in r.iter_mut().enumerate() {
            let mut b = [0u8; 16];
            b[..8].copy_from_slice(&a.to_be_bytes());
            b[8..].copy_from_slice(&ri.to_be_bytes());
            aes.verschluessel(&mut b);
            let t = (n * j + i + 1) as u64;
            a = u64::from_be_bytes(b[..8].try_into().unwrap_or([0; 8])) ^ t;
            *ri = u64::from_be_bytes(b[8..].try_into().unwrap_or([0; 8]));
        }
    }
    let mut out = a.to_be_bytes().to_vec();
    for ri in r {
        out.extend_from_slice(&ri.to_be_bytes());
    }
    Ok(out)
}

/// AES Key Unwrap (RFC 3394 section 2.2.2) with the integrity check.
pub fn kw_unwrap(kek: &[u8], data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let aes = cipher(kek)?;
    if data.len() < 24 || data.len() % 8 != 0 {
        return Err("AES-KW wrapped data must be a multiple of 8 bytes, at least 24");
    }
    let n = data.len() / 8 - 1;
    let mut a = u64::from_be_bytes(data[..8].try_into().unwrap_or([0; 8]));
    let mut r: Vec<u64> = data[8..].chunks(8).map(|c| u64::from_be_bytes(c.try_into().unwrap_or([0; 8]))).collect();
    for j in (0..6).rev() {
        for i in (0..n).rev() {
            let t = (n * j + i + 1) as u64;
            let mut b = [0u8; 16];
            b[..8].copy_from_slice(&(a ^ t).to_be_bytes());
            b[8..].copy_from_slice(&r[i].to_be_bytes());
            aes.entschluessel(&mut b);
            a = u64::from_be_bytes(b[..8].try_into().unwrap_or([0; 8]));
            r[i] = u64::from_be_bytes(b[8..].try_into().unwrap_or([0; 8]));
        }
    }
    if !super::equal(&a.to_be_bytes(), &KW_IV.to_be_bytes()) {
        return Err("AES-KW integrity check failed");
    }
    let mut out = Vec::with_capacity(n * 8);
    for ri in r {
        out.extend_from_slice(&ri.to_be_bytes());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn cbc_nist_sp800_38a() {
        let key = unhex("2b7e151628aed2a6abf7158809cf4f3c");
        let iv = unhex("000102030405060708090a0b0c0d0e0f");
        let pt = unhex("6bc1bee22e409f96e93d7e117393172a");
        let ct = cbc_encrypt(&key, &iv, &pt).unwrap();
        assert_eq!(&ct[..16], &unhex("7649abac8119b246cee98e9b12e9197d")[..]);
        assert_eq!(cbc_decrypt(&key, &iv, &ct).unwrap(), pt);
    }

    #[test]
    fn ctr_nist_and_192_bit_keys() {
        let key = unhex("8e73b0f7da0e6452c810f32b809079e562f8ead2522c6b7b");
        let ctr0 = unhex("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
        let pt = unhex("6bc1bee22e409f96e93d7e117393172a");
        let ct = ctr(&key, &ctr0, 128, &pt).unwrap();
        assert_eq!(ct, unhex("1abc932417521ca24f2b0459fe7e6e0b"));
    }

    #[test]
    fn gcm_nist_case_4_and_odd_iv() {
        let key = unhex("feffe9928665731c6d6a8f9467308308");
        let iv = unhex("cafebabefacedbaddecaf888");
        let pt = unhex(
            "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
        );
        let aad = unhex("feedfacedeadbeeffeedfacedeadbeefabaddad2");
        let out = gcm_encrypt(&key, &iv, &aad, &pt, 128).unwrap();
        assert_eq!(
            out,
            unhex(
                "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e0915bc94fbc3221a5db94fae95ae7121a47"
            )
        );
        assert_eq!(gcm_decrypt(&key, &iv, &aad, &out, 128).unwrap(), pt);
        // Test case 6: a 60-byte IV goes through GHASH.
        let iv6 = unhex(
            "9313225df88406e555909c5aff5269aa6a7a9538534f7da1e4c303d2a318a728c3c0c95156809539fcf0e2429a6b525416aedbf5a0de6a57a637b39b",
        );
        let out6 = gcm_encrypt(&key, &iv6, &aad, &pt, 128).unwrap();
        assert_eq!(&out6[out6.len() - 16..], &unhex("619cc5aefffe0bfa462af43c1699d050")[..]);
    }

    #[test]
    fn kw_rfc3394_4_1() {
        let kek = unhex("000102030405060708090a0b0c0d0e0f");
        let key = unhex("00112233445566778899aabbccddeeff");
        let w = kw_wrap(&kek, &key).unwrap();
        assert_eq!(w, unhex("1fa68b0a8112b447aef34bd8fb5a7b829d3e862371d2cfe5"));
        assert_eq!(kw_unwrap(&kek, &w).unwrap(), key);
    }
}
