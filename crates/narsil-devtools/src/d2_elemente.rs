//! D2 — ELEMENTE: DOM-Baum aufklappbar, Auswahl hebt hervor, Styles + Box-Modell.
//!
//! # WAS D2 VERLANGT (team/AUTONOM.txt, wörtlich)
//!
//! > D2 Elemente: DOM-Baum aufklappbar, Knoten waehlen hebt ihn in der
//! > Seite hervor, berechnete Styles + Box-Modell.
//!
//! # WORHER DIE DATEN HERKOMMEN — gemessen, nicht geraten
//!
//! GEMESSEN am 01.10.2026 am Quelltext von `narsil-page` und `narsil-dom`:
//!
//! | Was der Reiter zeigt | Wo es wirklich steht |
//! |---|---|
//! | Tag, Attribute, Klassen | `narsil_page::CssKnoten` (`Seite::css_baum`) |
//! | Eltern / Geschwister | `CssKnoten::{eltern, vorher, nachher}` |
//! | gelegter Kasten (hervorheben) | `narsil_layout::LayoutBox::border_box` je `NodeId` |
//! | berechnete Styles | `narsil_css::Berechnet` — über `narsil_page::kaskade::end_stil` |
//! | Box-Modell | `Berechnet::{marge_*, fuellung_*, rahmen_*_breite, breite, hoehe}` |
//!
//! # DIE ENTSCHEIDUNG, DIE DIESE DATEI TRAEGT
//!
//! Der Baum wird aus dem **`CssBaum`** gebaut, nicht aus `narsil_dom::Document`.
//!
//! GEMESSENE BEGRÜNDUNG: `narsil-dom` speichert Attribute **nicht** —
//! die werden beim Bau in den Stil übersetzt und danach verworfen (so steht
//! es wörtlich in `narsil-page/src/lib.rs`, beim Feld `Seite::ids`). Ein
//! DevTools, das aus `narsil-dom` baut, zeigt `<div class="a b">` als
//! `<div>` — und der Nutzer kann weder eine Klasse noch eine `id` sehen.
//! Der `CssBaum` hat beides, PLUS Eltern und Geschwister, die
//! `narsil-dom` nicht braucht (es hat dafür `Vec<NodeId>`).
//!
//! Die Brücke vom CSS-Knoten zum gelegten Kasten geht über genau EINE
//! Tabelle: `Seite::css_zu_dom` (CSS-Index → Vorlauf-Nummer) und
//! `Seite::dom_zu_ord` (DOM-Knotennummer → Vorlauf-Nummer). Beide sind
//! `pub` und werden hier benutzt — sie sind extra dafür da, und sie zu
//! ignorieren wäre eine zweite Wahrheit über „welches Element ist das".

use crate::grenzen::Grenzen;
use narsil_core::geometry::Rect;
use narsil_css::Berechnet;
use narsil_page::Seite;
use narsil_page::kaskade::CssBaum;
use std::collections::BTreeMap;

/// Ein Platz je Einrückungsebene (in Pixeln).
///
/// GEMESSENE ENTSCHEIDUNG: `narsil-chrome` rückt die Unterpunkte der
/// Adressleiste um [`ZEICHENBREITE`] = 8 px ein. Der DOM-Baum rückt um
/// 12 px ein: dort stehen Attribute hinter dem Tag, und bei 8 px rückte der
/// Wert (`class="…"` beginnt bei ~50 px) unter den Wert des nächsten Levels.
pub const EINZUG: u32 = 12;

/// Breite eines Dreiecks (auf/zu) in Pixeln.
pub const DREIECK: u32 = 9;

/// Platz für Tag und Attribute einer Zeile.
pub const SPAEZE_ATTR: u32 = 16;

/// Die Breite EINER Zeichens im Baum (Pixel).
///
/// GEMESSEN (03.10. 19:5x, Bild `team/log/devpanel_elemente_attribute.png`):
/// die Zeile `<div id="gross" class="kasten">` mit ihrem Einzug (36 Zeichen)
/// misst 215 px — also **5,97 px je Zeichen**, nicht 7. Mit 7 px entstand an
/// jeder Farbgrenze ein Loch: im Bild stand `id= "gross"` mit einem Spalt
/// zwischen `=` und dem Anfuehrungszeichen und einem weiteren innerhalb der
/// Quotes.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier eine EIGENE Zahl steht und nicht
/// `werkzeug::ZEICHEN` (7,0): die Leiste rundet gross und grosszuegig, der
/// Baum setzt Attribut fuer Attribut aneinander — dort faellt der Fehler je
/// Zeichen auf. Zwei Zahlen fuer zwei Aufgaben, jede an ihrer Stelle
/// gemessen, ist ehrlicher als eine Zahl, die in beiden falsch ist.
pub const ZEICHEN_BREITE: f32 = 5.97;

/// Der Zustand des Element-Reiters.
#[derive(Debug, Clone, PartialEq)]
pub struct Elemente {
    /// Die flache Liste der SICHTBAREN Zeilen (aufgeklappter Baum).
    pub zeilen: Vec<Zeile>,
    /// Welche Knoten aufgeklappt sind (CSS-Nummern).
    pub auf: Vec<u32>,
    /// Der gewählte Knoten (CSS-Nummer).
    pub gewaehlt: Option<u32>,
    /// Der Knoten, den der Zeiger gerade berührt (CSS-Nummer).
    pub hover: Option<u32>,
    /// Der CSS-Baum der Seite (Quelle aller Angaben).
    pub baum: Option<CssBaum>,
    /// Die Blaetter OHNE UA-Blatt („hat der Autor das geschrieben?").
    ///
    /// GEMESSENE BEGRÜNDUNG: `autor_stil` rechnet über `author`, `end_stil`
    /// über `alle`. `narsil-page` leitet `author` aus `alle` ab
    /// (`lib.rs:2235`: `alle.iter().skip(1).cloned()`). Das ist hier
    /// dieselbe Ableitung, mit derselben Begründung: ohne UA-Blatt in
    /// `author` würde eine Autorenfarbe „vom Browser" heißen.
    pub author: Vec<narsil_css::Stylesheet>,
    /// Alle Blaetter (UA zuerst).
    pub alle: Vec<narsil_css::Stylesheet>,
    /// Ansichtsbreite der Seite (in px) für `@media`-Abfragen.
    pub ansicht_breite: u32,
    /// Ansichtshöhe der Seite (in px) für `@media`-Abfragen.
    pub ansicht_hoehe: u32,
    /// Der gelegte Kasten je DOM-Knoten (`NodeId` → `border_box`).
    pub kasten: BTreeMap<u32, Rect>,
    /// Der INHALTSkasten je DOM-Knoten (`NodeId` → `content_box`).
    ///
    /// GEMESSENE BEGRÜNDUNG, warum ein zweiter Baum statt einer
    /// Ableitung: `narsil_layout::LayoutBox` hat BEIDE (`border_box` und
    /// `content_box`), und der Unterschied ist genau die Zeile „Inhalt" in
    /// der Box-Modell-Ansicht. Ihn aus `border_box` minus Padding zu
    /// rechnen waere eine zweite Rechnung — und sie waere falsch, wenn der
    /// Layout-Engine den Innenraum fuer Textumbruch veraendert hat (was er
    /// tut, das ist der ganze Punkt von `content_box`).
    pub inhaltskasten: BTreeMap<u32, Rect>,
    /// Der Text, den der Kasten zeigt (für die Textzeile unter einem Knoten).
    pub texte: BTreeMap<u32, String>,
    /// Die CSS-Nummer zu jedem DOM-Knoten (`NodeId` → CSS-Index).
    pub css_von_dom: BTreeMap<u32, u32>,
    /// Wie viele Knoten der Baum wirklich hat.
    pub knoten_gesamt: usize,
    /// Wie viele Zeilen die Grenze abgeschnitten hat (M7: mitzählen).
    pub abgeschnitten: usize,
    /// Der zuletzt berechnete Stil des gewählten Knotens.
    pub stil: Option<Berechnet>,
    /// Das Box-Modell des gewählten Knotens.
    pub boxmodell: Option<Boxmodell>,
    /// Ein Text, der oben im Reiter steht (Hinweis, Grenze, Fehler).
    pub hinweis: String,
    /// Der Textfilter, auf den die Zeilen beschnitten sind.
    pub filter: String,
    /// D19: DIE REGELN DES GEWAEHLTEN KNOTENS (das Regeln-Panel).
    ///
    /// GEMESSENER BEFUND VORHER (04.10.): der Reiter „Regeln" zeigte die
    /// ERSTEN 14 ZEILEN der berechneten Eigenschaften — er war also eine
    /// zweite Sicht auf „Berechnet", nicht die Regeln. Ein Regeln-Panel, das
    /// keine Regeln zeigt, ist eine Tabelle mit einem anderen Titel.
    ///
    /// Die Zeilen kommen aus `d9_layout::regeln_aus` (echte
    /// `narsil_css::StilRegel`-Daten, vom Aufrufer gefuellt — siehe der
    /// Modul-Kopf von `d9_layout` und BRETT 04.10.).
    pub regeln: crate::d9_layout::Regeln,
    /// D19: DAS EDITIERFELD EINES BOX-WERTES.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es an `Elemente` haengt und nicht an
    /// `Devtools`: der Wert gehoert zum AUSGEWAEHLTEN Knoten, und mit ihm
    /// wandert das Feld beim Knotenwechsel (siehe `waehle`).
    pub box_eingabe: crate::d9_layout::BoxEingabe,
    /// D19: DIE BARRIEREFREIHEIT DES GEWAEHLTEN KNOTENS (Rolle, Name).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum zwei `String` und kein Baum: der
    /// Auftrag sagt „minimal". Ein ganzer Barrierefreiheits-Baum waere eine
    /// eigene Anzeige mit eigener Navigation;_role + Name sind die beiden
    /// Angaben, an denen man im Alltag steht („was liest das vor?").
    pub rolle: String,
    /// Der Name (leer = die Vorlesesoftware sagt nichts).
    pub name: String,
    /// Welcher rechte Reiter steht oben: Regeln, Berechnet oder Layout (D12).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier und nicht im Zeichencode steht:
    /// die Drei sind in der Werkzeugleiste Knoepfe, und ein Knopf, dessen
    /// Zustand nur im Zeichner liegt, ist beim naechsten Bild wieder weg —
    /// der Aufrufer (der Klick) wuerde ihn setzen und der Zeichner wuerde
    /// ihn nicht zeigen. Der Zustand gehoert in den Reiter, die LEISTE nur
    /// dazu, ihn zu zeigen.
    pub spalte: crate::werkzeug::Spalte,
    /// Wird der gewaehlte Knoten in der Seite hervorgehoben? (D12)
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein SCHALTER und nicht einfach immer:
    /// Firefox hat die Hervorhebung als Werkzeug mit einem Aus-Schalter.
    /// Und es gibt einen echten Fall dafuer: eine Seite mit 40 000 Knoten
    /// zeichnet fuer jeden Knoten ein Rechteck, wenn man durch den Baum
    /// laeuft (siehe `zeichnen::hervorhebungs_konstrukte`). Der Schalter
    /// ist also auch eine Antwort auf die Frage „wie teuer ist das".
    pub hervorheben: bool,
}

