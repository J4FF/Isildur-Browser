//! D12 — DIE WERKZEUGLEISTE: die Leiste, die Firefox in JEDEM Reiter oben hat.
//!
//! # WAS DIESE DATEI LOEST (gemessen am Bild, nicht am Wunsch)
//!
//! GEMESSEN am 03.10.2026 an den sieben Abnahmebildern
//! (`team/log/devtools_D*.png`, jeweils der PANEL-Ausschnitt): in allen sieben
//! Reitern beginnt der Inhalt IMMER mit einer Zeile aus freier Stueck, und
//! die Bedienelemente liegen irgendwo anders:
//!
//! | Reiter | Firefox | unser Stand 03.10. |
//! |---|---|---|
//! | Elemente | Leiste + rechts „Regeln / Berechnet / Layout" | nur Text „display: …" ohne Reiter |
//! | Konsole | Filterfeld, vier Stufen-Schalter, „Leeren" | nur eine Zaehlzeile |
//! | Quellen | vier Schrittknoepfe OBEN in der Leiste | unten links in der Dateiliste |
//! | Netzwerk | Filterfeld + „Alle loeschen", Zeitachse | nur Spaltenkoepfe |
//! | Leistung | „Neu messen", Diagramm mit Achse | Balken ohne Achse |
//! | Speicher | Speicher-Reiter (Cookies/localStorage/Cache) | drei Knöpfe (richtig, aber ohne Leiste) |
//! | WebAssembly | Schrittknoepfe + Modulwahl | Kopfzeile ohne Leiste |
//!
//! GEMESSENE ENTSCHEIDUNG, warum eine LEISTE und nicht sieben einzelne
//! Erweiterungen: Firefox hat in jedem Reiter dieselbe Zeile unter dem
//! Reiterband, mit derselben Hoehe und derselben Farbe. Sieben Reiter mit
//! sieben eigenen Kopfzeilen (Hoehe 21, 24, 27 px, verschiedene Farben) sind
//! sieben Sonderfaelle — und das ist die Fehlerklasse „Zahl stimmt, Bild
//! stimmt nicht" in ihrer Oberflaechenform.
//!
//! # DIE EINZIGE GEOMETRIE
//!
//! Zeichner (`zeichnen.rs`) und Klick (`narsil::devtools_bruecke`) brauchen
//! dieselben Zahlen. Sie stehen deshalb HIER, einmal:
//! [`aufteilung`] (Leiste und Inhalt), [`rechteck`] (ein Element),
//! [`element_unter`] (der Treffer). Alle drei sind `const fn` oder rein
//! rechnend — kein Zustand, keine zweite Wahrheit.
//!
//! # DIE WIRKUNG: WAS EIN KLICK TUT
//!
//! [`Wirkung`] ist eine **Sprache**, keine Funktion: der Aufrufer uebersetzt
//! sie in seinen Zustand. Zwei Staende gibt es, und beide werden benannt:
//!
//! * `Text` — die Zeile ist nur ein Text (kein Treffer).
//! * ein Zustand, den die Crate selbst kennt (`ElementeSpalte`, `KonsoleStufe`,
//!   `SpeicherBereich`) — der Aufrufer darf ihn genauso setzen.
//! * [`Aktion`] — das kann die Crate nicht (die Seite neu laden, neu messen,
//!   den Speicher des Browsers loeschen). Der Aufrufer entscheidet, was er
//!   tut; die Leiste sagt nur, WAS gewaehlt wurde.

use crate::Devtools;
use crate::d1_geruest::Reiter;
use crate::d3_konsole::StufeSchalter;
use crate::d5_quellen::Schritt;
use crate::d6_wasm::WasmSchritt;
use crate::d7_speicher::Bereich;
use narsil_core::geometry::Rect;

// ---------------------------------------------------------------------------
// Die Zahlen — jede mit ihrer Herkunft
// ---------------------------------------------------------------------------

/// Höhe der Werkzeugleiste in Pixeln.
///
/// GEMESSENE ENTSCHEIDUNG: Firefox hat 28 px (die Toolbar), Chrome 32.
/// `REITER_LEISTE_HOEHE` daneben ist 26 — die Leiste DARF NICHT gleich
/// dick sein wie das Reiterband darüber, sonst verschmelzen die beiden
/// Zeilen im Bild zu einem Block. 24 ist die Mitte: 20 px Element + 2 px
/// Luft oben und unten, und 2 px kleiner als das Reiterband.
pub const LEISTE_HOEHE: f32 = 24.0;

/// Höhe eines Bedienelements in Pixeln (der Knopf, das Feld).
///
/// GEMESSENE ENTSCHEIDUNG: `ZEILEN_HOEHE` ist 15 (eine Datenzeile), der
/// Knopf braucht Luft fuer den Rahmen: 20 px ist der kleinste Wert, bei dem
/// 13-px-Schrift nicht am Rand klebt (gemessen an `narsil-chrome`: die
/// Menueknoepfe sind 24 px hoch bei 13 px Schrift, ohne Rand 20).
pub const ELEMENT_HOEHE: f32 = 20.0;

/// Luft zwischen zwei Elementen, und zwischen Leiste und Rand.
pub const LUFT: f32 = 4.0;

/// Innenabstand des Textes im Knopf (links und rechts).
pub const INNEN: f32 = 8.0;

/// Breite eines Zeichens in Pixeln — dieselbe Zahl wie in `zeichnen::zentriert`.
///
/// GEMESSENE ENTSCHEIDUNG, warum NICHT gemessen wird: die Breite haengt von
/// der Schrift ab, und der Zeichencode hat keinen Schriftsatz (der Aufrufer
/// rastert). `zeichnen.rs` rechnet an vielen Stellen mit 7 px je Zeichen
/// (Cursorbalken, Spaltenkoepfe), also gilt dieselbe Zahl — sonst waere die
/// Breite eines Knopfes eine andere als die Stelle seines Textes.
pub const ZEICHEN: f32 = 7.0;

