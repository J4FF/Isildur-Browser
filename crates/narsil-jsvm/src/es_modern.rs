//! Release 15.10.2026: ECMAScript 2022-2026 conformance work (agent I).
//!
//! Engine fixes that need a helper of their own, and the built-ins that
//! were missing. Every native of this file is one `EsOp`, dispatched through
//! the single `Nativ::Es` variant, so the engine's big `Nativ` enum and its
//! dispatch grow by one line each.

use std::collections::HashMap;

use crate::heap::{Eigenschaft, GVal, HObj, HeapRef, Sperre};
use crate::interp::{Engine, Stoerung};

/// At most this many cached template objects (M4: code built by `eval` in a
/// loop makes new sites; beyond the cap a site gets a fresh object per call).
const MAX_VORLAGEN: usize = 20_000;

/// State of the built-ins of this file (one field of `Engine`).
#[derive(Default)]
pub(crate) struct EsZustand {
    /// Tagged template objects per site (function index, pc).
    pub(crate) vorlagen: HashMap<(usize, usize), HeapRef>,
    /// `Iterator`, %IteratorHelperPrototype%, %WrapForValidIteratorPrototype%.
    pub(crate) iterator_ctor: Option<HeapRef>,
    pub(crate) helfer_proto: Option<HeapRef>,
    pub(crate) huelle_proto: Option<HeapRef>,
    /// The compiled `Array.fromAsync` body.
    pub(crate) from_async: Option<HeapRef>,
    /// %ArrayIteratorPrototype%, %MapIteratorPrototype%, %SetIteratorPrototype%,
    /// %RegExpStringIteratorPrototype%.
    pub(crate) feld_iter_proto: Option<HeapRef>,
    pub(crate) karte_iter_proto: Option<HeapRef>,
    pub(crate) menge_iter_proto: Option<HeapRef>,
    pub(crate) regex_iter_proto: Option<HeapRef>,
    /// %StringIteratorPrototype%.
    pub(crate) text_iter_proto: Option<HeapRef>,
    /// `Atomics.waitAsync`: the element being registered and the waiters
    /// (buffer, byte index, waiter object).
    pub(crate) warte_ort: Option<(HeapRef, usize)>,
    pub(crate) warter: Vec<(HeapRef, usize, HeapRef)>,
    /// Lazily compiled JavaScript helpers.
    pub(crate) glue: Vec<Option<HeapRef>>,
    /// The original %RegExp% (SpeciesConstructor default, `RegExp(re)`).
    pub(crate) regex_ctor: Option<HeapRef>,
    /// The original %Array% and the shared `get [Symbol.species]`.
    pub(crate) array_ctor: Option<HeapRef>,
    pub(crate) spezies_getter: Option<HeapRef>,
    /// %TypedArray% and %Promise%.
    pub(crate) ta_ctor: Option<HeapRef>,
    pub(crate) promise_ctor: Option<HeapRef>,
    /// %Array.prototype.values% (the own `@@iterator` of arguments objects).
    pub(crate) feld_werte_fn: Option<HeapRef>,
    /// %AsyncIteratorPrototype% and %AsyncGeneratorPrototype%.
    pub(crate) async_iter_proto: Option<HeapRef>,
    pub(crate) async_gen_proto: Option<HeapRef>,
}

impl EsZustand {
    /// Every value kept alive here (GC roots).
    pub(crate) fn roots(&self, out: &mut Vec<GVal>) {
        out.extend(self.vorlagen.values().map(|r| GVal::Ref(*r)));
        for r in [
            self.iterator_ctor,
            self.helfer_proto,
            self.huelle_proto,
            self.from_async,
            self.feld_iter_proto,
            self.karte_iter_proto,
            self.menge_iter_proto,
            self.regex_iter_proto,
            self.text_iter_proto,
            self.regex_ctor,
            self.array_ctor,
            self.spezies_getter,
            self.async_iter_proto,
            self.async_gen_proto,
            self.feld_werte_fn,
            self.ta_ctor,
            self.promise_ctor,
        ]
        .into_iter()
        .flatten()
        {
            out.push(GVal::Ref(r));
        }
        for (p, _, w) in &self.warter {
            out.push(GVal::Ref(*p));
            out.push(GVal::Ref(*w));
        }
        out.extend(self.glue.iter().flatten().map(|r| GVal::Ref(*r)));
    }
}

impl Engine {
    /// Release 15.10.2026: GetTemplateObject (ECMA-262 13.2.8.4): a frozen
    /// array of the cooked strings with a frozen `raw` array, the same object
    /// for every evaluation of one site. `werte` holds `n` cooked values and
    /// `n` raw strings.
    pub(crate) fn vorlagen_objekt(&mut self, stelle: (usize, usize), werte: &[GVal]) -> Result<GVal, Stoerung> {
        if let Some(r) = self.es.vorlagen.get(&stelle).copied()
            && self.heap.lebt(r)
        {
            return Ok(GVal::Ref(r));
        }
        let marke = self.wurzel_merke();
        self.wurzeln.extend_from_slice(werte);
        let n = werte.len() / 2;
        let r: Result<HeapRef, Stoerung> = (|| {
            let gekocht = self.feld_aus_werten(&werte[..n])?;
            self.wurzeln.push(GVal::Ref(gekocht));
            let roh = self.feld_aus_werten(&werte[n..])?;
            self.wurzeln.push(GVal::Ref(roh));
            if let HObj::Feld { rest, .. } = self.heap.holen_mut(gekocht) {
                rest.insert(
                    "raw".to_string(),
                    Eigenschaft {
                        wert: GVal::Ref(roh),
                        getter: None,
                        setter: None,
                        aufzählbar: false,
                        beschreibbar: false,
                        konfigurierbar: false,
                    },
                );
            }
            self.n_objekt_sperren(&[GVal::Ref(roh)], Sperre::Gefroren)?;
            self.n_objekt_sperren(&[GVal::Ref(gekocht)], Sperre::Gefroren)?;
            Ok(gekocht)
        })();
        self.wurzel_zurueck(marke);
        let r = r?;
        if self.es.vorlagen.len() < MAX_VORLAGEN {
            self.es.vorlagen.insert(stelle, r);
        }
        Ok(GVal::Ref(r))
    }

    /// Release 15.10.2026: the `indices` array of a match with the `d` flag
    /// (ECMA-262 22.2.7.8 MakeMatchIndicesIndexPairArray): `[start, end]`
    /// per group or undefined, with `groups` (null prototype) when the
    /// pattern has named groups. MEASURED before: `/a(b)/d.exec('ab')
    /// .indices` was undefined (node 22: `[[0,2],[1,2]]`).
    pub(crate) fn treffer_indices(
        &mut self,
        erg: GVal,
        t: &crate::regexmini::Treffer,
        namen: &[String],
    ) -> Result<(), Stoerung> {
        let marke = self.wurzel_merke();
        let r = (|| {
            let mut paare = Vec::with_capacity(t.gruppen.len());
            let ganz = std::iter::once(Some((t.anfang, t.ende))).chain(t.gruppen.iter().skip(1).copied());
            for g in ganz {
                let v = match g {
                    Some((a, b)) => {
                        let p = self.feld_aus_werten(&[GVal::Zahl(a as f64), GVal::Zahl(b as f64)])?;
                        GVal::Ref(p)
                    }
                    None => GVal::Undefiniert,
                };
                self.wurzeln.push(v);
                paare.push(v);
            }
            let feld = self.feld_aus_werten(&paare)?;
            self.wurzeln.push(GVal::Ref(feld));
            let gruppen = if namen.iter().all(|n| n.is_empty()) {
                GVal::Undefiniert
            } else {
                let g = self.neues_objekt(None).map_err(Stoerung::Fehler)?;
                self.wurzeln.push(GVal::Ref(g));
                for (i, n) in namen.iter().enumerate() {
                    if n.is_empty() {
                        continue;
                    }
                    let v = paare.get(i + 1).copied().unwrap_or(GVal::Undefiniert);
                    if matches!(v, GVal::Undefiniert) && self.eigene_lese(g, n).is_some() {
                        continue;
                    }
                    self.setze(GVal::Ref(g), n, v)?;
                }
                GVal::Ref(g)
            };
            self.setze(GVal::Ref(feld), "groups", gruppen)?;
            self.setze(erg, "indices", GVal::Ref(feld))
        })();
        self.wurzel_zurueck(marke);
        r
    }

    /// Release 15.10.2026: Array.prototype mutators on a sealed, frozen or
    /// non-extensible array throw a TypeError like node 22 (ECMA-262: the
    /// `Set`/`DeletePropertyOrThrow`/`CreateDataPropertyOrThrow` steps fail).
    /// MEASURED before: `Object.freeze([1]).push(2)` returned 2 and changed
    /// the frozen array.
    ///
    /// `art`: 0 adds `n` elements (push, unshift), 1 removes one (pop,
    /// shift), 2 writes in place (sort, reverse, fill, copyWithin; `n` is the
    /// minimum length that writes anything), 3 is splice with `n` arguments.
    pub(crate) fn feld_sperre_pruefen(&mut self, this: GVal, art: u8, n: usize) -> Result<(), Stoerung> {
        let GVal::Ref(r) = this else { return Ok(()) };
        let (sperre, len) = match self.heap.holen(r) {
            HObj::Feld { sperre: Sperre::Offen, .. } => return Ok(()),
            HObj::Feld { sperre, elemente, .. } => (*sperre, elemente.len()),
            _ => return Ok(()),
        };
        // node 22 messages, measured:
        //   Object.freeze([1]).push(2) -> Cannot add property 1, object is not extensible
        //   Object.freeze([1]).push()  -> Cannot assign to read only property 'length' of object '[object Array]'
        //   Object.seal([1]).pop()     -> Cannot delete property '0' of [object Array]
        //   Object.freeze([2,1]).sort() -> Cannot assign to read only property '0' of object '[object Array]'
        let hinzu = format!("Cannot add property {len}, object is not extensible");
        let laenge = "Cannot assign to read only property 'length' of object '[object Array]'".to_string();
        let loeschen = format!("Cannot delete property '{}' of [object Array]", len.saturating_sub(1));
        let schreiben = "Cannot assign to read only property '0' of object '[object Array]'".to_string();
        let gefroren = sperre == Sperre::Gefroren;
        let versiegelt = sperre == Sperre::Versiegelt || gefroren;
        let meldung = match art {
            0 if n > 0 => Some(hinzu),
            0 if gefroren => Some(laenge),
            1 if len > 0 && versiegelt => Some(loeschen),
            1 if gefroren => Some(laenge),
            2 if gefroren && len >= n => Some(schreiben),
            3 if gefroren => Some(schreiben),
            3 if versiegelt && n > 0 => Some(loeschen),
            3 if n > 2 => Some(hinzu),
            _ => None,
        };
        match meldung {
            Some(m) => Err(Stoerung::Wurf(self.fehler_wert("TypeError", &m))),
            None => Ok(()),
        }
    }

    /// Release 15.10.2026: the full own property (value or accessor pair and
    /// attributes) of the ordinary object kinds, `None` when absent or when
    /// the kind keeps its properties elsewhere (functions' `name`/`length`,
    /// typed array elements, host objects). Used by `defineProperty` to
    /// validate a redefinition (ECMA-262 10.1.6.3).
    pub(crate) fn eigene_eigenschaft(&self, r: HeapRef, k: &str) -> Option<Eigenschaft> {
        match self.heap.holen(r) {
            HObj::Feld { elemente, rest, sperre, luecken, .. } => {
                if let Some(e) = rest.get(k) {
                    return Some(e.clone());
                }
                let i = crate::ops::index_aus_schluessel(k)?;
                if i >= elemente.len() || luecken.as_ref().is_some_and(|m| m.contains(&i)) {
                    return None;
                }
                Some(Eigenschaft {
                    wert: elemente[i],
                    getter: None,
                    setter: None,
                    aufzählbar: true,
                    beschreibbar: *sperre != Sperre::Gefroren,
                    konfigurierbar: *sperre != Sperre::Gefroren && *sperre != Sperre::Versiegelt,
                })
            }
            HObj::Gegenstand { eigenschaften, .. }
            | HObj::Verpackt { eigenschaften, .. }
            | HObj::Fehler { eigenschaften, .. }
            | HObj::Datum { eigenschaften, .. }
            | HObj::Regex { eigenschaften, .. }
            | HObj::Karte { eigenschaften, .. }
            | HObj::Menge { eigenschaften, .. }
            | HObj::Versprechen { eigenschaften, .. }
            | HObj::Funktion { eigenschaften, .. }
            | HObj::Nativ { eigenschaften, .. }
            | HObj::Generator { eigenschaften, .. }
            | HObj::BytePuffer { eigenschaften, .. }
            | HObj::TypedArray { eigenschaften, .. } => eigenschaften.get(k).cloned(),
            _ => None,
        }
    }

    /// The own property map of the ordinary object kinds (arrays: the map
    /// beside the dense elements).
    pub(crate) fn eigenschaften_karte_mut(&mut self, r: HeapRef) -> Option<&mut crate::heap::Karte> {
        match self.heap.holen_mut(r) {
            HObj::Feld { rest, .. } => Some(rest),
            HObj::Gegenstand { eigenschaften, .. }
            | HObj::Verpackt { eigenschaften, .. }
            | HObj::Fehler { eigenschaften, .. }
            | HObj::Datum { eigenschaften, .. }
            | HObj::Regex { eigenschaften, .. }
            | HObj::Karte { eigenschaften, .. }
            | HObj::Menge { eigenschaften, .. }
            | HObj::Versprechen { eigenschaften, .. }
            | HObj::Funktion { eigenschaften, .. }
            | HObj::Nativ { eigenschaften, .. }
            | HObj::Generator { eigenschaften, .. }
            | HObj::BytePuffer { eigenschaften, .. }
            | HObj::TypedArray { eigenschaften, .. } => Some(eigenschaften),
            _ => None,
        }
    }

    /// ECMA-262 7.2.10 SameValue (NaN equals NaN, +0 differs from -0).
    pub(crate) fn same_value(&self, a: GVal, b: GVal) -> bool {
        match (a, b) {
            (GVal::Zahl(x), GVal::Zahl(y)) => {
                if x.is_nan() && y.is_nan() {
                    true
                } else {
                    x == y && x.is_sign_negative() == y.is_sign_negative()
                }
            }
            _ => self.gleich_strikt(a, b),
        }
    }

    /// Release 15.10.2026: the checks of ToPropertyDescriptor (6.2.6.5) and
    /// ValidateAndApplyPropertyDescriptor (10.1.6.3) that `defineProperty`
    /// skipped. Returns the current accessor pair when the redefinition keeps
    /// an accessor, so a partial accessor descriptor keeps its other half.
    /// MEASURED before (node 22 throws a TypeError in each case):
    /// `defineProperty({}, 'x', {get: 5})` and `{get(){}, value: 1}` were
    /// accepted, a non-configurable property took a new value, and
    /// `{enumerable: true}` on an accessor turned it into `undefined` data.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn deskriptor_pruefen(
        &mut self,
        r: HeapRef,
        k: &str,
        d: HeapRef,
        wert: Option<GVal>,
        get: GVal,
        set: GVal,
    ) -> Result<Option<Eigenschaft>, Stoerung> {
        let hat_get = self.in_operator("get", GVal::Ref(d));
        let hat_set = self.in_operator("set", GVal::Ref(d));
        let hat_w = self.in_operator("writable", GVal::Ref(d));
        if hat_get && !matches!(get, GVal::Undefiniert) && !self.ist_aufrufbar(get) {
            let t = self.zu_text(get).unwrap_or_default();
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", &format!("Getter must be a function: {t}"))));
        }
        if hat_set && !matches!(set, GVal::Undefiniert) && !self.ist_aufrufbar(set) {
            let t = self.zu_text(set).unwrap_or_default();
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", &format!("Setter must be a function: {t}"))));
        }
        let accessor = hat_get || hat_set;
        if accessor && (wert.is_some() || hat_w) {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                "Invalid property descriptor. Cannot both specify accessors and a value or writable attribute",
            )));
        }
        let Some(cur) = self.eigene_eigenschaft(r, k) else { return Ok(None) };
        let cur_accessor = cur.getter.is_some() || cur.setter.is_some();
        if !cur.konfigurierbar {
            let conf = self.eigene_lese(d, "configurable").map(|v| self.zu_bool(v));
            let enumb = self.eigene_lese(d, "enumerable").map(|v| self.zu_bool(v));
            let mut verboten = conf == Some(true) || enumb.is_some_and(|e| e != cur.aufzählbar);
            if accessor && !cur_accessor || (wert.is_some() || hat_w) && cur_accessor {
                verboten = true;
            } else if cur_accessor {
                let g = if let GVal::Ref(g) = get { Some(g) } else { None };
                let s = if let GVal::Ref(s) = set { Some(s) } else { None };
                verboten |= hat_get && g != cur.getter || hat_set && s != cur.setter;
            } else if !cur.beschreibbar {
                let w = self.eigene_lese(d, "writable").map(|v| self.zu_bool(v));
                verboten |= w == Some(true) || wert.is_some_and(|v| !self.same_value(v, cur.wert));
            }
            if verboten {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", &format!("Cannot redefine property: {}", self.schluessel_anzeige(k))),
                ));
            }
        }
        Ok(cur_accessor.then_some(cur))
    }

    /// The property key as node prints it in messages (`Symbol(x)` for
    /// symbol keys, the text otherwise).
    pub(crate) fn schluessel_anzeige(&self, k: &str) -> String {
        match k.rfind(")@") {
            Some(i) if k.starts_with("Symbol(") => k[..i + 1].to_string(),
            _ => k.to_string(),
        }
    }

    /// Release 15.10.2026: calling a class constructor without `new` is a
    /// TypeError (ECMA-262 10.2.1 [[Call]] step 2). MEASURED before:
    /// `class A {}; A()` returned undefined, `A.call({})` ran the body.
    pub(crate) fn klassen_aufruf_fehler(&mut self, fr: HeapRef) -> Stoerung {
        let name = match self.heap.holen(fr) {
            HObj::Funktion { name, .. } => name.clone(),
            _ => String::new(),
        };
        // node 22: "Class constructor A cannot be invoked without 'new'",
        // anonymous: "Class constructors cannot be invoked without 'new'".
        let m = if name.is_empty() {
            "Class constructors cannot be invoked without 'new'".to_string()
        } else {
            format!("Class constructor {name} cannot be invoked without 'new'")
        };
        Stoerung::Wurf(self.fehler_wert("TypeError", &m))
    }
}

