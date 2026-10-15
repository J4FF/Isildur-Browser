// BEGRUENDUNG fuer `dead_code`: die Aufrufer dieser Datei lagen in `vm.rs`,
// das am 03.10. verloren ging und nur als 30.09.-Stand wieder da ist
// (`team/NOTIZ_VM_VERLOREN.txt`). Der Code hier ist nicht tot, sondern
// **wartet** — er kommt wieder, sobald der VM-Teil nachgezogen ist.
#![allow(dead_code)]
//! Klassen, `arguments`, Versprechen-Abwicklung, Wert-Export
//! (Fortsetzung von eigenschaften.rs).

use std::collections::{HashMap, HashSet};

use crate::fehler::JsFehler;
use crate::heap::{Eigenschaft, GVal, HObj, Heap, HeapRef};
use crate::interp::{Engine, Stoerung};
use crate::wert::Wert;

impl Engine {
    // ---------------------------------------------------------------
    // Klassen
    // ---------------------------------------------------------------

    pub(crate) fn eigenschaft_nicht_aufzaehlbar(
        &mut self,
        obj: HeapRef,
        k: &str,
        v: GVal,
    ) -> Result<(), JsFehler> {
        self.vor_allok(crate::heap::KOSTEN_EINTRAG)?;
        let o = self.heap.holen_mut(obj);
        let map = match o {
            HObj::Gegenstand { eigenschaften, .. } => eigenschaften,
            HObj::Funktion { eigenschaften, .. } | HObj::Nativ { eigenschaften, .. } => {
                eigenschaften
            }
            _ => return Err(JsFehler::Laufzeit("Klassen-Heim korrupt".to_string())),
        };
        map.insert(
            k.to_string(),
            Eigenschaft {
                konfigurierbar: true,
                wert: v,
                getter: None,
                setter: None,
                aufzählbar: false,
                beschreibbar: true,
            },
        );
        self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
        Ok(())
    }

    // ---------------------------------------------------------------
    // arguments
    // ---------------------------------------------------------------

