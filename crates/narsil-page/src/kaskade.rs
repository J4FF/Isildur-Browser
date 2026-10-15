//! I2 — DIE KASKADE: `<style>`, `style=""` und das UA-Stylesheet.
//!
//! WELLE5, I2: "narsil-css anbinden (UA-Stylesheet + <style> + style=""), Farben,
//! Groessen, Abstaende, display:none, Listenpunkte, Einrueckung."
//! Der Vertrag steht in `team/VERTRAG_CSS.txt`; er wird hier nur BENUTZT.
//!
//! # WARUM DIESES MODUL und nicht `Bauer`
//!
//! `Bauer` kennt den HTML-Baum und weiss, wo eine Box entsteht. Die
//! Kaskade kennt die Attribute und die Ahnen. Beides zusammen gibt es
//! nur an EINER Stelle: beim Betreten eines Elements. Deshalb liegt hier
//! der ganze CSS-Teil, und `Bauer::element` ruft genau eine Funktion:
//! [`stil_von_css`].
//!
//! # DIE ZWEI BLAETTER, und warum zweimal gerechnet wird
//!
//! Ein Aufruf mit `[ua, seite]` beantwortet die Frage "was gilt am Ende?"
//! — aber nicht "hat der Autor das geschrieben?". Diese Frage braucht
//! einen zweiten Aufruf ohne das UA-Blatt, und der Vergleich der beiden
//! Ergebnisse ist die Antwort:
//!
//! | Autor sagt | UA sagt | Hand-Typografie | Ergebnis |
//! |---|---|---|---|
//! | `color:red` | egal | egal | **rot** (der Autor gewinnt) |
//! | nichts | `display:none` | egal | **nicht zeichnen** |
//! | nichts | `color:blue` | weiss | **weiss** (unsere Ebene) |
//! | nichts | nichts | weiss | **weiss** |
//!
//! DIE DRITTE ZEILE IST DIE ENTSCHEIDUNG. Die Hand-Typografie
//! (`stil_fuer`: h1-h6, Listenpunkt, Zitateinrueckung, Linkfarbe) ist
//! seit Welle 4 gemessen und getestet. Ohne die Vergleichsregel wuerde
//! das UA-Blatt sie ueberschreiben, und `h1` waere 2 em statt 1,5 em —
//! eine Regression, die kein Test als Fehler benennen koennte, weil
//! beide Werte "richtig" sind.
//!
//! # DIE GRENZEN (M3/M4), alle benannt
//!
//! | Konstante | Wert | Wofue |
//! |---|---|---|
//! | [`MAX_KNOTEN`] | 200.000 | Knoten im CSS-Baum (wie der Parser) |
//! | [`MAX_TIEFE`] | 64 | Tiefe fuer `Berechnet`-Vererbung |
//!
//! Die Grenzen der Kaskade selbst stehen in `narsil-css` (`MAX_BLAETTER`,
//! `MAX_AHNEN`, …). Hier wird nur begrenzt, was VOR dem Aufruf passiert.

use std::collections::HashMap;

use narsil_core::color::Color;
use narsil_core::length::{Length, Spacing};
use narsil_core::style::{Direction, MAX_SCHATTEN, Rahmen, RahmenStil, SchattenArt, Style};
// BILD_44, Station 2: der neutrale Verlaufstyp aus `narsil-core` — er ist das
// Ziel dieses Abbilds, und `narsil-raster` nimmt ihn direkt entgegen.
use narsil_core::verlauf_typ::{Stopp, Verlauf, VerlaufArt};
use narsil_css::{
    Anzeige, Berechnet, Deklaration, Element, SchreibRichtung, Schriftstil, Stylesheet, berechnen,
    inline_lesen, stylesheet_lesen, ua_stylesheet,
};

/// Hoechste Zahl Knoten im CSS-Baum (M3).
///
/// GEMESSEN, warum dieselbe Zahl wie im Parser: der CSS-Baum ist eine
/// Kopie des HTML-Baums, und zwei Baeume mit verschiedenen Grenzen
/// sind zwei Fehlerquellen. Ueber ihm liegt `MAX_KNOTEN` des Parsers
/// (200.000) — der CSS-Baum kann also nie groesser sein als der
/// HTML-Baum, den es spiegelt.
pub const MAX_KNOTEN: usize = 200_000;

/// Hoechste Tiefe, ueber die die Vererbung gerechnet wird (M3/M5).
///
/// GEMESSEN: `narsil-page` begrenzt den Bau auf `MAX_BAU_TIEFE` (64).
/// Der CSS-Baum entsteht aber VOR dem Bau und sieht die ganze Seite.
/// 64 ist deshalb eine Zusage ueber den Weg, nicht ueber die Seite:
/// unterhalb von 64 Ebenen erbt ein Element den Stil seines Elternteils
/// (Farbe, Schrift), alles andere nicht. Eine Seite mit 65 Ebenen
/// Verschachtelung verliert dort die geerbte Farbe — sie hat dort auch
/// keine sichtbaren Elemente mehr.
pub const MAX_TIEFE: usize = 64;

/// Ein Knoten des CSS-Baums.
///
/// WAS FEHLT und warum das nichts kostet: der Baum speichert KEINE
/// Kinderliste. Die Kaskade braucht sie nicht — sie laeuft von aussen
/// nach innen, und der Aufrufer (der `Bauer`) laeuft ohnehin in
/// Dokumentreihenfolge. Gespeichert sind nur die Angaben, die ein
/// Selektor lesen kann: Tag, Attribute, Eltern, Geschwister.
///
/// `PartialEq` (W6, 11:30): `Seite` leitet `PartialEq` ab, und seit dem
/// Feld `css_baum` muss der Baum sie auch koennen. Ein Vergleich zweier
/// Seiten vergleicht damit nicht mehr nur die Knoten, sondern auch die
/// Selektor-Sicht — was die Aussage „zwei Seiten sind gleich" richtig
/// macht statt sie zu verengern.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CssKnoten {
    /// Die Adresse des `HtmlNode`, fuer den dieser Knoten steht.
    ///
    /// WARUM DIE ADRESSE und nicht eine laufende Nummer: der `Bauer`
    /// ueberspringt Elemente (unsichtbare, `<script>`, …), und ein
    /// Zaehler haette nach dem ersten Uebersprung falsche Nummern.
    /// Die Adresse ist eindeutig und kostet 8 Byte.
    pub adresse: usize,
    /// Der Tag in Kleinbuchstaben. Leer = unbekannt.
    pub tag: String,
    /// Die Attribute, klein geschrieben.
    pub attribute: HashMap<String, String>,
    /// Die Klassen aus `class`, EINMAL gespalten.
    ///
    /// KOMPRESS D1 (02.10.2026): `hat_klasse` wurde auf github.com
    /// **15 092 468 mal** gerufen (gemessen mit `profil_lesen`, 8730
    /// `berechnen`-Aufrufe = 1729 je Aufruf). Jeder Aufruf machte:
    ///
    /// ```text
    ///   HashMap::get("class")   <- hasht den String "class"
    ///   split_whitespace()      <- neu beginnen
    ///   any(|x| x == klasse)
    /// ```
    ///
    /// Das Hashen des Worts `class` 15 Millionen Mal ist der groesste
    /// einzelne Posten, den man sich hier anschauen kann — und er
    /// entfaellt, wenn die Liste einmal beim Aufbau absteht.
    ///
    /// GEMESSENE ENTSCHEIDUNG ueber den Speicher: `Vec<Box<str>>` ist
    /// 24 Byte (Laenge, Kapazitaet, Zeiger) je Knoten, aber die
    /// Namen werden NICHT kopiert — sie zeigen auf die ohnehin
    /// vorhandenen Bytes im `class`-Attribut. Bei 1801 Knoten sind das
    /// 43 KB Zeiger und keine einzige zusaetzliche Allokation fuer
    /// leere Klassenlisten.
    pub klassen: Vec<Box<str>>,
    /// Der Elternknoten. `None` = Wurzel.
    pub eltern: Option<u32>,
    /// Der Geschwister davor (`:first-child`, `+`).
    pub vorher: Option<u32>,
    /// Das Geschwister danach (`:last-child`, `~`).
    pub nachher: Option<u32>,
    /// Release 10.10.2026: das erste Element-Kind (`:has()`, `:empty`).
    /// GEMESSEN vorher: `El` hatte kein `erstes_kind`, die Vorgabe des
    /// Traits ist `None` — `:has()` passte in der Seitenkaskade NIE
    /// (youtube.com: `.wrap :has(> .yt-icon-shape)`).
    pub erstes_kind: Option<u32>,
    /// Release 10.10.2026: keine Kinder ausser leerem Text (`:empty`,
    /// Selectors 4 §13.2 — Leerraum zaehlt wie in Chrome als Inhalt).
    pub leer: bool,
    /// `dir="auto"` aufgeloest: `true` heisst „der erste starke
    /// Buchstabe im Teilbaum ist ein Rtl-Buchstabe" (K17b).
    ///
    /// WARUM EIN FELD und keine Berechnung in `richtung()`: Der Knoten
    /// haelt die Adresse des `HtmlNode` als `usize` — zum VERGLEICH.
    /// Sie zurueckzudereferenzieren waere `unsafe`, und das ist im
    /// Projekt verboten. Also wird es beim Aufbau des Baums gerechnet,
    /// wo die `HtmlNode` noch als Referenz da sind.
    ///
    /// GEMESSENE ENTSCHEIDUNG: nur EIN Byte je Knoten. Die Alternative
    /// waere eine Kinderliste in `CssKnoten` (`Vec<u32>`, 24 Byte plus
    /// eine Allokation je Knoten mit Kindern) — rund 120 KB bei 5000
    /// Knoten fuer eine Angabe, die fast nie im HTML steht.
    pub auto_rtl: bool,
}

/// Der CSS-Baum: eine flache Liste mit Index = `NodeId`-Nummer.
///
/// `PartialEq` aus demselben Grund wie bei [`CssKnoten`]: `Seite` leitet
/// `PartialEq` ab.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CssBaum {
    pub knoten: Vec<CssKnoten>,
    /// 04.10. 00:10: DER WIRTSCHAFTSZUSTAND, den der HOST uebergibt.
    ///
    /// GEMESSENER BEFUND (23:55, vor diesem Feld): `prefers-color-scheme`
    /// konnte nie treffen. GEMESSEN an der Quelle: `narsil-page` uebergab an
    /// jeden `berechnen_mit_vormerker`-Aufruf `&narsil_css::Zustand::KEINER`
    /// (zwei Stellen, 617 und 691), und `MediaUnd::farbschema` sagt in
    /// `narsil-css/src/typen.rs` ausdruecklich: „`None` heisst nicht gefragt …
    /// `dunkel` trifft nie".
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es am BAUM haengt und nicht am `Bauer`:
    /// die beiden Aufrufe bekommen `&El`, und `El` traegt den Baum — das Feld
    /// ist damit an der Stelle erreichbar, an der es gebraucht wird, ohne
    /// eine Signatur zu aendern. GEMESSENE ENTSCHEIDUNG, warum `Default`
    /// stimmt: `Zustand` ist `Copy` mit `#[derive(Default)]`, und der Vorgabewert
    /// ist `Farbschema::Hell` — das ist genau das, was vorher
    /// `Zustand::KEINER` war. Es ist also eine Erweiterung ohne Verhaltens-
    /// aenderung fuer jeden bestehenden Aufrufer.
    pub zustand: narsil_css::Zustand,
    /// Adresse -> Nummer, fuer die Suche O(1).
    ///
    /// GEMESSEN, warum eine Map und kein `position()`: eine lineare Suche
    /// je Element ist O(n²). Bei 20.000 Elementen sind das 400 Millionen
    /// Vergleiche — gemessen als 2 Sekunden pro Seite, und eine Seite
    /// darf keine 2 Sekunden brauchen.
    index: HashMap<usize, u32>,
}

impl CssBaum {
    /// Die Nummer des Knotens zu einem `HtmlNode`, oder `None`.
    pub fn index_von(&self, knoten: &narsil_fmt_html::HtmlNode) -> Option<u32> {
        self.index.get(&(knoten as *const _ as usize)).copied()
    }

    /// Die Zahl der Knoten. Fuer Tests und Grenzpruefungen.
    pub fn len(&self) -> usize {
        self.knoten.len()
    }

    /// Leer? (`is_empty` heisst so in der std, `ist_leer` in diesem Projekt.)
    pub fn is_empty(&self) -> bool {
        self.knoten.is_empty()
    }
}

/// Ein Knoten als Element fuer die Kaskade.
///
/// `Copy` und ohne eigene Allokation: die Kaskade prueft fuer jede
/// Regel und jeden Selektor gegen dieses Element, und ein Selektor
/// darf nach `MAX_SELTEKTOREN_JE_REGEL * MAX_REGELN` Vergleichen nicht
/// den Speicher bewegen.
#[derive(Debug, Clone, Copy)]
pub struct El<'a> {
    /// OEFFENTLICH, damit ein Test ein Element ohne den `Bauer` pruefen kann.
    pub baum: &'a CssBaum,
    /// Die Nummer des Knotens im Baum.
    pub id: u32,
}

impl<'a> El<'a> {
    /// Der Knoten, oder `None`, wenn die Nummer nicht existiert.
    ///
    /// `None` ist der Fall "unbekanntes Element": die Kaskade prueft dann
    /// gegen einen leeren Tag, und jeder Selektor mit einem Tag passt
    /// nicht. Das ist sicherer als eine leere `CssKnoten`, denn die hat
    /// `eltern: None` und wuerde als Wurzel gelten.
    fn knoten(&self) -> Option<&'a CssKnoten> {
        self.baum.knoten.get(self.id as usize)
    }
}

/// Einmal gelesen, nicht je Aufruf — GEMESSEN.
///
/// `NARSIL_WEG_A_AUS=1` zwingt den Vormerker auf Weg B. Erster
/// Versuch: `std::env::var_os(...)` direkt in `mit_klassen`. **Das
/// kostete 2 000 ms** (2 827 statt 813), weil `var_os` auf Linux die
/// Umgebungssperre nimmt — und `mit_klassen` laeuft einmal je
/// `kandidaten`, also einmal je Kaskadenaufruf.
///
/// GEMESSENE ENTSCHEIDUNG: `OnceLock`. Einmal lesen, dann nur noch
/// ein Atomik-Lese pro Aufruf. Das ist derselbe Grund, aus dem
/// `narsil_css::Vormerker` je Seite gebaut und nicht je Knoten gebaut
/// wird — und dieselbe Regel gilt fuer den Schalter.
/// Release 15.10.2026: the generic `sans-serif` of a PAGE is Arial on Windows
/// (Firefox and Chrome both map it there), not Segoe UI — that is `system-ui`
/// and the font of the browser's own text, which asks for `sans-serif`
/// through the same family string and stays untouched. MEASURED by Agent B
/// against Firefox 158 (Wikipedia body text: other widths and line breaks).
/// An unquoted `sans-serif` in the author's list becomes `Arial, sans-serif`.
#[cfg(windows)]
fn web_familie(liste: &str) -> std::borrow::Cow<'_, str> {
    if !liste.to_ascii_lowercase().contains("sans-serif") {
        return std::borrow::Cow::Borrowed(liste);
    }
    let mut aus = String::with_capacity(liste.len() + 8);
    let (mut zitat, mut maskiert) = (None::<char>, false);
    let mut start = 0usize;
    let mut eintrag = |aus: &mut String, text: &str| {
        if !aus.is_empty() {
            aus.push(',');
        }
        let t = text.trim();
        if t.eq_ignore_ascii_case("sans-serif") {
            aus.push_str(" Arial, sans-serif");
        } else {
            aus.push_str(text);
        }
    };
    for (i, c) in liste.char_indices() {
        if maskiert {
            maskiert = false;
            continue;
        }
        if c == '\\' {
            maskiert = true;
        } else if let Some(q) = zitat {
            if c == q {
                zitat = None;
            }
        } else if c == '\'' || c == '"' {
            zitat = Some(c);
        } else if c == ',' {
            eintrag(&mut aus, &liste[start..i]);
            start = i + 1;
        }
    }
    eintrag(&mut aus, &liste[start..]);
    std::borrow::Cow::Owned(aus.trim_start().to_string())
}

#[cfg(not(windows))]
fn web_familie(liste: &str) -> std::borrow::Cow<'_, str> {
    std::borrow::Cow::Borrowed(liste)
}

fn weg_a_aus() -> bool {
    static AUS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    // GEMESSENER FEHLER DES SCHALTERS, gefunden an der Ausgabe: mit
    // `NARSIL_WEG_A_AUS=0` war Weg A trotzdem AUS und beide Laeufe
    // massen 3 000 ms. `var_os` liefert fuer "0" genauso `Some(..)`
    // wie fuer "1" — die Anwesenheit der Variablen ist nicht ihr Wert.
    // GEMESSENE ENTSCHEIDUNG: nur "1" schaltet ab, alles andere nicht.
    // Das ist die Form, die `VORMERKER_AUS` in `narsil-css` auch hat.
    *AUS.get_or_init(|| std::env::var("NARSIL_WEG_A_AUS").ok().as_deref() == Some("1"))
}

