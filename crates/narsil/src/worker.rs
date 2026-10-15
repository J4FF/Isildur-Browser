//! Web workers (HTML §10), MessageChannel/MessagePort (HTML §9.4),
//! BroadcastChannel (HTML §9.5) and SharedWorker — the host side.
//!
//! # Design
//!
//! Every script context that takes part in messaging (a page with its
//! frames, a dedicated worker, a shared worker) is a *context* with a
//! [`Mailbox`]: a bounded task queue with a condition variable. A small
//! process-wide [`Hub`] knows the contexts, the entangled port pairs, the
//! workers and the broadcast channels, and routes structured-serialized
//! messages ([`narsil_jsvm::structured`]) between them.
//!
//! * A dedicated or shared worker runs in its own OS thread with its own
//!   [`Engine`]. The thread sleeps on its mailbox while there is nothing to
//!   do (no busy waiting) and wakes for a task or the next timer.
//! * The page drains its mailbox from the session tick
//!   (`Sitzung::timer_abarbeiten`), one task per call of the script pump
//!   `__narsilWPump`, so every task gets its own microtask checkpoint.
//! * A page that never touches these APIs never creates a context: the
//!   host functions only register a closure, and the tick sees an empty slot.
//!
//! Lock order: the hub lock may be held while a mailbox lock is taken,
//! never the other way round.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use narsil_jsvm::structured::{self, Ser, SerializedMessage};
use narsil_jsvm::{Engine, Grenzen, JsFehler, Wert};

/// The id of a messaging context.
pub type ContextId = u64;

/// Hard cap of queued tasks per mailbox.
const MAX_TASKS: usize = 100_000;
/// Hard cap of queued message bytes per mailbox.
const MAX_QUEUED_BYTES: usize = 512 * 1024 * 1024;
/// Hard cap of live workers per process (a page that spawns workers in a
/// loop must not exhaust the threads of the machine).
const MAX_WORKERS: usize = 256;
/// Hard cap of live ports per process.
const MAX_PORTS: usize = 200_000;
/// Heap limit of one worker engine.
const WORKER_HEAP_BYTES: usize = 256 * 1024 * 1024;
/// Stack of a worker thread. The engine itself runs every call on its own
/// big-stack thread (`auf_dickem_stapel`); this thread only drives it.
const WORKER_THREAD_STACK: usize = 4 * 1024 * 1024;

const WORKER_SCOPE_JS: &str = include_str!("worker_scope.js");
/// Installs messaging and IndexedDB on first use (`lazy_apis.js`).
pub const LAZY_APIS_JS: &str = include_str!("lazy_apis.js");
/// Bytes all blob: URLs together may hold in the shared registry.
const MAX_BLOB_URL_BYTES: usize = 256 * 1024 * 1024;
/// A single Blob larger than this stays local to its context.
const MAX_SHARED_BLOB: usize = 64 * 1024 * 1024;

/// blob: URLs, shared by the contexts of the process (a worker fetches or
/// starts a blob: URL its page made, and the other way round). An entry
/// belongs to the context that made it and goes with it (File API §8.5:
/// blob URLs are revoked when their document goes).
struct BlobUrl {
    owner: ContextId,
    blob: SerializedMessage,
}

#[derive(Default)]
struct BlobUrls {
    map: HashMap<String, BlobUrl>,
    bytes: usize,
}

fn blob_urls() -> std::sync::MutexGuard<'static, BlobUrls> {
    static B: OnceLock<Mutex<BlobUrls>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(BlobUrls::default())).lock().unwrap_or_else(|e| e.into_inner())
}

/// The origin part of a blob: URL (`blob:http://host:1/uuid` -> `http://host:1`).
fn blob_url_origin(url: &str) -> String {
    let rest = url.get(5..).unwrap_or("");
    match rest.rfind('/') {
        Some(i) => rest[..i].to_string(),
        None => String::new(),
    }
}

/// The type and bytes behind a blob: URL (module imports, `js.rs`).
pub fn blob_url_with_type(url: &str) -> Option<(String, Vec<u8>)> {
    let key = url.split('#').next().unwrap_or(url);
    match &blob_urls().map.get(key)?.blob.value {
        Ser::Blob { bytes, mime } | Ser::File { bytes, mime, .. } => Some((mime.clone(), bytes.as_ref().clone())),
        _ => None,
    }
}

/// The bytes behind a blob: URL, if some context registered it.
fn blob_url_bytes(url: &str) -> Option<Vec<u8>> {
    let key = url.split('#').next().unwrap_or(url);
    match &blob_urls().map.get(key)?.blob.value {
        Ser::Blob { bytes, .. } | Ser::File { bytes, .. } => Some(bytes.as_ref().clone()),
        _ => None,
    }
}

/// The Streams Standard of the page preamble (`vorspann.js`: its block
/// "WHATWG Streams" up to the end of that function), shared with workers —
/// it uses no DOM: ReadableStream & Co., the body mixin of Response/Request
/// (arrayBuffer, blob, bytes, formData, clone, body) and Blob.stream().
/// Workers load it on first use (`lazy_apis.js`). Empty if the block moved.
fn streams_js() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        let v = crate::js::VORSPANN_JS;
        let Some(a) = v.find("// -- WHATWG Streams (ReadableStream, WritableStream, TransformStream) --") else {
            return String::new();
        };
        let rest = &v[a..];
        match rest.find("\n})();") {
            Some(e) => rest[..e + "\n})();".len()].to_string(),
            None => String::new(),
        }
    })
}

/// The script side of all of this (MessagePort, Worker, BroadcastChannel,
/// SharedWorker, the pump); runs in pages and in workers.
pub const MESSAGING_JS: &str = include_str!("messaging.js");

/// One task for a context.
#[derive(Debug)]
pub enum Task {
    /// A message from the dedicated worker `worker` for its Worker object.
    FromWorker { worker: u64, msg: SerializedMessage },
    /// An uncaught exception in the worker, not handled there.
    WorkerError { worker: u64, message: String, filename: String, lineno: u32, colno: u32 },
    /// The worker script could not be fetched or parsed.
    WorkerFailed { worker: u64 },
    /// A message for the worker global (`self.onmessage`).
    ToGlobal { msg: SerializedMessage },
    /// A message for a MessagePort owned by this context.
    Port { port: u64, msg: SerializedMessage },
    /// A BroadcastChannel message for the channel object `channel`.
    Broadcast { channel: u64, origin: String, msg: SerializedMessage },
    /// A new connection to this shared worker (the inside port).
    Connect { port: u64 },
    /// The shared worker behind the SharedWorker object `shared` failed.
    SharedFailed { shared: u64 },
}

impl Task {
    fn bytes(&self) -> usize {
        match self {
            Task::FromWorker { msg, .. } | Task::ToGlobal { msg } | Task::Port { msg, .. } | Task::Broadcast { msg, .. } => {
                msg.bytes.saturating_add(64)
            }
            _ => 64,
        }
    }
}

#[derive(Default)]
struct MailboxState {
    tasks: VecDeque<Task>,
    bytes: usize,
    closed: bool,
}

/// The task queue of one context.
#[derive(Default)]
pub struct Mailbox {
    state: Mutex<MailboxState>,
    wake: Condvar,
}