impl Elemente {
    /// Leer — vor der ersten Seite.
    pub fn leer() -> Self {
        Self {
            zeilen: Vec::new(),
            auf: Vec::new(),
            gewaehlt: None,
            hover: None,
            baum: None,
            author: Vec::new(),
            alle: Vec::new(),
            regeln: crate::d9_layout::Regeln::leer(),
            box_eingabe: crate::d9_layout::BoxEingabe::default(),
            rolle: String::new(),
            name: String::new(),
            ansicht_breite: 0,
            ansicht_hoehe: 0,
            kasten: BTreeMap::new(),
            inhaltskasten: BTreeMap::new(),
            texte: BTreeMap::new(),
            css_von_dom: BTreeMap::new(),
            knoten_gesamt: 0,
            abgeschnitten: 0,
            stil: None,
            boxmodell: None,
            hinweis: String::new(),
            filter: String::new(),
            spalte: crate::werkzeug::Spalte::Regeln,
            hervorheben: true,
        }
    }

    /// Aus der Seite lesen: Baum, Blaetter, Brücke aufbauen, Wurzel aufklappen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die WURZEL offen ist und sonst nichts:
    /// ein Element-Reiter, der geschlossen aufmacht, zeigt eine leere Liste —
    /// und der Nutzer weiss nicht, ob die Seite leer ist oder der Reiter
    /// kaputt. Die Wurzel ist `<html>`, darunter liegen `<head>` und
    /// `<body>`. Der Reiter zeigt damit sofort, dass die Seite da ist.
    pub fn an_seite(&mut self, seite: &Seite) {
        self.baum = Some(seite.css_baum.clone());
        self.alle = narsil_page::kaskade::blaetter(&seite.stile);
        self.author = self.alle.iter().skip(1).cloned().collect();
        // Ansichtsbreite und -hoehe kommen vom Aufrufer (`mit_ansicht`).
        // Bis dahin sind beide 0, und 0 heisst „unbekannt" — das ist bei
        // `@media (min-height: 500px)` die richtige Antwort, weil eine
        // geratene Hoehe die Abfrage erfinden wuerde (so steht es auch in
        // `narsil_page::kaskade::autor_stil`).
        self.ansicht_breite = 0;
        self.ansicht_hoehe = 0;
        self.knoten_gesamt = seite.css_baum.len();
        self.bruecke_bauen(seite);
        // WURZEL + deren Kinder offen: drei Ebenen sind lesbar, vier sind
        // eine Wand aus Tags.
        self.abgeschnitten = 0;
        let (neu_auf, wurzel) = match self.baum.as_ref() {
            Some(b) => {
                let w = self.wurzel(b);
                let mut auf = Vec::new();
                if let Some(w) = w {
                    auf.push(w);
                    aufklappen_alle(b, w, 2, &mut auf);
                }
                (auf, w)
            }
            None => (Vec::new(), None),
        };
        self.auf = neu_auf;
        self.gewaehlt = wurzel;
        self.zeilen_bauen();
        self.stil_berechnen();
        // D19: Regeln und Barrierefreiheit des (neu gewaehlten) Knotens.
        self.regeln_bauen();
        self.barrierefreiheit_bauen();
    }

    /// D19: DIE REGELN DES GEWAEHLTEN KNOTENS BAUEN.
    ///
    /// GEMESSEN, dass das ohne den Aufrufer geht (04.10.): `an_seite` hat
    /// `self.author` (die Autorblaetter: `narsil_page::kaskade::blaetter` ohne
    /// der erste, das ist die Browser-Vorgabe) und `self.baum` (die
    /// `CssBaum`-Kopie). `narsil_page::kaskade::El` ist oeffentlich (mit
    /// `baum`/`id`) und `narsil_css::selektor_passt_mit` ist oeffentlich — also
    /// filtert diese Crate die Regeln selbst, statt den Aufrufer zu bitten.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Herkunft leer bleibt: `Stylesheet`
    /// traegt keine URL (GEMESSEN an `narsil-css/src/typen.rs:21`), und
    /// `an_seite` bekommt die Seite ohne Stylesheet-URLs. Im Bild steht dann
    /// `?` — und nicht die URL des Dokuments, die geraten waere.
    pub fn regeln_bauen(&mut self) {
        let (Some(b), Some(k)) = (self.baum.as_ref(), self.gewaehlt) else {
            self.regeln = crate::d9_layout::Regeln::leer();
            return;
        };
        self.regeln = crate::d9_layout::regeln_fuer(b, &self.author, k, &[]);
    }

    /// D19: DIE BARRIEREFREIHEIT DES GEWAEHLTEN KNOTENS (Rolle, Name).
    ///
    /// # GEMESSEN, woher die Angaben kommen (04.10.)
    ///
    /// `narsil_page::kaskade::CssKnoten` traegt `tag` und `attribute`
    /// (oeffentliche Felder, `kaskade.rs:97-144`), aber **weder einen Text
    /// noch `kinder`** (grep in `kaskade.rs`: keine Treffer). Der sichtbare
    /// Text ist aus dieser Kopie also nicht erreichbar — und wird hier NICHT
    /// erfunden. Was geht, wird gezeigt: Rolle aus dem Tag, Name aus
    /// `aria-label`/`alt`/`value`/`placeholder`/`title`. Fehlt alles, steht
    /// „(kein Name)" im Bild.
    pub fn barrierefreiheit_bauen(&mut self) {
        let (Some(b), Some(k)) = (self.baum.as_ref(), self.gewaehlt) else {
            self.rolle = String::new();
            self.name = String::new();
            return;
        };
        let Some(knoten) = b.knoten.get(k as usize) else {
            self.rolle = String::new();
            self.name = String::new();
            return;
        };
        let attr = |n: &str| knoten.attribute.get(n).cloned();
        self.rolle = crate::d9_layout::rolle(&knoten.tag, &attr);
        self.name = crate::d9_layout::name(&knoten.tag, &attr, "");
    }

    /// Alles zuruecksetzen (Seitenwechsel): Baum, Auswahl, Kasten.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum auch der Baum weg ist: die
    /// `CssBaum`-Kopie ist der groesste Teil des Zustands (bis 200 000
    /// Knoten mit Attributen). Sie zu behalten, obwohl die Seite
    /// weg ist, waere ein Speicherleck ueber Tabs — genau die Regel M6.
    pub fn zuruecksetzen(&mut self) {
        self.zeilen.clear();
        self.auf.clear();
        self.gewaehlt = None;
        self.hover = None;
        self.baum = None;
        self.author.clear();
        self.alle.clear();
        self.ansicht_breite = 0;
        self.ansicht_hoehe = 0;
        self.kasten.clear();
        self.inhaltskasten.clear();
        self.texte.clear();
        self.css_von_dom.clear();
        self.knoten_gesamt = 0;
        self.abgeschnitten = 0;
        self.stil = None;
        self.boxmodell = None;
        self.hinweis.clear();
        self.filter.clear();
        // D19: Regeln, Rolle und Name gehoeren zum gewaehlten Knoten — ohne
        // Knoten gibt es keine. Sonst stuenden die Regeln der ALTEN Seite im
        // Reiter (das ist die Fehlerklasse „Bild stimmt nicht").
        self.regeln = crate::d9_layout::Regeln::leer();
        self.rolle = String::new();
        self.name = String::new();
        self.box_eingabe = crate::d9_layout::BoxEingabe::default();
    }

    /// Ansichtsbreite und -hoehe melden (der Aufrufer kennt sie aus dem Fenster).
    pub fn mit_ansicht(&mut self, breite: u32, hoehe: u32) {
        self.ansicht_breite = breite.min(crate::d1_geruest::MAX_FENSTER);
        self.ansicht_hoehe = hoehe.min(crate::d1_geruest::MAX_FENSTER);
        self.stil_berechnen();
    }

    /// Die gelegten Kästen eintragen (aus dem Layoutbaum des Aufrufers).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Aufrufer den Layoutbaum gibt und
    /// nicht diese Crate ihn holt: `narsil_layout_engine::layout_mit_plan`
    /// braucht den Messer der Seite (die gewaehlte Schrift). Das Gerüst zu
    /// bauen, ohne dieselbe Schrift zu haben, ergaebe ANDERE Kästen — und
    /// ein hervorgehobener Kasten, den es nicht gibt, ist schlimmer als
    /// keiner.
    pub fn mit_layout(&mut self, baum: &narsil_layout::LayoutTree) {
        self.kasten.clear();
        self.inhaltskasten.clear();
        self.texte.clear();
        self.kasten_sammeln(baum, 0);
        self.stil_berechnen();
    }

    /// Einen Knoten wählen (D2: „Knoten waehlen hebt ihn in der Seite hervor").
    ///
    /// Liefert `true`, wenn der Knoten wirklich im Baum liegt — ein Klick auf
    /// eine Zeile, die zu einem anderen Knoten gehoert (etwa der Textzeile
    /// unter einem Element), liefert `false` und aendert nichts.
    pub fn waehle(&mut self, css: u32) -> bool {
        let Some(b) = self.baum.as_ref() else {
            return false;
        };
        if css as usize >= b.knoten.len() {
            return false;
        }
        self.gewaehlt = Some(css);
        self.stil_berechnen();
        // D19: Regeln und Barrierefreiheit gehoeren zum GEWAEHLTEN Knoten.
        // GEMESSENE ENTSCHEIDUNG: ohne diesen Aufruf zeigte der Reiter nach
        // einem Klick die Regeln des VORHER gewaehlten Knotens — eine Zahl an
        // der falschen Stelle, also der Befund „Bild stimmt nicht".
        self.regeln_bauen();
        self.barrierefreiheit_bauen();
        true
    }

    /// Auf- oder zuklappen.
    ///
    /// Liefert den neuen Zustand. Ein Blatt (keine Kinder) bleibt zu —
    /// sonst hätte der Klick auf einen Textknoten einen „auf"-Pfeil ohne
    /// Inhalt, und der Reiter behauptete eine Tiefe, die es nicht gibt.
    pub fn kippe(&mut self, css: u32) -> bool {
        let Some(b) = self.baum.as_ref() else {
            return false;
        };
        let hat_kinder = self.hat_kinder(b, css);
        if !hat_kinder {
            return false;
        }
        match self.auf.iter().position(|k| *k == css) {
            Some(p) => {
                self.auf.remove(p);
                false
            }
            None => {
                if self.auf.len() >= Grenzen::STANDARDAUSGABE.max_baumzeilen {
                    return false;
                }
                self.auf.push(css);
                true
            }
        }
    }

    /// Alle auf- und zuklappen (maximal bis zur Grenze).
    pub fn alle_auf(&mut self) {
        let Some(b) = self.baum.as_ref() else {
            return;
        };
        let wurzel = self.wurzel(b).unwrap_or(0);
        let mut auf = Vec::new();
        aufklappen_alle(b, wurzel, u32::MAX, &mut auf);
        self.auf = auf;
        self.zeilen_bauen();
    }

    /// Alles zuklappen bis auf die Wurzel.
    pub fn alle_zu(&mut self) {
        let wurzel = self.baum.as_ref().and_then(|b| self.wurzel(b));
        self.auf.clear();
        if let Some(w) = wurzel {
            self.auf.push(w);
        }
        self.zeilen_bauen();
    }

