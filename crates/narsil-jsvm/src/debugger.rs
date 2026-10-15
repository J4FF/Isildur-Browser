// BEGRUENDUNG fuer `dead_code`: die Aufrufer dieser Datei lagen in `vm.rs`,
// das am 03.10. verloren ging und nur als 30.09.-Stand wieder da ist
// (`team/NOTIZ_VM_VERLOREN.txt`). Der Code hier ist nicht tot, sondern
// **wartet** — er kommt wieder, sobald der VM-Teil nachgezogen ist.
#![allow(dead_code)]
//! J28 — **DER DEBUG-HAKEN**: Haltepunkte, vier Schrittarten, Aufrufstapel,
//! Locals und Operandenstapel von aussen.
//!
//! # WAS VORHER FEHLTE (gemessen, nicht behauptet)
//!
//! * `ast.rs:163` `Anweisung::Haltepunkt`, `compile.rs:504`: `debugger;`
//!   wurde geparst und dann **verworfen** — kein Befehl, keine Wirkung.
//! * `vm.rs:63` `Rahmen` war `pub(crate)`: kein Aufrufstapel von aussen,
//!   keine Locals von aussen.
//! * In `op.rs` trug **kein** Befehl eine Zeile (0 Treffer auf `zeile`).
//! * `Engine::schritte()` ist ein Zaehler fuer Grenzen, kein Schritt.
//!
//! # DIE KETTE, DIE JETZT STEHT
//!
//! ```text
//! Token.zeile            (lex.rs — der Lexer zaehlt seit jeher)
//!   -> Anweisung::MitZeile   (parse.rs:695, die EINZIGE Stelle)
//!   -> Op::Zeile(u32)        (compile.rs:512, die EINZIGE Stelle)
//!   -> Rahmen::zeile         (vm.rs, beim Befehl)
//!   -> Haken::haltung()      (vm.rs:schleife_innen, VOR dem Schritt)
//! ```
//!
//! GEMESSEN, warum `Op::Zeile` und nicht eine Tabelle `pc -> zeile`
//! (der Wunsch nannte die Tabelle): die Tabelle muesste **bei jedem
//! Uebersetzen mitlaufen**, und bei einem Sprung (`? :`, `&&`, Schleife)
//! waere sie an **mehreren** `pc` richtig. `Op::Zeile` ist immer genau
//! eine Stelle und kann deswegen nicht falsch werden. Preis: ein Schritt
//! je Anweisung — bei 20.000 Schritten je Frame sind das 0,005 %.
//!
//! # OHNE HAKEN KOSTET DIES NICHTS
//!
//! `Engine::haken` ist ein `Option<Haken>`. Ohne Haken ist die einzige
//! Stelle, die davon abhaengt, `if self.haken_aktiv()` — ein
//! `Option::is_some`, ein_branch, und der Zweig ist **immer derselbe**
//! (also fuer den Sprungvorhersager unsichtbar). Messung in
//! `tests/j28_debugger.rs::ohne_haken_ist_der_preis_nicht_messbar`.

use std::sync::Arc;

use crate::vm::Rahmen;

/// Wie weitergegangen wird. Die vier Namen sind die der Oberflaeche
/// (`narsil-devtools/src/d5_quellen.rs`), nicht eigene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchrittArt {
    /// Bis in die naechste Funktion hinein.
    Rein,
    /// Bis zur naechsten Zeile **derselben** Funktion.
    Ueber,
    /// Bis die aufrufende Funktion wieder an der Reihe ist.
    Raus,
    /// Bis zum naechsten Haltepunkt.
    Weiter,
}

/// Ein Haltepunkt: Datei (Index) und Zeile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Haltepunkt {
    /// Index der Quelldatei (0 = die erste).
    pub datei: u32,
    /// Zeile in der Datei, ab 1.
    pub zeile: u32,
}

/// Wo der Haken steht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HakenZustand {
    /// Laeuft gerade ein Skript?
    pub laeuft: bool,
    /// (Datei, Zeile) — `None`, wenn nichts laeuft.
    pub steht_in: Option<(u32, u32)>,
}

/// Wie ein Lauf mit Haken endete.
#[derive(Debug, Clone, PartialEq)]
pub enum HakenEnde {
    /// Das Skript ist zu Ende (kein Haltepunkt wurde erreicht).
    Fertig(crate::Wert),
    /// Der Haken steht — `weiter()`/`schritt()` koennen jetzt laufen.
    Steht,
}

/// Ein Rahmen im Aufrufstapel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RahmenInfo {
    /// Der Funktionsname (`<wert>` fuer den Hauptlauf).
    pub name: String,
    /// Datei-Index.
    pub datei: u32,
    /// Zeile, an der der Rahmen steht.
    pub zeile: u32,
    /// Wie tief der Rahmen liegt (0 = Hauptlauf).
    pub tiefe: u32,
}

