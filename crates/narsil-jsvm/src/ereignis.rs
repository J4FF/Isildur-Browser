//! A6: Ereignisse — der Host loest sie aus (`ereignis_senden`), das Skript
//! hoert darauf (`addEventListener`).
//!
//! WARUM eine eigene Liste und nicht der Host-Haken aus W6B: die Liste liegt
//! im Motor, damit **jedes** Objekt Ziel sein kann — nicht nur die
//! Host-Objekte, fuer die `narsil` einen Haken anmeldet. Fuer ein
//! DOM-Element genuegt es, dass `el` keinen eigenen `addEventListener`
//! anmeldet: die Suche laeuft dann ueber die Kette bis
//! `EventTarget.prototype` (das hier geholt wird).
//!
//! Die Semantik ist an node 22 gemessen (`EventTarget`/`Event` gibt es dort
//! seit 15) — `tests/ereignis_benutzer.rs` vergleicht beide.
//!
//! * Die Hoerer laufen in **Reihenfolge der Anmeldung**.
//! * Zweimal dieselbe Funktion anmelden -> sie laeuft zweimal (node auch so).
//! * `removeEventListener` nimmt **eine** Anmeldung weg.
//! * `{ once: true }` nimmt den Hoerer nach dem Lauf.
//! * `preventDefault()` wirkt nur bei `cancelable: true` (der Standard des
//!   Konstruktors); `dispatchEvent` gibt dann `false` zurueck — so wie der
//!   Host es braucht, um die Defaulthandlung zu unterlassen.
//! * `stopPropagation()` beendet die Hoerer **dieses** Ziels (das Aufsteigen
//!   ueber die Kette ist nicht abgebildet — dokumentierte Naeherung).
//!
//! Grenzen (M1–M9): hoechstens `max_hoerer` Anmeldungen je Motor, jede mit
//! benannter harter Obergrenze; die Liste beim Senden wird mit einem Zaehler
//! gegen Laufzeit geschuetzt.

use crate::Wert;
use crate::grenzen::Grenzen;
use crate::heap::{GVal, HObj, HeapRef};
use crate::interp::{Engine, Stoerung};

/// Harte Obergrenze der Anmeldungen je Motor (M4). Der Motor lebt fuer eine
/// Seite; 4096 Hoerer sind fuer sie reichlich.
pub(crate) const MAX_HOERER_HART: usize = 4096;
/// Harte Obergrenze fuer die Typ-Laenge eines Ereignistyps (M4).
pub(crate) const MAX_TYP_HART: usize = 128;

/// Eine Anmeldung.
///
/// Das Ziel ist entweder ein JS-Objekt (`ziel`) oder ein **Host-Objekt**
/// (`host_ziel` = die Element-Nummer des DOM-Hakens). GEMESSEN/Begruendung:
/// `Engine::host_objekt_als_wert(id)` liefert einen WERT (`{__host: id}`),
/// und beim Senden wird daraus ein NEUES Heap-Objekt — nach einem reinen
/// Heap-Vergleich fand der Motor die Anmeldung des Skripts nicht. Ueber die
/// Nummer stimmt es wieder.
pub(crate) struct Hoerer {
    /// Zielobjekt (ein Heap-Verweis, `KEIN_REF` = der globale Bereich).
    pub ziel: HeapRef,
    /// Nummer des Host-Objekts (DOM-Element), wenn das Ziel so kam.
    pub host_ziel: Option<u32>,
    /// Typ, z. B. `click` (Kleinschreibung).
    pub typ: String,
    /// Die JS-Funktion.
    pub funktion: HeapRef,
    /// `{ once: true }`.
    pub einmal: bool,
    /// Release 11.10.2026: `capture` (drittes Argument `true` oder
    /// `{ capture: true }`) — gehoert zur Identitaet der Anmeldung (DOM §2.7
    /// „flatten options").
    pub capture: bool,
    /// Release 10.10.2026: `{ signal }` — ist das Signal abgebrochen, ist
    /// die Anmeldung weg (DOM §2.7: „remove an event listener" beim Abbruch;
    /// hier vor jedem Aufruf geprueft und beim Abbruch geraeumt).
    pub signal: Option<HeapRef>,
}

