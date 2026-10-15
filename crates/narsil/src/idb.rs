//! IndexedDB storage (IndexedDB 3.0 §2): the databases of each origin,
//! object stores with ordered records, indexes and key generators.
//!
//! The API objects, the request/event order and the transaction scheduling
//! live in `indexeddb.js`; this module is the storage engine behind it. One
//! store per origin is shared by every context of the process (the page,
//! its frames and its workers). Values are structured-serialized messages
//! ([`narsil_jsvm::structured`]); keys are IndexedDB keys with their own
//! order ([`Key`]). A transaction keeps snapshots of what it changed, so an
//! abort restores the state from before it.
//!
//! Bounded: every origin has a byte budget ([`ORIGIN_QUOTA`]); a write
//! beyond it fails with `QuotaExceededError`. The data of an http(s) origin
//! lives on disk (`idb_file.rs`): loaded on first use, written by a
//! background thread shortly after a transaction commits (opaque origins
//! stay in memory).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Mutex, OnceLock};

use narsil_jsvm::structured::{self, SerializedMessage};
use narsil_jsvm::{Engine, Wert};

/// Byte budget of one origin (values, keys and index entries).
const ORIGIN_QUOTA: usize = 512 * 1024 * 1024;
/// Hard cap of live transactions (a script that never finishes one must
/// not keep snapshots forever).
const MAX_TRANSACTIONS: usize = 4096;
/// Largest key generator value (IndexedDB §2.11: 2^53).
const MAX_GENERATED: f64 = 9_007_199_254_740_992.0;

/// An IndexedDB key (IndexedDB §2.5) with its ordering: numbers < dates <
/// strings < binary < arrays; strings compare by UTF-16 code units.
#[derive(Debug, Clone)]
pub enum Key {
    Number(f64),
    Date(f64),
    /// UTF-16 code units (keys may hold lone surrogates).
    String(Vec<u16>),
    Binary(Vec<u8>),
    Array(Vec<Key>),
}

impl Key {
    fn rank(&self) -> u8 {
        match self {
            Key::Number(_) => 0,
            Key::Date(_) => 1,
            Key::String(_) => 2,
            Key::Binary(_) => 3,
            Key::Array(_) => 4,
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Key::Number(_) | Key::Date(_) => 16,
            Key::String(s) => 16 + s.len() * 2,
            Key::Binary(b) => 16 + b.len(),
            Key::Array(a) => 16 + a.iter().map(Key::bytes).sum::<usize>(),
        }
    }

    /// From the script's encoding (`indexeddb.js` `toKey`): a text of
    /// `;`-terminated tokens, so that keys of any length and depth cross
    /// the host boundary (which caps arrays and nesting): `n<number>`,
    /// `d<ms>`, `s<string>` (`\` escaped as `\\`, `;` as `\:`), `u<code
    /// units, comma separated>` (strings with surrogates), `b<hex>`, and
    /// `a` ... `e` around the items of an array.
    fn from_wert(w: &Wert, _depth: u32) -> Option<Key> {
        let Wert::Text(text) = w else { return None };
        Key::parse(text)
    }

    /// Deepest array nesting accepted (the key types recurse).
    const MAX_DEPTH: usize = 1000;

    fn parse(text: &str) -> Option<Key> {
        let mut stack: Vec<Vec<Key>> = Vec::new();
        let mut tokens = text.split(';');
        loop {
            let t = tokens.next()?;
            let mut chars = t.chars();
            let kind = chars.next()?;
            let body = chars.as_str();
            let key = match kind {
                'n' | 'd' => {
                    let n = match body {
                        "Infinity" => f64::INFINITY,
                        "-Infinity" => f64::NEG_INFINITY,
                        _ => body.parse::<f64>().ok()?,
                    };
                    if n.is_nan() {
                        return None;
                    }
                    if kind == 'n' { Key::Number(if n == 0.0 { 0.0 } else { n }) } else { Key::Date(n) }
                }
                's' => {
                    let mut units = Vec::with_capacity(body.len());
                    let mut it = body.chars();
                    while let Some(c) = it.next() {
                        let c = if c == '\\' {
                            match it.next()? {
                                ':' => ';',
                                '\\' => '\\',
                                _ => return None,
                            }
                        } else {
                            c
                        };
                        let mut buf = [0u16; 2];
                        units.extend_from_slice(c.encode_utf16(&mut buf));
                    }
                    Key::String(units)
                }
                'u' => Key::String(body.split(',').map(|u| u.parse::<u16>().ok()).collect::<Option<Vec<u16>>>()?),
                'b' => {
                    if body.len() % 2 != 0 {
                        return None;
                    }
                    let mut bytes = Vec::with_capacity(body.len() / 2);
                    for i in (0..body.len()).step_by(2) {
                        bytes.push(u8::from_str_radix(body.get(i..i + 2)?, 16).ok()?);
                    }
                    Key::Binary(bytes)
                }
                'a' => {
                    if !body.is_empty() || stack.len() >= Self::MAX_DEPTH {
                        return None;
                    }
                    stack.push(Vec::new());
                    continue;
                }
                'e' => {
                    if !body.is_empty() {
                        return None;
                    }
                    Key::Array(stack.pop()?)
                }
                _ => return None,
            };
            match stack.last_mut() {
                Some(items) => items.push(key),
                None => return Some(key),
            }
        }
    }

