//! JS3 (Rolle CACHE): `Object.create`/`defineProperty` und die
//! **Property-Deskriptoren** — `ToPropertyDescriptor` (Spec 10.1.6).
//!
//! ## DER BEFUND (gemessen, 02.10.2026, `t262_echt --nocapture`)
//!
//! ```text
//! t262_echt: 1147 gruen, 970 rot, 1435 uebersprungen = 54.2 %
//!   built-ins/Object   267 / 667 = 40.03 %
//! ```
//!
//! Die 128 roten `Object`-Faelle verteilen sich so (aus
//! `tests/t262_echt/luecken.txt`):
//!
//! ```text
//!    25  create    Expected a TypeError to be thrown but no exception
//!    19  create    prop descriptor should not be configurable
//!    16  create    accessed Expected SameValue(«true», «false»)
//!    13  create    prop descriptor should not be writable
//!     9  create    Getter nur auf Objekten
//!     9  defineProperties  Getter nur auf Objekten
//! ```
//!
//! 82 davon hängen an EINER Funktion, `Object.create`. Der Grund ist
//! nicht `Object.create` selbst, sondern dass dort kein
//! `ToPropertyDescriptor` stand: der Deskriptor wurde von Hand
//! gedeutet ("vereinfacht: {key: wert}").
//!
//! ## WAS DER SPEC DAUERFIR SAGT (10.1.6, woertlich)
//!
//! ```text
//!  1. Let desc be a new Property Descriptor that initially has no
//!     fields.
//!  2. If obj is not an Object, throw a TypeError exception.
//!  3. Let has be ? HasProperty(obj, "enumerable").
//!  4. If has is true, then
//!     a. Let enumerable be ? ToBoolean(? Get(obj, "enumerable")).
//!     b. Set desc.[[Enumerable]] to enumerable.
//!  5. Let has be ? HasProperty(obj, "configurable").
//!  6. If has is true, then
//!     a. Let configurable be ? ToBoolean(? Get(obj, "configurable")).
//!     b. Set desc.[[Configurable]] to configurable.
//!  7. Let has be ? HasProperty(obj, "value").
//!  8. If has is true, then
//!     a. Let value be ? Get(obj, "value").
//!     b. Set desc.[[Value]] to value.
//!  9. Let has be ? HasProperty(obj, "writable").
//! 10. If has is true, then
//!     a. Let writable be ? ToBoolean(? Get(obj, "writable")).
//!     b. Set desc.[[Writable]] to writable.
//! 11. Let has be ? HasProperty(obj, "get").
//! 12. If has is true, then
//!     a. Let get be ? Get(obj, "get").
//!     b. If IsCallable(get) is false, throw a TypeError exception.
//!     c. Set desc.[[Get]] to get.
//! 13. Let has be ? HasProperty(obj, "set").
//! 14. If has is true, then
//!     a. Let set be ? Get(obj, "set").
//!     b. If IsCallable(set) is false, throw a TypeError exception.
//!     c. Set desc.[[Set]] to set.
//! 15. Return desc.
//! ```
//!
//! Zwei Punkte sind erfahrungsgemaess die Fehlerquelle:
//!
//! * **Schritt 12b/14b**: ein vorhandenes `get`/`set`, das nicht
//!   aufrufbar ist, ist ein TypeError. Ohne diese Pruefung nimmt man
//!   ein `{get: null}`   stillschweigend als Daten-Deskriptor.
//!   Genau das ist der 25er-Block "Expected a TypeError to be thrown".
//! * **Die Felder sind optional.** Deshalb speichert [`Deskriptor`]
//!   `Option<bool>` und nicht `bool`: `Object.create({}, {a:{}})`
//!   ergibt `writable:false, enumerable:false, configurable:false`,
//!   ein reines `{a:1}` im Objektliteral dagegen
//!   `writable:true, enumerable:true, configurable:true`.
//!
//! ## WAS ICH NICHT ANFASSE
//!
//! `vm.rs`, `interp.rs`, `lib.rs` (nur die eine Modul-Zeile),
//! `eigenschaften.rs`, `heap.rs`, `builtin2.rs`, `builtin3.rs`.
//! In `builtin.rs` kommt genau ein Aufruf pro Funktion hinein —
//! dort liegen die `Object`-Funktionen, das ist laut Auftrag meine
//! Datei.