impl Mailbox {
    fn lock(&self) -> std::sync::MutexGuard<'_, MailboxState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Queues a task; `false` if the mailbox is closed or full.
    fn push(&self, task: Task) -> bool {
        let mut s = self.lock();
        let b = task.bytes();
        if s.closed || s.tasks.len() >= MAX_TASKS || s.bytes.saturating_add(b) > MAX_QUEUED_BYTES {
            return false;
        }
        s.bytes += b;
        s.tasks.push_back(task);
        drop(s);
        self.wake.notify_all();
        true
    }

    fn pop(&self) -> Option<Task> {
        let mut s = self.lock();
        let t = s.tasks.pop_front()?;
        s.bytes = s.bytes.saturating_sub(t.bytes());
        Some(t)
    }

    /// Is a task waiting?
    pub fn pending(&self) -> bool {
        !self.lock().tasks.is_empty()
    }

    fn close(&self) -> Vec<Task> {
        let mut s = self.lock();
        s.closed = true;
        s.bytes = 0;
        let rest: Vec<Task> = s.tasks.drain(..).collect();
        drop(s);
        self.wake.notify_all();
        rest
    }

    /// Wakes a sleeping worker thread (terminate, close).
    fn poke(&self) {
        self.wake.notify_all();
    }

    /// Sleeps until a task arrives, the mailbox closes or `timeout` passes.
    fn wait(&self, timeout: Duration) {
        let s = self.lock();
        if !s.tasks.is_empty() || s.closed {
            return;
        }
        let _ = self.wake.wait_timeout(s, timeout);
    }

    /// Removes the queued tasks for which `pick` holds (keeps the order).
    fn extract(&self, mut pick: impl FnMut(&Task) -> bool) -> Vec<Task> {
        let mut s = self.lock();
        let mut out = Vec::new();
        let mut keep = VecDeque::with_capacity(s.tasks.len());
        while let Some(t) = s.tasks.pop_front() {
            if pick(&t) {
                out.push(t);
            } else {
                keep.push_back(t);
            }
        }
        s.tasks = keep;
        s.bytes = s.tasks.iter().map(Task::bytes).sum();
        out
    }
}

struct Ctx {
    mailbox: Arc<Mailbox>,
}

struct Port {
    owner: Option<ContextId>,
    peer: Option<u64>,
    enabled: bool,
    held: VecDeque<SerializedMessage>,
}

struct WorkerRec {
    parent: ContextId,
    /// The worker's own context.
    ctx: ContextId,
    mailbox: Arc<Mailbox>,
    interrupt: Arc<AtomicBool>,
}

struct Channel {
    ctx: ContextId,
    /// The origin as the receiver sees it (`MessageEvent.origin`).
    origin: String,
    /// The partition: the origin, or the context for an opaque origin
    /// (every opaque origin is its own).
    key: String,
    name: String,
}

struct SharedRec {
    origin: String,
    url: String,
    name: String,
    ctx: ContextId,
    mailbox: Arc<Mailbox>,
    interrupt: Arc<AtomicBool>,
    /// The SharedWorker objects connected to it: (context, object id).
    clients: Vec<(ContextId, u64)>,
    failed: bool,
    /// Type, credentials mode and extendedLifetime it was started with.
    options: String,
}

/// The process-wide routing state.
#[derive(Default)]
struct Hub {
    next: u64,
    contexts: HashMap<ContextId, Ctx>,
    ports: HashMap<u64, Port>,
    workers: HashMap<u64, WorkerRec>,
    channels: HashMap<u64, Channel>,
    shared: HashMap<u64, SharedRec>,
}

fn hub() -> std::sync::MutexGuard<'static, Hub> {
    static H: OnceLock<Mutex<Hub>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(Hub::default())).lock().unwrap_or_else(|e| e.into_inner())
}

impl Hub {
    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    fn mailbox(&self, ctx: ContextId) -> Option<Arc<Mailbox>> {
        self.contexts.get(&ctx).map(|c| c.mailbox.clone())
    }

    /// Delivers a message to a port: into its owner's mailbox when the
    /// port is enabled, else into the port's own queue.
    fn deliver(&mut self, port: u64, msg: SerializedMessage) {
        let Some(p) = self.ports.get_mut(&port) else { return };
        if p.enabled
            && let Some(owner) = p.owner
            && let Some(mb) = self.contexts.get(&owner).map(|c| c.mailbox.clone())
        {
            mb.push(Task::Port { port, msg });
            return;
        }
        if p.held.len() < MAX_TASKS {
            p.held.push_back(msg);
        }
    }

    /// The ports `ids` leave context `ctx` (transfer): they lose their
    /// owner, and messages already queued for them travel with them.
    fn detach_ports(&mut self, ctx: ContextId, ids: &[u64]) -> Result<(), String> {
        for id in ids {
            match self.ports.get(id) {
                Some(p) if p.owner == Some(ctx) => {}
                _ => return Err("DataCloneError: a MessagePort could not be transferred".to_string()),
            }
        }
        let mb = self.mailbox(ctx);
        for id in ids {
            let queued = mb.as_ref().map_or_else(Vec::new, |m| m.extract(|t| matches!(t, Task::Port { port, .. } if port == id)));
            if let Some(p) = self.ports.get_mut(id) {
                p.owner = None;
                p.enabled = false;
                let mut front: VecDeque<SerializedMessage> = queued
                    .into_iter()
                    .filter_map(|t| if let Task::Port { msg, .. } = t { Some(msg) } else { None })
                    .collect();
                front.extend(p.held.drain(..));
                p.held = front;
            }
        }
        Ok(())
    }

    /// The ports of a received message now belong to `ctx` (disabled until
    /// `start()` or `onmessage`).
    fn adopt_ports(&mut self, ctx: ContextId, ids: &[u64]) {
        for id in ids {
            if let Some(p) = self.ports.get_mut(id) {
                p.owner = Some(ctx);
                p.enabled = false;
            }
        }
    }

    fn close_port(&mut self, id: u64) {
        if let Some(p) = self.ports.remove(&id)
            && let Some(peer) = p.peer
            && let Some(q) = self.ports.get_mut(&peer)
        {
            q.peer = None;
        }
    }

    /// Ports referenced by tasks that will never run are closed.
    fn close_ports_of(&mut self, tasks: &[Task]) {
        for t in tasks {
            let ids: &[u64] = match t {
                Task::FromWorker { msg, .. } | Task::ToGlobal { msg } | Task::Port { msg, .. } | Task::Broadcast { msg, .. } => &msg.ports,
                Task::Connect { port } => std::slice::from_ref(port),
                _ => &[],
            };
            for id in ids.to_vec() {
                self.close_port(id);
            }
        }
    }
}

/// Registers a new context.
fn register_context() -> (ContextId, Arc<Mailbox>) {
    let mut h = hub();
    let id = h.id();
    let mailbox = Arc::new(Mailbox::default());
    h.contexts.insert(id, Ctx { mailbox: mailbox.clone() });
    (id, mailbox)
}