impl Engine {
    /// Release 15.10.2026: store the accessor `k` with exactly these halves
    /// (an explicit `get: undefined` removes the getter) and attributes.
    ///
    /// An accessor whose get AND set are undefined has no form of its own in
    /// the heap (no getter and no setter means data), so it becomes a
    /// NON-writable `undefined`: reading gives undefined and writes are
    /// refused, as for the accessor (ECMA-262 10.1.9.2). MEASURED:
    /// TypeScript's `__decorate` on youtube.com defines
    /// `{get: undefined, set: undefined}` for ~870 Polymer properties; stored
    /// as a WRITABLE `undefined` on the prototype, instance writes shadowed
    /// the later Polymer accessors and constructing `ytd-app` failed with
    /// "kein Zugriff auf Undefiniert (Eigenschaft isSearchPage)".
    pub(crate) fn accessor_genau_setzen(
        &mut self,
        r: HeapRef,
        k: &str,
        getter: Option<HeapRef>,
        setter: Option<HeapRef>,
        aufzaehlbar: bool,
        konfigurierbar: bool,
    ) -> Result<(), Stoerung> {
        if getter.is_none() && setter.is_none() && self.eigene_lese(r, k).is_none() {
            self.eigenschaft_mit_attributen(r, k, GVal::Undefiniert, aufzaehlbar, false)?;
        } else {
            self.getter_setter_definieren_mit(r, k, getter, setter, aufzaehlbar, konfigurierbar)
                .map_err(Stoerung::Fehler)?;
        }
        if let Some(map) = self.eigenschaften_karte_mut(r)
            && let Some(e) = map.get_mut(k)
        {
            e.getter = getter;
            e.setter = setter;
            e.aufzählbar = aufzaehlbar;
            e.konfigurierbar = konfigurierbar;
            if getter.is_none() && setter.is_none() {
                e.wert = GVal::Undefiniert;
                e.beschreibbar = false;
            }
        }
        Ok(())
    }
}

// ===========================================================================
// Release 15.10.2026: the ES2023-2025 library (natives of `Nativ::Es`).
// ===========================================================================

/// One native of this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EsOp {
    // Array
    FeldLetzterIndex,
    FeldVonAsync,
    // String
    TextWohlgeformt,
    TextZuWohlgeformt,
    // Promise, RegExp, Error
    VersprechenVersuch,
    RegexMaskieren,
    FehlerStapelFangen,
    FehlerIstFehler,
    // Set methods (ES2025)
    MengeVereinigung,
    MengeSchnitt,
    MengeDifferenz,
    MengeSymDifferenz,
    MengeTeilVon,
    MengeObermengeVon,
    MengeDisjunkt,
    // Iterator helpers (ES2025)
    IterCtor,
    IterVon,
    IterMap,
    IterFilter,
    IterTake,
    IterDrop,
    IterFlatMap,
    IterReduce,
    IterToArray,
    IterForEach,
    IterSome,
    IterEvery,
    IterFind,
    HelferNext,
    HelferReturn,
    HuelleNext,
    HuelleReturn,
    IterTagLesen,
    IterTagSchreiben,
    IterCtorLesen,
    IterCtorSchreiben,
    // Binary data (ES2025/2026)
    TaCtorFloat16,
    DvGetFloat16,
    DvSetFloat16,
    MatheF16Runden,
    AtomAdd,
    AtomSub,
    AtomAnd,
    AtomOr,
    AtomXor,
    AtomXchg,
    AtomStore,
    AtomLoad,
    AtomCmpXchg,
    AtomIsLockFree,
    AtomNotify,
    AtomWait,
    AtomWaitAsync,
    AtomPause,
    AtomRegister,
    U8FromBase64,
    U8FromHex,
    U8ToBase64,
    U8ToHex,
    U8SetFromBase64,
    U8SetFromHex,
    PufferResizable,
    PufferMax,
    PufferResize,
    SabGrow,
    PufferTransferEs,
    PufferTransferFest,
    // Wrapper objects
    BigIntWertVon,
    BigIntZuText,
    SymWertVon,
    SymZuText,
    SymBeschreibungEs,
    // RegExp symbol protocol (es_regex.rs)
    RegexSymMatch,
    RegexSymSearch,
    RegexSymReplace,
    RegexSymSplit,
    RegexSymMatchAll,
    RegexExecIntern,
    SpeziesLesen,
    // Async iteration (asyncgen.rs prelude)
    AsyncIterSelbst,
    AsyncGenNext,
    AsyncGenReturn,
    AsyncGenThrow,
    // WeakRef / FinalizationRegistry (ES2021)
    WeakRefCtor,
    WeakRefDeref,
    FinRegCtor,
    FinRegRegister,
    FinRegUnregister,
    // %TypedArray%
    TaAbstrakt,
    TaTagLesen,
    // JSON.rawJSON / isRawJSON (ES2026), Math.sumPrecise (ES2026)
    JsonRoh,
    JsonIstRoh,
    MatheSummeGenau,
}

impl EsOp {
    /// The `length` of the function (ECMA-262).
    pub(crate) fn laenge(self) -> u32 {
        use EsOp::*;
        match self {
            FeldLetzterIndex | FeldVonAsync => 1,
            TextWohlgeformt | TextZuWohlgeformt => 0,
            VersprechenVersuch | RegexMaskieren | FehlerIstFehler => 1,
            FehlerStapelFangen => 2,
            MengeVereinigung | MengeSchnitt | MengeDifferenz | MengeSymDifferenz | MengeTeilVon
            | MengeObermengeVon | MengeDisjunkt => 1,
            IterCtor => 0,
            IterVon | IterMap | IterFilter | IterTake | IterDrop | IterFlatMap | IterReduce | IterForEach
            | IterSome | IterEvery | IterFind => 1,
            IterToArray | HelferNext | HelferReturn | HuelleNext | HuelleReturn | IterTagLesen
            | IterCtorLesen => 0,
            IterTagSchreiben | IterCtorSchreiben => 1,
            TaCtorFloat16 => 3,
            DvGetFloat16 | MatheF16Runden => 1,
            DvSetFloat16 => 2,
            AtomAdd | AtomSub | AtomAnd | AtomOr | AtomXor | AtomXchg | AtomStore | AtomNotify => 3,
            AtomCmpXchg | AtomWait | AtomWaitAsync => 4,
            AtomLoad => 2,
            AtomIsLockFree | AtomRegister => 1,
            AtomPause => 0,
            U8FromBase64 | U8FromHex | U8SetFromBase64 | U8SetFromHex => 1,
            U8ToBase64 | U8ToHex => 0,
            PufferResizable | PufferMax | PufferTransferEs | PufferTransferFest => 0,
            PufferResize | SabGrow => 1,
            BigIntWertVon | BigIntZuText | SymWertVon | SymZuText | SymBeschreibungEs => 0,
            RegexSymMatch | RegexSymSearch | RegexSymMatchAll => 1,
            RegexSymReplace | RegexSymSplit | RegexExecIntern => 2,
            SpeziesLesen | AsyncIterSelbst => 0,
            AsyncGenNext | AsyncGenReturn | AsyncGenThrow => 1,
            WeakRefCtor | FinRegCtor | FinRegUnregister => 1,
            WeakRefDeref => 0,
            FinRegRegister => 2,
            JsonRoh | JsonIstRoh | MatheSummeGenau => 1,
            TaAbstrakt | TaTagLesen => 0,
        }
    }
}

/// The [[IsRawJSON]] slot of a JSON.rawJSON object (read by JSON.stringify).
pub(crate) const H_ROH_JSON: &str = "#es_rawjson";

/// Hidden state keys (private names are not listed by reflection).
const H_ITER: &str = "#es_iter";
const H_NEXT: &str = "#es_next";
const H_ART: &str = "#es_art";
const H_FN: &str = "#es_fn";
const H_N: &str = "#es_n";
const H_STATUS: &str = "#es_status";
const H_INNEN: &str = "#es_innen";
const H_INNEN_NEXT: &str = "#es_innen_next";

/// The `Array.fromAsync` body (ECMA-262 2026 23.1.2.2), run as JavaScript:
/// it is an async function and awaits every value. The native wrapper passes
/// the receiver and whether it is a constructor.
const FELD_VON_ASYNC: &str = r#"(async function fromAsync(items, mapfn, thisArg, C, isCtor) {
  'use strict';
  var mapping = mapfn !== undefined;
  if (mapping && typeof mapfn !== 'function') throw new TypeError('Array.fromAsync: ' + String(mapfn) + ' is not a function');
  var put = function (A, k, v) { Object.defineProperty(A, k, { value: v, writable: true, enumerable: true, configurable: true }); };
  if (items == null) throw new TypeError('Array.fromAsync requires an array-like or iterable object');
  var ai = items[Symbol.asyncIterator];
  var si = ai == null ? items[Symbol.iterator] : undefined;
  if (ai != null || si != null) {
    var A = isCtor ? new C() : [];
    var k = 0;
    for await (var v of items) {
      if (k >= 9007199254740991) throw new TypeError('Array.fromAsync: too many elements');
      put(A, k, mapping ? await mapfn.call(thisArg, v, k) : v);
      k++;
    }
    A.length = k;
    return A;
  }
  var o = Object(items);
  var n = Number(o.length);
  var len = n > 0 ? Math.min(Math.floor(n), 9007199254740991) : 0;
  var B = isCtor ? new C(len) : new Array(len);
  for (var i = 0; i < len; i++) {
    var w = await o[i];
    put(B, i, mapping ? await mapfn.call(thisArg, w, i) : w);
  }
  B.length = len;
  return B;
})"#;

