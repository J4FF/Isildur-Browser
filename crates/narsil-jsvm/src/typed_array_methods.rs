//! `%TypedArray%.prototype` methods (ES2024 23.2.3) — 2026-10-15.
//!
//! MEASURED before: `%TypedArray%.prototype` had only `set`, `join`,
//! `toString` and `subarray`; `[...new Uint8Array(2)]` and `for (x of u8)`
//! threw "Wert ist nicht iterierbar", `u8.slice` was undefined. The WPT
//! WebCryptoAPI vectors (`ciphertext.slice(...)`), the compression tests and
//! countless libraries use these.
//!
//! One `Nativ::TypedArrayMethode(TaMethod)` variant carries all of them.
//! New arrays (`slice`, `map`, `filter`, ...) get the prototype of `this`
//! (TypedArraySpeciesCreate without a user-defined `constructor[@@species]`).

use crate::eingebaut_typed::ElementArt;
use crate::heap::{GVal, HObj, HeapRef};
use crate::interp::{Engine, Stoerung};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaMethod {
    At,
    CopyWithin,
    Entries,
    Every,
    Fill,
    Filter,
    Find,
    FindIndex,
    FindLast,
    FindLastIndex,
    ForEach,
    Includes,
    IndexOf,
    Keys,
    LastIndexOf,
    Map,
    Reduce,
    ReduceRight,
    Reverse,
    Slice,
    Some,
    Sort,
    ToLocaleString,
    ToReversed,
    ToSorted,
    Values,
    With,
}

/// (name, method, length) for the prototype.
pub(crate) const METHODS: &[(&str, TaMethod, u32)] = &[
    ("at", TaMethod::At, 1),
    ("copyWithin", TaMethod::CopyWithin, 2),
    ("entries", TaMethod::Entries, 0),
    ("every", TaMethod::Every, 1),
    ("fill", TaMethod::Fill, 1),
    ("filter", TaMethod::Filter, 1),
    ("find", TaMethod::Find, 1),
    ("findIndex", TaMethod::FindIndex, 1),
    ("findLast", TaMethod::FindLast, 1),
    ("findLastIndex", TaMethod::FindLastIndex, 1),
    ("forEach", TaMethod::ForEach, 1),
    ("includes", TaMethod::Includes, 1),
    ("indexOf", TaMethod::IndexOf, 1),
    ("keys", TaMethod::Keys, 0),
    ("lastIndexOf", TaMethod::LastIndexOf, 1),
    ("map", TaMethod::Map, 1),
    ("reduce", TaMethod::Reduce, 1),
    ("reduceRight", TaMethod::ReduceRight, 1),
    ("reverse", TaMethod::Reverse, 0),
    ("slice", TaMethod::Slice, 2),
    ("some", TaMethod::Some, 1),
    ("sort", TaMethod::Sort, 1),
    ("toLocaleString", TaMethod::ToLocaleString, 0),
    ("toReversed", TaMethod::ToReversed, 0),
    ("toSorted", TaMethod::ToSorted, 1),
    ("values", TaMethod::Values, 0),
    ("with", TaMethod::With, 2),
];

/// SameValueZero / strict equality of two typed array element values.
fn same(a: GVal, b: GVal, zero: bool) -> bool {
    match (a, b) {
        (GVal::Zahl(x), GVal::Zahl(y)) => x == y || (zero && x.is_nan() && y.is_nan()),
        (GVal::Ganz(x), GVal::Ganz(y)) => x == y,
        _ => false,
    }
}