    fn write(&self, out: &mut String) {
        use std::fmt::Write;
        match self {
            Key::Number(n) | Key::Date(n) => {
                out.push(if matches!(self, Key::Number(_)) { 'n' } else { 'd' });
                if n.is_infinite() {
                    out.push_str(if *n > 0.0 { "Infinity" } else { "-Infinity" });
                } else {
                    // -0 is the key 0 (`parse` folds it as well)
                    let _ = write!(out, "{:e}", if *n == 0.0 { 0.0 } else { *n });
                }
            }
            Key::String(units) => {
                if units.iter().any(|u| (0xD800..0xE000).contains(u)) {
                    out.push('u');
                    for (i, u) in units.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        let _ = write!(out, "{u}");
                    }
                } else {
                    out.push('s');
                    for c in char::decode_utf16(units.iter().copied()).map(|c| c.unwrap_or('\u{FFFD}')) {
                        match c {
                            '\\' => out.push_str("\\\\"),
                            ';' => out.push_str("\\:"),
                            c => out.push(c),
                        }
                    }
                }
            }
            Key::Binary(b) => {
                out.push('b');
                for x in b {
                    let _ = write!(out, "{x:02x}");
                }
            }
            Key::Array(items) => {
                out.push_str("a;");
                for k in items {
                    k.write(out);
                }
                out.push('e');
            }
        }
        out.push(';');
    }

    fn to_wert(&self) -> Wert {
        let mut out = String::new();
        self.write(&mut out);
        Wert::Text(out)
    }
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Key {}
impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Key::Number(a), Key::Number(b)) | (Key::Date(a), Key::Date(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Key::String(a), Key::String(b)) => a.cmp(b),
            (Key::Binary(a), Key::Binary(b)) => a.cmp(b),
            (Key::Array(a), Key::Array(b)) => a.cmp(b),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

/// A key range (IndexedDB §2.9); `None` bounds are open-ended.
#[derive(Debug, Clone, Default)]
struct Range {
    lower: Option<Key>,
    upper: Option<Key>,
    lower_open: bool,
    upper_open: bool,
}

impl Range {
    /// `null` (everything), a key, or `[lower|null, upper|null, lowerOpen, upperOpen]` with a marker `0`.
    fn from_wert(w: Option<&Wert>) -> Result<Range, String> {
        match w {
            None | Some(Wert::Null) | Some(Wert::Undefiniert) => Ok(Range::default()),
            Some(Wert::Feld(v)) if matches!(v.first(), Some(Wert::Zahl(t)) if *t == 0.0) => {
                let k = |i: usize| match v.get(i) {
                    Some(Wert::Null) | None => Ok(None),
                    Some(x) => Key::from_wert(x, 0).map(Some).ok_or_else(|| "DataError: bad key".to_string()),
                };
                Ok(Range {
                    lower: k(1)?,
                    upper: k(2)?,
                    lower_open: matches!(v.get(3), Some(Wert::Wahr(true))),
                    upper_open: matches!(v.get(4), Some(Wert::Wahr(true))),
                })
            }
            Some(x) => {
                let key = Key::from_wert(x, 0).ok_or_else(|| "DataError: bad key".to_string())?;
                Ok(Range { lower: Some(key.clone()), upper: Some(key), lower_open: false, upper_open: false })
            }
        }
    }

    fn contains(&self, k: &Key) -> bool {
        if let Some(l) = &self.lower {
            match k.cmp(l) {
                Ordering::Less => return false,
                Ordering::Equal if self.lower_open => return false,
                _ => {}
            }
        }
        if let Some(u) = &self.upper {
            match k.cmp(u) {
                Ordering::Greater => return false,
                Ordering::Equal if self.upper_open => return false,
                _ => {}
            }
        }
        true
    }
}

#[derive(Debug, Clone)]
struct Record {
    value: SerializedMessage,
    /// The keys of this record in each index (to remove them again).
    index_keys: Vec<(String, Key)>,
}

#[derive(Debug, Clone)]
struct Index {
    /// The key path as the script gave it (JSON of a string or an array).
    key_path: String,
    unique: bool,
    multi_entry: bool,
    /// index key -> primary keys
    entries: BTreeMap<Key, BTreeSet<Key>>,
}

#[derive(Debug, Clone)]
struct ObjectStore {
    key_path: Option<String>,
    auto_increment: bool,
    /// The key generator's current number (IndexedDB §2.11), starts at 1.
    current: f64,
    records: BTreeMap<Key, Record>,
    indexes: BTreeMap<String, Index>,
}

#[derive(Debug, Clone, Default)]
struct Database {
    version: f64,
    stores: BTreeMap<String, ObjectStore>,
}

#[derive(Default)]
struct OriginData {
    dbs: BTreeMap<String, Database>,
    bytes: usize,
}

/// Only real origins are written to disk.
fn persistent(origin: &str) -> bool {
    origin.starts_with("http://") || origin.starts_with("https://")
}

fn write_key(w: &mut crate::idb_file::Writer, k: &Key) {
    match k {
        Key::Number(n) => {
            w.u8(1);
            w.f64(*n);
        }
        Key::Date(n) => {
            w.u8(2);
            w.f64(*n);
        }
        Key::String(u) => {
            w.u8(3);
            w.units(u);
        }
        Key::Binary(b) => {
            w.u8(4);
            w.bytes(b);
        }
        Key::Array(a) => {
            w.u8(5);
            w.u64(a.len() as u64);
            for x in a {
                write_key(w, x);
            }
        }
    }
}

fn read_key(r: &mut crate::idb_file::Reader, depth: u32) -> Option<Key> {
    if depth > 256 {
        return None;
    }
    Some(match r.u8()? {
        1 => Key::Number(r.f64()?),
        2 => Key::Date(r.f64()?),
        3 => Key::String(r.units()?),
        4 => Key::Binary(r.bytes()?),
        5 => {
            let n = r.len(9)?;
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                out.push(read_key(r, depth + 1)?);
            }
            Key::Array(out)
        }
        _ => return None,
    })
}

/// The bytes of an origin's data on disk.
fn encode(o: &OriginData) -> Vec<u8> {
    let mut w = crate::idb_file::Writer::new();
    w.u64(o.dbs.len() as u64);
    for (name, db) in &o.dbs {
        w.text(name);
        w.f64(db.version);
        w.u64(db.stores.len() as u64);
        for (sname, st) in &db.stores {
            w.text(sname);
            w.opt_text(st.key_path.as_deref());
            w.u8(st.auto_increment as u8);
            w.f64(st.current);
            w.u64(st.indexes.len() as u64);
            for (iname, ix) in &st.indexes {
                w.text(iname);
                w.text(&ix.key_path);
                w.u8(ix.unique as u8);
                w.u8(ix.multi_entry as u8);
            }
            w.u64(st.records.len() as u64);
            for (k, rec) in &st.records {
                write_key(&mut w, k);
                w.ser(&rec.value.value);
                w.u64(rec.index_keys.len() as u64);
                for (iname, ik) in &rec.index_keys {
                    w.text(iname);
                    write_key(&mut w, ik);
                }
            }
        }
    }
    w.out
}

