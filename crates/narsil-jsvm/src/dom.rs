//! J1: DOM-API im Motor — ohne Zugriff auf fremde Crates.
//!
//! # WARUM DER BAUM EIN JS-OBJEKTGRAPH IST
//!
//! Die Knoten sind ganz normale Objekte im Heap (`__typ`, `__name`, `__kinder`,
//! `__eltern` als **nicht aufzaehlbare** eigene Felder). Das hat drei
//! Folgen, alle messbar:
//! * **Zyklen sind kein Sonderfall.** `a.__kindern.push(b); b.__kindern.push(a)`
//!   ist fuer den Motor ein ganz normaler Heap; der GC raeumt es mit
//!   (J4). Ein Rust-Baum mit `Vec<Knoten>` haette genau hier eine
//!   Leak-Gefahr.
//! * Der Host sieht dieselben Objekte, die das Skript sieht —
//!   `narsil` braucht keine eigene Abbildung.
//! * Alle Grenzen greifen an einer Stelle: Jeder Knoten nimmt Speicher aus
//!   dem Heap, und der Heap hat seine Obergrenze.
//!
//! # WAS ES NICHT IST (bewusst)
//!
//! * **Kein Layout, kein CSS-Kaskaden.** Selektoren werden nur zum **Finden**
//!   benutzt (Tag, `#id`, `.klasse`, `[attr]`, `*`, Nachfahren). Die
//!   Selektor-Engine aus `narsil-css` wird *nicht* eingebunden — die Crate
//!   gehoert einer anderen Rolle, und eine Abhaengigkeit wuerde jeden Bau
//!   von `narsil-jsvm` an deren Zwischenstand koppeln (GEMESSEN am 30.09. mit
//!   `narsil-net`/`narsil-tls`).
//! * **Kein vollstaendiger HTML-Parser.** `innerHTML = …` versteht die
//!   gehoeufigen Faelle (Text, Tags, Attribute, Selbstschliessende, Kommentare).
//!   Der volle Parser bleibt bei `narsil-fmt-html` (J2 beschreibt, wie narsil
//!   die Bruecke baut).
//!
//! # GRENZEN (M1–M9)
//!
//! | was | Grenze | wo |
//! |---|---|---|
//! | erzeugte Knoten je Lauf | `max_dom_knoten` 20.000 (hart 200.000) | `Grenzen` |
//! | Tiefe beim `innerHTML`-Schreiben | `max_dom_tiefe` 64 (hart 512) | `Grenzen` |
//! | Laenge eines Textknotens | `max_string_laenge` | `Grenzen` |
//! | `querySelectorAll` Treffer | `MAX_SAMMLUNG` 100.000 | `crate::ops` |
//! | Laenge des HTML-Textes | `max_dom_html` 4 MiB (hart 32 MiB) | `Grenzen` |
//!
//! Jede Zaehlung ist eine **benannte Konstante** mit hartem Deckel: die
//! Knoedenzahl wird beim Anlegen erhoeht und nie zurueckgesetzt (dokumentiert:
//! „pro Lauf", wie die Timer).

use crate::Wert;
use crate::builtin::Nativ;
use crate::grenzen::Grenzen;
use crate::heap::{GVal, HObj, HeapRef};

/// M3/J-AR: Grenze fuer `document.cookie = …` (ein Cookie ist klein; 8 KiB
/// sind schon ein sehr langes).
const MAX_COOKIE_LAENGE: usize = 8 * 1024;
use crate::interp::{Engine, Stoerung};

/// Die Felder, mit denen ein Knoten seine Daten traegt (alle `__`, damit sie
/// nicht in `Object.keys` erscheinen).
pub(crate) const K_TYP: &str = "__typ";
pub(crate) const K_NAME: &str = "__name";
pub(crate) const K_TEXT: &str = "__text";
pub(crate) const K_KINDER: &str = "__kinder";
pub(crate) const K_ELTERN: &str = "__eltern";
pub(crate) const K_ID: &str = "__id";
pub(crate) const K_KLASSE: &str = "__klasse";
pub(crate) const K_STIL: &str = "__stil";
/// Die stabile Nummer eines Knotens (J2: der Host spricht den Knoten
/// ueber diese Nummer an, nicht ueber eine `HeapRef` — die ist nach einer
/// Heap-Sammlung nicht mehr gueltig).
pub(crate) const K_KNOTEN: &str = "__knoten";

/// `nodeType` eines Elements (DOM-Spec).
pub(crate) const TYP_ELEMENT: f64 = 1.0;
/// `nodeType` eines Textknotens.
pub(crate) const TYP_TEXT: f64 = 3.0;
/// `nodeType` des Dokuments.
pub(crate) const TYP_DOKUMENT: f64 = 9.0;

/// Harte Obergrenze der erzeugten Knoten je Lauf (M4).
pub(crate) const MAX_KNOTEN_HART: usize = 200_000;
/// Harte Obergrenze der Tiefe beim `innerHTML`-Schreiben (M5).
pub(crate) const TIEFE_HART: u32 = 512;
/// Harte Obergrenze des HTML-Textes (M4).
pub(crate) const MAX_HTML_HART: usize = 32 * 1024 * 1024;

/// Die void-Elemente (ohne schliessendes Tag) — HTML-Spec.
pub(crate) fn ist_void(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Die Elemente, deren Inhalt ROH ist (`script`, `style`) — dort wird der
/// Text nicht als Kind geparst (HTML-Spec).
pub(crate) fn ist_roh(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "script" | "style")
}

impl Engine {
    // -----------------------------------------------------------------
    // Knoten anlegen
    // -----------------------------------------------------------------

    /// `document.createElement(name)`.
    pub(crate) fn dom_create_element(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let name = self.zu_text(self.arg(args, 0))?.to_ascii_lowercase();
        crate::element_name(&name).map_err(|kind| Stoerung::Wurf(self.fehler_wert(kind, "ungueltiger Elementname")))?;
        self.dom_element(&name)
    }

    pub(crate) fn dom_create_element_ns(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let ns = self.arg(args, 0);
        let ns = if matches!(ns, GVal::Null | GVal::Undefiniert) { None } else { Some(self.zu_text(ns)?) };
        let name = self.zu_text(self.arg(args, 1))?;
        let (prefix, local) = crate::namespace_name(ns.as_deref(), &name)
            .map_err(|e| Stoerung::Wurf(self.fehler_wert(e, "ungueltiger qualifizierter Name")))?;
        let element = self.dom_element(&name)?;
        if let GVal::Ref(r) = element {
            if ns.as_deref() != Some("http://www.w3.org/1999/xhtml") {
                if let HObj::Gegenstand { proto, .. } = self.heap.holen_mut(r) { *proto = Some(self.proto_dom_element); }
            }
            self.dom_namespace(r, ns.as_deref().filter(|s| !s.is_empty()), prefix.as_deref(), &local)?;
        }
        Ok(element)
    }

    pub(crate) fn dom_image(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let doc = self.hole(GVal::Ref(self.global_objekt), "document")?;
        let creator = self.hole(doc, "createElement")?;
        let tag = self.text_wert("img").map_err(Stoerung::Fehler)?;
        let image = self.rueckruf_mit(creator, doc, &[tag])?;
        self.wurzeln.push(image);
        if let GVal::Ref(r) = image {
            if let HObj::Gegenstand { proto, .. } = self.heap.holen_mut(r) { *proto = Some(self.proto_dom_image); }
        }
        for (i, name) in ["width", "height"].iter().enumerate() {
            let number = if let Some(value) = args.get(i) { self.zu_zahl(*value)? } else { 0.0 };
            let dimension = if !number.is_finite() { 0.0 } else { number.trunc().rem_euclid(4294967296.0) };
            if args.get(i).is_some() {
                let setter = self.hole(image, "setAttribute")?;
                let key = self.text_wert(name).map_err(Stoerung::Fehler)?;
                self.rueckruf_mit(setter, image, &[key, GVal::Zahl(dimension)])?;
            }
        }
        Ok(image)
    }

    pub(crate) fn dom_ref_lesen(&mut self, this: GVal, name: &str) -> Result<GVal, Stoerung> {
        let getter = self.hole(this, "getAttribute")?;
        let key = self.text_wert(name).map_err(Stoerung::Fehler)?;
        let value = self.rueckruf_mit(getter, this, &[key])?;
        if matches!(name, "width" | "height") {
            let n = if matches!(value, GVal::Null | GVal::Undefiniert) { 0.0 } else { self.zu_zahl(value)? };
            Ok(GVal::Zahl(if n.is_finite() && n >= 0.0 { n.trunc() } else { 0.0 }))
        } else if matches!(value, GVal::Null | GVal::Undefiniert) {
            self.text_wert("").map_err(Stoerung::Fehler)
        } else { Ok(value) }
    }

    pub(crate) fn dom_ref_schreiben(&mut self, this: GVal, args: &[GVal], name: &str) -> Result<GVal, Stoerung> {
        let mut value = self.arg(args, 0);
        if matches!(name, "width" | "height") {
            let n = self.zu_zahl(value)?;
            value = GVal::Zahl(if n.is_finite() { n.trunc().rem_euclid(4294967296.0) } else { 0.0 });
        }
        let setter = self.hole(this, "setAttribute")?;
        let key = self.text_wert(name).map_err(Stoerung::Fehler)?;
        self.rueckruf_mit(setter, this, &[key, value])?;
        Ok(GVal::Undefiniert)
    }

    fn dom_namespace(&mut self, r: HeapRef, ns: Option<&str>, prefix: Option<&str>, local: &str) -> Result<(), Stoerung> {
        // HTML metadata is shared through prototype getters. Allocating three
        // identical properties per node wastes the bounded DOM heap.
        if ns == Some("http://www.w3.org/1999/xhtml") && prefix.is_none() { return Ok(()); }
        for (name, text) in [("namespaceURI", ns), ("prefix", prefix), ("localName", Some(local))] {
            let value = match text { Some(t) => self.text_wert(t).map_err(Stoerung::Fehler)?, None => GVal::Null };
            self.versteckt_setzen(r, name, value)?;
            if let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen_mut(r) {
                eigenschaften.get_mut(name).unwrap().beschreibbar = false;
            }
        }
        Ok(())
    }

