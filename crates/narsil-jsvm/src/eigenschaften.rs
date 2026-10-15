// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Eigenschaften, Literale, Operatoren, Destrukturierung, Klassen,
//! Versprechen-Verwaltung, Export (Fortsetzung von ops.rs).

use std::collections::{HashMap, HashSet};

use crate::fehler::JsFehler;
use crate::heap::{Eigenschaft, GVal, HObj, Heap, HeapRef, KEIN_REF};
use crate::interp::{Engine, Stoerung};
use crate::ops::{MAX_SAMMLUNG, index_aus_schluessel};

impl Engine {
    // ---------------------------------------------------------------
    // Boxen / Funktionen anlegen
    // ---------------------------------------------------------------

    // ---------------------------------------------------------------
    // Eigenschafts-Zugriff
    // ---------------------------------------------------------------

    /// Eigene (nicht vererbte) Eigenschaft lesen, OHNE Getter aufzurufen
    /// (fuer Export, Interna, for-in-Aufzaehlung).
    pub(crate) fn eigene_lese(&self, obj: HeapRef, name: &str) -> Option<GVal> {
        match self.heap.holen(obj) {
            HObj::Gegenstand { eigenschaften, .. } => eigenschaften.get(name).map(|e| e.wert),
            // ## W30 (JS2/QUIC, Runde 74) — `HObj::Verpackt` bekommt einen
            // **eigenen** Arm, weil es eine **zweite** Quelle hat:
            // `eigenschaften` **und** `elemente`.
            //
            // Vorher war es ein Union-Arm mit `Gegenstand` und las **nur**
            // `eigenschaften`. GEMESSEN (`node 22`, `T262_NUR`, 10,5 ms):
            // ```js
            // new String('ab').hasOwnProperty('0')          node true   bei uns false
            // Object.getOwnPropertyNames(new String('ab'))   node '0 1 length'  bei uns ''
            // ```
            // und `String/numeric-properties.js` meldete genau das:
            // `Unbehandelt: 0 should be an own property`.
            //
            // **WARUM ES AUCH ALS EIGENE EIGENSCHAFT ZAEHLT** (ES2023
            // 10.4.3): die Indexeigenschaften eines `String`-Objekts sind
            // `writable: false, enumerable: true, configurable: false` —
            // **ausdruecklich eigene** Eigenschaften. `n_text_ctor`
            // (`builtin2.rs`) legt sie in `elemente` ab, und `hole` liest
            // sie dort; **nur `eigene_lese` kannte sie nicht**.
            //
            // **DIE REIHENFOLGE IST DIE REGEL:** `eigenschaften` **vor**
            // `elemente` — sonst verdeckt ein normaler String-Schluessel
            // einen Index. Dasselbe Muster wie bei `HObj::Feld` zwei Zeilen
            // darunter (`rest` **vor** dem Index, J27) — *dieselbe Frage,
            // dieselbe Antwort.*
            HObj::Verpackt {
                eigenschaften,
                elemente,
                ..
            } => {
                if let Some(e) = eigenschaften.get(name) {
                    return Some(e.wert);
                }
                if let Some(el) = elemente
                    && let Some(i) = index_aus_schluessel(name)
                    && i < el.len()
                {
                    return el.get(i).copied();
                }
                None
            }
            HObj::Feld {
                elemente,
                rest,
                luecken,
                ..
            } => {
                if name == "length" {
                    // ## W51 (JS, 05.10.2026) — DIE GROSSE LAENGE STEHT IN `rest`
                    //
                    // `elemente.len()` ist **nicht** die Laenge, sobald W50 das
                    // Feld **nicht** materialisiert: bei `a.length = 4294967295`
                    // bleibt `elemente` **leer**, und die Zahl liegt nach
                    // `rest["length"]`. GEMESSEN vorher: `a.length` lieferte `0`.
                    // node 22: `4294967295` (und **sofort**, ohne Speicher).
                    //
                    // **UND: `rest["length"]` muss verschwinden, wenn wieder
                    // materialisiert wird.** Sonst bleibt ein alter Wert stehen
                    // und ueberdeckt `elemente.len()`:
                    // ```js
                    // var a = []; a.length = 4294967295; a.length = 2;
                    // ```
                    // Der Setter loescht den Eintrag in diesem Fall (siehe dort).
                    if let Some(e) = rest.get("length") {
                        return Some(e.wert);
                    }
                    return Some(GVal::Zahl(elemente.len() as f64));
                }
                // J27: `rest` **vor** dem Index. GEMESSEN, warum: ein
                // Accessor auf einem Feld wird in `rest` abgelegt (siehe
                // `getter_setter_definieren_mit`), und ohne diese Zeile
                // wuerde `elemente[i]` den Accessor **verdecken** —
                // `Object.defineProperty(a,'0',{get:f})` wuerde dann `f`
                // nie aufrufen. Die Zusage ist sicher: `setze_eigen`
                // legt **keine** Indexschluessel in `rest`
                // (`eingebaut_array.rs`: nur was kein Index ist und nicht
                // `length`), also steht in `rest` ein Index nur, wenn
                // dort bewusst ein Accessor liegt.
                if let Some(e) = rest.get(name) {
                    return Some(e.wert);
                }
                if let Some(i) = index_aus_schluessel(name) {
                    // Luecke (J7): kein eigener Index — `hasOwnProperty`
                    // und `Object.getOwnPropertyDescriptor` melden sie nicht.
                    if luecken.as_ref().is_some_and(|m| m.contains(&i)) {
                        return None;
                    }
                    return elemente.get(i).copied();
                }
                rest.get(name).map(|e| e.wert)
            }
            // ## W55 (JS, 05.10.2026) — DER INDEX EINES TYPEDARRAYS
            //
            // **GEMESSEN (node 22, v22.23.3), die vier Faelle, die diese
            // Funktion entscheidet:**
            // ```text
            // String(new Uint8Array([1,2,3]))     -> '1,2,3'  (also: 0 = 1, nicht undefined)
            // String(new Uint8Array(2)[5])       -> 'undefined'
            // String(new Uint8Array(2)[-1])      -> 'undefined'
            // new Uint8Array(2).hasOwnProperty('0') -> true
            // ```
            // **DIE REIHENFOLGE IST DIE REGEL** (J27, dieselbe Frage bei
            // `HObj::Feld`): `eigenschaften` **vor** dem Index. Auf einem
            // TypedArray darf ein Accessor auf `'0'` gelegt werden, und ohne
            // diese Zeile wuerde `art.lesen(...)` ihn verdecken — der Getter
            // liefe nie.
            //
            // **AUSSERHALB DER LAENGE: `None`, nicht `undefined`.** `None`
            // heisst hier „keine eigene Eigenschaft" — das ist, was
            // `hasOwnProperty` und `Object.keys` sehen. **GEMESSEN (node 22):**
            // `Object.keys(new Uint8Array(2))` -> `"0,1"` und **nicht**
            // `0,1,5` — ein Index ausserhalb ist **kein** Schluessel.
            HObj::TypedArray {
                eigenschaften,
                art,
                puffer,
                versatz,
                laenge,
                ..
            } => {
                if let Some(e) = eigenschaften.get(name) {
                    return Some(e.wert);
                }
                if let Some(i) = index_aus_schluessel(name) {
                    if i >= *laenge {
                        return None;
                    }
                    // Die Bytes kommen aus dem **geteilten** Puffer — das ist
                    // der ganze Punkt der Variante (siehe `heap.rs`, W55).
                    return match self.heap.holen(*puffer) {
                        HObj::BytePuffer { daten, .. } => {
                            Some(art.lesen(daten, versatz + i * art.bytes()))
                        }
                        _ => None,
                    };
                }
                None
            }
            HObj::Funktion { eigenschaften, .. } | HObj::Nativ { eigenschaften, .. } => {
                eigenschaften.get(name).map(|e| e.wert)
            }
            HObj::Generator { eigenschaften, .. }
            | HObj::Fehler { eigenschaften, .. }
            | HObj::Datum { eigenschaften, .. }
            | HObj::Regex { eigenschaften, .. }
            | HObj::Karte { eigenschaften, .. }
            | HObj::Menge { eigenschaften, .. }
            | HObj::Versprechen { eigenschaften, .. } => eigenschaften.get(name).map(|e| e.wert),
            _ => None,
        }
    }

    /// Eigenschaft inkl. Prototyp-Kette + Getter (mit this=basis).
    /// Release 09.10.2026: das Feld aus `el.attributes` bekommt den
    /// Prototyp `NamedNodeMap.prototype` (Methoden aus `vorspann.js`) und das
    /// versteckte Feld `__el` (DOM §4.9.1). Ohne Prototyp im globalen Bereich
    /// bleibt es ein Feld.
    /// Release 11.10.2026: eine Knotenliste des Hosts bekommt ihren
    /// Prototyp (`HTMLCollection`, `NodeList`; DOM §4.2.10). GEMESSEN vorher
    /// (WPT dom/nodes): `getElementsByTagName(...) instanceof HTMLCollection`
    /// war false, `children.item` „is not a function".
    pub(crate) fn als_sammlung(&mut self, v: GVal, schnittstelle: &str) {
        let GVal::Ref(r) = v else { return };
        if !matches!(self.heap.holen(r), HObj::Feld { .. }) {
            return;
        }
        let proto = match self.hole_ohne_host(GVal::Ref(self.global_objekt), schnittstelle) {
            Ok(GVal::Ref(c)) => match self.hole_ohne_host(GVal::Ref(c), "prototype") {
                Ok(GVal::Ref(p)) => p,
                _ => return,
            },
            _ => return,
        };
        if let HObj::Feld { proto: p, .. } = self.heap.holen_mut(r) {
            *p = Some(proto);
        }
        if schnittstelle == "HTMLCollection" {
            self.proto_html_sammlung = Some(proto);
        }
    }

    /// Release 13.10.2026: der Namenszugriff einer HTMLCollection
    /// (`document.forms.fm1`, WebIDL §3.9 „named properties"): fehlt der
    /// Name im Feld und in der Kette, fragt `namedItem`. GEMESSEN vorher (WPT
    /// forms/resetting-a-form, form-nameditem): `document.forms.fm1` war
    /// undefined.
    fn sammlung_name(&mut self, basis: GVal, name: &str) -> Result<Option<GVal>, Stoerung> {
        let Some(proto) = self.proto_html_sammlung else { return Ok(None) };
        let GVal::Ref(r) = basis else { return Ok(None) };
        if !matches!(self.heap.holen(r), HObj::Feld { proto: Some(p), .. } if *p == proto) {
            return Ok(None);
        }
        if name.is_empty() || name.starts_with("__") || name.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(None);
        }
        let f = self.hole_ohne_host(GVal::Ref(proto), "namedItem")?;
        if !self.ist_aufrufbar(f) {
            return Ok(None);
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(basis);
        let n = self.text_wert(name).map_err(Stoerung::Fehler);
        let erg = match n {
            Ok(n) => self.aufruf_synchron(f, basis, &[n], false),
            Err(e) => Err(e),
        };
        self.wurzel_zurueck(marke);
        match erg? {
            GVal::Null | GVal::Undefiniert => Ok(None),
            v => Ok(Some(v)),
        }
    }

