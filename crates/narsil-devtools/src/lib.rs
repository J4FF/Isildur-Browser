//! Narsil — Entwicklerwerkzeuge (Rolle DEVTOOLS, Punkte D1–D8).
//!
//! Nutzerwunsch, wörtlich aus `team/ANFORDERUNGEN.txt` Z. 12:
//! „Entwicklerwerkzeuge SEHR ausgebaut: Elemente/DOM+CSS, Konsole,
//! Netzwerk, Quellen mit JS-Debugger (Haltepunkte, Schritt, Variablen),
//! Leistung, Speicher/Cookies, und WebAssembly-Debugging (Module,
//! Funktionen, Disassembly, Haltepunkte, Schritt, Stack/Locals/Speicher)."
//!
//! # WAS DIESE CRATE IST
//!
//! Sie ist **Zustand und Oberfläche**, nicht Zeichner: sie hält die Daten
//! der sieben Reiter und liefert für jeden eine [`narsil_display::DisplayList`].
//! Gerastert wird vom Aufrufer über `narsil-raster` — derselbe Weg wie bei
//! `narsil-chrome` (siehe `Cargo.toml`, warum der Rasterer nur dev-dep ist).
//!
//! # DIE GRENZEN DIESES ENTWURFS (gemessen, nicht behauptet)
//!
//! * **D4** hängt an `narsil-net`. Was es dort heute NICHT gibt: einen
//!   Ereignishaken, der Anfragen *während* sie laufen meldet. Diese Crate
//!   definiert deshalb die Ereignisquelle selbst (`netzwerk::NetzLog` +
//!   `AnfrageBau`), und der Wunsch an Rolle TLS steht in
//!   `team/WUNSCH_TLS.txt`. Die Umrechnung von `narsil_net::{Request,
//!   Response, NetError}` ist trotzdem echt und getestet.
//! * **D5** hängt an einem Debug-Haken in `narsil-jsvm`, den es noch nicht
//!   gibt: `debugger;` wird geparst und dann verworfen
//!   (`narsil-jsvm/src/compile.rs:504`), und der Aufrufstapel (`Rahmen`) ist
//!   `pub(crate)`. Diese Crate baut den Reiter vollständig (Haltepunkte,
//!   Schrittarten, Stapel, Variablen, Quelltext mit Haltepunktmarken) und
//!   nimmt die Daten über eine schmale, benannte Schnittstelle entgegen —
//!   siehe `quellen::Debugger` und `team/WUNSCH_JS.txt`.
//! * **D6** hängt an W5 der Rolle WASM (`crates/narsil-wasm`, im Bau).
//!   Der Reiter ist vollständig; die Zahlen kommen über `wasm::WasmDbg`,
//!   dieselbe Schnittstelle, die W5 liefern soll (`team/WUNSCH_WASM.txt`).
//!
//! # REGELN, DIE HIER GEZÄHLT WERDEN
//!
//! Jede Schleife über eine Eingabe und jede Allokation aus einer Eingabe
//! hat eine benannte Grenze in [`grenzen`]. Sie ist nicht verteilt über die
//! Dateien gesammelt, sondern an einer Stelle nachzuschlagen — das ist der
//! Grund, warum es das Modul gibt.
//!
//! ```
//! use narsil_devtools::Devtools;
//! use narsil_devtools::d1_geruest::Reiter;
//!
//! let mut d = Devtools::neu(1200, 800);
//! d.oeffnen();                       // wie F12
//! assert!(d.offen());
//! assert_eq!(d.reiter(), Reiter::Elemente);
//! assert_eq!(d.reiter_leiste().len(), 7, "D1: sieben Reiter");
//! ```

pub mod grenzen;

pub mod d1_geruest;
pub mod d2_elemente;
pub mod d3_konsole;
pub mod d4_netzwerk;
pub mod d5_quellen;
pub mod d6_wasm;
pub mod d7_speicher;
pub mod d8_leistung;
/// D19: Box-Werte bearbeiten, Regeln-Panel, Barrierefreiheit (Rolle/Name).
pub mod d9_layout;
pub mod thema;
pub mod werkzeug;
pub mod zeichnen;

pub use d1_geruest::{GERUEST, Geruest, Reiter, Taste};
pub use thema::Palette;

use d2_elemente::Elemente;
use d3_konsole::Konsole;
use d4_netzwerk::NetzLog;
use d5_quellen::Quellen;
use d6_wasm::WasmDbg;
use d7_speicher::Speicher;
use d8_leistung::Leistung;