/// An origin's data from disk (`None`: missing or damaged file).
fn decode(data: &[u8]) -> Option<OriginData> {
    let mut r = crate::idb_file::Reader::new(data)?;
    let mut o = OriginData::default();
    let ndb = r.len(16)?;
    for _ in 0..ndb {
        let name = r.text()?;
        let version = r.f64()?;
        let mut db = Database { version, stores: BTreeMap::new() };
        let nst = r.len(16)?;
        for _ in 0..nst {
            let sname = r.text()?;
            let key_path = r.opt_text()?;
            let auto_increment = r.u8()? != 0;
            let current = r.f64()?;
            let mut st = ObjectStore { key_path, auto_increment, current, records: BTreeMap::new(), indexes: BTreeMap::new() };
            let nix = r.len(16)?;
            for _ in 0..nix {
                let iname = r.text()?;
                let key_path = r.text()?;
                let unique = r.u8()? != 0;
                let multi_entry = r.u8()? != 0;
                st.indexes.insert(iname, Index { key_path, unique, multi_entry, entries: BTreeMap::new() });
            }
            let nrec = r.len(10)?;
            for _ in 0..nrec {
                let k = read_key(&mut r, 0)?;
                let value = r.ser(0)?;
                let nik = r.len(10)?;
                let mut index_keys = Vec::with_capacity(nik);
                for _ in 0..nik {
                    let iname = r.text()?;
                    let ik = read_key(&mut r, 0)?;
                    if let Some(ix) = st.indexes.get_mut(&iname) {
                        ix.entries.entry(ik.clone()).or_default().insert(k.clone());
                    }
                    index_keys.push((iname, ik));
                }
                st.records.insert(k, Record { value: crate::idb_file::message(value), index_keys });
            }
            db.stores.insert(sname, st);
        }
        o.dbs.insert(name, db);
    }
    if !r.done() {
        return None;
    }
    o.bytes = o.dbs.values().map(|d| d.stores.values().map(store_bytes).sum::<usize>()).sum();
    Some(o)
}

fn load(origin: &str) -> OriginData {
    if !persistent(origin) {
        return OriginData::default();
    }
    std::fs::read(crate::idb_file::file_of(origin)).ok().and_then(|d| decode(&d)).unwrap_or_default()
}

/// Origins with unwritten changes, and the thread that writes them.
struct Flusher {
    dirty: Mutex<std::collections::BTreeSet<String>>,
    wake: std::sync::Condvar,
}

fn flusher() -> &'static Flusher {
    static F: OnceLock<Flusher> = OnceLock::new();
    static STARTED: std::sync::Once = std::sync::Once::new();
    let f = F.get_or_init(|| Flusher { dirty: Mutex::new(Default::default()), wake: std::sync::Condvar::new() });
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new().name("narsil-idb-flush".to_string()).spawn(flush_loop);
    });
    f
}

fn mark_dirty(origin: &str) {
    if !persistent(origin) {
        return;
    }
    let f = flusher();
    if let Ok(mut d) = f.dirty.lock() {
        d.insert(origin.to_string());
    }
    f.wake.notify_all();
}

/// Waits for changes, lets them settle a moment, writes each origin.
fn flush_loop() {
    let f = flusher();
    loop {
        let mut d = f.dirty.lock().unwrap_or_else(|e| e.into_inner());
        while d.is_empty() {
            d = f.wake.wait(d).unwrap_or_else(|e| e.into_inner());
        }
        drop(d);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let origins: Vec<String> = std::mem::take(&mut *f.dirty.lock().unwrap_or_else(|e| e.into_inner())).into_iter().collect();
        for origin in origins {
            let bytes = {
                let s = storage();
                s.origins.get(&origin).map(encode)
            };
            if let Some(bytes) = bytes {
                let path = crate::idb_file::file_of(&origin);
                if let Err(e) = crate::idb_file::write_file(&path, &bytes) {
                    eprintln!("narsil IndexedDB: {} not written: {e}", path.display());
                }
            }
        }
    }
}

struct Transaction {
    origin: String,
    db: String,
    /// The whole database before a versionchange transaction (`None`: it
    /// did not exist yet).
    whole: Option<Option<Database>>,
    /// Stores as they were before the first change (`None`: did not exist).
    stores: BTreeMap<String, Option<ObjectStore>>,
}

#[derive(Default)]
struct Storage {
    origins: HashMap<String, OriginData>,
    transactions: HashMap<u64, Transaction>,
    next_txn: u64,
}

fn storage() -> std::sync::MutexGuard<'static, Storage> {
    static S: OnceLock<Mutex<Storage>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Storage::default())).lock().unwrap_or_else(|e| e.into_inner())
}

fn record_bytes(key: &Key, r: &Record) -> usize {
    key.bytes() + r.value.bytes + r.index_keys.iter().map(|(n, k)| n.len() + k.bytes() * 2).sum::<usize>()
}

fn store_bytes(s: &ObjectStore) -> usize {
    s.records.iter().map(|(k, r)| record_bytes(k, r)).sum()
}

fn arg_text(args: &[Wert], i: usize) -> String {
    match args.get(i) {
        Some(Wert::Text(t)) => t.clone(),
        Some(Wert::Null) | Some(Wert::Undefiniert) | None => String::new(),
        Some(w) => w.als_text(),
    }
}

fn arg_num(args: &[Wert], i: usize) -> f64 {
    match args.get(i) {
        Some(Wert::Zahl(n)) => *n,
        _ => 0.0,
    }
}

/// The source of a read: `null` is the object store, a string (also the
/// empty one) names an index.
fn arg_index(args: &[Wert], i: usize) -> Option<String> {
    match args.get(i) {
        Some(Wert::Text(t)) => Some(t.clone()),
        _ => None,
    }
}

fn arg_bool(args: &[Wert], i: usize) -> bool {
    matches!(args.get(i), Some(Wert::Wahr(true)))
}

