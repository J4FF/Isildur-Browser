//! Release 10.10.2026: `Proxy` (ES2023 10.5 „Proxy Object Internal Methods",
//! 28.2 „Proxy Objects").
//!
//! GEMESSEN vorher (github.com, landing-pages-Modul): „ReferenceError: 'Proxy'
//! ist nicht definiert" — der letzte Fehler der Startseite. Proxys tragen
//! dort (und in Vue, MobX, immer, framer-motion …) Zustandsspeicher und
//! Komponenten-Fabriken.
//!
//! Aufbau: ein Proxy ist ein eigenes Heap-Objekt (`HObj::Proxy`). Die
//! internen Methoden (`[[Get]]`, `[[Set]]`, `[[HasProperty]]`, `[[Delete]]`,
//! `[[OwnPropertyKeys]]`, `[[GetOwnProperty]]`, `[[DefineOwnProperty]]`,
//! `[[GetPrototypeOf]]`, `[[SetPrototypeOf]]`, `[[IsExtensible]]`,
//! `[[PreventExtensions]]`, `[[Call]]`, `[[Construct]]`) stehen hier; die
//! Eigenschaftswege des Motors (`hole`, `setze`, `loesche`, `in_operator`,
//! `eigene_schluessel`, `schluessel_werte`, Aufruf, `new`) und die
//! `Object.*`/`Reflect.*`-Verfahren (`proxy_nativ`) leiten hierher um.
//! Ohne Falle geht jede Operation an das Ziel.
//!
//! BEWUSST NICHT (noch): die Invarianten-Pruefungen der Norm (z.B. „get"
//! darf fuer eine nicht konfigurierbare, nicht beschreibbare Eigenschaft
//! keinen anderen Wert melden). Sie schuetzen nur gegen widerspruechliche
//! Handler; echte Seiten verletzen sie nicht.

use crate::builtin::Nativ;
use crate::heap::{GVal, HObj, HeapRef};
use crate::interp::{Engine, Stoerung};

/// Obergrenze fuer verschachtelte Proxy-Weiterleitungen (Proxy auf Proxy …)
/// — schuetzt den Rust-Stapel (M3).
const MAX_PROXY_TIEFE: u32 = 256;