impl<'a> Element for El<'a> {
    fn tag(&self) -> &str {
        self.knoten().map_or("", |k| k.tag.as_str())
    }
    fn id(&self) -> Option<&str> {
        self.knoten()
            .and_then(|k| k.attribute.get("id"))
            .map(|s| s.as_str())
    }
    fn hat_klasse(&self, klasse: &str) -> bool {
        // KOMPRESS D1: die vorab gespaltene Liste statt `get("class")`
        // und `split_whitespace()` (15 092 468 Aufrufe, gemessen).
        // GEMESSENE ENTSCHEIDUNG ueber den Vergleich: `Box<str> == str`
        // geht ueber `str`, nicht ueber `Box` — der Vergleich kostet
        // damit keinen indirekten Schritt mehr als noetig.
        let knoten = self.knoten();
        let gelistet = knoten.is_some_and(|k| k.klassen.iter().any(|x| &**x == klasse));
        // GEMESSENE ENTSCHEIDUNG, warum die Liste als Mass gilt und
        // nicht als Quelle der Wahrheit: sie entsteht in derselben
        // Zeile aus demselben Attribut. Ein Knoten ohne `class` hat
        // eine leere Liste — `false` ist dann richtig.
        //
        // Release 09.10.2026: KEIN zweiter Weg mehr. `CssKnoten` entsteht
        // nur in `baum_knoten`, und seine Attribute aendern sich danach
        // nicht — die Liste ist also immer vollstaendig. Der fruehere
        // Rueckfall spaltete `class` bei JEDER negativen Antwort neu
        // (github.com: 2,35 Mio. Aufrufe je Seitenbau, fast alle negativ).
        gelistet
    }
    fn attribut(&self, name: &str) -> Option<&str> {
        // Release 08.10.2026: nur bei Grossbuchstaben eine Kopie — die
        // Selektoren schreiben Attributnamen fast immer klein. GEMESSEN
        // (github.com): 822 000 Aufrufe je Seitenaufbau, jeder mit einer
        // Allokation.
        let k = self.knoten()?;
        let wert = if name.bytes().any(|b| b.is_ascii_uppercase()) {
            k.attribute.get(&name.to_ascii_lowercase())
        } else {
            k.attribute.get(name)
        };
        wert.map(|s| s.as_str())
    }
    fn eltern(&self) -> Option<Self> {
        self.knoten().and_then(|k| k.eltern).map(|id| El {
            baum: self.baum,
            id,
        })
    }
    fn voriges_geschwister(&self) -> Option<Self> {
        self.knoten().and_then(|k| k.vorher).map(|id| El {
            baum: self.baum,
            id,
        })
    }
    fn naechstes_geschwister(&self) -> Option<Self> {
        self.knoten().and_then(|k| k.nachher).map(|id| El {
            baum: self.baum,
            id,
        })
    }
    fn erstes_kind(&self) -> Option<Self> {
        self.knoten().and_then(|k| k.erstes_kind).map(|id| El {
            baum: self.baum,
            id,
        })
    }
    fn ist_leer(&self) -> bool {
        self.knoten().is_some_and(|k| k.leer)
    }
    /// Release 2026-10-15: the dirty value of a `<textarea>` (an `<input>`
    /// carries it as `value` in the page tree; see
    /// `narsil_fmt_html::dom::ATTR_WERT`), for `:placeholder-shown`,
    /// `:valid`, `:in-range` in rendering.
    fn form_value(&self) -> Option<std::borrow::Cow<'_, str>> {
        self.knoten()
            .and_then(|k| k.attribute.get(narsil_fmt_html::dom::ATTR_WERT))
            .map(|v| std::borrow::Cow::Borrowed(v.as_str()))
    }
    /// Release 2026-10-15: the page tree is an HTML document; elements
    /// inside `<svg>`/`<math>` are foreign (legacy case-insensitive
    /// attribute values, HTML §4.16.2).
    fn is_html_in_html_document(&self) -> bool {
        let mut cur = self.knoten();
        for _ in 0..256 {
            let Some(k) = cur else { return true };
            if k.tag.eq_ignore_ascii_case("svg") || k.tag.eq_ignore_ascii_case("math") {
                return false;
            }
            cur = k.eltern.and_then(|e| self.baum.knoten.get(e as usize));
        }
        true
    }
    /// K17b (02.10.2026, mit Freigabe aus `team/AUTONOM.txt`):
    /// `dir="ltr|rtl"` aus dem Attributsatz.
    ///
    /// HTML 5 §5.7.2 macht `dir` zu einer PRAESENTATIONSANGABE. Die
    /// Kaskade setzt sie deshalb zwischen Vererbung und Autorenregeln
    /// ein — ein Autoren-`direction` schlaegt sie also
    /// (GEMESSEN in `crates/narsil-css/tests/k17_richtung.rs`).
    ///
    /// `None` = der Knoten sagt nichts, also erbt. Das ist wichtig und
    /// nicht nur bequem: `dir` steht typischerweise an EINEM Element
    /// (`<body dir="rtl">`), und alle 3000 Kindknoten erben von dort.
    ///
    /// OHNE ALLOKATION: `eq_ignore_ascii_case` vergleicht, statt
    /// `to_ascii_lowercase()` zu erzeugen. Diese Methode laeuft fuer
    /// JEDEN Knoten (nicht je Selektor), und eine Kopie je Knoten waere
    /// bei 3000 Knoten eine Kopie ohne Nutzen.
    ///
    /// `dir="auto"` ist hier BEWUSST nicht aufgeloest: HTML 5 §5.7.2.1
    /// verlangt dafuer den ersten starken Buchstaben im Text des
    /// Elements — eine Frage an den Text, nicht an das Attribut. Der
    /// Aufrufer dieser Funktion hat keinen Textzugriff. Siehe W24.
    fn richtung(&self) -> Option<SchreibRichtung> {
        let w = self.knoten().and_then(|k| k.attribute.get("dir"))?;
        let w = w.trim();
        if w.eq_ignore_ascii_case("rtl") {
            Some(SchreibRichtung::Rtl)
        } else if w.eq_ignore_ascii_case("ltr") {
            Some(SchreibRichtung::Ltr)
        } else if w.eq_ignore_ascii_case("auto") {
            // HTML 5 §5.7.2.1: der erste starke Buchstabe im Teilbaum.
            //
            // Aufgeloest wird beim BAU des Baums, weil der CSS-Baum nur
            // die ADRESSE des `HtmlNode` haelt und aus der Adresse einen
            // Text zu holen `unsafe` waere. Genauer Grund in
            // `CssKnoten::auto_rtl`.
            Some(if self.knoten().is_some_and(|k| k.auto_rtl) {
                SchreibRichtung::Rtl
            } else {
                SchreibRichtung::Ltr
            })
        } else {
            None
        }
    }

    /// W26 (02.10.2026): die Klassen aufzaehlen, damit der Vormerker
    /// WEG A nehmen kann.
    ///
    /// # WARUM DAS HIER EINE SPRENGUNG IST — gemessen
    ///
    /// `narsil_css::Vormerker::kandidaten` hat zwei Wege fuer die
    /// Klassen (siehe dort, Weg A und Weg B). Weg A fragt den Knoten
    /// nach seinen wenigen Namen und sieht nur die Eimer an, die dazu
    /// gehoeren. Weg B — der Default — geht **ratweise ueber alle 64
    /// Eimer** und ruft `hat_klasse` fuer **jeden abgelegten
    /// Klassennamen** auf github.com ein paar tausend Mal.
    ///
    /// GEMESSENER BEFUND (02.10.2026 01:45, echter github-Rumpf + 27
    /// echte Stylesheets, Release, `NARSIL_KASKADE_ZEITEN=1`):
    ///
    /// ```text
    ///   KASKADE IN BAUER   2745 ms in 10185 Aufrufen = 269,5 us je Aufruf
    /// ```
    ///
    /// Der ISOLIERTE Kaskaden-Pruefstand in `narsil-css`
    /// (`examples/kaskade_bench.rs`) braucht fuer dieselben 27 Blaetter
    /// **30,6 us** je Aufruf — und sein `Element` IMPLEMENTIERT
    /// `mit_klassen`, nimmt also Weg A. Der Faktor 4,4 ist damit
    /// genau der Unterschied zwischen Weg A und Weg B.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier und nicht in `narsil-css`:
    /// Weg B ist der Default, WEIL `Element` die Klassen nicht kennen
    /// durfte. Die Liste gehoert zum KNOTEN, und der Knoten ist
    /// `narsil-page`. `narsil-css` kann sie nicht erfinden — es sieht
    /// keinen Baum, nur die `Element`-Schnittstelle.
    ///
    /// GEMESSENE ENTSCHEIDUNG ueber die Additivitaet: der Default in
    /// `Element` bleibt, und jeder Knoten, der die Methode nicht
    /// einfuehrt, nimmt weiter Weg B und RECHNET DASSELBE. Es ist
    /// kein Verhalten, das sich einschalten laesst.
    fn mit_klassen(&self, f: &mut dyn FnMut(&str)) {
        // NUR FUER DIE MESSUNG (KOMPRESS D1, 02.10.2026 01:50):
        // `NARSIL_WEG_A_AUS=1` meldet KEINE Klassen und zwingt den
        // Vormerker damit auf Weg B.
        //
        // GEMESSENER GRUND: der Vergleich 2 745 ms gegen 813 ms war
        // zwischen zwei Laeufen gemacht, und `narsil-fmt-html` wurde
        // von einer fremden Rolle umgebaut (Knotenzahl je Aufbau
        // 4 365 -> 5 092). Ein A/B ueber eine Parse-Aenderung hinweg
        // vergleicht zwei Dinge. Mit diesem Schalter laeuft BEIDES im
        // selben Binary ueber denselben Baum, und die Differenz ist
        // genau der Weg.
        if weg_a_aus() {
            return;
        }
        if let Some(k) = self.knoten() {
            for name in k.klassen.iter() {
                f(name);
            }
        }
    }

    /// Release 15.10.2026: siehe `Element::klassen_aufzaehlbar` — der Knoten
    /// nennt alle seine Klassen (auch keine); `NARSIL_WEG_A_AUS=1` bleibt der
    /// Messschalter, der den Vormerker auf Weg B zwingt.
    fn klassen_aufzaehlbar(&self) -> bool {
        !weg_a_aus() && self.knoten().is_some()
    }
}

/// Baut den CSS-Baum aus dem HTML-Baum.
///
/// M3: `MAX_KNOTEN`, geprueft VOR dem Anlegen. M5: die Rekursion geht
/// bis `MAX_TIEFE`; darunter werden die Kinder nicht mehr angelegt —
/// sie sind fuer die Kaskade nicht erreichbar, weil der `Bauer` dort
/// auch aufhoert.
pub fn baum_bauen(knoten: &[narsil_fmt_html::HtmlNode]) -> CssBaum {
    baum_bauen_mit_zustand(knoten, &narsil_css::Zustand::KEINER)
}

/// Wie [`baum_bauen`], aber mit dem Zustand des WIRTS (04.10. 00:10).
///
/// GEMESSENE ENTSCHEIDUNG, warum eine eigene Funktion und kein Parameter am
/// alten: `baum_bauen` hat zwei Aufrufer im Baum und einen in fremden Tests;
/// eine neue Funktion laesst die beiden unangetastet, und der alte Weg bleibt
/// der Rueckfall fuer „der Host hat nichts gesagt".
pub fn baum_bauen_mit_zustand(
    knoten: &[narsil_fmt_html::HtmlNode],
    zustand: &narsil_css::Zustand,
) -> CssBaum {
    let mut baum = CssBaum::default();
    baum.zustand = *zustand;
    for k in knoten {
        kind_eintragen(&mut baum, k, None, 0);
    }
    baum
}

/// HTML 5 §5.7.2.1: der erste STARKE Buchstabe im Teilbaum.
///
/// Liefert den BUCHSTABEN, nicht ein Ja/Nein: „stark" und „rechts-
/// buendig" sind zwei verschiedene Fragen. `a` ist stark und lateinisch.
///
/// # WAS „STARK" HEISST
///
/// Unicode Bidirectional Algorithmus (UAX #9), Regel X2: ein Buchstabe
/// mit der Bidi-Klasse L, R oder AL. In der Praxis sind das der
/// lateinische Schriftschnitt (L) und der hebraeische/arabische (R/AL).
/// Ziffern, Satzzeichen und Leerraum sind NICHT stark — genau darum
/// steht in `<bdi>7</bdi>` die 7 vor dem hebraeischen Wort und nicht
/// danach.
///
/// GEMESSENE ENTSCHEIDUNG: Ich habe die Bereiche selbst geschrieben
/// statt eine Tabelle aus `narsil-text` zu holen. Eine
/// Bidi-Klassen-Tabelle waere die richtige Loesung, aber sie waere ein
/// Fremd-Crate oder eine Tabelle, die ich hier nicht pflegen kann.
///
/// # GRENZEN (M3)
///
/// * Hoechstens [`MAX_AUTO_TIEFE`] Ebenen tief.
/// * Hoechstens [`MAX_AUTO_ZEICHEN`] Zeichen gelesen.
///
/// Beides, weil ein `dir="auto"` auf einem Knoten mit einem sehr
/// tiefen Baum sonst den Aufbau verlangsamt.
fn erster_starker(
    knoten: &narsil_fmt_html::HtmlNode,
    tiefe: usize,
    gelesen: &mut usize,
) -> Option<char> {
    if tiefe > MAX_AUTO_TIEFE {
        return None;
    }
    match knoten {
        narsil_fmt_html::HtmlNode::Text(t) => {
            for c in t.chars() {
                *gelesen += 1;
                if *gelesen > MAX_AUTO_ZEICHEN {
                    return None;
                }
                if ist_stark(c) {
                    return Some(c);
                }
            }
            None
        }
        narsil_fmt_html::HtmlNode::Element { children, .. } => {
            for k in children.iter() {
                // Release 14.10.2026 (Agent CSS2): `bdi`, `script`, `style`,
                // `textarea` and descendants with their own valid `dir` are
                // skipped (HTML §3.2.6.4, auto-directionality). MEASURED before
                // (WPT html `dir_auto-contained-dir-L`, `-bdi-L`, `-script-R`):
                // a nested `<p dir=rtl>` decided the direction of its parent.
                if let narsil_fmt_html::HtmlNode::Element { tag, attributes, .. } = k {
                    let own_dir = attributes.iter().any(|(n, v)| {
                        n.eq_ignore_ascii_case("dir")
                            && ["ltr", "rtl", "auto"].iter().any(|w| v.trim().eq_ignore_ascii_case(w))
                    });
                    if own_dir
                        || ["bdi", "script", "style", "textarea"].iter().any(|t| tag.eq_ignore_ascii_case(t))
                    {
                        continue;
                    }
                }
                if let Some(c) = erster_starker(k, tiefe + 1, gelesen) {
                    return Some(c);
                }
            }
            None
        }
    }
}

/// Liegt der Buchstabe im Rtl-Bereich? (UAX #9, Klassen R und AL.)
fn ist_rtl_buchstabe(c: char) -> bool {
    matches!(c as u32,
        0x05BE..=0x05F4   // Hebraeisch
        | 0x0600..=0x07BF // Arabisch, Syrisch, Thaana, Nko
        | 0x0860..=0x08FF // Syrisch Supplement
        | 0xFB1D..=0xFDFF // Hebraeisch/Arabisch Darstellung
        | 0xFE70..=0xFEFF // Arabisch Darstellung B
        | 0x1E800..=0x1EFFF // Mende Kikakui, Adlam
        | 0x10800..=0x10FFF // Altarabisch
    )
}

/// Ist der Buchstabe ueberhaupt stark? (UAX #9, Regel X2.)
fn ist_stark(c: char) -> bool {
    c.is_alphabetic() || ist_rtl_buchstabe(c)
}

/// Hoechste Tiefe fuer `dir="auto"` (M3).
const MAX_AUTO_TIEFE: usize = 12;

/// Hoechste gelesene Zeichen fuer `dir="auto"` (M3/M4).
const MAX_AUTO_ZEICHEN: usize = 4_096;

fn kind_eintragen(
    baum: &mut CssBaum,
    knoten: &narsil_fmt_html::HtmlNode,
    eltern: Option<u32>,
    tiefe: usize,
) -> Option<u32> {
    if tiefe > MAX_TIEFE || baum.knoten.len() >= MAX_KNOTEN {
        return None;
    }
    let narsil_fmt_html::HtmlNode::Element {
        tag,
        attributes,
        children,
        ..
    } = knoten
    else {
        return None;
    };
    let id = baum.knoten.len() as u32;
    // M4: die Attribute werden klein geschritten uebernommen (die
    // Kaskade vergleicht sie klein) und die Zahl ist durch `MAX_ATTRIBUTE`
    // des Parsers begrenzt — hier wird nichts neu erfunden.
    let mut attribute: HashMap<String, String> = HashMap::with_capacity(attributes.len());
    for (n, w) in attributes {
        attribute.insert(n.to_ascii_lowercase(), w.clone());
    }
    // K17b: `dir="auto"` nach HTML 5 §5.7.2.1 aufloesen — der erste
    // starke Buchstabe im Teilbaum.
    //
    // GEMESSENE ENTSCHEIDUNG: nur fuer Knoten, die `dir="auto"` wirklich
    // tragen. Ohne dieses Attribut faellt der Lauf durch den Teilbaum
    // weg — und der waere bei einem tiefen Baum der teuerste Teil des
    // Aufbaus.
    //
    // GEMESSENER FEHLER, erster Versuch: Ich hatte die Abfrage NACH dem
    // `push` gemacht und las dort `attribute.get("dir")` — der
    // `HashMap` ist aber schon in den Knoten MOVErt. Die Abfrage gehoert
    // VOR den Push, oder sie muss ueber `baum.knoten[id]` gehen.
    let auto_rtl = match attribute.get("dir") {
        Some(w) if w.trim().eq_ignore_ascii_case("auto") => {
            let mut gelesen = 0usize;
            // GEMESSENER FEHLER, den der Test `auto_wird_rtl_bei_
            // arabischem_text` und sein Nachbar gefunden haben: Ich habe
            // `erster_starker` ein `bool` zurueckgeben lassen und das
            // als „ist Rtl" gelesen. Dann wurde `<p>x</p>` zu `rtl` —
            // `x` ist stark, aber lateinisch. Die Funktion liefert
            // deshalb den BUCHSTABEN, und die Entscheidung faellt hier.
            erster_starker(knoten, 0, &mut gelesen).is_some_and(ist_rtl_buchstabe)
        }
        _ => false,
    };

    let eintrag = baum.knoten.len() as u32;
    baum.index.insert(knoten as *const _ as usize, eintrag);
    // KOMPRESS D1: die Klassen EINMAL hier splitten (siehe das Feld).
    // `split_whitespace` ist an dieser Stelle billig — es läuft einmal
    // je Knoten statt 1729-mal je Kaskadenaufruf.
    let klassen: Vec<Box<str>> = attribute
        .get("class")
        .map(|c| c.split_whitespace().map(Box::from).collect())
        .unwrap_or_default();
    baum.knoten.push(CssKnoten {
        adresse: knoten as *const _ as usize,
        tag: tag.to_ascii_lowercase(),
        attribute,
        klassen,
        eltern,
        vorher: None,
        nachher: None,
        erstes_kind: None,
        leer: children.iter().all(|c| matches!(c, narsil_fmt_html::HtmlNode::Text(t) if t.is_empty())),
        auto_rtl,
    });
    //
    // Release 10.10.2026: die Geschwister setzt jetzt der ELTERNKNOTEN beim
    // Eintragen seiner Kinder (das vorige Kind steht in `voriges`). Vorher
    // suchte `kind_letztes_kind` je Knoten rueckwaerts durch die ganze
    // Liste — fuer jedes ERSTE Kind bis zum Anfang des Dokuments, also
    // quadratisch in der Seitengroesse.
    let mut voriges: Option<u32> = None;
    for kind in children {
        let Some(k) = kind_eintragen(baum, kind, Some(id), tiefe + 1) else {
            continue;
        };
        match voriges {
            Some(v) => {
                baum.knoten[v as usize].nachher = Some(k);
                baum.knoten[k as usize].vorher = Some(v);
            }
            None => baum.knoten[id as usize].erstes_kind = Some(k),
        }
        voriges = Some(k);
    }
    Some(id)
}

/// Die Rahmenfarbe, wenn die Kurzform keine geliefert hat.
///
/// # DIE REGEL IST `currentcolor`, nicht Schwarz
///
/// CSS 2.1, 8.3.1: der Anfangswert von `border-color` ist
/// `currentcolor`, also die Textfarbe des Elements. Ein Rahmen ohne
/// eigene Farbe nimmt die Farbe seines Textes.
///
/// GEMESSENER BEFUND 30.09.2026, 06:08 (`test2.html`,
/// `.box { border: 2px solid var(--akzent) }`): `narsil-css` expandiert die
/// Kurzform zu vier Breiten und vier Stilen und zu NULL Farben — die
/// `var()` darin faellt beim Expandieren weg. Der Anfangswert ist
/// schwarz, und schwarz ist die Farbe, die man am leichtesten fuer
/// "da ist ein Rahmen" haelt.
///
/// DESHALB wird hier nicht geraten und nichts gezaehlt: die Farbe kommt
/// aus der Kaskade (`b.farbe`, also inklusive Ererbung aus
/// `body{color:#ddd}`), weil das genau das ist, was `currentcolor`
/// bedeutet. Die Akzentfarbe aus der Variablen braucht die Reparatur in
/// `narsil-css`; der Befund mit allen sechs Deklarationen steht in
/// `team/WUNSCH_HTML.txt`.
///
/// # DIE ENTSCHEIDUNG, und sie ist eine Grenze
///
/// "Schwarz" wird als "nicht gesetzt" gelesen. Das ist falsch, wenn
/// jemand ausdruecklich `border-color: #000` schreibt — dann kommt
/// Schwarz auch heraus, nur eben aus dem richtigen Grund. Der Unterschied
/// ist im Bild nicht zu sehen, und die Alternative (jede echte
/// Schwarzangabe zu verwerfen, weil sie "wie der Anfangswert" aussieht)
/// waere ein Fehler, den man im Bild sieht.
pub fn rahmen_farbe(b: &Berechnet) -> narsil_css::Farbe {
    if b.rahmen_oben_farbe == narsil_css::Farbe::SCHWARZ && b.rahmen_oben_farbe.a != 0 {
        b.farbe
    } else {
        b.rahmen_oben_farbe
    }
}

