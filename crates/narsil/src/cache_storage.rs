//! Cache API storage (Service Workers §5.4, §5.5): the named caches of each
//! origin with their request/response pairs, shared by the page, its frames
//! and its workers. The API objects are `cache_storage.js`. Bounded per
//! origin ([`ORIGIN_QUOTA`]). The caches of an http(s) origin live on disk
//! (`<profile>/cache_storage/<origin>.cache`, so `NARSIL_PROFIL` applies):
//! loaded on first use, written by a background thread shortly after a
//! change; other origins stay in memory.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use narsil_jsvm::structured::{self, Ser, SerializedMessage};
use narsil_jsvm::{Engine, Wert};

/// Bytes of response bodies one origin may keep.
const ORIGIN_QUOTA: usize = 256 * 1024 * 1024;
/// Hard cap of named caches per origin.
const MAX_CACHES: usize = 1024;
/// A deleted cache lives on under this prefix while Cache objects may still
/// use it (Service Workers §5.5.5: delete removes the name, not the cache);
/// at most [`MAX_DOOMED`] of them per origin, the oldest go first.
const DOOMED: &str = "\u{0}doomed\u{0}";
const MAX_DOOMED: usize = 16;

fn doomed(name: &str) -> bool {
    name.starts_with(DOOMED)
}

#[derive(Clone)]
struct Entry {
    url: String,
    method: String,
    request_headers: Vec<(String, String)>,
    status: f64,
    status_text: String,
    headers: Vec<(String, String)>,
    body: Arc<Vec<u8>>,
    response_url: String,
    response_type: String,
}

#[derive(Default, Clone)]
struct OriginCaches {
    /// Counter of the keys of deleted caches.
    next_doomed: u64,
    /// In creation order (CacheStorage.keys()).
    caches: Vec<(String, Vec<Entry>)>,
    bytes: usize,
}

fn store() -> std::sync::MutexGuard<'static, HashMap<String, OriginCaches>> {
    static S: OnceLock<Mutex<HashMap<String, OriginCaches>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap_or_else(|e| e.into_inner())
}

/// Only real origins are written to disk.
fn persistent(origin: &str) -> bool {
    origin.starts_with("http://") || origin.starts_with("https://")
}

const MAGIC: &[u8] = b"NCACHE1\n";
/// Bounds while reading a file (it is never trusted).
const MAX_FILE_ITEMS: u64 = 1 << 20;

fn file_of(origin: &str) -> std::path::PathBuf {
    crate::idb_file::profile_file("cache_storage", origin, "cache")
}

fn write_pairs(w: &mut crate::idb_file::Writer, p: &[(String, String)]) {
    w.u64(p.len() as u64);
    for (n, v) in p {
        w.text(n);
        w.text(v);
    }
}

fn read_pairs(r: &mut crate::idb_file::Reader) -> Option<Vec<(String, String)>> {
    let n = r.u64()?;
    if n > MAX_FILE_ITEMS {
        return None;
    }
    let mut out = Vec::new();
    for _ in 0..n {
        out.push((r.text()?, r.text()?));
    }
    Some(out)
}

fn encode(o: &OriginCaches) -> Vec<u8> {
    let mut w = crate::idb_file::Writer::with_magic(MAGIC);
    let named: Vec<&(String, Vec<Entry>)> = o.caches.iter().filter(|(n, _)| !doomed(n)).collect();
    w.u64(named.len() as u64);
    for (name, list) in named {
        w.text(name);
        w.u64(list.len() as u64);
        for e in list {
            w.text(&e.url);
            w.text(&e.method);
            write_pairs(&mut w, &e.request_headers);
            w.f64(e.status);
            w.text(&e.status_text);
            write_pairs(&mut w, &e.headers);
            w.bytes(&e.body);
            w.text(&e.response_url);
            w.text(&e.response_type);
        }
    }
    w.out
}