impl Engine {
    /// `true`, wenn `v` ein Proxy ist (auch ein widerrufener).
    pub(crate) fn ist_proxy(&self, v: GVal) -> bool {
        matches!(v, GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Proxy { .. }))
    }

    /// Ziel und Handler — TypeError, wenn der Proxy widerrufen ist
    /// (ES2023 10.5.x Schritt 1–3: „If handler is null, throw a TypeError").
    fn proxy_teile(&mut self, p: HeapRef, op: &str) -> Result<(HeapRef, HeapRef), Stoerung> {
        match self.heap.holen(p) {
            HObj::Proxy { ziel: Some(z), handler: Some(h), .. } => Ok((*z, *h)),
            _ => Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                &format!("Cannot perform '{op}' on a proxy that has been revoked"),
            ))),
        }
    }

    /// Die Falle `name` des Handlers: `None` ohne Falle (undefined/null),
    /// TypeError, wenn sie keine Funktion ist (GetMethod, ES2023 7.3.11).
    fn proxy_falle(&mut self, p: HeapRef, name: &str) -> Result<(HeapRef, HeapRef, Option<GVal>), Stoerung> {
        let (z, h) = self.proxy_teile(p, name)?;
        let f = self.hole(GVal::Ref(h), name)?;
        if matches!(f, GVal::Undefiniert | GVal::Null) {
            return Ok((z, h, None));
        }
        if !self.ist_aufrufbar(f) {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                &format!("'{name}' on proxy: trap is not a function"),
            )));
        }
        Ok((z, h, Some(f)))
    }

    /// Ein interner Schluessel als JS-Wert (Symbol-Schluessel -> das Symbol).
    fn proxy_schluessel_wert(&mut self, k: &str) -> Result<GVal, Stoerung> {
        if crate::heap::ist_symbolschluessel(k)
            && let Some(s) = self.symbol_nach_schluessel.get(k).copied()
        {
            return Ok(GVal::Ref(s));
        }
        self.text_wert(k).map_err(Stoerung::Fehler)
    }

    /// Eine Falle rufen — alle Werte bleiben waehrend des Rufs Wurzeln.
    fn proxy_rufen(&mut self, falle: GVal, handler: HeapRef, args: &[GVal]) -> Result<GVal, Stoerung> {
        if self.proxy_tiefe >= MAX_PROXY_TIEFE {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "Maximum call stack size exceeded")));
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(falle);
        self.wurzeln.push(GVal::Ref(handler));
        self.wurzeln.extend_from_slice(args);
        self.proxy_tiefe += 1;
        let r = self.aufruf_ohne_stapelspur(falle, GVal::Ref(handler), args);
        self.proxy_tiefe -= 1;
        self.wurzel_zurueck(marke);
        r
    }

    /// Weiterleitung an das Ziel mit Tiefenzaehler (Proxy auf Proxy).
    fn proxy_weiter<T>(&mut self, f: impl FnOnce(&mut Engine) -> Result<T, Stoerung>) -> Result<T, Stoerung> {
        if self.proxy_tiefe >= MAX_PROXY_TIEFE {
            return Err(Stoerung::Wurf(self.fehler_wert("RangeError", "Maximum call stack size exceeded")));
        }
        self.proxy_tiefe += 1;
        let r = f(self);
        self.proxy_tiefe -= 1;
        r
    }

    /// `[[Get]]` (10.5.8).
    pub(crate) fn proxy_get(&mut self, p: HeapRef, k: &str, empfaenger: GVal) -> Result<GVal, Stoerung> {
        let (z, h, falle) = self.proxy_falle(p, "get")?;
        match falle {
            Some(f) => {
                let kw = self.proxy_schluessel_wert(k)?;
                self.proxy_rufen(f, h, &[GVal::Ref(z), kw, empfaenger])
            }
            None => {
                // Empfaenger = der Proxy selbst: das Ziel liest wie immer
                // (auch `length` eines Felds, das kein gespeicherter Eintrag
                // ist). Ein Getter sieht dabei das Ziel als `this`.
                if empfaenger == GVal::Ref(p) {
                    return self.proxy_weiter(|e| e.hole(GVal::Ref(z), k));
                }
                let kw = self.proxy_schluessel_wert(k)?;
                self.proxy_weiter(|e| e.n_refl_hole(&[GVal::Ref(z), kw, empfaenger]))
            }
        }
    }

    /// `[[Set]]` (10.5.9) — `false` heisst „abgelehnt" (strikt: TypeError
    /// beim Aufrufer).
    pub(crate) fn proxy_set(&mut self, p: HeapRef, k: &str, wert: GVal, empfaenger: GVal) -> Result<bool, Stoerung> {
        let (z, h, falle) = self.proxy_falle(p, "set")?;
        match falle {
            Some(f) => {
                let kw = self.proxy_schluessel_wert(k)?;
                let r = self.proxy_rufen(f, h, &[GVal::Ref(z), kw, wert, empfaenger])?;
                Ok(self.zu_bool(r))
            }
            None => {
                // Empfaenger = der Proxy selbst: auf das Ziel schreiben (ein
                // `Reflect.set` mit dem Proxy als Empfaenger schriebe wieder
                // auf den Proxy — Endlosschleife).
                if empfaenger == GVal::Ref(p) {
                    self.proxy_weiter(|e| e.setze(GVal::Ref(z), k, wert))?;
                    return Ok(true);
                }
                let kw = self.proxy_schluessel_wert(k)?;
                let r = self.proxy_weiter(|e| e.n_refl_setze(&[GVal::Ref(z), kw, wert, empfaenger]))?;
                Ok(self.zu_bool(r))
            }
        }
    }

    /// `[[HasProperty]]` (10.5.7).
    pub(crate) fn proxy_has(&mut self, p: HeapRef, k: &str) -> Result<bool, Stoerung> {
        let (z, h, falle) = self.proxy_falle(p, "has")?;
        match falle {
            Some(f) => {
                let kw = self.proxy_schluessel_wert(k)?;
                let r = self.proxy_rufen(f, h, &[GVal::Ref(z), kw])?;
                Ok(self.zu_bool(r))
            }
            None => self.proxy_weiter(|e| Ok(e.in_operator(k, GVal::Ref(z)))),
        }
    }

    /// `[[Delete]]` (10.5.10).
    pub(crate) fn proxy_delete(&mut self, p: HeapRef, k: &str) -> Result<bool, Stoerung> {
        let (z, h, falle) = self.proxy_falle(p, "deleteProperty")?;
        match falle {
            Some(f) => {
                let kw = self.proxy_schluessel_wert(k)?;
                let r = self.proxy_rufen(f, h, &[GVal::Ref(z), kw])?;
                Ok(self.zu_bool(r))
            }
            None => self.proxy_weiter(|e| e.loesche(GVal::Ref(z), k)),
        }
    }

    /// `[[OwnPropertyKeys]]` (10.5.11) — interne Schluessel (Symbole als
    /// Symbol-Schluessel), Reihenfolge wie geliefert.
    pub(crate) fn proxy_own_keys(&mut self, p: HeapRef) -> Result<Vec<String>, Stoerung> {
        let (z, h, falle) = self.proxy_falle(p, "ownKeys")?;
        let liste = match falle {
            Some(f) => self.proxy_rufen(f, h, &[GVal::Ref(z)])?,
            None => self.proxy_weiter(|e| e.n_objekt_namen(&[GVal::Ref(z)]))?,
        };
        if !self.ist_objekt(liste) {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                "CreateListFromArrayLike called on non-object",
            )));
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(liste);
        let n = self.hole(liste, "length")?;
        let n = match n {
            GVal::Zahl(x) if x.is_finite() && x > 0.0 => (x as usize).min(crate::ops::MAX_SAMMLUNG),
            _ => 0,
        };
        let mut out: Vec<String> = Vec::with_capacity(n.min(1024));
        for i in 0..n {
            let v = self.hole(liste, &i.to_string())?;
            let ist_symbol = matches!(v, GVal::Ref(r) if crate::symbole::ist_symbol(self, r));
            let ist_text = matches!(v, GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Zeichenkette(_)));
            if !ist_symbol && !ist_text {
                self.wurzel_zurueck(marke);
                return Err(Stoerung::Wurf(self.fehler_wert(
                    "TypeError",
                    "proxy [[OwnPropertyKeys]] must return an array with only string and symbol elements",
                )));
            }
            let k = self.schluessel_text(v)?;
            if out.contains(&k) {
                self.wurzel_zurueck(marke);
                return Err(Stoerung::Wurf(self.fehler_wert(
                    "TypeError",
                    "proxy [[OwnPropertyKeys]] can't report property twice",
                )));
            }
            out.push(k);
        }
        self.wurzel_zurueck(marke);
        Ok(out)
    }

    /// `[[GetOwnProperty]]` (10.5.5) — der Deskriptor als Objekt oder
    /// `undefined`.
    pub(crate) fn proxy_deskriptor(&mut self, p: HeapRef, k: &str) -> Result<GVal, Stoerung> {
        let (z, h, falle) = self.proxy_falle(p, "getOwnPropertyDescriptor")?;
        let kw = self.proxy_schluessel_wert(k)?;
        match falle {
            Some(f) => {
                let d = self.proxy_rufen(f, h, &[GVal::Ref(z), kw])?;
                if !matches!(d, GVal::Undefiniert) && !self.ist_objekt(d) {
                    return Err(Stoerung::Wurf(self.fehler_wert(
                        "TypeError",
                        "'getOwnPropertyDescriptor' on proxy: trap returned neither object nor undefined",
                    )));
                }
                Ok(d)
            }
            None => self.proxy_weiter(|e| e.n_objekt_deskriptor(&[GVal::Ref(z), kw])),
        }
    }

    /// Ist die eigene Eigenschaft `k` aufzaehlbar? (fuer `Object.keys`,
    /// `for-in`, JSON, Streuen).
    fn proxy_aufzaehlbar(&mut self, p: HeapRef, k: &str) -> Result<bool, Stoerung> {
        let d = self.proxy_deskriptor(p, k)?;
        if matches!(d, GVal::Undefiniert) {
            return Ok(false);
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(d);
        let a = self.hole(d, "enumerable");
        self.wurzel_zurueck(marke);
        Ok(self.zu_bool(a?))
    }

    /// Die aufzaehlbaren eigenen TEXT-Schluessel (EnumerableOwnProperties).
    pub(crate) fn proxy_aufzaehlbare_schluessel(&mut self, p: HeapRef) -> Result<Vec<String>, Stoerung> {
        let alle = self.proxy_own_keys(p)?;
        let mut out = Vec::with_capacity(alle.len());
        for k in alle {
            if crate::heap::ist_symbolschluessel(&k) {
                continue;
            }
            if self.proxy_aufzaehlbar(p, &k)? {
                out.push(k);
            }
        }
        Ok(out)
    }

    /// `[[DefineOwnProperty]]` (10.5.6).
    pub(crate) fn proxy_definiere(&mut self, p: HeapRef, k: &str, desk: GVal) -> Result<bool, Stoerung> {
        let (z, h, falle) = self.proxy_falle(p, "defineProperty")?;
        let kw = self.proxy_schluessel_wert(k)?;
        match falle {
            Some(f) => {
                let r = self.proxy_rufen(f, h, &[GVal::Ref(z), kw, desk])?;
                Ok(self.zu_bool(r))
            }
            None => {
                let r = self.proxy_weiter(|e| e.n_refl_definiere(&[GVal::Ref(z), kw, desk]))?;
                Ok(self.zu_bool(r))
            }
        }
    }

    /// `[[GetPrototypeOf]]` (10.5.1).
    pub(crate) fn proxy_proto(&mut self, p: HeapRef) -> Result<GVal, Stoerung> {
        let (z, h, falle) = self.proxy_falle(p, "getPrototypeOf")?;
        match falle {
            Some(f) => {
                let v = self.proxy_rufen(f, h, &[GVal::Ref(z)])?;
                if !matches!(v, GVal::Null) && !self.ist_objekt(v) {
                    return Err(Stoerung::Wurf(self.fehler_wert(
                        "TypeError",
                        "'getPrototypeOf' on proxy: trap returned neither object nor null",
                    )));
                }
                Ok(v)
            }
            None => self.proxy_weiter(|e| e.n_objekt_proto_von(&[GVal::Ref(z)])),
        }
    }

    /// `[[SetPrototypeOf]]` (10.5.2).
    pub(crate) fn proxy_proto_setzen(&mut self, p: HeapRef, v: GVal) -> Result<bool, Stoerung> {
        let (z, h, falle) = self.proxy_falle(p, "setPrototypeOf")?;
        match falle {
            Some(f) => {
                let r = self.proxy_rufen(f, h, &[GVal::Ref(z), v])?;
                Ok(self.zu_bool(r))
            }
            None => {
                let r = self.proxy_weiter(|e| e.n_refl_setze_proto(&[GVal::Ref(z), v]))?;
                Ok(self.zu_bool(r))
            }
        }
    }

    /// `[[IsExtensible]]` (10.5.3).
    pub(crate) fn proxy_erweiterbar(&mut self, p: HeapRef) -> Result<bool, Stoerung> {
        let (z, h, falle) = self.proxy_falle(p, "isExtensible")?;
        match falle {
            Some(f) => {
                let r = self.proxy_rufen(f, h, &[GVal::Ref(z)])?;
                Ok(self.zu_bool(r))
            }
            None => {
                let r = self.proxy_weiter(|e| e.n_objekt_ist_sperre(&[GVal::Ref(z)], 0))?;
                Ok(self.zu_bool(r))
            }
        }
    }

    /// `[[PreventExtensions]]` (10.5.4).
    pub(crate) fn proxy_verhindere(&mut self, p: HeapRef) -> Result<bool, Stoerung> {
        let (z, h, falle) = self.proxy_falle(p, "preventExtensions")?;
        match falle {
            Some(f) => {
                let r = self.proxy_rufen(f, h, &[GVal::Ref(z)])?;
                Ok(self.zu_bool(r))
            }
            None => {
                let r = self.proxy_weiter(|e| e.n_refl_verhindere(&[GVal::Ref(z)]))?;
                Ok(self.zu_bool(r))
            }
        }
    }

    /// `[[Call]]` (10.5.12).
    pub(crate) fn proxy_anwenden(&mut self, p: HeapRef, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let aufrufbar = matches!(self.heap.holen(p), HObj::Proxy { aufrufbar: true, .. });
        if !aufrufbar {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Aufrufziel ist keine Funktion")));
        }
        let (z, h, falle) = self.proxy_falle(p, "apply")?;
        match falle {
            Some(f) => {
                let feld = self.feld_aus_werten(args)?;
                self.proxy_rufen(f, h, &[GVal::Ref(z), this, GVal::Ref(feld)])
            }
            None => self.proxy_weiter(|e| e.aufruf_ohne_stapelspur(GVal::Ref(z), this, args)),
        }
    }

    /// `[[Construct]]` (10.5.13).
    pub(crate) fn proxy_konstruieren(&mut self, p: HeapRef, args: &[GVal], neu_ziel: GVal) -> Result<GVal, Stoerung> {
        let konstruierbar = matches!(self.heap.holen(p), HObj::Proxy { konstruierbar: true, .. });
        if !konstruierbar {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "new-Ziel ist kein Konstruktor")));
        }
        let (z, h, falle) = self.proxy_falle(p, "construct")?;
        let feld = self.feld_aus_werten(args)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(feld));
        let neu_ziel = if neu_ziel == GVal::Ref(p) { neu_ziel } else { neu_ziel };
        let r = match falle {
            Some(f) => {
                let o = self.proxy_rufen(f, h, &[GVal::Ref(z), GVal::Ref(feld), neu_ziel]);
                match o {
                    Ok(o) if !self.ist_objekt(o) => Err(Stoerung::Wurf(self.fehler_wert(
                        "TypeError",
                        "proxy [[Construct]] must return an object",
                    ))),
                    x => x,
                }
            }
            None => {
                // `new.target` bleibt das Ziel, wenn der Proxy selbst gerufen
                // wurde (sonst sucht `Reflect.construct` den Prototyp ueber
                // den Proxy — gleichwertig, aber ein Umweg).
                let nz = if neu_ziel == GVal::Ref(p) { GVal::Ref(z) } else { neu_ziel };
                self.proxy_weiter(|e| e.n_refl_konstruieren(&[GVal::Ref(z), GVal::Ref(feld), nz]))
            }
        };
        self.wurzel_zurueck(marke);
        r
    }

    /// `Array.isArray(proxy)` sieht durch den Proxy (ES2023 7.2.2 Schritt 3).
    pub(crate) fn proxy_ist_feld(&mut self, v: GVal) -> Result<bool, Stoerung> {
        let mut v = v;
        for _ in 0..MAX_PROXY_TIEFE {
            match v {
                GVal::Ref(r) => match self.heap.holen(r) {
                    // Release 15.10.2026: not an arguments object.
                    HObj::Feld { rest, .. } => return Ok(!rest.contains_key(crate::builtin::ARGUMENTE_MARKE)),
                    HObj::Proxy { ziel: Some(z), .. } => v = GVal::Ref(*z),
                    HObj::Proxy { .. } => {
                        return Err(Stoerung::Wurf(self.fehler_wert(
                            "TypeError",
                            "Cannot perform 'IsArray' on a proxy that has been revoked",
                        )));
                    }
                    _ => return Ok(false),
                },
                _ => return Ok(false),
            }
        }
        Ok(false)
    }

    // -----------------------------------------------------------------
    // Die Konstruktoren
    // -----------------------------------------------------------------

    fn proxy_anlegen(&mut self, args: &[GVal]) -> Result<HeapRef, Stoerung> {
        let ziel = self.arg(args, 0);
        let handler = self.arg(args, 1);
        // Ein Symbol ist bei uns ein Gegenstand mit `proto_symbol` — fuer
        // `new Proxy` aber kein Objekt (GEMESSEN node 22: TypeError).
        let ok = |e: &Engine, v: GVal| {
            (e.ist_objekt(v) || e.ist_proxy(v)) && !matches!(v, GVal::Ref(r) if crate::symbole::ist_symbol(e, r))
        };
        if !ok(self, ziel) || !ok(self, handler) {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                "Cannot create proxy with a non-object as target or handler",
            )));
        }
        let (GVal::Ref(z), GVal::Ref(h)) = (ziel, handler) else {
            unreachable!("oben geprueft")
        };
        let aufrufbar = self.ist_aufrufbar(ziel);
        let konstruierbar = aufrufbar && crate::eingebaut_array::ist_konstruktor(self, ziel);
        let obj = HObj::Proxy { ziel: Some(z), handler: Some(h), aufrufbar, konstruierbar };
        let kosten = crate::heap::Heap::kosten_fuer(&obj);
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        Ok(self.belege(obj, kosten))
    }

    /// `new Proxy(target, handler)` (28.2.1.1) — ohne `new` TypeError.
    pub(crate) fn n_proxy_ctor(&mut self, args: &[GVal], als_konstruktor: bool) -> Result<GVal, Stoerung> {
        if !als_konstruktor {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                "Constructor Proxy requires 'new'",
            )));
        }
        Ok(GVal::Ref(self.proxy_anlegen(args)?))
    }

    /// `Proxy.revocable(target, handler)` (28.2.2.1) -> `{ proxy, revoke }`.
    pub(crate) fn n_proxy_widerrufbar(&mut self, args: &[GVal], als_konstruktor: bool) -> Result<GVal, Stoerung> {
        if als_konstruktor {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Proxy.revocable is not a constructor")));
        }
        let p = self.proxy_anlegen(args)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(p));
        let widerruf = self.nativ_anlegen("", Nativ::ProxyWiderrufen, self.proto_funktion);
        if let HObj::Nativ { ziel, .. } = self.heap.holen_mut(widerruf) {
            *ziel = Some(p);
        }
        self.wurzeln.push(GVal::Ref(widerruf));
        let o = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(o));
        self.setze(GVal::Ref(o), "proxy", GVal::Ref(p))?;
        self.setze(GVal::Ref(o), "revoke", GVal::Ref(widerruf))?;
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(o))
    }

    /// Die `revoke`-Funktion: der Proxy verliert Ziel und Handler; ein
    /// zweiter Aufruf tut nichts (28.2.2.1.1 Schritt 2).
    pub(crate) fn n_proxy_widerrufen(&mut self, fn_ref: HeapRef) -> Result<GVal, Stoerung> {
        let p = match self.heap.holen_mut(fn_ref) {
            HObj::Nativ { ziel, .. } => ziel.take(),
            _ => None,
        };
        if let Some(p) = p
            && let HObj::Proxy { ziel, handler, .. } = self.heap.holen_mut(p)
        {
            *ziel = None;
            *handler = None;
        }
        Ok(GVal::Undefiniert)
    }

    // -----------------------------------------------------------------
    // `Object.*` / `Reflect.*` mit einem Proxy als Argument
    // -----------------------------------------------------------------

    /// Vor der eigentlichen Verteilung (`rufe_nativ_innen`): betrifft das
    /// Verfahren einen Proxy, wird es hier beantwortet. `None` = kein
    /// Proxy-Fall, der normale Weg laeuft.
    pub(crate) fn proxy_nativ(
        &mut self,
        nativ: &Nativ,
        fn_ref: HeapRef,
        this: GVal,
        args: &[GVal],
    ) -> Option<Result<GVal, Stoerung>> {
        use Nativ::*;
        if let Some(r) = self.proxy_feld_methode(nativ, fn_ref, this, args) {
            return Some(r);
        }
        let erstes = self.arg(args, 0);
        let p = match nativ {
            ObjektProtoHatEigen | ObjektProtoAufzaehlbar => this,
            _ => erstes,
        };
        let GVal::Ref(pr) = p else { return None };
        if !matches!(self.heap.holen(pr), HObj::Proxy { .. }) {
            return None;
        }
        let r = match nativ {
            ObjektSchluessel => (|| {
                let ks = self.proxy_aufzaehlbare_schluessel(pr)?;
                self.texte_als_feld(&ks)
            })(),
            ObjektWerte | ObjektEintraege => (|| {
                let ks = self.proxy_aufzaehlbare_schluessel(pr)?;
                let mut werte = Vec::with_capacity(ks.len());
                let marke = self.wurzel_merke();
                for k in &ks {
                    let v = self.proxy_get(pr, k, p)?;
                    self.wurzeln.push(v);
                    if matches!(nativ, ObjektEintraege) {
                        let kt = self.text_wert(k).map_err(Stoerung::Fehler)?;
                        self.wurzeln.push(kt);
                        let paar = self.feld_aus_werten(&[kt, v])?;
                        self.wurzeln.push(GVal::Ref(paar));
                        werte.push(GVal::Ref(paar));
                    } else {
                        werte.push(v);
                    }
                }
                let f = self.feld_aus_werten(&werte);
                self.wurzel_zurueck(marke);
                Ok(GVal::Ref(f?))
            })(),
            ObjektNamen | ObjektEigeneSymbole | ReflEigeneSchluessel => (|| {
                let ks = self.proxy_own_keys(pr)?;
                let marke = self.wurzel_merke();
                let mut werte = Vec::with_capacity(ks.len());
                for k in &ks {
                    let sym = crate::heap::ist_symbolschluessel(k);
                    let nehmen = match nativ {
                        ObjektNamen => !sym,
                        ObjektEigeneSymbole => sym,
                        _ => true,
                    };
                    if !nehmen {
                        continue;
                    }
                    let v = self.proxy_schluessel_wert(k)?;
                    self.wurzeln.push(v);
                    werte.push(v);
                }
                let f = self.feld_aus_werten(&werte);
                self.wurzel_zurueck(marke);
                Ok(GVal::Ref(f?))
            })(),
            ObjektDeskriptor | ReflDeskriptor => (|| {
                let k = self.schluessel_text(self.arg(args, 1))?;
                self.proxy_deskriptor(pr, &k)
            })(),
            ObjektHatEigen => (|| {
                let k = self.schluessel_text(self.arg(args, 1))?;
                let d = self.proxy_deskriptor(pr, &k)?;
                Ok(GVal::Bool(!matches!(d, GVal::Undefiniert)))
            })(),
            ObjektProtoHatEigen => (|| {
                let k = self.schluessel_text(erstes)?;
                let d = self.proxy_deskriptor(pr, &k)?;
                Ok(GVal::Bool(!matches!(d, GVal::Undefiniert)))
            })(),
            ObjektProtoAufzaehlbar => (|| {
                let k = self.schluessel_text(erstes)?;
                Ok(GVal::Bool(self.proxy_aufzaehlbar(pr, &k)?))
            })(),
            ObjektDefiniere | ReflDefiniere => (|| {
                let k = self.schluessel_text(self.arg(args, 1))?;
                let ok = self.proxy_definiere(pr, &k, self.arg(args, 2))?;
                if matches!(nativ, ReflDefiniere) {
                    return Ok(GVal::Bool(ok));
                }
                if !ok {
                    return Err(Stoerung::Wurf(self.fehler_wert(
                        "TypeError",
                        &format!("'defineProperty' on proxy: trap returned falsish for property '{k}'"),
                    )));
                }
                Ok(p)
            })(),
            ObjektProtoVon | ReflProto => self.proxy_proto(pr),
            ObjektProtoSetzen | ReflSetzeProto => (|| {
                let ok = self.proxy_proto_setzen(pr, self.arg(args, 1))?;
                if matches!(nativ, ReflSetzeProto) {
                    return Ok(GVal::Bool(ok));
                }
                if !ok {
                    return Err(Stoerung::Wurf(self.fehler_wert(
                        "TypeError",
                        "'setPrototypeOf' on proxy: trap returned falsish",
                    )));
                }
                Ok(p)
            })(),
            ObjektIstErweiterbar | ReflErweiterbar => self.proxy_erweiterbar(pr).map(GVal::Bool),
            ObjektNichtErweiterbar | ReflVerhindere => (|| {
                let ok = self.proxy_verhindere(pr)?;
                if matches!(nativ, ReflVerhindere) {
                    return Ok(GVal::Bool(ok));
                }
                if !ok {
                    return Err(Stoerung::Wurf(self.fehler_wert(
                        "TypeError",
                        "'preventExtensions' on proxy: trap returned falsish",
                    )));
                }
                Ok(p)
            })(),
            ReflHole => (|| {
                let k = self.schluessel_text(self.arg(args, 1))?;
                let empf = if args.len() > 2 { self.arg(args, 2) } else { p };
                self.proxy_get(pr, &k, empf)
            })(),
            ReflSetze => (|| {
                let k = self.schluessel_text(self.arg(args, 1))?;
                let empf = if args.len() > 3 { self.arg(args, 3) } else { p };
                Ok(GVal::Bool(self.proxy_set(pr, &k, self.arg(args, 2), empf)?))
            })(),
            ReflHat => (|| {
                let k = self.schluessel_text(self.arg(args, 1))?;
                Ok(GVal::Bool(self.proxy_has(pr, &k)?))
            })(),
            ReflLoesche => (|| {
                let k = self.schluessel_text(self.arg(args, 1))?;
                Ok(GVal::Bool(self.proxy_delete(pr, &k)?))
            })(),
            FeldIstFeld => self.proxy_ist_feld(p).map(GVal::Bool),
            _ => return None,
        };
        Some(r)
    }

    /// `Array.prototype.*` mit einem PROXY als `this` (Vue/Alpine: reaktive
    /// Felder sind Proxys auf Felder). Die Feld-Verfahren des Motors
    /// arbeiten nur auf echten Feldern — deshalb: ein Abbild ueber
    /// `[[Get]]` (`length`, Indizes), das Verfahren darauf, und bei den
    /// veraendernden Verfahren das Ergebnis ueber `[[Set]]`/`[[Delete]]`
    /// zurueck (die Fallen des Proxys sehen jede Aenderung, wie in der Norm,
    /// die diese Verfahren generisch beschreibt: ES2023 23.1.3).
    ///
    /// GEMESSEN vorher (spiegel.de, Alpine.js): `items.push(x)` auf einem
    /// reaktiven Feld warf „push schreibt nur in ein echtes Feld" — Alpine
    /// brach in `init` ab, und Dialoge/Seitenleisten blieben offen.
    fn proxy_feld_methode(
        &mut self,
        nativ: &Nativ,
        fn_ref: HeapRef,
        this: GVal,
        args: &[GVal],
    ) -> Option<Result<GVal, Stoerung>> {
        use Nativ::*;
        let schreibt = match nativ {
            FeldSchieben | FeldNehmen | FeldVorne | FeldHinten | FeldSpleissen | FeldSortieren
            | FeldUmdrehen | FeldFuellen | FeldKopierenInnerhalb => true,
            FeldAbbilden | FeldFiltern | FeldFalten | FeldFaltenRechts | FeldFuerJedes | FeldFinden
            | FeldFindeIndex | FeldIndexVon | FeldLetzterIndex | FeldEnthaelt | FeldScheibe
            | FeldVerbinden | FeldVerketten | FeldAlle | FeldEinige | FeldFlach | FeldFlachAbbilden
            | FeldSchluesselIter | FeldWerteIter | FeldEintraegeIter | FeldAn | FeldProtoText
            | FeldFindeIndexLetzten | FeldKopiertUmdreht | FeldKopiertSortiert | FeldMit
            | FeldKopiertGeteilt => false,
            _ => return None,
        };
        let GVal::Ref(p) = this else { return None };
        if !matches!(self.heap.holen(p), HObj::Proxy { .. }) {
            return None;
        }
        let marke = self.wurzel_merke();
        let r = (|| {
            let n = match self.proxy_get(p, "length", this)? {
                GVal::Zahl(x) if x.is_finite() && x > 0.0 => (x as usize).min(crate::ops::MAX_SAMMLUNG),
                _ => 0,
            };
            let mut alt = Vec::with_capacity(n.min(1024));
            for i in 0..n {
                let v = self.proxy_get(p, &i.to_string(), this)?;
                self.wurzeln.push(v);
                alt.push(v);
            }
            let abbild = self.feld_aus_werten(&alt)?;
            self.wurzeln.push(GVal::Ref(abbild));
            let ergebnis = self.rufe_nativ_innen(nativ.clone(), fn_ref, GVal::Ref(abbild), args, false)?;
            self.wurzeln.push(ergebnis);
            if schreibt {
                let neu: Vec<GVal> = match self.heap.holen(abbild) {
                    HObj::Feld { elemente, .. } => elemente.clone(),
                    _ => Vec::new(),
                };
                for (i, v) in neu.iter().enumerate() {
                    if i >= alt.len() || !self.gleich_strikt(alt[i], *v) {
                        self.proxy_set(p, &i.to_string(), *v, this)?;
                    }
                }
                for i in (neu.len()..alt.len()).rev() {
                    self.proxy_delete(p, &i.to_string())?;
                }
                if neu.len() != alt.len() {
                    self.proxy_set(p, "length", GVal::Zahl(neu.len() as f64), this)?;
                }
            }
            // `sort`/`reverse`/`fill`/`copyWithin` liefern `this` — das ist
            // der Proxy, nicht das Abbild.
            Ok(if ergebnis == GVal::Ref(abbild) { this } else { ergebnis })
        })();
        self.wurzel_zurueck(marke);
        Some(r)
    }

    /// Ein Feld aus Texten (`Object.keys`).
    fn texte_als_feld(&mut self, ks: &[String]) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        let mut werte = Vec::with_capacity(ks.len());
        for k in ks {
            let t = self.text_wert(k).map_err(Stoerung::Fehler)?;
            self.wurzeln.push(t);
            werte.push(t);
        }
        let f = self.feld_aus_werten(&werte);
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(f?))
    }
}
