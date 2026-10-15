// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Eingebaute Objekte, Teil 3 (J4): JSON, Fehler, Map, Set, Date,
//! Promise-Ausfuehrung, RegExp-Laufzeit, console.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fehler::JsFehler;

/// Release 15.10.2026: the Map/Set behind a Map or Set Iterator.
const KARTE_ITER_KARTE: &str = "#es_karte";
use crate::heap::{Eigenschaft, GVal, HObj, Heap, HeapRef};
use crate::interp::{Engine, Stoerung};
use crate::wert::Wert;

impl Engine {
    // ---------------------------------------------------------------
    // JSON
    // ---------------------------------------------------------------

    pub(crate) fn n_json_lesen(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        // J8b: der Leser stellt seine Zwischenwerte als Wurzeln ein — und
        // baute sie bisher **nie** ab. In einer Schleife wuchsen die
        // Wurzeln mit jedem `JSON.parse` (GEMESSEN: 10.000 Runden), der
        // GC-Sammelstapel lief ueber und die Markierung brach ab (dann
        // raeumte der Sweep lebende Objekte weg — Panik). Die Marke steht
        // vor dem Lesen; am Ende werden die Wurzeln wieder abgebaut. Der
        // zurueckgegebene Wert lebt danach im Aufruferrahmen (Stapel oder
        // Wurzel des Aufrufs).
        let wurzel_marke = self.wurzel_merke();
        let s = self.zu_text(self.arg(args, 0))?;
        if s.len() > self.grenzen.max_json_bytes {
            return Err(Stoerung::Fehler(JsFehler::Speicher(
                "JSON-Eingabe zu lang".to_string(),
            )));
        }
        let mut p = JLeser {
            zeichen: s.chars().collect(),
            pos: 0,
            tiefe: 0,
            max_tiefe: self.grenzen.max_json_tiefe,
        };
        // J27: **jeder** Fehler des JSON-Lesers ist ein `SyntaxError`
        // **Wert** (ECMA-262 25.5.1: „It throws a SyntaxError exception"),
        // und die Wurzeln werden auch im Fehlerfall abgebaut.
        //
        // GEMESSEN, warum das eine echte Luecke war und keine Feinheit:
        //   `try { JSON.parse('"a\nb"') } catch (e) { e.name }`
        // lieferte **keinen** `catch`, sondern
        // `Laufzeitfehler: JSON: Steuerzeichen im String` — ein harter
        // Motorfehler umging das `catch`. Der Lauf ueber die echte
        // test262-Suite hatte **22** rote Faelle in
        // `test/built-ins/JSON/parse/` (12 mit „Expected a SyntaxError to
        // be thrown", 10 mit der Steuerzeichen-Meldung).
        //
        // GEMESSEN, warum die Wurzeln hier abgebaut werden muessen: die
        // Marke steht seit J8b vor dem Lesen, aber **kein** Fehlerweg
        // rief `wurzel_zurueck`. Jeder Fehlschlag liess also bis zu
        // `max_json_tiefe` Wurzeln stehen — bei einem Bild, das in einer
        // Schleife `JSON.parse` auf kaputten Text laufen laesst, waere
        // das ein Wachstum ohne Abraeumen.
        let v = match p.wert(self) {
            Ok(v) => v,
            // DIE GRENZE: `JsFehler::Speicher` bleibt ein harter Fehler.
            // Das ist eine Grenze des Motors (`max_json_bytes`), keine
            // Eigenschaft der Eingabe — sie als `SyntaxError` zu melden
            // waere eine Behauptung, die der Laeufer nicht von einer
            // kaputten Eingabe unterscheiden koennte.
            Err(Stoerung::Fehler(JsFehler::Speicher(m))) => {
                self.wurzel_zurueck(wurzel_marke);
                return Err(Stoerung::Fehler(JsFehler::Speicher(m)));
            }
            Err(Stoerung::Fehler(f)) => {
                self.wurzel_zurueck(wurzel_marke);
                return Err(Stoerung::Wurf(
                    self.fehler_wert("SyntaxError", &f.to_string()),
                ));
            }
            Err(andr) => {
                self.wurzel_zurueck(wurzel_marke);
                return Err(andr);
            }
        };
        p.leerraum();
        if p.pos != p.zeichen.len() {
            let m = format!("JSON: Rest nach Wert (Zeichen {})", p.zeichen.len() - p.pos);
            self.wurzel_zurueck(wurzel_marke);
            return Err(Stoerung::Wurf(self.fehler_wert("SyntaxError", &m)));
        }
        // Reviver (optional).
        let rev = self.arg(args, 1);
        if self.ist_aufrufbar(rev) {
            let umh = self
                .neues_objekt(Some(self.proto_objekt))
                .map_err(Stoerung::Fehler)?;
            // ## GEBAUT (JS2/QUIC, 05.10.2026, 05:10) — `CreateDataPropertyOrThrow`,
            // **nicht** `[[Set]]`.
            //
            // ECMA-262 25.5.1.1 Schritt 7.c, woertlich:
            // ```text
            // a. Let root be OrdinaryObjectCreate(%Object.prototype%).
            // b. Let rootName be the empty String.
            // c. Perform ! CreateDataPropertyOrThrow(root, rootName, unfiltered).
            // ```
            //
            // **GEMESSEN, `node 22`, `JSON/parse/reviver-wrapper.js` sagt es
            // woertlich („[[Set]] should not be called.“), und der Unterschied
            // ist **beobachtbar** — `[[Set]]` loest einen **geerbten Setter**
            // aus, `CreateDataPropertyOrThrow` nicht:
            // ```js
            // var g = false;
            // Object.defineProperty(Object.prototype, '', {set(){ g = true }, configurable: true});
            // JSON.parse('2', function () { return this });
            // g      node: false        bei uns (vorher): true
            // ```
            //
            // **UND ES WAR DIE ZWEITE STELLE, nicht die einzige.** Ich hatte
            // zuerst `reviver_gehe` umgestellt (Schritt 2.a.ii.3.a sagt
            // `[[DefineOwnProperty]]`) — **das allein hat nichts geaendert**,
            // weil der Wurf schon **hier** passiert, **eine Ebene ueber** der
            // Reviver-Schleife. *Der naheliegende Ort und der richtige Ort sehen
            // gleich aus* — dieselbe Form wie in Runde 48 und Runde 54.
            //
            // **`create_data_property_or_throw` ist der vorhandene Helfer**
            // (Runde 49, `builtin.rs`) und macht genau Schritt 7.c: er prueft
            // Erweiterbarkeit und Konfigurierbarkeit und **wirft** sonst. Hier
            // ist das Wohlgefall der Norm (`!` = kein Wurf), und der Wurf
            // kommt **nicht** in Frage: `umh` ist eben **neu** und
            // erweiterbar, und `""` ist auf ihm **noch nicht** vorhanden.
            self.create_data_property_or_throw(umh, "", v)?;
            let r = self.reviver_gehe(GVal::Ref(umh), "", rev)?;
            self.wurzel_zurueck(wurzel_marke);
            return Ok(r);
        }
        self.wurzel_zurueck(wurzel_marke);
        Ok(v)
    }

    pub(crate) fn reviver_gehe(
        &mut self,
        halter: GVal,
        schluessel: &str,
        rev: GVal,
    ) -> Result<GVal, Stoerung> {
        let wert = self.hole(halter, schluessel)?;
        if let GVal::Ref(r) = wert {
            let ist_feld = matches!(self.heap.holen(r), HObj::Feld { .. });
            let keys: Vec<String> = if ist_feld {
                match self.heap.holen(r) {
                    HObj::Feld { elemente, .. } => {
                        (0..elemente.len()).map(|i| i.to_string()).collect()
                    }
                    _ => unreachable!(),
                }
            } else if matches!(self.heap.holen(r), HObj::Gegenstand { .. }) {
                self.eigene_schluessel(GVal::Ref(r))?
            } else {
                Vec::new()
            };
            for k in keys {
                let nv = self.reviver_gehe(GVal::Ref(r), &k, rev)?;
                if matches!(nv, GVal::Undefiniert) {
                    self.loesche(GVal::Ref(r), &k)?;
                } else {
                    // ## `CreateDataProperty`, NICHT `Set` — ECMA-262 24.5.2.1
                    // Schritt 7.c.e.3.a.
                    //
                    // **GEMESSEN, und die Tests sagen es woertlich:**
                    // * `JSON/parse/reviver-wrapper.js`: „[[Set]] should not be
                    //   called." — der Test stellt einen `Proxy` davor und
                    //   laeuft bei uns an `Proxy` (nicht vorhanden).
                    // * `reviver-{array,object}-non-configurable-prop-create`
                    //   (2): der Reviver legt `b` als `configurable: false`
                    //   fest und gibt dann `22` zurueck — erwartet wird
                    //   **`b === 2`**, nicht 22.
                    // * `…-prop-delete` (2): derselbe Mechanismus.
                    //
                    // **DIE REGEL:** `CreateDataProperty` ist
                    // `[[DefineOwnProperty]]` mit
                    // `{value, writable:true, enumerable:true, configurable:true}`.
                    // Auf einer **nicht konfigurierbaren** Eigenschaft schlaegt
                    // die Deskriptor-Pruefung fehl, `[[DefineOwnProperty]]` gibt
                    // `false` zurueck — und **kein Wurf** folgt.
                    //
                    // **WARUM DIE EINE BEDINGUNG REICHT** (und nicht bloss
                    // „faustregel" ist): `CreateDataProperty` kann auf einer
                    // nicht konfigurierbaren Eigenschaft nur dann erfolgreich
                    // sein, wenn sie **genau** `{value:neu, writable:true,
                    // enumerable:true, configurable:false}` schon ist — dann
                    // aendert sich **nichts**. In **jedem** anderen Fall
                    // schlaegt die Pruefung fehl, und mein `else` tut nichts.
                    // **Die beiden Wege sind nicht unterscheidbar**, deshalb ist
                    // die eine Bedingung nicht nur kurz, sondern vollstaendig.
                    //
                    // **UND DER HELFER IST DER VORHANDENE:**
                    // `Engine::eig_ist_konfigurierbar` (`builtin.rs`) ist genau
                    // diese Deskriptor-Pruefung, und seit Runde 23 kennt er auch
                    // `HObj::Feld { rest, .. }` — ohne das waere der
                    // Array-Zweig hier durchgefallen.
                    if self.eig_ist_konfigurierbar(GVal::Ref(r), &k) {
                        // ## GEBAUT (JS2/QUIC, 05.10.2026, 05:05)
                        //
                        // **ECMA-262 25.5.1.1 `InternalizeJSONProperty`
                        // Schritt 2.a.ii.3.a / 2.b.ii.3.a:**
                        // ```text
                        // Perform ? val.[[DefineOwnProperty]](P, {
                        //     [[Value]]: status, [[Writable]]: true,
                        //     [[Enumerable]]: true, [[Configurable]]: true }).
                        // ```
                        //
                        // **`[[DefineOwnProperty]]`, NICHT `[[Set]]`** — und der
                        // Unterschied ist **beobachtbar**, nicht theoretisch:
                        // `[[Set]]` loest einen **geerbten Setter** aus,
                        // `[[DefineOwnProperty]]` nicht.
                        //
                        // **GEMESSEN, `node 22`, und `JSON/parse/reviver-
                        // wrapper.js` sagt es woertlich: „[[Set]] should not be
                        // called.“** Der Test stellt einen Setter auf
                        // `Object.prototype`:
                        // ```js
                        // var g = false;
                        // Object.defineProperty(Object.prototype, '', {set(){ g = true }, configurable: true});
                        // JSON.parse('2', function () { return this });
                        // g      node: false        bei uns (vorher): true
                        // ```
                        // **UND NUR DIESER EINZIGE PUNKT WAR FALSCH.** Alles
                        // andere am Wurzel-Wrapper stimmte schon (GEMESSEN):
                        // ```text
                        // eigene Namen          node: 1          bei uns: 1
                        // Prototyp === Object   node: true       bei uns: true
                        // isExtensible          node: true       bei uns: true
                        // Descriptor der ''     node: 2/true/true/true  bei uns: gleich
                        // ```
                        //
                        // **`eigenschaft_mit_attributen` ist der vorhandene
                        // Helfer** und setzt **genau** den Norm-Deskriptor:
                        // `aufzaehlbar = true`, `beschreibbar = true` und
                        // `konfigurierbar = true` fest verdrahtet. Fuenfter
                        // Beleg fuer die Regel aus Runde 30 — erst `grep`, dann
                        // nehmen.
                        //
                        // **DIE BEDINGUNG DARUEBER bleibt** und ist **richtig**:
                        // `[[DefineOwnProperty]]` schlaegt auf einer nicht
                        // konfigurierbaren Eigenschaft fehl und gibt `false`
                        // zurueck — **kein Wurf** (25.5.1.1 sagt `?` nur fuer den
                        // **Aufruf**, nicht fuer das Ergebnis).
                        self.eigenschaft_mit_attributen(r, &k, nv, true, true)?;
                    }
                }
            }
        }
        let kt = self.text_wert(schluessel).map_err(Stoerung::Fehler)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(halter);
        self.wurzeln.push(kt);
        self.wurzeln.push(wert);
        let r = self.aufruf_synchron(rev, halter, &[kt, wert], false);
        self.wurzel_zurueck(marke);
        r
    }

    /// ECMA-262 24.5.2.1 Schritt 7.c.e: hat die Box `[[StringData]]` oder
    /// `[[NumberData]]`, ist ihr Eintrag im Replacer-Array `? ToString(v)`;
    /// sonst `undefined` (und der Eintrag zaehlt dann nach Schritt 7.c.d
    /// **nicht**).
    ///
    /// **WARUM EIN EIGENER HELFER und nicht ein `match` an der Aufrufstelle:**
    /// die Regel braucht `&mut self` (`zu_text` darf Benutzercode aufrufen),
    /// und die Aufrufstelle steht **innerhalb** eines `match` ueber `self.arg`.
    /// Das liest sich als drei Zeilen und ist es nicht.
    ///
    /// **UND DIE REIHENFOLGE DER ZWEI METHODEN ist hier der ganze Inhalt:**
    /// `ToString` auf einer Box ist `ToPrimitive(v, string)` — **`toString`
    /// zuerst**, `valueOf` **nie** bei einer Number-Box, weil `toString`
    /// antwortet. `replacer-array-number-object.js` wirft in `valueOf`
    /// ausdruecklich, damit das geprueft wird.
    ///
    /// M3: **keine** Allokation ausser der Erzwung selbst (die kommt aus
    /// fremdem Input und wird von `zu_text` gebucht). `None` heisst „kein
    /// Schluessel" — `bool` ist nicht `false` im Sinne von 7.c.d.
    fn ersatz_schluessel_aus_box(&mut self, v: GVal) -> Result<Option<String>, Stoerung> {
        let GVal::Ref(r) = v else { return Ok(None) };
        let art = match self.heap.holen(r).clone() {
            HObj::Verpackt { primitiv, .. } => match primitiv {
                GVal::Zahl(_) => Some("Zahl"),
                GVal::Ref(sr) if matches!(self.heap.holen(sr), HObj::Zeichenkette(_)) => {
                    Some("Text")
                }
                // 7.c.d: kein `[[StringData]]`/`[[NumberData]]` -> der Eintrag
                // ist `undefined` und wird **nicht** benutzt.
                _ => None,
            },
            _ => None,
        };
        match art {
            Some(_) => Ok(Some(self.zu_text(v)?)),
            None => Ok(None),
        }
    }