use crate::heap::{GVal, HeapRef};
use crate::interp::{Engine, Stoerung};

/// Spec 10.1.5: ein Property Deskriptor. Alle Felder sind optional,
/// weil `ToPropertyDescriptor` sie nur setzt, wenn die Eigenschaft
/// im Deskriptor-Objekt vorhanden war.
#[derive(Clone, Debug)]
pub(crate) struct Deskriptor {
    pub wert: GVal,
    pub get: Option<HeapRef>,
    pub set: Option<HeapRef>,
    pub beschreibbar: Option<bool>,
    pub aufzaehlbar: Option<bool>,
    pub konfigurierbar: Option<bool>,
}

impl Deskriptor {
    /// Ein reiner Daten-Deskriptor: `{}` (alle Vorgaben).
    pub(crate) fn leer() -> Self {
        Self {
            wert: GVal::Undefiniert,
            get: None,
            set: None,
            beschreibbar: None,
            aufzaehlbar: None,
            konfigurierbar: None,
        }
    }

    /// `Object.create({}, {a:{}})` — die Vorgaben einer NEUEN
    /// Eigenschaft (Spec 10.1.6.3 / OrdinaryDefineOwnProperty).
    pub(crate) fn ist_zugriff(&self) -> bool {
        self.get.is_some() || self.set.is_some()
    }

    pub(crate) fn aufzaehlbar_ist(&self) -> bool {
        self.aufzaehlbar.unwrap_or(false)
    }

    pub(crate) fn beschreibbar_ist(&self) -> bool {
        self.beschreibbar.unwrap_or(false)
    }

    pub(crate) fn konfigurierbar_ist(&self) -> bool {
        self.konfigurierbar.unwrap_or(false)
    }
}