fn err(name: &str, text: &str) -> String {
    format!("{name}: {text}")
}

impl Storage {
    fn origin(&mut self, origin: &str) -> &mut OriginData {
        self.origins.entry(origin.to_string()).or_insert_with(|| load(origin))
    }

    fn txn(&self, id: u64) -> Result<&Transaction, String> {
        self.transactions.get(&id).ok_or_else(|| err("TransactionInactiveError", "the transaction is not active"))
    }

    /// Snapshot a store before its first change in transaction `id`.
    fn touch(&mut self, id: u64, store: &str) -> Result<(), String> {
        let (origin, db) = {
            let t = self.txn(id)?;
            if t.stores.contains_key(store) || t.whole.is_some() {
                return Ok(());
            }
            (t.origin.clone(), t.db.clone())
        };
        let snapshot = self.origins.get(&origin).and_then(|o| o.dbs.get(&db)).and_then(|d| d.stores.get(store)).cloned();
        if let Some(t) = self.transactions.get_mut(&id) {
            t.stores.insert(store.to_string(), snapshot);
        }
        Ok(())
    }

    fn db_mut(&mut self, origin: &str, db: &str) -> Result<&mut Database, String> {
        self.origin(origin).dbs.get_mut(db).ok_or_else(|| err("InvalidStateError", "the database is gone"))
    }

    fn store_mut(&mut self, origin: &str, db: &str, store: &str) -> Result<&mut ObjectStore, String> {
        self.db_mut(origin, db)?.stores.get_mut(store).ok_or_else(|| err("InvalidStateError", "the object store is gone"))
    }

    fn store(&self, origin: &str, db: &str, store: &str) -> Result<&ObjectStore, String> {
        self.origins
            .get(origin)
            .and_then(|o| o.dbs.get(db))
            .and_then(|d| d.stores.get(store))
            .ok_or_else(|| err("InvalidStateError", "the object store is gone"))
    }

    fn recount(&mut self, origin: &str) {
        let o = self.origin(origin);
        o.bytes = o.dbs.values().map(|d| d.stores.values().map(store_bytes).sum::<usize>()).sum();
    }
}

/// Removes the index entries of a record.
fn unindex(store: &mut ObjectStore, primary: &Key, rec: &Record) {
    for (name, k) in &rec.index_keys {
        if let Some(ix) = store.indexes.get_mut(name) {
            if let Some(set) = ix.entries.get_mut(k) {
                set.remove(primary);
                if set.is_empty() {
                    ix.entries.remove(k);
                }
            }
        }
    }
}

/// The index keys a script computed for one record: `[[name, [keys]]...]`.
fn index_keys(w: Option<&Wert>) -> Vec<(String, Key)> {
    let mut out = Vec::new();
    if let Some(Wert::Feld(list)) = w {
        for e in list {
            if let Wert::Feld(pair) = e
                && let (Some(Wert::Text(name)), Some(Wert::Feld(keys))) = (pair.first(), pair.get(1))
            {
                let mut seen: BTreeSet<Key> = BTreeSet::new();
                for k in keys {
                    if let Some(key) = Key::from_wert(k, 0)
                        && seen.insert(key.clone())
                    {
                        out.push((name.clone(), key));
                    }
                }
            }
        }
    }
    out
}

/// Would these index keys break a unique index (another record has one)?
fn unique_violation(store: &ObjectStore, primary: &Key, keys: &[(String, Key)]) -> bool {
    for (name, k) in keys {
        if let Some(ix) = store.indexes.get(name)
            && ix.unique
        {
            if ix.entries.get(k).is_some_and(|set| set.iter().any(|pk| pk != primary)) {
                return true;
            }
        }
    }
    false
}

fn store_meta(name: &str, s: &ObjectStore) -> Wert {
    let indexes = s
        .indexes
        .iter()
        .map(|(n, ix)| {
            Wert::Feld(vec![
                Wert::Text(n.clone()),
                Wert::Text(ix.key_path.clone()),
                Wert::Wahr(ix.unique),
                Wert::Wahr(ix.multi_entry),
            ])
        })
        .collect();
    Wert::Feld(vec![
        Wert::Text(name.to_string()),
        s.key_path.clone().map_or(Wert::Null, Wert::Text),
        Wert::Wahr(s.auto_increment),
        Wert::Feld(indexes),
    ])
}

/// One record for the script: `[key, primaryKey, valueHandle]`.
fn row(key: &Key, primary: &Key, rec: Option<&Record>) -> Wert {
    Wert::Feld(vec![
        key.to_wert(),
        primary.to_wert(),
        rec.map_or(Wert::Null, |r| Wert::Zahl(f64::from(structured::put(r.value.clone())))),
    ])
}

/// The records of a store or index in a range and direction:
/// `(key, primary key)` pairs, at most `limit`, by repeated cursor steps.
fn walk(store: &ObjectStore, index: Option<&str>, range: &Range, reverse: bool, unique: bool, limit: usize) -> Result<Vec<(Key, Key)>, String> {
    let mut out: Vec<(Key, Key)> = Vec::new();
    let mut pos: Option<(Key, Key)> = None;
    while out.len() < limit {
        let next = cursor_step(store, index, range, reverse, unique, pos.as_ref().map(|p| &p.0), pos.as_ref().map(|p| &p.1), false)?;
        match next {
            Some(r) => {
                pos = Some(r.clone());
                out.push(r);
            }
            None => break,
        }
    }
    Ok(out)
}