/// A context goes away (page closed or navigated, worker ended): its
/// mailbox closes, its workers are terminated, its ports and channels go.
fn drop_context(id: ContextId) {
    let mut h = hub();
    let Some(ctx) = h.contexts.remove(&id) else { return };
    let rest = ctx.mailbox.close();
    h.close_ports_of(&rest);
    let owned: Vec<u64> = h.ports.iter().filter(|(_, p)| p.owner == Some(id)).map(|(k, _)| *k).collect();
    for p in owned {
        h.close_port(p);
    }
    h.channels.retain(|_, c| c.ctx != id);
    {
        let mut b = blob_urls();
        let gone: Vec<String> = b.map.iter().filter(|(_, e)| e.owner == id).map(|(k, _)| k.clone()).collect();
        for k in gone {
            if let Some(e) = b.map.remove(&k) {
                b.bytes = b.bytes.saturating_sub(e.blob.bytes);
            }
        }
    }
    let children: Vec<u64> = h.workers.iter().filter(|(_, w)| w.parent == id).map(|(k, _)| *k).collect();
    for w in children {
        if let Some(rec) = h.workers.remove(&w) {
            rec.interrupt.store(true, Ordering::Relaxed);
            rec.mailbox.poke();
        }
    }
    // The worker's own record goes when its context goes.
    h.workers.retain(|_, w| w.ctx != id);
    let mut ended = Vec::new();
    for (k, s) in h.shared.iter_mut() {
        s.clients.retain(|(c, _)| *c != id);
        if s.ctx == id || s.clients.is_empty() {
            ended.push(*k);
        }
    }
    for k in ended {
        if let Some(s) = h.shared.remove(&k) {
            s.interrupt.store(true, Ordering::Relaxed);
            s.mailbox.poke();
        }
    }
}

/// Owns a context: dropping the last handle ends it.
pub struct ContextGuard {
    pub id: ContextId,
    pub mailbox: Arc<Mailbox>,
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        drop_context(self.id);
    }
}

/// What a worker context knows about itself.
#[derive(Clone)]
struct WorkerLink {
    id: u64,
    parent: ContextId,
    closing: Arc<AtomicBool>,
    shared: bool,
}

/// The (lazily made) context of one script environment, shared by its host
/// functions and its owner (the page's `Kiste`, the worker thread).
pub struct ContextSlot {
    origin: Mutex<String>,
    guard: OnceLock<Arc<ContextGuard>>,
    worker: Option<WorkerLink>,
}

impl Default for ContextSlot {
    fn default() -> Self {
        ContextSlot { origin: Mutex::new("null".to_string()), guard: OnceLock::new(), worker: None }
    }
}

impl std::fmt::Debug for ContextSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextSlot")
            .field("origin", &self.origin())
            .field("context", &self.guard.get().map(|g| g.id))
            .finish()
    }
}

impl ContextSlot {
    /// A page slot; the context is made on first use.
    pub fn page(origin: &str) -> Arc<Self> {
        Arc::new(ContextSlot { origin: Mutex::new(origin.to_string()), guard: OnceLock::new(), worker: None })
    }

    fn origin(&self) -> String {
        self.origin.lock().map(|o| o.clone()).unwrap_or_default()
    }

    /// Updates the origin (the page address is known after binding).
    pub fn set_origin(&self, origin: &str) {
        if let Ok(mut o) = self.origin.lock() {
            *o = origin.to_string();
        }
    }

    fn ctx(&self) -> Arc<ContextGuard> {
        self.guard
            .get_or_init(|| {
                let (id, mailbox) = register_context();
                Arc::new(ContextGuard { id, mailbox })
            })
            .clone()
    }

    /// The context, if one was made.
    pub fn existing(&self) -> Option<Arc<ContextGuard>> {
        self.guard.get().cloned()
    }

    /// Is a task waiting for this context?
    pub fn pending(&self) -> bool {
        self.guard.get().is_some_and(|g| g.mailbox.pending())
    }
}

/// The origin of an address (`scheme://host[:port]`, "null" if opaque).
pub fn origin_of(url: &str) -> String {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return "null".to_string();
    }
    match narsil_jsvm::netz::Herkunft::aus_url(url) {
        Ok(h) => h.als_origin(),
        Err(_) => "null".to_string(),
    }
}

fn num(args: &[Wert], i: usize) -> f64 {
    match args.get(i) {
        Some(Wert::Zahl(n)) => *n,
        _ => 0.0,
    }
}

fn id_arg(args: &[Wert], i: usize) -> u64 {
    let n = num(args, i);
    if n.is_finite() && n > 0.0 { n as u64 } else { 0 }
}

fn text(args: &[Wert], i: usize) -> String {
    match args.get(i) {
        Some(Wert::Text(t)) => t.clone(),
        Some(Wert::Undefiniert) | Some(Wert::Null) | None => String::new(),
        Some(w) => w.als_text(),
    }
}

fn ids(args: &[Wert], i: usize) -> Vec<u64> {
    match args.get(i) {
        Some(Wert::Feld(v)) => v
            .iter()
            .filter_map(|w| if let Wert::Zahl(n) = w { (*n > 0.0).then_some(*n as u64) } else { None })
            .collect(),
        _ => Vec::new(),
    }
}

fn id_list(v: &[u64]) -> Wert {
    Wert::Feld(v.iter().map(|n| Wert::Zahl(*n as f64)).collect())
}

/// Takes the serialized message of a handle, with the transferred ports.
fn message(args: &[Wert], handle_at: usize, ports_at: usize) -> Result<SerializedMessage, String> {
    let handle = num(args, handle_at);
    let mut msg = structured::take(handle as u32).ok_or_else(|| "DataCloneError: unknown message".to_string())?;
    msg.ports = ids(args, ports_at);
    Ok(msg)
}

/// What the script needs to start a worker.
#[derive(Clone)]
struct WorkerStart {
    id: u64,
    parent: ContextId,
    ctx: ContextId,
    mailbox: Arc<Mailbox>,
    interrupt: Arc<AtomicBool>,
    url: String,
    module: bool,
    name: String,
    shared: bool,
    origin: String,
    /// The script body for `blob:` URLs (the page resolves the blob).
    body: Option<Vec<u8>>,
}