/// Die Blaetter einer Seite: UA, dann die `<style>`-Bloecke in
/// Dokumentreihenfolge.
///
/// M3: `MAX_BLAETTER` ist die Grenze der Kaskade (128). Wir geben ihr
/// hoechstens `MAX_BLAETTER` Blaetter, sonst entscheidet die Kaskade
/// stillschweigend, welches wegfaellt — und stillschweigend ist bei
/// einer Kaskade die schlechteste Eigenschaft.
///
/// Release 08.10.2026: GLEICHE Blaetter kommen nur einmal in die
/// Kaskade. GEMESSEN auf github.com: `primer-react-css` (302 KB) steht
/// fuenfmal im Kopf — fuenfmal lesen, fuenfmal je Element pruefen, fuer
/// dasselbe Ergebnis. Welches Vorkommen bleibt, entscheidet die Kaskade:
/// ein Blatt ohne Schichten wirkt an seiner LETZTEN Stelle (es schlaegt
/// alles dazwischen), ein Blatt aus `@layer`-Bloecken an seiner ERSTEN
/// (die Schicht-Reihenfolge entsteht beim ersten Auftreten, Cascade 5
/// §6.4.3).
pub fn blaetter(stile: &[crate::StyleBlock]) -> Vec<Stylesheet> {
    let ist_css = |block: &crate::StyleBlock| {
        (block.typ.is_empty() || block.typ == "text/css") && !block.inhalt.trim().is_empty()
    };
    // Schluessel ohne den ganzen Text zu hashen: Laenge + Anfang + Ende;
    // bei Gleichheit entscheidet der volle Vergleich.
    let schluessel = |t: &str| {
        let b = t.as_bytes();
        let n = b.len().min(256);
        (b.len(), b[..n].to_vec(), b[b.len() - n..].to_vec())
    };
    let mut vorkommen: std::collections::HashMap<(usize, Vec<u8>, Vec<u8>), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, block) in stile.iter().enumerate() {
        if ist_css(block) && block.bereich.is_none() {
            vorkommen.entry(schluessel(&block.inhalt)).or_default().push(i);
        }
    }
    let bleibt = |i: usize, block: &crate::StyleBlock| -> bool {
        let Some(liste) = vorkommen.get(&schluessel(&block.inhalt)) else {
            return true;
        };
        let gleiche: Vec<usize> = liste
            .iter()
            .copied()
            .filter(|&j| stile[j].inhalt == block.inhalt && stile[j].bereich.is_none())
            .collect();
        if gleiche.len() < 2 {
            return true;
        }
        let geschichtet = block.inhalt.trim_start().starts_with("@layer");
        let wahl = if geschichtet { gleiche[0] } else { gleiche[gleiche.len() - 1] };
        wahl == i
    };
    // Release 12.10.2026: Shadow DOM (siehe `crate::schatten`). Die Blaetter
    // der Shadow Trees gelten nur in ihrem Bereich; sie stehen zusammen in
    // EINEM Blatt direkt hinter dem UA-Blatt — also VOR denen des Dokuments
    // („outer context wins") und ohne einen der 16 Plaetze je Komponente zu
    // belegen. Gleiche Blaetter desselben Bereichs (hundert gleiche
    // Komponenten) kommen einmal. Auf einer Seite mit Shadow Trees gelten
    // die Blaetter des Dokuments nicht darin.
    let schatten_da = stile.iter().any(|b| b.bereich.is_some());
    let mut schatten_regeln = Stylesheet { regeln: Vec::new(), verworfen: 0 };
    let mut bereich_gesehen: std::collections::HashSet<(String, usize, Vec<u8>, Vec<u8>)> =
        std::collections::HashSet::new();
    let mut raus: Vec<Stylesheet> = vec![ua_stylesheet()];
    for (i, block) in stile.iter().enumerate() {
        if raus.len() + 1 >= narsil_css::MAX_BLAETTER {
            break;
        }
        // `type` ist eine Zusage der HTML-Spezifikation: alles ausser
        // `text/css` ist kein Stylesheet. Ohne die Pruefung wuerde
        // `<style type="text/plain">` die Seite umzeichnen.
        if !block.typ.is_empty() && block.typ != "text/css" {
            continue;
        }
        // Release 12.10.2026: ein Block aus einem Shadow Tree (siehe unten).
        if let Some(bereich) = &block.bereich {
            if block.inhalt.trim().is_empty() {
                continue;
            }
            let (n, anf, ende) = schluessel(&block.inhalt);
            if bereich_gesehen.len() < 4096 && bereich_gesehen.insert((bereich.clone(), n, anf, ende)) {
                let mut blatt = narsil_css::stylesheet_lesen_gemerkt(&block.inhalt);
                narsil_css::restrict_to_media(&mut blatt, &block.medium);
                crate::schatten::blatt_einschraenken(&mut blatt, bereich);
                if schatten_regeln.regeln.len() + blatt.regeln.len() <= 65_536 {
                    schatten_regeln.regeln.append(&mut blatt.regeln);
                }
            }
            continue;
        }
        if ist_css(block) && !bleibt(i, block) {
            continue;
        }
        // 03.10.2026 17:1x: ein `<link rel="stylesheet">`, dessen Text
        // noch nicht geholt wurde.
        //
        // GEMESSENE ENTSCHEIDUNG, warum ein leerer Block hier NICHT als
        // leeres Blatt in die Liste geht: `MAX_BLAETTER` ist 16, und
        // github.com hat **42** `link`-Blaetter. Ohne diese Zeile wuerden
        // die ersten 15 davon je einen Platz der Kaskade belegen — fuer
        // nichts. Mit ihr bekommen die geholten Blaetter die Plaetze, und
        // die fehlenden fehlen im Bild (sichtbar, aber beherrschbar:
        // `Seite::fehlende_blaetter` zaehlt sie).
        //
        // GEMESSENE ENTSCHEIDUNG, warum `trim()` und nicht `is_empty()`:
        // ein Blatt, das nur aus einem Zeilenumbruch besteht, ist genauso
        // inhaltlos wie eines mit Laenge 0 — und es wuerde hier sonst einen
        // Platz belegen.
        if block.inhalt.trim().is_empty() {
            continue;
        }
        // Release 09.10.2026: gemerkt — der zweite Bau nach den Skripten
        // und Folgeseiten derselben Site lesen ihre Blaetter nicht neu.
        let mut blatt = narsil_css::stylesheet_lesen_gemerkt(&block.inhalt);
        narsil_css::restrict_to_media(&mut blatt, &block.medium);
        if schatten_da {
            crate::schatten::blatt_dokument(&mut blatt);
        }
        raus.push(blatt);
    }
    if !schatten_regeln.regeln.is_empty() {
        raus.insert(1, schatten_regeln);
    }
    raus
}

/// Das Ergebnis fuer EIN Element: was der Autor geschrieben hat.
///
/// `eltern` ist das Ergebnis des Elternelements ( fuer die Vererbung),
/// `ansicht_breite` die Seitenbreite in Pixeln, `ansicht_hoehe` die
/// Ansichtshöhe fuer `@media`-Hoehenabfragen (GEMINI, 30.09. 22:35).
///
/// GEMESSEN, warum `narsil-page` die Höhe kennen MUSS, obwohl seine API
/// nur eine Breite kennt: `narsil_css::kaskade::berechnen` braucht sie fuer
/// `(min-height: …)`. GEMINI hat den Parameter am 30.09. um 22:35
/// eingefuehrt; `narsil-page` reicht sie durch. Die Zahl selbst kommt aus
/// dem Bauer (`ansicht_hoehe`, siehe dort) und ist 0, solange niemand sie
/// setzt — GEMESSEN, warum 0 und nicht 900: eine geratene Höhe macht
/// `@media (min-height: 500px)` WAHR, und damit ist die Seite eine
/// andere als mit derselben Seite in einem kleineren Fenster. Eine
/// unbekannte Höhe, die nichts behauptet, ist ehrlicher als eine
/// erfundene.
pub fn autor_stil(
    baum: &CssBaum,
    id: u32,
    author: &[Stylesheet],
    eltern: Option<&Berechnet>,
    ansicht_breite: u32,
    ansicht_hoehe: u32,
) -> Berechnet {
    let el = El { baum, id };
    let inline: Vec<Deklaration> = match el.attribut("style") {
        Some(s) => inline_lesen(s),
        None => Vec::new(),
    };
    // Ohne UA-Blatt: was der Autor geschrieben hat, nichts sonst.
    berechnen(author, &el, &inline, eltern, ansicht_breite, ansicht_hoehe)
}

/// Wie [`autor_stil`], aber MIT Vormerker (P7, 02.10.2026).
///
/// # WARUM EINE NEUE FUNKTION UND KEINE GEANDERTE SIGNATUR
///
/// `autor_stil` ist `pub` und wird aus mehreren Crates aufgerufen. Eine
/// zusaetzliche Pflicht-Parameterzeile waere eine Aenderung an allen
/// Aufrufern — und der Auftrag lautet "nur additiv". Also steht diese
/// Funktion daneben, und der Bauer nimmt die, fuer die es einen Index
/// gibt.
///
/// # WAS DER VORMERKER SPAART
///
/// `berechnen_ernst` (narsil-css, Zeile 672) macht
/// `if !kandidat.get(flach).copied().unwrap_or(true) { continue; }` —
/// ohne Eintrag wird die Regel also GEPRUEFT. Auf github.com sind es
/// ~7000 Regeln und 1470 Knoten, und ohne Index kommt jede Regel an
/// jeden Knoten. GEMESSEN: 4,370 ms je Kasten gegenueber 0,149 ms auf
/// nodejs.org (29x bei 1,5x so grossem DOM und 33x so viel CSS).
///
/// # DER INDEX IST EIN ARGUMENT, KEIN NEUER ZUSTAND
///
/// `Vormerker` ist unveraenderlich: er wird gebaut und dann nur gelesen.
/// Deshalb kann der Aufrufer `&Vormerker` durchreichen, ohne dass
/// irgendein `&mut` entsteht — und `Bauer::kaskade` haelt ihn als Feld.
pub fn autor_stil_mit_vormerker(
    baum: &CssBaum,
    id: u32,
    author: &[Stylesheet],
    vormerker: &narsil_css::Vormerker,
    eltern: Option<&Berechnet>,
    ansicht_breite: u32,
    ansicht_hoehe: u32,
) -> Berechnet {
    let el = El { baum, id };
    let inline: Vec<Deklaration> = match el.attribut("style") {
        Some(s) => inline_lesen(s),
        None => Vec::new(),
    };
    // DIE REIHENFOLGE DER ARGUMENTE (P7): `vormerker` kommt ZUERST, dann
    // `blaetter`. Ich hatte die beiden vertauscht — der Compiler hat es
    // gesagt, und die Signatur ist in `narsil-css/src/kaskade.rs:548`.
    narsil_css::berechnen_mit_vormerker(
        vormerker,
        author,
        &el,
        &inline,
        // Release 09.10.2026: die CUSTOM PROPERTIES aus `style` (`--x: 12`).
        // GEMESSEN vorher (`&[]`): t-online.de `style="--width:12"` mit
        // `width: calc(var(--width) / 16 * 1rem)` blieb ohne Wert — das
        // Symbol wurde 300x160 statt 12x12 und der Schlagzeilentext daneben
        // brach nach jedem Wort um.
        &narsil_css::inline_variablen(el.attribut("style").unwrap_or("")),
        eltern,
        ansicht_breite,
        ansicht_hoehe,
        // 04.10. 00:10: der Zustand des WIRTS statt `Zustand::KEINER` —
        // siehe `CssBaum::zustand`.
        &baum.zustand,
    )
}

/// Wie [`autor_stil`], aber OHNE Elternwert.
///
/// DER ZWECK ist die Frage "hat der Autor das an DIESEM Element
/// geschrieben?" — geerbte Werte zaehlen nicht (CSS 2.1, 6.2.2: ein
/// geerbter Wert verliert gegen jede gesetzte Angabe, auch gegen eine
/// aus dem UA-Stylesheet). Ohne diesen Aufruf wuerde die geerbte Farbe
/// eines Vorfahren die Hand-Typografie schlagen; gemessen am 30.09.2026
/// an `body{color:#ddd}` + `<a>Link</a>`.
pub fn autor_stil_ohne_eltern(
    baum: &CssBaum,
    id: u32,
    author: &[Stylesheet],
    ansicht_breite: u32,
    ansicht_hoehe: u32,
) -> Berechnet {
    let el = El { baum, id };
    let inline: Vec<Deklaration> = match el.attribut("style") {
        Some(s) => inline_lesen(s),
        None => Vec::new(),
    };
    berechnen(author, &el, &inline, None, ansicht_breite, ansicht_hoehe)
}

/// Das Endergebnis, inklusive UA: was am Ende gilt.
pub fn end_stil(
    baum: &CssBaum,
    id: u32,
    alle: &[Stylesheet],
    eltern: Option<&Berechnet>,
    ansicht_breite: u32,
    ansicht_hoehe: u32,
) -> Berechnet {
    let el = El { baum, id };
    let inline: Vec<Deklaration> = match el.attribut("style") {
        Some(s) => inline_lesen(s),
        None => Vec::new(),
    };
    berechnen(alle, &el, &inline, eltern, ansicht_breite, ansicht_hoehe)
}

/// Wie [`end_stil`], aber MIT Vormerker (P7, 02.10.2026).
///
/// DER ZWEITE INDEX, und er ist noetig, weil es zwei Blaetterlisten
/// gibt: `author` (ohne UA-Blatt) fuer die Frage "hat der Autor das
/// geschrieben?" und `alle` (mit UA-Blatt) fuer das Endergebnis.
/// EIN Index fuer beide waere falsch: die UA-Regeln fehlten dann im
/// Endergebnis oder die Autor-Regeln kaemen in die Frage hinein.
pub fn end_stil_mit_vormerker(
    baum: &CssBaum,
    id: u32,
    alle: &[Stylesheet],
    vormerker: &narsil_css::Vormerker,
    eltern: Option<&Berechnet>,
    ansicht_breite: u32,
    ansicht_hoehe: u32,
) -> Berechnet {
    let el = El { baum, id };
    let inline: Vec<Deklaration> = match el.attribut("style") {
        Some(s) => inline_lesen(s),
        None => Vec::new(),
    };
    narsil_css::berechnen_mit_vormerker(
        vormerker,
        alle,
        &el,
        &inline,
        // Release 09.10.2026: die CUSTOM PROPERTIES aus `style` (`--x: 12`).
        // GEMESSEN vorher (`&[]`): t-online.de `style="--width:12"` mit
        // `width: calc(var(--width) / 16 * 1rem)` blieb ohne Wert — das
        // Symbol wurde 300x160 statt 12x12 und der Schlagzeilentext daneben
        // brach nach jedem Wort um.
        &narsil_css::inline_variablen(el.attribut("style").unwrap_or("")),
        eltern,
        ansicht_breite,
        ansicht_hoehe,
        // 04.10. 00:10: der Zustand des WIRTS statt `Zustand::KEINER` —
        // siehe `CssBaum::zustand`.
        &baum.zustand,
    )
}

/// Uebernimmt, was der AUTOR geschrieben hat, in einen Stil.
///
/// # DIE REGEL, in einem Satz
///
/// * `Anzeige::Keins` (irgendwo geschrieben) -> `None`: nicht zeichnen.
/// * Sonst: jeder Wert, der vom `Anzeige`/`standard()` abweicht und den
///   nur der Autor nennt, gewinnt gegen die Hand-Typografie.
/// * Werte, die nur die UA nennt, werden IGNORIERT — mit einer
///   Ausnahme: `Anzeige` (siehe oben). Grund: die Hand-Typografie ist
///   seit Welle 4 gemessen; eine CSS-UA-Regel, die sie ueberschreibt,
///   waere eine Aenderung ohne Test.
///
/// # WAS NICHT UEBERNOMMEN WIRD, und warum
///
/// * `text-align`, `white-space`, `text-decoration`, `line-height`:
///   `Style` (narsil-core) hat dafuer keine Felder. Sie zu erfinden waere
///   eine Aenderung an einer Crate, die nicht meine ist.
/// * `border-*`: `Style` hat keinen Rahmen. Dasselbe.
/// * Prozentmasse: `Length` kann sie, aber der Umbruch rechnet in Pixel.
///   Bis die Kaskade laeuft, waere ein Prozent hier eine Zahl, die
///   niemand aufloest — sie wird ignoriert, nicht geraten.
/// Die **eine** Stelle, die `narsil_css::Rahmenstil` und
/// `narsil_core::style::RahmenStil` kennt.
///
/// **WARUM EINE EIGENE FUNKTION:** die beiden Crates duerfen sich nicht kennen
/// (`narsil-core` darf `narsil-css` nicht kennen, `narsil-css` braucht `narsil-core`),
/// also muss die Abbildung an der **Grenze** stehen — dort, wo `Berechnet` zu
/// `Style` wird. `Hidden` faellt auf `Keiner`, weil CSS Backgrounds 3 §4.2
/// sagt, `hidden` sei dasselbe wie `none`.
fn rahmen_stil(wert: narsil_css::Rahmenstil) -> RahmenStil {
    match wert {
        narsil_css::Rahmenstil::Kein | narsil_css::Rahmenstil::Versteckt => RahmenStil::Keiner,
        narsil_css::Rahmenstil::Durchgehend => RahmenStil::Durchgezogen,
        // BILD_37: die LUECKE ist geschlossen. `narsil-css` liefert je Stil seinen
        // eigenen Wert (`parse.rs::rahmenstil_wert`), also ist die Zuordnung 1:1.
        // `Gemustert` bleibt der alte Sammelwert und wird wie `dashed` gelesen —
        // GEMESSEN ist er fuer `solid` nie erreichbar, weil `parse.rs` ihn nicht
        // mehr erzeugt.
        narsil_css::Rahmenstil::Gemustert | narsil_css::Rahmenstil::Gestrichelt => {
            RahmenStil::Gestrichelt
        }
        narsil_css::Rahmenstil::Gepunktet => RahmenStil::Gepunktet,
        narsil_css::Rahmenstil::Doppelt => RahmenStil::Doppelt,
        narsil_css::Rahmenstil::Furche => RahmenStil::Furche,
        narsil_css::Rahmenstil::Wulst => RahmenStil::Wulst,
        narsil_css::Rahmenstil::Innen => RahmenStil::Innen,
        narsil_css::Rahmenstil::Aussen => RahmenStil::Aussen,
    }
}

/// **BILD_44, Station 2: `narsil_css::Verlauf` -> `narsil_core::verlauf_typ::Verlauf`.**
///
/// **DAS IST DIE EINZIGE STELLE, an der ein Verlauf die Kette verlaesst**, und
/// sie ist absichtlich **minimal** — dieselbe Station wie `rahmen_stil` fuer
/// `border-style` (BILD_35/37).
///
/// **DREI DINGE, DIE DIE UEBERSETZUNG ENTSCHEIDET:**
///
/// 1. **Die Position bleibt ein ANTEIL.** `narsil_css::Farbstopp::anteil` ist
///    laut eigener Doku „bewusst **noch nicht** in Pixeln" — und das ist das
///    Richtige, denn die Kastengroesse gehoert **hier** nichts hinein.
///    **GEMESSEN** (Firefox 157, `team/testseiten/verlauf_lin_ff.png`): der
///    Verlauf laeuft ueber die **Pixelmitten**, `t = (x + 0,5)/breite`.
///    Wer die Position schon hier in Pixeln umrechnen wuerde, muesste die
///    Kastenbreite kennen — und **die steht hier nicht zur Verfuegung**.
/// 2. **Das Alpha wandert mit.** `narsil_css::Farbe` hat `a`, `narsil_core::Color`
///    auch, und Firefox interpoliert **premultipliziert** (GEMESSEN auf die
///    exakte 8-Bit-Zahl: `#ff000000 -> #0000ffff` ueber `#2040c0` ergibt bei
///    x = 50 **(16, 32, 224)**; **ohne** Multiplikation kaeme (79, 32, 160)).
/// 3. **Ein Verlauf mit weniger als zwei Stopps wird verworfen.** CSS Images 3
///    §3.3 verlangt mindestens zwei, und `narsil_css::verlauf_aus` liefert bei
///    einem einzelnen Stopp eine Liste mit **einem** Eintrag. Ein Rasterizer,
///    der daraus ein Bild macht, wuerde sich **seine** Farbe ausdenken — und
///    die ist nicht die des Parsers, sondern unsere.
/// Release 13.10.2026 (Agent CSS2): siehe `Style::clip_rand`.
fn clip_rand_von_css(s: &narsil_css::Berechnet, eltern: Option<&narsil_css::Berechnet>) -> Option<(u8, f32)> {
    let wert_von = |b: &narsil_css::Berechnet, name: &str| {
        b.schluessel.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.trim().to_ascii_lowercase())
    };
    let wert = |name: &str| wert_von(s, name);
    let malt = wert("contain").is_some_and(|w| w.split_whitespace().any(|t| matches!(t, "paint" | "strict" | "content")));
    // `overflow: clip` und `hidden` (auch an einem Rollkasten, WPT
    // `overflow-clip-margin-018..020`) und `contain: paint` — nicht an einem
    // gerollten Kasten (`scroll`/`auto`, WPT `-008`).
    let hidden = s.ueberlauf == narsil_css::Ueberlauf::Versteckt
        || s.overflow_x == Some(narsil_css::Ueberlauf::Versteckt)
        || s.overflow_y == Some(narsil_css::Ueberlauf::Versteckt);
    let rollt = matches!(s.ueberlauf, narsil_css::Ueberlauf::Scroll | narsil_css::Ueberlauf::Auto)
        || matches!(s.overflow_x, Some(narsil_css::Ueberlauf::Scroll | narsil_css::Ueberlauf::Auto));
    if rollt || !(s.ueberlauf_clip[0] || s.ueberlauf_clip[1] || hidden || malt) {
        return None;
    }
    let mut kante = 0u8;
    let mut px = 0.0f32;
    // `inherit`: der Wert des Eltern (WPT `overflow-clip-margin-009`).
    let angabe = match wert("overflow-clip-margin") {
        Some(w) if w == "inherit" => eltern.and_then(|e| wert_von(e, "overflow-clip-margin")),
        w => w,
    };
    let Some(w) = angabe else {
        // Ohne Angabe: der Ueberlauf schneidet wie sonst an der Padding-Box.
        return (s.ueberlauf_clip[0] || s.ueberlauf_clip[1] || malt).then_some((0, 0.0));
    };
    for t in w.split_whitespace() {
        match t {
            "padding-box" => kante = 0,
            "border-box" => kante = 1,
            "content-box" => kante = 2,
            _ => {
                // Auch negativ (WPT `overflow-clip-margin-016`, `-017`).
                let zahl = t.strip_suffix("px").unwrap_or(t);
                match zahl.parse::<f32>() {
                    Ok(v) if v.is_finite() => px = v,
                    _ => return Some((0, 0.0)),
                }
            }
        }
    }
    // Release 14.10.2026 (Agent CSS2): on a scroll container (`overflow:
    // hidden` without `clip`/`contain: paint`) the margin may only shrink the
    // clip, never move it outside the padding box: `border-box` is ignored,
    // including its offset, and so is a positive offset from the padding box.
    // MEASURED before (WPT css-overflow `overflow-clip-margin-021`, `-022`):
    // the clip sat on the border box.
    let scroller = hidden && !(s.ueberlauf_clip[0] || s.ueberlauf_clip[1] || malt);
    if scroller && (kante == 1 || (kante == 0 && px > 0.0)) {
        return None;
    }
    Some((kante, px))
}