/// The script's operations, `__narsilIdb(op, ...)`, for one origin.
fn op(origin: &str, args: &[Wert]) -> Result<Wert, String> {
    let name = arg_text(args, 0);
    let a = &args[1.min(args.len())..];
    if name == "opaque" {
        return Ok(Wert::Wahr(origin == "null"));
    }
    let mut s = storage();
    match name.as_str() {
        // [[name, version]...]
        // A database whose first upgrade has not committed is not listed;
        // one in an upgrade shows its committed version.
        "databases" => {
            let mut before: HashMap<String, Option<f64>> = HashMap::new();
            for t in s.transactions.values() {
                if t.origin == origin
                    && let Some(whole) = &t.whole
                {
                    before.insert(t.db.clone(), whole.as_ref().map(|d| d.version));
                }
            }
            Ok(Wert::Feld(
                s.origin(origin)
                    .dbs
                    .iter()
                    .filter_map(|(n, d)| match before.get(n) {
                        Some(None) => None,
                        Some(Some(v)) => Some(Wert::Feld(vec![Wert::Text(n.clone()), Wert::Zahl(*v)])),
                        None => Some(Wert::Feld(vec![Wert::Text(n.clone()), Wert::Zahl(d.version)])),
                    })
                    .collect(),
            ))
        }
        // version of a database, 0 if it does not exist
        "version" => {
            let db = arg_text(a, 0);
            Ok(Wert::Zahl(s.origin(origin).dbs.get(&db).map_or(0.0, |d| d.version)))
        }
        // begin(db, versionchangeTo) -> transaction id; a version > 0 makes
        // it the upgrade transaction (creating the database if needed).
        "begin" => {
            if s.transactions.len() >= MAX_TRANSACTIONS {
                return Err(err("UnknownError", "too many transactions"));
            }
            let db = arg_text(a, 0);
            let upgrade = arg_num(a, 1);
            s.next_txn += 1;
            let id = s.next_txn;
            let mut t = Transaction { origin: origin.to_string(), db: db.clone(), whole: None, stores: BTreeMap::new() };
            if upgrade > 0.0 {
                let o = s.origin(origin);
                let before = o.dbs.get(&db).cloned();
                o.dbs.entry(db.clone()).or_default().version = upgrade;
                t.whole = Some(before);
            }
            s.transactions.insert(id, t);
            Ok(Wert::Zahl(id as f64))
        }
        "commit" => {
            let id = arg_num(a, 0) as u64;
            if let Some(t) = s.transactions.remove(&id) {
                s.recount(&t.origin);
                if t.whole.is_some() || !t.stores.is_empty() {
                    mark_dirty(&t.origin);
                }
            }
            Ok(Wert::Undefiniert)
        }
        "abort" => {
            let id = arg_num(a, 0) as u64;
            if let Some(t) = s.transactions.remove(&id) {
                let o = s.origin(&t.origin);
                if let Some(whole) = t.whole {
                    match whole {
                        Some(d) => {
                            o.dbs.insert(t.db.clone(), d);
                        }
                        None => {
                            o.dbs.remove(&t.db);
                        }
                    }
                } else if let Some(d) = o.dbs.get_mut(&t.db) {
                    for (name, snap) in t.stores {
                        match snap {
                            Some(st) => {
                                d.stores.insert(name, st);
                            }
                            None => {
                                d.stores.remove(&name);
                            }
                        }
                    }
                }
                s.recount(&t.origin);
            }
            Ok(Wert::Undefiniert)
        }
        "delete_db" => {
            let db = arg_text(a, 0);
            let old = s.origin(origin).dbs.remove(&db).map_or(0.0, |d| d.version);
            s.recount(origin);
            mark_dirty(origin);
            Ok(Wert::Zahl(old))
        }
        // [[name, keyPath, autoIncrement, [[index, keyPath, unique, multiEntry]...]]...]
        "stores" => {
            let db = arg_text(a, 0);
            let list = s
                .origin(origin)
                .dbs
                .get(&db)
                .map(|d| d.stores.iter().map(|(n, st)| store_meta(n, st)).collect())
                .unwrap_or_default();
            Ok(Wert::Feld(list))
        }
        "create_store" => {
            let (id, db, store) = (arg_num(a, 0) as u64, arg_text(a, 1), arg_text(a, 2));
            let key_path = match a.get(3) {
                Some(Wert::Text(t)) => Some(t.clone()),
                _ => None,
            };
            let auto = arg_bool(a, 4);
            s.txn(id)?;
            let d = s.db_mut(origin, &db)?;
            if d.stores.contains_key(&store) {
                return Err(err("ConstraintError", "an object store with this name exists"));
            }
            d.stores.insert(
                store,
                ObjectStore { key_path, auto_increment: auto, current: 1.0, records: BTreeMap::new(), indexes: BTreeMap::new() },
            );
            Ok(Wert::Undefiniert)
        }
        "delete_store" => {
            let (id, db, store) = (arg_num(a, 0) as u64, arg_text(a, 1), arg_text(a, 2));
            s.txn(id)?;
            s.db_mut(origin, &db)?.stores.remove(&store);
            s.recount(origin);
            Ok(Wert::Undefiniert)
        }
        "rename_store" => {
            let (id, db, old, new) = (arg_num(a, 0) as u64, arg_text(a, 1), arg_text(a, 2), arg_text(a, 3));
            s.txn(id)?;
            let d = s.db_mut(origin, &db)?;
            if d.stores.contains_key(&new) {
                return Err(err("ConstraintError", "an object store with this name exists"));
            }
            if let Some(st) = d.stores.remove(&old) {
                d.stores.insert(new, st);
            }
            Ok(Wert::Undefiniert)
        }
        // create_index(txn, db, store, name, keyPath, unique, multiEntry, [[primary, [[name,[keys]]]]...])
        "create_index" => {
            let (id, db, store, index) = (arg_num(a, 0) as u64, arg_text(a, 1), arg_text(a, 2), arg_text(a, 3));
            let key_path = arg_text(a, 4);
            let (unique, multi_entry) = (arg_bool(a, 5), arg_bool(a, 6));
            s.txn(id)?;
            let st = s.store_mut(origin, &db, &store)?;
            if st.indexes.contains_key(&index) {
                return Err(err("ConstraintError", "an index with this name exists"));
            }
            st.indexes.insert(index.clone(), Index { key_path, unique, multi_entry, entries: BTreeMap::new() });
            // The keys of the existing records (computed by the script).
            let mut violation = false;
            if let Some(Wert::Feld(rows)) = a.get(7) {
                for r in rows {
                    let Wert::Feld(pair) = r else { continue };
                    let Some(primary) = pair.first().and_then(|w| Key::from_wert(w, 0)) else { continue };
                    let keys = index_keys(Some(&Wert::Feld(vec![Wert::Feld(vec![Wert::Text(index.clone()), pair.get(1).cloned().unwrap_or(Wert::Feld(Vec::new()))])])));
                    if unique_violation(st, &primary, &keys) {
                        violation = true;
                    }
                    for (n, k) in keys {
                        if let Some(ix) = st.indexes.get_mut(&n) {
                            ix.entries.entry(k.clone()).or_default().insert(primary.clone());
                        }
                        if let Some(rec) = st.records.get_mut(&primary) {
                            rec.index_keys.push((n, k));
                        }
                    }
                }
            }
            // A unique index over existing duplicates aborts the upgrade
            // transaction (IndexedDB §4.5 createIndex step 11, asynchronous).
            Ok(Wert::Wahr(violation))
        }
        "delete_index" => {
            let (id, db, store, index) = (arg_num(a, 0) as u64, arg_text(a, 1), arg_text(a, 2), arg_text(a, 3));
            s.txn(id)?;
            let st = s.store_mut(origin, &db, &store)?;
            st.indexes.remove(&index);
            for rec in st.records.values_mut() {
                rec.index_keys.retain(|(n, _)| *n != index);
            }
            Ok(Wert::Undefiniert)
        }
        "rename_index" => {
            let (id, db, store, old, new) = (arg_num(a, 0) as u64, arg_text(a, 1), arg_text(a, 2), arg_text(a, 3), arg_text(a, 4));
            s.txn(id)?;
            let st = s.store_mut(origin, &db, &store)?;
            if st.indexes.contains_key(&new) {
                return Err(err("ConstraintError", "an index with this name exists"));
            }
            if let Some(ix) = st.indexes.remove(&old) {
                st.indexes.insert(new.clone(), ix);
            }
            for rec in st.records.values_mut() {
                for (n, _) in rec.index_keys.iter_mut() {
                    if *n == old {
                        *n = new.clone();
                    }
                }
            }
            Ok(Wert::Undefiniert)
        }
        // next generated key, or an error at the end of the generator
        "gen_key" => {
            let (id, db, store) = (arg_num(a, 0) as u64, arg_text(a, 1), arg_text(a, 2));
            s.touch(id, &store)?;
            let st = s.store_mut(origin, &db, &store)?;
            if st.current > MAX_GENERATED {
                return Err(err("ConstraintError", "the key generator is exhausted"));
            }
            let k = st.current;
            st.current += 1.0;
            Ok(Key::Number(k).to_wert())
        }
        // A failed store gives its generated key back (the request is atomic).
        "gen_restore" => {
            let (id, db, store) = (arg_num(a, 0) as u64, arg_text(a, 1), arg_text(a, 2));
            let key = a.get(3).and_then(|w| Key::from_wert(w, 0));
            s.touch(id, &store)?;
            let st = s.store_mut(origin, &db, &store)?;
            if let Some(Key::Number(n)) = key
                && st.current == n + 1.0
            {
                st.current = n;
            }
            Ok(Wert::Undefiniert)
        }
        // put(txn, db, store, key, valueHandle, indexKeys, noOverwrite)
        "put" => {
            let (id, db, store) = (arg_num(a, 0) as u64, arg_text(a, 1), arg_text(a, 2));
            let key = a.get(3).and_then(|w| Key::from_wert(w, 0)).ok_or_else(|| err("DataError", "invalid key"))?;
            let value = structured::take(arg_num(a, 4) as u32).ok_or_else(|| err("DataCloneError", "the value is gone"))?;
            let keys = index_keys(a.get(5));
            let no_overwrite = arg_bool(a, 6);
            s.touch(id, &store)?;
            let used = s.origin(origin).bytes;
            let st = s.store_mut(origin, &db, &store)?;
            if no_overwrite && st.records.contains_key(&key) {
                return Err(err("ConstraintError", "a record with this key exists"));
            }
            if unique_violation(st, &key, &keys) {
                return Err(err("ConstraintError", "a unique index already has this key"));
            }
            let rec = Record { value, index_keys: keys };
            let add = record_bytes(&key, &rec);
            if used.saturating_add(add) > ORIGIN_QUOTA {
                return Err(err("QuotaExceededError", "the origin's storage quota is used up"));
            }
            // Possibly update the key generator (IndexedDB §2.11).
            if st.auto_increment
                && let Key::Number(n) = key
                && n >= st.current
            {
                st.current = if n >= MAX_GENERATED { f64::INFINITY } else { n.floor() + 1.0 };
            }
            if let Some(old) = st.records.remove(&key) {
                unindex(st, &key, &old);
            }
            for (n, k) in &rec.index_keys {
                if let Some(ix) = st.indexes.get_mut(n) {
                    ix.entries.entry(k.clone()).or_default().insert(key.clone());
                }
            }
            st.records.insert(key.clone(), rec);
            s.origin(origin).bytes = used.saturating_add(add);
            Ok(key.to_wert())
        }
        // get(db, store, index, range, reverse, unique, limit, withValues) -> [[key, primary, handle]...]
        "get" => {
            let (db, store, index) = (arg_text(a, 0), arg_text(a, 1), arg_index(a, 2));
            let range = Range::from_wert(a.get(3))?;
            let (reverse, unique) = (arg_bool(a, 4), arg_bool(a, 5));
            let limit = match arg_num(a, 6) {
                n if n >= 1.0 => n as usize,
                _ => usize::MAX,
            };
            let values = arg_bool(a, 7);
            let st = s.store(origin, &db, &store)?;
            let rows = walk(st, index.as_deref(), &range, reverse, unique, limit)?;
            Ok(Wert::Feld(rows.iter().map(|(k, p)| row(k, p, if values { st.records.get(p) } else { None })).collect()))
        }
        "count" => {
            let (db, store, index) = (arg_text(a, 0), arg_text(a, 1), arg_index(a, 2));
            let range = Range::from_wert(a.get(3))?;
            let st = s.store(origin, &db, &store)?;
            let n = if let Some(index) = index {
                let ix = st.indexes.get(&index).ok_or_else(|| err("InvalidStateError", "the index is gone"))?;
                ix.entries.iter().filter(|(k, _)| range.contains(k)).map(|(_, set)| set.len()).sum()
            } else {
                st.records.keys().filter(|k| range.contains(k)).count()
            };
            Ok(Wert::Zahl(n as f64))
        }
        // cursor(db, store, index, range, direction, posKey|null, posPrimary|null, inclusive, withValue)
        // -> the next [key, primary, handle] after the position, or null
        "cursor" => {
            let (db, store, index) = (arg_text(a, 0), arg_text(a, 1), arg_index(a, 2));
            let range = Range::from_wert(a.get(3))?;
            let dir = arg_text(a, 4);
            let pos_key = a.get(5).and_then(|w| Key::from_wert(w, 0));
            let pos_primary = a.get(6).and_then(|w| Key::from_wert(w, 0));
            let inclusive = arg_bool(a, 7);
            let values = arg_bool(a, 8);
            let reverse = dir.starts_with("prev");
            let unique = dir.ends_with("unique");
            let st = s.store(origin, &db, &store)?;
            let found = cursor_step(st, index.as_deref(), &range, reverse, unique, pos_key.as_ref(), pos_primary.as_ref(), inclusive)?;
            Ok(match found {
                Some((k, p)) => row(&k, &p, if values { st.records.get(&p) } else { None }),
                None => Wert::Null,
            })
        }
        // delete(txn, db, store, range)
        "delete" => {
            let (id, db, store) = (arg_num(a, 0) as u64, arg_text(a, 1), arg_text(a, 2));
            let range = Range::from_wert(a.get(3))?;
            s.touch(id, &store)?;
            let st = s.store_mut(origin, &db, &store)?;
            let doomed: Vec<Key> = st.records.keys().filter(|k| range.contains(k)).cloned().collect();
            for k in doomed {
                if let Some(rec) = st.records.remove(&k) {
                    unindex(st, &k, &rec);
                }
            }
            s.recount(origin);
            Ok(Wert::Undefiniert)
        }
        "clear" => {
            let (id, db, store) = (arg_num(a, 0) as u64, arg_text(a, 1), arg_text(a, 2));
            s.touch(id, &store)?;
            let st = s.store_mut(origin, &db, &store)?;
            st.records.clear();
            for ix in st.indexes.values_mut() {
                ix.entries.clear();
            }
            s.recount(origin);
            Ok(Wert::Undefiniert)
        }
        // compare two keys: -1, 0, 1
        "cmp" => {
            let x = a.first().and_then(|w| Key::from_wert(w, 0)).ok_or_else(|| err("DataError", "invalid key"))?;
            let y = a.get(1).and_then(|w| Key::from_wert(w, 0)).ok_or_else(|| err("DataError", "invalid key"))?;
            Ok(Wert::Zahl(match x.cmp(&y) {
                Ordering::Less => -1.0,
                Ordering::Equal => 0.0,
                Ordering::Greater => 1.0,
            }))
        }
        _ => Err(err("NotSupportedError", "unknown IndexedDB operation")),
    }
}

