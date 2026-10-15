//! WebAssembly JS API: the Rust half (the WebIDL half is `web_wasm.js`).
//!
//! One `narsil_wasm::Lauf` (store) per engine holds every module instance,
//! memory, table, global and tag a page creates; JS objects name store
//! entries by their index.
//!
//! ## Calls across the boundary
//!
//! A JS -> wasm call (an exported function is the native
//! `Nativ::WasmFunktion(store index)`) takes the store out of the engine,
//! runs it with the engine as `narsil_wasm::lauf::Host`, and puts it back.
//! A wasm -> JS call (an imported JS function) arrives in
//! [`Engine::call_import`] with the live store: it is put back into the engine
//! for the duration of the JS call, so the JS code can call wasm again.
//!
//! ## Memory bytes
//!
//! `Memory.buffer` must be one `ArrayBuffer` that JS and wasm both write.
//! The bytes live in the JS `ArrayBuffer` whenever JS runs and in the wasm
//! memory whenever wasm runs: every boundary crossing moves the `Vec<u8>`
//! (a pointer swap, no copy) for the memories that have a buffer object.
//! After `memory.grow` the old buffer is detached and a new one is made on
//! the next `buffer` read (JS API, "refresh the memory buffer").

use std::collections::HashMap;
use std::sync::Arc;

use narsil_wasm::Fehler;
use narsil_wasm::js::{ImportTabelle, ImportWert};
use narsil_wasm::lauf::{FremdFn, Funktion, Lauf};
use narsil_wasm::modell::{
    Art, Func, FuncTyp, ImportiertSpeicher, ImportiertTab, JEDER_ARRAYTYP, JEDER_EQTYP, JEDER_EXNTYP,
    JEDER_EXTERNTYP, JEDER_FUNKTIONSTYP, JEDER_I31TYP, JEDER_STRUKTYP, JEDER_TYP, KEIN_EXNTYP,
    KEIN_EXTERNTYP, KEIN_FUNKTIONSTYP, KEIN_TYP, Modul, Reftyp, Valtyp, ist_heaptyp_wildcard,
};
use narsil_wasm::wert::Wert;

use crate::fehler::JsFehler;
use crate::heap::{GVal, HObj, HeapRef};
use crate::interp::{Engine, Stoerung};
use crate::web_platform::PlatformOp;

/// The glue file (see the module header).
const GLUE: &str = include_str!("web_wasm.js");

/// Natives of `web_wasm.js` (`N.<name>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WasmOp {
    Init,
    Validate,
    Compile,
    CopyBytes,
    ModuleExports,
    ModuleImports,
    CustomSections,
    Instantiate,
    Func,
    MemNew,
    MemGrow,
    MemBuffer,
    TableNew,
    TableGrow,
    TableGet,
    TableSet,
    TableLength,
    GlobalNew,
    GlobalGet,
    GlobalSet,
    TagNew,
}

const NATIVES: &[(&str, WasmOp, u32)] = &[
    ("init", WasmOp::Init, 3),
    ("validate", WasmOp::Validate, 1),
    ("compile", WasmOp::Compile, 1),
    ("copyBytes", WasmOp::CopyBytes, 1),
    ("moduleExports", WasmOp::ModuleExports, 1),
    ("moduleImports", WasmOp::ModuleImports, 1),
    ("customSections", WasmOp::CustomSections, 2),
    ("instantiate", WasmOp::Instantiate, 3),
    ("func", WasmOp::Func, 1),
    ("memNew", WasmOp::MemNew, 3),
    ("memGrow", WasmOp::MemGrow, 2),
    ("memBuffer", WasmOp::MemBuffer, 1),
    ("tableNew", WasmOp::TableNew, 5),
    ("tableGrow", WasmOp::TableGrow, 4),
    ("tableGet", WasmOp::TableGet, 2),
    ("tableSet", WasmOp::TableSet, 4),
    ("tableLength", WasmOp::TableLength, 1),
    ("globalNew", WasmOp::GlobalNew, 4),
    ("globalGet", WasmOp::GlobalGet, 1),
    ("globalSet", WasmOp::GlobalSet, 2),
    ("tagNew", WasmOp::TagNew, 1),
];

/// Trap kind of a JS exception (or engine error) unwinding through wasm.
const T_HOST: &str = "trap:host";

/// The three error classes of the namespace.
#[derive(Clone, Copy)]
enum ErrKind {
    Compile,
    Link,
    Runtime,
}

/// Key of an `externref` value (one id per distinct JS value).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ExternKey {
    Ref(HeapRef),
    Num(u64),
    Big(i128),
    Bool(bool),
    Undefined,
}

/// The WebAssembly state of one engine.
pub(crate) struct WasmState {
    /// The store. During a wasm call it is out of here (a placeholder sits
    /// here) — except while an import runs JS.
    pub(crate) lauf: Lauf,
    modules: Vec<Arc<Modul>>,
    /// Per store memory: the `ArrayBuffer` that holds its bytes while JS
    /// runs, and the byte length it was made for.
    buffers: Vec<Option<(HeapRef, usize)>>,
    /// Memories with a buffer (the swap list).
    live: Vec<u32>,
    /// Shared memories (their buffer is a SharedArrayBuffer).
    shared: std::collections::HashSet<u32>,
    /// Imported JS functions: (function, wasm type) per host slot.
    imports: Vec<(GVal, Arc<FuncTyp>)>,
    /// `externref` values handed to wasm.
    externs: Vec<GVal>,
    extern_ids: HashMap<ExternKey, u32>,
    /// Exported function objects per store function (identity).
    functions: HashMap<u32, HeapRef>,
    /// The `name` of a store function: its index in the defining instance.
    names: HashMap<u32, u32>,
    /// A JS exception (or engine error) unwinding through wasm frames.
    pending: Option<Stoerung>,
    /// Prototypes of CompileError, LinkError, RuntimeError.
    protos: [Option<HeapRef>; 3],
    /// Opaque JS objects of wasm GC structs/arrays, both directions.
    gc_objects: HashMap<u32, HeapRef>,
    gc_ids: HashMap<HeapRef, u32>,
}

impl WasmState {
    fn new() -> WasmState {
        WasmState {
            lauf: Lauf::neu(web_limits()),
            modules: Vec::new(),
            buffers: Vec::new(),
            live: Vec::new(),
            shared: std::collections::HashSet::new(),
            imports: Vec::new(),
            externs: Vec::new(),
            extern_ids: HashMap::new(),
            functions: HashMap::new(),
            names: HashMap::new(),
            pending: None,
            protos: [None; 3],
            gc_objects: HashMap::new(),
            gc_ids: HashMap::new(),
        }
    }

