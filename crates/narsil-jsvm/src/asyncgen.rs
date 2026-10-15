//! Release 10.10.2026: ASYNCHRONE GENERATOREN (`async function*`, ES2018
//! 27.6) und `for await` (14.7.5).
//!
//! DER WEG: der Koerper eines asynchronen Generators wird wie ein
//! GEWOEHNLICHER Generator uebersetzt; jedes `await x` darin wird
//! `yield marke(x)` (siehe `compile.rs`, `erwarten_emittieren`). Der Treiber
//! (`asyncGen` unten) zieht den Generator: eine Marke heisst „warte auf x und
//! setze mit dem Ergebnis fort", jeder andere Ertrag ist ein echtes `yield`
//! und loest das Versprechen der wartenden `next()`-Anfrage auf.
//! `asyncIter` ist GetIterator(obj, async) samt CreateAsyncFromSyncIterator.
//!
//! GEMESSEN vorher: `async function*` lief als async-Funktion (der Aufruf
//! lieferte ein Versprechen, `it.next` war `undefined`), und `for await` war
//! ein Syntaxfehler — bbc.com (axios im Next.js-Buendel) brach ab.
//!
//! Die Helfer leben nur in `Engine::hilfen` (GC-Wurzeln), nicht im globalen
//! Bereich; uebersetzt wird der Vorspann beim ERSTEN Bedarf.

use crate::heap::{GVal, HObj};
use crate::interp::Engine;
use crate::interp::Stoerung;

/// `marke(x)`: die Await-Marke im Generator-Koerper.
pub(crate) const HILFE_MARKE: u8 = 0;
/// `asyncIter(obj)`: GetIterator(obj, async).
pub(crate) const HILFE_ASYNC_ITER: u8 = 1;
/// `asyncGen(gen)`: das AsyncGenerator-Objekt zu einem gestarteten Generator.
pub(crate) const HILFE_ASYNC_GEN: u8 = 2;
/// Release 10.10.2026: `lauscherHuelle(obj)`: die Funktion zu einem
/// Lauscher-OBJEKT (`{ handleEvent }`, siehe `Engine::lauscher_huelle`) —
/// je Objekt dieselbe (WeakMap).
pub(crate) const HILFE_LAUSCHER: u8 = 3;
/// Release 15.10.2026: `asyncClose(it)`: AsyncIteratorClose (ES2024 7.4.13)
/// for a `for await` left early — `it.return()` if present, its result
/// awaited by the caller and checked to be an object.
pub(crate) const HILFE_ASYNC_SCHLIESSEN: u8 = 4;
/// Release 15.10.2026: `asyncGenRequest(art, o, wert)`: the body of
/// `%AsyncGeneratorPrototype%.next/return/throw` (the natives in
/// `es_modern.rs` call it, so the prototype exists before this prelude is
/// compiled).
pub(crate) const HILFE_ASYNC_ANFRAGE: u8 = 5;