    /// Release 15.10.2026: a plain object with the enumerable own string
    /// properties of the arguments object `r` (indices first), for
    /// JSON.stringify.
    pub(crate) fn argumente_als_objekt(&mut self, r: HeapRef) -> Result<GVal, Stoerung> {
        let (elemente, luecken, rest) = match self.heap.holen(r) {
            HObj::Feld { elemente, luecken, rest, .. } => (elemente.clone(), luecken.clone(), rest.clone()),
            _ => return Ok(GVal::Ref(r)),
        };
        let o = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(o));
        for (i, v) in elemente.iter().enumerate() {
            if luecken.as_ref().is_some_and(|l| l.contains(&i)) {
                continue;
            }
            self.daten_setzen(o, &i.to_string(), *v);
        }
        for (k, e) in rest.iter() {
            if e.aufzählbar && !crate::heap::ist_privat(k) && !crate::heap::ist_symbolschluessel(k) {
                let w = match self.hole(GVal::Ref(r), k) {
                    Ok(w) => w,
                    Err(f) => {
                        self.wurzel_zurueck(marke);
                        return Err(f);
                    }
                };
                self.daten_setzen(o, k, w);
            }
        }
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(o))
    }

    pub(crate) fn argumente_anlegen(&mut self, args: &[GVal]) -> Result<HeapRef, Stoerung> {
        let f = self.neues_feld().map_err(Stoerung::Fehler)?;
        // Release 15.10.2026: an arguments object is an ORDINARY object
        // (ECMA-262 10.4.4: [[Prototype]] %Object.prototype%, not an Array)
        // with an own `@@iterator` = %Array.prototype.values% (10.4.4.6
        // step 7). The indexed storage stays a `Feld`; the private marker
        // `#es_args` tells IsArray, Object.prototype.toString, JSON and
        // IsConcatSpreadable apart. MEASURED before (node 22 in brackets):
        // `Array.isArray(arguments)` true (false), `typeof arguments.map`
        // "function" ("undefined"), `Object.prototype.toString.call(
        // arguments)` "[object Array]" ("[object Arguments]"),
        // `JSON.stringify(arguments)` "[1,2]" ('{"0":1,"1":2}').
        let werte = self.es.feld_werte_fn;
        let po = self.proto_objekt;
        let o = self.heap.holen_mut(f);
        if let HObj::Feld { elemente, rest, proto, .. } = o {
            *proto = Some(po);
            rest.insert(
                crate::builtin::ARGUMENTE_MARKE.to_string(),
                Eigenschaft {
                    wert: GVal::Bool(true),
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: false,
                    konfigurierbar: false,
                },
            );
            if let Some(w) = werte {
                rest.insert(
                    "Symbol(Symbol.iterator)".to_string(),
                    Eigenschaft {
                        wert: GVal::Ref(w),
                        getter: None,
                        setter: None,
                        aufzählbar: false,
                        beschreibbar: true,
                        konfigurierbar: true,
                    },
                );
            }
            elemente.extend_from_slice(args);
            // callee: bewusst undefined (strict wirft; tolerant undefined).
            rest.insert(
                "callee".to_string(),
                Eigenschaft {
                    wert: GVal::Undefiniert,
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
        }
        self.heap
            .kosten_verbuchen((args.len() + 2) * crate::heap::KOSTEN_EINTRAG);
        Ok(f)
    }

    // ---------------------------------------------------------------
    // Versprechen (Promise-Kern)
    // ---------------------------------------------------------------

    pub(crate) fn versprechen_anlegen(&mut self, zustand: crate::heap::VersprechenZustand) -> GVal {
        let obj = HObj::Versprechen {
            proto: Some(self.proto_versprechen),
            zustand,
            reaktionen: Vec::new(),
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = Heap::kosten_fuer(&obj);
        if self.vor_allok(kosten).is_err() {
            // Notfall wie fehler_wert (winzig, gezaehlt).
            let r = self.belege(obj, kosten);
            return GVal::Ref(r);
        }
        GVal::Ref(self.belege(obj, kosten))
    }

    pub(crate) fn versprechen_erfuellt(&mut self, w: GVal) -> GVal {
        // Release 15.10.2026: `w` is often a value the caller just made
        // (a parsed JSON body, a buffer) and held only in Rust; it stays
        // rooted while the promise is built (`then` lookup and the
        // allocation can collect). MEASURED before (chip.de,
        // NARSIL_JS_GC_STRESS=29): `response.json()` returned a swept
        // promise ("Heap: toter Verweis" at the `return`).
        let marke = self.wurzel_merke();
        self.wurzeln.push(w);
        let p = self.versprechen_erfuellt_innen(w);
        self.wurzel_zurueck(marke);
        p
    }

    fn versprechen_erfuellt_innen(&mut self, w: GVal) -> GVal {
        // Assimilation: Promise bleibt Promise (kein Verschachteln).
        if let GVal::Ref(r) = w {
            if matches!(self.heap.holen(r), HObj::Versprechen { .. }) {
                return w;
            }
        }
        // J9: **Thenable** uebernehmen (sec-27.2.3.3): ein Objekt mit
        // aufrufbarem `then` wird uebernommen — der Motor hat es vorher
        // ignoriert. GEMESSEN: `Promise.resolve({then(res){…}}).then(…)`
        // rief `then` nie auf (node: `then-aufgerufen,p1,t0`).
        if let GVal::Ref(r) = w
            && !matches!(
                self.heap.holen(r),
                HObj::Funktion { .. } | HObj::Nativ { .. }
            )
        {
            let f = self.hole(w, "then").unwrap_or(GVal::Undefiniert);
            if self.ist_aufrufbar(f) {
                let ziel = match self.versprechen_anlegen(crate::heap::VersprechenZustand::Offen) {
                    GVal::Ref(z) => z,
                    _ => unreachable!(),
                };
                self.thenable_einreihen(ziel, f, w);
                return GVal::Ref(ziel);
            }
        }
        self.versprechen_anlegen(crate::heap::VersprechenZustand::Erfuellt(w))
    }

    /// J9: `then` als **Mikrotask** einreihen (PromiseResolveThenableJob).
    pub(crate) fn thenable_einreihen(&mut self, ziel: HeapRef, then_fn: GVal, thenable: GVal) {
        let GVal::Ref(f) = then_fn else {
            self.versprechen_settle(ziel, true, thenable);
            return;
        };
        if self.mikrotasks.len() >= self.grenzen.max_mikrotasks {
            let e = self.fehler_wert("RangeError", "zu viele Mikrotasks (Grenze)");
            self.versprechen_settle(ziel, false, e);
            return;
        }
        self.mikrotasks
            .push(crate::interp::Mikrotask::thenable_job(f, thenable, ziel));
    }

    pub(crate) fn versprechen_abgelehnt(&mut self, w: GVal) -> GVal {
        self.versprechen_anlegen(crate::heap::VersprechenZustand::Abgelehnt(w))
    }

    /// Reaktion einreihen (fuer then/catch/finally + await-Pumpen).
    pub(crate) fn reaktion_einreihen(
        &mut self,
        ziel: HeapRef,
        bei_erfuellt: Option<HeapRef>,
        bei_fehler: Option<HeapRef>,
        folge: HeapRef,
    ) {
        self.reaktion_einreihen_art(ziel, bei_erfuellt, bei_fehler, folge, 0)
    }

    /// Reaktion einreihen mit Art (`finally` = 1).
    pub(crate) fn reaktion_einreihen_art(
        &mut self,
        ziel: HeapRef,
        bei_erfuellt: Option<HeapRef>,
        bei_fehler: Option<HeapRef>,
        folge: HeapRef,
        art: u8,
    ) {
        self.behandelt(ziel);
        if self.mikrotasks.len() >= self.grenzen.max_mikrotasks {
            return; // Gedeckelt (M6): Ueberhang faellt weg (dokumentiert).
        }
        let obj = self.heap.holen_mut(ziel);
        if let HObj::Versprechen { reaktionen, .. } = obj {
            reaktionen.push(crate::heap::Reaktion {
                bei_erfuellt,
                bei_fehler,
                folge,
                fortsetzung: None,
                art,
            });
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
        }
    }

    /// Versprechen sofort erfuellen/ablehnen + wartende Reaktionen als
    /// Mikrotasks einreihen. `marke` nicht noetig (wurzelt selbst).
    /// J9: **Assimilation** (sec-27.2.1.3.2, Promise Resolution Procedure).
    /// `true`, wenn der Wert uebernommen wurde und `ziel` deshalb **nicht**
    /// direkt gesetzt werden darf.
    ///
    /// GEMESSEN, warum das noetig war: ein Versprechen, das ein **Versprechen
    /// oder ein Thenable** als Wert bekommt, muss dieses uebernehmen. Vorher
    /// blieb das Versprechen-Objekt als Wert stehen:
    ///   `Promise.resolve(1).then(()=>Promise.resolve(9)).then(v=>…)`
    ///   lieferte `[object Promise]` statt `9`, und
    ///   `(async()=>Promise.resolve(6))().then(v=>…)` ebenso. Das trifft
    ///   jede verkettete `fetch`-Kette in einem Seitenskript.
    /// Release 10.10.2026: `p` hat einen Handler bekommen (`then`, `catch`,
    /// `finally`, `await`, Uebernahme) — keine „Uncaught (in promise)"-Meldung.
    /// Release 10.10.2026: „Uncaught (in promise) …" fuer abgelehnte
    /// Versprechen ohne Handler (HTML §8.1.7.3) an- oder abschalten.
    pub fn unbehandelte_ablehnungen_melden(&mut self, an: bool) {
        self.melde_unbehandelt = an;
        if !an {
            self.unbehandelt.clear();
        }
    }

    pub(crate) fn behandelt(&mut self, p: HeapRef) {
        if !self.unbehandelt.is_empty() {
            self.unbehandelt.retain(|x| *x != p);
        }
    }

    /// Release 10.10.2026: die Meldung fuer alle abgelehnten Versprechen, die
    /// bis zum Ende der Mikrotasks keinen Handler bekommen haben (Chrome:
    /// „Uncaught (in promise) …").
    pub(crate) fn unbehandelt_melden(&mut self) {
        if self.unbehandelt.is_empty() {
            return;
        }
        let liste = std::mem::take(&mut self.unbehandelt);
        for p in liste {
            let grund = match self.heap.holen(p) {
                HObj::Versprechen { zustand: crate::heap::VersprechenZustand::Abgelehnt(w), .. } => *w,
                _ => continue,
            };
            let text = self.wurf_text_mit_stapel(grund);
            if self.konsole.len() < 10_000 {
                self.konsole.push(format!("Uncaught (in promise) {text}"));
            }
        }
    }

    pub(crate) fn assimilation(&mut self, ziel: HeapRef, wert: GVal) -> bool {
        let GVal::Ref(r) = wert else { return false };
        if r == ziel {
            return true; // sich selbst: ignorieren (sonst Endlos-Rekursion)
        }
        // 1) Versprechen: Zustand uebernehmen oder verketten.
        let zustand = match self.heap.holen(r) {
            HObj::Versprechen { zustand, .. } => {
                let z = zustand.clone();
                self.behandelt(r);
                z
            }
            _ => {
                // 2) Thenable: `then` als Mikrotask aufrufen. Funktionen
                // zaehlen nicht als Thenable (sie haben kein `then`).
                if matches!(
                    self.heap.holen(r),
                    HObj::Funktion { .. } | HObj::Nativ { .. }
                ) {
                    return false;
                }
                match self.hole(wert, "then") {
                    Ok(f) if self.ist_aufrufbar(f) => {
                        self.thenable_einreihen(ziel, f, wert);
                        return true;
                    }
                    Ok(_) => return false,
                    // Getter des `then` wirft: das Versprechen lehnt ab
                    // (sec-27.2.1.3.2 Schritt 3).
                    Err(e) => {
                        let w = match e {
                            crate::interp::Stoerung::Wurf(w) => w,
                            crate::interp::Stoerung::Fehler(f) => {
                                let m = f.to_string();
                                self.fehler_wert("TypeError", &m)
                            }
                        };
                        self.versprechen_settle(ziel, false, w);
                        return true;
                    }
                }
            }
        };
        match zustand {
            crate::heap::VersprechenZustand::Erfuellt(w) => self.versprechen_settle(ziel, true, w),
            crate::heap::VersprechenZustand::Abgelehnt(w) => {
                self.versprechen_settle(ziel, false, w)
            }
            crate::heap::VersprechenZustand::Offen => {
                let erf2 = self.resolver_anlegen(ziel, true);
                let abl2 = self.resolver_anlegen(ziel, false);
                self.reaktion_einreihen(r, Some(erf2), Some(abl2), ziel);
            }
        }
        true
    }

    pub(crate) fn versprechen_settle(&mut self, ziel: HeapRef, erfuellt: bool, wert: GVal) {
        // J9: erst die Assimilation (Versprechen/Thenable uebernehmen).
        if erfuellt && self.assimilation(ziel, wert) {
            return;
        }
        let reaktionen = {
            let obj = self.heap.holen_mut(ziel);
            match obj {
                HObj::Versprechen {
                    zustand,
                    reaktionen,
                    ..
                } => {
                    // Bereits entschieden: nichts tun (JS-Semantik).
                    if !matches!(zustand, crate::heap::VersprechenZustand::Offen) {
                        return;
                    }
                    *zustand = if erfuellt {
                        crate::heap::VersprechenZustand::Erfuellt(wert)
                    } else {
                        crate::heap::VersprechenZustand::Abgelehnt(wert)
                    };
                    std::mem::take(reaktionen)
                }
                _ => return,
            }
        };
        // Release 10.10.2026: abgelehnt und (noch) ohne Handler.
        if self.melde_unbehandelt && !erfuellt && reaktionen.is_empty() && self.unbehandelt.len() < 4096 {
            self.unbehandelt.push(ziel);
        }
        for r in reaktionen {
            if self.mikrotasks.len() >= self.grenzen.max_mikrotasks {
                break;
            }
            // V3: Fortsetzung eines Generators (das ist `await`).
            if let Some(g) = r.fortsetzung {
                let aufnahme = if erfuellt {
                    crate::vm::Aufnahme::Wert(wert)
                } else {
                    crate::vm::Aufnahme::Wurf(wert)
                };
                self.mikrotasks
                    .push(crate::interp::Mikrotask::fortsetzung(g, aufnahme));
                continue;
            }
            self.mikrotasks.push(crate::interp::Mikrotask::reaktion_art(
                if erfuellt {
                    r.bei_erfuellt
                } else {
                    r.bei_fehler
                },
                r.folge,
                wert,
                erfuellt,
                r.art,
            ));
        }
    }

    // ---------------------------------------------------------------
    // Export GVal -> Wert (fuer Host + Tests)
    // ---------------------------------------------------------------

    pub(crate) fn export_wert(&self, v: GVal, _tiefe: u32) -> Wert {
        let mut besucht = HashSet::new();
        EXPORT_BUDGET.with(|b| b.set(MAX_EXPORT_NODES));
        self.export_innen(v, 0, &mut besucht)
    }

    pub(crate) fn export_innen(&self, v: GVal, tiefe: u32, besucht: &mut HashSet<u32>) -> Wert {
        use crate::wert::zahl_text;
        if tiefe > 32 {
            return Wert::Text("[tiefe]".to_string());
        }
        // Release 15.10.2026 (Agent C): one export visits at most
        // `MAX_EXPORT_NODES` values. The path check below only stops cycles;
        // a graph with shared parts (Polymer elements: `__dataHost`, `root`,
        // ...) was walked once per path. MEASURED before (youtube.com, async,
        // cdb): `export_innen` recursing under `host_argument`, 250 MB/s up
        // to 20 GB.
        if EXPORT_BUDGET.with(|b| {
            let n = b.get();
            b.set(n.saturating_sub(1));
            n == 0
        }) {
            return Wert::Text("[gross]".to_string());
        }
        // A host object (an element) inside an array or an object goes over
        // as its handle, as at the top level (`gval_als_wert`), not with all
        // its enumerable expandos.
        if let GVal::Ref(r) = v
            && matches!(self.heap.holen(r), HObj::Gegenstand { .. })
            && self.host_id_von_gval(v).is_some()
            && let Some(w) = self.gval_als_wert(v)
        {
            return w;
        }
        match v {
            GVal::Undefiniert => Wert::Undefiniert,
            GVal::Null => Wert::Null,
            GVal::Bool(b) => Wert::Wahr(b),
            GVal::Zahl(n) => Wert::Zahl(n),
            // J24: siehe `eigenschaften.rs` — `crate::Wert` hat keine
            // BigInt-Variante (oeffentliches API von `narsil`), also der
            // Inspektions-Text.
            GVal::Ganz(g) => Wert::Text(format!("{g}n")),
            GVal::Ref(r) => {
                if !besucht.insert(r.0) {
                    return Wert::Text("[zyklus]".to_string());
                }
                let w = match self.heap.holen(r) {
                    HObj::Zeichenkette(s) => Wert::Text(s.clone()),
                    HObj::Generator { .. } => Wert::Text("[Generator]".to_string()),
                    HObj::Proxy { .. } => Wert::Text("[object Proxy]".to_string()),
                    HObj::Funktion {
                        name, ist_native, ..
                    } => {
                        let _ = ist_native;
                        Wert::Text(format!("[function {name}]"))
                    }
                    HObj::Nativ { name, .. } => Wert::Text(format!("[function {name}]")),
                    // W16 (JS, 04.10.2026): ein Byte-Puffer gibt seine Laenge
                    // aus — das ist die Form, die `String(new ArrayBuffer(4))`
                    // in node 22 liefert, und damit auch der Text, den ein
                    // Skript in einer Konsole sieht.
                    HObj::BytePuffer { daten, .. } => {
                        Wert::Text(format!("[object ArrayBuffer({})]", daten.len()))
                    }
                    // ## W55: `String(new Uint8Array(4))` liefert in node 22
                    // `0, 0, 0, 0` — **nicht** den Objekttext. Das ist der
                    // billigste Beweis, dass die Host-Ausgabe die **Elemente**
                    // sieht und nicht die Bytes; `String(ta)` ist in jedem
                    // Skript der erste Aufruf nach dem Konstruktor.
                    // GEMESSEN (node 22, v22.23.3):
                    // ```text
                    // String(new Uint8Array(4))              -> '0, 0, 0, 0'
                    // String(new Uint8Array([1,2,3]))         -> '1, 2, 3'
                    // String(new Float64Array([1.5]))         -> '1.5'
                    // ```
                    HObj::TypedArray {
                        art,
                        puffer,
                        versatz,
                        laenge,
                        ..
                    } => {
                        let b = art.bytes();
                        // **Die Bytes KOPIEREN, nicht leihen:** `self` ist
                        // hinter der `&self`-Leihgabe der Schleife nicht mehr
                        // verfuegbar, und ein `&[u8]` aus dem Heap wuerde den
                        // Borrow des `match` halten. GEMESSEN als E0308/E0425.
                        let bytes: Vec<u8> = match self.heap.holen(*puffer) {
                            HObj::BytePuffer { daten, .. } => daten.clone(),
                            _ => Vec::new(),
                        };
                        let teil = |i: usize| -> GVal {
                            art.lesen(&bytes, versatz + i * b)
                        };
                        let text: Vec<String> = (0..*laenge)
                            .map(|i| match teil(i) {
                                GVal::Zahl(n) => crate::wert::zahl_text(n),
                                GVal::Ganz(g) => format!("{g}"),
                                andere => {
                                    let _ = andere;
                                    "undefined".to_string()
                                }
                            })
                            .collect();
                        Wert::Text(text.join(", "))
                    }
                    HObj::Fehler {
                        name, nachricht, ..
                    } => Wert::Text(format!("{name}: {nachricht}")),
                    HObj::Datum { millis, .. } => Wert::Text(crate::builtin3::datum_iso(*millis)),
                    HObj::Regex {
                        muster, schalter, ..
                    } => Wert::Text(format!("/{muster}/{schalter}")),
                    HObj::Feld { elemente, .. } => {
                        let mut out = Vec::with_capacity(elemente.len().min(1024));
                        for e in elemente.iter().take(1024) {
                            out.push(self.export_innen(*e, tiefe + 1, besucht));
                        }
                        Wert::Feld(out)
                    }
                    HObj::Menge { eintraege, .. } => {
                        let mut out = Vec::with_capacity(eintraege.len().min(1024));
                        for e in eintraege.iter().take(1024) {
                            out.push(self.export_innen(*e, tiefe + 1, besucht));
                        }
                        Wert::Feld(out)
                    }
                    HObj::Karte { eintraege, .. } => {
                        let mut out = Vec::with_capacity(eintraege.len().min(1024));
                        for (k, val) in eintraege.iter().take(1024) {
                            out.push(self.export_innen(*k, tiefe + 1, besucht));
                            out.push(self.export_innen(*val, tiefe + 1, besucht));
                        }
                        Wert::Feld(out)
                    }
                    HObj::Versprechen { zustand, .. } => match zustand {
                        crate::heap::VersprechenZustand::Erfuellt(w) => Wert::Text(format!(
                            "[Promise: {}]",
                            self.export_innen(*w, tiefe + 1, besucht).als_text()
                        )),
                        crate::heap::VersprechenZustand::Abgelehnt(w) => Wert::Text(format!(
                            "[Promise abgelehnt: {}]",
                            self.export_innen(*w, tiefe + 1, besucht).als_text()
                        )),
                        crate::heap::VersprechenZustand::Offen => {
                            Wert::Text("[Promise offen]".to_string())
                        }
                    },
                    HObj::Verpackt {
                        primitiv,
                        eigenschaften,
                        ..
                    } => {
                        // Wie Primitiv exportieren, wenn keine eigenen Props.
                        if eigenschaften.is_empty() {
                            self.export_innen(*primitiv, tiefe + 1, besucht)
                        } else {
                            Wert::Text(format!(
                                "[Box {}]",
                                self.export_innen(*primitiv, tiefe + 1, besucht).als_text()
                            ))
                        }
                    }
                    HObj::Gegenstand { eigenschaften, .. } => {
                        let mut keys: Vec<&String> = eigenschaften
                            .iter()
                            .filter(|(_, e)| e.aufzählbar)
                            .map(|(k, _)| k)
                            .collect();
                        keys.sort_by(|a, b| schluessel_ordnung(a, b));
                        let mut out = Vec::new();
                        for k in keys.iter().take(1024) {
                            let e = &eigenschaften[*k];
                            out.push(((*k).clone(), self.export_innen(e.wert, tiefe + 1, besucht)));
                        }
                        Wert::Gegenstand(out)
                    }
                    HObj::Umgebung(_) => Wert::Text("[scope]".to_string()),
                };
                besucht.remove(&r.0);
                let _ = zahl_text;
                w
            }
        }
    }
}

/// Release 15.10.2026: the most values one export visits (`export_innen`).
const MAX_EXPORT_NODES: u32 = 200_000;

thread_local! {
    /// The values the running export may still visit.
    static EXPORT_BUDGET: std::cell::Cell<u32> = const { std::cell::Cell::new(MAX_EXPORT_NODES) };
}

/// Schluessel-Ordnung fuer deterministischen Export: ganzzahlige zuerst
/// aufsteigend, dann Rest lexikalisch (JS-Naeherung, dokumentiert).
fn schluessel_ordnung(a: &str, b: &str) -> std::cmp::Ordering {
    let ai = crate::ops::index_aus_schluessel(a);
    let bi = crate::ops::index_aus_schluessel(b);
    match (ai, bi) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.cmp(b),
    }
}