    pub(crate) fn n_json_schreiben(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let ersatz = self.arg(args, 1);
        let ersatz_fn = self.ist_aufrufbar(ersatz);
        // ## JS2/QUIC (03.10.2026, 01:15): DIE **REIHENFOLGE** (W21).
        //
        // ECMA-262 25.5.4.5 `SerializeJSONObject` Schritt 5.b/7: ist
        // `PropertyList` gesetzt, **ist K die Liste selbst** — in ihrer
        // Reihenfolge. Wir hatten nur eine `HashSet`, also war die Reihenfolge
        // an der Wurzel verloren und `PropertyList` diente als **Filter**.
        //
        // GEMESSEN, `node 22`:
        // ```js
        // JSON.stringify({c:3,b:1,a:2}, ['b','a','c'])
        //   node: {"b":1,"a":2,"c":3}    bei uns (vorher): {"c":3,"b":1,"a":2}
        // ```
        //
        // **BEIDE** Strukturen: die `HashSet` beantwortet „gehoert der
        // Schluessel dazu?“ in **O(1)**, die Liste „in welcher Reihenfolge?“.
        // **DAS IST AUCH DIE ANTWORT AUF MEINE EIGENE SORGE** aus W21 (eine
        // `Vec` brauche ein `MAX`, M1–M9): **nein** — die Liste waechst mit
        // der **Laenge des Eingabefeldes**, und die ist ueber `MAX_SAMMLUNG`
        // begrenzt. Das Problem waere ein **O(n²)-Dedupe** gewesen; **ein**
        // `insert` als Dedupe-Zeiger ist **O(1)**.
        let mut ersatz_liste: Vec<String> = Vec::new();
        let ersatz_array: Option<HashSet<String>> = match ersatz {
            GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Feld { .. }) => {
                let mut set = HashSet::new();
                if let HObj::Feld { elemente, .. } = self.heap.holen(r).clone() {
                    for e in elemente {
                        if let GVal::Ref(sr) = e {
                            if let HObj::Zeichenkette(s) = self.heap.holen(sr) {
                                // **`insert` entscheidet**: `true` heisst
                                // **neu**, und **nur dann** gehoert der
                                // Schluessel in die Liste. Das ist der
                                // Dedupe — `['a','a','b']` darf **ein** `a`
                                // liefern (`node 22`: `{"a":1,"b":2}`).
                                if set.insert(s.clone()) {
                                    ersatz_liste.push(s.clone());
                                }
                            }
                        }
                        match e {
                            GVal::Zahl(n) => {
                                let k = crate::ops::zahl_schluessel(n);
                                if set.insert(k.clone()) {
                                    ersatz_liste.push(k);
                                }
                            }
                            // ## ECMA-262 24.5.2.1 Schritt 7.c.e — der
                            // ERSETZER-ARRAY-EINTRAG, und er fehlte ganz
                            //
                            // ```text
                            // e. Else if Type(v) is Object, then
                            //    i. If v has a [[StringData]] or [[NumberData]]
                            //       internal slot, set item to ? ToString(v).
                            // ```
                            //
                            // GEMESSEN (07:20), `node 22`:
                            //
                            // ```js
                            // var num = new Number(10);
                            // num.toString = function () { return 'toString' };
                            // num.valueOf = function () { throw new Error('NEIN') };
                            // JSON.stringify({toString: 2, valueOf: 3}, [num])
                            //   -> node: '{"toString":2}'    bei uns: '{}'
                            // ```
                            //
                            // **DER TEST PRUEFT AUCH, DASS `valueOf` NICHT
                            // GERUFEN WIRD** — er wirft dort. `ToString` auf
                            // einer Box ist `ToPrimitive(v, string)`, also
                            // **`toString` zuerst**. Genau das macht `zu_text`.
                            //
                            // VORHER: nur `HObj::Zeichenkette` und `GVal::Zahl`
                            // wurden uebernommen. Eine **Box** war weder das eine
                            // noch das andere, also kam ihr Schluessel **nie**
                            // in die Menge — und `{}` war die Antwort.
                            //
                            // M3: **eine** `ToString`-Erzwung pro Box im
                            // Replacer-Array, und die kommt aus **fremdem
                            // Input** — sie kann ein Getter aufrufen, dessen
                            // Kosten der Aufrufmechanismus bucht. Die Menge
                            // selbst waechst mit der Laenge des Arrays, und
                            // die ist ein Feld aus dem Programm.
                            GVal::Ref(sr)
                                if matches!(self.heap.holen(sr), HObj::Verpackt { .. }) =>
                            {
                                if let Some(t) = self.ersatz_schluessel_aus_box(GVal::Ref(sr))? {
                                    // Der `push` **vor** dem `insert`: `insert`
                                    // **bewegt** `t` (Gewicht genommen), und die
                                    // Liste braucht ihren **Wert**.
                                    if set.insert(t.clone()) {
                                        ersatz_liste.push(t);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some(set)
            }
            _ => None,
        };
        // JS2 (02.10.2026, 00:25): `space` durch `ToPrimitive`.
        //
        // GEMESSEN, was vorher lief: 3 rote test262-Faelle
        // (`space-number-object.js`, `space-number-float.js`,
        // `space-string-object.js`) — alle drei uebergeben eine **Box**
        // (`new Number(5.11111)`, `new String('xxx')`). Die Rechnung war
        // schon richtig und ist es weiter: sieben Proben mit einer
        // **primitiven** Zahl (0, 2, 11, 1.5, -1.99999, 'xy', verschachtelt)
        // stimmen **wie node**.
        //
        // DIE VIER FAELLE, und es sind vier (nicht zwei):
        //
        // | `space`            | was                            | Weg          |
        // |--------------------|--------------------------------|--------------|
        // | `2`                | Zahl                           | `einrueck_aus_zahl` |
        // | `'xy'`             | String                         | `einrueck_aus_text` |
        // | `new Number(5.1)`  | Box um eine Zahl               | `ToNumber`   |
        // | `new String('xxx')`| Box um einen Text              | `ToString`   |
        //
        // DIE HINWEIS-REIHENFOLGE ist der ganze Unterschied zwischen Zeile 3
        // und 4, und sie ist gemessen (00:27): `new String('x')` mit
        // `toString = () => '---'` und `valueOf = throw` ergibt in node
        // `'---'`. Ein Motor, der dort `valueOf` zuerst ruft, wirft. Ich hatte
        // genau diesen Fehler: im `Ref(sr)`-Zweig stand die Text-Erkennung an
        // der Stelle der Zahl-Erkennung, also landete die String-Box im
        // Zahl-Zweig. Der Probe hat es gezeigt, nicht der test262-Fall.
        //
        // `true`/`{}` ergeben keinen Einrueck — `node 22`, 00:27:
        // `JSON.stringify({a:1},null,true)` -> `{"a":1}`.
        //
        // DIE REGELN (`einrueck_aus_zahl`, `einrueck_aus_text`) stehen in
        // `eingebaut_json.rs` mit sechs Unit-Tests. GETEILTE DATEI: die
        // Verdrahtung hier, gemeldet in `team/WUNSCH_JS.txt`.
        let einrueck = match self.arg(args, 2) {
            GVal::Zahl(n) => crate::eingebaut_json::einrueck_aus_zahl(n),
            GVal::Ref(r) => {
                // Die Information **vor** dem Entlehren kopieren: `zu_zahl`
                // und `zu_text` brauchen `&mut self`.
                let mut boxe = None; // (ist_zahl, ist_text)
                if let HObj::Verpackt { primitiv, .. } = self.heap.holen(r).clone() {
                    boxe = Some(match primitiv {
                        GVal::Zahl(_) => (true, false),
                        GVal::Ref(sr) => {
                            (false, matches!(self.heap.holen(sr), HObj::Zeichenkette(_)))
                        }
                        _ => (false, false),
                    });
                }
                // Ein **primitiver** String: `HObj::Zeichenkette`. Kein
                // `ToPrimitive` noetig — er ist schon ein Text.
                let primitiver_text = match self.heap.holen(r) {
                    HObj::Zeichenkette(s) => Some(s.clone()),
                    _ => None,
                };
                if let Some(t) = primitiver_text {
                    crate::eingebaut_json::einrueck_aus_text(&t)
                } else if let Some((true, _)) = boxe {
                    // Number-Box: `ToNumber` — `valueOf` zuerst.
                    let n = self.zu_zahl(GVal::Ref(r))?;
                    crate::eingebaut_json::einrueck_aus_zahl(n)
                } else if let Some((_, true)) = boxe {
                    // String-Box: `ToString` — `toString` zuerst.
                    let t = self.zu_text(GVal::Ref(r))?;
                    crate::eingebaut_json::einrueck_aus_text(&t)
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        };
        // ## `ersatz_array` wird **eine Zeile darunter bewegt** (`erlaubnis:`),
        // also merke ich mir das `is_some()` **hier** — sonst kommt
        // `E0382: borrow of moved value`.
        let hat_replacer_array = ersatz_array.is_some();
        let mut schreiber = JSchreiber {
            ersatz: if ersatz_fn { Some(ersatz) } else { None },
            erlaubnis: ersatz_array,
            // ## DER FEHLER, DEN DIESER BAU HIER GEMACHT HAT — **meiner**,
            // und er war **gross**.
            //
            // `Some(ersatz_liste.clone())` **immer** zu setzen war falsch:
            // **ohne** Replacer ist die Liste **leer**, also wurde `keys` leer
            // und **jedes** Objekt serialisierte zu `{}`.
            //
            // GEMESSEN, `node 22`, gegen diesen Zwischenstand:
            // ```js
            // JSON.stringify({c:3,b:1,a:2})              node: {"c":3,"b":1,"a":2}
            // JSON.stringify({b:1,a:2}, function(k,v){…}) node: {"b":1,"a":2}
            // JSON.stringify({a:1}, 5)                    node: {"a":1}
            // JSON.stringify({a:{b:1}}, ['a'])             node: {"a":{"b":1}}
            // ```
            // -> **alle vier `{}` bzw. `{"a":{}}`**. Sieben Kontrollfaelle auf
            // **ein** zusaetzliches `Option`, und `check` war **gruen**.
            //
            // **DIE REGEL:** ein `Option`, das man **immer** mit `Some`
            // fuellt, ist **kein** `Option`. Und ein Feld, das nur in
            // **einem** Pfad benutzt wird, braucht in **jedem** Pfad eine
            // Entscheidung — nicht in einem.
            reihenfolge: if hat_replacer_array {
                Some(ersatz_liste.clone())
            } else {
                None
            },
            einrueck,
            stapel: Vec::new(),
            max_tiefe: self.grenzen.max_json_tiefe,
        };
        // ## DER WRAPPER — ECMA-262 24.5.2.1 Schritt 9 und 10
        //
        // ```text
        //  9.  Let wrapper be ObjectCreate(%ObjectPrototype%).
        //  10. Let status be CreateDataProperty(wrapper, the empty String, value).
        // ```
        //
        // **GEMESSEN (06:40):** vorher stand hier
        // `schreiber.serial(self, v, None, GVal::Undefiniert, 0)` — als
        // `this` des Replacers also **`undefined`**, und `aufruf_synchron`
        // macht daraus den **globalen Bereich**. Folge:
        //
        // ```js
        // var w; JSON.stringify({}, function () { w = this });
        // Object.getOwnPropertyNames(w)
        //   -> uns: "Array,BigInt,Boolean,Date,Error,…"  (56 Namen!)
        //   -> node: ""
        // ```
        //
        // Das ist der Fall `replacer-function-wrapper.js`, und er ist der
        // unangenehmste, weil der Replacer bei einem **Feld** richtig lief
        // (`Object.getOwnPropertyNames(this)` war `"0,1,length"` wie in node) —
        // nur der **oberste** Aufruf hatte den falschen `this`.
        //
        // **WARUM DAS WICHTIG IST und nicht nur `Object.keys`-Kosmetik:** der
        // Replacer darf ueber `this` den Halter lesen. Mit dem globalen
        // Bereich sieht er statt `{'': value}` **den ganzen globalen Zustand** —
        // jede Enumeration, die ein Replacer macht, liefert dann Unsinn. Der
        // Test installiert zudem einen **Setter** fuer `""` auf
        // `Object.prototype` und erwartet, dass er **nicht** feuert: ein
        // `ObjectCreate` + `CreateDataProperty` feuert ihn nicht, ein
        // Schreibzugriff auf den Prototyp schon.
        //
        // M3: **eine** Allokation (der Wrapper), und sie ist aus fremdem Input
        // — aber `neues_objekt` bucht die Kosten ueber `vor_allok`. Keine
        // Schleife.
        let v = self.arg(args, 0);
        let huelle = self
            .neues_objekt(Some(self.proto_objekt))
            .map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(huelle));
        self.setze_eigen(huelle, "", v);
        self.wurzeln.push(v);
        match schreiber.serial(self, v, Some(String::new()), GVal::Ref(huelle), 0)? {
            Some(s) => self.text_wert(&s).map_err(Stoerung::Fehler),
            None => Ok(GVal::Undefiniert),
        }
    }

    // ---------------------------------------------------------------
    // Fehler
    // ---------------------------------------------------------------

    pub(crate) fn n_fehler_ctor(
        &mut self,
        art: &str,
        args: &[GVal],
        _neu: bool,
    ) -> Result<GVal, Stoerung> {
        // Error(...) und new Error(...) sind identisch.
        // Release 10.10.2026: `new AggregateError(errors, message, options)`
        // — die Nachricht steht an ZWEITER Stelle, `errors` ist eine Liste
        // aus dem Iterable (eigene, nicht aufzaehlbare Eigenschaft).
        if art == "AggregateError" {
            let liste = self.iter_werte(self.arg(args, 0))?;
            let feld = self.feld_aus_werten(&liste)?;
            self.wurzeln.push(GVal::Ref(feld));
            let rest: Vec<GVal> = args.iter().skip(1).copied().collect();
            let e = self.n_fehler_ctor("AggregateError:intern", &rest, _neu)?;
            if let GVal::Ref(r) = e {
                self.fehler_eigen_versteckt(r, "errors", GVal::Ref(feld));
            }
            return Ok(e);
        }
        let art = art.strip_suffix(":intern").unwrap_or(art);
        let nachricht = if matches!(self.arg(args, 0), GVal::Undefiniert) {
            String::new()
        } else {
            self.zu_text(self.arg(args, 0))?
        };
        // JS2 (02.10.2026, 03:02): `URIError` und `EvalError` bekommen ihre
        // **eigenen** Prototypen.
        //
        // VORHER stand hier kein Arm fuer sie, also `_ => self.proto_fehler`.
        // Zusammen mit dem Setup-Kurzweg (dort bekamen sie `proto_fehler`
        // auch als `mk_ctor`-Prototyp) ergab das **dieselbe** Fehlerklasse
        // fuer `Error`, `EvalError` und `URIError`.
        //
        // GEMESSEN 02:52–02:58, sechs Proben gegen `node 22`:
        //
        //   Error.prototype === EvalError.prototype   true / false
        //   Error.prototype === URIError.prototype    true / false
        //   Error.prototype.constructor.name          URIError / Error
        //   (new Error('x')).constructor.name          URIError / Error
        //   (new Error('x')) instanceof EvalError     true / false
        //   new Set([1]).forEach(throw)                URIError / Error
        //
        // **BEWEIS OHNE `node`:** `Error !== EvalError` und
        // `Error.prototype === EvalError.prototype`. Ein Objekt hat **einen**
        // `constructor`-Slot; zwei verschiedene Konstruktoren koennen nicht
        // beide darauf stehen. Der Motor widerspricht sich also selbst.
        //
        // **DER WEG ZU DEN PROTOTYPEN, und warum er ohne `Engine`-Feld
        // auskommt:** `X.prototype` ist eine eigene lesbare Eigenschaft des
        // Konstruktors (02:59 gemessen, `typeof` = `object`), und der
        // Konstruktor steht in `global_objekt`. Also zwei `hole`-Aufrufe.
        //
        // M3: KEINE Allokation hier — nur zwei Lesezugriffe und ein Muster.
        // Der Prototyp ist bereits im Heap, und `n_fehler_ctor` legt nichts
        // an, was eine Grenze brauchte.
        let proto = match art {
            "TypeError" => self.proto_typfehler,
            "RangeError" => self.proto_bereichsfehler,
            "SyntaxError" => self.proto_syntaxfehler,
            "ReferenceError" => self.proto_referenzfehler,
            "URIError" | "EvalError" | "AggregateError" => {
                let mut gefunden = self.proto_fehler;
                if let Ok(GVal::Ref(konstruktor)) = self.hole(GVal::Ref(self.global_objekt), art)
                    && let Ok(GVal::Ref(p)) = self.hole(GVal::Ref(konstruktor), "prototype")
                {
                    gefunden = p;
                }
                gefunden
            }
            _ => self.proto_fehler,
        };
        let obj = HObj::Fehler {
            proto: Some(proto),
            name: art.to_string(),
            nachricht: nachricht.clone(),
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = Heap::kosten_fuer(&obj) + nachricht.len();
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        let r = self.belege(obj, kosten);
        self.wurzeln.push(GVal::Ref(r));
        // Release 09.10.2026: `message` nur, wenn eine Nachricht kam, und
        // nicht aufzaehlbar; `name` erbt das Objekt (ES2023 20.5.1.1).
        // GEMESSEN (node 22): `new Error().hasOwnProperty("message")` ->
        // `false`, `Object.keys(new Error("x"))` -> `[]`.
        if !matches!(self.arg(args, 0), GVal::Undefiniert) {
            let m = self.text_wert(&nachricht).map_err(Stoerung::Fehler)?;
            self.wurzeln.push(m);
            self.fehler_eigen_versteckt(r, "message", m);
        }
        let kopf = if nachricht.is_empty() { art.to_string() } else { format!("{art}: {nachricht}") };
        self.stack_setzen(r, &kopf);
        // cause (ES2022): zweites Argument {cause}.
        if let GVal::Ref(cr) = self.arg(args, 1) {
            let c = self.hole(GVal::Ref(cr), "cause")?;
            if !matches!(c, GVal::Undefiniert) {
                self.wurzeln.push(c);
                // Release 10.10.2026: NICHT aufzaehlbar (InstallErrorCause:
                // CreateNonEnumerableDataPropertyOrThrow). GEMESSEN vorher:
                // `Object.keys(new Error('x', {cause: 1})).length` -> 1 (node: 0).
                self.fehler_eigen_versteckt(r, "cause", c);
            }
        }
        Ok(GVal::Ref(r))
    }

    pub(crate) fn n_fehler_text(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        // Release 09.10.2026: ES2023 20.5.3.4 — `name` und `message` ueber
        // die Prototypkette (`Get`), fuer JEDES Objekt. GEMESSEN (node 22):
        // `class F extends Error {}; F.prototype.name = "F"; String(new F("m"))`
        // -> `F: m`; `Error.prototype.toString.call({name: "X", message: "y"})`
        // -> `X: y`. Der Motor las nur eigene Felder eines Fehlerobjekts.
        if !matches!(this, GVal::Ref(_)) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Error.prototype.toString braucht ein Objekt"),
            ));
        }
        let n = self.hole(this, "name")?;
        let name = if matches!(n, GVal::Undefiniert) { "Error".to_string() } else { self.zu_text(n)? };
        let m = self.hole(this, "message")?;
        let nachricht = if matches!(m, GVal::Undefiniert) { String::new() } else { self.zu_text(m)? };
        let s = if nachricht.is_empty() {
            name
        } else if name.is_empty() {
            nachricht
        } else {
            format!("{name}: {nachricht}")
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    // ---------------------------------------------------------------
    // Map
    // ---------------------------------------------------------------

    pub(crate) fn karte_als_ref(&mut self, this: GVal, name: &str) -> Result<HeapRef, Stoerung> {
        match this {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Karte { .. } => Ok(r),
                _ => Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", &format!("{name} braucht Map")),
                )),
            },
            _ => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", &format!("{name} braucht Map")),
            )),
        }
    }

    /// ECMA-262 24.1.1.1 `Map ( [ iterable ] )`, **Schritt 1**:
    /// *„If NewTarget is undefined, throw a TypeError exception."*
    ///
    /// **GEMESSEN (23:55), `node 22` gegen uns — und die Luecke ist breiter
    /// als `Map`:**
    ///
    /// | Aufruf | `node 22` | bei uns |
    /// |---|---|---|
    /// | `Map()` / `Map([])` | **Wurf** | **kein Wurf** |
    /// | `Set()` / `Set([])` | **Wurf** | **kein Wurf** |
    /// | `new Map()` / `new Set([1,2])` | ok | ok |
    ///
    /// Und **was nicht** werfen muss (sonst bricht man die anderen):
    /// `Array()`, `Object()`, `Date()`, `RegExp()`, `Symbol()`, `Error()` —
    /// alle **ohne** Wurf bei node. `JSON()` und `Promise()` werfen (bei uns
    /// schon richtig).
    ///
    /// **DIE FLAGGE WAR SCHON DA — und hiess `_neu`.** Die Verdrahtung
    /// (`builtin2.rs`: `KarteCtor => self.n_karte_ctor(args, als_konstruktor)`)
    /// hat den Wert **geliefert**, und die Funktion hat ihn **verworfen**. Das
    /// ist nicht „eine fehlende Pruefung", sondern eine **vorhandene Angabe,
    /// die niemand benutzt** — die Form, die man beim Lesen am leichtesten
    /// uebersieht, weil sie da steht und aussieht wie Absicht.
    pub(crate) fn n_karte_ctor(&mut self, args: &[GVal], neu: bool) -> Result<GVal, Stoerung> {
        if !neu {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Map: ohne new"),
            ));
        }
        let obj = HObj::Karte {
            proto: Some(self.proto_karte),
            index: std::collections::HashMap::new(),
            eintraege: Vec::new(),
            belegt: Vec::new(), // W35: leere Record (heap.rs)
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        let r = self.belege(obj, kosten);
        let m = GVal::Ref(r);
        if !matches!(self.arg(args, 0), GVal::Null | GVal::Undefiniert) {
            let iter = self.iter_werte(self.arg(args, 0))?;
            // ## DER KONSTRUKTOR RUFT `Map.prototype.set` — ECMA-262 24.5.1.3
            // Schritt 10-11.
            //
            // **GEMESSEN (`Map/iterable-calls-set.js`, `node 22` als Orakel
            // nicht noetig — der Test sagt es):** mit ueberschriebenem
            // `Map.prototype.set` meldet er „called **0** times" (erwartet 2).
            // Und `Map/map-iterable-throws-when-set-is-not-callable.js` setzt
            // `set = null` und erwartet einen `TypeError`.
            //
            // **BEIDES IST EINE URSACHE:** wir schrieben direkt in die
            // Tabelle (`karte_setzen_innen`) und sind damit an der **Methode**
            // vorbeigelaufen. Die Norm holt `adder` **einmal** vor der
            // Schleife und ruft es je Eintrag mit `this = map`.
            //
            // **UND DAS IST NICHT NUR BEQUEMER:** ein Konstruktor, der an der
            // Methode vorbeigeht, ist nicht nur anders, sondern **falsch** —
            // `Map.prototype.set` ist ein **oeffentlicher** Einstieg, und
            // `Engine::aufruf_synchron` landet bei unveraenderter `adder` in
            // `n_karte_setzen` -> `karte_setzen_innen`, also im **denselben**
            // Pfad wie vorher. **Es gibt damit wieder nur einen Pfad, der
            // den Index schreibt** — die Absicht aus Runde 19 bleibt erhalten.
            let adder = self.hole(GVal::Ref(r), "set")?;
            if !self.ist_aufrufbar(adder) {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Map.prototype.set ist nicht aufrufbar"),
                ));
            }
            for e in iter {
                let paar = self.iter_werte(e)?;
                if paar.len() < 2 {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("TypeError", "Map-Eintrag kein Paar"),
                    ));
                }
                // M6: `adder`, `this` und die Argumente sind **Wurzeln** — der
                // Aufruf darf allozieren (eine ueberschriebene `set` legt
                // sicher etwas an).
                self.wurzeln.push(adder);
                self.wurzeln.push(m);
                self.wurzeln.push(paar[0]);
                self.wurzeln.push(paar[1]);
                self.aufruf_synchron(adder, m, &[paar[0], paar[1]], false)?;
            }
        }
        Ok(m)
    }

    pub(crate) fn karte_setzen_innen(
        &mut self,
        r: HeapRef,
        k: GVal,
        v: GVal,
    ) -> Result<(), JsFehler> {
        // JS2 (03.10.2026, 10:45): der Schluessel wird **einmal** hier
        // kanonisiert, **vor** der ersten Leine. Das ist die **zweite
        // Haelfte** der `-0`-Regel: der Hash waere ohne das richtig, aber der
        // **gespeicherte** Schluessel bliebe `-0`, und
        // `1/[...m.keys()][0]` waere `-Infinity` statt `Infinity`
        // (`Map/prototype/set/replaces-a-value-normalizes-zero-key.js`).
        //
        // **WARUM HIER UND NICHT IM HASH-AUFRUF:** `holen_mut` ist weiter
        // unten, und `kanonischer_schluessel` braucht `&mut self` — zwei
        // gleichzeitige Leihen sind nicht moeglich. **Die Regel an den Anfang
        // der Funktion zu ziehen ist der einzige Ort, an dem sie gleichzeitig
        // sichtbar und anwendbar ist.**
        let k = self.kanonischer_schluessel(k);
        // J8: **Index** statt linearem Scan (der Scan war der Grund fuer
        // die Zeitgrenze: 10^5 `set` kosteten 129.581 ms). Der Hash
        // (`schluessel_hash`) passt zu `schluessel_gleich`, deshalb
        // muss nur noch die (seltene) Hash-Kollision geprueft werden.
        let h = crate::eingebaut_menge::schluessel_hash_normiert(&self.heap, k);
        let pos = match self.heap.holen(r) {
            HObj::Karte {
                eintraege, index, ..
            } => index.get(&h).and_then(|liste| {
                liste
                    .iter()
                    .copied()
                    .find(|i| schluessel_gleich(&self.heap, eintraege[*i].0, k))
            }),
            _ => return Err(JsFehler::Laufzeit("Map korrupt (intern)".to_string())),
        };
        let o = self.heap.holen_mut(r);
        if let HObj::Karte { eintraege, .. } = o {
            if let Some(i) = pos {
                eintraege[i].1 = v;
                return Ok(());
            }
            if eintraege.len() > 1_000_000 {
                return Err(JsFehler::Speicher("Map zu gross".to_string()));
            }
            let pos = eintraege.len();
            eintraege.push((k, v));
            if let HObj::Karte { index, belegt, .. } = self.heap.holen_mut(r) {
                index.entry(h).or_default().push(pos);
                belegt.push(true); // W35: neuer Record ist belegt
            }
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG * 2);
            Ok(())
        } else {
            Err(JsFehler::Laufzeit("Map korrupt (intern)".to_string()))
        }
    }

    pub(crate) fn n_karte_holen(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let r = self.karte_als_ref(this, "get")?;
        let k = self.arg(args, 0);
        // J8: Index statt Scan (Begruendung bei `karte_setzen_innen`).
        let h = crate::eingebaut_menge::schluessel_hash_normiert(&self.heap, k);
        if let HObj::Karte {
            eintraege, index, ..
        } = self.heap.holen(r)
        {
            if let Some(liste) = index.get(&h)
                && let Some(i) = liste
                    .iter()
                    .copied()
                    .find(|i| schluessel_gleich(&self.heap, eintraege[*i].0, k))
            {
                return Ok(eintraege[i].1);
            }
        }
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn n_karte_setzen(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let r = self.karte_als_ref(this, "set")?;
        let k = self.arg(args, 0);
        let v = self.arg(args, 1);
        let marke = self.wurzel_merke();
        self.wurzeln.push(k);
        self.wurzeln.push(v);
        self.karte_setzen_innen(r, k, v).map_err(Stoerung::Fehler)?;
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(r))
    }

    pub(crate) fn n_karte_haben(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let r = self.karte_als_ref(this, "has")?;
        let k = self.arg(args, 0);
        // J8: Index statt Scan.
        let h = crate::eingebaut_menge::schluessel_hash_normiert(&self.heap, k);
        if let HObj::Karte {
            eintraege, index, ..
        } = self.heap.holen(r)
            && let Some(liste) = index.get(&h)
            && liste
                .iter()
                .any(|i| schluessel_gleich(&self.heap, eintraege[*i].0, k))
        {
            return Ok(GVal::Bool(true));
        }
        Ok(GVal::Bool(false))
    }

    pub(crate) fn n_karte_loeschen(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let r = self.karte_als_ref(this, "delete")?;
        let k = self.arg(args, 0);
        // J8: Index statt Scan — erst die **Position** suchen (unveraenderte
        // Leihen), dann aendern.
        let h = crate::eingebaut_menge::schluessel_hash_normiert(&self.heap, k);
        let pos = match self.heap.holen(r) {
            HObj::Karte {
                eintraege, index, ..
            } => index.get(&h).and_then(|liste| {
                liste
                    .iter()
                    .copied()
                    .find(|i| schluessel_gleich(&self.heap, eintraege[*i].0, k))
            }),
            _ => None,
        };
        let Some(p) = pos else {
            return Ok(GVal::Bool(false));
        };
        // W35 (QUIC, 03.10.2026) — **MARKIEREN STATT ENTFERNEN**, wie bei
        // `n_menge_loeschen`; dort steht die vollstaendige Begruendung mit den
        // Messungen. ECMA-262 24.5.4.3 Schritt 5 setzt `p.[[Key]]` auf
        // *empty* und laesst den Record stehen — sonst bricht ein haengender
        // Iterator, und ein wieder eingefuegter Schluessel kaeme an die
        // **falsche** Stelle (`does-not-break-iterators.js`,
        // `iterates-values-deleted-then-readded.js`).
        //
        // **AUCH DER PREIS stimmt jetzt:** vorher war ein `delete` **O(n)**
        // (alle Hashes berechnen, Index neu bauen) — fuer den haeufigsten
        // Aufruf der Sammlung. Jetzt ist es O(1) plus die Laenge der
        // Hash-Liste.
        let frei = {
            let o = self.heap.holen_mut(r);
            if let HObj::Karte { index, belegt, .. } = o {
                if let Some(liste) = index.get_mut(&h) {
                    liste.retain(|i| *i != p);
                }
                if let Some(b) = belegt.get_mut(p) {
                    *b = false;
                }
                crate::heap::KOSTEN_EINTRAG
            } else {
                0
            }
        };
        self.heap.kosten_freigeben(frei);
        Ok(GVal::Bool(true))
    }

    pub(crate) fn n_karte_leeren(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let r = self.karte_als_ref(this, "clear")?;
        let frei = {
            let o = self.heap.holen_mut(r);
            if let HObj::Karte { index, belegt, .. } = o {
                // W35: **leeren, nicht verkuerzen** — die Liste bleibt, weil
                // ein haengender Iterator sie noch liest. Grund und Messung
                // bei `n_menge_leeren`.
                for b in belegt.iter_mut() {
                    *b = false;
                }
                index.clear(); // J8
                0
            } else {
                0
            }
        };
        self.heap.kosten_freigeben(frei);
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn n_karte_groesse(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let r = self.karte_als_ref(this, "size")?;
        // W35: die **belegten** Records zaehlen (Begruendung bei `n_menge_groesse`).
        if let HObj::Karte { belegt, .. } = self.heap.holen(r) {
            return Ok(GVal::Zahl(belegt.iter().filter(|b| **b).count() as f64));
        }
        Ok(GVal::Zahl(0.0))
    }

    // =================================================================
    // W48 (QUIC, 04.10.2026) — der **lebendige** Karten-/Mengen-Iterator
    // =================================================================
    //
    // **DIE EINE URSACHE VON DREI ROTEN FAELLEN, und sie ist eine Zeile:**
    // `n_karte_iter` hat **den ganzen Inhalt vorher in ein Feld materialisiert**:
    // ```rust
    // let eintraege = … eintraege.into_iter().zip(belegt).filter(…) .collect::<Vec<_>>();
    // ```
    // Ein Iterator ist aber per Norm **ein Fenster auf die Karte**, kein
    // **Abzug** — und ein Abzug sieht nichts, was danach passiert.
    //
    // **GEMESSEN (`node 22` als Soll, drei Suite-Faelle als Beleg):**
    // ```text
    // Map/prototype/delete/does-not-break-iterators.js        node: "c"   bei uns: Ende
    // Set/prototype/values/values-iteration-mutable.js         node: neu   bei uns: Ende
    // Map/prototype/clear/map-data-list-is-preserved.js        node: leer  bei uns: alt
    // ```
    // **ES2023 24.5.3 `Map.prototype.clear` Schritt 7 / 24.1.3 `[[MapNext]]`:**
    // der Cursor ist ein **Index in die interne Liste**, und die Liste wird
    // **an Ort und Stelle** geleert — nicht durch ein neues Objekt. **Ein
    // Snapshot braucht diese Regel nicht, weil er von ihr nichts bemerkt.**
    //
    // **DER WEG IST DERSELBE WIE BEI `matchAll` (W47)**, und das ist der Punkt:
    // der Iterator traegt **seinen Zustand als nicht aufzaehlbare eigene
    // Properties** und liest die Karte **bei jedem Schritt neu**. *Eine Familie
    // von Zufallsfehlern, die man zweimal loest, ist keine Family von
    // Zufallsfehlern.*
    /// Release 15.10.2026: private state key of Map/Set Iterators (the
    /// kind and position use `builtin::ITER_ART`/`ITER_POS`).
    pub(crate) fn karte_zum_iterator(
        &mut self,
        karte: HeapRef,
        art: u8,
        menge: bool,
    ) -> Result<GVal, Stoerung> {
        // **DIE FORM IST `feld_zum_iterator` nachgebildet** (frischer Prototyp
        // mit `next`, Iterator mit diesem Prototyp) — damit gelten dieselben
        // Beobachtungen: `it.next.name === "next"`,
        // `Object.getPrototypeOf(it) !== Map.prototype`.
        // Release 09.10.2026: unter `%IteratorPrototype%`, nicht unter
        // `Array.prototype` (siehe `iterator_basis`).
        // Release 15.10.2026: shared %MapIteratorPrototype% /
        // %SetIteratorPrototype% (see `es_iter_proto`); `@@iterator` comes
        // from %Iterator.prototype%. MEASURED before: a fresh prototype, `next`
        // and `[Symbol.iterator]` function per iterator.
        let proto = self.es_iter_proto(if menge { 2 } else { 1 })?;
        let it = self.neues_objekt(Some(proto)).map_err(Stoerung::Fehler)?;
        // **Der Zustand: nicht aufzaehlbar** — `Object.keys(it)` bleibt `[]`.
        // Release 15.10.2026: private names, so reflection does not list
        // them either. MEASURED before (node 22 in brackets):
        // `Object.getOwnPropertyNames(new Map().entries())` gave
        // ["__art","__karte","__menge","__pos"] ([]).
        self.zustand_setzen(it, KARTE_ITER_KARTE, GVal::Ref(karte));
        self.zustand_setzen(it, crate::builtin::ITER_ART, GVal::Zahl(art as f64));
        self.zustand_setzen(it, crate::builtin::ITER_POS, GVal::Zahl(0.0));
        self.heap.kosten_verbuchen(3 * crate::heap::KOSTEN_EINTRAG);
        let _ = menge;
        Ok(GVal::Ref(it))
    }

    /// **Ein** Schritt des lebendigen Iterators. Die Karte wird **hier** gelesen,
    /// nicht beim Anlegen — deshalb sieht der Iterator spaetere `set`/`delete`/
    /// `clear` **und** das Ende einer Schleife, die waehrend des Laufes eine
    /// Gruppe erhaelt.
    pub(crate) fn n_karte_iter_next(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let it = match this {
            GVal::Ref(r) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "next an Nicht-Iterator"),
                ));
            }
        };
        let karte = match self.eigene_lese(it, KARTE_ITER_KARTE) {
            Some(GVal::Ref(r)) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "next an Nicht-Iterator"),
                ));
            }
        };
        let art = match self.eigene_lese(it, crate::builtin::ITER_ART) {
            Some(v) => self.zu_zahl(v).unwrap_or(0.0) as u8,
            None => 0,
        };
        // **`eigende_lese_zahl2` ist der vorhandene Helfer** (`builtin2.rs`) —
        // der Motor hat **einen** `impl Engine` ueber alle Dateien, also waere
        // ein zweiter Helfer mit demselben Namen ein **Doppelregister**, kein
        // zweiter Weg. *Eine Liste, die an zwei Stellen steht, ist an der zweiten
        // Stelle eine andere Liste.*
        let mut pos = self.eigende_lese_zahl2(it, crate::builtin::ITER_POS);
        // **Schritt fuer Schritt lesen — und W35: leere Records ueberspringen.**
        // Der Cursor rueckt auch ueber leere Records hinweg, sonst waere ein
        // zweites `delete` waehrend der Iteration nicht sichtbar.
        loop {
            let (hat, belegt) = match self.heap.holen(karte) {
                HObj::Karte {
                    eintraege, belegt, ..
                } => (eintraege.len(), belegt.clone()),
                HObj::Menge { eintraege, belegt, .. } => (eintraege.len(), belegt.clone()),
                _ => (0, Vec::new()),
            };
            let i = pos as usize;
            if i >= hat {
                return self.iter_ergebnis(it, GVal::Undefiniert, true);
            }
            pos += 1.0;
            let frei = belegt.get(i).copied().unwrap_or(true);
            if !frei {
                continue;
            }
            // Der Wert **jetzt** holen — nach dem Cursor-Schritt, damit er zur
            // Position passt.
            let wert = match self.heap.holen(karte) {
                HObj::Karte { eintraege, .. } => match art {
                    0 => eintraege[i].0,
                    1 => eintraege[i].1,
                    _ => {
                        let paar = self.feld_aus_werten(&[eintraege[i].0, eintraege[i].1])?;
                        GVal::Ref(paar)
                    }
                },
                // Release 15.10.2026: Set entries are `[value, value]`.
                HObj::Menge { eintraege, .. } => {
                    let v = eintraege[i];
                    if art == 2 {
                        self.wurzeln.push(v);
                        GVal::Ref(self.feld_aus_werten(&[v, v])?)
                    } else {
                        v
                    }
                }
                _ => GVal::Undefiniert,
            };
            self.wurzeln.push(wert);
            self.eigente_pos_setzen(it, pos);
            return self.iter_ergebnis(it, wert, false);
        }
    }

    fn eigente_pos_setzen(&mut self, r: HeapRef, pos: f64) {
        self.zustand_setzen(r, crate::builtin::ITER_POS, GVal::Zahl(pos));
    }

    /// `{ value, done }` — 7.3.30 IteratorResult, als frisches Objekt.
    fn iter_ergebnis(&mut self, r: HeapRef, wert: GVal, fertig: bool) -> Result<GVal, Stoerung> {
        self.wurzeln.push(wert);
        let o = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
        self.daten_setzen(o, "value", wert);
        self.daten_setzen(o, "done", GVal::Bool(fertig));
        let _ = r;
        Ok(GVal::Ref(o))
    }

    pub(crate) fn n_karte_iter(&mut self, this: GVal, art: u8) -> Result<GVal, Stoerung> {
        let r = self.karte_als_ref(this, "keys")?;
        // **W48: der lebendige Iterator** — frueher stand hier der Abzug in ein
        // Feld (`into_iter().zip(belegt).filter().collect()`), und **ein Abzug
        // sieht nichts, was danach passiert**. Begruendung und die drei
        // gemessenen Faelle stehen bei `karte_zum_iterator`.
        self.karte_zum_iterator(r, art, false)
    }

    pub(crate) fn n_karte_fuer_jedes(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let r = self.karte_als_ref(this, "forEach")?;
        let f = self.arg(args, 0);
        if !self.ist_aufrufbar(f) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Rueckruf keine Funktion"),
            ));
        }
        let this_arg = self.arg(args, 1);
        // ## W36 (JS2/QUIC, Runde 96) — siehe `n_menge_fuer_jedes` dort. Die
        // gleiche Normzeile, die gleiche lebende Laenge, derselbe Grund.
        // **GEMESSEN (`node 22`):**
        // ```js
        // var m=new Map([[1,"a"]]); var h=[];
        // m.forEach(function(v,k){ h.push(k); m.set(9,"z") });
        // node: 1,9        bei uns (vorher): 1
        // ```
        let mut pos = 0usize;
        loop {
            self.schritt().map_err(Stoerung::Fehler)?;
            let (k, v) = match self.heap.holen(r) {
                // W35: leere Records ueberspringen, Position ruecken —
                // dieselbe Regel wie bei `n_menge_fuer_jedes`, dort steht
                // die Normzeile (ECMA-262 24.5.5.1 Schritt 6).
                HObj::Karte {
                    eintraege, belegt, ..
                } => match eintraege.get(pos) {
                    Some(p) if belegt.get(pos).copied().unwrap_or(true) => *p,
                    // `pos += 1` vor dem `continue` — siehe die
                    // `n_menge_fuer_jedes`-Stelle: ohne sie Endlosschleife.
                    Some(_) => {
                        pos += 1;
                        continue;
                    }
                    None => break,
                },
                // `karte_als_ref` hat oben schon geworfen.
                _ => break,
            };
            pos += 1;
            let argv = [v, k, GVal::Ref(r)];
            let marke = self.wurzel_merke();
            for a in &argv {
                self.wurzeln.push(*a);
            }
            let rr = self.aufruf_synchron(f, this_arg, &argv, false);
            self.wurzel_zurueck(marke);
            rr?;
        }
        Ok(GVal::Undefiniert)
    }

    // ---------------------------------------------------------------
    // Set
    // ---------------------------------------------------------------

    pub(crate) fn menge_als_ref(&mut self, this: GVal, name: &str) -> Result<HeapRef, Stoerung> {
        match this {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Menge { .. } => Ok(r),
                _ => Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", &format!("{name} braucht Set")),
                )),
            },
            _ => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", &format!("{name} braucht Set")),
            )),
        }
    }

    /// ECMA-262 24.5.1.1 `Set ( [ iterable ] )`, **Schritt 1** — wortgleich
    /// mit `Map` (24.1.1.1). Siehe dort fuer die **gemessene** Tabelle und
    /// fuer den Grund, warum `Array()`, `Object()`, `Date()`, `RegExp()`,
    /// `Symbol()` und `Error()` **ohne** Wurf bleiben muessen.
    ///
    /// Auch hier war die Flagge vorhanden und **verworfen** (`_neu`).
    pub(crate) fn n_menge_ctor(&mut self, args: &[GVal], neu: bool) -> Result<GVal, Stoerung> {
        if !neu {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Set: ohne new"),
            ));
        }
        let obj = HObj::Menge {
            proto: Some(self.proto_menge),
            index: std::collections::HashMap::new(),
            eintraege: Vec::new(),
            belegt: Vec::new(), // W35: leere Record (heap.rs)
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        let r = self.belege(obj, kosten);
        if !matches!(self.arg(args, 0), GVal::Null | GVal::Undefiniert) {
            let iter = self.iter_werte(self.arg(args, 0))?;
            // ## DER KONSTRUKTOR RUFT `Set.prototype.add` — ECMA-262 24.5.2.1
            // Schritt 10-11. **GEMESSEN:** `Set/set-iterable-calls-add.js`
            // meldet „called 0 times", `Set/set-iterable-throws-when-add-is-
            // not-callable.js` erwartet einen `TypeError`. Dieselbe Ursache wie
            // beim `Map` (Runde 26): wir sind an der **Methode** vorbei.
            //
            // **UND HIER WIRD ES KURZER, nicht laenger:** der alte Rumpf hat
            // den Index **selbst** gepflegt, obwohl `n_menge_hinzu` genau das
            // tut. Die Absicht aus Runde 19 war ausdruecklich „nur einen Pfad,
            // der den Index schreibt" — der Konstruktor war die **zweite**.
            let adder = self.hole(GVal::Ref(r), "add")?;
            if !self.ist_aufrufbar(adder) {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Set.prototype.add ist nicht aufrufbar"),
                ));
            }
            for e in iter {
                // M6: Wurzeln — der Aufruf darf allozieren.
                self.wurzeln.push(adder);
                self.wurzeln.push(GVal::Ref(r));
                self.wurzeln.push(e);
                self.aufruf_synchron(adder, GVal::Ref(r), &[e], false)?;
            }
        }
        Ok(GVal::Ref(r))
    }

    pub(crate) fn n_menge_hinzu(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let r = self.menge_als_ref(this, "add")?;
        let v = self.arg(args, 0);
        let marke = self.wurzel_merke();
        self.wurzeln.push(v);
        // J8: Index statt Scan (Begruendung bei `karte_setzen_innen`).
        let h = crate::eingebaut_menge::schluessel_hash_normiert(&self.heap, v);
        let doppelt = match self.heap.holen(r) {
            HObj::Menge {
                eintraege, index, ..
            } => index.get(&h).is_some_and(|liste| {
                liste
                    .iter()
                    .copied()
                    .any(|i| schluessel_gleich(&self.heap, eintraege[i], v))
            }),
            _ => false,
        };
        if !doppelt {
            // JS2 (03.10.2026, 10:45): wie beim `Map` — **vor** `holen_mut`.
            let v = self.kanonischer_schluessel(v);
            self.vor_allok(crate::heap::KOSTEN_EINTRAG)
                .map_err(Stoerung::Fehler)?;
            let o = self.heap.holen_mut(r);
            if let HObj::Menge { eintraege, .. } = o {
                if eintraege.len() > 1_000_000 {
                    self.wurzel_zurueck(marke);
                    return Err(Stoerung::Fehler(JsFehler::Speicher(
                        "Set zu gross".to_string(),
                    )));
                }
                let pos = eintraege.len();
                eintraege.push(v);
                if let HObj::Menge { index, belegt, .. } = self.heap.holen_mut(r) {
                    index.entry(h).or_default().push(pos);
                    belegt.push(true); // W35: ein neuer Record ist belegt
                }
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            }
        }
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(r))
    }

    pub(crate) fn n_menge_haben(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let r = self.menge_als_ref(this, "has")?;
        let v = self.arg(args, 0);
        // J8: Index statt Scan.
        let h = crate::eingebaut_menge::schluessel_hash_normiert(&self.heap, v);
        if let HObj::Menge {
            eintraege, index, ..
        } = self.heap.holen(r)
            && let Some(liste) = index.get(&h)
            && liste
                .iter()
                .any(|i| schluessel_gleich(&self.heap, eintraege[*i], v))
        {
            return Ok(GVal::Bool(true));
        }
        Ok(GVal::Bool(false))
    }

    pub(crate) fn n_menge_loeschen(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let r = self.menge_als_ref(this, "delete")?;
        let v = self.arg(args, 0);
        // J8: Index statt Scan (siehe `karte_setzen_innen`).
        let h = crate::eingebaut_menge::schluessel_hash_normiert(&self.heap, v);
        let pos = match self.heap.holen(r) {
            HObj::Menge {
                eintraege, index, ..
            } => index.get(&h).and_then(|liste| {
                liste
                    .iter()
                    .copied()
                    .find(|i| schluessel_gleich(&self.heap, eintraege[*i], v))
            }),
            _ => None,
        };
        let Some(p) = pos else {
            return Ok(GVal::Bool(false));
        };
        // W35: die Hashes aller uebrigen Eintraege **nicht** mehr berechnen —
        // sie wurden nur fuer den Neuaufbau des Index gebraucht, und den gibt
        // es nicht mehr (nur die eine Position wird aus der Liste genommen).
        let frei = {
            let o = self.heap.holen_mut(r);
            if let HObj::Menge { index, belegt, .. } = o {
                // ## W35 (QUIC, 03.10.2026) — **MARKIEREN STATT ENTFERNEN.**
                //
                // ECMA-262 24.5.4.3 Schritt 5: `Set p.[[Key]] to empty`, und
                // `Set p.[[Value]] to empty` — **der Record bleibt**. Vorher
                // stand hier `eintraege.remove(p)` mit einem **vollstaendigen
                // Neuaufbau des Index** (`index.clear()` + Schleife). Das war
                // an zwei Dingen falsch:
                //
                // * **Die Reihenfolge.** `values-iteration-mutable.js` will, dass
                //   ein **zufaetzig wieder eingefuegter** Wert **am Ende**
                //   wiederkommt; mit `remove` rutscht er an die Stelle des
                //   geloeschten.
                // * **Die Kosten.** Das Neuaufbauen war **O(n)** fuer ein
                //   `delete` — und `delete` ist der haeufigste Aufruf.
                //
                // **Jetzt:** nur die Position aus der Liste des Hash nehmen.
                // Der Eintrag bleibt stehen (der GC sieht also weiter seinen
                // Schluessel — das ist der Grund fuer die Fahne statt eines
                // Sentinels, siehe `heap.rs`), und `belegt` sagt allen, die
                // **zaehlen oder aufzaehlen**, dass er nicht mitzaehlt.
                //
                // **GEMESSEN, `node 22` als Soll:**
                // ```js
                // var s = new Set([1,2]); s.delete(1); s.add(1);
                // [...s]            node: [2, 1]      bei uns (vorher): [1]
                // ```
                if let Some(liste) = index.get_mut(&h) {
                    liste.retain(|i| *i != p);
                }
                if let Some(b) = belegt.get_mut(p) {
                    *b = false;
                }
                crate::heap::KOSTEN_EINTRAG / 2
            } else {
                0
            }
        };
        self.heap.kosten_freigeben(frei);
        Ok(GVal::Bool(true))
    }

    pub(crate) fn n_menge_leeren(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let r = self.menge_als_ref(this, "clear")?;
        let frei = {
            let o = self.heap.holen_mut(r);
            if let HObj::Menge { index, belegt, .. } = o {
                // ## W35 (QUIC, 03.10.2026) — **LEEREN, NICHT VERKUERZEN.**
                //
                // ECMA-262 24.5.4.2 Schritt 5.b: `Repeat for each Record p …
                // Set p.[[Key]] to empty` — die **Liste bleibt**. Der Fall
                // `Map/prototype/clear/map-data-list-is-preserved.js` sagt es
                // woertlich und nennt den Grund: *„there may be existing
                // MapIterator objects that are suspended midway through
                // iterating over that List."*
                //
                // Vorher stand hier `eintraege.clear()` — damit bekam ein
                // **haengender** Iterator `done: false` und lieferte danach
                // **leere Eintraege** weiter, statt `done: true`.
                for b in belegt.iter_mut() {
                    *b = false;
                }
                index.clear(); // J8
                // Der Speicher bleibt belegt: `Heap::kosten_fuer` rechnet nach
                // `eintraege.len()`, und ein freigegebener Record, den ein
                // haengender Iterator noch liest, waere ein **Use-after-free**.
                0
            } else {
                0
            }
        };
        self.heap.kosten_freigeben(frei);
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn n_menge_groesse(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let r = self.menge_als_ref(this, "size")?;
        // W35: `size` zaehlt die **belegten** Records, nicht die Zeilen der
        // Liste. `node 22`: `var s = new Set([1,2]); s.delete(1); s.size` -> 1.
        if let HObj::Menge { belegt, .. } = self.heap.holen(r) {
            return Ok(GVal::Zahl(belegt.iter().filter(|b| **b).count() as f64));
        }
        Ok(GVal::Zahl(0.0))
    }

    pub(crate) fn n_menge_iter(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let r = self.menge_als_ref(this, "values")?;
        // **W48: derselbe lebendige Weg wie bei `Map`** — und das ist der Grund,
        // warum es **einen** Helfer gibt und nicht zwei. `Set.prototype.values`
        // ist nach ECMA-262 24.2.5.2 `CreateSetIterator(S, "value")`, und der
        // Cursor ist ein Index in die interne Liste.
        //
        // **GEMESSEN, und das ist der Fall, der die Regel gestellt hat:**
        // `Set/prototype/values/values-iteration-mutable.js` — ein `add`
        // **waehrend** der Laufzeit muss den neuen Wert **liefern**.
        self.karte_zum_iterator(r, 1, true)
    }

    pub(crate) fn n_menge_eintraege(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let r = self.menge_als_ref(this, "entries")?;
        // Release 15.10.2026: the live Set Iterator (kind 2, `[v, v]`), not a
        // copy: CreateSetIterator(S, key+value) (24.2.5.5). MEASURED before
        // (node 22 in brackets): `var s = new Set([1]); var it = s.entries();
        // s.add(2); [...it].length` was 1 (2), and the iterator was an Array
        // Iterator (`[object Array Iterator]`, "[object Set Iterator]").
        self.karte_zum_iterator(r, 2, true)
    }

    pub(crate) fn n_menge_fuer_jedes(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let r = self.menge_als_ref(this, "forEach")?;
        let f = self.arg(args, 0);
        if !self.ist_aufrufbar(f) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Rueckruf keine Funktion"),
            ));
        }
        let this_arg = self.arg(args, 1);
        // ## W36 (JS2/QUIC, Runde 96) — `forEach` liest die **lebende** Liste.
        //
        // **GEMESSEN (`node 22`), der Fall ist
        // `Set/prototype/forEach/iterates-values-added-after-foreach-begins.js`:**
        // ```js
        // var s=new Set([1,2,3]); var g=[];
        // s.forEach(function(v){ g.push(v); if(v===3) s.add(4); if(v===4) s.add(5) });
        // node: 1,2,3,4,5      bei uns (vorher): 1,2,3
        // ```
        // **Kaskadierend** — nicht „ein Element mehr". Wer nur `+1` lesen
        // koennte, haette `1,2,3,4` geliefert. Also wird die Laenge **jeden**
        // Schritt neu gelesen.
        //
        // **DIE NORM-ZEILE, die es entscheidet** (im Fall selbst zitiert):
        // > „7. Repeat for each e that is an element of entries, in original
        // > insertion order"
        //
        // Es steht **kein** `numEntries` darin. Ich hatte im Kopf die
        // aeltere Fassung mit `Let numEntries be the number of elements` und
        // `while index < numEntries` — **die haette den Fall nicht erklaert**,
        // sondern nur verschoben. *Zwei Fassungen derselben Norm sind nicht
        // dasselbe Gesetz; welche gilt, steht im Fall, nicht im Gedaechtnis.*
        //
        // **WARUM DER INDEX VOR dem Aufruf hochzaehlt:** Schritt b steht vor
        // Schritt c. Fuer die Faelle, die diese Runde behebt, ist der
        // Unterschied nicht beobachtbar — er waere es nur, wenn der Rueckruf
        // einen Eintrag **vor** dem Zeiger loeschte, und da weicht unser
        // `Vec` ohne Grabsteine **ohnehin** ab (das ist W11 und gehoert nach
        // `heap.rs`). *Ein Zeiger, der schon vorher steht, kostet nichts und
        // macht die Abweichung an einer Stelle sichtbar statt an zwei.*
        let mut pos = 0usize;
        loop {
            self.schritt().map_err(Stoerung::Fehler)?;
            // **EIN** Schritt: Element **und** Laende werden frisch gelesen,
            // und die Borrow endet **vor** dem Aufruf — der Aufruf darf die
            // Liste veraendern, also darf hier nichts geborgen bleiben.
            let e = match self.heap.holen(r) {
                // W35: ein **leerer** Record (von `delete` oder `clear`) wird
                // **uebersprungen**, nicht geliefert — ECMA-262 24.5.5.1
                // Schritt 6: `If e.[[Key]] is not empty, then …`. Ohne das
                // wuerde mein Bau geloeschte Werte **zurueckgeben**, weil er
                // sie absichtlich stehen laesst.
                HObj::Menge {
                    eintraege, belegt, ..
                } => match eintraege.get(pos) {
                    Some(e) if belegt.get(pos).copied().unwrap_or(true) => *e,
                    // Leerer Record: **Position ruecken, nicht liefern.**
                    //
                    // GEMESSEN, warum `pos += 1` HIER stehen muss und nicht
                    // weiter unten: ein `continue` ohne diese Zeile laeuft in
                    // eine **Endlosschleife** — `pos` bliebe stehen, und die
                    // Schleife nähme denselben leeren Record immer wieder.
                    // Das war kein roter Fall, sondern `Zeitlimit:
                    // Schrittgrenze 5000000 ueberschritten` in **vier**
                    // `forEach`-Faellen. *Ein Messgerät, das erst nach dem
                    // Bauen sagt, was daran falsch war.*
                    Some(_) => {
                        pos += 1;
                        continue;
                    }
                    None => break,
                },
                // `menge_als_ref` hat oben schon geworfen; hier ist der Arm
                // unerreichbar, und ein `break` ist besser als ein `panic`.
                _ => break,
            };
            pos += 1;
            let argv = [e, e, GVal::Ref(r)];
            let marke = self.wurzel_merke();
            for a in &argv {
                self.wurzeln.push(*a);
            }
            let rr = self.aufruf_synchron(f, this_arg, &argv, false);
            self.wurzel_zurueck(marke);
            rr?;
        }
        Ok(GVal::Undefiniert)
    }

    // ---------------------------------------------------------------
    // Date
    // ---------------------------------------------------------------

    pub(crate) fn datum_als_ref(&mut self, this: GVal, name: &str) -> Result<f64, Stoerung> {
        match this {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Datum { millis, .. } => Ok(*millis),
                _ => Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", &format!("{name} braucht Date")),
                )),
            },
            _ => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", &format!("{name} braucht Date")),
            )),
        }
    }

    pub(crate) fn datum_anlegen(&mut self, millis: f64) -> Result<GVal, Stoerung> {
        let obj = HObj::Datum {
            proto: Some(self.proto_datum),
            millis,
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        Ok(GVal::Ref(self.belege(obj, kosten)))
    }

    pub(crate) fn n_datum_ctor(
        &mut self,
        _this: GVal,
        args: &[GVal],
        _neu: bool,
    ) -> Result<GVal, Stoerung> {
        // Date() als Funktion: String. new Date(): Objekt.
        let millis = if args.is_empty() {
            jetzt_millis()
        } else if args.len() == 1 {
            match self.arg(args, 0) {
                GVal::Ref(r) => match self.heap.holen(r).clone() {
                    HObj::Zeichenkette(s) => datum_parsen(&s),
                    HObj::Datum { millis, .. } => millis,
                    _ => self.zu_zahl(GVal::Ref(r))?,
                },
                v => self.zu_zahl(v)?,
            }
        } else {
            // Teile (Jahr, Monat, ...): LOKAL (Spec 20.4.3.1). Die Teile
            // sind Wandzeit und werden mit zwei Durchgaengen nach UTC
            // gelegt (wie im Setter — Sommerzeit-Wechsel sonst ±1h).
            // J-CACHE (04.10.2026): vorher UTC-Naeherung — new Date(2016,
            // 6, 6, 0, 0, 0, -1) gab 5. Juli 23:59 UTC statt 6. Juli
            // 01:59 Berlin (getDate 6 statt 5).
            let n = |e: &mut Engine, i: usize, dflt: f64| -> Result<f64, Stoerung> {
                if i < args.len() {
                    e.zu_zahl(args[i])
                } else {
                    Ok(dflt)
                }
            };
            let jahr = n(self, 0, 1970.0)?;
            let monat = n(self, 1, 0.0)?;
            let tag = n(self, 2, 1.0)?;
            let std = n(self, 3, 0.0)?;
            let min = n(self, 4, 0.0)?;
            let sek = n(self, 5, 0.0)?;
            let ms = n(self, 6, 0.0)?;
            let wand = datum_aus_teilen(jahr, monat, tag, std, min, sek, ms, true);
            if !wand.is_finite() {
                wand
            } else {
                let m1 = wand - crate::zone::verschiebung_s(wand) as f64 * 1000.0;
                wand - crate::zone::verschiebung_s(m1) as f64 * 1000.0
            }
        };
        let millis = zeit_klammern(millis);
        // J-CACHE (04.10.2026): **Date() als Funktion -> String.**
        // ES2023 20.4.2.1: Aufruf (nicht Konstruktor) liefert String der
        // aktuellen Zeit. `_neu` wurde bisher ignoriert (S15.9.2.1_A1/A2).
        if !_neu {
            let s = datum_text(jetzt_millis());
            return self.text_wert(&s).map_err(Stoerung::Fehler);
        }
        self.datum_anlegen(millis)
    }

    /// `setUTCFullYear` & Co. — GEMESSEN (A5): fehlten vorher ganz.
    ///
    /// `teil`: 0 Jahr, 1 Monat, 2 Tag, 4 Stunden, 5 Minuten, 6 Sekunden,
    /// 7 Millisekunden (Nummern wie bei `n_datum_teil`). Ueberlaeufe
    /// tragen wie in JS (`setUTCMonth(13)` -> Januar des Folgejahres), ein
    /// nicht endlicher Wert macht das Datum unbrauchbar (NaN-Zeit).
    pub(crate) fn n_datum_setzen(
        &mut self,
        this: GVal,
        args: &[GVal],
        teil: u8,
        lokal: bool,
    ) -> Result<GVal, Stoerung> {
        // J24: bei `lokal` rechnet der Setzer in der **lokalen** Zone des
        // Hosts (die beiden Durchgaenge sind die Standardloesung fuer die
        // Umstellung: erste Runde mit der alten, zweite mit der neuen
        // Verschiebung — sonst waere der Sommerzeitwechsel um eine Stunde
        // daneben).
        let m0 = self.datum_als_ref(this, if lokal { "set" } else { "setUTC" })?;
        // Argument holen; fehlt es, bleibt der alte Teil.
        let hol = |e: &mut Engine, i: usize| -> Result<f64, Stoerung> {
            if i < args.len() {
                e.zu_zahl(args[i])
            } else {
                Ok(f64::NAN)
            }
        };
        // J-CACHE (04.10.2026, Freigabe Leitung): **ToNumber VOR dem
        // NaN-Check.** Alle set*-Setter lesen erst `t`, koerzieren dann
        // die Argumente (Seiteneffekt laeuft!), und erst dann gilt die
        // NaN-Regel — gemessen an 20 Dateien (`date-value-read-...`,
        // `arg-coercion-order`: valueOf-Zaehler = 1 trotz NaN-Datum).
        // NaN-Regel je Familie (aus den Spec-Kaesten der Tests):
        // nur Jahr (teil 0, setFullYear/setUTCFullYear) rechnet mit
        // t = +0 weiter; alle anderen returnieren NaN.
        let v = hol(self, 0)?;
        // J-CACHE (04.10.2026): **Folge-Argumente SOFORT koerzieren
        // (nur lesen, noch nicht setzen).** Die Spec koerziert vorhandene
        // Folge-Args VOR dem NaN-Check (arg-coercion-order: alle valueOf
        // laufen, dann return NaN). Wer sie erst hinten laese, verlöre
        // die Seiteneffekte bei NaN-Datum. Gespeichert wird NaN als
        // None (wie bisher: NaN-Teile bleiben unveraendert).
        // Folge-Teile tragen dieselben Nummern wie `teil`; teil 0
        // (Jahr) braucht [1, 2] (setFullYear), Rest wie gehabt.
        let paare: &[u8] = match teil {
            0 => &[1, 2],
            1 => &[2],
            4 => &[5, 6, 7],
            5 => &[6, 7],
            6 => &[7],
            _ => &[],
        };
        let mut folge_werte: Vec<Option<f64>> = Vec::new();
        for (k, _) in paare.iter().enumerate() {
            if k + 1 < args.len() {
                // Vorhanden (auch undefined/NaN!) — Seiteneffekt laeuft;
                // NaN merkt sich der NaN-Check unten (Spec: vorhandenes
                // NaN-Arg -> Gesamtergebnis NaN, "undefined"-Test in
                // arg-date-to-number.js). Fehlend -> None (Default aus t).
                let x = self.zu_zahl(args[k + 1])?;
                folge_werte.push(Some(x));
            } else {
                folge_werte.push(None);
            }
        }
        let alt = if lokal {
            m0 + crate::zone::verschiebung_s(m0) as f64 * 1000.0
        } else {
            m0
        };
        let alt = if !alt.is_finite() {
            if teil == 0 {
                0.0
            } else {
                return Ok(GVal::Zahl(f64::NAN));
            }
        } else {
            alt
        };
        let (j, mo, t, std, min, sek, ms, _) = datum_teile(alt);
        if v.is_nan() {
            // J-CACHE (04.10.2026): **fehlendes oder NaN-Arg 0 -> NaN.**
            // Spec: ToNumber(undefined) = NaN -> MakeDate/Time -> NaN.
            // (Nur FEHLENDE Folge-Args behalten alte Teile.) GEMESSEN an
            // setHours() -> NaN (arg-min-to-number.js u.a.).
            return Ok(GVal::Zahl(f64::NAN));
        }
        // `teil` (Nummer wie bei `n_datum_teil`) -> Index im Teile-Feld.
        fn idx(teil: u8) -> usize {
            match teil {
                0 => 0,
                1 => 1,
                2 => 2,
                4 => 3,
                5 => 4,
                6 => 5,
                _ => 6,
            }
        }
        // GEMESSEN: `datum_teile` liefert den Monat 1-basiert (wie im
        // ISO-Text), `datum_aus_teilen` erwartet ihn 0-basiert — ohne das
        // Minus war `setUTCMonth(5)` der Juli statt des Juni.
        let mut teile: [i64; 7] = [
            j,
            mo as i64 - 1,
            t as i64,
            std as i64,
            min as i64,
            sek as i64,
            ms as i64,
        ];
        // v ist hier garantiert nicht NaN (oben abgefangen).
        teile[idx(teil)] = v.trunc() as i64;
        // Folge-Argumente setzen — aus den VOR dem NaN-Check koerzierten
        // Werten (kein zweiter Seiteneffekt).
        for (k, pteil) in paare.iter().enumerate() {
            // Vorhandenes NaN-Arg -> Gesamtergebnis NaN (Spec MakeDay/
            // TimeClip); fehlend (None) -> Default aus t behalten.
            if let Some(&Some(x)) = folge_werte.get(k) {
                if x.is_nan() {
                    return Ok(GVal::Zahl(f64::NAN));
                }
                teile[idx(*pteil)] = x.trunc() as i64;
            }
        }
        let neu = datum_aus_teilen(
            teile[0] as f64,
            teile[1] as f64,
            teile[2] as f64,
            teile[3] as f64,
            teile[4] as f64,
            teile[5] as f64,
            teile[6] as f64,
            false,
        );
        let neu = if lokal {
            // `neu` ist hier „lokal wie UTC" — zurueck in echte Zeit, mit
            // zwei Durchgaengen (erst die alte, dann die neue Verschiebung).
            let m1 = neu - crate::zone::verschiebung_s(m0) as f64 * 1000.0;
            neu - crate::zone::verschiebung_s(m1) as f64 * 1000.0
        } else {
            neu
        };
        // J-CACHE (04.10.2026): **TimeClip vor Setzen + Rueckgabe.**
        // Spec 20.4.4.x Schritt 6-7: u = TimeClip(...), setzen, return u.
        // Ohne Klammer blieb ein uebergrosses Datum endlich (statt NaN) —
        // setMinutes/setSeconds-new-value-time-clip.js.
        let neu = zeit_klammern(neu);
        let r = match this {
            GVal::Ref(r) => r,
            _ => return Ok(GVal::Zahl(neu)),
        };
        let o = self.heap.holen_mut(r);
        if let HObj::Datum { millis, .. } = o {
            *millis = neu;
        }
        Ok(GVal::Zahl(neu))
    }

    /// Ein Datumsteil in der **lokalen** Zone lesen (`get*`).
    pub(crate) fn n_datum_teil_lokal(&mut self, this: GVal, teil: u8) -> Result<GVal, Stoerung> {
        let m = self.datum_als_ref(this, "get")?;
        if !m.is_finite() {
            return Ok(GVal::Zahl(f64::NAN));
        }
        // Die Verschiebung in ms addieren: der Rest rechnet wie `getUTC*`.
        Ok(GVal::Zahl(
            datum_teil(m + crate::zone::verschiebung_s(m) as f64 * 1000.0, teil) as f64,
        ))
    }

    /// `toUTCString()`: „Wed, 01 Jan 2020 00:00:00 GMT“.
    pub(crate) fn n_datum_utc_string(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        const TAGE: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const MONATE: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let m = self.datum_als_ref(this, "toUTCString")?;
        if !m.is_finite() {
            return Ok(self.text_wert("Invalid Date").map_err(Stoerung::Fehler)?);
        }
        let (j, mo, t, std, min, sek, _, woche) = datum_teile(m);
        Ok(self
            .text_wert(&format!(
                "{}, {:02} {} {} {std:02}:{min:02}:{sek:02} GMT",
                TAGE[woche as usize],
                t,
                MONATE[(mo - 1) as usize],
                jahr_text(j)
            ))
            .map_err(Stoerung::Fehler)?)
    }

    pub(crate) fn n_datum_lesen(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let s = self.zu_text(self.arg(args, 0))?;
        Ok(GVal::Zahl(datum_parsen(&s)))
    }

    pub(crate) fn n_datum_utc(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        // J-CACHE (04.10.2026): **ohne Args -> NaN.** ES2023 20.4.3.2:
        // y = ToNumber(undefined) = NaN. Vorher griff der 1970-Default
        // (UTC/no-arg.js).
        if args.is_empty() {
            return Ok(GVal::Zahl(f64::NAN));
        }
        let n = |e: &mut Engine, i: usize, dflt: f64| -> Result<f64, Stoerung> {
            if i < args.len() {
                e.zu_zahl(args[i])
            } else {
                Ok(dflt)
            }
        };
        let jahr = n(self, 0, 1970.0)?;
        let monat = n(self, 1, 0.0)?;
        let tag = n(self, 2, 1.0)?;
        let std = n(self, 3, 0.0)?;
        let min = n(self, 4, 0.0)?;
        let sek = n(self, 5, 0.0)?;
        let ms = n(self, 6, 0.0)?;
        Ok(GVal::Zahl(zeit_klammern(datum_aus_teilen(
            jahr, monat, tag, std, min, sek, ms, true,
        ))))
    }

    pub(crate) fn n_datum_zeit(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        Ok(GVal::Zahl(self.datum_als_ref(this, "getTime")?))
    }

    /// J-CACHE (04.10.2026, Weisung): `toLocaleDateString` (art 0),
    /// `toLocaleTimeString` (1), `toLocaleString` (2). Logik in
    /// eingebaut_intl.rs; hier nur this/Args/Optionen. Locale = erstes
    /// Arg als Text (fehlt -> "en-US", deterministisch). Optionen:
    /// weekday -> Volldatum, year|month|day -> mit Optionen, sonst
    /// Grundform. Invalid Date -> "Invalid Date" (Spec).
    pub(crate) fn n_datum_lokal(
        &mut self,
        this: GVal,
        args: &[GVal],
        art: u8,
    ) -> Result<GVal, Stoerung> {
        let m = self.datum_als_ref(this, "toLocaleString")?;
        if !m.is_finite() {
            return self.text_wert("Invalid Date").map_err(Stoerung::Fehler);
        }
        let locale = match self.arg(args, 0) {
            GVal::Undefiniert => "en-US".to_string(),
            v => self
                .schluessel_text(v)
                .unwrap_or_else(|_| "en-US".to_string()),
        };
        let mut opt = |name: &str| -> Option<String> {
            let o = self.arg(args, 1);
            match o {
                GVal::Ref(_) => self.hole(o, name).ok().and_then(|v| match v {
                    GVal::Undefiniert => None,
                    x => self.schluessel_text(x).ok(),
                }),
                _ => None,
            }
        };
        let s = match art {
            0 => {
                if opt("weekday").is_some() {
                    crate::eingebaut_intl::datum_voll(&locale, m, None)
                } else if opt("year").is_some() || opt("month").is_some() || opt("day").is_some() {
                    crate::eingebaut_intl::datum_mit_optionen(
                        &locale,
                        m,
                        &opt("year").unwrap_or_else(|| "numeric".to_string()),
                        &opt("month").unwrap_or_else(|| "numeric".to_string()),
                        &opt("day").unwrap_or_else(|| "numeric".to_string()),
                        None,
                    )
                } else {
                    crate::eingebaut_intl::datum_format(&locale, m, None)
                }
            }
            1 => crate::eingebaut_intl::zeit_format(&locale, m, None),
            _ => format!(
                "{}, {}",
                crate::eingebaut_intl::datum_format(&locale, m, None),
                crate::eingebaut_intl::zeit_format(&locale, m, None)
            ),
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_datum_iso(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let m = self.datum_als_ref(this, "toISOString")?;
        // J-CACHE (04.10.2026): **TimeClip vor dem Test.** ES2023 20.4.4.33:
        // toISOString wirft RangeError bei NaN — und TimeClip macht aus
        // |t| > 8.64e15 NaN. Vorher fiel nur NaN/Infinity durch; ein
        // endliches Riesen-Datum (new Date(1970,0,-99999999,...)) kam
        // durch. GEMESSEN an 15.9.5.43-0-10/-0-9 (TZ-abhaengig).
        let m = zeit_klammern(m);
        if m.is_nan() {
            return Err(Stoerung::Wurf(
                self.fehler_wert("RangeError", "unguetiges Datum"),
            ));
        }
        self.text_wert(&datum_iso(m)).map_err(Stoerung::Fehler)
    }

    /// J-CACHE (04.10.2026, Freigabe Leitung): eigenes `Date.prototype.
    /// toJSON`. ES2023 20.4.4.37: O = ToObject(this), tv = ToPrimitive(O,
    /// number); Zahl+inifinit -> null; sonst Invoke(O, "toISOString")
    /// (mit O als this, ohne Argumente). Der alte Alias auf DatumIso
    /// rief datum_als_ref(this) direkt — bei Nicht-Datum-this
    /// „toISOString braucht Date" (6 toJSON-Dateien).
    pub(crate) fn n_datum_json(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let o = GVal::Ref(self.zu_objekt(this)?);
        let tv = self.zu_primitiv(o, false)?;
        if let GVal::Zahl(n) = tv {
            if !n.is_finite() {
                return Ok(GVal::Null);
            }
        }
        let f = self.hole(o, "toISOString")?;
        self.aufruf_synchron(f, o, &[], false)
    }

    pub(crate) fn n_datum_text(&mut self, this: GVal, teil: u8) -> Result<GVal, Stoerung> {
        let m = self.datum_als_ref(this, "toString")?;
        let s = if !m.is_finite() {
            "Invalid Date".to_string()
        } else {
            match teil {
                1 => datum_text_datum(m),
                2 => datum_text_zeit(m),
                _ => datum_text(m),
            }
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_datum_teil(&mut self, this: GVal, teil: u8) -> Result<GVal, Stoerung> {
        let m = self.datum_als_ref(this, "getTeil")?;
        if !m.is_finite() {
            return Ok(GVal::Zahl(f64::NAN));
        }
        Ok(GVal::Zahl(datum_teil(m, teil) as f64))
    }

    pub(crate) fn n_datum_zeit_setzen(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let r = match this {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Datum { .. } => r,
                _ => {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("TypeError", "setTime braucht Date"),
                    ));
                }
            },
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "setTime braucht Date"),
                ));
            }
        };
        let m = zeit_klammern(self.zu_zahl(self.arg(args, 0))?);
        let o = self.heap.holen_mut(r);
        if let HObj::Datum { millis, .. } = o {
            *millis = m;
        }
        Ok(GVal::Zahl(m))
    }

    // ---------------------------------------------------------------
    // Promise
    // ---------------------------------------------------------------

    pub(crate) fn resolver_anlegen(&mut self, ziel: HeapRef, erf: bool) -> HeapRef {
        let (name, nat) = if erf {
            ("resolve", crate::builtin::Nativ::AufloeserErfuellt)
        } else {
            ("reject", crate::builtin::Nativ::AufloeserAbgelehnt)
        };
        // J-CACHE (03.10.2026, 19:4x — nach einem Verlust durch den
        // Sicherungsdienst wiederhergestellt): `length` = 1 und
        // `{ writable: false, enumerable: false, CONFIGURABLE: true }`.
        //
        // ES2023 27.2.3.3.2 woertlich: «The length property of a promise
        // resolve function is 1» (27.2.3.3.1 fuer `reject`); der
        // allgemeine Built-in-Satz im selben Kapitel nennt
        // `[[Configurable]]: true`.
        //
        // GEMESSEN vorher gegen `node 22`:
        //   new Promise(function(a,b){…}); a.length   Motor 0   node 1
        // Korpus-Fall: `test/built-ins/Promise/exec-args.js`.
        let mut eigene = crate::heap::Karte::default();
        eigene.insert(
            "length".to_string(),
            crate::heap::Eigenschaft {
                wert: crate::heap::GVal::Zahl(1.0),
                getter: None,
                setter: None,
                aufzählbar: false,
                beschreibbar: false,
                konfigurierbar: true,
            },
        );
        let obj = HObj::Nativ {
            name: name.to_string(),
            proto: Some(self.proto_funktion),
            ziel: Some(ziel),
            eigenschaften: eigene,
        };
        let kosten = Heap::kosten_fuer(&obj);
        // Aufbau/Promise-Pfad: notfalls trotzdem (winzig, gezaehlt).
        if self.vor_allok(kosten).is_err() {
            let r = self.belege(obj, kosten);
            self.native.insert(r, nat);
            return r;
        }
        let r = self.belege(obj, kosten);
        self.native.insert(r, nat);
        r
    }

    pub(crate) fn n_versprechen_ctor(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let roller = self.arg(args, 0);
        if !self.ist_aufrufbar(roller) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Promise braucht Roller-Funktion"),
            ));
        }
        // Release 09.10.2026: jedes neue Objekt SOFORT verwurzeln — das
        // Anlegen des naechsten kann sammeln. GEMESSEN vorher: `ziel` und
        // `erf` waren ungeschuetzt, waehrend `abl` entstand; youtube.com im
        // asynchronen Betrieb endete in „Heap: toter Verweis" (verschachtelte
        // `new Promise`).
        let marke = self.wurzel_merke();
        let ziel = match self.versprechen_anlegen(crate::heap::VersprechenZustand::Offen) {
            GVal::Ref(r) => r,
            _ => unreachable!(),
        };
        self.wurzeln.push(GVal::Ref(ziel));
        let erf = self.resolver_anlegen(ziel, true);
        self.wurzeln.push(GVal::Ref(erf));
        let abl = self.resolver_anlegen(ziel, false);
        self.wurzeln.push(GVal::Ref(abl));
        let r = self.aufruf_synchron(
            roller,
            GVal::Undefiniert,
            &[GVal::Ref(erf), GVal::Ref(abl)],
            false,
        );
        self.wurzel_zurueck(marke);
        match r {
            Ok(_) => Ok(GVal::Ref(ziel)),
            Err(Stoerung::Wurf(g)) => {
                self.versprechen_settle(ziel, false, g);
                Ok(GVal::Ref(ziel))
            }
            Err(Stoerung::Fehler(e)) => Err(Stoerung::Fehler(e)),
        }
    }

    /// `Promise.resolve(wert)` / `Promise.reject(grund)` (statisch).
    /// Promises werden direkt uebernommen (Assimilation eine Stufe).
    /// J-CACHE (04.10.2026, Weisung Web-API): `Promise.withResolvers()`
    /// (ES2024 27.2.4.7). Gibt { promise, resolve, reject } mit der
    /// Capability von %Promise% zurueck. GEMESSEN node 22: resolve/reject
    /// sind Funktionen, promise ein Promise; resolve(5) erfuellt.
    pub(crate) fn n_versprechen_mit_aufloesern(&mut self) -> Result<GVal, Stoerung> {
        let c = self.versprechen_globals_ctor()?;
        let (ziel, erf, abl) = self.versprechen_capability(c)?;
        let o = self
            .neues_objekt(Some(self.proto_objekt))
            .map_err(Stoerung::Fehler)?;
        self.setze(GVal::Ref(o), "promise", GVal::Ref(ziel))?;
        self.setze(GVal::Ref(o), "resolve", erf)?;
        self.setze(GVal::Ref(o), "reject", abl)?;
        Ok(GVal::Ref(o))
    }

    pub(crate) fn n_versprechen_statisch(
        &mut self,
        this: GVal,
        args: &[GVal],
        erf: bool,
    ) -> Result<GVal, Stoerung> {
        let v = self.arg(args, 0);
        // ES2023 27.2.4.1 (`Promise.resolve`) / 27.2.4.2 (`Promise.reject`)
        // Schritt 1-3: `this` ist C, sonst `%Promise%`.
        //
        // **J-CACHE 03.10.2026, 22:1x — der Fehler, den ich ZWEIMAL gemacht
        // habe, und beide Male gemessen: 16/54 -> 15/54.**
        // Beim ersten Versuch gab ich das Ergebnis von
        // `Call(capability.[[Resolve]], …)` zurueck; das ist `undefined`
        // (Schritt 3 sagt woertlich „Return promiseCapability.[[Promise]]").
        // Beim zweiten Versuch benutzte ich die Capability fuer **jedes**
        // `this` — und damit verlor ich die **Assimilation**:
        // `versprechen_erfuellt` nimmt ein Promise direkt ueber
        // („Promises werden direkt uebernommen", Kommentar dort), und
        // `Promise.resolve(p) === p` ist in `node 22` **wahr**. Ohne die
        // Kurzschluss-Regel bekommt jeder Aufruf ein neues Promise.
        //
        // **DIE REGEL, DIE BEIDES ZUSAMMENHAELT:** die Capability gilt nur,
        // wenn `this` ein **Konstruktor** ist. Sonst der bisherige Weg.
        // Das ist genau die Trennung der Spec (Schritt 2 gegen Schritt 3a)
        // und laesst die Assimilation unangetastet.
        if !crate::eingebaut_array::ist_konstruktor(self, this) {
            return if erf {
                Ok(self.versprechen_erfuellt(v))
            } else {
                Ok(self.versprechen_abgelehnt(v))
            };
        }
        if self.ist_kein_objekt(this) {
            let e = self.fehler_wert("TypeError", "Promise auf Nicht-Objekt aufgerufen");
            return Err(Stoerung::Wurf(e));
        }
        // Release 10.10.2026: PromiseResolve (ES2023 27.2.4.7.1) — ein
        // Promise, dessen `constructor` der Empfaenger ist, kommt UNVERAENDERT
        // zurueck. GEMESSEN vorher: `Promise.resolve(p) === p` war `false`;
        // die Kopie uebernahm eine Ablehnung, ohne `p` zu behandeln, und
        // `await` in einem asynchronen Generator meldete „Uncaught (in
        // promise)", obwohl das `catch` sie fing.
        if erf
            && let GVal::Ref(r) = v
            && matches!(self.heap.holen(r), HObj::Versprechen { .. })
        {
            let k = self.hole(v, "constructor")?;
            if k == this {
                return Ok(v);
            }
        }
        let (ziel, cap_erf, cap_abl) = self.versprechen_capability(this)?;
        // Schritt 6/8: IsCallable, Meldung woertlich wie `node 22`.
        if !self.ist_aufrufbar(cap_erf) || !self.ist_aufrufbar(cap_abl) {
            let e = self.fehler_wert(
                "TypeError",
                "Promise resolve or reject function is not callable",
            );
            return Err(Stoerung::Wurf(e));
        }
        // Schritt 2: `Call(capability.[[Resolve]] bzw. [[Reject]], …)` —
        // zurueck kommt Schritt 3 das PROMISE, nicht der Aufrufwert.
        let f = if erf { cap_erf } else { cap_abl };
        self.aufruf_synchron(f, GVal::Undefiniert, &[v], false)?;
        Ok(GVal::Ref(ziel))
    }

    pub(crate) fn n_versprechen_resolver(
        &mut self,
        fn_ref: HeapRef,
        args: &[GVal],
        erf: bool,
    ) -> Result<GVal, Stoerung> {
        let ziel = match self.heap.holen(fn_ref) {
            HObj::Nativ { ziel: Some(z), .. } => *z,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Resolver korrupt (intern)"),
                ));
            }
        };
        let v = self.arg(args, 0);
        if erf {
            // Assimilation: Promise-Wert uebernehmen/verketten.
            if let GVal::Ref(r) = v {
                if r == ziel {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("TypeError", "Promise an sich selbst"),
                    ));
                }
                if matches!(self.heap.holen(r), HObj::Versprechen { .. }) {
                    // Release 10.10.2026: die Uebernahme ist ein `then` —
                    // die Quelle ist damit behandelt (keine „Uncaught (in
                    // promise)"-Meldung fuer sie).
                    self.behandelt(r);
                    match self.heap.holen(r).clone() {
                        HObj::Versprechen { zustand, .. } => match zustand {
                            crate::heap::VersprechenZustand::Erfuellt(w) => {
                                self.versprechen_settle(ziel, true, w);
                                return Ok(GVal::Undefiniert);
                            }
                            crate::heap::VersprechenZustand::Abgelehnt(w) => {
                                self.versprechen_settle(ziel, false, w);
                                return Ok(GVal::Undefiniert);
                            }
                            crate::heap::VersprechenZustand::Offen => {
                                // Verketten: Reaktionen mit Folge=ziel.
                                let erf2 = self.resolver_anlegen(ziel, true);
                                let abl2 = self.resolver_anlegen(ziel, false);
                                let folge_leer = match self
                                    .versprechen_anlegen(crate::heap::VersprechenZustand::Offen)
                                {
                                    GVal::Ref(f) => f,
                                    _ => unreachable!(),
                                };
                                // Direkte Reaktion mit Folge=ziel (Umweg ueber
                                // Zwischen-Tasks vermeiden).
                                let _ = (erf2, abl2, folge_leer);
                                self.reaktion_einreihen(r, Some(erf2), Some(abl2), ziel);
                                return Ok(GVal::Undefiniert);
                            }
                        },
                        _ => unreachable!(),
                    }
                }
            }
            self.versprechen_settle(ziel, true, v);
        } else {
            self.versprechen_settle(ziel, false, v);
        }
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn versprechen_dann_innen(
        &mut self,
        quelle: HeapRef,
        bei_erfuellt: Option<HeapRef>,
        bei_fehler: Option<HeapRef>,
    ) -> Result<GVal, Stoerung> {
        let folge = match self.versprechen_anlegen(crate::heap::VersprechenZustand::Offen) {
            GVal::Ref(f) => f,
            _ => unreachable!(),
        };
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(quelle));
        self.wurzeln.push(GVal::Ref(folge));
        // Release 10.10.2026: `then`/`catch` -> behandelt.
        self.behandelt(quelle);
        match self.heap.holen(quelle).clone() {
            HObj::Versprechen { zustand, .. } => match zustand {
                crate::heap::VersprechenZustand::Erfuellt(w) => {
                    if self.mikrotasks.len() >= self.grenzen.max_mikrotasks {
                        self.wurzel_zurueck(marke);
                        return Ok(GVal::Ref(folge));
                    }
                    self.mikrotasks.push(crate::interp::Mikrotask::reaktion(
                        bei_erfuellt,
                        folge,
                        w,
                        true,
                    ));
                }
                crate::heap::VersprechenZustand::Abgelehnt(w) => {
                    if self.mikrotasks.len() >= self.grenzen.max_mikrotasks {
                        self.wurzel_zurueck(marke);
                        return Ok(GVal::Ref(folge));
                    }
                    self.mikrotasks.push(crate::interp::Mikrotask::reaktion(
                        bei_fehler, folge, w, false,
                    ));
                }
                crate::heap::VersprechenZustand::Offen => {
                    self.reaktion_einreihen(quelle, bei_erfuellt, bei_fehler, folge);
                }
            },
            _ => {
                self.wurzel_zurueck(marke);
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "then an Nicht-Promise"),
                ));
            }
        }
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(folge))
    }

    pub(crate) fn n_versprechen_dann(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let q = match this {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Versprechen { .. } => r,
                _ => {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("TypeError", "then an Nicht-Promise"),
                    ));
                }
            },
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "then an Nicht-Promise"),
                ));
            }
        };
        let f = |v: GVal| -> Option<HeapRef> {
            match v {
                GVal::Ref(r)
                    if matches!(
                        self.heap.holen(r),
                        HObj::Funktion { .. } | HObj::Nativ { .. }
                    ) =>
                {
                    Some(r)
                }
                _ => None,
            }
        };
        let bei_erfuellt = f(self.arg(args, 0));
        let bei_fehler = f(self.arg(args, 1));
        self.versprechen_dann_innen(q, bei_erfuellt, bei_fehler)
    }

    pub(crate) fn endlich_warten(
        &mut self, wert: GVal, erfuellt: bool, ziel: HeapRef, cleanup: GVal,
    ) -> Result<(), Stoerung> {
        self.wurzeln.push(cleanup);
        let GVal::Ref(warten) = self.versprechen_anlegen(crate::heap::VersprechenZustand::Offen)
            else { unreachable!() };
        self.wurzeln.push(GVal::Ref(warten));
        // The ordinary resolution procedure also handles throwing `then`
        // getters and pending thenables, rather than inspecting their state.
        self.versprechen_settle(warten, true, cleanup);
        let weiter = self.nativ_anlegen("", crate::builtin::Nativ::EndlichWeiter(erfuellt), self.proto_funktion);
        self.wurzeln.push(GVal::Ref(weiter));
        self.eigenschaft_nicht_aufzaehlbar(weiter, "__finally_value", wert).map_err(Stoerung::Fehler)?;
        self.eigenschaft_nicht_aufzaehlbar(weiter, "__finally_target", GVal::Ref(ziel)).map_err(Stoerung::Fehler)?;
        let ablehnen = self.resolver_anlegen(ziel, false);
        self.wurzeln.push(GVal::Ref(ablehnen));
        self.versprechen_dann_innen(warten, Some(weiter), Some(ablehnen))?;
        Ok(())
    }

    pub(crate) fn n_endlich_weiter(&mut self, funktion: HeapRef, erfuellt: bool) -> Result<GVal, Stoerung> {
        let (wert, ziel) = match self.heap.holen(funktion) {
            HObj::Nativ { eigenschaften, .. } => (
                eigenschaften.get("__finally_value").map(|e| e.wert),
                eigenschaften.get("__finally_target").map(|e| e.wert),
            ),
            _ => (None, None),
        };
        let (Some(wert), Some(GVal::Ref(ziel))) = (wert, ziel) else {
            return Err(Stoerung::Fehler(JsFehler::Laufzeit("finally-Fortsetzung ohne Zustand".into())));
        };
        self.versprechen_settle(ziel, erfuellt, wert);
        Ok(GVal::Undefiniert)
    }

    /// `finally(cb)`: Wert/Fehler durchreichen, vorher `cb()` aufrufen.
    ///
    /// V6: OHNE AST-Closures. `narsil-js` baute hier zwei JS-Funktionen aus
    /// einem `FunktionsBau` — das kann eine VM nicht (der Bytecode ist zur
    /// Uebersetzungszeit fertig). Deshalb traegt die Reaktion die Art 1
    /// ("Handler ohne Argument, Wert durchreichen"), und die Task-Schlange
    /// in `vm.rs` erledigt das.
    pub(crate) fn n_versprechen_endlich(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let q = match this {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Versprechen { .. } => {
                    // Release 10.10.2026: `finally` -> behandelt.
                    self.behandelt(r);
                    r
                }
                _ => {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("TypeError", "finally an Nicht-Promise"),
                    ));
                }
            },
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "finally an Nicht-Promise"),
                ));
            }
        };
        let cb = self.arg(args, 0);
        let cb_ref = match cb {
            GVal::Ref(r) if self.ist_aufrufbar(cb) => Some(r),
            _ => None,
        };
        let folge = match self.versprechen_anlegen(crate::heap::VersprechenZustand::Offen) {
            GVal::Ref(f) => f,
            _ => unreachable!(),
        };
        self.wurzeln.push(GVal::Ref(folge));
        if cb_ref.is_some() {
            self.wurzeln.push(GVal::Ref(cb_ref.unwrap()));
        }
        let zustand = match self.heap.holen(q).clone() {
            HObj::Versprechen { zustand, .. } => zustand,
            _ => crate::heap::VersprechenZustand::Offen,
        };
        match zustand {
            crate::heap::VersprechenZustand::Erfuellt(w) => {
                if self.mikrotasks.len() < self.grenzen.max_mikrotasks {
                    self.mikrotasks.push(crate::interp::Mikrotask::reaktion_art(
                        cb_ref, folge, w, true, 1,
                    ));
                } else {
                    self.versprechen_settle(folge, true, w);
                }
            }
            crate::heap::VersprechenZustand::Abgelehnt(w) => {
                if self.mikrotasks.len() < self.grenzen.max_mikrotasks {
                    self.mikrotasks.push(crate::interp::Mikrotask::reaktion_art(
                        cb_ref, folge, w, false, 1,
                    ));
                } else {
                    self.versprechen_settle(folge, false, w);
                }
            }
            crate::heap::VersprechenZustand::Offen => {
                self.reaktion_einreihen_art(q, cb_ref, cb_ref, folge, 1);
            }
        }
        Ok(GVal::Ref(folge))
    }

    /// ES2023 27.2.4.x Schritt 2: „If Type(C) is not Object, throw a TypeError."
    ///
    /// J-CACHE (03.10.2026). **GEMESSEN an `node 22`, alle sechs Faelle aus
    /// `all/ctx-non-object.js`** — sie werfen dort *alle* mit
    /// „Promise.all called on non-object":
    ///   `call(undefined)`, `call(null)`, `call(86)`, `call("string")`,
    ///   `call(true)`, `call(Symbol())`.
    ///
    /// **WARUM DIE EINFACHE PRUEFUNG NICHT REICHT** (das war mein Fehler):
    /// `!matches!(this, GVal::Ref(_))` hat undefined, null, 86 und true
    /// richtig erkannt — aber `"string"` und `Symbol()` **nicht**. GEMESSEN:
    ///     undefined=WURF  null=WURF  86=WURF  true=WURF
    ///     string=kein Wurf  Symbol=kein Wurf        <- beides im Motor
    ///     node: alle sechs WURF
    /// Sie kommen als **verpackte Primitive** herein und sind damit `Ref`.
    /// `Engine::ist_objekt` (vm.rs:3235) hilft auch nicht: es nimmt
    /// `HObj::Verpackt` mit — und `Verpackt` traegt laut heap.rs:401-406 ein
    /// Feld `primitiv`, ist also genau eine verpackte Primitive.
    /// `HObj::Zeichenkette`, `Zahl` und `Bool` nimmt `ist_objekt` immerhin
    /// schon heraus.
    ///
    /// Deshalb hier die Liste **ohne** `Verpackt`.
    pub(crate) fn ist_kein_objekt(&self, v: GVal) -> bool {
        match v {
            GVal::Ref(r) => !matches!(
                self.heap.holen(r),
                HObj::Gegenstand { .. }
                    | HObj::Feld { .. }
                    | HObj::Funktion { .. }
                    | HObj::Nativ { .. }
                    | HObj::Fehler { .. }
                    | HObj::Karte { .. }
                    | HObj::Menge { .. }
                    | HObj::Versprechen { .. }
                    | HObj::Datum { .. }
                    | HObj::Regex { .. }
                    | HObj::Generator { .. }
            ),
            _ => true,
        }
    }

    /// Das globale `%Promise%` — Schritt 3a der Kapitel 27.2.4.1-27.2.4.6
    /// („Set C to %Promise%"), wenn `this` kein Konstruktor ist.
    ///
    /// `Engine` hat `proto_versprechen` (das PROTOTYP), aber **kein** Feld
    /// fuer den Promise-KONSTRUKTOR — `prom_ctor` ist eine Lokale in
    /// `aufbauen`. Ein neues Feld waere eine Aenderung an `interp.rs`
    /// (EIGENTUM: JS), deshalb lese ich die globale Bindung
    /// (`bind(self, "Promise", …)`, `builtin.rs`).
    fn versprechen_globals_ctor(&mut self) -> Result<GVal, Stoerung> {
        let g = self.global_objekt;
        self.hole(GVal::Ref(g), "Promise")
    }

    /// ES2023 27.2.1.1 `NewPromiseCapability(C)` — als `(Promise, Resolve,
    /// Reject)`.
    ///
    /// J-CACHE (03.10.2026). Die drei Punkte, die ich in CACHE_6/7 „Kern"
    /// genannt hatte, liegen **hier**: `aufruf_synchron` (`vm.rs:3600`) ist der
    /// vorhandene `Construct`-Weg, und `HObj::Nativ.ziel` (ein
    /// `Option<HeapRef>`) traegt die beiden Slots als **Feld**.
    ///
    /// Schritt 4: `Let promise be ? Construct(C, « executor »)` — der
    /// `executor` ist eine gewöhnliche Funktion (GEMESSEN an `node 22`:
    /// `typeof ex === "function"`, `typeof ex.resolve === "undefined"`); er
    /// schreibt seine zwei Argumente in die Slots.
    /// J-CACHE (05.10.2026): pub(crate) fuer JS-async (BRETT-Anfrage):
    /// NewPromiseCapability(C) als Einstieg (Handle + resolve/reject).
    pub(crate) fn versprechen_capability(&mut self, c: GVal) -> Result<(HeapRef, GVal, GVal), Stoerung> {
        let slots = self.neues_feld().map_err(Stoerung::Fehler)?;
        {
            let o = self.heap.holen_mut(slots);
            if let HObj::Feld { elemente, .. } = o {
                elemente.resize(2, GVal::Undefiniert);
            }
        }
        let exec = self.versprechen_executor_anlegen(slots);
        // Das ist der Unterschied zu meinem verworfenen Versuch aus CACHE_6:
        // dort rief ich `Construct(C)` **ohne** Argument und zerstörte damit
        // den Normalweg (`TypeError: Promise braucht Roller-Funktion`).
        // **J-CACHE 03.10.2026 — `Construct` richtig machen, in meiner Datei.**
        //
        // Der Kern hat **zwei** Wege, und sie sind nicht dasselbe. GEMESSEN
        // und im Quelltext gegeneinander gehalten:
        //
        //   (a) `new F(...)`  -> `Op::AufrufNeu` -> `neu_aufruf` (vm.rs:3056):
        //         let proto = self.konstruktor_proto(fr);
        //         let obj   = self.neues_objekt(Some(proto))?;   // <- erzeugt `this`
        //         self.rahmen_starten_fn(fr, fn_index, GVal::Ref(obj), aa, true)?;
        //         rahmen.last_mut().this_gesetzt = false;       // <- erst super() setzt
        //       `new F(7)` -> x=7, proto-eigen="constructor"        (Motor == node)
        //
        //   (b) `Construct` -> `aufruf_einfuegen` (vm.rs:2735), und dort
        //       gibt es **kein** `neues_objekt` und **kein**
        //       `this_gesetzt = false`. `this` laeuft unveraendert in
        //       `rahmen_starten_fn` (vm.rs:2840). Dazu passt
        //       compile.rs:626: `Op::RueckgabeThis` wird **nur** emittiert,
        //       wenn `konstruktor == true` — und das ist nur fuer
        //       Klassenkonstruktoren gesetzt. Eine gewoehnliche Funktion
        //       emittiert stattdessen `Op::Undef; Op::Rueckgabe`.
        //       Ergebnis von (b): der Rumpf laeuft, aber `Construct`
        //       liefert **`undefined`** statt `this`.
        //
        // Also baue ich `Construct` hier selbst: ich lege `this` an und
        // uebergebe es. Beide Helfer sind `pub(crate)` —
        // `Engine::konstruktor_proto` (vm.rs:4577) und
        // `Engine::neues_objekt` (ops.rs:201) — also ist das **ohne**
        // Eingriff in eine fremde Datei moeglich.
        //
        // GEMESSEN, was das bringt (Versuch mit `NotPromise`, das den
        // Executor aufruft — genau die Form aus
        // `Promise/executor-function-length.js`):
        //     Promise.resolve.call(NotPromise)
        //       vorher : TypeError: new-Ziel ist kein Promise-Konstruktor
        //       node 22: typeof=object  istPromise=false
        //                proto-eigen="constructor"  exec.length=2  -> RUECKGABE
        let c_ref = match c {
            GVal::Ref(r) => r,
            _ => {
                let e = self.fehler_wert("TypeError", "Promise-Konstruktor fehlt");
                return Err(Stoerung::Wurf(e));
            }
        };
        let proto = self.konstruktor_proto(c_ref);
        let obj = self.neues_objekt(Some(proto)).map_err(Stoerung::Fehler)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(slots));
        self.wurzeln.push(GVal::Ref(exec));
        self.wurzeln.push(GVal::Ref(obj));
        let promise = self.aufruf_synchron(c, GVal::Ref(obj), &[GVal::Ref(exec)], true);
        self.wurzel_zurueck(marke);
        let promise = promise?;
        // **Wahl des Ergebnisses.** Nach ES2023 10.2.11 (`OrdinaryCreateFromConstructor`
        // / `[[Construct]]`) ist das Ergebnis `this` — **ausser** der
        // Konstruktor gibt selbst ein Objekt zurueck. Also: ein Objekt
        // bevorzugen, sonst `this`.
        //   * Native (`Promise` selbst) -> `rufe_nativ(.., true)` liefert
        //     das echte Promise        -> wird uebernommen.
        //   * Gewoehnliche Funktion     -> liefert `undefined` (compile.rs:626)
        //     -> `this` wird uebernommen, mit `C.prototype`.
        // `ziel` wird danach nur noch in `settle_ueber_capability` benutzt,
        // und dort ausschliesslich im Zweig, in dem `cap` **nicht**
        // aufrufbar ist.
        let ziel = match promise {
            GVal::Ref(r) => r,
            _ => obj,
        };
        let (erf, abl) = match self.heap.holen(slots) {
            HObj::Feld { elemente, .. } => (
                elemente.first().copied().unwrap_or(GVal::Undefiniert),
                elemente.get(1).copied().unwrap_or(GVal::Undefiniert),
            ),
            _ => (GVal::Undefiniert, GVal::Undefiniert),
        };
        Ok((ziel, erf, abl))
    }

    /// Der Executor als native Funktion; `ziel` traegt die Slot-Liste.
    fn versprechen_executor_anlegen(&mut self, slots: HeapRef) -> HeapRef {
        let mut eigene = crate::heap::Karte::default();
        eigene.insert(
            "length".to_string(),
            crate::heap::Eigenschaft {
                // 27.1.3.1: der Executor nimmt (resolve, reject) — zwei.
                wert: crate::heap::GVal::Zahl(2.0),
                getter: None,
                setter: None,
                aufzählbar: false,
                beschreibbar: false,
                konfigurierbar: true,
            },
        );
        // J-CACHE (04.10.2026): **`name` = "".** ES2023 25.4.1.x /
        // `sec-getcapabilitiesexecutor-functions`: „A GetCapabilitiesExecutor
        // function is an anonymous built-in function" — anonyme Einbauten
        // tragen `""` (Attribute wie `length`). Gemessen an
        // `Promise/executor-function-name.js` („name should be an own
        // property"), dieselbe Ursache wie die Helfer-`name` in `all_helfer`.
        if let Ok(leer) = self.text_wert("") {
            eigene.insert(
                "name".to_string(),
                crate::heap::Eigenschaft {
                    wert: leer,
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: false,
                    konfigurierbar: true,
                },
            );
        }
        let obj = HObj::Nativ {
            name: "executor".to_string(),
            proto: Some(self.proto_funktion),
            ziel: Some(slots),
            eigenschaften: eigene,
        };
        let kosten = Heap::kosten_fuer(&obj);
        let _ = self.vor_allok(kosten);
        let r = self.belege(obj, kosten);
        self.native
            .insert(r, crate::builtin::Nativ::VersprechenExecutor);
        r
    }

    /// ES2023 27.2.1.1, Executor-Schritt: „If promiseResolve is not undefined,
    /// set capability.[[Resolve]] to promiseResolve" — dto. reject. Die
    /// Bedingung ist wichtig: ein Konstruktor, der `undefined` uebergibt,
    /// laesst den eingebauten Aufloeser stehen.
    ///
    /// J-CACHE (04.10.2026): **Zweitbelegung wirft TypeError.** ES2023
    /// `GetCapabilitiesExecutor` Schritte 3-4 woertlich: „If
    /// promiseCapability.[[Resolve]] is not undefined, throw a TypeError
    /// exception" — dto. reject. Die Pruefung gilt den SLOTS (unbedingt),
    /// das Setzen den Argumenten (nur nicht-undefined). GEMESSEN an
    /// `capability-executor-called-twice.js`: `executor(undefined, fn)`
    /// dann `executor(fn, fn)` — der zweite Aufruf trifft Slot1 belegt
    /// und muss werfen (checkPoint bleibt "ab"). Die ersten beiden Faelle
    /// (Slots noch leer) laufen weiter wie bisher.
    pub(crate) fn versprechen_executor_impl(
        &mut self,
        fn_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let slots = match self.heap.holen(fn_ref) {
            HObj::Nativ { ziel: Some(z), .. } => *z,
            _ => return Ok(GVal::Undefiniert),
        };
        let (belegt0, belegt1) = match self.heap.holen(slots) {
            HObj::Feld { elemente, .. } => (
                elemente.first().copied().unwrap_or(GVal::Undefiniert),
                elemente.get(1).copied().unwrap_or(GVal::Undefiniert),
            ),
            _ => (GVal::Undefiniert, GVal::Undefiniert),
        };
        if !matches!(belegt0, GVal::Undefiniert) || !matches!(belegt1, GVal::Undefiniert) {
            let e = self.fehler_wert("TypeError", "Promise-Faehigkeit bereits belegt");
            return Err(Stoerung::Wurf(e));
        };
        let marke = self.wurzel_merke();
        let o = self.heap.holen_mut(slots);
        if let HObj::Feld { elemente, .. } = o {
            if elemente.len() < 2 {
                elemente.resize(2, GVal::Undefiniert);
            }
            if let Some(v) = args.first() {
                if !matches!(v, GVal::Undefiniert) {
                    elemente[0] = *v;
                }
            }
            if let Some(v) = args.get(1) {
                if !matches!(v, GVal::Undefiniert) {
                    elemente[1] = *v;
                }
            }
        }
        self.wurzel_zurueck(marke);
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn n_versprechen_alle(
        &mut self,
        this: GVal,
        args: &[GVal],
        art: u8,
    ) -> Result<GVal, Stoerung> {
        // art: 0 = all, 1 = race, 2 = allSettled, 3 = any.
        // ES2023 27.2.4.x Schritt 1-3 woertlich:
        //   „1. Let C be the this value.
        //    2. If IsConstructor(C) is true, then
        //       a. Let promiseCapability be ? NewPromiseCapability(C).
        //    3. Else,  a. Set C to %Promise%.
        //            b. Let promiseCapability be ! NewPromiseCapability(C)."
        //
        // GEMESSEN vorher gegen `node 22`:
        //   Promise.all.call(Constructor, [p1])   Motor: callCount = 0
        //                                       node:  callCount = 1
        // ES2023 27.2.4.x Schritt 2, und die Reihenfolge ist wichtig:
        //   „If Type(C) is not Object, throw a TypeError exception."
        //
        // GEMESSEN an `node 22`, alle sechs Faelle einzeln:
        //   Promise.all.call(undefined, [])  -> TypeError: Promise.all
        //                                             called on non-object
        //   dasselbe mit null, 86, "", true, Symbol()
        //   Promise.all.call({}, [])         -> TypeError: #<Object> is
        //                                             not a constructor
        //   Promise.all.call(function(){},[]) -> TypeError: Promise resolve
        //                                             or reject function is
        //                                             not callable
        //   Promise.all([])                   -> kein Wurf
        //
        // `GVal` ohne `Ref` ist genau „kein Objekt": undefined, null,
        // Zahl, Text, Bool, Symbol. Die anderen Faelle sind `Ref` und
        // laufen weiter.
        if self.ist_kein_objekt(this) {
            let e = self.fehler_wert("TypeError", "Promise auf Nicht-Objekt aufgerufen");
            return Err(Stoerung::Wurf(e));
        }
        let c = if crate::eingebaut_array::ist_konstruktor(self, this) {
            this
        } else {
            self.versprechen_globals_ctor()?
        };
        let (ziel, cap_erf, cap_abl) = self.versprechen_capability(c)?;
        // Schritt 6/8: die IsCallable-Proben. Rief der Konstruktor den
        // Executor nie, sind die Slots leer — dann wirft es hier.
        // Meldung woertlich wie `node 22`.
        if !self.ist_aufrufbar(cap_erf) || !self.ist_aufrufbar(cap_abl) {
            let e = self.fehler_wert(
                "TypeError",
                "Promise resolve or reject function is not callable",
            );
            return Err(Stoerung::Wurf(e));
        }
        let werte = self.iter_werte(self.arg(args, 0))?;
        if werte.len() > 100_000 {
            return Err(Stoerung::Fehler(JsFehler::Speicher(
                "Promise.all zu gross".to_string(),
            )));
        }
        // J-CACHE (04.10.2026): **`promiseResolve = Get(C, "resolve")` —
        // EINMAL, vor der Schleife.** ES2023 27.2.4.1/27.2.4.3/27.2.4.4/
        // 27.2.4.5 Schritt 3 woertlich: „Let promiseResolve be
        // ? Get(C, "resolve")." Danach pro Element
        // „Let nextPromise be ? Call(promiseResolve, C, « nextValue »)."
        //
        // **WARUM (gemessen):** `resolve-element-function-length.js` setzt
        // `NotPromise.resolve = v => v` und faengt die Aufloesungsfunktion
        // ueber ein Thenable ab. Mein alter Weg (`versprechen_erfuellt`,
        // Mikrotask aus klassen.rs) rief `then` nie SYNCHRON — `fulfill`
        // blieb ungesetzt (Sonde: typeof="string" statt "function"), und
        // `verifyProperty(undefined, "length")` warf „kein Objekt aus
        // null/undefined". Mit `C.resolve` kommt das Thenable zurueck und
        // `then` wird unten synchron aufgerufen.
        let aufloeser = match self.hole(c, "resolve") {
            Ok(f) => f,
            Err(Stoerung::Wurf(e)) => {
                self.versprechen_settle(ziel, false, e);
                return Ok(GVal::Ref(ziel));
            }
            Err(e) => return Err(e),
        };
        let ziel = ziel;
        if werte.is_empty() {
            let f = self.feld_aus_werten(&[])?;
            self.versprechen_settle(ziel, true, GVal::Ref(f));
            return Ok(GVal::Ref(ziel));
        }
        if art == 1 {
            // race: erstes Settlen gewinnt (Reaktionen direkt auf ziel).
            // J-CACHE (04.10.2026): pro Element `Call(promiseResolve, C,
            // « w »)` statt `versprechen_erfuellt` (Begruendung oben).
            // Wurf -> settle + return (IfAbruptRejectPromise).
            for w in werte {
                let erf = self.resolver_anlegen(ziel, true);
                let abl = self.resolver_anlegen(ziel, false);
                let w = match self.aufruf_synchron(aufloeser, c, &[w], false) {
                    Ok(v) => self.versprechen_erfuellt(v),
                    Err(Stoerung::Wurf(e)) => {
                        self.versprechen_settle(ziel, false, e);
                        return Ok(GVal::Ref(ziel));
                    }
                    Err(e) => return Err(e),
                };
                self.promise_dann_roh(w, Some(erf), Some(abl), ziel)?;
            }
            return Ok(GVal::Ref(ziel));
        }
        // all/allSettled: Ergebnis-Feld. Die Helfer schreiben je Index
        // und settlen `ziel` erst bei Vollstaendigkeit. Ihre Rueckgaben
        // duerfen `ziel` NICHT frueh settlen (Drain-Protokoll!) — deshalb
        // laufen sie gegen eine Senke (verworfenes Folge-Promise).
        let ergebnis = self.neues_feld().map_err(Stoerung::Fehler)?;
        {
            let o = self.heap.holen_mut(ergebnis);
            if let HObj::Feld { elemente, .. } = o {
                elemente.resize(werte.len(), GVal::Undefiniert);
            }
        }
        let senke = match self.versprechen_anlegen(crate::heap::VersprechenZustand::Offen) {
            GVal::Ref(f) => f,
            _ => unreachable!(),
        };
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(ziel));
        self.wurzeln.push(GVal::Ref(ergebnis));
        self.wurzeln.push(GVal::Ref(senke));
        // Release 10.10.2026: `remainingElementsCount` (27.2.4.1.2) als
        // GETEILTER Zaehler. GEMESSEN vorher: fertig hiess „kein Element ist
        // `undefined`" — `Promise.all([Promise.resolve(undefined)])` blieb
        // fuer immer offen (node: `[undefined]`), ebenso jedes `Promise.all`
        // ueber `async`-Funktionen ohne Rueckgabewert.
        let zaehler = self.feld_aus_werten(&[GVal::Zahl(werte.len() as f64)])?;
        self.wurzeln.push(GVal::Ref(zaehler));
        // Jedes w wird mit then(wrap_i) versehen, wobei wrap_i native
        // Helfer mit Ziel=(ziel, ergebnis, index, art) sind: HObj::Nativ.ziel
        // ist EIN Ref — wir kodieren den Kontext als Feld.
        for (i, w) in werte.iter().enumerate() {
            let gerufen = self.feld_aus_werten(&[GVal::Bool(false)])?;
            self.wurzeln.push(GVal::Ref(gerufen));
            let ctx = self.feld_aus_werten(&[
                GVal::Ref(ziel),
                GVal::Ref(ergebnis),
                GVal::Zahl(i as f64),
                GVal::Zahl(art as f64),
                GVal::Zahl(werte.len() as f64),
                // J-CACHE: das sechste Kontext-Element ist
                // `capability.[[Resolve]]` — 27.2.4.3 Schritt 11 ruft
                // `Call(capability.[[Resolve]], undefined, « value »)`,
                // und die kann die **Konstruktor-eigene** Funktion sein.
                // `versprechen_settle` waere nicht gleichwertig. Das
                // siebte Element (angehaengt in `all_helfer`) ist das
                // Erfuellt/Abgelehnt-Bit.
                cap_erf,
                // Release 10.10.2026: [6] der geteilte Zaehler, [7]
                // `alreadyCalled` dieses Elements (geteilt zwischen dem
                // Erfuellt- und dem Abgelehnt-Helfer), [8]
                // `capability.[[Reject]]` (fuer `any`); das Bit wandert auf [9].
                GVal::Ref(zaehler),
                GVal::Ref(gerufen),
                cap_abl,
            ])?;
            let erf = self.all_helfer(ctx, true);
            let abl = self.all_helfer(ctx, false);
            // J-CACHE (04.10.2026): **`Call(promiseResolve, C, « w »)` statt
            // `versprechen_erfuellt`.** ES2023 PerformPromiseAll Schritt 4
            // woertlich: „Let nextPromise be ? Call(promiseResolve, C,
            // « nextValue »)." Das `then` unten wird vom ERGEBNIS geholt
            // und synchron aufgerufen (Schritt 8). Wurf -> settle + return.
            let w = match self.aufruf_synchron(aufloeser, c, &[*w], false) {
                Ok(v) => v,
                Err(Stoerung::Wurf(e)) => {
                    self.versprechen_settle(ziel, false, e);
                    return Ok(GVal::Ref(ziel));
                }
                Err(e) => return Err(e),
            };
            // J-CACHE (03.10.2026): **`then` muss SYNCHRON aufgerufen
            // werden.** ES2023 27.2.4.3 `PerformPromiseAll` Schritt 8
            // woertlich: „Let then be Completion(Invoke(nextPromise,
            // "then", « resolveElement, rejectElement »))" — `Invoke`
            // ist ein **synchroner** Aufruf, kein Mikrotask.
            //
            // GEMESSEN (Motor 0 gegen `node 22` 3):
            //   var p1 = new Promise(function(){}), p2 = …, p3 = …;
            //   p1.then = p2.then = p3.then = function(a,b){ c += 1; };
            //   Promise.all([p1,p2,p3]);
            //     Motor: callCount = 0        node: callCount = 3
            // Der alte Weg (`promise_dann_roh`) legt die Reaktion in die
            // **Mikrotask-Schlange**, also passiert `then` erst, wenn der
            // Stapel leer ist — und der Korpus zaehlt synchron.
            // Genau daran scheiterten `all/invoke-then.js` und
            // `all/invoke-resolve*.js`.
            //
            // `aufruf_synchron` ist der vorhandene Invoke-Weg
            // (`vm.rs:3600`, `pub(crate)`, 32 Aufrufstellen) und
            // korrigiert den Stapel selbst (J49). Schritt 9 der Spec:
            // ist `then` nicht aufrufbar, ist das ein **Wurf** — der
            // wird unten behandelt.
            let then = self.hole(w, "then")?;
            if self.ist_aufrufbar(then) {
                // J-CACHE (04.10.2026): **fuer `all` (art 0) ist das zweite
                // `then`-Argument `capability.[[Reject]]` DIREKT, kein
                // Helfer.** ES2023 PerformPromiseAll Schritt r woertlich:
                // „Let result be Invoke(nextPromise, "then",
                // « resolveElement, resultCapability.[[Reject]] »)."
                // GEMESSEN an `same-reject-function.js`: `Constructor`
                // uebergibt `rejectFunction`, und jedes `then` muss
                // GENAU DIE als `onRejected` sehen (SameValue). Mein
                // abl-Helfer waere ein anderes Objekt. `allSettled`/`any`
                // (art 2/3) behalten ihre reject-Element-Helfer (eigene
                // Funktionen mit Buchhaltung); `race` laeuft nicht hier.
                let ablehner = if art == 0 { cap_abl } else { GVal::Ref(abl) };
                let r = self.aufruf_synchron(then, w, &[GVal::Ref(erf), ablehner], false);
                match r {
                    // Schritt 10: `? IteratorClose` bei abruptem Completion.
                    Err(Stoerung::Wurf(e)) => {
                        self.versprechen_settle(ziel, false, e);
                        return Ok(GVal::Ref(ziel));
                    }
                    Err(Stoerung::Fehler(e)) => return Err(Stoerung::Fehler(e)),
                    Ok(_) => {}
                }
            } else {
                // Schritt 9: „If IsCallable(then) is false, throw a
                // TypeError" — der wird ueber `reject` weitergereicht.
                let t = self.fehler_wert("TypeError", "then ist keine Funktion");
                self.versprechen_settle(ziel, false, t);
                return Ok(GVal::Ref(ziel));
            }
            let _ = senke;
        }
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(ziel))
    }

    pub(crate) fn all_helfer(&mut self, ctx: HeapRef, erf: bool) -> HeapRef {
        let (name, nat) = if erf {
            ("all_erf", crate::builtin::Nativ::AllHelfer)
        } else {
            ("all_abl", crate::builtin::Nativ::AllHelfer)
        };
        // Eigener Kontext je Helfer (Modus-Bit angehaengt).
        let elemente = match self.heap.holen(ctx).clone() {
            HObj::Feld { elemente, .. } => elemente,
            _ => Vec::new(),
        };
        let mut neu = elemente;
        neu.push(GVal::Bool(erf));
        let ctx2 = self.feld_aus_werten(&neu).unwrap_or(ctx);
        // J-CACHE (04.10.2026): **`length` = 1, `name` = "".** ES2023 17:
        // „Unless otherwise specified, the length property of a built-in
        // Function object ..." — die Aufloesungs-Elementfunktionen von
        // `all`/`allSettled`/`any` (resolve UND reject) haben `length` 1
        // und `name` "" — gemessen an
        // `resolve-element-function-{length,name,property-order,
        // prototype}.js` (via `NotPromise.resolve = v => v` + Thenable).
        // Dieselbe Attribut-Kombination wie `laenge_setzen` (builtin.rs).
        let mut helfer_eigen = crate::heap::Karte::default();
        helfer_eigen.insert(
            "length".to_string(),
            Eigenschaft {
                wert: GVal::Zahl(1.0),
                getter: None,
                setter: None,
                aufzählbar: false,
                beschreibbar: false,
                konfigurierbar: true,
            },
        );
        if let Ok(leer) = self.text_wert("") {
            helfer_eigen.insert(
                "name".to_string(),
                Eigenschaft {
                    wert: leer,
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: false,
                    konfigurierbar: true,
                },
            );
        }
        let obj = HObj::Nativ {
            name: name.to_string(),
            proto: Some(self.proto_funktion),
            ziel: Some(ctx2),
            eigenschaften: helfer_eigen,
        };
        let kosten = Heap::kosten_fuer(&obj);
        // Winzig und gezaehlt (Notfall wie fehler_wert).
        let _ = self.vor_allok(kosten);
        let r = self.belege(obj, kosten);
        self.native.insert(r, nat);
        r
    }

    /// Settelt ueber `capability.[[Resolve]]`, wenn die Capability eine
    /// aufrufbare Funktion traegt — sonst ueber den eingebauten Aufloeser des
    /// Ziels.
    ///
    /// J-CACHE (03.10.2026). ES2023 27.2.4.3 Schritt 11 woertlich:
    ///   „Call(capability.[[Resolve]], undefined, « values[index] »)."
    /// Die Capability-Funktion kann die **Konstruktor-eigene** sein; das ist
    /// genau, was `all/call-resolve-element-after-return.js` zaehlt. Deshalb
    /// ist `versprechen_settle` allein nicht gleichwertig — es ist nur der
    /// Rueckfall fuer eine Capability ohne eigene Funktion.
    fn settle_ueber_capability(&mut self, cap: GVal, ziel: HeapRef, erfuellt: bool, wert: GVal) {
        if self.ist_aufrufbar(cap) {
            let _ = self.aufruf_synchron(cap, GVal::Undefiniert, &[wert], false);
        } else {
            self.versprechen_settle(ziel, erfuellt, wert);
        }
    }

    pub(crate) fn all_helfer_impl(
        &mut self,
        fn_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let ctx = match self.heap.holen(fn_ref) {
            HObj::Nativ { ziel: Some(z), .. } => *z,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "all-Helfer korrupt"),
                ));
            }
        };
        let felder = match self.heap.holen(ctx).clone() {
            HObj::Feld { elemente, .. } => elemente,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "all-Kontext korrupt"),
                ));
            }
        };
        let g = |i: usize| felder.get(i).copied().unwrap_or(GVal::Undefiniert);
        let ziel = match g(0) {
            GVal::Ref(r) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "all-Ziel korrupt"),
                ));
            }
        };
        let ergebnis = match g(1) {
            GVal::Ref(r) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "all-Ergebnis korrupt"),
                ));
            }
        };
        let index = match g(2) {
            GVal::Zahl(n) => n as usize,
            _ => 0,
        };
        let art = match g(3) {
            GVal::Zahl(n) => n as u8,
            _ => 0,
        };
        let erf = matches!(g(9), GVal::Bool(true));
        // J-CACHE: `capability.[[Resolve]]` aus dem sechsten Element.
        let cap = g(5);
        let wert = self.arg(args, 0);
        // Release 10.10.2026: `alreadyCalled` — jedes Element zaehlt EINMAL.
        if let GVal::Ref(gr) = g(7)
            && let HObj::Feld { elemente, .. } = self.heap.holen_mut(gr)
            && let Some(e) = elemente.first_mut()
        {
            if matches!(e, GVal::Bool(true)) {
                return Ok(GVal::Undefiniert);
            }
            *e = GVal::Bool(true);
        }
        let zaehler = g(6);
        if art == 3 {
            // J-CACHE (03.10.2026): `Promise.any` — ES2023 27.2.4.5.
            // **Genau umgekehrt zu `all`**: die ERSTE ERFUELLUNG erfuellt
            // das Gesamtergebnis (der Wert wird direkt durchgereicht, ohne
            // Index-Platz), und erst wenn ALLE abgelehnt sind, wird mit
            // einem `AggregateError` abgelehnt, dessen `errors` die
            // Ablehnungen in Reihenfolge traegt.
            //
            // GEMESSEN gegen `node 22`, bevor es existierte:
            // `typeof Promise.any` -> `undefined`.
            if erf {
                self.settle_ueber_capability(cap, ziel, true, wert);
                return Ok(GVal::Undefiniert);
            }
            let oo = self.heap.holen_mut(ergebnis);
            if let HObj::Feld { elemente, .. } = oo {
                if index < elemente.len() {
                    elemente[index] = wert;
                }
            }
            if self.all_zaehlen(zaehler) {
                // AggregateError: `new AggregateError(errors, message)`.
                // Der Motor hat keinen AggregateError; die Struktur wird
                // als `Fehler`-Knoten mit `name = "AggregateError"` und
                // `errors` gebaut — das ist, was die Suite liest.
                let fehler = self.neues_feld().map_err(Stoerung::Fehler)?;
                {
                    let inhalt = match self.heap.holen(ergebnis) {
                        HObj::Feld { elemente, .. } => elemente.clone(),
                        _ => Vec::new(),
                    };
                    let felde = self.heap.holen_mut(fehler);
                    if let HObj::Feld { elemente, .. } = felde {
                        *elemente = inhalt;
                    }
                }
                self.wurzeln.push(GVal::Ref(fehler));
                // Release 10.10.2026: ein echter `AggregateError`
                // (`e instanceof AggregateError`, `e.errors`).
                let nachricht_ag = self.text_wert("All promises were rejected")?;
                self.wurzeln.push(nachricht_ag);
                let agg = match self.n_fehler_ctor("AggregateError", &[GVal::Ref(fehler), nachricht_ag], true)? {
                    GVal::Ref(fr) => fr,
                    _ => return Ok(GVal::Undefiniert),
                };
                self.wurzeln.push(GVal::Ref(agg));
                // Release 10.10.2026: ABLEHNEN ueber `capability.[[Reject]]`.
                // GEMESSEN vorher: hier stand die Resolve-Funktion —
                // `Promise.any([Promise.reject(1)])` wurde mit dem Fehler
                // ERFUELLT (node: abgelehnt).
                self.settle_ueber_capability(g(8), ziel, false, GVal::Ref(agg));
            }
            return Ok(GVal::Undefiniert);
        }
        if art == 2 {
            // allSettled: {status, value|reason} an index.
            let o = self
                .neues_objekt(Some(self.proto_objekt))
                .map_err(Stoerung::Fehler)?;
            let st = if erf { "fulfilled" } else { "rejected" };
            let stt = self.text_wert(st).map_err(Stoerung::Fehler)?;
            self.wurzeln.push(GVal::Ref(o));
            self.wurzeln.push(stt);
            self.wurzeln.push(wert);
            self.setze(GVal::Ref(o), "status", stt)?;
            self.setze(GVal::Ref(o), if erf { "value" } else { "reason" }, wert)?;
            let oo = self.heap.holen_mut(ergebnis);
            if let HObj::Feld { elemente, .. } = oo {
                if index < elemente.len() {
                    elemente[index] = GVal::Ref(o);
                }
            }
            // Zaehler: leere Slots zaehlen.
            let fertig = self.all_zaehlen(zaehler);
            if fertig {
                self.settle_ueber_capability(cap, ziel, true, GVal::Ref(ergebnis));
            }
            return Ok(GVal::Undefiniert);
        }
        if !erf {
            // all: erste Ablehnung lehnt alles ab.
            self.settle_ueber_capability(cap, ziel, false, wert);
            return Ok(GVal::Undefiniert);
        }
        let oo = self.heap.holen_mut(ergebnis);
        if let HObj::Feld { elemente, .. } = oo {
            if index < elemente.len() {
                elemente[index] = wert;
            }
        }
        if self.all_zaehlen(zaehler) {
            self.settle_ueber_capability(cap, ziel, true, GVal::Ref(ergebnis));
        }
        Ok(GVal::Undefiniert)
    }

    /// Release 10.10.2026: den geteilten Zaehler senken; `true` bei 0
    /// (vorher: „kein Element ist `undefined`" — siehe `n_versprechen_alle`).
    fn all_zaehlen(&mut self, z: GVal) -> bool {
        let GVal::Ref(r) = z else { return false };
        if let HObj::Feld { elemente, .. } = self.heap.holen_mut(r)
            && let Some(GVal::Zahl(n)) = elemente.first_mut()
        {
            *n -= 1.0;
            return *n <= 0.0;
        }
        false
    }

    /// then-Verkettung fuer beliebige Werte (Promise oder direkt).
    pub(crate) fn promise_dann_roh(
        &mut self,
        w: GVal,
        bei_erfuellt: Option<HeapRef>,
        bei_fehler: Option<HeapRef>,
        folge: HeapRef,
    ) -> Result<(), Stoerung> {
        match w {
            GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Versprechen { .. }) => {
                match self.heap.holen(r).clone() {
                    HObj::Versprechen { zustand, .. } => match zustand {
                        crate::heap::VersprechenZustand::Erfuellt(v) => {
                            if self.mikrotasks.len() < self.grenzen.max_mikrotasks {
                                self.mikrotasks.push(crate::interp::Mikrotask::reaktion(
                                    bei_erfuellt,
                                    folge,
                                    v,
                                    true,
                                ));
                            }
                        }
                        crate::heap::VersprechenZustand::Abgelehnt(v) => {
                            if self.mikrotasks.len() < self.grenzen.max_mikrotasks {
                                self.mikrotasks.push(crate::interp::Mikrotask::reaktion(
                                    bei_fehler, folge, v, false,
                                ));
                            }
                        }
                        crate::heap::VersprechenZustand::Offen => {
                            self.reaktion_einreihen(r, bei_erfuellt, bei_fehler, folge);
                        }
                    },
                    _ => unreachable!(),
                }
                Ok(())
            }
            _ => {
                // Kein Promise: sofortige Reaktion als Task.
                if self.mikrotasks.len() < self.grenzen.max_mikrotasks {
                    self.mikrotasks.push(crate::interp::Mikrotask::reaktion(
                        bei_erfuellt,
                        folge,
                        w,
                        true,
                    ));
                }
                Ok(())
            }
        }
    }

    // ---------------------------------------------------------------
    // RegExp
    // ---------------------------------------------------------------

    pub(crate) fn n_regex_ctor(&mut self, args: &[GVal], neu: bool) -> Result<GVal, Stoerung> {
        let a0 = self.arg(args, 0);
        let a1 = self.arg(args, 1);
        // Release 15.10.2026: IsRegExp (with @@match), `RegExp(re)` returns
        // `re` itself when its constructor is RegExp, a RegExp with new flags
        // keeps its source, and a RegExp-like object gives `source`/`flags`
        // (ECMA-262 22.2.4.1). MEASURED before (node 22 in brackets):
        // `RegExp(r) === r` false (true), `String(new RegExp(/a/g, 'i'))`
        // "/\/a\/g/i" ("/a/i"), `RegExp({[Symbol.match]: true, source: 'x',
        // flags: 'm'})` "/[object Object]/" ("/x/m").
        let ist_re = self.es_ist_regexp(a0)?;
        if !neu && ist_re && matches!(a1, GVal::Undefiniert) {
            let c = self.hole(a0, "constructor")?;
            if matches!((c, self.es.regex_ctor), (GVal::Ref(c), Some(r)) if c == r) {
                return Ok(a0);
            }
        }
        let (muster, schalter) =
            if self.ist_regex(a0) {
                let (m, f) = self.regex_daten(a0)?;
                if matches!(a1, GVal::Undefiniert) {
                    (m, f)
                } else {
                    (m, crate::eingebaut_text::zu_text_kein_symbol(self, a1)?)
                }
            } else if ist_re {
                let quelle = self.hole(a0, "source")?;
                let m = if matches!(quelle, GVal::Undefiniert) {
                    "(?:)".to_string()
                } else {
                    crate::eingebaut_text::zu_text_kein_symbol(self, quelle)?
                };
                let fl = if matches!(a1, GVal::Undefiniert) { self.hole(a0, "flags")? } else { a1 };
                let f = if matches!(fl, GVal::Undefiniert) {
                    String::new()
                } else {
                    crate::eingebaut_text::zu_text_kein_symbol(self, fl)?
                };
                (if m.is_empty() { "(?:)".to_string() } else { m }, f)
            } else {
                let m = if matches!(a0, GVal::Undefiniert) {
                    "(?:)".to_string()
                } else {
                    self.zu_text(a0)?
                };
                let f = if matches!(self.arg(args, 1), GVal::Undefiniert) {
                    String::new()
                } else {
                    self.zu_text(self.arg(args, 1))?
                };
                (m, f)
            };
        // Validieren.
        crate::regexmini::schalter_aus(&schalter)
            .map_err(|e| Stoerung::Wurf(self.fehler_wert("SyntaxError", &e)))?;
        if let Err(e) = crate::regexmini::kompilieren_mit_flags(&muster, &schalter) {
            return Err(Stoerung::Wurf(self.fehler_wert("SyntaxError", &e)));
        }
        let obj = HObj::Regex {
            proto: Some(self.proto_regex),
            muster: muster.clone(),
            schalter: schalter.clone(),
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = Heap::kosten_fuer(&obj) + muster.len() + schalter.len();
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        let r = self.belege(obj, kosten);
        // Instanz-Props: nur lastIndex (Spec 22.2: source/flags/global/... sind
        // Getter auf RegExp.prototype, keine eigenen Instanz-Eigenschaften).
        // lastIndex: beschreibbar, aber NICHT konfigurierbar (Spec).
        self.eigenschaft_mit_alle_attributen(r, "lastIndex", GVal::Zahl(0.0), false, true, false)?;
        Ok(GVal::Ref(r))
    }

    pub(crate) fn regex_start(
        &mut self,
        re: HeapRef,
        f: crate::regexmini::Schalter,
        len: usize,
    ) -> Result<Option<usize>, Stoerung> {
        if f.global || f.klebrig {
            let li = self.eigene_lese(re, "lastIndex").unwrap_or(GVal::Zahl(0.0));
            // Release 15.10.2026: ToLength, and a lastIndex past the end
            // fails (22.2.7.2 step 13.a: lastIndex 0, null). MEASURED before:
            // `r = /$/g; r.lastIndex = 5; r.exec('ab')` matched at 2 (null).
            let n = self.zu_zahl(li)?;
            let n = if n.is_nan() || n <= 0.0 { 0.0 } else { n.trunc() };
            if n > len as f64 {
                return Ok(None);
            }
            Ok(Some(n as usize))
        } else {
            Ok(Some(0))
        }
    }

    pub(crate) fn regex_last_index_setzen(
        &mut self,
        re: HeapRef,
        f: crate::regexmini::Schalter,
        wert: usize,
    ) {
        if f.global || f.klebrig {
            self.setze_eigen(re, "lastIndex", GVal::Zahl(wert as f64));
        }
    }

    pub(crate) fn n_regex_test(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        // Release 15.10.2026: RegExpExec (22.2.6.16): an own or inherited
        // user `exec` decides. MEASURED before: `r.exec = () => null;
        // r.test('x')` was true (node 22: false).
        let schnell = match this {
            GVal::Ref(r) => self.es_regex_schnell(r)?,
            _ => false,
        };
        if !schnell && self.es_ist_objekt(this) {
            let s = crate::eingebaut_text::zu_text_kein_symbol(self, self.arg(args, 0))?;
            let sv = self.text_wert(&s).map_err(Stoerung::Fehler)?;
            let marke = self.wurzel_merke();
            self.wurzeln.push(sv);
            let m = self.es_regexp_exec(this, sv);
            self.wurzel_zurueck(marke);
            return Ok(GVal::Bool(!matches!(m?, GVal::Null)));
        }
        let re = match this {
            GVal::Ref(r) if self.ist_regex(this) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "test an Nicht-RegExp"),
                ));
            }
        };
        let s = self.zu_text(self.arg(args, 0))?;
        let (muster, schalter) = self.regex_daten(GVal::Ref(re))?;
        let (knoten, gruppen_n, _namen) = crate::regexmini::kompilieren_mit_flags(&muster, &schalter)
            .map_err(|m| Stoerung::Wurf(self.fehler_wert("SyntaxError", &m)))?;
        let f = crate::regexmini::schalter_aus(&schalter).unwrap();
        let einheiten_text = crate::regexmini::als_einheiten(&s);
        let Some(start) = self.regex_start(re, f, einheiten_text.len())? else {
            self.regex_last_index_setzen(re, f, 0);
            return Ok(GVal::Bool(false));
        };
        // Unicode-Modus (Codepoint-Semantik) bei `u` oder `v` Flag.
        let unicode_modus = schalter.contains('u') || schalter.contains('v');
        match crate::regexmini::suchen_einheiten(&knoten, gruppen_n, f, unicode_modus, &einheiten_text, start) {
            Some(t) => {
                self.regex_last_index_setzen(re, f, t.ende);
                Ok(GVal::Bool(true))
            }
            None => {
                self.regex_last_index_setzen(re, f, 0);
                Ok(GVal::Bool(false))
            }
        }
    }

    pub(crate) fn n_regex_exec(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let re = match this {
            GVal::Ref(r) if self.ist_regex(this) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "exec an Nicht-RegExp"),
                ));
            }
        };
        let s = self.zu_text(self.arg(args, 0))?;
        let (muster, schalter) = self.regex_daten(GVal::Ref(re))?;
        let (knoten, gruppen_n, namen) = crate::regexmini::kompilieren_mit_flags(&muster, &schalter)
            .map_err(|m| Stoerung::Wurf(self.fehler_wert("SyntaxError", &m)))?;
        let f = crate::regexmini::schalter_aus(&schalter).unwrap();
        let einheiten_text = crate::regexmini::als_einheiten(&s);
        let Some(start) = self.regex_start(re, f, einheiten_text.len())? else {
            self.regex_last_index_setzen(re, f, 0);
            return Ok(GVal::Null);
        };
        // Unicode-Modus (Codepoint-Semantik) bei `u` oder `v` Flag.
        let unicode_modus = schalter.contains('u') || schalter.contains('v');
        match crate::regexmini::suchen_einheiten(&knoten, gruppen_n, f, unicode_modus, &einheiten_text, start) {
            Some(t) => {
                self.regex_last_index_setzen(re, f, t.ende);
                let erg = self.treffer_feld(&s, &einheiten_text, &t, &namen)?;
                // Release 15.10.2026: `d` adds `indices` (see `treffer_indices`).
                if schalter.contains('d') {
                    self.treffer_indices(erg, &t, &namen)?;
                }
                Ok(erg)
            }
            None => {
                self.regex_last_index_setzen(re, f, 0);
                Ok(GVal::Null)
            }
        }
    }

    pub(crate) fn n_regex_text(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (muster, schalter) = match this {
            GVal::Ref(_) if self.ist_regex(this) => self.regex_daten(this)?,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "toString an Nicht-RegExp"),
                ));
            }
        };
        // ECMA-262 22.2.6.3: `"/" + source + "/" + flags` — mit
        // maskiertem Schraegstrich und Schaltern in der Reihenfolge
        // `dgimsuvy`. Vorher stand hier das rohe Muster, also lieferte
        // `String(/a\/b/g)` `"/a/b/g"`.
        //
        // GEMESSEN 03.10. (Basislinie `T262_NUR=RegExp`: 33/433).
        let q = crate::eingebaut_regexp::quelle_text(&muster, &schalter);
        self.text_wert(&q).map_err(Stoerung::Fehler)
    }

    // --- RegExp prototype getters (ECMA-262 22.2.6) ---
    // source, flags, global, ignoreCase, multiline, dotAll, unicode, sticky,
    // hasIndices (ES2022), unicodeSets (ES2024)

    pub(crate) fn n_regex_source(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (muster, _schalter) = self.regex_daten(this)?;
        self.text_wert(&crate::eingebaut_regexp::quelle(&muster))
            .map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_regex_flags(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_muster, schalter) = self.regex_daten(this)?;
        let flags = crate::eingebaut_regexp::schalter_normale(&schalter);
        self.text_wert(&flags).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_regex_global(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_muster, schalter) = self.regex_daten(this)?;
        let f = crate::regexmini::schalter_aus(&schalter).unwrap();
        Ok(GVal::Bool(f.global))
    }

    pub(crate) fn n_regex_ignore_case(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_muster, schalter) = self.regex_daten(this)?;
        let f = crate::regexmini::schalter_aus(&schalter).unwrap();
        Ok(GVal::Bool(f.egal_gross))
    }

    pub(crate) fn n_regex_multiline(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_muster, schalter) = self.regex_daten(this)?;
        let f = crate::regexmini::schalter_aus(&schalter).unwrap();
        Ok(GVal::Bool(f.mehrzeilig))
    }

    pub(crate) fn n_regex_dot_all(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_muster, schalter) = self.regex_daten(this)?;
        let f = crate::regexmini::schalter_aus(&schalter).unwrap();
        Ok(GVal::Bool(f.punkt_alles))
    }

    pub(crate) fn n_regex_unicode(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_muster, schalter) = self.regex_daten(this)?;
        // Release 15.10.2026: the getter reports the `u` flag only
        // (ECMA-262 22.2.6.17); `v` has its own getter. MEASURED before:
        // `new RegExp('a', 'v').unicode` was true (node 22: false).
        Ok(GVal::Bool(schalter.contains('u')))
    }

    pub(crate) fn n_regex_sticky(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_muster, schalter) = self.regex_daten(this)?;
        let f = crate::regexmini::schalter_aus(&schalter).unwrap();
        Ok(GVal::Bool(f.klebrig))
    }

    pub(crate) fn n_regex_has_indices(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_muster, schalter) = self.regex_daten(this)?;
        Ok(GVal::Bool(schalter.contains('d')))
    }

    pub(crate) fn n_regex_unicode_sets(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_muster, schalter) = self.regex_daten(this)?;
        Ok(GVal::Bool(schalter.contains('v')))
    }

    // ---------------------------------------------------------------
    // console
    // ---------------------------------------------------------------

    pub(crate) fn n_konsole(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let mut teile = Vec::new();
        for a in args {
            teile.push(self.konsolen_format(*a, 0)?);
        }
        let zeile = teile.join(" ");
        // M6: Puffer gedeckelt (10.000 Zeilen, aelteste fallen weg).
        if self.konsole.len() >= 10_000 {
            self.konsole.remove(0);
        }
        self.konsole.push(zeile);
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn konsolen_format(&mut self, v: GVal, tiefe: u32) -> Result<String, Stoerung> {
        if tiefe > 4 {
            return Ok("[...]".to_string());
        }
        match v {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                // Top-Level-Strings roh (wie node); verschachtelt zitiert.
                HObj::Zeichenkette(s) => Ok(s),
                HObj::Feld { elemente, .. } => {
                    if elemente.is_empty() {
                        return Ok("[]".to_string());
                    }
                    let mut t = Vec::new();
                    for e in elemente.iter().take(100) {
                        t.push(self.inspekt(*e, tiefe + 1)?);
                    }
                    Ok(format!("[ {} ]", t.join(", ")))
                }
                HObj::Gegenstand { eigenschaften, .. } => {
                    if eigenschaften.is_empty() {
                        return Ok("{}".to_string());
                    }
                    let mut keys: Vec<&String> = eigenschaften.keys().collect();
                    keys.sort();
                    let mut t = Vec::new();
                    for k in keys.iter().take(100) {
                        let e = &eigenschaften[*k];
                        t.push(format!(
                            "{}: {}",
                            inspekt_schluessel(k),
                            self.inspekt(e.wert, tiefe + 1)?
                        ));
                    }
                    Ok(format!("{{ {} }}", t.join(", ")))
                }
                HObj::Fehler {
                    name, nachricht, ..
                } => Ok(format!("{name}: {nachricht}")),
                HObj::Funktion { name, .. } | HObj::Nativ { name, .. } => {
                    // J17: eine **anonyme** Funktion zeigt node als
                    // `[Function (anonymous)]` — vorher stand dort
                    // `[Function: ]` (mit leerem Namen).
                    if name.is_empty() {
                        Ok("[Function (anonymous)]".to_string())
                    } else {
                        Ok(format!("[Function: {name}]"))
                    }
                }
                // J17: Karte/Menge wie in node: `Map(1) { 1 => 'a' }`,
                // `Set(2) { 1, 2 }`. GEMESSEN vorher: `[object Map]` /
                // `[object Set]` — das ist die `toString`-Form, nicht die
                // Konsolen-Form. Fuer die Entwickler-Konsole eines
                // Browsers ist die Form mit Inhalt die nuetzliche.
                HObj::Karte { eintraege, .. } => {
                    if eintraege.is_empty() {
                        return Ok("Map(0) {}".to_string());
                    }
                    let mut t = Vec::with_capacity(eintraege.len().min(100));
                    for (k, w) in eintraege.iter().take(100) {
                        t.push(format!(
                            "{} => {}",
                            self.inspekt(*k, tiefe + 1)?,
                            self.inspekt(*w, tiefe + 1)?
                        ));
                    }
                    Ok(format!("Map({}) {{ {} }}", eintraege.len(), t.join(", ")))
                }
                HObj::Menge { eintraege, .. } => {
                    if eintraege.is_empty() {
                        return Ok("Set(0) {}".to_string());
                    }
                    let mut t = Vec::with_capacity(eintraege.len().min(100));
                    for w in eintraege.iter().take(100) {
                        t.push(self.inspekt(*w, tiefe + 1)?);
                    }
                    Ok(format!("Set({}) {{ {} }}", eintraege.len(), t.join(", ")))
                }
                // J17: node druckt ein Datum als **ISO**-Text
                // (`1970-01-01T00:00:00.000Z`), nicht als `Thu Jan 01
                // 1970 00:00:00 GMT+0000 (UTC)` — das ist die Form, die
                // auch `String(new Date(0))` liefert.
                HObj::Datum { millis, .. } => Ok(datum_iso(millis)),
                HObj::Regex {
                    muster, schalter, ..
                } => Ok(format!("/{muster}/{schalter}")),
                _ => Ok(self.zu_text(v)?),
            },
            // J5: `console.log` druckt die **negative Null** als `-0`.
            // GEMESSEN: `console.log(-0)` lieferte `0` (der Wert war richtig —
            // `Object.is(-0, 0)` ist false — nur die Ausgabe war falsch).
            // `String(-0)` bleibt `"0"` (JS-Regel), deshalb passiert das
            // nur hier und nicht in `zahl_text`.
            GVal::Zahl(n) if n == 0.0 && n.is_sign_negative() => Ok("-0".to_string()),
            // J24: GEMESSEN (node 22): `console.log(2n)` druckt `2n` —
            // die Inspektions-Form traegt das `n`, `String(2n)` nicht.
            GVal::Ganz(g) => Ok(format!("{g}n")),
            _ => Ok(self.zu_text(v)?),
        }
    }

    /// Verschachtelter Wert im node-inspect-Stil (Strings zitiert).
    pub(crate) fn inspekt(&mut self, v: GVal, tiefe: u32) -> Result<String, Stoerung> {
        if tiefe > 4 {
            return Ok("[...]".to_string());
        }
        match v {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Zeichenkette(s) => Ok(inspekt_text(&s)),
                HObj::Feld { elemente, .. } => {
                    if elemente.is_empty() {
                        return Ok("[]".to_string());
                    }
                    let mut t = Vec::new();
                    for e in elemente.iter().take(100) {
                        t.push(self.inspekt(*e, tiefe + 1)?);
                    }
                    Ok(format!("[ {} ]", t.join(", ")))
                }
                HObj::Gegenstand { eigenschaften, .. } => {
                    if eigenschaften.is_empty() {
                        return Ok("{}".to_string());
                    }
                    let mut keys: Vec<&String> = eigenschaften.keys().collect();
                    keys.sort();
                    let mut t = Vec::new();
                    for k in keys.iter().take(100) {
                        let e = &eigenschaften[*k];
                        t.push(format!(
                            "{}: {}",
                            inspekt_schluessel(k),
                            self.inspekt(e.wert, tiefe + 1)?
                        ));
                    }
                    Ok(format!("{{ {} }}", t.join(", ")))
                }
                _ => self.konsolen_format(v, tiefe + 1),
            },
            _ => self.konsolen_format(v, tiefe + 1),
        }
    }
}

