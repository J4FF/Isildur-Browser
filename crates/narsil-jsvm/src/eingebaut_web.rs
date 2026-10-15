//! J-CACHE (04.10.2026, Weisung Web-API): sprachnahes Web in eigenen
//! Dateien — atob/btoa (Base64), AbortController/Signal, CustomEvent-Detail,
//! crypto.getRandomValues/randomUUID. Reine Funktionen + node-Tests;
//! Nativ-Verdrahtung duenn (Muster Intl), gemeldet.
//!
//! node-Referenzen 04.10.2026: btoa("Hello")="SGVsbG8=",
//! atob("SGVsbG8=")="Hello", atob("YQ==")="a", Fehlername
//! InvalidCharacterError (beide Richtungen).

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_wert(c: u8) -> Option<u8> {
    if c.is_ascii_uppercase() {
        Some(c - b'A')
    } else if c.is_ascii_lowercase() {
        Some(c - b'a' + 26)
    } else if c.is_ascii_digit() {
        Some(c - b'0' + 52)
    } else if c == b'+' {
        Some(62)
    } else if c == b'/' {
        Some(63)
    } else {
        None
    }
}

/// btoa: Latin1-Bytes -> Base64. Zeichen > 255 -> Err (InvalidCharacterError).
pub(crate) fn btoa_encode(s: &str) -> Result<String, ()> {
    let mut bytes: Vec<u8> = Vec::new();
    for c in s.chars() {
        let n = c as u32;
        if n > 255 {
            return Err(());
        }
        bytes.push(n as u8);
    }
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() {
            bytes[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < bytes.len() {
            bytes[i + 2] as u32
        } else {
            0
        };
        out.push(B64[((b0 >> 2) & 63) as usize] as char);
        out.push(B64[(((b0 & 3) << 4) | ((b1 >> 4) & 15)) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(B64[(((b1 & 15) << 2) | ((b2 >> 6) & 3)) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < bytes.len() {
            out.push(B64[(b2 & 63) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    Ok(out)
}

/// atob: Base64 -> Latin1-Text. Leerzeichen werden ignoriert (Spec:
/// „forgiving-base64"), Ungueltiges -> Err. Padding aehnlich V8.
pub(crate) fn atob_decode(s: &str) -> Result<String, ()> {
    let sauber: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    if sauber.is_empty() {
        return Ok(String::new());
    }
    if sauber.len() % 4 != 0 {
        return Err(());
    }
    let mut out = String::new();
    let mut i = 0;
    while i < sauber.len() {
        let v = [
            b64_wert(sauber[i]),
            b64_wert(sauber[i + 1]),
            if sauber[i + 2] == b'=' {
                Some(0)
            } else {
                b64_wert(sauber[i + 2])
            },
            if sauber[i + 3] == b'=' {
                Some(0)
            } else {
                b64_wert(sauber[i + 3])
            },
        ];
        let v = match (v[0], v[1], v[2], v[3]) {
            (Some(a), Some(b), Some(c), Some(d)) => [a, b, c, d],
            _ => return Err(()),
        };
        // Padding nur an den letzten beiden Stellen des Endes.
        if (sauber[i + 2] == b'=' || sauber[i + 3] == b'=') && i + 4 != sauber.len() {
            return Err(());
        }
        if sauber[i + 2] == b'=' && sauber[i + 3] != b'=' {
            return Err(());
        }
        out.push((v[0] << 2 | v[1] >> 4) as char);
        if sauber[i + 2] != b'=' {
            out.push((v[1] << 4 | v[2] >> 2) as char);
        }
        if sauber[i + 3] != b'=' {
            out.push((v[2] << 6 | v[3]) as char);
        }
        i += 4;
    }
    Ok(out)
}

/// crypto.getRandomValues: gemeinsamer System-CSPRNG auf Linux und Windows.
/// Schreibt in `ziel` (Feld-Bytes), gibt Anzahl zurueck.
pub(crate) fn zufalls_bytes(ziel: &mut [u8]) -> Result<usize, ()> {
    for block in ziel.chunks_mut(narsil_tls::zufall::MAX_BLOCK) {
        let zufall = narsil_tls::zufall::bytes(block.len()).map_err(|_| ())?;
        block.copy_from_slice(&zufall);
    }
    Ok(ziel.len())
}

/// crypto.randomUUID v4 aus 16 Zufallsbytes (RFC 4122: Version+Variante).
pub(crate) fn zufalls_uuid() -> Result<String, ()> {
    let mut b = [0u8; 16];
    zufalls_bytes(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    ))
}

// Web-Engine-Teil (J-CACHE 04.10.2026, Weisung CACHE_26).
use crate::builtin::Nativ;
use crate::heap::GVal;
use crate::interp::{Engine, Stoerung};

impl Engine {
    /// atob/btoa global (Weisung). Fehlername InvalidCharacterError
    /// (Proto faellt auf Error zurueck — instanceof DOMException
    /// nicht abbildbar, dokumentiert).
    pub(crate) fn n_web_atob(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let s = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        match crate::eingebaut_web::atob_decode(&s) {
            Ok(t) => self.text_wert(&t).map_err(Stoerung::Fehler),
            Err(()) => Err(Stoerung::Wurf(
                self.fehler_wert("InvalidCharacterError", "ungueltiges Base64"),
            )),
        }
    }

    pub(crate) fn n_web_btoa(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let s = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        match crate::eingebaut_web::btoa_encode(&s) {
            Ok(t) => self.text_wert(&t).map_err(Stoerung::Fehler),
            Err(()) => Err(Stoerung::Wurf(
                self.fehler_wert("InvalidCharacterError", "Zeichen ausserhalb Latin1"),
            )),
        }
    }
}

// Web-Engine: AbortController/Signal + CustomEvent + crypto (J-CACHE 04.10.2026).
use crate::heap::{HObj, Heap, HeapRef};

impl Engine {
    /// AbortSignal-Objekt anlegen (aborted-Vorgabe). Modell: Nativ mit
    /// Feld [aborted] als ziel + eigene Eigenschaften aborted/onabort.
    /// Spec-Naeherung: addEventListener auf Signalen laeuft ueber
    /// onabort (dokumentiert); fuer fetch-Abbrueche reicht aborted.
    /// Proto aus dem Konstruktor lesen (kein Engine-Feld noetig).
    pub(crate) fn abbruch_proto(&mut self, ctor_ref: HeapRef) -> HeapRef {
        match self.heap.holen(ctor_ref).clone() {
            HObj::Nativ { eigenschaften, .. } | HObj::Funktion { eigenschaften, .. } => {
                match eigenschaften.get("prototype") {
                    Some(v) => match v.wert {
                        GVal::Ref(q) => q,
                        _ => self.proto_objekt,
                    },
                    _ => self.proto_objekt,
                }
            }
            _ => self.proto_objekt,
        }
    }

    /// Signal als GEWOEHNLICHES Objekt (typeof object, Spec) — aborted/
    /// onabort als eigene Eigenschaften. n_abbruch erkennt das Signal am
    /// aborted-Prop (Controller traegt stattdessen das Signal im Feld).
    pub(crate) fn abbruch_signal(
        &mut self,
        proto: HeapRef,
        abgebrochen: bool,
    ) -> Result<GVal, Stoerung> {
        let r = self.neues_objekt(Some(proto)).map_err(Stoerung::Fehler)?;
        self.setze(GVal::Ref(r), "aborted", GVal::Bool(abgebrochen))?;
        self.setze(GVal::Ref(r), "onabort", GVal::Undefiniert)?;
        Ok(GVal::Ref(r))
    }

    /// new AbortController() -> { signal }.
    pub(crate) fn n_abbruch_ctor(&mut self, ctor_ref: HeapRef) -> Result<GVal, Stoerung> {
        let proto = self.abbruch_proto(ctor_ref);
        // Signal-Proto ist das Proto des AbortSignal-Ctors; hier liegt nur
        // der Controller-Ctor vor — das Signal-Proto haengt an ihm als
        // "__signal_proto" (Setup). Fallback: eigenes Proto-Verhalten.
        let signal_proto = match self.eigene_lese(ctor_ref, "__signal_proto") {
            Some(GVal::Ref(q)) => q,
            _ => proto,
        };
        let signal = self.abbruch_signal(signal_proto, false)?;
        let sr = match signal {
            GVal::Ref(r) => r,
            _ => unreachable!(),
        };
        let ctx = self.feld_aus_werten(&[signal])?;
        let obj = HObj::Nativ {
            name: "AbortController".to_string(),
            proto: Some(proto),
            ziel: Some(ctx),
            eigenschaften: Default::default(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        let _ = self.vor_allok(kosten);
        let r = self.belege(obj, kosten);
        self.setze(GVal::Ref(r), "signal", GVal::Ref(sr))?;
        Ok(GVal::Ref(r))
    }

    /// controller.abort() / signal.throwIfAborted(): Art 0 = abort,
    /// 1 = throwIfAborted (this = Signal).
    pub(crate) fn n_abbruch(&mut self, this: GVal, art: u8) -> Result<GVal, Stoerung> {
        // Signal finden: Controller (Nativ mit Signal im Feld) oder das
        // Signal selbst (aborted-Eigenschaft).
        let sig = match this {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Nativ { ziel: Some(z), .. } => match self.heap.holen(z).clone() {
                    HObj::Feld { elemente, .. } => match elemente.first() {
                        Some(GVal::Ref(sr)) => *sr,
                        _ => r,
                    },
                    _ => r,
                },
                _ => r,
            },
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Abort braucht Objekt"),
                ));
            }
        };
        if art == 1 {
            // throwIfAborted.
            let ab = matches!(self.eigene_lese(sig, "aborted"), Some(GVal::Bool(true)));
            if ab {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("AbortError", "abgebrochen"),
                ));
            }
            return Ok(GVal::Undefiniert);
        }
        // abort(): aborted setzen + onabort rufen.
        // Release 10.10.2026: ein zweites `abort()` tut nichts (DOM §3.2).
        if matches!(self.eigene_lese(sig, "aborted"), Some(GVal::Bool(true))) {
            return Ok(GVal::Undefiniert);
        }
        self.setze(GVal::Ref(sig), "aborted", GVal::Bool(true))?;
        // Die Anmeldungen mit diesem Signal sind weg, und das Signal meldet
        // `abort` an seine eigenen Hoerer (`signal.addEventListener('abort')`
        // — fetch-Abbruch, Bibliotheken). GEMESSEN vorher: nur `onabort`.
        self.hoerer.retain(|h| h.signal != Some(sig));
        let ev = self.ereignis_bauen("abort", GVal::Ref(sig), false, Vec::new())?;
        let cb = self
            .eigene_lese(sig, "onabort")
            .unwrap_or(GVal::Undefiniert);
        if self.ist_aufrufbar(cb) {
            let _ = self.aufruf_synchron(cb, GVal::Ref(sig), &[ev], false);
        }
        let _ = self.abbruch_melden(GVal::Ref(sig), ev);
        Ok(GVal::Undefiniert)
    }

    /// new CustomEvent(type, {detail, cancelable}): Event mit detail.
    /// Nutzt proto_ereignis direkt (eigene detail-Eigenschaft auf Instanz).
    pub(crate) fn n_ereignis_anwender(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let typ = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let (detail, cancel) = match self.arg(args, 1) {
            GVal::Ref(_) => {
                let o = self.arg(args, 1);
                let d = self.hole(o, "detail").unwrap_or(GVal::Undefiniert);
                let c = self.hole(o, "cancelable").unwrap_or(GVal::Bool(false));
                (d, self.zu_bool(c))
            }
            _ => (GVal::Undefiniert, false),
        };
        let ev = self
            .neues_objekt(Some(self.proto_ereignis))
            .map_err(Stoerung::Fehler)?;
        let t = self.text_wert(&typ).map_err(Stoerung::Fehler)?;
        self.setze(GVal::Ref(ev), "type", t)?;
        self.setze(GVal::Ref(ev), "detail", detail)?;
        self.setze(GVal::Ref(ev), "cancelable", GVal::Bool(cancel))?;
        self.setze(GVal::Ref(ev), "defaultPrevented", GVal::Bool(false))?;
        // 05.10.2026: die Felder der Event-Konstruktoren (MouseEvent,
        // KeyboardEvent, ...). GEMESSEN an youtubes
        // `webcomponents-all-noPatch.js`: der Polyfill laeuft nur, wenn
        // `window.MouseEvent` fehlt; mit dem Konstruktor ueberspringt er
        // sich. Ohne die Felder waere `new MouseEvent('click',{clientX:5})`
        // ein Event ohne `clientX` — still falsch statt fehlend.
        self.setze(GVal::Ref(ev), "bubbles", GVal::Bool(false))?;
        self.setze(GVal::Ref(ev), "composed", GVal::Bool(false))?;
        if let GVal::Ref(_) = self.arg(args, 1) {
            let o = self.arg(args, 1);
            for feld in [
                "bubbles",
                "composed",
                "clientX",
                "clientY",
                "screenX",
                "screenY",
                "pageX",
                "pageY",
                "button",
                "buttons",
                "ctrlKey",
                "shiftKey",
                "altKey",
                "metaKey",
                "key",
                "code",
                "repeat",
                "relatedTarget",
                "view",
                "deltaX",
                "deltaY",
                "deltaMode",
                "isComposing",
                "data",
                "pointerId",
                "pointerType",
                "width",
                "height",
                "target",
            ] {
                let w = self.hole(o, feld)?;
                if !matches!(w, GVal::Undefiniert) {
                    self.setze(GVal::Ref(ev), feld, w)?;
                }
            }
        }
        Ok(GVal::Ref(ev))
    }

    /// Web-Aufbau: AbortController/Signal, CustomEvent, crypto-Objekt.
    /// EINE Aufrufzeile in builtin.rs-aufbauen (Weisung CACHE_26).
    /// Methoden liegen auf den Protos (nicht auf Instanzen).
    pub(crate) fn web_aufbauen(&mut self) -> Result<(), Stoerung> {
        let po = self.proto_objekt;
        // Methode auf Proto registrieren (nicht aufzaehlbar wie reg).
        let meth = |e: &mut Engine, proto: HeapRef, n: &str, nat: Nativ| {
            let f = e.nativ_anlegen(n, nat, e.proto_funktion);
            e.setze_eigen(proto, n, GVal::Ref(f));
        };
        // AbortController + Signal.
        let sig_proto = self.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
        meth(self, sig_proto, "throwIfAborted", Nativ::WebAbbruchWerfen);
        meth(self, sig_proto, "addEventListener", Nativ::HoererHinzu);
        let ctl_proto = self.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
        meth(self, ctl_proto, "abort", Nativ::WebAbbruch);
        let ctl = self.nativ_anlegen(
            "AbortController",
            Nativ::WebAbbruchCtor,
            self.proto_funktion,
        );
        self.nativ_prototyp(ctl, ctl_proto);
        let sig_ctor = self.nativ_anlegen(
            "AbortSignal",
            Nativ::WebAbbruchSignalCtor,
            self.proto_funktion,
        );
        self.nativ_prototyp(sig_ctor, sig_proto);
        // abort() statisch + Signal-Proto am Controller verlinken.
        meth(self, ctl, "abort", Nativ::WebAbbruchStatisch);
        self.setze_eigen(ctl, "__signal_proto", GVal::Ref(sig_proto));
        meth(self, sig_ctor, "abort", Nativ::WebAbbruchStatisch);
        self.global_binden("AbortController", GVal::Ref(ctl));
        self.global_binden("AbortSignal", GVal::Ref(sig_ctor));
        // CustomEvent (Proto-Kette -> Event-Prototyp).
        let ce_proto = self
            .neues_objekt(Some(self.proto_ereignis))
            .map_err(Stoerung::Fehler)?;
        let ce = self.nativ_anlegen(
            "CustomEvent",
            Nativ::WebEreignisAnwender,
            self.proto_funktion,
        );
        self.nativ_prototyp(ce, ce_proto);
        self.global_binden("CustomEvent", GVal::Ref(ce));
        // 05.10.2026: DIE EVENT-KONSTRUKTOREN DER MAUS/TASTATUR-FAMILIE.
        //
        // GEMESSEN an youtubes `webcomponents-all-noPatch.js`: es prueft
        // `!window.MouseEvent || typeof window.MouseEvent!=="function"` und
        // polyfillt sonst — dabei steht `window.MouseEvent.prototype =
        // ja.prototype` HINTER der `if(ja)`-Wache; `ja` ist `undefined`, und
        // das ganze Skript stirbt mit `TypeError: kein Zugriff auf
        // Undefiniert (Eigenschaft prototype)`. Ein Browser mit
        // `MouseEvent` ueberspringt den Block.
        //
        // Alle teilen den Event-Prototyp und den Anwender-Handler; ihre
        // Init-Felder kopiert `n_ereignis_anwender` aus dem Optionsobjekt.
        for name in [
            "Event",
            "UIEvent",
            "MouseEvent",
            "KeyboardEvent",
            "FocusEvent",
            "InputEvent",
            "WheelEvent",
            "TouchEvent",
            "PointerEvent",
            "CompositionEvent",
            "DragEvent",
            "ClipboardEvent",
            "SubmitEvent",
            "ProgressEvent",
            "PageTransitionEvent",
            "HashChangeEvent",
            "PopStateEvent",
            "ErrorEvent",
        ] {
            let proto = self
                .neues_objekt(Some(self.proto_ereignis))
                .map_err(Stoerung::Fehler)?;
            let ctor = self.nativ_anlegen(name, Nativ::WebEreignisAnwender, self.proto_funktion);
            self.nativ_prototyp(ctor, proto);
            self.global_binden(name, GVal::Ref(ctor));
        }
        // crypto-Objekt.
        let crypto = self.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
        meth(self, crypto, "randomUUID", Nativ::WebKryptoUuid);
        meth(self, crypto, "getRandomValues", Nativ::WebKryptoZufall);
        // 2026-10-15: `crypto.subtle`, loaded on its first read.
        self.crypto_subtle_getter(crypto);
        self.global_binden("crypto", GVal::Ref(crypto));
        self.web_daten_aufbauen()
    }

    /// P1-Datenobjekte aufbauen (Weisung CACHE_26/27): Headers,
    /// Request, Response, Blob, File, FormData. Modell: Nativ mit
    /// Options-Feld als ziel (wie Intl). Wird aus web_aufbauen gerufen.
    pub(crate) fn web_daten_aufbauen(&mut self) -> Result<(), Stoerung> {
        let po = self.proto_objekt;
        let meth = |e: &mut Engine, proto: HeapRef, n: &str, nat: Nativ| {
            let f = e.nativ_anlegen(n, nat, e.proto_funktion);
            e.setze_eigen(proto, n, GVal::Ref(f));
        };
        // Headers.
        let kopf_proto = self.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
        meth(self, kopf_proto, "append", Nativ::WebKopfAnhaengen);
        meth(self, kopf_proto, "delete", Nativ::WebKopfLoeschen);
        meth(self, kopf_proto, "get", Nativ::WebKopfHolen);
        meth(self, kopf_proto, "has", Nativ::WebKopfHat);
        meth(self, kopf_proto, "set", Nativ::WebKopfSetzen);
        let kopf_ctor = self.nativ_anlegen("Headers", Nativ::WebKopfCtor, self.proto_funktion);
        self.nativ_prototyp(kopf_ctor, kopf_proto);
        self.global_binden("Headers", GVal::Ref(kopf_ctor));
        // Request.
        let anf_proto = self.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
        let anf_ctor = self.nativ_anlegen("Request", Nativ::WebAnfrageCtor, self.proto_funktion);
        self.nativ_prototyp(anf_ctor, anf_proto);
        self.global_binden("Request", GVal::Ref(anf_ctor));
        // Response (+ text/json).
        let ant_proto = self.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
        meth(self, ant_proto, "text", Nativ::WebAntwortText);
        meth(self, ant_proto, "json", Nativ::WebAntwortJson);
        meth(self, ant_proto, "arrayBuffer", Nativ::WebAntwortPuffer);
        meth(self, ant_proto, "bytes", Nativ::WebAntwortBytes);
        let ant_ctor = self.nativ_anlegen("Response", Nativ::WebAntwortCtor, self.proto_funktion);
        self.nativ_prototyp(ant_ctor, ant_proto);
        self.global_binden("Response", GVal::Ref(ant_ctor));
        // Blob/File (+ text/slice).
        let klecks_proto = self.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
        meth(self, klecks_proto, "text", Nativ::WebKlecksText);
        meth(self, klecks_proto, "slice", Nativ::WebKlecksSchneiden);
        meth(self, klecks_proto, "arrayBuffer", Nativ::WebKlecksPuffer);
        meth(self, klecks_proto, "bytes", Nativ::WebKlecksBytes);
        let klecks_ctor = self.nativ_anlegen("Blob", Nativ::WebKlecksCtor, self.proto_funktion);
        self.nativ_prototyp(klecks_ctor, klecks_proto);
        self.global_binden("Blob", GVal::Ref(klecks_ctor));
        let datei_ctor = self.nativ_anlegen("File", Nativ::WebDateiCtor, self.proto_funktion);
        self.nativ_prototyp(datei_ctor, klecks_proto);
        self.global_binden("File", GVal::Ref(datei_ctor));
        // FormData.
        let form_proto = self.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
        meth(self, form_proto, "append", Nativ::WebFormularAnhaengen);
        meth(self, form_proto, "delete", Nativ::WebFormularLoeschen);
        meth(self, form_proto, "get", Nativ::WebFormularHolen);
        meth(self, form_proto, "has", Nativ::WebFormularHat);
        meth(self, form_proto, "set", Nativ::WebFormularSetzen);
        meth(self, form_proto, "getAll", Nativ::WebFormularAlle);
        // Release 12.10.2026: FormData ist `iterable<USVString,
        // FormDataEntryValue>` (XHR §5.1, WebIDL §3.7.9). GEMESSEN vorher:
        // `fd.entries` fehlte, `new URLSearchParams(fd)` war LEER — jedes
        // GET-Formular schickte keine Felder (WPT encoding/*-encode-form,
        // 46 Dateien, rund 700.000 Teiltests).
        meth(self, form_proto, "keys", Nativ::WebFormularSchluessel);
        meth(self, form_proto, "values", Nativ::WebFormularWerte);
        meth(self, form_proto, "entries", Nativ::WebFormularEintraege);
        meth(self, form_proto, "forEach", Nativ::WebFormularFuerAlle);
        self.symbol_iterator_als_alias(form_proto, "entries");
        let form_ctor = self.nativ_anlegen("FormData", Nativ::WebFormularCtor, self.proto_funktion);
        self.nativ_prototyp(form_ctor, form_proto);
        self.global_binden("FormData", GVal::Ref(form_ctor));
        // J-CACHE (05.10., PROBE_24): Knoten-Kette Node <- Element <-
        // HTMLElement (neue Protos, nichts Bestehendes umgehaengt) +
        // Beobachter (observe/disconnect/takeRecords + Feuer-Wrapper).
        let knoten_proto = self.neues_objekt(Some(self.proto_ereignisziel)).map_err(Stoerung::Fehler)?;
        let knoten_ctor = self.nativ_anlegen("Node", Nativ::WebKnoten, self.proto_funktion);
        self.nativ_prototyp(knoten_ctor, knoten_proto);
        self.global_binden("Node", GVal::Ref(knoten_ctor));
        for r in [self.proto_dom_element, self.proto_dom_text, self.proto_dom_dokument] {
            if let HObj::Gegenstand { proto, .. } = self.heap.holen_mut(r) { *proto = Some(knoten_proto); }
        }
        let document_ctor = self.nativ_anlegen("Document", Nativ::WebDocument, self.proto_funktion);
        self.nativ_prototyp(document_ctor, self.proto_dom_dokument);
        self.setze_eigen(self.proto_dom_dokument, "constructor", GVal::Ref(document_ctor));
        self.global_binden("Document", GVal::Ref(document_ctor));
        for (name, native) in [
            ("nodeType", Nativ::DomNodeType), ("nodeName", Nativ::DomNodeNameKurz),
            ("parentNode", Nativ::DomElternLesen), ("childNodes", Nativ::DomChildNodes),
            ("firstChild", Nativ::DomFirstChild), ("lastChild", Nativ::DomLastChild),
        ] { self.nativ_getter(knoten_proto, name, native); }
        for (name, value) in [("ELEMENT_NODE",1.0),("ATTRIBUTE_NODE",2.0),("TEXT_NODE",3.0),
            ("CDATA_SECTION_NODE",4.0),("ENTITY_REFERENCE_NODE",5.0),("ENTITY_NODE",6.0),
            ("PROCESSING_INSTRUCTION_NODE",7.0),("COMMENT_NODE",8.0),("DOCUMENT_NODE",9.0),
            ("DOCUMENT_TYPE_NODE",10.0),("DOCUMENT_FRAGMENT_NODE",11.0),("NOTATION_NODE",12.0),
            // Release 12.10.2026: DOM §4.4 `compareDocumentPosition`-Masken.
            // GEMESSEN vorher: `Node.DOCUMENT_POSITION_FOLLOWING` war
            // undefined — `cdp(x) & Node.DOCUMENT_POSITION_FOLLOWING` immer 0
            // (WPT dom/ranges common.js `getPosition`, ~3.000 Teiltests).
            ("DOCUMENT_POSITION_DISCONNECTED",1.0),("DOCUMENT_POSITION_PRECEDING",2.0),
            ("DOCUMENT_POSITION_FOLLOWING",4.0),("DOCUMENT_POSITION_CONTAINS",8.0),
            ("DOCUMENT_POSITION_CONTAINED_BY",16.0),("DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC",32.0)] {
            self.setze_eigen(knoten_ctor, name, GVal::Zahl(value));
            self.setze_eigen(knoten_proto, name, GVal::Zahl(value));
            for target in [knoten_ctor, knoten_proto] {
                if let HObj::Gegenstand { eigenschaften, .. } | HObj::Nativ { eigenschaften, .. } = self.heap.holen_mut(target)
                    && let Some(property) = eigenschaften.get_mut(name)
                { property.beschreibbar = false; property.konfigurierbar = false; }
            }
        }
        let element_proto = self.proto_dom_element;
        let element_ctor = self.nativ_anlegen("Element", Nativ::WebElement, self.proto_funktion);
        self.nativ_prototyp(element_ctor, element_proto);
        self.global_binden("Element", GVal::Ref(element_ctor));
        let html_proto = self.proto_dom_html;
        let html_ctor = self.nativ_anlegen("HTMLElement", Nativ::WebHtmlElement, self.proto_funktion);
        self.nativ_prototyp(html_ctor, html_proto);
        self.global_binden("HTMLElement", GVal::Ref(html_ctor));
        // Release 2026-10-15: `X.prototype.constructor === X` (WebIDL
        // §3.7.3) for Node, Element and HTMLElement too, so their
        // @@toStringTag is set below. MEASURED before:
        // `HTMLElement.prototype.constructor === HTMLElement` was false and
        // `String(document.createElement("article"))` "[object Object]".
        self.setze_eigen(knoten_proto, "constructor", GVal::Ref(knoten_ctor));
        self.setze_eigen(element_proto, "constructor", GVal::Ref(element_ctor));
        self.setze_eigen(html_proto, "constructor", GVal::Ref(html_ctor));
        self.dom_eingebaut.insert(knoten_proto);
        self.dom_schnittstellen_anlegen(knoten_proto, element_proto)?;
        // Release 09.10.2026: die Custom-Element-Bruecke fuer `vorspann.js`
        // (das Skript nimmt sie und loescht den Namen wieder).
        let ce = self.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
        meth(self, ce, "haken", Nativ::CeHaken);
        meth(self, ce, "definieren", Nativ::CeDefinieren);
        meth(self, ce, "bauen", Nativ::CeBauen);
        meth(self, ce, "fertig", Nativ::CeFertig);
        self.global_binden("__narsilCe", GVal::Ref(ce));
        for (name, ctor_nat) in [
            ("MutationObserver", Nativ::WebMutationCtor),
            ("IntersectionObserver", Nativ::WebKreuzungCtor),
            ("ResizeObserver", Nativ::WebGroesseCtor),
        ] {
            let proto = self.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
            meth(self, proto, "observe", Nativ::WebBeobachten);
            meth(self, proto, "disconnect", Nativ::WebTrennen);
            meth(self, proto, "takeRecords", Nativ::WebNehmen);
            let ctor = self.nativ_anlegen(name, ctor_nat, self.proto_funktion);
            self.nativ_prototyp(ctor, proto);
            self.global_binden(name, GVal::Ref(ctor));
        }
        Ok(())
    }

    /// Kopf-Feld (Nativ-ziel) lesen: [name, wert, ...], Namen klein.
    fn kopf_feld(&mut self, this: GVal) -> Result<HeapRef, Stoerung> {
        match this {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Nativ { ziel: Some(z), .. } => Ok(*z),
                _ => Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Headers braucht Headers-Objekt"),
                )),
            },
            _ => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Headers braucht Headers-Objekt"),
            )),
        }
    }

    /// new Headers(init?): Gegenstand (Schluessel), Feld-Paare oder Headers.
    pub(crate) fn n_kopf_ctor(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let proto = self.abbruch_proto(ctor_ref);
        let mut paare: Vec<GVal> = Vec::new();
        let mut nimm = |e: &mut Engine, n: &str, w: &str| {
            let a = e.text_wert(&n.to_lowercase()).unwrap_or(GVal::Undefiniert);
            let b = e.text_wert(w).unwrap_or(GVal::Undefiniert);
            paare.push(a);
            paare.push(b);
        };
        match self.arg(args, 0) {
            GVal::Undefiniert => {}
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Nativ { ziel: Some(z), .. } => {
                    // Headers-Kopie (Namen schon klein).
                    if let HObj::Feld { elemente, .. } = self.heap.holen(z).clone() {
                        paare.extend(elemente.iter().cloned());
                    }
                }
                HObj::Feld { elemente, .. } => {
                    // Paare [[n, w], ...].
                    for e in elemente {
                        if let GVal::Ref(fr) = e {
                            if let HObj::Feld { elemente: p, .. } = self.heap.holen(fr).clone() {
                                if p.len() >= 2 {
                                    let n = self.zu_text(p[0]).unwrap_or_default();
                                    let w = self.zu_text(p[1]).unwrap_or_default();
                                    nimm(self, &n, &w);
                                }
                            }
                        }
                    }
                }
                _ => {
                    // Gegenstand: eigene Schluessel.
                    let ks = self.eigene_schluessel(GVal::Ref(r))?;
                    for k in ks {
                        let w = self.hole(GVal::Ref(r), &k).unwrap_or(GVal::Undefiniert);
                        let w = self.zu_text(w).unwrap_or_default();
                        nimm(self, &k, &w);
                    }
                }
            },
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Headers braucht Objekt oder Paare"),
                ));
            }
        }
        let ctx = self.feld_aus_werten(&paare)?;
        let obj = HObj::Nativ {
            name: "Headers".to_string(),
            proto: Some(proto),
            ziel: Some(ctx),
            eigenschaften: Default::default(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        let _ = self.vor_allok(kosten);
        Ok(GVal::Ref(self.belege(obj, kosten)))
    }

    /// Headers-Methoden: Art 0=append, 1=delete, 2=get, 3=has, 4=set.
    /// Namen klein + gestutzt (Spec-Norm). get -> null wenn fehlt.
    pub(crate) fn n_kopf(&mut self, this: GVal, args: &[GVal], art: u8) -> Result<GVal, Stoerung> {
        let z = self.kopf_feld(this)?;
        let name = self
            .zu_text(self.arg(args, 0))
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if art == 2 || art == 3 {
            // J-CACHE: get kombiniert ALLE Werte mit ", " (Spec) — nicht
            // nur den ersten. GEMESSEN node: append("A","2") -> "1, 2".
            let el = match self.heap.holen(z).clone() {
                HObj::Feld { elemente, .. } => elemente,
                _ => Vec::new(),
            };
            let mut gefunden: Vec<String> = Vec::new();
            let mut i = 0;
            while i + 1 < el.len() {
                let n = self.zu_text(el[i]).unwrap_or_default();
                if n == name {
                    gefunden.push(self.zu_text(el[i + 1]).unwrap_or_default());
                }
                i += 2;
            }
            if art == 3 {
                return Ok(GVal::Bool(!gefunden.is_empty()));
            }
            if gefunden.is_empty() {
                return Ok(GVal::Null);
            }
            let t = self
                .text_wert(&gefunden.join(", "))
                .unwrap_or(GVal::Undefiniert);
            return Ok(t);
        }
        if art == 1 {
            // delete: alle Paare mit Namen entfernen (erst klonen, dann
            // schreiben — sonst Borrow-Konflikt mit zu_text).
            let alt: Vec<GVal> = match self.heap.holen(z).clone() {
                HObj::Feld { elemente, .. } => elemente,
                _ => Vec::new(),
            };
            let mut neu: Vec<GVal> = Vec::new();
            let mut k = 0;
            while k + 1 < alt.len() {
                if self.zu_text(alt[k]).unwrap_or_default() != name {
                    neu.push(alt[k]);
                    neu.push(alt[k + 1]);
                }
                k += 2;
            }
            if k < alt.len() {
                neu.push(alt[k]);
            }
            let o = self.heap.holen_mut(z);
            if let HObj::Feld { elemente, .. } = o {
                *elemente = neu;
            }
            return Ok(GVal::Undefiniert);
        }
        // append (0): ans Ende. set (4): erst loeschen, dann ans Ende.
        let wert = self.zu_text(self.arg(args, 1)).unwrap_or_default();
        let wt = self.text_wert(&wert).unwrap_or(GVal::Undefiniert);
        let nt = self.text_wert(&name).unwrap_or(GVal::Undefiniert);
        if art == 4 {
            let alt: Vec<GVal> = match self.heap.holen(z).clone() {
                HObj::Feld { elemente, .. } => elemente,
                _ => Vec::new(),
            };
            let mut gefiltert: Vec<GVal> = Vec::new();
            let mut k = 0;
            while k + 1 < alt.len() {
                if self.zu_text(alt[k]).unwrap_or_default() != name {
                    gefiltert.push(alt[k]);
                    gefiltert.push(alt[k + 1]);
                }
                k += 2;
            }
            if k < alt.len() {
                gefiltert.push(alt[k]);
            }
            let o = self.heap.holen_mut(z);
            if let HObj::Feld { elemente, .. } = o {
                *elemente = gefiltert;
            }
        }
        let o = self.heap.holen_mut(z);
        if let HObj::Feld { elemente, .. } = o {
            elemente.push(nt);
            elemente.push(wt);
        }
        Ok(GVal::Undefiniert)
    }

    /// The API base URL of this global (Fetch "parse a URL"): the
    /// document's base URL; in a worker (no document) `location.href`.
    /// `None` for an engine without either (plain script runs).
    fn api_basis_url(&mut self) -> Option<String> {
        let global = GVal::Ref(self.global_objekt);
        let doc = self.hole(global, "document").ok()?;
        let quelle = if matches!(doc, GVal::Ref(_)) {
            self.hole(doc, "baseURI").ok()?
        } else {
            let ort = self.hole(global, "location").ok()?;
            if !matches!(ort, GVal::Ref(_)) {
                return None;
            }
            self.hole(ort, "href").ok()?
        };
        let t = self.zu_text(quelle).ok()?;
        (t.contains(':') && t != "undefined").then_some(t)
    }

    /// new Request(url, init?): Feld [url, methode, kopf, koerper].
    /// Eigene Props url/method/headers (lesbar wie im Browser).
    pub(crate) fn n_anfrage_ctor(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let proto = self.abbruch_proto(ctor_ref);
        let url = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        // A URL string is parsed against the API base URL (Fetch §5.4
        // `new Request` step 5): the document's base URL, in a worker its
        // script URL. A URL that does not parse is a TypeError. MEASURED
        // before: `new Request('./x')` kept the relative text, also in a
        // blob: worker (whose base cannot resolve it; WPT
        // dedicated-worker-from-blob-url).
        let von_anfrage = matches!(self.arg(args, 0), GVal::Ref(r)
            if matches!(self.heap.holen(r), HObj::Nativ { name, .. } if name == "Request"));
        // `new Request(request)`: its URL (and below its method/headers when
        // the init does not name them). MEASURED before: the URL was the text
        // "[object Request]".
        let url = if von_anfrage {
            let u = self.hole(self.arg(args, 0), "url").unwrap_or(GVal::Undefiniert);
            self.zu_text(u).unwrap_or_default()
        } else {
            url
        };
        let url = match (von_anfrage, self.api_basis_url()) {
            (false, Some(basis)) => match narsil_url::loese_auf(&basis, &url) {
                Ok(u) => u,
                Err(_) => {
                    return Err(Stoerung::Wurf(self.fehler_wert(
                        "TypeError",
                        &format!("Failed to construct 'Request': Failed to parse URL from {url}"),
                    )));
                }
            },
            _ => url,
        };
        let (methode, kopf_init, koerper) = match self.arg(args, 1) {
            GVal::Ref(_) => {
                let o = self.arg(args, 1);
                let m = self.hole(o, "method").unwrap_or(GVal::Undefiniert);
                let m = if matches!(m, GVal::Undefiniert) {
                    "GET".to_string()
                } else {
                    self.zu_text(m).unwrap_or_default().to_uppercase()
                };
                let h = self.hole(o, "headers").unwrap_or(GVal::Undefiniert);
                let b = self.hole(o, "body").unwrap_or(GVal::Undefiniert);
                let b = if matches!(b, GVal::Undefiniert) {
                    b
                } else {
                    let t = self.zu_text(b).unwrap_or_default();
                    self.text_wert(&t).unwrap_or(GVal::Undefiniert)
                };
                (m, h, b)
            }
            _ => ("GET".to_string(), GVal::Undefiniert, GVal::Undefiniert),
        };
        let (methode, kopf_init) = if von_anfrage {
            let quelle = self.arg(args, 0);
            let init = self.arg(args, 1);
            let hat = |e: &mut Self, n: &str| matches!(init, GVal::Ref(_)) && !matches!(e.hole(init, n).unwrap_or(GVal::Undefiniert), GVal::Undefiniert);
            let m = if hat(self, "method") {
                methode
            } else {
                let m = self.hole(quelle, "method").unwrap_or(GVal::Undefiniert);
                self.zu_text(m).unwrap_or_else(|_| "GET".to_string())
            };
            let h = if hat(self, "headers") { kopf_init } else { self.hole(quelle, "headers").unwrap_or(GVal::Undefiniert) };
            (m, h)
        } else {
            (methode, kopf_init)
        };
        // Kopf-Objekt bauen (leer oder uebernehmen/fuellen).
        // 2026-10-15: with the Headers prototype (not Request's).
        let headers_ctor = self.headers_ctor_ref(ctor_ref);
        let kopf = match kopf_init {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Nativ { .. } => GVal::Ref(r),
                _ => self.n_kopf_ctor(headers_ctor, &[kopf_init])?,
            },
            _ => self.n_kopf_ctor(headers_ctor, &[])?,
        };
        let ut = self.text_wert(&url).unwrap_or(GVal::Undefiniert);
        let mt = self.text_wert(&methode).unwrap_or(GVal::Undefiniert);
        let ctx = self.feld_aus_werten(&[ut, mt, kopf, koerper])?;
        let obj = HObj::Nativ {
            name: "Request".to_string(),
            proto: Some(proto),
            ziel: Some(ctx),
            eigenschaften: Default::default(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        let _ = self.vor_allok(kosten);
        let r = self.belege(obj, kosten);
        self.setze(GVal::Ref(r), "url", ut)?;
        self.setze(GVal::Ref(r), "method", mt)?;
        self.setze(GVal::Ref(r), "headers", kopf)?;
        Ok(GVal::Ref(r))
    }

    /// new Response(koerper?, init?): Feld [koerper, status, kopf].
    /// status ausser 200-599 (und null->200) wirft RangeError (Spec).
    pub(crate) fn n_antwort_ctor(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let proto = self.abbruch_proto(ctor_ref);
        // 2026-10-15: the body is extracted as bytes (Fetch §5.2 "extract a
        // body"), so binary bodies survive (`new Response(arrayBuffer)` was
        // the text "[object ArrayBuffer]"), with its default Content-Type.
        let (koerper, body_type) = match self.arg(args, 0) {
            GVal::Undefiniert | GVal::Null => (GVal::Undefiniert, None),
            v => {
                let (bytes, typ) = if matches!(v, GVal::Ref(r) if self.eigene_lese(r, "#es_dv_puffer").is_some()) {
                    (self.buffer_source_bytes(v)?, None)
                } else {
                    self.koerper_extrahieren(v)?
                };
                (self.new_array_buffer(bytes)?, typ)
            }
        };
        let marke = self.wurzel_merke();
        self.wurzeln.push(koerper);
        let (status, kopf_init, status_text) = match self.arg(args, 1) {
            GVal::Ref(_) => {
                let o = self.arg(args, 1);
                let s = self.hole(o, "status").unwrap_or(GVal::Undefiniert);
                let s = if matches!(s, GVal::Undefiniert | GVal::Null) {
                    200.0
                } else {
                    self.zu_zahl(s).unwrap_or(200.0)
                };
                let st = match self.hole(o, "statusText").unwrap_or(GVal::Undefiniert) {
                    GVal::Undefiniert => String::new(),
                    v => self.zu_text(v).unwrap_or_default(),
                };
                (s, self.hole(o, "headers").unwrap_or(GVal::Undefiniert), st)
            }
            _ => (200.0, GVal::Undefiniert, String::new()),
        };
        if !(200.0..=599.0).contains(&status) {
            self.wurzel_zurueck(marke);
            return Err(Stoerung::Wurf(
                self.fehler_wert("RangeError", "Antwort-Status 200-599"),
            ));
        }
        // The headers are a copy made with the Headers prototype (before,
        // the Response constructor's prototype became the headers'
        // prototype, so `response.headers.get` was undefined).
        let headers_ctor = self.headers_ctor_ref(ctor_ref);
        let kopf = match kopf_init {
            GVal::Ref(_) => self.n_kopf_ctor(headers_ctor, &[kopf_init])?,
            _ => self.n_kopf_ctor(headers_ctor, &[])?,
        };
        self.wurzeln.push(kopf);
        if let Some(t) = body_type {
            self.headers_append_if_missing(kopf, "content-type", &t)?;
        }
        let ctx = self.feld_aus_werten(&[koerper, GVal::Zahl(status), kopf])?;
        self.wurzeln.push(GVal::Ref(ctx));
        let st = self.text_wert(&status_text).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(st);
        let obj = HObj::Nativ {
            name: "Response".to_string(),
            proto: Some(proto),
            ziel: Some(ctx),
            eigenschaften: Default::default(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        let _ = self.vor_allok(kosten);
        let r = self.belege(obj, kosten);
        self.setze(GVal::Ref(r), "status", GVal::Zahl(status))?;
        self.setze(
            GVal::Ref(r),
            "ok",
            GVal::Bool((200.0..300.0).contains(&status)),
        )?;
        self.setze(GVal::Ref(r), "headers", kopf)?;
        self.setze(GVal::Ref(r), "statusText", st)?;
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(r))
    }

    /// The native Headers constructor (for headers made by Request and
    /// Response); `fallback` when the global is gone.
    fn headers_ctor_ref(&mut self, fallback: HeapRef) -> HeapRef {
        match self.hole_ohne_host(GVal::Ref(self.global_objekt), "Headers") {
            Ok(GVal::Ref(c)) => c,
            _ => fallback,
        }
    }

    /// Appends `name: value` to a native Headers object unless the name is
    /// present (names are stored lower-case).
    fn headers_append_if_missing(&mut self, headers: GVal, name: &str, value: &str) -> Result<(), Stoerung> {
        let GVal::Ref(h) = headers else { return Ok(()) };
        let Some(z) = (match self.heap.holen(h) {
            HObj::Nativ { ziel: Some(z), .. } => Some(*z),
            _ => None,
        }) else {
            return Ok(());
        };
        let elemente = match self.heap.holen(z) {
            HObj::Feld { elemente, .. } => elemente.clone(),
            _ => return Ok(()),
        };
        for k in elemente.iter().step_by(2) {
            if self.zu_text(*k).unwrap_or_default() == name {
                return Ok(());
            }
        }
        let marke = self.wurzel_merke();
        let n = self.text_wert(name).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(n);
        let v = self.text_wert(value).map_err(Stoerung::Fehler)?;
        if let HObj::Feld { elemente, .. } = self.heap.holen_mut(z) {
            elemente.push(n);
            elemente.push(v);
        }
        self.wurzel_zurueck(marke);
        Ok(())
    }

    /// 2026-10-15: `Response.prototype.arrayBuffer()` / `bytes()` for
    /// constructed responses (the body slot holds bytes or text).
    pub(crate) fn n_antwort_puffer(&mut self, this: GVal, as_uint8: bool) -> Result<GVal, Stoerung> {
        let z = match this {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Nativ { ziel: Some(zz), name, .. } if name == "Response" => *zz,
                _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Illegal invocation"))),
            },
            _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Illegal invocation"))),
        };
        let bytes = self.response_body_bytes(z);
        let buf = self.new_array_buffer(bytes)?;
        let out = if as_uint8 { self.uint8_array_over(buf)? } else { buf };
        Ok(self.versprechen_erfuellt(out))
    }

    /// The body of a constructed Response (slot 0: bytes, text or nothing).
    fn response_body_bytes(&mut self, ctx: HeapRef) -> Vec<u8> {
        let koerper = match self.heap.holen(ctx) {
            HObj::Feld { elemente, .. } => elemente.first().copied().unwrap_or(GVal::Undefiniert),
            _ => GVal::Undefiniert,
        };
        match koerper {
            GVal::Undefiniert | GVal::Null => Vec::new(),
            GVal::Ref(b) if matches!(self.heap.holen(b), HObj::BytePuffer { .. }) => match self.heap.holen(b) {
                HObj::BytePuffer { daten, .. } => daten.clone(),
                _ => Vec::new(),
            },
            v => self.zu_text(v).unwrap_or_default().into_bytes(),
        }
    }

    /// `new Uint8Array(buffer)` with the global constructor.
    pub(crate) fn uint8_array_over(&mut self, buffer: GVal) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        self.wurzeln.push(buffer);
        let ctor = self.hole_ohne_host(GVal::Ref(self.global_objekt), "Uint8Array").unwrap_or(GVal::Undefiniert);
        let out = match ctor {
            GVal::Ref(c) => self.n_typedarray_ctor_uint8(c, &[buffer]),
            _ => Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Uint8Array is not available"))),
        };
        self.wurzel_zurueck(marke);
        out
    }

    /// 2026-10-15: `Blob.prototype.bytes()`.
    pub(crate) fn n_klecks_bytes(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let bytes = self.klecks_bytes(this)?;
        let buf = self.new_array_buffer(bytes)?;
        let out = self.uint8_array_over(buf)?;
        Ok(self.versprechen_erfuellt(out))
    }

    /// Antwort-Koerper lesen (Feld-Index 0). text()/json() -> Promise.
    pub(crate) fn n_antwort_koerper(
        &mut self,
        this: GVal,
        als_json: bool,
    ) -> Result<GVal, Stoerung> {
        let z = match this {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Nativ { ziel: Some(zz), .. } => zz,
                _ => {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("TypeError", "Antwort braucht Response-Objekt"),
                    ));
                }
            },
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Antwort braucht Response-Objekt"),
                ));
            }
        };
        // 2026-10-15: the body is bytes (UTF-8 decoded without a BOM) or,
        // for older callers, text; no body reads as "".
        let bytes = self.response_body_bytes(z);
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&bytes);
        let text = String::from_utf8_lossy(bytes).into_owned();
        let koerper = self.text_wert(&text).map_err(Stoerung::Fehler)?;
        // Release 15.10.2026: rooted (the JSON branch allocates first).
        self.wurzeln.push(koerper);
        let wert = if als_json {
            let t = text;
            // Leerer Koerper -> SyntaxError (Spec); sonst JSON parsen.
            if t.trim().is_empty() {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("SyntaxError", "Antwort leer"),
                ));
            }
            let tt = self.text_wert(&t).unwrap_or(GVal::Undefiniert);
            self.wurzeln.push(tt);
            self.n_json_lesen(&[tt])?
        } else {
            koerper
        };
        Ok(self.versprechen_erfuellt(wert))
    }

    /// Release 10.10.2026: ein Blob aus BYTES (File API §3). Vorher war der
    /// Inhalt ein UTF-8-Text: ein `Uint8Array` als Teil wurde zu
    /// `"72,97,108,..."`, und jede Datei mit Bytes ueber 0x7F (Bilder, PDFs)
    /// ging beim Hochladen kaputt.
    pub(crate) fn klecks_bauen(&mut self, proto: HeapRef, bytes: &[u8], typ: &str) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        let puffer = self.neuer_bytepuffer(bytes.len(), false)?;
        self.wurzeln.push(GVal::Ref(puffer));
        if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(puffer) {
            daten.copy_from_slice(bytes);
        }
        let tt = self.text_wert(typ).unwrap_or(GVal::Undefiniert);
        self.wurzeln.push(tt);
        let ctx = self.feld_aus_werten(&[GVal::Ref(puffer), GVal::Zahl(bytes.len() as f64), tt])?;
        self.wurzeln.push(GVal::Ref(ctx));
        let obj = HObj::Nativ {
            name: "Blob".to_string(),
            proto: Some(proto),
            ziel: Some(ctx),
            eigenschaften: Default::default(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        let _ = self.vor_allok(kosten);
        let r = self.belege(obj, kosten);
        self.wurzel_zurueck(marke);
        self.setze(GVal::Ref(r), "size", GVal::Zahl(bytes.len() as f64))?;
        self.setze(GVal::Ref(r), "type", tt)?;
        Ok(GVal::Ref(r))
    }

    /// Release 10.10.2026: die Bytes EINES Blob-Teils (File API §4.1 „process
    /// blob parts"): ArrayBuffer, TypedArray und DataView roh, ein Blob mit
    /// seinen Bytes, alles andere als Text in UTF-8.
    fn klecks_teil_bytes(&mut self, v: GVal, raus: &mut Vec<u8>) -> Result<(), Stoerung> {
        if let GVal::Ref(r) = v {
            match self.heap.holen(r) {
                HObj::BytePuffer { daten, .. } => {
                    raus.extend_from_slice(daten);
                    return Ok(());
                }
                HObj::TypedArray { .. } => {
                    if let Some(b) = self.heap.typedarray_bytes(r) {
                        raus.extend_from_slice(&b);
                    }
                    return Ok(());
                }
                HObj::Nativ { name, .. } if name == "Blob" => {
                    let b = self.klecks_bytes(v)?;
                    raus.extend_from_slice(&b);
                    return Ok(());
                }
                // Eine DataView ist ein Objekt mit dem versteckten Feld
                // `__puffer` (siehe `dataview_teile`).
                HObj::Gegenstand { .. }
                    if matches!(self.eigene_lese(r, "#es_dv_puffer"), Some(GVal::Ref(p)) if matches!(self.heap.holen(p), HObj::BytePuffer { .. })) =>
                {
                    let (p, z, n) = self.dataview_teile(v)?;
                    if let HObj::BytePuffer { daten, .. } = self.heap.holen(p) {
                        let von = z.min(daten.len());
                        let bis = z.saturating_add(n).min(daten.len());
                        raus.extend_from_slice(&daten[von..bis]);
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
        raus.extend_from_slice(self.zu_text(v).unwrap_or_default().as_bytes());
        Ok(())
    }

    /// Release 10.10.2026: der Typ aus den Optionen (`{type}`), klein.
    fn klecks_typ(&mut self, optionen: GVal) -> String {
        match optionen {
            GVal::Ref(_) => {
                let t = self.hole(optionen, "type").unwrap_or(GVal::Undefiniert);
                if matches!(t, GVal::Undefiniert) {
                    String::new()
                } else {
                    self.zu_text(t).unwrap_or_default().to_lowercase()
                }
            }
            _ => String::new(),
        }
    }

    /// Release 10.10.2026: die Teile (`[teil, ...]`) zu Bytes.
    fn klecks_teile(&mut self, teile: GVal) -> Result<Vec<u8>, Stoerung> {
        let mut bytes = Vec::new();
        if let GVal::Ref(r) = teile
            && let HObj::Feld { elemente, .. } = self.heap.holen(r).clone()
        {
            for e in elemente {
                self.klecks_teil_bytes(e, &mut bytes)?;
            }
        }
        Ok(bytes)
    }

    /// new Blob(teile?, {type}) — Release 10.10.2026: aus Bytes.
    pub(crate) fn n_klecks_ctor(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let proto = self.abbruch_proto(ctor_ref);
        let bytes = self.klecks_teile(self.arg(args, 0))?;
        let typ = self.klecks_typ(self.arg(args, 1));
        self.klecks_bauen(proto, &bytes, &typ)
    }

    /// new File(teile, name, {type, lastModified}): wie Blob + `name` und
    /// `lastModified` (Release 10.10.2026: die Optionen sind `args[2]`, nicht
    /// der Name — vorher blieb `type` leer).
    pub(crate) fn n_datei_ctor(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let proto = self.abbruch_proto(ctor_ref);
        let bytes = self.klecks_teile(self.arg(args, 0))?;
        let optionen = self.arg(args, 2);
        let typ = self.klecks_typ(optionen);
        let b = self.klecks_bauen(proto, &bytes, &typ)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(b);
        let name = self.zu_text(self.arg(args, 1)).unwrap_or_default();
        let nt = self.text_wert(&name).unwrap_or(GVal::Undefiniert);
        self.setze(b, "name", nt)?;
        let zeit = match optionen {
            GVal::Ref(_) => match self.hole(optionen, "lastModified").unwrap_or(GVal::Undefiniert) {
                GVal::Undefiniert => None,
                v => self.zu_zahl(v).ok(),
            },
            _ => None,
        };
        let zeit = zeit.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_millis() as f64)
        });
        self.setze(b, "lastModified", GVal::Zahl(zeit.trunc()))?;
        let leer = self.text_wert("").unwrap_or(GVal::Undefiniert);
        self.setze(b, "webkitRelativePath", leer)?;
        self.wurzel_zurueck(marke);
        Ok(b)
    }

    /// Release 10.10.2026: die Bytes eines Blob (Feld-Index 0: Byte-Puffer;
    /// ein Text aus einem alten Stand wird als UTF-8 gelesen).
    pub(crate) fn klecks_bytes(&mut self, this: GVal) -> Result<Vec<u8>, Stoerung> {
        let z = match this {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Nativ { name, ziel: Some(z), .. } if name == "Blob" => *z,
                _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Blob braucht Blob-Objekt"))),
            },
            _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Blob braucht Blob-Objekt"))),
        };
        let erstes = match self.heap.holen(z) {
            HObj::Feld { elemente, .. } => elemente.first().copied().unwrap_or(GVal::Undefiniert),
            _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Klecks korrupt"))),
        };
        if let GVal::Ref(p) = erstes
            && let HObj::BytePuffer { daten, .. } = self.heap.holen(p)
        {
            return Ok(daten.clone());
        }
        Ok(self.zu_text(erstes).unwrap_or_default().into_bytes())
    }

    /// blob.text() -> Promise(Daten). slice(anfang?, ende?, typ?) ->
    /// neuer Blob (Byte-Grenzen, negativ = ab Ende).
    pub(crate) fn n_klecks(
        &mut self,
        ctor_ref: HeapRef,
        this: GVal,
        args: &[GVal],
        schneiden: bool,
    ) -> Result<GVal, Stoerung> {
        let bytes = self.klecks_bytes(this)?;
        if !schneiden {
            let t = self.text_wert(&String::from_utf8_lossy(&bytes)).unwrap_or(GVal::Undefiniert);
            return Ok(self.versprechen_erfuellt(t));
        }
        let n = bytes.len() as i64;
        let klammer = |v: f64| -> usize {
            let x = v.trunc() as i64;
            let x = if x < 0 { (n + x).max(0) } else { x.min(n) };
            x as usize
        };
        let a = match self.arg(args, 0) {
            GVal::Undefiniert => 0,
            v => klammer(self.zu_zahl(v).unwrap_or(0.0)),
        };
        let b = match self.arg(args, 1) {
            GVal::Undefiniert => n as usize,
            v => klammer(self.zu_zahl(v).unwrap_or(0.0)),
        };
        let (a, b) = (a.min(b), b.max(a));
        let typ = match self.arg(args, 2) {
            GVal::Undefiniert => String::new(),
            t => self.zu_text(t).unwrap_or_default().to_lowercase(),
        };
        // Release 10.10.2026: der Prototyp des BLOBS (`ctor_ref` ist hier
        // die Funktion `slice` — das Stueck hatte vorher kein `text()`).
        let proto = match this {
            GVal::Ref(r) => self.heap_proto_von(r).unwrap_or_else(|| self.abbruch_proto(ctor_ref)),
            _ => self.abbruch_proto(ctor_ref),
        };
        self.klecks_bauen(proto, &bytes[a.min(bytes.len())..b.min(bytes.len())], &typ)
    }

    /// Release 10.10.2026: blob.arrayBuffer() -> Promise(ArrayBuffer, Kopie).
    pub(crate) fn n_klecks_puffer(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let bytes = self.klecks_bytes(this)?;
        let p = self.neuer_bytepuffer(bytes.len(), false)?;
        if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(p) {
            daten.copy_from_slice(&bytes);
        }
        Ok(self.versprechen_erfuellt(GVal::Ref(p)))
    }

    /// Release 10.10.2026: der Koerper einer Anfrage (Fetch §5.2 „extract a
    /// body"): Text als UTF-8, `URLSearchParams` urlencoded, `FormData`
    /// mehrteilig, Blob mit seinem Typ, Puffer roh. Dazu der Content-Type,
    /// den die Anfrage bekommt, wenn sie keinen setzt.
    pub(crate) fn koerper_extrahieren(&mut self, b: GVal) -> Result<(Vec<u8>, Option<String>), Stoerung> {
        if let GVal::Ref(r) = b {
            match self.heap.holen(r) {
                HObj::BytePuffer { daten, .. } => return Ok((daten.clone(), None)),
                HObj::TypedArray { .. } => return Ok((self.heap.typedarray_bytes(r).unwrap_or_default(), None)),
                HObj::Nativ { name, .. } if name == "Blob" => {
                    let bytes = self.klecks_bytes(b)?;
                    let t = self.hole(b, "type").unwrap_or(GVal::Undefiniert);
                    let typ = self.zu_text(t).unwrap_or_default();
                    return Ok((bytes, (!typ.is_empty()).then_some(typ)));
                }
                HObj::Nativ { name, .. } if name == "FormData" => {
                    let z = self.formular_feld(b)?;
                    let el = match self.heap.holen(z).clone() {
                        HObj::Feld { elemente, .. } => elemente,
                        _ => Vec::new(),
                    };
                    let zufall = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_nanos() as u64);
                    let grenze = format!("----NarsilFormBoundary{zufall:016x}");
                    let name_fix = |n: &str| n.replace('"', "%22").replace('\r', "%0D").replace('\n', "%0A");
                    let mut raus: Vec<u8> = Vec::new();
                    let mut i = 0;
                    while i + 1 < el.len() {
                        let name = self.zu_text(el[i]).unwrap_or_default();
                        let wert = el[i + 1];
                        let ist_blob = matches!(wert, GVal::Ref(w) if matches!(self.heap.holen(w), HObj::Nativ { name, .. } if name == "Blob"));
                        if ist_blob {
                            let dateiname = match wert {
                                GVal::Ref(w) => match self.eigene_lese(w, "name") {
                                    Some(n) => self.zu_text(n).unwrap_or_else(|_| "blob".to_string()),
                                    None => "blob".to_string(),
                                },
                                _ => "blob".to_string(),
                            };
                            let t = self.hole(wert, "type").unwrap_or(GVal::Undefiniert);
                            let typ = self.zu_text(t).unwrap_or_default();
                            let typ = if typ.is_empty() { "application/octet-stream".to_string() } else { typ };
                            raus.extend_from_slice(format!(
                                "--{grenze}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {typ}\r\n\r\n",
                                name_fix(&name),
                                name_fix(&dateiname)
                            ).as_bytes());
                            let bytes = self.klecks_bytes(wert)?;
                            raus.extend_from_slice(&bytes);
                        } else {
                            let t = self.zu_text(wert).unwrap_or_default();
                            raus.extend_from_slice(format!(
                                "--{grenze}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n",
                                name_fix(&name)
                            ).as_bytes());
                            raus.extend_from_slice(t.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n").as_bytes());
                        }
                        raus.extend_from_slice(b"\r\n");
                        i += 2;
                    }
                    raus.extend_from_slice(format!("--{grenze}--\r\n").as_bytes());
                    return Ok((raus, Some(format!("multipart/form-data; boundary={grenze}"))));
                }
                _ => {}
            }
            // `URLSearchParams` (Klasse im Motor oder im Vorspann).
            let ist_suchparameter = self
                .heap_proto_von(r)
                .and_then(|p| self.eigene_lese(p, "constructor"))
                .and_then(|c| match c {
                    GVal::Ref(c) => self.eigene_lese(c, "name"),
                    _ => None,
                })
                .is_some_and(|n| self.zu_text(n).unwrap_or_default() == "URLSearchParams");
            if ist_suchparameter {
                let t = self.zu_text(b).unwrap_or_default();
                return Ok((t.into_bytes(), Some("application/x-www-form-urlencoded;charset=UTF-8".to_string())));
            }
        }
        let t = self.zu_text(b).unwrap_or_default();
        Ok((t.into_bytes(), Some("text/plain;charset=UTF-8".to_string())))
    }

    /// Formular-Eintraege lesen (Feld [name, wert, ...] aus ziel).
    fn formular_feld(&mut self, this: GVal) -> Result<HeapRef, Stoerung> {
        match this {
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::Nativ { ziel: Some(z), .. } => Ok(*z),
                _ => Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "FormData braucht FormData-Objekt"),
                )),
            },
            _ => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "FormData braucht FormData-Objekt"),
            )),
        }
    }

    /// Release 12.10.2026: `keys`/`values`/`entries` als Iterator (ueber
    /// eine Kopie der Liste, wie die Spec-Iteratoren ueber einen Index) und
    /// `forEach(callback(value, key, formData), thisArg)`.
    fn formular_lauf(&mut self, this: GVal, args: &[GVal], art: u8, el: &[GVal]) -> Result<GVal, Stoerung> {
        if art == 9 {
            let f = self.arg(args, 0);
            if !self.ist_aufrufbar(f) {
                return Err(Stoerung::Wurf(self.fehler_wert(
                    "TypeError",
                    "Failed to execute 'forEach' on 'FormData': The callback provided as parameter 1 is not a function.",
                )));
            }
            let dies = self.arg(args, 1);
            let mut k = 0;
            while k + 1 < el.len() {
                self.aufruf_synchron(f, dies, &[el[k + 1], el[k], this], false)?;
                k += 2;
            }
            return Ok(GVal::Undefiniert);
        }
        let feld = self.neues_feld().map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(feld));
        let mut k = 0;
        while k + 1 < el.len() {
            let w = match art {
                6 => el[k],
                7 => el[k + 1],
                _ => {
                    let p = self.neues_feld().map_err(Stoerung::Fehler)?;
                    self.feld_anhaengen(feld, GVal::Ref(p))?;
                    self.feld_anhaengen(p, el[k])?;
                    self.feld_anhaengen(p, el[k + 1])?;
                    k += 2;
                    continue;
                }
            };
            self.feld_anhaengen(feld, w)?;
            k += 2;
        }
        self.feld_zum_iterator(feld)
    }

    /// new FormData(): leere Eintragsliste.
    pub(crate) fn n_formular_ctor(&mut self, ctor_ref: HeapRef) -> Result<GVal, Stoerung> {
        let proto = self.abbruch_proto(ctor_ref);
        let ctx = self.feld_aus_werten(&[])?;
        let obj = HObj::Nativ {
            name: "FormData".to_string(),
            proto: Some(proto),
            ziel: Some(ctx),
            eigenschaften: Default::default(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        let _ = self.vor_allok(kosten);
        Ok(GVal::Ref(self.belege(obj, kosten)))
    }

    /// FormData-Methoden: 0=append, 1=delete, 2=get, 3=has, 4=set,
    /// 5=getAll, 6=keys, 7=values, 8=entries, 9=forEach. Werte: Text oder
    /// File.
    pub(crate) fn n_formular(
        &mut self,
        this: GVal,
        args: &[GVal],
        art: u8,
    ) -> Result<GVal, Stoerung> {
        let z = self.formular_feld(this)?;
        if art >= 6 {
            let el = match self.heap.holen(z).clone() {
                HObj::Feld { elemente, .. } => elemente,
                _ => Vec::new(),
            };
            let marke = self.wurzel_merke();
            self.wurzeln.extend(el.iter().copied());
            let erg = self.formular_lauf(this, args, art, &el);
            self.wurzel_zurueck(marke);
            return erg;
        }
        let name = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let el = match self.heap.holen(z).clone() {
            HObj::Feld { elemente, .. } => elemente,
            _ => Vec::new(),
        };
        // Paare suchen.
        let mut treffer: Vec<usize> = Vec::new();
        let mut i = 0;
        while i + 1 < el.len() {
            if self.zu_text(el[i]).unwrap_or_default() == name {
                treffer.push(i);
            }
            i += 2;
        }
        match art {
            3 => return Ok(GVal::Bool(!treffer.is_empty())),
            2 => return Ok(treffer.first().map(|&k| el[k + 1]).unwrap_or(GVal::Null)),
            5 => {
                let alle: Vec<GVal> = treffer.iter().map(|&k| el[k + 1]).collect();
                let f = self.feld_aus_werten(&alle)?;
                return Ok(GVal::Ref(f));
            }
            1 => {
                // delete: alle Paare mit Namen entfernen.
                let mut neu: Vec<GVal> = Vec::new();
                let mut k = 0;
                while k + 1 < el.len() {
                    if self.zu_text(el[k]).unwrap_or_default() != name {
                        neu.push(el[k]);
                        neu.push(el[k + 1]);
                    }
                    k += 2;
                }
                let o = self.heap.holen_mut(z);
                if let HObj::Feld { elemente, .. } = o {
                    *elemente = neu;
                }
                return Ok(GVal::Undefiniert);
            }
            _ => {}
        }
        // append (0): ans Ende. set (4): erst loeschen, dann ans Ende.
        //
        // Release 10.10.2026: ein Blob/File bleibt ein OBJEKT (XHR §5.2
        // „create an entry"): ein Blob, der kein File ist, oder einer mit
        // drittem Argument wird ein File mit diesem Namen (sonst „blob").
        // Vorher wurde jeder Wert Text — ein Hochladen schickte
        // „[object Blob]".
        let roh = self.arg(args, 1);
        let ist_blob = matches!(roh, GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Nativ { name, .. } if name == "Blob"));
        let wert = if ist_blob {
            let dateiname = self.arg(args, 2);
            // Ein File traegt `name` als EIGENES Feld (ein Blob nicht).
            let hat_namen = matches!(roh, GVal::Ref(r) if matches!(self.eigene_lese(r, "name"), Some(GVal::Ref(_))));
            if hat_namen && matches!(dateiname, GVal::Undefiniert) {
                roh
            } else {
                let name = match dateiname {
                    GVal::Undefiniert if hat_namen => {
                        let n = self.hole(roh, "name").unwrap_or(GVal::Undefiniert);
                        self.zu_text(n).unwrap_or_default()
                    }
                    GVal::Undefiniert => "blob".to_string(),
                    n => self.zu_text(n).unwrap_or_default(),
                };
                let bytes = self.klecks_bytes(roh)?;
                let t = self.hole(roh, "type").unwrap_or(GVal::Undefiniert);
                let typ = self.zu_text(t).unwrap_or_default();
                let proto = match roh {
                    GVal::Ref(r) => self.heap_proto_von(r).unwrap_or(self.proto_objekt),
                    _ => self.proto_objekt,
                };
                let marke = self.wurzel_merke();
                self.wurzeln.push(roh);
                let f = self.klecks_bauen(proto, &bytes, &typ)?;
                self.wurzeln.push(f);
                let nt = self.text_wert(&name).unwrap_or(GVal::Undefiniert);
                self.setze(f, "name", nt)?;
                let jetzt = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0.0, |d| d.as_millis() as f64);
                self.setze(f, "lastModified", GVal::Zahl(jetzt))?;
                self.wurzel_zurueck(marke);
                f
            }
        } else {
            match roh {
                GVal::Undefiniert => self.text_wert("").unwrap_or(GVal::Undefiniert),
                v => {
                    let t = self.zu_text(v).unwrap_or_default();
                    self.text_wert(&t).unwrap_or(GVal::Undefiniert)
                }
            }
        };
        let nt = self.text_wert(&name).unwrap_or(GVal::Undefiniert);
        if art == 4 {
            let alt: Vec<GVal> = match self.heap.holen(z).clone() {
                HObj::Feld { elemente, .. } => elemente,
                _ => Vec::new(),
            };
            let mut gefiltert: Vec<GVal> = Vec::new();
            let mut k = 0;
            while k + 1 < alt.len() {
                if self.zu_text(alt[k]).unwrap_or_default() != name {
                    gefiltert.push(alt[k]);
                    gefiltert.push(alt[k + 1]);
                }
                k += 2;
            }
            if k < alt.len() {
                gefiltert.push(alt[k]);
            }
            let o = self.heap.holen_mut(z);
            if let HObj::Feld { elemente, .. } = o {
                *elemente = gefiltert;
            }
        }
        let o = self.heap.holen_mut(z);
        if let HObj::Feld { elemente, .. } = o {
            elemente.push(nt);
            elemente.push(wert);
        }
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn n_abbruch_signal_verboten(&mut self) -> Result<GVal, Stoerung> {
        Err(Stoerung::Wurf(self.fehler_wert(
            "TypeError",
            "AbortSignal ist kein Konstruktor",
        )))
    }

    /// AbortSignal.abort() statisch: bereits abgebrochenes Signal.
    pub(crate) fn n_abbruch_statisch(&mut self, ctor_ref: HeapRef) -> Result<GVal, Stoerung> {
        let proto = self.abbruch_proto(ctor_ref);
        self.abbruch_signal(proto, true)
    }

    /// crypto.getRandomValues(typedArray): fuellt ein TypedArray mit
    /// kryptografisch zufaelligen Werten (in-place Mutation, Spec-konform).
    ///
    /// Unterstuetzte Typen: Int8Array, Uint8Array, Uint8ClampedArray,
    /// Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array,
    /// Float64Array, BigInt64Array, BigUint64Array.
    ///
    /// Wirft TypeError, wenn:
    /// - Argument kein TypedArray ist
    /// - Argument ein DataView oder SharedArrayBuffer-View ist (nicht im Modell)
    pub(crate) fn n_krypto_zufall(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let ta = self.arg(args, 0);
        let GVal::Ref(r) = ta else {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                "getRandomValues braucht ein TypedArray",
            )));
        };
        // Pruefen, dass es ein TypedArray ist (und kein DataView/ArrayBuffer).
        // 2026-10-15: a DataView is an ArrayBufferView of the wrong kind:
        // TypeMismatchError (WebCrypto 10.1.1), not TypeError.
        if matches!(self.eigene_lese(r, "#es_dv_puffer"), Some(GVal::Ref(_))) {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeMismatchError", "getRandomValues: DataView is not an integer TypedArray")));
        }
        if !self.heap.ist_typedarray(r) {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                "getRandomValues braucht ein TypedArray",
            )));
        }
        if let HObj::TypedArray { art, laenge, .. } = self.heap.holen(r) {
            if matches!(art, crate::eingebaut_typed::ElementArt::Float32 | crate::eingebaut_typed::ElementArt::Float64 | crate::eingebaut_typed::ElementArt::Float16) {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeMismatchError", "getRandomValues braucht ein Integer-TypedArray")));
            }
            if laenge.saturating_mul(art.bytes()) > 65_536 {
                return Err(Stoerung::Wurf(self.fehler_wert("QuotaExceededError", "getRandomValues maximal 65536 Bytes")));
            }
        }
        // Mutablen Byte-Slice holen und mit Zufall fuellen.
        let bytes = match self.heap.typedarray_bytes_mut(r) {
            Some(b) => b,
            None => {
                return Err(Stoerung::Wurf(self.fehler_wert(
                    "TypeError",
                    "getRandomValues: TypedArray-Puffer nicht zugaenglich",
                )));
            }
        };
        // Zufaellige Bytes generieren (nutzt narsil-tls::zufall::bytes intern).
        crate::eingebaut_web::zufalls_bytes(bytes).map_err(|_| {
            Stoerung::Wurf(self.fehler_wert("Error", "kein Zufall verfuegbar"))
        })?;
        // Gleiches TypedArray zurueckgeben (in-place, wie Spec).
        Ok(GVal::Ref(r))
    }

    /// J-CACHE (05.10., PROBE_24): `new Node/Element/HTMLElement()` wirft
    /// immer (illegal constructor, Spec) — die Existenz (`typeof Element`
    /// === "function") behebt „Element nicht definiert".
    /// Release 09.10.2026: die DOM-Schnittstellen, die Polyfills (ShadyDOM,
    /// Custom Elements) und Frameworks erwarten: `CharacterData`, `Text`,
    /// `Comment`, `CDATASection`, `ProcessingInstruction`,
    /// `DocumentFragment`, `ShadowRoot` — mit Prototypkette bis `Node` — und
    /// die Knoten-Eigenschaften/-Methoden als Prototyp-Mitglieder, die an den
    /// Host weiterreichen. Vorhandene Mitglieder bleiben (das interne DOM des
    /// Motors benutzt sie).
    fn dom_schnittstellen_anlegen(&mut self, knoten_proto: HeapRef, element_proto: HeapRef) -> Result<(), Stoerung> {
        let zeichen_proto = self.neues_objekt(Some(knoten_proto)).map_err(Stoerung::Fehler)?;
        if let HObj::Gegenstand { proto, .. } = self.heap.holen_mut(self.proto_dom_text) {
            *proto = Some(zeichen_proto);
        }
        let kommentar_proto = self.neues_objekt(Some(zeichen_proto)).map_err(Stoerung::Fehler)?;
        let fragment_proto = self.neues_objekt(Some(knoten_proto)).map_err(Stoerung::Fehler)?;
        let shadow_proto = self.neues_objekt(Some(fragment_proto)).map_err(Stoerung::Fehler)?;
        let cdata_proto = self.neues_objekt(Some(self.proto_dom_text)).map_err(Stoerung::Fehler)?;
        let pi_proto = self.neues_objekt(Some(zeichen_proto)).map_err(Stoerung::Fehler)?;
        // `Window`: das globale Objekt erbt wie im Browser von
        // `Window.prototype` -> `EventTarget.prototype` -> `Object.prototype`.
        // GEMESSEN (youtube.com): ShadyDOM legt `__shady_native_addEventListener`
        // auf die Prototypen und ruft es an `window` auf — ohne Kette fehlte es.
        let fenster_proto = self.neues_objekt(Some(self.proto_ereignisziel)).map_err(Stoerung::Fehler)?;
        let global = self.global_objekt;
        let objekt_proto = self.proto_objekt;
        if let HObj::Gegenstand { proto, .. } = self.heap.holen_mut(global)
            && (proto.is_none() || *proto == Some(objekt_proto))
        {
            *proto = Some(fenster_proto);
        }
        for (name, proto) in [
            ("Window", fenster_proto),
            ("CharacterData", zeichen_proto),
            ("Text", self.proto_dom_text),
            ("Comment", kommentar_proto),
            ("CDATASection", cdata_proto),
            ("ProcessingInstruction", pi_proto),
            ("DocumentFragment", fragment_proto),
            ("ShadowRoot", shadow_proto),
        ] {
            let ctor = self.nativ_anlegen(name, Nativ::WebSchnittstelle(name.to_string()), self.proto_funktion);
            self.nativ_prototyp(ctor, proto);
            self.setze_eigen(proto, "constructor", GVal::Ref(ctor));
            self.global_binden(name, GVal::Ref(ctor));
        }
        let fehlt = |e: &mut Self, ziel: HeapRef, name: &str| -> bool {
            match e.heap.holen(ziel) {
                HObj::Gegenstand { eigenschaften, .. } => !eigenschaften.contains_key(name),
                _ => false,
            }
        };
        // Lesbare Knoten-Eigenschaften (Node.prototype).
        for name in [
            "nextSibling", "previousSibling", "parentElement", "ownerDocument", "isConnected",
            "nodeName", "nodeType", "parentNode", "childNodes", "firstChild", "lastChild",
        ] {
            if fehlt(self, knoten_proto, name) {
                self.nativ_getter(knoten_proto, name, Nativ::HostLesen(name.to_string()));
            }
        }
        for name in ["textContent", "nodeValue"] {
            if fehlt(self, knoten_proto, name) {
                self.nativ_getter(knoten_proto, name, Nativ::HostLesen(name.to_string()));
                self.nativ_setter(knoten_proto, name, Nativ::HostSchreiben(name.to_string()));
            }
        }
        // CharacterData: `data`, `length`.
        self.nativ_getter(zeichen_proto, "data", Nativ::HostLesen("data".to_string()));
        self.nativ_setter(zeichen_proto, "data", Nativ::HostSchreiben("data".to_string()));
        self.nativ_getter(zeichen_proto, "length", Nativ::HostLesen("length".to_string()));
        // Element.prototype.
        for name in [
            "children", "firstElementChild", "lastElementChild", "nextElementSibling",
            "previousElementSibling", "childElementCount", "localName", "tagName", "shadowRoot",
            "attributes",
        ] {
            if fehlt(self, element_proto, name) {
                self.nativ_getter(element_proto, name, Nativ::HostLesen(name.to_string()));
            }
        }
        for name in ["innerHTML", "outerHTML"] {
            if fehlt(self, element_proto, name) {
                self.nativ_getter(element_proto, name, Nativ::HostLesen(name.to_string()));
                self.nativ_setter(element_proto, name, Nativ::HostSchreiben(name.to_string()));
            }
        }
        // ShadowRoot: `host`, `mode`; Fragment/ShadowRoot: Kinder wie Elemente.
        self.nativ_getter(shadow_proto, "host", Nativ::HostLesen("host".to_string()));
        for name in ["children", "firstElementChild", "lastElementChild", "childElementCount"] {
            self.nativ_getter(fragment_proto, name, Nativ::HostLesen(name.to_string()));
        }
        self.nativ_getter(shadow_proto, "innerHTML", Nativ::HostLesen("innerHTML".to_string()));
        self.nativ_setter(shadow_proto, "innerHTML", Nativ::HostSchreiben("innerHTML".to_string()));
        // Methoden: Node.prototype, Element.prototype, Document/Fragment.
        let methoden = |e: &mut Self, ziel: HeapRef, namen: &[&str]| {
            for name in namen {
                if fehlt(e, ziel, name) {
                    let f = e.nativ_anlegen(
                        name,
                        Nativ::HostMethode { methode: name.to_string(), name: name.to_string(), art: 0 },
                        e.proto_funktion,
                    );
                    e.setze_eigen(ziel, name, GVal::Ref(f));
                }
            }
        };
        methoden(self, knoten_proto, &[
            "appendChild", "insertBefore", "removeChild", "replaceChild", "cloneNode", "contains",
            "hasChildNodes", "getRootNode", "normalize", "isEqualNode", "isSameNode",
            "compareDocumentPosition",
        ]);
        methoden(self, zeichen_proto, &["remove", "before", "after", "replaceWith"]);
        methoden(self, element_proto, &[
            "getAttribute", "setAttribute", "removeAttribute", "hasAttribute", "toggleAttribute",
            "getAttributeNames", "hasAttributes", "querySelector", "querySelectorAll", "matches",
            "closest", "attachShadow", "insertAdjacentHTML", "insertAdjacentText",
            "insertAdjacentElement", "append", "prepend", "before", "after", "remove",
            "replaceWith", "replaceChildren", "getElementsByTagName", "getElementsByClassName",
            "webkitMatchesSelector",
        ]);
        methoden(self, fragment_proto, &[
            "querySelector", "querySelectorAll", "getElementById", "append", "prepend",
            "replaceChildren",
        ]);
        methoden(self, self.proto_dom_dokument, &[
            "createElement", "createElementNS", "createTextNode", "createComment",
            "createDocumentFragment", "importNode", "adoptNode", "getElementById",
            "querySelector", "querySelectorAll", "getElementsByTagName", "getElementsByClassName",
            "append", "prepend", "replaceChildren",
        ]);
        self.element_schnittstellen_anlegen(element_proto)?;
        Ok(())
    }

    /// Release 09.10.2026: die Element-Schnittstellen aus HTML §3.2.8 und
    /// SVG 2 §4 (`HTMLDivElement`, `HTMLTemplateElement`, `SVGSVGElement` …).
    /// GEMESSEN (youtube.com, kevlar_base): `HTMLTemplateElement` war nicht
    /// definiert; Seiten pruefen auch `x instanceof HTMLInputElement` u. a.
    fn element_schnittstellen_anlegen(&mut self, element_proto: HeapRef) -> Result<(), Stoerung> {
        let html = self.proto_dom_html;
        for p in [html, element_proto, self.proto_dom_image, self.proto_dom_text, self.proto_dom_dokument] {
            self.dom_eingebaut.insert(p);
        }
        let schnittstelle = |e: &mut Self, name: &str, eltern: HeapRef, eigen: Option<HeapRef>| -> Result<HeapRef, Stoerung> {
            let proto = match eigen {
                Some(p) => p,
                None => e.neues_objekt(Some(eltern)).map_err(Stoerung::Fehler)?,
            };
            let ctor = e.nativ_anlegen(name, Nativ::WebSchnittstelle(name.to_string()), e.proto_funktion);
            e.nativ_prototyp(ctor, proto);
            e.setze_eigen(proto, "constructor", GVal::Ref(ctor));
            e.global_binden(name, GVal::Ref(ctor));
            e.dom_eingebaut.insert(proto);
            Ok(proto)
        };
        let medien = schnittstelle(self, "HTMLMediaElement", html, None)?;
        let bild = self.proto_dom_image;
        let bild = schnittstelle(self, "HTMLImageElement", html, Some(bild))?;
        self.dom_tag_protos.insert("img".into(), bild);
        let liste: &[(&str, HeapRef, &[&str])] = &[
            ("HTMLAudioElement", medien, &["audio"][..]),
            ("HTMLVideoElement", medien, &["video"][..]),
            ("HTMLAnchorElement", html, &["a"][..]),
            ("HTMLAreaElement", html, &["area"][..]),
            ("HTMLBRElement", html, &["br"][..]),
            ("HTMLBaseElement", html, &["base"][..]),
            ("HTMLBodyElement", html, &["body"][..]),
            ("HTMLButtonElement", html, &["button"][..]),
            ("HTMLCanvasElement", html, &["canvas"][..]),
            ("HTMLDListElement", html, &["dl"][..]),
            ("HTMLDataElement", html, &["data"][..]),
            ("HTMLDataListElement", html, &["datalist"][..]),
            ("HTMLDetailsElement", html, &["details"][..]),
            ("HTMLDialogElement", html, &["dialog"][..]),
            ("HTMLDivElement", html, &["div"][..]),
            ("HTMLEmbedElement", html, &["embed"][..]),
            ("HTMLFieldSetElement", html, &["fieldset"][..]),
            ("HTMLFormElement", html, &["form"][..]),
            ("HTMLHeadingElement", html, &["h1", "h2", "h3", "h4", "h5", "h6"][..]),
            ("HTMLHeadElement", html, &["head"][..]),
            ("HTMLHRElement", html, &["hr"][..]),
            ("HTMLHtmlElement", html, &["html"][..]),
            ("HTMLIFrameElement", html, &["iframe"][..]),
            ("HTMLInputElement", html, &["input"][..]),
            ("HTMLLIElement", html, &["li"][..]),
            ("HTMLLabelElement", html, &["label"][..]),
            ("HTMLLegendElement", html, &["legend"][..]),
            ("HTMLLinkElement", html, &["link"][..]),
            ("HTMLMapElement", html, &["map"][..]),
            ("HTMLMetaElement", html, &["meta"][..]),
            ("HTMLMeterElement", html, &["meter"][..]),
            ("HTMLModElement", html, &["del", "ins"][..]),
            ("HTMLOListElement", html, &["ol"][..]),
            ("HTMLObjectElement", html, &["object"][..]),
            ("HTMLOptGroupElement", html, &["optgroup"][..]),
            ("HTMLOptionElement", html, &["option"][..]),
            ("HTMLOutputElement", html, &["output"][..]),
            ("HTMLParagraphElement", html, &["p"][..]),
            ("HTMLPictureElement", html, &["picture"][..]),
            ("HTMLPreElement", html, &["pre", "listing", "xmp"][..]),
            ("HTMLProgressElement", html, &["progress"][..]),
            ("HTMLQuoteElement", html, &["q", "blockquote"][..]),
            ("HTMLScriptElement", html, &["script"][..]),
            ("HTMLSelectElement", html, &["select"][..]),
            ("HTMLSlotElement", html, &["slot"][..]),
            ("HTMLSourceElement", html, &["source"][..]),
            ("HTMLSpanElement", html, &["span"][..]),
            ("HTMLStyleElement", html, &["style"][..]),
            ("HTMLTableCaptionElement", html, &["caption"][..]),
            ("HTMLTableCellElement", html, &["td", "th"][..]),
            ("HTMLTableColElement", html, &["col", "colgroup"][..]),
            ("HTMLTableElement", html, &["table"][..]),
            ("HTMLTableRowElement", html, &["tr"][..]),
            ("HTMLTableSectionElement", html, &["tbody", "thead", "tfoot"][..]),
            ("HTMLTemplateElement", html, &["template"][..]),
            ("HTMLTextAreaElement", html, &["textarea"][..]),
            ("HTMLTimeElement", html, &["time"][..]),
            ("HTMLTitleElement", html, &["title"][..]),
            ("HTMLTrackElement", html, &["track"][..]),
            ("HTMLUListElement", html, &["ul"][..]),
            ("HTMLUnknownElement", html, &[][..]),
            ("HTMLMenuElement", html, &["menu"][..]),
            ("HTMLMarqueeElement", html, &["marquee"][..]),
            ("HTMLFrameSetElement", html, &["frameset"][..]),
            // Release 2026-10-15: `<frame>` (HTML §16.3.2).
            ("HTMLFrameElement", html, &["frame"][..]),
            ("HTMLParamElement", html, &["param"][..]),
            ("HTMLFontElement", html, &["font"][..]),
            ("HTMLDirectoryElement", html, &["dir"][..]),
        ];
        for (name, eltern, tags) in liste {
            let p = schnittstelle(self, name, *eltern, None)?;
            for t in *tags {
                self.dom_tag_protos.insert((*t).to_string(), p);
            }
            // Release 2026-10-15: the interface of unknown HTML names (no
            // local name starts with '#').
            if *name == "HTMLUnknownElement" {
                self.dom_tag_protos.insert("#unknown".into(), p);
            }
        }
        // SVG 2: SVGElement <- SVGGraphicsElement <- SVGGeometryElement.
        let svg = schnittstelle(self, "SVGElement", element_proto, None)?;
        let grafik = schnittstelle(self, "SVGGraphicsElement", svg, None)?;
        let geometrie = schnittstelle(self, "SVGGeometryElement", grafik, None)?;
        self.dom_tag_protos.insert("svg:*".into(), svg);
        // Release 2026-10-15: MathML elements get MathMLElement (MathML Core
        // §2.1.1) instead of a bare Element. MEASURED before: the prototype
        // chain of `createElementNS(MATHML, "math")` skipped any MathML
        // interface and `typeof MathMLElement` was "undefined".
        let mathml = schnittstelle(self, "MathMLElement", element_proto, None)?;
        self.dom_tag_protos.insert("mathml:*".into(), mathml);
        let liste: &[(&str, HeapRef, &str)] = &[
            ("SVGSVGElement", grafik, "svg"),
            ("SVGGElement", grafik, "g"),
            ("SVGDefsElement", grafik, "defs"),
            ("SVGUseElement", grafik, "use"),
            ("SVGImageElement", grafik, "image"),
            ("SVGTextElement", grafik, "text"),
            ("SVGForeignObjectElement", grafik, "foreignObject"),
            ("SVGAElement", grafik, "a"),
            ("SVGSymbolElement", svg, "symbol"),
            ("SVGTitleElement", svg, "title"),
            ("SVGStyleElement", svg, "style"),
            ("SVGPathElement", geometrie, "path"),
            ("SVGRectElement", geometrie, "rect"),
            ("SVGCircleElement", geometrie, "circle"),
            ("SVGEllipseElement", geometrie, "ellipse"),
            ("SVGLineElement", geometrie, "line"),
            ("SVGPolylineElement", geometrie, "polyline"),
            ("SVGPolygonElement", geometrie, "polygon"),
            ("SVGLinearGradientElement", svg, "linearGradient"),
            ("SVGRadialGradientElement", svg, "radialGradient"),
            ("SVGStopElement", svg, "stop"),
            ("SVGClipPathElement", svg, "clipPath"),
            ("SVGMaskElement", svg, "mask"),
            ("SVGPatternElement", svg, "pattern"),
            ("SVGFilterElement", svg, "filter"),
        ];
        for (name, eltern, tag) in liste {
            let p = schnittstelle(self, name, *eltern, None)?;
            self.dom_tag_protos.insert(format!("svg:{tag}"), p);
        }
        // Release 09.10.2026: `@@toStringTag` je Schnittstelle (WebIDL §3.7.3:
        // der Klassenname). GEMESSEN (Chrome): `String(document.body)` ->
        // `[object HTMLBodyElement]`; der Motor lieferte `[object Object]`.
        let protos: Vec<HeapRef> = self.dom_eingebaut.iter().copied().collect();
        for p in protos {
            let name = match self.eigene_lese(p, "constructor") {
                Some(GVal::Ref(c)) => match self.hole_ohne_host(GVal::Ref(c), "name") {
                    Ok(GVal::Ref(t)) => match self.heap.holen(t) {
                        HObj::Zeichenkette(s) if !s.is_empty() => s.clone(),
                        _ => continue,
                    },
                    _ => continue,
                },
                _ => continue,
            };
            if self.eigene_lese(p, "Symbol(Symbol.toStringTag)").is_some() {
                continue;
            }
            let Ok(t) = self.text_wert(&name) else { continue };
            if let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen_mut(p) {
                eigenschaften.insert(
                    "Symbol(Symbol.toStringTag)".to_string(),
                    crate::heap::Eigenschaft {
                        wert: t,
                        getter: None,
                        setter: None,
                        aufzählbar: false,
                        beschreibbar: false,
                        konfigurierbar: true,
                    },
                );
            }
        }
        Ok(())
    }

    pub(crate) fn n_verboten_ctor(&mut self, name: &str) -> Result<GVal, Stoerung> {
        Err(Stoerung::Wurf(self.fehler_wert(
            "TypeError",
            &format!("{name} ist kein Konstruktor"),
        )))
    }

    /// Beobachter-Instanz lesen: (art, rueckruf, ziele-Feld).
    fn beobachter_ctx(&mut self, this: GVal) -> Result<(u8, GVal, HeapRef), Stoerung> {
        let fr = match this {
            GVal::Ref(r) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Beobachter braucht Objekt"),
                ))
            }
        };
        let ctx = match self.heap.holen(fr).clone() {
            HObj::Nativ { ziel: Some(z), .. } => z,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Beobachter korrupt"),
                ))
            }
        };
        let el = match self.heap.holen(ctx).clone() {
            HObj::Feld { elemente, .. } => elemente,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Beobachter korrupt"),
                ))
            }
        };
        let art = match el.first() {
            Some(GVal::Zahl(n)) => *n as u8,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Beobachter korrupt"),
                ))
            }
        };
        let rueckruf = el.get(1).copied().unwrap_or(GVal::Undefiniert);
        let ziele = match el.get(2) {
            Some(GVal::Ref(z)) => *z,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Beobachter korrupt"),
                ))
            }
        };
        Ok((art, rueckruf, ziele))
    }

    /// new MutationObserver/IntersectionObserver/ResizeObserver(rueckruf).
    /// Rueckruf muss aufrufbar sein (Spec). Instanz: [art, rueckruf, ziele].
    pub(crate) fn n_beobachter_ctor(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
        art: u8,
    ) -> Result<GVal, Stoerung> {
        let rueckruf = self.arg(args, 0);
        if !self.ist_aufrufbar(rueckruf) {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                "Beobachter braucht Funktion",
            )));
        }
        let proto = self.abbruch_proto(ctor_ref);
        let leer = self.feld_aus_werten(&[])?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(rueckruf);
        self.wurzeln.push(GVal::Ref(leer));
        let ctx = self.feld_aus_werten(&[GVal::Zahl(art as f64), rueckruf, GVal::Ref(leer)])?;
        self.wurzel_zurueck(marke);
        let obj = HObj::Nativ {
            name: "Beobachter".to_string(),
            proto: Some(proto),
            ziel: Some(ctx),
            eigenschaften: Default::default(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        let _ = self.vor_allok(kosten);
        Ok(GVal::Ref(self.belege(obj, kosten)))
    }

    /// observe(ziel): Ziel merken; Kreuzung/Größe feuern EINMAL als
    /// Mikrotask (sichtbar/isIntersecting — dokumentierte Naeherung,
    /// kein Layout-Anschluss). Mutation feuert nicht von selbst (kein
    /// Mutations-Einzug — Sitzung/JS; takeRecords geht).
    pub(crate) fn n_beobachten(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let (art, rueckruf, ziele) = self.beobachter_ctx(this)?;
        let ziel = self.arg(args, 0);
        {
            let o = self.heap.holen_mut(ziele);
            if let HObj::Feld { elemente, .. } = o {
                if !elemente.contains(&ziel) {
                    elemente.push(ziel);
                }
            }
        }
        if art == 0 {
            // MutationObserver: kein automatisches Feuern (s. oben).
            return Ok(GVal::Undefiniert);
        }
        // Feuer-Wrapper als Mikrotask: erfuelltes Wegwerf-Promise +
        // eingereihte Reaktion (wie promise_dann_roh bei erfuellt).
        let huelle = self.beobachter_huelle(rueckruf, this, ziel, art)?;
        let p = match self.versprechen_anlegen(crate::heap::VersprechenZustand::Erfuellt(
            GVal::Undefiniert,
        )) {
            GVal::Ref(r) => r,
            _ => unreachable!(),
        };
        let folge = match self.versprechen_anlegen(crate::heap::VersprechenZustand::Offen) {
            GVal::Ref(r) => r,
            _ => unreachable!(),
        };
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(huelle));
        self.reaktion_einreihen(p, Some(huelle), None, folge);
        self.wurzel_zurueck(marke);
        Ok(GVal::Undefiniert)
    }

    /// Feuer-Huelle bauen: Nativ mit [rueckruf, beobachter, ziel, art].
    fn beobachter_huelle(
        &mut self,
        rueckruf: GVal,
        beobachter: GVal,
        ziel: GVal,
        art: u8,
    ) -> Result<HeapRef, Stoerung> {
        let ctx = self.feld_aus_werten(&[
            rueckruf,
            beobachter,
            ziel,
            GVal::Zahl(art as f64),
        ])?;
        let obj = HObj::Nativ {
            name: "beobachter_feuer".to_string(),
            proto: Some(self.proto_funktion),
            ziel: Some(ctx),
            eigenschaften: Default::default(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        let _ = self.vor_allok(kosten);
        let r = self.belege(obj, kosten);
        self.native.insert(r, Nativ::WebBeobachterFeuer);
        Ok(r)
    }

    /// Feuer-Wrapper (als Mikrotask gerufen): Eintraege bauen, Rueckruf
    /// mit (eintraege, beobachter) rufen.
    pub(crate) fn n_beobachter_feuer(
        &mut self,
        fn_ref: HeapRef,
        _args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let ctx = match self.heap.holen(fn_ref).clone() {
            HObj::Nativ { ziel: Some(z), .. } => z,
            _ => return Ok(GVal::Undefiniert),
        };
        let el = match self.heap.holen(ctx).clone() {
            HObj::Feld { elemente, .. } => elemente,
            _ => return Ok(GVal::Undefiniert),
        };
        let hol = |i: usize| el.get(i).copied().unwrap_or(GVal::Undefiniert);
        let (rueckruf, beobachter, ziel, art) = (hol(0), hol(1), hol(2), hol(3));
        let art = match art {
            GVal::Zahl(n) => n as u8,
            _ => 1,
        };
        // Eintrag: {isIntersecting: true, target, intersectionRatio: 1,
        // time: 0}. time 0 ist dokumentierte Naeherung (kein Zeitgeber).
        let eintrag = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
        self.setze(GVal::Ref(eintrag), "isIntersecting", GVal::Bool(true))?;
        self.setze(GVal::Ref(eintrag), "target", ziel)?;
        self.setze(GVal::Ref(eintrag), "intersectionRatio", GVal::Zahl(1.0))?;
        self.setze(GVal::Ref(eintrag), "time", GVal::Zahl(0.0))?;
        let liste = self.feld_aus_werten(&[GVal::Ref(eintrag)])?;
        let _ = self.aufruf_synchron(rueckruf, beobachter, &[GVal::Ref(liste), beobachter], false);
        Ok(GVal::Undefiniert)
    }

    /// disconnect(): Ziele vergessen.
    pub(crate) fn n_trennen(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let (_, _, ziele) = self.beobachter_ctx(this)?;
        let o = self.heap.holen_mut(ziele);
        if let HObj::Feld { elemente, .. } = o {
            elemente.clear();
        }
        Ok(GVal::Undefiniert)
    }

    /// takeRecords(): derzeit keine (kein Mutations-Einzug) -> leeres Feld.
    pub(crate) fn n_nehmen(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let _ = self.beobachter_ctx(this)?;
        let f = self.feld_aus_werten(&[])?;
        Ok(GVal::Ref(f))
    }

    /// crypto.randomUUID() (RFC 4122 v4, /dev/urandom, nur std).
    pub(crate) fn n_krypto_uuid(&mut self) -> Result<GVal, Stoerung> {
        match crate::eingebaut_web::zufalls_uuid() {
            Ok(u) => self.text_wert(&u).map_err(Stoerung::Fehler),
            Err(()) => Err(Stoerung::Wurf(
                self.fehler_wert("Error", "kein Zufall verfuegbar"),
            )),
        }
    }
}

/// Medienabfrage auswerten (J-CACHE 04.10.2026, Weisung CACHE_28).
/// Reine Funktion (testbar): Anfrage gegen Breite/Hoehe (CSS-px),
/// Farbschema ("dark"/"light"), Bewegung ("reduce"/"no-preference").
/// Unterstuetzt: screen/all/print-Typ, not/and/Komma-Oder, min-/max-width/
/// height (px), prefers-color-scheme, prefers-reduced-motion, orientation.
/// Unbekanntes -> false (konservativ, dokumentiert). Faelle aus
/// CSS-Spec-Semantik (node hat kein matchMedia — Browser-API).
/// `pub`, weil narsil/js.rs sie wiederverwendet (CACHE_28).
pub fn medien_treffer(
    anfrage: &str,
    breite: f64,
    hoehe: f64,
    farbschema: &str,
    bewegung: &str,
) -> bool {
    // Komma = Oder.
    for teil in anfrage.split(',') {
        if medien_und(teil.trim(), breite, hoehe, farbschema, bewegung) {
            return true;
        }
    }
    false
}

fn medien_und(teil: &str, breite: f64, hoehe: f64, farbschema: &str, bewegung: &str) -> bool {
    let mut t = teil.trim().to_lowercase();
    let mut verneint = false;
    if let Some(rest) = t.strip_prefix("not ") {
        verneint = true;
        t = rest.trim().to_string();
    } else if t.starts_with("only ") {
        t = t["only ".len()..].trim().to_string();
    }
    // Typ vorne (screen/all/print/sonst).
    let mut ergebnis = true;
    let mut rest = t.as_str();
    for typ in ["screen", "all", "print"] {
        if rest == typ {
            ergebnis = typ != "print";
            rest = "";
            break;
        }
        if let Some(r) = rest.strip_prefix(typ) {
            let r = r.trim_start();
            if r.starts_with("and ") {
                ergebnis = typ != "print";
                rest = r["and ".len()..].trim();
                break;
            }
        }
    }
    if ergebnis {
        for bedingung in rest.split(" and ") {
            let b = bedingung
                .trim()
                .trim_start_matches('(')
                .trim_end_matches(')');
            if !medien_bedingung(b.trim(), breite, hoehe, farbschema, bewegung) {
                ergebnis = false;
                break;
            }
        }
    }
    if verneint { !ergebnis } else { ergebnis }
}

fn medien_bedingung(b: &str, breite: f64, hoehe: f64, farbschema: &str, bewegung: &str) -> bool {
    // min-/max-width/height: Zahl + px (em/rem grob: 1em = 16px).
    for (name, wert) in [
        ("min-width", breite),
        ("max-width", breite),
        ("min-height", hoehe),
        ("max-height", hoehe),
    ] {
        if let Some(r) = b.strip_prefix(name) {
            let r = r.trim().strip_prefix(':').unwrap_or("").trim();
            let (zahl, einheit) = zahl_einheit(r);
            let px = if einheit == "em" || einheit == "rem" {
                zahl * 16.0
            } else {
                zahl
            };
            if name.starts_with("min-") {
                return wert >= px;
            } else {
                return wert <= px;
            }
        }
    }
    if let Some(r) = b.strip_prefix("prefers-color-scheme") {
        let r = r.trim().strip_prefix(':').unwrap_or("").trim();
        return r == farbschema;
    }
    if let Some(r) = b.strip_prefix("prefers-reduced-motion") {
        let r = r.trim().strip_prefix(':').unwrap_or("").trim();
        return r == bewegung;
    }
    if let Some(r) = b.strip_prefix("orientation") {
        let r = r.trim().strip_prefix(':').unwrap_or("").trim();
        let hoch = hoehe >= breite;
        return (r == "portrait") == hoch;
    }
    // min-/max-resolution grob (dpi/dppx/x): 1dppx = 96dpi.
    for name in ["min-resolution", "max-resolution"] {
        if let Some(r) = b.strip_prefix(name) {
            let r = r.trim().strip_prefix(':').unwrap_or("").trim();
            let (zahl, einheit) = zahl_einheit(r);
            let dpi = if einheit == "dppx" || einheit == "x" {
                zahl * 96.0
            } else {
                zahl
            };
            // Annahme: 96dpi-Anzeige (dokumentiert).
            if name.starts_with("min-") {
                return 96.0 >= dpi;
            } else {
                return 96.0 <= dpi;
            }
        }
    }
    false
}

/// Zahl + Einheit trennen ("700px" -> (700, "px")).
fn zahl_einheit(s: &str) -> (f64, String) {
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.' || b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let zahl = s[..i].parse::<f64>().unwrap_or(f64::NAN);
    (zahl, s[i..].trim().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_wege() {
        // node-Referenzen 04.10.2026.
        assert_eq!(btoa_encode("Hello"), Ok("SGVsbG8=".to_string()));
        assert_eq!(atob_decode("SGVsbG8="), Ok("Hello".to_string()));
        assert_eq!(atob_decode("YQ=="), Ok("a".to_string()));
        assert_eq!(atob_decode(""), Ok(String::new()));
        assert!(atob_decode("!!!").is_err());
        assert!(btoa_encode("✓").is_err());
        // Runde.
        let s = "Man ist, was man isst: 123!";
        assert_eq!(atob_decode(&btoa_encode(s).unwrap()).unwrap(), s);
    }

    #[test]
    fn medien_grundfaelle() {
        // CSS-Spec-Semantik an 1280x800/light/no-preference.
        let b = 1280.0;
        let h = 800.0;
        assert!(medien_treffer(
            "(min-width: 700px)",
            b,
            h,
            "light",
            "no-preference"
        ));
        assert!(!medien_treffer(
            "(min-width: 1400px)",
            b,
            h,
            "light",
            "no-preference"
        ));
        assert!(medien_treffer(
            "(max-width: 1400px)",
            b,
            h,
            "light",
            "no-preference"
        ));
        assert!(medien_treffer(
            "screen and (min-width: 700px)",
            b,
            h,
            "light",
            "no-preference"
        ));
        assert!(!medien_treffer("print", b, h, "light", "no-preference"));
        assert!(medien_treffer(
            "(prefers-color-scheme: light)",
            b,
            h,
            "light",
            "no-preference"
        ));
        assert!(!medien_treffer(
            "(prefers-color-scheme: dark)",
            b,
            h,
            "light",
            "no-preference"
        ));
        assert!(medien_treffer(
            "(prefers-color-scheme: dark)",
            b,
            h,
            "dark",
            "no-preference"
        ));
        assert!(medien_treffer(
            "(prefers-reduced-motion: no-preference)",
            b,
            h,
            "light",
            "no-preference"
        ));
        assert!(medien_treffer(
            "(orientation: landscape)",
            b,
            h,
            "light",
            "no-preference"
        ));
        assert!(!medien_treffer(
            "(orientation: portrait)",
            b,
            h,
            "light",
            "no-preference"
        ));
        assert!(medien_treffer("not print", b, h, "light", "no-preference"));
        assert!(medien_treffer(
            "(min-width: 2000px), (min-width: 700px)",
            b,
            h,
            "light",
            "no-preference"
        ));
        assert!(medien_treffer(
            "(min-width: 43.75em)",
            b,
            h,
            "light",
            "no-preference"
        ));
    }

    #[test]
    fn zufall_form() {
        // Form pruefen (Werte zufaellig): UUID v4 Muster 8-4-4-4-12,
        // Stellen 13 = '4', Stelle 17 in 89ab.
        let u = zufalls_uuid().unwrap();
        assert_eq!(u.len(), 36);
        assert_eq!(u.chars().nth(14), Some('4'));
        assert!(matches!(
            u.chars().nth(19),
            Some('8') | Some('9') | Some('a') | Some('b')
        ));
        let mut z = [0u8; 32];
        assert_eq!(zufalls_bytes(&mut z), Ok(32));
    }
}