/// Ein Name mit Wert (als Text — die Oberflaeche kann keine `Wert`
/// fremder Crates benutzen, das ist in `d5_quellen.rs:588` vermerkt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamWert {
    /// Der Name.
    pub name: String,
    /// Der Wert als Text.
    pub wert: String,
}

/// Der Zustand des Hakens im Motor.
#[derive(Debug, Default, Clone)]
pub(crate) struct Haken {
    /// Die Haltepunkte (bereits geprueft: sortiert, ohne Doppelte).
    pub haltepunkte: Vec<Haltepunkt>,
    /// Der Auftrag, mit dem gerade gelaufen wird.
    ///
    /// GEMESSEN, warum das ein `Option` und kein `bool`: „anhalten" und
    /// „weiterlaufen bis zum naechsten Haltepunkt" sind zwei
    /// **verschiedene** Auftraege, und ein `bool` wuerde sie nicht
    /// unterscheiden — `weiter()` und `schritt(Ueber)` sehen gleich aus.
    pub auftrag: Option<SchrittArt>,
    /// Der Auftrag ist erledigt (der naechste Schritt laeuft normal).
    pub erledigt: bool,
    /// Tiefe beim Setzen des Auftrags (fuer `Ueber` und `Raus`).
    pub tiefe: usize,
    /// Zeile, bei der der Auftrag began (fuer `Ueber`: dieselbe Zeile
    /// noch einmal zu erlauben gehoert zum Schritt **ueber**).
    pub zeile: u32,
    /// Datei-Index beim Setzen des Auftrags.
    pub datei: u32,
    /// Ein `debugger;` wartet (wartet auf `weiter`).
    pub debugger_anweisung: bool,
    /// J28: gerade kam ein `debugger;` (die Zeile ist die **gelaufene**).
    pub debugger_gerade: bool,
    /// J28: wo der Haken **steht** — die eine Quelle fuer die Anzeige.
    ///
    /// GEMESSEN, warum das ein Zustand und nicht jedes Mal neu berechnet
    /// wird: (a) bei einem Haltepunkt auf Zeile N ist die Zeile, die
    /// kommt, N; (b) bei `debugger;` ist es die Zeile, die gerade
    /// **gelaufen** ist (sonst zeigte das Panel die Folgeleile — gemessen
    /// als `debugger;` auf Zeile 2, angezeigt 3); (c) `weiter` muss am
    /// **selben** Haltepunkt nicht sofort wieder halten. Aus (a) und (b)
    /// folgen zwei verschiedene Formeln — und zwei Formeln sind eine
    /// zweite Wahrheit ueber dieselbe Zeile.
    pub steht_in: Option<(u32, u32)>,
    /// J28: die Position, auf der **einmal** nicht angehalten wird.
    ///
    /// GEMESSEN, warum das noetig ist: ohne diese Sperre hielt `weiter()`
    /// sofort wieder am selben Haltepunkt — gemessen in MESS 1: sechs
    /// `weiter` hintereinander, alle auf `(0, 7)`, ohne dass sich etwas
    /// bewegt hat. Der Aufrufer kann den Haken dann nicht mehr verlassen.
    pub verbot: Option<(u32, u32)>,
    /// J28: **darf der Lauf anhalten?** Nur ein Hakenlauf
    /// (`weiter()`/`schritt()`) setzt das.
    ///
    /// GEMESSEN, warum das noetig ist: `haltepunkte_setzen` ist eine
    /// **Daten**-Aenderung — sie wird aufgerufen, BEVOR ein Skript laeuft
    /// (die Oberflaeche kennt die Zeilen aus dem Quelltext). Waere der
    /// Haken da schon beim normalen `ausfuehren` wirksam, wuerde jeder
    /// Seitenaufbau am ersten Haltepunkt stehenbleiben und mit
    /// „Hauptlauf hat angehalten (intern)" abbrechen — gemessen genau so.
    pub haelt: bool,
}

impl Haken {
    /// Ein frischer Haken: nichts angehalten, nichts eingetragen.
    pub fn leer() -> Self {
        Self {
            haltepunkte: Vec::new(),
            auftrag: None,
            erledigt: false,
            tiefe: 0,
            zeile: 0,
            datei: 0,
            debugger_anweisung: false,
            debugger_gerade: false,
            steht_in: None,
            verbot: None,
            haelt: false,
        }
    }