    pub(crate) fn owns_buffer(&self, r: HeapRef) -> bool {
        self.buffers.iter().flatten().any(|(b, _)| *b == r)
    }

    pub(crate) fn roots(&self, out: &mut Vec<GVal>) {
        out.extend(self.buffers.iter().flatten().map(|(b, _)| GVal::Ref(*b)));
        out.extend(self.imports.iter().map(|(f, _)| *f));
        out.extend(self.externs.iter().copied());
        out.extend(self.functions.values().map(|f| GVal::Ref(*f)));
        out.extend(self.protos.iter().flatten().map(|p| GVal::Ref(*p)));
        out.extend(self.gc_objects.values().map(|o| GVal::Ref(*o)));
        if let Some(Stoerung::Wurf(v)) = &self.pending {
            out.push(*v);
        }
    }
}

/// Limits of page wasm: the store defaults, with the fuel a real page
/// needs (the fuel only ends runaway loops; it is refilled per call).
fn web_limits() -> narsil_wasm::Grenzen {
    let mut g = narsil_wasm::Grenzen::standard();
    g.brennstoff = narsil_wasm::grenzen::MAX_BRENNSTOFF_HART;
    g
}

fn valtype_from_code(c: u32) -> Option<Valtyp> {
    Some(match c {
        0 => Valtyp::I32,
        1 => Valtyp::I64,
        2 => Valtyp::F32,
        3 => Valtyp::F64,
        4 => Valtyp::Ref(Reftyp::Func),
        5 => Valtyp::Ref(Reftyp::Extern),
        _ => return None,
    })
}

fn art_code(a: &Art) -> u32 {
    match a {
        Art::Func(_) => 0,
        Art::Tabelle(_) => 1,
        Art::Speicher(_) => 2,
        Art::Global(_) => 3,
        Art::Tag(_) => 4,
    }
}

/// `(module, name)` of an import, from the per-kind lists.
fn import_name(m: &Modul, a: &Art) -> Option<(String, String)> {
    match a {
        Art::Func(i) => match m.funktionen.get(*i as usize)? {
            Func::Importiert { modul, feld, .. } => Some((modul.clone(), feld.clone())),
            _ => None,
        },
        Art::Tabelle(i) => match m.tabellen.get(*i as usize)? {
            ImportiertTab::Importiert { modul, feld, .. } => Some((modul.clone(), feld.clone())),
            _ => None,
        },
        Art::Speicher(i) => match m.speicher.get(*i as usize)? {
            ImportiertSpeicher::Importiert { modul, feld, .. } => Some((modul.clone(), feld.clone())),
            _ => None,
        },
        Art::Global(i) => m.globale.get(*i as usize)?.import.clone(),
        Art::Tag(i) => m.tags.get(*i as usize)?.import.clone(),
    }
}

/// The imports in module order (modules from the binary reader carry it;
/// for others the per-kind order is the best there is).
fn import_order(m: &Modul) -> Vec<Art> {
    if !m.import_reihe.is_empty() {
        return m.import_reihe.clone();
    }
    let mut v = Vec::new();
    for (i, f) in m.funktionen.iter().enumerate() {
        if matches!(f, Func::Importiert { .. }) {
            v.push(Art::Func(i as u32));
        }
    }
    for (i, t) in m.tabellen.iter().enumerate() {
        if matches!(t, ImportiertTab::Importiert { .. }) {
            v.push(Art::Tabelle(i as u32));
        }
    }
    for (i, s) in m.speicher.iter().enumerate() {
        if matches!(s, ImportiertSpeicher::Importiert { .. }) {
            v.push(Art::Speicher(i as u32));
        }
    }
    for (i, g) in m.globale.iter().enumerate() {
        if g.import.is_some() {
            v.push(Art::Global(i as u32));
        }
    }
    for (i, t) in m.tags.iter().enumerate() {
        if t.import.is_some() {
            v.push(Art::Tag(i as u32));
        }
    }
    v
}

fn func_type(f: &Funktion) -> &FuncTyp {
    match f {
        Funktion::Eigen { typ, .. } | Funktion::Fremd { typ, .. } => typ,
    }
}

impl Engine {
    fn wasm(&self) -> &WasmState {
        self.platform.wasm.as_deref().expect("WebAssembly state")
    }

    fn wasm_mut(&mut self) -> &mut WasmState {
        self.platform.wasm.as_deref_mut().expect("WebAssembly state")
    }

    /// Run `web_wasm.js` (first use of `WebAssembly`).
    pub(crate) fn wasm_glue_laden(&mut self) -> Result<(), Stoerung> {
        if self.platform.wasm.is_none() {
            self.platform.wasm = Some(Box::new(WasmState::new()));
        }
        let list: Vec<(&str, PlatformOp, u32)> =
            NATIVES.iter().map(|(n, o, l)| (*n, PlatformOp::Wasm(*o), *l)).collect();
        let natives = self.platform_natives(&list)?;
        self.platform_run_glue(GLUE, natives)
    }

    pub(crate) fn type_error(&mut self, msg: &str) -> Stoerung {
        Stoerung::Wurf(self.fehler_wert("TypeError", msg))
    }

    fn range_error(&mut self, msg: &str) -> Stoerung {
        Stoerung::Wurf(self.fehler_wert("RangeError", msg))
    }

    /// An instance of one of the namespace's error classes.
    fn wasm_error(&mut self, kind: ErrKind, msg: &str) -> Stoerung {
        let (i, name) = match kind {
            ErrKind::Compile => (0, "CompileError"),
            ErrKind::Link => (1, "LinkError"),
            ErrKind::Runtime => (2, "RuntimeError"),
        };
        let Some(proto) = self.wasm().protos[i] else {
            return Stoerung::Wurf(self.fehler_wert("Error", msg));
        };
        let obj = HObj::Fehler {
            proto: Some(proto),
            name: name.to_string(),
            nachricht: msg.to_string(),
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = crate::heap::Heap::kosten_fuer(&obj) + msg.len().min(4096);
        let r = self.belege(obj, kosten);
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(r));
        if let Ok(m) = self.text_wert(msg) {
            let _ = self.eigenschaft_mit_attributen(r, "message", m, true, false);
        }
        self.wurzel_zurueck(marke);
        Stoerung::Wurf(GVal::Ref(r))
    }

