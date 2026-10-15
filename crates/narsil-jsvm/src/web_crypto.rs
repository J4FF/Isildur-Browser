//! Web Crypto API (`crypto.subtle`): the Rust half. The WebIDL half
//! (algorithm normalization, CryptoKey, key formats, promises, the
//! DOMException names) is `web_crypto.js`, loaded on the first read of
//! `crypto.subtle` or of the `SubtleCrypto`/`CryptoKey` globals.
//!
//! Every native is a pure function over bytes (the primitives live in
//! `narsil_tls::web_crypto`); key material stays in the glue's private
//! slots. A failing primitive throws a plain `Error` with the reason, and
//! the glue turns it into the DOMException the spec names.

use narsil_tls::web_crypto::ec::{self, CurveId};
use narsil_tls::web_crypto::hash::HashAlg;
use narsil_tls::web_crypto::{aes, ed25519, hash, num, rsa};

use crate::heap::{GVal, HeapRef};
use crate::interp::{Engine, Stoerung};
use crate::web_platform::PlatformOp;

const GLUE: &str = include_str!("web_crypto.js");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CryptoOp {
    /// The native `crypto.subtle` getter installed at engine start.
    SubtleGetter,
    /// The glue hands over its `SubtleCrypto` instance.
    SetSubtle,
    Digest,
    Hmac,
    Hkdf,
    Pbkdf2,
    AesCbc,
    AesCtr,
    AesGcm,
    AesKw,
    Random,
    RsaGenerate,
    RsaSign,
    RsaVerify,
    RsaEncrypt,
    RsaDecrypt,
    RsaCheck,
    EcGenerate,
    EcPublic,
    EcPoint,
    EcCompress,
    EcSign,
    EcVerify,
    Ecdh,
    EdGenerate,
    EdPublic,
    EdSign,
    EdVerify,
    EdValid,
    XGenerate,
    XPublic,
    X25519,
}

const NATIVES: &[(&str, CryptoOp, u32)] = &[
    ("setSubtle", CryptoOp::SetSubtle, 1),
    ("digest", CryptoOp::Digest, 2),
    ("hmac", CryptoOp::Hmac, 3),
    ("hkdf", CryptoOp::Hkdf, 5),
    ("pbkdf2", CryptoOp::Pbkdf2, 5),
    ("aesCbc", CryptoOp::AesCbc, 4),
    ("aesCtr", CryptoOp::AesCtr, 4),
    ("aesGcm", CryptoOp::AesGcm, 6),
    ("aesKw", CryptoOp::AesKw, 3),
    ("random", CryptoOp::Random, 1),
    ("rsaGenerate", CryptoOp::RsaGenerate, 2),
    ("rsaSign", CryptoOp::RsaSign, 5),
    ("rsaVerify", CryptoOp::RsaVerify, 7),
    ("rsaEncrypt", CryptoOp::RsaEncrypt, 5),
    ("rsaDecrypt", CryptoOp::RsaDecrypt, 4),
    ("rsaCheck", CryptoOp::RsaCheck, 1),
    ("ecGenerate", CryptoOp::EcGenerate, 1),
    ("ecPublic", CryptoOp::EcPublic, 2),
    ("ecPoint", CryptoOp::EcPoint, 2),
    ("ecCompress", CryptoOp::EcCompress, 3),
    ("ecSign", CryptoOp::EcSign, 4),
    ("ecVerify", CryptoOp::EcVerify, 6),
    ("ecdh", CryptoOp::Ecdh, 4),
    ("edGenerate", CryptoOp::EdGenerate, 0),
    ("edPublic", CryptoOp::EdPublic, 1),
    ("edSign", CryptoOp::EdSign, 2),
    ("edVerify", CryptoOp::EdVerify, 3),
    ("edValid", CryptoOp::EdValid, 1),
    ("xGenerate", CryptoOp::XGenerate, 0),
    ("xPublic", CryptoOp::XPublic, 1),
    ("x25519", CryptoOp::X25519, 2),
];