impl Engine {
    /// Ein Ereignis zustellen: die Hoerer des Ziels laufen, danach die
    /// Mikrotasks.
    ///
    /// `gibt` sagt dem Host, ob die Defaulthandlung stattfinden darf
    /// (`false`, wenn `preventDefault()` gerufen wurde). Das ist die
    /// einzige Information, die der Host aus einem Ereignis braucht.
    ///
    /// Der Host ruft das aus **ausserhalb** des JS-Laufs (nach
    /// `ausfuehren`, neben `netz_schleife`). Laeuft es mitten im JS, sieht
    /// `ereignis_dispatched` es zuerst.
    ///
    /// `ziel` ist ein `Wert` aus dem JS-Bereich (der Host bekommt ihn von
    /// `Engine::host_gval()`), `eigenschaften` sind eigene Felder des
    /// Ereignisses (`ev.value`, `ev.key` …). Der Host kann den Wert nach
    /// dem Lauf wieder ausgeben lassen.
    pub fn ereignis_senden(
        &mut self,
        typ: &str,
        ziel: &Wert,
        eigenschaften: Vec<(String, Wert)>,
    ) -> Result<bool, String> {
        let gz = self.import_wert(ziel, 0).map_err(|e| e.to_string())?;
        match self.ereignis_senden_intern(typ, gz, eigenschaften) {
            Ok(f) => Ok(f),
            Err(e) => Err(self.stoerung_text(e)),
        }
    }

    /// `engine.ereignis_senden_abbrechbar(...)` — `preventDefault()` wirkt.
    pub fn ereignis_senden_abbrechbar(
        &mut self,
        typ: &str,
        ziel: &Wert,
        eigenschaften: Vec<(String, Wert)>,
    ) -> Result<bool, String> {
        let gz = self.import_wert(ziel, 0).map_err(|e| e.to_string())?;
        match self.ereignis_abbrechbar_intern(typ, gz, eigenschaften) {
            Ok(f) => Ok(f),
            Err(e) => Err(self.stoerung_text(e)),
        }
    }

    /// Ein Ereignis an den **globalen** Bereich (`window`).
    ///
    /// GEMESSEN/Begruendung: der globale Bereich ist ein Objekt des Motors
    /// (`global_objekt`); ein vom Host gebauter `Wert` waere ein anderes
    /// Objekt, deshalb gibt es dafür einen eigenen Aufruf.
    pub fn ereignis_senden_global(
        &mut self,
        typ: &str,
        eigenschaften: Vec<(String, Wert)>,
    ) -> Result<bool, String> {
        let ziel = GVal::Ref(self.global_objekt);
        match self.ereignis_senden_intern(typ, ziel, eigenschaften) {
            Ok(f) => Ok(f),
            Err(e) => Err(self.stoerung_text(e)),
        }
    }

    /// Wie `ereignis_senden`, aber mit einem `GVal` als Ziel — das
    /// braucht J2 (ein DOM-Knoten kann nicht als `Wert` importiert
    /// werden, siehe `builtin.rs::import_wert`).
    pub(crate) fn ereignis_senden_intern_offen(
        &mut self,
        typ: &str,
        ziel: GVal,
        eigenschaften: Vec<(String, Wert)>,
    ) -> Result<bool, Stoerung> {
        self.ereignis_senden_intern(typ, ziel, eigenschaften)
    }

    fn ereignis_senden_intern(
        &mut self,
        typ: &str,
        ziel: GVal,
        eigenschaften: Vec<(String, Wert)>,
    ) -> Result<bool, Stoerung> {
        let ev = self.ereignis_bauen(typ, ziel, false, eigenschaften)?;
        self.hoerer_laufen(typ, ziel, ev)
    }