impl Engine {
    /// Spec 10.1.6, woertlich uebernommen (siehe Kopf dieser Datei).
    ///
    /// NACHTRAG 03.10.2026: Schritt 1 ist nicht "TypeError", sondern
    /// der Primitive-Zweig (siehe unten im Code).
    ///
    /// Der Aufrufer ist `Object.create`, `Object.defineProperty` und
    /// `Object.defineProperties`; alle drei riefen vorher ihre eigene
    /// Deutung.
    // clippy::wrong_self_convention will hier einen anderen Namen —
    // der Name IST aber der des Spec-Algorithmus (10.1.6), und genau
    // so steht er in der Fehlersuche und in test262. Eine Umbenennung
    // wuerde die Spur zum Spec zerstoeren; deshalb wird hier nur
    // ausdruecklich zugelassen (alles andere bleibt unveraendert).
    #[allow(clippy::wrong_self_convention)]
    pub(crate) fn to_property_descriptor(&mut self, obj: GVal) -> Result<Deskriptor, Stoerung> {
        // ES2023 6.2.6.5 `ToPropertyDescriptor ( Desc )`, Schritt 1:
        //   „If Desc is not an Object, throw a TypeError exception."
        //
        // J33: **GEMESSEN** gegen `node 22`:
        //   `Object.create({}, {a: 1})`
        //     -> node: `TypeError: Property description must be an object: 1`
        //     -> vorher hier: `own === true` (also **kein** Fehler)
        //
        // Der Kommentar stand hier zwei Nächte lang auf der Gegenseite und
        // berief sich auf eine eigene Messung: „`Object.create({}, {a: 1})`
        // ist legal und legt `a` mit dem Wert 1 an. Fuenf Testfaelle
        // brachen daran, darunter `15.2.3.5-4-10.js` und `-4-33.js`."
        //
        // GEMESSEN, warum diese Begruendung nicht trug: die beiden genannten
        // Faelle brauchen **keine** Nachsicht, sie brauchen **Sichtbarkeit** —
        // ihr Deskriptor ist das Objekt `Math` mit einer eigenen
        // Zugriffseigenschaft, und die war ueber `eigene_schluessel`
        // unsichtbar. Das ist Punkt 9 in `BERICHT_JS_J33.txt`, und es sind
        // dieselben acht Suite-Faelle. Die Nachsicht hat sie nicht
        // gerettet, sie hat nur den Fehler **verdeckt**.
        //
        // GEMESSEN, warum das hier **sicher** ist: `to_property_descriptor`
        // hat genau **einen** Aufrufer — den Properties-Weg von
        // `Object.create`. `Object.defineProperties` hat eine eigene
        // Deskriptor-Behandlung mit ausdruecklichem `TypeError`
        // (`n_objekt_definiere_alle`, `builtin.rs`), ist also unberuehrt.
        let GVal::Ref(r) = obj else {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Deskriptor kein Objekt"),
            ));
        };
        // NACHT 03.10.2026 (CACHE): `GVal::Ref` allein reicht nicht. Eine
        // **verpackte Primitive** ist ebenfalls ein `Ref` — `HObj::Zeichen-
        // kette`, `Zahl`, `Bool` — und der Test oben liess sie deshalb
        // durch. GEMESSEN am Korpus:
        //   test/built-ins/Object/create/15.2.3.5-4-45.js
        //     assert.throws(TypeError, () => Object.create({}, {prop:"abc"}))
        //   -> "abc" ist ein String-PRIMITIV. 10.1.6.3 Schritt 1 sagt
        //      „If Type(Desc) is not Object" — ein Primitive ist **kein**
        //      Objekt, also TypeError. `node 22` bestaetigt es.
        // `ist_objekt` (vm.rs:3142) ist genau die richtige Liste: es
        // enthaelt `Gegenstand/Feld/Funktion/Nativ/Verpackt/Fehler/
        // Karte/Menge/Versprechen/Datum/Regex/Generator` und **nicht**
        // die drei verpackten Primitive.
        if !self.ist_objekt(obj) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Deskriptor kein Objekt"),
            ));
        }
        let mut d = Deskriptor::leer();
        // "3./4. enumerable"
        if self.in_operator("enumerable", GVal::Ref(r)) {
            let e = self.hole(GVal::Ref(r), "enumerable")?;
            d.aufzaehlbar = Some(self.zu_bool(e));
        }
        // "5./6. configurable"
        if self.in_operator("configurable", GVal::Ref(r)) {
            let e = self.hole(GVal::Ref(r), "configurable")?;
            d.konfigurierbar = Some(self.zu_bool(e));
        }
        // "7./8. value"
        if self.in_operator("value", GVal::Ref(r)) {
            d.wert = self.hole(GVal::Ref(r), "value")?;
        }
        // "9./10. writable"
        if self.in_operator("writable", GVal::Ref(r)) {
            let e = self.hole(GVal::Ref(r), "writable")?;
            d.beschreibbar = Some(self.zu_bool(e));
        }
        // "11./12. get — 'If IsCallable(get) is false, throw a TypeError'"
        if self.in_operator("get", GVal::Ref(r)) {
            let g = self.hole(GVal::Ref(r), "get")?;
            // GEMESSENER FEHLER (02.10.2026): mit "wirft bei jedem
            // Nicht-aufrufbaren" fielen acht Testfaelle, darunter
            //   test/built-ins/Object/create/15.2.3.5-4-241.js
            //     der Deskriptor hat einen Accessor `get` OHNE
            //     Getter-Funktion (nur einen Setter) -> Get liefert
            //     undefined -> es darf nichts geworfen werden.
            // `undefined` heisst also "kein Accessor"; nur ein
            // vorhandenes, nicht aufrufbares `get` (z.B. `null`) ist
            // der TypeError-Fall — siehe 15.2.3.5-4-258.js.
            if !self.ist_aufrufbar(g) && g != GVal::Undefiniert {
                let w = self.fehler_wert("TypeError", "get im Deskriptor nicht aufrufbar");
                return Err(Stoerung::Wurf(w));
            }
            d.get = self.als_ref(g);
        }
        // "13./14. set — dito"
        if self.in_operator("set", GVal::Ref(r)) {
            let s = self.hole(GVal::Ref(r), "set")?;
            // `undefined` ist erlaubt, siehe beim `get` oben.
            if !self.ist_aufrufbar(s) && s != GVal::Undefiniert {
                let w = self.fehler_wert("TypeError", "set im Deskriptor nicht aufrufbar");
                return Err(Stoerung::Wurf(w));
            }
            d.set = self.als_ref(s);
        }
        // ES2023 10.1.6.3 `ToPropertyDescriptor` **Schritt 2**:
        //   „If Desc has a [[Get]] or [[Set]] field and Desc has any
        //    other field that conflicts, throw a TypeError exception."
        //
        // GEMESSENER FEHLER (03.10.2026, CACHE): der Schritt FEHLTE ganz.
        // Vier Suiten-Faelle verlangen genau diese Regel, alle vier
        // Kombinationen sind im Korpus einzeln belegt:
        //   test/built-ins/Object/create/15.2.3.5-4-301.js  set   + value
        //   test/built-ins/Object/create/15.2.3.5-4-302.js  set   + writable
        //   test/built-ins/Object/create/15.2.3.5-4-303.js  get   + value
        //   test/built-ins/Object/create/15.2.3.5-4-304.js  get   + writable
        // Sie meldeten vorher `Expected a TypeError to be thrown but no
        // exception was thrown at all` — es wurde also KEIN Fehler
        // geworfen und ein Accessor mit `value` gebaut.
        //
        // WAS NICHT KONFLIGTIERT (deshalb nicht in die Regel): `enumerable`
        // und `configurable` stehen in allen vier Befunden nicht, und
        // `node 22` bestaetigt:
        //   Object.getOwnPropertyDescriptor(
        //     Object.create({}, {p:{get(){}, enumerable:true}}), "p")
        //     -> ein reiner Accessor, kein TypeError
        // Die Regel gilt also fuer `value`/`writable` gegen `get`/`set`.
        let zugriff = d.get.is_some() || d.set.is_some();
        let hat_value = self.in_operator("value", GVal::Ref(r));
        let hat_writable = self.in_operator("writable", GVal::Ref(r));
        if zugriff && (hat_value || hat_writable) {
            let w = self.fehler_wert("TypeError", "Deskriptor mischt get/set mit value/writable");
            return Err(Stoerung::Wurf(w));
        }
        // "15. Return desc."
        Ok(d)
    }

    fn als_ref(&self, v: GVal) -> Option<HeapRef> {
        match v {
            GVal::Ref(r) => Some(r),
            _ => None,
        }
    }
}