/// String im node-inspect-Stil zitieren (einfache Anfuehrung).
fn inspekt_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            '\u{000B}' => out.push_str("\\v"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7F => {
                out.push_str(&format!("\\u{c:04x}", c = c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Objektschluessel im node-Stil (Bezeichner roh, Rest zitiert).
fn inspekt_schluessel(k: &str) -> String {
    let mut zeichen = k.chars();
    let ok = match zeichen.next() {
        Some(c) if c == '_' || c == '$' || c.is_alphabetic() => {
            zeichen.all(|c| c == '_' || c == '$' || c.is_alphanumeric())
        }
        _ => false,
    };
    if ok && !k.is_empty() {
        k.to_string()
    } else {
        inspekt_text(k)
    }
}

/// SameValueZero (Map/Set): NaN gleich, Strings nach Inhalt, sonst
/// Identitaet. Ref-Gleichheit allein wuerde `'a'` nie wiederfinden (M7).
fn same_value_zero(a: GVal, b: GVal) -> bool {
    match (a, b) {
        (GVal::Zahl(x), GVal::Zahl(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => a == b,
    }
}

/// Schluessel-Ansicht fuer Map/Set (owned, daher borrow-frei vergleichbar).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SchluesselAnsicht {
    Zahl(f64),
    /// JS2 (03.10.2026, 10:35): `i128`, **kein** `f64`. `1n` und `1` sind
    /// nach `SameValueZero` verschiedene Schluessel, und **eine** Zahl im
    /// `f64`-Feld wuerde sie gleichmachen — genau der Fehler, den der `Ganz`-
    /// Arm in `schluessel_gleich` verhindert.
    Ganz(i128),
    Bool(bool),
    Text(String),
    Sonst(GVal),
}

/// Ansicht bauen (Heap nur lesend, kurz).
pub(crate) fn schluessel_ansicht(heap: &Heap, v: GVal) -> SchluesselAnsicht {
    match v {
        GVal::Zahl(n) => SchluesselAnsicht::Zahl(n),
        GVal::Ganz(g) => SchluesselAnsicht::Ganz(g),
        GVal::Bool(b) => SchluesselAnsicht::Bool(b),
        GVal::Ref(r) => match heap.holen(r) {
            HObj::Zeichenkette(s) => SchluesselAnsicht::Text(s.clone()),
            _ => SchluesselAnsicht::Sonst(v),
        },
        _ => SchluesselAnsicht::Sonst(v),
    }
}

/// J8: Schluessel-Vergleich **ohne** Zwischenkopie (SameValueZero +
/// String-Inhalt).
///
/// GEMESSEN, warum das noetig war: `Map.set/get/has/delete` suchten mit
/// `schluessel_gleich_ansicht(&schluessel_ansicht(heap, k), …)` — und
/// `schluessel_ansicht` **klont** den Schluesseltext. Ein `set` auf einer
/// Karte mit n Textschluesseln kostete damit n Textkopien; 10^5 `set`
/// ueber eine wachsende Karte brauchten die Zeitgrenze (GEMESSEN: 6800
/// ns/Schritt, `tests/j8_leistung.rs` Haenger). Jetzt wird direkt
/// verglichen: Zahl/Bool nach Wert, Texte nach Inhalt, sonst nach
/// `GVal`-Gleichheit (Referenz auf dasselbe Objekt).
pub(crate) fn schluessel_gleich(heap: &Heap, a: GVal, b: GVal) -> bool {
    match (a, b) {
        (GVal::Zahl(x), GVal::Zahl(y)) => x == y || (x.is_nan() && y.is_nan()),
        (GVal::Bool(x), GVal::Bool(y)) => x == y,
        (GVal::Undefiniert, GVal::Undefiniert) => true,
        (GVal::Null, GVal::Null) => true,
        // ## JS2 (03.10.2026, 10:35): DER `Ganz`-ARM — der dritte Wunsch von
        // 23:32, und diesmal in **meiner** Datei.
        //
        // GEMESSEN (10:35), `node 22`:
        //
        // ```js
        // var m = new Map(); m.set(1n,'a'); m.get(1n)      node: 'a'   bei uns: undefined
        // var m = new Map(); m.set(1n,'a'); m.has(1n)      node: true  bei uns: false
        // var m = new Map(); m.set(1n,'a'); m.delete(1n)   node: true  bei uns: false
        // var m = new Map(); m.set(1n,'a'); m.set(1n,'b'); m.size
        //                                                     node: 1     bei uns: 2
        // var s = new Set(); s.add(1n); s.has(1n)          node: true  bei uns: false
        // var s = new Set(); s.add(1n); s.delete(1n)       node: true  bei uns: false
        // ```
        //
        // **DER HASH WAR RICHTIG** — `heap.rs:187` hat einen `Ganz`-Zweig, und
        // `m.set(1n,'a'); m.set(1n,'b')` legte **zwei** Elemente an, nicht
        // eines: der Hash von `1n` stimmte, also fand der zweite `set` **kein**
        // vorhandenes Element und haengte an. **Der Schlüssel wurde also
        // gefunden und trotzdem nicht als gleich erkannt.**
        //
        // **UND DAS IST GENAU DIE FORM, DIE MAN NICHT SUCHT:** ein fehlender
        // `match`-Arm sieht aus wie „dieser Typ wird nicht unterstützt" und
        // nicht wie „dieser Typ wird **gespeichert** und nie **wiedergefunden**".
        // Die Karte ist nicht leer, `size` stimmt sogar beim Konstruktor —
        // nur ist jedes Element **unerreichbar**.
        //
        // **UND DIE GEGENREGEL, DIE MAN VERLIERT, WENN MAN NUR DEN ARM
        // HINZUFÜGT:** `1n` ist **kein** Schluessel `1`. Nach `SameValueZero`
        // sind die Zahlen `1` und `1n` verschieden (verschiedene Typen), und
        // `heap.rs` hat dafuer **auch** getrennte Hashes. Die Regel steht als
        // eigener Test daneben (`ganz_als_schluessel`, jetzt **nicht** mehr
        // `#[ignore]`), weil der Arm `1n == 1` **nicht** macht — der fehlende
        // `_`-Arm macht es.
        (GVal::Ganz(x), GVal::Ganz(y)) => x == y,
        (GVal::Ref(x), GVal::Ref(y)) => {
            if x == y {
                return true;
            }
            match (heap.holen(x), heap.holen(y)) {
                (HObj::Zeichenkette(sa), HObj::Zeichenkette(sb)) => sa == sb,
                _ => false,
            }
        }
        _ => false,
    }
}

/// Schluessel-Gleichheit (SameValueZero + String-Inhalt).
pub(crate) fn schluessel_gleich_ansicht(a: &SchluesselAnsicht, b: &SchluesselAnsicht) -> bool {
    match (a, b) {
        (SchluesselAnsicht::Zahl(x), SchluesselAnsicht::Zahl(y)) => {
            x == y || (x.is_nan() && y.is_nan())
        }
        // Siehe `schluessel_gleich`: `1n` und `1` sind verschiedene Schluessel.
        (SchluesselAnsicht::Ganz(x), SchluesselAnsicht::Ganz(y)) => x == y,
        (SchluesselAnsicht::Bool(x), SchluesselAnsicht::Bool(y)) => x == y,
        (SchluesselAnsicht::Text(x), SchluesselAnsicht::Text(y)) => x == y,
        (SchluesselAnsicht::Sonst(x), SchluesselAnsicht::Sonst(y)) => x == y,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// JSON-Leser (strikt, mit Tiefen-/Laengen-Deckel)
// ---------------------------------------------------------------------------

struct JLeser {
    zeichen: Vec<char>,
    pos: usize,
    tiefe: u32,
    max_tiefe: u32,
}

impl JLeser {
    /// JSON-Leerraum — **vier** Zeichen, nicht die Unicode-Eigenschaft.
    ///
    /// JS2 (02.10.2026, 00:05): `char::is_whitespace()` ist `White_Space` und
    /// laesst elf Zeichen durch, die JSON verbietet (U+000B, U+000C, U+00A0,
    /// U+1680, U+2000, U+2009, U+200A, U+2028, U+2029, U+202F, U+205F, U+3000).
    /// GEMESSEN, 5 rote test262-Faelle in `built-ins/JSON/parse` — alle mit
    /// „Expected a SyntaxError to be thrown but no exception was thrown at
    /// all". `node 22` sagt zu jedem der elf `SyntaxError`.
    ///
    /// DIE REGEL STEHT NICHT HIER, sondern in `eingebaut_json.rs` (meine
    /// Datei) mit drei Unit-Tests; hier wird sie gelesen. Grund: dieselbe
    /// Frage wird an drei Stellen gebraucht (hier, im `raw`-Leser, im
    /// Schreibweg), und drei Abschriften sind drei Fehlerchancen.
    ///
    /// GETEILTE DATEI: eine Zeile, gemeldet in `team/WUNSCH_JS.txt`.
    pub(crate) fn leerraum(&mut self) {
        while self.pos < self.zeichen.len()
            && crate::eingebaut_json::ist_json_leerraum(self.zeichen[self.pos])
        {
            self.pos += 1;
        }
    }

    pub(crate) fn fehler(&self, msg: &str) -> JsFehler {
        JsFehler::Laufzeit(format!("JSON: {msg}"))
    }

    pub(crate) fn wert(&mut self, e: &mut Engine) -> Result<GVal, Stoerung> {
        self.tiefe += 1;
        if self.tiefe > e.grenzen.max_json_tiefe.max(8) {
            return Err(Stoerung::Wurf(e.fehler_wert("SyntaxError", "JSON zu tief")));
        }
        self.leerraum();
        if self.pos >= self.zeichen.len() {
            return Err(Stoerung::Fehler(self.fehler("Eingabe endet")));
        }
        let c = self.zeichen[self.pos];
        let v = match c {
            '{' => self.objekt(e)?,
            '[' => self.feld(e)?,
            '"' => {
                let s = self.zeichenkette()?;
                e.text_wert(&s).map_err(Stoerung::Fehler)?
            }
            't' => {
                self.wort("true")?;
                GVal::Bool(true)
            }
            'f' => {
                self.wort("false")?;
                GVal::Bool(false)
            }
            'n' => {
                self.wort("null")?;
                GVal::Null
            }
            '-' | '0'..='9' => {
                let n = self.zahl()?;
                GVal::Zahl(n)
            }
            _ => {
                return Err(Stoerung::Wurf(
                    e.fehler_wert("SyntaxError", "JSON: unerwartetes Zeichen"),
                ));
            }
        };
        self.tiefe -= 1;
        Ok(v)
    }

    pub(crate) fn wort(&mut self, w: &str) -> Result<(), Stoerung> {
        for c in w.chars() {
            if self.pos >= self.zeichen.len() || self.zeichen[self.pos] != c {
                return Err(Stoerung::Fehler(self.fehler("ungueltiges Literal")));
            }
            self.pos += 1;
        }
        Ok(())
    }

    pub(crate) fn objekt(&mut self, e: &mut Engine) -> Result<GVal, Stoerung> {
        self.pos += 1; // {
        let o = e
            .neues_objekt(Some(e.proto_objekt))
            .map_err(Stoerung::Fehler)?;
        e.wurzeln.push(GVal::Ref(o));
        self.leerraum();
        if self.verbrauche('}') {
            return Ok(GVal::Ref(o));
        }
        loop {
            self.leerraum();
            if self.pos >= self.zeichen.len() || self.zeichen[self.pos] != '"' {
                e.wurzeln.pop();
                return Err(Stoerung::Wurf(
                    e.fehler_wert("SyntaxError", "JSON: Schluessel kein String"),
                ));
            }
            let k = self.zeichenkette()?;
            self.leerraum();
            if !self.verbrauche(':') {
                e.wurzeln.pop();
                return Err(Stoerung::Wurf(
                    e.fehler_wert("SyntaxError", "JSON: ':' fehlt"),
                ));
            }
            let v = self.wert(e)?;
            e.wurzeln.push(v);
            // Release 15.10.2026: CreateDataProperty, not [[Set]]
            // (ECMA-262 25.5.1.1 via InternalizeJSONProperty/ParseJSON): an
            // Object.prototype setter must not see the member, and
            // "__proto__" is an own key. MEASURED before: a setter on
            // Object.prototype swallowed `JSON.parse('{"zz":1}').zz`.
            e.create_data_property_or_throw(o, &k, v)?;
            self.leerraum();
            if self.verbrauche('}') {
                break;
            }
            if !self.verbrauche(',') {
                e.wurzeln.pop();
                return Err(Stoerung::Wurf(
                    e.fehler_wert("SyntaxError", "JSON: ','/'}' fehlt"),
                ));
            }
            // Nachgestelltes Komma ist ungueltig (strikt).
            self.leerraum();
            if self.pos < self.zeichen.len() && self.zeichen[self.pos] == '}' {
                e.wurzeln.pop();
                return Err(Stoerung::Wurf(
                    e.fehler_wert("SyntaxError", "JSON: haengendes Komma"),
                ));
            }
        }
        Ok(GVal::Ref(o))
    }

    pub(crate) fn feld(&mut self, e: &mut Engine) -> Result<GVal, Stoerung> {
        self.pos += 1; // [
        let f = e.neues_feld().map_err(Stoerung::Fehler)?;
        e.wurzeln.push(GVal::Ref(f));
        self.leerraum();
        if self.verbrauche(']') {
            return Ok(GVal::Ref(f));
        }
        loop {
            let v = self.wert(e)?;
            e.wurzeln.push(v);
            let oo = e.heap.holen_mut(f);
            if let HObj::Feld { elemente, .. } = oo {
                if elemente.len() > 1_000_000 {
                    return Err(Stoerung::Fehler(JsFehler::Speicher(
                        "JSON-Feld zu gross".to_string(),
                    )));
                }
                elemente.push(v);
            }
            e.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            self.leerraum();
            if self.verbrauche(']') {
                break;
            }
            if !self.verbrauche(',') {
                return Err(Stoerung::Wurf(
                    e.fehler_wert("SyntaxError", "JSON: ','/']' fehlt"),
                ));
            }
            self.leerraum();
            if self.pos < self.zeichen.len() && self.zeichen[self.pos] == ']' {
                return Err(Stoerung::Wurf(
                    e.fehler_wert("SyntaxError", "JSON: haengendes Komma"),
                ));
            }
        }
        Ok(GVal::Ref(f))
    }

    pub(crate) fn verbrauche(&mut self, c: char) -> bool {
        if self.pos < self.zeichen.len() && self.zeichen[self.pos] == c {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub(crate) fn zeichenkette(&mut self) -> Result<String, Stoerung> {
        // Annahme: pos zeigt auf oeffnendes `"`.
        self.pos += 1;
        let mut out = String::new();
        loop {
            if self.pos >= self.zeichen.len() {
                return Err(Stoerung::Fehler(self.fehler("String endet nicht")));
            }
            let c = self.zeichen[self.pos];
            self.pos += 1;
            match c {
                '"' => break,
                '\\' => {
                    if self.pos >= self.zeichen.len() {
                        return Err(Stoerung::Fehler(self.fehler("Escape endet")));
                    }
                    let e = self.zeichen[self.pos];
                    self.pos += 1;
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000C}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let h = self.hex4()?;
                            // Surrogatpaare zusammenfuehren.
                            if (0xD800..0xDC00).contains(&h)
                                && self.pos + 1 < self.zeichen.len()
                                && self.zeichen[self.pos] == '\\'
                                && self.zeichen[self.pos + 1] == 'u'
                            {
                                let merk = self.pos;
                                self.pos += 2;
                                if let Ok(lo) = self.hex4() {
                                    if (0xDC00..0xE000).contains(&lo) {
                                        let cp = 0x10000 + ((h - 0xD800) << 10) + (lo - 0xDC00);
                                        out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                                    } else {
                                        self.pos = merk;
                                        out.push(char::from_u32(h).unwrap_or('\u{FFFD}'));
                                    }
                                } else {
                                    self.pos = merk;
                                    out.push(char::from_u32(h).unwrap_or('\u{FFFD}'));
                                }
                            } else {
                                out.push(char::from_u32(h).unwrap_or('\u{FFFD}'));
                            }
                        }
                        _ => return Err(Stoerung::Fehler(self.fehler("ungueltiges Escape"))),
                    }
                }
                c if (c as u32) < 0x20 => {
                    return Err(Stoerung::Fehler(self.fehler("Steuerzeichen im String")));
                }
                c => out.push(c),
            }
            if out.len() > 64 * 1024 * 1024 {
                return Err(Stoerung::Fehler(JsFehler::Speicher(
                    "JSON-String zu lang".to_string(),
                )));
            }
        }
        Ok(out)
    }

    pub(crate) fn hex4(&mut self) -> Result<u32, Stoerung> {
        if self.pos + 4 > self.zeichen.len() {
            return Err(Stoerung::Fehler(self.fehler("\\u unvollstaendig")));
        }
        let mut w: u32 = 0;
        for i in 0..4 {
            match self.zeichen[self.pos + i].to_digit(16) {
                Some(d) => w = w * 16 + d,
                None => return Err(Stoerung::Fehler(self.fehler("ungueltiges \\u"))),
            }
        }
        self.pos += 4;
        Ok(w)
    }

    pub(crate) fn zahl(&mut self) -> Result<f64, Stoerung> {
        let start = self.pos;
        if self.verbrauche('-') {
            // ok
        }
        if self.pos >= self.zeichen.len() {
            return Err(Stoerung::Fehler(self.fehler("Zahl endet")));
        }
        if self.zeichen[self.pos] == '0' {
            self.pos += 1;
        } else if self.zeichen[self.pos].is_ascii_digit() {
            while self.pos < self.zeichen.len() && self.zeichen[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        } else {
            return Err(Stoerung::Fehler(self.fehler("Zahl erwartet")));
        }
        if self.pos < self.zeichen.len() && self.zeichen[self.pos] == '.' {
            self.pos += 1;
            if self.pos >= self.zeichen.len() || !self.zeichen[self.pos].is_ascii_digit() {
                return Err(Stoerung::Fehler(self.fehler("Nachkomma fehlt")));
            }
            while self.pos < self.zeichen.len() && self.zeichen[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if self.pos < self.zeichen.len()
            && (self.zeichen[self.pos] == 'e' || self.zeichen[self.pos] == 'E')
        {
            self.pos += 1;
            if self.pos < self.zeichen.len()
                && (self.zeichen[self.pos] == '+' || self.zeichen[self.pos] == '-')
            {
                self.pos += 1;
            }
            if self.pos >= self.zeichen.len() || !self.zeichen[self.pos].is_ascii_digit() {
                return Err(Stoerung::Fehler(self.fehler("Exponent fehlt")));
            }
            while self.pos < self.zeichen.len() && self.zeichen[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let s: String = self.zeichen[start..self.pos].iter().collect();
        s.parse::<f64>()
            .map_err(|_| Stoerung::Fehler(self.fehler("Zahl unlesbar")))
    }
}

// ---------------------------------------------------------------------------
// JSON-Schreiber
// ---------------------------------------------------------------------------

struct JSchreiber {
    ersatz: Option<GVal>,
    erlaubnis: Option<HashSet<String>>,
    /// JS2/QUIC (03.10.2026, 01:15): die **Reihenfolge** von
    /// `PropertyList` — siehe `JSON.stringify` (W21).
    reihenfolge: Option<Vec<String>>,
    einrueck: String,
    stapel: Vec<u32>,
    max_tiefe: u32,
}

impl JSchreiber {
    pub(crate) fn serial(
        &mut self,
        e: &mut Engine,
        v: GVal,
        schluessel: Option<String>,
        halter: GVal,
        tiefe: u32,
    ) -> Result<Option<String>, Stoerung> {
        if tiefe > self.max_tiefe.max(8) {
            return Err(Stoerung::Wurf(e.fehler_wert("TypeError", "JSON zu tief")));
        }
        // ## JS2/QUIC (03.10.2026, 02:45): ein **Symbol** wird **nichts
        // geschrieben**.
        //
        // ECMA-262 25.5.4.5 `SerializeJSONProperty` Schritt 5: *„Return
        // undefined if Type(value) is Undefined or Type(value) is Symbol or
        // Type(value) is Number and value is NaN."*
        //
        // **UND — der Punkt, der erst die Stellung bestimmt** — Schritt 2 sagt
        // *„If Type(value) is Object **or BigInt**"*. Ein Symbol ist
        // **weder** noch, also wird `toJSON` **nicht** geholt und **nicht**
        // gerufen.
        //
        // **GEMESSEN, `node 22`, und der Vorher-Zustand daneben (Gegenprobe,
        // Block entfernt, gleicher Codestand sonst):**
        // ```js
        // JSON.stringify(Symbol('a'))                 node: undefined   ->  {}
        // Symbol.prototype.toJSON = f; Stringify(sym)  node: undefined   ->  1
        // JSON.stringify({s: Symbol('a')})             node: {}          ->  {"s":{}}
        // JSON.stringify([Symbol('a'), 1])             node: [null,1]    ->  [{},1]
        // JSON.stringify({a:1, b: Symbol('c')})       node: {"a":1}     ->  {"a":1,"b":{}}
        // JSON.stringify(Symbol.iterator)              node: undefined   ->  {}
        // ```
        // **Vorher war die Reihenfolge genau verkehrt herum:** unser Symbol ist
        // ein `GVal::Ref` (es hat **keinen** eigenen Heap-Typ — das ist W14/W16),
        // also griff der `toJSON`-Zweig, und `serial_nach_erzwung` behandelte
        // das Ding als **gewoehnliches** Objekt.
        //
        // **DESHALB GANZ OBEN, und nicht in `serial_nach_erzwung`:** dort
        // waere es zu **spaet** — der `toJSON`-Zweig in `serial` waere vorher
        // gelaufen, und `JSON.stringify(sym)` mit gesetztem `toJSON` waere `1`
        // statt `undefined`. Das ist die Reihenfolge der Norm, nicht Geschmack.
        //
        // **`Ok(None)`** ist hier richtig und nicht nur bequem: `None` heisst in
        // diesem Code „**nichts geschrieben**“ — dieselbe Bedeutung wie fuer
        // `GVal::Undefiniert`, fuer `Funktion | Nativ`, und **die** Bedeutung,
        // aus der ein **Feld** `[null,1]` macht.
        if let GVal::Ref(r) = v {
            if crate::symbole::ist_symbol(e, r) {
                return Ok(None);
            }
        }
        // toJSON ehren.
        let mut w = v;
        // ## BigInt GEHOERT AUCH HIERHER (JS2/QUIC, 18:35).
        //
        // ECMA-262 24.5.1.1 Schritt 2 sagt woertlich: „If Type(value) is
        // Object **or BigInt**". Unser `if let GVal::Ref(r) = v` unten hat
        // BigInt also **ausgeschlossen**, und `serial_nach_erzwung` wirft fuer
        // `GVal::Ganz` **sofort** — `BigInt.prototype.toJSON` war damit
        // **toter Code**, und der Fall, dass man es setzt, Wirkungs**los**.
        //
        // **GEMESSEN (`JSON/stringify/value-bigint-tojson.js`, `node 22`):**
        // ```js
        // BigInt.prototype.toJSON = function () { return this.toString(); };
        // JSON.stringify(0n)   node: '"0"'    bei uns (vorher): TypeError
        // ```
        //
        // **UND DER WURF BLEIBT.** `JSON.stringify(1n)` **ohne** `toJSON`
        // wirft weiterhin — die J24-Messung gilt unveraendert. Es kommt nur
        // der Weg **mit** `toJSON` durch. Das ist der Unterschied zwischen
        // „BigInt kann man nicht serialisieren“ und „BigInt **ohne**
        // `toJSON` kann man nicht serialisieren“.
        //
        // **WARUM `hole` UND NICHT `eigene_oder_proto`:** ein BigInt ist ein
        // Primitiv **ohne Heap-Slot** (`heap.rs`), und `eigene_oder_proto`
        // gibt fuer alles ausser `GVal::Ref` `Ok(None)` zurueck
        // (`eigenschaften.rs:583`). `Engine::hole` hat den **eigenen**
        // BigInt-Zweig ueber `proto_bigint` (`eigenschaften.rs:408`).
        if matches!(w, GVal::Ganz(_)) {
            // `Engine::hole` gibt `Result<GVal, Stoerung>` und liefert
            // `undefined` fuer „nicht vorhanden“ — also **kein** `Option`.
            let f = e.hole(w, "toJSON")?;
            if e.ist_aufrufbar(f) {
                let kt = match &schluessel {
                    Some(k) => e.text_wert(k).map_err(Stoerung::Fehler)?,
                    None => GVal::Undefiniert,
                };
                // M6: `toJSON` und sein `this` sind Wurzeln — der Aufruf darf
                // allozieren.
                let marke = e.wurzel_merke();
                e.wurzeln.push(f);
                e.wurzeln.push(w);
                let rr = e.aufruf_synchron(f, w, &[kt], false);
                e.wurzel_zurueck(marke);
                w = rr?;
            }
        }
        if let GVal::Ref(r) = v {
            if matches!(
                e.heap.holen(r),
                HObj::Gegenstand { .. }
                    | HObj::Feld { .. }
                    | HObj::Verpackt { .. }
                    | HObj::Datum { .. }
                    | HObj::Karte { .. }
                    | HObj::Menge { .. }
                    | HObj::Fehler { .. }
            ) {
                if let Some(f) = e.eigene_oder_proto(v, "toJSON")? {
                    if e.ist_aufrufbar(f) {
                        let kt = match &schluessel {
                            Some(k) => e.text_wert(k).map_err(Stoerung::Fehler)?,
                            None => GVal::Undefiniert,
                        };
                        let marke = e.wurzel_merke();
                        e.wurzeln.push(f);
                        e.wurzeln.push(v);
                        let rr = e.aufruf_synchron(f, v, &[kt], false);
                        e.wurzel_zurueck(marke);
                        w = rr?;
                    }
                }
            }
        }
        // Replacer-Funktion.
        if let Some(rf) = self.ersatz {
            let kt = match &schluessel {
                Some(k) => e.text_wert(k).map_err(Stoerung::Fehler)?,
                None => GVal::Undefiniert,
            };
            let marke = e.wurzel_merke();
            e.wurzeln.push(rf);
            e.wurzeln.push(w);
            let rr = e.aufruf_synchron(rf, halter, &[kt, w], false);
            e.wurzel_zurueck(marke);
            w = rr?;
        }
        self.serial_nach_erzwung(e, w, halter, tiefe)
    }

    /// Der Teil von `SerializeJSONProperty`, der **nach** `toJSON` und dem
    /// Replacer kommt: aus einem bereits geprueften `value` einen Text machen
    /// (ECMA-262 24.5.2.1 Schritt 4 bis 9, ohne 4).
    ///
    /// **WARUM DAS EIN EIGENER HELFER IST, und der Grund ist ein Fehler, den
    /// ich dabei gefunden habe (07:15):** vorher stand dieser Block am Ende
    /// von `serial`, und `serial_ref` rief fuer eine **Box**
    /// `self.serial(e, primitiv, None, v, tiefe + 1)` auf — also `serial`
    /// **ein zweites Mal**. Damit liefen `toJSON` **und der Replacer** noch
    /// einmal, mit `primitiv` statt mit der Box. Der Replacer wurde also
    /// **zweimal** gerufen, und das ist in `SerializeJSONProperty` genau einmal
    /// erlaubt.
    ///
    /// Mit dem Helfer ist der Weg: `toJSON` → Replacer → **hier** → und die
    /// Box wird in Schritt 4 (unten) entpackt und **gleich hier** hingesendet,
    /// also ohne den zweiten Durchlauf.
    ///
    /// M3: **keine** Allokation in diesem Helfer — der Text entsteht wie
    /// vorher mit `json_zahl`/`json_quote`. Die `wurzeln`-Klammer und der
    /// `stapel`-Eintrag fuer den Zyklusschutz sind unveraendert.
    fn serial_nach_erzwung(
        &mut self,
        e: &mut Engine,
        w: GVal,
        halter: GVal,
        tiefe: u32,
    ) -> Result<Option<String>, Stoerung> {
        match w {
            GVal::Undefiniert => Ok(None),
            GVal::Null => Ok(Some("null".to_string())),
            GVal::Bool(true) => Ok(Some("true".to_string())),
            GVal::Bool(false) => Ok(Some("false".to_string())),
            GVal::Zahl(n) => Ok(Some(if n.is_finite() {
                json_zahl(n)
            } else {
                "null".to_string()
            })),
            // J24: GEMESSEN (node): `JSON.stringify(2n)` ->
            // `TypeError: Do not know how to serialize a BigInt`.
            GVal::Ganz(_) => Err(Stoerung::Wurf(
                e.fehler_wert("TypeError", "Do not know how to serialize a BigInt"),
            )),
            GVal::Ref(r) => {
                if let HObj::Funktion { .. } | HObj::Nativ { .. } = e.heap.holen(r) {
                    return Ok(None);
                }
                // Release 15.10.2026: a JSON.rawJSON object writes its text.
                if e.eigene_lese(r, crate::es_modern::H_ROH_JSON).is_some() {
                    let t = e.eigene_lese(r, "rawJSON").unwrap_or(GVal::Undefiniert);
                    return Ok(Some(e.text_von(t)));
                }
                if self.stapel.contains(&r.0) {
                    return Err(Stoerung::Wurf(e.fehler_wert("TypeError", "JSON-Zyklus")));
                }
                self.stapel.push(r.0);
                let erg = self.serial_ref(e, GVal::Ref(r), halter, tiefe);
                self.stapel.pop();
                erg
            }
        }
    }

    /// **DER HALTER, und warum er jetzt durchgereicht wird:** Schritt 4 der
    /// Box-Entpackung unten braucht ihn fuer `ToNumber`/`ToString`, und der
    /// Aufruf von `serial_ref` muss ihn deshalb kennen. Vorher war er `v`
    /// (der Wert selbst) — und im Replacer-Zweig war er der Halter, im
    /// `Zeichenkette`-/`Feld`-Zweig wieder ein anderer. **Drei Halter an drei
    /// Stellen ist die Art, bei der sich kein Mensch etwas merkt.**
    pub(crate) fn serial_ref(
        &mut self,
        e: &mut Engine,
        v: GVal,
        halter: GVal,
        tiefe: u32,
    ) -> Result<Option<String>, Stoerung> {
        let r = match v {
            GVal::Ref(r) => r,
            _ => unreachable!(),
        };
        // Release 15.10.2026: an arguments object is serialized as an
        // ordinary object — through a plain copy of its enumerable own
        // properties (indices first), so the object branch below applies.
        if e.ist_argumente_objekt(r) {
            let kopie = e.argumente_als_objekt(r)?;
            let marke = e.wurzel_merke();
            e.wurzeln.push(kopie);
            let erg = self.serial_ref(e, kopie, halter, tiefe);
            e.wurzel_zurueck(marke);
            return erg;
        }
        match e.heap.holen(r).clone() {
            HObj::Zeichenkette(s) => Ok(Some(json_quote(&s))),
            // Release 15.10.2026: an arguments object serializes as an
            // object (SerializeJSONObject, IsArray is false).
            HObj::Feld { elemente, rest, .. } if !rest.contains_key(crate::builtin::ARGUMENTE_MARKE) => {
                let mut out = String::from("[");
                for (i, el) in elemente.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if !self.einrueck.is_empty() {
                        out.push('\n');
                        out.push_str(&self.einrueck.repeat((tiefe + 1) as usize));
                    }
                    match self.serial(e, *el, Some(i.to_string()), v, tiefe + 1)? {
                        Some(s) => out.push_str(&s),
                        None => out.push_str("null"),
                    }
                    e.string_grenze(out.len()).map_err(Stoerung::Fehler)?;
                }
                if !self.einrueck.is_empty() && !elemente.is_empty() {
                    out.push('\n');
                    out.push_str(&self.einrueck.repeat(tiefe as usize));
                }
                out.push(']');
                Ok(Some(out))
            }
            HObj::Gegenstand {
                eigenschaften,
                reihenfolge,
                ..
            } => {
                // GEMESSEN: Einfuegereihenfolge wie in node — vorher wurde
                // alphabetisch sortiert, und `JSON.stringify({value, done})`
                // lieferte `{"done","value"}` (p3.js Z4).
                let mut keys: Vec<String> = Vec::with_capacity(eigenschaften.len());
                for k in reihenfolge {
                    if eigenschaften.get(&k).is_some_and(|p| p.aufzählbar) && !keys.contains(&k) {
                        keys.push(k.clone());
                    }
                }
                // W38 (QUIC, 04.10.2026): **die Reste sortieren** statt in HashMap-Reihenfolge
                // anhaengen — dieselbe Sache wie in `eigenschaften.rs`
                // (`schluessel_werte`) und in `builtin.rs`
                // (`eigene_schluessel`); Begründung und Messung dort.
                //
                // **GERADE HIER IST ES SICHTBAR:** `JSON.stringify` gibt die
                // Schluessel in **Einfuegereihenfolge** aus (ECMA-262
                // 25.5.4.5 `SerializeJSONObject` Schritt 6), also war diese
                // Schleife eine **zufaellige** Reihenfolge im Ausgabetext —
                // und `JSON.stringify({a:1,b:2,c:3})` koennte je nach Prozess
                // anders lauten.
                let mut reste: Vec<String> = eigenschaften
                    .iter()
                    .filter(|(k, p)| p.aufzählbar && !keys.contains(k))
                    .map(|(k, _)| k.clone())
                    .collect();
                reste.sort();
                keys.extend(reste);
                // J11: private Felder (`#x`) sind keine eigenen Properties
                // — `JSON.stringify(new C())` gibt in node `{}` (GEMESSEN
                // mit node 22, `class C { #x = 1; y = 2 }`).
                keys.retain(|k| !crate::heap::ist_privat(k));
                // ## W56 — **UND DERSELBE SYMBOL-FILTER IM ZWEITEN ZWEIG.**
                // Der Fall `JSON/stringify/value-symbol.js` nimmt **diesen**
                // Zweig (Schluessel aus der Map, `reihenfolge` ist nicht
                // gesetzt), nicht den mit der PropertyList — und mein erster
                // Filter sass nur dort. **GEMESSEN:** `JSON/stringify/` blieb
                // bei 47 von 53 (88,7 %) und `value-symbol.js` rot.
                //
                // *Ein Filter an einer von zwei Stellen ist kein Filter, sondern
                // ein Zufall mit Bestaetigungsschein.*
                keys.retain(|k| !crate::heap::ist_symbolschluessel(k));
                // ## JS2/QUIC (03.10.2026, 01:15): **DIE REIHENFOLGE IST K**
                // (W21). Bis eben kam `keys` aus der Objektreihenfolge und
                // `erlaubnis` **filterte** nur.
                //
                // ECMA-262 25.5.4.5 `SerializeJSONObject` Schritt 5.b und 7:
                // ```text
                // 5.b  Let P be Get(state.PropertyList, ToString(ix)).
                // 7.   For each element P of K, in list order:
                // 8.     Let value be ? Get(V, P).
                // ```
                // Ist `PropertyList` gesetzt, **ist K die Liste selbst** — und
                // ihre Reihenfolge ist massgeblich, also **kein**
                // `ordne_schluessel` darueber (das ist der Unterschied zu
                // einem Feld, wo `6.1.7` gilt).
                if let Some(liste) = &self.reihenfolge {
                    keys = liste
                        .iter()
                        .filter(|k| {
                            !crate::heap::ist_privat(k)
                                // ## W56 (QUIC, 05.10.2026) — **EIN SYMBOL-
                                // SCHLUESSEL GEHOERT NICHT INS JSON.**
                                //
                                // ECMA-262 25.5.4.5 `SerializeJSONObject`
                                // Schritt 7: `K` kommt aus
                                // `EnumerableOwnPropertyNames(V, key)` — und das
                                // sind **nur String-Schluessel**. Die Symbol-
                                // Schluessel des Heap liegen als **Text**
                                // (`"Symbol(desc)"`, `"@sym<uid>"`), also **im
                                // selben Behaelter** und **ohne** Unterschied —
                                // dieser Filter ist die **einzige** Stelle, die
                                // sie trennen kann.
                                //
                                // **GEMESSEN, `node 22`:**
                                // ```js
                                // JSON.stringify({ [Symbol('desc')]: 1 })  ->  "{}"
                                // ```
                                // **Bei uns: `{"Symbol(desc)":1}`** — der Symbol-
                                // Schluessel kam als **Textschluessel** heraus
                                // (`JSON/stringify/value-symbol.js`).
                                //
                                // **UND DAS IST DIE ARBEIT, DIE ICH IN QUIC_31 ALS
                                // „0 FAELLE" GEMELDET HATTE** — weil
                                // `Object/getOwnPropertySymbols` und
                                // `Reflect/ownKeys` in der Auswahl mit **je 0
                                // Pfaden** stehen. *Eine Auswahl, die einen Ort
                                // nicht enthaelt, meldet nicht, dass der Ort
                                // fehlt — sie meldet nur, dass man ihn nicht
                                // sieht.* `ist_symbolschluessel` ist die
                                // vorhandene Form (`heap.rs`, zwei Formen), also
                                // **eine** Bedingung und keine neue.
                                && !crate::heap::ist_symbolschluessel(k)
                                && eigenschaften.get(*k).is_some_and(|p| p.aufzählbar)
                        })
                        .cloned()
                        .collect();
                } else {
                    // sec-6.1.7: Zahlschluessel zuerst aufsteigend (J5).
                    crate::heap::ordne_schluessel(&mut keys);
                }
                let mut out = String::from("{");
                let mut erste = true;
                for k in keys {
                    if let Some(erl) = &self.erlaubnis {
                        if !erl.contains(&k) {
                            continue;
                        }
                    }
                    let val = e.hole(v, &k)?;
                    // ## GEBAUT (JS2/QUIC, 07.15): DIE ZWEI VORPRUEFUNGEN
                    // GEHOREN **NACH** DEM REPLACER — und sie sind ganz
                    // ueberfluessig, weil `serial` selbst entscheidet.
                    //
                    // ECMA-262 25.5.4.5 `SerializeJSONObject`, Schritt 7-11:
                    // ```text
                    // 8.  Let value be ? Get(V, P).
                    // 9.  If ReplacerFunction is not undefined, then
                    //     a. Set value to ? Call(ReplacerFunction, holder, « P, value »).
                    // 10. If Type(value) is Object, then … g. If IsCallable(value)
                    //     is true, return undefined.
                    // 11. Return undefined, if value is undefined.
                    // ```
                    // **`undefined` (11) und aufrufbar (10.g) kommen NACH 9.**
                    // Bei uns standen beide **vor** dem Replacer-Aufruf — und
                    // der Replacer-Aufruf steckt in `serial`, eine Ebene tiefer.
                    //
                    // GEMESSEN, `node 22`, **alle vier vorher falsch**:
                    // ```js
                    // var seen = [];
                    // JSON.stringify({a: undefined, b: 1}, function (k, v) { seen.push(k+':'+String(v)); return v; });
                    //   node: :[object Object]|a:undefined|b:1   bei uns: :[object Object]|b:1
                    // JSON.stringify({a: undefined}, function (k, v) { return k === 'a' ? 7 : v; });
                    //   node: {"a":7}                            bei uns: {}
                    // JSON.stringify({a: function(){}}, function (k, v) { return k === 'a' ? 7 : v; });
                    //   node: {"a":7}                            bei uns: {}
                    // var seen = [];
                    // JSON.stringify({a: function(){}}, function (k, v) { seen.push(k+':'+typeof v); return v; });
                    //   node: :object|a:function                 bei uns: :object
                    // ```
                    //
                    // **DAS DER EIGENTLICHE TESTFALL**,
                    // `JSON/stringify/repliver-function-object-deleted-property.js`:
                    // er benutzt einen **Getter**, der `b` loescht —
                    // ```js
                    // var obj = { get a() { delete this.b; return 1; }, b: 2 };
                    // ```
                    // Schritt 8 holt `a` → der Getter loescht `b` → Schritt 9
                    // ruft den Replacer → Schritt 8 holt `b` → **`undefined`**,
                    // und Schritt 9 ruft den Replacer **trotzdem**.
                    //
                    // **WARUM DAS LOESEN GENUEGT:** `serial_nach_erzwung` gibt
                    // fuer `Funktion | Nativ` bereits `Ok(None)` zurueck
                    // (`builtin3.rs:3048`) und fuer `GVal::Undefiniert` in der
                    // naechsten Zeile. **`serial` erledigt beide Faelle
                    // normgerecht** — es war nur nie zustaendig, es zu tun.
                    // Ein eigener Replacer-Aufruf hier waere ein **zweiter**
                    // Weg fuer dieselbe Sache: die neunte Form von „erst
                    // greppen, dann bauen".
                    // ## DER EINTRAG WIRD **GEPUFFERT**, und das ist der ganze
                    // Unterschied zwischen `{}` und `{"prop":null}`
                    //
                    // GEMESSEN (06:40), `node 22`:
                    //
                    // ```js
                    // JSON.stringify({prop:1}, function(k,v){ return v===1?undefined:v })
                    //   node:  "{}"        <- die Eigenschaft ENTFALLT
                    //   vorher: '{"prop":null}'
                    // ```
                    //
                    // Der alte Code schrieb erst das Komma und den
                    // `"k":`-Praefix **direkt in `out`** und kam dann zu
                    // `None`. Ein Eintrag laesst sich aus einem String nicht
                    // mehr zuruecknehmen — **das stand so auch im Kommentar**
                    // („Komma/TTL stimmen nicht … akzeptieren wir ersatzweise
                    // null? NEIN — JSON waere falsch"). Die Antwort war also
                    // bekannt und wurde nicht umgesetzt.
                    //
                    // **DIE LOESUNG IST ZWEI PHASEN**, und dieselbe Form wie
                    // bei `copyWithin` (Runde 10) und `concat` (Runde 13):
                    // erst den **fertigen** Eintrag bauen, dann anhängen —
                    // nur, wenn die Serialisation etwas ergeben hat.
                    //
                    // **IN EINEM FELD** ist es `null`, nicht „entfallen":
                    // die Stelle muss bestehen, sonst ruecken alle folgenden
                    // Indizes nach. Das steht im Zweig `HObj::Feld` und wird
                    // dort auch getestet — die beiden Regeln sind
                    // **verschieden** und man verwechselt sie leicht.
                    let mut eintrag = String::new();
                    if !erste {
                        eintrag.push(',');
                    }
                    if !self.einrueck.is_empty() {
                        eintrag.push('\n');
                        eintrag.push_str(&self.einrueck.repeat((tiefe + 1) as usize));
                    }
                    eintrag.push_str(&json_quote(&k));
                    eintrag.push(':');
                    if !self.einrueck.is_empty() {
                        eintrag.push(' ');
                    }
                    match self.serial(e, val, Some(k), v, tiefe + 1)? {
                        Some(teil) => eintrag.push_str(&teil),
                        // `undefined`: **kein** Eintrag. Kein Komma, kein
                        // `"k":`, kein `null`.
                        None => continue,
                    }
                    out.push_str(&eintrag);
                    erste = false;
                    e.string_grenze(out.len()).map_err(Stoerung::Fehler)?;
                }
                if !self.einrueck.is_empty() && !erste {
                    out.push('\n');
                    out.push_str(&self.einrueck.repeat(tiefe as usize));
                }
                out.push('}');
                Ok(Some(out))
            }
            // Map/Set/Fehler/Datum-ohne-toJSON/Regex: wie leeres Objekt.
            HObj::Karte { .. } | HObj::Menge { .. } | HObj::Fehler { .. } | HObj::Regex { .. } => {
                Ok(Some("{}".to_string()))
            }
            HObj::Datum { millis, .. } => Ok(Some(json_quote(&datum_iso(millis)))),
            // Release 10.10.2026: `JSON.stringify(proxy)` (ES2023 25.5.2.4/5):
            // ein Proxy auf ein Feld wird ein Feld (`length` + Indizes), sonst
            // ein Objekt aus seinen aufzaehlbaren eigenen Schluesseln — beides
            // ueber die Fallen. Vorher: immer `{}`.
            HObj::Proxy { .. } => {
                if e.proxy_ist_feld(v)? {
                    let n = match e.hole(v, "length")? {
                        GVal::Zahl(x) if x.is_finite() && x > 0.0 => (x as usize).min(crate::ops::MAX_SAMMLUNG),
                        _ => 0,
                    };
                    let mut out = String::from("[");
                    for i in 0..n {
                        if i > 0 {
                            out.push(',');
                        }
                        let el = e.hole(v, &i.to_string())?;
                        match self.serial(e, el, Some(i.to_string()), v, tiefe + 1)? {
                            Some(t) => out.push_str(&t),
                            None => out.push_str("null"),
                        }
                        e.string_grenze(out.len()).map_err(Stoerung::Fehler)?;
                    }
                    out.push(']');
                    return Ok(Some(out));
                }
                let mut keys = e.proxy_aufzaehlbare_schluessel(r)?;
                if let Some(liste) = &self.reihenfolge {
                    keys = liste.iter().filter(|k| keys.contains(k)).cloned().collect();
                }
                let mut out = String::from("{");
                let mut erste = true;
                for k in keys {
                    if let Some(erl) = &self.erlaubnis
                        && !erl.contains(&k)
                    {
                        continue;
                    }
                    let val = e.hole(v, &k)?;
                    let teil = match self.serial(e, val, Some(k.clone()), v, tiefe + 1)? {
                        Some(t) => t,
                        None => continue,
                    };
                    if !erste {
                        out.push(',');
                    }
                    if !self.einrueck.is_empty() {
                        out.push('\n');
                        out.push_str(&self.einrueck.repeat((tiefe + 1) as usize));
                    }
                    out.push_str(&json_quote(&k));
                    out.push(':');
                    if !self.einrueck.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(&teil);
                    erste = false;
                    e.string_grenze(out.len()).map_err(Stoerung::Fehler)?;
                }
                if !self.einrueck.is_empty() && !erste {
                    out.push('\n');
                    out.push_str(&self.einrueck.repeat(tiefe as usize));
                }
                out.push('}');
                Ok(Some(out))
            }
            // ## SCHRITT 4 — und vorher stand hier der reine Slot-Zugriff
            //
            // ```text
            // a. If value has a [[NumberData]] internal slot, then
            //    i. Set value to ? ToNumber(value).
            // b. Else if value has a [[StringData]] internal slot, then
            //    i. Set value to ? ToString(value).
            // c. Else if value has a [[BooleanData]] internal slot, then
            //    i. Set value to ? ToBoolean(value).
            // ```
            //
            // Vorher: `self.serial(e, primitiv, None, v, tiefe + 1)` — also der
            // **interne Slot direkt gelesen**, und `valueOf`/`toString` waren
            // **nie** beteiligt.
            //
            // **DER WIDERSPRUCH IM MOTOR, gemessen 07:10, ohne `node`:**
            //
            // ```js
            // var n = new Number(42);
            // n.valueOf = function () { throw new Error('vo') };
            // +n            // WURF: vo      <- der Motor ruft valueOf
            // n * 1         // WURF: vo
            // n + 1         // WURF: vo
            // n == 42       // WURF: vo
            // JSON.stringify({key: n})   // {"key":42}   <- ruft es NICHT
            // ```
            //
            // **Fuenf** Erzwingungen des Motors rufen `valueOf` und
            // reichen den Wurf weiter; `JSON.stringify` nicht. Das ist die
            // **sechsfache** Unstimmigkeit desselben Motors — und sie braucht
            // keine Referenzimplementierung.
            //
            // `ToNumber` auf einer Number-Box ist `ToPrimitive(v, number)`,
            // also **`valueOf` zuerst**; `ToString` auf einer String-Box ist
            // `ToPrimitive(v, string)`, also **`toString` zuerst**. Genau das
            // prueft `value-number-object.js` / `value-string-object.js`.
            // `ToBoolean` ruft **nie** Benutzercode — dort ist das
            // Auspacken richtig.
            //
            // M3: `zu_zahl`/`zu_text` koennen Benutzercode aufrufen (deren
            // Kosten bucht der Aufrufmechanismus); **hier** wird nichts
            // allokiert. Und es ist der **letzte** Schritt, der die Box
            // verlaesst — also **ohne** den zweiten `toJSON`/Replacer-
            // Durchlauf, den der alte Weg hatte (siehe `serial_nach_erzwung`).
            HObj::Verpackt { primitiv, .. } => {
                let entpackt = match primitiv {
                    GVal::Zahl(_) => GVal::Zahl(e.zu_zahl(v)?),
                    GVal::Ref(sr) if matches!(e.heap.holen(sr), HObj::Zeichenkette(_)) => {
                        // Fremder Bearbeiter (Schreibvorgang 03.10. 06:18:43):
                        // `zu_text` liefert eine `String`, die `match`-Aerme
                        // hier aber `GVal` liefern muessen — die Crate war
                        // dadurch **nicht übersetzbar**. Die Absicht des
                        // Fremdschreibers (Box mit `toString` auspacken)
                        // bleibt unveraendert; nur der Typ wird hergestellt.
                        let t = e.zu_text(v)?;
                        e.text_wert(&t).map_err(Stoerung::Fehler)?
                    }
                    GVal::Bool(b) => GVal::Bool(b),
                    andere => andere,
                };
                self.serial_nach_erzwung(e, entpackt, halter, tiefe + 1)
            }
            _ => Ok(Some("{}".to_string())),
        }
    }
}

fn json_zahl(n: f64) -> String {
    if n == 0.0 {
        return "0".to_string();
    }
    crate::wert::zahl_text(n)
}

fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// Datum (UTC, dokumentiert)
// ---------------------------------------------------------------------------

pub(crate) fn jetzt_millis() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// Zeitwert klammern (|t| <= 8.64e15, sonst NaN).
fn zeit_klammern(t: f64) -> f64 {
    if t.is_nan() || t.abs() > 8.64e15 {
        f64::NAN
    } else {
        // J-CACHE (04.10.2026): **TimeClip schneidet ab** (ES2023 20.3.1.15:
        // ToInteger + (+0)): new Date(6.54321) -> 6 (S9.4), -0 -> +0.
        t.trunc() + 0.0
    }
}

fn tage_aus_civil(j: i64, m: i64, t: i64) -> i64 {
    let jj = if m <= 2 { j - 1 } else { j };
    let era = jj.div_euclid(400);
    let yoe = jj - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + t - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_aus_tagen(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut j = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let t = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    j += if m <= 2 { 1 } else { 0 };
    (j, m, t)
}

fn datum_aus_teilen(
    jahr: f64,
    monat: f64,
    tag: f64,
    std: f64,
    min: f64,
    sek: f64,
    ms: f64,
    mit_1900: bool,
) -> f64 {
    if ![jahr, monat, tag, std, min, sek, ms]
        .iter()
        .all(|n| n.is_finite())
    {
        return f64::NAN;
    }
    let mut j = jahr.trunc() as i64;
    // J-CACHE (04.10.2026): **Komponenten trunkieren** (MakeDay/MakeTime
    // nutzen ToInteger je Komponente — non-integer-values.js). fp-Werte
    // sind ganzzahlig und bleiben unberuehrt.
    let monat = monat.trunc();
    let tag = tag.trunc();
    let std = std.trunc();
    let min = min.trunc();
    let sek = sek.trunc();
    let ms = ms.trunc();
    // J-CACHE (04.10.2026): **1900-Regel nur fuer Konstruktor/UTC.**
    // ES2023 20.4.3.1 (Konstruktor) und 20.4.3.2 (Date.UTC): 0-99 ->
    // 1900+y. MakeDay/setFullYear haben sie NICHT — `setFullYear(1)`
    // setzt Jahr 1 (Datei date-value-...-when-date-is-valid erwartet
    // getFullYear() == 1, nicht 1901).
    if mit_1900 && (0..100).contains(&j) {
        j += 1900;
    }
    // Monat/Tag normalisieren ueber Tage-Rechnung.
    let mo = monat.trunc() as i64;
    let jj = j + mo.div_euclid(12);
    let mm = mo.rem_euclid(12) + 1;
    let tage = tage_aus_civil(jj, mm, 1) + (tag.trunc() as i64 - 1);
    // J-CACHE (04.10.2026): **Millis in f64 nach Spec** (MakeTime/MakeDate,
    // IEEE-754: ((h*msPerHour + m*msPerMinute) + s*msPerSecond) + milli).
    // Die i64-exakte Rechnung traf Riesen-Werte anders als die
    // Spec-Rundung (fp-evaluation-order.js: 29312 statt 29256). Bei
    // Normalwerten (< 2^53) bitgleich zur alten Rechnung.
    let zeit_ms = ((std * 3_600_000.0 + min * 60_000.0) + sek * 1000.0) + ms;
    (tage as f64) * 86_400_000.0 + zeit_ms
}

fn datum_teile(millis: f64) -> (i64, u32, u32, u32, u32, u32, u32, u32) {
    let ms_ges = millis.trunc() as i64;
    let tage = ms_ges.div_euclid(86_400_000);
    let rest = ms_ges.rem_euclid(86_400_000);
    let (j, mo, t) = civil_aus_tagen(tage);
    let std = (rest / 3_600_000) as u32;
    let min = ((rest % 3_600_000) / 60_000) as u32;
    let sek = ((rest % 60_000) / 1000) as u32;
    let ms = (rest % 1000) as u32;
    // Wochentag: 1970-01-01 war Donnerstag (4).
    let woche = ((tage + 4).rem_euclid(7)) as u32;
    (j, mo, t, std, min, sek, ms, woche)
}

pub(crate) fn datum_iso(millis: f64) -> String {
    let (j, mo, t, std, min, sek, ms, _) = datum_teile(millis);
    // J-CACHE (04.10.2026): **erweitertes Jahr mit Vorzeichen.** ES2023
    // 20.4.4.33: Jahre < 0 oder > 9999 tragen Vorzeichen + 6 Stellen
    // („-271821-04-20...", „+275760-09-13..."). GEMESSEN an
    // time-value-maximum-range.js (maxDateStr).
    let jahr = if j < 0 {
        format!("-{:06}", -j)
    } else if j > 9999 {
        format!("+{:06}", j)
    } else {
        format!("{j:04}")
    };
    format!("{jahr}-{mo:02}-{t:02}T{std:02}:{min:02}:{sek:02}.{ms:03}Z")
}

/// `toString()` / `toDateString()` / `toTimeString()` — **lokale** Zeit.
///
/// J24: davor stand hier fest `GMT+0000 (UTC)`. GEMESSEN (node 22,
/// `String(new Date(0))`): `Thu Jan 01 1970 01:00:00 GMT+0100 (Central
/// European Standard Time)` — die Zone kommt aus `zone.rs`. Die
/// Zonen**bezeichnung** (der Klammertext) steht nicht im TZif-Dateihead,
/// sondern im Fuss als POSIX-Text; der Motor nimmt dafuer eine feste
/// Kurzform (`CET`, `MEZ` bleibt leer) — dokumentierte Naeherung, siehe
/// `docs` in `zone.rs`.
pub(crate) fn datum_text(millis: f64) -> String {
    const TAGE: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONATE: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let schiebung = crate::zone::verschiebung_s(millis);
    let (j, mo, t, std, min, sek, _, woche) = datum_teile(millis + schiebung as f64 * 1000.0);
    // GEMESSEN (node 22, `String(new Date(0))`): Berlin (UTC+1) schreibt
    // `GMT+0100` — das Vorzeichen folgt also **ost** (nicht west wie bei
    // `getTimezoneOffset()`, das -60 liefert). Genau diese Umkehr stand
    // zuerst falsch herum im Motor.
    let (vh, vm) = {
        let m = schiebung as i64 / 60;
        let vorzeichen = if m < 0 { '-' } else { '+' };
        let m = m.abs();
        (vorzeichen, format!("{:02}{:02}", m / 60, m % 60))
    };
    let zonen_text = {
        let k = crate::zone::kenner(millis);
        if k.is_empty() { "UTC".to_string() } else { k }
    };
    format!(
        "{} {} {:02} {} {std:02}:{min:02}:{sek:02} GMT{vh}{vm} ({zonen_text})",
        TAGE[woche as usize],
        MONATE[(mo - 1) as usize],
        t,
        jahr_text(j)
    )
}

/// Jahreszahl fuer toString/toDateString/toUTCString: negativ mit
/// Minus + 4 Stellen („-0001"), sonst 4-stellig ohne Vorzeichen
/// („0000", „275760" — kein Plus, GEMESSEN node 22). Anders als ISO
/// (dort ±6-stellig, siehe datum_iso). GEMESSEN an negative-year.js
/// (toDateString/toString/toUTCString: Jahr -1 -> „-0001", nicht „-001").
fn jahr_text(j: i64) -> String {
    if j < 0 {
        format!("-{:04}", -j)
    } else {
        format!("{j:04}")
    }
}

/// `toDateString()` — nur der **Datums**teil in lokaler Zeit.
/// GEMESSEN (node 22): `new Date(0).toDateString()` -> `Thu Jan 01 1970`
/// (vor J24 schrieb der Motor hier den vollen `toString`-Text).
pub(crate) fn datum_text_datum(millis: f64) -> String {
    const TAGE: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONATE: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let schiebung = crate::zone::verschiebung_s(millis);
    let (j, mo, t, _, _, _, _, woche) = datum_teile(millis + schiebung as f64 * 1000.0);
    format!(
        "{} {} {:02} {}",
        TAGE[woche as usize],
        MONATE[(mo - 1) as usize],
        t,
        jahr_text(j)
    )
}

/// `toTimeString()` — nur der **Zeit**teil in lokaler Zeit.
/// GEMESSEN (node 22): `new Date(0).toTimeString()` ->
/// `01:00:00 GMT+0100 (Central European Standard Time)`.
pub(crate) fn datum_text_zeit(millis: f64) -> String {
    let schiebung = crate::zone::verschiebung_s(millis);
    let (_, _, _, std, min, sek, _, _) = datum_teile(millis + schiebung as f64 * 1000.0);
    let m = schiebung as i64 / 60;
    let vorzeichen = if m < 0 { '-' } else { '+' };
    let m = m.abs();
    let zonen_text = {
        let k = crate::zone::kenner(millis);
        if k.is_empty() { "UTC".to_string() } else { k }
    };
    format!(
        "{std:02}:{min:02}:{sek:02} GMT{vorzeichen}{:02}{:02} ({zonen_text})",
        m / 60,
        m % 60
    )
}

fn datum_teil(millis: f64, teil: u8) -> i64 {
    let (j, mo, t, std, min, sek, ms, woche) = datum_teile(millis);
    match teil {
        0 => j,
        1 => mo as i64 - 1,
        2 => t as i64,
        3 => woche as i64,
        4 => std as i64,
        5 => min as i64,
        6 => sek as i64,
        _ => ms as i64,
    }
}

/// ISO-Teilmenge + `Month D, YYYY` + Millis-Zahl, sonst NaN.
fn datum_parsen(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return f64::NAN;
    }
    if let Ok(n) = t.parse::<f64>() {
        // J-CACHE (04.10.2026): **reine Ganzzahl = JAHR, nicht Millis.**
        // GEMESSEN node 22: parse("1970") = 0, parse("123") = Jahr 123,
        // parse("+2026") = 2026-01-01. Die alte Millis-Deutung war falsch.
        // (1900-Regel bei 0-99 wie Konstruktor; "-0" allein -> NaN wie in
        // ISO-Form.) Nicht-ganzzahliges Numerisches („1.5") bleibt
        // Millis-Naeherung (V8-Heuristik, kein Test).
        let (neg, ziffern) = match t.strip_prefix(['+', '-']) {
            Some(r) => (t.starts_with('-'), r),
            None => (false, t),
        };
        if !ziffern.is_empty()
            && ziffern.len() <= 6
            && ziffern.bytes().all(|c| c.is_ascii_digit())
        {
            if let Ok(z) = ziffern.parse::<i64>() {
                if neg && z == 0 {
                    return f64::NAN;
                }
                let mut j = if neg { -z } else { z };
                if (0..100).contains(&j) {
                    j += 1900;
                }
                return zeit_klammern(tage_aus_civil(j, 1, 1) as f64 * 86_400_000.0);
            }
        }
        return zeit_klammern(n);
    }
    // ISO: YYYY-MM-DD[THH:MM[:SS[.sss]][Z|±HH:?MM]] oder erweitert
    // ±YYYYYY-MM-DD (time-value-maximum-range: "-271821-04-20T00:00:00Z").
    // J-CACHE (04.10.2026): erweitertes Jahr — als "0000-MM-DD..." fuer
    // den ISO-Block unten tarnen, echtes Jahr in jahr_ext merken.
    let (t_iso, jahr_ext): (String, Option<i64>) = match t.strip_prefix(['+', '-']) {
        Some(rest) if rest.len() >= 7 && rest.as_bytes()[6] == b'-' => {
            let (z, _) = rest.split_at(6);
            match (z.parse::<i64>(), t.starts_with('-')) {
                // J-CACHE (04.10.2026): **-000000 ist ungueltig** (Spec
                // expanded-years: Jahr 0 traegt +, nie -). year-zero.js.
                (Ok(0), true) => return f64::NAN,
                (Ok(z), neg) => (
                    format!("0000{}", &rest[6..]),
                    Some(if neg { -z } else { z }),
                ),
                _ => (t.to_string(), None),
            }
        }
        _ => (t.to_string(), None),
    };
    let b = t_iso.as_bytes();
    let ziffer = |i: usize, n: usize| -> Option<i64> {
        if i + n > b.len() {
            return None;
        }
        if !b[i..i + n].iter().all(|c| c.is_ascii_digit()) {
            return None;
        }
        t_iso[i..i + n].parse::<i64>().ok()
    };
    if b.len() >= 10 && b[4] == b'-' && b[7] == b'-' {
        // Erweitertes Jahr (±YYYYYY) aus jahr_ext, sonst 4-stellig.
        let j = match jahr_ext {
            Some(x) => x,
            None => match ziffer(0, 4) {
                Some(x) => x,
                None => return f64::NAN,
            },
        };
        let mo = match ziffer(5, 2) {
            Some(x) => x,
            None => return f64::NAN,
        };
        let tg = match ziffer(8, 2) {
            Some(x) => x,
            None => return f64::NAN,
        };
        if !(1..=12).contains(&mo) || !(1..=31).contains(&tg) {
            return f64::NAN;
        }
        let mut ms_tag = tage_aus_civil(j, mo, tg) as f64 * 86_400_000.0;
        let mut rest = &t_iso[10..];
        if rest.is_empty() {
            return zeit_klammern(ms_tag);
        }
        if rest.starts_with(['T', ' ']) {
            rest = &rest[1..];
            if rest.len() < 5 {
                return f64::NAN;
            }
            let hh: i64 = match rest[0..2].parse() {
                Ok(x) => x,
                Err(_) => return f64::NAN,
            };
            if rest.as_bytes()[2] != b':' {
                return f64::NAN;
            }
            let mm: i64 = match rest[3..5].parse() {
                Ok(x) => x,
                Err(_) => return f64::NAN,
            };
            let mut pos = 5;
            let mut ss: i64 = 0;
            let mut mss: i64 = 0;
            if rest.len() > pos && rest.as_bytes()[pos] == b':' {
                if rest.len() < pos + 3 {
                    return f64::NAN;
                }
                ss = match rest[pos + 1..pos + 3].parse() {
                    Ok(x) => x,
                    Err(_) => return f64::NAN,
                };
                pos += 3;
                if rest.len() > pos && rest.as_bytes()[pos] == b'.' {
                    pos += 1;
                    let mut n = 0;
                    let mut f = 0i64;
                    while pos + n < rest.len() && rest.as_bytes()[pos + n].is_ascii_digit() && n < 3
                    {
                        f = f * 10 + (rest.as_bytes()[pos + n] - b'0') as i64;
                        n += 1;
                    }
                    while n < 3 {
                        f *= 10;
                        n += 1;
                    }
                    // Weitere Ziffern ignorieren (ms-Genauigkeit).
                    while pos < rest.len() && rest.as_bytes()[pos].is_ascii_digit() {
                        pos += 1;
                    }
                    mss = f;
                }
            }
            if !(0..24).contains(&hh) || !(0..60).contains(&mm) || !(0..61).contains(&ss) {
                return f64::NAN;
            }
            ms_tag += (hh * 3600 + mm * 60 + ss) as f64 * 1000.0 + mss as f64;
            let zone = &rest[pos..];
            if zone.is_empty() {
                // J-CACHE (04.10.2026): **date-time ohne Zone ist LOKAL**
                // (nur date-only „1970-01-01" ist UTC — steht oben).
                // `without-utc-offset.js`: parse('1970-01-01T00:00:00')
                // == timezoneOffsetMS. Einmalige Verschiebungs-Naeherung.
                ms_tag -= crate::zone::verschiebung_s(ms_tag) as f64 * 1000.0;
                return zeit_klammern(ms_tag);
            }
            if zone == "Z" {
                return zeit_klammern(ms_tag);
            }
            if (zone.starts_with('+') || zone.starts_with('-')) && zone.len() >= 3 {
                let neg = zone.starts_with('-');
                let zr = &zone[1..];
                let (zh, zm) = if zr.contains(':') {
                    let mut it = zr.split(':');
                    let a = it.next().unwrap_or("0").parse::<i64>().unwrap_or(-1);
                    let c = it.next().unwrap_or("0").parse::<i64>().unwrap_or(-1);
                    (a, c)
                } else if zr.len() == 4 {
                    (
                        zr[0..2].parse::<i64>().unwrap_or(-1),
                        zr[2..4].parse::<i64>().unwrap_or(-1),
                    )
                } else if zr.len() == 2 {
                    (zr.parse::<i64>().unwrap_or(-1), 0)
                } else {
                    return f64::NAN;
                };
                if zh < 0 || zh > 23 || zm < 0 || zm > 59 {
                    return f64::NAN;
                }
                let off = (zh * 60 + zm) * 60_000;
                ms_tag += if neg { off as f64 } else { -(off as f64) };
                return zeit_klammern(ms_tag);
            }
            return f64::NAN;
        }
        return f64::NAN;
    }
    // `Month D, YYYY [HH:MM:SS]` + RFC 2822 (`Sat, 03 Oct 2026 12:00:00
    // GMT`). J-CACHE (04.10.2026): **Umschreibung auf Suche statt Position.**
    // Vorher musste der Monat an worte[0] stehen — RFC mit Wochentag
    // („sat, 03 oct ...") fiel durch (NaN statt Zeit). Jetzt: erstes
    // Monats-Wort suchen, Tag = Zahl davor (RFC) oder danach (US),
    // Jahr = erste Zahl mit 4 Stellen (oder > 31), Rest = Zeit + Zone.
    // Zone: letztes Wort gmt/ut/z -> UTC; ±HHMM -> Offset; sonst LOKAL
    // (Wandzeit minus Verschiebungs-Naeherung — V8 rechnet exakt, wir
    // einmalig; DST-Wechsel kann ±1h danebenliegen, dokumentiert).
    // GEMESSEN an node 22: parse("Sat, 03 Oct 2026 12:00:00 GMT") und
    // parse("Oct 3, 2026 10:00") (lokal).
    const MONATE: [(&str, i64); 12] = [
        ("january", 1),
        ("february", 2),
        ("march", 3),
        ("april", 4),
        ("may", 5),
        ("june", 6),
        ("july", 7),
        ("august", 8),
        ("september", 9),
        ("october", 10),
        ("november", 11),
        ("december", 12),
    ];
    let klein = t.to_lowercase();
    // Wochentag-Präfix ("sat, ...") entfernen.
    let klein = match klein.split_once(',') {
        Some((vor, nach)) => {
            if vor.trim().len() <= 9
                && !vor.trim().is_empty()
                && MONATE.iter().all(|(n, _)| !vor.trim().starts_with(&n[..3]))
            {
                nach.to_string()
            } else {
                klein
            }
        }
        None => klein,
    };
    let klein = klein.replace(',', " ").replace(':', " ").replace('/', " ");
    let worte: Vec<&str> = klein.split_whitespace().collect();
    // Monats-Index suchen (erste 4 Wörter).
    let mut monat: Option<(usize, i64)> = None;
    for (i, w) in worte.iter().take(4).enumerate() {
        for (n, m) in MONATE {
            if w.starts_with(&n[..3]) {
                monat = Some((i, m));
                break;
            }
        }
        if monat.is_some() {
            break;
        }
    }
    if let Some((mi, mo)) = monat {
        // Tag: Zahl direkt davor (RFC "03 oct") oder danach (US "oct 3").
        // Jahr: erste Zahl mit 4 Stellen oder > 31 (nicht Tag/Zeit).
        let zahl = |s: &str| -> Option<i64> { s.parse::<i64>().ok() };
        let mut tg: Option<i64> = None;
        if mi > 0 {
            tg = worte.get(mi - 1).and_then(|s| zahl(s));
        }
        let mut jr: Option<i64> = None;
        let mut rest_ab = mi + 1;
        for (i, w) in worte.iter().enumerate() {
            if i == mi || (mi > 0 && i == mi - 1 && tg.is_some()) {
                continue;
            }
            if let Some(z) = zahl(w) {
                if w.len() == 4 || z.abs() > 31 {
                    jr = Some(z);
                    rest_ab = i + 1;
                    break;
                }
                if tg.is_none() {
                    tg = Some(z);
                }
            }
        }
        if let (Some(tg), Some(jr)) = (tg, jr) {
            if !(1..=31).contains(&tg) {
                return f64::NAN;
            }
            // Zeit + Zone aus dem Rest.
            let mut hh: i64 = 0;
            let mut mm: i64 = 0;
            let mut ss: i64 = 0;
            let mut zone_utc = false;
            let mut zone_off: Option<i64> = None;
            let mut zi = rest_ab;
            if worte.get(zi).map(|s| s.len() == 2).unwrap_or(false)
                && worte.get(zi + 1).map(|s| s.len() == 2).unwrap_or(false)
            {
                hh = worte[zi].parse().unwrap_or(0);
                mm = worte[zi + 1].parse().unwrap_or(0);
                zi += 2;
                if worte.get(zi).map(|s| s.len() == 2).unwrap_or(false) {
                    ss = worte[zi].parse().unwrap_or(0);
                    zi += 1;
                }
            }
            if let Some(z) = worte.get(zi) {
                if *z == "gmt" || *z == "ut" || *z == "utc" || *z == "z" {
                    zone_utc = true;
                } else if (z.starts_with('+') || z.starts_with('-')) && z.len() == 5 {
                    let neg = z.starts_with('-');
                    if let (Ok(zh), Ok(zm)) = (z[1..3].parse::<i64>(), z[3..5].parse::<i64>()) {
                        let off = (zh * 60 + zm) * 60_000;
                        zone_off = Some(if neg { off } else { -off });
                    }
                }
            }
            let mut ms_tag = tage_aus_civil(jr, mo, tg) as f64 * 86_400_000.0
                + ((hh * 3600 + mm * 60 + ss) as f64) * 1000.0;
            if let Some(off) = zone_off {
                ms_tag += off as f64;
            } else if !zone_utc {
                // Lokal: Wandzeit minus Verschiebung (einmalige Naeherung).
                ms_tag -= crate::zone::verschiebung_s(ms_tag) as f64 * 1000.0;
            }
            return zeit_klammern(ms_tag);
        }
    }
    f64::NAN
}

// Re-Export-Namen, damit `Wert` genutzt wird (kein ungenutzter Import).
#[allow(dead_code)]
fn _wert_nutzen(w: &Wert) -> String {
    w.als_text()
}