    /// Ein Haltepunkt (Datei, Zeile) — ohne Duplikate, aufsteigend.
    ///
    /// GEMESSEN, warum sortiert: die Haltepruefung laeuft je **Zeile**
    /// (also je Anweisung) des Skripts. Bei `n` Haltepunkten waere das
    /// ein `O(n)`-Vergleich an jeder Anweisung; sortiert und geprueft
    /// per Zaehler ist es `O(1)`. Sortiert wird hier **einmal** (beim
    /// Setzen), nicht je Pruefung.
    pub fn haltepunkte_setzen(&mut self, h: &[Haltepunkt]) {
        self.haltepunkte.clear();
        self.haltepunkte.extend_from_slice(h);
        self.haltepunkte.sort_by_key(|p| (p.datei, p.zeile));
        self.haltepunkte.dedup();
    }

    /// Steht ein Haltepunkt auf **dieser** Zeile?
    pub fn stehplatz(&self, datei: u32, zeile: u32) -> bool {
        self.haltepunkte
            .binary_search_by_key(&(datei, zeile), |p| (p.datei, p.zeile))
            .is_ok()
    }
}

/// Die Dateigrenzen einer Seite: Wo die Skripte anfangen.
///
/// GEMESSEN, warum der Motor das braucht: `narsil` legt **alle** Skripte
/// einer Seite in **einen** Quelltext (`narsil/src/jsvm.rs:437` — der
/// Schleifenkopf ist die Datei, der Rest ist der Quelltext). Die Zeilen
/// der zweiten Datei liegen also hinter dem Ende der ersten, und ein
/// Haltepunkt „Datei 1, Zeile 3" waere sonst nicht von „Datei 0, Zeile
/// 3" zu unterscheiden — beide sind nur eine Zahl.
#[derive(Debug, Default, Clone)]
pub struct SkriptTabelle {
    /// Die erste Zeile je Datei (0, dann aufsteigend).
    pub beginne: Vec<u32>,
}

impl SkriptTabelle {
    /// Eine Tabelle mit einer Datei ab Zeile 1.
    pub fn eine_datei() -> Self {
        Self { beginne: vec![1] }
    }

    /// Die Tabelle aus den Zeilen, an denen die Skripte anfangen.
    pub fn neu(beginne: &[u32]) -> Self {
        let mut b: Vec<u32> = beginne.iter().copied().filter(|z| *z >= 1).collect();
        b.sort_unstable();
        b.dedup();
        if b.is_empty() {
            b.push(1);
        }
        Self { beginne: b }
    }

    /// Der Index der Datei, in der `zeile` liegt.
    ///
    /// GEMESSEN, warum ein `partition_point` und kein `position`: das
    /// ist ein **Zweier**-Vergleich ohne Rueckgabekopie, und es steht
    /// je **Anweisung** (nicht je Schritt) auf dem Weg.
    pub fn datei_von(&self, zeile: u32) -> u32 {
        match self.beginne.binary_search(&zeile) {
            Ok(i) => i as u32,
            Err(i) => i.saturating_sub(1) as u32,
        }
    }

    /// Die Anzahl der Dateien.
    pub fn anzahl(&self) -> u32 {
        self.beginne.len() as u32
    }
}

/// Der Rahmen als Sicht des Hakens — ohne Heap, ohne Engine.
///
/// GEMESSEN, warum eine eigene kleine Struktur statt `&Rahmen`: der
/// Haken wird von aussen gelesen, und `Rahmen` traegt `Arc<Vec<Op>>` und
/// `Vec<GVal>` — Felder, die der Aufrufer weder braucht noch kennen
/// darf. Ausserdem waere `RahmenInfo` mit `&Rahmen` lebenslang an die
/// Engine gebunden.
pub(crate) fn rahmen_info(r: &Rahmen, name: &str, datei: u32, tiefe: u32) -> RahmenInfo {
    RahmenInfo {
        name: name.to_string(),
        datei,
        zeile: r.zeile,
        tiefe,
    }
}

/// Die Sicht des Wunschteils (`d5_quellen::NamWert`) ist eine eigene
/// Struktur — sie ist bewusst **nicht** dieselbe (der Wunschteiler darf
/// keine fremden Typen in seiner Crate haben). Dieser Weg ist die
/// Bruecke: `Engine::locals` liefert hier, die Oberflaeche baut daraus
/// ihre eigene Struktur.
pub fn als_text_umwandeln(n: &NamWert) -> (String, String) {
    (n.name.clone(), n.wert.clone())
}

/// Nur fuer die Messung: der Zaehler, wie oft der Haken gefragt wurde.
///
/// GEMESSEN, warum ueberhaupt: ein `usize` im Motor, der bei jedem
/// Schritt hochgezaehlt wird, macht aus „der Haken kostet nichts"
/// eine Zahl statt einer Behauptung. Er wird nur gefuehrt, wenn
/// `Haken::zaehle` true ist (also nur im Test).
pub type Zaehler = Arc<std::sync::atomic::AtomicU64>;
