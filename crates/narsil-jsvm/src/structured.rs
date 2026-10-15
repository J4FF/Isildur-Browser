//! HTML §2.7 "Safe passing of structured data": StructuredSerializeWithTransfer
//! and StructuredDeserializeWithTransfer between engines.
//!
//! A serialized value is plain Rust data (`Send`), so it can cross threads
//! (dedicated and shared workers, MessagePort, BroadcastChannel) and be
//! stored (IndexedDB). Scripts never see it directly: the native
//! `__narsilSerialize` files it in a small process-wide registry and returns
//! a handle number; the host takes it out by that number ([`take`]), and the
//! receiving side files it again ([`put`]) for `__narsilDeserialize`.
//!
//! Object identity inside one message is kept through a memory map (the
//! "memory" of the spec): every object gets an index when it is first
//! serialized, and later occurrences become [`Ser::Ref`]. Containers get
//! their index before their children (so cycles work); a view gets its index
//! after its buffer. The deserializer assigns indices in exactly that order.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use crate::heap::{GVal, HObj, HeapRef};
use crate::interp::{Engine, Stoerung};

/// Upper bound for the bytes of one message (strings, buffers, blobs).
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024 * 1024;
/// Upper bound for the nesting depth of one message.
pub const MAX_DEPTH: u32 = 10_000;
/// How many serialized messages the registry keeps at most. A handle that
/// is never taken (a script that threw between serializing and posting)
/// must not leak memory forever: the oldest entries fall out.
const MAX_REGISTRY_ENTRIES: usize = 4096;

/// One serialized value.
#[derive(Debug, Clone)]
pub enum Ser {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    BigInt(i128),
    String(String),
    /// A back-reference to the object with this memory index.
    Ref(u32),
    Object(Vec<(String, Ser)>),
    Array { length: u64, props: Vec<(String, Ser)> },
    BoxedBool(bool),
    BoxedNumber(f64),
    BoxedString(String),
    Date(f64),
    RegExp { source: String, flags: String },
    ArrayBuffer(Vec<u8>),
    /// An ArrayBuffer from the transfer list: its bytes are in
    /// [`SerializedMessage::buffers`] at this index.
    TransferredBuffer(u32),
    /// A typed array (`kind` is the constructor name, `length` the element
    /// count) or a DataView (`kind` "DataView", `length` in bytes).
    View { kind: String, buffer: Box<Ser>, offset: usize, length: usize },
    Map(Vec<(Ser, Ser)>),
    Set(Vec<Ser>),
    Error { name: String, message: Option<String>, stack: Option<String> },
    DomException { name: String, message: String },
    Blob { bytes: Arc<Vec<u8>>, mime: String },
    File { bytes: Arc<Vec<u8>>, mime: String, name: String, last_modified: f64 },
    /// A transferred MessagePort: index into the port list of the message.
    Port(u32),
}

/// A serialized message with its transferred parts.
#[derive(Debug, Clone)]
pub struct SerializedMessage {
    pub value: Ser,
    /// The bytes of the transferred ArrayBuffers (moved, not copied).
    pub buffers: Vec<Vec<u8>>,
    /// Host ids of the transferred MessagePorts (filled in by the host).
    pub ports: Vec<u64>,
    /// Approximate size in bytes (for queue limits).
    pub bytes: usize,
}

impl SerializedMessage {
    /// A message that carries just a string (host-made messages).
    pub fn string(text: &str) -> Self {
        SerializedMessage { value: Ser::String(text.to_string()), buffers: Vec::new(), ports: Vec::new(), bytes: text.len() }
    }
}

struct Registry {
    next: u32,
    entries: BTreeMap<u32, SerializedMessage>,
}

fn registry() -> &'static Mutex<Registry> {
    static R: OnceLock<Mutex<Registry>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Registry { next: 1, entries: BTreeMap::new() }))
}

/// Files a message and returns its handle (always > 0).
pub fn put(message: SerializedMessage) -> u32 {
    let mut r = registry().lock().unwrap_or_else(|e| e.into_inner());
    let id = r.next;
    r.next = r.next.wrapping_add(1).max(1);
    r.entries.insert(id, message);
    while r.entries.len() > MAX_REGISTRY_ENTRIES {
        let Some(oldest) = r.entries.keys().next().copied() else { break };
        r.entries.remove(&oldest);
    }
    id
}

/// Takes a filed message out of the registry.
pub fn take(handle: u32) -> Option<SerializedMessage> {
    registry().lock().unwrap_or_else(|e| e.into_inner()).entries.remove(&handle)
}

