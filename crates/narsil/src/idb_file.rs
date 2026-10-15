//! IndexedDB on disk: one file per origin under the profile folder
//! (`<profile>/indexeddb/<origin>.idb`, so `NARSIL_PROFIL` and the test
//! profile apply). A compact binary format of our own; a damaged or foreign
//! file is ignored (the origin starts empty), never trusted: every length
//! is checked against the rest of the file and nesting is bounded.

use std::sync::Arc;

use narsil_jsvm::structured::{Ser, SerializedMessage};

const MAGIC: &[u8; 6] = b"NIDB1\n";
/// Nesting bound while reading (values and keys).
const MAX_DEPTH: u32 = 2000;

/// The folder of the files.
pub fn folder() -> std::path::PathBuf {
    crate::plattform::konfiguration().join("indexeddb")
}

/// The file name of an origin (only [A-Za-z0-9._-], the rest hex-escaped).
pub fn file_of(origin: &str) -> std::path::PathBuf {
    folder().join(format!("{}.idb", origin_name(origin)))
}

/// The file of an origin in another folder of the profile (the Cache API:
/// `<profile>/cache_storage/<origin>.cache`).
pub fn profile_file(dir: &str, origin: &str, ext: &str) -> std::path::PathBuf {
    crate::plattform::konfiguration().join(dir).join(format!("{}.{ext}", origin_name(origin)))
}

fn origin_name(origin: &str) -> String {
    let mut name = String::new();
    for b in origin.bytes() {
        if b.is_ascii_alphanumeric() || b == b'.' || b == b'-' {
            name.push(b as char);
        } else {
            name.push_str(&format!("_{b:02x}"));
        }
    }
    if name.len() > 180 {
        name.truncate(180);
    }
    name
}

pub struct Writer {
    pub out: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Writer { out: MAGIC.to_vec() }
    }
    /// A writer for another file kind (its own magic).
    pub fn with_magic(magic: &[u8]) -> Self {
        Writer { out: magic.to_vec() }
    }
    pub fn u8(&mut self, v: u8) {
        self.out.push(v);
    }
    pub fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.u64(b.len() as u64);
        self.out.extend_from_slice(b);
    }
    pub fn text(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    pub fn units(&mut self, u: &[u16]) {
        self.u64(u.len() as u64);
        for x in u {
            self.out.extend_from_slice(&x.to_le_bytes());
        }
    }
    pub fn ser(&mut self, s: &Ser) {
        match s {
            Ser::Undefined => self.u8(0),
            Ser::Null => self.u8(1),
            Ser::Bool(b) => {
                self.u8(2);
                self.u8(*b as u8);
            }
            Ser::Number(n) => {
                self.u8(3);
                self.f64(*n);
            }
            Ser::BigInt(g) => {
                self.u8(4);
                self.out.extend_from_slice(&g.to_le_bytes());
            }
            Ser::String(t) => {
                self.u8(5);
                self.text(t);
            }
            Ser::Ref(i) => {
                self.u8(6);
                self.u64(u64::from(*i));
            }
            Ser::Object(props) => {
                self.u8(7);
                self.props(props);
            }
            Ser::Array { length, props } => {
                self.u8(8);
                self.u64(*length);
                self.props(props);
            }
            Ser::BoxedBool(b) => {
                self.u8(9);
                self.u8(*b as u8);
            }
            Ser::BoxedNumber(n) => {
                self.u8(10);
                self.f64(*n);
            }
            Ser::BoxedString(t) => {
                self.u8(11);
                self.text(t);
            }
            Ser::Date(ms) => {
                self.u8(12);
                self.f64(*ms);
            }
            Ser::RegExp { source, flags } => {
                self.u8(13);
                self.text(source);
                self.text(flags);
            }
            Ser::ArrayBuffer(b) => {
                self.u8(14);
                self.bytes(b);
            }
            // Stored values never carry transferred parts.
            Ser::TransferredBuffer(_) | Ser::Port(_) => self.u8(0),
            Ser::View { kind, buffer, offset, length } => {
                self.u8(16);
                self.text(kind);
                self.ser(buffer);
                self.u64(*offset as u64);
                self.u64(*length as u64);
            }
            Ser::Map(entries) => {
                self.u8(17);
                self.u64(entries.len() as u64);
                for (k, v) in entries {
                    self.ser(k);
                    self.ser(v);
                }
            }
            Ser::Set(items) => {
                self.u8(18);
                self.u64(items.len() as u64);
                for v in items {
                    self.ser(v);
                }
            }
            Ser::Error { name, message, stack } => {
                self.u8(19);
                self.text(name);
                self.opt_text(message.as_deref());
                self.opt_text(stack.as_deref());
            }
            Ser::DomException { name, message } => {
                self.u8(20);
                self.text(name);
                self.text(message);
            }
            Ser::Blob { bytes, mime } => {
                self.u8(21);
                self.bytes(bytes);
                self.text(mime);
            }
            Ser::File { bytes, mime, name, last_modified } => {
                self.u8(22);
                self.bytes(bytes);
                self.text(mime);
                self.text(name);
                self.f64(*last_modified);
            }
        }
    }
    fn props(&mut self, props: &[(String, Ser)]) {
        self.u64(props.len() as u64);
        for (k, v) in props {
            self.text(k);
            self.ser(v);
        }
    }
    pub fn opt_text(&mut self, t: Option<&str>) {
        match t {
            Some(t) => {
                self.u8(1);
                self.text(t);
            }
            None => self.u8(0),
        }
    }
}

pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader after the magic, or `None` for a foreign file.
    pub fn new(data: &'a [u8]) -> Option<Self> {
        Self::with_magic(data, MAGIC)
    }
    /// A reader for another file kind (its own magic).
    pub fn with_magic(data: &'a [u8], magic: &[u8]) -> Option<Self> {
        if !data.starts_with(magic) {
            return None;
        }
        Some(Reader { data, pos: magic.len() })
    }
    pub fn done(&self) -> bool {
        self.pos >= self.data.len()
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    pub fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])))
    }
    pub fn f64(&mut self) -> Option<f64> {
        self.take(8).map(|b| f64::from_le_bytes(b.try_into().unwrap_or([0; 8])))
    }
    /// A length that must fit in the rest of the file (each item >= `min`).
    pub fn len(&mut self, min: usize) -> Option<usize> {
        let n = self.u64()? as usize;
        if n.checked_mul(min.max(1))? > self.data.len().saturating_sub(self.pos) {
            return None;
        }
        Some(n)
    }
    pub fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.len(1)?;
        self.take(n).map(|b| b.to_vec())
    }
    pub fn text(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
    pub fn units(&mut self) -> Option<Vec<u16>> {
        let n = self.len(2)?;
        let raw = self.take(n * 2)?;
        Some(raw.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
    }
    pub fn opt_text(&mut self) -> Option<Option<String>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.text()?)),
            _ => None,
        }
    }
    pub fn ser(&mut self, depth: u32) -> Option<Ser> {
        if depth > MAX_DEPTH {
            return None;
        }
        Some(match self.u8()? {
            0 => Ser::Undefined,
            1 => Ser::Null,
            2 => Ser::Bool(self.u8()? != 0),
            3 => Ser::Number(self.f64()?),
            4 => Ser::BigInt(i128::from_le_bytes(self.take(16)?.try_into().ok()?)),
            5 => Ser::String(self.text()?),
            6 => Ser::Ref(u32::try_from(self.u64()?).ok()?),
            7 => Ser::Object(self.props(depth)?),
            8 => {
                let length = self.u64()?;
                Ser::Array { length, props: self.props(depth)? }
            }
            9 => Ser::BoxedBool(self.u8()? != 0),
            10 => Ser::BoxedNumber(self.f64()?),
            11 => Ser::BoxedString(self.text()?),
            12 => Ser::Date(self.f64()?),
            13 => Ser::RegExp { source: self.text()?, flags: self.text()? },
            14 => Ser::ArrayBuffer(self.bytes()?),
            16 => {
                let kind = self.text()?;
                let buffer = Box::new(self.ser(depth + 1)?);
                let offset = self.u64()? as usize;
                let length = self.u64()? as usize;
                Ser::View { kind, buffer, offset, length }
            }
            17 => {
                let n = self.len(2)?;
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    let k = self.ser(depth + 1)?;
                    let v = self.ser(depth + 1)?;
                    out.push((k, v));
                }
                Ser::Map(out)
            }
            18 => {
                let n = self.len(1)?;
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    out.push(self.ser(depth + 1)?);
                }
                Ser::Set(out)
            }
            19 => Ser::Error { name: self.text()?, message: self.opt_text()?, stack: self.opt_text()? },
            20 => Ser::DomException { name: self.text()?, message: self.text()? },
            21 => Ser::Blob { bytes: Arc::new(self.bytes()?), mime: self.text()? },
            22 => Ser::File { bytes: Arc::new(self.bytes()?), mime: self.text()?, name: self.text()?, last_modified: self.f64()? },
            _ => return None,
        })
    }
    fn props(&mut self, depth: u32) -> Option<Vec<(String, Ser)>> {
        let n = self.len(9)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let k = self.text()?;
            let v = self.ser(depth + 1)?;
            out.push((k, v));
        }
        Some(out)
    }
}

/// A stored value from its serialized form.
pub fn message(value: Ser) -> SerializedMessage {
    let bytes = approx_bytes(&value);
    SerializedMessage { value, buffers: Vec::new(), ports: Vec::new(), bytes }
}

fn approx_bytes(s: &Ser) -> usize {
    match s {
        Ser::String(t) | Ser::BoxedString(t) => t.len() + 16,
        Ser::ArrayBuffer(b) => b.len() + 16,
        Ser::Blob { bytes, .. } | Ser::File { bytes, .. } => bytes.len() + 32,
        Ser::Object(p) | Ser::Array { props: p, .. } => 32 + p.iter().map(|(k, v)| k.len() + approx_bytes(v)).sum::<usize>(),
        Ser::Map(e) => 32 + e.iter().map(|(k, v)| approx_bytes(k) + approx_bytes(v)).sum::<usize>(),
        Ser::Set(e) => 32 + e.iter().map(approx_bytes).sum::<usize>(),
        Ser::View { buffer, .. } => 32 + approx_bytes(buffer),
        _ => 16,
    }
}

/// Writes `data` atomically (temporary file, then rename).
pub fn write_file(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp_name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_and_garbage_is_refused() {
        let v = Ser::Object(vec![
            ("a".into(), Ser::Number(1.5)),
            ("m".into(), Ser::Map(vec![(Ser::String("k".into()), Ser::Date(7.0))])),
            ("v".into(), Ser::View { kind: "Uint8Array".into(), buffer: Box::new(Ser::ArrayBuffer(vec![1, 2])), offset: 0, length: 2 }),
        ]);
        let mut w = Writer::new();
        w.ser(&v);
        let mut r = Reader::new(&w.out).expect("magic");
        let back = r.ser(0).expect("value");
        assert_eq!(format!("{back:?}"), format!("{v:?}"));
        // A length beyond the end and a foreign file are refused.
        let mut bad = w.out.clone();
        bad.truncate(bad.len() - 3);
        assert!(Reader::new(&bad).and_then(|mut r| r.ser(0)).is_none());
        assert!(Reader::new(b"not ours").is_none());
    }
}