/// The next position of a cursor (IndexedDB §2.10 "iterate a cursor"):
/// the first record after (or, with `inclusive`, at) the position in the
/// cursor's direction; for an index the position is (key, primary key).
#[allow(clippy::too_many_arguments)]
fn cursor_step(
    st: &ObjectStore,
    index: Option<&str>,
    range: &Range,
    reverse: bool,
    unique: bool,
    pos_key: Option<&Key>,
    pos_primary: Option<&Key>,
    inclusive: bool,
) -> Result<Option<(Key, Key)>, String> {
    use std::ops::Bound::{Excluded, Included, Unbounded};
    let past_end = |k: &Key| -> bool {
        if reverse {
            range.lower.as_ref().is_some_and(|l| k < l)
        } else {
            range.upper.as_ref().is_some_and(|u| k > u)
        }
    };
    let Some(index) = index else {
        let bound = match pos_key {
            Some(k) if inclusive => Included(k),
            Some(k) => Excluded(k),
            None => Unbounded,
        };
        let it: Box<dyn Iterator<Item = &Key>> = if reverse {
            Box::new(st.records.range::<Key, _>((Unbounded, bound)).rev().map(|(k, _)| k))
        } else {
            Box::new(st.records.range::<Key, _>((bound, Unbounded)).map(|(k, _)| k))
        };
        for k in it {
            if past_end(k) {
                return Ok(None);
            }
            if range.contains(k) {
                return Ok(Some((k.clone(), k.clone())));
            }
        }
        return Ok(None);
    };
    let ix = st.indexes.get(index).ok_or_else(|| err("InvalidStateError", "the index is gone"))?;
    let bound = match pos_key {
        Some(k) => Included(k),
        None => Unbounded,
    };
    let it: Box<dyn Iterator<Item = (&Key, &BTreeSet<Key>)>> = if reverse {
        Box::new(ix.entries.range::<Key, _>((Unbounded, bound)).rev())
    } else {
        Box::new(ix.entries.range::<Key, _>((bound, Unbounded)))
    };
    for (k, set) in it {
        if past_end(k) {
            return Ok(None);
        }
        if !range.contains(k) {
            continue;
        }
        let first = || set.iter().next().cloned();
        if pos_key == Some(k) {
            if unique {
                if inclusive && let Some(p) = first() {
                    return Ok(Some((k.clone(), p)));
                }
                continue;
            }
            let p = match pos_primary {
                None if inclusive => if reverse { set.iter().next_back().cloned() } else { first() },
                None => None,
                Some(pp) => {
                    let b = if inclusive { Included(pp) } else { Excluded(pp) };
                    if reverse {
                        set.range::<Key, _>((Unbounded, b)).next_back().cloned()
                    } else {
                        set.range::<Key, _>((b, Unbounded)).next().cloned()
                    }
                }
            };
            if let Some(p) = p {
                return Ok(Some((k.clone(), p)));
            }
            continue;
        }
        // A new key: the first record of it in the direction (for the
        // unique directions the one with the lowest primary key).
        let p = if reverse && !unique { set.iter().next_back().cloned() } else { first() };
        if let Some(p) = p {
            return Ok(Some((k.clone(), p)));
        }
    }
    Ok(None)
}

