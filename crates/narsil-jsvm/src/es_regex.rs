//! Release 15.10.2026: the RegExp symbol protocol.
//!
//! * `RegExp.prototype[@@match]`, `[@@matchAll]`, `[@@replace]`,
//!   `[@@search]`, `[@@split]` (ECMA-262 22.2.6).
//! * The delegation in `String.prototype.match/matchAll/replace/replaceAll/
//!   search/split` (22.1.3: `GetMethod(regexp, @@x)` first), `IsRegExp`
//!   (7.2.8) and `RegExpExec` (22.2.7.1, a user `exec` runs).
//! * The RegExp String Iterator of `matchAll` (22.2.9).
//!
//! MEASURED before (node 22 in brackets): `typeof Symbol.replace` was
//! "undefined" ("symbol"), `'abc'.replace({[Symbol.replace]: f}, 'X')`
//! ignored `f` ("abc|X"), a RegExp subclass that overrides `exec` was not
//! asked (`'abcb'.replace(new R('b','g'), '[$&]')` gave "a[b]c[b]",
//! "a[B]c[B]"), and `r.lastIndex` stayed 3 after a global replace (0).
//!
//! The fast path: a plain RegExp whose `exec` is the built-in one runs the
//! native algorithms in `builtin2.rs` (no JavaScript call per match). Every
//! other receiver takes the spec algorithms below, written in JavaScript and
//! compiled on first use (no cost at startup).

use crate::builtin::Nativ;
use crate::es_modern::EsOp;
use crate::heap::{GVal, HObj, HeapRef};
use crate::interp::{Engine, Stoerung};
use crate::symbole::{SYM_MATCH, SYM_MATCH_ALL, SYM_REPLACE, SYM_SEARCH, SYM_SPECIES, SYM_SPLIT};

/// How a `String.prototype` method goes on after `GetMethod(arg, @@x)`.
pub(crate) enum SymWeg {
    /// A method other than the fast built-in ran: its result.
    Fertig(GVal),
    /// The built-in method on a plain RegExp: the native algorithm.
    Eingebaut,
    /// No method: the argument is used as a string.
    Kein,
}

/// Protocol number -> (property key, built-in native). 0 match, 1 search,
/// 2 replace, 3 split, 4 matchAll.
fn sym_art(art: u8) -> (&'static str, EsOp) {
    match art {
        0 => (SYM_MATCH, EsOp::RegexSymMatch),
        1 => (SYM_SEARCH, EsOp::RegexSymSearch),
        2 => (SYM_REPLACE, EsOp::RegexSymReplace),
        3 => (SYM_SPLIT, EsOp::RegexSymSplit),
        _ => (SYM_MATCH_ALL, EsOp::RegexSymMatchAll),
    }
}

/// Hidden state of a RegExp String Iterator (private names are not listed
/// by reflection).
const H_RE: &str = "#es_re";
const H_TEXT: &str = "#es_text";
const H_GLOBAL: &str = "#es_g";
const H_UNICODE: &str = "#es_u";
const H_FERTIG: &str = "#es_fertig";