/// Kleinste Breite eines Knopfes.
///
/// GEMESSENE ENTSCHEIDUNG: `SCHRITT_BREITE` = 52 ist die kleinste Breite, bei
/// der die vier Schrittknoepfe (D5) in der linken Spalte (240 px) noch
/// passen. Dieselbe Zahl gilt hier fuer „Loeschen" (7 Zeichen = 49 + 16 = 65,
/// aber der Knopf soll nicht mit einem zweizeiligen Wort brechen).
pub const KNOPF_MIN: f32 = 52.0;

/// Breite eines Eingabefeldes.
///
/// GEMESSENE ENTSCHEIDUNG: 150 px = 21 Zeichen bei 7 px. Firefoxs
/// Konsolenfilter ist ~200 px breit; 21 Zeichen reichen fuer „console" oder
/// einen Fehlertext, und der Rest der Leiste bleibt fuer die Schalter.
pub const EINGABE_BREITE: f32 = 150.0;

/// Höchste Elemente in einer Leiste (M3/M4).
///
/// GEMESSENE ENTSCHEIDUNG: die groesste Liste ist die Konsole (Filter + vier
/// Schalter + „Leeren" = 6). 8 ist die Zusage „kein Reiter baut hier eine
/// Schleife ueber eine Zahl aus der Eingabe" — die Liste selbst ist `const`,
/// aber die TREFFERsuche laeuft ueber alle Elemente, und sie soll bei einer
/// kaputten Liste (z. B. per `mem::forget` eines Aufrufers nicht moeglich,
/// aber zur Sicherheit) nicht laenger laufen als die Liste lang ist.
pub const MAX_ELEMENTS: usize = 8;

// ---------------------------------------------------------------------------
// Die Art eines Elements
// ---------------------------------------------------------------------------

/// Wie ein Element aussieht und sich benimmt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    /// Ein Knopf: wird mit einem Klick gedrueckt, bleibt aber nicht unten.
    Knopf,
    /// Ein Umschalter: bleibt an, solange er gedrueckt ist (Firefox nennt das
    /// „Toggle button"). Erkennbar an der anderen Farbe.
    Umschalter,
    /// Ein Eingabefeld: nimmt Text an (Firefox: das Filterfeld).
    Eingabe,
}

/// Ob ein Element links oder rechts in der Leiste steht.
///
/// GEMESSENE ENTSCHEIDUNG: Firefox hat links die Auswahl (Filter, Reiter) und
/// rechts die Werkzeuge („Leeren", „Pause“-Knoepfe liegen aussen). Beide
/// Seiten in EINER Liste — nicht zwei Listen, denn die Reihenfolge der
/// Treffer muss stabil bleiben.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seite {
    /// Von links nach rechts.
    Links,
    /// Von rechts nach links (am rechten Rand ausgerichtet).
    Rechts,
}

/// Der rechte Spaltenreiter des Elemente-Reiters (Firefox: „Regeln",
/// „Berechnet", „Layout").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spalte {
    /// Die Eigenschaften, die eine CSS-Regel setzt.
    Regeln,
    /// Alle berechneten Eigenschaften (der lange Wert nach dem Namen).
    Berechnet,
    /// Das Box-Modell als Kachelbild (Marge/Rahmen/Fuellung/Inhalt).
    Layout,
}

impl Spalte {
    /// Die drei in der Reihenfolge der Leiste.
    pub const ALLE: [Spalte; 3] = [Spalte::Regeln, Spalte::Berechnet, Spalte::Layout];

    /// Der Name, wie er auf dem Reiter steht (deutsch, wie der Rest).
    pub const fn name(self) -> &'static str {
        match self {
            Spalte::Regeln => "Regeln",
            Spalte::Berechnet => "Berechnet",
            Spalte::Layout => "Layout",
        }
    }

    /// Der Index (0..2) — fuer Tests und fuer den Zustand als Zahl.
    pub const fn index(self) -> usize {
        match self {
            Spalte::Regeln => 0,
            Spalte::Berechnet => 1,
            Spalte::Layout => 2,
        }
    }

    /// Die Spalte an dieser Stelle, oder `None`.
    pub const fn an_index(i: usize) -> Option<Spalte> {
        match i {
            0 => Some(Spalte::Regeln),
            1 => Some(Spalte::Berechnet),
            2 => Some(Spalte::Layout),
            _ => None,
        }
    }
}

/// Etwas, das die Crate nicht selbst tun kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aktion {
    /// Die Seite neu laden (Netzwerk, WebAssembly, Quellen).
    SeiteNeu,
    /// Die Leistung neu messen (der Aufrufer ist der, der misst).
    LeistungNeu,
    /// Den echten Speicher des Browsers loeschen (Cookies/localStorage/Cache).
    SpeicherWirklichLoeschen,
    /// Das Bild der Hervorhebung im Seitenbereich ein-/ausblenden.
    Hervorheben,
}

/// Was ein Klick auf ein Element bewirkt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wirkung {
    /// Nur ein Text — kein Treffer, keine Wirkung.
    Text,
    /// Der rechte Spaltenreiter des Elemente-Reiters.
    ElementeSpalte(Spalte),
    /// Alle Knoten auf- oder zuklappen.
    ElementeUmbaum(bool),
    /// Die Hervorhebung ein-/ausschalten (ruft [`Aktion::Hervorheben`] fuer
    /// den Aufrufer auf, damit er das Rechteck in der Seite wegnimmt).
    ElementeHervorheben,
    /// Das Filterfeld der Konsole.
    KonsoleFilter,
    /// Ein Stufen-Schalter der Konsole.
    KonsoleStufe(StufeSchalter),
    /// Die Konsole leeren.
    KonsoleLeeren,
    /// Das Filterfeld des Netzwerks.
    NetzFilter,
    /// Das Protokoll leeren.
    NetzLeeren,
    /// Die Seite neu laden (Netzwerk).
    NetzNeu,
    /// Ein Schritt des JS-Debuggers (D5).
    Schritt(Schritt),
    /// Einen Haltepunkt in der gezeigten Zeile setzen/entfernen (D5).
    HaltepunktSetzen,
    /// Die Quellen neu aus der Seite lesen.
    QuellenNeu,
    /// Die Leistung neu messen.
    LeistungNeu,
    /// Die Leistung leeren.
    LeistungLeeren,
    /// Der Speicher-Reiter (Cookies/localStorage/Cache).
    SpeicherBereich(Bereich),
    /// Das Gewaehlte (oder alles) im gewaehlten Speicher-Bereich loeschen.
    SpeicherLoeschen,
    /// Ein Schritt des WASM-Debuggers (D6).
    WasmSchritt(WasmSchritt),
    /// Das Modul neu aus der Datei lesen.
    WasmNeu,
}