/// Registers the host functions every messaging context has.
fn bind_common(engine: &mut Engine, slot: Arc<ContextSlot>) {
    // The source of a lazily installed script (`lazy_apis.js`).
    engine.host_funktion("__narsilLazySource", |args: &[Wert]| {
        Ok(Wert::Text(match text(args, 0).as_str() {
            "indexeddb" => crate::idb::INDEXEDDB_JS.to_string(),
            "caches" => crate::cache_storage::CACHE_STORAGE_JS.to_string(),
            "streams" => streams_js().to_string(),
            _ => MESSAGING_JS.to_string(),
        }))
    });
    // The next task, as an array for the pump (see `messaging.js`).
    engine.host_funktion("__narsilWNext", {
        let slot = slot.clone();
        move |_args: &[Wert]| {
            let Some(g) = slot.existing() else { return Ok(Wert::Null) };
            let Some(task) = g.mailbox.pop() else { return Ok(Wert::Null) };
            let adopt = |ports: &[u64]| hub().adopt_ports(g.id, ports);
            Ok(match task {
                Task::FromWorker { worker, msg } => {
                    adopt(&msg.ports);
                    let ports = id_list(&msg.ports);
                    Wert::Feld(vec![Wert::Text("w".into()), Wert::Zahl(worker as f64), Wert::Zahl(f64::from(structured::put(msg))), ports])
                }
                Task::ToGlobal { msg } => {
                    adopt(&msg.ports);
                    let ports = id_list(&msg.ports);
                    Wert::Feld(vec![Wert::Text("g".into()), Wert::Zahl(0.0), Wert::Zahl(f64::from(structured::put(msg))), ports])
                }
                Task::Port { port, msg } => {
                    adopt(&msg.ports);
                    let ports = id_list(&msg.ports);
                    Wert::Feld(vec![Wert::Text("p".into()), Wert::Zahl(port as f64), Wert::Zahl(f64::from(structured::put(msg))), ports])
                }
                Task::Broadcast { channel, origin, msg } => Wert::Feld(vec![
                    Wert::Text("b".into()),
                    Wert::Zahl(channel as f64),
                    Wert::Zahl(f64::from(structured::put(msg))),
                    Wert::Text(origin),
                ]),
                Task::WorkerError { worker, message, filename, lineno, colno } => Wert::Feld(vec![
                    Wert::Text("we".into()),
                    Wert::Zahl(worker as f64),
                    Wert::Text(message),
                    Wert::Text(filename),
                    Wert::Zahl(f64::from(lineno)),
                    Wert::Zahl(f64::from(colno)),
                ]),
                Task::WorkerFailed { worker } => Wert::Feld(vec![Wert::Text("wf".into()), Wert::Zahl(worker as f64)]),
                Task::Connect { port } => {
                    adopt(&[port]);
                    Wert::Feld(vec![Wert::Text("c".into()), Wert::Zahl(port as f64)])
                }
                Task::SharedFailed { shared } => Wert::Feld(vec![Wert::Text("sf".into()), Wert::Zahl(shared as f64)]),
            })
        }
    });
    // MessageChannel: two entangled ports owned by this context.
    engine.host_funktion("__narsilWPortPair", {
        let slot = slot.clone();
        move |_args: &[Wert]| {
            let g = slot.ctx();
            let mut h = hub();
            if h.ports.len() >= MAX_PORTS {
                return Err("RangeError: too many MessagePorts".to_string());
            }
            let a = h.id();
            let b = h.id();
            h.ports.insert(a, Port { owner: Some(g.id), peer: Some(b), enabled: false, held: VecDeque::new() });
            h.ports.insert(b, Port { owner: Some(g.id), peer: Some(a), enabled: false, held: VecDeque::new() });
            Ok(Wert::Feld(vec![Wert::Zahl(a as f64), Wert::Zahl(b as f64)]))
        }
    });
    // port.postMessage(handle, transferredPorts)
    engine.host_funktion("__narsilWPortPost", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let port = id_arg(args, 0);
            let msg = message(args, 1, 2)?;
            let mut h = hub();
            h.detach_ports(g.id, &msg.ports)?;
            let peer = h.ports.get(&port).filter(|p| p.owner == Some(g.id)).and_then(|p| p.peer);
            match peer {
                Some(peer) => h.deliver(peer, msg),
                // A disentangled port drops the message; its ports are lost.
                None => {
                    for p in msg.ports {
                        h.close_port(p);
                    }
                }
            }
            Ok(Wert::Undefiniert)
        }
    });
    engine.host_funktion("__narsilWPortStart", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let port = id_arg(args, 0);
            let mut h = hub();
            let held: Vec<SerializedMessage> = match h.ports.get_mut(&port) {
                Some(p) if p.owner == Some(g.id) => {
                    p.enabled = true;
                    p.held.drain(..).collect()
                }
                _ => return Ok(Wert::Undefiniert),
            };
            for msg in held {
                g.mailbox.push(Task::Port { port, msg });
            }
            Ok(Wert::Undefiniert)
        }
    });
    // Ports handed on inside this context (window.postMessage): queued
    // messages wait in the port again until the new object starts it.
    engine.host_funktion("__narsilWPortRebind", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let ports = ids(args, 0);
            let mut h = hub();
            h.detach_ports(g.id, &ports)?;
            h.adopt_ports(g.id, &ports);
            Ok(Wert::Undefiniert)
        }
    });
    engine.host_funktion("__narsilWPortClose", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let port = id_arg(args, 0);
            let mut h = hub();
            if h.ports.get(&port).is_some_and(|p| p.owner == Some(g.id)) {
                h.close_port(port);
                g.mailbox.extract(|t| matches!(t, Task::Port { port: p, .. } if *p == port));
            }
            Ok(Wert::Undefiniert)
        }
    });
    // new Worker(url, {type, name}) -> id; the URL is absolute and checked.
    engine.host_funktion("__narsilWWorkerNew", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let url = text(args, 0);
            let module = text(args, 1) == "module";
            let name = text(args, 2);
            let body = blob_body(args, 4);
            let blocked = matches!(args.get(5), Some(Wert::Wahr(true)));
            let origin = worker_origin(&url, &slot.origin());
            if blocked {
                // Not same-origin: the fetch fails, the worker fires `error`.
                let mut h = hub();
                let id = h.id();
                drop(h);
                g.mailbox.push(Task::WorkerFailed { worker: id });
                return Ok(Wert::Zahl(id as f64));
            }
            spawn_worker(g.id, url, module, name, origin, body).map(|id| Wert::Zahl(id as f64))
        }
    });
    engine.host_funktion("__narsilWWorkerPost", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let worker = id_arg(args, 0);
            let msg = message(args, 1, 2)?;
            let mut h = hub();
            h.detach_ports(g.id, &msg.ports)?;
            let target = h.workers.get(&worker).filter(|w| w.parent == g.id).map(|w| w.mailbox.clone());
            match target {
                Some(mb) => {
                    let ports = msg.ports.clone();
                    if !mb.push(Task::ToGlobal { msg }) {
                        for p in ports {
                            h.close_port(p);
                        }
                    }
                }
                None => {
                    for p in msg.ports {
                        h.close_port(p);
                    }
                }
            }
            Ok(Wert::Undefiniert)
        }
    });
    engine.host_funktion("__narsilWWorkerTerminate", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let worker = id_arg(args, 0);
            let mut h = hub();
            let mine = h.workers.get(&worker).is_some_and(|w| w.parent == g.id);
            if mine && let Some(rec) = h.workers.remove(&worker) {
                rec.interrupt.store(true, Ordering::Relaxed);
                let rest = rec.mailbox.close();
                h.close_ports_of(&rest);
            }
            drop(h);
            // Messages of the worker that wait here are dropped (HTML §10.2.6
            // "terminate a worker": the port's message queue is emptied).
            let gone = g.mailbox.extract(|t| match t {
                Task::FromWorker { worker: w, .. } | Task::WorkerError { worker: w, .. } | Task::WorkerFailed { worker: w } => *w == worker,
                _ => false,
            });
            hub().close_ports_of(&gone);
            Ok(Wert::Undefiniert)
        }
    });
    // BroadcastChannel
    engine.host_funktion("__narsilWBcOpen", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let name = text(args, 0);
            let mut h = hub();
            let id = h.id();
            let origin = slot.origin();
            let key = if origin == "null" { format!("null:{}", g.id) } else { origin.clone() };
            h.channels.insert(id, Channel { ctx: g.id, origin, key, name });
            Ok(Wert::Zahl(id as f64))
        }
    });
    engine.host_funktion("__narsilWBcPost", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let channel = id_arg(args, 0);
            let msg = message(args, 1, 2)?;
            let h = hub();
            let Some(me) = h.channels.get(&channel).filter(|c| c.ctx == g.id) else {
                return Ok(Wert::Undefiniert);
            };
            let mut targets: Vec<(u64, ContextId)> = h
                .channels
                .iter()
                .filter(|(k, c)| **k != channel && c.key == me.key && c.name == me.name)
                .map(|(k, c)| (*k, c.ctx))
                .collect();
            // Creation order (HTML §9.5 "sort destinations").
            targets.sort_by_key(|(k, _)| *k);
            let origin = me.origin.clone();
            for (k, c) in targets {
                if let Some(mb) = h.mailbox(c) {
                    mb.push(Task::Broadcast { channel: k, origin: origin.clone(), msg: msg.clone() });
                }
            }
            Ok(Wert::Undefiniert)
        }
    });
    engine.host_funktion("__narsilWBcClose", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let channel = id_arg(args, 0);
            let mut h = hub();
            if h.channels.get(&channel).is_some_and(|c| c.ctx == g.id) {
                h.channels.remove(&channel);
                g.mailbox.extract(|t| matches!(t, Task::Broadcast { channel: c, .. } if *c == channel));
            }
            Ok(Wert::Undefiniert)
        }
    });
    // blob: URLs into the shared registry (`URL.createObjectURL`).
    engine.host_funktion("__narsilWBlobPut", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let url = text(args, 0);
            let Some(msg) = structured::take(num(args, 1) as u32) else { return Ok(Wert::Wahr(false)) };
            if !matches!(msg.value, Ser::Blob { .. } | Ser::File { .. }) || msg.bytes > MAX_SHARED_BLOB {
                return Ok(Wert::Wahr(false));
            }
            let g = slot.ctx();
            let mut b = blob_urls();
            if b.bytes.saturating_add(msg.bytes) > MAX_BLOB_URL_BYTES {
                return Ok(Wert::Wahr(false));
            }
            b.bytes += msg.bytes;
            if let Some(old) = b.map.insert(url, BlobUrl { owner: g.id, blob: msg }) {
                b.bytes = b.bytes.saturating_sub(old.blob.bytes);
            }
            Ok(Wert::Wahr(true))
        }
    });
    engine.host_funktion("__narsilWBlobDrop", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let url = text(args, 0);
            let origin = slot.origin();
            let mut b = blob_urls();
            if blob_url_origin(&url) == origin
                && let Some(e) = b.map.remove(&url)
            {
                b.bytes = b.bytes.saturating_sub(e.blob.bytes);
            }
            Ok(Wert::Undefiniert)
        }
    });
    // The Blob of a blob: URL of this origin (a handle), or 0.
    engine.host_funktion("__narsilWBlobGet", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let url = text(args, 0);
            let key = url.split('#').next().unwrap_or(&url).to_string();
            if blob_url_origin(&key) != slot.origin() {
                return Ok(Wert::Zahl(0.0));
            }
            let msg = blob_urls().map.get(&key).map(|e| e.blob.clone());
            Ok(Wert::Zahl(msg.map_or(0.0, |m| f64::from(structured::put(m)))))
        }
    });
    // new SharedWorker(url, {name, type}) -> [object id, outside port]
    engine.host_funktion("__narsilWSharedNew", {
        let slot = slot.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let url = text(args, 0);
            let name = text(args, 1);
            let module = text(args, 2) == "module";
            let body = blob_body(args, 3);
            let blocked = matches!(args.get(4), Some(Wert::Wahr(true)));
            let options = text(args, 5);
            let origin = slot.origin();
            let mismatch = {
                let h = hub();
                h.shared.values().any(|s| !s.failed && s.origin == origin && s.url == url && s.name == name && origin != "null" && s.options != options)
            };
            if blocked || mismatch {
                let mut h = hub();
                let obj = h.id();
                let port = h.id();
                h.ports.insert(port, Port { owner: Some(g.id), peer: None, enabled: false, held: VecDeque::new() });
                drop(h);
                g.mailbox.push(Task::SharedFailed { shared: obj });
                return Ok(Wert::Feld(vec![Wert::Zahl(obj as f64), Wert::Zahl(port as f64)]));
            }
            shared_connect(g.id, &origin, url, name, module, body, options)
                .map(|(obj, port)| Wert::Feld(vec![Wert::Zahl(obj as f64), Wert::Zahl(port as f64)]))
        }
    });
}