fn decode(data: &[u8]) -> Option<OriginCaches> {
    let mut r = crate::idb_file::Reader::with_magic(data, MAGIC)?;
    let mut o = OriginCaches::default();
    let n = r.u64()?;
    if n > MAX_CACHES as u64 {
        return None;
    }
    for _ in 0..n {
        let name = r.text()?;
        let m = r.u64()?;
        if m > MAX_FILE_ITEMS {
            return None;
        }
        let mut list = Vec::new();
        for _ in 0..m {
            let e = Entry {
                url: r.text()?,
                method: r.text()?,
                request_headers: read_pairs(&mut r)?,
                status: r.f64()?,
                status_text: r.text()?,
                headers: read_pairs(&mut r)?,
                body: Arc::new(r.bytes()?),
                response_url: r.text()?,
                response_type: r.text()?,
            };
            o.bytes = o.bytes.saturating_add(e.body.len());
            list.push(e);
        }
        o.caches.push((name, list));
    }
    if !r.done() || o.bytes > ORIGIN_QUOTA {
        return None;
    }
    Some(o)
}

fn load(origin: &str) -> OriginCaches {
    if !persistent(origin) {
        return OriginCaches::default();
    }
    std::fs::read(file_of(origin)).ok().and_then(|d| decode(&d)).unwrap_or_default()
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
        let _ = std::thread::Builder::new().name("narsil-cache-flush".to_string()).spawn(flush_loop);
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

/// Waits for changes, lets them settle a moment, writes each origin (the
/// bodies are shared, the copy taken under the lock is cheap).
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
            let copy = store().get(&origin).cloned();
            if let Some(o) = copy {
                let path = file_of(&origin);
                if let Err(e) = crate::idb_file::write_file(&path, &encode(&o)) {
                    eprintln!("narsil Cache API: {} not written: {e}", path.display());
                }
            }
        }
    }
}

fn text(args: &[Wert], i: usize) -> String {
    match args.get(i) {
        Some(Wert::Text(t)) => t.clone(),
        Some(Wert::Null) | Some(Wert::Undefiniert) | None => String::new(),
        Some(w) => w.als_text(),
    }
}

fn flag(args: &[Wert], i: usize) -> bool {
    matches!(args.get(i), Some(Wert::Wahr(true)))
}

fn pairs(w: Option<&Wert>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(Wert::Feld(list)) = w {
        for p in list.iter().take(512) {
            if let Wert::Feld(nv) = p
                && let (Some(n), Some(v)) = (nv.first(), nv.get(1))
            {
                out.push((n.als_text().to_ascii_lowercase(), v.als_text()));
            }
        }
    }
    out
}

fn pairs_wert(p: &[(String, String)]) -> Wert {
    Wert::Feld(p.iter().map(|(n, v)| Wert::Feld(vec![Wert::Text(n.clone()), Wert::Text(v.clone())])).collect())
}

fn header<'a>(list: &'a [(String, String)], name: &str) -> Option<&'a str> {
    list.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

fn strip(url: &str, search: bool) -> &str {
    let url = url.split('#').next().unwrap_or(url);
    if search { url.split('?').next().unwrap_or(url) } else { url }
}

/// Service Workers §5.4.4 "request matches cached item".
fn matches(e: &Entry, url: &str, method: &str, headers: &[(String, String)], ignore_search: bool, ignore_method: bool, ignore_vary: bool) -> bool {
    if !ignore_method && method != "GET" {
        return false;
    }
    if strip(url, ignore_search) != strip(&e.url, ignore_search) {
        return false;
    }
    if !ignore_vary && let Some(vary) = header(&e.headers, "vary") {
        for field in vary.split(',').map(|f| f.trim().to_ascii_lowercase()).filter(|f| !f.is_empty()) {
            if field == "*" || header(headers, &field) != header(&e.request_headers, &field) {
                return false;
            }
        }
    }
    true
}