    fn ereignis_abbrechbar_intern(
        &mut self,
        typ: &str,
        ziel: GVal,
        eigenschaften: Vec<(String, Wert)>,
    ) -> Result<bool, Stoerung> {
        let ev = self.ereignis_bauen(typ, ziel, true, eigenschaften)?;
        self.hoerer_laufen(typ, ziel, ev)
    }

    /// Ein `Stoerung` als Text (oeffentliche API).
    pub(crate) fn stoerung_text(&mut self, e: Stoerung) -> String {
        match e {
            Stoerung::Wurf(w) => self.wurf_text(w),
            Stoerung::Fehler(f) => f.to_string(),
        }
    }

    /// Release 10.10.2026: `abort` an die Hoerer eines AbortSignal zustellen.
    pub(crate) fn abbruch_melden(&mut self, signal: GVal, ev: GVal) -> Result<bool, Stoerung> {
        self.hoerer_laufen("abort", signal, ev)
    }

    /// Das Ereignis-Objekt bauen (nicht zustellen).
    pub(crate) fn ereignis_bauen(
        &mut self,
        typ: &str,
        ziel: GVal,
        cancelable: bool,
        eigenschaften: Vec<(String, Wert)>,
    ) -> Result<GVal, Stoerung> {
        // Release 11.10.2026: der Typ bleibt, wie er ist (DOM §2.2 —
        // `new Event('Foo').type` ist "Foo").
        let typ = typ.to_string();
        if typ.len() > MAX_TYP_HART {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Ereignistyp zu lang"),
            ));
        }
        // M4: Allokation vorher pruefen (Objekt + Typ + Ziel).
        self.vor_allok(typ.len() + 256).map_err(Stoerung::Fehler)?;
        let typ_wert = self.text_wert(&typ).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(typ_wert);
        self.wurzeln.push(ziel);
        let ev = self
            .neues_objekt(Some(self.proto_ereignis))
            .map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(ev));
        self.setze(GVal::Ref(ev), "type", typ_wert)?;
        self.setze(GVal::Ref(ev), "target", ziel)?;
        self.setze(GVal::Ref(ev), "currentTarget", ziel)?;
        self.setze(GVal::Ref(ev), "defaultPrevented", GVal::Bool(false))?;
        self.setze(GVal::Ref(ev), "cancelable", GVal::Bool(cancelable))?;
        // Release 11.10.2026: was der Host baut, kommt vom Browser (DOM
        // §2.2 „isTrusted"); `new Event()` setzt es wieder auf `false`.
        // GEMESSEN vorher: `isTrusted` war bei Host-Ereignissen undefined.
        self.setze(GVal::Ref(ev), "isTrusted", GVal::Bool(true))?;
        for (n, w) in eigenschaften {
            let v = self
                .import_wert(&w, 0)
                .map_err(|e| Stoerung::Wurf(self.fehler_wert("Error", &e.to_string())))?;
            self.wurzeln.push(v);
            self.setze(GVal::Ref(ev), &n, v)?;
        }
        Ok(GVal::Ref(ev))
    }

    /// Die passenden Hoerter aufrufen. `true` = Defaulthandlung erlaubt.
    fn hoerer_laufen(&mut self, typ: &str, ziel: GVal, ev: GVal) -> Result<bool, Stoerung> {
        let GVal::Ref(zr) = ziel else {
            return Ok(true);
        };
        // Release 14.10.2026: the microtask checkpoint after a listener only
        // happens when no script is running (HTML §8.1.4.4 „clean up after
        // running a callback": the JavaScript execution context stack is
        // empty) — i.e. for events the host dispatches, not for
        // `dispatchEvent()` called by a script. MEASURED before (WPT
        // dom/observable, own probe): promise reactions queued in the first
        // listener ran before the second listener of a scripted dispatch.
        let mit_checkpoint = self.rahmen.is_empty();
        let host = self.host_id_von_gval(ziel);
        self.wurzeln.push(ev);
        // M3: Die Liste ist durch `max_hoerer` gedeckelt; `zaehler` ist
        // die zusaetzliche Laufzeitgrenze (ein Hoerer darf nicht laenger
        // laufen als der ganze Aufruf).
        let deckel = self.grenzen.max_hoerer.min(MAX_HOERER_HART) + 8;
        let mut zaehler: usize = 0;
        let mut gestoppt = false;
        let typ = typ.to_string();
        // Release 10.10.2026: die Anmeldungen werden beim START festgehalten
        // (DOM §2.9 „inner invoke": eine Kopie der Liste); vor jedem Aufruf
        // wird geprueft, ob die Anmeldung noch besteht, und `once` wird VOR
        // dem Aufruf entfernt. GEMESSEN vorher (WPT
        // `AddEventListenerOptions-signal`): ein `once`-Hoerer, der im
        // eigenen Aufruf `dispatchEvent` rief, lief erneut — die Rekursion
        // wuchs ohne Ende (5 Mio. Stapelueberlaeufe, der Laeufer hing).
        let passt = |h: &Hoerer| h.typ == typ && (h.ziel == zr || (host.is_some() && h.host_ziel == host));
        let kandidaten: Vec<HeapRef> = self.hoerer.iter().filter(|h| passt(h)).map(|h| h.funktion).collect();
        // Die Kopie haelt ihre Funktionen am Leben (ein verschachtelter
        // Aufruf kann eine Anmeldung entfernen und den Speicher raeumen).
        let marke_kandidaten = self.wurzel_merke();
        for f in &kandidaten {
            self.wurzeln.push(GVal::Ref(*f));
        }
        for h_funktion in kandidaten {
            if gestoppt {
                break;
            }
            let Some(pos) = self.hoerer.iter().position(|h| passt(h) && h.funktion == h_funktion) else {
                continue;
            };
            if let Some(sig) = self.hoerer[pos].signal
                && self.eigene_lese(sig, "aborted") == Some(GVal::Bool(true))
            {
                self.hoerer.remove(pos);
                continue;
            }
            zaehler += 1;
            if zaehler > deckel {
                break;
            }
            if self.hoerer[pos].einmal {
                self.hoerer.remove(pos);
            }
            let marke = self.wurzel_merke();
            self.wurzeln.push(ev);
            self.wurzeln.push(ziel);
            let erg = self.aufruf_synchron(GVal::Ref(h_funktion), ziel, &[ev], false);
            self.wurzel_zurueck(marke);
            if let Err(e) = erg {
                // Ein Fehler im Hoerer darf den Host nicht sprengen: er wird
                // gemeldet (Wurf) und die Kette laeuft weiter.
                // GEMESSEN/ENTSCHIEDEN: ein Wurf im Hoerer beendet die
                // Kette nicht (der Browser meldet und laeuft weiter), der
                // Fehler wird aber sichtbar.
                //
                // J6c (Befund in team/PROBLEME.txt, 06:00, Rolle
                // INTEGRATION): der Fehler ging nur nach **stderr**
                // (`eprintln!`). Fuer den Host ist das unsichtbar — er liest
                // `konsole()`. GEMESSEN vorher: `dom_ereignis_senden` meldete
                // `Ok(true)`, `konsole()` war leer, im Fenster passierte
                // gar nichts: „der Klick ist angekommen, der Handler hat
                // nichts getan" — die schlimmste Form (kein Fehler, keine
                // Wirkung, kein Hinweis). Der alte Weg (`narsil-js`) schrieb
                // wenigstens „Handler: …" in die Statuszeile.
                // FIX: die Meldung geht **zusaetzlich** in die Konsole mit
                // dem Prefix `JS: ` (so, wie Skript-Meldungen kommen) —
                // `konsole()` liefert sie der Bruecke, die daraus ihre
                // Meldung fuer die Statuszeile macht. Der Rueckgabewert
                // bleibt die Antwort auf die Frage nach der
                // Defaulthandlung (siehe unten), damit Hosts, die nur den
                // Boolean lesen, unveraendert arbeiten.
                let text = match &e {
                    Stoerung::Wurf(w) => self.wurf_text(*w),
                    Stoerung::Fehler(f) => f.to_string(),
                };
                let zeile = format!("JS: Ereignis {typ}: {text}");
                // M6: Puffer gedeckelt (wie `console.log`).
                if self.konsole.len() >= 10_000 {
                    self.konsole.remove(0);
                }
                self.konsole.push(zeile.clone());
                eprintln!("{zeile}");
            }
            // Nach jedem Hoerer die Mikrotasks (wie im Browser).
            if mit_checkpoint {
                let d = self.grenzen.max_mikrotasks.max(8);
                let _ = self.mikro_abarbeiten(d);
            }
            // `stopPropagation` / `preventDefault` auslesen.
            if let GVal::Ref(er) = ev {
                if self.eigene_lese(er, "__gestoppt") == Some(GVal::Bool(true)) {
                    gestoppt = true;
                }
            }
        }
        self.wurzel_zurueck(marke_kandidaten);
        if let GVal::Ref(er) = ev {
            self.versteckt_setzen(er, "__gestoppt", GVal::Bool(false))?;
            let verhindert = self.eigene_lese(er, "defaultPrevented") == Some(GVal::Bool(true));
            let cancelable = self.eigene_lese(er, "cancelable") == Some(GVal::Bool(true));
            return Ok(!(verhindert && cancelable));
        }
        Ok(true)
    }

    /// `addEventListener(typ, fn, opts)` — `this` ist das Ziel.
    pub(crate) fn n_add_event_listener(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        // Release 11.10.2026: DOM §2.7 „add an event listener" — ein
        // `null`-Hoerer ist nichts (kein Fehler), ein Objekt mit
        // `handleEvent` wird zur Huelle (dieselbe je Objekt). GEMESSEN
        // vorher (WPT `EventTarget-addEventListener.any`):
        // „addEventListener: kein Funktionswert" bei `null`.
        // Die Optionen werden VOR dem `null`-Fall geprueft (DOM §2.7
        // „flatten more" vor „add an event listener"): `{ signal: null }`
        // wirft auch mit `null`-Hoerer.
        let null_hoerer = matches!(self.arg(args, 1), GVal::Null | GVal::Undefiniert);
        let args_feld = if null_hoerer { args.to_vec() } else { self.lauscher_huelle("addEventListener", args.to_vec())? };
        let args = &args_feld[..];
        let (capture, einmal_opt) = self.hoerer_optionen(self.arg(args, 2))?;
        let typ = match self.zu_text(self.arg(args, 0)) {
            Ok(t) => t,
            Err(_) => {
                return Err(Stoerung::Wurf(self.fehler_wert(
                    "TypeError",
                    "addEventListener: der Typ muss Text sein",
                )));
            }
        };
        let funktion = match self.arg(args, 1) {
            GVal::Ref(r)
                if matches!(
                    self.heap.holen(r),
                    HObj::Funktion { .. } | HObj::Nativ { .. }
                ) =>
            {
                r
            }
            _ if null_hoerer => crate::heap::KEIN_REF,
            _ => {
                // node wirft bei allem, was keine Funktion ist.
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "addEventListener: kein Funktionswert"),
                ));
            }
        };
        if typ.len() > MAX_TYP_HART {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "addEventListener: Ereignistyp zu lang"),
            ));
        }
        // Release 11.10.2026: `true` als drittes Argument ist `capture`, nicht
        // `once` (GEMESSEN vorher: `addEventListener(t, f, true)` lief nur
        // einmal).
        let einmal = einmal_opt;
        // Release 10.10.2026: `{ signal }` (DOM §2.7). `null` ist ein
        // TypeError, ein schon abgebrochenes Signal meldet nichts an.
        let mut signal = None;
        if let GVal::Ref(o) = self.arg(args, 2)
            && !self.ist_aufrufbar(self.arg(args, 2))
        {
            match self.hole(GVal::Ref(o), "signal")? {
                GVal::Undefiniert => {}
                GVal::Ref(sig) => {
                    if self.eigene_lese(sig, "aborted") == Some(GVal::Bool(true)) {
                        return Ok(GVal::Undefiniert);
                    }
                    signal = Some(sig);
                }
                _ => {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("TypeError", "addEventListener: signal ist kein AbortSignal"),
                    ));
                }
            }
        }
        if null_hoerer {
            return Ok(GVal::Undefiniert);
        }
        let (ziel, host_ziel) = match this {
            GVal::Ref(r) => {
                // Host-Objekt? Dann ist die Nummer das wahre Ziel (siehe
                // `Hoerer::host_ziel`).
                let h = self.host_id_von_gval(GVal::Ref(r));
                (r, h)
            }
            _ => return Ok(GVal::Undefiniert),
        };
        // GEMESSEN an node 22: dieselbe Funktion zweimal fuer denselben Typ
        // anzumelden ist ein **Nichts** (die Doppelanmeldung wird
        // ignoriert) — sonst liefe sie zweimal.
        // Release 11.10.2026: Ereignistypen sind GROSS-/kleinschreibungs-
        // genau (DOM §2.2: `type` ist ein beliebiger Text).
        let typ_klein = typ;
        if self.hoerer.iter().any(|h| {
            h.typ == typ_klein
                && h.funktion == funktion
                && h.capture == capture
                && (h.ziel == ziel || (host_ziel.is_some() && h.host_ziel == host_ziel))
        }) {
            return Ok(GVal::Undefiniert);
        }
        // M3/M4: Die Liste ist gedeckelt; danach meldet der Zusatz nichts
        // (gemessen: `max_hoerer` = 1024).
        if self.hoerer.len() >= self.grenzen.max_hoerer.min(MAX_HOERER_HART) {
            return Ok(GVal::Undefiniert);
        }
        self.hoerer.push(Hoerer {
            ziel,
            host_ziel,
            typ: typ_klein,
            funktion,
            einmal,
            capture,
            signal,
        });
        Ok(GVal::Undefiniert)
    }

    /// Release 11.10.2026: die Optionen (DOM §2.7 „flatten more"): ein
    /// Wahrheitswert ist `capture`, ein Objekt hat `capture` und `once`.
    fn hoerer_optionen(&mut self, opt: GVal) -> Result<(bool, bool), Stoerung> {
        match opt {
            GVal::Ref(o) if !self.ist_aufrufbar(opt) => {
                let c = self.hole(GVal::Ref(o), "capture")?;
                let e = self.hole(GVal::Ref(o), "once")?;
                Ok((self.zu_bool(c), self.zu_bool(e)))
            }
            GVal::Undefiniert | GVal::Null => Ok((false, false)),
            andere => Ok((self.zu_bool(andere), false)),
        }
    }

    /// `removeEventListener(typ, fn)` — nimmt EINE Anmeldung.
    pub(crate) fn n_remove_event_listener(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        if matches!(self.arg(args, 1), GVal::Null | GVal::Undefiniert) {
            return Ok(GVal::Undefiniert);
        }
        let args = &self.lauscher_huelle("removeEventListener", args.to_vec())?[..];
        let (capture, _) = self.hoerer_optionen(self.arg(args, 2))?;
        let typ = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let funktion = match self.arg(args, 1) {
            GVal::Ref(r) => r,
            _ => return Ok(GVal::Undefiniert),
        };
        let GVal::Ref(ziel) = this else {
            return Ok(GVal::Undefiniert);
        };
        let host = self.host_id_von_gval(GVal::Ref(ziel));
        if let Some(stelle) = self.hoerer.iter().position(|h| {
            h.typ == typ
                && h.funktion == funktion
                && h.capture == capture
                && (h.ziel == ziel || (host.is_some() && h.host_ziel == host))
        }) {
            self.hoerer.remove(stelle);
        }
        Ok(GVal::Undefiniert)
    }

    /// `dispatchEvent(ev)` — wie im Browser; gibt `false` zurueck, wenn
    /// `preventDefault()` bei einem abbrechbaren Ereignis gewirkt hat.
    pub(crate) fn n_dispatch_event(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let ev = self.arg(args, 0);
        let typ = self.zu_text_of(ev);
        let GVal::Ref(r) = ev else {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "dispatchEvent braucht ein Ereignis")));
        };
        if typ.is_empty() || self.eigene_lese(r, "__dispatching") == Some(GVal::Bool(true)) {
            return Err(Stoerung::Wurf(self.fehler_wert("InvalidStateError", "Ereignis nicht initialisiert oder bereits aktiv")));
        }
        self.versteckt_setzen(r, "__dispatching", GVal::Bool(true))?;
        self.setze(ev, "target", this)?;
        self.setze(ev, "currentTarget", this)?;
        self.setze(ev, "eventPhase", GVal::Zahl(2.0))?;
        let result = self.hoerer_laufen(&typ, this, ev);
        self.versteckt_setzen(r, "__dispatching", GVal::Bool(false))?;
        self.setze(ev, "currentTarget", GVal::Null)?;
        self.setze(ev, "eventPhase", GVal::Zahl(0.0))?;
        result.map(GVal::Bool)
    }

    /// `new EventTarget()` — ein Objekt mit `EventTarget.prototype`.
    ///
    /// GEMESSEN: mit dem allgemeinen `Object`-Konstruktor bekam das Objekt
    /// `Object.prototype` und damit kein `addEventListener`
    /// („Aufrufziel ist keine Funktion").
    pub(crate) fn n_ereignisziel_ctor(&mut self) -> Result<GVal, Stoerung> {
        Ok(GVal::Ref(
            self.neues_objekt(Some(self.proto_ereignisziel))
                .map_err(Stoerung::Fehler)?,
        ))
    }

    /// `new Event(typ, {cancelable})`.
    pub(crate) fn n_event_ctor(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let typ = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let mut cancelable = false;
        if let GVal::Ref(o) = self.arg(args, 1)
            && self.eigene_lese(o, "cancelable") == Some(GVal::Bool(true))
        {
            cancelable = true;
        }
        let event = self.ereignis_bauen(&typ, GVal::Null, cancelable, Vec::new())?;
        for name in ["bubbles", "composed"] {
            let value = self.hole(self.arg(args, 1), name).unwrap_or(GVal::Bool(false));
            self.setze(event, name, GVal::Bool(self.zu_bool(value)))?;
        }
        self.setze(event, "isTrusted", GVal::Bool(false))?;
        self.setze(event, "eventPhase", GVal::Zahl(0.0))?;
        Ok(event)
    }

    /// `ev.preventDefault()` — nur bei `cancelable`.
    pub(crate) fn dom_create_event(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let interface = self.zu_text(self.arg(args, 0))?.to_ascii_lowercase();
        if !matches!(interface.as_str(), "event" | "events" | "htmlevents" | "customevent") {
            return Err(Stoerung::Wurf(self.fehler_wert("NotSupportedError", "Ereignisschnittstelle nicht implementiert")));
        }
        let empty = self.text_wert("").map_err(Stoerung::Fehler)?;
        let event = self.n_event_ctor(&[empty])?;
        if interface == "customevent" { self.setze(event, "detail", GVal::Null)?; }
        Ok(event)
    }

    pub(crate) fn n_init_event(&mut self, this: GVal, args: &[GVal], custom: bool) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else { return Ok(GVal::Undefiniert); };
        if self.eigene_lese(r, "__dispatching") == Some(GVal::Bool(true)) { return Ok(GVal::Undefiniert); }
        let typ = self.zu_text(self.arg(args, 0))?;
        let typ = self.text_wert(&typ).map_err(Stoerung::Fehler)?;
        self.setze(this, "type", typ)?;
        self.setze(this, "bubbles", GVal::Bool(self.zu_bool(self.arg(args, 1))))?;
        self.setze(this, "cancelable", GVal::Bool(self.zu_bool(self.arg(args, 2))))?;
        self.setze(this, "defaultPrevented", GVal::Bool(false))?;
        self.setze(this, "target", GVal::Null)?;
        self.setze(this, "__gestoppt", GVal::Bool(false))?;
        if custom { self.setze(this, "detail", self.arg(args, 3))?; }
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn n_prevent_default(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        if self.eigene_lese(r, "cancelable") == Some(GVal::Bool(true)) {
            self.setze(this, "defaultPrevented", GVal::Bool(true))?;
        }
        Ok(GVal::Undefiniert)
    }

    /// `ev.stopPropagation()` — beendet die Hoerter dieses Ziels.
    pub(crate) fn n_stop_propagation(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        self.versteckt_setzen(r, "__gestoppt", GVal::Bool(true))?;
        Ok(GVal::Undefiniert)
    }

    /// `ev.stopImmediatePropagation()` — in diesem Motor dasselbe wie
    /// `stopPropagation` (es gibt keine bubbling-Phasen).
    pub(crate) fn n_stop_sofort(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        self.n_stop_propagation(this)
    }

    /// `ev.composedPath()` — leer dokumentiert (kein Aufsteigen).
    pub(crate) fn n_composed_path(&mut self) -> Result<GVal, Stoerung> {
        Ok(GVal::Ref(self.neues_feld().map_err(Stoerung::Fehler)?))
    }

    /// Die zusaetzlichen Felder eines Ereignisobjekts (`ev.detail` & Co.).
    fn ereignis_eigenschaften(&mut self, ev: GVal) -> Vec<(String, Wert)> {
        let mut raus = Vec::new();
        let GVal::Ref(r) = ev else {
            return raus;
        };
        // M3: hoechstens `MAX_SAMMLUNG` eigene Felder.
        for (n, e) in self
            .eigene_map(r)
            .into_iter()
            .take(crate::ops::MAX_SAMMLUNG)
        {
            if matches!(
                n.as_str(),
                "type" | "target" | "currentTarget" | "cancelable" | "__gestoppt"
            ) {
                continue;
            }
            if let Some(v) = self.gval_als_wert(e.wert) {
                raus.push((n, v));
            }
        }
        raus
    }
}

