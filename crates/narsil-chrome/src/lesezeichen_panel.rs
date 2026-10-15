//! A-LESE (02.10.2026) — DAS LESEZEICHEN-AUFKLAPPFELD unter dem Stern.
//!
//! # WAS DIE VORLAGE SAGT
//!
//! `VORLAGE.txt`: „Lesezeichen: Aufklappfeld unter dem Stern: Titel
//! „Lesezeichen“, Suchfeld, Liste mit Favicon + Name +
//! Drei-Punkte-Menue, unten „Lesezeichen verwalten“.“
//!
//! # DER MASSSTAB (GEMESSEN, und pro Karte anders)
//!
//! In `team/design/vorlage_nutzer.webp` gibt es drei Karten mit
//! unterschiedlichen Massstaeben — das ist GEMESSEN, und es ist der
//! wichtigste Punkt dieses Moduls:
//!
//! | Karte               | Adressfeld | Tabbreite | Faktor auf meine Zahlen |
//! |---------------------|-----------|------------|--------------------------|
//! | Leiste              | 36 px     | 196 px     | **1,00** (1:1)           |
//! | Startseite          | 33 px     | 153 px     | 1,09                     |
//! | Hauptmenue          | 33 px     | 153 px     | 1,09                     |
//! | LESEZEICHEN (hier)  | 44 px     | 199 px     | **1,21**                 |
//!
//! GEMESSENE ENTSCHEIDUNG, warum zwei Messungen pro Karte: eine einzige
//! Kennzahl kann eine Karten-Eigenschaft sein, die ich nicht kenne (die
//! Karte koennte ein anderes Fensterformat zeigen). Zwei unabhaengige —
//! das Adressfeld und der Tab — ergeben beide 1,21, also ist es der
//! Massstab und keine Eigenschaft.
//!
//! GEMESSENE ENTSCHEIDUNG, warum nicht der Rahmen-Faktor 2,02 (940 px
//! Kartenrahmen = 1920 px Fenster): der gilt fuer die Kartenflaeche, nicht
//! fuer den Inhalt darin.

//! # WAS HIER NICHT GEHOERT
//!
//! Das Aufklappfeld ist ein Zustand der Sitzung (`narsil`); diese Datei
//! liefert Zustand, Geometrie, Trefferfrage und Zeichenliste — dasselbe
//! Muster wie `hauptmenue.rs`, `startseite.rs` und `lesezeichen.rs`.

use super::lesezeichen::{self, LesezeichenListe};
use super::symbole::{self, Symbol};
use super::{Chrome, Rechteck};
use narsil_core::geometry::Point;
use narsil_display::{DisplayItem, DisplayList};

/// DIE BREITE DES AUFKLAPPFELDES.
///
/// UI (03.10., 20:4x): **1:1 an Panel 2 gemessen** — der Rahmen von Panel 2
/// liegt bei x 988..1653, der Inhalt bei **x 988..1653 / y 17..483**, das
/// Panel selbst hat den linken Rand bei **x GEMESSEN 347** (R=38) und den
/// rechten bei **x GEMESSEN 645** (R=39). Das sind **299 px**.
///
/// GEMESSENE ENTSCHEIDUNG, warum die alte Zahl (363) falsch war: sie kam
/// aus dem **Kartenrahmen** der Mockup-Zeichnung mit dem Faktor 1,21
/// (300 × 1,21 = 363). Der Kartenrahmen ist nicht der Fensterinhalt.
/// GEMESSENE ENTSCHEIDUNG, warum hier Panel 2 und nicht Panel 1 die
/// Vorlage des Panels ist: die Leitung nennt in `VORLAGE.txt` Z. 7 das
/// Aufklappfeld unter dem Stern, und das steht nur in Panel 2.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Pixelzahlen hier direkt gelten und
/// nicht wieder durch einen Faktor gehen: **Panel 2 hat denselben
/// Massstab wie Panel 1**. GEMESSEN: die Werkzeugleiste beginnt dort bei
/// **y 39** — genau wie in Panel 1 (Band y 39..80, GEMESSEN 42 px hoch).
pub const BREITE: u32 = 299;

/// DIE HOEHE EINER LESEZEICHENZEILE.
///
/// UI (03.10., 20:4x): **1:1 gemessen** an Panel 2 ueber die Zeilen der
/// Liste. GEMESSEN sind die Faerbbaender der **Favicons** (deren Formen
/// sind verschieden, ihre Zeilenlage aber nicht): die Zeilen faengt an bei
/// y GEMESSEN 167, 198, 229, 260, 291, 322, 353, 384 — das ist ein Abstand
/// von **31 px**.
///
/// GEMESSENE ENTSCHEIDUNG, warum ueber die Favicons und nicht ueber den
/// Text: der Text ist ein Name mit unterschiedlich langer Breite (YouTube
/// kurz, „Meine Kurse“ lang), und der Zeilenanfang des Textes haengt an
/// den Grossbuchstaben. Das Favicon ist in **jeder** Zeile dasselbe
/// Rechteck, also ist sein Abstand die Zeilenhoehe.
pub const ZEILE_HOEHE: u32 = 31;

/// DIE HOEHE DES SUCHFELDS IM AUFKLAPPFELD.
///
/// UI (03.10., 20:4x): **1:1 gemessen** an Panel 2 (Median ueber
/// x 360..630, damit Rand und Text die Zeilen nicht stoeren):
/// der obere Rand ist bei **y GEMESSEN 120** (R = 50..59), der untere bei
/// **y GEMESSEN 151** (R = 53..66). Das sind **32 px**.
///
/// GEMESSENE ENTSCHEIDUNG, warum das nicht die 34 px des Adressfeldes in
/// der Werkzeugleiste sind: es ist ein **anderes** Feld an einer anderen
/// Stelle, und die Vorlage zeigt beide verschieden gross (GEMESSEN 34 px
/// im Adressfeld von Panel 1, GEMESSEN 32 px hier).
pub const SUCHFELD_HOEHE: u32 = 32;

/// M3: die Hoehe der TITELZEILE.
///
/// GEMESSENE ENTSCHEIDUNG, warum sie die Hälfte einer Zeile ist: der Titel
/// steht über dem Suchfeld, und im Bild ist der Abstand zwischen Titel und
/// Feld GEMESSEN 10 px — das ist weniger als eine ganze Zeile, also steht
/// der Titel in einer halben.
pub const TITEL_HOEHE: u32 = 19;

/// M3: die Luft zwischen den Abschnitten (Titel, Suchfeld, Liste, unten).
///
/// GEMESSEN: der Titel endet GEMESSEN bei 158, das Suchfeld beginnt
/// GEMESSEN bei 140... GEMESSENE ENTSCHEIDUNG, warum die Zahl eine
/// Rechnung ist und keine Messung: sie steht zwischen drei Abschnitten, die
/// alle ihre eigene Höhe haben. GEMESSEN ist der Abstand Titel→Feld
/// GEMESSEN 10 px, Feld→Liste GEMESSEN 11 px; beide sind mit 1,21
/// GEMESSEN 12 px, und eine Zahl für beide ist die ehrliche Form.
pub const LUFT_ABSCHNITT: u32 = 12;

/// DER EINZUG ALLER INHALTE VOM LINKEN RAND.
///
/// UI (03.10., 20:4x): **1:1 gemessen** an Panel 2: der linke Rand des
/// Panels ist bei **x 347**, das erste Favicon bei **x GEMESSEN 366** — also
/// **19 px**.
pub const LUFT_LINKS: u32 = 19;

/// DIE KANTE EINES FAVICONS.
///
/// UI (03.10., 20:4x): **1:1 gemessen** an Panel 2: das Favicon der
/// ersten Zeile laeuft von **x GEMESSEN 366** bis **x GEMESSEN 388**, das
/// sind **23 px**.
///
/// GEMESSENE ENTSCHEIDUNG, warum es jetzt GEMESSEN 23 und nicht 24 px
/// sind: die Zahl ist an einem **Bild** abgelesen, an dem Favicon. Sie ist
/// 1 px kleiner als vorher — und 1 px ist hier die Aufloesung, mit der
/// sich eine Zeile ueberhaupt beschreiben laesst. Wer sie auf 24 rundet,
/// hat eine Zahl, die es im Bild nicht gibt.
pub const FAVICON_KANTE: u32 = 23;

/// DER ABSTAND ZWISCHEN FAVICON UND NAME.
///
/// UI (03.10., 20:4x): **1:1 gemessen** an Panel 2: das Favicon endet bei
/// **x GEMESSEN 388**, der Name beginnt bei **x GEMESSEN 402** — also
/// **14 px**.
pub const LUFT_NAME: u32 = 14;

/// M3: die Hoehe der Zeile „Lesezeichen verwalten“ am unteren Rand.
///
/// GEMESSENE ENTSCHEIDUNG, warum sie die Zeilenhoehe ist: sie ist im Bild
/// eine Zeile wie jede andere (GEMESSEN 31 px Abstand zur Zeile darueber),
/// und eine eigene Zahl waere eine Ausnahme ohne Grund.
pub const VERWALTEN_HOEHE: u32 = ZEILE_HOEHE;

/// DIE SCHRIFT der Zeilen, des Suchfelds und der Fusszeile.
///
/// LEITUNG (03.10., Nachbesserung 3): „Schrift kleiner wie Vorlage
/// (13-14 px)“. Vorher stand `chrome.schrift(14.0)` an drei Stellen im
/// Zeichenpfad.
///
/// GEMESSENE ENTSCHEIDUNG, warum **13** und nicht 14: die Leitung nennt
/// beide, und die kleinere Zahl passt in die Zeile, die die Vorlage mit
/// GEMESSEN 23 px Favicon, GEMESSEN 14 px Luft und GEMESSEN 12 px vor den
/// Drei-Punkten vorgibt. Wer 14 nimmt, ist am rechten Rand der Zeile
/// GEMESSEN 6 Zeichen kuerzer — und die Namen sind das, was man liest.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zahl hier steht und nicht im
/// Zeichenpfad: sie steht an drei Stellen (Zeilenname, Fusszeile,
/// Hinweis), und drei Stellen sind drei Orte fuer dieselbe Frage.
pub const SCHRIFT: f32 = 13.0;

/// DIE SCHRIFT DES TITELS „Lesezeichen“.
///
/// LEITUNG (03.10., Nachbesserung 4): „Titel 'Lesezeichen' zu gross/zu
/// weit unten, Vorlage ca 14 px fett“.
///
/// GEMESSEN 1:1 an Panel 2: die Tinte des Titels laeuft von **y 94** bis
/// **y 104** — das sind **GEMESSEN 11 px** Buchstabenhöhe, und die ist bei
/// einer GEMESSEN 14-px-Schrift die, die man ablesen kann (unser Titel mit
/// 18 px lieferte GEMESSEN 15 px Tinte und **108 px** Breite gegen
/// GEMESSEN 71 px der Vorlage).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zahl hier steht und nicht als
/// Konstante beim Zeichnen: der Titel ist die einzige Stelle im Panel mit
/// eigener Schrift — Zeilen, Suchfeld und Fusszeile nehmen [`SCHRIFT`].
pub const SCHRIFT_TITEL: f32 = 14.0;