/// The bytes of a serialized Blob handle (for `blob:` worker URLs).
fn blob_body(args: &[Wert], i: usize) -> Option<Vec<u8>> {
    let h = num(args, i);
    if h < 1.0 {
        return None;
    }
    match structured::take(h as u32)?.value {
        Ser::Blob { bytes, .. } | Ser::File { bytes, .. } => Some(bytes.as_ref().clone()),
        _ => None,
    }
}

/// The bytes of a request body given as a serialized handle (string as
/// UTF-8, ArrayBuffer, view, Blob).
fn body_bytes(handle: f64) -> Option<Vec<u8>> {
    if handle < 1.0 {
        return None;
    }
    let msg = structured::take(handle as u32)?;
    let mut buffers = msg.buffers;
    fn bytes_of(s: Ser, buffers: &mut [Vec<u8>]) -> Option<Vec<u8>> {
        match s {
            Ser::String(t) => Some(t.into_bytes()),
            Ser::ArrayBuffer(b) => Some(b),
            Ser::TransferredBuffer(i) => buffers.get_mut(i as usize).map(std::mem::take),
            Ser::View { kind, buffer, offset, length } => {
                let all = bytes_of(*buffer, buffers)?;
                let per = match kind.as_str() {
                    "Uint16Array" | "Int16Array" => 2,
                    "Uint32Array" | "Int32Array" | "Float32Array" => 4,
                    "Float64Array" | "BigInt64Array" | "BigUint64Array" => 8,
                    _ => 1,
                };
                let end = offset.saturating_add(length.saturating_mul(per)).min(all.len());
                Some(all.get(offset.min(end)..end).unwrap_or_default().to_vec())
            }
            Ser::Blob { bytes, .. } | Ser::File { bytes, .. } => Some(bytes.as_ref().clone()),
            _ => None,
        }
    }
    bytes_of(msg.value, &mut buffers)
}

/// A worker from a `data:` URL has an opaque origin; others inherit the
/// creator's origin (same-origin was checked by the script).
fn worker_origin(url: &str, parent_origin: &str) -> String {
    if url.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("data:")) {
        "null".to_string()
    } else {
        parent_origin.to_string()
    }
}

/// Makes the context and the records of a new worker (under the hub lock,
/// before its thread runs, so nothing it reports can get lost).
fn prepare_worker(
    h: &mut Hub,
    parent: ContextId,
    url: String,
    module: bool,
    name: String,
    shared: bool,
    origin: String,
    body: Option<Vec<u8>>,
) -> Result<WorkerStart, String> {
    let live = h.workers.len() + h.shared.len();
    if live >= MAX_WORKERS {
        return Err("RangeError: too many workers".to_string());
    }
    let id = h.id();
    let ctx = h.id();
    let mailbox = Arc::new(Mailbox::default());
    h.contexts.insert(ctx, Ctx { mailbox: mailbox.clone() });
    let interrupt = Arc::new(AtomicBool::new(false));
    if !shared {
        h.workers.insert(id, WorkerRec { parent, ctx, mailbox: mailbox.clone(), interrupt: interrupt.clone() });
    }
    Ok(WorkerStart { id, parent, ctx, mailbox, interrupt, url, module, name, shared, origin, body })
}