impl Engine {
    /// Install the methods on `%TypedArray%.prototype`.
    pub(crate) fn typed_array_methods_install(&mut self, proto: HeapRef) {
        let mut values = None;
        for (name, m, len) in METHODS {
            let f = self.nativ_anlegen(name, crate::builtin::Nativ::TypedArrayMethode(*m), self.proto_funktion);
            self.funktionsnamen_setzen(f, name, *len);
            // Release 15.10.2026: (enumerable false, writable true) — the
            // flags were swapped, so `for (k in new Uint8Array(1))` listed
            // all methods (node 22: only the index "0").
            let _ = self.eigenschaft_mit_attributen(proto, name, GVal::Ref(f), false, true);
            if *m == TaMethod::Values {
                values = Some(f);
            }
        }
        // `%TypedArray%.prototype[@@iterator]` is the same function as `values`.
        if let Some(f) = values
            && let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen_mut(proto)
        {
            eigenschaften.insert(
                "Symbol(Symbol.iterator)".to_string(),
                crate::heap::Eigenschaft {
                    wert: GVal::Ref(f),
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
        }
    }

    fn ta_get(&self, puffer: HeapRef, art: ElementArt, versatz: usize, i: usize) -> GVal {
        match self.heap.holen(puffer) {
            HObj::BytePuffer { daten, .. } => art.lesen(daten, versatz + i * art.bytes()),
            _ => GVal::Undefiniert,
        }
    }

    fn ta_values(&self, puffer: HeapRef, art: ElementArt, versatz: usize, len: usize) -> Vec<GVal> {
        (0..len).map(|i| self.ta_get(puffer, art, versatz, i)).collect()
    }

    /// A new typed array of `art` with `len` elements and the prototype of
    /// `like`.
    fn ta_new_like(&mut self, like: HeapRef, art: ElementArt, len: usize) -> Result<HeapRef, Stoerung> {
        // Release 15.10.2026 (agent I): TypedArraySpeciesCreate (23.2.4.1) —
        // a species constructor other than the intrinsic builds the result.
        if let Some(c) = self.ta_spezies(like, art)? {
            return self.ta_aus_spezies(c, &[GVal::Zahl(len as f64)], art, len);
        }
        let proto = match self.heap.holen(like) {
            HObj::TypedArray { proto, .. } => *proto,
            _ => None,
        };
        let bytes = len
            .checked_mul(art.bytes())
            .ok_or_else(|| Stoerung::Wurf(self.fehler_wert("RangeError", "TypedArray too large")))?;
        let puffer = self.neuer_bytepuffer(bytes, false)?;
        self.wurzeln.push(GVal::Ref(puffer));
        let obj = HObj::TypedArray {
            proto,
            art,
            puffer,
            versatz: 0,
            laenge: len,
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = crate::heap::Heap::kosten_fuer(&obj);
        self.vor_allok(kosten)?;
        let r = self.belege(obj, kosten);
        self.wurzeln.push(GVal::Ref(r));
        Ok(r)
    }

    fn ta_put(&mut self, r: HeapRef, i: usize, v: GVal) -> Result<(), Stoerung> {
        self.typedarray_element_setzen(r, Some(i), v)
    }

    /// ToIntegerOrInfinity, then a relative index clamped to 0..=len.
    fn ta_rel(&mut self, v: GVal, len: usize, default: usize) -> Result<usize, Stoerung> {
        if v == GVal::Undefiniert {
            return Ok(default);
        }
        let n = self.zu_zahl(v)?;
        let n = if n.is_nan() { 0.0 } else { n.trunc() };
        let l = len as f64;
        Ok(if n < 0.0 { (l + n).max(0.0) as usize } else { n.min(l) as usize })
    }

    fn ta_callback(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let f = self.arg(args, 0);
        if !self.ist_aufrufbar(f) {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "callback is not a function")));
        }
        Ok(f)
    }

    fn ta_iterator(&mut self, items: Vec<GVal>) -> Result<GVal, Stoerung> {
        let f = self.feld_aus_werten(&items)?;
        self.wurzeln.push(GVal::Ref(f));
        self.feld_zum_iterator(f)
    }