impl Engine {
    /// Install the lazy `crypto.subtle` getter on the `crypto` object.
    pub(crate) fn crypto_subtle_getter(&mut self, crypto: HeapRef) {
        self.nativ_getter(
            crypto,
            "subtle",
            crate::builtin::Nativ::Plattform(PlatformOp::Crypto(CryptoOp::SubtleGetter)),
        );
    }

    /// Run `web_crypto.js` (first use of `crypto.subtle`).
    pub(crate) fn crypto_glue_laden(&mut self) -> Result<(), Stoerung> {
        let list: Vec<(&str, PlatformOp, u32)> =
            NATIVES.iter().map(|(n, o, l)| (*n, PlatformOp::Crypto(*o), *l)).collect();
        let natives = self.platform_natives(&list)?;
        self.platform_run_glue(GLUE, natives)
    }

    fn crypto_error(&mut self, msg: &str) -> Stoerung {
        Stoerung::Wurf(self.fehler_wert("Error", msg))
    }

    fn crypto_bytes(&mut self, args: &[GVal], i: usize) -> Result<Vec<u8>, Stoerung> {
        self.buffer_source_bytes(self.arg(args, i))
    }

    fn crypto_text(&mut self, args: &[GVal], i: usize) -> Result<String, Stoerung> {
        self.zu_text(self.arg(args, i))
    }

    fn crypto_hash(&mut self, args: &[GVal], i: usize) -> Result<HashAlg, Stoerung> {
        let t = self.crypto_text(args, i)?;
        HashAlg::from_name(&t).ok_or_else(|| self.crypto_error("unknown hash"))
    }

    fn crypto_curve(&mut self, args: &[GVal], i: usize) -> Result<CurveId, Stoerung> {
        let t = self.crypto_text(args, i)?;
        CurveId::from_name(&t).ok_or_else(|| self.crypto_error("unknown curve"))
    }

    fn crypto_num(&mut self, args: &[GVal], i: usize) -> Result<f64, Stoerung> {
        self.zu_zahl(self.arg(args, i))
    }

    fn crypto_ab(&mut self, r: Result<Vec<u8>, &'static str>) -> Result<GVal, Stoerung> {
        match r {
            Ok(b) => self.new_array_buffer(b),
            Err(e) => Err(self.crypto_error(e)),
        }
    }