fn response_wert(e: &Entry) -> Wert {
    let blob = SerializedMessage {
        value: Ser::Blob { bytes: e.body.clone(), mime: header(&e.headers, "content-type").unwrap_or("").to_string() },
        buffers: Vec::new(),
        ports: Vec::new(),
        bytes: e.body.len(),
    };
    Wert::Feld(vec![
        Wert::Zahl(e.status),
        Wert::Text(e.status_text.clone()),
        pairs_wert(&e.headers),
        Wert::Zahl(f64::from(structured::put(blob))),
        Wert::Text(e.response_url.clone()),
        Wert::Text(e.response_type.clone()),
    ])
}

fn body_of(handle: f64) -> Vec<u8> {
    if handle < 1.0 {
        return Vec::new();
    }
    match structured::take(handle as u32).map(|m| m.value) {
        Some(Ser::Blob { bytes, .. }) | Some(Ser::File { bytes, .. }) => bytes.as_ref().clone(),
        Some(Ser::ArrayBuffer(b)) => b,
        Some(Ser::String(t)) => t.into_bytes(),
        _ => Vec::new(),
    }
}

fn op(origin: &str, args: &[Wert]) -> Result<Wert, String> {
    let name = text(args, 0);
    let a = &args[1.min(args.len())..];
    let mut s = store();
    let o = s.entry(origin.to_string()).or_insert_with(|| load(origin));
    let changes = matches!(name.as_str(), "open" | "delete" | "put" | "remove");
    let out = op_on(o, &name, a);
    drop(s);
    if changes && out.is_ok() {
        mark_dirty(origin);
    }
    out
}