    fn ta_sorted(&mut self, values: &mut [GVal], cmp: GVal) -> Result<(), Stoerung> {
        if cmp == GVal::Undefiniert {
            // Numeric order; NaN last, -0 before +0.
            values.sort_by(|a, b| match (a, b) {
                (GVal::Zahl(x), GVal::Zahl(y)) => {
                    if x.is_nan() || y.is_nan() {
                        return x.is_nan().cmp(&y.is_nan());
                    }
                    if *x == 0.0 && *y == 0.0 {
                        return y.is_sign_negative().cmp(&x.is_sign_negative());
                    }
                    x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
                }
                (GVal::Ganz(x), GVal::Ganz(y)) => x.cmp(y),
                _ => std::cmp::Ordering::Equal,
            });
            return Ok(());
        }
        if !self.ist_aufrufbar(cmp) {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "comparator is not a function")));
        }
        // Insertion-based merge sort with the user comparator (stable, and
        // a throwing comparator propagates).
        let mut v: Vec<GVal> = values.to_vec();
        let mut buf = v.clone();
        let n = v.len();
        let mut width = 1;
        while width < n {
            let mut i = 0;
            while i < n {
                let mid = (i + width).min(n);
                let hi = (i + 2 * width).min(n);
                let (mut a, mut b, mut k) = (i, mid, i);
                while a < mid && b < hi {
                    let r = self.aufruf_synchron(cmp, GVal::Undefiniert, &[v[b], v[a]], false)?;
                    let c = self.zu_zahl(r)?;
                    if c < 0.0 {
                        buf[k] = v[b];
                        b += 1;
                    } else {
                        buf[k] = v[a];
                        a += 1;
                    }
                    k += 1;
                }
                while a < mid {
                    buf[k] = v[a];
                    a += 1;
                    k += 1;
                }
                while b < hi {
                    buf[k] = v[b];
                    b += 1;
                    k += 1;
                }
                i += 2 * width;
            }
            std::mem::swap(&mut v, &mut buf);
            width *= 2;
        }
        values.copy_from_slice(&v);
        Ok(())
    }

    pub(crate) fn typed_array_method(&mut self, m: TaMethod, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let (r, art, versatz, len, puffer) = self.typedarray_this(this)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(this);
        self.wurzeln.extend_from_slice(args);
        let out = self.typed_array_method_inner(m, r, art, versatz, len, puffer, args);
        self.wurzel_zurueck(marke);
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn typed_array_method_inner(
        &mut self,
        m: TaMethod,
        r: HeapRef,
        art: ElementArt,
        versatz: usize,
        len: usize,
        puffer: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let this = GVal::Ref(r);
        match m {
            TaMethod::At => {
                let n = self.zu_zahl(self.arg(args, 0))?;
                let n = if n.is_nan() { 0.0 } else { n.trunc() };
                let k = if n < 0.0 { len as f64 + n } else { n };
                if k < 0.0 || k >= len as f64 {
                    return Ok(GVal::Undefiniert);
                }
                Ok(self.ta_get(puffer, art, versatz, k as usize))
            }
            TaMethod::CopyWithin => {
                let to = self.ta_rel(self.arg(args, 0), len, 0)?;
                let from = self.ta_rel(self.arg(args, 1), len, 0)?;
                let end = self.ta_rel(self.arg(args, 2), len, len)?;
                let count = end.saturating_sub(from).min(len - to);
                if count > 0 {
                    let b = art.bytes();
                    if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(puffer) {
                        let s = versatz + from * b;
                        let d = versatz + to * b;
                        if s + count * b <= daten.len() && d + count * b <= daten.len() {
                            daten.copy_within(s..s + count * b, d);
                        }
                    }
                }
                Ok(this)
            }
            TaMethod::Entries | TaMethod::Keys | TaMethod::Values => {
                let mut items = Vec::with_capacity(len);
                for i in 0..len {
                    let v = self.ta_get(puffer, art, versatz, i);
                    items.push(match m {
                        TaMethod::Keys => GVal::Zahl(i as f64),
                        TaMethod::Values => v,
                        _ => {
                            let p = self.feld_aus_werten(&[GVal::Zahl(i as f64), v])?;
                            self.wurzeln.push(GVal::Ref(p));
                            GVal::Ref(p)
                        }
                    });
                }
                self.ta_iterator(items)
            }
            TaMethod::Every | TaMethod::Some | TaMethod::ForEach | TaMethod::Find | TaMethod::FindIndex => {
                let f = self.ta_callback(args)?;
                let t = self.arg(args, 1);
                for i in 0..len {
                    let v = self.ta_get(puffer, art, versatz, i);
                    let res = self.aufruf_synchron(f, t, &[v, GVal::Zahl(i as f64), this], false)?;
                    let b = self.zu_bool(res);
                    match m {
                        TaMethod::Every if !b => return Ok(GVal::Bool(false)),
                        TaMethod::Some if b => return Ok(GVal::Bool(true)),
                        TaMethod::Find if b => return Ok(v),
                        TaMethod::FindIndex if b => return Ok(GVal::Zahl(i as f64)),
                        _ => {}
                    }
                }
                Ok(match m {
                    TaMethod::Every => GVal::Bool(true),
                    TaMethod::Some => GVal::Bool(false),
                    TaMethod::FindIndex => GVal::Zahl(-1.0),
                    _ => GVal::Undefiniert,
                })
            }
            TaMethod::FindLast | TaMethod::FindLastIndex => {
                let f = self.ta_callback(args)?;
                let t = self.arg(args, 1);
                for i in (0..len).rev() {
                    let v = self.ta_get(puffer, art, versatz, i);
                    let res = self.aufruf_synchron(f, t, &[v, GVal::Zahl(i as f64), this], false)?;
                    if self.zu_bool(res) {
                        return Ok(if m == TaMethod::FindLast { v } else { GVal::Zahl(i as f64) });
                    }
                }
                Ok(if m == TaMethod::FindLast { GVal::Undefiniert } else { GVal::Zahl(-1.0) })
            }
            TaMethod::Fill => {
                let w = self.typedarray_schreibwert(art, self.arg(args, 0))?;
                let start = self.ta_rel(self.arg(args, 1), len, 0)?;
                let end = self.ta_rel(self.arg(args, 2), len, len)?;
                if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(puffer) {
                    for i in start..end {
                        art.schreiben(daten, versatz + i * art.bytes(), w);
                    }
                }
                Ok(this)
            }
            TaMethod::Filter => {
                let f = self.ta_callback(args)?;
                let t = self.arg(args, 1);
                let mut kept = Vec::new();
                for i in 0..len {
                    let v = self.ta_get(puffer, art, versatz, i);
                    let res = self.aufruf_synchron(f, t, &[v, GVal::Zahl(i as f64), this], false)?;
                    if self.zu_bool(res) {
                        kept.push(v);
                    }
                }
                let n = self.ta_new_like(r, art, kept.len())?;
                for (i, v) in kept.into_iter().enumerate() {
                    self.ta_put(n, i, v)?;
                }
                Ok(GVal::Ref(n))
            }
            TaMethod::Map => {
                let f = self.ta_callback(args)?;
                let t = self.arg(args, 1);
                let n = self.ta_new_like(r, art, len)?;
                for i in 0..len {
                    let v = self.ta_get(puffer, art, versatz, i);
                    let res = self.aufruf_synchron(f, t, &[v, GVal::Zahl(i as f64), this], false)?;
                    self.ta_put(n, i, res)?;
                }
                Ok(GVal::Ref(n))
            }
            TaMethod::Includes | TaMethod::IndexOf => {
                if len == 0 {
                    return Ok(if m == TaMethod::Includes { GVal::Bool(false) } else { GVal::Zahl(-1.0) });
                }
                let target = self.arg(args, 0);
                let start = self.ta_rel(self.arg(args, 1), len, 0)?;
                for i in start..len {
                    let v = self.ta_get(puffer, art, versatz, i);
                    if same(v, target, m == TaMethod::Includes) {
                        return Ok(if m == TaMethod::Includes { GVal::Bool(true) } else { GVal::Zahl(i as f64) });
                    }
                }
                Ok(if m == TaMethod::Includes { GVal::Bool(false) } else { GVal::Zahl(-1.0) })
            }
            TaMethod::LastIndexOf => {
                if len == 0 {
                    return Ok(GVal::Zahl(-1.0));
                }
                let target = self.arg(args, 0);
                let from = if args.len() > 1 {
                    let n = self.zu_zahl(self.arg(args, 1))?;
                    let n = if n.is_nan() { 0.0 } else { n.trunc() };
                    if n < 0.0 { len as f64 + n } else { n.min(len as f64 - 1.0) }
                } else {
                    len as f64 - 1.0
                };
                let mut i = from;
                while i >= 0.0 {
                    let v = self.ta_get(puffer, art, versatz, i as usize);
                    if same(v, target, false) {
                        return Ok(GVal::Zahl(i));
                    }
                    i -= 1.0;
                }
                Ok(GVal::Zahl(-1.0))
            }
            TaMethod::Reduce | TaMethod::ReduceRight => {
                let f = self.ta_callback(args)?;
                let order: Vec<usize> = if m == TaMethod::Reduce { (0..len).collect() } else { (0..len).rev().collect() };
                let mut it = order.into_iter();
                let mut acc = if args.len() > 1 {
                    self.arg(args, 1)
                } else {
                    match it.next() {
                        Some(i) => self.ta_get(puffer, art, versatz, i),
                        None => {
                            return Err(Stoerung::Wurf(
                                self.fehler_wert("TypeError", "reduce of an empty TypedArray with no initial value"),
                            ));
                        }
                    }
                };
                for i in it {
                    self.wurzeln.push(acc);
                    let v = self.ta_get(puffer, art, versatz, i);
                    acc = self.aufruf_synchron(f, GVal::Undefiniert, &[acc, v, GVal::Zahl(i as f64), this], false)?;
                }
                Ok(acc)
            }
            TaMethod::Reverse => {
                let b = art.bytes();
                if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(puffer)
                    && versatz + len * b <= daten.len()
                {
                    let s = &mut daten[versatz..versatz + len * b];
                    for i in 0..len / 2 {
                        for k in 0..b {
                            s.swap(i * b + k, (len - 1 - i) * b + k);
                        }
                    }
                }
                Ok(this)
            }
            TaMethod::ToReversed => {
                let n = self.ta_new_like(r, art, len)?;
                for i in 0..len {
                    let v = self.ta_get(puffer, art, versatz, len - 1 - i);
                    self.ta_put(n, i, v)?;
                }
                Ok(GVal::Ref(n))
            }
            TaMethod::Slice => {
                let start = self.ta_rel(self.arg(args, 0), len, 0)?;
                let end = self.ta_rel(self.arg(args, 1), len, len)?;
                let count = end.saturating_sub(start);
                let n = self.ta_new_like(r, art, count)?;
                if count > 0 {
                    let b = art.bytes();
                    let bytes = match self.heap.holen(puffer) {
                        HObj::BytePuffer { daten, .. } => {
                            let s = versatz + start * b;
                            daten.get(s..s + count * b).map(|x| x.to_vec())
                        }
                        _ => None,
                    };
                    if let (Some(bytes), Some(np)) = (bytes, match self.heap.holen(n) {
                        HObj::TypedArray { puffer, .. } => Some(*puffer),
                        _ => None,
                    }) && let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(np)
                    {
                        daten[..bytes.len()].copy_from_slice(&bytes);
                    }
                }
                Ok(GVal::Ref(n))
            }
            TaMethod::Sort | TaMethod::ToSorted => {
                let cmp = self.arg(args, 0);
                if cmp != GVal::Undefiniert && !self.ist_aufrufbar(cmp) {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "comparator is not a function")));
                }
                let mut values = self.ta_values(puffer, art, versatz, len);
                self.ta_sorted(&mut values, cmp)?;
                let target = if m == TaMethod::Sort { r } else { self.ta_new_like(r, art, len)? };
                for (i, v) in values.into_iter().enumerate() {
                    self.ta_put(target, i, v)?;
                }
                Ok(GVal::Ref(target))
            }
            TaMethod::ToLocaleString => {
                let mut parts = Vec::with_capacity(len);
                for i in 0..len {
                    let v = self.ta_get(puffer, art, versatz, i);
                    parts.push(self.zu_text(v)?);
                }
                self.text_wert(&parts.join(",")).map_err(Stoerung::Fehler)
            }
            TaMethod::With => {
                let n = self.zu_zahl(self.arg(args, 0))?;
                let n = if n.is_nan() { 0.0 } else { n.trunc() };
                let k = if n < 0.0 { len as f64 + n } else { n };
                let value = self.arg(args, 1);
                // The value is converted before the index check (ES2024).
                let _ = self.typedarray_schreibwert(art, value)?;
                if k < 0.0 || k >= len as f64 {
                    return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "index out of range")));
                }
                let out = self.ta_new_like(r, art, len)?;
                for i in 0..len {
                    let v = if i == k as usize { value } else { self.ta_get(puffer, art, versatz, i) };
                    self.ta_put(out, i, v)?;
                }
                Ok(GVal::Ref(out))
            }
        }
    }
}