/// The spec algorithms for receivers off the fast path (ECMA-262 2025
/// 22.2.6.8 @@match, 22.2.6.12 @@search, 22.2.6.11 @@replace with
/// 22.1.3.19.1 GetSubstitution, 22.2.6.14 @@split). `exec` is RegExpExec
/// (a native), `RegExpCtor` the original %RegExp%.
const REGEX_GLUE: &str = r#"(function regExpProtocol(kind, rx, S, arg, exec, RegExpCtor) {
  'use strict';
  var MAX = 9007199254740991;
  function len(v) { v = +v; if (!(v > 0)) return 0; v = v - v % 1; return v > MAX ? MAX : v; }
  function int(v) { v = +v; if (v !== v) return 0; return v - v % 1; }
  function same(a, b) { return a === b ? (a !== 0 || 1 / a === 1 / b) : (a !== a && b !== b); }
  function adv(i, u) {
    if (!u || i + 1 >= S.length) return i + 1;
    var c = S.charCodeAt(i);
    if (c < 0xD800 || c > 0xDBFF) return i + 1;
    var d = S.charCodeAt(i + 1);
    return d >= 0xDC00 && d <= 0xDFFF ? i + 2 : i + 1;
  }
  function setLI(r, v) { r.lastIndex = v; }
  function has(f, c) { return f.indexOf(c) >= 0; }
  function subst(matched, position, captures, named, tpl) {
    var res = '', m = captures.length, sl = S.length, i = 0;
    for (;;) {
      var d = tpl.indexOf('$', i);
      if (d < 0) { res += tpl.slice(i); return res; }
      res += tpl.slice(i, d);
      var n = tpl.charCodeAt(d + 1);
      if (n === 36) { res += '$'; i = d + 2; }
      else if (n === 96) { res += S.slice(0, position); i = d + 2; }
      else if (n === 38) { res += matched; i = d + 2; }
      else if (n === 39) { var t = position + matched.length; if (t < sl) res += S.slice(t); i = d + 2; }
      else if (n >= 48 && n <= 57) {
        var n2 = tpl.charCodeAt(d + 2), two = n2 >= 48 && n2 <= 57;
        var idx = two ? (n - 48) * 10 + (n2 - 48) : n - 48, dc = two ? 2 : 1;
        if (two && idx > m) { idx = n - 48; dc = 1; }
        if (idx >= 1 && idx <= m) { var cap = captures[idx - 1]; if (cap !== undefined) res += cap; }
        else res += tpl.slice(d, d + 1 + dc);
        i = d + 1 + dc;
      }
      else if (n === 60) {
        var gt = tpl.indexOf('>', d + 2);
        if (gt < 0 || named === undefined) { res += '$<'; i = d + 2; }
        else { var v = named[tpl.slice(d + 2, gt)]; if (v !== undefined) res += `${v}`; i = gt + 1; }
      }
      else { res += '$'; i = d + 1; }
    }
  }
  var flags, u, r, A, n;
  if (kind === 0) {
    flags = `${rx.flags}`;
    if (!has(flags, 'g')) return exec(rx, S);
    u = has(flags, 'u') || has(flags, 'v');
    setLI(rx, 0);
    A = []; n = 0;
    for (;;) {
      r = exec(rx, S);
      if (r === null) return n === 0 ? null : A;
      var ms = `${r[0]}`;
      A[n++] = ms;
      if (ms === '') setLI(rx, adv(len(rx.lastIndex), u));
    }
  }
  if (kind === 1) {
    var prev = rx.lastIndex;
    if (!same(prev, 0)) setLI(rx, 0);
    r = exec(rx, S);
    var cur = rx.lastIndex;
    if (!same(cur, prev)) setLI(rx, prev);
    return r === null ? -1 : r.index;
  }
  if (kind === 2) {
    var lengthS = S.length, fn = typeof arg === 'function', rv = fn ? arg : `${arg}`;
    flags = `${rx.flags}`;
    var global = has(flags, 'g');
    u = false;
    if (global) { u = has(flags, 'u') || has(flags, 'v'); setLI(rx, 0); }
    var results = [], k = 0;
    for (;;) {
      r = exec(rx, S);
      if (r === null) break;
      results[k++] = r;
      if (!global) break;
      if (`${r[0]}` === '') setLI(rx, adv(len(rx.lastIndex), u));
    }
    var acc = '', next = 0;
    for (var i = 0; i < k; i++) {
      r = results[i];
      var nCap = len(r.length) - 1;
      if (nCap < 0) nCap = 0;
      var matched = `${r[0]}`;
      var pos = int(r.index);
      pos = pos < 0 ? 0 : pos > lengthS ? lengthS : pos;
      var caps = [];
      for (var j = 1; j <= nCap; j++) { var c = r[j]; caps[j - 1] = c === undefined ? c : `${c}`; }
      var named = r.groups, repl;
      if (fn) {
        var args = [matched];
        for (var q = 0; q < caps.length; q++) args[args.length] = caps[q];
        args[args.length] = pos;
        args[args.length] = S;
        if (named !== undefined) args[args.length] = named;
        repl = `${Reflect.apply(rv, undefined, args)}`;
      } else {
        if (named !== undefined) {
          if (named === null) throw new TypeError('Cannot convert undefined or null to object');
          named = Object(named);
        }
        repl = subst(matched, pos, caps, named, rv);
      }
      if (pos >= next) { acc += S.slice(next, pos) + repl; next = pos + matched.length; }
    }
    return next >= lengthS ? acc : acc + S.slice(next);
  }
  var C = rx.constructor;
  if (C === undefined) C = RegExpCtor;
  else {
    if (C === null || (typeof C !== 'object' && typeof C !== 'function')) throw new TypeError('object.constructor is not an object');
    C = C[Symbol.species];
    if (C === undefined || C === null) C = RegExpCtor;
  }
  flags = `${rx.flags}`;
  u = has(flags, 'u') || has(flags, 'v');
  var sp = new C(rx, has(flags, 'y') ? flags : flags + 'y');
  A = []; n = 0;
  var lim = arg === undefined ? 4294967295 : arg >>> 0;
  if (lim === 0) return A;
  var size = S.length;
  if (size === 0) { if (exec(sp, S) === null) A[0] = S; return A; }
  var p = 0, q2 = 0;
  while (q2 < size) {
    setLI(sp, q2);
    var z = exec(sp, S);
    if (z === null) { q2 = adv(q2, u); continue; }
    var e = len(sp.lastIndex);
    if (e > size) e = size;
    if (e === p) { q2 = adv(q2, u); continue; }
    A[n++] = S.slice(p, q2);
    if (n === lim) return A;
    p = e;
    var nc = len(z.length) - 1;
    for (var w = 1; w <= nc; w++) { A[n++] = z[w]; if (n === lim) return A; }
    q2 = p;
  }
  A[n++] = S.slice(p, size);
  return A;
})"#;