/// EIN Bedienelement der Leiste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Element {
    /// Der Text auf dem Element.
    pub beschriftung: &'static str,
    /// Wie es aussieht.
    pub art: Art,
    /// Links oder rechts.
    pub seite: Seite,
    /// Was ein Klick bewirkt.
    pub wirkung: Wirkung,
}

impl Element {
    /// Ein Knopf links.
    const fn k(b: &'static str, w: Wirkung) -> Self {
        Self {
            beschriftung: b,
            art: Art::Knopf,
            seite: Seite::Links,
            wirkung: w,
        }
    }
    /// Ein Umschalter links.
    const fn u(b: &'static str, w: Wirkung) -> Self {
        Self {
            beschriftung: b,
            art: Art::Umschalter,
            seite: Seite::Links,
            wirkung: w,
        }
    }
    /// Ein Eingabefeld links.
    const fn e(b: &'static str, w: Wirkung) -> Self {
        Self {
            beschriftung: b,
            art: Art::Eingabe,
            seite: Seite::Links,
            wirkung: w,
        }
    }
    /// Ein Element rechts (alle Reiter nutzen nur Knopf/Umschalter dort).
    const fn r(b: &'static str, w: Wirkung) -> Self {
        Self {
            beschriftung: b,
            art: Art::Knopf,
            seite: Seite::Rechts,
            wirkung: w,
        }
    }
}

// ---------------------------------------------------------------------------
// DIE SIEBEN LISTEN — als const, damit „jeder Reiter hat eine Leiste" eine
// Zusage ist und keine Eigenschaft, die ein Test jedes Mal neu prueft.
// ---------------------------------------------------------------------------

/// Elemente: Regeln/Berechnet/Layout links, Umbaum und Hervorheben rechts.
const LEISTE_ELEMENTE: [Element; 6] = [
    Element::u("Regeln", Wirkung::ElementeSpalte(Spalte::Regeln)),
    Element::u("Berechnet", Wirkung::ElementeSpalte(Spalte::Berechnet)),
    Element::u("Layout", Wirkung::ElementeSpalte(Spalte::Layout)),
    Element::k("Alle auf", Wirkung::ElementeUmbaum(true)),
    Element::k("Alle zu", Wirkung::ElementeUmbaum(false)),
    Element::r("Hervorheben", Wirkung::ElementeHervorheben),
];

/// Konsole: Filter, vier Stufen-Schalter, „Leeren" rechts.
const LEISTE_KONSOLE: [Element; 6] = [
    Element::e("Filter", Wirkung::KonsoleFilter),
    Element::u("Fehler", Wirkung::KonsoleStufe(StufeSchalter::Fehler)),
    Element::u("Warnung", Wirkung::KonsoleStufe(StufeSchalter::Warnung)),
    Element::u("Meldung", Wirkung::KonsoleStufe(StufeSchalter::Log)),
    Element::u("Ergebnis", Wirkung::KonsoleStufe(StufeSchalter::Ergebnis)),
    Element::r("Leeren", Wirkung::KonsoleLeeren),
];

/// Quellen: die vier Schrittknoepfe, der Haltepunkt, „Neu lesen" rechts.
const LEISTE_QUELLEN: [Element; 6] = [
    Element::k(">", Wirkung::Schritt(Schritt::Rein)),
    Element::k(">>", Wirkung::Schritt(Schritt::Ueber)),
    Element::k("<", Wirkung::Schritt(Schritt::Raus)),
    Element::k("v", Wirkung::Schritt(Schritt::Weiter)),
    Element::k("Haltepunkt", Wirkung::HaltepunktSetzen),
    Element::r("Neu lesen", Wirkung::QuellenNeu),
];

/// Netzwerk: Filter, „Alle loeschen", „Seite neu laden" rechts.
const LEISTE_NETZ: [Element; 3] = [
    Element::e("Filter", Wirkung::NetzFilter),
    Element::k("Alle loeschen", Wirkung::NetzLeeren),
    Element::r("Seite neu laden", Wirkung::NetzNeu),
];

/// Leistung: „Neu messen", „Leeren" — rechts die Summe als Text.
const LEISTE_LEISTUNG: [Element; 2] = [
    Element::k("Neu messen", Wirkung::LeistungNeu),
    Element::r("Leeren", Wirkung::LeistungLeeren),
];

/// Speicher: die drei Bereiche (wie die Firefox-Speicher-Reiter) + Loeschen.
const LEISTE_SPEICHER: [Element; 5] = [
    Element::u("Cookies", Wirkung::SpeicherBereich(Bereich::Cookies)),
    Element::u(
        "localStorage",
        Wirkung::SpeicherBereich(Bereich::Lokalspeicher),
    ),
    //
    // D18 (04.10., Auftrag der Leitung: „Cookies/localStorage/sessionStorage
    // lesen+bearbeiten"): der Zwilling braucht einen eigenen Knopf — sonst
    // waeren seine Eintraege nicht erreichbar. GEMESSEN: `localStorage` ist
    // 12 Zeichen breit, `sessionStorage` 14; bei 7 px je Zeichen bleiben
    // links 4 + 5 * 7 = 39 px fuer den Rest, und der steht rechts.
    Element::u("sessionStorage", Wirkung::SpeicherBereich(Bereich::Sitzung)),
    Element::u("Cache", Wirkung::SpeicherBereich(Bereich::Cache)),
    Element::r("Loeschen", Wirkung::SpeicherLoeschen),
];

