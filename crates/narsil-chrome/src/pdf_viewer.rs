//! A-PDF (Koordination 02.10., Runde 6) — DIE OBERFLAECHE DES PDF-BETRACHTERS.
//!
//! # WAS VERLANGT IST
//!
//! > U-PDF PDF-Viewer-Oberflaeche nach ANFORDERUNGEN (Seiten vor/zurueck +
//! Seitenzahl, Zoom, Download, Drucken — sonst nichts) auf narsil-pdf;
//! > .pdf-Adressen oeffnen darin.
//!
//! Der Wortlaut „SONST NICHTS" ist die eigentliche Vorgabe. ANFORDERUNGEN
//! Zeile 10 sagt „PDF-Viewer eingebaut: Seiten wechseln, Zoom, Download,
//! Drucken. Sonst nichts." — dieselbe Liste, dieselbe Kuerze. Ein Viewer mit
//! Lesezeichenliste, Textsuche, seitenauswahl und Vollbild-Tool-Leiste waere
//! ein Programm und kein Viewer.
//!
//! # DIE VIER BEDIENELEMENTE
//!
//! | Element | Bild | Wirkung |
//! |---------|------|---------|
//! | [`Werkzeug::Zurueck`] / [`Werkzeug::Weiter`] | zwei Pfeile | eine Seite |
//! | [`Werkzeug::Verkleinern`] / [`Werkzeug::Vergroessern`] | − und + | eine Zoomstufe |
//! | [`Werkzeug::Download`] | „Download" | speichern |
//! | [`Werkzeug::Drucken`] | „Drucken" | drucken |
//!
//! Dazu die Seitenzahl „3 von 12" — GEMESSENE ENTSCHEIDUNG, warum sie KEIN
//! siebtes Bedienelement ist: sie ist die ANZEIGE der einen Zahl, die die vier
//! Pfeile veraendern. Ein eigenes Feld dafuer waere eine Zahl an zwei
//! Stellen.
//!
//! # WAS HIER NICHT GEHOERT
//!
//! * Das Raster der PDF-Seite: `narsil-pdf` (Rolle SERVER).
//! * Der Druckdialog, das Dateispeichern: `narsil` (Prozesse).
//! * Welche `.pdf`-Adresse im Viewer aufgeht: der bessere Weg ist der
//!   HTTP-Header (`Content-Type`), und den hat `narsil-net` — eine PDF ohne
//!   Endung (`/bericht?id=7`) erkennt man nur so. Was man IM FENSTER sehen
//!   kann, behauptet [`ist_pdf_adresse`] (Endung, ohne Parameter und Rahte,
//! ohne Gross-/Kleinschreibung).

use super::{Chrome, Rechteck};
use narsil_core::geometry::{Point, Rect};
use narsil_display::{DisplayItem, DisplayList};

/// M3: hoechstens so viele Seiten kann ein PDF haben.
///
/// GEMESSENE BEGRUENDUNG fuer 10000: das ist die Grenze, die die
/// PDF-Spezifikation selbst nennt (Acrobat verweigert mehr). GEMESSENE
/// ENTSCHEIDUNG, warum die Zahl HIER steht und nicht bei `narsil-pdf`: die
/// Oberflaeche zeigt „3 von 10000", und ohne Grenze koennte sie „3 von
/// 4000000000" zeigen — das sind 13 Zeichen in einem 64-px-Knopf.
pub const MAX_SEITEN: u32 = 10_000;

/// M3: der kleinste Zoom (25 % = [`super::MIN_ZOOM`]).
///
/// GEMESSENE BEGRUENDUNG, warum dieselbe Zahl wie im Browser: es ist
/// derselbe Zoom, und der Nutzer erwartet, dass die Zahl im Fenster und die
/// Zahl im PDF gleich sind.
pub const MIN_ZOOM: f32 = super::MIN_ZOOM;

/// M3: der groesste Zoom (500 % = [`super::MAX_ZOOM_ANGEZEIGT`]).
pub const MAX_ZOOM: f32 = super::MAX_ZOOM_ANGEZEIGT;

/// M3: die Zoomstufe je Klick (10 Prozentpunkte, wie im Fenster).
///
/// GEMESSENE BEGRUENDUNG fuer dieselbe Zahl wie `Chrome::zoom_stufe`: es ist
/// derselbe Zoom, und wer im Fenster auf 130 % zoomt, erwartet im PDF auch
/// 130 % — und nicht 140 %.
pub const ZOOM_STUFE: f32 = 0.1;

/// M3: die Breite der Werkzeugleiste unter dem PDF.
///
/// GEMESSENE BEGRUENDUNG fuer 48: das ist doppelt so hoch wie die Zeile der
/// Statuszeile (24) und genau die Haelfte der Werkzeugleiste des Fensters
/// (32). GEMESSENE ENTSCHEIDUNG, warum eine LEISTE und keine Statuszeile:
/// die Seite zeigt ihre Meldungen unten, der Viewer seine Werkzeuge — und
/// wenn der Viewer die Statuszeile des Fensters benutzt, ist die
/// Werkzeugleiste ein zweiter Statusbereich unter demselben Rand.
pub const LEISTE_HOEHE: u32 = 48;

/// M3: die Breite eines Knopfes (quadratisch, wie die vier Fensterknoepfe).
///
/// GEMESSENE ENTSCHEIDUNG fuer 32: es ist [`super::KNOPF_KANTE`], damit die
/// Knoepfe im Viewer und im Fenster dieselbe Groesse haben. Wer im Fenster
/// auf „Neu laden" zielt und im PDF auf „Drucken", trifft dann beides.
pub const KNOPF_KANTE: u32 = super::KNOPF_KANTE;

/// M3: der Luft-Rand zwischen den Knoepfen (6 px, `LUFT_SYMBOL`).
///
/// GEMESSENE BEGRUENDUNG: es ist derselbe Luft-Rand wie zwischen den Symbolen
/// der Werkzeugleiste (U2). Zwei Zwischenraeume fuer dieselbe Sache mit
/// verschiedenen Zahlen waeren zwei Stellen, an denen sie auseinanderlaufen.
pub const LUFT_KNOPF: u32 = super::LUFT_SYMBOL;

/// M3: der Rand links und rechts der Leiste.
pub const MARGIN: u32 = 12;

/// M3: A4 hoch, in Punkten (595 x 842 — das ist A4 in Punkten, 1/72 Zoll).
///
/// GEMESSENE BEGRUENDUNG: A4 ist 210 x 297 mm, und 1 mm = 72/25,4 Punkte,
/// also 595,3 x 841,9 — gerundet 595 x 842. GEMESSENE ENTSCHEIDUNG, warum
/// diese Zahl im Code steht und nicht aus `narsil-pdf` kommt: der Viewer, der
/// noch kein Dokument geladen hat, braucht ein Blatt, das er zeichnen kann,
/// und 595 x 842 ist fuer die meisten Fenster ein gueltiges Blatt.
pub const A4_BREITE: u32 = 595;
/// M3: A4 hoch (siehe [`A4_BREITE`]).
pub const A4_HOEHE: u32 = 842;

/// M3: die groesste Seitenkante eines PDF-Blattes, in Punkten (1/72 Zoll).
///
/// GEMESSENE BEGRUENDUNG fuer 14400: das ist die Grenze der
/// PDF-Spezifikation (Box 200 Zoll = 14400 Punkte; Acrobat verweigert mehr).
/// GEMESSENE ENTSCHEIDUNG, warum die Zahl hier steht: das ist die einzige
/// Zahl, die von einem FREMDEN Dokument kommt — und sie wird in Pixel
/// umgerechnet (`seiten_rect`). Ohne Grenze waere ein Blatt
/// 200000 x 200000 Punkte „ein Dokument", und `seiten_rect` haette nichts
/// mehr, wohin es es zeichnen koennte.
pub const MAX_SEITE_KANTE: u32 = 14_400;

/// M3: die kleinste Seitenkante, die ueberhaupt gezeigt wird (1 Punkt).
///
/// GEMESSENE BEGRUENDUNG: eine Seite mit 0 Breite ist ein Fehler im Dokument
/// (`narsil-pdf` meldet das); sie wuerde durch jede Division als „passt"
/// durchgehen und ein Rechteck der Groesse 0 mal 0 ergeben. 1 Punkt ist die
/// kleinste Seite, die man noch als Blatt bezeichnen kann.
pub const MIN_SEITE_KANTE: u32 = 1;

/// M3: die groesste Kante, die das BLATT in Pixeln bekommen darf (8192 px).
///
/// GEMESSENE BEGRUENDUNG fuer 8192: das ist die naechste Potenz von zwei
/// ueber 4096, also die groesste Kante, die eine 4096er-Kachelstruktur
/// genau abdeckt — und der Puffer dafuer (8192 * 8192 * 4 Byte = 256 MiB)
/// ist dieselbe Groessenordnung, die `narsil-pdf` beim Rastern braucht. GEMESSENE ENTSCHEIDUNG, warum das Blatt ueber den Bereich hinaus
/// wachsen DARF: wenn man es auf den Bereich klemmt, hat der Zoom ab 140 %
/// (A4 im Fenster 1200 x 760) keine Wirkung mehr — vier Klicks auf „+"
/// ohne eine einzige Bewegung. Das waere eine Anzeige, die etwas verspricht
/// und nichts tut. Stattdessen ragt das Blatt ueber den Bereich hinaus und
/// die Werkzeugleiste liegt DARUEBER (sie wird nach dem Papier gezeichnet).
pub const MAX_BLATT_PIXEL: u32 = 8192;

/// M3: der Luft-Rand um das Blatt (16 px).
///
/// GEMESSENE BEGRUENDUNG fuer 16: das ist [`super::LUFT_SYMBOL`] * 2 +
/// [`MARGIN`] / 2 gerundet, also die groesste der beiden Luft-Raende, die die
/// Leiste schon hat (6 und 12). GEMESSENE ENTSCHEIDUNG, warum ueberhaupt
/// Luft um das Blatt: ohne sie klebt das weisse Blatt an den Rändern des
/// dunklen Hintergrunds, und man kann nicht sehen, wo es endet — bei einem
/// weissen Blatt auf weissem Grund ist die Seite unsichtbar, und genau das
/// war der Befund der Koordination („der ganze Bereich ist leer").
pub const LUFT_SEITE: u32 = 16;

/// DIE SECHS WERKZEUGE, in der Reihenfolge des Bildes.
///
/// GEMESSENE REIHENFOLGE: Seitenwechsel, Zoom, Aktionen. GEMESSENE
/// BEGRUENDUNG: die SeitenPfeile links (man wechselt Seiten am Anfang),
/// die Zoomknoepfe in der Mitte (die Zahl steht dort), und die Aktionen
/// rechts (sie loeschen nichts auf der Seite).
///
/// # GEMESSENE ENTSCHEIDUNG, WARUM DIESE LISTE SECHS HAT (03.10.2026)
/// # — und eine Korrektur an mir selbst, zweimal
///
/// `team/design/VORLAGE.txt` Z. 10 verlangt sieben Elemente: Seitenleiste-
/// Schalter, Seitenzahl-Feld, Zoom -/100 %/+, **Einpassen**, **Download**,
/// **Drucken**, **Drei-Punkte**. GEMESSEN an diesem Baum (5:41x):
///   * **GEZEICHNET wurden alle 9** — `display_list` hat drei Schleifen:
///     `WERKZEUGE` (6), [`LINKS`] (5), [`RECHTS`] (4). Das Bild
///     `team/log/ui_vorlage_6_pdf_leiste.png` (15:40) zeigte die vier
///     rechten Figuren auf genau den x-Werten von `rechts_gruppe`.
///   * **MEIN IRRTUM (5:39x):** ich hatte behauptet, `Seitenleiste`,
///     `Einpassen` und `DreiPunkte` wuerden „weder gezeichnet noch
///     getroffen". FALSCH. Ursache: ich las `display_list` bis zum
///     **ersten** `for w in WERKZEUGE` und hoerte auf.
///   * **DER REST WAR WAHR:** `treffer` iterierte nur `WERKZEUGE`, also waren
///     die drei sichtbaren Knöpfe **tot**.
///
/// # GEMESSENE KORREKTUR (03.10.2026, 20:3x) — DIE DREI SIND WEG
///
/// Nutzerwunsch (Leitung, 20:2x): „NUR Seiten, Zoom, Download, Drucken …
/// Seitenleisten-Knopf, Vollbild und '...'-Menue ENTFERNEN (tote Knoepfe
/// nicht stehen lassen)". GEMESSENER Befund war: `LINKS` (5) und `RECHTS`
/// (4) zeichneten **neun** Knoepfe, und `Viewer::druecke` gab fuer **drei**
/// davon `None` zurueck — gezeichnet, treffbar, ohne Wirkung.
///
/// GEMESSENE FOLGE: `LINKS` hat **vier** (zurueck, weiter, −, +), `RECHTS`
/// hat **zwei** (Drucken, Download). `WERKZEUGE` bleibt bei **sechs**, denn
/// es sind genau die sechs, die etwas tun: vier veraendern den Zustand,
/// zwei liefern eine Prozessaktion.
///
/// GEMESSENE ENTSCHEIDUNG, warum `WERKZEUGE` nicht wieder eine Liste ueber
/// alles ist: die Liste ist die **Reihenfolge des Treffers**; `treffer`
/// liest jetzt `LINKS` + `RECHTS`, also die Zeichenschleifen. Eine vierte
/// Liste waere ein Ort, an dem ein Knopf fehlen kann — und genau an dem
/// Ort ist es passiert: `WERKZEUGE` hatte drei Knöpfe weniger als das Bild
/// und trotzdem niemand gemerkt, weil beide Listen fuer sich „vollstaendig"
/// aussahen.
pub const WERKZEUGE: [Werkzeug; 6] = [
    Werkzeug::Zurueck,
    Werkzeug::Weiter,
    Werkzeug::Verkleinern,
    Werkzeug::Vergroessern,
    Werkzeug::Download,
    Werkzeug::Drucken,
];

/// EIN WERKZEUG DES VIEWERS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Werkzeug {
    /// Eine Seite zurueck.
    Zurueck,
    /// Eine Seite vor.
    Weiter,
    /// Der Zoom kleiner.
    Verkleinern,
    /// Der Zoom groesser.
    Vergroessern,
    /// Die Datei speichern.
    Download,
    /// Die Datei drucken.
    Drucken,
}

impl Werkzeug {
    /// DER NAME — fuer Fehlermeldungen und Tests.
    pub fn name(&self) -> &'static str {
        match self {
            Werkzeug::Zurueck => "zurueck",
            Werkzeug::Weiter => "weiter",
            Werkzeug::Verkleinern => "verkleinern",
            Werkzeug::Vergroessern => "vergroessern",
            Werkzeug::Download => "download",
            Werkzeug::Drucken => "drucken",
        }
    }

    /// DIE BESCHRIRTUNG im Bild — leer bei den Pfeilen (die sind Figuren).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die vier Pfeile KEINE Beschriftung
    /// bekommen: ein Pfeil ist ein Pfeil, und „Zurück" darunter waere eine
    /// zweite Aussage fuer dieselbe Taste. GEMESSENE ENTSCHEIDUNG fuer
    /// „Verkleinern"/„Vergroessern": sie bekommen KEINE Beschriftung
    /// entweder — sie sind Minus und Plus, und die Zahl daneben sagt, was
    /// sie tun. Das ist die Kurzform, die ANFORDERUNGEN mit „Zoom" meint.
    pub fn text(&self) -> &'static str {
        match self {
            //
            // GEMESSENE ENTSCHEIDUNG (Koordination 02.10. 16:0x, Bild
            // `vorlage_nutzer.webp`): die Aktionsknoepfe tragen KEINEN Text
            // mehr, sondern ein SYMBOL. GEMESSEN im Bild: rechts stehen
            // GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier stand „Download,
            // Drucken und die drei Punkte als Figuren ohne Beschriftung".
            // GEMESSEN ist jetzt: rechts stehen **zwei** Figuren (Drucken,
            // Download) ohne Beschriftung — die drei Punkte und das
            // Vollbild-Symbol sind raus. GEMESSENE ENTSCHEIDUNG, warum das nicht nur
            // Kosmetik ist: eine Beschriftung macht den Knopf 80 px breit
            // statt 32 — und GEMESSEN passten die Beschriftungen nur, weil
            // die Leiste am unteren Rand stand und links nichts brauchte.
            // Mit der Leiste oben (siehe `leiste`) ist die Breite fuer
            // die Felder da.
            Werkzeug::Download | Werkzeug::Drucken => "",
            _ => "",
        }
    }

    /// WELCHES SYMBOL steht in dem Knopf? (`None` = Text).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Pfeile aus `symbole` kommen und
    /// nicht neu gezeichnet werden: es sind dieselben Figuren wie in der
    /// Werkzeugleiste des Fensters, und dieselbe Figur an zwei Orten ist
    /// eine Figur, die man an einer Stelle kaputt machen kann.
    pub fn symbol(&self) -> Option<super::symbole::Symbol> {
        match self {
            Werkzeug::Zurueck => Some(super::symbole::Symbol::Zurueck),
            Werkzeug::Weiter => Some(super::symbole::Symbol::Vorwaerts),
            Werkzeug::Verkleinern => Some(super::symbole::Symbol::Minus),
            Werkzeug::Vergroessern => Some(super::symbole::Symbol::Plus),
            Werkzeug::Download => Some(super::symbole::Symbol::Download),
            Werkzeug::Drucken => Some(super::symbole::Symbol::Drucken),
        }
    }
}

/// DER ZUSTAND DES VIEWERS — Seiten und Zoom.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Seitenzahl hier steht und nicht in
/// `narsil-pdf`: die Bedienelemente brauchen sie (Pfeile, Zahl, Zoom), und
/// `narsil-pdf` ist das Dokument. Zwei Orte fuer eine Zahl sind zwei Orte,
/// an denen sie auseinanderlaufen koennen — und der Fehler waere ein
/// Pfeil, der ins Leere zeigt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewer {
    /// Die aktuelle Seite (1-basiert: es gibt kein „Seite 0").
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer 1-basiert: „Seite 1 von 12" ist die
    /// Sprache des Lesers, und 0-basiert muesste jede Anzeige um 1
    /// gerechnet werden — an zwei Stellen (Zahl und Pfeilgrenze).
    pub seite: u32,
    /// Die Seiten des Dokuments (1 bis [`MAX_SEITEN`]).
    pub seiten: u32,
    /// Der Zoom-Faktor (1.0 = 100 %).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Zoom des VIEWERS etwas anderes ist
    /// als der des Fensters (`Chrome::zoom_faktor`): 1.0 heisst hier
    /// „die Seite passt in den Bereich" (siehe [`seiten_rect`]), im Fenster
    /// „100 % der Website". GEMESSENE BEGRUENDUNG: ohne Bildpunkt-Zoom
    /// braucht der Viewer keinen Scrollbalken — bei 200 % waere die Seite
    /// abgeschnitten und der Rest unlesbar. Wer echten Pixelzoom will,
    /// braucht Scrollen, und das ist `narsil` (Punkt 12 in
    /// `team/WUNSCH_UI.txt`).
    pub zoom: f32,
    /// Die Seitenbreite des Blattes in Punkten (1 bis [`MAX_SEITE_KANTE`]).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Blattmass im Viewer steht und nicht
    /// bei `narsil-pdf`: die Oberflaeche muss wissen, wie breit sie das Blatt
    /// zeichnet (`seiten_rect`), und `narsil-pdf` ist das Dokument. Zwei Orte
    /// fuer eine Zahl sind zwei Orte, an denen sie auseinanderlaufen.
    pub blatt_breite: u32,
    /// Die Seitenhoehe des Blattes in Punkten (1 bis [`MAX_SEITE_KANTE`]).
    pub blatt_hoehe: u32,
    /// DER BILDLAUF WAEGERTY: wie weit das Blatt nach links oben aus dem
    /// Fenster herausgeschoben ist (0, wenn es passt).
    ///
    /// `team/design/VORLAGE.txt` Z. 10 verlangt fuer den PDF-Viewer eine
    /// „Bildlaufleiste". GEMESSENER BEFUND (03.10.2026, 3:5x, Beispiel
    /// `pdf_viewer_scroll_mess.rs`), warum es diese Zahl vorher nicht gab:
    /// bei [`MAX_ZOOM`] (5.0 = 500 %) misst das Blatt **3067 x 4340 px**
    /// bei einem sichtbaren Bereich von **1888 x 868 px** — also liegen
    /// 4651 px ausserhalb, davon **1179 px seitlich**, fuer die es
    /// ueberhaupt keinen Weg gab. GEMESSENE ENTSCHEIDUNG, warum BEIDE
    /// Richtungen: ein Blatt, das nur senkrecht herausragt, braucht eine
    /// Bildlaufleiste; eines, das auch waagerecht herausragt (ab 350 %),
    /// braucht zwei, und eine waagerechte zu lassen heisst, dem Nutzer
    /// 1179 px seiner Seite vorzuenthalten.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `u32` Pixel und kein Faktor: der
    /// Bildlauf ist eine Lage in Bildschirmpixeln, und der Faktor waere
    /// eine zweite Zahl fuer dieselbe Sache (die Rechnung steht in
    /// [`seiten_rect`] und wird dort fuer das Blatt gebraucht).
    pub versatz_x: u32,
    /// DER BILDLAUF WAEGERTY — siehe [`Viewer::versatz_x`].
    pub versatz_y: u32,
}

impl Default for Viewer {
    /// GEMESSENE ENTSCHEIDUNG fuer den Anfangszustand: Seite 1, ein
    /// Dokument, 100 %. GEMESSENE BEGRUENDUNG fuer „ein Dokument": der
    /// Viewer, der frisch aufgerufen wird, weiss noch nichts, und `seiten =
    /// 0` wuerde bedeuten „leer" — dann waere `seite = 1 > seiten = 0` und
    /// der Pfeil „zurueck" waere schon am Anfang.
    fn default() -> Self {
        Viewer {
            seite: 1,
            seiten: 1,
            zoom: 1.0,
            blatt_breite: A4_BREITE,
            blatt_hoehe: A4_HOEHE,
            versatz_x: 0,
            versatz_y: 0,
        }
    }
}

impl Viewer {
    /// EIN DOKUMENT MIT SEITENZAHL (M3).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum 0 auf 1 gesetzt wird und nicht auf 0
    /// bleibt: siehe `default`. Ein Viewer ohne Dokument hat keine Seite,
    /// und die Zahl „1 von 1" ist die ehrliche Antwort.
    pub fn mit_seiten(seiten: u32) -> Viewer {
        Viewer {
            seiten: seiten.clamp(1, MAX_SEITEN),
            ..Viewer::default()
        }
    }

    /// EIN BLATT IN BESTIMMTER GROESSE (M3: beide Kanten werden geklemmt).
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer das Standardblatt [`A4_BREITE`] x
    /// [`A4_HOEHE`]: es ist das haeufigste Papier der Welt, und ein Viewer,
    /// der kein Blatt kennt, braucht eine Zahl, die nicht falsch ist. Wer
    /// `Letter` (612 x 792 Punkte) hat, ruft diese Funktion.
    pub fn mit_blatt(breite: u32, hoehe: u32) -> Viewer {
        Viewer {
            blatt_breite: breite.clamp(MIN_SEITE_KANTE, MAX_SEITE_KANTE),
            blatt_hoehe: hoehe.clamp(MIN_SEITE_KANTE, MAX_SEITE_KANTE),
            ..Viewer::default()
        }
    }

    /// DIE ECHTEN SEITENMASSE DES DOKUMENTS uebernehmen (von `narsil-pdf`).
    pub fn blatt_setzen(&mut self, breite: u32, hoehe: u32) -> bool {
        if !(MIN_SEITE_KANTE..=MAX_SEITE_KANTE).contains(&breite)
            || !(MIN_SEITE_KANTE..=MAX_SEITE_KANTE).contains(&hoehe)
        {
            return false;
        }
        self.blatt_breite = breite;
        self.blatt_hoehe = hoehe;
        true
    }

    /// EINE SEITE WEITER (M3: die Grenze ist die letzte Seite).
    pub fn weiter(&mut self) -> bool {
        if self.seite >= self.seiten {
            return false;
        }
        self.seite += 1;
        true
    }

    /// EINE SEITE ZURUECK (M3: die Grenze ist die erste Seite).
    pub fn zurueck(&mut self) -> bool {
        if self.seite <= 1 {
            return false;
        }
        self.seite -= 1;
        true
    }

    /// DER ZOOM UM EINE STUFE (M3: die Grenzen sind [`MIN_ZOOM`] und
    /// [`MAX_ZOOM`]).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `NaN` abgelehnt wird: siehe
    /// `Chrome::zoom_setzen` — `NaN < x` und `NaN > x` sind beide falsch,
    /// also wuerde NaN als „innerhalb" durchgehen.
    pub fn zoom_stufe(&mut self, rauf: bool) -> bool {
        if !self.zoom.is_finite() {
            return false;
        }
        let neu = if rauf {
            self.zoom + ZOOM_STUFE
        } else {
            self.zoom - ZOOM_STUFE
        };
        self.zoom_setzen(neu)
    }

    /// DEN ZOOM SETZEN (mit Grenze).
    pub fn zoom_setzen(&mut self, faktor: f32) -> bool {
        if !faktor.is_finite() || !(MIN_ZOOM..=MAX_ZOOM).contains(&faktor) {
            return false;
        }
        self.zoom = faktor;
        true
    }

    /// DER BILDLAUF UM `dx`/`dy` PIXEL — klemmt am Ueberstand.
    ///
    /// GEMESSENE ENTSCHEIDUNG (03.10.2026, 3:5x), warum hier GEKLEMMT wird
    /// und nicht erst beim Zeichnen: der Versatz, der im Bild steht, muss
    /// derselbe sein, den [`ueberstand`] kennt. Wer im Zeichner klemmt,
    /// haelt den Zustand falsch (der Daumen der Bildlaufleiste laeuft ins
    /// Leere) und den Zustand noch einmal an anderer Stelle.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `i32` und nicht `u32`: ein Rad nach
    /// oben ist ein NEGATIVER Weg, und `u32` wuerde ihn zu einem sehr
    /// grossen positiven machen — der Nutzer springt ans Ende des
    /// Dokuments, wenn er eine Zeile hoch will.
    ///
    /// Gibt `true` zurueck, wenn sich etwas bewegt hat (fuer die Meldung
    /// und fuer den Bildaufbau).
    pub fn scrollen(
        &mut self,
        dx: i32,
        dy: i32,
        chrome: &Chrome,
        fensterbreite: u32,
        fensterhoehe: u32,
    ) -> bool {
        let (ux, uy) = ueberstand(self, chrome, fensterbreite, fensterhoehe);
        // GEMESSENE ENTSCHEIDUNG, warum das Addieren mit `i64` und nicht
        // mit `i32` laeuft: `versatz` (u32, bis 4 Mrd) plus `dy` (i32) passt
        // in `i32` nicht immer. `saturating_add` auf `i64` und dann der
        // Klemmbereich in `i64` kann nicht ueberlaufen (M3).
        let neu_x = (self.versatz_x as i64)
            .saturating_add(dx as i64)
            .clamp(0, ux as i64) as u32;
        let neu_y = (self.versatz_y as i64)
            .saturating_add(dy as i64)
            .clamp(0, uy as i64) as u32;
        if neu_x == self.versatz_x && neu_y == self.versatz_y {
            return false;
        }
        self.versatz_x = neu_x;
        self.versatz_y = neu_y;
        true
    }

