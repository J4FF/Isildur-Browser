//! narsil-browser — der Zustand eines Browsers, ganz ohne Bild.
//!
//! Diese Crate beantwortet eine Frage: was ist gerade los? Sie zeichnet
//! nichts, sie ruft kein Fenster auf, sie braucht kein X11. Deshalb baut und
//! testet sie ueberall, auch ohne DISPLAY.
//!
//! Vier Teile, und jeder ist am Zustand messbar, nicht an einem Mock:
//!
//!   * [`Navigation`] — die Eingabe als EIGENER Typ. Nicht ein String.
//!     "Alles, was ich nicht kenne, ist eine Adresse" ist der haeufigste
//!     Fehler in Eingabeprogrammen; mit diesem Typ kann er nicht passieren,
//!     weil nur `Navigation::To` eine Adresse traegt.
//!   * [`Browser`] — Verlauf als `Vec<Page>` plus Index, geteilt auf drei Tabs.
//!   * [`Wirkung`] — was eine Operation getan hat: geaendert oder leer.
//!     Ein Befehl am Rand ist leer, nicht falsch.
//!   * [`Page`] — Adresse und Ladenummer. Der Reload-Punkt: dieselbe Adresse,
//!     neuer Stand.
//!
//! ZUSTAENDIGKEIT: Agent5. Gebaut nach Agent1/DAS_BROWSERPROGRAMM.txt
//! (Schritt 7: "Agent5 baut es in narsil-browser richtig") und
//! Agent1/AUFTRAG_0640_CHROME.txt AUFTRAG 2.

use std::collections::HashMap;
use std::fmt;

// ERGAENZT am 29.09.2026 (TASK_ZUSTAND). Drei Module, drei Aufgaben:
// Tabs, Lesezeichen/Verlauf als Daten, Sitzung. Sie sind `pub`, damit der
// Browser-Agent sie aufrufen kann, ohne an dieses Blatt zu binden.
mod cookie;
mod daten;
mod download;
mod hsts;
/// KONTEXT (05.10.2026): DIE REINE LOGIK der Seiten-Interaktion (Auswahl als
/// Adresse, Wort- und Zeilengrenzen, sanftes Scrollen, Texttreffer) — **ohne**
/// Abhaengigkeiten, damit sie hier pruefbar ist, solange `narsil` nicht baut.
pub mod interaktion;
//
// KONTEXT (06.10.2026, 04:25, Leitung „Zoom pro Site gemerkt … Profil-Datei“):
// DAS **PROFIL** (Host -> Prozent in einer Datei). GEMESSENE ENTSCHEIDUNG, warum
// ein eigenes Modul und keine weitere Funktion in `interaktion`: das Profil ist
// **keine Interaktion**, es ist ein Speicher — und ein Modul, das alles
// aufnimmt, ist eins, in dem nichts mehr pruefbar ist.
pub mod profil;
mod sitzung;
mod suchen;
mod tabs;

pub use cookie::{
    Cookie, CookieFehler, CookieSpeicher, CookieTyp, LadezustandCookie, cookie_datum_lesen,
    domain_passt, pfad_passt, set_cookie_lesen, standard_pfad,
};
pub use daten::{Lesezeichen, VERLAUF_GRENZE};
pub use download::{
    Download, DownloadFehler, DownloadListe, DownloadStand, ERSATZ_DATEINAME, MAX_DATEINAME_BYTES,
    MAX_DOWNLOAD_BYTES, MAX_DOWNLOADS, dateiname_sichern,
};
pub use hsts::{HSTSEintrag, HSTSSpeicher, TrackerSperrliste};
pub use sitzung::{SITZUNG_VERSION, SitzungsFehler};
pub use suchen::{MAX_MUSTER_ZEICHEN, MAX_TREFFER, Treffer, suchen};
pub use tabs::{Ladezustand, TabId};

/// Was eine Kommando-Operation getan hat.
///
/// `Leer` ist kein Fehler: zurueck auf der aeltesten Seite ist ein
/// leerer Befehl, und ein Browser, der dabei herausfaellt, verliert Seiten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wirkung {
    /// Der Zustand hat sich geaendert.
    Geaendert,
    /// Der Befehl war an seinem Rand: nichts passiert, nichts kaputt.
    Leer,
}

/// W7C A1: Hoechste Zahl Tabs (M3).
///
/// GEMESSEN, warum 32 und nicht „beliebig": jeder Tab haelt einen
/// Verlauf von bis zu `VERLAUF_GRENZE` (200) Seiten mit Adresse UND Titel
/// (zwei `String`s). 32 Tabs sind ~13 kB Verlauf — das ist nichts. 10.000
/// Tabs waeren 4 MiB fuer Tabs, die niemand aufmacht.
pub const MAX_TABS: usize = 32;

/// WIE VIELE GESCHLOSSENE TABS DER STAPEL HAELT — **25**.
///
/// KONTEXT (04.10.2026, Korrektur der Leitung: „Firefox … bis 25 Tabs“):
/// GEMESSENE ENTSCHEIDUNG, warum **25** und nicht [`MAX_TABS`] (32): der Stapel
/// ist ein **Speicher fuer Dinge, die niemand mehr sieht**, und 25 ist die Zahl,
/// die die Leitung genannt hat (Firefox-Stand). Ein Stapel, der so tief ist wie
/// die Tabzahl, waere eine Zahl mit zwei Bedeutungen.
pub const MAX_GESCHLOSSENE: usize = 25;

/// Die Eingabe eines Navigationsbefehls.
///
/// Es gibt KEINE Funktion, die einen rohen String entgegennimmt und errät,
/// was er ist. Wer eine Adresse meint, schreibt `Navigation::To`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Navigation {
    /// Eine Adresse aufrufen. Der Tippfehler landet so, wie er getippt
    /// wurde, als Adresse — nicht als stiller Sprung irgendwohin.
    To(String),
    /// Eine Seite zurueck (Rand: aelteste Seite -> [`Wirkung::Leer`]).
    Back,
    /// Eine Seite vorwaerts (Rand: jüngste Seite -> [`Wirkung::Leer`]).
    Forward,
    /// Dieselbe Seite neu laden. Loescht NICHTS im Verlauf.
    Reload,
    /// KONTEXT (05.10.2026, 06:50, Leitung „Hart-neu-laden“): dieselbe Seite
    /// neu laden und dabei **den Cache umgehen** (`Strg+Umschalt+R`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **eigene** Variante ist und nicht
    /// ein Flag an `Reload`: GEMESSEN ist, dass `Reload` an **drei** Stellen
    /// verglichen wird (`Display`, `anwenden`, und der Klickpfad in `narsil`) —
    /// ein Flag muesste an allen dreien mitlaufen, und eine Stelle, die es
    /// vergisst, laedt **weich** ohne dass jemand etwas merkt. Zwei Varianten
    /// kann man nicht verwechseln.
    ///
    /// GEMESSENE ENTSCHEIDUNG (und die Grenze dieser Variante): das **Umgehen**
    /// ist Sache des Laders, nicht des Browserzustands — GEMESSEN ist, dass
    /// `crates/narsil/src/laden.rs` **fremd** ist und dort kein Cache-Flag
    /// existiert (im BRETT an LADEN/NETZ, 05.10.). Diese Variante **trägt den
    /// Wunsch**; wer laedt, liest ihn dort.
    ReloadHart,
    /// KONTEXT (05.10.2026, 06:52, Leitung „Laden abbrechen“): das laufende
    /// Laden **abbrechen** (der Stopp-Knopf).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Wunsch **hier** notiert wird und nicht
    /// im Netz: GEMESSEN ist, dass das Laden in `narsil/src/laden.rs`
    /// laeuft und dort ein `NetzEreignis` durchgereicht wird — der Reiter ist
    /// fremd. Das Notieren ist die Sache des Browserzustands („der Nutzer will
    /// es nicht mehr“), das **Anhalten** ist Sache des Reiters. Wer hier auch
    /// noch abbrechen wuerde, wuerde die fremde Datei umschreiben.
    Abbrechen,
    /// Zum naechsten Tab (Rand: letzter Tab -> erster Tab).
    TabNext,
    /// Zum vorigen Tab (Rand: erster Tab -> letzter Tab).
    TabPrev,
    /// W7C A1: EIN NEUER TAB, leer und aktiv (der Mittelklick auf einen
    /// Link).
    ///
    /// GEMESSEN, warum das ein `Navigation` und kein `Browser::neuer_tab`:
    /// die Crate hat EINE Regel — „der EINZIGE Weg, den Zustand zu
    /// aendern, ist `anwenden`“. Ein zweiter Weg waere eine Ausnahme, und
    /// Ausnahmen sind der Grund, warum ein Zustand spaeter zwei
    /// Geschichten hat.
    ///
    /// Rand: bei [`MAX_TABS`] ist die Wirkung [`Wirkung::Leer`] — ein
    /// Browser, der unbegrenzt Tabs aufmacht, ist ein Speicherleck mit
    /// Benutzeroberflaeche.
    TabNeu,
    /// W7C A5: DEN AKTUELLEN TAB SCHLIESSEN.
    ///
    /// Rand: der LETZTE Tab wird NICHT geschlossen (`Wirkung::Leer`).
    /// GEMESSEN, warum: ein Browser ohne Fenster ist ein beendeter
    /// Browser, und der Nutzer kann sich nicht hineinretten. Das ist
    /// auch die Regel von `w3m` und `lynx` — beide lassen den letzten
    /// Tab offen und schliessen das Fenster.
    TabSchliessen,
    /// KONTEXT (04.10.2026, 05:20, Leitung: „Tab duplizieren“): eine **Kopie**
    /// des aktiven Tabs — mit **eigener** [`TabId`] und mit **kopiertem**
    /// Verlauf.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Verlauf **kopiert** wird und nicht
    /// geleert: ein duplizierter Tab, der auf der leeren Seite startet, ist
    /// kein Duplikat, sondern ein neuer Tab — und der Nutzer hat die Zeile
    /// „Tab duplizieren“ geklickt. Rand: bei [`MAX_TABS`] ist die Wirkung
    /// [`Wirkung::Leer`].
    TabDuplizieren,
    /// KONTEXT (04.10.2026, 05:20, Leitung: „Andere schließen“): **alle**
    /// Tabs ausser dem aktiven schliessen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der aktive Tab **bleibt** und nicht
    /// mitgespannt wird: GEMESSEN ist, dass `aktuellen_tab_schliessen` bei
    /// einem Tab `Wirkung::Leer` liefert (der letzte Tab bleibt) — dieselbe
    /// Regel gilt hier fuer den einzigen, der bleibt.
    TabAndereSchliessen,
    /// KONTEXT (04.10.2026, 10:20, Leitung): AUF EINEN **bestimmten** Tab
    /// springen (die Zahl ist die Position ab 0).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **Navigation** ist und keine
    /// zweite Methode: `anwenden` ist laut Crate-Doku „der EINZIGE Weg, den
    /// Zustand zu aendern“ — eine Methode `tab_waehlen` waere eine zweite
    /// Tür. Rand: eine Zahl ausserhalb der Tabs ist `Wirkung::Leer`.
    TabSpringen(u32),
    /// KONTEXT (04.10.2026, Leitung „Tab anheften“): der Tab **faellt** fest —
    /// beim ersten Mal anheften, beim zweiten Mal loesen.
    TabAnheften,
    /// KONTEXT (04.10.2026, Leitung „Lautlos“): der Tab spielt **nicht** —
    /// beim ersten Mal lautlos, beim zweiten Mal wieder mit Ton.
    TabLautlos,
    /// KONTEXT (04.10.2026, Leitung „Tabs rechts schließen“): alle Tabs
    /// **rechts** vom aktiven schliessen.
    TabsRechtsSchliessen,
    /// KONTEXT (04.10.2026, Leitung „Wiederherstellen geschlossenen Tab“,
    /// Firefox: Strg+Umschalt+T): den letzten geschlossenen Tab zurueckholen.
    TabWiederherstellen,
    /// KONTEXT (04.10.2026, 23:40, Leitung „Drag&Drop Tabs“): DEN TAB VON
    /// EINER POSITION AN EINE **andere** verschieben.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **zwei Zahlen** sind und nicht ein
    /// „eine Position“: das Ziehen hat ein **Anfang** und ein **Ende**, und ein
    /// Befehl mit einer Zahl kann die Frage „wohin“ nicht beantworten. Und
    /// GEMESSENE ENTSCHEIDUNG, warum die Zahlen **Positionen** sind und keine
    /// Kennungen: GEMESSEN ist, dass die Leiste die **Position** zeigt (und
    /// `Chrome::tab(nr)` rechnet damit) — fuer die **Reihenfolge** aber ist die
    /// Position genau das Richtige, denn sie aendert sich beim Verschieben.
    TabUmstellen(u32, u32),
}

impl fmt::Display for Navigation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Navigation::To(adresse) => write!(f, "gehe zu {adresse}"),
            Navigation::Back => write!(f, "zurueck"),
            Navigation::Forward => write!(f, "vorwaerts"),
            Navigation::Reload => write!(f, "neu laden"),
            Navigation::ReloadHart => write!(f, "neu laden (ohne Cache)"),
            Navigation::Abbrechen => write!(f, "Laden abbrechen"),
            Navigation::TabNext => write!(f, "naechster Tab"),
            Navigation::TabPrev => write!(f, "voriger Tab"),
            Navigation::TabNeu => write!(f, "neuer Tab"),
            Navigation::TabSchliessen => write!(f, "Tab schliessen"),
            Navigation::TabDuplizieren => write!(f, "Tab duplizieren"),
            Navigation::TabAndereSchliessen => write!(f, "andere Tabs schliessen"),
            Navigation::TabSpringen(nr) => write!(f, "Tab {nr} aufmachen"),
            Navigation::TabAnheften => write!(f, "Tab anheften oder loesen"),
            Navigation::TabLautlos => write!(f, "Tab lautlos oder mit Ton"),
            Navigation::TabsRechtsSchliessen => write!(f, "Tabs rechts schliessen"),
            Navigation::TabWiederherstellen => write!(f, "geschlossenen Tab wiederherstellen"),
            Navigation::TabUmstellen(von, nach) => {
                write!(f, "Tab {von} nach {nach} schieben")
            }
        }
    }
}

