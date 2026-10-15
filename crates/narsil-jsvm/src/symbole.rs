//! J24: `Symbol`, `WeakMap`, `WeakSet` — die drei Typen, an denen die eigene
//! test262-Auswahl scheiterte (`tests/t262/luecken.txt`).
//!
//! # DIE DARSTELLUNG — bewusst ohne neue Heap-Art
//!
//! `Symbol` ist ein **Gegenstand mit dem Prototyp `proto_symbol`**. GEMESSEN
//! warum das reicht und ein eigener `HObj`-Zweig nicht nötig ist:
//!
//! * `typeof` prüft im VM genau **eine** Stelle (`Op::Typof`) — dort genügt
//!   „der Prototyp ist `proto_symbol`".
//! * Als Eigenschaftsschluessel wird der Text `@sym<uid>` benutzt
//!   (`schluessel_text`); die `uid` macht jedes Symbol eindeutig, und
//!   `Symbol.for('x')` liefert **dasselbe** Objekt → denselben Schluessel.
//!   Damit stimmen `{[sym]: 1}` und `o[sym]` ohne Heap-Art.
//! * Der **schwache** Teil von WeakMap/WeakSet fehlt bewusst: die Paare
//!   liegen im Objekt, und der GC räumt das Objekt **mitsamt** den Werten.
//!   Das ist eine dokumentierte Naeherung (der WeakMap-Aufsitzer haelt die
//!   Schluessel stark) — see `docs`-Kopf und STATUS_JS.txt.
//!
//! # DIE MESSUNG
//!
//! `tests/t262/faelle/typ-symbol.js`, `weakmap-symbol.js` (Soll aus `node 22`),
//! dazu `tests/j24_symbole.rs` mit 30 Faellen gegen node.

use crate::builtin::Nativ;
use crate::heap::{GVal, HObj, Heap};
use crate::interp::{Engine, Stoerung};

// Release 15.10.2026: `symbol_text`/`unscopables_text` (the `@sym<uid>`
// keys) are gone — no key has that form any more (well-known symbols use
// `Symbol(Symbol.x)`, see `symbol_schluessel`), and `with` reads
// `Symbol(Symbol.unscopables)` (interp.rs).

/// Ist der Slot ein Symbol?
pub(crate) fn ist_symbol(e: &Engine, r: crate::heap::HeapRef) -> bool {
    match e.heap.holen(r) {
        HObj::Gegenstand { proto: Some(p), .. } => *p == e.proto_symbol,
        // ## NEU (JS2/QUIC, 14:30). **GEMESSEN, `node 22` (14:20):**
        //   `Array.of.call(Symbol, 1)` -> TypeError
        //   `Array.from.call(Symbol, [1])` -> TypeError
        //   bei uns (vorher): **kein** Wurf, es kam `Symbol(1)` zurueck.
        //
        // **BELEGT:** `builtin.rs:918` baut den Konstruktor als
        // `mk_ctor(self, Nativ::SymCtor, self.proto_symbol, "Symbol")`, und
        // `mk_ctor` (Zeile 813) legt `proto` in der **Seitentabelle**
        // `nativ_proto` ab — **nicht** in einem Feld von `HObj::Nativ`.
        // (Mein erster Versuch las `HObj::Nativ { proto }`; das ist
        // `e.proto_funktion` und damit **immer** falsch. Der Weg ist derselbe,
        // den `eigenschaften.rs:385` geht.)
        //
        // Der alte Arm sah nur `HObj::Gegenstand` und erkannte also die
        // **Wert**-Huelle, nicht die **Funktion**. Genau daran scheitert
        // `ist_konstruktor` (`eingebaut_array.rs:121`), und daran wieder
        // `Array.of`/`Array.from`.
        HObj::Nativ { .. } => e.nativ_proto.get(&r).copied() == Some(e.proto_symbol),
        _ => false,
    }
}

