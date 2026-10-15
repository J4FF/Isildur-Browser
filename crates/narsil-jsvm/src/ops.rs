// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Operationen der Engine: Konvertierungen, Eigenschaften, Operatoren,
//! Literale, Destrukturierung, Klassen, Fehlerwerte, Export, Tasks.
//!
//! Alle Eingabe-abhaengigen Schleifen sind gedeckelt (M3), alle
//! Allokationen vorgeprueft (M4).

use std::collections::HashMap;

use crate::fehler::JsFehler;
use crate::heap::{Eigenschaft, GVal, HObj, Heap, HeapRef};
use crate::interp::{Engine, Stoerung};

/// Max. Eintraege bei Spread/Destrukturierung/Schluessel-Sammlung (M4).
pub(crate) const MAX_SAMMLUNG: usize = 1_000_000;
/// Max. Export-Tiefe GVal -> Wert (M5).
const MAX_EXPORT_TIEFE: u32 = 32;
/// Max. Textlaenge eines unbehandelten Wurfs in der Fehlermeldung.
const MAX_WURF_TEXT: usize = 2000;

impl Engine {
    // ---------------------------------------------------------------
    // Fehler-Helfer
    // ---------------------------------------------------------------

    pub(crate) fn stoerung_fehler(&self, s: Stoerung) -> JsFehler {
        match s {
            Stoerung::Fehler(e) => e,
            Stoerung::Wurf(g) => JsFehler::Laufzeit(format!("Unbehandelt: {}", self.wurf_text(g))),
        }
    }

    /// Release 11.10.2026: ein `DOMException` (WebIDL §3.14.1) — Name, `code`
    /// und der Prototyp der Klasse `DOMException` des Fensters (falls es sie
    /// gibt). GEMESSEN vorher (WPT `assert_throws_dom`): Host-Fehler kamen
    /// als `TypeError` an, DOM-Namen ohne `code` und nicht `instanceof
    /// DOMException`.
    pub(crate) fn dom_fehler_wert(&mut self, art: &str, nachricht: &str) -> GVal {
        let w = self.fehler_wert_basis(art, nachricht);
        let GVal::Ref(r) = w else {
            return w;
        };
        let m = self.wurzel_merke();
        self.wurzeln.push(w);
        let global = GVal::Ref(self.global_objekt);
        let dom_proto = match self.hole_ohne_host(global, "DOMException") {
            Ok(k @ GVal::Ref(_)) => match self.hole_ohne_host(k, "prototype") {
                Ok(GVal::Ref(p)) => Some(p),
                _ => None,
            },
            _ => None,
        };
        if let Some(p) = dom_proto
            && let HObj::Fehler { proto, .. } = self.heap.holen_mut(r)
        {
            *proto = Some(p);
        }
        if let Ok(nm) = self.text_wert(art) {
            self.wurzeln.push(nm);
            self.fehler_eigen_versteckt(r, "name", nm);
        }
        self.fehler_eigen_versteckt(r, "code", GVal::Zahl(f64::from(dom_ausnahme_code(art).unwrap_or(0))));
        self.wurzel_zurueck(m);
        w
    }

    /// Release 13.10.2026: der Fehler eines Host-SETTERS — „Name: …" mit
    /// einem DOM-Namen wird diese `DOMException` (wie bei Host-Methoden),
    /// sonst TypeError. GEMESSEN vorher (WPT outerhtml-01):
    /// `documentElement.outerHTML = …` warf TypeError statt
    /// NoModificationAllowedError.
    pub(crate) fn host_setzfehler_wert(&mut self, nachricht: &str) -> GVal {
        let kopf = nachricht.split_once(':').map_or(nachricht, |(a, _)| a.trim());
        if kopf != "SyntaxError" && dom_ausnahme_code(kopf).is_some() {
            return self.dom_fehler_wert(kopf, nachricht);
        }
        self.fehler_wert("TypeError", nachricht)
    }

    /// Baut ein JS-Fehlerobjekt (`new TypeError("...")`-aequivalent).
    /// Notfall-Regel: Fehlerobjekte sind winzig; scheitert die
    /// Limit-Pruefung, wird einmalig trotzdem alloziert (dokumentierte
    /// Ausnahme — der eigentliche Limit-Fehler entsteht an der
    /// verursachenden Allokation, nicht hier).
    pub(crate) fn fehler_wert(&mut self, art: &str, nachricht: &str) -> GVal {
        // Release 11.10.2026: DOM-Namen werden `DOMException`s (siehe
        // `dom_fehler_wert`; `SyntaxError` bleibt der JS-Fehler).
        if art != "SyntaxError" && dom_ausnahme_code(art).is_some() {
            return self.dom_fehler_wert(art, nachricht);
        }
        self.fehler_wert_basis(art, nachricht)
    }

