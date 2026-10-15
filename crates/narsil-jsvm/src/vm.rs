// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Die VM (WELLE6, V2–V4): Rahmen, Aufrufe, Ausnahmen, Generatoren, `await`.
//!
//! # DER UNTERSCHIED ZUM BAUM-INTERPRETER
//!
//! Der Kontrollfluss ist ein `pc` in einem `Vec<Op>`, der Aufrufstapel ein
//! `Vec<Rahmen>`. Ein JS-Aufruf kostet **keinen Rust-Stapel** (V1): die
//! Schleife laeuft weiter, nur der Index in `self.rahmen` aendert sich.
//!
//! GEMESSEN in `narsil-js`: rund 26.800 Byte Rust-Stapel je Aufruf — Tiefe
//! 2000 brauchte dort einen 256-MiB-Thread und war trotzdem `await`-los. Hier
//! ist ein Rahmen ein `Vec`-Eintrag (etwa 200 Byte).
//!
//! # ANHALTEN UND FORTSETZEN (V3)
//!
//! `Erwartung` und `Ertrag` setzen `pc` **auf sich selbst zurueck** und
//! tragen den Wert in `Rahmen::aufnahme`. Wird der Rahmen fortgesetzt (als
//! Mikrotask bzw. `gen.next(v)`), laeuft derselbe Befehl noch einmal —
//! diesmal mit dem Wert. Das ist die kleinste Form von Fortsetzung, die ohne
//! CPS auskommt, und sie ist in einem Test messbar (siehe
//! `tests/await_reihenfolge.rs`).

/// BEGRUENDUNG fuer `dead_code` (gemessen am 30.09.2026): Steuerflaeche der VM: `Kontrolle`-Varianten und Hilfer, die der
/// naechste Ausbau (Generator-Fortsetzen ueber Klassenmethoden) braucht.
#[allow(dead_code)]
use std::sync::Arc;

/// M4/M6: wie viele `Function`-Konstruktor-Aufrufe ein Skript machen darf.
/// GEMESSEN/ENTSCHIEDEN: `4096` reicht fuer jedes Skript, das nicht
/// absichtlich Speicher frisst, und begrenzt die Funktions-Tabelle.
const MAX_DYNAMISCHE_FUNKTIONEN: usize = 4096;

use crate::ast::BinaerOp;
use crate::fehler::JsFehler;
use crate::heap::{BindungsArt, GVal, HObj, HeapRef, KEIN_REF};
use crate::interp::{Engine, Stoerung};
use crate::op::{Bytecode, Funktion, IcEintrag, KEIN_TEMP, Op};

/// Offene Fertig-Aktion (von `finally` ausgefuehrt).
#[derive(Debug, Clone)]
pub(crate) enum Aktion {
    /// Spring an diese Stelle.
    Springen(u32),
    /// Rahmen beenden mit diesem Wert.
    Rueckgabe(GVal),
    /// Ausnahme mit diesem Wert weiterwerfen.
    Wurf(GVal),
}

/// Was beim Anhalten uebergeben wird.
#[derive(Debug, Clone)]
pub(crate) enum Aufnahme {
    /// Wert wurde geliefert (`next(v)`, erfuelltes `await`).
    Wert(GVal),
    /// Ausnahme wurde geworfen (`throw(e)`, abgelehntes `await`).
    Wurf(GVal),
}

/// Release 09.10.2026: alle Heap-Werte eines Rahmens als GC-Wurzeln
/// (fuer ausgelagerte Rahmen, siehe `Engine::ausgelagert`).
pub(crate) fn rahmen_wurzeln(r: &Rahmen, wurzeln: &mut Vec<GVal>) {
    wurzeln.push(GVal::Ref(r.umg));
    // Release 15.10.2026: the generator object of the frame (see `sammeln`).
    if let Some(g) = r.generator {
        wurzeln.push(GVal::Ref(g));
    }
    if let Some(p) = r.param_umg {
        wurzeln.push(GVal::Ref(p));
    }
    if let Some(p) = r.prolog_rumpf {
        wurzeln.push(GVal::Ref(p));
    }
    if let Some(h) = r.heim {
        wurzeln.push(GVal::Ref(h));
    }
    if let Some(c) = r.eltern_ctor {
        wurzeln.push(GVal::Ref(c));
    }
    wurzeln.push(r.this);
    wurzeln.push(r.neu);
    wurzeln.extend(r.temps.iter().copied());
    wurzeln.extend(r.args.iter().copied());
    wurzeln.extend(r.stapel.iter().copied());
    for (werte, _) in &r.iter_lauf {
        wurzeln.extend(werte.iter().copied());
    }
    match &r.aufnahme {
        Some(Aufnahme::Wert(w)) | Some(Aufnahme::Wurf(w)) => wurzeln.push(*w),
        _ => {}
    }
    for a in &r.fertig {
        if let Aktion::Rueckgabe(v) | Aktion::Wurf(v) = a {
            wurzeln.push(*v);
        }
    }
}

/// Ein Ausfuehrungsrahmen — Daten, kein Rust-Stapel.
#[derive(Debug, Clone)]
pub(crate) struct Rahmen {
    /// Index in `Programm::funktionen`.
    pub fun: usize,
    /// Der Bytecode (geteilt; `Arc`-Klon ist billig).
    pub code: Arc<Vec<Op>>,
    /// Umgebung (liegt auf dem Heap und damit als GC-Wurzel da).
    pub umg: HeapRef,
    /// J48: die Umgebung der **Parameterliste** — nur gesetzt, wenn die
    /// Liste **nicht einfach** ist (ES2023 10.2.11 Schritt 27).
    ///
    /// GEMESSEN (node 22), der Anlass:
    ///   `var x = 'outside';
    ///    var pp, pb;
    ///    function f(a = (pp = function(){ return x; })) {
    ///      var x = 'inside'; pb = function(){ return x; };
    ///    }
    ///    f(); pp()` -> `outside`  (nicht `inside`!)
    ///
    /// Der Ausdruck in der Parameterliste darf die Deklarationen des Rumpfs
    /// **nicht** sehen. `umg` ist der Rumpf, `param_umg` haengt darunter:
    /// `umg -> param_umg -> scope`.
    pub param_umg: Option<HeapRef>,
    /// Release 10.10.2026: die RUMPF-Umgebung, solange der Rahmen im
    /// Parameterprolog steht (`Op::ParamProlog(true)` .. `(false)`). In
    /// dieser Zeit IST `umg` die Parameterumgebung — jeder Befehl (Lesen,
    /// Schreiben, Closures) loest dann dort auf, wie ES2023 10.2.11 Schritt
    /// 27-28 es will. GC-Wurzel (die Rumpfumgebung haengt UNTER `param_umg`
    /// und waere von dort aus nicht erreichbar).
    pub prolog_rumpf: Option<HeapRef>,
    /// J3b: eigene Nummer des Rahmens. Der Bytecode-Cache der
    /// Schleife gilt, solange die Nummer gleich bleibt — zwei Aufrufe
    /// hintereinander haben dieselbe Tiefe, aber verschiedene Nummern.
    pub id: u64,
    /// J3: Laenge des Wurzelstapels VOR diesem Rahmen.
    ///
    /// GEMESSEN, warum eine Marke und nicht ein einzelnes
    /// `umgebung_entfernen(umg)`: der Rahmen nimmt beim Start eine
    /// Umgebung als Wurzel, und beim Ende muss ALLES weg, was seitdem
    /// dazugekommen ist — auch die Umgebungen von Bloecken, die ein
    /// `return` uebersprungen hat (deren `BlockAb` nie laeuft). Ohne das
    /// blieb pro Aufruf eine Wurzel stehen: `fib(27)` haette 400.000
    /// Wurzeln, der GC koennte NICHTS freigeben, der Heap waechst auf
    /// 169 MiB und die Kosten je Schritt verdreifachen sich (156 auf
    /// 2688 ns). Jetzt: `fib(27)` in 1,4 s bei 3 MiB Heap.
    pub wurzel_marke: usize,
    /// Basisindex im gemeinsamen Operandenstapel.
    pub basis: usize,
    /// `this` (Pfeilfunktionen: unbenutzt, `This` laeuft nach aussen).
    pub this: GVal,
    /// J14: ist diese Funktion strikt? (Direktive im Rumpf oder ein
    /// striktes Programm). Bestimmt `this` beim Aufruf ohne Empfaenger
    /// und die globalen Erzeugungen bei Zuweisung.
    pub strikt: bool,
    /// Heimat-Objekt fuer `super` (der Prototyp der Klasse).
    pub heim: Option<HeapRef>,
    /// Elternkonstruktor fuer `super()`.
    pub eltern_ctor: Option<HeapRef>,
    /// Aktive Handler (Indizes in `Funktion::handler`).
    pub handler: Vec<usize>,
    /// Offene Fertig-Aktionen (`finally`).
    pub fertig: Vec<Aktion>,
    /// Temp-Plaetze.
    pub temps: Vec<GVal>,
    /// `arguments`.
    pub args: Vec<GVal>,
    /// `new.target`.
    pub neu: GVal,
    /// Konstruktor?
    pub ist_konstruktor: bool,
    /// Wurde `this` durch `super()` initialisiert?
    pub this_gesetzt: bool,
    /// Beim Anhalten uebergebener Wert.
    pub aufnahme: Option<Aufnahme>,
    /// `yield*`: Inner-Werte (Feld, Index) fuer die naechste Fortsetzung.
    pub ertrag_stern: Option<(GVal, usize)>,
    /// ## W17 (JS, 04.10.2026) — WELCHEN Generator-/async-Rahmen dieser
    /// Rahmen gehoert.
    ///
    /// **Vorher stand hier nichts**, und `Engine::ertrag` (der `yield`-Befehl)
    /// hat den Generator deshalb ueber `self.wurzeln.last()` gesucht. Das ist
    /// **falsch**, und zwar messbar:
    ///
    /// ```js
    /// function* g(){ yield 1 }   g().next().value
    /// ```
    /// lieferte `undefined` statt `1` — bei **jedem** Generator. Grund:
    /// `n_generator_schritt` legt den Generator in `wurzeln`, damit er
    /// waehrend des Rumpflaufs lebt; **der Rumpf alloziert aber** (auch
    /// `yield 1` erzeugt die Auswertung von `1` einen Stackwert, und jeder
    /// `String`/Aufruf alloziert), und `wurzeln.last()` ist danach **ein
    /// anderer** Slot. Das `if let HObj::Generator` traf nicht zu, `ertrag`
    /// blieb `None`, und `next()` lieferte `{done:false, value:undefined}`.
    /// `done` war richtig — der Befehl laeuft ja — nur der **Wert** kam nicht
    /// heraus.
    ///
    /// GEMESSEN (node 22 als Soll): `g().next().value` -> `1`.
    ///
    /// Der Rahmen weiss jetzt selbst, wem er gehoert. Das ist auch die
    /// Grundlage fuer `async`: eine async-Funktion ist derselbe Anhalte-
    /// Mechanismus, und ihr Versprechen haengt genauso am Rahmen.
    pub generator: Option<HeapRef>,
    /// ## W18 (JS, 04.10.2026) — ist dieser Rahmen schon **fortgesetzt**
    /// worden?
    ///
    /// **Vorher stand an dieser Stelle ein Unsinn**, und der hat *jeden*
    /// Generator kaputt gemacht. `Op::Ertrag` (der `yield`-Befehl) hat
    /// entschieden:
    /// ```text
    /// let resuming = self.rahmen.last().map(|r| r.aufnahme.is_some())
    /// ```
    /// und bei einer Fortsetzung den `yield`-Wert **nicht** vom Stapel
    /// gelesen, sondern `undefined` genommen — die Begruendung im Kommentar
    /// ist richtig: „bei einer FORTSETZUNG liegt auf dem Stapel der Wert des
    /// AUFERU FERS".
    ///
    /// **Aber `aufnahme` ist kein Fortsetzungs-Merkmal.** `n_generator_schritt`
    /// setzt es bei **jedem** Schritt, auch bei `next()` ganz ohne Argument
    /// (`Aufnahme::Wert(undefined)`). Also war `resuming` beim **ersten**
    /// `next()` bereits `true`, der `yield`-Operand wurde nie gelesen, und
    /// `ertrag` bekam `undefined`.
    ///
    /// GEMESSEN, vorher, bei **jedem** Generator (node 22 als Soll):
    ///     `function* g(){ yield 1 }   g().next().value`  ->  `undefined`
    ///     `function* g(){ yield 1 }   g().next().done`   ->  `false`  (richtig)
    ///     `function* g(){ yield 1; yield 2 }` Schleife  ->  `undefined`
    ///     `function* h(){ var x = yield 1; return x*2 } i.next(); i.next(21)`
    ///                                                          ->  `undefined`
    /// `done` stimmte immer, weil der Befehl laeuft — nur der **Wert** kam
    /// nie heraus. Das ist die Signatur eines Befehls, der laeuft und seinen
    /// Wert nicht ablegt.
    ///
    /// Das echte Merkmal ist der **Zustand des Generators** vor dem Schritt:
    /// `Start` heisst „nie gelaufen", erst danach ist es eine Fortsetzung.
    pub fortgesetzt: bool,
    /// Wurde dieser Rahmen ueber `super()` betreten? Dann wird sein
    /// Ergebnis NICHT auf den Stapel gelegt (der Kinderrumpf erwartet es
    /// nicht, und `super()` liefert in JS nichts).
    pub super_kind: bool,
    /// J28: die zuletzt gesetzte **Zeile** (von `Op::Zeile`).
    ///
    /// Die Bedeutung steht in `debuglogik.rs::zeile_gelaufen`: „steht der
    /// naechste Befehl auf `Op::Zeile`, ist das die Zeile der kommenden
    /// Anweisung, sonst die zuletzt gesetzte Zeile des Rahmens“.
    pub zeile: u32,
    /// J28: Schnappschuss des gemeinsamen Operandenstapels **beim
    /// Anhalten**. Gelesen in `debuglogik.rs` als `r.stapel` **ab**
    /// `r.basis`; ohne Haken wird darum keine Kopie gemacht.
    pub stapel: Vec<GVal>,
    /// Programmzaehler.
    pub pc: usize,
    /// Release 09.10.2026: die laufenden `for-in`/`for-of`-Listen DIESES
    /// Rahmens (Temp-Index -> Werte, Position). Vorher lagen sie im Motor,
    /// nach Temp-Index fuer ALLE Rahmen gemeinsam und ohne GC-Wurzel:
    /// rekursive Schleifen ueberschrieben einander, und die Schluessel einer
    /// `for-in`-Schleife wurden mitten im Lauf eingesammelt (GEMESSEN:
    /// `for (var k in o) { var x = {}; k.toUpperCase() }` -> TypeError).
    pub iter_lauf: Vec<(Vec<GVal>, usize)>,
}

/// Zustand eines Generators / einer async-Funktion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GenZustand {
    Start,
    Angehalten,
    Fertig,
}

/// Ergebnis eines Schleifendurchlaufs.
pub(crate) enum LaufErgebnis {
    /// Rahmen bis `ende` beendet, Wert da.
    Fertig(GVal),
    /// Ausnahme bis `ende` durchgekommen.
    Wurf(GVal),
    /// Ein Rahmen hat angehalten (await/yield).
    Angehalten,
    /// Engine-Limit (nicht fangbar).
    Fehler(JsFehler),
}

/// Was ein Befehl bewirkt hat.
pub(crate) enum Kontrolle {
    Weiter,
    /// Der oberste Rahmen ist fertig (der Aufrufer poppt ihn).
    Fertig,
    /// Der Lauf haelt an.
    Angehalten,
    /// Ausnahme: der Aufrufer sucht einen Handler.
    Wurf(GVal),
    /// Engine-Limit.
    Fehler(JsFehler),
}

/// Ergebnis einer `for`-Iteration.
pub(crate) enum IterErgebnis {
    Wert(GVal),
    Ende,
    Angehalten,
    Fehler(JsFehler),
    /// Release 10.10.2026: ein JS-Wurf aus `next()` (fauler Weg).
    Wurf(GVal),
}

/// Release 10.10.2026: die Marke eines FAULEN Laufs in `Rahmen::iter_lauf` —
/// die Werteliste traegt dann `[iterator, next]` (die GC sieht beide wie jede
/// andere Werteliste).
pub(crate) const ITER_FAUL: usize = usize::MAX;

impl Engine {
    // =================================================================
    // Programm starten
    // =================================================================

    /// Kompiliert und startet ein Programm (V1 + V2).
    pub(crate) fn programm_ausfuehren(&mut self, quelle: &str) -> Result<GVal, JsFehler> {
        self.letzte_fehlerstelle = None;
        let tokens = crate::lex::lexen(
            quelle,
            self.grenzen.max_quelltext_bytes,
            self.grenzen.max_token,
        )?;
        let ast = crate::parse::parsen_mit_quelle(tokens, self.grenzen.max_parser_tiefe, &quelle)?;
        // OPUS_1, angewandt auf **Skripte** (Wikipedia, 03.10. 21:15): ein
        // weiteres `<script>` wird an das laufende Programm **angehaengt** —
        // derselbe Weg wie `eval` (siehe `uebersetzen`).
        //
        // GEMESSEN vorher: `c.uebersetze(&ast)` + `self.programm = Some(prog)`
        // **ersetzte** das Programm. Text-, Funktions- und Klassenindizes
        // fingen damit wieder bei 0 an, waehrend die Closures eines
        // frueheren Skripts ihre alten Indizes behalten — beim Aufruf
        // `Laufzeit("Funktionsindex falsch (intern)")`, und auf wikipedia.org
        // `ReferenceError: doWhenReady ist nicht definiert`. `starten`
        // loescht danach nur Rahmen und Stapel — der globale Bereich bleibt
        // unberuehrt (ES2023 16.1, `tests/w9_globaler_bereich.rs`).
        let prog = self.uebersetzen(&ast)?;
        self.starten(prog)
    }

    /// Release 09.10.2026: uebersetzen **als Anhang** an das laufende
    /// Programm — der Compiler bekommt nur die Laengen der Pools und leiht
    /// sich den Textindex (siehe `Compiler::basis_setzen`, dort die Messung).
    pub(crate) fn uebersetzen(&mut self, ast: &crate::ast::Programm) -> Result<Bytecode, JsFehler> {
        let mut c = crate::compile::Compiler::neu();
        if let Some(p) = self.programm.as_ref() {
            let (t, f, k) = (p.texte.len(), p.funktionen.len(), p.klassen.len());
            c.basis_setzen(t, f, k, std::mem::take(&mut self.texte_index));
        }
        let r = c.uebersetze(ast);
        self.texte_index = c.texte_index_zurueck(r.is_ok());
        r
    }

    /// Release 09.10.2026: das Ergebnis von `uebersetzen` an das Programm
    /// haengen (Texte, Funktionen, Klassen); liefert den Index des neuen
    /// Hauptlaufs. Der Textpool wird dabei nicht kopiert: `texte_arc` gibt
    /// seinen Anteil vorher ab, damit `Arc::make_mut` an Ort und Stelle
    /// anhaengt.
    pub(crate) fn programm_anhaengen(&mut self, prog: Bytecode) -> Result<usize, JsFehler> {
        let bytes = prog.bytes();
        if self.programm_bytes.saturating_add(bytes) > self.grenzen.max_heap_bytes {
            // Die neuen Texte kommen nicht ins Programm — auch nicht in den Index.
            let basis = self.programm.as_ref().map(|p| p.texte.len()).unwrap_or(0) as u32;
            for t in prog.texte.iter() {
                if self.texte_index.get(t).is_some_and(|i| *i >= basis) {
                    self.texte_index.remove(t);
                }
            }
            return Err(JsFehler::Speicher(
                "Programm groesser als das Heap-Limit (M4)".to_string(),
            ));
        }
        self.programm_bytes += bytes;
        let haupt = prog.haupt;
        self.texte_arc = std::sync::Arc::new(Vec::new());
        match self.programm.as_mut() {
            None => self.programm = Some(prog),
            Some(p) => {
                let Bytecode { funktionen, texte, klassen, .. } = prog;
                let neu = std::sync::Arc::try_unwrap(texte).unwrap_or_else(|a| (*a).clone());
                std::sync::Arc::make_mut(&mut p.texte).extend(neu);
                p.funktionen.extend(funktionen);
                p.klassen.extend(klassen);
            }
        }
        let p = self.programm.as_ref().expect("gerade gesetzt");
        self.texte_arc = p.texte.clone();
        if self.inline_cache.len() < p.texte.len() {
            self.inline_cache.resize(p.texte.len(), IcEintrag::default());
        }
        Ok(haupt)
    }

    /// Ein uebersetztes Programm starten.
    pub(crate) fn starten(&mut self, prog: Bytecode) -> Result<GVal, JsFehler> {
        let strikt = prog.strikt;
        let haupt = self.programm_anhaengen(prog)?;
        let vorher_strikt = self.strikt;
        self.strikt = self.strikt || strikt;
        self.rahmen.clear();
        self.stapel.clear();
        // ## W10 (JS, 04.10.2026) — `var`-HEBUNG AUF SKRIPT-EBENE
        //
        // ES2023 8.1.1 `GlobalDeclarationInstantiation` Schritt 7: **vor**
        // der ersten Anweisung wird fuer jeden `var`-Namen, der sich noch
        // nicht aufloesen laesst, eine globale `var`-Bindung **mit Wert
        // `undefined`** angelegt. `String(x); var x;` ist damit kein
        // `ReferenceError`, sondern `undefined`.
        //
        // GEMESSEN, warum es nur die **Skript**-Ebene war: die
        // Funktionsrahmen machten den Vorlauf seit langem
        // (`rahmen_starten_fn` und `rahmen_starten_mit`, vm.rs: ~2927 und
        // ~4646 — `for n in &f.var_namen { deklariere(umg, n, Var) }`), der
        // **Hauptlauf** hier aber nicht, obwohl `haupt` genauso ein
        // `Funktion` mit `var_namen` ist. Messung:
        //   `function f(){ String(q); var q; return 'ok' } f()`  -> 'ok'
        //   `String(q2); var q2;`                                -> ReferenceError
        // Der Befund steht auch in `tests/js2_var_hoisting.rs` (JS2/QUIC)
        // — dessen vier Tests waren **rot**; die Diagnose dort war richtig,
        // der Einbau fehlte.
        //
        // Zwei Regeln, beide aus der Spec:
        //   * **nur** anlegen, was noch **nicht** da ist — ein globales `x`
        //     aus einem früheren `<script>` darf nicht auf `undefined`
        //     gesetzt werden (Schritt 7: "if HasRestrictedGlobalProperty is
        //     false"). `var x = 1` gibt `1`, `var x` allein gibt `undefined`
        //     (Schritt 7.c vs. die Initialisierer in Schritt 5.e.iii).
        //   * in den **globalen** Bereich, nicht in den Objektbereich —
        //     `CreateGlobalVarBinding` (8.1.7 d.ii.2) ist eine **andere**
        //     Regel und bleibt unberuehrt.
        {
            let var_namen = self
                .programm
                .as_ref()
                .and_then(|p| p.funktionen.get(haupt))
                .map(|f| f.var_namen.clone())
                .unwrap_or_default();
            for n in var_namen.iter() {
                if !self.umgebung_hat(self.global_umgebung, n) {
                    let _ = self.deklariere(self.global_umgebung, n, BindungsArt::Var);
                }
            }
        }
        let temps = self.programm.as_ref().unwrap().funktionen[haupt].temps as usize;
        let rahmen_id = self.rahmen_id_holen();
        self.rahmen.push(Rahmen {
            // BILD_29 (Befreiung fuer QUIC): `Rahmen` hat das Feld `fortgesetzt`
            // bekommen — „der Generator lief schon vor diesem Schritt". Beim
            // **Anlegen** einer Frame hat er noch nicht gelaufen (`Start`), also
            // `false`. **Diese Stellen sind die von `bau.sh narsil check`
            // gemeldeten 6x E0063** — ohne sie ist `narsil-jsvm` nicht baubar und
            // damit **kein** Test in `narsil` pruefbar. Der Wert ist an der
            // Semantik abgelesen, nicht geraten.
            fortgesetzt: false,
            wurzel_marke: self.wurzel_merke(),
            id: rahmen_id,
            fun: haupt,
            // J48: der Hauptlauf hat **keine** Parameterumgebung.
            param_umg: None,
            prolog_rumpf: None,
            // J28: die Startzeile der Funktion (siehe das Feld `zeile`).
            zeile: self.funktions_zeile(haupt),
            // J3: `Arc`-Klon statt Kopie des Bytecodes.
            code: self.programm.as_ref().unwrap().funktionen[haupt]
                .code
                .clone(),
            umg: self.global_umgebung,
            basis: 0,
            // Release 08.10.2026: am obersten Level eines SKRIPTS ist `this`
            // IMMER das globale Objekt — auch im Strict-Modus (ECMA-262
            // §9.1.1.4.11 GetThisBinding des globalen Umfelds). Das
            // `undefined` von node kam aus dessen Modul-Huelle, nicht aus
            // der Sprache. GEMESSEN: YouTubes Hauptskript beginnt mit
            // `"use strict";this.default_kevlar_base=…` und brach mit
            // „kein Setzen auf Primitiv" ab.
            this: GVal::Ref(self.global_objekt),
            // J14: der Hauptlauf erbt die Programmlage (Direktive oben).
            strikt: self.strikt,
            heim: None,
            eltern_ctor: None,
            handler: Vec::new(),
            fertig: Vec::new(),
            temps: vec![GVal::Undefiniert; temps],
            args: Vec::new(),
            neu: GVal::Undefiniert,
            ist_konstruktor: false,
            this_gesetzt: false,
            aufnahme: None,
            ertrag_stern: None,
            // W17: der Hauptlauf ist kein Generator- und kein async-Rahmen.
            generator: None,
            super_kind: false,
            iter_lauf: Vec::new(),
            stapel: Vec::new(),
            pc: 0,
        });
        let out = match self.schleife(0) {
            LaufErgebnis::Fertig(v) => Ok(v),
            LaufErgebnis::Wurf(w) => {
                self.letzter_wurf = Some(w);
                let text = self.wurf_text(w);
                self.mikrotasks.clear();
                Err(JsFehler::Laufzeit(format!("Unbehandelt: {text}")))
            }
            LaufErgebnis::Angehalten => {
                // J28: **der Debug-Haken** ist der einzige Grund, warum der
                // Hauptlauf anhaelt — und dann ist das **kein Fehler**:
                // `ausfuehren_mit_haken` gibt `HakenEnde::Steht` zurueck
                // und laesst die Rahmen stehen.
                //
                // GEMESSEN, warum man hier nicht einfach `Ok(undefined)`
                // schreibt: der Aufrufer von `ausfuehren` will einen
                // **Wert**, und ein stilles `undefined` waere „die Seite
                // ergab undefined" — eine falsche Antwort auf eine Frage,
                // die nie gestellt wurde.
                if self.haken_haelt() {
                    self.haken_ohne_halt();
                    return Ok(GVal::Zahl(-1.0));
                }
                // Der Hauptlauf haelt nicht an (er ist keine async-Funktion).
                self.mikrotasks.clear();
                Err(JsFehler::Laufzeit(
                    "Hauptlauf hat angehalten (intern)".to_string(),
                ))
            }
            LaufErgebnis::Fehler(e) => Err(e),
        };
        self.strikt = vorher_strikt;
        out
    }

    /// J8: nur die Felder, die der Aufrufpfad braucht (ohne `namen`/
    /// `namen_hash` zu kopieren) — siehe `op::AufrufKopf`.
    pub(crate) fn aufruf_kopf(&self, idx: usize) -> Result<crate::op::AufrufKopf, JsFehler> {
        let p = self
            .programm
            .as_ref()
            .ok_or_else(|| JsFehler::Laufzeit("kein Programm geladen".to_string()))?;
        let Some(f) = p.funktionen.get(idx) else {
            // Release 09.10.2026 (Diagnose): woher kommt ein falscher Index?
            if diag_an() {
                eprintln!(
                    "NARSIL_JS_DIAG aufruf_kopf({idx}) ungueltig:
{}",
                    std::backtrace::Backtrace::force_capture()
                );
            }
            return Err(JsFehler::Laufzeit("Funktionsindex falsch (intern)".to_string()));
        };
        Ok(crate::op::AufrufKopf {
            code: f.code.clone(),
            temps: f.temps,
            braucht_this: f.braucht_this,
            pfeil: f.pfeil,
            asynchron: f.asynchron,
            generator: f.generator,
            var_namen: f.var_namen.clone(),
            var_hashes: f.var_hashes.clone(),
            parameter: f.parameter,
            // J48: war die Parameterliste **einfach**? Nur bei einer nicht
            // einfachen Liste bekommt der Aufruf eine zweite Umgebung fuer
            // die Parameter (ES2023 10.2.11 Schritt 27).
            param_einfach: f.param_einfach,
            selbst_name: f.selbst_name.clone(),
            // J14: Direktive „use strict" der Funktion (siehe `Funktion`).
            strikt: f.strikt,
            // J19: der Quelltext (billiger `Arc`-Klon).
            quelle: f.quelle.clone(),
        })
    }

    /// J8: braucht der aktuelle Rahmen ein `arguments`-Objekt?
    fn arguments_noetig(&self) -> bool {
        // Release 09.10.2026: ein direktes `eval` meldet die Funktion selbst
        // (`braucht_arguments`, gesetzt im Compiler) — nicht mehr das Wort
        // `eval` irgendwo im Skript.
        let fun = self.rahmen.last().map(|r| r.fun).unwrap_or(0);
        self.programm
            .as_ref()
            .and_then(|p| p.funktionen.get(fun))
            .map(|f| f.braucht_arguments)
            .unwrap_or(true)
    }

    /// Funktionsdefinition aus dem geladenen Programm.
    pub(crate) fn funktion_von(&self, idx: usize) -> Result<Funktion, JsFehler> {
        match &self.programm {
            Some(p) => p
                .funktionen
                .get(idx)
                .cloned()
                .ok_or_else(|| JsFehler::Laufzeit("Funktionsindex falsch (intern)".to_string())),
            None => Err(JsFehler::Laufzeit("kein Programm geladen".to_string())),
        }
    }

    fn text_konstante(&self, i: u32) -> String {
        match &self.programm {
            Some(p) => p.texte.get(i as usize).cloned().unwrap_or_default(),
            None => String::new(),
        }
    }

    /// J-HN: **der Name des Aufrufausdrucks** fuer die Fehlermeldung.
    ///
    /// GEMESSEN/ANLASS (news.ycombinator.com, Statuszeile):
    /// `TypeError: Aufrufziel ist keine Funktion` — **ohne** jeden Hinweis,
    /// welche Funktion gemeint war. Bei einer Seite mit 40 Skripten und
    /// tausend Aufrufen ist das eine Suche ohne Land.
    ///
    /// Der Motor fuehrt die Kette `Op::LeseVar(i)` / `LeseVarName` /
    /// `EigenLese` / `FeldLese` vor jedem `Aufruf*` aus; der zuletzt
    /// davon gelesene Name ist der Name im Aufrufausdruck. Wir lesen ihn
    /// **aus dem Bytecode rueckwaerts** ab dem aktuellen `pc` — das kostet
    /// nichts im normalen Lauf (nur im Fehlerfall) und braucht keinen
    /// zusaetzlichen Zustand im Rahmen.
    fn aufruf_ausdruck(&self) -> Option<String> {
        let rahmen = self.rahmen.last()?;
        let code: &Vec<Op> = &rahmen.code;
        let fun = self.programm.as_ref()?.funktionen.get(rahmen.fun)?;
        // Ab `pc` rueckwaerts: der naechste Lese-Befehl **vor** dem Aufruf
        // ist der Name (die Argumente stehen dazwischen, sie lesen auch —
        // deshalb nehmen wir den **letzten** Lesebefehl, der vor dem
        // Aufruf steht und direkt auf ihn folgt).
        //
        // GEMESSEN (Bytecode-Dump): `({}).nichtda()` lautet
        //   `[NeuesObjekt, Text(0), AufrufMethode(0), Rueckgabe]`
        // Der Methodenname wird **nicht** durch einen Lesebefehl geladen,
        // sondern als `Text`-Konstante **unmittelbar vor** dem Aufruf
        // geschoben. Deshalb wird auch `Op::Text` als Name gelesen — und
        // der **erste** Name-/Textbefehl rueckwaerts ist der gesuchte
        // (nicht der letzte: das waere `NeuesObjekt`).
        let mut i = rahmen.pc.min(code.len());
        while i > 0 {
            i -= 1;
            match &code[i] {
                Op::LeseVar(k) => {
                    let n = fun.namen.get(*k as usize)?;
                    return Some(n.clone());
                }
                // GEMESSEN/GEFUNDEN: `LeseEigenschaft` holt seinen Namen
                // aus `texte` (nicht aus `namen` — das sind zwei Pools,
                // siehe die Auswertung bei `Op::LeseEigenschaft`). Mit dem
                // falschen Pool kam „nichtda" nie heraus.
                Op::LeseEigenschaft(k) => {
                    let n = self.text_konstante(*k);
                    if !n.is_empty() {
                        return Some(n);
                    }
                }
                // Release 09.10.2026: der Aufruf mit festem Namen traegt ihn selbst.
                Op::AufrufMethodeText(_, k) => {
                    let n = self.text_konstante(*k);
                    if !n.is_empty() {
                        return Some(n);
                    }
                }
                // Der Methodenname als **Konstante** (siehe oben).
                Op::Text(k) => {
                    let n = self.text_konstante(*k);
                    if !n.is_empty() {
                        return Some(n);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// J-HN: die Fehlermeldung mit dem Namen, wenn der Motor ihn kennt.
    fn aufrufziel_fehler(&mut self) -> GVal {
        match self.aufruf_ausdruck() {
            // GEMESSEN (node 22): node sagt „x is not a function" — mit
            // dem **Namen**. Genau das fehlte hier.
            Some(n) => {
                self.fehler_wert("TypeError", &format!("{n} is not a function (Aufrufziel)"))
            }
            None => self.fehler_wert("TypeError", "Aufrufziel ist keine Funktion"),
        }
    }

    // =================================================================
    // Die Schleife (V2)
    // =================================================================

    /// Ausfuehren, bis hoechstens `ende` Rahmen da sind.
    ///
    /// WICHTIG: `lauf_ende` wird gesichert und zurueckgesetzt. Sonst
    /// vergaesse ein verschachtelter Lauf (Native-Rueckruf, Klassenaufbau)
    /// die Tiefe des aeusseren Laufs — und der beendet sich dann zu frueh
    /// (gemessen: nach `new T()` endete das Skript beim Feld-Initialisierer).
    pub(crate) fn schleife(&mut self, ende: usize) -> LaufErgebnis {
        let alt = self.lauf_ende;
        self.lauf_ende = ende;
        let r = self.schleife_innen(ende);
        self.lauf_ende = alt;
        r
    }

    /// J28: **derselbe** Lauf, aber mit der Erlaubnis anzuhalten.
    ///
    /// GEMESSEN, warum es keine zweite Schleife gibt: die erste Fassung
    /// hatte den Hakenlauf als Kopie — und die Kopie vergass
    /// `pc = pc + 1` (das macht `schleife_innen`, nicht `schritt`). Der
    /// Lauf lief denselben Befehl 50 Millionen Mal und endete mit
    /// „Zeitlimit" (gemessen). Eine kopierte Schleife ist immer eine
    /// zweite Wahrheit ueber `pc`, `frisch` und den Rahmen-Abbau.
    pub(crate) fn schleife_mit_haken(&mut self, ende: usize) -> LaufErgebnis {
        let alt = self.lauf_ende;
        self.lauf_ende = ende;
        if let Some(h) = self.haken.as_mut() {
            h.haelt = true;
        }
        let r = self.schleife_innen(ende);
        self.lauf_ende = alt;
        if let Some(h) = self.haken.as_mut() {
            h.haelt = false;
        }
        r
    }

    fn schleife_innen(&mut self, ende: usize) -> LaufErgebnis {
        // J3b: der Bytecode des aktuellen Rahmens wird **einmal** geholt
        // und fuer alle Schritte dieses Rahmens behalten.
        //
        // GEMESSEN, warum das noetig war: vorher stand in der Schleife
        // `let (code, pc) = (r.code.clone(), r.pc);` — also ein
        // `Arc`-Klon **je Schritt** (zwei Atomaroperationen), und danach
        // `code[pc].clone()` (eine Kopie des Befehls). Bei
        // `Schleife 10^7` sind das 260 Millionen Klonpaare.
        //
        // Der Cache gilt, solange der **Rahmen** derselbe ist; dafuer
        // bekommt jeder Rahmen eine eigene Nummer (`Rahmen::id`).
        let mut cache: Option<(u64, Arc<Vec<Op>>)> = None;
        loop {
            if self.rahmen.len() <= ende {
                return LaufErgebnis::Angehalten;
            }
            // J28: DER HAKEN — **vor** dem Schritt, und genau deshalb gibt
            // es „Schritt ueber". Ein Schritt, der erst NACH dem Befehl
            // prueft, hat den Befehl des Sprungs schon ausgefuehrt; „Ueber
            // heisst aber, er soll **nicht** ausgefuehrt werden.
            //
            // GEMESSEN, warum hier und nicht in `schritt()`: `schritt()`
            // wird auch von `AufrufSchritt` (Wert einer Funktion
            // berechnen) benutzt — dort ist ein Haken nicht erlaubt, und
            // ein Haken in `schritt()` wuerde ihn in jeden nativen
            // Aufruf hineinziehen.
            if self.haken_aktiv() && self.haken_haelt() && self.haken_halt() {
                return LaufErgebnis::Angehalten;
            }
            if let Err(e) = self.schritt() {
                return LaufErgebnis::Fehler(e);
            }
            let (pc, id) = {
                let r = self.rahmen.last().unwrap();
                (r.pc, r.id)
            };
            if !matches!(&cache, Some((i, _)) if *i == id) {
                cache = Some((id, self.rahmen.last().unwrap().code.clone()));
            }
            let code = &cache.as_ref().expect("Cache gefuellt").1;
            if pc >= code.len() {
                // Sicherheitsnetz: Codeende ohne Rueckgabe.
                let v = self.stapel.last().copied().unwrap_or(GVal::Undefiniert);
                if self.rahmen_ende(v) {
                    return LaufErgebnis::Fertig(v);
                }
                continue;
            }
            self.rahmen.last_mut().unwrap().pc = pc + 1;
            // J21: Befehlswechsel — alles, was der **vorige** Befehl neu
            // belegt hat, liegt jetzt auf dem Stapel, in einer Umgebung,
            // in einem Timer oder in einer Host-Tabelle (allesamt Wurzeln).
            // `frisch` darf deshalb geleert werden; ab hier zaehlt wieder
            // nur, was wirklich erreichbar ist. GEMESSEN/ENTSCHIEDEN: genau
            // hier und nicht im GC — die Begruendung steht am Feld `frisch`.
            self.frisch.clear();
            if pc == 0 && diag_an() {
                let r = self.rahmen.last().unwrap();
                let (id, fun) = (r.id, r.fun);
                let erneut = DIAG_GESEHEN.with(|g| !g.borrow_mut().insert(id as u64));
                if erneut {
                    let name = self.programm.as_ref().and_then(|p| p.funktionen.get(fun)).map(|f| f.name.clone()).unwrap_or_default();
                    eprintln!("NARSIL_JS_DIAG Rahmen {id} ({name}, fun {fun}) springt erneut auf Befehl 0");
                }
            }
            if self.gc_faellig {
                self.gc_faellig = false;
                self.sammeln();
            }
            let op = code[pc];
            let profil_start = if profil_an() { Some(std::time::Instant::now()) } else { None };
            let kontrolle = self.befehl(&op);
            if let Some(t0) = profil_start {
                profil_buchen(&op, t0.elapsed());
                let fun = self.rahmen.last().map(|r| r.fun).unwrap_or(usize::MAX);
                PROFIL_FUN.with(|p| *p.borrow_mut().entry(fun).or_insert(0) += 1);
            }
            match kontrolle {
                Kontrolle::Weiter => {}
                Kontrolle::Fertig => {
                    // Der Rahmen endet; bei `super()` laeuft der Kinderrumpf
                    // einfach weiter (sein `pc` zeigt schon hinter dem Aufruf).
                    let v = self.stapel.last().copied().unwrap_or(GVal::Undefiniert);
                    if self.rahmen_ende(v) {
                        return LaufErgebnis::Fertig(v);
                    }
                }
                Kontrolle::Angehalten => return LaufErgebnis::Angehalten,
                Kontrolle::Wurf(w) => {
                    if let Err(e) = self.werfen(w) {
                        return e;
                    }
                }
                Kontrolle::Fehler(e) => return LaufErgebnis::Fehler(e),
            }
        }
    }

    /// Die Nummer des aktuellen Rahmens erhoehen (J3b) — beim Anlegen
    /// eines Rahmens aufrufen, damit zwei Rahmen derselben Tiefe
    /// verschiedene Nummern haben.
    fn rahmen_id_holen(&mut self) -> u64 {
        let id = self.naechste_rahmen_id;
        self.naechste_rahmen_id = self.naechste_rahmen_id.wrapping_add(1);
        id
    }

    /// Obersten Rahmen beenden. `true`, wenn die Schleife endet.
    /// J28: `rahmen_ende` fuer den Hakenlauf (die freie Funktion in
    /// diesem Modul darf die private Methode nicht rufen).
    pub(crate) fn rahmen_ende_pub(&mut self, v: GVal) -> bool {
        self.rahmen_ende(v)
    }

    /// J28: die Startzeile der Funktion `fun` (0 = unbekannt).
    pub(crate) fn funktions_zeile(&self, fun: usize) -> u32 {
        self.programm
            .as_ref()
            .and_then(|p| p.funktionen.get(fun))
            .map(|f| f.zeile)
            .unwrap_or(0)
    }

    fn rahmen_ende(&mut self, v: GVal) -> bool {
        let (b, super_kind, ist_konstruktor, this) = match self.rahmen.last() {
            Some(r) => (r.basis, r.super_kind, r.ist_konstruktor, r.this),
            None => return true,
        };
        self.stapel.truncate(b);
        // Konstruktor-Rueckgabe (ES2023 13.2.2 [[Construct]]): gibt die
        // Funktion ein Objekt zurueck, wird **das** zurueckgegeben, sonst
        // `this`. Ein `super()`-Rahmen (`super_kind`) erzeugt keinen Wert
        // auf dem Stapel (JS-Regel) — der Kinderrumpf laeuft danach weiter.
        //
        // GEMESSEN (47 test262-Faelle, Private-Getter-Klassen): der
        // Rueckgabewert darf nur **ein einziges Mal** gepusht werden.
        // Die erste Fassung dieser Funktion pushte `v` (fuer den
        // Nicht-`super`-Fall) **und** `rueckgabe` — zwei Werte je
        // Rueckkehr. Bei `new C()` mit Feldern blieb ein Wert zu viel
        // liegen und verschob die Operanden des naechsten Aufrufs
        // (`assert.sameValue(…)` las als Basis `42` statt `assert`).
        let rueckgabe = if ist_konstruktor && !super_kind {
            if self.ist_objekt(v) { v } else { this }
        } else {
            v
        };
        let marke = self.rahmen.last().map(|r| r.wurzel_marke);
        if let Some(r) = self.rahmen.pop() {
            // Release 09.10.2026: Temps und Argumente wiederverwenden.
            self.pool_zurueck(r.temps);
            self.pool_zurueck(r.args);
        }
        // J3: die Wurzeln des Rahmens mitnehmen (Begruendung am Feld
        // `Rahmen::wurzel_marke`).
        if let Some(m) = marke {
            self.wurzel_zurueck(m);
        }
        if !super_kind {
            // Nicht-`super()`-Rahmen: Rueckgabe (ein Wert) auf den
            // Stapel legen.
            self.stapel.push(rueckgabe);
        } else {
            // Release 10.10.2026: `super(...)` ist ein AUSDRUCK mit dem
            // Wert `this` (ES2023 13.3.7.1, Schritt 10: BindThisValue,
            // Rueckgabe `result`). Gibt der Eltern-Konstruktor ein Objekt
            // zurueck, ist DAS das neue `this`.
            //
            // GEMESSEN vorher: hier lag nichts — die Anweisung `super();`
            // (Ausdruck + `Pop(1)`) nahm darum einen Wert UNTER dem
            // Kind-Rahmen weg, also beim Aufrufer: `({ k: new B() })` mit
            // `class B extends A { constructor() { super() } }` warf „kein
            // Setzen auf Primitiv", `[9, new B()].length` las `undefined`.
            // Auf bbc.com (Optimizely-SDK) brach so die Next.js-Hydration.
            let neu_this = if self.ist_objekt(v) { v } else { this };
            if let Some(r) = self.this_rahmen_index().and_then(|i| self.rahmen.get_mut(i)) {
                r.this = neu_this;
            }
            self.pfeile_nach_super_binden();
            self.stapel.push(neu_this);
        }
        self.rahmen.len() <= self.lauf_ende
    }

    /// Ausnahme werfen: Handler suchen, sonst Rahmen verlassen.
    fn werfen(&mut self, w: GVal) -> Result<(), LaufErgebnis> {
        let ende = self.lauf_ende;
        loop {
            if self.rahmen.is_empty() {
                return Err(LaufErgebnis::Wurf(w));
            }
            let h = self.rahmen.last().unwrap().handler.last().copied();
            match h {
                Some(hi) => {
                    let hd = self.handler_von(hi);
                    match hd {
                        // Weder `catch` noch `finally`: der Handler kann
                        // nichts behandeln. Der Wurf laeuft nach aussen.
                        //
                        // GEMESSEN (J7): sonst sprang der Wurf auf Befehl 0
                        // (`fang == 0` als Merkmal „kein catch"), also wieder
                        // in den `try`-Rumpf — `try { throw 1 } finally { }`
                        // lief endlos bis zur Zeitgrenze.
                        Some(h) if h.fang == 0 && !h.hat_finally => {
                            self.rahmen.last_mut().unwrap().handler.pop();
                            continue;
                        }
                        Some(h) => {
                            // Reihenfolge wie in JS: erst `catch`, dann
                            // `finally` (der Rumpf des catch-Blocks laeuft am
                            // Ende in den finally-Block). Ohne `catch`
                            // (fang == 0) geht es direkt in den finally.
                            if h.hat_finally && h.fang == 0 {
                                let f = self.rahmen.last_mut().unwrap();
                                // GEMESSEN: beim Sprung in den `finally`-Block
                                // muss der Handler AUS der Liste. Sonst fand
                                // der weitergereichte Wurf wieder denselben
                                // Handler und der `finally`-Block lief endlos
                                // (Ausgabe: tausende „f"-Zeilen).
                                f.handler.pop();
                                f.fertig.push(Aktion::Wurf(w));
                                f.pc = h.finally_ as usize;
                            } else {
                                self.rahmen.last_mut().unwrap().pc = h.fang as usize;
                                self.stapel.push(w);
                            }
                            return Ok(());
                        }
                        None => return Err(LaufErgebnis::Wurf(w)),
                    }
                }
                None => {
                    // Kein Handler: Rahmen verlassen, Ausnahme weitergeben.
                    let b = self.rahmen.last().unwrap().basis;
                    self.stapel.truncate(b);
                    // J3: auch beim Ausnahmepfad die Wurzeln des
                    // Rahmens mitnehmen.
                    let marke = self.rahmen.last().map(|r| r.wurzel_marke);
                    self.rahmen.pop();
                    if let Some(m) = marke {
                        self.wurzel_zurueck(m);
                    }
                    if self.rahmen.len() <= ende {
                        return Err(LaufErgebnis::Wurf(w));
                    }
                }
            }
        }
    }

    /// ## W22 (JS, 05.10.2026) — `return(v)`: DER WURF LAEUFT NUR DURCH `finally`
    ///
    /// `werfen` kann hier **nicht** wiederverwendet werden: es steigt bei
    /// einem Handler **mit `catch`** in den `catch`-Rumpf ein. Bei
    /// `gen.return(v)` darf der `catch`-Rumpf aber **nicht** betreten werden —
    /// `return` ist eine **Rueckgabe**, keine Ausnahme (ES2023 27.3.5
    /// `GeneratorResumeAbrupt`, Schritt 6: der `[[Catch]]`-Zweig wird
    /// uebersprungen).
    ///
    /// GEMESSEN in node 22, alle vier Randfaelle:
    /// ```text
    /// R1 return(7)                       -> {"value":99,"done":false}
    /// R2 next() danach                   -> {"value":7,"done":true}
    /// R3 return(7) dann next(5)          -> {"value":7,"done":true}   (5 ignoriert)
    /// R5 zweites return(8)               -> {"value":8,"done":true}   (schliesst sofort)
    /// R7 try{return 'a'}finally{}         -> {"value":"a","done":true}
    /// ```
    /// R5 ist das, was `ertrags_art == 2` braucht: ein **zweites** `return`
    /// schliesst den Generator **sofort**, es laeuft **nicht** wieder in den
    /// `finally`-Rumpf. Und R3 ist der Grund, warum der gestellte Wert in
    /// diesem Zustand **verworfen** wird.
    fn rueckgabe_abwickeln(&mut self, w: GVal) -> Result<(), LaufErgebnis> {
        let ende = self.lauf_ende;
        loop {
            if self.rahmen.is_empty() {
                return Err(LaufErgebnis::Wurf(w));
            }
            let h = self.rahmen.last().unwrap().handler.last().copied();
            match h {
                Some(hi) => match self.handler_von(hi) {
                    // Weder `catch` noch `finally`: der Handler kann nichts
                    // behandeln — der Wurf laeuft nach aussen (wie `werfen`).
                    Some(h) if h.fang == 0 && !h.hat_finally => {
                        self.rahmen.last_mut().unwrap().handler.pop();
                        continue;
                    }
                    // `finally`: hineinspringen, den Wurf als `Aktion::Wurf`
                    // vormerken. Am Ende des `finally` (`Op::FertigEnde`) wird
                    // er **weitergereicht** und findet dort den naechsten
                    // Handler — das ist die Kette fuer geschachtelte
                    // `finally`.
                    Some(h) if h.hat_finally => {
                        let f = self.rahmen.last_mut().unwrap();
                        f.handler.pop();
                        f.fertig.push(Aktion::Wurf(w));
                        f.pc = h.finally_ as usize;
                        return Ok(());
                    }
                    // NUR `catch`, kein `finally`: `return` laeuft **dran
                    // vorbei** — der `catch`-Rumpf wird nicht betreten.
                    Some(_) => {
                        self.rahmen.last_mut().unwrap().handler.pop();
                        continue;
                    }
                    None => return Err(LaufErgebnis::Wurf(w)),
                },
                None => {
                    // Kein Handler mehr: der Rahmen endet, der Wert ist die
                    // **Rueckgabe** des Generators.
                    let b = self.rahmen.last().unwrap().basis;
                    self.stapel.truncate(b);
                    let marke = self.rahmen.last().map(|r| r.wurzel_marke);
                    self.rahmen.pop();
                    if let Some(m) = marke {
                        self.wurzel_zurueck(m);
                    }
                    if self.rahmen.len() <= ende {
                        return Err(LaufErgebnis::Wurf(w));
                    }
                }
            }
        }
    }

    /// Handler-Datensatz des aktuellen Rahmens.
    fn handler_von(&self, idx: usize) -> Option<crate::op::Handler> {
        let r = self.rahmen.last()?;
        let p = self.programm.as_ref()?;
        let f = p.funktionen.get(r.fun)?;
        f.handler.get(idx).cloned()
    }

    // =================================================================
    // Befehle
    // =================================================================

    /// Release 09.10.2026: der HEISSE Teil der Befehle, mit kleinem
    /// Stapelrahmen. Alles andere geht an `befehl_kalt`.
    ///
    /// GEMESSEN, warum: `befehl` mit allen Armen hatte einen Rahmen von
    /// 4 888 Byte (`sub rsp,1318h` hinter `call __chkstk`) — Windows prueft
    /// dafuer bei JEDEM Befehl zwei Stapelseiten, dazu acht gesicherte
    /// Register. Eine leere `for`-Schleife kostete 180 ns je Durchlauf
    /// (node --jitless: 25 ns). Die Zahl-Schnellwege fuer Vergleiche und
    /// `+ - *` umgehen `binaer_werte` (Umwandlungsregeln) ganz.
    #[inline]
    fn befehl(&mut self, op: &Op) -> Kontrolle {
        match *op {
            Op::Zeile(z) => {
                if let Some(r) = self.rahmen.last_mut() {
                    r.zeile = z;
                }
            }
            Op::Undef => self.stapel.push(GVal::Undefiniert),
            Op::Null => self.stapel.push(GVal::Null),
            Op::Wahr => self.stapel.push(GVal::Bool(true)),
            Op::Falsch => self.stapel.push(GVal::Bool(false)),
            Op::Zahl(z) => self.stapel.push(GVal::Zahl(z)),
            Op::Pop(n) => {
                let l = self.stapel.len();
                self.stapel.truncate(l.saturating_sub(n as usize));
            }
            Op::Spring(z) => {
                if let Some(r) = self.rahmen.last_mut() {
                    r.pc = z as usize;
                }
            }
            Op::SpringWennFalsch(z) => {
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                let wahr = match v {
                    GVal::Bool(b) => b,
                    _ => self.zu_bool(v),
                };
                if !wahr && let Some(r) = self.rahmen.last_mut() {
                    r.pc = z as usize;
                }
            }
            Op::SpringWennWahr(z) => {
                let v = self.stapel.last().copied().unwrap_or(GVal::Undefiniert);
                let wahr = match v {
                    GVal::Bool(b) => b,
                    _ => self.zu_bool(v),
                };
                if wahr && let Some(r) = self.rahmen.last_mut() {
                    r.pc = z as usize;
                }
            }
            Op::TempSet(t) => {
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                self.temp_set(t, v);
            }
            Op::TempGet(t) => {
                let v = self.temp(t);
                self.stapel.push(v);
            }
            Op::TempLoesche(t) => self.temp_set(t, GVal::Undefiniert),
            Op::Param(i) => {
                let v = self
                    .rahmen
                    .last()
                    .and_then(|r| r.args.get(i as usize).copied())
                    .unwrap_or(GVal::Undefiniert);
                self.stapel.push(v);
            }
            Op::VersuchStart(i) => {
                if let Some(r) = self.rahmen.last_mut() {
                    r.handler.push(i as usize);
                }
            }
            Op::VersuchEnde => {
                if let Some(r) = self.rahmen.last_mut() {
                    r.handler.pop();
                }
            }
            Op::Text(i) => return self.op_text(i),
            Op::LeseVar(i) => return self.op_lese_var(i),
            // Release 09.10.2026: `a[i]` auf einem dichten Feld ohne
            // Zusatzeigenschaften (Accessoren auf Indizes liegen in `rest`,
            // siehe `eigenschaften.rs` J27) direkt aus `elemente`.
            // GEMESSEN vorher: jeder Indexzugriff machte aus der Zahl einen
            // Text und suchte danach per Name (youtube.com: 1,6 Mio
            // `LeseIndex` zu ~370 ns).
            Op::LeseIndex => {
                let l = self.stapel.len();
                if l >= 2
                    && let (GVal::Ref(r), GVal::Zahl(n)) = (self.stapel[l - 2], self.stapel[l - 1])
                    && n >= 0.0
                    && n < 4_294_967_295.0
                    && n.fract() == 0.0
                    && let HObj::Feld { elemente, rest, luecken, .. } = self.heap.holen(r)
                    && rest.is_empty()
                {
                    let i = n as usize;
                    if i < elemente.len() && !luecken.as_ref().is_some_and(|m| m.contains(&i)) {
                        let v = elemente[i];
                        self.stapel.truncate(l - 2);
                        self.stapel.push(v);
                        return Kontrolle::Weiter;
                    }
                }
                return self.befehl_kalt(op);
            }
            // Release 09.10.2026: `a[i] = v` ueberschreibt ein vorhandenes
            // Element direkt (offenes, dichtes Feld ohne `rest`).
            Op::SchreibeIndex => {
                let l = self.stapel.len();
                if l >= 3
                    && let (GVal::Ref(r), GVal::Zahl(n)) = (self.stapel[l - 3], self.stapel[l - 1])
                    && n >= 0.0
                    && n < 4_294_967_295.0
                    && n.fract() == 0.0
                {
                    let wert = self.stapel[l - 2];
                    if let HObj::Feld { elemente, rest, luecken, sperre, .. } = self.heap.holen_mut(r)
                        && rest.is_empty()
                        && matches!(sperre, crate::heap::Sperre::Offen)
                        && luecken.is_none()
                        && let Some(platz) = elemente.get_mut(n as usize)
                    {
                        *platz = wert;
                        self.stapel.truncate(l - 3);
                        self.stapel.push(wert);
                        return Kontrolle::Weiter;
                    }
                }
                return self.befehl_kalt(op);
            }
            Op::StriktGleich
            | Op::StriktUngleich
            | Op::Kleiner
            | Op::KleinerGleich
            | Op::Groesser
            | Op::GroesserGleich
            | Op::Plus
            | Op::Minus
            | Op::Mal => {
                let l = self.stapel.len();
                if l >= 2
                    && let (GVal::Zahl(a), GVal::Zahl(b)) = (self.stapel[l - 2], self.stapel[l - 1])
                {
                    let v = match *op {
                        Op::StriktGleich => GVal::Bool(a == b),
                        Op::StriktUngleich => GVal::Bool(a != b),
                        Op::Kleiner => GVal::Bool(a < b),
                        Op::KleinerGleich => GVal::Bool(a <= b),
                        Op::Groesser => GVal::Bool(a > b),
                        Op::GroesserGleich => GVal::Bool(a >= b),
                        Op::Plus => GVal::Zahl(a + b),
                        Op::Minus => GVal::Zahl(a - b),
                        _ => GVal::Zahl(a * b),
                    };
                    self.stapel.truncate(l - 2);
                    self.stapel.push(v);
                } else {
                    return self.befehl_kalt(op);
                }
            }
            _ => return self.befehl_kalt(op),
        }
        Kontrolle::Weiter
    }

    /// Release 09.10.2026: der geteilte Heap-Text eines `typeof`-Ergebnisses
    /// (siehe `Engine::typ_werte`).
    fn typ_text(&mut self, s: &'static str) -> Result<GVal, JsFehler> {
        if let Some((_, v)) = self.typ_werte.iter().find(|(t, _)| std::ptr::eq(*t, s) || *t == s) {
            return Ok(*v);
        }
        let v = self.text_wert(s)?;
        self.typ_werte.push((s, v));
        Ok(v)
    }

    /// Release 09.10.2026 (Diagnose, `NARSIL_JS_DIAG`): Funktion, Zeile und
    /// die letzten Befehle vor einem Zugriff auf ein totes Objekt.
    #[cold]
    fn diag_totes_objekt(&self, name: &str) {
        let Some(rahmen) = self.rahmen.last() else { return };
        let pc = rahmen.pc;
        let fun = rahmen.fun;
        let von = pc.saturating_sub(10);
        let ops: Vec<String> = rahmen.code[von..pc.min(rahmen.code.len())].iter().map(|o| format!("{o:?}")).collect();
        let f = self.programm.as_ref().and_then(|p| p.funktionen.get(fun));
        let fname = f.map(|f| f.name.clone()).unwrap_or_default();
        let namen: Vec<String> = rahmen.code[von..pc.min(rahmen.code.len())]
            .iter()
            .filter_map(|o| match o {
                Op::LeseVar(z) => f.and_then(|f| f.namen.get(*z as usize)).map(|n| format!("var {n}")),
                _ => None,
            })
            .collect();
        let tiefe = self.rahmen.len();
        eprintln!(
            "NARSIL_JS_DIAG totes Objekt bei .{name}: fun {fun} ({fname}) Zeile {} pc {pc} tiefe {tiefe} ops {ops:?} namen {namen:?}",
            rahmen.zeile
        );
    }

    /// Release 09.10.2026: `Op::Text` mit geteiltem Heap-Text je Konstante
    /// (siehe `Engine::text_werte`).
    #[inline(never)]
    fn op_text(&mut self, i: u32) -> Kontrolle {
        if let Some(&v) = self.text_werte.get(i as usize)
            && v.ist_ref()
        {
            self.stapel.push(v);
            return Kontrolle::Weiter;
        }
        let s = self.text_konstante(i);
        match self.text_wert(&s) {
            Ok(v) => {
                let n = self.texte_arc.len().max(i as usize + 1);
                if self.text_werte.len() < n {
                    self.text_werte.resize(n, GVal::Undefiniert);
                }
                self.text_werte[i as usize] = v;
                self.stapel.push(v);
                Kontrolle::Weiter
            }
            Err(e) => Kontrolle::Fehler(e),
        }
    }

    /// Release 09.10.2026: `Op::LeseVar` als eigene Funktion (kleiner Rahmen
    /// im heissen `befehl`).
    #[inline(never)]
    fn op_lese_var(&mut self, i: u32) -> Kontrolle {
        // J3b: Hash statt Text (keine Allokation, kein String-Hash).
        let h = self.name_hash(i);
        // J48: im **Parameterprolog** ist die Parameterumgebung die
        // richtige — siehe `Engine::lauf_umg`.
        let umg = self.lauf_umg();
        if let Some(v) = self.lese_schnell(umg, h) {
            // Release 09.10.2026 (Diagnose): eine Bindung mit totem Wert.
            if diag_an()
                && let GVal::Ref(r) = v
                && !self.heap.lebt(r)
            {
                let n = self.name(i);
                let mut tiefe = 0usize;
                let mut cur = Some(umg);
                while let Some(u) = cur {
                    let HObj::Umgebung(env) = self.heap.holen(u) else { break };
                    if env.bindungen.find_h(h).is_some() {
                        break;
                    }
                    tiefe += 1;
                    cur = env.eltern;
                }
                eprintln!("NARSIL_JS_DIAG Variable `{n}` haelt toten Wert (Umgebung Tiefe {tiefe}, global={})", cur == Some(self.global_umgebung));
                self.diag_totes_objekt(&n);
            }
            self.stapel.push(v);
            return Kontrolle::Weiter;
        }
        // Release 09.10.2026: der allgemeine Weg MIT Namen (Index im eigenen
        // Pool, geliehen) — ohne ihn sucht `name_fuer_text` den Text erst per
        // Hash.
        let pool = self.namen_pool();
        let name: &str = pool.get(i as usize).map(|s| s.as_str()).unwrap_or("");
        match self.lese_h(umg, h, name) {
            Ok(v) => self.schiebe(v),
            Err(_) => {
                // J3b: der kalte Weg — fuer die MELDUNG wird derselbe Zugriff
                // mit Text wiederholt (ein zweiter Versuch kann auch gelingen,
                // z. B. am Globalobjekt).
                let n = self.name(i);
                match self.lese(umg, &n) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(e),
                }
            }
        }
        Kontrolle::Weiter
    }

    /// Alle uebrigen Befehle (die heissen erledigt `befehl` selbst).
    #[inline(never)]
    fn befehl_kalt(&mut self, op: &Op) -> Kontrolle {
        match op {
            // --- J28: die Zeile ---
            //
            // GEMESSEN, warum der Preis so klein ist: der Befehl schiebt
            // **nichts** und gibt `Kontrolle::Weiter` zurueck — er kostet
            // einen Muskelabgleich im `match` und sonst nichts. Er steht
            // VOR dem Haken (`schleife_innen` prueft die Zeile **vor**
            // `schritt()`), damit ein Haltepunkt auf einer Zeile auch
            // dort gilt, wo noch kein Befehl dieser Zeile gelaufen ist.
            // J28: `debugger;` — der Haken wartet auf `weiter`.
            Op::Debugger => {
                if let Some(h) = self.haken.as_mut() {
                    h.debugger_anweisung = true;
                }
            }
            Op::Zeile(z) => {
                if let Some(r) = self.rahmen.last_mut() {
                    r.zeile = *z;
                }
            }
            // --- Literale ---
            Op::Null => self.schiebe(GVal::Null),
            Op::Undef => self.schiebe(GVal::Undefiniert),
            Op::Wahr => self.schiebe(GVal::Bool(true)),
            Op::Falsch => self.schiebe(GVal::Bool(false)),
            Op::Zahl(z) => self.schiebe(GVal::Zahl(*z)),
            Op::Ganz(g) => self.schiebe(GVal::Ganz(*g)),
            Op::Text(i) => {
                let s = self.text_konstante(*i);
                match self.text_wert(&s) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(Stoerung::Fehler(e)),
                }
            }
            Op::Nichts => {}

            // --- Stapel ---
            Op::Pop(n) => {
                let l = self.stapel.len();
                self.stapel.truncate(l.saturating_sub(*n as usize));
            }
            Op::Dup => {
                let v = self.stapel.last().copied().unwrap_or(GVal::Undefiniert);
                self.schiebe(v);
            }
            Op::Pick(n) => {
                let l = self.stapel.len();
                let v = if (*n as usize) < l {
                    self.stapel[l - 1 - *n as usize]
                } else {
                    GVal::Undefiniert
                };
                self.schiebe(v);
            }
            Op::DreheLinks(n) => {
                let l = self.stapel.len();
                let k = *n as usize + 1;
                if l >= k {
                    let basis = l - k;
                    let x0 = self.stapel[basis];
                    self.stapel.copy_within(basis + 1..l, basis);
                    self.stapel[l - 1] = x0;
                }
            }
            Op::Reserviere => self.schiebe(GVal::Undefiniert),

            // --- Variablen ---
            Op::LeseVar(i) => {
                // J3b: Hash statt Text (keine Allokation, kein String-Hash).
                let h = self.name_hash(*i);
                // J48: im **Parameterprolog** ist die Parameterumgebung die
                // richtige — siehe `Engine::lauf_umg`.
                //
                // GEMESSEN/GEFUNDEN: dies war die **letzte** Namensstelle,
                // die noch die Rumpfumgebung nahm. Deshalb sah der Ausdruck
                //   `a = arguments`
                // die hochgezogene Funktion des Rumpfs statt des
                // `arguments`-Objekts:
                //   `var a; var f = function(x = (a = arguments)) {
                //      function arguments() {} }; f(); typeof a`
                //   -> node `object`, im Motor `function`
                let umg = self.lauf_umg();
                match self.lese_h(umg, h, "") {
                    Ok(v) => self.schiebe(v),
                    Err(_) => {
                        // J3b: der kalte Weg. Der Hash-Weg kennt den Namen
                        // nicht mehr (er soll ihn nicht kopieren) — fuer die
                        // MELDUNG wird derselbe Zugriff mit Text wiederholt.
                        let n = self.name(*i);
                        // `match` statt `unwrap_err()`: GEFUNDEN, dass ein
                        // zweiter Versuch auch **gelingen** kann (z. B. wenn
                        // der Hash-Weg am Globalobjekt scheitert) — dann darf
                        // es keinen Panik geben.
                        match self.lese(umg, &n) {
                            Ok(v) => self.schiebe(v),
                            Err(e) => return self.stoerung(e),
                        }
                    }
                }
            }
            Op::VarSetzen(i) => {
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                let h = self.name_hash(*i);
                let mut cur = Some(self.rahmen.last().unwrap().umg);
                let mut gesetzt = false;
                let mut rest = 100_000;
                while let Some(u) = cur {
                    rest -= 1;
                    if rest == 0 {
                        break;
                    }
                    let (art, eltern) = match self.heap.holen(u) {
                        HObj::Umgebung(env) => (env.bindungen.find_h(h).map(|b| b.art), env.eltern),
                        _ => (None, None),
                    };
                    if art == Some(BindungsArt::Var) {
                        if u == self.global_umgebung {
                            let n = self.name(*i);
                            self.initialisiere(u, &n, v);
                        } else {
                            self.initialisiere_h(u, h, v);
                        }
                        gesetzt = true;
                        break;
                    }
                    cur = eltern;
                }
                if !gesetzt {
                    let n = self.name(*i);
                    let g = self.global_umgebung;
                    if !self.umgebung_hat(g, &n) {
                        let _ = self.deklariere(g, &n, BindungsArt::Var);
                    }
                    self.initialisiere(g, &n, v);
                }
            }
            Op::SchreibeVar(i) | Op::ZuweisVar(i) => {
                // J3b: **kein** `self.name(*i)` mehr im heissen Weg — das war
                // eine String-Kopie (eine Allokation) je Zuweisung.
                let umg = self.rahmen.last().unwrap().umg;
                let v = self.stapel.last().copied().unwrap_or(GVal::Undefiniert);
                // Release 09.10.2026: vorhandene, veraenderbare Bindung ->
                // direkt schreiben (siehe `Engine::schreibe_schnell`).
                if self.schreibe_schnell(umg, self.name_hash(*i), v) {
                    return Kontrolle::Weiter;
                }
                let strikt = self.rahmen_strikt();
                // GEMESSEN: geschrieben wird in die Umgebung, die die
                // Bindung HAelt — nicht in die eigene Rahmenumgebung.
                // Sonst legte `() => ++i` in jedem Aufruf eine eigene
                // Bindung `i` an (schatten) und der Zaehler blieb 0
                // (p1.js Z2: 1 statt 2).
                // J3b: der Zielrahmen wird ueber den Hash gefunden, damit
                // `suche` + `schreibe` nicht zweimal laufen.
                let h = self.name_hash(*i);
                // Release 12.10.2026: liegt ein `with`-Objekt in der Kette,
                // entscheidet die Suche MIT Objekten (ES2023 9.1.1.2.5
                // SetMutableBinding eines Object Environment Record): hat das
                // Objekt den Namen, wird SEINE Eigenschaft geschrieben.
                // GEMESSEN vorher (Agent Range, WPT Range-surroundContents
                // 0/1840): eine Funktion aus einem `with (__w)`-Block schrieb
                // `paras = []` in die aeussere `var`, nicht nach `__w.paras`.
                if self.kette_hat_mit(umg) {
                    match self.suche_mit(umg, h, "") {
                        Ok(Some((wo, _))) if !matches!(self.heap.holen(wo), HObj::Umgebung(_)) => {
                            let n = self.name(*i);
                            if let Err(e) = self.setze(GVal::Ref(wo), &n, v) {
                                return self.stoerung(e);
                            }
                            return Kontrolle::Weiter;
                        }
                        Ok(_) => {}
                        Err(e) => return self.stoerung(e),
                    }
                }
                let ziel = self.suche_h(umg, h, "").map(|(u, _)| u).unwrap_or(umg);
                let umg = ziel;
                if self.schreibe_h(umg, h, "", v, strikt).is_err() {
                    // J3b: kalter Weg fuer die Meldung (siehe `LeseVar`).
                    let n = self.name(*i);
                    if let Err(e) = self.schreibe(umg, &n, v, strikt) {
                        return self.stoerung(e);
                    }
                }
            }
            // --- J3b: Superinstruktionen der Schleife ---
            Op::TestWertKleinerZahlSpring(i, zahl, ziel) => {
                let h = self.name_hash(*i);
                let umg = self.rahmen.last().unwrap().umg;
                let schnell = self.lese_schnell(umg, h);
                let wert = match schnell.map(Ok).unwrap_or_else(|| self.lese_h(umg, h, "")) {
                    Ok(v) => v,
                    Err(_) => {
                        // Kalter Weg: dieselbe Lese mit Text, damit die
                        // Meldung den Namen nennt (J3b).
                        let n = self.name(*i);
                        match self.lese(umg, &n) {
                            Ok(v) => v,
                            Err(e) => return self.stoerung(e),
                        }
                    }
                };
                // Die Zahl ist zur Uebersetzungszeit bekannt; ist der Wert
                // eine Zahl, wird **ohne** Aufruf von `kleiner` verglichen
                // (das war der teure Teil: ToPrimitive + Aufrufkette).
                let kleiner = match wert {
                    GVal::Zahl(n) => n < *zahl,
                    // Alles andere ueber den gewohnten Weg — die
                    // JS-Vergleichssemantik bleibt damit exakt.
                    _ => {
                        // Der Weg ueber `kleiner` nimmt ein GVal; die Zahl
                        // ist eine `GVal::Zahl`.
                        match self.kleiner(wert, GVal::Zahl(*zahl), false) {
                            Ok(b) => b,
                            Err(e) => return self.stoerung(e),
                        }
                    }
                };
                if !kleiner {
                    self.rahmen.last_mut().unwrap().pc = *ziel as usize;
                }
            }
            Op::ErhoeheUndSchreibe(i) => {
                let h = self.name_hash(*i);
                let umg = self.rahmen.last().unwrap().umg;
                // Release 09.10.2026: Zahl in der Bindung -> direkt erhoehen.
                if let Some(GVal::Zahl(n)) = self.lese_schnell(umg, h)
                    && self.schreibe_schnell(umg, h, GVal::Zahl(n + 1.0))
                {
                    return Kontrolle::Weiter;
                }
                let alt = match self.lese_h(umg, h, "") {
                    Ok(v) => v,
                    Err(_) => {
                        // Kalter Weg: dieselbe Lese mit Text, damit die
                        // Meldung den Namen nennt (J3b).
                        let n = self.name(*i);
                        match self.lese(umg, &n) {
                            Ok(v) => v,
                            Err(e) => return self.stoerung(e),
                        }
                    }
                };
                // `++` macht immer eine Zahl aus dem Wert (JS: ToNumber).
                let neu = match alt {
                    GVal::Zahl(n) => GVal::Zahl(n + 1.0),
                    other => match self.zu_zahl(other) {
                        Ok(n) => GVal::Zahl(n + 1.0),
                        Err(e) => return self.stoerung(e),
                    },
                };
                let strikt = self.strikt;
                if self.schreibe_h(umg, h, "", neu, strikt).is_err() {
                    let n = self.name(*i);
                    if let Err(e) = self.schreibe(umg, &n, neu, strikt) {
                        return self.stoerung(e);
                    }
                }
            }
            Op::DefVar(i, art) => {
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                let umg = self.rahmen.last().unwrap().umg;
                let a = match art {
                    1 => BindungsArt::Let,
                    2 => BindungsArt::Const,
                    _ => BindungsArt::Var,
                };
                // J34: der Code 3 ist `var` **ohne** Initialisierer. Er
                // legt die Bindung an, wenn sie fehlt, und laesst eine
                // vorhandene **unangetastet** — sonst ueberschrieb
                // `function f(a){ var a; return a }` den Parameter mit
                // `undefined` (GEMESSEN: node liefert `7`).
                let nur_anlegen = *art == 3;
                // J8: im Globalrahmen mit Namen (Aufzaehlung!), sonst nur
                // ueber den Hash — sonst kostet jeder Parameter zwei
                // Zeichenketten-Kopien je Aufruf (Begruendung bei
                // `Engine::deklariere_h`).
                if umg == self.global_umgebung {
                    let name = self.name(*i);
                    let gab_es_bereits = self.umgebung_hat(umg, &name);
                    if !gab_es_bereits && let Err(e) = self.deklariere(umg, &name, a) {
                        return Kontrolle::Fehler(e);
                    }
                    if !(nur_anlegen && gab_es_bereits) {
                        self.initialisiere(umg, &name, v);
                    }
                } else {
                    let h = self.name_hash(*i);
                    let gab_es_bereits = self.umgebung_hat_h(umg, h);
                    // J34: `var` ist **funktionsweit**, nicht blockweit
                    // (ES2023 8.2.5 `VariableStatement`, Static Semantics
                    // `VarDeclaredNames`). Der Rahmen deklariert alle
                    // `var_namen` **vorab** (`rahmen_starten*`), aber
                    // `DefVar` schrieb in die **innerste** Umgebung — in
                    // einem `catch`, einem Block oder einer Schleife also in
                    // eine neue, die den Vorbereiteten nur ueberdeckte.
                    // GEMESSEN (node 22):
                    //   `function f(){ try { throw 1 } catch (e) { var a = 2 }
                    //    return a } f()` -> `2`, in diesem Motor `undefined`.
                    // Deshalb: erst die Kette nach der **deklarierenden**
                    // Umgebung durchsuchen, dort initialisieren, und nur
                    // deklarieren, wenn es keine gibt. Fuer `let`/`const`
                    // (art 1/2) gilt das **nicht** — die sind blockweit.
                    // Release 12.10.2026: `var x = v` ist eine ZUWEISUNG ueber
                    // die aktuelle Kette (ES2023 14.3.2.1: ResolveBinding, dann
                    // PutValue) — hat ein `with`-Objekt davor den Namen, geht
                    // der Wert an SEINE Eigenschaft. GEMESSEN vorher (node 22):
                    // `with (o = {p: 1}) { var p = 5 } o.p` -> node `5`, Motor `1`.
                    if !gab_es_bereits && *art == 0 && !nur_anlegen && self.kette_hat_mit(umg) {
                        match self.suche_mit(umg, h, "") {
                            Ok(Some((wo, _))) if !matches!(self.heap.holen(wo), HObj::Umgebung(_)) => {
                                let n = self.name(*i);
                                if let Err(e) = self.setze(GVal::Ref(wo), &n, v) {
                                    return self.stoerung(e);
                                }
                                return Kontrolle::Weiter;
                            }
                            Ok(_) => {}
                            Err(e) => return self.stoerung(e),
                        }
                    }
                    if !gab_es_bereits
                        && *art == 0
                        && !nur_anlegen
                        && let Some((wo, _)) = self.suche_h(umg, h, "")
                    {
                        // Release 09.10.2026: ein globales `var` traegt seinen
                        // Wert AUCH am globalen Objekt (siehe `initialisiere`).
                        // GEMESSEN vorher: `{ let z; var w = 5 } w` -> `undefined`
                        // auf oberster Ebene (der Hash-Weg schrieb nur die Bindung).
                        if wo == self.global_umgebung {
                            let name = self.name(*i);
                            self.initialisiere(wo, &name, v);
                        } else {
                            self.initialisiere_h(wo, h, v);
                        }
                        return Kontrolle::Weiter;
                    }
                    // Release 12.10.2026: auch `var x;` OHNE Initialisierer legt
                    // nichts im Block an, wenn die Funktion (bzw. eine aeussere
                    // Umgebung) den Namen schon hat. GEMESSEN vorher (node 22):
                    // `with (w) { var p; w.p = 0; function f() { p = 7 } f() }`
                    // -> node `w.p === 7`; im Motor verdeckte die Blockbindung
                    // `p` das `with`-Objekt (Agent Range, Rahmenskripte).
                    if !gab_es_bereits && nur_anlegen && self.suche_h(umg, h, "").is_some() {
                        return Kontrolle::Weiter;
                    }
                    if !gab_es_bereits && let Err(e) = self.deklariere_h(umg, h, a) {
                        return Kontrolle::Fehler(e);
                    }
                    if !(nur_anlegen && gab_es_bereits) {
                        self.initialisiere_h(umg, h, v);
                    }
                }
            }
            Op::DefParam(i, art) => {
                // J48: eine **Parameter**-Bindung. Sie gehoert in die
                // Parameterumgebung (`param_umg`), wenn es eine gibt — der
                // Rumpf darf sie nicht sehen.
                //
                // GEMESSEN (node 22):
                //   `var x = 'outside';
                //    var pp, pb;
                //    function f(a = (pp = function(){ return x; })) {
                //      var x = 'inside'; pb = function(){ return x; };
                //    }
                //    f(); pp()` -> `outside`, `pb()` -> `inside`
                //
                // Ohne zweite Umgebung gab **beides** `inside`.
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                let umg = self
                    .rahmen
                    .last()
                    .and_then(|r| r.param_umg)
                    .unwrap_or(self.rahmen.last().unwrap().umg);
                let a = match art {
                    1 => BindungsArt::Let,
                    2 => BindungsArt::Const,
                    _ => BindungsArt::Var,
                };
                let h = self.name_hash(*i);
                if !self.umgebung_hat_h(umg, h)
                    && let Err(e) = self.deklariere_h(umg, h, a)
                {
                    return Kontrolle::Fehler(e);
                }
                self.initialisiere_h(umg, h, v);
            }
            Op::NamenSetzenBerechnet => {
                // J41: Schluessel oben, Wert darunter (siehe `op.rs`).
                let l = self.stapel.len();
                if l < 2 {
                    return Kontrolle::Weiter;
                }
                let name = self.stapel[l - 1];
                let wert = self.stapel[l - 2];
                // Release 09.10.2026: der Schluessel BLEIBT liegen — beide
                // Verwender brauchen ihn danach (`DefEigenschaftNamen` im
                // Objektliteral, `SchreibeIndex` bei Klassenfeldern).
                // GEMESSEN vorher: `var o = {[k]: function(){}}` endete mit
                // „Laufzeitfehler: Stapel zu kurz" (nicht fangbar) — ebenso
                // `{[Symbol.iterator]: function(){…}}`.
                self.namen_setzen_auf(wert, name);
            }
            Op::NamenSetzen(i) => {
                // J34: `SetFunctionName` fuer `const f = function(){}` und
                // `const {a = function(){}} = {}`.
                // GEMESSEN/GEFUNDEN: `text_konstante` ist der Pool der
                // **Textkonstanten**; Funktionsnamen liegen im **Namenpool**
                // (`self.name`, derselbe wie bei `Op::DefVar`). Mit dem
                // falschen Pool kam ein **leerer** Name an und der Zweig
                // schrieb nichts — der Op sah aus, als wuerde er nie
                // ausgefuehrt.
                //
                // GEMESSEN (node 22), was vorher **alles** leer war:
                //   `var gen = function*(){}`        -> `gen`
                //   `const {a = function(){}} = {}`  -> `a`
                //   `const {a = class{}} = {}`      -> `a`
                //   `const {a = ()=>{}} = {}`       -> `a`
                //
                // **NUR** wenn der Name noch leer ist: `const f = function
                // inn(){}` behaelt `inn` (GEMESSEN: node `inn`). Und **nur**
                // hier — NICHT bei `o.gen = function(){}`, wo node `""`
                // liefert (das ist eine Member-Zuweisung, keine Bindung).
                //
                // GEMESSEN, warum am **Objekt** und nicht an der
                // Funktionstabelle: `function mach(){ return function(){} }`
                // liefert zwei Closures aus **einer** Tabellenzeile. An der
                // Tabelle benannt, hiessen spaeter beide `f`.
                let name = self.name(*i);
                if let GVal::Ref(r) = self.stapel.last().copied().unwrap_or(GVal::Undefiniert)
                    && let HObj::Funktion { name: alt, .. } = self.heap.holen_mut(r)
                    && alt.is_empty()
                {
                    *alt = name;
                }
            }
            Op::This => {
                // J50: `GetThisBinding` — vor `super()` ist `this` nicht da.
                //
                // **EINE STELLE FUER DREI FORMEN:** der Uebersetzer bildet
                // auch `Ausdruck::Super` (die nackte `super`-Referenz) auf
                // `Op::This` ab — damit deckt diese Pruefung `this`,
                // `this.x = 1`, `delete super.m` und `delete super[(1,0)]`.
                //
                // GEMESSEN (node 22), **sechs** ReferenceError:
                //   `class D extends B { constructor(){ this.x = 1 } }`
                //   `class D extends B { constructor(){ typeof this } }`
                //   `class D extends B { constructor(){ this } }`
                //   `class D extends B { constructor(){ delete super.m } }`
                //   `class D extends B { constructor(){ delete super[(1,0)] } }`
                //   `class D extends B { constructor(){ } m(){ return this } }`
                if let Some(e) = self.this_geprueft() {
                    return Kontrolle::Wurf(e);
                }
                let v = self.this_rahmen();
                self.schiebe(v);
            }
            // W14 (JS, 04.10.2026): `new.target` — ES2023 13.3.7.2.
            //
            // Schritt 4: `function.[[NewTarget]]`, bei uns `Rahmen::neu`. Das
            // setzt `rahmen_starten_fn` bei `als_konstruktor`, `super_aufruf`
            // reicht es an den Elternkonstruktor weiter, und eine Pfeilfunktion
            // **erbt** es (ES2023 15.2.1).
            //
            // **Schritt 3 habe ich erst falsch gebaut und dann gemessen:**
            // die Norm sagt „If function is not a Function Object, throw a
            // SyntaxError exception", und ich habe daraus oben im Hauptlauf
            // einen Wurf gemacht. GEMESSEN an **node 22 (v22.23.3)**:
            //     `String(new.target)` auf Skript-Ebene  ->  `undefined`,
            //     **kein** Fehler (`typeof` ebenfalls `undefined`).
            // Der Grund: im globalen Code ist `[[Function]]` nicht gesetzt, und
            // die Engine liefert deshalb `undefined` statt zu werfen. Ich folge
            // der Messung, nicht dem Text — die Leitung hat ausdruecklich
            // node 22 zur Referenz erklaert (JS_10).
            Op::EvalDirekt => {
                self.eval_direkt = true;
            }
            Op::NeuZiel => {
                let v = self.rahmen.last().map(|r| r.neu.clone());
                self.schiebe(v.unwrap_or(GVal::Undefiniert));
            }
            Op::DefArgumente => {
                // J8: `arguments` wird nur gebaut, wenn die Funktion es
                // sehen kann — sonst kostete jeder Aufruf ~15 Allokationen
                // (Begruendung bei `op::Funktion::braucht_arguments`).
                //
                // GEMESSEN: `fib(27)` 737 -> 545 ms, ein Aufruf von ~1,4 us
                // auf ~0,55 us. Ein direktes `eval` im Rumpf setzt
                // `braucht_arguments` im Compiler (Release 09.10.2026).
                // J48: bei einer **nicht einfachen** Parameterliste gehoert
                // `arguments` in die **Parameterumgebung** — sonst wuerde ein
                // Ausdruck in der Parameterliste die Deklaration des Rumpfs
                // sehen.
                //
                // GEMESSEN (node 22), der Anlass
                // (`expressions/function/arguments-with-arguments-fn.js`):
                //   `var args;
                //    var f = function(x = args = arguments) {
                //      function arguments() {}
                //    };
                //    f(); typeof args`
                //   -> node `object`, `args.length` = `0`
                // In **einer** Umgebung sah der Motor hier die Funktion
                // `arguments` des Rumpfs (`typeof args` = `function`, und
                // ohne Parameter-Ausdruck `undefined`).
                //
                // ES2023 10.2.11 Schritt 20: nur bei **einfacher** Liste
                // darf eine Deklaration des Rumpfs `arguments` unterdruecken;
                // Schritt 29 legt die Bindung in `env` ab — der
                // Parameterumgebung.
                if !self.arguments_noetig() {
                    return Kontrolle::Weiter;
                }
                let args = self.rahmen.last().unwrap().args.clone();
                match self.argumente_anlegen(&args) {
                    Ok(r) => {
                        let umg = self
                            .rahmen
                            .last()
                            .unwrap()
                            .param_umg
                            .unwrap_or(self.rahmen.last().unwrap().umg);
                        // J59: **immer lokal** erklaeren, nie eine geerbte
                        // Bindung benutzen. ES2023 10.2.11 Schritt 29 legt
                        // `arguments` in `env` (bzw. der
                        // Parameterumgebung) an — als *eigene* Bindung, die
                        // eine aeussere verdeckt.
                        //
                        // GEMESSEN (node 22 als Soll), warum das der grösste
                        // Einzelfehler war: `Op::DefArgumente` suchte mit
                        // `suche(umg, "arguments")` zuerst die **Kette** hoch.
                        // Sobald `Engine::quelle_hat_eval` (vm.rs:210) `true`
                        // war, legte schon der **globale** Rahmen eine
                        // `arguments`-Bindung im globalen Namensraum an
                        // (`compile.rs:549` emittiert `DefArgumente` auch im
                        // globalen Programm). Danach fand jeder Funktions-
                        // rumpf diese globale Bindung, deklarierte nichts
                        // eigenes und schrieb in sie hinein — alle Rahmen
                        // teilten sich **eine** Bindung, der letzte Aufruf
                        // gewann:
                        //   `var q = "not be evaluated."`      // enthaelt "eval"
                        //   `function f(a,b,c){ return arguments.length; }`
                        //   `f(1,2,3)` -> **`0`** (Soll 3)
                        // `var q = "...";` brauchte dafuer gar nichts als die
                        // Zeichenkette: `quelle.contains("eval")` ist eine
                        // **Teilstring**-Suche, und das Harness `sta.js`
                        // enthaelt die Zeichenkette in `"Test262: This
                        // statement should not be evaluated."`. Damit war
                        // **jeder** test262-Fall betroffen, der `sta.js`
                        // laedt.
                        if let Err(e) = self.deklariere(umg, "arguments", BindungsArt::Var) {
                            return Kontrolle::Fehler(e);
                        }
                        self.initialisiere(umg, "arguments", GVal::Ref(r));
                    }
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::Param(i) => {
                let v = self
                    .rahmen
                    .last()
                    .unwrap()
                    .args
                    .get(*i as usize)
                    .copied()
                    .unwrap_or(GVal::Undefiniert);
                self.schiebe(v);
            }
            Op::RestParams(k) => {
                let args = self.rahmen.last().unwrap().args.clone();
                let rest: Vec<GVal> = args.iter().skip(*k as usize).copied().collect();
                match self.feld_aus_werten(&rest) {
                    Ok(r) => self.schiebe(GVal::Ref(r)),
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::TypVonName(i) => {
                let umg = self.rahmen.last().unwrap().umg;
                // Release 09.10.2026: erst der Hash-Schnellweg, Namenskopie
                // nur im allgemeinen Fall.
                let v = match self.lese_schnell(umg, self.name_hash(*i)) {
                    Some(v) => v,
                    None => {
                        let name = self.name(*i);
                        self.lese(umg, &name).unwrap_or(GVal::Undefiniert)
                    }
                };
                let s = self.typ_von(v);
                match self.typ_text(s) {
                    Ok(t) => self.schiebe(t),
                    Err(e) => return Kontrolle::Fehler(e),
                }
            }

            // --- Zweiwertige Operatoren ---
            Op::Plus
            | Op::Minus
            | Op::Mal
            | Op::Geteilt
            | Op::Modulo
            | Op::Potenz
            | Op::Kleiner
            | Op::KleinerGleich
            | Op::Groesser
            | Op::GroesserGleich
            | Op::Gleich
            | Op::Ungleich
            | Op::StriktGleich
            | Op::StriktUngleich
            | Op::InOperator
            | Op::InstanceVon
            | Op::BitUnd
            | Op::BitOder
            | Op::BitXor
            | Op::LinksSchieben
            | Op::RechtsSchieben
            | Op::UnsRechtsSchieben => {
                let l = self.stapel.len();
                if l < 2 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit(
                        "Operandenstapel zu kurz (intern)".to_string(),
                    ));
                }
                let b = self.stapel[l - 1];
                let a = self.stapel[l - 2];
                self.stapel.truncate(l - 2);
                if let Op::InOperator = op {
                    // `a in b`: a ist der Schluessel, b das Objekt.
                    let k = match self.schluessel_text(a) {
                        Ok(k) => k,
                        Err(e) => return self.stoerung(e),
                    };
                    if !matches!(b, GVal::Ref(_)) {
                        let e = self.fehler_wert("TypeError", "rechts von 'in' kein Objekt");
                        return Kontrolle::Wurf(e);
                    }
                    // Lazily loaded platform globals (`'WebAssembly' in self`).
                    if self.platform_pending()
                        && b == GVal::Ref(self.global_objekt)
                        && let Err(e) = self.platform_lazy_global(&k)
                    {
                        return self.stoerung(e);
                    }
                    let r = self.in_operator(&k, b);
                    self.schiebe(GVal::Bool(r));
                    return Kontrolle::Weiter;
                }
                if let Op::InstanceVon = op {
                    match self.instanceof(a, b) {
                        Ok(x) => self.schiebe(GVal::Bool(x)),
                        Err(e) => return self.stoerung(e),
                    }
                    return Kontrolle::Weiter;
                }
                let art = match op {
                    Op::Plus => BinaerOp::Plus,
                    Op::Minus => BinaerOp::Minus,
                    Op::Mal => BinaerOp::Stern,
                    Op::Geteilt => BinaerOp::Schrag,
                    Op::Modulo => BinaerOp::Prozent,
                    Op::Potenz => BinaerOp::SternStern,
                    Op::Kleiner => BinaerOp::Kleiner,
                    Op::KleinerGleich => BinaerOp::KleinerGleich,
                    Op::Groesser => BinaerOp::Groesser,
                    Op::GroesserGleich => BinaerOp::GroesserGleich,
                    Op::Gleich => BinaerOp::GleichGleich,
                    Op::Ungleich => BinaerOp::RufGleich,
                    Op::StriktGleich => BinaerOp::GleichGleichGleich,
                    Op::StriktUngleich => BinaerOp::RufGleichGleich,
                    Op::BitUnd => BinaerOp::Und,
                    Op::BitOder => BinaerOp::Oder,
                    Op::BitXor => BinaerOp::Hoch,
                    Op::LinksSchieben => BinaerOp::KleinerKleiner,
                    Op::RechtsSchieben => BinaerOp::GroesserGroesser,
                    _ => BinaerOp::GroesserGroesserGroesser,
                };
                match self.binaer_werte(art, a, b) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(e),
                }
            }

            // --- Einwertige Operatoren ---
            Op::Negativ
            | Op::Positiv
            | Op::LogischNicht
            | Op::BitNicht
            | Op::TypVon
            | Op::LeerWert
            | Op::Inc
            | Op::Dec => {
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                let r: Result<GVal, Stoerung> =
                    match op {
                        // J24: `-10n` bleibt ein BigInt (GEMESSEN: sonst
                        // schrieb `console.log(-10n)` „-10" statt „-10n").
                        Op::Negativ => match v {
                            GVal::Ganz(g) => match g.checked_neg() {
                                Some(g) => Ok(GVal::Ganz(g)),
                                None => Err(Stoerung::Wurf(self.fehler_wert(
                                    "RangeError",
                                    "-2^127 passt nicht in einen BigInt",
                                ))),
                            },
                            _ => self.zu_zahl(v).map(|x| GVal::Zahl(-x)),
                        },
                        Op::Positiv => self.zu_zahl(v).map(|x| GVal::Zahl(x)),
                        Op::LogischNicht => Ok(GVal::Bool(!self.zu_bool(v))),
                        Op::BitNicht => self
                            .zu_zahl(v)
                            .map(|x| GVal::Zahl(!crate::ops::als_int32(x) as f64)),
                        Op::TypVon => {
                            let s = self.typ_von(v);
                            self.typ_text(s).map_err(Stoerung::Fehler)
                        }
                        Op::LeerWert => Ok(GVal::Undefiniert),
                        Op::Inc => self.zu_zahl(v).map(|x| GVal::Zahl(x + 1.0)),
                        _ => self.zu_zahl(v).map(|x| GVal::Zahl(x - 1.0)),
                    };
                match r {
                    Ok(v2) => self.schiebe(v2),
                    Err(e) => return self.stoerung(e),
                }
            }

            // --- Sprünge ---
            Op::Spring(z) => {
                self.rahmen.last_mut().unwrap().pc = *z as usize;
            }
            Op::SpringWennFalsch(z) => {
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                if !self.zu_bool(v) {
                    self.rahmen.last_mut().unwrap().pc = *z as usize;
                }
            }
            Op::SpringWennWahr(z) => {
                let v = self.stapel.last().copied().unwrap_or(GVal::Undefiniert);
                if self.zu_bool(v) {
                    self.rahmen.last_mut().unwrap().pc = *z as usize;
                }
            }
            Op::SpringWennUndef(z) => {
                let v = self.stapel.last().copied().unwrap_or(GVal::Undefiniert);
                if matches!(v, GVal::Undefiniert) {
                    self.rahmen.last_mut().unwrap().pc = *z as usize;
                }
            }
            Op::SpringWennNichtNullish(z) => {
                let v = self.stapel.last().copied().unwrap_or(GVal::Undefiniert);
                if !matches!(v, GVal::Undefiniert | GVal::Null) {
                    self.rahmen.last_mut().unwrap().pc = *z as usize;
                }
            }

            // --- Aufrufe ---
            //
            // J24: **Argumente sind bis zum Aufruf Wurzeln.** GEMESSEN/GEFUNDEN
            // (der Grund fuer die Reihenfolge in den Aufrufbefehlen): Der
            // Stapel wird geleert und die Werte liegen nur noch in einer
            // lokalen Liste; jeder GC in diesem Fenster sammelt sie ein. Der
            // Beweis: `for (let i=0;i<3000;i++) liste.push({a:i, b:[i,i+1]})`
            // lieferte ab Iteration 1206 `liste[1206] === "push"` — der
            // Objekt-Slot (4094) wurde zwischen dem Leeren und dem Aufruf
            // gesweept und von der naechsten Allokation (dem Property-Namen)
            // neu belegt; die Argumente lagen zu dieser Zeit in `args`.
            //
            // Daraus folgt die Reihenfolge: **die Suche passiert, solange der
            // Stapel noch voll ist** (Stapelwerte sind Wurzeln), erst danach
            // wird geleert. Fuer Fenster, in denen der Motor allozieren muss
            // und dabei **kein** Rahmen entsteht, werden die Werte mit
            // `wurzel_merke`/`wurzel_zurueck` gehalten — und **vor** jedem
            // `aufruf_einfuegen` wieder freigegeben, weil der Rahmen seine
            // eigene Marke aus `wurzeln.len()` nimmt (J3, Feld
            // `Rahmen::wurzel_marke`; eine zu grosse Marke wuerde beim
            // Rueckkehr nichts wegraeumen — GEMESSEN: 50.000 Aufrufe mit
            // fruehem `return` aus einem Block ergaben 10,9 MiB statt
            // 273 KiB).
            Op::Aufruf(n) => {
                if self.stapel.len() < *n as usize + 2 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit(
                        "Operandenstapel zu kurz (intern)".to_string(),
                    ));
                }

                let l = self.stapel.len();
                let f = self.stapel[l - 1];
                let this = self.stapel[l - 2];
                let mut args = self.pool_holen();
                args.extend_from_slice(&self.stapel[l - 2 - *n as usize..l - 2]);
                self.stapel.truncate(l - 2 - *n as usize);
                let (f2, t2, a2) = match self.entbinde_vec(f, this, args) {
                    Ok(x) => x,
                    Err(e) => {
                        self.eval_direkt = false;
                        return self.stoerung(e);
                    }
                };
                let r = self.aufruf_einfuegen_vec(f2, t2, a2, false);
                // Das Merk gilt nur fuer DIESEN Aufruf (ein verdecktes `eval`
                // ist eine gewoehnliche Funktion).
                self.eval_direkt = false;
                if let Err(e) = r {
                    return self.stoerung(e);
                }
            }
            Op::AufrufStreu => {
                if self.stapel.len() < 3 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit(
                        "Operandenstapel zu kurz (intern)".to_string(),
                    ));
                }
                // Stapel (unten): feld, this, f
                //
                // GEMESSEN (A5, Orakel w019): `this` und `feld` waren
                // vertauscht gelesen — der Compiler legt erst das Feld
                // (das gespreitete Array), dann `this` ab, der Befehl las
                // aber `this` an der Feldstelle. Folge: `f(...a)` fuerzte
                // `undefined` als Feld auf („Wert ist nicht iterierbar").
                let l = self.stapel.len();
                let f = self.stapel[l - 1];
                let this = self.stapel[l - 2];
                let feld = self.stapel[l - 3];
                self.stapel.truncate(l - 3);
                match self.aufruf_mit_feld(f, feld, this) {
                    Ok(()) => {}
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::AufrufMethode(n) => {
                // Stapel (unten): basis, arg1..argn, schluessel
                if self.stapel.len() < *n as usize + 2 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit(
                        "Operandenstapel zu kurz (intern)".to_string(),
                    ));
                }
                // J24: Suche **vor** dem Leeren (der Stapel ist die Wurzel).
                let l = self.stapel.len();
                let key = self.stapel[l - 1];
                let basis = self.stapel[l - *n as usize - 2];
                let name = self.rechnen_schluessel(key);
                let f = match self.hole(basis, &name) {
                    Ok(f) => f,
                    Err(e) => return self.stoerung(e),
                };
                let mut args = self.pool_holen();
                args.extend_from_slice(&self.stapel[l - *n as usize - 1..l - 1]);
                self.stapel.truncate(l - *n as usize - 2);
                // J5: auch der **Methodenaufruf** muss `entbinde` benutzen.
                //
                // GEMESSEN/GEFUNDEN: `let o = {n:1, g: f.bind({n:9}, 1)};
                // o.g(7)` scheiterte mit „Funktionsindex falsch (intern)" —
                // die gebundene Huelle hat `fn_index = usize::MAX`, und der
                // Methodenpfad gab sie unentbunden an `aufruf_einfuegen`
                // weiter (der einfache Aufrufpfad tat das richtig, daher
                // funktionierte `f.bind(null)(3)`).
                let (f2, t2, a2) = match self.entbinde_vec(f, basis, args) {
                    Ok(x) => x,
                    Err(e) => return self.stoerung(e),
                };
                if let Err(e) = self.aufruf_einfuegen_vec(f2, t2, a2, false) {
                    return self.stoerung(e);
                }
            }
            Op::AufrufMethodeText(n, ti) => {
                // Stapel (unten): basis, arg1..argn
                let n = *n as usize;
                if self.stapel.len() < n + 1 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit(
                        "Operandenstapel zu kurz (intern)".to_string(),
                    ));
                }
                let l = self.stapel.len();
                let basis = self.stapel[l - n - 1];
                let texte = self.texte_arc.clone();
                let name: &str = texte.get(*ti as usize).map(|s| s.as_str()).unwrap_or("");
                if diag_an()
                    && let GVal::Ref(r) = basis
                    && !self.heap.lebt(r)
                {
                    self.diag_totes_objekt(name);
                }
                let f = match self.hole_ic(basis, name, *ti as usize) {
                    Ok(f) => f,
                    Err(e) => return self.stoerung(e),
                };
                let mut args = self.pool_holen();
                args.extend_from_slice(&self.stapel[l - n..l]);
                self.stapel.truncate(l - n - 1);
                let (f2, t2, a2) = match self.entbinde_vec(f, basis, args) {
                    Ok(x) => x,
                    Err(e) => return self.stoerung(e),
                };
                if let Err(e) = self.aufruf_einfuegen_vec(f2, t2, a2, false) {
                    return self.stoerung(e);
                }
            }
            Op::AufrufMethodeStreu => {
                if self.stapel.len() < 3 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit(
                        "Operandenstapel zu kurz (intern)".to_string(),
                    ));
                }
                // Stapel (unten): basis, feld, schluessel
                let l = self.stapel.len();
                let key = self.stapel[l - 1];
                let feld = self.stapel[l - 2];
                let basis = self.stapel[l - 3];
                let name = self.rechnen_schluessel(key);
                let f = match self.hole(basis, &name) {
                    Ok(f) => f,
                    Err(e) => return self.stoerung(e),
                };
                self.stapel.truncate(l - 3);
                if let Err(e) = self.aufruf_mit_feld(f, feld, basis) {
                    return self.stoerung(e);
                }
            }
            Op::AufrufSuperMethode(n) => {
                if self.stapel.len() < *n as usize + 1 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit(
                        "Operandenstapel zu kurz (intern)".to_string(),
                    ));
                }
                // Stapel (unten): arg1..argn, schluessel — `this` kommt aus
                // dem Rahmen, die Suche startet im Heimat-Prototyp.
                let nc = self.stapel.len() - (*n as usize) - 1;
                let name = self.rechnen_schluessel(self.stapel[nc + *n as usize]);
                let this = self.this_heim();
                let f = match self.super_methode(&name) {
                    Ok(f) => f,
                    Err(e) => return self.stoerung(e),
                };
                let args: Vec<GVal> = self.stapel[nc..nc + *n as usize].to_vec();
                self.stapel.truncate(nc);
                if let Err(e) = self.aufruf_einfuegen(f, this, &args, false) {
                    return self.stoerung(e);
                }
            }
            Op::AufrufNeuStreu => {
                let nf = self.stapel.len() - 1;
                let f = self.stapel[nf];
                self.stapel.truncate(nf);
                if let Err(e) = self.neu_mit_feld(f) {
                    return self.stoerung(e);
                }
            }
            Op::AufrufSuper(n) => {
                if self.stapel.len() < *n as usize {
                    return Kontrolle::Fehler(JsFehler::Laufzeit(
                        "Operandenstapel zu kurz (intern)".to_string(),
                    ));
                }
                // Stapel (unten): arg1..argn
                let l = self.stapel.len();
                let args: Vec<GVal> = self.stapel[l - *n as usize..l].to_vec();
                self.stapel.truncate(l - *n as usize);
                // J24: die Argumente sind jetzt nur noch in Rust — als
                // Wurzeln halten (siehe der Kommentar bei `Op::Aufruf`).
                let marke = self.wurzel_merke();
                self.wurzeln.extend_from_slice(&args);
                if let Err(e) = self.super_aufruf(&args) {
                    self.wurzel_zurueck(marke);
                    return self.stoerung(e);
                }
                self.wurzel_zurueck(marke);
            }
            Op::AufrufSuperStreu => {
                if self.stapel.len() < 1 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit(
                        "Operandenstapel zu kurz (intern)".to_string(),
                    ));
                }
                let l = self.stapel.len();
                let feld = self.stapel[l - 1];
                self.stapel.truncate(l - 1);
                match self.feld_werte(feld) {
                    Ok(args) => {
                        if let Err(e) = self.super_aufruf(&args) {
                            return self.stoerung(e);
                        }
                    }
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::SuperLese(i) => {
                let name = self.text_konstante(*i);
                match self.super_lesen(&name) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::SuperLeseDynamisch => {
                let l = self.stapel.len();
                let key = self.stapel[l - 1];
                self.stapel.truncate(l - 1);
                let name = self.rechnen_schluessel(key);
                match self.super_lesen(&name) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::SuperSchreibe(i) => {
                let name = self.text_konstante(*i);
                let wert = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                if let Err(e) = self.super_schreiben(&name, wert) {
                    return self.stoerung(e);
                }
            }
            Op::AufrufNeu(n) => {
                // Stapel (unten): arg1..argn, f
                if self.stapel.len() < *n as usize + 1 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit(
                        "Operandenstapel zu kurz (intern)".to_string(),
                    ));
                }
                let l = self.stapel.len();
                let f = self.stapel[l - 1];
                let args: Vec<GVal> = self.stapel[l - 1 - *n as usize..l - 1].to_vec();
                self.stapel.truncate(l - 1 - *n as usize);
                // J24: `neu_aufruf` legt das `this`-Objekt an (das
                // alloziert) — die Argumente sind jetzt nur noch in Rust,
                // also Wurzeln. `neu_aufruf` selbst gibt sie an den
                // aufgerufenen Rahmen weiter (der hat sie dann in `args`).
                let marke = self.wurzel_merke();
                self.wurzeln.extend_from_slice(&args);
                let r = self.neu_aufruf(f, &args);
                self.wurzel_zurueck(marke);
                if let Err(e) = r {
                    return self.stoerung(e);
                }
            }
            Op::Rueckgabe => {
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                // Release 10.10.2026: ein ABGELEITETER Konstruktor gibt nur
                // ein Objekt oder `undefined` zurueck (ES2023 10.2.2
                // `[[Construct]]` Schritt 10.b/c), sonst TypeError. Vorher
                // wurde jedes Nicht-Objekt still zu `this`. GEMESSEN (node 22
                // in Klammern): `class D extends B { constructor(){ super();
                // return 'x' } } new D()` -> ein D  (TypeError), ebenso
                // `return 5`, `return null`, `return typeof this`.
                // Ein Symbol liegt im Motor als Heap-Slot — es ist trotzdem
                // kein Objekt.
                let kein_objekt = !self.ist_objekt(v)
                    || matches!(v, GVal::Ref(r) if crate::symbole::ist_symbol(self, r));
                if !matches!(v, GVal::Undefiniert) && kein_objekt {
                    let abgeleitet = self
                        .rahmen
                        .last()
                        .is_some_and(|r| r.ist_konstruktor && !r.super_kind && r.eltern_ctor.is_some());
                    if abgeleitet {
                        let e = self.fehler_wert(
                            "TypeError",
                            "Derived constructors may only return object or undefined",
                        );
                        return Kontrolle::Wurf(e);
                    }
                }
                return self.rahmen_zurueck(v);
            }
            Op::RueckgabeVormerken => {
                // J7: `return` **innerhalb** eines `try` mit `finally`: der
                // Wert wird als Aktion vorgemerkt, der `finally`-Block
                // laeuft, und erst sein `FertigEnde` gibt den Wert zurueck.
                // GEMESSEN: vorher sprang `return` direkt zum
                // Funktionsende — `try { return 'a' } finally { return 'b' }`
                // lieferte `a` statt `b`.
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                let r = self.rahmen.last_mut().unwrap();
                r.fertig.push(Aktion::Rueckgabe(v));
            }
            Op::RueckgabeThis => {
                let this = self.this_rahmen();
                let (abgeleitet, gesetzt) = {
                    let r = self.rahmen.last().unwrap();
                    (r.eltern_ctor.is_some(), r.this_gesetzt)
                };
                if abgeleitet && !gesetzt {
                    let e = self.fehler_wert("ReferenceError", "super() im Konstruktor fehlt");
                    return Kontrolle::Wurf(e);
                }
                // J32: `return objekt` in einem Konstruktor gewinnt — das
                // entscheidet hier **nicht** der Stapel.
                //
                // GEMESSEN (der Stapelrat-Versuch war falsch und ist
                // entfernt): `class D{ constructor(){ this.n=1 } }
                // class E extends D { constructor(){ super() } }
                // console.log(new E().n)` warf `TypeError: kein Zugriff
                // auf Undefiniert`, `var a=[9]; a[0] = new E()`warf
                // `kein Setzen auf Primitiv`, und
                // `var r = 'D' + typeof new E()` lieferte
                // `«undefinedobject»` — bei **jedem** `new E()`, das
                // nicht auf oberster Anweisungsebene steht. An der
                // Anweisungsebene war der Stapel leer, der Rat griff also
                // nicht, und deshalb war es dort richtig: die Wirkung war
                // **nicht** die Regel, sondern der Leerstand.
                //
                // WARUM DER RAT FALSCH IST: der Operandenstapel ist
                // **gemeinsam** fuer alle Rahmen. Nach `super()` liegt
                // dort das **Ergebnis des Elternkonstruktors** (so ist es
                // in `super_aufruf` dokumentiert: „sein Ergebnis bleibt
                // liegen"), und in einem verschachtelten Ausdruck der
                // Wert des **umgebenden** Ausdrucks — im Messfall das
                // `console.log`-Ziel, eine `Ref`, und `ist_objekt` gilt
                // fuer eine Funktion. Der Rat hat damit genau einen
                // fremden Wert verschluckt; danach war der Stapel um
                // einen Eintrag zu kurz, und **jeder** folgende Operator
                // las die falsche Operando-Reihenfolge. Aus einem
                // `TypeError` wurde dadurch ein `NaN` — die gefaehrlichere
                // Fehlform, weil sie nicht auffaellt.
                //
                // DIE REGEL STEHT SCHON AN DER RICHTIGEN STELLE:
                // `rahmen_zurueck` macht `if ctor && !super_kind` genau
                // das („`return objekt` gewinnt, sonst `this`"). Ein
                // ausdrueckliches `return x` wird zu `Op::Rueckgabe`, das
                // seinen Wert **poppt** — also ist der Ausdruck
                // `return this` durch `RueckgabeThis` gar nicht
                // erreichbar, und der Stapel muss nicht geraten werden.
                return self.rahmen_zurueck(this);
            }
            Op::RueckgabeUndefined => {
                return self.rahmen_zurueck(GVal::Undefiniert);
            }

            // --- Objekte und Felder ---
            Op::NeuesObjekt => match self.neues_objekt(Some(self.proto_objekt)) {
                Ok(r) => self.schiebe(GVal::Ref(r)),
                Err(e) => return Kontrolle::Fehler(e),
            },
            Op::NeuesFeld => match self.neues_feld() {
                Ok(r) => self.schiebe(GVal::Ref(r)),
                Err(e) => return Kontrolle::Fehler(e),
            },
            Op::FeldAnhaengen => {
                let l = self.stapel.len();
                let v = self.stapel[l - 1];
                let f = self.stapel[l - 2];
                self.stapel.truncate(l - 1);
                if let GVal::Ref(r) = f {
                    if let Err(e) = self.feld_anhang(r, v) {
                        return Kontrolle::Fehler(e);
                    }
                }
            }
            Op::FeldLoch => {
                // `[1,,3]`: der zuletzt angehaengte Index wird zur Luecke
                // (J7) — gelesen wird `undefined`, `i in a` ist falsch.
                let l = self.stapel.len();
                let f = self.stapel[l - 1];
                if let GVal::Ref(r) = f {
                    let anzahl = match self.heap.holen(r) {
                        HObj::Feld { elemente, .. } => elemente.len(),
                        _ => 0,
                    };
                    if anzahl > 0 {
                        let o = self.heap.holen_mut(r);
                        crate::heap::feld_luecke_markieren(o, anzahl - 1);
                    }
                }
            }
            Op::FeldStreu => {
                let l = self.stapel.len();
                let v = self.stapel[l - 1];
                let f = self.stapel[l - 2];
                self.stapel.truncate(l - 1);
                if let GVal::Ref(r) = f {
                    // Release 15.10.2026: `iter_werte` roots its values; they
                    // are in the array afterwards, so the roots go again.
                    let marke = self.wurzel_merke();
                    match self.iter_werte(v) {
                        Ok(werte) => {
                            for w in &werte {
                                if let Err(e) = self.feld_anhang(r, *w) {
                                    self.wurzel_zurueck(marke);
                                    return Kontrolle::Fehler(e);
                                }
                            }
                        }
                        Err(e) => {
                            self.wurzel_zurueck(marke);
                            return self.stoerung(e);
                        }
                    }
                    self.wurzel_zurueck(marke);
                }
            }
            Op::StreuKopieren => {
                let l = self.stapel.len();
                let quelle = self.stapel[l - 1];
                let ziel = self.stapel[l - 2];
                self.stapel.truncate(l - 1);
                if let GVal::Ref(z) = ziel {
                    if let Err(e) = self.streu_kopieren(z, quelle) {
                        return self.stoerung(e);
                    }
                }
            }
            Op::DefEigenschaft(i) => {
                let name = self.text_konstante(*i);
                let l = self.stapel.len();
                if l < 2 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit("Stapel zu kurz".to_string()));
                }
                let wert = self.stapel[l - 1];
                let ziel = self.stapel[l - 2];
                self.stapel.truncate(l - 1);
                // An object literal defines its properties
                // (CreateDataPropertyOrThrow), it does not assign them: a
                // setter on Object.prototype must not run. `__proto__: v`
                // sets the prototype (ES2023 B.3.1).
                if let Err(e) = self.literal_define(ziel, &name, wert, name != "__proto__") {
                    return self.stoerung(e);
                }
            }
            Op::DefEigenschaftNamen => {
                let l = self.stapel.len();
                if l < 3 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit("Stapel zu kurz".to_string()));
                }
                let key = self.stapel[l - 1];
                let wert = self.stapel[l - 2];
                let ziel = self.stapel[l - 3];
                self.stapel.truncate(l - 2);
                let name = self.rechnen_schluessel(key);
                if let Err(e) = self.literal_define(ziel, &name, wert, true) {
                    return self.stoerung(e);
                }
            }
            Op::DefZugriff(i, art) => {
                let name = self.text_konstante(*i);
                let l = self.stapel.len();
                let f = self.stapel[l - 1];
                let ziel = self.stapel[l - 2];
                self.stapel.truncate(l - 1);
                if let GVal::Ref(zr) = ziel
                    && let GVal::Ref(fr) = f
                    && let Err(e) = self.getter_setter_definieren(
                        zr,
                        &name,
                        if *art == 0 { Some(fr) } else { None },
                        if *art == 1 { Some(fr) } else { None },
                        // J19: ein Accessor im **Objektliteral** ist
                        // aufzählbar (Spec OrdinaryObjectLiteral), ein
                        // Accessor einer **Klasse** nicht.
                        true,
                    )
                {
                    return Kontrolle::Fehler(e);
                }
            }
            Op::DefZugriffNamen(art) => {
                let l = self.stapel.len();
                if l < 3 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit("Stapel zu kurz".to_string()));
                }
                let key = self.stapel[l - 1];
                let f = self.stapel[l - 2];
                let ziel = self.stapel[l - 3];
                self.stapel.truncate(l - 2);
                let name = self.rechnen_schluessel(key);
                if let GVal::Ref(zr) = ziel
                    && let GVal::Ref(fr) = f
                    && let Err(e) = self.getter_setter_definieren(
                        zr,
                        &name,
                        if *art == 0 { Some(fr) } else { None },
                        if *art == 1 { Some(fr) } else { None },
                        // J19: ein Accessor im **Objektliteral** ist
                        // aufzählbar (Spec OrdinaryObjectLiteral), ein
                        // Accessor einer **Klasse** nicht.
                        true,
                    )
                {
                    return Kontrolle::Fehler(e);
                }
            }
            Op::LeseEigenschaft(i) => {
                // J8: Name aus dem `Arc` **leihen** (keine Kopie je
                // Zugriff) und den Inline-Cache benutzen.
                let texte = self.texte_arc.clone();
                let name: &str = texte.get(*i as usize).map(|s| s.as_str()).unwrap_or("");
                let l = self.stapel.len();
                let basis = self.stapel[l - 1];
                // Release 09.10.2026 (Diagnose): totes Objekt -> woher kam es?
                if diag_an()
                    && let GVal::Ref(r) = basis
                    && !self.heap.lebt(r)
                {
                    self.diag_totes_objekt(name);
                }
                self.stapel.truncate(l - 1);
                match self.hole_ic(basis, name, *i as usize) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::SchreibeEigenschaft(i) => {
                let texte = self.texte_arc.clone();
                let name = texte.get(*i as usize).map(|s| s.as_str()).unwrap_or("");
                let l = self.stapel.len();
                if l < 2 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit("Stapel zu kurz".to_string()));
                }
                let wert = self.stapel[l - 1];
                let basis = self.stapel[l - 2];
                // Beide Werte bleiben waehrend des Setters GC-Wurzeln.
                if let Err(e) = self.setze(basis, name, wert) {
                    return self.stoerung(e);
                }
                self.stapel.truncate(l - 2);
                self.schiebe(wert);
            }
            Op::LeseIndex => {
                let l = self.stapel.len();
                if l < 2 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit("Stapel zu kurz".to_string()));
                }
                let key = self.stapel[l - 1];
                let basis = self.stapel[l - 2];
                // GEMESSEN: Objekt UND Schluessel werden verbraucht, das
                // ERGEBNIS bleibt. Mit `truncate(l - 1)` blieb das Objekt
                // liegen — bei `console.log(a[1])` stand dann das Array
                // statt des Werts an der Aufrufstelle („Aufrufziel ist
                // keine Funktion": 15 Orakel-Faelle in einem Schlag).
                self.stapel.truncate(l - 2);
                let name = self.rechnen_schluessel(key);
                match self.hole(basis, &name) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::SchreibeIndex => {
                let l = self.stapel.len();
                if l < 3 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit("Stapel zu kurz".to_string()));
                }
                let key = self.stapel[l - 1];
                let wert = self.stapel[l - 2];
                let basis = self.stapel[l - 3];
                let name = self.rechnen_schluessel(key);
                if let Err(e) = self.setze(basis, &name, wert) {
                    return self.stoerung(e);
                }
                self.stapel.truncate(l - 3);
                self.schiebe(wert);
            }
            Op::SchreibeZiel => {
                let l = self.stapel.len();
                if l < 3 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit("Stapel zu kurz".to_string()));
                }
                let key = self.stapel[l - 1];
                let basis = self.stapel[l - 2];
                let wert = self.stapel[l - 3];
                self.stapel.truncate(l - 2);
                let name = self.rechnen_schluessel(key);
                if let Err(e) = self.setze(basis, &name, wert) {
                    return self.stoerung(e);
                }
            }
            Op::SchreibeZielTemp(t, tiefe) => {
                let wert = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                let key = self.temp(*t);
                let li = self.stapel.len();
                let basis = if (*tiefe as usize) < li {
                    self.stapel[li - 1 - *tiefe as usize]
                } else {
                    GVal::Undefiniert
                };
                let name = self.rechnen_schluessel(key);
                if let Err(e) = self.setze(basis, &name, wert) {
                    return self.stoerung(e);
                }
                self.schiebe(wert);
            }
            Op::LeseZielTemp(t, q) => {
                let key = self.temp(*t);
                let li = self.stapel.len();
                let quelle = if (*q as usize) < li {
                    self.stapel[li - 1 - *q as usize]
                } else {
                    GVal::Undefiniert
                };
                let name = self.rechnen_schluessel(key);
                match self.hole(quelle, &name) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::LoescheEigenschaft(i) => {
                let name = self.text_konstante(*i);
                let l = self.stapel.len();
                let basis = self.stapel[l - 1];
                self.stapel.truncate(l - 1);
                match self.loesche(basis, &name) {
                    Ok(v) => self.schiebe(GVal::Bool(v)),
                    Err(e) => return self.stoerung(e),
                }
            }
            /// W13 (JS, 05.10.2026): `delete <Name>`. Der Befehl **laedt den
            /// Wert nicht** — das ist der ganze Unterschied zu `typeof`, wo
            /// ein nicht deklarierter Name `undefined` liefert.
            ///
            /// ES2023 13.5.1.1 Schritt 3 und 6: ist die Referenz
            /// **unaufloesbar**, ist das Ergebnis `true` — **kein**
            /// ReferenceError. Schritt 7/8: sonst `DeleteBinding`
            /// (9.1.1.4.2, siehe `Engine::delete_binding`).
            ///
            /// GEMESSEN (der test262-Fall `language/expressions/grouping/
            /// S11.1.6_A2_T2`): vorher `ReferenceError: 'x' ist nicht
            /// definiert`, jetzt `true`.
            Op::LoescheName(i) => {
                let name = self.name(*i);
                let h = self.name_hash(*i);
                let umg = self.lauf_umg();
                // ES2023 13.5.1.1 Schritt 3 und 6: eine **unaufloesbare**
                // Referenz liefert `true` — **keinen** ReferenceError.
                // Schritt 7/8: sonst `DeleteBinding` (9.1.1.4.2).
                //
                // W15 (JS, 04.10.2026) — **die Aufloesbarkeit hat zwei
                // Quellen**, und das war mein Fehler: ich hatte allein
                // `suche_h` (die Umgebungskette) geprueft. Ein **impliziter
                // Globaler** (`q = 1` ohne `var`) hat aber ueberhaupt **keine**
                // Umgebungsbindung — er ist nur eine **Eigenschaft des
                // globalen Objekts** (8.1.7, der ObjectRecord des
                // GlobalEnvironmentRecord). GEMESSEN, node 22:
                //     `qg=1; delete qg`              -> `true`, danach `undefined`
                // und davor, mit meinem Test nur gegen `suche_h`:
                //     `qg=1; delete qg`              -> `true`, danach **`number`**
                // `delete_binding` wurde in dem Fall nie gerufen — mein Zweig
                // fuer genau diesen Fall war toter Code.
                let aufloesbar = self.suche_h(umg, h, &name).is_some()
                    || self.eigene_lese(self.global_objekt, &name).is_some();
                let geloescht = if aufloesbar {
                    match self.delete_binding(umg, h, &name) {
                        Ok(v) => v,
                        Err(e) => return self.stoerung(e),
                    }
                } else {
                    true
                };
                self.schiebe(GVal::Bool(geloescht));
            }
            Op::LoescheIndex => {
                let l = self.stapel.len();
                if l < 2 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit("Stapel zu kurz".to_string()));
                }
                let key = self.stapel[l - 1];
                let basis = self.stapel[l - 2];
                // J31: **zwei** Elemente abraeumen, nicht eines.
                //
                // GEMESSEN, was hier vorher stand und warum es falsch war:
                // `truncate(l - 1)` laesst die **Basis** auf dem Stapel
                // liegen und schiebt dann das Ergebnis darueber. Bei
                // `var s = delete o[0]` faellt das nicht auf (der Wert
                // steht oben, der Rest wird am Ende der Anweisung
                // verworfen), bei
                //     `console.log(delete o[0])`
                // aber schon: der Aufruf liest `this` und den Aufrufer
                // **unterhalb** der Argumente, und der liegen jetzt ein
                // Wert zu tief. GEMESSENE Meldung:
                //     `Laufzeitfehler: Unbehandelt: TypeError: log is not a
                //      function (Aufrufziel)`
                // und sie nennt `log` auch dann, wenn der Aufrufer `f`
                // heisst — das ist das verschobene `this`.
                //
                // GEMESSEN, dass es **jeder** Index-Zugriff war und nicht
                // nur Arrays: `delete o['a']` an einem Objekt, `delete s[0]`
                // an einer Zeichenkette, `delete o[i]` mit einer Variablen —
                // alle acht mit demselben Fehler. Und `delete o.a` (der
                // Namensweg, `LoescheEigenschaft`) war richtig, weil dort
                // nur **ein** Element abgeraeumt werden muss.
                self.stapel.truncate(l - 2);
                let name = self.rechnen_schluessel(key);
                match self.loesche(basis, &name) {
                    Ok(v) => self.schiebe(GVal::Bool(v)),
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::MusterRest(i) => {
                let l = self.stapel.len();
                let quelle = self.stapel[l - 1];
                self.stapel.truncate(l - 1);
                // Die ausgeschlossenen Schluessel holen (Muster-Tabelle
                // der Funktion); M3: hoechstens `MAX_SAMMLUNG` Namen.
                let mut raus: Vec<String> = Vec::new();
                if let Some(liste) = self
                    .programm
                    .as_ref()
                    .and_then(|p| {
                        p.funktionen
                            .get(self.rahmen.last().map(|r| r.fun).unwrap_or(0))
                    })
                    .and_then(|f| f.muster_aus.get(*i as usize))
                {
                    for t in liste.iter().take(crate::ops::MAX_SAMMLUNG) {
                        raus.push(self.text_konstante(*t));
                    }
                }
                match self.muster_rest(quelle, &raus) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::MusterRestFeld(i) => {
                let l = self.stapel.len();
                let quelle = self.stapel[l - 1];
                self.stapel.truncate(l - 1);
                match self.muster_rest_feld(quelle, *i as usize) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(e),
                }
            }

            // --- Umgebungen ---
            Op::BlockAuf(t) => {
                let alt = self.rahmen.last().unwrap().umg;
                match self.umgebung_neu(Some(alt)) {
                    Ok(neu) => {
                        self.temp_set(*t, GVal::Ref(alt));
                        self.rahmen.last_mut().unwrap().umg = neu;
                        // J3: merken, damit `BlockAb` genau DIESE Umgebung
                        // wieder vom Wurzelstapel nimmt (Begruendung in
                        // `Engine::block_umgebungen`).
                        self.block_umgebungen.push(neu);
                    }
                    Err(e) => return Kontrolle::Fehler(e),
                }
            }
            Op::BlockAb(t) => {
                let alt = self.temp(*t);
                // J3: die Umgebung des Blocks muss vom WURZELSTAPEL
                // wieder runter — `umgebung_neu` hat sie hinaufgegeben.
                //
                // GEMESSEN, warum das der groesste Bremsklotz war: ohne
                // diesen Aufruf blieb jede Blockoeffnung als Wurzel
                // stehen. Der GC markierte dadurch ALLES, konnte NICHTS
                // freigeben, und die Kosten stiegen quadratisch:
                //   fib(18)  211 ns/Schritt   fib(26)  1676 ns/Schritt
                //   Heap nach 20.000 Leer-Schleifen: 4,4 MiB (220 B je
                //   Durchlauf), nach fib(20) bereits 5,8 MiB.
                let neu = self.rahmen.last().unwrap().umg;
                self.umgebung_entfernen(neu);
                // J3: und die Umgebung aus `BlockAuf` (bei einer Schleife
                // ist das eine ANDERE als die gerade geraeumte — eine aus
                // `UmgebungAusTemp`).
                if let Some(bl) = self.block_umgebungen.pop() {
                    self.umgebung_entfernen(bl);
                }
                if let GVal::Ref(r) = alt {
                    self.rahmen.last_mut().unwrap().umg = r;
                }
                self.temp_set(*t, GVal::Undefiniert);
            }
            Op::TempUmgebungSetzen(t) => {
                let umg = self.rahmen.last().unwrap().umg;
                self.temp_set(*t, GVal::Ref(umg));
            }
            Op::UmgebungTempSetzen(t) => {
                // J3b: Rahmenumgebung aus einem Temp (siehe `abbruch`).
                if let GVal::Ref(u) = self.temp(*t) {
                    self.rahmen.last_mut().unwrap().umg = u;
                }
            }
            Op::UmgebungAusTemp(t) => {
                let basis = self.temp(*t);
                if let GVal::Ref(b) = basis {
                    match self.umgebung_neu(Some(b)) {
                        Ok(neu) => {
                            self.rahmen.last_mut().unwrap().umg = neu;
                        }
                        Err(e) => return Kontrolle::Fehler(e),
                    }
                }
            }
            Op::KopiereNamen(quelle, anzahl, start) => {
                if let Err(e) = self.namen_kopieren(*quelle, *anzahl, *start) {
                    return Kontrolle::Fehler(e);
                }
            }
            Op::KopiereZurueck(ziel, anzahl, start) => {
                if let Err(e) = self.namen_kopieren_zurueck(*ziel, *anzahl, *start) {
                    return Kontrolle::Fehler(e);
                }
            }
            Op::TempSet(t) => {
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                self.temp_set(*t, v);
            }
            Op::TempGet(t) => {
                let v = self.temp(*t);
                self.schiebe(v);
            }
            Op::TempLoesche(t) => self.temp_set(*t, GVal::Undefiniert),

            // --- Kontrolle ---
            Op::VersuchStart(i) => {
                self.rahmen.last_mut().unwrap().handler.push(*i as usize);
            }
            Op::VersuchEnde => {
                self.rahmen.last_mut().unwrap().handler.pop();
            }
            Op::FertigVormerken(z) => {
                self.rahmen
                    .last_mut()
                    .unwrap()
                    .fertig
                    .push(Aktion::Springen(*z));
            }
            Op::FertigEnde => {
                let a = match self.rahmen.last_mut() {
                    Some(r) => r.fertig.pop(),
                    None => None,
                };
                match a {
                    Some(Aktion::Springen(z)) => {
                        self.rahmen.last_mut().unwrap().pc = z as usize;
                    }
                    Some(Aktion::Rueckgabe(v)) => return self.rahmen_zurueck(v),
                    Some(Aktion::Wurf(w)) => return Kontrolle::Wurf(w),
                    None => {}
                }
            }
            Op::Wurf => {
                let w = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                return Kontrolle::Wurf(w);
            }

            // --- Vorlagen, Regex, Klassen, Funktionen ---
            Op::Vorlage(n) => {
                let l = self.stapel.len();
                let n = *n as usize;
                if l < n {
                    return Kontrolle::Fehler(JsFehler::Laufzeit("Stapel zu kurz".to_string()));
                }
                let mut s = String::new();
                for i in l - n..l {
                    let v = self.stapel[i];
                    match self.zu_text(v) {
                        Ok(t) => s.push_str(&t),
                        Err(e) => return self.stoerung(e),
                    }
                }
                self.stapel.truncate(l - n);
                if let Err(e) = self.string_grenze(s.len()) {
                    return Kontrolle::Fehler(e);
                }
                match self.text_wert(&s) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return Kontrolle::Fehler(e),
                }
            }
            // Release 15.10.2026: object-literal methods know their object.
            // MEASURED before: `{__proto__: b, m() { return super.x; }}.m()`
            // threw "super ausserhalb einer Methode" (node 22: b.x).
            Op::HeimSetzen => {
                let l = self.stapel.len();
                if l >= 2
                    && let (GVal::Ref(o), GVal::Ref(fr)) = (self.stapel[l - 2], self.stapel[l - 1])
                    && let HObj::Funktion { heim, .. } = self.heap.holen_mut(fr)
                {
                    *heim = Some(o);
                }
            }
            Op::VorlagenObjekt(n) => {
                let n = *n as usize;
                let l = self.stapel.len();
                if l < 2 * n {
                    return Kontrolle::Fehler(JsFehler::Laufzeit("Stapel zu kurz".to_string()));
                }
                let stelle = self.rahmen.last().map(|r| (r.fun, r.pc)).unwrap_or((usize::MAX, 0));
                let werte: Vec<GVal> = self.stapel[l - 2 * n..].to_vec();
                self.stapel.truncate(l - 2 * n);
                match self.vorlagen_objekt(stelle, &werte) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::ZuText => {
                let l = self.stapel.len();
                let v = self.stapel[l - 1];
                let t = match self.zu_text(v) {
                    Ok(t) => t,
                    Err(e) => return self.stoerung(e),
                };
                match self.text_wert(&t) {
                    Ok(nv) => self.stapel[l - 1] = nv,
                    Err(e) => return Kontrolle::Fehler(e),
                }
            }
            Op::Regex(m, s) => {
                let muster = self.text_konstante(*m);
                let schalter = self.text_konstante(*s);
                match self.regex_anlegen(&muster, &schalter) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return Kontrolle::Fehler(e),
                }
            }
            Op::Klasse(i) => {
                let l = self.stapel.len();
                let eltern = self.stapel[l - 1];
                self.stapel.truncate(l - 1);
                match self.klasse_ausfuehren(*i, eltern) {
                    Ok(v) => self.schiebe(v),
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::NeuFunktion(i) => {
                let umg = self.rahmen.last().unwrap().umg;
                match self.closure_anlegen(*i, umg) {
                    Ok(r) => self.schiebe(GVal::Ref(r)),
                    Err(e) => return self.stoerung(e),
                }
            }
            // J44: hier beginnt der Generatorrumpf. Beim **Anlegen** wird
            // hier angehalten (die Parameter sind gebunden, der Rumpf laeuft
            // ab dem ersten `next()`); in jedem anderen Lauf ist der Befehl
            // ein Nichts.
            //
            // GEMESSEN (node 22): `delete Array.prototype[Symbol.iterator];
            // class C { *m([x]){} }  C.prototype.m([1])` -> **TypeError beim
            // Aufruf**. Ohne diese Marke kam er erst beim `next()`.
            Op::GeneratorPrologEnde => {
                if self.generator_startlauf {
                    self.generator_prolog_anhalten();
                    return Kontrolle::Angehalten;
                }
            }
            Op::FelderStart(i) => {
                let this = self.this_rahmen();
                // J12: der Initialisierer ist **lexikalisch** in der
                // Klasse. GEMESSEN vorher: `rahmen_starten` legte die
                // Umgebung ohne Eltern an — ein Feld-Initialisierer sah
                // darum keine Variablen der umgebenden Funktion
                // (`function f(){ let x=1; class C { y = x } … }` →
                // ReferenceError) und auch keinen berechneten
                // Klassennamen (`class B { ['n'] = 1 }`). Der Elternbereich
                // ist die Umgebung des **Konstruktor-Rahmens**; deren
                // Kette fuehrt ueber den Klassen-Scope bis zum Scope, in
                // dem die Klasse entstanden ist.
                let eltern = self.rahmen.last().map(|r| r.umg);
                if let Err(e) = self.rahmen_starten_mit(*i, this, &[], false, eltern) {
                    return self.stoerung(e);
                }
            }

            // J48: `ParamProlog(bool)` markiert den Parameterprolog. Der
            // Wiederaufbau hat **keine** zweite Parameterumgebung — bei
            // einer einfachen Liste ist das korrekt (ES2023 10.2.11 Schritt
            // 27 gibt sie nur bei einer nicht einfachen), bei einer
            // nicht einfachen fehlen die Muster-Ziele. Deshalb **Nichts**
            // statt eines Fehlers: ��er jeder Aufruf mit Parametern.
            // Release 10.10.2026: der Prolog schaltet die Umgebung um
            // (siehe `Rahmen::im_prolog`). GEMESSEN vorher (node 22 in
            // Klammern): `function f(a = (pp = function(){ return x })) {
            // var x = 'inside' } f(); pp()` -> `inside`  (`outside`).
            Op::ParamProlog(an) => {
                if let Some(r) = self.rahmen.last_mut() {
                    if *an {
                        if let Some(p) = r.param_umg
                            && r.prolog_rumpf.is_none()
                        {
                            r.prolog_rumpf = Some(r.umg);
                            r.umg = p;
                        }
                    } else if let Some(rumpf) = r.prolog_rumpf.take() {
                        r.umg = rumpf;
                    }
                }
            }
            // A5: `SpringWennFalsch` **verwirft** den Wert — fuer Anweisungen
            // (`if`, `while`) richtig, im Ausdruck `a && b` aber falsch: das
            // nachfolgende `Pop(1)` fraess dann einen fremden Stapelwert. Der
            // Befehl behaelt den Wert (GEMESSEN: `console.log("ende", a && b
            // ? 1 : 2)` lieferte „Operandenstapel zu kurz", die Argumente
            // waren verschoben).
            Op::SpringWennFalschBehalten(z) => {
                let v = self.stapel.last().copied().unwrap_or(GVal::Undefiniert);
                if !self.zu_bool(v) {
                    self.rahmen.last_mut().unwrap().pc = *z as usize;
                }
            }
            // Release 10.10.2026: `with (objekt) { … }` (ES2023 14.11) — die
            // Befehle waren seit dem Umbau der VM leer („Wiederaufbau: Befehl
            // MitStart fehlt noch"), jedes `with` brach ab. GEMESSEN
            // (spiegel.de): Alpine.js wertet JEDEN Ausdruck in
            // `with (scope) { … }` aus (`new AsyncFunction`) — nichts davon
            // lief, `x-show` blieb ohne Wirkung, Dialoge standen offen.
            Op::MitStart(_) => {
                let o = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                if matches!(o, GVal::Undefiniert | GVal::Null) {
                    let e = self.fehler_wert("TypeError", "Cannot convert undefined or null to object");
                    return Kontrolle::Wurf(e);
                }
                let obj = match self.zu_objekt(o) {
                    Ok(r) => r,
                    Err(e) => return self.stoerung(e),
                };
                let umg = self.rahmen.last().unwrap().umg;
                match self.umgebung_mit_objekt(Some(umg), obj) {
                    Ok(neu) => self.rahmen.last_mut().unwrap().umg = neu,
                    Err(e) => return Kontrolle::Fehler(e),
                }
            }
            Op::MitEnde(t) => {
                if let GVal::Ref(u) = self.temp(*t) {
                    self.rahmen.last_mut().unwrap().umg = u;
                }
            }

            // --- Anhalten und Fortsetzen ---
            Op::Hilfe(k) => match self.hilfe(*k) {
                Ok(v) => self.stapel.push(v),
                Err(Stoerung::Wurf(w)) => return Kontrolle::Wurf(w),
                Err(Stoerung::Fehler(e)) => return Kontrolle::Fehler(e),
            },
            Op::Erwartung => {
                // ## W23 (JS, 05.10.2026) — `await` IST `yield` MIT ANDEREM
                // WARTEVERTRAG
                //
                // Genau wie `Op::Ertrag` (W18/W21): das Fortsetzungs-Merkmal ist
                // `Rahmen::fortgesetzt`, **nicht** `aufnahme.is_some()`. Und es
                // ist **einmalig** — ein Rumpf kann mehrere `await` erreichen.
                //
                // GEMESSEN vorher: `aufnahme.is_some()` ist **immer** wahr, weil
                // `async_fortsetzen` (und `n_generator_schritt`) `aufnahme` bei
                // **jedem** Schritt setzen. Also war schon der **erste**
                // `await` eine Fortsetzung.
                let resuming = self
                    .rahmen
                    .last()
                    .map(|r| r.fortgesetzt)
                    .unwrap_or(false);
                if resuming {
                    if let Some(r) = self.rahmen.last_mut() {
                        r.fortgesetzt = false;
                    }
                    let aufnahme = self.rahmen.last_mut().and_then(|r| r.aufnahme.take());
                    return match aufnahme {
                        // Ein **abgelehntes** `await` ist ein Wurf in den Rumpf:
                        // `try { await p } catch(e)` muss ihn fangen (node 22,
                        // Fall 4: `"sync","gefangen:x"`).
                        Some(Aufnahme::Wurf(w)) => Kontrolle::Wurf(w),
                        Some(Aufnahme::Wert(v)) => {
                            self.schiebe(v);
                            Kontrolle::Weiter
                        }
                        None => {
                            self.schiebe(GVal::Undefiniert);
                            Kontrolle::Weiter
                        }
                    };
                }
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                return self.erwartung(v);
            }
            Op::Ertrag => {
                // GEMESSEN: bei einer FORTSETZUNG liegt auf dem Stapel der
                // Wert des AUFERU FERS (der poppte damals den gestellten
                // Wert schon). Ohne diese Unterscheidung riss das zweite
                // `next()` einen Stapelwert des Aufrufers weg — der Rahmen
                // stand dann eine Stelle zu tief.
                //
                // W18: `aufnahme.is_some()` ist **kein** Fortsetzungs-Merkmal
                // (es wird bei jedem Schritt gesetzt). Richtig ist
                // `Rahmen::fortgesetzt`, gesetzt aus dem Generatorzustand
                // **vor** diesem Schritt (`Start` = „lief noch nie").
                //
                // W19: die Fortsetzung liefert den **gestellten** Wert als
                // Ergebnis des `yield`-Ausdrucks — und haelt **nicht** wieder
                // an. `Aufnahme::Wurf` ist der von `gen.throw(x)` und vom
                // abgelehnten `await`; der gehoert in den Rumpf, nicht auf
                // den Stapel.
                let resuming = self
                    .rahmen
                    .last()
                    .map(|r| r.fortgesetzt)
                    .unwrap_or(false);
                if resuming {
                    // W21 (JS, 05.10.2026): das Merkmal ist **einmalig**.
                    //
                    // `fortgesetzt` wird **einmal pro `n_generator_schritt`**
                    // gesetzt, aber **ein Lauf kann mehrere `yield`
                    // erreichen**. Ohne dieses Zuruecksetzen geriet der
                    // **zweite** `yield` im selben Lauf wieder in den
                    // Fortsetzungszweig, schob den gestellten Wert des
                    // Aufrufers auf den Stapel und **haengte** dann mit
                    // `undefined` an derselben Stelle — also genau das,
                    // was W19 gerade abgeschafft hatte, nur eine Ebene tiefer.
                    //
                    // GEMESSEN (node 22 als Soll in Klammern):
                    //   `yield 1; yield 2` als Schleife  -> `1`  (12)
                    //   `for (i<3) s += yield i`          -> `0`  (3)
                    //
                    // Der zweite Fall ist derselbe Fehler an anderer Stelle:
                    // die **Schleife** laeuft nach dem Zuruecksetzen weiter
                    // und erreicht `Op::Ertrag` erneut — jetzt mit
                    // `fortgesetzt == false`, also suspendiert sie korrekt.
                    if let Some(r) = self.rahmen.last_mut() {
                        r.fortgesetzt = false;
                    }
                    let aufnahme = self.rahmen.last_mut().and_then(|r| r.aufnahme.take());
                    return match aufnahme {
                        Some(Aufnahme::Wurf(w)) => Kontrolle::Wurf(w),
                        Some(Aufnahme::Wert(v)) => {
                            self.schiebe(v);
                            Kontrolle::Weiter
                        }
                        // `next()` ganz ohne Argument: ES2023 27.3.4 Schritt 4
                        // haengt `undefined` an.
                        None => {
                            self.schiebe(GVal::Undefiniert);
                            Kontrolle::Weiter
                        }
                    };
                }
                let v = self.stapel.pop().unwrap_or(GVal::Undefiniert);
                return self.ertrag(v, false);
            }
            Op::ErtragStern => {
                let resuming = self
                    .rahmen
                    .last()
                    .map(|r| r.aufnahme.is_some())
                    .unwrap_or(false);
                let v = if resuming {
                    GVal::Undefiniert
                } else {
                    self.stapel.pop().unwrap_or(GVal::Undefiniert)
                };
                return self.ertrag(v, true);
            }

            // --- Iteration ---
            // J34: siehe `op.rs`. Ein Feld-Muster laeuft normalerweise **indiziert**
            // (`Op::LeseIndex`) — das ist eine Optimierung, die fuer echte
            // Felder und array-ahnliche Quellen (Treffer aus `RegExp.exec`,
            // `arguments`) gilt und die Suite an vielen Stellen braucht.
            //
            // **VERENGT GEMESSEN, weil die breite Fassung 5 Faelle kostete:**
            // Jede andere Quelle ueber `iter_werte` zu schicken ist nicht
            // nur eine andere Reihenfolge, sondern **verliert die
            // Faulheit**. GEMESSEN (Suite):
            //   `Map/iterator-items-are-not-object-close-iterator.js`
            //     -> „Wanduhr-Grenze (3000 ms) ueberschritten": der Motor
            //        sammelt **alle** Werte, ein `break` im Muster beendet
            //        nichts, und `IteratorClose` fehlt (dokumentierter
            //     offener Punkt). Mit dem Materialisieren wird aus einem
            //     `break` eine Endlosschleife bis `MAX_SAMMLUNG`.
            //   `RegExp/named-groups/groups-object.js` (12 Faelle)
            //     -> Trefferfelder wollen den **Index**weg.
            //
            // Deshalb wird **nur** der Generator umgeleitet — die einzige
            // Quelle, bei der der Indexweg **still** `undefined` liefert,
            // statt zu scheitern. GEMESSEN (node 22):
            //   `var n = 0; function* g(){ n += 1; yield 7 } var [a] = g();`
            //     -> `a === 7`, `n === 1` (hier: `undefined`, `n` 0)
            Op::MusterQuelle | Op::MusterQuelleFeld => {
                let l = self.stapel.len();
                let v = self.stapel[l - 1];
                // Release 10.10.2026: ein Feld-Muster faehrt auch Map, Set
                // und Iteratoren (siehe `Op::MusterQuelleFeld`).
                let ist_generator = match v {
                    GVal::Ref(r) => match self.heap.holen(r) {
                        HObj::Generator { .. } => true,
                        HObj::Karte { .. } | HObj::Menge { .. } => matches!(op, Op::MusterQuelleFeld),
                        HObj::Gegenstand { .. } => {
                            matches!(op, Op::MusterQuelleFeld) && self.hat_iterator_methode(r)
                        }
                        _ => false,
                    },
                    _ => false,
                };
                // J36: und derselbe Grund wie in `iter_werte` — ein
                // Muster ueber ein Feld nimmt den Indexweg nur, wenn
                // `@@iterator` der eingebaute ist. GEMESSEN (node 22):
                //   `Array.prototype[Symbol.iterator] = function*(){
                //      yield this[0]; yield this[1]; yield 42 }`
                //   `var [x, y, z] = [1,2,3]` -> `x=1 y=2 z=42`
                // (test262 `private-gen-meth-static-ary-ptrn-elem-id-iter-
                // val-array-prototype.js`; vorher stand dort `z` = `3`.)
                //
                // **NUR** bei einem Generator **oder** bei einem Feld mit
                // ersetztem `@@iterator` wird materialisiert.
                //
                // **GEMESSEN/GEFUNDEN:** die erste Fassung pruefte nur
                // `!eingebaut` — und thereby **jede** Quelle, also auch
                // `({p, ...s} = {p: 9, r: 8})`. Das Objekt ist kein Feld,
                // der Objektweg ist der richtige, und `iter_werte` warf
                // `TypeError: Wert ist nicht iterierbar`. Zwei rote Tests
                // (`sprache.rs::destrukturierung_mit_standard_und_rest`,
                // `sprache.rs::felder_und_objekte`) fanden es.
                //
                // Die Regel ist damit: **materialisieren nur, was der
                // Indexweg nicht kann** — ein Generator (keine `0`, `1`,
                // `2`) und ein Feld, dessen `@@iterator` jemand ersetzt
                // hat. Alles andere nimmt seinen eigenen Weg.
                let ist_feld = match v {
                    GVal::Ref(r) => matches!(self.heap.holen(r), HObj::Feld { .. }),
                    _ => false,
                };
                let materialisieren =
                    ist_generator || (ist_feld && !self.iterator_ist_eingebaut_feld(v));
                if !materialisieren {
                    return Kontrolle::Weiter;
                }
                self.stapel.truncate(l - 1);
                let marke = self.wurzel_merke();
                let werte = match self.iter_werte(v) {
                    Ok(w) => w,
                    Err(e) => {
                        self.wurzel_zurueck(marke);
                        return self.stoerung(e);
                    }
                };
                let f = self.feld_aus_werten(&werte);
                self.wurzel_zurueck(marke);
                match f {
                    Ok(f) => {
                        self.schiebe(GVal::Ref(f));
                    }
                    Err(e) => return self.stoerung(e),
                }
            }
            Op::MusterQuelleLeer => {
                // `[] = q`: **nur** die Iterierbarkeit pruefen. `q` selbst wird
                // nicht veraendert — ES2023 7.4.7 Schritt 3-4 holt nur den
                // Iterator (`GetMethod`), und das Muster iteriert **null** Mal.
                // Insbesondere bleibt ein Generator **unberuehrt**: `Op::
                // MusterQuelle` wuerde ihn per `iter_werte` materialisieren
                // und damit den Rumpf ausfuehren (Seiteneffekt!), obwohl `[]`
                // keinen Wert braucht. (Notiz bei `Op::MusterQuelleLeer`.)
                let l = self.stapel.len();
                if l == 0 {
                    return Kontrolle::Fehler(JsFehler::Laufzeit(
                        "Operandenstapel zu kurz (intern)".to_string(),
                    ));
                }
                let v = self.stapel[l - 1];
                // Die **nativ** iterierbaren Arten brauchen keinen
                // `@@iterator`-Schluessel — `iter_werte` nimmt fuer sie einen
                // Schnellweg. Wichtig: hier wird **nichts gezogen** (das Muster
                // hat null Elemente).
                let nativ_iterierbar = match v {
                    GVal::Ref(r) => matches!(
                        self.heap.holen(r),
                        HObj::Feld { .. }
                            | HObj::Zeichenkette(_)
                            | HObj::Verpackt { .. }
                            | HObj::Karte { .. }
                            | HObj::Menge { .. }
                            | HObj::Generator { .. }
                    ),
                    _ => false,
                };
                if nativ_iterierbar {
                    return Kontrolle::Weiter;
                }
                let sym = match self.n_symbol_bekannt(
                    GVal::Undefiniert,
                    &[],
                    crate::builtin::Nativ::SymIterator,
                ) {
                    Ok(s) => s,
                    Err(e) => return self.stoerung(e),
                };
                let sym_schluessel = match self.schluessel_text(sym) {
                    Ok(s) => s,
                    Err(e) => return self.stoerung(e),
                };
                let meth = match self.hole(v, &sym_schluessel) {
                    Ok(m) => m,
                    Err(e) => return self.stoerung(e),
                };
                if matches!(meth, GVal::Undefiniert | GVal::Null) || !self.ist_aufrufbar(meth) {
                    // Dieselbe Meldung wie `eigenschaften.rs::nicht_iterierbar`
                    // (ES2023 7.4.7 Schritt 4) — die Hilfsfunktion dort ist
                    // privat, deshalb hier direkt gebaut.
                    let e = self.fehler_wert("TypeError", "Wert ist nicht iterierbar");
                    return self.stoerung(Stoerung::Wurf(e));
                }
                return Kontrolle::Weiter;
            }
            Op::IterStart(t, art) => {
                let l = self.stapel.len();
                let v = self.stapel[l - 1];
                self.stapel.truncate(l - 1);
                if let Err(e) = self.iter_starten(v, *art == 1, *t) {
                    return self.stoerung(e);
                }
            }
            Op::IterSchliessen(t) => {
                if let Err(e) = self.iter_schliessen(*t) {
                    return self.stoerung(e);
                }
            }
            Op::IterNaechster(t, ziel) => match self.iter_naechster(*t) {
                IterErgebnis::Wert(v) => self.schiebe(v),
                IterErgebnis::Ende => {
                    self.rahmen.last_mut().unwrap().pc = *ziel as usize;
                }
                IterErgebnis::Angehalten => return Kontrolle::Angehalten,
                IterErgebnis::Fehler(e) => return Kontrolle::Fehler(e),
                IterErgebnis::Wurf(w) => return Kontrolle::Wurf(w),
            },
        }
        Kontrolle::Weiter
    }

    fn schiebe(&mut self, v: GVal) {
        self.stapel.push(v);
    }

    /// Release 09.10.2026: eine leere Liste aus dem Pool (siehe
    /// `Engine::gval_pool`).
    #[inline]
    fn pool_holen(&mut self) -> Vec<GVal> {
        self.gval_pool.pop().unwrap_or_default()
    }

    /// Release 09.10.2026: Liste in den Pool zurueck (begrenzt, M4).
    #[inline]
    fn pool_zurueck(&mut self, mut v: Vec<GVal>) {
        if self.gval_pool.len() < 256 && v.capacity() > 0 && v.capacity() <= 1024 {
            v.clear();
            self.gval_pool.push(v);
        }
    }

    fn stoerung(&mut self, s: Stoerung) -> Kontrolle {
        match s {
            Stoerung::Wurf(w) => Kontrolle::Wurf(w),
            Stoerung::Fehler(e) => Kontrolle::Fehler(e),
        }
    }

    /// Rahmen beenden: Stapel aufraeumen, `Fertig` melden.
    fn rahmen_zurueck(&mut self, v: GVal) -> Kontrolle {
        self.stapel
            .truncate(self.rahmen.last().map(|r| r.basis).unwrap_or(0));
        self.schiebe(v);
        Kontrolle::Fertig
    }

    /// Nameindex -> Name (Namenspool des aktuellen Rahmens).
    /// J3b: der **Hash** des Namens `i` im aktuellen Rahmen.
    ///
    /// GEMESSEN, warum der Hash und nicht der Text: `fn name(i) -> String`
    /// lieferte eine **Kopie** — eine Allokation je `LeseVar`, also
    /// 8 Allokationen je Durchlauf einer `for`-Schleife. Der Hash ist ein
    /// `u64` (Copy, 0 ns) und beim Uebersetzen berechnet. Der Text wird
    /// nur noch im Fehlerfall geholt.
    fn name_hash(&self, i: u32) -> u64 {
        let fun = self.rahmen.last().map(|r| r.fun).unwrap_or(0);
        self.programm
            .as_ref()
            .and_then(|p| p.funktionen.get(fun))
            .and_then(|f| f.namen_hash.get(i as usize).copied())
            .unwrap_or(0)
    }

    /// Release 09.10.2026: der Namenspool der laufenden Funktion (geteilt,
    /// ohne Kopie des Namens).
    fn namen_pool(&self) -> std::sync::Arc<Vec<String>> {
        let fun = self.rahmen.last().map(|r| r.fun).unwrap_or(0);
        self.programm
            .as_ref()
            .and_then(|p| p.funktionen.get(fun))
            .map(|f| f.namen.clone())
            .unwrap_or_default()
    }

    fn name(&self, i: u32) -> String {
        let fun = self.rahmen.last().map(|r| r.fun).unwrap_or(0);
        match &self.programm {
            Some(p) => p
                .funktionen
                .get(fun)
                .and_then(|f| f.namen.get(i as usize))
                .cloned()
                .unwrap_or_default(),
            None => String::new(),
        }
    }

    fn temp(&self, t: u16) -> GVal {
        if t == KEIN_TEMP {
            return GVal::Undefiniert;
        }
        self.rahmen
            .last()
            .and_then(|r| r.temps.get(t as usize).copied())
            .unwrap_or(GVal::Undefiniert)
    }

    fn temp_set(&mut self, t: u16, v: GVal) {
        if t == KEIN_TEMP {
            return;
        }
        if let Some(r) = self.rahmen.last_mut()
            && let Some(s) = r.temps.get_mut(t as usize)
        {
            *s = v;
        }
    }

    /// `this` des aktuellen Rahmens. Pfeile tragen den bei ihrer Erzeugung
    /// gesicherten Empfaenger auch dann, wenn der aeussere Frame beendet ist.
    fn this_rahmen(&self) -> GVal {
        for r in self.rahmen.iter().rev() {
            if self.braucht_this(r) {
                return r.this;
            }
        }
        // J14: **kein** Rahmen braucht `this` — dann ist es der Wert des
        // Hauptlaufs (oben im Stapel). GEMESSEN vorher: der Hauptlauf
        // fuehrt `braucht_this = false`, wenn der Quelltext kein `this`
        // enthaelt; eine Pfeilfunktion am obersten Level fand darum
        // nichts und lieferte `undefined`:
        //   `let o = { n: 3, f: () => typeof this }; o.f()`
        //   mein Motor: `unbestimmt` (das heisst: `this` war undefined)
        //   node 22: `undefined` (das heisst: `this.n` fehlte — `this` war
        //   ein Objekt, naemlich der Modulbereich)
        self.rahmen
            .first()
            .map(|r| r.this)
            .unwrap_or(GVal::Undefiniert)
    }

    fn braucht_this(&self, r: &Rahmen) -> bool {
        self.programm
            .as_ref()
            .and_then(|p| p.funktionen.get(r.fun))
            .map(|f| f.braucht_this || (f.pfeil && r.this_gesetzt))
            .unwrap_or(true)
    }

    /// Heimat-Objekt des naechsten nicht-Pfeil-Rahmens (fuer `super`).
    fn heim_rahmen(&self) -> Option<HeapRef> {
        for r in self.rahmen.iter().rev() {
            if self.braucht_this(r) {
                return r.heim;
            }
        }
        None
    }

    /// `this` des naechsten nicht-Pfeil-Rahmens (fuer `super`, `new`).
    fn this_heim(&self) -> GVal {
        for r in self.rahmen.iter().rev() {
            if self.braucht_this(r) {
                return r.this;
            }
        }
        GVal::Undefiniert
    }
}

// ---------------------------------------------------------------------------
// Umgebungs-Hilfen
// ---------------------------------------------------------------------------

impl Engine {
    /// `anzahl` Namen ab `start` aus `quelle` in die aktuelle Rahmenumgebung.
    fn namen_kopieren(&mut self, quelle: u16, anzahl: u16, start: u32) -> Result<(), JsFehler> {
        let ziel = self.rahmen.last().map(|r| r.umg).unwrap_or(KEIN_REF);
        let namen = self.namen_bereich(start, anzahl);
        for n in namen {
            let v = if quelle == KEIN_TEMP {
                // Aus der gesicherten Eltern-Umgebung (Temp = Ziel der Umgebung).
                self.frame_temp_als_umgebung()
            } else {
                self.temp(quelle)
            };
            if let GVal::Ref(q) = v {
                if let Some(w) = self.lese(q, &self.name(n)).ok() {
                    if self.umgebung_hat(ziel, &self.name(n)) {
                        self.initialisiere(ziel, &self.name(n), w);
                    } else {
                        self.deklariere(ziel, &self.name(n), BindungsArt::Let)?;
                        self.initialisiere(ziel, &self.name(n), w);
                    }
                }
            }
        }
        Ok(())
    }

    /// `anzahl` Namen aus der aktuellen Rahmenumgebung in die im Temp
    /// gespeicherte Umgebung zurueckschreiben.
    fn namen_kopieren_zurueck(
        &mut self,
        ziel: u16,
        anzahl: u16,
        start: u32,
    ) -> Result<(), JsFehler> {
        let von = self.rahmen.last().map(|r| r.umg).unwrap_or(KEIN_REF);
        let nach = self.temp(ziel);
        let zr = match nach {
            GVal::Ref(r) => r,
            _ => return Ok(()),
        };
        let namen = self.namen_bereich(start, anzahl);
        for n in namen {
            if let Ok(w) = self.lese(von, &self.name(n)) {
                self.initialisiere(zr, &self.name(n), w);
            }
        }
        Ok(())
    }

    /// Der Temp, in dem `BlockAuf` die Elternumgebung abgelegt hat: der
    /// Temp, der zur aktuellen Rahmenumgebung gehoert. Gesucht ueber die
    /// Umgebung selbst (der Temp-Wert ist die Elternumgebung).
    fn frame_temp_als_umgebung(&self) -> GVal {
        // Der aktuelle Rahmen weiss, welche Umgebung seine Elternumgebung
        // ist; der Temp mit genau diesem Wert wird benutzt.
        let aktuell = self.rahmen.last().map(|r| GVal::Ref(r.umg));
        if let Some(r) = self.rahmen.last() {
            for v in &r.temps {
                if Some(*v) == aktuell {
                    return GVal::Undefiniert;
                }
            }
        }
        GVal::Undefiniert
    }

    /// Die **Namenindizes** aus der Kopier-Liste der Funktion (fuer die
    /// Pro-Durchlauf-Umgebungen von `for (let …)`).
    ///
    /// J3b: liefert `u16`, NICHT `Vec<String>`. GEMESSEN, warum das der
    /// groesste Brocken der Schleife war: `for (let i = 0; …)` ruft
    /// `KopiereNamen` **zweimal je Durchlauf**, und dabei wurden die
    /// Namen als `String` **geklont** — eine Allokation je Name je
    /// Durchlauf, also 20 Millionen Allokationen bei `Schleife 10^7`.
    fn namen_bereich(&self, start: u32, anzahl: u16) -> Vec<u32> {
        let fun = self.rahmen.last().map(|r| r.fun).unwrap_or(0);
        match &self.programm {
            Some(p) => p
                .funktionen
                .get(fun)
                .map(|f| {
                    f.kopier_namen
                        .iter()
                        .skip(start as usize)
                        .take(anzahl as usize)
                        .copied()
                        .collect()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Der Name zu einem Namensindex **im aktuellen Rahmen** (leer, wenn
    /// es ihn nicht gibt).
    fn name_im_rahmen(&self, idx: u16) -> &str {
        let fun = self.rahmen.last().map(|r| r.fun).unwrap_or(0);
        self.programm
            .as_ref()
            .and_then(|p| p.funktionen.get(fun))
            .and_then(|f| f.namen.get(idx as usize))
            .map(|s| s.as_str())
            .unwrap_or("")
    }
}

// ---------------------------------------------------------------------------
// Aufrufwege
// ---------------------------------------------------------------------------

impl Engine {
    /// Bindungsketten aufloesen (`bind`, Host-Methoden durchreichen).
    pub(crate) fn entbinde(
        &mut self,
        f: GVal,
        this: GVal,
        args: &[GVal],
    ) -> Result<(GVal, GVal, Vec<GVal>), Stoerung> {
        self.entbinde_vec(f, this, args.to_vec())
    }

    /// Release 09.10.2026: wie `entbinde`, aber die Argumentliste wird
    /// **uebernommen** — der haeufige Fall (keine `bind`-Huelle) kopiert sie
    /// nicht noch einmal.
    pub(crate) fn entbinde_vec(
        &mut self,
        f: GVal,
        this: GVal,
        args: Vec<GVal>,
    ) -> Result<(GVal, GVal, Vec<GVal>), Stoerung> {
        let mut ff = f;
        let mut tt = this;
        let mut aa: Vec<GVal> = args;
        if self.host_methode_von_gval(ff).is_some() {
            return Ok((ff, tt, aa));
        }
        // M3: Kettenlaenge gedeckelt.
        for _ in 0..1024 {
            let r = match ff {
                GVal::Ref(r) => r,
                _ => {
                    let e = self.aufrufziel_fehler();
                    return Err(Stoerung::Wurf(e));
                }
            };
            // Pfeil-Closures verwenden dasselbe GC-verfolgte Empfaengerfeld
            // wie bind-Huellen, verweisen in `scope` aber auf eine Umgebung.
            // Ihr lexikalisches `this` ersetzt den Aufruf-Empfaenger, ohne
            // diese Umgebung als naechste Funktion der Bindungskette zu lesen.
            if let HObj::Funktion {
                ist_pfeil: true,
                gebundenes_this: Some(lexikalisch),
                ..
            } = self.heap.holen(r)
            {
                tt = *lexikalisch;
                break;
            }
            let gebunden = match self.heap.holen(r) {
                HObj::Funktion {
                    gebundenes_this: Some(_),
                    ..
                } => true,
                _ => false,
            };
            if !gebunden {
                break;
            }
            let (bt, gb, orig) = match self.heap.holen(r).clone() {
                HObj::Funktion {
                    gebundenes_this,
                    gebundene_args,
                    scope,
                    ..
                } => (gebundenes_this, gebundene_args, scope),
                _ => break,
            };
            tt = bt.unwrap_or(GVal::Undefiniert);
            let mut neu = gb;
            neu.extend(aa.iter().copied());
            aa = neu;
            match orig {
                Some(o) => ff = GVal::Ref(o),
                None => {
                    let e = self.fehler_wert("TypeError", "Bindung korrupt (intern)");
                    return Err(Stoerung::Wurf(e));
                }
            }
        }
        Ok((ff, tt, aa))
    }

    /// Aufruf: Nativ direkt, Benutzerfunktion als Rahmen, async/generator
    /// als Objekt.
    pub(crate) fn aufruf_einfuegen(
        &mut self,
        f: GVal,
        this: GVal,
        args: &[GVal],
        als_konstruktor: bool,
    ) -> Result<(), Stoerung> {
        self.aufruf_einfuegen_vec(f, this, args.to_vec(), als_konstruktor)
    }

    /// Release 09.10.2026: wie `aufruf_einfuegen`, die Argumentliste geht in
    /// den Rahmen ueber (vorher je Aufruf eine weitere Kopie).
    pub(crate) fn aufruf_einfuegen_vec(
        &mut self,
        f: GVal,
        this: GVal,
        args: Vec<GVal>,
        als_konstruktor: bool,
    ) -> Result<(), Stoerung> {
        // Call operands have already left the operand stack. Keep them alive
        // until a frame, native-call guard or host registry owns them. Frame
        // creation itself allocates environments and may collect the heap.
        self.frisch.push(f);
        self.frisch.push(this);
        self.frisch.extend_from_slice(&args);
        let r = match f {
            GVal::Ref(r) => r,
            _ => {
                let e = self.aufrufziel_fehler();
                return Err(Stoerung::Wurf(e));
            }
        };
        // Host-Methode (`el.classList.add(...)`).
        if let Some((id, methode)) = self.host_methode_von_gval(f)
            && let Some(h) = self.host_objekte.clone()
        {
            // Release 10.10.2026: Lauscher-Objekte (siehe `lauscher_huelle`).
            let args = if args.len() >= 2 && self.ist_objekt(args[1]) && !self.ist_aufrufbar(args[1]) {
                let name = self.host_methoden_name(f).unwrap_or_else(|| methode.clone());
                self.lauscher_huelle(&name, args)?
            } else {
                args
            };
            let mut args_w: Vec<crate::Wert> = Vec::with_capacity(args.len().min(16));
            for a in args.iter().take(16) {
                args_w.push(self.host_argument(*a));
            }
            return match h.rufen(id, &methode, &args_w) {
                Ok(w) => {
                    let v = self.gval_aus_wert(w);
                    self.schiebe(v);
                    Ok(())
                }
                Err(msg) => {
                    let kind = match msg.as_str() {
                        "NamespaceError" => "NamespaceError",
                        "InvalidCharacterError" => "InvalidCharacterError",
                        _ => "TypeError",
                    };
                    let e = self.fehler_wert(kind, &msg);
                    Err(Stoerung::Wurf(e))
                }
            };
        }
        // J8: die Nativ-Tabelle ist ein `HashMap<HeapRef, Nativ>` — der
        // Zugriff kostete bei JEDEM Aufruf auch dann einen Hash, wenn die
        // Zielklar kein `HObj::Nativ` ist (also bei jedem Benutzeraufruf).
        // Deshalb erst die Art pruefen (der Heap-Slot traegt sie bereits).
        // Release 10.10.2026: `[[Call]]` eines Proxys (`proxy.rs`).
        if matches!(self.heap.holen(r), HObj::Proxy { .. }) {
            let v = self.proxy_anwenden(r, this, &args)?;
            self.schiebe(v);
            return Ok(());
        }
        let ist_nativ = matches!(self.heap.holen(r), HObj::Nativ { .. });
        if ist_nativ && let Some(nativ) = self.native.get(&r).cloned() {
            let v = self.rufe_nativ(nativ, r, this, &args, als_konstruktor)?;
            self.schiebe(v);
            return Ok(());
        }
        let fn_index = match self.heap.holen(r) {
            HObj::Funktion { fn_index, .. } => *fn_index,
            _ => {
                let e = self.aufrufziel_fehler();
                return Err(Stoerung::Wurf(e));
            }
        };
        // Release 09.10.2026: eine `bind`-Huelle (`fn_index == usize::MAX`)
        // hier auspacken — nicht nur in `Op::Aufruf`/`Op::AufrufMethode`.
        // GEMESSEN (node 22): `function f(a){return this.x+a}; var g =
        // f.bind({x:1}); g.apply(null,[2])` -> `3`, ebenso `g.call`,
        // `[1].map(g)`, `Reflect.apply(g, …)`; der Motor brach jeden dieser
        // Wege mit „Funktionsindex falsch (intern)" ab (Laufzeitfehler,
        // nicht fangbar) — auf youtube.com mitten im Aufbau von `ytd-app`.
        if fn_index == usize::MAX {
            let (f2, t2, a2) = self.entbinde_vec(f, this, args)?;
            if f2 == f {
                let e = self.aufrufziel_fehler();
                return Err(Stoerung::Wurf(e));
            }
            return self.aufruf_einfuegen_vec(f2, t2, a2, als_konstruktor);
        }
        let f_def = self.aufruf_kopf(fn_index).map_err(Stoerung::Fehler)?;
        if f_def.pfeil && als_konstruktor {
            let e = self.fehler_wert("TypeError", "Pfeilfunktion ist kein Konstruktor");
            return Err(Stoerung::Wurf(e));
        }
        // Release 10.10.2026: ein ASYNCHRONER Generator — der Generator
        // startet wie jeder andere, der Treiber macht daraus das
        // AsyncGenerator-Objekt (siehe `asyncgen.rs`).
        if f_def.asynchron && f_def.generator {
            let g = self.generator_starten(r, fn_index, this, &args)?;
            // Der Generator ist WURZEL, bevor der Vorspann (beim ersten Mal
            // uebersetzt — viele Allokationen) geholt wird. GEMESSEN unter
            // `NARSIL_JS_GC_STRESS`: sonst sammelte die GC ihn, und der
            // Treiber hielt einen fremden Slot (`gen.next` undefined).
            let marke = self.wurzel_merke();
            self.wurzeln.push(g);
            let treiber = match self.hilfe(crate::asyncgen::HILFE_ASYNC_GEN) {
                Ok(t) => t,
                Err(e) => {
                    self.wurzel_zurueck(marke);
                    return Err(e);
                }
            };
            // Release 15.10.2026: OrdinaryCreateFromConstructor(F,
            // %AsyncGeneratorPrototype%): the object inherits from
            // `F.prototype` when that is an object.
            let proto = match self.eigene_lese(r, "prototype") {
                Some(GVal::Ref(p)) if self.ist_objekt(GVal::Ref(p)) => GVal::Ref(p),
                _ => self.es.async_gen_proto.map(GVal::Ref).unwrap_or(GVal::Null),
            };
            let v = self.aufruf_synchron(treiber, GVal::Undefiniert, &[g, proto], false);
            self.wurzel_zurueck(marke);
            self.schiebe(v?);
            return Ok(());
        }
        if f_def.asynchron {
            let v = self.async_starten(r, fn_index, this, &args)?;
            self.schiebe(v);
            return Ok(());
        }
        if f_def.generator {
            let v = self.generator_starten(r, fn_index, this, &args)?;
            self.schiebe(v);
            return Ok(());
        }
        // J14: **sloppy**-`this` (sec-10.1.6 / OrdinaryCallBindThis). Beim
        // Aufruf ohne Empfaenger ist `this` im strikten Modus `undefined`,
        // im lockeren der globale Bereich. GEMESSEN vorher (Top-Level-
        // Skript, kein `"use strict"` — also sloppy nach Spec):
        //   `function f(){ return this === globalThis } f()` -> `false`
        //   node 22: `true`
        //   `function g(){ return typeof this } g()` -> `undefined`
        //   node 22: `object`
        // Das trifft echte Seitenskripte, die `this` in einer freien
        // Funktion lesen (`function(){ return this.tagName }`).
        //
        // Eine Methode (`o.m()`) und ein Konstruktor (`new`) uebergeben
        // ihr `this` ausdruecklich — sie sind hier nicht betroffen, weil
        // `this != Undefiniert`. Arrowfunktionen bekommen keinen Rahmen
        // und erben `this` (siehe `Op::This`).
        //
        // GEMESSEN, warum die Lage der **aufgerufenen** Funktion zaehlt
        // und nicht die des aufrufenden Rahmens: die Direktive
        // `"use strict"` steht im Rumpf der Funktion, und der Rahmen
        // entsteht erst einen Schritt spaeter. Deshalb wird hier der
        // Index benutzt (ein Vec-Zugriff, keine Allokation):
        //   `function f(){ 'use strict'; return this } f()` -> `undefined`
        let this = if matches!(this, GVal::Undefiniert) && !self.strikt {
            let strikt_funktion = self
                .programm
                .as_ref()
                .and_then(|p| p.funktionen.get(fn_index))
                .map(|f| f.strikt)
                .unwrap_or(false);
            if strikt_funktion {
                this
            } else {
                GVal::Ref(self.global_objekt)
            }
        } else {
            this
        };
        // J8: `args` wird **verschieben** statt kopiert — vorher
        // kopierte `rahmen_starten_fn` die Liste ein zweites Mal
        // (`args: args.to_vec()`), also zwei Allokationen je Aufruf.
        self.rahmen_starten_fn_kopf(r, fn_index, this, args, als_konstruktor, f_def)
    }

    /// J14: ist der **aktuelle** Rahmen strikt? (Direktive der Funktion
    /// oder striktes Programm; der Hauptlauf zaehlt als Rahmen.)
    pub(crate) fn rahmen_strikt(&self) -> bool {
        match self.rahmen.last() {
            Some(r) => r.strikt,
            None => self.strikt,
        }
    }

    /// Rahmen fuer eine Benutzerfunktion aufsetzen (kein Rust-Rekursionspfad).
    fn rahmen_starten_fn(
        &mut self,
        fr: HeapRef,
        fn_index: usize,
        this: GVal,
        // J8: als `Vec` uebergeben, damit der Rahmen sie uebernimmt statt
        // noch einmal zu kopieren.
        args: Vec<GVal>,
        als_konstruktor: bool,
    ) -> Result<(), Stoerung> {
        let f_def = self.aufruf_kopf(fn_index).map_err(Stoerung::Fehler)?;
        self.rahmen_starten_fn_kopf(fr, fn_index, this, args, als_konstruktor, f_def)
    }

    /// Release 09.10.2026: wie `rahmen_starten_fn`, mit schon geholtem Kopf
    /// (`aufruf_einfuegen_vec` hatte ihn bereits — vorher zweimal je Aufruf).
    fn rahmen_starten_fn_kopf(
        &mut self,
        fr: HeapRef,
        fn_index: usize,
        this: GVal,
        args: Vec<GVal>,
        als_konstruktor: bool,
        f_def: crate::op::AufrufKopf,
    ) -> Result<(), Stoerung> {
        // Release 09.10.2026 (Diagnose, `NARSIL_JS_DIAG`): ein Hauptlauf als
        // Funktion gerufen ist fast immer ein falscher Funktionsindex.
        if diag_an()
            && self.programm.as_ref().is_some_and(|p| p.funktionen.get(fn_index).is_some_and(|f| f.name == "<main>"))
        {
            let spur: Vec<String> = self.rahmen.iter().rev().take(6).map(|r| {
                let n = self.programm.as_ref().and_then(|p| p.funktionen.get(r.fun)).map(|f| f.name.clone()).unwrap_or_default();
                format!("{n}:{}@pc{}", r.zeile, r.pc)
            }).collect();
            eprintln!("NARSIL_JS_DIAG Hauptlauf {fn_index} als Funktion gerufen: {}", spur.join(" <- "));
        }
        if self.rahmen.len() >= self.grenzen.max_aufruftiefe as usize {
            let e = self.fehler_wert("RangeError", "Maximum call stack size exceeded");
            return Err(Stoerung::Wurf(e));
        }
        // J8: Kopfkopf statt voller `Funktion` (Begruendung bei
        // `op::AufrufKopf`) und den Heap-Slot **ohne** Klon lesen — der
        // Klon kopierte die Property-Map der Funktion.
        let (scope, heim, eltern_ctor, lexikalisches_this, klasse) = match self.heap.holen(fr) {
            HObj::Funktion {
                scope,
                heim,
                eltern_ctor,
                gebundenes_this,
                ist_konstruktor,
                ..
            } => (
                *scope,
                *heim,
                *eltern_ctor,
                if f_def.pfeil { *gebundenes_this } else { None },
                *ist_konstruktor,
            ),
            _ => {
                let e = self.fehler_wert("TypeError", "Funktion ohne Scope (intern)");
                return Err(Stoerung::Wurf(e));
            }
        };
        // Release 15.10.2026: `ist_konstruktor` is set only on class
        // constructors (`klasse_ausfuehren`); [[Call]] on them throws.
        if klasse && !als_konstruktor {
            return Err(self.klassen_aufruf_fehler(fr));
        }
        // J3: die Marke MUSS vor `umgebung_neu` liegen, sonst gehoert die
        // Umgebung des Aufrufs nicht zum Rahmen (sie waere dann eine
        // Wurzel des AUSSEREN Rahmens und wuerde beim Rueckkehr des
        // inneren mit verschwinden, waehrend der aeussere noch laeuft).
        let wurzel_marke = self.wurzel_merke();
        // J48: bei **nicht einfacher** Parameterliste zwei Umgebungen — die
        // der Parameter und die des Rumpfs (ES2023 10.2.11 Schritt 27).
        //
        // GEMESSEN (node 22), **fuenf** Faelle, die alle **dieselbe**
        // Ursache haben und **alle** mit einer einzigen Umgebung falsch
        // werden (`inside` statt `outside`):
        //   `function f(a = (pp = function(){ return x; })) { var x = 'i' }`
        //   `function f(a = …) { let x = 'i' }`   (Muster: der `let`-Fall)
        //   `(a = …) => { var x = 'i' }`          (Pfeil)
        //   `{ m(a = …) { var x = 'i' } }`        (Methode)
        //   `class { static set a(_ = …) { var x = 'i' } }` (Setter, der
        //     Anlass der Suite-Datei
        //     `class/scope-static-setter-paramsbody-var-open.js`)
        //
        // Bei **einfacher** Liste bleibt es bei einer Umgebung: dort ist
        // zusatzlich `arguments` nach Spec nur dann nicht vorhanden, wenn der
        // Rumpf es selbst deklariert (`arguments-with-arguments-fn.js`).
        let param_umg = if !f_def.param_einfach {
            let p = self.umgebung_neu(scope).map_err(Stoerung::Fehler)?;
            Some(p)
        } else {
            None
        };
        let umg = match param_umg {
            Some(p) => self.umgebung_neu(Some(p)).map_err(Stoerung::Fehler)?,
            // Release 09.10.2026: Platz fuer `var`s und Parameter vorab (nur
            // wenn es welche gibt — eine leere Liste kostet nichts).
            None => self
                .umgebung_neu_kap(scope, f_def.var_hashes.len() + f_def.parameter as usize)
                .map_err(Stoerung::Fehler)?,
        };
        // Benannter Funktionsausdruck: der Name liegt im EIGENEN Rahmen-Scope
        // (JS-Regel, sonst kann sich die Funktion nicht selbst aufrufen).
        // J48: bei nicht einfacher Liste in der **Parameterumgebung** — ein
        // Ausdruck in der Parameterliste soll den Namen sehen duerfen
        // (ES2023 10.2.11 Schritt 22: `env`).
        let name_umg = param_umg.unwrap_or(umg);
        if let Some(n) = &f_def.selbst_name {
            let _ = self.deklariere(name_umg, n, BindungsArt::Let);
            self.initialisiere(name_umg, n, GVal::Ref(fr));
        }
        // Release 09.10.2026: nur nach dem Hash (siehe `Funktion::var_hashes`).
        for &h in f_def.var_hashes.iter() {
            if let Err(e) = self.deklariere_h(umg, h, BindungsArt::Var) {
                self.umgebung_entfernen(umg);
                return Err(Stoerung::Fehler(e));
            }
        }
        let basis = self.stapel.len();
        let rahmen_id = self.rahmen_id_holen();
        // Release 09.10.2026: Temps aus dem Pool (siehe `pool_holen`).
        let mut temps = self.pool_holen();
        temps.resize(f_def.temps as usize, GVal::Undefiniert);
        self.rahmen.push(Rahmen {
            // BILD_29 (Befreiung fuer QUIC): `Rahmen` hat das Feld `fortgesetzt`
            // („der Generator lief schon vor diesem Schritt"). Beim **Anlegen**
            // einer Frame hat er noch nicht gelaufen (`Start`) — also `false`.
            // Ohne diese Zeilen ist `narsil-jsvm` nicht baubar und damit **kein**
            // Test in `narsil` pruefbar. Der Wert steht an der Semantik, nicht
            // geraten.
            fortgesetzt: false,
            wurzel_marke,
            id: rahmen_id,
            fun: fn_index,
            // J28: die Startzeile der Funktion (siehe das Feld `zeile`).
            zeile: self.funktions_zeile(fn_index),
            code: f_def.code.clone(),
            umg,
            param_umg,
            prolog_rumpf: None,
            basis,
            this: if let Some(lexikalisch) = lexikalisches_this {
                lexikalisch
            } else if f_def.braucht_this {
                this
            } else {
                GVal::Undefiniert
            },
            // J14: Direktive der Funktion ODER striktes Programm.
            strikt: f_def.strikt || self.strikt,
            heim,
            eltern_ctor,
            handler: Vec::new(),
            fertig: Vec::new(),
            temps,
            args,
            neu: if als_konstruktor {
                GVal::Ref(fr)
            } else if f_def.pfeil {
                // W14 (JS, 04.10.2026): eine **Pfeilfunktion** hat kein eigenes
                // `[[NewTarget]]` — ES2023 15.2.1ArrowFunction: „Perform
                // CopyDataProperties(closure, F, [[Environment]], ...)" und
                // `MakeMethod`/`[[NewTarget]]` bleiben vom umgebenden
                // Constructor. Also wird der **geerbt**, sonst gaebe es
                //   `function F(){ return (()=>new.target)() }  new F()`
                //   `undefined` statt `F`.
                //
                // GEMESSEN (node 22 als Sollwert, siehe Bericht JS_11).
                self.rahmen
                    .last()
                    .map(|r| r.neu.clone())
                    .unwrap_or(GVal::Undefiniert)
            } else {
                GVal::Undefiniert
            },
            ist_konstruktor: als_konstruktor,
            this_gesetzt: if f_def.pfeil {
                lexikalisches_this.is_some()
            } else {
                !als_konstruktor
            },
            aufnahme: None,
            ertrag_stern: None,
            // W17: `rahmen_starten_fn` — eine async-Funktion traegt das
            // Versprechen, aber `async_starten` (der Aufrufer) setzt es.
            generator: None,
            super_kind: false,
            iter_lauf: Vec::new(),
            stapel: Vec::new(),
            pc: 0,
        });
        Ok(())
    }

    /// Rahmen fuer eine beliebige kompilierte Funktion starten
    /// (Feld-Initialisierer, statische Initialisierer).
    pub(crate) fn rahmen_starten(
        &mut self,
        idx: u32,
        this: GVal,
        args: &[GVal],
        als_konstruktor: bool,
    ) -> Result<(), Stoerung> {
        self.rahmen_starten_mit(idx, this, args, als_konstruktor, None)
    }

    /// Wie `rahmen_starten`, aber mit Elternumgebung fuer den Scope.
    ///
    /// GEMESSEN: der Initialisierer der statischen Felder muss den
    /// KLASSENNAMEN sehen duerfen (`static b = Z.a + 1`) — ohne
    /// Elternumgebung war `Z` undefiniert (ReferenceError).
    pub(crate) fn rahmen_starten_mit(
        &mut self,
        idx: u32,
        this: GVal,
        args: &[GVal],
        als_konstruktor: bool,
        eltern: Option<HeapRef>,
    ) -> Result<(), Stoerung> {
        let fnr = idx as usize;
        if self.rahmen.len() >= self.grenzen.max_aufruftiefe as usize {
            let e = self.fehler_wert("RangeError", "Maximum call stack size exceeded");
            return Err(Stoerung::Wurf(e));
        }
        let f = self.funktion_von(fnr).map_err(Stoerung::Fehler)?;
        let wurzel_marke = self.wurzel_merke();
        let umg = self.umgebung_neu(eltern).map_err(Stoerung::Fehler)?;
        for &h in f.var_hashes.iter() {
            let _ = self.deklariere_h(umg, h, BindungsArt::Var);
        }
        let basis = self.stapel.len();
        let rahmen_id = self.rahmen_id_holen();
        self.rahmen.push(Rahmen {
            // BILD_29 (Befreiung fuer QUIC): `Rahmen` hat das Feld `fortgesetzt`
            // („der Generator lief schon vor diesem Schritt"). Beim **Anlegen**
            // einer Frame hat er noch nicht gelaufen (`Start`) — also `false`.
            // Ohne diese Zeilen ist `narsil-jsvm` nicht baubar und damit **kein**
            // Test in `narsil` pruefbar. Der Wert steht an der Semantik, nicht
            // geraten.
            fortgesetzt: false,
            wurzel_marke,
            id: rahmen_id,
            fun: fnr,
            // J28: die Startzeile der Funktion (siehe das Feld `zeile`).
            zeile: self.funktions_zeile(fnr),
            code: f.code.clone(),
            umg,
            // J48: ein Initialisierer hat keine Parameterliste.
            param_umg: None,
            prolog_rumpf: None,
            basis,
            this,
            // J14: dieselbe Lage wie der aufrufende Rahmen.
            strikt: self.strikt,
            heim: None,
            eltern_ctor: None,
            handler: Vec::new(),
            fertig: Vec::new(),
            temps: vec![GVal::Undefiniert; f.temps as usize],
            args: args.to_vec(),
            neu: GVal::Undefiniert,
            ist_konstruktor: als_konstruktor,
            this_gesetzt: true,
            aufnahme: None,
            ertrag_stern: None,
            // W17: siehe `rahmen_starten_fn`.
            generator: None,
            super_kind: false,
            iter_lauf: Vec::new(),
            stapel: Vec::new(),
            pc: 0,
        });
        Ok(())
    }

    /// Aufruf mit einer Argumentliste als Feld (`f(...args)`).
    fn aufruf_mit_feld(&mut self, f: GVal, feld: GVal, this: GVal) -> Result<(), Stoerung> {
        let args = self.feld_werte(feld)?;
        let (f2, t2, a2) = self.entbinde(f, this, &args)?;
        self.aufruf_einfuegen(f2, t2, &a2, false)
    }

    /// Feld in seine Elemente zerlegen (Spread-Argumente).
    fn feld_werte(&mut self, v: GVal) -> Result<Vec<GVal>, Stoerung> {
        self.iter_werte(v)
    }

    /// `new F(...)`.
    fn neu_aufruf(&mut self, f: GVal, args: &[GVal]) -> Result<(), Stoerung> {
        let mut ff = f;
        let mut aa: Vec<GVal> = args.to_vec();
        for _ in 0..1024 {
            let r = match ff {
                GVal::Ref(r) => r,
                _ => {
                    let e = self.fehler_wert("TypeError", "new-Ziel ist kein Konstruktor");
                    return Err(Stoerung::Wurf(e));
                }
            };
            let obj = match self.heap.holen(r) {
                HObj::Funktion {
                    gebundenes_this: Some(_),
                    ist_pfeil: false,
                    ..
                } => true,
                _ => false,
            };
            if !obj {
                break;
            }
            let (geb, orig) = match self.heap.holen(r).clone() {
                HObj::Funktion {
                    gebundene_args,
                    scope,
                    ..
                } => (gebundene_args, scope),
                _ => break,
            };
            let mut neu = geb;
            neu.extend(aa.iter().copied());
            aa = neu;
            match orig {
                Some(o) => ff = GVal::Ref(o),
                None => break,
            }
        }
        let fr = match ff {
            GVal::Ref(r) => r,
            _ => {
                let e = self.fehler_wert("TypeError", "new-Ziel ist kein Konstruktor");
                return Err(Stoerung::Wurf(e));
            }
        };
        // Release 10.10.2026: `[[Construct]]` eines Proxys (`proxy.rs`).
        if matches!(self.heap.holen(fr), HObj::Proxy { .. }) {
            let v = self.proxy_konstruieren(fr, &aa, GVal::Ref(fr))?;
            self.schiebe(v);
            return Ok(());
        }
        if let Some(nativ) = self.native.get(&fr).cloned() {
            let v = self.rufe_nativ(nativ, fr, GVal::Undefiniert, &aa, true)?;
            self.schiebe(v);
            return Ok(());
        }
        let fn_index = match self.heap.holen(fr) {
            HObj::Funktion { fn_index, .. } => *fn_index,
            _ => {
                let e = self.fehler_wert("TypeError", "new-Ziel ist kein Konstruktor");
                return Err(Stoerung::Wurf(e));
            }
        };
        let f_def = self.funktion_von(fn_index).map_err(Stoerung::Fehler)?;
        if f_def.pfeil {
            let e = self.fehler_wert("TypeError", "Pfeilfunktion ist kein Konstruktor");
            return Err(Stoerung::Wurf(e));
        }
        // `this` = neues Objekt mit `F.prototype` als Prototyp.
        let proto = self.konstruktor_proto(fr);
        let obj = match self.neues_objekt(Some(proto)) {
            Ok(o) => o,
            Err(e) => return Err(Stoerung::Fehler(e)),
        };
        if f_def.asynchron {
            let v = self.async_starten(fr, fn_index, GVal::Ref(obj), &aa)?;
            self.schiebe(v);
            return Ok(());
        }
        if f_def.generator {
            let e = self.fehler_wert("TypeError", "Generator ist kein Konstruktor");
            return Err(Stoerung::Wurf(e));
        }
        self.rahmen_starten_fn(fr, fn_index, GVal::Ref(obj), aa.clone(), true)?;
        // `this_gesetzt` erst durch `super()` (siehe Rahmen_starten_fn).
        if let Some(r) = self.rahmen.last_mut() {
            r.this_gesetzt = false;
        }
        Ok(())
    }

    fn neu_mit_feld(&mut self, f: GVal) -> Result<(), Stoerung> {
        // `new F(...args)`: die Argumentliste steht als Feld ueber `f`.
        let l = self.stapel.len();
        let feld = self.stapel[l - 1];
        self.stapel.truncate(l - 1);
        let args = self.feld_werte(feld)?;
        self.neu_aufruf(f, &args)
    }

    /// `super(...)` im Konstruktor: Elternkonstruktor mit demselben `this`.
    /// Release 10.10.2026: der Rahmen, der `this` bindet — der naechste
    /// Nicht-Pfeil von oben (ein Pfeil im Konstruktor ruft `super()` fuer
    /// den KONSTRUKTOR, ES2023 13.3.7.1: GetThisEnvironment).
    fn this_rahmen_index(&self) -> Option<usize> {
        (0..self.rahmen.len()).rev().find(|&i| self.braucht_this(&self.rahmen[i]))
    }

    fn super_aufruf(&mut self, args: &[GVal]) -> Result<(), Stoerung> {
        let (eltern, this, neu) = {
            let r = match self.this_rahmen_index().and_then(|i| self.rahmen.get(i)) {
                Some(r) => r,
                None => {
                    let e = self.fehler_wert("SyntaxError", "super() ausserhalb des Konstruktors");
                    return Err(Stoerung::Wurf(e));
                }
            };
            (r.eltern_ctor, r.this, r.neu.clone())
        };
        let eltern = match eltern {
            Some(e) => e,
            None => {
                let e = self.fehler_wert("SyntaxError", "super() in Basiskonstruktor");
                return Err(Stoerung::Wurf(e));
            }
        };
        // Doppeltes `super()`: `this` ist dann schon belegt.
        let ti = self.this_rahmen_index();
        if ti.and_then(|i| self.rahmen.get(i)).is_some_and(|r| r.this_gesetzt) {
            let e = self.fehler_wert("ReferenceError", "super() zweimal");
            return Err(Stoerung::Wurf(e));
        }
        // `this` in die Eltern-Umgebung schieben (deren Rumpf sieht es als
        // sein eigenes `this`).
        if let Some(r) = ti.and_then(|i| self.rahmen.get_mut(i)) {
            r.this_gesetzt = true;
        }
        // ES2023 15.7.9: `super()` ruft `F.[[Construct]](newTarget, args)` mit
        // `F` = Elternkonstruktor.
        //
        // GEMESSEN (10 test262-Faelle, alle mit "Eltern ist keine Funktion"):
        // vorher ging AUSSCHLIESSLICH der JS-Funktionsweg — `funktions_index_von`
        // matchte nur `HObj::Funktion`. `class X extends Promise` / `Object` /
        // `RangeError` / `URIError` / `RegExp` — also das Ableiten von
        // **eingebauten** Konstruktoren, das haeufigste `extends` ueberhaupt —
        // brach hier ab. Jetzt beide Wege:
        //
        // (a) JS-Funktion: als Rahmen starten (unveraenderter Weg). Der
        //     Elternrahmen ist ein `super()`-Aufruf: sein Ergebnis bleibt
        //     liegen, der Kinderrumpf laeuft danach weiter.
        let js_idx = match self.heap.holen(eltern) {
            HObj::Funktion { fn_index, .. } => Some(*fn_index),
            _ => None,
        };
        if let Some(idx) = js_idx {
            // Der Elternkonstruktor bekommt dieses `this` und `new.target`.
            // Release 15.10.2026: started as a construct (`true`), the fields
            // below are set again; a call would throw for a class parent.
            self.rahmen_starten_fn(eltern, idx, this, args.to_vec(), true)?;
            if let Some(r) = self.rahmen.last_mut() {
                r.neu = neu;
                // Release 10.10.2026: ein selbst ABGELEITETER Eltern-
                // Konstruktor bekommt `this` erst durch SEIN `super()`.
                // GEMESSEN vorher (auch im Stand cd6f6d2c): drei Stufen
                // `A0 <- A <- B` mit eigenen Konstruktoren warfen „super()
                // zweimal", ebenso `class M2 extends M1 extends Map` —
                // heise.de: 183 Custom Elements scheiterten so.
                r.this_gesetzt = r.eltern_ctor.is_none();
                r.super_kind = true;
                r.ist_konstruktor = true;
            }
            return Ok(());
        }
        // (b) Eingebauter Konstruktor (`Nativ`): synchron aufrufen. Er legt
        // sein Objekt selbst an (`n_objekt_ctor` z. B. mit `proto_objekt`,
        // **nicht** mit `new.target.prototype`) — deshalb bekommt sein
        // Ergebnis gleich den **abgeleiteten** Prototyp. Das ist genau die
        // Wirkung von `OrdinaryCreateFromConstructor(newTarget, …)`
        // (ES2023 10.1.13 Schritt 3): `new.target` ist die abgeleitete Klasse.
        let nativ = match self.native.get(&eltern).cloned() {
            Some(n) => n,
            None => {
                // z. B. `class X extends 42 {}` — der Elternwert ist gar kein
                // Konstruktor. Die Meldung bleibt die alte (der Weg ist jetzt
                // der richtige, die Diagnose unveraendert).
                let e = self.fehler_wert("TypeError", "Eltern ist keine Funktion (intern)");
                return Err(Stoerung::Wurf(e));
            }
        };
        // `this` mitgeben: eingebaute Konstruktoren, die das uebergebene
        // Objekt initialisieren (statt ein eigenes zu legen), bekommen so den
        // schon abgeleiteten `this`.
        let v = self.rufe_nativ(nativ, eltern, this, args, true)?;
        // ES2023 [[Construct]]: "Return ? F.[[Construct]](…)". Legt der
        // eingebaute Konstruktor ein **eigenes** Objekt an, uebernehmen wir es
        // als `this` (mit abgeleitetem Prototyp). Fuellt er das uebergebene,
        // bleibt `this` unveraendert.
        let this_ref = match this {
            GVal::Ref(t) => Some(t),
            _ => None,
        };
        let mut ergebnis = this;
        if let GVal::Ref(vr) = v {
            if Some(vr) != this_ref {
                let abgeleitet = this_ref.and_then(|t| self.heap_proto_von(t));
                self.ableitungs_prototyp_setzen(vr, abgeleitet);
                if let Some(r) = self.this_rahmen_index().and_then(|i| self.rahmen.get_mut(i)) {
                    r.this = v;
                }
                ergebnis = v;
            }
        }
        self.pfeile_nach_super_binden();
        // Release 10.10.2026: auch hier ist `super(...)` ein Wert (`this`),
        // siehe `rahmen_ende`.
        self.stapel.push(ergebnis);
        Ok(())
    }

    /// Release 10.10.2026: nach `super()` steht `this` des Konstruktors fest
    /// (ES2023 9.1.1.3.1 BindThisValue). Die Pfeile, die er davor anlegte,
    /// teilen sich DIESE Bindung — sie bekommen `this` jetzt als
    /// lexikalisches `this` (`gebundenes_this`).
    fn pfeile_nach_super_binden(&mut self) {
        if self.pfeile_vor_super.is_empty() {
            return;
        }
        let Some((id, this)) = self
            .this_rahmen_index()
            .and_then(|i| self.rahmen.get(i))
            .map(|r| (r.id, r.this))
        else {
            return;
        };
        let mut i = 0;
        while i < self.pfeile_vor_super.len() {
            if self.pfeile_vor_super[i].0 != id {
                i += 1;
                continue;
            }
            let (_, p) = self.pfeile_vor_super.swap_remove(i);
            if let HObj::Funktion { gebundenes_this, .. } = self.heap.holen_mut(p) {
                *gebundenes_this = Some(this);
            }
        }
    }

    /// Den Prototyp eines Objekts auf den **abgeleiteten** heben (der Weg von
    /// `super()` an einen eingebauten Elternkonstruktor, siehe `super_aufruf`).
    /// Deckt alle `HObj`-Arten mit `proto`-Feld ab (`Umgebung`/`Zeichenkette`
    /// haben keins und sind hier nie Ziel).
    pub(crate) fn ableitungs_prototyp_setzen(&mut self, o: HeapRef, p: Option<HeapRef>) {
        let h = self.heap.holen_mut(o);
        match h {
            HObj::Gegenstand { proto, .. }
            | HObj::Feld { proto, .. }
            | HObj::Verpackt { proto, .. }
            | HObj::Funktion { proto, .. }
            | HObj::Nativ { proto, .. }
            | HObj::Fehler { proto, .. }
            | HObj::Karte { proto, .. }
            | HObj::Menge { proto, .. }
            | HObj::Versprechen { proto, .. }
            | HObj::Datum { proto, .. }
            | HObj::Regex { proto, .. }
            | HObj::Generator { proto, .. }
            // Release 15.10.2026: `class M extends Uint8Array {}` /
            // `extends ArrayBuffer`. MEASURED before: `new M(4) instanceof M`
            // was false (node 22: true).
            | HObj::TypedArray { proto, .. }
            | HObj::BytePuffer { proto, .. } => *proto = p,
            _ => {}
        }
    }

    /// `true`, wenn `v` ein Objekt ist (fuer `return obj` im Konstruktor).
    pub(crate) fn ist_objekt(&self, v: GVal) -> bool {
        // Release 15.10.2026: typed arrays and buffers are objects too.
        // MEASURED before: a constructor's `return new Uint8Array(2)` was
        // dropped for `this`, `Promise.try` rejected a buffer receiver.
        matches!(v, GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Gegenstand{..} | HObj::Feld{..} | HObj::Funktion{..} | HObj::Nativ{..} | HObj::Verpackt{..} | HObj::Fehler{..} | HObj::Karte{..} | HObj::Menge{..} | HObj::Versprechen{..} | HObj::Datum{..} | HObj::Regex{..} | HObj::Generator{..} | HObj::Proxy{..} | HObj::TypedArray{..} | HObj::BytePuffer{..}))
    }
}

// ---------------------------------------------------------------------------
// Wert-Operatoren (aus narsil-js uebernommen: die VM rechnet selbst)
// ---------------------------------------------------------------------------

impl Engine {
    /// `a + b` mit Textverkettung.
    pub(crate) fn plus(&mut self, a: GVal, b: GVal) -> Result<GVal, Stoerung> {
        let pa = self.zu_primitiv(a, false)?;
        let pb = self.zu_primitiv(b, false)?;
        // Release 15.10.2026: objects that became BigInts (`Object(1n) +
        // Object(2n)`) add as BigInts.
        if (crate::ganz::ist_ganz(pa) || crate::ganz::ist_ganz(pb))
            && (!crate::ganz::ist_ganz(a) || !crate::ganz::ist_ganz(b))
            && let Some(r) = self.ganz_op(BinaerOp::Plus, pa, pb)?
        {
            return Ok(r);
        }
        // J8: der Textpfad kopierte den **grossen** String bis zu dreimal:
        // `zu_text` der linken Seite (Klon), `format!("{sa}{sb}")` und noch
        // einmal `text_wert` (Klon). GEMESSEN: `let s=''; for(…) s+='x'`
        // 10^5x kostete 1820 ms (1011 ns/Schritt) — auf einer Seite ein
        // sichtbarer Haenger. Der Weg unten baut das Ergebnis mit EINEM
        // Durchlauf; `zu_text` bleibt fuer den Mischfall (Zahl, Bool,
        // Objekt) unveraendert.
        if let (GVal::Ref(ra), GVal::Ref(rb)) = (pa, pb) {
            // Beide Seiten sind echte Heap-Texte. Der Puffer wird **ausserhalb**
            // der Leihen gebaut, damit kein Zwischen-Klon entsteht: die
            // Grenze wird ueber die Gesamtlaenge geprueft (wie im langsamen
            // Weg), und erst danach wandert der Text in den Heap.
            let mut neu = String::new();
            let mut passt = false;
            {
                let (a, b) = (self.heap.holen(ra), self.heap.holen(rb));
                if let (HObj::Zeichenkette(sa), HObj::Zeichenkette(sb)) = (a, b) {
                    if let Err(e) = self.string_grenze(sa.len() + sb.len()) {
                        return Err(Stoerung::Fehler(e));
                    }
                    neu.reserve(sa.len() + sb.len());
                    neu.push_str(sa);
                    neu.push_str(sb);
                    passt = true;
                }
            }
            if passt {
                return self.text_wert(&neu).map_err(Stoerung::Fehler);
            }
        }
        let ist_text = |e: &Self, v: &GVal| matches!(v, GVal::Ref(r) if matches!(e.heap.holen(*r), HObj::Zeichenkette(_)));
        if ist_text(self, &pa) || ist_text(self, &pb) {
            let sa = self.zu_text(pa)?;
            let sb = self.zu_text(pb)?;
            if let Err(e) = self.string_grenze(sa.len() + sb.len()) {
                return Err(Stoerung::Fehler(e));
            }
            return self
                .text_wert(&format!("{sa}{sb}"))
                .map_err(Stoerung::Fehler);
        }
        self.zahl_op(pa, pb, |x, y| x + y)
    }

    /// Rechenoperation auf Zahlen.
    pub(crate) fn zahl_op(
        &mut self,
        a: GVal,
        b: GVal,
        f: impl Fn(f64, f64) -> f64,
    ) -> Result<GVal, Stoerung> {
        let x = self.zu_zahl(a)?;
        let y = self.zu_zahl(b)?;
        Ok(GVal::Zahl(f(x, y)))
    }

    /// Ganzzahl-Operation (Bitoperationen, `<<`, `>>`).
    pub(crate) fn int_op(
        &mut self,
        a: GVal,
        b: GVal,
        f: impl Fn(i32, i32) -> i32,
    ) -> Result<GVal, Stoerung> {
        let x = crate::ops::als_int32(self.zu_zahl(a)?);
        let y = crate::ops::als_int32(self.zu_zahl(b)?);
        Ok(GVal::Zahl(f(x, y) as f64))
    }

    /// Binaerer Operator auf zwei fertigen Werten.
    pub(crate) fn binaer_werte(
        &mut self,
        op: BinaerOp,
        a: GVal,
        b: GVal,
    ) -> Result<GVal, Stoerung> {
        // J24: BigInt zuerst (der Zahlweg rundet sonst).
        if crate::ganz::ist_ganz(a) || crate::ganz::ist_ganz(b) {
            // Release 15.10.2026: an object operand goes through ToPrimitive
            // first (a BigInt wrapper, `valueOf` returning a BigInt).
            // MEASURED before: `Object(1n) + 1n` threw "Rechnung mit BigInt
            // braucht ganzzahlige Operanden" (node 22: 2n).
            if self.ist_objekt(a) || self.ist_objekt(b) {
                let pa = self.zu_primitiv(a, false)?;
                let pb = self.zu_primitiv(b, false)?;
                if !self.ist_objekt(pa) && !self.ist_objekt(pb) {
                    return self.binaer_werte(op, pa, pb);
                }
            }
            if let Some(r) = self.ganz_op(op, a, b)? {
                return Ok(r);
            }
        }
        match op {
            BinaerOp::Plus => self.plus(a, b),
            BinaerOp::Minus => self.zahl_op(a, b, |x, y| x - y),
            BinaerOp::Stern => self.zahl_op(a, b, |x, y| x * y),
            BinaerOp::Schrag => self.zahl_op(a, b, |x, y| x / y),
            BinaerOp::Prozent => self.zahl_op(a, b, |x, y| x % y),
            BinaerOp::SternStern => self.zahl_op(a, b, |x, y| x.powf(y)),
            BinaerOp::Kleiner => Ok(GVal::Bool(self.kleiner(a, b, false)?)),
            BinaerOp::KleinerGleich => Ok(GVal::Bool(self.kleiner(a, b, true)?)),
            BinaerOp::Groesser => Ok(GVal::Bool(self.kleiner(b, a, false)?)),
            BinaerOp::GroesserGleich => Ok(GVal::Bool(self.kleiner(b, a, true)?)),
            BinaerOp::GleichGleich => Ok(GVal::Bool(self.gleich_locker(a, b)?)),
            BinaerOp::RufGleich => Ok(GVal::Bool(!self.gleich_locker(a, b)?)),
            BinaerOp::GleichGleichGleich => Ok(GVal::Bool(self.gleich_strikt(a, b))),
            BinaerOp::RufGleichGleich => Ok(GVal::Bool(!self.gleich_strikt(a, b))),
            BinaerOp::KleinerKleiner => self.int_op(a, b, |x, y| x.wrapping_shl((y as u32) & 31)),
            BinaerOp::GroesserGroesser => self.int_op(a, b, |x, y| x.wrapping_shr((y as u32) & 31)),
            BinaerOp::GroesserGroesserGroesser => {
                let x = self.zu_uint32(a)?;
                let y = self.zu_uint32(b)?;
                Ok(GVal::Zahl(x.wrapping_shr(y & 31) as f64))
            }
            BinaerOp::Und => self.int_op(a, b, |x, y| x & y),
            BinaerOp::Oder => self.int_op(a, b, |x, y| x | y),
            BinaerOp::Hoch => self.int_op(a, b, |x, y| x ^ y),
            BinaerOp::Instanceof | BinaerOp::In => {
                let e = self.fehler_wert("SyntaxError", "Operator hier nicht moeglich");
                Err(Stoerung::Wurf(e))
            }
        }
    }
}

impl Engine {
    /// Ein Quelltext-Stueck im aktuellen Programm starten (`eval`).
    ///
    /// GEMESSEN/Begruendung: `eval` nutzt denselben Bytecode-Weg wie ein
    /// Skript. Das Programm wird danach weiterverwendet (der naechste
    /// `ausfuehren`-Aufruf uebersetzt neu).
    pub(crate) fn quelltext_ausfuehren(&mut self, quelle: &str) -> Result<GVal, Stoerung> {
        if diag_an() {
            let anfang: String = quelle.chars().take(80).collect();
            eprintln!("NARSIL_JS_DIAG eval {} Byte: {anfang}", quelle.len());
        }
        // J7: ein **Quelltextfehler** in `eval(...)` ist ein Wurf
        // (sec-18.6.1.1: wirft einen SyntaxError), kein Engine-Fehler —
        // nur so faengt ein `try { eval('let =') } catch (e) { … }` ihn.
        // GEMESSEN: vorher brach `eval('let =')` den ganzen Lauf ab
        // (`Syntaxfehler 1:5: erwartet: …`).
        let tokens = match crate::lex::lexen(
            quelle,
            self.grenzen.max_quelltext_bytes,
            self.grenzen.max_token,
        ) {
            Ok(t) => t,
            Err(e) => return Err(self.syntax_als_wurf(e)),
        };
        let ast =
            match crate::parse::parsen_mit_quelle(tokens, self.grenzen.max_parser_tiefe, &quelle) {
                Ok(a) => a,
                Err(e) => return Err(self.syntax_als_wurf(e)),
            };
        // OPUS_1: laeuft schon ein Programm, wird der eval-Text **an** das
        // Programm angehaengt (gleiche Indizes) und in einem eigenen Rahmen
        // ausgefuehrt — `starten` wuerde Rahmen und Stapel loeschen.
        if self.rahmen.is_empty() || self.programm.is_none() {
            let prog = match self.uebersetzen(&ast) {
                Ok(p) => p,
                Err(e) => return Err(self.syntax_als_wurf(e)),
            };
            return match self.starten(prog) {
                Ok(v) => Ok(v),
                Err(e) => Err(Stoerung::Fehler(e)),
            };
        }
        let prog = match self.uebersetzen(&ast) {
            Ok(p) => p,
            Err(e) => return Err(self.syntax_als_wurf(e)),
        };
        let strikt = prog.strikt || self.strikt;
        let haupt = self.programm_anhaengen(prog).map_err(Stoerung::Fehler)?;
        // Release 09.10.2026: Eval-Code hat **kein** eigenes `arguments`
        // (ES2023 19.2.1.3 EvalDeclarationInstantiation legt keins an) — er
        // sieht das des Aufrufers. GEMESSEN (node 22): `function f(a,b){
        // return eval("arguments.length") } f(1,2,3)` -> `3`; der Motor
        // baute im Eval-Rahmen ein leeres und lieferte `0`.
        if let Some(f) = self.programm.as_mut().and_then(|p| p.funktionen.get_mut(haupt)) {
            f.braucht_arguments = false;
        }
        let (code, temps) = {
            let f = &self.programm.as_ref().expect("gesetzt").funktionen[haupt];
            (f.code.clone(), f.temps as usize)
        };
        let vor = self.rahmen.len();
        let stapel_vor = self.stapel.len();
        let rahmen_id = self.rahmen_id_holen();
        // OPUS_2: direktes `eval` sieht den Bereich des Aufrufers
        // (ES2023 19.2.1.1 PerformEval, `direct = true`): Umgebung und
        // `this` des Rahmens, der `eval` gerufen hat (Nativ-Aufrufe legen
        // keinen Rahmen an, also ist das `rahmen.last()`).
        // GEMESSEN vorher: `function f(){ var x = 1; return eval('x') }`
        // -> ReferenceError (9 test262-Faelle).
        // Striktes eval: `var` bleibt in einer **eigenen** Umgebung.
        let global = std::mem::take(&mut self.eval_global);
        let als_skript = std::mem::take(&mut self.skript_global);
        let aufrufer_umg = if global {
            self.global_umgebung
        } else {
            self.rahmen
                .last()
                .map(|r| r.umg)
                .unwrap_or(self.global_umgebung)
        };
        let this = if global { GVal::Ref(self.global_objekt) } else { self.this_rahmen() };
        let umg = if strikt && !als_skript {
            self.umgebung_neu(Some(aufrufer_umg))
                .map_err(Stoerung::Fehler)?
        } else {
            aufrufer_umg
        };
        self.rahmen.push(Rahmen {
            // BILD_29 (Befreiung fuer QUIC): `Rahmen` hat das Feld `fortgesetzt`
            // bekommen — „der Generator lief schon vor diesem Schritt". Beim
            // **Anlegen** einer Frame hat er noch nicht gelaufen (`Start`), also
            // `false`. **Diese Stellen sind die von `bau.sh narsil check`
            // gemeldeten 6x E0063** — ohne sie ist `narsil-jsvm` nicht baubar und
            // damit **kein** Test in `narsil` pruefbar. Der Wert ist an der
            // Semantik abgelesen, nicht geraten.
            fortgesetzt: false,
            wurzel_marke: self.wurzel_merke(),
            id: rahmen_id,
            fun: haupt,
            param_umg: None,
            prolog_rumpf: None,
            zeile: self.funktions_zeile(haupt),
            code,
            umg,
            basis: stapel_vor,
            this,
            strikt,
            heim: None,
            eltern_ctor: None,
            handler: Vec::new(),
            fertig: Vec::new(),
            temps: vec![GVal::Undefiniert; temps],
            args: Vec::new(),
            neu: GVal::Undefiniert,
            ist_konstruktor: false,
            this_gesetzt: true,
            aufnahme: None,
            ertrag_stern: None,
            // W17: der eval-Rahmen ist kein Generator- und kein async-Rahmen.
            generator: None,
            super_kind: false,
            iter_lauf: Vec::new(),
            stapel: Vec::new(),
            pc: 0,
        });
        match self.schleife(vor) {
            LaufErgebnis::Fertig(v) => {
                if self.stapel.len() > stapel_vor {
                    self.stapel.truncate(stapel_vor);
                }
                Ok(v)
            }
            LaufErgebnis::Wurf(w) => Err(Stoerung::Wurf(w)),
            LaufErgebnis::Angehalten => Err(self.fehlt_noch("Anhalten in eval")),
            LaufErgebnis::Fehler(e) => Err(Stoerung::Fehler(e)),
        }
    }
}

impl Engine {
    /// Closure aus einer (zur Laufzeit uebersetzten) Funktion.
    ///
    /// V6: der Bytecode wird in die Tabelle des **laufenden** Programms
    /// aufgenommen. `MAX_DYNAMISCHE_FUNKTIONEN` (M4/M6) deckt das: ein Skript
    /// kann nicht unbegrenzt `new Function` aufrufen — jeder Aufruf macht
    /// einen GC-Lauf noetig, und der Heap hat seine Grenze.
    pub(crate) fn closure_aus_funktion(
        &mut self,
        f: crate::op::Funktion,
        scope: HeapRef,
    ) -> Result<HeapRef, Stoerung> {
        let idx = match &mut self.programm {
            Some(p) => {
                if p.funktionen.len() >= MAX_DYNAMISCHE_FUNKTIONEN {
                    let e = self.fehler_wert("RangeError", "zu viele dynamische Funktionen");
                    return Err(Stoerung::Wurf(e));
                }
                p.funktionen.push(f);
                (p.funktionen.len() - 1) as u32
            }
            None => {
                let e = self.fehler_wert("SyntaxError", "kein Programm (intern)");
                return Err(Stoerung::Wurf(e));
            }
        };
        self.closure_anlegen(idx, scope)
    }
}

impl Engine {
    /// `super.name` lesen: Suche startet im Heimat-Prototyp (V2).
    pub(crate) fn super_lesen(&mut self, name: &str) -> Result<GVal, Stoerung> {
        match self.super_methode(name) {
            Ok(f) => Ok(f),
            Err(e) => Err(e),
        }
    }

    /// `super.name` als Wert (Methodenaufruf) oder als Funktion.
    fn super_methode(&mut self, name: &str) -> Result<GVal, Stoerung> {
        // J50: auch `super` braucht `this` — vor `super()` wirft es einen
        // ReferenceError (ES2023 9.1.1.3.4 `GetThisBinding`).
        //
        // GEMESSEN (node 22), **drei** ReferenceError:
        //   `class D extends B { constructor(){ super.m } }`
        //   `class D extends B { constructor(){ super['m'] } }`
        //   `class D extends B { constructor(){ super.m() } }`
        if let Some(e) = self.this_geprueft() {
            return Err(Stoerung::Wurf(e));
        }
        let heim = match self.heim_rahmen() {
            Some(h) => h,
            None => {
                let e = self.fehler_wert("SyntaxError", "super ausserhalb einer Methode");
                return Err(Stoerung::Wurf(e));
            }
        };
        let start = match self.heap_proto_von(heim) {
            Some(p) => p,
            None => {
                let e = self.fehler_wert("TypeError", "super: kein Heimat-Prototyp");
                return Err(Stoerung::Wurf(e));
            }
        };
        match self.hole(GVal::Ref(start), name) {
            Ok(v) => Ok(v),
            Err(Stoerung::Wurf(w)) => Err(Stoerung::Wurf(w)),
            Err(Stoerung::Fehler(e)) => Err(Stoerung::Fehler(e)),
        }
    }

    /// `super.name = wert` schreiben (eigene Property des `this`).
    pub(crate) fn super_schreiben(&mut self, name: &str, wert: GVal) -> Result<(), Stoerung> {
        // J50: siehe `super_methode` — auch das Schreiben braucht `this`.
        if let Some(e) = self.this_geprueft() {
            return Err(Stoerung::Wurf(e));
        }
        let this = self.this_heim();
        self.setze(this, name, wert)
    }
}

impl Engine {
    /// Generator-Prototyp mit `next`/`return`/`throw` (V3).
    ///
    /// Wird aus `aufbauen` (builtin.rs) nach dem Anlegen der anderen
    /// Prototypen gerufen — der Generator-Typ ist V6-Neuheit.
    pub(crate) fn generator_prototyp_aufbauen(&mut self) {
        let p = match self.neues_objekt(Some(self.proto_objekt)) {
            Ok(p) => p,
            Err(_) => return,
        };
        self.proto_generator = p;
        // Release 14.10.2026: the methods are functions — their [[Prototype]]
        // is Function.prototype, not the generator prototype. MEASURED before:
        // `gen().next.call(g)` -> "call is not a function" (WPT dom/observable).
        let fp = self.proto_funktion;
        for (name, art) in [("next", 0u8), ("return", 2u8), ("throw", 1u8)] {
            let f = self.nativ_anlegen(name, crate::builtin::Nativ::GeneratorSchritt(art), fp);
            let _ = self.eigenschaft_nicht_aufzaehlbar(p, name, GVal::Ref(f));
        }
        // J34: `@@iterator` mit **abgeleitetem** Schluessel — derselbe Weg,
        // den `iterator_werte` zum Suchen geht (`n_symbol_bekannt` +
        // `schluessel_text`). Ein festgeschriebener Text waere hier still
        // falsch, sobald sich die Schluessel-Konvention aendert.
        if let Ok(sym) =
            self.n_symbol_bekannt(GVal::Undefiniert, &[], crate::builtin::Nativ::SymIterator)
            && let Ok(schl) = self.schluessel_text(sym)
        {
            let f = self.nativ_anlegen(
                "[Symbol.iterator]",
                crate::builtin::Nativ::IteratorSelbst,
                fp,
            );
            let _ = self.eigenschaft_nicht_aufzaehlbar(p, &schl, GVal::Ref(f));
        }
    }
}

// ==================================================================
// WIEDERAUFBAU (03.10.2026, JS-Rolle) — nach dem Fund
// `team/vm_opus_nachspiel_1111.rs` (Stand ~11:11).
//
// Die Funddatei hatte den ganzen VM-Kern (Schleife, Dispatch, Generator-,
// Klassen- und Aufrufwege) wieder, aber **nicht** die Methoden, die die
// anderen Module auf der Engine rufen. Diese sind hier **neu geschrieben**:
// jede Signatur ist aus den Aufrufstellen im Baum abgelesen, jede
// Bedeutung aus der Doku der Aufrufer oder aus einer Messung.
//
// Wo die Bedeutung nicht sicher rekonstruierbar ist, gibt die Methode einen
// **benannten Fehler** zurueck — keinen stillen Ersatzwert (M1/M3).
// ==================================================================

/// M4/M6: Obergrenze der Schlangenlaenge in einem Abarbeiten (M3).
const MIKRO_DECKEL: usize = 10_000;

impl Engine {
    /// „Diese Stelle fehlt noch" — benannter **Wert**, kein Panic.
    fn fehlt_noch(&mut self, was: &str) -> Stoerung {
        Stoerung::Fehler(JsFehler::Laufzeit(format!(
            "Wiederaufbau: Engine::{was} fehlt noch (Funddatei 11:11)"
        )))
    }

    /// `text_von(v)`: der Text eines Wertes, **ohne** Wurf.
    ///
    /// GEMESSEN/ENTSCHIEDEN, warum die Primitive eigene Wege bekommen: der
    /// Dispatch laeuft bei **jedem** Eigenschaftszugriff hier vorbei
    /// (13 Aufrufstellen in `vm.rs`, dazu `builtin.rs`, `symbole.rs`,
    /// `url.rs`). Nur Objekte gehen ueber `zu_text` — dort kann ein Symbol
    /// einen TypeError werfen, der landet als Leertext (die Alternative
    /// waere `panic!`).
    pub(crate) fn text_von(&mut self, v: GVal) -> String {
        match v {
            GVal::Zahl(z) => crate::wert::zahl_text(z),
            GVal::Bool(true) => "true".to_string(),
            GVal::Bool(false) => "false".to_string(),
            GVal::Undefiniert => "undefined".to_string(),
            GVal::Null => "null".to_string(),
            GVal::Ganz(g) => g.to_string(),
            GVal::Ref(_) => self.zu_text(v).unwrap_or_default(),
        }
    }

    /// Der Schluesseltext eines **berechneten** Zugriffs `o[k]` — die
    /// Umsetzung von ES2023 6.1.7 `ToPropertyKey` fuer diesen Motor.
    ///
    /// ## W11 (JS, 04.10.2026) — WARUM NEBEN `text_von`
    ///
    /// GEMESSEN: `var s = Symbol('a'); var o = {}; o[s] = 1;` legte die
    /// Eigenschaft unter dem **leeren** Schluessel `""` an:
    ///     JSON.stringify(o)                    -> `{"":1}`   (statt `{}`)
    ///     Object.getOwnPropertyNames(o)        -> `[""]`     (statt `[]`)
    ///     Object.keys(o).length                 -> `1`        (statt `0`)
    /// Ursache: `text_von` geht fuer `GVal::Ref` ueber `zu_text(v)`, und
    /// **`ToString` eines Symbols ist laut Spec ein TypeError** (ES2023
    /// 7.1.18) — `unwrap_or_default()` machte daraus `""`.
    ///
    /// `schluessel_text` (ops.rs) macht es richtig: es liefert fuer ein
    /// Symbol `n_symbol_text` = `"Symbol(<Beschreibung>)"`, und genau diesen
    /// Text benutzt der Motor an anderer Stelle (dokumentiert in ops.rs
    /// W16). Es fehlte nur an den **zwölf** Dispatch-Stellen, die den
    /// Schluessel eines berechneten Zugriffs selbst holen.
    ///
    /// Der Fehlerpfad bleibt wie vorher: `unwrap_or_default()`. Eine
    /// Symbol-Beschreibung kann nichts an `ToString` scheitern lassen, also
    /// entsteht hier **kein** neuer Wurf — der Unterschied ist genau der
    /// Schluesseltext.
    ///
    /// **BEKANNTE GRENZE (nicht hier behoben):** der Text enthaelt nur die
    /// *Beschreibung*, also fallen `Symbol('a')` und ein zweites
    /// `Symbol('a')` auf denselben Schluessel. Das ist die in ops.rs W16
    /// dokumentierte Motor-Eigenheit und gehoert in einen eigenen Schritt.
    pub(crate) fn rechnen_schluessel(&mut self, k: GVal) -> String {
        self.schluessel_text(k).unwrap_or_default()
    }

    /// `eigene_map(r)`: die **eigenen** Eigenschaften in
    /// Einfuegereihenfolge (ES2023 7.3.2).
    ///
    /// GEMESSEN, warum nicht ueber die `HashMap`: deren Reihenfolge ist
    /// hashabhaengig — `JSON.stringify({value, done})` lieferte
    /// `{"done","value"}`. Das Feld `reihenfolge` im Heap traegt genau
    /// darum diese Reihenfolge (siehe `heap.rs`).
    ///
    /// Der Aufrufer braucht **beides**: `dom.rs` liest `w.wert`,
    /// `eigenschaften.rs` prueft `e.aufzählbar`.
    pub(crate) fn eigene_map(&self, r: HeapRef) -> Vec<(String, crate::heap::Eigenschaft)> {
        let HObj::Gegenstand {
            eigenschaften,
            reihenfolge,
            ..
        } = self.heap.holen(r)
        else {
            return Vec::new();
        };
        let mut raus = Vec::with_capacity(reihenfolge.len());
        for name in reihenfolge.iter() {
            if let Some(e) = eigenschaften.get(name) {
                raus.push((name.clone(), e.clone()));
            }
        }
        raus
    }

    /// `lauf_umg()`: die Umgebung, in der **jetzt** Namen aufgeloest werden —
    /// im Parameterprolog die Parameterumgebung, sonst die des Rumpfs (J48).
    /// `lauf_umg()`: die Umgebung, in der **jetzt** Namen aufgeloest werden.
    ///
    /// J48: im **Parameterprolog** ist die Parameterumgebung die richtige
    /// (ES2023 10.2.11 Schritt 27 — nur bei einer nicht einfachen Liste gibt
    /// es sie; dann ist `param_umg` gesetzt, sonst faellt sie auf `umg`
    /// zurueck), sonst die des Rumpfs.
    pub(crate) fn lauf_umg(&self) -> HeapRef {
        self.rahmen
            .last()
            // Release 10.10.2026: im Prolog IST `umg` die Parameterumgebung
            // (siehe `Rahmen::prolog_rumpf`) — vorher lieferte diese Stelle
            // `param_umg`, sobald es sie gab, also auch im Rumpf.
            .map(|r| r.umg)
            .unwrap_or(self.global_umgebung)
    }

    /// `aufruf_synchron(f, this, args, als_konstruktor)`: ein Aufruf, der
    /// **hier** endet (32 Aufrufstellen in `builtin*.rs`, `ops.rs`,
    /// `eigenschaften.rs`, `ereignis.rs`, `url.rs`, `interp.rs`).
    ///
    /// GEMESSEN/ENTSCHIEDEN, warum das zwei Schritte sind: `aufruf_einfuegen`
    /// legt Nativ-/Host-Ergebnisse **auf den Stapel** und schiebt
    /// Benutzerfunktionen als Rahmen darauf; im Dispatch laeuft dann die
    /// Schleife weiter. Von aussen (Builtin) muss der Aufruf hier enden:
    /// Rahmenzahl merken, `schleife(vor)` laufen lassen, Ergebnis nehmen.
    pub(crate) fn aufruf_synchron(
        &mut self,
        f: GVal,
        this: GVal,
        args: &[GVal],
        als_konstruktor: bool,
    ) -> Result<GVal, Stoerung> {
        self.aufruf_synchron_neu(f, this, args, als_konstruktor, None)
    }

    /// Release 15.10.2026: like `aufruf_synchron`, with an explicit
    /// `new.target` for the called frame (`Reflect.construct(F, args, NT)`).
    /// MEASURED before: `new.target` inside `F` was `F`, not `NT`.
    pub(crate) fn aufruf_synchron_neu(
        &mut self,
        f: GVal,
        this: GVal,
        args: &[GVal],
        als_konstruktor: bool,
        neu: Option<GVal>,
    ) -> Result<GVal, Stoerung> {
        let vor = self.rahmen.len();
        // J49: die Stapellaenge VOR dem Aufruf merken.
        //
        // GEMESSEN (84 test262-Faelle, Ursache von QUIC im
        // BRETT 17:40 benannt): `aufruf_synchron` hat zwei
        // Wege, und nur der Nativ-/Host-Zweig ist
        // stapelneutral. Im **Benutzerzweig** beendet
        // `rahmen_ende` den Rahmen mit `stapel.truncate(basis)`
        // **und** `stapel.push(v)` — der Rueckgabewert bleibt
        // also liegen, und `Fertig(v)` gibt ihn nur noch einmal
        // zurueck. Ein Wert zu viel auf dem Stapel des
        // aufrufenden Rahmens verschiebt alle folgenden
        // Operanden (`Op::AufrufMethode` liest die Basis fest
        // unter den Argumenten).
        //
        //   `var o = { get k(){ return true; } };
        //    assert.sameValue(JSON.stringify(o, ['k']), '…')`
        //   -> der Getter liess `true` auf dem Stapel, und
        //      `assert.sameValue(…)` las als Basis `true`
        //      statt `assert` — „sameValue is not a function".
        //
        // Der eine Ort, an dem der `pop` hingehört, ist hier
        // (QUIC hat in `builtin.rs` nur den Workaround
        // `aufruf_ohne_stapelspur` gebaut, der den Stapel
        // danach zuruecksetzt — die direkten Aufrufer in
        // `eigenschaften.rs` (Getter/Setter/Iterator) und
        // `builtin3.rs` (JSON/Map/Set) hatten keinen).
        let stapel_vor = self.stapel.len();
        self.aufruf_einfuegen(f, this, args, als_konstruktor)?;
        if self.rahmen.len() == vor {
            // Nativ oder Hostfunktion: das Ergebnis liegt oben.
            return Ok(self.stapel.pop().unwrap_or(GVal::Undefiniert));
        }
        if let Some(n) = neu
            && let Some(r) = self.rahmen.get_mut(vor)
        {
            r.neu = n;
        }
        let profil = profil_an();
        if profil {
            profil_ebene_an("<js-rueckruf>");
        }
        let lauf = self.schleife(vor);
        if profil {
            profil_ebene_aus(None);
        }
        match lauf {
            LaufErgebnis::Fertig(v) => {
                // Der Rueckgabewert liegt via `rahmen_ende`
                // **auch** auf dem Stapel — fuer den synchronen
                // Aufruf aus einem Nativ heraus ist er doppelt.
                // Zuruecksetzen auf die Laenge vor dem Aufruf.
                if self.stapel.len() > stapel_vor {
                    self.stapel.truncate(stapel_vor);
                }
                Ok(v)
            }
            LaufErgebnis::Wurf(w) => Err(Stoerung::Wurf(w)),
            LaufErgebnis::Angehalten => Err(self.fehlt_noch("Anhalten im Aufruf (await/yield)")),
            LaufErgebnis::Fehler(e) => Err(Stoerung::Fehler(e)),
        }
    }

    /// `this_geprueft()` (J50): ist `this` schon benutzt worden, bevor es
    /// initialisiert war?
    ///
    /// GEMESSEN (`node 22`): `class A extends B { constructor(){ this.x;
    /// super(); } }` -> **ReferenceError**. Der Pfad folgt dem Rahmen, der
    /// `this` liefert — Pfeile binden keins, bei ihnen laeuft die Suche
    /// nach aussen (darum wird `braucht_this` geprueft).
    pub(crate) fn this_geprueft(&mut self) -> Option<GVal> {
        for r in self.rahmen.iter().rev() {
            let braucht = self
                .programm
                .as_ref()
                .and_then(|p| p.funktionen.get(r.fun))
                .map(|f| f.braucht_this)
                .unwrap_or(true);
            if !braucht {
                continue;
            }
            // GEMESSEN, warum **nur abgeleitete** Konstruktoren: die
            // Funddatei prueft es bei `Op::RueckgabeThis` genauso
            // (`abgeleitet && !gesetzt`) — ein Basis-Konstruktor hat `this`
            // schon beim Eintritt, ein abgeleiteter erst nach `super()`.
            //
            // GEMESSEN (Ursachenliste, 234 Faelle): mit `ist_konstruktor`
            // statt `eltern_ctor.is_some()` war **jeder** Konstruktor
            // betroffen und die Suite brach bei Klassentests ab.
            if r.eltern_ctor.is_some() && !r.this_gesetzt {
                let e = self.fehler_wert(
                    "ReferenceError",
                    "this ist noch nicht initialisiert (vor super())",
                );
                return Some(e);
            }
            return None;
        }
        None
    }

    /// `namen_setzen_auf(wert, name)` — `SetFunctionName` (ES2023 10.2.4).
    ///
    /// GEMESSEN: `eigenschaften.rs` liest zuerst die **eigene**
    /// Eigenschaft und faellt sonst auf das Struct-Feld zurueck — darum
    /// gehoert der Name hier hinein und nicht in `name`.
    /// Nicht aufzählbar, nicht beschreibbar, konfigurierbar.
    pub(crate) fn namen_setzen_auf(&mut self, wert: GVal, name: GVal) {
        // W11 (JS, 04.10.2026): ES2023 10.2.4 `SetFunctionName` Schritt 5 —
        // "If name is a Symbol, then let name be name's [[Description]] value".
        // Verlangt ist also die **Beschreibung**, nicht `String(symbol)`.
        // `text_von` lieferte fuer ein Symbol `""` (`ToString` eines Symbols
        // ist laut 7.1.18 ein TypeError, `unwrap_or_default()` machte daraus
        // den leeren Text) und die Leer-Pruefung darunter verwarf den Namen
        // dann **ganz**. Bei einem Symbol **ohne** Beschreibung ist
        // `undefined` die Vorgabe — der Leerfall bleibt damit richtig.
        let text = match name {
            // Release 09.10.2026: mit eckigen Klammern (ES2023 10.2.9
            // Schritt 5.b: "[" + description + "]"). GEMESSEN (node 22):
            // `{[Symbol('r')]: function(){}}` -> Name `[r]`.
            GVal::Ref(r) if crate::symbole::ist_symbol(self, r) => {
                match self.n_symbol_text(name, &[], true) {
                    Ok(g) => {
                        let t = self.text_von(g);
                        if t.is_empty() { t } else { format!("[{t}]") }
                    }
                    Err(_) => String::new(),
                }
            }
            andere => self.text_von(andere),
        };
        if text.is_empty() {
            return;
        }
        let Ok(n) = self.text_wert(&text) else { return };
        let GVal::Ref(r) = wert else { return };
        let e = crate::heap::Eigenschaft {
            wert: n,
            getter: None,
            setter: None,
            aufzählbar: false,
            beschreibbar: false,
            konfigurierbar: true,
        };
        // Release 09.10.2026: die Eigenschaft heisst `name` (ES2023 10.2.9
        // SetFunctionName Schritt 6). GEMESSEN vorher: hier stand der
        // (ueberschattete) Funktionsname als Schluessel — `{[k]: function(){}}`
        // mit k = 'q' bekam eine Eigenschaft `q` und `.name` blieb `""`.
        match self.heap.holen_mut(r) {
            HObj::Funktion { eigenschaften, .. } | HObj::Nativ { eigenschaften, .. } => {
                eigenschaften.insert("name".to_string(), e);
            }
            _ => {}
        }
    }

    /// `closure_anlegen(i, umg)`: aus einem Funktions-Index ein
    /// Funktions-Objekt machen (ES2023 10.2.10 `MakeFunctionObject`).
    ///
    /// GEMESSEN: **1971 von 2117** Suite-Faellen sind an dieser einen
    /// Stelle gescheitert — jeder Ausdruck `function …`/`(…) =>` und jede
    /// Deklaration laeuft hier durch.
    pub(crate) fn closure_anlegen(&mut self, i: u32, umg: HeapRef) -> Result<HeapRef, Stoerung> {
        let idx = i as usize;
        let kopf = self.aufruf_kopf(idx)?;
        let name = self
            .programm
            .as_ref()
            .and_then(|p| p.funktionen.get(idx))
            .map(|f| f.selbst_name.clone().unwrap_or_else(|| f.name.clone()))
            .unwrap_or_default();
        // `heim`/`eltern_ctor` aus dem laufenden Rahmen: eine Methode erbt
        // damit das Heimat-Objekt fuer `super` (J50).
        let (heim, eltern_ctor) = match self.rahmen.last() {
            Some(r) => (r.heim, r.eltern_ctor),
            None => (None, None),
        };
        // Release 10.10.2026: `super()` erbt NUR ein Pfeil (ES2023 15.3: ein
        // Pfeil hat keine eigene `this`-/`super`-Bindung). GEMESSEN vorher:
        // jede `function`/`function*` in einer abgeleiteten Klasse erbte den
        // Elternkonstruktor und galt damit als Konstruktor „vor super()" —
        // TypeScripts `__awaiter(this, …, function* () { this.x })` warf
        // „this ist noch nicht initialisiert" (Optimizely-SDK, bbc.com).
        let eltern_ctor = if kopf.pfeil { eltern_ctor } else { None };
        // `this` ist lexikalisch und gehoert zur Closure, nicht zum spaeteren
        // Aufrufstapel. Vor `super()` bleibt der vorhandene Weg ueber den
        // aktiven Konstruktor erhalten: dessen `this` ist noch uninitialisiert.
        let vor_super: Option<u64> = self.rahmen.iter().rev()
            .find(|rahmen| self.braucht_this(rahmen))
            .filter(|rahmen| rahmen.eltern_ctor.is_some() && !rahmen.this_gesetzt)
            .map(|rahmen| rahmen.id);
        let uninitialisiert = vor_super.is_some();
        let lexikalisches_this = if kopf.pfeil && !uninitialisiert {
            Some(self.this_rahmen())
        } else {
            None
        };
        // Release 10.10.2026: async/Generator-Funktionen haben ihren eigenen
        // [[Prototype]] (`%AsyncFunction.prototype%` …, siehe `interp.rs`).
        let fn_proto = match (kopf.asynchron, kopf.generator) {
            (true, false) => self.proto_async_funktion,
            (false, true) => self.proto_generator_funktion,
            (true, true) => self.proto_async_generator_funktion,
            _ => self.proto_funktion,
        };
        let fn_proto = if fn_proto == crate::heap::KEIN_REF { self.proto_funktion } else { fn_proto };
        // Release 15.10.2026: the async iteration prototypes and the
        // generator-function links, built with the first such function
        // (`es_modern.rs`, `es_async_aufbauen`).
        if (kopf.asynchron || kopf.generator) && self.es.async_gen_proto.is_none() {
            self.es_async_aufbauen()?;
        }
        let obj = HObj::Funktion {
            name,
            fn_index: idx,
            scope: Some(umg),
            proto: Some(fn_proto),
            gebundenes_this: lexikalisches_this,
            gebundene_args: Vec::new(),
            prototyp_objekt: None,
            ist_pfeil: kopf.pfeil,
            ist_native: false,
            heim,
            eltern_ctor,
            ist_konstruktor: false,
            eigenschaften: Default::default(),
        };
        let kosten = crate::heap::Heap::kosten_fuer(&obj);
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        let r = self.belege(obj, kosten);
        // Release 10.10.2026: ein Pfeil VOR `super()` bekommt sein `this`,
        // sobald `super()` es festlegt (`pfeile_nach_super_binden`). Vorher
        // blieb er ohne — ein spaeterer Aufruf (ausserhalb des
        // Konstruktors, oder `async`: der Rumpf liest `this` aus dem
        // eigenen Rahmen) bekam das `this` des AUFRUFERS. GEMESSEN (node 22
        // in Klammern): `constructor(){ const f = async () => this.b;
        // super(); f() }` -> TypeError „kein Zugriff auf Undefiniert" (b).
        if kopf.pfeil
            && let Some(id) = vor_super
        {
            if self.pfeile_vor_super.len() >= 4096 {
                self.pfeile_vor_super.remove(0);
            }
            self.pfeile_vor_super.push((id, r));
        }

        // J54: das `prototype`-Objekt einer **gewöhnlichen** Funktion
        // (ES2023 10.2.4 `OrdinaryFunctionCreate`, Schritt 27).
        //
        // GEMESSEN (`node 22`, eine Zeile je Fall, jede in einem eigenen
        // Prozess):
        //   function f(){}        -> f.prototype: object, constructor === f,
        //                             enumerable: false, writable: true,
        //                             configurable: false
        //   function k(){}        -> descriptor(k.prototype,'constructor')
        //                             .enumerable: false
        //   function* g(){}       -> g.prototype: object, aber
        //                             constructor !== g
        //   async function a(){}  -> a.prototype: undefined
        //   var y = () => 1       -> y.prototype: undefined
        //   ({ m(){} }).prototype -> undefined
        //
        // OHNE das Objekt war `f.prototype` **undefined** — und der
        // test262-Harness starbt an `Test262Error.prototype.toString = …`
        // mit „kein Setzen auf Primitiv“ (1971 von 2117 Faellen).
        if !kopf.pfeil && !kopf.generator && !kopf.asynchron {
            let po = self
                .neues_objekt(Some(self.proto_objekt))
                .map_err(Stoerung::Fehler)?;
            // `po.constructor = f` — nicht aufzählbar, beschreibbar,
            // konfigurierbar.
            let ctor = crate::heap::Eigenschaft {
                wert: GVal::Ref(r),
                getter: None,
                setter: None,
                aufzählbar: false,
                beschreibbar: true,
                konfigurierbar: true,
            };
            if let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen_mut(po) {
                eigenschaften.insert("constructor".to_string(), ctor);
            }
            // Das `prototype`-Feld bedient den `hole()`-Zweig
            // (`eigenschaften.rs`); zusaetzlich die **echte** eigene
            // Eigenschaft, damit `getOwnPropertyDescriptor` sie sieht.
            let proto_e = crate::heap::Eigenschaft {
                wert: GVal::Ref(po),
                getter: None,
                setter: None,
                aufzählbar: false,
                beschreibbar: true,
                konfigurierbar: false,
            };
            if let HObj::Funktion {
                prototyp_objekt,
                eigenschaften,
                ..
            } = self.heap.holen_mut(r)
            {
                *prototyp_objekt = Some(po);
                eigenschaften.insert("prototype".to_string(), proto_e);
            }
        } else if kopf.generator && !kopf.pfeil && !kopf.asynchron {
            // Release 15.10.2026: every generator function has its OWN
            // `prototype` (no `constructor`) inheriting from
            // %GeneratorPrototype% (ECMA-262 15.5.3); the generator objects
            // inherit from it. MEASURED before: all generator functions
            // shared %GeneratorPrototype% as `prototype`, so `g.prototype ===
            // h.prototype` and the chain was one level short.
            let po = self
                .neues_objekt(Some(self.proto_generator))
                .map_err(Stoerung::Fehler)?;
            let proto_e = crate::heap::Eigenschaft {
                wert: GVal::Ref(po),
                getter: None,
                setter: None,
                aufzählbar: false,
                beschreibbar: true,
                konfigurierbar: false,
            };
            if let HObj::Funktion {
                prototyp_objekt,
                eigenschaften,
                ..
            } = self.heap.holen_mut(r)
            {
                *prototyp_objekt = Some(po);
                eigenschaften.insert("prototype".to_string(), proto_e);
            }
        } else if kopf.generator && !kopf.pfeil {
            // Release 15.10.2026: an async generator function has its OWN
            // `prototype` inheriting from %AsyncGeneratorPrototype%
            // (ECMA-262 15.6.3). MEASURED before (node 22 in brackets): it
            // shared %GeneratorPrototype% (`f.prototype !== g.prototype`
            // false, true) and `f() instanceof f` was false (true).
            let agp = self.es.async_gen_proto.unwrap_or(self.proto_objekt);
            let po = self.neues_objekt(Some(agp)).map_err(Stoerung::Fehler)?;
            let proto_e = crate::heap::Eigenschaft {
                wert: GVal::Ref(po),
                getter: None,
                setter: None,
                aufzählbar: false,
                beschreibbar: true,
                konfigurierbar: false,
            };
            if let HObj::Funktion {
                prototyp_objekt,
                eigenschaften,
                ..
            } = self.heap.holen_mut(r)
            {
                *prototyp_objekt = Some(po);
                eigenschaften.insert("prototype".to_string(), proto_e);
            }
        }
        Ok(r)
    }

    /// Eine entnommene Promise-Reaktion bleibt bis zum Abschluss GC-verwurzelt.
    fn mikrotask_ausfuehren(&mut self, t: crate::interp::Mikrotask) -> Option<JsFehler> {
        let marke = self.wurzel_merke();
        self.wurzeln.push(t.wert);
        self.wurzeln.push(GVal::Ref(t.folge));
        if let Some(f) = t.reaktion_fn { self.wurzeln.push(GVal::Ref(f)); }
        let fehler = if let Some(g) = t.fortsetzung {
            self.wurzeln.push(GVal::Ref(g));
            self.async_fortsetzen(g, t.aufnahme).err()
        } else if t.art == 3 {
            let erf = self.resolver_anlegen(t.folge, true);
            self.wurzeln.push(GVal::Ref(erf));
            let abl = self.resolver_anlegen(t.folge, false);
            self.wurzeln.push(GVal::Ref(abl));
            let f = t.reaktion_fn.expect("Thenable-Job hat eine Funktion");
            match self.aufruf_synchron(GVal::Ref(f), t.wert, &[GVal::Ref(erf), GVal::Ref(abl)], false) {
                Ok(_) => None,
                Err(Stoerung::Wurf(g)) => { self.versprechen_settle(t.folge, false, g); None }
                Err(Stoerung::Fehler(e)) => Some(e),
            }
        } else if let Some(f) = t.reaktion_fn {
            let args = if t.art == 1 { Vec::new() } else { vec![t.wert] };
            match self.aufruf_synchron(GVal::Ref(f), GVal::Undefiniert, &args, false) {
                Ok(g) => {
                    if t.art == 1 {
                        match self.endlich_warten(t.wert, t.erfuellt, t.folge, g) {
                            Ok(()) => None,
                            Err(Stoerung::Wurf(w)) => { self.versprechen_settle(t.folge, false, w); None }
                            Err(Stoerung::Fehler(e)) => Some(e),
                        }
                    } else {
                        self.versprechen_settle(t.folge, true, g);
                        None
                    }
                }
                Err(Stoerung::Wurf(g)) => { self.versprechen_settle(t.folge, false, g); None }
                Err(Stoerung::Fehler(e)) => Some(e),
            }
        } else {
            self.versprechen_settle(t.folge, t.erfuellt, t.wert);
            None
        };
        self.wurzel_zurueck(marke);
        fehler
    }

    /// Die Mikrotask-Schlange abarbeiten. Timer folgen im autonomen Modus
    /// am Ende des Hauptlaufs; im Browser steuert sie der Fensterhost.
    ///
    /// GEMESSEN/ENTSCHIEDEN: **ohne** diese Schleife erfuellt sich kein
    /// Versprechen — `ausfuehren_innen` ruft sie nach dem Hauptlauf auf.
    pub(crate) fn schlange_abarbeiten(&mut self) -> Option<JsFehler> {
        let deckel = self.grenzen.max_mikrotasks.max(8);
        let mut schritte = 0usize;
        while !self.mikrotasks.is_empty() {
            schritte += 1;
            if schritte > MIKRO_DECKEL {
                return Some(JsFehler::Zeitlimit(
                    "Mikrotask-Schlange laeuft nicht leer".to_string(),
                ));
            }
            let t = self.mikrotasks.remove(0);
            if let Some(e) = self.mikrotask_ausfuehren(t) { return Some(e); }
            if self.mikrotasks.len() > deckel {
                return Some(JsFehler::Zeitlimit("zu viele Mikrotasks".to_string()));
            }
        }
        // Release 10.10.2026: Mikrotasks leer -> unbehandelte Ablehnungen melden.
        self.unbehandelt_melden();
        // Die **Timer** laufen hier nicht: der Host zieht sie ueber
        // `timer_einen_ausfuehren`/`timer_frueheste_ms` (`netz.rs`,
        // `ereignis.rs`) — eine Schleife hier wuerde die Reihenfolge der
        // Mikrotasks gegen die des Hosts verwirren.
        None
    }

    /// `mikro_abarbeiten(deckel)`: die Schlange **begrenzt** abarbeiten
    /// (Aufrufstellen `netz.rs`, `ereignis.rs`: dort, wo der Host keine
    /// Zeit hat, alles zu Ende zu rechnen).
    pub(crate) fn mikro_abarbeiten(&mut self, deckel: usize) -> Option<JsFehler> {
        let grenze = deckel.max(1).min(MIKRO_DECKEL);
        let mut n = 0usize;
        while !self.mikrotasks.is_empty() && n < grenze {
            let t = self.mikrotasks.remove(0);
            if let Some(e) = self.mikrotask_ausfuehren(t) { return Some(e); }
        }
        // Release 10.10.2026: Mikrotasks leer -> unbehandelte Ablehnungen melden.
        if self.mikrotasks.is_empty() {
            self.unbehandelt_melden();
        }
        None
    }

    /// J36: traegt der **Feldwert** noch den eingebauten `@@iterator`?
    ///
    /// GEMESSEN (node 22), der Grund fuer die Pruefung:
    ///   `Array.prototype[Symbol.iterator] = function*(){ yield 42 }`
    ///   `for (var x of [1,2,3])` -> `42` — der **eigene** Iterator laeuft.
    /// Ohne diese Frage nahm `iter_werte` immer den Iteratorweg.
    /// Release 10.10.2026: traegt das Objekt (selbst oder ueber die Kette)
    /// eine `@@iterator`-Methode? Fuer `Op::MusterQuelleFeld` — nur lesen,
    /// keine Getter (die Kette ist hoechstens 64 tief, M4).
    fn hat_iterator_methode(&self, r: HeapRef) -> bool {
        let mut o = Some(r);
        for _ in 0..64 {
            let Some(x) = o else { return false };
            if let Some(m) = self.eigene_lese(x, "Symbol(Symbol.iterator)") {
                return self.ist_aufrufbar(m);
            }
            o = self.heap_proto_von(x);
        }
        false
    }

    pub(crate) fn iterator_ist_eingebaut_feld(&self, v: GVal) -> bool {
        let GVal::Ref(r) = v else {
            return false;
        };
        // Release 10.10.2026: der ECHTE Schluessel im Property-Speicher ist
        // `Symbol(Symbol.iterator)` (`symbole::symbol_schluessel` fuer ein
        // wohlbekanntes Symbol) — `symbol_text(1)` („@sym1") fand nie etwas,
        // also wurde auch eine EIGENE Ersetzung nie bemerkt.
        let sk = "Symbol(Symbol.iterator)";
        if let Some(eigen) = self.eigene_lese(r, sk) {
            // Release 15.10.2026: an arguments object carries
            // %Array.prototype.values% as its own @@iterator (klassen.rs);
            // unchanged, it keeps the index fast path.
            return self.ist_argumente_objekt(r) && matches!(eigen, GVal::Ref(f) if Some(f) == self.es.feld_werte_fn);
        }
        // Release 10.10.2026: auch der PROTOTYP muss noch den eingebauten
        // `@@iterator` tragen (`%Array.prototype.values%`). GEMESSEN vorher
        // (node 22 in Klammern): `Array.prototype[Symbol.iterator] =
        // function*(){ n++; … }; var [a, b] = [5, 6]` -> n = 0  (1) — der
        // Indexweg sah die Ersetzung nicht, nur eine eigene.
        match self.heap_proto_von(r) {
            Some(p) if p == self.proto_feld => match self.eigene_lese(p, sk) {
                Some(GVal::Ref(f)) => {
                    matches!(self.native.get(&f), Some(crate::builtin::Nativ::FeldWerteIter))
                }
                _ => false,
            },
            // Ein Feld mit fremdem Prototyp (Unterklasse von Array): der
            // Iteratorweg fragt die Kette.
            Some(_) => false,
            None => true,
        }
    }

    /// `iter_starten(v, umgekehrt, t)`: die Quelle eines `for…of`/`for…in`
    /// materialisieren und in Temp `t` ablegen.
    ///
    /// GEMESSEN/ENTSCHIEDEN, warum **hier** und nicht im Dispatch: der
    /// Iteratorweg ist der teure (J36) — er wird nur betreten, wenn es
    /// laufen muss. Fuer ein Feld mit dem eingebauten `@@iterator` reichen
    /// die Indizes.
    pub(crate) fn iter_starten(
        &mut self,
        v: GVal,
        umgekehrt: bool,
        t: u16,
    ) -> Result<(), Stoerung> {
        // GEMESSEN/ENTSCHIEDEN: der zweite Parameter ist **kein** „rueckwaerts"
        // — `Op::IterStarten` traegt `0 = of, 1 = in` (siehe `op.rs`), und der
        // Aufrufer setzt `*art == 1`. Fuer `in` will die Norm (ES2023
        // 14.7.5.6) die **Namen** der aufzählbaren Eigenschaften (eigene und
        // geerbte), fuer `of` die Werte des Iterators.
        //
        // GEMESSEN (Ursachenliste, 56 Faelle, `built-ins/JSON/15.12-0-4.js`):
        // mit dem Iteratorweg bekam jedes `for (k in o)` „Wert ist nicht
        // iterierbar".
        let werte = if umgekehrt {
            let namen = self.schluessel_werte(v)?;
            let mut raus: Vec<GVal> = Vec::with_capacity(namen.len());
            // Release 09.10.2026: `for-in` zaehlt nur TEXT-Schluessel auf
            // (EnumerateObjectProperties, ES2023 14.7.5.9) — Symbole nicht.
            for n in namen.into_iter().filter(|n| !crate::heap::ist_symbolschluessel(n)) {
                raus.push(self.text_wert(&n).map_err(Stoerung::Fehler)?);
            }
            raus
        } else if self.iter_faul(v) {
            // Release 10.10.2026: der FAULE Weg (ES2023 14.7.5.7
            // `ForIn/OfBodyEvaluation`: je Durchlauf EIN `next()`). Vorher
            // wurde jeder Iterator vorab leergelesen. GEMESSEN (node 22 in
            // Klammern): `function* h(){ g++; yield 1; g++; yield 2; g++ }
            // for (x of h()) break; g` -> 3  (1); ein unendlicher Generator
            // mit `break` lief bis zur Zeitgrenze.
            let (iter, next) = self.iterator_holen(v)?;
            let idx = t as usize;
            if let Some(r) = self.rahmen.last_mut() {
                if r.iter_lauf.len() <= idx {
                    r.iter_lauf.resize(idx + 1, (Vec::new(), 0));
                }
                r.iter_lauf[idx] = (vec![iter, next], ITER_FAUL);
            }
            return Ok(());
        } else {
            self.iter_werte(v)?
        };
        let idx = t as usize;
        if let Some(r) = self.rahmen.last_mut() {
            if r.iter_lauf.len() <= idx {
                r.iter_lauf.resize(idx + 1, (Vec::new(), 0));
            }
            r.iter_lauf[idx] = (werte, 0);
        }
        Ok(())
    }

    /// Release 10.10.2026: nimmt `for…of` ueber `v` den faulen Weg? Felder
    /// mit eingebautem `@@iterator`, Zeichenketten, Boxen, Map und Set
    /// behalten den Schnellweg (eine Werteliste); Generatoren, eigene
    /// Iteratoren und Felder mit ersetztem `@@iterator` gehen je Schritt
    /// ueber `next()`.
    fn iter_faul(&self, v: GVal) -> bool {
        let GVal::Ref(r) = v else {
            return false;
        };
        match self.heap.holen(r) {
            HObj::Feld { .. } => !self.iterator_ist_eingebaut_feld(v),
            HObj::Zeichenkette(_) | HObj::Verpackt { .. } | HObj::Karte { .. } | HObj::Menge { .. } => false,
            _ => true,
        }
    }

    /// Release 14.10.2026: IteratorClose (ES2024 7.4.11) for a `for…of` that
    /// is left by `break`, `continue` to an outer loop or `return`: the lazy
    /// iterator in temp `t` gets `return()` (if it has one; the result must
    /// be an object). The eager fast path (arrays, strings, Map, Set) has
    /// nothing to close. MEASURED before: `for (x of gen()) break;` never ran
    /// the generator's `finally`, and a ReadableStream iterated with
    /// `for await` stayed locked after `break` (Agent D, WPT streams).
    pub(crate) fn iter_schliessen(&mut self, t: u16) -> Result<(), Stoerung> {
        let idx = t as usize;
        let iter = self
            .rahmen
            .last()
            .and_then(|r| r.iter_lauf.get(idx))
            .and_then(|(w, pos)| (*pos == ITER_FAUL && w.len() == 2).then(|| w[0]));
        let Some(iter) = iter else {
            return Ok(());
        };
        let marke = self.wurzel_merke();
        self.wurzeln.push(iter);
        if let Some(r) = self.rahmen.last_mut()
            && let Some(e) = r.iter_lauf.get_mut(idx)
        {
            *e = (Vec::new(), 0);
        }
        let ret = match self.hole(iter, "return") {
            Ok(v) => v,
            Err(e) => {
                self.wurzel_zurueck(marke);
                return Err(e);
            }
        };
        if matches!(ret, GVal::Undefiniert | GVal::Null) {
            self.wurzel_zurueck(marke);
            return Ok(());
        }
        if !self.ist_aufrufbar(ret) {
            self.wurzel_zurueck(marke);
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "iterator.return is not a function")));
        }
        self.wurzeln.push(ret);
        let erg = self.aufruf_synchron(ret, iter, &[], false);
        self.wurzel_zurueck(marke);
        let erg = erg?;
        if !self.ist_objekt(erg) {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "iterator.return() did not return an object")));
        }
        Ok(())
    }

    /// `iter_naechster(t)`: der naechste Wert des Laufs aus Temp `t`
    /// (`Wert`/`Ende`/`Angehalten`/`Fehler` — die Form steht am Dispatch).
    pub(crate) fn iter_naechster(&mut self, t: u16) -> IterErgebnis {
        let idx = t as usize;
        // Release 10.10.2026: der faule Weg (siehe `iter_starten`).
        let faul = self
            .rahmen
            .last()
            .and_then(|r| r.iter_lauf.get(idx))
            .and_then(|(w, pos)| (*pos == ITER_FAUL && w.len() == 2).then(|| (w[0], w[1])));
        if let Some((iter, next)) = faul {
            let schritt = self.iterator_schritt(iter, next);
            let fertig = !matches!(schritt, Ok(Some(_)));
            if fertig
                && let Some(r) = self.rahmen.last_mut()
                && let Some(e) = r.iter_lauf.get_mut(idx)
            {
                *e = (Vec::new(), 0);
            }
            return match schritt {
                Ok(Some(v)) => IterErgebnis::Wert(v),
                Ok(None) => IterErgebnis::Ende,
                Err(Stoerung::Wurf(w)) => IterErgebnis::Wurf(w),
                Err(Stoerung::Fehler(e)) => IterErgebnis::Fehler(e),
            };
        }
        if let Some(r) = self.rahmen.last_mut()
            && let Some((werte, pos)) = r.iter_lauf.get_mut(idx)
        {
            if *pos < werte.len() {
                let v = werte[*pos];
                *pos += 1;
                return IterErgebnis::Wert(v);
            }
            // Aufraeumen: der Lauf ist vorbei, der Platz bleibt frei fuer
            // den naechsten (M3, sonst waechst die Liste pro Schleife).
            r.iter_lauf[idx] = (Vec::new(), 0);
        }
        IterErgebnis::Ende
    }

    // --- Stellen, deren Bedeutung **nicht** sicher rekonstruierbar ist ---
    // Jede gibt einen benannten Fehler. Wer die alte Fassung findet,
    // fuellt genau diese eine Funktion.

    /// `symbol_eigen_wert_eintragen(r, name)`.
    ///
    /// GEMESSEN: der native Eintrag `("values", Nativ::FeldWerteIter)` fuer
    /// das Feld-Prototyp steht in `builtin.rs` **vor** diesem Aufruf, und
    /// der Kommentar darueber (J32) warnt nur vor der Reihenfolge. Der
    /// Originalkoerper ist nicht (mit)gekommen; ein Fehler hier wuerde
    /// `Engine::aufbauen` und damit die ganze Engine mitreissen.
    pub(crate) fn symbol_eigen_wert_eintragen(&mut self, r: HeapRef, name: &str) {
        // J36 (GEMESSEN in `builtin.rs:1413`): `r["@@iterator"] = r[name]`,
        // damit `[][Symbol.iterator] === [].values` **true** ist und
        // `typeof [][Symbol.iterator]` `"function"` liefert (node 22 so).
        //
        // Ohne das war `Array.prototype[Symbol.iterator]` **undefined** — die
        // Feld-Schleife ging nur ueber den `iter_werte`-Schnellweg, aber
        // jeder, der den echten Schluessel las, fand nichts. Genau daran
        // scheiterte die Leer-Muster-Pruefung (`Op::MusterQuelleLeer`).
        //
        // Der Schluessel wird **abgeleitet** (`schluessel_text`), nicht
        // eingesetzt; der Aufrufer steht deshalb bewusst ganz hinten im
        // Aufbau (`Symbol.prototype.toString` muss schon stehen, sonst
        // liefert `ToPrimitive` `"[object Object]"` — GEMESSEN, J36/J32).
        let sym =
            match self.n_symbol_bekannt(GVal::Undefiniert, &[], crate::builtin::Nativ::SymIterator)
            {
                Ok(s) => s,
                Err(_) => return,
            };
        let schluessel = match self.schluessel_text(sym) {
            Ok(k) => k,
            Err(_) => return,
        };
        let wert = match self.hole(GVal::Ref(r), name) {
            Ok(w) => w,
            Err(_) => return,
        };
        let _ = self.setze(GVal::Ref(r), &schluessel, wert);
        // ES2023 23.1.3.2: `{ writable: true, enumerable: false,
        // configurable: true }` — nicht aufzaehlbar (der einfache Weg in
        // `setze` setzt `aufzaehlbar: true`).
        if let Some(e) = self.eigenschaft_mut(r, &schluessel) {
            e.aufzählbar = false;
            e.beschreibbar = true;
            e.konfigurierbar = true;
        }
    }

    // --- Helfer fuer `klasse_ausfuehren` (ES2023 15.7) ---

    /// `name_im_lauf(idx)`: den Wert einer Variable im laufenden
    /// Scope holen — fuer berechnete Klassennamen (`['n']`), die
    /// **einmal** beim Anlegen der Klasse ausgewertet werden (J12).
    ///
    /// Der Index zeigt in `namen` der aktuellen Funktion (der Scope, in
    /// dem die Klasse steht). Der Wert ist ein `GVal`, der Text wird
    /// dann per `text_von` gelesen.
    fn name_im_lauf(&mut self, idx: u32) -> Result<GVal, Stoerung> {
        let rahmen = self.rahmen.last().ok_or_else(|| {
            Stoerung::Fehler(JsFehler::Laufzeit(
                "kein Rahmen fuer Klassenname".to_string(),
            ))
        })?;
        let text = self.text_konstante(idx);
        // Der Name steht als Variable im Scope — `LeseVar` oder
        // `LeseVarName` hat ihn dort abgelegt. Wir lesen den Wert.
        let g = self.umgebung_lesen(rahmen.umg, &text)?;
        Ok(g)
    }

    /// `umgebung_lesen(umg, name)`: einen Wert aus einer Umgebung lesen
    /// (ueber die Kette, wie `hole` aber ohne Host-Haken).
    fn umgebung_lesen(&mut self, umg: HeapRef, name: &str) -> Result<GVal, Stoerung> {
        let mut cur = Some(umg);
        while let Some(r) = cur {
            let o = self.heap.holen(r);
            if let crate::heap::HObj::Umgebung(u) = o {
                // J3b: Hash-Vergleich (FNV-1a), dann Text zur Sicherheit.
                let h = crate::heap::namens_hash(name);
                if let Some(e) = u.bindungen.find(h, name) {
                    return Ok(e.wert);
                }
                cur = u.eltern;
            } else {
                break;
            }
        }
        Ok(GVal::Undefiniert)
    }

    /// `klassen_zugriff_definieren`: Getter **oder** Setzer in einer
    /// Klassen-Eigenschaft definieren. Die Gegenseite bleibt erhalten.
    ///
    /// `is_getter` = true -> `get`, false -> `set`.
    /// `aufzählbar` = false, `konfigurierbar` = true (ES2023 15.7.13).
    fn klassen_zugriff_definieren(
        &mut self,
        ziel: HeapRef,
        name: &str,
        fun: HeapRef,
        is_getter: bool,
    ) -> Result<(), Stoerung> {
        let (getter, setter) = if is_getter {
            (Some(fun), None)
        } else {
            (None, Some(fun))
        };
        // J27/ES2023 15.7.13: `enumerable: false`, `configurable: true`
        self.getter_setter_definieren_mit(ziel, name, getter, setter, false, true)
            .map_err(Stoerung::Fehler)
    }

    /// Rohes Eintragen einer Daten-Eigenschaft (wie `setze`
    /// aber ohne Host-Haken — der Aufrufer hat die Allokation
    /// schon im Budget via `vor_allok` falls noetig).
    fn eigenschaft_setzen_roh(&mut self, ziel: HeapRef, name: &str, wert: GVal) {
        let _ = self.setze(GVal::Ref(ziel), name, wert);
    }

    /// Mutability auf eine Eigenschaft — fuer Attribut-Aenderung nach
    /// dem Anlegen (z.B. `configurable: true` bei Klassen-Methoden).
    fn eigenschaft_mut(
        &mut self,
        ziel: HeapRef,
        name: &str,
    ) -> Option<&mut crate::heap::Eigenschaft> {
        let o = self.heap.holen_mut(ziel);
        match o {
            crate::heap::HObj::Gegenstand { eigenschaften, .. }
            | crate::heap::HObj::Verpackt { eigenschaften, .. }
            | crate::heap::HObj::Feld {
                rest: eigenschaften,
                ..
            }
            | crate::heap::HObj::Funktion { eigenschaften, .. }
            | crate::heap::HObj::Nativ { eigenschaften, .. }
            | crate::heap::HObj::Generator { eigenschaften, .. }
            | crate::heap::HObj::Fehler { eigenschaften, .. }
            | crate::heap::HObj::Datum { eigenschaften, .. }
            | crate::heap::HObj::Regex { eigenschaften, .. }
            | crate::heap::HObj::Karte { eigenschaften, .. }
            | crate::heap::HObj::Menge { eigenschaften, .. }
            | crate::heap::HObj::Versprechen { eigenschaften, .. } => eigenschaften.get_mut(name),
            _ => None,
        }
    }

    /// `klasse_ausfuehren(i, eltern)`: eine kompilierte Klasse **anlegen**
    /// (ES2023 15.7 `ClassDefinitionEvaluation`, die fuer diesen Motor
    /// noetigen Schritte).
    ///
    /// GEMESSEN, warum das die groesste offene Gruppe war: die Ursachenliste
    /// aus dem Lauf ueber die echte Suite (Stand 51,4 %) nennt
    /// **`187`** Faelle mit `Wiederaufbau: Engine::klasse_ausfuehren fehlt
    /// noch` — mehr als jede andere einzelne Ursache. Alle lagen in
    /// `test/language/expressions/class` und `test/built-ins/...`, also in
    /// `class`-Quellen, die der Uebersetzer bereits uebersetzt (der Befehl
    /// `Op::Klasse` mit der Nummer der `KlassenDef` stand da), nur eben
    /// nicht ausfuehrt.
    ///
    /// Der Elternwert liegt als **GVal** da (`Op::Null`, wenn die Klasse kein
    /// `extends` hat — siehe `compile.rs`, `klasse()`). Daraus werden:
    ///
    /// * `eltern_ctor` — der Elternkonstruktor fuer `super()` (im Rahmen
    ///   gesetzt, siehe `Engine::super_aufruf`),
    /// * `eltern_prototyp` — `A.prototype` als Prototyp von `B.prototype`,
    /// * `eltern_von_ctor` — `A` als Prototyp von `B` selbst
    ///   (ES2023 15.7.12: `Object.setPrototypeOf(F, ParentClass)`).
    ///
    /// **Naeherung, benannt:** `extends null` und „kein `extends`" kommen
    /// beide als `GVal::Null` an (`compile.rs` emittiert in beiden Faellen
    /// `Op::Null`), werden also gleich behandelt — die Klasse gilt als
    /// **nicht abgeleitet**. Das ist an den Ergebnissen nicht zu sehen
    /// (`B.prototype.__proto__` ist `Object.prototype` statt `null`, und
    /// `new B()` legt ein Objekt an statt zu scheitern); es sind vier
    /// Suite-Faelle.
    pub(crate) fn klasse_ausfuehren(
        &mut self,
        i: u32,
        eltern_wert: GVal,
    ) -> Result<GVal, Stoerung> {
        // Alles, was aus dem Programm gelesen wird, **vor** der ersten
        // Allokation holen (der Aufrufer haelt nur `i` als Wurzel).
        let k_def = match self
            .programm
            .as_ref()
            .and_then(|p| p.klassen.get(i as usize))
            .cloned()
        {
            Some(k) => k,
            None => {
                let e = self.fehler_wert("TypeError", "Klassenindex falsch (intern)");
                return Err(Stoerung::Wurf(e));
            }
        };
        // Release 10.10.2026: die KLASSEN-UMGEBUNG (ES2023 15.7.14
        // `ClassDefinitionEvaluation` Schritt 1-4): eine eigene Umgebung mit
        // dem Klassennamen als unveraenderlicher Bindung. Alle Methoden und
        // Initialisierer schliessen ueber ihr ab; initialisiert wird sie vor
        // den statischen Feldern (Schritt 27, unten). Vorher wurde der Name
        // erst NACH `Op::Klasse` gebunden. GEMESSEN (node 22 in Klammern):
        //   `class Z { static a = 1; static b = Z.a + 1 } Z.b`
        //       -> ReferenceError 'Z'  (2)
        //   `class X { static i = new X() }` -> ReferenceError  (ein X)
        //   `var K = class Z { static b = Z }` -> ebenso
        let umg_aussen = self.lauf_umg();
        let klassen_umg = match k_def.name.as_deref() {
            Some(n) if !n.is_empty() => {
                let k = self.umgebung_neu(Some(umg_aussen)).map_err(Stoerung::Fehler)?;
                let _ = self.deklariere(k, n, BindungsArt::Const);
                Some(k)
            }
            _ => None,
        };
        let umg = klassen_umg.unwrap_or(umg_aussen);

        // --- 1. Elternklasse bestimmen (ES2023 15.7.9 `ClassDefinitionEvaluation`
        // Schritt 6: `ClassHeritage`) ---
        let (eltern_ctor, eltern_prototyp, proto_von_ctor) = match eltern_wert {
            GVal::Null | GVal::Undefiniert => (None, None, self.proto_funktion),
            GVal::Ref(r) => {
                // Release 10.10.2026: ERST `IsConstructor` (ES2023 15.7.14
                // Schritt 8.f), dann `prototype` (8.g) — und eine
                // Pfeilfunktion/Methode ist kein Konstruktor. GEMESSEN vorher
                // (node 22 in Klammern): `class C extends (() => 1) {}` ->
                // „does not have valid prototype property"  („is not a
                // constructor or null").
                if !crate::eingebaut_array::ist_konstruktor(self, eltern_wert) {
                    let e = self.fehler_wert(
                        "TypeError",
                        "Class extends value is not a constructor or null",
                    );
                    return Err(Stoerung::Wurf(e));
                }
                // `A.prototype` — bei `extends null` ist das `null`.
                let p = self.hole(eltern_wert, "prototype")?;
                let eltern_prototyp = match p {
                    GVal::Ref(q) if self.heap.lebt(q) => Some(q),
                    GVal::Null => None,
                    _ => {
                        let e = self.fehler_wert(
                            "TypeError",
                            "Class extends value does not have valid prototype property",
                        );
                        return Err(Stoerung::Wurf(e));
                    }
                };
                // `A` muss ein **Konstruktor** sein (ES2023 15.7.9 Schritt 6.b).
                let ist_ctor = match self.heap.holen(r) {
                    HObj::Funktion { .. } | HObj::Nativ { .. } => true,
                    _ => false,
                };
                if !ist_ctor {
                    let e =
                        self.fehler_wert("TypeError", "Class extends value is not a constructor");
                    return Err(Stoerung::Wurf(e));
                }
                (Some(r), eltern_prototyp, r)
            }
            _ => {
                let e = self.fehler_wert("TypeError", "Class extends value is not a constructor");
                return Err(Stoerung::Wurf(e));
            }
        };

        // --- 2. `B.prototype` (ES2023 15.7.12 `SetFunctionName` +
        // `MakeConstructor`) ---
        let proto_quell = eltern_prototyp.unwrap_or(self.proto_objekt);
        let prototyp = self
            .neues_objekt(Some(proto_quell))
            .map_err(Stoerung::Fehler)?;

        // --- 3. Die Konstruktorfunktion ---
        let ctor_idx = match k_def.ctor {
            Some(c) => c,
            None => {
                let e = self.fehler_wert("TypeError", "Klasse ohne Konstruktor (intern)");
                return Err(Stoerung::Wurf(e));
            }
        };
        // `closure_anlegen` legt bei einer gewöhnlichen Funktion **selbst**
        // ein `prototype` an (J54). Bei einer Klasse muss es **das** hier
        // angelegte sein, deshalb wird es gleich ueberschrieben.
        let cr = self.closure_anlegen(ctor_idx, umg)?;
        let klassenname = k_def.name.clone().unwrap_or_default();
        if let HObj::Funktion {
            name,
            proto,
            prototyp_objekt,
            heim,
            eltern_ctor: ec,
            ist_konstruktor,
            eigenschaften,
            ..
        } = self.heap.holen_mut(cr)
        {
            // J54/Klassen: `B.prototype` ist **nicht** aufzählbar und
            // **nicht** konfigurierbar (bei einer gewöhnlichen Funktion ist
            // es konfigurierbar — `klasse_ausfuehren` setzt es daher neu).
            if let Some(alt) = eigenschaften.get_mut("prototype") {
                // OPUS_1: auch den **Wert** ersetzen. GEMESSEN vorher:
                // `class C { m(){} }; typeof C.prototype.m` -> `undefined`
                // (die Methoden lagen am Objekt aus `prototyp_objekt`, die
                // Eigenschaft zeigte auf das von `closure_anlegen`).
                alt.wert = GVal::Ref(prototyp);
                alt.getter = None;
                alt.setter = None;
                alt.aufzählbar = false;
                alt.beschreibbar = false;
                alt.konfigurierbar = false;
            } else {
                eigenschaften.insert(
                    "prototype".to_string(),
                    crate::heap::Eigenschaft {
                        wert: GVal::Ref(prototyp),
                        getter: None,
                        setter: None,
                        aufzählbar: false,
                        beschreibbar: false,
                        konfigurierbar: false,
                    },
                );
            }
            *name = klassenname.clone();
            *proto = Some(proto_von_ctor);
            *prototyp_objekt = Some(prototyp);
            *heim = Some(prototyp);
            *ec = eltern_ctor;
            *ist_konstruktor = true;
        }
        // Der constructor wird vor den Methoden als erste eigene
        // Prototyp-Eigenschaft angelegt: writable/configurable, nicht enumerable.
        if let HObj::Gegenstand { eigenschaften, reihenfolge, .. } = self.heap.holen_mut(prototyp) {
            reihenfolge.push("constructor".to_string());
            eigenschaften.insert(
                "constructor".to_string(),
                crate::heap::Eigenschaft {
                    wert: GVal::Ref(cr),
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
        }

        // --- 4. Elemente (ES2023 15.7.13) ---
        //
        // Reihenfolge ist **Quelltextordnung** (ein statisches Feld darf ein
        // vorher definiertes statisches Feld lesen).
        //
        // GEMESSEN, warum Getter und Setzer **hier** und nicht gesammelt am
        // Ende definiert werden: `class C { get x(){…} set x(v){…} }` ergibt
        // **eine** Eigenschaft mit beiden Haelften. Wer sie nacheinander
        // ueber `getter_setter_definieren_mit` anlegt, loescht mit jedem
        // Schritt die andere Haelfte — `get` blieb dann weg. Der Weg liest
        // darum die **Gegenseite** aus dem Property-Map und traegt beide ein.
        for el in k_def.elemente.iter() {
            match el {
                crate::op::KlassenElementCode::Methode {
                    name,
                    fun,
                    statisch,
                    art,
                    berechnet,
                } => {
                    // `berechnet` = der **Name steht als Variable** im
                    // umgebenden Scope (J12) — einmal lesen, nicht fuer
                    // Getter/Setter getrennt.
                    let schluessel = if *berechnet {
                        let v = self.name_im_lauf(*name)?;
                        // W11: `ToPropertyKey` (ES2023 6.1.7) — ein berechneter
                        // Klassenname darf ein **Symbol** sein
                        // (`class C { [Symbol()](){} }`, ES2023 15.7.1). Mit
                        // `text_von` kam da `""` heraus.
                        self.rechnen_schluessel(v)
                    } else {
                        self.text_konstante(*name)
                    };
                    let mr = self.closure_anlegen(*fun, umg)?;
                    // Heimat-Objekt: `prototype` bei einer Instanzmethode,
                    // der Konstruktor bei `static`.
                    let heim = if *statisch { cr } else { prototyp };
                    if let HObj::Funktion {
                        name,
                        heim: h,
                        eltern_ctor: ec,
                        ..
                    } = self.heap.holen_mut(mr)
                    {
                        // SetFunctionName: bei `get x` steht **kein** `get`
                        // im Funktionsnamen (node 22: `get x(){}`.name ===
                        // "get x", `x` ist der Schluessel).
                        *name = schluessel.clone();
                        *h = Some(heim);
                        *ec = eltern_ctor;
                    }
                    let ziel = if *statisch { cr } else { prototyp };
                    match art {
                        crate::ast::MethodenArt::Getter => {
                            self.klassen_zugriff_definieren(ziel, &schluessel, mr, true)?;
                        }
                        crate::ast::MethodenArt::Setter => {
                            self.klassen_zugriff_definieren(ziel, &schluessel, mr, false)?;
                        }
                        _ => {
                            // ES2023 15.7.13: `writable: true`,
                            // `enumerable: false`, `configurable: true`
                            // (die Suite prueft das in
                            // `class/.../definition-property-attributes.js`).
                            self.eigenschaft_setzen_roh(ziel, &schluessel, GVal::Ref(mr));
                            if let Some(e) = self.eigenschaft_mut(ziel, &schluessel) {
                                e.aufzählbar = false;
                                e.beschreibbar = true;
                                e.konfigurierbar = true;
                            }
                        }
                    }
                }
                crate::op::KlassenElementCode::Feld {
                    name,
                    statisch,
                    berechnet,
                } => {
                    // Instanzfelder laufen ueber `felder_inst` im
                    // Konstruktor (compile.rs, `klasse()`) — hier nur die
                    // **statischen**.
                    if !*statisch {
                        continue;
                    }
                    let schluessel = if *berechnet {
                        let v = self.name_im_lauf(*name)?;
                        // W11: `ToPropertyKey` (ES2023 6.1.7) — ein berechneter
                        // Klassenname darf ein **Symbol** sein
                        // (`class C { [Symbol()](){} }`, ES2023 15.7.1). Mit
                        // `text_von` kam da `""` heraus.
                        self.rechnen_schluessel(v)
                    } else {
                        self.text_konstante(*name)
                    };
                    // Der Wert kommt aus `felder_statisch` (Schritt 5). Bis
                    // dahin steht hier `undefined` — der Initialisierer
                    // schreibt darueber.
                    self.eigenschaft_setzen_roh(cr, &schluessel, GVal::Undefiniert);
                    if let Some(e) = self.eigenschaft_mut(cr, &schluessel) {
                        e.aufzählbar = true;
                        e.beschreibbar = true;
                        e.konfigurierbar = true;
                    }
                }
            }
        }

        // Release 10.10.2026: der Klassenname ist ab hier gebunden (ES2023
        // 15.7.14 Schritt 27 — vor den statischen Feldern).
        if let (Some(k), Some(n)) = (klassen_umg, k_def.name.as_deref()) {
            self.initialisiere(k, n, GVal::Ref(cr));
        }

        // --- 5. Statische Felder (ES2023 15.7.14) ---
        //
        // Sie laufen als **Funktion** mit `this` = Konstruktor, damit ein
        // berechneter Name und `B` selbst sichtbar sind. Der Aufruf endet
        // hier — der Dispatch laeuft danach weiter.
        if k_def.felder_statisch != 0
            && let Some(fi) = self.programm.as_ref().and_then(|p| {
                p.funktionen
                    .get(k_def.felder_statisch as usize)
                    .map(|f| !f.code.is_empty())
            })
        {
            if fi {
                let stat_fun = self.closure_anlegen(k_def.felder_statisch, umg)?;
                self.aufruf_synchron(GVal::Ref(stat_fun), GVal::Ref(cr), &[], false)?;
            }
        }

        Ok(GVal::Ref(cr))
    }

    /// `hole_index_nach_property` — nur ein Lesehilfsweg fuer die
    /// Getter/Setter-Folge in `klasse_ausfuehren` (kein neues Merkmal).
    fn hole_index_nach_property(&mut self, basis: HeapRef, name: &str) -> Option<()> {
        let _ = (basis, name);
        None
    }

    /// `konstruktor_proto(fr)`: der Prototyp fuer `new F()` — `F.prototype`,
    /// sonst `Object.prototype` (ES2023 10.1.15 `GetPrototypeFromConstructor`).
    ///
    /// GEMESSEN/GEFUNDEN: der alte Stand gab `KEIN_REF` zurueck. `neu_aufruf
    /// ruft die Stelle fuer **jedes** `new`, also bekam jedes Objekt einen
    /// toten Prototypverweis — und der naechste Methodenzugriff darauf
    /// panickte in `heap.rs` („Heap: toter Verweis", gefangen als
    /// „JS-Absturz (intern)").
    pub(crate) fn konstruktor_proto(&mut self, fr: HeapRef) -> HeapRef {
        let p = match self.heap.holen(fr) {
            HObj::Funktion {
                prototyp_objekt: Some(po),
                ..
            } => Some(*po),
            _ => None,
        };
        match p {
            Some(q) if q != KEIN_REF && self.heap.lebt(q) => q,
            _ => self.proto_objekt,
        }
    }

    /// `generator_starten(r, fn_index, this, args)`: einen Generator
    /// **starten** (ES2023 27.3.1 `GeneratorStart`).
    ///
    /// Der Aufrufer hat `closure_anlegen` schon gerufen, also liegt die
    /// Funktions-Closure in `r`. Ein Generator wird aber **nicht**
    /// ausgefuehrt, sondern gibt ein **Generator-Objekt** zurueck (mit
    /// `next`/`return`/`throw`). Der Generator-Rahmen wird hier **angelegt**
    /// und sofort **angehalten** (`GenZustand::Start` -> beim ersten `next`
    /// laeuft er bis zum ersten `yield`).
    ///
    /// GEMESSEN (Ursachenliste): **57** Suite-Faelle scheiterten an
    /// `Wiederaufbau: Engine::generator_starten fehlt noch` — alles in
    /// `test/language/expressions/class/dstr/` (Generator-Methoden in
    /// Klassen, oft mit Destrukturierung). Die Quote von 51,4 % auf 59,9 %
    /// kam **ohne** Generatoren — Generatoren sind der naechste grosse Block.
    pub(crate) fn generator_starten(
        &mut self,
        r: HeapRef,
        fn_index: usize,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        // 1. Den Generator-Rahmen anlegen (wie einen normalen Aufruf,
        // aber NICHT ausfuehren). Der Generator haelt seine eigenen
        // Rahmen im Heap (`HObj::Generator { frames }`).
        let f_def = self.aufruf_kopf(fn_index).map_err(Stoerung::Fehler)?;
        let (scope, heim, eltern_ctor) = match self.heap.holen(r) {
            HObj::Funktion {
                scope,
                heim,
                eltern_ctor,
                ..
            } => (*scope, *heim, *eltern_ctor),
            _ => {
                let e = self.fehler_wert("TypeError", "Generator-Funktion ohne Scope (intern)");
                return Err(Stoerung::Wurf(e));
            }
        };
        let wurzel_marke = self.wurzel_merke();
        let param_umg = if !f_def.param_einfach {
            let p = self.umgebung_neu(scope).map_err(Stoerung::Fehler)?;
            Some(p)
        } else {
            None
        };
        let umg = match param_umg {
            Some(p) => self.umgebung_neu(Some(p)).map_err(Stoerung::Fehler)?,
            None => self.umgebung_neu(scope).map_err(Stoerung::Fehler)?,
        };
        // Parameter binden (wie `rahmen_starten_fn`).
        let name_umg = param_umg.unwrap_or(umg);
        if let Some(n) = &f_def.selbst_name {
            let _ = self.deklariere(name_umg, n, BindungsArt::Let);
            self.initialisiere(name_umg, n, GVal::Ref(r));
        }
        // Release 09.10.2026: nur nach dem Hash (siehe `Funktion::var_hashes`).
        for &h in f_def.var_hashes.iter() {
            if let Err(e) = self.deklariere_h(umg, h, BindungsArt::Var) {
                self.umgebung_entfernen(umg);
                return Err(Stoerung::Fehler(e));
            }
        }
        // Argumente in die Parameter schreiben (ES2023 9.2.12 Schritt 12).
        // Der Compiler hat `Op::DefArg` / `Op::DefVar` fuer Parameter emittiert.
        // Hier binden wir sie direkt in die Umgebung.
        // `AufrufKopf` hat keine `parameter` / `param_namen` — wir holen die
        // volle `Funktion` aus dem Programm.
        let (param_anz, param_namen, namen) = self
            .programm
            .as_ref()
            .and_then(|p| p.funktionen.get(fn_index))
            .map(|f| (f.parameter as usize, f.param_namen.clone(), f.namen.clone()))
            .unwrap_or((0, Vec::new(), Default::default()));
        for (i, arg) in args.iter().copied().take(param_anz).enumerate() {
            if let Some(ni) = param_namen.get(i).copied() {
                if let Some(n) = namen.get(ni as usize) {
                    let _ = self.deklariere(name_umg, n, BindungsArt::Let);
                    self.initialisiere(name_umg, n, arg);
                }
            }
        }
        let basis = 0; // Generator hat eigenen Stapel
        let rahmen_id = self.rahmen_id_holen();
        let rahmen = Rahmen {
            // BILD_29 (Befreiung fuer QUIC): `fortgesetzt` = „der Generator lief schon
            // vor diesem Schritt“. Beim Anlegen einer Frame hat er noch nicht gelaufen
            // (`Start`) — also `false`. Ohne diese Zeile ist `narsil-jsvm` nicht baubar
            // und damit kein Test in `narsil` pruefbar.
            fortgesetzt: false,
            wurzel_marke,
            id: rahmen_id,
            fun: fn_index,
            zeile: self.funktions_zeile(fn_index),
            code: f_def.code.clone(),
            umg,
            param_umg,
            prolog_rumpf: None,
            basis,
            this,
            strikt: f_def.strikt || self.strikt,
            heim,
            eltern_ctor,
            handler: Vec::new(),
            fertig: Vec::new(),
            temps: vec![GVal::Undefiniert; f_def.temps as usize],
            args: args.to_vec(),
            neu: GVal::Undefiniert,
            ist_konstruktor: false,
            // Release 10.10.2026: ein Generator/`async`-Rumpf ist nie ein
            // Konstruktor — `this` steht beim Aufruf fest. GEMESSEN vorher:
            // `async connectedCallback(){ await super.connectedCallback() }`
            // einer abgeleiteten Klasse warf „this ist noch nicht
            // initialisiert" (heise.de: 180 Custom Elements).
            this_gesetzt: true,
            aufnahme: None,
            ertrag_stern: None,
            // W17: **hier** gehoert der Rahmen zum Generator `r`. Das ist
            // der ganze Fix fuer `yield`: `Engine::ertrag` fand vorher nur
            // `self.wurzeln.last()` und damit meist das Falsche.
            generator: Some(r),
            super_kind: false,
            iter_lauf: Vec::new(),
            stapel: Vec::new(),
            pc: 0,
        };
        // 2. Generator-Objekt im Heap anlegen (ES2023 27.3.1).
        // Release 15.10.2026: its prototype is the function's `prototype`
        // when that is an object (OrdinaryCreateFromConstructor), else
        // %GeneratorPrototype%.
        let gen_proto = match self.eigene_lese(r, "prototype") {
            Some(GVal::Ref(p)) if !f_def.asynchron && self.ist_objekt(GVal::Ref(p)) => p,
            _ => self.proto_generator,
        };
        let gen_obj = HObj::Generator {
            proto: Some(gen_proto),
            zustand: GenZustand::Start,
            frames: vec![rahmen],
            ertrag: None,
            ertrags_art: 0,
            versprechen: None,
            ertrag_stern: None,
            stapel_hoch: 0,
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = crate::heap::Heap::kosten_fuer(&gen_obj);
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        let gen_ref = self.belege(gen_obj, kosten);
        // ## W17 (JS, 04.10.2026) — DER RAHMEN KENNT SEINEN GENERATOR
        //
        // `Rahmen::generator` war beim Anlegen auf `Some(r)` gesetzt, und `r`
        // ist die **Funktion**, nicht der Generator: das
        // `HObj::Generator`-Objekt entsteht **hier**, zwei Zeilen weiter,
        // und bekommt erst dabei seinen Heap-Slot. Der Rahmen war also im
        // Moment seiner Erzeugung nicht in der Lage, sich selbst zu benennen.
        //
        // GEMESSEN, der Fehler war dadurch unsichtbar:
        //     `DIAG ertrag:    v=Zahl(1.0) gen_ref=Some(HeapRef(1118))`
        //     `DIAG schritt5:  gen_ref=HeapRef(1120) ertrag-Feld=None`
        // Zwei verschiedene Slots. `ertrag()` schrieb in **1118** (das ist
        // die Funktion — kein `HObj::Generator`, das `if let` traf nicht zu),
        // und `next()` las aus **1120**. Der `yield`-Wert kam also in einem
        // anderen Objekt an als dem, aus dem gelesen wird.
        //
        // Die Nachtrag-Reparatur ist noetig, weil der Rahmen **schon** im
        // Generator steckt (`frames: vec![rahmen]`), bevor `gen_ref` feststeht.
        if let HObj::Generator { frames, .. } = self.heap.holen_mut(gen_ref)
            && let Some(r0) = frames.first_mut()
        {
            r0.generator = Some(gen_ref);
        }
        self.wurzel_zurueck(wurzel_marke);
        Ok(GVal::Ref(gen_ref))
    }

    pub(crate) fn generator_prolog_anhalten(&mut self) {
        // OFFEN: haelt den Generator nach dem Parameterprolog an
        // (`Op::GeneratorPrologEnde`, J44).
    }

    /// `n_generator_schritt(this, args, art)`: einen Generator-Schritt
    /// ausfuehren (`next`/`throw`/`return`).
    ///
    /// `art`: 0 = `next`, 1 = `throw`, 2 = `return` (wie `Nativ::GeneratorSchritt`).
    ///
    /// GEMESSEN (Ursachenliste): **39** Suite-Faelle scheiterten an
    /// `Wiederaufbau: Engine::n_generator_schritt fehlt noch` — die naechste
    /// grosse Gruppe nach `generator_starten` (57 weg).
    pub(crate) fn n_generator_schritt(
        &mut self,
        this: GVal,
        args: &[GVal],
        art: u8,
    ) -> Result<GVal, Stoerung> {
        let gen_ref = match this {
            GVal::Ref(r) => r,
            _ => {
                let e = self.fehler_wert("TypeError", "Generator-Schritt auf Nicht-Generator");
                return Err(Stoerung::Wurf(e));
            }
        };
        // ## W22 (JS, 05.10.2026) — `gen.return(v)`
        //
        // GEMESSEN in node 22, die vier Randfaelle, nach denen sich diese
        // Funktion richtet (der ganze Abschnitt ist an ihnen gebaut):
        // ```text
        // R1 return(7)          -> {"value":99,"done":false}  (haelt im finally an)
        // R2 next() danach      -> {"value":7,"done":true}    (schliesst)
        // R3 return(7), next(5) -> {"value":7,"done":true}    (die 5 wird verworfen)
        // R5 zweites return(8)  -> {"value":8,"done":true}    (schliesst sofort)
        // R7 try{return 'a'}finally{} -> {"value":"a","done":true}
        // ```
        // Generator-Objekt holen und pruefen
        let (frames_len, ist_fertig, zustand_vorher, ertrags_art_vorher) = {
            let gen_obj = self.heap.holen(gen_ref);
            match gen_obj {
                HObj::Generator {
                    frames,
                    zustand,
                    ertrags_art,
                    ..
                } => (
                    frames.len(),
                    *zustand == GenZustand::Fertig,
                    *zustand,
                    *ertrags_art,
                ),
                _ => {
                    let e = self.fehler_wert("TypeError", "Generator-Schritt auf Nicht-Generator");
                    return Err(Stoerung::Wurf(e));
                }
            }
        };
        // ## W22 (JS, 05.10.2026) — „KEIN RAHMEN" HEISST **NICHT** IMMER „FERTIG"
        //
        // **DAS IST EIN FOLGEFEHLER VON W20**, und er ist genau die
        // Zweideutigkeit, die W20 beseitigen sollte: W20 legt einen
        // **verbrauchten** Rahmen **nicht** mehr in `frames`. Damit hat ein
        // **fertiger** Generator `frames.len() == 0` — und diese Zeile stand
        // **vor** der `ist_fertig`-Pruefung. Also bekam jeder fertige
        // Generator `undefined` statt `{done: true, value: undefined}`.
        //
        // GEMESSEN, vorher (`gen.return`, danach gefixt):
        // ```text
        // D22 ein: art=2 frames=0 zustand=Fertig
        //   -> TypeError: kein Zugriff auf Undefiniert    (.value auf undefined)
        // node 22 -> 9/true
        // ```
        //
        // **Die Regel:** der **Zustand** entscheidet, nicht die Rahmenanzahl
        // (derselbe Satz wie in W20 — nur diesmal eine Ebene hoeher).
        if frames_len == 0 && !ist_fertig {
            // Generator ohne Rahmen (sollte nicht vorkommen)
            return Ok(GVal::Undefiniert);
        }
        if ist_fertig {
            // Generator schon fertig -> {done: true, value: undefined}
            let erg = self
                .neues_objekt(Some(self.proto_objekt))
                .map_err(Stoerung::Fehler)?;
            if let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen_mut(erg) {
                eigenschaften.insert(
                    "done".to_string(),
                    crate::heap::Eigenschaft {
                        wert: GVal::Bool(true),
                        getter: None,
                        setter: None,
                        aufzählbar: true,
                        beschreibbar: true,
                        konfigurierbar: true,
                    },
                );
                eigenschaften.insert(
                    "value".to_string(),
                    crate::heap::Eigenschaft {
                        wert: GVal::Undefiniert,
                        getter: None,
                        setter: None,
                        aufzählbar: true,
                        beschreibbar: true,
                        konfigurierbar: true,
                    },
                );
            }
            return Ok(GVal::Ref(erg));
        }
        // ## W22 (JS, 05.10.2026) — EIN ZWEITES `return` SCHLIESST SOFORT
        //
        // `ertrags_art == 2` heisst: eine Rueckgabe ist **unterwegs**, der
        // Generator haelt in einem `finally` (oder ist schon geschlossen).
        // GEMESSEN in node 22:
        //     function* d(){ try { yield 1 } finally { yield 99 } }
        //     a.return(7) -> {"value":99,"done":false}
        //     a.return(8) -> {"value":8,"done":true}   <- **kein** zweites
        //                                                      finally, sofort
        //     a.throw(e)  -> **wirft** raus               <- siehe gleich
        //
        // ES2023 27.3.5 Schritt 2: bei `suspended-start` gibt es nur den
        // Abschluss zurueck. Der `finally`-Rumpf wird **nicht** erneut
        // betreten.
        let rueckgabe_unterwegs = ertrags_art_vorher == 2;
        if rueckgabe_unterwegs && art != 0 {
            let w = args.get(0).copied().unwrap_or(GVal::Undefiniert);
            // Ein **Wurf** in diesem Zustand verlaesst den Generator (in node
            // gemessen: `throw` wirft raus, es liefert kein Ergebnis-Objekt).
            if art == 1 {
                return Err(Stoerung::Wurf(w));
            }
            // Ein **zweites** `return` schliesst sofort mit dem neuen Wert.
            //
            // `art == 0` ist hier **nicht** dabei: GEMESSEN in node 22 laeuft
            // ein `next()` in diesem Zustast **weiter** — es setzt die
            // `finally`-Kette fort und liefert danach den zurueckgestellten
            // Wert (R2: `{"value":7,"done":true}`, R8: `{"value":98,"done":false}`).
            // Der normale Weg weiter unten erledigt das; der gestellte Wert
            // wird dabei verworfen (R3).
            if let HObj::Generator {
                zustand, frames, ..
            } = self.heap.holen_mut(gen_ref)
            {
                *zustand = GenZustand::Fertig;
                frames.clear();
            }
            let erg = self
                .neues_objekt(Some(self.proto_objekt))
                .map_err(Stoerung::Fehler)?;
            if let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen_mut(erg) {
                for (name, v) in [("done", GVal::Bool(true)), ("value", w)] {
                    eigenschaften.insert(
                        name.to_string(),
                        crate::heap::Eigenschaft {
                            wert: v,
                            getter: None,
                            setter: None,
                            aufzählbar: true,
                            beschreibbar: true,
                            konfigurierbar: true,
                        },
                    );
                }
            }
            return Ok(GVal::Ref(erg));
        }
        // Wert fuer `next`/`throw`/`return` vorbereiten
        let (wert, ist_wurf) = match art {
            0 => (args.get(0).copied().unwrap_or(GVal::Undefiniert), false), // next(value)
            1 => (args.get(0).copied().unwrap_or(GVal::Undefiniert), true),  // throw(value)
            2 => (args.get(0).copied().unwrap_or(GVal::Undefiniert), false), // return(value)
            _ => (GVal::Undefiniert, false),
        };
        // W22: `art == 2` ist eine **Rueckgabe**, kein `next`. Sie startet die
        // `finally`-Kette ueber `rueckgabe_abwickeln` und stellt **keinen**
        // Wert zum Fortsetzen bereit — der `finally`-Rumpf darf den
        // aufgerufenen Wert nicht sehen.
        let ist_rueckgabe = art == 2;
        // Den obersten Generator-Rahmen holen und in den VM-Stapel schieben
        // Wir arbeiten direkt mit dem Heap-Objekt, um Borrow-Probleme zu vermeiden.
        // 1. Rahmen aus dem Generator entnehmen
        let mut gen_rahmen = {
            let gen_obj = self.heap.holen_mut(gen_ref);
            if let HObj::Generator { frames, .. } = gen_obj {
                frames.pop().unwrap()
            } else {
                unreachable!()
            }
        };
        // 2. VM-Zustand sichern
        //
        // W18 (JS, 04.10.2026): **hier** entsteht das Fortsetzungs-Merkmal.
        // `GenZustand::Start` heisst „der Generator lief noch nie" — dann ist
        // der jetzt ausgefuehrte Schritt der **Start**, und `Op::Ertrag` muss
        // den `yield`-Wert vom Stapel **lesen**. Ab da ist jeder weitere
        // Schritt eine Fortsetzung, und der vom Aufrufer gestellte Wert liegt
        // schon auf dem Stapel.
        //
        // GEMESSEN (vorher): `Op::Ertrag` fragte stattdessen
        // `aufnahme.is_some()` — und das ist **immer** wahr, weil
        // `n_generator_schritt` `aufnahme` bei **jedem** Schritt setzt (auch
        // bei `next()` ohne Argument). Folge bei **jedem** Generator:
        //     `g().next().value` -> `undefined`   (node 22: `1`)
        //     `g().next().done`  -> `false`       (richtig)
        gen_rahmen.fortgesetzt = !matches!(zustand_vorher, GenZustand::Start);
        // ## W22 — EIN FRISCHES `return` SETZT **NICHT** FORT
        //
        // `rueckgabe_abwickeln` springt in den `finally`-Rumpf. Was dort
        // steht, ist ein **neues `yield`**, kein resumed — es hat noch
        // nichts vom Aufrufer bekommen. Mit `fortgesetzt == true` wtog
        // `Op::Ertrag` in den Fortsetzungszweig, schob `undefined` auf den
        // Stapel und **lief weiter**: der `finally`-Ertrag wurde uebersprungen.
        //
        // GEMESSEN (node 22 als Soll in Klammern):
        // ```text
        // function* d(){ try { yield 1 } finally { yield 99 } }
        // i.return(7)   node: {"value":99,"done":false}   bei uns: 7  (also uebersprungen)
        // i.next()      node: {"value":7,"done":true}
        // ```
        //
        // **UMGEKEHRT** bleibt es richtig: ein `next()` waehrend der laufenden
        // Rueckgabe (`rueckgabe_unterwegs`) setzt `fortgesetzt == true`, weil
        // der `finally`-Rumpf dann **weiterlaeuft** — node, R8:
        // `return(7) -> {"value":99,...}`, `next() -> {"value":98,...}`.
        if ist_rueckgabe {
            gen_rahmen.fortgesetzt = false;
        }
        let alt_stapel = std::mem::take(&mut self.stapel);
        let alt_rahmen = std::mem::take(&mut self.rahmen);
        let alt_wurzeln = std::mem::take(&mut self.wurzeln);
        let alt_frisch = std::mem::take(&mut self.frisch);
        let alt_stapel_laenge = alt_stapel.len();
        self.ausgelagert.push((alt_stapel, alt_rahmen, alt_wurzeln, alt_frisch));
        // 3. Generator-Rahmen in VM uebernehmen (seine Wurzelmarke gilt fuer
        //    die NEUE, leere Wurzelliste).
        let mut gen_rahmen = gen_rahmen;
        gen_rahmen.wurzel_marke = self.wurzel_merke();
        self.rahmen.push(gen_rahmen);
        // Generator-Stapel wiederherstellen
        let rahmen = self.rahmen.last_mut().unwrap();
        self.stapel = rahmen.stapel.clone();
        // Wurzeln: Generator-Objekt schuetzen
        self.wurzeln.push(GVal::Ref(gen_ref));
        // `aufnahme` fuer `next`/`throw`/`return` setzen
        //
        // W22: bei einer **Rueckgabe** (`art == 2`) und bei einer
        // **unterwegs** gesetzten (`ertrags_art == 2`) wird **kein** Wert
        // bereitgestellt — der vom Aufrufer gestellte Wert wird verworfen.
        // In node gemessen (R3): `return(7)` haelt mit 99, `next(5)` liefert
        // danach `{7,true}` — die **5** kommt nirgends hin.
        rahmen.aufnahme = if ist_rueckgabe || rueckgabe_unterwegs {
            None
        } else if ist_wurf {
            Some(crate::vm::Aufnahme::Wurf(wert))
        } else {
            Some(crate::vm::Aufnahme::Wert(wert))
        };
        // 4. VM-Schleife ausfuehren (bis Rahmen anhaelt/endet)
        //
        // W22: eine **Rueckgabe** startet zuerst die `finally`-Kette. Ohne das
        // wuerde `return` einfach wie ein `next` behandelt — GEMESSEN vorher:
        // `i.return(9)` lieferte `2` (den Wert des naechsten `yield`) statt `9`.
        let lauf_ende_gen = self.rahmen.len() - 1;
        let ergebnis = if ist_rueckgabe {
            let alt = self.lauf_ende;
            self.lauf_ende = lauf_ende_gen;
            let r = match self.rueckgabe_abwickeln(wert) {
                Ok(()) => self.schleife(lauf_ende_gen),
                // Kein `finally` mehr: der Rahmen ist weg und `wert` ist die
                // Rueckgabe des Generators. Das ist **kein** Fehler — der
                // Wurf wird unten als `{done:true, value:wert}` geliefert.
                Err(e) => e,
            };
            self.lauf_ende = alt;
            r
        } else {
            self.schleife(lauf_ende_gen)
        };
        // 5. Generator-Rahmen zurueck in den Generator speichern
        // Der Rahmen wurde evtl. schon von `schleife`/`rahmen_ende` entfernt
        // (wenn der Generator fertig ist). In dem Fall holen wir ihn nicht
        // mehr vom Stapel, sondern der Generator-Zustand wurde schon
        // aktualisiert.
        // ## W20 (JS, 04.10.2026) — DER ZUSTAND ENTSCHEIDET, NICHT DIE ANZAHL
        //
        // Die alte Bedingung war `self.rahmen.len() > alt_rahmen.len()`.
        // Sie ist **strukturell falsch**, nicht bloss fragil: `alt_rahmen`
        // haelt die Rahmen des **Aufrufers**, und die stehen per
        // `std::mem::take` gar nicht mehr in `self.rahmen` — der
        // Generator-Rahmen ist dort der **einzige**. Also ist nach dem
        // Anhalten `len == alt == 1`, `1 > 1` ist **falsch**, und es wurde
        // jedes Mal der **Dummy** genommen. Genau das ist die Messung:
        //
        //     DBG rueck: len=1 alt=1 ergebnis="Angehalten"
        //     DBG rueck: len=0 alt=1 ergebnis="Fertig"
        //
        // Der zweite Lauf sieht dann `len=0`, weil der Dummy-`Rahmen` mit
        // `code: Arc::new(Vec::new())` aufgesetzt wird, `pc >= code.len()`
        // gilt (0 >= 0), `rahmen_ende` laeuft — und der Generator gilt als
        // **fertig**. Aus einem falsch behandelten Anhalten wurde so ein
        // falscher Abschluss.
        //
        // GEMESSENE FOLGE (node 22 als Soll in Klammern):
        //   `var x = yield 1; return x*2` + `next(21)` -> `undefined`  (42)
        //   `yield 1; yield 2` als Schleife                     -> `1`     (12)
        //   `for(i<3) s += yield i`                             -> `0`     (3)
        //
        // **Die Regel, die jetzt gilt:** `Angehalten` heisst „der Rahmen
        // haelt noch auf dem VM-Stapel und wird **zurueckgelegt**"; alles
        // andere heisst „der Rahmen ist verbraucht". Keine Zaehlung.
        let rahmen_haengt_noch = matches!(ergebnis, LaufErgebnis::Angehalten);
        let mut gen_rahmen = if rahmen_haengt_noch {
            self.rahmen.pop().unwrap()
        } else {
            // Generator ist fertig - der Rahmen wurde schon entfernt.
            // Wir erzeugen einen Dummy-Rahmen, der nicht verwendet wird.
            Rahmen {
                // BILD_29 (Befreiung fuer QUIC): `fortgesetzt` = „der Generator lief schon
                // vor diesem Schritt“. Beim Anlegen einer Frame hat er noch nicht gelaufen
                // (`Start`) — also `false`. Ohne diese Zeile ist `narsil-jsvm` nicht baubar
                // und damit kein Test in `narsil` pruefbar.
                fortgesetzt: false,
                wurzel_marke: 0,
                id: 0,
                fun: 0,
                zeile: 0,
                code: std::sync::Arc::new(Vec::new()),
                umg: KEIN_REF,
                param_umg: None,
                prolog_rumpf: None,
                basis: 0,
                this: GVal::Undefiniert,
                strikt: false,
                heim: None,
                eltern_ctor: None,
                handler: Vec::new(),
                fertig: Vec::new(),
                temps: Vec::new(),
                args: Vec::new(),
                neu: GVal::Undefiniert,
                ist_konstruktor: false,
                this_gesetzt: false,
                aufnahme: None,
                ertrag_stern: None,
                // W17: der Dummy in `n_generator_schritt` wird nicht benutzt.
                generator: None,
                super_kind: false,
                iter_lauf: Vec::new(),
                stapel: Vec::new(),
                pc: 0,
            }
        };
        // `Fehler` (ein interner `JsFehler`) zuerst in einen Wurf umrechnen —
        // so ist `ergebnis` danach nur noch `Fertig`/`Wurf`/`Angehalten` mit
        // `Copy`-Nutzlast, und der Generator-Zustand laesst sich ohne zweiten
        // Zugriff auf `self` setzen.
        let ergebnis = match ergebnis {
            LaufErgebnis::Fehler(e) => {
                LaufErgebnis::Wurf(self.fehler_wert("TypeError", &e.to_string()))
            }
            other => other,
        };
        {
            let gen_obj = self.heap.holen_mut(gen_ref);
            if let HObj::Generator {
                frames,
                zustand,
                ertrag,
                ertrags_art,
                stapel_hoch,
                ..
            } = gen_obj
            {
                // ## W19 (JS, 04.10.2026) — DER STAPEL DES RAHMENS
                //
                // `Rahmen::stapel` wurde **nie** zurueckgeschrieben. Beim
                // Anhaengen wurde `self.stapel = rahmen.stapel.clone()`
                // gelesen (Zeile 5181) — aber in die andere Richtung gab es
                // keinen Schritt, also war der Generator-Stapel nach jedem
                // `next()` wieder der vom **Anlegen** (leer). Alles, was der
                // Rumpf zwischen zwei `yield` aufgelegt hatte, war weg.
                //
                // Das ist der Grund, warum die Fortsetzung auch nach dem
                // `pc`-Zuruecksetzen nicht wirkte: der Rumpf hatte seine
                // Zwischenwerte nicht mehr.
                //
                // GEMESSEN vorher (node 22 als Soll in Klammern):
                //   `var x = yield 1; return x*2` + `next(21)` -> `undefined` (42)
                //   `yield 1; yield 2` als Schleife                     -> `1`  (12)
                //   `for(i<3) s += yield i`                             -> `0`  (3)
                if matches!(ergebnis, LaufErgebnis::Angehalten) {
                    gen_rahmen.stapel = self.stapel.clone();
                }
                // W20: **nur** den haengenden Rahmen zuruecklegen. Ein
                // verbrauchter (fertiger) Rahmen wird **nicht** gespeichert —
                // sonst waere `frames` nach dem ersten `next()` nicht leer und
                // jeder weitere Schritt holte einen **alten** Rahmen.
                if rahmen_haengt_noch {
                    frames.push(gen_rahmen);
                }
                *stapel_hoch = alt_stapel_laenge;
                // ## W22 (JS, 05.10.2026) — `ertrags_art`: DIE ART DES ABSCHLUSSES
                //
                // `ertrags_art == 2` bedeutet: eine **Rueckgabe** ist unterwegs.
                // Das Feld stand in `heap.rs` bereit („0 = `yield`, 1 = `yield*`,
                // 2 = `return`") und wurde **nicht** beschrieben — deshalb wusste
                // der naechste Schritt nicht, dass er in einer `finally`-Kette
                // weiterlaufen muss.
                //
                // GEMESSEN in node 22:
                // ```text
                // R1 return(7)      -> {"value":99,"done":false}   <- haelt an
                // R2 next() danach   -> {"value":7,"done":true}     <- schliesst
                // R5 zweites return  -> {"value":8,"done":true}     <- schliesst sofort
                // ```
                // Der Wert 7 lebt in `Aktion::Wurf` **im Rahmen**; dieses Merkmal
                // sagt nur, **dass** ein Rueckgabe-Abschluss unterwegs ist. Sobald
                // der Generator `Fertig` ist, wird es zurueckgesetzt — ein fertiger
                // Generator nimmt kein `return` mehr an.
                *ertrags_art = if ist_rueckgabe || rueckgabe_unterwegs { 2 } else { 0 };
                match ergebnis {
                    LaufErgebnis::Fertig(v) => {
                        *zustand = GenZustand::Fertig;
                        *ertrag = Some(v);
                        *ertrags_art = 0;
                    }
                    LaufErgebnis::Wurf(w) => {
                        *zustand = GenZustand::Fertig;
                        *ertrag = Some(w);
                        *ertrags_art = 0;
                    }
                    LaufErgebnis::Angehalten => {
                        *zustand = GenZustand::Angehalten;
                    }
                    LaufErgebnis::Fehler(_) => {}
                }
            }
        }
        // 6. VM-Zustand wiederherstellen
        let (alt_stapel, alt_rahmen, alt_wurzeln, alt_frisch) =
            self.ausgelagert.pop().expect("ausgelagerter Aufrufer (intern)");
        self.stapel = alt_stapel;
        self.rahmen = alt_rahmen;
        self.wurzeln = alt_wurzeln;
        self.frisch = alt_frisch;
        // ES2023 27.3.5 `GeneratorResumeAbrupt`: ein **Wurf** im Rumpf
        // **propagiert** aus `next()`/`throw()` heraus — er wird NICHT zu
        // einem normalen `{done:true, value:<wurf>}`.
        //
        // GEMESSEN (18 test262-Faelle, `class/dstr` + `generators/dstr`): die
        // erste Fassung schluckte den Wurf (`*ertrag = Some(w)`, `done=true`).
        // Ein `assert.sameValue(…)` **im Generatorrumpf** blieb dadurch
        // folgenlos, und das `callCount`-Assert am Testende meldete dann
        // «0» statt «1» — die Fehlergruppe sah nach einem Fehler im
        // Aufrufzerg aus, lag aber im Rumpf.
        if let LaufErgebnis::Wurf(w) = ergebnis
            && !ist_rueckgabe
            && !rueckgabe_unterwegs
        {
            return Err(Stoerung::Wurf(w));
        }
        // ## W22 — EIN RUECKGABE-ABSCHLUSS IST **KEIN** WURF
        //
        // Die `finally`-Kette endet mit `Aktion::Wurf(w)`, und der Wurf
        // verlaesst den letzten Rahmen — also kommt er als `LaufErgebnis::Wurf`
        // zurueck. Bei `gen.throw(x)` ist das **richtig** (der Wurf gehoert nach
        // aussen, ES2023 27.3.5). Bei `gen.return(v)` ist derselbe Wurf die
        // **Rueckgabe**: `{done: true, value: v}`.
        //
        // GEMESSEN vorher, mit dem halben W22 (die `finally`-Kette lief schon):
        // ```text
        // i.return(9)     -> !Laufzeitfehler: Unbehandelt: 9    <- der Wert stimmte
        // node 22         ->  9/true                            <- aber so nicht
        // ```
        // 7. Ergebnis-Objekt bauen: {done, value}
        let erg = self
            .neues_objekt(Some(self.proto_objekt))
            .map_err(Stoerung::Fehler)?;
        let (done, value) = match ergebnis {
            LaufErgebnis::Fertig(v) | LaufErgebnis::Wurf(v) => (true, v),
            LaufErgebnis::Angehalten => {
                let gen_obj2 = self.heap.holen(gen_ref);
                if let HObj::Generator {
                    ertrag: Some(v), ..
                } = gen_obj2
                {
                    (false, *v)
                } else {
                    (false, GVal::Undefiniert)
                }
            }
            LaufErgebnis::Fehler(_) => (true, GVal::Undefiniert),
        };
        if let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen_mut(erg) {
            eigenschaften.insert(
                "done".to_string(),
                crate::heap::Eigenschaft {
                    wert: GVal::Bool(done),
                    getter: None,
                    setter: None,
                    aufzählbar: true,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
            eigenschaften.insert(
                "value".to_string(),
                crate::heap::Eigenschaft {
                    wert: value,
                    getter: None,
                    setter: None,
                    aufzählbar: true,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
        }
        Ok(GVal::Ref(erg))
    }

    /// ## W23 (JS, 05.10.2026) — EINE async-FUNKTION IST EIN GENERATOR, DER
    /// AUF EIN **VERSPRECHEN** WARTET
    ///
    /// Bis hierher stand hier `self.fehlt_noch("async_starten")`. Die drei
    /// Einstiegsfunktionen der Promise-Seite, um die ich seit **JS_13**
    /// gebeten hatte, waren **alle** schon im Baum — sie waren nur nicht
    /// benutzt:
    ///   * `versprechen_anlegen(Offen)`  — das eigene Versprechen (`klassen.rs:82`)
    ///   * `versprechen_settle(z, erf, w)` — erfuellen/ablehnen (`klassen.rs:250`)
    ///   * `self.mikrotasks` — die Mikrotask-Schlange (`interp.rs:359`)
    ///
    /// Der Rahmen liegt in einem **`HObj::Generator`-Container**, genau wie bei
    /// einem Generator — mit `versprechen: Some(p)`. Das ist kein Umweg, das
    /// ist derselbe Mechanismus: ein Generator haelt an und wird fortgesetzt,
    /// ein async-Rumpf haelt am `await` an und wird aus der Schlange
    /// fortgesetzt. W19/W20 haben **beides** vorher gebaut.
    ///
    /// GEMESSEN in node 22 — der Rumpf laeuft **synchron bis zum ersten
    /// `await`**, alles Weitere wartet auf die Mikrotasks:
    /// ```text
    /// async function f(){ s.push("a"); const v = await 1; s.push("b"+v); return 9 }
    /// const p = f(); s.push("sync"); p.then(v => s.push("t"+v)); s.push("sync2");
    /// -> ["a","sync","sync2","b1","t9"]
    /// ```
    /// `"a"` steht **vor** `"sync"` — der erste Teil laeuft also sofort. Und
    /// `f()` gibt **ein Versprechen** zurueck, nicht den Wert (Fall 10:
    /// `[true,"function"]`).
    pub(crate) fn async_starten(
        &mut self,
        r: HeapRef,
        fn_index: usize,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        // 1. Das eigene Versprechen (ES2023 27.7.5.3 Schritt 1).
        let promise = match self.versprechen_anlegen(crate::heap::VersprechenZustand::Offen) {
            GVal::Ref(p) => p,
            _ => {
                let e = self.fehler_wert("TypeError", "Versprechen nicht anlegbar (intern)");
                return Err(Stoerung::Wurf(e));
            }
        };
        // 2. Der Container fuer den angehaltenen Rahmen. Er ist **kein**
        //    Generator, den jemand von aussen sieht — deshalb `proto: None`.
        let container_obj = HObj::Generator {
            proto: None,
            zustand: GenZustand::Start,
            frames: Vec::new(),
            ertrag: None,
            ertrags_art: 0,
            versprechen: Some(promise),
            ertrag_stern: None,
            stapel_hoch: 0,
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = crate::heap::Heap::kosten_fuer(&container_obj);
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        let container = self.belege(container_obj, kosten);
        // 3. Der Rahmen — dieselbe Anlage wie `generator_starten`, mit **einem**
        //    Unterschied: `generator` zeigt auf den **Container**, damit
        //    `erwartung` ihn findet (W17 hat genau diesen Weg gebaut).
        let f_def = self.aufruf_kopf(fn_index).map_err(Stoerung::Fehler)?;
        let (scope, heim, eltern_ctor) = match self.heap.holen(r) {
            HObj::Funktion {
                scope,
                heim,
                eltern_ctor,
                ..
            } => (*scope, *heim, *eltern_ctor),
            _ => {
                let e = self.fehler_wert("TypeError", "async-Funktion ohne Scope (intern)");
                return Err(Stoerung::Wurf(e));
            }
        };
        let wurzel_marke = self.wurzel_merke();
        let param_umg = if !f_def.param_einfach {
            let p = self.umgebung_neu(scope).map_err(Stoerung::Fehler)?;
            Some(p)
        } else {
            None
        };
        let umg = match param_umg {
            Some(p) => self.umgebung_neu(Some(p)).map_err(Stoerung::Fehler)?,
            None => self.umgebung_neu(scope).map_err(Stoerung::Fehler)?,
        };
        let name_umg = param_umg.unwrap_or(umg);
        if let Some(n) = &f_def.selbst_name {
            let _ = self.deklariere(name_umg, n, BindungsArt::Let);
            self.initialisiere(name_umg, n, GVal::Ref(r));
        }
        // Release 09.10.2026: nur nach dem Hash (siehe `Funktion::var_hashes`).
        for &h in f_def.var_hashes.iter() {
            if let Err(e) = self.deklariere_h(umg, h, BindungsArt::Var) {
                self.umgebung_entfernen(umg);
                return Err(Stoerung::Fehler(e));
            }
        }
        let (param_anz, param_namen, namen) = self
            .programm
            .as_ref()
            .and_then(|p| p.funktionen.get(fn_index))
            .map(|f| (f.parameter as usize, f.param_namen.clone(), f.namen.clone()))
            .unwrap_or((0, Vec::new(), Default::default()));
        for (i, arg) in args.iter().copied().take(param_anz).enumerate() {
            if let Some(ni) = param_namen.get(i).copied() {
                if let Some(n) = namen.get(ni as usize) {
                    let _ = self.deklariere(name_umg, n, BindungsArt::Let);
                    self.initialisiere(name_umg, n, arg);
                }
            }
        }
        let rahmen = Rahmen {
            fortgesetzt: false,
            wurzel_marke,
            id: self.rahmen_id_holen(),
            fun: fn_index,
            zeile: self.funktions_zeile(fn_index),
            code: f_def.code.clone(),
            umg,
            param_umg,
            prolog_rumpf: None,
            basis: 0,
            this,
            strikt: f_def.strikt || self.strikt,
            heim,
            eltern_ctor,
            handler: Vec::new(),
            fertig: Vec::new(),
            temps: vec![GVal::Undefiniert; f_def.temps as usize],
            args: args.to_vec(),
            neu: GVal::Undefiniert,
            ist_konstruktor: false,
            // Release 10.10.2026: wie beim Generator — `async` ist nie ein
            // Konstruktor, `this` steht fest (heise.de, siehe dort).
            this_gesetzt: true,
            aufnahme: None,
            ertrag_stern: None,
            // W23: der **Container**, nicht die Funktion. `erwartung` findet
            // hier den Rahmen und reiht die Fortsetzung darauf ein.
            generator: Some(container),
            super_kind: false,
            iter_lauf: Vec::new(),
            stapel: Vec::new(),
            pc: 0,
        };
        // 4. Der erste Lauf — **synchron**, bis zum ersten `await` oder Ende.
        //
        // Release 10.10.2026: auch der OPERANDENSTAPEL des Aufrufers wird
        // ausgelagert (wie in `async_fortsetzen`). Der Rahmen hat `basis: 0`;
        // auf dem geteilten Stapel raeumte sein `return` die Werte des
        // Aufrufers mit ab. GEMESSEN vorher (node 22 in Klammern):
        //   `1 + (async function(){}(), 2)`        -> NaN  (3)
        //   `[7, async function(){}(), 8].length`  -> TypeError  (3)
        //   `f()(0)` mit async `f`                 -> „Operandenstapel zu
        //       kurz (intern)", nicht fangbar  (TypeError)
        // Mit `await` fiel es nicht auf: der angehaltene Rahmen raeumt nichts.
        let alt_stapel = std::mem::take(&mut self.stapel);
        let alt_rahmen = std::mem::take(&mut self.rahmen);
        self.ausgelagert.push((alt_stapel, alt_rahmen, Vec::new(), Vec::new()));
        self.rahmen.push(rahmen);
        self.wurzeln.push(GVal::Ref(container));
        let ende = self.rahmen.len() - 1;
        let ergebnis = self.schleife(ende);
        let ergebnis = match ergebnis {
            LaufErgebnis::Fehler(e) => {
                LaufErgebnis::Wurf(self.fehler_wert("TypeError", &e.to_string()))
            }
            other => other,
        };
        let haengt = matches!(ergebnis, LaufErgebnis::Angehalten);
        if haengt {
            let mut r = self.rahmen.pop().unwrap();
            r.stapel = std::mem::take(&mut self.stapel);
            if let HObj::Generator { frames, .. } = self.heap.holen_mut(container) {
                frames.push(r);
            }
        } else if let HObj::Generator { zustand, .. } = self.heap.holen_mut(container) {
            *zustand = GenZustand::Fertig;
        }
        let (alt_stapel, alt_rahmen, _, _) = self.ausgelagert.pop().expect("ausgelagerter Aufrufer (intern)");
        self.stapel = alt_stapel;
        self.rahmen = alt_rahmen;
        // 5. Der eigene Abschluss. Ein `throw` **ablehnt** das Versprechen —
        //    es ist kein Wurf nach aussen (der Aufrufer sieht ein abgelehntes
        //    Versprechen, kein Skript-Abbruch). GEMESSEN (Fall 8/14).
        if !haengt {
            match ergebnis {
                LaufErgebnis::Fertig(v) => self.versprechen_settle(promise, true, v),
                LaufErgebnis::Wurf(w) => self.versprechen_settle(promise, false, w),
                _ => {}
            }
        }
        Ok(GVal::Ref(promise))
    }

    /// `ertrag(v, stern)`: `yield` / `yield*` im Generator (ES2023 27.3.3
    /// `GeneratorYield` / `GeneratorYieldStar`).
    ///
    /// Der Generator gibt `v` zurueck und haelt an. Beim naechsten `next()`
    /// wird der Rahmen ab dem naechsten Befehl fortgesetzt.
    ///
    /// `stern` = true fuer `yield*` (Delegation an ein anderes Iterierbares).
    pub(crate) fn ertrag(&mut self, v: GVal, stern: bool) -> Kontrolle {
        // Den aktuellen Rahmen als angehalten markieren und Wert speichern.
        if let Some(rahmen) = self.rahmen.last_mut() {
            rahmen.aufnahme = Some(crate::vm::Aufnahme::Wert(v));
            rahmen.ertrag_stern = if stern {
                Some((GVal::Undefiniert, 0))
            } else {
                None
            };
        }
        // Wert in den Generator schreiben, damit `next()` ihn zurueckgeben
        // kann.
        //
        // ## W17 (JS, 04.10.2026) — DER WEG, UND WARUM DER ALTE FALSCH WAR
        //
        // Vorher stand hier `self.wurzeln.last()` mit dem Kommentar „der
        // Generator-Objekt-Ref ist in `wurzeln`". Das stimmt **unmittelbar
        // nach** dem Schutzen — und **nicht mehr danach**, denn **der Rumpf
        // alloziert**. Jeder `String`, jeder Aufruf, jeder Objektliteral-
        // Wert schiebt in `wurzeln`, also war `wurzeln.last()` beim `yield`
        // in aller Regel ein **anderer** Slot. Das `if let HObj::Generator`
        // traf nicht zu, `ertrag` blieb `None`.
        //
        // GEMESSEN, vorher, bei **jedem** Generator:
        //     `function* g(){ yield 1 }   g().next().value`
        //     -> `undefined`   (node 22: `1`)
        //     `function* g(){ yield 1 }   g().next().done`
        //     -> `false`       (richtig — der Befehl laeuft ja)
        // `done` stimmte, nur der **Wert** kam nicht heraus. Genau das ist
        // die Signatur eines Befehls, der laeuft, aber seinen Wert nicht
        // ablegt.
        //
        // Neu: der Rahmen weiss selbst, zu welchem Generator er gehoert
        // (`Rahmen::generator`, gesetzt in `generator_starten`).
        let gen_ref = self.rahmen.last().and_then(|r| r.generator);
        if let Some(gen_ref) = gen_ref
            && let HObj::Generator { ertrag, .. } = self.heap.holen_mut(gen_ref)
        {
            *ertrag = Some(v);
        }
        // ## W19 (JS, 04.10.2026) — `pc` **auf sich selbst zurueck**
        //
        // Das ist die im Modulkopf beschriebene Fortsetzungsform, und sie war
        // **nicht** gebaut: die Schleife setzt `pc` auf `pc + 1`, **bevor** der
        // Befehl laeuft (vm.rs, `schleife_innen`). Ohne diese Zeile zeigt `pc`
        // hinter den `yield`, und beim Fortsetzen laeuft der **naechste**
        // Befehl — der `yield`-Ausdruck bekommt seinen Wert nie, und es
        // braucht gar kein zweites `Op::Ertrag`.
        //
        // GEMESSEN vorher (node 22 als Soll in Klammern):
        //   `var x = yield 1; return x*2`, `i.next(); i.next(21)` -> `undefined` (42)
        // Nach dem Zuruecksetzen laeuft dasselbe `Op::Ertrag` ein zweites Mal,
        // sieht `Rahmen::fortgesetzt` und liefert den gestellten Wert.
        if let Some(r) = self.rahmen.last_mut()
            && r.pc > 0
        {
            r.pc -= 1;
        }
        Kontrolle::Angehalten
    }

    /// `erwartung(v)`: `await v` in einer async-Funktion (ES2023 27.5.4
    /// `Await`).
    ///
    /// Die async-Funktion haelt an und wartet auf `v` (ein Thenable/
    /// Promise). Wenn `v` erfuellt ist, wird der Rahmen mit dem Wert
    /// fortgesetzt; bei Ablehnung wird der Wert als Ausnahme geworfen.
    pub(crate) fn erwartung(&mut self, v: GVal) -> Kontrolle {
        // ## W23 (JS, 05.10.2026) — `await` HAENGT AN EIN **VERSPRECHEN**
        //
        // Bis hierher war das ein reiner Stub: der Rahmen haelt, und es
        // passiert **nichts** — kein Versprechen, keine Reaktion, und
        // `schlange_abarbeiten` bricht mit „Generator-Fortsetzung aus der
        // Mikrotask-Schlange fehlt noch" ab.
        //
        // **DIE KETTE WAR BEREITS VORBEREITET** (das ist der Grund, warum dieser
        // Schritt klein ist und nicht ein Neubau):
        //   `Reaktion.fortsetzung`  -> `versprechen_settle` schiebt
        //   `Mikrotask::fortsetzung(gen, Aufnahme)` -> und dort wartete nur noch
        //   das Wiederaufsetzen. GEMESSEN vorher: diese drei Stellen waren
        //   vorbereitet und **unbenutzt**.
        let container = self.rahmen.last().and_then(|r| r.generator);
        let Some(container) = container else {
            // `await` ausserhalb einer async-Funktion: das kann der Compiler
            // nicht erzeugen. Ein **lauter** Fehler ist hier richtig — ein
            // stilles `undefined` waere ein Befund, den niemand sieht.
            return Kontrolle::Fehler(JsFehler::Laufzeit(
                "await ausserhalb einer async-Funktion".to_string(),
            ));
        };
        // Das Versprechen, auf das gewartet wird. `await 1` ist laut Norm
        // `PromiseResolve(%Promise%, 1)` — es kostet also **einen**
        // Mikrotask-Schritt, auch bei einem nackten Wert. GEMESSEN in node 22
        // (Fall 5): `[1,"s",2,3,"then:r"]` — `"s"` steht **vor** der 2, das
        // heisst: `await 0` laeuft nicht synchron weiter.
        // Release 15.10.2026: the awaited value (already popped from the
        // stack), the frame's generator and the promise (fresh for `await 1`
        // or a thenable) stay rooted until the reaction is queued — creating
        // the promise and resolving it with a thenable allocate. MEASURED
        // before (chip.de, NARSIL_JS_GC_STRESS=64): "Heap: toter Verweis" here,
        // the fresh promise had been swept.
        let marke = self.wurzel_merke();
        self.wurzeln.push(v);
        self.wurzeln.push(GVal::Ref(container));
        let ziel = self.versprechen_aus_wert(v);
        self.wurzeln.push(GVal::Ref(ziel));
        self.reaktion_fortsetzung_einreihen(ziel, container);
        self.wurzel_zurueck(marke);
        // `pc` auf sich selbst zurueck: die Schleife setzt `pc + 1`, **bevor**
        // der Befehl laeuft. Ohne das zeigt `pc` hinter dem `await`.
        if let Some(r) = self.rahmen.last_mut()
            && r.pc > 0
        {
            r.pc -= 1;
        }
        Kontrolle::Angehalten
    }

    /// `versprechen_aus_wert(v)`: das Versprechen, auf das `await v` wartet.
    ///
    /// GEMESSEN in node 22, die drei Arten (Fall 2, 3, 1):
    /// ```text
    /// await Promise.resolve(1)  -> wartet auf **dieses** Versprechen
    /// await { then(r){ r(7) } } -> der `then`-Aufruf ist ein **eigener**
    ///                              Mikrotask (Fall 3: `"sync","then","v7"`)
    /// await 1                   -> ein erfuelltes Versprechen, **ein** Schritt
    /// ```
    /// Fuer ein Thenable ist der Umweg ueber `versprechen_settle` **richtig**
    /// und nicht nur bequem: `assimilation` erkennt es und reiht den `then`
    /// als `thenable_job` ein, statt es sofort aufzurufen.
    pub(crate) fn versprechen_aus_wert(&mut self, v: GVal) -> HeapRef {
        if let GVal::Ref(r) = v
            && matches!(self.heap.holen(r), HObj::Versprechen { .. })
        {
            return r;
        }
        let GVal::Ref(frisch) = self.versprechen_anlegen(crate::heap::VersprechenZustand::Offen)
        else {
            // `versprechen_anlegen` baut immer ein `HObj::Versprechen`; wenn das
            // je nicht stimmt, ist der Heap kaputt — und dann ist ein lauter
            // Fehler die einzige richtige Antwort.
            return KEIN_REF;
        };
        // Assimilation: bei einem Thenable bleibt `frisch` **offen** und wird
        // spaeter erfuellt; die Reaktion ist trotzdem richtig eingereiht.
        // Release 15.10.2026: `frisch` and `v` rooted while settling (a
        // thenable's `then` getter runs and allocates).
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(frisch));
        self.wurzeln.push(v);
        self.versprechen_settle(frisch, true, v);
        self.wurzel_zurueck(marke);
        frisch
    }

    /// `reaktion_fortsetzung_einreihen(ziel, fortsetzung)`: eine Reaktion, die
    /// **keinen** `then`-Aufruf erledigt, sondern beim Entscheiden des
    /// Versprechens einen **Rahmen fortsetzt**.
    ///
    /// **WARUM EIGENS HIER UND NICHT `reaktion_einreihen`:** `fortsetzung` ist
    /// `None` in *jedem* anderen Aufruf, und `versprechen_settle` schickt es an
    /// `Mikrotask::fortsetzung`. Eine zweite Art `Reaktion` gibt es nicht —
    /// `art` ist fuer `finally`/`then` da. Die Liste in `klassen.rs` bleibt
    /// damit die **eine** Wahrheit, und hier kommt nur der async-Weg dazu.
    pub(crate) fn reaktion_fortsetzung_einreihen(&mut self, ziel: HeapRef, fortsetzung: HeapRef) {
        if ziel == KEIN_REF {
            return;
        }
        // Release 10.10.2026: `await` -> behandelt.
        self.behandelt(ziel);
        if self.mikrotasks.len() >= self.grenzen.max_mikrotasks {
            return; // Gedeckelt, wie `reaktion_einreihen_art`.
        }
        // ## W23 — DER ENTSCHEIDENE FALL GEHOERT AUCH HIERHER
        //
        // **`await` ruft zuerst `PromiseResolve` auf und dann
        // `PerformPromiseThen`** (ES2023 27.7.5.3 Schritt 9). Bei einem nackten
        // Wert heisst das: das Versprechen wird **erfuellt**, und **erst dann**
        // wird die Reaktion eingereiht — das Versprechen ist also in diesem
        // Moment **schon entschieden**, und `versprechen_settle` hat
        // `reaktionen` mit `mem::take` **leer** herausgenommen.
        //
        // **GEMESSEN, vorher:** die Reaktion blieb dort liegen, es kam **nie**
        // eine Mikrotask heraus, und der Rumpf blieb bei `a,sync,sync2`
        // stehen — ohne jeden Fehler. Genau die Art von Fehler, die man
        // zuerst fuer „richtig haenge ich noch" haelt.
        //
        // Der entschiedene Fall geht darum **direkt** in die Schlange, genau
        // wie in `versprechen_dann_innen` (das macht es fuer `then` genauso).
        let zustand = match self.heap.holen(ziel) {
            HObj::Versprechen { zustand, .. } => Some(zustand.clone()),
            _ => None,
        };
        match zustand {
            Some(crate::heap::VersprechenZustand::Erfuellt(w)) => {
                self.mikrotasks.push(crate::interp::Mikrotask::fortsetzung(
                    fortsetzung,
                    Aufnahme::Wert(w),
                ));
                return;
            }
            Some(crate::heap::VersprechenZustand::Abgelehnt(w)) => {
                self.mikrotasks.push(crate::interp::Mikrotask::fortsetzung(
                    fortsetzung,
                    Aufnahme::Wurf(w),
                ));
                return;
            }
            // `None`: kein Versprechen — dann kann auch nichts eingereiht
            // werden. `Offen`: warten auf die Entscheidung.
            None | Some(crate::heap::VersprechenZustand::Offen) => {}
        }
        let o = self.heap.holen_mut(ziel);
        if let HObj::Versprechen { reaktionen, .. } = o {
            reaktionen.push(crate::heap::Reaktion {
                bei_erfuellt: None,
                bei_fehler: None,
                folge: ziel,
                fortsetzung: Some(fortsetzung),
                art: 0,
            });
        }
        self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
    }

    /// `async_fortsetzen(container, aufnahme)`: einen angehaltenen
    /// async-Rahmen aus der Mikrotask-Schlange **fortsetzen**.
    ///
    /// Das ist die Stelle, die vorher `„Wiederaufbau: … fehlt noch"` sagte
    /// (zweimal: `schlange_abarbeiten` und `mikro_abarbeiten`). Der Rahmen
    /// liegt im `HObj::Generator`-Container, nicht im VM-Stapel — genau wie
    /// bei einem Generator, und aus **demselben** Grund (W19/W20).
    ///
    /// GEMESSEN in node 22, was dabei passieren muss:
    /// ```text
    //  Fall 1:  async function f(){ a; const v = await 1; b+v; return 9 }
    //           -> ["a","sync","sync2","b1","t9"]
    //  Fall 5:  zwei awaits -> [1,"s",2,3,"then:r"]
    //  Fall 14: return await Promise.reject(e) -> abgelehnt:z
    // ```
    /// JavaScript-Wuerfe lehnen das eigene Promise ab. Engine-Grenzen
    /// (Zeit, Schritte, Speicher) gehen als Fehler an den Host zurueck;
    /// sie duerfen durch eine async-Funktion nicht zu fangbaren TypeErrors werden.
    pub(crate) fn async_fortsetzen(&mut self, container: HeapRef, aufnahme: Aufnahme) -> Result<(), JsFehler> {
        let (mut gen_rahmen, promise) = {
            let o = self.heap.holen_mut(container);
            match o {
                HObj::Generator {
                    frames, versprechen, ..
                } => (frames.pop(), *versprechen),
                _ => (None, None),
            }
        };
        let Some(mut gen_rahmen) = gen_rahmen else {
            // Der Container hat keinen Rahmen mehr: der Rumpf ist fertig. Der
            // eigene Abschluss ist schon versandt (unten), hier ist nichts zu
            // tun — **kein** Fehler.
            return Ok(());
        };
        gen_rahmen.fortgesetzt = true;
        let alt_stapel = std::mem::take(&mut self.stapel);
        let alt_rahmen = std::mem::take(&mut self.rahmen);
        let alt_wurzeln = std::mem::take(&mut self.wurzeln);
        let alt_frisch = std::mem::take(&mut self.frisch);
        self.ausgelagert.push((alt_stapel, alt_rahmen, alt_wurzeln, alt_frisch));
        gen_rahmen.wurzel_marke = self.wurzel_merke();
        self.rahmen.push(gen_rahmen);
        let rahmen = self.rahmen.last_mut().unwrap();
        self.stapel = rahmen.stapel.clone();
        // Der Container muss leben, waehrend der Rumpf laeuft — er ist ueber
        // `wurzeln` geschuetzt, genau wie bei `n_generator_schritt`.
        self.wurzeln.push(GVal::Ref(container));
        rahmen.aufnahme = Some(aufnahme);
        let ende = self.rahmen.len() - 1;
        let ergebnis = self.schleife(ende);
        let haengt = matches!(ergebnis, LaufErgebnis::Angehalten);
        if haengt {
            let mut r = self.rahmen.pop().unwrap();
            r.stapel = self.stapel.clone();
            if let HObj::Generator { frames, .. } = self.heap.holen_mut(container) {
                frames.push(r);
            }
        } else if let HObj::Generator { zustand, .. } = self.heap.holen_mut(container) {
            *zustand = GenZustand::Fertig;
        }
        let (alt_stapel, alt_rahmen, alt_wurzeln, alt_frisch) =
            self.ausgelagert.pop().expect("ausgelagerter Aufrufer (intern)");
        self.stapel = alt_stapel;
        self.rahmen = alt_rahmen;
        self.wurzeln = alt_wurzeln;
        self.frisch = alt_frisch;
        // Engine-Grenzen sind keine fangbaren JavaScript-Ausnahmen.
        if let LaufErgebnis::Fehler(e) = ergebnis { return Err(e); }
        // **DER EIGENE ABSCHLUSS DES VERSPRECHENS.** node 22:
        //   `return await 5`      -> `.then(v => v)` ergibt `5` (Fall 9)
        //   `throw new Error("y")`-> `.catch` ergibt `geholt:y`  (Fall 8)
        //   `return await Promise.reject(e)` -> `abgelehnt:z`   (Fall 14)
        // `versprechen_settle` erledigt auch die Assimilation: `return
        // Promise.resolve(6)` liefert `6`, nicht das Versprechen-Objekt.
        if let Some(p) = promise
            && !haengt
        {
            match ergebnis {
                LaufErgebnis::Fertig(v) => self.versprechen_settle(p, true, v),
                LaufErgebnis::Wurf(w) => self.versprechen_settle(p, false, w),
                _ => {}
            }
        }
        Ok(())
    }

    /// `muster_rest(quelle, rest_namen)`: Rest-Muster fuer
    /// Destrukturierung (`const {a, ...rest} = quelle`). Erstellt ein
    /// neues Objekt mit den uebrigen Eigenschaften.
    ///
    /// `rest_namen` sind die Schluessel, die **schon herausgenommen**
    /// wurden (aus `Funktion::muster_aus`). Der Rest sind alle
    /// aufzaehlbaren eigenen Eigenschaften von `quelle`, die nicht in
    /// `rest_namen` stehen.
    /// `muster_rest(quelle, rest_namen)`: Rest-Muster fuer
    /// Destrukturierung (`const {a, ...rest} = quelle`). Erstellt ein
    /// neues Objekt mit den uebrigen Eigenschaften.
    ///
    /// `rest_namen` sind die Schluessel, die **schon herausgenommen**
    /// wurden (aus `Funktion::muster_aus`). Der Rest sind alle
    /// aufzaehlbaren eigenen Eigenschaften von `quelle`, die nicht in
    /// `rest_namen` stehen.
    pub(crate) fn muster_rest(
        &mut self,
        quelle: GVal,
        rest_namen: &[String],
    ) -> Result<GVal, Stoerung> {
        // Quelle muss ein Objekt sein (oder null/undefined -> TypeError).
        let _obj_ref = match quelle {
            GVal::Ref(r) => r,
            _ => {
                let e = self.fehler_wert("TypeError", "Destrukturierungsquelle ist kein Objekt");
                return Err(Stoerung::Wurf(e));
            }
        };
        // Eigene aufzaehlbare Eigenschaften sammeln.
        let rest_obj = self
            .neues_objekt(Some(self.proto_objekt))
            .map_err(Stoerung::Fehler)?;
        let mut gesamt = 0usize;
        // Wir iterieren ueber die Eigenschaften der Quelle.
        let namen = self.schluessel_werte(quelle)?;
        for name in namen {
            if !rest_namen.contains(&name) {
                // Wert lesen und in Rest-Objekt setzen.
                let wert = self.hole(quelle, &name)?;
                self.setze(GVal::Ref(rest_obj), &name, wert)?;
                gesamt += 1;
            }
        }
        // Kosten verbuchen (ungefaehr).
        self.vor_allok(gesamt * crate::heap::KOSTEN_EINTRAG)
            .map_err(Stoerung::Fehler)?;
        Ok(GVal::Ref(rest_obj))
    }

    /// `muster_rest_feld(quelle, i)`: Rest-Muster fuer Feld-Destrukturierung
    /// (`const [...rest] = quelle` oder `const [a, ...rest] = quelle`).
    ///
    /// ES2023 13.3.3.4 `BindingRestElement : ... BindingPattern`:
    /// 1. `A = ArrayCreate(0)`.
    /// 2. `iteratorRecord = ? GetIterator(value)`.
    /// 3. Repeat: `nextValue = ? IteratorNextValue(iteratorRecord)`; wenn nicht
    ///    fertig, `CreateDataPropertyOrThrow(A, index, nextValue)`.
    /// 4. `? IteratorClose(iteratorRecord, ReturnCompletion)`.
    ///
    /// Der Operand `i` ist **der Index des Rest-Elements im Muster** (siehe
    /// `compile.rs`, `Muster::Feld`/`Muster::Rest`: `MusterRestFeld(i)`), also
    /// beginnt der Rest bei `i`.
    ///
    /// GEMESSEN/GEFUNDEN (test262 `ary-ptrn-rest-ary-empty` in
    /// `arrow-function/dstr` + `variable/dstr`, weiters
    /// `gen-meth-dflt-ary-ptrn-rest-id-exhausted`): die erste Fassung sah
    /// `i` als Index in `Funktion::muster_aus` — das sind aber die **Objekt**
    /// -Muster (ausgeschlossene Schluesselnamen), nicht Feld-Indizes. Daraus
    /// kam ein falscher `start`, und statt zu iterieren wurde `length` plus
    /// Indexzugriff benutzt: an einem Generator oder einem eigenen
    /// `@@iterator` (der Restwert darf **irgendein** Iterable sein) ergab das
    /// ein Feld **ohne** `@@iterator` — der folgende Schritt meldete dann
    /// „Wert ist nicht iterierbar".
    pub(crate) fn muster_rest_feld(&mut self, quelle: GVal, i: usize) -> Result<GVal, Stoerung> {
        // `GetIterator` + alle Werte ziehen (schluckt Generatoren und eigene
        // `@@iterator` korrekt) — danach die `i` vorherigen verwerfen.
        let werte = self.iter_werte(quelle)?;
        let rest: Vec<GVal> = werte.iter().skip(i).copied().collect();
        // `feld_aus_werten` deckelt die Groesse selbst (`MAX_SAMMLUNG`).
        Ok(GVal::Ref(self.feld_aus_werten(&rest)?))
    }
}

// Release 09.10.2026 (Diagnose): `NARSIL_JS_DIAG`.
thread_local! {
    static DIAG_GESEHEN: std::cell::RefCell<std::collections::HashSet<u64>> = std::cell::RefCell::new(std::collections::HashSet::new());
}

fn diag_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("NARSIL_JS_DIAG").is_some())
}

// Release 09.10.2026 (Diagnose): `NARSIL_JS_PROFIL` misst die Zeit je
// Befehlsart (inklusive verschachtelter Aufrufe) und druckt die Spitze.
thread_local! {
    static PROFIL: std::cell::RefCell<std::collections::HashMap<std::mem::Discriminant<Op>, (String, u64, u128)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    static PROFIL_FUN: std::cell::RefCell<std::collections::HashMap<usize, u64>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

pub(crate) fn profil_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("NARSIL_JS_PROFIL").is_some())
}

// Release 10.10.2026: die Zeit je HOST-Aufruf (DOM-Methode, Lesen,
// Schreiben, Host-Funktion) — die Befehlszeiten oben sind verschachtelt
// und sagen nicht, welcher Host-Weg die Zeit kostet.
thread_local! {
    static PROFIL_HOST: std::cell::RefCell<std::collections::HashMap<String, (u64, u128)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

pub(crate) fn profil_host(art: &str, name: &str, start: Option<std::time::Instant>) {
    let Some(start) = start else { return };
    let d = start.elapsed().as_nanos();
    PROFIL_HOST.with(|p| {
        let mut p = p.borrow_mut();
        let schluessel = format!("{art}:{name}");
        let e = p.entry(schluessel).or_insert((0, 0));
        e.0 += 1;
        e.1 += d;
    });
}

pub(crate) fn profil_host_start() -> Option<std::time::Instant> {
    profil_an().then(std::time::Instant::now)
}

// Release 10.10.2026: EIGENZEIT je eingebauter Funktion — ohne die Zeit der
// JS-Rueckrufe, die sie aufruft (`forEach`, `call`, `sort`): ein Stapel mit
// der Kinderzeit je Ebene.
thread_local! {
    static NATIV_STAPEL: std::cell::RefCell<Vec<(&'static str, std::time::Instant, u128)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub(crate) fn profil_ebene_an(name: &'static str) {
    NATIV_STAPEL.with(|s| s.borrow_mut().push((name, std::time::Instant::now(), 0)));
}

pub(crate) fn profil_ebene_aus(name_dyn: Option<&str>) {
    let Some((name, start, kinder)) = NATIV_STAPEL.with(|s| s.borrow_mut().pop()) else { return };
    let gesamt = start.elapsed().as_nanos();
    NATIV_STAPEL.with(|s| {
        if let Some(oben) = s.borrow_mut().last_mut() {
            oben.2 += gesamt;
        }
    });
    let eigen = gesamt.saturating_sub(kinder);
    PROFIL_HOST.with(|p| {
        let mut p = p.borrow_mut();
        let e = p.entry(format!("eigen:{}", name_dyn.unwrap_or(name))).or_insert((0, 0));
        e.0 += 1;
        e.1 += eigen;
    });
}

fn profil_buchen(op: &Op, d: std::time::Duration) {
    PROFIL.with(|p| {
        let mut p = p.borrow_mut();
        let e = p.entry(std::mem::discriminant(op)).or_insert_with(|| {
            let n = format!("{op:?}");
            (n.split('(').next().unwrap_or("").to_string(), 0, 0)
        });
        e.1 += 1;
        e.2 += d.as_nanos();
    });
}

pub(crate) fn profil_drucken() {
    PROFIL.with(|p| {
        let mut v: Vec<(String, u64, u128)> = p.borrow().values().cloned().collect();
        v.sort_by(|a, b| b.2.cmp(&a.2));
        eprintln!("NARSIL_JS_PROFIL Befehl Anzahl ms ns/Befehl");
        for (n, c, t) in v.iter().take(30) {
            eprintln!("NARSIL_JS_PROFIL {n:28} {c:>12} {:>9.1} {:>8}", *t as f64 / 1e6, if *c > 0 { *t / u128::from(*c) } else { 0 });
        }
        p.borrow_mut().clear();
    });
    PROFIL_HOST.with(|p| {
        let mut v: Vec<(String, u64, u128)> = p.borrow().iter().map(|(k, (c, t))| (k.clone(), *c, *t)).collect();
        v.sort_by(|a, b| b.2.cmp(&a.2));
        for (n, c, t) in v.iter().take(40) {
            eprintln!("NARSIL_JS_PROFIL_HOST {n:40} {c:>10} {:>9.1} ms {:>8} ns", *t as f64 / 1e6, if *c > 0 { *t / u128::from(*c) } else { 0 });
        }
        p.borrow_mut().clear();
    });
}

/// Befehle je Funktion (ohne die Zeit der aufgerufenen Funktionen).
pub(crate) fn profil_funktionen_drucken(programm: Option<&crate::op::Bytecode>) {
    PROFIL_FUN.with(|p| {
        let mut v: Vec<(usize, u64)> = p.borrow().iter().map(|(k, v)| (*k, *v)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        for (fun, n) in v.iter().take(40) {
            let (name, zeile) = programm
                .and_then(|p| p.funktionen.get(*fun))
                .map(|f| (f.name.clone(), f.zeile))
                .unwrap_or_default();
            eprintln!("NARSIL_JS_PROFIL_FUN {fun:>8} {n:>12} {name} @{zeile}");
        }
        p.borrow_mut().clear();
    });
}

impl Engine {
    /// A property of an object literal: defined on a plain object (no
    /// setter of the prototype chain runs), assigned otherwise.
    fn literal_define(&mut self, ziel: GVal, name: &str, wert: GVal, define: bool) -> Result<(), Stoerung> {
        if define
            && let GVal::Ref(r) = ziel
            && matches!(self.heap.holen(r), HObj::Gegenstand { .. })
        {
            let marke = self.wurzel_merke();
            let out = self.eigenschaft_mit_attributen(r, name, wert, true, true);
            self.wurzel_zurueck(marke);
            return out;
        }
        self.setze(ziel, name, wert)
    }
}