    pub(crate) fn dom_namespace_lesen(&mut self, this: GVal, name: &str) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else { return Ok(GVal::Null); };
        if let Some(value) = self.eigene_lese(r, name) { return Ok(value); }
        match name {
            "namespaceURI" => self.text_wert("http://www.w3.org/1999/xhtml").map_err(Stoerung::Fehler),
            "localName" => Ok(self.dom_versteckt_lese(r, K_NAME).unwrap_or(GVal::Null)),
            _ => Ok(GVal::Null),
        }
    }

    /// `document.createTextNode(text)`.
    pub(crate) fn dom_create_text(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let text = self
            .zu_text(self.arg(args, 0))
            .unwrap_or_else(|_| String::new());
        if text.len() > self.grenzen.max_string_laenge {
            return Err(Stoerung::Wurf(
                self.fehler_wert("Error", "Textknoten zu lang (Grenze erreicht)"),
            ));
        }
        self.dom_text(&text)
    }

    /// Ein Element ohne Eigenschaften anlegen (der Dokument-Zaehler zaehlt).
    pub(crate) fn dom_element(&mut self, name: &str) -> Result<GVal, Stoerung> {
        self.dom_zaehle()?;
        let o = self
            .neues_objekt(Some(if name == "img" { self.proto_dom_image } else { self.proto_dom_html }))
            .map_err(Stoerung::Fehler)?;
        let kinder = self.neues_feld().map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(o, K_TYP, GVal::Zahl(TYP_ELEMENT))?;
        let v_k_name = self.text_wert(name).map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(o, K_NAME, v_k_name)?;
        let v_k_text = self.text_wert("").map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(o, K_TEXT, v_k_text)?;
        self.versteckt_setzen(o, K_KINDER, GVal::Ref(kinder))?;
        self.versteckt_setzen(o, K_ELTERN, GVal::Undefiniert)?;
        let v_k_id = self.text_wert("").map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(o, K_ID, v_k_id)?;
        let v_k_klasse = self.text_wert("").map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(o, K_KLASSE, v_k_klasse)?;
        let v_k_stil = self.text_wert("").map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(o, K_STIL, v_k_stil)?;
        // J2: die Nummer, ueber die der Host den Knoten anspricht.
        let nr = self.dom_nummer()? as f64;
        self.versteckt_setzen(o, K_KNOTEN, GVal::Zahl(nr))?;
        self.dom_namespace(o, Some("http://www.w3.org/1999/xhtml"), None, name)?;
        Ok(GVal::Ref(o))
    }

    /// Einen Textknoten anlegen.
    pub(crate) fn dom_text(&mut self, text: &str) -> Result<GVal, Stoerung> {
        self.dom_zaehle()?;
        let o = self
            .neues_objekt(Some(self.proto_dom_text))
            .map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(o, K_TYP, GVal::Zahl(TYP_TEXT))?;
        let v_k_name = self.text_wert("#text").map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(o, K_NAME, v_k_name)?;
        let v_k_text = self.text_wert(text).map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(o, K_TEXT, v_k_text)?;
        self.versteckt_setzen(o, K_ELTERN, GVal::Undefiniert)?;
        let nr = self.dom_nummer()? as f64;
        self.versteckt_setzen(o, K_KNOTEN, GVal::Zahl(nr))?;
        Ok(GVal::Ref(o))
    }

    /// Das Dokument anlegen (Wurzel, ein Kind `<html>`).
    pub(crate) fn dom_dokument_leer(&mut self) -> Result<GVal, Stoerung> {
        self.dom_zaehle()?;
        let doc = self
            .neues_objekt(Some(self.proto_dom_dokument))
            .map_err(Stoerung::Fehler)?;
        let kinder = self.neues_feld().map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(doc, K_TYP, GVal::Zahl(TYP_DOKUMENT))?;
        let dok_name = self.text_wert("#document").map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(doc, K_NAME, dok_name)?;
        self.versteckt_setzen(doc, K_KINDER, GVal::Ref(kinder))?;
        self.versteckt_setzen(doc, K_ELTERN, GVal::Undefiniert)?;
        let nr = self.dom_nummer()? as f64;
        self.versteckt_setzen(doc, K_KNOTEN, GVal::Zahl(nr))?;
        let content_type = self.text_wert("application/xml").map_err(Stoerung::Fehler)?;
        self.setze_eigen(doc, "contentType", content_type);
        Ok(GVal::Ref(doc))
    }

    pub(crate) fn dom_dokument_anlegen(&mut self) -> Result<GVal, Stoerung> {
        let GVal::Ref(doc) = self.dom_dokument_leer()? else { unreachable!() };
        let html = self.dom_element("html")?;
        self.dom_anhaengen(GVal::Ref(doc), html)?;
        // GEMESSEN: ohne `<head>`/`<body>` war `document.body === null` und
        // damit fast jede Skriptzeile auf `body.…` ein TypeError.
        for tag in ["head", "body"] {
            let GVal::Ref(h) = html else {
                break;
            };
            let kind = self.dom_element(tag)?;
            self.dom_anhaengen(GVal::Ref(h), kind)?;
        }
        Ok(GVal::Ref(doc))
    }

    /// Den Knoten-Zaehler pruefen (M4) und erhoehen.
    fn dom_zaehle(&mut self) -> Result<(), Stoerung> {
        if self.dom_knoten >= self.grenzen.max_dom_knoten.min(MAX_KNOTEN_HART) {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "Error",
                &format!(
                    "zu viele Knoten (Grenze {})",
                    self.grenzen.max_dom_knoten.min(MAX_KNOTEN_HART)
                ),
            )));
        }
        self.dom_knoten += 1;
        Ok(())
    }

    /// Den Tag-Namen eines Arguments lesen (`createElement(42)` -> „div").
    fn dom_name(&mut self, v: Option<&GVal>) -> Result<String, Stoerung> {
        let roh = match v {
            None | Some(GVal::Undefiniert) | Some(GVal::Null) => String::new(),
            Some(other) => self.zu_text(*other).unwrap_or_default(),
        };
        // HTML-Spec: ein ungueltiger Name wird zu `div` (createElement
        // mit einem Namen, der kein gueltiger Bezeichner ist).
        let name = roh.trim().to_ascii_lowercase();
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ':')
        {
            return Ok("div".to_string());
        }
        Ok(name)
    }

    // -----------------------------------------------------------------
    // Baum
    // -----------------------------------------------------------------

    /// `el.appendChild(kind)` — haengt an und gibt den Knoten zurueck
    /// (DOM-Spec: `appendChild` gibt den angehaengten Knoten zurueck).
    pub(crate) fn dom_append_child(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let kind = self.arg(args, 0);
        self.dom_anhaengen(this, kind)?;
        Ok(kind)
    }

    /// `kind` an das Ende der Kinder von `el` haengen (DOM-Spec: ein Knoten,
    /// der schon einen Elternteil hat, wird erst von diesem geloest — das
    /// nennt die Spec „replace" bei `appendChild` mit einem Knoten, der
    /// schon Eltern hat; wir loesen und haengen, wie die meisten Engines).
    fn dom_anhaengen(&mut self, eltern: GVal, kind: GVal) -> Result<(), Stoerung> {
        let GVal::Ref(e) = eltern else {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "kein Knoten")));
        };
        // GEMESSEN: diese Pruefung verbot das Dokument als Elternteil —
        // damit blieb `document` leer und `document.body` war `null`
        // („ReferenceError: 'document' ist nicht definiert", weil
        // `dom_aufbauen` stillschweigend scheiterte). Das Dokument IST die
        // Wurzel des Baums (DOM-Spec) und darf Kinder haben.
        let _ = e;
        if kind_ref(kind).is_none()
            || self.dom_typ(kind_ref(kind).unwrap_or(crate::heap::KEIN_REF)) == 0.0
        {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "appendChild: kein Knoten"),
            ));
        }
        // GEMESSEN (J1): die Pruefung lief in die falsche Richtung —
        // `b.appendChild(a)` (a ist b's Elternteil) wurde erlaubt und
        // liess einen Zyklus entstehen. Gefragt ist: ist der ELTERNIEL
        // selbst unter dem neuen Kind?
        if self.dom_ist_hierunter(GVal::Ref(e), kind) {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "HierarchyRequestError",
                "ein Knoten kann nicht sein eigener Vorfahr werden",
            )));
        }
        // M3: Die Kinderliste ist ein Feld aus dem Heap — das zaehlt mit.
        self.dom_loese(kind)?;
        let kinder = self.dom_kinder_ref(e)?;
        self.wurzeln.push(kind);
        {
            let o = self.heap.holen_mut(kinder);
            if let HObj::Feld { elemente, .. } = o {
                elemente.push(kind);
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            }
        }
        if let Some(k) = kind_ref(kind) {
            self.versteckt_setzen(k, K_ELTERN, eltern)?;
        }
        Ok(())
    }

    /// `el.insertBefore(kind, vor)` — DOM-Spec: `vor === null` == anhaengen.
    pub(crate) fn dom_insert_before(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let kind = self.arg(args, 0);
        let vor = self.arg(args, 1);
        let GVal::Ref(e) = this else {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "kein Knoten")));
        };
        if !matches!(vor, GVal::Null) {
            // `vor` muss ein Kind von `el` sein (DOM-Spec: sonst NotFoundError).
            if !self.dom_kinder_von(e).contains(&vor) {
                return Err(Stoerung::Wurf(self.fehler_wert(
                    "NotFoundError",
                    "insertBefore: der Knoten ist kein Kind",
                )));
            }
        }
        if matches!(vor, GVal::Null) {
            self.dom_anhaengen(this, kind)?;
            return Ok(kind);
        }
        self.dom_loese(kind)?;
        let kinder = self.dom_kinder_ref(e)?;
        let stelle = self
            .dom_kinder_von(e)
            .iter()
            .position(|k| *k == vor)
            .unwrap_or(0);
        self.wurzeln.push(kind);
        {
            let o = self.heap.holen_mut(kinder);
            if let HObj::Feld { elemente, .. } = o {
                let s = stelle.min(elemente.len());
                elemente.insert(s, kind);
                self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            }
        }
        if let Some(k) = kind_ref(kind) {
            self.versteckt_setzen(k, K_ELTERN, this)?;
        }
        Ok(kind)
    }

    /// `el.removeChild(kind)`.
    pub(crate) fn dom_remove_child(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let kind = self.arg(args, 0);
        let GVal::Ref(e) = this else {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "kein Knoten")));
        };
        let stelle = self
            .dom_kinder_von(e)
            .iter()
            .position(|k| *k == kind)
            .ok_or_else(|| {
                Stoerung::Wurf(self.fehler_wert("NotFoundError", "removeChild: kein Kind"))
            })?;
        let kinder = self.dom_kinder_ref(e)?;
        {
            let o = self.heap.holen_mut(kinder);
            if let HObj::Feld { elemente, .. } = o {
                elemente.remove(stelle);
            }
        }
        if let GVal::Ref(k) = kind {
            self.versteckt_setzen(k, K_ELTERN, GVal::Undefiniert)?;
        }
        Ok(kind)
    }

    /// J6c: `el.remove()` — der Knoten loest sich vom Elternteil.
    ///
    /// GEMESSEN: `removeChild` war da, `remove` nicht — die Meldung
    /// „`remove()` ist ein wirkungsloser Aufruf" traf also zu, die Ursache
    /// war aber eine **fehlende** Methode (TypeError) und kein Fehler im
    /// Ablauf. Ein Knoten ohne Elternteil ist laut Spec ein Fehler; da der
    /// hier nicht im Baum ist, wird er wie in `dom_loese` behandelt
    /// (nichts tun, `undefined` zurueckgeben).
    pub(crate) fn dom_entfernen(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        if !matches!(this, GVal::Ref(_)) {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "kein Knoten")));
        }
        self.dom_loese(this)?;
        Ok(GVal::Undefiniert)
    }

    /// Einen Knoten aus seinem Elternteil loesen (Spec: der Knoten bleibt
    /// erhalten, nur die Bindung faellt).
    fn dom_loese(&mut self, kind: GVal) -> Result<(), Stoerung> {
        let GVal::Ref(k) = kind else {
            return Ok(());
        };
        let e = self.dom_eltern_ref(k);
        let Some(ep) = e else {
            return Ok(());
        };
        let kinder = self.dom_kinder_ref(ep)?;
        {
            let o = self.heap.holen_mut(kinder);
            if let HObj::Feld { elemente, .. } = o {
                elemente.retain(|x| *x != kind);
            }
        }
        Ok(())
    }

    /// `kind` ist ein Vorfahr von `vater`?
    fn dom_ist_hierunter(&mut self, kind: GVal, vater: GVal) -> bool {
        let GVal::Ref(mut k) = kind else {
            return false;
        };
        // M3: Die Kette ist durch die Baumtiefe begrenzt (sonst Endlos).
        let mut reste = self.grenzen.max_dom_tiefe.min(TIEFE_HART) + 1;
        while reste > 0 {
            reste -= 1;
            if GVal::Ref(k) == vater {
                return true;
            }
            match self.dom_eltern_ref(k) {
                Some(e) => k = e,
                None => return false,
            }
        }
        false
    }

    // -----------------------------------------------------------------
    // Eigenschaften
    // -----------------------------------------------------------------

    /// `el.textContent` lesen: die Texte aller Nachfahren in Reihenfolge
    /// (DOM-Spec: „the descendant text content").
    pub(crate) fn dom_text_lese(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let mut s = String::new();
        self.dom_text_sammeln(this, &mut s, 0)?;
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    /// `el.textContent = w` — entfernt alle Kinder und setzt einen
    /// Textknoten (DOM-Spec).
    pub(crate) fn dom_text_schreibe(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let text = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        if let GVal::Ref(e) = this {
            // Alle Kinder loesen und als eigene Felder eintragen (die
            // Kinderliste bleibt bestehen, wird aber geleert).
            for k in self.dom_kinder_von(e) {
                if let GVal::Ref(kr) = k {
                    self.versteckt_setzen(kr, K_ELTERN, GVal::Undefiniert)?;
                }
            }
            let kinder = self.dom_kinder_ref(e)?;
            {
                let o = self.heap.holen_mut(kinder);
                if let HObj::Feld { elemente, .. } = o {
                    elemente.clear();
                }
            }
        }
        if !text.is_empty() {
            let t = self.dom_text(&text)?;
            self.dom_anhaengen(this, t)?;
        }
        Ok(GVal::Undefiniert)
    }

    fn dom_text_sammeln(&mut self, v: GVal, s: &mut String, tiefe: u32) -> Result<(), Stoerung> {
        if tiefe > self.grenzen.max_dom_tiefe.min(TIEFE_HART) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("Error", "DOM zu tief (Grenze erreicht)"),
            ));
        }
        let GVal::Ref(r) = v else {
            return Ok(());
        };
        match self.dom_typ(r) {
            TYP_TEXT => {
                let t = self.dom_text_von(r).unwrap_or_else(|| GVal::Undefiniert);
                let txt = self.zu_text(t).unwrap_or_default();
                // M4: Die Laenge ist durch `max_string_laenge` begrenzt.
                if s.len() + txt.len() > self.grenzen.max_string_laenge {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("Error", "Text zu lang (Grenze erreicht)"),
                    ));
                }
                s.push_str(&txt);
            }
            TYP_ELEMENT | TYP_DOKUMENT => {
                for k in self.dom_kinder_von(r) {
                    self.dom_text_sammeln(k, s, tiefe + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// `el.getAttribute(name)` — `null`, wenn es das Attribut nicht gibt
    /// (DOM-Spec, nicht `undefined`).
    pub(crate) fn dom_attribute_lesen(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let name = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let GVal::Ref(r) = this else {
            return Ok(GVal::Null);
        };
        match self.dom_attribute_suche(r, &name.to_ascii_lowercase()) {
            Some(v) => Ok(v),
            None => Ok(GVal::Null),
        }
    }

    /// `el.setAttribute(name, wert)`.
    pub(crate) fn dom_attribute_schreiben(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let name = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let wert = self.zu_text(self.arg(args, 1)).unwrap_or_default();
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        if self.dom_typ(r) != TYP_ELEMENT {
            return Err(Stoerung::Wurf(
                self.fehler_wert("InvalidCharacterError", "kein Element"),
            ));
        }
        let name = name.to_ascii_lowercase();
        if name.is_empty() {
            return Err(Stoerung::Wurf(
                self.fehler_wert("InvalidCharacterError", "leerer Attributname"),
            ));
        }
        // Die Liste der „sonstigen" Attribute liegt in einem Objekt, damit
        // `setAttribute` beliebig viele Namen trägt (M4: Heap zählt).
        if self.dom_versteckt_lese(r, "__attribute").is_none() {
            let m = self.neues_objekt(None).map_err(Stoerung::Fehler)?;
            self.versteckt_setzen(r, "__attribute", GVal::Ref(m))?;
        }
        let m = self
            .dom_versteckt_lese(r, "__attribute")
            .unwrap_or(GVal::Undefiniert);
        let w = self.text_wert(&wert).map_err(Stoerung::Fehler)?;
        self.setze(m, &name, w)?;
        Ok(GVal::Undefiniert)
    }

    /// `el.removeAttribute(name)`.
    pub(crate) fn dom_attribute_entfernen(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let name = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        if let Some(m) = self.dom_versteckt_lese(r, "__attribute") {
            self.loesche(m, &name.to_ascii_lowercase())?;
        }
        Ok(GVal::Undefiniert)
    }

    /// Ein Attribut suchen (`id`, `class`, `style` liegen in eigenen
    /// Feldern, der Rest im Attribut-Objekt).
    fn dom_attribute_suche(&mut self, r: HeapRef, name: &str) -> Option<GVal> {
        match name {
            "id" => self.dom_versteckt_lese(r, K_ID),
            "class" => self.dom_versteckt_lese(r, K_KLASSE),
            "style" => self.dom_versteckt_lese(r, K_STIL),
            _ => {
                let GVal::Ref(m) = self.dom_versteckt_lese(r, "__attribute")? else {
                    return None;
                };
                self.eigene_lese(m, name)
            }
        }
    }

    // -----------------------------------------------------------------
    // classList / style
    // -----------------------------------------------------------------

    /// `el.classList.add(...)` — die Klassen sind ein **Text** mit Leerzeichen
    /// getrennt (DOM-Spec: `class` ist eine geordnete Menge von Tokens).
    pub(crate) fn dom_klasse_aendern(
        &mut self,
        this: GVal,
        args: &[GVal],
        art: u8,
    ) -> Result<GVal, Stoerung> {
        // `this` ist das `classList`-Objekt; gearbeitet wird auf dem
        // ELEMENT (`__ziel`).
        let this = self.dom_ziel(this);
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let mut klassen = self.dom_klassen(r);
        for (i, a) in args.iter().enumerate() {
            let name = match self.zu_text(*a) {
                Ok(t) => t.trim().to_string(),
                Err(_) => {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("SyntaxError", "classList: keine Klasse"),
                    ));
                }
            };
            if name.is_empty() {
                continue;
            }
            // M3: Die Klassenliste ist durch die Attributgrenze gedeckelt
            // (ein Attribut hat `max_string_laenge`).
            if klassen.len() + name.len() > self.grenzen.max_string_laenge {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("Error", "classList zu lang (Grenze erreicht)"),
                ));
            }
            let stelle = klassen.iter().position(|k| *k == name);
            // 0 = add, 1 = remove, 2 = toggle
            if (art == 0 && stelle.is_none())
                || (art == 2 && stelle.is_none())
                || (art == 1 && stelle.is_some())
            {
                match art {
                    0 | 2 => klassen.push(name),
                    _ => klassen.retain(|k| *k != name),
                }
            } else if art == 2
                && let Some(i) = stelle
            {
                klassen.remove(i);
            }
            let _ = i;
        }
        let v_k_klasse = self
            .text_wert(&klassen.join(" "))
            .map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(r, K_KLASSE, v_k_klasse)?;
        Ok(GVal::Undefiniert)
    }

    /// `el.classList.contains(name)`.
    pub(crate) fn dom_klasse_hat(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let name = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let this = self.dom_ziel(this);
        let GVal::Ref(r) = this else {
            return Ok(GVal::Bool(false));
        };
        let treffer = self.dom_klassen(r).iter().any(|k| *k == name.trim());
        Ok(GVal::Bool(treffer))
    }

    /// Die Klassen eines Knotens als Liste (Token der Reihenfolge nach).
    fn dom_klassen(&mut self, r: HeapRef) -> Vec<String> {
        self.dom_versteckt_lese(r, K_KLASSE)
            .map(|v| self.zu_text(v).unwrap_or_default())
            .unwrap_or_default()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    }

    /// `el.style` als Objekt: Lesen und Schreiben einzelner eigenschaften
    /// geht ueber `__stil` (ein Text „a:b; c:d").
    pub(crate) fn dom_stil_setzen(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let this = self.dom_ziel(this);
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let name = self
            .zu_text(self.arg(args, 0))
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let wert = self.zu_text(self.arg(args, 1)).unwrap_or_default();
        if name.is_empty() {
            return Err(Stoerung::Wurf(
                self.fehler_wert("SyntaxError", "style: leerer Name"),
            ));
        }
        // GEMESSEN (J1): `setProperty('color', '')` HAETTE die Angabe nur auf
        // einen leeren Wert gesetzt (CSSOM entfernt sie). Der Leerwert wird
        // deshalb als „entfernen" behandelt — und das Entfernen ist ein
        // eigener Weg, weil `dom_stil_text(…, None)` nur liest.
        if wert.is_empty() {
            self.dom_stil_weg(r, &name);
        } else {
            self.dom_stil_text(r, &name, Some(&wert));
        }
        Ok(GVal::Undefiniert)
    }

    /// Eine Angabe aus dem Stiltext entfernen (CSSOM `removeProperty`).
    pub(crate) fn dom_stil_weg(&mut self, r: HeapRef, name: &str) {
        let alt = self
            .dom_versteckt_lese(r, K_STIL)
            .map(|v| self.zu_text(v).unwrap_or_default())
            .unwrap_or_default();
        let paare: Vec<(String, String)> = alt
            .split(';')
            .filter_map(|t| {
                let (a, b) = t.split_once(':')?;
                Some((a.trim().to_ascii_lowercase(), b.trim().to_string()))
            })
            .filter(|(a, _)| a.as_str() != name)
            .collect();
        let neu = paare
            .iter()
            .map(|(a, b)| format!("{a}: {b}"))
            .collect::<Vec<String>>()
            .join("; ");
        let v = self.text_wert(&neu).unwrap_or(GVal::Undefiniert);
        let _ = self.versteckt_setzen(r, K_STIL, v);
    }

    /// `el.style.getPropertyValue(name)`.
    pub(crate) fn dom_stil_lesen(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let this = self.dom_ziel(this);
        let name = self
            .zu_text(self.arg(args, 0))
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let GVal::Ref(r) = this else {
            return Ok(self.text_wert("").map_err(Stoerung::Fehler)?);
        };
        let w = self.dom_stil_text(r, &name, None);
        Ok(self.text_wert(&w).map_err(Stoerung::Fehler)?)
    }

    /// Eine Angabe im Stiltext setzen/entfernen und den Text zurueckschreiben.
    fn dom_stil_text(&mut self, r: HeapRef, name: &str, wert: Option<&str>) -> String {
        let alt = self
            .dom_versteckt_lese(r, K_STIL)
            .map(|v| self.zu_text(v).unwrap_or_default())
            .unwrap_or_default();
        let mut paare: Vec<(String, String)> = alt
            .split(';')
            .filter_map(|t| {
                let (a, b) = t.split_once(':')?;
                Some((a.trim().to_ascii_lowercase(), b.trim().to_string()))
            })
            .collect();
        // GEMESSEN (J1): `retain` lief auch beim LESEN — der Wert wurde
        // also beim `getPropertyValue` aus dem Stiltext entfernt (und
        // `p.__stil` war danach leer). Beim Lesen wird nichts geaendert.
        if wert.is_none() {
            return paare
                .iter()
                .find(|(a, _)| a.as_str() == name)
                .map(|(_, b)| b.clone())
                .unwrap_or_default();
        }
        paare.retain(|(a, _)| a.as_str() != name);
        paare.push((
            name.to_string(),
            wert.unwrap_or_default().trim().to_string(),
        ));
        let neu = paare
            .iter()
            .map(|(a, b)| format!("{a}: {b}"))
            .collect::<Vec<String>>()
            .join("; ");
        let v = self.text_wert(&neu).unwrap_or(GVal::Undefiniert);
        let _ = self.versteckt_setzen(r, K_STIL, v);
        // GEMESSEN (J1): hier stand `wert.map(...)` — beim Lesen kam damit
        // immer `leer` zurueck (`style.getPropertyValue('color')` lieferte
        // „" nach einem `setProperty`). Jetzt der Wert, der jetzt steht.
        paare
            .iter()
            .find(|(a, _)| a.as_str() == name)
            .map(|(_, b)| b.clone())
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------
    // innerHTML
    // -----------------------------------------------------------------

    /// `el.innerHTML` lesen: die Kinder als HTML-Text (Serialisierung).
    pub(crate) fn dom_inner_html_lesen(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(self.text_wert("").map_err(Stoerung::Fehler)?);
        };
        let mut s = String::new();
        self.dom_serialisieren(r, &mut s, 0)?;
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    fn dom_serialisieren(
        &mut self,
        r: HeapRef,
        s: &mut String,
        tiefe: u32,
    ) -> Result<(), Stoerung> {
        if tiefe > self.grenzen.max_dom_tiefe.min(TIEFE_HART) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("Error", "DOM zu tief (Grenze erreicht)"),
            ));
        }
        if s.len() > self.grenzen.max_dom_html.min(MAX_HTML_HART) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("Error", "innerHTML zu lang (Grenze erreicht)"),
            ));
        }
        let kinder = self.dom_kinder_von(r);
        for k in kinder {
            let GVal::Ref(kr) = k else { continue };
            if self.dom_typ(kr) == TYP_TEXT {
                let t = self
                    .dom_text_von(kr)
                    .map(|v| self.zu_text(v).unwrap_or_default())
                    .unwrap_or_default();
                s.push_str(&t);
                continue;
            }
            let name = self
                .dom_versteckt_lese(kr, K_NAME)
                .map(|v| self.zu_text(v).unwrap_or_default())
                .unwrap_or_default();
            s.push('<');
            s.push_str(&name);
            for (a, w) in self.dom_attribute_liste(kr) {
                s.push(' ');
                s.push_str(&a);
                s.push_str("=\"");
                s.push_str(&w.replace('"', "&quot;"));
                s.push('"');
            }
            s.push('>');
            if !ist_void(&name) {
                self.dom_serialisieren(kr, s, tiefe + 1)?;
                s.push_str("</");
                s.push_str(&name);
                s.push('>');
            }
        }
        Ok(())
    }

    /// Die Attribute eines Elements in Reihenfolge (Sonderattribute zuerst,
    /// wie im HTML-Quelltext gebraeuchlich).
    fn dom_attribute_liste(&mut self, r: HeapRef) -> Vec<(String, String)> {
        let mut raus: Vec<(String, String)> = Vec::new();
        for (feld, name) in [(K_ID, "id"), (K_KLASSE, "class"), (K_STIL, "style")] {
            if let Some(v) = self.dom_versteckt_lese(r, feld) {
                let w = self.zu_text(v).unwrap_or_default();
                if !w.is_empty() {
                    raus.push((name.to_string(), w));
                }
            }
        }
        if let Some(m) = self.dom_versteckt_lese(r, "__attribute") {
            // M3: `MAX_SAMMLUNG` Attribute.
            let Some(m) = kind_ref(m) else { return raus };
            for (k, w) in self
                .eigene_map(m)
                .into_iter()
                .take(crate::ops::MAX_SAMMLUNG)
            {
                let wt = self.zu_text(w.wert).unwrap_or_default();
                raus.push((k, wt));
            }
        }
        raus
    }

    /// `el.innerHTML = html` — die Kinder ersetzen.
    ///
    /// Der Parser versteht: Text, offene/schliessende Tags mit Attributen,
    /// Kommentare, Selbstschliessende (`<br/>`) und die void-Elemente. Er
    /// laeuft **rekursiv mit Tiefengrenze** (`max_dom_tiefe`), jeder Knoten
    /// zaehlt gegen `max_dom_knoten`.
    pub(crate) fn dom_inner_html_schreiben(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let html = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        if html.len() > self.grenzen.max_dom_html.min(MAX_HTML_HART) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("Error", "innerHTML zu lang (Grenze erreicht)"),
            ));
        }
        // Alte Kinder loesen.
        if let GVal::Ref(e) = this {
            for k in self.dom_kinder_von(e) {
                if let GVal::Ref(kr) = k {
                    self.versteckt_setzen(kr, K_ELTERN, GVal::Undefiniert)?;
                }
            }
            let kinder = self.dom_kinder_ref(e)?;
            {
                let o = self.heap.holen_mut(kinder);
                if let HObj::Feld { elemente, .. } = o {
                    elemente.clear();
                }
            }
        }
        self.dom_html_als_kinder(this, &html, 0)?;
        Ok(GVal::Undefiniert)
    }

    /// Den Text als Kinder einhaengen (rekursiv, mit Tiefengrenze).
    fn dom_html_als_kinder(
        &mut self,
        eltern: GVal,
        html: &str,
        tiefe: u32,
    ) -> Result<(), Stoerung> {
        if tiefe > self.grenzen.max_dom_tiefe.min(TIEFE_HART) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("Error", "innerHTML zu tief (Grenze erreicht)"),
            ));
        }
        let mut pos = 0usize;
        let mut offen: Vec<(String, GVal)> = Vec::new(); // (Name, Knoten)
        let bytes: Vec<char> = html.chars().collect();
        let mut text_start = 0usize;
        while pos < bytes.len() {
            // M3: Die Schleife laeuft ueber die Laenge des Textes (Eingabe).
            if bytes[pos] != '<' {
                pos += 1;
                continue;
            }
            if text_start < pos {
                let t: String = bytes[text_start..pos].iter().collect();
                if !t.trim().is_empty() {
                    let kn = self.dom_text(&t)?;
                    let ziel = offen.last().map(|(_, k)| *k).unwrap_or(eltern);
                    self.dom_anhaengen(ziel, kn)?;
                }
            }
            // Kommentar?
            if bytes[pos..].starts_with(&['<', '!', '-', '-']) {
                match find(&bytes, pos, "-->") {
                    Some(ende) => {
                        pos = ende + 3;
                        text_start = pos;
                        continue;
                    }
                    None => break,
                }
            }
            // Ende des Tags finden (`>` ausserhalb der Anfuehrungszeichen).
            let mut q: Option<char> = None;
            let mut j = pos + 1;
            while j < bytes.len() {
                let c = bytes[j];
                match q {
                    Some(x) if c == x => q = None,
                    Some(_) => {}
                    None if c == '"' || c == '\'' => q = Some(c),
                    None if c == '>' => break,
                    None => {}
                }
                j += 1;
            }
            if j >= bytes.len() {
                break;
            }
            let roh: String = bytes[pos + 1..j].iter().collect();
            pos = j + 1;
            text_start = pos;
            let roh = roh.trim().to_string();
            if let Some(name) = roh.strip_prefix('/') {
                // Schliesstag: bis zum passenden oeffnenden zurueck.
                let name = name.trim().to_ascii_lowercase();
                if let Some(stelle) = offen.iter().rposition(|(n, _)| *n == name) {
                    let (_, k) = offen.remove(stelle);
                    offen.truncate(stelle);
                    let _ = k;
                }
                continue;
            }
            let selbst = roh.ends_with('/');
            let roh = roh.trim_end_matches('/').trim().to_string();
            let (name, attribute) = match roh.find(char::is_whitespace) {
                Some(i) => (roh[..i].to_ascii_lowercase(), roh[i..].to_string()),
                None => (roh.to_ascii_lowercase(), String::new()),
            };
            if name.is_empty() {
                continue;
            }
            let kn = self.dom_element(&name)?;
            self.dom_setze_attribute_text(kn, &attribute)?;
            let ziel = offen.last().map(|(_, k)| *k).unwrap_or(eltern);
            self.dom_anhaengen(ziel, kn)?;
            if !(selbst || ist_void(&name)) && !ist_roh(&name) {
                // GEMESSEN (J1): der Parser laeuft linear (kein Rekursion),
                // deshalb musste die Grenze an der **Stacktiefe** geprueft
                // werden — vorher kamen 40 verschachtelte `div` bei einer
                // Grenze von 16 durch.
                if offen.len() as u32 >= self.grenzen.max_dom_tiefe.min(TIEFE_HART) {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("Error", "innerHTML zu tief (Grenze erreicht)"),
                    ));
                }
                offen.push((name, kn));
            }
        }
        if text_start < bytes.len() {
            let t: String = bytes[text_start..].iter().collect();
            if !t.trim().is_empty() {
                let kn = self.dom_text(&t)?;
                let ziel = offen.last().map(|(_, k)| *k).unwrap_or(eltern);
                self.dom_anhaengen(ziel, kn)?;
            }
        }
        Ok(())
    }

    /// Attributliste aus Quelltext (`id="x" class='y' n=z`).
    fn dom_setze_attribute_text(&mut self, el: GVal, text: &str) -> Result<(), Stoerung> {
        let z: Vec<char> = text.chars().collect();
        let mut i = 0usize;
        while i < z.len() {
            while i < z.len() && z[i].is_whitespace() {
                i += 1;
            }
            let start = i;
            while i < z.len() && !z[i].is_whitespace() && z[i] != '=' {
                i += 1;
            }
            if start == i {
                break;
            }
            let name: String = z[start..i].iter().collect::<String>().to_ascii_lowercase();
            while i < z.len() && z[i].is_whitespace() {
                i += 1;
            }
            let mut wert = String::new();
            if i < z.len() && z[i] == '=' {
                i += 1;
                while i < z.len() && z[i].is_whitespace() {
                    i += 1;
                }
                if i < z.len() && (z[i] == '"' || z[i] == '\'') {
                    let q = z[i];
                    i += 1;
                    while i < z.len() && z[i] != q {
                        wert.push(z[i]);
                        i += 1;
                    }
                    i += 1; // schliessendes
                } else {
                    while i < z.len() && !z[i].is_whitespace() {
                        wert.push(z[i]);
                        i += 1;
                    }
                }
            }
            if name.is_empty() {
                break;
            }
            match name.as_str() {
                "id" => {
                    if let GVal::Ref(r) = el {
                        let v = self.text_wert(&wert).map_err(Stoerung::Fehler)?;
                        self.versteckt_setzen(r, K_ID, v)?;
                    }
                }
                "class" => {
                    if let GVal::Ref(r) = el {
                        let v = self.text_wert(&wert).map_err(Stoerung::Fehler)?;
                        self.versteckt_setzen(r, K_KLASSE, v)?;
                    }
                }
                "style" => {
                    if let GVal::Ref(r) = el {
                        let v = self.text_wert(&wert).map_err(Stoerung::Fehler)?;
                        self.versteckt_setzen(r, K_STIL, v)?;
                    }
                }
                _ => {
                    let v = self.text_wert(&wert).map_err(Stoerung::Fehler)?;
                    self.dom_attribute_setzen_mit(el, &name, v)?;
                }
            }
        }
        Ok(())
    }

    /// Wie `setAttribute`, aber mit fertigem `GVal` (aus dem Parser).
    fn dom_attribute_setzen_mit(
        &mut self,
        el: GVal,
        name: &str,
        wert: GVal,
    ) -> Result<(), Stoerung> {
        let GVal::Ref(r) = el else {
            return Ok(());
        };
        if self.dom_versteckt_lese(r, "__attribute").is_none() {
            let m = self.neues_objekt(None).map_err(Stoerung::Fehler)?;
            self.versteckt_setzen(r, "__attribute", GVal::Ref(m))?;
        }
        let m = self
            .dom_versteckt_lese(r, "__attribute")
            .unwrap_or(GVal::Undefiniert);
        self.setze(m, name, wert)?;
        Ok(())
    }

    // -----------------------------------------------------------------
    // Suchen
    // -----------------------------------------------------------------

    /// `document.getElementById(id)`.
    pub(crate) fn dom_get_element_by_id(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let id = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let Some(doc) = self.dom_dokument else {
            return Ok(GVal::Null);
        };
        // Suche ohne Closure (die brauchte `&mut self` zweimal).
        let mut gefunden: Option<GVal> = None;
        let mut stapel: Vec<(HeapRef, u32)> = vec![(doc, 0)];
        let mut reste = self.grenzen.max_dom_tiefe.min(TIEFE_HART) + 1;
        while let Some((r, _)) = stapel.pop() {
            if gefunden.is_some() {
                break;
            }
            for k in self.dom_kinder_von(r) {
                let GVal::Ref(kr) = k else { continue };
                if self
                    .dom_versteckt_lese(kr, K_ID)
                    .map(|v| self.zu_text(v).unwrap_or_default())
                    .unwrap_or_default()
                    == id
                {
                    gefunden = Some(GVal::Ref(kr));
                    break;
                }
                stapel.push((kr, 0));
            }
            reste = reste.saturating_sub(1);
            if reste == 0 {
                break;
            }
        }
        Ok(gefunden.unwrap_or(GVal::Null))
    }

    /// `el.querySelector(selektor)` — der erste Treffer in Dokumentordnung.
    pub(crate) fn dom_query_selector(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let sel = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let teile = zerlege(&sel);
        let GVal::Ref(wurzel) = this else {
            return Ok(GVal::Null);
        };
        let treffer = self.dom_suche_selektor(wurzel, &teile, 0);
        Ok(treffer.unwrap_or(GVal::Null))
    }

    /// `el.querySelectorAll(selektor)` — alle Treffer als Feld.

    /// J-AR: `document.cookie` lesen.
    ///
    /// GEMESSEN/ANLASS (ar.wikipedia.org, `inline0.js` Zeile 1):
    /// ```js
    /// var cookie = document.cookie.match(/(?:^|; )arwikimwclientpreferences=([^;]+)/);
    /// ```
    /// `document.cookie` war **undefined**, das Skript endete mit
    /// „kein Zugriff auf Undefiniert".
    ///
    /// DER MOTOR HAELT KEINEN COOKIE-SPEICHER: der gehoert `narsil-net`
    /// (der Host liefert die Antwort). Der Getter liefert deshalb einen
    /// **leeren** Text — das ist die ehrliche Antwort „keine Cookies", und
    /// es ist ein Text (`.match` findet daran nichts und `if (cookie)`
    /// ist falsch, genau wie ohne Cookies im Browser). Der Setzer nimmt
    /// den Wert **an** und speichert nichts; wer Cookies braucht, liest
    /// sie ueber den Host.
    ///
    /// Grenze (M3): `MAX_COOKIE_LAENGE` — ein `document.cookie = …` mit
    /// 10 MB Text ist ein Fehler, keine Speicherung.
    pub(crate) fn dom_cookie_lesen(&mut self, _this: GVal) -> Result<GVal, Stoerung> {
        self.text_wert("").map_err(Stoerung::Fehler)
    }

    /// J-AR: `document.cookie = „a=b"` (angenommen, nicht gespeichert —
    /// siehe `dom_cookie_lesen`).
    pub(crate) fn dom_cookie_setzen(
        &mut self,
        _this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let t = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        if t.len() > MAX_COOKIE_LAENGE {
            // GEMESSEN: der Motor nennt fuer **Grenzen** `RangeError`
            // (das entspricht dem JS-Aufrufmuster: `new Array(-1)` ist ein
            // RangeError) — die erste Fassung sagte `Error`.
            return Err(Stoerung::Wurf(self.fehler_wert(
                "RangeError",
                &format!("cookie zu lang (Grenze {MAX_COOKIE_LAENGE} Bytes)"),
            )));
        }
        Ok(GVal::Undefiniert)
    }

    /// J-HN (Nachtrag der Koordination): `el.matches(selektor)`.
    ///
    /// DOM-Spec 4.2.4.4: true, wenn der Knoten den Selektor **selbst**
    /// erfuellt (nicht die Kinder, nicht die Eltern — das ist `closest`).
    /// Wir nutzen den Selektorweg mit `ab = 0` und einem einzigen Teil:
    /// `dom_sammle_selektor` laeuft ab `wurzel` in die Tiefe; fuer
    /// `matches` darf nur der Wurzelknoten selbst zaehlen.
    pub(crate) fn dom_matches(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let sel = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        if sel.trim().is_empty() {
            // DOM-Spec: `matches('')` ist ein SyntaxError.
            return Err(Stoerung::Wurf(
                self.fehler_wert("SyntaxError", "matches: leerer Selektor"),
            ));
        }
        let teile = zerlege(&sel);
        let GVal::Ref(wurzel) = this else {
            return Ok(GVal::Bool(false));
        };
        let mut treffer: Vec<GVal> = Vec::new();
        self.dom_sammle_selektor_selbst(wurzel, &teile, &mut treffer);
        Ok(GVal::Bool(!treffer.is_empty()))
    }

    /// J-HN (Nachtrag): `el.closest(selektor)` — der naechste Vorfahr
    /// **einschliesslich** `el` selbst, der den Selektor erfuellt.
    ///
    /// GEMESSEN (node/jsdom kann das nicht, also die DOM-Spec 4.2.6.2):
    /// die Kette laeuft von `el` nach oben ueber `parentNode`; der erste
    /// Treffer gewinnt. Ohne Treffer `null` — nicht `false`, nicht
    /// `undefined`.
    pub(crate) fn dom_closest(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let sel = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        if sel.trim().is_empty() {
            return Err(Stoerung::Wurf(
                self.fehler_wert("SyntaxError", "closest: leerer Selektor"),
            ));
        }
        let teile = zerlege(&sel);
        let mut k: GVal = this;
        // M3: die Kette ist durch die DOM-Tiefe begrenzt (64), aber ein
        // zyklischer Baum (der Motor laesst ihn nicht zu) wuerde sonst
        // haengen — die Schleife hat deshalb eine eigene Grenze.
        for _ in 0..1024 {
            let GVal::Ref(r) = k else {
                return Ok(GVal::Null);
            };
            let mut treffer: Vec<GVal> = Vec::new();
            self.dom_sammle_selektor_selbst(r, &teile, &mut treffer);
            if !treffer.is_empty() {
                return Ok(k);
            }
            k = match self.dom_eltern(k) {
                Ok(v) => v,
                Err(_) => return Ok(GVal::Null),
            };
            if matches!(k, GVal::Null | GVal::Undefiniert) {
                return Ok(GVal::Null);
            }
        }
        Ok(GVal::Null)
    }

    /// J-HN (Nachtrag): `el.getElementsByName(name)` — nach dem
    /// **`name`-Attribut**, nicht nach dem Tag (DOM-Spec 4.2.5.2).
    pub(crate) fn dom_get_elements_by_name(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let name = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let feld = self.neues_feld().map_err(Stoerung::Fehler)?;
        let GVal::Ref(wurzel) = this else {
            return Ok(GVal::Ref(feld));
        };
        self.wurzeln.push(GVal::Ref(feld));
        let mut alle: Vec<GVal> = Vec::new();
        self.dom_sammle_nach_attribut(wurzel, "name", &name, &mut alle);
        for v in alle.into_iter() {
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            let o = self.heap.holen_mut(feld);
            if let HObj::Feld { elemente, .. } = o {
                elemente.push(v);
            }
        }
        Ok(GVal::Ref(feld))
    }

    /// Sammelt die Knoten mit `attribut == wert` (unterhalb von `wurzel`,
    /// **inklusive** `wurzel` selbst).
    fn dom_sammle_nach_attribut(
        &mut self,
        wurzel: HeapRef,
        attribut: &str,
        wert: &str,
        raus: &mut Vec<GVal>,
    ) {
        if raus.len() >= crate::ops::MAX_SAMMLUNG {
            return;
        }
        // Das Attribut ueber denselben Weg lesen wie `getAttribute`.
        if let Some(GVal::Ref(v)) = self.dom_attribute_suche(wurzel, attribut)
            && let crate::heap::HObj::Zeichenkette(t) = self.heap.holen(v)
            && t == wert
        {
            raus.push(GVal::Ref(wurzel));
        }
        let kinder = match self.dom_kinder_ref(wurzel) {
            Ok(k) => k,
            Err(_) => return,
        };
        let HObj::Feld { elemente, .. } = self.heap.holen(kinder).clone() else {
            return;
        };
        for k in elemente.iter() {
            if let GVal::Ref(r) = k {
                let r = *r;
                self.dom_sammle_nach_attribut(r, attribut, wert, raus);
            }
        }
    }

    /// Sammelt **nur** `wurzel`, wenn er den Selektor erfuellt (fuer
    /// `matches` und `closest`).
    fn dom_sammle_selektor_selbst(
        &mut self,
        wurzel: HeapRef,
        teile: &[String],
        raus: &mut Vec<GVal>,
    ) {
        if teile.is_empty() {
            return;
        }
        // GEMESSEN (Bytecode-Lesen): `dom_passt` ist der Selektor-Teil-
        // gegen-Knoten-Vergleich, den `querySelector` benutzt. `matches`
        // und `closest` brauchen **alle** Teile (UND-Verknuepfung), nicht
        // nur den ersten.
        let treffer = teile.iter().all(|t| self.dom_passt(wurzel, t));
        if treffer {
            raus.push(GVal::Ref(wurzel));
        }
    }

    /// J-HN: `el.getElementsByClassName(klassen)` (DOM-Spec 4.2.8.2).
    ///
    /// GEMESSEN/ANLASS (news.ycombinator.com, `hn.js` Zeile 2):
    /// ```js
    /// function byClass (el, cl) { return el ? el.getElementsByClassName(cl) : [] }
    /// ```
    /// Der Aufruf endete mit „getElementsByClassName is not a function" —
    /// das war der Statuszeilen-Fehler der Seite.
    ///
    /// Die DOM-Spec sagt: es ist `querySelectorAll` mit jedem Namen als
    /// `.klassenname`-Selektor, und der Selektor darf **kein Leerzeichen**
    /// enthalten (es sind **mehrere** Namen). Wir prüfen beides und
    /// benutzen sonst den Selektorweg (`dom_sammle_selektor`), damit die
    /// Reihenfolge (Dokumentordnung) und die Grenzen (`MAX_SAMMLUNG`) die
    /// **gleichen** sind wie bei `querySelectorAll`.
    pub(crate) fn dom_get_elements_by_class_name(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let namen = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        // DOM-Spec: `getElementsByClassName('')` liefert `undefined` als
        // Argument -> DOMException. Der Motor meldet TypeError (die
        // naechste aehnliche Art) — die Meldung nennt die Ursache.
        if namen.trim().is_empty() {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                "getElementsByClassName: leere Klassenliste (DOM-Spec: SyntaxError)",
            )));
        }
        // Leerzeichen = zwei Namen = laut Spec ein SyntaxError. Wir
        // nehmen beide (der Browser wuerf; ehrlicher ist hier die
        // Arbeit: HN ruft mit einem Namen auf).
        let selektor: String = namen
            .split_whitespace()
            .filter(|k| !k.is_empty())
            .map(|k| format!(".{k}"))
            .collect::<Vec<String>>()
            .join(".");
        // `.` allein (Klassenname mit Sonderzeichen) faellt auf den
        // Attributpfad zurueck: `class="a:b"` -> `class~="a:b"`.
        let teile = zerlege(&selektor);
        let feld = self.neues_feld().map_err(Stoerung::Fehler)?;
        let GVal::Ref(wurzel) = this else {
            return Ok(GVal::Ref(feld));
        };
        self.wurzeln.push(GVal::Ref(feld));
        let mut alle: Vec<GVal> = Vec::new();
        self.dom_sammle_selektor(wurzel, &teile, 0, &mut alle);
        for v in alle.into_iter() {
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            let o = self.heap.holen_mut(feld);
            if let HObj::Feld { elemente, .. } = o {
                elemente.push(v);
            }
        }
        Ok(GVal::Ref(feld))
    }

    pub(crate) fn dom_query_selector_all(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let sel = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let teile = zerlege(&sel);
        let feld = self.neues_feld().map_err(Stoerung::Fehler)?;
        let GVal::Ref(wurzel) = this else {
            return Ok(GVal::Ref(feld));
        };
        self.wurzeln.push(GVal::Ref(feld));
        let mut alle: Vec<GVal> = Vec::new();
        self.dom_sammle_selektor(wurzel, &teile, 0, &mut alle);
        for v in alle.into_iter() {
            // M3: `MAX_SAMMLUNG` Treffer (oben geschnitten).
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            let o = self.heap.holen_mut(feld);
            if let HObj::Feld { elemente, .. } = o {
                elemente.push(v);
            }
        }
        Ok(GVal::Ref(feld))
    }

    /// Der erste passende Knoten (Tiefensuche in Dokumentordnung).
    fn dom_suche_selektor(&mut self, wurzel: HeapRef, teile: &[String], ab: usize) -> Option<GVal> {
        if ab >= teile.len() {
            return None;
        }
        for k in self.dom_kinder_von(wurzel) {
            let GVal::Ref(kr) = k else { continue };
            if self.dom_typ(kr) != TYP_ELEMENT {
                continue;
            }
            if self.dom_passt(kr, &teile[ab]) {
                if ab + 1 == teile.len() {
                    return Some(GVal::Ref(kr));
                }
                if let Some(t) = self.dom_suche_selektor(kr, teile, ab + 1) {
                    return Some(t);
                }
            }
            // Nachfahren mit demselben Selektor (Nachfahr-Kombinator).
            if ab == 0 {
                if let Some(t) = self.dom_suche_selektor(kr, teile, ab) {
                    return Some(t);
                }
            }
        }
        None
    }

    fn dom_sammle_selektor(
        &mut self,
        wurzel: HeapRef,
        teile: &[String],
        ab: usize,
        raus: &mut Vec<GVal>,
    ) {
        if ab >= teile.len() {
            return;
        }
        for k in self.dom_kinder_von(wurzel) {
            let GVal::Ref(kr) = k else { continue };
            if self.dom_typ(kr) != TYP_ELEMENT {
                continue;
            }
            if self.dom_passt(kr, &teile[ab]) {
                raus.push(GVal::Ref(kr));
                if ab + 1 < teile.len() {
                    self.dom_sammle_selektor(kr, teile, ab + 1, raus);
                }
            }
            self.dom_sammle_selektor(kr, teile, ab, raus);
        }
    }

    /// Passt ein Knoten auf EINEN Selektor-Teil (`div`, `#id`, `.k`, `[a]`)?
    fn dom_passt(&mut self, r: HeapRef, teil: &str) -> bool {
        let t = teil.trim();
        if t.is_empty() {
            return false;
        }
        if t == "*" {
            return true;
        }
        if let Some(id) = t.strip_prefix('#') {
            return self
                .dom_versteckt_lese(r, K_ID)
                .map(|v| self.zu_text(v).unwrap_or_default())
                .unwrap_or_default()
                == id;
        }
        if let Some(k) = t.strip_prefix('.') {
            return self.dom_klassen(r).iter().any(|x| x == k);
        }
        if let Some(rest) = t.strip_prefix('[') {
            let rest = rest.strip_suffix(']').unwrap_or(rest);
            let name = rest.split('=').next().unwrap_or(rest).trim();
            if name == "*" {
                return !self.dom_attribute_liste(r).is_empty();
            }
            return self
                .dom_attribute_suche(r, &name.to_ascii_lowercase())
                .is_some();
        }
        // Tag-Name (mit Namensraum `ns:tag`).
        let name = self
            .dom_versteckt_lese(r, K_NAME)
            .map(|v| self.zu_text(v).unwrap_or_default())
            .unwrap_or_default();
        name.eq_ignore_ascii_case(t)
    }

    // -----------------------------------------------------------------
    // Helfer (Knotenfelder lesen)
    // -----------------------------------------------------------------

    /// Ein verstecktes Feld (`__…`) lesen; `None`, wenn es fehlt.
    ///
    /// `netz.rs` hat dieselbeOperation fuer `GVal` (`versteckt_lesen`).
    pub(crate) fn dom_versteckt_lese(&self, r: HeapRef, name: &str) -> Option<GVal> {
        self.eigene_lese(r, name)
    }

    /// `nodeType`.
    pub(crate) fn dom_typ(&self, r: HeapRef) -> f64 {
        match self.dom_versteckt_lese(r, K_TYP) {
            Some(GVal::Zahl(n)) => n,
            _ => 0.0,
        }
    }

    /// Der Text eines Textknotens (`__text`).
    pub(crate) fn dom_text_von(&self, r: HeapRef) -> Option<GVal> {
        self.dom_versteckt_lese(r, K_TEXT)
    }

    /// Der Elternknoten als `HeapRef` (Dokument = kein Elternteil).
    pub(crate) fn dom_eltern_ref(&self, r: HeapRef) -> Option<HeapRef> {
        match self.dom_versteckt_lese(r, K_ELTERN) {
            Some(GVal::Ref(e)) => Some(e),
            _ => None,
        }
    }

    /// Die Kinderliste (das `__kinder`-Feld als `HeapRef`).
    fn dom_kinder_ref(&mut self, r: HeapRef) -> Result<HeapRef, Stoerung> {
        match self.dom_versteckt_lese(r, K_KINDER) {
            Some(GVal::Ref(f)) => Ok(f),
            _ => {
                let f = self.neues_feld().map_err(Stoerung::Fehler)?;
                self.versteckt_setzen(r, K_KINDER, GVal::Ref(f))?;
                Ok(f)
            }
        }
    }

    /// Die Kinder als Liste von Werten (fuer `childNodes`, `children`).
    pub(crate) fn dom_kinder_von(&mut self, r: HeapRef) -> Vec<GVal> {
        match self.dom_versteckt_lese(r, K_KINDER) {
            Some(GVal::Ref(f)) => match self.heap.holen(f) {
                HObj::Feld { elemente, .. } => elemente.clone(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// `childNodes` als Feld.
    pub(crate) fn dom_child_nodes(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let feld = self.neues_feld().map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(feld));
        let GVal::Ref(r) = this else {
            return Ok(GVal::Ref(feld));
        };
        for k in self.dom_kinder_von(r) {
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            let o = self.heap.holen_mut(feld);
            if let HObj::Feld { elemente, .. } = o {
                elemente.push(k);
            }
        }
        Ok(GVal::Ref(feld))
    }

    /// `children` als Feld (nur Elemente).
    pub(crate) fn dom_children(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let feld = self.neues_feld().map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(feld));
        let GVal::Ref(r) = this else {
            return Ok(GVal::Ref(feld));
        };
        for k in self.dom_kinder_von(r) {
            let GVal::Ref(kr) = k else { continue };
            if self.dom_typ(kr) != TYP_ELEMENT {
                continue;
            }
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
            let o = self.heap.holen_mut(feld);
            if let HObj::Feld { elemente, .. } = o {
                elemente.push(k);
            }
        }
        Ok(GVal::Ref(feld))
    }

    /// `firstChild` / `lastChild`.
    pub(crate) fn dom_erstes_letztes(
        &mut self,
        this: GVal,
        letzte: bool,
    ) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Null);
        };
        let kinder = self.dom_kinder_von(r);
        Ok(if letzte {
            kinder.last().copied().unwrap_or(GVal::Null)
        } else {
            kinder.first().copied().unwrap_or(GVal::Null)
        })
    }

    /// `nodeName`/`tagName`.
    pub(crate) fn dom_name_von(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let t = self.dom_typ(r);
        let name = self
            .dom_versteckt_lese(r, K_NAME)
            .map(|v| self.zu_text(v).unwrap_or_default())
            .unwrap_or_default();
        // DOM-Spec: `tagName` gibt es nur bei Elementen (gross geschrieben).
        let html = match self.eigene_lese(r, "namespaceURI") {
            None => true,
            Some(value) => self.zu_text(value).ok().as_deref() == Some("http://www.w3.org/1999/xhtml"),
        };
        if t == TYP_ELEMENT && html {
            return Ok(self
                .text_wert(&name.to_ascii_uppercase())
                .map_err(Stoerung::Fehler)?);
        }
        Ok(self.text_wert(&name).map_err(Stoerung::Fehler)?)
    }

    /// `id`/`className`: lesen und schreiben.
    pub(crate) fn dom_id_class(
        &mut self,
        this: GVal,
        args: &[GVal],
        klasse: bool,
        schreiben: bool,
    ) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(if schreiben {
                GVal::Undefiniert
            } else {
                GVal::Undefiniert
            });
        };
        let feld = if klasse { K_KLASSE } else { K_ID };
        if !schreiben {
            let v = self
                .dom_versteckt_lese(r, feld)
                .unwrap_or(GVal::Undefiniert);
            return Ok(v);
        }
        let w = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let v = self.text_wert(&w).map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(r, feld, v)?;
        Ok(GVal::Undefiniert)
    }

    /// `style` als Objekt mit Getter/Setter je Angabe.
    ///
    /// GEMESSEN/ENTSCHIEDEN: statt eines echten `Proxy` bekommt das Skript
    /// ein Objekt mit einer vorab gebauten Auswahl gangbarer Eigenschaften
    /// (`color`, `background`, `display`, `width`, `height`, …) plus
    /// `setProperty`/`getPropertyValue`/`removeProperty`.
    pub(crate) fn dom_stil_objekt(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let stil = self
            .neues_objekt(Some(self.proto_dom_stil))
            .map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(stil));
        self.versteckt_setzen(stil, "__ziel", this)?;
        // M3: Die Liste der vorab gebauten Eigenschaften ist fest (14).
        const EIGEN: [&str; 14] = [
            "color",
            "background",
            "display",
            "width",
            "height",
            "margin",
            "padding",
            "font",
            "fontSize",
            "position",
            "top",
            "left",
            "zIndex",
            "visibility",
        ];
        for e in EIGEN {
            let arg = self.text_wert(e).map_err(Stoerung::Fehler)?;
            let w = self.dom_stil_lesen(this, &[arg])?;
            self.setze_eigen(stil, e, w);
        }
        Ok(GVal::Ref(stil))
    }
}