/// Starts the thread of a prepared worker.
fn launch(start: WorkerStart) -> Result<(), String> {
    let (id, ctx) = (start.id, start.ctx);
    let spawned = std::thread::Builder::new()
        .name("narsil-worker".to_string())
        .stack_size(WORKER_THREAD_STACK)
        .spawn(move || worker_main(start));
    if let Err(e) = spawned {
        drop_context(ctx);
        let mut h = hub();
        h.workers.remove(&id);
        h.shared.remove(&id);
        return Err(format!("Error: worker thread could not start: {e}"));
    }
    Ok(())
}

fn spawn_worker(
    parent: ContextId,
    url: String,
    module: bool,
    name: String,
    origin: String,
    body: Option<Vec<u8>>,
) -> Result<u64, String> {
    let start = {
        let mut h = hub();
        prepare_worker(&mut h, parent, url, module, name, false, origin, body)?
    };
    let id = start.id;
    launch(start)?;
    Ok(id)
}

fn shared_connect(
    client: ContextId,
    origin: &str,
    url: String,
    name: String,
    module: bool,
    body: Option<Vec<u8>>,
    options: String,
) -> Result<(u64, u64), String> {
    let mut h = hub();
    let existing = h
        .shared
        .iter()
        .find(|(_, s)| !s.failed && s.origin == origin && s.url == url && s.name == name && origin != "null")
        .map(|(k, _)| *k);
    let mut launch_it = None;
    let shared_id = match existing {
        Some(k) => k,
        None => {
            let worker_origin = worker_origin(&url, origin);
            let start = prepare_worker(&mut h, client, url.clone(), module, name.clone(), true, worker_origin, body)?;
            let id = start.id;
            h.shared.insert(
                id,
                SharedRec {
                    origin: origin.to_string(),
                    url,
                    name,
                    ctx: start.ctx,
                    mailbox: start.mailbox.clone(),
                    interrupt: start.interrupt.clone(),
                    clients: Vec::new(),
                    failed: false,
                    options,
                },
            );
            launch_it = Some(start);
            id
        }
    };
    let obj = h.id();
    let outside = h.id();
    let inside = h.id();
    h.ports.insert(outside, Port { owner: Some(client), peer: Some(inside), enabled: false, held: VecDeque::new() });
    h.ports.insert(inside, Port { owner: None, peer: Some(outside), enabled: false, held: VecDeque::new() });
    let mailbox = match h.shared.get_mut(&shared_id) {
        Some(s) => {
            s.clients.push((client, obj));
            s.mailbox.clone()
        }
        None => return Err("Error: shared worker vanished".to_string()),
    };
    mailbox.push(Task::Connect { port: inside });
    drop(h);
    if let Some(start) = launch_it {
        launch(start)?;
    }
    Ok((obj, outside))
}

/// Sends a task to the parent of a dedicated worker (if it still exists).
fn to_parent(link: &WorkerLink, task: Task) {
    let h = hub();
    if link.shared {
        // A shared worker has no parent object: failures go to every
        // connected SharedWorker object.
        if let Task::WorkerFailed { .. } = task
            && let Some(s) = h.shared.get(&link.id)
        {
            for (c, obj) in s.clients.clone() {
                if let Some(mb) = h.mailbox(c) {
                    mb.push(Task::SharedFailed { shared: obj });
                }
            }
        }
        return;
    }
    if !h.workers.contains_key(&link.id) {
        return;
    }
    if let Some(mb) = h.mailbox(link.parent) {
        mb.push(task);
    }
}

/// Registers the host functions only a worker has.
fn bind_worker(engine: &mut Engine, slot: Arc<ContextSlot>, start: &WorkerStart, final_url: String) {
    let link = slot.worker.clone().expect("worker slot");
    let sprache = narsil_chrome::ktx_menue::sprache_aus_umgebung();
    let kerne = std::thread::available_parallelism().map_or(4.0, |n| n.get() as f64);
    // The same values as the page's navigator (`vorspann.js`, Firefox 128).
    let ua = narsil_net::STANDARD_USER_AGENT;
    let (platform, app_version) = if ua.contains("Windows") {
        ("Win32", "5.0 (Windows)")
    } else if ua.contains("Macintosh") {
        ("MacIntel", "5.0 (Macintosh)")
    } else {
        ("Linux x86_64", "5.0 (X11)")
    };
    let nav = Wert::Gegenstand(vec![
        ("userAgent".to_string(), Wert::Text(ua.to_string())),
        ("appVersion".to_string(), Wert::Text(app_version.to_string())),
        ("language".to_string(), Wert::Text(sprache)),
        ("platform".to_string(), Wert::Text(platform.to_string())),
        ("hardwareConcurrency".to_string(), Wert::Zahl(kerne)),
    ]);
    engine.host_funktion("__narsilWSelf", {
        let url = final_url.clone();
        let name = start.name.clone();
        let shared = start.shared;
        let origin = start.origin.clone();
        let module = start.module;
        move |_args: &[Wert]| {
            Ok(Wert::Feld(vec![
                Wert::Text(url.clone()),
                Wert::Text(name.clone()),
                Wert::Wahr(shared),
                Wert::Text(origin.clone()),
                nav.clone(),
                Wert::Wahr(module),
            ]))
        }
    });
    engine.host_funktion("__narsilWSelfPost", {
        let slot = slot.clone();
        let link = link.clone();
        move |args: &[Wert]| {
            let g = slot.ctx();
            let msg = message(args, 0, 1)?;
            let mut h = hub();
            h.detach_ports(g.id, &msg.ports)?;
            let parent = h.workers.get(&link.id).map(|w| w.parent).and_then(|p| h.mailbox(p));
            match parent {
                Some(mb) => {
                    let ports = msg.ports.clone();
                    if !mb.push(Task::FromWorker { worker: link.id, msg }) {
                        for p in ports {
                            h.close_port(p);
                        }
                    }
                }
                None => {
                    for p in msg.ports {
                        h.close_port(p);
                    }
                }
            }
            Ok(Wert::Undefiniert)
        }
    });
    engine.host_funktion("__narsilWClose", {
        let link = link.clone();
        let mailbox = start.mailbox.clone();
        move |_args: &[Wert]| {
            link.closing.store(true, Ordering::Relaxed);
            mailbox.poke();
            Ok(Wert::Undefiniert)
        }
    });
    // importScripts: the body of one script, fetched synchronously.
    // FileReaderSync (`worker_scope.js`): the bytes of a serialized Blob.
    engine.host_funktion("__narsilWBlobBytes", |args: &[Wert]| {
        match structured::take(num(args, 0) as u32).map(|m| m.value) {
            Some(Ser::Blob { bytes, .. }) | Some(Ser::File { bytes, .. }) => Ok(Wert::ByteFeld { daten: bytes.as_ref().clone(), geklemmt: false }),
            _ => Ok(Wert::ByteFeld { daten: Vec::new(), geklemmt: false }),
        }
    });
    engine.host_funktion("__narsilWImport", move |args: &[Wert]| {
        let url = text(args, 0);
        match fetch_script(&url, None, true) {
            Ok((body, end)) => Ok(Wert::Feld(vec![Wert::Text(body), Wert::Text(end)])),
            Err(_) => Err(format!("NetworkError: Failed to execute 'importScripts' on 'WorkerGlobalScope': The script at '{url}' failed to load.")),
        }
    });
    // XMLHttpRequest in a worker: one request, blocking this worker thread
    // (synchronous XHR is allowed in workers, XHR §4.5.6).
    // Arguments: method, absolute URL, [[name, value]...], body handle (0 =
    // none). Result: [status, [[name, value]...], body bytes].
    engine.host_funktion("__narsilWXhr", move |args: &[Wert]| {
        let methode = text(args, 0).to_ascii_uppercase();
        let url = text(args, 1);
        let mut kopf = Vec::new();
        if let Some(Wert::Feld(paare)) = args.get(2) {
            for p in paare.iter().take(256) {
                if let Wert::Feld(nv) = p
                    && let (Some(n), Some(v)) = (nv.first(), nv.get(1))
                {
                    kopf.push((n.als_text(), v.als_text()));
                }
            }
        }
        let koerper = body_bytes(num(args, 3));
        if url.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("data:")) {
            let (mime, bytes) = narsil_jsvm::netz::daten_url_lesen(&url).ok_or_else(|| "NetworkError: bad data: URL".to_string())?;
            return Ok(Wert::Feld(vec![
                Wert::Zahl(200.0),
                Wert::Feld(vec![Wert::Feld(vec![Wert::Text("content-type".into()), Wert::Text(mime)])]),
                Wert::ByteFeld { daten: bytes, geklemmt: false },
            ]));
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("NetworkError: unsupported scheme".to_string());
        }
        let anfrage = narsil_jsvm::netz::NetzAnfrage { id: 0, url, methode, kopf, koerper };
        let mut grenzen = Grenzen::standard();
        grenzen.netz_antwort_bytes = 64 * 1024 * 1024;
        let mut netz = narsil_jsvm::netz::NarsilNet { https_erlaubt: true };
        match netz.hole_mit(&anfrage, grenzen) {
            Ok(a) => Ok(Wert::Feld(vec![
                Wert::Zahl(f64::from(a.status)),
                Wert::Feld(a.kopf.into_iter().map(|(n, v)| Wert::Feld(vec![Wert::Text(n), Wert::Text(v)])).collect()),
                Wert::ByteFeld { daten: a.koerper, geklemmt: false },
            ])),
            Err(e) => Err(format!("NetworkError: {e}")),
        }
    });
    engine.host_funktion("__narsilWReportError", {
        let link = link.clone();
        move |args: &[Wert]| {
            let task = Task::WorkerError {
                worker: link.id,
                message: text(args, 0),
                filename: text(args, 1),
                lineno: num(args, 2).max(0.0) as u32,
                colno: num(args, 3).max(0.0) as u32,
            };
            to_parent(&link, task);
            Ok(Wert::Undefiniert)
        }
    });
}