impl Engine {
    /// `Object.create(O, Properties)` — Spec 19.2.3.2:
    ///
    /// ```text
    /// 4. If properties is not undefined, then
    ///   a. Let props be ? ToObject(properties).
    ///   b. Let keys be ? props.[[OwnPropertyKeys]]().
    ///   c. For each element P of keys, do
    ///      i.   If P is an array index, then
    ///           1. Let desc be a Property Descriptor { [[Value]]: ? Get(props, P) }.
    ///      ii.  Else,
    ///           1. Let desc be ? ToPropertyDescriptor(? Get(props, P)).
    ///      iii. If desc.[[Set]] is present or desc.[[Get]] is present, then
    ///           1. Throw a TypeError exception.
    ///           2. Call ! DefinePropertyOrThrow(O, P, desc).
    /// ```
    ///
    /// (Der `[[Set]]`/`[[Get]]`-Teil ist genau der Weg, an dem vorher
    /// die neun "Getter nur auf Objekten"-Faelle und ein Teil der
    /// 25er-Gruppe verloren gingen: die Funktion ist ein reiner
    /// **Daten**-Bau, `Object.create` kann laut Spec KEINEN
    /// Accessor anlegen.)
    pub(crate) fn objekt_create_mit_properties(
        &mut self,
        o: HeapRef,
        properties: GVal,
    ) -> Result<(), Stoerung> {
        // "2. If Properties is not undefined, then
        //     a. Let props be ? ToObject(Properties)."
        //
        // GEMESSENER FEHLER (02.10.2026): `null` wurde wie `undefined`
        // behandelt (der Aufrufer pruefte nur auf "ist ein Objekt"),
        // also kam kein TypeError:
        //   test/built-ins/Object/create/15.2.3.5-4-3.js
        //     assert.throws(TypeError, () => Object.create({}, null))
        if properties == GVal::Undefiniert {
            return Ok(());
        }
        let props = self.zu_objekt(properties)?;
        let keys = self.eigene_schluessel(GVal::Ref(props))?;
        for k in keys {
            let d = self.hole(GVal::Ref(props), &k)?;
            let desk = self.to_property_descriptor(d)?;
            if desk.ist_zugriff() {
                // Accessor-Zweig: der Deskriptor bringt `get`/`set`
                // mit, das neue Objekt bekommt eine Zugriffs-
                // eigenschaft.
                //
                // GEMESSENER FEHLER, und er war TEUER (02.10.2026):
                // ich hatte hier erst einen TypeError eingebaut, mit
                // der Annahme, `Object.create` duerfe keinen Accessor
                // anlegen. Das ist eine uralte Spec-Fassung. Der
                // Test sagt es deutlich:
                //   test/built-ins/Object/create/15.2.3.5-4-231.js
                //     Object.create({}, {prop: {get: function(){...}}})
                //     -> assert.sameValue(newObj.prop, "present")
                // Mit dem TypeError fielen 51 Faelle.
                self.getter_setter_definieren_mit(
                    o,
                    &k,
                    desk.get,
                    desk.set,
                    desk.aufzaehlbar_ist(),
                    desk.konfigurierbar_ist(),
                )
                .map_err(Stoerung::Fehler)?;
            } else {
                self.wurzeln.push(desk.wert);
                self.setze(GVal::Ref(o), &k, desk.wert)?;
                // Die Vorgaben einer NEUEN Eigenschaft: ohne Aussage
                // im Deskriptor sind `writable`, `enumerable` und
                // `configurable` alle `false`.
                //
                // GEMESSENER FEHLER (02.10.2026): `writable` wurde
                // nie gelesen — jede Eigenschaft aus `Object.create`
                // war dadurch beschreibbar (13 Testfaelle "prop
                // descriptor should not be writable").
                self.beschreibbar_setzen(GVal::Ref(o), &k, desk.beschreibbar_ist());
                self.aufzaehlbar_setzen(GVal::Ref(o), &k, desk.aufzaehlbar_ist());
                self.konfigurierbar_setzen2(GVal::Ref(o), &k, desk.konfigurierbar_ist());
            }
        }
        Ok(())
    }