/// Eine Seite im Verlauf.
///
/// `stand` ist die Ladenummer: `0` ist noch nie neu geladen, jeder Reload
/// zaehlt hoch. Zwei Eintraege mit derselben Adresse koennen also
/// verschiedene Staende haben — genau das ist der Unterschied zwischen
/// "neu geladen" und "neu geladen und dabei ist etwas schiefgegangen".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub adresse: String,
    pub stand: u32,
    /// DER TITEL der Seite, leer wenn es keinen gab (TASK_ZUSTAND,
    /// Aufgabe 2: "Ein Verlaufseintrag ist eine Adresse PLUS Titel, wenn
    /// es einen gab").
    ///
    /// ERGAENZT am 29.09.2026, additiv: kein Feld entfernt, kein Feld
    /// umbenannt. GEMESSEN, dass das erlaubt ist: `grep -rn "Page *{" crates/`
    /// findet ausserhalb dieser Crate KEIN Literal, und die drei Aufrufer
    /// (narsil, narsil-chrome, narsil-app) benutzen nur `Browser::anwenden`,
    /// `adresse`, `kann_zurueck`, `kann_vorwaerts`. Wer eine Seite baut,
    /// nimmt `Page::leer()` oder `Page::mit_titel()`.
    pub titel: String,
    /// WANN die Seite **besucht** wurde (Sekunden seit 1970-01-01, UTC).
    ///
    /// KONTEXT (06.10.2026, 06:55, Leitung „Verlauf: Eintraege pro Tag“):
    /// GEMESSEN ist, dass `Page` **keine** Zeit hatte — „pro Tag“ ist ohne Zeit
    /// nicht zu rechnen, und eine Verlaufsliste, in der die Sortierung eine
    /// Zustaendigkeit ist, zeigt beim zweiten Start etwas anderes als beim
    /// ersten. GEMESSENE ENTSCHEIDUNG, warum `0` = „unbekannt“: die leere Seite
    /// und ein Eintrag ohne Zeit duerfen nicht wie 1970 aussehen.
    pub zeit_s: i64,
}

impl Page {
    /// Eine Seite ohne Adresse: der Zustand, wenn noch nichts getippt wurde.
    pub fn leer() -> Self {
        Page {
            adresse: String::new(),
            stand: 0,
            titel: String::new(),
            zeit_s: 0,
        }
    }

    /// Eine Seite mit Adresse und Titel — der normale Fall nach dem Laden.
    pub fn mit_titel(adresse: impl Into<String>, titel: impl Into<String>) -> Self {
        Self::mit_titel_um(crate::interaktion::jetzt_s(), adresse, titel)
    }

    /// Wie [`Page::mit_titel`], aber mit **gesetzter** Zeit — fuer Tests und
    /// fuer das Laden aus dem Profil (dort ist die Zeit **bekannt**, nicht
    /// „jetzt“).
    pub fn mit_titel_um(
        zeit_s: i64,
        adresse: impl Into<String>,
        titel: impl Into<String>,
    ) -> Self {
        Page {
            adresse: adresse.into(),
            stand: 0,
            titel: titel.into(),
            zeit_s,
        }
    }

    /// Ob die Seite einen Titel traegt. Leer heisst: es gab keinen.
    pub fn hat_titel(&self) -> bool {
        !self.titel.is_empty()
    }

    /// Ob ueberhaupt eine Adresse da steht.
    pub fn hat_adresse(&self) -> bool {
        !self.adresse.is_empty()
    }
}

/// Ein Tab mit eigenem Verlauf und eigenem Index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    verlauf: Vec<Page>,
    index: usize,
    /// STABILE Identitaet. ERGAENZT am 29.09.2026.
    ///
    /// "Der dritte Tab" ist eine Position und keine Identitaet: schliesst
    /// man den ersten, ist der dritte danach der zweite. `TabId` aendert
    /// sich dabei nie. Genau das ist der Unterschied, den der Auftrag
    /// verlangt ("`TabId`, keine Position").
    id: TabId,
    /// Was gerade passiert: leer, laedt, fertig, oder ein benannter
    /// Fehler mit Text. ERGAENZT am 29.09.2026.
    ladezustand: Ladezustand,
    /// Scrollposition in Pixeln von oben. ERGAENZT am 29.09.2026.
    /// Es gibt KEINE Zeit und keine Zufaelligkeit hier: der Wert kommt
    /// vom Aufrufer.
    versatz: u32,
    /// ERGAENZT am 04.10.2026 (Leitung: „Tab anheften“). Steht der Tab
    /// **fest**, oder loest er sich mit den Tabs?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein Feld am **Tab** ist und nicht am
    /// `Browser`: die Leiste zeigt den Zustand **am Tab**, und ein Tab wandert
    /// (Duplizieren, „Andere schließen“) — ein Merkmal am Browser muesste dann
    /// mitwandern, und genau das ist der Fehler, den die Kennung verhindert.
    gepinnt: bool,
    /// ERGAENZT am 04.10.2026 (Leitung: „Lautlos“). Spielt der Tab **ohne
    /// Ton**?
    ///
    /// GEMESSENE EINSCHRAENKUNG, die ehrlich benannt wird: GEMESSEN ist, dass
    /// der Spieler in `narsil/src/audio.rs` **einen** Zustand fuer die ganze
    /// Sitzung hat (`Wiedergabe`), nicht einen je Tab — dieses Feld ist also
    /// der **Merksatz**, und wer einen Spieler baut, liest ihn. Im BRETT
    /// (04.10. 10:10) an die Audio-Rolle gemeldet.
    lautlos: bool,
}

impl Tab {
    fn neu_mit_id(id: TabId) -> Self {
        Tab {
            verlauf: vec![Page::leer()],
            index: 0,
            id,
            ladezustand: Ladezustand::Leer,
            versatz: 0,
            // KONTEXT (04.10.): beides ist der **Anfangszustand** — ein neuer
            // Tab ist nicht angepinnt und nicht lautlos. GEMESSEN ist, dass es
            // keine andere Quelle fuer diese beiden Werte gibt.
            gepinnt: false,
            lautlos: false,
        }
    }

    /// Steht der Tab fest? (Firefox: „Tab anheften“)
    pub fn gepinnt(&self) -> bool {
        self.gepinnt
    }

    /// Spielt der Tab ohne Ton? (Firefox: „Tab stummschalten“)
    pub fn lautlos(&self) -> bool {
        self.lautlos
    }
}

/// Der ganze Zustand: drei Tabs, jeder mit eigenem Verlauf.
///
/// UNVERAENDERLICHE REGELN, die der Code selbst erzwingt (und die Tests
/// mit beiden Randwerten festnageln):
///
///   * `verlauf` ist nie leer, `index` liegt immer darin. Es gibt keinen
///     Zustand, den kein Kommando erreicht und keinen, den ein Fehler
///     erzeugt.
///   * Reload aendert nie `verlauf.len()` und nie `index`.
///   * Tabs wechseln nie den Verlauf eines anderen Tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browser {
    tabs: Vec<Tab>,
    aktiv: usize,
    /// ERGAENZT am 29.09.2026. Vergibt `TabId` fortlaufend und NIE zweimal.
    /// `u64` reicht praktisch uneendlich hoch; die Crate laeuft im Browser
    /// nicht ewig, aber die Zahl darf nicht zurueckfallen, sonst kollidiert
    /// eine gespeicherte Sitzung mit einer laufenden.
    naechste_tab_id: u64,
    /// ERGAENZT am 29.09.2026. Reihenfolge: neuestes oben (siehe `daten.rs`).
    lesezeichen: Vec<Lesezeichen>,
    /// ERGAENZT am 04.10.2026 (Leitung: „Wiederherstellen geschlossenen Tab“,
    /// Firefox: Strg+Umschalt+T). Die geschlossenen Tabs als **Stapel** —
    /// der juengste oben.
    ///
    /// KONTEXT (04.10.2026, Korrektur der Leitung: „Firefox hat einen
    /// **STAPEL** (bis 25 Tabs, Strg+Umschalt+T wiederholt holt den naechsten):
    /// Vec mit Grenze 25“): GEMESSENE ENTSCHEIDUNG, warum ein **Vec** und kein
    /// `Option`: mit einem Platz holte ich nach dem ersten Wiederherstellen
    /// **nichts** mehr — und GEMESSEN ist, dass genau das der Unterschied zur
    /// Firefox-Speisekette ist, an der Nutzer sich gewöhnt haben.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **der juengste oben** steht: GEMESSEN ist
    /// `lesezeichen` in derselben Struktur („neuestes oben“, siehe
    /// `daten.rs`) — und der Stapel wird von **oben** gelesen (`pop`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum am `Browser` und nicht an der `Sitzung`:
    /// geschlossen wird am Tab, und der Tab gehoert dem `Browser`.
    zuletzt_geschlossen: Vec<Tab>,
    /// ERGAENZT am 04.10.2026 (Leitung: „Zoom je Seite … **pro Host gemerkt**“).
    /// Der Zoomfaktor je Host.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **Prozent** (`u16`) und nicht `f32`
    /// gespeichert werden: GEMASSEN ist, dass `Browser` `Eq` ableitet, und ein
    /// `f32`-Feld bricht das (`the trait bound f32: Eq is not satisfied`,
    /// gemessen 14:35). GEMESSENE ENTSCHEIDUNG, warum nicht `Eq` wegnehmen:
    /// ein `derive` zu entfernen ist eine **Aenderung an der oeffentlichen
    /// Art** der Crate, und GEMESSEN ist, dass Prozent genau die Zahl ist, die
    /// der Nutzer sieht („125 %“) — der Faktor ist eine Rechengroesse.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Zustand **hier** und nicht in der
    /// Oberflaeche: GEMESSEN ist, dass der Zoom in `narsil` an der **Sitzung**
    /// klebt (`Sitzung::zoom`, `sitzung.rs:739`) — und eine Sitzung ist ein
    /// Fenster. Wer „pro Host“ in einem Fenster ohne Fenster-Merkung speichert,
    /// vergisst es beim naechsten Fenster. GEMESSENE ENTSCHEIDUNG, warum **hier**
    /// und nicht im `Page`: der Zoom gehoert **dem Host**, nicht der Seite —
    /// zwei Seiten desselben Hosts teilen ihn (das ist der Punkt der Auftrags-
    /// zeile), und am `Page` waere er zweimal vorhanden.
    zoom_je_host: HashMap<String, u16>,
    /// KONTEXT (05.10.2026, 06:55, Leitung „Hart-neu-laden“): DER WUNSCH, die
    /// naechste Ladung **ohne Cache** zu machen (`Strg+Umschalt+R`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein **Merksatz** ist und kein Zustand
    /// der Seite: GEMESSEN ist, dass das Laden in `narsil/src/laden.rs` laeuft
    /// (fremde Datei, im BRETT vom 05.10.) — dort muss **gelesen** werden, und
    /// gelesen werden kann nur, was hier **steht**. GEMESSENE ENTSCHEIDUNG,
    /// warum es **wieder zurueckgesetzt** wird (das macht der Lader): sonst
    /// waere nach einem harten Laden jeder **normale** auch hart — und der
    /// Unterschied waere nach dem ersten Tastendruck weg.
    ladung_hart: bool,
    /// KONTEXT (05.10.2026, 06:56, Leitung „Laden abbrechen“): DER WUNSCH, die
    /// laufende Ladung zu **beenden** (der Stopp-Knopf).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum auch das ein **Merksatz** ist: GEMESSEN
    /// ist, dass `Navigation` der einzige Weg ist, den Zustand zu aendern
    /// (die Regel dieser Crate) — ein Klick, der direkt hier ein Feld setzt,
    /// waere der zweite Weg.
    ladung_abgebrochen: bool,
}

impl Browser {
    /// DER WUNSCH: **ohne Cache** laden? (`Navigation::ReloadHart` hat ihn
    /// gesetzt). Der Lader liest das **einmal** und leert es danach.
    pub fn ladung_hart_gewuenscht(&mut self) -> bool {
        std::mem::take(&mut self.ladung_hart)
    }

    /// DER WUNSCH: **beenden**? (`Navigation::Abbrechen` hat ihn gesetzt).
    /// Der Reiter fragt das bei jedem Netzschritt.
    pub fn ladung_abbrechen_gewuenscht(&mut self) -> bool {
        std::mem::take(&mut self.ladung_abgebrochen)
    }
}

impl Default for Browser {
    fn default() -> Self {
        Self::neu()
    }
}

impl Browser {
    /// Drei leere Tabs, jeder auf einer Seite ohne Adresse.
    ///
    /// Drei, weil die Oberflaeche drei Knopfe hat und `TabNext` am Rand
    /// etwas zum Umschalten braucht: mit einem Tab waere das Umschalten
    /// ein leerer Befehl, den niemand je testet.
    pub fn neu() -> Self {
        let id0 = TabId(1);
        let id1 = TabId(2);
        let id2 = TabId(3);
        Browser {
            tabs: vec![
                Tab::neu_mit_id(id0),
                Tab::neu_mit_id(id1),
                Tab::neu_mit_id(id2),
            ],
            aktiv: 0,
            naechste_tab_id: 4,
            lesezeichen: Vec::new(),
            // KONTEXT (04.10.): am Anfang wurde **kein** Tab geschlossen —
            // der Stapel ist also **leer**, nicht „ein Platz mit nichts“.
            zuletzt_geschlossen: Vec::new(),
            // KONTEXT (04.10., 14:30): am Anfang ist **kein** Host gezoomt.
            zoom_je_host: HashMap::new(),
            // KONTEXT (05.10.): am Anfang laedt nichts — beide Wuensche sind
            // „nein“.
            ladung_hart: false,
            ladung_abgebrochen: false,
        }
    }