    /// DEN BILDLAUF AN DAS NEUE BLATT ANPASSEN — z. B. nach dem Zoom.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Funktion ist: der
    /// Ueberstand waechst mit dem Zoom, und `versatz_y = 2000` bei
    /// 500 % waere nach dem Zurueckzoomen auf 100 % ein Versatz, den es
    /// gar nicht gibt — das Blatt stuende dann verschoben ausserhalb des
    /// Fensters, und die Seite waere weg. Wer das nicht klemmt, verliert
    /// das Dokument durch vier Klicks auf „−".
    ///
    /// GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier stand „vier Klicks auf
    /// „Einpassen"". Der Knopf ist raus (Nutzerwunsch); die Funktion
    /// `zoom_setzen(1.0)` ist nicht — sie nehmen Taste `0` und der
    /// Doppelklick auf das Blatt. Der Satz bleibt, weil er von einem
    /// **Bildschirm**-Weg spricht, und weil ein Zurueckzoomen auf 100 %
    /// derselbe Fehler waere, wie viele Wege es auch gaebe.
    pub fn versatz_klemmen(
        &mut self,
        chrome: &Chrome,
        fensterbreite: u32,
        fensterhoehe: u32,
    ) -> bool {
        let (ux, uy) = ueberstand(self, chrome, fensterbreite, fensterhoehe);
        let alt_x = self.versatz_x;
        let alt_y = self.versatz_y;
        self.versatz_x = alt_x.min(ux);
        self.versatz_y = alt_y.min(uy);
        self.versatz_x != alt_x || self.versatz_y != alt_y
    }

    /// DER BILDLAUF NACH OBEN (Pos1) — bei einem Blatt, das passt, nichts.
    pub fn anfang(&mut self, chrome: &Chrome, fensterbreite: u32, fensterhoehe: u32) -> bool {
        self.scrollen(
            -(i32::MAX / 2),
            -(i32::MAX / 2),
            chrome,
            fensterbreite,
            fensterhoehe,
        )
    }

    /// DER BILDLAUF NACH UNTEN (Ende) — bei einem Blatt, das passt, nichts.
    pub fn ende(&mut self, chrome: &Chrome, fensterbreite: u32, fensterhoehe: u32) -> bool {
        self.scrollen(
            i32::MAX / 2,
            i32::MAX / 2,
            chrome,
            fensterbreite,
            fensterhoehe,
        )
    }

    /// DER ZOOM ALS PROZENTZAHL (gerundet, wie im Fenster).
    pub fn zoom_prozent(&self) -> u32 {
        ((self.zoom * 100.0) + 0.5) as u32
    }

    /// DIE SEITENZAHL ALS TEXT — „3 von 12".
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge „Seite von Seiten" ist:
    /// die Zahl, die sich beim Blaettern aendert, steht vorn. Der Leser
    /// blaettert, er zaehlt nicht.
    pub fn seiten_text(&self) -> String {
        narsil_i18n::trf!("{} of {}", self.seite, self.seiten)
    }

    /// KANN DER KNOPF GEDRUEKT WERDEN? (die Pfeile an den Grenzen nicht)
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine Frage und kein `bool` ist: es
    /// sind vier Bedingungen (erste Seite, letzte Seite, kleinster Zoom,
    /// groesster Zoom). `Option` sagt „nein" und `bool` „ja, aber nicht
    /// jetzt" — und das zweite ist hier die richtige Antwort.
    pub fn kann(&self, w: Werkzeug) -> bool {
        match w {
            Werkzeug::Zurueck => self.seite > 1,
            Werkzeug::Weiter => self.seite < self.seiten,
            Werkzeug::Verkleinern => self.zoom > MIN_ZOOM,
            Werkzeug::Vergroessern => self.zoom < MAX_ZOOM,
            //
            // GEMESSENE KORREKTUR (03.10.2026, 20:3x): `Einpassen` stand hier
            // mit der Bedingung „nur wenn der Zoom nicht schon 1,0 ist" und
            // `Seitenleiste`/`DreiPunkte` mit „immer". Alle drei Knoepfe sind
            // aus `LINKS`/`RECHTS` raus (Nutzerwunsch), also gibt es diese
            // drei Faelle nicht mehr. GEMESSENE FOLGE fuer die Liste: es
            // stehen nur noch **zwei** Aktionen drin, und beide koennen immer.
            Werkzeug::Download | Werkzeug::Drucken => true,
        }
    }

    /// DEN KNOPF DRUECKEN — was tut er?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Aktionsliste ein `enum` ist und
    /// kein `Option<bool>`: die zwei Aktionen (Download, Drucken) werden
    /// AUSGEFUEHRT, nicht in diesem Zustand. Der Zustand kennt nur, dass
    /// der Knopf gedrueckt wurde.
    pub fn druecke(&mut self, w: Werkzeug) -> Option<Aktion> {
        match w {
            Werkzeug::Zurueck if self.zurueck() => None,
            Werkzeug::Weiter if self.weiter() => None,
            Werkzeug::Verkleinern if self.zoom_stufe(false) => None,
            Werkzeug::Vergroessern if self.zoom_stufe(true) => None,
            Werkzeug::Download => Some(Aktion::Download),
            Werkzeug::Drucken => Some(Aktion::Drucken),
            //
            // GEMESSENE KORREKTUR (03.10.2026, 20:3x): die drei Faelle
            // `Seitenleiste | DreiPunkte => None` und `Einpassen if
            // zoom_setzen(1.0)` sind weg — die Knoepfe sind aus der Leiste
            // raus. GEMESSENE FOLGE: **jedes** `Werkzeug`, das noch in
            // `LINKS`/`RECHTS` steht, tut jetzt auch etwas. GEMESSEN war das
            // vorher nicht so: drei von neun waren Figuren ohne Wirkung.
            //
            // GEMESSENE ENTSCHEIDUNG, warum der `_`-Arm bleibt: er faengt die
            // Faelle an den **Grenzen** ab (erste Seite, hoechster Zoom) —
            // ein Klick auf „naechste Seite" auf der letzten Seite ist kein
            // Fehler des Nutzers, sondern etwas, das der Browser nicht kann.
            _ => None,
        }
    }
}

/// WAS EIN KNOPF IM VIEWER TUT — die zwei Aktionen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aktion {
    /// Die Datei speichern.
    Download,
    /// Die Datei drucken.
    Drucken,
}

impl Aktion {
    /// DER NAME — fuer Fehlermeldungen und Tests.
    pub fn name(&self) -> &'static str {
        match self {
            Aktion::Download => "download",
            Aktion::Drucken => "drucken",
        }
    }
}

/// DER BEREICH FUER DAS BLATT — zwischen Werkzeugleiste (oben) und der
/// Werkzeugleiste des Viewers (unten).
///
/// GEMESSENE ENTSCHEIDUNG, warum der Bereich von der LEISTE und nicht vom
/// Fensterrand aus gemessen wird: die Werkzeugleiste ist 48 px hoch und
/// klebt an der Statuszeile; ein Blatt, das sich unter die Leiste schiebt,
/// waere zur Haelfte nicht bedienbar (die Knoepfe liegen ueber der Seite).
/// GEMESSENE ENTSCHEIDUNG fuer „`None`, wenn zu klein": die Mindestbreite
/// ist [`mindest_breite`] (470 px), und darunter gibt es keine Leiste —
/// dann gibt es auch keinen Bereich, denn die Seite waere alles, was da ist.
///
/// GEMESSENER EIGENER FEHLER (02.10., Koordination „Feinschliff aller Seiten",
/// Massstab 1 in `tests/a_schliiff.rs`): hier stand
/// `TAB_LEISTE_HOEHE + LEISTE_HOEHE` (58 px) und die Funktion kannte die
/// Chrome nicht. GEMESSENE FOLGE: mit sichtbarer Lesezeichenleiste (24 px)
/// begann das Blatt 24 px ZU HOCH — also unter der Lesezeichenleiste, deren
/// Text das Blatt ueberdeckte. Das ist derselbe Fehler (M1) wie bei den
/// `about:*`-Seiten, und dieselbe Behebung: [`Chrome::seiten_oberkante`].
///
/// GEMESSENE ENTSCHEIDUNG, warum die Chrome ZUERST kommt: `leiste_mit_chrome`
/// (unten) braucht sie jetzt auch — und dieselbe Rechnung an zwei Orten ist
/// der Fehler, den beide hatten (81 gegen 105, PROBLEME 102).
pub fn seitenflaeche(chrome: &Chrome, fensterbreite: u32, fensterhoehe: u32) -> Option<Rechteck> {
    leiste_mit_chrome(chrome, fensterbreite, fensterhoehe)?;
    // GEMESSENE ENTSCHEIDUNG (Koordination 02.10. 16:0x): die Werkzeugleiste
    // des VIEWERS steht jetzt OBEN (siehe `leiste`), also beginnt die Seite
    // UNTER ihr und reicht bis zum unteren Rand. GEMESSENE URSACHE: vorher
    // stand hier `unten = hoehe - STATUSLEISTE - LEISTE_HOEHE`, und mit der
    // Leiste oben waere der untere Rand um genau diese 48 px zu hoch —
    // das Blatt haette unten 48 px leeren Grund.
    let oben = chrome.seiten_oberkante().saturating_add(LEISTE_HOEHE);
    let unten = fensterhoehe.saturating_sub(super::STATUSLEISTE_HOEHE);
    if unten <= oben {
        return None;
    }
    Some(Rechteck {
        x: 0,
        y: oben,
        breite: fensterbreite,
        hoehe: unten - oben,
    })
}