impl Engine {
    /// `Symbol(desc)` / `Symbol.for(key)`.
    pub(crate) fn n_symbol_ctor(
        &mut self,
        _this: GVal,
        args: &[GVal],
        als_konstruktor: bool,
        registry: bool,
    ) -> Result<GVal, Stoerung> {
        if registry {
            // `Symbol.for` ist **statisch** — `new` ist dort nicht erlaubt
            // (node: `new Symbol.for('x')` -> TypeError).
            if als_konstruktor {
                return Err(Stoerung::Wurf(self.fehler_wert(
                    "TypeError",
                    "Symbol.for ist keine Konstruktorfunktion",
                )));
            }
        }
        // GEMESSEN/GEFUNDEN (und gegen node geprueft): `Symbol('x')` ist
        // **auch ohne** `new` erlaubt — die Funktion gibt ein Symbol
        // zurueck (anders als eine Klasse). Die erste Fassung werfe hier,
        // und `typ-symbol` meldete genau das.
        let _ = als_konstruktor;
        let arg0 = self.arg(args, 0);
        // `Symbol.for(key)` — die Registry haelt je Schluessel **ein**
        // Symbol (Spec 20.4.2.2); `Symbol.keyFor` liefert den Schluessel
        // zurueck, sonst `undefined`.
        if registry {
            let k = self.zu_text(arg0).unwrap_or_default();
            if let Some(v) = self.symbol_registry.get(&k) {
                return Ok(GVal::Ref(*v));
            }
            let s = self.symbol_neu(&k, true)?;
            self.symbol_registry.insert(k, s);
            return Ok(GVal::Ref(s));
        }
        // `Symbol(desc)` — die Beschreibung darf fehlen (`undefined`).
        let beschreibung = if matches!(arg0, GVal::Undefiniert) {
            String::new()
        } else {
            self.zu_text(arg0).unwrap_or_default()
        };
        let r = self.symbol_neu(&beschreibung, false)?;
        Ok(GVal::Ref(r))
    }

    /// Ein Symbol-Objekt bauen (Beschreibung + Eindeutigkeit).
    fn symbol_neu(
        &mut self,
        beschreibung: &str,
        registriert: bool,
    ) -> Result<crate::heap::HeapRef, Stoerung> {
        let obj = HObj::Gegenstand {
            proto: Some(self.proto_symbol),
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
            reihenfolge: Vec::new(),
        };
        let kosten = Heap::kosten_fuer(&obj) + beschreibung.len();
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        let r = self.belege(obj, kosten);
        self.naechste_symbol += 1;
        self.versteckt_setzen(r, "__uid", GVal::Zahl(self.naechste_symbol as f64))?;
        let t = self.text_wert(beschreibung).map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(r, "__text", t)?;
        // Release 15.10.2026: no `__registriert` property any more — nothing
        // read it (`Symbol.keyFor` asks `symbol_registry`), and it cost a
        // property and a string per symbol (baseline heap, `j21_gc_wurzel`).
        let _ = registriert;
        Ok(r)
    }

    /// Die Eindeutigkeitsnummer eines Symbols.
    fn symbol_uid(&self, r: crate::heap::HeapRef) -> u64 {
        match self.eigene_lese(r, "__uid") {
            Some(GVal::Zahl(z)) => z as u64,
            _ => 0,
        }
    }

    /// Die Beschreibung eines Symbols (leer bei `Symbol()`).
    fn symbol_beschreibung(&mut self, r: crate::heap::HeapRef) -> String {
        match self.eigene_lese(r, "__text") {
            Some(v) => self.text_von(v),
            None => String::new(),
        }
    }

    /// `Symbol.keyFor(sym)`.
    pub(crate) fn n_symbol_key_for(
        &mut self,
        _this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = self.arg(args, 0) else {
            return Ok(GVal::Undefiniert);
        };
        // GEMESSEN/ENTSCHIEDEN: die Registry wird **vor** dem Lesen kopiert
        // (sonst leiht `self` zweimal).
        let treffer = self
            .symbol_registry
            .iter()
            .find(|(_, v)| **v == r)
            .map(|(k, _)| k.clone());
        match treffer {
            Some(k) => self.text_wert(&k).map_err(Stoerung::Fehler),
            None => Ok(GVal::Undefiniert),
        }
    }

    /// `Symbol.iterator` und die anderen **woh bekannten** Symbole.
    pub(crate) fn n_symbol_bekannt(
        &mut self,
        _this: GVal,
        _args: &[GVal],
        art: Nativ,
    ) -> Result<GVal, Stoerung> {
        // GEMESSEN (node 22): `Symbol.iterator === Symbol.iterator` -> true.
        // Also **ein** Objekt je wohlbekanntem Symbol (nicht neu je Zugriff).
        let uid = match art {
            Nativ::SymIterator => 1u64,
            Nativ::SymAsyncIterator => 2,
            Nativ::SymToPrimitive => 3,
            Nativ::SymHasInstance => 4,
            Nativ::SymSpecies => 5,
            Nativ::SymToStringTag => 6,
            // J52: `Symbol.unscopables` (ES2023 19.1.3, uid 7) — es fehlte
            // ganz. Ohne es kann `with` die Ausnahme nicht pruefen.
            //
            // GEMESSEN (node 22):
            //   `var o = {a: 1, [Symbol.unscopables]: {a: true}};
            //    var a = 2; with (o) { 'a=' + a }` -> `a=2`
            //   `Symbol.unscopables === Symbol.unscopables` -> `true`
            Nativ::SymUnscopables => 7,
            _ => 0,
        };
        self.wohlbekanntes_symbol(uid)
    }