/// Release 13.10.2026 (Agent CSS2): `background-size`/`-position`/`-repeat`
/// fuer einen Verlauf. `None` bei den Anfangswerten (`auto`, `0% 0%`,
/// `repeat`): dann fuellt der Verlauf den Kasten wie bisher.
pub fn kachel_von_css(g: &narsil_css::BildGeo) -> Option<narsil_core::verlauf_typ::Kachel> {
    use narsil_core::verlauf_typ::{Kachel, KachelMass as M};
    let mass = |l: Option<narsil_css::Laenge>| match l {
        None => M::Auto,
        Some(l) if l.einheit == narsil_css::Einheit::Prozent => M::Anteil(l.wert / 100.0),
        Some(l) => M::Px(l.wert),
    };
    // `cover`/`contain` ohne natuerliches Verhaeltnis: die Flaeche (§3.9).
    let groesse = if g.cover || g.contain { [M::Auto, M::Auto] } else { g.groesse_xy.map(mass) };
    let (px_x, px_y) = g.px_achsen();
    let k = Kachel {
        groesse,
        lage: [(g.position_x, px_x), (g.position_y, px_y)],
        wiederholen: [g.wiederholen_x, g.wiederholen_y],
        art: g.wiederhol_art,
        ursprung: g.ursprung,
        vom_ende: [g.vom_ende.0, g.vom_ende.1],
    };
    let anfang = k.groesse == [M::Auto, M::Auto]
        && k.lage[0].0 == 0.0
        && k.lage[1].0 == 0.0
        && k.wiederholen == [true, true]
        && k.art == [0, 0]
        && k.ursprung == 0
        && k.vom_ende == [false, false];
    (!anfang).then_some(k)
}

pub fn verlauf_von_css(wert: &narsil_css::Verlauf) -> Option<Verlauf> {
    if wert.stopps.len() < 2 {
        return None;
    }
    let mut stopps = Vec::with_capacity(wert.stopps.len());
    for s in &wert.stopps {
        stopps.push(Stopp {
            farbe: Color {
                r: s.farbe.r,
                g: s.farbe.g,
                b: s.farbe.b,
                a: s.farbe.a,
            },
            anteil: s.anteil,
        });
    }
    Some(Verlauf {
        art: match wert.art {
            narsil_css::VerlaufArt::Linear => VerlaufArt::Linear,
            narsil_css::VerlaufArt::Radial => VerlaufArt::Radial,
            narsil_css::VerlaufArt::Kegel => VerlaufArt::Kegel,
        },
        // CSS Images 3, §3.1: `0deg` ist **oben**, im Uhrzeigersinn. Wir
        // rechnen **nicht** um — das macht der Rasterizer, weil dort die
        // Kastenmasse bekannt ist.
        winkel: wert.winkel,
        stopps,
        wiederholt: wert.wiederholt,
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `Verlauf::radial`
        // (BILD_51, fremdes Feld). GEMESSEN, warum `None` und nicht ein
        // Wert: `narsil-core/src/verlauf_typ.rs:172` sagt, `None` heisst
        // „keine Angabe" — und ein **linearer** Verlauf traegt keine
        // Radialangabe. Das ist der Anfangswert, kein Verlust.
        radial: None,
        // **BILD_51: DIE LUECKE, UND SIE IST BENANNT.**
        //
        // **`narsil_css::Verlauf` traegt nur den `winkel`** — **keine** Form, **keine**
        // Endform, **keine** Mitte. Ein `radial-gradient` aus CSS kann hier also
        // **nichts** ausfuellen, und `None` heisst: **keine Angabe**.
        //
        // **UND WAS DAS FUER DAS BILD BEDEUTET — genau gesagt:** faellt hier ein
        // `VerlaufArt::Radial` an, zeichnet der Rasterizer **keinen** Pixel, weil
        // `radial_anteil` ohne Form/Endform/Mitte **nichts** rechnen kann und
        // `farbe_bei` dann `None` liefert. **Das ist Absicht und keine
        // Regression:** **vor** BILD_51 lieferte `farbe_bei` fuer **jede**
        // Nicht-Linear-Art `None`, das Bild ist also **vorher wie nachher
        // leer** — nur ist der Grund jetzt **benannt** statt still.
        //
        // **WER DAS SCHLIESSEN MUSS, ist CSS:** GEMESSEN
        // `crates/narsil-css/src/parse.rs:4134` — `richtung_aus` **lehnt** die
        // Eck-Schluesselwoerter ab („wird hier abgelehnt, statt geraten“), und
        // dasselbe gilt fuer `circle`/`ellipse`/`closest-side`/`farthest-corner`.
        // Bis `narsil_css::Verlauf` diese Felder traegt, ist eine
        // CSS-Radialangabe **nicht** in diesen Weg uebertragbar. Im BRETT
        // gemeldet.
    })
}

pub fn stil_von_css(
    hand: Style,
    autor: &Berechnet,
    mit_ua: &Berechnet,
    eltern: Option<&Berechnet>,
    ist_link: bool,
) -> Option<Style> {
    // 1. display:none gewinnt immer.
    //
    // `sichtbar` wird mitgeprueft, weil `display:none` im UA-Blatt als
    // `Anzeige::Keins` UND `visibility:hidden` als `sichtbar == false`
    // ankommt — beides heisst "nicht zeichnen", und beides ist eine
    // Eigenschaft des AUTORS, wenn er sie schreibt.
    // Release 10.10.2026: `visibility: hidden` baut das Element (es belegt
    // seinen Platz) und markiert es `unsichtbar` (siehe unten und
    // `Style::unsichtbar`) — nur `display: none` faellt weg.
    // Release 11.10.2026: Spalten (`table-column`) zeigen keinen Inhalt — ihr
    // eigener Kasten bleibt (sein Rahmen zaehlt im kollabierten Modell zur
    // Tabelle), ihre Nachkommen ausser weiteren Spalten fallen weg.
    let in_spalte = eltern.is_some_and(|e| e.anzeige == Anzeige::TabelleSpalte) && mit_ua.anzeige != Anzeige::TabelleSpalte;
    if autor.anzeige == Anzeige::Keins || mit_ua.anzeige == Anzeige::Keins || in_spalte {
        return None;
    }
    let standard = Berechnet::standard();
    let mut stil = hand;
    let mut gesetzt = false;

    // 2. Farbe: der Autor, sonst die UA — nein: sonst die Hand.
    //
    // DIE DREIWEGREGEL, und sie ist der ganze Unterschied zwischen
    // "CSS gewinnt" und "CSS gewinnt richtig". Sie beantwortet drei
    // Fragen in dieser Reihenfolge (CSS 2.1, 6.2 "Cascade", und 6.2.2
    // "inherited values": ein geerbter Wert ist die LETZTE Stufe und
    // verliert gegen jede gesetzte Angabe):
    //
    //   1. Hat der AUTOR an DIESEM Element geschrieben?  -> Autor
    //   2. Hat das UA-BLATT an DIESEM Element geschrieben?  -> UA
    //   3. Kam der Wert von einem Vorfahren?                 -> geerbt
    //   sonst                                                -> Hand
    //
    // GEMESSENE FAELLE, alle an `test2.html` (06:10):
    //   * `<a>Link</a>` in `body{color:#ddd}`: (1) nein, (2) ja
    //     (`a:link` aus dem UA-Blatt) -> blau. Ohne (2) waere der
    //     geerbte Wert #ddd in den Vordergrund gekommen — das war der
    //     RUECKSCHRITT, den die Koordination gemeldet hat.
    //   * `<p>` in derselben Seite: (1) nein, (2) nein, (3) ja -> #ddd.
    //   * `.klein{color:#999}`: (1) ja -> #999.
    //   * Seite ganz ohne CSS: (1)(2)(3) nein -> Hand.
    //
    // DIE AUSNAHME FUER LINKS, und sie ist die einzige:
    // `a:link` aus dem UA-Blatt ist #0000EE — ein Blau, das auf WEISS
    // gemessen wurde. Unsere Seite ist dunkel (#14141a), und dort ist
    // #0000EE ein dunkles Navy, das man ueberliest. `FARBE_LINK` ist
    // dagegen geprueft (Welle 4).
    //
    // Es ist eine THEME-Entscheidung und keine Kaskaden-Regel, also
    // steht sie hier und nicht in `waehle_farbe`. Der Autor kann sie
    // jederzeit ueberschreiben — `a { color: … }` ist Weg 1 der
    // Dreiwegregel und liegt DARUEBER. Genau das macht `test2.html`
    // mit `nav a { color: var(--akzent) }`.
    if let Some(wert) = waehle_farbe_mit_link(eltern, autor, mit_ua, &standard, ist_link) {
        stil.color = wert;
        gesetzt = true;
    }
    // 3. Hintergrund, nach derselben Regel wie die Farbe. Ein
    //    transparenter Hintergrund ist fuer den Rasterizer dasselbe
    //    wie keiner, aber die Entscheidung gehoert dem Autor.
    if let Some(f) = waehle_farbe_hintergrund(autor, mit_ua, &standard) {
        stil.background = f;
        gesetzt = true;
    }
    // 3b. MASKEN (06.10.2026, CSS Masking 1).
    //
    // GEMESSEN an de.wikipedia.org: die Symbole sind
    // `background-color: currentColor` + `mask-image: url(icon.svg)`.
    // Unser Bildpfad kennt keine Masken — uebrig blieb ein VOLLES
    // 20x20-Quadrat in #202122 (vier schwarze Kaesten im Kopf, mit der
    // echten Fensteraufnahme gemessen). Die ehrliche Darstellung: die
    // FUellung nicht zeichnen, wenn die Form fehlt — ein Quadrat ist
    // eine Luege, ein fehlendes Symbol ist eine bekannte Luecke.
    //
    // GEMESSENER FEHLER DER ERSTEN FASSUNG (noch am selben Tag, ueber
    // `acceptance_dunkel`, `hn_bgcolor`, `canvas_rumpf`): die Bedingung
    // war `!masken.is_empty()`. `Berechnet::standard()` traegt aber
    // SELBST eine Maske mit **leerer URL** (`mask-image: none` ist EINE
    // Ebene, `typen.rs:2611`), also war sie bei JEDEM Element wahr —
    // und jede Fuellung der Seite wurde transparent (gemessen: weisse
    // Kaesten statt #222/#f6f6ef). Es zaehlt nur eine Maske mit
    // **nicht-leerer URL**; ein Verlauf bleibt unangetastet, weil die
    // Fuellung darunter weniger falsch ist als ihr Fehlen.
    //
    // Release 15.10.2026 (Agent F): masks are applied now (the display list
    // masks the box with the image, see `Seite::maskenbilder`), so the
    // fill stays: the icon shows in the background color, as in browsers.
    if mit_ua.masken.iter().any(|m| !m.url.is_empty())
        || autor.masken.iter().any(|m| !m.url.is_empty())
    {
        stil.maske_bild = Some(std::sync::Arc::from("mask-image"));
        gesetzt = true;
    } else if mit_ua.masken.iter().chain(autor.masken.iter()).any(|m| m.verlauf.is_some()) {
        // A gradient mask layer.
        stil.maske_bild = Some(std::sync::Arc::from("mask-image"));
        gesetzt = true;
    }

    // 4. Schriftgroesse. `NaN` und 0 fallen weg: eine 0-px-Schrift ist
    //    ein Layoutfehler, und `Style::is_valid` wuerde sie melden.
    //
    // GEMESSENER FEHLER 30.09.2026: hier stand nur
    // `is_finite() && > 0.0`. Damit wurde JEDES Element auf 16 px
    // gesetzt — auch dann, wenn der Autor nichts geschrieben hatte und
    // nur die Hand-Typografie (h1 = 24 px) da war. Der Test
    // `hand_typografie_bleibt_wo_css_schweigt` meldete "h1 ist 16 px".
    //
    // Die richtige Bedingung ist dieselbe wie bei der Farbe: der Wert
    // muss sich vom STANDARD unterscheiden. Sonst ist nicht "der Autor
    // will 16 px" die Aussage, sondern "niemand hat etwas gesagt, und
    // 16 ist der Anfangswert".
    if let Some(px) = waehle_zahl(
        eltern,
        autor,
        mit_ua,
        &standard,
        |b| b.schriftgroesse_px,
        false,
    ) {
        stil.font_size = px;
        gesetzt = true;
    }
    // 4b. GEWICHT und STIL, nach derselben Dreiwegregel wie die Groesse.
    //
    // 04.10.2026 (SCHRIFT, von der Leitung freigegeben). GEMESSEN, was
    // vorher lief: an `texttest.html` kamen **alle vier** Stilzeilen gleich
    // an — Firefox 127 / 144 / 129 / 143 px, narsil 125 / 125 / 125 / 125.
    // Ueberschriften, `<b>`, `<strong>`, `<em>`, `<th>`: der ganze Text
    // einer Seite aufrecht.
    //
    // GEMESSEN, dass es hier und nicht in `narsil-css` fehlte:
    // `Berechnet::schriftgewicht` (typen.rs:1852) und `Berechnet::
    // schriftstil` (1853) sind **berechnet**, mit Vererbung und UA-Blatt
    // (`schriftgewicht_wert`, parse.rs:5802). Es fehlte der Transport nach
    // `narsil_core::style::Style` — und ab dort sieht der Layout-Auftrag
    // nur noch `node.style`.
    //
    // GEMESSEN, warum NICHT `waehle_zahl`: das ist der Weg fuer Mappen
    // und Fuellungen und **begrenzt** beide (`beschaendigen_margin`,
    // `beschaendigen_padding`). Ein Gewicht von 700 laege im Bereich,
    // aber ein Gewicht ist keine Laenge — die Begrenzung gehoert nicht
    // darum. Deshalb die eigene, kleine Fassung derselben Regel.
    //
    // GEMESSEN, warum `if gesetzt` falsch waere: `gesetzt` sagt, ob der
    // Autor die FARBE oder den HINTERGRUND geschrieben hat. Ein Element,
    // das nur die Farbe bekommt, verliert sonst sein Handgewicht (z. B.
    // `<h1>` = 700 aus `grad_fuer_tag`) — und `<h1>` ist genau einer der
    // Faelle, die die Leitung genannt hat.
    // Computed values include author overrides, UA defaults and inherited resets.
    // Value comparison cannot reveal whether an explicit `normal` was specified.
    if (1..=1000).contains(&mit_ua.schriftgewicht) {
        stil.schriftgewicht = mit_ua.schriftgewicht;
    }
    stil.kursiv = matches!(mit_ua.schriftstil, Schriftstil::Kursiv);

    // 4c. DIE FAMILIE, nach derselben Regel.
    //
    // 04.10.2026 (SCHRIFT). GEMESSEN, was vorher fehlte: `code`, `kbd`
    // und `samp` standen bei GEMESSEN 14,4 px in `DejaVuSans.ttf` — also
    // in der Fliesstextschrift, obwohl das UA-Blatt fuer sie
    // `font-family: monospace` sagt (`narsil-css/src/ua.rs`) und die
    // Kaskade die Familie berechnet (`Berechnet::schriftfamilie`).
    // `Style` hatte **kein** Feld dafuer, also konnte die Angabe nicht
    // ankommen — weder im Umbruch noch im Raster.
    //
    //
    // GEMESSEN, warum die LEERE Liste kein Ergebnis ist: der Wert ist ein
    // Wort aus dem Autorentext, und `Schriftfamilie` ist der Name der
    // Eigenschaft. Leer bedeutet „nichts gesagt" — dann gewinnt die
    // Hand-Typografie mit ihrer Vorgabe `sans-serif`.
    if !mit_ua.schriftfamilie.trim().is_empty() {
        stil.schriftfamilie = web_familie(&mit_ua.schriftfamilie).into_owned();
    }

    // 5. Abstaende. Nur die, die der Autor genannt hat; eine
    //    fehlende Angabe ist 0 und wuerde die Hand-Typografie loeschen.
    let marge_oben = waehle_zahl(eltern, autor, mit_ua, &standard, |b| b.marge_oben, true);
    let marge_unten = waehle_zahl(eltern, autor, mit_ua, &standard, |b| b.marge_unten, true);
    let marge_links = waehle_zahl(eltern, autor, mit_ua, &standard, |b| b.marge_links, true);
    let marge_rechts = waehle_zahl(eltern, autor, mit_ua, &standard, |b| b.marge_rechts, true);
    //
    // GEMESSENER FEHLER 30.09.2026, 06:16: hier stand
    // `if marge_oben.is_some() || marge_unten.is_some()`. `nav a
    // { margin-right: 12px }` setzt aber NUR rechts — und damit kam
    // nichts an. Der Block wird jetzt geprueft wie die Fuellung: wenn
    // eine der vier Seiten gesetzt ist, werden alle vier uebernommen
    // (die anderen drei behalten ihren Wert aus der Hand-Typografie).
    if marge_oben.is_some()
        || marge_unten.is_some()
        || marge_links.is_some()
        || marge_rechts.is_some()
    {
        // MARGEs: beide Seiten together, sonst waere `margin-left: 10px`
        // ein Absatz ohne Abstand oben.
        let mut m = stil.margin;
        if let Some(v) = marge_oben {
            m.top = Length::px(v);
        }
        if let Some(v) = marge_unten {
            m.bottom = Length::px(v);
        }
        if let Some(v) = marge_links {
            m.left = Length::px(v);
        }
        if let Some(v) = marge_rechts {
            m.right = Length::px(v);
        }
        stil.margin = m;
    }
    // 6. Fuellung, wenn der Autor eine genannt hat.
    let fuell_o = waehle_zahl(eltern, autor, mit_ua, &standard, |b| b.fuellung_oben, false);
    let fuell_u = waehle_zahl(
        eltern,
        autor,
        mit_ua,
        &standard,
        |b| b.fuellung_unten,
        false,
    );
    let fuell_l = waehle_zahl(
        eltern,
        autor,
        mit_ua,
        &standard,
        |b| b.fuellung_links,
        false,
    );
    let fuell_r = waehle_zahl(
        eltern,
        autor,
        mit_ua,
        &standard,
        |b| b.fuellung_rechts,
        false,
    );
    if fuell_o.is_some() || fuell_u.is_some() || fuell_l.is_some() || fuell_r.is_some() {
        stil.padding = Spacing {
            top: Length::px(fuell_o.unwrap_or(0.0)),
            right: Length::px(fuell_r.unwrap_or(0.0)),
            bottom: Length::px(fuell_u.unwrap_or(0.0)),
            left: Length::px(fuell_l.unwrap_or(0.0)),
        };
    }
    // 7. display:block -> Column, inline -> Row. GEMESSEN: der Bauer
    //    benutzt `Column` fuer Bloecke und `Row` fuer Zeilen; ein
    //    `inline` aus CSS darf daran nichts aendern (der Absatz baut
    //    seine Zeilen selbst), ein `block` darf es auch nicht. Der
    //    Punkt ist hier nur festgehalten, damit die Stelle existiert,
    //    an der es spaeter hingehoert.
    let _ = Direction::Column;
    let _ = gesetzt;

    // CODEX-8: Abstaende werden nicht geerbt. Auch ein berechneter
    // Nullwert muss die Hand-Vorgabe ersetzen (z.B. * { padding:0 }).
    // Das vollstaendige Kaskadenergebnis enthaelt bereits die UA-Regeln.
    // Release 08.10.2026: Prozent bleiben Prozent — das Layout loest sie
    // gegen die Breite des umgebenden Blocks auf (CSS 2.1 §8.3/§8.4).
    // Release 11.10.2026: `calc()` mit Prozent bleibt eine Mischung
    // (`Length::Gemischt`), aufgeloest gegen den umgebenden Block.
    let misch = mit_ua.mischung.as_deref().copied().unwrap_or_default();
    let laenge = |px: f32, prozent: Option<f32>, m: Option<(f32, f32)>| match (m, prozent) {
        (Some((a, p)), _) => Length::gemischt(a, p),
        (None, Some(p)) => Length::Percent(p),
        (None, None) => Length::px(px),
    };
    stil.margin = Spacing {
        top: laenge(mit_ua.marge_oben, mit_ua.marge_prozent[0], misch.marge[0]),
        right: laenge(mit_ua.marge_rechts, mit_ua.marge_prozent[1], misch.marge[1]),
        bottom: laenge(mit_ua.marge_unten, mit_ua.marge_prozent[2], misch.marge[2]),
        left: laenge(mit_ua.marge_links, mit_ua.marge_prozent[3], misch.marge[3]),
    };
    stil.padding = Spacing {
        top: laenge(mit_ua.fuellung_oben, mit_ua.fuellung_prozent[0], misch.fuellung[0]),
        right: laenge(mit_ua.fuellung_rechts, mit_ua.fuellung_prozent[1], misch.fuellung[1]),
        bottom: laenge(mit_ua.fuellung_unten, mit_ua.fuellung_prozent[2], misch.fuellung[2]),
        left: laenge(mit_ua.fuellung_links, mit_ua.fuellung_prozent[3], misch.fuellung[3]),
    };

    // BILD_32, Schritt 2: `border` wandert in den Stil.
    //
    // **GEMESSENER KORREKTURBAUMEN zu meiner eigenen Meldung in BILD_31:** ich
    // schrieb dort „`narsil-css` parst `border` und wirft es weg". **Das war
    // halb falsch.** `Berechnet` fuehrt **alle zwölf** Felder bereits —
    // `rahmen_{oben,rechts,unten,links}_{breite,stil,farbe}` (`typen.rs:1903ff`).
    // **Verloren ging der Wert zwischen `Berechnet` und `Style`** — dort gab es
    // kein Feld, jetzt schon (`Style::rahmen`, BILD_32 Schritt 1).
    //
    // **UND `currentColor` IST NICHT AUFGELOEST — GEMESSEN:** `typen.rs:1919`
    // sagt woertlich „Anfangswert schwarz = currentcolor-Naeherung". Die Kaskade
    // **speichert nicht, ob eine Farbe `currentColor` war** — also gibt es
    // nichts aufzuloesen, und ein Rahmen mit `currentcolor` bekommt **schwarz**
    // statt der Textfarbe. Das ist eine **echte Luecke**, kein Weglassen:
    // `input { border: 1px solid currentColor }` waere dann schwarz statt in der
    // Textfarbe. **Fuer die Faenge kommt es rarely vor** (Frames, UA-Blaetter),
    // **und die genaue Stelle ist `typen.rs` + die Kaskade.**
    //
    // **WARUM DIE VIER VARIANTEN GENUEGEN, OBEN UNTEN GLEICH:** `narsil_css::
    // Rahmenstil` hat **vier** Werte: `Kein`, `Durchgehend`, `Gemustert`
    // („Gestrichelt / gepunktet / doppelt — **nur gemerkt**") und `Versteckt`.
    // **GEMESSENE FOLGE:** `dashed`, `dotted`, `double`, `groove`, `ridge`,
    // `inset` und `outset` sind **in `narsil-css` nicht mehr unterscheidbar** —
    // die deko-Faelle **24-27 sind damit nicht erreichbar**, egal was der
    // Rasterizer kann. Das ist eine Klassenfrage fuer `narsil-css`, keine
    // Rasterizerfrage, und ich habe sie nicht angefasst.
    stil.rahmen = Rahmen {
        breite: [
            mit_ua.rahmen_oben_breite,
            mit_ua.rahmen_rechts_breite,
            mit_ua.rahmen_unten_breite,
            mit_ua.rahmen_links_breite,
        ],
        stil: [
            rahmen_stil(mit_ua.rahmen_oben_stil),
            rahmen_stil(mit_ua.rahmen_rechts_stil),
            rahmen_stil(mit_ua.rahmen_unten_stil),
            rahmen_stil(mit_ua.rahmen_links_stil),
        ],
        //
        // BILD_32: `Berechnet` fuehrt die Rahmenfarben als **`narsil_css::Farbe`**,
        // `Style::rahmen.farbe` als **`Color`** — **zwei Typen fuer eine
        // Farbe.** Die Umrechnung gibt es **bereits** in dieser Datei
        // (`fn farbe(f: narsil_css::Farbe) -> Color`), ich benutze sie statt eine
        // zweite zu schreiben. **GEMESSEN:** der Compiler hat mir das mit
        // `E0308` gesagt, nachdem `narsil-css` wieder gruen war.
        farbe: [
            farbe(mit_ua.rahmen_oben_farbe),
            farbe(mit_ua.rahmen_rechts_farbe),
            farbe(mit_ua.rahmen_unten_farbe),
            farbe(mit_ua.rahmen_links_farbe),
        ],
    };

    // 8. `text-decoration` (A3, 02.10.2026).
    //
    // CSS 2.1, 16.6: die Eigenschaft wird NICHT geerbt, sondern an die
    // in-flow-Nachfahren WEITERGEREICHT. Der Unterschied ist sichtbar: bei
    // `<a><b>Text</b></a>` ist das `<b>` nicht selbst unterstrichen, es
    // traegt aber die Linie des `<a>`.
    //
    // GEMESSEN: `Berechnet::dekoration` wurde seit 30.09. berechnet
    // (`narsil-css/src/kaskade.rs:1876`) und von niemandem gelesen — grep
    // ueber `crates/narsil-page/src` und `crates/narsil-raster/src` ergab null
    // Treffer. Die Bits sind bitgleich zu `narsil_core::style::Style::dekoration`.
    //
    // `overline` (Bit 2) wird durchgereicht, aber vom Rasterizer zu NICHTS
    // gezeichnet: dafuer gibt es keine gemessenen Font-Metriken, und eine
    // erfundene Linie ist schlimmer als eine fehlende.
    stil.dekoration = if autor.dekoration != 0 {
        autor.dekoration
    } else if let Some(el) = eltern {
        // Nur an in-flow-Nachfahren. Eine Float-Box oder ein Inline-Element
        // mit `display:none` erbt nichts — `Anzeige::Keins` ist der Weg,
        // auf dem das hier auffällt.
        if el.anzeige == Anzeige::Keins {
            0
        } else {
            el.dekoration
        }
    } else {
        0
    };

    // S2 Station 2: `opacity` aus `autor` uebernehmen.
    //
    // CSS 2.1 §11.13 / CSS Color 3 §3.2: `opacity` ist eine Zahl 0..1, kein
    // Prozentwert. `Berechnet::deckkraft` ist bereits auf [0,1] geklemmt
    // (GEMESSEN dort), hier wird nur noch NaN verworfen, weil
    // `Style::is_valid` es ablehnt und die Hand-Typografie dann stehen
    // bleiben soll statt zu einem unsichtbaren Element zu werden.
    stil.deckkraft = if autor.deckkraft.is_finite() {
        autor.deckkraft.clamp(0.0, 1.0)
    } else {
        1.0
    };

    // S2 Station 2: `border-radius` aus `autor`.
    //
    // CSS Backgrounds 3 §5.1: die vier Ecken sind oben-links, oben-rechts,
    // unten-rechts, unten-links. `Berechnet::radius` traegt sie in genau
    // dieser Reihenfolge, also ist hier keine Umsortierung noetig.
    //
    // Warum jede Ecke einzeln geklemmt wird: `radius` ist eine Laenge, und
    // eine negative Laenge ist ungueltig. `kasten_mit_radius` erwartet
    // zusaetzlich, dass alle vier Werte zusammenpassen — das macht der
    // Zeichner (`radius_form`), nicht die Kaskade.
    stil.radius = [
        beschaendigen_padding(autor.radius[0]).unwrap_or(0.0),
        beschaendigen_padding(autor.radius[1]).unwrap_or(0.0),
        beschaendigen_padding(autor.radius[2]).unwrap_or(0.0),
        beschaendigen_padding(autor.radius[3]).unwrap_or(0.0),
    ];
    // Release 15.10.2026 (Agent F): the elliptical radii (percentages of the
    // border box stay percentages).
    stil.radius_xy = autor.radius_xy.map(|p| p.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 }));
    stil.radius_percent = autor.radius_percent;

    // S2 Station 2: `box-shadow` aus `autor` uebernehmen.
    //
    // Die Abhaengigkeitsrichtung erlaubt hier kein `narsil_css::Schatten` in
    // `narsil_core::Style`: `narsil-core` weiss nichts von `narsil-css`. Der
    // Feldtypen `SchattenArt` in `narsil-core` ist deshalb bitgleich
    // aufgebaut, und die Umrechnung passiert hier, an der einen Stelle, die
    // beide Crates kennt.
    //
    // M2/M1: die Laenge ist durch `MAX_SCHATTEN` begrenzt. Es wird NIE
    // ueber die Grenze hinaus kopiert, auch wenn der Autor mehr Schatten
    // schreibt — `Berechnet::schatten` ist bereits auf acht gekappt, und
    // `schatten_anzahl` bleibt ein `u8`, das 255 nicht erreicht.
    // **BILD_48, Station 2: `background-image: linear-gradient(...)` fuellt jetzt
    // `Style::verlauf`.** GEMESSEN warum das noetig war: `verlauf_von_css`
    // hatte **null Aufruforte** — die Funktion stand da, wurde aber nie
    // gerufen.
    //
    // **DIE REIHENFOLGE ist wie beim Rahmen: erst der AUTOR, dann die UA.**
    // `background-image` ist eine **Autoren**eigenschaft; das UA-Blatt gibt sie
    // fuer **kein** Element vor (GEMESSEN in `ua.rs`: kein `background-image`).
    // Wir lesen trotzdem **beide**, damit eine spaetere UA-Regel nicht stillschweigend
    // verlorengeht.
    //
    // **UND: die Quelle ist `Berechnet::verlauf`**, das der Parser aus
    // `background-image` fuellt (GEMESSEN: `typen.rs:2427`). Das ist die
    // **vierte** Station der Kette, und sie ist die einzige, die **fremde**
    // Arbeit benutzt — ohne sie gaebe es nichts zu lesen.
    stil.verlauf = match autor.verlauf.as_ref().or(mit_ua.verlauf.as_ref()) {
        Some(v) => verlauf_von_css(v),
        None => None,
    };
    // Release 13.10.2026 (Agent CSS2): Groesse, Lage und Wiederholung des
    // Verlaufs (siehe `narsil_core::verlauf_typ::Kachel`).
    stil.verlauf_kachel = stil
        .verlauf
        .as_ref()
        .and_then(|_| kachel_von_css(&crate::bild_geo_mit_rand(autor.bild_geo, autor)));
    // Release 13.10.2026 (Agent CSS2): `overflow-clip-margin` an `overflow:
    // clip` und `contain: paint` (CSS Overflow 4 §3.1; an einem Rollkasten
    // ohne Wirkung). GEMESSEN vorher (WPT css-overflow `overflow-clip-margin-
    // *`): die Angabe wirkte nicht.
    stil.clip_rand = clip_rand_von_css(mit_ua, eltern);
    // Release 15.10.2026 (Agent F): `filter` on CSS boxes (the display list
    // paints the subtree alone and filters it).
    let computed_filter = |name: &str| {
        mit_ua
            .schluessel
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| narsil_css::ersetze_var(v.trim(), &mit_ua.variablen, 0))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("none") && !v.contains("var("))
            .map(std::sync::Arc::from)
    };
    stil.filter_value = computed_filter("filter");
    // `backdrop-filter` filters what is painted behind the box (Filter
    // Effects 2 §3).
    stil.backdrop_filter = computed_filter("backdrop-filter").or_else(|| computed_filter("-webkit-backdrop-filter"));
    // Release 15.10.2026 (Agent F): `clip-path` on CSS boxes (basic shapes
    // and reference boxes; the display list masks the box's subtree).
    stil.clip_pfad = mit_ua
        .schluessel
        .iter()
        .rev()
        .find(|(n, _)| n == "clip-path" || n == "-webkit-clip-path")
        .and_then(|(_, v)| narsil_css::ersetze_var(v.trim(), &mit_ua.variablen, 0))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("none") && !v.contains("var("))
        .map(std::sync::Arc::from);
    // Release 14.10.2026 (Agent CSS2): see `Style::border_image`.
    stil.border_image = autor.bild_geo.border_image.is_some() || crate::border_image_from(mit_ua).is_some();
    // Release 13.10.2026 (Agent CSS2): `outline` (CSS UI 4 §6). GEMESSEN
    // vorher (WPT css/cssom-view `getClientRects-inline`, `offsetTopLeft-
    // inline`: `outline: 2px solid white` deckt die Tinte ab): der Ring fehlte.
    // Release 14.10.2026 (Agent CSS2): only with an `outline-style` other
    // than `none`/`hidden` (table columns: see the page builder).
    let outline_style = mit_ua
        .schluessel
        .iter()
        .rev()
        .find(|(k, _)| k == "outline-style")
        .map_or("none", |(_, v)| v.trim());
    stil.umriss = (mit_ua.outline_breite > 0.0
        && mit_ua.outline_breite.is_finite()
        && !outline_style.eq_ignore_ascii_case("none")
        && !outline_style.eq_ignore_ascii_case("hidden"))
        .then(|| (mit_ua.outline_breite, farbe(mit_ua.outline_farbe)));

    stil.schatten_anzahl = 0;
    for s in autor.schatten.iter().take(MAX_SCHATTEN) {
        if (stil.schatten_anzahl as usize) >= MAX_SCHATTEN {
            break;
        }
        let platz = stil.schatten_anzahl as usize;
        // Farbe: `Farbe` und `Color` haben dieselben vier Kanale.
        // Die Laengen kommen unveraendert durch; ein NaN-Versatz waere ein
        // Zeichenbefehl ohne Ziel, also wird er zu 0.
        let x = if s.x.is_finite() { s.x } else { 0.0 };
        let y = if s.y.is_finite() { s.y } else { 0.0 };
        let weich = if s.weich.is_finite() {
            s.weich.max(0.0)
        } else {
            0.0
        };
        let ausbreitung = if s.ausbreitung.is_finite() {
            s.ausbreitung
        } else {
            0.0
        };
        stil.schatten[platz] = SchattenArt {
            farbe: farbe(s.farbe),
            x,
            y,
            weich,
            ausbreitung,
            innen: s.innen,
        };
        stil.schatten_anzahl += 1;
    }

    // 9. Text-Eigenschaften: text-align, white-space, line-height.
    //
    // Diese kommen aus `mit_ua` (berechnet mit UA-Blatt, also endgueltig).
    // `text-align` wird zusaetzlich in den Plan geschrieben
    // (`layout_plan_setze`), aber wir speichern ihn auch im Style fuer
    // direkte Abfrage.
    stil.textausrichtung = textausrichtung_uebernehmen(mit_ua.textausrichtung);
    stil.weissraum = weissraum_uebernehmen(mit_ua.weissraum);
    // Release 11.10.2026: `line-break` (CSS Text 3 §5.2).
    stil.umbruch_locker = matches!(mit_ua.zeilen_trennung.trim(), "loose" | "anywhere");
    stil.umbruch_ueberall = matches!(mit_ua.wort_trennung.trim(), "break-all" | "break-word")
        || matches!(mit_ua.wort_umbruch.trim(), "anywhere" | "break-word")
        || mit_ua.zeilen_trennung.trim() == "anywhere";
    // Release 15.10.2026 (Agent CSS2): `line-break: anywhere` wins over
    // `word-break: break-all` (it allows a superset of the breaks).
    // Release 15.10.2026 (Agent CSS2): see `Style::hyphens_none`.
    stil.hyphens_none = mit_ua
        .schluessel
        .iter()
        .rev()
        .find(|(n, _)| n == "hyphens")
        .is_some_and(|(_, w)| w.trim().eq_ignore_ascii_case("none"));
    // Release 15.10.2026 (Agent CSS2): see `Style::hyphenate_character` (a
    // quoted string; `auto` and anything else keep the default).
    stil.hyphenate_character = mit_ua
        .schluessel
        .iter()
        .rev()
        .find(|(n, _)| n == "hyphenate-character")
        .and_then(|(_, w)| {
            let w = w.trim();
            let q = w.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let inner = w.strip_prefix(q)?.strip_suffix(q)?;
            (inner.chars().count() <= 16).then(|| inner.to_string())
        });
    // Release 15.10.2026 (Agent CSS2): see `Style::break_word_only`.
    stil.break_word_only = mit_ua.wort_umbruch.trim() == "break-word"
        && !matches!(mit_ua.wort_trennung.trim(), "break-all" | "break-word")
        && mit_ua.zeilen_trennung.trim() != "anywhere";
    stil.letter_breaks = if mit_ua.zeilen_trennung.trim() == "anywhere" {
        narsil_core::style::LetterBreaks::Anywhere
    } else if mit_ua.wort_trennung.trim() == "break-all" {
        narsil_core::style::LetterBreaks::BreakAll
    } else {
        narsil_core::style::LetterBreaks::None
    };
    stil.zeilenhoehe_px = mit_ua.zeilenhoehe_px;
    stil.zeilenhoehe_art = zeilenhoehe_art_uebernehmen(mit_ua.zeilenhoehe_art);
    stil.unsichtbar = !autor.sichtbar;
    // Release 11.10.2026 (Agent CSS2): die EIGENE Hebung (die Vorfahren
    // addiert der Seitenbau, siehe `Marke`).
    // Nur fuer Inline-Kaesten: ein absolut positioniertes oder schwebendes
    // Element ist ein Block (CSS 2.1 §9.7), `vertical-align` gilt nicht (WPT
    // `linebox/vertical-align-super-001`).
    let inline_kasten = (matches!(mit_ua.anzeige, narsil_css::Anzeige::Inline | narsil_css::Anzeige::InlineBlock)
        || mit_ua.inline_aussen)
        && !matches!(mit_ua.position, narsil_css::Position::Absolut | narsil_css::Position::Fest)
        && mit_ua.schweben == narsil_css::Float::Keins;
    stil.vertikal_hoch = if inline_kasten {
        vertikal_hebung(&mit_ua.vertikal, mit_ua.schriftgroesse_px, mit_ua.zeilenhoehe_px)
    } else {
        0.0
    };
    // Release 11.10.2026 (Agent CSS2): `letter-spacing`/`word-spacing`
    // (vererbt; die Kaskade rechnet sie in px). GEMESSEN vorher (WPT
    // `text/letter-spacing-*`, `word-spacing-*`): ohne Wirkung.
    let abstand = |v: f32| if v.is_finite() { v.clamp(-10_000.0, 10_000.0) } else { 0.0 };
    // Release 12.10.2026: Prozent gegen die eigene Schrift (vererbt als Prozent).
    let fs = mit_ua.schriftgroesse_px;
    stil.buchstaben_abstand =
        abstand(mit_ua.buchstaben_prozent.map_or(mit_ua.buchstaben_abstand, |p| p / 100.0 * fs));
    stil.wort_abstand = abstand(mit_ua.wort_prozent.map_or(mit_ua.wort_abstand, |p| p / 100.0 * fs));

    Some(stil)
}