    fn fehler_wert_basis(&mut self, art: &str, nachricht: &str) -> GVal {
        // Release 09.10.2026 (Diagnose): `NARSIL_JS_DIAG_FEHLER=<Text>` —
        // Herkunft eines passenden Fehlers (Funktionsquelle, letzte Befehle).
        if let Some(such) = diag_fehler_text()
            && nachricht.contains(such.as_str())
        {
            self.diag_fehler_herkunft(art, nachricht);
        }
        let frames: Vec<_> = self.rahmen.iter().rev().take(8).map(|frame| {
            let name = self.programm.as_ref().and_then(|p| p.funktionen.get(frame.fun))
                .map(|f| f.name.as_str()).unwrap_or("<script>");
            format!("{}:{}", name, frame.zeile)
        }).collect();
        self.letzte_fehlerstelle = (!frames.is_empty()).then(|| frames.join(" <- "));
        let proto = match art {
            "TypeError" => self.proto_typfehler,
            "RangeError" => self.proto_bereichsfehler,
            "SyntaxError" => self.proto_syntaxfehler,
            "ReferenceError" => self.proto_referenzfehler,
            _ => self.proto_fehler,
        };
        let obj = HObj::Fehler {
            proto: Some(proto),
            name: art.to_string(),
            nachricht: nachricht.to_string(),
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = Heap::kosten_fuer(&obj) + art.len() + nachricht.len().min(4096);
        if self.vor_allok(kosten).is_err() {
            // Notfall-Pfad: trotzdem anlegen (winzig, gezaehlt).
            let r = self.belege(obj, kosten);
            self.wurzeln.push(GVal::Ref(r));
            self.setze_eigen_text(
                r,
                "message",
                &nachricht.chars().take(512).collect::<String>(),
            );
            self.setze_eigen_text(r, "name", &art.chars().take(64).collect::<String>());
            return GVal::Ref(r);
        }
        let r = self.belege(obj, kosten);
        self.wurzeln.push(GVal::Ref(r));
        let m = self.wurzel_merke();
        let msg = match self.text_wert(nachricht) {
            Ok(v) => v,
            Err(_) => GVal::Undefiniert,
        };
        self.wurzeln.push(msg);
        // Release 09.10.2026: `message` ist eine eigene, NICHT aufzaehlbare
        // Eigenschaft, `name` kommt vom Prototyp (ES2023 20.5.1.1 /
        // 20.5.3.3). GEMESSEN (node 22): `try { null.x } catch (e) {
        // Object.keys(e).length }` -> `0`; der Motor lieferte `2`
        // (`message`, `name`), und `Object.assign({}, e)`/`for…in` sahen
        // beide.
        self.fehler_eigen_versteckt(r, "message", msg);
        // Nur die Arten mit eigenem Prototyp erben `name`; DOM-Arten
        // (`QuotaExceededError`, `DataCloneError`, `NamespaceError`, …) tragen
        // ihn selbst (WebIDL: DOMException.name).
        if !matches!(art, "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError")
            && let Ok(nm) = self.text_wert(art)
        {
            self.wurzeln.push(nm);
            self.fehler_eigen_versteckt(r, "name", nm);
        }
        let kopf = if nachricht.is_empty() { art.to_string() } else { format!("{art}: {nachricht}") };
        self.stack_setzen(r, &kopf);
        self.wurzel_zurueck(m);
        GVal::Ref(r)
    }

    /// Release 09.10.2026: `stack` wie in V8/Chrome —
    /// `"TypeError: x\n    at f (<anonymous>:12:1)\n    at <anonymous>:30:1"`,
    /// als eigene, nicht aufzaehlbare Eigenschaft.
    ///
    /// GEMESSEN, warum: `e.stack` war `undefined`. Bibliotheken lesen es
    /// (Fehlermelder, Polyfills, `console.error(e.stack)`), und youtube.com
    /// meldete Custom-Element-Fehler ohne jede Stelle. Die Zeile ist die
    /// laufende Zeile des Rahmens (`Op::Zeile`); eine Skript-Adresse kennt
    /// der Bytecode nicht, deshalb `<anonymous>` wie bei `eval`-Code in V8.
    /// Release 09.10.2026 (Diagnose, siehe `fehler_wert`).
    #[cold]
    fn diag_fehler_herkunft(&self, art: &str, nachricht: &str) {
        for (tiefe, r) in self.rahmen.iter().rev().take(8).enumerate() {
            let args: Vec<&str> = r.args.iter().map(|a| self.typ_von(*a)).collect();
            let f = self.programm.as_ref().and_then(|p| p.funktionen.get(r.fun));
            let quelle: String = f
                .and_then(|f| f.quelle.as_deref())
                .map(|q| q.chars().take(240).collect())
                .unwrap_or_default();
            let von = r.pc.saturating_sub(10);
            let bis = r.pc.min(r.code.len());
            let ops: Vec<String> = r.code[von..bis]
                .iter()
                .map(|o| match o {
                    crate::op::Op::LeseVar(z) => format!(
                        "LeseVar({})",
                        f.and_then(|f| f.namen.get(*z as usize)).cloned().unwrap_or_default()
                    ),
                    crate::op::Op::LeseEigenschaft(z) | crate::op::Op::Text(z) => {
                        format!("{o:?}={}", self.texte_arc.get(*z as usize).cloned().unwrap_or_default())
                    }
                    andere => format!("{andere:?}"),
                })
                .collect();
            eprintln!(
                "NARSIL_JS_DIAG_FEHLER {art}: {nachricht} | Rahmen -{tiefe} Zeile {} pc {} | args {args:?} | ops {ops:?} | quelle {quelle:?}",
                r.zeile, r.pc
            );
        }
    }

    /// Release 09.10.2026: eine eigene, nicht aufzaehlbare Eigenschaft an
    /// einem Fehlerobjekt (`message`, `stack`).
    pub(crate) fn fehler_eigen_versteckt(&mut self, r: HeapRef, name: &str, wert: GVal) {
        if let HObj::Fehler { eigenschaften, .. } = self.heap.holen_mut(r) {
            eigenschaften.insert(
                name.to_string(),
                crate::heap::Eigenschaft {
                    wert,
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
        }
    }

    pub(crate) fn stack_setzen(&mut self, r: HeapRef, kopf: &str) {
        let mut s = String::with_capacity(kopf.len() + 256);
        s.push_str(kopf);
        for frame in self.rahmen.iter().rev().take(10) {
            let name = self
                .programm
                .as_ref()
                .and_then(|p| p.funktionen.get(frame.fun))
                .map(|f| f.name.as_str())
                .unwrap_or("");
            if name.is_empty() || name.starts_with('<') {
                s.push_str(&format!("\n    at <anonymous>:{}:1", frame.zeile));
            } else {
                s.push_str(&format!("\n    at {name} (<anonymous>:{}:1)", frame.zeile));
            }
        }
        let Ok(t) = self.text_wert(&s) else { return };
        self.fehler_eigen_versteckt(r, "stack", t);
    }

    /// Release 09.10.2026: wie `wurf_text`, aber mit dem Stapel eines
    /// Fehlerobjekts (`stack`, siehe `stack_setzen`) — die Meldung zeigt dann,
    /// WO der Fehler entstand.
    pub(crate) fn wurf_text_mit_stapel(&self, g: GVal) -> String {
        if let GVal::Ref(r) = g
            && let HObj::Fehler { eigenschaften, .. } = self.heap.holen(r)
            && let Some(e) = eigenschaften.get("stack")
            && let GVal::Ref(t) = e.wert
            && let HObj::Zeichenkette(s) = self.heap.holen(t)
        {
            return s.clone();
        }
        self.wurf_text(g)
    }

    /// Best-Effort-Text eines geworfenen Werts (fuer `Unbehandelt: ...`).
    pub(crate) fn wurf_text(&self, g: GVal) -> String {
        let s = match g {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Fehler {
                    name, nachricht, ..
                } => format!("{name}: {nachricht}"),
                // J27: ein **geworfenes Objekt**, das kein Motor-Fehler ist
                // (z. B. `Test262Error` aus dem Harness der test262-Suite).
                // GEMESSEN, warum das hier steht: `export_wert(g,0).als_text()`
                // lieferte `[object Object]`, und im test262-Lauf ueber die
                // echte Suite waren das **507 der 1090** roten Faelle — eine
                // Gruppe, die nur „irgendeine Assertion ist fehlgeschlagen"
                // sagte. Mit dem Namen und der Meldung wird daraus eine
                // benennbare Luecke (und der Fehler, den ein Skript sieht,
                // nennt seinen Grund).
                HObj::Gegenstand { .. } | HObj::Verpackt { .. } => {
                    let name = self.eigenes_text(g, "name");
                    let nachricht = self.eigenes_text(g, "message");
                    match (name, nachricht) {
                        (Some(n), Some(m)) => format!("{n}: {m}"),
                        (Some(n), None) => n,
                        (None, Some(m)) => m,
                        // Kein Fehlerobjekt: der alte Weg bleibt, damit
                        // `throw {a:1}` weiterhin `[object Object]` sagt
                        // (node sagt auch das).
                        (None, None) => self.export_wert(g, 0).als_text(),
                    }
                }
                _ => self.export_wert(g, 0).als_text(),
            },
            _ => self.export_wert(g, 0).als_text(),
        };
        if s.len() > MAX_WURF_TEXT {
            // J27: an einer **Zeichen**-grenze kuerzen, nicht an einer
            // Byte-Grenze. GEMESSEN: `&s[..MAX_WURF_TEXT]` panickte
            // (`assertion failed: self.is_char_boundary(new_len)`), sobald
            // die Meldung an dieser Stelle ein Mehrbyte-Zeichen hat — die
            // test262-Suite hat solche Meldungen (Zeichen aus
            // `property-escapes`), und ein Panic im Motor ist eine
            // Regelverletzung, keine Ausgabe.
            let mut ende = MAX_WURF_TEXT;
            while ende > 0 && !s.is_char_boundary(ende) {
                ende -= 1;
            }
            format!("{}...", &s[..ende])
        } else {
            s
        }
    }

    /// J27: der Text einer **eigenen** Eigenschaft, oder `None`.
    ///
    /// GEMESSEN, warum der Umweg ueber den Heap noetig ist: `wurf_text`
    /// nimmt `&self` (es wird aus `Werfen` heraus benutzt, wo der Stapel
    /// noch nicht aufgeraeumt ist), und `hole` braucht `&mut self`.
    pub(crate) fn eigenes_text(&self, g: GVal, name: &str) -> Option<String> {
        let GVal::Ref(r) = g else { return None };
        let map = match self.heap.holen(r) {
            HObj::Gegenstand { eigenschaften, .. } | HObj::Verpackt { eigenschaften, .. } => {
                eigenschaften
            }
            _ => return None,
        };
        let e = map.get(name)?;
        match e.wert {
            GVal::Ref(z) => match self.heap.holen(z) {
                HObj::Zeichenkette(t) => Some(t.clone()),
                _ => None,
            },
            // Ein `name`, der kein Text ist, ist kein Name (node gibt dann
            // auch nicht den Namen aus).
            _ => None,
        }
    }

    // ---------------------------------------------------------------
    // Allokations-Helfer
    // ---------------------------------------------------------------

    /// J8: **interner** Text ohne die String-Grenze — fuer Prototyp-Namen
    /// und Fehlertexte des Motors selbst.
    ///
    /// GEMESSEN, warum das noetig war: der Bau eines Fehler-Objekts holt
    /// `"RangeError"`, `"TypeError"`, `""` … mit `text_wert(...).expect(...)`.
    /// Mit einem kleinen `max_string_laenge` (im Test 10) scheiterte schon
    /// `"RangeError"` an der Grenze, und das `expect` **PANICTE** — ein
    /// Grenzwert erzeugte einen Absturz statt eines Fehlers. Interner
    /// Maschinentext ist kein Skriptdatum und darf die Grenze nicht
    /// treffen; passt er nicht mehr in den Heap, kommt `undefined` zurueck
    /// (kein Panic, kein Wachsen).
    pub(crate) fn text_intern(&mut self, s: &str) -> GVal {
        let obj = HObj::Zeichenkette(s.to_string());
        let kosten = Heap::kosten_fuer(&obj);
        if self.heap.bytes.saturating_add(kosten) > self.grenzen.max_heap_bytes {
            return GVal::Undefiniert;
        }
        GVal::Ref(self.belege(obj, kosten))
    }

    pub(crate) fn text_wert(&mut self, s: &str) -> Result<GVal, JsFehler> {
        self.string_grenze(s.len())?;
        let obj = HObj::Zeichenkette(s.to_string());
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten)?;
        Ok(GVal::Ref(self.belege(obj, kosten)))
    }

    pub(crate) fn neues_objekt(&mut self, proto: Option<HeapRef>) -> Result<HeapRef, JsFehler> {
        let obj = HObj::Gegenstand {
            proto,
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
            reihenfolge: Vec::new(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten)?;
        Ok(self.belege(obj, kosten))
    }

    pub(crate) fn neues_feld(&mut self) -> Result<HeapRef, JsFehler> {
        let obj = HObj::Feld {
            proto: Some(self.proto_feld),
            elemente: Vec::new(),
            rest: Default::default(),
            sperre: crate::heap::Sperre::Offen,
            luecken: None,
        };
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten)?;
        Ok(self.belege(obj, kosten))
    }

    pub(crate) fn feld_aus_werten(&mut self, werten: &[GVal]) -> Result<HeapRef, Stoerung> {
        if werten.len() > MAX_SAMMLUNG {
            return Err(Stoerung::Fehler(JsFehler::Speicher(
                "Feld zu gross".to_string(),
            )));
        }
        let obj = HObj::Feld {
            proto: Some(self.proto_feld),
            elemente: werten.to_vec(),
            rest: Default::default(),
            sperre: crate::heap::Sperre::Offen,
            luecken: None,
        };
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        Ok(self.belege(obj, kosten))
    }

    pub(crate) fn nativ_anlegen(
        &mut self,
        name: &str,
        nativ: crate::builtin::Nativ,
        proto: HeapRef,
    ) -> HeapRef {
        // J27: die Kosten der **zwei** echten Eigenschaften und des
        // Namens-Textes mitrechnen — sonst waere die Buchhaltung eine
        // Behauptung und der Wert waere zu klein (M4).
        let kosten = Heap::kosten_fuer(&HObj::Nativ {
            name: name.to_string(),
            proto: Some(proto),
            ziel: None,
            eigenschaften: Default::default(),
        }) + name.len()
            + 2 * crate::heap::KOSTEN_EINTRAG
            + name.len() * 2;
        let obj = HObj::Nativ {
            name: name.to_string(),
            proto: Some(proto),
            ziel: None,
            eigenschaften: Default::default(),
        };
        // GEMESSEN, warum die Zahl **vor** dem `insert` steht: `Nativ`
        // ist nicht `Copy`, und `self.native.insert(r, nativ)` nimmt es
        // per Wert. Die Tabelle braucht es also vorher.
        let laenge = crate::eingebaut_array::parameterzahl(&nativ);
        // Setup-Phase: Limits grosszuegig — notfalls trotzdem anlegen
        // (Engine-Aufbau muss gelingen; wenige hundert kleine Objekte).
        if self.vor_allok(kosten).is_err() {
            let r = self.belege(obj, kosten);
            self.native.insert(r, nativ);
            self.funktionsnamen_setzen(r, name, laenge);
            return r;
        }
        let r = self.belege(obj, kosten);
        self.native.insert(r, nativ);
        self.funktionsnamen_setzen(r, name, laenge);
        r
    }

    /// J27: `length` und `name` als **echte eigene** Eigenschaften einer
    /// Funktion anlegen — so, wie es die Spec beim Erzeugen des
    /// Funktionsobjekts vorschreibt (ES2023 10.2.4, OrdinaryFunctionCreate
    /// → `MakeMethod`/`SetFunctionName` mit
    /// `{writable:false, enumerable:false, configurable:true}`).
    ///
    /// GEMESSEN, warum das **hier** und nicht nur im Deskriptor-Pfad
    /// geschehen muss: der Lauf ueber die echte test262-Suite hatte **80**
    /// rote Faelle mit „name should be an own property" (45) und „length
    /// should be an own property" (35) — alle aus
    /// `test/built-ins/Array/from/`. `Object.getOwnPropertyDescriptor` fand
    /// die Werte (der Sonderfall in `builtin.rs`), aber
    ///   `Array.from.hasOwnProperty('name')`            -> `false`
    ///   `Object.getOwnPropertyNames(Array.from).join()` -> `` (leer)
    /// Ein Sonderfall im Deskriptor-Pfad laesst genau diese beiden Wege
    /// blind. `harness/propertyHelper.js:68` prueft aber
    /// `__hasOwnProperty(obj, name)` — daher die echten Eigenschaften.
    ///
    /// GEMESSEN, warum `length` **hier** die Parameterzahl ist und nicht
    /// `0`: `Array.from.length` ist in node `1` (ECMA-262 23.1.2.1), der
    /// Motor lieferte `0` — und `test/built-ins/Array/from/
    /// Array.from_arity.js` prueft genau das mit
    /// `verifyProperty(Array.from, "length", {value: 1, …})`.
    ///
    /// W6D: der Name-Text ist ein **Wert im Heap**; zwischen
    /// `text_wert` und dem Eintragen kann der Heap wachsen, also steht er
    /// bis zum Eintragen in `wurzeln`.
    pub(crate) fn funktionsnamen_setzen(&mut self, r: HeapRef, name: &str, laenge: u32) {
        let mut name_wert = GVal::Undefiniert;
        let mut ok = true;
        match self.text_wert(name) {
            Ok(v) => name_wert = v,
            // GEMESSEN, warum `Undefiniert` statt `unwrap`: der
            // Aufbau-Zustand (`Engine::neu`) kann an der Speichergrenze
            // scheitern; der Motor muss dann **weiterbauen** und der Name
            // fehlt — ein Absturz hier wuerde den ganzen Motor wegnehmen.
            Err(_) => ok = false,
        }
        // Release 09.10.2026: mit MARKE zurueck. GEMESSEN vorher: hier stand
        // `wurzel_zurueck(1)` — JEDES zur Laufzeit angelegte Native (Iterator-
        // `next`, Resolver, gebundene Funktionen) kuerzte die GANZE
        // Wurzelliste auf einen Eintrag; ein GC danach raeumte z. B. den
        // laufenden `forEach`-Rueckruf weg (youtube.com: „toter Verweis").
        let marke = self.wurzel_merke();
        self.wurzeln.push(name_wert);
        let laenge_e = crate::heap::Eigenschaft {
            wert: GVal::Zahl(laenge as f64),
            getter: None,
            setter: None,
            aufzählbar: false,
            beschreibbar: false,
            konfigurierbar: true,
        };
        let name_e = crate::heap::Eigenschaft {
            wert: name_wert,
            getter: None,
            setter: None,
            aufzählbar: false,
            beschreibbar: false,
            konfigurierbar: true,
        };
        let eingetragen = if let HObj::Funktion { eigenschaften, .. }
        | HObj::Nativ { eigenschaften, .. } = self.heap.holen_mut(r)
        {
            eigenschaften.insert("length".to_string(), laenge_e);
            eigenschaften.insert("name".to_string(), name_e);
            true
        } else {
            false
        };
        self.wurzel_zurueck(marke);
        if eingetragen && ok {
            // M4: die zwei Eintraege und der Text kosten Platz, und die
            // Buchhaltung muss sie **sehen** — sonst waere `heap_bytes`
            // eine Zahl, die kleiner ist als das, was belegt wurde.
            self.heap
                .kosten_verbuchen(2 * crate::heap::KOSTEN_EINTRAG * 2);
        }
    }

    /// Ein eingebautes Global anlegen.
    ///
    /// ## W12, GEBAUT (JS2/QUIC, 04.10.2026, 04:40)
    ///
    /// **ECMA-262 8.1.7 / die Property-Tabellen der Kapitel 19–22:** jedes
    /// eingebaute Global — Konstruktoren (`Map`, `Set`, `Array`, `String`,
    /// `JSON`, `Object`, `Number`) **und** die Namensraeume (`Math`, `Reflect`)
    /// — hat `{ writable: true, enumerable: **false**, configurable: true }`.
    /// `NaN`, `Infinity`, `undefined` und `globalThis` haben sogar
    /// `{ false, false, false }`.
    ///
    /// GEMESSEN (`node 22`), **alle sieben gleich**:
    /// ```js
    /// Object.getOwnPropertyDescriptor(globalThis, 'Map')
    ///   node: writable true, enumerable **false**, configurable true
    ///   vorher bei uns: true, **true**, true
    /// ```
    ///
    /// **WARUM ICH `setze_eigen` NICHT GEAENDERT HABE — obwohl es die
    /// offensichtliche Stelle ist.** `setze_eigen` macht
    /// `aufzaehlbar: true`, und das ist fuer ein **benutztes** `var` auf
    /// Skriptebene **richtig** (8.1.7 d.ii.2:
    /// `{ writable: true, enumerable: true, configurable: false }`, `node`
    /// bestaetigt: `true/true/false`). Haette ich `setze_eigen` umgestellt,
    /// waeren **alle** globalen Variablen falsch enumerierbar.
    ///
    /// **`global_binden` wird ausschliesslich fuer eingebaute Globale gerufen**
    /// (11 Stellen: `builtin.rs` 5, `dom.rs` 2, `interp.rs` 3, hier 1) — und
    /// **das** ist der Grund, warum die Aenderung hier hingehört und nicht in
    /// `setze_eigen`: die Funktion **behauptet** „das ist ein Global“, und ein
    /// eingebautes Global ist per Norm **nie** enumerierbar.
    ///
    /// **`NaN`/`Infinity`/`undefined`/`globalThis` bleiben `true/true/true`** —
    /// das ist `writable`/`configurable`, ein **zweiter** Schritt und ein
    /// eigener Wunsch. Ich habe **beides nicht in einem** gemacht: die
    /// enumerable-Haelfte ist W12, die andere ist neu und ungeprueft.
    ///
    /// **`let _ =` ist hier Absicht, nicht Schlamperei:** `eigenschaft_
    /// mit_attributen` kann nur an einem `Feld` mit Index-Schluessel
    /// scheitern, und `global_objekt` ist ein `Gegenstand`. Eine fehlende
    /// globale Eigenschaft ist ausserdem **besser** als eine enumerable.
    pub(crate) fn global_binden(&mut self, name: &str, wert: GVal) {
        let g = self.global_objekt;
        // Der **vorhandene** Helfer mit dem Enumerierbarkeits-Parameter — der
        // vierte Beleg fuer die Regel aus Runde 30 (zuerst greppen, dann
        // annehmen): `eigenschaft_mit_attributen` nimmt `aufzaehlbar` bereits.
        let _ = self.eigenschaft_mit_attributen(g, name, wert, false, true);
    }

    /// Eigene Daten-Eigenschaft (aufzaehlbar, beschreibbar) anlegen.
    pub(crate) fn setze_eigen(&mut self, obj: HeapRef, name: &str, wert: GVal) {
        let neu = name.to_string();
        let o = self.heap.holen_mut(obj);
        let slot = match o {
            HObj::Gegenstand { eigenschaften, .. } => Some(eigenschaften),
            HObj::Verpackt { eigenschaften, .. } => Some(eigenschaften),
            HObj::Feld { rest, .. } => {
                if index_aus_schluessel(name).is_none() && name != "length" {
                    Some(rest)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(map) = slot {
            if map
                .insert(
                    neu,
                    Eigenschaft {
                        konfigurierbar: true,
                        wert,
                        getter: None,
                        setter: None,
                        aufzählbar: true,
                        beschreibbar: true,
                    },
                )
                .is_none()
            {
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            }
            return;
        }
        // Feld-Index/laenge oder Spezialfall: ueber setze() regeln.
        let w = wert;
        let n = name.to_string();
        self.setze(GVal::Ref(obj), &n, w).ok();
    }

    pub(crate) fn setze_eigen_text(&mut self, obj: HeapRef, name: &str, text: &str) {
        let kosten = text.len() + crate::heap::KOSTEN_SLOT;
        self.heap.kosten_verbuchen(kosten);
        let o = HObj::Zeichenkette(text.to_string());
        let r = self.belege(o, 0);
        self.setze_eigen(obj, name, GVal::Ref(r));
    }

    // ---------------------------------------------------------------
    // Konvertierungen (JS-Semantik)
    // ---------------------------------------------------------------

    pub(crate) fn zu_bool(&self, v: GVal) -> bool {
        match v {
            GVal::Undefiniert | GVal::Null => false,
            GVal::Bool(b) => b,
            GVal::Zahl(n) => n != 0.0 && !n.is_nan(),
            // J24: `0n` ist falsch, alles andere wahr (wie bei der Zahl).
            GVal::Ganz(g) => g != 0,
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Zeichenkette(s) => !s.is_empty(),
                _ => true,
            },
        }
    }

    pub(crate) fn zu_zahl(&mut self, v: GVal) -> Result<f64, Stoerung> {
        match v {
            GVal::Undefiniert => Ok(f64::NAN),
            GVal::Null => Ok(0.0),
            GVal::Bool(true) => Ok(1.0),
            GVal::Bool(false) => Ok(0.0),
            GVal::Zahl(n) => Ok(n),
            // ## JS2/QUIC (03.10.2026, 23:20): DIE J24-NOTIZ WAR **FALSCH**.
            //
            // Sie sagte: *„`Number(2n)` rundet (das ist die dokumentierte
            // Grenze)“* — und daraus wurde `Ok(g as f64)`. **GEMESSEN,
            // `node 22`:** `Number(2n)` liefert **tatsaechlich kein** Wurf
            // (`"2"`), **aber** aus einem anderen Grund:
            //
            // ```js
            // Number(2n)          -> "2"     KEIN Wurf
            // +2n                 -> WURF TypeError
            // String.fromCharCode(1n) -> WURF TypeError
            // 'a'.repeat(2n)      -> WURF TypeError
            // Math.floor(1n)      -> WURF TypeError
            // ```
            //
            // `Number(x)` hat einen **eigenen** BigInt-Zweig
            // (ECMA-262 20.1.2.2: *„If Type(number) is BigInt, let n be
            // 𝔽(number); return Number(n)"*) — der steht in
            // `Engine::n_zahl_ctor` (`builtin2.rs`) und **nicht** hier.
            // `zu_zahl` **ist** `ToNumber`, und 7.1.4 Schritt 1 sagt
            // *„Let prim be ? ToPrimitive(argument)"*, und `ToNumber` einer
            // BigInt **wirft** (6.1.6.1 -> 7.1.4 -> BigInt hat kein
            // `valueOf` fuer Zahlen).
            //
            // **DIE KONFOLENZ, gemessen:** die alte Note hat **nicht nur**
            // BigInt falsch gemacht, sondern **jede** Stelle, die `ToNumber`
            // braucht — `fromCharCode`, `fromCodePoint`, `repeat`,
            // `Math.floor`, `indexOf`-Argumente. Das sind die Faelle aus **W8**.
            GVal::Ganz(_) => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "ToNumber einer BigInt"),
            )),
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                // Strings direkt (kein Umweg ueber zu_primitiv, der sie
                // unveraendert zurueckgaebe -> Endlosrekursion, M3).
                HObj::Zeichenkette(s) => Ok(crate::ops::zahl_aus_text(&s)),
                // J27: **ein Symbol ist keine Zahl** — `ToNumber` wirft
                // (ES2023 7.1.4). GEMESSEN (node 22):
                // `Number(Symbol())` -> TypeError; GEMESSEN im Lauf ueber
                // die echte test262-Suite: 66 Faelle „Expected a TypeError
                // to be thrown but no exception was thrown at all".
                //
                // GEMESSEN, warum es **hier** und nicht erst beim Aufruf:
                // ein Symbol ist ein `Gegenstand` mit `proto_symbol`
                // (`symbole.rs`), also lief es ueber `valueOf`/`toString`
                // und kam als `NaN` zurueck — kein Wurf, und **jede**
                // Stelle, die `?` setzt, half dann nichts.
                HObj::Gegenstand { proto: Some(p), .. } if p == self.proto_symbol => Err(
                    Stoerung::Wurf(self.fehler_wert("TypeError", "Symbol kann nicht in eine Zahl")),
                ),
                _ => {
                    let p = self.zu_primitiv(v, false)?;
                    // primitives Ergebnis sofort werten (Rekursionstiefe 1).
                    match p {
                        GVal::Ref(r2) => match self.heap.holen(r2).clone() {
                            HObj::Zeichenkette(s2) => Ok(crate::ops::zahl_aus_text(&s2)),
                            _ => Err(Stoerung::Wurf(
                                self.fehler_wert("TypeError", "keine Zahl ableitbar"),
                            )),
                        },
                        q => self.zu_zahl(q),
                    }
                }
            },
        }
    }

    pub(crate) fn zu_text(&mut self, v: GVal) -> Result<String, Stoerung> {
        match v {
            GVal::Undefiniert => Ok("undefined".to_string()),
            GVal::Null => Ok("null".to_string()),
            GVal::Bool(true) => Ok("true".to_string()),
            GVal::Bool(false) => Ok("false".to_string()),
            GVal::Zahl(n) => Ok(crate::wert::zahl_text(n)),
            // J24: `String(2n)` ist „2" — **ohne** `n` (Spec 6.1.6.1.20;
            // das `n` steht nur in der *Inspektion*, `console.log`).
            GVal::Ganz(g) => Ok(g.to_string()),
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Zeichenkette(s) => Ok(s),
                // ## JS2/QUIC (03.10.2026, 01:50): `ToString` EINES **SYMBOLS**
                // WIRFT (W8, der **zweite** der beiden dort genannten Arme).
                //
                // ECMA-262 6.1.6.1 `ToString` Schritt 3: *„If value is a
                // Symbol, throw a TypeError exception."* — und der Weg dorthin
                // laeuft ueber `ToPrimitive`, dessen `@@toPrimitive` bei einem
                // Symbol **fehlt**, also `TypeError` (7.1.18 `ToPrimitive`
                // Schritt 4: *„If Type(value) is Symbol, throw a TypeError"*).
                //
                // GEMESSEN, `node 22`:
                // ```js
                // JSON.parse(Symbol())   node: TypeError     bei uns: SyntaxError
                // ```
                // `JSON.parse` Schritt 1 ist `Let JText be ? ToString(text)` —
                // der Wurf **haette** durchgehen muessen. Stattdessen kam
                // `"Symbol()"` heraus, und der **JSON-Grammatiker** meldete
                // `SyntaxError`. **Ein Fehler an der falschen Stelle sieht aus
                // wie ein anderer Fehler**: `JSON/parse/text-non-string-primitive`
                // sagt „Expected a TypeError but got a SyntaxError".
                //
                // **DER ZWEITE ARM, den W8 nennt, und der erste ist `ops.rs:441`
                // (`GVal::Ganz(g) => Ok(g as f64)`) — der wurde in Runde 39
                // gebaut.** Damit ist W8 an **beiden** Stellen erledigt.
                //
                // **`zu_text_kein_symbol` (eingebaut_text.rs) bleibt bestehen** —
                // das ist die Stelle, die den Wurf **verlangt**; dieses hier ist
                // die, die ihn bisher **verschluckt** hat.
                HObj::Gegenstand {
                    proto: Some(pb), ..
                } if pb == self.proto_symbol => Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "ToString eines Symbols"),
                )),
                _ => {
                    let p = self.zu_primitiv(v, true)?;
                    self.zu_text(p)
                }
            },
        }
    }

    /// Eigenschaftsschluessel: Zahl -> kanonisch, Rest -> String.
    pub(crate) fn schluessel_text(&mut self, v: GVal) -> Result<String, Stoerung> {
        match v {
            GVal::Zahl(n) => Ok(zahl_schluessel(n)),
            GVal::Ref(r) => {
                // ## W16, TEIL 1, GEBAUT (JS2/QUIC, 03.10.2026, 05:50).
                //
                // **GEMESSEN, vorher:** `schluessel_text(symbol)` ergab
                // **`"[object Object]"`** — und damit legte jeder Aufrufer
                // **einen Muellschluessel** an. Zwei Stellen sind mir davon
                // **begegnet**: `vm.rs:5106` (Generator-Prototyp, geteilte
                // Datei, gemeldet) und mein eigener Bau fuer `Map`/`Set`
                // (zurueckgenommen, Runde 57).
                //
                // **WARUM ES FALSCH WAR:** der `_`-Arm ging ueber
                // `zu_primitiv(v, true)`, und der landet beim Symbol-Objekt
                // **nicht** bei `Symbol.prototype.toString`, sondern bei
                // `Object.prototype.toString` — daher `[object Object]`.
                //
                // **DER ZIELSCHLUESSEL IST GEMESSEN, und zwar am Haus:**
                // ```js
                // String(Symbol.iterator)                       -> "Symbol(Symbol.iterator)"
                // Object.getOwnPropertyNames(Array.prototype)[0] -> "Symbol(Symbol.iterator)"
                // ```
                // Und die **Zuweisung** benutzt genau diesen Text — `o[sym]=1`
                // erzeugt den Schluessel `Symbol(Symbol.iterator)` und ist danach
                // wieder lesbar. **Es gab also zwei Ableitungen**, und nur
                // `schluessel_text` war falsch. Diese Funktion ist damit die
                // **eine** Stelle, die beide auf denselben Text zieht — die
                // kleinste moegliche Aenderung, und **kein** Eingriff in die
                // 14 Aufrufer.
                //
                // **DER VORHANDENE HELFER**, wie immer: `n_symbol_text(.., false)`
                // (`symbole.rs:199`, `pub(crate)`) **ist** die
                // `SymbolDescriptiveString`. Ich habe **erst geprueft**, dass es
                // ihn gibt und `pub(crate)` ist (Runde 46: `als_symbol` gab es
                // nicht).
                //
                // **WAS DAS NICHT LOEST — und nicht geloest werden soll:**
                // `node 22` listet ein Symbol-Schluessel-**nicht** in
                // `getOwnPropertyNames` (`o[Symbol.iterator]=1` ergibt dort
                // `undefined` als erstes Element, bei uns
                // `Symbol(Symbol.iterator)`). Der Norm-Text ist
                // `ToPropertyKey` (6.1.7): `ToPrimitive(value, hint String)`
                // gibt bei einem Symbol **das Symbol** zurueck — es wird
                // **nicht** zu Text. **Wir ersetzen es durch Text**, und das
                // ist eine **Eigenheit der Darstellung**, die alle 14 Aufrufer
                // teilen. Das **echte** Loeschen des Lecks ist der zweite Teil
                // von W16 und **nicht** diese Aenderung — sonst muesste ich
                // `HObj::Gegenstand` um eine Symbol-Schluesselkarte erweitern,
                // und das ist `heap.rs`.
                if crate::symbole::ist_symbol(self, r) {
                    // **ZWEIMAL hat mich der Compiler hier belogen — bzw. ich
                    // mich selbst:** erst gab es `als_text` an `GVal` nicht,
                    // dann gab es `GVal::Text` nicht. `GVal` hat **keine**
                    // Zeichenketten-Variante (`heap.rs:26-35`): eine Kette ist
                    // `GVal::Ref` auf `HObj::Zeichenkette`. Ich habe **zweimal
                    // geraten statt zu greppen** — die Regel aus Runde 30 in
                    // ihrer kuerzesten Form.
                    // Release 09.10.2026: eindeutiger Schluessel je Symbol
                    // (siehe `Engine::symbol_schluessel`).
                    return Ok(self.symbol_schluessel(r));
                }
                match self.heap.holen(r).clone() {
                    HObj::Zeichenkette(s) => Ok(s),
                    _ => {
                        let p = self.zu_primitiv(v, true)?;
                        self.schluessel_text(p)
                    }
                }
            }
            _ => self.zu_text(v),
        }
    }

    /// `ToPrimitive` mit valueOf/toString-Protokoll. Gibt bei zwei
    /// Objekt-Rueckgaben einen TypeError (keine Endlosschleife, M3).
    pub(crate) fn zu_primitiv(&mut self, v: GVal, string_zuerst: bool) -> Result<GVal, Stoerung> {
        let r = match v {
            GVal::Ref(r) => r,
            _ => return Ok(v),
        };
        // Heap-Strings SIND primitiv (kein Methoden-Aufruf noetig).
        if matches!(self.heap.holen(r), HObj::Zeichenkette(_)) || crate::symbole::ist_symbol(self, r) {
            return Ok(v);
        }
        // Fehler/Datum/Regex haben eigene Protokolle ueber Natives auf
        // ihren Prototypen (valueOf/toString werden normal geholt).
        // ## W27, GEBAUT (JS2/QUIC, 06.00) — ECMA-262 7.1.18 Schritt 1.a/1.b.
        //
        // `@@toPrimitive` wurde **nie** nachgeschlagen; der Motor fiel immer auf
        // `valueOf`/`toString` zurueck. **GEMESSEN, `node 22`:**
        // ```js
        // "abc".indexOf("b", {[Symbol.toPrimitive](){ return 5 }})   node: -1  bei uns: 1
        // "abc".indexOf({[Symbol.toPrimitive](){ return "c" }})     node:  2  bei uns: -1
        // "".indexOf("", {[Symbol.toPrimitive](){ return Symbol("1") }})
        //                                              node: TypeError  bei uns: 0
        // "".indexOf("", {[Symbol.toPrimitive](){ return 1n }})
        //                                              node: TypeError  bei uns: 0
        // ```
        //
        // **DIE VORLAGE IST IM HAUS** und seit Runde 58 benutzbar:
        // `eigenschaften.rs:1962-1965` macht genau diese Nachsuche fuer
        // `@@iterator` — `n_symbol_bekannt` -> `schluessel_text` -> `hole`.
        // **Runde 58 war die Voraussetzung**: erst seit `schluessel_text` fuer
        // ein Symbol die `SymbolDescriptiveString` liefert, stimmt der Weg.
        // Sechster Beleg fuer „erst greppen, dann annehmen“.
        //
        // **`hole` und nicht `eigene_oder_proto`:** `@@toPrimitive` wird geholt
        // **ueber die Prototypkette** (Schritt 1.a sagt `GetMethod(input, …)`),
        // und `eigene_oder_proto` nimmt bei `Zeichenkette`-Objekten den
        // Sonderfall — genau der, den der `Rückfall` oben schon abhaengt.
        if let Ok(sym) = self.n_symbol_bekannt(
            GVal::Undefiniert,
            &[],
            crate::builtin::Nativ::SymToPrimitive,
        ) {
            let schl = self.schluessel_text(sym)?;
            let m = self.hole(GVal::Ref(r), &schl)?;
            if self.ist_aufrufbar(m) {
                // Schritt 1.b.i: der Hinweis. **`string_zuerst: bool` kann kein
                // `"default"` ausdruecken** — das ist eine **echte, offene**
                // Luecke dieser Signatur (siehe WUNSCH_JS.txt W27): ohne
                // `"default"` nimmt `Date.prototype[@@toPrimitive]` das
                // falsche Format. Ich baue hier **keine** Signatur-Aenderung,
                // weil `zu_primitiv` **zweiundzwanzig** Aufrufer hat.
                let hinweis = self
                    .text_wert(if string_zuerst { "string" } else { "number" })
                    .map_err(Stoerung::Fehler)?;
                let marke = self.wurzel_merke();
                self.wurzeln.push(m);
                self.wurzeln.push(GVal::Ref(r));
                let erg = self.aufruf_synchron(m, GVal::Ref(r), &[hinweis], false);
                self.wurzel_zurueck(marke);
                let erg = erg?;
                // Schritt 1.b.iii: **kein** Object -> zurueckgeben.
                //
                // **MEIN ERSTER WACHTER WAR FALSCH — und der richtige stand im
                // Haus, im `valueOf`-Zweig direkt darunter:**
                // ```rust
                // if !matches!(erg, GVal::Ref(_)) || self.string_inhalt(erg).is_some() {
                // ```
                // `!matches!(erg, GVal::Ref(_))` allein haelt eine **Primitive
                // Zeichenkette fuer ein Objekt** — sie ist bei uns `GVal::Ref`
                // auf `HObj::Zeichenkette`. GEMESSEN, und es war eine **von mir
                // eingebautе** Regression:
                // ```js
                // "abc".indexOf({[Symbol.toPrimitive](){ return "c" }})
                //   node: 2       bei mir (vorher): -1     bei mir (mit dem
                //                                                 falschen
                //                                                 Wächter): TypeError
                // ```
                // Siebter Beleg fuer „erst greppen, dann annehmen“ — und diesmal
                // war das Muster **im selben Funktionskoerper**.
                if !matches!(erg, GVal::Ref(_)) || self.string_inhalt(erg).is_some()
                    || matches!(erg, GVal::Ref(rr) if crate::symbole::ist_symbol(self, rr)) {
                    return Ok(erg);
                }
                // Schritt 1.b.iv: ein Object -> TypeError. (Hierhin ist
                // `Date` der einzige Norm-Fall, und der gibt immer eine
                // Primitive zurueck.)
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "@@toPrimitive lieferte ein Objekt"),
                ));
            }
            if !matches!(m, GVal::Undefiniert | GVal::Null) {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "@@toPrimitive ist nicht aufrufbar")));
            }
        }
        let methoden = if string_zuerst {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        for m in methoden {
            let f = match self.eigene_oder_proto(GVal::Ref(r), m)? {
                Some(f) => f,
                None => continue,
            };
            // Nur aufrufbar, sonst ueberspringen.
            if !self.ist_aufrufbar(f) {
                continue;
            }
            let marke = self.wurzel_merke();
            self.wurzeln.push(f);
            self.wurzeln.push(GVal::Ref(r));
            let erg = self.aufruf_synchron(f, GVal::Ref(r), &[], false);
            self.wurzel_zurueck(marke);
            let erg = erg?;
            // Heap-Strings sind Refs, aber primitiv (Koordination W5B).
            // ## GEBAUT (JS2/QUIC, 06.20): EIN SYMBOL IST KEIN OBJECT.
            //
            // ECMA-262 7.1.19 `OrdinaryToPrimitive` Schritt 3.b.ii sagt
            // *„If Type(result) is not Object, return result"* — und ein Symbol
            // ist **kein** Object, also wird es **zurueckgegeben**. Schritt 4
            // von 7.1.18 (`ToPrimitive`) **wirft** dann: *„If Type(value) is
            // Symbol, throw a TypeError exception."*
            //
            // **OHNE diese Bedingung fiel die Schleife auf `toString` zurueck**
            // und lieferte `"[object Object]"` — daraus `NaN`, **ohne Wurf**.
            // GEMESSEN, `node 22`:
            // ```js
            // Number({valueOf(){ return Symbol("1") }})          node: TypeError  bei uns: NaN
            // "".indexOf("", {valueOf(){ return Symbol("1") }})  node: TypeError  bei uns: 0
            // "".indexOf("", {toString(){ return Symbol("1") }}) node: TypeError  bei uns: TypeError
            // ```
            // Der **`toString`-Fall warf schon** — er gibt eine Zeichenkette
            // zurueck, kein Symbol-Objekt. **Genau das war mein Irrtum in Runde
            // 59:** ich mass drei Faelle, von denen einer bereits stimmte, und
            // schloss daraus, `valueOf` verhalte sich gleich.
            // *Zwei Faelle derselben Sache muessen sich gleich verhalten — wenn
            // sie es nicht tun, ist die Regel nicht „sondern", sondern es sind
            // zwei verschiedene Regeln.*
            //
            // `ist_symbol` ist der **vorhandene** Helfer (`symbole.rs:35`,
            // `pub(crate)`), und `string_inhalt` ist derselbe Trick wie im
            // `@@toPrimitive`-Zweig: eine Primitive Zeichenkette ist bei uns
            // `GVal::Ref`, ein Symbol ebenso.
            if !matches!(erg, GVal::Ref(_))
                || self.string_inhalt(erg).is_some()
                || matches!(erg, GVal::Ref(rr) if crate::symbole::ist_symbol(self, rr))
            {
                return Ok(erg);
            }
        }
        Err(Stoerung::Wurf(self.fehler_wert(
            "TypeError",
            "Objekt nicht in Primitiv wandelbar",
        )))
    }

    /// `ToObject`: Primitiv einpacken (fuer Methoden auf Primitiven).
    pub(crate) fn zu_objekt(&mut self, v: GVal) -> Result<HeapRef, Stoerung> {
        match v {
            // W14 (Runde 70, **ZURUECKGENOMMEN**): hier stand
            // `if matches!(self.heap.holen(r), HObj::Zeichenkette(_))` und
            // darunter `self.verpackt(Boxen::Text(GVal::Ref(r)))`.
            // GEMESSEN: `typeof Object('x')` blieb `string`, `Object('x')
            // instanceof String` blieb `false` — **kein einziger Messwert
            // hat sich bewegt** — und `new String('ab') === 'ab'` kippte von
            // **locker** auf **stark**. *Zwei Zeilen ohne Gewinn und mit
            // einem Bruch sind keine halbe Loesung, sondern eine neue
            // Fehlermeldung.* Der Befund steht in `team/STATUS_QUIC.txt`,
            // und die naechste Stelle, die zu aendern waere, ist der
            // **`Object`-Konstruktor selbst** (siehe dort), nicht `ToObject`.
            GVal::Ref(r) => Ok(r),
            GVal::Undefiniert | GVal::Null => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "kein Objekt aus null/undefined"),
            )),
            GVal::Bool(b) => self.verpackt(Boxen::Bool(b)).map_err(Stoerung::Fehler),
            GVal::Zahl(n) => self.verpackt(Boxen::Zahl(n)).map_err(Stoerung::Fehler),
            // J24: dokumentierte Naeherung — `Object(2n)` wirft im Motor
            // (es gibt keine `BigInt`-Wrapper-Art im Heap). Die.Methoden
            // von `BigInt.prototype` gehen trotzdem (siehe
            // `eigenschaften.rs`).
            // Release 15.10.2026: a BigInt wrapper (ECMA-262 7.1.18). MEASURED
            // before: `Object(1n)` threw "BigInt kann der Motor nicht in ein
            // Objekt umwandeln" (IndexedDB structured-clone tests).
            GVal::Ganz(g) => self.verpackt(Boxen::Ganz(g)).map_err(Stoerung::Fehler),
        }
    }

    pub(crate) fn typ_von(&self, v: GVal) -> &'static str {
        match v {
            GVal::Undefiniert => "undefined",
            GVal::Null => "object",
            GVal::Bool(_) => "boolean",
            GVal::Zahl(_) => "number",
            // J24: `typeof 2n` -> "bigint".
            GVal::Ganz(_) => "bigint",
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Zeichenkette(_) => "string",
                HObj::Funktion { .. } => "function",
                // An `HObj::Nativ` is a function only if it is a registered
                // native; the platform objects stored the same way (Blob,
                // File, Headers, Request, Response, FormData,
                // AbortController) are objects. MEASURED before: `typeof new
                // Blob([])` was "function" (Chrome: "object").
                HObj::Nativ { .. } => if self.native.contains_key(&r) { "function" } else { "object" },
                // Release 10.10.2026: ein Proxy auf eine Funktion ist eine.
                HObj::Proxy { aufrufbar: true, .. } => "function",
                // J24: `typeof Symbol('x')` -> "symbol". Ein Symbol ist ein
                // Gegenstand mit `proto_symbol` (siehe `symbole.rs`) — an
                // **einer** Stelle erkannt, wie der Modulkopf sagt.
                HObj::Gegenstand { proto: Some(p), .. } if *p == self.proto_symbol => "symbol",
                _ => "object",
            },
        }
    }

    pub(crate) fn ist_aufrufbar(&self, v: GVal) -> bool {
        match v {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Funktion { .. } | HObj::Proxy { aufrufbar: true, .. } => true,
                HObj::Nativ { .. } => self.native.contains_key(&r),
                _ => false,
            },
            _ => false,
        }
    }

    pub(crate) fn gleich_strikt(&self, a: GVal, b: GVal) -> bool {
        match (a, b) {
            (GVal::Undefiniert, GVal::Undefiniert) | (GVal::Null, GVal::Null) => true,
            (GVal::Bool(x), GVal::Bool(y)) => x == y,
            (GVal::Zahl(x), GVal::Zahl(y)) => x == y, // NaN != NaN korrekt.
            (GVal::Ref(x), GVal::Ref(y)) => {
                if x == y {
                    return true;
                }
                // Heap-Strings: Inhalt vergleichen (Identitaet falscht!).
                match (self.heap.holen(x), self.heap.holen(y)) {
                    (HObj::Zeichenkette(sx), HObj::Zeichenkette(sy)) => sx == sy,
                    _ => false,
                }
            }
            (GVal::Zahl(_), _) | (_, GVal::Zahl(_)) => false,
            _ => false,
        }
    }

    /// Verpackten Primitiv erzeugen (`new Number/Boolean`).
    pub(crate) fn verpackt(&mut self, b: Boxen) -> Result<HeapRef, JsFehler> {
        let (b, proto) = match b {
            Boxen::Zahl(n) => (GVal::Zahl(n), self.proto_zahl),
            Boxen::Bool(v) => (GVal::Bool(v), self.proto_bool),
            Boxen::Ganz(g) => (GVal::Ganz(g), self.proto_bigint),
            Boxen::Symbol(s) => (GVal::Ref(s), self.proto_symbol),
            // W14 (Runde 70, **ZURUECKGENOMMEN**): `Boxen::Text` stand hier
            // und ist entfernt — siehe `Boxen`.
        };
        let obj = HObj::Verpackt {
            proto: Some(proto),
            eigenschaften: Default::default(),
            primitiv: b,
            elemente: None,
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten)?;
        let r = self.belege(obj, kosten);
        self.wurzeln.push(GVal::Ref(r));
        Ok(r)
    }

    /// Heap-String-Inhalt (fuer lockere Gleichheit). Verpackte Strings
    /// (`new String`) zaehlen als ihr Primitiv (JS-ToPrimitive-Naeherung).
    pub(crate) fn string_inhalt(&self, v: GVal) -> Option<String> {
        match v {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Zeichenkette(s) => Some(s.clone()),
                HObj::Verpackt {
                    primitiv: GVal::Ref(sr),
                    ..
                } => match self.heap.holen(*sr) {
                    HObj::Zeichenkette(s) => Some(s.clone()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    }

    /// IsLooselyEqual: null/undefined never coerce objects; Symbols are
    /// primitives even though this VM stores them in heap slots.
    pub(crate) fn ist_vergleichsobjekt(&self, value: GVal) -> bool {
        matches!(value, GVal::Ref(r) if !matches!(self.heap.holen(r), HObj::Zeichenkette(_))
            && !crate::symbole::ist_symbol(self, r))
    }

    pub(crate) fn gleich_locker(&mut self, a: GVal, b: GVal) -> Result<bool, Stoerung> {
        if self.gleich_strikt(a, b) { return Ok(true); }
        if matches!(a, GVal::Null | GVal::Undefiniert)
            || matches!(b, GVal::Null | GVal::Undefiniert) {
            return Ok(matches!(a, GVal::Null | GVal::Undefiniert)
                && matches!(b, GVal::Null | GVal::Undefiniert));
        }
        if let GVal::Bool(v) = a {
            return self.gleich_locker(GVal::Zahl(if v { 1.0 } else { 0.0 }), b);
        }
        if let GVal::Bool(v) = b {
            return self.gleich_locker(a, GVal::Zahl(if v { 1.0 } else { 0.0 }));
        }
        match (self.ist_vergleichsobjekt(a), self.ist_vergleichsobjekt(b)) {
            (true, true) => return Ok(false),
            (true, false) => {
                let p = self.zu_primitiv(a, false)?;
                return self.gleich_locker(p, b);
            }
            (false, true) => {
                let p = self.zu_primitiv(b, false)?;
                return self.gleich_locker(a, p);
            }
            _ => {}
        }
        if matches!(a, GVal::Ref(r) if crate::symbole::ist_symbol(self, r))
            || matches!(b, GVal::Ref(r) if crate::symbole::ist_symbol(self, r)) {
            return Ok(false);
        }
        if crate::ganz::ist_ganz(a) || crate::ganz::ist_ganz(b) {
            if let Some(GVal::Bool(result)) = self.ganz_op(crate::ast::BinaerOp::GleichGleich, a, b)? {
                return Ok(result);
            }
        }
        match (a, b) {
            (GVal::Zahl(x), GVal::Zahl(y)) => Ok(x == y),
            (GVal::Zahl(n), GVal::Ref(r)) | (GVal::Ref(r), GVal::Zahl(n)) => {
                Ok(self.heap_string(r).is_some_and(|s| zahl_aus_text(&s) == n))
            }
            _ => Ok(false),
        }
    }

    /// Relationaler Vergleich (`<`, ggf. mit `=`): String-String
    /// lexikalisch (nach UTF-16-Einheiten, JS-Naeherung), sonst numerisch.
    /// NaN -> alle Vergleiche falsch.
    pub(crate) fn kleiner(
        &mut self,
        a: GVal,
        b: GVal,
        oder_gleich: bool,
    ) -> Result<bool, Stoerung> {
        let pa = self.zu_primitiv(a, true)?;
        let pb = self.zu_primitiv(b, true)?;
        let (kleiner, gleich) = match (pa, pb) {
            (GVal::Ref(x), GVal::Ref(y)) => {
                let sx = self.heap_string(x);
                let sy = self.heap_string(y);
                match (sx, sy) {
                    (Some(sx), Some(sy)) => {
                        let xu: Vec<u16> = sx.encode_utf16().collect();
                        let yu: Vec<u16> = sy.encode_utf16().collect();
                        (xu < yu, xu == yu)
                    }
                    _ => {
                        let x = self.zu_zahl(pa)?;
                        let y = self.zu_zahl(pb)?;
                        if x.is_nan() || y.is_nan() {
                            return Ok(false);
                        }
                        (x < y, x == y)
                    }
                }
            }
            _ => {
                let x = self.zu_zahl(pa)?;
                let y = self.zu_zahl(pb)?;
                if x.is_nan() || y.is_nan() {
                    return Ok(false);
                }
                (x < y, x == y)
            }
        };
        let erg = kleiner || (oder_gleich && gleich);
        Ok(erg)
    }

    pub(crate) fn heap_string(&self, r: HeapRef) -> Option<String> {
        match self.heap.holen(r) {
            HObj::Zeichenkette(s) => Some(s.clone()),
            HObj::Verpackt {
                primitiv: GVal::Ref(sr),
                ..
            } => match self.heap.holen(*sr) {
                HObj::Zeichenkette(s) => Some(s.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn zu_int32(&mut self, v: GVal) -> Result<i32, Stoerung> {
        let n = self.zu_zahl(v)?;
        Ok(als_int32(n))
    }

    pub(crate) fn zu_uint32(&mut self, v: GVal) -> Result<u32, Stoerung> {
        let n = self.zu_zahl(v)?;
        Ok(als_uint32(n))
    }
}

pub(crate) fn als_int32(n: f64) -> i32 {
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    let t = n.trunc();
    let m = t % 4294967296.0;
    let m = if m < 0.0 { m + 4294967296.0 } else { m };
    if m >= 2147483648.0 {
        (m - 4294967296.0) as i32
    } else {
        m as i32
    }
}

pub(crate) fn als_uint32(n: f64) -> u32 {
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    let t = n.trunc();
    let m = t % 4294967296.0;
    (if m < 0.0 { m + 4294967296.0 } else { m }) as u32
}

/// Zahl -> Eigenschaftsschluessel (ganzzahlig ohne `.0`).
pub(crate) fn zahl_schluessel(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        crate::wert::zahl_text(n)
    }
}

pub(crate) enum Boxen {
    Bool(bool),
    Zahl(f64),
    /// Release 15.10.2026: BigInt and Symbol wrapper objects.
    Ganz(i128),
    Symbol(HeapRef),
    // W14 (Runde 70, **ZURUECKGENOMMEN**): ein dritter Arm `Text(GVal)`
    // wurde gebaut und gemessen und wieder entfernt. Begruendung in
    // `team/STATUS_QUIC.txt`, Runde 70: die Verpackung von `Object('x')`
    // **aendert nichts** (der `Object`-Weg ruft `zu_objekt` nicht), und sie
    // **verschlechtert** `new String('ab') === 'ab'` von locker auf stark.
    // *Zwei Zeilen ohne Gewinn und mit einem Bruch sind keine halbe Loesung,
    // sondern eine neue Fehlermeldung.*
}

/// JS `String -> Number` (getrimmter, strikter als Rust-parse).
pub(crate) fn zahl_aus_text(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| {
        c.is_whitespace()
            || c == '\u{FEFF}'
            || c == '\u{00A0}'
            || c == '\u{2028}'
            || c == '\u{2029}'
    });
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let b = t.as_bytes();
    if b.len() > 2 && b[0] == b'0' && (b[1] == b'x' || b[1] == b'X') {
        let mut w: f64 = 0.0;
        let mut stellen = 0;
        for c in t[2..].chars() {
            match c.to_digit(16) {
                Some(d) => {
                    w = w * 16.0 + d as f64;
                    stellen += 1;
                }
                None => return f64::NAN,
            }
        }
        if stellen == 0 {
            return f64::NAN;
        }
        return w;
    }
    if b.len() > 2 && b[0] == b'0' && (b[1] == b'b' || b[1] == b'B') {
        let mut w: f64 = 0.0;
        let mut stellen = 0;
        for c in t[2..].chars() {
            match c {
                '0' | '1' => {
                    w = w * 2.0 + (c as u32 - '0' as u32) as f64;
                    stellen += 1;
                }
                '_' => {}
                _ => return f64::NAN,
            }
        }
        if stellen == 0 {
            return f64::NAN;
        }
        return w;
    }
    if b.len() > 2 && b[0] == b'0' && (b[1] == b'o' || b[1] == b'O') {
        let mut w: f64 = 0.0;
        let mut stellen = 0;
        for c in t[2..].chars() {
            match c {
                '0'..='7' => {
                    w = w * 8.0 + (c as u32 - '0' as u32) as f64;
                    stellen += 1;
                }
                '_' => {}
                _ => return f64::NAN,
            }
        }
        if stellen == 0 {
            return f64::NAN;
        }
        return w;
    }
    // Dezimal: Zifferntrennstriche entfernen, dann Rust-parse.
    if t.contains('_') {
        let sauber: String = t.chars().filter(|&c| c != '_').collect();
        return sauber.parse::<f64>().unwrap_or(f64::NAN);
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// Kanonischer Feld-Index aus Schluessel? (`"0"`..`"4294967293"`).
pub(crate) fn index_aus_schluessel(s: &str) -> Option<usize> {
    if s.is_empty() || s.len() > 10 {
        return None;
    }
    if s.len() > 1 && s.starts_with('0') {
        return None;
    }
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u64 = s.parse().ok()?;
    if n >= 4294967295 {
        return None;
    }
    Some(n as usize)
}

/// Release 09.10.2026 (Diagnose): Suchtext fuer `NARSIL_JS_DIAG_FEHLER`.
fn diag_fehler_text() -> Option<&'static String> {
    static T: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    T.get_or_init(|| std::env::var("NARSIL_JS_DIAG_FEHLER").ok().filter(|s| !s.is_empty())).as_ref()
}

/// Release 11.10.2026: die Namen der `DOMException` (WebIDL §3.14.1) mit
/// ihrem alten `code` (0 fuer die neueren Namen).
pub(crate) fn dom_ausnahme_code(name: &str) -> Option<u16> {
    Some(match name {
        "IndexSizeError" => 1,
        "HierarchyRequestError" => 3,
        "WrongDocumentError" => 4,
        "InvalidCharacterError" => 5,
        "NoModificationAllowedError" => 7,
        "NotFoundError" => 8,
        "NotSupportedError" => 9,
        "InUseAttributeError" => 10,
        "InvalidStateError" => 11,
        "SyntaxError" => 12,
        "InvalidModificationError" => 13,
        "NamespaceError" => 14,
        "InvalidAccessError" => 15,
        "TypeMismatchError" => 17,
        "SecurityError" => 18,
        "NetworkError" => 19,
        "AbortError" => 20,
        "URLMismatchError" => 21,
        "QuotaExceededError" => 22,
        "TimeoutError" => 23,
        "InvalidNodeTypeError" => 24,
        "DataCloneError" => 25,
        "EncodingError" | "NotReadableError" | "UnknownError" | "ConstraintError" | "DataError"
        | "TransactionInactiveError" | "ReadOnlyError" | "VersionError" | "OperationError" | "NotAllowedError"
        | "OptOutError" => 0,
        _ => return None,
    })
}