/// `HeapRef` eines Werts, sonst `None` (die Aufrufer behandeln das selbst).
fn kind_ref(v: GVal) -> Option<HeapRef> {
    match v {
        GVal::Ref(r) => Some(r),
        _ => None,
    }
}

/// `start_muster` im Zeichenfeld suchen (fuer `<!-- -->`).
fn find(z: &[char], ab: usize, muster: &str) -> Option<usize> {
    let m: Vec<char> = muster.chars().collect();
    let mut i = ab;
    while i + m.len() <= z.len() {
        if z[i..i + m.len()] == m[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Den Selektor in Nachfahr-Teile zerlegen (`div p` -> ["div", "p"]).
/// Mehrere durch Komma getrennte Selektoren werden nicht unterstuetzt
/// (dokumentierte Naeherung: der erste gewinnt).
pub(crate) fn zerlege(sel: &str) -> Vec<String> {
    let erst = sel.split(',').next().unwrap_or(sel);
    erst.split_whitespace()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Nur fuer Berichte/Diagnose: die Grenzen als Text.
pub fn grenzen_text(g: &Grenzen) -> String {
    format!(
        "max_dom_knoten = {}, max_dom_tiefe = {}, max_dom_html = {}",
        g.max_dom_knoten, g.max_dom_tiefe, g.max_dom_html
    )
}
impl Engine {
    /// Eine Methode auf einem Prototyp anlegen — **nicht aufzaehlbar**.
    ///
    /// GEMESSEN (J1): mit `setze_eigen` (aufzaehlbar) landeten die
    /// DOM-Methoden in `Object.keys(el)` — der Test
    /// `knotenfelder_sind_versteckt` hat das gefunden.
    fn dom_methode(&mut self, ziel: HeapRef, name: &str, f: HeapRef) {
        let o = self.heap.holen_mut(ziel);
        let map = match o {
            HObj::Gegenstand { eigenschaften, .. } => Some(eigenschaften),
            _ => None,
        };
        if let Some(eigenschaften) = map {
            eigenschaften.insert(
                name.to_string(),
                crate::heap::Eigenschaft {
                    konfigurierbar: true,
                    wert: GVal::Ref(f),
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: true,
                },
            );
        }
    }

    /// Den DOM-Baum als `Wert` herausgeben — **die Bruecke fuer `narsil`**
    /// (J2). `Wert::Gegenstand` mit `nodeType`, `tagName`, `textContent`
    /// und `children` (Textknoten: `Wert::Text`).
    ///
    /// GEMESSEN/Begruendung: `GVal` ist crate-privat, der Host kann den
    /// Knoten nicht sehen. Die Bruecke ist deshalb ein **Export**: der Host
    /// bekommt einen `Wert`-Baum und kann ihn rendern; Rueckgaben laufen
    /// ueber `innerHTML`/`dom_importieren`.
    pub(crate) fn dom_export(&mut self, start: GVal) -> Option<Wert> {
        self.dom_export_in(start, 0)
    }

    fn dom_export_in(&mut self, v: GVal, tiefe: u32) -> Option<Wert> {
        if tiefe > self.grenzen.max_dom_tiefe.min(TIEFE_HART) {
            return None;
        }
        let GVal::Ref(r) = v else {
            return None;
        };
        let typ = self.dom_typ(r);
        if typ == TYP_TEXT {
            let t = self
                .dom_versteckt_lese(r, K_TEXT)
                .map(|x| self.zu_text(x).unwrap_or_default())
                .unwrap_or_default();
            return Some(Wert::Text(t));
        }
        let tag = self
            .dom_versteckt_lese(r, K_NAME)
            .map(|x| self.zu_text(x).unwrap_or_default())
            .unwrap_or_default();
        // DOM-Spec 4.3: `tagName` ist GROSS — der Host (und `narsil`)
        // vergleichen Tag-Namen in dieser Schreibweise.
        let tag = if typ == TYP_ELEMENT {
            tag.to_ascii_uppercase()
        } else {
            tag
        };
        let kinder = self.dom_kinder_von(r);
        let mut felder: Vec<(String, Wert)> = vec![
            ("nodeType".to_string(), Wert::Zahl(typ)),
            ("tagName".to_string(), Wert::Text(tag.clone())),
            // J2: die Nummer, ueber die der Host das Ereignis zustellt
            // (`dom_ereignis_senden`).
            (
                "__id".to_string(),
                Wert::Zahl(self.dom_knoten_id(r).unwrap_or(0) as f64),
            ),
        ];
        // J2/J6: die `id` mitgeben, wenn der Knoten eine hat. Der Host
        // (narsil) sucht den Knoten eines Klicks ueber die `id` — ohne sie
        // bleibt nur der Tag-Name, und der ist nicht eindeutig (zwei
        // „OK"-Knoepfe einer Cookie-Leiste). GEMESSEN, warum es hier
        // fehlte: `id` war nur ein Getter/Setter auf dem Prototyp
        // (`dom_getter_setter(…, "id", …)`), der Export schrieb das Feld
        // nicht mit — `narsil/src/jsvm.rs::id_von_id` lieferte darum
        // `None` (der Test dort sagt es laut: „die id-Suche kann es jetzt
        // doch").
        //
        // Nur wenn eine `id` da ist: ein leeres Feld wuerde der Host als
        // Treffer zaehlen (sein `id_von_id("")` waere sonst ein Knoten).
        if typ == TYP_ELEMENT {
            // Export live attributes so mutations, deletion and moved siblings
            // retain their own state; positional matching against old HTML is unsafe.
            let attribute = self.dom_attribute_liste(r).into_iter()
                .map(|(name, value)| (name, Wert::Text(value))).collect();
            felder.push(("attributes".to_string(), Wert::Gegenstand(attribute)));
            let id = self
                .dom_versteckt_lese(r, K_ID)
                .map(|x| self.zu_text(x).unwrap_or_default())
                .unwrap_or_default();
            if !id.is_empty() {
                felder.push(("id".to_string(), Wert::Text(id)));
            }
        }
        // M3: Der Export ist durch `max_dom_knoten` und die Tiefe begrenzt.
        if kinder.is_empty() {
            felder.push((
                "textContent".to_string(),
                Wert::Text(
                    self.dom_versteckt_lese(r, K_TEXT)
                        .map(|x| self.zu_text(x).unwrap_or_default())
                        .unwrap_or_default(),
                ),
            ));
            return Some(Wert::Gegenstand(felder));
        }
        let mut liste: Vec<Wert> = Vec::new();
        for k in kinder {
            if let Some(w) = self.dom_export_in(k, tiefe + 1) {
                liste.push(w);
            }
        }
        let feld = Wert::Feld(liste);
        felder.push(("children".to_string(), feld));
        let tv = self.dom_text_lese(v).unwrap_or(GVal::Undefiniert);
        let ts = self.zu_text(tv).unwrap_or_default();
        felder.push(("textContent".to_string(), Wert::Text(ts)));
        Some(Wert::Gegenstand(felder))
    }

    /// Getter und Setzer einer Eigenschaft auf einem Prototyp anlegen
    /// (DOM-Spec: `id`, `className`, `textContent`, `innerHTML` …).
    pub(crate) fn dom_getter_setter(
        &mut self,
        ziel: HeapRef,
        name: &str,
        lesen: Nativ,
        schreiben: Nativ,
    ) {
        let f = self.nativ_anlegen(name, lesen, self.proto_funktion);
        let s = self.nativ_anlegen(name, schreiben, self.proto_funktion);
        let o = self.heap.holen_mut(ziel);
        let map = match o {
            HObj::Gegenstand { eigenschaften, .. } => Some(eigenschaften),
            _ => None,
        };
        if let Some(eigenschaften) = map {
            eigenschaften.insert(
                name.to_string(),
                crate::heap::Eigenschaft {
                    wert: GVal::Undefiniert,
                    getter: Some(f),
                    setter: Some(s),
                    aufzählbar: false,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
        }
    }

    /// Die Nummer des Knotens (`__knoten`), wenn er welche hat.
    pub(crate) fn dom_knoten_id(&self, r: HeapRef) -> Option<u32> {
        match self.dom_versteckt_lese(r, K_KNOTEN) {
            Some(GVal::Zahl(n)) if n >= 0.0 => Some(n as u32),
            _ => None,
        }
    }

    /// Die naechste Knotennummer vergeben (Zaehler je Lauf, M4: gedeckelt
    /// durch `max_dom_knoten`).
    fn dom_nummer(&mut self) -> Result<u32, Stoerung> {
        let n = self.dom_naechste_knoten;
        self.dom_naechste_knoten = self.dom_naechste_knoten.saturating_add(1);
        Ok(n)
    }

    /// Einen Knoten anhand seiner Nummer suchen (J2).
    ///
    /// GEMESSEN/Begruendung: eine Tabelle `Nummer -> HeapRef` waere schneller,
    /// aber eine `HeapRef` zeigt in den Heap, und nach einer Sammlung kann
    /// die Nummer fuer ein ANDERES Objekt stehen. Die Suche laeuft ueber den
    /// Baum — die Nummer steht im Knoten selbst, ein fremdes Objekt kann sie
    /// also nicht vortaeuschen.
    pub(crate) fn dom_knoten_suchen(&mut self, nummer: u32) -> Option<GVal> {
        let doc = self.dom_dokument?;
        let mut stapel: Vec<HeapRef> = vec![doc];
        // M3: Die Suche ist durch die Baumtiefe und die Knotenzahl begrenzt.
        let mut reste = self.grenzen.max_dom_tiefe.min(TIEFE_HART) as usize
            * self
                .grenzen
                .max_dom_knoten
                .min(MAX_KNOTEN_HART)
                .min(4096)
                .max(1);
        while let Some(r) = stapel.pop() {
            if reste == 0 {
                break;
            }
            reste -= 1;
            if self.dom_knoten_id(r) == Some(nummer) {
                return Some(GVal::Ref(r));
            }
            for k in self.dom_kinder_von(r) {
                if let GVal::Ref(kr) = k {
                    stapel.push(kr);
                }
            }
        }
        None
    }

    /// Ein Ereignis an einen DOM-Knoten senden (J2 — die Bruecke fuer
    /// `narsil`: ein Klick auf ein Element, das das Skript gebaut hat).
    ///
    /// `nummer` ist die `__id` aus [`Engine::dom_wurzel`]. `true` heisst:
    /// die Defaulthandlung darf stattfinden.
    pub fn dom_ereignis_senden(
        &mut self,
        typ: &str,
        nummer: u32,
        eigenschaften: Vec<(String, Wert)>,
    ) -> Result<bool, String> {
        let ziel = self
            .dom_knoten_suchen(nummer)
            .ok_or_else(|| format!("kein Knoten mit der Nummer {nummer}"))?;
        match self.ereignis_senden_intern_offen(typ, ziel, eigenschaften) {
            Ok(f) => Ok(f),
            Err(e) => Err(self.stoerung_text(e)),
        }
    }

    /// `document` anlegen und global binden (J1).
    ///
    /// GEMESSEN/ENTSCHIEDEN: das Dokument ist ein ganz normales Objekt im
    /// Heap. Die Bruecke fuer `narsil` (J2) ist damit: `dom_wurzel()`
    /// liefert das Objekt, das der Host als Wurzel seines Baums nimmt.
    pub fn dom_aufbauen(&mut self) -> Result<(), String> {
        if self.dom_dokument.is_some() {
            return Ok(());
        }
        let doc = self
            .dom_dokument_anlegen()
            .map_err(|_| "DOM: Dokument nicht anlegbar (Heap-Grenze?)".to_string())?;
        let GVal::Ref(d) = doc else {
            return Err("DOM: Dokument kein Objekt".to_string());
        };
        self.dom_dokument = Some(d);
        // `document` global binden — die Skriptseite sieht genau dieses
        // Objekt, der Host kann daran weiterbauen (`innerHTML` u. a.).
        self.global_binden("document", doc);
        // `window` ist im Browser dasselbe wie der globale Bereich; fuer
        // `window.document` genuegt eine Selbstbindung.
        self.global_binden("window", GVal::Ref(self.global_objekt));
        Ok(())
    }

    /// Das Dokument als **exportierten** `Wert` (die Bruecke fuer `narsil`,
    /// J2) — siehe [`Engine::dom_export`].
    pub fn dom_wurzel(&mut self) -> Option<Wert> {
        let d = self.dom_dokument?;
        self.dom_export(GVal::Ref(d))
    }

    /// Den Text des Dokuments (fuer `narsil`: was auf die Seite soll).
    pub fn dom_text_dokument(&mut self) -> Result<String, String> {
        let Some(d) = self.dom_dokument else {
            return Ok(String::new());
        };
        match self.dom_text_lese(GVal::Ref(d)) {
            Ok(v) => Ok(self.zu_text(v).unwrap_or_default()),
            Err(_) => Ok(String::new()),
        }
    }

    /// `el.style` als Objekt; `removeProperty` braucht einen eigenen Nativ.
    pub(crate) fn dom_stil_entfernen(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.dom_stil_setzen(this, &[self.arg(args, 0), GVal::Undefiniert])
    }

    /// `classList.length`.
    pub(crate) fn dom_klasse_laenge(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let this = self.dom_ziel(this);
        let GVal::Ref(r) = this else {
            return Ok(GVal::Zahl(0.0));
        };
        Ok(GVal::Zahl(self.dom_klassen(r).len() as f64))
    }

    /// `el.textContent` auf einem **Textknoten**.
    pub(crate) fn dom_text_von_wert(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        Ok(self
            .dom_versteckt_lese(r, K_TEXT)
            .unwrap_or(GVal::Undefiniert))
    }

    /// `el.data = w` / `el.nodeValue = w` auf einem Textknoten.
    pub(crate) fn dom_text_von_setzen(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let w = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        if w.len() > self.grenzen.max_string_laenge {
            return Err(Stoerung::Wurf(
                self.fehler_wert("Error", "Text zu lang (Grenze erreicht)"),
            ));
        }
        let v = self.text_wert(&w).map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(r, K_TEXT, v)?;
        Ok(GVal::Undefiniert)
    }

    /// `el.parentNode`.
    pub(crate) fn dom_eltern(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Null);
        };
        Ok(match self.dom_eltern_ref(r) {
            Some(e) => GVal::Ref(e),
            None => GVal::Null,
        })
    }

    /// `nodeType` als Zahl (ohne Getter-Umweg).
    pub(crate) fn dom_typ_als(&mut self, this: GVal) -> f64 {
        match this {
            GVal::Ref(r) => self.dom_typ(r),
            _ => 0.0,
        }
    }

    /// Die Prototypen des DOM befuellen (J1) — aufrufen aus `builtins()`.
    pub(crate) fn dom_prototypen_anlegen(&mut self) {
        // --- Element ---
        for (n, nat) in [
            ("getAttribute", Nativ::DomAttrLesen),
            ("setAttribute", Nativ::DomAttrSchreiben),
            ("removeAttribute", Nativ::DomAttrEntfernen),
            ("hasAttribute", Nativ::DomAttrHat),
            ("appendChild", Nativ::DomAppendChild),
            ("insertAdjacentHTML", Nativ::DomInsertAdjacentHTML),
            ("prepend", Nativ::DomPrepend),
            ("append", Nativ::DomAppend),
            ("before", Nativ::DomBefore),
            ("after", Nativ::DomAfter),
            ("insertBefore", Nativ::DomInsertBefore),
            ("removeChild", Nativ::DomRemoveChild),
            // J6c: `el.remove()` (Spec: `ChildNode.remove`). Ohne diese
            // Zeile war der Aufruf ein TypeError — und ein TypeError im
            // Klick-Handler war fuer den Host unsichtbar (Befund in
            // team/PROBLEME.txt, 12:05).
            ("remove", Nativ::DomEntfernen),
            ("querySelector", Nativ::DomQuerySelector),
            ("querySelectorAll", Nativ::DomQuerySelectorAll),
            ("getElementsByTagName", Nativ::DomQuerySelectorAll),
            // J-HN: `getElementsByClassName` (DOM-Spec 4.2.8.2) — fehlte,
            // `hn.js` von news.ycombinator.com ruft es in Zeile 2 auf.
            ("getElementsByClassName", Nativ::DomNachKlasse),
            // J-HN (Nachtrag der Koordination): die drei, die die Seite
            // ebenfalls benutzt und die der Motor nicht kannte.
            ("getElementsByName", Nativ::DomNachName),
            ("matches", Nativ::DomMatches),
            ("closest", Nativ::DomClosest),
        ] {
            let f = self.nativ_anlegen(n, nat, self.proto_funktion);
            self.dom_methode(self.proto_dom_element, n, f);
        }
        for (n, nat) in [
            ("add", Nativ::DomKlasseAendern),
            ("remove", Nativ::DomKlasseEntfernen),
            ("toggle", Nativ::DomKlasseUmschalten),
            ("contains", Nativ::DomKlasseHat),
        ] {
            let f = self.nativ_anlegen(n, nat, self.proto_funktion);
            self.dom_methode(self.proto_dom_klasse, n, f);
        }
        // `classList.length` (DOM-Spec: Zahl der Klassen).
        self.nativ_getter(self.proto_dom_klasse, "length", Nativ::DomKlasseLaenge);
        for (n, nat) in [
            ("setProperty", Nativ::DomStilSetzen),
            ("getPropertyValue", Nativ::DomStilLesen),
            ("removeProperty", Nativ::DomStilEntfernenNativ),
        ] {
            let f = self.nativ_anlegen(n, nat, self.proto_funktion);
            self.dom_methode(self.proto_dom_stil, n, f);
        }
        // Getter/Setter (DOM-Spec: auf `Node.prototype` bzw. `Element`).
        self.nativ_getter(self.proto_dom_element, "nodeType", Nativ::DomNodeType);
        for name in ["namespaceURI", "localName", "prefix"] {
            self.nativ_getter(self.proto_dom_element, name, Nativ::DomNamespaceLesen(name));
        }
        self.nativ_getter(self.proto_dom_element, "nodeName", Nativ::DomNodeNameKurz);
        self.nativ_getter(self.proto_dom_element, "tagName", Nativ::DomName);
        self.nativ_getter(self.proto_dom_element, "children", Nativ::DomChildren);
        self.nativ_getter(self.proto_dom_element, "childNodes", Nativ::DomChildNodes);
        self.nativ_getter(self.proto_dom_element, "firstChild", Nativ::DomFirstChild);
        self.nativ_getter(self.proto_dom_element, "lastChild", Nativ::DomLastChild);
        self.nativ_getter(self.proto_dom_element, "parentNode", Nativ::DomElternLesen);
        self.nativ_getter(self.proto_dom_element, "style", Nativ::DomStilObjekt);
        self.dom_getter_setter(
            self.proto_dom_element,
            "id",
            Nativ::DomIdLesen,
            Nativ::DomIdSchreiben,
        );
        self.dom_getter_setter(
            self.proto_dom_element,
            "className",
            Nativ::DomClassLesen,
            Nativ::DomClassSchreiben,
        );
        self.dom_getter_setter(
            self.proto_dom_element,
            "textContent",
            Nativ::DomTextLese,
            Nativ::DomTextSchreiben,
        );
        self.dom_getter_setter(
            self.proto_dom_element,
            "innerHTML",
            Nativ::DomInnerHtmlLesen,
            Nativ::DomInnerHtmlSchreiben,
        );
        self.dom_getter_setzer_classliste();
        // --- Textknoten ---
        self.nativ_getter(self.proto_dom_text, "nodeType", Nativ::DomNodeType);
        self.nativ_getter(self.proto_dom_text, "nodeName", Nativ::DomNodeNameKurz);
        self.nativ_getter(self.proto_dom_text, "parentNode", Nativ::DomElternLesen);
        self.dom_getter_setter(
            self.proto_dom_text,
            "textContent",
            Nativ::DomTextKnotenLesen,
            Nativ::DomNodeValueSchreiben,
        );
        self.dom_getter_setter(
            self.proto_dom_text,
            "data",
            Nativ::DomTextKnotenLesen,
            Nativ::DomNodeValueSchreiben,
        );
        self.dom_getter_setter(
            self.proto_dom_text,
            "nodeValue",
            Nativ::DomTextKnotenLesen,
            Nativ::DomNodeValueSchreiben,
        );
        // --- Dokument ---
        for (n, nat) in [
            ("getElementById", Nativ::DomGetElementById),
            ("createElement", Nativ::DomCreateElement),
            ("createElementNS", Nativ::DomCreateElementNS),
            ("createEvent", Nativ::DomCreateEvent),
            ("createTextNode", Nativ::DomCreateText),
            ("querySelector", Nativ::DomQuerySelector),
            ("querySelectorAll", Nativ::DomQuerySelectorAll),
            // J-HN: auch am **Dokument** (DOM-Spec 4.2.8.2 gilt fuer
            // `Document` wie fuer `Element`).
            ("getElementsByClassName", Nativ::DomNachKlasse),
            ("getElementsByTagName", Nativ::DomQuerySelectorAll),
            ("getElementsByName", Nativ::DomNachName),
            // GEMESSEN (DOM-Spec 4.2.6/4.2.4): `matches` und `closest`
            // sind **Methoden am Element**, nicht am Dokument. Ich haenge
            // sie trotzdem nicht ans Dokument: `document.matches` gibt es
            // in keinem Browser.
        ] {
            let f = self.nativ_anlegen(n, nat, self.proto_funktion);
            self.dom_methode(self.proto_dom_dokument, n, f);
        }
        // J-AR: `document.cookie` (ar.wikipedia.org).
        self.nativ_getter(self.proto_dom_dokument, "cookie", Nativ::DomCookieLesen);
        self.nativ_setter(self.proto_dom_dokument, "cookie", Nativ::DomCookieSetzen);
        self.nativ_getter(self.proto_dom_dokument, "nodeType", Nativ::DomNodeType);
        self.nativ_getter(self.proto_dom_dokument, "body", Nativ::DomBody);
        self.nativ_getter(self.proto_dom_dokument, "head", Nativ::DomHead);
        self.nativ_getter(
            self.proto_dom_dokument,
            "documentElement",
            Nativ::DomDocumentElement,
        );
    }

    /// `classList` als Objekt: ein Getter, der bei jedem Zugriff neu baut
    /// (der Knoten kann seine Klassen ja aendern).
    pub(crate) fn dom_getter_setzer_classliste(&mut self) {
        let f = self.nativ_anlegen("classList", Nativ::DomClassListe, self.proto_funktion);
        let o = self.heap.holen_mut(self.proto_dom_element);
        let map = match o {
            HObj::Gegenstand { eigenschaften, .. } => Some(eigenschaften),
            _ => None,
        };
        if let Some(eigenschaften) = map {
            eigenschaften.insert(
                "classList".to_string(),
                crate::heap::Eigenschaft {
                    wert: GVal::Undefiniert,
                    getter: Some(f),
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
        }
    }

    /// Das ELEMENT hinter einem `classList`/`style`-Objekt (`__ziel`);
    /// ist `this` selbst ein Knoten, wird es unveraendert zurueckgegeben.
    pub(crate) fn dom_ziel(&mut self, v: GVal) -> GVal {
        match self.dom_versteckt_lese_wert(v, "__ziel") {
            Some(z) => z,
            None => v,
        }
    }

    /// `__…`-Feld eines Werts lesen (Element oder `classList`-Objekt).
    fn dom_versteckt_lese_wert(&mut self, v: GVal, name: &str) -> Option<GVal> {
        match v {
            GVal::Ref(r) => self.dom_versteckt_lese(r, name),
            _ => None,
        }
    }

    /// `el.classList` (Objekt mit `add`/`remove`/`toggle`/`contains`).
    pub(crate) fn dom_class_liste(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let liste = self
            .neues_objekt(Some(self.proto_dom_klasse))
            .map_err(Stoerung::Fehler)?;
        // GEMESSEN: ein zusaetzliches `__host`-Feld machte aus dem
        // `classList`-Objekt ein HOST-Objekt — jede Eigenschaft wurde dann
        // ueber den (nicht gesetzten) DOM-Haken beantwortet, und
        // `classList.add(...)` endete mit „Aufrufziel ist keine Funktion".
        self.versteckt_setzen(liste, "__ziel", this)?;
        Ok(GVal::Ref(liste))
    }

    /// `classList.remove(...)`.
    pub(crate) fn dom_klasse_entfernen(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.dom_klasse_aendern(this, args, 1)
    }

    /// `classList.toggle(...)`.
    pub(crate) fn dom_klasse_umschalten(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        self.dom_klasse_aendern(this, args, 2)
    }
}

impl Engine {
    /// `document.body` / `document.head` — das Kind mit dem Namen, sonst
    /// `null` (DOM-Spec).
    pub(crate) fn dom_teil_von_dokument(&mut self, this: GVal, name: &str) -> Result<GVal, Stoerung> {
        let GVal::Ref(doc) = this else {
            return Ok(GVal::Null);
        };
        let html = self.dom_kind_von(doc, "html");
        let Some(html) = html else {
            return Ok(GVal::Null);
        };
        for k in self.dom_kinder_von(html) {
            let GVal::Ref(kr) = k else { continue };
            let n = self
                .dom_versteckt_lese(kr, K_NAME)
                .map(|v| self.zu_text(v).unwrap_or_default())
                .unwrap_or_default();
            if n.eq_ignore_ascii_case(name) {
                return Ok(GVal::Ref(kr));
            }
        }
        Ok(GVal::Null)
    }

    /// Das erste Kind mit diesem Tag-Namen.
    fn dom_kind_von(&mut self, wurzel: HeapRef, name: &str) -> Option<HeapRef> {
        for k in self.dom_kinder_von(wurzel) {
            let GVal::Ref(kr) = k else { continue };
            let n = self
                .dom_versteckt_lese(kr, K_NAME)
                .map(|v| self.zu_text(v).unwrap_or_default())
                .unwrap_or_default();
            if n.eq_ignore_ascii_case(name) {
                return Some(kr);
            }
        }
        None
    }

    /// `document.documentElement` — das `<html>`-Element.
    pub(crate) fn dom_document_element(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let GVal::Ref(doc) = this else {
            return Ok(GVal::Null);
        };
        Ok(match self.dom_kind_von(doc, "html") {
            Some(e) => GVal::Ref(e),
            None => GVal::Null,
        })
    }
    pub(crate) fn dom_insert_adjacent_html(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let position = self.zu_text(self.arg(args, 0)).unwrap_or_default().to_ascii_lowercase();
        let html = self.zu_text(self.arg(args, 1)).unwrap_or_default();
        if html.len() > self.grenzen.max_dom_html.min(MAX_HTML_HART) {
            return Err(Stoerung::Wurf(self.fehler_wert("Error", "insertAdjacentHTML zu lang (Grenze erreicht)")));
        }
        let dummy = self.dom_element("div")?;
        self.dom_html_als_kinder(dummy, &html, 0)?;
        let mut fragment_kinder = Vec::new();
        if let GVal::Ref(d) = dummy {
            for k in self.dom_kinder_von(d) {
                fragment_kinder.push(k);
            }
        }
        match position.as_str() {
            "beforebegin" => {
                let vor = this;
                let eltern = match this {
                    GVal::Ref(r) => match self.dom_eltern_ref(r) {
                        Some(e) => GVal::Ref(e),
                        None => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "insertAdjacentHTML: kein eltern"))),
                    },
                    _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "kein Knoten"))),
                };
                for k in fragment_kinder {
                    self.dom_insert_before(eltern, &[k, vor])?;
                }
            },
            "afterbegin" => {
                let erste = match this {
                    GVal::Ref(r) => self.dom_kinder_von(r).into_iter().next().unwrap_or(GVal::Null),
                    _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "kein Knoten"))),
                };
                for k in fragment_kinder {
                    self.dom_insert_before(this, &[k, erste.clone()])?;
                }
            },
            "beforeend" => {
                for k in fragment_kinder {
                    self.dom_append_child(this, &[k])?;
                }
            },
            "afterend" => {
                let ep = match this {
                    GVal::Ref(r) => match self.dom_eltern_ref(r) {
                        Some(ep) => ep,
                        None => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "insertAdjacentHTML: kein eltern"))),
                    },
                    _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "kein Knoten"))),
                };
                let naechste = self.dom_naechstes_geschwister(ep, this);
                let eltern = GVal::Ref(ep);
                for k in fragment_kinder {
                    self.dom_insert_before(eltern, &[k, naechste.clone()])?;
                }
            },
            _ => return Err(Stoerung::Wurf(self.fehler_wert("SyntaxError", "unbekannte Position"))),
        }
        Ok(GVal::Undefiniert)
    }

    /// Das Geschwister nach `this` unter `eltern` — `Null` am Ende
    /// (`insertBefore(k, null)` haengt dann an).
    fn dom_naechstes_geschwister(&mut self, eltern: HeapRef, this: GVal) -> GVal {
        let kinder = self.dom_kinder_von(eltern);
        match kinder.iter().position(|x| *x == this) {
            Some(p) => kinder.get(p + 1).copied().unwrap_or(GVal::Null),
            None => GVal::Null,
        }
    }

    fn dom_knoten_oder_text(&mut self, val: GVal) -> Result<GVal, Stoerung> {
        if let GVal::Ref(r) = val {
            if self.dom_typ(r) > 0.0 {
                return Ok(val);
            }
        }
        let txt = self.zu_text(val).unwrap_or_default();
        self.dom_text(&txt)
    }

    pub(crate) fn dom_prepend(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let erste = match this {
            GVal::Ref(r) => self.dom_kinder_von(r).into_iter().next().unwrap_or(GVal::Null),
            _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "kein Knoten"))),
        };
        for arg in args {
            let k = self.dom_knoten_oder_text(*arg)?;
            self.dom_insert_before(this, &[k, erste.clone()])?;
        }
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn dom_append(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        for arg in args {
            let k = self.dom_knoten_oder_text(*arg)?;
            self.dom_append_child(this, &[k])?;
        }
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn dom_before(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let eltern = match this {
            GVal::Ref(r) => match self.dom_eltern_ref(r) {
                Some(e) => GVal::Ref(e),
                None => return Ok(GVal::Undefiniert),
            },
            _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "kein Knoten"))),
        };
        for arg in args {
            let k = self.dom_knoten_oder_text(*arg)?;
            self.dom_insert_before(eltern, &[k, this.clone()])?;
        }
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn dom_after(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let ep = match this {
            GVal::Ref(r) => match self.dom_eltern_ref(r) {
                Some(e) => e,
                None => return Ok(GVal::Undefiniert),
            },
            _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "kein Knoten"))),
        };
        let eltern = GVal::Ref(ep);
        let naechste = self.dom_naechstes_geschwister(ep, this);
        for arg in args {
            let k = self.dom_knoten_oder_text(*arg)?;
            self.dom_insert_before(eltern, &[k, naechste.clone()])?;
        }
        Ok(GVal::Undefiniert)
    }

}