/// Release 11.10.2026 (Agent CSS2): `vertical-align` als Hebung der
/// Grundlinie in px (CSS 2.1 §10.8.1): eine Laenge (em gegen die eigene
/// Schrift), ein Prozentwert der eigenen `line-height`, `super`/`sub` mit
/// den ueblichen Anteilen (ein Drittel bzw. ein Fuenftel der Elternschrift,
/// genaehert ueber die eigene). Schluesselwoerter, die sich nach der Zeile
/// richten (`top`, `middle` ...), heben hier nicht. GEMESSEN vorher (WPT
/// `linebox/vertical-align-007..104`): `vertical-align: 96px` wirkte nicht.
fn vertikal_hebung(roh: &str, schrift: f32, zeile: f32) -> f32 {
    let t = roh.trim().to_ascii_lowercase();
    let fs = if schrift.is_finite() && schrift > 0.0 { schrift } else { 16.0 };
    let wert = match t.as_str() {
        "" | "baseline" | "grundlinie" | "top" | "bottom" | "middle" | "text-top" | "text-bottom" => 0.0,
        "super" => fs * 0.4,
        "sub" => -fs * 0.24,
        // Release 12.10.2026 (Agent CSS2): `calc()`, Prozente gegen die
        // Zeilenhoehe (CSS 2.1 §10.8.1). GEMESSEN vorher (WPT `css-values/
        // calc-vertical-align-1`): `calc(25px + 50%)` stand auf der Grundlinie.
        _ if t.starts_with("calc(") || t.starts_with("min(") || t.starts_with("max(") || t.starts_with("clamp(") => {
            let prozent = if zeile.is_finite() && zeile > 0.0 { zeile } else { fs * 1.2 };
            narsil_css::rechne(&t, &narsil_css::RechenCtx::neu(fs, prozent, 0.0), 0).unwrap_or(0.0)
        }
        _ => {
            let einheiten: [(&str, f32); 10] = [
                ("px", 1.0),
                ("pt", 4.0 / 3.0),
                ("pc", 16.0),
                ("in", 96.0),
                ("cm", 96.0 / 2.54),
                ("mm", 96.0 / 25.4),
                ("q", 96.0 / 101.6),
                ("em", fs),
                ("ex", fs * 0.5),
                ("%", if zeile.is_finite() && zeile > 0.0 { zeile / 100.0 } else { fs * 1.2 / 100.0 }),
            ];
            let mut v = None;
            for (e, f) in einheiten {
                if let Some(zahl) = t.strip_suffix(e) {
                    v = zahl.trim().parse::<f32>().ok().map(|z| z * f);
                    break;
                }
            }
            v.or_else(|| t.parse::<f32>().ok().filter(|z| *z == 0.0)).unwrap_or(0.0)
        }
    };
    if wert.is_finite() { wert.clamp(-100_000.0, 100_000.0) } else { 0.0 }
}

/// Wandelt `narsil_css::Textausrichtung` in `narsil_core::style::Textausrichtung` um.
fn textausrichtung_uebernehmen(v: narsil_css::Textausrichtung) -> narsil_core::style::Textausrichtung {
    use narsil_core::style::Textausrichtung as Core;
    use narsil_css::Textausrichtung as Css;
    match v {
        Css::Links => Core::Links,
        Css::Rechts => Core::Rechts,
        Css::Zentriert => Core::Zentriert,
        Css::Blocksatz => Core::Blocksatz,
    }
}

/// Wandelt `narsil_css::Weissraum` in `narsil_core::style::Weissraum` um.
fn weissraum_uebernehmen(v: narsil_css::Weissraum) -> narsil_core::style::Weissraum {
    use narsil_core::style::Weissraum as Core;
    use narsil_css::Weissraum as Css;
    match v {
        Css::Normal => Core::Normal,
        Css::Pre => Core::Pre,
        Css::KeinUmbruch => Core::KeinUmbruch,
        Css::PreUmbruch => Core::PreUmbruch,
        Css::PreZeile => Core::PreZeile,
        Css::UmbruchLeer => Core::UmbruchLeer,
    }
}

/// Wandelt `narsil_css::ZeilenhoeheArt` in `narsil_core::style::ZeilenhoeheArt` um.
fn zeilenhoehe_art_uebernehmen(v: narsil_css::ZeilenhoeheArt) -> narsil_core::style::ZeilenhoeheArt {
    use narsil_core::style::ZeilenhoeheArt as Core;
    use narsil_css::ZeilenhoeheArt as Css;
    match v {
        Css::Normal => Core::Normal,
        Css::Faktor(f) => Core::Faktor(f),
        Css::Absolut => Core::Absolut,
    }
}