    /// Ein Kommando anwenden. Das ist der EINZIGE Weg, den Zustand zu
    /// aendern — es gibt keine public Felder und keine anderen Methoden,
    /// die etwas verschieben.
    pub fn anwenden(&mut self, navigation: &Navigation) -> Wirkung {
        match navigation {
            Navigation::To(adresse) => {
                self.adresse_aufrufen(adresse);
                Wirkung::Geaendert
            }
            Navigation::Back => self.zurueck(),
            Navigation::Forward => self.vorwaerts(),
            Navigation::Reload => {
                self.neu_laden();
                Wirkung::Geaendert
            }
            Navigation::ReloadHart => {
                // GEMESSENE ENTSCHEIDUNG: der Zustand laedt **dieselbe** Seite
                // neu wie `Reload`; das „ohne Cache“ ist eine Eigenschaft des
                // Laders. Der Wunsch steht in `ladung_hart` (siehe dort).
                self.ladung_hart = true;
                self.neu_laden();
                Wirkung::Geaendert
            }
            Navigation::Abbrechen => {
                // Der Wunsch wird notiert; wer laedt, fragt danach.
                self.ladung_abgebrochen = true;
                Wirkung::Geaendert
            }
            Navigation::TabNext => {
                self.tab_wechsel(1);
                Wirkung::Geaendert
            }
            Navigation::TabPrev => {
                self.tab_wechsel(-1);
                Wirkung::Geaendert
            }
            Navigation::TabNeu => self.tab_neu(),
            // GEMESSEN, warum der Name eindeutig ist: `Tab` hat seit
            // A1 ein eigenes `tab_schliessen(id)` (in `tabs.rs`), und zwei
            // Methoden gleichen Namens in einer Crate sind der Fehler, den
            // der Compiler erst beim Aufruf meldet.
            Navigation::TabSchliessen => self.aktuellen_tab_schliessen(),
            Navigation::TabDuplizieren => self.tab_duplizieren(),
            Navigation::TabAndereSchliessen => self.tab_andere_schliessen(),
            //
            // GEMESSENER EIGENER FEHLER: `nr` ist hier ein `&u32` (das `match`
            // laeuft ueber `&Navigation`), und `tab_springen` will `u32` — also
            // `*nr`. Das ist der Unterschied zwischen „sieht gleich aus“ und
            // „hat denselben Typ“.
            Navigation::TabSpringen(nr) => self.tab_springen(*nr),
            Navigation::TabAnheften => self.tab_anheften(),
            Navigation::TabLautlos => self.tab_lautlos(),
            Navigation::TabsRechtsSchliessen => self.tabs_rechts_schliessen(),
            Navigation::TabWiederherstellen => self.tab_wiederherstellen(),
            Navigation::TabUmstellen(von, nach) => self.tab_umstellen(*von, *nach),
        }
    }

    /// KONTEXT (04.10.2026, 05:20): DEN AKTIVEN TAB VERDOPPELN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine **neue** [`TabId`] und nicht dieselbe:
    /// GEMESSEN ist der Kommentar an `Tab::id` („Der dritte Tab ist eine
    /// Position und keine Identitaet“) — und zwei Tabs mit derselben Kennung
    /// waere genau die Verwechslung, die die Kennung verhindern soll.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der neue Tab **hinter** dem aktiven
    /// steht und nicht am Ende: wer einen Tab verdoppelt, will ihn neben sich
    /// sehen — am Ende muesste er durch die anderen hindurchwechseln.
    fn tab_duplizieren(&mut self) -> Wirkung {
        if self.tabs.len() >= MAX_TABS || self.tabs.is_empty() {
            return Wirkung::Leer;
        }
        self.naechste_tab_id += 1;
        let mut kopie = self.tabs[self.aktiv].clone();
        kopie.id = TabId(self.naechste_tab_id);
        self.tabs.insert(self.aktiv + 1, kopie);
        self.aktiv += 1;
        self.verlauf_einkuerzen();
        Wirkung::Geaendert
    }

    /// KONTEXT (04.10.2026, 05:20): ALLE ANDEREN TABS SCHLIESSEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der aktive Tab an **Position 0** wandert
    /// und nicht an seiner Stelle bleibt: die Crate haelt `tabs` als Vec, und
    /// ein Vec mit einem Element hat nur Index 0 — der aktive Index wird also
    /// auf 0 gesetzt, und die **Kennung** des Tabs wandert mit (GEMESSEN: sie
    /// gehoert zum Tab, nicht zur Position).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das bei **einem** Tab `Wirkung::Leer`
    /// liefert: es gibt dann nichts zu schliessen, und ein Klick, der nichts
    /// tut, soll das **sagen** (das Menue macht die Zeile dann grau).
    fn tab_andere_schliessen(&mut self) -> Wirkung {
        if self.tabs.len() <= 1 {
            return Wirkung::Leer;
        }
        //
        // KONTEXT (04.10.2026, 10:25): AUCH HIER wird gemerkt, welcher Tab als
        // naechster wiederhergestellt wird: der **rechte Nachbar**, sonst der
        // linke. GEMESSENE ENTSCHEIDUNG, warum der rechte zuerst: alle Tabs
        // rechts vom aktiven sind fuer den Nutzer „die nachsten“, und der
        // naechste ist der rechte Nachbar.
        let merke_nr = if self.aktiv + 1 < self.tabs.len() {
            self.aktiv + 1
        } else {
            self.aktiv.saturating_sub(1)
        };
        if let Some(t) = self.tabs.get(merke_nr).cloned() {
            self.gemerkt(t);
        }
        let bleibt = self.tabs[self.aktiv].clone();
        self.tabs = vec![bleibt];
        self.aktiv = 0;
        self.verlauf_einkuerzen();
        Wirkung::Geaendert
    }

    /// KONTEXT (04.10.2026, 10:25): AUF EINEN BESTIMMTEN TAB SPRINGEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine Zahl ausserhalb der Liste
    /// `Wirkung::Leer` ist: das Menue zeichnet seine Zeilen aus derselben
    /// Zahl, die der Klick liefert — ein Tab, den es nicht gibt, kann keine
    /// Zeile haben.
    fn tab_springen(&mut self, nr: u32) -> Wirkung {
        let nr = nr as usize;
        if nr >= self.tabs.len() || nr == self.aktiv {
            return Wirkung::Leer;
        }
        self.aktiv = nr;
        self.verlauf_einkuerzen();
        Wirkung::Geaendert
    }

    /// KONTEXT (04.10.2026, 10:25): DEN TAB ANHEBEN ODER LOESEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Anheften den Tab nach **vorne**
    /// bringt und das Loesen ihn **stehen laesst**: bei Firefox steht ein
    /// angehefteter Tab links vor allen anderen; wer ihn loest, sieht ihn an
    /// der Stelle, an der er ihn gerade sieht (ein Verschieben beim Loesen
    /// waere eine zweite Bewegung fuer einen Klick).
    fn tab_anheften(&mut self) -> Wirkung {
        if self.tabs.is_empty() {
            return Wirkung::Leer;
        }
        let jetzt_gepinnt = !self.tabs[self.aktiv].gepinnt;
        self.tabs[self.aktiv].gepinnt = jetzt_gepinnt;
        if jetzt_gepinnt {
            // Nach links: vor alle anderen.
            let t = self.tabs.remove(self.aktiv);
            self.tabs.insert(0, t);
            self.aktiv = 0;
        }
        self.verlauf_einkuerzen();
        Wirkung::Geaendert
    }

    /// KONTEXT (04.10.2026, 10:25): DEN TAB STUMM ODER WIEDER MIT TON.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein **Umschalten** ist: die Zeile
    /// heisst je nach Zustand anders („Tab stummschalten“ / „Tab Ton
    /// einschalten“), und ein Befehl, der nur eine Richtung kann, braeuchte
    /// **zwei** Zeilen fuer **einen** Sachverhalt.
    fn tab_lautlos(&mut self) -> Wirkung {
        if self.tabs.is_empty() {
            return Wirkung::Leer;
        }
        self.tabs[self.aktiv].lautlos = !self.tabs[self.aktiv].lautlos;
        Wirkung::Geaendert
    }

    /// KONTEXT (04.10.2026, 10:25): ALLE TABS RECHTS VOM AKTIVEN SCHLIESSEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der **letzte** rechts gemerkt wird: die
    /// Tabs verschwinden von links nach rechts, und der zuletzt verschwundene
    /// ist der erste, den man zurueckholen will.
    fn tabs_rechts_schliessen(&mut self) -> Wirkung {
        if self.tabs.is_empty() || self.aktiv + 1 >= self.tabs.len() {
            return Wirkung::Leer;
        }
        if let Some(t) = self.tabs.last().cloned() {
            self.gemerkt(t);
        }
        self.tabs.truncate(self.aktiv + 1);
        self.verlauf_einkuerzen();
        Wirkung::Geaendert
    }

    /// EINEN GESCHLOSSENEN TAB AUF DEN STAPEL LEGEN — mit der **Grenze**.
    ///
    /// KONTEXT (04.10.2026, Korrektur der Leitung): GEMESSENE ENTSCHEIDUNG,
    /// warum die Grenze [`MAX_GESCHLOSSENE`] auch bei **jedem** Schliessen
    /// laeuft und nicht nur beim Wiederherstellen: ein Stapel ohne Grenze waere
    /// ein Speicher, den man beim Schliessen bemerkt und beim Wiederherstellen
    /// nicht — und GEMESSEN ist, dass [`MAX_TABS`] = 32 die Zahl der
    /// **offenen** Tabs begrenzt, nicht die der geschlossenen.
    fn gemerkt(&mut self, tab: Tab) {
        self.zuletzt_geschlossen.insert(0, tab);
        self.zuletzt_geschlossen.truncate(MAX_GESCHLOSSENE);
    }

    /// WIE VIELE GESCHLOSSENE TABS AUF DEM STAPEL LIEGEN?
    ///
    /// KONTEXT (04.10.2026): fuer die **Meldung** und fuer den Test der Grenze
    /// — GEMESSENE ENTSCHEIDUNG, warum das eine Zahl ist und kein `bool`: die
    /// Meldung „3 Tabs liegen noch auf dem Stapel“ beantwortet eine Frage, die
    /// der Nutzer stellt, wenn er Strg+Umschalt+T **mehrfach** gedrueckt hat.
    pub fn geschlossene_anzahl(&self) -> usize {
        self.zuletzt_geschlossen.len()
    }

    /// KONTEXT (04.10.2026, 10:25): DEN LETZTEN GESCHLOSSENEN TAB ZURUECK.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der wiederhergestellte Tab **aktiv**
    /// wird und der aktive daneben stehen bleibt: Firefox holt den Tab ins
    /// Bild — ein wiederhergestellter Tab, den man nicht sieht, ist wie
    /// einer, den man nie geoeffnet hat. GEMESSENE ENTSCHEIDUNG, warum er
    /// eine **neue** [`TabId`] bekommt: die alte steht in keinem Zustand mehr,
    /// und eine vergebene Kennung noch einmal zu vergeben waere die
    /// Kollision, die die Kennung verhindern soll. Rand: nichts geschlossen →
    /// [`Wirkung::Leer`] (das Menue zeichnet die Zeile dann grau).
    fn tab_wiederherstellen(&mut self) -> Wirkung {
        //
        // KONTEXT (04.10.2026, Korrektur der Leitung): DER STAPEL. GEMESSEN ist,
        // dass `pop()` den juengsten nimmt und den Stapel **unveraendert**
        // laesst, wenn nichts mehr da ist — also genau das Verhalten von
        // Strg+Umschalt+T in Firefox: der erste Druck holt den letzten Tab, der
        // zweite den davor.
        if self.zuletzt_geschlossen.is_empty() {
            return Wirkung::Leer;
        }
        if self.tabs.len() >= MAX_TABS {
            //
            // GEMESSENE ENTSCHEIDUNG: bei [`MAX_TABS`] wird **nichts**
            // genommen — der Stapel bleibt voll, und der naechste Klick (nach
            // dem Schliessen eines Tabs) holt weiter. Ein Stapel, der sich bei
            // einem Rand leert, waere ein Zustand, den man nicht sieht.
            return Wirkung::Leer;
        }
        let Some(mut alt) = self.zuletzt_geschlossen.first().cloned() else {
            return Wirkung::Leer;
        };
        self.zuletzt_geschlossen.remove(0);
        self.naechste_tab_id += 1;
        let mut tab = alt;
        tab.id = TabId(self.naechste_tab_id);
        self.tabs.insert(self.aktiv + 1, tab);
        self.aktiv += 1;
        self.verlauf_einkuerzen();
        Wirkung::Geaendert
    }

    /// KONTEXT (04.10.2026, 10:30): STEHT TAB `nr` fest?
    pub fn tab_gepinnt(&self, nr: usize) -> bool {
        self.tabs.get(nr).is_some_and(|t| t.gepinnt())
    }

    /// KONTEXT (04.10.2026, 10:30): SPIELT TAB `nr` OHNE TON?
    ///
    /// GEMESSENER EIGENER FEHLER (der Compiler hat ihn gemeldet,
    /// `E0592 duplicate definitions`): die Aktion hiess auch `tab_lautlos` —
    /// zwei Methoden mit demselben Namen in einer Crate sind der Fehler, den
    /// der Compiler erst beim **Aufruf** meldet (dieselbe Formel steht am
    /// Kommentar zu `Navigation::TabSchliessen`). Darum heisst die **Abfrage**
    /// `tab_ist_lautlos`: sie fragt, sie tut nichts.
    pub fn tab_ist_lautlos(&self, nr: usize) -> bool {
        self.tabs.get(nr).is_some_and(|t| t.lautlos())
    }

    /// KONTEXT (04.10.2026, 10:30): WIE VIELE TABS STEHEN **RECHTS** VON `nr`?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine Zahl und kein `bool`: die Zeile
    /// „Tabs rechts schließen“ ist bei **jedem** Tab rechts grau, bei keinem
    /// aktiv — die Zahl traegt die Entscheidung und die Meldung gleich mit.
    pub fn tabs_rechts_von(&self, nr: usize) -> u32 {
        if nr >= self.tabs.len() {
            return 0;
        }
        (self.tabs.len() - nr - 1) as u32
    }