const VORSPANN: &str = r#"(function () {
  'use strict';
  var P = Promise, S = Symbol, AI = S.asyncIterator, IT = S.iterator;
  var MARKE = {};
  function marke(v) { return { m: MARKE, v: v }; }
  function istMarke(x) { return x !== null && typeof x === 'object' && x.m === MARKE; }
  function vonSync(s) {
    var next = s.next;
    function schritt(r) {
      if (r === null || (typeof r !== 'object' && typeof r !== 'function')) throw new TypeError('Iterator result ' + r + ' is not an object');
      var done = !!r.done;
      return P.resolve(r.value).then(function (v) { return { value: v, done: done }; });
    }
    return {
      next: function (v) { try { return schritt(next.call(s, v)); } catch (e) { return P.reject(e); } },
      'return': function (v) {
        var f = s['return'];
        if (f === undefined || f === null) return P.resolve({ value: v, done: true });
        try { return schritt(f.call(s, v)); } catch (e) { return P.reject(e); }
      },
      'throw': function (v) {
        var f = s['throw'];
        if (f === undefined || f === null) return P.reject(v);
        try { return schritt(f.call(s, v)); } catch (e) { return P.reject(e); }
      }
    };
  }
  function asyncIter(o) {
    var m = o == null ? undefined : o[AI];
    if (m != null) {
      var r = m.call(o);
      if (r === null || (typeof r !== 'object' && typeof r !== 'function')) throw new TypeError('Result of the Symbol.asyncIterator method is not an object');
      return r;
    }
    var sm = o == null ? undefined : o[IT];
    if (typeof sm !== 'function') throw new TypeError((o === null ? 'null' : typeof o) + ' is not async iterable');
    return vonSync(sm.call(o));
  }
  var zustand = new WeakMap();
  function weiter(z) {
    if (z.laeuft || !z.schlange.length) return;
    var a = z.schlange[0];
    if (z.fertig) {
      z.schlange.shift();
      if (a.art === 'throw') a.rej(a.wert);
      else if (a.art === 'return') P.resolve(a.wert).then(function (v) { a.res({ value: v, done: true }); }, a.rej);
      else a.res({ value: undefined, done: true });
      weiter(z);
      return;
    }
    z.laeuft = true;
    schritt(z, a.art, a.wert);
  }
  function schritt(z, art, wert) {
    var r;
    try {
      r = art === 'next' ? z.gen.next(wert) : art === 'throw' ? z.gen['throw'](wert) : z.gen['return'](wert);
    } catch (e) {
      z.fertig = true; z.laeuft = false; z.schlange.shift().rej(e); weiter(z); return;
    }
    var v = r.value;
    if (!r.done && istMarke(v)) {
      P.resolve(v.v).then(function (x) { schritt(z, 'next', x); }, function (e) { schritt(z, 'throw', e); });
      return;
    }
    if (r.done) z.fertig = true;
    P.resolve(v).then(function (x) {
      z.laeuft = false; z.schlange.shift().res({ value: x, done: !!r.done }); weiter(z);
    }, function (e) {
      if (r.done) { z.laeuft = false; z.schlange.shift().rej(e); weiter(z); }
      else schritt(z, 'throw', e);
    });
  }
  function anfrage(art, o, wert) {
    var z = zustand.get(o);
    if (!z) return P.reject(new TypeError('AsyncGenerator.prototype.' + art + ' called on incompatible receiver'));
    return new P(function (res, rej) { z.schlange.push({ art: art, wert: wert, res: res, rej: rej }); weiter(z); });
  }
  function asyncGen(gen, proto) {
    var o = Object.create(proto);
    zustand.set(o, { gen: gen, schlange: [], laeuft: false, fertig: false });
    return o;
  }
  var huellen = new WeakMap();
  function lauscherHuelle(h) {
    var f = huellen.get(h);
    if (!f) {
      f = function (ev) {
        var m = h.handleEvent;
        if (typeof m !== 'function') throw new TypeError("'handleEvent' property of event listener should be a function");
        return m.call(h, ev);
      };
      huellen.set(h, f);
    }
    return f;
  }
  function asyncClose(it) {
    var f = it == null ? undefined : it['return'];
    if (f === undefined || f === null) return undefined;
    return P.resolve(f.call(it)).then(function (r) {
      if (r === null || (typeof r !== 'object' && typeof r !== 'function')) throw new TypeError('iterator.return() did not return an object');
      return r;
    });
  }
  return [marke, asyncIter, asyncGen, lauscherHuelle, asyncClose, anfrage];
})()"#;

impl Engine {
    /// Ein Helfer des Vorspanns (`HILFE_*`), beim ersten Bedarf uebersetzt.
    pub(crate) fn hilfe(&mut self, k: u8) -> Result<GVal, Stoerung> {
        if self.hilfen.is_empty() {
            let vorher = self.eval_global;
            self.eval_global = true;
            let r = self.quelltext_ausfuehren(VORSPANN);
            self.eval_global = vorher;
            let feld = r?;
            let refs: Vec<crate::heap::HeapRef> = match feld {
                GVal::Ref(f) => match self.heap.holen(f) {
                    HObj::Feld { elemente, .. } => elemente
                        .iter()
                        .filter_map(|v| match v {
                            GVal::Ref(x) => Some(*x),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            };
            if refs.len() != 6 {
                let e = self.fehler_wert("TypeError", "Vorspann der asynchronen Generatoren unvollstaendig (intern)");
                return Err(Stoerung::Wurf(e));
            }
            self.hilfen = refs;
        }
        match self.hilfen.get(k as usize) {
            Some(r) => Ok(GVal::Ref(*r)),
            None => {
                let e = self.fehler_wert("TypeError", "unbekannter Helfer (intern)");
                Err(Stoerung::Wurf(e))
            }
        }
    }
}