impl Engine {
    /// Register the five `RegExp.prototype[@@x]` methods (names
    /// `[Symbol.match]` …, lengths 1/1/2/2/1).
    pub(crate) fn es_regex_aufbauen(&mut self) -> Result<(), Stoerung> {
        self.es.regex_ctor = match self.eigene_lese(self.global_objekt, "RegExp") {
            Some(GVal::Ref(r)) => Some(r),
            _ => None,
        };
        for (art, name) in [(0u8, "match"), (1, "search"), (2, "replace"), (3, "split"), (4, "matchAll")] {
            let (k, op) = sym_art(art);
            let f = self.nativ_anlegen(&format!("[Symbol.{name}]"), Nativ::Es(op), self.proto_funktion);
            self.es_eigen(self.proto_regex, k, GVal::Ref(f), true, false, true);
        }
        // `get [Symbol.species]` (returns `this`) on the constructors that
        // define it (ECMA-262 23.1.2.5, 24.1.2.3, 24.2.2.3, 27.2.4.8,
        // 22.2.5.2, 25.1.5.3, 25.2.4.2). One shared getter: the spec's seven
        // functions all do the same. MEASURED before: `RegExp[Symbol.species]`
        // was undefined, so a RegExp subclass (Babel's `_wrapRegExp`) got
        // plain RegExp matchers in `matchAll`.
        let g = self.nativ_anlegen("get [Symbol.species]", Nativ::Es(EsOp::SpeziesLesen), self.proto_funktion);
        self.es.spezies_getter = Some(g);
        self.es.array_ctor = match self.eigene_lese(self.global_objekt, "Array") {
            Some(GVal::Ref(r)) => Some(r),
            _ => None,
        };
        for n in ["Array", "Map", "Set", "Promise", "RegExp", "ArrayBuffer", "SharedArrayBuffer"] {
            if let Some(GVal::Ref(c)) = self.eigene_lese(self.global_objekt, n) {
                self.es_eigen(c, SYM_SPECIES, GVal::Undefiniert, false, false, true);
                if let Some(map) = self.eigenschaften_karte_mut(c)
                    && let Some(e) = map.get_mut(SYM_SPECIES)
                {
                    e.getter = Some(g);
                }
            }
        }
        Ok(())
    }