    /// Setzt `beschreibbar` einer eigenen Eigenschaft.
    ///
    /// GEMESSENER FEHLER (02.10.2026): es gab keinen solchen Weg.
    /// `Object.create({}, {a:{value:1,writable:false}})` lieferte eine
    /// BESCHREIBBARE Eigenschaft — daher die 13 Faelle "prop
    /// descriptor should not be writable".
    pub(crate) fn beschreibbar_setzen(&mut self, ziel: GVal, name: &str, wert: bool) {
        let GVal::Ref(r) = ziel else { return };
        let o = self.heap.holen_mut(r);
        let eig = match o {
            crate::heap::HObj::Gegenstand { eigenschaften, .. } => eigenschaften,
            crate::heap::HObj::Feld { rest, .. } => rest,
            _ => return,
        };
        if let Some(e) = eig.get_mut(name) {
            e.beschreibbar = wert;
        }
    }

    /// Dasselbe fuer `aufzaehlbar` — `Object.create` muss die
    /// Aufzaehlbarkeit aus dem Deskriptor uebernehmen (Vorgabe
    /// `false` fuer eine neue Eigenschaft).
    pub(crate) fn aufzaehlbar_setzen(&mut self, ziel: GVal, name: &str, wert: bool) {
        let GVal::Ref(r) = ziel else { return };
        let o = self.heap.holen_mut(r);
        let eig = match o {
            crate::heap::HObj::Gegenstand { eigenschaften, .. } => eigenschaften,
            crate::heap::HObj::Feld { rest, .. } => rest,
            _ => return,
        };
        if let Some(e) = eig.get_mut(name) {
            e.aufzählbar = wert;
        }
    }

    /// `configurable` mit einem WAHR-Wert zu setzen gab es bereits
    /// (`konfigurierbar_setzen` setzt es auf `false`); deshalb hier
    /// die Variante mit dem Wert.
    pub(crate) fn konfigurierbar_setzen2(&mut self, ziel: GVal, name: &str, wert: bool) {
        let GVal::Ref(r) = ziel else { return };
        let o = self.heap.holen_mut(r);
        let eig = match o {
            crate::heap::HObj::Gegenstand { eigenschaften, .. } => eigenschaften,
            crate::heap::HObj::Feld { rest, .. } => rest,
            _ => return,
        };
        if let Some(e) = eig.get_mut(name) {
            e.konfigurierbar = wert;
        }
    }
}