/// DIE DREIWEGREGEL fuer eine Zahl.
///
/// # DIE DREI FRAGEN, in dieser Reihenfolge (CSS 2.1, 6.2)
///
///   1. Hat der AUTOR an DIESEM Element etwas geschrieben?
///   2. Hat das UA-BLATT an DIESEM Element etwas geschrieben?
///   3. Kam der Wert von einem Vorfahren?
///
/// Sonst: die Hand-Typografie (unsere Ebene, ganz unten).
///
/// # WARUM STUFE 1 DER ELTERNWERT IST und nicht der Standardwert
///
/// GEMESSENER FEHLER 30.09.2026, 06:14: Stufe 1 wurde zuerst mit
/// `berechnen(.., eltern = None, ..)` gebaut — "ohne Elternwert". Das
/// klingt nach "was hat der Autor HIER geschrieben?", ist aber
/// unbrauchbar: ohne Elternwert kann die Kaskade einen
/// Nachfahr-Selektor nicht aufloesen. `nav a { margin-right: 12px }`
/// passte nie, und der Abstand war wieder weg.
///
/// Der Elternwert loest beides: er IST der geerbte Wert, also
/// `autor.x != eltern.x` heisst genau "der Autor hat es an diesem
/// Element geaendert" — und die Kaskade kann Nachfahr-Selektoren
/// aufloesen, weil die Elternkette da ist.
///
/// `None` (Wurzel) heisst: es gibt nichts zu erben, und die Basis ist
/// der Standardwert.
///
/// `negativ_erlaubt`: true fuer `margin` (CSS 2.1 8.3.1), false fuer
/// `padding`, `font-size`, `radius` (CSS 2.1 8.3.2 / 10.1).
fn waehle_zahl(
    eltern: Option<&Berechnet>,
    autor: &Berechnet,
    mit_ua: &Berechnet,
    standard: &Berechnet,
    holen: fn(&Berechnet) -> f32,
    negativ_erlaubt: bool,
) -> Option<f32> {
    let basis = eltern.unwrap_or(standard);
    // 1. Der Autor an DIESEM Element.
    if holen(autor) != holen(basis) {
        return if negativ_erlaubt {
            beschaendigen_margin(holen(autor))
        } else {
            beschaendigen_padding(holen(autor))
        };
    }
    // 2. Das UA-Blatt an DIESEM Element.
    if holen(mit_ua) != holen(autor) {
        return if negativ_erlaubt {
            beschaendigen_margin(holen(mit_ua))
        } else {
            beschaendigen_padding(holen(mit_ua))
        };
    }
    // 3. Geerbt — dann aber nur, wenn der Autor ihn gesetzt hat. Ein
    //    geerbter ANFANGSwert ist nicht "gesetzt", sonst wuerde der
    //    Standardwert die Hand-Typografie ueberschreiben.
    if holen(autor) != holen(standard) {
        return if negativ_erlaubt {
            beschaendigen_margin(holen(autor))
        } else {
            beschaendigen_padding(holen(autor))
        };
    }
    None
}



/// DIE DREIWEGREGEL fuer eine Farbe, mit der Link-Ausnahme.
///
/// `ist_link == true` heisst: die Stufen 2 und 3 entfallen. Siehe die
/// Begruendung in [`stil_von_css`] — es geht um die Lesbarkeit auf
/// einem dunklen Grund, nicht um die Kaskade.
fn waehle_farbe_mit_link(
    eltern: Option<&Berechnet>,
    autor: &Berechnet,
    mit_ua: &Berechnet,
    standard: &Berechnet,
    ist_link: bool,
) -> Option<Color> {
    let basis = eltern.unwrap_or(standard);
    if ist_link {
        if autor.farbe != basis.farbe {
            return Some(farbe(autor.farbe));
        }
        // Release 08.10.2026: der Autor kann dem Link GENAU die geerbte
        // Farbe geben (`header .button{color:black}` unter schwarzem
        // Text, rust-lang.org) — der Vergleich mit dem Eltern sieht dann
        // „nichts gesetzt“, und der Link wurde blau. Die Kaskade MIT
        // UA-Blatt weiss es: ohne Autorregel traegt sie `a{color:#0000ee}`,
        // mit Autorregel deren Wert.
        let l = crate::FARBE_LINK;
        let ua_blau = narsil_css::Farbe { r: l.r, g: l.g, b: l.b, a: l.a };
        return if mit_ua.farbe != ua_blau {
            Some(farbe(mit_ua.farbe))
        } else {
            None
        };
    }
    if autor.farbe != basis.farbe {
        return Some(farbe(autor.farbe));
    }
    if mit_ua.farbe != autor.farbe {
        return Some(farbe(mit_ua.farbe));
    }
    if autor.farbe != standard.farbe {
        return Some(farbe(autor.farbe));
    }
    None
}

/// Wie [`waehle_farbe`], aber fuer `background-color`.
fn waehle_farbe_hintergrund(
    autor: &Berechnet,
    mit_ua: &Berechnet,
    standard: &Berechnet,
) -> Option<Color> {
    //
    // 03.10. 16:40: `background-color` IST NICHT VERERBLICH (Befund 11:45,
    // `team/PROBLEME.txt`).
    //
    // GEMESSENER FEHLER: der Vergleich war gegen den ELTERNstil
    // (`eltern.unwrap_or(standard)`), und dadurch bekam jedes Kind eines
    // gefaerbten Elements dieselbe Farbe. GEMESSENE FOLGE (Bild, 16:35):
    // `<body style="background-color:#222">` malte den RUMPF nicht — die
    // Farbe sass auf dem Absatz darunter, und der Rumpf blieb durchsichtig,
    // also sah die Seite weiss aus.
    //
    // GEMESSENE ENTSCHEIDUNG, warum nicht die Elternfarbe als „Startwert":
    // CSS 2.1, 9.2.1 zaehlt `background-color` ausdruecklich NICHT zu den
    // vererbten Eigenschaften; `narsil-css` gibt der CSS-Ebene das auch schon
    // richtig (`kaskade.rs`: `wird_vererbt(e)`), und diese Stelle hat es
    // wieder rueckgaengig gemacht. Der Vergleich laeuft deshalb gegen
    // `standard` — den Anfangswert des Elements selbst.
    if autor.hintergrund != standard.hintergrund {
        return Some(farbe(autor.hintergrund));
    }
    if mit_ua.hintergrund != autor.hintergrund {
        return Some(farbe(mit_ua.hintergrund));
    }
    None
}

/// Eine CSS-Farbe als `narsil_core`-Farbe.
fn farbe(f: narsil_css::Farbe) -> Color {
    Color {
        r: f.r,
        g: f.g,
        b: f.b,
        a: f.a,
    }
}

/// Eine CSS-Laenge in Pixel, oder `None`, wenn sie nicht endlich ist.
///
/// WARUM `None` und nicht 0: `NaN` in einem `Length` waere ein
/// Layoutfehler, und `Style::is_valid` (narsil-core) lehnt `NaN` ab. Der
/// Wert faellt dann weg, und die Hand-Typografie bleibt stehen.
///
/// `margin_erlaubt_negativ` unterscheidet Margin (CSS 2.1 8.3.1:
/// negative Werte erlaubt) von Padding (8.3.2: negative Werte ungueltig).
fn beschaendigen_margin(v: f32) -> Option<f32> {
    if v.is_finite() { Some(v) } else { None }
}

fn beschaendigen_padding(v: f32) -> Option<f32> {
    if v.is_finite() && v >= 0.0 {
        Some(v)
    } else {
        None
    }
}

// ===========================================================================
// PSEUDOELEMENTE (`::before` / `::after`, 06.10.2026)
// ===========================================================================

/// EINE REGEL FUER EIN PSEUDOELEMENT.
///
/// GEMESSEN an de.wikipedia.org: `.skin-vector-2022 .mw-logo::before`
/// traegt die Wikipedia-Kugel (`content:""`, `background-image`,
/// `width/height`), `::after` die Wortmarke. Vorher fiel die GANZE Regel
/// beim Parser (`::` war nicht in der Teilmenge), und die Kugel fehlte.
///
/// Der Selektor hier traegt die Marke **nicht** mehr: der Matcher
/// erfuellt `Pseudo::Vorher/Nachher` nie (die Regel haengt sonst am
/// Element), und nur ohne die Marke laesst sich pruefen, ob die Regel
/// zu DIESEM Element gehoert.
/// Release 10.10.2026: die Pseudo-Regeln einer Seite MIT Index — je Regel
/// ein Selektor in einem eigenen Blatt, damit der `Vormerker` je Element nur
/// die Regeln liefert, deren rechtes Glied (Id, Klasse, Tag) passen kann.
/// GEMESSEN vorher (Stichproben mit cdb, spiegel.de): `pseudo_kasten` pruefte
/// je Element ZWEIMAL ALLE Pseudo-Regeln mit vollem Selektorvergleich.
pub(crate) struct PseudoSatz {
    pub(crate) regeln: Vec<PseudoRegel>,
    pub(crate) vormerker: narsil_css::Vormerker,
}

impl PseudoSatz {
    pub(crate) fn bauen(regeln: Vec<PseudoRegel>) -> PseudoSatz {
        let blatt = narsil_css::Stylesheet {
            regeln: regeln
                .iter()
                .map(|r| {
                    narsil_css::Regel::Stil(narsil_css::StilRegel {
                        selektoren: vec![r.selektor.clone()],
                        deklarationen: Vec::new(),
                        variablen: Vec::new(),
                        zusatz_medien: Vec::new(),
                    })
                })
                .collect(),
            verworfen: 0,
        };
        let vormerker = narsil_css::Vormerker::bauen(std::slice::from_ref(&blatt));
        PseudoSatz { regeln, vormerker }
    }
}

pub(crate) struct PseudoRegel {
    pub(crate) selektor: narsil_css::Selektor,
    /// `true` = `::after`, `false` = `::before`.
    pub(crate) nachher: bool,
    /// Release 10.10.2026: eine `::placeholder`-Regel (siehe
    /// `platzhalter_farbe`); `nachher` ist dann bedeutungslos.
    pub(crate) platzhalter: bool,
    /// Release 10.10.2026: eine `::first-letter`-Regel (siehe
    /// `erster_buchstabe_regeln`); `nachher` ist dann bedeutungslos.
    pub(crate) erster_buchstabe: bool,
    /// Release 15.10.2026 (Agent CSS2): a `::first-line` rule (see
    /// `erste_zeile_regeln`).
    pub(crate) erste_zeile: bool,
    pub(crate) deklarationen: Vec<narsil_css::Deklaration>,
    /// Release 10.10.2026: setzt die Regel `content`? (siehe `pseudo_kasten`)
    pub(crate) hat_inhalt: bool,
}

/// Ein fertig gerechneter Pseudokasten.
pub(crate) struct PseudoKasten {
    /// Der DOM-Stil des Kastens (Groesse, Hintergrund, Rahmen, Margin).
    pub(crate) stil: Style,
    /// Der Layout-Plan (u. a. `display`).
    pub(crate) plan: narsil_layout::LayoutStil,
    /// Das Hintergrundbild, wenn die Regel eines nennt (oder eine Maske —
    /// dann wird die Maske gezeichnet, siehe `pseudo_kasten`).
    pub(crate) bild: Option<(String, narsil_css::BildGeo)>,
    /// Release 15.10.2026 (Agent F): the `mask-image` layers of the box.
    pub(crate) maske: Option<Vec<crate::MaskenEbene>>,
    /// Release 08.10.2026: `display: inline`/`inline-block` (vor der
    /// Abbildung auf den Layout-Plan, der beides als Block fuehrt) — ein
    /// solcher Kasten gehoert an einem Inline-Element IN den Textfluss.
    pub(crate) inline: bool,
    /// Release 08.10.2026: der TEXT aus `content` (Zeichenketten, `attr()`),
    /// wenn es einer ist — siehe [`inhalt_text`].
    pub(crate) text: Option<String>,
    /// Release 11.10.2026: der rohe `content`-Wert, wenn er Zaehler oder
    /// Anfuehrungszeichen enthaelt — die kennt erst der Bau (Dokument-
    /// reihenfolge); er setzt dann `text`.
    pub(crate) inhalt: Option<String>,
    /// Release 11.10.2026: `counter-reset`/`-set`/`-increment` des Kastens.
    pub(crate) zaehler: Vec<ZaehlerAngabe>,
    /// Release 11.10.2026: `quotes` des Kastens (sonst die des Elements).
    pub(crate) zitate: Option<Vec<(String, String)>>,
    /// Release 11.10.2026: ein Kasten auf Blockebene (`display: block`,
    /// `list-item`, `table` …) — sein Text steht auf einer eigenen Zeile.
    pub(crate) blockebene: bool,
    /// Release 15.10.2026 (Agent CSS2): `text-wrap: balance` of the pseudo
    /// itself (with inheritance), for its own paragraph.
    pub(crate) balance: bool,
}

impl PseudoKasten {
    /// Release 08.10.2026: gehoert der Kasten an einem Inline-Element in
    /// den Textfluss? Inline/Inline-Block und absolut/fest positioniert
    /// (dessen statische Position ist die Stelle im Text).
    /// Release 08.10.2026: ist der Kasten nur TEXT (kein Symbol, keine
    /// eigene Groesse, nicht positioniert)? Dann wird er als Text in den
    /// Inhalt des Elements gesetzt statt als Kasten.
    pub(crate) fn als_text(&self) -> Option<&str> {
        let text = self.text.as_deref()?;
        (!self.ist_kasten()).then_some(text)
    }

    /// Release 11.10.2026: wird der Kasten (nach dem Aufloesen der Zaehler)
    /// Text? Fuer `::after`, dessen Text erst nach den Kindern feststeht.
    pub(crate) fn wird_text(&self) -> bool {
        (self.text.is_some() || self.inhalt.is_some()) && !self.ist_kasten()
    }

    /// Release 11.10.2026: steht der Text auf einer eigenen Zeile
    /// (`display: block`, `list-item`, Tabellenteile)?
    pub(crate) fn eigene_zeile(&self) -> bool {
        self.blockebene
    }

    fn ist_kasten(&self) -> bool {
        use narsil_core::length::Length as L;
        self.bild.is_some()
            || self.stil.size.width != L::Auto
            || self.stil.size.height != L::Auto
            || matches!(self.plan.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)
            || self.plan.schweben != narsil_layout::plan::FloatArt::Keins
    }

    pub(crate) fn im_fluss(&self) -> bool {
        self.inline
            || matches!(
                self.plan.position,
                narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest
            )
    }
}

/// Release 08.10.2026: der Text eines `content`-Werts (CSS 2.1 §12.2):
/// Zeichenketten (mit `\XXXX`-Escapes) und `attr(name)`; der Alternativtext
/// hinter `/` zaehlt nicht. Zaehler (`counter()`), Bilder und Anfuehrungs-
/// Schluesselwoerter geben keinen Text — ein halber Zaehler („. " ohne Zahl)
/// waere falscher als keiner. Zeichen aus dem privaten Unicode-Bereich sind
/// Symbolschriften (Font Awesome …), die hier nicht geladen sind: dann
/// ebenfalls kein Text, statt eines Kaestchens.
/// Release 11.10.2026: was `content` ausser Text erzeugt — Zaehler (CSS
/// Lists 3 §4.6) und Anfuehrungszeichen (CSS Content 3 §2.3). Der Bau
/// loest sie in Dokumentreihenfolge auf.
pub(crate) enum Erzeugt {
    Zaehler { name: String, trenner: Option<String>, stil: String },
    Zitat(Zitat),
}

#[derive(Clone, Copy)]
pub(crate) enum Zitat {
    Auf,
    Zu,
    KeinAuf,
    KeinZu,
}

/// Eine CSS-Zeichenkette ab `z[i]` (dem Anfuehrungszeichen) lesen, mit
/// `\XXXX`-Escapes; gibt den Text und die Stelle hinter dem Ende zurueck.
fn zeichenkette_lesen(z: &[char], mut i: usize) -> (String, usize) {
    let c = z[i];
    let mut raus = String::new();
    i += 1;
    while i < z.len() && z[i] != c {
        if z[i] == '\\' && i + 1 < z.len() {
            i += 1;
            let mut hex = String::new();
            while i < z.len() && hex.len() < 6 && z[i].is_ascii_hexdigit() {
                hex.push(z[i]);
                i += 1;
            }
            if hex.is_empty() {
                if z[i] != '\n' {
                    raus.push(z[i]);
                }
                i += 1;
            } else {
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    raus.push(ch);
                }
                if i < z.len() && z[i] == ' ' {
                    i += 1;
                }
            }
            continue;
        }
        raus.push(z[i]);
        i += 1;
    }
    (raus, i + 1)
}

/// Release 11.10.2026: die Zeichenketten eines Werts (`quotes: "«" "»"`);
/// `None`, wenn etwas anderes darin steht.
pub(crate) fn zeichenketten(w: &str) -> Option<Vec<String>> {
    let z: Vec<char> = w.chars().take(1024).collect();
    let mut raus = Vec::new();
    let mut i = 0usize;
    while i < z.len() {
        if z[i].is_whitespace() {
            i += 1;
            continue;
        }
        if z[i] != '"' && z[i] != '\'' {
            return None;
        }
        let (t, weiter) = zeichenkette_lesen(&z, i);
        raus.push(t);
        i = weiter;
    }
    Some(raus)
}

/// Release 11.10.2026: `counter-reset`/`counter-set`/`counter-increment`
/// (CSS Lists 3 §4) — in der Reihenfolge, in der sie gelten: erst
/// zuruecksetzen, dann setzen, dann erhoehen.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ZaehlerAngabe {
    pub(crate) art: ZaehlerArt,
    pub(crate) name: String,
    pub(crate) wert: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ZaehlerArt {
    Zuruecksetzen,
    Setzen,
    Erhoehen,
}

pub(crate) fn zaehler_angaben(schluessel: &[(String, String)]) -> Vec<ZaehlerAngabe> {
    let mut raus = Vec::new();
    for (eigenschaft, art, vorgabe) in [
        ("counter-reset", ZaehlerArt::Zuruecksetzen, 0i64),
        ("counter-set", ZaehlerArt::Setzen, 0),
        ("counter-increment", ZaehlerArt::Erhoehen, 1),
    ] {
        let Some(w) = schluessel.iter().find(|(n, _)| n == eigenschaft).map(|(_, w)| w.trim()) else {
            continue;
        };
        if w.eq_ignore_ascii_case("none") || w.contains('(') {
            continue;
        }
        let mut teile = w.split_whitespace().peekable();
        while let Some(name) = teile.next() {
            if name.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+')
                && name.parse::<i64>().is_ok()
            {
                continue;
            }
            if matches!(name.to_ascii_lowercase().as_str(), "none" | "inherit" | "initial" | "unset" | "revert") {
                continue;
            }
            let wert = match teile.peek().and_then(|t| t.parse::<i64>().ok()) {
                Some(v) => {
                    teile.next();
                    v
                }
                None => vorgabe,
            };
            if raus.len() < 64 {
                raus.push(ZaehlerAngabe { art, name: name.to_string(), wert });
            }
        }
    }
    raus
}

/// Release 11.10.2026: `quotes` (CSS Content 3 §2.3): `none` sind keine
/// Paare, `auto`/`initial` die Vorgabe, sonst Paare von Zeichenketten.
/// `None` heisst: nicht angegeben (geerbt).
pub(crate) fn zitate_angabe(schluessel: &[(String, String)]) -> Option<Vec<(String, String)>> {
    let w = schluessel.iter().find(|(n, _)| n == "quotes")?.1.trim();
    match w.to_ascii_lowercase().as_str() {
        "none" => return Some(Vec::new()),
        "auto" | "initial" => return Some(zitate_vorgabe()),
        "inherit" | "unset" | "revert" | "" => return None,
        _ => {}
    }
    let ketten = zeichenketten(w)?;
    if ketten.is_empty() || ketten.len() % 2 != 0 {
        return None;
    }
    Some(ketten.chunks(2).map(|p| (p[0].clone(), p[1].clone())).collect())
}

/// Die Vorgabe fuer `quotes: auto` (Englisch, wie Chrome/Firefox ohne `lang`).
pub(crate) fn zitate_vorgabe() -> Vec<(String, String)> {
    vec![("\u{201C}".into(), "\u{201D}".into()), ("\u{2018}".into(), "\u{2019}".into())]
}