    /// An object in the ECMAScript sense (not a string or symbol primitive).
    pub(crate) fn es_ist_objekt(&self, v: GVal) -> bool {
        match v {
            GVal::Ref(r) => {
                !matches!(self.heap.holen(r), HObj::Zeichenkette(_)) && !crate::symbole::ist_symbol(self, r)
            }
            _ => false,
        }
    }

    fn es_ist_op(&self, v: GVal, op: EsOp) -> bool {
        matches!(v, GVal::Ref(f) if matches!(self.native.get(&f), Some(Nativ::Es(o)) if *o == op))
    }

    fn es_klebrig(&self, r: HeapRef) -> bool {
        matches!(self.heap.holen(r), HObj::Regex { schalter, .. } if schalter.contains('y'))
    }

    /// A plain RegExp whose `exec` is the built-in one: then the native
    /// algorithms are exactly the spec's RegExpExec loops.
    pub(crate) fn es_regex_schnell(&mut self, r: HeapRef) -> Result<bool, Stoerung> {
        if !matches!(self.heap.holen(r), HObj::Regex { .. }) {
            return Ok(false);
        }
        let e = self.hole(GVal::Ref(r), "exec")?;
        Ok(matches!(e, GVal::Ref(f) if matches!(self.native.get(&f), Some(Nativ::RegexAusfuehren))))
    }

    /// The fast path for protocol `art` on `r`: a plain RegExp with the
    /// built-in `exec`; match/search/replace not sticky (the native loops
    /// start at 0), split only with %RegExp% as constructor (no species
    /// constructor to ask).
    fn es_schnellweg(&mut self, r: HeapRef, art: u8) -> Result<bool, Stoerung> {
        if !self.es_regex_schnell(r)? {
            return Ok(false);
        }
        match art {
            3 => {
                let c = self.hole(GVal::Ref(r), "constructor")?;
                Ok(matches!((c, self.es.regex_ctor), (GVal::Ref(c), Some(k)) if c == k))
            }
            4 => Ok(true),
            _ => Ok(!self.es_klebrig(r)),
        }
    }

    /// IsRegExp (ECMA-262 7.2.8): `@@match` decides when it is defined.
    pub(crate) fn es_ist_regexp(&mut self, v: GVal) -> Result<bool, Stoerung> {
        if !self.es_ist_objekt(v) {
            return Ok(false);
        }
        let m = self.hole(v, SYM_MATCH)?;
        if !matches!(m, GVal::Undefiniert) {
            return Ok(self.zu_bool(m));
        }
        Ok(self.ist_regex(v))
    }

    /// Step 2 of `String.prototype.match/search/replace/split/matchAll`:
    /// RequireObjectCoercible(this), then `GetMethod(arg, @@x)`; a method
    /// other than the fast built-in is called with `this` and `rest`.
    /// Primitive arguments have no such method (their prototypes do not
    /// define one).
    pub(crate) fn es_sym_weg(&mut self, this: GVal, arg: GVal, art: u8, rest: &[GVal]) -> Result<SymWeg, Stoerung> {
        if !crate::eingebaut_text::this_ist_coercible(&this) {
            self.this_text(this)?;
        }
        if !self.es_ist_objekt(arg) {
            return Ok(SymWeg::Kein);
        }
        let GVal::Ref(r) = arg else {
            return Ok(SymWeg::Kein);
        };
        let (k, op) = sym_art(art);
        let m = self.hole(arg, k)?;
        if matches!(m, GVal::Undefiniert | GVal::Null) {
            return Ok(SymWeg::Kein);
        }
        if self.es_ist_op(m, op) && self.es_schnellweg(r, art)? {
            return Ok(SymWeg::Eingebaut);
        }
        if !self.ist_aufrufbar(m) {
            let anzeige = self.schluessel_anzeige(k);
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", &format!("{anzeige} is not a function"))));
        }
        let mut argv = Vec::with_capacity(1 + rest.len());
        argv.push(this);
        argv.extend_from_slice(rest);
        let marke = self.wurzel_merke();
        self.wurzeln.push(m);
        self.wurzeln.push(arg);
        let erg = self.aufruf_synchron(m, arg, &argv, false);
        self.wurzel_zurueck(marke);
        Ok(SymWeg::Fertig(erg?))
    }