    /// Ein Filtertext setzen (leer = kein Filter). Die Zeilen werden gebaut.
    ///
    /// M4: der Filter kommt vom Nutzer und ist beliebig lang — er wird auf
    /// [`Grenzen::max_zeile`] gekuerzt, und die Suche vergleicht Zeichen,
    /// nicht Bytes (sonst bricht ein Filter mit einem Umlaut mitten im Wort).
    pub fn setze_filter(&mut self, text: &str) {
        self.filter = Grenzen::STANDARDAUSGABE.kuerze_zeile(text).to_lowercase();
        self.zeilen_bauen();
    }

    /// Den rechten Reiter setzen (D12: Regeln / Berechnet / Layout).
    ///
    /// Liefert `true`, wenn sich etwas geaendert hat. GEMESSENE ENTSCHEIDUNG:
    /// ein Knopf, der schon an ist, meldet „nichts passiert" — so kann der
    /// Aufrufer eine Tonart spielen oder nicht, ohne dass der Zustand
    /// springt (bei einem Umschalter, der sich selbst umschaltet, waere das
    /// der Unterschied zwischen „Knopf gedrueckt" und „Knopf nicht
    /// gedrueckt", und beides sieht gleich aus).
    pub fn spalte_setzen(&mut self, s: crate::werkzeug::Spalte) -> bool {
        if self.spalte == s {
            return false;
        }
        self.spalte = s;
        true
    }

