// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! W6B (J9) — HOST-OBJEKTE: Getter und Setter fuer Objekteigenschaften.
//!
//! # DER WEG, und warum ueberhaupt noetig
//!
//! Bisher gab es nur `host_funktion`: eine globale JS-Funktion, die
//! Werten entgegennimmt und einen Wert zurueckgibt. Damit liess sich
//! `document.getElementById("z")` bauen — aber nicht `z.textContent = "3"`.
//! Das ist eine EIGENSCHAFT, und Eigenschaften kommen ueber `hole()`
//! (lesen) und `setze_inner()` (schreiben), nicht ueber einen Aufruf.
//!
//! # WIE EIN HOST-OBJEKT AUSSIEHT
//!
//! Ein ganz gewöhnliches Objekt mit genau einem Feld:
//!
//! ```text
//! { "__host": 42 }
//! ```
//!
//! Die Zahl ist die Host-Id (der Browser gibt sie an, JavaScript sieht
//! sie nie). Sobald die Engine eine Eigenschaft eines solchen Objekts
//! liest oder schreibt, ruft sie den Haken — und NICHT die Feldliste.
//!
//! WARUM SO UND NICHT ALS EIGENER TYP: eine neue Variante in `GVal`
//! oder `HObj` muesste an mehreren hundert Stellen angefasst werden
//! (jedes `match` ueber `GVal` in 18.883 Zeilen). Ein vorhandenes
//! Objekt mit einem Merkfeld ist eine Aenderung an ZWEI Stellen.
//!
//! # DIE DREI REGELN
//!
//! 1. `None` heisst „ich kenne diese Eigenschaft nicht" — dann liest
//!    die Engine das Objekt ganz normal. Ein Getter, der immer einen Wert
//!    liefert, wuerde `console.log(el)` zu einem Objekt mit Eigenschaften
//!    machen, die es nicht gibt.
//! 2. Ein Haken darf keine Endlosschleife machen. Der Haken ist Rust und
//!    darf zum Beispiel KEIN JavaScript aufrufen — `narsil` sammelt Befehle
//!    in einer Kiste und gibt sie nach dem Lauf zurueck.
//! 3. Beide Haken sind `Send`: die Engine zieht in einen Kind-Thread um
//!    (`VERTRAG_JS`), und die Closures nehmen es mit.

use crate::Wert;
use std::sync::Arc;

/// Das Feld, an dem ein Host-Objekt erkannt wird.
///
/// GEMESSEN, warum `__host` und nicht `id`: `id` ist ein Name, den ein
/// Skript benutzt. Zwei Reserve-Namen mit Unterstrichen sind die
/// Hauskonvention; der Wert wird von Hand gesetzt, also kann es keine
/// Kollision mit `__proto__` geben (das ist ein Schluesselwort des
/// Parsers).
pub const HOST_MERKE: &str = "__host";

/// Der Getter: `(host_id, eigenschaft) -> Wert`.
///
/// `None` = „kenne ich nicht".
pub type HostGetter = dyn Fn(u32, &str) -> Result<Option<Wert>, String> + Send + Sync;

/// Der Setter: `(host_id, eigenschaft, wert) -> ()`.
///
/// `Err` = ein benannter Fehler (etwa „Eigenschaft gibt es nicht").
pub type HostSetter = dyn Fn(u32, &str, &Wert) -> Result<(), String> + Send + Sync;

/// Die beiden Haken zusammen.
///
/// WARUM EIN TRAIT und nicht zwei Felder: sie gehoeren zusammen, und ein
/// Haken ohne den anderen waere entweder halb lesbar oder halb
/// schreibbar — beides eine stille Ueberraschung fuer das Skript.
pub trait HostObjektHaken: Send + Sync {
    /// Only reflected native properties belong in the host. Other JS values
    /// stay in the VM, retaining identity, descriptors and GC ownership.
    fn verwaltet(&self, _id: u32, _art: u8, _name: &str) -> bool { true }
    fn schreibbar(&self, _id: u32, _art: u8, _name: &str) -> bool { true }
    fn lesen_mit_art(&self, id: u32, _art: u8, name: &str) -> Result<Option<Wert>, String> {
        self.lesen(id, name)
    }
    fn schreiben_mit_art(&self, id: u32, _art: u8, name: &str, wert: &Wert) -> Result<(), String> {
        self.schreiben(id, name, wert)
    }

    /// Eine Eigenschaft lesen.
    fn lesen(&self, id: u32, eigenschaft: &str) -> Result<Option<Wert>, String>;

    /// Eine Eigenschaft schreiben.
    fn schreiben(&self, id: u32, eigenschaft: &str, wert: &Wert) -> Result<(), String>;