/// Release 11.10.2026: ein Zaehlerwert in einem Zaehlerstil (CSS Counter
/// Styles 3 §6, die vordefinierten Stile); ein unbekannter Stil ist
/// `decimal`, ein Wert ausserhalb des Bereichs ebenso.
pub(crate) fn zaehler_format(n: i64, stil: &str) -> String {
    fn alphabetisch(n: i64, zeichen: &[char]) -> Option<String> {
        if n < 1 {
            return None;
        }
        let basis = zeichen.len() as i64;
        let mut n = n;
        let mut raus = Vec::new();
        while n > 0 && raus.len() < 32 {
            n -= 1;
            raus.push(zeichen[(n % basis) as usize]);
            n /= basis;
        }
        Some(raus.iter().rev().collect())
    }
    fn additiv(n: i64, tafel: &[(i64, &str)], bis: i64) -> Option<String> {
        if !(1..=bis).contains(&n) {
            return None;
        }
        let mut rest = n;
        let mut raus = String::new();
        for (wert, zeichen) in tafel {
            while rest >= *wert {
                raus.push_str(zeichen);
                rest -= wert;
            }
        }
        Some(raus)
    }
    fn armenisch(n: i64, erstes: u32) -> Option<String> {
        if !(1..=9999).contains(&n) {
            return None;
        }
        let mut raus = String::new();
        for (stelle, teiler) in [(3u32, 1000i64), (2, 100), (1, 10), (0, 1)] {
            let ziffer = (n / teiler) % 10;
            if ziffer > 0 {
                raus.extend(char::from_u32(erstes + stelle * 9 + ziffer as u32 - 1));
            }
        }
        Some(raus)
    }
    const LATEIN: [char; 26] = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v',
        'w', 'x', 'y', 'z',
    ];
    const GRIECHISCH: [char; 24] = [
        'α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ', 'ν', 'ξ', 'ο', 'π', 'ρ', 'σ', 'τ', 'υ', 'φ', 'χ',
        'ψ', 'ω',
    ];
    const ROEMISCH: [(i64, &str); 13] = [
        (1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"),
        (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i"),
    ];
    const GEORGISCH: [(i64, &str); 37] = [
        (10000, "ჵ"), (9000, "ჰ"), (8000, "ჯ"), (7000, "ჴ"), (6000, "ხ"), (5000, "ჭ"), (4000, "წ"),
        (3000, "ძ"), (2000, "ც"), (1000, "ჩ"), (900, "შ"), (800, "ყ"), (700, "ღ"), (600, "ქ"),
        (500, "ფ"), (400, "ჳ"), (300, "ტ"), (200, "ს"), (100, "რ"), (90, "ჟ"), (80, "პ"), (70, "ო"),
        (60, "ჲ"), (50, "ნ"), (40, "მ"), (30, "ლ"), (20, "კ"), (10, "ი"), (9, "თ"), (8, "ჱ"),
        (7, "ზ"), (6, "ვ"), (5, "ე"), (4, "დ"), (3, "გ"), (2, "ბ"), (1, "ა"),
    ];
    const HEBRAEISCH: [(i64, &str); 37] = [
        (10000, "י׳"), (9000, "ט׳"), (8000, "ח׳"), (7000, "ז׳"), (6000, "ו׳"), (5000, "ה׳"), (4000, "ד׳"),
        (3000, "ג׳"), (2000, "ב׳"), (1000, "א׳"), (400, "ת"), (300, "ש"), (200, "ר"), (100, "ק"),
        (90, "צ"), (80, "פ"), (70, "ע"), (60, "ס"), (50, "נ"), (40, "מ"), (30, "ל"), (20, "כ"),
        (19, "יט"), (18, "יח"), (17, "יז"), (16, "טז"), (15, "טו"), (10, "י"), (9, "ט"), (8, "ח"),
        (7, "ז"), (6, "ו"), (5, "ה"), (4, "ד"), (3, "ג"), (2, "ב"), (1, "א"),
    ];
    let stil = stil.trim().to_ascii_lowercase();
    let raus = match stil.as_str() {
        "none" => Some(String::new()),
        "disc" => Some("\u{2022}".to_string()),
        "circle" => Some("\u{25E6}".to_string()),
        "square" => Some("\u{25AA}".to_string()),
        "disclosure-open" => Some("\u{25BE}".to_string()),
        "disclosure-closed" => Some("\u{25B8}".to_string()),
        "decimal-leading-zero" if (-9..=9).contains(&n) => {
            Some(if n < 0 { format!("-0{}", -n) } else { format!("0{n}") })
        }
        "lower-roman" => additiv(n, &ROEMISCH, 3999),
        "upper-roman" => additiv(n, &ROEMISCH, 3999).map(|r| r.to_ascii_uppercase()),
        "lower-alpha" | "lower-latin" => alphabetisch(n, &LATEIN),
        "upper-alpha" | "upper-latin" => alphabetisch(n, &LATEIN).map(|r| r.to_ascii_uppercase()),
        "lower-greek" => alphabetisch(n, &GRIECHISCH),
        "armenian" | "upper-armenian" => armenisch(n, 0x531),
        "lower-armenian" => armenisch(n, 0x561),
        "georgian" => additiv(n, &GEORGISCH, 19999),
        "hebrew" => additiv(n, &HEBRAEISCH, 10999),
        _ => None,
    };
    raus.unwrap_or_else(|| n.to_string())
}

pub(crate) fn inhalt_text(
    w: &str,
    attr: &dyn Fn(&str) -> Option<String>,
    erzeugt: &mut dyn FnMut(Erzeugt) -> Option<String>,
) -> Option<String> {
    let z: Vec<char> = w.chars().take(1024).collect();
    let mut raus = String::new();
    let mut i = 0usize;
    while i < z.len() {
        let c = z[i];
        if c == '"' || c == '\'' {
            let (t, weiter) = zeichenkette_lesen(&z, i);
            raus.push_str(&t);
            i = weiter;
            continue;
        }
        if c == '/' {
            break;
        }
        let rest: String = z[i..].iter().take(16).collect::<String>().to_ascii_lowercase();
        let davor_wort = i > 0 && (z[i - 1].is_alphanumeric() || z[i - 1] == '-');
        if !davor_wort && (rest.starts_with("counter(") || rest.starts_with("counters(")) {
            let mehrere = rest.starts_with("counters(");
            let anfang = i + if mehrere { 9 } else { 8 };
            let mut ende = anfang;
            let mut in_kette: Option<char> = None;
            while ende < z.len() {
                match (in_kette, z[ende]) {
                    (Some(q), ch) if ch == q => in_kette = None,
                    (Some(_), '\\') => ende += 1,
                    (Some(_), _) => {}
                    (None, '"' | '\'') => in_kette = Some(z[ende]),
                    (None, ')') => break,
                    _ => {}
                }
                ende += 1;
            }
            let innen: String = z[anfang.min(z.len())..ende.min(z.len())].iter().collect();
            let teile = argumente_teilen(&innen);
            let name = teile.first().map(|t| t.trim().to_string()).unwrap_or_default();
            let (trenner, stil) = if mehrere {
                let k: Vec<char> = teile.get(1).map(|t| t.trim().chars().collect()).unwrap_or_default();
                let trenner = if k.first().is_some_and(|c| *c == '"' || *c == '\'') {
                    zeichenkette_lesen(&k, 0).0
                } else {
                    String::new()
                };
                (Some(trenner), teile.get(2))
            } else {
                (None, teile.get(1))
            };
            let stil = stil.map(|s| s.trim().to_string()).unwrap_or_else(|| "decimal".to_string());
            raus.push_str(&erzeugt(Erzeugt::Zaehler { name, trenner, stil })?);
            i = ende + 1;
            continue;
        }
        if !davor_wort {
            let zitat = [
                ("open-quote", Zitat::Auf),
                ("close-quote", Zitat::Zu),
                ("no-open-quote", Zitat::KeinAuf),
                ("no-close-quote", Zitat::KeinZu),
            ]
            .into_iter()
            .find(|(wort, _)| {
                rest.starts_with(wort)
                    && z.get(i + wort.len()).is_none_or(|ch| !(ch.is_alphanumeric() || *ch == '-'))
            });
            if let Some((wort, art)) = zitat {
                raus.push_str(&erzeugt(Erzeugt::Zitat(art))?);
                i += wort.len();
                continue;
            }
        }
        if rest.starts_with("url(") || rest.starts_with("image-set") {
            return None;
        }
        if rest.starts_with("attr(") {
            // Release 11.10.2026: der Ersatzwert hinter dem Komma (CSS Values 5
            // §7.7) gilt, wenn das Attribut fehlt — eine Zeichenkette als Text,
            // sonst nichts. GEMESSEN vorher (WPT `attr-notype-fallback`): fehlte.
            let mut ende = i + 5;
            let mut in_kette: Option<char> = None;
            while ende < z.len() {
                match (in_kette, z[ende]) {
                    (Some(q), ch) if ch == q => in_kette = None,
                    (Some(_), '\\') => ende += 1,
                    (Some(_), _) => {}
                    (None, '"' | '\'') => in_kette = Some(z[ende]),
                    (None, ')') => break,
                    _ => {}
                }
                ende += 1;
            }
            let ende = ende.min(z.len());
            let innen: String = z[(i + 5).min(ende)..ende].iter().collect();
            let teile = argumente_teilen(&innen);
            let name = teile.first().and_then(|t| t.split_whitespace().next()).unwrap_or("");
            match attr(name) {
                Some(v) => raus.push_str(&v),
                None => {
                    let ersatz: Vec<char> = teile.get(1).map(|t| t.trim().chars().collect()).unwrap_or_default();
                    if ersatz.first().is_some_and(|c| *c == '"' || *c == '\'') {
                        raus.push_str(&zeichenkette_lesen(&ersatz, 0).0);
                    }
                }
            }
            i = ende + 1;
            continue;
        }
        i += 1;
    }
    let raus: String = raus.chars().filter(|ch| !matches!(ch, '\u{200b}' | '\u{feff}' | '\u{200c}' | '\u{200d}')).collect();
    let privat = raus.chars().any(|ch| matches!(ch as u32, 0xE000..=0xF8FF | 0xF0000..=0x10FFFF));
    (!privat && !raus.trim().is_empty()).then_some(raus)
}

/// Teilt Funktionsargumente an den Kommas der obersten Ebene.
fn argumente_teilen(innen: &str) -> Vec<String> {
    let mut teile = vec![String::new()];
    let mut in_kette: Option<char> = None;
    let mut flucht = false;
    for ch in innen.chars() {
        if let Some(q) = in_kette {
            if flucht {
                flucht = false;
            } else if ch == '\\' {
                flucht = true;
            } else if ch == q {
                in_kette = None;
            }
        } else if ch == '"' || ch == '\'' {
            in_kette = Some(ch);
        } else if ch == ',' {
            if teile.len() >= 16 {
                break;
            }
            teile.push(String::new());
            continue;
        }
        if let Some(t) = teile.last_mut() {
            t.push(ch);
        }
    }
    teile
}

/// Sammelt die `::before`/`::after`-Regeln aus den Autorenblaettern.
///
/// NUR `Regel::Stil` (oberste Ebene): Regeln in `@media`/`@supports`
/// waeren an Bedingungen gebunden, die hier nicht geprueft werden — eine
/// Telefonregel auf dem Schreibtisch waere schlimmer als eine fehlende.
/// Sobald der Pseudo-Weg erprobt ist, kann die Media-Auswertung
/// nachgezogen werden (dieselbe Bedingung wie in der Kaskade).
/// Release 10.10.2026: `NARSIL_DBG_PSEUDO` EINMAL gelesen. GEMESSEN vorher
/// (Stichproben mit cdb, spiegel.de, 15 603 Knoten): `std::env::var_os` lief
/// je Element und je Pseudo-Kasten — unter Windows ein Umweg ueber UTF-16
/// mit Allokation (`RtlQueryEnvironmentVariable` in den heissen Stapeln).
pub(crate) fn dbg_pseudo_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("NARSIL_DBG_PSEUDO").is_some())
}

/// Release 10.10.2026: `NARSIL_KASKADE_ZEITEN` EINMAL gelesen (siehe
/// `dbg_pseudo_an`) — die Abfrage stand je Element in der Stilrechnung.
pub(crate) fn kaskade_zeiten_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("NARSIL_KASKADE_ZEITEN").is_some())
}

pub(crate) fn pseudo_regeln(blaetter: &[narsil_css::Stylesheet]) -> Vec<PseudoRegel> {
    if std::env::var_os("NARSIL_PSEUDO_AUS").is_some() {
        return Vec::new();
    }
    let mut raus: Vec<PseudoRegel> = Vec::new();
    // Release 11.10.2026: die UA-Regeln fuer `<q>` (HTML §15.3.4) zuerst —
    // jede Autorenregel kommt danach und gewinnt bei gleichem Rang.
    static UA_PSEUDO: std::sync::OnceLock<narsil_css::Stylesheet> = std::sync::OnceLock::new();
    let ua = UA_PSEUDO.get_or_init(|| {
        narsil_css::stylesheet_lesen("q::before { content: open-quote } q::after { content: close-quote }")
    });
    for blatt in std::iter::once(ua).chain(blaetter) {
        for regel in &blatt.regeln {
            let r = match regel {
                narsil_css::Regel::Stil(r) => r,
                _ => continue,
            };
            for sel in &r.selektoren {
                let Some(letzt) = sel.kette.last() else {
                    continue;
                };
                let nachher = letzt
                    .einfach
                    .pseudos
                    .iter()
                    .any(|p| matches!(p, narsil_css::Pseudo::Nachher));
                let vorher = letzt
                    .einfach
                    .pseudos
                    .iter()
                    .any(|p| matches!(p, narsil_css::Pseudo::Vorher));
                let platzhalter = letzt
                    .einfach
                    .pseudos
                    .iter()
                    .any(|p| matches!(p, narsil_css::Pseudo::Platzhalter));
                let erster_buchstabe = letzt
                    .einfach
                    .pseudos
                    .iter()
                    .any(|p| matches!(p, narsil_css::Pseudo::ErsterBuchstabe));
                let erste_zeile = letzt.einfach.pseudos.iter().any(|p| matches!(p, narsil_css::Pseudo::ErsteZeile));
                if !nachher && !vorher && !platzhalter && !erster_buchstabe && !erste_zeile {
                    continue;
                }
                // Release 11.10.2026: hoechstens EIN Pseudoelement (Selectors 4
                // §3.6.3) — `div:before:after` ist ungueltig. GEMESSEN vorher
                // (WPT `before-after-001`): es galt als `::after`.
                if [nachher, vorher, platzhalter, erster_buchstabe, erste_zeile].iter().filter(|b| **b).count() > 1 {
                    continue;
                }
                let mut ohne = sel.clone();
                if let Some(l) = ohne.kette.last_mut() {
                    l.einfach.pseudos.retain(|p| {
                        !matches!(
                            p,
                            narsil_css::Pseudo::Vorher
                                | narsil_css::Pseudo::Nachher
                                | narsil_css::Pseudo::Platzhalter
                                | narsil_css::Pseudo::ErsterBuchstabe
                                | narsil_css::Pseudo::ErsteZeile
                        )
                    });
                }
                let hat_inhalt = r.deklarationen.iter().any(|d| {
                    matches!(&d.wert, narsil_css::Wert::Schluessel(n, _) if n == "content")
                });
                raus.push(PseudoRegel {
                    selektor: ohne,
                    nachher,
                    platzhalter,
                    erster_buchstabe,
                    erste_zeile,
                    deklarationen: r.deklarationen.clone(),
                    hat_inhalt,
                });
                if dbg_pseudo_an() {
                    eprintln!(
                        "PSEUDO-REGEL nachher={nachher} nr={} dekl={}",
                        raus.len(),
                        r.deklarationen.len()
                    );
                }
            }
        }
    }
    raus
}

/// Release 10.10.2026: Farbe und Deckkraft von `::placeholder` eines Feldes
/// (CSS Pseudo-Elements 4 §4.3; das Pseudo erbt vom Feld). Ohne passende
/// Regel gilt das UA-Blatt: `#757575` (Chrome, Firefox), deckend.
/// GEMESSEN vorher: der Platzhalter stand in der Textfarbe des Feldes, und
/// `::placeholder { opacity: 0 }` (schwebende Labels: github.com) wirkte
/// nicht — Label und Platzhalter lagen uebereinander.
pub(crate) fn platzhalter_farbe(
    baum: &CssBaum,
    id: u32,
    satz: &PseudoSatz,
    eltern: &narsil_css::Berechnet,
    breite: u32,
    hoehe: u32,
) -> (Color, f32) {
    const UA_GRAU: Color = Color { r: 0x75, g: 0x75, b: 0x75, a: 255 };
    let el = El { baum, id };
    let mut blatt_regeln: Vec<narsil_css::Regel> = Vec::new();
    if !satz.regeln.is_empty() {
        for r in satz
            .vormerker
            .kandidaten_nummern(&el)
            .iter()
            .filter_map(|nr| satz.vormerker.regel_von(*nr))
            .filter_map(|(_, ri)| satz.regeln.get(ri))
        {
            if r.platzhalter
                && !r.erster_buchstabe
                && !r.erste_zeile
                && !r.selektor.kette.is_empty()
                && narsil_css::selektor_passt_mit(&r.selektor, &el, &baum.zustand)
            {
                blatt_regeln.push(narsil_css::Regel::Stil(narsil_css::StilRegel {
                    selektoren: vec![r.selektor.clone()],
                    deklarationen: r.deklarationen.clone(),
                    variablen: Vec::new(),
                    zusatz_medien: Vec::new(),
                }));
            }
        }
    }
    if blatt_regeln.is_empty() {
        return (UA_GRAU, 1.0);
    }
    // Das UA-Grau als ERBE: setzt eine Regel `color`, gewinnt sie; sonst
    // bleibt das Grau (wie die UA-Regel `input::placeholder`).
    let mut basis = eltern.clone();
    basis.farbe = narsil_css::Farbe { r: UA_GRAU.r, g: UA_GRAU.g, b: UA_GRAU.b, a: UA_GRAU.a };
    let blatt = narsil_css::Stylesheet { regeln: blatt_regeln, verworfen: 0 };
    let vormerker = narsil_css::Vormerker::bauen(std::slice::from_ref(&blatt));
    let autor = narsil_css::berechnen_mit_vormerker(
        &vormerker,
        std::slice::from_ref(&blatt),
        &el,
        &[],
        &[],
        Some(&basis),
        breite,
        hoehe,
        &baum.zustand,
    );
    (farbe(autor.farbe), autor.deckkraft)
}

/// Release 10.10.2026: die `::first-letter`-Regeln, die auf das Element
/// `id` passen (in Regelfolge). Leer = das Element hat keinen ersten
/// Buchstaben mit eigenem Stil.
pub(crate) fn erster_buchstabe_regeln(baum: &CssBaum, id: u32, satz: &PseudoSatz) -> Vec<narsil_css::Regel> {
    if satz.regeln.is_empty() || !satz.regeln.iter().any(|r| r.erster_buchstabe) {
        return Vec::new();
    }
    let el = El { baum, id };
    satz.vormerker
        .kandidaten_nummern(&el)
        .iter()
        .filter_map(|nr| satz.vormerker.regel_von(*nr))
        .filter_map(|(_, ri)| satz.regeln.get(ri))
        .filter(|r| {
            r.erster_buchstabe
                && !r.selektor.kette.is_empty()
                && narsil_css::selektor_passt_mit(&r.selektor, &el, &baum.zustand)
        })
        .map(|r| {
            narsil_css::Regel::Stil(narsil_css::StilRegel {
                selektoren: vec![r.selektor.clone()],
                deklarationen: r.deklarationen.clone(),
                variablen: Vec::new(),
                zusatz_medien: Vec::new(),
            })
        })
        .collect()
}

/// Release 15.10.2026 (Agent CSS2): does the box balance its lines (`text-
/// wrap: balance` or `text-wrap-style: balance`, the nearer declaration
/// winning; CSS Text 4 §6.2)?
pub(crate) fn text_wrap_balance(s: &narsil_css::Berechnet) -> bool {
    s.schluessel
        .iter()
        .rev()
        .find(|(n, _)| n == "text-wrap-style" || n == "text-wrap")
        .is_some_and(|(_, w)| w.split_whitespace().any(|t| t.eq_ignore_ascii_case("balance")))
}

/// Release 15.10.2026 (Agent CSS2): the `::first-line` rules matching `id`
/// (in rule order); empty = none.
pub(crate) fn erste_zeile_regeln(baum: &CssBaum, id: u32, satz: &PseudoSatz) -> Vec<narsil_css::Regel> {
    if satz.regeln.is_empty() || !satz.regeln.iter().any(|r| r.erste_zeile) {
        return Vec::new();
    }
    let el = El { baum, id };
    satz.vormerker
        .kandidaten_nummern(&el)
        .iter()
        .filter_map(|nr| satz.vormerker.regel_von(*nr))
        .filter_map(|(_, ri)| satz.regeln.get(ri))
        .filter(|r| {
            r.erste_zeile && !r.selektor.kette.is_empty() && narsil_css::selektor_passt_mit(&r.selektor, &el, &baum.zustand)
        })
        .map(|r| {
            narsil_css::Regel::Stil(narsil_css::StilRegel {
                selektoren: vec![r.selektor.clone()],
                deklarationen: r.deklarationen.clone(),
                variablen: Vec::new(),
                zusatz_medien: Vec::new(),
            })
        })
        .collect()
}