impl Engine {
    /// The dispatch of `Nativ::Es`.
    pub(crate) fn es_nativ(
        &mut self,
        op: EsOp,
        fn_ref: HeapRef,
        this: GVal,
        args: &[GVal],
        als_konstruktor: bool,
    ) -> Result<GVal, Stoerung> {
        use EsOp::*;
        let _ = fn_ref;
        match op {
            FeldLetzterIndex => self.es_find_last_index(this, args),
            FeldVonAsync => self.es_from_async(this, args),
            TextWohlgeformt => {
                // Strings are UTF-8 in this engine, so a lone surrogate was
                // already replaced by U+FFFD: every string is well formed.
                self.this_text(this)?;
                Ok(GVal::Bool(true))
            }
            TextZuWohlgeformt => {
                let s = self.this_text(this)?;
                self.text_wert(&s).map_err(Stoerung::Fehler)
            }
            VersprechenVersuch => self.es_promise_try(this, args),
            RegexMaskieren => self.es_regexp_escape(args),
            FehlerStapelFangen => self.es_capture_stack_trace(args),
            FehlerIstFehler => Ok(GVal::Bool(
                matches!(self.arg(args, 0), GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Fehler { .. })),
            )),
            MengeVereinigung => self.es_menge_op(this, args, 0),
            MengeSchnitt => self.es_menge_op(this, args, 1),
            MengeDifferenz => self.es_menge_op(this, args, 2),
            MengeSymDifferenz => self.es_menge_op(this, args, 3),
            MengeTeilVon => self.es_menge_test(this, args, 0),
            MengeObermengeVon => self.es_menge_test(this, args, 1),
            MengeDisjunkt => self.es_menge_test(this, args, 2),
            IterCtor => {
                // ECMA-262 27.1.3.1: abstract; only a subclass constructs it
                // (`super()` passes the derived object as `this`).
                if als_konstruktor && matches!(this, GVal::Ref(_)) {
                    return Ok(this);
                }
                Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Abstract class Iterator not directly constructable")))
            }
            IterVon => self.es_iterator_from(args),
            IterMap => self.es_helfer_bauen(this, args, 0),
            IterFilter => self.es_helfer_bauen(this, args, 1),
            IterTake => self.es_helfer_bauen(this, args, 2),
            IterDrop => self.es_helfer_bauen(this, args, 3),
            IterFlatMap => self.es_helfer_bauen(this, args, 4),
            IterReduce => self.es_iter_reduce(this, args),
            IterToArray => self.es_iter_eifrig(this, args, 0),
            IterForEach => self.es_iter_eifrig(this, args, 1),
            IterSome => self.es_iter_eifrig(this, args, 2),
            IterEvery => self.es_iter_eifrig(this, args, 3),
            IterFind => self.es_iter_eifrig(this, args, 4),
            HelferNext => self.es_helfer_next(this),
            HelferReturn => self.es_helfer_return(this),
            HuelleNext => self.es_huelle(this, false),
            HuelleReturn => self.es_huelle(this, true),
            IterTagLesen => self.text_wert("Iterator").map_err(Stoerung::Fehler),
            IterTagSchreiben => {
                let k = self.bekanntes_symbol_text(crate::builtin::Nativ::SymToStringTag)?;
                self.es_setter_ohne_prototyp(this, &k, self.arg(args, 0))
            }
            IterCtorLesen => Ok(self.es.iterator_ctor.map(GVal::Ref).unwrap_or(GVal::Undefiniert)),
            IterCtorSchreiben => self.es_setter_ohne_prototyp(this, "constructor", self.arg(args, 0)),
            TaCtorFloat16 => {
                if !als_konstruktor {
                    return Err(self.es_typfehler("Constructor Float16Array requires 'new'"));
                }
                self.typedarray_ctor(fn_ref, args, crate::eingebaut_typed::ElementArt::Float16)
            }
            DvGetFloat16 => self.n_dataview_get(this, args, crate::eingebaut_typed::ElementArt::Float16),
            DvSetFloat16 => self.n_dataview_set(this, args, crate::eingebaut_typed::ElementArt::Float16),
            MatheF16Runden => {
                let x = self.zu_zahl(self.arg(args, 0))?;
                Ok(GVal::Zahl(f16_zu_f64(f64_zu_f16(x))))
            }
            AtomRegister => self.es_warter_merken(self.arg(args, 0)),
            RegexSymMatch => self.es_regex_sym(0, this, args),
            RegexSymSearch => self.es_regex_sym(1, this, args),
            RegexSymReplace => self.es_regex_sym(2, this, args),
            RegexSymSplit => self.es_regex_sym(3, this, args),
            RegexSymMatchAll => self.es_regex_sym(4, this, args),
            RegexExecIntern => self.es_regex_exec_intern(args),
            SpeziesLesen | AsyncIterSelbst => Ok(this),
            WeakRefCtor | WeakRefDeref | FinRegCtor | FinRegRegister | FinRegUnregister => {
                self.es_schwach(op, fn_ref, this, args, als_konstruktor)
            }
            TaAbstrakt => Err(self.es_typfehler("Abstract class TypedArray not directly constructable")),
            TaTagLesen => match this {
                GVal::Ref(r) => match self.heap.holen(r) {
                    HObj::TypedArray { art, .. } => {
                        let n = art.name();
                        self.text_wert(n).map_err(Stoerung::Fehler)
                    }
                    _ => Ok(GVal::Undefiniert),
                },
                _ => Ok(GVal::Undefiniert),
            },
            JsonRoh => self.es_json_roh(self.arg(args, 0)),
            JsonIstRoh => Ok(GVal::Bool(
                matches!(self.arg(args, 0), GVal::Ref(r) if self.eigene_lese(r, H_ROH_JSON).is_some()),
            )),
            MatheSummeGenau => self.es_summe_genau(self.arg(args, 0)),
            AsyncGenNext | AsyncGenReturn | AsyncGenThrow => {
                let art = match op {
                    AsyncGenNext => "next",
                    AsyncGenReturn => "return",
                    _ => "throw",
                };
                let f = self.hilfe(crate::asyncgen::HILFE_ASYNC_ANFRAGE)?;
                let a = self.text_wert(art).map_err(Stoerung::Fehler)?;
                self.aufruf_synchron(f, GVal::Undefiniert, &[a, this, self.arg(args, 0)], false)
            }
            BigIntWertVon => Ok(GVal::Ganz(self.es_bigint_wert(this, "valueOf")?)),
            BigIntZuText => {
                let g = self.es_bigint_wert(this, "toString")?;
                self.n_bigint_text(GVal::Ganz(g), args)
            }
            SymWertVon => self.es_symbol_wert(this),
            SymZuText => {
                let s = self.es_symbol_wert(this)?;
                self.n_symbol_text(s, args, false)
            }
            SymBeschreibungEs => {
                let s = self.es_symbol_wert(this)?;
                self.n_symbol_text(s, args, true)
            }
            AtomAdd | AtomSub | AtomAnd | AtomOr | AtomXor | AtomXchg | AtomStore | AtomLoad | AtomCmpXchg
            | AtomIsLockFree | AtomNotify | AtomWait | AtomWaitAsync | AtomPause | U8FromBase64 | U8FromHex
            | U8ToBase64 | U8ToHex | U8SetFromBase64 | U8SetFromHex | PufferResizable | PufferMax | PufferResize
            | SabGrow | PufferTransferEs | PufferTransferFest => self.es_d_ops(op, fn_ref, this, args),
        }
    }

    // ---------------------------------------------------------------
    // Setup
    // ---------------------------------------------------------------

    /// Insert a property (any value, attributes as given) on an ordinary
    /// object, function or array.
    pub(crate) fn es_eigen(&mut self, obj: HeapRef, k: &str, wert: GVal, w: bool, e: bool, c: bool) {
        // Ordinary objects keep their key order in `reihenfolge`.
        if let HObj::Gegenstand { eigenschaften, reihenfolge, .. } = self.heap.holen_mut(obj)
            && !eigenschaften.contains_key(k)
            && !crate::heap::ist_privat(k)
        {
            reihenfolge.push(k.to_string());
        }
        if let Some(map) = self.eigenschaften_karte_mut(obj) {
            map.insert(
                k.to_string(),
                Eigenschaft { wert, getter: None, setter: None, aufzählbar: e, beschreibbar: w, konfigurierbar: c },
            );
        }
    }

    /// A built-in method (`{writable: true, enumerable: false, configurable: true}`).
    pub(crate) fn es_methode(&mut self, obj: HeapRef, name: &str, op: EsOp) -> HeapRef {
        let f = self.nativ_anlegen(name, crate::builtin::Nativ::Es(op), self.proto_funktion);
        self.es_eigen(obj, name, GVal::Ref(f), true, false, true);
        f
    }

    /// The property key of a well-known symbol (`Symbol(Symbol.iterator)`).
    pub(crate) fn bekanntes_symbol_text(&mut self, art: crate::builtin::Nativ) -> Result<String, Stoerung> {
        let s = self.n_symbol_bekannt(GVal::Undefiniert, &[], art)?;
        self.schluessel_text(s)
    }

    /// A global constructor by name (`Array`, `Promise`, …).
    fn es_global(&self, name: &str) -> Option<HeapRef> {
        match self.eigene_lese(self.global_objekt, name) {
            Some(GVal::Ref(r)) => Some(r),
            _ => None,
        }
    }

    /// Release 15.10.2026: register the built-ins of this file (one call at
    /// the end of `aufbauen`).
    pub(crate) fn es_modern_aufbauen(&mut self) -> Result<(), Stoerung> {
        use EsOp::*;
        // Array.prototype.findLastIndex (ES2023). MEASURED before: missing.
        self.es_methode(self.proto_feld, "findLastIndex", FeldLetzterIndex);
        self.es.feld_werte_fn = match self.eigene_lese(self.proto_feld, "values") {
            Some(GVal::Ref(f)) => Some(f),
            _ => None,
        };
        // Array.prototype[@@unscopables] (ECMA-262 23.1.3.41): a
        // null-prototype object. MEASURED before: undefined.
        let u = self.neues_objekt(None).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(u));
        for n in [
            "at", "copyWithin", "entries", "fill", "find", "findIndex", "findLast", "findLastIndex", "flat",
            "flatMap", "includes", "keys", "toReversed", "toSorted", "toSpliced", "values",
        ] {
            self.setze(GVal::Ref(u), n, GVal::Bool(true))?;
        }
        let k = self.bekanntes_symbol_text(crate::builtin::Nativ::SymUnscopables)?;
        self.es_eigen(self.proto_feld, &k, GVal::Ref(u), false, false, true);
        if let Some(a) = self.es_global("Array") {
            self.es_methode(a, "fromAsync", FeldVonAsync);
        }
        // JSON.rawJSON / JSON.isRawJSON (ES2026), Math.sumPrecise (ES2026).
        if let Some(j) = self.es_global("JSON") {
            self.es_methode(j, "rawJSON", JsonRoh);
            self.es_methode(j, "isRawJSON", JsonIstRoh);
        }
        if let Some(m) = self.es_global("Math") {
            self.es_methode(m, "sumPrecise", MatheSummeGenau);
        }
        // String.prototype.isWellFormed/toWellFormed (ES2024).
        self.es_methode(self.proto_text, "isWellFormed", TextWohlgeformt);
        self.es_methode(self.proto_text, "toWellFormed", TextZuWohlgeformt);
        // Promise.try (ES2025), RegExp.escape (ES2025).
        if let Some(p) = self.es_global("Promise") {
            self.es_methode(p, "try", VersprechenVersuch);
        }
        if let Some(r) = self.es_global("RegExp") {
            self.es_methode(r, "escape", RegexMaskieren);
        }
        // Error.captureStackTrace (V8), Error.isError (ES2026),
        // Error.stackTraceLimit (V8: a writable number, 10).
        if let Some(e) = self.es_global("Error") {
            self.es_methode(e, "captureStackTrace", FehlerStapelFangen);
            self.es_methode(e, "isError", FehlerIstFehler);
            self.es_eigen(e, "stackTraceLimit", GVal::Zahl(10.0), true, true, true);
        }
        // Set.prototype methods (ES2025).
        for (n, op) in [
            ("union", MengeVereinigung),
            ("intersection", MengeSchnitt),
            ("difference", MengeDifferenz),
            ("symmetricDifference", MengeSymDifferenz),
            ("isSubsetOf", MengeTeilVon),
            ("isSupersetOf", MengeObermengeVon),
            ("isDisjointFrom", MengeDisjunkt),
        ] {
            self.es_methode(self.proto_menge, n, op);
        }
        self.es_regex_aufbauen()?;
        self.es_typed_aufbauen()?;
        self.es_iterator_aufbauen()?;
        self.es_binaer_aufbauen()?;
        self.es_binaer_d_aufbauen()?;
        self.es_huellen_aufbauen()
    }

    /// `Iterator`, `%Iterator.prototype%` with the helpers, the helper and
    /// wrapper prototypes, and generators below `%Iterator.prototype%`.
    fn es_iterator_aufbauen(&mut self) -> Result<(), Stoerung> {
        use EsOp::*;
        let ip = self.iterator_basis()?;
        // The global constructor (`Iterator.prototype` non-writable,
        // non-configurable, 27.1.3.2.1).
        let ctor = self.nativ_anlegen("Iterator", crate::builtin::Nativ::Es(IterCtor), self.proto_funktion);
        self.es.iterator_ctor = Some(ctor);
        self.nativ_prototyp(ctor, ip);
        self.es_methode(ctor, "from", IterVon);
        // `constructor` and `@@toStringTag` are accessors (27.1.4.1/14).
        let tag = self.bekanntes_symbol_text(crate::builtin::Nativ::SymToStringTag)?;
        for (k, lesen, schreiben) in [
            ("constructor".to_string(), IterCtorLesen, IterCtorSchreiben),
            (tag.clone(), IterTagLesen, IterTagSchreiben),
        ] {
            let g = self.nativ_anlegen(&format!("get {}", self.schluessel_anzeige_kurz(&k)), crate::builtin::Nativ::Es(lesen), self.proto_funktion);
            let s = self.nativ_anlegen(&format!("set {}", self.schluessel_anzeige_kurz(&k)), crate::builtin::Nativ::Es(schreiben), self.proto_funktion);
            self.es_eigen(ip, &k, GVal::Undefiniert, false, false, true);
            if let Some(map) = self.eigenschaften_karte_mut(ip)
                && let Some(e) = map.get_mut(&k)
            {
                e.getter = Some(g);
                e.setter = Some(s);
            }
        }
        for (n, op) in [
            ("map", IterMap),
            ("filter", IterFilter),
            ("take", IterTake),
            ("drop", IterDrop),
            ("flatMap", IterFlatMap),
            ("reduce", IterReduce),
            ("toArray", IterToArray),
            ("forEach", IterForEach),
            ("some", IterSome),
            ("every", IterEvery),
            ("find", IterFind),
        ] {
            self.es_methode(ip, n, op);
        }
        // %IteratorHelperPrototype% (27.1.2.1).
        let hp = self.neues_objekt(Some(ip)).map_err(Stoerung::Fehler)?;
        self.es.helfer_proto = Some(hp);
        self.es_methode(hp, "next", HelferNext);
        self.es_methode(hp, "return", HelferReturn);
        let t = self.text_wert("Iterator Helper").map_err(Stoerung::Fehler)?;
        self.es_eigen(hp, &tag, t, false, false, true);
        // %WrapForValidIteratorPrototype% (27.1.3.2.1.1).
        let wp = self.neues_objekt(Some(ip)).map_err(Stoerung::Fehler)?;
        self.es.huelle_proto = Some(wp);
        self.es_methode(wp, "next", HuelleNext);
        self.es_methode(wp, "return", HuelleReturn);
        // Generators inherit the helpers (%GeneratorPrototype% -> %Iterator.prototype%,
        // 27.5.1). MEASURED before: their chain ended at Object.prototype.
        if let HObj::Gegenstand { proto, .. } = self.heap.holen_mut(self.proto_generator) {
            *proto = Some(ip);
        }
        let g = self.text_wert("Generator").map_err(Stoerung::Fehler)?;
        if self.eigene_lese(self.proto_generator, &tag).is_none() {
            self.es_eigen(self.proto_generator, &tag, g, false, false, true);
        }
        self.global_binden("Iterator", GVal::Ref(ctor));
        Ok(())
    }

    /// Release 15.10.2026: %AsyncIteratorPrototype% (27.1.3, `@@asyncIterator`
    /// returns `this`), %AsyncGeneratorPrototype% (27.6.1: next/return/throw,
    /// `constructor`, `@@toStringTag`), and the `prototype`/`@@toStringTag`
    /// links of %GeneratorFunction.prototype%, %AsyncGeneratorFunction.prototype%
    /// and %AsyncFunction.prototype% (27.3.3, 27.4.3, 27.7.3). MEASURED before
    /// (node 22 in brackets): the async generator prototype was a plain object
    /// created by the prelude, `Object.getPrototypeOf(AGProto)` was
    /// Object.prototype (%AsyncIteratorPrototype%), and
    /// `Object.getPrototypeOf(function*(){})[Symbol.toStringTag]` was
    /// undefined ("GeneratorFunction").
    ///
    /// Built when the first generator or async function object is created
    /// (`vm.rs`): every one of these objects is reachable only through such
    /// a function, and an engine that never creates one saves the heap
    /// (`j21_gc_wurzel` runs at 1 MiB).
    pub(crate) fn es_async_aufbauen(&mut self) -> Result<(), Stoerung> {
        use EsOp::*;
        if self.es.async_gen_proto.is_some() {
            return Ok(());
        }
        let tag = self.bekanntes_symbol_text(crate::builtin::Nativ::SymToStringTag)?;
        let tag = tag.as_str();
        let ai = self.bekanntes_symbol_text(crate::builtin::Nativ::SymAsyncIterator)?;
        let aip = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
        self.es.async_iter_proto = Some(aip);
        let f = self.nativ_anlegen("[Symbol.asyncIterator]", crate::builtin::Nativ::Es(AsyncIterSelbst), self.proto_funktion);
        self.es_eigen(aip, &ai, GVal::Ref(f), true, false, true);
        let agp = self.neues_objekt(Some(aip)).map_err(Stoerung::Fehler)?;
        self.es.async_gen_proto = Some(agp);
        self.es_methode(agp, "next", AsyncGenNext);
        self.es_methode(agp, "return", AsyncGenReturn);
        self.es_methode(agp, "throw", AsyncGenThrow);
        let pagf = self.proto_async_generator_funktion;
        let pgf = self.proto_generator_funktion;
        let paf = self.proto_async_funktion;
        let gp = self.proto_generator;
        if pagf != crate::heap::KEIN_REF {
            self.es_eigen(agp, "constructor", GVal::Ref(pagf), false, false, true);
            self.es_eigen(pagf, "prototype", GVal::Ref(agp), false, false, true);
        }
        if pgf != crate::heap::KEIN_REF {
            self.es_eigen(pgf, "prototype", GVal::Ref(gp), false, false, true);
            self.es_eigen(gp, "constructor", GVal::Ref(pgf), false, false, true);
        }
        for (o, name) in [(agp, "AsyncGenerator"), (pagf, "AsyncGeneratorFunction"), (pgf, "GeneratorFunction"), (paf, "AsyncFunction")] {
            if o != crate::heap::KEIN_REF {
                let t = self.text_wert(name).map_err(Stoerung::Fehler)?;
                self.es_eigen(o, tag, t, false, false, true);
            }
        }
        Ok(())
    }

    /// `Symbol(Symbol.toStringTag)` -> `[Symbol.toStringTag]` for function names.
    fn schluessel_anzeige_kurz(&self, k: &str) -> String {
        match k.strip_prefix("Symbol(").and_then(|r| r.strip_suffix(')')) {
            Some(b) => format!("[{b}]"),
            None => k.to_string(),
        }
    }

    // ---------------------------------------------------------------
    // Small helpers
    // ---------------------------------------------------------------

    fn es_typfehler(&mut self, m: &str) -> Stoerung {
        Stoerung::Wurf(self.fehler_wert("TypeError", m))
    }

    /// CreateIterResultObject (7.4.14).
    pub(crate) fn es_iter_ergebnis(&mut self, wert: GVal, fertig: bool) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        self.wurzeln.push(wert);
        let r = (|| {
            let o = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
            self.wurzeln.push(GVal::Ref(o));
            self.setze(GVal::Ref(o), "value", wert)?;
            self.setze(GVal::Ref(o), "done", GVal::Bool(fertig))?;
            Ok(GVal::Ref(o))
        })();
        self.wurzel_zurueck(marke);
        r
    }

    /// IteratorClose with a normal completion (7.4.11): call `return`.
    fn es_iter_schliessen(&mut self, it: GVal) -> Result<(), Stoerung> {
        let ret = self.hole(it, "return")?;
        if matches!(ret, GVal::Undefiniert | GVal::Null) {
            return Ok(());
        }
        if !self.ist_aufrufbar(ret) {
            return Err(self.es_typfehler("iterator.return is not a function"));
        }
        let r = self.aufruf_synchron(ret, it, &[], false)?;
        if !self.ist_objekt(r) {
            return Err(self.es_typfehler("Iterator result is not an object"));
        }
        Ok(())
    }

    /// IteratorClose with a throw completion: `return` runs, its own errors
    /// are dropped, the original error wins.
    fn es_iter_schliessen_wurf(&mut self, it: GVal, fehler: Stoerung) -> Stoerung {
        if matches!(fehler, Stoerung::Wurf(_)) {
            let marke = self.wurzel_merke();
            if let Stoerung::Wurf(w) = &fehler {
                self.wurzeln.push(*w);
            }
            if let Ok(ret) = self.hole(it, "return")
                && self.ist_aufrufbar(ret)
            {
                let _ = self.aufruf_synchron(ret, it, &[], false);
            }
            self.wurzel_zurueck(marke);
        }
        fehler
    }

    /// The hidden state of a helper/wrapper object.
    fn es_zustand(&self, o: HeapRef, k: &str) -> GVal {
        self.eigene_lese(o, k).unwrap_or(GVal::Undefiniert)
    }

    fn es_zustand_setzen(&mut self, o: HeapRef, k: &str, v: GVal) {
        self.es_eigen(o, k, v, true, false, true);
    }

    /// SetterThatIgnoresPrototypeProperties (27.1.4.1.2): writes `k` on the
    /// receiver, never on `%Iterator.prototype%` itself.
    fn es_setter_ohne_prototyp(&mut self, this: GVal, k: &str, v: GVal) -> Result<GVal, Stoerung> {
        let GVal::Ref(o) = this else {
            return Err(self.es_typfehler("Iterator setter called on a non-object"));
        };
        if Some(o) == self.proto_iterator {
            return Err(self.es_typfehler("Cannot assign to a read only property of Iterator.prototype"));
        }
        if self.eigene_lese(o, k).is_none() {
            self.eigenschaft_mit_attributen(o, k, v, true, true)?;
        } else {
            self.setze(this, k, v)?;
        }
        Ok(GVal::Undefiniert)
    }

    // ---------------------------------------------------------------
    // Array, String, Promise, RegExp, Error
    // ---------------------------------------------------------------

    /// Release 15.10.2026: ArraySpeciesCreate (ECMA-262 10.4.2.3) for the
    /// result `erg` that map/filter/slice/splice/concat/flat/flatMap built
    /// as a plain array: when the receiver's species constructor is not
    /// %Array% (a subclass, or an own `constructor`), the result becomes
    /// `new C(n)` filled with CreateDataPropertyOrThrow (`n` = `laenge`, or
    /// the result length), `length` set when the method sets it. The
    /// constructor runs after the callbacks here (spec: before); the final
    /// object is the same. MEASURED before (node 22 in brackets): `class A
    /// extends Array {}; new A(1, 2).map(x => x) instanceof A` false (true).
    /// Steps 1-6 (before the method runs): `None` = a plain array, else the
    /// species constructor. Only an array receiver reads `constructor`; a
    /// primitive `constructor` throws.
    pub(crate) fn feld_spezies_ctor(&mut self, o: GVal) -> Result<Option<GVal>, Stoerung> {
        let GVal::Ref(r) = o else {
            return Ok(None);
        };
        if !self.ist_js_feld(r) {
            return Ok(None);
        }
        let array = self.es.array_ctor;
        let mut c = self.hole(o, "constructor")?;
        if let (GVal::Ref(cr), Some(a)) = (c, array)
            && cr == a
            && self
                .eigene_eigenschaft(a, crate::symbole::SYM_SPECIES)
                .is_some_and(|e| e.getter.is_some() && e.getter == self.es.spezies_getter)
        {
            return Ok(None);
        }
        if self.es_ist_objekt(c) {
            let marke = self.wurzel_merke();
            self.wurzeln.push(c);
            let s = self.hole(c, crate::symbole::SYM_SPECIES);
            self.wurzel_zurueck(marke);
            c = match s? {
                GVal::Null => GVal::Undefiniert,
                x => x,
            };
        }
        if matches!(c, GVal::Undefiniert) || matches!((c, array), (GVal::Ref(cr), Some(a)) if cr == a) {
            return Ok(None);
        }
        if !crate::eingebaut_array::ist_konstruktor(self, c) {
            return Err(self.es_typfehler("object.constructor[Symbol.species] is not a constructor"));
        }
        // Rooted until the native returns: a species getter may return a
        // function nothing else holds, and the method runs callbacks.
        self.wurzeln.push(c);
        Ok(Some(c))
    }

    /// Step 7 and the copy: the result `erg` (a plain array) as an instance
    /// of the species constructor `c`.
    pub(crate) fn feld_spezies(&mut self, c: Option<GVal>, erg: GVal, laenge: Option<usize>, laenge_setzen: bool) -> Result<GVal, Stoerung> {
        let Some(c) = c else {
            return Ok(erg);
        };
        let marke = self.wurzel_merke();
        self.wurzeln.push(erg);
        self.wurzeln.push(c);
        let ergebnis = self.feld_spezies_innen(c, erg, laenge, laenge_setzen);
        self.wurzel_zurueck(marke);
        ergebnis
    }

    fn feld_spezies_innen(&mut self, c: GVal, erg: GVal, laenge: Option<usize>, laenge_setzen: bool) -> Result<GVal, Stoerung> {
        let GVal::Ref(er) = erg else {
            return Ok(erg);
        };
        let (elemente, luecken) = match self.heap.holen(er) {
            HObj::Feld { elemente, luecken, .. } => (elemente.clone(), luecken.clone()),
            _ => return Ok(erg),
        };
        let n = laenge.unwrap_or(elemente.len());
        let a = self.konstruktor_aufrufen(c, &[GVal::Zahl(n as f64)])?;
        self.wurzeln.push(a);
        let GVal::Ref(ar) = a else {
            return Err(self.es_typfehler("Array species constructor returned a non-object"));
        };
        for (i, v) in elemente.iter().enumerate() {
            if luecken.as_ref().is_some_and(|l| l.contains(&i)) {
                continue;
            }
            self.create_data_property_or_throw(ar, &i.to_string(), *v)?;
        }
        if laenge_setzen {
            self.setze(a, "length", GVal::Zahl(elemente.len() as f64))?;
        }
        Ok(a)
    }

    /// Array.prototype.findLastIndex (ECMA-262 23.1.3.12).
    fn es_find_last_index(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let o = self.zu_objekt(this)?;
        let lw = self.hole(GVal::Ref(o), "length")?;
        let n = self.zu_zahl(lw)?;
        let len = if n.is_nan() || n <= 0.0 { 0.0 } else { n.min(9007199254740991.0).floor() };
        let f = self.arg(args, 0);
        if !self.ist_aufrufbar(f) {
            return Err(self.es_typfehler("findLastIndex: callback is not a function"));
        }
        let this_arg = self.arg(args, 1);
        let mut k = len as i64 - 1;
        while k >= 0 {
            self.schritt().map_err(Stoerung::Fehler)?;
            let v = self.hole(GVal::Ref(o), &k.to_string())?;
            let r = self.aufruf_synchron(f, this_arg, &[v, GVal::Zahl(k as f64), GVal::Ref(o)], false)?;
            if self.zu_bool(r) {
                return Ok(GVal::Zahl(k as f64));
            }
            k -= 1;
        }
        Ok(GVal::Zahl(-1.0))
    }

    /// Array.fromAsync: the JavaScript body above, compiled on first use.
    fn es_from_async(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let f = match self.es.from_async {
            Some(f) => f,
            None => {
                let vorher = self.eval_global;
                self.eval_global = true;
                let f = self.quelltext_ausfuehren(FELD_VON_ASYNC);
                self.eval_global = vorher;
                let GVal::Ref(f) = f? else {
                    return Err(self.es_typfehler("Array.fromAsync is unavailable"));
                };
                self.es.from_async = Some(f);
                f
            }
        };
        let ist_ctor = crate::eingebaut_array::ist_konstruktor(self, this);
        let argv = [self.arg(args, 0), self.arg(args, 1), self.arg(args, 2), this, GVal::Bool(ist_ctor)];
        self.aufruf_synchron(GVal::Ref(f), GVal::Undefiniert, &argv, false)
    }

    /// Release 15.10.2026: JSON.rawJSON (ES2026 JSON.parse source text
    /// access): a frozen null-prototype object `{rawJSON}` whose text is
    /// emitted verbatim by JSON.stringify; only a primitive JSON text without
    /// surrounding whitespace is accepted. MEASURED before: missing (node 22
    /// has it).
    fn es_json_roh(&mut self, text: GVal) -> Result<GVal, Stoerung> {
        let s = crate::eingebaut_text::zu_text_kein_symbol(self, text)?;
        let rand = |c: Option<char>| matches!(c, None | Some('\t' | '\n' | '\r' | ' '));
        if rand(s.chars().next()) || rand(s.chars().last()) {
            return Err(Stoerung::Wurf(self.fehler_wert("SyntaxError", "Invalid value for JSON.rawJSON")));
        }
        let sv = self.text_wert(&s).map_err(Stoerung::Fehler)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(sv);
        let r = (|| {
            let wert = self.n_json_lesen(&[sv])?;
            if self.es_ist_objekt(wert) {
                return Err(Stoerung::Wurf(self.fehler_wert("SyntaxError", "Invalid value for JSON.rawJSON")));
            }
            let o = self.neues_objekt(None).map_err(Stoerung::Fehler)?;
            self.wurzeln.push(GVal::Ref(o));
            self.es_eigen(o, "rawJSON", sv, false, true, false);
            self.es_eigen(o, H_ROH_JSON, GVal::Bool(true), false, false, false);
            self.heap.kosten_verbuchen(2 * crate::heap::KOSTEN_EINTRAG);
            if let HObj::Gegenstand { sperre, .. } = self.heap.holen_mut(o) {
                *sperre = crate::heap::Sperre::Gefroren;
            }
            Ok(GVal::Ref(o))
        })();
        self.wurzel_zurueck(marke);
        r
    }

    /// Release 15.10.2026: Math.sumPrecise (ES2026): the exact sum of an
    /// iterable of Numbers, rounded once (ties to even); -0 for an empty
    /// list or only -0; NaN/±Infinity as IEEE; a non-Number is a TypeError.
    /// MEASURED before: missing (node 22 does not have it either; the
    /// expected values come from Python's math.fsum, see the tests).
    fn es_summe_genau(&mut self, items: GVal) -> Result<GVal, Stoerung> {
        if matches!(items, GVal::Undefiniert | GVal::Null) {
            return Err(self.es_typfehler("Math.sumPrecise requires an iterable"));
        }
        let (it, next) = self.iterator_holen(items)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(it);
        self.wurzeln.push(next);
        let mut summe = GenaueSumme::default();
        let r = loop {
            let schritt = match self.iterator_schritt(it, next) {
                Ok(s) => s,
                Err(e) => break Err(e),
            };
            let Some(v) = schritt else {
                break Ok(GVal::Zahl(summe.ergebnis()));
            };
            let GVal::Zahl(n) = v else {
                let f = self.es_typfehler("Math.sumPrecise: value is not a Number");
                break Err(self.es_iter_schliessen_wurf(it, f));
            };
            summe.dazu(n);
        };
        self.wurzel_zurueck(marke);
        r
    }

    /// Promise.try (ECMA-262 2025 27.2.4.8).
    fn es_promise_try(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        if !self.ist_objekt(this) {
            return Err(self.es_typfehler("Promise.try called on a non-object"));
        }
        let (ziel, erf, abl) = self.versprechen_capability(this)?;
        let marke = self.wurzel_merke();
        self.wurzeln.extend_from_slice(&[GVal::Ref(ziel), erf, abl]);
        let rest = if args.len() > 1 { &args[1..] } else { &[] };
        let r = (|| {
            match self.aufruf_synchron(self.arg(args, 0), GVal::Undefiniert, rest, false) {
                Ok(v) => self.aufruf_synchron(erf, GVal::Undefiniert, &[v], false)?,
                Err(Stoerung::Wurf(w)) => self.aufruf_synchron(abl, GVal::Undefiniert, &[w], false)?,
                Err(e) => return Err(e),
            };
            Ok(GVal::Ref(ziel))
        })();
        self.wurzel_zurueck(marke);
        r
    }

    /// RegExp.escape (ECMA-262 2025 22.2.5.1).
    fn es_regexp_escape(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let s = match self.arg(args, 0) {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Zeichenkette(s) => s.clone(),
                _ => return Err(self.es_typfehler("RegExp.escape requires a string")),
            },
            _ => return Err(self.es_typfehler("RegExp.escape requires a string")),
        };
        let mut out = String::with_capacity(s.len() * 2);
        for (i, c) in s.chars().enumerate() {
            let cp = c as u32;
            if i == 0 && c.is_ascii_alphanumeric() {
                out.push_str(&format!("\\x{cp:02x}"));
                continue;
            }
            match c {
                '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '/' => {
                    out.push('\\');
                    out.push(c);
                }
                '\t' => out.push_str("\\t"),
                '\n' => out.push_str("\\n"),
                '\u{0B}' => out.push_str("\\v"),
                '\u{0C}' => out.push_str("\\f"),
                '\r' => out.push_str("\\r"),
                _ => {
                    let leer = matches!(cp, 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF);
                    if ",-=<>#&!%:;@~'`\"".contains(c) || leer {
                        if cp <= 0xFF {
                            out.push_str(&format!("\\x{cp:02x}"));
                        } else {
                            let mut b = [0u16; 2];
                            for u in c.encode_utf16(&mut b) {
                                out.push_str(&format!("\\u{:04x}", u));
                            }
                        }
                    } else {
                        out.push(c);
                    }
                }
            }
        }
        self.text_wert(&out).map_err(Stoerung::Fehler)
    }

    /// Error.captureStackTrace(target[, constructorOpt]) (V8): an own,
    /// non-enumerable `stack` on `target`, "Name: message" plus frames.
    /// MEASURED before: missing; many libraries call it unguarded (node:
    /// `typeof o.stack` -> "string" afterwards).
    fn es_capture_stack_trace(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let ziel = self.arg(args, 0);
        let GVal::Ref(o) = ziel else {
            return Err(self.es_typfehler("Invalid argument"));
        };
        if !self.ist_objekt(ziel) {
            return Err(self.es_typfehler("Invalid argument"));
        }
        let name = match self.hole(ziel, "name")? {
            GVal::Undefiniert => "Error".to_string(),
            v => self.zu_text(v)?,
        };
        let nachricht = match self.hole(ziel, "message")? {
            GVal::Undefiniert => String::new(),
            v => self.zu_text(v)?,
        };
        let mut kopf = if name.is_empty() {
            nachricht
        } else if nachricht.is_empty() {
            name
        } else {
            format!("{name}: {nachricht}")
        };
        for frame in self.rahmen.iter().rev().take(10) {
            let fname = self
                .programm
                .as_ref()
                .and_then(|p| p.funktionen.get(frame.fun))
                .map(|f| f.name.as_str())
                .unwrap_or("");
            if fname.is_empty() || fname.starts_with('<') {
                kopf.push_str(&format!("\n    at <anonymous>:{}:1", frame.zeile));
            } else {
                kopf.push_str(&format!("\n    at {fname} (<anonymous>:{}:1)", frame.zeile));
            }
        }
        let t = self.text_wert(&kopf).map_err(Stoerung::Fehler)?;
        if self.eigenschaften_karte_mut(o).is_some() {
            self.es_eigen(o, "stack", t, true, false, true);
        } else {
            self.setze(ziel, "stack", t)?;
        }
        Ok(GVal::Undefiniert)
    }

    // ---------------------------------------------------------------
    // Set methods (ECMA-262 2025 24.2.4)
    // ---------------------------------------------------------------

    /// GetSetRecord (24.2.1.2): (size, has, keys) of a set-like object.
    fn es_set_record(&mut self, other: GVal) -> Result<(f64, GVal, GVal), Stoerung> {
        if !self.ist_objekt(other) {
            return Err(self.es_typfehler("The argument must be a set-like object"));
        }
        let roh = self.hole(other, "size")?;
        let n = self.zu_zahl(roh)?;
        if n.is_nan() {
            return Err(self.es_typfehler("The 'size' property must be a number"));
        }
        let groesse = n.trunc();
        if groesse < 0.0 {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "'size' must not be negative")));
        }
        let has = self.hole(other, "has")?;
        if !self.ist_aufrufbar(has) {
            return Err(self.es_typfehler("The 'has' property must be a function"));
        }
        let keys = self.hole(other, "keys")?;
        if !self.ist_aufrufbar(keys) {
            return Err(self.es_typfehler("The 'keys' property must be a function"));
        }
        Ok((groesse, has, keys))
    }

    /// The live entries of a Set (re-read per step: callbacks may change it).
    fn es_menge_eintrag(&self, r: HeapRef, i: usize) -> Option<Option<GVal>> {
        match self.heap.holen(r) {
            HObj::Menge { eintraege, belegt, .. } => {
                if i >= eintraege.len() {
                    None
                } else {
                    Some(belegt.get(i).copied().unwrap_or(false).then(|| eintraege[i]))
                }
            }
            _ => None,
        }
    }

    fn es_menge_groesse(&self, r: HeapRef) -> f64 {
        match self.heap.holen(r) {
            HObj::Menge { belegt, .. } => belegt.iter().filter(|b| **b).count() as f64,
            _ => 0.0,
        }
    }

    /// A copy of this Set's data as a new Set.
    fn es_menge_kopie(&mut self, r: HeapRef) -> Result<HeapRef, Stoerung> {
        let neu = self.n_menge_ctor(&[], true)?;
        let GVal::Ref(n) = neu else { return Err(self.es_typfehler("Set")) };
        self.wurzeln.push(neu);
        let mut i = 0;
        while let Some(e) = self.es_menge_eintrag(r, i) {
            if let Some(v) = e {
                self.n_menge_hinzu(neu, &[v])?;
            }
            i += 1;
        }
        Ok(n)
    }

    /// GetIteratorFromMethod(other, keys) and its steps, calling `f` per
    /// value (with -0 as +0); `f` returns `false` to stop (the iterator is
    /// then closed).
    fn es_keys_durchlaufen(
        &mut self,
        other: GVal,
        keys: GVal,
        mut f: impl FnMut(&mut Engine, GVal) -> Result<bool, Stoerung>,
    ) -> Result<bool, Stoerung> {
        let it = self.aufruf_synchron(keys, other, &[], false)?;
        if !self.ist_objekt(it) {
            return Err(self.es_typfehler("keys() result is not an object"));
        }
        self.wurzeln.push(it);
        let next = self.hole(it, "next")?;
        self.wurzeln.push(next);
        loop {
            self.schritt().map_err(Stoerung::Fehler)?;
            let Some(v) = self.iterator_schritt(it, next)? else { return Ok(true) };
            let v = match v {
                GVal::Zahl(z) if z == 0.0 => GVal::Zahl(0.0),
                v => v,
            };
            self.wurzeln.push(v);
            if !f(self, v)? {
                self.es_iter_schliessen(it)?;
                return Ok(false);
            }
        }
    }

    /// union (0), intersection (1), difference (2), symmetricDifference (3).
    fn es_menge_op(&mut self, this: GVal, args: &[GVal], art: u8) -> Result<GVal, Stoerung> {
        let name = ["union", "intersection", "difference", "symmetricDifference"][art as usize];
        let r = self.menge_als_ref(this, name)?;
        let other = self.arg(args, 0);
        let marke = self.wurzel_merke();
        let erg = (|| {
            let (groesse, has, keys) = self.es_set_record(other)?;
            self.wurzeln.extend_from_slice(&[has, keys]);
            match art {
                0 => {
                    let neu = self.es_menge_kopie(r)?;
                    self.es_keys_durchlaufen(other, keys, |e, v| {
                        e.n_menge_hinzu(GVal::Ref(neu), &[v])?;
                        Ok(true)
                    })?;
                    Ok(GVal::Ref(neu))
                }
                1 => {
                    let neu = self.n_menge_ctor(&[], true)?;
                    self.wurzeln.push(neu);
                    if self.es_menge_groesse(r) <= groesse {
                        let mut i = 0;
                        while let Some(e) = self.es_menge_eintrag(r, i) {
                            if let Some(v) = e {
                                let drin = self.aufruf_synchron(has, other, &[v], false)?;
                                if self.zu_bool(drin) {
                                    self.n_menge_hinzu(neu, &[v])?;
                                }
                            }
                            i += 1;
                        }
                    } else {
                        self.es_keys_durchlaufen(other, keys, |e, v| {
                            if e.es_menge_hat(r, v)? {
                                e.n_menge_hinzu(neu, &[v])?;
                            }
                            Ok(true)
                        })?;
                    }
                    Ok(neu)
                }
                2 => {
                    let neu = self.es_menge_kopie(r)?;
                    if self.es_menge_groesse(r) <= groesse {
                        let mut i = 0;
                        while let Some(e) = self.es_menge_eintrag(r, i) {
                            if let Some(v) = e {
                                let drin = self.aufruf_synchron(has, other, &[v], false)?;
                                if self.zu_bool(drin) {
                                    self.n_menge_loeschen(GVal::Ref(neu), &[v])?;
                                }
                            }
                            i += 1;
                        }
                    } else {
                        self.es_keys_durchlaufen(other, keys, |e, v| {
                            e.n_menge_loeschen(GVal::Ref(neu), &[v])?;
                            Ok(true)
                        })?;
                    }
                    Ok(GVal::Ref(neu))
                }
                _ => {
                    let neu = self.es_menge_kopie(r)?;
                    self.es_keys_durchlaufen(other, keys, |e, v| {
                        if e.es_menge_hat(r, v)? {
                            e.n_menge_loeschen(GVal::Ref(neu), &[v])?;
                        } else {
                            e.n_menge_hinzu(GVal::Ref(neu), &[v])?;
                        }
                        Ok(true)
                    })?;
                    Ok(GVal::Ref(neu))
                }
            }
        })();
        self.wurzel_zurueck(marke);
        erg
    }

    /// isSubsetOf (0), isSupersetOf (1), isDisjointFrom (2).
    fn es_menge_test(&mut self, this: GVal, args: &[GVal], art: u8) -> Result<GVal, Stoerung> {
        let name = ["isSubsetOf", "isSupersetOf", "isDisjointFrom"][art as usize];
        let r = self.menge_als_ref(this, name)?;
        let other = self.arg(args, 0);
        let marke = self.wurzel_merke();
        let erg = (|| {
            let (groesse, has, keys) = self.es_set_record(other)?;
            self.wurzeln.extend_from_slice(&[has, keys]);
            let eigene = self.es_menge_groesse(r);
            let ueber_eigene = |e: &mut Engine, soll: bool| -> Result<bool, Stoerung> {
                let mut i = 0;
                while let Some(x) = e.es_menge_eintrag(r, i) {
                    if let Some(v) = x {
                        let drin = e.aufruf_synchron(has, other, &[v], false)?;
                        if e.zu_bool(drin) != soll {
                            return Ok(false);
                        }
                    }
                    i += 1;
                }
                Ok(true)
            };
            Ok(GVal::Bool(match art {
                0 => eigene <= groesse && ueber_eigene(self, true)?,
                1 => {
                    eigene >= groesse
                        && self.es_keys_durchlaufen(other, keys, |e, v| Ok(e.es_menge_hat(r, v)?))?
                }
                _ => {
                    if eigene <= groesse {
                        ueber_eigene(self, false)?
                    } else {
                        self.es_keys_durchlaufen(other, keys, |e, v| Ok(!e.es_menge_hat(r, v)?))?
                    }
                }
            }))
        })();
        self.wurzel_zurueck(marke);
        erg
    }

    // ---------------------------------------------------------------
    // Iterator helpers (ECMA-262 2025 27.1)
    // ---------------------------------------------------------------

    /// GetIteratorDirect (7.4.4) on `this`: the object and its `next`.
    fn es_iter_direkt(&mut self, this: GVal, name: &str) -> Result<(GVal, GVal), Stoerung> {
        if !self.ist_objekt(this) {
            return Err(self.es_typfehler(&format!("Iterator.prototype.{name} called on a non-object")));
        }
        let next = self.hole(this, "next")?;
        Ok((this, next))
    }

    /// map (0), filter (1), take (2), drop (3), flatMap (4): a lazy helper.
    fn es_helfer_bauen(&mut self, this: GVal, args: &[GVal], art: u8) -> Result<GVal, Stoerung> {
        let name = ["map", "filter", "take", "drop", "flatMap"][art as usize];
        if !self.ist_objekt(this) {
            return Err(self.es_typfehler(&format!("Iterator.prototype.{name} called on a non-object")));
        }
        let a = self.arg(args, 0);
        let mut n = GVal::Undefiniert;
        if art == 2 || art == 3 {
            // ToNumber, NaN -> RangeError, ToIntegerOrInfinity < 0 -> RangeError;
            // an error closes the underlying iterator (2025).
            let z = match self.zu_zahl(a) {
                Ok(z) => z,
                Err(e) => return Err(self.es_iter_schliessen_wurf(this, e)),
            };
            if z.is_nan() || z.trunc() < 0.0 {
                let e = Stoerung::Wurf(self.fehler_wert("RangeError", &format!("{} must be positive", z)));
                return Err(self.es_iter_schliessen_wurf(this, e));
            }
            n = GVal::Zahl(if z.is_infinite() { z } else { z.trunc() });
        } else if !self.ist_aufrufbar(a) {
            let e = self.es_typfehler(&format!("Iterator.prototype.{name}: callback is not a function"));
            return Err(self.es_iter_schliessen_wurf(this, e));
        }
        let (it, next) = self.es_iter_direkt(this, name)?;
        let marke = self.wurzel_merke();
        self.wurzeln.extend_from_slice(&[it, next, a]);
        let r = (|| {
            let o = self.neues_objekt(self.es.helfer_proto).map_err(Stoerung::Fehler)?;
            self.es_zustand_setzen(o, H_ITER, it);
            self.es_zustand_setzen(o, H_NEXT, next);
            self.es_zustand_setzen(o, H_ART, GVal::Zahl(art as f64));
            self.es_zustand_setzen(o, H_FN, a);
            self.es_zustand_setzen(o, H_N, if art >= 2 && art <= 3 { n } else { GVal::Zahl(0.0) });
            self.es_zustand_setzen(o, H_STATUS, GVal::Zahl(0.0));
            Ok(GVal::Ref(o))
        })();
        self.wurzel_zurueck(marke);
        r
    }

    /// %IteratorHelperPrototype%.next (27.1.2.1.1).
    fn es_helfer_next(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let o = match this {
            GVal::Ref(o) if self.eigene_lese(o, H_ART).is_some() => o,
            _ => return Err(self.es_typfehler("Iterator helper next called on an incompatible receiver")),
        };
        match self.es_zustand(o, H_STATUS) {
            GVal::Zahl(s) if s == 1.0 => return Err(self.es_typfehler("Generator is already running")),
            GVal::Zahl(s) if s == 2.0 => return self.es_iter_ergebnis(GVal::Undefiniert, true),
            _ => {}
        }
        self.es_zustand_setzen(o, H_STATUS, GVal::Zahl(1.0));
        let r = self.es_helfer_schritt(o);
        let fertig = !matches!(r, Ok(Some(_)));
        self.es_zustand_setzen(o, H_STATUS, GVal::Zahl(if fertig { 2.0 } else { 0.0 }));
        match r? {
            Some(v) => self.es_iter_ergebnis(v, false),
            None => self.es_iter_ergebnis(GVal::Undefiniert, true),
        }
    }

    /// One step of a helper: `Some(value)` or `None` when done.
    fn es_helfer_schritt(&mut self, o: HeapRef) -> Result<Option<GVal>, Stoerung> {
        let it = self.es_zustand(o, H_ITER);
        let next = self.es_zustand(o, H_NEXT);
        let f = self.es_zustand(o, H_FN);
        let art = match self.es_zustand(o, H_ART) {
            GVal::Zahl(a) => a as u8,
            _ => 0,
        };
        let zahl = |e: &Engine| match e.es_zustand(o, H_N) {
            GVal::Zahl(z) => z,
            _ => 0.0,
        };
        match art {
            // map / filter: the mapper gets (value, counter).
            0 | 1 => loop {
                self.schritt().map_err(Stoerung::Fehler)?;
                let Some(v) = self.iterator_schritt(it, next)? else { return Ok(None) };
                let zaehler = zahl(self);
                self.es_zustand_setzen(o, H_N, GVal::Zahl(zaehler + 1.0));
                let marke = self.wurzel_merke();
                self.wurzeln.push(v);
                let r = self.aufruf_synchron(f, GVal::Undefiniert, &[v, GVal::Zahl(zaehler)], false);
                self.wurzel_zurueck(marke);
                let r = match r {
                    Ok(r) => r,
                    Err(e) => return Err(self.es_iter_schliessen_wurf(it, e)),
                };
                if art == 0 {
                    return Ok(Some(r));
                }
                if self.zu_bool(r) {
                    return Ok(Some(v));
                }
            },
            // take: after `limit` values the underlying iterator is closed.
            2 => {
                let rest = zahl(self);
                if rest == 0.0 {
                    self.es_iter_schliessen(it)?;
                    return Ok(None);
                }
                if rest.is_finite() {
                    self.es_zustand_setzen(o, H_N, GVal::Zahl(rest - 1.0));
                }
                self.iterator_schritt(it, next)
            }
            // drop: skip `limit` values first.
            3 => {
                loop {
                    let rest = zahl(self);
                    if rest <= 0.0 {
                        break;
                    }
                    self.schritt().map_err(Stoerung::Fehler)?;
                    if rest.is_finite() {
                        self.es_zustand_setzen(o, H_N, GVal::Zahl(rest - 1.0));
                    }
                    if self.iterator_schritt(it, next)?.is_none() {
                        return Ok(None);
                    }
                }
                self.iterator_schritt(it, next)
            }
            // flatMap: the inner iterator first, then the next outer value.
            _ => loop {
                self.schritt().map_err(Stoerung::Fehler)?;
                let innen = self.es_zustand(o, H_INNEN);
                if !matches!(innen, GVal::Undefiniert) {
                    let inext = self.es_zustand(o, H_INNEN_NEXT);
                    match self.iterator_schritt(innen, inext) {
                        Ok(Some(v)) => return Ok(Some(v)),
                        Ok(None) => self.es_zustand_setzen(o, H_INNEN, GVal::Undefiniert),
                        Err(e) => return Err(self.es_iter_schliessen_wurf(it, e)),
                    }
                    continue;
                }
                let Some(v) = self.iterator_schritt(it, next)? else { return Ok(None) };
                let zaehler = zahl(self);
                self.es_zustand_setzen(o, H_N, GVal::Zahl(zaehler + 1.0));
                let marke = self.wurzel_merke();
                self.wurzeln.push(v);
                let r = self
                    .aufruf_synchron(f, GVal::Undefiniert, &[v, GVal::Zahl(zaehler)], false)
                    .and_then(|m| self.es_iterator_flach(m, false));
                self.wurzel_zurueck(marke);
                match r {
                    Ok((i2, n2)) => {
                        self.es_zustand_setzen(o, H_INNEN, i2);
                        self.es_zustand_setzen(o, H_INNEN_NEXT, n2);
                    }
                    Err(e) => return Err(self.es_iter_schliessen_wurf(it, e)),
                }
            },
        }
    }

    /// %IteratorHelperPrototype%.return (27.1.2.1.2).
    fn es_helfer_return(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let o = match this {
            GVal::Ref(o) if self.eigene_lese(o, H_ART).is_some() => o,
            _ => return Err(self.es_typfehler("Iterator helper return called on an incompatible receiver")),
        };
        match self.es_zustand(o, H_STATUS) {
            GVal::Zahl(s) if s == 1.0 => return Err(self.es_typfehler("Generator is already running")),
            GVal::Zahl(s) if s == 2.0 => return self.es_iter_ergebnis(GVal::Undefiniert, true),
            _ => {}
        }
        self.es_zustand_setzen(o, H_STATUS, GVal::Zahl(2.0));
        let innen = self.es_zustand(o, H_INNEN);
        if !matches!(innen, GVal::Undefiniert) {
            self.es_iter_schliessen(innen)?;
        }
        let it = self.es_zustand(o, H_ITER);
        self.es_iter_schliessen(it)?;
        self.es_iter_ergebnis(GVal::Undefiniert, true)
    }

    /// GetIteratorFlattenable (7.4.5): an object's @@iterator result (or the
    /// object itself as an iterator); strings only with `strings`.
    fn es_iterator_flach(&mut self, v: GVal, strings: bool) -> Result<(GVal, GVal), Stoerung> {
        let ist_text = matches!(v, GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Zeichenkette(_)));
        if !self.ist_objekt(v) && !(strings && ist_text) {
            return Err(self.es_typfehler("Value is not an iterable object"));
        }
        let sym = self.bekanntes_symbol_text(crate::builtin::Nativ::SymIterator)?;
        let m = self.hole(v, &sym)?;
        let it = if matches!(m, GVal::Undefiniert | GVal::Null) {
            v
        } else {
            if !self.ist_aufrufbar(m) {
                return Err(self.es_typfehler("Symbol.iterator is not a function"));
            }
            let it = self.aufruf_synchron(m, v, &[], false)?;
            if !self.ist_objekt(it) {
                return Err(self.es_typfehler("Iterator is not an object"));
            }
            it
        };
        self.wurzeln.push(it);
        let next = self.hole(it, "next")?;
        Ok((it, next))
    }

    /// Iterator.from (27.1.3.2.1).
    fn es_iterator_from(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        let r = (|| {
            let (it, next) = self.es_iterator_flach(self.arg(args, 0), true)?;
            self.wurzeln.push(next);
            // OrdinaryHasInstance(%Iterator%, it): already an Iterator.
            let ip = self.proto_iterator;
            let mut p = match it {
                GVal::Ref(r) => self.heap_proto_von(r),
                _ => None,
            };
            let mut tiefe = 0;
            while let Some(q) = p {
                if Some(q) == ip {
                    return Ok(it);
                }
                tiefe += 1;
                if tiefe > 10_000 {
                    break;
                }
                p = self.heap_proto_von(q);
            }
            let o = self.neues_objekt(self.es.huelle_proto).map_err(Stoerung::Fehler)?;
            self.es_zustand_setzen(o, H_ITER, it);
            self.es_zustand_setzen(o, H_NEXT, next);
            Ok(GVal::Ref(o))
        })();
        self.wurzel_zurueck(marke);
        r
    }

    /// %WrapForValidIteratorPrototype%.next / .return.
    fn es_huelle(&mut self, this: GVal, zurueck: bool) -> Result<GVal, Stoerung> {
        let o = match this {
            GVal::Ref(o) if self.eigene_lese(o, H_ITER).is_some() && self.eigene_lese(o, H_ART).is_none() => o,
            _ => return Err(self.es_typfehler("Iterator wrapper called on an incompatible receiver")),
        };
        let it = self.es_zustand(o, H_ITER);
        if !zurueck {
            let next = self.es_zustand(o, H_NEXT);
            return self.aufruf_synchron(next, it, &[], false);
        }
        let ret = self.hole(it, "return")?;
        if matches!(ret, GVal::Undefiniert | GVal::Null) {
            return self.es_iter_ergebnis(GVal::Undefiniert, true);
        }
        self.aufruf_synchron(ret, it, &[], false)
    }

    /// reduce (27.1.4.8).
    fn es_iter_reduce(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        if !self.ist_objekt(this) {
            return Err(self.es_typfehler("Iterator.prototype.reduce called on a non-object"));
        }
        let f = self.arg(args, 0);
        if !self.ist_aufrufbar(f) {
            let e = self.es_typfehler("Iterator.prototype.reduce: callback is not a function");
            return Err(self.es_iter_schliessen_wurf(this, e));
        }
        let (it, next) = self.es_iter_direkt(this, "reduce")?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(next);
        let r = (|| {
            let (mut acc, mut zaehler) = if args.len() < 2 {
                match self.iterator_schritt(it, next)? {
                    Some(v) => (v, 1.0),
                    None => return Err(self.es_typfehler("Reduce of empty iterator with no initial value")),
                }
            } else {
                (self.arg(args, 1), 0.0)
            };
            loop {
                self.schritt().map_err(Stoerung::Fehler)?;
                self.wurzeln.push(acc);
                let Some(v) = self.iterator_schritt(it, next)? else { return Ok(acc) };
                self.wurzeln.push(v);
                acc = match self.aufruf_synchron(f, GVal::Undefiniert, &[acc, v, GVal::Zahl(zaehler)], false) {
                    Ok(a) => a,
                    Err(e) => return Err(self.es_iter_schliessen_wurf(it, e)),
                };
                zaehler += 1.0;
            }
        })();
        self.wurzel_zurueck(marke);
        r
    }

    /// toArray (0), forEach (1), some (2), every (3), find (4).
    fn es_iter_eifrig(&mut self, this: GVal, args: &[GVal], art: u8) -> Result<GVal, Stoerung> {
        let name = ["toArray", "forEach", "some", "every", "find"][art as usize];
        if !self.ist_objekt(this) {
            return Err(self.es_typfehler(&format!("Iterator.prototype.{name} called on a non-object")));
        }
        let f = self.arg(args, 0);
        if art > 0 && !self.ist_aufrufbar(f) {
            let e = self.es_typfehler(&format!("Iterator.prototype.{name}: callback is not a function"));
            return Err(self.es_iter_schliessen_wurf(this, e));
        }
        let (it, next) = self.es_iter_direkt(this, name)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(next);
        let r = (|| {
            let mut gesammelt: Vec<GVal> = Vec::new();
            let mut zaehler = 0.0;
            loop {
                self.schritt().map_err(Stoerung::Fehler)?;
                let Some(v) = self.iterator_schritt(it, next)? else {
                    return Ok(match art {
                        0 => {
                            let a = self.feld_aus_werten(&gesammelt)?;
                            GVal::Ref(a)
                        }
                        2 => GVal::Bool(false),
                        3 => GVal::Bool(true),
                        _ => GVal::Undefiniert,
                    });
                };
                self.wurzeln.push(v);
                if art == 0 {
                    if gesammelt.len() >= crate::ops::MAX_SAMMLUNG {
                        return Err(Stoerung::Fehler(crate::fehler::JsFehler::Speicher("toArray zu gross".to_string())));
                    }
                    gesammelt.push(v);
                    continue;
                }
                let r = match self.aufruf_synchron(f, GVal::Undefiniert, &[v, GVal::Zahl(zaehler)], false) {
                    Ok(r) => r,
                    Err(e) => return Err(self.es_iter_schliessen_wurf(it, e)),
                };
                zaehler += 1.0;
                let wahr = self.zu_bool(r);
                let ende = match art {
                    2 if wahr => Some(GVal::Bool(true)),
                    3 if !wahr => Some(GVal::Bool(false)),
                    4 if wahr => Some(v),
                    _ => None,
                };
                if let Some(e) = ende {
                    self.es_iter_schliessen(it)?;
                    return Ok(e);
                }
            }
        })();
        self.wurzel_zurueck(marke);
        r
    }
}