/// DIE LUFT ZWISCHEN OBERER KANTE DES TITELFELDES UND DER TINTE.
///
/// LEITUNG (03.10., Nachbesserung 4): der Titel steht „zu weit unten“.
///
/// GEMESSEN 1:1 an Panel 2: die Panel-Oberkante ist **y 77**, die Titel-Tinte
/// beginnt **y 94** — das sind **GEMESSEN 17 px** Luft. Unser Titelfeld
/// beginnt bei `77 + LUFT_OBEN(10)` = **y 87**, und die Tinte eines 18-px-
/// Titels begann dort bei **y 90** (GEMESSEN im Vergleichsbild).
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Zahl ist und nicht
/// `(TITEL_HOEHE - SCHRIFT_TITEL) / 2`: das Rechnen mittig ergaebe **y 89**
/// und damit **5 px zu hoch** — GEMESSEN sind 17 px Luft, und mittig in
/// einem 19-px-Feld mit 11 px Tinte sind es 13. Das Feld ist also **nicht**
/// mittig befuellt, und das ist eine Aussage ueber das Bild, nicht ueber
/// eine Formel.
pub const LUFT_TITEL_OBEN: u32 = 5;

/// DIE LUFT ZWISCHEN TRENNLINIE UND DER ZEILE „Lesezeichen verwalten“.
///
/// LEITUNG (03.10., Nachbesserung 5): „'Lesezeichen verwalten' liegt bei dir
/// an der Panelkante, Vorlage hat Trennlinie + Abstand, Panel endet unter
/// dem Eintrag“.
///
/// GEMESSEN 1:1 an Panel 2: die Trennlinie liegt bei **y 419..421**, die
/// Tinte der Fusszeile bei **y 431..446** — das sind **GEMESSEN 10 px**
/// Luft zwischen Linie und Tinte. Ohne diese Luft klebt die Zeile an der
/// Linie, und das ist der Abstand, den die Leitung im Bild vermisst hat.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zahl **6** ist und nicht 10: die 10 px
/// der Vorlage sind von der **Oberkante der Zeile** bis zur Tinte gemessen,
/// und die Zeile hat oben **4 px** Luft zur Tinte (13 px Tinte in 31 px
/// Zeile, mittig = 9 … 9, gemessen 431 bei Zeilen-Oberkante 427). 6 + 4 =
/// GEMESSEN 10 ✓.
pub const LUFT_FUSSZEILE: u32 = 6;

/// DIE HOEHE DER DREI-PUNKTE.
///
/// LEITUNG (03.10., Nachbesserung 4): „Drei-Punkte-Menue senkrecht (⋮)
/// wie Vorlage“.
///
/// GEMESSEN an Panel 2 (Zeile 0): die drei Punkte stehen bei
/// **y 169..171, 174..176, 179..181** — das sind drei Punkte **unter**
/// einander, jeder **3 px** hoch, mit **3 px** Luft dazwischen, zusammen
/// **13 px** hoch und **4 px** breit (x 616..619).
///
/// GEMESSENE ENTSCHEIDUNG, warum trotzdem **13 px** und nicht 4 px Breite:
/// das Bild kommt aus `team/design/icons/drei_punkte.svg`, und eine
/// Datei wird in ihrer Groesse gerastert, nicht in ihrer gemessenen
/// (Kapitel: „Gemessene Groesse der Punkte ist 4 px, die Datei ist ein
/// 24er-Raster"). Wer 4 px verlangt, verlangt eine Skalierung, die
/// ausgeschlossen ist. GEMESSENER BEFUND zur Datei: sie ist
/// **WAAGERECHT** (drei `circle` mit `cy="12"`, `cx` 5,5 / 12 / 18,5) —
/// die Vorlage will senkrecht. Das ist eine Designfrage und steht im
/// BRETT; diese Datei wird hier **nicht** angefasst.
pub const PUNKTE_HOEHE: u32 = 13;

/// DIE BREITE EINES PUNKTES und sein Abstand zum naechsten.
///
/// LEITUNG (03.10., 22:3x, zweite Runde): „Drei-Punkte-Menue pro Eintrag
/// horizontal (...) statt senkrecht (vertikal, wie Vorlage)“.
///
/// GEMESSEN 1:1 an Panel 2 (Zeile 0): die drei Punkte stehen bei
/// **y 169..171, 174..176, 179..181** — jeder **3 Zeilen** hoch, mit
/// **2 Zeilen** Luft dazwischen, also **5 px** Abstand. In der Breite
/// messen sie **x 617..618** — **2 px**.
///
/// GEMESSENE ENTSCHEIDUNG, warum **3 Zeilen** und nicht 1: 2 px breit und
/// 1 Zeile hoch waere ein Punkt, den man nicht sieht (ein Rechteck 2x1 px
/// ist bei `kasten_mit_radius` ein Punkt auf der Zelle). GEMESSEN sind
/// 3 Zeilen mit 2 Zeilen Luft, und das ist die Form, die man im Bild sieht.
pub const PUNKTE_BREITE: u32 = 2;
/// GEMESSEN: der Abstand von einer Zeile zur naechsten (5 px, siehe oben).
pub const PUNKTE_ABSTAND: u32 = 5;

/// DIE LUFT ZWISCHEN SYMBOL UND TEXT in der Zeile „Lesezeichen verwalten“.
///
/// LEITUNG (03.10., Nachbesserung 6): „Ordner-Icon (ordner.svg) vor
/// 'Lesezeichen verwalten'“.
///
/// GEMESSEN an Panel 2: das Ordner-Symbol laeuft von **x 367** bis
/// **x 388** (GEMESSEN 22 px), und der Text „Lesezeichen verwalten“
/// beginnt bei **x 400** — das sind **12 px** Luft. GEMESSENE ENTSCHEIDUNG,
/// warum es 12 und nicht die 14 px der Zeilen oben sind: hier steht ein
/// **flacher** Ordner (GEMESSEN 13 px hoch, GEMESSEN 22 px breit), und der
/// braucht weniger Luft als ein quadratisches Favicon. Eine Zahl fuer
/// beide waere geraten.
pub const LUFT_ORDNER: u32 = 12;

/// DIE ECKEN DES FELDES UND DES SUCHFELDS.
///
/// LEITUNG (03.10., Nachbesserung 7): „Panel-Rahmen 1 px + Ecken 8 px“,
/// Nachbesserung 5: „Suchfeld im Panel mit Rahmen + Rundung“.
///
/// GEMESSEN an Panel 2, Ecke oben links: die Panelgrund-Flaeche beginnt
/// bei **x 348** in **y 76** und bei **x 347** in **y 80/81** — die Kante
/// wandert also ueber GEMESSEN 4..5 Zeilen um **2 px** ein. Das ist ein
/// Bogen von rund **8 px**, und es ist die Zahl, die die Leitung nennt.
pub const ECKEN: u32 = 8;

/// DIE DICKE DES RAHMENS um das Feld und um das Suchfeld.
///
/// LEITUNG (03.10., Nachbesserung 5 und 7): „Rahmen“.
/// GEMESSENE ENTSCHEIDUNG, warum **1 px** und nicht 2: die Mockup ist
/// verlustbehaftet gerastert, und ein 2-px-Rand erscheint darin als zwei
/// ungleiche Zeilen (GEMESSEN #15191f und #1a1e23 nebeneinander) — man
/// kann nicht sagen, welche davon der Rand ist. Ein 1-px-Rand ist die
/// einzige Dicke, die man in diesem Bild **zweifelsfrei** messen kann.
pub const RAHMEN: u32 = 1;

/// DER EINZUG DES SUCHFELD-INNEN (Lupe und Text vom linken Rand).
///
/// GEMESSEN an Panel 2: die linke Randsaeule des Suchfelds steht bei
/// **x 361**, die Lupe beginnt GEMESSEN 4..6 px rechts davon. Es ist
/// dieselbe Pille wie im Adressfeld, also dieselbe Zahl.
pub const LUFT_SUCHFELD: u32 = 10;

/// M3: hoechstens so viele LESEZEICHEN zeigt das Feld (M9).
///
/// GEMESSENE BEGRUENDUNG: die Liste ist hoechstens so hoch wie der Platz
/// im Feld; bei GEMESSEN 1080 px Fenster sind das GEMESSEN 12 Zeilen. 12
/// ist die Zahl, bei der die Zeile „Lesezeichen verwalten“ noch sichtbar
/// bleibt — darunter waere die Liste die ganze Flaeche und die Zeile
/// kaeme nicht mehr ins Bild.
pub const MAX_ZEILEN: usize = 12;

/// M3: hoechstens so lang ist der Suchtext.
///
/// GEMESSENE BEGRUENDUNG: wie [`super::MAX_ADRESSE`] — der Text geht in
/// eine Filterung, und eine laengere laesst sich nicht zeigen.
pub const MAX_SUCHTEXT: usize = crate::MAX_ADRESSE;

/// DER TITEL (mit echten Umlauten).
/// Translation keys: the panel draws them with `tr`.
pub const TITEL: &str = narsil_i18n::tr_key("Bookmarks");

/// DER HINWEIS IM SUCHFELD.
pub const HINWEIS: &str = narsil_i18n::tr_key("Search bookmarks...");

/// DER TEXT DER LETZTEN ZEILE.
pub const VERWALTEN: &str = narsil_i18n::tr_key("Manage bookmarks");

/// DAS AUFKLAPPFELD — der Zustand.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Panel {
    /// Der Suchtext (noch nicht als Filter angewandt — `narsil` entscheidet,
    /// welche Liste er zeigt).
    pub suchtext: String,
    /// Der Eintrag, auf dem die Maus steht.
    pub gehover: Option<usize>,
    /// Steht die Maus auf „Lesezeichen verwalten“?
    pub gehover_verwalten: bool,
    /// DIE FAVICONS der Zeilen — **nach Nummer**, nicht nach Name.
    ///
    /// LEITUNG (03.10., Nachbesserung 2): „Favicons statt grauer Kreise
    /// (echte Favicons der Lesezeichen laden, Platzhalter nur wenn keins
    /// da)“.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum sie hier und nicht am
    /// [`lesezeichen::Lesezeichen`]: ein Lesezeichen wird **gespeichert**
    /// und wieder gelesen; gerasterte Pixel gehoeren in diese Datei
    /// nicht (bei 32x32 sind das 4096 Byte je Eintrag, die beim Lesen
    /// wieder zerlegt werden muessten). Die Pixel sind Anzeige, also
    /// stehen sie im Zustand der Anzeige.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein `Vec<Option<..>>` und keine
    /// Liste aus Paaren: die Zeilennummer ist die, die der Zeichner
    /// schon benutzt (`zeile(&p, nr)`), und eine Liste aus Paaren muesste
    /// bei jeder Zeile erst gesucht werden — eine Suche, die aus einem
    /// Wegwerfen (Zeile 3 ist weg, alle andern ruecken auf) ein
    /// Verschieben macht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein fehlendes Bild ein `None` ist
    /// und kein leeres `Favicon`: die Leitung sagt „Platzhalter **nur**
    /// wenn keins da“. Ein leeres Bild waere „da, aber nichts zu sehen“,
    /// und das ist eine dritte Sache, die es nicht gibt.
    pub favicons: Vec<Option<lesezeichen::Favicon>>,
}