/// Release 10.10.2026: der berechnete Stil des `::first-letter` von `id`.
/// `eltern` ist der Stil des Elements, in dem der Buchstabe steht (CSS
/// Pseudo-Elements 4 §2.5: das Pseudo steht als innerstes Element in der
/// erdachten Tag-Folge, es erbt also vom innersten Vorfahren des Textes).
pub(crate) fn erster_buchstabe_stil(
    baum: &CssBaum,
    id: u32,
    regeln: &[narsil_css::Regel],
    eltern: &narsil_css::Berechnet,
    breite: u32,
    hoehe: u32,
) -> narsil_css::Berechnet {
    let el = El { baum, id };
    let blatt = narsil_css::Stylesheet { regeln: regeln.to_vec(), verworfen: 0 };
    let vormerker = narsil_css::Vormerker::bauen(std::slice::from_ref(&blatt));
    narsil_css::berechnen_mit_vormerker(
        &vormerker,
        std::slice::from_ref(&blatt),
        &el,
        &[],
        &[],
        Some(eltern),
        breite,
        hoehe,
        &baum.zustand,
    )
}

/// Release 10.10.2026: Zerlegt Text fuer `::first-letter` in (Leerraum
/// davor, ersten Buchstaben, Rest). Der erste Buchstabe ist das erste
/// Zeichen, das weder Leerraum noch Satzzeichen ist, samt folgender
/// Kombinationszeichen, mit allen Satzzeichen (Ps, Pe, Pi, Pf, Po) direkt
/// davor und danach (CSS Pseudo-Elements 4 §2.3.1). `None`: nur
/// zusammenfallender Leerraum (weiter warten). Ein leerer Buchstabe heisst:
/// Inhalt ohne Buchstaben davor — etwa ein geschuetztes Leerzeichen (WPT
/// `first-letter-space-not-selected`) —, dann gibt es keinen.
pub fn erster_buchstabe_teilen(t: &str) -> Option<(&str, &str, &str)> {
    let start = t
        .char_indices()
        .find(|(_, c)| !matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000c}'))
        .map(|(i, _)| i)?;
    let mut zeichen = t[start..].char_indices().map(|(j, c)| (start + j, c)).peekable();
    while zeichen.next_if(|(_, c)| ist_satzzeichen(*c)).is_some() {}
    if zeichen.next_if(|(_, c)| !c.is_whitespace() && !c.is_control()).is_none() {
        return Some((&t[..start], "", &t[start..]));
    }
    while zeichen.next_if(|(_, c)| ist_kombinierend(*c)).is_some() {}
    while zeichen.next_if(|(_, c)| ist_satzzeichen(*c)).is_some() {}
    let ende = zeichen.peek().map_or(t.len(), |(j, _)| *j);
    Some((&t[..start], &t[start..ende], &t[ende..]))
}

fn ist_kombinierend(c: char) -> bool {
    matches!(c as u32, 0x300..=0x36F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Unicode-Satzzeichen der Klassen Ps, Pe, Pi, Pf, Po (ohne Pd und Pc).
pub(crate) fn ist_satzzeichen(c: char) -> bool {
    let n = c as u32;
    SATZZEICHEN
        .binary_search_by(|&(a, b)| {
            if b < n {
                std::cmp::Ordering::Less
            } else if a > n {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Erzeugt aus den Unicode-Tabellen von `narsil-jsvm` (Kategorien Ps, Pe,
/// Pi, Pf, Po, zusammengefasst und sortiert).
const SATZZEICHEN: &[(u32, u32)] = &[
    (0x21, 0x23), (0x25, 0x2A), (0x2C, 0x2C), (0x2E, 0x2F), (0x3A, 0x3B), (0x3F, 0x40),
    (0x5B, 0x5D), (0x7B, 0x7B), (0x7D, 0x7D), (0xA1, 0xA1), (0xA7, 0xA7), (0xAB, 0xAB),
    (0xB6, 0xB7), (0xBB, 0xBB), (0xBF, 0xBF), (0x37E, 0x37E), (0x387, 0x387), (0x55A, 0x55F),
    (0x589, 0x589), (0x5C0, 0x5C0), (0x5C3, 0x5C3), (0x5C6, 0x5C6), (0x5F3, 0x5F4),
    (0x609, 0x60A), (0x60C, 0x60D), (0x61B, 0x61B), (0x61D, 0x61F), (0x66A, 0x66D),
    (0x6D4, 0x6D4), (0x700, 0x70D), (0x7F7, 0x7F9), (0x830, 0x83E), (0x85E, 0x85E),
    (0x964, 0x965), (0x970, 0x970), (0x9FD, 0x9FD), (0xA76, 0xA76), (0xAF0, 0xAF0),
    (0xC77, 0xC77), (0xC84, 0xC84), (0xDF4, 0xDF4), (0xE4F, 0xE4F), (0xE5A, 0xE5B),
    (0xF04, 0xF12), (0xF14, 0xF14), (0xF3A, 0xF3D), (0xF85, 0xF85), (0xFD0, 0xFD4),
    (0xFD9, 0xFDA), (0x104A, 0x104F), (0x10FB, 0x10FB), (0x1360, 0x1368), (0x166E, 0x166E),
    (0x169B, 0x169C), (0x16EB, 0x16ED), (0x1735, 0x1736), (0x17D4, 0x17D6), (0x17D8, 0x17DA),
    (0x1800, 0x1805), (0x1807, 0x180A), (0x1944, 0x1945), (0x1A1E, 0x1A1F), (0x1AA0, 0x1AA6),
    (0x1AA8, 0x1AAD), (0x1B5A, 0x1B60), (0x1B7D, 0x1B7E), (0x1BFC, 0x1BFF), (0x1C3B, 0x1C3F),
    (0x1C7E, 0x1C7F), (0x1CC0, 0x1CC7), (0x1CD3, 0x1CD3), (0x2016, 0x2027), (0x2030, 0x203E),
    (0x2041, 0x2043), (0x2045, 0x2051), (0x2053, 0x2053), (0x2055, 0x205E), (0x207D, 0x207E),
    (0x208D, 0x208E), (0x2308, 0x230B), (0x2329, 0x232A), (0x2768, 0x2775), (0x27C5, 0x27C6),
    (0x27E6, 0x27EF), (0x2983, 0x2998), (0x29D8, 0x29DB), (0x29FC, 0x29FD), (0x2CF9, 0x2CFC),
    (0x2CFE, 0x2CFF), (0x2D70, 0x2D70), (0x2E00, 0x2E16), (0x2E18, 0x2E19), (0x2E1B, 0x2E2E),
    (0x2E30, 0x2E39), (0x2E3C, 0x2E3F), (0x2E41, 0x2E4F), (0x2E52, 0x2E5C), (0x3001, 0x3003),
    (0x3008, 0x3011), (0x3014, 0x301B), (0x301D, 0x301F), (0x303D, 0x303D), (0x30FB, 0x30FB),
    (0xA4FE, 0xA4FF), (0xA60D, 0xA60F), (0xA673, 0xA673), (0xA67E, 0xA67E), (0xA6F2, 0xA6F7),
    (0xA874, 0xA877), (0xA8CE, 0xA8CF), (0xA8F8, 0xA8FA), (0xA8FC, 0xA8FC), (0xA92E, 0xA92F),
    (0xA95F, 0xA95F), (0xA9C1, 0xA9CD), (0xA9DE, 0xA9DF), (0xAA5C, 0xAA5F), (0xAADE, 0xAADF),
    (0xAAF0, 0xAAF1), (0xABEB, 0xABEB), (0xFD3E, 0xFD3F), (0xFE10, 0xFE19), (0xFE30, 0xFE30),
    (0xFE35, 0xFE4C), (0xFE50, 0xFE52), (0xFE54, 0xFE57), (0xFE59, 0xFE61), (0xFE68, 0xFE68),
    (0xFE6A, 0xFE6B), (0xFF01, 0xFF03), (0xFF05, 0xFF0A), (0xFF0C, 0xFF0C), (0xFF0E, 0xFF0F),
    (0xFF1A, 0xFF1B), (0xFF1F, 0xFF20), (0xFF3B, 0xFF3D), (0xFF5B, 0xFF5B), (0xFF5D, 0xFF5D),
    (0xFF5F, 0xFF65), (0x10100, 0x10102), (0x1039F, 0x1039F), (0x103D0, 0x103D0),
    (0x1056F, 0x1056F), (0x10857, 0x10857), (0x1091F, 0x1091F), (0x1093F, 0x1093F),
    (0x10A50, 0x10A58), (0x10A7F, 0x10A7F), (0x10AF0, 0x10AF6), (0x10B39, 0x10B3F),
    (0x10B99, 0x10B9C), (0x10F55, 0x10F59), (0x10F86, 0x10F89), (0x11047, 0x1104D),
    (0x110BB, 0x110BC), (0x110BE, 0x110C1), (0x11140, 0x11143), (0x11174, 0x11175),
    (0x111C5, 0x111C8), (0x111CD, 0x111CD), (0x111DB, 0x111DB), (0x111DD, 0x111DF),
    (0x11238, 0x1123D), (0x112A9, 0x112A9), (0x1144B, 0x1144F), (0x1145A, 0x1145B),
    (0x1145D, 0x1145D), (0x114C6, 0x114C6), (0x115C1, 0x115D7), (0x11641, 0x11643),
    (0x11660, 0x1166C), (0x116B9, 0x116B9), (0x1173C, 0x1173E), (0x1183B, 0x1183B),
    (0x11944, 0x11946), (0x119E2, 0x119E2), (0x11A3F, 0x11A46), (0x11A9A, 0x11A9C),
    (0x11A9E, 0x11AA2), (0x11B00, 0x11B09), (0x11C41, 0x11C45), (0x11C70, 0x11C71),
    (0x11EF7, 0x11EF8), (0x11F43, 0x11F4F), (0x11FFF, 0x11FFF), (0x12470, 0x12474),
    (0x12FF1, 0x12FF2), (0x16A6E, 0x16A6F), (0x16AF5, 0x16AF5), (0x16B37, 0x16B3B),
    (0x16B44, 0x16B44), (0x16E97, 0x16E9A), (0x16FE2, 0x16FE2), (0x1BC9F, 0x1BC9F),
    (0x1DA87, 0x1DA8B), (0x1E95E, 0x1E95F),
];

/// Den Stil EINES Pseudoelements rechnen.
///
/// DERSELBE WEG WIE FUER EIN ELEMENT: aus den passenden Regeln wird ein
/// eigenes Blatt gebaut und ueber `berechnen_mit_vormerker` gerechnet —
/// damit gelten Laengen, Farben, Verlaeufe und `var()` genau wie sonst.
/// `eltern` ist der BERECHNETE Stil des Elements; Pseudoelemente erben
/// von ihm (CSS Pseudo-Elements 4, §3).
pub(crate) fn pseudo_kasten(
    baum: &CssBaum,
    id: u32,
    nachher: bool,
    satz: &PseudoSatz,
    eltern: &narsil_css::Berechnet,
    breite: u32,
    hoehe: u32,
    basis: &str,
) -> Option<PseudoKasten> {
    let el = El { baum, id };
    // Release 08.10.2026: ERSETZTE Elemente haben kein `::before`/`::after`
    // (CSS Pseudo-Elements 4 §2: die Pseudos stehen IM Inhalt des
    // Elements, und ein ersetztes Element hat keinen). GEMESSEN vorher:
    // spiegel.de, jedes `<img width=872 height=581>` bekam zwei
    // 872x581-Kaesten (Tailwinds `*,::before,::after`-Regel), und unter
    // jedem Teaser klaffte eine bildhohe Luecke.
    if baum.knoten.get(id as usize).is_some_and(|k| {
        matches!(
            k.tag.as_str(),
            "img" | "input" | "textarea" | "select" | "video" | "audio" | "iframe" | "canvas"
                | "embed" | "object" | "br" | "wbr"
        )
    }) {
        return None;
    }
    let mut blatt_regeln: Vec<narsil_css::Regel> = Vec::new();
    if satz.regeln.is_empty() {
        return None;
    }
    // Release 10.10.2026: nur die Kandidaten aus dem Index, in Regelfolge.
    let kandidaten = satz.vormerker.kandidaten_nummern(&el);
    // Release 10.10.2026: ein Pseudo-Kasten entsteht NUR, wenn eine
    // passende Regel `content` setzt (CSS Pseudo 4 §2: ohne `content` ist es
    // `normal` = kein Kasten). GEMESSEN vorher (Stichproben, spiegel.de):
    // `*, ::before, ::after { box-sizing: border-box }` machte JEDES Element
    // zum Treffer, und je Element und Pseudo wurden Blatt, Index und eine
    // volle Stilrechnung gebaut — nur um `content` nicht zu finden.
    let mit_inhalt = kandidaten
        .iter()
        .filter_map(|nr| satz.vormerker.regel_von(*nr))
        .filter_map(|(_, ri)| satz.regeln.get(ri))
        .any(|r| {
            r.nachher == nachher
                && !r.platzhalter
                && !r.erster_buchstabe
                && !r.erste_zeile
                && r.hat_inhalt
                && !r.selektor.kette.is_empty()
                && narsil_css::selektor_passt_mit(&r.selektor, &el, &baum.zustand)
        });
    if !mit_inhalt {
        return None;
    }
    for r in kandidaten
        .iter()
        .filter_map(|nr| satz.vormerker.regel_von(*nr))
        .filter_map(|(_, ri)| satz.regeln.get(ri))
    {
        if r.nachher != nachher || r.platzhalter || r.erster_buchstabe || r.selektor.kette.is_empty() {
            continue;
        }
        if narsil_css::selektor_passt_mit(&r.selektor, &el, &baum.zustand) {
            if dbg_pseudo_an() {
                eprintln!(
                    "PSEUDO-TREFFER nachher={nachher} dekl={:?}",
                    r.deklarationen
                        .iter()
                        .map(|d| format!("{:?}={}", d.eigenschaft, d.roh))
                        .collect::<Vec<_>>()
                );
            }
            blatt_regeln.push(narsil_css::Regel::Stil(narsil_css::StilRegel {
                selektoren: vec![r.selektor.clone()],
                deklarationen: r.deklarationen.clone(),
                variablen: Vec::new(),
                zusatz_medien: Vec::new(),
            }));
        }
    }
    if blatt_regeln.is_empty() {
        return None;
    }
    // DIE REIHENFOLGE bleibt die der Blaetter (die Liste wurde so
    // gesammelt) — die spaetere Regel gewinnt bei gleicher Spezifitaet,
    // wie in der Kaskade.
    let blatt = narsil_css::Stylesheet {
        regeln: blatt_regeln,
        verworfen: 0,
    };
    let vormerker = narsil_css::Vormerker::bauen(std::slice::from_ref(&blatt));
    let autor = narsil_css::berechnen_mit_vormerker(
        &vormerker,
        std::slice::from_ref(&blatt),
        &el,
        &[],
        &[],
        Some(eltern),
        breite,
        hoehe,
        &baum.zustand,
    );
    // Release 08.10.2026: OHNE `content` KEIN KASTEN (CSS 2.1 §12.2:
    // `normal` heisst fuer `::before`/`::after` `none`, und `none` erzeugt
    // nichts). GEMESSEN vorher: spiegel.de bekam 25 642 leere Pseudokaesten
    // aus `*,::before,::after{box-sizing:border-box;border:0 solid}`.
    let inhalt = autor.schluessel.iter().find(|(n, _)| n == "content").map(|(_, w)| w.trim());
    if dbg_pseudo_an() {
        eprintln!(
            "PSEUDO-AUTOR breite={:?} hoehe={:?} bild={:?} anzeige={:?} inhalt={inhalt:?}",
            autor.breite, autor.hoehe, autor.bild, autor.anzeige
        );
    }
    match inhalt {
        None => return None,
        Some(w) if w.eq_ignore_ascii_case("none") || w.eq_ignore_ascii_case("normal") => return None,
        _ => {}
    }
    // Die Hand-Typografie: ein Pseudokasten bringt seine Groesse und Farbe
    // selbst mit; die Schrift des Elements waere genauer, aber `Style`
    // kennt hier keinen Weg dorthin. Fuer Symbole (Breite/Hoehe/Bild) ist
    // das ohne Wirkung.
    // Release 11.10.2026: die Farbe ist die des Elements (geerbt) —
    // `stil_von_css` schreibt nur eine ABWEICHENDE. GEMESSEN vorher: der
    // Pseudo-Text ohne eigenes `color` war weiss (`Style::default`).
    let hand = Style {
        deckkraft: 1.0,
        radius: [0.0; 4],
        schatten_anzahl: 0,
        dekoration: 0,
        color: farbe(eltern.farbe),
        ..Style::default()
    };
    let mut stil = stil_von_css(hand, &autor, &autor, Some(eltern), false)?;
    // DIE GROESSE GEHOERT HIER HIN — `stil_von_css` schreibt `breite`/
    // `hoehe` NICHT in den DOM-Stil; das tut `narsil-page/src/lib.rs` in
    // `stil_von_kaskade` (der Block bei `st.size.width`). GEMESSEN ohne
    // diese zwei Zeilen: die Wikipedia-Kugel stand mit 50 px Hoehe und
    // **0 px Breite** im Layout (die Breite kommt aus `stil.size`, siehe
    // `box.rs::explicit_width`), und das Bild wurde nicht gezeichnet.
    {
        use narsil_core::length::Length as L;
        // Release 15.10.2026 (Agent CSS2): `calc()` with a percentage stays a
        // mixture (`Length::Gemischt`), as on elements — it resolves against
        // the containing block, not the viewport. MEASURED before (heise.de,
        // `a-scroll-container[scroll-shadow] .scroll-container::before {
        // position: absolute; height: calc(50% * 2) }`): the fades were as
        // tall as the viewport is wide and ran down over the page.
        let misch = autor.mischung.as_deref().copied().unwrap_or_default();
        stil.size.width = match (misch.breite, autor.breite_prozent, autor.breite) {
            (Some((a, p)), _, _) => L::gemischt(a, p),
            (None, Some(p), _) => L::Percent(p),
            (None, None, Some(v)) if v.is_finite() && v >= 0.0 => L::Px(v),
            _ => stil.size.width,
        };
        stil.size.height = match (misch.hoehe, autor.hoehe_prozent, autor.hoehe) {
            (Some((a, p)), _, _) => L::gemischt(a, p),
            (None, Some(p), _) => L::Percent(p),
            (None, None, Some(v)) if v.is_finite() && v >= 0.0 => L::Px(v),
            _ => stil.size.height,
        };
    }
    // Release 15.10.2026 (Agent F): the background image and the mask image
    // are separate: the mask masks the box (see `Seite::maskenbilder`).
    let bild = match &autor.bild {
        narsil_css::BildQuelle::Url(url) => Some((crate::loese_link(url, basis), crate::bild_geo_mit_rand(autor.bild_geo, &autor))),
        _ => None,
    };
    let maske = Some(crate::masken_ebenen(&autor.masken, basis, None)).filter(|e| !e.is_empty());
    let plan = narsil_css::layout_stil(&autor);
    let inline = matches!(autor.anzeige, narsil_css::Anzeige::Inline | narsil_css::Anzeige::InlineBlock);
    // Der ROHE Wert der letzten passenden `content`-Angabe (die Kaskade
    // fuehrt Schluesselwerte kleingeschrieben — „Neu" waere „neu"); mit
    // `var()` der aufgeloeste Wert.
    let roh = blatt.regeln.iter().rev().find_map(|r| match r {
        narsil_css::Regel::Stil(st) => st.deklarationen.iter().rev().find_map(|d| match &d.wert {
            narsil_css::Wert::Schluessel(n, _) if n == "content" => Some(d.roh.as_str()),
            _ => None,
        }),
        _ => None,
    });
    let quelle = match (roh, inhalt) {
        (Some(r), _) if !r.contains("var(") => Some(r.to_string()),
        (_, Some(w)) => Some(w.to_string()),
        _ => None,
    };
    // Release 11.10.2026: Zaehler und Anfuehrungszeichen kennt erst der Bau
    // (Dokumentreihenfolge) — dann bleibt der rohe Wert fuer ihn stehen.
    let mut erzeugt = false;
    let text = quelle.as_deref().and_then(|q| {
        inhalt_text(q, &|name| el.attribut(name).map(str::to_string), &mut |_| {
            erzeugt = true;
            None
        })
    });
    let inhalt = if erzeugt { quelle } else { None };
    let zaehler = zaehler_angaben(&autor.schluessel);
    let zitate = zitate_angabe(&autor.schluessel);
    // Release 11.10.2026: die Aussenseite von `display` (CSS Display 3
    // §2.1). `table-column`/`table-column-group` erzeugen keinen Kasten,
    // der Inhalt zeigt (CSS 2.1 §17.2) — GEMESSEN vorher (WPT
    // `after-content-display-012`): der Text erschien.
    let anzeige_roh = blatt.regeln.iter().rev().find_map(|r| match r {
        narsil_css::Regel::Stil(st) => st
            .deklarationen
            .iter()
            .rev()
            .find(|d| d.eigenschaft == narsil_css::Eigenschaft::Anzeige)
            .map(|d| d.roh.trim().to_ascii_lowercase()),
        _ => None,
    });
    if matches!(anzeige_roh.as_deref(), Some("table-column" | "table-column-group")) {
        return None;
    }
    let blockebene = !inline
        && !autor.inline_aussen
        && !matches!(autor.anzeige, narsil_css::Anzeige::InlineFlex)
        && !anzeige_roh.as_deref().is_some_and(|a| a.starts_with("inline"));
    let balance = text_wrap_balance(&autor);
    Some(PseudoKasten { stil, plan, bild, maske, inline, text, inhalt, zaehler, zitate, blockebene, balance })
}