impl Engine {
    /// SetDataHas on a Set object.
    fn es_menge_hat(&mut self, r: HeapRef, v: GVal) -> Result<bool, Stoerung> {
        Ok(matches!(self.n_menge_haben(GVal::Ref(r), &[v])?, GVal::Bool(true)))
    }
}

impl Engine {
    /// Release 15.10.2026: the shared prototype of the built-in iterators:
    /// 0 %ArrayIteratorPrototype%, 1 %MapIteratorPrototype%, 2
    /// %SetIteratorPrototype%, 3 %RegExpStringIteratorPrototype% (below
    /// %Iterator.prototype%, with `next` and `@@toStringTag`), created on
    /// first use.
    pub(crate) fn es_iter_proto(&mut self, art: u8) -> Result<HeapRef, Stoerung> {
        let vorhanden = match art {
            0 => self.es.feld_iter_proto,
            1 => self.es.karte_iter_proto,
            2 => self.es.menge_iter_proto,
            4 => self.es.text_iter_proto,
            _ => self.es.regex_iter_proto,
        };
        if let Some(p) = vorhanden {
            return Ok(p);
        }
        let basis = self.iterator_basis()?;
        let p = self.neues_objekt(Some(basis)).map_err(Stoerung::Fehler)?;
        match art {
            0 => self.es.feld_iter_proto = Some(p),
            1 => self.es.karte_iter_proto = Some(p),
            2 => self.es.menge_iter_proto = Some(p),
            4 => self.es.text_iter_proto = Some(p),
            _ => self.es.regex_iter_proto = Some(p),
        }
        let next = match art {
            0 | 4 => crate::builtin::Nativ::FeldIteratorNext,
            1 | 2 => crate::builtin::Nativ::KarteIterNext,
            _ => crate::builtin::Nativ::TextMatchAlleNext,
        };
        let f = self.nativ_anlegen("next", next, self.proto_funktion);
        self.es_eigen(p, "next", GVal::Ref(f), true, false, true);
        let tag = self.bekanntes_symbol_text(crate::builtin::Nativ::SymToStringTag)?;
        let name = ["Array Iterator", "Map Iterator", "Set Iterator", "RegExp String Iterator", "String Iterator"][art.min(4) as usize];
        let t = self.text_wert(name).map_err(Stoerung::Fehler)?;
        self.es_eigen(p, &tag, t, false, false, true);
        Ok(p)
    }
}