    /// Release 15.10.2026: the well-known symbol number `uid` (1-based index
    /// into `WOHLBEKANNT`), created on first use and then always the same.
    pub(crate) fn wohlbekanntes_symbol(&mut self, uid: u64) -> Result<GVal, Stoerung> {
        if let Some((_, v)) = self.symbol_bekannt.iter().find(|(u, _)| *u == uid) {
            return Ok(GVal::Ref(*v));
        }
        let name = WOHLBEKANNT.get((uid as usize).wrapping_sub(1)).copied().unwrap_or("?");
        let r = self.symbol_neu(&format!("Symbol.{name}"), false)?;
        self.symbol_bekannt.push((uid, r));
        Ok(GVal::Ref(r))
    }

    /// Release 15.10.2026: the well-known symbols as properties of `Symbol`
    /// (ECMA-262 20.4.2): DATA properties, `{writable: false, enumerable:
    /// false, configurable: false}`, all thirteen of ES2025 plus `dispose`
    /// and `asyncDispose` (explicit resource management, shipped in node 22).
    /// MEASURED before: seven getters (`Object.getOwnPropertyDescriptor(Symbol,
    /// 'iterator').value` was undefined) and `Symbol.match`, `matchAll`,
    /// `replace`, `search`, `split`, `isConcatSpreadable`, `dispose` and
    /// `asyncDispose` were missing, so `{[Symbol.split]: f}` used the key
    /// "undefined".
    pub(crate) fn wohlbekannte_symbole_eintragen(&mut self, ctor: crate::heap::HeapRef) {
        for (i, name) in WOHLBEKANNT.iter().enumerate() {
            let Ok(GVal::Ref(s)) = self.wohlbekanntes_symbol(i as u64 + 1) else {
                continue;
            };
            if let HObj::Nativ { eigenschaften, .. } = self.heap.holen_mut(ctor) {
                eigenschaften.insert(
                    name.to_string(),
                    crate::heap::Eigenschaft {
                        wert: GVal::Ref(s),
                        getter: None,
                        setter: None,
                        aufzählbar: false,
                        beschreibbar: false,
                        konfigurierbar: false,
                    },
                );
            }
        }
    }

    /// Release 09.10.2026: der EIGENSCHAFTSSCHLUESSEL eines Symbols. Die
    /// wohlbekannten behalten `Symbol(Symbol.iterator)` (darauf greift der
    /// Motor an vielen Stellen zu), alle anderen tragen ihre Nummer:
    /// `Symbol(beschreibung)@17`.
    ///
    /// GEMESSEN, warum: der Schluessel war `String(sym)` — zwei Symbole mit
    /// gleicher (oder leerer) Beschreibung teilten sich den Platz. node:
    /// `var a = Symbol(), b = Symbol(), o = {}; o[a] = 1; o[b] = 2; o[a]` ->
    /// `1`, der Motor `2`. youtube.com legt so Caches an (`P = Symbol();
    /// NFE = Symbol(); … t[NFE]`) und bekam das Objekt aus `t[P]` statt der
    /// Serialisier-Funktion: „R is not a function".
    pub(crate) fn symbol_schluessel(&mut self, r: crate::heap::HeapRef) -> String {
        let b = self.symbol_beschreibung(r);
        let schluessel = if self.symbol_bekannt.iter().any(|(_, s)| *s == r) {
            format!("Symbol({b})")
        } else {
            format!("Symbol({b})@{}", self.symbol_uid(r))
        };
        if !self.symbol_nach_schluessel.contains_key(&schluessel) {
            self.symbol_nach_schluessel.insert(schluessel.clone(), r);
        }
        schluessel
    }