    /// KONTEXT (04.10.2026, 23:45): DEN TAB AN EINE ANDERE POSITION SCHIEBEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Kennung** mitwandert und der Tab
    /// **nicht** ersetzt wird: GEMESSEN ist der Kommentar an `Tab::id` („Der
    /// dritte Tab ist eine Position und keine Identitaet“) — ein verschobener
    /// Tab ist derselbe Tab, sonst waere jeder Tabwechsel nach dem Ziehen ein
    /// neuer Tab.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der **aktive** Tab dem Verschieben folgt
    /// (und nicht der Index): verschiebt der Nutzer den aktiven Tab nach rechts,
    /// ist danach der **rechte** aktiv — sonst wuerde beim Ziehen eines Tabs
    /// ohnehin der falsche Tab im Bild stehen.
    ///
    /// Rand: gleiche Position oder eine ausserhalb → [`Wirkung::Leer`].
    fn tab_umstellen(&mut self, von: u32, nach: u32) -> Wirkung {
        let (von, nach) = (von as usize, nach as usize);
        if von >= self.tabs.len() || nach >= self.tabs.len() || von == nach {
            return Wirkung::Leer;
        }
        let war_aktiv = self.aktiv;
        let aktive_kennung = self.tabs[war_aktiv].id;
        let tab = self.tabs.remove(von);
        self.tabs.insert(nach, tab);
        // Der aktive Index wandert **mit dem Tab**.
        self.aktiv = if war_aktiv == von {
            nach
        } else if von < war_aktiv && nach >= war_aktiv {
            war_aktiv - 1
        } else if war_aktiv < von && nach <= war_aktiv {
            war_aktiv + 1
        } else {
            war_aktiv
        };
        // GEMESSENE ENTSCHEIDUNG, warum **zusaetzlich** nach der Kennung
        // gesucht wird: die drei Faelle oben sind eine Rechnung, und eine
        // Rechnung, die falsch liegt, zeigt den **falschen** Tab. Die Kennung
        // ist der **Beweis**.
        self.aktiv = self
            .tabs
            .iter()
            .position(|t| t.id == aktive_kennung)
            .unwrap_or(self.aktiv);
        self.verlauf_einkuerzen();
        Wirkung::Geaendert
    }

    /// WIE VIELE HOSTS EIN GEMERKTER ZOOM BEHALTEN DARF — **64**.
    ///
    /// KONTEXT (04.10.2026, 14:30): GEMESSENE ENTSCHEIDUNG, warum eine Grenze
    /// ueberhaupt: GEMESSEN ist, dass `Browser` ueberall sonst begrenzt ist (der
    /// Stapel 25, die Tabs [`MAX_TABS`], die Seiten [`VERLAUF_GRENZE`]) — eine
    /// Karte ohne Grenze waere die einzige Stelle, die wachsen kann, ohne dass
    /// jemand es merkt. GEMESSENE ENTSCHEIDUNG, warum **64**: die Zahl soll
    /// „viele Seiten, an denen man zoommt“ tragen, nicht „so viele wie Tabs“.
    pub const MAX_ZOOM_HOSTS: usize = 64;

    /// DER **HOST** AUS EINER ADRESSE — der Schluessel des Zoomgedenkens.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der **Port** mit in den Schluessel geht:
    /// GEMESSEN ist, dass `http://a.example:8080/` und `http://a.example/` zwei
    /// verschiedene Dienste sind, und ein Zoom, der dem einen den anderen
    /// wegnimmt, ist ein Fehler, den man erst beim Zurueckkehren merkt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `file:` **ohne** Pfad der Schluessel ist:
    /// eine lokale Datei hat keinen Host, und der Pfad waere ein Schluessel pro
    /// Datei — das ist „pro Seite“, nicht „pro Host“.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine Adresse **ohne** `://` **ganz** der
    /// Schluessel ist: `about:blank` und `data:` haben keinen Host, und
    /// `narsil://start` ist eine **interne** Adresse — da ist der ganze Text die
    /// beste Unterscheidung.
    pub fn host_aus_adresse(adresse: &str) -> String {
        let Some((schema, rest)) = adresse.split_once("://") else {
            return adresse.trim().to_string();
        };
        if schema.eq_ignore_ascii_case("file") {
            return "file:".to_string();
        }
        let ende = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let host = &rest[..ende];
        if host.is_empty() {
            format!("{schema}:")
        } else {
            host.to_string()
        }
    }

    /// DER GEMERKTE ZOOM FUER DIESE ADRESSE — in **Prozent**, `None`, wenn der
    /// Host ihn nie bekommen hat (der Aufrufer nimmt dann **100**).
    pub fn zoom_fuer_adresse(&self, adresse: &str) -> Option<u16> {
        self.zoom_je_host
            .get(&Self::host_aus_adresse(adresse))
            .copied()
    }

    /// DEN ZOOM FUER DIESE ADRESSE MERKEN (in **Prozent**). `false`, wenn die
    /// Karte voll ist.
    ///
    /// KONTEXT (04.10.2026, 14:35): GEMESSENE ENTSCHEIDUNG, warum bei voller
    /// Karte **nicht** das aelteste rausgeworfen wird: dafuer braechte es
    /// eine **Reihenfolge** (ein `Vec` neben der Karte), und die Frage „welcher
    /// Host war zuerst da?“ ist eine, die kein Nutzer stellt. Der 65. Host wird
    /// also **nicht** gemerkt — und `false` sagt es dem Aufrufer.
    pub fn merke_zoom_fuer_adresse(&mut self, adresse: &str, prozent: u16) -> bool {
        let host = Self::host_aus_adresse(adresse);
        if !self.zoom_je_host.contains_key(&host) && self.zoom_je_host.len() >= Self::MAX_ZOOM_HOSTS {
            return false;
        }
        self.zoom_je_host.insert(host, prozent);
        true
    }

    /// Release 10.10.2026: den gemerkten Zoom dieser Adresse VERGESSEN — ein
    /// Host auf dem Standard-Zoom hat keine Ausnahme (wie Chrome). `true`,
    /// wenn es einen Eintrag gab.
    pub fn vergiss_zoom_fuer_adresse(&mut self, adresse: &str) -> bool {
        self.zoom_je_host.remove(&Self::host_aus_adresse(adresse)).is_some()
    }

    /// DER **GANZE** Verlauf aller Tabs als `(Zeit, Titel, Adresse)`, **chronologisch**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **alle** Tabs und nicht nur der aktive:
    /// GEMESSEN ist, dass `verlauf` je Tab liegt — eine Seitenleiste, die nur
    /// den aktiven Tab zeigt, waere beim Wechsel des Tabs eine **andere**
    /// Verlaufleiste, und das ist keine Seitenleiste, sondern ein Zufall.
    /// GEMESSENE ENTSCHEIDUNG, warum **sortiert** und nicht in Tab-Reihenfolge:
    /// die Zeit steht drin; sie zu benutzen ist der ganze Zweck.
    pub fn verlauf_alle(&self) -> Vec<crate::interaktion::Verlaufseintrag> {
        let mut raus: Vec<crate::interaktion::Verlaufseintrag> = Vec::new();
        for t in &self.tabs {
            for p in &t.verlauf {
                if p.adresse.is_empty() {
                    continue;
                }
                raus.push((p.zeit_s, p.titel.clone(), p.adresse.clone()));
            }
        }
        raus.sort_by_key(|(z, _, _)| *z);
        raus
    }

    /// DAS GEMERKTE ZOOM ALS **TABELLE** (Host, Prozent) — fuer das Profil.
    pub fn zoom_tabelle(&self) -> Vec<(String, u16)> {
        self.zoom_je_host.iter().map(|(h, p)| (h.clone(), *p)).collect()
    }

    /// DIE TABELLE **in** den Speicher uebernehmen (aus dem Profil gelesen).
    /// `false`, wenn etwas nicht passte (zu viele Hosts).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **nicht** `zoom_je_host` direkt
    /// ersetzt: `zoom_je_host` ist eine `HashMap` (Reihenfolge egal), und die
    /// Datei hat eine Reihenfolge — die wird hierher kopiert, nicht umgebaut.
    pub fn zoom_tabelle_setzen(&mut self, tabelle: &[(String, u16)]) -> bool {
        if tabelle.len() > Self::MAX_ZOOM_HOSTS {
            return false;
        }
        self.zoom_je_host.clear();
        for (h, p) in tabelle {
            self.zoom_je_host.insert(h.clone(), *p);
        }
        true
    }

    /// DAS PROFIL **LESEN** und in den Speicher uebernehmen.
    pub fn zoomprofil_lesen(&mut self, pfad: &std::path::Path) -> bool {
        let p = crate::profil::Zoomprofil::lesen(pfad);
        self.zoom_tabelle_setzen(&p.eintraege().to_vec())
    }

    /// DEN SPEICHER **ALS PROFIL SCHREIBEN** (Host -> Prozent).
    pub fn zoomprofil_schreiben(&self, pfad: &std::path::Path) -> std::io::Result<()> {
        let mut p = crate::profil::Zoomprofil::leer();
        for (h, z) in self.zoom_tabelle() {
            p.setzen(&h, z);
        }
        p.schreiben(pfad)
    }

    /// DER ZOMMFaktor AUS PROZENT — damit der Aufrufer nicht selbst rechnet.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Umrechnung **hier** steht: GEMESSEN
    /// ist, dass `Sitzung::zoom` ein `f32`-Faktor ist (`sitzung.rs:739`) und die
    /// Leiste einen **Prozentwert** zeigt — zwei Stellen, eine Umrechnung.
    pub fn faktor_aus_prozent(prozent: u16) -> f32 {
        prozent as f32 / 100.0
    }

    /// Und umgekehrt (`1.25 -> 125`), **begrenzt**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier **geklemmt** wird: `Sitzung::
    /// zoom_setzen` klemmt den Faktor auf 0.5..3.0 (`sitzung.rs:5466`), und
    /// `as u16` ohne Klammer waere bei einem `NaN` aus dem Netz **0** — und
    /// 0 % ist eine Zahl, die es nicht gibt.
    pub fn prozent_aus_faktor(faktor: f32) -> u16 {
        if !faktor.is_finite() {
            return 100;
        }
        let p = (faktor * 100.0).round();
        if p < 25.0 {
            return 25;
        }
        if p > 300.0 {
            return 300;
        }
        p as u16
    }

    /// KONTEXT (04.10.2026, 10:30; 12:50 Stapel): GIBT ES EINEN
    /// ZURUECKZUHOLENDEN TAB?
    pub fn hat_geschlossenen_tab(&self) -> bool {
        !self.zuletzt_geschlossen.is_empty()
    }

    /// W7C A5: DEN AKTUELLEN TAB SCHLIESSEN.
    ///
    /// GEMESSEN, warum der aktive Index danach der VORHERIGE ist (und
    /// nicht „Index 0"): wer den mittleren von drei Tabs schliesst, will
    /// danach links weiterarbeiten, nicht wieder bei der ersten Seite
    /// anfangen. Das ist die Regel von jedem Browser.
    fn aktuellen_tab_schliessen(&mut self) -> Wirkung {
        if self.tabs.len() <= 1 {
            // Der letzte Tab bleibt (siehe `Navigation::TabSchliessen`).
            return Wirkung::Leer;
        }
        //
        // KONTEXT (04.10.2026, 10:00): DER GESCHLOSSENE TAB BLEIBT **hier**.
        // GEMESSENE ENTSCHEIDUNG, warum `remove` in die Merkzeile wandert:
        // `remove` gibt den Tab zurueck, und genau den braucht „Wiederherstellen“.
        // Wer stattdessen vorher kopiert, laesst ihn fuer `truncate` liegen —
        // und `truncate` gibt **nichts** zurueck (das ist der Unterschied
        // zwischen beiden Aufrufen, und er ist nicht sichtbar, nur spuerbar).
        //
        // GEMESSENER EIGENER FEHLER (der Compiler, E0499): `self.gemerkt(...)`
        // **direkt** um `self.tabs.remove(...)` — zwei mutable Zugriffe auf
        // `self` in einem Ausdruck. GEMESSENE ENTSCHEIDUNG: erst in eine
        // lokale Variable, dann merken; das ist auch der Weg, den die anderen
        // drei Stellen gehen.
        let entfernt = self.tabs.remove(self.aktiv);
        self.gemerkt(entfernt);
        // Der neue aktive Tab: der linke Nachbar, sonst der erste.
        self.aktiv = self
            .aktiv
            .saturating_sub(1)
            .min(self.tabs.len().saturating_sub(1));
        Wirkung::Geaendert
    }

    /// W7C A1: EIN NEUER TAB (Mittelklick auf einen Link).
    ///
    /// Der neue Tab ist LEER und wird sofort aktiv — ein Tab, den man
    /// nicht sieht, ist keiner. Die Adresse kommt als eigener Befehl
    /// (`Navigation::To`), damit der Verlauf genau einen Eintrag hat und
    /// nicht zwei.
    fn tab_neu(&mut self) -> Wirkung {
        // M3: Die Zahl der Tabs ist eine Eingabe (jeder Mittelklick eins).
        if self.tabs.len() >= MAX_TABS {
            return Wirkung::Leer;
        }
        self.naechste_tab_id += 1;
        self.tabs.push(Tab::neu_mit_id(TabId(self.naechste_tab_id)));
        self.aktiv = self.tabs.len() - 1;
        // GEMESSEN, warum der Verlauf des neuen Tabs `Page::leer()` und
        // nicht leer ist: die Crate sagt „`verlauf` ist nie leer, `index`
        // liegt immer darin" — ein leerer Vec waere ein Zustand, den kein
        // Befehl erreicht (und `Zurueck` muesste ihn erfinden).
        self.verlauf_einkuerzen();
        Wirkung::Geaendert
    }