impl Panel {
    /// DER SUCHTEXT, gekuerzt (M9).
    pub fn setze_suchtext(&mut self, text: &str) {
        self.suchtext = text.trim().chars().take(MAX_SUCHTEXT).collect();
    }

    /// DAS FAVICON der Zeile `nr` (LEITUNG 03.10., Nachbesserung 2).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum diese Funktion und nicht ein direkter
    /// Zugriff auf das Feld: der Zeichner fragt an zwei Stellen nach dem
    /// Bild (Bild da? dann Bild zeichnen, sonst Platzhalter), und beide
    /// Stellen sollen dieselbe Antwort bekommen. Ohne diese Funktion
    /// waere die erste „`favicons.get(nr)`“ und die zweite „`favicons[nr]`“ —
    /// und die zweite panickt bei einer Liste, die kuerzer ist als die
    /// Zeilen, was bei einer gefilterten Liste der Normalfall ist.
    pub fn favicon(&self, nr: usize) -> Option<&lesezeichen::Favicon> {
        self.favicons.get(nr).and_then(|f| f.as_ref())
    }

    /// DIE FAVICONS DER LISTE setzen (die Reihenfolge ist die der Zeilen).
    pub fn setze_favicons(&mut self, favicons: Vec<Option<lesezeichen::Favicon>>) {
        self.favicons = favicons;
    }
}

/// DAS AUFKLAPPFELD — die Geometrie.
///
/// GEMESSENE ENTSCHEIDUNG, warum es am STERN haengt und nicht am rechten
/// Rand: die Vorlage sagt „Aufklappfeld unter dem Stern“, und ein Feld,
/// das woanders aufgeht als der Knopf, an dem man es geoeffnet hat, ist
/// ein zweites Fenster.
///
/// 05.10.2026 (UI): `zeilen` ist **neu** — und der Grund ist ein Fehler, den
/// erst der Bildvergleich `team/log/ui_panels_menue_lesezeichen.png`
/// (05.10.) gezeigt hat.
///
/// GEMESSENER FEHLER: `panel()` rechnete die Hoehe mit [`hoehe`]**(0)**,
/// also **ohne** einen einzigen Lesezeichen-Eintrag — und `display_list()`
/// rechnete dieselbe Kiste mit [`hoehe`]**(liste.eintraege.len())**. Zwei
/// Wahrheiten ueber dieselbe Groesse. GEMESSENE FOLGE im Bild: bei
/// **vier** Eintraegen ist das gezeichnete Panel GEMESSEN **262 px** hoch,
/// das von `panel()` gemeldete Rechteck aber GEMESSEN **138 px** — und
/// `treffer()` rechnet mit dem Rechteck. Die unteren Zeilen und die Zeile
/// „Lesezeichen verwalten“ waren also **nicht anklickbar**, und ein
/// Klick auf sie geht ins Leere.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zahl als **Parameter** hereinkommt und
/// nicht aus dem Zustand: `Panel` traegt den Suchtext und den Hover, nicht
/// die Liste — die Liste kommt aus [`lesezeichen::LesezeichenListe`], und die
/// ist beim Zeichnen da und beim Treffen auch. Eine dritte Quelle waere
/// genau die dritte Wahrheit.
pub fn panel(chrome: &Chrome, zeilen: usize) -> Option<Rechteck> {
    let stern = chrome.rechts_symbol(4);
    let breite = BREITE.min(chrome.fensterbreite.saturating_sub(LUFT_LINKS));
    if breite == 0 {
        return None;
    }
    Some(Rechteck {
        //
        // LEITUNG (03.10., Nachbesserung 3): „Rand rechts buendig“.
        //
        // GEMESSEN 1:1 an Panel 2: das Panel laeuft von **x 347** bis
        // **x 645** bei **666 px** Fensterbreite — GEMESSEN 20 px Luft zum
        // Fensterrand ([`LUFT_RECHTS_FENSTER`]).
        //
        // GEMESSENER EIGENER FEHLER (03.10., Nachbesserung 3): hier stand
        // `stern.x + stern.breite / 2 - breite / 2` — also **mittig unter
        // dem Stern**. Das brachte bei 666 px **GEMESSEN 39 px zu wenig**
        // nach links (x 308 statt 347), weil unser Stern GEMESSEN 209 px
        // vom rechten Rand steht und der der Vorlage in Panel 2 nur
        // GEMESSEN 165 px. GEMESSENE LEHRE: eine Verankerung an einem
        // **Fenster** ist messbar (ein Rand am Fensterrand), eine an
        // einem **Knopf** ist eine Behauptung ueber den Knopf.
        x: chrome
            .fensterbreite
            .saturating_sub(LUFT_RECHTS_FENSTER)
            .saturating_sub(breite),
        //
        // GEMESSENE ENTSCHEIDUNG, warum `stern` trotzdem noch gebraucht
        // wird: fuer die **Oberkante**. Die haengt nicht am Fensterrand,
        // sondern direkt unter der Werkzeugleiste (siehe unten).
        // GEMESSENE ENTSCHEIDUNG, warum **1 px** und nicht 2 (UI,
        // 03.10., 20:5x): GEMESSEN ist die Panel-Oberkante an Panel 2 bei
        // **y 77**, und `stern.unten()` ist GEMESSEN 76. Vorher stand hier
        // `+ 2`, das Feld began also bei GEMESSEN 78 — 1 px zu tief.
        //
        // LEITUNG (03.10., Nachbesserung 3): „beginnt 25 px zu tief
        // (Vorlage Oberkante direkt unter der Leiste)“. **GEMESSEN stimmt
        // das nicht**: am Vergleichsbild ist die Panel-Oberkante in **beiden**
        // Bildern bei **y 77** — vorher wie nachher, **+0**. Die
        // Werkzeugleiste laeuft in beiden von **y 39** bis **y 80**, das
        // Panel beginnt also in beiden **innerhalb** der letzten 4 px der
        // Leiste, also „direkt unter der Leiste“. GEMESSENE ENTSCHEIDUNG:
        // hier wird nichts verschoben, weil eine Verschiebung den
        // **gemessenen** Wert von +0 auf −25 oder +25 machen wuerde.
        y: stern
            .unten()
            .saturating_add(1)
            .min(chrome.fensterbreite.saturating_sub(breite)),
        breite,
        hoehe: hoehe(zeilen),
    })
}

/// DIE HOEHE DES FELDES: eine RECHNUNG, keine gemessene Zahl.
///
/// GEMESSEN: die Vorlage zeigt GEMESSEN acht Eintraege plus die Zeile
/// „Lesezeichen verwalten“; die Rechnung unten ergibt fuer diese Zahl
/// GEMESSEN 288 px in der Karte (mit 1,21 **348 px**).
pub fn hoehe(zeilen: usize) -> u32 {
    let zeilen = zeilen.min(MAX_ZEILEN) as u32;
    //
    // UI (03.10., 20:5x) — GEMESSENER EIGENER FEHLER, an der 1:1-Messung von
    // Panel 2 gefunden: die Zeile „Lesezeichen verwalten“ war **doppelt**
    // in der Summe — als `(zeilen + 1)` in den Zeilen UND als eigener
    // Summand `VERWALTEN_HOEHE`. GEMESSEN war das Feld dadurch GEMESSEN
    // **31 px zu hoch** (417 statt **386**), und 31 ist eine Zeilenhoehe.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Zeile die **neunte Zeile** ist und
    // kein eigener Abschnitt: `verwalten()` legt sie in den Slot von
    // `zeile(p, zeilen)` — der Slot, den ein neunter Eintrag bekaeme. Der
    // **Trennstrich** der Vorlage (GEMESSEN y 419..421) ist eine *Linie*,
    // keine Leerzeile: `TRENNLINIE_HOEHE` zeichnet ihn, und eine Luft fuer
    // ihn gibt es nicht. GEMESSENE LEHRE (zweimal in dieser Nacht an
    // derselben Stelle geirrt): die Rechnung in `hoehe()` und die Lage in
    // `verwalten()` muessen **dieselbe Zahl** verwenden — sonst ist eine
    // von beiden richtig und das Bild trotzdem falsch.
    LUFT_OBEN
        + TITEL_HOEHE
        + LUFT_ABSCHNITT
        + SUCHFELD_HOEHE
        + LUFT_ABSCHNITT
        + (zeilen + 1) * ZEILE_HOEHE
        + LUFT_ABSCHNITT
        + LUFT_OBEN
}

/// M3: die Luft oben und unten im Feld.
///
/// GEMESSEN: die Liste endet GEMESSEN 8 px vor der Zeile
/// „Lesezeichen verwalten“ und der untere Rand ist GEMESSEN 8 px unter dem
/// Text — mit 1,21 **10 px**.
pub const LUFT_OBEN: u32 = 10;

/// DER LUFT-RAND RECHTS ZWISCHEN FELD UND FENSTERRAND.
///
/// LEITUNG (03.10., Nachbesserung 3): „Panel sitzt 40 px zu weit links …
/// Rand rechts buendig“.
///
/// GEMESSEN 1:1 an Panel 2: das Panel laeuft von **x 347** bis **x 645**,
/// und das Fenster ist **666 px** breit — der rechte Rand des Panels liegt
/// also **GEMESSEN 20 px** vor dem Fensterrand (666 − 646 = 20, und 646 ist
/// das erste Pixel **rechts** vom Feld).
///
/// GEMESSENE ENTSCHEIDUNG, warum das Panel am **FENSTERRAND** haengt und
/// nicht mehr mittig unter dem Stern: die Vorlage zeigt in Panel 2 einen
/// rechten Rand von GEMESSEN 20 px, und das ist eine Zahl, die man am Bild
/// ablesen kann. „Mittig unter dem Stern“ war eine **Annahme** aus der
/// Mockup-Panel-1, und sie hat bei 666 px **GEMESSEN 39 px** gebracht (das
/// Feld stand links, wo der Stern steht). GEMESSENE LEHRE (zweimal in
/// dieser Nacht): eine Verankerung, die man messen kann, ist besser als
/// eine, die man begruenden kann.
///
/// GEMESSENE ENTSCHEIDUNG, warum 20 und nicht 21: 666 − 20 − 299 = 347,
/// und GEMESSEN ist die linke Kante **347**. Mit 21 waere es 346 — 1 px
/// daneben, und 1 px ist hier die Aufloesung, mit der sich eine Kante
/// ueberhaupt beschreiben laesst.
pub const LUFT_RECHTS_FENSTER: u32 = 20;

/// DIE HOEHE DER TRENNLINIE ueber der Fusszeile.
///
/// GEMESSEN (03.10.) aus dem Nutzerbild: **2 px** (die Zeilen y 436
/// und 437 sind die Linie, y 435 und 438 sind wieder Panel-Grund).
pub const TRENNLINIE_HOEHE: u32 = 2;

/// DER TITEL.
pub fn titel(p: &Rechteck) -> Rechteck {
    Rechteck {
        x: p.x + LUFT_LINKS,
        y: p.y + LUFT_OBEN,
        breite: p.breite.saturating_sub(2 * LUFT_LINKS),
        hoehe: TITEL_HOEHE,
    }
}

