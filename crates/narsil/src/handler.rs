//! W6D — EIN HANDLER ALS FUNKTIONSWERT (`addEventListener('click', () => …)`).
//!
//! # DER BEFUND (13:40)
//!
//! `addEventListener` bekam fuer eine Funktion `undefined`:
//! `Wert` (`wert.rs`) kennt keine Funktions-Variante, also gab
//! `gval_als_wert` fuer einen `Ref` auf ein `HObj::Funktion` `None`
//! zurueck. Der Handler wurde als „undefined" gemeldet und nicht
//! angenommen — und `Sitzung::klick` lieferte `false`.
//!
//! # DIE LOESUNG, und sie sind Drei
//!
//! 1. **`Wert::Funktion(u32)`** — ein Funktionswert bekommt eine eigene
//!    Variante (nur den Heap-Index). GEMESSEN, warum nicht „irgendein
//!    Objekt": ein Objekt kann man aufrufen, ohne dass etwas passiert;
//!    die Engine muss den Aufruf unterscheiden koennen.
//!
//! 2. **DIE ENGINE BLEIBT AM LEBEN.** Ein Handler ist eine CLOSURE
//!    (`const z` aus dem Seitenskript). Wuerde der Klick in einer neuen
//!    Engine laufen, waere `z` nicht da — und der Handler liefe zwar,
//!    scheiterte aber an `z.textContent`. Also liegt die Engine nach dem
//!    Laden in der Sitzung (`Angezeigt::engine`) und wird beim Klick
//!    WIEDER VERWENDET.
//!
//! 3. **GC-WURZEL.** Ein `HeapRef` zeigt in den Heap, und der Heap wird
//!    bei einer Sammlung freigegeben. Der Handler wird also als Wurzel
//!    gehalten (`Engine::wurzel_setzen`), solange die Seite lebt.
//!
//! # WAS UNVERAENDERT BLEIBT
//!
//! String-Handler (`addEventListener('click', 'function(){…}')`) gehen
//! weiter — sie sind der W6-Weg und der bleibt als Zusatz.

// B1-B3 (01.10.2026, Rolle KOMPRESS): `Engine` stand hier lange im
// Import und wurde nicht benutzt — nur `Wert`. `cargo` hat es als
// Warnung gemeldet und niemand hat es weggeraeumt; beim Tausch auf
// `narsil-jsvm` faellt es weg, weil man den Import sowieso anfasst.
use narsil_jsvm::Wert;

/// EIN HANDLER, und er kann beides sein.
///
/// GEMESSEN, warum ein `enum` und kein `String`: der Handler ENTWEDER ein
/// Funktionswert (mit Closure, W6D) ODER ein Quelltext (W6). Beides in
/// einem `String` zu mischen hiesse, einen Funktionswert in Text zu
/// verwandeln — genau das, was vorher nicht ging.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Handler {
    /// Ein Funktionswert aus der Engine (mit seinen geschlossenen
    /// Variablen).
    Funktion(u32),
    /// Quelltext, der bei jedem Ereignis neu getragen wird.
    ///
    /// GEMESSEN, warum das noetig bleibt: der Text ist die einzige Form,
    /// die einen Handler ueber einen Skriptlauf hinweg ueberlebt, und
    /// `elAddEventListener` (W6) benutzt sie.
    Quelltext(String),
    /// EIN ANWEISUNGSBLOCK — der Quelltext eines `on*`-ATTRIBUTS
    /// (H2, 03.10.2026).
    ///
    /// GEMESSEN, warum das eine eigene Art sein MUSS und kein Erkennen:
    /// `onclick="document.getElementById('z').textContent = 'x'"` ist
    /// eine ZUWEISUNG, `addEventListener('click', function(){ … })` ein
    /// FUNKTIONSWERT. Die alte Huelle war `({q})(ev)` — ein Aufruf, der
    /// beim Anweisungsblock `('x')(ev)` macht und `TypeError: Aufrufziel
    /// ist keine Funktion` meldet (gemessen 07:10). Die neue Huelle war
    /// `(function(){ q }).call(ev)` — dasUEBERLEGT beim Funktionswert,
    /// weil er nur DEFINIERT und nie aufgerufen wird (gemessen: `handler_
    /// funktion.rs::der_string_handler_laeuft_weiter` und `javascript.rs::
    /// ein_klick_ruft_den_handler` wurden rot).
    ///
    /// GEMESSEN, warum NICHT Raten: Erkennen ist eine Syntaxfrage, die an
    /// zwei Stellen (Skript-Weg, Attribut-Weg) beantwortet werden muss. Wer
    /// sie an einer Stelle beantwortet, hat sie an der anderen nicht
    /// beantwortet — und die andere ist dann der Fehler. Zwei Arten sind
    /// eine Entscheidung, die der ERKENNER faellt, wenn er die Quelle
    /// sieht.
    Anweisung(String),
}

impl Handler {
    /// Der Quelltext, falls es einer ist (fuer die Meldungen).
    pub fn als_text(&self) -> String {
        match self {
            Handler::Funktion(_) => "[Funktion]".to_string(),
            Handler::Quelltext(q) => q.clone(),
            Handler::Anweisung(q) => q.clone(),
        }
    }
}

/// Meldet, ob ein Wert ueberhaupt ein Handler ist.
pub fn ist_handler(w: &Wert) -> bool {
    matches!(w, Wert::Funktion(_))
}

/// Registration identity is never reused, including after removal or `once`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerId(pub u64);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListenerOptions {
    pub capture: bool,
    pub once: bool,
    pub passive: bool,
}