    /// A wasm error as the JS exception the API names for it.
    fn wasm_fehler_js(&mut self, e: Fehler) -> Stoerung {
        if let Some(p) = self.wasm_mut().pending.take() {
            return p;
        }
        let text = e.kurz();
        match e {
            Fehler::Form(..) | Fehler::Ungueltig(..) => self.wasm_error(ErrKind::Compile, &text),
            Fehler::Link(..) => self.wasm_error(ErrKind::Link, &text),
            Fehler::Trap(art, _) if art == narsil_wasm::fehler::T_AUFRUFTIEFE => {
                self.range_error("Maximum call stack size exceeded")
            }
            Fehler::Trap(..) | Fehler::Brennstoff { .. } => self.wasm_error(ErrKind::Runtime, &text),
        }
    }

    // ---- bytes -------------------------------------------------------

    /// The bytes of a `BufferSource` (copy).
    pub(crate) fn buffer_source_bytes(&mut self, v: GVal) -> Result<Vec<u8>, Stoerung> {
        if let GVal::Ref(r) = v {
            match self.heap.holen(r) {
                HObj::BytePuffer { daten, .. } => return Ok(daten.clone()),
                HObj::TypedArray { .. } => return Ok(self.heap.typedarray_bytes(r).unwrap_or_default()),
                _ => {}
            }
            if let Some(GVal::Ref(p)) = self.eigene_lese(r, "#es_dv_puffer")
                && matches!(self.heap.holen(p), HObj::BytePuffer { .. })
            {
                let (buf, off, len) = self.dataview_teile(v)?;
                if let HObj::BytePuffer { daten, .. } = self.heap.holen(buf) {
                    let a = off.min(daten.len());
                    let b = off.saturating_add(len).min(daten.len());
                    return Ok(daten[a..b].to_vec());
                }
            }
        }
        Err(self.type_error("argument must be an ArrayBuffer or an ArrayBuffer view"))
    }

    pub(crate) fn new_array_buffer(&mut self, bytes: Vec<u8>) -> Result<GVal, Stoerung> {
        let b = self.neuer_bytepuffer(0, false)?;
        self.heap.kosten_verbuchen(bytes.len());
        if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(b) {
            *daten = bytes;
        }
        Ok(GVal::Ref(b))
    }

    pub(crate) fn array_of(&mut self, values: &[GVal]) -> Result<GVal, Stoerung> {
        Ok(GVal::Ref(self.feld_aus_werten(values)?))
    }

    fn index_arg(&mut self, args: &[GVal], i: usize) -> Result<u32, Stoerung> {
        let n = self.zu_zahl(self.arg(args, i))?;
        if !(0.0..=u32::MAX as f64).contains(&n) {
            return Err(self.range_error("WebAssembly: index out of range"));
        }
        Ok(n as u32)
    }

    fn opt_index_arg(&mut self, args: &[GVal], i: usize) -> Result<Option<u64>, Stoerung> {
        let n = self.zu_zahl(self.arg(args, i))?;
        Ok(if n < 0.0 { None } else { Some(n as u64) })
    }

    // ---- memory bytes across the boundary ----------------------------

    /// JS -> wasm: the bytes of every memory with a buffer go into the
    /// store.
    fn wasm_swap_in(&mut self, lauf: &mut Lauf) {
        let live = std::mem::take(&mut self.wasm_mut().live);
        for &m in &live {
            let Some((buf, _)) = self.wasm().buffers.get(m as usize).copied().flatten() else { continue };
            let bytes = match self.heap.holen_mut(buf) {
                HObj::BytePuffer { daten, .. } => std::mem::take(daten),
                _ => Vec::new(),
            };
            if let Some(s) = lauf.store.speicher.get_mut(m as usize) {
                s.put_bytes(bytes);
            }
        }
        self.wasm_mut().live = live;
    }

    /// wasm -> JS: the bytes go back into the buffers; a memory that grew
    /// detaches its buffer (a new one is made on the next `buffer` read).
    fn wasm_swap_out(&mut self, lauf: &mut Lauf) {
        let mut live = std::mem::take(&mut self.wasm_mut().live);
        live.retain(|&m| {
            let Some((buf, len)) = self.wasm().buffers.get(m as usize).copied().flatten() else {
                return false;
            };
            let Some(s) = lauf.store.speicher.get_mut(m as usize) else { return false };
            if s.byte_len() == len {
                let bytes = s.take_bytes();
                if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(buf) {
                    *daten = bytes;
                }
                true
            } else {
                self.detach_buffer(buf);
                self.wasm_mut().buffers[m as usize] = None;
                false
            }
        });
        self.wasm_mut().live = live;
    }

    fn detach_buffer(&mut self, buf: HeapRef) {
        if let HObj::BytePuffer { daten, detached, .. } = self.heap.holen_mut(buf) {
            daten.clear();
            *detached = true;
        }
    }

    /// Run `f` with the store taken out of the engine (bytes swapped in).
    fn wasm_with_lauf<R>(&mut self, f: impl FnOnce(&mut Engine, &mut Lauf) -> R) -> R {
        let grenzen = self.wasm().lauf.grenzen.clone();
        let mut lauf = std::mem::replace(&mut self.wasm_mut().lauf, Lauf::neu(grenzen));
        self.wasm_swap_in(&mut lauf);
        let r = f(self, &mut lauf);
        self.wasm_swap_out(&mut lauf);
        self.wasm_mut().lauf = lauf;
        r
    }

    // ---- values ------------------------------------------------------

    /// The store index of an exported wasm function object.
    fn wasm_function_index(&self, v: GVal) -> Option<u32> {
        let GVal::Ref(r) = v else { return None };
        match self.native.get(&r) {
            Some(crate::builtin::Nativ::WasmFunktion(i)) => Some(*i),
            _ => None,
        }
    }

    /// The exported function object of store function `idx` (one per
    /// function: identity across exports, tables and globals).
    pub(crate) fn wasm_function_object(&mut self, idx: u32) -> Result<HeapRef, Stoerung> {
        if let Some(f) = self.wasm().functions.get(&idx) {
            return Ok(*f);
        }
        let Some(f) = self.wasm().lauf.store.funktionen.get(idx as usize).cloned() else {
            return Err(self.type_error("WebAssembly: unknown function"));
        };
        let params = func_type(&f).parameter.len() as u32;
        let name = self.wasm().names.get(&idx).copied().unwrap_or(idx).to_string();
        let r = self.nativ_anlegen(&name, crate::builtin::Nativ::WasmFunktion(idx), self.proto_funktion);
        self.funktionsnamen_setzen(r, &name, params);
        self.wasm_mut().functions.insert(idx, r);
        Ok(r)
    }