/// WebAssembly: dieselben vier Schrittknoepfe wie in den Quellen und
/// „Neu laden" rechts.
///
/// GEMESSENE ENTSCHEIDUNG (03.10.): die Leiste nimmt die Arten, die
/// `WasmSchritt` wirklich hat — Rein/Ueber/Raus/Weiter, dieselben vier wie
/// `Schritt` in D5. Drei weitere Arten (`Zurueck`, `BisHaltepunkt`,
/// `Haltepunkt`) waeren zusaetzliche Faelle im WASM-Zustand, und die Frage
/// „welche vier" faellt dann nicht mehr im Bild auf, sondern nur noch im
/// Enum. Firefox hat im Debugger genau vier: hinein, ueber, heraus,
/// weiterlaufen.
const LEISTE_WASM: [Element; 5] = [
    Element::k(">", Wirkung::WasmSchritt(WasmSchritt::Rein)),
    Element::k(">>", Wirkung::WasmSchritt(WasmSchritt::Ueber)),
    Element::k("<", Wirkung::WasmSchritt(WasmSchritt::Raus)),
    Element::k("v", Wirkung::WasmSchritt(WasmSchritt::Weiter)),
    Element::r("Neu laden", Wirkung::WasmNeu),
];

/// Die Leiste des Reiters — **immer** eine, nie leer.
pub const fn leiste(r: Reiter) -> &'static [Element] {
    match r {
        Reiter::Elemente => &LEISTE_ELEMENTE,
        Reiter::Konsole => &LEISTE_KONSOLE,
        Reiter::Quellen => &LEISTE_QUELLEN,
        Reiter::Netzwerk => &LEISTE_NETZ,
        Reiter::Leistung => &LEISTE_LEISTUNG,
        Reiter::Speicher => &LEISTE_SPEICHER,
        Reiter::WebAssembly => &LEISTE_WASM,
    }
}

/// D18: Der Knopftext eines Speicher-Bereichs in der Werkzeugleiste.
///
/// GEMESSENE ENTSCHEIDUNG, warum es diese Abfrage in der LEISTE gibt und
/// nicht nur im Zeichencode: `Bereich::name()` (das Areal) und die
/// Beschriftung am Knopf (das Werkzeug) koennten auseinanderlaufen — und
/// genau das ist gemessen worden: `BEREICHE` hatte vier Eintraege, die Leiste
/// drei, und `bereich_knopf` lieferte fuer den vierten das ganze Panel
/// zurueck (der Knopf waere dort gelandet, wo keiner ist).
///
/// Der Test `d7_speicher` vergleicht beide Wege fuer jeden Bereich — das ist
/// der Beweis, dass es nur EINEN gibt.
pub fn bereich_knopf_name(b: crate::d7_speicher::Bereich) -> &'static str {
    for e in leiste(Reiter::Speicher).iter() {
        if e.wirkung == Wirkung::SpeicherBereich(b) {
            return e.beschriftung;
        }
    }
    ""
}

// ---------------------------------------------------------------------------
// Die Geometrie — eine Rechnung fuer Zeichnen und Treffer
// ---------------------------------------------------------------------------

/// Die Breite eines Elements.
///
/// GEMESSENE ENTSCHEIDUNG: Eingabefelder bekommen [`EINGABE_BREITE`] fest
/// (der Text wandert darin, die Breite nicht), Knoepfe und Umschalter
/// [`KNOPF_MIN`] oder so breit wie ihr Text plus [`INNEN`] an beiden Seiten.
/// „Neu laden" ist 9 Zeichen = 63 + 16 = 79 px; „v" ist 1 Zeichen und
/// behaelt die 52 px, damit die vier Schrittknoepfe eine TASTE breit sind
/// (D5: vier Tasten in 240 px).
pub fn breite(e: &Element) -> f32 {
    match e.art {
        Art::Eingabe => EINGABE_BREITE,
        Art::Knopf | Art::Umschalter => {
            (e.beschriftung.chars().count() as f32 * ZEICHEN + 2.0 * INNEN).max(KNOPF_MIN)
        }
    }
}

/// Die Leiste und der Inhalt darunter.
///
/// # WARUM DIE TRENNUNG HIER STEHT UND NICHT IM ZEICHNER
///
/// Der Zeichencode zeichnet die Leiste, der Bridge trifft sie, und ein Test
/// prueft beide. Drei Stellen, die dieselbe Zahl rechnen, sind drei Zahlen.
/// Also: EIN Aufruf, und alle drei benutzen ihn.
///
/// M3/M4: `f` kommt vom Fenster. Breite 0 und Hoehe < [`LEISTE_HOEHE`] sind
/// die beiden Enden, an denen `bottom() - LEISTE_HOEHE` unterlaufen wuerde —
/// die Inhaltshoehe ist `max(0.0, ...)`, damit der Panelcode mit einem
/// Rechteck von 0 Hoehe rechnen muss (und nicht mit einem Unterlauf).
pub fn aufteilung(f: Rect) -> (Rect, Rect) {
    let h = LEISTE_HOEHE.min(f.height.max(0.0));
    let leiste = Rect::new(f.x, f.y, f.width, h);
    let inhalt = Rect::new(f.x, f.y + h, f.width, (f.height - h).max(0.0));
    (leiste, inhalt)
}