    fn als_named_node_map(&mut self, v: GVal, el: GVal) {
        let GVal::Ref(r) = v else { return };
        if !matches!(self.heap.holen(r), HObj::Feld { .. }) {
            return;
        }
        let proto = match self.hole_ohne_host(GVal::Ref(self.global_objekt), "NamedNodeMap") {
            Ok(GVal::Ref(c)) => match self.hole_ohne_host(GVal::Ref(c), "prototype") {
                Ok(GVal::Ref(p)) => p,
                _ => return,
            },
            _ => return,
        };
        if let HObj::Feld { proto: p, .. } = self.heap.holen_mut(r) {
            *p = Some(proto);
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(v);
        self.wurzeln.push(el);
        let _ = self.versteckt_setzen(r, "__el", el);
        self.wurzel_zurueck(marke);
    }

    /// Gibt (Wert, gefunden?) zurueck — Getter werden aufgerufen.
    pub(crate) fn hole(&mut self, basis: GVal, name: &str) -> Result<GVal, Stoerung> {
        // Lazily loaded platform globals (`window.WebAssembly`, …): one
        // compare until every API has been loaded.
        if self.platform_pending() && basis == GVal::Ref(self.global_objekt) {
            self.platform_lazy_global(name)?;
        }
        //
        // W6B, J9: HOST-OBJEKT ZUERST.
        //
        // Ein Objekt mit dem Feld `__host` (einer Zahl) traegt seine
        // Eigenschaften im Haken des Aufrufers, nicht in der Feldliste.
        // `el.textContent` geht genau hier hinein.
        //
        // GEMESSEN, warum VOR allem anderen: ein Host-Objekt ist ein
        // ganz gewöhnliches Objekt — ohne diesen Schritt liefe `hole()`
        // in die Feldliste und fände dort nichts als `__host`.
        //
        // GEMESSEN, warum `basis` geklont wird: `self.host_objekte`
        // wird spaeter im Haken benutzt, und der Haken darf keinen
        // `&mut self` halten.
        if let Some(id) = self.host_id_von_gval(basis)
            && let Some(h) = self.host_objekte.clone()
        {
            if let GVal::Ref(r) = basis {
                // Release 09.10.2026: eigene Felder (auch `__data` & Co. von
                // Polymer) und die Prototypen einer Custom-Element-Klasse
                // gewinnen gegen den Host — ohne Umweg ueber den Haken.
                // `constructor` kommt IMMER aus der Prototypkette (DOM §3:
                // `HTMLDivElement.prototype.constructor`). GEMESSEN vorher:
                // `document.createElement('div').constructor.name` ->
                // `Document` (die DOM-Prototyp-Suche fand zuerst
                // `Document.prototype.constructor`); Shady DOM (youtube.com)
                // erkannte so seine Shady-Roots nicht (`constructor` null,
                // `instanceof ShadowRoot` falsch) und fragte das leere
                // Fragment statt des logischen Baums.
                if matches!(name, "createElement" | "createElementNS" | "attachShadow") {
                    let v = self.hole_ohne_host(basis, name)?;
                    if !matches!(v, GVal::Undefiniert) { return Ok(v); }
                }
                // Release 11.10.2026: eine METHODE auf einem Prototyp
                // zwischen dem Objekt und `Element.prototype` (z. B.
                // `HTMLSelectElement.prototype.add`) verdeckt die
                // gleichnamige Methode des Hosts (WebIDL). GEMESSEN vorher:
                // `select.add(option)` rief `classList.add`,
                // `select.remove(0)` entfernte das ganze `<select>`.
                if (self.eigene_lese(r, name).is_some() && name != "__proto__")
                    || name == "constructor"
                    || self.ce_eigene_kette(r, name)
                    // Nur am Element selbst (Art 0): Unterobjekte wie
                    // `classList` behalten ihre Host-Methoden.
                    || (self.host_art(basis) == 0 && self.kette_vor_element_hat(r, name))
                    // Release 2026-10-15: a live collection's prototype
                    // (item, namedItem, forEach …) shadows its named
                    // properties (WebIDL §3.9 named property visibility).
                    // An array index is always the collection's own.
                    || (matches!(self.host_art(basis), 5..=8)
                        && crate::ops::index_aus_schluessel(name).is_none()
                        && self.proto_kette_hat(r, name))
                {
                    return self.hole_ohne_host(basis, name);
                }
            }
            let art = self.host_art(basis);
            // Release 2026-10-15: `length` and indices of a live collection
            // come from a cache while the host's collection stamp holds (a
            // loop over `c[i]` with `c.length` asked the host twice per
            // step).
            let sammlung_platz = if (5..=8).contains(&art)
                && let GVal::Ref(r) = basis
            {
                let platz = if name == "length" { Some(None) } else { crate::ops::index_aus_schluessel(name).filter(|&i| i < 1 << 20).map(Some) };
                match platz {
                    Some(p) => {
                        let stand = h.sammlung_stand();
                        if stand == 0 {
                            None
                        } else {
                            if self.sammlung_cache.0 != stand || self.sammlung_cache.1.len() > 4096 {
                                self.sammlung_cache.0 = stand;
                                self.sammlung_cache.1.clear();
                            }
                            if let Some((laenge, items)) = self.sammlung_cache.1.get(&r) {
                                let treffer = match p {
                                    None => *laenge,
                                    Some(i) => items.get(i).copied().flatten(),
                                };
                                if let Some(v) = treffer {
                                    return Ok(v);
                                }
                            }
                            Some((r, p))
                        }
                    }
                    None => None,
                }
            } else {
                None
            };
            let profil_start = crate::vm::profil_host_start();
            let antwort = h.lesen_mit_art(id, art, name);
            crate::vm::profil_host("lies", name, profil_start);
            if let Some((r, p)) = sammlung_platz {
                let v = match &antwort {
                    Ok(Some(w)) => Some(self.gval_aus_wert(w.clone())),
                    Ok(None) => Some(GVal::Undefiniert),
                    Err(_) => None,
                };
                if let Some(v) = v {
                    let eintrag = self.sammlung_cache.1.entry(r).or_insert_with(|| (None, Vec::new()));
                    match p {
                        None => eintrag.0 = Some(v),
                        Some(i) => {
                            if eintrag.1.len() <= i {
                                eintrag.1.resize(i + 1, None);
                            }
                            eintrag.1[i] = Some(v);
                        }
                    }
                    return Ok(v);
                }
            }
            return match antwort {
                // `None`: der Haken kennt die Eigenschaft nicht — dann
                // geht es normal weiter (undefined, oder ein Feld).
                Ok(etwas) => match etwas {
                    Some(wert) => {
                        let v = self.gval_aus_wert(wert);
                        if art == 0 && name == "attributes" {
                            self.als_named_node_map(v, basis);
                        }
                        if art == 0 {
                            match name {
                                "children" | "forms" | "images" | "links" | "scripts" | "anchors" | "embeds" | "plugins" => {
                                    self.als_sammlung(v, "HTMLCollection");
                                }
                                "childNodes" => self.als_sammlung(v, "NodeList"),
                                _ => {}
                            }
                        }
                        Ok(v)
                    }
                    // J28: `None` heisst beim Haken **nicht** „gibt es
                    // nicht", sondern nur „das weiss der Haken nicht".
                    None => match self.dom_prototyp_funktion(basis, name) {
                        // Der Motor kann die Methode — dann gewinnt der
                        // Motor (siehe die Messung im Bericht).
                        Some(f) => Ok(f),
                        // J-CACHE (05.10.): `window` spiegelt den globalen
                        // Bereich (HTML: window === global scope). Traegt
                        // das Host-Objekt die Marke `__global`, antworten
                        // hier die Engine-Globals (`setTimeout`, `fetch`,
                        // `atob`, …) — sonst waere `window.setTimeout`
                        // tot, obwohl der Motor es kann (www.wikipedia.org:
                        // „setTimeout is not a function" x3). NUR mit
                        // Marke: Elemente sehen Globals NICHT
                        // (`el.fetch` gibt es nicht).
                        None => {
                            let e = self.hole_ohne_host(basis, name)?;
                            if matches!(e, GVal::Undefiniert)
                                && self.ist_global_spiegel(basis)
                            {
                                return self.hole_ohne_host(
                                    GVal::Ref(self.global_objekt),
                                    name,
                                );
                            }
                            Ok(e)
                        }
                    },
                },
                // J28: **der Haken kann die Eigenschaft nicht kennen.**
                //
                // DERSELBE FALL wie oben, nur mit `Err`: der Haken meldet
                // aktiv, dass er die Methode nicht kennt. Auch hier darf er
                // dem Motor seine eigene DOM-Schicht nicht verdecken. Findet
                // der Motor auch nichts, wird der Haken-Fehler **doch**
                // gemeldet — sonst waere ein wirklich unbekanntes Feld still.
                Err(msg) => match self.dom_prototyp_funktion(basis, name) {
                    Some(f) => Ok(f),
                    None => {
                        // Wie oben: window-Spiegel antwortet mit Globals.
                        if self.ist_global_spiegel(basis) {
                            let g = self.hole_ohne_host(GVal::Ref(self.global_objekt), name)?;
                            if !matches!(g, GVal::Undefiniert) {
                                return Ok(g);
                            }
                        }
                        Err(Stoerung::Wurf(self.fehler_wert("TypeError", &msg)))
                    }
                },
            };
        }
        let v = self.hole_ohne_host(basis, name)?;
        if matches!(v, GVal::Undefiniert) && self.proto_html_sammlung.is_some()
            && let Some(benannt) = self.sammlung_name(basis, name)?
        {
            return Ok(benannt);
        }
        Ok(v)
    }

    /// J8: `basis[i]` mit **Zahl** als Schluessel — ohne den Umweg ueber
    /// einen Text.
    ///
    /// GEMESSEN: `LeseIndex` holte den Schluessel als Text
    /// (`text_von(key)`: Zahl -> String, bei Strings eine Kopie des
    /// **ganzen** Inhalts) und liest ihn dann zur Zahl zurueck
    /// (`index_aus_schluessel`). `s = s + a[1]` in 10^6 Runden kostete
    /// 306 ms. Alles, was kein Feld ist, geht weiter den normalen Weg.
    /// JS2 (03.10.2026, 05:45): ein Loch oder ein Index **ausserhalb** des
    /// Feldes ging **nicht** in die Prototypkette.
    ///
    /// GETEILTE DATEI — `eigenschaften.rs`. Ich habe hier **eine Zeile**
    /// geaendert, wie in Runde 6, und melde es an genau zwei Stellen: hier und
    /// in `team/WUNSCH_JS.txt` (05:50). Rueckgaengig in einer Minute.
    ///
    /// ## DER BEFEUND — ein WIDERSPRUCH IM MOTOR, OHNE `node` BEWEISBAR
    ///
    /// ```js
    /// Object.prototype[1] = 1;
    /// var a = [];
    /// a[1]                 // undefined   <- der Index-Zweig
    /// a['1']               // 1           <- der Namens-Zweig
    /// a[1] === a['1']      // false
    /// '1' in a             // true        <- es EXISTIERT
    /// a.foo                // geht, wenn Object.prototype.foo existiert
    /// ```
    ///
    /// `a[1]` und `a['1']` sind **derselbe** Zugriff: `ToPropertyKey(1)` und
    /// `ToPropertyKey("1")` sind beide `"1"`. Der Motor gibt zwei
    /// **verschiedene** Antworten — und der dritte Test sagt ihm, dass die
    /// Eigenschaft **da ist**, deren Wert er nicht liefert. **Drei Aussagen
    /// desselben Motors, die nicht zusammenpassen.** Das ist ohne `node`
    /// beweisbar, weil es die Definition von `in` und von
    /// `ToPropertyKey` benutzt und sonst nichts.
    ///
    /// ## DIE URSACHE, und sie ist die J8-Optimierung
    ///
    /// `hole_index` ist der **schnelle** Weg (J8: „ohne den Umweg ueber einen
    /// Text"). Fuer ein Feld gab er **sofort** zurueck — bei einem Loch
    /// `undefined`, bei einem Index ausserhalb `elemente.get(i)` =
    /// `undefined`. **Die Prototypkette wurde nie betreten.** Der Kettenlauf
    /// steckt in `hole_ohne_host` (ab Zeile 452), und dorthin kommt man nur,
    /// wenn man **nicht** zurueckkehrt.
    ///
    /// **NUR DER LOCH-/AUSSERHALB-ZWEIG IST GEENDERT.** Der Normalfall — der
    /// Index liegt im Feld und ist kein Loch — geht **unveraendert** ueber
    /// `elemente.get(i)`, also ohne Textumweg und ohne Kettenlauf. Das ist der
    /// Grund, warum der Fix die J8-Messung nicht ruiniert: der gemessene
    /// Fall (`s + a[1]` in 10^6 Runden, 306 ms) ist ein Index **im** Feld.
    ///
    /// **UND JETZT GEHT DER LOCH-ZWEIG NACH `hole`,** also in den bereits
    /// **richtigen** Kettenlauf. Ich habe die Regel nicht neu geschrieben,
    /// sondern dem Aufrufer den Weg gezeigt, den `name`-Zweig schon geht.
    pub(crate) fn hole_index(&mut self, basis: GVal, i: usize) -> Result<GVal, Stoerung> {
        if let GVal::Ref(r) = basis
            && let HObj::Feld {
                elemente,
                luecken,
                rest,
                ..
            } = self.heap.holen(r)
        {
            // Normalfall: im Feld, kein Loch. Der J8-Schnellweg.
            //
            // ## W34 Stufe 1 (JS2/QUIC, Runde 92) — **`rest` VOR dem Index.**
            //
            // **GEMESSEN (`node 22`, Runde 88):** ein Index mit abweichenden
            // Attributen oder Accessor liegt in **`rest`**, und der
            // Schnellweg sah ihn nie:
            // ```js
            // Object.defineProperty(a,'1',{get(){throw …}}); a[1]
            //   node: WURF      bei uns: der Getter wurde nie gerufen
            // Object.defineProperty(a,'1',{configurable:false})
            //   .getOwnPropertyDescriptor(a,'1').configurable
            //   node: false      bei uns: true
            // ```
            //
            // **DIE BEIDINGUNG IST `rest.is_empty()` UND NICHT
            // `rest.get(...)`** — damit bleibt der **J8-Schnellweg
            // bitgleich** fuer jedes gewoehnliche Feld (dort ist `rest`
            // leer), und der Fall „Index mit Deskriptor" ist genau der, in dem
            // die Kette **ohnehin** laufen muss. *Ein Leser, der immer zuerst
            // sucht, ist langsamer; ein Leser, der nur dann sucht, wenn es
            // etwas zu suchen gibt, ist es nicht — und der Unterschied ist
            // eine leere `HashMap`.*
            // J59: **die Bedingung ist „kein eigener Eintrag fuer DIESEN
            // Index", nicht „`rest` ist leer".**
            //
            // GEMESSEN (Soll aus node 22), was `rest.is_empty()` kostete:
            // ```js
            // function f(a,b,c){ return arguments[0]; }  f(7,8,9)
            //   node: 7        bei uns: undefined
            // ```
            // Das `arguments`-Objekt legt `callee` in `rest` ab
            // (`argumente_anlegen`, klassen.rs:61), also ist `rest`
            // **nie leer** — der Schnellweg wurde nie genommen, und der
            // Rueckfallweg `hole` lieferte fuer den Index `undefined`.
            // Sichtbar war das nur an `arguments`, denn dort ist `rest`
            // gefuellt; `Object.keys`, `hasOwnProperty`, `join` und
            // `getOwnPropertyDescriptor` sahen die Werte alle (deren Weg
            // laeuft ueber die Kette), nur der normale Get nicht.
            //
            // **Die Reihenfolge der Kurzschaltungen ist der J8-Gewinn:**
            // `rest.is_empty()` steht zuerst, also wird fuer ein gewoehnliches
            // Feld **kein** `String` fuer den Schluessel gebaut — der
            // Schnellweg bleibt bitgleich. Nur wenn `rest` wirklich etwas
            // enthaelt, wird der Schluessel einmal gebaut und geprueft, ob
            // ausgerechnet *dieser* Index dort einen Deskriptor traegt (den
            // Fall aus W34 Stufe 1 oben).
            if !luecken.as_ref().is_some_and(|m| m.contains(&i))
                && let Some(v) = elemente.get(i)
                && (rest.is_empty() || !rest.contains_key(&i.to_string()))
            {
                return Ok(*v);
            }
            // Loch, ausserhalb **oder Deskriptor in `rest`**: die Kette darf
            // nicht fehlen. Also an `hole` — derselbe Weg wie bei `a['1']`.
        }
        self.hole(basis, &i.to_string())
    }

    /// J8: `basis[i] = wert` mit **Zahl** als Schluessel (ohne Textumweg).
    pub(crate) fn setze_index(
        &mut self,
        basis: GVal,
        i: usize,
        wert: GVal,
    ) -> Result<(), Stoerung> {
        if let GVal::Ref(r) = basis
            && matches!(self.heap.holen(r), HObj::Feld { .. })
        {
            if i > MAX_SAMMLUNG {
                return Err(Stoerung::Fehler(JsFehler::Speicher(
                    "Feld-Index zu gross".to_string(),
                )));
            }
            if self.sperre_von(r) == crate::heap::Sperre::Gefroren {
                return self.verweigern("gefrorenes Feld");
            }
            let alt = match self.heap.holen(r) {
                HObj::Feld { elemente, .. } => elemente.len(),
                _ => 0,
            };
            let mut o = self.heap.holen_mut(r);
            if let HObj::Feld { elemente, .. } = &mut *o {
                if i >= elemente.len() {
                    elemente.resize(i + 1, GVal::Undefiniert);
                }
                elemente[i] = wert;
            }
            crate::heap::feld_luecken_von_bis(&mut o, alt, i);
            crate::heap::feld_luecke_loeschen(&mut o, i);
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            return Ok(());
        }
        self.setze(basis, &i.to_string(), wert)
    }

    /// J8: `hole` **mit** Inline-Cache (siehe `op::IcEintrag`).
    ///
    /// `platz` ist der Textindex des Namens (die Zugriffsstelle). Passt der
    /// Eintrag, wird der Wert direkt aus der Property-Map **neu** gelesen —
    /// kein Host-Haken, kein Kettenlauf, keine Namenskopie. Veraltet sich
    /// der Eintrag (Eigenschaft geloescht oder zum Getter geworden), greift
    /// der normale Weg.
    pub(crate) fn hole_ic(
        &mut self,
        basis: GVal,
        name: &str,
        platz: usize,
    ) -> Result<GVal, Stoerung> {
        if let GVal::Ref(r) = basis
            && let Some(ic) = self.inline_cache.get(platz).copied()
            && ic.eigen
            && ic.objekt == Some(r)
            && let Some(e) = self.eigene_lese(r, name)
        {
            // Ein Getter gehoert in den langen Weg (der ruft ihn mit `this`).
            let ist_getter = match self.heap.holen(r) {
                HObj::Gegenstand { eigenschaften, .. } => {
                    eigenschaften.get(name).is_some_and(|x| x.getter.is_some())
                }
                HObj::Feld { rest, .. } => rest.get(name).is_some_and(|x| x.getter.is_some()),
                _ => true,
            };
            if !ist_getter {
                return Ok(e);
            }
        }
        let wert = self.hole(basis, name)?;
        self.ic_fuellen(platz, basis, name);
        Ok(wert)
    }

    /// Cache-Eintrag nach einem Zugriff schreiben (J8): nur wenn das
    /// Objekt die Eigenschaft **selbst** traegt (nicht ueber die Kette) und
    /// es eine Daten-Eigenschaft ist.
    fn ic_fuellen(&mut self, platz: usize, basis: GVal, name: &str) {
        let GVal::Ref(r) = basis else { return };
        let (eigen, daten) = match self.heap.holen(r) {
            HObj::Gegenstand { eigenschaften, .. } => eigenschaften
                .get(name)
                .map(|e| (true, e.getter.is_none()))
                .unwrap_or((false, false)),
            HObj::Feld { rest, .. } => rest
                .get(name)
                .map(|e| (true, e.getter.is_none()))
                .unwrap_or((false, false)),
            _ => (false, false),
        };
        if eigen
            && daten
            && let Some(ic) = self.inline_cache.get_mut(platz)
        {
            *ic = crate::op::IcEintrag {
                objekt: Some(r),
                eigen: true,
            };
        }
    }

    /// `hole` ohne den Host-Haken — der Rest der Aufloesung.
    pub(crate) fn hole_ohne_host(&mut self, basis: GVal, name: &str) -> Result<GVal, Stoerung> {
        // Synthetische Funktions-Props (Benutzer + nativ).
        //
        // J8: **ohne** `self.heap.holen(r).clone()`. GEMESSEN: der Klon
        // kopierte je Eigenschaftszugriff das GANZE `HObj` — bei einem
        // `Gegenstand` also die Property-Map UND die Reihenfolgeliste
        // (7 Allokationen fuer `{a:1,b:2,c:3}`). Ein Feldzugriff kostete
        // 629 ns je Durchlauf (`o.a` in einer Schleife, 10^6 Runden).
        if let GVal::Ref(r) = basis {
            match self.heap.holen(r) {
                HObj::Funktion {
                    name: n,
                    fn_index,
                    prototyp_objekt,
                    eigenschaften,
                    ..
                } => {
                    // J27: die **echte** eigene Eigenschaft gewinnt vor dem
                    // Notwert. GEMESSEN, warum das noetig ist: `hole`
                    // lieferte fuer `Nativ` fest `length = 0`, auch wenn
                    // `nativ_anlegen` die Parameterzahl als echte
                    // Eigenschaft angelegt hatte — `Array.from.length`
                    // las sich als `0` statt `1`, waehrend
                    // `Object.getOwnPropertyDescriptor` korrekt `1` sagte.
                    // Zwei Wege, zwei Zahlen: genau das, was die Suite mit
                    // `verifyProperty` auseinanderhaelt.
                    //
                    // GEMESSEN, warum **nur** eine Daten-Eigenschaft hier
                    // zurueckkehrt: `Symbol.iterator` ist ein **Getter**
                    // (`nativ_getter` in `builtin.rs`) — die erste Fassung
                    // gab `e.wert` zurueck, das ist `undefined`, und
                    // `typeof Symbol.iterator` wurde `undefined` statt
                    // `symbol`. Ein Getter gehoert in den normalen Weg
                    // weiter unten, der ihn mit `this` aufruft.
                    if let Some(e) = eigenschaften.get(name) {
                        if e.getter.is_none() {
                            return Ok(e.wert);
                        }
                    }
                    if name == "name" {
                        let n = n.clone();
                        return self.text_wert(&n).map_err(Stoerung::Fehler);
                    }
                    if name == "length" {
                        // V6: die Parameterzahl steht im Bytecode.
                        let l = self
                            .programm
                            .as_ref()
                            .and_then(|p| p.funktionen.get(*fn_index))
                            .map(|d| d.parameter)
                            .unwrap_or(0);
                        return Ok(GVal::Zahl(l as f64));
                    }
                    if name == "prototype" {
                        return Ok(prototyp_objekt.map(GVal::Ref).unwrap_or(GVal::Undefiniert));
                    }
                }
                HObj::Nativ {
                    name: n,
                    eigenschaften,
                    ..
                } => {
                    // Wie im `Funktion`-Zweig: nur eine Daten-Eigenschaft
                    // kehrt hier zurueck, ein Getter laeuft im normalen
                    // Weg (Begruendung dort).
                    if let Some(e) = eigenschaften.get(name) {
                        if e.getter.is_none() {
                            return Ok(e.wert);
                        }
                    }
                    if name == "name" {
                        let n = n.clone();
                        return self.text_wert(&n).map_err(Stoerung::Fehler);
                    }
                    if name == "length" {
                        // GEMESSEN (node 22): `Function.prototype.length` ist
                        // 0, und ein Native ohne `nativ_anlegen`-Eintrag
                        // kennt seine Parameterzahl nicht. 0 ist hier die
                        // ehrliche Untergrenze, keine erfundene Zahl.
                        return Ok(GVal::Zahl(0.0));
                    }
                    if name == "prototype" {
                        return Ok(self
                            .nativ_proto
                            .get(&r)
                            .copied()
                            .map(GVal::Ref)
                            .unwrap_or(GVal::Undefiniert));
                    }
                }
                _ => {}
            }
        }
        // Primitive: Box-Prototyp + Spezialfaelle (String-Index/laenge).
        let obj_ref = match basis {
            GVal::Ref(r) => r,
            GVal::Zahl(n) => {
                if name == "constructor" {
                    return Ok(GVal::Ref(self.ctor_zahl));
                }
                let _ = n;
                return self.proto_hole(self.proto_zahl, name, basis);
            }
            // J24: ein BigInt hat seinen **eigenen** Prototyp
            // (`toString` mit Radix, `valueOf`) — `constructor` zeigt wie
            // bei den Zahlen auf den BigInt-Konstruktor.
            GVal::Ganz(_) => {
                if name == "constructor" {
                    return Ok(GVal::Ref(self.ctor_bigint));
                }
                return self.proto_hole(self.proto_bigint, name, basis);
            }
            GVal::Bool(b) => {
                if name == "constructor" {
                    return Ok(GVal::Ref(self.ctor_bool));
                }
                let _ = b;
                return self.proto_hole(self.proto_bool, name, basis);
            }
            GVal::Undefiniert | GVal::Null => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", &format!("kein Zugriff auf {basis:?} (Eigenschaft {name})")),
                ));
            }
        };
        // String-Objekt: Index + length synthetisch.
        //
        // J8b: der alte Weg schrieb `self.heap.holen(obj_ref).clone()` **vor**
        // das Muster — damit kopierte JEDER Lesezugriff das GANZE `HObj`,
        // auch bei Karten, Mengen und Objekten (Property-Map +
        // Reihenfolgeliste; bei einer Karte sogar alle Eintraege und der
        // Index). GEMESSEN: `m.size` auf einer Karte mit 4000 Eintraegen
        // kostete **386 us je Zugriff** (100 ns je Eintrag), `s.length` auf
        // einem 4000-Zeichen-Text 2,4 us, `a.length` auf einem 4000-
        // Elementen-Feld 3,7 us — alles linear in der Container-Groesse,
        // obwohl nur eine Zahl zurueckkommt. Jetzt wird erst die **Art**
        // geprueft (eine Discriminant-Pruefung, kostenlos) und nur im
        // Textfall kopiert.
        // Release 15.10.2026: `length` and `s[i]` read the string in place
        // (`text_einheiten`, an ASCII fast path) instead of copying it and
        // collecting its UTF-16 units on every read. MEASURED before: each
        // `s.length` / `s[i]` cloned the whole string — a loop over a 20 KB
        // text was quadratic in allocations (bench_text.js, see
        // release_1015_es.rs::string_positions_count_utf16_units).
        if matches!(self.heap.holen(obj_ref), HObj::Zeichenkette(_)) {
            if name == "length" {
                let n = self.text_masse(obj_ref).map_or(0, |(_, n)| n);
                return Ok(GVal::Zahl(n as f64));
            }
            if let Some(i) = index_aus_schluessel(name) {
                return match self.text_stelle(obj_ref, i) {
                    Some(crate::text_einheiten::Stelle::Zeichen(c)) => {
                        let mut puffer = [0u8; 4];
                        self.text_wert(c.encode_utf8(&mut puffer)).map_err(Stoerung::Fehler)
                    }
                    Some(crate::text_einheiten::Stelle::Zweite(_)) => self.text_wert("\u{FFFD}").map_err(Stoerung::Fehler),
                    None => Ok(GVal::Undefiniert),
                };
            }
            if name == "constructor" {
                return Ok(GVal::Ref(self.ctor_text));
            }
            if let Some(e) = self.eigene_proto_hole(self.proto_text, name)? {
                // NUR echte Getter aufrufen; Daten (Methoden!) direkt geben.
                if let Some(g) = e.getter {
                    return self.rufe_getter(g, basis);
                }
                return Ok(e.wert);
            }
            return Ok(GVal::Undefiniert);
        }
        // Verpackte Strings: Elemente + length.
        //
        // J8b: wie oben — erst die Art, dann die Kopie.
        if matches!(
            self.heap.holen(obj_ref),
            HObj::Verpackt {
                elemente: Some(_),
                ..
            }
        ) && let HObj::Verpackt {
            elemente: Some(el), ..
        } = self.heap.holen(obj_ref).clone()
        {
            if name == "length" {
                // ## W30 (JS2/QUIC, Runde 74, ANGEKUENDIGT in WUNSCH_JS.txt):
                // die **eigene** `length` zuerst — sie ist in **UTF-16-Einheiten**
                // gezählt (ES2023 10.4.3), `el.len()` zählt Rust-`char`.
                //
                // GEMESSEN (`node 22`, mit `T262_NUR` isoliert, 9,6 ms):
                // ```js
                // new String('\uD834\uDF06').length   node 2   vorher 1
                // ```
                // `elemente` hat **ein** Element je `char`, ein astralisches
                // Zeichen ist also **ein** Element.
                //
                // **WARUM DIE EIGENE UND NICHT `el.len()`:** den Wert gibt es
                // bereits — `n_text_ctor` (`builtin2.rs`) legt ihn mit
                // `crate::regexmini::als_einheiten(&s).len()` an, also
                // **in UTF-16-Einheiten** und ueber **dieselbe** Funktion,
                // ueber die `String.fromCharCode` seine Laenge bekommt.
                // *Eine Huelle soll ihre `length` **einmal** berechnen, nicht
                // bei jedem Zugriff neu — sonst sind es zwei Werte mit
                // demselben Namen.*
                //
                // **DER RUECKFALL BLEIBT** fuer `Number`-/`Boolean`-Hullen
                // (`verpackt(Boxen::Zahl)`, `elemente: None`): dort laeuft
                // dieser Block gar nicht. Er wird nur nicht mehr **bevorzugt**.
                if let HObj::Verpackt {
                    eigenschaften: e, ..
                } = self.heap.holen(obj_ref)
                    && let Some(eg) = e.get("length")
                {
                    return Ok(eg.wert);
                }
                return Ok(GVal::Zahl(el.len() as f64));
            }
            if let Some(i) = index_aus_schluessel(name) {
                if let Some(v) = el.get(i) {
                    return Ok(*v);
                }
            }
        }
        // TypedArray-Index-Zugriff (vor der Prototyp-Kette).
        //
        // ES2023 23.2.5: TypedArray-Indizes sind **keine** eigenen
        // Eigenschaften im Property-Map-Sinn — sie liegen im geteilten
        // BytePuffer. Deshalb muss der Zugriff hier (wie bei Feld) **vor**
        // der Kette abgefangen werden.
        if matches!(self.heap.holen(obj_ref), HObj::TypedArray { .. }) {
            if let HObj::TypedArray {
                art,
                puffer,
                versatz,
                laenge,
                ..
            } = self.heap.holen(obj_ref)
            {
                if let Some(i) = index_aus_schluessel(name) {
                    if i < *laenge {
                        let element_bytes = art.bytes();
                        if let HObj::BytePuffer { daten, .. } = self.heap.holen(*puffer) {
                            let byte_offset = *versatz + i * element_bytes;
                            if byte_offset + element_bytes <= daten.len() {
                                return Ok(art.lesen(daten, byte_offset));
                            }
                        }
                        return Ok(GVal::Undefiniert);
                    }
                    // Ausserhalb der Laenge -> Undefiniert (keine Kette)
                    return Ok(GVal::Undefiniert);
                }
            }
        }
        // Normale Kette (eigene Props + Getter + Proto).
        let mut cur = Some(obj_ref);
        let mut rest = 10_000; // M3: Proto-Ketten-Deckel.
        while let Some(o) = cur {
            rest -= 1;
            if rest == 0 {
                return Err(Stoerung::Fehler(JsFehler::Laufzeit(
                    "Proto-Kette zu lang".to_string(),
                )));
            }
            // Release 10.10.2026: ein Proxy beantwortet `[[Get]]` selbst —
            // als Basis und als Prototyp (Empfaenger bleibt `basis`).
            if matches!(self.heap.holen(o), HObj::Proxy { .. }) {
                return self.proxy_get(o, name, basis);
            }
            // Feld-Index/laenge.
            //
            // JS2 (03.10.2026, 06:00): **`luecken` wird mitgeprueft.** Vorher
            // stand hier `HObj::Feld { elemente, .. }` — ohne die
            // Loch-Map. Ein Loch **innerhalb** von `elemente` (etwa durch
            // `a.length = 3` bei `a = [0]`) traf also `elemente.get(i)` und
            // gab `Undefiniert` zurueck, **ohne** die Kette zu betreten.
            //
            // GEMESSEN 05:55, `Array/prototype/concat/S15.4.4.4_A3_T2.js`:
            // ```js
            // var a = [0]; a.length = 3;      // Indizes 1,2 sind Luecken
            // Array.prototype[2] = 2;
            // a[2]                              // muss 2 sein
            // ```
            // Bei uns `undefined`. Und `a['2']` war `2` — **derselbe
            // Widerspruch wie in `hole_index`**, an der zweiten Stelle.
            //
            // **DIE REGEL IST EINMAL:** ein **Loch** geht in die Kette, wie
            // ein Index ausserhalb. Beide sind „nicht eigen". `elemente.get(i)`
            // liefert fuer ein Loch `Some(Undefiniert)` — und **genau das**
            // war der Fehler: `Undefiniert` ist ein Wert, kein „fehlt".
            if let HObj::Feld {
                elemente,
                luecken,
                rest,
                ..
            } = self.heap.holen(o)
            {
                if name == "length" {
                    // ## W50: **die Property zuerst.** Bei einer Laenge ueber
                    // `MAX_SAMMLUNG` steht die Zahl in `rest["length"]`, und
                    // `elemente` ist **nicht** nachgefuehrt worden (ein Loch
                    // kostet nichts, 4 Milliarden davon schon). *Zwei Quellen fuer
                    // dieselbe Zahl sind zwei Wahrheiten — die Property gewinnt,
                    // wenn sie da ist, weil nur sie gross sein kann.*
                    if let Some(crate::heap::Eigenschaft { wert, .. }) = rest.get("length") {
                        return Ok(*wert);
                    }
                    return Ok(GVal::Zahl(elemente.len() as f64));
                }
                if let Some(i) = index_aus_schluessel(name) {
                    // Normalfall: im Feld und **kein** Loch.
                    //
                    // ## W34 Stufe 1 (JS2/QUIC, Runde 93) — `rest` **vor** dem
                    // Index. Siehe `hole_index` (Zeile 222) fuer Messung und
                    // Begruendung.
                    //
                    // J59: **auch hier** ist die Bedingung „kein eigener
                    // Eintrag fuer DIESEN Index", nicht „`rest` ist leer".
                    // Beide Stellen trugen denselben Fehler, und beide
                    // braucht man: `hole_index` (Zeile 222) verarbeitet den
                    // **Zahl**-Schluessel, dieser Arm den **Text**-Schluessel
                    // — welcher davon genommen wird, haengt am Bytecode.
                    //
                    // GEMESSEN (Soll node 22, Beleg in
                    // team/BERICHT_JS_J59.txt):
                    // ```js
                    // function f(a,b,c){ return arguments[0]; }  f(7,8,9)
                    //   node: 7        bei uns: undefined
                    // ```
                    // `argumente_anlegen` (klassen.rs:61) legt `callee` in
                    // `rest` ab, also ist `rest` beim `arguments`-Objekt
                    // **nie leer** und der Schnellweg wurde nie genommen.
                    // Sichtbar war es nur dort — `Object.keys`,
                    // `hasOwnProperty`, `join` und
                    // `getOwnPropertyDescriptor` gingen ueber die Kette und
                    // sahen die Werte.
                    //
                    // **Die Kurzschaltung `rest.is_empty()` bleibt zuerst:**
                    // fuer ein gewoehnliches Feld wird kein Schluessel-String
                    // gebaut, der J8-Schnellweg bleibt bitgleich. Nur wenn
                    // `rest` etwas enthaelt, wird der Schluessel geprueft, ob
                    // *dieser* Index dort einen Deskriptor traegt — der Fall
                    // aus W34 Stufe 1.
                    if (rest.is_empty() || !rest.contains_key(name))
                        && !luecken.as_ref().is_some_and(|m| m.contains(&i))
                        && let Some(v) = elemente.get(i)
                    {
                        return Ok(*v);
                    }
                    // Loch, ausserhalb **oder Deskriptor in `rest`**: Kette
                    // (toString, Array.prototype[2] usw. fallen durch).
                    //
                    // ## W34 Stufe 3 (QUIC, 03.10.2026, ~22:4x) — **DER
                    // EIGENE `rest`-EINTRAG WURDE HIER NIE GEHOLT.**
                    //
                    // Der Sprung auf den Prototyp ist richtig, wenn `rest` den
                    // Schluessel **nicht** traegt: dann gibt es nichts Eigenes,
                    // und die Kette ist der einzige Weg. Traegt `rest` ihn aber,
                    // ist der **eigene** Deskriptor/Accessor genau der Fall, um
                    // den es hier geht — und `continue` hat ihn uebersprungen.
                    //
                    // **GEMESSEN, node 22 als Soll — und es ist Datenverlust,
                    // kein fehlendes Feature:**
                    // ```js
                    // var a = [1, 2]; var c = 0;
                    // Object.defineProperty(a, '1', {get: function () { c = c + 1; return 9; }});
                    // a[1]        node: 9          bei uns (vorher): undefined
                    // c            node: 1          bei uns (vorher): 0
                    // ```
                    // `a[1]` war **2** und wurde nach dem `defineProperty`
                    // **undefined**: der Wert war weg. Grund: der Prototyp hat
                    // keine "1", die Kette lief leer aus (`Ok(Undefiniert)`,
                    // Zeile 727) — und `elemente[1]` wurde auch nicht mehr
                    // gelesen, weil der Normalfall oben an `rest` scheitert.
                    //
                    // **DER BEWEIS, dass es an dieser einen Zeile lag:** die
                    // Zwillingsstelle `eigene_oder_proto` (Zeile 752ff) hat
                    // denselben Normalfall und **faellt bei `rest`-Treffer
                    // auf `objekt_eigene_proto` durch** — deshalb sah
                    // `Object.keys`, `hasOwnProperty` und
                    // `getOwnPropertyDescriptor` den Wert und nur der normale
                    // Get nicht. Zwei gleich gebaute Stellen, eine davon
                    // falsch: das ist der Beweis, dass es nicht am Konzept lag.
                    //
                    // **MINIMAL, und die Reihenfolge ist der ganze Fix:**
                    // `rest.contains_key(name)` **zuerst** — der eigene
                    // Eintrag geht nach unten in `objekt_eigene_proto`
                    // (Zeile 710), das `getter` aufruft und `wert` liefert.
                    // Sonst unveraendert der Prototyp-Sprung. Fuer ein
                    // gewoehnliches Feld ist `rest` leer, die Bedingung ist
                    // `false`, und der **J8-Schnellweg oben ist bitgleich**.
                    if !rest.contains_key(name) {
                        cur = self.heap_proto_von(o);
                        continue;
                    }
                    // **`rest` traegt den Schluessel:** der eigene Deskriptor
                    // wird jetzt geholt — Getter laeuft, `wert` gilt.
                }
            }
            let (eigene, proto) = self.objekt_eigene_proto(o, name);
            if let Some(e) = eigene {
                if let Some(g) = e.getter {
                    return self.rufe_getter(g, basis);
                }
                return Ok(e.wert);
            }
            // Funktions-Prototyp-Objekt weiter oben behandelt.
            cur = proto;
            // Funktions-`prototype`-Kette: HObj::Funktion ohne Map — weiter
            // zum Funktions-Prototyp.
            if let Some(c) = cur {
                let _ = c;
            } else if matches!(self.heap.holen(o), HObj::Funktion { .. }) {
                cur = Some(self.proto_funktion);
            }
        }
        Ok(GVal::Undefiniert)
    }

    /// Eigene ODER vererbte Eigenschaft, roh gelesen — Getter werden dabei
    /// aufgerufen (this=basis). Gibt None bei Fehlen.
    pub(crate) fn eigene_oder_proto(
        &mut self,
        basis: GVal,
        name: &str,
    ) -> Result<Option<GVal>, Stoerung> {
        let r = match basis {
            GVal::Ref(r) => r,
            _ => return Ok(None),
        };
        let mut cur = Some(r);
        let mut rest = 10_000;
        while let Some(o) = cur {
            rest -= 1;
            // `KEIN_REF` heisst: Kette endet (kein Panic, kein Unsinn).
            if o == crate::heap::KEIN_REF {
                break;
            }
            if rest == 0 {
                return Ok(None);
            }
            // Release 14.10.2026: a proxy (as the object or on its prototype
            // chain) answers through its `get` trap / target. MEASURED
            // before: ToPrimitive found no `toString`/`valueOf` on ANY
            // proxy — `String(proxy)`, `${proxy}` and `"x" + proxy` threw
            // "Objekt nicht in Primitiv wandelbar" (Vue 3 reactive objects,
            // WPT HTMLOptionsCollection).
            if matches!(self.heap.holen(o), HObj::Proxy { .. }) {
                let v = self.proxy_get(o, name, basis)?;
                return Ok(if matches!(v, GVal::Undefiniert) { None } else { Some(v) });
            }
            if let HObj::Feld { elemente, rest, .. } = self.heap.holen(o) {
                if name == "length" {
                    return Ok(Some(GVal::Zahl(elemente.len() as f64)));
                }
                if let Some(i) = index_aus_schluessel(name) {
                    // ## W34 Stufe 1 (JS2/QUIC, Runde 92) — `rest` **vor** dem
                    // Index. Siehe `hole_index` (Zeile 222) fuer die Messung
                    // und die Begruendung: **`rest.is_empty()`** als Bedingung,
                    // damit der **J8-Schnellweg** fuer gewoehnliche Felder
                    // **bitgleich** bleibt und nur der Fall „Index mit
                    // Deskriptor oder Accessor" in die Kette geht.
                    if rest.is_empty()
                        && let Some(v) = elemente.get(i).copied()
                    {
                        return Ok(Some(v));
                    }
                    // `rest` **nicht** leer: die Kette entscheidet — sie liest
                    // `rest` und damit auch den Deskriptor/Accessor.
                }
            }
            let (eigene, p) = self.objekt_eigene_proto(o, name);
            if let Some(e) = eigene {
                if let Some(g) = e.getter {
                    return self.rufe_getter(g, basis).map(Some);
                }
                return Ok(Some(e.wert));
            }
            cur = p;
            if cur.is_none()
                && matches!(
                    self.heap.holen(o),
                    HObj::Funktion { .. } | HObj::Nativ { .. }
                )
            {
                // Funktions-Prototyp-Kette weiter oben in hole() — hier
                // nur proto_funktion als Fallback.
                cur = Some(self.proto_funktion);
                // Endlosschleife vermeiden: nur einmal.
                if o == self.proto_funktion {
                    break;
                }
            }
        }
        Ok(None)
    }

    pub(crate) fn objekt_eigene_proto(
        &self,
        o: HeapRef,
        name: &str,
    ) -> (Option<Eigenschaft>, Option<HeapRef>) {
        match self.heap.holen(o) {
            HObj::Gegenstand {
                proto,
                eigenschaften,
                ..
            } => (eigenschaften.get(name).cloned(), *proto),
            HObj::Verpackt {
                proto,
                eigenschaften,
                ..
            } => (eigenschaften.get(name).cloned(), *proto),
            HObj::Feld { proto, rest, .. } => (rest.get(name).cloned(), *proto),
            // W16 (JS, 04.10.2026): ein Byte-Puffer traegt dieselbe
            // Zusatz-Tabelle wie die anderen Varianten (`byteLength` am
            // Prototyp ist ein Getter, an einem Buffer selbst gibt es keine
            // eigenen Felder).
            HObj::BytePuffer {
                proto,
                eigenschaften,
                ..
            } => (eigenschaften.get(name).cloned(), *proto),
            // ## W55: der eigene Zugriff (Accessor-Index) **plus** die Kette.
            // `length`/`byteLength`/`buffer` sind **Getter auf dem Prototyp**
            // (die hat `getter: Some(..)`), also braucht dieser Arm die
            // Property-Map **und** `proto` — sonst sieht `ta.length` weder
            // seinen Getter noch `ta[0]`.
            HObj::TypedArray {
                proto,
                eigenschaften,
                ..
            } => (eigenschaften.get(name).cloned(), *proto),
            HObj::Funktion {
                proto,
                eigenschaften,
                ..
            }
            | HObj::Nativ {
                proto,
                eigenschaften,
                ..
            } => (eigenschaften.get(name).cloned(), *proto),
            HObj::Generator {
                proto,
                eigenschaften,
                ..
            }
            | HObj::Fehler {
                proto,
                eigenschaften,
                ..
            }
            | HObj::Datum {
                proto,
                eigenschaften,
                ..
            }
            | HObj::Regex {
                proto,
                eigenschaften,
                ..
            }
            | HObj::Karte {
                proto,
                eigenschaften,
                ..
            }
            | HObj::Menge {
                proto,
                eigenschaften,
                ..
            }
            | HObj::Versprechen {
                proto,
                eigenschaften,
                ..
            } => (eigenschaften.get(name).cloned(), *proto),
            HObj::Umgebung { .. } | HObj::Zeichenkette(_) | HObj::Proxy { .. } => (None, None),
        }
    }

    /// J56: der Prototyp eines Heap-Objekts.
    ///
    /// GEMESSEN (Ursachenliste, **142** Suite-Faelle, `RUST_BACKTRACE=1` auf
    /// `built-ins/Array/from/Array.from-descriptor.js`):
    /// ```text
    /// panicked at crates/narsil-jsvm/src/heap.rs:725:19:
    /// index out of bounds: the len is 1008 but the index is 4294967295
    ///   at heap.rs:725        (holen)
    ///   at eigenschaften.rs:812  (heap_proto_von)
    ///   at eigenschaften.rs:2468 (instanceof)
    ///   at vm.rs:1181            (Op::InstanceVon)
    /// ```
    /// `4294967295` ist `KEIN_REF`, also ein **nicht belegter** Slot: ein
    /// Objekt traegt `proto: Some(KEIN_REF)` statt `None`. Der Kettenlauf
    /// rief darauf `holen` und **panickte** — ein M3-Verstoss (ein Skript
    /// darf den Motor nie zum Absturz bringen).
    ///
    /// Die Regel ist damit an **einer** Stelle festgelegt: `KEIN_REF` ist
    /// wie `None` zu behandeln. Das ist keine Notloesung, sondern die
    /// Bedeutung des Platzhalters (unbekannter/ungesetzter Prototyp).
    pub(crate) fn heap_proto_von(&self, o: HeapRef) -> Option<HeapRef> {
        if o == KEIN_REF {
            return None;
        }
        // Release 10.10.2026: ein Proxy zeigt (ohne Falle — diese Funktion
        // ruft kein JS) den Prototyp seines Ziels; `Object.getPrototypeOf`
        // und `Reflect.getPrototypeOf` fragen die Falle (`proxy_nativ`).
        if let HObj::Proxy { ziel, .. } = self.heap.holen(o) {
            return ziel.and_then(|z| if z == o { None } else { self.heap_proto_von(z) });
        }
        match self.heap.holen(o) {
            HObj::Gegenstand { proto, .. }
            | HObj::Verpackt { proto, .. }
            | HObj::Feld { proto, .. }
            | HObj::Funktion { proto, .. }
            | HObj::Nativ { proto, .. }
            | HObj::Fehler { proto, .. }
            | HObj::Datum { proto, .. }
            | HObj::Regex { proto, .. }
            | HObj::Karte { proto, .. }
            | HObj::Menge { proto, .. }
            | HObj::Versprechen { proto, .. }
            // ## W53 (JS, 05.10.2026) — `BytePuffer` FEHLTE, und der `_`-Arm
            // hat es **stillschweigend** geschluckt.
            //
            // `HObj::BytePuffer` ist in JS_12 von mir hinzugekommen; an diese
            // Liste habe ich nicht gedacht. Der `_ => None`-Arm **kompiliert**
            // und liefert `None` — also ist die **ganze** Prototyp-Kette eines
            // `ArrayBuffer` unsichtbar. GEMESSEN, vorher:
            // ```text
            // new ArrayBuffer(4) instanceof ArrayBuffer          -> false (node: true)
            // new SharedArrayBuffer(4) instanceof ArrayBuffer    -> true  (node: false)
            // Object.getPrototypeOf(new ArrayBuffer(4)) === ArrayBuffer.prototype -> false
            // ```
            //
            // **DIE REGEL, die ich daraus ziehe und die mehr wert ist als der
            // Fix:** eine neue `HObj`-Variante ist **nicht** fertig, wenn sie
            // kompiliert. Sie ist fertig, wenn sie in **jeder** Liste steht, die
            // Varianten aufzählt — und `_`-Arme sind dabei die **teuerste**
            // Stelle, weil sie genau das verschweigen.
            | HObj::BytePuffer { proto, .. }
            // ## W55: **vor** der eigenen Liste, aus demselben Grund wie
            // `BytePuffer` (siehe dort): ohne die Kette sieht `ta.length`
            // seinen Getter nicht, und `ta instanceof Uint8Array` ist `false`.
            | HObj::TypedArray { proto, .. }
            | HObj::Generator { proto, .. } => match proto {
                Some(q) if *q != KEIN_REF => Some(*q),
                _ => None,
            },
            _ => None,
        }
    }

    /// Eigenschaft auf Proto-Kette suchen (roh, OHNE Getter-Aufruf).
    /// Der Aufrufer unterscheidet Daten (`.wert`) von Zugriff
    /// (`.getter`/`.setter`) — Methoden duerfen beim blossen
    /// Nachschlagen NICHT aufgerufen werden.
    pub(crate) fn eigene_proto_hole(
        &mut self,
        proto: HeapRef,
        name: &str,
    ) -> Result<Option<Eigenschaft>, Stoerung> {
        let mut cur = Some(proto);
        let mut rest = 10_000;
        while let Some(o) = cur {
            rest -= 1;
            if o == crate::heap::KEIN_REF {
                break;
            }
            if rest == 0 {
                return Ok(None);
            }
            let (eigene, p) = self.objekt_eigene_proto(o, name);
            if eigene.is_some() {
                return Ok(eigene);
            }
            cur = p;
        }
        Ok(None)
    }

    pub(crate) fn proto_hole(
        &mut self,
        proto: HeapRef,
        name: &str,
        this: GVal,
    ) -> Result<GVal, Stoerung> {
        match self.eigene_proto_hole(proto, name)? {
            Some(e) if e.getter.is_some() => self.rufe_getter(e.getter.unwrap(), this),
            Some(e) => Ok(e.wert),
            None => Ok(GVal::Undefiniert),
        }
    }

    pub(crate) fn ist_getter_ref(&self, proto: HeapRef, name: &str) -> bool {
        let mut cur = Some(proto);
        let mut rest = 1000;
        while let Some(o) = cur {
            rest -= 1;
            if rest == 0 {
                break;
            }
            let (eigene, p) = self.objekt_eigene_proto(o, name);
            if let Some(e) = eigene {
                return e.getter.is_some();
            }
            cur = p;
        }
        false
    }

    /// EIGENSCHAFT (nicht Getter-Funktion!) als Methode rufen — nur fuer
    /// echte Zugriffs-Getter. Daten-Eigenschaften gibt der Aufrufer direkt
    /// zurueck (siehe hole()).
    pub(crate) fn getter_aufrufen(
        &mut self,
        v: GVal,
        this: GVal,
        _name: &str,
    ) -> Result<GVal, Stoerung> {
        // `eigene_proto_hole` gibt bei Getter die Ref zurueck — hier als
        // Methode aufrufen. Vereinfachung: Getter-Refs werden wie Methoden
        // gerufen (this=String-Box ist ok).
        if self.ist_aufrufbar(v) {
            let marke = self.wurzel_merke();
            self.wurzeln.push(v);
            self.wurzeln.push(this);
            let r = self.aufruf_synchron(v, this, &[], false);
            self.wurzel_zurueck(marke);
            r
        } else {
            Ok(v)
        }
    }

    pub(crate) fn rufe_getter(&mut self, g: HeapRef, this: GVal) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(g));
        self.wurzeln.push(this);
        let r = self.aufruf_synchron(GVal::Ref(g), this, &[], false);
        self.wurzel_zurueck(marke);
        r
    }

    /// Die Host-Id eines `GVal`, wenn es ein Host-Objekt ist.
    ///
    /// GEMESSEN, warum das NICHT ueber `gval_als_wert` laeuft (der Fehler
    /// um 13:40): `gval_als_wert` gibt fuer einen `Ref` auf ein Objekt
    /// `None` zurueck — es kann nur Texte. Ein Host-Objekt IST aber ein
    /// Objekt im Heap, mit dem Feld `__host`. Der erste Versuch suchte
    /// die Id ueber `Wert::Gegenstand` und fand NIE etwas.
    ///
    /// M3: der Zugriff geht ueber `heap.holen` (kein Clone des ganzen
    /// Objekts); die Feldliste eines Objekts ist durch `MAX_EIGEN`
    /// begrenzt, und `__host` liegt in ihr.
    /// Release 2026-10-15: whether `name` is on the prototype chain of `r`
    /// (not on `r` itself).
    pub(crate) fn proto_kette_hat(&self, r: HeapRef, name: &str) -> bool {
        let mut p = self.heap_proto_von(r);
        for _ in 0..64 {
            let Some(x) = p else { return false };
            if self.eigene_lese(x, name).is_some() {
                return true;
            }
            p = self.heap_proto_von(x);
        }
        false
    }

    /// Release 2026-10-15: a host-provided own property of a list object —
    /// an index of a live collection (art 5/6) or of `classList` (art 2), or
    /// a visible named property of a live HTMLCollection (art 5; WebIDL
    /// §3.9 "named property visibility": no own property and nothing on the
    /// prototype chain shadows it). The value and whether it is an index.
    pub(crate) fn host_eigene(&mut self, basis: GVal, name: &str) -> Option<(GVal, bool)> {
        let art = self.host_art(basis);
        if !matches!(art, 2 | 5..=8) || name.starts_with("__") || name == "length" || crate::heap::ist_symbolschluessel(name) {
            return None;
        }
        let GVal::Ref(r) = basis else { return None };
        let id = self.host_id_von_gval(basis)?;
        let index = crate::ops::index_aus_schluessel(name).is_some();
        if !index && (!matches!(art, 5 | 7) || self.eigene_lese(r, name).is_some() || self.proto_kette_hat(r, name)) {
            return None;
        }
        let h = self.host_objekte.clone()?;
        match h.lesen_mit_art(id, art, name) {
            Ok(Some(w)) => Some((self.gval_aus_wert(w), index)),
            _ => None,
        }
    }

    /// Release 2026-10-15: the host-provided own keys of a host object —
    /// indices, then visible names, for lists (art 2/5/6), none otherwise;
    /// `None` when `basis` is no host object.
    pub(crate) fn host_eigene_schluessel(&mut self, basis: GVal) -> Option<Vec<String>> {
        let id = self.host_id_von_gval(basis)?;
        let art = self.host_art(basis);
        // Release 2026-10-15: art 0 too — the document lists its named
        // elements (HTML §3.1.3 supported property names); other objects
        // answer nothing.
        if !matches!(art, 0 | 2 | 5..=8) {
            return Some(Vec::new());
        }
        let GVal::Ref(r) = basis else { return Some(Vec::new()) };
        let Some(h) = self.host_objekte.clone() else { return Some(Vec::new()) };
        let Ok(Some(crate::Wert::Feld(liste))) = h.lesen_mit_art(id, art, "__narsilOwnKeys") else {
            return Some(Vec::new());
        };
        let mut raus = Vec::with_capacity(liste.len());
        for w in liste {
            let crate::Wert::Text(k) = w else { continue };
            if crate::ops::index_aus_schluessel(&k).is_none() && (self.eigene_lese(r, &k).is_some() || self.proto_kette_hat(r, &k)) {
                continue;
            }
            raus.push(k);
        }
        Some(raus)
    }

    pub(crate) fn host_art(&self, basis: GVal) -> u8 {
        if let GVal::Ref(r) = basis {
            if let Some(GVal::Zahl(n)) = self.eigene_lese(r, "__art") { return n as u8; }
        }
        0
    }

    /// Release 11.10.2026: ein gewoehnliches Objekt mit EIGENEM `toString`
    /// (oder `valueOf`) wird vor der Uebergabe an den Host zur Zeichenkette
    /// (WebIDL DOMString: ToString ruft die Methoden der Seite). GEMESSEN
    /// vorher (WPT html/dom/reflection-*): `setAttribute('x', {toString(){
    /// return "a"}})` speicherte „[object Object]" — rund 4 000 Teiltests.
    pub(crate) fn host_text_vorbereiten(&mut self, v: GVal) -> Result<GVal, Stoerung> {
        // Release 12.10.2026: ein BigInt wird fuer den Host zu Text (WebIDL
        // DOMString: ToString). GEMESSEN vorher (WPT style-set-property):
        // `el.style.opacity = 1n` kam leer an.
        if matches!(v, GVal::Ganz(_)) {
            let t = self.zu_text(v)?;
            return self.text_wert(&t).map_err(Stoerung::Fehler);
        }
        let GVal::Ref(r) = v else { return Ok(v) };
        if !matches!(self.heap.holen(r), HObj::Gegenstand { .. }) || self.host_id_von_gval(v).is_some() {
            return Ok(v);
        }
        let basis_ts = self.hole_ohne_host(GVal::Ref(self.proto_objekt), "toString")?;
        let basis_vo = self.hole_ohne_host(GVal::Ref(self.proto_objekt), "valueOf")?;
        let ts = self.hole_ohne_host(v, "toString")?;
        let vo = self.hole_ohne_host(v, "valueOf")?;
        let eigen = |x: GVal, b: GVal| matches!(x, GVal::Ref(_)) && x != b;
        if !(eigen(ts, basis_ts) || eigen(vo, basis_vo)) {
            return Ok(v);
        }
        let t = self.zu_text(v)?;
        self.text_wert(&t).map_err(Stoerung::Fehler)
    }

    pub(crate) fn host_id_von_gval(&self, v: GVal) -> Option<u32> {
        //
        // GEMESSEN, warum das Muster so aussieht (E0614, 13:45):
        // `GVal::Ref(r)` bindet `r` als `&HeapRef` (das Muster matcht
        // gegen die Referenz, weil `GVal::Ref` eine HeapRef enthaelt und
        // `heap.holen` eine Referenz zurueckgibt). Also kein `*r` und
        // kein `*n` — beides gab E0614.
        let GVal::Ref(r) = v else { return None };
        // J8: ohne Haken gibt es keine Host-Objekte — der HashMap-Zugriff auf
        // `__host` faellt dann bei **jedem** Zugriff weg. GEMESSEN: der
        // Zugriff stand in `hole`, `host_methode_von_gval` (also in JEDEM
        // Aufruf) und `schreibe_host`.
        if self.host_objekte.is_none() {
            return None;
        }
        let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen(r) else {
            return None;
        };
        let e = eigenschaften.get(crate::hostobjekt::HOST_MERKE)?;
        let GVal::Zahl(n) = e.wert else { return None };
        if !n.is_finite() || n < 0.0 {
            return None;
        }
        Some(n as u32)
    }

    /// J-CACHE (05.10.): Traegt das Host-Objekt die Marke `__global`
    /// (Zahl)? Dann spiegelt es den globalen Bereich (`window` === global
    /// scope, HTML) und `hole()` antwortet dort mit Engine-Globals.
    /// Reine Kartenlesung, kein Haken, keine Rekursion.
    pub(crate) fn ist_global_spiegel(&self, basis: GVal) -> bool {
        let GVal::Ref(r) = basis else { return false };
        match self.heap.holen(r) {
            HObj::Gegenstand { eigenschaften, .. } => matches!(
                eigenschaften.get("__global"),
                Some(e) if matches!(e.wert, GVal::Zahl(_) | GVal::Bool(true))
            ),
            _ => false,
        }
    }

    /// J28: die DOM-Methode des **Motors** mit diesem Namen — oder `None`.
    ///
    /// GEMESSEN (news.ycombinator.com, `probe`, frisch gebaut, 02.10.):
    /// `hn.js` Zeile 2/4 ruft `document.getElementsByClassName(…)`.
    /// `document` ist bei `narsil` ein **Host-Objekt** (`{__host: 1}`,
    /// `crates/narsil/src/dom.rs:1330`), und dessen Getter `lesen` gibt
    /// fuer `getElementsByClassName` `Ok(None)` zurueck (`methode_nummer`
    /// kennt den Namen nicht). Der Motor lieferte daraufhin `undefined`,
    /// und der Aufruf endete als **TypeError: getElementsByClassName is
    /// not a function** — die ganze Seite brach ab, obwohl der Motor die
    /// Methode sehr wohl kann (sie haengt an `proto_dom_dokument`).
    ///
    /// DIE REGEL, DIE DARAUS FOLGT: ein Haken, der ein Feld nicht kennt,
    /// darf dem Motor seine **eigene** DOM-Schicht nicht verdecken. Es
    /// gewinnt der Motor — sein DOM ist das vollstaendigere (die vier
    /// Methoden aus J-HN/J-AR kennt der Haken nicht). Findet der Motor
    /// auch nichts, bleibt es bei `undefined` bzw. beim Haken-Fehler:
    /// sonst waere ein wirklich unbekanntes Feld still.
    ///
    /// **NUR Methoden.** Ein *Feld* (etwa `document.title`) wird nicht
    /// geliefert: dafuer ist der Haken die Wahrheit, und eine zweite
    /// Antwort waere eine zweite Wahrheit ueber dieselbe Seite.
    ///
    /// Es gibt hier bewusst **keine Handliste** der DOM-Namen: die
    /// Quelle ist die Registrierung selbst (`dom_prototypen_anlegen`
    /// schreibt ueber `dom_methode`/`nativ_anlegen`), damit die Liste
    /// nicht veralten kann. Der Weg ist in `tests/dom.rs` bezeugt
    /// (`dom_wurzel_fuer_haust_funktion_gefunden_und_gegenprobe`).
    /// Release 11.10.2026: hat ein Prototyp zwischen `r` und den Grund-
    /// Prototypen des DOM (`Element`/`Document`) den Namen als eigene
    /// Eigenschaft?
    fn kette_vor_element_hat(&self, r: HeapRef, name: &str) -> bool {
        let mut p = self.heap_proto_von(r);
        for _ in 0..16 {
            let Some(x) = p else { return false };
            // Nur die Unterschnittstellen UNTER `HTMLElement` (Select,
            // Input, Media …): `HTMLElement`, `Element`, `Document` und
            // `EventTarget` behalten ihre Host-Methoden (der Vorspann huellt
            // dort u. a. `dispatchEvent`, das fuer Fenster/Dokument der Host
            // beantwortet).
            if x == self.proto_dom_html
                || x == self.proto_dom_element
                || x == self.proto_dom_dokument
                || x == self.proto_ereignisziel
                || x == self.proto_objekt
            {
                return false;
            }
            if let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen(x)
                && let Some(e) = eigenschaften.get(name)
                && e.getter.is_none()
                && let GVal::Ref(f) = e.wert
                && matches!(self.heap.holen(f), HObj::Nativ { .. } | HObj::Funktion { .. })
            {
                return true;
            }
            p = self.heap_proto_von(x);
        }
        false
    }

    fn dom_prototyp_funktion(&mut self, basis: GVal, name: &str) -> Option<GVal> {
        // Release 2026-10-15: Element.prototype's methods do not stand in for
        // a node that is known not to be an element (its chain reaches
        // Node.prototype without Element.prototype — a document has no
        // `matches`). Host objects without a DOM prototype keep the old
        // fallback. MEASURED before (WPT dom/nodes/Element-matches):
        // `"matches" in document` was true.
        let knoten_proto = self.heap_proto_von(self.proto_dom_element);
        let ist_element = match basis {
            GVal::Ref(r) => {
                let mut p = self.heap_proto_von(r);
                let mut antwort = true;
                for _ in 0..64 {
                    let Some(x) = p else { break };
                    if x == self.proto_dom_element { antwort = true; break; }
                    if Some(x) == knoten_proto { antwort = false; break; }
                    p = self.heap_proto_von(x);
                }
                antwort
            }
            _ => true,
        };
        for proto in [self.proto_dom_dokument, self.proto_dom_element] {
            if proto == self.proto_dom_element && !ist_element {
                continue;
            }
            let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen(proto) else {
                continue;
            };
            let Some(e) = eigenschaften.get(name) else {
                continue;
            };
            // Ein Getter wird hier **nicht** aufgerufen: die Frage ist
            // „kann der Motor die Methode?", und ein Getter ist eine
            // Eigenschaft, keine Methode.
            if e.getter.is_some() {
                continue;
            }
            let GVal::Ref(r) = e.wert else { continue };
            if matches!(
                self.heap.holen(r),
                HObj::Nativ { .. } | HObj::Funktion { .. }
            ) {
                return Some(e.wert);
            }
        }
        None
    }

    /// Ein `GVal` als oeffentlicher `Wert` (fuer die Haken).
    ///
    /// GEMESSEN, warum das nicht `Wert::als_text` ist: der Haken bekommt
    /// den WERT (Zahl, Text, Objekt), nicht seine Darstellung. `3` und
    /// `"3"` unterscheiden sich, und ein Getter, der beides gleich macht,
    /// liefert einem Skript die falsche Zahl.
    pub(crate) fn gval_als_wert(&self, v: GVal) -> Option<crate::Wert> {
        self.gval_als_wert_tief(v, 1)
    }

    /// Wie [`Self::gval_als_wert`], mit einer Tiefe fuer Objektliterale.
    ///
    /// 06.10.2026: GEMESSEN an `document.addEventListener('x', fn,
    /// {capture:true})` — ein GEWOEHNLICHES Objekt (ohne `__host`) wurde
    /// hier zu `None` und damit zu `undefined`; `ListenerOptions::from_wert`
    /// sah keine Optionen, `capture` war immer `false`, und
    /// Doppelregistrierung wie Entfernen per `capture` schlugen fehl
    /// (Tests `event_listener_registry`). WebIDL-Dictionaries sind aber
    /// genau solch ein Objekt. Tiefe 1 genuegt: die Felder sind Primitive.
    fn gval_als_wert_tief(&self, v: GVal, tiefe: u32) -> Option<crate::Wert> {
        // GEMESSEN, warum `Text` fehlt: `GVal` kennt nur `Undefiniert`,
        // `Null`, `Bool`, `Zahl` und `Ref` — Texte sind IMMER ein `Ref`
        // auf ein `HObj::Text`. `crate::Wert::Text` gibt es, aber der
        // Heap-Wert dahinter muss geholt werden.
        match v {
            GVal::Undefiniert => Some(crate::Wert::Undefiniert),
            GVal::Null => Some(crate::Wert::Null),
            GVal::Bool(b) => Some(crate::Wert::Wahr(b)),
            GVal::Zahl(n) => Some(crate::Wert::Zahl(n)),
            // J24: `crate::Wert` (die oeffentliche Schnittstelle, die
            // `narsil` liest) hat **keine** BigInt-Variante — eine neue
            // waere eine Aenderung des oeffentlichen API. Der Host sieht
            // den Inspektions-Text `2n`, genau wie `console.log` ihn zeigt.
            GVal::Ganz(g) => Some(crate::Wert::Text(format!("{g}n"))),
            //
            // GEMESSEN, warum der HEAP noetig ist: `GVal` kennt keinen
            // Text, und ein Text liegt als `HObj::Zeichenkette` im
            // Heap. Fuer EINEN Text zu clonen ist das die billigste
            // Loesung — ein Zeigerkoegel-Zaehler waere hier eine
            // Aenderung an 30 Stellen fuer einen Fall, der in der DOM
            // fast nie vorkommt (`el.textContent = "x"` liest
            // hoechstens einmal danach).
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Zeichenkette(t) => Some(crate::Wert::Text(t.clone())),
                //
                // W6D: EINE FUNKTION wird als solche durchgereicht.
                //
                // GEMESSEN, warum das der entscheidende Ort ist (13:40):
                // `addEventListener('click', () => {…})` kam hier als
                // `undefined` an, weil eine Funktion kein Text ist. Der
                // Host-Haken bekommt jetzt `Wert::Funktion(index)` und
                // kann den Handler speichern und spaeter aufrufen.
                HObj::TypedArray { art, puffer, versatz, laenge, .. }
                    if matches!(art, crate::eingebaut_typed::ElementArt::Uint8 | crate::eingebaut_typed::ElementArt::Uint8Clamped) => {
                    let HObj::BytePuffer { daten, .. } = self.heap.holen(*puffer) else { return None };
                    Some(crate::Wert::ByteFeld { daten: daten[*versatz..*versatz + *laenge].to_vec(), geklemmt: *art == crate::eingebaut_typed::ElementArt::Uint8Clamped })
                }
                HObj::Funktion { .. } | HObj::Nativ { .. } => Some(crate::Wert::Funktion(r.0)),
                //
                // J24: **ein HOST-OBJEKT geht zurueck** — genau so, wie es
                // angekommen ist.
                //
                // GEMESSEN/GEFUNDEN: `li.appendChild(li)`-Faelle ueber die
                // Bruecke verloren ihr Argument. `narsil` meldete
                // „appendChild: undefined ist kein Element", weil
                // `aufruf_einfuegen` (`vm.rs`) jedes Argument ueber
                // `gval_als_wert` wandert und `None` zu `undefined` machte.
                // Der Import (`import_wert`) baut `{__host: 7}` als
                // `Gegenstand`; ohne diesen Arm kam nur `undefined` zurueck
                // — und `narsil::dom` (1229) kann mit einem undefinierten
                // Argument nichts anfangen.
                //
                // Ein **normales** Objekt wird als FELDLISTE exportiert
                // (seit 06.10.2026, siehe `gval_als_wert_tief`).
                //
                // EIN HOST-OBJEKT BEHAELT SEINE FELDER. GEMESSEN an
                // `elSetText(getElementById("z"), …)`: `getElementById`
                // liefert `{__host, __ord}`; die erste Fassung exportierte
                // nur `__host`, und die Kiste meldete „Objekt ohne __ord" —
                // die alten Tests (`javascript`) sahen keine Aenderung.
                // `wert_aus_gegenstand` nimmt die EIGENEN Felder (auch
                // `__ord`, `__methode`, `__art`, `__name`), `__host` wird
                // zur Sicherheit ergaenzt.
                HObj::Gegenstand { .. } => {
                    if let Some(id) = self.host_id_von_gval(v) {
                        let mut felder = self
                            .wert_aus_host_gegenstand(r)
                            .unwrap_or(crate::Wert::Gegenstand(Vec::new()));
                        if let crate::Wert::Gegenstand(liste) = &mut felder
                            && !liste.iter().any(|(n, _)| n == crate::hostobjekt::HOST_MERKE)
                        {
                            liste.push((
                                crate::hostobjekt::HOST_MERKE.to_string(),
                                crate::Wert::Zahl(f64::from(id)),
                            ));
                        }
                        Some(felder)
                    } else if tiefe > 0 {
                        self.wert_aus_gegenstand(r, tiefe - 1)
                    } else {
                        None
                    }
                }
                _ => None,
            },
        }
    }

    /// Release 09.10.2026: ein HOST-Objekt traegt nur seine **Kennfelder**
    /// ueber die Bruecke — eigene Felder mit `__` und primitivem Wert
    /// (`__host`, `__ord`, `__art`, `__name`, `__methode`, …).
    ///
    /// GEMESSEN, warum: die vorige Fassung nahm ALLE eigenen Felder und
    /// folgte Objekten darin. Ein Element mit Expandos, die auf andere
    /// Elemente zeigen (Polymer: `__dataHost`, `root`, …), lief so im Kreis:
    /// youtube.com endete mit `thread 'narsil-js' has overflowed its stack`
    /// (cdb: `gval_als_wert_tief` <-> `wert_aus_gegenstand`,
    /// eigenschaften.rs 1429/1472) und vorher 1,1 GB Speicher.
    fn wert_aus_host_gegenstand(&self, r: HeapRef) -> Option<crate::Wert> {
        let crate::heap::HObj::Gegenstand {
            eigenschaften,
            reihenfolge,
            ..
        } = self.heap.holen(r)
        else {
            return None;
        };
        let mut felder = Vec::new();
        for name in reihenfolge {
            if !name.starts_with("__") {
                continue;
            }
            let Some(e) = eigenschaften.get(name) else {
                continue;
            };
            if e.getter.is_some() || e.setter.is_some() {
                continue;
            }
            let primitiv = match e.wert {
                GVal::Ref(x) => matches!(self.heap.holen(x), crate::heap::HObj::Zeichenkette(_)),
                _ => true,
            };
            if primitiv && let Some(w) = self.gval_als_wert_tief(e.wert, 0) {
                felder.push((name.clone(), w));
            }
        }
        Some(crate::Wert::Gegenstand(felder))
    }

    /// Ein gewoehnliches Objekt als flache Feldliste.
    fn wert_aus_gegenstand(&self, r: HeapRef, tiefe: u32) -> Option<crate::Wert> {
        let crate::heap::HObj::Gegenstand {
            eigenschaften,
            reihenfolge,
            ..
        } = self.heap.holen(r)
        else {
            return None;
        };
        let mut felder = Vec::with_capacity(reihenfolge.len());
        for name in reihenfolge {
            let Some(e) = eigenschaften.get(name) else {
                continue;
            };
            // Zugriffsmethoden bleiben draussen: ihr Wert waere der
            // Getter-Aufruf, und dieses Fenster hat `&self` (kein
            // Ausfuehren). Ein Datenfeld ist der Fall, den WebIDL meint.
            if e.getter.is_some() || e.setter.is_some() {
                continue;
            }
            let wert = self
                .gval_als_wert_tief(e.wert, tiefe)
                .unwrap_or(crate::Wert::Undefiniert);
            felder.push((name.clone(), wert));
        }
        Some(crate::Wert::Gegenstand(felder))
    }

    /// Ein oeffentlicher `Wert` als `GVal` (Rueckgabe eines Getters).
    ///
    /// GEMESSEN, warum ein Text ein `HObj::Text` wird: `GVal` hat keine
    /// Text-Variante. Und warum `Feld`/`Gegenstand` zu `undefined` werden:
    /// ein Haken, der ein Objekt zurueckgibt, ist in diesem Browser nicht
    /// gebaut (M5: kein Weg, es im Kind-Thread zu besitzen) — und
    /// `undefined` ist eine ehrliche Antwort.
    pub(crate) fn gval_aus_wert(&mut self, w: crate::Wert) -> GVal {
        match w {
            //
            // GEMESSEN, warum `Feld`/`Gegenstand` ECHTE Objekte werden
            // (15:15): der Getter gibt `document.body` als
            // `{__host: 2}` zurueck, und `undefined` daraus gemacht wurde
            // zu „document.body ist undefined" — der Aufruf
            // `document.getElementById("z")` wurde dann „Aufrufziel ist
            // keine Funktion". `import_wert` baut das Objekt im Heap, und
            // der Haken erkennt es wieder (es hat `__host`).
            crate::Wert::ByteFeld { .. } | crate::Wert::Feld(_) | crate::Wert::Gegenstand(_) => {
                match self.import_wert(&w, 0) {
                    Ok(v) => v,
                    // M4: passt der Wert nicht in den Heap, ist das ein
                    // Speicherfehler der Engine — kein stilles
                    // `undefined`.
                    Err(f) => self
                        .gval_aus_wert(crate::Wert::Text(std::format!("Host-Wert zu gross: {f}"))),
                }
            }
            //
            // W6D: eine Funktion laesst sich nicht zurueckgeben (sie
            // existiert nur in dieser Engine). Sie wird `undefined`.
            crate::Wert::Funktion(_) => GVal::Undefiniert,
            // Eine native Funktion laesst sich nicht zurueckgeben, ohne
            // ihre Registrierung zu verlieren (W6D: `Wert::Nativ` traegt
            // den Namen, nicht die Funktion).
            crate::Wert::Nativ(_) => GVal::Undefiniert,
            crate::Wert::Undefiniert => GVal::Undefiniert,
            crate::Wert::Null => GVal::Null,
            crate::Wert::Wahr(b) => GVal::Bool(b),
            crate::Wert::Zahl(n) => GVal::Zahl(n),
            crate::Wert::Text(t) => match self.text_wert(&t) {
                Ok(v) => v,
                // M4: ein Text, der nicht in den Heap passt, ist ein
                // Speicherfehler — kein stilles `undefined`. Der Haken
                // bekommt die Fehlermeldung der Engine.
                Err(_) => {
                    self.geworfen = None;
                    return GVal::Undefiniert;
                }
            },
        }
    }

    /// Sperrzustand eines Objekts (Offen, wenn kein Objekt/keine Sperre).
    pub(crate) fn sperre_von(&self, r: HeapRef) -> crate::heap::Sperre {
        use crate::heap::Sperre;
        match self.heap.holen(r) {
            HObj::Gegenstand { sperre, .. }
            | HObj::Feld { sperre, .. }
            | HObj::Verpackt { sperre, .. }
            | HObj::Fehler { sperre, .. }
            | HObj::Datum { sperre, .. }
            | HObj::Regex { sperre, .. }
            | HObj::Karte { sperre, .. }
            | HObj::Menge { sperre, .. }
            | HObj::Versprechen { sperre, .. } => *sperre,
            _ => Sperre::Offen,
        }
    }

    /// Verweigerung: strikt -> TypeError-Wurf, sloppy -> stilles Ok
    /// (Koordination W5C: freeze muss sloppy schweigen).
    /// J18: eine verweigerte Zuweisung ist im lockeren Modus **still**
    /// und im strikten ein TypeError.
    ///
    /// GEMESSEN, warum `rahmen_strikt()` statt `self.strikt`: die
    /// Direktive `"use strict"` steht im Rumpf der Funktion, nicht im
    /// Programm. `(() => { 'use strict'; var o = Object.freeze({a:1});
    /// o.a = 2; return 'ok' })()` lieferte `ok` statt eines TypeError —
    /// mit `self.strikt` (Programmlage) sieht die Funktion nicht, dass
    /// sie strikt ist. node 22 (sloppy Skript, Direktive in der
    /// Funktion): `TypeError: Cannot assign to read only property 'a'`.
    pub(crate) fn verweigern(&mut self, msg: &str) -> Result<(), Stoerung> {
        if self.strikt || self.rahmen_strikt() {
            Err(Stoerung::Wurf(self.fehler_wert("TypeError", msg)))
        } else {
            Ok(())
        }
    }

    /// Setzen mit Setter-Protokoll (eigene + Proto-Setter), sonst eigene
    /// Daten-Eigenschaft. Gibt zurueck, ob geschrieben wurde.
    pub(crate) fn setze(&mut self, basis: GVal, name: &str, wert: GVal) -> Result<(), Stoerung> {
        let marke = self.wurzel_merke();
        self.wurzeln.push(wert);
        self.wurzeln.push(basis);
        // Release 10.10.2026: `[[Set]]` eines Proxys (`proxy.rs`); ein
        // `false` ist im strikten Code ein TypeError (ES2023 13.15.2).
        if let GVal::Ref(p) = basis
            && matches!(self.heap.holen(p), HObj::Proxy { .. })
        {
            let r = match self.proxy_set(p, name, wert, basis) {
                Ok(false) if self.strikt || self.rahmen_strikt() => Err(Stoerung::Wurf(self.fehler_wert(
                    "TypeError",
                    &format!("'set' on proxy: trap returned falsish for property '{name}'"),
                ))),
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            };
            self.wurzel_zurueck(marke);
            return r;
        }
        let r = self.setze_inner(basis, name, wert);
        self.wurzel_zurueck(marke);
        r
    }

    pub(crate) fn setze_inner(
        &mut self,
        basis: GVal,
        name: &str,
        wert: GVal,
    ) -> Result<(), Stoerung> {
        //
        // W6B, J9: HOST-OBJEKT ZUERST — `el.textContent = "3"` laeuft
        // hier. GEMESSEN, warum die Reihenfolge stimmen muss: ein
        // Host-Objekt ist ein Objekt, und `setze_inner` wuerde den Wert
        // als Feld eintragen; beim naechsten Lesen gewinnt trotzdem der
        // Haken, und der Wert waere weg.
        //
        // W6B: MASCHINEN-FELDER (`__host`, `__methode`) gehen am Haken
        // VORBEI.
        //
        // GEMESSEN, warum (15:30): `import_wert` baut ein Host-Objekt, indem
        // es die Felder nacheinander schreibt. Ohne diese Ausnahme hat
        // der Setter beim zweiten Feld (`__methode`) zurueckgegeben,
        // `dom::schreiben` aber den Befehl geschluckt — das Feld stand
        // danach nicht im Objekt, `host_methode_von_gval` fand nichts,
        // und `document.getElementById` war „keine Funktion".
        //
        // Die Ausnahme steht HIER und nicht im Haken: sie ist eine
        // Eigenschaft der Maschine, nicht des Aufrufers.
        if !name.starts_with("__")
            && let Some(id) = self.host_id_von_gval(basis)
            && let Some(h) = self.host_objekte.clone()
            && !matches!(basis, GVal::Ref(r) if self.ce_eigene_kette(r, name))
            && h.verwaltet(id, self.host_art(basis), name)
        {
            if !h.schreibbar(id, self.host_art(basis), name) {
                if self.strikt { return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "DOM-Eigenschaft ist schreibgeschuetzt"))); }
                return Ok(());
            }
            //
            // GEMESSEN, warum `unwrap_or(Undefiniert)`: ein `Ref` auf
            // etwas, das kein Text ist (eine Funktion, ein Objekt), wird
            // zu `undefined` geschrieben — das ist der JS-nahe Weg
            // (`el.textContent = el` setzt „undefined" als Text, weil
            // `String(el)` das ergibt).
            let wert_text = self.host_text_vorbereiten(wert)?;
            let als_wert = self.host_argument(wert_text);
            let profil_start = crate::vm::profil_host_start();
            let antwort = h.schreiben_mit_art(id, self.host_art(basis), name, &als_wert);
            crate::vm::profil_host("schreib", name, profil_start);
            antwort.map_err(|msg| Stoerung::Wurf(self.host_setzfehler_wert(&msg)))?;
            self.ce_reaktionen_liefern(&[basis, wert])?;
            if !name.starts_with("on") { return Ok(()); }

        }
        // Release 2026-10-15: a live collection (host art 5/6) has no indexed
        // or named setter (WebIDL §3.9.2 [[Set]]): an index never becomes a
        // property, nor does a visible named property; other names are
        // expandos. MEASURED before (WPT HTMLCollection-own-props):
        // `c[0] = "foo"` replaced the element.
        if !name.starts_with("__")
            && matches!(self.host_art(basis), 5..=8)
            && self.host_id_von_gval(basis).is_some()
            && (crate::ops::index_aus_schluessel(name).is_some() || self.host_eigene(basis, name).is_some())
        {
            return self.verweigern(&format!("Failed to set the indexed or named property '{name}' of a live collection"));
        }
        if !name.starts_with("__") && !name.starts_with("on") && self.ist_global_spiegel(basis) {
            return self.setze(GVal::Ref(self.global_objekt), name, wert);
        }
        let r = match basis {
            GVal::Ref(r) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "kein Setzen auf Primitiv"),
                ));
            }
        };
        if matches!(self.heap.holen(r), HObj::TypedArray { .. })
            && crate::eingebaut_typed::typedarray_numerischer_schluessel(name) {
            let index = if name == "-0" { None } else { name.parse::<usize>().ok() };
            return self.typedarray_element_setzen(r, index, wert);
        }
        // J6: `F.prototype = obj` (sec-10.2.4) — an einer gewöhnlichen
        // Funktion ist `prototype` eine **beschreibbare** eigene
        // Eigenschaft; bei einer **Klasse** ist sie fest.
        //
        // GEMESSEN, warum das fehlte: der Schreibzugriff landete in der
        // Property-Map, `hole` lieferte aber immer das synthetische
        // `prototyp_objekt` — die Zuweisung war unsichtbar. An echten
        // Skripten (J6, wikipedia `Map.prototype={…}`) brach dadurch jede
        // weitere Benutzung ab: `typeof M.prototype.get` war `undefined`
        // (node: `function`), `new M().get()` warf „Aufrufziel ist keine
        // Funktion".
        if name == "prototype" && matches!(self.heap.holen(r), HObj::Funktion { .. }) {
            // J34: **datengetrieben** entscheiden, nicht ueber
            // `ist_konstruktor`. GEMESSEN (node 22):
            //   `class C {}`     -> `C.prototype = x` bleibt still (nicht
            //                       beschreibbar), im strikten Code TypeError
            //   `function f(){}` -> `f.prototype = x` ist **erlaubt**
            // Beide sind Konstruktorfunktionen, beide tragen dasselbe Flag —
            // der Unterschied steckt allein im `[[Writable]]`, und das steht
            // jetzt in der Property-Map (siehe `closure_anlegen_als` und
            // `klasse_bauen`). Die erste Fassung verweigerte nach dem Flag
            // und brach damit jede Zuweisung an eine gewöhnliche Funktion ab.
            let schreibbar = match self.heap.holen(r) {
                HObj::Funktion { eigenschaften, .. } => eigenschaften
                    .get("prototype")
                    .map(|e| e.beschreibbar)
                    .unwrap_or(true),
                _ => true,
            };
            if !schreibbar {
                return self.verweigern("Klassen-Prototyp ist fest");
            }
            let GVal::Ref(neu) = wert else {
                let e = self.fehler_wert("TypeError", "prototype ist kein Objekt");
                return Err(Stoerung::Wurf(e));
            };
            // Slot **und** Karte: sonst beschreibt der Deskriptor weiter das
            // alte Objekt (J34 — dieselbe Klasse Fehler wie der Schreibpfad).
            if let HObj::Funktion {
                prototyp_objekt,
                eigenschaften,
                ..
            } = self.heap.holen_mut(r)
            {
                *prototyp_objekt = Some(neu);
                if let Some(e) = eigenschaften.get_mut("prototype") {
                    e.wert = GVal::Ref(neu);
                }
            }
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            return Ok(());
        }
        // Feld-Index/laenge-Spezialfaelle.
        if let HObj::Feld { .. } = self.heap.holen(r) {
            use crate::heap::Sperre;
            let sperre = self.sperre_von(r);
            if name == "length" {
                // length ist bei Versiegelt/Gefroren fest (JS-Naeherung).
                if sperre == Sperre::Versiegelt || sperre == Sperre::Gefroren {
                    return self.verweigern("Feld-Laenge gesperrt");
                }
                // JS2 (02.10.2026, 01:15): **pruefen statt schneiden**.
                //
                // GEMESSEN, `node 22` (01:10), zehn Eingaben. Der alte Code
                // war `zu_uint32(wert)? as usize` — das **schneidet**: aus `-1`
                // wurde `4294967295`, aus `1.5` wurde `1`, aus `NaN` wurde `0`.
                // Danach kam die Speichergrenze, also ein **Speicherfehler**
                // statt `RangeError`.
                //
                // DIE REGEL (ECMA-262 10.4.2.4, `ArraySetLength`, Schritt 1 d)
                // ist **ein Vergleich zweier Umwandlungen**:
                //
                //   Let newLen be ? ToUint32(Desc.[[Value]]).
                //   Let numberLen be ? ToNumber(Desc.[[Value]]).
                //   If newLen is not numberLen, throw a RangeError exception.
                //
                // `1.5` ist der Fall, der die Regel noetig macht:
                // `ToUint32(1.5)` ist `1`, wer nur `zu_uint32` benutzt, laesst
                // es **durch** — node wirft. `-0` ist der Fall, der die Regel
                // **nicht** verletzt: `0 === -0` gilt in JS und in Rust.
                //
                // DIE REIHENFOLGE: `zu_zahl` zuerst, weil es bei einem Symbol
                // einen `TypeError` werfen muss (`define-own-prop-length-
                // coercion-order-set.js` erwartet im strikten Modus genau den).
                // Die Regel steht in `eingebaut_array.rs` mit zwei Unit-Tests.
                //
                // GETEILTE DATEI: die Verdrahtung hier, gemeldet in
                // `team/WUNSCH_JS.txt`.
                let als_zahl = self.zu_zahl(wert)?;
                let n = match crate::eingebaut_array::laenge_pruefen(
                    crate::ops::als_uint32(als_zahl),
                    als_zahl,
                ) {
                    Ok(n) => n as usize,
                    Err(()) => {
                        return Err(Stoerung::Wurf(
                            self.fehler_wert("RangeError", "ungueltige Feld-Laenge"),
                        ));
                    }
                };
                // ## W50 (QUIC, 04.10.2026) — **`length` DARF BIS 2^32-1.**
                //
                // **FREIGEGEBEN:** die Leitung hat Regel 15 gelesen — die vier
                // JS-Rollen duerfen sich in `narsil-jsvm` **minimal** aendern. Diese
                // Datei ist `eigenschaften.rs`; **JS ist im BRETT informiert**.
                //
                // **DER BEFUND (`node 22` als Soll, drei Suite-Faelle):**
                // ```text
                // var a = []; a.length = 4294967295;  node: a.length === 4294967295
                // new Array(4294967295)                node: length === 4294967295
                // ```
                // Wir warfen `Speichergrenze: Feld-Laenge zu gross`, weil
                // `MAX_SAMMLUNG` (4 000 000) **beides** war: eine Speichergrenze
                // **und** eine Laengengrenze. **Das sind zwei verschiedene Fragen
                // — und die Norm beantwortet nur eine davon.**
                //
                // **WARUM `MAX_SAMMLUNG` NICHT GELOESCHT WIRD, sondern daneben
                // gestellt wird:** eine **Speichergrenze** muss bleiben, denn
                // `elemente.resize(4_000_000_000, …)` waere 64 GB. **Ein Loch
                // kostet aber nichts** — es ist nur ein Index, den es nicht gibt.
                // Also: die **Laenge** darf gross werden, und **materialisiert
                // wird nur, was bezahlbar ist.**
                //
                // **UND WO DIE GROSSE LANGE LIEGT:** in `rest["length"]`, einer
                // **Property-Map mit einem Eintrag**. Das kostet nichts — und es
                // ist derselbe Ort, an dem `Object.defineProperty(a, "length", …)`
                // ohnehin landet. *Der Index bleibt abgeschnitten: `a[4294967294] =
                // x` braucht einen echten Eintrag, und dafuer habe ich nichts
                // Gebautes — der Fall bleibt offen und ist benannt.*
                let gross = n > MAX_SAMMLUNG;
                if n > (u32::MAX as usize) {
                    // `laenge_pruefen` hat schon `ToUint32` erzwungen, also ist
                    // das **nicht** erreichbar — die Zeile steht trotzdem da,
                    // weil ein `u32`-Vergleich ohne obere Klammer ein
                    // `Vergleich mit `u32::MAX as usize` liest.
                    return Err(Stoerung::Fehler(JsFehler::Speicher(
                        "Feld-Laenge zu gross".to_string(),
                    )));
                }
                let alt = match self.heap.holen(r) {
                    HObj::Feld { elemente, .. } => elemente.len(),
                    _ => 0,
                };
                // **M3: die Kosten des grossen `length` sind EIN Eintrag**
                // (`KOSTEN_EINTRAG`), nicht `n` Eintraege — das ist der ganze
                // Grund, warum die Property-Map die richtige Ablage ist.
                if gross {
                    self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
                }
                let mut o = self.heap.holen_mut(r);
                if let HObj::Feld { elemente, .. } = &mut *o {
                    // sec-22.1.1: `length` vergroessern erzeugt **Luecken**
                    // (J7) — `Object.keys([1])` nach `length = 3` bleibt `0`.
                    if !gross {
                        elemente.resize(n, GVal::Undefiniert);
                    }
                }
                if !gross {
                    // ## W51 (JS, 05.10.2026) — EIN ALTER WERT DARF NICHT
                    // **STEHEN BLEIBEN**
                    //
                    // Sobald wieder materialisiert wird, ist `elemente.len()`
                    // die Wahrheit, und ein liegengebliebenes `rest["length"]`
                    // wuerde sie **ueberdecken** (der Leser schlaegt
                    // `elemente` bewusst nach). GEMESSEN:
                    // ```js
                    // var a = []; a.length = 4294967295; a.length = 2;
                    // ```
                    // node 22: `a.length === 2`. Ohne diese Zeile: `4294967295`.
                    if let HObj::Feld { rest, .. } = &mut *o {
                        rest.remove("length");
                    }
                }
                if gross {
                    // **Die Zahl in die Property-Map** — ein Eintrag, kein Feld.
                    let lv = GVal::Zahl(n as f64);
                    // **`o` ist `&mut HObj`** — und `rest` ist bei einem Feld
                    // **das** Feld, an das der Wert gehoert. Ich nehme es
                    // ueber ein Muster, damit kein zweiter Zugriffspfad fuer
                    // `rest` entsteht.
                    if let HObj::Feld { rest, .. } = &mut *o {
                        rest.insert(
                            "length".to_string(),
                            crate::heap::Eigenschaft {
                                wert: lv,
                                getter: None,
                                setter: None,
                                aufzählbar: false,
                                beschreibbar: true,
                                konfigurierbar: false,
                            },
                        );
                    }
                }
                crate::heap::feld_luecken_von_bis(&mut o, alt, n);
                if n < alt {
                    crate::heap::feld_luecken_oberhalb_weg(&mut o, n);
                }
                // ## W51 (JS, 05.10.2026) — DIE KOSTEN DER MATERIALISIERUNG
                //
                // Diese Zeile berechnet die Kosten fuer `n` **materialisierte**
                // Elemente. Wenn `gross` gilt, gibt es **keine** — das Feld
                // wurde ja absichtlich **nicht** vergroessert, und die grosse
                // Laenge kostet **einen** Property-Eintrag (siehe oben). Die
                // Rechnung ohne diese Bedingung waere also doppelt falsch: sie
                // verlangt Speicher fuer eine Sammlung, die es nicht gibt, und
                // sie macht aus dem Fall `a.length = 4294967295` (node 22:
                // `a.length === 4294967295`, **sofort**) einen
                // Speicherfehler.
                if !gross {
                    self.heap
                        .kosten_verbuchen(n.saturating_mul(crate::heap::KOSTEN_EINTRAG) / 4);
                }
                return Ok(());
            }
            if let Some(i) = index_aus_schluessel(name) {
                if i > MAX_SAMMLUNG {
                    return Err(Stoerung::Fehler(JsFehler::Speicher(
                        "Feld-Index zu gross".to_string(),
                    )));
                }
                // Gefrorene Felder: Elemente fest (strikt Wurf, sloppy still).
                if self.sperre_von(r) == crate::heap::Sperre::Gefroren {
                    return self.verweigern("gefrorenes Feld");
                }
                let alt = match self.heap.holen(r) {
                    HObj::Feld { elemente, .. } => elemente.len(),
                    _ => 0,
                };
                let mut o = self.heap.holen_mut(r);
                if let HObj::Feld { elemente, .. } = &mut *o {
                    if i >= elemente.len() {
                        elemente.resize(i + 1, GVal::Undefiniert);
                    }
                    elemente[i] = wert;
                }
                // sec-12.2.2: die uebersprungenen Indizes sind Luecken
                // (J7), nicht `undefined`; ein Schreibzugriff hebt die Luecke
                // beim Zielindex auf.
                crate::heap::feld_luecken_von_bis(&mut o, alt, i);
                crate::heap::feld_luecke_loeschen(&mut o, i);
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
                return Ok(());
            }
        }
        // Eigener Setter?
        let eigener = self.objekt_eigene_proto(r, name).0;
        if let Some(e) = eigener {
            if let Some(s) = e.setter {
                let marke = self.wurzel_merke();
                self.wurzeln.push(GVal::Ref(s));
                let rr = self.aufruf_synchron(GVal::Ref(s), basis, &[wert], false);
                self.wurzel_zurueck(marke);
                rr?;
                return Ok(());
            }
            if e.getter.is_some() || !e.beschreibbar {
                return self.verweigern(&format!("nur-lesbare Eigenschaft '{name}'"));
            }
            let o = self.heap.holen_mut(r);
            Self::eigenschaft_setzen(o, name, wert);
            return Ok(());
        }
        // Die erste geerbte Eigenschaft entscheidet. Eine Daten- oder
        // Getter-Eigenschaft verdeckt auch Setter auf ferneren Prototypen.
        if let Some(e) = self.proto_eigenschaft_suche(r, name) {
            if let Some(s) = e.setter {
                let marke = self.wurzel_merke();
                self.wurzeln.push(GVal::Ref(s));
                let rr = self.aufruf_synchron(GVal::Ref(s), basis, &[wert], false);
                self.wurzel_zurueck(marke);
                rr?;
                return Ok(());
            }
            if e.getter.is_some() || !e.beschreibbar {
                // Release 10.10.2026: mit dem Namen (Diagnose; Chrome:
                // „Cannot assign to read only property 'x' of object").
                return self.verweigern(&format!("nur-lesbare geerbte Eigenschaft '{name}'"));
            }
        }
        // Neue eigene Eigenschaft (M4: vorher Limit pruefen).
        // Gesperrte Objekte nehmen keine neuen Props (strikt Wurf).
        if self.sperre_von(r) != crate::heap::Sperre::Offen {
            return self.verweigern("Objekt nicht erweiterbar");
        }
        self.vor_allok(crate::heap::KOSTEN_EINTRAG)
            .map_err(Stoerung::Fehler)?;
        let o = self.heap.holen_mut(r);
        match o {
            HObj::Gegenstand {
                eigenschaften,
                reihenfolge,
                ..
            } => {
                // GEMESSEN: die Reihenfolge der eigenen Eigenschaften muss
                // mitlaufen, sonst haengt die Ausgabe am HashMap
                // (`Object.keys({z:1,a:2})` -> a,z statt z,a wie in node).
                if !eigenschaften.contains_key(name) {
                    reihenfolge.push(name.to_string());
                }
                eigenschaften.insert(
                    name.to_string(),
                    Eigenschaft {
                        konfigurierbar: true,
                        wert,
                        getter: None,
                        setter: None,
                        aufzählbar: true,
                        beschreibbar: true,
                    },
                );
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
                Ok(())
            }
            HObj::Verpackt { eigenschaften, .. } => {
                eigenschaften.insert(
                    name.to_string(),
                    Eigenschaft {
                        wert,
                        getter: None,
                        setter: None,
                        aufzählbar: true,
                        beschreibbar: true,
                        konfigurierbar: true,
                    },
                );
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
                Ok(())
            }
            HObj::Funktion { eigenschaften, .. } | HObj::Nativ { eigenschaften, .. } => {
                // `name`/`length`/`prototype` sind synthetisch und bleiben
                // es (dokumentierte Naeherung); Rest ist freie Eigenschaft.
                eigenschaften.insert(
                    name.to_string(),
                    Eigenschaft {
                        wert,
                        getter: None,
                        setter: None,
                        aufzählbar: true,
                        beschreibbar: true,
                        konfigurierbar: true,
                    },
                );
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
                Ok(())
            }
            HObj::Feld { rest, .. } => {
                rest.insert(
                    name.to_string(),
                    Eigenschaft {
                        wert,
                        getter: None,
                        setter: None,
                        aufzählbar: true,
                        beschreibbar: true,
                        konfigurierbar: true,
                    },
                );
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
                Ok(())
            }
            HObj::Generator { eigenschaften, .. }
            | HObj::Fehler { eigenschaften, .. }
            | HObj::Datum { eigenschaften, .. }
            | HObj::Regex { eigenschaften, .. }
            | HObj::Karte { eigenschaften, .. }
            | HObj::Menge { eigenschaften, .. }
            | HObj::BytePuffer { eigenschaften, .. }
            | HObj::TypedArray { eigenschaften, .. }
            | HObj::Versprechen { eigenschaften, .. } => {
                eigenschaften.insert(
                    name.to_string(),
                    Eigenschaft {
                        wert,
                        getter: None,
                        setter: None,
                        aufzählbar: true,
                        beschreibbar: true,
                        konfigurierbar: true,
                    },
                );
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
                Ok(())
            }
            _ => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Objekt nicht erweiterbar (Art)"),
            )),
        }
    }

    pub(crate) fn eigenschaft_setzen(o: &mut HObj, name: &str, wert: GVal) {
        let map = match o {
            HObj::Gegenstand { eigenschaften, .. } => eigenschaften,
            HObj::Verpackt { eigenschaften, .. } => eigenschaften,
            HObj::Feld { rest, .. } => rest,
            HObj::Funktion { eigenschaften, .. } | HObj::Nativ { eigenschaften, .. } => {
                eigenschaften
            }
            HObj::Generator { eigenschaften, .. }
            | HObj::Fehler { eigenschaften, .. }
            | HObj::Datum { eigenschaften, .. }
            | HObj::Regex { eigenschaften, .. }
            | HObj::Karte { eigenschaften, .. }
            | HObj::Menge { eigenschaften, .. }
            | HObj::BytePuffer { eigenschaften, .. }
            | HObj::TypedArray { eigenschaften, .. }
            | HObj::Versprechen { eigenschaften, .. } => eigenschaften,
            _ => return,
        };
        if let Some(e) = map.get_mut(name) {
            e.wert = wert;
        }
    }

    pub(crate) fn proto_eigenschaft_suche(&self, r: HeapRef, name: &str) -> Option<Eigenschaft> {
        let mut cur = self.heap_proto_von(r);
        let mut rest = 10_000;
        while let Some(o) = cur {
            rest -= 1;
            if rest == 0 {
                break;
            }
            let (eigene, p) = self.objekt_eigene_proto(o, name);
            if let Some(e) = eigene {
                return Some(e);
            }
            cur = p;
        }
        None
    }

    pub(crate) fn loesche(&mut self, basis: GVal, name: &str) -> Result<bool, Stoerung> {
        let r = match basis {
            GVal::Ref(r) => r,
            _ => return Ok(true),
        };
        // Release 10.10.2026: `[[Delete]]` eines Proxys.
        if matches!(self.heap.holen(r), HObj::Proxy { .. }) {
            let ok = self.proxy_delete(r, name)?;
            if !ok && (self.strikt || self.rahmen_strikt()) {
                return Err(Stoerung::Wurf(self.fehler_wert(
                    "TypeError",
                    &format!("'deleteProperty' on proxy: trap returned falsish for property '{name}'"),
                )));
            }
            return Ok(ok);
        }
        // Release 2026-10-15: a supported index or a visible named property
        // of a host list cannot be deleted (WebIDL §3.9.4 [[Delete]]).
        if !name.starts_with("__")
            && matches!(self.host_art(basis), 2 | 5..=8)
            && self.host_id_von_gval(basis).is_some()
            && self.host_eigene(basis, name).is_some()
        {
            if self.strikt || self.rahmen_strikt() {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeError", &format!("Cannot delete property '{name}' of a live collection"))));
            }
            return Ok(false);
        }
        // Release 2026-10-15: "strict" includes a strict function frame
        // (`rahmen_strikt`), as for writes (`verweigern`); a failed delete in
        // a function with "use strict" threw nothing before.
        // Versiegelt/Gefroren: eigene Props nicht loeschbar.
        // Release 15.10.2026: only seal/freeze forbid deleting; a merely
        // non-extensible object may lose properties (ECMA-262 10.1.10), and
        // strictness is the running function's (`rahmen_strikt`). MEASURED
        // before: `delete o.a` on a frozen object inside a 'use strict'
        // function returned false instead of throwing.
        let gesperrt_eigen = matches!(
            self.sperre_von(r),
            crate::heap::Sperre::Versiegelt | crate::heap::Sperre::Gefroren
        ) && self.eigene_lese(r, name).is_some();
        if gesperrt_eigen {
            // Strikt wirft, sloppy meldet false (JS-Regel).
            if self.strikt || self.rahmen_strikt() {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "versiegelte Eigenschaft"),
                ));
            }
            return Ok(false);
        }
        // J31: `configurable: false` verhindert das Loeschen.
        //
        // GEMESSEN, warum das eine echte Luecke war (alle acht Faelle mit
        // `node 22` gegessen):
        //   `var o={a:1}; delete o.a`                          -> true,  removed
        //   `defineProperty(o,'a',{value:1,configurable:false})`
        //                                      `delete o.a`   -> false, kept
        //   `defineProperty(o,'a',{value:1,configurable:true})` -> true,  removed
        //   `defineProperty(o,'a',{value:1})` (Vorgabe ist `false`)
        //                                      `delete o.a`   -> false, kept
        //   `[1,2]` mit `length` nicht beschreibbar:
        //                                      `delete o.length` -> false, kept
        //   `delete C.prototype` (Funktions-`prototype`)        -> false, kept
        //   `Object.freeze({a:1}); delete o.a`                  -> false, kept
        //                            Motor **vorher**          -> true,  removed
        //                            (also: **immer** geloescht)
        //
        // GEMESSEN, wie die Suite darueber stolpert: `harness/
        // propertyHelper.js:122` prueft `isConfigurable(obj, name)` — und
        // das ist **kein** Deskriptorvergleich, sondern ein **Versuch**:
        //     function isConfigurable(obj, name) { try { delete obj[name] }
        //       catch (e) {} return !hasOwnProperty(obj, name) }
        // Ein Motor, der `configurable` ignoriert, loescht auch dann, und
        // die Suite meldet „prop descriptor should not be configurable" —
        // eine Meldung, die nach einem **Deskriptor**-Fehler aussieht und
        // es nicht ist. 18 Faelle in `built-ins/Object/create/`.
        //
        // ## J32 (JS2/QUIC, 02.10.2026, 20:20): DIE BEDINGUNG WAR ZU ENG —
        // und ihre **Begruendung** war der eigentliche Fund.
        //
        // Bis eben stand hier:
        // ```text
        //     if index_aus_schluessel(name).is_none()          // <- das hier
        //         && self.eigene_lese(r, name).is_some()
        //         && !self.eig_ist_konfigurierbar(GVal::Ref(r), name)
        // ```
        // mit der Begruendung: *„die Elemente eines Feldes sind per
        // `CreateDataProperty` immer konfigurierbar, und
        // `eig_ist_konfigurierbar` liefert da `true`"*. **Diese Begruendung
        // stimmt — und macht die Bedingung ueberfluessig.** Sie beweist
        // naemlich, dass `eig_ist_konfigurierbar` bei einem Feld-Index ohnehin
        // `true` liefert (der Index liegt in `elemente`, nicht in `rest`, also
        // greift `unwrap_or(true)`), also **haette** der Aufruf nie etwas
        // geaendert. Und fuer alles **andere** hat er nur **verhindert**,
        // dass geprueft wird.
        //
        // GEMESSEN (`Array/prototype/copyWithin/return-abrupt-from-delete-target.js`,
        // `node 22` -> `TypeError`, bei uns **kein** Wurf):
        // ```js
        // var o = { length: 43 };
        // Object.defineProperty(o, '42', { configurable: false, writable: true });
        // Array.prototype.copyWithin.call(o, 42, 0);   // muss TypeError werfen
        // ```
        // `o` ist ein **gewoehnliches** Objekt. `'42'` ist fuer ihn kein
        // Element, sondern ein ganz **normaler String-Schluessel** in
        // `eigenschaften` — und der darf `configurable: false` sein.
        // `index_aus_schluessel("42")` ist aber `Some(42)`, also hat die
        // Bedingung die Pruefung **uebersprungen**, `copyWithin` hat
        // `DeletePropertyOrThrow` mit `true` beantwortet bekommen und die
        // Eigenschaft geloescht.
        //
        // **GEMESSEN, dass `delete` selbst richtig war** — das war der
        // Springhaken, denn es haette nahelegen, `loesche` sei falsch:
        // ```js
        // var p = {}; Object.defineProperty(p,'a',{configurable:false,writable:true});
        // delete p.a;                     // sloppy -> kein Wurf, `a` bleibt
        // Object.getOwnPropertyNames(p)   // ['a']  <- node: ['a']
        // ```
        // `a` ist kein Indexschluessel, also lief die Pruefung — und sie
        // stimmte. **Nur der Indexzweig war blind.**
        //
        // **WARUM DAS ENTFALLEN DARF:** die Bedingung pruefte *„ist es ein
        // Indexschluessel?“* statt *„liegt er in `elemente` eines Feldes?“*.
        // Nur die zweite Frage sagt etwas ueber die Konfigurierbarkeit.
        // Fuer ein `HObj::Feld` liefert `eig_ist_konfigurierbar` weiterhin
        // `true` (der Index ist nicht in `rest`) — **das Verhalten der
        // Feld-Elemente aendert sich also nicht**, es wird nur nicht mehr
        // vom Zufall eines Schluesselformats abhaengig gemacht.
        // `length` eines Feldes steht weiterhin **ausdruecklich** oben, weil
        // es dort `configurable: false` ist (ES2023 10.4.2.4).
        //
        // **WIRKUNG, GEMESSEN:** `Array/prototype/copyWithin/return-abrupt-
        // from-delete-target.js` wird gruen. Und `J31` oben (Zeichenkette)
        // bleibt unberuehrt, weil es **vor** dieser Stelle steht.
        if name == "length" && matches!(self.heap.holen(r), HObj::Feld { .. }) {
            // ES2023 10.4.2.4: `length` ist `configurable: false`.
            if self.strikt || self.rahmen_strikt() {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "'length' ist nicht konfigurierbar"),
                ));
            }
            return Ok(false);
        }
        // J31: dasselbe fuer die **Zeichenkette**. GEMESSEN (node 22):
        // `var s = 'x'; delete s[0]` -> `false`, Motor `true`.
        //
        // GEMESSEN, warum: die Indexeigenschaften einer Zeichenkette sind
        // laut Spec (ES2023 10.4.3, `StringCreate`) `writable: false,
        // enumerable: true, configurable: false` — sie sind auf dem
        // **Wrapper**, und der wird bei jedem Zugriff neu gemacht. Sie
        // koennen deshalb nicht geloescht werden; `delete` meldet `false`
        // (sloppy) bzw. wirft `TypeError` (strikt). `length` ebenso
        // (`configurable: false`).
        if matches!(self.heap.holen(r), HObj::Zeichenkette(_)) {
            if self.strikt || self.rahmen_strikt() {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Zeichenkette ist nicht konfigurierbar"),
                ));
            }
            return Ok(false);
        }
        if self.eigene_lese(r, name).is_some() && !self.eig_ist_konfigurierbar(GVal::Ref(r), name) {
            if self.strikt || self.rahmen_strikt() {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Eigenschaft ist nicht konfigurierbar"),
                ));
            }
            return Ok(false);
        }
        if let Some(i) = index_aus_schluessel(name) {
            let o = self.heap.holen_mut(r);
            if let HObj::Feld { elemente, .. } = o {
                if i < elemente.len() {
                    elemente[i] = GVal::Undefiniert;
                }
                // sec-13.1.1 mit sec-12.2.2: `delete a[1]` macht eine
                // Luecke (J7), kein `undefined`.
                crate::heap::feld_luecke_markieren(o, i);
                return Ok(true);
            }
        }
        // Die Reihenfolgeliste mitpflegen: eine geloeschte (und neu
        // gesetzte) Eigenschaft steht danach wieder am Ende — wie in node.
        if let HObj::Gegenstand {
            eigenschaften,
            reihenfolge,
            ..
        } = self.heap.holen_mut(r)
        {
            eigenschaften.remove(name);
            reihenfolge.retain(|k| k != name);
            return Ok(true);
        }
        let o = self.heap.holen_mut(r);
        let map = match o {
            HObj::Gegenstand { eigenschaften, .. } => Some(eigenschaften),
            HObj::Verpackt { eigenschaften, .. } => Some(eigenschaften),
            HObj::Feld { rest, .. } => Some(rest),
            HObj::Funktion { eigenschaften, .. } | HObj::Nativ { eigenschaften, .. } => {
                Some(eigenschaften)
            }
            HObj::Generator { eigenschaften, .. }
            | HObj::Fehler { eigenschaften, .. }
            | HObj::Datum { eigenschaften, .. }
            | HObj::Regex { eigenschaften, .. }
            | HObj::Karte { eigenschaften, .. }
            | HObj::Menge { eigenschaften, .. }
            | HObj::BytePuffer { eigenschaften, .. }
            | HObj::TypedArray { eigenschaften, .. }
            | HObj::Versprechen { eigenschaften, .. } => Some(eigenschaften),
            _ => None,
        };
        // Fehlende Eigenschaft loeschen ist true (alles konfigurierbar).
        match map {
            Some(m) => {
                m.remove(name);
                Ok(true)
            }
            None => Ok(true),
        }
    }

    /// for-in-Schluessel: eigene aufzaehlbare + Proto-Kette (ohne Duplikate).
    /// Release 2026-10-15: `for-in` over a host object lists the host's own
    /// keys (collection indices and names, the document's named elements)
    /// first and never the machine fields `__host`/`__art`. MEASURED before
    /// (WPT document.forms "iteration"): `for (p in document.forms)` gave
    /// "__host", "__art".
    pub(crate) fn schluessel_werte(&mut self, v: GVal) -> Result<Vec<String>, Stoerung> {
        if let GVal::Ref(r) = v
            && self.eigene_lese(r, "__host").is_some()
        {
            let mut out = self.host_eigene_schluessel(v).unwrap_or_default();
            let mut gesehen: HashSet<String> = out.iter().cloned().collect();
            for k in self.schluessel_werte_basis(v)? {
                if k != "__host" && k != "__art" && gesehen.insert(k.clone()) {
                    out.push(k);
                }
            }
            return Ok(out);
        }
        self.schluessel_werte_basis(v)
    }

    fn schluessel_werte_basis(&mut self, v: GVal) -> Result<Vec<String>, Stoerung> {
        // Release 10.10.2026: `for-in` ueber einen Proxy — seine
        // aufzaehlbaren eigenen Schluessel (ownKeys + getOwnPropertyDescriptor).
        if let GVal::Ref(p) = v
            && matches!(self.heap.holen(p), HObj::Proxy { .. })
        {
            return self.proxy_aufzaehlbare_schluessel(p);
        }
        match v {
            GVal::Undefiniert | GVal::Null => Ok(Vec::new()),
            // J24: ein BigInt hat keine aufzaehlbaren eigenen Schluessel
            // (wie eine Zahl).
            GVal::Bool(_) | GVal::Zahl(_) | GVal::Ganz(_) => Ok(Vec::new()),
            GVal::Ref(r) => {
                // J8b: erst die Art pruefen, dann kopieren (vorher kopierte
                // JEDER for-in eines nicht-Textes das ganze `HObj`).
                if matches!(self.heap.holen(r), HObj::Zeichenkette(_))
                    && let HObj::Zeichenkette(s) = self.heap.holen(r).clone()
                {
                    let n = s.encode_utf16().count();
                    if n > MAX_SAMMLUNG {
                        return Err(Stoerung::Fehler(JsFehler::Speicher(
                            "String zu gross fuer for-in".to_string(),
                        )));
                    }
                    return Ok((0..n).map(|i| i.to_string()).collect());
                }
                // J-CACHE (05.10., Freigabe Leitung): Regex-eigene aus der
                // Map (defineProperty legt dort ab) — NUR aufzaehlbare
                // (for-in-Filter wie in der Schleife unten; defineProperties
                // prueft zusaetzlich via eig_ist_aufzaehlbar).
                // Gehoert zu den Regex-Armen in map_kopieren/
                // n_objekt_namen/eig_ist_aufzaehlbar (eine Ursache).
                if matches!(self.heap.holen(r), HObj::Regex { .. })
                    && let HObj::Regex { eigenschaften, .. } = self.heap.holen(r).clone()
                {
                    return Ok(eigenschaften
                        .iter()
                        .filter(|(_, e)| e.aufzählbar)
                        .map(|(k, _)| k.clone())
                        .collect());
                }
                let mut out = Vec::new();
                let mut gesehen = HashSet::new();
                // Feld-Indizes zuerst.
                if let HObj::Feld { elemente, .. } = self.heap.holen(r) {
                    if elemente.len() > MAX_SAMMLUNG {
                        return Err(Stoerung::Fehler(JsFehler::Speicher(
                            "Feld zu gross fuer for-in".to_string(),
                        )));
                    }
                    for i in 0..elemente.len() {
                        // Luecken sind nicht aufzaehlbar (J7).
                        if crate::heap::feld_ist_luecke(self.heap.holen(r), i) {
                            continue;
                        }
                        out.push(i.to_string());
                        gesehen.insert(i.to_string());
                    }
                }
                if let HObj::Verpackt {
                    elemente: Some(el), ..
                } = self.heap.holen(r)
                {
                    for i in 0..el.len() {
                        out.push(i.to_string());
                        gesehen.insert(i.to_string());
                    }
                }
                // Release 15.10.2026: `for (k in typedArray)` visits its
                // indices. MEASURED before: none (node 22: "0,1,2").
                if let HObj::TypedArray { laenge, .. } = self.heap.holen(r) {
                    let n = *laenge;
                    if n > MAX_SAMMLUNG {
                        return Err(Stoerung::Fehler(JsFehler::Speicher(
                            "TypedArray zu gross fuer for-in".to_string(),
                        )));
                    }
                    for i in 0..n {
                        out.push(i.to_string());
                        gesehen.insert(i.to_string());
                    }
                }
                let mut cur = Some(r);
                let mut rest = 10_000;
                while let Some(o) = cur {
                    rest -= 1;
                    if rest == 0 {
                        break;
                    }
                    // M4: Schluessel-Menge gedeckelt.
                    if out.len() > MAX_SAMMLUNG {
                        return Err(Stoerung::Fehler(JsFehler::Speicher(
                            "zu viele Schluessel".to_string(),
                        )));
                    }
                    let (mut keys, p) = match self.heap.holen(o) {
                        // GEMESSEN: die eigene Reihenfolge des Gegenstands
                        // (Einfuegereihenfolge wie in node) statt der
                        // HashMap-Reihenfolge — sonst kam der Spread
                        // `{a:1, ...{b:2,c:3}}` als `a,c,b` heraus (p2.js Z2).
                        HObj::Gegenstand {
                            proto,
                            eigenschaften,
                            reihenfolge,
                            ..
                        } => {
                            let mut ks: Vec<(String, bool)> =
                                Vec::with_capacity(eigenschaften.len());
                            for k in reihenfolge.iter() {
                                if let Some(e) = eigenschaften.get(k)
                                    && !ks.iter().any(|x: &(String, bool)| &x.0 == k)
                                {
                                    ks.push((k.clone(), e.aufzählbar));
                                }
                            }
                            // ## W38 (QUIC, 04.10.2026) — **die Reste SORTIEREN statt in
                            // HashMap-Reihenfolge anhaengen.** Auftrag aus dem
                            // BRETT-Eintrag von JS (`JS_6`), der die Schwankung
                            // meiner beiden `forEach`-Faelle erklaert hat.
                            //
                            // **WARUM ÜBERHAUPT SORTIEREN.** `reihenfolge` haelt
                            // die Einfuegereihenfolge, und der Arm oben
                            // benutzt sie — aber **32 von 35** Einfuegestellen
                            // pflegen sie **nicht** (JS_6, mit Zahlen pro
                            // Datei). Alles, was ueber `Object.assign`,
                            // `defineProperty` oder das Anlegen eingebauter
                            // Objekte entsteht, liegt deshalb **nur** in der
                            // `HashMap` — und `HashMap` heisst `RandomState`,
                            // also **pro Prozess eine andere Reihenfolge**.
                            //
                            // **DAS WAR MEINE FEHLERMELDUNG.** Zwei Laeufe,
                            // dieselbe Quelle: einmal `1901` gruen, einmal
                            // `1900`. Ein Test, der eine **Reihenfolge**
                            // prueft, war damit **nie stabil** — und
                            // „isoliert gruen, im Lauf rot" sah erst nach
                            // Zufall aus.
                            //
                            // **WAS SORTIEREN KOSTET — und es ist nicht
                            // „nichts":** es ist **nicht** die
                            // Einfuegereihenfolge der Norm. Fuer diese
                            // Schluessel **gibt es sie nicht**, weil niemand sie
                            // aufgezeichnet hat; eine **bestimmte** Reihenfolge
                            // ist aber immer besser als eine **zufaellige**, und
                            // sie ist der ganze Unterschied zwischen einem
                            // Befund und einem Zufall. *Determinismus ist hier
                            // kein Ziel neben der Norm, sondern die Bedingung,
                            // unter der man die Norm ueberhaupt messen kann.*
                            //
                            // **WARUM NICHT `reihenfolge.push` nachziehen** (die
                            // andere der beiden in JS_6 genannten Wege): **32
                            // Stellen**, und wer eine vergisst, bekommt denselben
                            // Fehler wieder. Hier ist es **eine** Stelle, und
                            // sie wirkt **fuer alle 32**, auch fuer die, die
                            // jemand in einem Jahr hinzufuegt.
                            let mut reste: Vec<(String, bool)> = Vec::new();
                            for (k, e) in eigenschaften.iter() {
                                if !ks.iter().any(|x: &(String, bool)| &x.0 == k) {
                                    reste.push((k.clone(), e.aufzählbar));
                                }
                            }
                            reste.sort_by(|a, b| a.0.cmp(&b.0));
                            ks.extend(reste);
                            (ks, *proto)
                        }
                        HObj::Verpackt {
                            proto,
                            eigenschaften,
                            ..
                        } => {
                            let mut ks: Vec<(String, bool)> = eigenschaften
                                .iter()
                                .map(|(k, e)| (k.clone(), e.aufzählbar))
                                .collect();
                            // W9: keine Einfuegereihenfolge vorhanden -> strikt
                            // sortieren, sonst kommt die `HashMap`-Reihenfolge
                            // (`RandomState`) heraus.
                            crate::heap::ordne_schluessel_paare_strikt(&mut ks);
                            (ks, *proto)
                        }
                        HObj::Funktion {
                            proto,
                            eigenschaften,
                            ..
                        }
                        | HObj::Nativ {
                            proto,
                            eigenschaften,
                            ..
                        }
                        | HObj::Fehler {
                            proto,
                            eigenschaften,
                            ..
                        }
                        | HObj::Datum {
                            proto,
                            eigenschaften,
                            ..
                        }
                        | HObj::Regex {
                            proto,
                            eigenschaften,
                            ..
                        }
                        | HObj::Karte {
                            proto,
                            eigenschaften,
                            ..
                        }
                        | HObj::Menge {
                            proto,
                            eigenschaften,
                            ..
                        }
                        | HObj::Versprechen {
                            proto,
                            eigenschaften,
                            ..
                        } => {
                            let mut ks: Vec<(String, bool)> = eigenschaften
                                .iter()
                                .map(|(k, e)| (k.clone(), e.aufzählbar))
                                .collect();
                            // W9: wie beim `Verpackt`-Arm — strikt sortieren.
                            crate::heap::ordne_schluessel_paare_strikt(&mut ks);
                            (ks, *proto)
                        }
                        HObj::Feld { proto, rest, .. } => {
                            let mut ks: Vec<(String, bool)> = rest
                                .iter()
                                .map(|(k, e)| (k.clone(), e.aufzählbar))
                                .collect();
                            // W9: die Nicht-Index-Felder haben keine
                            // Einfuegereihenfolge -> strikt sortieren.
                            crate::heap::ordne_schluessel_paare_strikt(&mut ks);
                            (ks, *proto)
                        }
                        _ => (Vec::new(), self.heap_proto_von(o)),
                    };
                    // sec-6.1.7 auch im `for-in`: Zahlschluessel eines
                    // Objekts zuerst aufsteigend (J5).
                    crate::heap::ordne_schluessel_paare(&mut keys);
                    for (k, aufz) in keys {
                        // J11: private Felder (`#x`) erscheinen nicht im
                        // `for…in` (node 22: nur `y`).
                        if crate::heap::ist_privat(&k) {
                            continue;
                        }
                        if aufz && gesehen.insert(k.clone()) {
                            out.push(k);
                        }
                    }
                    cur = p;
                }
                Ok(out)
            }
        }
    }

    /// for-of/Spread-Werte (Feld, String, Map, Set). Sonst TypeError.
    ///
    /// Release 15.10.2026: the values come back ROOTED (on `wurzeln`, so the
    /// caller's frame or native call releases them) unless they are the
    /// elements of an array, which keeps them alive itself. MEASURED before
    /// with `NARSIL_JS_GC_STRESS=1`: `Array.from('a.b'.matchAll('.'), m =>
    /// m.index)` read a swept match ("Heap: toter Verweis"), because the
    /// iterator results were only held by this Rust `Vec` while the mapper
    /// ran — the same class as the JS-thread panic on chip.de (a network
    /// reaction calling into such a native).
    pub(crate) fn iter_werte(&mut self, v: GVal) -> Result<Vec<GVal>, Stoerung> {
        let eigene_elemente = matches!(v, GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Feld { .. }))
            && self.iterator_ist_eingebaut_feld(v);
        let werte = self.iter_werte_innen(v)?;
        if !eigene_elemente {
            self.wurzeln.extend(werte.iter().copied().filter(|w| w.ist_ref()));
        }
        Ok(werte)
    }

    fn iter_werte_innen(&mut self, v: GVal) -> Result<Vec<GVal>, Stoerung> {
        match v {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Feld { elemente, .. } => {
                    // J36: der Schnellweg nur, wenn `@@iterator` **noch der
                    // eingebaute** ist. GEMESSEN (node 22):
                    //   `Array.prototype[Symbol.iterator] = function*(){
                    //      yield this[0]; yield this[1]; yield 42 }`
                    //   `for (var x of [1,2,3])` -> der eigene Iterator laeuft
                    // Ohne diese Frage lief `for…of` **immer** ueber die
                    // Indizes und bemerkte die Ersetzung nie.
                    if !self.iterator_ist_eingebaut_feld(v) {
                        return self.iterator_werte(v);
                    }
                    if elemente.len() > MAX_SAMMLUNG {
                        return Err(Stoerung::Fehler(JsFehler::Speicher(
                            "Feld zu gross zum Iterieren".to_string(),
                        )));
                    }
                    Ok(elemente)
                }
                HObj::Zeichenkette(s) => {
                    // J30: die Grenze ist `Grenzen::max_string_laenge`, nicht
                    // eine hart kodierte Konstante.
                    //
                    // GEMESSEN, warum: `MAX_SAMMLUNG * 4` sind 4.000.000
                    // Byte, und die test262-Faelle in
                    // `RegExp/property-escapes/generated/` bauen mit
                    // `buildString` einen Text ueber **alle** Codepunkte
                    // (`[0x000000,0x00DBFF]` und `[0x00E000,0x10FFFF]`) —
                    // 1.114.112 Codepunkte, als UTF-8 rund 4,4 MB. Die
                    // Meldung war `Speichergrenze: String zu gross zum
                    // Iterieren`, also **knapp** daneben.
                    //
                    // GEMESSEN, warum das Feld und nicht eine groessere
                    // Konstante: `max_string_laenge` ist die Grenze, an der
                    // der Motor sonst schon Zeichenketten begrenzt
                    // (`dom.rs:126`, `builtin2.rs:1395`) — dieselbe Frage
                    // muss ueberall dieselbe Grenze haben. Und
                    // `Grenzen::bombe()` (200.000) ist **straffer** als
                    // 4.000.000, die Bomben enden also weiterhin in
                    // Millisekunden.
                    if s.len() > self.grenzen.max_string_laenge {
                        return Err(Stoerung::Fehler(JsFehler::Speicher(
                            "String zu gross zum Iterieren".to_string(),
                        )));
                    }
                    let mut out = Vec::new();
                    for c in s.chars() {
                        let mut buf = String::new();
                        buf.push(c);
                        out.push(self.text_wert(&buf).map_err(Stoerung::Fehler)?);
                    }
                    Ok(out)
                }
                HObj::Verpackt {
                    elemente: Some(el), ..
                } => Ok(el),
                HObj::Verpackt {
                    primitiv: GVal::Ref(sr),
                    ..
                } => self.iter_werte(GVal::Ref(sr)),
                // Release 10.10.2026: nur die BELEGTEN Records (W35, siehe
                // `belegt`). GEMESSEN vorher (node 22 in Klammern):
                //   `s = new Set(['a','b']); s.delete('a'); [...s].join()`
                //       -> `a,b`  (`b`) — ebenso `for…of`, `Array.from`, `[...map]`
                // `values()`/`keys()` gingen ueber den lebendigen Iterator und
                // waren richtig; nur dieser Schnellweg las die Liste roh.
                HObj::Karte { eintraege, belegt, .. } => {
                    let mut out = Vec::with_capacity(eintraege.len().min(MAX_SAMMLUNG));
                    for (i, (k, val)) in eintraege.into_iter().enumerate() {
                        if !belegt.get(i).copied().unwrap_or(true) {
                            continue;
                        }
                        if out.len() >= MAX_SAMMLUNG {
                            return Err(Stoerung::Fehler(JsFehler::Speicher(
                                "Map zu gross zum Iterieren".to_string(),
                            )));
                        }
                        let paar = [k, val];
                        let f = self.feld_aus_werten(&paar)?;
                        // Release 15.10.2026: rooted at once — the next pair's
                        // allocation can collect.
                        self.wurzeln.push(GVal::Ref(f));
                        out.push(GVal::Ref(f));
                    }
                    Ok(out)
                }
                HObj::Menge { eintraege, belegt, .. } => Ok(eintraege
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| belegt.get(*i).copied().unwrap_or(true))
                    .map(|(_, w)| w)
                    .collect()),
                // J27: **kein `println!` mehr im Lauf.** GEMESSEN, warum:
                // der test262-Lauf ueber die echte Suite hat auf stderr
                // Zeilen wie `ITER FAIL! HObj=Gegenstand { … }` gedruckt
                // — bei 3552 Faellen sind das Hunderte Zeilen Rauschen,
                // die den Lauf unlesbar machen. Ausserdem ist `println!`
                // im Motor eine **Ausgabe**, die der Host nicht sieht
                // (nur stderr) — sie gehoert in einen Fehlerwert, und der
                // steht eine Zeile darunter.
                // J32: **der allgemeine Weg** — ES2023 7.4.7 `GetIterator`:
                // ein Objekt, das selbst kein Feld/String/Map/Set ist, kann
                // trotzdem `[Symbol.iterator]` haben. GEMESSEN (vorher):
                //   `var o={}; o[Symbol.iterator]=function(){…}; Promise.all(o)`
                //   -> `TypeError: Wert ist nicht iterierbar`
                // (node: `rejects=1` und ein Promise-Objekt zurueck)
                //
                // GEMESSEN, warum der Schluessel **`@sym1`** ist: die
                // Symbole bekommen ihren Schlüssel als Text (`symbole::
                // symbol_text`), und `Symbol.iterator` ist das **erste**
                // wohlbekannte Symbol (`n_symbol_bekannt`, uid 1). Damit
                // findet derselbe Text den vom Programm gesetzten Wert
                // wieder — es ist derselbe Weg, den `o[Symbol.iterator]`
                // beim Setzen und beim Lesen geht.
                //
                // Die **Reihenfolge** ist gewollt: die schnellen Faelle
                // (Feld, String, Map, Set, Box) stehen **vor** diesem Arm
                // und kosten damit keinen Aufruf. `for…of` ueber ein Feld
                // ist der haeufigste Fall der Suite.
                _ => self.iterator_werte(v),
            },
            _ => self.iterator_werte(v),
        }
    }

    /// J32: der allgemeine Iteratorweg (ES2023 7.4.7 `GetIterator`,
    /// 7.4.9 `IteratorNext`/`IteratorValue`).
    ///
    /// Frueher gab es **keinen** — `iter_werte` kannte nur Feld, String,
    /// Map, Set und Box. Damit waren `Promise.all` ueber einen eigenen
    /// `@@iterator`, `for…of` darueber, der Spread und `Array.from` alle
    /// tot. GEMESSEN (node 22) an `Promise.all.call(Bad, o)` mit
    /// `o[Symbol.iterator]`: **sieben** von sieben `return`-Varianten
    /// ergeben `rejects=1` und ein Promise-Objekt; der Motor warf
    /// siebenmal **`TypeError: Wert ist nicht iterierbar`** und rief
    /// `reject` **null** Mal.
    ///
    /// M3/M4: `@@iterator`, `next` und ein Getter koennen Benutzercode
    /// aufrufen; dessen Kosten bucht der Aufrufmechanismus
    /// (`aufruf_synchron`). Die **Schleife selbst** ist Buchhaltung des
    /// Motors und deshalb an `MAX_SAMMLUNG` begrenzt — ein Iterator, der
    /// nie `done` liefert, darf den Motor nicht festhalten. Es folgt keine
    /// `IteratorClose`: der Motor sammelt **vollstaendig** (die Stellen, die
    /// frueh abbrechen, sind `break`/`return` in der Sprechhandlung und
    /// laufen ueber `iter_werte` nicht) — dokumentiert als offener Punkt.
    fn iterator_werte(&mut self, v: GVal) -> Result<Vec<GVal>, Stoerung> {
        // ES2023 7.4.7 Schritt 3: `GetMethod(iterable, @@iterator)`.
        //
        // DER SCHLUESSEL WIRD **ABGELEITET**, nicht hartkodiert: Eigenschaften
        // liegen im Heap als **Text**, und ein berechneter Zugriff
        // (`o[sym]`) macht daraus `schluessel_text(sym)` — bei einem Symbol
        // also `String(sym)`.
        //
        // GEMESSEN (die erste Fassung riet `@sym1` aus `symbole::
        // symbol_text` — **falsch**, und `o[Symbol.iterator]` blieb
        // ungefunden, also blieb alles bei `TypeError`):
        //   `String(Symbol.iterator)` -> `"Symbol(Symbol.iterator)"`
        //   `Symbol.iterator.description` -> `"Symbol.iterator"`
        //   (node 22 liefert dieselben zwei Werte)
        //
        // GEMESSEN, warum **ableiten** und nicht den Text einsetzen: der Text
        // ist eine Konvention des Heap (siehe `schluessel_text`), und wenn er
        // sich aendert, muss der Motor **nicht** an zwei Stellen mitgezogen
        // werden. `n_symbol_bekannt` liefert dasselbe Symbol-Objekt, das der
        // Programmtext bei `o[Symbol.iterator]` auch benutzt — es ist damit
        // derselbe Schluessel, den der Aufrufer gesetzt hat.
        //
        // BEKANNTE GRENZE des Text-Schluessels: zwei **verschiedene**
        // Symbole mit derselben Beschreibung teilen sich den Feldnamen
        // (`Symbol('a') !== Symbol('a')`, aber beide ergeben `Symbol(a)`).
        // Das ist eine Eigenschaft des Heap, nicht dieses Aufrufs, und wird
        // hier nicht kaschiert.
        let sym =
            self.n_symbol_bekannt(GVal::Undefiniert, &[], crate::builtin::Nativ::SymIterator)?;
        let sym_schluessel = self.schluessel_text(sym)?;
        let meth = self.hole(v, &sym_schluessel)?;
        // Schritt 4: `undefined`/`null` -> nicht iterierbar; nicht
        // aufrufbar -> TypeError. Beide melden dieselbe Sache, damit die
        // bisherigen Texte stabil bleiben.
        if matches!(meth, GVal::Undefiniert | GVal::Null) || !self.ist_aufrufbar(meth) {
            return self.nicht_iterierbar();
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(v);
        self.wurzeln.push(meth);
        let r = self.iterator_treiben(meth, v);
        self.wurzel_zurueck(marke);
        r
    }

    /// J32: der Teil von `iterator_werte`, der den Iterator **faehrt**.
    /// Getrennt, damit die Wurzelmarke nur **ein** Mal gesetzt wird.
    fn iterator_treiben(&mut self, meth: GVal, v: GVal) -> Result<Vec<GVal>, Stoerung> {
        let iter = self.aufruf_synchron(meth, v, &[], false)?;
        if !self.ist_objekt(iter) {
            return self.nicht_iterierbar();
        }
        self.wurzeln.push(iter);
        let mut out: Vec<GVal> = Vec::new();
        let mut n = 0usize;
        loop {
            n = n.saturating_add(1);
            if n > MAX_SAMMLUNG {
                return Err(Stoerung::Fehler(JsFehler::Speicher(
                    "Iterator zu gross zum Iterieren".to_string(),
                )));
            }
            // ES2023 7.4.9 `IteratorNext`: `next` holen und aufrufen.
            let next = self.hole(iter, "next")?;
            if !self.ist_aufrufbar(next) {
                return self.nicht_iterierbar();
            }
            self.wurzeln.push(next);
            let schritt = self.aufruf_synchron(next, iter, &[], false)?;
            if !self.ist_objekt(schritt) {
                return self.nicht_iterierbar();
            }
            self.wurzeln.push(schritt);
            // `IteratorComplete` / `IteratorValue` (ES2023 7.4.9).
            // Zwei Schritte, kein `self` in beiden: `zu_bool` nimmt `&self`,
            // `hole` `&mut self` — in einem Ausdruck leiht sich das nicht.
            let done = self.hole(schritt, "done")?;
            let fertig = self.zu_bool(done);
            if fertig {
                return Ok(out);
            }
            if out.len() >= MAX_SAMMLUNG {
                return Err(Stoerung::Fehler(JsFehler::Speicher(
                    "Iterator zu gross zum Iterieren".to_string(),
                )));
            }
            let wert = self.hole(schritt, "value")?;
            self.wurzeln.push(wert);
            out.push(wert);
        }
    }

    /// Release 10.10.2026: `GetIterator` (ES2023 7.4.7) fuer den FAULEN
    /// `for…of`-Weg — der Iterator und seine `next`-Methode (einmal geholt,
    /// `[[NextMethod]]`). Der Aufrufer legt beide im Rahmen ab
    /// (`Rahmen::iter_lauf`, dort sieht sie die GC).
    pub(crate) fn iterator_holen(&mut self, v: GVal) -> Result<(GVal, GVal), Stoerung> {
        let sym =
            self.n_symbol_bekannt(GVal::Undefiniert, &[], crate::builtin::Nativ::SymIterator)?;
        let sym_schluessel = self.schluessel_text(sym)?;
        let meth = self.hole(v, &sym_schluessel)?;
        if matches!(meth, GVal::Undefiniert | GVal::Null) || !self.ist_aufrufbar(meth) {
            return self.nicht_iterierbar();
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(v);
        self.wurzeln.push(meth);
        let ergebnis = (|| {
            let iter = self.aufruf_synchron(meth, v, &[], false)?;
            if !self.ist_objekt(iter) {
                return self.nicht_iterierbar();
            }
            self.wurzeln.push(iter);
            let next = self.hole(iter, "next")?;
            Ok((iter, next))
        })();
        self.wurzel_zurueck(marke);
        ergebnis
    }

    /// Release 10.10.2026: EIN Schritt des faulen Weges (ES2023 7.4.10
    /// `IteratorStep`): `Some(wert)` oder `None` am Ende.
    pub(crate) fn iterator_schritt(&mut self, iter: GVal, next: GVal) -> Result<Option<GVal>, Stoerung> {
        if !self.ist_aufrufbar(next) {
            return self.nicht_iterierbar();
        }
        let schritt = self.aufruf_synchron(next, iter, &[], false)?;
        if !self.ist_objekt(schritt) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Iterator result is not an object"),
            ));
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(schritt);
        let ergebnis = (|| {
            let done = self.hole(schritt, "done")?;
            if self.zu_bool(done) {
                return Ok(None);
            }
            Ok(Some(self.hole(schritt, "value")?))
        })();
        self.wurzel_zurueck(marke);
        ergebnis
    }

    /// J32: **eine** Stelle fuer „kein Iterator" (ES2023 7.4.7 Schritt 3/4,
    /// 7.4.9 Schritt 3). Sie wirft bewusst nur, damit die Aufrufer mit `?`
    /// lesbar bleiben.
    fn nicht_iterierbar<T>(&mut self) -> Result<T, Stoerung> {
        Err(Stoerung::Wurf(
            self.fehler_wert("TypeError", "Wert ist nicht iterierbar"),
        ))
    }

    // ---------------------------------------------------------------
    // Literale
    // ---------------------------------------------------------------

    pub(crate) fn regex_anlegen(&mut self, muster: &str, schalter: &str) -> Result<GVal, JsFehler> {
        // Schalter pruefen (doppelte/ungueltige -> Syntaxfehler-Objekt?
        // Nein: Regex-Literal mit ungueltigem Schalter ist SyntaxError).
        let mut gesehen = HashSet::new();
        for c in schalter.chars() {
            // Erlaubte Flags: g i m s u y d (ES2022) v (ES2024)
            if !"gimsuyvd".contains(c) || !gesehen.insert(c) {
                return Err(JsFehler::Laufzeit(format!(
                    "ungueltiger Regex-Schalter '{c}'"
                )));
            }
        }
        // Muster vorkompilieren (Mini-Engine prueft Syntax **mit** Flags,
        // damit `u`/`v` Unicode-Modus fuer Parser aktivieren).
        if let Err(msg) = crate::regexmini::pruefen_mit_flags(muster, schalter) {
            return Err(JsFehler::Laufzeit(format!("ungueltiges Regex: {msg}")));
        }
        let obj = HObj::Regex {
            proto: Some(self.proto_regex),
            muster: muster.to_string(),
            schalter: schalter.to_string(),
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
        };
        let kosten = Heap::kosten_fuer(&obj) + muster.len() + schalter.len();
        self.vor_allok(kosten)?;
        let r = self.belege(obj, kosten);
        // GEMESSEN: ein Regex-LITERAL hatte weder `source` noch `flags`
        // (nur `new RegExp(...)` bekam sie) — `/a/g.source` war "".
        self.wurzeln.push(GVal::Ref(r));
        // Ein Wurf waere hier ein JS-Wurf (kein Host-Fehler); die
        // Eigenschaften lassen sich aber nicht verweigern.
        match self.regex_eigenschaften(r) {
            Ok(()) => {}
            Err(Stoerung::Fehler(f)) => return Err(f),
            Err(Stoerung::Wurf(_)) => return Ok(GVal::Ref(r)),
        }
        Ok(GVal::Ref(r))
    }

    /// `source`, `flags`, die Schalter-Booleans und `lastIndex` setzen.
    pub(crate) fn regex_eigenschaften(&mut self, r: HeapRef) -> Result<(), Stoerung> {
        let (muster, schalter) = match self.heap.holen(r).clone() {
            HObj::Regex {
                muster, schalter, ..
            } => (muster, schalter),
            _ => return Ok(()),
        };
        let f = crate::regexmini::schalter_aus(&schalter).unwrap_or(crate::regexmini::Schalter {
            global: false,
            egal_gross: false,
            mehrzeilig: false,
            punkt_alles: false,
            klebrig: false,
        });
        let src = self.text_wert(&muster).map_err(Stoerung::Fehler)?;
        let flg = self.text_wert(&schalter).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(src);
        self.wurzeln.push(flg);
        // J-CACHE (05.10., Freigabe Leitung): wie im Konstruktor
        // (builtin3.rs) — Instanz-Props NICHT aufzaehlbar (Spec 22.2),
        // sonst sehen for-in/keys/assign/create/defineProperties die
        // internen source/flags/... als Deskriptoren. Eine Ursache,
        // zwei Schreibstellen.
        let eigen = |e: &mut Engine, n: &str, v: GVal| {
            e.eigenschaft_mit_alle_attributen(r, n, v, false, true, true)
        };
        eigen(self, "source", src)?;
        eigen(self, "flags", flg)?;
        eigen(self, "global", GVal::Bool(f.global))?;
        eigen(self, "ignoreCase", GVal::Bool(f.egal_gross))?;
        eigen(self, "multiline", GVal::Bool(f.mehrzeilig))?;
        eigen(self, "dotAll", GVal::Bool(f.punkt_alles))?;
        eigen(self, "unicode", GVal::Bool(f.klebrig))?;
        eigen(self, "sticky", GVal::Bool(false))?;
        self.eigenschaft_mit_alle_attributen(r, "lastIndex", GVal::Zahl(0.0), false, true, false)?;
        Ok(())
    }

    pub(crate) fn feld_anhang(&mut self, f: HeapRef, v: GVal) -> Result<(), JsFehler> {
        // Release 09.10.2026: Feld und Wert ueberleben einen GC in `vor_allok`
        // (der Aufrufer hat den Wert oft schon vom Stapel genommen).
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(f));
        self.wurzeln.push(v);
        let r = self.vor_allok(crate::heap::KOSTEN_EINTRAG);
        self.wurzel_zurueck(marke);
        r?;
        let o = self.heap.holen_mut(f);
        if let HObj::Feld { elemente, .. } = o {
            elemente.push(v);
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            Ok(())
        } else {
            Err(JsFehler::Laufzeit("Feld korrupt (intern)".to_string()))
        }
    }

    /// `aufzaehlbar` (J19): die Aufzaehlbarkeit ist je Stelle
    /// verschieden und war vorher ueberall `true`:
    /// * **Klassen**-Accessor: **nicht** aufzaehlbar (Spec sec-10.9) —
    ///   GEMESSEN vorher: `Object.keys(class D { get v(){ return 1 } }
    ///   .prototype)` lieferte `v`, node `[]`.
    /// * **Objektliteral**-Accessor: aufzaehlbar (node gleich).
    /// * `Object.defineProperty`: folgt dem Deskriptor, Vorgabe `false`.
    pub(crate) fn getter_setter_definieren(
        &mut self,
        o: HeapRef,
        k: &str,
        getter: Option<HeapRef>,
        setter: Option<HeapRef>,
        aufzaehlbar: bool,
    ) -> Result<(), JsFehler> {
        self.getter_setter_definieren_mit(o, k, getter, setter, aufzaehlbar, true)
    }

    /// J27: dasselbe, aber mit **eigenem** `configurable`.
    ///
    /// GEMESSEN, warum der Wert durchgereicht werden muss: `Object.
    /// defineProperty(o,'g',{get:f})` ergibt in node
    /// `configurable: false`. Ohne den zweiten Weg waere der Deskriptor
    /// eines per `defineProperty` gesetzten Accessors `configurable:
    /// true` — und der Unterschied zwischen „mit defineProperty" und
    /// „im Objektliteral" waere nicht sichtbar.
    pub(crate) fn getter_setter_definieren_mit(
        &mut self,
        o: HeapRef,
        k: &str,
        getter: Option<HeapRef>,
        setter: Option<HeapRef>,
        aufzaehlbar: bool,
        konfigurierbar: bool,
    ) -> Result<(), JsFehler> {
        self.vor_allok(crate::heap::KOSTEN_EINTRAG)?;
        // J27: ein **Accessor auf einem Feld** (Array) ist gueltig — der
        // Schreibweg kannte nur `Gegenstand`.
        //
        // GEMESSEN, wie das auffaellt: der Lauf ueber die echte
        // test262-Suite hatte **47** rote Faelle mit
        // `Laufzeitfehler: Getter nur auf Objekten` — alle in
        // `test/built-ins/Array/prototype/concat/`. Der Fall
        // `create-ctor-poisoned.js` macht
        //   `Object.defineProperty(a, 'constructor', {get: function(){throw …}})`
        // an einem `[]` und erwartet, dass `a.concat()` den Wurf
        // weitergibt (ES2023 23.1.3.3 Schritt 2 `ArraySpeciesCreate`).
        // Die Meldung war ausserdem **falsch benannt**: sie sagte
        // „Getter nur auf Objekten", gemeint war „der Schreibweg nimmt
        // nur `Gegenstand`".
        //
        // GEMESSEN, warum der **Lesepfad** schon passt: `hole` erkennt
        // einen Getter in `HObj::Feld { rest }` (siehe `ist_getter` in
        // dieser Datei) und ruft ihn. Es fehlte nur das Eintragen.
        //
        // DIE GRENZE, und sie steht hier statt versteckt: bei einem
        // **Feld** (Array) ist `length` per Spec (ES2023 10.4.2.4) eine
        // **Daten**eigenschaft ohne `configurable` — ein `get` darauf ist
        // ein `TypeError`, kein Accessor.
        //
        // GEMESSEN, warum die Grenze nur fuer ein **Feld** gilt und nicht
        // fuer jedes Objekt: `tests/js2_eigen.rs::length_getter_laeuft_
        // genau_einmal` benutzt `{get length(){ n++; return 2 }, 0:'1',
        // 1:'2'}` — ein Objektliteral, und dort ist `length` eine
        // ganz normale Eigenschaft. Die erste Fassung dieser Sperre
        // unterschied nicht und liess den Test mit
        // `Accessor auf 'length' ist nicht erlaubt` scheitern.
        //
        // Ein **Index** als Accessor wird in `rest` abgelegt;
        // `eigene_lese` sieht dort **vor** dem Element nach, damit der
        // Accessor den Index wirklich verdeckt.
        let ist_feld = matches!(self.heap.holen(o), HObj::Feld { .. });
        if ist_feld && k == "length" {
            if getter.is_some() || setter.is_some() {
                return Err(JsFehler::Laufzeit(
                    "Accessor auf der Laenge eines Feldes ist nicht erlaubt".to_string(),
                ));
            }
            return Ok(());
        }
        if let HObj::Gegenstand { eigenschaften, reihenfolge, .. } = self.heap.holen_mut(o) {
            if !eigenschaften.contains_key(k) {
                reihenfolge.push(k.to_string());
            }
        }
        let oo = self.heap.holen_mut(o);
        let map = match oo {
            HObj::Gegenstand { eigenschaften, .. }
            | HObj::Verpackt { eigenschaften, .. }
            // J27: **jeder** Objekttyp traegt eigene Eigenschaften, also
            // muss ein Accessor ueberall anlegbar sein.
            //
            // GEMESSEN, warum das eine eigene Regel ist und keine
            // Ausnahmeliste: der Lauf ueber die echte Suite hatte **35**
            // rote Faelle mit `Accessor auf diesem Objekt nicht anlegbar`
            // — der Fall `Object/create/15.2.3.5-4-11.js` macht
            //   `Object.defineProperty(new Date(0), 'prop', {get: …})`
            // und verlangt, dass der Getter laeuft (`this instanceof
            // Date`). Die Liste hier ist **die** Liste der map-tragenden
            // Typen — sie ist identisch mit der in `eigene_lese` und in
            // `n_objekt_namen`; damit gibt es fuer „wo liegen eigene
            // Eigenschaften" eine einzige Wahrheit und nicht drei
            // Abschriften.
            | HObj::Funktion { eigenschaften, .. }
            | HObj::Nativ { eigenschaften, .. }
            | HObj::Generator { eigenschaften, .. }
            | HObj::Fehler { eigenschaften, .. }
            | HObj::Datum { eigenschaften, .. }
            | HObj::Regex { eigenschaften, .. }
            | HObj::Karte { eigenschaften, .. }
            | HObj::Menge { eigenschaften, .. }
            | HObj::BytePuffer { eigenschaften, .. }
            | HObj::TypedArray { eigenschaften, .. }
            | HObj::Versprechen { eigenschaften, .. } => eigenschaften,
            HObj::Feld { rest, .. } => rest,
            _ => {
                return Err(JsFehler::Laufzeit(
                    "Accessor auf diesem Objekt nicht anlegbar".to_string(),
                ));
            }
        };
        let e = map.entry(k.to_string()).or_insert(Eigenschaft {
            wert: GVal::Undefiniert,
            getter: None,
            setter: None,
            aufzählbar: aufzaehlbar,
            beschreibbar: true,
            konfigurierbar,
        });
        // Bestehende Eigenschaft: die Aufzaehlbarkeit wird mitgezogen
        // (`Object.defineProperty` darf sie setzen, ein Klassen-Accessor
        // nicht).
        if !aufzaehlbar {
            e.aufzählbar = false;
        }
        if getter.is_some() {
            e.getter = getter;
        }
        if setter.is_some() {
            e.setter = setter;
        }
        self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
        Ok(())
    }

    pub(crate) fn streu_kopieren(&mut self, ziel: HeapRef, quelle: GVal) -> Result<(), Stoerung> {
        // Release 15.10.2026: CopyDataProperties defines (CreateDataProperty,
        // ECMA-262 7.3.25), it does not [[Set]]: an Object.prototype setter
        // swallowed `{...{zz: 2}}.zz` before.
        // Release 10.10.2026: `{...proxy}` — die aufzaehlbaren eigenen
        // Schluessel des Proxys (ownKeys + getOwnPropertyDescriptor), Werte
        // ueber `[[Get]]`.
        if let GVal::Ref(p) = quelle
            && matches!(self.heap.holen(p), HObj::Proxy { .. })
        {
            for k in self.proxy_aufzaehlbare_schluessel(p)? {
                let w = self.proxy_get(p, &k, quelle)?;
                self.wurzeln.push(w);
                self.create_data_property_or_throw(ziel, &k, w)?;
            }
            return Ok(());
        }
        match quelle {
            GVal::Undefiniert | GVal::Null => Ok(()),
            GVal::Ref(r) => {
                for (k, e) in self.eigene_map(r) {
                    if e.aufzählbar {
                        let w = self.hole(quelle, &k)?;
                        self.wurzeln.push(w);
                        self.create_data_property_or_throw(ziel, &k, w)?;
                    }
                }
                Ok(())
            }
            _ => {
                // Primitiv einpacken und eigene Keys kopieren (String: Indizes).
                let r = self.zu_objekt(quelle)?;
                for (k, e) in self.eigene_map(r) {
                    if e.aufzählbar {
                        let w = self.hole(GVal::Ref(r), &k)?;
                        self.wurzeln.push(w);
                        self.create_data_property_or_throw(ziel, &k, w)?;
                    }
                }
                Ok(())
            }
        }
    }

    // ---------------------------------------------------------------
    // Zugriff / Zuweisung / Update / Operatoren
    // ---------------------------------------------------------------

    pub(crate) fn in_operator(&mut self, k: &str, r: GVal) -> bool {
        let rr = match r {
            GVal::Ref(r) => r,
            _ => return false,
        };
        // Release 10.10.2026: `[[HasProperty]]` eines Proxys (ein Wurf der
        // Falle wird hier zu `false` — `in_operator` liefert kein Result).
        if matches!(self.heap.holen(rr), HObj::Proxy { .. }) {
            return self.proxy_has(rr, k).unwrap_or(false);
        }
        let mut cur = Some(rr);
        let mut rest = 10_000;
        while let Some(o) = cur {
            rest -= 1;
            if rest == 0 {
                break;
            }
            // Release 15.10.2026: an integer index on a typed array answers
            // itself, without the prototype chain (10.4.5.2 [[HasProperty]]).
            // MEASURED before: `'0' in new Uint8Array(1)` was false.
            if matches!(self.heap.holen(o), HObj::TypedArray { .. })
                && let Some(i) = index_aus_schluessel(k)
                && let HObj::TypedArray { laenge, .. } = self.heap.holen(o)
            {
                return i < *laenge;
            }
            if let HObj::Feld { elemente, .. } = self.heap.holen(o) {
                if k == "length" {
                    return true;
                }
                if let Some(i) = index_aus_schluessel(k) {
                    // J7: eine Luecke gibt es nicht (`1 in a` ist falsch,
                    // obwohl `a[1]` `undefined` liefert).
                    if i < elemente.len() && !crate::heap::feld_ist_luecke(self.heap.holen(o), i) {
                        return true;
                    }
                }
            }
            let (eigene, p) = self.objekt_eigene_proto(o, k);
            if eigene.is_some() {
                return true;
            }
            cur = p;
        }
        // Release 10.10.2026: HOST-Objekte kennen ihre Eigenschaften nur im
        // Haken, und `window` spiegelt die Globals (siehe `hole`). GEMESSEN
        // vorher: `'document' in self` war falsch — testharness.js hielt
        // jede Seite fuer eine JS-Shell (`ShellTestEnvironment`), schloss
        // nach dem ERSTEN `test()` ab und verwarf alle weiteren (WPT: jede
        // Datei mit mehreren synchronen Tests meldete nur einen).
        // Der Haken antwortet auf JEDES `on…` (Handler-Eigenschaft); die
        // Beruehrungs-Ereignisse hat ein Schreibtisch-Chrome aber nicht —
        // `'ontouchstart' in window` ist die uebliche Erkennung fuer
        // Telefone (sonst bekaemen Seiten ihre Mobil-Fassung).
        let nur_mobil = matches!(
            k,
            "ontouchstart" | "ontouchmove" | "ontouchend" | "ontouchcancel" | "onorientationchange"
                | "ongesturestart" | "ongesturechange" | "ongestureend"
        );
        if !nur_mobil
            && let Some(id) = self.host_id_von_gval(r)
            && let Some(h) = self.host_objekte.clone()
        {
            let art = self.host_art(r);
            if matches!(h.lesen_mit_art(id, art, k), Ok(Some(_))) || self.dom_prototyp_funktion(r, k).is_some() {
                return true;
            }
            if self.ist_global_spiegel(r) {
                return self.in_operator(k, GVal::Ref(self.global_objekt));
            }
        }
        false
    }

    pub(crate) fn instanceof(&mut self, l: GVal, r: GVal) -> Result<bool, Stoerung> {
        if !self.ist_aufrufbar(r) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "rechts von instanceof keine Funktion"),
            ));
        }
        if !matches!(l, GVal::Ref(_)) {
            return Ok(false);
        }
        // `r.prototype` holen.
        let proto = self.hole(r, "prototype")?;
        let pr = match proto {
            GVal::Ref(p) => p,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "prototype kein Objekt"),
                ));
            }
        };
        let mut cur = match l {
            GVal::Ref(lr) => self.heap_proto_von(lr),
            _ => None,
        };
        let mut rest = 10_000;
        while let Some(o) = cur {
            rest -= 1;
            if rest == 0 {
                break;
            }
            if o == pr {
                return Ok(true);
            }
            cur = self.heap_proto_von(o);
        }
        Ok(false)
    }

    // ---------------------------------------------------------------
    // Destrukturierung
    // ---------------------------------------------------------------
}