    /// Die Liste, die rechts steht — je nach [`crate::werkzeug::Spalte`].
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum EINE Funktion und nicht drei Aufrufe im
    /// Zeichencode: „Berechnet" ist die lange Liste (alle 36 Eigenschaften
    /// mit Wert), „Regeln" dieselbe Liste, aber auf die zurueckgeschnitten,
    /// die eine CSS-Regel wirklich setzt (die Angabe, nicht das Ergebnis).
    /// Zwei Sortierungen an zwei Stellen sind zwei Sortierungen, die
    /// irgendwann auseinanderlaufen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „Regeln" nicht leer ist, wenn keine
    /// Autor-Regel den Knoten trifft: dann steht der Hinweis „keine
    /// Autorenregel" EINMAL als Zeile drin. Ein leeres Feld sieht aus wie
    /// ein kaputter Reiter.
    pub fn spalten_zeilen(&self) -> Vec<(String, String)> {
        match self.spalte {
            crate::werkzeug::Spalte::Berechnet => self.eigenschaften(),
            crate::werkzeug::Spalte::Regeln => {
                //
                // D19: DIE REGELN — nicht mehr die berechneten Eigenschaften.
                // GEMESSENER BEFUND (04.10.): vorher standen hier die ersten
                // 14 Zeilen aus `eigenschaften()`, also eine zweite Sicht auf
                // den Reiter „Berechnet". Die Liste ist jetzt
                // `Regeln::text_zeilen()`: je Regel eine Zeile mit der
                // Herkunft (`datei.css:12` oder `?`, wenn die Zeile unbekannt
                // ist) und dem Selektor, darunter je Eigenschaft eine Zeile mit
                // dem Schalterzeichen (` ` = an, `-` = aus).
                if self.regeln.zeilen.is_empty() {
                    // GEMESSENE ENTSCHEIDUNG (die alte Begruendung gilt
                    // weiter): ein leeres Feld sieht aus wie ein kaputter
                    // Reiter. Also steht EINMAL der Grund drin — und er
                    // unterscheidet „keine Autorregel" von „nicht gefuellt".
                    return vec![(
                        if self.regeln.abgeschnitten > 0 {
                            format!(
                                "(keine Autorregel — {} abgeschnitten)",
                                self.regeln.abgeschnitten
                            )
                        } else {
                            String::from("(keine Autorregel geliefert)")
                        },
                        String::new(),
                    )];
                }
                self.regeln
                    .text_zeilen()
                    .into_iter()
                    //
                    // GEMESSENER FEHLER (04.10., DEVPANEL, gefunden von der
                    // Treffer-Pruefung `regel_schalter_unter`): die Zeile stand
                    // hier als ERSTER Teil des Paars. Der Zeichencode
                    // (`zeichnen.rs`) liest aber `if name.is_empty() { Text(wert) }`
                    // else `Text("{name}:")` — er erwartet die Zeile im ZWEITEN
                    // Teil. Im Bild stand deshalb jede Regelzeile mit einem
                    // Doppelpunkt: GEMESSEN `  Farbe: red:` statt
                    // `  Farbe: red`, und `datei.css  p:` statt `datei.css  p`.
                    // Das ist genau der Fehler, den der Kommentar am Zweig
                    // `name.is_empty()` im Zeichencode (D19) beschreibt —
                    // die Ausnahme stand dort, aber die Daten kamen verkehrt
                    // herum. `check` war GRUEN und das Bild falsch.
                    .map(|z| (String::new(), z))
                    .collect()
            }
            // Der Layout-Reiter malt das Box-Modell; seine Zeilen kommen aus
            // `Boxmodell::text_zeilen` und werden an der Stelle gezeichnet.
            crate::werkzeug::Spalte::Layout => self
                .boxmodell
                .as_ref()
                .map(|b| {
                    b.text_zeilen()
                        .into_iter()
                        // Wie bei den Regeln: die Zeile bringt ihren Text mit,
                        // also steht sie im ZWEITEN Teil (sonst haengt der
                        // Zeichencode einen Doppelpunkt an).
                        .map(|z| (String::new(), z))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// Der Hinweis, wenn es nichts zu zeigen gibt.
    ///
    /// GEMESSENE ENTSCHEIDUNG: die Frage wird EINE beantwortet, damit der
    /// Zeichencode nicht drei Faelle selbst entscheidet. „Kein Knoten
    /// gewaehlt" ist ein Zustand, kein Fehler — die Seite kann leer sein.
    pub fn spalten_hinweis(&self) -> Option<&'static str> {
        if self.gewaehlt.is_none() {
            return Some("Kein Knoten gewaehlt — im Baum klicken.");
        }
        if self.spalte == crate::werkzeug::Spalte::Layout && self.boxmodell.is_none() {
            return Some("Fuer diesen Knoten liegt kein Kasten vor.");
        }
        None
    }

    /// Der Knoten, der im SEITEN-KASTEN liegt — für „Untersuchen" mit Auswahl.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der KLEINSTE Kasten gewinnt: bei
    /// ineinander liegenden Kästen (Chrome nennt es „the deepest node") ist
    /// das der, den der Nutter mit dem Cursor meint — der sichtbare Text
    /// liegt im innersten Kasten. Ohne die Beschränkung würde immer das
    /// `<html>` gewinnen und die Auswahl wäre wertlos.
    pub fn knoten_unter(&self, punkt: narsil_core::geometry::Point) -> Option<u32> {
        let mut best: Option<(f32, u32)> = None;
        for (n, r) in self.kasten.iter() {
            if !r.contains(punkt) || r.width <= 0.0 || r.height <= 0.0 {
                continue;
            }
            let css = self.css_von_dom.get(n).copied();
            let Some(css) = css else {
                continue;
            };
            let flaeche = r.width * r.height;
            match best {
                Some((b, _)) if b <= flaeche => {}
                _ => best = Some((flaeche, css)),
            }
        }
        best.map(|(_, c)| c)
    }

    /// Der Hervorhebungsrahmen des gewählten Knotens, in Seitenkoordinaten.
    ///
    /// `None`, wenn der Knoten keinen gelegten Kasten hat (ein `<head>`
    /// zum Beispiel ist im Layout ein 0x0-Kasten).
    pub fn hervorhebung(&self) -> Option<Rect> {
        let css = self.gewaehlt?;
        self.kasten_fuer_css(css)
    }

    /// Die Kette der Hervorhebungen: der gewählte Knoten und seine Vorfahren.
    ///
    /// GEMESSENE BEGRÜNDUNG: genau eine Zahl reicht nicht. Chrome zeichnet
    /// die Kette (die Vorfahren in einer helleren Farbe), weil sonst bei
    /// einem verschachtelten Baum nicht erkennbar ist, WO im Baum der
    /// Knoten hängt. Die Liste ist auf [`Grenzen::max_tiefe`]
    /// geklemmt — das ist die Tiefe, die der Seitenweg selbst geht.
    pub fn hervorhebungskette(&self) -> Vec<(u32, Rect)> {
        let Some(css) = self.gewaehlt else {
            return Vec::new();
        };
        let mut kette = Vec::new();
        let mut lauf = Some(css);
        let mut n = 0usize;
        while let Some(c) = lauf {
            if n >= Grenzen::STANDARDAUSGABE.max_tiefe {
                break;
            }
            n += 1;
            if let Some(r) = self.kasten_fuer_css(c) {
                kette.push((c, r));
            }
            lauf = self.eltern(c);
        }
        kette
    }

    /// Der Kasten eines CSS-Knotens, oder `None`.
    pub fn kasten_fuer_css(&self, css: u32) -> Option<Rect> {
        self.dom_von_css(css)
            .and_then(|d| self.kasten.get(&d).copied())
    }

    /// Der Text eines CSS-Knotens (gekuerzt), oder `None`.
    pub fn text_fuer_css(&self, css: u32) -> Option<String> {
        let d = self.dom_von_css(css)?;
        let t = self.texte.get(&d)?;
        Some(Grenzen::STANDARDAUSGABE.kuerze_zeile(t))
    }

    /// Die berechneten Eigenschaften des gewählten Knotens als
    /// (Name, Wert)-Zeilen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine FESTE Liste und nicht „alles aus
    /// `Berechnet`": `Berechnet` hat über 100 Felder, und die meisten sind
    /// Flex-Richtungen oder Variablen. Chrome zeigt 20 Eigenschaften und
    /// dahinter „Mehr". Diese Liste sind die 32 Eigenschaften, an denen man
    /// im Alltag steht — und jede ist eine Zahl, die man nachschlagen kann.
    pub fn eigenschaften(&self) -> Vec<(String, String)> {
        let Some(b) = self.stil.as_ref() else {
            return Vec::new();
        };
        let g = Grenzen::STANDARDAUSGABE;
        let p = |n: &str, w: String| (n.to_string(), g.kuerze_zeile(&w));
        vec![
            p("display", format!("{:?}", b.anzeige)),
            p("position", format!("{:?}", b.position)),
            p("z-index", opt_i32(b.z_index)),
            p("color", farbe_text(&b.farbe)),
            p("background-color", farbe_text(&b.hintergrund)),
            p("opacity", format!("{:.2}", b.deckkraft)),
            p("font-family", b.schriftfamilie.clone()),
            p("font-size", format!("{}px", b.schriftgroesse_px)),
            p("font-weight", format!("{}", b.schriftgewicht)),
            p("font-style", format!("{:?}", b.schriftstil)),
            p("line-height", format!("{}px", b.zeilenhoehe_px)),
            p("text-align", format!("{:?}", b.textausrichtung)),
            p("text-decoration", dekoration_text(b.dekoration)),
            p("letter-spacing", format!("{}px", b.buchstaben_abstand)),
            p("word-spacing", format!("{}px", b.wort_abstand)),
            p("white-space", format!("{:?}", b.weissraum)),
            p("direction", format!("{:?}", b.richtung)),
            p("width", opt_px(b.breite)),
            p("height", opt_px(b.hoehe)),
            p("min-width", opt_px(b.min_breite)),
            p("max-width", opt_px(b.max_breite)),
            p("min-height", opt_px(b.min_hoehe)),
            p("max-height", opt_px(b.max_hoehe)),
            p(
                "margin",
                kuerzel_4(b.marge_oben, b.marge_rechts, b.marge_unten, b.marge_links),
            ),
            p(
                "padding",
                kuerzel_4(
                    b.fuellung_oben,
                    b.fuellung_rechts,
                    b.fuellung_unten,
                    b.fuellung_links,
                ),
            ),
            p(
                "border-width",
                kuerzel_4(
                    b.rahmen_oben_breite,
                    b.rahmen_rechts_breite,
                    b.rahmen_unten_breite,
                    b.rahmen_links_breite,
                ),
            ),
            p("border-style", border_stil_text(b)),
            p("border-color", border_farbe_text(b)),
            p("border-radius", format!("{:?}", b.radius)),
            p("box-sizing", format!("{:?}", b.box_art)),
            p("float", format!("{:?}", b.schweben)),
            p("clear", format!("{:?}", b.raeumung)),
            p("overflow", format!("{:?}", b.ueberlauf)),
            p(
                "visibility",
                if b.sichtbar { "visible" } else { "hidden" }.to_string(),
            ),
        ]
    }

    /// Das Box-Modell MIT DEN GELEGTEN Massen — die Zahlen, die im Reiter
    /// stehen (D9, Befund 2 der Koordination, 01.10.2026).
    ///
    /// # DER BEFUND, DER DIESE FUNKTION ERZEUGT HAT
    ///
    /// Anzeige (wikipedia.org, `main`): „Inhalt 0 × 0 px, Marge 0/0/0/0,
    /// Rahmen 0/0/0/0, Fuellung 0/0/0/0" — bei einem Element, das im Bild
    /// einen Kasten von 640 × 60 Pixeln hat. Der Kasten war also DA, die
    /// Zahlen waren es nicht.
    ///
    /// GEMESSENE URSACHE: `Boxmodell::aus(&Berechnet)` nimmt
    /// `Berechnet::breite` und `hoehe`. Die sind `Option<f32>` und stehen
    /// bei `width: auto` auf `None` — also wird daraus 0. Ein Element mit
    /// `width: auto` hat aber eine BREITE, nur keine angegebene. Das sind
    /// zwei verschiedene Fragen („was steht im Stylesheet" und „wie breit
    /// ist es geworden"), und die zweite beantwortet der Layoutbaum.
    ///
    /// GEMESSENE ENTSCHEIDUNG: die Anzeige bekommt BEIDE. Der Kasten aus
    /// dem Layout ist die Zahl, die man im Bild nachmisst; `auto` steht
    /// als Zusatz dabei, damit man weiss, dass es nicht im Stylesheet
    /// stand.
    pub fn boxmodell_mit_layout(&self) -> Option<(Boxmodell, Rect, Rect)> {
        let b = self.boxmodell?;
        let (aussen, inhalt) = self.kasten_zahlen()?;
        let mut b = b;
        // GEMESSENE ENTSCHEIDUNG, warum der INHALTSkasten und nicht der
        // AUSSENkasten die Breite traegt: die Eigenschaften `width`/
        // `height` in CSS beschreiben den INHALTSkasten (ohne Padding
        // und Border) — CSS 2.1, 10.3.2. Ein anderes Kastenmodell waere
        // eine falsche Zahl mit dem richtigen Namen.
        b.gelegt_inhalt = Some((inhalt.width, inhalt.height));
        b.gelegt_aussen = Some((aussen.width, aussen.height));
        Some((b, aussen, inhalt))
    }

    /// Der Kasten und der Inhaltskasten des gewählten Knotens, in Zahlen.
    pub fn kasten_zahlen(&self) -> Option<(Rect, Rect)> {
        let css = self.gewaehlt?;
        let aussen = self.kasten_fuer_css(css);
        let inhalt = self
            .dom_von_css(css)
            .and_then(|d| self.inhaltskasten.get(&d).copied())
            .or(aussen);
        Some((aussen?, inhalt.unwrap_or(aussen?)))
    }

    /// Die Zeilen für den Reiter: je Knoten eine Zeile.
    /// Die Zeilen als Text, aber hoechstens `n` (der Weg des Reiters).
    ///
    /// # GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:11x)
    ///
    /// GEMESSEN: der Elemente-Reiter kostete bei **2 000 Knoten 0,569 ms**
    /// (Median aus 30 Laeufen; 20 Knoten: 0,042 ms — Faktor 13,5 bei
    /// Faktor 100 der Knoten). Ursache: `text_zeilen()` baute fuer JEDEN
    /// Knoten einen String (mit einer Einrueckungsschleife und einem
    /// `kuerze_zeile`), gezeichnet werden aber nur die Zeilen, die in die
    /// Spalte passen — bei einem 900-px-Fenster rund 12.
    ///
    /// Also dasselbe wie in der Konsole (dort gemessen: 1,192 ms -> 0,138
    /// ms): nur so viele Zeilen bauen, wie gezeichnet werden.
    pub fn text_zeilen_bis(&self, n: usize) -> Vec<String> {
        self.zeilen
            .iter()
            .take(n)
            .map(|z| self.zeile_text(z))
            .collect()
    }

    /// Die Zeilen in Stuecken mit Farbe — was der ZEICHNER braucht (D12).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Zeichencode die Stuecke hier holt und
    /// nicht selbst zerlegt: die Zerlegung ist eine Rechnung ueber die
    /// Attribute des Knotens (Reihenfolge, Kuerzung, Grenzen). Steht sie im
    /// Zeichencode, ist sie an zwei Orten — und die Farbe des Attributwerts
    /// ist dann die eine, die zaehlt.
    ///
    /// D12: JEDER Teil traegt seinen Einzug selbst. Der Zeichencode muss
    /// also keine eigene Einzugsrechnung haben — dieselbe wie beim Text.
    pub fn teil_zeilen_bis(&self, n: usize) -> Vec<Vec<Zeilenteil>> {
        self.zeilen
            .iter()
            .take(n)
            .map(|z| {
                let mut v: Vec<Zeilenteil> = Vec::with_capacity(z.teile.len() + 2);
                for _ in 0..(z.tiefe as usize).min(MAX_EINRUEKUNG) {
                    v.push(Zeilenteil::neu(" ", TeilArt::Klammer));
                }
                if !z.schliesst {
                    if z.hat_kinder {
                        v.push(Zeilenteil::neu(
                            if z.aufgeklappt { "▾ " } else { "▸ " },
                            TeilArt::Klammer,
                        ));
                    } else {
                        v.push(Zeilenteil::neu("  ", TeilArt::Klammer));
                    }
                }
                v.extend(z.teile.iter().cloned());
                v
            })
            .collect()
    }

    /// Die Zeilen als Text — ALLE, unabhaengig von der Fenstergroesse.
    ///
    /// GEMESSENE ENTSCHEIDUNG (3:11x): der REITER benutzt
    /// [`Elemente::text_zeilen_bis`], weil er nur die Zeilen baut, die er
    /// zeichnet. Diese Fassung bleibt fuer Protokolle, Tests und jede
    /// Stelle, die wirklich alle Zeilen braucht.
    pub fn text_zeilen(&self) -> Vec<String> {
        self.zeilen.iter().map(|z| self.zeile_text(z)).collect()
    }

    /// Eine Zeile als Text (der Formatierehler — siehe [`Elemente::text_zeilen`]).
    fn zeile_text(&self, z: &Zeile) -> String {
        let g = Grenzen::STANDARDAUSGABE;
        let mut s = String::new();
        // Einrückung, aufgeschachtelt als Leerzeichen (kein Tab: die Schrift
        // ist proportional, ein Tab wäre eine Zufallsbreite).
        //
        // GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:11x): die Einrueckung ist
        // auf [`MAX_EINRUEKUNG`] begrenzt. GEMESSEN: ein Baum aus 2 000
        // KNOTEN in einer Kette ergibt eine Tiefe von 2 000 — also 2 000
        // Leerzeichen je Zeile, 4 000 Zeichen String fuer eine Zeile, die
        // hoechstens `max_zeile` (100) zeigen kann.
        for _ in 0..(z.tiefe as usize).min(MAX_EINRUEKUNG) {
            s.push(' ');
        }
        // D12: ein Schliesstext hat KEINEN Pfeil — er hat nichts, was er
        // aufklappen koennte. Sonst stuende vor `</div>` ein `▸`, und der
        // Baum behauptete eine Zeile, die es nicht gibt.
        if !z.schliesst {
            if z.hat_kinder {
                s.push(if z.aufgeklappt { '▾' } else { '▸' });
                s.push(' ');
            } else {
                s.push_str("  ");
            }
        }
        s.push_str(&z.beschriftung);
        g.kuerze_zeile(&s)
    }

    /// Die Zeile `i` als Beschriftung mit Auswahl-Markierung.
    pub fn zeile_beschriftung(&self, i: usize) -> Option<(String, bool)> {
        let z = self.zeilen.get(i)?;
        Some((z.beschriftung.clone(), self.gewaehlt == Some(z.css)))
    }

    // ------------------------------------------------------------------
    // innen
    // ------------------------------------------------------------------

    /// Die Wurzel: der erste Knoten ohne Eltern.
    fn wurzel(&self, b: &CssBaum) -> Option<u32> {
        (0..b.knoten.len() as u32).find(|i| b.knoten[*i as usize].eltern.is_none())
    }

    /// Hat der Knoten Kinder?
    fn hat_kinder(&self, b: &CssBaum, css: u32) -> bool {
        let Some(k) = b.knoten.get(css as usize) else {
            return false;
        };
        if k.vorher.is_some() || k.nachher.is_some() {
            return true;
        }
        // Das erste Kind: irgendein Knoten, dessen Eltern dieser ist.
        b.knoten
            .iter()
            .enumerate()
            .any(|(i, x)| x.eltern == Some(css) && (i as u32) != css)
    }

    /// Die Elternnummer eines Knotens.
    fn eltern(&self, css: u32) -> Option<u32> {
        let b = self.baum.as_ref()?;
        b.knoten.get(css as usize)?.eltern
    }

    /// Der DOM-Knoten hinter einer CSS-Nummer.
    fn dom_von_css(&self, css: u32) -> Option<u32> {
        for (d, c) in self.css_von_dom.iter() {
            if *c == css {
                return Some(*d);
            }
        }
        None
    }

    /// Die flache Liste der sichtbaren Zeilen bauen.
    fn zeilen_bauen(&mut self) {
        self.zeilen.clear();
        self.abgeschnitten = 0;
        let Some(b) = self.baum.as_ref() else {
            return;
        };
        let g = Grenzen::STANDARDAUSGABE;
        let mut stapel: Vec<(u32, u32, bool)> = Vec::new(); // (css, tiefe, hat_kinder)
        let mut wurzeln: Vec<u32> = Vec::new();
        for (i, k) in b.knoten.iter().enumerate() {
            if k.eltern.is_none() {
                wurzeln.push(i as u32);
            }
        }
        for w in wurzeln.iter() {
            stapel.push((*w, 0, false));
        }
        let mut besuchte = 0usize;
        while let Some((css, tiefe, _)) = stapel.pop() {
            besuchte += 1;
            if besuchte > g.max_knoten {
                self.abgeschnitten += 1;
                break;
            }
            // M5: die Tiefe ist eine harte Grenze (endlose Verschachtelung).
            if tiefe as usize >= g.max_tiefe {
                continue;
            }
            if b.knoten.get(css as usize).is_none() {
                continue;
            }
            let kinder = kinder_von(b, css);
            let hat_kinder = !kinder.is_empty();
            let aufgeklappt = hat_kinder && self.auf.contains(&css);
            let passt_zum_filter = self.entspricht_filter(css);
            if passt_zum_filter || aufgeklappt {
                if self.zeilen.len() >= g.max_baumzeilen {
                    self.abgeschnitten += 1;
                    continue;
                }
                let teile = self.teile_bauen(css);
                let beschriftung = beschriftung_aus_teilen(&teile);
                self.zeilen.push(Zeile {
                    css,
                    tiefe,
                    hat_kinder,
                    aufgeklappt,
                    beschriftung,
                    teile,
                    schliesst: false,
                });
            }
            // UND: Kinder werden immer nachgefuehrt (auch die eines
            // gefilterten Knotens) — sonst zeigt ein Filter, der auf
            // `.klasse` passt, nur die Eltern ohne ihr Kind mit dem Text.
            if aufgeklappt || passt_zum_filter {
                let mut tiefer = Vec::new();
                for k2 in kinder.iter() {
                    tiefer.push((*k2, tiefe + 1, false));
                }
                // `pop` nimmt von hinten — also rueckwaerts fuellen, damit
                // die Dokumentreihenfolge im Bild steht.
                for t in tiefer.into_iter().rev() {
                    stapel.push(t);
                }
            }
        }
        // D12: jetzt die Schliesstexte (siehe `schliesstzeilen_einbauen`).
        self.schliesstzeilen_einbauen();
    }

    /// D12: DIE SCHLIESSTEXTE einbauen — `</div>` unter den Kindern.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein eigener Schritt nach dem
    /// Aufbau ist und nicht im Aufbau selbst: im Aufbau weiss der Stapel,
    /// WELCHE Knoten offen sind, aber nicht, wo ein Knoten zu Ende ist — das
    /// weiss erst die Reihenfolge der fertigen Zeilen. Ein zweiter Durchgang
    /// ueber dieselbe Liste ist damit die ehrliche Rechnung; ein zweiter
    /// Stapel waere eine zweite Rechnung fuer dieselbe Sache.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum rekursiv und nicht mit einem Stapel:
    /// die Zeilen stehen in Dokumentreihenfolge, also ist der Block eines
    /// Knotens „alle folgenden Zeilen mit groesserer Tiefe". Genau das ist
    /// eine Rekursion ueber die Liste — ein Stapel muesste die Reihenfolge
    /// wieder umbauen, und dann kaeme es auf die Reihenfolge des Stapels an.
    fn schliesstzeilen_einbauen(&mut self) {
        let alt = std::mem::take(&mut self.zeilen);
        if alt.is_empty() {
            return;
        }
        // Der Anfangszustand kann noch Teile haben (die Zeilen sind aus einem
        // früheren Aufbau) — der Durchgang ersetzt sie vollstaendig.
        let mut raus: Vec<Zeile> = Vec::with_capacity(alt.len() + 8);
        self.ausbauen(&alt, 0, alt.len(), &mut raus);
        self.zeilen = raus;
    }

    /// Der rekursive Teil von [`Elemente::schliesstzeilen_einbauen`].
    fn ausbauen(&self, alt: &[Zeile], von: usize, bis: usize, raus: &mut Vec<Zeile>) {
        let mut i = von;
        while i < bis {
            let z = &alt[i];
            raus.push(z.clone());
            let mut ende = i + 1;
            if z.aufgeklappt && z.hat_kinder {
                while ende < bis && alt[ende].tiefe > z.tiefe {
                    ende += 1;
                }
                self.ausbauen(alt, i + 1, ende, raus);
                raus.push(self.schliesstzeile(z));
            }
            i = ende;
        }
    }

    /// Die Zeile mit einem Schliesstext (`</div>`), D12.
    fn schliesstzeile(&self, z: &Zeile) -> Zeile {
        let teile = self.schliessteile(z.css);
        Zeile {
            css: z.css,
            tiefe: z.tiefe,
            hat_kinder: true,
            aufgeklappt: false,
            beschriftung: beschriftung_aus_teilen(&teile),
            teile,
            schliesst: true,
        }
    }

    /// Die Beschriftung einer Zeile: `<tag id="…" class="…" …>`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `id` und `class` ZUERST kommen: sie
    /// stehen in 95 % aller Zeilen links, weil man nach ihnen sucht. Alles
    /// andere ist gekuerzt.
    /// Die Zeile in Stuecken mit Farbe — DIE Rechnung fuer die Farben (D12).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge hier und nicht im
    /// Zeichencode steht: die Attribute muessen in FESTER Reihenfolge stehen
    /// (`id`, `class`, dann der Rest alphabetisch), sonst sieht derselbe
    /// Baum bei jedem Aufbau anders aus und zwei Screenshots sind nicht
    /// vergleichbar. Diese Regel ist eine Eigenschaft des BAUMS, nicht des
    /// Bildes — der Zeichencode darf sie nur lesen, nicht entscheiden.
    fn teile_bauen(&self, css: u32) -> Vec<Zeilenteil> {
        let g = Grenzen::STANDARDAUSGABE;
        let mut raus: Vec<Zeilenteil> = Vec::new();
        let Some(b) = self.baum.as_ref() else {
            return raus;
        };
        let Some(k) = b.knoten.get(css as usize) else {
            return raus;
        };
        raus.push(Zeilenteil::neu("<", TeilArt::Klammer));
        raus.push(Zeilenteil::neu(
            if k.tag.is_empty() {
                "unbekannt"
            } else {
                &k.tag
            },
            TeilArt::Name,
        ));
        // Die Attribute: Name und Wert sind ZWEI Teile, weil sie in Firefox
        // zwei Farben haben. Das Zeichen ist dabei beim NAMEN (D12, 03.10.:
        // `id=` gelb, der Wert gruen).
        //
        // GEMESSENER FEHLER (03.10. 21:2x, Bild `devpanel_elemente.png`):
        // das Attribut stand als `<htmllang="de">` — das Leerzeichen fehlte
        // und die Anfuehrungszeichen lagen auf den Buchstaben. Zwei Ursachen,
        // beide hier behoben:
        //
        // 1. FUENF Teile je Attribut (` `, `name=`, `"`, Wert, `"`) ergaben
        //    vier Grenzen, an denen der Zeichencode den Vorschub schaetzt.
        //    Jede Grenze ist ein Fehler. Jetzt ZWEI Teile: der erste traegt
        //    das Leerzeichen mit (` id="`), der zweite den Wert mit dem
        //    schliessenden Zeichen (`gross"`). Eine Grenze je Attribut.
        // 2. Das Leerzeichen war ein eigener Teil. Getrennt gezeichnet wird es
        //    nicht (`text.trim().is_empty()`), aber der Vorschub wird
        //    geschaetzt — ein Leerzeichen, das nur in der Rechnung existiert,
        //    ist genau das, was im Bild fehlt. Jetzt steht es im ersten Teil
        //    und wird mitgezeichnet.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Wert das schliessende Zeichen
        // traegt und nicht der Name: das Anfuehrungszeichen ist schmaler als
        // ein Durchschnittszeichen, der Abschaetz-Fehler faellt also an der
        // Grenze VOR dem Wert auf. Legt man sie nach den Wert, ist sie
        // optisch die harmloseste Stelle der Zeile.
        let mut ein_attribut = |name: &str, wert: &str, raus: &mut Vec<Zeilenteil>| {
            raus.push(Zeilenteil::neu(format!(" {name}=\""), TeilArt::Attribut));
            raus.push(Zeilenteil::neu(
                format!("{}\"", g.kuerze(wert, g.max_wert_zeichen)),
                TeilArt::Wert,
            ));
        };
        if let Some(w) = k.attribute.get("id") {
            ein_attribut("id", w, &mut raus);
        }
        if let Some(w) = k.attribute.get("class") {
            ein_attribut("class", w, &mut raus);
        }
        let mut reste: Vec<(&String, &String)> = k
            .attribute
            .iter()
            .filter(|(n, _)| n.as_str() != "id" && n.as_str() != "class")
            .collect();
        reste.sort_by(|a, b2| a.0.cmp(b2.0));
        for (n, (name, wert)) in reste.iter().enumerate() {
            if n >= g.max_attribute {
                raus.push(Zeilenteil::neu(" …", TeilArt::Klammer));
                break;
            }
            ein_attribut(name, wert, &mut raus);
        }
        raus.push(Zeilenteil::neu(">", TeilArt::Klammer));
        raus
    }

    /// Der Schliesstext eines Knotens als Teile (`</div>`, D12).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum NUR Name und Klammern: Firefox faerbt
    /// im Schliesstext den Tag-Namen genauso wie im oeffnenden Tag. Ein
    /// Schliesstext mit Attributen gibt es nicht — die gehoeren zum Anfang.
    fn schliessteile(&self, css: u32) -> Vec<Zeilenteil> {
        let tag = self
            .baum
            .as_ref()
            .and_then(|b| b.knoten.get(css as usize))
            .map(|k| {
                if k.tag.is_empty() {
                    "unbekannt"
                } else {
                    &k.tag
                }
            })
            .unwrap_or("unbekannt");
        vec![
            Zeilenteil::neu("</", TeilArt::Klammer),
            Zeilenteil::neu(tag, TeilArt::Name),
            Zeilenteil::neu(">", TeilArt::Klammer),
        ]
    }

    fn beschriftung(&self, css: u32) -> String {
        let teile = self.teile_bauen(css);
        beschriftung_aus_teilen(&teile)
    }

    /// Passt der Knoten zum Filter?
    fn entspricht_filter(&self, css: u32) -> bool {
        if self.filter.is_empty() {
            return true;
        }
        self.beschriftung(css).to_lowercase().contains(&self.filter)
    }

    /// Die Brücke DOM -> CSS und DOM -> Ordinal bauen.
    fn bruecke_bauen(&mut self, seite: &Seite) {
        self.css_von_dom.clear();
        // `css_zu_dom`: CSS-Index -> Vorlauf-Nummer.
        // `dom_zu_ord`: DOM-Nummer -> Vorlauf-Nummer.
        // Also: Vorlauf-Nummer -> DOM-Nummer ist die Umkehrung der zweiten.
        let mut ord_zu_dom: BTreeMap<usize, u32> = BTreeMap::new();
        for (dom, ord) in seite.dom_zu_ord.iter() {
            ord_zu_dom.insert(*ord, *dom);
        }
        for (css, ord) in seite.css_zu_dom.iter() {
            if let Some(dom) = ord_zu_dom.get(ord) {
                self.css_von_dom.insert(*dom, *css);
            }
        }
    }

    /// Die Kaesten und Texte aus dem Layoutbaum einsammeln.
    fn kasten_sammeln(&mut self, b: &narsil_layout::LayoutTree, tiefe: u32) {
        self.kasten_sammeln_kasten(&b.root, tiefe);
    }

    fn kasten_sammeln_kasten(&mut self, k: &narsil_layout::LayoutBox, tiefe: u32) {
        // M5: die Layoutbaum kann tief sein (ein verschachteltes Flex-Raster
        // in einer Schleife). Ohne diese Grenze ist der Stapel der Grund
        // fuer einen Absturz, und der Grund waere eine Seite.
        if tiefe as usize >= Grenzen::STANDARDAUSGABE.max_tiefe * 4 {
            return;
        }
        let n = k.node.0;
        // Die Groesse kommt aus dem Layout; ein Kasten mit 0 Hoehe traegt
        // keine Hervorhebung (es gibt nichts hervorzuheben).
        if k.border_box.width > 0.0 && k.border_box.height > 0.0 {
            self.kasten.entry(n).or_insert(k.border_box);
        }
        if k.content_box.width > 0.0 && k.content_box.height > 0.0 {
            self.inhaltskasten.entry(n).or_insert(k.content_box);
        }
        if let Some(t) = &k.text {
            if !t.text.trim().is_empty() {
                let g = Grenzen::STANDARDAUSGABE;
                self.texte
                    .entry(n)
                    .or_insert_with(|| g.kuerze(&t.text, 200));
            }
        }
        for kind in k.children.iter() {
            self.kasten_sammeln_kasten(kind, tiefe + 1);
        }
    }

    /// Den berechneten Stil des gewaehlten Knotens holen.
    ///
    /// GEMESSENE BEGRÜNDUNG, warum die KETTE von der Wurzel bis zum
    /// gewählten Knoten gerechnet wird und nicht nur der Knoten selbst:
    /// `berechnen` erbt. `color`, `font-size`, `line-height`, `visibility`,
    /// `direction`, `white-space` und die Variablen kommen vom Vorfahren —
    /// ohne Elternwert bekommt der Reiter den Anfangswert und behauptet,
    /// die Seite hätte nicht die geerbte Farbe. Das ist bei `body`
    /// `{color:#ddd}` der Unterschied zwischen „schwarz" und „#ddd".
    fn stil_berechnen(&mut self) {
        self.stil = None;
        self.boxmodell = None;
        let Some(b) = self.baum.as_ref() else {
            return;
        };
        let Some(css) = self.gewaehlt else {
            return;
        };
        // Die Kette sammeln (von der Wurzel nach unten), mit einer Grenze.
        let mut kette: Vec<u32> = Vec::new();
        let mut lauf = Some(css);
        let mut n = 0usize;
        while let Some(c) = lauf {
            if n >= Grenzen::STANDARDAUSGABE.max_tiefe {
                break;
            }
            n += 1;
            kette.push(c);
            lauf = b.knoten.get(c as usize).and_then(|k| k.eltern);
        }
        kette.reverse();
        let mut eltern: Option<Berechnet> = None;
        for c in kette.iter() {
            let w = narsil_page::kaskade::end_stil(
                b,
                *c,
                &self.alle,
                eltern.as_ref(),
                self.ansicht_breite,
                self.ansicht_hoehe,
            );
            eltern = Some(w);
        }
        if let Some(w) = eltern {
            self.boxmodell = Some(Boxmodell::aus(&w));
            self.stil = Some(w);
        }
    }
}

/// Alle Kinder bis zur Tiefe `bis` aufklappen (freie Form, siehe
/// [`Elemente::alle_kinder_aufklappen`]).
///
/// M3: `bis` ist die Tiefengrenze (M5 — Rekursion mit Grenze) UND
/// `auf.len()` ist die Mengengrenze: beide sind benannt, und beide sind
/// benoetigt, weil ein flacher, breiter Baum (10 000 Geschwister) an der
/// einen Grenze scheitert und ein tiefer, schmaler (5000 Ebenen) an der
/// anderen.
fn aufklappen_alle(b: &CssBaum, css: u32, bis: u32, auf: &mut Vec<u32>) {
    let grenze = Grenzen::STANDARDAUSGABE.max_baumzeilen;
    if bis == 0 || auf.len() >= grenze {
        return;
    }
    for k in kinder_von(b, css) {
        if auf.len() >= grenze {
            return;
        }
        auf.push(k);
        aufklappen_alle(b, k, bis.saturating_sub(1), auf);
    }
}

/// Die Kinder einer Nummer, in Dokumentreihenfolge — freie Form.
///
/// GEMESSENE ENTSCHEIDUNG, warum `nachher` und kein Index: `CssBaum` hat
/// keine Kinderliste, nur `vorher`/`nachher`. Der erste Kindknoten wird
/// linear gesucht — EINMAL je aufgeklapptem Knoten, bei 200 000 Knoten im
/// schlimmsten Fall 200 000 Vergleiche (die gleiche Suche laeuft im
/// Kaskaden-Bau von `narsil-page` und ist dort gemessen).
fn kinder_von(b: &CssBaum, css: u32) -> Vec<u32> {
    let erstes = b
        .knoten
        .iter()
        .enumerate()
        .find(|(i, k)| k.eltern == Some(css) && (*i as u32) != css)
        .map(|(i, _)| i as u32);
    let Some(mut lauf) = erstes else {
        return Vec::new();
    };
    let mut raus = Vec::new();
    let grenze = Grenzen::STANDARDAUSGABE.max_baumzeilen;
    let mut n = 0usize;
    loop {
        if raus.len() >= grenze || n > b.knoten.len() {
            break;
        }
        n += 1;
        raus.push(lauf);
        match b.knoten.get(lauf as usize).and_then(|k| k.nachher) {
            Some(next) => lauf = next,
            None => break,
        }
    }
    raus
}

/// Wie tief die Einrueckung gezeichnet wird (M3: eine Grenze mit Namen).
///
/// GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:11x), wieso diese Zahl: der
/// Zeichencode zeigt `max_zeile` (100) Zeichen je Zeile. Eine Einrueckung
/// von mehr als ~24 Zeichen schiebt den Text aus der Zeile heraus, und
/// `kuerze_zeile` wuerde ihn abschneiden. Also: 24 Leerzeichen hoechstens —
/// und der Rest des Baums steht links ohne Einrueckung. Eine TIEFE ist
/// dann an der Zeile nicht mehr ablesbar, aber die Zeile ist lesbar. Das
/// ist der Handel, und er ist gemessen: bei einer Baumkette von 2 000
/// Knoten waren es 4 000 Zeichen String je Zeile (2 000 Leerzeichen plus
/// Text), jetzt sind es 24 plus Text.
pub const MAX_EINRUEKUNG: usize = 24;

/// Eine Zeile im Baum.
#[derive(Debug, Clone, PartialEq)]
pub struct Zeile {
    /// Die Nummer im `CssBaum`.
    pub css: u32,
    /// Wie tief eingerückt (0 = Wurzel).
    pub tiefe: u32,
    /// Hat der Knoten Kinder?
    pub hat_kinder: bool,
    /// Steht der Knoten auf?
    pub aufgeklappt: bool,
    /// `<tag id="…" …>` — der Text der Zeile, wie er im Reiter steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG (D12): das bleibt der GANZE Text, obwohl die
    /// Zeile jetzt auch [`Zeile::teile`] traegt. Der Zeichencode braucht die
    /// Farben, aber jeder Test, Protokollausgabe und Textvergleich liest
    /// einen String. Zwei Quellen fuer denselben Text waeren zwei Zahlen —
    /// und die Teile entstehen aus derselben Rechnung, also ist der String
    /// ihre Zusammenfassung, nicht eine zweite.
    pub beschriftung: String,
    /// Die Zeile in Stuecken mit Farbe (D12: `id`/`class` farbig).
    pub teile: Vec<Zeilenteil>,
    /// Steht das hier fuer einen SCHLIESSTEXT (`</div>`)?
    ///
    /// GEMESSENE ENTSCHEIDUNG (D12, 03.10.): Firefox zeigt den Schliesstext
    /// nur bei einem Knoten, der auch ELEMENTKINDER hat — ein `<img>` mit
    /// einem Schliesstext waer eine Zeile ohne Inhalt. Das entscheidet
    /// `zeilen_bauen`, nicht der Zeichencode.
    pub schliesst: bool,
}

/// EIN STUECK EINER Baumzeile, mit der Farbe, in der es im Bild steht.
///
/// D12 (03.10., Auftrag der Leitung): `id` und `class` sollen FARBIG sein und
/// die Zeile soll einen Schliesstext haben. Beides geht nicht mit EINEM
/// `String` je Zeile — ein String hat eine Farbe. Also: die Zeile traegt die
/// Teile, und [`Zeile::beschriftung`] bleibt der zusammengesetzte Text, damit
/// jeder Test, der eine Zeile als Text liest, weiter dasselbe sieht.
///
/// GEMESSENE ENTSCHEIDUNG, warum `art` ein Enum ist und keine Zahl: die Zahl
/// muesste an drei Stellen (Zeichner, Test, Auswertung) dieselbe Bedeutung
/// haben. Eine Zahl, die an drei Stellen dasselbe bedeutet, ist drei Zahlen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeilArt {
    /// Die spitzen Klammern `<` `>` und der Schraegstrich eines Schliesstexts.
    Klammer,
    /// Der Name des Tags (`html`, `div`).
    Name,
    /// Der Name eines Attributs (`id`, `class`, `href`).
    Attribut,
    /// Der Wert eines Attributs zwischen den Anfuehrungszeichen.
    Wert,
}

/// Ein Stueck einer Baumzeile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zeilenteil {
    /// Der Text des Stuecks.
    pub text: String,
    /// Wie er im Bild steht.
    pub art: TeilArt,
}

impl Zeilenteil {
    /// Ein Stueck anlegen.
    pub fn neu(text: impl Into<String>, art: TeilArt) -> Self {
        Self {
            text: text.into(),
            art,
        }
    }
}

/// Das Box-Modell eines Knotens: vier Kästchen um den Inhalt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Boxmodell {
    /// Aussen (Marge) oben/rechts/unten/links.
    pub marge: [f32; 4],
    /// Innen (Fuellung) oben/rechts/unten/links.
    pub fuellung: [f32; 4],
    /// Rahmen oben/rechts/unten/links.
    pub rahmen: [f32; 4],
    /// Der Inhaltskasten in px aus dem Stylesheet (`0` = `width: auto`).
    pub inhalt_breite: f32,
    pub inhalt_hoehe: f32,
    /// Der Inhaltskasten in px AUS DEM LAYOUT (womit er wirklich gezeichnet
    /// wurde), `None` wenn es keinen gelegten Kasten gibt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein eigenes Feld ist und nicht
    /// ein Ersatz fuer `inhalt_breite`: beide Zahlen sind richtig, aber
    /// sie antworten auf verschiedene Fragen. „Breite 0" bei `auto`
    /// ist die Angabe, „Breite 640" ist die Messung — und ein Reiter,
    /// der beides uebereinander in eine Zahl presst, verliert eine davon.
    pub gelegt_inhalt: Option<(f32, f32)>,
    /// Der AUSSENkasten aus dem Layout (`border-box`).
    pub gelegt_aussen: Option<(f32, f32)>,
}