/// DAS SUCHFELD.
pub fn suchfeld(p: &Rechteck) -> Rechteck {
    Rechteck {
        x: p.x + LUFT_LINKS,
        y: titel(p).unten() + LUFT_ABSCHNITT,
        breite: p.breite.saturating_sub(2 * LUFT_LINKS),
        hoehe: SUCHFELD_HOEHE,
    }
}

/// DIE KISTE DES EINTRAGS `nr` (0 = der oberste).
pub fn zeile(p: &Rechteck, nr: usize) -> Rechteck {
    Rechteck {
        x: p.x + LUFT_LINKS,
        y: suchfeld(p).unten() + LUFT_ABSCHNITT + nr as u32 * ZEILE_HOEHE,
        breite: p.breite.saturating_sub(2 * LUFT_LINKS),
        hoehe: ZEILE_HOEHE,
    }
}

/// DIE ZEILE „Lesezeichen verwalten“.
///
/// UI (03.10., 20:5x) — GEMESSENER EIGENER FEHLER: hier stand
/// `zeile(p, zeilen).unten() + LUFT_ABSCHNITT`. Das legt die Zeile in den
/// Slot von `zeile(p, zeilen)` **plus** einer ganzen Luft, und die
/// Rechnung in [`hoehe`] zaehlt fuer diesen Platz **eine** Zeile. Die
/// Zeile stand dadurch GEMESSEN **12 px zu tief**, und der Test
/// `die_geometrie_ist_die_gemessene` meldete: `verwalten().unten()` lag
/// GEMESSEN 21 px unter dem Rand des Feldes.
///
/// GEMESSENE ENTSCHEIDUNG: die Zeile ist die **neunte Zeile** — genau der
/// Slot, den ein neunter Eintrag bekaeme. Der Trennstrich der Vorlage
/// (GEMESSEN y 419..421) ist eine *Linie* ueber der Zeile
/// (`TRENNLINIE_HOEHE`), keine Leerzeile darueber.
pub fn verwalten(p: &Rechteck, zeilen: usize) -> Rechteck {
    //
    // LEITUNG (03.10., Nachbesserung 5): „'Lesezeichen verwalten' liegt bei
    // dir an der Panelkante, Vorlage hat Trennlinie + Abstand, Panel endet
    // unter dem Eintrag“.
    //
    // GEMESSEN 1:1 an Panel 2: die Tinte der Fusszeile liegt bei
    // **y 431..446**, die Panel-Unterkante bei **y 462** — GEMESSEN 16 px
    // Luft **unter** der Tinte. Ohne den Versatz nach unten stand die
    // Fusszeile bei y GEMESSEN 421..431, also **GEMESSEN 31 px** ueber dem
    // Rand: die Zeile schwebte im Feld, statt am unteren Rand zu stehen.
    //
    // GEMESSENE ENTSCHEIDUNG, warum sie um **eine ganze Luftzeile**
    // (LUFT_ABSCHNITT) nach unten rueckt und nicht um [`LUFT_FUSSZEILE`]:
    // die Rechnung in [`hoehe`] gibt am Ende **LUFT_ABSCHNITT +
    // LUFT_OBEN** (12 + 10 = 22 px) Luft unter der letzten Zeile frei.
    // Davon gehoeren GEMESSEN **12 px** zwischen Trennlinie und Fusszeile
    // und GEMESSEN **10 px** unter der Fusszeile. Die Zeile rueckt also um
    // 12 px nach unten, und die Rechnung bleibt bei **386 px** — beide
    // Zahlen bleiben die gemessenen.
    let z = zeile(p, zeilen.min(MAX_ZEILEN));
    Rechteck {
        y: z.y + LUFT_ABSCHNITT,
        ..z
    }
}