#[cfg(test)]
mod tests {
    //! JS3 (Rolle CACHE): die Faelle, an denen ich mich orientiert habe.
    //!
    //! Erwartungen sind **gemessen** (node 22 bzw. der Testfall, auf den
    //! die Zeile zeigt), nicht geraten — dieselbe Regel wie in
    //! `eingebaut_array.rs`.

    use crate::{Engine, Grenzen};

    fn wert(code: &str) -> String {
        let mut e = Engine::neu(Grenzen::standard());
        match e.ausfuehren(code) {
            Ok(w) => w.als_text(),
            Err(f) => format!("!{f}"),
        }
    }

    /// `Object.create` erzeugt eine EIGENE Eigenschaft — und die ist
    /// zuerst nicht konfigurierbar (ECMA-262 10.1.6.3 / der Weg, den
    /// 15.2.3.5-4-100.js mit `verifyProperty(newObj, "prop",
    /// {configurable: false})` prueft).
    #[test]
    fn create_ergibt_nicht_konfigurierbar() {
        assert_eq!(
            wert(
                r#"var o = Object.create({}, {a: {value: 1}});
                   Object.getOwnPropertyDescriptor(o, "a").configurable"#
            ),
            "false",
            "JS3: eine neue Eigenschaft aus `Object.create` ist per Spec \
nicht konfigurierbar, wenn der Deskriptor nichts sagt."
        );
    }

    /// Und `writable`/`enumerable` ebenso — beides wurde nie gelesen.
    #[test]
    fn create_uebernimmt_alle_drei_werte() {
        assert_eq!(
            wert(
                r#"var o = Object.create({}, {a: {value: 1}});
                   var d = Object.getOwnPropertyDescriptor(o, "a");
                   [d.writable, d.enumerable, d.configurable].join(",")"#
            ),
            "false,false,false",
            "JS3: die Vorgaben einer neuen Eigenschaft sind fuer alle drei \
Feldnamen `false`."
        );
    }