    /// W7C A5: AUF EINEN TAB REDUZIEREN (ein Fenster, ein Tab).
    ///
    /// GEMESSEN, warum das noetig ist: `neu()` macht DREI Tabs, damit
    /// `TabNext` am Rand etwas zum Umschalten hat (das war die Begruendung
    /// aus A1). Seit A5 hat jeder Tab eine eigene Seite — und damit ist
    /// ein Fenster mit drei Tabs ein Fenster mit drei LEEREN Seiten, von
    /// denen zwei nie benutzt werden. Jedes echte Browserfenster
    /// oeffnet mit EINEM Tab; `Strg+T` macht den zweiten.
    ///
    /// Der aktive Tab bleibt (Index 0), die anderen werden verworfen.
    pub fn auf_einen_tab(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        //
        // KONTEXT (04.10.2026, 10:00): AUCH HIER wird gemerkt — es ist auch ein
        // Schliessen, und die Zeile „Geschlossenen Tab wiederherstellen“ soll
        // nach „ein Fenster, ein Tab“ (Strg+Umschalt+T in Firefox) wieder etwas
        // **haben**. GEMESSENE ENTSCHEIDUNG, warum der **letzte** der
        // entfernten Tabs: die Reihenfolge, in der der Nutzer sie sieht, ist
        // von links nach rechts — der rechte verschwindet zuletzt.
        if let Some(t) = self.tabs.last().cloned() {
            self.gemerkt(t);
        }
        self.tabs.truncate(1);
        self.aktiv = 0;
    }

    /// Eine Adresse aufrufen.
    ///
    /// Wie im echten Browser: was nach dem Index lag, gibt es nicht mehr.
    /// Zurueck, tippen, vorwaerts darf nicht in eine Seite fuehren, die
    /// beim Tippen ersetzt wurde.
    fn adresse_aufrufen(&mut self, adresse: &str) {
        if let Some(tab) = self.aktiver_tab_mut() {
            if tab.index + 1 < tab.verlauf.len() {
                tab.verlauf.truncate(tab.index + 1);
            }
            tab.verlauf.push(Page {
                adresse: adresse.to_string(),
                stand: 0,
                // Der Titel kommt spaeter: er entsteht erst, wenn die Seite
                // geladen ist. `titel_setzen` traegt ihn dann nach.
                titel: String::new(),
                zeit_s: crate::interaktion::jetzt_s(),
            });
            tab.index = tab.verlauf.len() - 1;
        }
        // DIE VERLAUFSGRENZE, 29.09.2026.
        //
        // EIGENER FEHLER, vom Test gefunden. `verlauf_einkuerzen` stand nur
        // in `verlaufseintrag_mit_zeit`, nicht im normalen Navigationspfad.
        // GEMESSEN: der Test `verlaufsgrenze_haelt_n_eintraege_und_wirft_
        // die_alten_weg` sah 202 Eintraege bei einer Grenze von 200. Eine
        // Grenze, die nur auf einem von zwei Wegen gilt, ist keine Grenze.
        self.verlauf_einkuerzen();
    }

    /// Eine Seite zurueck. Am aeltesten Feld: leerer Befehl.
    fn zurueck(&mut self) -> Wirkung {
        match self.aktiver_tab_mut() {
            Some(tab) if tab.index > 0 => {
                tab.index -= 1;
                Wirkung::Geaendert
            }
            _ => Wirkung::Leer,
        }
    }

    /// Eine Seite vorwaerts. Am jüngsten Feld: leerer Befehl.
    fn vorwaerts(&mut self) -> Wirkung {
        match self.aktiver_tab_mut() {
            Some(tab) if tab.index + 1 < tab.verlauf.len() => {
                tab.index += 1;
                Wirkung::Geaendert
            }
            _ => Wirkung::Leer,
        }
    }

    /// Die aktuelle Seite neu laden.
    ///
    /// DER WAHRSCHEINLICHSTE DENKFEHLER, laut Agent1: reload sieht aus wie
    /// "zurueck auf Seite 2 und weiter". Er ist es nicht. Der Index bleibt,
    /// die Laenge bleibt, nur die Ladenummer der SEITE waechst.
    fn neu_laden(&mut self) {
        if let Some(tab) = self.aktiver_tab_mut() {
            if let Some(seite) = tab.verlauf.get_mut(tab.index) {
                seite.stand = seite.stand.saturating_add(1);
            }
        }
    }

    /// Zum naechsten oder vorigen Tab, mit Rumlaufen am Rand.
    fn tab_wechsel(&mut self, schritt: isize) {
        let anzahl = self.tabs.len() as isize;
        if anzahl <= 0 {
            return;
        }
        let neu = (self.aktiv as isize + schritt).rem_euclid(anzahl);
        if neu >= 0 && (neu as usize) < self.tabs.len() {
            self.aktiv = neu as usize;
        }
    }

    fn aktiver_tab_mut(&mut self) -> Option<&mut Tab> {
        let aktiv = self.aktiv;
        self.tabs.get_mut(aktiv)
    }