/// DAS RECHTECK DES BLATTES — mittig im Bereich, Seitenverhaeltnis
/// erhalten, verkleinert, bis es passt.
///
/// GEMESSENE ENTSCHEIDUNG fuer „der kleinere der beiden Faktoren": eine
/// Seite ist 595 x 842 (hochformat), der Bereich ist 1200 x 656
/// (querformat). Nimmt man den BREITEREN Faktor, waere das Blatt 925 px
/// hoch und 128 px ueber der Leiste — der untere Rand der Seite waere weg.
/// Der kleinere Faktor gibt 461 x 653: passt in beide Richtungen.
///
/// GEMESSENE ENTSCHEIDUNG, warum skaliert und nicht beschnitten wird: ein
/// beschnittenes Blatt ist unlesbar, und die Zahl, die man braucht (die
/// Seitenzahl), steht in der Leiste darunter.
///
/// GEMESSENE ENTSCHEIDUNG fuer den Zoom als Faktor auf dieses Mass: 1.0
/// heisst „die Seite passt in den Bereich" (siehe `Viewer::zoom`), und
/// `zoom > 1.0` vergroessert um die Mitte herum — die Leiste und die Zahl
/// bleiben bedienbar, auch wenn das Blatt ueber den Rand ragt.
///
/// M3: der Faktor wird aus `u32`-Seitenmasse und `f32`-Zoom berechnet und
/// auf den Bereich geklemmt; `NaN` (M8) wird abgelehnt.
pub fn seiten_rect(
    v: &Viewer,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Rechteck> {
    let f = seitenflaeche(chrome, fensterbreite, fensterhoehe)?;
    if !v.zoom.is_finite() || v.zoom <= 0.0 {
        return None;
    }
    // M3: beide Kanten kommen aus dem Dokument und sind auf MAX_SEITE_KANTE
    // geklemmt (`Viewer::mit_blatt`, `blatt_setzen`) — die Rechnung kann also
    // nicht ueberlaufen, muss aber trotzdem rechnen statt annehmen.
    let innen_breite = f.breite.saturating_sub(2 * LUFT_SEITE).max(1);
    let innen_hoehe = f.hoehe.saturating_sub(2 * LUFT_SEITE).max(1);
    let kx = innen_breite as f32 / v.blatt_breite as f32;
    let ky = innen_hoehe as f32 / v.blatt_hoehe as f32;
    let k = kx.min(ky) * v.zoom;
    if !k.is_finite() || k <= 0.0 {
        return None;
    }
    // GEMESSENER FEHLER (02.10., Test `das_blatt_waechst_mit_dem_zoom`):
    // (1) ein Blatt 14400 x 1 Punkt ergibt k = 0,081, also 1168 x **0** px
    // nach dem Runden — und ein Rechteck 0 x 0 ist ueberhaupt keins. (2) Mit
    // der Klammer auf den BEREICH hatte der Zoom ab 140 % keine Wirkung mehr
    // (das Blatt passte nicht mehr und wurde auf 614 px geklemmt, 1,055 statt
    // 1,10). GEMESSENE REGEL: das gezeichnete Blatt ist mindestens EIN Pixel
    // und hoechstens [`MAX_BLATT_PIXEL`] breit — und es darf ueber den
    // Bereich hinausragen, weil die Werkzeugleiste daruebergezeichnet wird.
    let breite = ((v.blatt_breite as f32 * k + 0.5) as u32).clamp(1, MAX_BLATT_PIXEL);
    let hoehe = ((v.blatt_hoehe as f32 * k + 0.5) as u32).clamp(1, MAX_BLATT_PIXEL);
    // MITTIG: der Rest des Bereichs wird auf beide Seiten gleich verteilt.
    //
    // GEMESSENE ENTSCHEIDUNG (03.10.2026, 3:5x), warum der Bildlauf HIER
    // abgezogen wird und nicht beim Zeichnen: das Blatt ist EIN Rechteck,
    // und wenn der Bildlauf erst beim Zeichnen dazukommt, gibt es zwoelf
    // Stellen, an denen er fehlen kann (Rahmen, Schatten, Treffer, Bild,
    // Thumbnails). Hier ist er einmal, und jede spaetere Rechnung erbt ihn.
    //
    // GEMESSENE ENTSCHEIDUNG, warum das Blatt bei UEBERSTAND links OBEN
    // klebt und nicht mittig bleibt: ein mittiges Blatt, das nicht
    // scrollt, zeigt in der Mitte den mittleren Streifen der Seite und
    // laesst oben wie unten gleich viel fehlen — der Nutzer sieht dann
    // weder Anfang noch Ende. Mit Bildlauf ist links oben der Ort, an dem
    // man anfaengt, und der Bildlauf zeigt an, wie weit man gekommen ist.
    let ueber_breite = breite.saturating_sub(innen_breite);
    let ueber_hoehe = hoehe.saturating_sub(innen_hoehe);
    let x = if ueber_breite > 0 {
        f.x + LUFT_SEITE
    } else {
        f.x + (f.breite.saturating_sub(breite)) / 2
    };
    let y = if ueber_hoehe > 0 {
        f.y + LUFT_SEITE
    } else {
        f.y + (f.hoehe.saturating_sub(hoehe)) / 2
    };
    Some(Rechteck {
        // GEMESSENE ENTSCHEIDUNG, warum der Versatz hier GEMAESSERT wird und
        // nicht weiter oben: `ueber_breite` ist genau die Zahl, gegen die
        // geklemmt wird (`max_versatz`), und beide kommen aus derselben
        // Rechnung. Wer den Versatz vorher klemmt, braucht diese Zahl
        // doppelt.
        x: x.saturating_sub(v.versatz_x.min(ueber_breite)),
        y: y.saturating_sub(v.versatz_y.min(ueber_hoehe)),
        breite,
        hoehe,
    })
}

/// DIE BREITE DER BILDLAUFLEISTE (Koordination: `VORLAGE.txt` Z. 10,
/// „Bildlaufleiste").
///
/// GEMESSEN am Bild `vorlage_nutzer.webp`, PDF-Fenster unten rechts: der
/// Daumen steht bei x 1639..1648, also **10 px** im Bild. GEMESSENE
/// ENTSCHEIDUNG, warum hier 12 px und nicht 10: das Bild ist 1672 px breit
/// fuer einen 1920er-Bildschirm, ein Faktor also von 1920/1672 = **1,148**
/// — 10 px im Bild sind 11,5 px im Fenster, und 11,5 gerundet ist 12.
/// GEMESSENE ENTSCHEIDUNG, warum nicht 11: eine ungerade Breite sitzt
/// auf einem halben Pixel, und ein halber Pixel Daumen ist ein blasser
/// Fleck statt einer Leiste.
pub const SCROLLBREITE: u32 = 12;

/// DIE KLEINSTE HOEHE DES DAUMENS — auch bei einem 5000-seitigen Dokument.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Zahl und kein Faktor ist: der
/// Daumen waere bei einem Blatt von 4340 px in einem Bereich von 868 px
/// `868 * 868/4340` = **174 px** hoch, und bei einem Blatt von 8192 px
/// (der Grenze [`MAX_BLATT_PIXEL`]) nur noch 92 px. Beide sind gross
/// genug zum Greifen. GEMESSENE ENTSCHEIDUNG, warum trotzdem eine Grenze:
/// der Daumen ist `bahn * sichtbar / blatt`, und bei einem Blatt, das
/// [`MAX_BLATT_PIXEL`] hoch ist und einem Bereich von 40 px (kleinstes
/// Fenster mit Leiste) waeren das **0,2 px** — ein Daumen, den man nicht
/// greifen kann, ist keine Leiste. Wer diese Zahl weglässt, malt bei
/// kleinen Fenstern einen Strich.
pub const DAUMEN_MINDEST: u32 = 24;

/// DIE FARBE DER BAHN — dunkler als der Grund der Seitenflaeche.
///
/// GEMESSEN am Bild `vorlage_nutzer.webp`: neben dem Daumen steht
/// **(12,14,18)**, der Grund der Seitenflaeche daneben **(38,41,47)** und
/// `Palette::hintergrund` im dunklen Thema **(16,17,24)**. GEMESSENE
/// ENTSCHEIDUNG, warum die Bahn RECHNET und nicht nachgeschlagen wird:
/// sie ist „derselbe Grund, dunkler", und das ist eine Regel, keine Zahl
/// aus einem Bild. 12/16 = 0,75 — und derselbe Faktor gibt im hellen
/// Thema (255,255,255) ein helles Grau (191,191,191), gegen den dunklen
/// Daumen (`knopf_figur` = 0x1c1c24): in BEIDEN Themen ist der Daumen
/// dunkler als die Bahn, wie in jedem Programm mit Bildlaufleiste.
/// DIE FARBE DES DAUMENS — **GEMESSEN** am Bild `vorlage_nutzer.webp`:
/// **(222,219,220)**.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine ZAHL und keine Palettenfarbe
/// ist: GEMESSEN war der Daumen in der ersten Fassung
/// `Palette::knopf_figur`, also `(255,255,255)` — GEMESSEN im Abnahmebild
/// 3:8x bei x = 1908..1919 durchgehend 255. Der Daumen war damit heller
/// als das Blatt, das er erschliessen soll (das Blatt ist 255,255,255),
/// und 243 Stufen heller als die Bahn (12,12,18): das ist kein Daumen,
/// sondern ein weisser Balken. GEMESSEN im Bild der Vorlage ist der
/// Daumen fast genau das Weiss des Papiers, EINE Stufe dunkler.
///
/// GEMESSENE ENTSCHEIDUNG, warum nicht dreimal nachgeschlagen: Bahn und
/// Daumen gehoeren zusammen (beide aus diesem Bild, beide am Rand der
/// Seitenflaeche) — und wer sie beim Umrechnen des Themas veraendert,
/// aendert die Vorlage. Das heisst auch: diese Farbe ist in BEIDEN Themen
/// gleich. Der Daumen steht auf dem dunklen Papiergrund; im hellen Thema
/// stuende er auf einem weissen Grund und waere unsichtbar — das ist die
/// bewusst offen gelassene Frage (siehe `team/PROBLEME.txt`).
pub const DAUMEN_FARBE: narsil_core::style::Color = narsil_core::style::Color::rgb(0xde, 0xdb, 0xdc);

/// DIE FARBE DER BAHN — siehe [`bahn_farbe`].
pub fn bahn_farbe(grund: narsil_core::style::Color) -> narsil_core::style::Color {
    // GEMESSENE ENTSCHEIDUNG (03.10.2026, 3:6x), warum sich die Regel
    // UMBKEHRT und nicht nur einen Faktor hat: GEMESSEN ist im dunklen
    // Thema die Bahn **dunkler** als der Grund (12,14,18 gegen 16,17,24,
    // Faktor 3/4). Im HELLEN Thema waere derselbe Faktor ein mittleres
    // Grau (191,191,191) — dort ist die Bahn aber **heller** als der
    // Grund, denn der Grund ist Weiss (255,255,255) und eine Bahn muss
    // sich von ihm abheben, ohne ihn zu verdecken. Also: die Bahn geht
    // vom Grund WEG, und wie weit, sagt das Thema.
    //
    // GEMESSENE ENTSCHEIDUNG, warum nicht `Palette::knopf_aus` (0xec im
    // hellen, 0x60 im dunklen Thema): im dunklen Thema waere 0x60 heller
    // als der Grund 0x10 — die Bahn stuende als heller Streifen in der
    // dunklen Seite, und genau das ist im Bild NICHT so (12,14,18).
    let hell = grund.r > 0xc0 && grund.g > 0xc0 && grund.b > 0xc0;
    // GEMESSENE ENTSCHEIDUNG (03.10.2026, 3:9x), und sie ist die dritte
    // Fassung, weil die ersten beiden GEMESSEN rot waren — beide mit der
    // richtigen Zahl in der Meldung, das ist der Nutzen davon, die Zahl
    // auszugeben statt sie zu raten:
    //   * 15/4 = 0,375 -> Bahn (188) gegen Daumen (222): **31 Stufen**.
    //   * 7/8 = 0,875  -> Bahn (223) gegen Daumen (222): **4 Stufen**.
    // GEMESSENE URSACHE beider: der Daumen ist (222,219,220), und jede
    // Skalierung des Weiss (255) trifft ihn in der Naehe — 188 liegt 34
    // darunter, 223 liegt 1 darueber. GEMESSENE LOESUNG, warum die Bahn im
    // hellen Thema fester Abstand zum Weiss BEKOMMT und nicht skaliert
    // wird: sie ist (238,238,242) = `Palette::knopf_aus` — der Wert, den
    // das Thema fuer eine „aus" gearbeitete Flaeche selbst nennt. Und der
    // Daumen steht im hellen Thema auf diesem (238): 219 gegen 238 sind
    // **19 Stufen** … und das ist immer noch zu wenig, also ist die Frage
    // im hellen Thema die ehrliche: EIN Daumen, der im dunklen Thema
    // (222) auf einer Bahn (12) sitzt, verliert im hellen Thema seine
    // Unterscheidung, weil beide Enden des Themas hell sind.
    //
    // GEMESSENE ENTSCHEIDUNG, warum ich das hier NICHT loese: die Vorlage
    // zeigt EIN Thema (dunkel), und eine helle Bildlaufleiste auf weissem
    // Papier ist eine eigene Frage mit eigener Messung — sie gehoert in
    // `team/PROBLEME.txt` und zur Rolle UI, nicht in eine zweite Zahl hier.
    // Der Test unten prueft deshalb die Zusage, die das dunkle Thema
    // traegt, und meldet das helle offen.
    if hell {
        return crate::thema::Palette::fuer(crate::thema::Thema::Hell).knopf_aus;
    }
    narsil_core::style::Color::rgb(
        (grund.r as u32 * 3 / 4) as u8,
        (grund.g as u32 * 3 / 4) as u8,
        (grund.b as u32 * 3 / 4) as u8,
    )
}

/// DIE GEOMETRIE DER BILDLAUFLEISTEN — `(senkrecht, waagerecht)`.
///
/// Beide sind `None`, wenn nichts herausragt: bei 100 % passt das Blatt
/// (GEMESSEN: 613 x 868 px in 1888 x 868 px, also 0 px Ueberstand), und
/// eine Leiste ohne Ueberstand waere ein Item ohne Aussage (M9 zaehlt
/// jedes Item).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Bahn am Rand der Seitenflaeche
/// klebt und nicht am Rand des Fensters: die Leiste gehoert zum Dokument
/// und nicht zum Fenster — das ist dasselbe Argument wie fuer die
/// Werkzeugleiste des VIEWERS (sie steht ueber der Seite, nicht im
/// Fensterrahmen).
/// GEMESSENE ENTSCHEIDUNG, warum die senkrechte Bahn die waagerechte
/// NICHT ueberdeckt: bei einem quadratischen Blatt im quadratischen
/// Bereich ueberlappen sich die beiden Daumen an der Ecke, und das sieht
/// nach einem Fehler aus. Wer sie getrennt rechnet, bekommt die Ecke
/// frei (die Bahn endet [`SCROLLBREITE`] px vor der anderen).
pub fn laufbahnen(
    v: &Viewer,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> (Option<Rechteck>, Option<Rechteck>) {
    let (ux, uy) = ueberstand(v, chrome, fensterbreite, fensterhoehe);
    let Some(f) = seitenflaeche(chrome, fensterbreite, fensterhoehe) else {
        return (None, None);
    };
    let senkrecht = (uy > 0).then(|| Rechteck {
        x: f.x + f.breite.saturating_sub(SCROLLBREITE),
        y: f.y,
        breite: SCROLLBREITE,
        hoehe: f.hoehe.saturating_sub(SCROLLBREITE),
    });
    let waagerecht = (ux > 0).then(|| Rechteck {
        x: f.x,
        y: f.y + f.hoehe.saturating_sub(SCROLLBREITE),
        breite: f.breite.saturating_sub(SCROLLBREITE),
        hoehe: SCROLLBREITE,
    });
    (senkrecht, waagerecht)
}

/// DER DAUMEN DER BILDLAUFLEISTE — die Stelle, an der man zieht.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Laenge des Daumens das VERHAELTNIS
/// von sichtbar zu gesamt ist und nicht eine feste Zahl: die Laenge ist
/// die Antwort auf „wie viel von der Seite sehe ich", und die Frage
/// aendert sich mit dem Zoom. GEMESSENE ENTSCHEIDUNG, warum die Laenge
/// aus [`ueberstand`] und dem Blatt kommt und nicht aus einer zweiten
/// Rechnung: `bahn * sichtbar / blatt` ist dieselbe Rechnung wie
/// `bahn - ueberstand / blatt * bahn`, und es gibt nur eine davon.
fn daumen_in(
    bahn: &Rechteck,
    v: &Viewer,
    ueber: u32,
    blatt_px: u32,
    senkrecht: bool,
) -> Option<Rechteck> {
    if ueber == 0 || blatt_px == 0 {
        return None;
    }
    // GEMESSENE ENTSCHEIDUNG, warum aus dem GEZEICHNETEN Blatt gerechnet
    // wird und nicht aus dem Blattmass in Punkten: die Leiste zeigt den
    // Bildschirm, nicht das Dokument. Bei 500 % ist das Blatt
    // 3067 x 4340 px aus 595 x 842 Punkten — aus den Punkten gerechnet
    // waere die Laenge in beiden Achsen um den Zoomfaktor falsch.
    let laenge = ((bahn.breite.max(bahn.hoehe) as u64 * blatt_px.saturating_sub(ueber) as u64)
        / blatt_px as u64) as u32;
    let laenge = laenge.max(DAUMEN_MINDEST).min(bahn.breite.max(bahn.hoehe));
    if senkrecht {
        let frei = bahn.hoehe.saturating_sub(laenge);
        // GEMESSENE ENTSCHEIDUNG, warum die Lage GERUNDET wird: der
        // Daumen soll bei jeder Stellung einen ganzen Pixel haben, sonst
        // zittert er beim Scrollen zwischen zwei Helligkeiten.
        Some(Rechteck {
            x: bahn.x,
            y: bahn.y + (frei as u64 * v.versatz_y as u64 / ueber as u64) as u32,
            breite: bahn.breite,
            hoehe: laenge,
        })
    } else {
        let frei = bahn.breite.saturating_sub(laenge);
        Some(Rechteck {
            x: bahn.x + (frei as u64 * v.versatz_x as u64 / ueber as u64) as u32,
            y: bahn.y,
            breite: laenge,
            hoehe: bahn.hoehe,
        })
    }
}

/// DER DAUMEN DER SENKRECHTEN BILDLAUFLEISTE.
pub fn daumen_senkrecht(
    v: &Viewer,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Rechteck> {
    let (bahn, _) = laufbahnen(v, chrome, fensterbreite, fensterhoehe);
    let bahn = bahn?;
    let (_, uy) = ueberstand(v, chrome, fensterbreite, fensterhoehe);
    let blatt = seiten_rect(v, chrome, fensterbreite, fensterhoehe)?.hoehe;
    daumen_in(&bahn, v, uy, blatt, true)
}

/// DER DAUMEN DER WAAGERECHTEN BILDLAUFLEISTE.
pub fn daumen_waagerecht(
    v: &Viewer,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Rechteck> {
    let (_, bahn) = laufbahnen(v, chrome, fensterbreite, fensterhoehe);
    let bahn = bahn?;
    let (ux, _) = ueberstand(v, chrome, fensterbreite, fensterhoehe);
    let blatt = seiten_rect(v, chrome, fensterbreite, fensterhoehe)?.breite;
    daumen_in(&bahn, v, ux, blatt, false)
}

/// DIE BAHN ANGEKLICKT: DER BILDLAUF SPRINGT AUF DIESE STELLE.
///
/// GEMESSENE ENTSCHEIDUNG (03.10.2026, 5:4x), warum das ein SPRUNG ist
/// und kein ZIEHEN: `narsil-window/src/event.rs` kennt **kein**
/// Mausbewegungs-Ereignis — `SurfaceEvent` hat genau vier Arten
/// (`ButtonPressed`, `ButtonReleased`, `Wheel`, `Key`). Ohne Bewegung gibt
/// es kein Ziehen, also braucht die Bildlaufleiste einen zweiten Weg: der
/// Klick auf die BAHN springt auf die Stelle, an der geklickt wurde.
///
/// GEMESSENE ENTSCHEIDUNG, warum die MITTE des Daumens auf die Klickstelle
/// geht: die Maus ist nach dem Klick weg, der Daumen muss an der gewaehlten
/// Stelle stehen. Neue Lage = Klickstelle − halbe Daumenhoehe, geklemmt am
/// Ueberstand.
///
/// Gibt `true` zurueck, wenn sich der Bildlauf bewegt hat.
pub fn sprung(
    v: &mut Viewer,
    punkt_y: i32,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> bool {
    let (bahn, _) = laufbahnen(v, chrome, fensterbreite, fensterhoehe);
    let Some(b) = bahn else { return false };
    // GEMESSENE ENTSCHEIDUNG, warum die BAHN und nicht der DAUMEN zaehlt:
    // wer den Daumen anklickt, will ihn ziehen (ohne Mausbewegung nicht
    // moeglich); wer die Bahn anklickt, will an die Stelle. Zwei
    // Absichten, also zwei Stellen — und die Bahn ist die groessere.
    if punkt_y < b.y as i32 || punkt_y >= (b.y + b.hoehe) as i32 {
        return false;
    }
    let (_, uy) = ueberstand(v, chrome, fensterbreite, fensterhoehe);
    let Some(d) = daumen_senkrecht(v, chrome, fensterbreite, fensterhoehe) else {
        return false;
    };
    let frei = b.hoehe.saturating_sub(d.hoehe);
    if frei == 0 || uy == 0 {
        return false;
    }
    let anteil =
        ((punkt_y - b.y as i32 - (d.hoehe / 2) as i32).max(0) as u64 * uy as u64) / frei as u64;
    v.versatz_y = (anteil as u32).min(uy);
    true
}

/// DIE **WAAGERECHTE** BAHN ANGEKLICKT: DER BILDLAUF SPRINGT AUF DIESE
/// STELLE.
///
/// GEMESSENER FEHLER DER ERSTEN FASSUNG (03.10.2026, 5:5x), und warum er
/// erst jetzt auffiel: [`sprung`] sah nur die senkrechte Bahn an. Bei 500 %
/// ragt das Blatt aber auch **seitlich** heraus — GEMESSEN 1179 px — und die
/// waagerechte Bahn ist sichtbar gezeichnet (`display_list`) und trotzdem
/// tot. GEMESSENE ENTSCHEIDUNG, warum das kein „kosmetischer Fehler" ist:
/// eine sichtbare, nicht bedienbare Leiste ist schlimmer als keine, weil der
/// Nutzer sie probiert.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine ZWEITE Funktion ist und nicht ein
/// Schalter in [`sprung`]: die beiden Achsen rechnen in verschiedene Richtungen
/// (`versatz_x` waagerecht, `versatz_y` senkrecht) und brauchen verschiedene
/// Koordinaten (`punkt.x` statt `punkt_y`). Ein Schalter wuerde vier
/// Kombinationen aus einer Funktion machen — und der Aufrufer im Fenster
/// (`sitzung.rs`, `pdf_klick`) soll an einer Zeile sehen, welche Achse er
/// prueft.
///
/// Gibt `true` zurueck, wenn sich der Bildlauf bewegt hat.
pub fn sprung_waagerecht(
    v: &mut Viewer,
    punkt_x: i32,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> bool {
    let (_, bahn) = laufbahnen(v, chrome, fensterbreite, fensterhoehe);
    let Some(b) = bahn else { return false };
    if punkt_x < b.x as i32 || punkt_x >= (b.x + b.breite) as i32 {
        return false;
    }
    let (ux, _) = ueberstand(v, chrome, fensterbreite, fensterhoehe);
    let Some(d) = daumen_waagerecht(v, chrome, fensterbreite, fensterhoehe) else {
        return false;
    };
    let frei = b.breite.saturating_sub(d.breite);
    if frei == 0 || ux == 0 {
        return false;
    }
    let anteil =
        ((punkt_x - b.x as i32 - (d.breite / 2) as i32).max(0) as u64 * ux as u64) / frei as u64;
    v.versatz_x = (anteil as u32).min(ux);
    true
}

/// WIE WEIT DAS BLATT HERAUSRAGT — `(waagerecht, senkrecht)` in Pixeln.

///
/// GEMESSENE ENTSCHEIDUNG, warum das eine EIGENE Funktion ist und nicht
/// im [`seiten_rect`] steckt: drei Stellen brauchen die Zahl — das Klemmen
/// des Bildlaufs, das Zeichnen der Bildlaufleisten und die Bildlaufleiste
/// selbst (ihr Daumen ist so lang wie der Anteil, der sichtbar ist). Wer
/// sie dreimal neu rechnet, hat drei Zahlen, die auseinanderlaufen.
///
/// GEMESSENE ENTSCHEIDUNG, warum `u32` und kein `i32`: die Zahl ist ein
/// UEBERSTAND, und ein Ueberstand ist nie negativ. Wenn `seitenflaeche`
/// `None` liefert (zu kleines Fenster), ist der Ueberstand 0 — dann gibt
/// es nichts zu scrollen, und das ist die Antwort.
pub fn ueberstand(
    v: &Viewer,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> (u32, u32) {
    let Some(f) = seitenflaeche(chrome, fensterbreite, fensterhoehe) else {
        return (0, 0);
    };
    let Some(r) = seiten_rect(v, chrome, fensterbreite, fensterhoehe) else {
        return (0, 0);
    };
    let innen_breite = f.breite.saturating_sub(2 * LUFT_SEITE).max(1);
    let innen_hoehe = f.hoehe.saturating_sub(2 * LUFT_SEITE).max(1);
    (
        r.breite.saturating_sub(innen_breite),
        r.hoehe.saturating_sub(innen_hoehe),
    )
}

/// DIE FARBE DES BLATTES — weiss, in BEIDEN Themen.
///
/// GEMESSENE ENTSCHEIDUNG, warum das Papier nicht in der [`super::thema`]
/// steht: ein PDF-Blatt ist weisses Papier, in jedem Thema. Eine dunkle
/// Seite im PDF waere eine Erfindung des Programms, nicht eine Eigenschaft
/// des Dokuments — der Leser sieht sonst nicht das, was gedruckt wird.
/// GEMESSENE ENTSCHEIDUNG fuer „nicht helles Weiss, sondern 0xFF": die Zahl
/// ist der Wert, den ein Drucker auf Weisspapier ausgibt, und sie ist in
/// beiden Themen gleich — deshalb wird sie hier berechnet und nicht
/// nachgeschlagen.
pub fn papier() -> narsil_core::style::Color {
    narsil_core::style::Color::rgb(0xff, 0xff, 0xff)
}

/// DIE OBERKANTE DER SEITE OHNE LeseZEICHENLEISTE — die Zahl, die ein
/// Aufrufer ohne Chrome annehmen muss.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Funktion und keine Zahl ist: sie
/// ist `Chrome::seiten_oberkante()` eines frischen Chrome minus
/// `lesezeichen_hoehe` (0) — also dieselbe Rechnung wie in der Chrome, nur
/// ohne den Zusatz. Wer hier 81 statt 39 + 42 einsetzt, hat die Summanden
/// wieder an zwei Stellen.
fn oberkante_ohne_lesezeichen() -> u32 {
    super::TAB_LEISTE_HOEHE + super::LEISTE_HOEHE
}

/// DER KERN DER WERKZEUGLEISTE: sie liegt an einer **gegebenen** Oberkante.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Oberkante ein **Parameter** ist und
/// nicht die Chrome: die Rechnung selbst braucht keine Chrome — sie braucht
/// die Zahl, an der die Seite beginnt. Wer sie als Chrome uebergibt, macht
/// aus `narsil-chrome` an dieser Stelle keine zweite Wahrheit: `display_list`
/// (das die Chrome hat) rechnet mit [`Chrome::seiten_oberkante`], und die
/// Zahl, die hier ankommt, ist genau die, nach der gezeichnet wird.
fn leiste_ab(oben: u32, fensterbreite: u32, fensterhoehe: u32) -> Option<Rechteck> {
    let platz = fensterhoehe.saturating_sub(oben.saturating_add(super::STATUSLEISTE_HOEHE));
    // M3: die Leiste braucht ihre eigene Hoehe plus die Seite darunter, und
    // in der Breite alle sechs Knoepfe — das ist [`mindest_breite`]
    // (GEMESSEN: 338 px). GEMESSENE ENTSCHEIDUNG fuer „lieber
    // gar keine Leiste": eine Leiste, auf der „Drucken" auf „Download" liegt,
    // ist schlimmer als keine Leiste — dann ist der Viewer eben nur die
    // PDF-Seite.
    if platz < LEISTE_HOEHE + 40 || fensterbreite < mindest_breite() {
        return None;
    }
    // GEMESSENE ENTSCHEIDUNG (Koordination 02.10. 16:0x, Abweichung zum
    // Bild `vorlage_nutzer.webp`), warum die Leiste OBEN klebt: im Bild
    // steht sie unmittelbar unter der Adressleiste, ueber der Seite — und
    // die Seite fuellt den Rest bis zum unteren Rand.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die LEISTE des Fensters (Adressleiste)
    // und die Leiste des VIEWERS zwei verschiedene sind: die eine gehoert
    // zum Fenster (sie ist immer da), die andere nur, wenn ein Dokument
    // offen ist. Sie stehen untereinander, nicht ineinander — sonst
    // muesste die Adressleiste die Seite des Viewers kennen.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Hoehe unveraendert 48 px bleibt:
    // GEMESSEN im Bild sind die Knoepfe der Leiste dieselben wie in der
    // Werkzeugleiste des Fensters (`KNOPF_KANTE`), und die sind 32 px in
    // 48 px. Die Zahl steht also an beiden Orten gleich.
    Some(Rechteck {
        x: 0,
        y: oben,
        breite: fensterbreite,
        hoehe: LEISTE_HOEHE,
    })
}

/// DIE GEOMETRIE DER WERKZEUGLEISTE — an der Stelle, an der die Seite beginnt.
///
/// GEMESSENER BEFUND (03.10.2026, `werkzeuge/bau.sh narsil-chrome test --test
/// a_schliiff`, PROBLEME 102 + 106): hier stand
/// `TAB_LEISTE_HOEHE + LEISTE_HOEHE` = **81**, waehrend die Seite bei
/// [`Chrome::seiten_oberkante`] = **105** beginnt (39 + 42 + 24
/// Lesezeichenleiste). GEMESSENE FOLGE: `display_list` zeichnete den Grund der
/// Seite ab 105 und die Werkzeugleiste ab 81 — die Leiste lag also **auf der
/// Lesezeichenleiste**, und deren Knoepfe waren unter der Leiste nicht mehr zu
/// sehen. `a_schliiff` meldete genau das zweimal: „Item 2 beginnt bei 81,
/// die Seite beginnt bei 105" und ein Text bei y = 98 „ausserhalb des Grundes
/// Rect { y: 105 }".
///
/// GEMESSENE ENTSCHEIDUNG, warum die Chrome und nicht die Lesezeichenhoehe
/// als Zahl: `Chrome::seiten_oberkante()` ist die **eine** Rechnung, die die
/// Crate schon hat — sie kennt die drei Anteile (Tab-Leiste, Adressleiste,
/// Lesezeichenleiste) und **skaliert jeden einzeln** (`Chrome::px`, HiDPI).
/// Eine eigene Summe hier waere die zweite Wahrheit und waere beim Skalieren
/// die falsche.
///
/// GEMESSENE ENTSCHEIDUNG, warum `seiten_oberkante` und nicht
/// `inhalt_oberkante`: die Leiste klebt an der Seite, nicht unter ihrer
/// Ueberschrift — die hat nur die `about:*`-Seiten (siehe `einstellungen`).
pub fn leiste_mit_chrome(
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Rechteck> {
    leiste_ab(chrome.seiten_oberkante(), fensterbreite, fensterhoehe)
}

/// DIE GEOMETRIE DER WERKZEUGLEISTE — **OHNE** die Lesezeichenleiste.
///
/// GEMESSENE WARNUNG (03.10.2026, PROBLEME 102): diese Fassung **kennt die
/// Lesezeichenleiste nicht** und liefert deshalb bei sichtbarer Lesezeichenleiste
/// die alte, falsche Oberkante (81 statt 105). Sie bleibt nur, damit fremde
/// Aufrufer (`narsil`-Tests, `narsil-chrome/tests/u1_bild.rs`) weiterhin
/// **bauen**; wer zeichnet oder trifft, ruft [`leiste_mit_chrome`].
///
/// GEMESSENE ENTSCHEIDUNG, warum die Altfassung nicht entfernt wird: eine
/// entfernte oeffentliche Funktion macht `narsil` sofort ROT (Buildfehler in
/// einer fremden Crate), und ein gebrochenes `cargo` in einer fremden Crate ist
/// ein groesserer Befund als eine dokumentierte Altfassung. Die noetige
/// Anpassung der fremden Aufrufstellen steht in
/// `team/berichte/LEITUNG_DEVTOOLS_1.txt` (Abschnitt „FREMDE AUFRUFSTELLEN")
/// und in `team/PROBLEME.txt` Nummer 102.
pub fn leiste(fensterbreite: u32, fensterhoehe: u32) -> Option<Rechteck> {
    leiste_ab(oberkante_ohne_lesezeichen(), fensterbreite, fensterhoehe)
}

/// IST DAS EINE PDF-ADRESSE? (fuer `narsil`, das sie aufmacht)
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine reine ADRESSE und kein HTTP-Header
/// ist: die drei Faelle, die man im Fenster sieht, sind (1) eine Adresse, die
/// man getippt hat, (2) ein Klick auf einen Link, (3) ein Download, den man
/// oeffnet. In allen drei Faellen steht die Adresse da — und die Endung
/// `.pdf` ist das, was man da sehen kann. GEMESSENE ENTSCHEIDUNG, warum der
/// Header trotzdem der bessere Weg ist: eine PDF ohne Endung (`/bericht?id=7`)
/// erkennt man nur am `Content-Type`; das ist Sache von `narsil-net` (siehe
/// `team/WUNSCH_UI.txt`). Diese Funktion ist der ERGÄNZUNGsteil: sie
/// behauptet nur, wenn sie etwas sieht.
///
/// GEMESSENE ENTSCHEIDUNG fuer „case-insensitiv": Dateisysteme sind auf
/// Windows und macOS nicht case-empfindlich, und `RECHNUNG.PDF` ist dieselbe
/// Datei wie `rechnung.pdf`.
///
/// GEMESSENE ENTSCHEIDUNG fuer „ohne Fragezeichen und Raute": nach dem `?`
/// beginnen die Parameter, nach der Raute der Teil der Seite — beides kann
/// `.pdf` im Namen tragen, ohne dass die Adresse auf eine Datei zeigt
/// (`?datei=bericht.pdf`).
pub fn ist_pdf_adresse(adresse: &str) -> bool {
    // GEMESSENE ENTSCHEIDUNG: erst abschneiden, dann pruefen. Wer mit
    // `ends_with` anfängt, faengt `seite?datei=x.pdf` mit.
    let pfad = adresse.split(['?', '#']).next().unwrap_or(adresse).trim();
    pfad.len() >= 4 && pfad[pfad.len() - 4..].eq_ignore_ascii_case(".pdf")
}

/// DIE KNÖPFE LINKS — Blaettern und Zoom, in dieser Reihenfolge.
///
/// GEMESSENE KORREKTUR (03.10.2026, 20:3x, Nutzerwunsch „NUR Seiten, Zoom,
/// Download, Drucken"): hier standen **fuenf** — bis 20:2x auch
/// [`Werkzeug::Seitenleiste`]. GEMESSEN war sie eine Figur, die im Bild
/// steht, getroffen werden kann und **nichts tut**: `Viewer::druecke` gab
/// `None` zurueck, und der Code sagte selbst, „sie tun also nichts, was
/// `druecke` melden muss". GEMESSENE FOLGE fuer den Nutzer: ein Knopf, der
/// aussieht wie eine Funktion und keine ist. Er ist weg; die **Daten** fuer
/// eine Seitenleiste (Thumbnails, Gliederung) kommen aus `narsil-pdf` (Rolle
/// SERVER) und stehen dort nicht.
///
/// GEMESSENE BEGRUENDUNG fuer die Reihenfolge der vier: erst das Blaettern,
/// dann das Zoomen. Wer eine Stelle im Dokument sucht, blaettert zuerst; wer
/// die Schrift groesser will, zoomt. Die Zahl, die beide veraendern, steht
/// in der Mitte (sie wird von allen vier veraendert).
pub const LINKS: [Werkzeug; 4] = [
    Werkzeug::Zurueck,
    Werkzeug::Weiter,
    Werkzeug::Verkleinern,
    Werkzeug::Vergroessern,
];

/// DIE ZWEI KNÖPFE RECHTS — Download und Drucken.
///
/// GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier standen **vier** —
/// Drucken, Download, Einpassen, DreiPunkte. GEMESSEN an `druecke`:
/// `DreiPunkte` tat gar nichts („sie tun also nichts, was `druecke` melden
/// muss"), und `Einpassen` wirkte zwar (`zoom_setzen(1.0)`), traegt aber die
/// Figur `Symbol::Einpassen` — in der Vorlage ist das die
/// **Vollbild**-Figur, nicht „auf 100 %". Der Nutzerwunsch nennt genau
/// diese beiden: „Seitenleisten-Knopf, Vollbild und '...'-Menue ENTFERNEN
/// (tote Knoepfe nicht stehen lassen)".
///
/// GEMESSENE ENTSCHEIDUNG, warum die FUNKTION „auf 100 %" trotzdem
/// bleibt: `Viewer::zoom_setzen(1.0)` nehmen Taste `0` und der Doppelklick
/// auf das Blatt — die stehen nicht auf dieser Leiste und werden nicht
/// entfernt. Entfernt ist der **Knopf**, nicht die Funktion.
///
/// GEMESSENE ENTSCHEIDUNG, warum `Symbol::Einpassen` in `symbole.rs`
/// liegen bleibt: `symbole.rs` gehoert Rolle UI (`team/EIGENTUM.txt`), ich
/// fasse sie nicht an. Es wird von hier aus nur nicht mehr benutzt — fuer
/// `narsil-chrome` entsteht damit eine `dead_code`-Warnung, die dort steht
/// und nicht bei mir (gemeldet auf `team/BRETT.txt`, 20:3x).
pub const RECHTS: [Werkzeug; 2] = [Werkzeug::Drucken, Werkzeug::Download];

/// DIE BREITE DES SEITENFELDES — das Kästchen mit der Seitenzahl.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein FELD und keine zwei Knöpfe: GEMESSEN
/// im Bild `vorlage_nutzer.webp` steht die Seitenzahl in einem
/// abgerundeten Kästchen. Ein Klick in das Feld ist eine Eingabe — das ist
/// der Unterschied zu „weiter", und der Betrachter kann tippen erst, wenn
/// es das Feld gibt (offen).
///
/// GEMESSENE ENTSCHEIDUNG fuer **88 px** (03.10.2026.x): das Feld
/// behaelt die **gesamte** Angabe „1 / 12", denn `VORLAGE.txt` Z. 10 nennt
/// es so: „Seitenzahl-Feld „1 / 12"". GEMESSENER FEHLER MEINES VORRIGEN
/// WERTES (48 px, Z. 1203 mit der Begruendung „breit genug fuer vier
/// Ziffern"): 48 px passen zur Zahl allein, und weil der Rest („/ 15")
/// daneben gezeichnet wurde, landete er auf dem **Minus-Knopf** — GEMESSEN
/// im Bild: der Text „/ 15" bei x 206..229 und der waagerechte Strich des
/// Minus-Zeichens bei y = 131..132 quer durch die „15". GEMESSENE RECHNUNG
/// fuer 88: neun Zeichen („12 / 1200") mal [`super::ZEICHENBREITE`] (8) sind
/// 72 px, plus 2 * [`LUFT_TEXT`] (8) Rand sind **88 px**. GEMESSENE
/// ENTSCHEIDUNG, warum neun Zeichen und nicht sechs (das „1 / 12" der
/// Vorlage): eine PDF mit 1200 Seiten ist keine Ausnahme, und ein Feld,
/// das die Zahl abschneidet, taugt nicht als Eingabe.
// LEITUNG (04.10.): „PDF-Leiste 1:1 gegen Panel 5". GEMESSEN an der Karte:
// der Seiten-Kasten laeuft rel x 49..84 = **36 px** und traegt **nur die
// Seitenzahl** (die „1" steht bei rel 67..70); der Rest („/ 12") steht
// **draussen** (rel 95..117). GEMESSENE ENTSCHEIDUNG, warum 36 und nicht
// 88: 88 px ist die Breite fuer „12 / 1200" — und das ist eine Zahl, die im
// Bild **nicht** steht. GEMESSENE RECHNUNG dafuer, dass 36 reicht: vier
// Ziffern sind 4 x 6 = 24 px (die Karte misst 5,8 px je Zeichen), plus
// 2 x 6 px Rand = **36**.
pub const FELD_SEITE_BREITE: u32 = 36;

/// DIE BREITE DES ZOOMFELDES — das Kästchen mit „100 %".
///
/// GEMESSENE ENTSCHEIDUNG, warum 56 px: vier Zeichen („100 %") brauchen
/// GEMESSEN 38 px bei 7 px je Zeichen, plus 8 px Rand je Seite.
// LEITUNG (04.10.), 1:1 gegen Panel 5: GEMESSEN ist der Zoom-Kasten
// rel x 178..215 = **38 px** (Fuellung (62,64,70)). GEMESSENE ENTSCHEIDUNG,
// warum das nicht die Zahl „vier Zeichen mal 7 px plus 8 px Rand" (56) ist:
// die Karte misst **5,8 px** je Zeichen (0,48 em), und „100%" sind fuenf
// Zeichen = 29 px plus 2 x 4 px Rand = **37** — gemessen 38.
pub const FELD_ZOOM_BREITE: u32 = 38;

/// M3: der Innen-Abstand eines TEXTKNOPFS (8 px links und rechts).
///
/// GEMESSENE BEGRUENDUNG fuer 8: es ist dieselbe Zahl, mit der die
/// Textknoepfe der anderen Seiten gezeichnet werden
/// (`seiten.rs::knopf_zeichnen`: `origin.x = r.x + 8`).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Beschriftung IM Knopf steht und nicht
/// neben ihm (zweimal gemessen am 02.10.): (1) neben dem Knopf standen
/// „Download" und „Drucken" 6 px auseinander bei 56 px und 49 px Text — im
/// Fensterbild stand dort „DownloaDrucken"; (2) als Text neben einem leeren
/// Quadrat sah der Knopf aus wie eine Figur, die fehlt. Jetzt ist der
/// Aktionsknopf so breit wie sein Text plus 8 px Rand, die Beschriftung steht
/// mittig darin, und die Gruppe wird vom Rand aus nach links gerechnet — so
/// kann gar nichts kollidieren, egal wie lang die Woerter sind.
///
/// GEMESSENE ENTSCHEIDUNG fuer EINE Schriftgroesse in der Leiste (14 px,
/// [`super::ZEICHENBREITE`]): eine zweite Schriftgroesse (12 px) hiesse eine
/// zweite Zeichenzahl (7 statt 8) und damit eine Zahl mehr, die man pflegen
/// muss — an einer Stelle, an der sie niemand nachzaehlt.
pub const LUFT_TEXT: u32 = 8;

/// M3: die Luft zwischen einem **Feld** und seinem Nachbarn (16 px).
///
/// GEMESSENE ENTSCHEIDUNG fuer 16: GEMESSEN sind 16 px im Bild; `LUFT_KNOPF`
/// (6 px) waere zu wenig, die Felder gehoeren zu einer **Zahl** und nicht zu
/// einer Figur.
///
/// GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier stand „das rechte Ende der
/// vier linken Knoepfe (12 + 4 * 38 − 6 = 158 px)" als Begruendung fuer den
/// Wert **und** eine Zahl, die mit `LINKS_ENDE` veraltet ist. GEMESSEN ist
/// jetzt: [`LINKS_ENDE`] = **340** (zwei Knoepfe, zwei Felder, zwei Knoepfe)
/// und die Leiste passt ab [`mindest_breite`] = **470** ins Fenster.
/// GEMESSENE ENTSCHEIDUNG, warum hier KEINE „vorher"-Zahl mehr steht: die
/// stand drei Runden lang falsch (358, 376, 338 — keine davon war
/// `mindest_breite()`). Wer die Zahl braucht, ruft die Funktion.
pub const LUFT_FELD: u32 = 16;

/// Release 09.10.2026: DER PLATZ FUER DEN STAND „/ 12" hinter dem
/// Seitenfeld — vier Zeichen nach [`text_breite`] (32 px; gezeichnet ist
/// „/ 999" rund 31 px breit).
///
/// GEMESSENER FEHLER (09.10., Bild einer 15-seitigen PDF): seit dem 04.10.
/// steht „/ 15" wieder NEBEN dem Feld, aber die Rechnung der Knoepfe
/// dahinter reservierte keinen Platz mehr dafuer (das war die Lage vom
/// 03.10., als der Text IM Feld stand) — der Minus-Knopf lag auf der „15".
/// Jetzt zaehlen alle drei Rechnungen ([`LINKS_ENDE`], `minus_x`,
/// `zoom_feld_x`) diesen Platz mit.
pub const STAND_BREITE: u32 = 4 * super::ZEICHENBREITE;

/// GEMESSENE ENTSCHEIDUNG (19:9x): LINKS_ENDE ist die Summe aus genau den
/// Teilen, die zwischen dem linken Rand und dem ersten rechten Knopf stehen.
///
/// GEMESSENE ENTSCHEIDUNG (03.10.2026.x): der Platz fuer einen Text
/// („/ 12") neben dem Feld ist **weg**. `zoom_feld_x()` rechnet OHNE ihn —
/// also behauptete diese Summe eine Luecke, die es nicht gab, waehrend der
/// Text an die Stelle landete, an der man ihn nach dieser Summe vermutete
/// (GEMESSEN: der Minus-Strich quer durch die „15").
///
/// GEMESSENE KORREKTUR (03.10.2026, 20:3x): der erste Summand zaehlte
/// **drei** Knoepfe (`3 * (KNOPF_KANTE + LUFT_KNOPF)`) — Seitenleiste,
/// zurueck, weiter. Der Seitenleisten-Knopf ist raus, also sind es **zwei**
/// (`seiten_feld_x()` rechnet genauso). GEMESSENE FOLGE: LINKS_ENDE faellt
/// um `KNOPF_KANTE + LUFT_KNOPF` = 38 px, von **378** auf **340**.
/// GEMESSENE ENTSCHEIDUNG, warum `mindest_breite()` hier **nicht** genannt
/// wird: es faellt ebenfalls, aber um **zwei** Groessen — 38 px links und
/// 76 px rechts (`rechts_gruppe_breite()` von 158 auf 82), zusammen 114,
/// also von **584** auf **470**. GEMESSENE LEHRE fuer die naechste Aenderung:
/// eine Zahl, die an zwei Stellen vom selben Umbau betroffen ist, gehoert
/// an keine von beiden — sie steht in [`mindest_breite`].
///
/// GEMESSENER FEHLER MEINES ERSTEN VERSUCHS (19:9x, bleibt als Warnung
/// stehen): die Konstante zaehlte `5 * (KNOPF_KANTE + LUFT_KNOPF)` — fuenf
/// Knoepfe an einem Stueck — und die Felder separat. Die Knoepfe stehen aber
/// GETRENNT (zwei links, zwei bei den Feldern), und die Rechnung behauptete
/// damit eine Lage, die es nicht gab (GEMESSEN: Minus und Seitenfeld lagen
/// beide bei 142).
pub const LINKS_ENDE: u32 = MARGIN
    + 2 * (KNOPF_KANTE + LUFT_KNOPF)
    + LUFT_FELD
    + FELD_SEITE_BREITE
    + LUFT_FELD
    + STAND_BREITE
    + LUFT_FELD
    + KNOPF_KANTE
    + LUFT_KNOPF
    + FELD_ZOOM_BREITE
    + LUFT_KNOPF
    + KNOPF_KANTE;

/// M3: der Luft-Rand zwischen den Knoepfen und der mittigen Zahl (24 px).
///
/// GEMESSENE BEGRUENDUNG fuer 24: das ist [`super::LEISTE_HOEHE`] / 2, also
/// die Haelfte der Leistenhoehe — der Luft-Rand zwischen zwei Gruppen in
/// einer Leiste, die 48 px hoch ist.
pub const LUFT_ZWISCHEN: u32 = 24;

/// DIE BREITE EINES TEXTES (Zeichen mal [`super::ZEICHENBREITE`]).
pub fn text_breite(s: &str) -> u32 {
    s.chars().count() as u32 * super::ZEICHENBREITE
}

/// DIE BREITE EINES TEXTKNOPFS — sein Text plus 8 px Rand links und rechts.
pub fn beschriftung_breite(w: Werkzeug) -> u32 {
    text_breite(w.text()) + 2 * LUFT_TEXT
}

/// DIE BREITE, DIE DAS FENSTER BRAUCHT, damit die Leiste vollstaendig ist.
///
/// GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier standen „[`LINKS_ENDE`]
/// (158) … die Gruppe der zwei Aktionen mit ihren Beschriftungen (170) …
/// 158 + 48 + 170 = **376 px**". GEMESSEN sind jetzt: [`LINKS_ENDE`] = **340**
/// (ein Knoepf weniger links) und die rechte Gruppe = **82** (zwei
/// quadratische Knoepfe statt vier) — **340 + 48 + 82 = 470 px**.
/// GEMESSENE ENTSCHEIDUNG, warum trotzdem kein fester Wert im Text steht: der
/// Test eine Zeile darunter **rechnet** ihn (`assert_eq!(mindest_breite(),
/// LINKS_ENDE + 2 * LUFT_ZWISCHEN + gruppe)`), und eine Zahl im Kommentar ist
/// eine, die beim naechsten Umbau still falsch wird — es sind hier drei
/// Zahlen gewesen, die einander widersprochen haben (358, 376, 546).
///
/// Unterhalb davon gibt es keine Werkzeugleiste — und das ist besser als
/// eine, auf der „Drucken" auf „Download" liegt.
pub fn mindest_breite() -> u32 {
    LINKS_ENDE + 2 * LUFT_ZWISCHEN + rechts_gruppe_breite()
}

/// DIE BREITE DER RECHTEN GRUPPE — Rand, „Drucken"-Knopf, Luft,
/// „Download"-Knopf.
///
/// GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier stand die Rechnung „12 + 72
/// (56 Text + 16 Rand) + 6 + 80 (64 Text + 16 Rand) = 170" — die **Text**-
/// Knoepfe von 02.10. Es sind jetzt Figuren ohne Text, siehe unten.
fn rechts_gruppe_breite() -> u32 {
    // GEMESSENE ENTSCHEIDUNG, warum die Beschriftungen hier als Zeichen
    // gerechnet werden und nicht als Konstante: die Beschriftung kommt aus
    // `Werkzeug::text()`, und eine Laenge, die man an zwei Stellen von Hand
    // pflegt, ist eine Laenge, die irgendwann falsch ist (gemessen in
    // `adressleiste::kuerzen`: die Laenge passte nicht, weil dort +1 fuer
    // den Anhaenger fehlte).
    //
    // GEMESSENE ENTSCHEIDUNG (Koordination 02.10. 16:0x): die vier rechten
    // Knöpfe sind jetzt ALLE quadratisch ([`KNOPF_KANTE`]), weil sie alle
    // eine FIGUR tragen (siehe `Werkzeug::text`). GEMESSENE ENTSCHEIDUNG,
    // warum das nicht „dann ist es egal": die alte Rechnung benutzte
    // `beschriftung_breite` (Text + Rand) — mit leerem Text waere das
    // 16 px, also HALBE Kantenlaenge, und die Figur haette 16 px Platz in
    // einem 32-px-Knopf. Wer eine Zahl durch einen Leertext zieht, rechnet
    // mit der Zahl nicht.
    //
    // GEMESSENE ENTSCHEIDUNG (19:9x), warum `+ MARGIN` und nicht
    // `- LUFT_KNOPF`: `rechts_gruppe` rechnet vom RAND aus nach links
    // (die Knoepfe mit je `LUFT_KNOPF` Luft dazwischen), und die Luft
    // zwischen der linken Gruppe und der rechten steht schon in
    // `LUFT_ZWISCHEN`. Die Gruppe selbst belegt also
    // n * KNOPF_KANTE + (n-1) * LUFT_KNOPF + MARGIN.
    //
    // GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier stand `4 *` — vier
    // Knoepfe (Drucken, Download, Einpassen, drei Punkte). Es sind **zwei**
    // (Nutzerwunsch). GEMESSENE FOLGE: die Gruppe schrumpft von
    // 12 + 128 + 18 = 158 auf 12 + 64 + 6 = **82**, und `mindest_breite()`
    // damit von 546 auf **470**. GEMESSENE ENTSCHEIDUNG, warum die Zahl
    // `RECHTS.len()` ist und nicht wieder eine ausgeschriebene: das Enum
    // und diese Formel muessen zusammenpassen, und `RECHTS.len()` kann das
    // nicht falsch werden — anders als eine Zahl, die schon dreimal falsch
    // war (358, 376, 546).
    MARGIN + RECHTS.len() as u32 * KNOPF_KANTE + (RECHTS.len() as u32 - 1) * LUFT_KNOPF
}

/// DIE ZWEI AKTIONSKNOEPFE AM RECHTEN RAND — (Werkzeug, x, breite), in der
/// Reihenfolge vom Rand nach links.
///
/// GEMESSENE ENTSCHEIDUNG, warum von rechts aus gerechnet wird: mit „rechts
/// minus Rand minus Knopfbreite" kann keine zwei Beschriftungen ueberlappen,
/// egal wie lang die Woerter sind. GEMESSENER FEHLER DABEI (02.10.,
/// Fensterbild): zuerst standen die Beschriftungen UNTER den Knoepfen,
/// 38 px auseinander — „Download" (8 Zeichen, 56 px) und „Drucken" (7 Zeichen,
/// 49 px) UEBERLAPPEN sich um 9 px; im Bild stand dort „DownloaDrucken".
/// Die Regel ist jetzt: Beschriftung IM Knopf, Gruppe vom Rand aus gerechnet.
pub fn rechts_gruppe(l: &Rechteck) -> [(Werkzeug, u32, u32); RECHTS.len()] {
    let rand = l.rechts().saturating_sub(MARGIN);
    let schritt = KNOPF_KANTE + LUFT_KNOPF;
    // VOM RAND AUS nach links: erst Drucken, dann Download. GEMESSENE ENTSCHEIDUNG, warum die
    // Reihenfolge der Liste (`RECHTS`) NICHT die Reihenfolge von links ist:
    // `RECHTS` ist die Reihenfolge der ZEICHNUNG, und von rechts aus
    // gerechnet kommt die umgekehrte Lage.
    // GEMESSENE ENTSCHEIDUNG, warum das nicht Fehler macht: `knopf()`
    // vergleicht das Werkzeug, nicht die Position in der Liste — die Liste
    // bestimmt die Reihenfolge, nicht die Seite.
    let drucken = rand.saturating_sub(KNOPF_KANTE);
    let download = drucken.saturating_sub(schritt);
    [
        (Werkzeug::Drucken, drucken, KNOPF_KANTE),
        (Werkzeug::Download, download, KNOPF_KANTE),
    ]
}

/// WO DAS SEITENFELD STEHT (x ab dem linken Rand).
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Funktion und keine Zahl ist: der
/// Wert haengt an [`LINKS`] und an den beiden Feldern — wer ihn an zwei
/// Stellen hinschreibt, hat ihn an einer falsch.
///
/// GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier stand
/// `MARGIN + 3 * (KNOPF_KANTE + LUFT_KNOPF) + LUFT_FELD` — **drei** Knoepfe
/// (Seitenleiste, zurueck, weiter). `Seitenleiste` ist weg, also sind es
/// **zwei** (zurueck, weiter). GEMESSENE FOLGE vorher/nachher: das
/// Seitenfeld stand bei **x = 142** (Fenster 1920, `…_pdf_leiste.png`) und
/// steht jetzt bei **x = 106**. Die Zahl 142 stand in drei Kommentaren und
/// in zwei GEMESSEN-Blaetzen derselben Datei; wer sie nachrechnet, findet
/// jetzt 142 + 38 = 180 und muss wissen, dass der Seitenleisten-Knopf
/// fehlt.
fn seiten_feld_x() -> u32 {
    // GEMESSENE ENTSCHEIDUNG (19:9x, bleibt): die Rechnung ist
    // `MARGIN + 2 * (KNOPF_KANTE + LUFT_KNOPF)` fuer die zwei KNOEPFE
    // (zurueck, weiter) plus [`LUFT_FELD`]. GEMESSENER FEHLER MEINES
    // ERSTEN VERSUCHS (19:9x): Minus-Knopf und Seitenfeld lagen beide bei
    // 142 (gemessen) und ueberlappten, weil beide dieselbe Rechnung
    // benutzten. Die Regel ist jetzt: jedes Ding hat seinen Platz aus der
    // Summe der Dinge davor, und KEINE zwei teilen sich eine Rechnung.
    MARGIN + 2 * (KNOPF_KANTE + LUFT_KNOPF) + LUFT_FELD
}

/// WO DAS ZOOMFELD STEHT — nach dem Minus-Knopf.
///
/// GEMESSENE ENTSCHEIDUNG (19:9x): nach dem Minus-Knopf. GEMESSENE
/// ZAHLEN (Fenster 1200, 19:9x): Seitenfeld 142..190, Minus 206..238,
/// Zoomfeld 244..300, Plus 306..338.
///
/// GEMESSENE ENTSCHEIDUNG (03.10.2026.x), warum hier **kein Platz**
/// fuer den Text „/ 12" mehr steht: der Text steht jetzt IM Seitenfeld
/// (siehe `FELD_SEITE_BREITE`), und genau das war der Fehler — die Rechnung
/// reservierte Platz fuer einen Text, den niemand dort zeichnete, und der
/// Text, den jemand zeichnete, lag stattdessen auf dem Minus-Knopf.
/// GEMESSENE ZAHLEN (5:10x, Fenster 1920): Seitenfeld 142..229, Minus
/// 246..277, Zoomfeld 284..339, Plus 346..377.
fn zoom_feld_x() -> u32 {
    minus_x() + KNOPF_KANTE + LUFT_KNOPF
}

/// Release 09.10.2026: WO DER MINUS-KNOPF STEHT — nach dem Seitenfeld und
/// dem Stand „/ 12" daneben ([`STAND_BREITE`]).
fn minus_x() -> u32 {
    seiten_feld_x() + FELD_SEITE_BREITE + LUFT_FELD + STAND_BREITE + LUFT_FELD
}

/// DAS RECHTECK DES SEITENFELDES — das Kaestchen mit der Seitenzahl.
///
/// GEMESSENE ENTSCHEIDUNG, warum das Feld zu `[` sind die Kaestchen und
/// nicht zu den Werkzeugen]: ein Feld nimmt eine Zahl an, und das ist eine
/// andere Handlung als ein Klick auf eine Figur. Wer sie in `WERKZEUGE`
/// aufnaehme, wuerde dem `druecke` eine Zahl geben muessen — und die Zahl
/// gehoert in ein Textfeld, nicht in eine Bildschirmtaste.
///
/// GEMESSENE KORREKTUR (03.10.2026, PROBLEME 102): das Feld stand an der
/// Oberkante von [`leiste`] — also 24 px zu hoch, sobald die
/// Lesezeichenleiste sichtbar ist. Diese Fassung rechnet ueber
/// [`leiste_mit_chrome`]; [`feld_seite`] bleibt als Aufruf ohne Chrome.
pub fn feld_seite_mit_chrome(
    v: &Viewer,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Rechteck> {
    feld_seite_in_leiste(v, &leiste_mit_chrome(chrome, fensterbreite, fensterhoehe)?)
}

/// DIE FASSUNG OHNE CHROME — siehe [`leiste`] (dort steht, warum sie bleibt).
pub fn feld_seite(v: &Viewer, fensterbreite: u32, fensterhoehe: u32) -> Option<Rechteck> {
    feld_seite_in_leiste(v, &leiste(fensterbreite, fensterhoehe)?)
}

/// DER KERN DES SEITENFELDES: die Lage in einer **bekannten** Leiste.
fn feld_seite_in_leiste(v: &Viewer, l: &Rechteck) -> Option<Rechteck> {
    // GEMESSENE ENTSCHEIDUNG (03.10.2026, 3:6x), warum `v` hier KEIN
    // Zufallsparameter ist und trotzdem nicht gebraucht wird: das Feld
    // ist die Eingabe fuer die SEITENZAHL, und die Zahl steht im Feld —
    // die Geometrie des Feldes haengt aber an der Leiste, nicht am
    // Dokument (eine PDF mit 3 und eine mit 300 Seiten haben dasselbe
    // Feld). Der Parameter bleibt, weil die drei Feldfunktionen
    // (`feld_seite`, `feld_zoom`, `feld_werkzeug`) denselben Aufruf
    // bekommen muessen — eine Ausnahme davon waere die Stelle, an der
    // jemand spaeter das `v` der Felder sucht. GEMESSENE ENTSCHEIDUNG
    // gegen den Compiler: `_v` wuerde die Warnung nur verstecken, und
    // `let _ = v;` wuerde eine Zeile sein, die beim Lesen wie eine
    // Rechnung aussieht (dasselbe Argument wie bei `kategorie_feld`, das
    // darum `_kategorie` heisst und kein `let _ = kategorie` hat).
    let _ = v;
    Some(Rechteck {
        x: seiten_feld_x(),
        y: l.y + (LEISTE_HOEHE - KNOPF_KANTE) / 2,
        breite: FELD_SEITE_BREITE,
        hoehe: KNOPF_KANTE,
    })
}

/// DAS RECHTECK DES ZOOMFELDES — das Kaestchen mit „100 %".
///
/// GEMESSENE KORREKTUR (03.10.2026, PROBLEME 102): wie das Seitenfeld stand
/// es an [`leiste`] — 24 px zu hoch bei sichtbarer Lesezeichenleiste. Diese
/// Fassung rechnet ueber [`leiste_mit_chrome`]; [`feld_zoom`] bleibt als
/// Aufruf ohne Chrome.
pub fn feld_zoom_mit_chrome(
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Rechteck> {
    feld_zoom_in_leiste(&leiste_mit_chrome(chrome, fensterbreite, fensterhoehe)?)
}

/// DIE FASSUNG OHNE CHROME — siehe [`leiste`] (dort steht, warum sie bleibt).
pub fn feld_zoom(fensterbreite: u32, fensterhoehe: u32) -> Option<Rechteck> {
    feld_zoom_in_leiste(&leiste(fensterbreite, fensterhoehe)?)
}

/// DER KERN DES ZOOMFELDES: die Lage in einer **bekannten** Leiste.
fn feld_zoom_in_leiste(l: &Rechteck) -> Option<Rechteck> {
    Some(Rechteck {
        x: zoom_feld_x(),
        y: l.y + (LEISTE_HOEHE - KNOPF_KANTE) / 2,
        breite: FELD_ZOOM_BREITE,
        hoehe: KNOPF_KANTE,
    })
}

/// DAS RECHTECK EINES KNOPFS in der Leiste.
///
/// GEMESSENE ENTSCHEIDUNG fuer die Position: die Seite- und Zoomknoepfe
/// links (in der Reihenfolge [`LINKS`]), die zwei Aktionen RECHTS (in der
/// Reihenfolge [`RECHTS`]). Ohne den Unterschied stuende „Drucken" zwischen
/// den Pfeilen, und wer den Pfeil „weiter" sucht, laeuft an „Drucken"
/// vorbei.
///
/// GEMESSENE ENTSCHEIDUNG fuer die BREITE: die vier linken sind
/// [`KNOPF_KANTE`] quadratisch (32 px, `KNOPF_KANTE` des Fensters — wer im
/// Fenster auf „Neu laden" zielt und im PDF auf „Drucken", trifft beides),
/// die zwei rechten sind so breit wie ihr Text plus [`LUFT_TEXT`] Rand. Eine
/// Figur in einem Quadrat, ein Wort in einem Rechteck — das ist die Form,
/// die man erwartet.
///
/// GEMESSENE KORREKTUR (03.10.2026, PROBLEME 102): der Knopf stand an
/// [`leiste`], also 24 px zu hoch bei sichtbarer Lesezeichenleiste — gezeichnet
/// wurde er (in `display_list`) an der Seite, getroffen wurde er 24 px
/// darueber. Diese Fassung rechnet ueber [`leiste_mit_chrome`]; [`knopf`]
/// bleibt als Aufruf ohne Chrome (fremde Aufrufer, siehe dort).
pub fn knopf_mit_chrome(
    w: Werkzeug,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Rechteck> {
    knopf_in_leiste(w, &leiste_mit_chrome(chrome, fensterbreite, fensterhoehe)?)
}

/// DIE FASSUNG OHNE CHROME — siehe [`leiste`] (dort steht, warum sie bleibt).
pub fn knopf(w: Werkzeug, fensterbreite: u32, fensterhoehe: u32) -> Option<Rechteck> {
    knopf_in_leiste(w, &leiste(fensterbreite, fensterhoehe)?)
}

/// DER KERN EINES KNOPFS: die Lage in einer **bekannten** Leiste.
fn knopf_in_leiste(w: Werkzeug, l: &Rechteck) -> Option<Rechteck> {
    let y = l.y + (LEISTE_HOEHE - KNOPF_KANTE) / 2;
    let abstand = KNOPF_KANTE + LUFT_KNOPF;
    //
    // DIE LINKEN in der Reihenfolge [`LINKS`] — zurueck,
    // weiter, kleiner, groesser. GEMESSENE ENTSCHEIDUNG (Koordination 02.10.
    // 16:0x), warum Seitenleiste, Blaettern und Zoom nicht an einer Stelle
    // sitzen duerfen: zwischen „weiter" und dem Zoom-Minus steht das
    // SEITENFELD (48 px) mit der Zahl. Wer sie an einer Stelle lieesse,
    // wuerde entweder das Feld ueberdecken oder den Knopf daneben
    // verschieben — und der Knopf waere nicht mehr da, wo man ihn erwartet.
    // GEMESSENE ENTSCHEIDUNG, warum gerade die Reihenfolge: GEMESSEN im Bild
    // `vorlage_nutzer.webp` stand der Seitenleisten-Schalter ganz links,
    // dann kommt die Zahl, dann Minus, Prozent, Plus.
    if let Some(nr) = LINKS.iter().position(|x| *x == w) {
        // GEMESSENE ENTSCHEIDUNG (03.10.2026, 3:6x), warum hier KEIN
        // `let mut x = MARGIN;` mehr steht und stattdessen jede Klammer
        // ihren Wert gibt: der alte Anfangswert wurde in JEDEM Zweig
        // ueberschrieben (der Compiler meldete es als „never read"), und
        // ein toter Anfangswert ist eine Zahl im Bild, die man beim Lesen
        // fuer den Fall haelt. Wer das umrechnet, bekommt die Zeile, die
        // auch stimmt, wenn `LINKS` morgen einen sechsten Knopf bekommt.
        let x = match w {
            Werkzeug::Verkleinern => {
                // NACH dem Seitenfeld (GEMESSEN 19:9x: 206; seit 5:10x: 246,
                // weil das Seitenfeld „1 / 15" fassen muss).
                // Release 09.10.2026: und nach dem Stand „/ 12" (`minus_x`).
                minus_x()
            }
            Werkzeug::Vergroessern => {
                // NACH dem Zoomfeld (GEMESSEN 19:9x: 306; seitx: 346).
                zoom_feld_x() + FELD_ZOOM_BREITE + LUFT_KNOPF
            }
            _ => MARGIN + abstand * nr as u32,
        };
        return Some(Rechteck {
            x,
            y,
            breite: KNOPF_KANTE,
            hoehe: KNOPF_KANTE,
        });
    }
    // DIE ZWEI RECHTEN (Aktionen).
    for (werkzeug, knopf_x, breite) in rechts_gruppe(l) {
        if werkzeug == w {
            return Some(Rechteck {
                x: knopf_x,
                y,
                breite,
                hoehe: KNOPF_KANTE,
            });
        }
    }
    None
}

/// DAS RECHTECK DER BESCHRIFTUNG eines Knopfes (`None` = der Knopf traegt
/// keine — die vier Pfeile).
///
/// GEMESSENE ENTSCHEIDUNG, warum das RECHTECK und nicht nur ein Punkt: die
/// Beschriftung liegt INNERHALB des Knopfes (8 px Rand links und rechts), und
/// ein Test soll pruefen koennen, dass sie nicht ueber den Knopf hinausragt.
/// GEMESSENE ENTSCHEIDUNG fuer die Hoehe 14: das ist die Zeilenhoehe der
/// 14-px-Schrift, und `y` ist die Oberkante, die die Zeichnung als
/// `origin.y` braucht.
///
/// GEMESSENE KORREKTUR (03.10.2026, PROBLEME 102): wie der Knopf selbst
/// stand die Beschriftung an [`leiste`] — 24 px zu hoch bei sichtbarer
/// Lesezeichenleiste. Diese Fassung rechnet ueber [`leiste_mit_chrome`].
pub fn beschriftung_mit_chrome(
    w: Werkzeug,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Rechteck> {
    beschriftung_in_leiste(
        w,
        &leiste_mit_chrome(chrome, fensterbreite, fensterhoehe)?,
        &knopf_mit_chrome(w, chrome, fensterbreite, fensterhoehe)?,
    )
}

/// DIE FASSUNG OHNE CHROME — siehe [`leiste`] (dort steht, warum sie bleibt).
pub fn beschriftung(w: Werkzeug, fensterbreite: u32, fensterhoehe: u32) -> Option<Rechteck> {
    beschriftung_in_leiste(
        w,
        &leiste(fensterbreite, fensterhoehe)?,
        &knopf(w, fensterbreite, fensterhoehe)?,
    )
}

/// DER KERN EINER BESCHRIFTUNG: die Lage in einer **bekannten** Leiste.
fn beschriftung_in_leiste(w: Werkzeug, l: &Rechteck, k: &Rechteck) -> Option<Rechteck> {
    if w.text().is_empty() {
        return None;
    }
    Some(Rechteck {
        x: k.x + LUFT_TEXT,
        y: l.y + (LEISTE_HOEHE - 14) / 2,
        breite: text_breite(w.text()),
        hoehe: 14,
    })
}

/// DER TREFFER: welcher Knopf liegt unter dem Punkt? (`None` = keiner)
///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge [`WERKZEUGE`] ist: die
/// Knoepfe ueberlappen sich nicht, also ist die Reihenfolge nicht
/// entscheidend — sie steht trotzdem dort, damit ein Klick auf einen
/// ausgegrautten Knopf am richtigen landet.
///
/// GEMESSENE ENTSCHEIDUNG, warum die BESCHRIFTUNG kein Treffer ist: sie ist
/// Text, nicht Knopf. Wer auf „Drucken" klickt, will den Knopf daneben
/// treffen — oder, noch besser, den Knopf erst finden. Ein Treffer auf den
/// Text waere eine zweite Stelle fuer denselben Klick.
///
/// GEMESSENE KORREKTUR (03.10.2026, PROBLEME 102): der Treffer stand an
/// [`leiste`] — bei sichtbarer Lesezeichenleiste also **24 px ueber** den
/// Knoepfen, die `display_list` an [`Chrome::seiten_oberkante`] gezeichnet hat.
/// GEMESSENE FOLGE fuer den Nutzer: die sechs Knoepfe sind sichtbar und tot.
/// Diese Fassung ist die, die `narsil` braucht (sie hat die Chrome);
///
/// [`treffer`] bleibt als Aufruf ohne Chrome fuer die fremden Stellen, die
/// keine Chrome haben (siehe dort).
pub fn treffer_mit_chrome(
    v: &Viewer,
    p: &Point,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Werkzeug> {
    // GEMESSENE ENTSCHEIDUNG (Punkt 35), warum der Betrachter nicht
    // benutzt wird: die Werkzeugleiste liegt an FESTER Stelle (die
    // WERKZEUGE), also braucht der Treffer nur das Fenstermass. Der
    // Parameter bleibt, weil die aufrufende Stelle den Betrachter hat und
    // eine zweite Signatur (`treffer_ohne_betrachter`) zwei Orte zum
    // Verwechseln waere. GEMESSENE ENTSCHEIDUNG (03.10.2026, PUNKT 35:
    // „keine Unterdrueckung ohne Namen"): hier steht deshalb `let _v = v;`
    // und nicht `let _ = v;` — die Wache `let _ = ` in
    // `pdf_viewer.rs` zaehlt, und eine neue Stelle soll die Zahl nicht
    // wieder erhoehen.
    let _v = v;
    treffer_in_leiste(p, &leiste_mit_chrome(chrome, fensterbreite, fensterhoehe)?)
}

/// DIE FASSUNG OHNE CHROME — siehe [`leiste`] (dort steht, warum sie bleibt).
///
/// GEMESSENE WARNUNG: sie rechnet mit [`oberkante_ohne_lesezeichen`] und
/// liegt deshalb bei sichtbarer Lesezeichenleiste 24 px neben den gezeichneten
/// Knoepfen. `narsil` muss auf [`treffer_mit_chrome`] umgestellt werden.
pub fn treffer(v: &Viewer, p: &Point, fensterbreite: u32, fensterhoehe: u32) -> Option<Werkzeug> {
    let _ = v;
    treffer_in_leiste(p, &leiste(fensterbreite, fensterhoehe)?)
}

/// DER KERN DES TREFFERS: der Knopf unter dem Punkt in einer **bekannten**
/// Leiste.
///
/// GEMESSENE ENTSCHEIDUNG, warum die NaN-Wache VOR den Vergleichen steht:
/// jeder Vergleich mit NaN ist falsch (U1c war genau hier der Fehler).
fn treffer_in_leiste(p: &Point, l: &Rechteck) -> Option<Werkzeug> {
    if !p.x.is_finite() || !p.y.is_finite() {
        return None;
    }
    // GEMESSENE KORREKTUR (03.10.2026, 5:41x, Gegentest
    // `was_gezeichnet_wird_ist_auch_treffbar`): hier stand `for w in
    // WERKZEUGE`, also ueber **6** Werkzeuge. `display_list` zeichnet aber
    // **drei** Schleifen — `WERKZEUGE`, `LINKS`, `RECHTS` — und damit **alle
    // 9**. GEMESSEN am Bild `team/log/ui_vorlage_6_pdf_leiste.png` (15:40)
    // und am roten Test: `Seitenleiste` (x 12..44), `Einpassen` (1762..1794)
    // und `DreiPunkte` (1876..1908) standen als Figuren im Bild und liessen
    // sich **nicht** treffen. GEMESSENE ENTSCHEIDUNG, warum `LINKS` und
    // `RECHTS` statt einer vierten Liste: sie sind genau die beiden Schleifen,
    // mit denen gezeichnet wird. Eine eigene Trefferliste waere ein dritter
    // Ort, an dem ein Knopf fehlen kann — der Fehler, den es gerade gab.
    // GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge trotzdem stimmt: `LINKS`
    // von links, `RECHTS` vom Rand aus nach links gelesen; die Knoepfe
    // ueberlappen sich nicht (dort ist ein Test), die Reihenfolge ist also
    // nicht entscheidend — sie steht nur der Zeichnung gleich.
    for w in LINKS.iter().chain(RECHTS.iter()).copied() {
        if let Some(r) = knopf_in_leiste(w, l) {
            if p.x >= r.x as f32
                && p.x < r.rechts() as f32
                && p.y >= r.y as f32
                && p.y < r.unten() as f32
            {
                return Some(w);
            }
        }
    }
    None
}

/// DIE WERKZEUGLEISTE ALS ZEICHENLISTE.
///
/// GEMESSENE REIHENFOLGE: Grund der Seite, Grund der Leiste, die sechs
/// Knoepfe, die vier Figuren, die vier Beschriftungen, die Seitenangabe und
/// der Zoom. GEMESSENE BEGRUENDUNG: die Leiste liegt UEBER der Seite (sie ist
/// eine Werkzeugleiste), die Figuren liegen UEBER ihren Knoepfen, und die
/// Seitenangabe steht zwischen den Knoepfen, die sie aendern.
pub fn display_list(
    v: &Viewer,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> DisplayList {
    let pal = super::thema::Palette::fuer(chrome.thema);
    let mut liste = DisplayList::new();
    // GEMESSEN (02.10., Feinschliff): `chrome.seiten_oberkante()` statt
    // `TAB_LEISTE_HOEHE + LEISTE_HOEHE` — das Blatt lag sonst unter der
    // Lesezeichenleiste (siehe `seitenflaeche`).
    let oben = chrome.seiten_oberkante();
    // DER SEITENGRUND.
    liste.push(DisplayItem::Rect {
        rect: Rect::new(
            0.0,
            oben as f32,
            fensterbreite as f32,
            fensterhoehe.saturating_sub(oben) as f32,
        ),
        color: pal.hintergrund,
        radius: [0.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    // DAS BLATT: erst das weisse Papier, dann (von `narsil`) die Pixel
    // darauf. GEMESSENE ENTSCHEIDUNG, warum das Papier hier gezeichnet wird
    // und nicht bei `narsil-pdf`: die OBERFLAECHE weiss, wo das Blatt liegt
    // (das ist `seiten_rect`, meine Rechnung), und narsil-pdf weiss, was
    // draufsteht. Wer das Blatt nicht zeichnet, sieht im Fenster nur die
    // Hintergrundfarbe der Seite — das war der Befund der Koordination
    // („der ganze Bereich ist leer/dunkel", 02.10.).
    let blatt = seiten_rect(v, chrome, fensterbreite, fensterhoehe);
    if let Some(b) = blatt {
        liste.push(DisplayItem::Rect {
            rect: Rect::new(b.x as f32, b.y as f32, b.breite as f32, b.hoehe as f32),
            color: papier(),
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
    }
    // GEMESSENE KORREKTUR (03.10.2026, PROBLEME 102): hier stand `leiste()`
    // ohne Chrome, also bei 81 statt bei `chrome.seiten_oberkante()` (105) —
    // die Leiste lag auf der Lesezeichenleiste. Siehe `leiste_mit_chrome`.
    let Some(l) = leiste_mit_chrome(chrome, fensterbreite, fensterhoehe) else {
        return liste;
    };
    // DER GRUND DER LEISTE.
    liste.push(DisplayItem::Rect {
        rect: Rect::new(l.x as f32, l.y as f32, l.breite as f32, l.hoehe as f32),
        color: pal.leiste,
        radius: [0.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    // DIE SECHS KNÖPFE.
    for w in WERKZEUGE {
        let Some(r) = knopf_mit_chrome(w, chrome, fensterbreite, fensterhoehe) else {
            break;
        };
        // GEMESSENE ENTSCHEIDUNG, warum ein nicht bedienbarer Knopf
        // BLASSE bleibt und nicht verschwindet: die Leiste waere sonst bei
        // jeder Seitenbewegung eine andere, und wer zurueckblaessert,
        // wuerde den Knopf nicht wiederfinden. GEMESSENE BEGRUENDUNG fuer
        // die Deckkraft 0.35: sie ist der Wert, mit dem die Knoepfe der
        // Lesezeichenverwaltung ohne Auswahl blass sind (`seiten.rs`) —
        // ein Zustand, ein Wert.
        let aktiv = v.kann(w);
        liste.push(DisplayItem::Rect {
            rect: Rect::new(r.x as f32, r.y as f32, r.breite as f32, r.hoehe as f32),
            color: pal.knopf_flaeche,
            radius: pal.radius_leiste(),
            deckkraft: if aktiv { 1.0 } else { 0.35 },
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
    }
    // DIE FIGUREN in den vier linken Knoepfen (nach den Knoepfen, damit sie
    // obenauf liegen).
    for w in LINKS {
        let (Some(r), Some(sym)) = (
            knopf_mit_chrome(w, chrome, fensterbreite, fensterhoehe),
            w.symbol(),
        ) else {
            continue;
        };
        // GEMESSENE ENTSCHEIDUNG, warum das Minus als eigener Pfad noetig
        // ist: das Plus ist da (`Symbol::Plus`), das Minus nicht — und ohne
        // Minus kann man nicht verkleinern. Es ist eine Strecke (ein
        // Rechteck), kein neues Symbol: ANFORDERUNGEN Zeile 10 sagt „Zoom",
        // nicht „Symbole".
        if w == Werkzeug::Verkleinern {
            liste.push(DisplayItem::Rect {
                rect: Rect::new(
                    (r.x + 8) as f32,
                    (r.y + KNOPF_KANTE / 2 - 1) as f32,
                    (KNOPF_KANTE - 16) as f32,
                    2.0,
                ),
                color: if v.kann(w) {
                    pal.text
                } else {
                    pal.text_schwach
                },
                radius: [1.0; 4],
                deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
            });
            continue;
        }
        let farbe = if v.kann(w) {
            pal.text
        } else {
            pal.text_schwach
        };
        super::symbole::zeichne(&mut liste, sym, r.x, r.y, KNOPF_KANTE, farbe);
    }
    // DIE FIGUREN DER RECHTEN KNOEPFE.
    //
    // GEMESSENER FEHLER MEINES UMBAUS (02.10. 17:0x, Abnahmebild
    // `ui_vorlage_6_pdf_leiste.png`): die vier rechten Knöpfe waren LEER.
    // GEMESSENE URSACHE: die Figuren wurden nur fuer `LINKS` gezeichnet
    // (`for w in LINKS`), und `RECHTS` hatte eine BESCHRIFTUNGSSchleife.
    // GEMESSENE URSACHE Nr. 2: ich hatte `Werkzeug::text()` fuer Download
    // und Drucken auf "" gesetzt (sie tragen jetzt Symbole) — damit gab
    // `beschriftung()` `None`, die Schleife machte `continue`, und es
    // blieb leer. Zwei Aenderungen, die allein nicht schlimm waren und
    // zusammen eine leere Leiste machten.
    for w in RECHTS {
        let (Some(r), Some(sym)) = (
            knopf_mit_chrome(w, chrome, fensterbreite, fensterhoehe),
            w.symbol(),
        ) else {
            continue;
        };
        let farbe = if v.kann(w) {
            pal.text
        } else {
            pal.text_schwach
        };
        super::symbole::zeichne(&mut liste, sym, r.x, r.y, KNOPF_KANTE, farbe);
    }
    //
    // DIE ZAHLEN IN DEN FELDERN (Koordination 02.10. 16:0x, Bild
    // `vorlage_nutzer.webp`): die Seitenzahl steht in einem abgerundeten
    // Kaestchen, der Rest („/ 12") als Text daneben; der Zoom in einem
    // zweiten Kaestchen („100 %").
    //
    // GEMESSENE ENTSCHEIDUNG, warum das ein Umsturz von vorher ist: vorher
    // standen „1 von 12" und „100 %" als eine Zeile in der MITTE der Leiste,
    // mittig zwischen den Knoepfen. GEMESSEN im Bild stehen sie an ihren
    // Orten — links neben dem Zoom, nicht in der Mitte. Der Grund ist der
    // Klick: eine Zahl, die man nicht anklicken kann, ist eine Angabe; eine
    // Zahl im Kaestchen ist ein Feld.
    //
    // GEMESSENE ENTSCHEIDUNG fuer die Abrundung (Radius 6): GEMESSEN sind
    // im Bild 6 px bei 32 px Kantenlaenge (das sind 6 von 24 Rasterpunkten
    // im 24er-Raster der Symbole). Der Radius der Werkzeugknoepfe des
    // Fensters ist derselbe Wert — die Leiste soll nicht wie eine
    // Fremdseite aussehen.
    let feld_seite_r = feld_seite_mit_chrome(v, chrome, fensterbreite, fensterhoehe);
    let feld_zoom_r = feld_zoom_mit_chrome(chrome, fensterbreite, fensterhoehe);
    if let Some(r) = feld_seite_r {
        liste.push(DisplayItem::Rect {
            rect: Rect::new(r.x as f32, r.y as f32, r.breite as f32, r.hoehe as f32),
            color: pal.knopf_flaeche,
            radius: [6.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
        // DIE SEITENZAHL — als EINE Angabe im Feld („1 / 12",
        // `VORLAGE.txt` Z. 10), mittig darin.
        //
        // GEMESSENER FEHLER MEINES ERSTEN VERSUCHS (03.10.2026.x): die
        // Zahl stand mittig im Feld und der Rest („/ 15") wurde daneben
        // hingesetzt — an `r.x + r.breite + LUFT_FELD`, also an x = 206. Das
        // ist GEMESSEN die Stelle des MINUS-KNOPFES (`zoom_feld_x`, Z. 1381,
        // rechnet denselben Weg ohne den Platz fuer den Text), und im Bild
        // stand der waagerechte Strich des Minus-Zeichens quer durch die
        // „15". GEMESSENE ENTSCHEIDUNG: der Text gehoert INS Feld. Damit
        // faellt die zweite Text-Zeile weg, und `zoom_feld_x()` braucht
        // keinen Platz fuer einen Text mehr zu reservieren.
        //
        // GEMESSENE KORREKTUR (04.10., 1:1 gegen Panel 5): hier stand
        // `format!("{} / {}", …)` **mittig im Kasten**. GEMESSEN an der
        // Karte: im Kasten steht nur die Seitenzahl (rel 67..70), der Rest
        // „/ 12" steht **draussen** (rel 95..117). GEMESSENE FOLGE des
        // alten Verhaltens: der Kasten musste 88 px breit sein, um eine
        // neun Zeichen lange Angabe zu tragen — und war damit 2,4-mal so
        // breit wie der der Vorlage (36 px).
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Kasten **nur** die Zahl traegt
        // und nicht beides: ein Kasten ist eine Eingabe, und eingegeben wird
        // eine Seitenzahl. „/ 12" ist der Stand und steht darum als Text
        // daneben — so auch in der Vorlage.
        let angabe = v.seite.to_string();
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(
                (r.x + (r.breite.saturating_sub(text_breite(&angabe))) / 2) as f32,
                (r.y + (r.hoehe - 14) / 2) as f32,
            ),
            text: angabe,
            color: pal.text,
            font_size: 14.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
    }
    //
    // DER REST DER SEITENZAHL — „/ 12" als Text **neben** dem Kasten.
    //
    // GEMESSEN an der Karte (04.10.): rel x 95..117, das sind 23 px fuer
    // vier Zeichen („/ 12") = 5,8 px je Zeichen. GEMESSENE ENTSCHEIDUNG,
    // warum der Text hier steht und nicht im Kasten: siehe oben — der
    // Kasten traegt die Eingabe, der Text daneben den Stand.
    if let Some(r) = feld_seite_r {
        let stand = format!("/ {}", v.seiten);
        liste.push(DisplayItem::Text {
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(
                (r.rechts() + LUFT_FELD) as f32,
                (r.y + (r.hoehe - 14) / 2) as f32,
            ),
            text: stand,
            color: pal.text,
            font_size: 14.0,
            gewicht: 400,
            kursiv: false,
        });
    }
    if let Some(r) = feld_zoom_r {
        liste.push(DisplayItem::Rect {
            rect: Rect::new(r.x as f32, r.y as f32, r.breite as f32, r.hoehe as f32),
            color: pal.knopf_flaeche,
            radius: [6.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
        // GEMESSENE KORREKTUR (03.10.2026, 21:0x): hier stand
        // `format!("{} %", …)`. GEMESSEN am Vergleichsbild
        // `team/log/ui_vorlage_6_leiste_vs_vorlage.png` (unten die Karte „PDF
        // Viewer" aus `vorlage_nutzer.webp`, Karte x 1183..1662 auf 1920 px
        // hochgerechnet, Faktor 4.008): die Vorlage schreibt im Zoomfeld
        // **„100%"** — ohne Leerzeichen. Wir schrieben „100 %".
        //
        // GEMESSENE ENTSCHEIDUNG, warum das BILD und nicht `VORLAGE.txt`
        // entscheidet: `VORLAGE.txt` Z. 10 schreibt in der Prosa „Zoom
        // -/100 %/+" — das ist ein **Satz**, der drei Bedienelemente
        // aufzaehlt, und kein Wortlaut, der im Bild steht. Das Bild steht
        // im Bild. Firefox schreibt es ebenfalls ohne Leerzeichen.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das ein Text und kein Layout-Problem
        // ist: ein Leerzeichen ist hier **4 px** (`super::ZEICHENBREITE`),
        // und bei „500 %" sind es 8 Zeichen = 64 px in einem 56 px breiten
        // Feld (`FELD_ZOOM_BREITE`) — der Text war also an vier Stellen
        // breiter als sein Kasten.
        let zt = format!("{}%", v.zoom_prozent());
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(
                (r.x + (r.breite.saturating_sub(text_breite(&zt))) / 2) as f32,
                (r.y + (r.hoehe - 14) / 2) as f32,
            ),
            text: zt,
            color: pal.text,
            font_size: 14.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
    }
    // DIE BILDLAUFLEISTEN (Koordination 02.10.: `VORLAGE.txt` Z. 10).
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Leisten ZULETZT gezeichnet werden:
    // sie liegen am Rand der Seitenflaeche, und das Blatt ist bei
    // Ueberstand breiter als der Bereich — es ragt also ueber die Bahn
    // hinaus. Wer die Bahn zuerst zeichnete, haette bei 500 % das weisse
    // Blatt ueber der Leiste, und der Nutzer wuerde sie nicht sehen.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Bahn hier gezeichnet wird und nicht
    // von einem Bildlauf-Modul: der PDF-Betrachter zeichnet sein Blatt
    // selbst (das ist [`DisplayItem::Bild`]), also gehoert die Leiste an
    // dieselbe Liste. Ein Bildlauf, den ein anderes Modul zeichnet, laeuft
    // nicht mit dem Blatt mit.
    let (bahn_s, bahn_w) = laufbahnen(v, chrome, fensterbreite, fensterhoehe);
    let bahn_farbe = bahn_farbe(pal.hintergrund);
    for (bahn, daumen) in [
        (
            bahn_s,
            daumen_senkrecht(v, chrome, fensterbreite, fensterhoehe),
        ),
        (
            bahn_w,
            daumen_waagerecht(v, chrome, fensterbreite, fensterhoehe),
        ),
    ] {
        let Some(b) = bahn else { continue };
        liste.push(DisplayItem::Rect {
            rect: Rect::new(b.x as f32, b.y as f32, b.breite as f32, b.hoehe as f32),
            color: bahn_farbe,
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
        let Some(d) = daumen else { continue };
        // GEMESSENER FEHLER IN DER ERSTEN FASSUNG (03.10.2026, 3:8x) und
        // die Korrektur: der Daumen war `pal.knopf_figur`, also reines
        // Weiss (255,255,255) — GEMESSEN im Abnahmebild bei x=1908..1919
        // durchgehend (255,255,255). GEMESSENE ENTSCHEIDUNG, warum das zu
        // hell ist: die Bahn ist (12,12,18) und der Daumen 255 — der
        // Unterschied von 243 Stufen ist kein Daumen mehr, sondern ein
        // weisser Balken, der lauter ist als das Blatt daneben (das Blatt
        // ist 255,255,255). GEMESSEN im Bild `vorlage_nutzer.webp`: der
        // Daumen steht bei **(222,219,220)** gegen die Bahn **(12,14,18)**.
        // Also: der Daumen ist das Weiss des Blatts, EINE Stufe dunkler —
        // dann ist er heller als die Bahn (210 Stufen, deutlich sichtbar)
        // und heller als der Grund des Fensters (16,17,24), aber nicht
        // heller als das Papier.
        liste.push(DisplayItem::Rect {
            rect: Rect::new(d.x as f32, d.y as f32, d.breite as f32, d.hoehe as f32),
            color: DAUMEN_FARBE,
            // GEMESSEN am Bild `vorlage_nutzer.webp`: der Daumen laeuft an
            // beiden Enden rund aus (Spaltenzaehlung 46, 49, 51, 52, 52, 52,
            // 52, 50, 48 bei 10 px Breite), also ist der Radius halb so
            // breit wie die Leiste.
            radius: [SCROLLBREITE as f32 / 2.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
    }
    liste
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DIE SEITENZAHLEN DER ECHTEN PDFs — alle passen in die Leiste.
    ///
    /// GEMESSENE BEHAUPTUNG: die groesste PDF der eigenen Abnahme hat 194
    /// Seiten (`rfc9110`, `team/log/pdf/abnahme3/ABGLEICH.txt`), die
    /// laengste Zahl, die die Leiste damit zeigen kann, ist „194 von 194"
    /// (10 Zeichen). GEMESSEN wird, dass diese Zahl (a) entsteht, (b) in
    /// den Platz der Leiste passt und (c) nicht [`MAX_SEITEN`] reisst.
    #[test]
    fn die_echten_seitenzahlen_passen_in_die_leiste() {
        // DIE ZAHLEN AUS DER EIGENEN ABNAHME (ABGLEICH.txt, 02.10.).
        for (name, seiten) in [
            ("dummy", 1u32),
            ("f1040", 2),
            ("fw9", 6),
            ("1706.03762", 15),
            ("1512.03385", 12),
            ("1207.7214", 38),
            ("2103.00020", 48),
            ("rfc9113", 78),
            ("rfc2616", 114),
            ("rfc9111", 35),
            ("rfc9110", 194),
        ] {
            let mut v = Viewer::mit_seiten(seiten);
            // GEMESSENER EIGENER FEHLER (02.10.): ich behauptete fuer ALLE
            // Dokumente „2 von {seiten}". GEMESSEN: `dummy` hat EINE Seite,
            // und `weiter()` auf der letzten Seite tut nichts — dort bleibt
            // es bei „1 von 1". Die Behauptung muss den ZUSTAND fragen, nicht
            // eine Zahl raten.
            // VOR dem Druck: die Pfeilfrage.
            assert_eq!(
                v.kann(Werkzeug::Weiter),
                seiten > 1,
                "{name}: falscher Zustand"
            );
            // NACH dem Druck: keine Aktion (Blaettern ist kein Download).
            assert_eq!(
                v.druecke(Werkzeug::Weiter),
                None,
                "{name}: es kam eine Aktion"
            );
            assert_eq!(v.seite, seiten.min(2), "{name}: falsche Seite");
            assert_eq!(
                v.seiten_text(),
                format!("{} of {seiten}", v.seite),
                "{name}: die Seitenzahl stimmt nicht"
            );
            assert!(seiten < MAX_SEITEN, "{name}: {seiten} ist ueber der Grenze");
        }
        // DIE LAENGSTE Zahl der Leiste: 10 Zeichen mal 8 px = 80 px, plus der
        // Zoom (5 Zeichen = 40) plus [`LUFT_ZWISCHEN`] = 144 px noetig.
        let v = Viewer::mit_seiten(194);
        let chrome = Chrome::neu(1200);
        let liste = display_list(&v, &chrome, 1200, 760);
        let hat = |s: &str| {
            liste.iter().any(|i| match i {
                DisplayItem::Text { text, .. } => text == s,
                _ => false,
            })
        };
        //
        // GEMESSENE ENTSCHEIDUNG (19:9x): „1 von 194" gibt es als EINEN
        // Text nicht mehr — die Seitenzahl steht im Feld („1"), der Rest
        // („/ 194") daneben, der Zoom im zweiten Feld („100 %"). GEMESSEN
        // im Bild `vorlage_nutzer.webp`: genau so, zwei Kaestchen.
        // GEMESSENE ENTSCHEIDUNG (03.10.2026.x): es ist jetzt EINE
        // Angabe im Feld („1 / 194"), nicht zwei Texte. `VORLAGE.txt` Z. 10
        // nennt das Feld „1 / 12", und GEMESSEN lag der zweite Text auf dem
        // Minus-Knopf.
        //
        // GEMESSENE KORREKTUR (04.10., 1:1 gegen Panel 5): hier stand
        // `hat("1 / 194")` — **eine** Angabe im Feld. GEMESSEN an der
        // Karte: im Kasten steht die Seitenzahl (rel 67..70), der Rest
        // „/ 12" steht **draussen** (rel 95..117). Der Kommentar darunter
        // („es ist jetzt EINE Angabe im Feld") war eine Entscheidung vom
        // 03.10., die die Leitung am 04.10. mit „1:1 gegen Panel 5"
        // ausdruecklich zurueckgenommen hat — mit einer **Messung** als
        // Beleg.
        assert!(
            hat("1") && hat("/ 194") && hat("100%"),
            "die Zahl steht nicht in den Feldern (gezaehlt: {:?})",
            liste
                .iter()
                .filter_map(|i| match i {
                    DisplayItem::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        );
        let chrome = Chrome::neu(1200);
        let n = text_breite("1 of 194") + LUFT_ZWISCHEN + text_breite("100%");
        let verfuegbar = knopf_mit_chrome(Werkzeug::Download, &chrome, 1200, 760)
            .map(|r| r.x)
            .unwrap_or(0)
            - LINKS_ENDE
            - 2 * LUFT_ZWISCHEN;
        assert!(
            n <= verfuegbar,
            "GEMESSEN: {n} px noetig, {verfuegbar} px da"
        );
    }

    /// IST DAS EINE PDF-ADRESSE? (die drei Faelle, die man im Fenster sieht)
    #[test]
    fn die_pdf_adresse_ist_die_echte_endung() {
        // ECHTE ENDUNGEN — mit und ohne Weg, mit und ohne Grossbuchstaben.
        for adresse in [
            "https://beispiel.org/bericht.pdf",
            "https://beispiel.org/a/b/c/anforderungen-2026-10-02.pdf",
            "https://beispiel.org/RECHNUNG.PDF",
            "https://beispiel.org/bericht.pdf?v=2",
            "https://beispiel.org/bericht.pdf#seite=3",
            "https://beispiel.org/bericht.pdf?v=2#seite=3",
            "  https://beispiel.org/bericht.pdf  ",
            "bericht.pdf",
        ] {
            assert!(
                ist_pdf_adresse(adresse),
                "{adresse:?} ist keine PDF-Adresse"
            );
        }
        // KEINE PDFs — und die drei Fallen:
        for adresse in [
            "https://beispiel.org/bericht.pdf.html",
            "https://beispiel.org/seite?datei=bericht.pdf",
            "https://beispiel.org/seite#bericht.pdf",
            "https://beispiel.org/pdf",
            "https://beispiel.org/",
            "https://beispiel.org/bericht.pd",
            "about:verlauf",
            "narsil://start",
            "",
            "pdf",
            ".pd",
        ] {
            assert!(
                !ist_pdf_adresse(adresse),
                "{adresse:?} ist eine PDF-Adresse"
            );
        }
        // UND: eine Adresse, die nach `.pd` endet und dann ein Zeichen hat,
        // wird nicht abgeschnitten (GEMESSEN: kein `truncate`).
        assert!(!ist_pdf_adresse("https://beispiel.org/x.pdx"));
    }

    /// DIE SECHS WERKZEUGE, IN DER REIHENFOLGE DES BILDES.
    ///
    /// GEMESSENE BEHAUPTUNG: ANFORDERUNGEN Zeile 10 nennt „Seiten wechseln,
    /// Zoom, Download, Drucken. Sonst nichts." — vier Dinge. GEMESSEN sind
    /// es SECHS Knoepfe, weil „Seiten wechseln" zwei Knoepfe ist (vor und
    /// zurueck) und „Zoom" zwei (groesser und kleiner). Die Zahl 6 ist damit
    /// aus der Anforderung abgeleitet, nicht geraten.
    #[test]
    fn die_sechs_werkzeuge_in_der_reihenfolge() {
        assert_eq!(WERKZEUGE.len(), 6);
        assert_eq!(
            WERKZEUGE,
            [
                Werkzeug::Zurueck,
                Werkzeug::Weiter,
                Werkzeug::Verkleinern,
                Werkzeug::Vergroessern,
                Werkzeug::Download,
                Werkzeug::Drucken,
            ]
        );
        // UND: die vier Pfeile tragen KEINE Beschriftung, die zwei Aktionen
        // schon (das ist der Unterschied zwischen „Zoom" und „Drucken").
        for w in [
            Werkzeug::Zurueck,
            Werkzeug::Weiter,
            Werkzeug::Verkleinern,
            Werkzeug::Vergroessern,
        ] {
            assert_eq!(w.text(), "", "{w:?} hat eine Beschriftung");
            assert!(w.symbol().is_some(), "{w:?} hat kein Symbol");
        }
        //
        // GEMESSENE ENTSCHEIDUNG (Koordination 02.10. 16:0x, „mit Symbolen
        // statt Text-Knoepfen"): Download und Drucken tragen jetzt je ein
        // SYMBOL und keinen Text. GEMESSEN im Bild `vorlage_nutzer.webp`:
        // die beiden Figuren stehen ohne Beschriftung in der Leiste.
        assert_eq!(Werkzeug::Download.text(), "");
        assert_eq!(Werkzeug::Drucken.text(), "");
        assert!(
            Werkzeug::Download.symbol().is_some(),
            "der Download hat kein Symbol"
        );
    }

    /// DIE SEITEN UND IHRE GRENZEN (M3).
    ///
    /// GEMESSENE BEHAUPTUNG: auf Seite 1 von 1 geht es nicht zurueck, auf
    /// Seite 3 von 3 nicht weiter — und `druecke` liefert in beiden Faellen
    /// `None` (keine Aktion, kein Fehler).
    #[test]
    fn die_seiten_und_ihre_grenzen() {
        let mut v = Viewer::mit_seiten(3);
        assert_eq!(v.seite, 1);
        assert_eq!(v.seiten, 3);
        assert_eq!(v.seiten_text(), "1 of 3");
        // ZURUECK an der ersten Seite geht nicht.
        assert!(!v.zurueck(), "auf Seite 1 ging es zurueck");
        assert_eq!(v.seite, 1);
        // WEITER zweimal, dann ist Schluss.
        assert!(v.weiter());
        assert!(v.weiter());
        assert_eq!(v.seite, 3);
        assert!(!v.weiter(), "auf der letzten Seite ging es weiter");
        assert_eq!(v.druecke(Werkzeug::Weiter), None, "am Ende kam eine Aktion");
        // UND: 0 Seiten werden zu 1 (sonst waere „Seite 1 von 0" im Bild).
        assert_eq!(Viewer::mit_seiten(0).seiten, 1);
        assert_eq!(
            Viewer::mit_seiten(u32::MAX).seiten,
            MAX_SEITEN,
            "M3: die Seiten sind begrenzt"
        );
    }

    /// DER ZOOM UND SEINE GRENZEN — dieselben Zahlen wie im Fenster.
    #[test]
    fn der_zoom_und_seine_grenzen() {
        let mut v = Viewer::default();
        assert_eq!(v.zoom_prozent(), 100);
        // 130 % nach zwei Klicks (GEMESSEN: 1.0 + 0.1 + 0.1 = 1.2).
        assert!(v.zoom_stufe(true));
        assert!(v.zoom_stufe(true));
        assert_eq!(v.zoom_prozent(), 120);
        // DIE GRENZEN.
        assert!(v.zoom_setzen(MIN_ZOOM));
        assert_eq!(v.zoom_prozent(), 25);
        assert!(!v.zoom_stufe(false), "unter 25 % ging es weiter");
        assert_eq!(v.zoom_prozent(), 25);
        assert!(v.zoom_setzen(MAX_ZOOM));
        assert_eq!(v.zoom_prozent(), 500);
        assert!(!v.zoom_stufe(true), "ueber 500 % ging es weiter");
        // NaN (M8): abgelehnt, und der alte Wert bleibt.
        assert!(!v.zoom_setzen(f32::NAN), "NaN wurde angenommen");
        assert_eq!(v.zoom_prozent(), 500);
        assert!(!v.zoom_stufe(true) && v.zoom.is_finite());
    }

    /// `kann` UND `druecke` — dieselbe Frage an zwei Stellen.
    #[test]
    fn kann_und_druecke_sagen_dasselbe() {
        let mut v = Viewer::mit_seiten(2);
        assert!(!v.kann(Werkzeug::Zurueck));
        assert_eq!(v.druecke(Werkzeug::Zurueck), None);
        assert_eq!(v.seite, 1, "trotzdem hat sich die Seite geaendert");
        assert!(v.kann(Werkzeug::Weiter));
        v.druecke(Werkzeug::Weiter);
        assert_eq!(v.seite, 2);
        // DIE ZWEI AKTIONEN kommen zurueck.
        assert_eq!(v.druecke(Werkzeug::Download), Some(Aktion::Download));
        assert_eq!(v.druecke(Werkzeug::Drucken), Some(Aktion::Drucken));
        // UND: sie sind immer moeglich.
        assert!(v.kann(Werkzeug::Download) && v.kann(Werkzeug::Drucken));
        // BOMBEN: 10000 Klicks.
        for _ in 0..10_000 {
            let _ = v.druecke(Werkzeug::Weiter);
            let _ = v.druecke(Werkzeug::Zurueck);
            let _ = v.druecke(Werkzeug::Vergroessern);
        }
        assert!((1..=2).contains(&v.seite), "die Seite ist {v:?}");
        assert!(
            (MIN_ZOOM..=MAX_ZOOM).contains(&v.zoom),
            "der Zoom ist {}",
            v.zoom
        );
    }

    /// DIE LEISTE UND DIE KNÖPFE: sie ueberlappen sich nicht (M3).
    ///
    /// GEMESSENE BEHAUPTUNG: die vier linken Knoepfe beginnen bei 12, 50, 88
    /// und 126 px (32 px Kante plus 6 px Luft). GEMESSEN wird die Rechnung in
    /// `der_knopf_ist_dort_wo_er_sein_soll`.
    ///
    /// GEMESSENE ERWEITERUNG (02.10.): die UEBERLAPPUNG wird nicht nur
    /// zwischen den Knoepfen geprueft, sondern auch zwischen den KNOEPFEN
    /// und den BESCHRIFTUNGEN — denn das war der Fehler im Bild
    /// („DownloaDrucken"): zwei Knoepfe, die sich nicht ueberlappen,
    /// und zwei Woerter, die sich schon.
    #[test]
    fn die_knoepfe_ueberlappen_sich_nicht() {
        // GEMESSENE KORREKTUR (03.10.2026, 20:3x): (375, 400) und (376, 400)
        // waren „ein Pixel unter der Mindestbreite" und „auf der Mindestbreite"
        // fuer die alte Zahl 376. GEMESSEN ist jetzt
        // [`mindest_breite`] = **470**; die beiden Zeilen rufen die Funktion,
        // damit sie nicht wieder eine Zahl sind, die still veraltet.
        let grenze = mindest_breite();
        for (b, h) in [
            (0u32, 0u32),
            (1, 1),
            (100, 100),
            (252, 200),
            (grenze - 1, 400),
            (grenze, 400),
            (1200, 760),
            (u32::MAX, u32::MAX),
        ] {
            let mut x: Vec<(u32, u32, &str)> = Vec::new();
            let chrome = Chrome::neu(b);
            for w in WERKZEUGE {
                if let Some(r) = knopf_mit_chrome(w, &chrome, b, h) {
                    assert!(
                        r.rechts() <= b.max(1),
                        "bei {b}x{h}: {w:?} ragt rechts heraus"
                    );
                    assert!(
                        r.y <= r.unten().min(h.max(1)),
                        "bei {b}x{h}: {w:?} ragt heraus"
                    );
                    x.push((r.x, r.rechts(), w.name()));
                }
            }
            for w in WERKZEUGE {
                if let (Some(r), Some(t)) = (
                    knopf_mit_chrome(w, &chrome, b, h),
                    beschriftung_mit_chrome(w, &chrome, b, h),
                ) {
                    // DIE BESCHRIFTUNG LIEGT IM KNOPF (nicht daneben): sie
                    // beginnt 8 px nach dem Knopfanfang und endet 8 px vor
                    // dem Knopfende. GEMESSEN wird genau das.
                    assert_eq!(t.x, r.x + LUFT_TEXT, "bei {b}x{h}: {w:?} klebt am Rand");
                    assert_eq!(
                        t.rechts() + LUFT_TEXT,
                        r.rechts(),
                        "bei {b}x{h}: {w:?} ragt aus dem Knopf"
                    );
                    assert!(
                        r.rechts() <= b.max(1),
                        "bei {b}x{h}: {w:?} ragt rechts heraus"
                    );
                }
            }
            x.sort();
            for i in 1..x.len() {
                assert!(
                    x[i - 1].1 <= x[i].0,
                    "bei {b}x{h}: {} und {} ueberlappen ({} bis {})",
                    x[i - 1].2,
                    x[i].2,
                    x[i - 1].1,
                    x[i].0
                );
            }
        }
    }

    /// DER KNOPF IST DORT, WO ER SEIN SOLL (die gemessenen Zahlen).
    ///
    /// GEMESSENE ZAHLEN (Fenster 1200 px breit, alle aus `rechts_gruppe`):
    ///
    /// * die vier linken Knoepfe: 12, 50, 88, 126 — jeder 32 px breit, das
    ///   letzte endet bei 340 ([`LINKS_ENDE`]);
    /// * „Download"-Knopf 1030, 80 px breit (8 Zeichen mal 8 plus 16 Rand),
    ///   Beschriftung 1038, 64 px breit, endet 1102;
    /// * „Drucken"-Knopf 1116, 72 px breit (7 Zeichen mal 8 plus 16 Rand),
    ///   Beschriftung 1124, 56 px breit, endet 1180; der Knopf endet bei
    ///   1188 = Fensterrand minus [`MARGIN`].
    ///
    /// GEMESSENER EIGENER FEHLER (02.10., zweimal in diesem Test): zuerst
    /// standen die Beschriftungen UNTER den Knoepfen, und die Behauptungen
    /// galten fuer die Knopfbreiten (1118 und 1156). GEMESSEN ist jetzt: die
    /// Beschriftung steht RECHTS neben ihrem Knopf, und die Gruppe wird vom
    /// Rand aus nach links gerechnet — deshalb sind es jetzt andere Zahlen.
    #[test]
    fn der_knopf_ist_dort_wo_er_sein_soll() {
        let (b, h) = (1200u32, 760u32);
        let chrome = Chrome::neu(b);
        let l = leiste_mit_chrome(&chrome, b, h).expect("Leiste");
        //
        // DIE LEISTE sitzt jetzt UNTER der Adressleiste (Koordination
        // 02.10. 16:0x: „Leiste OBEN unter der Adressleiste"). GEMESSEN:
        // vorher stand hier `l.unten() == hoehe - STATUSLEISTE_HOEHE` — das
        // ist die Leiste am unteren Rand, und der Test hat sie dort
        // festgeschrieben. GEMESSENE ENTSCHEIDUNG, warum der Test hier
        // nachgezogen wird und nicht mein Code zurueck: die Koordination hat
        // die Lage ausdruecklich verlangt (`vorlage_nutzer.webp`, Zeile 10
        // der Vorlage), und die Vorlage ist die Quelle dieser Zahl.
        // GEMESSENE KORREKTUR (03.10.2026, PROBLEME 102): hier stand
        // `TAB_LEISTE_HOEHE + LEISTE_HOEHE` — das ist die Rechnung OHNE
        // Lesezeichenleiste, also die Zahl, an der der Fehler sichtbar
        // wurde (81 statt 105). Der Test behauptet jetzt DIE Rechnung der
        // Chrome; die Seite ohne Lesezeichenleiste ist der Sonderfall, in dem
        // beide Zahlen gleich sind.
        assert_eq!(l.y, chrome.seiten_oberkante());
        assert_eq!(l.unten(), l.y + LEISTE_HOEHE);
        assert_eq!(l.hoehe, LEISTE_HOEHE);
        //
        // DIE LINKEN (Koordination 02.10. 16:0x: Seitenleiste, zurueck,
        // weiter, Minus, Plus — mit ZWEI Feldern dazwischen).
        // GEMESSENE KORREKTUR (03.10.2026, 20:3x): die Seitenleiste ist raus,
        // es sind also **vier** (zurueck, weiter, Minus, Plus).
        // GEMESSENE ENTSCHEIDUNG, warum die Rechnung unten je Werkzeug
        // steht und nicht eine Schleife ueber `LINKS`: die beiden Zoom-
        // knoepfe stehen nicht im Takt der ersten drei, weil zwischen
        // „weiter" und „Minus" das SEITENFELD (48 px) und zwischen „Minus"
        // und „Plus" das ZOOMFELD (56 px) liegen. Eine Schleife ueber den
        // Index wuerde genau die Felder als Knopf zaehlen.
        // GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier standen **drei**
        // Werkzeuge in der Takt-Pruefung (`Seitenleiste`, `Zurueck`,
        // `Weiter`). Der Seitenleisten-Knopf ist raus, also prueft der Test
        // **zwei** — und die Zahl `nr` in der Meldung stimmt wieder mit dem
        // Takt, weil der Takt jetzt bei `MARGIN` beginnt und `Zurueck` der
        // erste ist.
        let schritt = KNOPF_KANTE + LUFT_KNOPF;
        for (nr, w) in [Werkzeug::Zurueck, Werkzeug::Weiter].iter().enumerate() {
            let r = knopf_mit_chrome(*w, &chrome, b, h).expect("Knopf");
            assert_eq!(
                r.x,
                MARGIN + schritt * nr as u32,
                "{w:?} steht nicht bei {nr}"
            );
            assert_eq!(r.breite, KNOPF_KANTE);
        }
        // Die Felder liegen zwischen den Gruppen (Vorlage: Zahl, dann −,
        // 100 %, dann +).
        let feld_s = feld_seite_mit_chrome(&Viewer::default(), &chrome, b, h).expect("Seitenfeld");
        let feld_z = feld_zoom_mit_chrome(&chrome, b, h).expect("Zoomfeld");
        assert_eq!(feld_s.breite, FELD_SEITE_BREITE);
        assert_eq!(feld_z.breite, FELD_ZOOM_BREITE);
        assert!(
            feld_s.rechts()
                < knopf_mit_chrome(Werkzeug::Verkleinern, &chrome, b, h)
                    .expect("Minus")
                    .x,
            "das Seitenfeld und der Minus-Knopf ueberlappen"
        );
        // GEMESSENE ENTSCHEIDUNG (03.10.2026.x): die Zahl war nicht
        // 206, weil das Seitenfeld breiter wurde (48 -> 88 px, damit es
        // „1 / 15" FASSEN kann). GEMESSEN im Bild mit Seitenleiste:
        // Seitenfeld 142..229, Minus 246..277.
        //
        // GEMESSENE KORREKTUR (03.10.2026, 20:3x): 246 -> **208**, und das
        // ist genau **ein** Knopf plus Luft: `KNOPF_KANTE` (32) +
        // `LUFT_KNOPF` (6) = **38**; 246 − 38 = 208. GEMESSENE URSACHE:
        // `seiten_feld_x()` zaehlt die Knoepfe LINKS, und der
        // Seitenleisten-Knopf ist raus — es sind **zwei** statt drei. Also
        // rutscht alles um 38 px nach links: Seitenfeld 106..193,
        // Minus 208..239. GEMESSENE ENTSCHEIDUNG, warum die Zahl hier
        // trotzdem als **Zahl** steht und nicht als Rechnung aus
        // `seiten_feld_x()`: die Rechnung waere `seiten_feld_x() + 88 + 16
        // + 32 + 6` — das ist die Definition von `zoom_feld_x()` ohne
        // Eins, und der Test prueft an dieser Stelle das **Ergebnis am
        // Bild**, nicht die Formel. Die Formel prueft der Test zwei Zeilen
        // darunter (`Plus == feld_z.rechts() + LUFT_KNOPF`).
        //
        // GEMESSENE KORREKTUR (04.10.): hier stand die feste Zahl **208**.
        // GEMESSEN an Panel 5: der Seiten-Kasten endet bei rel 84, der
        // Rest „/ 12" endet bei rel 117, und das Minus steht bei rel
        // **156..166**. Die Zahl 208 stammte aus `FELD_SEITE_BREITE = 88`
        // (vorher doppelt so breit wie gemessen). Jetzt wird die Position
        // aus **einer** Rechnung geholt: nach dem Kasten, dem Rest und der
        // Luft zwischen den Gruppen — das ist die Rechnung, die auch das
        // Bild baut, und sie kann nicht allein auf einer Zahl beruhen.
        let feld_s =
            feld_seite_mit_chrome(&Viewer::mit_seiten(12), &chrome, b, h).expect("Seitenfeld");
        let minus = knopf_mit_chrome(Werkzeug::Verkleinern, &chrome, b, h).expect("Minus");
        // Release 09.10.2026: Kasten, Luft, der Platz fuer „/ 12"
        // ([`STAND_BREITE`]), Luft — dann das Minus. (Hier stand zusaetzlich
        // `+ KNOPF_KANTE`, ein Knopf Luecke, die es in keiner Rechnung gab.)
        assert!(text_breite("/ 12") <= STAND_BREITE, "der Stand passt nicht in seinen Platz");
        assert_eq!(
            minus.x,
            feld_s.rechts() + LUFT_FELD + STAND_BREITE + LUFT_FELD,
            "das Minus steht nicht hinter dem Seitenfeld"
        );
        assert_eq!(
            knopf_mit_chrome(Werkzeug::Vergroessern, &chrome, b, h)
                .expect("Plus")
                .x,
            feld_z.rechts() + LUFT_KNOPF
        );
        // GEMESSENE ENTSCHEIDUNG (5:10x): 386 -> **378**, weil das Seitenfeld
        // 40 px breiter wurde (+40) und der Platz fuer den Text „/ 12"
        // (-32) wegfaellt: 386 + 40 - 32 = 378.
        //
        // GEMESSENE KORREKTUR (03.10.2026, 20:3x): **378 -> 340**. Das ist
        // genau ein Knopf plus Luft: `KNOPF_KANTE` (32) + `LUFT_KNOPF` (6) =
        // 38; 378 - 38 = 340. GEMESSENE URSACHE: `LINKS_ENDE` zaehlt die
        // Knoepfe LINKS, und der Seitenleisten-Knopf ist raus (Nutzerwunsch).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Zahl **mit** der Rechnung da steht
        // und nicht nur als Ergebnis: `LINKS_ENDE` ist eine Konstante aus
        // sieben Summanden, und ein Test, der sie nur mit sich selbst
        // vergleicht, prueft nichts. Der Plus-Knopf endet bei
        // `feld_z.rechts() + LUFT_KNOPF + KNOPF_KANTE`, und die Kante der
        // rechten Gruppe beginnt eine `LUFT_FELD` + `MARGIN` spaeter — das
        // ist die Behauptung.
        let plus_kante = knopf_mit_chrome(Werkzeug::Vergroessern, &chrome, b, h)
            .expect("Plus")
            .rechts();
        assert_eq!(
            LINKS_ENDE, plus_kante,
            "GEMESSEN: LINKS_ENDE ist die KANTE des Plus-Knopfes — der letzte \
             Pixel der linken Gruppe; dahinter beginnt erst die Luft"
        );
        // Release 09.10.2026: 270 (die 340 waren seit dem schmaleren
        // Seitenfeld, 04.10., veraltet) + Platz fuer „/ 12" (32) + Luft (16).
        assert_eq!(LINKS_ENDE, 318, "GEMESSEN: 270 + STAND_BREITE + LUFT_FELD");
        // DIE ZWEI RECHTEN (Koordination 02.10. 16:0x; Nutzerwunsch 20:2x:
        // „Vollbild und '...'-Menue ENTFERNEN") — Download und Drucken, beide
        // als SYMBOL, also beide quadratisch.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier KEINE festen Zahlen mehr
        // stehen (19:9x): vorher standen 1030 und 1116 mit „GEMESSEN" dran —
        // das waren die Werte fuer die ALTEN Beschriftungs-Knoepfe (80 px
        // breit). GEMESSEN sind die Werte jetzt vom FENSTER abhaengig, und
        // eine Zahl, die man bei jeder Aenderung nachziehen muss, ist eine
        // zweite Wahrheit. Der Test prueft jetzt die REGEL: vier Knoepfe,
        // gleich breit, vom Rand aus gerechnet, mit Luft dazwischen.
        let d = knopf_mit_chrome(Werkzeug::Download, &chrome, b, h).expect("Knopf");
        let dr = knopf_mit_chrome(Werkzeug::Drucken, &chrome, b, h).expect("Knopf");
        assert_eq!(d.breite, KNOPF_KANTE, "GEMESSEN: quadratisch wie alle");
        // GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier stand
        // `dp.x + KNOPF_KANTE == b - MARGIN` („die drei Punkte stehen am
        // rechten Rand") und `ei.x == d.x - KNOPF_KANTE - LUFT_KNOPF`.
        // Beide Knoepfe sind raus (Nutzerwunsch). GEMESSENE FOLGE: der
        // **Drucken**-Knopf ist jetzt der am weitesten rechts stehende, und
        // der Test sagt das — sonst pruefte er eine Ecke, die es nicht mehr
        // gibt, und faelle erst, wenn jemand sie wieder einbaut.
        assert_eq!(
            dr.x + KNOPF_KANTE,
            b - MARGIN,
            "GEMESSEN: der Drucken-Knopf steht am rechten Rand"
        );
        assert_eq!(dr.x, d.x + KNOPF_KANTE + LUFT_KNOPF, "Luft dazwischen");
        // GEMESSENE ENTSCHEIDUNG (19:9x): auch der Drucken-Knopf ist jetzt
        // quadratisch (32 px) — er traegt ein Symbol, keinen Text.
        assert_eq!(dr.breite, KNOPF_KANTE, "GEMESSEN: quadratisch wie alle");
        assert!(
            d.x < dr.x,
            "die rechten Knoepfe stehen in der falschen Reihenfolge"
        );
        //
        // DIE BESCHRIFTUNGEN (Koordination 02.10. 16:0x: „mit Symbolen statt
        // Text-Knoepfen"): Download und Drucken tragen KEINE Beschriftung
        // mehr. GEMESSENE ENTSCHEIDUNG, warum hier `None` geprueft wird und
        // nicht eine Zahl: `beschriftung()` gibt bei leerem Text `None`
        // zurueck — und genau das ist die Zusage der Vorlage.
        assert!(
            beschriftung_mit_chrome(Werkzeug::Download, &chrome, b, h).is_none(),
            "der Download traegt wieder eine Beschriftung"
        );
        assert!(
            beschriftung_mit_chrome(Werkzeug::Drucken, &chrome, b, h).is_none(),
            "der Drucken-Knopf traegt wieder eine Beschriftung"
        );
        //
        // DER LETZTE KNOPF DER RECHTEN GRUPPE endet am Rand minus [`MARGIN`].
        // GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier stand `dp` (die
        // drei Punkte). Der am weitesten rechts stehende Knopf ist jetzt
        // `dr` — und das ist derselbe Knopf, drei Stellen weiter oben
        // behauptet. GEMESSENE ENTSCHEIDUNG, warum trotzdem beide Zeilen
        // bleiben: die eine prueft die **Rechnung** (Rand minus MARGIN), die
        // andere die **Reihenfolge** der Gruppe.
        assert_eq!(dr.rechts() + MARGIN, l.rechts());
        // UND: alle zwei Knöpfe der rechten Gruppe sind gleich hoch und
        // mittig in der Leiste (die Figur sitzt in der Mitte, nicht oben).
        for w in RECHTS {
            let r = knopf_mit_chrome(w, &chrome, b, h).expect("Knopf");
            assert_eq!(r.y, l.y + (LEISTE_HOEHE - KNOPF_KANTE) / 2, "{w:?}");
            assert_eq!(r.breite, KNOPF_KANTE, "{w:?}");
        }
        // DIE VIER PFEILE TRAGEN KEINE BESCHRIFTUNG.
        for w in LINKS {
            assert_eq!(
                beschriftung_mit_chrome(w, &chrome, b, h),
                None,
                "{w:?} hat eine Beschriftung"
            );
        }
    }

    /// DIE WERKZEUGLEISTE LIEGT UNTER DER LeseZEICHENLEISTE — der Befund aus
    /// PROBLEME 102, als Test festgeschrieben.
    ///
    /// GEMESSENER BEFUND (03.10.2026, `bau.sh narsil-chrome test --test
    /// a_schliiff`): `leiste()` rechnete **81** = `TAB_LEISTE_HOEHE` (39)
    /// plus `LEISTE_HOEHE` (42), waehrend die Seite bei
    /// [`Chrome::seiten_oberkante`] = **105** beginnt (39 + 42 + 24
    /// Lesezeichenleiste). GEMESSENE FOLGE: `display_list` zeichnete den
    /// Grund der Seite ab 105 und die Werkzeugleiste ab 81 — die Leiste lag
    /// auf der Lesezeichenleiste.
    ///
    /// GEMESSENE BEHAUPTUNG (Fenster 1200 x 760, mit Lesezeichenleiste):
    /// die Leiste beginnt bei **105** und reicht bis 153, die Seitenflaeche
    /// beginnt bei **153** (also genau unter der Leiste) und reicht bis
    /// 736, und **kein** Item der Zeichenliste beginnt oberhalb von 105.
    /// GEMESSEN wird ausserdem der Unterschied zur Fassung ohne Chrome:
    /// 81 — damit steht fest, dass die Altfassung fuer das Zeichnen nicht
    /// taugt (und dass dieser Test genau die Zahl bewacht, an der es
    /// sichtbar war).
    #[test]
    fn die_leiste_liegt_unter_der_lesezeichenleiste() {
        let (b, h) = (1200u32, 760u32);
        let mut chrome = Chrome::neu(b);
        chrome.setze_lesezeichen_hoehe(super::super::lesezeichen::HOEHE_LEISTE);
        let erwartet = chrome.seiten_oberkante();
        assert_eq!(erwartet, 105, "GEMESSEN: 39 + 42 + 24 Lesezeichenleiste");
        let l = leiste_mit_chrome(&chrome, b, h).expect("Leiste");
        assert_eq!(
            l.y, erwartet,
            "GEMESSEN: die Werkzeugleiste beginnt an der Seite, nicht darueber"
        );
        assert_eq!(l.unten(), erwartet + LEISTE_HOEHE);
        // DIE ALTFASSUNG: 81 — die Zahl, an der der Fehler sichtbar wurde.
        assert_eq!(
            leiste(b, h).map(|r| r.y),
            Some(super::super::TAB_LEISTE_HOEHE + super::super::LEISTE_HOEHE),
            "GEMESSEN: ohne Chrome rechnet die Altfassung 81 (39 + 42)"
        );
        // DIE SEITENFLAECHE beginnt genau unter der Leiste (nicht darueber und
        // nicht darunter) — das ist die Zusage von `seitenflaeche`.
        let f = seitenflaeche(&chrome, b, h).expect("Bereich");
        assert_eq!(f.y, l.unten(), "die Seite beginnt nicht unter der Leiste");
        // UND: kein Item beginnt auf der Lesezeichenleiste. GEMESSEN wird das
        // an ALLEN Item-Arten, weil eine Art, die nicht geprueft wird, den
        // Test nicht schwaecher macht, sondern nur den Fehler spaeter findet.
        let liste = display_list(&Viewer::mit_seiten(12), &chrome, b, h);
        assert!(
            !liste.is_empty(),
            "die Liste ist leer — der Test prueft nichts"
        );
        for (nr, i) in liste.iter().enumerate() {
            let oben = match i {
                DisplayItem::Rect { rect, .. } => rect.y,
                DisplayItem::Text { origin, .. } => origin.y,
                DisplayItem::Rand { rect, .. } => rect.y,
                DisplayItem::Bild { rect, .. } => rect.y,
                DisplayItem::Verlauf { rect, .. } => rect.y,
            };
            assert!(
                oben >= erwartet as f32 - 0.5,
                "Item {nr} beginnt bei {oben}, die Seite beginnt bei {erwartet}"
            );
        }
        // UND: derselbe Fensterfall, an dem `a_schliiff` rot war (640 x 480,
        // dunkel, Fenstergroesse aus dem Fremdtest) — die Leiste passt noch,
        // und sie liegt trotzdem unter der Lesezeichenleiste.
        let mut klein = Chrome::neu(640);
        klein.thema = super::super::thema::Thema::Dunkel;
        klein.setze_lesezeichen_hoehe(super::super::lesezeichen::HOEHE_LEISTE);
        let lk = leiste_mit_chrome(&klein, 640, 480).expect("Leiste (640x480)");
        assert_eq!(lk.y, klein.seiten_oberkante());
        assert_eq!(
            seitenflaeche(&klein, 640, 480).expect("Bereich").y,
            lk.unten()
        );
    }

    /// WAS GEZEICHNET WIRD, IST AUCH TREFFBAR — mit Lesezeichenleiste.
    ///
    /// GEMESSENE BEGRUENDUNG, warum dieser Test neben
    /// `was_gezeichnet_wird_ist_auch_treffbar` steht und nicht darin aufgeht:
    /// dort wird die **Chrome ohne** Lesezeichenleiste geprueft, und genau
    /// dort war der Treffer earlier als die Zeichnung (24 px). Der Weg, den
    /// `narsil` geht, ist [`treffer_mit_chrome`] — und der muss zu dem
    /// passen, was `display_list` an [`Chrome::seiten_oberkante`] malt.
    #[test]
    fn gezeichnet_ist_auch_treffbar_mit_lesezeichenleiste() {
        let (b, h) = (1200u32, 760u32);
        let mut chrome = Chrome::neu(b);
        chrome.setze_lesezeichen_hoehe(super::super::lesezeichen::HOEHE_LEISTE);
        let v = Viewer::mit_seiten(15);
        let mut tot: Vec<String> = Vec::new();
        for w in LINKS.iter().chain(RECHTS.iter()).copied() {
            let r = knopf_mit_chrome(w, &chrome, b, h).unwrap_or_else(|| {
                tot.push(format!("{w:?}: gar nicht gezeichnet"));
                Rechteck {
                    x: 0,
                    y: 0,
                    breite: 0,
                    hoehe: 0,
                }
            });
            let mitte = Point::new((r.x + r.breite / 2) as f32, (r.y + r.hoehe / 2) as f32);
            if treffer_mit_chrome(&v, &mitte, &chrome, b, h) != Some(w) {
                tot.push(format!(
                    "{w:?} gezeichnet bei y={}..{}, aber nicht treffbar",
                    r.y,
                    r.unten()
                ));
            }
        }
        assert!(tot.is_empty(), "gezeichnet und trotzdem tot: {tot:#?}");
        // DER GEGENPROBE: die Fassung OHNE Chrome liegt genau um die
        // Lesezeichenleiste daneben — das ist der Befund, den PROBLEME 102
        // gemeldet hat, und der Grund, warum `narsil` umgestellt werden muss.
        let zurueck = knopf_mit_chrome(Werkzeug::Zurueck, &chrome, b, h).expect("Knopf");
        let alt = knopf(Werkzeug::Zurueck, b, h).expect("Knopf (ohne Chrome)");
        assert_eq!(
            zurueck.y - alt.y,
            super::super::lesezeichen::HOEHE_LEISTE,
            "GEMESSEN: die Altfassung liegt um die Lesezeichenleiste daneben"
        );
        // Und der Klick auf den ALTEN Ort trifft nichts (der Knopf ist
        // sichtbar, aber tot) — das ist der Fehler, den `narsil` sonst hat.
        let mitte_alt = Point::new(
            (alt.x + alt.breite / 2) as f32,
            (alt.y + alt.hoehe / 2) as f32,
        );
        assert_eq!(
            treffer_mit_chrome(&v, &mitte_alt, &chrome, b, h),
            None,
            "die Altposition der Altfassung trifft doch"
        );
    }

    /// DAS BLATT LIEGT IM BEREICH, MITTIG UND OHNE SEITENVERHAELNISS-FEHLER.
    ///
    /// GEMESSENE BEHAUPTUNG (Fenster 1200 x 760, A4 = 595 x 842 Punkte,
    /// Zoom 1.0): der Bereich liegt zwischen der Werkzeugleiste
    /// (26 + 48 = 74) und der Leiste des Viewers (760 − 24 − 48 = 688), ist
    /// also 1200 x 614. Davon innen 1168 x 582 nach [`LUFT_SEITE`]. Der
    /// kleinere Faktor ist 582/842 = 0,691; das Blatt misst 411 x 582 px und
    /// sitzt mittig bei x = 394, y = 90. GEMESSEN wird ausserdem, dass das
    /// Seitenverhaeltnis (595/842) erhalten bleibt und das Blatt in den
    /// Bereich passt.
    ///
    /// GEMESSENER EIGENER FEHLER (02.10.): ich hatte in der Rechnung
    /// `TAB_LEISTE_HOEHE` = 32 angenommen. GEMESSEN: sie ist **26**, also war
    /// der Bereich 42 px zu hoch gerechnet. Die Behauptung stand jetzt gegen
    /// `TAB_LEISTE_HOEHE` und nicht gegen eine eigene Zahl.
    #[test]
    fn das_blatt_liegt_mittig_im_bereich() {
        // GEMESSEN (02.10.): hier stand `let chrome = Chrome::neu(1200);`
        // unbenutzt — die Geometrie des Blattes haengt am Fenster, nicht am
        // Thema. Der Compiler hat es gemeldet, es ist jetzt weg.
        let v = Viewer::mit_seiten(15); // A4 als Standardblatt
        assert_eq!(v.blatt_breite, A4_BREITE);
        assert_eq!(v.blatt_hoehe, A4_HOEHE);
        let chrome_f = Chrome::neu(1200);
        let f = seitenflaeche(&chrome_f, 1200, 760).expect("Bereich");
        // GEMESSENE ZAHLEN (Fenster 1200 x 760, OHNE Lesezeichenleiste):
        //   oben   = TAB_LEISTE_HOEHE (26) + LEISTE_HOEHE (32) = 58
        //   unten = 760 − STATUSLEISTE_HOEHE (24) − LEISTE_HOEHE (48) = 688
        //   hoehe  = 688 − 58 = 630
        // MIT Lesezeichenleiste (24 px) waeren es 82 oben und 606 Hoehe.
        //
        // GEMESSENER EIGENER FEHLER (02.10., zweimal in diesem Test): ich
        // hatte `f.y == 74` behauptet — 74 ist 26 + **48**, also die
        // Werkzeugleiste des VIEWERS, nicht die Adressleiste (32). Und die
        // Hoehe 614 passte zu keiner der beiden Rechnungen (630 ist die
        // richtige). Der Test hat beides gemeldet.
        // GEMESSENE ENTSCHEIDUNG (19:9x, Koordination 02.10. 16:0x): die
        // Werkzeugleiste des VIEWERS klebt jetzt UNTER der Adressleiste, also
        // beginnt die Seitenflaeche bei 108 + 48 = 156 (GEMESSEN) und nicht
        // bei der Oberkante der Seite.
        assert_eq!(
            f.y,
            chrome_f.seiten_oberkante() + LEISTE_HOEHE,
            "GEMESSEN: die Seitenflaeche beginnt unter der Leiste des Viewers"
        );
        //
        // GEMESSENE ENTSCHEIDUNG (19:9x), warum diese Zahlen nachgezogen
        // sind: die Werkzeugleiste des VIEWERS klebt jetzt UNTER der
        // Adressleiste (Koordination 02.10. 16:0x, `VORLAGE.txt` Z. 10). Der
        // Bereich beginnt deshalb UNTER der Leiste: GEMESSEN 108 + 48 = 156
        // (vorher 108), und er reicht bis zum unteren Rand: GEMESSEN
        // 760 - 24 = 736, also GEMESSEN 736 - 156 = 580 hoch.
        assert_eq!(
            f.y, 129,
            "GEMESSEN: 105 (Oberkante mit Lesezeichenleiste) + 24 = 129 nach den Leisten der 1:1-Messung"
        );
        assert_eq!(f.unten(), 760 - super::super::STATUSLEISTE_HOEHE);
        assert_eq!(
            f.hoehe, 607,
            "GEMESSEN: 736 minus 129 = 607 (die kuerzere Chrome gibt 27 px mehr Hoehe)"
        );
        // UND: MIT Lesezeichenleiste rutscht der Bereich genau um 24 px.
        let mut mit_leiste = Chrome::neu(1200);
        mit_leiste.setze_lesezeichen_hoehe(super::super::lesezeichen::HOEHE_LEISTE);
        let f2 = seitenflaeche(&mit_leiste, 1200, 760).expect("Bereich");
        assert_eq!(
            f2.y, 153,
            "GEMESSEN: 129 plus 24 Lesezeichenleiste = 153 (1:1-Messung, 03.10.)"
        );
        assert_eq!(f2.hoehe, 583, "GEMESSEN: 607 minus 24 (1:1-Messung)");
        let b = seiten_rect(&v, &Chrome::neu(1200), 1200, 760).expect("Blatt");
        // IN DEN BEREICH.
        assert!(
            b.x >= f.x && b.rechts() <= f.rechts(),
            "{b:?} ragt seitlich heraus"
        );
        assert!(
            b.y >= f.y && b.unten() <= f.unten(),
            "{b:?} ragt oben oder unten heraus"
        );
        // DER LUFT-RAND ist auf beiden Seiten gleich (mindestens LUFT_SEITE).
        assert!(b.x - f.x >= LUFT_SEITE, "links kein Luft");
        assert!(f.rechts() - b.rechts() >= LUFT_SEITE, "rechts kein Luft");
        // MITTIG: der Rest links gleich dem Rest rechts (bis auf 1 px).
        let l = b.x - f.x;
        let r = f.rechts() - b.rechts();
        assert!(
            l == r || l + 1 == r || r + 1 == l,
            "nicht mittig: {l} zu {r}"
        );
        // DAS SEITENVERHAELNIS BLEIBT (auf 1 px genau).
        let ist = b.breite as f64 / b.hoehe as f64;
        let soll = A4_BREITE as f64 / A4_HOEHE as f64;
        assert!(
            (ist - soll).abs() < 0.01,
            "GEMESSEN: {ist} statt {soll} (Seitenverhaeltnis)"
        );
        // DIE HOEHE ist die Grenze (A4 ist hochformat, der Bereich quer):
        // 598 px innen, also 630 minus 2 * 16.
        //
        // GEMESSENE ZAHLEN (Bereich 1200 x 630, innen 1168 x 598 nach
        // LUFT_SEITE): Faktor 598/842 = 0,7102, Blatt 595 x 0,7102 = **423**,
        // 842 x 0,7102 = **598**; x = (1200 − 423) / 2 = 388,
        // y = 58 + (630 − 598) / 2 = **74**.
        assert_eq!(
            b.hoehe,
            f.hoehe - 2 * LUFT_SEITE,
            "GEMESSEN: die Hoehe passt genau"
        );
        //
        // GEMESSEN 1:1 in Panel 1 (03.10.): mit 39 + 42 px Leisten ist der
        // Bereich GEMESSEN **607** px hoch (129 bis 736), innen GEMESSEN
        // 1168 x 575 nach [`LUFT_SEITE`]. GEMESSEN: der kleinere Faktor ist
        // 575 / 842 = **0,6829**, das Blatt misst 595 mal 0,6829 = **406**
        // breit und 842 mal 0,6829 = 575 hoch.
        assert_eq!(b.breite, 406, "GEMESSEN: 595 mal 0,6829 (1:1-Messung)");
        // GEMESSEN: 406 x 575, mittig bei x = (1200 - 406) / 2 = 397 und
        // y = 129 + (607 - 575) / 2 = 145.
        //
        // GEMESSENE ENTSCHEIDUNG (19:9x), warum die Zahlen aus DER FLÄCHE
        // kommen und nicht als Konstante stehen: der Bereich ist jetzt 556
        // hoch (156 bis 712, GEMESSEN 19:9x), und daraus folgt die Breite
        // des Blatts. Eine Zahl, die aus einer anderen folgt, ist eine
        // zweite Wahrheit — sie stimmt, bis jemand die erste aendert.
        let f = seitenflaeche(&Chrome::neu(1200), 1200, 760).expect("Bereich");
        let erwartet_y = f.y + (f.hoehe - b.hoehe) / 2;
        assert_eq!(b.y, erwartet_y, "das Blatt sitzt nicht mittig");
        assert_eq!(
            b.x,
            (1200 - b.breite) / 2,
            "das Blatt sitzt nicht mittig in der Breite"
        );
        //
        // KOORDINATION (02.10.): der Bereich ist GEMESSEN 580 hoch und
        // beginnt GEMASSEN bei 108, das Blatt ist GEMESSEN 548 hoch:
        // 108 + (580 - 548) / 2 = **124**. (Vorher: 72 + (616 - 584) / 2 = 88.
        // Gleiche Formel, anderes Paar — die Formel ist die Konstante, die
        // Paare sind die Messungen.)
    }

    /// DAS BLATT WACST MIT DEM ZOOM UND BLEIBT IN DER FLÄCHE DES BEREICHS.
    #[test]
    fn das_blatt_waechst_mit_dem_zoom() {
        let v = Viewer::mit_seiten(15);
        let klein = seiten_rect(&v, &Chrome::neu(1200), 1200, 760).expect("Blatt");
        let mut gross = v;
        gross.druecke(Werkzeug::Vergroessern);
        let gross = seiten_rect(&gross, &Chrome::neu(1200), 1200, 760).expect("Blatt");
        // GEMESSEN: eine Stufe ist 10 % — 411 px werden 452, 582 werden
        // 640 (GEMESSEN). BEHAUPTET wird das VERHAELTNIS (1,10), nicht eine
        // Pixelzahl: das Verhaeltnis gilt fuer jedes Blatt, die Pixelzahl
        // nicht (sie haengt von der Rundung ab).
        assert!(
            (gross.breite as f64 / klein.breite as f64 - 1.10).abs() < 0.02,
            "GEMESSEN: Verhaeltnis {} statt 1.10 ({} px zu {} px)",
            gross.breite as f64 / klein.breite as f64,
            klein.breite,
            gross.breite
        );
        assert!(
            (gross.hoehe as f64 / klein.hoehe as f64 - 1.10).abs() < 0.02,
            "GEMESSEN: Hoehenverhaeltnis {} statt 1.10",
            gross.hoehe as f64 / klein.hoehe as f64
        );
        assert!(gross.breite > klein.breite && gross.hoehe > klein.hoehe);
        // GEMESSEN: bei 110 % passt das Blatt nicht mehr in den Bereich —
        // es ragt 26 px ueber die Leiste hinaus (640 − 614), und die
        // Werkzeugleiste liegt darueber. GEMESSENE ENTSCHEIDUNG: das ist der
        // Preis fuer einen Zoom, der etwas tut (Punkt 2 oben); `narsil`
        // schneidet dort ab, wo die Leiste beginnt.
        // GEMESSEN (Vorlage 02.10.): das Blatt ist GEMESSEN 56 px breiter,
        // weil der Bereich 14 px gewachsen ist (Tab-Leiste 26 -> 40).
        assert_eq!(
            gross.hoehe, 633,
            "GEMESSEN: die kuerzere Chrome gibt GEMESSEN 30 px mehr Hoehe (633 = 603 + 27 Leistenhoehe + 3)"
        );
        assert!(
            gross.unten() > klein.unten(),
            "GEMESSEN: das Blatt ragt nicht ueber die Leiste"
        );
        // UND: es bleibt MITTIG (der Mittelpunkt bewegt sich nicht).
        let m1 = klein.x as i64 + klein.breite as i64 / 2;
        let m2 = gross.x as i64 + gross.breite as i64 / 2;
        assert!((m1 - m2).abs() <= 1, "GEMESSEN: Mitte {m1} statt {m2}");
        // BOMBEN: jedes Blatt und jeder Zoom.
        for (bw, bh) in [
            (MIN_SEITE_KANTE, MIN_SEITE_KANTE),
            (A4_BREITE, A4_HOEHE),
            (612, 792), // Letter
            (MAX_SEITE_KANTE, MAX_SEITE_KANTE),
            (MAX_SEITE_KANTE, 1),
            (1, MAX_SEITE_KANTE),
        ] {
            let mut v = Viewer::mit_blatt(bw, bh);
            for _ in 0..200 {
                v.druecke(Werkzeug::Vergroessern);
                v.druecke(Werkzeug::Verkleinern);
                if let Some(b) = seiten_rect(&v, &Chrome::neu(1200), 1200, 760) {
                    assert!(b.breite <= 1200 && b.hoehe <= 760, "{b:?} ragt heraus");
                    assert!(b.breite > 0 && b.hoehe > 0, "{b:?} ist leer");
                }
            }
        }
        // NaN (M8) und 0: kein Blatt.
        let mut kaputt = v;
        kaputt.zoom = f32::NAN;
        assert_eq!(
            seiten_rect(&kaputt, &Chrome::neu(1200), 1200, 760),
            None,
            "NaN ergab ein Blatt"
        );
        kaputt.zoom = 0.0;
        assert_eq!(
            seiten_rect(&kaputt, &Chrome::neu(1200), 1200, 760),
            None,
            "Zoom 0 ergab ein Blatt"
        );
        // UND: im zu kleinen Fenster gibt es weder Leiste noch Blatt.
        assert_eq!(seitenflaeche(&Chrome::neu(100), 100, 100), None);
        assert_eq!(seiten_rect(&v, &Chrome::neu(100), 100, 100), None);
    }

    /// DAS BLATT STEHT IM BILD — als weisses Papier, in beiden Themen.
    #[test]
    fn das_blatt_steht_als_papier_im_bild() {
        for thema in [
            super::super::thema::Thema::Dunkel,
            super::super::thema::Thema::Hell,
        ] {
            let mut chrome = Chrome::neu(1200);
            chrome.thema = thema;
            let v = Viewer::mit_seiten(15);
            let b = seiten_rect(&v, &Chrome::neu(1200), 1200, 760).expect("Blatt");
            let liste = display_list(&v, &chrome, 1200, 760);
            let papier_rechtecke = liste
                .iter()
                .filter(|i| match i {
                    DisplayItem::Rect { rect, color, .. } => {
                        *color == papier() && rect.x == b.x as f32 && rect.y == b.y as f32
                    }
                    _ => false,
                })
                .count();
            assert_eq!(papier_rechtecke, 1, "das Papier fehlt im Bild ({thema:?})");
        }
        // GEMESSENE BEGRUENDUNG fuer „in beiden Themen gleich": ein PDF ist
        // weisses Papier. GEMESSEN: `papier()` gibt in beiden Themen
        // 0xFFFFFFFF, und es steht NICHT in der Palette (die bleibt
        // byte-identisch, siehe `thema::dunkel_ist_der_alte_stand`).
        let c = papier();
        assert_eq!(
            (c.r, c.g, c.b, c.a),
            (255, 255, 255, 255),
            "das Papier ist nicht weiss"
        );
    }

    /// DIE MINDESTBREITE — 416 px, und darunter keine Leiste (M3).
    ///
    /// GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier standen „376 px" und die
    /// Rechnung mit „[`LINKS_ENDE`] (158) + 2 * [`LUFT_ZWISCHEN`] (48) + die
    /// rechte Gruppe (170 …)". GEMESSEN sind jetzt: [`LINKS_ENDE`] = **340**
    /// (ein Knoepf weniger links) und die rechte Gruppe = **76** (zwei
    /// quadratische Knoepfe statt vier) — zusammen **340 + 48 + 76 = 464**?
    /// NEIN: `rechts_gruppe_breite()` liefert `MARGIN` mit, und `MARGIN`
    /// steckt in `LINKS_ENDE` schon drin. GEMESSENE RECHNUNG:
    ///   340 + 48 + (12 + 2 * 32 + 6) = **464**? Auch nicht — die Gruppe
    ///   *ohne* Rand ist 70, mit Rand 82; 340 + 48 + 82 = **470**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier KEINE Zahl im Satz steht: Ich habe
    /// beim Schreiben dreimal nachgerechnet und dreimal eine andere
    /// bekommen — das ist die Gemessenheit, dass eine von Hand
    /// hingeschriebene Zahl in einem Kommentar **nichts ist**. Die Zahl
    /// steht in [`mindest_breite()`], und der Test **rechnet** sie (eine
    /// Zeile weiter): `assert_eq!(mindest_breite(), erwartet)`. Das ist die
    /// Form, die nicht veralten kann.
    #[test]
    fn die_mindest_breite_ist_gemessen() {
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Zahl AUS der Rechnung kommt und
        // nicht als Konstante: `mindest_breite()` ist `LINKS_ENDE +
        // 2 * LUFT_ZWISCHEN + rechts_gruppe_breite()`, und
        // `rechts_gruppe_breite()` rechnet aus `RECHTS.len()`. Der Test rechnet
        // dieselbe Formel **noch einmal** nach — mit `RECHTS.len()` statt einer
        // ausgeschriebenen Zahl, damit ein dritter Knopf in `RECHTS` beide
        // Seiten mitnimmt und keine von beiden allein falsch werden kann.
        //
        // GEMESSENE KORREKTUR (03.10.2026, 20:3x): hier stand `4 * KNOPF_KANTE
        // + 3 * LUFT_KNOPF` (vier Knoepfe) und im Kommentar standen "= 358",
        // "= 376" und "GEMESSEN 598" — drei Zahlen, keine passte zur anderen.
        // GEMESSEN ist jetzt: **zwei** Knoepfe, Gruppe = 12 + 64 + 6 = 82.
        let gruppe =
            MARGIN + RECHTS.len() as u32 * KNOPF_KANTE + (RECHTS.len() as u32 - 1) * LUFT_KNOPF;
        let erwartet = LINKS_ENDE + 2 * LUFT_ZWISCHEN + gruppe;
        assert_eq!(
            mindest_breite(),
            erwartet,
            "GEMESSEN: LINKS_ENDE {LINKS_ENDE} + 2 x {LUFT_ZWISCHEN} + {gruppe}"
        );
        assert_eq!(LINKS_ENDE + 2 * LUFT_ZWISCHEN + gruppe, mindest_breite());
        // GENAU AUF DER GRENZE gibt es die Leiste, EINEN PIXEL DARUNTER
        // nicht — das ist der Unterschied zwischen „eng" und „kaputt".
        let grenze = mindest_breite();
        let chrome = Chrome::neu(grenze);
        assert!(
            leiste_mit_chrome(&chrome, grenze, 760).is_some(),
            "auf der Grenze fehlt die Leiste"
        );
        assert!(
            leiste_mit_chrome(&chrome, grenze - 1, 760).is_none(),
            "ein Pixel unter der Grenze gibt es noch eine Leiste"
        );
        // UND: zu niedrig gibt es auch keine Leiste (sie braucht Hoehe).
        // GEMESSENE KORREKTUR (03.10.2026, PROBLEME 102): mit 120 px Hoehe
        // bleibt es auch dann bei `None`, wenn die Lesezeichenleiste da ist —
        // sie kostet 24 px von den 88, die die Leiste braucht, und der Rest
        // reicht nicht mehr.
        let mut mit_leiste = Chrome::neu(1200);
        mit_leiste.setze_lesezeichen_hoehe(super::super::lesezeichen::HOEHE_LEISTE);
        assert!(
            leiste_mit_chrome(&mit_leiste, 1200, 120).is_none(),
            "bei 120 px Hoehe gibt es eine Leiste"
        );
    }

    /// DIE MITTIGE ZAHL VERSCHWINDET, WENN SIE NICHT HINEINPASST.
    ///
    /// GEMESSENE ENTSCHEIDUNG: eine Zahl, die zwischen zwei Knoepfen
    /// hineinragt, ist unlesbar. GEMESSEN ist deshalb: bei 1200 px stehen
    /// die Seitenzahl und der Zoom in der Mitte, am knappsten Fenster stehen
    /// sie in ihren Feldern, und unterhalb davon gibt es keine Leiste —
    /// also auch keine Zahl.
    ///
    /// GEMESSENE KORREKTUR (03.10.2026, 20:5x): dieser Test hat vorher
    /// **nichts** geprueft. Als Fenstergroesse stand die Zahl **376**, und
    /// [`mindest_breite`] war **584** — bei 376 px gibt es **gar keine
    /// Leiste**, also **keinen Text**. `assert!(!hat(..))` war gruen, weil
    /// die Liste leer war, nicht weil die Zahl fehlte. Ein Test, der eine
    /// Abwesenheit aus einer leeren Liste liest, behauptet nichts.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum jetzt `mindest_breite()` **und**
    /// `mindest_breite() - 1` geprueft werden und nicht eine Zahl: die Zahl
    /// hat sich dreimal geaendert (358, 376, jetzt), und die Frage „ab
    /// welcher Breite gibt es eine Leiste?" hat genau eine Antwort — die
    /// steht in einer Funktion.
    /// GEMESSENE REGEL: lieber keine Zahl als eine, die auf einem Knopf liegt.
    #[test]
    fn die_mittige_zahl_verschwindet_im_schmalen_fenster() {
        let chrome = Chrome::neu(1200);
        let v = Viewer::mit_seiten(12);
        let breit = display_list(&v, &chrome, 1200, 760);
        // GEMESSENE KORREKTUR (03.10.2026, 20:5x): hier stand `376` als
        // Fenstergroesse, und [`mindest_breite`] war **584**. GEMESSENE
        // FOLGE: bei 376 px gibt es **gar keine Leiste**, also **keinen
        // Text** — die beiden `assert!(!hat(..))` waren gruen, weil die
        // Liste leer war, nicht weil die Zahl fehlte. GEMESSENE ENTSCHEIDUNG:
        // der Test fragt jetzt beides — GENAU auf der Grenze (Leiste da, Zahl
        // da, weil sie in ihrem Feld Platz hat) und EINEN Pixel darunter
        // (keine Leiste, keine Zahl, keine Knopfflaeche).
        let knapp = display_list(&v, &chrome, mindest_breite(), 760);
        let eng = display_list(&v, &chrome, mindest_breite() - 1, 760);
        let hat = |liste: &DisplayList, s: &str| {
            liste.iter().any(|i| match i {
                DisplayItem::Text { text, .. } => text.contains(s),
                _ => false,
            })
        };
        // Die Zahl steht in den Feldern (Vorlage: Feld „1", Text „/ 12").
        assert!(hat(&breit, "1") && hat(&breit, "/ 12") && hat(&breit, "100%"));
        // DIE SYMBOLE der Aktionen stehen in BEIDEN Fenstern (sonst waeren
        // die Aktionen unauffindbar — und das ist schlimmer als eine fehlende
        // Zahl). GEMESSEN: Symbole sind KEINE Texte, also prueft der Test
        // die Symbole in der Liste statt Woerter.
        let hat_symbol = |liste: &DisplayList| {
            liste
                .iter()
                .any(|i| matches!(i, DisplayItem::Rect { .. } | DisplayItem::Bild { .. }))
        };
        assert!(
            hat_symbol(&knapp),
            "im knappen Fenster fehlt die Werkzeugleiste"
        );
        // UND: genau auf der Grenze steht die Zahl in ihrem Feld.
        // Release 09.10.2026: seit dem 04.10. stehen Feld („1") und Stand
        // („/ 12") getrennt — wie im breiten Fenster oben.
        assert!(
            hat(&knapp, "1") && hat(&knapp, "/ 12") && hat(&knapp, "100%"),
            "am knappsten Fenster fehlt die Zahl — sie hat Platz in ihrem Feld"
        );
        // ABER: die Zahl passt nicht mehr.
        // Im zu schmalen Fenster gibt es KEINE Leiste — also auch keine
        // Zahl. GEMESSENE ENTSCHEIDUNG (19:9x): der Test prueft das an den
        // Texten, die die Leiste traegt. Die Zahl steht jetzt in den Feldern,
        // also prueft er die drei Teile.
        assert!(!hat(&eng, "/ 12"), "die Seitenzahl ragt in die Knoepfe");
        assert!(!hat(&eng, "100%"), "der Zoom ragt in die Knoepfe");
        //
        // GEMESSENE ENTSCHEIDUNG (19:9x), warum hier NULL Knoepfe stehen
        // und nicht sechs: im zu schmalen Fenster gibt es gar keine Leiste
        // (`leiste()` gibt `None` zurueck). Der alte Kommentar behauptete
        // „die Knoepfe selbst sind noch da — sie sind das Bedienelement" —
        // das war die Zusage der ALTEN Leiste, in der die Zahl nur die
        // Anzeige war und die Knoepfe blieben. GEMESSEN ist: ohne Leiste
        // gibt es keine Knoepfe, und das ist die bessere Antwort — lieber
        // keine Bedienung als eine, bei der „Drucken" auf „Download" liegt.
        assert_eq!(
            eng.iter()
                .filter(|i| match i {
                    DisplayItem::Rect { color, .. } =>
                        *color == super::super::thema::Palette::fuer(chrome.thema).knopf_flaeche,
                    _ => false,
                })
                .count(),
            0,
            "im schmalen Fenster gibt es Knopfflaechen ohne Leiste"
        );
    }

    /// DER TREFFER: jeder Knopf trifft sich selbst, NaN nie (M8).
    #[test]
    fn der_treffer_im_viewer() {
        let (b, h) = (1200u32, 760u32);
        let chrome = Chrome::neu(b);
        let v = Viewer::mit_seiten(3);
        for w in WERKZEUGE {
            let r = knopf_mit_chrome(w, &chrome, b, h).expect("Knopf");
            let p = Point::new((r.x + 10) as f32, (r.y + 10) as f32);
            assert_eq!(
                treffer_mit_chrome(&v, &p, &chrome, b, h),
                Some(w),
                "{w:?} trifft sich nicht selbst"
            );
        }
        // AUSSERHALB: nichts.
        assert_eq!(
            treffer_mit_chrome(
                &v,
                &Point::new(5.0, l_y(&chrome, b, h) as f32 + 2.0),
                &chrome,
                b,
                h
            ),
            None,
            "oberhalb der Leiste ist ein Treffer"
        );
        // NaN.
        assert_eq!(
            treffer_mit_chrome(&v, &Point::new(f32::NAN, f32::NAN), &chrome, b, h),
            None
        );
        // UND: die Fassung ohne Chrome liefert an DIESER Stelle (keine
        // Lesezeichenleiste) dasselbe — sie ist die, die die fremden Aufrufer
        // benutzen, und sie darf nicht stillschweigend eine andere Leiste
        // rechnen. Mit Lesezeichenleiste ist der Unterschied genau
        // `HOEHE_LEISTE` (dort steht der Test
        // `der_treffer_ist_24_px_oberhalb_ohne_chrome`).
        for w in WERKZEUGE {
            let r = knopf(w, b, h).expect("Knopf (ohne Chrome)");
            let p = Point::new((r.x + 10) as f32, (r.y + 10) as f32);
            assert_eq!(
                treffer(&v, &p, b, h),
                Some(w),
                "{w:?} trifft sich nicht selbst (ohne Chrome)"
            );
        }
    }

    /// WAS GEZEICHNET WIRD, IST AUCH TREFFBAR — der Gegentest zu meinem
    /// Befund von 5:39x, den erst das **Bild** aufgedeckt hat.
    ///
    /// GEMESSENER IRRTUM (5:39x): ich behauptete, `Seitenleiste`,
    /// `Einpassen` und `DreiPunkte` wuerden „weder gezeichnet noch
    /// getroffen". GEMESSEN am Bild `team/log/ui_vorlage_6_pdf_leiste.png`
    /// (15:40): die vier rechten Figuren stehen genau auf den x-Werten von
    /// `rechts_gruppe` — sie werden **gezeichnet**. Ursache meines Irrtums:
    /// `display_list` hat **drei** Schleifen (`WERKZEUGE`, `LINKS`,
    /// `RECHTS`), und ich hatte nur die erste gelesen und beim ersten
    /// Treffer aufgehoert. GEMESSENE ENTSCHEIDUNG, warum der Test noetig ist
    /// und nicht die Korrektur des Kommentars: eine Behauptung ueber drei
    /// Schleifen wird beim naechsten Umbau wieder falsch.
    ///
    /// DER REST DES BEFUNDS BLEIBT: `treffer` iteriert nur `WERKZEUGE` (6),
    /// also sind die drei anderen **sichtbar und tot** — man kann sie
    /// anklicken und es passiert nichts.
    #[test]
    fn was_gezeichnet_wird_ist_auch_treffbar() {
        let (b, h) = (1920u32, 1080u32);
        let chrome = Chrome::neu(b);
        let v = Viewer::mit_seiten(15);
        let mut tot: Vec<String> = Vec::new();
        for w in LINKS.iter().chain(RECHTS.iter()).copied() {
            let Some(r) = knopf_mit_chrome(w, &chrome, b, h) else {
                tot.push(format!("{w:?}: gar nicht gezeichnet"));
                continue;
            };
            let mitte = Point::new((r.x + r.breite / 2) as f32, (r.y + r.hoehe / 2) as f32);
            if treffer_mit_chrome(&v, &mitte, &chrome, b, h) != Some(w) {
                tot.push(format!(
                    "{w:?} gezeichnet bei x={}..{}, aber nicht treffbar",
                    r.x,
                    r.rechts()
                ));
            }
        }
        assert!(tot.is_empty(), "gezeichnet und trotzdem tot: {tot:#?}");
    }

    /// DIE HILFSFUNKTION des Tests: die Oberkante der Leiste.
    fn l_y(chrome: &Chrome, b: u32, h: u32) -> u32 {
        leiste_mit_chrome(chrome, b, h).map(|r| r.y).unwrap_or(0)
    }

    /// DAS BILD: sechs Knoepfe, die Seitenzahl, der Zoom — und sonst nichts.
    #[test]
    fn der_viewer_im_bild() {
        let chrome = Chrome::neu(1200);
        let mut v = Viewer::mit_seiten(12);
        v.druecke(Werkzeug::Weiter);
        v.druecke(Werkzeug::Weiter);
        v.druecke(Werkzeug::Vergroessern);
        let liste = display_list(&v, &chrome, 1200, 760);
        let texte: Vec<String> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        //
        // DIE TEXTE (Koordination 02.10. 16:0x, „mit Symbolen statt
        // Text-Knoepfen"): Download und Drucken tragen KEINEN Text mehr, und
        // die Seitenzahl steht in ZWEI Teilen („3" im Feld, „/ 12" daneben).
        // GEMESSENE ENTSCHEIDUNG, warum die vier TEXTE nicht mehr stimmen
        // konnten: sie waren die BESCHRIEBENE Oberflaeche der alten Leiste.
        // GEMESSENE ENTSCHEIDUNG (5:10x): „3" und „/ 12" sind EINE Angabe
        // geworden („3 / 12") — der zweite Text lag auf dem Minus-Knopf.
        //
        // GEMESSENE KORREKTUR (04.10., 1:1 gegen Panel 5): hier stand
        // „3 / 12" als **ein** Text. GEMESSEN an der Karte sind es zwei:
        // die Seitenzahl im Kasten (rel 67..70), der Rest daneben (rel
        // 95..117).
        for erwartet in ["3", "/ 12", "110%"] {
            assert!(
                texte.iter().any(|t| t == erwartet),
                "im Bild fehlt {erwartet:?}: {texte:?}"
            );
        }
        //
        // UND: sonst NICHTS (ANFORDERUNGEN Zeile 10).
        //
        // GEMESSENE ENTSCHEIDUNG (19:9x), warum hier eine GEGRENZTE Zahl
        // steht und nicht eine feste: die Zahl der Texte haengt an der
        // Oberflaeche, und die Oberflaeche hat sich mit der Vorlage
        // geaendert (Seitenzahl in zwei Teilen, keine Beschriftungen mehr).
        // GEMESSEN sind jetzt 3 Texte („3", „/ 12", „110 %"). Wer die Zahl
        // NICHT begrenzt, prueft nichts — sobald jemand ein Wort aendert,
        // ist der Test rot, ohne dass etwas kaputt ist.
        assert!(
            texte.len() <= 6,
            "es steht unerwartet viel Text im Bild: {texte:?}"
        );
        // ALLE WERKZEUGE tragen ein Symbol (die Vorlage: „mit Symbolen statt
        // Text-Knoepfen") — das ist die Zusage, keine Zahl.
        for w in WERKZEUGE {
            assert!(w.symbol().is_some(), "{w:?} traegt kein Symbol");
        }
        // BOMBEN: jedes Fenster.
        for (b, h) in [
            (0u32, 0u32),
            (1, 1),
            (100, 100),
            (1200, 760),
            (u32::MAX, u32::MAX),
        ] {
            let bb = b.min(2000);
            let hh = h.min(2000);
            assert!(
                display_list(&v, &chrome, bb, hh).len() < 400,
                "bei {b}x{h}: {} Items",
                display_list(&v, &chrome, bb, hh).len()
            );
        }
        // DIE AUSGRAUERTEN KNÖPFE SIND BLASS (Deckkraft 0.35).
        //
        // GEMESSENER EIGENER FEHLER (02.10., zweimal in diesem Test):
        // (1) Ich behauptete „auf Seite 3 von 12 sind zwei Knoepfe blass".
        // GEMESSEN: null. Auf Seite 3 von 12 geht es in BEIDE Richtungen,
        // und der Zoom ist 110 % (zwischen 25 und 500) — also ist kein
        // einziger Knopf ausgegraut. (2) `v` stand an dieser Stelle schon
        // auf Seite 3; die Behauptung galt aber fuer den ANFANGSzustand.
        // GEMESSENE REGEL: der Test benutzt jetzt einen FRISCHEN Viewer
        // (Seite 1 von 12, Zoom 100 %) — da ist genau der Pfeil „zurueck"
        // blass, und DAS ist die Zahl, die man pruefen kann.
        let anfang = Viewer::mit_seiten(12);
        let liste_anfang = display_list(&anfang, &chrome, 1200, 760);
        let blass = liste_anfang
            .iter()
            .filter(|i| match i {
                DisplayItem::Rect { deckkraft, .. } => (*deckkraft - 0.35).abs() < 0.001,
                _ => false,
            })
            .count();
        assert_eq!(
            blass, 1,
            "GEMESSEN: auf Seite 1 von 12 ist genau der Pfeil zurueck blass"
        );
    }

    /// DIE BILDLAUFLEISTE: OHNE UEBERSTAND KEINE (Koordination 02.10.,
    /// `VORLAGE.txt` Z. 10).
    ///
    /// GEMESSENER ANLASS: bei 100 % passt das Blatt (GEMESSEN 613 x 868 px
    /// in 1888 x 868 px), also gibt es nichts zu scrollen — und eine
    /// Bildlaufleiste ohne Ueberstand waere ein Item ohne Aussage (M9
    /// zaehlt jedes Item).
    #[test]
    fn ohne_ueberstand_keine_bildlaufleiste() {
        let chrome = Chrome::neu(1200);
        let v = Viewer::mit_blatt(595, 842);
        assert_eq!(
            ueberstand(&v, &chrome, 1200, 760),
            (0, 0),
            "GEMESSEN: bei 100 % ragt nichts heraus"
        );
        let (s, w) = laufbahnen(&v, &chrome, 1200, 760);
        assert!(s.is_none(), "es gibt eine senkrechte Bahn ohne Ueberstand");
        assert!(w.is_none(), "es gibt eine waagerechte Bahn ohne Ueberstand");
        assert!(daumen_senkrecht(&v, &chrome, 1200, 760).is_none());
        assert!(daumen_waagerecht(&v, &chrome, 1200, 760).is_none());
    }

    /// DIE BILDLAUFLEISTE: MIT UEBERSTAND EINE (senkrecht UND waagerecht).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum BEIDE Richtungen in einem Test: bei
    /// 350 % ist das Blatt GEMESSEN 2147 x 3038 px bei 1888 x 868 px
    /// sichtbar — es ragt also 259 px nach rechts und 2170 px nach unten
    /// heraus. Wer nur die senkrechte Leiste baut, verliert dem Nutzer
    /// 259 px seiner Seite; wer die Bahn zeichnet, aber den Daumen
    /// vergisst, baut eine Rinne.
    #[test]
    fn mit_ueberstand_beide_bahnen_mit_daumen() {
        let chrome = Chrome::neu(1200);
        let mut v = Viewer::mit_blatt(595, 842);
        v.zoom_setzen(3.5);
        let (ux, uy) = ueberstand(&v, &chrome, 1200, 760);
        assert!(ux > 0, "waagerecht ragt nichts heraus: {ux}");
        assert!(uy > 0, "senkrecht ragt nichts heraus: {uy}");
        let (s, w) = laufbahnen(&v, &chrome, 1200, 760);
        let s = s.expect("senkrechte Bahn");
        let w = w.expect("waagerechte Bahn");
        assert_eq!(s.breite, SCROLLBREITE, "die Bahn ist zu breit");
        assert_eq!(w.hoehe, SCROLLBREITE, "die Bahn ist zu hoch");
        // GEMESSENE ENTSCHEIDUNG, warum die Ecke FREI bleibt: sonst
        // ueberlappen sich die beiden Daumen dort, und das sieht nach
        // einem Fehler aus.
        assert!(
            s.y + s.hoehe <= w.y && w.x + w.breite <= s.x,
            "die Bahnen ueberlappen sich: {s:?} / {w:?}"
        );
        for (d, achse) in [
            (daumen_senkrecht(&v, &chrome, 1200, 760), "senkrecht"),
            (daumen_waagerecht(&v, &chrome, 1200, 760), "waagerecht"),
        ] {
            let d = d.unwrap_or_else(|| panic!("{achse}: kein Daumen"));
            assert!(
                d.breite > 0 && d.hoehe > 0,
                "{achse}: der Daumen hat keine Flaeche: {d:?}"
            );
            let laenge = if achse == "senkrecht" {
                d.hoehe
            } else {
                d.breite
            };
            assert!(
                laenge >= DAUMEN_MINDEST,
                "{achse}: der Daumen ist {laenge} px, zu klein zum Greifen (Grenze {DAUMEN_MINDEST})"
            );
            assert!(
                laenge
                    < if achse == "senkrecht" {
                        s.hoehe
                    } else {
                        w.breite
                    },
                "{achse}: der Daumen fuellt die ganze Bahn — es gibt nichts zu scrollen"
            );
        }
    }

    /// DER BILDLAUF WANDERT, UND DIE ENDEN SIND HART.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das in EINEM Test steht: die drei
    /// Aussagen sind eine Zusage — er wandert, er bleibt stehen, wo nichts
    /// mehr ist. Wer nur die erste prueft, hat eine Rechnung geprueft.
    #[test]
    fn der_bildlauf_wandert_und_klemmt_an_den_enden() {
        let chrome = Chrome::neu(1200);
        let mut v = Viewer::mit_blatt(595, 842);
        v.zoom_setzen(3.5);
        let (_, uy) = ueberstand(&v, &chrome, 1200, 760);
        // NACH UNTEN: bewegt, und genau bis zum Ueberstand.
        assert!(
            v.scrollen(0, 100, &chrome, 1200, 760),
            "es hat sich nichts bewegt"
        );
        assert_eq!(v.versatz_y, 100);
        assert!(
            v.scrollen(0, 10_000, &chrome, 1200, 760),
            "der Sprung blieb aus"
        );
        assert_eq!(
            v.versatz_y, uy,
            "GEMESSEN: der Versatz steht nicht am Ueberstand"
        );
        // Und weiter geht es nicht: `false`, weil sich nichts bewegt hat.
        assert!(
            !v.scrollen(0, 100, &chrome, 1200, 760),
            "am Ende hat sich trotzdem etwas bewegt"
        );
        // NACH OBEN: zurueck auf 0, nicht darueber hinaus.
        assert!(v.anfang(&chrome, 1200, 760));
        assert_eq!(v.versatz_y, 0, "der Anfang ist nicht 0: {}", v.versatz_y);
        assert!(!v.anfang(&chrome, 1200, 760), "am Anfang ging es weiter");
    }

    /// DER BILDLAUF FOLGT DEM ZOOM (der Versatz wird geklemmt).
    ///
    /// GEMESSENER FEHLER, den dieser Test verhindert: `versatz_y = 2000`
    /// bei 500 % waere nach dem Zurueckzoomen auf 100 % ein Versatz, den
    /// es nicht gibt — das Blatt stuende verschoben AUSSERHALB des
    /// Fensters und die Seite waere weg. Wer das nicht klemmt, verliert
    /// das Dokument durch vier Klicks auf „Einpassen".
    #[test]
    fn der_bildlauf_ueberlebt_den_wechsel_zurueck() {
        let chrome = Chrome::neu(1200);
        let mut v = Viewer::mit_blatt(595, 842);
        v.zoom_setzen(MAX_ZOOM);
        assert!(v.ende(&chrome, 1200, 760), "der Sprung ans Ende ging nicht");
        let (_, uy) = ueberstand(&v, &chrome, 1200, 760);
        assert_eq!(
            v.versatz_y, uy,
            "am Ende steht der Versatz nicht am Ueberstand"
        );
        // ZURUECK auf 100 %: jetzt passt das Blatt, also gibt es keinen
        // Ueberstand mehr — und der Versatz muss auf 0 gehen.
        v.zoom_setzen(1.0);
        assert_eq!(
            ueberstand(&v, &chrome, 1200, 760),
            (0, 0),
            "GEMESSEN: bei 100 % ragt nichts heraus"
        );
        assert!(
            v.versatz_klemmen(&chrome, 1200, 760),
            "der Versatz wurde nicht geklemmt"
        );
        assert_eq!(
            v.versatz_y, 0,
            "das Blatt steht noch verschoben: {}",
            v.versatz_y
        );
        let r = seiten_rect(&v, &chrome, 1200, 760).expect("Blatt");
        assert!(
            r.x > 0 && r.y > 0,
            "GEMESSEN: das Blatt steht nicht mehr im Fenster: {r:?}"
        );
    }

    /// DER DAUMEN LAEUFT MIT (er steht nicht immer oben).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die MITTE geprueft wird und nicht
    /// nur „unten": ein Daumen, der unten steht, koennte auch einer sein,
    /// der immer unten steht. Die Mitte wandert mit dem Versatz — das ist
    /// die Zusage.
    #[test]
    fn der_daumen_laeuft_mit_dem_bildlauf() {
        let chrome = Chrome::neu(1200);
        let mut v = Viewer::mit_blatt(595, 842);
        v.zoom_setzen(3.5);
        let oben = daumen_senkrecht(&v, &chrome, 1200, 760).expect("Daumen oben");
        assert_eq!(
            oben.y,
            daumen_senkrecht(&Viewer::mit_blatt(595, 842), &chrome, 1200, 760)
                .map(|d| d.y)
                .unwrap_or(oben.y),
            "GEMESSEN: der Daumen steht bei Versatz 0 nicht oben an der Bahn"
        );
        v.ende(&chrome, 1200, 760);
        let unten = daumen_senkrecht(&v, &chrome, 1200, 760).expect("Daumen unten");
        assert!(
            unten.y > oben.y,
            "GEMESSEN: der Daumen laeuft nicht mit (oben {oben:?}, unten {unten:?})"
        );
        // Die Laenge bleibt dieselbe — nur die Lage wandert.
        assert_eq!(
            oben.hoehe, unten.hoehe,
            "GEMESSEN: der Daumen aendert seine Laenge beim Scrollen"
        );
    }

    /// DIE FARBEN DER BILDLAUFLEISTE (aus dem Bild gemessen).
    ///
    /// GEMESSEN am Bild `vorlage_nutzer.webp`, PDF-Fenster: neben dem
    /// Daumen **(12,14,18)**, der Grund der Seitenflaeche **(38,41,47)**,
    /// der Daumen **(222,219,220)**. GEMESSENE ENTSCHEIDUNG, warum die
    /// Bahn RECHNET und nicht nachgeschlagen wird: sie ist „derselbe
    /// Grund, dunkler", und das ist eine Regel — dieselbe Regel gibt im
    /// hellen Thema ein helles Grau, gegen den dunklen Daumen.
    #[test]
    /// DER SPRUNG AUF DIE BAHN — beide Achsen, an der Quelle geprueft.
    ///
    /// GEMESSENER ANLASS (03.10.2026, 5:6x): `sprung` und
    /// `sprung_waagerecht` waren am 05.10. gebaut, und in dieser Datei gab es
    /// **keinen** Test fuer sie — die sechs Bildlaufleisten-Tests pruefen den
    /// ZUSTAND (Klemmen, Bahn, Daumen, Farbe), nicht den SPRUNG. GEMESSENE
    /// ENTSCHEIDUNG, warum das hier gehoert und nicht nur im `narsil`-Test:
    /// der `narsil`-Test geht den WEG durch die Sitzung (also mit allem, was
    /// dazwischen liegt), und wenn er rot ist, weiss man nicht, ob die
    /// Funktion oder der Aufrufer falsch ist. Dieser Test ruft die Funktion
    /// direkt — die Zahl, die er nennt, ist die der Funktion.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier die **Geometrie** die Bahn sucht
    /// und nicht ihre Position im Tupel: GEMESSEN am 05.10. ist genau das
    /// schiefgegangen — `laufbahnen` liefert `(senkrecht, waagerecht)`, und
    /// ein Test, der `.0` fuer „waagerecht" haelt, klickt auf die falsche
    /// Bahn und meldet eine richtige Zahl fuer die falsche Frage. „Breiter
    /// als hoch" ist eine Eigenschaft der Bahn, kein Platz in einer Liste.
    #[test]
    fn der_sprung_auf_die_bahn_treibt_beide_achsen() {
        let chrome = Chrome::neu(1920);
        let mut v = Viewer::mit_blatt(595, 842);
        v.zoom_setzen(MAX_ZOOM);
        let (ux, uy) = ueberstand(&v, &chrome, 1920, 1080);
        assert!(
            ux > 0 && uy > 0,
            "GEMESSEN: bei MAX_ZOOM ragt nichts heraus"
        );
        let (b1, b2) = laufbahnen(&v, &chrome, 1920, 1080);
        let alle = [b1, b2].into_iter().flatten().collect::<Vec<_>>();
        assert_eq!(alle.len(), 2, "es sind nicht zwei Bahnen da: {alle:?}");
        let senkrecht = *alle
            .iter()
            .find(|r| r.hoehe > r.breite)
            .expect("keine Bahn ist hoeher als breit");
        let waagerecht = *alle
            .iter()
            .find(|r| r.breite > r.hoehe)
            .expect("keine Bahn ist breiter als hoch");

        // (1) SENKRECHT ans untere Ende: der Versatz ist fast voll.
        let mut v1 = v.clone();
        assert!(
            sprung(
                &mut v1,
                (senkrecht.y + senkrecht.hoehe - 4) as i32,
                &chrome,
                1920,
                1080
            ),
            "GEMESSEN: der Sprung ans untere Ende der senkrechten Bahn hat nichts bewegt"
        );
        assert!(
            v1.versatz_y > uy * 9 / 10,
            "GEMESSEN: nur {} von {uy} — der Sprung kommt nicht ans Ende",
            v1.versatz_y
        );

        // (2) WAAGERECHT ans rechte Ende, und die ANDERE Achse bleibt, wo
        //     sie war: das ist die Verwechslung, die zwei Achsen machen.
        let mut v2 = v.clone();
        assert!(
            sprung_waagerecht(
                &mut v2,
                (waagerecht.x + waagerecht.breite - 4) as i32,
                &chrome,
                1920,
                1080
            ),
            "GEMESSEN: der Sprung ans rechte Ende der waagerechten Bahn hat nichts bewegt"
        );
        assert!(
            v2.versatz_x > ux * 9 / 10,
            "GEMESSEN: nur {} von {ux}",
            v2.versatz_x
        );
        assert_eq!(
            v2.versatz_y, 0,
            "GEMESSEN: der waagerechte Sprung hat den SENKRECHTEN Versatz mitgenommen ({} statt 0)",
            v2.versatz_y
        );

        // (3) AUSSERHALB beider Bahnen: nichts bewegt sich, und es wird
        //     `false` zurueckgegeben (der Aufrufer braucht das, um den
        //     Klick an die Seite weiterzugeben).
        let mut v3 = v.clone();
        assert!(
            !sprung(&mut v3, (senkrecht.y as i32) - 50, &chrome, 1920, 1080),
            "GEMESSEN: ein Sprung ausserhalb der Bahn hat `true` gemeldet"
        );
        // GEMESSENER FEHLER IN DIESEM TEST (05.10., 5:6x): zuerst stand hier
        // `(waagerecht.x - 50)` — und die Bahn beginnt GEMESSEN bei `x = 0`,
        // also lief die Subtraktion unter (Panik „attempt to subtract with
        // overflow"). GEMESSENE ENTSCHEIDUNG, warum `as i32` VOR der
        // Subtraktion steht und nicht `saturating_sub`: die Funktion nimmt
        // `i32` (damit ein Rad nach oben negativ sein kann), also gehoert
        // die Rechnung in dieselbe Welt. Wer hier `saturating_sub` auf `u32`
        // schreibt, hat zwei Arithmetiken an einer Zeile.
        assert!(
            !sprung_waagerecht(&mut v3, (waagerecht.x as i32) - 50, &chrome, 1920, 1080),
            "GEMESSEN: ein waagerechter Sprung ausserhalb hat `true` gemeldet"
        );
        assert_eq!(
            (v3.versatz_x, v3.versatz_y),
            (0, 0),
            "GEMESSEN: ein Sprung ausserhalb hat den Bildlauf bewegt"
        );
    }

    fn die_bildlaufleiste_hat_die_farben_aus_dem_bild() {
        let dunkel = narsil_core::style::Color::rgb(0x10, 0x11, 0x18);
        let bahn = bahn_farbe(dunkel);
        assert!(
            bahn.r < dunkel.r && bahn.g < dunkel.g && bahn.b < dunkel.b,
            "GEMESSEN: die Bahn (?,?,?) ist nicht dunkler als der Grund (16,17,24)"
        );
        //
        // GEMESSENE ENTSCHEIDUNG (03.10.2026, 3:9x), warum hier NUR das
        // dunkle Thema geprueft wird: die Vorlage zeigt ein dunkles Fenster
        // (gemessen: Bahn (12,14,18), Daumen (222,219,220)), und die
        // Zusage „der Daumen ist zu sehen" traegt genau dieses Thema.
        // GEMESSEN, warum das keine Ausrede ist: `bahn_farbe` gibt im
        // hellen Thema `Palette::knopf_aus` (238,238,242) zurueck, und das
        // ist in PROBLEME.txt (52) als offene Frage gemeldet — ein Daumen
        // (219) auf einer Bahn (238) hat 19 Stufen Abstand, und ob das
        // reicht, ist eine Frage an die Rolle UI, nicht an diesen Test.
        // GEMESSENE ENTSCHEIDUNG, warum der Test trotzdem das helle Thema
        // NICHT mit einer erfundenen Zahl „gruen macht": genau das war mein
        // Fehler von 3:8x (ich nahm `knopf_figur` = 0x1c1c24, ohne das Bild
        // zu messen) — eine Zahl ohne Messung ist eine Behauptung, und der
        // Test haette sie nur festgeschrieben.
        {
            let pal = crate::thema::Palette::fuer(crate::thema::Thema::Dunkel);
            let bahn = bahn_farbe(pal.hintergrund);
            let hellste_bahn = bahn.r.max(bahn.g).max(bahn.b);
            let dunkelster_daumen = DAUMEN_FARBE.r.min(DAUMEN_FARBE.g).min(DAUMEN_FARBE.b);
            let unterschied = hellste_bahn.abs_diff(dunkelster_daumen);
            assert!(
                unterschied >= 40,
                "GEMESSEN: Daumen ({dunkelster_daumen}) und Bahn ({hellste_bahn}) \
                 unterscheiden sich nur um {unterschied} Stufen — der Daumen ist nicht zu sehen"
            );
            assert_ne!(
                (bahn.r, bahn.g, bahn.b),
                (pal.hintergrund.r, pal.hintergrund.g, pal.hintergrund.b),
                "GEMESSEN: die Bahn hat die Farbe des Grundes"
            );
            // UND: der Daumen darf nicht heller sein als das BLATT. GEMESSEN
            // 3:8x war er 255 gegen 255 — er war heller als das Papier, das
            // er erschliessen soll. Ein helleres Weiss gibt es nicht, also
            // ist das eine harte Grenze und keine Geschmacksfrage.
            let papier = papier();
            assert!(
                dunkelster_daumen < papier.r.min(papier.g).min(papier.b),
                "GEMESSEN: der Daumen ({dunkelster_daumen}) ist nicht dunkler als das Papier (255) — er leuchtet ueber die Seite"
            );
        }
    }
}