    /// RegExpExec (ECMA-262 22.2.7.1): a callable `exec` runs (its result
    /// must be an object or null), else RegExpBuiltinExec.
    pub(crate) fn es_regexp_exec(&mut self, r: GVal, s: GVal) -> Result<GVal, Stoerung> {
        let exec = self.hole(r, "exec")?;
        if self.ist_aufrufbar(exec) {
            if matches!(exec, GVal::Ref(f) if matches!(self.native.get(&f), Some(Nativ::RegexAusfuehren))) {
                return self.n_regex_exec(r, &[s]);
            }
            let marke = self.wurzel_merke();
            self.wurzeln.push(exec);
            self.wurzeln.push(s);
            let erg = self.aufruf_synchron(exec, r, &[s], false);
            self.wurzel_zurueck(marke);
            let erg = erg?;
            if !matches!(erg, GVal::Null) && !self.es_ist_objekt(erg) {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "RegExp exec method returned something other than an Object or null")));
            }
            return Ok(erg);
        }
        if !self.ist_regex(r) {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "RegExp.prototype.exec called on incompatible receiver")));
        }
        self.n_regex_exec(r, &[s])
    }

    /// ToLength (7.1.20).
    fn es_zu_laenge(&mut self, v: GVal) -> Result<f64, Stoerung> {
        let n = self.zu_zahl(v)?;
        if n.is_nan() || n <= 0.0 {
            return Ok(0.0);
        }
        Ok(n.trunc().min(9_007_199_254_740_991.0))
    }

    /// `Set(R, "lastIndex", v, true)` on a RegExp: a non-writable
    /// `lastIndex` throws (a frozen RegExp).
    pub(crate) fn es_last_index(&mut self, r: HeapRef, v: f64) -> Result<(), Stoerung> {
        if let Some(e) = self.eigene_eigenschaft(r, "lastIndex")
            && e.getter.is_none()
            && e.setter.is_none()
        {
            if !e.beschreibbar {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Cannot assign to read only property 'lastIndex' of object")));
            }
            if matches!(self.heap.holen(r), HObj::Regex { .. }) {
                self.setze_eigen(r, "lastIndex", GVal::Zahl(v));
                return Ok(());
            }
        }
        self.setze(GVal::Ref(r), "lastIndex", GVal::Zahl(v))
    }

    /// `RegExp.prototype[@@match|@@search|@@replace|@@split|@@matchAll]`.
    pub(crate) fn es_regex_sym(&mut self, art: u8, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "RegExp method called on incompatible receiver")));
        };
        if !self.es_ist_objekt(this) {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "RegExp method called on incompatible receiver")));
        }
        let s = crate::eingebaut_text::zu_text_kein_symbol(self, self.arg(args, 0))?;
        if art == 4 {
            return self.es_match_all_bauen(r, s);
        }
        if self.es_schnellweg(r, art)? {
            return match art {
                0 => {
                    let (m, f) = self.regex_daten(this)?;
                    self.regex_treffer_kern(&s, &m, &f, Some(r))
                }
                1 => {
                    let (m, f) = self.regex_daten(this)?;
                    self.regex_suchen_kern(&s, &m, &f)
                }
                2 => {
                    let ersatz = self.arg(args, 1);
                    let ersatz_fn = self.ist_aufrufbar(ersatz);
                    self.regex_ersetzen(&s, this, ersatz, ersatz_fn, false)
                }
                _ => self.regex_teilen_feld(&s, this, self.arg(args, 1)),
            };
        }
        let f = self.es_regex_glue()?;
        let exec = self.es_exec_helfer()?;
        let sv = self.text_wert(&s).map_err(Stoerung::Fehler)?;
        let rc = self.es.regex_ctor.map(GVal::Ref).unwrap_or(GVal::Undefiniert);
        let argv = [GVal::Zahl(art as f64), this, sv, self.arg(args, 1), GVal::Ref(exec), rc];
        let marke = self.wurzel_merke();
        self.wurzeln.push(sv);
        let erg = self.aufruf_synchron(GVal::Ref(f), GVal::Undefiniert, &argv, false);
        self.wurzel_zurueck(marke);
        erg
    }

    fn es_regex_glue(&mut self) -> Result<HeapRef, Stoerung> {
        self.es_glue(REGEX_GLUE, 2)
    }

    /// The RegExpExec native handed to the glue (kept in glue slot 3).
    fn es_exec_helfer(&mut self) -> Result<HeapRef, Stoerung> {
        if let Some(f) = self.es.glue.get(3).copied().flatten() {
            return Ok(f);
        }
        let f = self.nativ_anlegen("RegExpExec", Nativ::Es(EsOp::RegexExecIntern), self.proto_funktion);
        if self.es.glue.len() <= 3 {
            self.es.glue.resize(4, None);
        }
        self.es.glue[3] = Some(f);
        Ok(f)
    }

    /// `RegExp.prototype[@@split]` on the fast path: limit (ToUint32), the
    /// native sticky splitter, the result array.
    pub(crate) fn regex_teilen_feld(&mut self, s: &str, re: GVal, limit: GVal) -> Result<GVal, Stoerung> {
        let grenze = if matches!(limit, GVal::Undefiniert) {
            None
        } else {
            Some(self.zu_uint32(limit)? as usize)
        };
        let teile = if grenze == Some(0) { Vec::new() } else { self.regex_teilung(s, re, grenze)? };
        self.teile_als_feld(teile, grenze)
    }

    /// The pieces of `split` as an array (truncated to the limit).
    pub(crate) fn teile_als_feld(&mut self, mut teile: Vec<Option<String>>, grenze: Option<usize>) -> Result<GVal, Stoerung> {
        if let Some(g) = grenze {
            teile.truncate(g);
        }
        if teile.len() > 100_000 {
            return Err(Stoerung::Fehler(crate::fehler::JsFehler::Speicher("split zu viele Teile".to_string())));
        }
        let f = self.neues_feld().map_err(Stoerung::Fehler)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(f));
        for t in teile {
            let g = match t {
                Some(t) => match self.text_wert(&t) {
                    Ok(g) => g,
                    Err(e) => {
                        self.wurzel_zurueck(marke);
                        return Err(Stoerung::Fehler(e));
                    }
                },
                None => GVal::Undefiniert,
            };
            if let HObj::Feld { elemente, .. } = self.heap.holen_mut(f) {
                elemente.push(g);
            }
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG * 2);
        }
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(f))
    }

    /// SpeciesConstructor(o, %RegExp%) (7.3.22).
    fn es_regex_species(&mut self, o: GVal) -> Result<GVal, Stoerung> {
        let vorgabe = self.es.regex_ctor.map(GVal::Ref).unwrap_or(GVal::Undefiniert);
        let c = self.hole(o, "constructor")?;
        if matches!(c, GVal::Undefiniert) {
            return Ok(vorgabe);
        }
        if !self.es_ist_objekt(c) {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "object.constructor is not an object")));
        }
        let s = self.hole(c, SYM_SPECIES)?;
        if matches!(s, GVal::Undefiniert | GVal::Null) {
            return Ok(vorgabe);
        }
        if crate::eingebaut_array::ist_konstruktor(self, s) {
            return Ok(s);
        }
        Err(Stoerung::Wurf(self.fehler_wert("TypeError", "object.constructor[Symbol.species] is not a constructor")))
    }

    /// `RegExp.prototype[@@matchAll]` (22.2.6.9): the matcher is a copy built
    /// by the species constructor with the same flags, starting at
    /// `lastIndex`; then the iterator.
    pub(crate) fn es_match_all_bauen(&mut self, r: HeapRef, s: String) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        let erg = self.es_match_all_innen(r, s);
        self.wurzel_zurueck(marke);
        erg
    }

    fn es_match_all_innen(&mut self, r: HeapRef, s: String) -> Result<GVal, Stoerung> {
        let c = self.es_regex_species(GVal::Ref(r))?;
        self.wurzeln.push(c);
        let fl = self.hole(GVal::Ref(r), "flags")?;
        let flags = crate::eingebaut_text::zu_text_kein_symbol(self, fl)?;
        let fv = self.text_wert(&flags).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(fv);
        let matcher = if Some(c) == self.es.regex_ctor.map(GVal::Ref) {
            self.n_regex_ctor(&[GVal::Ref(r), fv], true)?
        } else {
            self.konstruktor_aufrufen(c, &[GVal::Ref(r), fv])?
        };
        self.wurzeln.push(matcher);
        let li = self.hole(GVal::Ref(r), "lastIndex")?;
        let li = self.es_zu_laenge(li)?;
        let GVal::Ref(m) = matcher else {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "matchAll: the species constructor did not return an object")));
        };
        self.es_last_index(m, li)?;
        let proto = self.es_iter_proto(3)?;
        let it = self.neues_objekt(Some(proto)).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(it));
        let sv = self.text_wert(&s).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(sv);
        self.es_eigen(it, H_RE, matcher, true, false, true);
        self.es_eigen(it, H_TEXT, sv, true, false, true);
        self.es_eigen(it, H_GLOBAL, GVal::Bool(flags.contains('g')), true, false, true);
        self.es_eigen(it, H_UNICODE, GVal::Bool(flags.contains('u') || flags.contains('v')), true, false, true);
        self.es_eigen(it, H_FERTIG, GVal::Bool(false), true, false, true);
        Ok(GVal::Ref(it))
    }

    /// `%RegExpStringIteratorPrototype%.next` (22.2.9.2.1).
    pub(crate) fn es_regex_iter_next(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let it = match this {
            GVal::Ref(o) if self.eigene_lese(o, H_RE).is_some() => o,
            _ => {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "%RegExpStringIterator%.next requires that 'this' be an Object")));
            }
        };
        if matches!(self.eigene_lese(it, H_FERTIG), Some(GVal::Bool(true))) {
            return self.es_iter_ergebnis(GVal::Undefiniert, true);
        }
        let re = self.eigene_lese(it, H_RE).unwrap_or(GVal::Undefiniert);
        let text = self.eigene_lese(it, H_TEXT).unwrap_or(GVal::Undefiniert);
        let global = matches!(self.eigene_lese(it, H_GLOBAL), Some(GVal::Bool(true)));
        let unicode = matches!(self.eigene_lese(it, H_UNICODE), Some(GVal::Bool(true)));
        let m = self.es_regexp_exec(re, text)?;
        if matches!(m, GVal::Null) {
            self.es_eigen(it, H_FERTIG, GVal::Bool(true), true, false, true);
            return self.es_iter_ergebnis(GVal::Undefiniert, true);
        }
        if !global {
            self.es_eigen(it, H_FERTIG, GVal::Bool(true), true, false, true);
            return self.es_iter_ergebnis(m, false);
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(m);
        let erg = (|| {
            let m0 = self.hole(m, "0")?;
            let ms = crate::eingebaut_text::zu_text_kein_symbol(self, m0)?;
            if ms.is_empty() {
                let li = self.hole(re, "lastIndex")?;
                let li = self.es_zu_laenge(li)? as usize;
                let s = self.text_von(text);
                let neu = vorruecken(&s, li, unicode);
                match re {
                    GVal::Ref(r) => self.es_last_index(r, neu as f64)?,
                    _ => self.setze(re, "lastIndex", GVal::Zahl(neu as f64))?,
                }
            }
            self.es_iter_ergebnis(m, false)
        })();
        self.wurzel_zurueck(marke);
        erg
    }

    /// The internal RegExpExec native of the glue.
    pub(crate) fn es_regex_exec_intern(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        self.es_regexp_exec(self.arg(args, 0), self.arg(args, 1))
    }
}

/// AdvanceStringIndex (22.2.7.3) in UTF-16 units: past a surrogate pair in
/// Unicode mode, else one unit.
pub(crate) fn vorruecken(s: &str, i: usize, unicode: bool) -> usize {
    if !unicode {
        return i + 1;
    }
    let e = crate::regexmini::als_einheiten(s);
    if i + 1 >= e.len() {
        return i + 1;
    }
    if (0xD800..=0xDBFF).contains(&e[i]) && (0xDC00..=0xDFFF).contains(&e[i + 1]) {
        i + 2
    } else {
        i + 1
    }
}