    fn extern_id(&mut self, v: GVal) -> u32 {
        let key = match v {
            GVal::Ref(r) => ExternKey::Ref(r),
            GVal::Zahl(n) => ExternKey::Num(n.to_bits()),
            GVal::Ganz(g) => ExternKey::Big(g),
            GVal::Bool(b) => ExternKey::Bool(b),
            _ => ExternKey::Undefined,
        };
        let st = self.wasm_mut();
        if let Some(i) = st.extern_ids.get(&key) {
            return *i;
        }
        let i = st.externs.len() as u32;
        st.externs.push(v);
        st.extern_ids.insert(key, i);
        i
    }

    /// ToBigInt64 (JS API ToWebAssemblyValue for i64).
    fn to_bigint64(&mut self, v: GVal) -> Result<i64, Stoerung> {
        let p = self.zu_primitiv(v, false)?;
        let g = match p {
            GVal::Ganz(g) => g,
            GVal::Bool(b) => i128::from(b),
            GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Zeichenkette(_)) => {
                match self.n_bigint_ctor(&[p])? {
                    GVal::Ganz(g) => g,
                    _ => return Err(self.type_error("cannot convert to BigInt")),
                }
            }
            _ => return Err(self.type_error("cannot convert to BigInt")),
        };
        Ok(g as i64)
    }

    /// ToWebAssemblyValue.
    fn wasm_to_value(&mut self, v: GVal, t: Valtyp) -> Result<Wert, Stoerung> {
        Ok(match t {
            Valtyp::I32 => match v {
                GVal::Zahl(n) if n.fract() == 0.0 && n.abs() < 2147483648.0 => Wert::I32(n as i32),
                _ => Wert::I32(self.zu_int32(v)?),
            },
            Valtyp::I64 => Wert::I64(self.to_bigint64(v)?),
            Valtyp::F32 => Wert::F32((self.zu_zahl(v)? as f32).to_bits()),
            Valtyp::F64 => Wert::F64(self.zu_zahl(v)?.to_bits()),
            Valtyp::Ref(Reftyp::Extern) => {
                if v == GVal::Null {
                    Wert::Extern(None)
                } else {
                    Wert::Extern(Some(self.extern_id(v)))
                }
            }
            Valtyp::Ref(r) => {
                let (typ, nullable) = match r {
                    Reftyp::Konkret { typ, nullable } => (typ, nullable),
                    _ => (JEDER_FUNKTIONSTYP, true),
                };
                if v == GVal::Null {
                    if !nullable {
                        return Err(self.type_error("WebAssembly: null for a non-nullable reference"));
                    }
                    return Ok(match typ {
                        JEDER_EXTERNTYP | KEIN_EXTERNTYP => Wert::Extern(None),
                        JEDER_FUNKTIONSTYP | KEIN_FUNKTIONSTYP => Wert::Func(None),
                        t if !ist_heaptyp_wildcard(t) => Wert::Func(None),
                        _ => Wert::Gcref(None),
                    });
                }
                match typ {
                    // `(ref extern)`: any JS value.
                    JEDER_EXTERNTYP => Wert::Extern(Some(self.extern_id(v))),
                    // anyref: a wasm GC object, an i31-range number, or any
                    // other JS value as an internalized host reference.
                    JEDER_TYP => match self.wasm_gc_value(v) {
                        Some(w) => w,
                        None => Wert::Extern(Some(self.extern_id(v))),
                    },
                    JEDER_EQTYP | JEDER_I31TYP | JEDER_STRUKTYP | JEDER_ARRAYTYP => {
                        match self.wasm_gc_value(v) {
                            Some(w) if self.wasm_gc_fits(w, typ) => w,
                            _ => return Err(self.type_error("WebAssembly: value does not match the reference type")),
                        }
                    }
                    KEIN_TYP | KEIN_FUNKTIONSTYP | KEIN_EXTERNTYP | KEIN_EXNTYP | JEDER_EXNTYP => {
                        return Err(self.type_error("WebAssembly: value does not match the reference type"));
                    }
                    // funcref, `(ref func)` and concrete function types.
                    _ => match self.wasm_function_index(v) {
                        Some(i) => Wert::Func(Some(i)),
                        None => {
                            return Err(self.type_error("WebAssembly: value is not an exported wasm function"));
                        }
                    },
                }
            }
        })
    }

    /// A JS value as a wasm GC value: an exported GC object, or a number
    /// in the i31 range (`ToWebAssemblyValue` for anyref/eqref/i31ref).
    fn wasm_gc_value(&mut self, v: GVal) -> Option<Wert> {
        match v {
            GVal::Zahl(n) if n.fract() == 0.0 && (-1073741824.0..=1073741823.0).contains(&n) && (n != 0.0 || n.is_sign_positive()) => {
                let st = self.wasm_mut();
                st.lauf.store.gc.push(narsil_wasm::modell::GcObjekt {
                    typ: JEDER_I31TYP,
                    laenge: 1,
                    inhalt: narsil_wasm::modell::GcInhalt::I31((n as i32) & 0x7fff_ffff),
                });
                Some(Wert::Gcref(Some((st.lauf.store.gc.len() - 1) as u32)))
            }
            GVal::Ref(r) => self.wasm().gc_ids.get(&r).map(|id| Wert::Gcref(Some(*id))),
            _ => None,
        }
    }

    /// Does a GC value fit an abstract heap type (eq, i31, struct, array)?
    fn wasm_gc_fits(&self, w: Wert, typ: u32) -> bool {
        let Wert::Gcref(Some(id)) = w else { return false };
        let Some(o) = self.wasm().lauf.store.gc.get(id as usize) else { return false };
        let i31 = matches!(o.inhalt, narsil_wasm::modell::GcInhalt::I31(_));
        match typ {
            JEDER_EQTYP => true,
            JEDER_I31TYP => i31,
            JEDER_ARRAYTYP => !i31 && o.typ == JEDER_ARRAYTYP,
            JEDER_STRUKTYP => !i31 && o.typ != JEDER_ARRAYTYP,
            _ => false,
        }
    }

    /// The JS value of a wasm GC reference: i31 as a number, struct and
    /// array as an opaque frozen object (one per GC object).
    fn wasm_gc_to_js(&mut self, id: u32) -> Result<GVal, Stoerung> {
        if let Some(narsil_wasm::modell::GcObjekt { inhalt: narsil_wasm::modell::GcInhalt::I31(w), .. }) =
            self.wasm().lauf.store.gc.get(id as usize)
        {
            // Sign-extend the 31 bits.
            return Ok(GVal::Zahl(((*w << 1) >> 1) as f64));
        }
        if let Some(o) = self.wasm().gc_objects.get(&id) {
            return Ok(GVal::Ref(*o));
        }
        let o = self.neues_objekt(None).map_err(Stoerung::Fehler)?;
        if let HObj::Gegenstand { sperre, .. } = self.heap.holen_mut(o) {
            *sperre = crate::heap::Sperre::Gefroren;
        }
        let st = self.wasm_mut();
        st.gc_objects.insert(id, o);
        st.gc_ids.insert(o, id);
        Ok(GVal::Ref(o))
    }

    /// ToJSValue.
    fn wasm_to_js(&mut self, w: Wert) -> Result<GVal, Stoerung> {
        Ok(match w {
            Wert::I32(v) => GVal::Zahl(v as f64),
            Wert::I64(v) => GVal::Ganz(v as i128),
            Wert::F32(b) => GVal::Zahl(f32::from_bits(b) as f64),
            Wert::F64(b) => GVal::Zahl(f64::from_bits(b)),
            Wert::Func(None) | Wert::Extern(None) | Wert::Gcref(None) => GVal::Null,
            Wert::Func(Some(i)) => GVal::Ref(self.wasm_function_object(i)?),
            Wert::Extern(Some(i)) => self.wasm().externs.get(i as usize).copied().unwrap_or(GVal::Undefiniert),
            Wert::Gcref(Some(id)) => self.wasm_gc_to_js(id)?,
        })
    }

    /// DefaultValue(valtype) of the JS API.
    fn wasm_default(&mut self, t: Valtyp) -> Wert {
        match t {
            Valtyp::Ref(Reftyp::Extern) => Wert::Extern(Some(self.extern_id(GVal::Undefiniert))),
            _ => Wert::standard(t),
        }
    }

    // ---- calls -------------------------------------------------------

    /// Call of an exported function object (`Nativ::WasmFunktion`).
    pub(crate) fn wasm_call_export(
        &mut self,
        idx: u32,
        args: &[GVal],
        als_konstruktor: bool,
    ) -> Result<GVal, Stoerung> {
        if als_konstruktor {
            return Err(self.type_error("WebAssembly function is not a constructor"));
        }
        let Some(f) = self.platform.wasm.as_ref().and_then(|w| w.lauf.store.funktionen.get(idx as usize).cloned())
        else {
            return Err(self.type_error("WebAssembly: unknown function"));
        };
        let typ = func_type(&f);
        let mut vals: Vec<Wert> = Vec::with_capacity(typ.parameter.len());
        for (i, t) in typ.parameter.iter().enumerate() {
            let a = args.get(i).copied().unwrap_or(GVal::Undefiniert);
            vals.push(self.wasm_to_value(a, *t)?);
        }
        let r = self.wasm_with_lauf(|e, lauf| lauf.rufe_store_host(e, idx, &vals));
        let r = match r {
            Ok(r) => r,
            Err(e) => return Err(self.wasm_fehler_js(e)),
        };
        match r.len() {
            0 => Ok(GVal::Undefiniert),
            1 => self.wasm_to_js(r[0]),
            _ => {
                let marke = self.wurzel_merke();
                let mut out = Vec::with_capacity(r.len());
                for w in r {
                    let v = self.wasm_to_js(w)?;
                    self.wurzeln.push(v);
                    out.push(v);
                }
                let a = self.array_of(&out);
                self.wurzel_zurueck(marke);
                a
            }
        }
    }

    /// An imported JS function, called from wasm (the store is back in the
    /// engine while it runs).
    fn wasm_call_js(&mut self, slot: u32, args: &[Wert]) -> Result<Vec<Wert>, Stoerung> {
        let Some((f, typ)) = self.wasm().imports.get(slot as usize).cloned() else {
            return Err(self.type_error("WebAssembly: unknown import"));
        };
        let marke = self.wurzel_merke();
        self.wurzeln.push(f);
        let mut js = Vec::with_capacity(args.len());
        for w in args {
            let v = match self.wasm_to_js(*w) {
                Ok(v) => v,
                Err(e) => {
                    self.wurzel_zurueck(marke);
                    return Err(e);
                }
            };
            self.wurzeln.push(v);
            js.push(v);
        }
        let ret = self.aufruf_synchron(f, GVal::Undefiniert, &js, false);
        let ret = match ret {
            Ok(v) => v,
            Err(e) => {
                self.wurzel_zurueck(marke);
                return Err(e);
            }
        };
        self.wurzeln.push(ret);
        let r = match typ.ergebnis.len() {
            0 => Ok(Vec::new()),
            1 => self.wasm_to_value(ret, typ.ergebnis[0]).map(|w| vec![w]),
            n => self.wasm_results_from_iterable(ret, &typ.ergebnis, n),
        };
        self.wurzel_zurueck(marke);
        r
    }

    /// Multi-value results of a JS import: IterableToList, then one
    /// ToWebAssemblyValue per value (JS API "run a host function").
    fn wasm_results_from_iterable(
        &mut self,
        ret: GVal,
        types: &[Valtyp],
        n: usize,
    ) -> Result<Vec<Wert>, Stoerung> {
        if !self.ist_objekt(ret) {
            return Err(self.type_error("WebAssembly: a multi-value import must return an iterable"));
        }
        let (iter, next) = self.iterator_holen(ret)?;
        self.wurzeln.push(iter);
        self.wurzeln.push(next);
        let mut werte = Vec::with_capacity(n);
        while let Some(w) = self.iterator_schritt(iter, next)? {
            self.wurzeln.push(w);
            werte.push(w);
            if werte.len() > n {
                break;
            }
        }
        if werte.len() != n {
            return Err(self.type_error("WebAssembly: wrong number of results"));
        }
        let mut out = Vec::with_capacity(n);
        for (w, t) in werte.iter().zip(types.iter()) {
            out.push(self.wasm_to_value(*w, *t)?);
        }
        Ok(out)
    }

    // ---- natives -----------------------------------------------------

    pub(crate) fn wasm_native(&mut self, op: WasmOp, _this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        if self.platform.wasm.is_none() {
            return Err(self.type_error("WebAssembly is not initialized"));
        }
        match op {
            WasmOp::Init => {
                for i in 0..3 {
                    if let GVal::Ref(p) = self.arg(args, i) {
                        self.wasm_mut().protos[i] = Some(p);
                    }
                }
                Ok(GVal::Undefiniert)
            }
            WasmOp::Validate => {
                let bytes = self.buffer_source_bytes(self.arg(args, 0))?;
                let g = self.wasm().lauf.grenzen.clone();
                Ok(GVal::Bool(narsil_wasm::modul_pruefen(&bytes, &g).is_ok()))
            }
            WasmOp::Compile => {
                let bytes = self.buffer_source_bytes(self.arg(args, 0))?;
                let g = self.wasm().lauf.grenzen.clone();
                match narsil_wasm::modul_pruefen(&bytes, &g) {
                    Ok(m) => {
                        let st = self.wasm_mut();
                        st.modules.push(Arc::new(m));
                        Ok(GVal::Zahl((st.modules.len() - 1) as f64))
                    }
                    Err(e) => {
                        let t = e.kurz();
                        Err(self.wasm_error(ErrKind::Compile, &t))
                    }
                }
            }
            WasmOp::CopyBytes => {
                let bytes = self.buffer_source_bytes(self.arg(args, 0))?;
                self.new_array_buffer(bytes)
            }
            WasmOp::ModuleExports => {
                let m = self.wasm_module_arg(args)?;
                let marke = self.wurzel_merke();
                let mut out = Vec::new();
                for e in &m.exporte {
                    let n = self.text_wert(&e.name).map_err(Stoerung::Fehler)?;
                    self.wurzeln.push(n);
                    out.push(n);
                    out.push(GVal::Zahl(art_code(&e.art) as f64));
                }
                let a = self.array_of(&out);
                self.wurzel_zurueck(marke);
                a
            }
            WasmOp::ModuleImports => {
                let m = self.wasm_module_arg(args)?;
                let marke = self.wurzel_merke();
                let mut out = Vec::new();
                for a in import_order(&m) {
                    let (modul, feld) = import_name(&m, &a).unwrap_or_default();
                    let mv = self.text_wert(&modul).map_err(Stoerung::Fehler)?;
                    self.wurzeln.push(mv);
                    let fv = self.text_wert(&feld).map_err(Stoerung::Fehler)?;
                    self.wurzeln.push(fv);
                    out.push(mv);
                    out.push(fv);
                    out.push(GVal::Zahl(art_code(&a) as f64));
                }
                let r = self.array_of(&out);
                self.wurzel_zurueck(marke);
                r
            }
            WasmOp::CustomSections => {
                let m = self.wasm_module_arg(args)?;
                let name = self.zu_text(self.arg(args, 1))?;
                let marke = self.wurzel_merke();
                let mut out = Vec::new();
                for c in m.custom.iter().filter(|c| c.name == name) {
                    let b = self.new_array_buffer(c.inhalt.clone())?;
                    self.wurzeln.push(b);
                    out.push(b);
                }
                let r = self.array_of(&out);
                self.wurzel_zurueck(marke);
                r
            }
            WasmOp::Instantiate => self.wasm_instantiate(args),
            WasmOp::Func => {
                let i = self.index_arg(args, 0)?;
                Ok(GVal::Ref(self.wasm_function_object(i)?))
            }
            WasmOp::MemNew => {
                let initial = self.index_arg(args, 0)? as u64;
                let maximum = self.opt_index_arg(args, 1)?;
                let shared = self.zu_bool(self.arg(args, 2));
                let r = narsil_wasm::js::neue_memory(&mut self.wasm_mut().lauf, initial, maximum);
                match r {
                    Ok(i) => {
                        if shared {
                            self.wasm_mut().shared.insert(i);
                        }
                        Ok(GVal::Zahl(i as f64))
                    }
                    Err(_) => Err(self.range_error("WebAssembly.Memory: could not allocate memory")),
                }
            }
            WasmOp::MemGrow => {
                let m = self.index_arg(args, 0)?;
                let delta = self.index_arg(args, 1)? as u64;
                self.wasm_mem_grow(m, delta)
            }
            WasmOp::MemBuffer => {
                let m = self.index_arg(args, 0)?;
                self.wasm_mem_buffer(m)
            }
            WasmOp::TableNew => {
                let kind = self.index_arg(args, 0)?;
                let initial = self.index_arg(args, 1)? as u64;
                let maximum = self.opt_index_arg(args, 2)?;
                let elem = if kind == 5 { Reftyp::Extern } else { Reftyp::Func };
                let t = Valtyp::Ref(elem);
                let value = if self.zu_bool(self.arg(args, 4)) {
                    self.wasm_to_value(self.arg(args, 3), t)?
                } else {
                    self.wasm_default(t)
                };
                let st = self.wasm_mut();
                let idx = match narsil_wasm::js::neue_table(&mut st.lauf, elem, initial, maximum) {
                    Ok(i) => i,
                    Err(_) => return Err(self.range_error("WebAssembly.Table: could not allocate the table")),
                };
                if value.als_ref().is_some() {
                    for i in 0..initial {
                        let _ = narsil_wasm::js::table_set(&mut self.wasm_mut().lauf, idx, i, Some(value));
                    }
                }
                Ok(GVal::Zahl(idx as f64))
            }
            WasmOp::TableGrow => {
                let t = self.index_arg(args, 0)?;
                let delta = self.index_arg(args, 1)? as u64;
                let et = Valtyp::Ref(self.wasm_table_type(t)?);
                let value = if self.zu_bool(self.arg(args, 3)) {
                    self.wasm_to_value(self.arg(args, 2), et)?
                } else {
                    self.wasm_default(et)
                };
                let r = narsil_wasm::js::table_grow(&mut self.wasm_mut().lauf, t, delta, Some(value));
                if r < 0 {
                    return Err(self.range_error("WebAssembly.Table.grow: failed to grow the table"));
                }
                Ok(GVal::Zahl(r as f64))
            }
            WasmOp::TableGet => {
                let t = self.index_arg(args, 0)?;
                let i = self.index_arg(args, 1)? as u64;
                match narsil_wasm::js::table_get(&self.wasm().lauf, t, i) {
                    Ok(Some(w)) => self.wasm_to_js(w),
                    Ok(None) => Ok(GVal::Null),
                    Err(_) => Err(self.range_error("WebAssembly.Table.get: index out of bounds")),
                }
            }
            WasmOp::TableSet => {
                let t = self.index_arg(args, 0)?;
                let i = self.index_arg(args, 1)? as u64;
                let et = Valtyp::Ref(self.wasm_table_type(t)?);
                let len = narsil_wasm::js::table_len(&self.wasm().lauf, t).unwrap_or(0);
                if i >= len {
                    return Err(self.range_error("WebAssembly.Table.set: index out of bounds"));
                }
                let value = if self.zu_bool(self.arg(args, 3)) {
                    self.wasm_to_value(self.arg(args, 2), et)?
                } else {
                    self.wasm_default(et)
                };
                match narsil_wasm::js::table_set(&mut self.wasm_mut().lauf, t, i, Some(value)) {
                    Ok(()) => Ok(GVal::Undefiniert),
                    Err(_) => Err(self.range_error("WebAssembly.Table.set: index out of bounds")),
                }
            }
            WasmOp::TableLength => {
                let t = self.index_arg(args, 0)?;
                let n = narsil_wasm::js::table_len(&self.wasm().lauf, t).unwrap_or(0);
                Ok(GVal::Zahl(n as f64))
            }
            WasmOp::GlobalNew => {
                let code = self.index_arg(args, 0)?;
                let Some(t) = valtype_from_code(code) else {
                    return Err(self.type_error("WebAssembly.Global: unsupported value type"));
                };
                let mutable = self.zu_bool(self.arg(args, 1));
                let value = if self.zu_bool(self.arg(args, 3)) {
                    self.wasm_to_value(self.arg(args, 2), t)?
                } else {
                    self.wasm_default(t)
                };
                match narsil_wasm::js::new_global(&mut self.wasm_mut().lauf, value, mutable) {
                    Ok(i) => Ok(GVal::Zahl(i as f64)),
                    Err(_) => Err(self.type_error("WebAssembly.Global: could not create the global")),
                }
            }
            WasmOp::GlobalGet => {
                let i = self.index_arg(args, 0)?;
                let w = self.wasm().lauf.zelle(i).copied();
                match w {
                    Some(w) => self.wasm_to_js(w),
                    None => Err(self.type_error("WebAssembly.Global: unknown global")),
                }
            }
            WasmOp::GlobalSet => {
                let i = self.index_arg(args, 0)?;
                let (t, mutable) = {
                    let s = &self.wasm().lauf.store;
                    (
                        s.globale_typen.get(i as usize).copied(),
                        s.globale_veraenderbar.get(i as usize).copied().unwrap_or(false),
                    )
                };
                let Some(t) = t else {
                    return Err(self.type_error("WebAssembly.Global: unknown global"));
                };
                if !mutable {
                    return Err(self.type_error("WebAssembly.Global: the global is immutable"));
                }
                let w = self.wasm_to_value(self.arg(args, 1), t)?;
                if let Some(z) = self.wasm_mut().lauf.zelle_mut(i) {
                    *z = w;
                }
                Ok(GVal::Undefiniert)
            }
            WasmOp::TagNew => {
                let werte = self.iter_werte(self.arg(args, 0))?;
                let mut parameter = Vec::with_capacity(werte.len());
                for w in werte {
                    let c = self.zu_zahl(w)? as u32;
                    match valtype_from_code(c) {
                        Some(t) => parameter.push(t),
                        None => return Err(self.type_error("WebAssembly.Tag: unsupported value type")),
                    }
                }
                let st = self.wasm_mut();
                st.lauf.store.tags.push(FuncTyp { parameter, ergebnis: Vec::new() });
                Ok(GVal::Zahl((st.lauf.store.tags.len() - 1) as f64))
            }
        }
    }

    fn wasm_module_arg(&mut self, args: &[GVal]) -> Result<Arc<Modul>, Stoerung> {
        let i = self.index_arg(args, 0)?;
        match self.wasm().modules.get(i as usize) {
            Some(m) => Ok(m.clone()),
            None => Err(self.type_error("WebAssembly: unknown module")),
        }
    }

    fn wasm_table_type(&mut self, t: u32) -> Result<Reftyp, Stoerung> {
        match self.wasm().lauf.store.tabellen.get(t as usize) {
            Some(tab) => Ok(match tab.reftyp() {
                Reftyp::Extern => Reftyp::Extern,
                _ => Reftyp::Func,
            }),
            None => Err(self.type_error("WebAssembly.Table: unknown table")),
        }
    }

    fn wasm_mem_buffer(&mut self, m: u32) -> Result<GVal, Stoerung> {
        if let Some(Some((b, _))) = self.wasm().buffers.get(m as usize) {
            return Ok(GVal::Ref(*b));
        }
        let Some(bytes) = self.wasm_mut().lauf.store.speicher.get_mut(m as usize).map(|s| s.take_bytes()) else {
            return Err(self.type_error("WebAssembly.Memory: unknown memory"));
        };
        let len = bytes.len();
        let shared = self.wasm().shared.contains(&m);
        let b = self.neuer_bytepuffer(0, shared)?;
        if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(b) {
            *daten = bytes;
        }
        let st = self.wasm_mut();
        if st.buffers.len() <= m as usize {
            st.buffers.resize(m as usize + 1, None);
        }
        st.buffers[m as usize] = Some((b, len));
        st.live.push(m);
        Ok(GVal::Ref(b))
    }

    fn wasm_mem_grow(&mut self, m: u32, delta: u64) -> Result<GVal, Stoerung> {
        // The bytes may sit in the buffer: take them back first.
        let buf = self.wasm().buffers.get(m as usize).copied().flatten();
        if let Some((b, _)) = buf {
            let bytes = match self.heap.holen_mut(b) {
                HObj::BytePuffer { daten, .. } => std::mem::take(daten),
                _ => Vec::new(),
            };
            if let Some(s) = self.wasm_mut().lauf.store.speicher.get_mut(m as usize) {
                s.put_bytes(bytes);
            }
        }
        let r = narsil_wasm::js::memory_grow(&mut self.wasm_mut().lauf, m, delta);
        if let Some((b, len)) = buf {
            let now = self.wasm().lauf.store.speicher.get(m as usize).map_or(0, |s| s.byte_len());
            if r >= 0 {
                // "refresh the memory buffer": the old buffer is detached,
                // even for a grow by 0.
                self.detach_buffer(b);
                let st = self.wasm_mut();
                st.buffers[m as usize] = None;
                st.live.retain(|&x| x != m);
            } else {
                // Failed: the bytes go back into the same buffer.
                let _ = (len, now);
                let bytes = self.wasm_mut().lauf.store.speicher.get_mut(m as usize).map(|s| s.take_bytes());
                if let (Some(bytes), HObj::BytePuffer { daten, .. }) = (bytes, self.heap.holen_mut(b)) {
                    *daten = bytes;
                }
            }
        }
        if r < 0 {
            return Err(self.range_error("WebAssembly.Memory.grow: failed to grow the memory"));
        }
        Ok(GVal::Zahl(r as f64))
    }

    /// `N.instantiate(module, values, isObject)`: link and instantiate,
    /// returning the exports as `[name, kind, store index, …]`.
    fn wasm_instantiate(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let m = self.wasm_module_arg(args)?;
        let vals = self.iter_werte(self.arg(args, 1))?;
        let objs = self.iter_werte(self.arg(args, 2))?;
        let order = import_order(&m);
        let mut tab = ImportTabelle::neu();
        for (i, a) in order.iter().enumerate() {
            let (modul, feld) = import_name(&m, a).unwrap_or_default();
            let v = vals.get(i).copied().unwrap_or(GVal::Undefiniert);
            let is_obj = objs.get(i).is_some_and(|o| self.zu_bool(*o));
            let label = format!("import {modul}.{feld}");
            let w = match a {
                Art::Func(fi) => {
                    if let Some(idx) = self.wasm_function_index(v) {
                        ImportWert::Funktion(idx)
                    } else {
                        let Some(typ) = m.signatur(*fi).cloned() else {
                            return Err(self.wasm_error(ErrKind::Link, &label));
                        };
                        let st = self.wasm_mut();
                        let slot = st.imports.len() as u32;
                        st.imports.push((v, Arc::new(typ.clone())));
                        let f: FremdFn = Arc::new(move |lauf: &mut Lauf, host: &mut dyn narsil_wasm::lauf::Host, args: &[Wert]| {
                            host.call_import(lauf, slot, args)
                        });
                        let idx = st.lauf.neue_funktion(typ, f);
                        ImportWert::Funktion(idx)
                    }
                }
                Art::Tabelle(_) => ImportWert::Tabelle(self.zu_zahl(v)? as u32),
                Art::Speicher(_) => ImportWert::Speicher(self.zu_zahl(v)? as u32),
                Art::Tag(_) => ImportWert::Tag(self.zu_zahl(v)? as u32),
                Art::Global(gi) => {
                    if is_obj {
                        ImportWert::Global(self.zu_zahl(v)? as u32)
                    } else {
                        let Some(gl) = m.globale.get(*gi as usize).cloned() else {
                            return Err(self.wasm_error(ErrKind::Link, &label));
                        };
                        if gl.veraenderbar {
                            return Err(self.wasm_error(
                                ErrKind::Link,
                                &format!("{label}: a mutable global needs a WebAssembly.Global"),
                            ));
                        }
                        let ok = match gl.typ {
                            Valtyp::I64 => matches!(v, GVal::Ganz(_)),
                            Valtyp::I32 | Valtyp::F32 | Valtyp::F64 => matches!(v, GVal::Zahl(_)),
                            Valtyp::Ref(_) => true,
                        };
                        if !ok {
                            return Err(self.wasm_error(
                                ErrKind::Link,
                                &format!("{label}: the value does not match the global type"),
                            ));
                        }
                        let w = self.wasm_to_value(v, gl.typ)?;
                        let idx = match narsil_wasm::js::new_global(&mut self.wasm_mut().lauf, w, false) {
                            Ok(i) => i,
                            Err(e) => return Err(self.wasm_fehler_js(e)),
                        };
                        ImportWert::Global(idx)
                    }
                }
            };
            tab = tab.mit(&modul, &feld, w);
        }
        let r = self.wasm_with_lauf(|e, lauf| narsil_wasm::lauf::instanziieren_host(lauf, e, &m, &tab));
        let inst = match r {
            Ok(i) => i,
            Err(e) => return Err(self.wasm_fehler_js(e)),
        };
        {
            let st = self.wasm_mut();
            for (lokal, store) in inst.funktionen.iter().enumerate() {
                st.names.entry(*store).or_insert(lokal as u32);
            }
        }
        let marke = self.wurzel_merke();
        let mut out = Vec::new();
        for e in &m.exporte {
            let idx = match e.art {
                Art::Func(l) => inst.funktions_index(l),
                Art::Tabelle(l) => inst.tabellen_index(l),
                Art::Speicher(l) => inst.speicher_index_von(l),
                // A global's identity is its cell: a re-exported import is
                // the same `WebAssembly.Global` object.
                Art::Global(l) => inst
                    .globale_index
                    .get(l as usize)
                    .map(|g| self.wasm().lauf.store.globale_zelle.get(*g as usize).copied().unwrap_or(*g)),
                Art::Tag(l) => inst.tags.get(l as usize).copied(),
            };
            let Some(idx) = idx else { continue };
            let n = self.text_wert(&e.name).map_err(Stoerung::Fehler)?;
            self.wurzeln.push(n);
            out.push(n);
            out.push(GVal::Zahl(art_code(&e.art) as f64));
            out.push(GVal::Zahl(idx as f64));
        }
        let r = self.array_of(&out);
        self.wurzel_zurueck(marke);
        r
    }
}

impl narsil_wasm::lauf::Host for Engine {
    fn call_import(&mut self, lauf: &mut Lauf, slot: u32, args: &[Wert]) -> Result<Vec<Wert>, Fehler> {
        // The live store goes back into the engine while JS runs: JS may
        // read memory buffers and call wasm again.
        self.wasm_swap_out(lauf);
        std::mem::swap(&mut self.wasm_mut().lauf, lauf);
        let r = self.wasm_call_js(slot, args);
        std::mem::swap(&mut self.wasm_mut().lauf, lauf);
        self.wasm_swap_in(lauf);
        match r {
            Ok(v) => Ok(v),
            Err(s) => {
                let text = match &s {
                    Stoerung::Wurf(_) => "JS exception".to_string(),
                    Stoerung::Fehler(e) => format!("{e:?}"),
                };
                self.wasm_mut().pending = Some(s);
                Err(Fehler::trap(T_HOST, text))
            }
        }
    }
}

/// Keep `JsFehler` in scope for the `Stoerung` variants used above.
#[allow(dead_code)]
fn _fehler_typ(_: JsFehler) {}