impl Engine {
    /// Die Element-Id und der Methodenname eines Host-Objekts, wenn der
    /// Wert eine METHODE ist (W6B, J9: `el.classList.add`).
    ///
    /// GEMESSEN, warum das getrennt von `host_id_von_gval` ist: ein
    /// Host-Objekt traegt die Id, aber nur ein Objekt mit `__methode`
    /// darf AUFGERUFEN werden. Ohne diese Trennung waere `el.style.color`
    /// (eine Eigenschaft) genauso aufrufbar wie `el.classList.add`
    /// (eine Methode) — und `el.classList.add` waere eine Eigenschaft mit
    /// dem Wert „add".
    pub(crate) fn host_methode_von_gval(&self, v: GVal) -> Option<(u32, String)> {
        let id = self.host_id_von_gval(v)?;
        let GVal::Ref(r) = v else { return None };
        // J8: ohne Haken gibt es keine Host-Objekte — der HashMap-Zugriff auf
        // `__host` faellt dann bei **jedem** Zugriff weg. GEMESSEN: der
        // Zugriff stand in `hole`, `host_methode_von_gval` (also in JEDEM
        // Aufruf) und `schreibe_host`.
        if self.host_objekte.is_none() {
            return None;
        }
        let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen(r) else {
            return None;
        };
        let e = eigenschaften.get(crate::hostobjekt::METHODE)?;
        //
        // GEMESSEN, warum der Methodenname eine ZAHL ist: `Wert` hat
        // keine Funktions-Variante, also wird der Name als Zahl
        // transportiert (`narsil::dom` vergibt Nummern je Methode). Der
        // Aufrufer macht daraus wieder Text.
        //
        // J6: **TEXT** ist jetzt genauso gueltig. GEMESSEN, warum: ein
        // Host, der eine benannte Methode anbietet, muss sonst eine
        // Tabelle mit Nummern je Methode bauen — und der Aufrufer im
        // Skript sieht `knopf.meldung(…)` scheitern mit „Aufrufziel ist
        // keine Funktion", wenn diese Tabelle fehlt. Der Weg
        // `Wert::host_methode(id, "meldung")` (wert.rs) ist damit ohne
        // Tabelle benutzbar; die Zahl-Form bleibt fuer `narsil::dom`.
        let GVal::Zahl(n) = e.wert else {
            if let GVal::Ref(tr) = e.wert {
                if let HObj::Zeichenkette(t) = self.heap.holen(tr) {
                    return Some((id, t.clone()));
                }
            }
            return None;
        };
        Some((id, crate::wert::zahl_text(n)))
    }
}