// ===========================================================================
// Release 15.10.2026: binary data (Float16Array, Math.f16round, DataView
// float16).
// ===========================================================================

/// Release 15.10.2026: exact summation for Math.sumPrecise. Finite values
/// are integers in units of 2^-1074 (at most 2^2098); positives and
/// negatives go into two unsigned big integers, subtracted at the end and
/// rounded once. `zustand`: 0 only -0 seen, 1 finite, 2 +Inf, 3 -Inf, 4 NaN.
#[derive(Default)]
struct GenaueSumme {
    plus: Vec<u64>,
    minus: Vec<u64>,
    zustand: u8,
}

impl GenaueSumme {
    fn dazu(&mut self, n: f64) {
        if self.zustand == 4 {
            return;
        }
        if n.is_nan() {
            self.zustand = 4;
            return;
        }
        if n.is_infinite() {
            let z = if n > 0.0 { 2 } else { 3 };
            self.zustand = if self.zustand == 5 - z { 4 } else { z };
            return;
        }
        if n == 0.0 && n.is_sign_negative() {
            return;
        }
        if self.zustand == 0 {
            self.zustand = 1;
        }
        if self.zustand != 1 || n == 0.0 {
            return;
        }
        let bits = n.to_bits();
        let exp = ((bits >> 52) & 0x7FF) as u32;
        let frac = bits & ((1u64 << 52) - 1);
        let (mant, schiebe) = if exp == 0 { (frac, 0) } else { (frac | (1u64 << 52), exp - 1) };
        let ziel = if n > 0.0 { &mut self.plus } else { &mut self.minus };
        addiere(ziel, mant, schiebe as usize);
    }

    fn ergebnis(&self) -> f64 {
        match self.zustand {
            0 => -0.0,
            2 => f64::INFINITY,
            3 => f64::NEG_INFINITY,
            4 => f64::NAN,
            _ => {
                let (betrag, negativ) = if vergleiche(&self.plus, &self.minus) == std::cmp::Ordering::Less {
                    (subtrahiere(&self.minus, &self.plus), true)
                } else {
                    (subtrahiere(&self.plus, &self.minus), false)
                };
                let x = runde_zu_f64(&betrag);
                if negativ { -x } else { x }
            }
        }
    }
}

/// `ziel += mant << schiebe` (little-endian 64-bit limbs).
fn addiere(ziel: &mut Vec<u64>, mant: u64, schiebe: usize) {
    let (w, b) = (schiebe / 64, schiebe % 64);
    let breit = (mant as u128) << b;
    if ziel.len() < w + 3 {
        ziel.resize(w + 3, 0);
    }
    let mut uebertrag = 0u128;
    for (i, teil) in [breit as u64, (breit >> 64) as u64].into_iter().enumerate() {
        let s = ziel[w + i] as u128 + teil as u128 + uebertrag;
        ziel[w + i] = s as u64;
        uebertrag = s >> 64;
    }
    let mut i = w + 2;
    while uebertrag != 0 {
        if i >= ziel.len() {
            ziel.push(0);
        }
        let s = ziel[i] as u128 + uebertrag;
        ziel[i] = s as u64;
        uebertrag = s >> 64;
        i += 1;
    }
}