/// Binds `__narsilIdb` for a script context of `origin` (the page, a frame
/// or a worker).
pub fn bind(engine: &mut Engine, origin: String) {
    engine.host_funktion("__narsilIdb", move |args: &[Wert]| op(&origin, args));
}

/// The storage key of a page: its origin, and `file://` for local files
/// (they keep IndexedDB, in memory, as in Chromium). An opaque origin
/// ("null": data: documents and workers) has no IndexedDB — `open()`
/// throws a SecurityError (IndexedDB §4.2 step 1).
pub fn storage_key(url: &str, origin: &str) -> String {
    if origin == "null" && url.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) {
        "file://".to_string()
    } else {
        origin.to_string()
    }
}

/// The script side (runs after the preamble, before `messaging.js`).
pub const INDEXEDDB_JS: &str = include_str!("indexeddb.js");

#[cfg(test)]
mod tests {
    use super::*;

    fn k(n: f64) -> Key {
        Key::Number(n)
    }

    #[test]
    fn keys_order_by_type_then_value() {
        let mut v = vec![
            Key::Array(vec![]),
            Key::Binary(vec![1]),
            Key::String("b".encode_utf16().collect()),
            Key::String("a".encode_utf16().collect()),
            Key::Date(5.0),
            k(10.0),
            k(-1.0),
        ];
        v.sort();
        let ranks: Vec<u8> = v.iter().map(Key::rank).collect();
        assert_eq!(ranks, vec![0, 0, 1, 2, 2, 3, 4]);
        // UTF-16 order: a surrogate pair sorts before U+FFFF.
        assert!(Key::String("\u{10000}".encode_utf16().collect()) < Key::String("\u{FFFF}".encode_utf16().collect()));
    }