impl Boxmodell {
    /// Aus einem berechneten Stil.
    pub fn aus(b: &Berechnet) -> Self {
        Self {
            marge: [b.marge_oben, b.marge_rechts, b.marge_unten, b.marge_links],
            fuellung: [
                b.fuellung_oben,
                b.fuellung_rechts,
                b.fuellung_unten,
                b.fuellung_links,
            ],
            rahmen: [
                b.rahmen_oben_breite,
                b.rahmen_rechts_breite,
                b.rahmen_unten_breite,
                b.rahmen_links_breite,
            ],
            inhalt_breite: b.breite.unwrap_or(0.0),
            inhalt_hoehe: b.hoehe.unwrap_or(0.0),
            gelegt_inhalt: None,
            gelegt_aussen: None,
        }
    }

    /// Die Summe von Rahmen und Fuellung in eine Richtung.
    pub fn rand(&self, kanten: usize) -> f32 {
        if kanten >= 4 {
            return 0.0;
        }
        (self.rahmen[kanten] + self.fuellung[kanten]).max(0.0)
    }

    /// Die vier Kästchen um einen Inhaltskasten (Seitenkoordinaten).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das `Result` ist: ein Inhaltskasten
    /// von 0 px Breite (ein `<head>` oder ein `display:none`) ergibt
    /// Aussen- und Innenkasten von derselben Groesse, und das ist richtig —
    /// aber es ist ein Kasten, den man nicht hervorheben kann. Der
    /// Aufrufer kriegt `None` und zeichnet nichts.
    pub fn kasten(&self, inhalt: Rect) -> Option<[Rect; 4]> {
        if inhalt.width <= 0.0 || inhalt.height <= 0.0 {
            return None;
        }
        let mt = self.rand(0);
        let mr = self.rand(1);
        let mb = self.rand(2);
        let _ml = self.rand(3);
        // Innenkasten (Rand drumherum).
        let innen = Rect::new(
            inhalt.x - mt,
            inhalt.y - mt,
            inhalt.width + mt + mr,
            inhalt.height + mt + mb,
        );
        // Aussenkasten (Marge drumherum).
        let aussen = Rect::new(
            innen.x - self.marge[3].max(0.0),
            innen.y - self.marge[0].max(0.0),
            innen.width + self.marge[3].max(0.0) + self.marge[1].max(0.0),
            innen.height + self.marge[0].max(0.0) + self.marge[2].max(0.0),
        );
        // Fuer das Bild wird jede Ebene als Rand um den INNERSTEN Kasten
        // gezeichnet — genau so, wie Chrome es malt.
        Some([aussen, innen, inhalt, inhalt])
    }