/// Is this a JavaScript MIME type (MIME Sniffing §4.6)?
fn js_mime(content_type: &str) -> bool {
    let t = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    matches!(
        t.as_str(),
        "text/javascript"
            | "application/javascript"
            | "application/x-javascript"
            | "text/ecmascript"
            | "application/ecmascript"
            | "text/x-javascript"
            | "text/x-ecmascript"
            | "application/x-ecmascript"
            | "text/jscript"
            | "text/livescript"
            | "text/javascript1.0"
            | "text/javascript1.1"
            | "text/javascript1.2"
            | "text/javascript1.3"
            | "text/javascript1.4"
            | "text/javascript1.5"
    )
}

/// Fetches a worker script (or an imported one): `(body, final URL)`.
/// UTF-8 decode (Encoding "UTF-8 decode"): a UTF-8 BOM goes, any other
/// byte order mark stays as the replacement characters it decodes to.
fn utf8_decode(bytes: &[u8]) -> String {
    let b = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(b).into_owned()
}

fn fetch_script(url: &str, body: Option<Vec<u8>>, strict_mime: bool) -> Result<(String, String), String> {
    if let Some(b) = body {
        return Ok((utf8_decode(&b), url.to_string()));
    }
    if url.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("data:")) {
        let (_, bytes) = narsil_jsvm::netz::daten_url_lesen(url).ok_or_else(|| "bad data: URL".to_string())?;
        return Ok((utf8_decode(&bytes), url.to_string()));
    }
    if url.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("blob:")) {
        let bytes = blob_url_bytes(url).ok_or_else(|| "blob: URL without a Blob".to_string())?;
        return Ok((utf8_decode(&bytes), url.to_string()));
    }
    let (l, bytes) = crate::laden::lade_mit_bytes(url).map_err(|e| e.to_string())?;
    let http = url.starts_with("http://") || url.starts_with("https://");
    if http && !(200..300).contains(&l.status) {
        return Err(format!("HTTP {}", l.status));
    }
    if http && strict_mime && !js_mime(&l.content_type) {
        return Err(format!("MIME type {:?} is not JavaScript", l.content_type));
    }
    if bytes.len() > crate::js::MAX_SKRIPT_QUELLE {
        return Err("script too large".to_string());
    }
    let end = if l.end_adresse.is_empty() { url.to_string() } else { l.end_adresse };
    // Worker scripts are always UTF-8, whatever the charset parameter says.
    Ok((utf8_decode(&bytes), end))
}

fn worker_engine(start: &WorkerStart, url: &str) -> Engine {
    let mut g = Grenzen::standard();
    g.max_quelltext_bytes = crate::js::MAX_SKRIPT_QUELLE;
    g.max_token = 6_000_000;
    g.max_parser_tiefe = 2048;
    g.max_heap_bytes = WORKER_HEAP_BYTES;
    g.max_dauer_ms = 15_000;
    g.max_schritte = 300_000_000;
    let gesetzt = crate::js::JS_FRIST_MS.load(Ordering::Relaxed);
    if gesetzt > 0 {
        g.max_dauer_ms = gesetzt;
    }
    let mut e = Engine::neu(g);
    e.unbehandelte_ablehnungen_melden(true);
    e.timer_vom_host(true);
    e.set_interrupt_flag(start.interrupt.clone());
    let net_url = if url.starts_with("http://") || url.starts_with("https://") {
        Some(url.to_string())
    } else if start.origin.starts_with("http") {
        Some(format!("{}/", start.origin))
    } else {
        None
    };
    if let Some(u) = net_url
        && e.mit_herkunft(&u).is_ok()
    {
        e.mit_netz(Box::new(narsil_jsvm::netz::NarsilNet { https_erlaubt: true }));
        e.cors_erlauben(true);
    }
    e
}

/// Runs one host task in the worker engine with fresh budgets.
fn run_task<T>(e: &mut Engine, f: impl FnOnce(&mut Engine) -> T) -> T {
    e.schritte_zuruecksetzen();
    e.browser_task_beginnen();
    let r = f(e);
    e.browser_task_beenden();
    r
}

/// Reports an uncaught exception of the last run (HTML §8.1.4.6): the
/// worker global's error handlers first, then the parent.
fn report_uncaught(e: &mut Engine, fehler: &JsFehler) {
    if matches!(fehler, JsFehler::Zeitlimit(t) if t == "terminated") {
        return;
    }
    let text = fehler.to_string();
    if let Some(f) = e.funktionsnummer("__narsilWUncaught") {
        let _ = run_task(e, |e| e.handler_aufrufen(f, &[Wert::Text(text)]));
    }
}