fn vergleiche(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    let n = a.len().max(b.len());
    for i in (0..n).rev() {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

/// `a - b` for `a >= b`.
fn subtrahiere(a: &[u64], b: &[u64]) -> Vec<u64> {
    let mut out = Vec::with_capacity(a.len());
    let mut leihe = 0i128;
    for i in 0..a.len() {
        let d = a[i] as i128 - b.get(i).copied().unwrap_or(0) as i128 - leihe;
        if d < 0 {
            out.push((d + (1i128 << 64)) as u64);
            leihe = 1;
        } else {
            out.push(d as u64);
            leihe = 0;
        }
    }
    out
}

/// The non-negative integer `z` (units of 2^-1074) as the nearest double,
/// ties to even; past the largest double it is +Infinity.
fn runde_zu_f64(z: &[u64]) -> f64 {
    let Some(oben) = z.iter().rposition(|&w| w != 0) else {
        return 0.0;
    };
    let hoch = oben * 64 + (63 - z[oben].leading_zeros() as usize);
    let bit = |i: usize| (z[i / 64] >> (i % 64)) & 1;
    if hoch < 53 {
        let wert = z[0] as f64;
        return wert * f64::from_bits(1);
    }
    let mut mant = 0u64;
    for i in (hoch - 52..=hoch).rev() {
        mant = (mant << 1) | bit(i);
    }
    let rund = bit(hoch - 53);
    let klebrig = (0..hoch - 53).any(|i| bit(i) == 1);
    let mut hoch = hoch;
    if rund == 1 && (klebrig || mant & 1 == 1) {
        mant += 1;
        if mant == 1u64 << 53 {
            mant >>= 1;
            hoch += 1;
        }
    }
    // value = mant * 2^(hoch - 52 - 1074) = 1.f * 2^(hoch - 1074)
    let exp = hoch as i64 - 1074 + 1023;
    if exp >= 2047 {
        return f64::INFINITY;
    }
    f64::from_bits(((exp as u64) << 52) | (mant & ((1u64 << 52) - 1)))
}

/// IEEE 754 binary16 bits of `x`, rounded to nearest, ties to even, directly
/// from the double (no double rounding through f32).
pub(crate) fn f64_zu_f16(x: f64) -> u16 {
    let vorzeichen = ((x.to_bits() >> 48) & 0x8000) as u16;
    if x.is_nan() {
        return 0x7E00;
    }
    let a = x.abs();
    if a.is_infinite() || a >= 65520.0 {
        return vorzeichen | 0x7C00;
    }
    if a < 6.103515625e-05 {
        // Subnormal (or zero): a multiple of 2^-24; 1024 rounds up into the
        // smallest normal number, whose encoding is the next bit pattern.
        let m = (a * 16_777_216.0).round_ties_even();
        return vorzeichen | m as u16;
    }
    let e = ((a.to_bits() >> 52) & 0x7FF) as i32 - 1023;
    let mantisse = a / 2f64.powi(e);
    let mut m = ((mantisse - 1.0) * 1024.0).round_ties_even() as u32;
    let mut e = e;
    if m == 1024 {
        m = 0;
        e += 1;
    }
    if e > 15 {
        return vorzeichen | 0x7C00;
    }
    vorzeichen | (((e + 15) as u16) << 10) | m as u16
}

/// The double value of binary16 bits.
pub(crate) fn f16_zu_f64(h: u16) -> f64 {
    let vorzeichen = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1F) as i32;
    let m = (h & 0x3FF) as f64;
    vorzeichen
        * match e {
            0 => m * 2f64.powi(-24),
            31 => {
                if m == 0.0 {
                    f64::INFINITY
                } else {
                    f64::NAN
                }
            }
            _ => (1.0 + m / 1024.0) * 2f64.powi(e - 15),
        }
}

impl Engine {
    /// `Float16Array` (built like the other typed array constructors),
    /// `Math.f16round`, `DataView.prototype.getFloat16/setFloat16`.
    /// MEASURED before: all four missing (`typeof Float16Array` was
    /// "undefined").
    fn es_binaer_aufbauen(&mut self) -> Result<(), Stoerung> {
        use EsOp::*;
        self.es_methode(self.proto_dataview, "getFloat16", DvGetFloat16);
        self.es_methode(self.proto_dataview, "setFloat16", DvSetFloat16);
        if let Some(math) = self.es_global("Math") {
            self.es_methode(math, "f16round", MatheF16Runden);
        }
        Ok(())
    }
}

// ===========================================================================
// Release 15.10.2026: Atomics, Uint8Array base64/hex, resizable buffers.
// ===========================================================================

/// The private key of a resizable buffer's maximum byte length.
const H_MAX: &str = "#es_max";
/// A length-tracking view (TypedArray or DataView) over a resizable buffer.
const H_SPUR: &str = "#es_spur";

/// The `Atomics.waitAsync` waiter, as JavaScript (it needs a Promise and a
/// timer): `register(w)` keeps `w`, `w.res(v)` settles it once.
const WARTEN_ASYNC: &str = r#"(function waitAsync(register, timeout) {
  'use strict';
  return new Promise(function (res) {
    var w = { done: false, res: function (v) { if (!w.done) { w.done = true; res(v); } } };
    register(w);
    if (timeout !== Infinity) setTimeout(function () { w.res('timed-out'); }, timeout);
  });
})"#;

impl Engine {
    fn es_d_ops(&mut self, op: EsOp, fn_ref: HeapRef, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        use EsOp::*;
        let _ = fn_ref;
        match op {
            AtomAdd => self.es_atomics(args, 0),
            AtomSub => self.es_atomics(args, 1),
            AtomAnd => self.es_atomics(args, 2),
            AtomOr => self.es_atomics(args, 3),
            AtomXor => self.es_atomics(args, 4),
            AtomXchg => self.es_atomics(args, 5),
            AtomStore => self.es_atomics(args, 6),
            AtomLoad => self.es_atomics(args, 7),
            AtomCmpXchg => self.es_atomics(args, 8),
            AtomIsLockFree => {
                let n = self.zu_zahl(self.arg(args, 0))?;
                Ok(GVal::Bool(matches!(n, 1.0 | 2.0 | 4.0 | 8.0)))
            }
            AtomNotify => self.es_atomics_notify(args),
            AtomWait => self.es_atomics_wait(args, false),
            AtomWaitAsync => self.es_atomics_wait(args, true),
            AtomPause => {
                // ES2026: undefined or an integral Number.
                match self.arg(args, 0) {
                    GVal::Undefiniert => Ok(GVal::Undefiniert),
                    GVal::Zahl(z) if z.is_finite() && z.trunc() == z => Ok(GVal::Undefiniert),
                    _ => Err(self.es_typfehler("Atomics.pause: the argument must be an integral number")),
                }
            }
            U8FromBase64 => self.es_from_base64(args, None),
            U8SetFromBase64 => self.es_from_base64(args, Some(this)),
            U8FromHex => self.es_from_hex(args, None),
            U8SetFromHex => self.es_from_hex(args, Some(this)),
            U8ToBase64 => self.es_to_base64(this, args),
            U8ToHex => {
                let bytes = self.es_uint8_bytes(this)?;
                let mut s = String::with_capacity(bytes.len() * 2);
                for b in bytes {
                    s.push_str(&format!("{b:02x}"));
                }
                self.text_wert(&s).map_err(Stoerung::Fehler)
            }
            PufferResizable => {
                let r = self.es_puffer(this)?;
                Ok(GVal::Bool(self.es_privat(r, H_MAX).is_some()))
            }
            PufferMax => {
                let r = self.es_puffer(this)?;
                // A detached buffer reports 0 (ES2024 25.1.6.4 step 4).
                if matches!(self.heap.holen(r), HObj::BytePuffer { detached: true, .. }) {
                    return Ok(GVal::Zahl(0.0));
                }
                match self.es_privat(r, H_MAX) {
                    Some(m) => Ok(m),
                    None => Ok(GVal::Zahl(self.es_puffer_laenge(r) as f64)),
                }
            }
            PufferResize => self.es_puffer_resize(this, args, false),
            SabGrow => self.es_puffer_resize(this, args, true),
            PufferTransferEs => self.es_transfer(this, args, false),
            PufferTransferFest => self.es_transfer(this, args, true),
            _ => Err(self.es_typfehler("internal: not a binary data native")),
        }
    }

    // ---------------------------------------------------------------
    // Atomics (ECMA-262 25.4)
    // ---------------------------------------------------------------

    /// ValidateIntegerTypedArray + ValidateAtomicAccess: the array, its kind,
    /// the byte position of the element and its buffer.
    fn es_atomics_ziel(
        &mut self,
        ta: GVal,
        idx: GVal,
        warten: bool,
    ) -> Result<(crate::eingebaut_typed::ElementArt, usize, HeapRef), Stoerung> {
        use crate::eingebaut_typed::ElementArt as A;
        let ist_ta = matches!(ta, GVal::Ref(r) if matches!(self.heap.holen(r), HObj::TypedArray { .. }));
        if !ist_ta {
            return Err(self.es_typfehler("Atomics: the argument is not an integer TypedArray"));
        }
        let (_, art, versatz, laenge, puffer) = self.typedarray_this(ta)?;
        let erlaubt = if warten {
            matches!(art, A::Int32 | A::BigInt64)
        } else {
            !matches!(art, A::Float32 | A::Float64 | A::Float16 | A::Uint8Clamped)
        };
        if !erlaubt {
            return Err(self.es_typfehler("Atomics: the argument is not an integer TypedArray"));
        }
        if matches!(self.heap.holen(puffer), HObj::BytePuffer { detached: true, .. }) {
            return Err(self.es_typfehler("Atomics: the array buffer is detached"));
        }
        let i = self.typedarray_index(idx)?;
        if i >= laenge {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "Atomics: index out of range")));
        }
        Ok((art, versatz + i * art.bytes(), puffer))
    }

    /// The raw bits of the element (little endian, zero-extended).
    fn es_atom_lesen(&self, puffer: HeapRef, bei: usize, n: usize) -> u64 {
        let mut b = [0u8; 8];
        if let HObj::BytePuffer { daten, .. } = self.heap.holen(puffer)
            && bei + n <= daten.len()
        {
            b[..n].copy_from_slice(&daten[bei..bei + n]);
        }
        u64::from_le_bytes(b)
    }

    fn es_atom_schreiben(&mut self, puffer: HeapRef, bei: usize, n: usize, bits: u64) {
        if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(puffer)
            && bei + n <= daten.len()
        {
            daten[bei..bei + n].copy_from_slice(&bits.to_le_bytes()[..n]);
        }
    }

    /// A value as element bits (ToIntegerOrInfinity resp. ToBigInt, modulo
    /// 2^width) and the value the operation reports for it.
    fn es_atom_wert(&mut self, art: crate::eingebaut_typed::ElementArt, v: GVal) -> Result<(u64, GVal), Stoerung> {
        match self.typedarray_schreibwert(art, v)? {
            crate::eingebaut_typed::Schreibwert::Ganz(g) => Ok((g as u64, GVal::Ganz(g))),
            crate::eingebaut_typed::Schreibwert::Zahl(z) => {
                let t = if z.is_nan() { 0.0 } else { z.trunc() };
                let bits = if t.is_finite() { t.rem_euclid(18_446_744_073_709_551_616.0) as u64 } else { 0 };
                Ok((bits, GVal::Zahl(if t == 0.0 { 0.0 } else { t })))
            }
        }
    }

    /// add (0), sub (1), and (2), or (3), xor (4), exchange (5), store (6),
    /// load (7), compareExchange (8).
    fn es_atomics(&mut self, args: &[GVal], art_op: u8) -> Result<GVal, Stoerung> {
        let (art, bei, puffer) = self.es_atomics_ziel(self.arg(args, 0), self.arg(args, 1), false)?;
        let n = art.bytes();
        let maske = if n == 8 { u64::MAX } else { (1u64 << (n * 8)) - 1 };
        if art_op == 7 {
            return Ok(self.es_atom_alt(art, puffer, bei));
        }
        let (v, v_wert) = self.es_atom_wert(art, self.arg(args, 2))?;
        let ersatz = if art_op == 8 { Some(self.es_atom_wert(art, self.arg(args, 3))?.0) } else { None };
        // The conversion may have run user code that detached the buffer.
        if matches!(self.heap.holen(puffer), HObj::BytePuffer { detached: true, .. }) {
            return Err(self.es_typfehler("Atomics: the array buffer is detached"));
        }
        let alt_wert = self.es_atom_alt(art, puffer, bei);
        let alt = self.es_atom_lesen(puffer, bei, n);
        let neu = match art_op {
            0 => alt.wrapping_add(v),
            1 => alt.wrapping_sub(v),
            2 => alt & v,
            3 => alt | v,
            4 => alt ^ v,
            5 | 6 => v,
            _ => {
                if alt & maske == v & maske {
                    ersatz.unwrap_or(alt)
                } else {
                    alt
                }
            }
        };
        self.es_atom_schreiben(puffer, bei, n, neu & maske);
        Ok(if art_op == 6 { v_wert } else { alt_wert })
    }

    fn es_atom_alt(&self, art: crate::eingebaut_typed::ElementArt, puffer: HeapRef, bei: usize) -> GVal {
        match self.heap.holen(puffer) {
            HObj::BytePuffer { daten, .. } => art.lesen(daten, bei),
            _ => GVal::Undefiniert,
        }
    }

    /// Atomics.wait (sync: the main thread cannot suspend, a TypeError as in
    /// browsers, ECMA-262 25.4.3.14 step 9) and Atomics.waitAsync.
    fn es_atomics_wait(&mut self, args: &[GVal], asynchron: bool) -> Result<GVal, Stoerung> {
        let (art, bei, puffer) = self.es_atomics_ziel(self.arg(args, 0), self.arg(args, 1), true)?;
        if !matches!(self.heap.holen(puffer), HObj::BytePuffer { geteilt: true, .. }) {
            return Err(self.es_typfehler("Atomics.wait cannot be called on a non-shared buffer"));
        }
        let (v, _) = self.es_atom_wert(art, self.arg(args, 2))?;
        let t = match self.arg(args, 3) {
            GVal::Undefiniert => f64::INFINITY,
            x => {
                let z = self.zu_zahl(x)?;
                if z.is_nan() { f64::INFINITY } else { z.max(0.0) }
            }
        };
        if !asynchron {
            return Err(self.es_typfehler("Atomics.wait cannot be called in this context"));
        }
        let n = art.bytes();
        let maske = if n == 8 { u64::MAX } else { (1u64 << (n * 8)) - 1 };
        let ergebnis = |e: &mut Engine, asy: bool, wert: GVal| -> Result<GVal, Stoerung> {
            let o = e.neues_objekt(Some(e.proto_objekt)).map_err(Stoerung::Fehler)?;
            e.wurzeln.push(GVal::Ref(o));
            e.wurzeln.push(wert);
            e.setze(GVal::Ref(o), "async", GVal::Bool(asy))?;
            e.setze(GVal::Ref(o), "value", wert)?;
            Ok(GVal::Ref(o))
        };
        if self.es_atom_lesen(puffer, bei, n) & maske != v & maske {
            let t = self.text_wert("not-equal").map_err(Stoerung::Fehler)?;
            return ergebnis(self, false, t);
        }
        if t == 0.0 {
            let t = self.text_wert("timed-out").map_err(Stoerung::Fehler)?;
            return ergebnis(self, false, t);
        }
        let f = self.es_glue(WARTEN_ASYNC, 1)?;
        let reg = self.nativ_anlegen("register", crate::builtin::Nativ::Es(EsOp::AtomRegister), self.proto_funktion);
        self.es.warte_ort = Some((puffer, bei));
        let p = self.aufruf_synchron(GVal::Ref(f), GVal::Undefiniert, &[GVal::Ref(reg), GVal::Zahl(t)], false);
        self.es.warte_ort = None;
        let p = p?;
        ergebnis(self, true, p)
    }

    /// Atomics.notify: wakes `waitAsync` waiters of that element (count
    /// defaults to all); 0 on a non-shared buffer.
    fn es_atomics_notify(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let (_, bei, puffer) = self.es_atomics_ziel(self.arg(args, 0), self.arg(args, 1), true)?;
        let anzahl = match self.arg(args, 2) {
            GVal::Undefiniert => f64::INFINITY,
            x => {
                let z = self.zu_zahl(x)?;
                if z.is_nan() { 0.0 } else { z.trunc().max(0.0) }
            }
        };
        if !matches!(self.heap.holen(puffer), HObj::BytePuffer { geteilt: true, .. }) {
            return Ok(GVal::Zahl(0.0));
        }
        let mut geweckt = 0.0;
        let mut i = 0;
        while i < self.es.warter.len() && geweckt < anzahl {
            let (p, b, w) = self.es.warter[i];
            let fertig = matches!(self.hole(GVal::Ref(w), "done")?, GVal::Bool(true));
            if fertig {
                self.es.warter.remove(i);
                continue;
            }
            if p == puffer && b == bei {
                self.es.warter.remove(i);
                let res = self.hole(GVal::Ref(w), "res")?;
                let ok = self.text_wert("ok").map_err(Stoerung::Fehler)?;
                self.aufruf_synchron(res, GVal::Ref(w), &[ok], false)?;
                geweckt += 1.0;
                continue;
            }
            i += 1;
        }
        Ok(GVal::Zahl(geweckt))
    }

    /// A lazily compiled JavaScript helper (`slot` 0 = fromAsync, 1 =
    /// waitAsync, 2 = the RegExp protocol; slot 3 holds its RegExpExec native).
    pub(crate) fn es_glue(&mut self, quelle: &str, slot: usize) -> Result<HeapRef, Stoerung> {
        if let Some(f) = self.es.glue.get(slot).copied().flatten() {
            return Ok(f);
        }
        let vorher = self.eval_global;
        self.eval_global = true;
        let f = self.quelltext_ausfuehren(quelle);
        self.eval_global = vorher;
        let GVal::Ref(f) = f? else {
            return Err(self.es_typfehler("internal helper unavailable"));
        };
        if self.es.glue.len() <= slot {
            self.es.glue.resize(slot + 1, None);
        }
        self.es.glue[slot] = Some(f);
        Ok(f)
    }

    // ---------------------------------------------------------------
    // Uint8Array base64 / hex (ES2026)
    // ---------------------------------------------------------------

    /// The bytes of a Uint8Array `this` (TypeError otherwise or detached).
    fn es_uint8_bytes(&mut self, this: GVal) -> Result<Vec<u8>, Stoerung> {
        let ok = matches!(this, GVal::Ref(r) if matches!(self.heap.holen(r),
            HObj::TypedArray { art: crate::eingebaut_typed::ElementArt::Uint8, .. }));
        if !ok {
            return Err(self.es_typfehler("Uint8Array method called on an incompatible receiver"));
        }
        let (_, _, versatz, laenge, puffer) = self.typedarray_this(this)?;
        match self.heap.holen(puffer) {
            HObj::BytePuffer { detached: true, .. } => Err(self.es_typfehler("the array buffer is detached")),
            HObj::BytePuffer { daten, .. } => Ok(daten.get(versatz..versatz + laenge).map(|s| s.to_vec()).unwrap_or_default()),
            _ => Ok(Vec::new()),
        }
    }

    /// A string option of `opts` (undefined -> `vorgabe`, others must be one of `erlaubt`).
    fn es_option(&mut self, opts: GVal, name: &str, vorgabe: &str, erlaubt: &[&str]) -> Result<String, Stoerung> {
        if matches!(opts, GVal::Undefiniert) {
            return Ok(vorgabe.to_string());
        }
        let v = self.hole(opts, name)?;
        match v {
            GVal::Undefiniert => Ok(vorgabe.to_string()),
            GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Zeichenkette(_)) => {
                let s = self.zu_text(v)?;
                if erlaubt.contains(&s.as_str()) {
                    Ok(s)
                } else {
                    Err(self.es_typfehler(&format!("invalid {name} option: {s}")))
                }
            }
            _ => Err(self.es_typfehler(&format!("the {name} option must be a string"))),
        }
    }

    fn es_optionen(&mut self, v: GVal) -> Result<GVal, Stoerung> {
        match v {
            GVal::Undefiniert => Ok(v),
            _ if self.ist_objekt(v) => Ok(v),
            _ => Err(self.es_typfehler("options must be an object")),
        }
    }

    fn es_text_arg(&mut self, v: GVal) -> Result<String, Stoerung> {
        match v {
            GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Zeichenkette(_)) => self.zu_text(v),
            _ => Err(self.es_typfehler("the argument must be a string")),
        }
    }

    /// `{read, written}`.
    fn es_gelesen_geschrieben(&mut self, read: usize, written: usize) -> Result<GVal, Stoerung> {
        let o = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(o));
        self.setze(GVal::Ref(o), "read", GVal::Zahl(read as f64))?;
        self.setze(GVal::Ref(o), "written", GVal::Zahl(written as f64))?;
        Ok(GVal::Ref(o))
    }

    /// A new Uint8Array with these bytes.
    fn es_neues_uint8(&mut self, bytes: &[u8]) -> Result<GVal, Stoerung> {
        let Some(ctor) = self.es_global("Uint8Array") else {
            return Err(self.es_typfehler("Uint8Array is unavailable"));
        };
        let a = self.typedarray_ctor(ctor, &[GVal::Zahl(bytes.len() as f64)], crate::eingebaut_typed::ElementArt::Uint8)?;
        self.es_uint8_setzen(a, bytes)?;
        Ok(a)
    }

    fn es_uint8_setzen(&mut self, ziel: GVal, bytes: &[u8]) -> Result<(), Stoerung> {
        let (_, _, versatz, laenge, puffer) = self.typedarray_this(ziel)?;
        if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(puffer) {
            let n = bytes.len().min(laenge);
            if versatz + n <= daten.len() {
                daten[versatz..versatz + n].copy_from_slice(&bytes[..n]);
            }
        }
        Ok(())
    }

    /// Uint8Array.fromBase64 / Uint8Array.prototype.setFromBase64.
    fn es_from_base64(&mut self, args: &[GVal], ziel: Option<GVal>) -> Result<GVal, Stoerung> {
        let max = match ziel {
            Some(t) => Some(self.es_uint8_bytes(t)?.len()),
            None => None,
        };
        let s = self.es_text_arg(self.arg(args, 0))?;
        let opts = self.es_optionen(self.arg(args, 1))?;
        let alphabet = self.es_option(opts, "alphabet", "base64", &["base64", "base64url"])?;
        let letzter = self.es_option(opts, "lastChunkHandling", "loose", &["loose", "strict", "stop-before-partial"])?;
        let einheiten: Vec<u16> = s.encode_utf16().collect();
        let (read, bytes, fehler) = base64_lesen(&einheiten, alphabet == "base64url", &letzter, max.unwrap_or(usize::MAX));
        match ziel {
            None => {
                if let Some(m) = fehler {
                    return Err(Stoerung::Wurf(self.fehler_wert("SyntaxError", m)));
                }
                self.es_neues_uint8(&bytes)
            }
            Some(t) => {
                self.es_uint8_setzen(t, &bytes)?;
                if let Some(m) = fehler {
                    return Err(Stoerung::Wurf(self.fehler_wert("SyntaxError", m)));
                }
                self.es_gelesen_geschrieben(read, bytes.len())
            }
        }
    }

    /// Uint8Array.fromHex / Uint8Array.prototype.setFromHex.
    fn es_from_hex(&mut self, args: &[GVal], ziel: Option<GVal>) -> Result<GVal, Stoerung> {
        let max = match ziel {
            Some(t) => self.es_uint8_bytes(t)?.len(),
            None => usize::MAX,
        };
        let s = self.es_text_arg(self.arg(args, 0))?;
        let e: Vec<u16> = s.encode_utf16().collect();
        let mut bytes = Vec::new();
        let mut read = 0;
        let mut fehler = None;
        if e.len() % 2 != 0 {
            fehler = Some("the hex string must have an even length");
        } else {
            let hex = |u: u16| -> Option<u8> { char::from_u32(u as u32).and_then(|c| c.to_digit(16)).map(|d| d as u8) };
            while read < e.len() && bytes.len() < max {
                match (hex(e[read]), hex(e[read + 1])) {
                    (Some(a), Some(b)) => bytes.push(a * 16 + b),
                    _ => {
                        fehler = Some("the string contains a non-hex character");
                        break;
                    }
                }
                read += 2;
            }
        }
        match ziel {
            None => {
                if let Some(m) = fehler {
                    return Err(Stoerung::Wurf(self.fehler_wert("SyntaxError", m)));
                }
                self.es_neues_uint8(&bytes)
            }
            Some(t) => {
                self.es_uint8_setzen(t, &bytes)?;
                if let Some(m) = fehler {
                    return Err(Stoerung::Wurf(self.fehler_wert("SyntaxError", m)));
                }
                self.es_gelesen_geschrieben(read, bytes.len())
            }
        }
    }

    /// Uint8Array.prototype.toBase64({alphabet, omitPadding}).
    fn es_to_base64(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let _ = self.es_uint8_bytes(this)?;
        let opts = self.es_optionen(self.arg(args, 0))?;
        let alphabet = self.es_option(opts, "alphabet", "base64", &["base64", "base64url"])?;
        let ohne = if matches!(opts, GVal::Undefiniert) {
            false
        } else {
            let v = self.hole(opts, "omitPadding")?;
            self.zu_bool(v)
        };
        // The options may have run user code: read the bytes afterwards.
        let bytes = self.es_uint8_bytes(this)?;
        let tab: &[u8; 64] = if alphabet == "base64url" {
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        } else {
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        };
        let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for c in bytes.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            let zeichen = c.len() + 1;
            for i in 0..4 {
                if i < zeichen {
                    s.push(tab[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else if !ohne {
                    s.push('=');
                }
            }
        }
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    // ---------------------------------------------------------------
    // Resizable ArrayBuffer, growable SharedArrayBuffer (ES2024)
    // ---------------------------------------------------------------

    fn es_puffer(&mut self, this: GVal) -> Result<HeapRef, Stoerung> {
        match this {
            GVal::Ref(r) if matches!(self.heap.holen(r), HObj::BytePuffer { .. }) => Ok(r),
            _ => Err(self.es_typfehler("not an ArrayBuffer")),
        }
    }

    fn es_puffer_laenge(&self, r: HeapRef) -> usize {
        match self.heap.holen(r) {
            HObj::BytePuffer { daten, .. } => daten.len(),
            _ => 0,
        }
    }

    /// The maxByteLength option of `new ArrayBuffer(n, {maxByteLength})`;
    /// `Some(max)` makes the buffer resizable (growable for a SAB).
    pub(crate) fn es_puffer_max_option(&mut self, opts: GVal, laenge: usize) -> Result<Option<usize>, Stoerung> {
        if !self.ist_objekt(opts) {
            return Ok(None);
        }
        let m = self.hole(opts, "maxByteLength")?;
        if matches!(m, GVal::Undefiniert) {
            return Ok(None);
        }
        let max = self.typedarray_index(m)?;
        if laenge > max {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "Invalid array buffer max length")));
        }
        if max > self.grenzen.max_heap_bytes {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "Invalid array buffer max length")));
        }
        Ok(Some(max))
    }

    /// Marks a new buffer resizable.
    pub(crate) fn es_puffer_max_setzen(&mut self, r: HeapRef, max: Option<usize>) {
        if let Some(m) = max {
            self.es_eigen(r, H_MAX, GVal::Zahl(m as f64), true, false, true);
        }
    }

    /// resize (ArrayBuffer) / grow (SharedArrayBuffer, only larger).
    fn es_puffer_resize(&mut self, this: GVal, args: &[GVal], wachsen: bool) -> Result<GVal, Stoerung> {
        let r = self.es_puffer(this)?;
        let geteilt = matches!(self.heap.holen(r), HObj::BytePuffer { geteilt: true, .. });
        if geteilt != wachsen {
            return Err(self.es_typfehler(if wachsen { "not a SharedArrayBuffer" } else { "not an ArrayBuffer" }));
        }
        let Some(GVal::Zahl(max)) = self.es_privat(r, H_MAX) else {
            return Err(self.es_typfehler(if wachsen { "SharedArrayBuffer is not growable" } else { "ArrayBuffer is not resizable" }));
        };
        let neu = self.typedarray_index(self.arg(args, 0))?;
        if matches!(self.heap.holen(r), HObj::BytePuffer { detached: true, .. }) {
            return Err(self.es_typfehler("ArrayBuffer is detached"));
        }
        let alt = self.es_puffer_laenge(r);
        if neu > max as usize || wachsen && neu < alt {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "Invalid array buffer length")));
        }
        if neu > alt {
            self.vor_allok(neu - alt).map_err(Stoerung::Fehler)?;
        }
        if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(r) {
            daten.resize(neu, 0);
        }
        self.es_spur_nachfuehren(r, neu);
        Ok(GVal::Undefiniert)
    }

    /// Length-tracking TypedArrays over `puffer` follow its new length.
    fn es_spur_nachfuehren(&mut self, puffer: HeapRef, neu: usize) {
        for i in 0..self.heap.slot_zahl() {
            let r = HeapRef(i as u32);
            if !self.heap.lebt(r) {
                continue;
            }
            if let HObj::TypedArray { puffer: p, versatz, laenge, art, eigenschaften, .. } = self.heap.holen_mut(r)
                && *p == puffer
                && eigenschaften.contains_key(H_SPUR)
            {
                *laenge = neu.saturating_sub(*versatz) / art.bytes();
            }
        }
    }

    /// After `new TA(buffer)` / `new DataView(buffer)`: without an explicit
    /// length over a resizable buffer, the view tracks the buffer's length.
    pub(crate) fn es_spur_markieren(&mut self, sicht: GVal, args: &[GVal]) {
        let (GVal::Ref(v), GVal::Ref(b)) = (sicht, self.arg(args, 0)) else { return };
        if !matches!(self.arg(args, 2), GVal::Undefiniert) {
            return;
        }
        if matches!(self.heap.holen(b), HObj::BytePuffer { .. }) && self.es_privat(b, H_MAX).is_some() {
            self.es_eigen(v, H_SPUR, GVal::Bool(true), true, false, true);
        }
    }

    /// The tracked byte length of a DataView (`None`: not tracking).
    pub(crate) fn es_dataview_spur(&self, dv: HeapRef, puffer: HeapRef, versatz: usize) -> Option<usize> {
        self.es_privat(dv, H_SPUR)?;
        Some(self.es_puffer_laenge(puffer).saturating_sub(versatz))
    }

    /// transfer (keeps resizability) / transferToFixedLength.
    fn es_transfer(&mut self, this: GVal, args: &[GVal], fest: bool) -> Result<GVal, Stoerung> {
        let r = self.es_puffer(this)?;
        let max = match self.es_privat(r, H_MAX) {
            Some(GVal::Zahl(m)) if !fest => Some(m as usize),
            _ => None,
        };
        if let Some(m) = max
            && !matches!(self.arg(args, 0), GVal::Undefiniert)
        {
            let n = self.typedarray_index(self.arg(args, 0))?;
            if n > m {
                return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "Invalid array buffer length")));
            }
        }
        let neu = self.n_puffer_transfer(this, args)?;
        if let GVal::Ref(n) = neu {
            self.es_puffer_max_setzen(n, max);
        }
        Ok(neu)
    }
}