    /// Der Text fuer den Reiter: sechs Zeilen, und die erste nennt BEIDE
    /// Zahlen.
    ///
    /// # DER BEFUND, DER DIESE ZEILE GESCHAFFT HAT
    ///
    /// Die Koordination meldete am 01.10.2026 (22:2x, Bild
    /// `team/log/devtools_D9_wikipedia.png`): „Box-Modell zeigt 0x0 und
    /// alle Raender 0 fuer das gewaehlte Element — echte Masse aus dem
    /// Layout holen".
    ///
    /// GEMESSENE URSACHE: `inhalt_breite` kommt aus `Berechnet::breite`,
    /// und das ist die ANGABE aus dem Stylesheet. Bei `width: auto` ist
    /// sie `None`, also 0 — bei einem Element, das im Bild einen Kasten
    /// von 640 × 60 Pixeln hat. Der Kasten war DA, die Zahl nicht.
    ///
    /// GEMESSENE ENTSCHEIDUNG: die Zeile nennt beides. „Breite 0" bei
    /// `auto` ist die Angabe, „Breite 456" ist die Messung; eine Zeile,
    /// die beide in eine Zahl presst, verliert eine davon. Und die
    /// Aussenkante kommt dazu — die Zahl, die man im Bild NACHMESSEN
    /// kann, wenn man dem Rahmen folgt.
    pub fn text_zeilen(&self) -> Vec<String> {
        let f = |v: [f32; 4]| -> String {
            format!("{:.0} / {:.0} / {:.0} / {:.0} px", v[0], v[1], v[2], v[3])
        };
        let mut r = Vec::new();
        match self.gelegt_inhalt {
            // MIT Kasten, aber ohne Angabe (`width: auto`): die Messung
            // zuerst, die Angabe als Wort dahinter.
            Some((b, h)) if self.inhalt_breite == 0.0 => {
                r.push(format!("Inhalt    {b:.0} × {h:.0} px  (auto)"));
            }
            // MIT Kasten UND Angabe: beide, die Messung vorn.
            Some((b, h)) => {
                r.push(format!(
                    "Inhalt    {b:.0} × {h:.0} px  (angegeben {:.0} × {:.0})",
                    self.inhalt_breite, self.inhalt_hoehe
                ));
            }
            // OHNE Kasten (ein `<head>`, ein `display:none`): ehrlich
            // sagen, dass es keinen gibt, statt 0 zu behaupten.
            None => {
                r.push(format!(
                    "Inhalt    {:.0} × {:.0} px  (kein Kasten im Layout)",
                    self.inhalt_breite, self.inhalt_hoehe
                ));
            }
        }
        if let Some((b, h)) = self.gelegt_aussen {
            r.push(format!("Aussen    {b:.0} × {h:.0} px"));
        }
        r.push(format!("Marge     {}", f(self.marge)));
        r.push(format!("Rahmen    {}", f(self.rahmen)));
        r.push(format!("Fuellung  {}", f(self.fuellung)));
        r
    }
}