/// Der ganze Zustand der Entwicklerwerkzeuge fuer EINEN Tab.
///
/// # WARUM EINE STRUKTUR UND SIEBEN SINGLETONS
///
/// Die sieben Reiter bekommen jeder ihren eigenen Zustand mit eigenen
/// Grenzen. Wer sie in EINEN Topf wirft, braucht für „nur die Konsole
/// leeren" eine Bedingung, und eine Bedingung, die man vergisst, ist ein
/// Speicherleck. Hier ist das Leeren eine Zeile ohne Möglichkeit daneben.
#[derive(Debug, Clone)]
pub struct Devtools {
    /// Das Geruest: offen/zu, welcher Reiter, wie gross der Bereich (D1).
    pub geruest: Geruest,
    /// Elemente: DOM-Baum, Auswahl, berechnete Styles (D2).
    pub elemente: Elemente,
    /// Konsole: `console.*`-Ausgaben und die JS-Eingabezeile (D3).
    pub konsole: Konsole,
    /// Quellen: Skripte, Haltepunkte, Schritt, Stapel, Variablen (D5).
    pub quellen: Quellen,
    /// Netzwerk: jede Anfrage mit Methode, Status, Protokoll, Zeit (D4).
    pub netz: NetzLog,
    /// Leistung: Zeit je Stufe (D8).
    pub leistung: Leistung,
    /// Speicher: Cookies, localStorage, Cache (D7).
    pub speicher: Speicher,
    /// WebAssembly: Module, Funktionen, Disassembly, Speicher (D6).
    pub wasm: WasmDbg,
    /// Die Textmessung des Fensters (D16).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum sie im Zustand liegt und nicht im
    /// Zeichencode: `narsil-layout/src/messung.rs:155` — „wenn der Rasterizer
    /// einen anderen Vorschub nimmt als der Umbruch vorsieht, rutscht der
    /// Text auseinander (gemessen: sichtbar gesperrte Buchstaben)". Genau das
    /// war das Bild vom 03.10. 22:0x, als der Zeichencode mit einem EIGENEN
    /// Schriftstapel mass. Der Reiter muss mit der Schrift messen, mit der
    /// gezeichnet wird — und das weiss nur das Fenster.
    messung: MessungZeiger,
}

/// Die Messung des Fensters, traegbar von `Debug` (D16, 22:2x).
///
/// GEMESSENE ENTSCHEIDUNG, warum ein Zeiger und kein Weglassen der
/// `Debug`-Ableitung: `narsil_layout::Messung` hat kein `Debug` (sie traegt
/// `Arc`-Stapel und TrueType-Daten), und `Devtools` wird an mehreren Stellen
/// mit `{:?}` ausgegeben — Tests, Fehlermeldungen. Ein Weglassen der
/// Ableitung wuerde diese Stellen brechen; `Messung` selbst zu aendern waere
/// eine fremde Datei. Also ein kleiner Zeiger, der `Debug` beantwortet, ohne
/// die Messung auszupacken — sie ist ein Speicher, keine Zahl.
#[derive(Clone)]
struct MessungZeiger(Option<narsil_layout::Messung>);

impl std::fmt::Debug for MessungZeiger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // GEMESSENE ENTSCHEIDUNG: nur „ja/nein". Die Messung zu formatieren
        // hiesse, den Schriftstapel zu debuggen — das ist eine Datei, kein
        // Zustand, und es steht in keiner Fehlermeldung, die man lesen will.
        f.write_str(if self.0.is_some() {
            "Messung: des Fensters"
        } else {
            "Messung: keine (Durchschnittsbreite)"
        })
    }
}

impl Devtools {
    /// Ein geschlossener Zustand für ein Fenster von `breite` × `hoehe`.
    ///
    /// M3/M4: Breite und Höhe kommen von aussen (der Fensterereignissen).
    /// 0, `u32::MAX` und NaN sind die drei Enden, an denen eine
    /// `saturating_sub`-Rechnung zu einer Stelle fuehrt, die es nicht gibt —
    /// `Geruest::neu` klemmt beide.
    pub fn neu(breite: u32, hoehe: u32) -> Self {
        Self {
            geruest: Geruest::neu(breite, hoehe),
            elemente: Elemente::leer(),
            konsole: Konsole::neu(),
            quellen: Quellen::leer(),
            netz: NetzLog::neu(),
            leistung: Leistung::neu(),
            speicher: Speicher::neu(),
            wasm: WasmDbg::leer(),
            messung: MessungZeiger(None),
        }
    }

    /// Die Textmessung des Fensters setzen (D16).
    ///
    /// Das ist der Weg des Fensters: es hat die Messung aus dem Layout und
    /// gibt sie dem Zustand. Ein Zustand ohne Messung zeichnet mit der
    /// gemessenen Durchschnittsbreite — das ist der alte Weg, und er ist
    /// schlechter, aber er zeichnet.
    pub fn mit_messung(&mut self, m: narsil_layout::Messung) {
        self.messung = MessungZeiger(Some(m));
    }

    /// Die Textmessung des Fensters, falls gesetzt.
    pub fn messung(&self) -> Option<&narsil_layout::Messung> {
        self.messung.0.as_ref()
    }