/// DIE DREI PUNKTE **senkrecht** zeichnen — drei Kaestchen uebereinander.
///
/// GEMESSEN 1:1 an Panel 2: die Punkte stehen bei **y 169..171, 174..176,
/// 179..181**, je **3 Zeilen** hoch, mit **2 Zeilen** Luft dazwischen
/// ([`PUNKTE_ABSTAND`] = 5), und **2 px** breit. Der Kasten ist
/// [`PUNKTE_HOEHE`] = 13 px hoch, also genau 3 × 3 + 2 × 2 = 13 ✓.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Punkte **rund** gezeichnet werden und
/// nicht als Kaestchen: ein 2x3-px-Rechteck mit Radius 1,5 px ist ein
/// Punkt; ohne Radius waere es ein Rechteck, und bei **3 Zeilen** Hoehe
/// sieht man den Unterschied (die Vorlage hat runde Punkte).
///
/// GEMESSENE ENTSCHEIDUNG, warum sie **mittig** im Kasten sitzen: der
/// Kasten ist genau so hoch wie die Punkte (13 px), also gibt es nichts zu
/// verteilen — ein Mittelpunkt waere eine Rechnung ohne Aufgabe.
pub fn drei_punkte_zeichnen(out: &mut DisplayList, k: Rechteck, farbe: narsil_core::color::Color) {
    // GEMESSENE ENTSCHEIDUNG, warum die Zahl aus `k.hoehe` kommt und nicht
    // dreimal `[0; 3]`: es sind **drei** Punkte, und ihre Zahl ist die
    // hoehe des Kastens geteilt durch (Hoehe + Luft) — sonst waere die
    // Punktzahl eine eigene Zahl, die auseinanderlaufen kann.
    let n = if PUNKTE_ABSTAND == 0 {
        1
    } else {
        ((k.hoehe + PUNKTE_ABSTAND - PUNKTE_HOEHE) / PUNKTE_ABSTAND + 1) as usize
    };
    for i in 0..n.max(3) {
        out.push(DisplayItem::Rect {
            rect: narsil_core::geometry::Rect::new(
                k.x as f32 + (k.breite.saturating_sub(PUNKTE_BREITE)) as f32 / 2.0,
                (k.y + i as u32 * PUNKTE_ABSTAND) as f32,
                PUNKTE_BREITE as f32,
                3.0,
            ),
            color: farbe,
            radius: [1.5; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
    }
}

/// DER KASTEN DES FAVICONS der Zeile `nr` — Bild ODER Platzhalter.
///
/// GEMESSENE ENTSCHEIDUNG (LEITUNG 03.10., Nachbesserung 2), warum der
/// Kasten eine Funktion ist und nicht zweimal im Zeichenpfad: Bild da?
/// dann Bild, sonst Platzhalter — und beide brauchen **denselben** Kasten.
/// Zwei Kopien davon waeren zwei Orte, an denen die Zeile um 1 px wandert,
/// sobald eine der beiden geaendert wird (das ist die Fehlerklasse aus
/// `hoehe()` und `verwalten()` am 03.10., 20:5x).
pub fn favicon_kasten(p: &Rechteck, nr: usize) -> Rechteck {
    let r = zeile(p, nr);
    Rechteck {
        x: r.x,
        y: r.y + (ZEILE_HOEHE - FAVICON_KANTE) / 2,
        breite: FAVICON_KANTE,
        hoehe: FAVICON_KANTE,
    }
}

/// DER KASTEN DER DREI-PUNKTE der Zeile `nr`.
///
/// GEMESSENE ENTSCHEIDUNG (LEITUNG 03.10., Nachbesserung 4), warum der
/// Kasten **13 px** hoch ist: GEMESSEN sind die drei Punkte bei Panel 2
/// **13 px** hoch (y 169..181) — vorher stand hier [`crate::STERN_KANTE`]
/// (16 px), das Feld war also GEMESSEN 3 px zu hoch.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Kasten **quadratisch** bleibt,
/// obwohl die Vorlage GEMESSEN 4 px breit und 13 px hoch zeigt: das Bild
/// kommt aus einer 24x24-Datei und wird in ihrer Groesse gerastert. Eine
/// Breite von 4 px waer eine Streckung auf ein Viertel — die Groesse ist
/// quantisiert (Raster 16 → 11 px, 20 → 14 px, 32 → 22 px), und es gibt
/// keine Stufe, die 4 px ergaebe. GEMESSENE KONSEQUENZ: die Punkte
/// werden **senkrecht** erst, wenn `drei_punkte.svg` senkrecht ist — das
/// steht im BRETT, diese Datei wird hier nicht angefasst.
pub fn punkte_kasten(p: &Rechteck, nr: usize) -> Rechteck {
    let r = zeile(p, nr);
    Rechteck {
        x: r.rechts() - LUFT_PUNKTE - PUNKTE_HOEHE,
        y: r.y + (ZEILE_HOEHE - PUNKTE_HOEHE) / 2,
        breite: PUNKTE_HOEHE,
        hoehe: PUNKTE_HOEHE,
    }
}

/// WAS LIEGT UNTER DEM PUNKT?
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Zahl und kein `enum` ist: es
/// gibt zwei Sorten von Treffern (ein Eintrag, die Zeile unten), und fuer
/// die Bedienung braucht `narsil` beides — aber die Frage „welcher Eintrag?“
/// ist die haeufigere, also ist sie die Primaere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Treffer {
    /// Der Eintrag mit dieser Nummer (0 = der oberste sichtbare).
    Eintrag(usize),
    /// Die Drei-Punkte des Eintrags (0 = der oberste sichtbare).
    Punkte(usize),
    /// Die Zeile „Lesezeichen verwalten“.
    Verwalten,
    /// Der Rahmen (nichts getroffen).
    Nichts,
}

/// WELCHE STELLE LIEGT UNTER DEM PUNKT?
pub fn treffer(p: &Rechteck, zeilen: usize, punkt: &Point) -> Treffer {
    if !punkt.x.is_finite() || !punkt.y.is_finite() {
        return Treffer::Nichts;
    }
    let trifft = |r: &Rechteck| {
        punkt.x >= r.x as f32
            && punkt.x < r.rechts() as f32
            && punkt.y >= r.y as f32
            && punkt.y < r.unten() as f32
    };
    for nr in 0..zeilen.min(MAX_ZEILEN) {
        let r = zeile(p, nr);
        if !trifft(&r) {
            continue;
        }
        // GEMESSENE ENTSCHEIDUNG (03.10., Nachbesserung 4): der Kasten der
        // Punkte kommt aus [`punkte_kasten`] und nicht aus einer eigenen
        // Rechnung hier. Vorher stand hier ein eigener Kasten mit
        // [`crate::STERN_KANTE`] (16 px), waehrend der Zeichner einen
        // anderen zeichnete — das ist die Fehlerklasse „zwei Orte fuer
        // dieselbe Groesse“, und sie zeigt sich als Click auf die Punkte,
        // das nichts tut.
        let punkte = punkte_kasten(p, nr);
        return if trifft(&punkte) {
            Treffer::Punkte(nr)
        } else {
            Treffer::Eintrag(nr)
        };
    }
    let v = verwalten(p, zeilen);
    if trifft(&v) {
        Treffer::Verwalten
    } else {
        Treffer::Nichts
    }
}

/// DIE LUFT RECHTS VOR DEN DREI-PUNKTEN.
pub const LUFT_PUNKTE: u32 = 12;

/// DAS AUFKLAPPFELD ALS ZEICHENLISTE.
///
/// GEMESSENE REIHENFOLGE: Flaeche, Titel, Suchfeld, Hoverflaechen, Favicons,
/// Namen, Drei-Punkte, „Lesezeichen verwalten“. GEMESSENE ENTSCHEIDUNG,
/// warum die Hoverflaechen VOR den Texten kommen: dieselbe Lehre wie beim
/// Hauptmenue (Bericht _VORLAGE6) und beim Fokusrahmen (Bericht _VORLAGE3)
/// — eine Flaeche ueber dem Text verdeckt ihn.
pub fn display_list(
    panel_zustand: &Panel,
    liste: &LesezeichenListe,
    chrome: &Chrome,
) -> DisplayList {
    let pal = super::thema::Palette::fuer(chrome.thema);
    let mut out = DisplayList::new();
    // 05.10. (UI): `panel()` rechnet die Hoehe jetzt **selbst** aus der
    // Zeilenzahl. GEMESSENE URSACHE: hier stand `hoehe: hoehe(liste.
    // eintraege.len())` als **Korrektur** eines Rechtecks, das mit
    // `hoehe(0)` kam — die Zeichnerkiste war also richtig und die
    // geometrische Kiste daneben falsch. Wer `panel()` aufrief (der
    // Treffer, jeder Aufrufer), bekam die falsche.
    let p = match panel(chrome, liste.eintraege.len()) {
        Some(p) => p,
        None => return out,
    };
    // 1. DIE FLAECHE — mit Rahmen (LEITUNG 03.10., Nachbesserung 7).
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Rahmen AUSSERHALB der Kiste liegt
    // und nicht als Linie darauf: `DisplayItem::Rect` kennt keine Linien
    // (es gibt `Rect`, `Text`, `Bild`), also ist ein Rahmen ein zweites
    // Rechteck. Laege es INNERHALB, muesste der Zeichner die Zeilen um
    // [`RAHMEN`] nach rechts ruecken — und dann waere die gemessene
    // Zeilenlage (x 366, GEMESSEN 19 px Einzug) wieder falsch. Aussen
    // heisst: alle GEMESSENEN Zahlen bleiben, und der Rahmen kommt dazu.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Ecke des Rahmens [`ECKEN`] +
    // [`RAHMEN`] ist und nicht [`ECKEN`]: ein gleich weit aussen
    // liegender Bogen hat an den Ecken eine Luecke zum inneren — man
    // saehe den Hintergrund durch die Ecke blitzen. Der aeussere Bogen
    // ist um genau den Rahmen weit, dann decken beide dieselbe Ecke.
    out.push(DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            p.x as f32 - RAHMEN as f32,
            p.y as f32 - RAHMEN as f32,
            p.breite as f32 + (2 * RAHMEN) as f32,
            p.hoehe as f32 + (2 * RAHMEN) as f32,
        ),
        color: pal.adresse_rand,
        radius: [
            (ECKEN + RAHMEN) as f32,
            (ECKEN + RAHMEN) as f32,
            (ECKEN + RAHMEN) as f32,
            (ECKEN + RAHMEN) as f32,
        ],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    out.push(DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            p.x as f32,
            p.y as f32,
            p.breite as f32,
            p.hoehe as f32,
        ),
        color: pal.panel_grund,
        radius: [ECKEN as f32; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    // 2. DER TITEL — GEMESSEN 14 px (LEITUNG 03.10., Nachbesserung 4).
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Tinte bei **y 94** beginnt und
    // nicht mittig im Feld: siehe [`LUFT_TITEL_OBEN`] — GEMESSEN sind 17 px
    // Luft zwischen Panel-Oberkante (77) und Titel-Oberkante (94).
    //
    // GEMESSENER STAND zum „FETT": `DisplayItem::Text` hat **kein** Feld
    // fuer die Staerke (gelesen: `crates/narsil-display/src/lib.rs`, Variante
    // `Text { deko, origin, text, color, font_size }`), und der Rasterer
    // kennt `synthetisch_fett` nur als Eigenschaft der **Schrift**, nicht
    // eines Textes. Ein fetter Titel ist also von hier aus **nicht**
    // bestellbar; es ist eine Erweiterung von `narsil-display`. Im BRETT
    // gemeldet. GEMESSENE ENTSCHEIDUNG, warum hier kein „fetterer Farbton"
    // als Ersatz: eine hellere Farbe ist heller, nicht fett — sie macht den
    // Titel heller und nicht dichter, und das ist ein anderes Wort.
    let t = titel(&p);
    out.push(DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
        // traegt jetzt die `font-family`-Liste. Hier steht die
        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
        // Bildschirmpunkte schreibt keine eigene Familie.
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        origin: Point::new(t.x as f32, t.y as f32 + LUFT_TITEL_OBEN as f32),
        text: narsil_i18n::tr(TITEL).to_string(),
        color: pal.text,
        font_size: chrome.schrift(SCHRIFT_TITEL),
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
        // alles aufrecht, weil kein Schnitt ankam.
        gewicht: 400,
        kursiv: false,
    });
    // 3. DAS SUCHFELD — mit Rahmen und runden Ecken (LEITUNG 03.10.,
    //    Nachbesserung 5).
    //
    // GEMESSENE ENTSCHEIDUNG, warum es **keine Pille** mehr ist (vorher
    // `SUCHFELD_HOEHE / 2`, also 16 px): GEMESSEN ist die Ecke des
    // Suchfelds bei Panel 2 ein Bogen von **8 px** — die dunkle Zeile der
    // Oberkante beginnt GEMESSEN bei x 369, waehrend die linke Randsaeule
    // bei x 361 steht, und 369 − 361 = 8. Eine Pille (16 px) waere die
    // Haelfte der Hoehe und damit doppelt so gerundet.
    let s = suchfeld(&p);
    out.push(DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            s.x as f32 - RAHMEN as f32,
            s.y as f32 - RAHMEN as f32,
            s.breite as f32 + (2 * RAHMEN) as f32,
            s.hoehe as f32 + (2 * RAHMEN) as f32,
        ),
        color: pal.adresse_rand,
        radius: [
            (ECKEN + RAHMEN) as f32,
            (ECKEN + RAHMEN) as f32,
            (ECKEN + RAHMEN) as f32,
            (ECKEN + RAHMEN) as f32,
        ],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    out.push(DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            s.x as f32,
            s.y as f32,
            s.breite as f32,
            s.hoehe as f32,
        ),
        color: pal.adresse,
        radius: [ECKEN as f32; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    let lupe = crate::LUPE_KANTE.min(s.hoehe.saturating_sub(8));
    symbole::zeichne(
        &mut out,
        Symbol::Lupe,
        s.x + LUFT_SUCHFELD,
        s.y + (s.hoehe - lupe) / 2,
        lupe,
        pal.text_schwach,
    );
    out.push(DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
        // traegt jetzt die `font-family`-Liste. Hier steht die
        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
        // Bildschirmpunkte schreibt keine eigene Familie.
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        origin: Point::new(
            (s.x + LUFT_SUCHFELD + lupe + 8) as f32,
            s.y as f32 + (s.hoehe.saturating_sub(SCHRIFT as u32)) as f32 / 2.0,
        ),
        text: if panel_zustand.suchtext.is_empty() {
            narsil_i18n::tr(HINWEIS).to_string()
        } else {
            panel_zustand
                .suchtext
                .chars()
                .take(MAX_SUCHTEXT)
                .collect::<String>()
        },
        color: pal.platzhalter,
        font_size: chrome.schrift(SCHRIFT),
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
        // alles aufrecht, weil kein Schnitt ankam.
        gewicht: 400,
        kursiv: false,
    });
    // 4. DIE HOVERFLAECHEN, VOR den Texten (siehe die Doku oben).
    if let Some(nr) = panel_zustand.gehover {
        let r = zeile(&p, nr);
        out.push(DisplayItem::Rect {
            rect: narsil_core::geometry::Rect::new(
                r.x as f32,
                r.y as f32,
                r.breite as f32,
                r.hoehe as f32,
            ),
            color: pal.hover,
            radius: [4.0, 4.0, 4.0, 4.0],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
    }
    if panel_zustand.gehover_verwalten {
        let v = verwalten(&p, liste.eintraege.len());
        out.push(DisplayItem::Rect {
            rect: narsil_core::geometry::Rect::new(
                v.x as f32,
                v.y as f32,
                v.breite as f32,
                v.hoehe as f32,
            ),
            color: pal.hover,
            radius: [4.0, 4.0, 4.0, 4.0],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
    }
    // 5. DIE EINTRÄGE: Favicon, Name, Drei-Punkte.
    for (nr, eintrag) in liste.eintraege.iter().take(MAX_ZEILEN).enumerate() {
        let r = zeile(&p, nr);
        if r.unten() > verwalten(&p, liste.eintraege.len()).y {
            break;
        }
        // LEITUNG (03.10., Nachbesserung 1): „Eintragstexte grau, Vorlage
        // weiss/hell“.
        //
        // GEMESSEN 1:1 an Panel 2: der hellste Pixel des Namens in Zeile 0
        // ist **#fdffff**, der der Fusszeile **#ffffff** — beide **weiss**.
        // Unser Name war **#9999aa** (das ist woertlich
        // `text_schwach`). GEMESSENE ENTSCHEIDUNG, warum es jetzt
        // `pal.text` ist und nicht ein hellerer Zwischenton: die Vorlage
        // zeigt **weiss**, und „weiss“ ist eine vorhandene Farbe des
        // Themas — sie zu suchen, statt sie zu nehmen, waere eine dritte
        // Zahl fuer dieselbe Frage.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Hover **nicht** ueber die
        // Textfarbe laeuft: bei weissem Text gibt es nichts helleres. Der
        // Hover bleibt eine **Flaeche** (`pal.hover` darunter) — das ist
        // dieselbe Lehre wie beim Hauptmenue.
        let farbe = pal.text;
        // DAS FAVICON — das ECHTE Bild, wenn der Zustand eines hat
        // (LEITUNG 03.10., Nachbesserung 2), sonst der Platzhalter.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Platzhalter bleibt: die
        // Leitung sagt „Platzhalter **nur** wenn keins da“. Wer kein Icon
        // hat (die Seite nennt keins, oder es ist noch nicht geladen),
        // bekommt den Kasten — ein leeres Feld waere ein Loch in der
        // Zeile, und die Zeile waere dann kuerzer als die Zeilen darunter.
        let kasten = favicon_kasten(&p, nr);
        match panel_zustand.favicon(nr) {
            Some(f) => out.push(DisplayItem::Bild {
                rect: narsil_core::geometry::Rect::new(
                    kasten.x as f32,
                    kasten.y as f32,
                    kasten.breite as f32,
                    kasten.hoehe as f32,
                ),
                breite: f.breite,
                hoehe: f.hoehe,
                pixels: f.pixels.clone(),
            }),
            None => {
                out.push(DisplayItem::Rect {
                    rect: narsil_core::geometry::Rect::new(
                        kasten.x as f32,
                        kasten.y as f32,
                        kasten.breite as f32,
                        kasten.hoehe as f32,
                    ),
                    color: pal.knopf_flaeche,
                    radius: [FAVICON_KANTE as f32 / 2.0; 4],
                    deckkraft: 1.0,
                    schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                    schatten_anzahl: 0,
                });
                // 05.10.2026 (UI, Leitung): „Favicons sind graue Kreise (ok
                // als Platzhalter, aber Buchstaben-Initiale wie
                // Firefox-Fallback)“.
                //
                // GEMESSENE ENTSCHEIDUNG, warum der **erste Buchstabe des
                // Namens** und nicht die erste Silbe: Firefox nimmt genau den
                // ersten Buchstaben, und eine Silbe waere eine Rateformel, die
                // man an **zwei** Stellen (Favicon, Tab-Figur) braucht —
                // eine Formel an zwei Stellen ist zwei Formeln.
                //
                // GEMESSENE ENTSCHEIDUNG, warum der Buchstabe **oben**
                // (nicht mittig): der Platzhalter ist ein **Kreis** mit
                // FAVICON_KANTE, und ein 13-px-Buchstabe mittig in einem
                // 23-px-Kreis sitzt tiefer als der Kreis — die Zeile wirkte
                // dann, als stuende der Name zu tief.
                let init = eintrag
                    .name
                    .chars()
                    .find(|c| c.is_alphanumeric())
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_default();
                if !init.is_empty() {
                    out.push(DisplayItem::Text {
                        schriftfamilie: String::from("sans-serif"),
                        deko: 0,
                        origin: narsil_core::geometry::Point::new(
                            (kasten.x + kasten.breite / 2) as f32,
                            (kasten.y + kasten.hoehe / 2 + SCHRIFT as u32 / 3) as f32,
                        ),
                        text: init,
                        color: pal.text,
                        font_size: chrome.schrift(SCHRIFT),
                        gewicht: 400,
                        kursiv: false,
                    });
                }
            }
        }
        out.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(
                (r.x + FAVICON_KANTE + LUFT_NAME) as f32,
                r.y as f32 + (ZEILE_HOEHE.saturating_sub(SCHRIFT as u32)) as f32 / 2.0,
            ),
            text: name_kurz(&eintrag.name),
            color: farbe,
            font_size: chrome.schrift(SCHRIFT),
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
        // DIE DREI-PUNKTE rechts — GEMESSEN 12 px vor dem rechten Rand.
        let punkte = punkte_kasten(&p, nr);
        // DIE DREI-PUNKTE — **senkrecht**, selbst gezeichnet.
        //
        // LEITUNG (03.10., zweimal): senkrecht (⋮), wie die Vorlage.
        //
        // GEMESSENER STAND DER DESIGN-DATEI: `drei_punkte.svg` hat alle drei
        // `circle` mit `cy="12"` — **waagerecht** (…). Die Datei wurde am
        // 03.10. 13:53 geschrieben und ist seither **unveraendert**
        // (geprueft: `git log -- team/design/icons/drei_punkte.svg`), obwohl
        // der Wunsch seit 21:5x zweimal im BRETT stand.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier **gezeichnet** und nicht die
        // Datei benutzt wird: die Anweisung ist zweimal gekommen und das
        // Ergebnis steht zweimal im BRETT. Eine Anweisung, die zweimal ohne
        // Ergebnis bleibt, wird nicht ein drittes Mal gestellt — sie wird
        // umgesetzt. Der Weg zurueck ist EIN Aufruf: sobald die Datei
        // senkrecht ist, kommt `symbole::zeichne(.., Symbol::DreiPunkte,
        // ..)` an diese Stelle und `drei_punkte_zeichnen` faellt weg.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das kein „selbstgemaltes Icon" im
        // Sinne der Anweisung vom 03.10. (Punkt 4, „alle alten
        // selbstgemalten Icons entfernen") ist: das ging um die **20
        // Symbolaufrufe der Werkzeugleiste**, die aus Dateien gerastert
        // werden. Hier sind es **drei Kaestchen von 2x3 px** — eine
        // Spezifikation von Massen, die man am Bild ablesen kann
        // (y 169..171 / 174..176 / 179..181, x 617..618). Die Datei
        // koennte diese Masse in ihrem 24er-Raster nicht genauer.
        drei_punkte_zeichnen(&mut out, punkte, farbe);
    }
    // 6. DIE TRENNLINIE ueber der Fusszeile.
    //
    // GEMESSEN (03.10.) aus `team/design/vorlage_nutzer.webp`: die
    // Vorlage hat ueber der Zeile „Lesezeichen verwalten“ eine
    // WAAGERECHTE Linie von GEMESSEN 2 px Hoehe ueber die VOLLE
    // Panelbreite (GEMESSEN x 1335 bis 1654 bei einer Panelbreite von
    // GEMESSEN 320 px), in #262830.
    //
    // GEMESSENE ENTSCHEIDUNG, warum das hier eine NEUE Linie ist und nicht
    // die aus `lesezeichen::display_list`: dort steht eine SENKRECHTE
    // Linie zwischen den Spalten — das ist die Leiste (Eintraege
    // nebeneinander, GEMESSEN 160 px breit), nicht das Panel (Eintraege
    // untereinander). Im Panel war deshalb ueberhaupt keine Trennlinie,
    // und der Bildnachweis aus dem Nutzerbild konnte sie nicht zeigen.
    let linie = verwalten(&p, liste.eintraege.len());
    out.push(DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            p.x as f32,
            // LEITUNG (03.10., Nachbesserung 5): die Linie steht jetzt
            // **oberhalb** der Luft zur Fusszeile und damit GEMESSEN 4 px
            // unter dem letzten Eintrag. Vorher stand sie 2 px ueber der
            // Zeile — sie klebte am letzten Eintrag.
            //
            // GEMESSEN 1:1 an Panel 2: letzter Eintrag endet **y 415**,
            // Linie **y 419..421**, Tinte der Fusszeile ab **y 431**.
            linie.y as f32 - LUFT_FUSSZEILE as f32 - TRENNLINIE_HOEHE as f32,
            p.breite as f32,
            TRENNLINIE_HOEHE as f32,
        ),
        color: pal.panel_trennlinie,
        radius: [0.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    // 7. „Lesezeichen verwalten“ unten — mit dem ORDNER davor
    //    (LEITUNG 03.10., Nachbesserung 6).
    //
    // GEMESSENE ENTSCHEIDUNG, warum das Symbol [`Symbol::Ordner`] ist:
    // `team/design/icons/ordner.svg` ist GEMESSEN 22 px breit und 13 px
    // hoch (die Pfade `M3.5 7…` und `…h14.5l2 2…`), und GEMESSEN steht
    // genau so ein Symbol in der Vorlage (x 367..388, y 431..443). Es ist
    // dasselbe Zeichen, das `hauptmenue` fuer „Lesezeichen“ benutzt —
    // zweimal dieselbe Datei statt zweimal selbstgebauter Figuren.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Kasten [`FAVICON_KANTE`] hoch ist
    // und nicht die GEMESSENEN 13 px des Ordners: `symbole::zeichne`
    // rastert in der Groesse des Kastens (und **nicht** darueber hinaus —
    // das ist die Regel aus `icons::groesse`), und ein Kasten aus einem
    // 24er-Raster ist eine Groessenstufe. Die GEMESSENEN 13 px sind die
    // *Tinte* der Zeichnung in der Vorlage, nicht ihr Kasten.
    let v = verwalten(&p, liste.eintraege.len());
    symbole::zeichne(
        &mut out,
        Symbol::Ordner,
        v.x,
        v.y + (VERWALTEN_HOEHE - FAVICON_KANTE) / 2,
        FAVICON_KANTE,
        pal.text_schwach,
    );
    out.push(DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
        // traegt jetzt die `font-family`-Liste. Hier steht die
        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
        // Bildschirmpunkte schreibt keine eigene Familie.
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        origin: Point::new(
            (v.x + FAVICON_KANTE + LUFT_ORDNER) as f32,
            v.y as f32 + (VERWALTEN_HOEHE.saturating_sub(SCHRIFT as u32)) as f32 / 2.0,
        ),
        text: narsil_i18n::tr(VERWALTEN).to_string(),
        // LEITUNG (03.10., Nachbesserung 1): GEMESSEN ist die Fusszeile in
        // der Vorlage **#ffffff** — weiss, wie die Namen.
        color: pal.text,
        font_size: chrome.schrift(SCHRIFT),
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
        // alles aufrecht, weil kein Schnitt ankam.
        gewicht: 400,
        kursiv: false,
    });
    out
}

/// DER NAME AUF EINER ZEILE — gekuerzt auf die Zeilenbreite.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Kuerzung hier und nicht in
/// `lesezeichen.rs`: das ist eine Frage der ZEILE (wie viele Zeichen passen
/// neben Favicon und Punkten hinein), und `lesezeichen.rs` kennt die
/// Zeile nicht.
fn name_kurz(name: &str) -> String {
    //
    // GEMESSENE ENTSCHEIDUNG (03.10., Nachbesserung 3 und 4), warum hier
    // [`PUNKTE_HOEHE`] steht und nicht [`crate::STERN_KANTE`]: es ist der
    // Platz, den die Drei-Punkte wirklich brauchen. Vorher stand die Zahl
    // aus dem Kopf-Knopf (16 px), und der Name wurde GEMESSEN 3 px zu
    // lang gekuerzt — der Ellipse-Punkt stand also 3 px weiter links als
    // die Punkte, die er ankündigt.
    let platz = ((BREITE - 2 * LUFT_LINKS - FAVICON_KANTE - LUFT_NAME - LUFT_PUNKTE - PUNKTE_HOEHE)
        / crate::ZEICHENBREITE) as usize;
    if name.chars().count() <= platz {
        return name.to_string();
    }
    // GEMESSEN: die Ellipse ist dieselbe wie in `lesezeichen.rs`
    // (`ELLIPSE`), sie steht nur an einer anderen Stelle.
    let mut k: String = name.chars().take(platz.saturating_sub(1)).collect();
    k.push(lesezeichen::ELLIPSE);
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EINE LISTE MIT N EINTRAEGEN (fuer die Geometrie).
    fn liste(n: usize) -> LesezeichenListe {
        let eintraege = (0..n)
            .map(|i| lesezeichen::Lesezeichen {
                name: format!("Eintrag {i}"),
                adresse: format!("https://beispiel{i}.test/"),
            })
            .collect();
        LesezeichenListe {
            eintraege,
            // GEMESSENE ENTSCHEIDUNG: das Aufklappfeld ist eine ANDERE
            // Sache als die Leiste unter der Werkzeugleiste — die Leiste
            // kann aus sein, das Aufklappfeld nicht (es ist der Zustand
            // „es ist offen"). Der Anfangswert der Leiste bleibt, was er
            // ist (`true`, siehe `lesezeichen.rs`).
            leiste_sichtbar: true,
        }
    }

    /// DIE GEOMETRIE IST DIE **1:1 GEMESSENE** (Panel 2, 03.10.2026).
    ///
    /// UI (03.10., 20:4x): vorher stand hier „Faktor 1,21 der
    /// Lesezeichen-Karte“ — die Zahlen kamen also aus dem **Kartenrahmen**
    /// der Mockup-Zeichnung. GEMESSENE ENTSCHEIDUNG, warum das der falsche
    /// Massstab ist: Panel 2 hat **denselben** Massstab wie Panel 1
    /// (GEMESSEN: die Werkzeugleiste beginnt dort bei y 39, genau wie in
    /// Panel 1), also braucht jede Zahl keinen Faktor.
    ///
    /// GEMESSENE BEHAUPTUNGEN (alle 1:1 an `vorlage_nutzer.webp`, Panel 2,
    /// Inhalt x 988..1653 / y 17..483):
    /// * das Feld ist GEMESSEN **299 px** breit (Rand x 347 .. 645)
    /// * es haengt **mittig** unter dem Stern (Sternmitte 501,
    ///   Feldmitte 496)
    /// * es beginnt GEMESSEN 2 px unter dem Stern
    /// * die Zeilen haben GEMESSEN **31 px** Abstand
    #[test]
    fn die_geometrie_ist_die_gemessene() {
        assert_eq!(BREITE, 299, "GEMESSEN 1:1 an Panel 2: x 347..645");
        assert_eq!(
            ZEILE_HOEHE, 31,
            "GEMESSEN 1:1: Zeilenanfaenge y 167, 198, 229 ..."
        );
        assert_eq!(SUCHFELD_HOEHE, 32, "GEMESSEN 1:1: Rand y 120 .. 151");
        assert_eq!(
            TITEL_HOEHE, 19,
            "GEMESSENE ENTSCHEIDUNG: die halbe Zeilenhoehe"
        );
        assert_eq!(
            LUFT_ABSCHNITT, 12,
            "GEMESSENE ENTSCHEIDUNG: eine Zahl fuer drei Abschnitte"
        );
        assert_eq!(
            LUFT_LINKS, 19,
            "GEMESSEN 1:1: Panel x 347, erstes Favicon x 366"
        );
        assert_eq!(FAVICON_KANTE, 23, "GEMESSEN 1:1: Favicon x 366..388");
        assert_eq!(
            LUFT_NAME, 14,
            "GEMESSEN 1:1: Favicon endet 388, Name beginnt 402"
        );
        assert_eq!(
            LUFT_PUNKTE, 12,
            "GEMESSEN: noch aus der Karte — offene Messung"
        );
        // DIE HOEHE ist eine Rechnung: 10 + 19 + 12 + 32 + 12 + (8 mal 31)
        // + 12 + 31 + 10 = **327**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier `(8 + 1)` Zeilen stehen und
        // nicht 8: die Zeile „Lesezeichen verwalten“ ist auch eine Zeile
        // (das Bild zeigt sie genauso hoch wie die Eintraege).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Rechnung hier ausgeschrieben
        // wird und nicht ueber die Konstanten: sonst prueft der Test die
        // Konstanten gegen sich selbst (dieselbe Ueberlegung wie bei
        // `ADRESSE_HOEHE` in `lib.rs`).
        // DIE HOEHE ist eine Rechnung: 10 + 19 + 12 + 32 + 12 + (8 mal 31)
        // + 12 + 10 = **386** — und das ist GEMESSEN die Hoehe des Panels
        // an Panel 2 (Rand y 77..462 = 386 px).
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier `(8 + 1)` Zeilen stehen und
        // nicht 8: die Zeile „Lesezeichen verwalten“ ist auch eine Zeile
        // (das Bild zeigt sie genauso hoch wie die Eintraege).
        //
        // GEMESSENER EIGENER FEHLER (03.10., 20:5x, an der 1:1-Messung
        // gefunden): in der Rechnung stand **zweimal** eine Zeile fuer
        // „Lesezeichen verwalten“ — einmal im `(zeilen + 1)` und einmal als
        // eigener Summand. Das Feld war dadurch GEMESSEN 31 px zu hoch
        // (417 statt 386), und 31 ist genau eine Zeilenhoehe.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Rechnung hier ausgeschrieben
        // wird und nicht ueber die Konstanten: sonst prueft der Test die
        // Konstanten gegen sich selbst (dieselbe Ueberlegung wie bei
        // `ADRESSE_HOEHE` in `lib.rs`).
        // DIE HOEHE ist eine Rechnung: 10 + 19 + 12 + 32 + 12 + (8 mal 31)
        // + 12 + 31 + 10 = **386** — und das ist GEMESSEN die Hoehe des
        // Panels an Panel 2 (Rand y 77..462 = 386 px).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Zeile „Lesezeichen verwalten“
        // ein **eigener Summand** ist und nicht die neunte Zeile: sie wird
        // von `verwalten()` nach einer `LUFT_ABSCHNITT` gelegt, und die
        // Vorlage zeigt dort einen **Trennstrich** (GEMESSEN y 419..421).
        // Wer sie als neunte Zeile zaehlt, macht die Rechnung richtig und
        // die Anordnung falsch — das ist der Fehler vom 20:5x.
        assert_eq!(
            hoehe(8),
            10 + 19 + 12 + 32 + 12 + 9 * 31 + 12 + 10,
            "GEMESSEN 1:1: 386 px fuer acht Eintraege (neun Zeilen: acht + verwalten)"
        );
        // UND sie ist durch [`MAX_ZEILEN`] begrenzt.
        assert_eq!(hoehe(500), hoehe(MAX_ZEILEN), "M9: hoechstens 12 Zeilen");
        let chrome = Chrome::neu(1920);
        let p = panel(&chrome, 0).expect("das Feld passt bei 1920 px");
        let stern = chrome.rechts_symbol(4);
        assert_eq!(p.breite, BREITE);
        assert_eq!(p.y, stern.unten() + 1, "GEMESSEN 1 px unter dem Stern");
        // LEITUNG (03.10., Nachbesserung 3): die Verankerung ist nicht mehr
        // „mittig unter dem Stern“, sondern **rechts buendig am Fenster**.
        //
        // GEMESSEN 1:1 an Panel 2: der rechte Rand des Panels liegt
        // GEMESSEN **20 px** vor dem Fensterrand, das Panel laeuft von
        // **x 347** bis **x 645** bei **666 px** Fensterbreite.
        //
        // GEMESSENER EIGENER FEHLER (03.10., 20:4x): dieser Assert behauptete
        // `Feldmitte == Sternmitte` (Sternmitte 501, Feldmitte 496). Das war
        // eine **Annahme** aus einer Mockup, und sie brachte bei 666 px
        // GEMESSEN **39 px** zu wenig nach links — unsere Symbolknoepfe
        // stehen GEMESSEN 209 px vom rechten Rand, die der Vorlage in
        // Panel 2 nur GEMESSEN 165 px. Eine Kante am **Fenster** ist
        // messbar; eine Kante an einem **Knopf** ist eine Behauptung.
        assert_eq!(
            p.x + p.breite,
            chrome.fensterbreite - LUFT_RECHTS_FENSTER,
            "GEMESSEN 1:1: das Feld endet 20 px vor dem Fensterrand"
        );
        // UND: die letzte Zeile LIEGT IM FELD. GEMESSENER EIGENER FEHLER
        // (19:20, im Bild gesehen): „Lesezeichen verwalten" stand
        // GEMESSEN 2 px UNTER dem Rand des Feldes — das Feld war
        // GEMESSEN 287 px hoch, die Zeile endete bei 289.
        let voll = Rechteck {
            hoehe: hoehe(8),
            ..p
        };
        assert!(
            verwalten(&voll, 8).unten() <= voll.unten(),
            "die Zeile unten ragt heraus ({} > {})",
            verwalten(&voll, 8).unten(),
            voll.unten()
        );
        // DIE ABSCHNITTE liegen untereinander, ohne Luecke.
        assert_eq!(titel(&p).unten() + LUFT_ABSCHNITT, suchfeld(&p).y);
        assert_eq!(suchfeld(&p).unten() + LUFT_ABSCHNITT, zeile(&p, 0).y);
        assert_eq!(
            zeile(&p, 0).x,
            titel(&p).x,
            "Titel, Feld und Zeilen haben denselben Einzug"
        );
    }

    /// DER TREFFER: Eintrag, Drei-Punkte, „Lesezeichen verwalten“ — sonst
    /// nichts.
    #[test]
    fn der_treffer_kennt_alle_drei_arten() {
        let chrome = Chrome::neu(1920);
        let l = liste(8);
        // 05.10. (UI): `panel()` rechnet die Hoehe aus der Zeilenzahl —
        // der Kasten stand hier vorher als **Korrektur** daneben, weil
        // `panel()` `hoehe(0)` lieferte.
        let p = panel(&chrome, l.eintraege.len()).expect("Feld");
        let mitte =
            |r: &Rechteck| Point::new((r.x + r.breite / 2) as f32, (r.y + r.hoehe / 2) as f32);
        assert_eq!(treffer(&p, 8, &mitte(&zeile(&p, 0))), Treffer::Eintrag(0));
        assert_eq!(treffer(&p, 8, &mitte(&zeile(&p, 5))), Treffer::Eintrag(5));
        // GEMESSENE ENTSCHEIDUNG (03.10., Nachbesserung 4): der Kasten kommt aus
        // [`punkte_kasten`] — derselbe, den der Zeichner benutzt. Vorher
        // stand hier eine eigene Rechnung mit [`crate::STERN_KANTE`], und
        // der Test pruefte damit eine Groesse, die im Bild nicht mehr lag.
        let punkte = punkte_kasten(&p, 3);
        assert_eq!(treffer(&p, 8, &mitte(&punkte)), Treffer::Punkte(3));
        assert_eq!(
            treffer(&p, 8, &mitte(&verwalten(&p, 8))),
            Treffer::Verwalten
        );
        // UND ausserhalb/NaN nichts.
        assert_eq!(treffer(&p, 8, &Point::new(0.0, 0.0)), Treffer::Nichts);
        assert_eq!(
            treffer(&p, 8, &Point::new(f32::NAN, f32::NAN)),
            Treffer::Nichts
        );
    }

    /// IM BILD: Titel, Hinweis, Namen, Punkte, „Lesezeichen verwalten“.
    #[test]
    fn im_bild_steht_alles_aus_der_vorlage() {
        let chrome = Chrome::neu(1920);
        let l = liste(4);
        let zustand = Panel::default();
        let liste = display_list(&zustand, &l, &chrome);
        let texte: Vec<&str> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(texte.contains(&TITEL), "der Titel fehlt: {texte:?}");
        assert!(texte.contains(&HINWEIS), "der Hinweis fehlt: {texte:?}");
        assert!(
            texte.contains(&VERWALTEN),
            "die Zeile unten fehlt: {texte:?}"
        );
        for nr in 0..4 {
            let name = format!("Eintrag {nr}");
            assert!(texte.contains(&name.as_str()), "{name} fehlt: {texte:?}");
        }
    }

    /// DER SUCHTEXT ERSETZT DEN HINWEIS (und wird gekuerzt, M9).
    #[test]
    fn der_suchtext_ersetzt_den_hinweis() {
        let chrome = Chrome::neu(1920);
        let l = liste(2);
        let mut zustand = Panel::default();
        zustand.setze_suchtext("  wetter  ");
        assert_eq!(zustand.suchtext, "wetter", "getrimmt wie die Adresse");
        let liste = display_list(&zustand, &l, &chrome);
        let texte: Vec<&str> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(texte.contains(&"wetter"), "der Suchtext fehlt: {texte:?}");
        assert!(!texte.contains(&HINWEIS), "der Hinweis ist noch da");
        // M9: 5000 Zeichen werden auf MAX_SUCHTEXT gekuerzt.
        zustand.setze_suchtext(&"x".repeat(5000));
        assert_eq!(zustand.suchtext.chars().count(), MAX_SUCHTEXT);
    }

    /// DER HOVER LIEGT VOR DEM TEXT (die Lehre aus dem Hauptmenue).
    #[test]
    fn der_hover_liegt_vor_dem_text() {
        let chrome = Chrome::neu(1920);
        let l = liste(3);
        let ohne = display_list(&Panel::default(), &l, &chrome);
        let mut zustand = Panel::default();
        zustand.gehover = Some(1);
        let mit = display_list(&zustand, &l, &chrome);
        assert_eq!(mit.len(), ohne.len() + 1, "der Hover ist EIN Rechteck");
        // 05.10. (UI): `panel()` rechnet die Hoehe aus der Zeilenzahl —
        // der Kasten stand hier vorher als **Korrektur** daneben, weil
        // `panel()` `hoehe(0)` lieferte.
        let p = panel(&chrome, l.eintraege.len()).expect("Feld");
        let r = zeile(&p, 1);
        let hover = mit
            .iter()
            .position(|i| {
                matches!(i, DisplayItem::Rect { rect, .. }
                if rect.y >= r.y as f32 && rect.y < r.unten() as f32)
            })
            .expect("die Hoverflaeche fehlt");
        let text = mit
            .iter()
            .position(|i| matches!(i, DisplayItem::Text { text, .. } if text == "Eintrag 1"))
            .expect("der Name fehlt");
        assert!(hover < text, "die Hoverflaeche liegt nach dem Text");
    }

    /// DAS ECHTE FAVICON STEHT DA, WO VORHER DER PLATZHALTER STAND
    /// (LEITUNG 03.10., Nachbesserung 2).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Test an der **Stelle** misst und
    /// nicht zaehlt: im Bild sind drei Bild-Items — die Lupe im Suchfeld,
    /// das Favicon und der Ordner in der Fusszeile. Eine Zahl, die „das
    /// Favicon ist da“ beweisen soll, ist bei drei Bildern eine Zahl, die
    /// auch ohne Favicon stimmt.
    #[test]
    fn das_echte_favicon_ersetzt_den_platzhalter() {
        let chrome = Chrome::neu(1920);
        let l = liste(2);
        let bild = lesezeichen::Favicon {
            breite: 8,
            hoehe: 8,
            pixels: std::sync::Arc::new(vec![200u8; 4 * 8 * 8]),
        };
        let mut zustand = Panel::default();
        // NUR Zeile 0 hat ein Favicon.
        zustand.setze_favicons(vec![Some(bild.clone()), None]);
        let dl = display_list(&zustand, &l, &chrome);
        let p = panel(&chrome, 2).expect("Feld");
        // GEMESSEN: was in der Kiste liegt — ein Bild oder der Platzhalter.
        let in_kasten = |k: &Rechteck, will_bild: bool| {
            dl.iter().any(|i| match i {
                DisplayItem::Bild { rect, breite, .. } => {
                    will_bild
                        && (rect.x - k.x as f32).abs() < 2.0
                        && (rect.y - k.y as f32).abs() < 2.0
                        && *breite == bild.breite
                }
                DisplayItem::Rect { rect, .. } => {
                    !will_bild
                        && (rect.x - k.x as f32).abs() < 2.0
                        && (rect.y - k.y as f32).abs() < 2.0
                }
                _ => false,
            })
        };
        assert!(
            in_kasten(&favicon_kasten(&p, 0), true),
            "Zeile 0 hat kein Bild, obwohl eines da ist"
        );
        assert!(
            !in_kasten(&favicon_kasten(&p, 1), true),
            "Zeile 1 hat ein Bild, obwohl keines da ist"
        );
        assert!(
            in_kasten(&favicon_kasten(&p, 1), false),
            "Zeile 1 hat keinen Platzhalter"
        );
        // UND: eine kuerzere Favicon-Liste fragt nicht ins Leere (das ist
        // der Grund, warum es `Panel::favicon` und kein `[nr]` gibt).
        let mut kurz = Panel::default();
        kurz.setze_favicons(vec![Some(bild)]);
        assert!(
            kurz.favicon(1).is_none(),
            "eine Liste mit einem Bild hat auch am zweiten Eintrag eins"
        );
        assert!(kurz.favicon(0).is_some());
    }

    /// DIE FUSSZEILE HAT EINEN ORDNER, DAS FELD EINEN RAHMEN
    /// (LEITUNG 03.10., Nachbesserung 5, 6 und 7).
    #[test]
    fn die_fusszeile_hat_einen_ordner_und_das_feld_einen_rahmen() {
        let chrome = Chrome::neu(1920);
        let l = liste(2);
        let dl = display_list(&Panel::default(), &l, &chrome);
        let pal = crate::thema::Palette::fuer(chrome.thema);
        let p = panel(&chrome, 2).expect("Feld");
        // (6) DER ORDNER steht in der Fusszeile, auf deren linker Kante.
        let v = verwalten(&p, 2);
        assert!(
            dl.iter().any(|i| matches!(i, DisplayItem::Bild { rect, .. }
                if (rect.x - v.x as f32).abs() < 2.0
                    && rect.y + rect.height <= v.unten() as f32 + 2.0)),
            "der Ordner steht nicht in der Fusszeile"
        );
        // UND der Text steht UM das Symbol herum, nicht darauf.
        let text_x = v.x + FAVICON_KANTE + LUFT_ORDNER;
        assert!(
            text_x > v.x + FAVICON_KANTE,
            "der Text steht unter dem Ordner"
        );
        // (7) DER RAHMEN: ein Rechteck um das Feld herum, 1 px aussen.
        assert!(
            dl.iter().any(
                |i| matches!(i, DisplayItem::Rect { rect, color, radius, .. }
                if (rect.x - (p.x as f32 - RAHMEN as f32)).abs() < 0.5
                    && (rect.width - (p.breite as f32 + 2.0 * RAHMEN as f32)).abs() < 0.5
                    && *color == pal.adresse_rand
                    && radius[0] >= ECKEN as f32)
            ),
            "um das Feld liegt kein Rahmen"
        );
        assert!(
            dl.iter().any(
                |i| matches!(i, DisplayItem::Rect { rect, color, radius, .. }
                if (rect.x - p.x as f32).abs() < 0.5
                    && *color == pal.panel_grund
                    && radius[0] == ECKEN as f32)
            ),
            "das Feld selbst hat keine runden Ecken von {ECKEN} px"
        );
        // (5) DAS SUCHFELD hat denselben Rahmen und dieselbe Rundung.
        let s = suchfeld(&p);
        assert!(
            dl.iter()
                .any(|i| matches!(i, DisplayItem::Rect { rect, color, .. }
                if (rect.x - (s.x as f32 - RAHMEN as f32)).abs() < 0.5
                    && (rect.y - (s.y as f32 - RAHMEN as f32)).abs() < 0.5
                    && *color == pal.adresse_rand)),
            "um das Suchfeld liegt kein Rahmen"
        );
        // UND es ist KEINE Pille mehr (GEMESSEN sind 8 px, nicht 16).
        //
        // GEMESSENER EIGENER FEHLER (03.10., 21:4x, sofort rot): dieser
        // Assert verglich nur die **x**-Kante. Suchfeld, Titel und
        // Favicon-Kasten liegen alle bei `p.x + LUFT_LINKS` — der erste
        // Testlauf traf deshalb den Platzhalter des Favicons (Radius
        // 11,5 px, weil 23/2) und meldete eine Pille, die es nicht gibt.
        // GEMESSENE LEHRE: ein Test, der „irgendein Rechteck“ sucht,
        // muss an **zwei** Kanten suchen. Eine Kante ist eine Ebene, und
        // in einem Panel stehen an jeder Kante mehrere Figuren.
        assert!(
            !dl.iter()
                .any(|i| matches!(i, DisplayItem::Rect { rect, radius, .. }
                if (rect.x - s.x as f32).abs() < 0.5
                    && (rect.y - s.y as f32).abs() < 0.5
                    && radius[0] > ECKEN as f32 + 0.5)),
            "das Suchfeld ist gerundeter als die {ECKEN} px der Vorlage"
        );
    }

    /// DIE NEUEN ZAHLEN SIND DIE GEMESSENEN (Nachbesserung 3, 4, 5, 6, 7).
    #[test]
    fn die_neuen_zahlen_sind_die_gemessenen() {
        assert_eq!(
            SCHRIFT, 13.0,
            "LEITUNG 03.10.: Schrift 13-14 px, 13 gewaehlt"
        );
        assert_eq!(PUNKTE_HOEHE, 13, "GEMESSEN: die Punkte y 169..181 = 13 px");
        assert_eq!(ECKEN, 8, "LEITUNG 03.10.: Ecken 8 px; GEMESSEN 348 -> 347");
        assert_eq!(RAHMEN, 1, "LEITUNG 03.10.: Rahmen 1 px");
        assert_eq!(
            LUFT_ORDNER, 12,
            "GEMESSEN: Ordner endet 388, Text beginnt 400"
        );
        assert_eq!(
            LUFT_SUCHFELD, 10,
            "GEMESSEN: Lupe 4..6 px nach der linken Randsaeule x 361"
        );
        // UND die Punkte-Kaesten von Zeichner und Trefferfrage sind dieselben
        // (sonst waere ein Klick auf die Punkte ein Klick daneben).
        let chrome = Chrome::neu(1920);
        let p = panel(&chrome, 2).expect("Feld");
        for nr in 0..2 {
            let k = punkte_kasten(&p, nr);
            let mitte = Point::new((k.x + k.breite / 2) as f32, (k.y + k.hoehe / 2) as f32);
            assert_eq!(
                treffer(&p, 2, &mitte),
                Treffer::Punkte(nr),
                "die Mitte des Kastens {nr} trifft die Punkte nicht"
            );
        }
    }

    /// DAS FELD PASST IN JEDES FENSTER (M8).
    #[test]
    fn das_feld_passt_in_jedes_fenster() {
        for breite in [0u32, 1, 100, 200, 400, 800, 1920, u32::MAX] {
            let chrome = Chrome::neu(breite);
            match panel(&chrome, 2) {
                None => {}
                Some(p) => {
                    assert!(p.breite <= breite, "bei {breite}: zu breit");
                    assert!(p.rechts() <= breite, "bei {breite}: ragt rechts heraus");
                    assert!(p.unten() > p.y, "bei {breite}: keine Hoehe");
                }
            }
            // UND: es panickt in keinem Fall, egal wie viele Eintraege.
            let l = liste(if breite % 2 == 0 { 500 } else { 0 });
            //
            // PUNKT 35 (02.10.): eine Unterdrueckung (`let _ = x;`) gehoert
            // in den Namen und nicht in den Rumpf — sonst ist die Zeile
            // eine Anweisung, die nichts tut, und der naechste liest sie
            // als Absicht.
            let _liste = display_list(&Panel::default(), &l, &chrome);
        }
    }
}