    /// Eine METHODE rufen (`el.classList.add("x")`).
    ///
    /// # WARUM DAS NOETIG IST
    ///
    /// `el.classList.add` ist eine Eigenschaft, und ihr Wert wird
    /// aufgerufen. Ein Wert aus `lesen()` ist aber ein `Wert` — und
    /// `Wert` kennt keine Funktionen (siehe `wert.rs`: Undefiniert, Null,
    /// Wahr, Zahl, Text, Feld, Gegenstand). Ein Objekt ist nicht
    /// aufrufbar.
    ///
    /// GEMESSEN, warum nicht „eine Funktion zurueckgeben": das ginge
    /// nicht — `Wert::Gegenstand` wird zu einem Objekt, und `{}()` ist in
    /// JavaScript ein `TypeError`. Es gibt drei Wege aus dieser Lage:
    /// eine neue `Wert`-Variante (30 Stellen), Getter-Methoden (ein
    /// Haken, der nur Namen annimmt — und `add` braucht Argumente), oder
    /// dieser Kanal: der Getter liefert `{__host, __methode}`, und der
    /// Aufruf geht hierhin.
    ///
    /// DER PREIS: `this` ist die Element-Id, nicht das Objekt. Fuer
    /// `el.style.color = "rot"` reicht das (die Eigenschaft kennt ihr
    /// Element), und es ist messbar billiger als ein Objekt im Heap.
    ///
    /// # DIE GRENZE
    ///
    /// M3: der Aufrufer zaehlt (`MAX_ZUGRiffe` in `narsil::dom`). Der
    /// Haken selbst ist ein Rust-Aufruf und kann nicht haengen.
    fn rufen(&self, id: u32, methode: &str, args: &[Wert]) -> Result<Wert, String>;
    fn rufen_mit_art(&self, id: u32, _art: u8, methode: &str, args: &[Wert]) -> Result<Wert, String> {
        self.rufen(id, methode, args)
    }
    /// Release 09.10.2026: wartende Custom-Element-Reaktionen (HTML §4.13.6),
    /// je `[art, knoten, …]`. Der Motor holt sie nach jedem DOM-Aufruf ab und
    /// gibt sie dem Haken des Skripts (`__narsilCe.haken`). Leer = nichts.
    fn reaktionen(&self) -> Vec<Wert> {
        Vec::new()
    }
    /// Release 2026-10-15: a stamp that changes whenever the members of a
    /// live collection (host kinds 5-8) may have changed; 0 = unknown, then
    /// the engine does not cache their `length` and indices.
    fn sammlung_stand(&self) -> u64 {
        0
    }
}

/// Die Standard-Implementierung: zwei Closures in einer Box.
///
/// GEMESSEN, warum `Arc<Mutex<…>>` UND `Send + Sync`: die Crate, die den
/// Haken baut (`narsil`), haelt den DOM-Zustand in einem `Mutex` — der
/// muss von beiden Seiten erreichbar sein. Ein `Rc` waere nicht `Send`
/// und die Engine kaeme gar nicht erst in ihren Kind-Thread.
pub struct Closures {
    lesen: Box<HostGetter>,
    schreiben: Box<HostSetter>,
    rufen: Box<HostRufer>,
}

/// Der Typ des Aufruf-Hakens.
pub type HostRufer = dyn Fn(u32, &str, &[Wert]) -> Result<Wert, String> + Send + Sync;

impl HostObjektHaken for Closures {
    fn lesen(&self, id: u32, eigenschaft: &str) -> Result<Option<Wert>, String> {
        (self.lesen)(id, eigenschaft)
    }

    fn schreiben(&self, id: u32, eigenschaft: &str, wert: &Wert) -> Result<(), String> {
        (self.schreiben)(id, eigenschaft, wert)
    }

    fn rufen(&self, id: u32, methode: &str, args: &[Wert]) -> Result<Wert, String> {
        (self.rufen)(id, methode, args)
    }
}

/// Baut einen Haken aus drei Closures.
///
/// # DIE ARGUMENT-REIHENFOLGE
///
/// `(lesen, schreiben, rufen)` — in der Reihenfolge, in der die Engine
/// sie braucht. GEMESSEN, warum das eine Begruendung und nicht die
/// Reihenfolge des Alphabets ist: eine falsche Zuordnung faellt nicht auf
/// (alles kompiliert), sondern ruft den falschen Haken auf.
pub fn haken<G, S, R>(lesen: G, schreiben: S, rufen: R) -> Arc<dyn HostObjektHaken>
where
    G: Fn(u32, &str) -> Result<Option<Wert>, String> + Send + Sync + 'static,
    S: Fn(u32, &str, &Wert) -> Result<(), String> + Send + Sync + 'static,
    R: Fn(u32, &str, &[Wert]) -> Result<Wert, String> + Send + Sync + 'static,
{
    Arc::new(Closures {
        lesen: Box::new(lesen),
        schreiben: Box::new(schreiben),
        rufen: Box::new(rufen),
    })
}

/// Der Name des Feldes, das eine METHODE am Host-Objekt kennzeichnet.
pub const METHODE: &str = "__methode";

/// Meldet die Host-Id eines Wertes, wenn es ein Host-Objekt ist.
///
/// GEMESSEN, warum das hier und nicht in `eigenschaften.rs` steht: es ist
/// die Frage „was ist das?", und die gehoert zum Objekt, nicht zum
/// Zugriff. `eigenschaften.rs` fragt nur noch.
pub fn host_id(wert: &Wert) -> Option<u32> {
    match wert {
        Wert::Gegenstand(felder) => {
            felder
                .iter()
                .find(|(n, _)| n == HOST_MERKE)
                .and_then(|(_, w)| match w {
                    Wert::Zahl(n) if *n >= 0.0 && n.is_finite() => Some(*n as u32),
                    _ => None,
                })
        }
        _ => None,
    }
}

/// Der Zaehler, den `narsil` fuer seine Host-Ids braucht.
///
/// WARUM EIN EIGENER Zaehler: eine Host-Id darf keine `NodeId` sein und
/// keine Vorlauf-Nummer — sie ist nur eine Zahl im Haken. Ein Zaehler mit
/// Grenze macht daraus eine Zusage (M3).
#[derive(Debug)]
pub struct IdZaehler {
    naechste: u32,
    grenze: u32,
}

impl IdZaehler {
    /// `grenze` ist M3: so viele Host-Objekte gibt es höchstens.
    pub fn neu(grenze: u32) -> Self {
        Self {
            naechste: 0,
            grenze,
        }
    }

    /// Die naechste Id, oder `None`, wenn die Grenze erreicht ist.
    pub fn naechste(&mut self) -> Option<u32> {
        if self.naechste >= self.grenze {
            return None;
        }
        let n = self.naechste;
        self.naechste += 1;
        Some(n)
    }
}