fn op_on(o: &mut OriginCaches, name: &str, a: &[Wert]) -> Result<Wert, String> {
    match name {
        "keys" => Ok(Wert::Feld(o.caches.iter().filter(|(n, _)| !doomed(n)).map(|(n, _)| Wert::Text(n.clone())).collect())),
        "has" => {
            let n = text(a, 0);
            Ok(Wert::Wahr(!doomed(&n) && o.caches.iter().any(|(k, _)| *k == n)))
        }
        "open" => {
            let n = text(a, 0);
            if !o.caches.iter().any(|(k, _)| *k == n) {
                if o.caches.len() >= MAX_CACHES {
                    return Err("QuotaExceededError: too many caches".to_string());
                }
                o.caches.push((n, Vec::new()));
            }
            Ok(Wert::Wahr(true))
        }
        // delete(name) -> the key the cache lives on under (false: none)
        "delete" => {
            let n = text(a, 0);
            if doomed(&n) {
                return Ok(Wert::Wahr(false));
            }
            let Some(i) = o.caches.iter().position(|(k, _)| *k == n) else { return Ok(Wert::Wahr(false)) };
            o.next_doomed += 1;
            let key = format!("{DOOMED}{}", o.next_doomed);
            o.caches[i].0 = key.clone();
            let entry = o.caches.remove(i);
            o.caches.push(entry);
            while o.caches.iter().filter(|(k, _)| doomed(k)).count() > MAX_DOOMED {
                if let Some(j) = o.caches.iter().position(|(k, _)| doomed(k)) {
                    o.caches.remove(j);
                }
            }
            o.bytes = o.caches.iter().flat_map(|(_, v)| v.iter()).map(|e| e.body.len()).sum();
            Ok(Wert::Text(key))
        }
        // put(cache, url, method, requestHeaders, status, statusText, headers, body, responseUrl, responseType)
        "put" => {
            let n = text(a, 0);
            let url = text(a, 1);
            let entry = Entry {
                url: strip(&url, false).to_string(),
                method: text(a, 2),
                request_headers: pairs(a.get(3)),
                status: match a.get(4) { Some(Wert::Zahl(x)) => *x, _ => 200.0 },
                status_text: text(a, 5),
                headers: pairs(a.get(6)),
                body: Arc::new(body_of(match a.get(7) { Some(Wert::Zahl(x)) => *x, _ => 0.0 })),
                response_url: text(a, 8),
                response_type: text(a, 9),
            };
            if o.bytes.saturating_add(entry.body.len()) > ORIGIN_QUOTA {
                return Err("QuotaExceededError: the origin's cache quota is used up".to_string());
            }
            let added = entry.body.len();
            let Some((_, list)) = o.caches.iter_mut().find(|(k, _)| *k == n) else {
                return Err("NotFoundError: the cache is gone".to_string());
            };
            // The new pair replaces what the request matches (§5.4.5 step 3).
            let rh = entry.request_headers.clone();
            let mut freed = 0;
            list.retain(|e| {
                let gone = matches(e, &entry.url, "GET", &rh, false, false, false);
                if gone {
                    freed += e.body.len();
                }
                !gone
            });
            list.push(entry);
            o.bytes = o.bytes.saturating_sub(freed).saturating_add(added);
            Ok(Wert::Undefiniert)
        }
        // match(cache|null, url, method, requestHeaders, ignoreSearch, ignoreMethod, ignoreVary, all)
        "match" => {
            let n = match a.first() { Some(Wert::Text(t)) => Some(t.clone()), _ => None };
            let url = text(a, 1);
            let method = text(a, 2);
            let rh = pairs(a.get(3));
            let (is, im, iv, all) = (flag(a, 4), flag(a, 5), flag(a, 6), flag(a, 7));
            let mut out = Vec::new();
            for (k, list) in &o.caches {
                if n.as_ref().is_some_and(|x| x != k) || (n.is_none() && doomed(k)) {
                    continue;
                }
                for e in list {
                    if url.is_empty() || matches(e, &url, &method, &rh, is, im, iv) {
                        out.push(response_wert(e));
                        if !all {
                            return Ok(Wert::Feld(out));
                        }
                    }
                }
            }
            Ok(Wert::Feld(out))
        }
        // remove(cache, url, method, requestHeaders, ignoreSearch, ignoreMethod, ignoreVary) -> removed?
        "remove" => {
            let n = text(a, 0);
            let url = text(a, 1);
            let method = text(a, 2);
            let rh = pairs(a.get(3));
            let (is, im, iv) = (flag(a, 4), flag(a, 5), flag(a, 6));
            let Some((_, list)) = o.caches.iter_mut().find(|(k, _)| *k == n) else { return Ok(Wert::Wahr(false)) };
            let before = list.len();
            list.retain(|e| !matches(e, &url, &method, &rh, is, im, iv));
            let removed = list.len() != before;
            o.bytes = o.caches.iter().flat_map(|(_, v)| v.iter()).map(|e| e.body.len()).sum();
            Ok(Wert::Wahr(removed))
        }
        // requests(cache, url|"", method, requestHeaders, ignoreSearch, ignoreMethod, ignoreVary)
        "requests" => {
            let n = text(a, 0);
            let url = text(a, 1);
            let method = text(a, 2);
            let rh = pairs(a.get(3));
            let (is, im, iv) = (flag(a, 4), flag(a, 5), flag(a, 6));
            let Some((_, list)) = o.caches.iter().find(|(k, _)| *k == n) else { return Ok(Wert::Feld(Vec::new())) };
            Ok(Wert::Feld(
                list.iter()
                    .filter(|e| url.is_empty() || matches(e, &url, &method, &rh, is, im, iv))
                    .map(|e| Wert::Feld(vec![Wert::Text(e.url.clone()), Wert::Text(e.method.clone()), pairs_wert(&e.request_headers)]))
                    .collect(),
            ))
        }
        _ => Err("NotSupportedError: unknown cache operation".to_string()),
    }
}

/// Binds `__narsilCache` for a script context of `origin`.
pub fn bind(engine: &mut Engine, origin: String) {
    engine.host_funktion("__narsilCache", move |args: &[Wert]| op(&origin, args));
}