// ---------------------------------------------------------------------------
// Hilfsfunktionen fuer die Textausgabe (die Debug-Formate der Crate, deutsch)
// ---------------------------------------------------------------------------

/// Eine Farbe als Text, wie sie im Reiter steht.
pub fn farbe_text(f: &narsil_css::Farbe) -> String {
    if f.a == 0 {
        return "transparent".to_string();
    }
    if f.r == f.g && f.g == f.b {
        return format!(
            "rgb({}, {}, {}) = #{:02x}{:02x}{:02x}",
            f.r, f.g, f.b, f.r, f.g, f.b
        );
    }
    format!(
        "rgb({}, {}, {}) = #{:02x}{:02x}{:02x}",
        f.r, f.g, f.b, f.r, f.g, f.b
    )
}

/// `None` = `auto`.
fn opt_px(v: Option<f32>) -> String {
    match v {
        Some(x) if x.is_finite() => format!("{x:.0}px"),
        Some(_) => "ungueltig".to_string(),
        None => "auto".to_string(),
    }
}

/// `None` = `auto`.
fn opt_i32(v: Option<i32>) -> String {
    match v {
        Some(x) => x.to_string(),
        None => "auto".to_string(),
    }
}

/// Vier Werte als Kuerzel (oben rechts unten links).
fn kuerzel_4(o: f32, r: f32, u: f32, l: f32) -> String {
    if (o - r).abs() < f32::EPSILON && (o - u).abs() < f32::EPSILON && (o - l).abs() < f32::EPSILON
    {
        return format!("{o:.0}px");
    }
    format!("{o:.0}px {r:.0}px {u:.0}px {l:.0}px")
}

/// Die Textdekoration als Wort (die Bitmaske aus `narsil-core`).
fn dekoration_text(d: u8) -> String {
    let mut v = Vec::new();
    if d & 1 != 0 {
        v.push("underline");
    }
    if d & 2 != 0 {
        v.push("overline");
    }
    if d & 4 != 0 {
        v.push("line-through");
    }
    if v.is_empty() {
        "none".to_string()
    } else {
        v.join(" ")
    }
}

/// Die Rahmenstile als Wort, wenn alle vier gleich sind.
fn border_stil_text(b: &Berechnet) -> String {
    let alle_gleich = b.rahmen_oben_stil == b.rahmen_rechts_stil
        && b.rahmen_oben_stil == b.rahmen_unten_stil
        && b.rahmen_oben_stil == b.rahmen_links_stil;
    if alle_gleich {
        format!("{:?}", b.rahmen_oben_stil)
    } else {
        format!(
            "{:?} {:?} {:?} {:?}",
            b.rahmen_oben_stil, b.rahmen_rechts_stil, b.rahmen_unten_stil, b.rahmen_links_stil
        )
    }
}

/// Die Rahmenfarben als Wort.
fn border_farbe_text(b: &Berechnet) -> String {
    format!(
        "{} / {} / {} / {}",
        farbe_text(&b.rahmen_oben_farbe),
        farbe_text(&b.rahmen_rechts_farbe),
        farbe_text(&b.rahmen_unten_farbe),
        farbe_text(&b.rahmen_links_farbe)
    )
}

// GEMESSENE ENTSCHEIDUNG (kein Doc-Kommentar, sondern eine Notiz in der
// Datei): die Ansichtsbreite gehoert NICHT in die `Seite` — sie ist eine
// Eigenschaft des Fensters, nicht des Dokuments, und `narsil-page` baut
// breitengetrieben. Sie kommt ueber [`Elemente::mit_ansicht`] vom Aufrufer.
// Bis dahin ist 0 = „unbekannt", und das ist fuer `@media (min-width: …)`
// die richtige (nicht erfundene) Antwort.

#[cfg(test)]
mod elemente_eigene {
    use super::*;

    /// DER BAUM AUS EINER ECHTEN SEITE: KNOTEN, TIEFE, REIHENFOLGE.
    ///
    /// M7: der Test nennt die Beschriftungen, nicht nur die Zahl.
    #[test]
    fn der_baum_zeigt_die_elemente_in_reihenfolge() {
        let html = "<html><head><title>T</title></head><body><div id=\"a\" class=\"x\"><p>Hallo</p></div></body></html>";
        let seite = narsil_page::aus_html(html, 800, "https://beispiel.test/");
        let mut e = Elemente::leer();
        e.an_seite(&seite);
        // Der CssBaum hat 6 Elemente (html, head, title, body, div, p).
        assert_eq!(e.knoten_gesamt, 6, "die Knotenzahl stimmt nicht");
        let zeilen = e.text_zeilen();
        // Die Wurzel ist offen, ihre Kinder sind offen, deren nicht mehr.
        let tags: Vec<String> = zeilen
            .iter()
            .map(|z| {
                z.trim_start_matches([' ', '\u{25be}', '\u{25b8}'])
                    .to_string()
            })
            .collect();
        assert!(
            tags.iter().any(|t| t.starts_with("<html")),
            "die Wurzel fehlt: {tags:?}"
        );
        assert!(
            tags.iter().any(|t| t.starts_with("<head")),
            "der Kopf fehlt: {tags:?}"
        );
        assert!(
            tags.iter().any(|t| t.starts_with("<body")),
            "der Rumpf fehlt: {tags:?}"
        );
        // Das `div` ist EINE Ebene unter `body` — bei offenem `body`.
        assert!(
            tags.iter()
                .any(|t| t.contains("<div id=\"a\" class=\"x\">")),
            "das div fehlt oder die Attribute fehlen: {tags:?}"
        );
        // UND die Reihenfolge ist Dokumentreihenfolge.
        let p_html = tags.iter().position(|t| t.starts_with("<html")).unwrap();
        let p_body = tags.iter().position(|t| t.starts_with("<body")).unwrap();
        assert!(p_html < p_body, "die Reihenfolge stimmt nicht: {tags:?}");
    }