/// Wo das Element `i` liegt, oder `None` wenn der Index zu gross ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Liste von vorn durchlaufen wird und
/// nicht „x direkt aus der Breite mal `i`": die Liste hat zwei Seiten. Die
/// linken Elemente liegen ab `f.x + LUFT` nebeneinander, die rechten ab
/// `f.right() - LUFT` rueckwaerts. Wer das in zwei Schleifen macht, hat
/// fuer jede Seite eine eigene Formel — und die muss dann zweimal stimmen.
pub fn rechteck(f: Rect, r: Reiter, i: usize) -> Option<Rect> {
    let l = leiste(r);
    if i >= l.len().min(MAX_ELEMENTS) {
        return None;
    }
    let mut x = f.x + LUFT;
    // Links: von vorn. `x` wandert ueber die linken Elemente.
    for e in l.iter().take(i) {
        if e.seite == Seite::Links {
            x += breite(e) + LUFT;
        }
    }
    // Das gesuchte Element, wenn es links liegt.
    let gesucht = l[i];
    if gesucht.seite == Seite::Links {
        let y = f.y + (LEISTE_HOEHE - ELEMENT_HOEHE) / 2.0;
        return Some(Rect::new(x, y, breite(&gesucht), ELEMENT_HOEHE));
    }
    // Rechts: von hinten. Erst die Breite aller RECHTEN Elemente bis `i`
    // (die hinter `i` stehenden stehen weiter aussen, sie fressen aber
    // keinen Platz vor `i`).
    let mut rx = f.right() - LUFT;
    for e in l.iter().skip(i) {
        if e.seite == Seite::Rechts {
            rx -= breite(e) + LUFT;
        }
    }
    let y = f.y + (LEISTE_HOEHE - ELEMENT_HOEHE) / 2.0;
    Some(Rect::new(
        rx - breite(&gesucht),
        y,
        breite(&gesucht),
        ELEMENT_HOEHE,
    ))
}

/// Das Element unter einem Punkt — der TREFFER (D9: eine Rechnung, ein Ort).
///
/// `Text` wird nie getroffen: es ist ein Text, kein Knopf. Sonst waere der
/// Klick auf die Beschriftung eines Elements etwas anderes als der Klick auf
/// den Knopf selbst — bei einem 20-px-Knopf ist das ein Unterschied, den man
/// im Bild sieht und im Betrieb nicht erklären kann.
pub fn element_unter(f: Rect, r: Reiter, x: f32, y: f32) -> Option<(usize, &'static Element)> {
    let l = leiste(r);
    for i in 0..l.len().min(MAX_ELEMENTS) {
        let Some(k) = rechteck(f, r, i) else {
            continue;
        };
        if k.contains(narsil_core::geometry::Point::new(x, y)) {
            // Die Liste ist ein &'static [Element] — der Treffer gibt also
            // einen Verweis auf das ELEMENT DER LISTE zurueck. Eine Kopie
            // (`let e = l[i]`) waere ein Verweis auf eine lokale Variable,
            // und den kann man nicht zurueckgeben (E0515).
            let e = &l[i];
            if e.wirkung == Wirkung::Text {
                return None;
            }
            return Some((i, e));
        }
    }
    None
}

/// Ist das Element gerade AN (Umschalter) — die Frage fuer den Zeichner.
///
/// GEMESSENE ENTSCHEIDUNG, warum das der ZUSTAND und nicht die Leiste
/// entscheidet: dieselbe Beschriftung ist in zwei Reitern etwas anderes
/// („Loeschen" im Speicher loescht, „Leeren" in der Konsole leert). Und
/// ein Schalter, der an ist, muss es aus dem Zustand wissen — nicht aus
/// dem, was der Zeichner das letzte Mal gemalt hat.
pub fn an(d: &Devtools, e: &Element) -> bool {
    match e.wirkung {
        Wirkung::ElementeSpalte(s) => d.elemente.spalte == s,
        Wirkung::ElementeHervorheben => d.elemente.hervorheben,
        Wirkung::KonsoleStufe(s) => d.konsole.stufen.an(s),
        Wirkung::SpeicherBereich(b) => d.speicher.reiter == b,
        _ => false,
    }
}

/// Der Text, der in einem Eingabefeld steht (sonst der Name).
pub fn feld_text(d: &Devtools, e: &Element) -> String {
    match e.wirkung {
        Wirkung::KonsoleFilter => d.konsole.filtertext.clone(),
        // GEMESSENE ENTSCHEIDUNG: das Feld heisst `filter` (nicht
        // `filtertext` wie in der Konsole) — der Zustand weiss selber,
        // wie sein Text heisst, der Zeichencode nicht.
        Wirkung::NetzFilter => d.netz.filter.clone(),
        _ => e.beschriftung.to_string(),
    }
}

/// Setzt die Wirkung, soweit sie in der Crate steht. Liefert, ob etwas
/// passiert ist (der Aufrufer prueft daran, ob er selbst ran muss).
pub fn wirke(d: &mut Devtools, e: &Element) -> bool {
    match e.wirkung {
        Wirkung::ElementeSpalte(s) => d.elemente.spalte_setzen(s),
        Wirkung::ElementeUmbaum(auf) => {
            if auf {
                d.elemente.alle_auf();
            } else {
                d.elemente.alle_zu();
            }
            true
        }
        Wirkung::ElementeHervorheben => {
            d.elemente.hervorheben = !d.elemente.hervorheben;
            true
        }
        Wirkung::KonsoleLeeren => {
            d.konsole.leeren();
            true
        }
        Wirkung::KonsoleStufe(s) => {
            d.konsole.stufen.schalte(s);
            true
        }
        Wirkung::NetzLeeren => {
            d.netz.leeren();
            true
        }
        Wirkung::LeistungLeeren => {
            d.leistung.leeren();
            true
        }
        // D12, GEMESSEN 18:0x: dieser Arm FEHLTE — die drei Reiter-Knoepfe
        // waren im Bild, im Hover und in `an()`, aber ein Klick blieb ohne
        // Wirkung. Siehe `Speicher::reiter_setzen`.
        Wirkung::SpeicherBereich(b) => d.speicher.reiter_setzen(b),
        // Die Aktionen kann die Crate nicht — der Aufrufer.
        _ => false,
    }
}