/// A copy of a filed message (the entry stays).
pub fn peek(handle: u32) -> Option<SerializedMessage> {
    registry().lock().unwrap_or_else(|e| e.into_inner()).entries.get(&handle).cloned()
}

/// Kinds of heap objects, read once so that no heap borrow is held while the
/// serializer calls back into the engine (getters).
enum Kind {
    Plain,
    Array(u64),
    BoxedBool(bool),
    BoxedNumber(f64),
    BoxedString(GVal),
    Date(f64),
    RegExp(String, String),
    Buffer { shared: bool },
    Typed { kind: &'static str, buffer: HeapRef, offset: usize, length: usize },
    Map(Vec<(GVal, GVal)>),
    Set(Vec<GVal>),
    Error,
    Blob,
    NotCloneable(&'static str),
}

struct SerState {
    memory: HashMap<HeapRef, u32>,
    next: u32,
    transfer: Vec<HeapRef>,
    ports: Vec<HeapRef>,
    forbidden: Vec<HeapRef>,
    bytes: usize,
    depth: u32,
    file_proto: Option<HeapRef>,
    dom_exception_proto: Option<HeapRef>,
}

struct DeState {
    memory: Vec<GVal>,
    ports: Vec<GVal>,
    buffers: Vec<Option<Vec<u8>>>,
    depth: u32,
}


impl Engine {
    fn clone_error(&mut self, text: &str) -> Stoerung {
        Stoerung::Wurf(self.fehler_wert("DataCloneError", text))
    }

    /// The elements of a JS array argument (empty for anything else).
    fn array_elements(&self, v: GVal) -> Vec<GVal> {
        match v {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Feld { elemente, .. } => elemente.clone(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// The `prototype` of a global constructor (`File`, `DOMException`, ...).
    fn global_prototype(&mut self, name: &str) -> Option<HeapRef> {
        let global = GVal::Ref(self.global_objekt);
        match self.hole_ohne_host(global, name) {
            Ok(c @ GVal::Ref(_)) => match self.hole_ohne_host(c, "prototype") {
                Ok(GVal::Ref(p)) => Some(p),
                _ => None,
            },
            _ => None,
        }
    }

    /// Is `proto` on the prototype chain of `r` (bounded walk)?
    fn chain_has(&self, r: HeapRef, proto: HeapRef) -> bool {
        let mut cur = self.heap_proto_von(r);
        for _ in 0..64 {
            match cur {
                Some(p) if p == proto => return true,
                Some(p) => cur = self.heap_proto_von(p),
                None => return false,
            }
        }
        false
    }

    /// Is this ArrayBuffer detached (transferred away)?
    pub(crate) fn buffer_detached(&self, r: HeapRef) -> bool {
        matches!(self.heap.holen(r), HObj::BytePuffer { detached: true, .. })
    }

    /// Detaches an ArrayBuffer (ES2023 DetachArrayBuffer) and returns its
    /// bytes.
    fn buffer_detach(&mut self, r: HeapRef) -> Vec<u8> {
        if let HObj::BytePuffer { daten, detached, .. } = self.heap.holen_mut(r) {
            *detached = true;
            return std::mem::take(daten);
        }
        Vec::new()
    }

    fn kind_of(&self, r: HeapRef) -> Kind {
        match self.heap.holen(r) {
            HObj::Gegenstand { .. } => {
                if let Some(GVal::Ref(p)) = self.eigene_lese(r, "#es_dv_puffer")
                    && matches!(self.heap.holen(p), HObj::BytePuffer { .. })
                {
                    let offset = match self.eigene_lese(r, "#es_dv_versatz") { Some(GVal::Zahl(n)) => n as usize, _ => 0 };
                    let length = match self.eigene_lese(r, "#es_dv_umfang") { Some(GVal::Zahl(n)) => n as usize, _ => 0 };
                    return Kind::Typed { kind: "DataView", buffer: p, offset, length };
                }
                Kind::Plain
            }
            HObj::Feld { elemente, .. } => Kind::Array(elemente.len() as u64),
            HObj::Verpackt { primitiv, .. } => match primitiv {
                GVal::Bool(b) => Kind::BoxedBool(*b),
                GVal::Zahl(n) => Kind::BoxedNumber(*n),
                GVal::Ref(_) => Kind::BoxedString(*primitiv),
                _ => Kind::NotCloneable("this boxed value"),
            },
            HObj::Datum { millis, .. } => Kind::Date(*millis),
            HObj::Regex { muster, schalter, .. } => Kind::RegExp(muster.clone(), schalter.clone()),
            HObj::BytePuffer { geteilt, .. } => Kind::Buffer { shared: *geteilt },
            HObj::TypedArray { art, puffer, versatz, laenge, .. } => {
                Kind::Typed { kind: art.name(), buffer: *puffer, offset: *versatz, length: *laenge }
            }
            HObj::Karte { eintraege, belegt, .. } => Kind::Map(
                eintraege
                    .iter()
                    .zip(belegt.iter())
                    .filter(|(_, b)| **b)
                    .map(|(e, _)| *e)
                    .collect(),
            ),
            HObj::Menge { eintraege, belegt, .. } => Kind::Set(
                eintraege
                    .iter()
                    .zip(belegt.iter())
                    .filter(|(_, b)| **b)
                    .map(|(e, _)| *e)
                    .collect(),
            ),
            HObj::Fehler { .. } => Kind::Error,
            HObj::Nativ { name, .. } if name == "Blob" => Kind::Blob,
            HObj::Funktion { .. } | HObj::Nativ { .. } => Kind::NotCloneable("a function or platform object"),
            HObj::Versprechen { .. } => Kind::NotCloneable("a Promise"),
            HObj::Generator { .. } => Kind::NotCloneable("a generator"),
            HObj::Proxy { .. } => Kind::NotCloneable("a Proxy"),
            HObj::Zeichenkette(_) | HObj::Umgebung(_) => Kind::NotCloneable("this value"),
        }
    }

    fn text_of(&mut self, v: GVal) -> Result<String, Stoerung> {
        self.zu_text(v)
    }

    /// The enumerable own string keys of an object, and their values (Get,
    /// so getters run — HTML §2.7.3 step 26.4).
    fn serialize_props(&mut self, r: HeapRef, st: &mut SerState, skip_length: bool) -> Result<Vec<(String, Ser)>, Stoerung> {
        let mut keys = self.eigene_schluessel(GVal::Ref(r))?;
        keys.retain(|k| !crate::heap::ist_symbolschluessel(k) && !(skip_length && k == "length"));
        let mut out = Vec::with_capacity(keys.len());
        for k in keys {
            let v = self.hole(GVal::Ref(r), &k)?;
            self.wurzeln.push(v);
            st.bytes = st.bytes.saturating_add(k.len() + 16);
            let s = self.serialize_value(v, st)?;
            out.push((k, s));
        }
        Ok(out)
    }

    fn serialize_value(&mut self, v: GVal, st: &mut SerState) -> Result<Ser, Stoerung> {
        let r = match v {
            GVal::Undefiniert => return Ok(Ser::Undefined),
            GVal::Null => return Ok(Ser::Null),
            GVal::Bool(b) => return Ok(Ser::Bool(b)),
            GVal::Zahl(n) => return Ok(Ser::Number(n)),
            GVal::Ganz(g) => return Ok(Ser::BigInt(g)),
            GVal::Ref(r) => r,
        };
        if let HObj::Zeichenkette(s) = self.heap.holen(r) {
            st.bytes = st.bytes.saturating_add(s.len());
            if st.bytes > MAX_MESSAGE_BYTES {
                return Err(self.clone_error("The message is too large to be cloned."));
            }
            return Ok(Ser::String(s.clone()));
        }
        if let Some(&i) = st.memory.get(&r) {
            return Ok(Ser::Ref(i));
        }
        if let Some(i) = st.ports.iter().position(|p| *p == r) {
            return Ok(Ser::Port(i as u32));
        }
        if let Some(i) = st.transfer.iter().position(|p| *p == r) {
            let idx = st.next;
            st.next += 1;
            st.memory.insert(r, idx);
            return Ok(Ser::TransferredBuffer(i as u32));
        }
        if st.depth >= MAX_DEPTH {
            return Err(self.clone_error("The object is nested too deeply to be cloned."));
        }
        if crate::symbole::ist_symbol(self, r) {
            return Err(self.clone_error("A Symbol could not be cloned."));
        }
        if self.host_id_von_gval(v).is_some() || self.eigene_lese(r, crate::hostobjekt::HOST_MERKE).is_some() {
            return Err(self.clone_error("A platform object could not be cloned."));
        }
        let weak = matches!(self.heap_proto_von(r), Some(p) if p == self.proto_weakmap || p == self.proto_weakset);
        if weak {
            return Err(self.clone_error("A WeakMap or WeakSet could not be cloned."));
        }
        let forbidden = st.forbidden.clone();
        if forbidden.iter().any(|p| self.chain_has(r, *p))
            || self.chain_has(r, self.proto_ereignis)
            || self.chain_has(r, self.proto_ereignisziel)
        {
            return Err(self.clone_error("A platform object could not be cloned."));
        }
        if self.bytes_guard(st) {
            return Err(self.clone_error("The message is too large to be cloned."));
        }
        st.depth += 1;
        let out = self.serialize_object(r, st);
        st.depth -= 1;
        out
    }

    fn bytes_guard(&self, st: &mut SerState) -> bool {
        st.bytes = st.bytes.saturating_add(32);
        st.bytes > MAX_MESSAGE_BYTES
    }

    fn assign(st: &mut SerState, r: HeapRef) {
        let idx = st.next;
        st.next += 1;
        st.memory.insert(r, idx);
    }

    fn serialize_object(&mut self, r: HeapRef, st: &mut SerState) -> Result<Ser, Stoerung> {
        self.wurzeln.push(GVal::Ref(r));
        match self.kind_of(r) {
            Kind::NotCloneable(what) => Err(self.clone_error(&format!("{what} could not be cloned."))),
            Kind::BoxedBool(b) => {
                Self::assign(st, r);
                Ok(Ser::BoxedBool(b))
            }
            Kind::BoxedNumber(n) => {
                Self::assign(st, r);
                Ok(Ser::BoxedNumber(n))
            }
            Kind::BoxedString(s) => {
                Self::assign(st, r);
                let t = self.text_of(s)?;
                st.bytes = st.bytes.saturating_add(t.len());
                Ok(Ser::BoxedString(t))
            }
            Kind::Date(ms) => {
                Self::assign(st, r);
                Ok(Ser::Date(ms))
            }
            Kind::RegExp(source, flags) => {
                Self::assign(st, r);
                Ok(Ser::RegExp { source, flags })
            }
            Kind::Buffer { shared } => {
                if shared {
                    return Err(self.clone_error("A SharedArrayBuffer could not be cloned."));
                }
                if self.buffer_detached(r) {
                    return Err(self.clone_error("An ArrayBuffer is detached and could not be cloned."));
                }
                Self::assign(st, r);
                let bytes = match self.heap.holen(r) {
                    HObj::BytePuffer { daten, .. } => daten.clone(),
                    _ => Vec::new(),
                };
                st.bytes = st.bytes.saturating_add(bytes.len());
                if st.bytes > MAX_MESSAGE_BYTES {
                    return Err(self.clone_error("The message is too large to be cloned."));
                }
                Ok(Ser::ArrayBuffer(bytes))
            }
            Kind::Typed { kind, buffer, offset, length } => {
                if self.buffer_detached(buffer) {
                    return Err(self.clone_error("An ArrayBuffer is detached and could not be cloned."));
                }
                let b = self.serialize_value(GVal::Ref(buffer), st)?;
                Self::assign(st, r);
                Ok(Ser::View { kind: kind.to_string(), buffer: Box::new(b), offset, length })
            }
            Kind::Map(entries) => {
                Self::assign(st, r);
                for (k, v) in &entries {
                    self.wurzeln.push(*k);
                    self.wurzeln.push(*v);
                }
                let mut out = Vec::with_capacity(entries.len());
                for (k, v) in entries {
                    let ks = self.serialize_value(k, st)?;
                    let vs = self.serialize_value(v, st)?;
                    out.push((ks, vs));
                }
                Ok(Ser::Map(out))
            }
            Kind::Set(entries) => {
                Self::assign(st, r);
                self.wurzeln.extend_from_slice(&entries);
                let mut out = Vec::with_capacity(entries.len());
                for v in entries {
                    out.push(self.serialize_value(v, st)?);
                }
                Ok(Ser::Set(out))
            }
            Kind::Error => {
                Self::assign(st, r);
                if let Some(p) = st.dom_exception_proto
                    && self.chain_has(r, p)
                {
                    let n = self.hole(GVal::Ref(r), "name")?;
                    let name = self.text_of(n)?;
                    let m = self.hole(GVal::Ref(r), "message")?;
                    let message = self.text_of(m)?;
                    return Ok(Ser::DomException { name, message });
                }
                let n = self.hole(GVal::Ref(r), "name")?;
                let mut name = if matches!(n, GVal::Undefiniert) { "Error".to_string() } else { self.text_of(n)? };
                if !matches!(
                    name.as_str(),
                    "Error" | "EvalError" | "RangeError" | "ReferenceError" | "SyntaxError" | "TypeError" | "URIError"
                ) {
                    name = "Error".to_string();
                }
                let message = match self.eigene_lese(r, "message") {
                    Some(m) => Some(self.text_of(m)?),
                    None => match self.heap.holen(r) {
                        HObj::Fehler { nachricht, .. } if !nachricht.is_empty() => Some(nachricht.clone()),
                        _ => None,
                    },
                };
                let stack = match self.eigene_lese(r, "stack") {
                    Some(s @ GVal::Ref(_)) => Some(self.text_of(s)?),
                    _ => None,
                };
                Ok(Ser::Error { name, message, stack })
            }
            Kind::Blob => {
                Self::assign(st, r);
                let bytes = self.klecks_bytes(GVal::Ref(r))?;
                st.bytes = st.bytes.saturating_add(bytes.len());
                if st.bytes > MAX_MESSAGE_BYTES {
                    return Err(self.clone_error("The message is too large to be cloned."));
                }
                let t = self.hole(GVal::Ref(r), "type")?;
                let mime = if matches!(t, GVal::Undefiniert) { String::new() } else { self.text_of(t)? };
                let is_file = st.file_proto.is_some_and(|p| self.chain_has(r, p));
                if is_file {
                    let n = self.hole(GVal::Ref(r), "name")?;
                    let name = self.text_of(n)?;
                    let lm = self.hole(GVal::Ref(r), "lastModified")?;
                    let last_modified = self.zu_zahl(lm).unwrap_or(0.0);
                    return Ok(Ser::File { bytes: Arc::new(bytes), mime, name, last_modified });
                }
                Ok(Ser::Blob { bytes: Arc::new(bytes), mime })
            }
            Kind::Array(length) => {
                Self::assign(st, r);
                let props = self.serialize_props(r, st, true)?;
                Ok(Ser::Array { length, props })
            }
            Kind::Plain => {
                Self::assign(st, r);
                let props = self.serialize_props(r, st, false)?;
                Ok(Ser::Object(props))
            }
        }
    }

    /// StructuredSerializeWithTransfer: `value`, the ArrayBuffers to
    /// transfer, the MessagePort objects to transfer (their identity becomes
    /// [`Ser::Port`]) and prototypes whose instances must not be cloned.
    pub(crate) fn structured_serialize(
        &mut self,
        value: GVal,
        transfer: &[GVal],
        ports: &[GVal],
        forbidden: &[GVal],
    ) -> Result<SerializedMessage, Stoerung> {
        let marke = self.wurzel_merke();
        self.wurzeln.push(value);
        let result = self.structured_serialize_inner(value, transfer, ports, forbidden);
        self.wurzel_zurueck(marke);
        result
    }

    fn structured_serialize_inner(
        &mut self,
        value: GVal,
        transfer: &[GVal],
        ports: &[GVal],
        forbidden: &[GVal],
    ) -> Result<SerializedMessage, Stoerung> {
        let mut buffers: Vec<HeapRef> = Vec::new();
        for t in transfer {
            let GVal::Ref(r) = *t else {
                return Err(self.clone_error("Value at index of the transfer list is not transferable."));
            };
            let ok = matches!(self.heap.holen(r), HObj::BytePuffer { geteilt: false, .. });
            if !ok {
                return Err(self.clone_error("Value of the transfer list is not transferable."));
            }
            if buffers.contains(&r) {
                return Err(self.clone_error("ArrayBuffer is duplicated in the transfer list."));
            }
            if self.buffer_detached(r) {
                return Err(self.clone_error("An ArrayBuffer is detached and could not be transferred."));
            }
            buffers.push(r);
        }
        let mut port_refs: Vec<HeapRef> = Vec::new();
        for p in ports {
            if let GVal::Ref(r) = *p {
                port_refs.push(r);
            }
        }
        let forbidden: Vec<HeapRef> = forbidden.iter().filter_map(|f| if let GVal::Ref(r) = f { Some(*r) } else { None }).collect();
        let file_proto = self.global_prototype("File");
        let dom_exception_proto = self.global_prototype("DOMException");
        let mut st = SerState {
            memory: HashMap::new(),
            next: 0,
            transfer: buffers.clone(),
            ports: port_refs,
            forbidden,
            bytes: 0,
            depth: 0,
            file_proto,
            dom_exception_proto,
        };
        let ser = self.serialize_value(value, &mut st)?;
        // Only now, after the whole value serialized, are the buffers
        // detached (HTML §2.7.4 step 5): a DataCloneError keeps them intact.
        let mut moved = Vec::with_capacity(buffers.len());
        let mut bytes = st.bytes;
        for b in buffers {
            let data = self.buffer_detach(b);
            bytes = bytes.saturating_add(data.len());
            moved.push(data);
        }
        Ok(SerializedMessage { value: ser, buffers: moved, ports: Vec::new(), bytes })
    }

    /// StructuredDeserializeWithTransfer: `ports` are the MessagePort objects
    /// for [`Ser::Port`] (made by the caller for the transferred port ids).
    pub(crate) fn structured_deserialize(&mut self, message: SerializedMessage, ports: Vec<GVal>) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        self.wurzeln.extend_from_slice(&ports);
        let mut st = DeState {
            memory: Vec::new(),
            ports,
            buffers: message.buffers.into_iter().map(Some).collect(),
            depth: 0,
        };
        let result = self.deserialize_value(message.value, &mut st);
        if let Ok(v) = result {
            // Keep the result alive for the caller beyond the mark: the VM
            // puts it on its stack right after the native returns.
            self.wurzel_zurueck(marke);
            self.frisch.push(v);
            return Ok(v);
        }
        self.wurzel_zurueck(marke);
        result
    }

    fn remember(&mut self, st: &mut DeState, v: GVal) {
        self.wurzeln.push(v);
        st.memory.push(v);
    }

    fn deserialize_value(&mut self, s: Ser, st: &mut DeState) -> Result<GVal, Stoerung> {
        if st.depth >= MAX_DEPTH {
            return Err(self.clone_error("The message is nested too deeply."));
        }
        st.depth += 1;
        let out = self.deserialize_inner(s, st);
        st.depth -= 1;
        out
    }

    fn deserialize_inner(&mut self, s: Ser, st: &mut DeState) -> Result<GVal, Stoerung> {
        Ok(match s {
            Ser::Undefined => GVal::Undefiniert,
            Ser::Null => GVal::Null,
            Ser::Bool(b) => GVal::Bool(b),
            Ser::Number(n) => GVal::Zahl(n),
            Ser::BigInt(g) => GVal::Ganz(g),
            Ser::String(t) => self.text_wert(&t).map_err(Stoerung::Fehler)?,
            Ser::Ref(i) => match st.memory.get(i as usize) {
                Some(v) => *v,
                None => return Err(self.clone_error("The message is corrupt (reference).")),
            },
            Ser::Port(i) => match st.ports.get(i as usize) {
                Some(v) => *v,
                None => return Err(self.clone_error("The message is corrupt (port).")),
            },
            Ser::Object(props) => {
                let o = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
                self.remember(st, GVal::Ref(o));
                for (k, v) in props {
                    let w = self.deserialize_value(v, st)?;
                    self.wurzeln.push(w);
                    self.eigenschaft_mit_attributen(o, &k, w, true, true)?;
                }
                GVal::Ref(o)
            }
            Ser::Array { length, props } => {
                let a = self.neues_feld().map_err(Stoerung::Fehler)?;
                self.remember(st, GVal::Ref(a));
                if length > 0 {
                    self.setze(GVal::Ref(a), "length", GVal::Zahl(length as f64))?;
                }
                for (k, v) in props {
                    let w = self.deserialize_value(v, st)?;
                    self.wurzeln.push(w);
                    self.eigenschaft_mit_attributen(a, &k, w, true, true)?;
                }
                GVal::Ref(a)
            }
            Ser::BoxedBool(b) => {
                let o = self.n_bool_ctor(GVal::Undefiniert, &[GVal::Bool(b)], true)?;
                self.remember(st, o);
                o
            }
            Ser::BoxedNumber(n) => {
                let o = self.n_zahl_ctor(GVal::Undefiniert, &[GVal::Zahl(n)], true)?;
                self.remember(st, o);
                o
            }
            Ser::BoxedString(t) => {
                let tv = self.text_wert(&t).map_err(Stoerung::Fehler)?;
                self.wurzeln.push(tv);
                let o = self.n_text_ctor(GVal::Undefiniert, &[tv], true)?;
                self.remember(st, o);
                o
            }
            Ser::Date(ms) => {
                let d = self.datum_anlegen(ms)?;
                self.remember(st, d);
                d
            }
            Ser::RegExp { source, flags } => {
                let r = self.regex_anlegen(&source, &flags).map_err(Stoerung::Fehler)?;
                self.remember(st, r);
                r
            }
            Ser::ArrayBuffer(bytes) => {
                let b = self.buffer_from_vec(bytes)?;
                self.remember(st, b);
                b
            }
            Ser::TransferredBuffer(i) => {
                let bytes = st.buffers.get_mut(i as usize).and_then(|b| b.take()).unwrap_or_default();
                let b = self.buffer_from_vec(bytes)?;
                self.remember(st, b);
                b
            }
            Ser::View { kind, buffer, offset, length } => {
                let b = self.deserialize_value(*buffer, st)?;
                self.wurzeln.push(b);
                let GVal::Ref(br) = b else {
                    return Err(self.clone_error("The message is corrupt (view)."));
                };
                let blen = match self.heap.holen(br) {
                    HObj::BytePuffer { daten, .. } => daten.len(),
                    _ => return Err(self.clone_error("The message is corrupt (view buffer).")),
                };
                let v = if kind == "DataView" {
                    if offset.saturating_add(length) > blen {
                        return Err(self.clone_error("The message is corrupt (view range)."));
                    }
                    self.n_dataview_ctor(&[b, GVal::Zahl(offset as f64), GVal::Zahl(length as f64)])?
                } else {
                    let Some(art) = crate::eingebaut_typed::ElementArt::aus_name(&kind) else {
                        return Err(self.clone_error("The message is corrupt (view kind)."));
                    };
                    if offset.saturating_add(length.saturating_mul(art.bytes())) > blen {
                        return Err(self.clone_error("The message is corrupt (view range)."));
                    }
                    let proto = self.global_prototype(art.name()).unwrap_or(self.proto_objekt);
                    let obj = HObj::TypedArray {
                        proto: Some(proto),
                        art,
                        puffer: br,
                        versatz: offset,
                        laenge: length,
                        eigenschaften: Default::default(),
                        sperre: crate::heap::Sperre::Offen,
                    };
                    let kosten = crate::heap::Heap::kosten_fuer(&obj);
                    self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
                    GVal::Ref(self.belege(obj, kosten))
                };
                self.remember(st, v);
                v
            }
            Ser::Map(entries) => {
                let m = self.n_karte_ctor(&[], true)?;
                self.remember(st, m);
                let GVal::Ref(mr) = m else { return Ok(m) };
                for (k, v) in entries {
                    let kv = self.deserialize_value(k, st)?;
                    self.wurzeln.push(kv);
                    let vv = self.deserialize_value(v, st)?;
                    self.wurzeln.push(vv);
                    self.karte_setzen_innen(mr, kv, vv).map_err(Stoerung::Fehler)?;
                }
                m
            }
            Ser::Set(entries) => {
                let m = self.n_menge_ctor(&[], true)?;
                self.remember(st, m);
                for v in entries {
                    let vv = self.deserialize_value(v, st)?;
                    self.wurzeln.push(vv);
                    self.n_menge_hinzu(m, &[vv])?;
                }
                m
            }
            Ser::Error { name, message, stack } => {
                let args = match &message {
                    Some(m) => {
                        let t = self.text_wert(m).map_err(Stoerung::Fehler)?;
                        self.wurzeln.push(t);
                        vec![t]
                    }
                    None => Vec::new(),
                };
                let e = self.n_fehler_ctor(&name, &args, true)?;
                self.remember(st, e);
                if let (Some(s), GVal::Ref(er)) = (stack, e) {
                    let t = self.text_wert(&s).map_err(Stoerung::Fehler)?;
                    self.wurzeln.push(t);
                    self.eigenschaft_mit_attributen(er, "stack", t, false, true)?;
                }
                e
            }
            Ser::DomException { name, message } => {
                let e = self.dom_fehler_wert(&name, &message);
                if let GVal::Ref(er) = e {
                    // `dom_fehler_wert` keeps the given name only for known
                    // DOMException names; a clone keeps it as it was.
                    let t = self.text_wert(&message).map_err(Stoerung::Fehler)?;
                    self.wurzeln.push(t);
                    self.eigenschaft_mit_attributen(er, "message", t, false, true)?;
                }
                self.remember(st, e);
                e
            }
            Ser::Blob { bytes, mime } => {
                let Some(proto) = self.global_prototype("Blob") else {
                    return Err(self.clone_error("Blob is not available here."));
                };
                let b = self.klecks_bauen(proto, &bytes, &mime)?;
                self.remember(st, b);
                b
            }
            Ser::File { bytes, mime, name, last_modified } => {
                let Some(proto) = self.global_prototype("File") else {
                    return Err(self.clone_error("File is not available here."));
                };
                let b = self.klecks_bauen(proto, &bytes, &mime)?;
                self.remember(st, b);
                let nt = self.text_wert(&name).map_err(Stoerung::Fehler)?;
                self.wurzeln.push(nt);
                self.setze(b, "name", nt)?;
                self.setze(b, "lastModified", GVal::Zahl(last_modified))?;
                let leer = self.text_wert("").map_err(Stoerung::Fehler)?;
                self.setze(b, "webkitRelativePath", leer)?;
                b
            }
        })
    }

    /// A new ArrayBuffer that owns `bytes` (no copy).
    fn buffer_from_vec(&mut self, bytes: Vec<u8>) -> Result<GVal, Stoerung> {
        if bytes.len() > self.grenzen.max_heap_bytes {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "ArrayBuffer too large")));
        }
        self.vor_allok(bytes.len().saturating_add(crate::heap::KOSTEN_EINTRAG)).map_err(Stoerung::Fehler)?;
        let obj = HObj::BytePuffer {
            proto: Some(self.proto_puffer),
            daten: bytes,
            geteilt: false,
            detached: false,
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = crate::heap::Heap::kosten_fuer(&obj);
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        Ok(GVal::Ref(self.belege(obj, kosten)))
    }

    /// `__narsilSerialize(value, transferBuffers, ports, forbiddenProtos)`
    /// -> handle (a number). Throws a DataCloneError.
    pub(crate) fn n_structured_serialize(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let value = self.arg(args, 0);
        let transfer = self.array_elements(self.arg(args, 1));
        let ports = self.array_elements(self.arg(args, 2));
        let forbidden = self.array_elements(self.arg(args, 3));
        let message = self.structured_serialize(value, &transfer, &ports, &forbidden)?;
        Ok(GVal::Zahl(f64::from(put(message))))
    }

    /// `__narsilDeserialize(handle, ports)` -> value. Throws a
    /// DataCloneError if the handle is unknown or the data cannot be built.
    pub(crate) fn n_structured_deserialize(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let handle = match self.arg(args, 0) {
            GVal::Zahl(n) if n >= 1.0 && n <= f64::from(u32::MAX) => n as u32,
            _ => return Err(self.clone_error("Unknown message.")),
        };
        let ports = self.array_elements(self.arg(args, 1));
        let Some(message) = take(handle) else {
            return Err(self.clone_error("Unknown message."));
        };
        self.structured_deserialize(message, ports)
    }

    /// `__narsilSerializeClone(handle)` -> a second handle with the same
    /// data (BroadcastChannel delivers one message to many receivers).
    pub(crate) fn n_structured_copy(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let handle = match self.arg(args, 0) {
            GVal::Zahl(n) if n >= 1.0 && n <= f64::from(u32::MAX) => n as u32,
            _ => return Ok(GVal::Zahl(0.0)),
        };
        Ok(GVal::Zahl(peek(handle).map_or(0.0, |m| f64::from(put(m)))))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Engine, Grenzen};

    fn run(src: &str) -> String {
        let mut e = Engine::neu(Grenzen::standard());
        e.ausfuehren(src).map(|w| w.als_text()).unwrap_or_else(|f| format!("ERR {f}"))
    }

    #[test]
    fn round_trip_keeps_types_and_identity() {
        let out = run(
            "var o = {a: 1, s: 'x', d: new Date(5), r: /a+/gi, m: new Map([[1, 'one']]), st: new Set([2]),
              u8: new Uint8Array([1, 2, 3]), arr: [1, , 3], n: null, b: new Boolean(true)};
             o.self = o; o.again = o.u8;
             var h = __narsilSerialize(o, [], [], []);
             var c = __narsilDeserialize(h, []);
             [c !== o, c.self === c, c.again === c.u8, c.d.getTime(), c.r.source + c.r.flags,
              c.m.get(1), c.st.has(2), c.u8[2], c.u8 instanceof Uint8Array, 1 in c.arr, c.arr.length,
              c.n, c.b instanceof Boolean].join('|')",
        );
        assert_eq!(out, "true|true|true|5|a+gi|one|true|3|true|false|3||true");
    }

    #[test]
    fn functions_and_symbols_throw_data_clone_error() {
        let out = run(
            "var r = [];
             try { __narsilSerialize({f: function () {}}, [], [], []); r.push('no'); } catch (e) { r.push(e.name); }
             try { __narsilSerialize(Symbol('x'), [], [], []); r.push('no'); } catch (e) { r.push(e.name); }
             r.join(',')",
        );
        assert_eq!(out, "DataCloneError,DataCloneError");
    }

    #[test]
    fn transfer_detaches_the_buffer() {
        let out = run(
            "var b = new ArrayBuffer(8); var v = new Uint8Array(b); v[0] = 7;
             var h = __narsilSerialize({b: b}, [b], [], []);
             var c = __narsilDeserialize(h, []);
             var again; try { __narsilSerialize(b, [], [], []); again = 'no'; } catch (e) { again = e.name; }
             [b.byteLength, v.length, c.b.byteLength, new Uint8Array(c.b)[0], again].join(',')",
        );
        assert_eq!(out, "0,0,8,7,DataCloneError");
    }

    #[test]
    fn getters_run_and_errors_keep_their_kind() {
        let out = run(
            "var o = {}; var n = 0; Object.defineProperty(o, 'g', {get: function () { n++; return 4; }, enumerable: true});
             var c = __narsilDeserialize(__narsilSerialize(o, [], [], []), []);
             var e = __narsilDeserialize(__narsilSerialize(new RangeError('bad'), [], [], []), []);
             [c.g, n, e instanceof RangeError, e.message].join(',')",
        );
        assert_eq!(out, "4,1,true,bad");
    }
}