    #[test]
    fn keys_cross_the_host_boundary_as_text() {
        let big = Key::Array((0..10_001).map(|i| k(f64::from(i))).collect());
        let keys = vec![
            k(-0.0),
            k(f64::NEG_INFINITY),
            k(5e-324),
            k(1.7976931348623157e308),
            Key::Date(-1e13),
            Key::String("a;b\\c\u{10000}".encode_utf16().collect()),
            Key::String(vec![0xD800, 0x41]),
            Key::String(Vec::new()),
            Key::Binary(vec![0, 255, 16]),
            Key::Array(vec![Key::Array(vec![]), k(1.0), Key::String("x".encode_utf16().collect())]),
            big,
        ];
        for key in keys {
            let w = key.to_wert();
            let back = Key::from_wert(&w, 0).expect("parses");
            assert_eq!(back.cmp(&key), Ordering::Equal, "{w:?}");
            assert_eq!(back.to_wert(), w);
        }
        assert_eq!(Key::from_wert(&Wert::Text("n1;".into()), 0).map(|k| k.to_wert()), Some(Wert::Text("n1e0;".into())));
        for bad in ["", "x1;", "nNaN;", "a;n1;", "e;", "b0;", "s\\q;", "u70000;"] {
            assert!(Key::from_wert(&Wert::Text(bad.into()), 0).is_none(), "{bad}");
        }
    }

    #[test]
    fn origin_data_survives_encode_and_decode() {
        let mut o = OriginData::default();
        let mut st = ObjectStore { key_path: Some("\"id\"".into()), auto_increment: true, current: 3.0, records: BTreeMap::new(), indexes: BTreeMap::new() };
        st.indexes.insert("byName".into(), Index { key_path: "\"name\"".into(), unique: false, multi_entry: false, entries: BTreeMap::new() });
        let rec = Record { value: SerializedMessage::string("v"), index_keys: vec![("byName".into(), Key::String("a".encode_utf16().collect()))] };
        st.records.insert(Key::Number(1.0), rec);
        let mut db = Database { version: 2.0, stores: BTreeMap::new() };
        db.stores.insert("s".into(), st);
        o.dbs.insert("db".into(), db);
        let back = decode(&encode(&o)).expect("decodes");
        let st = &back.dbs["db"].stores["s"];
        assert_eq!(back.dbs["db"].version, 2.0);
        assert_eq!(st.current, 3.0);
        assert_eq!(st.records.len(), 1);
        assert_eq!(st.indexes["byName"].entries.len(), 1);
        assert!(decode(b"NIDB1\n\xff").is_none());
    }

    #[test]
    fn put_get_and_abort_restore() {
        let origin = "test://idb-unit";
        let w = |v: Vec<Wert>| op(origin, &v);
        let t = match w(vec![Wert::Text("begin".into()), Wert::Text("db".into()), Wert::Zahl(1.0)]).unwrap() {
            Wert::Zahl(n) => n,
            _ => panic!(),
        };
        w(vec![Wert::Text("create_store".into()), Wert::Zahl(t), Wert::Text("db".into()), Wert::Text("s".into()), Wert::Null, Wert::Wahr(true)]).unwrap();
        w(vec![Wert::Text("commit".into()), Wert::Zahl(t)]).unwrap();
        let t2 = match w(vec![Wert::Text("begin".into()), Wert::Text("db".into()), Wert::Zahl(0.0)]).unwrap() {
            Wert::Zahl(n) => n,
            _ => panic!(),
        };
        let h = structured::put(SerializedMessage::string("v"));
        w(vec![Wert::Text("put".into()), Wert::Zahl(t2), Wert::Text("db".into()), Wert::Text("s".into()), k(3.0).to_wert(), Wert::Zahl(f64::from(h)), Wert::Feld(vec![]), Wert::Wahr(false)]).unwrap();
        let count = |w: &dyn Fn(Vec<Wert>) -> Result<Wert, String>| w(vec![Wert::Text("count".into()), Wert::Text("db".into()), Wert::Text("s".into()), Wert::Null, Wert::Null]).unwrap();
        assert_eq!(count(&w), Wert::Zahl(1.0));
        w(vec![Wert::Text("abort".into()), Wert::Zahl(t2)]).unwrap();
        assert_eq!(count(&w), Wert::Zahl(0.0));
    }
}