/// Die ([`LEISTE_HOEHE`], Inhalt) fuer einen Zustand — Kurzform fuer
/// Aufrufer, die den Inhaltsbereich brauchen (der Bridge, der Tests).
pub fn inhalt(d: &Devtools) -> Option<Rect> {
    let f = d.geruest.inhalt()?;
    Some(aufteilung(f).1)
}

#[cfg(test)]
mod werkzeug_eigene {
    use super::*;
    use narsil_core::geometry::Point;

    /// JEDER Reiter hat eine Leiste, und keine hat mehr als die Zusage.
    #[test]
    fn jeder_reiter_hat_eine_leiste() {
        for r in crate::d1_geruest::GERUEST {
            let l = leiste(r);
            assert!(!l.is_empty(), "{}: die Leiste ist leer", r.name());
            assert!(l.len() <= MAX_ELEMENTS, "{}: zu viele Elemente", r.name());
            for e in l.iter() {
                assert!(!e.beschriftung.is_empty(), "{}: leerer Text", r.name());
                assert_ne!(e.wirkung, Wirkung::Text, "{}: Text-Element", r.name());
            }
        }
    }

    /// DIE GEOMETRIE LIEGT IN DER LEISTE — gemessen, nicht behauptet.
    #[test]
    fn die_elemente_liegen_in_der_leiste() {
        let f = Rect::new(0.0, 600.0, 1200.0, 300.0);
        for r in crate::d1_geruest::GERUEST {
            for i in 0..leiste(r).len() {
                let k = rechteck(f, r, i).expect("Element");
                assert!(
                    k.y >= f.y - 0.01 && k.bottom() <= f.bottom() + 0.01,
                    "{}: Element {i} steht ausserhalb der Leiste: {k:?}",
                    r.name()
                );
                assert!(k.x >= f.x - 0.01, "{}: {i} links daneben", r.name());
                assert!(
                    k.right() <= f.right() + 0.01,
                    "{}: {i} rechts daneben",
                    r.name()
                );
                assert!(
                    k.height > 0.0 && k.width > 0.0,
                    "{}: {i} ohne Groesse",
                    r.name()
                );
            }
        }
    }

    /// KEINE ZWEI ELEMENTE LIEGEN UEBEREINANDER (der Treffer waere sonst
    /// von der Reihenfolge abhaengig).
    #[test]
    fn keine_zwei_elemente_liegen_uebereinander() {
        let f = Rect::new(0.0, 600.0, 1200.0, 300.0);
        for r in crate::d1_geruest::GERUEST {
            let mut k = Vec::new();
            for i in 0..leiste(r).len() {
                k.push(rechteck(f, r, i).expect("Element"));
            }
            for a in 0..k.len() {
                for b in a + 1..k.len() {
                    let ueber = k[a].right() > k[b].x + 0.01 && k[b].right() > k[a].x + 0.01;
                    assert!(
                        !ueber,
                        "{}: Element {a} ({:?}) und {b} ({:?}) liegen uebereinander",
                        r.name(),
                        k[a],
                        k[b]
                    );
                }
            }
        }
    }

    /// DER TREFFER FINDET JEDES ELEMENT — und der Mittelpunkt trifft genau
    /// das, was gezeichnet wurde (der Fehler „gezeichnet an der einen Stelle,
    /// getroffen an der anderen").
    #[test]
    fn der_treffer_findet_jedes_element() {
        let f = Rect::new(0.0, 600.0, 1200.0, 300.0);
        for r in crate::d1_geruest::GERUEST {
            for i in 0..leiste(r).len() {
                let k = rechteck(f, r, i).expect("Element");
                let mitte = Point::new(k.x + k.width / 2.0, k.y + k.height / 2.0);
                let (j, e) = element_unter(f, r, mitte.x, mitte.y)
                    .unwrap_or_else(|| panic!("{}: Element {i} nicht getroffen", r.name()));
                assert_eq!(j, i, "{}: Element {i} aber {j} getroffen", r.name());
                assert_eq!(
                    e.wirkung,
                    leiste(r)[i].wirkung,
                    "{}: andere Wirkung getroffen",
                    r.name()
                );
            }
        }
    }

    /// AUSSERHALB DER LEISTE IST NICHTS.
    #[test]
    fn ausserhalb_der_leiste_ist_nichts() {
        let f = Rect::new(0.0, 600.0, 1200.0, 300.0);
        let (_, inh) = aufteilung(f);
        for r in crate::d1_geruest::GERUEST {
            assert!(
                element_unter(f, r, 10.0, inh.y + 5.0).is_none(),
                "{}: unter der Leiste getroffen",
                r.name()
            );
            assert!(
                element_unter(f, r, 10.0, f.y - 5.0).is_none(),
                "{}: ueber der Leiste getroffen",
                r.name()
            );
        }
    }

    /// DIE KLEINE FLASCHE: 0 px Breite, 1 px Hoehe — kein Unterlauf.
    #[test]
    fn die_kleinste_flaeche_gibt_leer_nicht_panik() {
        for (b, h) in [(0.0f32, 0.0f32), (1.0, 1.0), (10.0, 5.0), (1200.0, 10.0)] {
            let f = Rect::new(0.0, 0.0, b, h);
            let (l, i) = aufteilung(f);
            assert!(l.height >= 0.0 && i.height >= 0.0, "Unterlauf bei {b}x{h}");
            for r in crate::d1_geruest::GERUEST {
                let _ = rechteck(f, r, 0);
                let _ = element_unter(f, r, f.x, f.y);
            }
        }
        // Und der Inhalt liegt IMMER unter der Leiste (M3: kein Unterlauf).
        let f = Rect::new(0.0, 0.0, 100.0, 3.0);
        let (l, i) = aufteilung(f);
        assert!(
            i.y >= l.bottom() - 0.01,
            "der Inhalt liegt ueber der Leiste"
        );
        assert!(i.height <= 0.01, "der Inhalt hat Höhe, die es nicht gibt");
    }