/// Diagnostics: `NARSIL_KONSOLE_SPUR=1` shows the console of workers on
/// stderr (like the page's timer console).
fn trace_console(e: &Engine, seen: &mut usize, url: &str) {
    let k = e.konsole();
    if k.len() < *seen {
        *seen = 0;
    }
    if k.len() > *seen {
        if std::env::var_os("NARSIL_KONSOLE_SPUR").is_some() {
            for line in &k[*seen..] {
                eprintln!("KONSOLE-WORKER {url}: {}", line.chars().take(2000).collect::<String>());
            }
        }
        *seen = k.len();
    }
}

/// Time and task budget of one pump call (`messaging.js` `__narsilWPump`).
const PUMP_BUDGET_MS: f64 = 8.0;
const PUMP_MAX_TASKS: f64 = 64.0;

fn pump_once(e: &mut Engine) {
    let Some(f) = e.funktionsnummer("__narsilWPump") else { return };
    let r = run_task(e, |e| e.handler_aufrufen(f, &[Wert::Zahl(PUMP_BUDGET_MS), Wert::Zahl(PUMP_MAX_TASKS)]));
    if let Err(err) = r {
        report_uncaught(e, &err);
    }
}

fn worker_main(start: WorkerStart) {
    let link = WorkerLink { id: start.id, parent: start.parent, closing: Arc::new(AtomicBool::new(false)), shared: start.shared };
    let guard = Arc::new(ContextGuard { id: start.ctx, mailbox: start.mailbox.clone() });
    let slot = Arc::new(ContextSlot {
        origin: Mutex::new(start.origin.clone()),
        guard: OnceLock::new(),
        worker: Some(link.clone()),
    });
    let _ = slot.guard.set(guard.clone());
    drop(guard);
    let (source, final_url) = match fetch_script(&start.url, start.body.clone(), true) {
        Ok(x) => x,
        Err(_) => {
            to_parent(&link, Task::WorkerFailed { worker: start.id });
            mark_shared_failed(&link);
            return;
        }
    };
    if start.interrupt.load(Ordering::Relaxed) {
        return;
    }
    let mut e = worker_engine(&start, &final_url);
    bind_common(&mut e, slot.clone());
    bind_worker(&mut e, slot.clone(), &start, final_url.clone());
    crate::idb::bind(&mut e, start.origin.clone());
    crate::cache_storage::bind(&mut e, start.origin.clone());
    for (what, js) in [("worker scope", WORKER_SCOPE_JS), ("lazy apis", LAZY_APIS_JS), ("messaging", "__narsilLazyLoad('messaging')")] {
        if let Err(err) = e.ausfuehren(js) {
            eprintln!("narsil worker: {what} preamble failed: {err}");
        }
    }
    // Run the script (HTML §10.2.4 "run a worker").
    let result = if start.module {
        match e.module_starten(&final_url, &source) {
            Ok(()) => {
                let errors = crate::js::module_pumpen(&mut e);
                // A module of the graph that could not be fetched or parsed
                // ("Modul <url>: …", "Module fehlen noch") fails the load
                // (HTML §8.1.4.2 "fetch a module worker script graph"); an
                // exception in a module body is a runtime error.
                let load_failed = errors.iter().any(|f| !f.starts_with("Modul: ") || f.contains("Module fehlen noch"))
                    || !e.module_offene().is_empty();
                if load_failed {
                    Err(JsFehler::Syntax { zeile: 0, spalte: 0, nachricht: errors.join("; ") })
                } else if let Some(body) = errors.first() {
                    Err(JsFehler::Laufzeit(body.clone()))
                } else {
                    Ok(())
                }
            }
            Err(err) => Err(err),
        }
    } else {
        run_task(&mut e, |e| e.ausfuehren(&source).map(|_| ()))
    };
    match result {
        Ok(()) => {}
        Err(JsFehler::Syntax { .. }) => {
            // A script that does not parse is a failed load: a plain `error`
            // event at the Worker object (HTML §10.2.4 step 12).
            to_parent(&link, Task::WorkerFailed { worker: start.id });
            mark_shared_failed(&link);
            return;
        }
        Err(err) => report_uncaught(&mut e, &err),
    }
    let mut seen = 0usize;
    trace_console(&e, &mut seen, &final_url);
    // The event loop of the worker.
    loop {
        trace_console(&e, &mut seen, &final_url);
        if start.interrupt.load(Ordering::Relaxed) || link.closing.load(Ordering::Relaxed) {
            break;
        }
        let mut busy = false;
        if start.mailbox.pending() {
            pump_once(&mut e);
            busy = true;
        }
        if start.interrupt.load(Ordering::Relaxed) || link.closing.load(Ordering::Relaxed) {
            break;
        }
        e.uhr_auf_wanduhr();
        let due = e.timer_frueheste_ms().is_some_and(|t| t <= e.wanduhr_ms());
        if due {
            let r = run_task(&mut e, |e| e.timer_einen_ausfuehren());
            if let Err(err) = r {
                report_uncaught(&mut e, &err);
            }
            busy = true;
        }
        if e.netz_offen() > 0 {
            let _ = run_task(&mut e, |e| e.netz_schleife());
            busy = true;
        }
        if e.module_wartende() > 0 || !e.module_offene().is_empty() {
            let _ = crate::js::module_pumpen(&mut e);
        }
        if !busy {
            let wait = match e.timer_frueheste_ms() {
                Some(t) => t.saturating_sub(e.wanduhr_ms()).clamp(1, 1000),
                None => 1000,
            };
            start.mailbox.wait(Duration::from_millis(wait));
        }
    }
    // A closing worker drops what is still queued (HTML §10.2.6).
    drop(e);
}

fn mark_shared_failed(link: &WorkerLink) {
    if link.shared
        && let Some(s) = hub().shared.get_mut(&link.id)
    {
        s.failed = true;
    }
}

/// Binds the messaging host functions of a page engine; the context is
/// made lazily in `slot`.
pub fn bind_page(engine: &mut Engine, slot: Arc<ContextSlot>) {
    bind_common(engine, slot);
}

/// The worker scope preamble (tests).
pub fn worker_scope_js() -> &'static str {
    WORKER_SCOPE_JS
}

/// Lets the page run queued messaging tasks (`__narsilWPump`), at most
/// `max` of them or for about `budget_ms`; returns how many ran (0 if
/// nothing was waiting).
pub fn page_pump(engine: &mut Engine, slot: &ContextSlot, budget_ms: u64, max: usize) -> Result<usize, JsFehler> {
    if !slot.pending() {
        return Ok(0);
    }
    let Some(f) = engine.funktionsnummer("__narsilWPump") else {
        // Without the script side the tasks could never run: drop them.
        if let Some(g) = slot.existing() {
            let rest = g.mailbox.close();
            hub().close_ports_of(&rest);
        }
        return Ok(0);
    };
    let args = [Wert::Zahl(budget_ms as f64), Wert::Zahl(max.max(1) as f64)];
    engine.handler_aufrufen(f, &args).map(|w| match w {
        Wert::Zahl(n) if n >= 1.0 => n as usize,
        _ => 0,
    })
}

/// Live workers and contexts (diagnostics and tests).
pub fn live_counts() -> (usize, usize, usize) {
    let h = hub();
    (h.contexts.len(), h.workers.len() + h.shared.len(), h.ports.len())
}