/// FromBase64 (Uint8Array base64 proposal): read units, bytes, an error.
fn base64_lesen(s: &[u16], url: bool, letzter: &str, max: usize) -> (usize, Vec<u8>, Option<&'static str>) {
    let mut bytes: Vec<u8> = Vec::new();
    if max == 0 {
        return (0, bytes, None);
    }
    let leer = |u: u16| matches!(u, 0x09 | 0x0A | 0x0C | 0x0D | 0x20);
    let ueberspringen = |mut i: usize| {
        while i < s.len() && leer(s[i]) {
            i += 1;
        }
        i
    };
    let wert = |u: u16| -> Option<u32> {
        match u {
            0x41..=0x5A => Some(u as u32 - 0x41),
            0x61..=0x7A => Some(u as u32 - 0x61 + 26),
            0x30..=0x39 => Some(u as u32 - 0x30 + 52),
            0x2B if !url => Some(62),
            0x2F if !url => Some(63),
            0x2D if url => Some(62),
            0x5F if url => Some(63),
            _ => None,
        }
    };
    // DecodeBase64Chunk: `extra` = the overflow bits must be zero.
    let block = |c: &[u32], extra: bool, bytes: &mut Vec<u8>| -> Result<(), &'static str> {
        let mut n = 0u32;
        for i in 0..4 {
            n = n << 6 | c.get(i).copied().unwrap_or(0);
        }
        let b = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        match c.len() {
            2 => {
                if extra && b[1] != 0 {
                    return Err("extra bits in the last base64 chunk");
                }
                bytes.push(b[0]);
            }
            3 => {
                if extra && b[2] != 0 {
                    return Err("extra bits in the last base64 chunk");
                }
                bytes.extend_from_slice(&b[..2]);
            }
            _ => bytes.extend_from_slice(&b),
        }
        Ok(())
    };
    let mut read = 0;
    let mut chunk: Vec<u32> = Vec::with_capacity(4);
    let mut i = 0;
    loop {
        i = ueberspringen(i);
        if i == s.len() {
            if !chunk.is_empty() {
                match letzter {
                    "stop-before-partial" => return (read, bytes, None),
                    "loose" => {
                        if chunk.len() == 1 {
                            return (read, bytes, Some("a base64 string cannot end with a single character"));
                        }
                        let _ = block(&chunk, false, &mut bytes);
                    }
                    _ => return (read, bytes, Some("the last base64 chunk is incomplete")),
                }
            }
            return (s.len(), bytes, None);
        }
        let c = s[i];
        i += 1;
        if c == 0x3D {
            if chunk.len() < 2 {
                return (read, bytes, Some("unexpected padding in the base64 string"));
            }
            i = ueberspringen(i);
            if chunk.len() == 2 {
                if i == s.len() {
                    if letzter == "stop-before-partial" {
                        return (read, bytes, None);
                    }
                    return (read, bytes, Some("incomplete base64 padding"));
                }
                if s[i] == 0x3D {
                    i = ueberspringen(i + 1);
                }
            }
            if i < s.len() {
                return (read, bytes, Some("characters after the base64 padding"));
            }
            if let Err(m) = block(&chunk, letzter == "strict", &mut bytes) {
                return (read, bytes, Some(m));
            }
            return (s.len(), bytes, None);
        }
        let Some(w) = wert(c) else {
            return (read, bytes, Some("the string contains a non-base64 character"));
        };
        let rest = max - bytes.len();
        if rest == 1 && chunk.len() == 2 || rest == 2 && chunk.len() == 3 {
            return (read, bytes, None);
        }
        chunk.push(w);
        if chunk.len() == 4 {
            let _ = block(&chunk, false, &mut bytes);
            chunk.clear();
            read = i;
            if bytes.len() == max {
                return (read, bytes, None);
            }
        }
    }
}

impl Engine {
    /// Release 15.10.2026: Atomics, Uint8Array base64/hex, resizable
    /// buffers. MEASURED before: `Atomics` was not defined, `toBase64`,
    /// `resize` and `grow` were missing, `resizable` was always false.
    fn es_binaer_d_aufbauen(&mut self) -> Result<(), Stoerung> {
        use EsOp::*;
        if let Some(u8c) = self.es_global("Uint8Array") {
            self.es_methode(u8c, "fromBase64", U8FromBase64);
            self.es_methode(u8c, "fromHex", U8FromHex);
            if let Some(GVal::Ref(p)) = self.eigene_lese(u8c, "prototype") {
                self.es_methode(p, "toBase64", U8ToBase64);
                self.es_methode(p, "toHex", U8ToHex);
                self.es_methode(p, "setFromBase64", U8SetFromBase64);
                self.es_methode(p, "setFromHex", U8SetFromHex);
            }
        }
        let getter = |e: &mut Engine, proto: HeapRef, name: &str, op: EsOp| {
            let f = e.nativ_anlegen(&format!("get {name}"), crate::builtin::Nativ::Es(op), e.proto_funktion);
            e.es_eigen(proto, name, GVal::Undefiniert, false, false, true);
            if let Some(map) = e.eigenschaften_karte_mut(proto)
                && let Some(x) = map.get_mut(name)
            {
                x.getter = Some(f);
            }
        };
        let ap = self.proto_puffer;
        getter(self, ap, "resizable", PufferResizable);
        getter(self, ap, "maxByteLength", PufferMax);
        self.es_methode(ap, "resize", PufferResize);
        self.es_methode(ap, "transfer", PufferTransferEs);
        self.es_methode(ap, "transferToFixedLength", PufferTransferFest);
        let sp = self.proto_shared_puffer;
        getter(self, sp, "growable", PufferResizable);
        getter(self, sp, "maxByteLength", PufferMax);
        self.es_methode(sp, "grow", SabGrow);
        Ok(())
    }

    /// `register(w)` for `Atomics.waitAsync`: remembers the waiter for the
    /// element being waited on.
    fn es_warter_merken(&mut self, w: GVal) -> Result<GVal, Stoerung> {
        if let (Some((p, b)), GVal::Ref(w)) = (self.es.warte_ort, w) {
            self.es.warter.push((p, b, w));
        }
        Ok(GVal::Undefiniert)
    }
}

impl Engine {
    /// A private-key property of any kind with a property map (buffers and
    /// typed arrays included, which `eigene_lese` does not read).
    fn es_privat(&self, r: HeapRef, k: &str) -> Option<GVal> {
        self.eigene_eigenschaft(r, k).map(|e| e.wert)
    }
}

impl Engine {
    /// thisBigIntValue (21.2.3): a BigInt or its wrapper.
    fn es_bigint_wert(&mut self, this: GVal, name: &str) -> Result<i128, Stoerung> {
        match this {
            GVal::Ganz(g) => Ok(g),
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Verpackt { primitiv: GVal::Ganz(g), .. } => Ok(*g),
                _ => Err(self.es_typfehler(&format!("BigInt.prototype.{name} requires that 'this' be a BigInt"))),
            },
            _ => Err(self.es_typfehler(&format!("BigInt.prototype.{name} requires that 'this' be a BigInt"))),
        }
    }

    /// thisSymbolValue (20.4.3): a symbol or its wrapper.
    fn es_symbol_wert(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        if let GVal::Ref(r) = this {
            if crate::symbole::ist_symbol(self, r) {
                return Ok(this);
            }
            if let HObj::Verpackt { primitiv: GVal::Ref(s), .. } = self.heap.holen(r)
                && crate::symbole::ist_symbol(self, *s)
            {
                return Ok(GVal::Ref(*s));
            }
        }
        Err(self.es_typfehler("Symbol.prototype method called on an incompatible receiver"))
    }

    /// Release 15.10.2026: BigInt.prototype.valueOf returns the BigInt (it
    /// was toString: `(1n).valueOf() === 1n` was false), toString/valueOf of
    /// BigInt and Symbol take their wrapper objects, Symbol.prototype gets
    /// valueOf and @@toPrimitive.
    fn es_huellen_aufbauen(&mut self) -> Result<(), Stoerung> {
        use EsOp::*;
        self.es_methode(self.proto_bigint, "valueOf", BigIntWertVon);
        self.es_methode(self.proto_bigint, "toString", BigIntZuText);
        let sp = self.proto_symbol;
        self.es_methode(sp, "valueOf", SymWertVon);
        self.es_methode(sp, "toString", SymZuText);
        let f = self.nativ_anlegen("[Symbol.toPrimitive]", crate::builtin::Nativ::Es(SymWertVon), self.proto_funktion);
        let k = self.bekanntes_symbol_text(crate::builtin::Nativ::SymToPrimitive)?;
        self.es_eigen(sp, &k, GVal::Ref(f), false, false, true);
        let g = self.nativ_anlegen("get description", crate::builtin::Nativ::Es(SymBeschreibungEs), self.proto_funktion);
        if let Some(map) = self.eigenschaften_karte_mut(sp)
            && let Some(e) = map.get_mut("description")
        {
            e.getter = Some(g);
        }
        let tag = self.bekanntes_symbol_text(crate::builtin::Nativ::SymToStringTag)?;
        if self.eigene_lese(self.proto_bigint, &tag).is_none() {
            let t = self.text_wert("BigInt").map_err(Stoerung::Fehler)?;
            self.es_eigen(self.proto_bigint, &tag, t, false, false, true);
        }
        if self.eigene_lese(sp, &tag).is_none() {
            let t = self.text_wert("Symbol").map_err(Stoerung::Fehler)?;
            self.es_eigen(sp, &tag, t, false, false, true);
        }
        Ok(())
    }
}