    /// DIE SPALTEN-REITER SIND DREI, UND IHR NAME STEHT IN DER LEISTE.
    #[test]
    fn die_drei_spalten_stehen_in_der_leiste() {
        let l = leiste(Reiter::Elemente);
        let namen: Vec<&str> = l
            .iter()
            .filter_map(|e| match e.wirkung {
                Wirkung::ElementeSpalte(s) => Some(s.name()),
                _ => None,
            })
            .collect();
        assert_eq!(namen, ["Regeln", "Berechnet", "Layout"]);
        for (i, s) in Spalte::ALLE.iter().enumerate() {
            assert_eq!(Spalte::an_index(i), Some(*s));
        }
        assert_eq!(Spalte::an_index(3), None);
        assert_eq!(Spalte::an_index(usize::MAX), None);
    }

    /// DER KNOPF AENDERT DEN ZUSTAND — sonst sieht die Leiste nur gut aus.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das der wichtigste Test der Leiste
    /// ist: `an()` und `wirke()` sind zwei Seiten derselben Sache, und eine
    /// Leiste kann vollstaendig gezeichnet sein und trotzdem nichts bewirken.
    /// Genau das ist die Fehlerklasse „Zahl stimmt, Bild stimmt nicht" in
    /// ihrer reinen Form: das Bild zeigt einen Umschalter, der Zustand
    /// bleibt, wie er war.
    #[test]
    fn der_knopf_aendert_den_zustand() {
        let mut d = crate::Devtools::neu(1200, 900);

        // Elemente: die drei Spalten-Reiter.
        //
        // GEMESSENE ENTSCHEIDUNG (03.10. 18:0x): der Anfangszustand ist
        // „Regeln", und `spalte_setzen` meldet `false`, wenn die Spalte
        // schon steht (es hat sich nichts geaendert). Der Test faengt deswegen
        // bei „Layout" an und prueft das Stehenbleiben ausdruecklich mit —
        // sonst waere der erste Schritt der einzige, der nichts bewirkt,
        // und genau das sieht man im Bild nicht.
        assert!(wirke(
            &mut d,
            &Element::u("Layout", Wirkung::ElementeSpalte(Spalte::Layout))
        ));
        for s in Spalte::ALLE {
            let k = Element::u("Spalte", Wirkung::ElementeSpalte(s));
            let war_aktiv = d.elemente.spalte == s;
            assert_eq!(
                wirke(&mut d, &k),
                !war_aktiv,
                "{s:?}: gewechselt={war_aktiv} verkehrt herum"
            );
            assert_eq!(d.elemente.spalte, s, "der Reiter hat nicht gewechselt");
            assert!(an(&d, &k), "der Reiter steht nicht als AN da");
        }

        // Elemente: Hervorheben ist ein Umschalter, kein Knopf.
        //
        // GEMESSENE ENTSCHEIDUNG (03.10. 18:0x): der Test hat zuerst behauptet,
        // der Umschalter stehe am Anfang AUS. Er steht AN — das ist der
        // Anfangszustand des Reiters. Der Test merkt sich deshalb den Zustand
        // und prueft das UMSCHALTEN, nicht einen festen Wert: ein fester Wert
        // ist hier die Buchhaltung des Zustands, nicht die Zusage der Leiste.
        let h = Element::u("Hervorheben", Wirkung::ElementeHervorheben);
        let vorher = d.elemente.hervorheben;
        assert_eq!(an(&d, &h), vorher, "der Umschalter sagt das Gegenteil");
        assert!(wirke(&mut d, &h));
        assert_eq!(d.elemente.hervorheben, !vorher, "nicht umgeschaltet");
        assert_eq!(an(&d, &h), !vorher, "das Bild zeigte den alten Zustand");
        assert!(wirke(&mut d, &h));
        assert_eq!(d.elemente.hervorheben, vorher, "zweimal geklickt hebt auf");

        // Konsole: die vier Stufen-Schalter schalten wirklich um.
        for s in [
            StufeSchalter::Fehler,
            StufeSchalter::Warnung,
            StufeSchalter::Log,
            StufeSchalter::Ergebnis,
        ] {
            let k = Element::u("Stufe", Wirkung::KonsoleStufe(s));
            assert!(an(&d, &k), "am Anfang steht {s:?} nicht an");
            assert!(wirke(&mut d, &k));
            assert!(!d.konsole.stufen.an(s), "{s:?} blieb an");
            assert!(wirke(&mut d, &k));
            assert!(d.konsole.stufen.an(s), "{s:?} blieb aus");
        }

        // Speicher: die drei Bereiche stehen als Reiter. Wie bei den
        // Spalten gilt: der Reiter, der schon steht, meldet `false`.
        for b in [Bereich::Cookies, Bereich::Lokalspeicher, Bereich::Cache] {
            let k = Element::u("Bereich", Wirkung::SpeicherBereich(b));
            let war_aktiv = d.speicher.reiter == b;
            assert_eq!(wirke(&mut d, &k), !war_aktiv, "{b:?}: falsch gemeldet");
            assert_eq!(d.speicher.reiter, b);
            assert!(an(&d, &k), "{b:?} steht nicht als AN da");
        }
    }