    fn crypto_abs(&mut self, parts: Vec<Vec<u8>>) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        let mut out = Vec::with_capacity(parts.len());
        for p in parts {
            let b = self.new_array_buffer(p)?;
            self.wurzeln.push(b);
            out.push(b);
        }
        let r = self.array_of(&out);
        self.wurzel_zurueck(marke);
        r
    }

    /// The eight RSA private key parts from an array of buffers.
    fn crypto_rsa_private(&mut self, v: GVal) -> Result<rsa::RsaPrivate, Stoerung> {
        let werte = self.iter_werte(v)?;
        if werte.len() != 8 {
            return Err(self.crypto_error("RSA private key needs 8 parts"));
        }
        let mut parts: Vec<Vec<u8>> = Vec::with_capacity(8);
        for w in werte {
            parts.push(self.buffer_source_bytes(w)?);
        }
        let refs: [&[u8]; 8] = [
            &parts[0], &parts[1], &parts[2], &parts[3], &parts[4], &parts[5], &parts[6], &parts[7],
        ];
        rsa::RsaPrivate::from_parts(refs).map_err(|e| self.crypto_error(e))
    }

    fn crypto_rsa_public(&mut self, args: &[GVal], i: usize) -> Result<rsa::RsaPublic, Stoerung> {
        let n = self.crypto_bytes(args, i)?;
        let e = self.crypto_bytes(args, i + 1)?;
        let n = num::from_bytes(&n).map_err(|e| self.crypto_error(e))?;
        let e = num::from_bytes(&e).map_err(|e| self.crypto_error(e))?;
        Ok(rsa::RsaPublic { n, e })
    }

    pub(crate) fn crypto_native(&mut self, op: CryptoOp, _this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        match op {
            CryptoOp::SubtleGetter => {
                self.platform_lazy_api(crate::web_platform::Api::Crypto)?;
                Ok(self.platform.crypto_subtle.unwrap_or(GVal::Undefiniert))
            }
            CryptoOp::SetSubtle => {
                self.platform.crypto_subtle = Some(self.arg(args, 0));
                Ok(GVal::Undefiniert)
            }
            CryptoOp::Digest => {
                let h = self.crypto_hash(args, 0)?;
                let d = self.crypto_bytes(args, 1)?;
                self.new_array_buffer(hash::digest(h, &d))
            }
            CryptoOp::Hmac => {
                let h = self.crypto_hash(args, 0)?;
                let k = self.crypto_bytes(args, 1)?;
                let d = self.crypto_bytes(args, 2)?;
                self.new_array_buffer(hash::hmac(h, &k, &d))
            }
            CryptoOp::Hkdf => {
                let h = self.crypto_hash(args, 0)?;
                let ikm = self.crypto_bytes(args, 1)?;
                let salt = self.crypto_bytes(args, 2)?;
                let info = self.crypto_bytes(args, 3)?;
                let len = self.crypto_num(args, 4)? as usize;
                self.crypto_ab(hash::hkdf(h, &ikm, &salt, &info, len))
            }
            CryptoOp::Pbkdf2 => {
                let h = self.crypto_hash(args, 0)?;
                let pwd = self.crypto_bytes(args, 1)?;
                let salt = self.crypto_bytes(args, 2)?;
                let it = self.crypto_num(args, 3)? as u32;
                let len = self.crypto_num(args, 4)? as usize;
                self.crypto_ab(hash::pbkdf2(h, &pwd, &salt, it, len))
            }
            CryptoOp::AesCbc => {
                let enc = self.zu_bool(self.arg(args, 0));
                let k = self.crypto_bytes(args, 1)?;
                let iv = self.crypto_bytes(args, 2)?;
                let d = self.crypto_bytes(args, 3)?;
                let r = if enc { aes::cbc_encrypt(&k, &iv, &d) } else { aes::cbc_decrypt(&k, &iv, &d) };
                self.crypto_ab(r)
            }
            CryptoOp::AesCtr => {
                let k = self.crypto_bytes(args, 0)?;
                let c = self.crypto_bytes(args, 1)?;
                let l = self.crypto_num(args, 2)? as u32;
                let d = self.crypto_bytes(args, 3)?;
                self.crypto_ab(aes::ctr(&k, &c, l, &d))
            }
            CryptoOp::AesGcm => {
                let enc = self.zu_bool(self.arg(args, 0));
                let k = self.crypto_bytes(args, 1)?;
                let iv = self.crypto_bytes(args, 2)?;
                let aad = self.crypto_bytes(args, 3)?;
                let d = self.crypto_bytes(args, 4)?;
                let t = self.crypto_num(args, 5)? as u32;
                let r = if enc {
                    aes::gcm_encrypt(&k, &iv, &aad, &d, t)
                } else {
                    aes::gcm_decrypt(&k, &iv, &aad, &d, t)
                };
                self.crypto_ab(r)
            }
            CryptoOp::AesKw => {
                let wrap = self.zu_bool(self.arg(args, 0));
                let k = self.crypto_bytes(args, 1)?;
                let d = self.crypto_bytes(args, 2)?;
                let r = if wrap { aes::kw_wrap(&k, &d) } else { aes::kw_unwrap(&k, &d) };
                self.crypto_ab(r)
            }
            CryptoOp::Random => {
                let n = self.crypto_num(args, 0)? as usize;
                self.crypto_ab(narsil_tls::web_crypto::random(n))
            }
            CryptoOp::RsaGenerate => {
                let bits = self.crypto_num(args, 0)? as usize;
                let e = self.crypto_bytes(args, 1)?;
                match rsa::generate(bits, &e) {
                    Ok(k) => self.crypto_abs(
                        [&k.n, &k.e, &k.d, &k.p, &k.q, &k.dp, &k.dq, &k.qi]
                            .iter()
                            .map(|z| num::to_bytes_min(z))
                            .collect(),
                    ),
                    Err(e) => Err(self.crypto_error(e)),
                }
            }
            CryptoOp::RsaSign => {
                let pss = self.zu_bool(self.arg(args, 0));
                let h = self.crypto_hash(args, 1)?;
                let k = self.crypto_rsa_private(self.arg(args, 2))?;
                let d = self.crypto_bytes(args, 3)?;
                let salt = self.crypto_num(args, 4)? as usize;
                let r = if pss { rsa::pss_sign(&k, h, &d, salt) } else { rsa::pkcs1_sign(&k, h, &d) };
                self.crypto_ab(r)
            }
            CryptoOp::RsaVerify => {
                let pss = self.zu_bool(self.arg(args, 0));
                let h = self.crypto_hash(args, 1)?;
                let k = self.crypto_rsa_public(args, 2)?;
                let d = self.crypto_bytes(args, 4)?;
                let s = self.crypto_bytes(args, 5)?;
                let salt = self.crypto_num(args, 6)? as usize;
                let ok = if pss { rsa::pss_verify(&k, h, &d, &s, salt) } else { rsa::pkcs1_verify(&k, h, &d, &s) };
                Ok(GVal::Bool(ok))
            }
            CryptoOp::RsaEncrypt => {
                let h = self.crypto_hash(args, 0)?;
                let k = self.crypto_rsa_public(args, 1)?;
                let d = self.crypto_bytes(args, 3)?;
                let l = self.crypto_bytes(args, 4)?;
                self.crypto_ab(rsa::oaep_encrypt(&k, h, &d, &l))
            }
            CryptoOp::RsaDecrypt => {
                let h = self.crypto_hash(args, 0)?;
                let k = self.crypto_rsa_private(self.arg(args, 1))?;
                let d = self.crypto_bytes(args, 2)?;
                let l = self.crypto_bytes(args, 3)?;
                self.crypto_ab(rsa::oaep_decrypt(&k, h, &d, &l))
            }
            CryptoOp::RsaCheck => Ok(GVal::Bool(self.crypto_rsa_private(self.arg(args, 0)).is_ok())),
            CryptoOp::EcGenerate => {
                let c = self.crypto_curve(args, 0)?;
                match ec::generate(c) {
                    Ok((d, x, y)) => self.crypto_abs(vec![d, x.nach_bytes_gross(c.len()), y.nach_bytes_gross(c.len())]),
                    Err(e) => Err(self.crypto_error(e)),
                }
            }
            CryptoOp::EcPublic => {
                let c = self.crypto_curve(args, 0)?;
                let d = self.crypto_bytes(args, 1)?;
                let curve = ec::curve(c);
                let r = num::from_bytes(&d).and_then(|d| ec::public_from_private(&curve, &d));
                match r {
                    Ok((x, y)) => self.crypto_abs(vec![x.nach_bytes_gross(c.len()), y.nach_bytes_gross(c.len())]),
                    Err(e) => Err(self.crypto_error(e)),
                }
            }
            CryptoOp::EcPoint => {
                let c = self.crypto_curve(args, 0)?;
                let b = self.crypto_bytes(args, 1)?;
                match ec::decode_point(&ec::curve(c), &b) {
                    Ok((x, y)) => self.crypto_abs(vec![x.nach_bytes_gross(c.len()), y.nach_bytes_gross(c.len())]),
                    Err(e) => Err(self.crypto_error(e)),
                }
            }
            CryptoOp::EcCompress => {
                let c = self.crypto_curve(args, 0)?;
                let x = self.crypto_bytes(args, 1)?;
                let y = self.crypto_bytes(args, 2)?;
                let curve = ec::curve(c);
                let r = (|| -> Result<Vec<u8>, &'static str> {
                    let x = num::from_bytes(&x)?;
                    let y = num::from_bytes(&y)?;
                    Ok(ec::encode_point_compressed(&curve, &x, &y))
                })();
                self.crypto_ab(r)
            }
            CryptoOp::EcSign => {
                let c = self.crypto_curve(args, 0)?;
                let h = self.crypto_hash(args, 1)?;
                let d = self.crypto_bytes(args, 2)?;
                let m = self.crypto_bytes(args, 3)?;
                self.crypto_ab(ec::sign(c, &d, h, &m))
            }
            CryptoOp::EcVerify => {
                let c = self.crypto_curve(args, 0)?;
                let h = self.crypto_hash(args, 1)?;
                let x = self.crypto_bytes(args, 2)?;
                let y = self.crypto_bytes(args, 3)?;
                let m = self.crypto_bytes(args, 4)?;
                let s = self.crypto_bytes(args, 5)?;
                let ok = match (num::from_bytes(&x), num::from_bytes(&y)) {
                    (Ok(x), Ok(y)) => ec::verify(c, &x, &y, h, &m, &s),
                    _ => false,
                };
                Ok(GVal::Bool(ok))
            }
            CryptoOp::Ecdh => {
                let c = self.crypto_curve(args, 0)?;
                let d = self.crypto_bytes(args, 1)?;
                let x = self.crypto_bytes(args, 2)?;
                let y = self.crypto_bytes(args, 3)?;
                let r = (|| -> Result<Vec<u8>, &'static str> {
                    let x = num::from_bytes(&x)?;
                    let y = num::from_bytes(&y)?;
                    ec::ecdh(c, &d, &x, &y)
                })();
                self.crypto_ab(r)
            }
            CryptoOp::EdGenerate => match ed25519::generate() {
                Ok((seed, pk)) => self.crypto_abs(vec![seed, pk.to_vec()]),
                Err(e) => Err(self.crypto_error(e)),
            },
            CryptoOp::EdPublic => {
                let s = self.crypto_bytes(args, 0)?;
                self.crypto_ab(ed25519::public_key(&s).map(|k| k.to_vec()))
            }
            CryptoOp::EdSign => {
                let s = self.crypto_bytes(args, 0)?;
                let m = self.crypto_bytes(args, 1)?;
                self.crypto_ab(ed25519::sign(&s, &m))
            }
            CryptoOp::EdVerify => {
                let p = self.crypto_bytes(args, 0)?;
                let m = self.crypto_bytes(args, 1)?;
                let s = self.crypto_bytes(args, 2)?;
                Ok(GVal::Bool(ed25519::verify(&p, &m, &s)))
            }
            CryptoOp::EdValid => {
                let p = self.crypto_bytes(args, 0)?;
                Ok(GVal::Bool(ed25519::valid_public(&p)))
            }
            CryptoOp::XGenerate => {
                let r = narsil_tls::web_crypto::random(32).and_then(|k| {
                    let p = ed25519::x25519_public(&k)?;
                    Ok((k, p))
                });
                match r {
                    Ok((k, p)) => self.crypto_abs(vec![k, p.to_vec()]),
                    Err(e) => Err(self.crypto_error(e)),
                }
            }
            CryptoOp::XPublic => {
                let k = self.crypto_bytes(args, 0)?;
                self.crypto_ab(ed25519::x25519_public(&k).map(|p| p.to_vec()))
            }
            CryptoOp::X25519 => {
                let k = self.crypto_bytes(args, 0)?;
                let p = self.crypto_bytes(args, 1)?;
                self.crypto_ab(ed25519::x25519(&k, &p).map(|s| s.to_vec()))
            }
        }
    }
}