impl ListenerOptions {
    /// Options have already crossed the VM boundary as a value snapshot.
    /// `signal` is not implemented until the host can retain object identity.
    pub fn from_wert(value: Option<&Wert>) -> Self {
        match value {
            Some(Wert::Gegenstand(fields)) => {
                let flag = |name: &str| fields.iter().rev()
                    .find(|(key, _)| key == name)
                    .is_some_and(|(_, value)| truthy(value));
                Self { capture: flag("capture"), once: flag("once"), passive: flag("passive") }
            }
            Some(value) => Self { capture: truthy(value), ..Self::default() },
            None => Self::default(),
        }
    }
}

/// Release 2026-10-15: whether the options name `passive` with a value
/// other than undefined (DOM §2.7 "flatten more": otherwise the default
/// passive value applies).
pub fn passive_given(value: Option<&Wert>) -> bool {
    match value {
        Some(Wert::Gegenstand(fields)) => fields
            .iter()
            .rev()
            .find(|(key, _)| key == "passive")
            .is_some_and(|(_, v)| !matches!(v, Wert::Undefiniert)),
        _ => false,
    }
}

fn truthy(value: &Wert) -> bool {
    match value {
        Wert::Undefiniert | Wert::Null => false,
        Wert::Wahr(value) => *value,
        Wert::Zahl(value) => *value != 0.0 && !value.is_nan(),
        Wert::Text(value) => !value.is_empty(),
        Wert::ByteFeld { .. } | Wert::Feld(_) | Wert::Gegenstand(_) | Wert::Funktion(_) | Wert::Nativ(_) => true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    pub id: ListenerId,
    pub target: u32,
    pub event_type: String,
    pub callback: Handler,
    pub options: ListenerOptions,
}

/// Ordered listeners are independent of the replaceable `on*` properties.
///
/// A dispatch snapshots IDs, then calls `prepare` immediately before each
/// callback. Added callbacks wait for the next snapshot, removed callbacks
/// are skipped, and a `once` callback is removed before reentrant dispatch.
#[derive(Debug, Default)]
pub struct ListenerRegistry {
    next_id: u64,
    entries: std::collections::HashMap<ListenerId, Listener>,
    ordered: std::collections::HashMap<(u32, String), Vec<ListenerId>>,
    /// Release 15.10.2026: the duplicate check of `add` and the lookup of
    /// `remove` as a hash lookup — (target, type, capture, callback) -> id.
    /// MEASURED (github.com): 15,657 `load` listeners on `window`, each `add`
    /// scanned all earlier ones (innermost frame in 4 of 10 samples).
    index: std::collections::HashMap<(u32, String, bool, Handler), ListenerId>,
}

impl ListenerRegistry {
    pub const MAX_LISTENERS: usize = 32_768;

    pub fn add(&mut self, target: u32, event_type: impl Into<String>, callback: Handler,
        options: ListenerOptions) -> Result<ListenerId, &'static str> {
        let event_type = event_type.into();
        let key = (target, event_type.clone());
        let index_key = (target, event_type.clone(), options.capture, callback.clone());
        if let Some(id) = self.index.get(&index_key) {
            return Ok(*id);
        }
        if self.entries.len() >= Self::MAX_LISTENERS {
            return Err("addEventListener: too many listeners in this document");
        }
        self.next_id = self.next_id.checked_add(1)
            .ok_or("addEventListener: registration identity exhausted")?;
        let id = ListenerId(self.next_id);
        self.entries.insert(id, Listener { id, target, event_type, callback, options });
        self.ordered.entry(key).or_default().push(id);
        self.index.insert(index_key, id);
        Ok(id)
    }

    /// Only type, callback identity and capture participate in removal.
    pub fn remove(&mut self, target: u32, event_type: &str, callback: &Handler,
        capture: bool) -> bool {
        let id = self.index.get(&(target, event_type.to_string(), capture, callback.clone())).copied();
        id.is_some_and(|id| self.remove_id(id).is_some())
    }

    pub fn snapshot(&self, target: u32, event_type: &str) -> Vec<ListenerId> {
        self.ordered.get(&(target, event_type.to_string())).cloned().unwrap_or_default()
    }

    pub fn get(&self, id: ListenerId) -> Option<&Listener> {
        self.entries.get(&id)
    }

    pub fn prepare(&mut self, id: ListenerId) -> Option<Listener> {
        let listener = self.entries.get(&id)?.clone();
        if listener.options.once { self.remove_id(id); }
        Some(listener)
    }

    fn remove_id(&mut self, id: ListenerId) -> Option<Listener> {
        let entry = self.entries.remove(&id)?;
        self.index.remove(&(entry.target, entry.event_type.clone(), entry.options.capture, entry.callback.clone()));
        let key = (entry.target, entry.event_type.clone());
        if let Some(ids) = self.ordered.get_mut(&key) {
            ids.retain(|candidate| *candidate != id);
            if ids.is_empty() { self.ordered.remove(&key); }
        }
        Some(entry)
    }

    /// Root these heap handles in the owning page VM, never in a fresh VM.
    pub fn function_roots(&self) -> Vec<u32> {
        self.entries.values().filter_map(|entry| match &entry.callback {
            Handler::Funktion(index) => Some(*index), _ => None,
        }).collect()
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    /// Release 10.10.2026: gibt es irgendwo einen Einfang-Horcher (`capture`)
    /// fuer diesen Typ? Dann laeuft auch ein nicht aufsteigendes Ereignis
    /// (`load` eines Bildes) durch die Vorfahren.
    pub fn hat_einfangen(&self, event_type: &str) -> bool {
        self.entries.values().any(|l| l.options.capture && l.event_type == event_type)
    }
}