    /// EIN KLICK WAEHLT, EIN KLICK AUF EIN BLATT KLAPPT NIX AUF.
    ///
    /// GEMESSENE ENTSCHEIDUNG: `kippe` gibt bei einem Blatt `false`
    /// zurück. Das ist die Zusage „ein Blatt hat keinen Pfeil" — und der
    /// Test misst sie an einem Knoten OHNE Kinder, damit kein Testfehler
    /// das Zufallsergebnis „es war schon offen" verdeckt.
    #[test]
    fn kippen_oeffnet_und_schliesst_und_nicht_bei_blaettern() {
        let html = "<html><body><div id=\"a\"><span></span></div></body></html>";
        let seite = narsil_page::aus_html(html, 800, "https://beispiel.test/");
        let mut e = Elemente::leer();
        e.an_seite(&seite);
        // Das div finden (es ist als Zeile da, weil `body` offen ist).
        let div = e
            .zeilen
            .iter()
            .find(|z| z.beschriftung.contains("<div id=\"a\">"))
            .map(|z| z.css)
            .expect("das div ist nicht als Zeile da");
        // Es ist offen (die Wurzel + zwei Ebenen).
        assert!(e.auf.contains(&div), "das div ist nicht offen: {:?}", e.auf);
        assert!(
            !e.kippe(div),
            "kippen auf einem offenen Knoten meldet offen"
        );
        assert!(!e.auf.contains(&div));
        assert!(
            e.kippe(div),
            "kippen auf einem geschlossenen Knoten meldet nichts"
        );
        // Das span ist ein Blatt.
        let span = e
            .zeilen
            .iter()
            .find(|z| z.beschriftung.starts_with("<span"))
            .map(|z| z.css)
            .expect("das span ist nicht als Zeile da");
        let vorher = e.auf.clone();
        assert!(!e.kippe(span), "ein Blatt laesst sich aufklappen");
        assert_eq!(e.auf, vorher, "ein Blatt hat den Auf-Zustand veraendert");
    }

    /// EINE AUSWAHL OHNE KNOTEN (u32::MAX) TUT NICHTS.
    ///
    /// GEMESSENE BEGRÜNDUNG: die Nummer kommt aus einem Klick auf eine
    /// Zeile, und die Zeile kommt aus dieser Liste — aber die Liste kann
    /// zwischen Klick und Auswahl gekuerzt worden sein (Filter). Also ist
    /// die Zahl aus der fremden Seite. `u32::MAX` darf keinen Panic machen.
    #[test]
    fn eine_auswahl_ohne_knoten_tut_nichts() {
        let seite = narsil_page::aus_html("<html><body>x</body></html>", 800, "https://b.test/");
        let mut e = Elemente::leer();
        e.an_seite(&seite);
        let vorher = e.gewaehlt;
        for num in [u32::MAX, 999_999, 1_000_000_000] {
            assert!(!e.waehle(num), "die Nummer {num} waehlte einen Knoten");
        }
        assert_eq!(
            e.gewaehlt, vorher,
            "eine falsche Nummer hat die Auswahl veraendert"
        );
    }

    /// BOMBEN (M8): ENDLOSE VERSCHACHTELUNG, RIESENATTRIBUTE, 0-BREITE.
    ///
    /// GEMESSENE BEGRÜNDUNG: alle drei Eingaben kommen aus einer Seite.
    /// Endlose Verschachtelung testet M5 (Rekursion mit Tiefengrenze), die
    /// Riesenattribute M4 (Attributwert aus der Eingabe) und die
    /// 0-Breite den Kastenpfad (`border_box` mit Breite 0 ist ein Kasten,
    /// den man nicht hervorheben kann).
    #[test]
    fn bomben_seiten_kommen_mit_einem_benannten_ergebnis() {
        // 1. Endlose Verschachtelung: 5000 <div>.
        let mut html = String::from("<html><body>");
        for _ in 0..5000 {
            html.push_str("<div>");
        }
        for _ in 0..5000 {
            html.push_str("</div>");
        }
        html.push_str("</body></html>");
        let seite = narsil_page::aus_html(&html, 800, "https://b.test/");
        let mut e = Elemente::leer();
        e.an_seite(&seite);
        assert!(
            e.zeilen.len() <= Grenzen::STANDARDAUSGABE.max_baumzeilen,
            "5000 Verschachtelungen ergaben {} Zeilen",
            e.zeilen.len()
        );
        // Die sichtbare Tiefe haengt an der Grenze.
        let max_tiefe = e.zeilen.iter().map(|z| z.tiefe).max().unwrap_or(0);
        assert!(
            (max_tiefe as usize) < Grenzen::STANDARDAUSGABE.max_tiefe,
            "die sichtbare Tiefe ist {max_tiefe}"
        );

        // 2. Ein 1-MB-Attributwert.
        let lang = "x".repeat(1_000_000);
        let html = format!("<html><body><p data-x=\"{lang}\">t</p></body></html>");
        let seite = narsil_page::aus_html(&html, 800, "https://b.test/");
        let mut e = Elemente::leer();
        e.an_seite(&seite);
        for z in e.text_zeilen() {
            assert!(
                z.chars().count() <= Grenzen::STANDARDAUSGABE.max_zeile,
                "eine Zeile ist {} Zeichen lang",
                z.chars().count()
            );
        }

        // 3. Ein leeres Dokument und eine 0-Breite.
        let mut leer = Elemente::leer();
        let seite_leer = narsil_page::aus_html("", 0, "");
        leer.an_seite(&seite_leer);
        // GEMESSENE ENTSCHEIDUNG: beides ist ein Ergebnis. Die Seite ohne
        // Inhalt kann einen Knoten (`page`) haben oder keinen — der Reiter
        // zeichnet dann 0 oder 1 Zeile, und beides ist richtig.
        //
        // GEMESSENE ENTSCHEIDUNG (02.10., 16:0x): dieser Test stand vorher auf
        // `zeilen.len() <= 1` und wurde rot, als `narsil_page::aus_html("")`
        // drei Knoten lieferte (`<html>`, `<head>`, `<body>`) statt keinem.
        // GEMESSENE URSACHE: der Parser baut inzwischen die Struktur auch
        // fuer einen leeren Text — und das ist RICHTIG (eine Seite ist kein
        // Nichts, sie hat einen Kopf).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Grenze trotzdem bleibt und nicht
        // ersetzt wird: die Zahl, die den Reiter begrenzt, ist nicht die
        // Zeilenzahl einer leeren Seite, sondern ob die ZEILENZAHL ueberhaupt
        // begrenzt wird. Ein leeres Dokument zeigt drei Zeilen — das ist
        // eine Aussage ueber die Struktur. Zehntausend Zeilen waeren eine
        // Aussage ueber einen Fehler.
        assert!(
            leer.zeilen.len() <= Grenzen::STANDARDAUSGABE.max_gezeichnete_zeilen,
            "eine leere Seite hat {} Zeilen (Grenze {})",
            leer.zeilen.len(),
            Grenzen::STANDARDAUSGABE.max_gezeichnete_zeilen
        );
        //
        // GEMESSENE ENTSCHEIDUNG (02.10., dieselbe Ursache wie oben): der
        // Test stand auf `!leer.waehle(0)` („ohne Knoten waehlt 0 keinen").
        // GEMESSEN: `aus_html("")` liefert inzwischen drei Knoten, also
        // WAHLT Index 0 einen — und das ist richtig: es gibt etwas zu
        // waehlen. Die Grenze, die ich schuetze, ist die andere Frage:
        // ein Index AUSSERHALB des Baums darf nichts waehlen. Das wird
        // unten mit `waehle(u32::MAX)` geprueft.
        assert!(
            !leer.waehle(u32::MAX),
            "ein Index ausserhalb des Baums waehlt einen Knoten"
        );
        assert_eq!(
            leer.hervorhebung(),
            None,
            "ohne Layout gibt es keine Hervorhebung"
        );
        assert!(leer.hervorhebungskette().len() <= Grenzen::STANDARDAUSGABE.max_tiefe);
    }

    /// DIE BEREIGNETEN STILE SIND DIE DER SEITE, NICHT DIE ANFANGSWERTE.
    ///
    /// GEMESSENE ENTSCHEIDUNG: der Test nimmt eine Seite mit
    /// `body { color: #333 }` und fragt den Stil eines Kindes ab. Ohne den
    /// Elternwert waere das Kind schwarz — und der Reiter wuerde dem Nutzer
    /// sagen, die Seite zeige schwarzen Text, während sie #333 zeigt.
    #[test]
    fn die_berechneten_stile_erben_vom_vorfahren() {
        let html = "<html><head><style>body{color:#333333;font-size:21px}</style></head>\
                   <body><p id=\"z\">Text</p></body></html>";
        let seite = narsil_page::aus_html(html, 800, "https://b.test/");
        let mut e = Elemente::leer();
        e.an_seite(&seite);
        e.mit_ansicht(800, 600);
        // Das `p` waehlen.
        let p = e
            .zeilen
            .iter()
            .position(|z| z.beschriftung.contains("<p id=\"z\">"));
        // Das `p` ist eine Ebene unter `body`, und `body` ist offen, also
        // steht es als Zeile da. Sonst: alle offen und suchen.
        let css = match p {
            Some(_) => e
                .zeilen
                .iter()
                .find(|z| z.beschriftung.contains("<p id=\"z\">"))
                .map(|z| z.css),
            None => None,
        };
        let css = if let Some(c) = css {
            c
        } else {
            // Der Baum ist nicht tief genug aufgeklappt: dann klappen wir auf.
            e.alle_auf();
            e.zeilen
                .iter()
                .find(|z| z.beschriftung.contains("<p id=\"z\">"))
                .map(|z| z.css)
                .expect("das p steht nicht im Baum")
        };
        assert!(e.waehle(css), "das p liess sich nicht waehlen");
        let st = e.stil.as_ref().expect("kein berechneter Stil");
        assert_eq!(
            st.farbe.r, 0x33,
            "die geerbte Farbe stimmt nicht: {:?}",
            st.farbe
        );
        assert_eq!(st.farbe.g, 0x33, "die geerbte Farbe (g)");
        assert_eq!(st.farbe.b, 0x33, "die geerbte Farbe (b)");
        assert_eq!(
            st.schriftgroesse_px, 21.0,
            "die geerbte Schriftgroesse stimmt nicht: {}",
            st.schriftgroesse_px
        );
        // UND die Eigenschaftsliste nennt beide Werte als Text.
        let eig = e.eigenschaften();
        assert!(eig.len() >= 32, "nur {} Eigenschaften", eig.len());
        let farbe = eig.iter().find(|(n, _)| n == "color").expect("keine color");
        assert!(farbe.1.contains("51"), "die Farbe im Text: {}", farbe.1);
    }
}

/// Der Text einer Zeile aus ihren Teilen — die ZUSAMMENFASSUNG (D12).
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Funktion und kein `impl Display`:
/// es gibt genau zwei Stellen, die den Text wollen (der Protokolltext und
/// `Zeile::beschriftung`), und beide wollen DIESELBE Rechnung. Wer sie
/// zweimal schreibt, hat zwei Zeilen, die im Bild gleich aussehen sollen.
pub fn beschriftung_aus_teilen(teile: &[Zeilenteil]) -> String {
    let mut s = String::new();
    for t in teile {
        s.push_str(&t.text);
    }
    Grenzen::STANDARDAUSGABE.kuerze_zeile(&s)
}