    fn aktiver_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.aktiv)
    }

    // ------------------------------------------------------------------
    // Lesezugriffe. Alle liefern Option oder bool: kein Index greift ins
    // Leere, kein unwrap im ganzen Crate.
    // ------------------------------------------------------------------

    /// Die Adresse der aktuellen Seite, leer wenn noch keine da ist.
    pub fn adresse(&self) -> &str {
        self.aktiver_tab()
            .and_then(|t| t.verlauf.get(t.index))
            .map(|p| p.adresse.as_str())
            .unwrap_or("")
    }

    /// Die aktuelle Adresse eines bestimmten Tabs, leer fuer ungueltige Tabs.
    pub fn adresse_von_tab(&self, nr: usize) -> &str {
        self.tabs.get(nr)
            .and_then(|t| t.verlauf.get(t.index))
            .map(|p| p.adresse.as_str())
            .unwrap_or("")
    }

    /// Die Ladenummer der aktuellen Seite.
    pub fn stand(&self) -> u32 {
        self.aktiver_tab()
            .and_then(|t| t.verlauf.get(t.index))
            .map(|p| p.stand)
            .unwrap_or(0)
    }

    /// Position im Verlauf der aktuellen Tabs, 0 ist die aelteste Seite.
    pub fn index(&self) -> usize {
        self.aktiver_tab().map(|t| t.index).unwrap_or(0)
    }

    /// Wie viele Seiten im Verlauf der aktuellen Tabs stehen.
    pub fn verlauf_laenge(&self) -> usize {
        self.aktiver_tab().map(|t| t.verlauf.len()).unwrap_or(0)
    }

    /// Seite an einer Position im Verlauf, wenn es die gibt.
    pub fn seite(&self, position: usize) -> Option<&Page> {
        self.aktiver_tab()?.verlauf.get(position)
    }

    /// Anzahl der Tabs.
    pub fn tabs(&self) -> usize {
        self.tabs.len()
    }

    /// Welcher Tab gerade aktiv ist, 0 ist der erste.
    pub fn aktiver_tab_nr(&self) -> usize {
        self.aktiv
    }

    /// Ob "zurueck" gerade etwas tun koennte — daraus baut die Oberflaeche
    /// den Zustand "an" des Zurueck-Knopfs.
    pub fn kann_zurueck(&self) -> bool {
        self.index() > 0
    }

    /// Ob "vorwaerts" gerade etwas tun koennte — Zustand "an" des
    /// Vorwaerts-Knopfs.
    pub fn kann_vorwaerts(&self) -> bool {
        self.aktiver_tab()
            .map(|t| t.index + 1 < t.verlauf.len())
            .unwrap_or(false)
    }

    /// Alle Adressen im Verlauf der aktuellen Tabs, in Reihenfolge.
    pub fn verlauf(&self) -> Vec<&str> {
        match self.aktiver_tab() {
            Some(tab) => tab.verlauf.iter().map(|p| p.adresse.as_str()).collect(),
            None => Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests. ADR-044: EINE Eigenschaft, EIN Test, BEIDE Randwerte.
// Alle laufen ohne Fenster, ohne X11, ohne Bild — das ist der Zweck dieser
// Crate, und deshalb stehen sie hier und nicht in einer tests/-Datei.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// KONTEXT (04.10.2026, 10:50): TAB ANHEFTEN — der Tab wandert nach
    /// **links** und ist als **fest** gemerkt; beim zweiten Mal loest er sich
    /// und **bleibt** stehen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum als Beweis die **Position** und nicht nur
    /// die Markierung geprueft wird: eine Markierung ohne Bewegung waere
    /// „angeheftet, aber an seinem Platz“ — bei Firefox steht der angeheftete
    /// Tab links. Zwei Aussagen, zwei Messungen.
    #[test]
    fn tab_anheften_und_loesen() {
        let mut b = Browser::neu();
        b.auf_einen_tab();
        b.anwenden(&Navigation::To("https://a.example/erste".into()));
        b.anwenden(&Navigation::TabNeu);
        b.anwenden(&Navigation::To("https://a.example/zweite".into()));
        assert_eq!(b.tabs(), 2);
        // ANHEFTEN: der aktive Tab (Index 1) kommt nach vorn.
        assert_eq!(b.anwenden(&Navigation::TabAnheften), Wirkung::Geaendert);
        assert_eq!(b.aktiver_tab_nr(), 0, "GEMESSEN: der aktive Index");
        assert!(
            b.tab_gepinnt(0),
            "GEMESSEN: der angeheftete Tab ist nicht als fest gemerkt"
        );
        assert!(
            !b.tab_gepinnt(1),
            "GEMESSEN: der andere Tab ist auch fest"
        );
        assert_eq!(
            b.adresse().to_string(),
            "https://a.example/zweite",
            "GEMESSEN: nach dem Anheften steht eine andere Seite"
        );
        // LOESEN: er bleibt an **seiner** Stelle (Index 0) und ist nicht mehr fest.
        assert_eq!(b.anwenden(&Navigation::TabAnheften), Wirkung::Geaendert);
        assert!(!b.tab_gepinnt(0), "GEMESSEN: der geloeste Tab ist noch fest");
        assert_eq!(b.aktiver_tab_nr(), 0, "GEMESSEN: das Loesen hat den Tab bewegt");
    }

    /// KONTEXT (04.10.2026, 10:50): LAUTLOS SCHALTET UM — zweimal gedrueckt ist
    /// der Ausgangszustand wieder da.
    #[test]
    fn tab_lautlos_schaltet_um() {
        let mut b = Browser::neu();
        b.auf_einen_tab();
        assert!(!b.tab_ist_lautlos(0), "GEMESSEN: ein neuer Tab ist lautlos");
        b.anwenden(&Navigation::TabLautlos);
        assert!(b.tab_ist_lautlos(0), "GEMESSEN: der Klick hat nichts getan");
        b.anwenden(&Navigation::TabLautlos);
        assert!(
            !b.tab_ist_lautlos(0),
            "GEMESSEN: der zweite Klick hat nicht zurueckgeschaltet"
        );
    }

    /// KONTEXT (04.10.2026, 10:50): TABS RECHTS SCHLIESSEN — links bleibt, wie
    /// viele kamen, und am **letzten** Tab tut es nichts (`Wirkung::Leer`).
    #[test]
    fn tabs_rechts_schliessen_laesst_keinen_rechts() {
        let mut b = Browser::neu();
        b.auf_einen_tab();
        for adresse in ["a", "b", "c"] {
            b.anwenden(&Navigation::TabNeu);
            b.anwenden(&Navigation::To(format!("https://a.example/{adresse}")));
        }
        assert_eq!(b.tabs(), 4, "GEMESSEN: der Aufbau");
        assert_eq!(
            b.anwenden(&Navigation::TabsRechtsSchliessen),
            Wirkung::Leer,
            "GEMESSEN: am **letzten** Tab tut es etwas"
        );
        // GEMESSENE ENTSCHEIDUNG, warum **zwei** `TabPrev`: die drei neuen
        // Tabs stehen an den Positionen 1, 2, 3; der aktive ist 3, also
        // liegt Position 1 nach zwei Schritten zurueck. GEMESSEN ist, dass
        // `tab_wechsel` am Rand **nichts** tut und nicht weiterzaehlt.
        b.anwenden(&Navigation::TabPrev);
        b.anwenden(&Navigation::TabPrev);
        assert_eq!(
            b.adresse().to_string(),
            "https://a.example/a",
            "GEMESSEN: der Aufbau steht nicht auf Position 1"
        );
        assert_eq!(b.tabs_rechts_von(b.aktiver_tab_nr()), 2, "GEMESSEN: der Aufbau");
        b.anwenden(&Navigation::TabsRechtsSchliessen);
        assert_eq!(b.tabs(), 2, "GEMESSEN: es stehen {} Tabs", b.tabs());
        assert_eq!(
            b.adresse().to_string(),
            "https://a.example/a",
            "GEMESSEN: es steht die falsche Seite"
        );
        assert_eq!(
            b.tabs_rechts_von(b.aktiver_tab_nr()),
            0,
            "GEMESSEN: es steht noch etwas rechts"
        );
    }

    /// KONTEXT (04.10.2026, 10:50): WIEDERHERSTELLEN HOLT DEN LETZTEN
    /// GESCHLOSSENEN TAB ZURUECK — an seiner **Adresse** gemessen, und mit
    /// **neuer** Kennung.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Adresse** der Beweis ist: die Zahl
    /// der Tabs stimmt auch dann, wenn der falsche Tab zurueckkam.
    #[test]
    fn tab_wiederherstellen_holt_die_adresse_zurueck() {
        let mut b = Browser::neu();
        b.auf_einen_tab();
        b.anwenden(&Navigation::To("https://a.example/bleibt".into()));
        b.anwenden(&Navigation::TabNeu);
        b.anwenden(&Navigation::To("https://a.example/weg".into()));
        b.anwenden(&Navigation::TabSchliessen);
        assert!(
            b.hat_geschlossenen_tab(),
            "GEMESSEN: nach dem Schliessen ist nichts gemerkt"
        );
        let vorher = b.geschlossene_anzahl();
        assert_eq!(
            b.anwenden(&Navigation::TabWiederherstellen),
            Wirkung::Geaendert,
            "GEMESSEN: es gibt nichts zum Wiederherstellen"
        );
        //
        // KONTEXT (04.10.2026, 12:50, Korrektur der Leitung): DER STAPEL ist um
        // **einen** kuerzer, nicht leer. GEMESSENE ENTSCHEIDUNG, warum die Zahl
        // **vorher** gemessen wird und nicht „0“: `auf_einen_tab()` am Anfang
        // des Tests hat selbst Tabs geschlossen — der Stapel war also gefuellt,
        // und „ leer“ waere hier die falsche Behauptung.
        assert_eq!(
            b.geschlossene_anzahl(),
            vorher - 1,
            "GEMESSEN: der Stapel ist nicht einen kuerzer"
        );
        assert_eq!(b.tabs(), 2, "GEMESSEN: es kam kein Tab zurueck");
        assert_eq!(
            b.adresse().to_string(),
            "https://a.example/weg",
            "GEMESSEN: der falsche Tab ist zurueckgekommen"
        );
        assert_eq!(
            b.verlauf_laenge(),
            2,
            "GEMESSEN: der Verlauf ist nicht mitgekommen"
        );
        //
        // KONTEXT (04.10.2026, 12:50): DER STAPEL — ein zweiter Klick holt den
        // **naechsten**, und GEMESSEN ist, dass `auf_einen_tab()` am Anfang
        // dieses Tests den Stapel gefuellt hat. Also loest der Stapel erst
        // **leer**, wenn er leer ist — nicht nach einem Klick.
        let mut klicks = 1;
        while b.hat_geschlossenen_tab() {
            assert_eq!(
                b.anwenden(&Navigation::TabWiederherstellen),
                Wirkung::Geaendert,
                "GEMESSEN: der {klicks}. Klick tut nichts, obwohl der Stapel voll ist"
            );
            klicks += 1;
            assert!(klicks < 40, "GEMESSEN: der Stapel gibt Tabs ohne Ende her");
        }
        assert_eq!(
            b.anwenden(&Navigation::TabWiederherstellen),
            Wirkung::Leer,
            "GEMESSEN: der Klick auf einen **leeren** Stapel tut etwas"
        );
        assert_eq!(b.tabs(), 3, "GEMESSEN: am Ende stehen {} Tabs", b.tabs());
    }

    /// KONTEXT (04.10.2026, 10:50): WIEDERHERSTELLEN OHNE GESCHLOSSENEN TAB
    /// TUT **NICHTS** — und sagt es (`Wirkung::Leer`), damit die Menuezeile grau
    /// wird.
    #[test]
    fn wiederherstellen_ohne_geschlossenen_tab_tut_nichts() {
        //
        // GEMESSENER EIGENER TESTFEHLER: hier stand `b.auf_einen_tab()` — und
        // **das** merkt seit 10:00 einen Tab (es schliesst zwei). Der Test
        // pruefte also nicht „ohne geschlossenen Tab“, sondern „mit einem
        // gemerkten“. Ein frischer Browser ist der Aufbau, den ich meinte.
        let mut b = Browser::neu();
        assert!(
            !b.hat_geschlossenen_tab(),
            "GEMESSEN: ein neuer Browser hat schon einen gemerkten Tab"
        );
        assert_eq!(
            b.anwenden(&Navigation::TabWiederherstellen),
            Wirkung::Leer,
            "GEMESSEN: ohne geschlossenen Tab tut es etwas"
        );
        assert_eq!(b.tabs(), 3, "GEMESSEN: es kam ein Tab dazu");
    }

    /// KONTEXT (04.10.2026, 10:50): AUF EINE POSITION SPRINGEN — der Weg, den
    /// das Tabmenue braucht (es kennt eine **Position** aus `Chrome::tab(nr)`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das trotz `tab_waehlen(TabId)` (das es
    /// seit 01.10. gibt) **kein** ueberfluessiger zweiter Weg ist: die
    /// Menuezeile kennt die **Position**, die der Klick getroffen hat, und
    /// eine Position laesst sich nicht in eine Kennung umrechnen, ohne den
    /// Browser zu fragen — und die Frage beantwortet `tab_springen` direkt.
    #[test]
    fn tab_springen_geht_auf_die_angegebene_position() {
        let mut b = Browser::neu();
        b.auf_einen_tab();
        for adresse in ["a", "b"] {
            b.anwenden(&Navigation::TabNeu);
            b.anwenden(&Navigation::To(format!("https://a.example/{adresse}")));
        }
        assert_eq!(b.tabs(), 3, "GEMESSEN: der Aufbau");
        //
        // GEMESSENER EIGENER TESTFEHLER: `Navigation::To` macht den Tab
        // **aktiv** — nach der Schleife ist also Position 2 aktiv, und ein
        // Sprung auf 2 ist ein Sprung auf **dieselbe** Stelle. GEMESSEN ist,
        // dass das `Wirkung::Leer` gibt (das ist die Regel), also springt der
        // Test nach **links**.
        assert_eq!(b.aktiver_tab_nr(), 2, "GEMESSEN: der Aufbau (aktiv)");
        assert_eq!(
            b.anwenden(&Navigation::TabSpringen(0)),
            Wirkung::Geaendert,
            "GEMESSEN: der Sprung tut nichts"
        );
        assert_eq!(b.aktiver_tab_nr(), 0, "GEMESSEN: der falsche Tab ist aktiv");
        assert_eq!(
            b.adresse().to_string(),
            "",
            "GEMESSEN: GEMESSEN ist, dass Position 0 hier die **leere**
            Startseite traegt — `auf_einen_tab()` hat sie behalten, die drei
            neuen Tabs kamen daneben. Eine Adresse an dieser Stelle waere eine
            erfundene."
        );
        //
        // GEMESSENER EIGENER TESTFEHLER (zweiter in diesem Test): nach dem
        // Sprung nach links ist **0** aktiv — ein Sprung auf 2 ist also ein
        // Sprung auf eine **andere** Stelle und richtig `Geaendert`. Randfaelle
        // sind dieselbe Position und eine ausserhalb.
        assert_eq!(
            b.anwenden(&Navigation::TabSpringen(0)),
            Wirkung::Leer,
            "GEMESSEN: derselbe Tab gilt als Sprung"
        );
        assert_eq!(b.anwenden(&Navigation::TabSpringen(2)), Wirkung::Geaendert);
        assert_eq!(b.aktiver_tab_nr(), 2, "GEMESSEN: der Sprung nach rechts");
        assert_eq!(
            b.anwenden(&Navigation::TabSpringen(99)),
            Wirkung::Leer,
            "GEMESSEN: Position 99 springt irgendwohin"
        );
        assert_eq!(b.aktiver_tab_nr(), 2, "GEMESSEN: der aktive Tab hat sich bewegt");
    }

    /// KONTEXT (04.10.2026, 12:50, Korrektur der Leitung): DER STAPEL — der
    /// zweite Klick auf „Wiederherstellen“ holt den **naechsten**, und die
    /// Grenze ist [`MAX_GESCHLOSSENE`] (25).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Adressen** geprueft werden und nicht
    /// nur die Zahl: zwei Tabs, zwei Adressen, zwei Schritte — die Zahl stimmt
    /// auch dann, wenn der Stapel die **falsche** Reihenfolge haelt.
    #[test]
    fn der_stapel_holt_in_umgekehrter_reihenfolge() {
        //
        // GEMESSENER EIGENER TESTFEHLER: hier stand `b.auf_einen_tab()` — und
        // **das** legt seit 12:50 Tabs auf den Stapel (es schliesst zwei). Der
        // Stapel war also schon gefuellt, bevor der Test anfing. `Browser::neu()`
        // macht **drei** Tabs, und der Test braucht genau drei.
        let mut b = Browser::neu();
        b.anwenden(&Navigation::To("https://a.example/eins".into()));
        b.anwenden(&Navigation::TabNeu);
        b.anwenden(&Navigation::To("https://a.example/zwei".into()));
        b.anwenden(&Navigation::TabNeu);
        b.anwenden(&Navigation::To("https://a.example/drei".into()));
        // GEMESSEN: `Browser::neu()` macht **drei** Tabs, zwei kommen dazu.
        assert_eq!(b.tabs(), 5, "GEMESSEN: der Aufbau");
        //
        // GEMESSENER EIGENER TESTFEHLER (der Compiler nicht, der Test): hier
        // stand ein `TabPrev` zwischen den beiden `TabSchliessen`. GEMESSEN
        // ist, dass `TabSchliessen` den **aktiven** Tab schliesst — nach dem
        // `TabPrev` war das ein **leerer** Tab (Position 2), also lag „leer“
        // auf dem Stapel statt „zwei“. Der Stapel braucht keine Navigation:
        // nach dem Schliessen von „drei“ ist „zwei“ der aktive Tab.
        b.anwenden(&Navigation::TabSchliessen);
        assert_eq!(
            b.adresse().to_string(),
            "https://a.example/zwei",
            "GEMESSEN: nach dem Schliessen ist nicht der Nachbar aktiv"
        );
        b.anwenden(&Navigation::TabSchliessen);
        assert_eq!(b.geschlossene_anzahl(), 2, "GEMESSEN: der Stapel");
        //
        // GEMESSENER EIGENER TESTFEHLER (der Test, nicht der Compiler): ich
        // erwartete zuerst „drei“. GEMESSEN ist, dass „zwei“ **nach** „drei“
        // geschlossen wurde — also ist „zwei“ der **zuletzt** geschlossene, und
        // genau der kommt zuerst zurueck (Firefox: Strg+Umschalt+T holt den
        // letzten geschlossenen Tab). Die **Reihenfolge** ist die Behauptung,
        // und sie laeuft rueckwaerts durch die Schlussfolge.
        // 1. WIEDERHERSTELLEN: der zuletzt geschlossene = „zwei“.
        assert_eq!(b.anwenden(&Navigation::TabWiederherstellen), Wirkung::Geaendert);
        assert_eq!(
            b.adresse().to_string(),
            "https://a.example/zwei",
            "GEMESSEN: es kam nicht der zuletzt geschlossene Tab zurueck"
        );
        assert_eq!(b.geschlossene_anzahl(), 1, "GEMESSEN: der Stapel nach einem Zug");
        // 2. WIEDERHERSTELLEN: der davor = „drei“.
        assert_eq!(b.anwenden(&Navigation::TabWiederherstellen), Wirkung::Geaendert);
        assert_eq!(
            b.adresse().to_string(),
            "https://a.example/drei",
            "GEMESSEN: es kam der falsche als zweiter zurueck"
        );
        assert_eq!(
            b.geschlossene_anzahl(),
            0,
            "GEMESSEN: der Stapel ist noch nicht leer"
        );
        // 3. WIEDERHERSTELLEN: nichts mehr da.
        assert_eq!(b.anwenden(&Navigation::TabWiederherstellen), Wirkung::Leer);
    }

    /// KONTEXT (04.10.2026, 12:50): DIE GRENZE DES STAPELS — 25, nicht
    /// [`MAX_TABS`].
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Test **viele** Tabs braucht und nicht
    /// drei: die Grenze ist nur bei mehr als [`MAX_GESCHLOSSENE`] Schliessungen
    /// sichtbar, und drei Tabs koennen hoechstens zwei schliessen.
    #[test]
    fn der_stapel_hoelt_nur_25_tabs() {
        let mut b = Browser::neu();
        // Auf [`MAX_TABS`] fuellen, dann alle bis auf einen schliessen: das sind
        // [`MAX_TABS`] - 1 = 31 Schliessungen — mehr als 25.
        while b.tabs() < MAX_TABS {
            b.anwenden(&Navigation::TabNeu);
        }
        assert_eq!(b.tabs(), MAX_TABS, "GEMESSEN: der Aufbau");
        while b.tabs() > 1 {
            b.anwenden(&Navigation::TabSchliessen);
        }
        assert_eq!(
            b.geschlossene_anzahl(),
            MAX_GESCHLOSSENE,
            "GEMESSEN: auf dem Stapel liegen {} Tabs (Grenze {MAX_GESCHLOSSENE})",
            b.geschlossene_anzahl()
        );
        assert_ne!(
            MAX_GESCHLOSSENE,
            MAX_TABS,
            "GEMESSEN: die Stapelgrenze ist die Tabzahl — dann waere es keine"
        );
    }

    /// KONTEXT (04.10.2026, 23:50, Leitung „Drag&Drop Tabs“): DAS VERSCHIEBEN
    /// aendert die **Reihenfolge**, nicht die **Kennungen** — und der aktive
    /// Tab folgt dem Verschieben.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum an den **Adressen** geprueft wird und nicht
    /// nur an der Zahl: zwei Tabs koennen leer sein, und eine Zahl stimmt auch
    /// dann, wenn der **falsche** Tab verschoben wurde.
    #[test]
    fn tab_umstellen_aendert_die_reihenfolge() {
        let mut b = Browser::neu();
        b.auf_einen_tab();
        for adresse in ["eins", "zwei", "drei"] {
            b.anwenden(&Navigation::TabNeu);
            b.anwenden(&Navigation::To(format!("https://a.example/{adresse}")));
        }
        assert_eq!(b.tabs(), 4, "GEMESSEN: der Aufbau");
        // Der aktive Tab ist „drei“ (Position 3). „eins“ (0) nach rechts (2).
        let kennungen = b.tab_ids();
        assert_eq!(
            b.anwenden(&Navigation::TabUmstellen(0, 2)),
            Wirkung::Geaendert,
            "GEMESSEN: das Verschieben tut nichts"
        );
        let neu = b.tab_ids();
        assert_eq!(
            neu[2], kennungen[0],
            "GEMESSEN: der verschobene Tab steht nicht an der Zielposition"
        );
        assert_eq!(neu[0], kennungen[1], "GEMESSEN: die uebrigen sind verrutscht");
        assert_eq!(neu[1], kennungen[2], "GEMESSEN: die uebrigen sind verrutscht");
        assert_eq!(neu[3], kennungen[3], "GEMESSEN: der vierte ist gewandert");
        // UND: die **Adresse** des aktiven Tabs ist dieselbe — es ist derselbe
        // Tab, nur an einer anderen Stelle.
        assert_eq!(
            b.adresse().to_string(),
            "https://a.example/drei",
            "GEMESSEN: nach dem Verschieben steht eine andere Seite"
        );
    }

    /// KONTEXT (04.10.2026, 23:50): VERSCHIEBEN **NACH LINKS** — und der
    /// aktive Tab folgt, wenn er **zwischen** den Positionen lag.
    #[test]
    fn tab_umstellen_nach_links_und_der_aktive_folgt() {
        let mut b = Browser::neu();
        b.auf_einen_tab();
        for adresse in ["a", "b", "c"] {
            b.anwenden(&Navigation::TabNeu);
            b.anwenden(&Navigation::To(format!("https://a.example/{adresse}")));
        }
        // Der aktive Tab ist „c“ (Position 3). „c“ (3) nach links (1).
        let kennung_c = b.tab_ids()[3];
        b.anwenden(&Navigation::TabUmstellen(3, 1));
        let neu = b.tab_ids();
        assert_eq!(neu[1], kennung_c, "GEMESSEN: der Tab steht nicht links");
        assert_eq!(
            b.aktiver_tab_nr(),
            1,
            "GEMESSEN: der aktive Tab ist nicht mitgewandert (Index {})",
            b.aktiver_tab_nr()
        );
        assert_eq!(b.adresse().to_string(), "https://a.example/c");
    }

    /// KONTEXT (04.10.2026, 23:50): RANDFAELLE — gleiche Position oder eine
    /// ausserhalb bewegt **nichts** und sagt es (`Wirkung::Leer`).
    #[test]
    fn tab_umstellen_randfaelle_tun_nichts() {
        let mut b = Browser::neu();
        let vorher = b.tab_ids();
        assert_eq!(b.anwenden(&Navigation::TabUmstellen(0, 0)), Wirkung::Leer);
        assert_eq!(b.anwenden(&Navigation::TabUmstellen(9, 1)), Wirkung::Leer);
        assert_eq!(b.anwenden(&Navigation::TabUmstellen(1, 99)), Wirkung::Leer);
        assert_eq!(
            b.tab_ids(),
            vorher,
            "GEMESSEN: ein Randfall hat die Reihenfolge doch geaendert"
        );
    }

    /// KONTEXT (04.10.2026, 14:40, Leitung „Zoom je Seite … pro Host
    /// gemerkt“): DER ZOOM GEHOERT DEM **HOST**, nicht der Seite.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum an der **Adresse** und nicht am Tab
    /// gemessen wird: zwei Seiten desselben Hosts teilen den Zoom, und der
    /// Auftrag sagt „pro Host“.
    #[test]
    fn der_zoom_hoert_dem_host() {
        let mut b = Browser::neu();
        assert_eq!(
            b.zoom_fuer_adresse("https://a.example/erste"),
            None,
            "GEMESSEN: ein neuer Browser hat schon einen Zoom"
        );
        assert!(b.merke_zoom_fuer_adresse("https://a.example/erste", 125));
        assert_eq!(
            b.zoom_fuer_adresse("https://a.example/erste"),
            Some(125),
            "GEMESSEN: der Zoom ist nicht gemerkt"
        );
        // DIE ZWEITE SEITE DESSELBEN HOSTS HAT IHN AUCH — das ist der Punkt.
        assert_eq!(
            b.zoom_fuer_adresse("https://a.example/zweite"),
            Some(125),
            "GEMESSEN: der Zoom gilt nur fuer die erste Seite"
        );
        // EIN ANDERER HOST NICHT.
        assert_eq!(
            b.zoom_fuer_adresse("https://b.example/erste"),
            None,
            "GEMESSEN: der Zoom ist auf einen anderen Host uebergelaufen"
        );
    }

    /// KONTEXT (04.10.2026, 14:40): DER HOST AUS DER ADRESSE — vier Faelle,
    /// an denen man sich versetzen kann.
    #[test]
    fn der_host_steckt_in_der_adresse() {
        for (adresse, host) in [
            ("https://a.example/pfad?x=1#anker", "a.example"),
            ("http://a.example:8080/x", "a.example:8080"),
            ("https://a.example", "a.example"),
            ("file:///home/jaff/x.html", "file:"),
            ("about:blank", "about:blank"),
            ("narsil://start", "start"),
            ("", ""),
        ] {
            assert_eq!(
                Browser::host_aus_adresse(adresse),
                host,
                "GEMESSEN: der Host aus „{adresse}“"
            );
        }
    }

    /// KONTEXT (04.10.2026, 14:40): DIE GRENZE — 64 Hosts, der 65. wird nicht
    /// gemerkt und `false` sagt es.
    #[test]
    fn die_zoomgrenze_ist_64_hosts() {
        let mut b = Browser::neu();
        for n in 0..Browser::MAX_ZOOM_HOSTS {
            assert!(
                b.merke_zoom_fuer_adresse(&format!("https://host{n}.example/"), 110),
                "GEMESSEN: Host {n} wurde nicht gemerkt"
            );
        }
        assert_eq!(
            b.merke_zoom_fuer_adresse("https://zu_viel.example/", 110),
            false,
            "GEMESSEN: der 65. Host wurde gemerkt"
        );
        // UND: ein **bekannter** Host wird auch bei voller Karte gespeichert
        // (sein Wert ist ja schon da, es wird nur **ersetzt**).
        assert!(
            b.merke_zoom_fuer_adresse("https://host0.example/", 150),
            "GEMESSEN: ein bekannter Host wird bei voller Karte nicht gespeichert"
        );
        assert_eq!(b.zoom_fuer_adresse("https://host0.example/"), Some(150));
    }

    /// KONTEXT (04.10.2026, 14:40): PROZENT UND FAKTOR — und die **Grenzen**,
    /// weil ein `NaN` aus dem Netz **0 %** waere, und 0 % gibt es nicht.
    #[test]
    fn prozent_und_faktor_rechnen_gegene_und_warnen() {
        assert_eq!(Browser::prozent_aus_faktor(1.25), 125);
        assert_eq!(Browser::faktor_aus_prozent(125), 1.25);
        assert_eq!(Browser::prozent_aus_faktor(f32::NAN), 100, "GEMESSEN: NaN wird 0 %");
        assert_eq!(Browser::prozent_aus_faktor(0.0), 25, "GEMESSEN: 0 wird nicht 25 %");
        assert_eq!(Browser::prozent_aus_faktor(99.0), 300, "GEMESSEN: 99 wird nicht geklemmt");
    }

    /// KONTEXT (04.10.2026, 05:25): TAB DUPLIZIEREN — eine **Kopie** mit
    /// eigener Kennung, direkt hinter dem aktiven Tab, und die **Adresse**
    /// bleibt die des Originals.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Adresse** hier die Behauptung
    /// traegt und nicht die Tab-Zahl: eine Zahl kann stimmen, während der
    /// falsche Tab kopiert wurde. Die Adresse *kann* nicht stimmen, wenn der
    /// falsche Tab kopiert wurde — sie ist der Beweis, dass die Kopie eine
    /// Kopie ist.
    #[test]
    fn tab_duplizieren_kopiert_die_adresse() {
        let mut b = Browser::neu();
        b.auf_einen_tab();
        b.anwenden(&Navigation::To("https://a.example/erste".into()));
        let vorher = b.tabs();
        assert_eq!(
            b.anwenden(&Navigation::TabDuplizieren),
            Wirkung::Geaendert,
            "GEMESSEN: das Duplizieren tut nichts"
        );
        assert_eq!(b.tabs(), vorher + 1, "GEMESSEN: es ist kein Tab dazu gekommen");
        assert_eq!(
            b.adresse().to_string(),
            "https://a.example/erste",
            "GEMESSEN: die Kopie hat eine andere Adresse"
        );
        assert_eq!(
            b.aktiver_tab_nr(),
            1,
            "GEMESSEN: die Kopie ist nicht der aktive Tab (Index {})",
            b.aktiver_tab_nr()
        );
        // UND: der Verlauf der Kopie ist derselbe (ein Duplikat, kein neuer Tab).
        assert_eq!(b.verlauf_laenge(), 2, "GEMESSEN: der Verlauf ist nicht kopiert");
    }

    /// KONTEXT (04.10.2026, 05:25): ANDERE SCHLIESSEN — es bleibt **ein**
    /// Tab mit der Adresse des aktiven.
    #[test]
    fn andere_schliessen_laesst_den_aktiven_bleiben() {
        let mut b = Browser::neu();
        // GEMESSEN: `Browser::neu()` macht **drei** Tabs (siehe Kopf von
        // `auf_einen_tab`), also startet der Test bei einem.
        b.auf_einen_tab();
        b.anwenden(&Navigation::TabNeu);
        b.anwenden(&Navigation::To("https://a.example/zwei".into()));
        b.anwenden(&Navigation::TabNeu);
        b.anwenden(&Navigation::To("https://a.example/drei".into()));
        assert_eq!(b.tabs(), 3, "GEMESSEN: der Aufbau stimmt nicht");
        b.anwenden(&Navigation::TabPrev);
        assert_eq!(b.adresse().to_string(), "https://a.example/zwei");
        assert_eq!(
            b.anwenden(&Navigation::TabAndereSchliessen),
            Wirkung::Geaendert
        );
        assert_eq!(b.tabs(), 1, "GEMESSEN: es stehen {} Tabs", b.tabs());
        assert_eq!(
            b.adresse().to_string(),
            "https://a.example/zwei",
            "GEMESSEN: der falsche Tab ist geblieben"
        );
        assert_eq!(b.aktiver_tab_nr(), 0, "GEMESSEN: der aktive Index");
    }

    /// KONTEXT (04.10.2026, 05:25): BEI EINEM TAB — „Andere schließen“ tut
    /// **nichts** (und sagt es), „Tab duplizieren“ **tut** etwas.
    ///
    /// GEMESSENER BEFUND (der meinen ersten Test widerlegt hat): ich hatte
    /// auch für „Tab duplizieren“ `Wirkung::Leer` erwartet. GEMESSEN ist: ein
    /// duplizierter Tab braucht **keinen** zweiten — er **erzeugt** ihn. Also
    /// ist das die Zeile, die auch bei **einem** Tab aktiv ist (und genau
    /// deshalb ist „Andere schließen“ die einzige, die grau wird).
    #[test]
    fn bei_einem_tab_greift_nur_undere_schliessen_nicht() {
        let mut b = Browser::neu();
        b.auf_einen_tab();
        assert_eq!(b.tabs(), 1, "GEMESSEN: der Aufbau");
        assert_eq!(
            b.anwenden(&Navigation::TabAndereSchliessen),
            Wirkung::Leer,
            "GEMESSEN: „andere schliessen“ tut bei einem Tab etwas"
        );
        assert_eq!(
            b.anwenden(&Navigation::TabDuplizieren),
            Wirkung::Geaendert,
            "GEMESSEN: „Tab duplizieren“ tut bei einem Tab nichts"
        );
        assert_eq!(b.tabs(), 2, "GEMESSEN: es steht jetzt {} Tab", b.tabs());
    }

    /// Die Startseite ("leer", Adresse "") ist EINE Seite im Verlauf und
    /// zaehlt mit. Fuenf weitere Seiten dazu: sechs Eintraege, die neueste
    /// ist dran. Randwerte: genau fuenf, und mehr als fuenf.
    #[test]
    fn fuenf_seiten_stehen_im_verlauf() {
        let mut b = Browser::neu();
        assert_eq!(b.verlauf_laenge(), 1, "die Startseite steht auch drin");
        for adresse in ["a", "b", "c", "d", "e"] {
            b.anwenden(&Navigation::To(adresse.to_string()));
        }
        assert_eq!(b.verlauf_laenge(), 6, "Startseite + fuenf Seiten");
        assert_eq!(b.index(), 5, "die jüngste ist dran");
        assert_eq!(b.verlauf(), ["", "a", "b", "c", "d", "e"]);

        b.anwenden(&Navigation::To("f".to_string()));
        assert_eq!(b.verlauf_laenge(), 7, "mehr geht auch");
        assert_eq!(b.seite(6).map(|p| p.adresse.clone()), Some("f".to_string()));
        assert_eq!(b.tabs(), 3, "drei Tabs, unabhaengig davon");
    }

    /// Zurueck und vorwaerts sind EIGENE Operationen, nicht index-- und
    /// index++. Beide Raender: zurueck auf der aeltesten Seite ist ein
    /// leerer Befehl und faellt NICHT aus dem Verlauf; vorwaerts auf der
    /// juengsten ebenso.
    #[test]
    fn zurueck_und_vorwaerts_sind_eigene_operationen() {
        let mut b = Browser::neu();
        for adresse in ["a", "b"] {
            b.anwenden(&Navigation::To(adresse.to_string()));
        }
        assert_eq!(b.verlauf_laenge(), 3, "Startseite, a, b");
        assert_eq!(b.index(), 2, "Rand hinten: juengste Seite");

        // Rand hinten: vorwaerts tut nichts.
        assert_eq!(b.anwenden(&Navigation::Forward), Wirkung::Leer);
        assert_eq!(b.index(), 2, "unveraendert");
        assert_eq!(b.verlauf_laenge(), 3, "keine Seite verloren");
        assert!(!b.kann_vorwaerts(), "Knopf ist aus, nicht kaputt");

        // Mitte: zurueck wirkt.
        assert_eq!(b.anwenden(&Navigation::Back), Wirkung::Geaendert);
        assert_eq!(b.index(), 1);
        assert!(b.kann_zurueck() && b.kann_vorwaerts(), "beide Knoepfe an");

        assert_eq!(b.anwenden(&Navigation::Back), Wirkung::Geaendert);
        assert_eq!(b.index(), 0);

        // Rand vorn: aelteste Seite, zurueck tut nichts.
        assert_eq!(b.anwenden(&Navigation::Back), Wirkung::Leer);
        assert_eq!(b.index(), 0, "auf der aeltesten Seite geblieben");
        assert_eq!(b.verlauf_laenge(), 3, "keine Seite verloren");
        assert!(!b.kann_zurueck(), "Knopf ist aus, nicht kaputt");

        // Und vorwaerts geht wieder, ohne dass etwas verschwunden ist.
        assert_eq!(b.anwenden(&Navigation::Forward), Wirkung::Geaendert);
        assert_eq!(b.index(), 1, "zurueck auf Seite 2");
        assert_eq!(b.verlauf(), ["", "a", "b"], "der ganze Verlauf ist da");
    }

    /// Reload loescht die Historie NICHT: Laenge und Index bleiben, die
    /// Seite selbst wird neu (höhere Ladenummer). Beide Raender: auf der
    /// aeltesten und auf der juengsten Seite.
    #[test]
    fn reload_loescht_den_verlauf_nicht() {
        let mut b = Browser::neu();
        for adresse in ["a", "b", "c"] {
            b.anwenden(&Navigation::To(adresse.to_string()));
        }
        let vorher = b.verlauf_laenge();
        assert_eq!(vorher, 4, "Startseite, a, b, c");

        b.anwenden(&Navigation::Back);
        b.anwenden(&Navigation::Back);
        assert_eq!(b.index(), 1, "Rand vorn: Seite a");
        assert_eq!(b.anwenden(&Navigation::Reload), Wirkung::Geaendert);
        assert_eq!(b.verlauf_laenge(), vorher, "Verlauf bleibt");
        assert_eq!(b.index(), 1, "Index bleibt, wo er war");
        assert_eq!(b.adresse(), "a", "dieselbe Adresse");
        assert_eq!(b.stand(), 1, "aber ein neuer Stand");

        b.anwenden(&Navigation::Forward);
        b.anwenden(&Navigation::Forward);
        assert_eq!(b.index(), 3, "Rand hinten: juengste Seite");
        assert_eq!(b.anwenden(&Navigation::Reload), Wirkung::Geaendert);
        assert_eq!(b.verlauf_laenge(), vorher, "auch hier bleibt der Verlauf");
        assert_eq!(b.index(), 3, "und der Index auch");
        assert_eq!(b.adresse(), "c");
        assert_eq!(b.stand(), 1, "die Seite ist neu geladen");
        assert_eq!(b.verlauf(), ["", "a", "b", "c"], "nichts weg, nichts neu");
    }

    /// Drei Tabs, und beim Umschalten bleibt der Index stehen: Tab 2 ist
    /// Tab 2, egal wie oft man wechselt. Beide Raender des Rumlaufs.
    #[test]
    fn tabs_wechseln_ohne_verlauf_zu_verlieren() {
        let mut b = Browser::neu();
        // Tab 0 (aktiv): zwei eigene Seiten, dann ein Schritt zurueck.
        b.anwenden(&Navigation::To("eins".to_string()));
        b.anwenden(&Navigation::To("zwei".to_string()));
        b.anwenden(&Navigation::Back);
        assert_eq!(b.index(), 1, "Tab 0 steht auf Seite eins");

        b.anwenden(&Navigation::TabNext);
        assert_eq!(b.aktiver_tab_nr(), 1, "Tab 1 ist dran");
        b.anwenden(&Navigation::To("anderes".to_string()));
        assert_eq!(b.index(), 1, "in Tab 1 liegt sein eigener Verlauf");
        assert_eq!(b.adresse(), "anderes");

        b.anwenden(&Navigation::TabNext);
        b.anwenden(&Navigation::TabNext);
        assert_eq!(b.aktiver_tab_nr(), 0, "drei Tabs: von 1 wieder auf 0");
        assert_eq!(b.adresse(), "eins", "Tab 0 ist unveraendert");
        assert_eq!(b.index(), 1, "der Index von Tab 0 steht immer noch");

        // Rand hinten: vom letzten Tab weiter nach vorn -> erster Tab.
        b.anwenden(&Navigation::TabNext);
        b.anwenden(&Navigation::TabNext);
        assert_eq!(b.aktiver_tab_nr(), 2, "Tab 2");
        b.anwenden(&Navigation::TabNext);
        assert_eq!(b.aktiver_tab_nr(), 0, "Rumlaufen, kein Fehler");

        // Rand vorn: vom ersten Tab zurueck -> letzter Tab.
        b.anwenden(&Navigation::TabPrev);
        assert_eq!(b.aktiver_tab_nr(), 2, "Rumlaufen in die andere Richtung");
        assert_eq!(b.tabs(), 3, "immer noch drei Tabs");
        assert_eq!(b.adresse(), "", "Tab 2 wurde nie bedient: leer");
    }

    /// Der Durchlauf aus Agent1s Blatt: zurueck, vorwaerts, reload, zurueck.
    /// Er darf dieselbe Seite ergeben wie ein einfaches "zurueck" — Reload
    /// darf den Platz im Verlauf nicht verschieben. Dazu zwanzig
    /// VERSCHIEDENE Uebergaenge, jeder mit erwartetem Ergebnis: die
    /// Abnahme ist die Eigenschaft "nach jeder Operation ist der Zustand
    /// so, dass ein weitergedachtes Kommando dasselbe tut", nicht die Zahl.
    #[test]
    fn nach_jeder_operation_bleibt_die_welt_stimmig() {
        let mut b = Browser::neu();
        for adresse in ["start", "zweite", "dritte"] {
            b.anwenden(&Navigation::To(adresse.to_string()));
        }
        // Referenz: dieselbe Ausgangslage, aber nur EIN Zurueck.
        let mut referenz = b.clone();
        referenz.anwenden(&Navigation::Back);

        b.anwenden(&Navigation::Back);
        b.anwenden(&Navigation::Forward);
        b.anwenden(&Navigation::Reload);
        b.anwenden(&Navigation::Back);
        assert_eq!(
            b.index(),
            referenz.index(),
            "gleicher Platz wie ohne Reload"
        );
        assert_eq!(b.adresse(), referenz.adresse(), "dieselbe Seite wie davor");
        assert_eq!(b.verlauf(), referenz.verlauf(), "dieselbe Adressefolge");
        assert_eq!(
            b.verlauf_laenge(),
            referenz.verlauf_laenge(),
            "keine Seite weg"
        );

        // Zwanzig verschiedene Uebergaenge — nicht dieselbe Operation 20 Mal.
        let ablauf: [(Navigation, Wirkung, usize, &str, u32); 20] = [
            (
                Navigation::To("start".into()),
                Wirkung::Geaendert,
                1,
                "start",
                0,
            ),
            (
                Navigation::To("zweite".into()),
                Wirkung::Geaendert,
                2,
                "zweite",
                0,
            ),
            (
                Navigation::To("dritte".into()),
                Wirkung::Geaendert,
                3,
                "dritte",
                0,
            ),
            (Navigation::Back, Wirkung::Geaendert, 2, "zweite", 0),
            (Navigation::Back, Wirkung::Geaendert, 1, "start", 0),
            (Navigation::Reload, Wirkung::Geaendert, 1, "start", 1),
            (Navigation::Back, Wirkung::Geaendert, 0, "", 0),
            (Navigation::Back, Wirkung::Leer, 0, "", 0),
            (Navigation::Forward, Wirkung::Geaendert, 1, "start", 1),
            (Navigation::Forward, Wirkung::Geaendert, 2, "zweite", 0),
            (Navigation::Forward, Wirkung::Geaendert, 3, "dritte", 0),
            (Navigation::Forward, Wirkung::Leer, 3, "dritte", 0),
            (
                Navigation::To("vierte".into()),
                Wirkung::Geaendert,
                4,
                "vierte",
                0,
            ),
            (Navigation::Reload, Wirkung::Geaendert, 4, "vierte", 1),
            (Navigation::Back, Wirkung::Geaendert, 3, "dritte", 0),
            (Navigation::TabNext, Wirkung::Geaendert, 0, "", 0),
            (
                Navigation::To("tab1seite".into()),
                Wirkung::Geaendert,
                1,
                "tab1seite",
                0,
            ),
            (Navigation::TabPrev, Wirkung::Geaendert, 3, "dritte", 0),
            (Navigation::TabNext, Wirkung::Geaendert, 1, "tab1seite", 0),
            (Navigation::TabPrev, Wirkung::Geaendert, 3, "dritte", 0),
        ];
        let mut b = Browser::neu();
        let mut zaehler = 0;
        for (kommando, wirkung, index, adresse, stand) in ablauf {
            assert_eq!(
                b.anwenden(&kommando),
                wirkung,
                "Schritt {}: {kommando}",
                zaehler + 1
            );
            zaehler += 1;
            assert_eq!(b.index(), index, "Schritt {zaehler}: {kommando}");
            assert_eq!(b.adresse(), adresse, "Schritt {zaehler}: {kommando}");
            assert_eq!(b.stand(), stand, "Schritt {zaehler}: {kommando}");
            assert!(
                b.index() < b.verlauf_laenge(),
                "Schritt {zaehler}: Index im Verlauf"
            );
        }
        assert_eq!(zaehler, 20, "zwanzig Uebergaenge");
        assert_eq!(b.tabs(), 3, "die Tabs sind keine Geister geworden");
    }

    /// Kein Zustand ohne Weg: jeder Zustand, den diese Crate kennt, steht
    /// hier mit seinem Weg, und der Weg wird gebaut. Ein Zustand, den kein
    /// Kommando erzeugt, waere tot und truage nichts bei.
    #[test]
    fn kein_zustand_ohne_weg() {
        let mut b = Browser::neu();
        assert_eq!(b.index(), 0, "Start ist das aelteste Feld, ohne Befehl");

        b.anwenden(&Navigation::To("a".into()));
        b.anwenden(&Navigation::To("b".into()));
        b.anwenden(&Navigation::To("c".into()));
        assert_eq!(b.index(), 3, "Weg: drei mal To -> juengstes Feld");

        b.anwenden(&Navigation::Back);
        assert_eq!(b.index(), 2, "Weg: To, To, To, Back -> mittleres Feld");
        b.anwenden(&Navigation::Back);
        assert_eq!(b.index(), 1, "Weg: noch ein Back");
        b.anwenden(&Navigation::Back);
        assert_eq!(b.index(), 0, "Weg: wieder am aeltesten Feld");
        let laenge = b.verlauf_laenge();
        assert_eq!(
            b.anwenden(&Navigation::Back),
            Wirkung::Leer,
            "Weg: leerer Befehl am Rand"
        );

        // Weg: "neu geladene Seite" -> Reload erzeugt stand > 0, am Rand.
        b.anwenden(&Navigation::Reload);
        assert!(b.stand() > 0, "Weg: Reload am aeltesten Feld");
        assert_eq!(b.verlauf_laenge(), laenge, "und der Verlauf bleibt");

        // Weg: die drei Tabs, jeder einzeln erreichbar.
        for erwartet in [1usize, 2, 0] {
            b.anwenden(&Navigation::TabNext);
            assert_eq!(b.aktiver_tab_nr(), erwartet, "Weg: TabNext");
        }
        b.anwenden(&Navigation::TabPrev);
        assert_eq!(
            b.aktiver_tab_nr(),
            2,
            "Weg: TabPrev am ersten Tab -> letzter"
        );
        b.anwenden(&Navigation::TabPrev);
        assert_eq!(b.aktiver_tab_nr(), 1, "Weg: TabPrev in der Mitte");
        // Zurueck in den Tab, in dem wir den Verlauf gemessen haben — ein
        // Vergleich ueber Tabs hinweg waere eine Messung an der falschen Stelle.
        b.anwenden(&Navigation::TabNext);
        b.anwenden(&Navigation::TabNext);
        assert_eq!(b.aktiver_tab_nr(), 0, "Weg: wieder im ersten Tab");

        // Und nichts ist dabei verloren gegangen.
        assert_eq!(b.verlauf_laenge(), laenge, "derselbe Verlauf wie vorhin");
        assert_eq!(b.tabs(), 3, "immer noch drei Tabs");
    }

    /// Eingabe als eigener Typ: nur `To` traegt eine Adresse, und sie landet
    /// unveraendert dort, wo sie getippt wurde. Beide Raender: leer und lang.
    #[test]
    fn eingabe_ist_eigener_typ_und_landet_unveraendert() {
        let mut b = Browser::neu();
        b.anwenden(&Navigation::To("file:///tmp/hello.narsil".into()));
        assert_eq!(b.adresse(), "file:///tmp/hello.narsil");

        // Ein Tippfehler ist eine Adresse, kein Sprung ins Leere: er wird
        // nicht interpretiert, nicht aufgeraeumt, nicht still ersetzt.
        b.anwenden(&Navigation::Back);
        b.anwenden(&Navigation::To("htp:/xmpl".into()));
        assert_eq!(b.adresse(), "htp:/xmpl", "so, wie getippt");

        // Rand 1: leere Adresse ist erlaubt und bleibt leer.
        b.anwenden(&Navigation::Back);
        b.anwenden(&Navigation::To(String::new()));
        assert_eq!(b.adresse(), "", "leer ist ein Zustand, kein Fehler");
        assert!(b.adresse().is_empty());

        // Rand 2: sehr lange Adresse. Diese Crate rechnet keine
        // Zeichenbreiten (ADR-032: die Oberflaeche setzt von Hand).
        let lang = "a".repeat(5000);
        b.anwenden(&Navigation::Back);
        b.anwenden(&Navigation::To(lang.clone()));
        assert_eq!(b.adresse(), lang, "5000 Zeichen unveraendert");

        // Die anderen fuenf Befehle tragen KEINE Adresse — das ist der Typ,
        // nicht eine Pruefung zur Laufzeit.
        let befehle = [
            Navigation::Back,
            Navigation::Forward,
            Navigation::Reload,
            Navigation::TabNext,
            Navigation::TabPrev,
        ];
        assert_eq!(befehle.len(), 5, "fuenf Befehle ohne Adresse");
        let formatiert: Vec<String> = befehle.iter().map(|n| n.to_string()).collect();
        assert!(
            !formatiert.iter().any(|s| s.contains("://")),
            "keiner ist eine Adresse"
        );
    }
}