/// The script side (installed on first use by `lazy_apis.js`).
pub const CACHE_STORAGE_JS: &str = include_str!("cache_storage.js");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_match_vary_and_delete() {
        let o = "http://cache.test";
        let t = |s: &str| Wert::Text(s.to_string());
        op(o, &[t("open"), t("c")]).unwrap();
        let body = structured::put(SerializedMessage::string("hello"));
        op(o, &[t("put"), t("c"), t("http://cache.test/a?x=1#f"), t("GET"), Wert::Feld(vec![]), Wert::Zahl(200.0), t("OK"),
            Wert::Feld(vec![Wert::Feld(vec![t("Vary"), t("Accept")])]), Wert::Zahl(f64::from(body)), t("http://cache.test/a?x=1"), t("basic")]).unwrap();
        let hit = |url: &str, is: bool| match op(o, &[t("match"), t("c"), t(url), t("GET"), Wert::Feld(vec![]), Wert::Wahr(is), Wert::Wahr(false), Wert::Wahr(false), Wert::Wahr(false)]).unwrap() {
            Wert::Feld(v) => v.len(),
            _ => 99,
        };
        assert_eq!(hit("http://cache.test/a?x=1", false), 1);
        assert_eq!(hit("http://cache.test/a", false), 0);
        assert_eq!(hit("http://cache.test/a", true), 1);
        assert!(matches!(op(o, &[t("delete"), t("c")]).unwrap(), Wert::Text(_)));
        assert_eq!(op(o, &[t("has"), t("c")]).unwrap(), Wert::Wahr(false));
    }

    #[test]
    fn a_deleted_cache_lives_on_under_its_key() {
        let o = "http://doomed.test";
        let t = |s: &str| Wert::Text(s.to_string());
        op(o, &[t("open"), t("d")]).unwrap();
        let key = match op(o, &[t("delete"), t("d")]).unwrap() {
            Wert::Text(k) => k,
            w => panic!("{w:?}"),
        };
        assert_eq!(op(o, &[t("has"), t("d")]).unwrap(), Wert::Wahr(false));
        assert_eq!(op(o, &[t("keys")]).unwrap(), Wert::Feld(vec![]));
        let body = structured::put(SerializedMessage::string("x"));
        op(o, &[t("put"), Wert::Text(key.clone()), t("http://doomed.test/a"), t("GET"), Wert::Feld(vec![]), Wert::Zahl(200.0), t(""),
            Wert::Feld(vec![]), Wert::Zahl(f64::from(body)), t("http://doomed.test/a"), t("basic")]).unwrap();
        let hits = |n: Wert| match op(o, &[t("match"), n, t("http://doomed.test/a"), t("GET"), Wert::Feld(vec![]), Wert::Wahr(false), Wert::Wahr(false), Wert::Wahr(false), Wert::Wahr(true)]).unwrap() {
            Wert::Feld(v) => v.len(),
            _ => 99,
        };
        assert_eq!(hits(Wert::Text(key)), 1);
        assert_eq!(hits(Wert::Null), 0);
    }

    #[test]
    fn caches_survive_encode_and_decode() {
        let mut o = OriginCaches::default();
        let e = Entry {
            url: "https://x.test/a".into(),
            method: "GET".into(),
            request_headers: vec![("accept".into(), "*/*".into())],
            status: 200.0,
            status_text: "OK".into(),
            headers: vec![("content-type".into(), "text/plain".into())],
            body: Arc::new(b"body".to_vec()),
            response_url: "https://x.test/a".into(),
            response_type: "basic".into(),
        };
        o.bytes = 4;
        o.caches.push(("v1".into(), vec![e]));
        o.caches.push(("empty".into(), Vec::new()));
        let back = decode(&encode(&o)).expect("decodes");
        assert_eq!(back.caches.len(), 2);
        assert_eq!(back.caches[0].1[0].body.as_slice(), b"body");
        assert_eq!(back.caches[0].1[0].headers, o.caches[0].1[0].headers);
        assert_eq!(back.bytes, 4);
        let mut bad = encode(&o);
        bad.truncate(bad.len() - 3);
        assert!(decode(&bad).is_none());
        assert!(decode(b"NIDB1\n").is_none());
    }
}