impl Engine {
    /// Release 15.10.2026: `Atomics` and `Float16Array` are built on first
    /// use of either global (`web_platform.rs` lazy globals, `Api::Es`).
    /// MEASURED: the ES2022-2026 built-ins grew the heap after
    /// `Engine::neu` by 38 KB (485 -> 523 KB accounted bytes), and every page,
    /// frame and worker pays that; these two namespaces were 9 KB of it and
    /// most pages never touch them.
    pub(crate) fn es_lazy_laden(&mut self) -> Result<(), Stoerung> {
        use EsOp::*;
        let marke = self.wurzel_merke();
        let r = (|| {
            self.es_float16_bauen()?;
            let atomics = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
            self.wurzeln.push(GVal::Ref(atomics));
            for (n, op) in [
                ("add", AtomAdd),
                ("and", AtomAnd),
                ("compareExchange", AtomCmpXchg),
                ("exchange", AtomXchg),
                ("isLockFree", AtomIsLockFree),
                ("load", AtomLoad),
                ("notify", AtomNotify),
                ("or", AtomOr),
                ("pause", AtomPause),
                ("store", AtomStore),
                ("sub", AtomSub),
                ("wait", AtomWait),
                ("waitAsync", AtomWaitAsync),
                ("xor", AtomXor),
            ] {
                self.es_methode(atomics, n, op);
            }
            let tag = self.bekanntes_symbol_text(crate::builtin::Nativ::SymToStringTag)?;
            let t = self.text_wert("Atomics").map_err(Stoerung::Fehler)?;
            self.es_eigen(atomics, &tag, t, false, false, true);
            self.global_binden("Atomics", GVal::Ref(atomics));
            self.es_schwach_bauen()?;
            if self.eigene_lese(self.global_objekt, "Intl").is_none() {
                self.intl_aufbauen()?;
            }
            Ok(())
        })();
        self.wurzel_zurueck(marke);
        r
    }

    /// Release 15.10.2026: `WeakRef` and `FinalizationRegistry` (ECMA-262
    /// 26.1, 26.2), built with the other lazy globals. MEASURED before:
    /// both missing (`typeof WeakRef` was "undefined", node 22: "function").
    ///
    /// APPROXIMATION, as for WeakMap in this engine: the target is held
    /// strongly, so `deref()` never returns undefined and cleanup callbacks
    /// never run — the spec allows an implementation to keep any target
    /// alive (26.1.3.2 / 9.10.3 "may"), so this is conformant, it only
    /// never frees.
    fn es_schwach_bauen(&mut self) -> Result<(), Stoerung> {
        use EsOp::*;
        let tag = self.bekanntes_symbol_text(crate::builtin::Nativ::SymToStringTag)?;
        for (name, ctor_op, methoden) in [
            ("WeakRef", WeakRefCtor, &[("deref", WeakRefDeref)][..]),
            ("FinalizationRegistry", FinRegCtor, &[("register", FinRegRegister), ("unregister", FinRegUnregister)][..]),
        ] {
            let proto = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
            self.wurzeln.push(GVal::Ref(proto));
            let ctor = self.nativ_anlegen(name, crate::builtin::Nativ::Es(ctor_op), self.proto_funktion);
            self.wurzeln.push(GVal::Ref(ctor));
            self.nativ_prototyp(ctor, proto);
            self.es_eigen(proto, "constructor", GVal::Ref(ctor), true, false, true);
            for (n, op) in methoden {
                self.es_methode(proto, n, *op);
            }
            let t = self.text_wert(name).map_err(Stoerung::Fehler)?;
            self.es_eigen(proto, &tag, t, false, false, true);
            self.global_binden(name, GVal::Ref(ctor));
        }
        Ok(())
    }

    /// Release 15.10.2026: the %TypedArray% intrinsic (ECMA-262 23.2.1): the
    /// abstract constructor (TypeError on call and construct) that every
    /// typed array constructor inherits from, with `from`, `of`,
    /// `@@species`, and `prototype` = %TypedArray%.prototype (whose
    /// `constructor` it is). The concrete prototypes get `BYTES_PER_ELEMENT`
    /// as data property, %TypedArray%.prototype the `@@toStringTag` getter
    /// (23.2.3.38). MEASURED before (node 22 in brackets):
    /// `Object.getPrototypeOf(Int8Array) === Function.prototype` true
    /// (false, it is %TypedArray%), `Int8Array[Symbol.species]` undefined
    /// (Int8Array), and each of the eleven constructors carried its own
    /// `from`/`of` (22 natives in the baseline heap).
    fn es_typed_aufbauen(&mut self) -> Result<(), Stoerung> {
        use EsOp::*;
        let Some(i8_ctor) = self.es_global("Int8Array") else { return Ok(()) };
        let Some(i8_proto) = self.nativ_proto.get(&i8_ctor).copied() else { return Ok(()) };
        let Some(ta_proto) = self.heap_proto_von(i8_proto) else { return Ok(()) };
        let ctor = self.nativ_anlegen("TypedArray", crate::builtin::Nativ::Es(TaAbstrakt), self.proto_funktion);
        self.wurzeln.push(GVal::Ref(ctor));
        self.nativ_prototyp(ctor, ta_proto);
        self.es_eigen(ta_proto, "constructor", GVal::Ref(ctor), true, false, true);
        for (n, nat) in [("from", crate::builtin::Nativ::TypedArrayVon), ("of", crate::builtin::Nativ::TypedArrayOf)] {
            let f = self.nativ_anlegen(n, nat, self.proto_funktion);
            self.es_eigen(ctor, n, GVal::Ref(f), true, false, true);
        }
        if let Some(g) = self.es.spezies_getter {
            self.es_eigen(ctor, crate::symbole::SYM_SPECIES, GVal::Undefiniert, false, false, true);
            if let Some(map) = self.eigenschaften_karte_mut(ctor)
                && let Some(e) = map.get_mut(crate::symbole::SYM_SPECIES)
            {
                e.getter = Some(g);
            }
        }
        let tag = self.bekanntes_symbol_text(crate::builtin::Nativ::SymToStringTag)?;
        let g = self.nativ_anlegen("get [Symbol.toStringTag]", crate::builtin::Nativ::Es(TaTagLesen), self.proto_funktion);
        self.es_eigen(ta_proto, &tag, GVal::Undefiniert, false, false, true);
        if let Some(map) = self.eigenschaften_karte_mut(ta_proto)
            && let Some(e) = map.get_mut(&tag)
        {
            e.getter = Some(g);
        }
        // BYTES_PER_ELEMENT belongs to the concrete prototypes, not here.
        if let Some(map) = self.eigenschaften_karte_mut(ta_proto) {
            map.remove("BYTES_PER_ELEMENT");
        }
        for name in [
            "Uint8Array", "Int8Array", "Uint8ClampedArray", "Uint16Array", "Int16Array", "Uint32Array",
            "Int32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array",
        ] {
            let Some(c) = self.es_global(name) else { continue };
            if let HObj::Nativ { proto, .. } = self.heap.holen_mut(c) {
                *proto = Some(ctor);
            }
            let bytes = crate::eingebaut_typed::ElementArt::aus_name(name).map_or(1, |a| a.bytes());
            if let Some(p) = self.nativ_proto.get(&c).copied() {
                self.es_eigen(p, "BYTES_PER_ELEMENT", GVal::Zahl(bytes as f64), false, false, false);
                if let Some(map) = self.eigenschaften_karte_mut(p) {
                    map.remove(&tag);
                }
            }
        }
        self.es.ta_ctor = Some(ctor);
        self.es.promise_ctor = self.es_global("Promise");
        Ok(())
    }

    /// Release 15.10.2026: the species constructor of a typed array `like`
    /// when it is not the intrinsic constructor of its element type
    /// (SpeciesConstructor, 7.3.22), else `None`.
    pub(crate) fn ta_spezies(&mut self, like: HeapRef, art: crate::eingebaut_typed::ElementArt) -> Result<Option<GVal>, Stoerung> {
        let c = self.hole(GVal::Ref(like), "constructor")?;
        if matches!(c, GVal::Undefiniert) {
            return Ok(None);
        }
        if !self.es_ist_objekt(c) {
            return Err(self.es_typfehler("object.constructor is not an object"));
        }
        let ist_intrinsisch = |e: &Engine, v: GVal| {
            matches!(v, GVal::Ref(r) if e.native.contains_key(&r)
                && matches!(e.heap.holen(r), HObj::Nativ { name, .. } if name == art.name()))
        };
        self.wurzeln.push(c);
        let s = self.hole(c, crate::symbole::SYM_SPECIES)?;
        if matches!(s, GVal::Undefiniert | GVal::Null) || ist_intrinsisch(self, s) {
            return Ok(None);
        }
        if !crate::eingebaut_array::ist_konstruktor(self, s) {
            return Err(self.es_typfehler("object.constructor[Symbol.species] is not a constructor"));
        }
        self.wurzeln.push(s);
        Ok(Some(s))
    }

    /// TypedArrayCreateFromConstructor (23.2.4.2): `new c(...args)` must give
    /// a typed array (of the same content type, at least `min_len` long).
    pub(crate) fn ta_aus_spezies(&mut self, c: GVal, args: &[GVal], art: crate::eingebaut_typed::ElementArt, min_len: usize) -> Result<HeapRef, Stoerung> {
        let a = self.konstruktor_aufrufen(c, args)?;
        let ist_bigint = |x: crate::eingebaut_typed::ElementArt| x.name().starts_with("Big");
        match a {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::TypedArray { art: a_art, laenge, .. } => {
                    if ist_bigint(*a_art) != ist_bigint(art) {
                        return Err(self.es_typfehler("Content type of the species constructor does not match"));
                    }
                    if *laenge < min_len {
                        return Err(self.es_typfehler("The species constructor returned a typed array that is too short"));
                    }
                    self.wurzeln.push(a);
                    Ok(r)
                }
                _ => Err(self.es_typfehler("The species constructor did not return a typed array")),
            },
            _ => Err(self.es_typfehler("The species constructor did not return a typed array")),
        }
    }

    /// Release 15.10.2026: the derived promise of `then`/`catch`/`finally`
    /// takes the prototype of the species constructor (SpeciesConstructor
    /// of 27.2.5.4). APPROXIMATION: the subclass constructor itself is not
    /// run (NewPromiseCapability would run it); `instanceof` and the
    /// subclass methods work. MEASURED before (node 22 in brackets):
    /// `class P extends Promise {}; P.resolve(1).then(x => x) instanceof P`
    /// false (true), also for catch/finally.
    pub(crate) fn versprechen_spezies(&mut self, this: GVal, erg: GVal) -> Result<GVal, Stoerung> {
        let Some(pc) = self.es.promise_ctor else { return Ok(erg) };
        let GVal::Ref(er) = erg else { return Ok(erg) };
        let c = self.hole(this, "constructor")?;
        if matches!(c, GVal::Undefiniert) {
            return Ok(erg);
        }
        if c == GVal::Ref(pc)
            && self
                .eigene_eigenschaft(pc, crate::symbole::SYM_SPECIES)
                .is_some_and(|e| e.getter.is_some() && e.getter == self.es.spezies_getter)
        {
            return Ok(erg);
        }
        if !self.es_ist_objekt(c) {
            return Err(self.es_typfehler("object.constructor is not an object"));
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(erg);
        self.wurzeln.push(c);
        let r = (|| {
            let s = self.hole(c, crate::symbole::SYM_SPECIES)?;
            if matches!(s, GVal::Undefiniert | GVal::Null) || s == GVal::Ref(pc) {
                return Ok(erg);
            }
            if !crate::eingebaut_array::ist_konstruktor(self, s) {
                return Err(self.es_typfehler("object.constructor[Symbol.species] is not a constructor"));
            }
            let p = self.hole(s, "prototype")?;
            if let GVal::Ref(p) = p
                && self.es_ist_objekt(GVal::Ref(p))
                && let HObj::Versprechen { proto, .. } = self.heap.holen_mut(er)
            {
                *proto = Some(p);
            }
            Ok(erg)
        })();
        self.wurzel_zurueck(marke);
        r
    }

    /// CanBeHeldWeakly (9.13): an object, or a symbol not from Symbol.for.
    fn es_schwach_haltbar(&self, v: GVal) -> bool {
        match v {
            GVal::Ref(r) if crate::symbole::ist_symbol(self, r) => !self.symbol_registry.values().any(|s| *s == r),
            _ => self.es_ist_objekt(v),
        }
    }

    /// The instance of a `new` on the native constructor `c`: the derived
    /// object of a subclass `super()` call, else a fresh object below
    /// `c.prototype`.
    fn es_neue_instanz(&mut self, c: HeapRef, this: GVal) -> Result<HeapRef, Stoerung> {
        if let GVal::Ref(o) = this
            && matches!(self.heap.holen(o), HObj::Gegenstand { .. })
            && o != self.global_objekt
        {
            return Ok(o);
        }
        let proto = match self.nativ_proto.get(&c).copied() {
            Some(p) => p,
            None => self.proto_objekt,
        };
        self.neues_objekt(Some(proto)).map_err(Stoerung::Fehler)
    }

    fn es_schwach(&mut self, op: EsOp, fn_ref: HeapRef, this: GVal, args: &[GVal], als_konstruktor: bool) -> Result<GVal, Stoerung> {
        const ZIEL: &str = "#es_weakref";
        const RUECKRUF: &str = "#es_finreg";
        const ZELLEN: &str = "#es_zellen";
        let ziel_von = |e: &Engine, this: GVal, k: &str| match this {
            GVal::Ref(o) if e.eigene_lese(o, k).is_some() => Some(o),
            _ => None,
        };
        match op {
            EsOp::WeakRefCtor => {
                if !als_konstruktor {
                    return Err(self.es_typfehler("Constructor WeakRef requires 'new'"));
                }
                let t = self.arg(args, 0);
                if !self.es_schwach_haltbar(t) {
                    return Err(self.es_typfehler("WeakRef: invalid target"));
                }
                let o = self.es_neue_instanz(fn_ref, this)?;
                self.es_eigen(o, ZIEL, t, true, false, true);
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
                Ok(GVal::Ref(o))
            }
            EsOp::WeakRefDeref => match ziel_von(self, this, ZIEL) {
                Some(o) => Ok(self.eigene_lese(o, ZIEL).unwrap_or(GVal::Undefiniert)),
                None => Err(self.es_typfehler("WeakRef.prototype.deref called on incompatible receiver")),
            },
            EsOp::FinRegCtor => {
                if !als_konstruktor {
                    return Err(self.es_typfehler("Constructor FinalizationRegistry requires 'new'"));
                }
                let f = self.arg(args, 0);
                if !self.ist_aufrufbar(f) {
                    return Err(self.es_typfehler("FinalizationRegistry: cleanup must be callable"));
                }
                let o = self.es_neue_instanz(fn_ref, this)?;
                let marke = self.wurzel_merke();
                self.wurzeln.push(GVal::Ref(o));
                let z = self.neues_feld().map_err(Stoerung::Fehler);
                self.wurzel_zurueck(marke);
                let z = z?;
                self.es_eigen(o, RUECKRUF, f, true, false, true);
                self.es_eigen(o, ZELLEN, GVal::Ref(z), true, false, true);
                self.heap.kosten_verbuchen(2 * crate::heap::KOSTEN_EINTRAG);
                Ok(GVal::Ref(o))
            }
            EsOp::FinRegRegister => {
                let Some(o) = ziel_von(self, this, RUECKRUF) else {
                    return Err(self.es_typfehler("FinalizationRegistry.prototype.register called on incompatible receiver"));
                };
                let (t, h, k) = (self.arg(args, 0), self.arg(args, 1), self.arg(args, 2));
                if !self.es_schwach_haltbar(t) {
                    return Err(self.es_typfehler("FinalizationRegistry.prototype.register: invalid target"));
                }
                if self.same_value(t, h) {
                    return Err(self.es_typfehler("FinalizationRegistry.prototype.register: target and holdings must not be same"));
                }
                if !self.es_schwach_haltbar(k) && !matches!(k, GVal::Undefiniert) {
                    return Err(self.es_typfehler("FinalizationRegistry.prototype.register: invalid unregister token"));
                }
                if let Some(GVal::Ref(z)) = self.eigene_lese(o, ZELLEN)
                    && let HObj::Feld { elemente, .. } = self.heap.holen_mut(z)
                {
                    elemente.extend_from_slice(&[t, h, k]);
                    self.heap.kosten_verbuchen(3 * crate::heap::KOSTEN_EINTRAG);
                }
                Ok(GVal::Undefiniert)
            }
            _ => {
                let Some(o) = ziel_von(self, this, RUECKRUF) else {
                    return Err(self.es_typfehler("FinalizationRegistry.prototype.unregister called on incompatible receiver"));
                };
                let k = self.arg(args, 0);
                if !self.es_schwach_haltbar(k) {
                    return Err(self.es_typfehler("FinalizationRegistry.prototype.unregister: invalid unregister token"));
                }
                let Some(GVal::Ref(z)) = self.eigene_lese(o, ZELLEN) else {
                    return Ok(GVal::Bool(false));
                };
                let alt = match self.heap.holen(z) {
                    HObj::Feld { elemente, .. } => elemente.clone(),
                    _ => Vec::new(),
                };
                let mut neu = Vec::with_capacity(alt.len());
                let mut entfernt = false;
                for zelle in alt.chunks(3) {
                    if zelle.len() == 3 && self.same_value(zelle[2], k) {
                        entfernt = true;
                    } else {
                        neu.extend_from_slice(zelle);
                    }
                }
                if let HObj::Feld { elemente, .. } = self.heap.holen_mut(z) {
                    *elemente = neu;
                }
                Ok(GVal::Bool(entfernt))
            }
        }
    }

    fn es_float16_bauen(&mut self) -> Result<(), Stoerung> {
        use EsOp::*;
        let Some(f32_ctor) = self.es_global("Float32Array") else { return Ok(()) };
        let f32_proto = match self.eigene_lese(f32_ctor, "prototype") {
            Some(GVal::Ref(p)) => p,
            _ => return Ok(()),
        };
        let ta_proto = self.heap_proto_von(f32_proto).unwrap_or(self.proto_objekt);
        let proto = self.neues_objekt(Some(ta_proto)).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(proto));
        let ctor = self.nativ_anlegen("Float16Array", crate::builtin::Nativ::Es(TaCtorFloat16), self.proto_funktion);
        self.nativ_prototyp(ctor, proto);
        // The same [[Prototype]] as Float32Array (%TypedArray% or Function.prototype).
        let ctor_proto = self.heap_proto_von(f32_ctor);
        if let HObj::Nativ { proto: p, .. } = self.heap.holen_mut(ctor) {
            *p = ctor_proto.or(*p);
        }
        self.es_eigen(proto, "constructor", GVal::Ref(ctor), true, false, true);
        // `from`/`of` and `@@toStringTag` come from %TypedArray%.
        self.es_eigen(ctor, "BYTES_PER_ELEMENT", GVal::Zahl(2.0), false, false, false);
        self.es_eigen(proto, "BYTES_PER_ELEMENT", GVal::Zahl(2.0), false, false, false);
        self.global_binden("Float16Array", GVal::Ref(ctor));
        Ok(())
    }
}