impl Engine {
    /// Eigenes Feld eines `GVal` lesen (`None`, wenn kein Objekt).
    fn eigene_lese_of(&mut self, v: GVal, name: &str) -> Option<GVal> {
        match v {
            GVal::Ref(r) => self.eigene_lese(r, name),
            _ => None,
        }
    }

    /// Den Typ eines Ereignisobjekts lesen.
    fn zu_text_of(&mut self, v: GVal) -> String {
        self.eigene_lese_of(v, "type")
            .map(|t| self.zu_text(t).unwrap_or_default())
            .unwrap_or_default()
    }
}

/// Kleiner Helfer, damit `?` in `n_add_event_listener` lesbar bleibt.
trait PipeZuBool {
    fn pipe_zu_bool(self, e: &mut Engine) -> Option<bool>;
}

impl PipeZuBool for GVal {
    fn pipe_zu_bool(self, e: &mut Engine) -> Option<bool> {
        match self {
            // Release 10.10.2026: ein Wahrheitswert an dritter Stelle ist
            // `capture`, NICHT `once` (DOM §2.7). GEMESSEN vorher:
            // `addEventListener('click', f, true)` lief nur ein einziges Mal.
            GVal::Bool(_) => None,
            GVal::Ref(r) => match e.eigene_lese(r, "once") {
                Some(GVal::Bool(b)) => Some(b),
                _ => None,
            },
            _ => None,
        }
    }
}

/// Nur fuer die Diagnose: wie viele Anmeldungen stehen gerade drin.
pub fn anzahl_hoerer(e: &Engine) -> usize {
    e.hoerer.len()
}

/// `Grenzen`-Hilfszeile fuer die Berichte.
pub fn grenzen_text(g: &Grenzen) -> String {
    format!("max_hoerer = {}", g.max_hoerer)
}