    /// Release 09.10.2026: `Object.getOwnPropertySymbols(o)` (ES2023
    /// 20.1.2.11) — die eigenen Symbolschluessel als Symbole.
    pub(crate) fn n_objekt_eigene_symbole(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let namen = self.n_objekt_namen(args)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(namen);
        let texte: Vec<String> = match namen {
            GVal::Ref(f) => match self.heap.holen(f) {
                HObj::Feld { elemente, .. } => elemente
                    .iter()
                    .filter_map(|e| match e {
                        GVal::Ref(t) => match self.heap.holen(*t) {
                            HObj::Zeichenkette(s) => Some(s.clone()),
                            _ => None,
                        },
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        let mut symbole = Vec::new();
        for t in texte.iter().filter(|t| crate::heap::ist_symbolschluessel(t)) {
            if let Some(s) = self.symbol_nach_schluessel.get(t).copied() {
                symbole.push(GVal::Ref(s));
                continue;
            }
            // Wohlbekannte Symbole, die der Motor selbst eingetragen hat.
            let bekannt: Vec<crate::heap::HeapRef> = self.symbol_bekannt.iter().map(|(_, s)| *s).collect();
            for s in bekannt {
                if format!("Symbol({})", self.symbol_beschreibung(s)) == *t {
                    symbole.push(GVal::Ref(s));
                    break;
                }
            }
        }
        let f = self.feld_aus_werten(&symbole)?;
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(f))
    }

    /// `sym.toString()` / `sym.description` / `String(sym)`.
    pub(crate) fn n_symbol_text(
        &mut self,
        this: GVal,
        _args: &[GVal],
        nur_beschreibung: bool,
    ) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let b = self.symbol_beschreibung(r);
        let s = if nur_beschreibung {
            b
        } else {
            format!("Symbol({b})")
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    // -----------------------------------------------------------------
    // WeakMap / WeakSet
    // -----------------------------------------------------------------

    /// `new WeakMap()` / `new WeakSet()`.
    pub(crate) fn n_weak_ctor(
        &mut self,
        _this: GVal,
        args: &[GVal],
        schwach: bool,
    ) -> Result<GVal, Stoerung> {
        // GEMESSEN (node): `new WeakMap([[k,v]])` und `new WeakMap(iterable)`
        // sind erlaubt; ohne Argument ist die Map leer.
        let paare: Vec<(GVal, GVal)> = match self.arg(args, 0) {
            GVal::Undefiniert => Vec::new(),
            GVal::Ref(f) if self.ist_feld(f) => {
                let mut r = Vec::new();
                for p in self.feld_elemente(f) {
                    // `new WeakSet([a, b])`: jedes Element ist ein Schluessel.
                    if schwach {
                        r.push((p, GVal::Bool(true)));
                        continue;
                    }
                    let teile = match p {
                        GVal::Ref(x) if self.ist_feld(x) => self.feld_elemente(x),
                        GVal::Ref(x) if ist_symbol(self, x) => vec![p, GVal::Undefiniert],
                        _ => continue,
                    };
                    if let Some(k) = teile.first() {
                        r.push((*k, teile.get(1).copied().unwrap_or(GVal::Undefiniert)));
                    }
                }
                r
            }
            _ => Vec::new(),
        };
        let obj = HObj::Gegenstand {
            proto: Some(if schwach {
                self.proto_weakset
            } else {
                self.proto_weakmap
            }),
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
            reihenfolge: Vec::new(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        let r = self.belege(obj, kosten);
        // Release 10.10.2026: die Tabelle liegt im Heap (Ephemerone, siehe
        // `Heap::schwach`); die Karte haelt sich bis zum Ende selbst als
        // Wurzel, denn `weak_eintrag` kann sammeln.
        self.heap.schwach.insert(r.0, std::collections::HashMap::new());
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(r));
        for (k, v) in &paare {
            let w = if schwach { GVal::Bool(true) } else { *v };
            if let Err(e) = self.weak_eintrag(r.0, *k, w, schwach) {
                self.wurzel_zurueck(marke);
                return Err(e);
            }
        }
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(r))
    }

    /// Der Slot einer WeakMap/WeakSet (ihre Tabelle liegt im Heap).
    fn weak_feld(&mut self, this: GVal) -> Option<u32> {
        let GVal::Ref(r) = this else {
            return None;
        };
        self.heap.schwach.contains_key(&r.0).then_some(r.0)
    }

    /// Ein Paar schreiben (Schluessel muessen Objekt oder Symbol sein).
    fn weak_eintrag(&mut self, karte: u32, key: GVal, wert: GVal, schwach: bool) -> Result<(), Stoerung> {
        // GEMESSEN (node): ein **Primitive** als Schluessel wirft
        // `TypeError: Invalid value used as weak map key`.
        // Release 10.10.2026: JEDES Objekt ist ein gueltiger Schluessel
        // (ES2023 24.3.3.5, CanBeHeldWeakly) — nur Texte (und interne
        // Umgebungen) nicht. GEMESSEN vorher: `Map`, `Promise`, `Date`,
        // `RegExp` und `Error` warfen „Invalid value used as weak map key"
        // — und Next.js brach auf bbc.com damit die Hydration ab.
        let gueltig = match key {
            GVal::Ref(r) => !matches!(
                self.heap.holen(r),
                HObj::Zeichenkette(_) | HObj::Umgebung(_)
            ),
            _ => false,
        };
        let GVal::Ref(k) = key else { return Err(self.weak_fehler(schwach)) };
        if !gueltig {
            return Err(self.weak_fehler(schwach));
        }
        if let Some(t) = self.heap.schwach.get_mut(&karte)
            && let Some(alt) = t.get_mut(&k.0)
        {
            // Wert ersetzen (der Schluessel bleibt).
            *alt = wert;
            return Ok(());
        }
        let kosten = crate::heap::KOSTEN_EINTRAG;
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        // `vor_allok` kann gesammelt haben — die Karte lebt (Wurzel des
        // Aufrufers), der Schluessel auch (Argument).
        if let Some(t) = self.heap.schwach.get_mut(&karte) {
            t.insert(k.0, wert);
            self.heap.kosten_verbuchen(kosten);
        }
        Ok(())
    }

    fn weak_fehler(&mut self, schwach: bool) -> Stoerung {
        Stoerung::Wurf(self.fehler_wert(
            "TypeError",
            if schwach {
                "Invalid value used in weak set"
            } else {
                "Invalid value used as weak map key"
            },
        ))
    }

    /// Der Wert zu einem Schluessel (`get`).
    fn weak_wert(&mut self, this: GVal, key: GVal) -> Option<GVal> {
        let karte = self.weak_feld(this)?;
        let GVal::Ref(k) = key else { return None };
        self.heap.schwach.get(&karte)?.get(&k.0).copied()
    }

    pub(crate) fn n_weak_get(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        Ok(self
            .weak_wert(this, self.arg(args, 0))
            .unwrap_or(GVal::Undefiniert))
    }

    pub(crate) fn n_weak_hat(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        Ok(GVal::Bool(
            self.weak_wert(this, self.arg(args, 0)).is_some(),
        ))
    }

    pub(crate) fn n_weak_set(
        &mut self,
        this: GVal,
        args: &[GVal],
        schwach: bool,
    ) -> Result<GVal, Stoerung> {
        let Some(karte) = self.weak_feld(this) else {
            return Ok(GVal::Undefiniert);
        };
        let wert = if schwach {
            GVal::Bool(true)
        } else {
            self.arg(args, 1)
        };
        self.weak_eintrag(karte, self.arg(args, 0), wert, schwach)?;
        // `set`/`add` geben die Karte selbst zurueck (Verkettung).
        Ok(this)
    }

    pub(crate) fn n_weak_delete(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let Some(karte) = self.weak_feld(this) else {
            return Ok(GVal::Bool(false));
        };
        let GVal::Ref(k) = self.arg(args, 0) else { return Ok(GVal::Bool(false)) };
        let weg = self.heap.schwach.get_mut(&karte).and_then(|t| t.remove(&k.0)).is_some();
        if weg {
            self.heap.kosten_freigeben(crate::heap::KOSTEN_EINTRAG);
        }
        Ok(GVal::Bool(weg))
    }

    /// `WeakSet#add` (und `WeakMap#set` ueber dieselbe Natives mit `schwach`).
    pub(crate) fn n_weak_adder(
        &mut self,
        this: GVal,
        args: &[GVal],
        schwach: bool,
    ) -> Result<GVal, Stoerung> {
        self.n_weak_set(this, args, schwach)
    }
}

/// Release 15.10.2026: the well-known symbols in `uid` order (uid = index
/// + 1; the first seven keep the numbers they had).
pub(crate) const WOHLBEKANNT: [&str; 15] = [
    "iterator",
    "asyncIterator",
    "toPrimitive",
    "hasInstance",
    "species",
    "toStringTag",
    "unscopables",
    "isConcatSpreadable",
    "match",
    "matchAll",
    "replace",
    "search",
    "split",
    "dispose",
    "asyncDispose",
];

/// Release 15.10.2026: property keys of the well-known symbols added here
/// (the key of a well-known symbol is `Symbol(<description>)`).
pub(crate) const SYM_IS_CONCAT_SPREADABLE: &str = "Symbol(Symbol.isConcatSpreadable)";
pub(crate) const SYM_MATCH: &str = "Symbol(Symbol.match)";
pub(crate) const SYM_MATCH_ALL: &str = "Symbol(Symbol.matchAll)";
pub(crate) const SYM_REPLACE: &str = "Symbol(Symbol.replace)";
pub(crate) const SYM_SEARCH: &str = "Symbol(Symbol.search)";
pub(crate) const SYM_SPLIT: &str = "Symbol(Symbol.split)";
pub(crate) const SYM_SPECIES: &str = "Symbol(Symbol.species)";