    /// DevTools aufmachen (F12, Strg+Shift+I, Kontextmenü „Untersuchen").
    ///
    /// # GEMESSENE ENTSCHEIDUNG: zweimal aufmachen ist ein Wechsel
    ///
    /// Drei Wege öffnen dieselbe Sache. Der vierte — ein Klick auf einen
    /// schon offenen Reiter — soll ihn schliessen. Sonst passiert bei
    /// drei Oeffnungswegen und einem Reiterband genau das, was in
    /// `narsil` bei Tabs passiert ist: drei Zustaende fuer eine Sache.
    pub fn oeffnen(&mut self) {
        self.geruest.setze_offen(true);
    }

    /// DevTools schliessen.
    pub fn schliessen(&mut self) {
        self.geruest.setze_offen(false);
    }

    /// Auf oder zu (F12). Liefert den neuen Zustand.
    pub fn umschalten(&mut self) -> bool {
        let neu = !self.geruest.offen;
        self.geruest.setze_offen(neu);
        neu
    }

    /// Ist das Geruest offen?
    pub fn offen(&self) -> bool {
        self.geruest.offen
    }

    /// Der aktive Reiter.
    pub fn reiter(&self) -> Reiter {
        self.geruest.reiter()
    }

    /// Den aktiven Reiter setzen.
    pub fn setze_reiter(&mut self, r: Reiter) {
        self.geruest.setze_reiter(r);
    }

    /// Das Reiterband („die Reiterleiste"): sieben Felder mit Namen.
    pub fn reiter_leiste(&self) -> &[Reiter] {
        self.geruest.reiter_leiste()
    }

    /// Der Reiter unter einem Punkt — fuer den Klick (D1).
    pub fn reiter_unter(&self, x: f32, y: f32) -> Option<Reiter> {
        self.geruest.reiter_unter(x, y)
    }

    /// Alles schliessen und leeren (Tabwechsel, „Seite neu laden").
    ///
    /// # WAS HIER AUSDRUECKLICH LEER WIRD
    ///
    /// Konsole, Netz und Speicher sind Seiten-Daten: eine neue Seite hat
    /// neue. Elemente und Quellen bekommen ihre Seite ueber `an_seite`,
    /// weil die Seite sie fuellt. Leistung zaehlt je Vorgang neu — der
    /// letzte Eintrag bleibt stehen, damit der Nutzer die Zahl der Seite
    /// sieht, die er gerade ansieht.
    pub fn seite_verlassen(&mut self) {
        self.konsole.leeren();
        self.netz.leeren();
        self.elemente.zuruecksetzen();
        self.quellen.zuruecksetzen();
        self.speicher.zuruecksetzen();
    }

    /// Die komplette DisplayList des geoeffneten Geruests (D1).
    ///
    /// Zu = eine LEERE Liste, und zwar `DisplayList::new()` ohne
    /// Hintergrund. „Kein DevTools" heisst nicht „schwarzes Rechteck
    /// unten im Fenster" — es heisst nichts.
    pub fn to_display_list(&self) -> narsil_display::DisplayList {
        if !self.offen() {
            return narsil_display::DisplayList::new();
        }
        zeichnen::geruest_zeichnen(self)
    }

    /// Die Groesse des Bereichs in Fenstermaßen, den die Werkzeuge belegen.
    ///
    /// `None`, wenn die Werkzeuge ZU sind oder das Fenster zu klein ist
    /// (D1: unter [`grenzen::MIN_BEREICH_HOEHE`] gibt es keinen Bereich,
    /// sondern nichts — ein 40-px-Band mit halben Zeilen ist kein Werkzeug).
    ///
    /// # GEMESSENER FEHLER, den D9 fand (01.10.2026, 21:2x)
    ///
    /// Diese Funktion gab nur `self.geruest.bereich()` zurueck — also die
    /// GEOMETRIE, ohne zu fragen, ob die Werkzeuge ueberhaupt offen sind.
    /// Der Test `f12_schaltet` in `crates/narsil/tests/d9_devtools.rs`
    /// sagte: „zu, aber mit Bereich". Nach dem zweiten F12 war der
    /// Zustand `offen == false`, `bereich()` lieferte aber ein Rechteck
    /// — und der Aufrufer (die Bruecke in `narsil`) reserviert Platz fuer
    /// etwas, das nicht gezeichnet wird. Das ist die Fehlerklasse „Standort
    /// berechnet, aber nicht geprueft", und sie faellt nur auf, wenn
    /// jemand die ZU-Frage stellt.
    pub fn bereich(&self) -> Option<narsil_core::geometry::Rect> {
        if !self.offen() {
            return None;
        }
        self.geruest.bereich()
    }
}

/// Die gemessenen Grenzen dieses Auftrags — mit [`grenzen::Grenzen::pruefen`]
/// gegen einen ganzen Zustand, in einem Test.
pub use grenzen::Grenzen;

/// Kurzform fuer die Grenzen (siehe `grenzen.rs`).
pub const GRENZEN: Grenzen = grenzen::Grenzen::STANDARDAUSGABE;