    /// WAS DIE CRATE NICHT KANN, MELDET ES — sonst prueft der Aufrufer
    /// „ich habe geklickt" und es ist nichts passiert.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `false` kein Fehler ist: Filtertext,
    /// Haltepunkte, „Neu laden", WASM-Schritt und Speicher-Loeschen brauchen
    /// Dinge, die ausserhalb der Crate liegen (Tastatur, Datei, Motor).
    /// `wirke` sagt das ehrlich, statt es zu schlucken.
    #[test]
    fn was_die_crate_nicht_kann_sagt_es() {
        let mut d = crate::Devtools::neu(1200, 900);
        let vorher = d.konsole.stufen;
        for e in [
            Element::e("Filter", Wirkung::KonsoleFilter),
            Element::e("Filter", Wirkung::NetzFilter),
            Element::k("Neu laden", Wirkung::NetzNeu),
            Element::k("Haltepunkt", Wirkung::HaltepunktSetzen),
            Element::r("Neu laden", Wirkung::WasmNeu),
            Element::k("v", Wirkung::WasmSchritt(WasmSchritt::Rein)),
            Element::r("Loeschen", Wirkung::SpeicherLoeschen),
        ] {
            assert!(!wirke(&mut d, &e), "{:?} behauptet eine Wirkung", e.wirkung);
        }
        assert_eq!(d.konsole.stufen, vorher, "die Konsole wurde doch beruehrt");

        // GEMESSENE KONTROLLE: „Leeren" KANN die Crate (sie loescht die
        // Zeilen) und wird deshalb hier nicht genannt. Sonst prueft der
        // Test eine Liste, die sich mit jeder Zeile Code im Zeichenpfad
        // aendert — und dann ist er die Buchhaltung, nicht die Zusage.
        d.konsole
            .ausgabe(crate::d3_konsole::Stufe::Log, "eine Meldung");
        assert!(!d.konsole.zeilen.is_empty());
        assert!(wirke(&mut d, &Element::r("Leeren", Wirkung::KonsoleLeeren)));
        assert!(
            d.konsole.zeilen.is_empty(),
            "die Konsole wurde nicht geleert"
        );
    }

    /// DAS FILTERFELD ZEIGT DEN TEXT AUS DEM ZUSTAND, sonst waere es tot.
    ///
    /// GEMESSENE ENTSCHEIDUNG: `feld_text` liest zwei verschieden benannte
    /// Felder (`konsole.filtertext` und `netz.filter`). Genau dieser
    /// Namensunterschied ist der Fehler, der am 03.10. als `E0609`
    /// auflief — und im Bild faellt er nicht auf, weil ein leeres Feld
    /// genauso aussieht wie ein unbenutztes.
    #[test]
    fn das_filterfeld_zeigt_seinen_text() {
        let mut d = crate::Devtools::neu(1200, 900);
        d.konsole.filtertext = "fehl".to_string();
        d.netz.filter = "github".to_string();
        let konsole = Element::e("Filter", Wirkung::KonsoleFilter);
        let netz = Element::e("Filter", Wirkung::NetzFilter);
        assert_eq!(feld_text(&d, &konsole), "fehl");
        assert_eq!(feld_text(&d, &netz), "github");
        // Und jedes andere Feld zeigt seinen Namen.
        assert_eq!(
            feld_text(&d, &Element::k("Alle loeschen", Wirkung::NetzLeeren)),
            "Alle loeschen"
        );
    }

    /// DIE WASM-LEISTE NIMMT DIE VIER ARTEN, DIE ES GIBT — jede genau einmal.
    ///
    /// GEMESSENE ENTSCHEIDUNG (03.10.): die Leiste stand auf vier Arten, die
    /// `WasmSchritt` nicht hatte (`Zurueck`, `BisHaltepunkt`, `Haltepunkt`).
    /// Das fiel als reiner Compilerfehler auf; ohne diesen Test waere es im
    /// naechsten Umbau wieder „Knopf da, Knopf tut nichts" geworden. Hier
    /// ist beides festgehalten: jede Art kommt vor, keine kommt zweimal, und
    /// die Zeichen stehen in der Reihenfolge der Liste.
    #[test]
    fn die_wasm_leiste_nimmt_die_vier_schritte() {
        let l = leiste(Reiter::WebAssembly);
        let mut gesehen: Vec<WasmSchritt> = Vec::new();
        let mut zeichen: Vec<&str> = Vec::new();
        for e in l.iter() {
            if let Wirkung::WasmSchritt(s) = e.wirkung {
                assert!(!gesehen.contains(&s), "{s:?} steht zweimal in der Leiste");
                gesehen.push(s);
                zeichen.push(e.beschriftung);
            }
        }
        assert_eq!(gesehen.len(), WasmSchritt::ALLE.len(), "nicht alle vier");
        for s in WasmSchritt::ALLE {
            assert!(gesehen.contains(&s), "{s:?} fehlt in der Leiste");
        }
        assert_eq!(zeichen, [">", ">>", "<", "v"]);
    }

    /// DIE LEISTE LIEGT IM WERKZEUGBEREICH — und der Inhalt darunter. Der
    /// Hover, den der Zeichner liest, muss dasselbe Element nennen wie der
    /// Treffer, den der Klick benutzt; sonst ist die Leiste an einer Stelle
    /// hell und an der anderen klickbar.
    #[test]
    fn die_leiste_liegt_im_werkzeugbereich() {
        let mut d = crate::Devtools::neu(1200, 900);
        d.geruest.setze_offen(true);
        let f = d
            .geruest
            .inhalt()
            .expect("offenes Geraest hat einen Bereich");
        let (leiste, inhalt) = aufteilung(f);
        assert!(leiste.height > 0.0, "die Leiste hat keine Hoehe");
        assert!(inhalt.height > 0.0, "der Inhalt hat keine Hoehe");
        assert!(
            inhalt.y >= leiste.bottom() - 0.01,
            "der Inhalt liegt drueber"
        );
        let x = leiste.x + 10.0;
        let y = leiste.y + 5.0;
        let treffer = element_unter(f, d.geruest.reiter(), x, y);
        let hov = d.geruest.hover_werkzeug(x, y);
        match (treffer, hov) {
            (Some((i, _)), Some(crate::d1_geruest::HoverFeld::Werkzeug(j))) => assert_eq!(i, j),
            _ => panic!("Treffer {treffer:?} und Hover {hov:?} sagen nicht dasselbe"),
        }
    }
}