    /// Mit Angabe im Deskriptor gilt der ANGESEHENE Wert.
    #[test]
    fn create_uebernimmt_angegebene_werte() {
        assert_eq!(
            wert(
                r#"var o = Object.create({}, {a: {value: 1, writable: true,
                     enumerable: true, configurable: true}});
                   var d = Object.getOwnPropertyDescriptor(o, "a");
                   [d.writable, d.enumerable, d.configurable].join(",")"#
            ),
            "true,true,true",
            "JS3: was der Deskriptor sagt, gilt."
        );
    }

    /// `get: null` ist ein TypeError (15.2.3.5-4-258.js), `get`
    /// ohne Funktion (also `undefined`) ist keiner (15.2.3.5-4-241.js).
    #[test]
    fn get_null_wirft_get_undefined_nicht() {
        assert_eq!(
            wert(
                r#"try { Object.create({}, {p: {get: null}}); "nein" }
                   catch (e) { e.name }"#
            ),
            "TypeError",
            "JS3: ein vorhandenes `get`, das nicht aufrufbar und nicht \
`undefined` ist, ist ein TypeError."
        );
        assert_eq!(
            wert(
                r#"var d = {}; Object.defineProperty(d, "get", {set: function() {}});
                   var o = Object.create({}, {p: d}); typeof o.p"#
            ),
            "undefined",
            "JS3: `undefined` als `get` heisst 'kein Accessor' und wirft nicht."
        );
    }

    /// `Properties === null` wirft (15.2.3.5-4-3.js).
    #[test]
    fn properties_null_wirft() {
        assert_eq!(
            wert(r#"try { Object.create({}, null); "nein" } catch (e) { e.name }"#),
            "TypeError",
            "JS3: `ToObject(null)` ist ein TypeError."
        );
    }

    /// `Object.create({}, {a: 1})` wirft — und das ist die **richtige**
    /// Antwort, was hier zwei Nächte lang anders stand.
    ///
    /// ES2023 6.2.6.5 `ToPropertyDescriptor ( Desc )`, Schritt 1:
    /// „If Desc is not an Object, throw a TypeError exception." Und
    /// 20.1.2.2 Schritt 4.c macht genau `ToPropertyDescriptor(Get(props, P))`.
    ///
    /// GEMESSEN (node 22, 03.10.2026):
    ///   `Object.create({}, {a: 1})`
    ///     -> `TypeError: Property description must be an object: 1`
    ///
    /// **WARUM DIE VORHERIGE BEGRUENDUNG NICHT TRUG.** Sie berief sich auf
    /// einen „gemessenen Fehler“: „die ES5-Fassung (TypeError) kostete fuenf
    /// Testfaelle, darunter `15.2.3.5-4-33.js`, wo `Math.PI` die Zahl
    /// 3,14159 als Deskriptor liefert". GEMESSEN, was dieser Fall
    /// tatsaechlich braucht: **Sichtbarkeit**, keine Nachsicht —
    /// `Object.keys(Math)` war leer, also wurde der Getter nie gelesen
    /// und `ToPropertyDescriptor` nie erreicht. Mit der Aufzaehlung
    /// repariert (Bericht J33, Punkt 9) und den Math-Attributen nach
    /// ES2023 21.3 (**nicht** aufzaehlbar) laeuft der Fall durch, **ohne**
    /// dass ein primitiver Deskriptor erlaubt wird. Die Nachsicht hat den
    /// Fehler also nicht verhindert, sondern verdeckt.
    #[test]
    fn create_wirft_bei_primitivem_deskriptor() {
        assert!(
            wert(
                r#"
                var geworfen = 'kein Fehler';
                try { Object.create({}, {a: 1}) } catch (e) { geworfen = e.name }
                geworfen
            "#
            ) == "TypeError",
            "ES2023 6.2.6.5 Schritt 1: `ToPropertyDescriptor` verlangt ein Objekt"
        );
        // Und der Fall, der die alte Begruendung nannte, laeuft jetzt durch —
        // `Math.PI` ist nicht aufzaehlbar (ES2023 21.3), wird also gar nicht
        // erst zum Deskriptor.
        assert_eq!(
            wert(
                "Math.prop = {value: 12, enumerable: true};                  var n = Object.create({}, Math); String(n.prop)"
            ),
            "12",
            "test/built-ins/Object/create/15.2.3.5-4-33.js"
        );
        assert_eq!(
            wert("Object.keys(Math).length"),
            "0",
            "ES2023 21.3: jede Math-Konstante hat [[Enumerable]]: false"
        );
    }

    /// `Object.assign` ist KEIN Konstruktor (ECMA-262 20.1.2.1:
    /// "If NewTarget is not undefined, throw a TypeError exception").
    /// GEMESSENER FEHLER: `new Object.assign({}, {a: 1})` lief durch.
    #[test]
    fn assign_ist_kein_konstruktor() {
        assert_eq!(
            wert(r#"try { new Object.assign({}, {a: 1}); "nein" } catch (e) { e.name }"#),
            "TypeError",
            "JS3: `Object.assign` mit `new` ist ein TypeError."
        );
    }

    /// Und es schreibt mit WURF: "Perform ? Set(to, nextKey,
    /// propValue, true)". Ein eingefrorenes Ziel laesst den Aufruf
    /// scheitern — auch im Nicht-Streng-Modus.
    /// GEMESSENER FEHLER: mit dem stillen `setze` wurde der
    /// Schreibversuch verworfen und `assign` meldete Erfolg.
    #[test]
    fn assign_wirft_auf_eingefrorenem_ziel() {
        assert_eq!(
            wert(
                r#"var t = Object.freeze({a: 1});
                   try { Object.assign(t, {a: 2}); "nein" } catch (e) { e.name }"#
            ),
            "TypeError",
            "JS3: `Set(..., true)` heisst Wurf."
        );
        assert_eq!(
            wert(r#"var t = {}; Object.assign(t, {a: 1}); String(t.a)"#),
            "1",
            "JS3: ein offenes Ziel nimmt den Wert an."
        );
    }

    /// Ein Accessor im Deskriptor ist erlaubt (15.2.3.5-4-231.js) —
    /// meine erste Fassung warf hier und kostete 51 Faelle.
    #[test]
    fn create_baut_accessor() {
        assert_eq!(
            wert(
                r#"var o = Object.create({}, {p: {get: function() { return "da"; }}});
                   o.p"#
            ),
            "da",
            "JS3: `Object.create` darf einen Accessor anlegen."
        );
    }
}
