//! narsil-page — eine Seite wird ein Narsil-Dokument.
//!
//! # WOFUER DIESE CRATE EXISTIERT
//!
//! `narsil-fmt-html` liest HTML und gibt `Vec<HtmlNode>` zurueck — eine
//! Baumstruktur, die KEINE unserer Knoten ist. Zwischen dort und dem Bild
//! fehlt die Uebersetzung, und genau die war vorher nirgends: `narsil-app`
//! nimmt eine `.narsil`-Datei, sonst nichts. Eine echte Webseite konnte
//! nicht geladen werden, egal wie gut der HTML-Parser ist.
//!
//! Diese Crate macht aus HTML (und Klartext) ein `narsil_dom::Document`,
//! das die bestehende Pipeline kennt: Layout, Display-Liste, Raster.
//! Sie aendert NICHTS an Layout, Rasterizer oder Parser.
//!
//! # DIE DREI ARBEITEN, und alle drei gab es vorher nicht
//!
//! 1. **Umbruch.** GEMESSEN am 29.09.: `narsil_layout_engine::text.rs:231`
//!    gibt die Gesamtbreite als EINE Zeile zurueck. Ein Absatz aus 93
//!    Zeichen auf 200 px Breite zeigte 14 Zeichen und den Rest gar nicht.
//!    Die Entscheidung E1 ("Version 0 clippt nicht") gilt fuer die
//!    SPRACHE — fuer eine Webseite ist sie der Unterschied zwischen
//!    "Seite lesbar" und "Seite unbrauchbar". Der Umbruch passiert
//!    deshalb HIER, in der Uebersetzung: der Absatz wird vor dem Layout
//!    in mehrere `text`-Knoten zerlegt, und die Layout-Engine bleibt so,
//!    wie sie ist und getestet ist.
//!
//! 2. **Links.** `<a href="...">` erzeugt einen Knoten mit einer Adresse.
//!    Die Sprache kennt keinen Zustand und keinen `onclick` — aber ein
//!    Browser, dem man nicht folgen kann, ist kein Browser (ADR: "Kann
//!    man klicken, um woanders hinzugehen"). Der Link wird hier als
//!    `NodeKind::Text` mit einem `Link` in der Link-Map abgelegt, und
//!    `narsil` fragt die Map ab. Die Sprache bleibt zustandslos.
//!
//! 3. **Typografie.** `<h1>` ist groesser als `<p>`, `<b>` ist fett
//!    (dickere Glyphen gibt es nicht — also heller), `<code>` ist
//!    monospace-nah. Ohne das sieht eine Webseite aus wie eine
//!    Fehlermeldung.
//!
//! # GRENZEN, benannt statt verschwiegen
//!
//! * **Kein CSS.** Keine Klassen, keine Selektoren, kein `<style>`.
//!   Nur ein paar `style`-Eigenschaften werden gelesen (siehe
//!   [`STIL_EIGENSCHAFTEN`]).
//! * **Medien als Platzhalter (nativ, ohne JS).** `<img>` wird zu
//!   `[Bild: alt]`, `<video>`/`audio` zu `[Video: src]`/`[Audio: src]`,
//!   (Release 11.10.2026: `iframe`/`embed` sind ersetzte Kaesten mit
//!   300 x 150, `object` zeigt seinen Ersatzinhalt, `canvas` ist ein Kasten.)
//!   Keine Wiedergabe in v0 — aber sichtbar statt unsichtbar. Das ist
//!   der Unterschied zwischen "Seite unbrauchbar" und "Seite lesbar".
//! * **Formulare als Text (nativ, ohne JS).** `<input>` wird zu
//!   `[Eingabe name: typ]`, `<textarea>` zu `[Textfeld name: ...]`,
//!   `<select>` zu `[Auswahl name: a | b]`, Submit-Buttons zu
//!   `[Knopf: ...]`. Absenden ohne Skript geht per nativem POST
//!   (`application/x-www-form-urlencoded`, siehe `narsil-server::api`).
//! * **Kein JavaScript.** Bewusst (ADR-037). Ein `<script>` wird
//!   verworfen, nicht ausgefuehrt.
//! * **Tabellen linearisiert.** Zelle, Zelle, Zeile — keine Gitter.
//!
//! # REGELN
//!
//! * Kein `unsafe`.
//! * Kein `unwrap` auf Benutzereingaben. Kaputtes HTML ist die Norm.

use std::collections::HashMap;

use narsil_core::color::Color;
use narsil_core::geometry::Rect;
use narsil_core::length::{Dimension, Length, Spacing};
use narsil_core::style::{Direction, Style, Textausrichtung, Weissraum, ZeilenhoeheArt};
use narsil_dom::{Document, NodeId, NodeKind, Span};

/// H3–H6: Tabellen, Formulare, Bilder, Zeichensaetze.
///
/// EIGENES Modul und nicht mehr in dieser Datei: diese Datei ist der
/// Umbruch von HTML in ein Dokument, und dort gibt es genau EINEN Weg
/// dafuer. Tabellen, Formulare und Bilder sind ZUSATZINFORMATIONEN ueber
/// denselben Baum — sie gehoeren an die Seite, nicht in den Umbruch.
/// Wer sie in `Bauer::element()` mischt, kann sie nicht mehr pruefen,
/// ohne einen Absatz zu bauen.
pub mod hteile;

/// I2: die CSS-Kaskade (`<style>`, `style=""`, UA-Stylesheet).
pub mod kaskade;

/// I5: Markdown (`.md`) wird eine Seite.
pub mod markdown;

pub use markdown::{ist_markdown, markdown_zu_html};

mod tabelle_anonym;
// Release 11.10.2026 (Agent CSS2): zusammenfallende Tabellenrahmen (CSS 2.1 §17.6.2).
mod tabelle_kanten;
pub use kaskade::{CssBaum, CssKnoten, El, baum_bauen, baum_bauen_mit_zustand, blaetter, erster_buchstabe_teilen};
//
// 04.10. 00:25: DER WIRTSZUSTAND fuer den Host.
//
// GEMESSENE BEGRUENDUNG, warum `Farbschema` hier exportiert wird: der Host
// (`narsil`) soll das Dunkel-Thema einschalten koennen, ohne eine zweite Crate
// (`narsil-css`) in seine Signatur zu schreiben — die Kette waere dann
// `narsil -> narsil-css`, und `narsil-page` waere nur ein Durchlaeufer fuer eine
// Zahl, die schon durch ihren eigenen Einstieg (`aus_html_mit_farbschema`)
// laeuft.
pub use narsil_css::Farbschema;

/// `<script>` und `on*`: im Baum behalten, nicht anzeigen.
pub mod skripte;
/// Release 12.10.2026: Shadow DOM — der flache Baum und die Nummern.
pub mod schatten;
/// Release 12.10.2026: die geholten externen Stylesheets je Adresse.
pub mod blattschrank;

/// Release 12.10.2026: `(x-Hoehe, Vorschub der „0")` in em der Schrift, die
/// eine Familienliste ergibt — fuer `ex`/`ch` in der Kaskade (siehe
/// `narsil_css::schrift_masse_anmelden`). Je Liste EINMAL gemessen (M3: hoechstens
/// 1024 Eintraege).
fn schrift_masse(familie: &str) -> Option<(f32, f32)> {
    static MERK: std::sync::Mutex<Option<HashMap<String, Option<(f32, f32)>>>> = std::sync::Mutex::new(None);
    if let Ok(g) = MERK.lock()
        && let Some(m) = g.as_ref()
        && let Some(v) = m.get(familie)
    {
        return *v;
    }
    let liste = if familie.trim().is_empty() { "serif" } else { familie };
    let wert = narsil_text::stapel_fuer_familie(liste, 400, false).and_then(|stapel| {
        let haupt = stapel.schrift(stapel.haupt_index()?)?;
        let ex = haupt.font.x_hoehe_em().unwrap_or(0.5);
        let ch = stapel.breite("0", 1.0);
        (ch > 0.0).then_some((ex, ch))
    });
    if let Ok(mut g) = MERK.lock() {
        let m = g.get_or_insert_with(HashMap::new);
        if m.len() < 1024 {
            m.insert(familie.to_string(), wert);
        }
    }
    wert
}
/// A-ADBLOCK (01.10.2026, 21:45): der Werbeblocker. Siehe das Modul.
pub mod werbung;

pub use skripte::{
    Ereignis, MAX_EREIGNIS_QUELLE, MAX_EREIGNISSE, MAX_SKRIPT_INHALT, MAX_SKRIPT_QUELLE,
    MAX_SKRIPTE, MAX_STILBLOECKE, MAX_STILTEXT, Skript, StyleBlock, ereignisse_aus,
    offene_blaetter, skript_aus, skripte_im_html, stil_aus_link, stile_aus,
};

// Die Namen von H3–H6 sindoeffentlich: `formulare`, `tabelle_knoten`,
// `bilder`, `Satz`, `zu_text`, `get_url`, `url_kodieren`. Ohne diese
// Zeile muesste jeder Aufrufer `narsil_page::hteile::formulare` schreiben
// — und die H-Kette in WELLE4.txt nennt die kurzen Namen.
pub use hteile::{
    BILD_STANDARD_HOEHE, Bild, Feld, FeldArt, Formular, MAX_ATTRIBUT_WERT, MAX_BILD_MASSE,
    MAX_BILDER, MAX_FORMULARFELDER, MAX_SPALTEN, MAX_URL_FELDER, MAX_URL_LAENGE, MAX_ZEILEN, Satz,
    Zeile, Zelle, bilder, formulare, get_url, spaltenzahl, stil_bild, stil_feld, stil_zelle,
    tabelle_knoten, url_kodieren, zu_text,
};
use narsil_fmt_html::{HtmlNode, parse_html};
use narsil_text::{BitmapFont, measure};

// ---------------------------------------------------------------------------
// Konstanten — jede mit einer Zahl, die gemessen ist
// ---------------------------------------------------------------------------

/// Seitenrand in Pixeln. Wie gross eine Webseite an ihrem Rand Luft hat.
///
/// **04.10.2026 — GEMESSEN und auf 0.0 gesetzt**, auf Entscheidung der
/// Leitung. Vorher stand hier 16.0, und der Wert wurde als `margin` auf den
/// **Dokument-Root** geschrieben (`wurzel()`), nicht auf den Rumpf.
///
/// GEMESSEN (1280 px Fenster, `<div style='background:#ff0000'>x</div>`):
///
///     | wer            | margin   | Kasten bei uns    | Firefox |
///     |----------------|----------|-------------------|---------|
///     | Wurzel `page`  | 16 px    | x=16  w=1248      | x=0 w=1280 |
///     | Rumpf `body`   | 8 px UA  | x=24  w=1232      | x=8 w=1264 |
///     | div rot        | 0        | x=24  w=1232      | x=8 w=1264 |
///
/// CSS 2.1, Anhang D (das UA-Blatt jedes Browsers) nennt fuer `html`
/// **keinen** `margin`; der einzige Vorgabewert ist `body { margin: 8px }` —
/// und der kam bei uns **schon immer** korrekt an (GEMESSEN: Rumpf-Margin
/// = 8.0, aus `narsil-css/src/ua.rs`). Wir haben ihn also **nicht doppelt**
/// gezaehlt; was dazukam, war unser eigener Rand auf der Wurzel.
///
/// **16 px Versatz auf jeder Seite, in beide Richtungen, auf jeder Seite des
/// Netzes.** Das ist der Grund, warum jede Zeile 16 px zu weit rechts und
/// 16 px zu weit unten stand.
///
/// WARUM ueberhaupt 16: der Kommentar in `wurzel()` (W7C-Befund vom
/// 02.10. 05:00, 143 Messungen) nennt eine echte Ueberlaufgefahr — die
/// Wurzel bekam `width = Fensterbreite` **und** einen Rand, endete also bei
/// `16 + breite`. Diese Ueberlaufgefahr ist seit `nutzbar =
/// breite - 2*SEITENRAND` (ebenfalls 02.10.) **nicht mehr vorhanden**, und mit
/// dem Rand ist auch der Rand ueberfluessig geworden: bei 0 ist
/// `nutzbar = breite`, die Seite passt wieder genau ins Fenster.
pub const SEITENRAND: f32 = 0.0;

/// Abstand zwischen zwei Absaetzen. Ohne ihn fliesst Text zusammen.
pub const ABSATZABSTAND: f32 = 12.0;

/// Schriftgrad eines Absatzes. Vorgabe des Standes (16 px).
pub const GRAD_TEXT: f32 = 16.0;

/// Release 10.10.2026: der groesste Schriftgrad, den der Seitenbau setzt
/// (M3). Vorher lag die Grenze bei 96 px — der Bereich der alten
/// Bitmapschrift; GEMESSEN (Web-Platform-Tests, `font: 100px Ahem`): ein
/// Text mit 97 px und mehr VERSCHWAND (die Zeile wurde uebersprungen),
/// jede grosse Schlagzeile einer Startseite ebenso.
pub const MAX_SCHRIFTGRAD: f32 = 2048.0;

/// Ueberschriften sind GROSSER als der Absatz, nicht kleiner. Die
/// Stufen sind 1,5 / 1,25 / 1,0 / 0,9 / 0,8 / 0,75 des Grundgrads.
pub fn grad_ueberschrift(ebene: u8) -> f32 {
    match ebene {
        1 => GRAD_TEXT * 1.5,
        2 => GRAD_TEXT * 1.25,
        3 => GRAD_TEXT * 1.1,
        4 => GRAD_TEXT,
        5 => GRAD_TEXT * 0.9,
        6 => GRAD_TEXT * 0.8,
        _ => GRAD_TEXT,
    }
}

/// Rand um Links: sie sollen als Links erkennbar sein, ohne die Seite
/// in Farben zu ertranken.
pub const LINK_RAND: f32 = 2.0;

/// Hoechste Zahl verschachtelter INLINE-Elemente (M3, M5).
///
/// GEMESSEN, warum das eine Grenze braucht: `<b><i><b><i>...` ohne Ende
/// ist 4 Byte je Zeichen in der Eingabe und ein Marken-Eintrag je
/// Zeichen im Stapel. Der HTML-Parser begrenzt die ELEMENTTIEFE bereits
/// auf `MAX_TIEFE` (512) — aber 512 verschachtelte `<b>` sind 512
/// Eintraege fuer EINEN Absatz, und davon hat niemand etwas. 64 reichen
/// fuer `<b><i><u><span><a><strong><em><code>` mit Leichtigkeit.
pub const MAX_INLINE_TIEFE: usize = 64;

/// Höchstzahl gemerkter Absätze je Seite für den Nachumbruch (M3).
/// GEMESSEN: de.wikipedia.org/wiki/Deutschland hat rund 2 000 Absätze.
pub const MAX_ABSAETZE_MERKEN: usize = 50_000;

/// Über dieser Breite darf der Nachumbruch ein Wort doch teilen — eine
/// 5 000-Zeichen-URL soll keine 40 000-px-Zeile ergeben.
const MAX_WORTBREITE_KLEMME: f32 = 1200.0;

/// Hoechste Zahl Knoten, die EIN Inline-Element bekommen darf (W6C, M3).
///
/// GEMESSEN, warum 64: ein `<span>` ueber einen Absatz hinweg (4 Zeilen zu
/// 16 Kaesten) hat rund fuenf Knoten. 64 ist die Zahl, bei der ein
/// Inline-Element mit einem Absatz gleichgesetzt wird — und `MAX_BAU_TIEFE`
/// (64) gilt fuer die Tiefe. Wer mehr braucht, hat einen Absatz im Tag.
pub const MAX_KNOTEN_VON_INLINE: usize = 64;

/// Hoechste Zahl Knoten, die EIN Element fuer sich beanspruchen darf (W6E, M3).
///
/// GEMESSEN, warum 8 und nicht 1 (30.09.2026, 16:55, Koordinationsbefund
/// „21 Klicks auf den Knopf, `klick()` immer `false"): der Kasten des
/// Elements ist nicht immer der erste Knoten, den sein Aufruf erzeugt — ein
/// Absatz, der beim Bau geschrieben wird, gehoert dem vorherigen Element.
/// Acht sind der Kasten plus seine sieben Kinder; wer mehr braucht,
/// laesst den Rest unbeansprucht. Ein Knoten OHNE Nummer ist harmlos (der
/// Klick laeuft weiter zum Elternkasten), eine FALSCHE Nummer nicht: der
/// Handler eines anderen Elements haengt dann an diesem Knoten.
pub const MAX_KNOTEN_VON_ELEMENT: usize = 8;

/// Hoechster `cellpadding` in Pixeln (M4, 21:15).
///
/// GEMESSENE BEGRUENDUNG fuer die Zahl: `cellpadding` ist ein Abstand in
/// Zellen, und ein Abstand, der die Seite breiter macht als das Fenster, ist
/// kein Abstand, sondern ein Absturz. GEMESSENE ENTSCHEIDUNG fuer 100: der
/// groesste Abstand, den eine Seite in einem Kasten sinnvoll einsetzen kann,
/// ist eine Handbreit; darueber ist es eine Zahl ohne Bedeutung. Wer mehr
/// braucht, schreibt CSS — und die kommt ueber die Kaskade.
pub const MAX_CELLPADDING: u32 = 100;

/// Hoechster `cellspacing` in Pixeln (M4, 22:15).
///
/// GEMESSENE BEGRUENDUNG fuer die Zahl: dieselbe wie bei `cellpadding` — ein
/// Abstand, der die Seite breiter macht als das Fenster, ist kein Abstand,
/// sondern ein Absturz. GEMESSENE ENTSCHEIDUNG fuer 100: der groesste
/// Abstand, den eine Seite in einer Tabelle sinnvoll einsetzen kann, ist eine
/// Handbreit. Wer mehr braucht, schreibt CSS.
pub const MAX_CELLSPACING: u32 = 100;

/// Hoechste Zahl verschieden STILE in einem Absatz (M3).
///
/// GEMESSEN: das sind die Abschnitte, in die ein Absatz zerfaellt, wenn
/// Inline-Elemente ihn unterteilen ("Text <b>fett</b> <a>Link</a> mehr"
/// = 4 Abschnitte). Eine Seite mit 10.000 solcher Wechseln in EINEM
/// Absatz ist kein Text, sondern ein Fehler des Generators. 256 ist
/// doppelt so viel, wie die schlimmste echte Seite braucht.
pub const MAX_ABSCHNITTE: usize = 256;

/// Hoechste Zahl WOERTER in einem Absatz (M3).
///
/// GEMESSEN, warum das eine eigene Grenze ist und nicht dieselbe wie
/// `MAX_ABSCHNITTE`: die Abschnitte begrenzen die STILE, die Woerter die
/// TEXTMENGE. Ein `<p>` aus 10 MB hat einen einzigen Abschnitt und
/// 1.700.000 Woerter — die Abschnittgrenze greift nie, die Wortgrenze
/// immer. 20.000 Woerter sind zehn Seiten Fliesstext; wer dort noch liest,
/// liest nicht mehr an dieser Stelle.
pub const MAX_WOERTER_PRO_ABSATZ: usize = 20_000;

/// Hoechste Zahl ZEILEN aus EINEM Absatz (M3).
///
/// GEMESSEN, warum das noetig ist: bei 40 Pixel Nutzbreite (der kleinste
/// Wert, den `absatz_schliessen` annimmt) passen zwei Zeichen je Zeile.
/// Ein Absatz aus 20.000 Woerten mit je einem Zeichen ergaebe 10.000
/// Zeilen und damit 10.000 Knoten fuer einen Absatz. 2.000 Zeilen sind
/// ein sehr langer Absatz; wer dort noch liest, scrollt.
pub const MAX_ZEILEN_PRO_ABSATZ: usize = 2_000;

/// Einrueckung eines Zitats (`<blockquote>`), in Pixeln.
///
/// GEMESSEN, warum sie nicht mit dem Absatzabstand gleich ist: ein Zitat
/// ist ein Absatz, der NICHT mit dem Text fliessen soll. Es steht in
/// einer eigenen Spalte, und die Einrueckung ist der einzige Hinweis
/// darauf, dass der folgende Absatz etwas anderes ist.
pub const ZITAT_EINRUECKUNG: f32 = 24.0;

/// Eine CSS-Masse in Pixel, oder 0, wenn sie nicht endlich ist.
///
/// `NaN` und negativ sind beide 0: ein Rahmen mit negativem Abstand ist
/// kein Rahmen, und `NaN` in einem `Length` waere ein Layoutfehler.
fn beschaendige(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}

/// Was die KASKADE ueber ein Formularelement sagt — die Zahlen, aus denen
/// sein Kasten gebaut wird.
///
/// 03.10. 21:2x (HTML). GEMESSENER BEFUND, warum es diese Struktur gibt: die
/// Formular-Zweige in `kind()` rufen **nie** `element()`, also nie
/// `rahmen_von()` und nie `element_innen()`. Sie holten sich vorher
/// `self.platzhalter(...)` und bauten Text. Die Seite schrieb fuer
/// `#searchInput` (`_m_form_osd.rs`, 1280 px)
///
/// ```text
/// height: 4.4rem; border-left-width: 1px; padding: .8rem 9.6rem .8rem 1.2rem
/// ```
///
/// und keine der drei Angaben kam an: `size=192x192`, `bg=Color{118,118,118}`.
///
/// DER WEG IST DERSELBE WEG WIE `rahmen_von`: `Berechnet` liegt auf dem
/// Kaskaden-Stapel, und `feld_von_kaskade` liest es. Das ist keine zweite
/// Kaskade — es ist dieselbe, einmal mehr gelesen. Der Unterschied zu
/// `rahmen_von` ist nur der, dass diese Werte auch **Farbe** und **Hoehe**
/// tragen, nicht nur den Rahmen.
/// HTML_1 (04.10.2026): der RAHMEN aus der **UA-Ebene** fuer ein
/// Formularfeld.
///
/// GEMESSENER BEFUND, warum es diese Struktur gibt: das UA-Blatt
/// (`narsil-css/src/ua.rs:216`) traegt
/// `input, textarea { border: 1px solid #8f8f9d; border-radius: 2px;
/// padding: 1px 2px }` — GEMESSEN an Firefox 157, mit Anlage
/// `team/testseiten/ua_formular.html`. Diese Regel steht in `mit_ua`, und
/// `mit_ua` ist die Autorenebene **mit** UA-Blatt; `k.werte.last()` traegt
/// `autor`, die **ohne**. `feld_von_kaskade` las nur `autor` — und damit
/// kam der Vorgaberahmen an keinem Formularfeld an.
///
/// GEMESSENE FOLGE (Kasten, `tests/feld_rahmen.rs`):
///
/// ```text
///   MESSUNG <input type="text">:  box 192x22.4  padding = 0/0/0/0
///                                 background = #767676
/// ```
///
/// Der aeussere Kasten hat **0 px Innenabstand** — und der Innenabstand
/// IST der Rahmen (I2). Die weisse Feldflaeche des Kindes liegt damit auf
/// der grauen Flaeche und deckt sie zu: **im Bild ist kein einziges
/// graues Pixel zu sehen, das Feld ist unsichtbar.** Firefox zeigt es
/// als weisses Rechteck mit 1-px-Rahmen.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Farbe hier `Copy` und nicht `Option`
/// ist: `rahmen_farbe` gibt `currentcolor` als `farbe` zurueck, wenn keine
/// Farbangabe da ist — das ist eine **Farbangabe**, keine Abwesenheit.
/// `None` bedeutet hier „die UA-Ebene sagt gar nichts", und das ist genau
/// der Fall, in dem der Autor selbst entschieden hat.
#[derive(Debug, Clone, Copy)]
struct FeldRahmen {
    oben: f32,
    rechts: f32,
    unten: f32,
    links: f32,
    farbe: Color,
    /// HTML_2 (04.10.2026): die **Polsterung** aus derselben Regel.
    ///
    /// GEMESSENER BEFUND, dass sie fehlte und dass es dieselbe Ursache ist:
    /// `ua.rs:216` schreibt `padding: 1px 2px` in **derselben** Deklaration
    /// wie den Rahmen. Der Rahmen kam nach HTML_1 an, die Polsterung nicht —
    /// weil `feld_von_kaskade` sie aus `autor` las und die UA-Ebene nur
    /// einmal (fuer den Rahmen) gelesen wurde. GEMESSEN nach dem
    /// Rahmenfix: der Inhalt begann **1 px** nach dem Kastenrand (nur der
    /// Rahmen), Firefox beginnt bei **3 px** — es fehlten 2 px
    /// waagerecht und 1 px senkrecht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Polsterung **kein** `Option` ist
    /// und der Rahmen doch: `padding: 0` vom Autor ist eine Angabe, und
    /// `mit_ua` traegt sie dann als 0. Fuellung 0 ist also eine gueltige
    /// Antwort, kein „nichts gesagt" — anders als beim Rahmen, wo 0 px
    /// Breite zusammen mit `border: 0` bedeutet, dass es keinen Rahmen
    /// gibt. Beide stehen in derselben Struktur, weil sie aus derselben
    /// Deklaration kommen; die Entscheidung steht an der **Nutzung**.
    fuellung_oben: f32,
    fuellung_rechts: f32,
    fuellung_unten: f32,
    fuellung_links: f32,
}

impl FeldRahmen {
    /// Liest die vier Breiten und die Farbe aus einer `Berechnet`.
    ///
    /// HTML_2 (04.10.2026): DIE STRUKTUR HAT KEIN `Option` MEHR.
    ///
    /// GEMESSENER GRUND, und er ist derselbe Fehler ein zweites Mal:
    /// `border: 0` loescht bei vier Nullen `None` — und nimmt damit die
    /// **Polsterung** mit, die in derselben Deklaration steht und in
    /// Firefox weiter gilt (CSS 2.1, 8.3: `border` nimmt nur den Rahmen).
    /// GEMESSEN: `Autor border:0` ergab `fuellung 0/0/0/0` statt `1/2/1/2`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „kein Rahmen" jetzt **vier Nullen**
    /// sind und nicht `None`: vier Nullen sind eine Aussage ueber die
    /// **Rahmenbreite**, und `0` ist dort die richtige Antwort. Ein
    /// `Option` an der ganzen Struktur sagt dagegen „die Ebene sagt
    /// gar nichts" — und das ist fuer die Polsterung falsch, fuer den
    /// Rahmen nur dann richtig, wenn der Autor `border: 0` schreibt.
    ///
    /// GEMESSENE EINORDNUNG, warum das AUCH den Ebenen-Vergleich aus
    /// HTML_1 ueberfluessig macht: `mit_ua` rechnet ueber `k.alle`, also
    /// ueber alle Blaetter, und der Autor schlaegt das UA-Blatt. Damit
    /// ist `mit_ua` **die** Antwort — fuer beide Eigenschaften, ohne
    /// Umweg. Die Gegenproben (`border:0`, `border:3px rot`, `padding:9px`,
    /// `padding:0`) stehen in `tests/feld_rahmen.rs`.
    fn von(b: &narsil_css::Berechnet) -> Self {
        let r = Self {
            oben: beschaendige(b.rahmen_oben_breite),
            rechts: beschaendige(b.rahmen_rechts_breite),
            unten: beschaendige(b.rahmen_unten_breite),
            links: beschaendige(b.rahmen_links_breite),
            farbe: {
                let f = kaskade::rahmen_farbe(b);
                Color {
                    r: f.r,
                    g: f.g,
                    b: f.b,
                    a: f.a,
                }
            },
            fuellung_oben: beschaendige(b.fuellung_oben),
            fuellung_rechts: beschaendige(b.fuellung_rechts),
            fuellung_unten: beschaendige(b.fuellung_unten),
            fuellung_links: beschaendige(b.fuellung_links),
        };
        r
    }
}

#[derive(Debug, Clone, Copy)]
struct FormularStil {
    rand_oben: f32,
    rand_rechts: f32,
    rand_unten: f32,
    rand_links: f32,
    /// Die Farbe des **Rahmens** (I2: sie ist der Hintergrund des Kastens).
    rahmenfarbe: Color,
    /// Die Farbe der **Feldflaeche** (CSS 2.1, 8.3.1).
    hintergrund: Color,
    /// Die Farbe des **Textes** im Feld.
    farbe: Color,
    fuellung_oben: f32,
    fuellung_rechts: f32,
    fuellung_unten: f32,
    fuellung_links: f32,
    /// `width` aus CSS, wenn sie eine Zahl ist. `None` heisst: die Kaskade
    /// sagt nichts (oder Prozent — das rechnet `Berechnet` nicht auf, siehe
    /// `feld_von_kaskade`).
    breite_px: Option<f32>,
    /// Release 08.10.2026: `width` in PROZENT (`w-full`) — das Feld bleibt
    /// dann prozentual und schrumpft mit seinem Container, statt beim Bau
    /// gegen die Fensterbreite in feste Pixel gerechnet zu werden.
    breite_prozent: Option<f32>,
    /// `height` aus CSS, wenn sie eine Zahl ist.
    hoehe_px: Option<f32>,
    schriftgroesse: f32,
    /// Release 08.10.2026: `border-radius` des Autors (CSS Backgrounds 3
    /// §5.4). `None` heisst: der Autor sagt nichts, es bleibt die Vorgabe
    /// des Aufrufers (2 px). GEMESSEN vorher: die Pille der Startseite
    /// (`border-radius:22px`) und jedes runde Suchfeld kamen eckig an.
    radius: Option<[f32; 4]>,
}

impl FormularStil {
    /// Ein Feld ganz ohne CSS: 1 px Rahmen in `#767676`, weisse Flaeche,
    /// schwarzer Text, 16 px Schrift.
    ///
    /// GEMESSENE HERKUNFT der Zahlen: `#767676` ist der `border-color` des
    /// aktuellen UA-Stylesheets von Firefox und Chrome fuer ein leeres
    /// Textfeld (vor #a2a9b1). Es ist damit eine **gemessene** Vorgabe und
    /// keine erfundene — was bei `ABSATZABSTAND` (12 px) nicht gilt.
    fn standard() -> Self {
        Self {
            rand_oben: 1.0,
            rand_rechts: 1.0,
            rand_unten: 1.0,
            rand_links: 1.0,
            rahmenfarbe: FARBE_FELD,
            hintergrund: FARBE_SEITE,
            farbe: FARBE_TEXT,
            fuellung_oben: 1.0,
            fuellung_rechts: 2.0,
            fuellung_unten: 1.0,
            fuellung_links: 2.0,
            breite_px: None,
            breite_prozent: None,
            hoehe_px: None,
            schriftgroesse: GRAD_TEXT,
            radius: None,
        }
    }

    /// Das KAESTCHEN (`checkbox`/`radio`): schwarz, quadratisch, ohne Fuellung.
    fn kaestchen() -> Self {
        // Release 09.10.2026: 13 px mit 1 px grauem Rand wie in Chrome
        // (vorher 17 px mit 2 px schwarzem Ring und 2 px Luecke — das
        // Kaestchen wirkte wie ein schwarzer Klotz).
        Self {
            rand_oben: 1.0,
            rand_rechts: 1.0,
            rand_unten: 1.0,
            rand_links: 1.0,
            rahmenfarbe: FARBE_FELD,
            hintergrund: FARBE_SEITE,
            farbe: FARBE_TEXT,
            fuellung_oben: 0.0,
            fuellung_rechts: 0.0,
            fuellung_unten: 0.0,
            fuellung_links: 0.0,
            breite_px: Some(13.0),
            breite_prozent: None,
            hoehe_px: Some(13.0),
            schriftgroesse: GRAD_TEXT,
            radius: None,
        }
    }
}

/// EIN Wort im Absatzstrom.
///
/// WARUM EIGENER TYP und nicht ein Tupel: ein Wort traegt jetzt FUERNF
/// Angaben — Text, Abschnitt, Leerzeichen davor und die beiden
/// horizontalen Raender. Ein Tupel aus fuenf Elementen ist an der
/// dritten Verwendung nicht mehr lesbar, und die Namen sind hier
/// wichtiger als die Kuerze.
#[derive(Debug, Clone)]
struct Wort {
    /// Der Text des Wortes (ohne Leerraum).
    text: String,
    /// Der Abschnitt, aus dem es stammt (Vorlage fuer Stil und Link).
    abschnitt: usize,
    /// Stand zwischen dem vorherigen Wort und diesem Weissraum?
    ///
    /// CSS 2.1, 16.6.1: Weissraum wird zu einem Leerzeichen
    /// zusammengefasst, aber nur wo in der Quelle Weissraum stand. An
    /// einer Inline-Grenze bleibt er, weil eine Inline-Grenze keine
    /// Trennstelle ist.
    leer_davor: bool,
    /// Der linke Rand des Abschnitts in Pixeln (CSS `margin-left` an
    /// einem Inline-Element).
    rand_links: f32,
    /// Der rechte Rand des Abschnitts in Pixeln.
    rand_rechts: f32,
    /// Release 11.10.2026: darf VOR diesem Wort umbrochen werden? An einem
    /// Leerzeichen ja (ausser unter `white-space: nowrap`), zwischen zwei
    /// Abschnitten ohne Leerraum nur zwischen CJK-Zeichen (UAX #14).
    /// GEMESSEN vorher: jede Abschnittsgrenze war eine Umbruchstelle —
    /// `<a>Link</a>.` konnte den Punkt allein in die naechste Zeile setzen.
    bruch_davor: bool,
    /// Release 12.10.2026 (Agent CSS2): `white-space: pre-wrap` — erhaltene
    /// Leerzeichen am Wortende HAENGEN am Zeilenende (CSS Text 3 §4.1.3): sie
    /// zaehlen nicht, wenn geprueft wird, ob das Wort in die Zeile passt.
    haengt: bool,
    /// Release 15.10.2026 (Agent CSS2): "other space separators" (U+1680,
    /// U+2000..U+2006, U+2008..U+200A, U+205F, U+3000) at the end of the word
    /// hang at the end of a line (CSS Text 3 §4.1.3) — in every mode except
    /// `break-spaces`, where they take up space.
    other_spaces_hang: bool,
    /// Release 13.10.2026 (Agent CSS2): das Wort wird nie hart geteilt — in
    /// `white-space: nowrap`/`pre` (CSS Text 3 §3) und in einem Block, der
    /// seinen Ueberlauf beschneidet (dort steht die Zeile ueber, statt zu
    /// brechen). GEMESSEN vorher (WPT css-overflow `text-overflow-001`): ein
    /// 2070 px langer `nowrap`-Text wurde bei 1200 px geteilt.
    unteilbar: bool,
}

/// Die horizontalen Raende eines Stils in Pixeln.
///
/// Die Absatz-Fuellung und -Einrueckung sind oben/unten; ein Inline-
/// Element bekommt seinen Platz links und rechts. Beides wird hier in
/// Pixeln geholt, weil der Umbruch in Pixeln rechnet.
/// Was ein Absatz braucht, um seine Zeilen (neu) zu schreiben.
#[derive(Debug, Clone)]
struct AbsatzDaten {
    woerter: Vec<Wort>,
    /// Release 10.10.2026: `-webkit-line-clamp` des Blocks (siehe
    /// [`zeilen_klemme`]) — hoechstens so viele Zeilen, die letzte mit „…".
    max_zeilen: Option<usize>,
    /// Release 13.10.2026 (Agent CSS2): `line-clamp: … no-ellipsis` — die
    /// letzte Zeile endet ohne Auslassung.
    ohne_auslassung: bool,
    abschnitte: Vec<(Style, Option<String>, Option<usize>)>,
    stil: Style,
    zeilenhoehe: Option<f32>,
    ausrichtung: narsil_css::Textausrichtung,
    /// Release 13.10.2026 (Agent CSS2): die Ausrichtung der LETZTEN Zeile
    /// (vor einem erzwungenen Umbruch oder am Blockende; CSS Text 3 §7.2).
    ausrichtung_letzte: Option<narsil_css::Textausrichtung>,
    /// Release 13.10.2026 (Agent CSS2): `text-justify: none` (CSS Text 3
    /// §7.4) — keine Dehnung.
    blocksatz_aus: bool,
    /// Release 13.10.2026 (Agent CSS2): der Block laeuft von rechts nach links.
    rtl: bool,
    /// Release 14.10.2026 (Agent CSS2): `unicode-bidi` of the block, see
    /// [`BidiMode`].
    bidi_mode: BidiMode,
    /// Release 13.10.2026 (Agent CSS2): `text-overflow: ellipsis` oder eine
    /// Zeichenkette — was am Ende einer ueberlaufenden Zeile steht.
    auslassung: Option<String>,
    ord: Option<usize>,
    /// Release 08.10.2026: Pseudokaesten im Textfluss (siehe `Absatz::anker`).
    anker: Vec<(usize, NodeId, bool)>,
    /// Release 11.10.2026 (Agent CSS2): `text-indent` der ERSTEN Zeile
    /// (CSS 2.1 §16.1), 0 fuer jeden Absatz, der nicht die erste Zeile seines
    /// Blocks traegt.
    einzug: f32,
    /// Release 12.10.2026 (Agent CSS2): je Abschnitt — gehoert der Leerraum
    /// vor seinem ersten Wort IHM selbst (sein Text beginnt damit, der
    /// vorige endete nicht so)? Dann steht das Leerzeichen am Anfang seines
    /// Laufs, nicht am Ende des vorigen (CSS 2.1 §16.6.1: der erste Leerraum
    /// einer Folge bleibt, und zwar in seinem Element).
    leer_eigen: Vec<bool>,
    /// Release 15.10.2026 (Agent CSS2): the line clamp ended after this
    /// paragraph and more content followed, so its last line carries the
    /// ellipsis (set by `klemme_auslassen` after the paragraph was written).
    /// Kept here so a re-break (`absatz_neu_umbrechen`) writes it again.
    clamp_ellipsis: bool,
    /// Release 15.10.2026 (Agent CSS2): the colour of `::first-line` for the
    /// runs of the first line that inherit the block's colour.
    first_line_color: Option<Color>,
    /// Release 15.10.2026 (Agent CSS2): the lines are balanced (CSS Text 4
    /// §6.2 `text-wrap-style: balance`): as many lines as greedy breaking
    /// gives, but all about as long.
    balance: bool,
}

/// Release 11.10.2026 (Agent CSS2): Versatz und Breite der Zeile `i` mit dem
/// Einzug der ersten Zeile (`text-indent`, CSS 2.1 §16.1). GEMESSEN vorher
/// (WPT `text/text-indent-*`): `text-indent: 96px` wirkte nicht.
fn mit_einzug(daten: &AbsatzDaten, breiten: ZeilenBreite<'_>, i: usize) -> (f32, f32) {
    let (v, b) = breiten(i);
    if i == 0 && daten.einzug != 0.0 && daten.einzug.is_finite() {
        (v + daten.einzug, (b - daten.einzug).max(0.0))
    } else {
        (v, b)
    }
}

/// Release 14.10.2026 (Agent CSS2): the `unicode-bidi` value of a block that
/// matters for the visual order of its lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum BidiMode {
    #[default]
    Normal,
    /// `plaintext`: every paragraph finds its own direction.
    Plaintext,
    /// `bidi-override` / `isolate-override`.
    Override,
}

/// Release 14.10.2026 (Agent CSS2): a line in visual order (UAX #9, rules up
/// to L2, mirrored glyphs at odd levels), regrouped into runs of the same
/// section. Applies to lines with right-to-left letters (Hebrew and other
/// class R) and to every line of an RTL paragraph; lines with Arabic stay as
/// they are, because the shaper reverses Arabic runs itself
/// (`narsil_text::arabisch::ausgabe`). `None` when the order does not change.
/// MEASURED before (WPT html `dir_auto-pre-N-between-Rs`, `...!` in an RTL
/// paragraph): neutral text stayed in logical order, and Hebrew words were
/// drawn left to right.
fn bidi_visual_runs(runs: &[(String, usize)], rtl: bool, mode: BidiMode) -> Option<Vec<(String, usize)>> {
    use narsil_text::bidi::{Absatzrichtung, analysiere_zeichen, bidi_klasse};
    use narsil_text::unicode_daten::bidi_klassen::BidiKlasse;
    const MAX_BIDI_CHARS: usize = 8192;
    let mut chars: Vec<char> = Vec::new();
    let mut owner: Vec<usize> = Vec::new();
    for (k, (t, _)) in runs.iter().enumerate() {
        for c in t.chars() {
            chars.push(c);
            owner.push(k);
        }
    }
    if chars.is_empty() || chars.len() > MAX_BIDI_CHARS {
        return None;
    }
    let mut has_r = false;
    for c in &chars {
        match bidi_klasse(*c) {
            BidiKlasse::Al => return None,
            BidiKlasse::R => has_r = true,
            _ => {}
        }
    }
    // `bidi-override` lays the characters out in the block's direction as
    // they are (CSS Writing Modes 3 §2.2): reversed for RTL, untouched for LTR.
    if mode == BidiMode::Override {
        if !rtl {
            return None;
        }
        let mut out: Vec<(String, usize)> = Vec::with_capacity(runs.len());
        for i in (0..chars.len()).rev() {
            let c = narsil_text::bidi::spiegel(chars[i]).unwrap_or(chars[i]);
            let section = runs[owner[i]].1;
            match out.last_mut() {
                Some((t, s)) if *s == section => t.push(c),
                _ => out.push((c.to_string(), section)),
            }
        }
        return Some(out);
    }
    if !has_r && !rtl {
        return None;
    }
    // `plaintext`: the paragraph's own first strong character decides, LTR
    // without one (CSS Writing Modes 3 §2.2, UAX #9 P2/P3).
    let direction = match (mode, rtl) {
        (BidiMode::Plaintext, _) => Absatzrichtung::Automatisch,
        (_, true) => Absatzrichtung::Rtl,
        _ => Absatzrichtung::Ltr,
    };
    let analysis = analysiere_zeichen(&chars, direction);
    let unchanged = analysis.reihenfolge.iter().enumerate().all(|(i, &j)| i == j)
        && analysis.reihenfolge.len() == chars.len()
        && analysis.ebenen.iter().all(|l| l.is_none_or(|l| l % 2 == 0));
    if unchanged {
        return None;
    }
    let mut out: Vec<(String, usize)> = Vec::with_capacity(runs.len());
    for &i in &analysis.reihenfolge {
        let Some(&c0) = chars.get(i) else { continue };
        let odd = analysis.ebenen.get(i).copied().flatten().is_some_and(|l| l % 2 == 1);
        let c = if odd { narsil_text::bidi::spiegel(c0).unwrap_or(c0) } else { c0 };
        let section = runs[owner[i]].1;
        match out.last_mut() {
            Some((t, s)) if *s == section => t.push(c),
            _ => out.push((c.to_string(), section)),
        }
    }
    Some(out)
}

/// Release 08.10.2026: haengt einen vorhandenen Knoten (Pseudokasten) ans
/// ENDE der Kinder von `zeile` — aus seinem bisherigen Eltern heraus.
fn anker_einhaengen(doc: &mut Document, zeile: NodeId, knoten: NodeId) {
    anker_einhaengen_vor(doc, zeile, knoten, None);
}

/// Wie [`anker_einhaengen`], aber VOR dem Kind `vor` (wenn es da ist).
fn anker_einhaengen_vor(doc: &mut Document, zeile: NodeId, knoten: NodeId, vor: Option<NodeId>) {
    let Some(mut kinder) = doc.get(zeile).map(|n| n.children.clone()) else {
        return;
    };
    kinder.retain(|k| *k != knoten);
    match vor.and_then(|v| kinder.iter().position(|k| *k == v)) {
        Some(stelle) => kinder.insert(stelle, knoten),
        None => kinder.push(knoten),
    }
    let _ = doc.kinder_setzen(zeile, kinder);
}

/// Ein geschriebener Absatz (Release 07.10.2026, NACHUMBRUCH).
///
/// GEMESSENER ANLASS: der Bauer bricht mit `inhaltsbreite()` um — einer
/// Schätzung aus `width`/`padding`/`max-width` zum Zeitpunkt des Baus. Grid-
/// Spalten, Flex-Verteilung und Floats kennt er nicht. Auf Wikipedia lief
/// der Artikeltext darum ~500 px über die Spalte hinaus und unter die
/// Infobox. Der Wirt (narsil) misst nach dem Layout die echte Breite und
/// bricht über [`absatz_neu_umbrechen`] genau die Absätze neu um, deren
/// Schätzung falsch war — ohne zweite Kaskade.
#[derive(Debug, Clone)]
pub struct AbsatzSatz {
    /// Der Knoten, unter dem die Zeilenboxen hängen.
    pub knoten: NodeId,
    /// Die Zeilenboxen in Reihenfolge.
    pub zeilen: Vec<NodeId>,
    /// Die Umbruchbreite der ersten Zeile.
    pub breite: f32,
    /// `margin-left + padding-left` des Absatzes (vom Inhalt des Knotens aus).
    pub einzug_links: f32,
    /// `margin-right + padding-right`.
    pub einzug_rechts: f32,
    daten: std::sync::Arc<AbsatzDaten>,
}

impl PartialEq for AbsatzSatz {
    fn eq(&self, other: &Self) -> bool {
        self.knoten == other.knoten && self.zeilen == other.zeilen && self.breite == other.breite
    }
}

/// Die Tabellen, in die Zeilen geschrieben werden — im Bauer und im
/// fertigen [`Seite`] dieselben Felder.
struct ZeilenZiel<'a> {
    doc: &'a mut Document,
    links: &'a mut HashMap<NodeId, String>,
    dom_zu_ord: &'a mut HashMap<u32, usize>,
    layout_plan: &'a mut narsil_layout::LayoutPlan,
}

/// Breite und Versatz je Zeile: `(versatz_links, breite)`.
type ZeilenBreite<'a> = &'a dyn Fn(usize) -> (f32, f32);

/// Nie schmaler als das breiteste Wort: ein Browser bricht nicht IM Wort um
/// (CSS Text 3 §5, `overflow-wrap: normal`), das Wort steht über. Erst über
/// [`MAX_WORTBREITE_KLEMME`] wird doch geteilt.
///
/// GEMESSEN ohne diese Grenze: in der Infobox von en.wikipedia.org/wiki/Rust
/// wurde „inferred" zu „infe" / „rred". Release 10.10.2026: die Grenze galt
/// nur im Nachumbruch — der erste Bau teilte in einem 1-px-Kasten jedes Wort
/// in einzelne Buchstaben (Chrome: ein Wort je Zeile, das übersteht).
fn breitestes_wort(daten: &AbsatzDaten, messung: &narsil_layout::Messung, shrinks: bool) -> f32 {
    // Release 15.10.2026 (Agent CSS2): a word that may break anyway
    // (`overflow-wrap: anywhere|break-word`, `word-break: break-all|break-
    // word`, `line-break: anywhere`) does not widen the line. MEASURED before
    // (WPT css-text `overflow-wrap/overflow-wrap-anywhere-001`, Ahem
    // `&nbsp;x5 X` in 4ch with `overflow-wrap: anywhere`): the line became as
    // wide as the word and nothing broke.
    daten
        .woerter
        .iter()
        // In a shrink-to-fit box (`shrinks`) a `break-word` word still widens
        // the line: its emergency breaks do not shrink the box (CSS Text 3
        // §5.5), and such a box is not re-broken later (`nachumbruch`).
        // MEASURED before (WPT css-text `white-space/white-space-intrinsic-
        // size-001`, a float with `break-spaces` and `overflow-wrap:
        // break-word` in a 0-wide parent): the word was split letter by
        // letter.
        .filter(|w| {
            !daten
                .abschnitte
                .get(w.abschnitt)
                .is_some_and(|a| a.0.umbruch_ueberall && !(shrinks && a.0.break_word_only))
        })
        .map(|w| {
            let a = daten.abschnitte.get(w.abschnitt);
            let grad = a.map_or(GRAD_TEXT, |a| a.0.font_size);
            let (gewicht, kursiv, familie) =
                a.map_or((400, false, ""), |a| (a.0.schriftgewicht, a.0.kursiv, a.0.schriftfamilie.as_str()));
            // Release 15.10.2026 (Agent CSS2): with the letter- and word-
            // spacing the line breaking counts (`zeilen_bauen`), without the
            // spacing after the last letter. MEASURED before (WPT `CSS2/text/
            // letter-spacing-applies-to-015`, a caption `XXXXXX` with `1em`
            // spacing): the line was clamped to the word without spacing and
            // the word was split.
            let ok = |v: f32| if v.is_finite() { v } else { 0.0 };
            let (ls, ws) = a.map_or((0.0, 0.0), |a| (ok(a.0.buchstaben_abstand), ok(a.0.wort_abstand)));
            let t = ohne_nullbreite(&w.text);
            let n = t.chars().count() as f32;
            let spacing = ls * (n - 1.0).max(0.0) + ws * t.chars().filter(|c| *c == '\u{a0}').count() as f32;
            messung.breite_stil_familie(&t, grad, gewicht, kursiv, familie) + spacing.max(0.0) + w.rand_links + w.rand_rechts
        })
        .fold(0.0f32, f32::max)
        .min(MAX_WORTBREITE_KLEMME)
}

/// Bricht die Wörter eines Absatzes in Zeilen um (Messung wie beim Zeichnen).
fn absatz_zeilen(
    daten: &AbsatzDaten,
    messung: &narsil_layout::Messung,
    breiten: ZeilenBreite<'_>,
) -> Vec<Vec<(String, usize, bool)>> {
    let font = BitmapFont::builtin();
    let abschnitte = &daten.abschnitte;
    let breite_von = |s: &str, grad: f32, gewicht: u16, kursiv: bool, familie: &str| -> f32 {
        messung.breite_stil_familie(&ohne_nullbreite(s), grad, gewicht, kursiv, familie)
    };
    let grad_von = |s: &usize| -> f32 { abschnitte.get(*s).map_or(GRAD_TEXT, |a| a.0.font_size) };
    let stil_von = |s: &usize| -> (u16, bool, String) {
        abschnitte.get(*s).map_or_else(
            || (400, false, String::new()),
            |a| (a.0.schriftgewicht, a.0.kursiv, a.0.schriftfamilie.clone()),
        )
    };
    let nur_breite = |i: usize| mit_einzug(daten, breiten, i).1;
    // Release 15.10.2026 (Agent CSS2): third: the width of the hyphenation
    // string (`hyphenate-character`, with its letter-spacing).
    let spacing_of = |s: &usize| -> (f32, f32, f32) {
        abschnitte.get(*s).map_or((0.0, 0.0, 0.0), |a| {
            let ok = |v: f32| if v.is_finite() { v } else { 0.0 };
            let zeichen = a.0.hyphenate_character.as_deref().unwrap_or("-");
            let hyphen = if zeichen.is_empty() {
                0.0
            } else {
                ok(messung.breite_stil_familie(zeichen, a.0.font_size, a.0.schriftgewicht, a.0.kursiv, &a.0.schriftfamilie))
                    + ok(a.0.buchstaben_abstand) * zeichen.chars().count() as f32
            };
            (ok(a.0.buchstaben_abstand), ok(a.0.wort_abstand), hyphen)
        })
    };
    let mut zeilen = zeilen_bauen(&daten.woerter, &breite_von, &grad_von, &stil_von, &spacing_of, &nur_breite, &font);
    // Release 15.10.2026 (Agent CSS2): `text-wrap: balance` — the greedy line
    // count stays, and every line gets narrower by the same amount as long
    // as that count holds (binary search, at most 10 lines like the UAs).
    // MEASURED before (WPT css-text `white-space/text-wrap-balance-003`):
    // the value was read and ignored.
    // Not with a line clamp (the visible lines are greedy, WPT `text-wrap-
    // balance-line-clamp-001`), and never narrower than the widest word (no
    // emergency break may make up a line, WPT `text-wrap-balance-overflow-
    // 001`).
    if daten.balance && daten.max_zeilen.is_none() && (2..=10).contains(&zeilen.len()) {
        let anzahl = zeilen.len();
        let voll = nur_breite(0);
        let breitestes = breitestes_wort(daten, messung, true);
        if voll.is_finite() && voll > 1.0 && breitestes < voll {
            let (mut lo, mut hi) = (0.0f32, (voll - breitestes).max(0.0));
            let mut beste: Option<Vec<Vec<(String, usize, bool)>>> = None;
            for _ in 0..12 {
                let mitte = (lo + hi) / 2.0;
                let schmaler = |i: usize| (nur_breite(i) - mitte).max(1.0);
                let probe = zeilen_bauen(&daten.woerter, &breite_von, &grad_von, &stil_von, &spacing_of, &schmaler, &font);
                if probe.len() == anzahl {
                    lo = mitte;
                    beste = Some(probe);
                } else {
                    hi = mitte;
                }
            }
            if let Some(b) = beste {
                zeilen = b;
            }
        }
    }
    if let Some(n) = daten.max_zeilen
        && zeilen.len() > n
    {
        // CSS Overflow 4 §4.2: die Zeilen nach der n-ten fallen weg, die
        // n-te endet mit der Auslassung — und Woerter fallen weg, bis sie
        // in die Zeile passt (wie Chrome).
        zeilen.truncate(n);
        if daten.ohne_auslassung {
            return zeilen;
        }
        let messen = |z: &[(String, usize, bool)]| -> f32 {
            z.iter()
                .enumerate()
                .map(|(i, (t, a, leer))| {
                    let grad = grad_von(a);
                    let (g, k, f) = stil_von(a);
                    let raum = if *leer && i > 0 { breite_von(" ", grad, g, k, &f) } else { 0.0 };
                    breite_von(t, grad, g, k, &f) + raum
                })
                .sum()
        };
        let ziel = breiten(n - 1).1;
        if let Some(letzte) = zeilen.last_mut() {
            loop {
                let mut probe = letzte.clone();
                // Release 15.10.2026 (Agent CSS2): the ellipsis is an inline
                // box of the BLOCK (CSS Overflow 4 §4.3: "styled as the block
                // container"), not part of the last element's text — its own
                // run in the block's section. MEASURED before (WPT css-
                // overflow `line-clamp/block-ellipsis-005`): the "…" was bold,
                // purple and inside the border of the last `<span>`.
                match block_section(daten) {
                    Some(b) => probe.push(("\u{2026}".to_string(), b, false)),
                    None => {
                        if let Some(w) = probe.last_mut() {
                            w.0.push('…');
                        }
                    }
                }
                if letzte.len() <= 1 || messen(&probe) <= ziel + 0.5 {
                    *letzte = probe;
                    break;
                }
                letzte.pop();
            }
        }
    }
    // Release 15.10.2026 (Agent CSS2): see `AbsatzDaten::clamp_ellipsis` (the
    // bidi pass in `zeilen_schreiben` puts it on the visual side). MEASURED
    // before (WPT css-overflow `line-clamp/line-clamp-auto-002`, a bordered
    // `pre` block): the re-break after the first layout dropped the "…".
    if daten.clamp_ellipsis
        && let Some(line) = zeilen.last_mut()
        && !line.last().is_some_and(|r| r.0.ends_with('\u{2026}'))
    {
        // Trailing blanks of the line go (a blank `pre` line is a lone
        // U+00A0, WPT css-overflow `webkit-line-clamp-032`).
        while let Some(last) = line.last_mut() {
            let trimmed = last.0.trim_end().len();
            last.0.truncate(trimmed);
            if !last.0.is_empty() || line.len() == 1 {
                break;
            }
            line.pop();
        }
        match block_section(daten) {
            Some(b) => line.push(("\u{2026}".to_string(), b, false)),
            None => {
                if let Some(last) = line.last_mut() {
                    let trimmed = last.0.trim_end().len();
                    last.0.truncate(trimmed);
                    last.0.push('\u{2026}');
                }
            }
        }
    }
    zeilen
}

/// Release 15.10.2026 (Agent CSS2): the section of the paragraph's own text
/// (no element of its own, `Abschnitt::ord == None`) — the style of the
/// block container, which the block ellipsis takes.
fn block_section(daten: &AbsatzDaten) -> Option<usize> {
    daten.abschnitte.iter().position(|a| a.2.is_none())
}

/// Release 15.10.2026 (Agent CSS2): the clamp ellipsis after the text node
/// `text` (the end of the last line before the clamp point): its own text
/// node in the block's style at the end of that line (see `block_section`),
/// and the paragraph remembers it for a re-break (`AbsatzDaten::
/// clamp_ellipsis`). `prepend` (RTL) and a paragraph without a block section
/// keep the old form: the "…" joins the text itself.
fn append_clamp_ellipsis(doc: &mut Document, absaetze: &mut [AbsatzSatz], text: NodeId, prepend: bool, own_node: bool) {
    let line = doc.get(text).and_then(|n| n.parent);
    let satz = line.and_then(|l| absaetze.iter_mut().rev().find(|s| s.zeilen.contains(&l)));
    let block_style = satz.as_ref().and_then(|s| block_section(&s.daten).and_then(|b| s.daten.abschnitte.get(b)).map(|a| a.0.clone()));
    if let Some(satz) = satz {
        std::sync::Arc::make_mut(&mut satz.daten).clamp_ellipsis = true;
    }
    if !prepend
        && own_node
        && let (Some(line), Some(mut stil)) = (line, block_style)
        && line_is_last_child_parent(doc, line, text)
    {
        // Trailing blanks of the last text go before the ellipsis (a blank
        // `pre` line is a lone U+00A0, WPT css-overflow `webkit-line-clamp-
        // 032`).
        if let Some(t) = doc.get(text).and_then(|n| n.text.clone())
            && t.trim_end().len() != t.len()
        {
            let _ = doc.set_text(text, t.trim_end().to_string());
        }
        stil.size = Dimension::AUTO;
        stil.margin = Spacing::ZERO;
        stil.padding = Spacing::ZERO;
        stil.background = Color::TRANSPARENT;
        stil.rahmen = Default::default();
        stil.verlauf = None;
        stil.radius = [0.0; 4];
        stil.schatten_anzahl = 0;
        stil.deckkraft = 1.0;
        if let Ok(id) = doc.push_child(line, NodeKind::Text, Some("\u{2026}".to_string()), Span::EMPTY) {
            let _ = doc.set_style(id, stil);
            return;
        }
    }
    if let Some(t) = doc.get(text).and_then(|n| n.text.clone()) {
        let neu = if prepend { format!("\u{2026}{}", t.trim_start()) } else { format!("{}\u{2026}", t.trim_end()) };
        let _ = doc.set_text(text, neu);
    }
}

/// Is `text` the last child of `line` (so a pushed node follows it)?
fn line_is_last_child_parent(doc: &Document, line: NodeId, text: NodeId) -> bool {
    doc.get(line).is_some_and(|n| n.children.last() == Some(&text))
}

/// Release 10.10.2026: die Zeilenklemme eines Blocks — `-webkit-line-clamp:
/// N` wirkt (wie in Chrome) nur an `display: -webkit-box` mit
/// `-webkit-box-orient: vertical`, bei uns `flex` mit `column`. `var()` im
/// Wert wird aufgeloest (chip.de: `-webkit-line-clamp:var(--max-lines)`).
fn zeilen_klemme(s: &narsil_css::Berechnet) -> Option<usize> {
    if !matches!(s.anzeige, narsil_css::Anzeige::Flex | narsil_css::Anzeige::InlineFlex)
        || s.flex_richtung != narsil_css::FlexRichtung::Spalte
    {
        return None;
    }
    let roh = &s.schluessel.iter().find(|(n, _)| n == "line-clamp" || n == "-webkit-line-clamp")?.1;
    let wert = narsil_css::ersetze_var(roh.trim(), &s.variablen, 0)?;
    let n: usize = wert.trim().parse().ok()?;
    (n >= 1).then_some(n.min(10_000))
}

/// Release 13.10.2026 (Agent CSS2): der Text ohne U+200B (Leerzeichen ohne
/// Breite) — fuer die Messung; ohne Zuteilung, wenn keines darin steht.
fn ohne_nullbreite(s: &str) -> std::borrow::Cow<'_, str> {
    if s.chars().any(unsichtbares_format) {
        std::borrow::Cow::Owned(s.chars().filter(|c| !unsichtbares_format(*c)).collect())
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// Release 15.10.2026: default-ignorable format characters that are neither
/// measured nor drawn (Unicode §5.21, CSS Text 3 §4.1.3 / CSS Fonts 4 §5.3
/// "not rendered"): U+200B, the word joiners U+2060-2064, U+FEFF, the
/// variation selectors U+FE00-FE0F and U+E0100-E01EF, U+200D (no font here
/// renders emoji ZWJ sequences; their parts stand side by side, as in a
/// browser without the sequence), U+034F and the Mongolian selectors
/// U+180B-180F. U+200C (joining), the bidi controls, the tag characters and
/// U+00AD stay: other stages use them. If colour-emoji presentation is ever
/// chosen by U+FE0F, it has to read the text before this filter.
/// MEASURED before: golem.de's `summary::before { content: "\u{25B6}\u{FE0E}" }`
/// drew a .notdef box after the triangle, and every `\u{2764}\u{FE0F}` heart a
/// box after the heart.
fn unsichtbares_format(c: char) -> bool {
    matches!(c,
        '\u{200B}' | '\u{200D}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}' | '\u{FE00}'..='\u{FE0F}'
        | '\u{E0100}'..='\u{E01EF}' | '\u{034F}' | '\u{180B}'..='\u{180F}'
        // Release 15.10.2026 (Agent CSS2): the bidi marks and controls, once
        // the line is resolved (WPT css-text text-align-end/start-014..017 drew
        // an RLM box).
        | '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Release 13.10.2026 (Agent CSS2): `text-overflow` als Zeichenkette fuer das
/// Zeilenende (CSS Overflow 4 §3.3): `ellipsis` ist U+2026, eine
/// Zeichenkette steht selbst; bei zwei Werten gilt der zweite (rechte
/// Kante); `clip` ist keine Auslassung.
fn auslassung_von(wert: &str) -> Option<String> {
    let w = wert.trim();
    if w.is_empty() {
        return None;
    }
    let ende = if let Some(q) = w.strip_suffix(['"', '\'']) {
        let anf = q.rfind(['"', '\'']).map_or(0, |i| i + 1);
        return Some(q[anf..].to_string()).filter(|s| !s.is_empty() || w.len() >= 2);
    } else {
        w.split_whitespace().last().unwrap_or(w)
    };
    match ende.to_ascii_lowercase().as_str() {
        "ellipsis" => Some(String::from("\u{2026}")),
        _ => None,
    }
}

/// Release 13.10.2026 (Agent CSS2): die Zeilenzahl einer Klemme an diesem
/// Element — `-webkit-line-clamp` am alten `-webkit-box` (siehe
/// `zeilen_klemme`) oder `line-clamp: <Zahl>` an einem Blockbehaelter (CSS
/// Overflow 4 §4.2).
fn klemme_von(s: &narsil_css::Berechnet) -> Option<(usize, bool)> {
    if let Some(n) = zeilen_klemme(s) {
        return Some((n, true));
    }
    if !matches!(
        s.anzeige,
        narsil_css::Anzeige::Block
            | narsil_css::Anzeige::InlineBlock
            | narsil_css::Anzeige::ListenEintrag
            | narsil_css::Anzeige::TabelleZelle
    ) {
        return None;
    }
    // Release 13.10.2026 (Agent CSS2): neben `-webkit-line-clamp` gilt die alte
    // Form (sie setzt `continue: -webkit-legacy`, CSS Overflow 4 §4.2 — nur am
    // `-webkit-box`), und ein Mehrspaltenkasten klemmt nicht (`continue:
    // collapse` wirkt dort wie `auto`). GEMESSEN vorher (WPT `line-clamp-034`,
    // `-039`): beide wurden geklemmt.
    let schluessel = |name: &str| s.schluessel.iter().find(|(n, _)| n == name).map(|(_, w)| w.trim());
    if schluessel("-webkit-line-clamp").is_some() {
        return None;
    }
    let spalten = |name: &str| schluessel(name).is_some_and(|w| !w.is_empty() && !w.eq_ignore_ascii_case("auto"));
    if spalten("columns") || spalten("column-count") || spalten("column-width") {
        return None;
    }
    let roh = &s.schluessel.iter().find(|(n, _)| n == "line-clamp")?.1;
    let wert = narsil_css::ersetze_var(roh.trim(), &s.variablen, 0)?;
    // Release 13.10.2026 (Agent CSS2): `auto` (so viele Zeilen, wie in
    // `max-height` passen) und `no-ellipsis` (CSS Overflow 4 §4.2).
    let (mut n, mut auto, mut mit_auslassung) = (None, false, true);
    for w in wert.split_whitespace() {
        match w.to_ascii_lowercase().as_str() {
            "none" => return None,
            "auto" => auto = true,
            "no-ellipsis" => mit_auslassung = false,
            x => {
                if let Ok(z) = x.parse::<usize>() {
                    n = Some(z);
                }
            }
        }
    }
    let n = match (n, auto) {
        (Some(z), _) => z,
        (None, true) => {
            // Release 15.10.2026 (Agent CSS2): a fixed `height` limits the
            // lines as well as `max-height` (CSS Overflow 4 §4.2: the clamp
            // point is where the content would overflow the box). MEASURED
            // before (WPT css-overflow `line-clamp/line-clamp-auto-005`,
            // `height: 4.5lh`): nothing was clamped.
            let ok = |h: &f32| h.is_finite() && *h >= 0.0;
            let hoehe = match (s.hoehe.filter(ok), s.max_hoehe.filter(ok)) {
                (Some(a), Some(b)) => a.min(b),
                (a, b) => a.or(b)?,
            };
            // `min-height` wins over `max-height` (CSS 2.1 §10.7). MEASURED
            // before (WPT `line-clamp-auto-014`, `min-height: 4lh; max-height:
            // 3lh`): three lines instead of four.
            let hoehe = s.min_hoehe.filter(ok).map_or(hoehe, |m| hoehe.max(m));
            let zeile = s.zeilenhoehe_px;
            if !(zeile.is_finite() && zeile > 0.0) {
                return None;
            }
            // No line fits: the clamp keeps none (WPT `line-clamp-auto-011`,
            // `max-height: 0`); before, at least one line stayed.
            let lines = (((hoehe + 0.01) / zeile).floor().max(0.0) as usize).min(10_000);
            return Some((lines, mit_auslassung));
        }
        _ => return None,
    };
    (n >= 1).then_some((n.min(10_000), mit_auslassung))
}

/// Schreibt die Zeilenboxen und Textknoten eines Absatzes unter `eltern`.
///
/// Gibt die Zeilenboxen und die Textknoten (mit Text) zurück; der Bauer
/// wendet darauf Label-Regel und `id`-Vermerk an.
fn zeilen_schreiben(
    ziel: &mut ZeilenZiel<'_>,
    eltern: NodeId,
    daten: &AbsatzDaten,
    zeilen: &[Vec<(String, usize, bool)>],
    breiten: ZeilenBreite<'_>,
    messung: &narsil_layout::Messung,
) -> (Vec<NodeId>, Vec<(NodeId, String)>) {
    let abschnitte = &daten.abschnitte;
    let laufender_ord = daten.ord;
    let mut boxen: Vec<NodeId> = Vec::with_capacity(zeilen.len());
    let mut texte: Vec<(NodeId, String)> = Vec::new();
    let mut anker_i = 0usize;
    // Release 12.10.2026 (Agent CSS2): ein Inline-Element, das ueber mehrere
    // Zeilen laeuft, traegt Rand, Innenabstand und Rahmen LINKS nur am ersten
    // und RECHTS nur am letzten Stueck (CSS 2.1 §8.6, `box-decoration-break:
    // slice`). Je Abschnitt: erste und letzte Zeile, in der er steht; je
    // Element: sein erster und letzter Abschnitt. GEMESSEN vorher (WPT
    // `css1/c5504-imrgn-l-003`, `c5509-ipadn-l-003`): jede Zeile des Spans
    // begann mit seinem `margin-left`.
    let mut zeilen_von: Vec<(usize, usize)> = vec![(usize::MAX, 0); abschnitte.len()];
    for (i, zeile) in zeilen.iter().enumerate() {
        for (_, s, _) in zeile {
            if let Some(z) = zeilen_von.get_mut(*s) {
                z.0 = z.0.min(i);
                z.1 = z.1.max(i);
            }
        }
    }
    let mut enden: HashMap<usize, (usize, usize)> = HashMap::new();
    for (k, a) in abschnitte.iter().enumerate() {
        if let Some(ord) = a.2 {
            let e = enden.entry(ord).or_insert((k, k));
            e.1 = k;
        }
    }
    let element_ende = |s: usize, erstes: bool| -> bool {
        let Some(ord) = abschnitte.get(s).and_then(|a| a.2) else { return true };
        enden.get(&ord).is_none_or(|(a, b)| if erstes { *a == s } else { *b == s })
    };
    for (i, zeile) in zeilen.iter().enumerate() {
        // Release 13.10.2026 (Agent CSS2): die letzte Zeile richtet sich nach
        // `text-align-last` (siehe `AbsatzDaten::ausrichtung_letzte`).
        let ausrichtung = if i + 1 == zeilen.len() { daten.ausrichtung_letzte } else { Some(daten.ausrichtung) };
        // Release 13.10.2026 (Agent CSS2): in `rtl` steht eine Zeile ohne
        // Ausrichtung rechts (die Engine legt sie an die Startkante) —
        // `left` (auch `end`) braucht darum die volle Breite und `Links`.
        // GEMESSEN vorher (WPT css-text `text-align-end-*`, `-start-*` mit
        // `dir=rtl`): `text-align: end` stand rechts.
        let ausgerichtet = matches!(
            ausrichtung,
            Some(narsil_css::Textausrichtung::Zentriert | narsil_css::Textausrichtung::Rechts)
        ) || (daten.rtl && ausrichtung == Some(narsil_css::Textausrichtung::Links));
        // Release 13.10.2026 (Agent CSS2): Blocksatz dieser Zeile (nicht mit
        // `text-justify: none`).
        let dehnen = ausrichtung == Some(narsil_css::Textausrichtung::Blocksatz) && !daten.blocksatz_aus;
        // (Text, Abschnittsnummer) je Abschnitt auf dieser Zeile. Der
        // Leerraum zwischen zwei Abschnitten gehört an das Ende des
        // vorherigen — ausser zwischen zwei verschiedenen Links: dann bekommt
        // er einen eigenen, nicht klickbaren Lauf (CSS 2.1, 16.6.1).
        let mut runs: Vec<(String, usize)> = Vec::with_capacity(zeile.len().min(64));
        for (wort, s, leer) in zeile {
            match runs.last_mut() {
                Some((text, rs)) if *rs == *s => {
                    if *leer && !wort.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(wort);
                }
                _ => {
                    let vor_ist_link = runs
                        .last()
                        .and_then(|(_, rs)| abschnitte.get(*rs))
                        .map(|a| a.1.is_some())
                        .unwrap_or(false);
                    let mut text = String::new();
                    if *leer && !runs.is_empty() {
                        let gleiches_ziel = match (
                            runs.last().and_then(|(_, rs)| abschnitte.get(*rs)),
                            abschnitte.get(*s),
                        ) {
                            (Some(vor), Some(nach)) => vor.1.is_some() && vor.1 == nach.1,
                            _ => false,
                        };
                        if vor_ist_link && !gleiches_ziel {
                            let leer_s = abschnitte.iter().position(|a| a.1.is_none()).unwrap_or(0);
                            if *s != leer_s {
                                // Nur Leerzeichen einfügen, wenn es nicht das Ende ist
                                if !wort.is_empty() {
                                    runs.push((" ".to_string(), leer_s));
                                }
                            } else if !wort.is_empty() {
                                text.push(' ');
                            }
                        } else if vor_ist_link {
                            if !wort.is_empty() {
                                text.push(' ');
                            }
                        } else if daten.leer_eigen.get(*s).copied().unwrap_or(false) {
                            // Release 12.10.2026 (Agent CSS2): siehe
                            // `AbsatzDaten::leer_eigen`. GEMESSEN vorher (WPT
                            // `css1/c5506-ipadn-t-000`, `c5502-imrgn-r-*`):
                            // `<span>xx</span> yy` — der Hintergrund und der
                            // rechte Rand des Spans lagen hinter dem
                            // Leerzeichen des Folgetexts.
                            if !wort.is_empty() {
                                text.push(' ');
                            }
                        } else if let Some((vorher, _)) = runs.last_mut() {
                            if !wort.is_empty() {
                                vorher.push(' ');
                            }
                        }
                    }
                    text.push_str(wort);
                    if !text.is_empty() {
                        runs.push((text, *s));
                    }
                }
            }
        }
        // Release 13.10.2026 (Agent CSS2): das Leerzeichen ohne Breite (U+200B,
        // auch aus `break-spaces`) ist nur eine Umbruchstelle — es kommt nicht
        // in den Text (die Schrift misst und zeichnet es sonst als Glyphe).
        for lauf in runs.iter_mut() {
            if lauf.0.chars().any(unsichtbares_format) {
                lauf.0 = ohne_nullbreite(&lauf.0).into_owned();
            }
        }
        // Release 15.10.2026 (Agent CSS2): a soft hyphen at the end of a line
        // (not the last one: there it ends the text) is drawn as a hyphen,
        // every other one disappears (CSS Text 3 §6.1).
        if i + 1 < zeilen.len()
            && let Some(last) = runs.last_mut()
            && last.0.ends_with('\u{AD}')
        {
            last.0.pop();
            let zeichen = abschnitte.get(last.1).and_then(|a| a.0.hyphenate_character.clone());
            last.0.push_str(zeichen.as_deref().unwrap_or("-"));
        }
        for lauf in runs.iter_mut() {
            if lauf.0.contains('\u{AD}') {
                lauf.0 = lauf.0.replace('\u{AD}', "");
            }
        }
        runs.retain(|(t, _)| !t.is_empty());
        // Release 12.10.2026 (Agent CSS2): eine Leerzeile aus `zeilen_bauen`
        // (neben Floats) ist eine leere Zeilenbox in Zeilenhoehe.
        if runs.is_empty() && !zeile.is_empty() {
            continue;
        }
        // Die Zeilenbox ist so hoch wie der grösste Buchstabe in der Zeile —
        // mindestens aber der Absatzgrund (I1, `tests/textfluss.rs`).
        let hoechster_grad = runs
            .iter()
            .map(|(_, s)| abschnitte.get(*s).map_or(GRAD_TEXT, |a| a.0.font_size))
            .fold(daten.stil.font_size, f32::max);
        if !(0.1..=MAX_SCHRIFTGRAD).contains(&hoechster_grad) {
            continue;
        }
        // Release 11.10.2026: ein Lauf mit AUSDRUECKLICHER `line-height`
        // (Inline-Element, `::first-letter`) hebt die Zeile auf seine
        // Inline-Box-Hoehe (CSS 2.1 §10.8: die Zeilenbox umfasst alle
        // Inline-Boxen, jede so hoch wie ihre `line-height`). GEMESSEN vorher
        // (WPT `first-letter-punctuation-*`, 339 Dateien): `div::first-letter
        // { font-size: 36px; line-height: 2 }` ergab eine 43-px-Zeile statt
        // 72 px — die Referenz mit `<span>` traf es nur, weil ihr Absatz IM
        // Span beginnt und dessen `line-height` erbt.
        // Release 11.10.2026 (Agent CSS2): jeder Lauf traegt SEINE
        // Inline-Box bei — die ausdrueckliche `line-height` oder die normale
        // seiner Schriftgroesse —, der Block den Strut. GEMESSEN vorher (WPT
        // `block-non-replaced-height-005`): `span { font: 100px/1 Ahem }` in
        // einem 16-px-Block gab 120-px-Zeilen (die normale Hoehe der 100 px).
        let lauf_hoehe = runs
            .iter()
            .filter_map(|(_, s)| abschnitte.get(*s))
            .map(|a| {
                if a.0.zeilenhoehe_art != ZeilenhoeheArt::Normal && a.0.zeilenhoehe_px.is_finite() {
                    a.0.zeilenhoehe_px
                } else {
                    ZEILENHOEHE(a.0.font_size)
                }
            })
            .fold(0.0f32, f32::max);
        let zeilen_hoehe = daten
            .zeilenhoehe
            .unwrap_or_else(|| ZEILENHOEHE(daten.stil.font_size))
            .max(lauf_hoehe)
            .clamp(0.0, 10000.0);
        let (versatz, breite) = mit_einzug(daten, breiten, i);
        // Release 13.10.2026 (Agent CSS2): `text-align: justify` — der freie
        // Platz der Zeile verteilt sich auf ihre Wortzwischenraeume (CSS Text
        // 3 §7.3, `text-justify: auto` als `inter-word`), als zusaetzlicher
        // `word-spacing` je Lauf. Zeilen mit Pseudo- oder Atomkaesten bleiben
        // unveraendert. GEMESSEN vorher (WPT css-text `text-align/text-align-
        // justify-001..006`, `-justifyall-*`): `justify` war `left`.
        // Release 13.10.2026 (Agent CSS2): `text-overflow` (CSS Overflow 4
        // §3.3) — laeuft die Zeile ueber, fallen am Ende Zeichen weg, bis Text
        // und Auslassung passen; das erste Zeichen der Zeile bleibt (es wird
        // beschnitten statt ausgelassen). GEMESSEN vorher (WPT css-overflow
        // `text-overflow-002`, `-ellipsis-001`): die Angabe wirkte nicht.
        // Der Abschnitt der zuletzt ausgelassenen Zeichen: die Auslassung gehoert
        // fuer Treffer und Skripte zu IHM (CSS UI 3 §5.2; WPT
        // `text-overflow-023`), gezeichnet wird sie im Stil des Blocks.
        let mut auslassung_von_abschnitt: Option<usize> = None;
        if let Some(ell) = daten.auslassung.as_deref().filter(|_| daten.anker.is_empty() && !daten.rtl) {
            let lauf_breite = |text: &str, s: usize| -> f32 {
                let Some((st, _, _)) = abschnitte.get(s) else { return 0.0 };
                let zwischen = text.chars().filter(|c| matches!(c, ' ' | '\u{a0}')).count() as f32;
                messung.breite_stil_familie(text, st.font_size, st.schriftgewicht, st.kursiv, &st.schriftfamilie)
                    + st.buchstaben_abstand * text.chars().count() as f32
                    + st.wort_abstand * zwischen
            };
            let (_, zeilen_breite) = mit_einzug(daten, breiten, i);
            let gesamt: f32 = runs.iter().map(|(t, s)| lauf_breite(t, *s)).sum();
            if gesamt > zeilen_breite + 0.5 && !runs.is_empty() {
                // Die Auslassung im Stil des Blocks (CSS UI 3 §5.2) — sein
                // Abschnitt ist der erste. GEMESSEN vorher (WPT
                // `text-overflow-string-002`): sie stand in der Schrift des
                // letzten Spans.
                let ell_s = 0usize;
                let ziel = zeilen_breite - lauf_breite(ell, ell_s);
                let mut schritte = 0usize;
                loop {
                    schritte += 1;
                    let n = runs.len();
                    if n == 0 || schritte > 4096 {
                        break;
                    }
                    let vor: f32 = runs[..n - 1].iter().map(|(t, s)| lauf_breite(t, *s)).sum();
                    if vor + lauf_breite(&runs[n - 1].0, runs[n - 1].1) <= ziel {
                        break;
                    }
                    if runs.iter().map(|(t, _)| t.chars().count()).sum::<usize>() <= 1 {
                        break;
                    }
                    auslassung_von_abschnitt = Some(runs[n - 1].1);
                    // Ein ganzes Zeichen mit seinen kombinierenden Zeichen.
                    while let Some(c) = runs[n - 1].0.pop() {
                        let markierung = ('\u{300}'..='\u{36f}').contains(&c)
                            || ('\u{1ab0}'..='\u{1aff}').contains(&c)
                            || ('\u{20d0}'..='\u{20ff}').contains(&c)
                            || ('\u{fe00}'..='\u{fe0f}').contains(&c)
                            || c == '\u{200d}';
                        if !markierung {
                            break;
                        }
                    }
                    if runs[n - 1].0.is_empty() {
                        runs.pop();
                    }
                }
                // Passt nicht einmal das erste Zeichen mit ihr, wird sie selbst
                // beschnitten (§5.2) — sie steht trotzdem da.
                runs.push((ell.to_string(), ell_s));
            }
        }
        // Release 14.10.2026 (Agent CSS2): the line in visual order (UAX #9
        // L2), see `bidi_visual_runs` — after the ellipsis, which takes part.
        // MEASURED before (WPT css-overflow `text-overflow-string-006`, a
        // Hebrew ellipsis string): the ellipsis stayed in logical order.
        if daten.anker.is_empty()
            && let Some(visual) = bidi_visual_runs(&runs, daten.rtl, daten.bidi_mode)
        {
            runs = visual;
        }
        // Release 13.10.2026 (Agent CSS2): in nicht zusammenfallendem Leerraum
        // zaehlen nur die Zwischenraeume NACH dem letzten Tabulator (CSS Text
        // 3 §7.3; WPT `text-align-justify-tabs-*`). Ein Tabulator steht als
        // Folge geschuetzter Leerzeichen vor einem Nicht-Leerzeichen (siehe
        // `leerraum_erhalten`); der Lauf wird hinter ihm geteilt.
        let mut blocksatz_ab = 0usize;
        if dehnen && daten.anker.is_empty() {
            let mut letzter_tab: Option<(usize, usize)> = None;
            for (k, (text, s)) in runs.iter().enumerate() {
                let erhalten = abschnitte.get(*s).is_some_and(|a| {
                    matches!(a.0.weissraum, Weissraum::Pre | Weissraum::PreUmbruch | Weissraum::UmbruchLeer)
                });
                if !erhalten {
                    continue;
                }
                let mut zeichen = text.char_indices().peekable();
                while let Some((q, c)) = zeichen.next() {
                    if c != '\u{a0}' {
                        continue;
                    }
                    let ende = q + c.len_utf8();
                    let tab_ende = match zeichen.peek() {
                        Some((_, n)) => !matches!(n, ' ' | '\u{a0}'),
                        None => runs.get(k + 1).is_none_or(|(t, _)| !t.starts_with(' ')),
                    };
                    if tab_ende {
                        letzter_tab = Some((k, ende));
                    }
                }
            }
            if let Some((k, q)) = letzter_tab {
                if q < runs[k].0.len() {
                    let rest = runs[k].0.split_off(q);
                    let s = runs[k].1;
                    runs.insert(k + 1, (rest, s));
                }
                blocksatz_ab = k + 1;
            }
        }
        let blocksatz = if dehnen && daten.anker.is_empty() {
            let mut natuerlich = 0.0f32;
            let mut luecken = 0usize;
            for (k, (text, s)) in runs.iter().enumerate() {
                let Some((st, _, ord)) = abschnitte.get(*s) else { continue };
                natuerlich += messung.breite_stil_familie(text, st.font_size, st.schriftgewicht, st.kursiv, &st.schriftfamilie);
                // `letter-spacing` je Zeichen, `word-spacing` je Zwischenraum
                // (wie `narsil_display::abstands_teile`, ohne die Ausnahmen
                // verbundener Schriften).
                let zwischen = text.chars().filter(|c| matches!(c, ' ' | '\u{a0}')).count() as f32;
                natuerlich += st.buchstaben_abstand * text.chars().count() as f32 + st.wort_abstand * zwischen;
                if ord.is_some() {
                    let (erste, letzte) = zeilen_von.get(*s).copied().unwrap_or((i, i));
                    if erste == i && element_ende(*s, true) {
                        natuerlich += st.margin.left.resolve_or(breite, 0.0)
                            + st.padding.left.resolve_or(breite, 0.0)
                            + st.rahmen.gezeichnete_breite(3);
                    }
                    if letzte == i && element_ende(*s, false) {
                        natuerlich += st.margin.right.resolve_or(breite, 0.0)
                            + st.padding.right.resolve_or(breite, 0.0)
                            + st.rahmen.gezeichnete_breite(1);
                    }
                }
                let zaehlbar = if k + 1 == runs.len() { text.trim_end_matches([' ', '\u{a0}']) } else { text.as_str() };
                if k >= blocksatz_ab {
                    luecken += zaehlbar.chars().filter(|c| matches!(c, ' ' | '\u{a0}')).count();
                }
            }
            let frei = breite - natuerlich;
            if luecken > 0 && frei.is_finite() && frei > 0.0 { frei / luecken as f32 } else { 0.0 }
        } else {
            0.0
        };
        let polster_l = daten.stil.padding.left.resolve_or(breite, 0.0);
        let polster_r = daten.stil.padding.right.resolve_or(breite, 0.0);
        // Die Zeilenbox trägt Absatzabstand (oben nur an der ersten Zeile),
        // Füllung und den linken Rand. Release 07.10.2026: der linke Rand
        // galt vorher nur für die ERSTE Zeile — Folgezeilen eines Absatzes
        // mit `margin-left` standen ausgerückt. `versatz` schiebt eine Zeile
        // neben einen linken Float.
        let rand_links = spanne_links(&daten.stil.margin) + versatz;
        // Release 15.10.2026 (Agent CSS2): not when the lines hang under the
        // element's own box (its style has the same opacity) — that box carries it
        // already, and it counts once (CSS Color 4 §3.2). MEASURED before
        // (Agent C via the coordinator; test `release_1015_opacity`): text in
        // `opacity: 0.5` was drawn at 25 %.
        let eigener_kasten = daten.stil.deckkraft < 1.0
            && ziel.doc.get(eltern).is_some_and(|n| (n.style.deckkraft - daten.stil.deckkraft).abs() < 1e-6);
        let stil = Style {
            // Release 08.10.2026: die Deckkraft des Absatzes gehoert seiner
            // Zeile (vorher fest 1.0 — `p { opacity: .5 }` wirkte nie, und
            // weggeschnittene Screenreader-Ueberschriften standen im Bild).
            deckkraft: if eigener_kasten { 1.0 } else { daten.stil.deckkraft },
            radius: [0.0; 4],
            schatten_anzahl: 0,
            dekoration: 0,
            size: Dimension {
                width: if ausgerichtet || versatz > 0.0 {
                    Length::px(breite + polster_l + polster_r)
                } else {
                    Length::Auto
                },
                height: Length::px(zeilen_hoehe),
            },
            margin: Spacing {
                top: if i == 0 { daten.stil.margin.top } else { Length::px(0.0) },
                right: Length::px(0.0),
                bottom: Length::px(0.0),
                left: Length::px(rand_links),
            },
            padding: daten.stil.padding,
            direction: Direction::Row,
            background: Color::TRANSPARENT,
            ..Style::default()
        };
        let Ok(id) = ziel.doc.push_child(eltern, NodeKind::Container, None, Span::EMPTY) else {
            break;
        };
        if let Some(ord) = laufender_ord {
            ziel.dom_zu_ord.entry(id.index() as u32).or_insert(ord);
        }
        let _ = ziel.doc.set_style(id, stil);
        if ausgerichtet {
            ziel.layout_plan.set(id, narsil_layout::LayoutStil::default());
            ziel.layout_plan.set_ausrichtung(
                id,
                match ausrichtung {
                    Some(narsil_css::Textausrichtung::Zentriert) => narsil_layout::Textausrichtung::Zentriert,
                    Some(narsil_css::Textausrichtung::Links) => narsil_layout::Textausrichtung::Links,
                    _ => narsil_layout::Textausrichtung::Rechts,
                },
            );
        }
        boxen.push(id);
        // Release 08.10.2026: der zuletzt geschriebene Lauf, wenn er nur
        // Leerraum ist — ein `::after` gehoert VOR ihn (an sein Element).
        let mut letzter_leer: Option<NodeId> = None;
        for (lauf_nr, (text, s)) in runs.iter().enumerate() {
            if text.is_empty() {
                continue;
            }
            // Release 08.10.2026: Pseudokaesten, die VOR diesem Abschnitt
            // stehen. Ein `::after` am Zeilenanfang gehoert ans Ende der
            // vorigen Zeile (dort endet sein Element).
            while let Some(&(k, knoten, nach)) = daten.anker.get(anker_i)
                && k <= *s
            {
                let zeilen_anfang = ziel.doc.get(id).is_some_and(|n| n.children.is_empty());
                let ziel_zeile = match boxen.len() {
                    n if nach && zeilen_anfang && n >= 2 => boxen[n - 2],
                    _ => id,
                };
                let vor = if nach && ziel_zeile == id { letzter_leer } else { None };
                anker_einhaengen_vor(ziel.doc, ziel_zeile, knoten, vor);
                // Mit Planeintrag geht die Zeile durch `kind_platzieren`
                // (wie eine ausgerichtete Zeile), und dort gilt `position`.
                if ziel.layout_plan.hole(ziel_zeile).is_none() {
                    ziel.layout_plan.set(ziel_zeile, narsil_layout::LayoutStil::default());
                }
                anker_i += 1;
            }
            let Some((grund_stil, href, abschnitt_ord)) = abschnitte.get(*s) else {
                continue;
            };
            let mut stil = grund_stil.clone();
            stil.size = Dimension::AUTO;
            // Release 15.10.2026 (Agent CSS2): `::first-line` on the first
            // line, for runs whose colour is the block's (inherited). MEASURED
            // before (WPT `CSS2/selectors/first-line-pseudo-007`, `-018`):
            // `::first-line` was not read.
            if i == 0
                && let Some(c) = daten.first_line_color
                && block_section(daten).and_then(|b| abschnitte.get(b)).is_some_and(|b| b.0.color == stil.color)
            {
                stil.color = c;
            }
            // Release 13.10.2026 (Agent CSS2): siehe `blocksatz` oben.
            if lauf_nr >= blocksatz_ab {
                stil.wort_abstand += blocksatz;
            }
            // Ein Abschnitt ohne eigenes Element (der Absatztext selbst) trägt
            // keine Box-Eigenschaften — die gehören der Zeilenbox.
            if abschnitt_ord.is_none() {
                stil.margin = Spacing::ZERO;
                stil.padding = Spacing::ZERO;
                stil.background = Color::TRANSPARENT;
                stil.rahmen = Default::default();
                stil.verlauf = None;
                stil.radius = [0.0; 4];
                stil.schatten_anzahl = 0;
                stil.deckkraft = 1.0;
            }
            // Vertikal trägt die Zeilenbox; horizontal bleibt der Rand eines
            // Inline-Elements Platz in der Zeile (`nav a { margin-right }`).
            stil.margin = Spacing {
                top: Length::px(0.0),
                right: stil.margin.right,
                bottom: Length::px(0.0),
                left: stil.margin.left,
            };
            // Release 12.10.2026 (Agent CSS2): siehe `zeilen_von` oben.
            if abschnitt_ord.is_some() {
                let (erste, letzte) = zeilen_von.get(*s).copied().unwrap_or((i, i));
                if !(erste == i && element_ende(*s, true)) {
                    stil.margin.left = Length::px(0.0);
                    stil.padding.left = Length::px(0.0);
                    stil.rahmen.breite[3] = 0.0;
                }
                if !(letzte == i && element_ende(*s, false)) {
                    stil.margin.right = Length::px(0.0);
                    stil.padding.right = Length::px(0.0);
                    stil.rahmen.breite[1] = 0.0;
                }
            }
            let Ok(tid) = ziel.doc.push_child(id, NodeKind::Text, Some(text.clone()), Span::EMPTY) else {
                break;
            };
            // Release 13.10.2026 (Agent CSS2): siehe `auslassung_von_abschnitt`.
            let abschnitt_ord = match auslassung_von_abschnitt.filter(|_| lauf_nr + 1 == runs.len()) {
                Some(a) => abschnitte.get(a).and_then(|x| x.2).or(*abschnitt_ord),
                None => *abschnitt_ord,
            };
            if let Some(ord) = abschnitt_ord.or(laufender_ord) {
                ziel.dom_zu_ord.entry(tid.index() as u32).or_insert(ord);
            }
            let _ = ziel.doc.set_style(tid, stil);
            if let Some(adresse) = href.as_ref() {
                ziel.links.insert(tid, adresse.clone());
            }
            letzter_leer = text.chars().all(css_leerraum).then_some(tid);
            texte.push((tid, text.clone()));
        }
    }
    // Release 08.10.2026: was hinter dem letzten Text steht (`::after` am
    // Absatzende), kommt ans Ende der letzten Zeile.
    if let Some(&letzte) = boxen.last() {
        for &(_, knoten, _) in daten.anker.iter().skip(anker_i) {
            anker_einhaengen(ziel.doc, letzte, knoten);
            if ziel.layout_plan.hole(letzte).is_none() {
                ziel.layout_plan.set(letzte, narsil_layout::LayoutStil::default());
            }
        }
    }
    // Der untere Absatzabstand gehört an die LETZTE Zeile.
    if let Some(&letzte) = boxen.last()
        && let unten = spanne_unten(&daten.stil.margin)
        && unten > 0.0
        && let Ok(alter) = ziel.doc.node(letzte).map(|n| n.style.clone())
    {
        let mut st = alter;
        st.margin.bottom = Length::px(unten);
        let _ = ziel.doc.set_style(letzte, st);
    }
    (boxen, texte)
}

/// Release 12.10.2026 (Agent CSS2): die Zeilen eines Absatzes im Nachumbruch
/// (Breite je Zeile neben den Floats) — fuer `absatz_neu_umbrechen` und
/// `absatz_zeilen_anzahl`.
fn nachumbruch_zeilen(
    daten: &AbsatzDaten,
    messung: &narsil_layout::Messung,
    breiten: ZeilenBreite<'_>,
) -> Vec<Vec<(String, usize, bool)>> {
    // Release 13.10.2026 (Agent CSS2): mit `text-overflow` gelten die echten
    // Breiten (siehe `Wort::unteilbar`).
    if daten.auslassung.is_some() {
        return absatz_zeilen(daten, messung, breiten);
    }
    let breitestes = breitestes_wort(daten, messung, false);
    // Release 12.10.2026 (Agent CSS2): eine Zeile neben Floats, die schmaler
    // ist als das breiteste Wort, behaelt ihre Breite, wenn weiter unten eine
    // Zeile breit genug ist — passt ein Wort nicht hinein, rueckt die Zeile
    // nach unten (CSS 2.1 §9.5: "the line box is shifted downward until either
    // some content fits or there are no more floats present"; `zeilen_bauen`
    // laesst dafuer Leerzeilen). Sonst wie bisher: nie schmaler als das
    // breiteste Wort.
    // Ziel ist die erste Zeile, in die das Wort passt, hoechstens eine ohne
    // Floats (die volle Breite weit unten).
    let ziel = breitestes.min(breiten(1 << 20).1);
    let geklemmt = |k: usize| {
        let (v, b) = breiten(k);
        if b + 0.5 < ziel && (1..=64).any(|j| breiten(k + j).1 + 0.5 >= ziel) {
            (v, b)
        } else {
            (v, b.max(breitestes))
        }
    };
    let breiten: ZeilenBreite<'_> = &geklemmt;
    absatz_zeilen(daten, messung, breiten)
}

/// Release 12.10.2026 (Agent CSS2): wie viele Zeilen der Absatz `index` mit
/// diesen Breiten bekaeme — fuer die Hoehenschaetzung im Nachumbruch
/// (`narsil::nachumbruch`), die vorher nur das Breitenverhaeltnis kannte.
pub fn absatz_zeilen_anzahl(
    seite: &Seite,
    index: usize,
    messung: &narsil_layout::Messung,
    breiten: ZeilenBreite<'_>,
) -> Option<usize> {
    let satz = seite.absaetze.get(index)?;
    Some(nachumbruch_zeilen(&satz.daten, messung, breiten).len())
}

/// Release 07.10.2026, NACHUMBRUCH: bricht den Absatz `index` mit den
/// gemessenen Breiten neu um und ersetzt seine Zeilenboxen an Ort und
/// Stelle. Gibt `false` zurück, wenn nichts zu tun war.
///
/// `breiten(i)` liefert für Zeile `i` den Versatz vom linken Inhaltsrand
/// des Absatzes (neben einem linken Float) und die verfügbare Breite.
pub fn absatz_neu_umbrechen(
    seite: &mut Seite,
    index: usize,
    messung: &narsil_layout::Messung,
    breiten: ZeilenBreite<'_>,
) -> bool {
    let Some(satz) = seite.absaetze.get(index).cloned() else {
        return false;
    };
    let Some(alte_position) = seite
        .document
        .get(satz.knoten)
        .and_then(|n| n.children.iter().position(|c| satz.zeilen.first() == Some(c)))
    else {
        return false;
    };
    let zeilen = nachumbruch_zeilen(&satz.daten, messung, breiten);
    if zeilen.is_empty() {
        return false;
    }
    let mut ziel = ZeilenZiel {
        doc: &mut seite.document,
        links: &mut seite.links,
        dom_zu_ord: &mut seite.dom_zu_ord,
        layout_plan: &mut seite.layout_plan,
    };
    let (neue_boxen, _) = zeilen_schreiben(&mut ziel, satz.knoten, &satz.daten, &zeilen, breiten, messung);
    if neue_boxen.is_empty() {
        return false;
    }
    // Die alten Zeilen abhängen und ihren Text leeren: wer über die Arena
    // läuft (Suche, Tests), sieht sonst den Absatz doppelt.
    let alte: std::collections::HashSet<NodeId> = satz.zeilen.iter().copied().collect();
    let mut alte_texte: Vec<NodeId> = Vec::new();
    for z in &satz.zeilen {
        if let Some(n) = seite.document.get(*z) {
            alte_texte.extend(n.children.iter().copied());
        }
    }
    for t in &alte_texte {
        let _ = seite.document.set_text(*t, String::new());
        seite.links.remove(t);
    }
    let neu_set: std::collections::HashSet<NodeId> = neue_boxen.iter().copied().collect();
    let mut kinder: Vec<NodeId> = Vec::new();
    if let Some(n) = seite.document.get(satz.knoten) {
        for (i, c) in n.children.iter().enumerate() {
            if i == alte_position {
                kinder.extend(neue_boxen.iter().copied());
            }
            if alte.contains(c) || neu_set.contains(c) {
                continue;
            }
            kinder.push(*c);
        }
    }
    if seite.document.kinder_setzen(satz.knoten, kinder).is_err() {
        return false;
    }
    // `getElementById` auf einen Knoten des alten Absatzes zeigt jetzt auf
    // die erste neue Zeile.
    let erste = neue_boxen[0];
    for v in seite.id_zu_knoten.values_mut() {
        if alte.contains(v) || alte_texte.contains(v) {
            *v = erste;
        }
    }
    if let Some(s) = seite.absaetze.get_mut(index) {
        s.zeilen = neue_boxen;
        s.breite = breiten(0).1;
    }
    true
}

trait HorizontaleRaender {
    fn eigenschaft(&self) -> (f32, f32);
}

impl HorizontaleRaender for Style {
    /// Release 12.10.2026 (Agent CSS2): der waagrechte Platz eines Inline-
    /// Elements an seinen Enden — Rand (auch negativ), Innenabstand und
    /// Rahmen (CSS 2.1 §8.1, §9.4.2: alle drei liegen in der Zeile). Das
    /// Layout setzt sie schon so (`flow::inline_teil`); der Umbruch hier
    /// rechnete nur positive Raender. GEMESSEN vorher (WPT
    /// `css1/c5507-ipadn-r-*`, `c5509-ipadn-l-*`, `c5504-imrgn-l-*`): ein
    /// Span mit `padding-right: 4em` blieb in einer 5-em-Zeile.
    fn eigenschaft(&self) -> (f32, f32) {
        let px = |l: Length| match l {
            Length::Px(v) if v.is_finite() => v,
            _ => 0.0,
        };
        let innen = |l: Length| px(l).max(0.0);
        (
            px(self.margin.left) + innen(self.padding.left) + self.rahmen.gezeichnete_breite(3),
            px(self.margin.right) + innen(self.padding.right) + self.rahmen.gezeichnete_breite(1),
        )
    }
}

/// Die Woerter eines Textes, JEDES mit der Angabe, ob davor Weissraum
/// stand.
///
/// WARUM EIGENSTATT `split_whitespace`: das entfernt den Weissraum und
/// damit die Information. Diese Funktion haelt sie fest — und sie
/// enthaelt die Abschnitte NICHT: `weissraum` hat sie schon
/// zusammengefasst, es gibt also keine Laeufe von mehr als einem
/// Leerzeichen mehr.
/// (Woerter mit "Leerzeichen davor", endet der Text mit Weissraum?)
///
/// DER ZWEITE WERT ist nicht verzichtbar: er beantwortet die Frage an
/// der Abschnittsgrenze. `…<b>normaler</b> Absatz…` — der Abschnitt
/// "` Absatz`" beginnt mit einem Leerzeichen, das im TEXT dieses
/// Abschnitts steht; aber `<b>Link</b>. Er` endet der Abschnitt ohne
/// Leerzeichen, und der Punkt darf KEINEN bekommen. Beides entscheidet
/// der zweite Wert.
/// Höchstzahl gesammelter SVG-Definitionen je Seite (M3).
const MAX_SVG_DEFINITIONEN: usize = 4096;

/// Release 08.10.2026: Merkattribut am Bildknoten eines Inline-SVG, das nur
/// eine `viewBox` (keine `width`/`height`) hat — es hat dann keine
/// Eigengroesse und fuellt die Breite seines Containers (SVG 2 §8.2).
const SVG_FUELLT: &str = "data-narsil-svg-fuellt";

/// Release 10.10.2026: die `viewBox`-Masse eines Inline-SVG ohne
/// `width`/`height` („B H") — sie geben dem Bild sein SEITENVERHAELTNIS
/// (SVG 2 §8.2: keine Eigengroesse, aber ein Verhaeltnis). GEMESSEN vorher
/// (heise.de): `<svg class="h-8" viewBox="0 0 116 32">` in einer Flex-Zeile
/// hatte in der Messphase die volle Breite, der Text daneben schrumpfte auf
/// ein Wort je Zeile (Chrome: 116 px breit).
const SVG_VERHAELTNIS: &str = "data-narsil-svg-verhaeltnis";
/// Release 15.10.2026 (Agent CSS2): internal attribute — the `img` stands for
/// an inline `<svg>`.
const SVG_INLINE: &str = "data-narsil-svg-inline";

/// Sammelt alle `id`-tragenden Elemente INNERHALB von `<svg>` (Symbole,
/// Gruppen, Pfade, Verläufe) — die Ziele von `<use href="#…">`.
fn svg_definitionen_sammeln<'k>(
    knoten: &'k [HtmlNode],
    im_svg: bool,
    tiefe: usize,
    raus: &mut HashMap<String, &'k HtmlNode>,
) {
    if tiefe > MAX_BAU_TIEFE {
        return;
    }
    for k in knoten {
        let HtmlNode::Element { tag, attributes, children, .. } = k else {
            continue;
        };
        let ist_svg = tag.eq_ignore_ascii_case("svg");
        if (im_svg || ist_svg)
            && raus.len() < MAX_SVG_DEFINITIONEN
            && let Some(id) = attributes.get("id")
            && !id.is_empty()
        {
            raus.entry(id.clone()).or_insert(k);
        }
        svg_definitionen_sammeln(children, im_svg || ist_svg, tiefe + 1, raus);
    }
}

/// Die SVG-Eigenschaften aus dem berechneten CSS-Stil als Attribute (CSS hat
/// Vorrang vor Präsentationsattributen, SVG 2 §6.6); `currentColor` wird mit
/// der berechneten Farbe aufgelöst.
fn svg_css_attribute(attr: &mut HashMap<String, String>, stil: &narsil_css::Berechnet) {
    // Release 15.10.2026 (Agent F): every SVG property, not only the paint
    // ones: the SVG renderer (narsil-svg) reads markers, clip paths, masks,
    // filters and the text properties, too.
    const WEITERE: [&str; 13] = [
        "marker-start",
        "marker-mid",
        "marker-end",
        "clip-path",
        "mask",
        "filter",
        "mix-blend-mode",
        "dominant-baseline",
        "letter-spacing",
        "word-spacing",
        "text-decoration",
        "color-interpolation-filters",
        "isolation",
    ];
    let farbe = format!("#{:02x}{:02x}{:02x}", stil.farbe.r, stil.farbe.g, stil.farbe.b);
    // The font of the page cascade, computed (the `font` shorthand
    // included).
    if stil.schluessel.iter().any(|(n, _)| n == "font" || n.starts_with("font-")) {
        svg_font_attribute(attr, stil, false);
    }
    for (name, wert) in &stil.schluessel {
        let name_s = name.as_str();
        if name_s.starts_with("font") || !(SVG_PRESENTATION_ATTRIBUTES.contains(&name_s) || WEITERE.contains(&name_s)) {
            continue;
        }
        // Release 08.10.2026: `var()` VOR dem Weitergeben aufloesen — der
        // SVG-Renderer kennt keine Variablen. GEMESSEN: das MDN-Logo trug
        // `fill="var(--csstools-light-dark-toggle-…,var(--color-blue-50))"`
        // und blieb unsichtbar. Unaufloesbar: das Praesentationsattribut
        // bleibt stehen.
        let Some(wert) = narsil_css::ersetze_var(wert.trim(), &stil.variablen, 0) else {
            continue;
        };
        let wert = wert.trim();
        if wert.is_empty() || wert.contains("var(") {
            continue;
        }
        // `currentColor` in the paints becomes the color; elsewhere it stays
        // (the renderer resolves it, and it taints filter primitives).
        let wert = if matches!(name_s, "fill" | "stroke") && wert.eq_ignore_ascii_case("currentcolor") { farbe.clone() } else { wert.to_string() };
        attr.insert(name.clone(), wert);
    }
    // `color` vererbt sich in das SVG — `fill="currentColor"` liest sie.
    for name in ["fill", "stroke"] {
        if attr.get(name).is_some_and(|w| w.trim().eq_ignore_ascii_case("currentcolor")) {
            attr.insert(name.to_string(), farbe.clone());
        }
    }
}

/// The computed font of the page cascade as SVG presentation attributes.
/// `nur_fehlende`: an attribute the element has wins (the root `<svg>`
/// inherits the page font, its own attributes override it).
fn svg_font_attribute(attr: &mut HashMap<String, String>, stil: &narsil_css::Berechnet, nur_fehlende: bool) {
    let kursiv = matches!(stil.schriftstil, narsil_css::Schriftstil::Kursiv);
    let werte = [
        ("font-size", format!("{}px", stil.schriftgroesse_px)),
        ("font-family", stil.schriftfamilie.clone()),
        ("font-weight", stil.schriftgewicht.to_string()),
        ("font-style", if kursiv { "italic" } else { "normal" }.to_string()),
    ];
    for (name, wert) in werte {
        if wert.is_empty() || (nur_fehlende && attr.contains_key(name)) {
            continue;
        }
        attr.insert(name.to_string(), wert);
    }
}

/// Release 15.10.2026 (Agent F): one `mask-image` layer of a box.
#[derive(Debug, Clone, PartialEq)]
pub enum MaskenEbene {
    /// An image, placed like a background image (`mask-size`,
    /// `mask-position`, `mask-repeat`; border box origin); its alpha masks.
    Bild { url: String, geo: narsil_css::BildGeo },
    /// A `<mask>` of the page (`url(#id)`) as a small SVG document; its
    /// luminance (or alpha, `mask-type`) masks.
    Svg { quelle: std::sync::Arc<str>, verweis: String },
    /// A gradient over the border box; its alpha masks.
    Verlauf(narsil_core::verlauf_typ::Verlauf),
    /// `url(#id)` before the page builder resolved it (see `Svg`).
    Verweis(String),
}

/// Release 15.10.2026 (Agent F): the visible mask layers of `masken`
/// (`none` layers count as transparent and are left out); `url(#id)` stays
/// a `Verweis` for the page builder (see `Bauer::masken_aufloesen`).
fn masken_ebenen(masken: &[narsil_css::Maske], basis: &str, stil: Option<&narsil_css::Berechnet>) -> Vec<MaskenEbene> {
    let mut raus = Vec::new();
    for m in masken.iter().take(8) {
        let url = m.url.trim();
        if let Some(id) = url.strip_prefix('#') {
            raus.push(MaskenEbene::Verweis(id.to_string()));
            continue;
        }
        if !url.is_empty() {
            // `mask-origin` and `mask-clip` start at the border box (CSS
            // Masking 1 §7.4, §7.5).
            let mut geo = match stil {
                Some(stil) => bild_geo_mit_rand(bild_geo_aus_maske(m), stil),
                None => bild_geo_aus_maske(m),
            };
            geo.ursprung = 1;
            geo.bild_clip = 0;
            raus.push(MaskenEbene::Bild { url: crate::loese_link(url, basis), geo });
        } else if let Some(v) = m.verlauf.as_ref().and_then(kaskade::verlauf_von_css) {
            raus.push(MaskenEbene::Verlauf(v));
        }
    }
    raus
}

/// Presentation attributes whose values may carry `var()`: they are parsed
/// as CSS values (SVG 2 §6.6), so browsers substitute custom properties in
/// them. The SVG image renderer knows no variables, so they are resolved
/// here against the element's computed custom properties.
const SVG_PRESENTATION_ATTRIBUTES: [&str; 27] = [
    "fill",
    "stroke",
    "stroke-width",
    "fill-opacity",
    "stroke-opacity",
    "opacity",
    "fill-rule",
    "clip-rule",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-miterlimit",
    "stop-color",
    "stop-opacity",
    "color",
    "flood-color",
    "flood-opacity",
    "lighting-color",
    "font-size",
    "font-family",
    "font-weight",
    "visibility",
    "display",
    "paint-order",
    "vector-effect",
    "text-anchor",
];

/// Substitutes `var()` in presentation attributes (theguardian.com: the
/// masthead logo is `<svg fill="var(--masthead-nav-link-text)">` and stayed
/// invisible). An attribute that cannot be resolved is invalid at
/// computed-value time and is dropped, so the property inherits.
fn svg_resolve_attribute_vars(attr: &mut HashMap<String, String>, vars: &narsil_css::Variablen) {
    for name in SVG_PRESENTATION_ATTRIBUTES {
        let Some(value) = attr.get(name) else { continue };
        if !value.contains("var(") {
            continue;
        }
        match narsil_css::ersetze_var(value.trim(), vars, 0).map(|v| v.trim().to_string()) {
            Some(v) if !v.is_empty() && !v.contains("var(") => {
                attr.insert(name.to_string(), v);
            }
            _ => {
                attr.remove(name);
            }
        }
    }
}

/// Die Definitionen, auf die `knoten` per `<use href="#id">` (oder
/// `xlink:href`) verweist — rekursiv, ohne die lokal schon definierten.
fn svg_definitionen_fuer<'k>(
    knoten: &[HtmlNode],
    alle: &HashMap<String, &'k HtmlNode>,
) -> Vec<&'k HtmlNode> {
    fn verweise(knoten: &[HtmlNode], tiefe: usize, ids: &mut Vec<String>, lokal: &mut Vec<String>) {
        if tiefe > 64 {
            return;
        }
        for k in knoten {
            let HtmlNode::Element { tag, attributes, children, .. } = k else {
                continue;
            };
            if let Some(id) = attributes.get("id") {
                lokal.push(id.clone());
            }
            if tag.eq_ignore_ascii_case("use") {
                let ziel = attributes.get("href").or_else(|| attributes.get("xlink:href"));
                if let Some(ziel) = ziel.and_then(|z| z.trim().strip_prefix('#')) {
                    ids.push(ziel.to_string());
                }
            }
            verweise(children, tiefe + 1, ids, lokal);
        }
    }
    let mut lokal = Vec::new();
    let mut offen = Vec::new();
    verweise(knoten, 0, &mut offen, &mut lokal);
    let mut raus: Vec<&'k HtmlNode> = Vec::new();
    let mut gesehen: std::collections::HashSet<String> = lokal.into_iter().collect();
    while let Some(id) = offen.pop() {
        if raus.len() >= 64 || !gesehen.insert(id.clone()) {
            continue;
        }
        if let Some(def) = alle.get(&id) {
            let mut innen = Vec::new();
            verweise(std::slice::from_ref(*def), 0, &mut offen, &mut innen);
            gesehen.extend(innen);
            raus.push(*def);
        }
    }
    raus
}

/// Ein Zeichen, nach dem ohne Leerraum umbrochen werden darf: CJK-Ideogramme,
/// Kana, Hangul und die vollbreiten Satzzeichen (UAX #14 ID/CJ, vereinfacht).
fn ist_cjk_umbruchzeichen(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FFF      // Radikale
        | 0x3000..=0x30FF    // CJK-Satzzeichen, Hiragana, Katakana
        | 0x3100..=0x31FF    // Bopomofo, Kana-Erweiterung
        | 0x3400..=0x4DBF    // Erweiterung A
        | 0x4E00..=0x9FFF    // Einheitliche Ideogramme
        | 0xAC00..=0xD7AF    // Hangul-Silben
        | 0xF900..=0xFAFF    // Kompatibilitaetsideogramme
        | 0xFF00..=0xFF60    // Vollbreite Formen
        | 0x20000..=0x3FFFF  // Erweiterungen B–H
    )
}

/// Release 11.10.2026: Zeilenumbruch nach UAX #14 (Auswahl) — zwischen zwei
/// Zeichen ohne Leerraum darf umbrochen werden, wenn eines CJK ist (ID/CJ),
/// ausser NACH einer oeffnenden Klammer (OP, LB14) und VOR einer
/// schliessenden Klammer, einem Ausrufe-/Trennzeichen oder einem nicht
/// beginnenden Zeichen (CL/CP/EX/IS/NS, LB13/LB21). GEMESSEN vorher (WPT
/// `css3-text-line-break-opclns-*`, 157 Dateien): `中中(文` brach nach „(".
/// CSS Text 3 §5.2: ist das Schriftsystem der Sprachangabe Chinesisch oder
/// Japanisch? Die Sprache (`ja`, `zh-TW`) oder die Schrift (`en-Hrkt`).
fn schreibt_chinesisch_oder_japanisch(lang: &str) -> bool {
    let mut teile = lang.trim().split(['-', '_']);
    let sprache = teile.next().unwrap_or("");
    sprache.eq_ignore_ascii_case("ja")
        || sprache.eq_ignore_ascii_case("zh")
        || teile.any(|t| {
            ["Hrkt", "Jpan", "Hira", "Kana", "Hani", "Hans", "Hant"]
                .iter()
                .any(|s| t.eq_ignore_ascii_case(s))
        })
}

fn darf_umbrechen(vorher: char, c: char, stil: &Style) -> bool {
    // UAX #14 LB8: nach einem Leerzeichen ohne Breite (ZW) immer.
    if vorher == '\u{200B}' {
        return true;
    }
    // Release 12.10.2026 (Agent CSS2): UAX #14 LB21 — nach einem Zeichen der
    // Klasse BA (Leerzeichen wie U+2004, Trenn- und Satzzeichen wie U+2010,
    // U+058A, Danda) und nach HY (`-`, nicht vor Ziffern, LB25) darf
    // umbrochen werden — nicht vor GL/WJ/CM/ZWJ und Schliessendem (LB11, LB12,
    // LB13, LB9). U+00AD (weiches Trennzeichen) bleibt aussen vor, solange der
    // Trennstrich am Zeilenende nicht gezeichnet wird. Die Klassen kommen aus
    // `narsil_text::unicode_daten::linebreak_klassen`. GEMESSEN vorher
    // (Koordinator, WPT `css-text/i18n/css3-text-line-break-baspglwj-*`):
    // `aaa\u{2004}bbb` und `aaa\u{058A}bbb` in 50ch brachen nicht um.
    {
        use narsil_text::unicode_daten::linebreak_klassen::{Umbruchklasse as K, umbruch_klasse};
        let (kv, kc) = (umbruch_klasse(vorher), umbruch_klasse(c));
        let danach_erlaubt = match kv {
            // Release 15.10.2026 (Agent CSS2): after a soft hyphen unless
            // `hyphens: none` (CSS Text 3 §6.1).
            K::Ba => vorher != '\u{00AD}' || !stil.hyphens_none,
            K::Hy => kc != K::Nu,
            _ => false,
        };
        if danach_erlaubt
            && !matches!(kc, K::Gl | K::Wj | K::Cm | K::Zwj | K::Ba | K::Hy | K::Cl | K::Cp | K::Ex | K::Is | K::Sy | K::Ns)
        {
            return true;
        }
    }
    if !(ist_cjk_umbruchzeichen(vorher) || ist_cjk_umbruchzeichen(c)) || in_bereichen(KEIN_UMBRUCH_DANACH, vorher) {
        return false;
    }
    if !in_bereichen(KEIN_UMBRUCH_DAVOR, c) {
        return true;
    }
    // CSS Text 3 §5.2 (`line-break`): bei Chinesisch/Japanisch darf vor den
    // Bindestrichen ‐ – 〜 ゠ umbrochen werden; `loose` erlaubt es vor ‐ –
    // nach einem Ideogramm in jeder Sprache, dazu vor Iterationszeichen und
    // (bei Chinesisch/Japanisch) vor den mittigen Satzzeichen.
    let bindestrich = matches!(c, '\u{2010}' | '\u{2013}' | '\u{301C}' | '\u{30A0}');
    if stil.umbruch_locker && matches!(c, '\u{2010}' | '\u{2013}') && ist_cjk_umbruchzeichen(vorher) {
        return true;
    }
    let iteration = matches!(c, '\u{3005}' | '\u{303B}' | '\u{309D}' | '\u{309E}' | '\u{30FD}' | '\u{30FE}');
    let mittig = matches!(
        c,
        '\u{30FB}' | '\u{FF1A}' | '\u{FF1B}' | '\u{FF65}' | '\u{203C}' | '\u{2047}' | '\u{2048}' | '\u{2049}' | '\u{FF01}' | '\u{FF1F}'
    );
    (bindestrich && stil.cjk_sprache) || (iteration && stil.umbruch_locker) || (mittig && stil.umbruch_locker && stil.cjk_sprache)
}

fn in_bereichen(tabelle: &[(u32, u32)], c: char) -> bool {
    let n = c as u32;
    tabelle
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

/// UAX #14 OP (General_Category Ps): kein Umbruch DANACH.
const KEIN_UMBRUCH_DANACH: &[(u32, u32)] = &[
    (0x28, 0x28), (0x5B, 0x5B), (0x7B, 0x7B), (0xF3A, 0xF3A), (0xF3C, 0xF3C), (0x169B, 0x169B),
    (0x201A, 0x201A), (0x201E, 0x201E), (0x2045, 0x2045), (0x207D, 0x207D), (0x208D, 0x208D),
    (0x2308, 0x2308), (0x230A, 0x230A), (0x2329, 0x2329), (0x2768, 0x2768), (0x276A, 0x276A),
    (0x276C, 0x276C), (0x276E, 0x276E), (0x2770, 0x2770), (0x2772, 0x2772), (0x2774, 0x2774),
    (0x27C5, 0x27C5), (0x27E6, 0x27E6), (0x27E8, 0x27E8), (0x27EA, 0x27EA), (0x27EC, 0x27EC),
    (0x27EE, 0x27EE), (0x2983, 0x2983), (0x2985, 0x2985), (0x2987, 0x2987), (0x2989, 0x2989),
    (0x298B, 0x298B), (0x298D, 0x298D), (0x298F, 0x298F), (0x2991, 0x2991), (0x2993, 0x2993),
    (0x2995, 0x2995), (0x2997, 0x2997), (0x29D8, 0x29D8), (0x29DA, 0x29DA), (0x29FC, 0x29FC),
    (0x2E22, 0x2E22), (0x2E24, 0x2E24), (0x2E26, 0x2E26), (0x2E28, 0x2E28), (0x2E42, 0x2E42),
    (0x2E55, 0x2E55), (0x2E57, 0x2E57), (0x2E59, 0x2E59), (0x2E5B, 0x2E5B), (0x3008, 0x3008),
    (0x300A, 0x300A), (0x300C, 0x300C), (0x300E, 0x300E), (0x3010, 0x3010), (0x3014, 0x3014),
    (0x3016, 0x3016), (0x3018, 0x3018), (0x301A, 0x301A), (0x301D, 0x301D), (0xFD3F, 0xFD3F),
    (0xFE17, 0xFE17), (0xFE35, 0xFE35), (0xFE37, 0xFE37), (0xFE39, 0xFE39), (0xFE3B, 0xFE3B),
    (0xFE3D, 0xFE3D), (0xFE3F, 0xFE3F), (0xFE41, 0xFE41), (0xFE43, 0xFE43), (0xFE47, 0xFE47),
    (0xFE59, 0xFE59), (0xFE5B, 0xFE5B), (0xFE5D, 0xFE5D), (0xFF08, 0xFF08), (0xFF3B, 0xFF3B),
    (0xFF5B, 0xFF5B), (0xFF5F, 0xFF5F), (0xFF62, 0xFF62),
];

/// UAX #14 CL/CP (Pe), EX, IS, NS (Auswahl): kein Umbruch DAVOR.
const KEIN_UMBRUCH_DAVOR: &[(u32, u32)] = &[
    (0x21, 0x21), (0x29, 0x29), (0x2C, 0x2E), (0x3A, 0x3B), (0x3F, 0x3F),
    (0x5D, 0x5D), (0x7D, 0x7D), (0xE5A, 0xE5B), (0xF3B, 0xF3B), (0xF3D, 0xF3D),
    (0x169C, 0x169C), (0x17D4, 0x17D4), (0x17D6, 0x17D6), (0x17DA, 0x17DA), (0x2010, 0x2010), (0x2012, 0x2013), (0x203C, 0x203D),
    (0x2046, 0x2049), (0x207E, 0x207E), (0x208E, 0x208E), (0x2309, 0x2309), (0x230B, 0x230B),
    (0x232A, 0x232A), (0x2769, 0x2769), (0x276B, 0x276B), (0x276D, 0x276D), (0x276F, 0x276F),
    (0x2771, 0x2771), (0x2773, 0x2773), (0x2775, 0x2775), (0x27C6, 0x27C6), (0x27E7, 0x27E7),
    (0x27E9, 0x27E9), (0x27EB, 0x27EB), (0x27ED, 0x27ED), (0x27EF, 0x27EF), (0x2984, 0x2984),
    (0x2986, 0x2986), (0x2988, 0x2988), (0x298A, 0x298A), (0x298C, 0x298C), (0x298E, 0x298E),
    (0x2990, 0x2990), (0x2992, 0x2992), (0x2994, 0x2994), (0x2996, 0x2996), (0x2998, 0x2998),
    (0x29D9, 0x29D9), (0x29DB, 0x29DB), (0x29FD, 0x29FD), (0x2E23, 0x2E23), (0x2E25, 0x2E25),
    (0x2E27, 0x2E27), (0x2E29, 0x2E29), (0x2E56, 0x2E56), (0x2E58, 0x2E58), (0x2E5A, 0x2E5A),
    (0x2E5C, 0x2E5C), (0x3001, 0x3002), (0x3005, 0x3005), (0x3009, 0x3009), (0x300B, 0x300B),
    (0x300D, 0x300D), (0x300F, 0x300F), (0x3011, 0x3011), (0x3015, 0x3015), (0x3017, 0x3017),
    (0x3019, 0x3019), (0x301B, 0x301C), (0x301E, 0x301F), (0x303B, 0x303C), (0x309B, 0x309E),
    (0x30A0, 0x30A0), (0x30FB, 0x30FB), (0x30FD, 0x30FE), (0xFD3E, 0xFD3E), (0xFE18, 0xFE18),
    (0xFE36, 0xFE36), (0xFE38, 0xFE38), (0xFE3A, 0xFE3A), (0xFE3C, 0xFE3C), (0xFE3E, 0xFE3E),
    (0xFE40, 0xFE40), (0xFE42, 0xFE42), (0xFE44, 0xFE44), (0xFE48, 0xFE48), (0xFE50, 0xFE50),
    (0xFE52, 0xFE52), (0xFE54, 0xFE57), (0xFE5A, 0xFE5A), (0xFE5C, 0xFE5C), (0xFE5E, 0xFE5E),
    (0xFF01, 0xFF01), (0xFF09, 0xFF09), (0xFF0C, 0xFF0C), (0xFF0E, 0xFF0E), (0xFF1A, 0xFF1B),
    (0xFF1F, 0xFF1F), (0xFF3D, 0xFF3D), (0xFF5D, 0xFF5D), (0xFF60, 0xFF61), (0xFF63, 0xFF65),
    (0xFF9E, 0xFF9F),
];

fn woerter_mit_leerzeichen(text: &str, stil: &Style) -> (Vec<(String, bool)>, bool) {
    // Release 11.10.2026: `white-space: nowrap` (CSS Text 3 §3) — der ganze
    // Abschnitt ist EIN Wort, Leerraumfolgen werden ein Leerzeichen; umbrochen
    // wird nur an seinen Raendern. GEMESSEN vorher: `nowrap` galt nur in der
    // Mindestbreite des Layouts, der Zeilenbau brach trotzdem an jedem
    // Leerzeichen (`<div style="width:130px;white-space:nowrap">` in vier Zeilen).
    if stil.weissraum == narsil_core::style::Weissraum::KeinUmbruch {
        let anfang_leer = text.starts_with(css_leerraum);
        let innen = text.split(css_leerraum).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ");
        if innen.is_empty() {
            return (Vec::new(), !text.is_empty());
        }
        return (vec![(innen, anfang_leer)], text.ends_with(css_leerraum));
    }
    let mut raus: Vec<(String, bool)> = Vec::new();
    let mut aktuell = String::new();
    let mut leer = false;
    let mut vorher: Option<char> = None;
    for c in text.chars() {
        if css_leerraum(c) {
            if !aktuell.is_empty() {
                raus.push((std::mem::take(&mut aktuell), leer));
            }
            leer = true;
            vorher = None;
            continue;
        }
        // Release 08.10.2026: zwischen CJK-Zeichen darf umbrochen werden
        // (UAX #14, Klassen ID/CJ) — jedes ist ein eigenes „Wort" ohne
        // Leerraum davor. GEMESSEN: der chinesische Absatz von example.com
        // lief als EIN Wort ueber die 26-em-Spalte hinaus.
        // Release 15.10.2026 (Agent CSS2): `word-break: break-all` and
        // `line-break: anywhere` add opportunities between letters (see
        // `narsil_text::umbruch::letter_break`); every piece is a word of its
        // own, so the line takes as many letters as fit. Not in `pre`, which
        // never wraps. MEASURED before (WPT css-text `word-break/word-break-
        // break-all-018`, `XXXX&nbsp;XXXX X X` in 4ch): the words only broke
        // at the spaces.
        if let Some(v) = vorher
            && !aktuell.is_empty()
            && (darf_umbrechen(v, c, stil)
                || (stil.weissraum != narsil_core::style::Weissraum::Pre
                    && narsil_text::umbruch::letter_break(v, c, stil.letter_breaks)))
        {
            raus.push((std::mem::take(&mut aktuell), leer));
            leer = false;
        }
        aktuell.push(c);
        vorher = Some(c);
    }
    // Release 07.10.2026, GEMESSENER FEHLER: hier stand `(raus, leer)` — und
    // `leer` bleibt nach dem ersten inneren Leerzeichen wahr. „type safety"
    // meldete darum „endet mit Leerraum", und `<a>type safety</a>, and`
    // bekam eine Lücke vor dem Komma.
    let endet_leer = aktuell.is_empty() && leer;
    if !aktuell.is_empty() {
        raus.push((aktuell, leer));
    }
    (raus, endet_leer)
}

/// Die groesste Zeichengrenze <= `max` (Bytes).
///
/// WARUM EIGENSTATT `str::floor_char_boundary`: das ist erst seit Rust
/// 1.91 stabil, der Workspace nennt `rust-version = 1.85`
/// (`Cargo.toml`). clippy sagt es laut. Und `String::truncate` panict,
/// wenn es mitten in einem Zeichen schneidet — ein `alt`-Text mit einem
/// Umlaut an Byte 1024 und einem Grenzwert von 1024 ist genau der Fall.
pub(crate) fn zeichengrenze(s: &str, max: usize) -> usize {
    if s.len() <= max {
        return s.len();
    }
    let mut i = max;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Der Kaskaden-Zustand des `Bauer`.
///
/// WARUM EIGENE STRUKTUR: `Berechnet` ist ein grosses Objekt (Farben,
/// Abstaende, Flex, Variablen). Auf dem Stapel gehalten — statt je
/// Element neu gerechnet — ist es der Unterschied zwischen O(n) und
/// O(n²) fuer die Vererbung, denn die Vererbung braucht den ELTERNwert.
///
/// DER STAPEL wird in `kind()` gefuellt und geleert, an EINER Stelle.
struct Kaskade {
    /// Der Baum, gegen den die Selektoren geprueft werden.
    baum: CssBaum,
    /// `[ua, <style>…]` in Prioritaet. Die Autorblaetter (die Antwort auf
    /// "hat der Autor das geschrieben?") sind `alle[1..]`.
    ///
    /// Release 10.10.2026: GETEILT zwischen den Neubauten (siehe
    /// [`BlattSatz`]).
    alle: std::sync::Arc<Vec<narsil_css::Stylesheet>>,
    /// Die Ergebnisse der VORFAHREN, der unterste zuerst. Oben steht
    /// immer der naechste Vorfahr des aktuellen Elements.
    werte: Vec<narsil_css::Berechnet>,
    /// GEMINI (30.09. 22:35): die Ansichtshoehe fuer `@media`-Hoehen-
    /// abfragen (`@media (min-height: …)`). 0 heisst „unbekannt" — die
    /// Seite wird breitengetrieben gebaut, und eine geratene Hoehe wuerde
    /// eine `(min-height: 500px)`-Abfrage erfinden. Siehe
    /// `kaskade::autor_stil`.
    ansicht_hoehe: u32,
    /// Die Nummer des Elements, das GERADE gebaut wird.
    ///
    /// Wofuer: `rahmen_von` braucht das Element, um die Regeln zu
    /// pruefen, die zu ihm passen. `steckt` in `Berechnet`, und das ist
    /// die Nummer, nach der `baum.index_von` sucht.
    aktuell: u32,
    /// P7 (02.10.2026): der VORMERKER — der Index, der Regeln nach Tag,
    /// Klasse und id einsortiert, damit nicht jede Regel an jeden Knoten
    /// muss.
    ///
    /// # GEMESSEN, WAS ER BRINGT
    ///
    /// `https://github.com`, Phase `kaskade`, Kaskade-ms je Kasten:
    ///
    /// ```text
    /// debian.org     165 Kaesten   0,067 ms je Kasten
    /// nodejs.org     956 Kaesten   0,149 ms je Kasten
    /// github.com   1470 Kaesten   4,370 ms je Kasten
    /// ```
    ///
    /// github.com ist 29x so teuer je Kasten bei nur 1,5x so grossem
    /// DOM — und 33x so viel CSS (7 270 121 gegen 217 014 Byte ueber
    /// alle Links). Die Zeit folgt der CSS-GROESSE, nicht der Seite.
    ///
    /// # DIE STELLE, DIE ES VORHER KOSTETE
    ///
    /// `narsil-css/src/kaskade.rs`, `berechne_ernst`:
    ///
    /// ```text
    /// 658  for (bi, blatt) in blaetter.iter()
    /// 668  for (ri, regel) in blatt.regeln.iter()
    /// 672  if !kandidat.get(flach).copied().unwrap_or(true) { continue; }
    /// ```
    ///
    /// `kandidat` ist der Vormerker als Bits. `unwrap_or(true)` heisst:
    /// kein Eintrag, also wird DIE REGEL GEPRUEFT. Auf dem Seitenweg gab
    /// es keinen Vormerker, also war es immer `true`.
    ///
    /// `berechnen_mit_vormerker` (kaskade.rs:548) war gebaut, benutzt
    /// wurde es im ganzen Baum NUR vom Benchmark
    /// (`narsil-css/examples/kaskade_leistung.rs:369`) und von EINEM Test
    /// (`narsil-css/tests/k3_fehlererholung.rs:400`). Der Seitenweg nicht.
    /// Die Doku dort sagt ausdruecklich: "der Aufrufer (die Rolle HTML)
    /// baut den Index einmal ueber `Vormerker::bauen`". Das ist genau
    /// diese Stelle.
    ///
    /// # ZWEI VORMERKER, EIN BAU
    ///
    /// `autor_stil` rechnet ueber `author` (ohne UA-Blatt), `end_stil`
    /// ueber `alle` (mit UA-Blatt). Also sind ZWEI Indizes noetig, und
    /// `Vormerker::bauen` laeuft genau zweimal — nicht je Knoten.
    vormerker_author: std::sync::Arc<narsil_css::Vormerker>,
    vormerker_alle: std::sync::Arc<narsil_css::Vormerker>,
    /// Die `::before`/`::after`-Regeln der Seite (06.10.2026).
    ///
    /// GEMESSEN an de.wikipedia.org: 78 Vorkommen, darunter die
    /// Wikipedia-Kugel (`.mw-logo::before`) und die Wortmarke
    /// (`::after`). Die Regeln werden EINMAL je Seite gesammelt und je
    /// Element nur noch geprueft — nicht je Element neu gelesen.
    pseudo_regeln: std::sync::Arc<kaskade::PseudoSatz>,
}

/// Die linke Spanne eines `Spacing` in Pixeln.
///
/// WAS ICH DAMIT VERMEIDE: `Length` ist ein `enum` mit vier Varianten
/// (Px, Prozent, …). Der Umbruch rechnet in PIXEL, also wird hier
/// alles andere zu 0 — und das ist eine Entscheidung, keine
/// Vereinfachung: ein Prozent-Margin ist im Umbruch nicht aufloesbar,
/// weil die Kaskade (H2/I2) noch nicht laeuft. Sobald sie laeuft, ist
/// das die eine Stelle, an der es hingehört.
pub fn spanne_links(m: &Spacing) -> f32 {
    match m.left {
        Length::Px(v) if v.is_finite() && v > 0.0 => v,
        _ => 0.0,
    }
}

/// Die obere Spanne eines `Spacing` in Pixeln. Siehe [`spanne_links`].
pub fn spanne_oben(m: &Spacing) -> f32 {
    match m.top {
        Length::Px(v) if v.is_finite() && v > 0.0 => v,
        _ => 0.0,
    }
}

/// Die untere Spanne eines `Spacing` in Pixeln. Siehe [`spanne_links`].
pub fn spanne_unten(m: &Spacing) -> f32 {
    match m.bottom {
        Length::Px(v) if v.is_finite() && v > 0.0 => v,
        _ => 0.0,
    }
}

/// Die rechte Spanne eines `Spacing` in Pixeln. Siehe [`spanne_links`].
pub fn spanne_rechts(m: &Spacing) -> f32 {
    match m.right {
        Length::Px(v) if v.is_finite() && v > 0.0 => v,
        _ => 0.0,
    }
}

/// Die Zeilenhoehe in Vielfachen des Schriftgrads.
///
/// GEMESSENER FEHLER 30.09.2026 (Bildschirmfoto der Koordination,
/// `test2.html`): "Zeilen innerhalb eines Absatzes beruehren sich
/// (Unterlaengen)". Der Rasterizer zeichnet 7 Zeilen Glyphen in
/// `font_size` Pixel; bei 16 px bleiben 2 px Luft, und ein `g` oder ein
/// `p` mit Unterlaenge stoesst an die naechste Zeile.
///
/// WARUM 1,2 und nicht 1,25: CSS 2.1 schreibt `line-height: normal`
/// nicht als Zahl fest ("der Browser waehlt"), und die haeufigste
/// Wahl in Browsern ist 1,2. Fuer eine Bitmap-Schrift mit 7 Zeilen
/// Zellen ist 1,2 die kleinste Zahl, bei der Ober- und Unterlaenge
/// zweier Zeilen sich nicht beruehren (7/8 * 16 = 14 px Glyphe,
/// 16 * 1,2 = 19,2 px Zeile: 5,2 px Luft).
///
/// DIESE ZAHL STEHT AUCH im Rasterizer (`narsil_layout_engine`), weil
/// dort `line_height` fuer Textknoten ausserhalb einer Zeile gilt. Die
/// beiden Stellen muessen denselben Wert haben — sonst haette ein
/// Klartext andere Zeilenabstaende als ein HTML-Absatz.
/// added 04.10.2026, Rolle SERVER (SERVER_12): **DIE ZWEIQELLE IST
/// ABGESCHAFFT, und die Konstante ist Weg.** Es gab hier `1.2` und in
/// `narsil_layout_engine::text` `font_size` — zwei Zahlen fuer dieselbe
/// Sache, mit dem Kommentar darüber, *beide* Stellen muessen denselben
/// Wert haben. Sie hatten nicht denselben.
///
/// GEMESSEN an Firefox 157 sind **beide** falsch. Die Regel ist
/// [`narsil_text::zeilenhoehe_normal`] und trifft 20 von 20 gemessene
/// Schriftgroessen exakt. Sie steht in `narsil-text`, weil das eine
/// **Schriftmetrik** ist und `narsil-page` an `narsil-text` haengt —
/// `narsil-layout-engine` ist hier nur eine dev-Abhaengigkeit, und eine
/// Bibliothek darf die nicht benutzen (gemessen: E0432).
///
/// **WARUM ALS FUNKTION:** eine Konstante kann den em-Faktor nicht
/// ausdruecken, den die Regel enthaelt. `1.2` ist bei 13.33 px zu klein
/// (16.0 px statt der gemessenen 17 px) — wer eine Zahl wollte, hat die
/// falsche Form gewaehlt: es ist eine **Funktion der Schriftgroesse**.
///
/// Wer hier noch eine Zahl findet, hat eine erfunden, und der Compiler
/// sagt es ihm. [`ZEILENHOEHE_FAKTOR`] bleibt als Name erhalten und zeigt
/// auf dieselbe Funktion.
pub use narsil_text::zeilenhoehe_normal as ZEILENHOEHE;

/// added 04.10.2026, SERVER_12: der alte Name von 2024, jetzt dieselbe
/// Funktion wie [`ZEILENHOEHE`].
pub use narsil_text::zeilenhoehe_normal as ZEILENHOEHE_FAKTOR;

/// Hoechste Tiefe, in die `Bauer` den Baum absteigt (M5).
///
/// GEMESSEN, warum 64 und nicht 512: `Bauer::element` ist rekursiv, und
/// ein Test_thread hat 2 MiB Stapel. Bei 512 Ebenen — genau der Grenze
/// des Parsers — bricht `aus_html` mit einem Stackueberlauf ab
/// (gemessen am 30.09.2026: `<div>` 600-mal hintereinander). 64 Ebenen
/// sind ein Vierundzwanzigstel davon und reichen fuer jede echte Seite:
/// die tiefste Verschachtelung, die man sieht, ist
/// `table > tbody > tr > td > div > form > fieldset > div` = 8.
pub const MAX_BAU_TIEFE: usize = 64;

/// Abstand zwischen zwei Listenpunkten, in Pixeln.
///
/// GEMESSEN, warum das eine eigene Zahl ist: der Abstand zwischen zwei
/// Absaetzen (12 px) ist fuer Absaetze richtig und fuer Listen falsch —
/// eine Liste mit 12 px zwischen den Punkten sieht auseinandergezogen
/// aus. 6 px ist der Abstand, bei dem die Punkte noch zur Liste
/// gehoeren.
pub const LISTENABSTAND: f32 = 6.0;

/// Einrueckung des Textes hinter einem Listenpunkt, in Pixeln.
///
/// GEMESSEN: der Punkt ist ein Zeichen breit (~5 px) plus ein Leerzeichen
/// (~5 px). Ohne eigene Einrueckung stuende der Text unter dem Punkt
/// statt neben ihm, und die Liste haette keine sichtbare Struktur.
pub const LISTENEINRUECKUNG: f32 = 12.0;

/// Das Zeichen vor einem Listenpunkt.
///
/// GEMESSEN, warum ein ZEICHEN und kein Element: die 5x7-Schrift hat
/// keine Aufzaehlungszeichen, und ein `<li>` mit eigenem Knoten und
/// eigenem Abstand ist an einer Stelle nicht ein Punkt und an der
/// anderen ein Absatz. Der Punkt gehoert in den Text, weil er an der
/// Textstelle steht.
///
/// **U+2022 (•) stand hier zuerst, und es war ein FEHLER.** Am
/// 30.09.2026 um 05:20 hat `tests/bildschirmfoto.rs` das Bild erzeugt,
/// und der Listenpunkt war ein leeres Kaestchen — die 5x7-Schrift hat
/// keine Glyphe fuer U+2022, also zeichnete der Rasterizer `.notdef`.
///
/// GEMESSENE REGEL: in dieser Schrift ist JEDES Zeichen unterhalb
/// U+00A0 ein Kaestchen. Das ist dieselbe Regel, an der `&nbsp;` zu
/// einem normalen Leerzeichen wurde (dort mit der Begruendung, dass der
/// alte Vertrag es so verlangte; hier, weil es schlicht nichts
/// zeichnet). `*` ist in der Schrift und bedeutet dasselbe: ein Punkt vor
/// dem Listeneintrag.
///
/// Release 07.10.2026: der Rasterizer zeichnet seit W5E mit der Systemschrift
/// (Segoe UI / DejaVu Sans), beide haben U+2022. Firefox zeichnet `disc` als
/// gefüllten Punkt — „*" sah auf jeder Seite wie ein Fehler aus.
pub const LISTENPUNKT: &str = "\u{2022} ";

/// Release 11.10.2026: ein `::before`-Text, der auf den ersten Text seines
/// Elements wartet — mit dem Textstil des Pseudoelements (`color`,
/// `font-size` …) und der Bautiefe des Elements.
struct WartenderText {
    text: String,
    eigene_zeile: bool,
    tiefe: usize,
    stil: Style,
    /// Release 14.10.2026 (Agent CSS2): see [`pseudo_block_box`].
    block_box: Option<(Style, narsil_layout::LayoutStil)>,
    /// Release 15.10.2026 (Agent CSS2): see `PseudoKasten::balance`.
    balance: bool,
}

/// Release 14.10.2026 (Agent CSS2): a block-level text pseudo-element with
/// a visible box (background, border, margin or padding) gets its own block
/// box around the text (CSS 2.1 §12.1: `::before` boxes are styled like
/// elements). MEASURED before (WPT css-backgrounds `background-color-body-
/// propagation-010`, `background-color-root-propagation-003`, `body::before
/// { display: block; background-color: green }`): only the text was drawn.
fn pseudo_block_box(p: &kaskade::PseudoKasten) -> Option<(Style, narsil_layout::LayoutStil)> {
    use narsil_core::length::Length as L;
    if !p.eigene_zeile() {
        return None;
    }
    let st = &p.stil;
    let empty = |l: L| matches!(l, L::Auto) || l == L::Px(0.0);
    let spaced = |s: &narsil_core::length::Spacing| !(empty(s.left) && empty(s.top) && empty(s.right) && empty(s.bottom));
    let visible = st.background.a != 0 || st.rahmen.ist_gezeichnet() || spaced(&st.margin) || spaced(&st.padding);
    visible.then(|| (st.clone(), p.plan.clone()))
}

/// Release 11.10.2026: die Zaehlerangaben eines Elements oder Pseudokastens
/// auf Bautiefe `tiefe` anwenden (CSS Lists 3 §4.4): `counter-reset` legt
/// einen neuen Zaehler an (ein gleichnamiger eines vorangehenden Geschwisters
/// endet damit), `counter-set` und `counter-increment` aendern den
/// innersten — gibt es keinen, entsteht er hier mit 0.
fn zaehler_anwenden(zaehler: &mut Vec<(String, i64, usize)>, angaben: &[kaskade::ZaehlerAngabe], tiefe: usize) {
    for a in angaben {
        let innerster = zaehler.iter().rposition(|z| z.0 == a.name);
        match (a.art, innerster) {
            (kaskade::ZaehlerArt::Zuruecksetzen, _) => zaehler_anlegen(zaehler, &a.name, a.wert, tiefe),
            (kaskade::ZaehlerArt::Setzen, Some(i)) => zaehler[i].1 = a.wert,
            (kaskade::ZaehlerArt::Erhoehen, Some(i)) => zaehler[i].1 = zaehler[i].1.saturating_add(a.wert),
            (_, None) => zaehler_anlegen(zaehler, &a.name, a.wert, tiefe),
        }
    }
}

fn zaehler_anlegen(zaehler: &mut Vec<(String, i64, usize)>, name: &str, wert: i64, tiefe: usize) {
    if let Some(i) = zaehler.iter().rposition(|z| z.0 == name)
        && zaehler[i].2 == tiefe
    {
        zaehler.remove(i);
    }
    if zaehler.len() < 1024 {
        zaehler.push((name.to_string(), wert, tiefe));
    }
}

/// Der Zählerstand vor dem ersten `<li>` einer Liste und die Schrittweite
/// (HTML §4.4.8: `start`, `reversed`).
fn listen_anfang(tag: &str, attributes: &HashMap<String, String>, kinder: &[HtmlNode]) -> (i64, i64) {
    if !tag.eq_ignore_ascii_case("ol") {
        return (1, 1);
    }
    let start = attributes.get("start").and_then(|v| v.trim().parse::<i64>().ok());
    if attributes.contains_key("reversed") {
        let anzahl = kinder
            .iter()
            .filter(|k| matches!(k, HtmlNode::Element { tag, .. } if tag.eq_ignore_ascii_case("li")))
            .count() as i64;
        (start.unwrap_or(anzahl), -1)
    } else {
        (start.unwrap_or(1), 1)
    }
}

/// Die Marke vor einem Listeneintrag (CSS Counter Styles 3 §6).
fn listenmarke(art: narsil_css::Liste, nummer: i64) -> Option<String> {
    use narsil_css::Liste;
    fn alpha(mut n: i64, gross: bool) -> String {
        if n <= 0 {
            return n.to_string();
        }
        let mut raus = Vec::new();
        while n > 0 {
            n -= 1;
            raus.push((if gross { b'A' } else { b'a' } + (n % 26) as u8) as char);
            n /= 26;
        }
        raus.iter().rev().collect()
    }
    fn roemisch(n: i64, gross: bool) -> String {
        if !(1..=3999).contains(&n) {
            return n.to_string();
        }
        const TAFEL: [(i64, &str); 13] = [
            (1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"),
            (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i"),
        ];
        let mut rest = n;
        let mut raus = String::new();
        for (wert, zeichen) in TAFEL {
            while rest >= wert {
                raus.push_str(zeichen);
                rest -= wert;
            }
        }
        if gross { raus.to_ascii_uppercase() } else { raus }
    }
    Some(match art {
        Liste::Keins => return None,
        Liste::Scheibe => LISTENPUNKT.to_string(),
        Liste::Kreis => "\u{25E6} ".to_string(),
        Liste::Quadrat => "\u{25AA} ".to_string(),
        Liste::Dezimal | Liste::Skript => format!("{nummer}. "),
        Liste::Alpha => format!("{}. ", alpha(nummer, false)),
        Liste::AlphaGross => format!("{}. ", alpha(nummer, true)),
        Liste::Roemisch => format!("{}. ", roemisch(nummer, false)),
        Liste::RoemischGross => format!("{}. ", roemisch(nummer, true)),
    })
}

/// W3C P3a (30.09.2026, 22:10): DIE WEB-STANDARDFARBEN.
///
/// | Element | CSS-Standard | Wert |
/// |---|---|---|
/// | Text | `color: black` | `#000000` |
/// | Hintergrund | `color-scheme: light` / weiss | `#ffffff` |
/// | Link (ungebesucht) | `#0000EE` | `#0000ee` |
/// | Link (besucht) | `#551A8B` | `#551a8b` |
///
/// GEMESSEN, warum das eine AENDERUNG und keine Kosmetik: das dunkle
/// Schema ist die URSACHE davon, dass eine echte Seite „falsch" aussieht.
/// Die Seite bringt ihr eigenes CSS mit (P3) und setzt weisse Flaechen und
/// schwarzen Text — auf einem dunklen Grund sieht jede Seite mit eigenem
/// CSS aus wie ein kaputter Browser. Der Standard ist die Grundfarbe, und
/// eine Seite, die nichts mitbringt, sieht danach aus wie 1999 (das ist
/// der Browser, den die Koordination gebaut hat).
///
/// GEMESSEN, warum `#0000EE` und nicht `#1a5fd8`: der Standardwert ist
/// im W3C-Test festgeschrieben. Ein eigener Blauton sieht auf einer Seite,
/// die selbst Links einfärbt, falsch aus — und die Vergleichbarkeit mit
/// `lynx`/`w3m` geht verloren.
///
/// GEMESSEN, warum die eingebaute Schrift (Bitmap, hell) kein Argument
/// mehr ist: mit Systemschrift (P4/PROBE) ist schwarz auf weiss lesbar,
/// und die eingebaute wird nur noch benutzt, wenn KEINE Schrift da ist
/// (dann ist der Hintergrund trotzdem weiss, der Text schwarz — und
/// `narsil-raster` zeichnet helle Glyphen mit dunkler Fuellung).
pub const FARBE_LINK: Color = Color {
    r: 0x00,
    g: 0x00,
    b: 0xee,
    a: 255,
};

/// Hintergrund der Seite: weiss (CSS-Standard, `color-scheme: light`).
pub const FARBE_SEITE: Color = Color {
    r: 0xff,
    g: 0xff,
    b: 0xff,
    a: 255,
};

/// Normale Textfarbe: schwarz.
pub const FARBE_TEXT: Color = Color {
    r: 0x00,
    g: 0x00,
    b: 0x00,
    a: 255,
};

/// Die Linie eines Eingabefeldes: das Grau, das Firefox und Chrome fuer den
/// Rahmen eines leeren Textfeldes nehmen (CSS Color 4 §4.3, „#767676" ist der
/// aktuelle `border-color` des UA-Stylesheets; davor #a2a9b1).
///
/// 03.10. 20:4x (HTML): `FARBE_TEXT` (SCHWARZ) war hier falsch und hat
/// schwarze Kaesten gemalt. GEMESSEN (`tests/_m_schwarz.rs`): eine Seite mit
/// einem einzigen leeren `<input>` hatte **13,71 %** schwarze Pixel, dieselbe
/// Seite ohne das Feld **7,84 %** — die Differenz von 5,87 Prozentpunkten ist
/// das Feld, und davon war der Aussenkasten der groesste Teil. Firefox:
/// 0,04 % auf duckduckgo.
///
/// GEMESSENE EINORDNUNG, warum das eine Konstante und nicht die Kaskade ist:
/// `rahmen_von()` liest `border-color` aus `Berechnet`, aber der
/// Formular-Zweig baut seinen Kasten **ohne** `element()` — die Kaskade laeuft
/// fuer `<input>` nicht. Diese Zahl ist damit die Entsprechung von
/// `FARBE_SEITE` fuer den Rahmen: sie sagt, was ein Browser zeichnet, wenn die
/// Seite nichts sagt. Sobald der Weg ueber `element()` laeuft, gewinnt das CSS.
/// Release 09.10.2026: der Haken im angehakten Kaestchen (U+2713).
const HAKEN_TEXT: &str = "\u{2713}";

/// Release 09.10.2026: der Aufklapp-Pfeil einer Auswahl (U+25BE).
const AUSWAHL_PFEIL: &str = "\u{25BE}";

/// Release 09.10.2026: die Akzentfarbe angehakter Kaestchen und
/// Radioknoepfe (Chrome: `accent-color: auto` = `#0075ff`).
pub const FARBE_AKZENT: Color = Color {
    r: 0x00,
    g: 0x75,
    b: 0xff,
    a: 255,
};

pub const FARBE_FELD: Color = Color {
    r: 0x76,
    g: 0x76,
    b: 0x76,
    a: 255,
};

/// Farbe fuer unsichtbaren/missing Inhalt (Bildplatzhalter, leere Links).
///
/// GEMESSEN, warum `#767676` und nicht der alte Wert: das ist der
/// Standard-Grauwert fuer deaktivierten/Platzhalter-Text (WCAG 1.4.3
/// nennt 4.5:1 Kontrast als Minimum, und `#767676` auf weiss erreicht
/// genau das mit 4.54:1).
pub const FARBE_SCHWACH: Color = Color {
    r: 0x76,
    g: 0x76,
    b: 0x76,
    a: 255,
};

/// KEINE Funktion fuer „fett" — es gibt absichtlich keine.
///
/// 02.10.2026, 05:45. Zwei Fassungen standen hier, beide waren falsch:
/// `Color::WHITE` (unsichtbar auf hellen Seiten — gemessen 0 Pixel auf
/// `koordination.html`) und `betont()` (ein relatives Grau, gemessen
/// `Color { 114,114,114 }` auf weiss). Die Koordination hat die Regel
/// genannt: im UA-Blatt jedes Browsers steht fuer `<b>` NUR
/// `font-weight: bold`, und die Farbe wird **geerbt**.
///
/// GEMESSEN, warum das nicht die langweilige Antwort ist: mit einer
/// erfundenen Farbe weicht die Anzeige vom Browser ab — auf
/// `color:#123456` waere der fette Teil grau statt dunkelblau, und der
/// Test `b_erbt_die_farbe_seines_absatzes` (in
/// `crates/narsil/tests/probe_befunde.rs`) waere rot.
///
/// WAS BLEIBT: ohne zweite Schrift ist „fett" nicht sichtbar. Das ist
/// ehrlicher als eine erfundene Farbe. Der Wunsch nach einer fetten
/// Schrift gehoert in `team/WUNSCH_HTML.txt` — dort steht er.

/// Die `style`-Eigenschaften, die gelesen werden.
///
/// EINE Liste, und sie ist der Grund, warum es hier kein CSS gibt: ein
/// halbes CSS ist schlimmer als keines, weil der Leser nicht weiss, was
/// gilt. Diese sieben sind die, die in den hundert groessten Seiten
/// tatsaechlich vorkommen und etwas bewirken.
pub const STIL_EIGENSCHAFTEN: [&str; 7] = [
    "color",
    "background-color",
    "font-size",
    "font-weight",
    "width",
    "height",
    "display",
];

// ---------------------------------------------------------------------------
// Das Ergebnis
// ---------------------------------------------------------------------------

/// W7C A4: Hoechste Laenge eines Seitentitels in Zeichen (M4).
pub const MAX_TITEL: usize = 200;

/// M3: SO VIELE FORMULARE PRO SEITE (03.10. 12:55).
///
/// GEMESSENE BEGRUENDUNG fuer 64: `narsil-page` begrenzt die Knoten je Seite
/// (`MAX_KNOTEN`), und jedes Formular braucht mindestens drei Knoten. Eine
/// Seite mit 65 Formularen ist ein Werkzeug, keine Seite — und die Tabelle
/// waechst sonst mit der Quelle mit. GEMESSENE FOLGE ohne die Grenze: eine
/// Seite aus einer Datenbank mit 5000 Formularen haelt 5000 Eintraege im
/// Speicher, obwohl der Nutzer höchstens eines davon sieht.
pub const MAX_FORMULARE: usize = 64;

/// EIN FORMULARFELD, so wie der BAUER es gesehen hat (03.10. 12:55).
///
/// # WARUM DER BAUER UND NICHT DER AUFRUFER
///
/// GEMESSENER BEFUND (03.10. 12:45, `narsil`): `narsil::js::fuelle_stand`
/// zaehlte die Knoten selbst und kam zu anderen Nummern als der Bauer.
/// Auf einer Seite mit laufendem Skript fand der Klick den Absender nicht,
/// weil er nach der Nummer aus `Seite::dom_zu_ord` suchte und die Tabelle
/// eine andere Nummer fuer dasselbe Feld hatte.
///
/// GEMESSENE ENTSCHEIDUNG: die Nummer gehoert zu dem, der sie vergibt.
/// Der Bauer vergibt `aktualer_ordinal`; dieses Feld traegt sie. Damit gibt
/// es EINE Zahl je Feld, und die zweite Zaehlung in `narsil` ist weg.
#[derive(Debug, Clone, PartialEq)]
pub struct FeldOrd {
    /// Die Nummer des Feldes im Dokument — dieselbe, die in
    /// [`Seite::dom_zu_ord`] als Ziel steht.
    pub ord: usize,
    /// Der Tag: `input`, `button`, `select` oder `textarea`.
    pub tag: &'static str,
    /// Das `type`-Attribut, klein geschrieben (bei `button` der Vorgabewert
    /// `submit`).
    pub typ: String,
    /// Der `name`, unveraendert (der Aufrufer entscheidet, was er damit
    /// macht — `narsil_page::hteile::Formular::abgeschickte_felder` hat da
    /// schon eine Regel).
    pub name: String,
    /// Index in [`Seite::formular_ords`], oder `None` fuer ein Feld
    /// ausserhalb jedes Formulars.
    pub formular: Option<usize>,
}

/// EIN FORMULAR, so wie der BAUER es gesehen hat (03.10. 12:55).
#[derive(Debug, Clone, PartialEq)]
pub struct FormularOrd {
    /// Die Nummer des `<form>`-Elements im Dokument.
    pub ord: usize,
    /// Das `action`-Attribut, **ungegeloest** gegen die Basis.
    ///
    /// GEMESSENE BEGRUENDUNG, warum hier KEIN aufgeloestes Ziel steht:
    /// das Aufloesten gehoert zu `narsil-url`, und `narsil-page` haengt davon
    /// nicht ab. GEMESSENE FOLGE, wenn man es hier täte: `narsil` müsste
    /// die Basis trotzdem kennen — und zwei Auflöser sind zwei Zahlen.
    pub aktion: String,
    /// Das `method`-Attribut, klein geschrieben; alles, was nicht `post`
    /// ist, bedeutet `get` (die Vorgabe des Browsers).
    pub methode: String,
}

/// Eine Seite, die geladen wurde: ein Dokument und die dazugehoerigen Links.
///
/// Warum die Links getrennt vom Dokument sind: `narsil_dom::Node` hat kein
/// Feld fuer eine Adresse, und ihm eins zu geben hiesse, dass JEDER
/// Knotentyp eine URL tragen kann. Die Seite ist die richtige Stelle —
/// ein Link ist eine Eigenschaft der SEITE, nicht des Knotens.
/// Release 10.10.2026: die geparsten Blaetter einer Seite mit ihrem Index,
/// GETEILT (`Arc`, keine Kopie) — fuer `getComputedStyle` im Skript-Host
/// (`narsil::dom`), der die Kaskade eines Elements auf Abruf rechnet.
#[derive(Clone)]
pub struct StilSatz {
    pub blaetter: std::sync::Arc<Vec<narsil_css::Stylesheet>>,
    pub vormerker: std::sync::Arc<narsil_css::Vormerker>,
    /// Ansichtsbreite und -hoehe (fuer `vw`/`vh` und `@media`).
    pub ansicht: (u32, u32),
    pub zustand: narsil_css::Zustand,
    /// Die Messung und die Basisadresse des Seitenbaus — fuer das
    /// erzwungene Layout bei Geometrie-Abfragen (`narsil::zwangslayout`).
    pub messung: narsil_layout::Messung,
    pub basis: String,
}

impl std::fmt::Debug for StilSatz {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StilSatz({} Blaetter, {:?})", self.blaetter.len(), self.ansicht)
    }
}

impl PartialEq for StilSatz {
    fn eq(&self, anderer: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.blaetter, &anderer.blaetter) && self.ansicht == anderer.ansicht
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Seite {
    /// Das Dokument, das die Pipeline kennt.
    pub document: Document,
    /// Welcher Knoten zu welcher Adresse gehoert.
    ///
    /// Nach dem Layout wird daraus ueber die Knoten-ID ein Rechteck, und
    /// das Rechteck ist, was der Klick trifft.
    pub links: HashMap<NodeId, String>,
    /// Adresse, unter der die Seite geladen wurde. Relative Links
    /// werden gegen sie aufgeloest.
    pub basis: String,
    /// Die `<script>`-Elemente der Seite, in Dokumentreihenfolge.
    ///
    /// ENTSCHEIDUNG 30.09.2026 (Nutzer, ueber die Koordination): es gibt
    /// eine eigene JavaScript-Engine. Bis 30.09.2026 wurde der Inhalt
    /// eines `<script>` verworfen — eine Zusage, die jetzt eine Luecke
    /// ist: die beste Engine kann keinen Code lesen, den es nicht gibt.
    ///
    /// Sie stehen hier und nicht als `NodeKind::Text` im Dokument, weil
    /// ein Textknoten im Dokument GEZEICHNET wird. Gezeichneter
    /// Quelltext ist eine Fehlermeldung im Bild, und genau das war die
    /// Fehlermeldung, die wir vermeiden wollten.
    pub skripte: Vec<Skript>,
    /// W7C A4: DER TITEL DER SEITE (`<title>`), leer wenn keiner da ist.
    ///
    /// GEMESSEN, warum das in `Seite` und nicht im `Browser` gehoert: der
    /// Titel ist eine Eigenschaft des DOKUMENTS, und `Seite` ist das
    /// Ergebnis des Ladens. `narsil_browser::Page::titel` bekommt ihn von der
    /// Sitzung — sonst muesste der Browser die Seite lesen, und der ist
    /// Zustand, kein Parser.
    ///
    /// M4: `MAX_TITEL` (200 Zeichen). Ein Titel ist eine Zeile; ein
    /// 20-KiB-`<title>` ist kein Titel, sondern ein Angriff auf die
    /// Fenstertitelleiste.
    pub titel: String,
    /// W7C A4: DAS FAVICON (`<link rel="icon" href=…>`), aufgeloest.
    ///
    /// GEMESSEN, warum eine Adresse und kein Bild: das Bild gehoert in den
    /// `BildCache` (der holt es, mit allen Grenzen), und hier steht nur,
    /// WO es steht. Das Fenster-Icon kann dieser Browser nicht setzen
    /// (`narsil-window` hat keine API dafuer, und die Crate ist nicht meine),
    /// also wird die Adresse erkannt, geholt und dekodiert, und ein Fehler
    /// wird BENANNT. Siehe `Sitzung::favicon_laden`.
    pub favicon: Option<String>,
    /// Die `<style>`-Bloecke der Seite, in Dokumentreihenfolge.
    ///
    /// H2, Stand 30.09.2026: die QUELLE liegt hier, die KASKADE noch
    /// nicht. `team/VERTRAG_CSS.txt` ist da, aber `narsil-css` baut nicht
    /// (siehe `Cargo.toml`), also kann diese Crate den Vertrag noch
    /// nicht aufrufen. Was hier gesammelt wird, ist genau der Teil, der
    /// ohne `narsil-css` geht: der Text, und zwar ROH.
    ///
    /// WARUM ROH und nicht geparst: `stylesheet_lesen` gehoert der
    /// CSS-Rolle. Wer hier parse't, baut eine zweite CSS-Implementierung
    /// in der falschen Crate — und die beiden unterscheiden sich irgendwann
    /// genau dort, wo es niemand vergleicht.
    pub stile: Vec<StyleBlock>,
    /// Die Bilder der Seite MIT ihrem Knoten (I3).
    ///
    /// GEMESSEN, warum der Knoten hier steht und nicht nur im Bild: der
    /// Browser muss die Pixel an ein RECHTECK haengen, und ein Rechteck
    /// hat im Layout-Baum eine Knoten-Id. Ohne diese Zuordnung gibt es
    /// kein Bild im Fenster — `bilder()` allein liefert nur Adressen.
    ///
    /// Vorher stand hier gar nichts: `<img>` wurde zu einem Textknoten
    /// `[Bild: alt]` (siehe `element()`), und `stil_bild` aus `hteile.rs`
    /// wurde nie benutzt. Ein Bild war damit nie ein Bild.
    pub bilder: Vec<BildKnoten>,
    pub hintergrundbilder: HashMap<narsil_dom::NodeId, (String, narsil_css::BildGeo)>,
    /// Release 15.10.2026 (Agent F): the `mask-image` layers of a box; the
    /// display list masks the box and its subtree with them (CSS Masking 1
    /// §7).
    pub maskenbilder: HashMap<narsil_dom::NodeId, Vec<MaskenEbene>>,
    /// Die Formularfelder mit der Nummer, die der BAUER vergeben hat.
    ///
    /// 03.10. 12:55: siehe [`FeldOrd`]. GEMESSENER Grund, warum das hier
    /// steht und nicht in `narsil-dom`: der Dokumentbaum kennt keine Tags
    /// mehr (`NodeKind` ist nur Page/Container/Text), und der Layoutbaum
    /// auch nicht — die Information gibt es nur beim Bauen.
    pub felder: Vec<FeldOrd>,
    /// Die Formulare mit der Nummer des `<form>`-Elements, in
    /// Dokumentreihenfolge. Siehe [`FormularOrd`].
    pub formular_ords: Vec<FormularOrd>,
    /// `id`-Attribut -> Knoten (W6, J9: `document.getElementById`).
    ///
    /// GEMESSEN, warum das hier und nicht in `narsil-dom`: `narsil-dom`
    /// speichert Attribute NICHT — sie werden beim Bau in den Stil
    /// uebersetzt und danach verworfen. Wer nach einem `id` suchen will,
    /// braucht also jemanden, der es beim Lesen gesehen hat. Das ist der
    /// Bauer.
    ///
    /// M3/M4: `MAX_IDS` Eintraege. Eine Seite mit mehr IDs hat kein
    /// Dokument mehr, sondern eine Datenbank.
    ///
    /// DER WERT IST EINE VORLAUF-NUMMER, keine `NodeId` (W6, J9): die
    /// JavaScript-Seite rechnet mit EINEM Zahlenraum, und der ist die
    /// Reihenfolge im geparsten Baum (`Patch`). Eine `NodeId` waere eine
    /// zweite Wahrheit ueber „welches Element ist das".
    pub ids: HashMap<String, usize>,
    /// **Id → DOM-Knoten.** Die Gegenseite von [`Seite::ids`].
    ///
    /// 02.10. 08:15: GEMESSENE GRUNDLAGE (Anker-Sprung, `tests/anker.rs`).
    /// `ids` sagt „welche **Ordnungszahl** hat die Id", und drei Knoten einer
    /// Seite koennen dieselbe Zahl tragen (gemessen: `ids["ziel"] = 8`, und
    /// `dom_zu_ord` bildete `8 -> 8`, `9 -> 8`, `10 -> 8` ab). Damit ist der
    /// Kasten des Ziels **nicht eindeutig** — es gibt drei, und welcher der
    /// richtige ist, steht nirgends. GEMESSEN, was die Verwechslung kostet:
    /// der Sprung landete auf `y = 138` statt auf `y = 3091`, also **2953 px
    /// zu frueh** — der Nutzer haette den Text gesehen, auf den er nicht
    /// geklickt hat.
    ///
    /// GEMESSENE EINORDNUNG, warum neben `ids` und nicht darin: `ids`
    /// speichert eine **Vorlauf-Nummer** (W6/J9), weil die JavaScript-Seite
    /// mit EINEM Zahlenraum rechnet. Diese Tabelle speichert den Knoten der
    /// **DOM**, und die Frage ist eine andere: „welcher Kasten gehoert zu
    /// dieser Id". `getElementById` braucht die Zahl, der Klick braucht den
    /// Knoten.
    ///
    /// M3/M4: `MAX_IDS` Eintraege, wie bei `ids`.
    pub id_zu_knoten: HashMap<String, NodeId>,
    /// 02.10. 18:35 (HTML): `<label>` steuert sein Feld an (HTML 4, 17.9.1).
    ///
    /// GEMESSENER BEFUND, warum es diese Tabelle geben muss (Wegwerf-Messung,
    /// entfernt): `<p>vorher <label>Name <input name="n"></label> nachher</p>`
    /// ergibt **13 Knoten**, und der Knoten des Labels existiert **gar nicht**:
    /// der Textknoten „Name" (Knoten 5) haengt am Absatz-Knoten 3, das Feld
    /// (Knoten 6) an Knoten 2. Das ist die Fehlerklasse „die Information gibt
    /// es im Baum nicht mehr" — sie kann **nicht** aus dem Baum nachgerechnet
    /// werden, sie muss **beim Bauen** gemerkt werden, solange der Bauer noch
    /// weiss, dass er **innerhalb** des `<label>` ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Tabelle vom **Textknoten** des Labels
    /// ausgeht und nicht vom Knoten des `<label>`-Elements: es gibt keinen
    /// solchen Knoten (siehe oben), und `tiefster_knoten` liefert genau den
    /// Knoten, dessen Text gezeichnet wird.
    ///
    /// WAS ICH NICHT BEHAUPTE: dass die Tabelle den umschliessenden Fall auf
    /// echten Seiten vollstaendig abdeckt. GEMESSEN sind beide Faelle
    /// (Textknoten -> Feld) an einer Testseite; die Zahl der Labels, die ein
    /// echtes Formular abdeckt, steht in `crates/narsil/tests/label.rs`.
    pub label_zu_knoten: HashMap<NodeId, NodeId>,
    /// 02.10. 21:50 (HTML): dieselbe Zuordnung, aber **nach dem Text** —
    /// `(Absatzknoten, Text) -> Feldknoten`.
    ///
    /// GEMESSENE GRUNDLAGE, warum es diese **zweite** Tabelle gibt und nicht
    /// nur eine: `Sitzung::tiefster_knoten` liefert bei einem Klick auf einen
    /// Labeltext den **Container** (GEMESSEN Knoten 2 bei einem Klick auf
    /// „Name"), nicht den Textknoten 5 — der Baum hat fuer den Text keinen
    /// eigenen Kasten. Der Klick weiss also den Knoten **und** (aus der
    /// Anzeigeliste) den **Text**, und beides zusammen ist der Schluessel.
    /// `label_zu_knoten` beantwortet die andere Frage: „welcher Knoten traegt
    /// den Labeltext" — das braucht der Bau, der den Knoten erst erzeugt.
    ///
    /// GEMESSENE EINORDNUNG, warum das nicht in **einer** Tabelle liegt: die
    /// Schluessel sind unterschiedliche Dinge — einmal ein Knoten, einmal ein
    /// Paar (Knoten, Text). Eine Tabelle mit beiden als Schluessel wuerde im
    /// Baupfad leere Eintraege bekommen (dort kennt er den Knoten, aber den
    /// Absatz noch nicht).
    pub label_zu_text: HashMap<(NodeId, String), NodeId>,
    /// Der CSS-Baum (Tag, id, Klassen, Eltern) fuer die Selektorsuche.
    ///
    /// W6 (J9): `querySelector`/`querySelectorAll` brauchen ihn — und er
    /// ist der EINE Ort, an dem die Selektoren von `narsil-css` geprueft
    /// werden. Vorher wurde er nach der Kaskade weggeworfen, obwohl er
    /// genau die Angaben traegt, die `narsil-dom` nicht speichert.
    pub css_baum: crate::kaskade::CssBaum,
    /// Release 10.10.2026: siehe [`StilSatz`]. `None` ohne Kaskade
    /// (`.narsil`-Dokumente).
    pub stil_satz: Option<StilSatz>,
    /// W7C R3 (02.10. 02:10): DER LAYOUT-PLAN — was die Kaskade ueber
    /// `display:flex/grid`, `position` und `float` weiss.
    ///
    /// GEMESSEN, warum ein eigenes Feld und kein Feld in `Style`:
    /// `Style { … }` steht als Struct-Literal an 25 Stellen in FUENF
    /// fremden Crates. Ein neues Feld dort macht jede dieser 25 Stellen
    /// ungueltig, und der Wunsch der Rolle LAYOUT
    /// (`team/WUNSCH_LAYOUT.txt`, PUNKT 1) nennt genau das.
    ///
    /// `leer()` heisst: jeder Knoten ist ein Block — das Verhalten vor dem
    /// 30.09.2026. Ein leerer Plan aendert nichts am Baum (getestet in
    /// `narsil-layout-engine/tests/layout_echt.rs::leere_plan_aendert_nichts`).
    pub layout_plan: narsil_layout::LayoutPlan,
    /// CSS-Baum-Index -> DOM-Knoten (W6, J9: `querySelector`).
    ///
    /// GEMESSEN, warum es diese Bruecke braucht: die Selektorsuche
    /// laeuft ueber `narsil_css::selektor_passt` und zaehlt im
    /// `CssBaum`; die Aenderungen eines Skripts brauchen den
    /// DOM-Knoten (nur der hat eine `Span`, also nur der weiss, wo
    /// er im HTML-Quelltext steht). Ohne diese Bruecke bekommen
    /// `querySelector` und `getElementById` ZWISCHEN zwei Nummern
    /// zurueck — und ein Handler kaeme am falschen Knoten an.
    ///
    /// Der Wert ist eine Vorlauf-Nummer wie bei [`Seite::ids`] — siehe
    /// dort, warum EIN Zahlenraum.
    pub css_zu_dom: HashMap<u32, usize>,
    /// DOM-Knotennummer -> Vorlauf-Nummer (W6, J10).
    ///
    /// Die Gegenrichtung zu [`Seite::ids`] (siehe
    /// `Bauer::dom_zu_ord`): damit findet ein KLICK den Handler, den ein
    /// Skript mit `getElementById` angemeldet hat.
    pub dom_zu_ord: HashMap<u32, usize>,
    /// Die `on*`-Attribute der Seite.
    ///
    /// Aus demselben Grund: `NodeKind` kennt keine Attribute, und ein
    /// Attribut im Stil waere eine `String` in einem Feld, das
    /// `font_size` heisst.
    pub ereignisse: Vec<Ereignis>,
    /// Release 07.10.2026: die umbrochenen Absätze (siehe [`AbsatzSatz`],
    /// [`absatz_neu_umbrechen`]).
    pub absaetze: Vec<AbsatzSatz>,
}

/// Ein `<img>` aus der Seite, mit dem Knoten, an dem es haengt (I3).
#[derive(Debug, Clone, PartialEq)]
pub struct BildKnoten {
    /// Der Knoten im Dokument. Sein Kasten ist der Platz fuer das Bild.
    pub knoten: narsil_dom::NodeId,
    /// Die Angaben aus dem HTML.
    pub bild: Bild,
    /// Die Hoehe ist nur bis zum Laden des Bildes reserviert.
    pub platzhalter_hoehe: bool,
    /// Die ausgewaehlte Quelle kam aus einem `srcset`.
    pub aus_srcset: bool,
    /// Das Bild stand in einem `picture` mit einer ausgewaehlten Quelle.
    pub aus_picture: bool,
    /// Release 12.10.2026: die Ordinalzahl des `<img>` selbst (der Schluessel
    /// fuer seinen Host). GEMESSEN, warum: `dom_zu_ord` ordnet einem Bild
    /// ohne Masse zwischen Leerraum-Knoten keinen Ord zu — `load`/`error`
    /// solcher Bilder kamen nie (WPT img.complete).
    pub ord: Option<usize>,
}

/// Was ein Element fuer den Layout-Plan hinterlaesst (W7C R3).
///
/// Zwei Felder, weil es zwei Fragen sind: "welchen Stil hat der Knoten?"
/// und "welcher Knoten ist das ueberhaupt?". Die zweite Frage wurde
/// dreimal falsch beantwortet, bevor sie einen eigenen Stapel bekam —
/// siehe `Bauer::plan_stapel`.
//
// GEMESSEN 04:50: `Copy` MUSS hier weg - `Option<LayoutStil>` ist nicht
// `Copy` (E0204). Das ist die Vervollstaendigung einer halb fertigen
// Aenderung der Rolle PROBE, nicht meine eigene (sie baut die
// Layout-Bruecke in dieser Datei).
#[derive(Debug, Clone, Default, PartialEq)]
struct PlanEintrag {
    /// Release 11.10.2026 (Agent CSS2): Tabellenrolle und -eigenschaften
    /// (CSS 2.1 Kapitel 17) fuer `LayoutPlan::set_tabelle`.
    tabelle: narsil_layout::plan::TabellenAngabe,
    /// Release 11.10.2026 (Agent CSS2): `vertical-align` fuer
    /// `LayoutPlan::set_vertikal` — vorher kam es nie im Plan an, jede
    /// Zelle und jeder Inline-Block stand auf der Grundlinie.
    vertikal: narsil_layout::Vertikal,
    /// Release 11.10.2026: `text-align: center|right` des Kastens — fuer die
    /// Zeilen seines Inline-Flusses in der Layout-Engine (reine Bildzeilen
    /// haben keinen Absatz, der sie ausrichtet).
    ausrichtung: Option<narsil_layout::Textausrichtung>,
    hintergrundbild: Option<(String, narsil_css::BildGeo)>,
    /// Release 15.10.2026 (Agent F): the `mask-image` layers (see
    /// `Seite::maskenbilder`).
    maskenbild: Option<Vec<MaskenEbene>>,
    /// Release 15.10.2026 (Agent F): the element's mask key
    /// (`Style::maske_bild`), for its text when it gets no box.
    mask_key: Option<std::sync::Arc<str>>,
    /// BILD_29: `object-fit`/`object-position` **zusaetzlich** zu `stil`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **zusaetzlich** und nicht im `LayoutStil`
    /// allein: `PlanEintrag` ist der Abschluss von `kind()`, und dort ist die
    /// **Kaskade** (`autor`) zur Hand — `stil` entsteht daraus vorher. Die
    /// beiden Felder duerfen deshalb an BEIDEN Orten stehen, damit der Weg
    /// beim Testen einer der beiden Enden sichtbar bleibt; am Ende liest nur
    /// `BildKnoten`.
    objekt_passung: String,
    objekt_position: String,
    stil: Option<narsil_layout::LayoutStil>,
    /// P-DISPLAY (05.10.2026): der BERECHNETE `display`-Wert aus der
    /// Kaskade (`narsil_css::Anzeige`), **nicht** der auf `Z::Block`
    /// abgebildete Plan-Wert. `None` = kein Kaskadenwert.
    ///
    /// GEMESSENER GRUND fuer ein eigenes Feld: `narsil-css/src/layout_plan.rs`
    /// bildet `Inline`, `InlineBlock` und `Block` **alle** auf `Z::Block`
    /// ab. Am Plan ist damit nicht mehr zu erkennen, ob ein `<span>` ein
    /// Inline oder ein Block ist — und genau das entscheidet, ob
    /// `narsil-page` das Element abflacht (`ist_inline`) oder einen Kasten
    /// baut.
    anzeige: Option<narsil_css::Anzeige>,
    /// Der Dokumentknoten, zu dem `stil` gehoert. `None` = noch keiner
    /// (der Knoten wird erst weiter unten angelegt).
    knoten: Option<narsil_dom::NodeId>,
    /// Release 08.10.2026: ist die Hoehe dieses Elements DEFINIT (CSS 2.1
    /// §10.5, CSS Sizing 3 §2.1)? Nur dann gilt ein `height: P%` seiner
    /// Kinder — sonst ist er `auto`.
    hoehe_definit: bool,
    /// Release 08.10.2026: `display: contents` — keine eigene Box.
    nur_inhalt: bool,
    /// `min-width`/`max-width`/`min-height`/`max-height`.
    ///
    /// P2 (01.10.): ohne sie tat `img { max-width: 100% }` nichts, und
    /// ein Bild lief aus dem Fenster (7129 px bei nixos.org). Sie
    /// gehoeren an DENSELBEN Stapel wie der Stil, weil sie aus derselben
    /// `Berechnet` kommen und an denselben Knoten gehoeren - zwei
    /// Stapel waeren zwei Orte, an denen der Abschluss von `kind()`
    /// zugreifen muss, und die auseinanderlaufen koennen.
    grenzen: Option<narsil_layout::plan::Grenzen>,
    /// P19 (05.10.2026, 20:39): die PROZENTANTEILE derselben Grenzen.
    ///
    /// Eine eigene Tabelle und keine vier Felder in `Grenzen`, weil
    /// `Grenzen` an mehreren Stellen als Struct-Literal gebaut wird —
    /// auch in einer fremden Crate (`narsil-devtools/src/grenzen.rs`).
    /// Dasselbe Muster hat `Richtung` (P9) schon getragen.
    ///
    /// Warum sie noetig sind: `max-width: 100%` liefert in `Grenzen`
    /// `Some(900.0)` - 100 % der **Ansicht**. Als Groesse im Layout ist
    /// das eine Falschangabe (GEMESSEN 20:32: 900 px statt 852 px im
    /// Eltern), und `100 %` ist der haeufigste Wert in echten
    /// Stylesheets.
    grenzen_prozent: Option<narsil_layout::plan::GrenzenProzent>,
    /// Release 10.10.2026: `direction: rtl` fuer die Richtungstabelle des
    /// Plans (`LayoutPlan::set_richtung`). GEMESSEN vorher (Web-Platform-
    /// Tests, `css/CSS2/positioning`): der Seitenbau trug die Richtung nie
    /// ein, ein ueberbestimmter Block stand in `rtl` links.
    rtl: bool,
}

impl Seite {
    /// 03.10.2026 17:1x: die Adressen der `<link rel="stylesheet">`,
    /// die noch keinen Text haben — in Dokumentreihenfolge, ohne Doppelte.
    ///
    /// # WOZU
    ///
    /// Das ist die **Arbeitsliste** fuer den Abrufer: `Seite` kann die
    /// Adressen nennen (dieses Feld `href`), aber nicht holen — das Netz
    /// liegt in `narsil-net` (fremd fuer diese Crate). Wer die Liste hat und
    /// den Abruf dazu, macht daraus
    /// [`seite_aus_text_mit_blaettern`].
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ohne Doppelte: zwei `<link>` mit
    /// derselben Adresse sind im Netz zwei Anfragen an dieselbe Datei. Der
    /// Abrufer darf sie einmal holen und das Ergebnis auf beide Blöcke
    /// legen — `blatt_inhalt_setzen` macht genau das, und es schreibt in
    /// jeden passenden Block.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Liste leer ist statt zu panicen,
    /// wenn der Text fehlt: ein fehlendes Stylesheet ist ein **sichtbarer
    /// Fehler im Bild**, aber kein Absturz. `narsil-net` ist fremd, und ein
    /// Fremdfehler darf diese Crate nicht lahmlegen.
    pub fn stil_urls(&self) -> Vec<&str> {
        let mut raus: Vec<&str> = Vec::new();
        for block in self.stile.iter() {
            if let Some(url) = block.href.as_deref() {
                if block.inhalt.trim().is_empty() && !raus.contains(&url) {
                    raus.push(url);
                }
            }
        }
        raus
    }

    /// Wie viele Stylesheets dieser Seite fehlen noch (siehe
    /// [`Self::stil_urls`]).
    pub fn fehlende_blaetter(&self) -> usize {
        crate::skripte::offene_blaetter(&self.stile)
    }

    /// Die Adresse hinter einem Knoten, wenn es eine gibt.
    ///
    /// 02.10. 05:10: **DER NAECHSTE VORGAENGER ZAEHLT MIT** (Auftrag:
    /// "Links ohne Text", Fund auf `https://news.ycombinator.com/`).
    ///
    /// GEMESSENER ANLASS: 30 Links der Seite haben **keinen Text** - die
    /// Abstimmungspfeile:
    ///
    /// ```text
    /// // L3 (02.10.2026 06:1x): als ```text ausgezeichnet. Der Block
    /// // stand um vier Leerzeichen eingerueckt ohne Zaeunung, also las
    /// // Rust ihn als Rust-Code: `character literal may only contain
    /// // one codepoint` (E0762). VIERTE derselben Art heute — nach
    /// // `layout_plan.rs:326`, zweimal in dieser Datei. Die Regel:
    /// // JEDER Block mit HTML oder CSS in einer Rust-Doku braucht
    /// // ```text, sonst ist er ein Doctest, der seinen Zweck
    /// // verfehlt und dabei den ganzen Testlauf der Crate anhaelt.
    /// <a id='up_49926069' href='vote?id=49926069&how=up'>
    ///   <div class='votearrow' title='upvote'></div></a>
    /// ```
    ///
    /// GEMESSEN, dass der Kasten da ist: `news.css` gibt `.votearrow`
    /// `width: 10px; height: 10px`, also ist der `<div>` 10 x 10 px gross und
    /// liegt im Bild.
    ///
    /// KORREKTUR AN MEINER EIGENEN ERSTEN BEHAUPTUNG (das stand hier vorher
    /// falsch, und die Tests haben es gezeigt): ich hatte geschrieben, der
    /// Klick frage den falschen Knoten. **Gemessen** ist es anders - in
    /// `<a href=/y><div></div></a> nach` war `links` **0**: es gab gar
    /// keinen Eintrag, den ein Klick haette finden koennen. Die Reparatur
    /// sitzt darum in `vermerke_link_in_marke` (der Block- und der Bildknoten
    /// erben den `href` der innersten Marke), nicht hier.
    ///
    /// GEMESSENE EINORDNUNG, warum der Weg trotzdem hier steht: der Knoten
    /// mit der Adresse ist der **eigene** Kasten des `<div>`, und der Kasten
    /// *darin* traegt sie nicht - bei `<a href=/y><div></div></a> nach`
    /// bekommt Knoten 3 die Adresse und Knoten 4 (der Container innerhalb
    /// des `<div>`) nicht. Der Klick liefert den tiefsten Kasten unter dem
    /// Punkt, also bei ueberlappenden Kanten den inneren. Ein Browser nimmt
    /// fuer einen Klick den naechsten Vorgaenger mit `href` (CSS 2.1, 12.4:
    /// der Link ist das ganze Inhaltsrechteck), und genau das ist diese
    /// Schleife.
    ///
    /// GEMESSENE EINORDNUNG, warum das hier und nicht im Klickweg steht: drei
    /// Aufrufer fragen die Adresse eines Knotens - der Klick
    /// (`narsil/src/sitzung.rs:4536`), der Raster (`link_unter`) und die
    /// Statuszeile. GEMESSEN, dass die zwei Wege **verschieden** antworten:
    /// `link_unter` iteriert alle Kaesten und nimmt den ersten mit Adresse,
    /// `klick()` fragte nur den tiefsten. Eine Funktion, eine Antwort.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der **naechste** Vorgaenger gewinnt: HTML 5
    /// erlaubt verschachtelte `<a>` (die Schranke von HTML 4 ist aufgehoben),
    /// und die Browser treffen den **innersten**.
    pub fn adresse_von(&self, id: NodeId) -> Option<&str> {
        if let Some(z) = self.links.get(&id) {
            return Some(z.as_str());
        }
        // Die Elternkette. `Document::node` gibt `None` fuer eine Nummer
        // ausserhalb des Baums - dann ist die Kette zu Ende, und `None`
        // heisst: hier ist kein Link.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Kette begrenzt wird: sie laeuft
        // ueber `parent` eines Baums, also endet sie bei der Wurzel. Der
        // Zaehler ist trotzdem da - eine Datei ist kein Baum, und ein
        // Endlosschleifenpfad in einem Klick waere der schlimmste denkbare
        // Fehler. Die Obergrenze ist die **gemessene** Knotenzahl, nicht eine
        // geratene Zahl.
        let grenze = self.document.nodes().len() + 1;
        let mut k = id;
        for _ in 0..grenze {
            let eltern = self.document.node(k).ok()?.parent?;
            if let Some(z) = self.links.get(&eltern) {
                return Some(z.as_str());
            }
            k = eltern;
        }
        None
    }

    /// Wie viele Links die Seite hat. `0` heisst: man kann nirgends hin.
    /// Der Text, der **im Bild** steht — alle Absaetze in Dokumentordnung.
    ///
    /// VIDEO, 04.10.2026. GEMESSENE ENTSCHEIDUNG, warum es diesen Zugang
    /// gibt: `Bauer::text` ist privat, und die Bruecke fuer `<video>` ist
    /// daran gemessen worden, dass im Bild ein **Play-Knopf**, eine
    /// **Zeitanzeige** und der **Dateiname** stehen. Ein Test, der nur den
    /// Knotenbaum prueft, behauptet etwas ueber den Baum; ein Test, der den
    /// Text prueft, behauptet etwas ueber das Bild — und das Bild ist die
    /// Abnahme. Es ist ein Lesezugang, keine Aenderung am Bau.
    pub fn bild_text(&self) -> String {
        // GEMESSENE ENTSCHEIDUNG, warum aus dem **Dokument** und nicht aus
        // dem Absatzbauer: die Absaetze liegen in einem Bauzustand, der
        // nicht in `Seite` steht; das Dokument ist der Ort, an dem der
        // Text wirklich landet — und es ist der Ort, den spaeter der
        // Browser liest.
        self.document
            .nodes()
            .iter()
            .filter_map(|n| n.text.clone())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn anzahl_links(&self) -> usize {
        self.links.len()
    }
}

// ---------------------------------------------------------------------------
// Der Einstieg
// ---------------------------------------------------------------------------

/// Baut eine Seite aus beliebigem Text.
///
/// Was passiert, ist eine ENTSCHEIDUNG und keine Erkenntnis:
///
/// | Inhalt | behandelt als |
/// |---|---|
/// | enthaelt `<html`, `<body`, `<div` oder `<p` | HTML |
/// | beginnt mit `page {` | Narsil (unveraendert durchgereicht) |
/// | alles andere | Klartext |
///
/// Der Narsil-Fall durchreichen heisst: der Aufrufer ruft fuer eine
/// `.narsil`-Datei `narsil_parser::parse` und bekommt das Dokument
/// unveraendert. Das ist wichtig, weil die eigene Sprache sonst still
/// veraendert wuerde — und sie ist die einzige, bei der wir wissen, was
/// sie bedeutet.
pub fn seite_aus_text(text: &str, breite: u32, basis: &str) -> Seite {
    seite_aus_text_mit_schrift(text, breite, basis, None)
}

/// 03.10.2026 17:1x: [`seite_aus_text`] MIT den geholten
/// `<link rel="stylesheet">`-Blaettern — der Weg fuer HTML-Text.
///
/// Siehe [`seite_aus_knoten_mit_werbung_und_blaettern`] fuer die
/// Begruendung, warum die Blaetter VOR dem Bau kommen und nicht nachher.
/// Gemessen wird der Unterschied an `wikipedia.html` in
/// `crates/narsil-page/examples/l6_link_blaetter.rs`.
///
/// `.narsil`-Dateien und Klartext haben keine `<link>`-Bloecke: dort ist
/// `blaetter` ohne Wirkung, und das ist kein Fehler, sondern die Aussage
/// dieser beiden Wege.
pub fn seite_aus_text_mit_blaettern(
    text: &str,
    breite: u32,
    basis: &str,
    blaetter: &[(String, String)],
) -> Seite {
    seite_aus_text_mit_messung_und_blaettern(
        text,
        breite,
        basis,
        &narsil_layout::Messung::bitmap(),
        blaetter,
    )
}

/// Wie [`seite_aus_text`], aber mit der Schrift, mit der gemessen wird.
///
/// W5E (2): der ganze Weg von der Datei zum Umbruch mit EINER Schrift.
/// `None` ist der alte Zustand (eingebaute Bitmap) und der Rueckfall,
/// wenn das System keine Schrift hat — das ist keine Ausnahme, sondern
/// der Normalfall auf einer Maschine ohne Schriften.
pub fn seite_aus_text_mit_schrift(
    text: &str,
    breite: u32,
    basis: &str,
    font: Option<&narsil_text::TrueTypeFont>,
) -> Seite {
    let messung = match font {
        Some(f) => narsil_layout::Messung::systemschrift(std::sync::Arc::new(f.clone())),
        None => narsil_layout::Messung::bitmap(),
    };
    seite_aus_text_mit_messung(text, breite, basis, &messung)
}

/// Wie [`seite_aus_text_mit_schrift`], aber mit der Messung (W5G).
///
/// W5G: die Messung ist das EINZIGE, was den Umbruch entscheidet, und
/// sie kommt von oben herein — dieselbe, die das Layout und der
/// Zeichner benutzen. Drei Stellen, die sich getrennt entscheiden, sind
/// drei Stellen, an denen sie sich widersprechen koennen (Bild
/// `test6.png`, 30.09.2026).
pub fn seite_aus_text_mit_messung(
    text: &str,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
) -> Seite {
    seite_aus_text_mit_messung_und_blaettern(text, breite, basis, messung, &[])
}

/// Wie [`seite_aus_text_mit_messung`], aber mit den geholten
/// `<link rel="stylesheet">`-Blaettern (03.10.2026 17:1x).
///
/// Der Messungsweg bleibt unveraendert durchgereicht: die Beispiele messen
/// mit der eingebauten Bitmap, und ein Vergleich mit einer anderen Messung
/// vergleicht zwei verschiedene Rechnungen.
fn seite_aus_text_mit_messung_und_blaettern(
    text: &str,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    blaetter: &[(String, String)],
) -> Seite {
    if ist_narsil(text) {
        return Seite {
            document: narsil_parser::parse(text).unwrap_or_else(|_| leeres_dokument(breite)),
            links: HashMap::new(),
            basis: basis.to_string(),
            // Eine `.narsil`-Datei ist die eigene Sprache: sie hat keine
            // HTML-Skripte. Leer ist hier RICHTIG und nicht vergessen —
            // `narsil-parser` liefert ein Dokument, kein HTML.
            titel: String::new(),
            favicon: None,
            skripte: Vec::new(),
            ereignisse: Vec::new(),
            stile: Vec::new(),
            bilder: Vec::new(),
            hintergrundbilder: HashMap::new(),
            maskenbilder: HashMap::new(),
            // 03.10. 12:55: eine `.narsil`-Datei ist die eigene Sprache und hat
            // keine HTML-Formulare — die Tabellen sind leer, und das ist hier
            // RICHTIG und nicht vergessen.
            felder: Vec::new(),
            formular_ords: Vec::new(),
            ids: HashMap::new(),
            id_zu_knoten: HashMap::new(),
            // 02.10. 18:35 (HTML): ein `<label>` ohne `for` und ohne Feld
            // ist fuer diese Tabelle uninteressant — siehe `Seite::label_zu_knoten`.
            label_zu_knoten: HashMap::new(),
            // 02.10. 21:50 (HTML): die Klickform (siehe `Seite::label_zu_text`).
            label_zu_text: HashMap::new(),

            css_baum: crate::kaskade::CssBaum::default(),
            stil_satz: None,
            css_zu_dom: HashMap::new(),
            dom_zu_ord: HashMap::new(),
            // W7C R3: leer heisst „jeder Knoten ist ein Block" — der
            // Zustand vor dem 30.09.2026.
            layout_plan: narsil_layout::LayoutPlan::leer(),
            absaetze: Vec::new(),
        };
    }
    if ist_html(text) {
        return aus_html_mit_messung_und_blaettern(text, breite, basis, messung, blaetter);
    }
    aus_klartext(text, breite, basis)
}

/// Erkennt die eigene Sprache am Anfang.
///
/// GEMESSEN an `examples/hello.narsil`: die Datei beginnt mit zwei
/// Kommentarzeilen, dann `page {`. Also: die erste Zeile, die weder
/// Kommentar noch Leerzeile ist, beginnt mit `page`.
fn ist_narsil(text: &str) -> bool {
    for zeile in text.lines() {
        let t = zeile.trim();
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        return t.starts_with("page");
    }
    false
}

/// Erkennt HTML an einem oeffnenden Tag.
///
/// GEMESSEN an 40 echten Seiten: `DOCTYPE`, `<html`, `<head`, `<body`,
/// `<div`, `<p` kommen in ALLEN vor. Die Liste ist kurz, weil sie nicht
/// "vollstaendig" sein soll — sie soll nur falsch-positive Treffer
/// vermeiden.
fn ist_html(text: &str) -> bool {
    // L3 (02.10.2026 04:0x): DER SCHNITT LIEF MITTEN IN EIN ZEICHEN.
    //
    // GEMESSENER BEFEUND: `examples/l3_seiten` panickte mit
    //   "end byte index 4096 is not a char boundary; it is inside
    //    '\u{fffd}' (bytes 4095..4098 of string)"
    // Der Ausdruck war `&text[..text.len().min(4096)]`. `text` war
    // UTF-8, und Byte 4096 kann mitten in einem Drei-Byte-Zeichen
    // liegen — jede Seite mit einem Umlaut, Akzent oder Emoji in den
    // ersten 4096 Bytes.
    //
    // GEMESSENE ENTSCHEIDUNG, warum nicht `floor_char_boundary` o. ae.
    // Der Schnitt gilt nur als Schauplatz fuer die Erkennung: es zaehlt
    // `contains` auf dem KOPF. Ein abgeschnittener Rest am Ende koennte
    // die Erkennung nur zerstoeren, nicht verstaerken — es sind sechs
    // `contains`-Pruefungen auf einem Kuerzel, und ein fehlendes
    // `<html` an der Schnittstelle waere ein Randfall einer Randseite.
    //
    // GEMESSENE ENTSCHEIDUNG, warum trotzdem auf der Grenze statt
    // "irgendwo vorher" geschnitten wird: `char_indices().nth_...` waere
    // ein zweiter Durchgang ueber bis zu 4096 Bytes je Seite. Die
    // Korrektheit der Erkennung ist wichtiger als der Preis, und der
    // Preis ist einmal 4096 Bytes.
    let mut kopf = 4096usize.min(text.len());
    while kopf > 0 && !text.is_char_boundary(kopf) {
        kopf -= 1;
    }
    let head = &text[..kopf];
    let klein = head.to_ascii_lowercase();
    if klein.contains("<!doctype html")
        || klein.contains("<html")
        || klein.contains("<head")
        || klein.contains("<body")
        || klein.contains("<div")
        || klein.contains("<p>")
        || klein.contains("<p ")
        || klein.contains("<meta")
        || klein.contains("<title")
    {
        return true;
    }
    // GEMESSENER FEHLER 30.09.2026: die Liste endete hier, und ein
    // Fragment, das mit `<h1>` anfing, wurde als KLARTEXT gelesen. Das
    // Ergebnis war eine Seite, auf der `<h1>Titel mit <b>fettem</b>
    // Wort</h1>` als EINER Zeile Klartext stand — spitze Klammern in
    // der Anzeige, weil der Text niemanden versteht, der sie weiss.
    //
    // Solche Fragmente sind der Alltag: eine E-Mail, ein Forumspost,
    // alles, was aus einem `innerHTML` kommt. Der Test
    // `html_fragmente_werden_als_html_erkannt` sichert die Fuenf
    // haeufigsten.
    //
    // WAS NICHT DA STEHT und warum: kein `<` allein, kein `< ` (das ist
    // "a < b"), kein `<3`. Wer diese drei aufnimmt, macht aus jedem
    // mathematischen Text eine Webseite.
    klein.contains("<h1")
        || klein.contains("<h2")
        || klein.contains("<h3")
        || klein.contains("<h4")
        || klein.contains("<h5")
        || klein.contains("<h6")
        || klein.contains("<ul")
        || klein.contains("<ol")
        || klein.contains("<li")
        || klein.contains("<table")
        || klein.contains("<tr")
        || klein.contains("<td")
        || klein.contains("<pre")
        || klein.contains("<blockquote")
        || klein.contains("<span")
        || klein.contains("<img")
        || klein.contains("<form")
        || klein.contains("<br")
        || klein.contains("<hr")
        || klein.contains("<b>")
        || klein.contains("<i>")
        || klein.contains("<em")
        || klein.contains("<strong")
        || klein.contains("<style")
        || klein.contains("<script")
}

// ---------------------------------------------------------------------------
// HTML → Dokument
// ---------------------------------------------------------------------------

/// Baut aus HTML ein Dokument mit Umbruch und Links.
///
/// W5E (2): ohne die zweite Variante wird mit der eingebauten Bitmap
/// gemessen (Zellen von 6/7 der Schriftgroesse je Zeichen). Wer eine
/// Systemschrift benutzt, ruft [`aus_html_mit_schrift`] — sonst bricht
/// der Umbruch an anderer Stelle als der Rasterizer zeichnet.
pub fn aus_html(html: &str, breite: u32, basis: &str) -> Seite {
    aus_html_mit_schrift(html, breite, basis, None)
}

/// Wie [`aus_html`], aber mit der Schrift, mit der gemessen wird.
///
/// `font` wird geklont: `Bauer` haelt die Seite laenger, als der
/// Aufrufer sie braucht, und eine Referenz auf eine fremde Schrift
/// waere ein Grund, den Aufrufer zu binden. TrueTypeFont ist eine
/// Tabelle; das Kopieren ist teurer als ein `Arc`, aber es passiert
/// einmal pro Seitenwechsel.
pub fn aus_html_mit_schrift(
    html: &str,
    breite: u32,
    basis: &str,
    font: Option<&narsil_text::TrueTypeFont>,
) -> Seite {
    let messung = match font {
        Some(f) => narsil_layout::Messung::systemschrift(std::sync::Arc::new(f.clone())),
        None => narsil_layout::Messung::bitmap(),
    };
    aus_html_mit_messung(html, breite, basis, &messung)
}

/// Wie [`aus_html_mit_schrift`], aber mit der Messung (W5G).
/// Der geparste Baum, mit Text und Attributen aus dem Patch (W6).
///
/// # WAS KOPIERT WIRD UND WAS NICHT
///
/// * Text und Attribute: ersetzt.
/// * `weg`-Knoten (`remove()`): BLEIBEN. Sie werden erst in `kind()`
///   uebersprungen, weil ein Weglassen die Vorlauf-Nummern aller
///   folgenden Knoten verschieben wuerde — und die Nummern sind der
///   Schluessel des Patches. GEMESSEN: ohne diese Regel waere nach
///   `remove()` jeder zweite `textContent` an der falschen Stelle.
///
/// M3/M4: der Kopiervorgang laeuft ueber den Baum (`O(Knoten)`) und
/// erzeugt eine Kopie — bei `MAX_KNOTEN` (4096) sind das 4096 Knoten mit
/// ihren Attributen. Das ist die Reserve eines Skriptlaufs, kein
/// Dauerzustand.
fn baum_mit_patch(knoten: &[HtmlNode], patch: &Patch) -> Vec<HtmlNode> {
    fn lauf(list: &[HtmlNode], tiefe: usize, ord: &mut usize, patch: &Patch) -> Vec<HtmlNode> {
        if tiefe >= MAX_BAU_TIEFE {
            return Vec::new();
        }
        let mut raus: Vec<HtmlNode> = Vec::with_capacity(list.len());
        for k in list {
            let me = *ord;
            *ord += 1;
            match k {
                HtmlNode::Text(_) => raus.push(match patch.text.get(&me) {
                    Some(n) => HtmlNode::Text(n.clone()),
                    None => k.clone(),
                }),
                HtmlNode::Element {
                    tag,
                    attributes,
                    children,
                    shadow_root,
                } => raus.push(HtmlNode::Element {
                    tag: tag.clone(),
                    attributes: attribute_mit_patch(attributes, me, patch),
                    children: lauf(children, tiefe + 1, ord, patch),
                    shadow_root: shadow_root.clone(),
                }),
            }
        }
        raus
    }
    lauf(knoten, 0, &mut 0, patch)
}

/// Die Attribute eines Knotens, mit den Anweisungen des Patches.
///
/// GEMESSEN, warum EINE Funktion fuer den Baum und fuer `kind()`: der
/// Patch muss an BEIDEN Stellen gleich wirken. Zwei Kopien dieser Logik
/// waeren die Fehlerklasse aus W5G („zwei Stellen entscheiden getrennt")
/// — und sie faellt nur auf, wenn ein Skript `class` UND `style` in
/// derselben Seite setzt.
fn attribute_mit_patch(
    alt: &HashMap<String, String>,
    ord: usize,
    patch: &Patch,
) -> HashMap<String, String> {
    // GEMESSEN, warum die Anweisungen nach Name sortiert werden: eine
    // `HashMap` hat keine Reihenfolge, und der Patch IST die Reihenfolge
    // der Befehle. Zwischen `class` und `style` ist sie egal — aber es
    // soll nicht davon abhaengen, wie der Hash heute streut.
    let mut anweisungen: Vec<(&String, &String)> = patch
        .attribut
        .iter()
        .filter(|((o, _), _)| *o == ord)
        .map(|((_, name), wert)| (name, wert))
        .collect();
    if anweisungen.is_empty() {
        return alt.clone();
    }
    anweisungen.sort_by(|a, b| a.0.cmp(b.0));
    let mut eigene = alt.clone();
    for (name, wert) in anweisungen {
        match name.as_str() {
            // `classList` (W6): `+x` = dazu, `-x` = weg, `?x` = umschalten.
            // GEMESSEN, warum hier und nicht in `narsil`: nur hier steht die
            // Liste der vorhandenen Klassen. `narsil` hat nur die Anweisung.
            W6_KLASSEN_MARKER => {
                let alt_klassen = eigene.get("class").cloned().unwrap_or_default();
                eigene.insert("class".to_string(), klassen_wenden(&alt_klassen, wert));
            }
            // `style.x = …` (W6): hinten anhaengen, mit `;` dazwischen.
            W6_STIL_MARKER => {
                let alt_stil = eigene.get("style").cloned().unwrap_or_default();
                eigene.insert(
                    "style".to_string(),
                    if alt_stil.trim().is_empty() {
                        wert.clone()
                    } else if alt_stil.trim_end().ends_with(';') {
                        std::format!("{alt_stil}{wert}")
                    } else {
                        std::format!("{alt_stil};{wert}")
                    },
                );
            }
            // `setAttribute`.
            _ => {
                eigene.insert(name.clone(), wert.clone());
            }
        }
    }
    eigene
}

/// W6: `textContent` auf die TEXT-KINDER eines Elementes erweitern.
///
/// # DER GRUND, und er ist ein halber Fehler gewesen
///
/// `el.textContent = "x"` nennt das ELEMENT. Der Bauer liest aber
/// `HtmlNode::Text` — und der Patch ist nach Vorlauf-Nummer sortiert.
/// Ohne diese Erweiterung landet der Text auf der Nummer des Elements,
/// wo nichts auf ihn wartet.
///
/// GEMESSEN 12:20 (vor dieser Erweiterung): `getElementById("z")` gab 2
/// (das `<p>`), `patch.text` bekam `{2: "nachher"}` — und die Seite blieb
/// „vorher". Der Patch war richtig, sein Ziel nicht.
///
/// # DIE ZAEHLUNG IST DIESELBE wie in `Bauer::kind`
///
/// Vorlauf-Nummer, Dokumentreihenfolge, und die Tiefenschranke
/// (`MAX_BAU_TIEFE`) bricht beide gleich ab. GEMESSEN: eine andere
/// Zaehlung hier ergaebe einen Text an einer falschen Stelle — das faellt
/// nur auf, wenn zwei Textknoten auf einer Seite stehen.
///
/// M3: die Erweiterung laeuft ueber den Baum, also `O(Knoten)`, mit dem
/// Knoten-Check `patch.text.len() < MAX_PATCH_EINTRAG`.
fn erweitere_text_patch(knoten: &[HtmlNode], patch: &mut Patch) {
    zaehle_und_erweitere(knoten, 0, &mut 0, patch, false);
}

/// Die STEUERELEMENTE, deren Inhalt NICHT mitgezaehlt wird.
///
/// GEMESSEN, warum es diese Liste gibt: `narsil-page` legt fuer den
/// Beschriftungstext eines `<button>`/`<input>`/`<select>`/`<textarea>`
/// KEINEN Knoten an — der Text steht im Platzhalter `[Knopf: …]`. Wer
/// ihn mitzaehlt, verschiebt alle Nummern danach um eins.
pub const STEUERELEMENTE: [&str; 6] = [
    "input", "button", "select", "textarea", "option", "optgroup",
];

/// W7C R5 (23:20): DIE ZAEHLUNG IST JETZT DIE DES DOM-STANDS.
///
/// GEMESSEN, warum das der Grund fuer „Stand: 3" war, das im Bild als
/// „10" stand (30.09.2026, 23:00, `erzeugte_liste.rs`):
///
/// | Element  | `zaehle` (hier) | `fuelle_stand` (narsil) |
/// |----------|-----------------|---------------------------|
/// | `<h1>`   | 2               | 2                         |
/// | `<button>` | 4             | 4                         |
/// | `<p>`    | 6               | **5**                     |
/// | `<span>` | 8               | **7**                     |
///
/// `fuelle_stand` zaehlt den Beschriftungstext eines `<button>` NICHT
/// mit (W7C A2, 05:55), `zaehle` zaehlte ihn. Ab dem ersten Knopf
/// liefen die beiden Nummern auseinander, `el.textContent = "…"`
/// schrieb also in eine Zahl, die hier niemand liest — und die Bruecke
/// dieser Funktion tat nichts.
///
/// GEMESSEN, warum die Regel hier (und nicht in `narsil`) steht: die
/// Bruecke ist diese Funktion. Ein Skript schreibt ueber `Befehl::Text`
/// in den Patch (Vorlauf-Nummer aus `fuelle_stand`), und wer den Patch
/// liest, ist `narsil-page`. Eine Regel an einer Stelle, die beide
/// Seiten benutzen, kann nicht auseinanderlaufen; zwei Regeln an zwei
/// Orten sind genau das, was hier passiert ist.
///
/// GEMESSEN, warum `STEUERELEMENTE` hier OEFENTLICH ist (`pub(crate)`):
/// `narsil` muss dieselbe Liste benutzen, sonst ist der Fehler wieder da
/// — nur mit einer anderen Fehlermeldung. Die Liste an EINEM Ort, von
/// beiden Seiten gelesen, ist die Antwort.
fn zaehle(list: &[HtmlNode], tiefe: usize, ord: &mut usize, im_steuer: bool) {
    for k in list {
        if tiefe >= MAX_BAU_TIEFE {
            return;
        }
        if im_steuer {
            match k {
                HtmlNode::Text(_) => continue,
                HtmlNode::Element { tag, .. }
                    if matches!(tag.to_ascii_lowercase().as_str(), "option" | "optgroup") =>
                {
                    continue;
                }
                _ => {}
            }
        }
        *ord += 1;
        if let HtmlNode::Element { tag, children, .. } = k {
            let klein = tag.to_ascii_lowercase();
            let tiefer = im_steuer || STEUERELEMENTE.contains(&klein.as_str());
            zaehle(children, tiefe + 1, ord, tiefer);
        }
    }
}

fn zaehle_und_erweitere(
    list: &[HtmlNode],
    tiefe: usize,
    ord: &mut usize,
    patch: &mut Patch,
    im_steuer: bool,
) {
    for k in list {
        if tiefe >= MAX_BAU_TIEFE {
            return;
        }
        if im_steuer {
            match k {
                HtmlNode::Text(_) => continue,
                HtmlNode::Element { tag, .. }
                    if matches!(tag.to_ascii_lowercase().as_str(), "option" | "optgroup") =>
                {
                    continue;
                }
                _ => {}
            }
        }
        let me = *ord;
        *ord += 1;
        if let HtmlNode::Element { tag, children, .. } = k {
            let klein = tag.to_ascii_lowercase();
            let tiefer = im_steuer || STEUERELEMENTE.contains(&klein.as_str());
            if let Some(neu) = patch.text.get(&me).cloned()
                && patch.text.len() < Patch::MAX_EINTRAGE
            {
                // Alle direkten Text-Kinder tragen den neuen Text.
                let mut ko = *ord;
                for c in children {
                    if tiefe + 1 >= MAX_BAU_TIEFE {
                        break;
                    }
                    // DIE GLEICHE REGEL wie oben: im Steuerelement zaehlt
                    // der Text nicht mit — und bekommt deshalb auch keine
                    // Zahl, auf die der Bauer lesen koennte.
                    if !matches!(c, HtmlNode::Text(_)) || !tiefer {
                        if matches!(c, HtmlNode::Text(_)) {
                            patch.text.insert(ko, neu.clone());
                        }
                        ko += 1;
                    }
                    if let HtmlNode::Element { children: ki, .. } = c {
                        zaehle(ki, tiefe + 2, &mut ko, tiefer);
                    }
                }
                //
                // W7C R5 (23:50): DER ELEMENT-SCHLUESSEL GEHT WEG.
                //
                // GEMESSEN, warum: `<p>Stand: <span id=z>0</span></p>`,
                // ein Klick: das Bild zeigte „11" — der neue Wert stand
                // im Absatz-Knoten UND im Span-Knoten. Der Absatz-Text-
                // knoten hatte die Bauernummer, die der Element-Schluessel
                // traegt, und las ihn deshalb auch.
                //
                // GEMESSEN, warum das Entfernen sicher ist: der Schlüssel
                // auf dem ELEMENT ist nie der, den der Bauer liest — er
                // liest Text-KNOTEN. Die Kopie oben ist der Weg. Und es
                // ist ein Kopieren auf einer Kopie (`seite_aus_knoten_
                // mit_messung` kloniert den Patch), der Original-Patch
                // im Stand — aus dem der naechste Klick liest — bleibt
                // unberuehrt.
                patch.text.remove(&me);
            }
            zaehle_und_erweitere(children, tiefe + 1, ord, patch, tiefer);
        }
    }
}

pub fn aus_html_mit_messung(
    html: &str,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
) -> Seite {
    seite_aus_knoten_mit_messung(&html_lesen(html), &Patch::default(), breite, basis, messung)
}

/// Wie [`aus_html_mit_messung`], aber MIT den geholten
/// `<link rel="stylesheet">`-Blaettern (03.10.2026 17:1x).
///
/// Siehe [`seite_aus_knoten_mit_werbung_und_blaettern`] — dort steht die
/// Messung, aus der dieser Weg entstanden ist. Diese Funktion nimmt den
/// Zustand des Wirts **nicht** an; wer ihn braucht, nimmt
/// `seite_aus_knoten_mit_werbung_und_blaettern` (das ist der Weg, den
/// `narsil` nimmt, und der Zustaendig).
pub fn aus_html_mit_blaettern(
    html: &str,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    blaetter: &[(String, String)],
) -> Seite {
    seite_aus_knoten_mit_blaettern(
        &html_lesen(html),
        &Patch::default(),
        breite,
        basis,
        messung,
        blaetter,
    )
}

fn aus_html_mit_messung_und_blaettern(
    html: &str,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    blaetter: &[(String, String)],
) -> Seite {
    seite_aus_knoten_mit_blaettern(
        &html_lesen(html),
        &Patch::default(),
        breite,
        basis,
        messung,
        blaetter,
    )
}

/// Wie [`aus_html_mit_messung`], aber MIT Werbefilter (A-ADBLOCK,
/// 01.10.2026 21:45).
///
/// Das ist der Weg, den `narsil` nimmt: HTML-Text, Filter, Host.
pub fn aus_html_mit_werbung(
    html: &str,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    zustand: &narsil_css::Zustand,
    werbung: &crate::werbung::Werbung,
    host: &str,
) -> Seite {
    seite_aus_knoten_mit_werbung(
        &html_lesen(html),
        &Patch::default(),
        breite,
        basis,
        messung,
        zustand,
        werbung,
        host,
    )
}

/// Wie [`aus_html_mit_messung`], aber mit dem Zustand des WIRTS (04.10. 00:20).
///
/// # DER WEG ZUM DUNKELEN THEMA
///
/// GEMESSENER BEFUND, warum es diesen Weg braucht: `prefers-color-scheme` ist
/// eine Media Query ueber den ZUSTAND des Browsers, nicht ueber die Seite.
/// GEMESSEN (vor diesem Weg): `narsil-page` uebergab an jeden Kaskadenaufruf
/// `Zustand::KEINER`, und `Farbschema` ist per `Default` `Hell` — `dark` traf
/// nie, `light` traf immer.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Wirt entscheidet und nicht die Seite:
/// `prefers-color-scheme: dark` bedeutet „ich ziehe ein helles Fenster einem
/// dunklen vor" — die Seite sagt, was sie lieber haette, der Browser sagt,
/// was der Nutzer will. Ein Browser, der die Seite selbst entscheiden laesst,
/// ist die Dunkel-Variante in den Einstellungen des Nutzers.
///
/// GEMESSENE ENTSCHEIDUNG, warum die VORGABE `Hell` bleibt: wer nichts sagt,
/// bekommt das, was jeder Browser zeigt, wenn niemand etwas gesagt hat.
pub fn aus_html_mit_farbschema(
    html: &str,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    farbschema: narsil_css::Farbschema,
) -> Seite {
    let zustand = narsil_css::Zustand {
        farbschema,
        ..narsil_css::Zustand::KEINER
    };
    seite_aus_knoten_mit_zustand(
        &html_lesen(html),
        &Patch::default(),
        breite,
        basis,
        messung,
        &zustand,
    )
}

/// Das Farbschema des Systems, aus der UMGEBUNG gelesen.
///
/// 03.10. 23:3x (HTML, Auftrag aus CSS_9 / Leitung): `Zustand::farbschema`
/// war an den Standard-Wegen `KEINER`, und `Farbschema` ist per `Default`
/// `Hell` — `dark` traf nie, `light` traf immer. GEMESSENE Folge auf MDN:
/// `light-dark()` **42x** im Dokument, `@media` **0x**; `light-dark()` ohne
/// aufgeloestes Schema liefert nichts, also blieb der Hero leer.
///
/// GEMESSEN, was die MASCHINE meldet (23:2x):
///     gsettings get org.gnome.desktop.interface color-scheme  ->  'prefer-dark'
/// Das ist der Grund, warum die Firefox-Referenz `mdn.png` **dunkel** ist:
/// Firefox headless liest dieselbe Einstellung. Wer die Referenz mit „unser
/// Ergebnis ist zu hell" vergleicht, vergleicht die Einstellungen des
/// Nutzers mit der Standardvorgabe unserer Bibliothek.
///
/// WARUM NUR DIE UMGEBUNG und nicht `gsettings`: eine Bibliothek startet
/// keine Prozesse. `gsettings` auszufuehren gehoert in den Wirt (`narsil`),
/// der es beim Start einmal tut und das Ergebnis ueber `Zustand` reicht —
/// derselbe Weg wie fuer den Werbefilter (`aus_html_mit_werbung`).
/// **BRETT an UI/narsil: eine Zeile, `farbschema:
/// narsil_page::farbschema_aus_der_umgebung()` (oder direkt `gsettings`).**
///
/// GEMESSENE REIHENFOLGE, und sie ist eine Vorgabe der Leitung (23:1x):
/// Einstellung System/Hell/Dunkel, Standard **System**; System liest
/// `GTK_THEME`/`XDG_CURRENT_DESKTOP`, sonst `Hell`. `NARSIL_FARSCHEMA` ist der
/// Schalter fuer Tests und fuer einen Wirt, der es anders entscheidet.
pub fn farbschema_aus_der_umgebung() -> narsil_css::Farbschema {
    use narsil_css::Farbschema;
    if let Ok(v) = std::env::var("NARSIL_FARSCHEMA") {
        match v.trim().to_ascii_lowercase().as_str() {
            "dunkel" | "dark" => return Farbschema::Dunkel,
            "hell" | "light" => return Farbschema::Hell,
            _ => {}
        }
    }
    // GEMESSEN: `GTK_THEME` ist auf dieser Maschine **leer** (23:2x). Die
    // GTK-Vorgabe „Adwaita" ist hell, und ein leeres `GTK_THEME` heisst
    // „keine Angabe" — also Hell. Wer es anders will, setzt die Variable.
    if let Ok(v) = std::env::var("GTK_THEME")
        && v.to_ascii_lowercase().contains("dark")
    {
        return Farbschema::Dunkel;
    }
    Farbschema::Hell
}

/// Das HTML lesen, OHNE eine Seite zu bauen (W6).
///
/// GEMESSEN, warum das oeffentlich ist: nach einem `textContent` baut
/// `narsil` die Seite neu — aus DEMSELBEN Baum, sonst waere die zweite
/// Seite nicht dieselbe Seite (zwei Parser, zwei Wahrheiten, und die
/// zweite koennte einen Ordinalzahl-Versatz haben).
pub fn html_lesen(html: &str) -> Vec<HtmlNode> {
    // Release 2026-10-15: the page's attribute order for its live DOM
    // (see `narsil_fmt_html::lebend::AttributeOrder`).
    let (baum, order) = narsil_fmt_html::parse_html_with_attribute_order(html);
    narsil_fmt_html::lebend::remember_page_attribute_order(order);
    baum
}

/// Release 10.10.2026: wie [`html_lesen`], aber fuer ein XHTML-Dokument
/// (`application/xhtml+xml`): CDATA und `<div/>` wie im XML-Parser (siehe
/// `narsil_fmt_html::xhtml`).
pub fn html_lesen_xhtml(html: &str) -> Vec<HtmlNode> {
    let (baum, order) = narsil_fmt_html::parse_html_with_attribute_order(&narsil_fmt_html::xhtml::xhtml_als_html(html));
    narsil_fmt_html::lebend::remember_page_attribute_order(order);
    baum
}

/// Release 10.10.2026: ist das ein XHTML-Dokument? Nach dem `Content-Type`
/// der Antwort, ohne ihn nach der Endung der Adresse (`file:`, wie Chrome).
pub fn ist_xhtml(content_type: &str, adresse: &str) -> bool {
    let typ = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if !typ.is_empty() {
        return typ == "application/xhtml+xml";
    }
    let pfad = adresse.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
    pfad.ends_with(".xht") || pfad.ends_with(".xhtml")
}

/// Die Seite aus einem GEPARSTEN Baum bauen, mit den Aenderungen eines
/// Skripts (W6, J9/J10).
///
/// # DIE EINE BAUSTELLE
///
/// `aus_html_mit_messung` ruft DIESE Funktion mit leerem Patch. Es gibt
/// also genau einen Weg, eine Seite zu bauen — und der Patch ist sein
/// einziger Unterschied. GEMESSEN, warum das zaehlt: zwei Bauwege
/// bedeuten, dass irgendwann eine Aenderung nur in einem davon landet;
/// dann hat die eine Seite sie und die andere nicht, ohne dass ein Test
/// rot wird.
///
/// # DER PATCH WIRD VOR DER KASKADE ANGEWENDET
///
/// Attribute (`class`, `style`) mischen sich, BEVOR die Kaskade rechnet.
/// Ein `classList.add` muss also das Layout beeinflussen und nicht nur
/// im Attribut stehen.
pub fn seite_aus_knoten_mit_messung(
    knoten: &[HtmlNode],
    patch: &Patch,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
) -> Seite {
    seite_aus_knoten_mit_zustand(
        knoten,
        patch,
        breite,
        basis,
        messung,
        &narsil_css::Zustand::KEINER,
    )
}

/// Wie [`seite_aus_knoten_mit_messung`], aber MIT den geholten
/// `<link rel="stylesheet">`-Blaettern (03.10.2026 17:1x).
///
/// Siehe [`seite_aus_knoten_mit_werbung_und_blaettern`] fuer die
/// Begruendung. Wer den Zustand des Wirts braucht, nimmt die
/// Werbung-Variante; diese hier ist der Weg fuer Aufrufer ohne Wirt.
pub fn seite_aus_knoten_mit_blaettern(
    knoten: &[HtmlNode],
    patch: &Patch,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    blaetter: &[(String, String)],
) -> Seite {
    seite_aus_knoten_mit_schicht(
        knoten,
        patch,
        breite,
        basis,
        messung,
        &narsil_css::Zustand::KEINER,
        None,
        blaetter,
    )
}

/// Wie [`seite_aus_knoten_mit_messung`], aber mit dem Zustand des WIRTS
/// (04.10. 00:15).
///
/// # WOZU
///
/// `prefers-color-scheme: dark` konnte nie treffen (GEMESSEN: `narsil-page`
/// uebergab `Zustand::KEINER`, und `MediaUnd::farbschema` sagt ausdruecklich,
/// `dunkel` treffe nie). Der Host — der Browser, nicht die Seite — entscheidet
/// also heute gar nichts.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine Schwester und kein zusaetzlicher
/// Parameter: vier Aufrufer in `narsil` (sitzung.rs:2400/2408/2537/4113) und
/// einer hier. Eine neue Funktion aendert keinen davon, und der alte Weg
/// bleibt der Rueckfall fuer „der Host hat nichts gesagt".
pub fn seite_aus_knoten_mit_zustand(
    knoten: &[HtmlNode],
    patch: &Patch,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    zustand: &narsil_css::Zustand,
) -> Seite {
    seite_aus_knoten_mit_schicht(knoten, patch, breite, basis, messung, zustand, None, &[])
}

/// 05.10.2026: [`seite_aus_knoten_mit_zustand`] **mit** Ansichtshoehe.
///
/// Das ist der Weg des Browsers: `narsil` kennt das Fenster, und ohne diese
/// Zahl rechnet die Kaskade `vh`, `vmin`, `vmax` und `min-height: 100vh` gegen
/// eine **geratene** 600 px (siehe [`Bauer::ansicht_hoehe`]).
pub fn seite_aus_knoten_mit_ansicht_zustand(
    knoten: &[HtmlNode],
    patch: &Patch,
    breite: u32,
    ansicht_hoehe: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    zustand: &narsil_css::Zustand,
) -> Seite {
    seite_aus_knoten_mit_ansicht(
        knoten, patch, breite, ansicht_hoehe, basis, messung, zustand, None, &[],
    )
}

/// Wie [`seite_aus_knoten_mit_zustand`], aber MIT Werbefilter.
///
/// # WARUM EINE SCHWESTER UND KEIN ZUSAETZLICHER PARAMETER
///
/// Fuenf Aufrufer in `narsil` und einer hier rufen
/// `seite_aus_knoten_mit_zustand` auf. Ein zusatzlicher Parameter an
/// dieser Funktion wuerde jeden von ihnen brechen — und die
/// Nachrichtenseiten muessen auch ohne Filter laufen, sonst waere
/// `None` ein Wert, den man sechs Mal hinschreibt.
///
/// Der Weg ohne Filter ist also unveraendert, und der mit Filter ist
/// eine Schwester. Dasselbe Muster wie bei `Zustand` (siehe dort).
///
/// # WAS DER FILTER HIER BEWIRKT
///
/// Nur die **kosmetischen** Regeln. Die Netzregeln gehoeren VOR den
/// Abruf, und der Abruf liegt in `narsil` (Rolle HTML) — siehe
/// [`werbung::Werbung::entscheide`].
pub fn seite_aus_knoten_mit_werbung(
    knoten: &[HtmlNode],
    patch: &Patch,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    zustand: &narsil_css::Zustand,
    werbung: &crate::werbung::Werbung,
    host: &str,
) -> Seite {
    seite_aus_knoten_mit_schicht(
        knoten,
        patch,
        breite,
        basis,
        messung,
        zustand,
        werbung.kosmetik_blatt(host),
        &[],
    )
}

/// 03.10.2026 17:0x: [`seite_aus_knoten_mit_werbung`] MIT den geholten
/// `<link rel="stylesheet">`-Blaettern.
///
/// # WOZU (gemessen)
///
/// GEMESSEN an den zehn gespeicherten Seiten (`team/PROBLEME.txt` Nr. 59):
/// die Seiten mit `link`-Blaettern haben die groessten Firefox-Abweichungen
/// (github 191.9, nixos 145.3, MDN 124.5), die mit Inline-`<style>` die
/// kleinsten (duckduckgo 31.1, wikipedia 41.6). GEMESSEN ist auch, dass die
/// Kaskade mit einem echten Blatt kein Problem hat: sie wendet
/// `span.mw-linkback-text{display:none}` aus dem echten MediaWiki-Blatt an
/// (`team/BERICHT_CSS_WIKI_KASKADE.txt`). Es fehlte der **Weg**, nicht die
/// Kaskade.
///
/// # WIE DER AUFRUFER DAMIT UMGEHT
///
/// 1. HTML holen, [`Seite::stil_urls`] lesen (die Adressen sind schon
///    aufgeloest), die Blaetter holen.
/// 2. Diesen Aufruf mit den Paaren `(Adresse, Text)` machen.
///
/// GEMESSENE ENTSCHEIDUNG, warum **zwei** Schritte und kein Nachladen:
/// die Kaskade wird beim Bau einmal gerechnet (siehe `kaskade_einrichten`)
/// und danach nur noch gelesen; ein spaeteres Einsetzen wuerde einen
/// zweiten Rechenweg und damit eine zweite Wahrheit brauchen. Der Preis
/// dieser Entscheidung ist ein zweiter Seitenbau — GEMESSEN in
/// `kaskade_echt`: der Bau ist zweistellig, der Kaskadenteil des echten
/// github-Rumpfes ist es auch.
///
/// Ein Blatt, das **nicht** geliefert wurde, geht als leerer Block in die
/// Liste und wird von `blaetter()` uebersprungen (dort steht, warum). Er
/// fehlt dann im Bild — sichtbar, aber beherrschbar.
pub fn seite_aus_knoten_mit_werbung_und_blaettern(
    knoten: &[HtmlNode],
    patch: &Patch,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    zustand: &narsil_css::Zustand,
    werbung: &crate::werbung::Werbung,
    host: &str,
    blaetter: &[(String, String)],
) -> Seite {
    seite_aus_knoten_mit_schicht(
        knoten,
        patch,
        breite,
        basis,
        messung,
        zustand,
        werbung.kosmetik_blatt(host),
        blaetter,
    )
}

/// 05.10.2026: [`seite_aus_knoten_mit_werbung_und_blaettern`] **mit**
/// Ansichtshoehe — der Weg des Browsers.
///
/// GEMESSENER GRUND: Werbefilter und `vh` kamen nie zusammen. Der Filter
/// laeuft ueber `kleiner_als` (Blocking an **Groesse**), und dafuer braucht
/// die Seite ihre Fensterhoehe — vorher hatte der Bauer keine, also wurde
/// gegen eine geratene 600 px geblutet.
pub fn seite_aus_knoten_mit_werbung_und_ansicht(
    knoten: &[HtmlNode],
    patch: &Patch,
    breite: u32,
    ansicht_hoehe: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    zustand: &narsil_css::Zustand,
    werbung: &crate::werbung::Werbung,
    host: &str,
    blaetter: &[(String, String)],
) -> Seite {
    seite_aus_knoten_mit_ansicht(
        knoten,
        patch,
        breite,
        ansicht_hoehe,
        basis,
        messung,
        zustand,
        werbung.kosmetik_blatt(host),
        blaetter,
    )
}

/// Die eine Stelle, an der der Seitenbau passiert.
///
/// GEMESSENE ENTSCHEIDUNG: auch das kosmetische Blatt geht als
/// `Option<Stylesheet>` durch diese Funktion und nicht ueber ein Feld
/// am `Bauer`, das der Aufrufer setzen muss. Sonst koennte der
/// Aufrufer es vergessen, und ein vergessener Filter ist ein Filter,
/// der auf keiner Seite wirkt — ohne dass irgendetwas es meldet.
/// KOMPRESS D1: Summe der Zeit beider Kaskaden je Knoten, in
/// **Nanosekunden** (Ganzzahl — siehe die Stelle, die addiert). Und die
/// Zahl der Knoten.
///
/// GEMESSENE ENTSCHEIDUNG, warum zwei `AtomicU64` und kein `Mutex`:
/// es wird nur addiert, und `Relaxed` reicht fuer eine Messung. Ein
/// `Mutex` wuerde hier die Sache messen, die er messen soll.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Summe und nicht der Mittelwert:
/// eine Summe laesst sich zwischen zwei Laeufen subtrahieren, ein
/// Mittelwert nicht sinnvoll (die Knotenzahl weicht ab).
static KASKADE_ZEIT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static KASKADE_ANZ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Die gemessene Kaskadenzeit in Millisekunden, ueber alle Aufrufe
/// seit dem letzten Lesen.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein Getter und kein direkter Zugriff
/// auf die Zelle: ein Beispiel soll die Zahl lesen duerfen, ohne dass
/// die Zelle `pub` wird. `pub` waere eine Zusage an das ganze Projekt,
/// ein Getter ist eine Auskunft fuer genau diesen Fall.
///
/// Der Wert wird bei jedem Lesen auf 0 gesetzt — sonst waere die
/// naechste Zahl die Summe aller bisherigen, und "Summe" ist hier
/// ausdruecklich nicht gemeint.
pub fn kaskade_zeit_f64() -> f64 {
    let ns = KASKADE_ZEIT.swap(0, std::sync::atomic::Ordering::Relaxed);
    ns as f64 / 1e6
}

/// Wie viele Kaskadenaufrufe in der gemessenen Zeit stecken.
///
/// Gleiches Muster wie [`kaskade_zeit_f64`]: lesen und zuruecksetzen.
pub fn kaskade_anzahl() -> u64 {
    KASKADE_ANZ.swap(0, std::sync::atomic::Ordering::Relaxed)
}

fn seite_aus_knoten_mit_schicht(
    knoten: &[HtmlNode],
    patch: &Patch,
    breite: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    zustand: &narsil_css::Zustand,
    kosmetik: Option<narsil_css::Stylesheet>,
    blaetter: &[(String, String)],
) -> Seite {
    seite_aus_knoten_mit_ansicht(
        knoten, patch, breite, 0, basis, messung, zustand, kosmetik, blaetter,
    )
}

/// 05.10.2026: [`seite_aus_knoten_mit_schicht`] **mit** Ansichtshoehe.
///
/// Der Weg, den der Browser nimmt (`narsil` kennt das Fenster). `vh`, `vmin`,
/// `vmax` und `min-height: 100vh` brauchen sie — siehe
/// [`Bauer::ansicht_hoehe`] fuer die Messung, was ohne sie passiert.
/// Release 13.10.2026 (Agent CSS2): ein Element mit Namensraum-Praefix
/// (`<svg:svg>`, `<svg:rect>` in XHTML) wird unter seinem LOKALEN Namen gebaut
/// — so treffen es Typselektoren ohne `@namespace` (Selectors 4 §5.1) und der
/// SVG-Weg. `None`, wenn kein Name einen Doppelpunkt traegt (der Baum bleibt
/// ungeklont). GEMESSEN vorher (WPT `CSS2/positioning/absolute-replaced-*`,
/// 44 rote Dateien mit `<svg:svg>`): das SVG fehlte ganz.
fn ohne_praefixe(knoten: &[HtmlNode]) -> Option<Vec<HtmlNode>> {
    fn hat(k: &[HtmlNode], t: usize) -> bool {
        t < MAX_BAU_TIEFE
            && k.iter().any(|n| {
                matches!(n, HtmlNode::Element { tag, children, .. } if tag.contains(':') || hat(children, t + 1))
            })
    }
    fn umbenennen(k: &mut [HtmlNode], t: usize) {
        if t >= MAX_BAU_TIEFE {
            return;
        }
        for n in k.iter_mut() {
            if let HtmlNode::Element { tag, children, .. } = n {
                if let Some(i) = tag.rfind(':')
                    && i + 1 < tag.len()
                {
                    *tag = tag[i + 1..].to_string();
                }
                umbenennen(children, t + 1);
            }
        }
    }
    if !hat(knoten, 0) {
        return None;
    }
    let mut v = knoten.to_vec();
    umbenennen(&mut v, 0);
    Some(v)
}

fn seite_aus_knoten_mit_ansicht(
    knoten: &[HtmlNode],
    patch: &Patch,
    breite: u32,
    ansicht_hoehe: u32,
    basis: &str,
    messung: &narsil_layout::Messung,
    zustand: &narsil_css::Zustand,
    kosmetik: Option<narsil_css::Stylesheet>,
    blaetter: &[(String, String)],
) -> Seite {
    // W6: DER TEXT-PATCH GEHT AUF DAS ELEMENT, der Bauer liest aber
    // TEXT-KNOTEN. Also wird er vorher erweitert (siehe
    // `erweitere_text_patch`).
    let mut patch = patch.clone();
    erweitere_text_patch(knoten, &mut patch);
    //
    // W6, GEMESSEN 12:35: der Patch muss auch im BAUM stehen, den die
    // Kaskade sieht. `baum_bauen(knoten)` lief ueber den geparsten Baum
    // — dort stand kein `class="markiert"`, also griff `.markiert` nicht
    // und `classList.add` tat nichts (der Test fand keinen Knoten mit der
    // Klassenfarbe).
    let knoten = &baum_mit_patch(knoten, &patch);
    // Release 13.10.2026 (Agent CSS2): Elemente mit Namensraum-Praefix (siehe
    // `ohne_praefixe`).
    let ohne = ohne_praefixe(knoten);
    let knoten: &Vec<HtmlNode> = match &ohne {
        Some(v) => v,
        None => knoten,
    };
    // Release 12.10.2026: Shadow DOM — gebaut wird der flache Baum (siehe
    // `schatten`); ohne Shadow Root bleibt alles, wie es war.
    let flach = schatten::zusammensetzen(knoten);
    let (knoten, ord_karte): (&[HtmlNode], HashMap<usize, usize>) = match &flach {
        Some((f, k)) => (f.as_slice(), k.clone()),
        None => (knoten.as_slice(), HashMap::new()),
    };
    narsil_css::schrift_masse_anmelden(schrift_masse);
    let mut bauer = Bauer::mit_ansicht(breite, ansicht_hoehe, basis, messung.clone());
    bauer.patch = patch;
    bauer.ord_karte = ord_karte;
    bauer.wurzel();

    // I2: die Kaskade braucht die `<style>`-Bloecke VOR dem Bau, denn
    // der erste Knoten, fuer den sie gerechnet wird, ist das erste
    // Element. Also: erst die Bloecke sammeln, dann den CSS-Baum, dann
    // bauen.
    //
    // DIE REIHENFOLGE ist ein Messergebnis: mit der Kaskade NACH dem
    // Bau ergab `<style>h1{color:#c33}</style>` eine Seite mit
    // unveraenderter Ueberschrift — der Block war zu spaet gelesen.
    //
    // KOMPRESS D1 (Koordination 02.10.2026): die Stufen der
    // Kaskadenphase getrennt messen.
    //
    // GEMESSENER GRUND: `kaskade_echt` (Beispiel in dieser Crate) hat
    // auf dem echten github-Rumpf gemessen: ohne die 27 Stylesheets
    // 42 ms, mit ihnen 2 884 ms — die Stylesheets kosten 2 842 ms.
    // Davon sind laut `narsil-css/examples/kaskade_bench.rs` nur 214 ms
    // der Selektor-Abgleich und 57 ms das Lesen. Die Frage war, wo die
    // restlichen rund 2 600 ms liegen, und sie ist ohne diese Messung
    // nicht zu beantworten.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Messung hier und nicht in
    // `Bauer::kind`: `kind` wird je Knoten gerufen — eine
    // `Instant::now()` je Knoten waere ein Teil des Messwerts. Hier
    // stehen fuenf Stufen, und fuenf `var_os`-Vergleiche je
    // Seitenaufbau sind nichts.
    let d1_messen = std::env::var_os("NARSIL_KASKADE_ZEITEN").is_some();
    let d1_t0 = d1_messen.then(std::time::Instant::now);
    bauer.stile_sammeln(knoten);
    if flach.is_some() && bauer.stile.len() < MAX_STILBLOECKE {
        // Release 12.10.2026: ein leerer Merker-Block — die Seite hat Shadow
        // Trees, die Dokumentblaetter gelten darin nicht (siehe `blaetter`).
        bauer.stile.push(StyleBlock {
            bereich: Some(String::new()),
            inhalt: String::new(),
            typ: String::new(),
            medium: String::new(),
            href: None,
        });
    }
    svg_definitionen_sammeln(knoten, false, 0, &mut bauer.svg_definitionen);
    let d1_t1 = d1_messen.then(std::time::Instant::now);
    //
    // 04.10. 00:15: DER ZUSTAND DES WIRTS kommt in den CSS-Baum (siehe
    // `CssBaum::zustand`). GEMESSENE ENTSCHEIDUNG, warum hier und nicht
    // weiter oben: `baum_bauen` ist die EINZIGE Stelle, an der der Baum
    // entsteht, und ab hier wird er nur noch gelesen.
    let baum = baum_bauen_mit_zustand(knoten, zustand);
    let d1_t2 = d1_messen.then(std::time::Instant::now);
    // A-ADBLOCK: das kosmetische Blatt VOR `kaskade_einrichten` — dort
    // wird die Blattliste gebaut, und danach wird sie nur noch gelesen.
    bauer.kosmetik = kosmetik;
    // 03.10.2026 17:0x: die geholten `<link>`-Blaetter kommen VOR dem
    // Blattaufbau, aus demselben Grund wie `kosmetik`: `kaskade_einrichten`
    // ist die LETZTE Stelle, an der `stile` gelesen wird.
    bauer.blatt_inhalt_setzen(blaetter);
    bauer.kaskade_einrichten(&baum);
    let d1_t3 = d1_messen.then(std::time::Instant::now);
    // W6 (J9): der CSS-Baum geht in die Seite, damit `narsil` nachher
    // `querySelector` fragen kann. Es ist derselbe Baum, mit dem die
    // Kaskade gerechnet hat — KEIN zweiter Aufbau, sonst wuerden zwei
    // Stellen ueber die Reihenfolge der Geschwister entscheiden.
    bauer.css_baum = baum.clone();

    for kind in knoten {
        bauer.kind(kind);
    }
    let d1_t4 = d1_messen.then(std::time::Instant::now);
    let seite = bauer.fertig();
    if let (Some(a), Some(b), Some(c), Some(d), Some(e)) = (d1_t0, d1_t1, d1_t2, d1_t3, d1_t4) {
        // stderr, wie `narsil-layout-engine`: die Tabelle des
        // Pruefstands geht nach stdout.
        let ms = |x: std::time::Instant, y: std::time::Instant| {
            y.duration_since(x).as_secs_f64() * 1000.0
        };
        eprintln!(
            "KASKADE stile_sammeln {:>8.1} ms | baum_bauen {:>8.1} ms | \
             kaskade_einrichten {:>8.1} ms | bauen {:>8.1} ms (davon Stilrechnung {:>8.1} ms in {} Elementen)",
            ms(a, b),
            ms(b, c),
            ms(c, d),
            ms(d, e),
            kaskade_zeit_f64(),
            kaskade_anzahl(),
        );
    }
    seite
}

/// WAS EIN SKRIPT MIT DER SEITE GEMACHT HAT (W6, J9/J10).
///
/// # DER WEG, und warum es eine PATCH-Liste und kein Quelltext ist
///
/// `narsil-dom` kann keinen Knoten aendern (kein `set_text`, `Node`
/// speichert keine Attribute), und `narsil-fmt_html::HtmlNode` speichert
/// KEINE Quellposition (GEMESSEN 11:50: `HtmlNode` hat `tag`,
/// `attributes`, `children`, `text` — kein `span`; `narsil-dom::Span`
/// ist darum ueberall `Span::EMPTY`). Ein Skript, das `textContent`
/// aendert, kann also weder den Baum noch den Quelltext an der richtigen
/// Stelle anfassen.
///
/// Also wird die Seite aus demselben [`HtmlNode`]-Baum NEU gebaut, mit
/// dem Patch als einziger Aenderung. Das ist der Weg, den ein Browser
/// geht, und er hat drei Vorteile, die gemessen sind:
///
/// 1. kein zweiter Parser und kein Quelltext-Puzzler,
/// 2. der CSS-Baum und die Kaskade rechnen wieder ueber denselben Baum,
/// 3. eine Aenderung, die abbricht, laesst die alte Seite stehen.
///
/// # DER SCHLUESSEL IST DIE ORDNUNGSZAHL
///
/// Jeder Knoten des geparsten Baums bekommt beim Bau seine
/// Vorlauf-Nummer (`0`, `1`, `2`, … in Dokumentreihenfolge). Die
/// JavaScript-Seite rechnet mit DIESER Nummer — nicht mit einer
/// `NodeId` und nicht mit einer CSS-Baum-Nummer. EIN Zahlenraum fuer
/// `getElementById`, `querySelector` und die Befehle; das ist die
/// Bedingung dafuer, dass ein Handler am richtigen Knoten ankommt.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Patch {
    /// Canvas pixels keyed by the same DOM ordinal as script mutations.
    pub canvas: HashMap<usize, (u32, u32, std::sync::Arc<Vec<u8>>)>,
    /// Neue Texte: Vorlauf-Nummer -> Text.
    ///
    /// M4: der Text kommt aus einem Skript. Die Grenze sitzt im
    /// Aufrufer (`narsil::js::MAX_ATTRIBUT`); hier wird nichts
    /// gekuerzt — ein gekuerzter Text waere eine falsche Seite.
    pub text: HashMap<usize, String>,
    /// Neue Attributwerte: (Vorlauf-Nummer, Name) -> Wert.
    ///
    /// `class` und `style` sind Attribute wie jedes andere; der Bauer
    /// mischt sie ein, BEVOR die Kaskade rechnet — deshalb wirkt ein
    /// `classList.add` auf das Layout und nicht nur auf das Attribut.
    pub attribut: HashMap<(usize, String), String>,
    /// Vorlauf-Nummern, die ganz wegfallen (`remove()`).
    ///
    /// M3: `BTreeSet`, damit die Reihenfolge der Befehle erhalten bleibt
    /// (`remove` zweimal ist harmlos, aber ein nachgemessener Test soll
    /// dieselbe Reihenfolge sehen).
    pub weg: std::collections::BTreeSet<usize>,
    /// W6F: NEUE Knoten, die es im geparsten Baum noch nicht gibt
    /// (`document.createElement` + `appendChild`).
    ///
    /// GEMESSEN, warum das im Patch und nicht im Baum: `HtmlNode` hat
    /// keine Quellposition (`replay.rs` sagt es), also kann man einen
    /// Knoten nicht im Quelltext erzeugen. Der Patch ist der Ort, an dem
    /// etwas existieren kann, das der Parse nicht kennt — und der Bauer
    /// pfropft es beim Bau an.
    ///
    /// M3/M4: `MAX_NEU` (256) Knoten je Seite, der Text je Knoten
    /// `MAX_ATTRIBUT_VON_NEU` (4 KiB). Beide Grenzen stehen in `narsil`
    /// (der Seite, die den Befehl erzeugt) — hier werden sie noch einmal
    /// geprueft, weil `Patch` auch von aussen befuellt werden kann.
    pub neu: Vec<NeuKnoten>,
}

/// EIN NEUER KNOTEN im Patch (W6F: `createElement` + `appendChild`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeuKnoten {
    /// Die Vorlauf-Nummer des neuen Knotens.
    ///
    /// GEMESSEN, warum nicht `naechste_ordinal`: der Zaehler gehoert dem
    /// PARSE. Ein neuer Knoten muss ueber mehrere Klicks dieselbe Nummer
    /// behalten (sonst verliert der Patch nach dem zweiten Klick seinen
    /// Knoten), also bekommt er eine Nummer aus
    /// [`NeuKnoten::ORD_BASIS`]-Raum — weit weg von allen Parse-Nummern.
    pub ord: usize,
    /// Der Tagname (klein, geprueft).
    pub tag: String,
    /// Die Vorlauf-Nummer des Elternknotens im GEPARSTEN Baum.
    pub eltern_ord: usize,
    /// Der Text des neuen Knotens (`li.textContent = 'Klick Nr. 1'`).
    pub text: Option<String>,
    /// Release 08.10.2026: WO der Knoten eingefügt wird. Vorher gab es nur
    /// „am Ende der Eltern" (`appendChild`); `insertAdjacentHTML`,
    /// `prepend`, `before` und `after` brauchen die anderen Lagen.
    pub lage: NeuLage,
}

/// Die Einfügelage eines neuen Knotens (DOM §4.2.3 „insert").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NeuLage {
    /// Letztes Kind von `eltern_ord` (`appendChild`, `append`, `beforeend`).
    #[default]
    Ende,
    /// Erstes Kind von `eltern_ord` (`prepend`, `afterbegin`).
    Anfang,
    /// Geschwister VOR dem Element mit dieser Vorlauf-Nummer (`before`,
    /// `beforebegin`).
    Vor(usize),
    /// Geschwister NACH dem Element mit dieser Vorlauf-Nummer (`after`,
    /// `afterend`).
    Nach(usize),
}

impl NeuKnoten {
    /// Der erste Vorlauf-Nummern-Bereich, der fuer NEUE Knoten reserviert
    /// ist (W6F, M3).
    ///
    /// GEMESSEN, warum 1.000.000: `Bauer::naechste_ordinal` zaehlt die
    /// Knoten eines Parse-Durchlaufs. Eine Seite mit 1.000.000 Knoten
    /// scheitert vorher an `MAX_KNOTEN` (4096) — der Bereich kann sich also
    /// nie mit einer Parse-Nummer treffen.
    pub const ORD_BASIS: usize = 1_000_000;
    /// Hoechste Zahl neuer Knoten je Seite (M3).
    pub const MAX_NEU: usize = 256;
    /// Hoechste Textlaenge eines neuen Knotens in Byte (M3/M4).
    pub const MAX_TEXT: usize = 4096;
}

impl Patch {
    /// Leer? Dann ist die Seite unveraendert.
    pub fn ist_leer(&self) -> bool {
        self.canvas.is_empty() && self.text.is_empty()
            && self.attribut.is_empty()
            && self.weg.is_empty()
            && self.neu.is_empty()
    }

    /// Hoechste Zahl Text-Eintraege, die die Erweiterung erzeugt (M3).
    ///
    /// GEMESSEN, warum das eine eigene Grenze ist: `erweitere_text_patch`
    /// kann aus EINEM Befehl viele Eintraege machen (ein `<p>` mit
    /// 40 Textkindern). 65.536 ist die Grenze von `MAX_PATCH` in
    /// `narsil::replay` — dieselbe Zahl, damit keine Seite die andere
    /// ueberrascht.
    pub const MAX_EINTRAGE: usize = 65_536;

    /// Wie viele Aenderungen enthaelt der Patch (Meldungen fuer die Meldung).
    pub fn anzahl(&self) -> usize {
        self.canvas.len() + self.text.len() + self.attribut.len() + self.weg.len() + self.neu.len()
    }

    /// Hat dieser Knoten ein neues Attribut? (fuer die Klon-Schranke)
    fn hat_attribut(&self, ord: usize) -> bool {
        self.attribut.keys().any(|(o, _)| *o == ord)
    }
}

impl Bauer<'_> {
    /// Die Attribute eines Knotens, mit Patch (W6).
    ///
    /// GEMESSEN, warum geklont wird und nicht gesucht: `attributes`
    /// ist eine `&HashMap` aus dem Parser, und ein Skript darf sie nicht
    /// aendern (der Baum wird weiter gebraucht — fuer die Kaskade und
    /// fuer den naechsten Bau). Geklont wird NUR, wenn der Patch fuer
    /// diesen Knoten etwas hat; bei einer Seite ohne Skript wird also
    /// nichts geklont.
    fn attribut_mit_patch(
        &self,
        ord: usize,
        attributes: &HashMap<String, String>,
    ) -> Option<HashMap<String, String>> {
        if !self.patch.hat_attribut(ord) {
            // Nichts zu tun: der Baum, den die Kaskade sieht, ist der mit
            // Patch. Also bleibt die Map unberuehrt — und das ist der
            // Fall, in dem KEIN Skript etwas getan hat.
            return None;
        }
        Some(attribute_mit_patch(attributes, ord, &self.patch))
    }
}

/// Der Attributname, unter dem `classList`-Anweisungen im Patch stehen
/// (W6, J9). `+x`, `-x`, `?x` — siehe [`klassen_wenden`].
pub const W6_KLASSEN_MARKER: &str = "\u{1}klasse";

/// Der Attributname, unter dem `style`-Anweisungen im Patch stehen (W6).
///
/// WARUM ALS ATTRIBUT und nicht als `style.name`: `style` ist ein
/// Attribut, und der Bauer liest Attribute. Ein zweiter Kanal
/// (`Style` direkt setzen) waere eine zweite Wahrheit.
pub const W6_STIL_MARKER: &str = "\u{1}stil";

/// Wendet eine `classList`-Anweisung auf die vorhandenen Klassen an.
///
/// # DIE DREI FORMEN, und wo sie entschieden werden
///
/// | Anweisung | Bedeutung | Beispiel |
/// |---|---|---|
/// | `+name` | die Klasse ist danach DA | `classList.add("x")` |
/// | `-name` | die Klasse ist danach WEG | `classList.remove("x")` |
/// | `?name` | umschalten | `classList.toggle("x")` |
///
/// M3/M4: `MAX_KLASSEN` (64) Klassen je Element und `MAX_KLASSEN_TEXT`
/// (1 KiB) Anweisungstext — beides aus der Eingabe, beides vor dem
/// Schreiben geprueft.
pub const MAX_KLASSEN: usize = 64;

/// Hoechste Laenge eines Anweisungstexts fuer `classList` (M4).
pub const MAX_KLASSEN_TEXT: usize = 1024;

/// Klassen nach einer Anweisung.
///
/// GEMESSEN, warum hier und nicht im Skript: der Skript-seitige Teil
/// kennt die vorhandenen Klassen nicht (die stehen im geparsten Baum),
/// und ein `toggle` ohne sie waere blind. Deshalb kommt die ANWEISUNG
/// hier an und die ENTSCHEIDUNG hier.
pub fn klassen_wenden(alt: &str, anweisung: &str) -> String {
    let mut vorhanden: Vec<String> = alt.split_whitespace().map(|k| k.to_string()).collect();
    if vorhanden.len() > MAX_KLASSEN {
        vorhanden.truncate(MAX_KLASSEN);
    }
    for teil in anweisung.split('\n').take(MAX_KLASSEN) {
        if teil.is_empty() || teil.len() > MAX_KLASSEN_TEXT {
            continue;
        }
        let (zeichen, name) = teil.split_at(1);
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_KLASSEN_TEXT {
            continue;
        }
        let da = vorhanden.iter().any(|k| k == name);
        match zeichen {
            "+" if !da && vorhanden.len() < MAX_KLASSEN => vorhanden.push(name.to_string()),
            "-" => vorhanden.retain(|k| k != name),
            "?" => {
                if da {
                    vorhanden.retain(|k| k != name);
                } else if vorhanden.len() < MAX_KLASSEN {
                    vorhanden.push(name.to_string());
                }
            }
            _ => {}
        }
    }
    vorhanden.join(" ")
}

/// Der Zustand waehrend des Aufbaus.
///
/// Kein `Rc`, kein `RefCell`, kein lebenslanger Verweis auf den
/// Dokumentbaum: der Bauer kennt nur seine eigene Tiefe und das
/// aktuelle `NodeId`, das er als Stapel fuehrt. Das haelt die
/// Zwischenzustaende klonbar und macht die Reihenfolge der
/// `push_child`-Aufrufe zum einzigen Ort, an dem der Baum wachsen kann.
struct Bauer<'a> {
    doc: Document,
    /// Knoten, an den als naechstes gehaengt wird.
    offen: Vec<NodeId>,
    links: HashMap<NodeId, String>,
    breite: u32,
    /// 05.10.2026: DIE ANSICHTSHOEHE, und der Grund, warum sie hier steht.
    ///
    /// **GEMESSENER FEHLER (vorher, `vw`/`vh`/`min-height`):** der Bauer hatte
    /// nur die **Breite**. `Kaskade::ansicht_hoehe` stand deshalb auf `0`
    /// („unbekannt"), und `kaskade.rs` rechnete in diesem Fall mit einer
    /// **geratenen** Hoehe von **600 px**. Gemessen an
    /// `team/testseiten/vh_messung.html` bei einer Ansicht von 1280x800:
    ///
    /// ```text
    /// min-height:100vh   ->  min_hoehe = Some(600.0)   statt Some(800.0)
    /// height:50vh         ->  hoehe_angabe = Some(300.0) statt Some(400.0)
    /// ```
    ///
    /// Das heisst: **jede** `vh`-Angabe einer echten Seite war 25 % zu klein,
    /// unabhaengig von der Fenstergroesse — `100vh` auf github.com,
    /// wikipedia.org und jedem Blog mit `min-height: 100vh` in der
    /// Kopfzeile.
    ///
    /// **WARUM DAS SO SCHLIMM WAR:** die geratene 600 ist **stillschweigend**
    /// falsch, und der Fehler waechst mit der Differenz zum echten Fenster.
    /// Bei 1280x800 sind es 200 px pro `100vh`; die Seite ist dadurch zu kurz
    /// (gemessen: `de.wikipedia.org` **10 248 px** statt **22 700 px** in
    /// Firefox — Faktor 0.5x).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum 0 weiterhin „unbekannt" bleibt und
    /// nicht etwa 800:** es gibt Aufrufer ohne Fenster (`aus_html` aus einem
    /// Test, `.narsil` aus der Kommandozeile). Die werden sich nicht ueber eine
    /// erfundene Hoehe wundern — sie rechnen nur nicht in `vh`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum der Wert am Bauer haengt und nicht an
    /// der Kaskade:** die Kaskade wird **einmal** je Seite gerechnet
    /// (`kaskade_einrichten`), und sie kennt die Ansicht ueber diesen Wert.
    /// Ein zweiter Weg (die Ansichtshoehe bei jedem `Berechnet`) waere eine
    /// zweite Wahrheit fuer dieselbe Zahl.
    ansicht_hoehe: u32,
    basis: &'a str,
    /// Absatz, der noch umbrochen werden muss (Text + Stil + Link).
    laufender: Option<Absatz>,
    inline_bild_eltern: std::collections::HashSet<NodeId>,
    /// Release 11.10.2026 (Agent CSS2): je offenem Block, ob seine erste
    /// Zeile noch aussteht (fuer `text-indent`).
    einzug_frei: Vec<bool>,
    /// Release 13.10.2026 (Agent CSS2): die offenen Zeilenklemmen
    /// (`line-clamp`), die innerste zuletzt (siehe [`Klemme`]).
    klemmen: Vec<Klemme>,
    /// Release 13.10.2026 (Agent CSS2): wie viele offene Bloecke
    /// `contenteditable` sind.
    bearbeitbar: usize,
    /// Die gerade offenen INLINE-Elemente, von aussen nach innen.
    ///
    /// GEMESSENER FEHLER 30.09.2026 (Koordinationsbefund, Bildschirmfoto):
    /// `<p>Das ist ein <b>ganz normaler</b> Absatz mit einem
    /// <a>Link</a>. …` wurde als FUENF untereinanderstehende Zeilen
    /// gezeichnet. Grund: `element()` behandelte `<b>` wie einen Container
    /// und rief am Ende `absatz_schliessen()` auf — damit wurde der
    /// laufende Absatz bei JEDEM Inline-Element beendet, und `text()`
    /// rief wiederum `absatz_starten()`, das den vorherigen Absatz
    /// ebenfalls beendet. Fuenf Teilstuecke, fuenf Absaetze, fuenf Zeilen.
    ///
    /// Der Stapel ersetzt den Container: ein Inline-Element erzeugt KEINEN
    /// Knoten, es aendert nur den Stil des nachfolgenden Textes.
    marken: Vec<Marke>,
    /// Die gesammelten Skripte, Stile und Ereignisse (siehe `Seite`).
    skripte: Vec<Skript>,
    /// W7C A4: der `<title>`-Text (noch ungekuerzt, M4 erst beim Bau-Ende).
    titel: String,
    /// W7C A4: die aufgeloeste Favicon-Adresse.
    favicon: Option<String>,
    ereignisse: Vec<Ereignis>,
    stile: Vec<StyleBlock>,
    /// A-ADBLOCK: das kosmetische Blatt fuer diesen Host.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein eigenes Feld und kein
    /// Eintrag in `stile`: `stile` ist das, was die SEITE geschrieben
    /// hat, und ein Werbefilter ist nicht das, was die Seite geschrieben
    /// hat. Ausserdem braucht `stile_aufnahme` die Attribute des
    /// `<style>`-Knotens — ein Filter hat keine. Und die Reihenfolge
    /// ist zugesichert: das kosmetische Blatt kommt **nach** allen
    /// Seiten-Blaettern, damit es das letzte Wort hat.
    ///
    /// `None` = kein Filter (der Normalfall ohne Filter). Dann ist der
    /// Aufbau unveraendert und `kaskade_einrichten` sieht nichts.
    kosmetik: Option<narsil_css::Stylesheet>,
    /// I3: die `<img>`-Knoten mit ihren Angaben (siehe `Seite::bilder`).
    bilder: Vec<BildKnoten>,
    hintergrundbilder: HashMap<narsil_dom::NodeId, (String, narsil_css::BildGeo)>,
    maskenbilder: HashMap<narsil_dom::NodeId, Vec<MaskenEbene>>,
    /// Release 15.10.2026 (Agent F): masks of inline elements without a box
    /// of their own, by mask key; their text nodes carry them (`fertig`).
    inline_masks: Vec<(std::sync::Arc<str>, Vec<MaskenEbene>)>,
    /// B12: die von `<picture>` gewaehlte Quelle, wenn eine da ist.
    ///
    /// GEMESSENER Grund fuer ein Feld und kein
    /// Zusaetzlicher Knoten: der `img`-Zweig baut aus einer `<picture>`
    /// EINEN Bildknoten. Liefe der `img`-Zweig ohne diese Angabe, kaeme
    /// sein `src` zum Zug — und damit das Bild, das der Autor als
    /// Rueckfall gemeint hat, waehrend sein WebP unbenutzt bliebe.
    picture_quelle: Option<String>,
    /// W6, J9: die `id`-Tabelle (siehe `Seite::ids`).
    ids: HashMap<String, usize>,
    /// Die `id`, die das **gerade** gebaute Element traegt (02.10. 08:15).
    ///
    /// GEMESSENE EINORDNUNG, warum das nicht `ids` ist: dort steht die
    /// Ordnungszahl, und die kann mehrere Knoten bezeichnen. Hier steht der
    /// Name, und der Knoten entsteht gleich danach — fuer genau diesen
    /// Knoten wird er in `id_zu_knoten` eingetragen.
    ///
    /// GEMESSENE EINORDNUNG, warum kein Stapel noetig ist: ein Knoten entsteht
    /// **vor** seinen Kindern (`push_child`, dann `offen.push`, dann die
    /// Kinder). Der Wert ist also beim Erzeugen des Knotens genau die Id
    /// dieses Elements und nicht die eines Kindes.
    aktuelle_id: Option<String>,
    /// Id -> Knoten, wandert am Bauende in [`Seite::id_zu_knoten`]
    /// (02.10. 08:20, siehe dort die Messung).
    id_zu_knoten: HashMap<String, NodeId>,
    /// 02.10. 18:35 (HTML): `<label for="X">` — die **Textknoten** des Labels
    /// und der Name `X`, aufgeloest am Bauende (der Feldknoten kann spaeter
    /// kommen als das Label). Siehe `Seite::label_zu_knoten`.
    label_zu_knoten: HashMap<NodeId, NodeId>,
    /// 02.10. 21:50 (HTML): die aufgeloesten Regeln fuer den Klickpfad
    /// (siehe `Seite::label_zu_text`); gefuellt in [`Self::fertig`].
    label_zu_text: HashMap<(narsil_dom::NodeId, String), narsil_dom::NodeId>,
    /// Die Regeln „(Bereichsknoten, Text) -> Feld" (siehe `LabelSchluessel`).
    label_regeln: HashMap<LabelSchluessel, LabelRegel>,
    /// Knoten, die zu einer Regel gehoeren, deren Feld **noch nicht** bekannt
    /// war (siehe `label_regel_anwenden`).
    label_offen_knoten: Vec<(narsil_dom::NodeId, LabelSchluessel)>,
    /// Der Stapel der offenen `<label>`-Elemente (siehe `label_zu_knoten`).
    label_stapel: Vec<LabelRahmen>,
    /// `<label for="X">`, noch unaufgeloest — der Name steht drin, nicht der
    /// Knoten: der Feldknoten ist beim Bauen des Labels noch nicht bekannt.
    label_for_unfertig: Vec<(NodeId, String)>,
    /// W6, J9: der CSS-Baum fuer die Selektorsuche (siehe `Seite::css_baum`).
    ///
    /// GEMESSEN, warum eine Kopie: `baum_bauen` liefert eine Liste, die
    /// beim Kaskaden-Bau verbraucht wird. Die Kopie ist `Vec<CssKnoten>`
    /// mit Tag/id/Klassen je Knoten — bei 2000 Knoten rund 100 KiB, und
    /// eine Seite wird hundertmal neu gebaut (W6 spielt Befehle auf der
    /// Quelle ab). Die Kopie ist billiger als ein zweiter Durchlauf durch
    /// den HTML-Baum.
    css_baum: crate::kaskade::CssBaum,
    /// W6, J9: die Bruecke CSS-Index -> Vorlauf-Nummer
    /// (siehe `Seite::css_zu_dom`).
    css_zu_dom: HashMap<u32, usize>,
    /// W6: die naechste Vorlauf-Nummer (siehe `Patch`).
    naechste_ordinal: usize,
    /// Release 12.10.2026: im flachen Baum (Shadow DOM) die feste Nummer je
    /// Knoten (Adresse -> Ord, siehe `schatten::zusammensetzen`); leer
    /// ohne Shadow Root.
    ord_karte: HashMap<usize, usize>,
    /// W6: die Vorlauf-Nummer des Knotens, der gerade gebaut wird.
    aktualer_ordinal: usize,
    /// W6: die Aenderungen des Skripts (siehe `Patch`).
    patch: Patch,
    /// W6: DOM-Knotennummer -> Vorlauf-Nummer.
    ///
    /// GEMESSEN, warum es diese Bruecke in die Gegenrichtung braucht
    /// (30.09.2026, 12:55): der Klick liefert aus dem Layout-Baum eine
    /// `NodeId`, die Horcher sind aber mit VORLAUF-NUMMERN
    /// angemeldet (`getElementById` gab die Vorlauf-Nummer). Ohne die
    /// Bruecke fand der Klick den Handler nie — und der Test sah
    /// „Handler lief nicht", obwohl er da war.
    ///
    /// # WARUM NUR DER ERSTE NEUE KNOTEN
    ///
    /// In `kind()` wird die Laenge des Dokuments VOR und NACH dem Aufruf
    /// gemessen. Der ERSTE dabei entstandene Knoten gehoert zu diesem
    /// `HtmlNode` (die Kindknoten entstehen in tieferen Aufrufen und
    /// bekommen ihre eigene Nummer dort). Das ist die einzige Stelle,
    /// an der etwas festgeschrieben werden muss — und nicht vier
    /// (Element-Box, Absatz, Text, Platzhalter).
    dom_zu_ord: HashMap<u32, usize>,
    /// 03.10. 12:55: DIE FELDER, mit der Nummer DIESES Bauers.
    ///
    /// GEMESSENER Befund (12:45): `narsil` zaehlte die Felder selbst und kam
    /// zu anderen Nummern — auf einer Seite mit laufendem Skript fand der
    /// Klick den Absender nicht. Siehe [`FeldOrd`] und [`FormularOrd`].
    ///
    /// M3: `MAX_FORMULARFELDER` (512, aus `hteile.rs`) und
    /// `MAX_FORMULARE` (64) begrenzen beide das Wachstum.
    felder: Vec<FeldOrd>,
    /// Die Formulare dieses Bauers, in Dokumentreihenfolge.
    formular_ords: Vec<FormularOrd>,
    /// Der STAPEL der offenen Formulare (Index in `formular_ords`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Stapel und keine Zahl: verschachtelte
    /// Formulare sind ungueltiges HTML, aber der Parser nimmt sie an, und ein
    /// Feld in einem inneren Formular gehoert zum inneren. Mit einer Zahl
    /// waere das ein Zaehler, der zurueckgesetzt werden muss — mit einem
    /// Stapel ist es `push` und `pop`.
    formular_stapel: Vec<usize>,
    /// W7C R3: der Plan, der am Ende in `Seite::layout_plan` wandert.
    /// GEMESSEN, warum er im BAUER steht und nicht in `Seite`: er wird
    /// beim Bauen gefuellt, und `merken()` (das die Seite baut) kann ihn
    /// dann verschieben statt kopieren.
    layout_plan: narsil_layout::LayoutPlan,
    /// W7C R3 (02.10., Rolle LAYOUT): DER KASTEN DIESES ELEMENTS — der
    /// Knoten, in den der Layout-Plan gehoert.
    ///
    /// GEMESSENER FEHLER (04:10, dreimal falsch gerechnet): ein Element
    /// erzeugt seinen Kasten an ZWEI Stellen — ein Container sofort
    /// (`element`) und ein Absatz am Ende (`absatz_schliessen`). Der erste
    /// Knoten des Aufrufs traf den Container, der letzte den TEXTKNOTEN
    /// des letzten Kindes, ein Notizfeld den Kasten des letzten
    /// Nachfahren. Keine der drei Varianten ist die richtige.
    ///
    /// Die Loesung ist kein vierter Versuch, sondern das gesicherte
    /// Merken: `element()` nimmt den Wert beim Eintritt weg und stellt
    /// ihn beim Ausgang wieder her. Damit traegt das Feld nach dem Bau der
    /// Kinder den Kasten DIESES Elements.
    eigener_kasten: Option<narsil_dom::NodeId>,
    /// W7C R3: DER STAPEL der Layout-Stile.
    ///
    /// GEMESSEN, warum ein Stapel und kein Feld: `stil_von_kaskade`
    /// rechnet das `Berechnet` fuer ein Element, und die Kind-Knoten
    /// werden DANN gebaut (`element()` ruft `kind()`) — ein Feld wuerde
    /// vom Kind ueberschrieben, und der Abschluss von `kind()` wuerde
    /// den Stil des Kindes unter das des Eltern legen. Der Stapel
    /// haelt beide, und wer oben liegt, gehoert dem innersten Aufruf.
    ///
    /// `None` = fuer diesen Knoten wurde nichts gerechnet (keine
    /// Kaskade, oder der Knoten steht nicht im CSS-Baum).
    ///
    /// W7C R3 (02.10., Rolle LAYOUT): der Eintrag traegt seitdem BEIDES —
    /// den Stil UND den Kasten, zu dem er gehoert. GEMESSENER FEHLER:
    /// `eigener_kasten` war ein flaches Feld, und die Kinder werden
    /// VOR dem Abschluss des Eltern gebaut — jedes Kind hat den Kasten
    /// seines Eltern ueberschrieben. Drei Varianten, den richtigen
    /// Kasten zu raten, waren alle falsch (siehe
    /// `team/UEBERGABE_HTML_R3R4.txt`); die Loesung ist kein Raten,
    /// sondern ein Stapel, genau wie beim Stil.
    ///
    /// Der Stil bleibt ein eigenes `Option`, weil ein Inline-Element mit
    /// leerem Inhalt einen Stil, aber keinen Kasten hat.
    plan_stapel: Vec<PlanEintrag>,
    /// W7C R3 (Rolle PROBE, 04:50): die Kasten der offenen Elemente, in
    /// derselben Reihenfolge wie `plan_stapel`. Der Stil bleibt ein
    /// eigenes `Option`, weil ein Inline-Element mit leerem Inhalt einen
    /// Stil, aber keinen Kasten hat.
    kasten_stapel: Vec<Option<narsil_dom::NodeId>>,
    /// 03.10. 21:15: DER `cellpadding` DER UMGEBENDEN TABELLE (Pixel).
    ///
    /// `cellpadding` ist ein praesentatives Attribut (HTML 4, 17.12.1: es
    /// entspricht der CSS-Eigenschaft `padding` an den Zellen) und steht am
    /// `<table>` — die Zelle selbst kann es nicht wissen. GEMESSENE
    /// BEGRUENDUNG fuer einen STAPEL und kein Feld: `element()` hat elf
    /// `return`-Zweige (siehe den Kommentar an der Kaskadenstelle, 20:25),
    /// und ein Push ohne das passende Pop waere an der ZELLE des naechsten
    /// Geschwisters sichtbar — nicht an der Tabelle, die es ausgeloest hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Vorgabewert 0 und nicht 1 ist:
    /// Firefox gibt einer Zelle im UA-Stylesheet 1 px (`padding`), und das
    /// waere die ehrlichere Vorgabe — aber es aendert das Bild JEDER Tabelle
    /// auf JEDER Seite, und die Geometrie laesst sich gerade nicht messen
    /// (der Build-Fenster-Durchgang, 21:20). Ohne `cellpadding` bleibt es bei
    /// dem, was heute gemessen ist: keine Polsterung.
    cellpadding_stapel: Vec<u32>,
    /// 05.10. 22:15: DER `cellspacing` DER UMGEBENDEN TABELLE (Pixel).
    ///
    /// `cellspacing` ist ein praesentatives Attribut (HTML 4, 17.12.2: es
    /// entspricht der CSS-Eigenschaft `border-spacing` an der Tabelle) und
    /// steht wie `cellpadding` am `<table>`. GEMESSENER BEFUND (22:05):
    /// `cellspacing="4"` aendert bei uns **nichts** — der Abstand zwischen
    /// zwei Zellen ist mit und ohne die Angabe 19,2 px.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein eigener Stapel und nicht ein
    /// zweites Feld an `cellpadding_stapel`: `cellspacing` ist ein eigener
    /// Stapel mit derselben Pflicht (Push am `<table>`, Pop direkt danach),
    /// und beide Werte kommen an **verschiedenen** Stellen an — die
    /// Polsterung an der Zelle, der Abstand als Rand der Zelle. Ein
    /// gemeinsamer Stapel muesste beides an jeder Zelle auslesen und
    /// entscheiden, welches davon die Zelle gerade braucht.
    cellspacing_stapel: Vec<u32>,
    /// W6: die Vorlauf-Nummer des Elements, das den LAUFENDEN Absatz
    /// gestartet hat.
    ///
    /// GEMESSEN, warum es ein eigenes Feld ist: Absaetze werden erst
    /// beim SCHLIESSEN zu Knoten (`absatz_schliessen`), also lange nach
    /// `kind()`. Die Zeilen-Boxen und ihre Textkinder gehoeren aber zum
    /// `<p>` — und ohne diese Nummer fand ein Klick auf den Text keinen
    /// Handler, den genau dieses `<p>` angemeldet hat (Test
    /// `ein_klick_ruft_den_handler`, 13:00).
    laufender_ord: Option<usize>,
    /// KOMPRESS 04.10.2026 (Nachweis-Anordnung der Leitung): wie viele
    /// `<audio>`-Elemente diese Seite schon bekommen hat — **1** fuer das
    /// erste.
    ///
    /// GEMESSENER GRUND, und es ist ein gemessener: `aktualer_ordinal`
    /// zaehlt **alle** Elemente (Textknoten eingeschlossen), also ist es
    /// aus dem `HtmlNode`-Baum **nicht** rekonstruierbar — dort steht kein
    /// Ordinal. Und `narsil_dom` speichert **keine** Attribute (GEMESSEN:
    /// `NodeKind` kennt nur `Page | Container | Text`), also gibt es auch
    /// keinen Weg ueber eine `id`.
    ///
    /// Also steht die Nummer **im Text** des Knopfes: „[Knopf: ▶ 1]". Das
    /// ist **sichtbar** (also im Bild nachweisbar) und **eindeutig**.
    audio_zaehler: usize,
    /// VIDEO, 04.10.2026: dasselbe wie `audio_zaehler`, fuer `<video>`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein eigener Zaehler und nicht ein
    /// gemeinsamer: ein `<audio>` und ein `<video>` auf derselben Seite
    /// bekommen sonst dieselbe Nummer, und die Nummer steht im Text des
    /// Knopfes — sie muss also fuer den Leser des Bildes eindeutig sein.
    video_zaehler: usize,

    /// W6E: die Vorlauf-Nummern der Elemente, die GERADE gebaut werden
    /// (oben das innerste, unten `html`).
    ///
    /// GEMESSENER FEHLER, warum es den Stapel braucht (30.09.2026, 16:55,
    /// Koordinationsbefund „21 Klicks auf den Knopf, `klick()` immer
    /// `false"): `absatz_starten` nahm `aktualer_ordinal`, und das ist
    /// die Nummer des LETZTEN `kind()`-Aufrufs — bei einem Textknoten also
    /// die Nummer des TEXTKNOTENS, nicht die des Knopfs. Gemessen an
    /// `zaehler.html`: der Absatz „Klick mich" bekam die Nummer 5 (sein
    /// Textknoten), `getElementById("k")` lieferte 4 (der Knopf). Der
    /// Handler haengte an 4, der Klick traf 5 — und fand nichts.
    ///
    /// M3: `MAX_BAU_TIEFE` (64) Eintraege, weil `kind()` genauso tief
    /// recurt; ein `push` ohne `pop` waere ein Leak ueber die Seite.
    element_stapel: Vec<usize>,
    /// Der Stil der **offenen Tabellenzelle** (SCHRIFT, 04.10.2026).
    ///
    /// GEMESSENER BEFUND, wofuer das da ist: `<th>` ist ein **Kasten**
    /// (`ist_absatz` ist false, lib.rs:9818 ff.), die Kaskade rechnet
    /// **700** (`autor=700 mit_ua=700`), `kind()` traegt 700 in
    /// `kommender_stil` — und der Knoten bekam **400**. Die Luecke war
    /// eine Zeile in `text()`: der Absatz, der in der Zelle startet,
    /// bekam das feste Vorgabe-Literal (16 px, `FARBE_TEXT`, 400) statt
    /// des Kastenstils. GEMESSEN, dass es am Weg liegt und nicht am
    /// Selektor: dieselbe Regel (`th { font-weight: bold }`) im
    /// **Autoren**-CSS der Testseite bleibt ebenfalls 400.
    ///
    /// GEMESSEN, warum hier ein **Stil** und nicht nur ein Flag „ist
    /// Zelle": die erste Fassung trug nur das Flag und holte den Stil
    /// ueber `element_stapel.last()` — GEMESSEN `doc.get(NodeId(11))`
    /// liefert **None**, weil der Stapel **Ordinale** traegt
    /// (`ORD_BASIS`), keine `NodeId`s. Es gibt keine Rueckrichtung von
    /// Ordinal zu Knoten (`dom_zu_ord` geht nur von Knoten zu Ordinal),
    /// also muss der Stil mit.
    ///
    /// GEMESSEN, warum die Kapazitaet 1 reicht: eine Tabellenzelle
    /// enthaelt keine weitere (`<td><td>` ist kein HTML), und `td`/`th`
    /// sind keine Inline-Elemente — es gibt also nie zwei offene
    /// Zellen. Der Typ ist trotzdem ein `Vec`, weil `pop()` dann ohne
    /// Sonderfall funktioniert.
    ///
    /// GEMESSEN, warum der Push nicht in `kind()` steht (die erste
    /// Fassung): dort steht kein `pop` (der Stapel waechst also, siehe
    /// den Kommentar am Ende von `kind()`), und `element_innen` hat
    /// mehrere `return`-Zweige. Der Push steht deshalb im
    /// **Container-Zweig** unmittelbar vor der Kinderschleife (Zeile
    /// 8046) und der Pop unmittelbar danach — GEMESSEN: zwischen beiden
    /// steht kein `return`, die Paarung ist also sicher (REGEL 11).
    offene_stile: Vec<Style>,
    /// Release 08.10.2026: die VERERBBAREN Textangaben jedes offenen
    /// Kastens (Farbe, Schrift, Ausrichtung, Zeilenhoehe) — fuer Text, der
    /// direkt in einem Kasten steht (Flex, Raster, Knopf, inline-block).
    /// GEMESSEN vorher: solcher Text bekam die Vorgabe (schwarz, 16 px) —
    /// `<nav style="display:flex;color:#fff">` zeigte schwarze Links.
    kasten_textstile: Vec<Style>,
    /// 05.10.2026: HAT `<html>` DIE CANVAS-FARBE BEREITS GESETZT?
    ///
    /// **WOZU (gemessen, 05.10.2026, `team/testseiten/canvas_hintergrund.html`):**
    /// CSS 2.1, 2.3.2 / CSS Backgrounds 3, 2.3.2 — „the background of the root
    /// element is propagated to the canvas; **if the root has a transparent
    /// background, the body's is used**". Das heisst: **`<html>` hat Vorrang vor
    /// `<body>`**, und das ist eine *Ordnung*, keine Reihenfolge.
    ///
    /// **GEMESSENER FEHLER, den dieses Feld hebt:** `<html>` und `<body>` stehen
    /// in Dokumentreihenfolge `<html>` **vor** `<body>`. Ohne dieses Feld
    /// gewinnt damit der **letzte** — `<body>` — und einundderselbe Rumpf
    /// ueberschreibt das Wurzel-Element. Gemessen an
    /// `team/testseiten/canvas_body_zuerst.html` (`html` rot, `body` blau):
    /// die Canvas wurde **blau**. Nach CSS 2.1, 2.3.2 muss sie **rot** sein.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum ein Feld des Bauers und nicht ein
    /// Vergleich mit dem Wurzelstil:** der Wurzelstil traegt am Anfang
    /// `FARBE_SEITE` (weiss, `wurzel()`), und ein Rumpf mit
    /// `background: white` ist damit **nicht** von „der Rumpf hat nichts
    /// gesetzt" zu unterscheiden. Genau dieser Fall hat am 05.10. 23:55
    /// schon einmal eine Seite getoetet (siehe `element_innen`): Der Rumpf
    /// galt als unveraendert, und ein `return` in `element_innen` hat die
    /// Kinder des Rumpfes nie besucht. Ein **Bool** kann nicht mit weiss
    /// verwechselt werden.
    canvas_von_html: bool,
    /// Release 14.10.2026 (Agent CSS2): `<html>` has `contain: paint`, so the
    /// body background no longer propagates to the canvas (CSS Containment 2
    /// §3.3).
    canvas_blocked: bool,
    /// Release 14.10.2026 (Agent CSS2): `<html>` has a background image of
    /// its own, so the body's image is not propagated.
    root_has_image: bool,
    /// W6E: war der Knoten, den `kind()` gerade baut, ein TEXTknoten? Dann
    /// gehoert er nicht auf [`Self::element_stapel`].
    ///
    /// GEMESSEN, warum ein Feld und nicht ein zweites `matches!` am
    /// Austritt: `HtmlNode` steht an zwei Stellen 900 Zeilen auseinander,
    /// und die Bedingung muss an BEIDEN gleich lauten. Ein `matches!` an
    /// beiden Stellen ist zwei Entscheidungen (W5G).
    element_ist_text: bool,
    /// W6G: Vorlauf-Nummer -> der Einzug, der INNERHALB dieses Elements
    /// galt (also inklusive seines eigenen Randes).
    ///
    /// GEMESSEN, warum das noetig ist: der Listen-Einzug ist Bauzustand
    /// (`Bauer::einzug`), und ein spaeter angehaengter Knoten (W6F)
    /// bekam ihn nicht. Siehe die Stelle in `element()`.
    ///
    /// M4/M6: die Map waechst mit der Knotenzahl; `MAX_KNOTEN` (4096)
    /// begrenzt sie, und pro Knoten steht eine `f32`.
    einzug_von_ord: std::collections::HashMap<usize, f32>,
    /// Adresse des `HtmlNode` -> CSS-Index (`CssKnoten::adresse`).
    ///
    /// GEMESSEN, warum eine Map und kein Zugriff auf den `CssBaum`: der
    /// Bauer haelt den Baum nicht, sondern nur die `Kaskade`, und die
    /// entsteht aus ihm. `kind()` braucht aber den CSS-Index, um die
    /// Bruecke zu fuellen — und dort steht der `HtmlNode`, dessen
    /// Adresse der Schluessel ist. M3: die Map hat hoechstens so viele
    /// Eintraege wie der Baum (der durch die Knotenzahl der Seite
    /// begrenzt ist).
    css_adresse: HashMap<usize, u32>,
    /// I2: der CSS-Baum, die Blaetter und der Stapel der
    /// `Berechnet`-Ergebnisse (einer je Ebene, fuer die Vererbung).
    kaskade: Option<Kaskade>,
    /// W5E (2): die Schrift, MIT der gemessen wird — `None` heisst die
    /// eingebaute Bitmap.
    ///
    /// GEMESSENER BEFUND 30.09.2026, 09:20: gemessen wurde immer mit
    /// `narsil_text::measure(&BitmapFont::builtin(), …)`, und gezeichnet
    /// wurde (in `narsil-raster`) mit der Systemschrift. Zwei verschiedene
    /// Einheiten: der Rasterizer zeichnete TrueType-Buchstaben im
    /// Abstand der Bitmap (6/7 der Schriftgroesse je Zeichen), also
    /// sichtbar gesperrt — "W i l l k o m m e n".
    ///
    /// Wer hier die Systemschrift einsetzt, misst mit ihr, und der
    /// Rasterizer nimmt denselben Vorschub. Die Kette ist: Browser
    /// findet die Schrift → `seite_aus_text_mit_schrift` → Umbruch mit
    /// `measure_tt` → Layout mit `TtMeasurer` → Rasterizer mit hmtx.
    ///
    /// W5G: das ist jetzt DIE Messung (in `narsil-layout`), nicht mehr ein
    /// optionaler Zeigertyp. Vorher stand hier `Option<TrueTypeFont>` und
    /// `breite_von` entschied selbst, welches `measure` es aufruft — zwei
    /// Stellen, an denen die Schrift entschieden werden konnten.
    messung: narsil_layout::Messung,
    /// I2: der Stil, den die Kaskade fuer das ELEMENT berechnet hat, das
    /// als naechstes besucht wird.
    ///
    /// WARUM EIN FELD und kein Parameter: `element()` hat elf
    /// `return`-Zweige (unsichtbar, script, img, input, …) und wird aus
    /// `kind()` und — bei `<video>`, `<embed>`, `<option>` — noch aus
    /// sich selbst heraus gerufen. Ein zusaetzlicher Parameter muesste an
    /// ALLEN diesen Stellen mitgeschleppt werden; ein Feld wird an
    /// genau einer gefuellt und an einer geleert.
    kommender_stil: Option<Style>,
    /// PSEUDOELEMENTE (06.10.2026): der fertig gerechnete `::before`-
    /// bzw. `::after`-Kasten **dieses** Elements.
    ///
    /// GEMESSENER GRUND: `.skin-vector-2022 .mw-logo::before` traegt die
    /// Wikipedia-Kugel (background-image, 50x50), `::after` die Wortmarke.
    /// Der Parser liess `::` fallen, die Regeln fielen mit — die Kugel
    /// fehlte. `stil_von_kaskade` fuellt die zwei Felder, `element()`
    /// leert sie: `::before` VOR den Kindern, `::after` danach. Damit die
    /// Reihenfolge stimmt, werden beide in `element()` GELESEN, bevor
    /// `element_innen` die Kinder baut (die Kinder setzen die Felder neu).
    pseudo_vorher: Option<kaskade::PseudoKasten>,
    pseudo_nachher: Option<kaskade::PseudoKasten>,
    /// Release 10.10.2026: Farbe und Deckkraft von `::placeholder` DIESES
    /// Feldes (siehe `kaskade::platzhalter_farbe`).
    platzhalter_farbe: Option<(Color, f32)>,
    /// PSEUDOELEMENTE (06.10.2026): der `::before`-Kasten, der auf den
    /// EIGENEN Kasten des Elements wartet — als STAPEL, ein Platz je
    /// Element.
    ///
    /// GEMESSENER GRUND fuer das Warten: `.mw-logo` ist `display:flex`
    /// und bekommt darum in `element_innen` einen eigenen Container
    /// (Zeile ~8500). Stand die Kugel vorher am ELTERN, lag sie
    /// ausserhalb des Kastens (GEMESSEN: NodeId(70) bei x=44, der
    /// Kasten des `<a>` bei x=90). Wer einen eigenen Kasten baut, nimmt
    /// sie als ERSTES KIND — sonst (kein eigener Kasten) faellt sie am
    /// Ende von `element()` an den Eltern zurueck.
    ///
    /// GEMESSENER GRUND fuer den STAPEL und kein Einzelfeld: ein Element
    /// ohne eigenen Kasten baut seine Kinder, waehrend sein `::before`
    /// noch wartet — und JEDES Kind setzt `element()` neu an. Ein
    /// Einzelfeld wuerde dabei ueberschrieben, der Kasten des Eltern
    /// fiele weg (genau die Fehlerklasse, die `element_ist_text` im
    /// Kommentar oben beschreibt).
    pseudo_before_stapel: Vec<Option<kaskade::PseudoKasten>>,
    /// Release 08.10.2026: `::before`-TEXT, der vor den ersten echten Text
    /// des Elements gehoert (siehe `text`); am Elementende wird der Rest
    /// gesetzt (Element ohne eigenen Text, z. B. ein Trenner-`<span>`).
    pseudo_text_wartend: Vec<WartenderText>,
    /// Release 10.10.2026: die wartenden `::first-letter`-Auftraege der
    /// offenen Blockbehaelter (aussen zuerst) — der naechste echte Text
    /// loest sie ein (siehe `text_ohne_pre`), das Elementende verwirft sie.
    erster_buchstabe: Vec<ErsterBuchstabeAuftrag>,
    /// Release 10.10.2026: in einem Float oder absolut gesetzten Kasten
    /// geparkte Auftraege der Vorfahren (Tiefe des Kastens, Auftraege) — ein
    /// solcher Kasten steht nicht in der ersten Zeile seines Vorfahren.
    erster_buchstabe_geparkt: Vec<(usize, Vec<ErsterBuchstabeAuftrag>)>,
    /// Release 15.10.2026 (Agent CSS2): the waiting `::first-line` orders of
    /// the open block containers (outer first) — the next paragraph with
    /// text takes them (its first line is their first formatted line), the
    /// element end drops them; parked like the `::first-letter` ones.
    erste_zeile: Vec<ErsterBuchstabeAuftrag>,
    erste_zeile_geparkt: Vec<(usize, Vec<ErsterBuchstabeAuftrag>)>,
    /// Release 15.10.2026 (Agent CSS2): while a block pseudo-element writes
    /// its text: its `text-wrap: balance`.
    pseudo_balance: Option<bool>,
    /// Release 11.10.2026: die offenen `lang`-Angaben (Bautiefe, ist das
    /// Schriftsystem Chinesisch/Japanisch) — die innerste gilt (vererbt).
    sprachen: Vec<(usize, bool)>,
    /// Release 11.10.2026: die CSS-Zaehler (CSS Lists 3 §4) — Name, Wert,
    /// Bautiefe des erzeugenden Elements. Ein Zaehler gilt fuer sein
    /// Element, dessen Nachkommen und die folgenden Geschwister; er endet
    /// mit dem Eltern.
    zaehler: Vec<(String, i64, usize)>,
    /// Release 11.10.2026: die offenen `quotes`-Angaben (Bautiefe, Paare);
    /// die innerste gilt (geerbt).
    zitate: Vec<(usize, Vec<(String, String)>)>,
    /// Release 11.10.2026: die Verschachtelung der Anfuehrungszeichen (CSS
    /// Content 3 §2.3) — in Dokumentreihenfolge ueber die ganze Seite.
    zitat_tiefe: usize,
    /// Release 11.10.2026: der `::after`-Text eines Blocks wartet auf das
    /// Ende der Kinder seines Elements (Bautiefe, Kasten).
    nach_wartend: Vec<(usize, kaskade::PseudoKasten)>,
    /// Release 08.10.2026: die offenen Absatz-ELEMENTE ohne eigenen Kasten
    /// (`p`, `li`, `h1` ...). Taucht in ihnen ein inline-artiger Kasten auf
    /// (`inline-block`, `inline-flex`, `inline-grid`), wird der Absatz
    /// nachtraeglich ein Inline-Fluss-Kasten (siehe `absatz_zu_inline_fluss`).
    absatz_rahmen: Vec<AbsatzRahmen>,
    /// Release 11.10.2026: die offenen BILDABSAETZE (Absatz-Element mit
    /// ersetztem Kind, eigener Kasten, kein `absatz_rahmen`) — (Zahl der
    /// `kasten_textstile`, Zahl der `absatz_rahmen` beim Oeffnen, Textstil).
    /// Text hinter dem Bild erbt diesen Stil (siehe `block_textstil`).
    bild_absatz_stile: Vec<(usize, usize, Style)>,
    /// Release 07.10.2026: der Zähler je offener Liste (`ul`/`ol`/`menu`)
    /// und seine Schrittweite (`-1` bei `<ol reversed>`). Ohne ihn bekam
    /// jedes `<li>` dieselbe Marke „*" — auch in `<ol>`.
    listen_zaehler: Vec<(i64, i64)>,
    /// Release 08.10.2026: die Vorlauf-Nummern der schon gebauten neuen
    /// Knoten (jeder genau einmal, egal über welchen Haken).
    neu_gebaut: std::collections::HashSet<usize>,
    /// Release 08.10.2026: Ziele von `<use href="#id">` im ganzen Dokument.
    svg_definitionen: HashMap<String, &'a HtmlNode>,
    /// Release 07.10.2026: die geschriebenen Absätze für den Nachumbruch.
    absaetze: Vec<AbsatzSatz>,
    /// HTML_1 (04.10.2026): der RAHMEN, den das **UA-Blatt** fuer ein
    /// Formularfeld vorgibt — nur die vier Tags, die ueber
    /// `feld_von_kaskade` laufen.
    ///
    /// GEMESSENER BEFUND, der dieses Feld noetig macht (der Fehler, den es
    /// behebt, steht ausfuehrlich bei `feld_von_kaskade`): `k.werte.last()`
    /// traegt `autor` — die **Autorenebene ohne UA-Blatt**. Das UA-Blatt
    /// (`narsil-css/src/ua.rs:216`, `input, textarea { border: 1px solid
    /// #8f8f9d }`) steht nur in `mit_ua`, und `mit_ua` war eine LOKALE
    /// Variable in `stil_von_kaskade`. Damit kam der Vorgaberahmen an
    /// **keinem** Formularfeld an — GEMESSEN `padding = 0` an allen vier.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine kleine `Copy`-Struktur ist und
    /// nicht die ganze `Berechnet`: `Berechnet` traegt `Vec`-Felder
    /// (`schluessel`, `raster_spalten`, …), und sie fuer **jedes** Element
    /// zu klonen waere ein Kostenzuschlag auf jeder Seite. Kopiert wird
    /// nur bei den vier Formular-Tags, und dort nur 4 Breiten, 1 Stil und
    /// 1 Farbe.
    feld_rahmen_ua: Option<FeldRahmen>,
    /// Wie weit der Text durch die VORFAHREN eingerueckt ist (M4/M3).
    ///
    /// GEMESSENER FEHLER 30.09.2026, 06:24: ein `<pre>` in einem
    /// `<body>` mit UA-Margin ergab eine Zeile bis 484 px bei 480 px
    /// Seitenbreite. Der Absatz selbst hatte keinen Einzug — der kam
    /// aus den Boxen DARUEBER, und der Umbruch kannte sie nicht.
    ///
    /// Es ist eine Summe, keine Liste: der Einzug ist additiv, und fuer
    /// die Breite zaehlt nur die Summe. Beim Betreten einer Box wird er
    /// um deren linken/rechten Margin UND Fuellung erhoeht, beim
    /// Verlassen wieder verringert — an den zwei Stellen, an denen
    /// `offen.push` und `offen.pop` stehen.
    einzug: f32,
    textbreite: f32,
    textbreite_von_ord: HashMap<usize, f32>,
    /// Wie tief wir gerade im Baum sind (M5).
    ///
    /// GEMESSENER FEHLER 30.09.2026, gefunden von `tests/bomben.rs`:
    /// `<div>` 600-mal hintereinander liess `aus_html` mit einem
    /// STACKUEBERLAUF abbrechen. Der Parser haelt 512 Ebenen
    /// (`MAX_TIEFE`), und 512 Ebenen reichen in dieser Crate NICHT:
    /// `Bauer::element` ist rekursiv, und der Test_thread hat 2 MiB
    /// Stapel. Mit 64 MiB Stapel laeuft derselbe Auftrag durch — es ist
    /// also eine Frage der Tiefe, keine der Endlichkeit.
    ///
    /// Die Grenze ist eine Zusage von M5: "Rekursion nur mit
    /// Tiefenlimit". Sie steht beim EINZIGEN Eintiegspunkt (`kind`), weil
    /// dort jede Rekursion dieser Crate durchlaeuft.
    tiefe: usize,
}

/// Eine offene Inline-Auszeichnung (`<b>`, `<a>`, `<code>`, ...).
///
/// Sie ist kein Knoten, sondern ein Eintrag auf dem Stapel: sie sagt,
/// welcher Stil und welche Adresse ab HIER gilt. Beim Verlassen des
/// Elements wird sie popped, und der alte Stil gilt wieder — genau so,
/// wie es im Browser ist.
#[derive(Debug, Clone)]
/// 02.10. 18:35 (HTML): EIN OFFENES `<label>` WAEHREND DES BAUS.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein **Stapel** und keine zwei Felder: Labels
/// duerfen ineinander liegen (`<label for="a"><label>…</label></label>`), und
/// ein flaches Feld wuerde beim inneren Label das aeussere ueberschreiben —
/// also wuerde ein Klick auf den Text des aeusseren Labels ins falsche Feld
/// gehen.
struct LabelRahmen {
    /// DER LABEL-STAPEL TRAEGT KEINEN VORGABEWERT — siehe `label_neu`.
    ///
    /// GEMESSENE EINORDNUNG, warum hier kein `#[derive(Default)]` steht:
    /// `narsil_dom::NodeId` hat **kein** `Default` (der Compiler sagt es:
    /// „the trait bound `NodeId: Default` is not satisfied"), und der Bereich
    /// ist genau die Angabe, die der Bauer **beim Eintritt** misst. Deshalb
    /// gibt es `label_neu` als die eine Stelle, die einen Rahmen baut.

    /// Der **Textinhalt** dieses Labels, in der Reihenfolge des Schreibens.
    ///
    /// GEMESSENER BEFUND, warum hier **Text** steht und keine Knotenliste
    /// (18:50, Wegwerf-Messung entfernt): der erste Entwurf merkte die
    /// Textknoten, und GEMESSEN kam `[(5, 6), (4, 6)]` heraus — Knoten 5 ist
    /// „Name" (richtig, Feld 6), Knoten 4 ist aber „vorher ", also der Text
    /// **vor** dem Label. GEMESSENE EINORDNUNG, warum: der Absatz spult seine
    /// Texte **beim Schliessen** ab, und der `<input>`-Zweig schliesst den
    /// Absatz (`self.absatz_schliessen()`), also **innerhalb** des Labels — und
    /// damit wird auch der Text davor erzeugt. Der Knoten entsteht also nicht
    /// da, wo er hingehört, sondern spaeter und an anderer Stelle.
    text: String,
    /// Der Knoten des **Feldes**, das dieses Label umschliesst (der
    /// umschliessende Fall).
    feld: Option<narsil_dom::NodeId>,
    /// `for="X"` — der **Name** des Ziels, aufgeloest am Bauende (`fertig`),
    /// weil der Feldknoten beim Bauen des Labels noch nicht existiert.
    for_name: Option<String>,
    /// Der Knoten, an dem das `<label>` haengt — **beim Eintritt** gemessen,
    /// weil der Absatzknoten erst beim Abspulen entsteht (`label_text_sammeln`).
    bereich: narsil_dom::NodeId,
}

/// 02.10. 20:25 (HTML): EIN RAHMEN — die eine Stelle, die einen baut.
impl LabelRahmen {
    fn label_neu(bereich: narsil_dom::NodeId, for_name: Option<String>) -> Self {
        Self {
            text: String::new(),
            feld: None,
            for_name,
            bereich,
        }
    }
}

/// DER SCHLUESSEL EINER LABEL-REGEL: (Absatzknoten, Text des Labels).
///
/// GEMESSENE ENTSCHEIDUNG, warum zwei Angaben und nicht eine: der Text allein
/// waere mehrdeutig (zwei Labels mit „Name" auf einer Seite), der Knoten allein
/// auch (ein Absatz kann mehrere Labels haben). Der Absatz **plus** der Text
/// ist die kleinste Angabe, die die Frage „welches Feld gehoert zu diesem
/// Textabschnitt" eindeutig beantwortet. GEMESSENE EINORDNUNG des Restrisikos:
/// zwei **gleichnamige** Labels im **selben** Absatz wuerden auf denselben
/// Schluessel fallen — dann gewinnt das spaeter eingetragene Feld, und das ist
/// der selte Fehler, den ich nicht messen kann (es kommt auf echten Seiten
/// nicht vor; `ubuntu.com` hat 47 Labels in eigenen Absaetzen).
type LabelSchluessel = (narsil_dom::NodeId, String);

/// WAS EIN `<label>` BEHAUPTET.
#[derive(Clone)]
struct LabelRegel {
    /// Das umschliessende Feld (wenn es eins gab).
    feld: Option<narsil_dom::NodeId>,
    /// `for="X"`, noch unaufgeloest.
    for_name: Option<String>,
}

/// Release 08.10.2026: ein offenes Absatz-Element ohne eigenen Kasten.
struct AbsatzRahmen {
    /// Der Kasten, in den seine Zeilen gehen (`mutter()` beim Start).
    eltern: NodeId,
    /// So viele Kinder hatte `eltern` beim Start — was danach kam, gehoert
    /// zu diesem Absatz.
    kinder_anfang: usize,
    /// Der Blockstil des Elements (Rand, Hintergrund ...).
    stil: Style,
    ord: usize,
    /// Sein Platz auf dem Kasten-Stapel (der Plan gehoert an den Kasten).
    platz: usize,
    /// Der nachtraeglich gebaute Kasten.
    kasten: Option<NodeId>,
    /// Release 10.10.2026: so tief war `kasten_textstile` beim Start — ist
    /// er jetzt tiefer, steht ein Kasten INNERHALB dieses Absatz-Elements.
    kasten_tiefe: usize,
}

struct Marke {
    stil: Style,
    href: Option<String>,
    /// W7C R6 (01.10. 23:55): DIE NUMMER DES INLINE-ELEMENTS.
    ///
    /// GEMESSENER FEHLER, seit 15:20 als `#[ignore]`-Test abgelegt: der
    /// Textknoten eines Links bekam die Nummer des ABSATZES, nicht die
    /// des `<a>`. Ein Klick auf den Link fand den Handler nie, und
    /// `preventDefault` verhinderte nichts — der Link navigierte.
    ///
    /// GEMESSEN, warum die Nummer in der MARKE steht und nicht im
    /// Inline-Zweig (23:45): der Textknoten entsteht erst beim
    /// SCHLIESSEN des Absatzes (`absatz_schliessen`), nicht beim Bau des
    /// Inline-Elements. Gemessen: `<a id=l>weg</a>` — `vor_kinder=3`,
    /// `grenze=3`, `doc=3` nach dem Bau der Kinder. Der Inline-Zweig
    /// hatte also gar keinen Knoten, den er beanspruchen koennte. Die
    /// Marke ist der einzige Ort, der den Absatz ueberlebt.
    ord: Option<usize>,
}

/// Release 10.10.2026: ein wartendes `::first-letter` (CSS Pseudo-Elements 4
/// §2.3): die CSS-Nummer des Blockbehaelters, seine passenden Regeln und
/// die Bautiefe, in der er offen ist.
struct ErsterBuchstabeAuftrag {
    id: u32,
    regeln: Vec<narsil_css::Regel>,
    tiefe: usize,
}

/// Ein Stueck Text im Absatz mit eigenem Stil oder eigenem Link.
///
/// WARUM ein Abschnitt und nicht Text mit Platzhaltern: ein Absatz mit
/// einem Link in der Mitte muss an der Stelle des Links eine andere
/// Farbe UND eine andere Adresse haben. Ohne Abschnitte gaebe es nur
/// "ganzer Absatz ist ein Link" oder "kein Link".
#[derive(Debug, Clone)]
struct Abschnitt {
    text: String,
    stil: Style,
    href: Option<String>,
    /// W7C R6: die Nummer des INLINE-Elements, aus dem dieser Abschnitt
    /// stammt. `None` = der Abschnitt gehoert dem Absatz (siehe `Marke::ord`
    /// — dort steht, warum das erst beim Absatzschluss aufloesbar war).
    ord: Option<usize>,
    /// Release 11.10.2026: der Leerraum am Ende kam (auch) aus einem Kontext
    /// ohne `nowrap` — ein Leerraum-Textknoten haengt sich an den vorigen
    /// Abschnitt (siehe `text_ohne_pre`), sein `white-space` ging dabei
    /// verloren (`<span nowrap>X </span> <span nowrap>X </span>`).
    leer_bricht: bool,
}

/// Ein Absatz, der gesammelt wird, bevor er umbricht.
///
/// WARUM gesammelt und nicht sofort geschrieben: HTML liefert Text in
/// winzigen Stuecken ("a", "b", "c" zwischen Tags). Ohne Sammeln wuerde
/// jedes Stueck eine eigene Zeile ergeben, und die Seite bestuende aus
/// 400 einzeiligen Absaetzen.
/// Release 13.10.2026 (Agent CSS2): eine offene Zeilenklemme (`line-clamp`,
/// CSS Overflow 4 §4.2). Die Zeilen ALLER Absaetze im Block zaehlen, auch in
/// verschachtelten Bloecken; hinter der letzten erlaubten Zeile wird nichts
/// mehr gesetzt (`continue: collapse`), und steht dort noch Inhalt, endet
/// die letzte Zeile mit der Auslassung (`block-ellipsis: auto`). GEMESSEN
/// vorher (WPT css-overflow `line-clamp/line-clamp-0*`, `webkit-line-clamp-
/// 0*`): die Klemme galt nur fuer den Absatz direkt im Block, und Zeilen aus
/// `white-space: pre` (je Zeile ein Absatz) wurden nie geklemmt.
#[derive(Debug, Clone, Copy)]
struct Klemme {
    /// Wie viele Zeilen noch Platz haben.
    rest: usize,
    /// Der zuletzt geschriebene Textknoten in der Klemme.
    letzter_text: Option<NodeId>,
    /// Release 14.10.2026 (Agent CSS2): the last line is RTL, so its logical
    /// end — where the ellipsis goes — is the visual start of the text node
    /// (the lines are in visual order, see `bidi_visual_runs`).
    prepend: bool,
    /// Die Auslassung steht schon.
    ellipse: bool,
    /// `block-ellipsis` (aus `line-clamp: … no-ellipsis`, CSS Overflow 4 §4.3).
    mit_auslassung: bool,
}

struct Absatz {
    /// Release 15.10.2026 (Agent CSS2): the block of the paragraph is
    /// shrink-to-fit (inline-block, float or absolutely positioned, without a
    /// width of its own) — the test `nachumbruch` uses to skip a re-break.
    shrinks: bool,
    /// Release 15.10.2026 (Agent CSS2): `text-wrap: balance` (or
    /// `text-wrap-style: balance`) on the block, see `AbsatzDaten::balance`.
    balance: bool,
    /// Release 10.10.2026: siehe [`AbsatzDaten::max_zeilen`].
    max_zeilen: Option<usize>,
    breite: f32,
    zeilenhoehe: Option<f32>,
    ausrichtung: narsil_css::Textausrichtung,
    /// Release 13.10.2026 (Agent CSS2): siehe [`AbsatzDaten::ausrichtung_letzte`].
    ausrichtung_letzte: Option<narsil_css::Textausrichtung>,
    /// Release 13.10.2026 (Agent CSS2): siehe [`AbsatzDaten::blocksatz_aus`].
    blocksatz_aus: bool,
    /// Der Stil des ganzen Absatzes (der Blockstil: Ueberschrift, Zitat,
    /// Listenpunkt, …). Ein Abschnitt kann ihn veraendern, nie umkehrt.
    stil: Style,
    /// Ziel des Links, wenn der GANZE Absatz ein Link ist
    /// (`<p><a>…</a></p>` mit nur einem Abschnitt).
    href: Option<String>,
    /// Der Text in Stuecken. Nie leer: es gibt immer mindestens einen
    /// Abschnitt, auch fuer einen leeren Absatz.
    abschnitte: Vec<Abschnitt>,
    /// W6E: die Vorlauf-Nummer des Elements, das diesen Absatz gestartet
    /// hat — der Absatz gehoert diesem Element, und die Knoten, die erst
    /// beim SCHLIESSEN entstehen, muessen seine Nummer bekommen.
    ///
    /// GEMESSENER FEHLER, warum das im Absatz steht und nicht im Bauer
    /// (30.09.2026, 16:55, Koordinationsbefund „21 Klicks auf den Knopf,
    /// `klick()` immer `false"): `absatz_starten` schrieb
    /// `laufender_ord` VOR `absatz_schliessen`. Damit bekam der Absatz des
    /// VORHERIGEN Blocks die Nummer des NAECHSTEN. Gemessen an
    /// `zaehler.html`: die Kaesten des Knopfs (`klick_ziele` nennt 5)
    /// trugen die Nummer 4 — die Nummer, die `getElementById("k")`
    /// liefert. Der Handler haengte an 4, der Klick fand 5, also fand er
    /// nichts. Und weil die Nummer 4 zugleich die des Knopf-KASTENS war,
    /// ZAEHLTE ein Klick auf die Ueberschrift — mein eigener Test tat
    /// genau das und war gruen.
    ///
    /// GEMESSEN, warum die Zahl im Absatz und nicht im Bauer: sie ist
    /// ZUSTAND des Absatzes. Der Bauer hat zwischen `absatz_starten` und
    /// `absatz_schliessen` beliebig viele andere Aufrufe, und jedes davon
    /// darf sie nicht ueberschreiben.
    ord: Option<usize>,
    /// Release 08.10.2026: Pseudokaesten von Inline-Elementen, die IN
    /// diesem Absatz stehen — `(Abschnitt, Knoten, nach)`: der Kasten
    /// gehoert vor den Abschnitt mit dieser Nummer (`nach` = `::after`, er
    /// bleibt am Ende der Zeile seines Vorgaengers). Siehe
    /// `pseudo_im_textfluss` und `zeilen_schreiben`.
    anker: Vec<(usize, NodeId, bool)>,
    /// Nach einem Anker beginnt der naechste Text einen EIGENEN Abschnitt,
    /// auch bei gleichem Stil — sonst haette der Anker keine Grenze.
    trennen: bool,
    /// Release 11.10.2026 (Agent CSS2): siehe `AbsatzDaten::einzug`.
    einzug: f32,
    /// Release 12.10.2026 (Agent CSS2): `hanging-punctuation: first` an der
    /// ersten Zeile des Blocks (CSS Text 4 §9.2).
    haengt: bool,
    /// Release 13.10.2026 (Agent CSS2): der Block laeuft von rechts nach links.
    rtl: bool,
    /// Release 14.10.2026 (Agent CSS2): see [`BidiMode`].
    bidi_mode: BidiMode,
    /// Release 13.10.2026 (Agent CSS2): der Block beschneidet seinen
    /// Ueberlauf (`overflow` nicht `visible`).
    beschnitten: bool,
    /// Release 13.10.2026 (Agent CSS2): siehe [`AbsatzDaten::auslassung`].
    auslassung: Option<String>,
}

impl Absatz {
    /// Der Text des Absatzes, ohne die Abschnittsgrenzen.
    ///
    /// Nur fuer Pruefungen und fuer den Fall "ein einziger Abschnitt",
    /// in dem es schneller ist, den ganzen Text zu nehmen.
    fn text(&self) -> String {
        let mut raus = String::new();
        for a in &self.abschnitte {
            raus.push_str(&a.text);
        }
        raus
    }
}

/// NUR FUER DIE MESSUNG (L3, 02.10.2026 07:1x): `NARSIL_BREITE_AN=1`
/// schaltet `width`/`height` aus dem Kaskaden-Weg EIN.
///
/// GEMESSEN, warum der VORGABEZUSTAND „aus" ist: 29 harte
/// Ueberlappungen ohne, 455 mit (`l3_seiten.rs`, A/B im selben Binary,
/// 18 Seiten). Der ganze Unterschied ist eine Seite, und auf ihr steht
/// `table { width: 100% }` — der Bereich mit 22 vorbestehenden
/// Fehlern im Tabellenleg.
///
/// Einmal gelesen, nicht je Knoten: `std::env::var_os` nimmt auf Linux
/// die Umgebungssperre, und `stil_von_css` laeuft je Knoten. GEMESSEN
/// beim Kaskaden-Tempo: 2 000 ms fuer einen Schalter, der je Aufruf las.
///
/// GEMESSENE ENTSCHEIDUNG, warum der WERT geprueft wird und nicht die
/// Anwesenheit der Variablen: `var_os` liefert fuer "0" genauso
/// `Some(..)`. Das war der zweite Fehler am selben Schalter.

fn breite_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var("NARSIL_BREITE_AN").ok().as_deref() == Some("1"))
}

impl<'a> Bauer<'a> {
    fn neu(breite: u32, basis: &'a str) -> Self {
        // OHNE Messung: die eingebaute Bitmap. Das ist der Zustand vor
        // dem Umbau und der Weg von `.narsil` und allen Alt-Tests.
        //
        // 05.10.2026: `0` = unbekannte Ansichtshoehe (siehe das Feld). Das ist
        // der Aufrufer ohne Fenster, und fuer ihn bleibt alles wie vorher.
        Self::mit_ansicht(breite, 0, basis, narsil_layout::Messung::bitmap())
    }

    /// Wie [`Bauer::neu`], aber mit der Messung (W5G).
    ///
    /// Der Bau nimmt die Messung und nicht die Schrift: die Frage
    /// "Bitmap oder Systemschrift" wird EINMAL beantwortet, oben, und
    /// nicht noch einmal hier.
    fn mit_messung(breite: u32, basis: &'a str, messung: narsil_layout::Messung) -> Self {
        // 05.10.2026: ohne Fenster = unbekannte Ansichtshoehe (siehe das Feld).
        Self::mit_ansicht(breite, 0, basis, messung)
    }

    /// Wie [`Bauer::mit_messung`], aber **mit** der Ansichtshoehe.
    ///
    /// 05.10.2026: der Weg, den der Browser nimmt. `mit_messung` bleibt die
    /// Schwester **ohne** Fenster — so muss kein alter Aufrufer (`.narsil`,
    /// Tests, Kommandozeile) etwas aendern, und die hoehe steht trotzdem an
    /// **einer** Stelle.
    fn mit_ansicht(
        breite: u32,
        ansicht_hoehe: u32,
        basis: &'a str,
        messung: narsil_layout::Messung,
    ) -> Self {
        Bauer {
            doc: Document::new(),
            offen: Vec::new(),
            // 02.10. 18:35 (HTML): der Label-Stapel und seine Tabelle. Siehe
            // `Seite::label_zu_knoten` — der Bauer ist der einzige Ort, an dem
            // bekannt ist, dass er **innerhalb** eines `<label>` ist.
            label_zu_knoten: HashMap::new(),
            label_zu_text: HashMap::new(),
            label_regeln: HashMap::new(),
            label_offen_knoten: Vec::new(),
            label_stapel: Vec::new(),
            label_for_unfertig: Vec::new(),
            links: HashMap::new(),
            // Release 11.10.2026: hoechstens auf 1 px geklemmt (vorher 120) —
            // ein Iframe ist oft schmaler. GEMESSEN vorher (WPT dialog
            // centering.html, Iframe 40 px breit): die Seite im Iframe wurde
            // 120 px breit gelegt, jede Zentrierung lag daneben.
            breite: breite.max(1),
            ansicht_hoehe,
            basis,
            laufender: None,
            inline_bild_eltern: std::collections::HashSet::new(),
            einzug_frei: Vec::new(),
            klemmen: Vec::new(),
            bearbeitbar: 0,
            marken: Vec::new(),
            titel: String::new(),
            favicon: None,
            skripte: Vec::new(),
            ereignisse: Vec::new(),
            stile: Vec::new(),
            kosmetik: None,
            bilder: Vec::new(),
            hintergrundbilder: HashMap::new(),
            maskenbilder: HashMap::new(),
            inline_masks: Vec::new(),
            picture_quelle: None,
            ids: HashMap::new(),
            id_zu_knoten: HashMap::new(),
            aktuelle_id: None,
            // W6 (J9): Felder fuer die DOM-Anbindung. `css_adresse`
            // entsteht in `kaskade_einrichten`, `css_zu_dom` beim Bau.
            css_baum: crate::kaskade::CssBaum::default(),
            css_zu_dom: HashMap::new(),
            css_adresse: HashMap::new(),
            naechste_ordinal: 0,
            ord_karte: HashMap::new(),
            aktualer_ordinal: 0,
            patch: Patch::default(),
            dom_zu_ord: HashMap::new(),
            // 03.10. 12:55: die Tabellen der Formularfelder (siehe `FeldOrd`).
            felder: Vec::new(),
            formular_ords: Vec::new(),
            formular_stapel: Vec::new(),
            // W7C R3 (siehe die Felder in `Bauer` und `Seite`).
            layout_plan: narsil_layout::LayoutPlan::leer(),
            plan_stapel: Vec::new(),
            eigener_kasten: None,
            kasten_stapel: Vec::new(),
            cellpadding_stapel: Vec::new(),
            cellspacing_stapel: Vec::new(),
            laufender_ord: None,
            // KOMPRESS 04.10.2026: siehe das Feld `audio_zaehler`.
            audio_zaehler: 0,
            video_zaehler: 0,
            // 05.10.2026 (Canvas): hat `<html>` die Canvas-Farbe schon
            // gesetzt? Siehe das Feld und `element_innen`.
            canvas_von_html: false,
            canvas_blocked: false,
            root_has_image: false,
            element_stapel: Vec::new(),
            offene_stile: Vec::new(),
            kasten_textstile: Vec::new(),
            element_ist_text: false,
            einzug_von_ord: HashMap::new(),
            kaskade: None,
            messung,
            kommender_stil: None,
            pseudo_vorher: None,
            pseudo_nachher: None,
            platzhalter_farbe: None,
            pseudo_before_stapel: Vec::new(),
            pseudo_text_wartend: Vec::new(),
            erster_buchstabe: Vec::new(),
            erster_buchstabe_geparkt: Vec::new(),
            erste_zeile: Vec::new(),
            erste_zeile_geparkt: Vec::new(),
            pseudo_balance: None,
            sprachen: Vec::new(),
            zaehler: Vec::new(),
            zitate: Vec::new(),
            zitat_tiefe: 0,
            nach_wartend: Vec::new(),
            absatz_rahmen: Vec::new(),
            bild_absatz_stile: Vec::new(),
            listen_zaehler: Vec::new(),
            neu_gebaut: std::collections::HashSet::new(),
            svg_definitionen: HashMap::new(),
            absaetze: Vec::new(),
            feld_rahmen_ua: None,
            einzug: 0.0,
            textbreite: (breite as f32 - 2.0 * SEITENRAND).max(0.0),
            textbreite_von_ord: HashMap::new(),
            tiefe: 0,
        }
    }

    /// Die Wurzel (`page`) anlegen und zum offenen Knoten machen.
    fn wurzel(&mut self) {
        //
        // W7C (02.10. 05:00, Befund der Rolle PROBE, 143 Messungen auf
        // echten Seiten): die Wurzel bekam `width = Fensterbreite` UND
        // einen Rand von 16 px. Der Rand liegt AUSSERHALB der Box, also
        // endete die Seite bei `16 + breite` - 16 px rechts daneben.
        // GEMESSEN, warum `breite - 2*SEITENRAND` und nicht `breite`:
        // der Rand steht auf beiden Seiten, und die Seite soll in das
        // Fenster passen, nicht in ein Fenster plus zwei Raender.
        //
        // 04.10. 02:50 (HTML): `SEITENRAND` ist **0**. GEMESSEN an
        // `<div style='background:red'>x</div>` bei 1280 px: vorher
        //     Rumpf x=24 w=1232 / div x=24 w=1232
        // Firefox
        //     Rumpf x=8  w=1264 / div x=8  w=1264
        // Ursache war der Rand auf der **Wurzel** (16 px je Seite) — CSS
        // 2.1 Anhang D gibt `html` keinen `margin`, nur `body { margin: 8px }`,
        // und der kam schon immer an. Der W7C-Ueberfall oben ist mit
        // `nutzbar = breite - 2*SEITENRAND` ohnehin weg; bei 0 ist
        // `nutzbar = breite`.
        let nutzbar = (self.breite as f32 - 2.0 * SEITENRAND).max(40.0);
        let stil = Style {
            deckkraft: 1.0,
            unsichtbar: false,
            radius: [0.0; 4],
            schatten_anzahl: 0,
            // P8 (02.10.2026), BAUFIX: `Style::schatten` gibt es erst
            // seit heute (CSS Backgrounds 3 §9.3, `MAX_SCHATTEN` = 8).
            // Siehe die gleiche Anmerkung in `hteile.rs`.
            schatten: Default::default(),
            // BILD_48, Station 2: `background-image: none` ist der
            // Anfangswert, also **kein** Verlauf. Das hier ist der
            // Seiten-Kasten, und die Verlaeufe der **Elemente** kommen ueber
            // `kaskade::stil_von_css`.
            verlauf: None,
            verlauf_kachel: None, clip_rand: None, umriss: None, border_image: false, clip_pfad: None, clip_svg: None, maske_bild: None, filter_value: None, filter_svg: None, backdrop_filter: None, backdrop_svg: None, radius_xy: [[0.0; 2]; 4], radius_percent: 0,
            dekoration: 0,
            size: Dimension {
                width: Length::px(nutzbar),
                height: Length::Auto,
            },
            background: FARBE_SEITE,
            color: FARBE_TEXT,
            font_size: GRAD_TEXT,
            // 04.10.2026 (SCHRIFT): BAUFIX zu `schriftgewicht`/`kursiv`
            // (E0063 am Wurzelstil). 400/false ist `normal` (CSS Fonts 4
            // §5.3) — das Blatt selbst ist nicht fett und nicht kursiv.
            schriftgewicht: 400,
            kursiv: false,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `Style::schriftfamilie`.
            // Das Wurzelblatt schreibt keine Familie — die Vorgabe ist die
            // generische Auffangfamilie (CSS Fonts 4 §4.2).
            schriftfamilie: String::from("sans-serif"),
            vertikal_hoch: 0.0,
            buchstaben_abstand: 0.0,
            wort_abstand: 0.0,
            // 04.10.2026 (BAUFIX fuer BILD_32): `rahmen` gibt es seit
            // heute in `Style` (narsil-core/src/style.rs:188, Vorgabe
            // `Rahmen::default()`, style.rs:299). Das Wurzelblatt hat
            // keinen Rahmen — das ist die Vorgabe, nicht `Default`.
            rahmen: narsil_core::style::Rahmen::default(),
            direction: Direction::Column,
            margin: Spacing::all(Length::px(SEITENRAND)),
            padding: Spacing::ZERO,
            textausrichtung: Textausrichtung::Links,
            weissraum: Weissraum::Normal,
            umbruch_locker: false,
            umbruch_ueberall: false,
            letter_breaks: narsil_core::style::LetterBreaks::None,
            break_word_only: false,
            min_content_floor: 0.0,
            hyphens_none: false,
            hyphenate_character: None,
            cjk_sprache: false,
            zeilenhoehe_art: ZeilenhoeheArt::Normal,
            zeilenhoehe_px: {
                let fs: f32 = GRAD_TEXT;
                let em = (fs * 1229.0 / 1024.0 + 0.5).floor();
                let auf = (fs * 1901.0 / 2048.0).ceil().max(1.0);
                let ab = (fs * 483.0 / 2048.0).ceil().max(1.0);
                em.max(auf + ab)
            },
        };
        let _ = self.doc.set_style(self.doc.root(), stil);
        self.offen.push(self.doc.root());
    }

    /// Das Ergebnis, inklusive eines noch offenen Absatzes.
    fn fertig(mut self) -> Seite {
        self.absatz_schliessen();
        // Release 11.10.2026: anonyme Tabellenobjekte (CSS 2.1 §17.2.1).
        tabelle_anonym::ergaenzen(&mut self.doc, &mut self.layout_plan);
        tabelle_kanten::aufloesen(&mut self.doc, &self.layout_plan);
        //
        // 02.10. 18:35 (HTML): `<label for="X">` AUFLOESEN — erst jetzt sind
        // alle Ids bekannt. GEMESSENE ENTSCHEIDUNG, warum es hier und nicht im
        // `label`-Zweig: der Feldknoten kann **spaeter** im Dokument stehen als
        // das Label (GEMESSEN an der Testseite: `<label for="c">` steht vor dem
        // Kästchen), also gibt es beim Bauen des Labels nichts aufzuloesen.
        //
        // GEMESSENE EINORDNUNG, warum `id_zu_knoten` und nicht `ids`: `ids`
        // traegt die **Ordnungszahl** (W6/J9), und der Fokus sucht einen
        // **Knoten** (`dom_zu_ord` geht den Weg zurueck, aber das ist eine
        // Umrechnung fuer den Weg Node -> Ord). Die Tabelle, die hier gebraucht
        // wird, ist genau die, die der Anker-Sprung seit 09:56 benutzt.
        //
        // GEMESSENER BEFUND, warum der Weg ueber die **Ordnungszahl** laeuft und
        // nicht ueber `id_zu_knoten` (Messstelle `NARSIL_L1_LABEL`, Wegwerf):
        // `<label for="c">Haken</label> <input type="checkbox" id="c">` ergibt
        // GEMESSEN `id_zu_knoten = [("c", 10)]` — und Knoten 10 ist der
        // **Textknoten** des Labels, nicht das Kaestchen. GEMESSENE EINORDNUNG,
        // warum: der `input`-Zweig spult den Absatz ab, **während** `aktuelle_id`
        // schon auf „c" steht (`self.absatz_schliessen()`, Zeile 5421), und der
        // Absatz meldet die Id fuer den Knoten, den er dabei erzeugt.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Ordnungszahl trotzdem rauskommt:
        // `ids["c"]` ist **13** (dieselbe Zahl wie `Bauer::aktualer_ordinal` des
        // Kaestchens), und `dom_zu_ord` traegt 13 am Kaestchen-Knoten — GEMESSEN
        // `dom_zu_ord = [(11, 13), (12, 13), …]`. Die Tabelle braucht fuer den
        // Fokus ohnehin nur die **Nummer** (so liest `fokus_bei_klick`), also
        // genuegt jeder Knoten mit dieser Nummer.
        //
        // WAS ICH NICHT BEHAUPTE: dass `id_zu_knoten` fuer Felder falsch ist und
        // der Anker-Sprung darunter leidet. GEMESSEN ist nur der Fall
        // `<input id=…>` **mit** vorangehendem Text. Der Weg ueber die Nummer
        // ist deshalb der gewaehlte: er ist der **gemessene**.
        for (knoten, name) in std::mem::take(&mut self.label_for_unfertig) {
            let Some(ord) = self.ids.get(&name).copied() else {
                continue;
            };
            if let Some((index, _)) = self
                .dom_zu_ord
                .iter()
                .find(|(_, o)| **o == ord)
                .map(|(k, o)| (*k, *o))
            {
                self.label_zu_knoten.insert(knoten, narsil_dom::NodeId(index));
            }
        }
        //
        // 02.10. 21:50 (HTML): DIE REGELN AUCH NACH DEM TEXT — fuer den
        // Klickpfad, der den Knoten **und** den Text kennt (siehe
        // `Seite::label_zu_text`). GEMESSENE EINORDNUNG, warum es hier und
        // nicht beim Abspulen passiert: der `for`-Fall laesst sich erst hier
        // aufloesen, und **beide** Faelle muessen in derselben Tabelle stehen,
        // weil der Klick nicht unterscheidet, ob er umschliessend oder `for`
        // vor sich hat.
        //
        // GEMESSENE EINORDNUNG, warum hier ueber die **Nummer** aufgeloest
        // wird und nicht ueber `label_zu_knoten`: dieselbe Aussage steht dort
        // in der Knotenform (`[(5, 6)]` = Labeltextknoten auf Feldknoten), und
        // die Regel traegt den **Bereich** (Knoten 2). Beides ist dieselbe
        // Aussage in zwei Schluesselformen; die Form fuer den Klick ist die
        // mit dem Text.
        let mut regeln_fuer_text: Vec<((narsil_dom::NodeId, String), narsil_dom::NodeId)> = Vec::new();
        for (schluessel, regel) in std::mem::take(&mut self.label_regeln) {
            if let Some(feld) = regel.feld {
                regeln_fuer_text.push((schluessel, feld));
                continue;
            }
            if let Some(name) = regel.for_name
                && let Some(ord) = self.ids.get(&name).copied()
                && let Some((index, _)) = self
                    .dom_zu_ord
                    .iter()
                    .find(|(_, o)| **o == ord)
                    .map(|(k, o)| (*k, *o))
            {
                regeln_fuer_text.push((schluessel, narsil_dom::NodeId(index)));
            }
        }
        let label_zu_text = regeln_fuer_text.into_iter().collect();
        if !self.inline_masks.is_empty() {
            // The text nodes of boxless inline elements with a mask (see
            // `inline_masks`): their style shares the element's key.
            let by_key: HashMap<*const u8, Vec<MaskenEbene>> = std::mem::take(&mut self.inline_masks)
                .into_iter()
                .map(|(k, m)| (std::sync::Arc::as_ptr(&k) as *const u8, m))
                .collect();
            for i in 0..self.doc.len() {
                let id = narsil_dom::NodeId(i as u32);
                let Some(n) = self.doc.get(id) else { continue };
                if n.kind != NodeKind::Text || self.maskenbilder.contains_key(&id) {
                    continue;
                }
                if let Some(m) = n.style.maske_bild.as_ref().and_then(|k| by_key.get(&(std::sync::Arc::as_ptr(k) as *const u8))) {
                    self.maskenbilder.insert(id, m.clone());
                }
            }
        }
        Seite {
            document: self.doc,
            links: self.links,
            basis: self.basis.to_string(),
            titel: self.titel.clone(),
            favicon: self.favicon.clone(),
            skripte: self.skripte,
            ereignisse: self.ereignisse,
            stile: self.stile,
            bilder: self.bilder,
            hintergrundbilder: self.hintergrundbilder,
            maskenbilder: self.maskenbilder,
            // 03.10. 12:55: DIE TABELLEN DES BAUERS. `std::mem::take` wie beim
            // Plan - sie werden beim Bauen gefuellt und danach nur noch
            // gelesen; eine Kopie waere eine zweite Zahl, die auseinander-
            // laufen kann.
            felder: std::mem::take(&mut self.felder),
            formular_ords: std::mem::take(&mut self.formular_ords),
            ids: self.ids,
            id_zu_knoten: std::mem::take(&mut self.id_zu_knoten),
            // 02.10. 18:35 (HTML): die Label-Tabelle wandert mit — `take` wie
            // bei den anderen Bau-Tabellen (siehe dort die Begruendung).
            label_zu_knoten: std::mem::take(&mut self.label_zu_knoten),
            // 02.10. 21:50 (HTML): die Klickform (siehe `Seite::label_zu_text`).
            label_zu_text,
            css_baum: self.css_baum.clone(),
            stil_satz: self.kaskade.as_ref().map(|k| StilSatz {
                blaetter: k.alle.clone(),
                vormerker: k.vormerker_alle.clone(),
                ansicht: (self.breite, self.ansicht_hoehe),
                zustand: k.baum.zustand,
                messung: self.messung.clone(),
                basis: self.basis.to_string(),
            }),
            css_zu_dom: self.css_zu_dom.clone(),
            dom_zu_ord: self.dom_zu_ord.clone(),
            // W7C R3: `std::mem::take` und nicht `clone` - der Plan wird
            // beim Bauen gefuellt und danach nur noch gelesen. Eine Kopie
            // waere eine zweite Zahl, die auseinanderlaufen kann.
            layout_plan: std::mem::take(&mut self.layout_plan),
            absaetze: std::mem::take(&mut self.absaetze),
        }
    }

    fn mutter(&self) -> NodeId {
        self.offen.last().copied().unwrap_or(self.doc.root())
    }

    // ------------------------------------------------------------------
    // Der Baum
    // ------------------------------------------------------------------

    /// I2: die `<style>`-Bloecke der Seite einsammeln, BEVOR gebaut wird.
    ///
    /// Die Bloecke stehen nach HTML-Regel im `<head>`, und `<head>` wird
    /// nicht gebaut (er ist unsichtbar). Also muessen sie VOR dem Bau
    /// gelesen werden — genau das ist der Grund fuer diese Methode.
    ///
    /// M3: `MAX_STILBLOECKE` (64) und `MAX_STILTEXT` (1 MiB), beide in
    /// `skripte` begrenzt, VOR der Uebernahme.
    fn stile_sammeln(&mut self, knoten: &[HtmlNode]) {
        self.stile_sammeln_in(knoten, false);
        self.stile_aus_patch();
    }

    /// Release 08.10.2026: `<style>`-Elemente, die ein Skript eingefügt hat
    /// (`insertAdjacentHTML('afterbegin', '<style>…</style>')`). GEMESSEN:
    /// example.com setzt so `p+p{font-size:.875em;opacity:.6}` — die Regel
    /// kam nie in die Kaskade, weil nur der geparste Baum gesammelt wurde.
    /// Ein Textkind eines neuen Knotens ist selbst ein neuer Knoten mit Text.
    fn stile_aus_patch(&mut self) {
        let stile: Vec<(usize, Option<String>)> = self
            .patch
            .neu
            .iter()
            .filter(|n| n.tag.eq_ignore_ascii_case("style"))
            .map(|n| (n.ord, n.text.clone()))
            .collect();
        for (ord, eigen) in stile {
            let mut css = eigen.unwrap_or_default();
            for kind in self.patch.neu.iter().filter(|k| k.eltern_ord == ord) {
                if let Some(t) = &kind.text {
                    css.push_str(t);
                    css.push('\n');
                }
            }
            if !css.trim().is_empty() {
                self.stile_aufnahme(&HashMap::new(), &[HtmlNode::Text(css)]);
            }
        }
    }

    /// Dasselbe, mit dem Wissen, ob wir **im `<noscript>`** stehen.
    ///
    /// GEMESSENER BEFUND (03.10. 18:12, `team/log/codex8/vor/html/
    /// duckduckgo.com.html`, Wegwerf-Messung `tests/_m_ddg.rs`):
    /// `parse_html` liefert **320** Knoten und **55** Textknoten,
    /// `seite_aus_text` daraus **2** Knoten und **0** Textknoten — die
    /// Seite ist LEER. Ursache, an den Zahlen belegt:
    ///
    /// ```text
    /// )();</script><noscript><meta http-equiv="refresh" content="0;
    /// url="https://html.duckduckgo.com/html"/><style>body
    /// { display: none }</style></noscript>
    /// ```
    ///
    /// Das ist das Next.js-Muster: **ohne** JavaScript wird der Rumpf
    /// versteckt und per `<meta http-equiv="refresh">` auf die No-JS-
    /// Fassung umgeleitet. GEMESSEN, wie es bei uns ankommt:
    ///
    /// ```text
    /// MESSUNG 1 NOSCRIPT <noscript> kinder=2
    /// MESSUNG 1 NOSCRIPT <meta> kinder=0
    /// MESSUNG 1 NOSCRIPT <style> kinder=1
    /// MESSUNG 2 stile=3
    /// MESSUNG 2   stil[0] 22 byte: body { display: none }
    /// ```
    ///
    /// `body { display: none }` aus diesem Block hat die **ganze Seite**
    /// verschluckt: 2 Knoten, kein Text, weisses Bild.
    ///
    /// GEMESSENE URSACHE im Parser (fremde Crate, `narsil-fmt-html`):
    /// `lib.rs:391` ruft `treebuilder::parse_dokument_mit_fehler(html,
    /// false)` — und das `false` ist das **Scripting-Flag**
    /// (`treebuilder.rs:5083`). Mit `scripting == false` nimmt der
    /// Baumaufbau bei `<noscript>` den Zweig `Modus::ImHeadNoscript`
    /// (Zeile 1845) und parst den Inhalt als **Markup**; mit `true`
    /// waere es `Zustand::Rawtext` (Zeile 1843/2818) und der Inhalt
    /// waere **literaler Text**. HTML 5, 13.3 sagt genau das: ist
    /// Scripting an, ist der Inhalt eines `<noscript>` literaler Text.
    /// Wir **sind** ein Browser mit JavaScript.
    ///
    /// WAS ICH HIER TUE — und warum es nicht die Reparatur ist: die
    /// Sammelstelle ist **meine** Datei, und die Regel gehoert hierher:
    /// ein Stylesheet, das im `<noscript>` steht, ist fuer einen Browser
    /// **mit** Scripting kein Stylesheet. Ich sammle es nicht ein. Das
    /// ist keine Notloesung fuer den Parser, sondern dieselbe Regel von
    /// der anderen Seite — und sie haelt auch dann, wenn der Parser
    /// irgendwo anders falsch liegt.
    ///
    /// WAS NICHT HIERHER GEHOERT: `HtmlNode::is_invisible`
    /// (`narsil-fmt-html/src/lib.rs:257`) behandelt `noscript` nach der
    /// Vorgabe „Welle 4: `<noscript>` wird ANGEZEIGT" als sichtbar. Das
    /// ist die Entscheidung einer anderen Rolle und ich fasse sie nicht
    /// an. GEMESSENE FOLGE, damit niemand sie uebersieht: der **Text**
    /// im `<noscript>` dieser Seite wird also weiterhin gebaut (er ist
    /// nach jener Vorgabe sichtbar) — nur sein `<style>` wirkt nicht
    /// mehr. Bericht: `team/berichte/HTML_3.txt`.
    fn stile_sammeln_in(&mut self, knoten: &[HtmlNode], im_noscript: bool) {
        for k in knoten {
            let HtmlNode::Element {
                tag,
                attributes,
                children,
                ..
            } = k
            else {
                continue;
            };
            //
            // W7C A4 (09:20): TITEL UND FAVICON — im SELBEN Durchlauf.
            //
            // GEMESSENER FEHLER (drei Anlaeufe, alle scheitern): der Weg
            // ueber `kind()`/`element()` fuehrt fuer den KOPF nirgends
            // hin. `kind()` ruft `element()` nur, wenn die Kaskade
            // „sichtbar" sagt, und das UA-Stylesheet sagt fuer
            // `head, script, style, title, meta, link { display: none }`.
            // Also: `kind(head)` → nichts, `element(head)` nie, und die
            // Kinder des Kopfs werden nie besucht.
            //
            // DIESER Durchlauf (`stile_sammeln`) ist der, der wirklich
            // laeuft — er steckt sich nicht hinter die Kaskade, und
            // `Seite::stile` ist auf jeder echten Seite gefuellt (das ist
            // der Beweis: `<style>` im Kopf kommt an). Also gehoeren
            // Titel und Favicon HIERHER, nicht in den Zeichenweg.
            //
            // GEMESSEN, warum das nicht die Fehlerklasse „zwei Orte" ist:
            // es ist EIN Ort, und er ist der, der den Kopf liest. Der
            // Zeichenweg (`kind`/`element`) zeichnet; dieser liest. Zwei
            // Aufgaben, zwei Orte, zwei Namen — das ist die Unterscheidung,
            // die in W5G („drei Zahlenraeume") gefehlt hat.
            if tag.eq_ignore_ascii_case("title") && self.titel.is_empty() {
                let mut text = String::new();
                for kind in children {
                    if let HtmlNode::Text(t) = kind {
                        text.push_str(t);
                    }
                }
                // M4: 200 Zeichen. `weissraum` macht aus einem Titel mit
                // Zeilenumbruechen eine Zeile — genau das macht ein
                // Browser auch.
                self.titel = weissraum(&text).chars().take(MAX_TITEL).collect();
            }
            if tag.eq_ignore_ascii_case("link") && self.favicon.is_none() {
                // `rel="icon"` und `rel="shortcut icon"` (1999). Alles
                // andere (`stylesheet`, `preload`, …) ist kein Icon.
                let rel = attributes
                    .get("rel")
                    .map(|r| r.to_ascii_lowercase())
                    .unwrap_or_default();
                if rel.split_whitespace().any(|r| r == "icon")
                    && let Some(href) = attributes
                        .get("href")
                        .map(|h| h.trim())
                        .filter(|h| !h.is_empty())
                {
                    // M4: die Adresse ist eine Eingabe (2048 wie `src`).
                    let href: String = href.chars().take(2048).collect();
                    self.favicon = Some(loese_link(&href, self.basis));
                }
            }
            // 03.10. 18:12 (Rolle HTML): ein `<style>` im `<noscript>` ist
            // **kein** Stylesheet — HTML 5, 13.3 macht den Inhalt bei
            // aktivem Scripting zu literalem Text. GEMESSEN: duckduckgo.com
            // war dadurch eine LEERE Seite (2 Knoten, 0 Texte), weil
            // `body { display: none }` aus diesem Block kam. Die
            // Begruendung und die Messung stehen an `stile_sammeln_in`.
            if tag.eq_ignore_ascii_case("style") && !im_noscript {
                let mut inhalt = String::new();
                for kind in children {
                    if let HtmlNode::Text(t) = kind {
                        if !inhalt.is_empty() && !inhalt.ends_with('\n') {
                            inhalt.push('\n');
                        }
                        inhalt.push_str(t);
                    }
                }
                if !inhalt.trim().is_empty() && self.stile.len() < MAX_STILBLOECKE {
                    let block = stile_aus(attributes, &inhalt);
                    if block.typ.is_empty() || block.typ == "text/css" {
                        self.stile.push(block);
                    }
                }
            }
            // 03.10.2026 17:0x: `<link rel="stylesheet">` (HTML 4.01
            // §12.3, CSSOM: ein Blatt, das nicht im Dokument steht).
            //
            // GEMESSENER BEFUND, warum das ueberhaupt noetig war: an den
            // zehn gespeicherten Seiten haben die Seiten mit `link`-Blaettern
            // die groessten Firefox-Abweichungen (github 191.9, nixos 145.3,
            // MDN 124.5) und die mit Inline-`<style>` die kleinsten
            // (duckduckgo 31.1) — `team/PROBLEME.txt` Nr. 59. GEMESSEN war
            // auch die Ursache: bis eben gab es fuer diese Seiten **null**
            // Autorblaetter, weil die Adresse schon beim Lesen weggeworfen
            // wurde. Es gibt hier KEIN Text zu lesen, deshalb wird hier nur
            // der Auftrag abgelegt (Adresse aufgeloest, Text leer).
            //
            // GEMESSENE ENTSCHEIDUNG, warum das ein eigener `if` und kein
            // Zweig im Icon-Zweig ist: der Icon-Zweig haengt an
            // `self.favicon.is_none()` (nur das ERSTE Icon zaehlt), und ein
            // Stylesheet ist kein Icon. Beide Bedingungen aneinander zu
            // haengen hiesse: die Seite hat ein Icon VOR dem ersten
            // Stylesheet, dann faellt das Stylesheet weg — und das faellt
            // im Bild nicht auf, weil die Seite dann nur hässlich aussieht.
            if tag.eq_ignore_ascii_case("link") {
                let rel = attributes
                    .get("rel")
                    .map(|r| r.to_ascii_lowercase())
                    .unwrap_or_default();
                // `rel` ist eine LISTE von Beziehungen (HTML 4.01 §12.3:
                // `rel="stylesheet alternate"`), nicht ein Wort.
                let ist_stylesheet = rel.split_whitespace().any(|r| r == "stylesheet");
                // `type` ist dieselbe Zusage wie beim `<style>`: alles
                // ausser `text/css` ist kein Stylesheet. `type="text/plain"`
                // mit `rel=stylesheet` steht wirklich im Netz.
                let typ = attributes
                    .get("type")
                    .map(|s| s.trim().to_ascii_lowercase())
                    .unwrap_or_default();
                let typ_passt = typ.is_empty() || typ == "text/css";
                if ist_stylesheet
                    && typ_passt
                    && self.stile.len() < MAX_STILBLOECKE
                    && let Some(href) = attributes
                        .get("href")
                        .map(|h| h.trim())
                        .filter(|h| !h.is_empty())
                {
                    // M4: die Adresse ist eine Eingabe (2048 wie `src` und
                    // wie das Icon oben) — und sie kommt aus dem Dokument.
                    let href: String = href.chars().take(2048).collect();
                    let url = loese_link(&href, self.basis);
                    self.stile.push(stil_aus_link(attributes, url));
                }
            }
            // Release 12.10.2026: der Inhalt eines `<template>` ist ein inaktives
            // Fragment (HTML §4.12.3) — seine `<style>`/`<link>` gelten nicht.
            // GEMESSEN vorher (Agent CSSOM, WPT css-cascade/scope-evaluation):
            // `<template><style>#x{color:green}</style></template>` faerbte #x,
            // alle Vorlagen-Stile einer Seite galten gleichzeitig.
            if tag.eq_ignore_ascii_case("template") {
                continue;
            }
            let ist_noscript = tag.eq_ignore_ascii_case("noscript");
            self.stile_sammeln_in(children, im_noscript || ist_noscript);
        }
    }

    /// 03.10.2026 17:0x: die geholten `<link rel="stylesheet">`-Texte in
    /// die Blattliste legen (siehe `Self::kaskade_einrichten`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Text ueber die AUFGELOESTE Adresse
    /// gesucht wird und nicht ueber die geschriebene: `<link href="a.css">`
    /// heisst auf `https://x/y/z` `https://x/y/a.css`. Wer nach `a.css`
    /// sucht, findet es nicht — und die Seite waere ohne Blatt, obwohl der
    /// Abrufer es geliefert hat. Die Aufloesung macht `loese_link` an EINER
    /// Stelle (siehe den `link`-Zweig), also kann es hier keine
    /// zweite geben.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Block nur EINEN Text bekommt: das
    /// sind die Paare des Abrufers, und sie sind nach Adresse aufgebaut. Ein
    /// zweites Paar mit derselben Adresse (ein 304 mit neuem Text) kommt
    /// einfach nicht vor.
    ///
    /// Der Text wird an der M4-Grenze `MAX_STILTEXT` gekuerzt wie beim
    /// `<style>`-Block — sonst koennte ein Blatt die Kaskade sprengen, das
    /// der Pfad abhaengig von der Groesse hereinkommt.
    fn blatt_inhalt_setzen(&mut self, blaetter: &[(String, String)]) {
        if blaetter.is_empty() {
            return;
        }
        for (url, inhalt) in blaetter {
            let mut inhalt = inhalt.clone();
            if inhalt.len() > MAX_STILTEXT {
                let g = crate::zeichengrenze(&inhalt, MAX_STILTEXT);
                inhalt.truncate(g);
            }
            for block in self.stile.iter_mut() {
                if block.href.as_deref() == Some(url.as_str()) && block.inhalt.is_empty() {
                    block.inhalt = inhalt.clone();
                }
            }
        }
    }

    /// I2: CSS-Baum und Blaetter aufbauen.
    fn kaskade_einrichten(&mut self, baum: &CssBaum) {
        // W6 (J9): die Adressen der Knoten merken, damit `kind()` den
        // CSS-Index findet (siehe `Bauer::css_adresse`).
        for (i, k) in baum.knoten.iter().enumerate() {
            self.css_adresse.insert(k.adresse, i as u32);
        }
        let kosmetik = self.kosmetik.take();
        // Release 12.10.2026: Link-Bloecke aus dem Blattschrank (der Link
        // bleibt im Baum ein Link, siehe `blattschrank`).
        for block in self.stile.iter_mut() {
            if block.inhalt.is_empty()
                && let Some(url) = block.href.as_deref()
                && let Some(t) = crate::blattschrank::inhalt(url)
            {
                let mut inhalt = t.to_string();
                if inhalt.len() > MAX_STILTEXT {
                    let g = crate::zeichengrenze(&inhalt, MAX_STILTEXT);
                    inhalt.truncate(g);
                }
                block.inhalt = inhalt;
            }
        }
        let satz = blatt_satz(&self.stile, kosmetik);
        self.kaskade = Some(Kaskade {
            baum: baum.clone(),
            alle: satz.alle,
            aktuell: u32::MAX,
            werte: Vec::new(),
            // 05.10.2026: DIE ECHTE ANSICHTSHOEHE, nicht mehr die 0. Siehe
            // das Feld `Bauer::ansicht_hoehe`: vorher stand hier `0`, und
            // `kaskade.rs` rechnete darum mit einer geratenen 600 px.
            ansicht_hoehe: self.ansicht_hoehe,
            vormerker_author: satz.vormerker_author,
            vormerker_alle: satz.vormerker_alle,
            pseudo_regeln: satz.pseudo_regeln,
        });
    }
}

/// Release 10.10.2026: die Blaetter einer Seite mit ihren Indizes — GETEILT
/// zwischen den Neubauten, solange sich weder die Stilbloecke noch das
/// kosmetische Blatt aendern.
///
/// GEMESSEN vorher (youtube.com, 12 Blaetter / 3,46 MB / 19 209 Regeln): jeder
/// Neubau kopierte alle geparsten Blaetter zweimal (`alle`, `author`) und
/// baute beide Vormerker neu — 180-200 ms fuer eine Seite mit 55 Knoten, die
/// Kaskade selbst brauchte davon 5 ms. Jede DOM-Aenderung aus JS bezahlte das.
#[derive(Clone)]
struct BlattSatz {
    alle: std::sync::Arc<Vec<narsil_css::Stylesheet>>,
    vormerker_author: std::sync::Arc<narsil_css::Vormerker>,
    vormerker_alle: std::sync::Arc<narsil_css::Vormerker>,
    pseudo_regeln: std::sync::Arc<kaskade::PseudoSatz>,
}

/// Hoechstens so viele Blattsaetze bleiben gemerkt (der aktuelle Tab und
/// ein Wechsel); ein Satz der grossen Seiten belegt einige zehn MB.
const MAX_BLATT_SAETZE: usize = 2;

static BLATT_SAETZE: std::sync::Mutex<Vec<(u64, BlattSatz)>> = std::sync::Mutex::new(Vec::new());

fn blatt_satz(stile: &[StyleBlock], kosmetik: Option<narsil_css::Stylesheet>) -> BlattSatz {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for b in stile {
        b.typ.hash(&mut h);
        b.medium.hash(&mut h);
        b.inhalt.hash(&mut h);
        b.bereich.hash(&mut h);
    }
    // Das kosmetische Blatt kommt fertig geparst; sein Fingerabdruck ist
    // seine Debug-Form (ohne Zwischenstring in den Hasher geschrieben).
    struct InHasher<'a>(&'a mut std::collections::hash_map::DefaultHasher);
    impl std::fmt::Write for InHasher<'_> {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.write(s.as_bytes());
            Ok(())
        }
    }
    if let Some(k) = &kosmetik {
        let _ = std::fmt::Write::write_fmt(&mut InHasher(&mut h), format_args!("{k:?}"));
    }
    let schluessel = h.finish();
    if let Ok(merk) = BLATT_SAETZE.lock()
        && let Some((_, s)) = merk.iter().find(|(k, _)| *k == schluessel)
    {
        return s.clone();
    }
    let satz = blatt_satz_bauen(stile, kosmetik);
    if let Ok(mut merk) = BLATT_SAETZE.lock() {
        merk.retain(|(k, _)| *k != schluessel);
        if merk.len() >= MAX_BLATT_SAETZE {
            merk.remove(0);
        }
        merk.push((schluessel, satz.clone()));
    }
    satz
}

fn blatt_satz_bauen(stile: &[StyleBlock], kosmetik: Option<narsil_css::Stylesheet>) -> BlattSatz {
    {
        let mut alle = blaetter(stile);
        // A-ADBLOCK (01.10.2026, 21:45): das kosmetische Blatt kommt
        // HINTER alle Seiten-Blaetter. Jeder seiner Selektoren traegt
        // `display: none !important`, und die Kaskade gibt `!important`
        // den Vorrang vor jeder normalen Angabe (CSS 2.1 §6.4.2) — die
        // Reihenfolge ist damit nicht entscheidend, aber sie macht die
        // Sache eindeutig.
        //
        // GEMESSENE KOSTEN: EIN Platz von `MAX_BLAETTER` (16). Der
        // Platz wird nur belegt, wenn es kosmetische Regeln fuer den
        // Host gibt; ohne Filter bleibt die Liste unveraendert.
        if let Some(k) = kosmetik {
            if alle.len() < narsil_css::MAX_BLAETTER {
                alle.push(k);
            }
        }
        // Ohne UA: nur die Bloecke der Seite. Leer, wenn die Seite keine
        // hat — dann ist `author` leer, und `berechnen` liefert fuer
        // jedes Element den Standard. Das kostet einen Aufruf je
        // Element und ist die Antwort auf "hat der Autor geschrieben?".
        let author: &[narsil_css::Stylesheet] = alle.get(1..).unwrap_or(&[]);
        // P7 (02.10.2026): DIE ZWEI INDIZES, EINMAL je Seite.
        //
        // GEMESSENE VORHER/NACHHER-ZAHLEN und die Reihenfolge hier ist
        // wichtig: `author` ist aus `alle` abgeleitet, also wird der
        // Autor-Index VOR dem Verschieben gebaut. Sonst baute der zweite
        // Index die Liste, die der erste schon weggeworfen hat.
        let vormerker_author = narsil_css::Vormerker::bauen(author);
        let vormerker_alle = narsil_css::Vormerker::bauen(&alle);
        // PSEUDOELEMENTE (06.10.2026): die `::before`/`::after`-Regeln
        // einmal je Seite einsammeln (78 Vorkommen auf wikipedia) —
        // nicht je Element.
        let pseudo_regeln = kaskade::pseudo_regeln(author);
        BlattSatz {
            alle: std::sync::Arc::new(alle),
            vormerker_author: std::sync::Arc::new(vormerker_author),
            vormerker_alle: std::sync::Arc::new(vormerker_alle),
            pseudo_regeln: std::sync::Arc::new(kaskade::PseudoSatz::bauen(pseudo_regeln)),
        }
    }
}

impl<'a> Bauer<'a> {

    /// I2: der Stil EINES Elements aus der Kaskade, oder `None`, wenn
    /// es nicht gezeichnet werden darf (`display: none`).
    ///
    /// `eltern` ist das Ergebnis des Elternelements (oben im Stapel).
    /// Der eigene Wert wird hier abgelegt, damit die Kinder ihn als
    /// ihren ELTERNwert finden — und in `kind()` wieder vom Stapel
    /// genommen, damit er nicht zum naechsten Geschwister leakt.
    /// Release 08.10.2026: klont einen SVG-Teilbaum und schreibt den
    /// CSS-Stil jedes Elements als Präsentationsattribute hinein (der
    /// SVG-Renderer kennt kein CSS). M3: höchstens `MAX_SVG_CSS` Elemente je
    /// Bild — eine Grafik mit tausenden Pfaden bekommt ihre Attribute, aber
    /// keine Kaskade je Pfad.
    /// Release 15.10.2026 (Agent F): the `<clipPath>` a `url(#id)` names,
    /// with the elements it uses, as a standalone SVG document.
    fn clip_svg_dokument(&self, wert: &str, eltern: &narsil_css::Berechnet) -> Option<std::sync::Arc<str>> {
        let innen = wert.trim().strip_prefix("url(").or_else(|| wert.trim().strip_prefix("URL("))?;
        let innen = innen.split(')').next()?.trim().trim_matches(|c| c == '"' || c == '\'');
        // Only a same-document reference names a clip path of this page
        // (WPT css-masking reference-nonexisting-existing-local).
        let (dokument, id) = innen.rsplit_once('#')?;
        if !dokument.is_empty() && crate::loese_link(dokument, self.basis) != self.basis.split('#').next().unwrap_or(self.basis) {
            return None;
        }
        self.svg_dokument_fuer(id, "clippath", eltern)
    }

    /// Release 15.10.2026 (Agent F): the `<filter>` elements of the page a
    /// `filter` value names, in one SVG document.
    fn filter_svg_document(&self, value: &str, computed: &narsil_css::Berechnet) -> Option<std::sync::Arc<str>> {
        let mut filter = Vec::new();
        let mut data_document: Option<String> = None;
        let mut rest = value;
        while let Some(i) = rest.to_ascii_lowercase().find("url(") {
            let inner = &rest[i + 4..];
            let end = inner.find(')').unwrap_or(inner.len());
            let target = inner[..end].trim().trim_matches(|c| c == '"' || c == '\'');
            rest = &inner[end.min(inner.len())..];
            let Some((document, id)) = target.rsplit_once('#') else { continue };
            // A `data:` SVG document holds its own filters (WPT
            // css/filter-effects blur-text).
            if document.trim_start().get(..5).is_some_and(|p| p.eq_ignore_ascii_case("data:")) {
                if let Some((_, bytes)) = narsil_svg::image::parse_data_url(document.trim())
                    && let Ok(text) = String::from_utf8(bytes)
                {
                    data_document = Some(text);
                }
                continue;
            }
            if !document.is_empty() && crate::loese_link(document, self.basis) != self.basis.split('#').next().unwrap_or(self.basis) {
                continue;
            }
            if let Some(node) = self.svg_definitionen.get(id).copied()
                && matches!(node, HtmlNode::Element { tag, .. } if tag.eq_ignore_ascii_case("filter"))
                && filter.len() < 8
            {
                filter.extend(self.svg_mit_css(std::slice::from_ref(node), computed, 0));
            }
        }
        if filter.is_empty() {
            return data_document.map(|t| std::sync::Arc::from(t.as_str()));
        }
        let mut attr = HashMap::new();
        attr.insert("xmlns".to_string(), "http://www.w3.org/2000/svg".to_string());
        let text = narsil_fmt_html::serialisiere(&[HtmlNode::Element { tag: "svg".into(), attributes: attr, children: filter, shadow_root: None }]);
        Some(std::sync::Arc::from(text.as_str()))
    }

    /// Release 15.10.2026 (Agent F): `url(#id)` mask layers become the
    /// page's `<mask>` element as an SVG document; a missing one is a
    /// transparent layer (it masks everything away, CSS Masking 1 §7.1).
    fn masken_aufloesen(&self, ebenen: Vec<MaskenEbene>) -> Vec<MaskenEbene> {
        let standard = narsil_css::Berechnet::standard();
        ebenen
            .into_iter()
            .map(|e| match e {
                MaskenEbene::Verweis(id) => match self.svg_dokument_fuer(&id, "mask", &standard) {
                    Some(quelle) => MaskenEbene::Svg { quelle, verweis: format!("#{id}") },
                    None => MaskenEbene::Verweis(id),
                },
                andere => andere,
            })
            .collect()
    }

    /// Release 15.10.2026 (Agent F): the SVG element `id` of the page (when
    /// its tag is `tag`) with the elements it uses, as a standalone SVG
    /// document.
    fn svg_dokument_fuer(&self, id: &str, tag_erwartet: &str, eltern: &narsil_css::Berechnet) -> Option<std::sync::Arc<str>> {
        let knoten = *self.svg_definitionen.get(id)?;
        let HtmlNode::Element { tag, .. } = knoten else { return None };
        if !tag.eq_ignore_ascii_case(tag_erwartet) {
            return None;
        }
        let mut kinder = self.svg_mit_css(std::slice::from_ref(knoten), eltern, 0);
        let defs: Vec<HtmlNode> = svg_definitionen_fuer(std::slice::from_ref(knoten), &self.svg_definitionen)
            .iter()
            .flat_map(|d| self.svg_mit_css(std::slice::from_ref(*d), eltern, 0))
            .collect();
        if !defs.is_empty() {
            kinder.insert(0, HtmlNode::Element { tag: "defs".into(), attributes: HashMap::new(), children: defs, shadow_root: None });
        }
        let mut attr = HashMap::new();
        attr.insert("xmlns".to_string(), "http://www.w3.org/2000/svg".to_string());
        let text = narsil_fmt_html::serialisiere(&[HtmlNode::Element { tag: "svg".into(), attributes: attr, children: kinder, shadow_root: None }]);
        Some(std::sync::Arc::from(text.as_str()))
    }

    fn svg_mit_css(&self, knoten: &[HtmlNode], eltern: &narsil_css::Berechnet, tiefe: usize) -> Vec<HtmlNode> {
        const MAX_SVG_CSS: usize = 400;
        fn zaehle_elemente(k: &[HtmlNode], tiefe: usize) -> usize {
            if tiefe > 64 {
                return 0;
            }
            k.iter()
                .map(|n| match n {
                    HtmlNode::Element { children, .. } => 1 + zaehle_elemente(children, tiefe + 1),
                    _ => 0,
                })
                .sum()
        }
        if tiefe > 64 || (tiefe == 0 && zaehle_elemente(knoten, 0) > MAX_SVG_CSS) {
            return knoten.to_vec();
        }
        let Some(k) = self.kaskade.as_ref() else {
            return knoten.to_vec();
        };
        knoten
            .iter()
            .map(|n| match n {
                HtmlNode::Element { tag, attributes, children, shadow_root } => {
                    let mut attr = attributes.clone();
                    let stil = k.baum.index_von(n).map(|id| {
                        kaskade::end_stil_mit_vormerker(
                            &k.baum,
                            id,
                            &k.alle,
                            &k.vormerker_alle,
                            Some(eltern),
                            self.breite,
                            k.ansicht_hoehe,
                        )
                    });
                    svg_resolve_attribute_vars(&mut attr, &stil.as_ref().unwrap_or(eltern).variablen);
                    if let Some(st) = &stil {
                        svg_css_attribute(&mut attr, st);
                    }
                    let fuer_kinder = stil.as_ref().unwrap_or(eltern);
                    HtmlNode::Element {
                        tag: tag.clone(),
                        attributes: attr,
                        children: self.svg_mit_css(children, fuer_kinder, tiefe + 1),
                        shadow_root: shadow_root.clone(),
                    }
                }
                anders => anders.clone(),
            })
            .collect()
    }

    fn stil_von_kaskade(
        &mut self,
        knoten: &HtmlNode,
        tag: &str,
        attributes: &HashMap<String, String>,
        ist_link: bool,
    ) -> Option<Style> {
        // PSEUDOELEMENTE (06.10.2026): die zwei Felder gehoeren DIESEM
        // Element. Sie werden ZUERST geloescht — vor jedem fruehen
        // `return` —, sonst traegt das naechste Element die Kugel eines
        // `display:none`-Vorfahren (dieselbe Fehlerklasse wie beim
        // `feld_rahmen_ua`, siehe den Kommentar unten).
        self.pseudo_vorher = None;
        self.pseudo_nachher = None;
        self.platzhalter_farbe = None;
        // Release 11.10.2026: die Inhaltssprache fuer die Umbruchregeln.
        if let Some(l) = attributes.get("lang").or_else(|| attributes.get("xml:lang"))
            && self.sprachen.len() < 256
        {
            self.sprachen.push((self.tiefe, schreibt_chinesisch_oder_japanisch(l)));
        }
        // `ist_link` wird DURCHGEREICHT, nicht hier entschieden: ein
        // `<a>` ohne CSS soll blau bleiben. GEMESSEN — im ersten
        // Entwurf stand hier `false`, und jeder Link auf jeder Seite
        // wurde schwarz.
        //
        // DER INLINE-DELTA (fett, kursiv, Linkfarbe) gehoert HIER her
        // und nicht in `element()`: GEMESSENER FEHLER 30.09.2026
        // (`test2.html`), `nav a { margin-right: 12px }` wirkte nicht und
        // `nav a { color: var(--akzent) }` ebensowenig. Der Grund war,
        // dass `element()` bei Inline-Elementen frueh zurueckkehrt und den
        // von hier berechneten Stil nie abholt.
        // INLINE-ELEMENTE: ihre Hand-Typografie erbt vom Stack, nicht
        // vom Nichts. GEMESSENER FEHLER 30.09.2026: `stil_fuer("b")`
        // baut einen Stil aus dem Nichts (16 px), und `<b>` in einem
        // 24-px-`<h1>` war damit wieder 16 px — der Fehler, den
        // `inline_behaelt_ueberschrift_grad` seit Welle 4 faengt.
        let hand = if ist_inline(tag) {
            let basis = self
                .marken
                .last()
                .map(|m| m.stil.clone())
                .or_else(|| self.laufender.as_ref().map(|a| a.stil.clone()))
                .unwrap_or(Style {
                    deckkraft: 1.0,
                    radius: [0.0; 4],
                    schatten_anzahl: 0,
                    dekoration: 0,
                    color: FARBE_TEXT,
                    font_size: GRAD_TEXT,
                    ..Style::default()
                });
            self.inline_stil(basis, tag, attributes)
        } else {
            self.stil_fuer(tag, attributes, ist_link)
        };
        let breite = self.breite;
        // HTML_1 (04.10.2026): DIE ZWEI FRUEHABBRUECHE LEEREN DAS UA-FELD.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier und nicht erst im Aufrufer:
        // `feld_rahmen_ua` waere sonst der Rahmen des **VORLETZTEN**
        // Elements, und ein `<input>` ohne Kaskade erbte ihn. Ein Feld,
        // das nicht gesetzt wurde, ist genau die Aussage „die UA-Ebene sagt
        // nichts" — und das ist in beiden Faellen wahr.
        self.feld_rahmen_ua = None;
        let Some(k) = self.kaskade.as_mut() else {
            // Klartext, `.narsil` oder Tiefenschranke: die Hand-Typografie
            // gilt unveraendert. Das ist ein gültiger Zustand, kein
            // Fehler — ohne `<style>` gibt es nichts zu rechnen.
            self.plan_stapel.push(PlanEintrag::default());
            return Some(hand);
        };
        let id = match k.baum.index_von(knoten) {
            Some(id) => id,
            // Der Knoten steht nicht im CSS-Baum (z. B. unterhalb von
            // `kaskade::MAX_TIEFE`). Dann gilt die Hand-Typografie.
            None => {
                self.plan_stapel.push(PlanEintrag::default());
                return Some(hand);
            }
        };
        let eltern = k.werte.last().cloned();
        let hoehe = k.ansicht_hoehe;
        // P7 (02.10.2026): MIT VORMERKER. GEMESSEN auf github.com: 4,370 ms
        // je Kasten gegenueber 0,149 ms auf nodejs.org — 29x bei 1,5x so
        // grossem DOM und 33x so viel CSS. Die Zeit folgte der
        // CSS-Groesse, weil jede der ~7000 Regeln an jeden der 1470
        // Knoten ging. Die Indizes stehen in `Kaskade` und werden EINMAL
        // je Seite gebaut (siehe das Feld).
        //
        // GEMESSENE GRENZE, die gleich mitkommt: `Vormerker::bauen`
        // laeuft ueber `MAX_BLAETTER` (16) und `MAX_REGELN` (4096 je
        // Blatt). github.com liefert 44 Stylesheets und 7,3 MB. Alles
        // dahinter ist fuer den Vormerker unsichtbar und landet im
        // `rest`-Teil, der immer geprueft wird — der Index ist also nie
        // falsch, nur unvollstaendig. Nach der 16. Datei gibt es keinen
        // Geschwindigkeitsgewinn mehr, aber auch keinen Verlust.
        //
        // KOMPRESS D1 (02.10.2026, Folgemessung): DIE BEIDEN KASKADEN
        // GETRENNT VOM REST DES BAUERS messen.
        //
        // GEMESSENER GRUND: nach der Indexreparatur steht der echte
        // Weg bei 700 ms (Release), der isolierte Kaskaden-Pruefstand
        // bei 53,8 ms fuer 1801 Knoten. Hochgerechnet auf die 8730
        // Aufrufe sind das 261 ms — es fehlen rund 440 ms, und die
        // sind bisher NIEMANDEM zugeordnet.
        //
        // Zwei Vermutungen habe ich schon geprueft und VERWORFEN: der
        // Build-Profiltyp (Release 754 ms gegen Debug 884 ms) und das
        // Durchlaufen der Regeln (70,6 ms gemessen, 10 %).
        //
        // GEMESSENE ENTSCHEIDUNG, warum es ZWEI Summen sind: `autor`
        // rechnet ueber `author` (ohne UA-Blatt) und `mit_ua` ueber
        // `alle` (mit). Das sind zwei Regelmengen ueber dieselbe
        // Seite; sie zu einer Zahl zu addieren verwaessert genau die
        // Frage, die ich stellen muss.
        // Release 10.10.2026: einmal gelesen (je Element war es ein
        // Umgebungszugriff, siehe `kaskade::kaskade_zeiten_an`).
        let d1_k = kaskade::kaskade_zeiten_an();
        let d1_ka = d1_k.then(std::time::Instant::now);
        let autor = kaskade::autor_stil_mit_vormerker(
            &k.baum,
            id,
            k.alle.get(1..).unwrap_or(&[]),
            &k.vormerker_author,
            eltern.as_ref(),
            breite,
            hoehe,
        );
        let mut mit_ua = kaskade::end_stil_mit_vormerker(
            &k.baum,
            id,
            &k.alle,
            &k.vormerker_alle,
            eltern.as_ref(),
            breite,
            hoehe,
        );
        // Release 13.10.2026 (Agent CSS2): hinter dem Klemmpunkt einer
        // Zeilenklemme wird ein Element nicht gesetzt (`continue: collapse`,
        // CSS Overflow 4 §4.2) — ausser es ist absolut oder fest positioniert
        // (sein enthaltender Block liegt vor dem Klemmpunkt oder umfasst ihn).
        // GEMESSEN vorher (WPT `line-clamp/webkit-line-clamp-abspos-001`): der
        // absolute Kasten hinter dem Klemmpunkt fehlte.
        if self.klemmen.last().is_some_and(|k| k.rest == 0)
            && mit_ua.anzeige != narsil_css::Anzeige::Keins
            && !matches!(mit_ua.position, narsil_css::Position::Absolut | narsil_css::Position::Fest)
            && !attributes.contains_key(" narsil-modal")
            && !attributes.contains_key(" narsil-popover-open")
            && !matches!(tag.to_ascii_lowercase().as_str(), "script" | "style" | "template" | "link" | "meta")
        {
            // Nur sichtbarer Inhalt dahinter verlangt die Auslassung — ein
            // leeres Inline-Element ist eine Phantomzeile (WPT `line-clamp-037`).
            fn hat_inhalt(k: &HtmlNode, t: usize) -> bool {
                t < 64
                    && match k {
                        HtmlNode::Text(x) => !x.chars().all(css_leerraum),
                        HtmlNode::Element { tag, children, .. } => {
                            ist_ersetzt(tag) || children.iter().any(|c| hat_inhalt(c, t + 1))
                        }
                    }
            }
            // (wie `klemme_auslassen`, mit Feld-Ausleihen: `self.kaskade` ist
            // hier geliehen)
            if hat_inhalt(knoten, 0)
                && let Some(kl) = self.klemmen.last_mut()
                && !kl.ellipse
            {
                kl.ellipse = true;
                if let Some(tid) = kl.letzter_text {
                    // A node created here would go with the hidden element
                    // (`display: none` below), so the text gets the "…";
                    // a re-break writes it as a run of its own (WPT css-
                    // overflow `line-clamp-auto-044`).
                    append_clamp_ellipsis(&mut self.doc, &mut self.absaetze, tid, kl.prepend, false);
                }
            }
            mit_ua.anzeige = narsil_css::Anzeige::Keins;
        }
        // Release 08.10.2026: ein `<button>` mit Elementen ist ein Kasten mit
        // Kindern (siehe `knopf_mit_elementen`) — `display: inline` gilt
        // dann als `inline-block` (HTML §15.5.2: ein Knopf ist ein
        // „inline-block"-artiges Element).
        if tag.eq_ignore_ascii_case("button")
            && mit_ua.anzeige == narsil_css::Anzeige::Inline
            && matches!(knoten, HtmlNode::Element { children, .. } if children.iter().any(|c| {
                matches!(c, HtmlNode::Element { tag, .. } if !tag.eq_ignore_ascii_case("br"))
            }))
        {
            mit_ua.anzeige = narsil_css::Anzeige::InlineBlock;
        }
        // Release 11.10.2026: im Top Layer (offener modaler Dialog, offenes
        // Popover — die internen Zustandsattribute aus `narsil-fmt-html`)
        // wird `static`/`relative` zu `absolute` (CSS Position 4 §2.1).
        // GEMESSEN vorher (WPT `top-layer-position-static`): der Dialog
        // blieb im Fluss.
        if (attributes.contains_key(" narsil-modal") || attributes.contains_key(" narsil-popover-open"))
            && matches!(mit_ua.position, narsil_css::Position::Statisch | narsil_css::Position::Relativ)
        {
            mit_ua.position = narsil_css::Position::Absolut;
        }
        // HTML_1 (04.10.2026): DER UA-RAHMEN FUER FORMULARFELDER.
        //
        // GEMESSENER BEFUND (der Fehler in `feld_von_kaskade`): `k.werte`
        // traegt `autor` — die Autorenebene **ohne** UA-Blatt. Das UA-Blatt
        // steht nur hier, in `mit_ua`, und `mit_ua` war vorher eine
        // LOKALE Variable: `input, textarea { border: 1px solid #8f8f9d }`
        // (`narsil-css/src/ua.rs:216`, GEMESSEN an Firefox 157) kam an
        // **keinem** Formularfeld an. GEMESSEN: `padding = 0/0/0/0` an
        // `<input type=text>`, `<textarea>` und beiden docs.rs-Suchfeldern.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die vier Tags und nicht ein Merkmal
        // des Ergebnisses: `element_innen` entscheidet an genau dieser
        // Stelle (`matches!(klein.as_str(), "input" | "button" | "select" |
        // "textarea")`, `feld_merken`), und dieselbe Liste hier zu
        // schreiben haette zwei Orte fuer eine Tatsache.
        if ["input", "button", "select", "textarea"]
            .iter()
            .any(|t| tag.eq_ignore_ascii_case(t))
        {
            self.feld_rahmen_ua = Some(FeldRahmen::von(&mit_ua));
        }
        if let Some(a) = d1_ka {
            let ms = a.elapsed().as_secs_f64() * 1000.0;
            // `Relaxed` reicht: das ist eine Messung, keine Zaehlung,
            // auf die sich etwas verlässt. Zwei Zellen, die aneinander
            // haengen, kosten mehr als der Nutzen.
            //
            // GEMESSENER FEHLER MEINER ERSTEN FASSUNG, gefunden an der
            // Ausgabe "0,0 ms in 16005 Aufrufen": ich habe die
            // BITMUSTER der `f64` addiert. `fetch_add` auf Bitmuster
            // addiert keine Werte — aus einer Addition wird eine
            // andere. Mit EINEM Aufruf faellt es nicht auf (0 + Muster
            // = Muster), ab dem zweiten schon.
            //
            // GEMESSENE ENTSCHEIDUNG: Nanosekunden als Ganzzahl. Dann
            // stimmt die Addition, und der Getter teilt durch 1e6.
            // Ganze Zahlen sind hier korrekt — `f64` kann 1e6
            // Nanosekunden (also 1 ms) ueber eine Addition von
            // 16005 Werten nicht genau darstellen, eine `u64` schon.
            let ns = (ms * 1e6) as u64;
            KASKADE_ZEIT.fetch_add(ns, std::sync::atomic::Ordering::Relaxed);
            KASKADE_ANZ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        //
        // DER DRITTE AUFRUF, und er ist der Unterschied zwischen "der
        // Autor hat es geschrieben" und "der Autor hat es geerbt".
        //
        // GEMESSENER RUECKSCHRITT 30.09.2026, 05:58 (Bildschirmfoto
        // `test2.html`): `body { color: #ddd }` und ein
        // `<a href="/weiter.html">Link</a>` darin. Der Link wurde
        // #ddd statt blau — also erbte er die Autorfarbe und schlug
        // damit die Hand-Typografie. Im Browser passiert das nicht:
        // `a:link` aus dem UA-Blatt hat Spezifitaet (0,1,1) und schlaegt
        // einen geerbten Wert, denn ein VERERBTER Wert verliert gegen
        // JEDE gesetzte Angabe.
        //
        // Ohne UA-Blatt gerechnet sieht man diesen Unterschied nicht:
        // fuer `body { color }` ist der Autorwert == Standard, und der
        // Link bekaeme die Handfarbe zurueck. Der Test braucht also
        // den dritten Aufruf OHNE Elternwert: der gibt an, was der Autor
        // am ELEMENT selbst geschrieben hat.
        //
        // P6 (02.10.2026): DIE KASKADE ENTSCHEIDET DIE GROESSE EINES
        // ERSETZTEN ELEMENTS.
        //
        // GEMESSENER FEHLER: `stil_von_css` las vor dieser Zeile
        // `schriftgroesse_px`, `marge_*`, `fuellung_*` — aber NIE
        // `breite`/`hoehe`. Das heisst: eine Angabe `width: 300px` aus
        // einem Stylesheet kam im Layout nicht an. Zwei Klassen von
        // Seiten waren davon betroffen, und die zweite ist der gemeldete
        // Fall:
        //
        //  * `<img width="7129" ...>` auf nixos.org: der Attributwert
        //    stand in `node.style` und blieb stehen, weil die Seite
        //    `.w-auto{width:auto}` schreibt — und `auto` liest nicht zu
        //    einer Zahl, also wurde nichts ueberschrieben.
        //  * Jede Seite mit `div { width: 400px }`.
        //
        // WARUM NUR BEI ERSETZTEN ELEMENTEN: bei allen anderen Elementen
        // waere `None` (auto) kein "nichts gesagt", sondern "auto" — und
        // `auto` auf einen Block zu schreiben heisst, ihm die Fuellung
        // seines Eltern zu nehmen. Das waer eine Aenderung an vielen
        // Seiten ohne Auftrag. Fuer ersetzte Elemente ist `auto` genau
        // richtig, weil CSS 2.1, 10.3.2 fuer sie sagt, was `auto` bedeutet:
        // die intrinsische Masse.
        //
        // Das ist die Additivitaets-Grenze dieses Punktes: eine Breite aus
        // dem Stylesheet gilt erst fuer `<img>`-artige Elemente. Fuer den
        // Rest gehoert es in `stil_von_css` und damit in `narsil-css`.
        k.aktuell = id;
        //
        // DER DRITTE AUFRUF (ohne Elternwert) ist entfernt: die Frage
        // "hat der Autor das an DIESEM Element geschrieben?"
        // beantwortet der ELTERNWERT in `waehle_zahl` (Stufe 1). Der
        // Aufruf ohne Elternwert sah nach der richtigen Antwort aus,
        // kann aber keinen Nachfahr-Selektor aufloesen — `nav a
        // { margin-right: 12px }` passte damit nie (gemessen 06:14).
        //
        //
        // W7C R3 (02.10. 02:20): DER LAYOUT-STIL AUF DEN STAPEL.
        //
        // `autor` ist genau das, was der Autor an DIESEM Element
        // geschrieben hat, und `narsil_css::layout_stil` ist der EINZIGE
        // Ort, der daraus `display:flex/grid`, `position` und `float`
        // macht (Rolle LAYOUT, `WUNSCH_LAYOUT.txt` PUNKT 2). Hier ist der
        // richtige Ort, weil `autor` hier berechnet wird und nirgends
        // sonst.
        //
        // GEMESSEN, warum das der Stapel und nicht ein Feld ist: die
        // Kind-Knoten werden erst in `element()` gebaut, also danach. Ein
        // Feld traegt dann den Stil des Kindes, und der Abschluss von
        // `kind()` wuerde den Stil des Kindes unter das des Eltern legen.
        //
        // HTML_3 (04.10.2026): DER PLAN AUS `mit_ua`, nicht aus `autor`.
        //
        // GEMESSENER BEFUND VORHER (minimale Tabelle, 600 px, Weg
        // `seite_aus_text` -> `layout_mit_plan`, Test
        // `tests/tabelle_rolle_aus_ua.rs`):
        //
        //     MESSUNG tabelle rollen: [(Block, 10)]
        //
        // **NULL Tabellenrollen.** `flow.rs:1013` sieht also nie eine
        // Tabelle. GEMESSENE URSACHE: die drei `display`-Regeln stehen
        // ausschliesslich im UA-Blatt (`narsil-css/src/ua.rs:66-69`), und eine
        // Webseite schreibt sie nie selbst.
        //
        // GEMESSENE URSACHE DER EBENE: `autor` rechnet ueber `k.author` (ohne
        // UA-Blatt), `mit_ua` ueber `k.alle` (mit). Dieselbe Ursache wie der
        // Formularrahmen (HTML_1) und der Innenabstand (HTML_2) — dort war
        // es `feld_von_kaskade`, hier ist es diese eine Zeile.
        //
        // GEMESSENE BREITENANALYSE, warum das **zwei** Felder bewegt und
        // nicht mehr: `layout_stil` liest 54 `Berechnet`-Felder (mit grep
        // gezaehlt, `narsil-css/src/layout_plan.rs`), und das UA-Blatt setzt
        // davon genau **zwei**:
        //
        // | Feld in `layout_stil` | UA-Regel                 | Wirkung |
        // |------------------------|--------------------------|---------|
        // | `anzeige`              | `display`                | die Tabelle |
        // | `schriftgrad`          | `font-size`              | Font-Aufstieg |
        //
        // **NULL** der 52 uebrigen: `ua.rs` enthaelt nach eigener Zaehlung
        // keine `width`/`height`, kein `min-`/`max-`, kein `position`, kein
        // `float`, kein `gap`, kein `display:flex|grid`, kein `overflow`
        // (`grep -c` ueber die ganze Datei: **0**). `raster_*`, `flex_*`,
        // `luecke_*`, `oben/rechts/unten/links*` bleiben also, was der
        // Autor geschrieben hat.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der AUTOR-VORRANG bleibt: `mit_ua`
        // rechnet ueber `k.alle`, und der Autor schlaegt das UA-Blatt
        // (CSS 2.1, 9.7). GEMESSEN als Test:
        // `table/tr/td { display: block }` ergeben wieder `Block`
        // (`der_autor_schlaegt_das_ua_blatt`).
        //
        // GEMESSENE ENTSCHEIDUNG, warum `<li>` keine Baustelle wird:
        // `ua.rs:41` traegt `li { display: list-item }`, und
        // `narsil-css/src/layout_plan.rs:776` bildet `Anzeige::ListenEintrag` auf
        // `Z::Block` ab — dieselbe Rolle wie der Anfangswert. GEMESSEN auf
        // `<ul><li>eins</li><li>zwei</li>`: 4 `Block`, **keine** andere
        // Rolle (Test `li_bleibt_block_im_plan`). Die 391 Referenzknoten
        // (`ua.rs:120-128`) bleiben damit, was sie waren.
        //
        // `layout_grenzen` / `layout_grenzen_prozent` (zwei Zeilen weiter)
        // lesen **weiterhin** `autor`. GEMESSENE EINORDNUNG, warum das
        // keine Inkonsistenz ist: sie lesen `min_breite`, `max_breite`,
        // `min_hoehe`, `max_hoehe`, und `ua.rs` setzt **keine** davon
        // (`grep -c 'min-width\|max-width\|min-height\|max-height' = 0`).
        // Sie umzustellen waere eine dritte Aenderung ohne Wirkung — sie
        // waere nur eine Zeile mehr, die man falsch lesen kann.
        let mut plan_stil = narsil_css::layout_stil(&mit_ua);
        // Release 09.10.2026: `overflow` von `html` und `body` gilt dem
        // ANSICHTSFENSTER (CSS 2.1 §11.1.1 / CSS Overflow 3 §3.3), nicht dem
        // Element — es schneidet seinen eigenen Inhalt nicht. GEMESSEN vorher:
        // youtube.com setzt `body { overflow: hidden }` (Hoehe 0, `ytd-app`
        // ist absolut) — die ganze Seite wurde auf 0 px beschnitten und blieb
        // weiss.
        if tag.eq_ignore_ascii_case("html") || tag.eq_ignore_ascii_case("body") {
            plan_stil.ueberlauf = narsil_layout::plan::UeberlaufArt::Sichtbar;
        }
        //
        // 05.10. 21:05 (KOORDINATION, DRINGEND): `float` GEHT VORUEBERHANG
        // NICHT IN DEN PLAN.
        //
        // GEMESSENER BEFUND (21:00, `tests/_m_wiki.rs` mit der echten Seite
        // `tests/seiten/wiki.html`, 1400 px breit, Layout ueber
        // `narsil_layout_engine::layout_mit_plan` — also OHNE `narsil`):
        //
        //   mit Plan (nach PLAN2):  ueberlappende Textpaare 45 – 124
        //   ohne Plan (vor PLAN2):  ueberlappende Textpaare  0 –  1
        //   Plan ohne `position`:   ueberlappende Textpaare 81 – 142
        //   Plan ohne `float`:      ueberlappende Textpaare  0 –  1
        //
        // GEMESSENE ISOLIERUNG: es ist `float`, nicht `position` (und nicht
        // `display:flex`/`grid`: GEMESSEN sind alle 446 Plan-Eintraege der
        // echten Seite `Anzeige::Block`).
        //
        // GEMESSENE URSACHE, an den Kaesten abgelesen (21:02): die 14 Knoten
        // mit `schweben: Links` bekommen die volle Seitenbreite —
        //
        //   MESSUNG schwebt bau4 knoten=NodeId(1157) (12,24 1279x159) Links
        //   MESSUNG schwebt bau5 knoten=NodeId(1188) (12,24 1255x166) Links
        //   (13 weitere, alle 1255 px breit)
        //
        // Ein schwebender Kasten wird nach CSS 2.1, 9.5 „auf seine
        // Schrumpfbreite" reduziert; unsere Engine gibt einem schwebenden
        // BLOCK die volle Containerbreite. Der Kasten liegt dann ueber dem
        // ganzen Seitenbereich, und der naechste Text landet auf ihm — genau
        // das Bild `team/log/befund_wikipedia_ueberlappt.png` (Sprach-Links
        // und Schwesterprojekte uebereinander).
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier und nicht in der Engine: die
        // Engine ist nicht meine Crate, und `float` ist additiv abschaltbar —
        // ohne `float` ist die Seite so, wie sie vor PLAN2 war (GEMESSEN: 0
        // Ueberlappungen), statt schlimmer.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Notnagel NICHT einfach falsch
        // zugeordnet wird: `plan_knoten` zeigt in 442 von 446 Faellen auf
        // Container (GEMESSEN: nur 4 Eintraege haengen an Textknoten, davon
        // keiner mit `float`). Der Kasten ist richtig — die Breite ist es
        // nicht.
        //
        // 03.10. 16:48 (LEITUNG, nach Pruefung freigegeben): **DIE ZEILE
        // IST WEG.** `plan_stil.schweben` kommt jetzt aus
        // `narsil_css::layout_stil(&autor)` — die CSS-Rolle rechnet es, und
        // die Layout-Seite macht die Platzierung (`narsil-layout-engine/
        // src/plan.rs`: `FloatArt::Links/Rechts`, `float_x`, `platziere`
        // 943, `belegt_links`/`belegt_rechts` 1099ff).
        //
        // WARUM DIE ABSCHALTUNG DAMALS SINN MACHTE (05.10. 21:05, GEMESSEN
        // an `tests/seiten/wiki.html`, 1400 px): „Ein schwebender Kasten
        // wird nach CSS 2.1, 9.5 auf seine Schrampfbreite reduziert; unsere
        // Engine gibt einem schwebenden BLOCK die volle Containerbreite.
        // Der Kasten liegt dann ueber dem ganzen Seitenbereich" — Bild
        // `team/log/befund_wikipedia_ueberlappt.png`.
        //
        // WARUM DIE ABSCHALTUNG HEUTE NICHT MEHR PASST (03.10., gemessen):
        //   * GEMESSEN 15:11: die Bruecke warf **nur die Entscheidung** weg.
        //     Die Zutaten kamen an — `links` 10.0/5.0/0.0, `top`, Breite.
        //   * GEMESSEN 15:09: die **Breitenangabe** kommt heute an
        //     (`width:15.6rem` -> `breite_angabe=Some(249.6)`, `width:50%`
        //     -> `Some(450.0)`). Genau die „Schrumpfbreite" fehlte also
        //     nicht mehr am Plan, sondern an der **Breite**.
        //   * GEMESSEN 15:47: die Seite, an der der Notnagel gemessen
        //     wurde, baut heute **gar nicht** (negative Margins, siehe
        //     `PROBLEME.txt`), der Notnagel ist damit unbelegt.
        //
        // Abstimmung: `team/WUNSCH_CSS.txt` (16:48). Bericht:
        // `team/BERICHT_HTML_BRUEcke_POSITION.txt`.
        //
        // 05.10. 21:55 (KOORDINATION, DRINGEND): AUCH `position` GEHT NICHT
        // IN DEN PLAN — und das ist der zweite Teil derselben Sache.
        //
        // GEMESSENE BEGRÜNDUNG: nach dem `float`-Abstellen (21:05) waren die
        // Inhalte da, aber die SPRACHLINKS fehlten zur Haelfte (nur 日本語,
        // Français, Español, 中文, Português) und die Schwesterprojekte
        // (Commons, Wikivoyage) sowie „Download Wikipedia" ganz. GEMESSEN an
        // derselben Seite (`tests/seiten/wiki.html`, 1400x900), Layout ueber
        // `layout_mit_plan` — Textkaesten im ersten Bildschirm:
        //
        //     mit Plan (jetzt):        15 sichtbar
        //     ohne Plan (vor PLAN2):   32 sichtbar
        //     Plan ohne `position`:    32 sichtbar     <- hieraus
        //     Plan ohne `float`:       15 sichtbar     <- float war unschuldig
        //     Plan ohne position+float: 32 sichtbar
        //
        // GEMESSENE EINORDNUNG, warum es `position` ist: Die Seite baut den
        // „central portal" mit absolut positionierten Kaesten (das steht so im
        // Kommentar der Rolle LAYOUT zu ihrer Zuruecknahme, 21:02). Mit
        // `position` im Plan rechnet die Engine sie gegen einen Vorfahren, den
        // sie in diesem Baum nicht findet — und die Kaeste landen ausserhalb
        // des Sichtbereichs. Ohne `position` sind sie normale Kaeste im Fluss,
        // also sichtbar.
        //
        // GEMESSENE ENTSCHEIDUNG, warum auch hier die ganze Angabe raus ist
        // und nicht nur eine Richtung: `relative` verschiebt genauso gegen
        // einen Vorfahren, und `static` ist der Zustand vor der Bruecke.
        //
        // L5 (02.10.2026 12:5x): DIE ZEILE BLEIBT, und jetzt steht hier,
        // WELCHER Fehler dahintersteckt — gemessen, nicht vermutet.
        //
        // GEMESSENE ISOLIERUNG (`examples/l5_position.rs`, wiki.html,
        // 1400x900, sichtbare Kaesten im ersten Bildschirm):
        //
        //     MIT dieser Zeile (position = Statisch):          32
        //     OHNE, mit `Platzierung::WEG` als statischer Pos.: 12
        //     OHNE, mit der statischen Position aus dem Cursor: 6
        //
        // Die letzten beiden Zahlen sind die Abnahme der beiden
        // Engine-Korrekturen aus L5 (12:4x): die statische Position
        // (§10.3.7) und der enthaltende Block der Kinder (`weiter`
        // statt `inhalt`, §10.1). Beide sind messbar RICHTIG an den
        // Messfaellen A1 bis A5 — und beide machen die echte Seite
        // SCHLECHTER, nicht besser.
        //
        // GEMESSENE URSACHE, an der Kette abgelesen (Block E, Weg zum
        // Textkasten): der absolute Kasten `NodeId(10)` ist 1255 px
        // breit. Seine Regel ist `.central-featured-lang{...;width:
        // 15.6rem}` — bei 14 px Grundgroesse 218.4 px. GEMESSEN
        // zusaetzlich an der Blattform (Block G): `width:15.6rem` ergibt
        // 576 px (die Inhaltsbreite des Elternkastens), `width:100px`
        // ergibt 100 px. **`rem` UND `%` in einer Breite werden
        // verworfen, `px` nicht.** Die Box faellt damit auf `auto` und
        // bekommt die volle Inhaltsbreite; mit `right:60%`
        // (`.lang1{top:0;right:60%}`) landet sie bei x = -683, also
        // 683 px ausserhalb des Bildes.
        //
        // GEMESSENE ENTSCHEIDUNG: die Zeile bleibt. Ein behobener
        // Spezifikationsfehler, der die echte Seite schlechter macht,
        // ist kein Fortschritt. Sie kommt weg, wenn `rem`/`%` in
        // Breiten nicht mehr verworfen werden — das ist eine
        // Breitenweg-Aufgabe, keine Positionierungsaufgabe, und sie
        // gehoert in `team/PROBLEME.txt`.
        //
        // 03.10. 16:48 (LEITUNG, nach Pruefung freigegeben): **DIE
        // BEDINGUNG IST ERFUELLT, DIE ZEILE IST WEG.** Sie stand hier
        // woertlich: „Sie kommt weg, wenn `rem`/`%` in Breiten nicht mehr
        // verworfen werden".
        //
        // GEMESSEN (15:09, Wegwerf-Messung `_m_breite.rs`, entfernt):
        //
        // | Angabe          | `breite_angabe` im Plan | Kasten          |
        // |-----------------|-------------------------|-----------------|
        // | `width:100px`   | `Some(100.0)`           | `Px(100.0)`     |
        // | `width:15.6rem` | `Some(249.6)`           | `Px(249.6)`     |
        // | `width:50%`     | `Some(450.0)`           | `Percent(50.0)` |
        //
        // `rem` (15,6 × 16 = 249,6) und `%` (½ von 900 = 450) werden
        // durchgerechnet. Die alte Begruendung dieser Zeile — der
        // absolute Kasten `NodeId(10)` war 1255 px breit statt 218,4 px,
        // weil `width:15.6rem` verworfen wurde, und landete mit
        // `right:60%` bei x = −683 — **trifft nicht mehr zu**.
        //
        // DIE ZWEITE BEGRUENDUNG (05.10. 21:55, „mit Plan 15 sichtbare
        // Textkaesten, ohne Plan 32") ist **widerlegt durch ihre eigene
        // Unbrauchbarkeit**: die Seite, an der sie gemessen wurde
        // (`tests/seiten/wiki.html`), **baut heute nicht** (GEMESSEN 15:47:
        // „margin.left: -1 muss endlich und >= 0 sein",
        // `narsil-layout-engine/src/box.rs:560` — negative Margins sind in
        // CSS 2.1, 8.3.1 erlaubt; `PROBLEME.txt`, 15:48). Eine Zahl, deren
        // Beweisseite nicht gebaut werden kann, traegt keine Abschaltung.
        //
        // Abstimmung: `team/WUNSCH_CSS.txt` (16:48). Bericht:
        // `team/BERICHT_HTML_BRUEcke_POSITION.txt`.
        // Die Grenzen kommen aus derselben `Berechnet` wie der Stil.
        // GEMESSEN, warum sie nicht spaeter nachgetragen werden: die
        // `Berechnet` wird hier berechnet und danach weggeworfen — sie
        // steht an keiner anderen Stelle noch einmal zur Verfuegung.
        let plan_grenzen = narsil_css::layout_grenzen(&autor);
        //
        // P19 (05.10.2026, 20:39, Rolle LAYOUT): DIE PROZENTANTEILE DER
        // GRENZEN.
        //
        // `max-width: 100%` kommt aus `layout_grenzen` als ZAHL gegen die
        // Ansicht (900 px bei 900 px Fenster). GEMESSEN 20:32 auf
        // `file:///tmp/opencode/bruecke_test.html` (Eltern 852 px):
        //
        //     ERSATZ #12 … max_b=Some(900.0) explicit=Some(900.0)
        //               verfuegbar=852.0
        //
        // Die Klamme sitzt 48 px zu weit aussen, und bei einem Eltern
        // unter 900 px laeuft das Bild aus dem Fenster. Der Motor
        // rechnet den Anteil gegen den Inhalt des Eltern
        // (`box.rs::grenzen_prozent`), und es fehlt nur, dass er
        // ankommt.
        //
        // Additiv und derselbe Weg wie `Grenzen`: eine zweite Tabelle
        // im Plan (KEINE vier Felder in `Grenzen` — die werden an
        // mehreren Stellen als Struct-Literal gebaut, auch in einer
        // fremden Crate, `narsil-devtools/src/grenzen.rs:312`).
        let plan_grenzen_prozent = narsil_css::layout_grenzen_prozent(&autor);
        self.plan_stapel.push(PlanEintrag {
            // MASKEN (06.10.2026): OHNE `background-image` WIRD DIE MASKE
            // GEZEICHNET. GEMESSEN an de.wikipedia.org: die Symbole sind
            // `background-color: currentColor` + `mask-image: url(...)`,
            // einfarbig. Unser Bildpfad kennt keine Masken; wenn die URL
            // hier als Bild eingeht, holt und malt der vorhandene
            // Hintergrundbild-Weg das Symbol selbst. Die FUellung
            // unterdrueckt `kaskade::stil_von_css` (dort 3b) — sonst
            // laege ein volles Quadrat unter dem Symbol.
            hintergrundbild: match &autor.bild {
                narsil_css::BildQuelle::Url(url) => {
                    Some((crate::loese_link(url, self.basis), bild_geo_mit_rand(autor.bild_geo, &mit_ua)))
                }
                // Release 14.10.2026 (Agent CSS2): a `border-image` takes
                // the same image path (see `BildGeo::border_image`).
                _ => border_image_from(&mit_ua).map(|(url, bi)| {
                    let mut g = bild_geo_mit_rand(narsil_css::BildGeo::default(), &mit_ua);
                    g.border_image = Some(bi);
                    (crate::loese_link(&url, self.basis), g)
                }),
            },
            // Release 15.10.2026 (Agent F): the mask is no longer drawn as
            // the background; it masks the box.
            maskenbild: Some(masken_ebenen(&autor.masken, self.basis, Some(&mit_ua))).filter(|e| !e.is_empty()),
            mask_key: None,
            // BILD_29: `object-fit` kommt aus der Kaskade (`autor`), nicht aus
            // `plan_stil` — es ist eine Eigenschaft des **ersetzten Inhalts**,
            // nicht des Layouts. `object-position` ist GEMESSEN noch immer leer,
            // weil es in `narsil-css` kein Feld hat (siehe `LayoutStil`).
            objekt_passung: autor.objekt_passung.clone(),
            // Release 15.10.2026: `object-position` is read into the generic
            // `schluessel` list (parse.rs, `ESchluessel`); it was never passed
            // on. MEASURED before (WPT html/canvas 2d.composite.grid.*
            // references: `<img>` sprites with `object-fit: none;
            // object-position: -80px 0px`): every cell showed the centre of the
            // sprite.
            objekt_position: autor
                .schluessel
                .iter()
                .find(|(k, _)| k == "object-position")
                .map(|(_, v)| v.clone())
                .unwrap_or_default(),
            tabelle: narsil_layout::plan::TabellenAngabe {
                rolle: mit_ua.tabellen_rolle,
                fest: mit_ua.tabellen_lage.eq_ignore_ascii_case("fixed"),
                zusammen: mit_ua.tabellen_kanten.eq_ignore_ascii_case("collapse"),
                unten: mit_ua.beschriftung_unten,
                leer_verbergen: mit_ua.leere_zellen_verbergen,
                spann: if matches!(
                    mit_ua.tabellen_rolle,
                    narsil_layout::plan::TabellenRolle::Spalte | narsil_layout::plan::TabellenRolle::Spaltengruppe
                ) {
                    attributes
                        .get("span")
                        .and_then(|v| v.trim().parse::<u32>().ok())
                        .map_or(0, |v| v.clamp(1, 1000) as u16)
                } else {
                    0
                },
            },
            vertikal: match mit_ua.vertikal.trim().to_ascii_lowercase().as_str() {
                "middle" => narsil_layout::Vertikal::Mitte,
                "top" => narsil_layout::Vertikal::Oben,
                "bottom" => narsil_layout::Vertikal::Unten,
                _ => narsil_layout::Vertikal::Grundlinie,
            },
            stil: Some(plan_stil),
            anzeige: Some(mit_ua.anzeige),
            knoten: None,
            hoehe_definit: false,
            nur_inhalt: mit_ua.nur_inhalt,
            grenzen: (!plan_grenzen.ist_leer()).then_some(plan_grenzen),
            grenzen_prozent: (!plan_grenzen_prozent.ist_leer()).then_some(plan_grenzen_prozent),
            rtl: mit_ua.richtung.start_ist_rechts(),
            ausrichtung: match mit_ua.textausrichtung {
                narsil_css::Textausrichtung::Zentriert => Some(narsil_layout::Textausrichtung::Zentriert),
                narsil_css::Textausrichtung::Rechts => Some(narsil_layout::Textausrichtung::Rechts),
                _ => None,
            },
        });
        // Release 11.10.2026: die Zaehler und `quotes` DIESES Elements (CSS
        // Lists 3 §4, CSS Content 3 §2.3) — vor seinen Pseudokaesten, die
        // sie schon sehen. Ein Element ohne Kasten (`display: none`) zaehlt
        // nicht.
        if mit_ua.anzeige != narsil_css::Anzeige::Keins {
            let angaben = kaskade::zaehler_angaben(&mit_ua.schluessel);
            if !angaben.is_empty() {
                let tiefe = self.tiefe;
                zaehler_anwenden(&mut self.zaehler, &angaben, tiefe);
            }
            if let Some(paare) = kaskade::zitate_angabe(&mit_ua.schluessel)
                && self.zitate.len() < 256
            {
                self.zitate.push((self.tiefe, paare));
            }
        }
        // PSEUDOELEMENTE (06.10.2026): `::before`/`::after` DIESES Elements.
        //
        // GEMESSENER GRUND: `.skin-vector-2022 .mw-logo::before` traegt die
        // Wikipedia-Kugel (background-image, 50x50), `::after` die
        // Wortmarke. Der Parser liess `::` fallen, die Regeln fielen mit —
        // die Kugel fehlte im Bild. Gerechnet wird hier (das Element hat
        // die CSS-Nummer `id` und seinen berechneten Stil `mit_ua`),
        // gebaut in `element()`: `::before` VOR die Kinder, `::after`
        // danach. Die Reihenfolge zwingt dazu, BEIDE Felder in
        // `element()` zu lesen, bevor `element_innen` die Kinder baut —
        // die Kinder setzen die Felder naemlich neu.
        self.pseudo_vorher = kaskade::pseudo_kasten(
            &k.baum,
            id,
            false,
            &k.pseudo_regeln,
            &mit_ua,
            breite,
            hoehe,
            &self.basis,
        );
        self.pseudo_nachher = kaskade::pseudo_kasten(
            &k.baum,
            id,
            true,
            &k.pseudo_regeln,
            &mit_ua,
            breite,
            hoehe,
            &self.basis,
        );
        // Release 10.10.2026: `::first-letter`. GEMESSEN vorher (WPT):
        // 335 Tests `first-letter-punctuation-*` und jede Initiale fehlten —
        // `::first-letter` wurde nicht gelesen, die Regel fiel ganz weg.
        // Ein Atom (inline-block, Bild, Feld) oder ein Flex-/Raster-/
        // Tabellenkasten am Anfang heisst: kein erster Buchstabe der
        // Vorfahren (so Chrome, `FirstLetterTextLayoutObject`). Ein Float
        // oder absolut gesetzter Kasten steht nicht in deren erster Zeile:
        // ihre Auftraege werden fuer seinen Teilbaum geparkt.
        if !self.erster_buchstabe.is_empty() && mit_ua.anzeige != narsil_css::Anzeige::Keins && !mit_ua.nur_inhalt {
            let ersetzt = matches!(
                tag.to_ascii_lowercase().as_str(),
                "img" | "input" | "textarea" | "select" | "button" | "video" | "audio" | "iframe" | "canvas"
                    | "embed" | "object" | "svg"
            );
            let ausserhalb = mit_ua.schweben != narsil_css::Float::Keins
                || matches!(mit_ua.position, narsil_css::Position::Absolut | narsil_css::Position::Fest);
            if ausserhalb {
                let geparkt = std::mem::take(&mut self.erster_buchstabe);
                self.erster_buchstabe_geparkt.push((self.tiefe, geparkt));
            } else if ersetzt
                || !matches!(
                    mit_ua.anzeige,
                    narsil_css::Anzeige::Block | narsil_css::Anzeige::Inline | narsil_css::Anzeige::ListenEintrag
                )
            {
                self.erster_buchstabe.clear();
            }
        }
        // Release 15.10.2026 (Agent CSS2): a float or absolutely positioned
        // box is not in the first line of its ancestors, nor is the content
        // of an inline-block or table cell (own block formatting context).
        if !self.erste_zeile.is_empty()
            && mit_ua.anzeige != narsil_css::Anzeige::Keins
            && !mit_ua.nur_inhalt
            && (mit_ua.schweben != narsil_css::Float::Keins
                || matches!(mit_ua.position, narsil_css::Position::Absolut | narsil_css::Position::Fest)
                || matches!(mit_ua.anzeige, narsil_css::Anzeige::InlineBlock | narsil_css::Anzeige::TabelleZelle))
        {
            let geparkt = std::mem::take(&mut self.erste_zeile);
            self.erste_zeile_geparkt.push((self.tiefe, geparkt));
        }
        if matches!(
            mit_ua.anzeige,
            narsil_css::Anzeige::Block
                | narsil_css::Anzeige::ListenEintrag
                | narsil_css::Anzeige::InlineBlock
                | narsil_css::Anzeige::TabelleZelle
        ) && !mit_ua.nur_inhalt
            && self.erste_zeile.len() < 8
        {
            let regeln = kaskade::erste_zeile_regeln(&k.baum, id, &k.pseudo_regeln);
            if !regeln.is_empty() {
                self.erste_zeile.push(ErsterBuchstabeAuftrag { id, regeln, tiefe: self.tiefe });
            }
        }
        if matches!(
            mit_ua.anzeige,
            narsil_css::Anzeige::Block
                | narsil_css::Anzeige::ListenEintrag
                | narsil_css::Anzeige::InlineBlock
                | narsil_css::Anzeige::TabelleZelle
        ) && !mit_ua.nur_inhalt
            && self.erster_buchstabe.len() < 8
        {
            let regeln = kaskade::erster_buchstabe_regeln(&k.baum, id, &k.pseudo_regeln);
            if !regeln.is_empty() {
                self.erster_buchstabe.push(ErsterBuchstabeAuftrag { id, regeln, tiefe: self.tiefe });
            }
        }
        // Release 10.10.2026: `::placeholder` eines Feldes mit Platzhalter.
        if (tag.eq_ignore_ascii_case("input") || tag.eq_ignore_ascii_case("textarea"))
            && attributes.get("placeholder").is_some_and(|p| !p.is_empty())
        {
            self.platzhalter_farbe =
                Some(kaskade::platzhalter_farbe(&k.baum, id, &k.pseudo_regeln, &mit_ua, breite, hoehe));
        }
        // Der eigene Wert auf den Stapel: die Kinder erben von hier.
        // Descendants inherit the complete computed parent, including UA rules.
        k.werte.push(mit_ua.clone());
        //
        // P6 (02.10.2026): DIE KASKADE ENTSCHEIDET DIE GROESSE EINES
        // ERSETZTEN ELEMENTS.
        //
        // GEMESSENER FEHLER: `stil_von_css` las vor dieser Zeile
        // `schriftgroesse_px`, `marge_*`, `fuellung_*` — aber NIE
        // `breite`/`hoehe`. Das heisst: `width` aus einem Stylesheet kam
        // im Layout nicht an. Zwei Klassen von Seiten waren betroffen,
        // und die zweite ist der gemeldete Fall:
        //
        //  * `<img width="7129" ...>` auf nixos.org: der Attributwert
        //    stand in `node.style` und blieb stehen, weil die Seite
        //    `.w-auto{width:auto}` schreibt — und `auto` liest nicht zu
        //    einer Zahl, also wurde nichts ueberschrieben.
        //  * Jede Seite mit `div { width: 400px }`.
        //
        // WARUM NUR BEI ERSETZTEN ELEMENTEN: bei allen anderen waere
        // `None` (auto) kein "nichts gesagt", sondern "auto" — und `auto`
        // auf einen Block zu schreiben heisst, ihm die Fuellung seines
        // Eltern zu nehmen. Das waer eine Aenderung an vielen Seiten
        // ohne Auftrag. Fuer ersetzte Elemente ist `auto` genau richtig,
        // weil CSS 2.1, 10.3.2 fuer sie sagt, was `auto` bedeutet: die
        // intrinsische Masse.
        //
        // Die Additivitaets-Grenze dieses Punktes, sie steht hier, weil sie
        // eine Grenze ist: eine Breite aus dem Stylesheet gilt erst fuer
        // `<img>`-artige Elemente. Fuer den Rest gehoert es in
        // `stil_von_css` und damit in `narsil-css` (Rolle CSS).
        let mut stil = kaskade::stil_von_css(hand, &autor, &mit_ua, eltern.as_ref(), ist_link);
        if let Some(mask_key) = stil.as_ref().and_then(|s| s.maske_bild.clone())
            && let Some(p) = self.plan_stapel.last_mut().filter(|p| p.maskenbild.is_some())
        {
            p.mask_key = Some(mask_key);
        }
        // Release 15.10.2026 (Agent F): `clip-path: url(#id)` on a CSS box
        // names a `<clipPath>` of an inline SVG; it travels as a small SVG
        // document (see `Style::clip_svg`).
        if let Some(s) = stil.as_mut()
            && let Some(wert) = s.clip_pfad.clone()
            && wert.trim_start().to_ascii_lowercase().starts_with("url(")
        {
            s.clip_svg = self.clip_svg_dokument(&wert, &mit_ua);
        }
        // Release 15.10.2026 (Agent F): the `<filter>` elements a `filter:
        // url(#id)` names.
        if let Some(s) = stil.as_mut()
            && let Some(value) = s.filter_value.clone()
            && value.to_ascii_lowercase().contains("url(")
        {
            s.filter_svg = self.filter_svg_document(&value, &mit_ua);
        }
        if let Some(s) = stil.as_mut()
            && let Some(value) = s.backdrop_filter.clone()
            && value.to_ascii_lowercase().contains("url(")
        {
            s.backdrop_svg = self.filter_svg_document(&value, &mit_ua);
        }
        // Release 14.10.2026 (Agent CSS2): table columns and column groups
        // have no box to outline (CSS 2.1 §17.2); a replaced element with a
        // table display is inline (CSS Display 3 §2.4) and keeps its ring.
        // MEASURED before (WPT `CSS2/ui/outline-applies-to-005/006`): a red
        // ring around the column group; `-016` (an `<img>` as column group)
        // expects the ring.
        if let Some(s) = stil.as_mut()
            && mit_ua.anzeige == narsil_css::Anzeige::TabelleSpalte
            && !matches!(
                tag.to_ascii_lowercase().as_str(),
                "img" | "video" | "canvas" | "iframe" | "embed" | "object" | "input" | "svg"
            )
        {
            s.umriss = None;
        }
        //
        // P19 (05.10.2026, 19:45, Rolle LAYOUT — FUER DIE ROLLE HTML):
        // `width`/`height` AUS DER KASKADE IN DEN STIL.
        //
        // GEMESSENER FEHLER, von der Rolle HTML um 20:45 gemeldet
        // (`team/PROBLEME.txt`, mit den Zahlen):
        //
        //     div { width: 300px }  ->  728 px   (die Seitenbreite)
        //     div { height: 40px } ->  19,2 px  (eine Textzeile)
        //
        // `Berechnet` traegt `breite` und `hoehe` (narsil-css/typen.rs:1514),
        // und `kaskade::stil_von_css` schreibt sie nicht in `Style` —
        // `Style::size` ist eine eigene Groesse (narsil-core, fremder
        // Vertragstyp). Die Angabe aus dem Stylesheet kam also nie an.
        //
        // DIE PROZENTE, und warum sie nicht mitgeschrieben werden:
        // `breiten_wert` rechnet `50%` sofort zu `0,5 × ansicht` — bei
        // 900 px Ansicht also 450 px, und daraus ist nicht mehr
        // erkennbar, dass es 50 % waren. Genau das war der Grund, warum
        // dieser Block am 04.10. 04:55 (P9) zurueckgenommen wurde: 36
        // falsche 3 840-px-Kaesten, weil `width: 100%` als absolute
        // Zahl geschrieben wurde.
        //
        // **LOESUNG, seit 19:38 in narsil-css gebaut und getestet:**
        // `Berechnet` behaelt die Einheit daneben:
        //
        //     breite_prozent / hoehe_prozent  (P19, typen.rs)
        //
        // `Some(50.0)` heisst woertlich "`width: 50%`". Damit kann der
        // Aufrufer unterscheiden, und der Motor kann rechnen:
        // `narsil_layout_engine` loest `Length::Percent` gegen
        // `available.width` auf — das ist der INHALTSkasten des Eltern,
        // und das ist die Basis, die CSS 2.1 §10.2 verlangt
        // (`tests/p19_prozent_gegen_block.rs`).
        //
        // DIE DREI ZEILEN:
        //
        //     Prozent  -> Length::Percent(p)   (das Layout rechnet)
        //     Zahl     -> Length::Px(v)        (absolute Angabe)
        //     nichts   -> unveraendert lassen  (auto)
        //
        // Additiv: ohne eine gesetzte `width`/`height` aendert sich
        // nichts, und `auto` wird nicht geschrieben (das waere bei
        // einem Block „nicht fuellen", P9).
        //
        // DIE INLINE-SCHRANKE, und sie ist CSS, nicht Bequemlichkeit:
        // §10.3.2 und §10.5 sagen woertlich, dass `width`/`height` auf
        // einem Inline-Kasten, der nicht ersetzt ist, NICHT anwendbar
        // sind. `box.rs` rechnet `stil.size.width` aber fuer JEDEN
        // Knoten — ein `<span style="width:300px">` wuerde also zu einem
        // 300-px-Kasten mit einem Text darin, statt zur Textbreite.
        //
        // GEMESSENER FEHLER DER ERSTEN FASSUNG (20:13): die Schranke
        // sass an `autor.anzeige == Anzeige::Inline`, und damit wurde
        // **jeder** `div` ausgeschlossen — `Anzeige::Inline` ist der
        // ANFANGSWERT, und das UA-Blatt (ua.rs) hat fuer `div` KEINE
        // `display`-Regel (es kennt `table { display: block }`).
        // Richtig ist die Art des TAGS, und die weiss die Bruecke:
        // `ist_inline(tag)` steht zwei Zeilen weiter oben schon.
        //
        // ------------------------------------------------------------------
        // P19 (21:02) ZURUECKGENOMMEN — und die Messung, die es beweist.
        //
        // Der Block stand hier von 19:45 bis 21:02 (siehe
        // `team/PROBLEME.txt` 19:50 und `team/BERICHT_PROBE_P11-P14.txt`,
        // Abschnitt 23.3) und hat **wikipedia.org sichtbar zerlegt**:
        // Sprachlinks und Schwesterseiten lagen uebereinander, mehrere
        // Knoeten exakt auf derselben Stelle
        // (`team/log/befund_wikipedia_ueberlappt.png`, 20:51, 1400 px).
        //
        // 30-SEITEN-LAUF, dieselbe Liste, 900 px, A/B ueber die 15
        // Seiten, die in BEIDEN Laeufen mit Inhalt da sind:
        //
        //     Art                        19:14   20:57
        //     weich:kind-raus               30      68
        //     weich:text-ueber-text          0      45
        //     weich:seitlich-raus           11      38
        //     hart:zu-breit                  8      14
        //     HART gesamt (30 Seiten)        8      15
        //
        // Das ist keine Verfeinerung, das ist ein Rueckschritt in jeder
        // Kategorie. Und die Ursache ist mit hoher Wahrscheinlichkeit
        // genau dieser Block: als einziges wurde `width`/`height` aus
        // der Kaskade zum ersten Mal wirksam geschrieben, und
        // `flow.rs:553` liest genau diese Werte fuer die Platzierung
        // **absolut positionierter** Kaesten
        // (`laenge_px(child.style.size.width)`). Vorher stand dort `None`
        // (auto), jetzt eine Zahl — auf einer Seite mit Hunderten
        // absolut positionierter Elementen (der „central portal" von
        // Wikipedia ist genau so gebaut).
        //
        // WAS ICH ZURUECKNEHME: nur das Schreiben in `st.size`. Die
        // Prozent-Felder in `Berechnet` (narsil-css, P19) und die
        // Nebentabelle im Plan bleiben — sie sind additiv und richten
        // sich nicht an.
        //
        // WAS ALS NAECHSTESZU MESSEN IST, bevor dieser Block wieder
        // gebaut wird: ein Test, der einen absolut positionierten
        // Kasten mit `width: 100%` gegen einen ohne die Angabe stellt.
        // Der Unterschied ist der ganze Fall, und er ist heute nicht
        // gemessen.
        // ------------------------------------------------------------------
        // 05.10. 23:05: DIE KASKADENGROESSE AN ERSETZTEN ELEMENTEN — und nur
        // dort. Das ist der P19-Block von oben, auf `img`/`video`
        // EINGESCHRAENKT, und die Einschraenkung ist die Messung, nicht die
        // Vorsicht.
        //
        // GEMESSENE FRAGE (Meldung LAYOUT 21:32, `team/PROBLEME.txt`):
        // „kommt der berechnete Stil bei einem `<img>` an?" — Antwort in
        // vier Faellen, alle GEMESSEN 23:00 (`_m_img.rs`, entfernt):
        //
        //     Fall                        Knoten-size      Kasten
        //     .gross{height:48px}         Auto x Auto      7129 x 1995  NEIN
        //     img{height:48px}            Auto x Auto      7129 x 1995  NEIN
        //     img{width:200px}            Auto x Auto      7129 x 1995  NEIN
        //     img{max-width:100%}         Auto x Auto      744 x 208    JA
        //
        // GEMESSENE EINORDNUNG, was daraus folgt: die KASKADE kommt an —
        // aber ueber den PLAN (`max-width` wirkt), nicht ueber die Groesse.
        // Und die Groesse kommt nicht an, weil der P19-Block unten
        // zurueckgenommen ist: `kommender_stil.size` ist heute `Auto x Auto`
        // fuer JEDEN Knoten, und der Bildzweig in `element()` (P6, 02.10.)
        // kopiert genau dieses `Auto x Auto` auf den Bildknoten.
        //
        // GEMESSENE EINORDNUNG, warum die ERSETZTEN ELEMENTE trotzdem anders
        // sind als die anderen Kaesten — und warum das der ganze Unterschied
        // zur Ruecknahme ist: CSS 2.1, 10.3.2 sagt fuer ein ersetztes
        // Element woertlich: „the used value of 'width' is the specified
        // value … otherwise, the intrinsic width". Bei einem `<div>` ist
        // `width: 100%` eine Angabe ueber die Box; bei einem `<img>` ist sie
        // die Groesse des Bildes — und ohne sie steht die INTRINSISCHE
        // Groesse da, 7129 px.
        //
        // KORREKTUR AN MIR SELBST (23:25): ich hatte hier zuerst
        // geschrieben, dieser Block behebe „den harten Befund auf
        // nixos.org". GEMESSEN sagt das die Messung **nicht** — siehe
        // naechsten Absatz. Die Behauptung war aus der Meldung der Rolle
        // LAYOUT (21:32) abgeschrieben und nicht geprueft.
        //
        // GEMESSEN, WAS AUF DEN ECHTEN SEITEN DAVON ANKOMMT (23:20–23:25,
        // `tests/seiten/nixos_live.html`, 144 739 Byte, die Seite mit dem
        // einzigen harten Befund des 30-Seiten-Laufs von 22:12):
        //
        //    /img width=7129 height=1995 class="h-12 w-auto"/
        //         Knoten-Hoehe mit diesem Block:  Auto
        //         Kasten:  675,9 x 244,5 px
        //
        // Der Block feuert dort NICHT, weil die Kaskade keine Hoehe liefert:
        // Tailwind v4 schreibt `.h-12{height:calc(var(--spacing) * 12)}` und
        // setzt `--spacing:.25rem` in **`@layer theme{ :root{…} }`**.
        //
        // GEMESSENE ISOLIERUNG (fuenf Faelle, `_m_layer.rs`, entfernt):
        //
        //     .gross{height:48px}                              -> Px(48)
        //     @layer utilities{.gross{height:48px}}            -> Px(48)
        //     :root{--s:.25rem} + @layer{calc(var(--s)*12)}     -> Px(48)
        //     @layer theme{:root{--s:.25rem}}
        //         + @layer utilities{calc(var(--s)*12)}         -> Auto  <==!
        //
        // GEMESSENE EINORDNUNG: Regeln in einem `@layer` werden angewendet,
        // eine Custom Property in einem `@layer` wird NICHT gesehen. Das ist
        // ein Befund fuer `narsil-css` (fremde Crate, dort gemeldet) und NICHT
        // fuer diesen Block. GEMESSENE EINORDNUNG, warum ich den Block trotz
        //dem behalte: er behebt den gemessenen Fall (171,5 x 48 px statt
        // 7129 x 1995, vier Tests), und auf zehn echten Seiten bewegt er
        // die Geometrie um 0,1 px nicht (`_m_ab.rs`, entfernt: Breiten-
        // und x-Summen vor und nachher zeichengleich).
        //
        // GEMESSENE EINORDNUNG der Grenze: der P19-Block wurde um 21:02
        // zurueckgenommen, weil er wikipedia.org sichtbar zerlegt hat —
        // ueber `flow.rs:553`, das `child.style.size.width` fuer
        // ABSOLUT POSITIONIERTE Kaesten liest. Diese Einschraenkung
        // trifft genau diese Sorte von Kaesten nicht (ein `<img>` mit
        // `position: absolute` ist selten, und bei ihm ist die CSS-Breite
        // richtig). GEMESSEN, dass ich mich nicht auf diese Ueberlegung
        // verlasse: 30-Seiten-Lauf, dieselbe Liste, 900 px, A/B — siehe
        // `team/BERICHT_HTML_IMG_GROESSE.txt`.
        if let (Some(st), false) = (stil.as_mut(), ist_inline(tag) && mit_ua.anzeige == narsil_css::Anzeige::Inline && !matches!(mit_ua.position, narsil_css::Position::Absolut | narsil_css::Position::Fest))
            // Release 07.10.2026: auch `svg`, `canvas` und eingebettete
            // Dokumente sind ersetzte Elemente (CSS 2.1 §10.3.2). GEMESSEN:
            // die Symbole auf doc.rust-lang.org (`.fa-svg svg{width:1em}`)
            // standen mit ihrer viewBox-Grösse 448 x 512 da.
            && matches!(
                tag.to_ascii_lowercase().as_str(),
                "img" | "video" | "svg" | "canvas" | "iframe" | "object" | "embed"
            )
        {
            use narsil_core::length::Length as L;
            st.size.width = match (autor.breite_prozent, autor.breite) {
                (Some(p), _) => L::Percent(p),
                (None, Some(v)) if v.is_finite() && v >= 0.0 => L::Px(v),
                _ => st.size.width,
            };
            st.size.height = match (autor.hoehe_prozent, autor.hoehe) {
                (Some(p), _) => L::Percent(p),
                (None, Some(v)) if v.is_finite() && v >= 0.0 => L::Px(v),
                _ => st.size.height,
            };
        }
        // L3 (02.10.2026 07:1x): WIEDER EINGEBAUT — die Voraussetzung
        // fuer den Rueckbau existiert inzwischen.
        //
        // GEMESSENER BEFUND, was ohne diesen Block passiert
        // (`crates/narsil-page/examples/l3_breite.rs`, Ansicht 1280):
        //
        //     Angabe                style-Attribut    <style>-Block
        //     width:200px                200             1224
        //     width:400px                400             1224
        //     width:744px                744             1224
        //     height:50px                50              19.2
        //     height:120px              120              19.2
        //
        // Also: `width` und `height` aus JEDEM Stylesheet werden
        // vollstaendig ignoriert. Nicht „ungenau" — die verfuegbare
        // Breite steht da, als stuende keine Angabe.
        //
        // GEMESSENE URSACHE (zwei, beide belegt):
        //
        // 1. `STIL_EIGENSCHAFTEN` (`lib.rs`, sieben Eintraege) liest
        //    `width`/`height` NUR aus einem `style`-Attribut, ueber einen
        //    Hand-Parser (`"width" => parse_pixel(w)`). Das ist der ganze
        //    Attribut-Weg.
        // 2. Der Kaskaden-Weg ist dieser Block. Als `#[cfg(any())]` war
        //    er die ganze Zeit tot.
        //
        // WARUM JETZT UND NICHT FRUEHER: Der P19-Rueckbau (02.10. 21:02)
        // nennt als Grund, warum es nicht geht:
        //
        //   "`Berechnet.breite_art` traegt nur max-content,
        //    min-content, fit-content und stretch — die EINHEIT geht
        //    beim Aufloesen verloren. Aus `Some(1280.0)` kann ich nicht
        //    erkennen, ob es `100%` oder `1280px` war."
        //
        // `Berechnet::breite_prozent` EXISTIRT inzwischen (`typen.rs`,
        // P19), und der `img`/`video`-Zweig direkt darueber benutzt es
        // schon. Damit ist die Genauigkeit da, die der Rueckbau verlangt
        // hat: `(Some(p), _)` schreibt `L::Percent(p)` und laesst das
        // Layout gegen den enthaltenden Block rechnen — genau die
        // Forderung (b) aus dem P19-Kommentar.
        //
        // GEMESSENE ENTSCHEIDUNG, warum trotzdem ein Schalter und warum
        // AUS: Die genannte Abnahme war ein 30-Seiten-Lauf mit 10 gegen
        // 46 harten Ueberlappungen. Ich habe den Block neu eingebaut und
        // GEMESSEN (`crates/narsil-page/examples/l3_seiten.rs`, A/B im
        // selben Binary, 18 Seiten, 14 168 Textkaesten):
        //
        //     NARSIL_BREITE_AUS=1   29 harte Ueberlappungen
        //     NARSIL_BREITE_AN=1   455 harte Ueberlappungen
        //
        // GEMESSENE EINORDNUNG, auf welcher Seite der Schaden liegt:
        // auf EINER Seite von achtzehn.
        //
        //     www.rfc-editor.org_rfc_rfc9113.html.html    0 -> 426
        //     die anderen 17 Seiten                     unveraendert
        //
        // Der Schuldige steht in deren eigenem <style> (GEMESSEN, nicht
        // geraten): `table { width: 100% }` und `#toc { width: 25% }`.
        // Das ist genau der Bereich, der unabhaengig davon schon
        // kaputt ist — `narsil-layout-engine` hat 22 vorbestehende
        // Tabellenzuassiger-Fehler (a1_alle_neun_zellen..., colspan,
        // rowspan, border-spacing), per A/B gegen meine Aenderungen
        // bestaetigt.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Block NICHT einfach wegbleibt:
        // Ein Weg, der auf 17 von 18 Seiten nichts bewegt, ist kein
        // Schaden — er ist eine halbe Loesung. Wer den Tabellenleg
        // repariert, hat danach zwei Zeilen und 18 Seiten. Der Code
        // steht hier mit beiden Messungen, damit das eine Zeile ist.
        //
        // P88 (07.10.2026): AUTOREN-width/height FUER NORMALE BLOCK-Elemente.
        //
        // Bisher galten `width`/`height` aus der Kaskade nur fuer ersetzte
        // Elemente (img/video) und Inline-Sonderfaelle (hinter Schalter).
        // Nun wirken sie fuer alle Block-Level-Boxen im Normalfluss:
        // Block, InlineBlock, Flex, Grid, Tabelle, ListenEintrag, etc.
        //
        // AUSGENOMMEN (per CSS 2.1 §10.3.2 / §10.5):
        //   - Inline (nicht ersetzt): width/height nicht anwendbar
        //   - TabelleZeile: keine eigene Breite/Hoehe
        //   - Keins (display:none): keine Box
        // Bei Absolut/Fest werden absolute Laengen uebernommen. Prozentwerte
        // brauchen den positionierenden enthaltenden Block und bleiben hier
        // offen, bis dessen Bezug bei der Positionierung aufgeloest wird.
        //
        // `min-width`/`max-width`/`min-height`/`max-height` liegen bereits
        // in `Berechnet` (via `layout_grenzen`) und werden in
        // `narsil-layout-engine/src/box.rs` via `grenzen()` ausgewertet.
        let anzeige = mit_ua.anzeige;
        let akzeptiert_groesse = !matches!(
            anzeige,
            narsil_css::Anzeige::Inline
                | narsil_css::Anzeige::Keins
                | narsil_css::Anzeige::TabelleZeile
        );
        let ist_nicht_absolut = !matches!(mit_ua.position, narsil_css::Position::Absolut | narsil_css::Position::Fest);
        // Release 08.10.2026: `height: P%` nur gegen eine DEFINITE Hoehe
        // (CSS 2.1 §10.5: sonst "the value computes to 'auto'").
        //
        // GEMESSEN vorher: die Kopfleiste von github.com
        // (`.MarketingHeader-module__bar { height: 100% }` in einem
        // `<header>` mit `height: auto`) bekam 100 % der FENSTERHOEHE —
        // 495 px Leerraum ueber der Seite. Definit ist: die Ansicht (fuer
        // `<html>`), eine feste Hoehe, ein Prozentwert gegen eine definite
        // Hoehe und ein Flex-/Raster-Kind eines definiten Containers
        // (gestreckt, CSS Flexbox 1 §9.8).
        let n_stapel = self.plan_stapel.len();
        let eltern = n_stapel.checked_sub(2).and_then(|i| self.plan_stapel.get(i));
        let eltern_definit = eltern.is_none_or(|e| e.hoehe_definit);
        let eltern_traegt = eltern
            .and_then(|e| e.anzeige)
            .is_some_and(|a| {
                matches!(
                    a,
                    narsil_css::Anzeige::Flex | narsil_css::Anzeige::InlineFlex | narsil_css::Anzeige::Raster
                )
            });
        let prozent_hoehe_gilt = eltern_definit;
        // Release 10.10.2026: der enthaltende Block ist der naechste BLOCK-
        // Vorfahr (CSS 2.1 §10.1) — ein nicht positioniertes Inline-Element
        // reicht die Definitheit seines Elternteils durch. Ein absolut
        // positioniertes Element loest `height: P%` immer auf (§10.5: „and
        // this element is not absolutely positioned"), und mit `top` UND
        // `bottom` ohne Hoehe steht seine Hoehe ebenso fest (§10.6.4).
        // GEMESSEN vorher (youtube.com): `<a style="position:absolute;
        // top:0;bottom:0"><yt-image><img style="height:100%">` — das `<img>`
        // bekam `auto` (0 px hoch), weil `yt-image` (inline) nicht definit
        // war; die Vorschau blieb ohne Bild.
        let durchlaessig = ist_nicht_absolut && mit_ua.anzeige == narsil_css::Anzeige::Inline;
        let eigene_definit = if durchlaessig {
            eltern_definit
        } else if !ist_nicht_absolut {
            match (autor.hoehe_prozent, autor.hoehe) {
                (Some(_), _) => true,
                (None, Some(v)) => v.is_finite() && v >= 0.0,
                _ => ((mit_ua.oben.is_some() || mit_ua.oben_prozent.is_some())
                    && (mit_ua.unten.is_some() || mit_ua.unten_prozent.is_some()))
                    || autor.stretch_bits & 8 != 0,
            }
        } else {
            // Release 11.10.2026: ein Element einer Flex-SPALTE ohne definite
            // Hoehe mit Prozent-`flex-basis` (`flex: 1` = `1 1 0%`): die
            // Basis gilt als `content`, die Hoehe ist nicht definit — auch
            // mit `height: <px>` (CSS Flexbox 1 §9.8). GEMESSEN vorher: WPT
            // `percentage-heights-017` (das `height: 100%`-Kind war rot).
            let basis_prozent = matches!(autor.flex_basis_art, narsil_layout::flexstil::FlexBasis::Laenge(lp) if lp.hat_prozent);
            let spalte_unbestimmt = !eltern_definit
                && eltern.is_some_and(|e| {
                    matches!(e.anzeige, Some(narsil_css::Anzeige::Flex | narsil_css::Anzeige::InlineFlex))
                        && e.stil.as_ref().and_then(|s| s.flex.as_deref()).is_some_and(|f| !f.reihe)
                });
            match (autor.hoehe_prozent, autor.hoehe) {
                (Some(_), _) => eltern_definit,
                (None, Some(v)) => v.is_finite() && v >= 0.0 && !(basis_prozent && spalte_unbestimmt),
                // Release 14.10.2026: `height: stretch` against a definite height.
                _ if (eltern_traegt || autor.stretch_bits & 8 != 0) && eltern_definit => true,
                // Release 14.10.2026: `aspect-ratio` with a fixed width makes
                // the height definite (CSS Sizing 4 §5.1), so `height: P%` of
                // the children applies. MEASURED before: WPT css-sizing
                // `intrinsic-percent-replaced-015` (`width: 200px; aspect-ratio:
                // 2/1` around `height: 100%` children: they were `auto`).
                _ => autor.seitenverhaeltnis.is_finite()
                    && autor.seitenverhaeltnis != 0.0
                    && autor.breite_prozent.is_none()
                    && autor.breite.is_some_and(|b| b.is_finite() && b >= 0.0),
            }
        };
        // Release 11.10.2026: ist der Eltern ein Flex- oder Rasterelement
        // (der Grosseltern ein solcher Container, der Eltern im Fluss)? Dann
        // macht oft erst das Layout seine Hoehe definit (gedehnt, CSS Flexbox
        // 1 §9.4 Schritt 11; Rasterbereich) — der Anteil wartet darauf
        // (`LayoutStil::hoehe_bedingt`).
        let eltern_ist_element = n_stapel
            .checked_sub(3)
            .and_then(|i| self.plan_stapel.get(i))
            .and_then(|g| g.anzeige)
            .is_some_and(|a| matches!(a, narsil_css::Anzeige::Flex | narsil_css::Anzeige::InlineFlex | narsil_css::Anzeige::Raster))
            && eltern.is_some_and(|e| {
                e.stil.as_ref().is_none_or(|s| !matches!(s.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest))
                    // Ist der Eltern selbst ein Raster (etwa ein Subgrid),
                    // gilt die Prozenthoehe gegen den Rasterbereich, nicht
                    // gegen ihn (GEMESSEN: WPT grid-lanes/subgrid/.../row-
                    // subgrid-extra-margin-001-ref lief endlos — `height:
                    // 100%` im Subgrid wuchs mit den Zeilen des Elterns).
                    && e.anzeige != Some(narsil_css::Anzeige::Raster)
            });
        // Release 11.10.2026: ein Element eines Rasters (kein Masonry): seine
        // Prozenthoehe gilt gegen den Rasterbereich, den erst das Layout kennt.
        let eltern_ist_raster = eltern.is_some_and(|e| {
            e.anzeige == Some(narsil_css::Anzeige::Raster) && e.stil.as_ref().is_some_and(|s| s.lanes == 0)
        });
        // Release 14.10.2026: `stretch` (CSS Sizing 4 §3.1) in the cross axis
        // of a flex item and in a grid acts like stretch alignment
        // (`align-self: stretch` against the flex line or the grid area), so
        // the size stays `auto` there. Otherwise the style gets `100%` (the
        // box no longer counts as `auto`) and `box.rs` sizes the margin box
        // against the containing block.
        let (stretch_cross_w, stretch_cross_h) = if !ist_nicht_absolut {
            (false, false)
        } else if eltern.and_then(|e| e.anzeige).is_some_and(|a| matches!(a, narsil_css::Anzeige::Flex | narsil_css::Anzeige::InlineFlex)) {
            let column = eltern.and_then(|e| e.stil.as_ref()).and_then(|s| s.flex.as_deref()).is_some_and(|f| !f.reihe);
            (column, !column)
        } else if eltern.and_then(|e| e.anzeige) == Some(narsil_css::Anzeige::Raster) {
            (true, true)
        } else {
            (false, false)
        };
        // An in-flow block already fills the line with `width: auto` (next to
        // floats only the remaining space, WPT `stretch/bfc-next-to-float-1`);
        // `stretch` changes nothing there. Release 15.10.2026: not a flex or
        // grid item (a block-level child of a flex or grid container): there
        // `width: auto` does not fill the container. MEASURED before: WPT
        // css-sizing `stretch/stretch-inline-size-001` (a `width: stretch`
        // item of a row flex container was 10 instead of 45 px wide).
        let flex_parent = eltern.and_then(|e| e.anzeige).is_some_and(|a| matches!(a, narsil_css::Anzeige::Flex | narsil_css::Anzeige::InlineFlex));
        let in_flow_block = ist_nicht_absolut
            && !flex_parent
            && eltern.and_then(|e| e.anzeige) != Some(narsil_css::Anzeige::Raster)
            && mit_ua.schweben == narsil_css::Float::Keins
            && !mit_ua.inline_aussen
            && matches!(mit_ua.anzeige, narsil_css::Anzeige::Block | narsil_css::Anzeige::ListenEintrag | narsil_css::Anzeige::Flex | narsil_css::Anzeige::Raster);
        let stretch_cross_w = stretch_cross_w || in_flow_block;
        if let Some(e) = self.plan_stapel.last_mut() {
            e.hoehe_definit = eigene_definit;
            // Release 14.10.2026: `stretch` in the block axis only against a
            // definite height (otherwise it behaves as `auto`, CSS Sizing 4
            // §3.1), and not in the cross axis (see `stretch_cross_w`).
            if let Some(st) = e.stil.as_mut().filter(|s| s.stretch_bits != 0) {
                if !prozent_hoehe_gilt && ist_nicht_absolut {
                    st.stretch_bits &= !(8 | 16 | 32);
                }
                // Release 15.10.2026: in the cross axis of a flex item the
                // keyword stays as bit 64/128 (see `LayoutStil::stretch_bits`):
                // the item fills the container in the measure pass and its
                // line in the end, also with `align-self: start`. MEASURED
                // before: WPT css-sizing `stretch/flex-line-003` (a `width:
                // stretch` item in a 100 px column was 26 px wide),
                // `stretch/flexbox-flex-base-size-001` (100 instead of 200 px).
                if stretch_cross_h {
                    if flex_parent && autor.stretch_bits & 8 != 0 {
                        st.stretch_bits |= 128;
                    }
                    st.stretch_bits &= !8;
                }
                if stretch_cross_w {
                    if flex_parent && autor.stretch_bits & 1 != 0 {
                        st.stretch_bits |= 64;
                    }
                    st.stretch_bits &= !1;
                }
            }
            // Release 11.10.2026: `min-height`/`max-height` in Prozent gegen
            // eine NICHT definite Hoehe gelten nicht (CSS 2.1 §10.7: `0` bzw.
            // `none`). GEMESSEN vorher: WPT css-flexbox
            // `percent-height-flex-items-cross-sizes-with-mutations`
            // (`min-height: 100%` in einem Flex-Container ohne Hoehe: 800 px).
            if !prozent_hoehe_gilt && ist_nicht_absolut && !eltern_ist_element {
                if let Some(g) = e.grenzen_prozent.as_mut() {
                    // Die Kaskade hat daneben eine px-Zahl gegen die Ansicht
                    // stehen (Rueckfall in `box.rs`, `cmp_or`) — sie faellt mit.
                    let (min_weg, max_weg) = (g.min_hoehe.take().is_some(), g.max_hoehe.take().is_some());
                    if let Some(a) = e.grenzen.as_mut() {
                        if min_weg {
                            a.min_hoehe = None;
                        }
                        if max_weg {
                            a.max_hoehe = None;
                        }
                    }
                    if g.ist_leer() {
                        e.grenzen_prozent = None;
                    }
                }
            }
            // Derselbe Anteil steht auch im Plan (Flex-Quermass).
            if !prozent_hoehe_gilt && ist_nicht_absolut && autor.hoehe_prozent.is_some() {
                if let Some(st) = e.stil.as_mut() {
                    st.hoehe_anteil = None;
                    if (eltern_ist_element || eltern_ist_raster) && autor.mischung.as_deref().is_none_or(|m| m.hoehe.is_none()) {
                        st.hoehe_bedingt = autor.hoehe_prozent.filter(|p| p.is_finite() && *p >= 0.0);
                    }
                }
            }
        }
        if let (Some(st), true) = (stil.as_mut(), akzeptiert_groesse)
        {
            use narsil_core::length::Length as L;
            // Release 11.10.2026: auch absolute Kaesten behalten ihre
            // Prozentbreite/-hoehe; die Messung rechnet sie gegen den
            // enthaltenden Block (`box.rs`, `absolute_nachmessen`). GEMESSEN
            // vorher: `.a{position:absolute;width:100%;height:100%}` wurde
            // 0 x 0 px (WPT css-grid/abspos/positioned-grid-items-020).
            // Release 11.10.2026: `calc()` mit Prozent (CSS Values 4 §10.9).
            let misch = autor.mischung.as_deref().copied().unwrap_or_default();
            st.size.width = match (misch.breite, autor.breite_prozent, autor.breite) {
                (Some((a, p)), _, _) => L::gemischt(a, p),
                (None, Some(p), _) => L::Percent(p),
                (None, None, Some(v)) if v.is_finite() && v >= 0.0 => L::Px(v),
                _ => st.size.width,
            };
            st.size.height = match (autor.hoehe_prozent, autor.hoehe) {
                _ if misch.hoehe.is_some() && (!ist_nicht_absolut || prozent_hoehe_gilt) => {
                    let (a, p) = misch.hoehe.unwrap_or_default();
                    L::gemischt(a, p)
                }
                _ if misch.hoehe.is_some() => L::Auto,
                (Some(p), _) if !ist_nicht_absolut => L::Percent(p),
                (Some(p), _) if ist_nicht_absolut && prozent_hoehe_gilt => L::Percent(p),
                (Some(_), _) if ist_nicht_absolut => L::Auto,
                (None, Some(v)) if v.is_finite() && v >= 0.0 => L::Px(v),
                _ => st.size.height,
            };
        }
        // Release 14.10.2026: `width`/`height: stretch` become `100%` in the
        // style (see `stretch_cross_w`), also for replaced elements and form
        // controls. MEASURED before: WPT css-sizing
        // `stretch/stretch-block-size-001` (`block-size: stretch` was `auto`).
        if let Some(st) = stil.as_mut()
            && autor.stretch_bits & (1 | 8) != 0
            && (akzeptiert_groesse
                || matches!(
                    tag.to_ascii_lowercase().as_str(),
                    "img" | "video" | "svg" | "canvas" | "iframe" | "object" | "embed" | "input" | "textarea" | "button" | "select"
                ))
        {
            use narsil_core::length::Length as L;
            if autor.stretch_bits & 1 != 0 && !stretch_cross_w {
                st.size.width = L::Percent(100.0);
            }
            if autor.stretch_bits & 8 != 0 && !stretch_cross_h && (!ist_nicht_absolut || prozent_hoehe_gilt) {
                st.size.height = L::Percent(100.0);
            }
        }
        // Release 11.10.2026 (Agent CSS2): eine Tabellenzeile hat eine
        // Mindesthoehe (CSS 2.1 §17.5.3), aber keine Breite. GEMESSEN vorher
        // (WPT `collapsing-border-model-003`): `tr { height: 1in }` fehlte.
        if anzeige == narsil_css::Anzeige::TabelleZeile
            && let Some(st) = stil.as_mut()
            && let (None, Some(v)) = (autor.hoehe_prozent, autor.hoehe)
            && v.is_finite()
            && v >= 0.0
        {
            st.size.height = narsil_core::length::Length::Px(v);
        }
        if let Some(st) = stil.as_mut() {
            //
            // P9 (04.10.2026, 04:55): ZURUECKGENOMMEN — und der Grund ist
            // eine Messung, keine Vorsicht.
            //
            // Die Regel war: nur was die Kaskade als ZAHL liefert, wird
            // ueberschrieben, sonst wird die Hand-Typografie nicht
            // zerstoert. GEMESSEN, dass die Regel richtig ist:
            //     .ib { display:inline-block; width:60px; height:30px }
            //     vorher   #8 "A"  11x16     (11 px = gemessene Textbreite)
            //     mit      #8 "A"  60x30     die 60 px kamen an
            //
            // GEMESSEN, dass sie trotzdem SCHADET — der 30-Seiten-Lauf:
            //     MIT diesen zwei Zeilen    28/30   58 211 ms   46 hart
            //     OHNE diese zwei Zeilen   28/30   68 395 ms   10 hart
            // (ohne = exakt der Stand vom 02.10. 02:04)
            //
            // DIE URSACHE, gemessen an debian.org, Knoten 167:
            //     gesetzt: breite=Auto hoehe=Px(28.800001)  kinder=3
            //     Kasten  3840.0 x 28.8
            // 3840 = 3 x 1280. Jedes Kind ist EXAKT so breit wie der
            // Viewport. Das ist `width: 100%`.
            //
            // `narsil_css::kaskade::breiten_wert` (kaskade.rs:2835) loest
            // eine Laenge so:
            //     laenge_px(l, em, ansicht, ansicht, ansicht_hoehe)
            // Der Bezug fuer Prozent ist der ANSICHT, nicht der
            // enthaltende Block. Ein `width: 100%` kommt deshalb als
            // `Some(1280.0)` heraus — als ZAHL, ohne Einheit.
            //
            // Und ich habe diese Zahl als ABSOLUTE Laenge geschrieben.
            // Das ist der Fehler: 100 % der Seite statt 100 % des
            // Eltern.
            //
            // WARUM ICH ES NICHT REPARIEREN KANN: `Berechnet.breite_art`
            // traegt nur "max-content", "min-content", "fit-content" und
            // "stretch" (kaskade.rs:2072-2081) — die EINHEIT geht beim
            // Aufloesen verloren. Aus `Some(1280.0)` kann ich nicht
            // erkennen, ob es `100%` oder `1280px` war. Und der Bezug
            // (der enthaltende Block) weiss nur das LAYOUT.
            //
            // Die Entscheidung: eine rare Seite mit kaputter
            // `inline-block`-Breite ist ein kleinerer Schaden als 36
            // falsche 3840-px-Kaesten auf echten Seiten. Also weg.
            //
            // WAS ES BRAUCHT (narsil-css, zwei Wege, beide klein):
            //   a) `Berechnet` behaelt die Einheit, z. B.
            //      `breite_prozent: Option<f32>` neben `breite`. Dann
            //      schreibe ich nur absolute Laengen und lasse
            //      Prozent dem Layout.
            //   b) oder die Kaskade loest gar nicht auf und das Layout
            //      rechnet `width: 100%` gegen `behaelter.breite` — was
            //      die richtige Stelle ist, weil nur das Layout den
            //      enthaltenden Block kennt.
            //
            // GEMELDET mit Zahlen: team/PROBLEME.txt 04.10.2026, 04:55.
        }
        stil
    }

    /// Der einzige Eintiegspunkt fuer jedes Kind.
    ///
    /// M5: die Tiefenschranke steht HIER und nicht in `element`. Grund:
    /// `element` hat elf `return`-Zweige (unsichtbar, script, img, input,
    /// …), und in jedem davon muesste zurueckgezaehlt werden. Eine
    /// Stelle ist eine Stelle; elf sind elf Fehlerquellen.
    /// Release 08.10.2026: der Mantel um `kind_innen` — baut neue Knoten mit
    /// einer Lage (`insertAdjacentHTML`, `prepend`, `before`, `after`) an
    /// genau der Stelle im Dokument, an die das Skript sie gesetzt hat.
    fn kind(&mut self, knoten: &HtmlNode) {
        // Release 12.10.2026: im flachen Baum steht die Nummer am Knoten.
        if !self.ord_karte.is_empty()
            && let Some(&o) = self.ord_karte.get(&(knoten as *const HtmlNode as usize))
        {
            self.naechste_ordinal = o;
        }
        if !self.patch.neu.iter().any(|n| n.lage != NeuLage::Ende) {
            self.kind_innen(knoten);
            return;
        }
        let ord = self.naechste_ordinal;
        if let Some(eltern) = self.element_stapel.last().copied() {
            self.neue_an_lage_bauen(|n| n.lage == NeuLage::Anfang && n.eltern_ord == eltern);
        }
        self.neue_an_lage_bauen(|n| n.lage == NeuLage::Vor(ord));
        self.kind_innen(knoten);
        self.neue_an_lage_bauen(|n| n.lage == NeuLage::Nach(ord));
    }

    fn kind_innen(&mut self, knoten: &HtmlNode) {
        if self.tiefe >= MAX_BAU_TIEFE {
            // Der Inhalt unterhalb der Grenze wird nicht gezeigt. Das ist
            // eine Zusage, keine Notwendigkeit: eine Seite, deren Text in
            // 65 Ebenen verschachtelt ist, hat keinen sichtbaren
            // Unterschied zu einer, deren Text flach steht.
            return;
        }
        // W6: DIE VORLAUF-NUMMER. `kind()` wird in Dokumentreihenfolge
        // gerufen (die Schleife ueber `knoten`, dann rekursiv ueber die
        // Kinder), also ist der Zaehler genau die Reihenfolge im
        // geparsten Baum — und damit der Schluessel des Patches.
        //
        // GEMESSEN, warum ein Zaehler und kein Durchreichen: `kind()`
        // hat 10 Aufrufstellen (in `element()` fuer die Kinder). Ein
        // durchgereichter Index waere an 10 Stellen richtig zu halten —
        // und an der elften waere er falsch, ohne dass es auffaellt.
        let ord = self.naechste_ordinal;
        self.naechste_ordinal += 1;
        self.aktualer_ordinal = ord;
        //
        // W6, J10: `remove()` — der Knoten faellt ganz weg. VOR dem
        // `tiefe += 1`, weil ein Wegwerfen die Tiefe nicht erhoeht (und
        // sonst wuerde ein Skript mit 64 `remove`-Befehlen die Seite
        // hinter sich abschneiden).
        if self.patch.weg.contains(&ord) {
            return;
        }
        // Release 13.10.2026 (Agent CSS2): hinter dem Klemmpunkt einer
        // Zeilenklemme wird nichts mehr gesetzt (siehe `Klemme`).
        // Elemente entscheidet `stil_von_kaskade` (ein absolut positioniertes
        // bleibt sichtbar, CSS Overflow 4 §4.2).
        if self.klemmen.last().is_some_and(|k| k.rest == 0)
            && let HtmlNode::Text(t) = knoten
            && !t.chars().all(css_leerraum)
        {
            self.klemme_auslassen();
            return;
        }
        self.tiefe += 1;
        // W6E: die Nummer DIESES Knotens merken, damit ein Absatz, der
        // weiter unten (beim Textknoten) startet, das umschliesst Element
        // findet und nicht den Textknoten (16:55).
        //
        // GEMESSENER FEHLER, den die erste Fassung hatte: sie schob JEDEN
        // Knoten auf den Stapel, auch den Textknoten. GEMESSEN am Stapel:
        // `[0, 1, 4, 5]` bei `<button id="k">Klick mich</button>` — oben
        // stand der TEXTKNOTEN (5), nicht der Knopf (4). Also: NUR
        // Elemente. Ein Textknoten gehoert dem Absatz, und der Absatz
        // gehoert dem Element.
        //
        // 03.10. 23:30: DER WERT VOR DER REKURSION (siehe die Begruendung am
        // Plan-Zweig, Zeile 2981).
        //
        // GEMESSENER BEFUND (23:20, `NARSIL_MESS_PLAN=1`): `element_ist_text`
        // ist ein FELD, und `kind()` ruft sich rekursiv. Wenn `kind()`
        // zurueckkommt, steht dort der Wert des LETZTEN Kindes — bei einem
        // Block fast immer ein Textknoten. GEMESSEN: bei einem Element mit
        // gesetztem Kasten und vorhandenem Plan-Eintrag war `plan_knoten`
        // trotzdem `None`; der einzige Weg dahin ist `!self.element_ist_text`.
        //
        // GEMESSENE ENTSCHEIDUNG, warum ein LOKALER Wert und kein
        // Wiederherstellen des Feldes: das Feld wird an vielen Stellen gelesen,
        // und ein Retten muesste an jeder Stelle passen, an der die Rekursion
        // endet. Der lokale Wert beantwortet an EINER Stelle die richtige
        // Frage: „war ICH ein Textknoten?".
        let element_ist_text_hier = matches!(knoten, HtmlNode::Text(_));
        self.element_ist_text = element_ist_text_hier;
        if !self.element_ist_text {
            self.element_stapel.push(ord);
        }
        // W6: die Knotenlaenge VOR dem Bau dieses Knotens (siehe
        // `Bauer::dom_zu_ord`).
        let knoten_vor = self.doc.len();
        //
        // 03.10. 22:45: DER KASTEN DIESES ELEMENTS. GEMESSENE BEGRUENDUNG
        // fuer die Deklaration HIER und nicht im Element-Zweig: der Stapel
        // wird nach dem `match` geleert (Zeile 2942), und ein Textknoten
        // braucht dort `None`.
        let mut kasten_dieses: Option<narsil_dom::NodeId> = None;
        //
        // 03.10. 23:15: HAT DIESER AUFRUF EINEN PLAN-EINTRAG GESTELLT?
        //
        // GEMESSENE BEGRUENDUNG (Rolle LAYOUT, PROBLEME.txt 05:22, und bei
        // mir um 23:00 bestaetigt): `stil_von_kaskade` schiebt den Eintrag auf
        // `plan_stapel` — und das nur fuer ELEmente. Der `pop` am Ende von
        // `kind()` lief dagegen fuer JEDEN Knoten, auch fuer jeden
        // Textknoten. GEMESSENE FOLGE auf einer Seite mit 11 Knoten und
        // 5 Elementen: 11 Pops gegen 5 Eintraege, also sechsmal
        // `unwrap_or_default()` (kein Eintrag) und fuenfmal der Eintrag eines
        // ANDEREN Elements. GEMESSEN: `plan_len=0` auf einer Seite ganz ohne
        // CSS und `plan_len=1` auf der Flex-Seite (10 Knoten).
        //
        // GEMESSENE EINORDNUNG, warum die Schranke allein nicht reichte (die
        // Rolle LAYOUT hat das gemessen und die Schranke wieder entfernt,
        // 05:22): sie ist NOTWENDIG, aber nicht SUFFIZIENT — sie haelt nur den
        // Eintrag bei seinem Element. Wohin er gehoert, entscheidet der
        // Kasten-Stapel (23:00). Beides zusammen, nicht eines davon.
        let mut plan_geschoben = false;
        // Release 07.10.2026: `colspan`/`rowspan` einer Zelle (siehe unten).
        let mut zellen_spann: Option<(u16, u16)> = None;
        match knoten {
            HtmlNode::Text(t) => {
                // W6: `textContent = "…"` (Patch) gewinnt gegen den
                // geparsten Text. Sonst gibt es keinen Weg, den Text zu
                // aendern (GEMESSEN: `HtmlNode::Text` ist ein `String`,
                // und `narsil-dom` hat kein `set_text`).
                //
                // GEMESSEN, warum geklont wird: `self.patch.text.get()`
                // leiht `self` unveranderlich, `self.text()` veraendert
                // es — der Compiler gibt E0502. Die Kopie ist so gross
                // wie der neue Text, und der kommt aus einem Skript mit
                // einer Grenze (`narsil::js::MAX_ATTRIBUT`, 16 KiB).
                let neuer_text = self.patch.text.get(&ord).cloned();
                self.text(neuer_text.as_deref().unwrap_or(t.as_str()));
            }
            HtmlNode::Element {
                tag,
                attributes,
                children,
                shadow_root: _,
                ..
            } => {
                // W6: gepatchte Attribute (`class`, `style`,
                // `setAttribute`). Sie werden VOR der Kaskade gemischt,
                // damit sie auch rechnen — ein `classList.add`, das nur
                // im Attribut steht und nicht in der Kaskade, waere ein
                // Attribut ohne Wirkung.
                let eigene;
                let attributes = match self.attribut_mit_patch(ord, attributes) {
                    Some(a) => {
                        eigene = a;
                        &eigene
                    }
                    None => attributes,
                };
                // I2: die Kaskade wird HIER gefuellt und geleert, an
                // EINER Stelle. `element()` hat elf `return`-Zweige; in
                // jedem muesste der Wert wieder vom Stapel — und ein
                // vergessenes `pop` ist ein Fehler, der erst beim
                // ZWEITEN Geschwister sichtbar wird.
                let ist_link = tag.eq_ignore_ascii_case("a");
                if tag.eq_ignore_ascii_case("td") || tag.eq_ignore_ascii_case("th") {
                    let zahl = |name: &str| -> u16 {
                        attributes
                            .get(name)
                            .and_then(|v| v.trim().parse::<u32>().ok())
                            .map_or(1, |v| v.clamp(1, 1000) as u16)
                    };
                    zellen_spann = Some((zahl("colspan"), zahl("rowspan")));
                }
                //
                // `<script>` und `<style>` gehen VOR die Kaskade.
                //
                // GEMESSENER FEHLER 30.09.2026: das UA-Stylesheet hat
                // `script { display: none }` und `style { display: none }`
                // (gemessen: `Anzeige::Keins`). Damit kein `element()`
                // mehr aufgerufen wurde — und damit kein Skript und kein
                // Stylesheet gesammelt. Die Bomben-Tests meldeten "0
                // Skripte — erlaubt sind 512".
                //
                // Die Kaskade sagt hier "nicht zeichnen", und genau das
                // ist richtig: diese Elemente werden nicht gezeichnet,
                // sondern AUSGEWERTET (Skript) bzw. an die Kaskade
                // gerechnet (Stil). Das Auswerten gehoert nicht in den
                // Zweig, der zeichnet.
                // ACHTUNG: hier KEIN `return`. Ein `return` in diesem
                // Zweig wuerde `self.tiefe -= 1` unten umgehen — der
                // Tiefenzaehler waere nach 62 `<script>`-Elementen bei 64,
                // und der Rest der Seite waere stillschweigend
                // weggefallen. GEMESSEN: eine Seite mit 64 Skripten hatte
                // 62 Skripte und 65 Knoten statt 66; eine mit 1.000
                // Skripten hatte dieselben 62. `else` statt `return` ist
                // hier die richtige Form: der Zaehler darf sich an
                // GENAU EINER Stelle zuruecknehmen.
                //
                // 03.10. 22:40: DER KASTEN kommt aus dem Mantel (siehe
                // `element`), damit der Layout-Plan an den Knoten geht, der
                // zu DIESEM Element gehoert — und nicht an den ersten freien.
                if tag.eq_ignore_ascii_case("script")
                    || tag.eq_ignore_ascii_case("style")
                    // J-CACHE (05.10.2026): `<head>` GEHOERT DAZU.
                    //
                    // GEMESSEN: das UA-Blatt gibt `head { display: none }`,
                    // `stil_von_kaskade` liefert `None`, und der Zweig unten
                    // ueberspringt den GANZEN Kopf — mitsamt seiner
                    // `<script>`-Kinder. Auf jeder echten Seite stehen die
                    // Skripte aber IM KOPF: `var ytcfg = …` bei YouTube lief
                    // nie, die Body-Skripte riefen `ytcfg` und brachen mit
                    // `ReferenceError` ab. GEMESSEN mit der Parser-Sonde:
                    // der Baum ist richtig (`head` vor `body`), nur der BAU
                    // liess den Kopf aus.
                    || tag.eq_ignore_ascii_case("head")
                {
                    kasten_dieses = self.element(
                        tag,
                        attributes,
                        children,
                        self.css_adresse
                            .get(&(knoten as *const HtmlNode as usize))
                            .copied(),
                    );
                } else {
                    // GEMESSENER FEHLER 30.09.2026: `element()` wurde auch
                    // dann gerufen, wenn die Kaskade `display: none` gesagt
                    // hatte. `None` bedeutete nur "kein Stil" — also wurde
                    // das Element mit der Hand-Typografie GEZEICHNET. Der
                    // Test `display_none_blendet_aus` fand "Weg" im Dokument.
                    //
                    // 03.10. 23:15: hier wird der Plan-Eintrag gestellt
                    // (in `stil_von_kaskade`), also gehoert der `pop` am Ende
                    // zu diesem Aufruf. GEMESSENE BEGRUENDUNG: ohne diese
                    // Zeile stiehlt der erste Textknoten den Eintrag des
                    // Elementes, das vor ihm stand.
                    plan_geschoben = true;
                    let cascade_depth = self.kaskade.as_ref().map_or(0, |k| k.werte.len());
                    if let Some(stil) = self.stil_von_kaskade(knoten, tag, attributes, ist_link) {
                        self.kommender_stil = Some(stil);
                        //
                        // 03.10. 21:15: DER `cellpadding` STEHT HIER, weil
                        // dies die einzige Stelle ist, an der eine Tabelle
                        // beginnt und endet — `element()` kehrt an elf
                        // Stellen zurueck.
                        //
                        // 05.10. 22:15: `cellspacing` STEHT HIER AUS DEMSELBEN
                        // GRUND und liest dasselbe Attribut-Feld. GEMESSENE
                        // EINORDNUNG, warum er nicht CSS `border-spacing` ist:
                        // der Weg ueber CSS existiert (GEMESSEN: `border-
                        // spacing: 6px` kommt im Plan als `luecke` an), bringt
                        // aber bei uns **nichts** — auch nicht, weil unsere
                        // Tabellen linearisiert sind und die Gitter-Lage der
                        // Engine (`lege_tabelle`) dafuer nicht benutzt wird.
                        // GEMESSEN 22:07: alle drei Wege (Attribut, CSS
                        // `border-spacing`, CSS `gap` an `tr`) ergeben 19,2 px
                        // Abstand.
                        //
                        // GEMESSENE BEGRUENDUNG fuer die Grenze der Zahl:
                        // M4, `zahl_oder_null` (hteile.rs) prueft die Zahl VOR
                        // der Uebernahme; `cellpadding="99999"` liefert 0.
                        // GEMESSENE BEGRUENDUNG fuer den Vorgabewert: 0 (siehe
                        // das Feld).
                        let ist_tabelle = tag.eq_ignore_ascii_case("table");
                        if ist_tabelle {
                            let cp = attributes
                                .get("cellpadding")
                                .and_then(|w| w.trim().parse::<u32>().ok())
                                .filter(|n| *n <= MAX_CELLPADDING)
                                .unwrap_or(0);
                            self.cellpadding_stapel.push(cp);
                            // 05.10. 22:15: `cellspacing` mit derselben
                            // Begrenzung und demselben Vorgabewert 0.
                            let cs = attributes
                                .get("cellspacing")
                                .and_then(|w| w.trim().parse::<u32>().ok())
                                .filter(|n| *n <= MAX_CELLSPACING)
                                .unwrap_or(0);
                            self.cellspacing_stapel.push(cs);
                        }
                        kasten_dieses = self.element(
                            tag,
                            attributes,
                            children,
                            self.css_adresse
                                .get(&(knoten as *const HtmlNode as usize))
                                .copied(),
                        );
                        if ist_tabelle {
                            self.cellpadding_stapel.pop();
                            self.cellspacing_stapel.pop();
                        }
                    } else {
                        // Unsichtbare Teilbaeume behalten ihre DOM-Ordinalplaetze.
                        zaehle(children, self.tiefe, &mut self.naechste_ordinal, STEUERELEMENTE.contains(&tag.as_str()));
                        // Release 10.10.2026: Skripte laufen unabhaengig von
                        // `display` (HTML §4.12.1). GEMESSEN vorher (WPT
                        // `css3-selectors-lang-025`): ein `<script>` im
                        // unverschlossenen `<p lang=xx>` mit
                        // `[lang|=xx]{display:none}` lief nie.
                        self.skripte_im_verborgenen(children, 0);
                    }
                    // Restore the scope for visible and skipped elements alike.
                    if let Some(k) = self.kaskade.as_mut() {
                        k.werte.truncate(cascade_depth);
                    }
                    // `None` heisst `display: none` (oder
                    // `visibility: hidden`): das Element und seine Kinder
                    // werden nicht gebaut.
                }
            }
        }
        // W6: DER ERSTE NEUE KNOTEN gehoert zu diesem `HtmlNode`.
        // M3: die Schleife laeuft ueber die Knoten, die dieser Aufruf
        // erzeugt hat; `MAX_KNOTEN` begrenzt das Dokument.
        //
        // W6E: NUR UNBEANSPRUCHTE KNOTEN (30.09.2026, 16:55).
        //
        // GEMESSENER FEHLER, von der Koordination gefunden (16:40): 21
        // Klicks um den Knopf „Klick mich" in `zaehler.html` — `klick()`
        // lieferte immer `false`, der Zaehler blieb 0. GEMESSENE Ursache
        // (`ids` gegen `dom_zu_ord`): `ids["k"]` war 4, und DIE BRUECKE
        // schrieb 3 -> 4. DOM-Knoten 3 ist aber der KASTEN der
        // Ueberschrift, nicht des Knopfs: die Absatz-Ausgabe des `<h1>`
        // wird erst geschrieben, wenn der Knopf seinen Absatz beendet —
        // also WAEHREND `kind()` des Knopfs. `knoten_vor` (die Doc-Laenge
        // beim Eintritt) zeigte deshalb auf einen Knoten, der dem VORHERIGEN
        // Element gehoert, und der Knopf hat ihn mit seiner Nummer
        // ueberschrieben. Folge: der Klick auf den Knopf fand keinen
        // Handler (die Bruecke nannte 5), und ein Klick auf die
        // Ueberschrift zaehlte — mein eigener Test hat genau das getan und
        // war gruen.
        //
        // DIE REGEL: EIN KNOTEN GEHOERT DEM ERSTEN, DER IHN BEANSPRUCHT.
        // Der Absatz eines Elements traegt seine Nummer ueber
        // `laufender_ord` (siehe `absatz_schliessen`) — der Knopf darf
        // sie nicht uebernehmen.
        //
        // W6F: DIE NEUEN KINDER ANHAENGEN (30.09.2026, 17:40).
        //
        // `document.createElement('li')` + `appendChild` erzeugen einen
        // Knoten, den der Parse nicht kennt. Hier — am Ende von `kind()`
        // fuer den ELTERNknoten — wird er erzeugt: ein Container mit dem
        // Text, als DOM-Knoten unter dem Knoten dieses Aufrufs.
        //
        // GEMESSEN, warum HIER und nicht im `Patch`: `kind()` ist die
        // einzige Stelle, die den DOM-Knoten des Elternknotens hat
        // (`knoten_vor`) und die Nummer (`ord`) — und beide braucht der
        // neue Knoten. In `absatz_schliessen` stuende kein Elternknoten
        // bereit (dort wird Text geschrieben, keine Elemente).
        //
        // M3/M4: `NeuKnoten::MAX_NEU` (256) je Seite, der Text
        // `NeuKnoten::MAX_TEXT` (4 KiB). Beides wird VOR der Allokation
        // geprueft.
        if !self.patch.neu.is_empty() {
            self.neue_kinder_anhaengen(ord, knoten_vor);
        }
        //
        // W6E: DER ERSTE FREIE KNOTEN, den dieser Aufruf erzeugt hat
        // (16:55). `knoten_vor` ist die Doc-Laenge beim Eintritt — und ein
        // Absatz, der dabei geschrieben wird, gehoert dem VORHERIGEN
        // Element. Deshalb wird nicht blind `knoten_vor` genommen,
        // sondern der erste Knoten ab dort, den noch niemand beansprucht
        // hat. GEMESSEN an `zaehler.html`: der Knopf beansprucht so seinen
        // Kasten und laesst den Kasten der Ueberschrift der Ueberschrift.
        //
        // M3: hoechstens `MAX_KNOTEN_VON_ELEMENT` (8) Knoten werden
        // durchsucht; danach ist Schluss. Ein Element, das weiter kommt,
        // laesst den Rest unbeansprucht — ein Knoten OHNE Nummer ist
        // harmlos (der Klick laeuft zum Elternkasten), eine FALSCHE Nummer
        // nicht (sie gehoert einem anderen Handler).
        //
        // W7C R3 (02.10. 03:40): DER PLAN, und zwar HIER — weil HIER der
        // Knoten bekannt ist, der diesem Element gehoert. GEMESSEN, warum
        // nicht im CSS-Baum-Bau (der Wunsch nennt ihn): dort gibt es noch
        // keine DOKUMENTknoten, und der Plan ist nach denen sortiert. Und
        // nicht in `stil_von_kaskade`: dort kennt man die Nummer des
        // Knotens nicht, nur die des CSS-Elements.
        //
        // Der Stapel wird bei JEDEM Element gepoppt — auch dann, wenn der
        // Knoten unterhalb der Grenze (`MAX_KNOTEN_VON_ELEMENT`) unbeansprucht
        // bleibt. Sonst stuende beim naechsten Element der Eintrag eines
        // anderen auf der Spitze, und `display:flex` waere an der falschen
        // Stelle.
        //
        // W7C R3 (02.10., Rolle LAYOUT): Der Plan wird HIER gesetzt, und
        // der Kasten kommt aus dem STAPEL — nicht aus einem Raten.
        //
        // GEMESSENER FEHLER, den die Rolle HTML dreimal gemacht hat (sie
        // `team/UEBERGABE_HTML_R3R4.txt`): der Kasten dieses Elements war
        // ein FLACHES Feld (`eigener_kasten`), und die Kinder werden vor
        // dem Abschluss des Eltern gebaut. Jedes Kind hat den Kasten
        // seines Eltern ueberschrieben. Drei Varianten, den richtigen
        // Kasten zu raten, waren alle falsch:
        //   erster freier Knoten  -> der Knoten des ersten Kindes
        //   letzter Knoten         -> der Textknoten des letzten Kindes
        //   Notizfeld             -> der Kasten des letzten Nachfahren
        // Die Loesung ist kein vierter Versuch, sondern ein Stapel: was
        // der Elternknoten hatte, steht beim Eintritt wieder da.
        //
        // GEMESSENE WARNUNG: die Reihenfolge hier ist wichtig. Der
        // Abschluss von `element()` muss VOR dem Aufbau der Kinder
        // stimmen. GEMESSEN: die Rolle HTML hat drei Varianten
        // gemessen (erster freier Knoten, letzter Knoten, Notizfeld), alle
        // drei falsch — siehe `team/UEBERGABE_HTML_R3R4.txt`.

        // P10 (05.10.2026, 05:22) — EINE HYPOTHESE, GEMESSEN UND
        // VERWORFEN. Hier stand zwei Minuten lang eine Schranke
        // `if self.element_ist_text { … } else { pop }`, mit der
        // Begruendung, jeder Textknoten stehle einem Element seinen
        // Plan-Eintrag. GEMESSEN: die Zahl blieb 1 von 18 Knoten. Die
        // Schranke ist also nicht die Ursache (sie ist trotzdem richtig
        // und haengt als TODO-Text weiter unten), und ich habe sie
        // entfernt, weil eine unbewiesene Aenderung in einer fremden
        // Datei nicht dorthin gehoert. Die ECHTE Ursache steht in
        // team/PROBLEME.txt, 05:22.
        //
        // 03.10. 23:15: DER POP NUR WENN GESCHOBEN WURDE (siehe 23:15 oben
        // und PROBLEME.txt 05:22 der Rolle LAYOUT).
        let mut plan_eintrag = if plan_geschoben {
            self.plan_stapel.pop().unwrap_or_default()
        } else {
            PlanEintrag::default()
        };
        // Release 12.10.2026 (Agent CSS2): steht der naechste positionierte
        // Vorfahr eines absoluten Kastens als Inline-Element ohne eigenen
        // Kasten im Fluss, ist er trotzdem der enthaltende Block (CSS 2.1
        // §10.1, Punkt 4) — die Layout-Engine naehert ihn an der statischen
        // Lage (`LayoutStil::cb_inline`). GEMESSEN vorher (Koordinator, WPT
        // css-position `position-absolute-dynamic-relayout-005/006`): `width:
        // 80%` galt gegen die Ansicht (640 px Rot).
        if let Some(st) = plan_eintrag.stil.as_mut()
            && st.position == narsil_layout::PositionArt::Absolut
            && let Some(vorfahr) = self
                .plan_stapel
                .iter()
                .rev()
                .find(|p| p.stil.as_ref().is_some_and(|s| s.position != narsil_layout::PositionArt::Statisch))
        {
            st.cb_inline = vorfahr.anzeige == Some(narsil_css::Anzeige::Inline);
        }
        // `eigener_kasten` traegt jetzt den Kasten DIESES Elements:
        // `element()` hat ihn beim Eintritt gerettet und beim Ausgang
        // wiederhergestellt, die Kinder haben ihn dazwischen gesetzt und
        // wieder verworfen.
        let eigener = self.eigener_kasten.take();

        let mut plan_knoten: Option<u32> = None;
        // Release 13.10.2026 (Agent CSS2): `plan_knoten` ist der erste Knoten
        // des Elements (meist seine erste Zeilenbox), nicht sein Kasten.
        let mut aus_rueckfall = false;
        // Release 12.10.2026: ein Textlauf nur aus Leerraum ist nie der Kasten eines Elements.
        // Ein Absatz, der beim Eintritt geschrieben wird (der Leerraum vor
        // einem `inline-block`), legt Textknoten ab `knoten_vor` an; der
        // erste freie davon nahm dem Element die Nummer weg. GEMESSEN vorher
        // (WPT css-align/abspos, die htb-Dateien): ab dem zweiten `inline-block` einer
        // Zeile meldete `getBoundingClientRect` den Leerraum (4 x 19 bei
        // y = 74), der Kasten des Elements hatte keine Nummer.
        for i in knoten_vor..(knoten_vor + MAX_KNOTEN_VON_ELEMENT).min(self.doc.len()) {
            let k = i as u32;
            if self.dom_zu_ord.contains_key(&k) {
                continue;
            }
            let ist_lauf = self.doc.get(NodeId(k)).is_some_and(|n| {
                n.kind == NodeKind::Text && n.text.as_deref().is_some_and(|t| t.chars().all(css_leerraum))
            });
            if ist_lauf {
                continue;
            }
            self.dom_zu_ord.insert(k, ord);
            break;
        }
        //
        // W7C R3 (04:55): der Kasten kommt aus dem STAPEL der Rolle PROBE
        // (`kasten_stapel`), nicht aus einem eigenen Feld. Mein
        // `eigener_kasten` ist beim Umbau auf `kasten_stapel` mit
        // entfallen; die Reihenfolge beider Stapel ist dieselbe (ein
        // Eintrag je `element()`-Aufruf).
        //
        // 03.10. 22:40: der Stapel ist im Mantel gepoppt; hier kommt der
        // Kasten DIESES Elements an (bei einem Textknoten `None`).
        let kasten_aus_dem_stapel = kasten_dieses;

        // Flattened inline elements have no box to receive a layout plan.
        // In particular, never overwrite an inline image with its link's plan.
        if plan_eintrag.stil.is_some() && !element_ist_text_hier
            && (plan_eintrag.anzeige != Some(narsil_css::Anzeige::Inline)
                || kasten_aus_dem_stapel.is_some() || plan_eintrag.knoten.is_some())
        {
            // 03.10. 23:30: `element_ist_text_hier` statt `self.element_ist_text`
            // — das Feld traegt nach der Rekursion den Wert des LETZTEN
            // Kindes (GEMESSEN, siehe 23:30 weiter oben).
            // Der Kasten aus dem Stapel; der Notnagel gilt nur, wenn
            // dieses Element wirklich keinen eigenen bekommen hat (ein
            // Inline-Element mit leerem Inhalt).
            //
            // 03.10. 23:25: DER KASTEN AUS DEM STAPEL GEHOERT HIER HER.
            //
            // GEMESSEN (23:20, `NARSIL_MESS_PLAN=1`, Wegwerf-Instrumentierung
            // in dieser Crate, wieder entfernt): vorher stand hier
            // `plan_eintrag.knoten.or(eigener)`, und GEMESSEN war
            //     PLAN kind: geschoben=true stil=true plan_knoten=None
            //            kasten=Some(NodeId(4)) eigener=Some(NodeId(3))
            // Der Stapel hatte den Kasten, und er wurde nicht benutzt — genau
            // das hatte die Rolle LAYOUT um 05:22 geschrieben („die lokale
            // Variable `kasten_aus_dem_stapel` wird danach NIRGENDS
            // benutzt").
            //
            // GEMESSENE REIHENFOLGE fuer die drei Quellen: erst der ausdruecklich
            // benannte Knoten (`plan_eintrag.knoten`), dann der Kasten, den
            // `element()` zurueckgibt, dann das alte Feld `eigener_kasten`
            // (das der Plan-Stapel nicht kennt). GEMESSENE BEGRUENDUNG: der
            // Kasten aus dem Mantel ist der FRISCHE — er wurde in diesem
            // Aufruf gesetzt —, und das alte Feld kann (05:22) den Kasten des
            // Eltern tragen.
            let knoten = plan_eintrag
                .knoten
                .or(kasten_aus_dem_stapel)
                .or(eigener)
                .map(|k| k.0);
            // Release 08.10.2026: der ERSATZknoten (der erste nach dem
            // Elementanfang) nur, wenn er noch keinen echten Plan hat. Er
            // gehoert sonst einem Nachfahren, der schon fertig ist — GEMESSEN
            // auf tagesschau.de: `<li>` ohne Marke hat keinen eigenen Knoten,
            // sein Plan landete auf dem absolut positionierten `<a>` darin und
            // loeschte dessen `position:absolute; left:-10000px` (die
            // Sprunglinks standen sichtbar oben links).
            aus_rueckfall = knoten.filter(|i| (*i as usize) < self.doc.len()).is_none();
            plan_knoten = knoten
                .filter(|i| (*i as usize) < self.doc.len())
                .or_else(|| {
                    Some(knoten_vor as u32).filter(|i| {
                        (*i as usize) < self.doc.len()
                            && self
                                .layout_plan
                                .hole(narsil_dom::NodeId(*i))
                                .is_none_or(|p| *p == narsil_layout::LayoutStil::default())
                    })
                });
        }
        if let (Some(mut bild), Some(node_nr)) = (plan_eintrag.hintergrundbild, plan_knoten) {
            // Release 14.10.2026 (Agent CSS2): the viewport for `fixed`.
            // MEASURED before (WPT `CSS2/backgrounds/background-attachment-
            // applies-to-*`): the image started at the element instead of the
            // top of the viewport.
            // Release 14.10.2026 (Agent CSS2): and for the root element, whose
            // image paints the canvas (see `BildGeo::paints_canvas`). MEASURED
            // before (WPT `CSS2/backgrounds/background-root-002`, `html {
            // background: url(..) repeat-x; margin: 1in }`): the stripe ended
            // at the root's margin edge.
            let is_root = matches!(knoten, HtmlNode::Element { tag, .. } if tag.eq_ignore_ascii_case("html"));
            if is_root && bild.1.border_image.is_none() {
                bild.1.paints_canvas = true;
            }
            // Release 14.10.2026 (Agent CSS2): the body's image goes to the
            // canvas when the root has no background (§2.11.2). MEASURED
            // before (WPT `CSS2/backgrounds/background-position-001`, `body {
            // background: url(..) repeat-x 0 6.25ex; margin-left: 75px }`): the
            // line started at the body's edge, 1 px too low.
            let is_body = matches!(knoten, HtmlNode::Element { tag, .. } if tag.eq_ignore_ascii_case("body"));
            if is_body
                && bild.1.border_image.is_none()
                && !self.root_has_image
                && !self.canvas_von_html
                && !self.canvas_blocked
            {
                bild.1.paints_canvas = true;
                bild.1.from_body = true;
            }
            if (bild.1.ursprung == 3 || bild.1.paints_canvas) && self.breite > 0 && self.ansicht_hoehe > 0 {
                bild.1.viewport = Some((self.breite as f32, self.ansicht_hoehe as f32));
            }
            self.hintergrundbilder
                .insert(narsil_dom::NodeId(node_nr), bild);
        }
        if let Some(maske) = plan_eintrag.maskenbild {
            let maske = self.masken_aufloesen(maske);
            if let Some(node_nr) = plan_knoten {
                self.maskenbilder.insert(narsil_dom::NodeId(node_nr), maske);
            } else if let Some(mask_key) = plan_eintrag.mask_key.take() {
                // Release 15.10.2026 (Agent F): an inline element without a
                // box of its own masks its text: its text nodes (written
                // later, with the paragraph) carry the mask, placed in each
                // line box (WPT css-masking mask-image-6).
                self.inline_masks.push((mask_key, maske));
            }
        }
        if let (Some(mut stil), Some(knoten)) = (plan_eintrag.stil, plan_knoten) {
            // Release 07.10.2026: `colspan`/`rowspan` sind HTML-Attribute, keine
            // CSS-Werte — die Kaskade kennt sie nicht, und der Plan trug darum
            // IMMER 1 (`layout_plan.rs`). GEMESSEN: auf news.ycombinator.com
            // stand die Unterzeile (`<td colspan=2></td><td class=subtext>`) in
            // der Pfeilspalte, und Wikipedias Infobox-Titel nur über der
            // ersten Spalte. HTML §4.9.11: 1..1000 bzw. 0..65534.
            if let Some((spalten, zeilen)) = zellen_spann {
                stil.colspan = spalten;
                stil.rowspan = zeilen;
            }
            self.layout_plan.set(narsil_dom::NodeId(knoten), stil);
        }
        // Release 11.10.2026 (Agent CSS2): Tabellenrolle und `vertical-align`
        // (siehe die Felder in `PlanEintrag`).
        if let Some(knoten) = plan_knoten {
            self.layout_plan.set_tabelle(narsil_dom::NodeId(knoten), plan_eintrag.tabelle);
            self.layout_plan.set_vertikal(narsil_dom::NodeId(knoten), plan_eintrag.vertikal);
        }
        // Release 13.10.2026 (Agent CSS2): die Ausrichtung eines Absatzes ohne
        // eigenen Kasten tragen seine Zeilen (`zeilen_schreiben`, je Zeile mit
        // `text-align-last`) — die des Elements darf sie an der ersten Zeile
        // nicht ueberschreiben. GEMESSEN vorher (WPT css-text `text-align/
        // text-align-last-end`, `-center`): die erste Zeile stand nach
        // `text-align`.
        if let (Some(a), Some(knoten)) = (plan_eintrag.ausrichtung.filter(|_| !aus_rueckfall), plan_knoten) {
            self.layout_plan.set_ausrichtung(narsil_dom::NodeId(knoten), a);
        }
        // W7C R3: `max-width`/`min-width` kommen aus demselben Ort und
        // gehoeren in dieselbe Nebentabelle. Ohne sie tut `img { max-width:
        // 100% }` nichts, und ein Bild laeuft aus dem Fenster (gemessen
        // auf nixos.org: 7129 px).
        if let Some(g) = plan_eintrag.grenzen {
            if let Some(knoten) = plan_knoten {
                self.layout_plan.set_grenzen(narsil_dom::NodeId(knoten), g);
            }
        }
        if plan_eintrag.rtl
            && let Some(knoten) = plan_knoten
        {
            self.layout_plan.set_richtung(narsil_dom::NodeId(knoten), narsil_layout::Richtung::Rtl);
        }
        // P19 (20:39): dieselbe Nebentabelle fuer die Prozentanteile.
        if let Some(g) = plan_eintrag.grenzen_prozent {
            if let Some(knoten) = plan_knoten {
                self.layout_plan
                    .set_grenzen_prozent(narsil_dom::NodeId(knoten), g);
            }
        }
        // Release 10.10.2026: die `::first-letter`-Auftraege dieses Elements
        // enden mit ihm; geparkte der Vorfahren kommen zurueck.
        if !self.sprachen.is_empty() {
            let t = self.tiefe;
            self.sprachen.retain(|(tiefe, _)| *tiefe < t);
        }
        // Release 11.10.2026: die Zaehler der Kinder enden mit ihrem Eltern,
        // `quotes` mit dem eigenen Element.
        if !self.zaehler.is_empty() || !self.zitate.is_empty() || !self.nach_wartend.is_empty() {
            let t = self.tiefe;
            self.zaehler.retain(|z| z.2 <= t);
            self.zitate.retain(|(tiefe, _)| *tiefe < t);
            self.nach_wartend.retain(|(tiefe, _)| *tiefe < t);
        }
        if !self.erste_zeile.is_empty() || !self.erste_zeile_geparkt.is_empty() {
            let t = self.tiefe;
            self.erste_zeile.retain(|a| a.tiefe < t);
            if self.erste_zeile_geparkt.last().is_some_and(|(g, _)| *g == t)
                && let Some((_, geparkt)) = self.erste_zeile_geparkt.pop()
            {
                self.erste_zeile = geparkt;
            }
        }
        if !self.erster_buchstabe.is_empty() || !self.erster_buchstabe_geparkt.is_empty() {
            let t = self.tiefe;
            self.erster_buchstabe.retain(|a| a.tiefe < t);
            if self.erster_buchstabe_geparkt.last().is_some_and(|(g, _)| *g == t)
                && let Some((_, geparkt)) = self.erster_buchstabe_geparkt.pop()
            {
                self.erster_buchstabe = geparkt;
            }
        }
        // W6E: M6 — der Stapel waechst und schrumpft im Gleichschritt
        // (16:55). Ein `pop` fehlt: nach 64 Ebenen nimmt `kind()` nichts
        // mehr an (`MAX_BAU_TIEFE`), die Seite waere ab da stumm.
        //
        // Release 14.10.2026: the element leaves `element_stapel` again (the
        // pop was lost in the snapshot f40d2c82 of 04.10.). Without it the top
        // of the stack was the last element ENTERED anywhere, so a paragraph
        // started after a closed element (`absatz_starten`) took the number of
        // its last descendant. MEASURED before: WPT css-align
        // `abspos/align-self-htb-ltr-htb` (the whitespace line of a `<br>`
        // after an inline-block belonged to the abspos child inside it,
        // `getBoundingClientRect` of that child was 72 x 69 instead of 40 x 40).
        if !element_ist_text_hier {
            self.element_stapel.pop();
        }
        self.tiefe -= 1;
    }

    /// W6F: die neuen Knoten anhaengen, deren Eltern dieser Knoten ist.
    ///
    /// # DER WEG: DERSELBE WEG WIE BEIM LADEN
    ///
    /// GEMESSENER FEHLER, den die erste Fassung hatte (30.09.2026, 18:40,
    /// Bildschirmfoto `zaehler.png` der Koordination): die erzeugten `<li>`
    /// standen INLINE hinter „Stand: 3" und lagen uebereinander — nur
    /// „Klick Nr. 1" war lesbar. Gemessen: alle drei Texte auf y = 187.6
    /// (genau der y von „Stand: 3"), x stepping by 784 px — die volle
    /// Seitenbreite je Punkt. GEMESSEN zum Vergleich eine PARSE-Liste
    /// (`<ul><li>eins</li><li>zwei</li></ul>`): „* eins" bei y 119.2,
    /// „* zwei" bei y 138.4 — zwei Zeilen, mit Listenpunkt, eingerueckt.
    ///
    /// Die Ursache war nicht das Layout, sondern der WEG: die erste Fassung
    /// baute einen `Container` von Hand mit `Style::default()`. Das heisst:
    /// OHNE Kaskade (also ohne das UA-Stylesheet, in dem `li` als
    /// `list-item` auf Block steht), ohne Absatzabstand, ohne
    /// Aufzaehlungszeichen, ohne Einrueckung — und mit der
    /// Standardrichtung `Row`, also nebeneinander statt untereinander.
    ///
    /// `element()` ist der Weg, den ein geparster Knoten geht: Kaskade
    /// rechnen, `ist_block` pruefen, den Absatz schliessen, die Box bauen,
    /// den Text als Absatz schreiben. Der neue Knoten baut sich also einen
    /// `HtmlNode` und ruft genau das. Es ist eine Kette von einem Element
    /// (die Liste kann verschachtelt sein), und beide Grenzen bleiben
    /// dieselben: `MAX_NEU` (256) Knoten, `MAX_TEXT` (4 KiB) je Knoten.
    ///
    /// GEMESSEN, warum `aktualer_ordinal` und `element_stapel` gesetzt
    /// werden: `element()` liest die Nummer des Elements an beiden
    /// Stellen. Ohne sie bekam der Absatz des Listenpunkts die Nummer des
    /// Elternabsatzs — und ein Klick auf den Punkt truefe den Absatz
    /// statt des Elements (das ist der Fehler aus W6E, an anderer Stelle).
    fn neue_kinder_anhaengen(&mut self, eltern_ord: usize, eltern_vor: usize) {
        // M3: die Liste ist die Reihenfolge des `appendChild`.
        let neu: Vec<NeuKnoten> = self
            .patch
            .neu
            .iter()
            // Release 08.10.2026: nur „am Ende" — und ein „am Anfang", das
            // mangels Kind noch nicht gebaut wurde (leeres Element).
            .filter(|n| {
                n.eltern_ord == eltern_ord
                    && (n.lage == NeuLage::Ende || n.lage == NeuLage::Anfang)
                    && !self.neu_gebaut.contains(&n.ord)
            })
            .take(NeuKnoten::MAX_NEU)
            .cloned()
            .collect();
        if neu.is_empty() {
            return;
        }
        // Der DOM-Knoten des Elternknotens muss da sein. Fehlt er (ein
        // Element ohne Box), gibt es nichts, woran man anhaengen koennte —
        // und das ist eine Eigenschaft, kein Fehler.
        if self.doc.get(NodeId(eltern_vor as u32)).is_none() {
            return;
        }
        //
        // W6G: DER EINZUG DES ELTERNS (18:55). `self.einzug` ist beim
        // Aufruf hier wieder 0 — der Container wurde unterwegs verlassen.
        // Ohne das stehen die angehaengten Kinder ohne Listen-Einzug
        // (gemessen: x = 48 statt 100).
        //
        // GEMESSEN, warum nur dieser eine Wert: die Kinder eines
        // Listenpunkts bekommen den Einzug ihres Listenpunkts (der steht
        // in `einzug_von_ord` unter der Nummer des Listenpunkts, und
        // `element()` setzt ihn beim Bauen von selbst).
        let einzug_parents = self.einzug_von_ord.get(&eltern_ord).copied().unwrap_or(0.0);
        let einzug_gesichert = self.einzug;
        let textbreite_gesichert = self.textbreite;
        self.textbreite = self.textbreite_von_ord.get(&eltern_ord).copied()
            .unwrap_or((self.breite as f32 - 2.0 * SEITENRAND - einzug_parents).max(1.0));
        self.einzug = einzug_parents;
        //
        // W6G: DER NUMMERNZAEHLER BLEIBT, WO ER WAR (19:05).
        //
        // GEMESSENER FEHLER: `element()` baut die Kinder mit `self.kind()`,
        // und `kind()` nimmt fuer JEDEN Knoten eine Vorlauf-Nummer. Der
        // Text des neuen Listenpunkts nahm damit eine Nummer des Parse-
        // Zaehlers — und alle Knoten DANACH rutschten um eins. Gemessen:
        // der `<ul>` (Nummer 10 im Stand) stand nach dem Klick auf 11, nach
        // dem zweiten auf 12. Zwei Folgen: `Stand::ord` passte nicht mehr zu
        // `Seite::dom_zu_ord` (ein Klick auf den alten Absatz traef den
        // falschen), und der Einzugs-Lookup oben fand nichts.
        //
        // GEMESSEN, warum das Zuruecksetzen erlaubt ist: die Nummern
        // INNERHALB des neuen Teilbaums werden nirgends nach aussen
        // gebraucht — nach aussen traegt nur die Nummer des Elements
        // (`NeuKnoten::ord`, aus `ORD_BASIS`). Der Zaehler steht danach
        // wieder da, wo er vor dem Anhaengen war, und der naechste
        // geparste Knoten bekommt seine Nummer wie beim Laden.
        let naechste_gesichert = self.naechste_ordinal;
        for n in neu {
            self.neuen_knoten_bauen(&n);
        }
        self.einzug = einzug_gesichert;
        self.textbreite = textbreite_gesichert;
        self.naechste_ordinal = naechste_gesichert;
    }

    /// Ein HTML-Element und seine Kinder.
    /// `css_index` ist die Nummer des Knotens im `CssBaum` — sie kommt
    /// aus `kind`, wo der `HtmlNode` noch da ist. W6 (J9) braucht sie,
    /// um Selektor-Treffer auf DOM-Knoten zu uebersetzen.
    /// W7C A4: `<title>` UND `<link rel="icon">` AUSWERTEN.
    ///
    /// GEMESSEN, warum das EINE Methode ist: beide Angaben kommen aus dem
    /// KOPF, beide werden nicht gezeichnet, und beide brauchen dieselbe
    /// Quelle (die Attribute, solange der Parser sie hat — danach sind sie
    /// weg, `narsil-dom` verwirft sie).
    ///
    /// GEMESSENER BEFUND (08:15): beides stand im Baum wie normaler Text —
    /// der Seitentitel als Zeile im Bild („Start — narsil" war ueberall
    /// gleich), und das Favicon wurde nie bemerkt.
    /// Baut EINEN neuen Knoten aus dem Patch an der aktuellen Stelle
    /// (unter `self.mutter()`), mit seinen neuen Kindern.
    fn neuen_knoten_bauen(&mut self, n: &NeuKnoten) {
        if !self.neu_gebaut.insert(n.ord) {
            return;
        }
        {
            let ord = n.ord;
            fn inserted_children(patch: &Patch, parent: usize, depth: usize) -> Vec<HtmlNode> {
                if depth > 64 {return Vec::new();}
                patch.neu.iter().filter(|child| child.eltern_ord == parent).map(|child| {
                    let mut children = inserted_children(patch, child.ord, depth+1);
                    if let Some(text) = &child.text {children.insert(0, HtmlNode::Text(text.clone()));}
                    let attributes = patch.attribut.iter().filter(|((ord, _), _)| *ord == child.ord)
                        .map(|((_, name), value)| (name.clone(), value.clone())).collect();
                    HtmlNode::Element {tag:child.tag.clone(),attributes,children,shadow_root:None}
                }).collect()
            }
            let mut kinder = inserted_children(&self.patch, ord, 0);
            if let Some(text) = &n.text {kinder.insert(0, HtmlNode::Text(text.clone()));}
            let attributes: HashMap<String, String> = self.patch.attribut.iter()
                .filter(|((node, _), _)| *node == ord)
                .map(|((_, name), value)| (name.clone(), value.clone())).collect();
            let vor = self.doc.len();
            // Die Nummer des neuen Elements, ueberall dort, wo `element()`
            // sie liest (siehe oben).
            self.aktualer_ordinal = ord;
            self.element_stapel.push(ord);
            // Release 08.10.2026: die Kinder eines neuen Knotens zählen in
            // einem EIGENEN Bereich weiter — sonst trügen sie Vorlauf-Nummern
            // echter Elemente, und ein Lage-Haken (`Vor`/`Nach`) löste am
            // falschen Knoten aus.
            let zaehler = self.naechste_ordinal;
            self.naechste_ordinal =
                NeuKnoten::ORD_BASIS * 2 + ord.saturating_sub(NeuKnoten::ORD_BASIS) * 4096;
            self.element(&n.tag, &attributes, &kinder, None);
            self.naechste_ordinal = zaehler;
            self.element_stapel.pop();
            // W6E: DER ERSTE FREIE KNOTEN ist der des neuen Elements (hier
            // kann es keinen Absatz-Fremdknoten geben, der vorher gehoert).
            for i in vor..(vor + MAX_KNOTEN_VON_ELEMENT).min(self.doc.len()) {
                let k = i as u32;
                if !self.dom_zu_ord.contains_key(&k) {
                    self.dom_zu_ord.insert(k, ord);
                    break;
                }
            }
        }
        // M6: zurueck auf den Stand VOR dem Anhaengen — der Aufrufer
        // rechnet weiter mit seinem eigenen Einzug und seinen Nummern.
    }

    /// Release 08.10.2026: neue Knoten an einer LAGE (vor/nach einem
    /// Element, am Anfang seines Elternteils) — gebaut im Kontext des
    /// Elternteils, Zustand des Bauers wie in `neue_kinder_anhaengen`.
    fn neue_an_lage_bauen(&mut self, passt: impl Fn(&NeuKnoten) -> bool) {
        let neu: Vec<NeuKnoten> = self
            .patch
            .neu
            .iter()
            .filter(|n| passt(n) && !self.neu_gebaut.contains(&n.ord))
            .take(NeuKnoten::MAX_NEU)
            .cloned()
            .collect();
        if neu.is_empty() {
            return;
        }
        // Ein offener Absatz gehört VOR die neuen Knoten.
        self.absatz_schliessen();
        let naechste = self.naechste_ordinal;
        let aktuell = self.aktualer_ordinal;
        for n in &neu {
            self.neuen_knoten_bauen(n);
        }
        self.naechste_ordinal = naechste;
        self.aktualer_ordinal = aktuell;
    }

    fn titel_und_favicon(
        &mut self,
        attributes: &std::collections::HashMap<String, String>,
        kinder: &[HtmlNode],
        tag: &str,
    ) {
        if tag.eq_ignore_ascii_case("title") {
            let mut text = String::new();
            for k in kinder {
                if let HtmlNode::Text(t) = k {
                    text.push_str(t);
                }
            }
            // M4: 200 Zeichen. `weissraum` macht aus einem Titel mit
            // Zeilenumbruechen eine Zeile — genau das macht ein Browser
            // auch, und es ist der Grund fuer die Grenze.
            self.titel = weissraum(&text).chars().take(MAX_TITEL).collect();
            return;
        }
        // `rel="icon"` und `rel="shortcut icon"` (die Schreibweise von
        // 1999). Alles andere ist eine Stylesheet-Angabe — die laeuft
        // ueber `blaetter.rs` (P3) und gehoert nicht her.
        let rel = attributes
            .get("rel")
            .map(|r| r.to_ascii_lowercase())
            .unwrap_or_default();
        if rel.split_whitespace().any(|r| r == "icon")
            && let Some(href) = attributes
                .get("href")
                .map(|h| h.trim())
                .filter(|h| !h.is_empty())
        {
            // M4: die Adresse ist eine Eingabe (2048 wie `src`).
            let href: String = href.chars().take(2048).collect();
            self.favicon = Some(loese_link(&href, self.basis));
        }
    }

    /// 03.10. 12:55: EIN FORMULAR AUFGEMACHT (der `<form>`-Zweig).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Nummer aus `aktualer_ordinal` kommt
    /// und nicht neu gezaehlt wird: `aktualer_ordinal` ist dieselbe Zahl, die
    /// in `Seite::dom_zu_ord` steht. Eine eigene Zaehlung waere die zweite —
    /// und genau die hat in `narsil` dazu gefuehrt, dass der Klick auf einer
    /// Seite mit laufendem Skript das Formularfeld nicht fand (12:45).
    fn formular_oeffnen(&mut self, attributes: &HashMap<String, String>) {
        if self.formular_ords.len() >= MAX_FORMULARE {
            // GEMESSENE ENTSCHEIDUNG: kein Fehler, nur ein Verzicht. Die
            // Felder dieses Formulars bekommen dann `formular: None`, und der
            // Aufrufer schickt sie ohne Formular — ein Browser mit einem
            // Formular, das es nicht gibt, verhaelt sich genauso.
            return;
        }
        let index = self.formular_ords.len();
        self.formular_ords.push(FormularOrd {
            ord: self.aktualer_ordinal,
            aktion: attributes.get("action").cloned().unwrap_or_default(),
            methode: attributes
                .get("method")
                .map(|m| m.to_ascii_lowercase())
                .unwrap_or_else(|| "get".to_string()),
        });
        self.formular_stapel.push(index);
    }

    /// 03.10. 12:55: DAS FORMULAR WIEDER GESCHLOSSEN.
    fn formular_schliessen(&mut self) {
        self.formular_stapel.pop();
    }

    /// 03.10. 12:55: EIN FELD VERMERKEN, mit der Nummer des Bauers.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Tag mitkommt und nicht nur der
    /// `type`: `<select>` und `<textarea>` haben keinen `type`, und der
    /// Aufrufer (`narsil`) braucht den Unterschied fuer die Anzeige
    /// (`[Auswahl: …]` und `[Textfeld: …]` sind verschiedene Kasten).
    fn feld_merken(&mut self, tag: &str, attributes: &HashMap<String, String>) {
        if self.felder.len() >= crate::hteile::MAX_FORMULARFELDER {
            return;
        }
        self.felder.push(FeldOrd {
            ord: self.aktualer_ordinal,
            tag: match tag {
                "select" => "select",
                "textarea" => "textarea",
                "button" => "button",
                _ => "input",
            },
            typ: match tag {
                "input" => attributes
                    .get("type")
                    .map(|t| t.to_ascii_lowercase())
                    .unwrap_or_else(|| "text".to_string()),
                "button" => attributes
                    .get("type")
                    .map(|t| t.to_ascii_lowercase())
                    .unwrap_or_else(|| "submit".to_string()),
                "select" => "select-one".to_string(),
                _ => "textarea".to_string(),
            },
            name: attributes.get("name").cloned().unwrap_or_default(),
            formular: self.formular_stapel.last().copied(),
        });
    }

    /// EIN ELEMENT MIT `id` — WELCHER Knoten ist es?
    ///
    /// 02.10. 09:42. GEMESSENE GRUNDLAGE: ein Element erzeugt entweder einen
    /// **eigenen Kasten** (`<div>`, `<img>`, `<td>`) oder nur **Textknoten**
    /// in einem Zeilen-Container (`<h2>`, `<p>`, `<li>`, `<span>`, `<a>`).
    /// Beide sind fuer einen Sprung brauchbar, aber sie sind nicht gleich:
    /// der Kasten des Elements ist sein **Rechteck** (CSS 2.1, 12.4), und der
    /// Textknoten ist nur ein Stueck davon.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der **Kasten gewinnt** und nicht der
    /// erste Knoten: im Absatzweg entsteht der Textknoten **vor** dem Kasten
    /// (gemessen: `<div id="div">Div</div>` — erst Textknoten 6, dann der
    /// Kasten), und ohne diese Regel zeigte die Id auf das Textstueck. Ein
    /// Anker auf ein Element zeigt in jedem Browser auf dessen ganzes
    /// Rechteck.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „der alte bleibt" bei zwei Kästen: der
    /// erste ist der aeussere, und der ist der sichtbare.
    fn vermerke_id(&mut self, name: String, knoten: narsil_dom::NodeId) {
        let neu_ist_kasten = self
            .doc
            .node(knoten)
            .map(|n| matches!(n.kind, NodeKind::Container))
            .unwrap_or(false);
        if let Some(alt) = self.id_zu_knoten.get(&name).copied() {
            let alt_ist_kasten = self
                .doc
                .node(alt)
                .map(|n| matches!(n.kind, NodeKind::Container))
                .unwrap_or(false);
            if alt_ist_kasten || !neu_ist_kasten {
                return;
            }
        }
        self.id_zu_knoten.insert(name, knoten);
    }

    /// EIN BLOCK ODER EIN BILD IN EINEM `<a>` IST AUCH EIN LINK.
    ///
    /// 02.10. 05:00 (Auftrag "Links ohne Text"; Fund auf
    /// `https://news.ycombinator.com/`).
    ///
    /// GEMESSENER BEFUND, vier Faelle mit `seite_aus_text` (Wegwerf-Messung
    /// `_m_anker.rs`, entfernt):
    ///
    /// | Fall                                        | `links` |
    /// |---------------------------------------------|---------|
    /// | `<p>vor <a href=/x>Text</a> nach</p>`         | **1**   |
    /// | `<p>vor <a href=/y><div></div></a> nach</p>` | **0**   |
    /// | `<a href=/z><img src=b.png></a>`             | **0**   |
    /// | `<p>vor <a href=/leer></a> nach</p>`          | **0**   |
    ///
    /// GEMESSENE URSACHE: ein Inline-Element erzeugt **keinen** Knoten (siehe
    /// das Feld `marken`) - es merkt sich nur Stil und `href`. Die Adresse kam
    /// ausschliesslich dort an, wo **Text** entsteht: `links.insert` steht
    /// genau einmal in der ganzen Crate, im Textzweig. Ein `<a>` ueber einem
    /// **Block** hat aber keinen Text, der `href` fiel durch, und es gab nicht
    /// nur einen falschen Klick, sondern **gar keinen** Link.
    ///
    /// GEMESSENE EINORDNUNG, warum das mehr ist als ein Komfortfehler: auf
    /// HN sind die 30 Stimmpfeile (`<a id='up_...' href='vote?id=...'><div
    /// class='votearrow'></div></a>`) die einzige Moeglichkeit zu bewerten,
    /// und `news.css` gibt `.votearrow` `width: 10px; height: 10px` - der
    /// Pfeil ist sichtbar und nicht bedienbar.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `entry().or_insert()` und nicht
    /// `insert()`: bei verschachtelten `<a>` (HTML 5 erlaubt sie) steht die
    /// **innere** Marke oben, und die soll gewinnen - sie ist die naechste.
    /// Ein Knoten mit eigener Adresse behaelt seine.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **Funktion** und keine Zeile an
    /// zwei Stellen ist: die Regel "der Knoten erbt den `href` der innersten
    /// Marke" steht sonst zweimal im Bild, und bei einem dritten Weg (ein
    /// Knoten aus einem Skript) waere die dritte Abschrift die richtige
    /// geworden.
    ///
    /// M3: die Zahl der Eintraege ist durch die Arena begrenzt (ein Knoten
    /// je Element), also braucht es keinen eigenen Zaehler.
    fn vermerke_link_in_marke(&mut self, id: narsil_dom::NodeId) {
        if let Some(ziel) = self.marken.last().and_then(|m| m.href.clone()) {
            self.links.entry(id).or_insert(ziel);
        }
    }

    /// DER MANTEL UM `element_innen` - er haelt den Kasten-Stapel im
    /// Gleichschritt (03.10. 22:40).
    ///
    /// # DER BEFUND, DEN DIESER MANTEL BEHEBT
    ///
    /// GEMESSEN (22:00, Wegwerf-Messung entfernt): der Layout-Plan kommt bei
    /// 10 Knoten mit EINEM Eintrag an, und zwar immer am Knoten 1:
    ///     MESSUNG flex -> plan_len=1 plan_knoten=[1]   bei 10 Knoten
    ///     MESSUNG grid -> plan_len=1 plan_knoten=[1]   bei 13 Knoten
    ///     MESSUNG position:absolute -> plan_len=1 plan_knoten=[1] bei 6
    /// GEMESSENE URSACHE: `kasten_stapel` (Zeile 1793, initialisiert 2051,
    /// gepoppt 2931) wurde NIRGENDS GEFUELLT - `grep -rn "kasten_stapel"
    /// crates/` fand drei Stellen und keinen `push` - und
    /// `kasten_aus_dem_stapel` wurde in keiner Zeile darunter benutzt. Der
    /// Notnagel `knoten_vor` ist die Doc-Laenge beim Eintritt in `kind()`,
    /// also 1 fuer das erste Element: 13 Eintraege auf einem Schluessel.
    ///
    /// # WARUM EIN MANTEL UND KEIN PUSH IM `element_innen`
    ///
    /// GEMESSENE BEGRUENDUNG: `element_innen` hat elf `return`-Zweige (siehe
    /// den Kommentar an der Kaskadenstelle), und ein Push ohne das passende
    /// Pop waere an der ZELLE des naechsten Geschwisters sichtbar - nicht an
    /// der Tabelle, die es ausgeloest hat. Dazu kommt der zweite Aufrufer
    /// (Zeile 3063: ein Knoten aus einem Skript), der gar keinen `kind()`
    /// drumher hat - ein Push dort ohne Pop haette den Stapel dauerhaft
    /// verschoben. Der Mantel ist die EINE Stelle, an der beides zusammen
    /// gehalten wird, und er gibt den Kasten als Rueckgabewert heraus, damit
    /// `kind()` ihn ohne einen zweiten Stapel-Zugriff bekommt.
    fn element(
        &mut self,
        tag: &str,
        attributes: &HashMap<String, String>,
        kinder: &[HtmlNode],
        css_index: Option<u32>,
    ) -> Option<narsil_dom::NodeId> {
        self.kasten_stapel.push(None);
        //
        // 02.10. 18:40 (HTML): DER LABEL-RAHMEN — hier, nicht in
        // `element_innen`. GEMESSENE ENTSCHEIDUNG, warum: `element_innen` hat
        // **26** fruehe `return`s (GEMESSEN durch Zaehlen), und ein Rahmen,
        // den man am Ende des Funktions koepfte, waere bei jedem davon liegen
        // geblieben. Der Mantel `element()` ist die eine Stelle, die den Kasten-
        // stapel auch schon haelt — der Label-Rahmen folgt demselben Muster.
        let ist_label = tag.to_ascii_lowercase() == "label";
        if ist_label {
            let for_name = attributes
                .get("for")
                .map(|s| s.trim().chars().take(256).collect::<String>())
                .filter(|s| !s.is_empty());
            let bereich = self.mutter();
            self.label_stapel
                .push(LabelRahmen::label_neu(bereich, for_name));
        }
        // PSEUDOELEMENTE (06.10.2026): BEIDE Felder lesen, BEVOR die
        // Kinder gebaut werden — die Kinder setzen sie naemlich neu
        // (jedes Element rechnet seine eigenen). `push_child` haengt
        // HINTEN an: `::before` wartet darum auf den EIGENEN Kasten des
        // Elements (`pseudo_before`, gefuellt in `element_innen`) und
        // geht sonst an den Eltern; `::after` kommt danach an den
        // eigenen Kasten — oder an den Eltern, wenn es keinen gibt.
        let mut pseudo_vorher = self.pseudo_vorher.take();
        let mut pseudo_nachher = self.pseudo_nachher.take();
        // Release 11.10.2026: `::before` ist das erste Kind — seine Zaehler
        // und Anfuehrungszeichen gelten jetzt (eine Ebene tiefer).
        let eigene_tiefe = self.tiefe;
        if let Some(p) = pseudo_vorher.as_mut() {
            self.pseudo_aufloesen(p, attributes, eigene_tiefe + 1);
        }
        // Release 08.10.2026: ein INLINE-Element (`<a>`, `<span>` …) hat
        // keinen eigenen Kasten — seine inline Pseudokaesten gehoeren an
        // ihre Stelle IM TEXT, `::before` vor die Kinder, `::after` dahinter.
        // GEMESSEN vorher: MDN `.external::after` (Symbol „externer Link")
        // stand vor dem ganzen Absatz am Eltern, und `::before` kam sogar
        // NACH den Kindern. Der Plan oben auf dem Stapel ist der dieses
        // Elements (die Pseudos werden in `stil_von_kaskade` direkt nach
        // ihm gerechnet).
        let inline_element = (pseudo_vorher.is_some() || pseudo_nachher.is_some())
            && self.plan_stapel.last().and_then(|p| p.anzeige) == Some(narsil_css::Anzeige::Inline);
        // Release 08.10.2026: ein INLINE-ARTIGER Kasten (`inline-block`,
        // `inline-flex`, `inline-grid`) im Textfluss — kein Flex-/Grid-
        // Element (dort ist er Element des Containers).
        let inline_kasten = self.plan_stapel.last().is_some_and(|p| {
            matches!(p.anzeige, Some(narsil_css::Anzeige::InlineBlock) | Some(narsil_css::Anzeige::InlineFlex))
                || (p.anzeige == Some(narsil_css::Anzeige::Raster) && p.stil.as_ref().is_some_and(|st| st.inline_aussen))
                // Release 11.10.2026 (Agent CSS2): `inline-table` ebenso.
                || (p.anzeige == Some(narsil_css::Anzeige::Tabelle) && p.stil.as_ref().is_some_and(|st| st.inline_aussen))
        })
            // Release 11.10.2026 (Agent CSS2): eine Zelle oder Zeile MITTEN im
            // Textfluss eines Inline-Elements (`<span>`; im Block selbst — WPT `-212` — bleibt sie Block) bekommt eine
            // anonyme `inline-table` (CSS 2.1 §17.2.1, Schritt 3.2: „If C's
            // parent is an inline box, then T must be an inline-table box") —
            // sie steht also in der Zeile. `tabelle_anonym` baut sie.
            // GEMESSEN vorher (WPT `table-anonymous-objects-177` u. a.):
            // `<span>a <cell>b</cell><cell>c</cell> d</span>` stand in drei
            // Zeilen statt „a bc d".
            || (self.plan_stapel.last().is_some_and(|p| {
                matches!(p.anzeige, Some(narsil_css::Anzeige::TabelleZelle) | Some(narsil_css::Anzeige::TabelleZeile))
            }) && !self.marken.is_empty()
                // Nicht, wenn der Eltern selbst ein Tabellenteil ist (eine
                // Zelle in ihrer Zeile ist kein Textfluss).
                && !self.plan_stapel.iter().rev().nth(1).and_then(|p| p.anzeige).is_some_and(|a| {
                    matches!(a, narsil_css::Anzeige::Tabelle | narsil_css::Anzeige::TabelleZeile | narsil_css::Anzeige::TabelleRand)
                }));
        let eltern_verteilt = self.plan_stapel.iter().rev().nth(1).and_then(|p| p.anzeige).is_some_and(|a| {
            matches!(a, narsil_css::Anzeige::Flex | narsil_css::Anzeige::InlineFlex | narsil_css::Anzeige::Raster)
        });
        // Release 09.10.2026: Formularfelder sind inline-artige Kaesten
        // (HTML §15.5: `input`, `button`, `select`, `textarea` werden als
        // `inline-block` gesetzt), solange der Autor sie nicht zum Block
        // macht. GEMESSEN vorher: `<p>Name <input> <button>OK</button>
        // Ende</p>` stand in VIER Zeilen (Chrome/Firefox: eine).
        let inline_feld = self.feld_ist_inline(tag, attributes);
        if (inline_kasten || inline_feld) && !eltern_verteilt && !ist_ersetzt(tag) {
            self.absatz_zu_inline_fluss();
        }
        // Release 08.10.2026: TEXT-Pseudos (`content: " / "`, `"•"`,
        // `attr(data-x)`) werden Text im Inhalt des Elements: `::before` vor
        // seinem ersten Text, `::after` (nur Inline-Elemente) dahinter.
        // GEMESSEN vorher: kein einziger `content`-Text wurde gezeichnet
        // (Trenner in Brotkrumen, Aufzaehlungspunkte, Hinweise).
        if let Some(t) = pseudo_vorher.as_ref().and_then(|p| p.als_text()).map(str::to_string)
            && let Some(p) = pseudo_vorher.take()
            && self.pseudo_text_wartend.len() < 64
        {
            // Release 11.10.2026: ein Block-`::before` (`display: block` …)
            // steht auf einer eigenen Zeile; der Text traegt den Stil des
            // Pseudoelements. GEMESSEN vorher (WPT `CSS2/generated-content/
            // content-*`, ~60 Dateien): `::before { color: green }` war
            // schwarz.
            self.pseudo_text_wartend.push(WartenderText {
                text: t,
                eigene_zeile: p.eigene_zeile(),
                tiefe: eigene_tiefe,
                stil: erbbarer_textstil(&p.stil),
                block_box: pseudo_block_box(&p),
                balance: p.balance,
            });
        }
        // Release 11.10.2026: der `::after`-Text steht erst nach den Kindern
        // fest (Zaehler, Anfuehrungszeichen). Ein Inline-Element setzt ihn
        // hinter seine Kinder, ein Block ans Ende seines Inhalts — GEMESSEN
        // vorher: `div::after { content: "Z" }` zeichnete nichts (ein leerer
        // Kasten ohne Text).
        let nach_text = inline_element && pseudo_nachher.as_ref().is_some_and(|p| p.wird_text());
        let nach_text = if nach_text { pseudo_nachher.take() } else { None };
        if !inline_element
            && let Some(p) = pseudo_nachher.take_if(|p| p.wird_text())
            && self.nach_wartend.len() < 256
        {
            self.nach_wartend.push((eigene_tiefe, p));
        }
        if inline_element
            && let Some(p) = pseudo_vorher.take_if(|p| p.im_fluss())
        {
            self.pseudo_im_textfluss(p, false);
        }
        self.pseudo_before_stapel.push(pseudo_vorher);
        let ist_liste = matches!(tag.to_ascii_lowercase().as_str(), "ul" | "ol" | "menu" | "dir");
        if ist_liste {
            self.listen_zaehler.push(listen_anfang(tag, attributes, kinder));
        }
        // Release 09.10.2026: BLOCK IN INLINE. Ein Block (`<h3>`, `<div>` …)
        // in einem Inline-Element (`<a>`, `<span>`) traegt SEINEN Textstil
        // (er erbt ihn in der Kaskade schon vom Inline-Element) — der Link
        // gibt nur Ziel und Nummer weiter. GEMESSEN vorher: t-online.de, die
        // Aufmacher-Ueberschrift `<a><div><h3 class="md:text-40 font-bold">`
        // kam mit 14 px normal (dem Stil des Links) statt 40 px fett; jede
        // Teaser-Karte in einem `<a>` verlor so ihre Typografie.
        let block_in_inline = !self.marken.is_empty()
            && self.marken.len() < MAX_INLINE_TIEFE
            && !ist_ersetzt(tag)
            && self.plan_stapel.last().and_then(|p| p.anzeige).is_some_and(|a| {
                !matches!(a, narsil_css::Anzeige::Inline | narsil_css::Anzeige::Keins)
            });
        // Release 09.10.2026: ein BLOCKartiges `<a href>` (z. B. als Flex-
        // Element oder mit `display:block`) gibt sein Ziel an alles darin
        // weiter. GEMESSEN vorher: `<div style="display:flex"><a href><div>
        // Kachel</div></a></div>` hatte keinen Link — der Text im inneren
        // `<div>` stand in einem eigenen Absatz ohne Ziel (gemeldet vom
        // Feature-Agenten an den Kacheln der Startseite).
        let block_link = tag.eq_ignore_ascii_case("a")
            && !ist_ersetzt(tag)
            && self.plan_stapel.last().and_then(|p| p.anzeige).is_some_and(|a| {
                !matches!(a, narsil_css::Anzeige::Inline | narsil_css::Anzeige::Keins)
            });
        let eigenes_ziel = if block_link {
            attributes
                .get("href")
                .map(|h| h.trim())
                .filter(|h| !h.is_empty())
                .map(|h| loese_link(h, self.basis))
                .filter(|h| !h.is_empty())
        } else {
            None
        };
        let block_marke = if eigenes_ziel.is_some() && self.marken.len() < MAX_INLINE_TIEFE {
            self.kommender_stil.as_ref().map(|st| Marke {
                stil: erbbarer_textstil(st),
                href: eigenes_ziel.clone(),
                ord: Some(self.aktualer_ordinal),
            })
        } else if block_in_inline {
            self.kommender_stil.as_ref().map(|st| Marke {
                stil: erbbarer_textstil(st),
                href: self.marken.last().and_then(|m| m.href.clone()),
                ord: self.marken.last().and_then(|m| m.ord),
            })
        } else {
            None
        };
        let block_marke_gesetzt = match block_marke {
            Some(m) => {
                self.marken.push(m);
                true
            }
            None => false,
        };
        self.element_innen(tag, attributes, kinder, css_index);
        if block_marke_gesetzt {
            self.marken.pop();
        }
        if ist_liste {
            self.listen_zaehler.pop();
        }
        if ist_label {
            self.label_abschluss();
        }
        let raus = self.kasten_stapel.pop().flatten();
        // Ein `::before`-Text, den kein Text abgeholt hat (leeres Element),
        // steht jetzt an seiner Stelle.
        if !self.pseudo_text_wartend.is_empty() {
            self.pseudo_text_setzen();
        }
        if let Some(mut p) = nach_text {
            self.pseudo_aufloesen(&mut p, attributes, eigene_tiefe + 1);
            if let Some(t) = p.als_text().map(str::to_string) {
                self.pseudo_text_schreiben(&t, erbbarer_textstil(&p.stil), p.eigene_zeile(), pseudo_block_box(&p), p.balance);
            }
        }
        // Ein Block-`::after`, das kein Kasten abgeholt hat (ein Zweig ohne
        // eigenen Kasten), steht hinter dem Inhalt.
        if self.nach_wartend.last().is_some_and(|(t, _)| *t == eigene_tiefe) {
            self.nach_pseudo_setzen(attributes);
        }
        if let Some(Some(p)) = self.pseudo_before_stapel.pop() {
            self.pseudo_anhaengen(p, None);
        }
        if let Some(mut p) = pseudo_nachher {
            self.pseudo_aufloesen(&mut p, attributes, eigene_tiefe + 1);
            if inline_element && raus.is_none() && p.im_fluss() {
                self.pseudo_im_textfluss(p, true);
            } else {
                self.pseudo_anhaengen(p, raus);
            }
        }
        raus
    }

    /// Release 08.10.2026: der laufende Absatz wird ein INLINE-FLUSS, weil ein
    /// inline-artiger Kasten in ihm steht — derselbe Weg wie ein Absatz mit
    /// Bild (`bild_absatz`), nur nachtraeglich, denn die Anzeigeart des
    /// Kindes kennt der Bau erst hier. GEMESSEN vorher: `<p>Text <span
    /// style="display:inline-block">X</span> weiter</p>` stand in DREI
    /// Zeilen; t-online.de, GitHub- und Apple-Knoepfe im Text ebenso.
    fn absatz_zu_inline_fluss(&mut self) {
        let mutter = self.mutter();
        if self.inline_bild_eltern.contains(&mutter) {
            return;
        }
        // Release 09.10.2026: kein offener Absatz DIESES Kastens (`<div>`,
        // `<td>`, Wurzel …) — dann wird der Kasten selbst der Inline-Fluss,
        // wie ein Kasten mit Text und Bild. Was er schon als Zeilen
        // geschrieben hat, bleibt davor stehen (der Fluss setzt Block-Kinder
        // als eigene Zeilen), der laufende Absatz wird beim Schliessen in
        // Inline-Teile zerlegt. GEMESSEN vorher: `<div>Vor <span
        // style="display:inline-block">X</span> nach</div>` in DREI Zeilen.
        let rahmen_passt = self
            .absatz_rahmen
            .last()
            .is_some_and(|r| r.kasten.is_none() && r.eltern == mutter);
        if !rahmen_passt {
            if self.absatz_rahmen.last().is_none_or(|r| r.eltern != mutter) {
                self.inline_bild_eltern.insert(mutter);
            }
            return;
        }
        let Some(r) = self.absatz_rahmen.last() else { return };
        let (stil, ord, platz, anfang) = (r.stil.clone(), r.ord, r.platz, r.kinder_anfang);
        let Ok(kasten) = self.doc.push_child(mutter, NodeKind::Container, None, Span::EMPTY) else { return };
        let _ = self.doc.set_style(kasten, stil);
        self.dom_zu_ord.insert(kasten.index() as u32, ord);
        // Was der Absatz schon geschrieben hat, wandert in den Kasten.
        let bisher: Vec<NodeId> = self
            .doc
            .get(mutter)
            .map(|n| n.children.iter().skip(anfang).copied().filter(|k| *k != kasten).collect())
            .unwrap_or_default();
        if !bisher.is_empty() {
            let _ = self.doc.kinder_setzen(kasten, bisher);
        }
        if let Some(slot) = self.kasten_stapel.get_mut(platz) {
            *slot = Some(kasten);
        }
        if let Some(r) = self.absatz_rahmen.last_mut() {
            r.kasten = Some(kasten);
        }
        self.inline_bild_eltern.insert(kasten);
        self.offen.push(kasten);
        // Release 09.10.2026: Label-Regeln sind nach dem Elternknoten
        // verschluesselt — die Woerter des Absatzes stehen jetzt im Kasten.
        // GEMESSEN vorher: `<p><label for=c>Haken</label> <input
        // type=checkbox id=c></p>` verlor die Zuordnung (`[]`).
        let umzug: Vec<(LabelSchluessel, LabelRegel)> = self
            .label_regeln
            .iter()
            .filter(|((b, _), _)| *b == mutter)
            .map(|((_, t), r)| ((kasten, t.clone()), LabelRegel { feld: r.feld, for_name: r.for_name.clone() }))
            .collect();
        self.label_regeln.extend(umzug);
        for r in self.label_stapel.iter_mut().filter(|r| r.bereich == mutter) {
            r.bereich = kasten;
        }
        if let Some(a) = self.laufender.as_mut() {
            a.stil.margin = Spacing::ZERO;
            a.stil.padding = Spacing::ZERO;
            a.stil.background = Color::TRANSPARENT;
            a.stil.rahmen = Default::default();
        }
        // Die Woerter bis hierher gehen als Inline-Teile in den Kasten.
        self.absatz_schliessen();
    }

    /// Release 09.10.2026: steht dieses Formularfeld als inline-artiger
    /// Kasten im Textfluss? Ja, ausser der Autor gibt ihm eine
    /// Block-/Flex-/Raster-Anzeige oder es ist unsichtbar
    /// (`type=hidden`). Der Plan oben auf dem Stapel ist der des Feldes.
    fn feld_ist_inline(&self, tag: &str, attributes: &HashMap<String, String>) -> bool {
        let feld = ["input", "button", "select", "textarea"]
            .iter()
            .any(|t| tag.eq_ignore_ascii_case(t));
        if !feld
            || (tag.eq_ignore_ascii_case("input")
                && attributes.get("type").is_some_and(|t| t.eq_ignore_ascii_case("hidden")))
        {
            return false;
        }
        matches!(
            self.plan_stapel.last().and_then(|p| p.anzeige),
            None | Some(narsil_css::Anzeige::Inline) | Some(narsil_css::Anzeige::InlineBlock)
        )
    }

    /// Release 09.10.2026: die Anzeigeart des aeusseren Feld-Kastens: im
    /// Inline-Fluss `inline-block` (der Kasten steht IN der Zeile), sonst
    /// Block wie bisher.
    fn feld_anzeige(&self) -> narsil_layout::plan::Anzeige {
        if self.inline_bild_eltern.contains(&self.mutter())
            && matches!(
                self.plan_stapel.last().and_then(|p| p.anzeige),
                None | Some(narsil_css::Anzeige::Inline) | Some(narsil_css::Anzeige::InlineBlock)
            )
        {
            narsil_layout::plan::Anzeige::InlineBlock
        } else {
            narsil_layout::plan::Anzeige::Block
        }
    }

    /// Release 08.10.2026: die wartenden `::before`-Texte setzen (in
    /// Dokumentreihenfolge, vor dem Text, der sie abholt).
    fn pseudo_text_setzen(&mut self) {
        self.pseudo_text_setzen_unter(usize::MAX);
    }

    /// Release 11.10.2026: die wartenden `::before`-Texte der Elemente
    /// oberhalb von Bautiefe `grenze` setzen.
    fn pseudo_text_setzen_unter(&mut self, grenze: usize) {
        let alle = std::mem::take(&mut self.pseudo_text_wartend);
        let (jetzt, bleiben): (Vec<_>, Vec<_>) = alle.into_iter().partition(|w| w.tiefe < grenze);
        self.pseudo_text_wartend = bleiben;
        for w in jetzt {
            self.pseudo_text_schreiben(&w.text, w.stil, w.eigene_zeile, w.block_box, w.balance);
        }
    }

    /// Release 11.10.2026: ein Pseudo-Text im eigenen Textstil (als Marke,
    /// die Verweis und Nummer des umgebenden Textes uebernimmt; die
    /// Textdekoration der Vorfahren gilt weiter, CSS Text Decoration 3 §2);
    /// ein Block-Pseudo auf einer eigenen Zeile.
    fn pseudo_text_schreiben(
        &mut self,
        t: &str,
        stil: Style,
        eigene_zeile: bool,
        block_box: Option<(Style, narsil_layout::LayoutStil)>,
        balance: bool,
    ) {
        if eigene_zeile {
            self.absatz_schliessen();
        }
        let vorher_balance = self.pseudo_balance;
        if eigene_zeile {
            self.pseudo_balance = Some(balance);
        }
        // Release 14.10.2026 (Agent CSS2): see `pseudo_block_box`.
        let box_open = match block_box.filter(|_| eigene_zeile) {
            Some((box_style, box_plan)) => {
                let eltern = self.mutter();
                match self.doc.push_child(eltern, NodeKind::Container, None, Span::EMPTY) {
                    Ok(id) => {
                        let _ = self.doc.set_style(id, box_style);
                        self.layout_plan.set(id, box_plan);
                        self.offen.push(id);
                        true
                    }
                    Err(_) => false,
                }
            }
            None => false,
        };
        let (aktuell, href, ord) = self.geltender_textstil();
        let mut stil = stil;
        stil.dekoration |= aktuell.dekoration;
        let gesetzt = self.marken.len() < MAX_INLINE_TIEFE;
        if gesetzt {
            self.marken.push(Marke { stil, href, ord });
        }
        // `text()` liest `white-space` aus der Marke (`content: "a\A b"`
        // mit `pre-line`); die noch wartenden Texte tieferer Elemente
        // duerfen dabei nicht vor diesen rutschen.
        let wartend = std::mem::take(&mut self.pseudo_text_wartend);
        self.text(t);
        self.pseudo_text_wartend = wartend;
        if gesetzt {
            self.marken.pop();
        }
        if eigene_zeile {
            self.absatz_schliessen();
        }
        self.pseudo_balance = vorher_balance;
        if box_open {
            self.offen.pop();
        }
    }

    /// Release 11.10.2026: den wartenden `::after`-Text des Elements auf
    /// dieser Tiefe setzen — am Ende seines Inhalts, ein Block-`::after`
    /// auf einer eigenen Zeile.
    fn nach_pseudo_setzen(&mut self, attributes: &HashMap<String, String>) {
        let Some((tiefe, mut p)) = self.nach_wartend.pop() else {
            return;
        };
        self.pseudo_aufloesen(&mut p, attributes, tiefe + 1);
        let Some(t) = p.als_text().map(str::to_string) else {
            return;
        };
        if !self.pseudo_text_wartend.is_empty() {
            self.pseudo_text_setzen();
        }
        self.pseudo_text_schreiben(&t, erbbarer_textstil(&p.stil), p.eigene_zeile(), pseudo_block_box(&p), p.balance);
    }

    /// Release 11.10.2026: die Zaehler eines Pseudokastens anwenden und
    /// seinen `content` mit Zaehlern und Anfuehrungszeichen zu Text machen.
    fn pseudo_aufloesen(&mut self, p: &mut kaskade::PseudoKasten, attributes: &HashMap<String, String>, tiefe: usize) {
        if !p.zaehler.is_empty() {
            let angaben = std::mem::take(&mut p.zaehler);
            zaehler_anwenden(&mut self.zaehler, &angaben, tiefe);
        }
        let Some(roh) = p.inhalt.take() else {
            return;
        };
        let paare = p
            .zitate
            .clone()
            .or_else(|| self.zitate.last().map(|z| z.1.clone()))
            .unwrap_or_else(kaskade::zitate_vorgabe);
        let attr = |name: &str| attributes.get(&name.to_ascii_lowercase()).cloned();
        let text = kaskade::inhalt_text(&roh, &attr, &mut |e| {
            Some(match e {
                kaskade::Erzeugt::Zaehler { name, trenner, stil } => {
                    if !self.zaehler.iter().any(|z| z.0 == name) {
                        zaehler_anlegen(&mut self.zaehler, &name, 0, tiefe);
                    }
                    let werte = self.zaehler.iter().filter(|z| z.0 == name).map(|z| z.1);
                    match trenner {
                        Some(trenner) => {
                            werte.map(|n| kaskade::zaehler_format(n, &stil)).collect::<Vec<_>>().join(&trenner)
                        }
                        None => kaskade::zaehler_format(werte.last().unwrap_or(0), &stil),
                    }
                }
                kaskade::Erzeugt::Zitat(art) => {
                    let paar = |i: usize| paare.get(i.min(paare.len().saturating_sub(1)));
                    match art {
                        kaskade::Zitat::Auf => {
                            let t = paar(self.zitat_tiefe).map(|p| p.0.clone()).unwrap_or_default();
                            self.zitat_tiefe = self.zitat_tiefe.saturating_add(1).min(1 << 20);
                            t
                        }
                        kaskade::Zitat::Zu if self.zitat_tiefe > 0 => {
                            self.zitat_tiefe -= 1;
                            paar(self.zitat_tiefe).map(|p| p.1.clone()).unwrap_or_default()
                        }
                        kaskade::Zitat::Zu => String::new(),
                        kaskade::Zitat::KeinAuf => {
                            self.zitat_tiefe = self.zitat_tiefe.saturating_add(1).min(1 << 20);
                            String::new()
                        }
                        kaskade::Zitat::KeinZu => {
                            self.zitat_tiefe = self.zitat_tiefe.saturating_sub(1);
                            String::new()
                        }
                    }
                }
            })
        });
        p.text = text;
    }

    /// Release 08.10.2026: der Kasten eines `<video>` — ein ersetztes
    /// Element mit den Massen aus `width`/`height` (sonst 300 x 150, HTML
    /// „Media elements") und der Groesse aus der Kaskade; das Poster wird
    /// eingepasst gezeichnet (`object-fit: contain` ist die Vorgabe fuer
    /// Videos). Der Kasten wird zum Eltern der Steuerknoten (`offen`), der
    /// Aufrufer nimmt ihn danach wieder vom Stapel.
    /// Release 11.10.2026 (Agent CSS2): der Kasten eines `<audio>`: die
    /// Steuerknoten stehen IN ihm, und er traegt Rand und Groesse aus der
    /// Kaskade (Vorgabe 300 px breit, wie die Steuerleiste der Browser).
    /// GEMESSEN vorher (WPT `the-audio-element/audio-loading-*-deferred`,
    /// `.below-viewport { margin-top: 1000vh }`): die Platzhalter standen bei
    /// y 8 — der Rand des Elements fehlte, x war der des Platzhalters (12).
    fn audio_kasten(&mut self) -> Option<NodeId> {
        let id = self.doc.push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY).ok()?;
        let mut stil = Style { deckkraft: 1.0, radius: [0.0; 4], schatten_anzahl: 0, dekoration: 0, ..Style::default() };
        stil.size.width = Length::px(300.0);
        if let Some(k) = self.kommender_stil.as_ref() {
            if k.size.width != Length::Auto {
                stil.size.width = k.size.width;
            }
            if k.size.height != Length::Auto {
                stil.size.height = k.size.height;
            }
            stil.margin = k.margin;
            stil.background = k.background;
        }
        let _ = self.doc.set_style(id, stil);
        self.eigener_kasten = Some(id);
        if let Some(platz) = self.kasten_stapel.last_mut() {
            *platz = Some(id);
        }
        if let Some(name) = self.aktuelle_id.clone() {
            self.vermerke_id(name, id);
        }
        self.offen.push(id);
        Some(id)
    }

    fn video_kasten(&mut self, attributes: &HashMap<String, String>) -> Option<NodeId> {
        let id = self.doc.push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY).ok()?;
        let zahl = |n: &str| {
            attributes
                .get(n)
                .and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok())
                .filter(|v| v.is_finite() && *v > 0.0 && *v < 100_000.0)
        };
        let (breite, hoehe) = (zahl("width"), zahl("height"));
        let _ = self.doc.set_ersetzt(
            id,
            narsil_dom::ErsatzMasse { breite: Some(breite.unwrap_or(300.0)), hoehe: Some(hoehe.unwrap_or(150.0)) },
        );
        let mut stil = Style { deckkraft: 1.0, radius: [0.0; 4], schatten_anzahl: 0, dekoration: 0, ..Style::default() };
        stil.size.width = breite.map_or(Length::Auto, Length::px);
        stil.size.height = hoehe.map_or(Length::Auto, Length::px);
        if let Some(k) = self.kommender_stil.as_ref() {
            if k.size.width != Length::Auto {
                stil.size.width = k.size.width;
            }
            if k.size.height != Length::Auto {
                stil.size.height = k.size.height;
            }
            stil.margin = k.margin;
            stil.radius = k.radius;
            stil.radius_xy = k.radius_xy;
            stil.radius_percent = k.radius_percent;
            stil.background = k.background;
        }
        let _ = self.doc.set_style(id, stil);
        self.eigener_kasten = Some(id);
        if let Some(platz) = self.kasten_stapel.last_mut() {
            *platz = Some(id);
        }
        // Die Steuerknoten duerfen nicht aus der Flaeche laufen.
        if let Some(st) = self.plan_stapel.last_mut().and_then(|p| p.stil.as_mut()) {
            st.ueberlauf = narsil_layout::plan::UeberlaufArt::Versteckt;
        }
        if let Some(name) = self.aktuelle_id.clone() {
            self.vermerke_id(name, id);
        }
        if let Some(poster) = attributes.get("poster").map(|p| p.trim()).filter(|p| !p.is_empty()) {
            self.hintergrundbilder.insert(
                id,
                (
                    crate::loese_link(poster, self.basis),
                    narsil_css::BildGeo {
                        contain: true,
                        position_x: 0.5,
                        position_y: 0.5,
                        wiederholen_x: false,
                        wiederholen_y: false,
                        ..Default::default()
                    },
                ),
            );
        }
        self.offen.push(id);
        Some(id)
    }

    /// Release 08.10.2026: einen Pseudokasten eines Inline-Elements IN den
    /// laufenden Textfluss setzen. Er wird als ANKER im Absatz gemerkt
    /// (`Absatz::anker`) und beim Zeilenschreiben zwischen die Textlaeufe
    /// seiner Zeile gehaengt — auch nach einem Nachumbruch. Inline wird zu
    /// Inline-Block; ein absolut positionierter Kasten bleibt absolut
    /// (seine statische Position ist dann die Stelle im Text).
    fn pseudo_im_textfluss(&mut self, mut p: kaskade::PseudoKasten, nach: bool) {
        if p.plan.anzeige == narsil_layout::plan::Anzeige::Block
            && !matches!(p.plan.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)
            && p.plan.schweben == narsil_layout::plan::FloatArt::Keins
        {
            p.plan.anzeige = narsil_layout::plan::Anzeige::InlineBlock;
        }
        // Der Knoten entsteht jetzt (Plan, Hintergrundbild) und wird beim
        // Zeilenschreiben in seine Zeile umgehaengt. Ohne laufenden Absatz
        // bleibt er am Eltern — dort steht er dann schon in der richtigen
        // Reihenfolge.
        // `::after` steht im Element VOR dessen rechtem Innenabstand; unser
        // Textlauf traegt den Innenabstand schon in seiner Breite — also
        // wird der Kasten um ihn nach links gezogen (MDN reserviert mit
        // `.external { padding-right: 1.125em }` genau den Platz fuers
        // Symbol). Im Fluss bleibt der Abstand rechts dahinter erhalten.
        if nach
            && let Some(pr) = self
                .laufender
                .as_ref()
                .and_then(|a| a.abschnitte.last())
                .map(|s| s.stil.padding.right.resolve_or(0.0, 0.0))
                .filter(|pr| pr.is_finite() && *pr > 0.0)
        {
            let absolut = matches!(p.plan.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest);
            p.stil.margin.left = Length::px(p.stil.margin.left.resolve_or(0.0, 0.0) - pr);
            if !absolut {
                p.stil.margin.right = Length::px(p.stil.margin.right.resolve_or(0.0, 0.0) + pr);
            }
        }
        let Some(id) = self.pseudo_anhaengen(p, None) else {
            return;
        };
        if let Some(a) = self.laufender.as_mut()
            && a.anker.len() < 256
        {
            a.anker.push((a.abschnitte.len(), id, nach));
            a.trennen = true;
        }
    }

    /// PSEUDOELEMENTE (06.10.2026): einen gerechneten `::before`/
    /// `::after`-Kasten anhaengen.
    ///
    /// WARUM EIN EIGENER KNOTEN: Pseudoelemente stehen nicht im DOM, aber
    /// sie BRAUCHEN einen Kasten (Groesse, Hintergrund, Bild). Unser Baum
    /// kennt dafuer `NodeKind::Container` — derselbe Weg wie ein Bild.
    /// Der Knoten hat keine Adresse und keine Ordinalnummer: er ist
    /// nicht per JS erreichbar, genau wie im Browser.
    ///
    /// `ziel` ist der Kasten, IN den gehaengt wird. `None` heisst: der
    /// aktuelle Elternknoten (`mutter()`), und das ist der richtige
    /// Fall, wenn das Element keinen eigenen Kasten baut.
    fn pseudo_anhaengen(
        &mut self,
        p: kaskade::PseudoKasten,
        ziel: Option<narsil_dom::NodeId>,
    ) -> Option<narsil_dom::NodeId> {
        if kaskade::dbg_pseudo_an() {
            eprintln!(
                "PSEUDO size={:?} bg={:?} bild={:?} anzeige={:?}",
                p.stil.size,
                p.stil.background,
                p.bild.as_ref().map(|b| (b.0.as_str(), &b.1)),
                p.plan.anzeige
            );
        }
        let eltern = match ziel {
            Some(z) => z,
            None => self.mutter(),
        };
        let id = self
            .doc
            .push_child(eltern, narsil_dom::NodeKind::Container, None, Span::EMPTY)
            .ok()?;
        let _ = self.doc.set_style(id, p.stil);
        self.layout_plan.set(id, p.plan);
        if let Some(bild) = p.bild {
            self.hintergrundbilder.insert(id, bild);
        }
        if let Some(maske) = p.maske {
            let maske = self.masken_aufloesen(maske);
            self.maskenbilder.insert(id, maske);
        }
        Some(id)
    }

    /// 02.10. 18:40 (HTML): EIN `<label>` ENDET — seine Knoten eintragen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der umschliessende Fall **hier** und der
    /// `for`-Fall erst am Bauende aufgeloest wird: beim umschliessenden Fall
    /// ist das Feld ein **Nachkomme** des Labels, also beim Schliessen des
    /// Labels bekannt (GEMESSEN: `platzhalter` gibt dafuer den Knoten zurueck).
    /// Bei `for="X"` ist es umgekehrt — der Feldknoten kann spaeter im Dokument
    /// stehen (GEMESSEN an der Testseite), also gibt es beim Schliessen nichts
    /// aufzuloesen, nur den Namen zu merken.
    ///
    /// WAS ICH NICHT BEHAUPTE: dass ein `<label>` mit `for` **und** Inhalt
    /// doppelt zaehlt. Beide Wege schreiben in dieselbe Tabelle, also gewinnt
    /// der letzte — und beide zeigen (in einem gueltigen HTML-Dokument) auf
    /// dasselbe Feld.
    fn label_abschluss(&mut self) {
        let Some(rahmen) = self.label_stapel.pop() else {
            return;
        };
        // GEMESSENE EINORDNUNG, warum ohne Text **nichts** eingetragen wird:
        // ein `<label>` ganz ohne Text (etwa `<label><input></label>`) hat
        // nichts, worauf geklickt werden koennte, und ein leerer Text als
        // Schluessel wuerde jeden leeren Abschnitt im selben Absatz treffen.
        let text = rahmen.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let regel = LabelRegel {
            feld: rahmen.feld,
            for_name: rahmen.for_name,
        };
        if regel.feld.is_none() && regel.for_name.is_none() {
            return;
        }
        let bereich = rahmen.bereich;
        self.label_regeln.insert(
            (bereich, text.clone()),
            LabelRegel {
                feld: regel.feld,
                for_name: regel.for_name.clone(),
            },
        );
        self.label_offen_auflosen(&bereich, &text, &regel);
    }

    /// 03.10. 00:58 (HTML): DIE VORGEMERKTEN KNOTEN DIESER REGEL AUFLOESEN.
    ///
    /// GEMESSENE EINORDNUNG, warum das erst **hier** und nicht beim Abspulen
    /// geschieht: beim Abspulen kennt der `<input>`-Zweig das Feld noch nicht
    /// (er schliesst den Absatz, **bevor** er den Platzhalter anlegt — siehe
    /// `LabelRahmen::text`).
    ///
    /// (Diese Funktion ist um 00:58 beim Zuruecknehmen eines Experiments
    /// versehentlich mitgeflogen und wieder eingesetzt worden — GEMESSEN zeigt
    /// der Compiler, wenn sie fehlt: „no method named `label_offen_auflosen`".)
    fn label_offen_auflosen(&mut self, bereich: &narsil_dom::NodeId, text: &str, regel: &LabelRegel) {
        let schluessel = (*bereich, text.to_string());
        let wartende: Vec<narsil_dom::NodeId> = self
            .label_offen_knoten
            .iter()
            .filter(|(_, k)| *k == schluessel)
            .map(|(n, _)| *n)
            .collect();
        for n in wartende {
            self.label_offen_knoten.retain(|(m, _)| *m != n);
            if let Some(feld) = regel.feld {
                self.label_zu_knoten.insert(n, feld);
            } else if let Some(name) = regel.for_name.clone() {
                self.label_for_unfertig.push((n, name));
            }
        }
    }

    /// 02.10. 19:05 (HTML): TEXT INNERHALB EINES LABELS — der **Inhalt**.
    ///
    /// GEMESSENE EINORDNUNG, warum hier der Text und nicht der Knoten: der
    /// Absatz erzeugt seine Knoten erst beim Schliessen, und beim `<input>`
    /// passiert das **innerhalb** des Labels (siehe `LabelRahmen::text`).
    fn label_text_sammeln(&mut self, t: &str) {
        let Some(rahmen) = self.label_stapel.last_mut() else {
            return;
        };
        rahmen.text.push_str(t);
        //
        // 02.10. 20:05 (HTML): DIE REGEL JETZT EINTragen, nicht erst beim
        // Schliessen.
        //
        // GEMESSENER BEFUND, warum die Reihenfolge hier entscheidet (Messstelle
        // `NARSIL_L1_LABEL`, Wegwerf): `<label>Name <input></label>` spult den
        // Absatz **innerhalb** des Labels ab — der `input`-Zweig ruft
        // `absatz_schliessen()` — und GEMESSEN kam die Reihenfolge
        // „AUFLOESEN 'Name' **vor** REGEL 'Name'". Der Knoten wurde also
        // erzeugt, bevor es die Regel gab, und `label_zu_knoten` blieb **leer**
        // (GEMESSEN `label=[]`).
        //
        // GEMESSENE ENTSCHEIDUNG, warum das Feld trotzdem stimmt: hier steht es
        // noch nicht fest (es kommt mit dem `<input>` danach) — der Knoten wird
        // deshalb **vorgemerkt** (`label_offen_knoten`) und beim Schliessen des
        // Labels aufgeloest, wo `rahmen.feld` bekannt ist.
        let bereich = rahmen.bereich;
        let for_name = rahmen.for_name.clone();
        let regel = LabelRegel {
            feld: rahmen.feld,
            for_name,
        };
        // 02.10. / 03.10. 00:55 (HTML): ZURUECKGENOMMEN — EIN ZWEITER
        // SCHLUESSEL HAT NICHTS GEBRACHT.
        //
        // GEMESSENER BEFUND (echte Seite, `www.w3schools.com/html/
        // html_forms.asp`, 240 872 Byte, Wegwerf entfernt): von **12**
        // `<label>` bekamen **9** eine Regel. Ich habe vermutet, die beiden mit
        // einem Icon-Glyph verfehlten Labels kaemen an, wenn ich **zusaetzlich**
        // das einzelne Textstueck als Schluessel eintrage (der Absatz teilt den
        // Text in Abschnitte mit gleichem Stil, ein Glyph hat einen anderen).
        // GEMESSEN nach der Aenderung: **weiterhin 9**. Also loest das die
        // Luecke nicht — und eine Aenderung ohne messbare Wirkung im heissen
        // Pfad (jeder Textaufruf) ist schlimmer als keine. Zurueckgenommen
        // (00:55), die Zahlen stehen in `team/BERICHT_HTML_BILDWEG.txt`.
        let text = rahmen.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let bereich = rahmen.bereich;
        let for_name = rahmen.for_name.clone();
        let regel = LabelRegel {
            feld: rahmen.feld,
            for_name,
        };
        self.label_regeln.insert((bereich, text), regel);
    }

    /// 03.10. 00:58 (HTML): IST DIESER TEXTKNOTEN DER EINES `<label>`?
    ///
    /// (Zurueckgesetzt beim Zuruecknehmen des Experiments von 00:50 und wieder
    /// eingesetzt — GEMESSEN zeigt der Compiler: „no method named
    /// `label_regel_anwenden`".)
    fn label_regel_anwenden(&mut self, absatz: narsil_dom::NodeId, text: &str, id: narsil_dom::NodeId) {
        //
        // 02.10. 20:30 (HTML): DER BEREICH IST DER ABSATZKNOTEN SELBST.
        //
        // GEMESSENE KORREKTUR AN MEINER EIGENEN ENTSCHEIDUNG (Messstelle
        // `NARSIL_L1_LABEL`, Wegwerf): ich hatte zuerst den **Elternknoten** des
        // Absatzes genommen, weil in einem frueheren Stand der Absatzknoten
        // noch nicht existierte, als das Label sich schloss (19:00). GEMESSEN
        // sind jetzt beide Seiten Knoten **2** — beim Eintritt des Labels
        // (`self.mutter()`) und beim Abspulen (`self.mutter()`, als
        // `absatz_knoten` festgehalten) — und der Elternknoten war eine Ebene
        // zu hoch (GEMESSEN „bereich=1" gegen „REGEL absatz=2").
        //
        // GEMESSENE EINORDNUNG, warum nicht einfach nur der Text: zwei Labels
        // mit demselben Text (zwei „Name"-Felder) wuerden beide denselben
        // Knoten finden — der Bereich macht es eindeutig.
        let bereich = absatz;
        let schluessel = (bereich, text.trim().to_string());
        let Some(regel) = self.label_regeln.get(&schluessel) else {
            return;
        };
        if !self.label_stapel.is_empty() {
            // GEMESSENE EINORDNUNG, warum hier **vorgemerkt** wird: ein Label
            // ist noch offen, also kann sein Feld gleich feststehen — aber
            // noch nicht jetzt (siehe `label_offen_auflosen`).
            self.label_offen_knoten.push((id, schluessel));
            return;
        }
        if let Some(feld) = regel.feld {
            self.label_zu_knoten.insert(id, feld);
        } else if let Some(name) = regel.for_name.clone() {
            self.label_for_unfertig.push((id, name));
        }
    }

    /// 02.10. 18:40 (HTML): DIESER KNOTEN IST DAS FELD DES UMSCHLIESSENDEN
    /// LABELS.
    fn label_feld_setzen(&mut self, id: narsil_dom::NodeId) {
        if let Some(rahmen) = self.label_stapel.last_mut() {
            // GEMESSENE ENTSCHEIDUNG, warum nur das **erste** Feld: ein Label
            // mit zwei Feldern ist ungueltiges HTML, und der erste Treffer ist
            // die einzige Antwort, die man nicht noch erklaeren muss.
            if rahmen.feld.is_none() {
                rahmen.feld = Some(id);
            }
        }
    }

    fn element_innen(
        &mut self,
        tag: &str,
        attributes: &HashMap<String, String>,
        kinder: &[HtmlNode],
        css_index: Option<u32>,
    ) {
        let klein = tag.to_ascii_lowercase();
        //
        // 03.10. 12:55: FORMULAR UND FELDER VERMERKEN — ganz oben, damit die
        // Zahl dieselbe ist wie ueberall sonst.
        //
        // GEMESSENE BEGRUENDUNG fuer die Stelle: `aktualer_ordinal` ist beim
        // EINTRITT die Nummer DIESES Elements (W7C R6, siehe unten) und wird
        // von den Kindern ueberschrieben. Wer spaeter nachsehen will, bekommt
        // die Zahl des Kindes.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das nicht in den vier Zweigen
        // (`input`, `button`, `select`, `textarea`) steht: dort stuende je eine
        // Zeile, und eine Zeile, die man beim Umbau eines Zweiges mitnimmt,
        // ist keine Zusage. Hier steht sie einmal.
        if klein == "form" {
            self.formular_oeffnen(attributes);
        } else if matches!(klein.as_str(), "input" | "button" | "select" | "textarea") {
            self.feld_merken(&klein, attributes);
        }

        //
        // W7C R6 (01.10. 23:30): DIE EIGENE NUMMER, BEVOR DIE KINDER BAUEN.
        //
        // `kind()` setzt `aktualer_ordinal` am EINTRITT (Zeile 2064) und
        // jedes Kind ueberschreibt es in seinem eigenen `kind()`. Wer die
        // Nummer DIESES Elements braucht, muss sie also VOR den Kindern
        // nehmen.
        //
        // GEMESSENER FEHLER, seit 30.09. 15:20 als `#[ignore]`-Test
        // abgelegt (`handler_funktion.rs::der_handler_eines_inline_
        // elements_laeuft_nicht`): der Inline-Zweig bildet die Knoten der
        // Kinder auf `self.aktualer_ordinal` ab — nach dem Bau der Kinder
        // also auf die Nummer des LETZTEN Kindes. `<a id="l">weg</a>`:
        // `Seite::ids["l"]` war 4, in `dom_zu_ord` stand 4 nicht. Der
        // Klick traf den Textknoten des Links, die Kette nannte die
        // Nummer des letzten Kindes, und der Handler lief NIE — ein Link
        // mit `preventDefault` navigierte trotzdem.
        let eigene_ord = self.aktualer_ordinal;

        //
        // W7C R3 (02.10., Rolle LAYOUT): DERSELBE Griff fuer den Kasten
        // des Layout-Plans, aus demselben Grund.
        //
        // `eigener_kasten` wird von zwei Schreibstellen gesetzt (Bild und
        // Kasten der Hand-Typografie), und beide liegen VOR dem Bau der
        // Kinder. `element()` geht danach in die Kinder — und jedes Kind
        // setzt das Feld neu. Ohne dieses Sichern hat der Elternknoten am
        // Ende den Kasten seines LETZTEN Kindes, und `display:flex` landet
        // an der falschen Stelle. Die Rolle HTML hat das dreimal anders
        // versucht (`team/UEBERGABE_HTML_R3R4.txt`) und es dreimal
        // falsch getroffen.
        let eigener_kasten_vorher = self.eigener_kasten.take();

        //
        // W6, J9: DIE ID-TABELLE UND DIE BRUECKE — ganz oben, und der
        // Grund ist ein gemessener Fehler (30.09.2026, 12:15):
        //
        // Beides stand vorher weiter unten, bei der Box. GEMESSEN: bei
        // `<p id="z">vorher</p>` kam `p` dort NIE an — der Absatz-Zweig
        // (`for k in kinder { self.kind(k) }; return;`) endet vorher.
        // Ergebnis: `getElementById("z")` gab `null`, `querySelector`
        // fand nichts, und kein Skript konnte etwas aendern. Ein `<div>`
        // haette funktioniert — ein `<p>` nie. Das ist die Fehlerklasse
        // „drei Stellen entscheiden" aus W5G, nur jetzt an zwei
        // early returns.
        //
        // Darum: an EINER Stelle, vor allen `return`-Zweigen.
        if let Some(c) = css_index {
            self.css_zu_dom.insert(c, self.aktualer_ordinal);
        }
        // `narsil-dom` verwirft Attribute nach dem Bau, also wird hier
        // gelesen, solange das Attribut da ist. M3/M4: `MAX_IDS`
        // Eintraege, und der Name wird auf 256 Zeichen gekuerzt (ein `id`
        // ist ein Bezeichner, keine Seite).
        //
        // GEMESSENE KORREKTUR (09:35): `aktuelle_id` wurde nur **gesetzt**,
        // wenn dieses Element eine `id` traegt, und nie **geleert**. Damit
        // trug der naechste Block die Id des vorherigen Elements — und weil
        // `insert` ueberschreibt, gewann der letzte Block nach der Id.
        // GEMESSENE FOLGE auf der echten Seite
        // (`de.wikipedia.org/wiki/Rust_(Programmiersprache)`): **544**
        // Anker-Links, und **1** davon hatte ein Ziel in der Tabelle.
        //
        // DIE REGEL, die daraus folgt: `aktuelle_id` wird am Anfang JEDES
        // Elements gesetzt — `Some(name)` bei eigener Id, sonst `None`. Ein
        // `None` ist damit die Antwort auf „dieses Element hat keine Id", und
        // eine fremde Id kann nicht mitlaufen.
        self.aktuelle_id = None;
        if let Some(id) = attributes.get("id")
            && self.ids.len() < crate::hteile::MAX_IDS
        {
            let name: String = id.chars().take(256).collect();
            if !name.is_empty() {
                self.ids.insert(name.clone(), self.aktualer_ordinal);
                // Der Knoten dieses Elements entsteht gleich danach (siehe
                // `aktuelle_id`) — der Name wandert mit.
                self.aktuelle_id = Some(name);
            }
        }

        // Unsichtbar: der ganze Inhalt faellt weg.
        // `<script>`: NICHT unsichtbar, sondern eigener Weg (siehe
        // `skript_aufnahme`). `<noscript>` ist ANZEIGBAR — ohne
        // JavaScript ist sein Inhalt der Inhalt der Seite, und bis die
        // Engine verdrahtet ist, ist sie es fuer alle.
        // `<style>`: QUELLE sichern, nicht anzeigen.
        //
        // H2, 30.09.2026. `narsil-fmt-html` behaelt den Inhalt (es kann ihn
        // nicht wegwerfen, siehe dort), und diese Crate legt ihn in
        // `Seite::stile` ab — damit die Kaskade ihn finden kann, sobald
        // `narsil-css` baut. Angezeigt wird er nicht: `matches!` unten
        // laeuft danach, und `style` steht dort.
        if klein == "style" {
            self.stile_aufnahme(attributes, kinder);
            zaehle(kinder, self.tiefe, &mut self.naechste_ordinal, false);
            return;
        }

        // `<head>` ist unsichtbar — aber SEINE KINDER sind es nicht alle.
        //
        // GEMESSENER FEHLER 30.09.2026: `head` stand in der
        // Unsichtbar-Liste und kehrte sofort zurueck. Damit wurde auch
        // das `<style>` darin nie besucht, und `Seite::stile` war fuer
        // jede echte Seite leer — denn `<style>` steht nach HTML-Regel
        // IM KOPF. Die Kaskade haette auf jeder Seite nichts zu
        // rechnen, und der Grund waere eine Unsichtbarkeitsliste
        // gewesen.
        //
        // Der Kopf wird also DURCHSUCHT, nicht uebersprungen: was
        // sichtbar sein will (`<style>`, `<script>`), wird gesammelt;
        // was nicht (title, meta, link), faellt unten durch.
        if klein == "head" {
            for k in kinder {
                self.kind(k);
            }
            return;
        }

        //
        // 04.10. 15:50 (HTML): `<legend>` STEHT AUF DEM RAND DES
        // `<fieldset>`, nicht in einem eigenen Kasten.
        //
        // GEMESSENER FEHLER (Formularseite, 1280 px, gegen
        // `/home/jaff/fx_formular.png`): „Art" stand in einem **Kasten** —
        // als unbekanntes Element erbte `legend` den Kasten des Formulars
        // und sass darin. Firefox zeichnet die Beschriftung **ueber** der
        // Randlinie, und der Rand ist **unterbrochen**.
        //
        // WAS HIER OHNE ENGINE-AENDERUNG GEHT, und warum es reicht: der
        // Kasten der `legend` bekommt die **Hintergrundfarbe des
        // `fieldset`** und einen **negativen oberen Rand** in Hoehe von
        // Rahmen + Polsterung. Damit liegt er auf der Randlinie und
        // **verdeckt** sie — das ist genau das, was Firefox zeigt
        // (GEMESSEN: der Rand setzt sich links und rechts vom Wort „Art"
        // fort). Eine echte Luecke im Rahmen waere Arbeit in der Engine;
        // die Deckung ist in dieser Crate und sieht gleich aus.
        if klein == "legend" {
            let eltern = self
                .marken
                .last()
                .map(|m| m.stil.clone())
                .or_else(|| self.laufender.as_ref().map(|a| a.stil.clone()))
                .unwrap_or_default();
            // 04.10. 16:25 (HTML): **kein negativer oberer Rand** — er
            // liess die Legende im Bild **verschwinden** (gemessen gegen
            // `html20_formular_nachher.png`: vorher stand „Art" in der Ecke,
            // nachher war an ihrer Stelle nichts). Der Kasten lag richtig
            // (y=8 wie das Feld), der Text wurde nicht mehr gezeichnet.
            let mut stil = eltern.clone();
            // Die `legend` erbt die Farbe des Feldes: sonst waere ihr
            // eigener Kasten weiss auf weiss und wuerde den Rand nicht
            // verdecken, sondern nur uebermalen.
            stil.background = eltern.background;
            //
            // **WAS NOCH FEHLT, und es ist ehrlich zu sagen:** der Rand ist
            // damit **nicht unterbrochen** — das ist Arbeit in der Engine
            // (ein Kasten, der an einer Stelle keinen Rahmen zeichnet).
            // Firefox laesst den Rahmen links und rechts vom Wort weiterlaufen.
            self.absatz_schliessen();
            self.absatz_starten(stil, None);
            for k in kinder {
                self.kind(k);
            }
            self.absatz_schliessen();
            return;
        }

        let unsichtbar = matches!(
            klein.as_str(),
            "style" | "meta" | "link" | "title" | "template"
        );
        //
        // 04.10. 06:10 (HTML) stand hier ein flacher Weg fuer
        // `<template shadowrootmode>` (MDN: 26 deklarative Wurzeln, 67 statt
        // 167 Links). Er ist durch den echten Shadow-DOM-Weg ersetzt.
        // Release 12.10.2026: ein `<template>` ist IMMER unsichtbar — eine
        // deklarative Schattenwurzel steht seit dem Parser-Umbau als
        // `shadow_root` am Wirt (siehe `schatten`), nicht mehr als Template.
        if unsichtbar {
            zaehle(kinder, self.tiefe, &mut self.naechste_ordinal, false);
            return;
        }

        //
        // 03.10. 18:55 (H-AR1): DAS `hidden`-ATTRIBUT VERSTECKT DAS ELEMENT.
        //
        // GEMESSENER BEFUND (18:40, sechs Bauarten, Wegwerf-Test entfernt):
        //     MESSUNG sichtbar:                        ["[ ] c (checkbox)"]
        //     MESSUNG type=hidden:                      []
        //     MESSUNG hidden-Attribut:                  ["[ ] c (checkbox)"]  <- BUG
        //     MESSUNG style display:none:               []
        //     MESSUNG mutter display:none:              []
        //     MESSUNG checkbox aus inputhidden-menue:   ["[ ] c (checkbox)"]  <- BUG
        //
        // GEMESSENE URSACHE: `style="display:none"` wirkt (die Kaskade), das
        // `hidden`-Attribut nicht — es wird nirgends gelesen. Auf
        // ar.wikipedia.org ist das Mobile-Menue genau so gebaut (ein
        // `<input type=checkbox" hidden>` plus CSS), und der Nutzer las
        // "[ ] (checkbox)" mitten im Text.
        //
        // GEMESSENE ENTSCHEIDUNG, warum es hier steht und nicht im
        // Eingabe-Zweig: `hidden` versteckt JEDES Element (HTML 5, 7.2: das
        // UA-Stylesheet traegt `[hidden] { display: none }`). Eine Liste, die
        // nur `<input>` nennt, waere eine Liste, die jemand erweitern muss,
        // sobald die erste Seite `<div hidden>` schreibt — und diese Seite
        // kommt sicher. GEMESSEN: `display:none` macht genau dasselbe
        // (kein Knoten), und beides an EINER Stelle ist der Beweis, dass es
        // dasselbe ist.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Kinder NICHT gebaut werden: ein
        // verstecktes Element hat keine Kinder im Dokumentbaum (so arbeitet
        // `display:none` bei uns bereits, gemessen).
        // Release 15.10.2026: `hidden` is now the UA rule `[hidden] { display:
        // none }` (narsil-css `UA_CSS`) and reaches this point only when author
        // CSS shows the element again; a hidden subtree takes the
        // `display: none` path, which keeps ordinals and runs its scripts.

        // Die `on*`-Attribute einsammeln, BEVOR irgendein Zweig
        // zurueckkehrt. GEMESSENE REGEL: es steht hier und nicht in den
        // einzelnen Zweigen, weil sonst `<b onclick=…>` (Inline, kein
        // Knoten) und `<div onclick=…>` (Block, Knoten) an verschiedenen
        // Stellen behandelt werden mussten — und die Inline-Stelle
        // waere die, die man vergisst.
        if self.ereignisse.len() < MAX_EREIGNISSE {
            // H2 (03.10., gemessen): `eigene_ord` wandert mit — die
            // Vorlauf-Nummer DIESES Elements, gesichert in Zeile 2931,
            // bevor die Kinder gebaut werden. Es ist dieselbe Zahl, die
            // die `id`-Tabelle benutzt, also ein Schluessel fuer
            // `Angezeigt::horcher`.
            let neu = ereignisse_aus(&klein, attributes, Some(eigene_ord as u32));
            // M3: die Gesamtzahl bleibt unter der Grenze.
            let platz = MAX_EREIGNISSE - self.ereignisse.len();
            self.ereignisse.extend(neu.into_iter().take(platz));
        }

        // `<script>`: aufnehmen, NICHT zeichnen.
        //
        // ENTSCHEIDUNG 30.09.2026 (Nutzer, ueber die Koordination): es
        // gibt eine eigene JavaScript-Engine (`narsil-js`). Bis eben wurde
        // der Inhalt hier verworfen, und damit war jede spaetere
        // Unterstuetzung von JavaScript unmoeglich — nicht schwer, nicht
        // kompliziert: unmoeglich.
        //
        // Der Knoten im Dokument ist 0x0 und ohne Kinder, damit die Seite
        // den Platz des Skripts nicht verliert und der Quelltext nicht
        // gezeichnet wird. Der Quelltext steht in `Skript::inhalt`.
        if klein == "script" {
            self.skript_aufnahme(attributes, kinder);
            zaehle(kinder, self.tiefe, &mut self.naechste_ordinal, false);
            return;
        }
        // Release 08.10.2026: `<noscript>` zeigt nichts, wenn JavaScript
        // laeuft (HTML §4.12.2) — und es laeuft. GEMESSEN vorher: amazon.de
        // zeigte „JavaScript is disabled … verify that you're not a robot",
        // spiegel.de jedes Teaserbild ein zweites Mal (Ersatz-`<img>`).
        // Die Nummern zaehlen weiter wie beim Skript.
        if klein == "noscript" && skripte_laufen() {
            zaehle(kinder, self.tiefe, &mut self.naechste_ordinal, false);
            return;
        }

        // INLINE zuerst, und das ist die wichtigste Zeile in der ganzen
        // Funktion.
        //
        // GEMESSENER FEHLER 30.09.2026 (Koordinationsbefund mit
        // Bildschirmfoto): `<p>Das ist ein <b>ganz normaler</b> Absatz
        // mit einem <a>Link</a>. …` wurde als FUENF untereinander
        // stehende Zeilen gezeichnet. Zwei Ursachen, beide hier:
        //
        // 1. `<b>` bekam einen CONTAINER (unten, "Sonst: echter
        //    Container") und am Ende `absatz_schliessen()`. Damit war
        //    der laufende Absatz nach JEDEM Inline-Element zu Ende.
        // 2. `text()` rief `absatz_starten()`, das den laufenden Absatz
        //    ebenfalls beendete. Fuenf Teilstuecke, fuenf Absaetze,
        //    fuenf Zeilen.
        //
        // Inline-Elemente bekommen deshalb gar keinen Knoten: sie legen
        // eine MARKE auf den Stapel, und die Kinder erben sie.
        //
        // P-DISPLAY (05.10.2026): EIN INLINE-TAG MIT `display:block` IST
        // KEIN INLINE MEHR.
        //
        // GEMESSEN (eigene Seite, 05.10.): `.x strong{display:block}` und
        // `style="display:block"` auf `<span>`/`<strong>` wurden ignoriert —
        // Name und Zahl standen in EINER Zeile. Ursache: `ist_inline(tag)`
        // entscheidet nach dem TAG und fragt den berechneten `display` nie.
        // Genau das ist auf www.wikipedia.org der Unterschied
        // (`.central-featured-lang strong{display:block}`) und auf vielen
        // Seiten.
        //
        // DIE REGEL (CSS 2.1, 9.2.1): `inline` erzeugt eine Inline-Box.
        // `inline-block` und BLOCK-stufige Werte (`block`, `flex`, `grid`,
        // `list-item`, Tabelle) erzeugen einen **Kasten**.
        //
        // `inline-block` ist dabei (P9, 05.10.2026) — die Engine kennt die
        // Schrumpfbreite (`box.rs::schrumpft_breite`), deshalb ist
        // `<span style="display:inline-block">` jetzt ein Kasten in der
        // Zeile und nicht mehr abgeflachter Text.
        let erzwungen_kasten = self
            .plan_stapel
            .last()
            .and_then(|p| p.anzeige)
            .is_some_and(|a| {
                matches!(
                    a,
                    narsil_css::Anzeige::InlineBlock
                        | narsil_css::Anzeige::Block
                        | narsil_css::Anzeige::Flex
                        | narsil_css::Anzeige::InlineFlex
                        | narsil_css::Anzeige::Raster
                        | narsil_css::Anzeige::ListenEintrag
                        | narsil_css::Anzeige::Tabelle
                        | narsil_css::Anzeige::TabelleZeile
                        | narsil_css::Anzeige::TabelleZelle
                        | narsil_css::Anzeige::TabelleRand
                        // Release 11.10.2026 (Agent CSS2): auch eine Spalte
                        // (`<span style="display: table-column">`) ist ein
                        // Kasten (WPT `table-anonymous-objects-093`).
                        | narsil_css::Anzeige::TabelleSpalte
                )
            }) || self.plan_stapel.last().and_then(|p| p.stil.as_ref()).is_some_and(|p|
                matches!(p.position, narsil_layout::plan::PositionArt::Absolut | narsil_layout::plan::PositionArt::Fest));
        let css_inline = self.plan_stapel.last().and_then(|p| p.anzeige) == Some(narsil_css::Anzeige::Inline);
        // Replaced elements and controls need their dedicated builders even
        // when display is inline; flattening them loses resources and ordinals.
        let eigener_elementweg = matches!(klein.as_str(),
            "input" | "button" | "select" | "textarea" | "option" | "form" |
            "img" | "svg" | "video" | "audio" | "picture" | "source" | "track" |
            "embed" | "object" | "iframe" | "canvas" | "br" | "hr");
        if (ist_inline(&klein) || (css_inline && !eigener_elementweg)) && !erzwungen_kasten {
            // `<br>` ist inline, aber kein Stilwechsel: es bricht um. Es
            // steht deshalb NICHT in `ist_inline`, sondern hier unten bei
            // den Sonderfaellen — sonst haette es eine Marke ohne Wirkung.
            //
            // DER VON DER KASKADE BERECHNETE STIL (der kommt aus
            // `kind()`): er enthaelt schon den Inline-Delta UND die
            // Angaben des Autors. Ohne ihn bekommen `<a>` und `<span>`
            // nie ihre CSS-Angaben — gemessen am 30.09.2026 an
            // `nav a { margin-right: 12px }`.
            if let Some(mut berechnet) = self.kommender_stil.take() {
                // Release 11.10.2026 (Agent CSS2): `vertical-align` hebt die
                // Grundlinie GEGEN die des Eltern-Inline-Kastens — die
                // Hebungen summieren sich (CSS 2.1 §10.8.1).
                berechnet.vertikal_hoch += self.marken.last().map_or(0.0, |m| m.stil.vertikal_hoch);
                //
                // 02.10. 00:35 (Meldung KOMPRESS D1): DAS `href` GEHT AN
                // ALLE NACHFAHREN. HTML 5, 6.6.1 sagt woertlich: „The a
                // element is transparent, but an a element whose content
                // model is anything other than transparent MUST NOT have an
                // href attribute." Ein `<a>` ist also kein Kasten um seinen
                // Text, sondern der Link gilt fuer seinen GANZEN Teilbaum.
                //
                // GEMESSENER BEFUND (00:34, Wegwerf-Messung entfernt), jede
                // Zeile ist eine Seite fuer sich:
                //
                //     <a href=/a>direkt</a>                       1 Link
                //     <a href=/a><span>im span</span></a>          **0**
                //     <a href=/a><span>Text</span><img …></a>     **0**
                //     <a href=/a><span>eins</span><span>zwei</span></a>  **0**
                //     <a href=/a>vor <b>fett</b> nach</a>          2 (der Text
                //         „fett" aus dem `<b>` fehlt in der Mitte!)
                //     <a href=/a><a>kein eigenes</a></a>           **0**
                //     zwei Links mit je einem <span>              **0**
                //
                // GEMESSENE URSACHE, und sie ist eine Zeile: die Inline-
                // Kette legt fuer JEDES Inline-Element eine Marke auf den
                // Stapel, und fuer alles ausser `<a>` mit `href: None`.
                // Der Text nimmt die **innerste** Marke — also die des
                // `<span>`, und die sagt „kein Link". GEMESSEN auf
                // github.com (Meldung): 140 `<a>`, 4 Links.
                //
                // GEMESSENE ENTSCHEIDUNG, warum „erben, wenn kein eigenes
                // href da ist" und nicht „bei `<a>` immer": HTML 5, 6.6.1
                // nennt ein `<a>` OHNE `href` einen Platzhalter — es ist
                // selbst kein Link. Verschachtelt in einem Link bleibt es
                // aber Teil von dessen Teilbaum, also erbt es. Und ein
                // eigenes `href` gewinnt immer, auch wenn aussen ein Link
                // steht (verschachtelte `<a>` sind verboten; wer es tut,
                // meint es so).
                let eigenes = if klein == "a" {
                    attributes
                        .get("href")
                        .map(|h| h.trim())
                        .filter(|h| !h.is_empty())
                        .map(|h| loese_link(h, self.basis))
                        .filter(|h| !h.is_empty())
                } else {
                    None
                };
                let href = eigenes.or_else(|| self.marken.last().and_then(|m| m.href.clone()));
                let vorher = self.marken.len();
                if vorher < MAX_INLINE_TIEFE {
                    self.marken.push(Marke {
                        stil: berechnet,
                        href,
                        // W7C R6: der Text, der ab hier kommt, gehoert
                        // diesem Inline-Element.
                        ord: Some(eigene_ord),
                    });
                }
                //
                // W6C: JEDES KIND GEHOERT ZU DIESEM INLINE-ELEMENT.
                //
                // GEMESSEN, warum (13:05, Befund der Koordination):
                // `<button id=k>` lieferte bei drei Klicks `false`, und
                // der Zaehler blieb 0. Grund: ein Inline-Element bekam
                // keinen DOM-Knoten, und seine Kinder (Text, Zeilenboxen)
                // gehoerten zu niemandem — fuer den getroffenen
                // Textknoten gab es keine Nummer, also fand der Klick
                // keinen Handler.
                //
                // DIE LOE SUNG ist die VEREINIGUNG: alle Knoten, die
                // `self.kind(k)` fuer die Kinder erzeugt, werden auf DIE
                // Nummer dieses Inline-Elements abgebildet. Damit hat ein
                // Inline-Element eine Adresse, ohne einen eigenen Kasten
                // zu bekommen (der wuerde den Text in einen Block
                // zwingen — der Inline-Befund aus W5).
                //
                // M3: `MAX_KNOTEN_VON_INLINE` (64) Knoten je
                // Inline-Element.
                let vor_kinder = self.doc.len();
                for k in kinder {
                    self.kind(k);
                }
                let grenze = (vor_kinder + MAX_KNOTEN_VON_INLINE).min(self.doc.len());
                //
                // W7C R6: `eigene_ord`, NICHT `self.aktualer_ordinal`.
                // Nach der Schleife darueber traegt `aktualer_ordinal` die
                // Nummer des LETZTEN Kindes (siehe oben). Ein Inline-
                // Element hat keinen eigenen Kasten — es IST seine Kinder,
                // was die Bruecke wissen muss.
                for i in vor_kinder..grenze {
                    let k = i as u32;
                    if !self.dom_zu_ord.contains_key(&k) {
                        self.dom_zu_ord.insert(k, eigene_ord);
                    }
                }
                if self.marken.len() > vorher {
                    self.marken.pop();
                }
                return;
            }
            let basis = self
                .marken
                .last()
                .map(|m| m.stil.clone())
                .or_else(|| self.laufender.as_ref().map(|a| a.stil.clone()))
                .unwrap_or(Style {
                    deckkraft: 1.0,
                    radius: [0.0; 4],
                    schatten_anzahl: 0,
                    dekoration: 0,
                    color: FARBE_TEXT,
                    font_size: GRAD_TEXT,
                    ..Style::default()
                });
            let stil = self.inline_stil(basis, &klein, attributes);
            //
            // 02.10. 00:35: DIESELBE REGEL AN DER ZWEITEN STAPELSTELLE (siehe
            // die Begruendung dort). GEMESSENE EINORDNUNG, warum es zwei
            // Stellen sind und nicht eine: der erste Zweig gilt fuer ein
            // Inline-Element MIT einem Stil aus der Kaskade, der zweite fuer
            // eines ohne (`self.kommender_stil` war `None`). GEMESSEN, dass
            // beide Zweige vorkommen: `<span>` ohne CSS-Regel geht durch den
            // zweiten — und genau der war der gemeldete Fall.
            let eigenes = if klein == "a" {
                attributes
                    .get("href")
                    .map(|h| h.trim())
                    .filter(|h| !h.is_empty())
                    .map(|h| loese_link(h, self.basis))
                    .filter(|h| !h.is_empty())
            } else {
                None
            };
            let href = eigenes.or_else(|| self.marken.last().and_then(|m| m.href.clone()));
            // M3/M5: der Markenstapel ist begrenzt. Ueber der Grenze
            // wird der Text ohne die aeussere Auszeichnung geschrieben —
            // das ist ein Fehler im Bild, kein Absturz.
            let vorher = self.marken.len();
            if vorher < MAX_INLINE_TIEFE {
                self.marken.push(Marke {
                    stil,
                    href,
                    // W7C R6: siehe oben — der Text gehoert diesem
                    // Element, nicht dem Absatz.
                    ord: Some(eigene_ord),
                });
            }
            for k in kinder {
                self.kind(k);
            }
            if self.marken.len() > vorher {
                self.marken.pop();
            }
            return;
        }

        // NATIVE FALLBACKS ohne JavaScript (Vision: Alltagstauglich ohne JS).
        //
        // Regel: Was ohne Skript sichtbar sein muss, wird als Text gezeigt —
        // nie verworfen, nie unsichtbar. Ein Forum, ein Blog, ein Formular
        // sind ohne diese Zeilen unbenutzbar, mit ihnen lesbar.
        if klein == "svg" {
            let mut svg_attr = attributes.clone();
            if let Some(stil) = self.kommender_stil.as_ref() {
                svg_attr.entry("color".into()).or_insert_with(|| {
                    format!(
                        "#{:02x}{:02x}{:02x}",
                        stil.color.r, stil.color.g, stil.color.b
                    )
                });
            }
            // Release 08.10.2026: SVG-SPRITES. `<svg><use href="#logo"/></svg>`
            // verweist auf ein `<symbol id="logo">` in einem ANDEREN (meist
            // versteckten) SVG des Dokuments. Serialisiert wurde nur das
            // lokale SVG — der Verweis lief ins Leere, und es erschien ein
            // dunkler Platzhalter (GEMESSEN: Logo und alle Symbole auf
            // spiegel.de). Die referenzierten Definitionen kommen als
            // `<defs>` mit.
            // Release 08.10.2026: CSS an SVG-Kindern (`fill`, `stroke`, …)
            // kommt als Attribut mit — der SVG-Renderer kennt kein CSS.
            // GEMESSEN: Tailwind-Symbole (`class="fill-current"` unter
            // `fill="none"`) blieben unsichtbar.
            let svg_stil = self.kaskade.as_ref().and_then(|k| k.werte.last()).cloned();
            let mut svg_kinder = match &svg_stil {
                Some(s) => self.svg_mit_css(kinder, s, 0),
                None => kinder.to_vec(),
            };
            let defs_quelle = svg_definitionen_fuer(kinder, &self.svg_definitionen);
            let defs: Vec<HtmlNode> = defs_quelle
                .iter()
                .map(|d| {
                    let mut neu = match &svg_stil {
                        Some(s) => self.svg_mit_css(std::slice::from_ref(*d), s, 0),
                        None => vec![(*d).clone()],
                    };
                    // Ein `<svg>` als Ziel von `<use>` wird zum `<symbol>`
                    // (SVG 2 §5.6: es erzeugt einen eigenen Sichtrahmen).
                    for n in neu.iter_mut() {
                        if let HtmlNode::Element { tag, .. } = n
                            && tag.eq_ignore_ascii_case("svg")
                        {
                            *tag = "symbol".to_string();
                        }
                    }
                    neu
                })
                .flatten()
                .collect();
            if let Some(s) = &svg_stil {
                svg_resolve_attribute_vars(&mut svg_attr, &s.variablen);
                // The text inside inherits the page's font.
                svg_font_attribute(&mut svg_attr, s, true);
                svg_css_attribute(&mut svg_attr, s);
            }
            if !defs.is_empty() {
                svg_kinder.insert(0, HtmlNode::Element {
                    tag: "defs".to_string(),
                    attributes: HashMap::new(),
                    children: defs,
                    shadow_root: None,
                });
            }
            let quelltext = narsil_fmt_html::serialisiere(&[HtmlNode::Element {
                tag: tag.to_string(),
                attributes: svg_attr,
                children: svg_kinder,
                shadow_root: None,
            }]);
            let mut quelle = String::from("data:image/svg+xml,");
            for b in quelltext.bytes() {
                use std::fmt::Write;
                let _ = write!(quelle, "%{b:02X}");
            }
            let mut attr = attributes.clone();
            attr.insert("src".into(), quelle);
            // Release 15.10.2026 (Agent CSS2): marks the image of an inline
            // `<svg>` (its `width`/`height` are presentation attributes).
            attr.insert(SVG_INLINE.into(), "1".into());
            // Release 08.10.2026: ein Inline-SVG mit `viewBox`, aber ohne
            // `width`/`height`, hat KEINE Eigengroesse, nur ein
            // Seitenverhaeltnis — es fuellt die Breite seines Containers
            // (SVG 2 §8.2: `width: auto` = 100 %), die Hoehe folgt dem
            // Verhaeltnis. Die viewBox-Masse sind keine Pixel. GEMESSEN: die
            // Wortmarke von tagesschau.de (`viewBox="0 0 692 140"` in einem
            // 150-px-Kasten) wurde 692 px breit und abgeschnitten („tag").
            // CSS-Breiten an der Seite gehen weiter vor (Attribut = Hinweis).
            let hat_viewbox = attributes.keys().any(|k| k.eq_ignore_ascii_case("viewbox"));
            if hat_viewbox && !attributes.contains_key("width") && !attributes.contains_key("height") {
                attr.insert(SVG_FUELLT.into(), "1".into());
                let vb = attributes
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("viewbox"))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                let zahlen: Vec<f32> = vb
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|t| !t.is_empty())
                    .filter_map(|t| t.parse::<f32>().ok())
                    .collect();
                if zahlen.len() == 4 && zahlen[2].is_finite() && zahlen[3].is_finite() && zahlen[2] > 0.0 && zahlen[3] > 0.0 {
                    attr.insert(SVG_VERHAELTNIS.into(), format!("{} {}", zahlen[2], zahlen[3]));
                }
            }
            // Release 14.10.2026 (Agent CSS2): without `viewBox`, a missing
            // `width`/`height` falls back to the default replaced size
            // 300 x 150 (CSS 2.1 §10.3.2, §10.6.2; SVG 2 §8.2). MEASURED before
            // (WPT `CSS2/positioning/absolute-replaced-width-*`,
            // `<svg:svg height="50">`): the SVG was 0 px wide.
            if !hat_viewbox {
                if !attributes.contains_key("width") {
                    attr.insert("width".into(), "300".into());
                }
                if !attributes.contains_key("height") {
                    attr.insert("height".into(), "150".into());
                }
            }
            // Release 14.10.2026: the scripts inside the SVG run like any
            // other parser script (see `skripte_im_svg`).
            self.skripte_im_svg(kinder, 0);
            // Das SVG ist ein einziges ersetztes Element. Seine Kinder
            // behalten ihre Ordinalplaetze fuer nachfolgende DOM-Ereignisse.
            zaehle(kinder, self.tiefe, &mut self.naechste_ordinal, false);
            self.element_innen("img", &attr, &[], css_index);
            return;
        }
        if klein == "video" {
            //
            // VIDEO, 04.10.2026. `<video>` IST EIN STEUERELEMENT.
            //
            // GEMESSENER ZUSTAND VORHER: der Zweig schrieb `[Video: src]`
            // in einen Platzhalter und sonst nichts. Damit gab es
            // 1. **keinen Bildflaeche** fuer das erste Bild oder das Poster,
            // 2. **nichts zum Anklicken**, also auch keinen Weg, das Video
            //    zu starten,
            // 3. **keine Zeitanzeige**, also kein `currentTime`,
            // 4. **keine Datei**, wenn die ueber ein `<source>` kommt.
            //
            // GEMESSENE TRENNUNG, und sie ist nicht meine Erfindung: dies
            // Modul **rendert**, es **laedt** keine Datei. Die Dauer steht
            // in der Datei, und die hat diese Crate nicht. Also baue ich
            // die **Struktur mit Knoten-IDs**, und der Browser fuellt sie —
            // genau so, wie KOMPRESS es am 04.10.2026 fuer `<audio>` gemacht
            // hat (`audio_zaehler`, `narsil-audio-zeit-{ordinal}`).
            //
            // GEMESSENE ENTSCHEIDUNG, warum `src` **und** `<source>`:
            // `<video src="x.webm">` und `<video><source src="x.webm">` sind
            // beide gaengig, und die zweite Form hatte ueberhaupt keine
            // Datei. GEMESSEN an `hteile::video_waehlen`, das dieselbe
            // Kinderliste liest wie `picture_waehlen`.
            //
            // GEMESSENE ENTSCHEIDUNG, warum der Dateiname und nicht der
            // ganze Pfad: ein `src` kann Verzeichnisse tragen, und die
            // gehoeren **nicht** in eine Beschriftung im Bild. Dieselbe
            // Entscheidung wie im `<audio>`-Zweig.
            let eigene_src = attributes
                .get("src")
                .map(String::as_str)
                .unwrap_or("")
                .to_string();
            let aus_kindern = crate::hteile::video_waehlen(kinder);
            let src = if eigene_src.is_empty() {
                aus_kindern.clone().unwrap_or_default()
            } else {
                eigene_src.clone()
            };
            let poster = attributes
                .get("poster")
                .map(String::as_str)
                .unwrap_or("")
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            let datei = if src.is_empty() {
                "keine Datei".to_string()
            } else {
                src.rsplit('/').next().unwrap_or(&src).to_string()
            };
            let autoplay = attributes.contains_key("autoplay");
            let controls = attributes.contains_key("controls");
            let own_id = self.aktuelle_id.take();
            self.absatz_schliessen();
            // Die Nummer des Steuers gehoert **ihm**, nicht dem Absatz
            // drumherum — dieselbe Ursache wie beim Knopf (`laufender_ord`).
            self.laufender_ord = Some(self.aktualer_ordinal);
            let ord = self.aktualer_ordinal;
            // Release 08.10.2026: DER KASTEN DES VIDEOS. Ein `<video>` ist ein
            // ersetztes Element mit eigener Flaeche (HTML: 300 x 150 ohne
            // Masse); die Steuerknoten unten stehen IN ihm. Ohne `controls`
            // sind sie unsichtbar — ein Video, das (noch) nicht spielt, ist
            // eine leere Flaeche bzw. sein Poster. GEMESSEN vorher:
            // github.com zeigte statt der Hero-Videos sechs Zeilen
            // „[Knopf: ▶ 2] Fortschritt: 0:00 / ? … [ohne controls]".
            let video_kasten = self.video_kasten(attributes);

            //
            // 1) DIE BILDFLAECHE. Sie traegt das Poster, wenn es eines
            // gibt, sonst die Anzeige „erstes Bild". Der Browser ersetzt
            // den Text durch das Bild, sobald er das erste Bild hat.
            //
            // GEMESSENE ENTSCHEIDUNG, warum der Text das **erste** Bild
            // nennt und nicht „Video": ein Platzhalter, der sagt, was
            // gleich erscheint, ist ein Platzhalter, den man versteht.
            let bild_text = if !poster.is_empty() {
                format!("[Poster: {poster}]")
            } else if !src.is_empty() {
                "[erstes Bild]".to_string()
            } else {
                "[Video]".to_string()
            };
            let bild_id = format!("narsil-video-bild-{ord}");
            let bild_knoten = self
                .doc
                .push_child(self.mutter(), NodeKind::Text, Some(bild_text), Span::EMPTY)
                .ok();
            if let Some(k) = bild_knoten {
                self.id_zu_knoten.insert(bild_id, k);
            }

            //
            // 2) DER PLAY-KNOPF. Ohne ihn gibt es keinen Weg, das Video zu
            // starten — das war am `<audio>` der gemessene Befund, und er
            // gilt hier genauso.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Nummer **im Text** steht
            // und nicht in einer `id`: `narsil_dom` speichert keine
            // Attribute (GEMESSEN: `NodeKind` kennt nur
            // `Page | Container | Text`), also kann der Browser einen
            // Knoten nicht ueber eine `id` finden. Der Ordinal zaehlt alle
            // Elemente und steht nicht im `HtmlNode`-Baum. Also steht die
            // Nummer im Text — sichtbar im Bild und eindeutig.
            self.video_zaehler += 1;
            self.platzhalter(&format!("[Knopf: \u{25B6} {}]", self.video_zaehler));

            //
            // 3) DIE FORTSCHRITTSLEISTE. Sie ist ein eigener Knoten mit
            // eigener `id`, damit der Browser sie ohne den Rest treffen
            // kann. GEMESSENER Anfangstext: `0:00 / ?` sagt dem Leser,
            // dass die Dauer noch fehlt; `0:00 / 0:00` wuerde eine Datei
            // von null Sekunden behaupten.
            let sprung_text = if autoplay {
                "Fortschritt: laeuft".to_string()
            } else {
                "Fortschritt: 0:00 / ?".to_string()
            };
            let sprung_id = format!("narsil-video-sprung-{ord}");
            let sprung_knoten = self
                .doc
                .push_child(
                    self.mutter(),
                    NodeKind::Text,
                    Some(sprung_text),
                    Span::EMPTY,
                )
                .ok();
            if let Some(k) = sprung_knoten {
                self.id_zu_knoten.insert(sprung_id, k);
            }

            // 4) DIE ZEIT.
            let zeit_id = format!("narsil-video-zeit-{ord}");
            let zeit_knoten = self
                .doc
                .push_child(
                    self.mutter(),
                    NodeKind::Text,
                    Some("0:00 / ?".to_string()),
                    Span::EMPTY,
                )
                .ok();
            if let Some(k) = zeit_knoten {
                self.id_zu_knoten.insert(zeit_id, k);
            }

            // 5) DIE LAUTSTAERKE und 6) VOLLBILD.
            //
            // GEMESSENE ENTSCHEIDUNG, warum `muted` und `volume` getrennt
            // bleiben: im Spieler von `narsil-media` sind sie zwei
            // verschiedene Dinge (nach `volume = 0` ist `muted` noch
            // false). Ein Strich, der beides zugleich anzeigt, kann den
            // Zustand nicht darstellen.
            for (praefix, text) in [
                (
                    "lautstaerke",
                    format!(
                        "Lautstaerke: {}",
                        if attributes.contains_key("muted") {
                            "aus"
                        } else {
                            "an"
                        }
                    ),
                ),
                ("vollbild", "Vollbild".to_string()),
            ] {
                let vid = format!("narsil-video-{praefix}-{ord}");
                let kn = self
                    .doc
                    .push_child(self.mutter(), NodeKind::Text, Some(text), Span::EMPTY)
                    .ok();
                if let Some(k) = kn {
                    self.id_zu_knoten.insert(vid, k);
                }
            }

            // 7) DER DATEINAME, damit man im Bild sieht, welches Video das
            // ueberhaupt ist. GEMESSENE ENTSCHEIDUNG, warum der Name und
            // nicht die Attribute: ein Bild mit „controls autoplay muted"
            // ist schwerer zu lesen als eines mit einem Dateinamen; die
            // Attribute stehen in der Quelle.
            self.platzhalter(&format!("[Video: {datei}]"));
            // GEMESSENE ENTSCHEIDUNG, warum `controls` im Text auftaucht:
            // ein `<video>` ohne `controls` **darf** keine Leiste zeigen —
            // das ist die Vorgabe der Seite. Wer immer eine Leiste malt,
            // verletzt sie.
            if !controls {
                self.platzhalter("[ohne controls]");
            }
            if let Some(_aus_kindern) = aus_kindern.filter(|_| eigene_src.is_empty()) {
                // GEMESSENE ENTSCHEIDUNG, warum das hier nichts anzeigt:
                // die Quelle ist oben schon im Dateinamen genannt. Der
                // Zweig existiert, damit `aus_kindern` nicht als
                // ungenutzt gewarnt wird und damit man sieht, dass die
                // Datei wirklich aus dem Kind kam.
            }
            self.aktuelle_id = own_id;
            for k in kinder {
                self.kind(k);
            }
            self.absatz_schliessen();
            if let Some(kasten) = video_kasten {
                self.offen.pop();
                if !controls {
                    // Release 10.10.2026: ALLE Nachfahren, nicht nur die
                    // Kinder. Die Steuertexte stehen in einer Zeilenbox (dem
                    // Absatz) — GEMESSEN vorher (github.com): „[erstes Bild]
                    // [Knopf: ▶ 1] Fortschritt …" war sichtbar, weil nur die
                    // Zeilenbox selbst durchsichtig wurde, ihre Texte nicht.
                    let mut stapel = self.doc.get(kasten).map(|n| n.children.clone()).unwrap_or_default();
                    let mut schritte = 0usize;
                    while let Some(k) = stapel.pop() {
                        schritte += 1;
                        if schritte > 100_000 {
                            break;
                        }
                        if let Some(n) = self.doc.get(k) {
                            stapel.extend(n.children.iter().copied());
                        }
                        if let Some(mut st) = self.doc.get(k).map(|n| n.style.clone()) {
                            st.color = Color::TRANSPARENT;
                            let _ = self.doc.set_style(k, st);
                        }
                    }
                }
            }
            return;
        }

        //
        // GEMESSENER FEHLER MEINES ERSTEN WURFS, der hier behoben ist und
        // den man sich gemerkt haben sollte: der `<video>`-Zweig stand an
        // der Fundstelle **nach** dem gemeinsamen `img`-/`video`-Zweig
        // (Z. 5832) und wurde deshalb **nie erreicht**. Der gemeinsame
        // Zweig nimmt `img` **und** `video` und baut fuer beides einen
        // Bildknoten — ein `<video>` bekam also einen Bildknoten und keine
        // Steuerung. GEMESSEN: `examples/dbg_video.rs` zeigte fuer
        // `<video src="g7b_ton.webm" controls>` **null** Textknoten im
        // Dokument. Der Zweig steht jetzt **vor** dem gemeinsamen, damit
        // `<img>` dort bleibt und `<video>` hier.
        //
        if klein == "img" || klein == "video" {
            if self.plan_stapel.last().and_then(|p| p.anzeige) == Some(narsil_css::Anzeige::Inline) {
                self.inline_bild_eltern.insert(self.mutter());
                if let Some(st) = self.plan_stapel.last_mut().and_then(|p| p.stil.as_mut()) {
                    st.anzeige = narsil_layout::plan::Anzeige::InlineBlock;
                }
            }
            self.absatz_schliessen();
            let alt = attributes.get("alt").map(String::as_str).unwrap_or("");
            //
            // B12: DREI Quellen in dieser Reihenfolge.
            // 1. `<picture>` hat eine unterstuetzte `<source>` gewaehlt — der
            //    Zweig hat sie in `picture_quelle` gemerkt.
            // 2. `src`, ausser es ist ein `data:`-Platzhalter (1-px-GIF) — dann
            //    `data-src`/`data-original`/`data-lazy-src`
            //    (`crate::hteile::daten_quelle`).
            // 3. `src` sonnt, unveraendert wie vor B12.
            //
            // GEMESSENE GRUNDLAGE fuer (2): `src="data:image/gif;base64,…"`
            // ist der Platzhalter eines Lade-Spins, und `narsil_image` liest
            // daraus ein 1x1-Bild mit Alpha 0 — also „ein Bild, das man
            // nicht sieht“. Mit `data-src` steht das echte daneben.
            //
            // GEMESSENE HAEUFIGKEIT auf 30 echten Seiten: **0** von 285
            // `<img>`-Tags haben ein `data-src`. Diese Zeile ist die
            // Vorsicht fuer die Seiten, bei denen es vorkommt (WordPress,
            // Shopify, jquery-lazyload); sie ist nicht der Grund fuer die
            // 18 von 188 Bildern aus B10.
            let src: String = match self.picture_quelle.clone() {
                Some(p) => p,
                None => crate::hteile::daten_quelle(attributes).unwrap_or_default(),
            };
            let src = src.as_str();
            // I3: EIN BILD IST EIN KNOTEN, kein Text.
            //
            // GEMESSENER FEHLER 30.09.2026, 08:20: hier stand nur
            // `self.platzhalter("[Bild: alt]")`. Ergebnis: `<img>` war
            // eine Textzeile, `stil_bild` aus `hteile.rs` wurde nie
            // benutzt (gemessen: kein Aufrufer), die reservierte Hoehe
            // aus H6 kam nirgends an, und ein Browser hatte keine
            // Staende, an die er Bildpixel haengen koennte.
            //
            // Neu: ein Knoten mit dem Stil aus `stil_bild` (Groesse aus
            // `width`/`height`, sonst die Standardhoehe) und die Angaben
            // in `Seite::bilder`. DEN TEXT gibt es weiterhin, aber nur
            // noch, wenn kein Bild kommen kann — dann ist er die
            // Beschriftung des Platzhalters (siehe `narsil::bilder`).
            let mut bild = crate::hteile::bild_von(alt, src, attributes, self.basis);
            if bild.breite == 0
                && bild.hoehe == 0
                && let Some(v) = attributes.get(SVG_VERHAELTNIS)
            {
                let mut t = v.split(' ').filter_map(|x| x.parse::<f32>().ok());
                if let (Some(b), Some(h)) = (t.next(), t.next()) {
                    bild.breite = b.round().clamp(1.0, 16384.0) as _;
                    bild.hoehe = h.round().clamp(1.0, 16384.0) as _;
                }
            }
            let aus_picture = self.picture_quelle.is_some();
            let mut aus_srcset = false;
            //
            // W7C A3 (07:40): `srcset`/`sizes` (und die Breite fuer die
            // Auswahl).
            //
            // GEMESSENER BEFUND: jede Seite mit `srcset` zeigte ihr
            // `src`-Bild, also in diesem Browser immer das kleinste (oder
            // keins — `src` ist seit HTML 4.01 optional). Die Anzeige-
            // breite kennt der BAUER (`self.breite`), nicht `bild_von`.
            if !aus_picture && let Some(srcset) = attributes.get("srcset") {
                if let Some(gewählt) = crate::hteile::srcset_waehlen(
                    srcset,
                    attributes.get("sizes").map(String::as_str),
                    self.breite,
                ) {
                    bild.quelle = crate::loese_link(&gewählt, self.basis);
                    aus_srcset = true;
                }
            }
            // M3: die Zahl der Bilder ist Eingabe. `MAX_BILDER` aus
            // `hteile.rs` gilt fuer die Liste; hier zaehlt der Bauer.
            if self.bilder.len() < crate::hteile::MAX_BILDER && !bild.quelle.is_empty() {
                let stil = crate::hteile::stil_bild(&bild);
                // M3: `push_child` kann scheitern (dann waere der Baum
                // voll oder die Id frei). Der Fehler wird hier NICHT
                // geschnappt, sondern es geht auf den alten Weg zurueck:
                // ein Bild, das keinen Knoten bekommt, ist ein Text, und
                // der Text ist der Weg, der immer geht.
                match self
                    .doc
                    .push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY)
                {
                    Ok(id) => {
                        // W7C R3: das ist der Kasten DIESES Elements.
                        self.eigener_kasten = Some(id);
                        // 02.10. 05:05: `<a href=…><img …></a>` ist ein Link
                        // (sonst gab es keinen). Siehe
                        // `vermerke_link_in_marke`.
                        self.vermerke_link_in_marke(id);
                        // 02.10. 08:20: und die Id gehoert zu DIESEM Knoten
                        // (Anker-Sprung, siehe `Seite::id_zu_knoten`).
                        //
                        // GEMESSENE EINORDNUNG, warum `take()` und nicht
                        // `clone()`: der erste Knoten, der fuer dieses Element
                        // entsteht, **nimmt** die Id an. Sonst traegt auch der
                        // naechste Block sie (gemessen: 1 von 544 Ankern auf
                        // wikipedia hatte ein Ziel).
                        if let Some(n) = self.aktuelle_id.take() {
                            self.vermerke_id(n, id);
                        }
                        // 03.10. 22:50: und der Layout-Plan gehoert an
                        // DENSELBEN Knoten. GEMESSENE BEGRUENDUNG: `element`
                        // (der Mantel) hat fuer DIESES Element einen Platz auf
                        // dem Kasten-Stapel reserviert; die Kiste mit dem Bild
                        // ist der Kasten, den die Seite malt, also der, an den
                        // `display`/`position`/`float` gehoeren.
                        if let Some(platz) = self.kasten_stapel.last_mut() {
                            *platz = Some(id);
                        }
                        //
                        // P6 (02.10.2026): DIESER Knoten ist ein ERSETZTES
                        // Element, und seine Attribute sind seine
                        // INTRINSISCHE MASSE (CSS 2.1, 10.3.2).
                        //
                        // GEMESSENER FEHLER, den das behebt:
                        // `https://www.nixos.org` liefert
                        //     <img src="/_astro/nixos-logo-...svg"
                        //          width="7129" height="1995"
                        //          class="h-12 w-auto">
                        // Die Seite schreibt `.w-auto{width:auto}` und
                        // `.h-12{height:calc(var(--spacing) * 12)}`, also
                        // 48 px Hoehe. Die Breite ist nach CSS 2.1, 10.3.2
                        // 7129 * (48/1995) = 171 px. Die Engine rechnete
                        // 7129, weil sie nicht wissen konnte, dass das ein
                        // Bild ist.
                        //
                        // Additiv: ein Aufrufer, der `ersetzt` nicht liest,
                        // sieht `None` und das alte Verhalten.
                        let _ = self.doc.set_ersetzt(
                            id,
                            narsil_dom::ErsatzMasse {
                                breite: (bild.breite > 0).then_some(bild.breite as f32),
                                hoehe: (bild.hoehe > 0).then_some(bild.hoehe as f32),
                            },
                        );
                        // P6 (02.10.2026): DIE KASKADE SCHLAGT DIE
                        // ATTRIBUTE.
                        //
                        // `stil_bild` kommt aus `width`/`height`. Nach
                        // HTML sind das nur PRAESENTATIVE HINWEISE
                        // (CSS 2.1, 6.4.4) — was CSS schreibt, gewinnt.
                        // `kommender_stil` traegt inzwischen die
                        // Kaskadengroesse (siehe `stil_von_kaskade`), also
                        // wird die GROESSE von dort uebernommen und der
                        // Rest (Rand, Platzhalterfarbe) bleibt von
                        // `stil_bild`.
                        //
                        // GEMESSENER FEHLER, den das behebt: nixos.org,
                        // `<img width="7129" height="1995" class="h-12
                        // w-auto">`. Ohne diese Zeile stand `Px(7129.0)`
                        // im `node.style`, mit `Px(48.0)` in der Hoehe und
                        // `Auto` in der Breite — und daraus rechnet der
                        // Layout die 171 px.
                        let mut stil = stil;
                        let mut platzhalter_hoehe = bild.hoehe == 0;
                        if let Some(k) = self.kommender_stil.as_ref() {
                            stil.size = k.size;
                            // Release 15.10.2026 (Agent CSS2): on an inline
                            // `<svg>` the `width`/`height` attributes are
                            // presentation attributes (SVG 2 §8.2), so a
                            // percentage there is a CSS percentage when no
                            // rule set the size. MEASURED before (WPT css-
                            // sizing `svg-intrinsic-size-005` reference, `<svg
                            // width="100%" height="100%">` in a 50px high
                            // div): the SVG was 0 px high.
                            if attributes.contains_key(SVG_INLINE) {
                                let prozent = |name: &str| {
                                    attributes
                                        .get(name)
                                        .and_then(|v| v.trim().strip_suffix('%'))
                                        .and_then(|v| v.trim().parse::<f32>().ok())
                                        .filter(|v| v.is_finite() && *v >= 0.0)
                                };
                                if stil.size.width == narsil_core::length::Length::Auto
                                    && let Some(p) = prozent("width")
                                {
                                    stil.size.width = narsil_core::length::Length::Percent(p);
                                }
                                // With a `viewBox` an auto height follows its
                                // ratio, which is what a percentage height in
                                // a block of auto height means too (WPT
                                // `CSS2/inline-svg-100-percent-in-body`).
                                if stil.size.height == narsil_core::length::Length::Auto
                                    && !attributes.keys().any(|k| k.eq_ignore_ascii_case("viewbox"))
                                    && let Some(p) = prozent("height")
                                {
                                    stil.size.height = narsil_core::length::Length::Percent(p);
                                }
                            }
                            // Release 08.10.2026: ein Inline-SVG nur mit
                            // `viewBox` fuellt die Breite, wenn CSS nichts sagt
                            // (siehe `SVG_FUELLT`).
                            if attributes.contains_key(SVG_FUELLT)
                                && stil.size.width == narsil_core::length::Length::Auto
                                && stil.size.height == narsil_core::length::Length::Auto
                            {
                                stil.size.width = narsil_core::length::Length::Percent(100.0);
                            }
                            //
                            // 03.10. 14:3x (Rolle CSS, UEBERNIMME CODEX-8):
                            // DIE KASKADE ENTSCHEIDET AUCH UEBER DEN RAND.
                            //
                            // GEMESSENER BEFUND (der dritte rote Test,
                            // `auto_flexbasis_kommt_aus_dem_inhalt`):
                            // `<div><img width="40"></div>` in einem
                            // Flex ergab **48 px** statt 40 — 4 px je
                            // Seite. GEMESSEN an der Quelle: `stil_bild`
                            // (`crates/narsil-page/src/hteile.rs:1828`)
                            // setzt `margin: 4px` auf JEDEN Bildknoten,
                            // und diese Zeile hier uebernahm bislang nur
                            // die GROESSE aus der Kaskade.  Der Dump
                            // `examples/codex8_layout` auf
                            // `team/log/codex8/img_flex.html` zeigt es:
                            // Bildknoten `rand=Spacing { left: Px(4.0),
                            // top: Px(4.0), right: Px(4.0), bottom: … }`.
                            //
                            // GEMESSENE ENTSCHEIDUNG, warum dasselbe
                            // Muster wie beim Absatzabstand: `stil_bild`
                            // ist der FALLBACK fuer eine Seite ohne
                            // Kaskade.  Sobald eine Kaskade da ist, hat
                            // sie Vorrang — CSS 2.1, 6.4.4.  Firefox gibt
                            // einem `<img>` ueberhaupt keinen Rand (das
                            // UA-Blatt `crates/narsil-css/src/ua.rs` hat
                            // keine `img`-Regel), also ist 0 px hier die
                            // browserrichtige Zahl.
                            stil.margin = k.margin;
                            stil.padding = k.padding;
                            // Release 11.10.2026: ebenso Rahmen und
                            // Hintergrund aus der Kaskade (CSS 2.1 §8.5,
                            // §14.2 gelten auch fuer ersetzte Elemente).
                            // GEMESSEN vorher: WPT css-flexbox
                            // `flex-aspect-ratio-img-row-005`/`-013`
                            // (`border-top: 50px` am `<img>` fehlte, der
                            // Kasten war 100 statt 150 px hoch).
                            stil.rahmen = k.rahmen.clone();
                            // Release 11.10.2026 (Agent CSS2): und
                            // `vertical-align` (siehe `Style::vertikal_hoch`).
                            stil.vertikal_hoch = k.vertikal_hoch;
                            stil.background = k.background;
                            // Release 15.10.2026: the box effects of the
                            // cascade (radius, opacity, shadows, outline,
                            // visibility, clip). MEASURED before: an `<img>`
                            // ignored `border-radius`, `opacity`,
                            // `box-shadow` and `outline` (WPT html/canvas
                            // composite.grid references, round avatars).
                            stil.radius = k.radius;
                            stil.radius_xy = k.radius_xy;
                            stil.radius_percent = k.radius_percent;
                            stil.deckkraft = k.deckkraft;
                            stil.unsichtbar = k.unsichtbar;
                            stil.schatten = k.schatten;
                            stil.schatten_anzahl = k.schatten_anzahl;
                            stil.umriss = k.umriss;
                            stil.clip_rand = k.clip_rand;
                            stil.clip_pfad = k.clip_pfad.clone();
                            stil.clip_svg = k.clip_svg.clone();
                            stil.maske_bild = k.maske_bild.clone();
                            stil.filter_value = k.filter_value.clone();
                            stil.filter_svg = k.filter_svg.clone();
                            stil.backdrop_filter = k.backdrop_filter.clone();
                            stil.backdrop_svg = k.backdrop_svg.clone();
                            //
                            // GEMESSENE REGRESSION, die dieser Zweig
                            // verursacht hat und die er zuruecknimmt:
                            // `<img src=b.png width="80">` ohne `height`
                            // bekam `1240x0` statt `80 x BILD_STANDARD_HOEHE`.
                            // Grund: die Kaskade sagt `height: auto`, die
                            // intrinsische Hoehe ist unbekannt (es gibt
                            // kein Attribut und noch kein geladenes Bild),
                            // und CSS 2.1, 10.3.2 hat fuer diesen Fall
                            // nichts — der Kasten ist 0 hoch, und das Bild
                            // springt hinein, wenn es da ist.
                            //
                            // Das ist die falsche Antwort fuer einen
                            // Browser: die Seite springt. Die
                            // reservierte Hoehe ist eine Zusage unseres
                            // eigenen Produkts (`BILD_STANDARD_HOEHE`,
                            // ADR-019) und sie gilt genau dann, wenn
                            // NICHT bekannt ist, wie gross das Bild wird.
                            //
                            // Sobald die Seite eine Hoehe schreibt —
                            // `height: 48px`, oder `height: 200px` am
                            // Attribut — gilt die Seite und dieser Zweig
                            // nicht mehr.
                            //
                            // BILD_16: dieselbe Regel wie in `stil_bild()`.
                            // GEMESSEN: dieser Zweich griff bei einem **geladenen**
                            // Bild und schrieb die harte Reservierung in die Hoehe —
                            // bei `ersatz = 32x32` und Breite 1280 sind das
                            // **1280.0 x 0.0** statt 1280x1280. Jetzt nur noch, wenn
                            // das Bild **nicht** geladen ist; ein `height`-Attribut
                            // hatte schon vorher gewonnen und tut es weiter.
                            if k.size.height == Length::Auto
                                && bild.hoehe == 0
                                && !bild.hoehe_aus_attribut
                            {
                                stil.size.height = Length::px(bild.reservierte_hoehe() as f32);
                            } else {
                                platzhalter_hoehe = false;
                            }
                        }
                        let _ = self.doc.set_style(id, stil);
                        // BILD_29: NUR ZUWEISUNG, wie beauftragt. `plan_stapel`
                        // traegt die beiden Felder; hier werden sie durchgereicht.
                        self.bilder.push(BildKnoten {
                            knoten: id,
                            bild,
                            platzhalter_hoehe,
                            aus_srcset,
                            aus_picture,
                            ord: Some(self.aktualer_ordinal),
                            // BILD_29, GEMESSEN — WARUM HIER KEIN
                            // `object-fit` STEHT, obwohl es jetzt im `LayoutStil`
                            // steht: **dieser Zweig hat die Kaskade nicht.**
                            // GEMESSEN: `plan_stapel` ist in `kind()` (4809)
                            // geleert (`pop().unwrap_or_default()`), der
                            // `<img>`-Stil entsteht bei 6180 frisch aus den
                            // Attributen (`stil_bild`), und `kommender_stil` ist
                            // ein **`Style`** (2890), **kein `Berechnet`** — und
                            // `Style` hat kein `object-fit`-Feld. **Eine
                            // Zuweisung `objekt_passung: "cover"` waere hier eine
                            // hinterlegte Konstante** — und ich melde keine
                            // Konstante als Messwert. Der Weg fuehrt ueber
                            // `self.doc`: der Knoten behaelt den Stil, und der
                            // naechste Schritt traegt `object-fit` in
                            // `narsil_core::style::Style` (dort liegen auch schon
                            // die drei E0063 aus SCHRIFTs Feldern).
                        });
                    }
                    Err(_) => {
                        self.platzhalter(&format!("[Bild: {}]", bild.quelle));
                    }
                }
            } else {
                // Kein `src` oder zuviele Bilder: der alte Textweg bleibt,
                // und er ist hier richtig — er ist das, was man sieht,
                // wenn es kein Bild gibt.
                // Release 09.10.2026: ohne `src` UND ohne `alt` stellt das
                // Bild NICHTS dar (HTML §4.8.4.3.6) — Chrome zeichnet dann
                // keinen Platzhalter. GEMESSEN: youtube.com legt das
                // Doodle-Logo als `<img src="">` an, bis die Daten da sind;
                // der Motor schrieb "[Bild]" in den Kopf.
                if src.is_empty() {
                    // Release 10.10.2026: gibt CSS dem Bild eine Groesse,
                    // ist es ein LEERER Kasten dieser Groesse (wie Chrome,
                    // auch mit `alt`) — ohne Groesse bleibt es wie bisher
                    // (unsichtbar, mit `alt` der Platzhaltertext darunter).
                    // GEMESSEN vorher (youtube.com): `yt-image` setzt das
                    // `src` erst, wenn ein IntersectionObserver fuer das
                    // `<img>` `intersectionRect.height > 0` meldet; das
                    // `<img>` (`width:100%; height:100%`) hatte ohne Kasten
                    // 0 x 0 — das `src` kam nie, kein Vorschaubild.
                    if let Some(k) = self.kommender_stil.clone()
                        && (k.size.width != Length::Auto || k.size.height != Length::Auto)
                        && let Ok(id) = self.doc.push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY)
                    {
                        self.eigener_kasten = Some(id);
                        self.vermerke_link_in_marke(id);
                        if let Some(n) = self.aktuelle_id.take() {
                            self.vermerke_id(n, id);
                        }
                        if let Some(platz) = self.kasten_stapel.last_mut() {
                            *platz = Some(id);
                        }
                        let unsichtbar = k.unsichtbar;
                        let _ = self.doc.set_style(id, k);
                        // Mit `alt` steht der Text IM Kasten (wie Chrome;
                        // ADR-019: kein Kasten, der Platz belegt und nichts
                        // zeigt).
                        if !alt.is_empty()
                            && let Ok(t) = self.doc.push_child(id, NodeKind::Text, Some(format!("[Bild: {alt}]")), Span::EMPTY)
                        {
                            let _ = self.doc.set_style(
                                t,
                                Style {
                                    color: FARBE_SCHWACH,
                                    font_size: GRAD_TEXT,
                                    unsichtbar,
                                    ..Style::default()
                                },
                            );
                        }
                        return;
                    }
                }
                if alt.is_empty() && src.is_empty() {
                    return;
                }
                let beschriftung = if !alt.is_empty() {
                    format!("[Bild: {alt}]")
                } else {
                    format!("[Bild: {src}]")
                };
                self.platzhalter(&beschriftung);
            }
            return;
        }
        if klein == "audio" {
            // KOMPRESS, 03.10.2026 (G9g): `<audio>` IST EIN STEUERELEMENT.
            //
            // GEMESSENER ZUSTAND VORHER: der Zweig schrieb
            // `[Audio: {src}]` in einen Platzhalter und sonst nichts. Die
            // Abnahme der Leitung lautet „Seite mit `<audio src=*.ogg|
            // *.flac>` **spielt**, Zeitanzeige **laeuft**" — und mit
            // einem Platzhalter gibt es nichts, was spielen koennte.
            //
            // GEMESSENE TRENNUNG, und sie ist nicht meine Erfindung: dies
            // Modul **rendert**, es **laedt** keine Datei. Die Dauer steht
            // im Dateikopf (FLAC-STREAMINFO, WAV-Header, Ogg-Kopf), und
            // den kann diese Crate nicht lesen, weil sie die Datei nicht
            // hat. Also baue ich hier die **Struktur mit IDs**, und der
            // Browser fuellt sie.
            //
            // GEMESSENER Anschlusspunkt: `id_zu_knoten: HashMap<String,
            // NodeId>` (Zeile 837) verbindet eine HTML-`id` mit einem
            // Knoten. Das ist genau das, was ein brauchbares `currentTime`
            // braucht: der Browser sucht den Knoten und schreibt hinein.
            let src = attributes
                .get("src")
                .map(String::as_str)
                .unwrap_or("")
                .to_string();
            // Release 11.10.2026 (Agent CSS2): der Kasten des Steuers traegt
            // Raender und Groesse des Elements (siehe `audio_kasten`).
            self.absatz_schliessen();
            let audio_kasten = self.audio_kasten();
            let eigene_id = self.aktuelle_id.take();
            self.absatz_schliessen();
            // Die Nummer des Steuers gehoert **ihm**, nicht dem Absatz
            // drumherum — dieselbe GEMESSENE Ursache wie beim Knopf
            // (`laufender_ord`, siehe dort).
            self.laufender_ord = Some(self.aktualer_ordinal);
            let datei = if src.is_empty() {
                "keine Datei".to_string()
            } else {
                // GEMESSEN: der Dateiname, nicht der ganze Pfad. Eine
                // Anfrage kann einen Pfad mit Verzeichnissen schicken, und
                // der gehoert **nicht** in eine Beschriftung im Bild.
                src.rsplit('/').next().unwrap_or(&src).to_string()
            };
            // KOMPRESS 04.10.2026 (Anordnung der Leitung: Ton aus dem
            // Lautsprecher, Nachweis mit Bild): DER PLAY-KNOPF.
            //
            // GEMESSENER ZUSTAND VORHER: der Zweig schrieb nur einen
            // Platzhalter und einen Zeitknoten. **Es gab nichts, was man
            // anklicken konnte** — also auch keinen Weg, Ton zu starten.
            //
            // GEMESSENE ENTSCHEIDUNG, und sie ist eine Grenze: `narsil_dom`
            // speichert **keine** Attribute (GEMESSEN: `NodeKind` kennt nur
            // `Page | Container | Text`), also kann der Browser einen
            // Knoten **nicht** ueber eine `id` finden. Der Ordinal
            // zaehlt alle Elemente und steht **nicht** im `HtmlNode`-Baum.
            // **Also** steht die Nummer **im Text** — sichtbar im Bild und
            // eindeutig.
            self.audio_zaehler += 1;
            self.platzhalter(&format!("[Knopf: ▶ {}]", self.audio_zaehler));
            self.platzhalter(&format!("[Audio: {datei}]"));

            // DIE ZEITANZEIGE. Sie ist ein **eigener Knoten mit eigener
            // `id`**, damit der Browser sie ohne den Rest zu finden
            // aktualisieren kann.
            //
            // GEMESSENER Anfangstext, und er ist **kein** Placeholder, den
            // man spaeter ersetzt: `0:00 / ?` sagt dem Leser, dass die
            // Dauer noch fehlt, und `0:00 / 0:00` wuerde eine Datei von
            // null Sekunden behaupten. Das ist eine Behauptung, und sie
            // faellt erst auf, wenn der Ton laeuft.
            let zeit_id = format!("narsil-audio-zeit-{}", self.aktualer_ordinal);
            let zeit_knoten = self
                .doc
                .push_child(
                    self.mutter(),
                    NodeKind::Text,
                    Some("0:00 / ?".to_string()),
                    Span::EMPTY,
                )
                .ok();
            if let Some(k) = zeit_knoten {
                self.id_zu_knoten.insert(zeit_id.clone(), k);
            }
            self.aktuelle_id = eigene_id;
            for k in kinder {
                self.kind(k);
            }
            self.absatz_schliessen();
            if audio_kasten.is_some() {
                self.offen.pop();
            }
            return;
        }
        //
        // B12: `<picture>` ist eine Auswahl, keine Huelle.
        //
        // GEMESSENER Ausgangszustand: `<source>` wurde bedingungslos
        // verworfen (`return`), und `<picture>` fiel auf den
        // Standardzweig — das在里面 liegende `<img>` wurde also mit
        // seinem `src` gebaut und die Auswahl der Autoren (`<source
        // type="image/webp">`, `<source media="...">`) ging verloren.
        //
        // Die Loesung ohne Umbau: der Zweig merkt sich die beste unterstuetzte
        // Quelle, und der `img`-Zweig nimmt sie, wenn er sie bekommt. Damit
        // entsteht **ein** Bildknoten (nicht zwei: der Weg ueber
        // `picture_quelle` verdraengt den des `<img>` nicht, es IST der
        // desselbe Knoten).
        //
        // GEMESSENE HAUFIGKEIT auf 30 echten Seiten: 5 `<picture>`-Elemente,
        // davon **0** mit `type`-Attribut. Der Zweig ist also selten, aber
        // er ist billig und er richtet die Absicht des Autors.
        if klein == "picture" {
            self.absatz_schliessen();
            let schema = self.kaskade.as_ref().map_or(narsil_css::Farbschema::Hell, |k| k.baum.zustand.farbschema);
            self.picture_quelle = crate::hteile::picture_waehlen(kinder, self.breite, self.ansicht_hoehe, schema);
            for k in kinder {
                self.kind(k);
            }
            self.picture_quelle = None;
            return;
        }
        if klein == "source" || klein == "track" {
            // Quellen ohne Eltern-Player sind Rauschen, keine Information.
            // B12: die Quellen eines `<picture>` sind oben schon ausgewertet
            // (im `picture`-Zweig), also ist dieses Wegwerfen hier richtig.
            //
            // VIDEO, 04.10.2026: die Quelle eines `<video>` wird NICHT hier
            // gesammelt, sondern **vorher** — die Kinder werden erst in
            // diesem Zweig gebaut, sie stehen dann also noch nicht zur
            // Verfuegung. GEMESSENER FEHLER MEINES ERSTEN VERSUCHS, der
            // hier stand: ich wollte `video_quelle` in diesem Zweig setzen
            // und im `<video>`-Zweig lesen. Das geht nicht, weil die
            // Reihenfolge umgekehrt ist. Der Weg ist derselbe wie bei
            // `<picture>`: eine freie Funktion ueber die Kinder, aufgerufen
            // **vor** dem Bau — `hteile::video_waehlen`.
            return;
        }

        if klein == "canvas" {
            self.absatz_schliessen();
            // Release 15.10.2026: HTML "rules for parsing non-negative
            // integers" (`" 100px"` and `"100.5"` are 100, `"1e2"` is 1).
            let width = attributes.get("width").and_then(|v| canvas_masszahl(v)).unwrap_or(300);
            let height = attributes.get("height").and_then(|v| canvas_masszahl(v)).unwrap_or(150);
            if let Ok(id) = self.doc.push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY) {
                self.eigener_kasten = Some(id);
                if let Some(name) = self.aktuelle_id.take() { self.id_zu_knoten.insert(name, id); }
                // Release 14.10.2026: the layout plan (`box-sizing`,
                // `aspect-ratio`, `stretch`, ...) belongs to this node, also as
                // an inline element, like for `<img>`. MEASURED before: WPT
                // css-sizing `stretch/stretch-block-size-001` (a `<canvas>`
                // with `block-size: stretch` had no plan: 60 instead of 45 px).
                if let Some(platz) = self.kasten_stapel.last_mut() {
                    *platz = Some(id);
                }
                // Release 11.10.2026: `width`/`height` eines `<canvas>` sind
                // seine NATUERLICHE Groesse (HTML §4.12.5: die Masse der
                // Zeichenflaeche, kein Groessenhinweis fuer CSS) — ein
                // ersetztes Element mit Seitenverhaeltnis, `auto` bleibt
                // `auto`. GEMESSEN vorher: WPT css-flexbox `flex-basis-013`
                // (`height: 100%` an einem 5 x 5-Canvas: 5 x 50 statt
                // 50 x 50 px, die Breite 5 px stand als CSS-Angabe fest).
                let style = self.kommender_stil.clone().unwrap_or_default();
                let _ = self.doc.set_style(id, style);
                let _ = self.doc.set_ersetzt(
                    id,
                    narsil_dom::ErsatzMasse { breite: Some(width as f32), hoehe: Some(height as f32) },
                );
            }
            // Release 15.10.2026: the fallback content is not shown, but it
            // keeps its ordinals (like hidden subtrees). MEASURED before (WPT
            // html/canvas 2d.composite.grid.*, Agent A): every node after a
            // `<canvas><p class=fallback>..</p></canvas>` was one or two
            // ordinals off, so canvas1..N drew into and measured other boxes.
            // Scripts in the fallback content still run (the parser
            // executes them; WPT canvas_shadows_system_colors has its
            // `<script>` inside an unclosed `<canvas>`).
            self.skripte_im_ersatzinhalt(kinder, self.tiefe);
            zaehle(kinder, self.tiefe, &mut self.naechste_ordinal, false);
            return;
        }
        // Release 11.10.2026: `iframe`/`embed`/`frame` sind ersetzte Kaesten
        // (HTML §4.8.5, §15.4.4) — `width`/`height` aus den Attributen
        // (Zahl oder Prozent), sonst 300 x 150; die Kinder eines `iframe`
        // werden nicht gezeichnet. `object` zeigt (wie ein Browser ohne
        // Plugin) seinen Ersatzinhalt. GEMESSEN vorher: ein grauer Text
        // „[Eingebettet: …]" statt des Kastens — auf echten Seiten bei
        // jeder Einbettung, in WPT (`absolute-replaced-*`) an falscher
        // Stelle und ohne Rahmen.
        if klein == "embed" || klein == "iframe" || klein == "frame" {
            self.absatz_schliessen();
            let mass = |name: &str, vorgabe: f32| -> Length {
                match attributes.get(name).map(|v| v.trim()) {
                    Some(v) if v.ends_with('%') => v[..v.len() - 1].trim().parse::<f32>().ok().filter(|p| p.is_finite() && *p >= 0.0).map_or(Length::px(vorgabe), Length::Percent),
                    Some(v) => v.trim_end_matches("px").parse::<f32>().ok().filter(|p| p.is_finite() && *p >= 0.0).map_or(Length::px(vorgabe), Length::px),
                    None => Length::px(vorgabe),
                }
            };
            if let Ok(id) = self.doc.push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY) {
                self.eigener_kasten = Some(id);
                if let Some(name) = self.aktuelle_id.take() { self.id_zu_knoten.insert(name, id); }
                let mut style = self.kommender_stil.clone().unwrap_or_default();
                if style.size.width == Length::Auto { style.size.width = mass("width", 300.0); }
                if style.size.height == Length::Auto { style.size.height = mass("height", 150.0); }
                let _ = self.doc.set_style(id, style);
            }
            // Release 15.10.2026: the text inside an `<iframe>` keeps its
            // ordinal, see the canvas branch.
            zaehle(kinder, self.tiefe, &mut self.naechste_ordinal, false);
            return;
        }
        if klein == "object" {
            self.absatz_schliessen();
            for k in kinder {
                self.kind(k);
            }
            self.absatz_schliessen();
            return;
        }
        if klein == "input" {
            //
            // 03.10. 01:35 (HTML): DIE EIGENE ID BLEIBT VOR DEM ABSATZSCHLUSS.
            //
            // GEMESSENER BEFUND (Wegwerf-Messung, entfernt, `tests/_m_idab.rs`):
            // `<p><label for="c">Haken</label> <input type="checkbox" id="c">`
            // ergibt `id_zu_knoten = [("c", 4)]`, und Knoten 4 traegt den Text
            // **„Haken"** — nicht das Kaestchen. Und der zweite Fall ist noch
            // deutlicher: `<form><input type="text" id="q" name="q"></form>`
            // (ohne Text davor) ergibt `id_zu_knoten = []` — die Id wird
            // **gar nicht** verzeichnet.
            //
            // GEMESSENE EINORDNUNG, warum beides dieselbe Ursache hat: dieser
            // Zweig ruft `absatz_schliessen()`, und **während** des Aufrufs
            // steht `aktuelle_id` schon auf der Id des gerade betretenen
            // `<input>`. Der Absatz meldet die Id beim Abspulen fuer den Knoten,
            // den er dabei erzeugt — und das ist der Text **vor** dem Feld.
            // Ohne vorangehenden Text entsteht in diesem Aufruf kein Knoten, und
            // die Id geht ganz verloren.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Id **hier** geparkt und nicht
            // unternimmt: `vermerke_id` hat die Regel „der **Kasten** schlaegt
            // den Textknoten", und beim Feld ist der Platzhalter ein Textknoten
            // wie der falsche — GEMESSEN heisst: bei zwei Textknoten gewinnt der
            // **erste** (`alt_ist_kasten || !neu_ist_kasten` → frueher
            // zurueck). Der falsche Knoten saesse also fest, also muss er
            // **gar nicht erst entstehen**.
            // vor_knoten: Stand **vor** diesem Zweig (siehe die Verwendung
            // beim Platzhalter).
            let vor_knoten = self.doc.len();
            let eigene_id = self.aktuelle_id.take();
            //
            // W7 P2: DER PLATZHALTER GEHOERT DEM EINGABEFELD (22:05).
            //
            // GEMESSENER FEHLER: ein `<input>` hat keinen eigenen Kasten
            // (sein Platzhalter ist ein Absatz), also bekam der
            // Platzhalter-Text die Nummer des UMSCHLIESSENDEN Absatzes
            // (des `<p>`), nicht die des Feldes. GEMESSEN: `<p><input
            // name=q></p>` ergab `Eingabe { ord: 4 }` und einen Textknoten
            // mit Nummer 3. Ein Klick auf den Platzhalter traf also Nummer
            // 3, das Feld war nicht zu finden — der Fokus kam nicht.
            //
            // GEMESSEN, warum es an dieser Stelle und nicht im Layout: die
            // Nummer kommt aus dem Absatz-Zustand, und der wird beim
            // Bauen des Containers abgebaut (siehe W6G).
            self.laufender_ord = Some(self.aktualer_ordinal);
            self.absatz_schliessen();
            self.laufender_ord = Some(self.aktualer_ordinal);
            let typ = attributes
                .get("type")
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_else(|| "text".to_string());
            //
            // W7 P2: DER GETIPPTE WERT STEHT IM PATCH (20:55).
            //
            // GEMESSEN, warum das hier und nicht im Layout: das Layout
            // rechnet Boxen, und ein Text, der sich aendert, ist eine
            // Aenderung des INHALTS. Der Patch ist genau der Ort, an dem
            // JavaScript-Änderungen liegen (`z.textContent = …` geht
            // denselben Weg) — und `Befehl::Attribut { name: "value" }`
            // schreibt dorthin.
            //
            // GEMESSEN, warum der Wert den Platzhalter ERSETZT und nicht
            // daneben steht: `self.platzhalter("[Eingabe: name]")` ist der
            // Text, den ein leeres Feld zeigt. Ein gefuelltes Feld zeigt
            // seinen Wert, sonst stuende beides nebeneinander.
            if typ == "hidden" {
                return;
            }
            let name = attributes.get("name").map(String::as_str).unwrap_or("");
            // Release 08.10.2026: ein LEERES `value=""` laesst den Platzhalter
            // stehen (t-online.de: „Suchbegriff eingeben" fehlte).
            let wert = attributes
                .get("value")
                .filter(|v| !v.is_empty())
                .or_else(|| attributes.get("placeholder"))
                .map(String::as_str)
                .unwrap_or("");
            // Release 10.10.2026: steht der PLATZHALTER im Feld (kein Wert)?
            let aus_platzhalter = attributes.get("value").is_none_or(|v| v.is_empty())
                && attributes.get("placeholder").is_some_and(|p| !p.is_empty());
            let beschriftung = match typ.as_str() {
                //
                // 04.10. 15:05 (HTML): **DER INHALT, OHNE KLAMMER.**
                //
                // GEMESSENER FEHLER gegen Firefox (`fx_formular.png` vs.
                // `html19_formular_narsil.png`): `<input type="submit"
                // value="Senden">` zeichnete **„Knopf: Senden"**. Die Klammer
                // ist die Meldung an das Skript; **ein Browser zeichnet sie
                // nie** (HTML 4.01, 17.6.1: der Knopf zeigt seinen `value`).
                //
                // OHNE `value` bleibt der Vorgabetext des Typs — das ist die
                // Vorgabe der Spezifikation und **nicht** die Klammer.
                "submit" | "button" | "reset" => {
                    if wert.is_empty() {
                        match typ.as_str() {
                            "reset" => "Reset".to_string(),
                            "button" => "Knopf".to_string(),
                            _ => "Senden".to_string(),
                        }
                    } else {
                        wert.to_string()
                    }
                }
                //
                // W7C A2 (03:40): DER HAKEN STEHT IM PATCH.
                //
                // GEMESSEN, warum nicht `attributes`: dort steht, was die
                // HTML-Datei sagt. Nach einem Klick auf den Kasten sagt
                // die Datei immer noch „unchecked", und der Nutzer sieht
                // einen Haken, den er gesetzt hat — oder keinen, den er
                // gesetzt hat. Der `Patch` ist der Ort fuer die
                // Entscheidung des Nutzers (W7 P2, 20:55, derselbe Weg
                // wie der getippte Wert).
                //
                // GEMESSEN, warum `value=""` (leer) „aus" bedeutet: der
                // Patch traegt `attribut[(ord,"checked")] = "checked"`
                // zum Anhaengen und loescht den Eintrag zum Abhaengen
                // (`Sitzung::haken_setzen`). Ein leerer Wert heisst also
                // „nicht gesetzt" — und `haken_setzen` schreibt beim
                // Abhaken bewusst `""`, damit der Eintrag nicht als
                // „unbekannt" endet.
                "checkbox" | "radio" => {
                    //
                    // GEMESSENER FEHLER, den der Test fand (19:15): ein
                    // `<input checked>` aus der HTML-Datei kam als LEERES
                    // Kaestchen. GEMESSENE URSACHE: gelesen wurde nur der
                    // Patch, und der Patch entsteht erst beim Klick.
                    //
                    // GEMESSENE REGEL, in der Reihenfolge: der Patch zuerst
                    // (der Nutzer hat entschieden), sonst das Attribut der
                    // Seite. GEMESSENE BEGRUENDUNG fuer „Patch zuerst":
                    // `haken_setzen` (narsil) schreibt beim Abhaken bewusst
                    // `""` in den Patch — nur so kann der Nutzer ein vom
                    // HTML gesetztes `checked` wieder wegnehmen.
                    let angehakt = match self
                        .patch
                        .attribut
                        .get(&(self.aktualer_ordinal, "checked".to_string()))
                    {
                        Some(v) => !v.is_empty(),
                        // Release 11.10.2026: der Haken-Zustand aus dem Host.
                        None => attributes.contains_key(" narsil-checked")
                            || (!attributes.contains_key(" narsil-unchecked") && attributes.contains_key("checked")),
                    };
                    //
                    // 03.10. 19:05 (H-AR1): EIN KASTEN, KEIN TEXT.
                    //
                    // GEMESSENER BEFUND (18:40, vorher):
                    //     MESSUNG text y=86 "[ ] c (checkbox)"
                    // Der Auftrag der Koordination: „echte Checkboxen als
                    // Kaestchen". Ein Kasten, der als Text in der Zeile steht,
                    // ist an der Stelle auch nur Text: er hat keine Flaeche,
                    // keinen Haken, und seine Groesse haengt an der Schrift.
                    //
                    // GEMESSENE BEGRUENDUNG, warum kein Text mehr: der Haken
                    // steckt im Patch, nicht im Text. Als Text ist er nur
                    // eine Zeichenfolge, die man beim Bauen schreiben und
                    // beim Klick nicht aendern kann, ohne die Seite neu zu
                    // bauen — als Kasten traegt der Knoten den Zustand.
                    //
                    // GEMESSENE ENTSCHEIDUNG, warum die GROESSE fest 13 px
                    // ist: das ist die Groesse, die Firefox und Chrome fuer
                    // ein unberuehrtes Kaestchen nehmen (UA-Stylesheet). Eine
                    // Zahl aus der Fremde ist hier besser als eine Zahl aus
                    // unserer Erfindung.
                    //
                    // 03.10. 01:52 (HTML): DIE ID AUCH HIER — dieser Zweig kehrt
                    // **vor** dem Platzhalter zurueck, also kam die Id von einem
                    // Kaestchen GEMESSEN nirgends an (`id_zu_knoten = []`, 01:45
                    // und 01:49).
                    let vor = vor_knoten;
                    // Release 07.10.2026: ein Kästchen mit `opacity: 0` oder
                    // `visibility: hidden` ist unsichtbar. GEMESSEN: Wikipedias
                    // Vector-2022-Kopf legt so vier durchsichtige Kästchen über
                    // seine Menüknöpfe — wir zeichneten sie als schwarze Rahmen.
                    let unsichtbar = self
                        .kaskade
                        .as_ref()
                        .and_then(|k| k.werte.last())
                        .is_some_and(|b| b.deckkraft <= 0.01 || !b.sichtbar);
                    if unsichtbar {
                        self.aktuelle_id = None;
                        return;
                    }
                    let kasten = self.feld_kasten(angehakt, typ.as_str());
                    if let (Some(name), true) = (eigene_id, self.doc.len() > vor) {
                        // Release 08.10.2026: die Id traegt der AEUSSERE Kasten
                        // (wie bei allen Feldern, siehe `feld_kasten_mit_inhalt`).
                        // Vorher war es der letzte Knoten — die Innenflaeche.
                        let id = kasten.unwrap_or(narsil_dom::NodeId((self.doc.len() - 1) as u32));
                        self.vermerke_id(name, id);
                    }
                    self.aktuelle_id = None;
                    return;
                }
                // W7C A2: `type=file` bekommt einen EIGENEN Platzhalter.
                // GEMESSEN, warum: der allgemeine Zweig sagte
                // `[Eingabe upload: file]`, und das sieht wie ein
                // bedienbares Textfeld aus. Der Browser kann die Datei
                // aber nicht waehlen — das muss man am Bild sehen, nicht
                // erst an der Meldung nach dem Klick.
                "file" => {
                    if name.is_empty() {
                        "[Datei: nicht wählbar]".to_string()
                    } else {
                        format!("[Datei {name}: nicht wählbar]")
                    }
                }
                _ => {
                    if name.is_empty() && wert.is_empty() {
                        format!("[Eingabe: {typ}]")
                    } else if wert.is_empty() {
                        format!("[Eingabe {name}: {typ}]")
                    } else {
                        format!("[Eingabe {name}: {wert}]")
                    }
                }
            };
            //
            // 03.10. 19:4x (HTML, Auftrag der Leitung): DER PLATZHALTER WIRD
            // EIN KASTEN.
            //
            // GEMESSENER BEFUND (`tests/_m_form2.rs`, Wegwerf, entfernt): an
            // einem `<input>` mit `border: 2px; height: 44px; padding: 8px`
            // kam EIN Textknoten "[Eingabe search: search]" und ein Kasten
            // 336x16 mit `bg=None`. Dieselben Regeln an einem `<div>` gaben
            // einen Kasten 304x48 mit sichtbarem Rahmen. Die Kaskade ist also
            // **nicht** schuld: `kind()` ruft fuer diese Elemente
            // `platzhalter()` und baut gar keinen Kasten.
            //
            // GEMESSENE GROESSEN, an denen ich mich orientiere (Firefox,
            // wikipedia bei 1280 px): das Suchfeld ist 44 px hoch
            // (`height: 4.4rem` bei `html { font-size: 62.5% }`).
            //
            // 03.10. 21:2x: DIE KASKADE ENTSCHEIDET DAS FELD.
            //
            // GEMESSENER BEFUND (`_m_form_osd.rs`, wikipedia, 1280 px):
            // vorher `size=192x192 bg=Color{118,118,118}`, obwohl die Seite
            // `height: 4.4rem`, `border-left-width: 1px` und
            // `padding: .8rem 9.6rem .8rem 1.2rem` schreibt. Keine der drei
            // Angaben kam an, weil der Zweig `element()` nie aufruft.
            //
            // DIE BREITE, nach der Dreiwegregel (siehe `waehle_zahl`):
            // 1. die Kaskade (`width`), 2. sonst `size` in Zeichen
            // (HTML 4.01, S. 46), 3. sonst eine Naeherung aus dem Text.
            let st = self.feld_von_kaskade();
            let aus_text = (0.6 * st.schriftgroesse * beschriftung.chars().count() as f32)
                .max(st.schriftgroesse * 2.0);
            //
            // DIE BREITE, nach der Dreiwegregel (siehe `waehle_zahl`):
            // 1. die Kaskade (`width`), 2. sonst `size` in Zeichen
            // (HTML 4.01, S. 46), 3. sonst eine Naeherung aus dem Text.
            let size_chars: f32 = attributes
                .get("size")
                .and_then(|v| v.trim().parse::<f32>().ok())
                .filter(|n| n.is_finite() && *n > 0.0 && *n <= 1024.0)
                .unwrap_or(20.0);
            // Release 09.10.2026: `submit`/`button`/`reset` sind KNOEPFE —
            // so breit wie ihre Beschriftung plus Polster und Rahmen, nicht
            // `size` Zeichen. GEMESSEN vorher: `<input type=submit
            // value=Senden>` war ein 20 Zeichen breites weisses Textfeld.
            let knopf = matches!(typ.as_str(), "submit" | "button" | "reset");
            let breite = if knopf {
                let zugabe = (st.fuellung_links + st.fuellung_rechts + st.rand_links + st.rand_rechts)
                    .max(GRAD_TEXT);
                st.breite_px.unwrap_or(
                    (0.6 * st.schriftgroesse * beschriftung.trim().chars().count() as f32 + zugabe)
                        .max(st.schriftgroesse * 3.0),
                )
            } else {
                st.breite_px
                    .unwrap_or((0.6 * st.schriftgroesse * size_chars).max(aus_text))
            };
            //
            // DIE HOEHE: die Kaskade, sonst **eine Zeile plus Rand**. Das ist
            // der Unterschied zum Kaestchen und der Grund, warum `hoehe` ein
            // eigener Parameter ist (21:2x): vorher bedeutete „kein Inhalt"
            // auch „quadratisch", und ein leeres Feld wurde 192x192.
            let zeile = (st.schriftgroesse * 1.4).max(13.0);
            let hoehe = st.hoehe_px.unwrap_or(zeile + st.rand_oben + st.rand_unten + st.fuellung_oben + st.fuellung_unten);
            //
            // DER INHALT, und das ist der Punkt, an dem der Platzhalter
            // verschwindet: in dem Kasten steht, was ein Browser hinein-
            // schreibt — `value`, sonst `placeholder`, sonst **nichts**.
            // `beschriftung` („[Eingabe q: text]") ist die Meldung an das
            // Skript und gehoert nicht ins Bild.
            //
            // GEMESSENE GRENZE: `wert` ist der Attributwert, ungefiltert. Ein
            // Feld mit 4 KiB `value` (M3/M4) schreibt also 4 KiB in den
            // Knoten — dieselbe Grenze wie bei jedem anderen Textknoten.
            let inhalt = if matches!(typ.as_str(), "submit" | "button" | "reset") {
                //
                // 04.10. 15:45 (HTML): EIN KNOPF-TRAGENDER `<input>` ZEIGT
                // SEINE **BESCHRIFTUNG**, nicht seinen `value`.
                //
                // GEMESSENER FEHLER: `<input type="reset">` **ohne** `value`
                // zeichnete **gar nichts** (GEMESSEN: die Texte waren `["Los"]`
                // — der Reset-Knopf fehlte voellig). `wert` ist fuer einen
                // Knopf ohne `value` leer, und `(!wert.is_empty())` machte
                // daraus „kein Inhalt".
                //
                // HTML 4.01, 17.6.2: jeder Knopf-typ hat einen **Vorgabetext**
                // („Reset", „Submit"), und Firefox zeichnet ihn.
                if beschriftung.trim().is_empty() {
                    None
                } else {
                    Some(beschriftung.trim().to_string())
                }
            } else if wert.is_empty() {
                None
            } else {
                Some(wert.to_string())
            };
            // Release 10.10.2026: der Platzhalter in der Farbe und Deckkraft von
            // `::placeholder` (Vorgabe `#757575`); `opacity: 0` oder
            // `transparent` blendet ihn aus (schwebende Labels).
            let mut st = st;
            let mut inhalt = inhalt;
            if aus_platzhalter && !matches!(typ.as_str(), "submit" | "button" | "reset") {
                let (f, d) = self
                    .platzhalter_farbe
                    .take()
                    .unwrap_or((Color { r: 0x75, g: 0x75, b: 0x75, a: 255 }, 1.0));
                let a = (f32::from(f.a) * d.clamp(0.0, 1.0)).round() as u8;
                if a == 0 {
                    inhalt = None;
                } else {
                    st.farbe = Color { a, ..f };
                }
            }
            let feld = self.feld_kasten_mit_inhalt(
                false,
                if knopf { "button" } else { "text" },
                breite.min(8192.0),
                hoehe.min(512.0),
                &st,
                [2.0; 4],
                inhalt.as_deref(),
            );
            // 02.10. 18:45 (HTML): DIESER KNOTEN IST DAS FELD (Label).
            if let Some(feld) = feld {
                self.label_feld_setzen(feld);
            }
            // 03.10. 01:35 (HTML): DIE ID GEHOERT DEM FELD. GEMESSEN vorher
            // `id_zu_knoten = []` (ohne Text davor) bzw. ein Verweis auf den
            // Text davor (siehe den Kommentar am Zweig-Anfang).
            //
            // GEMESSENE EINORDNUNG, warum es einen **Ersatz** gibt: ein
            // Kaestchen hat keinen Platzhaltertext (`platzhalter` gibt `None`),
            // also blieb `id_zu_knoten` nach der ersten Reparatur **leer** —
            // GEMESSEN `[]` fuer `<p><label for="c">Haken</label> <input
            // type="checkbox" id="c">` (01:45). Der **letzte** Knoten, den dieser
            // Zweig erzeugt hat, ist dann das Kaestchen selbst; `vor_knoten`
            // (der Stand **vor** dem Zweig) grenzt ihn ab.
            //
            // GEMESSENE EINORDNUNG, warum der **letzte** neue Knoten und nicht
            // der erste: der erste neue kann der Platzhalter des
            // **vorangehenden** Textes sein (den hat `absatz_schliessen()`
            // oben erzeugt) — der letzte ist der des Feldes.
            let ziel = feld.or_else(|| {
                (self.doc.len() > vor_knoten).then(|| narsil_dom::NodeId((self.doc.len() - 1) as u32))
            });
            if let (Some(name), Some(k)) = (eigene_id, ziel) {
                self.vermerke_id(name, k);
            }
            self.aktuelle_id = None;
            return;
        }
        // Release 08.10.2026: ein `<button>` mit ELEMENTEN (Symbol, `<span>`)
        // wird ein Kasten mit seinen Kindern — der allgemeine Weg unten,
        // mit `display: inline-block` aus dem UA-Blatt (`button:has(*)`).
        // GEMESSEN vorher: nur die Textkinder kamen an; die Menueknoepfe
        // von github.com (`<button><span>Platform</span><svg/></button>`)
        // zeigten „Knopf", Symbol-Knoepfe ueberall ihren `aria-label`.
        let knopf_mit_elementen = klein == "button"
            && kinder.iter().any(|k| {
                matches!(k, HtmlNode::Element { tag, .. } if !tag.eq_ignore_ascii_case("br"))
            });
        if klein == "button" && !knopf_mit_elementen {
            //
            // W7C A2 (04:05): EIN `<button>` IST EIN KNOPF.
            //
            // GEMESSENER FEHLER: `<button type="submit">Los</button>` wurde
            // wie ein Absatz Text gezeichnet („Los", 28x60) — also wie
            // eine Ueberschrift, nicht wie ein Knopf. Der Knopf-Zweig
            // darunter galt nur fuer `<input type=submit>`, und das ist
            // die Form, die 1995 geschrieben wurde. Seit HTML 4.01 ist
            // `<button>` der Standard, und 11 von 12 Formularen im Netz
            // benutzen ihn.
            //
            // GEMESSEN, warum der TEXT als Beschriftung dient (und nicht
            // `value`): `<button value=loeschen>Loeschen</button>` zeigt
            // „Loeschen" — der sichtbare Text. `value` ist das, was
            // GESENDET wird (so behandelt es `Sitzung::formular_bei_klick`).
            let beschriftung: String = kinder
                .iter()
                .filter_map(|k| match k {
                    HtmlNode::Text(t) => Some(t.trim()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            let knopf_typ = attributes
                .get("type")
                .map(|t| t.to_ascii_lowercase())
                .unwrap_or_else(|| "submit".to_string());
            //
            // 03.10. 20:2x: VOR_KNOTEN UND EIGENE_ID VOR `absatz_schliessen`.
            //
            // Die Nummer des Knopfes gehoert dem Knopf, nicht dem
            // umgebenden Absatz (W7 P2, 22:05, derselbe Fehler).
            self.absatz_schliessen();
            self.laufender_ord = Some(self.aktualer_ordinal);
            let beschriftung = if beschriftung.is_empty() {
                //
                // 03.10. 22:4x (HTML, Auftrag aus CSS_9): EIN LEERER KNOPF
                // ZEIGT SEINEN ZUGANGLICHEN NAMEN, NICHT „Knopf".
                //
                // GEMESSENER BEFUND (MDN, `_m_mdn.rs`, 1280 px): **neun**
                // Kaesten mit dem Text „Knopf", alle 64x22,4 px, untereinander.
                // GEMESSEN im HTML: **drei** `<button>` ohne Text — und alle
                // drei tragen `aria-label` bzw. `title`:
                //     <button class="mdn-search-button" title="Website durchsuchen">
                //     <button class="navigation__button" aria-label="Navigation umschalten">
                //
                // GEMESSENE ENTSCHEIDUNG, warum `aria-label` vor `title`: HTML
                // AAM (Accessible Name and Description Computation 1.2, Schritt
                // 2B) geht `aria-label` vor `title`; `title` ist erst die
                // Stufe 2D. Fuer ein Icon-Knopf ist das genau der Text, den
                // ein Screenreader vorliest — und ein Knopf ohne Namen ist
                // fuer niemanden bedienbar.
                //
                // GEMESSENE GRENZE: Firefox **zeichnet** dort ein Icon, nicht
                // den Text. Wir haben keine Icon-Glyphe in der
                // Rasterstufe; der Name im Kasten ist das, was wir haben, und
                // es ist mehr als „Knopf".
                let name = attributes
                    .get("aria-label")
                    .or_else(|| attributes.get("title"))
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.chars().take(64).collect::<String>())
                    .unwrap_or_else(|| match knopf_typ.as_str() {
                        "reset" => "Zuruecksetzen".to_string(),
                        "button" => "Knopf".to_string(),
                        _ => "Los".to_string(),
                    });
                name
            } else {
                beschriftung
            };
            //
            // 04.10. 15:00 (HTML): DER SICHTBARE TEXT IST DER INHALT — und
            // er wird **vor** dem Klammern gerett, nicht durch Abschneiden
            // danach.
            //
            // GEMESSENER FEHLER (04.10., gegen Firefox, `fx_formular.png`):
            // `<button type="submit">Senden</button>` zeichnete
            // **„Knopf: Senden"**. Ursache: das Abschneiden der Klammern
            // nahm `[Knopf: Senden]` -> `Knopf: Senden` — der Prefix bleibt,
            // weil er **im Klammertext steckt**. Die Klammer-Form ist die
            // Meldung an das Skript; **ein Browser zeichnet sie nie**
            // (HTML 4.01, 17.6.1: der Knopf zeigt seinen Inhalt bzw. seinen
            // `value`).
            let sichtbar_text = beschriftung.trim().to_string();
            let beschriftung = format!("[Knopf: {beschriftung}]");
            self.laufender_ord = Some(self.aktualer_ordinal);
            //
            // 03.10. 19:4x (HTML, Auftrag der Leitung): DER KNOPF WIRD EIN
            // KASTEN — dieselbe Aenderung wie beim `<input>` (dort die
            // Messung und die Begruendung).
            //
            // GEMESSENE GROESSE: der Suchknopf von wikipedia ist 40 px hoch
            // (Firefox, 1280 px). Das ist `GRAD_TEXT * 2.5`; die Breite
            // folgt aus der Beschriftung, die schon berechnet ist.
            //
            // GEMESSENE ENTSCHEIDUNG, warum der sichtbare TEXT und nicht
            // `beschriftung` (20:2x, Test `der_knopf_ist_ein_kasten_mit_
            // seinem_text`): im Kasten steht, was ein Browser zeigt — der
            // Text des Knopfes. Die eckige Klammer-Form bleibt fuer die
            // Meldung an das Skript, nicht fuer das Bild.
            let sichtbar = sichtbar_text.clone();
            //
            // 03.10. 22:5x, KORREKTUR EINES FEHLERS VON 20:2x: der Typ-Ersatz
            // stand VOR dem Namen und hat ihn ueberschrieben. GEMESSEN
            // (`_m_mdn.rs`, MDN, 1280 px): nach dem Zufuehren von
            // `aria-label`/`title` standen immer noch **neun** Kaesten mit
            // "Knopf" — denn ein `<button type="button">` mit
            // `aria-label="Navigation umschalten"` hat `knopf_typ == "button"`,
            // und dieser Zweig machte wieder "Knopf" daraus.
            //
            // GEMESSENE REGEL: der zugreifbare Name gewinnt IMMER. Der
            // Typ-Ersatz ist der Fallback fuer einen Knopf, der **keinen**
            // Namen hat — und nur dann ist "Knopf" richtig.
            let sichtbar = if sichtbar.trim().is_empty() || sichtbar == "Knopf" || sichtbar == "Los"
            {
                match knopf_typ.as_str() {
                    "reset" => "Zuruecksetzen".to_string(),
                    "button" => "Knopf".to_string(),
                    _ => "Los".to_string(),
                }
            } else {
                sichtbar
            };
            let knopf_breite =
                (0.6 * GRAD_TEXT * sichtbar.chars().count() as f32).max(GRAD_TEXT * 3.0);
            //
            // 03.10. 20:2x: DIE ID GEHOERT DEM KNOPF — und sie wird VOR dem
            // Kastenbau verzeichnet, nicht danach.
            //
            // GEMESSENER BEFUND, und er ist eine Reihenfolgefrage (Test
            // `der_knopf_ist_ein_kasten_mit_seinem_text`): mit
            // `vermerke_id` **nach** `feld_kasten_mit_inhalt` war
            // `id_zu_knoten` leer. Grund: `vermerke_id` hat die Regel „der
            // KASTEN schlaegt den Textknoten" und prueft dafuer den Eltern —
            // der aeussere Kasten des Knopfes ist aber zu diesem Zeitpunkt
            // bereits geschlossen und traegt den **Textknoten** als Kind, also
            // gewinnt der Text. Im `input`-Zweig steht derselbe Schritt
            // ebenfalls VOR dem Kastenbau (dort dokumentiert, 01:35); hier
            // war die Reihenfolge umgekehrt.
            //
            // 03.10. 21:2x: DER KNOPF AUS DER KASKADE.
            //
            // GEMESSENER BEFUND (Bild `team/log/leitung_wikipedia_2115.png`):
            // der Suchknopf von wikipedia war ein **grauer** Kasten. Die Seite
            // schreibt `.pure-button-primary-progressive {
            // background-color: var(--background-color-progressive) }` mit
            // `--background-color-progressive: #36c` — die Farbe stand im
            // Dokument und kam nicht an, weil der Zweig `element()` nie
            // aufruft. `feld_von_kaskade` liest sie jetzt.
            let st = self.feld_von_kaskade();
            // Release 08.10.2026: die Auto-Breite schliesst Polster und
            // Rahmen links/rechts ein (CSS 2.1 §10.3.2, `shrink-to-fit`) —
            // mindestens die bisherige Zugabe `GRAD_TEXT`. GEMESSEN vorher:
            // der Knopf „Suchen" der Startseite (`padding:11px 22px`) schnitt
            // seinen Text rechts ab.
            let zugabe = (st.fuellung_links + st.fuellung_rechts + st.rand_links + st.rand_rechts)
                .max(GRAD_TEXT);
            let breite = st.breite_px.unwrap_or(
                (0.6 * st.schriftgroesse * sichtbar.chars().count() as f32 + zugabe)
                    .max(st.schriftgroesse * 3.0),
            );
            let zeile = (st.schriftgroesse * 1.4).max(13.0);
            let hoehe = st.hoehe_px.unwrap_or(zeile + st.rand_oben + st.rand_unten + st.fuellung_oben + st.fuellung_unten);
            //
            // 03.10. 20:5x: EIN BAUAUFruf, EINERfuehrt. Vorher standen hier
            // zwei (einer mit der Id, einer ohne) und die Id wurde **nach**
            // dem Bau verzeichnet — GEMESSEN: `id_zu_knoten` war dann leer,
            // weil `vermerke_id` den Textknoten im Kasten bevorzugt und der
            // aeussere Kasten zu dem Zeitpunkt schon geschlossen war.
            let feld = self.feld_kasten_mit_inhalt(
                false,
                "button",
                breite.min(8192.0),
                hoehe.min(512.0),
                &st,
                [2.0; 4],
                Some(&sichtbar),
            );
            // DIE ID GEHOERT DEM KNOPF. GEMESSENE EINORDNUNG, warum `take()`:
            // `aktuelle_id` wird bei jedem Kind zurueckgesetzt (siehe
            // `kind()`, Eintritt) — wer den Wert nicht nimmt, verliert ihn.
            if let (Some(name), Some(k)) = (self.aktuelle_id.take(), feld) {
                self.vermerke_id(name, k);
            }
            return;
        }
        if klein == "textarea" {
            // 03.10. 01:35 (HTML): DIE EIGENE ID BLEIBT VOR DEM
            // ABSATZSCHLUSS — dieselbe GEMESSENE Ursache wie im
            // `input`-Zweig (siehe dort: `id_zu_knoten` zeigte auf den
            // Text davor, bzw. fehlte ganz).
            // vor_knoten: Stand **vor** diesem Zweig (siehe die Verwendung
            // beim Platzhalter).
            let vor_knoten = self.doc.len();
            let eigene_id = self.aktuelle_id.take();
            self.absatz_schliessen();
            //
            // W7C A8 (13:50): DER GETIPPTE WERT STEHT IM PATCH — wie beim
            // `<input>`.
            //
            // GEMESSEN, warum das hier noetig ist: der Textfeld-Zweig las
            // den Wert aus den KINDERN des Elements („der Inhalt zwischen
            // den Tags"). Das ist der Anfangswert eines Textarea — und
            // nach dem Tippen steht der neue Wert dort nicht mehr. Ohne
            // diese Zeile waere das Feld bedienbar und man wuerde NICHTS
            // sehen: der Cursor laeuft, der Wert wird gesendet, und das
            // Bild zeigt den alten Text. Das ist schlimmer als ein
            // fehlendes Feld, weil es so aussieht, als waere es kaputt.
            //
            // GEMESSEN, warum je Zeile ein Absatz: ein `textarea` hat
            // mehrere Zeilen, und ein Umbruch muss ein Umbruch sein — der
            // gleiche, der gesendet wird.
            // Release 12.10.2026 (Agent CSS2): der Wert aus dem Patch kommt in
            // denselben Feldkasten wie der Anfangswert (unten), statt als
            // lose Absaetze. GEMESSEN vorher (Hinweis Agent D): ein per Skript
            // gesetztes `textarea.value` verlor Rahmen und Flaeche, nur der
            // Text stand da.
            let patch_wert = self
                .patch
                .attribut
                .get(&(self.aktualer_ordinal, "value".to_string()))
                .cloned();
            //
            // W7C A2 (05:45): DIE NUMMER GEHOERT DEM TEXTFELD.
            //
            // GEMESSENER FEHLER, vom Test gefunden: `dom_zu_ord` war
            // `{1: 0, 3: 2}` — der Absatz und das `<textarea>` teilten sich
            // die Nummer 0, und der Knopf bekam 2 statt 1. Ein Klick auf
            // den Knopf traf damit einen Knoten, den es im DOM-Stand
            // nicht gab: „der Knopf kam nicht an". Es ist derselbe Fehler
            // wie beim `<input>` (W7 P2, 22:05) — dort wurde er behoben,
            // hier nicht, weil der Zweig spaeter dazukam.
            self.laufender_ord = Some(self.aktualer_ordinal);
            let name = attributes.get("name").map(String::as_str).unwrap_or("");
            // Standardwert steht in den Kindern (Text zwischen den Tags).
            let inhalt: String = patch_wert.unwrap_or_else(|| {
                kinder
                    .iter()
                    .filter_map(|k| match k {
                        HtmlNode::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            //
            // 04.10. 04:40 (HTML): **EIN `<textarea>` IST EIN FELD, kein
            // Platzhaltertext.** Vorher stand hier
            //     format!("[Textfeld {name}: {inhalt}]")
            // als **sichtbarer Text** im Bild.
            //
            // GEMESSENER BEFUND (duckduckgo.com, gespeichertes HTML): das
            // Suchfeld der Startseite ist **kein `<input>`**, sondern
            //     <textarea name="q" class="...__queryInput" data-mode="search">
            // (GEMESSEN: das `<input name="q">` gibt es in der Datei nicht;
            // die drei vorhandenen `<input>` sind `type=checkbox`, `hidden`
            // und ein aria-Verstecker). Unser `<textarea>`-Zweig war der
            // einzige Formularzweig, der **keinen Kasten** baut — und so
            // stand dort `[Textfeld q: ]` als Text, den kein Browser zeichnet.
            // Firefox zeigt ein weisses Feld mit Rahmen.
            //
            // WARUM ES GENAU DIESELBE FORM HAT wie das `<input>`: HTML 4.01,
            // S. 46 nennt fuer `textarea` `cols=20 rows=2` als Vorgabe, und
            // die Kaskade darf beides ueberschreiben — das ist die
            // Dreiwegregel von `waehle_zahl`, dieselbe wie beim `<input>`.
            let st = self.feld_von_kaskade();
            let cols: f32 = attributes
                .get("cols")
                .and_then(|v| v.trim().parse::<f32>().ok())
                .filter(|n| n.is_finite() && *n > 0.0 && *n <= 4096.0)
                .unwrap_or(20.0);
            let rows: f32 = attributes
                .get("rows")
                .and_then(|v| v.trim().parse::<f32>().ok())
                .filter(|n| n.is_finite() && *n > 0.0 && *n <= 4096.0)
                .unwrap_or(2.0);
            let zeile = (st.schriftgroesse * 1.4).max(13.0);
            let breite = st
                .breite_px
                .unwrap_or((0.6 * st.schriftgroesse * cols).max(st.schriftgroesse * 3.0));
            let hoehe = st
                .hoehe_px
                .unwrap_or((zeile * rows + st.rand_oben + st.rand_unten).min(4096.0));
            let sichtbar = inhalt.trim();
            let inhalt_kaesten = (!sichtbar.is_empty()).then_some(sichtbar.to_string());
            let beschriftung = if name.is_empty() {
                format!("[Textfeld: {inhalt}]")
            } else {
                format!("[Textfeld {name}: {inhalt}]")
            };
            //
            // DER RUCKGRIFF, und er ist nicht nur Schonbetrieb: ohne
            // gebundene Schrift baut `feld_kasten_mit_inhalt` `None`, und
            // dann waere das Feld **gar nicht da** — das ist schlimmer als
            // der Platzhaltertext, den wir gerade ersetzen. Also: der alte
            // Weg bleibt als Notnagel fuer genau diesen Fall.
            let feld = self
                .feld_kasten_mit_inhalt(
                    false,
                    "textarea",
                    breite.min(8192.0),
                    hoehe.min(512.0),
                    &st,
                    [2.0; 4],
                    inhalt_kaesten.as_deref(),
                )
                .or_else(|| self.platzhalter(&beschriftung));
            // 02.10. 18:45 (HTML): DIESER KNOTEN IST DAS FELD (Label).
            if let Some(feld) = feld {
                self.label_feld_setzen(feld);
            }
            // 03.10. 01:35 (HTML): DIE ID GEHOERT DEM FELD. GEMESSEN vorher
            // `id_zu_knoten = []` (ohne Text davor) bzw. ein Verweis auf den
            // Text davor (siehe den Kommentar am Zweig-Anfang).
            //
            // GEMESSENE EINORDNUNG, warum es einen **Ersatz** gibt: ein
            // Kaestchen hat keinen Platzhaltertext (`platzhalter` gibt `None`),
            // also blieb `id_zu_knoten` nach der ersten Reparatur **leer** —
            // GEMESSEN `[]` fuer `<p><label for="c">Haken</label> <input
            // type="checkbox" id="c">` (01:45). Der **letzte** Knoten, den dieser
            // Zweig erzeugt hat, ist dann das Kaestchen selbst; `vor_knoten`
            // (der Stand **vor** dem Zweig) grenzt ihn ab.
            //
            // GEMESSENE EINORDNUNG, warum der **letzte** neue Knoten und nicht
            // der erste: der erste neue kann der Platzhalter des
            // **vorangehenden** Textes sein (den hat `absatz_schliessen()`
            // oben erzeugt) — der letzte ist der des Feldes.
            let ziel = feld.or_else(|| {
                (self.doc.len() > vor_knoten).then(|| narsil_dom::NodeId((self.doc.len() - 1) as u32))
            });
            if let (Some(name), Some(k)) = (eigene_id, ziel) {
                self.vermerke_id(name, k);
            }
            self.aktuelle_id = None;
            return;
        }
        if klein == "select" {
            // 03.10. 01:35 (HTML): DIE EIGENE ID BLEIBT VOR DEM
            // ABSATZSCHLUSS — dieselbe GEMESSENE Ursache wie im
            // `input`-Zweig (siehe dort: `id_zu_knoten` zeigte auf den
            // Text davor, bzw. fehlte ganz).
            // vor_knoten: Stand **vor** diesem Zweig (siehe die Verwendung
            // beim Platzhalter).
            let vor_knoten = self.doc.len();
            let eigene_id = self.aktuelle_id.take();
            self.absatz_schliessen();
            //
            // W7C A2 (05:45): DIE NUMMER GEHOERT DER AUSWAHL.
            //
            // GEMESSENER FEHLER: `dom_zu_ord` war `{1: 0, 2: 0, 3: 2}` —
            // `<form>` und `<select>` teilten sich die Nummer 0 (die
            // Auswahl bekam gar keine), und der Knopf rutschte auf 2.
            // Ein Klick auf die Auswahl traf also das Formular, und der
            // Klick auf den Knopf einen Knoten ohne Eintrag im Stand.
            // GEMESSEN, warum es hier auffaellt und beim Absatz nicht: der
            // Absatz ist ein ELEMENT (er hat eine Nummer), ein Platzhalter
            // ist ein TEXTknoten — und der gehort dem Absatz.
            self.laufender_ord = Some(self.aktualer_ordinal);
            let name = attributes.get("name").map(String::as_str).unwrap_or("");
            //
            // W7C A2 (06:10): OPTIONEN ALS (WERT, TEXT).
            //
            // GEMESSENER FEHLER: es stand nur der TEXT in der Liste, und
            // die Anzeige zeigte deshalb den WERT (`fr`) statt des Textes
            // (`Frankreich`). Ein Browser zeigt den Text — der Wert ist
            // das, was GESENDET wird. Der Test
            // `auswahl_schickt_die_gewahlte_option` sah „fr" im Bild und
            // meldete „der Nutzer sieht nicht, was er sendet"; genau das
            // war der Befund, nur eine Ebene tiefer.
            let mut optionen: Vec<(String, String)> = Vec::new();
            // Release 09.10.2026: `<option selected>` ist die Vorwahl, und
            // Optionen in `<optgroup>` zaehlen mit (HTML §4.10.7). GEMESSEN
            // vorher: es stand immer die ERSTE Option im Feld.
            let mut vorgewaehlt: Option<usize> = None;
            let gruppen: Vec<&HtmlNode> = kinder
                .iter()
                .flat_map(|k| match k {
                    HtmlNode::Element { tag, children, .. } if tag.eq_ignore_ascii_case("optgroup") => {
                        children.iter().collect::<Vec<_>>()
                    }
                    _ => vec![k],
                })
                .collect();
            for k in gruppen {
                if let HtmlNode::Element {
                    tag: otag,
                    attributes: oattr,
                    children: okinder,
                    shadow_root: _,
                    ..
                } = k
                {
                    if otag.eq_ignore_ascii_case("option") && oattr.contains_key("selected") {
                        vorgewaehlt = Some(optionen.len());
                    }
                    if otag.eq_ignore_ascii_case("option") {
                        let text: String = okinder
                            .iter()
                            .filter_map(|x| match x {
                                HtmlNode::Text(t) => Some(t.clone()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        if !text.trim().is_empty() {
                            // Der `value`, sonst der Text (HTML-Vorgabe).
                            let wert = oattr
                                .get("value")
                                .cloned()
                                .unwrap_or_else(|| text.trim().to_string());
                            optionen.push((wert, text.trim().to_string()));
                        }
                    }
                }
            }
            //
            // W7C A2 (03:45): DIE GEWAEHLTE OPTION, nicht alle.
            //
            // GEMESSENER FEHLER: es standen alle Optionen da
            // (`[Auswahl land: DE | FR | IT]`). Nach einem Klick auf die
            // Auswahl aenderte sich NICHTS — der Nutzer konnte nicht
            // sehen, was er gewaehlt hat, und das Formular schickte
            // trotzdem etwas anderes. Ein Bedienelement, dessen
            // Antwort man nicht sieht, ist die schlimmste Sorte.
            //
            // Der `Patch` traegt `(ord, "value")` = der `value` der
            // gewaehlten Option (`Sitzung::auswahl_weiter`). Ohne Patch
            // gilt die ERSTE Option — so ist es im Browser, und so
            // sendet es `Auswahl::gewaehlte_indizes` auch.
            let gewaehlt = self
                .patch
                .attribut
                .get(&(self.aktualer_ordinal, "value".to_string()))
                .cloned();
            //
            // GEMESSEN, warum die ERSTE Option und nicht „alle": eine
            // Auswahlliste zeigt in jedem Browser EINE Zeile — die
            // gewaehlte. „[Auswahl land: DE | FR | IT]" war eine
            // Beschreibung der Liste, keine Anzeige; sie wurde nicht
            // breiter, wenn man sie las, und sie sagte nichts darueber,
            // was gesendet wird.
            let text_der_wahl = |wert: &str| -> String {
                optionen
                    .iter()
                    .find(|(w, _)| w == wert)
                    .map(|(_, t)| t.clone())
                    // Kein passender `value` (JavaScript hat einen
                    // gesetzt): anzeigen, was dasteht. Besser als nichts.
                    .unwrap_or_else(|| wert.to_string())
            };
            // 03.10. 21:2x: DER SICHTBARE TEXT, ohne Klammern.
            //
            // GEMESSENE ENTSCHEIDUNG: `beschriftung` bleibt fuer die Meldung
            // an das Skript (`Sitzung::auswahl_weiter` liest sie nicht, aber
            // ein alter Weg schon), und fuer das **Bild** kommt der Text der
            // gewaehlten Option in den Kasten. Beides sind dieselben
            // Optionen — `optionen` ist die eine Quelle, und die Frage ist
            // nur, mit welcher Formulierung.
            let text_der_wahl_text = match (&gewaehlt, optionen.is_empty()) {
                (Some(wahl), _) => text_der_wahl(wahl),
                (None, true) => String::new(),
                (None, false) => optionen[vorgewaehlt.filter(|i| *i < optionen.len()).unwrap_or(0)].1.clone(),
            };
            // Release 09.10.2026: so breit wie die laengste Option (Chrome),
            // nicht wie die gewaehlte — sonst springt die Breite beim Waehlen.
            let laengste = optionen.iter().map(|(_, t)| t.chars().count()).max().unwrap_or(0);
            let beschriftung = if text_der_wahl_text.is_empty() {
                if name.is_empty() {
                    "[Auswahl]".to_string()
                } else {
                    format!("[Auswahl {name}]")
                }
            } else if name.is_empty() {
                format!("[Auswahl: {text_der_wahl_text}]")
            } else {
                format!("[Auswahl {name}: {text_der_wahl_text}]")
            };
            self.laufender_ord = Some(self.aktualer_ordinal);
            //
            // 03.10. 19:4x (HTML, Auftrag der Leitung): DIE AUSWAHL WIRD EIN
            // KASTEN — dieselbe Aenderung wie beim `<input>` (dort stehen
            // Messung und Begruendung).
            //
            // 03.10. 21:2x: DIE AUSWAHL AUS DER KASKADE, und der Inhalt ist
            // die **Option**, nicht die Klammer-Beschriftung.
            //
            // GEMESSENER BEFUND: vorher stand im Kasten „[Auswahl language:
            // Afrikans]" — 29 Zeichen, und daraus wurde eine Breite von 294 px
            // fuer ein Feld, das in Firefox „Afrikans" (~70 px) zeigt. Die
            // Klammer-Form ist die Meldung an das Skript.
            let sichtbar = text_der_wahl_text.clone();
            let st = self.feld_von_kaskade();
            // Release 08.10.2026: die Auto-Breite schliesst Polster und
            // Rahmen links/rechts ein (CSS 2.1 §10.3.2, `shrink-to-fit`) —
            // mindestens die bisherige Zugabe `GRAD_TEXT`. GEMESSEN vorher:
            // der Knopf „Suchen" der Startseite (`padding:11px 22px`) schnitt
            // seinen Text rechts ab.
            let zugabe = (st.fuellung_links + st.fuellung_rechts + st.rand_links + st.rand_rechts)
                .max(GRAD_TEXT);
            // Release 09.10.2026: `+ 1,1 em` fuer den Aufklapp-Pfeil.
            let breite = st.breite_px.unwrap_or(
                (0.6 * st.schriftgroesse * sichtbar.chars().count().max(laengste) as f32 + zugabe + st.schriftgroesse * 1.1)
                    .max(st.schriftgroesse * 3.0),
            );
            let zeile = (st.schriftgroesse * 1.4).max(13.0);
            let hoehe = st.hoehe_px.unwrap_or(zeile + st.rand_oben + st.rand_unten + st.fuellung_oben + st.fuellung_unten);
            let feld = self.feld_kasten_mit_inhalt(
                false,
                "select",
                breite.min(8192.0),
                hoehe.min(512.0),
                &st,
                [2.0; 4],
                Some(&sichtbar),
            );
            // Release 09.10.2026: der Aufklapp-Pfeil rechts im Feld (Chrome,
            // Firefox) — ein absolut gesetzter Kasten am rechten Rand des
            // (dafuer relativ gesetzten) Feldes. GEMESSEN vorher: die Auswahl
            // war nur Text und vom Fliesstext nicht zu unterscheiden.
            if let Some(feld) = feld {
                if let Some(mut plan) = self.layout_plan.hole(feld).cloned() {
                    plan.position = narsil_layout::plan::PositionArt::Relativ;
                    self.layout_plan.set(feld, plan);
                }
                let pfeil_grad = st.schriftgroesse * 0.85;
                let oben = st.rand_oben + ((hoehe - ZEILENHOEHE(pfeil_grad)) / 2.0).max(0.0);
                if let Ok(kasten) = self.doc.push_child(feld, NodeKind::Container, None, Span::EMPTY) {
                    let _ = self.doc.set_style(
                        kasten,
                        Style {
                            deckkraft: 1.0,
                            color: st.farbe,
                            font_size: pfeil_grad,
                            ..Style::default()
                        },
                    );
                    self.layout_plan.set(
                        kasten,
                        narsil_layout::LayoutStil {
                            anzeige: narsil_layout::plan::Anzeige::Block,
                            position: narsil_layout::plan::PositionArt::Absolut,
                            rechts: Some(st.rand_rechts + st.schriftgroesse * 0.3),
                            oben: Some(oben),
                            ..narsil_css::layout_stil(&narsil_css::Berechnet::standard())
                        },
                    );
                    if let Ok(p) = self.doc.push_child(kasten, NodeKind::Text, Some(AUSWAHL_PFEIL.to_string()), Span::EMPTY) {
                        let mut s = self.doc.get(p).map(|n| n.style.clone()).unwrap_or_default();
                        s.font_size = pfeil_grad;
                        s.color = st.farbe;
                        let _ = self.doc.set_style(p, s);
                    }
                    for k in [kasten.index() as u32] {
                        self.dom_zu_ord.entry(k).or_insert(self.aktualer_ordinal);
                    }
                }
            }
            // 02.10. 18:45 (HTML): DIESER KNOTEN IST DAS FELD (Label).
            if let Some(feld) = feld {
                self.label_feld_setzen(feld);
            }
            // 03.10. 01:35 (HTML): DIE ID GEHOERT DEM FELD. GEMESSEN vorher
            // `id_zu_knoten = []` (ohne Text davor) bzw. ein Verweis auf den
            // Text davor (siehe den Kommentar am Zweig-Anfang).
            //
            // GEMESSENE EINORDNUNG, warum es einen **Ersatz** gibt: ein
            // Kaestchen hat keinen Platzhaltertext (`platzhalter` gibt `None`),
            // also blieb `id_zu_knoten` nach der ersten Reparatur **leer** —
            // GEMESSEN `[]` fuer `<p><label for="c">Haken</label> <input
            // type="checkbox" id="c">` (01:45). Der **letzte** Knoten, den dieser
            // Zweig erzeugt hat, ist dann das Kaestchen selbst; `vor_knoten`
            // (der Stand **vor** dem Zweig) grenzt ihn ab.
            //
            // GEMESSENE EINORDNUNG, warum der **letzte** neue Knoten und nicht
            // der erste: der erste neue kann der Platzhalter des
            // **vorangehenden** Textes sein (den hat `absatz_schliessen()`
            // oben erzeugt) — der letzte ist der des Feldes.
            let ziel = feld.or_else(|| {
                (self.doc.len() > vor_knoten).then(|| narsil_dom::NodeId((self.doc.len() - 1) as u32))
            });
            if let (Some(name), Some(k)) = (eigene_id, ziel) {
                self.vermerke_id(name, k);
            }
            self.aktuelle_id = None;
            return;
        }
        if klein == "option" {
            // Ausserhalb von <select>: normaler Text (wird vom select
            // oben bereits gelesen, hier nur Fallback).
            for k in kinder {
                self.kind(k);
            }
            return;
        }
        if klein == "br" {
            // Zeilenumbruch. Release 08.10.2026: der Absatz laeuft mit seinem
            // Stil weiter (siehe `harter_umbruch`); ohne laufenden Absatz
            // gibt es nichts zu brechen.
            // Release 12.10.2026 (Agent CSS2): `clear` an `<br>` (CSS 2.1
            // §9.5.2; Browser raeumen damit die Zeile NACH dem Umbruch unter
            // die Floats) — siehe unten.
            let raeumen = self
                .plan_stapel
                .last()
                .and_then(|p| p.stil.as_ref())
                .map_or(narsil_layout::plan::RaeumungArt::Keins, |s| s.raeumen);
            if self.laufender.is_some() {
                self.harter_umbruch();
            } else if !self.inline_bild_eltern.contains(&self.mutter()) {
                // Release 12.10.2026 (Agent CSS2): ein `<br>` ohne laufenden
                // Text ist eine leere Zeile so hoch wie die Zeilenhoehe (wie in
                // jedem Browser: `<div><br></div>` ist eine Zeile hoch).
                // GEMESSEN vorher (Hinweis Agent A, css-flexbox `flex-abspos-
                // staticpos-margin-001`): `<br>` zwischen zwei Float-Reihen
                // erzeugte nichts, der Block blieb 0 px hoch.
                self.text_ohne_pre("\u{a0}");
                self.harter_umbruch();
            } else {
                // Release 11.10.2026 (Agent CSS2): im Inline-Fluss ohne
                // laufenden Text (nur Bilder) beendet ein `<br>` die Zeile —
                // ein leerer Blockkasten tut dasselbe im Fluss (CSS 2.1
                // §9.2.1.1). GEMESSEN vorher (WPT-Referenz
                // `inline-replaced-width-001-ref`, `<img/><br/><img/>`): beide
                // Bilder standen in einer Zeile.
                if let Ok(id) = self.doc.push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY) {
                    let stil = Style {
                        deckkraft: 1.0,
                        radius: [0.0; 4],
                        schatten_anzahl: 0,
                        dekoration: 0,
                        size: Dimension { width: Length::Auto, height: Length::px(0.0) },
                        ..Style::default()
                    };
                    let _ = self.doc.set_style(id, stil);
                }
            }
            // Release 12.10.2026 (Agent CSS2): mit `clear` folgt ein leerer,
            // raeumender Blockkasten — was nach dem Umbruch kommt, steht unter
            // den Floats.
            if raeumen != narsil_layout::plan::RaeumungArt::Keins {
                if self.laufender.is_some() {
                    // Der Folgeabsatz (noch leer) beginnt erst nach dem Kasten.
                    self.absatz_schliessen();
                }
                if let Ok(id) = self.doc.push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY) {
                    let stil = Style {
                        deckkraft: 1.0,
                        radius: [0.0; 4],
                        schatten_anzahl: 0,
                        dekoration: 0,
                        size: Dimension { width: Length::Auto, height: Length::px(0.0) },
                        ..Style::default()
                    };
                    let _ = self.doc.set_style(id, stil);
                    self.layout_plan.set(id, narsil_layout::LayoutStil { raeumen, ..Default::default() });
                }
            }
            return;
        }
        // Release 08.10.2026: mit Kaskade ist `<hr>` ein gewoehnlicher leerer
        // Kasten (Rahmen und Abstand aus dem UA-Blatt bzw. der Seite); der
        // Strichtext bleibt nur der Weg ohne CSS.
        if klein == "hr" && self.kaskade.is_none() {
            self.absatz_schliessen();
            self.platzhalter("────────────────");
            return;
        }
        // Release 08.10.2026: `display: contents` (CSS Display 3 §2.8): das
        // Element erzeugt keine Box — seine Kinder werden direkt in den
        // aktuellen Kasten gebaut (sie erben trotzdem seine Kaskade).
        // GEMESSEN vorher: `.layout__content{display:contents}` auf
        // developer.mozilla.org wurde ein eigenes Rasterkind in einer
        // 32 px schmalen Spur, und der ganze Artikel stand Wort fuer Wort
        // untereinander.
        if self.plan_stapel.last().is_some_and(|p| p.nur_inhalt)
            && !matches!(klein.as_str(), "html" | "body")
        {
            // Kein Planeintrag: ohne eigenen Knoten haengte der Abschluss
            // von `kind()` den Plan an den ERSTEN Knoten danach — und
            // ueberschrieb damit den Plan (samt `grid-area`) des ersten
            // Kindes.
            if let Some(p) = self.plan_stapel.last_mut() {
                p.stil = None;
                p.hintergrundbild = None;
                p.maskenbild = None;
            }
            for k in kinder {
                self.kind(k);
            }
            return;
        }
        // Absatz: laufenden Absatz zuerst schliessen.
        if ist_absatz(&klein) {
            self.absatz_schliessen();
        }

        // Ein Link ist ein Container, damit der Absatz darin Platz hat.
        let ist_link = klein == "a";
        let href = attributes.get("href").cloned().filter(|h| !h.is_empty());

        // I2: der von der Kaskade berechnete Stil gewinnt. `take()`
        // leert das Feld: ohne das wuerde der Stil des Elementes auch das
        // Kind mit falscher Typografie bauen.
        let mut stil = self
            .kommender_stil
            .take()
            .unwrap_or_else(|| self.stil_fuer(&klein, attributes, ist_link));
        //
        // 03.10. 18:55 (H-HN1, echte Seite): `bgcolor` VOR dem Zweig
        // „Absatz oder Kasten".
        //
        // GEMESSENER BEFUND, der diesen Platz verlangt (18:45, kleinster Fall
        // `<tr><td bgcolor="#ff6600">…` und `<td bgcolor="#00ff00">`):
        //     MESSUNG orange im stil: 0, gruen im stil: 0
        //     MESSUNG im bild: orange=0 gruen=0
        // und auf der ECHTEN Seite von news.ycombinator.com dieselbe Zahl:
        //     MESSUNG knoten mit orange im stil: 0
        //     MESSUNG rechtecke mit orange im bild: 0
        //
        // GEMESSENE URSACHE, und sie ist eine Tabelle: `td`/`th` stehen in
        // `ist_absatz` (`lib.rs:5493`). Der Absatz-Zweig kehrt zurueck,
        // BEVOR der Kasten-Code (und damit mein `bgcolor`) erreicht wird —
        // eine Zelle ist bei uns kein Kasten, sondern ein Absatz. Der
        // Hinweis gehoert deshalb an den Stil, und der Stil ist die eine
        // Stelle, die beide Wege benutzen.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Bedingung dieselbe bleibt
        // (`background.a == 0`): sie wird nur frueher geprueft. Wer eine
        // Hintergrundfarbe gesagt hat — CSS oder `style` — gewinnt weiterhin
        // (CSS 2.1, 6.4.4: praesentative Attribute zuerst).
        if stil.background.a == 0
            && matches!(klein.as_str(), "body" | "table" | "tr" | "th" | "td")
            && let Some(w) = attributes.get("bgcolor")
            && let Some(f) = parse_farbe(w)
        {
            stil.background = f;
        }

        // Bei einem Absatz/Link starten wir keinen neuen Container: der
        // Absatz IST der Knoten. Sonst haetten wir fuer `<p>` zwei
        // verschachtelte Container und damit doppelten Abstand.
        //
        // REIHENFOLGE, und sie war einmal falsch (29.09.2026, gefunden von
        // `html_wird_dokument_mit_typografie`): hier stand zuerst die
        // Schleife ueber die Kinder und DANACH `absatz_starten`. Der
        // Absatz bekam damit den Stil, der VOR dem Element galt — also
        // `Style::default()`, 16 px. `<h1>` war damit genauso gross wie
        // `<p>`, und die Seite sah aus wie eine Fehlermeldung.
        //
        // GEMESSENE Konsequenz: "h1 ist 16 px, p ist 16 px". Die
        // Ueberschriftenlogik in `stil_fuer` war vollstaendig richtig und
        // wurde nie benutzt. Ein Fehler, der aussieht wie "die Funktion
        // geht nicht", ist oft "die Funktion wird zur falschen Zeit
        // aufgerufen".
        //
        // SCHRIFT (04.10.2026): der Stil kommt auf den Stapel, BEVOR die
        // Kinder gebaut werden — der Text eines Kastens erbt ihn.
        // GEMESSEN, warum hier und nicht erst beim Absatz: zwischen hier
        // und `absatz_starten` liegen die Kindknoten, und der Absatz wird
        // von `text()` **der Kindknoten** gestartet.
        // Release 08.10.2026: ein Absatz-Element (`li`, `p`, `h1` ...) wird
        // ein KASTEN, sobald es Layout- oder Box-Eigenschaften traegt: eine
        // eigene `display`-Art, `position`, `float`, Hintergrund, Rahmen,
        // Fuellung oder eine feste Groesse. GEMESSEN vorher: ein solches `<li>`
        // ohne eigenen Text hatte keinen Knoten — sein Plan landete auf dem
        // ersten Kind (python.org: die Menue-`<li>` mit `display:
        // table-cell`) oder fiel weg, und Hintergrund und Rahmen fehlten.
        // Release 08.10.2026: ... und IMMER, wenn es ein Flex- oder
        // Grid-ELEMENT ist (der Eltern verteilt Kaesten, keine Zeilen).
        // GEMESSEN vorher: bbc.com, `<p>` in `display:grid` — seine Zeilen
        // wurden selbst Grid-Elemente, der Nachumbruch liess sie (zu Recht)
        // aus, und die 707-px-Zeile ragte aus der 353-px-Spalte.
        let in_flex_oder_raster = self.plan_stapel.iter().rev().nth(1).and_then(|p| p.anzeige).is_some_and(|a| {
            matches!(a, narsil_css::Anzeige::Flex | narsil_css::Anzeige::InlineFlex | narsil_css::Anzeige::Raster)
        });
        // Release 08.10.2026: ... und in einem INLINE-FLUSS (dort waeren
        // seine Woerter sonst Teil der laufenden Zeile).
        let im_inline_fluss = self.inline_bild_eltern.contains(&self.mutter());
        let absatz_als_kasten = ist_absatz(&klein) && !ist_link && (in_flex_oder_raster || im_inline_fluss || {
            let eintrag = self.plan_stapel.last();
            let anzeige = eintrag.and_then(|p| p.anzeige);
            let plan = eintrag.and_then(|p| p.stil.as_ref());
            // `inline` und `inline-block` bleiben Absaetze: als Kasten
            // stuenden Wikipedias `hlist`-Eintraege (346 `li` auf der
            // Portalseite) mit der Bitmap-Messung uebereinander
            // (`tests/plan_ueberlappung.rs`, 126 -> 141 Paare).
            if matches!(anzeige, Some(narsil_css::Anzeige::Inline) | Some(narsil_css::Anzeige::InlineBlock)) {
                false
            } else {
            !matches!(
                anzeige,
                None | Some(narsil_css::Anzeige::Block)
                    | Some(narsil_css::Anzeige::ListenEintrag)
                    | Some(narsil_css::Anzeige::Keins)
            ) || plan.is_some_and(|p| {
                p.position != narsil_layout::plan::PositionArt::Statisch
                    || p.schweben != narsil_layout::plan::FloatArt::Keins
            }) || stil.background.a != 0
                // Release 11.10.2026: auch ein Verlauf oder ein Schatten ist
                // eine Flaeche des Elements. GEMESSEN vorher: `<h1 style=
                // "background: linear-gradient(…)">` zeichnete keinen
                // Verlauf (der Absatz hatte keinen eigenen Kasten).
                || stil.verlauf.is_some()
                || stil.schatten_anzahl > 0
                || stil.rahmen.breite.iter().any(|b| *b > 0.0)
                || stil.padding != Spacing::ZERO
                || stil.size != Dimension::AUTO
            }
        });
        if ist_absatz(&klein) && !ist_link && !absatz_als_kasten {
            // Release 13.10.2026 (Agent CSS2): der wartende `::before`-Text
            // eines Vorfahren steht VOR diesem Absatz (ein anonymer Block, CSS
            // 2.1 §9.2.1.1) — wie beim echten Kasten weiter unten. GEMESSEN
            // vorher (WPT `CSS2/generated-content/content-113`, `-122`):
            // `body::before` stand in der ersten Zeile des `<p>`.
            if self.pseudo_text_wartend.iter().any(|w| w.tiefe < self.tiefe) {
                let t = self.tiefe;
                self.pseudo_text_setzen_unter(t);
            }
            fn hat_bild(nodes: &[HtmlNode], depth: usize) -> bool {
                depth < MAX_BAU_TIEFE && nodes.iter().any(|n| match n {
                    // Release 07.10.2026: alle ersetzten Elemente, auch `audio` und `svg`. Ohne
                    // eigenen Kasten markierte das umgebaute SVG den ELTERN
                    // als Bildabsatz, und alle Absätze darin liefen zu einer
                    // Zeile zusammen.
                    HtmlNode::Element {tag, children, ..} => ist_ersetzt(tag) || hat_bild(children, depth + 1),
                    _ => false,
                })
            }
            let bild_absatz = hat_bild(kinder, 0);
            if bild_absatz {
                let Ok(id) = self.doc.push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY) else { return; };
                let _ = self.doc.set_style(id, stil.clone());
                self.dom_zu_ord.insert(id.index() as u32, eigene_ord);
                if let Some(name) = self.aktuelle_id.take() { self.vermerke_id(name, id); }
                if let Some(slot) = self.kasten_stapel.last_mut() { *slot = Some(id); }
                self.inline_bild_eltern.insert(id);
                self.offen.push(id);
                stil.margin = Spacing::ZERO;
                stil.padding = Spacing::ZERO;
                stil.background = Color::TRANSPARENT;
                stil.rahmen = Default::default();
                self.bild_absatz_stile.push((self.kasten_textstile.len(), self.absatz_rahmen.len(), erbbarer_textstil(&stil)));
            }
            if !bild_absatz {
                let eltern = self.mutter();
                self.absatz_rahmen.push(AbsatzRahmen {
                    eltern,
                    kinder_anfang: self.doc.get(eltern).map_or(0, |n| n.children.len()),
                    stil: stil.clone(),
                    ord: eigene_ord,
                    platz: self.kasten_stapel.len().saturating_sub(1),
                    kasten: None,
                    kasten_tiefe: self.kasten_textstile.len(),
                });
            }
            self.absatz_starten(stil, None);
            // DER LISTENPUNKT. GEMESSENER FEHLER 30.09.2026
            // (Koordinationsbefund): `stil_fuer` hatte die Absicht
            // dokumentiert ("Punkt ist Text, keine Flaeche") und der
            // Punkt wurde nie geschrieben — `<li>Erster Punkt</li>` sah
            // aus wie ein Absatz. Er wird jetzt als ERSTES Zeichen des
            // Absatzes eingetragen, nicht als Knoten: ein eigener Knoten
            // waere eine eigene Zeile und damit eine eigene Zeile Luft
            // zwischen Punkt und Text.
            //
            // Release 07.10.2026: die Marke folgt `list-style-type` und dem
            // Zähler der Liste (vorher immer „* ", auch in `<ol>`).
            if klein == "li" {
                self.listenmarke_schreiben(attributes);
            }
            // Release 13.10.2026 (Agent CSS2): auch ein Absatz-Element kann
            // eine Zeilenklemme tragen (siehe `Klemme`).
            let klemme_hier = self.kaskade.as_ref().and_then(|k| k.werte.last()).and_then(klemme_von);
            if let Some((n, mit_auslassung)) = klemme_hier {
                self.klemmen.push(Klemme { rest: n, letzter_text: None, prepend: false, ellipse: !mit_auslassung, mit_auslassung });
            }
            for k in kinder {
                self.kind(k);
            }
            // Close while this element's cascade and parent are still active.
            self.absatz_schliessen();
            if klemme_hier.is_some() {
                self.klemmen.pop();
            }
            if bild_absatz {
                self.offen.pop();
                self.bild_absatz_stile.pop();
            }
            if !bild_absatz && self.absatz_rahmen.pop().is_some_and(|r| r.kasten.is_some()) {
                self.offen.pop();
            }
            return;
        }

        // Sonst: echter Container.
        //
        // Release 11.10.2026: der wartende `::before`-Text eines Vorfahren
        // steht VOR diesem Kasten (CSS 2.1 §9.2.1.1: ein anonymer Block),
        // nicht im ersten Text darin. GEMESSEN vorher (WPT `content-021`):
        // „0“ und „0.0“ standen zusammen im inneren Kasten.
        if self.pseudo_text_wartend.iter().any(|w| w.tiefe < self.tiefe) {
            let t = self.tiefe;
            self.pseudo_text_setzen_unter(t);
        }
        //
        // DER LAUFENDE ABSATZ GEHOERT VOR DIE BOX. GEMESSENER FEHLER
        // 30.09.2026: das stand hier nicht, und der Befund daraus war
        // `<blockquote>Ein Zitat.</blockquote><pre>fn main() …</pre>` als
        // EIN Textknoten "Ein Zitat.fn main() { … }" — und ein leerer
        // Container daneben. Der Absatz war noch offen, als die Box
        // entstand, und wurde erst NACH ihr geschlossen: sein Text
        // landete beim FALSCHEN Elternknoten und zusammen mit dem Text
        // der Box.
        //
        //
        // W6: DIE NUMMER DIESES ELEMENTS merken, solange die Kinder noch
        // nicht gebaut sind (siehe `absatz_starten`).
        //
        // GEMESSEN, warum das hier und nicht in `absatz_starten`
        // (16:45): `absatz_starten` wird aus `text()` gerufen, wo
        // `aktualer_ordinal` die Nummer des TEXTKNOTENS ist. Bei
        // `<div id="k">+</div><div id="z">0</div>` bekam die Zeilenbox von
        // `z` die Nummer des Textes von `k` — und ein Klick auf `z` fand
        // keinen Handler, weil die Nummer falsch war.
        self.laufender_ord = Some(self.aktualer_ordinal);
        //
        // Das ist dieselbe Fehlerklasse wie der Inline-Befund (fuenf
        // Zeilen statt einer) und dieselbe wie der Listenpunkt-Befund
        // (dritter Punkt mit anderem Abstand): ein Absatz, der zum
        // falschen Zeitpunkt endet oder nicht endet. Die Regel ist
        // jetzt an EINER Stelle: **eine Box schliesst den Absatz**.
        self.absatz_schliessen();
        // 04.10.2026 (SCHRIFT): `let mut box_stil = stil.clone();` — `Style`
        // traegt jetzt `schriftfamilie: String` und ist damit nicht mehr
        // `Copy` (narsil-core). `stil` wird weiter unten noch gebraucht
        // (Zellen und Rahmen-Zweig), also wandert es **nicht**.
        let mut box_stil = stil.clone();
        //
        // 03.10. 21:20: `cellpadding` IST DIE POLSTERUNG DER ZELLE.
        //
        // GEMESSENE BEGRUENDUNG, warum es neben `bgcolor` und nicht im
        // `<table>`-Zweig steht: die Zahl steht am `<table>`, aber die
        // Polsterung gehoert der ZELLE, und die Zelle ist ein Kasten seit
        // 19:10 (`ist_block`). Sie wird hier wie der Rahmen und wie die
        // Typografie gesetzt — an dem Kasten, der sie traegt.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Zahl VOR dem Rahmen kommt: der
        // Rahmen code-addiert Polsterung (I2, Zeile 4193 ff.), und
        // `cellpadding="0"` bei Firefox bedeutet „kein Rahmenabstand". Eine
        // Polsterung, die der Rahmen wieder wegnimmt, waere eine Angabe ohne
        // Wirkung — der haeufigste Wert auf echten Seiten ist genau 0.
        //
        // GEMESSENE ENTSCHEIDUNG, warum CSS gewinnt: wie bei `bgcolor` und
        // `height` gilt CSS 2.1, 6.4.4 — ein praesentatives Attribut hat die
        // niedrigste Prioritaet. `padding` aus dem Stylesheet steht in
        // `box_stil.padding` und wird deshalb nicht ueberschrieben.
        if matches!(klein.as_str(), "td" | "th")
            && box_stil.padding == Spacing::ZERO
            && let Some(cp) = self.cellpadding_stapel.last().copied()
            && cp > 0
        {
            box_stil.padding = Spacing::all(Length::px(cp as f32));
        }
        //
        // 05.10. 22:15: `cellspacing` IST DER ABSTAND ZWISCHEN DEN ZELLEN.
        //
        // GEMESSENER BEFUND (22:05, Wegwerf-Messung, entfernt): der Abstand
        // zwischen zwei Zellen ist 19,2 px — mit `cellspacing="0"`, mit
        // `cellspacing="4"` und ganz ohne die Angabe. `cellpadding="4"` dafuer
        // 27,2 px (die Polsterung wirkt also, der Abstand nicht).
        //
        // GEMESSENE ENTSCHEIDUNG, warum der RAND und nicht die Polsterung:
        // die Polsterung einer Zelle liegt auf BEIDEN Seiten, der Abstand
        // zweier Zellen also zweimal so gross (GEMESSEN bei `cellpadding`:
        // 19,2 -> 27,2 = 19,2 + 2 x 4). Mit dem Rand **oberhalb** der Zelle
        // liegt zwischen zwei Zellen genau eine Angabe: der Fuss der ersten
        // (0) trifft auf den Kopf der zweiten (N). Das ist der Wert, den
        // `cellspacing="4"` verlangt.
        //
        // GEMESSENE EINORDNUNG, was „zwischen den Zellen" bei uns heisst:
        // unsere Tabellen sind linearisiert (eine Spalte, GEMESSEN: zwei
        // Zellen derselben Zeile stehen genauso untereinander wie zwei
        // Zeilen). Es gibt damit EINEN Zwischenraum je Zelle und keine
        // waagerechte Nachbarschaft — `cellspacing` wirkt bei uns nur
        // senkrecht. Das ist die halbe Angabe, und die andere Haelfte
        // (links/rechts) gehoert in eine Gitter-Lage, die es nicht gibt.
        //
        // GEMESSENE EINORDNUNG der einen Kante, die anders ist als bei
        // Firefox: dort steht `cellspacing`-Raum auch **unter** der letzten
        // Zeile. Bei uns steht der Raum ueber der ersten Zeile (und damit
        // auch am oberen Rand der Tabelle wie bei Firefox), aber nicht unter
        // der letzten. Grund: wann die letzte Zelle kommt, weiss der Kasten
        // der Zelle nicht — er entsteht, bevor die naechste Zelle gelesen
        // ist. GEMELDET als Randfall, nicht kaschiert.
        //
        // GEMESSENE ENTSCHEIDUNG, warum CSS gewinnt: wie bei `bgcolor`,
        // `height` und `cellpadding` gilt CSS 2.1, 6.4.4 — ein
        // praesentatives Attribut hat die niedrigste Prioritaet. Ein
        // `margin-top` aus dem Stylesheet steht in `box_stil.margin` und
        // wird deshalb nicht ueberschrieben.
        if matches!(klein.as_str(), "td" | "th")
            && box_stil.margin == Spacing::ZERO
            && let Some(cs) = self.cellspacing_stapel.last().copied()
            && cs > 0
        {
            let mut m = box_stil.margin;
            m.top = Length::px(cs as f32);
            box_stil.margin = m;
        }
        //
        // 03.10. 16:15 (H-HN2, zweiter Teil): `height`/`width` SIND
        // PRAESENTATIVE ATTRIBUTE (HTML 4, 17.12.3; CSS 2.1, 6.4.4 nennt
        // dieselbe Uebertragung ausdruecklich fuer `width` und `height`).
        //
        // GEMESSENER BEFUND (14:50, Wegwerf-Test): news.ycombinator.com legt
        // zwischen zwei Eintraegen `<tr><td class="spacer" height="10"></td>
        // </tr>`, und GEMESSEN war der Kasten **704 x 0 px** — die
        // Leerzeile hat keine Hoehe. HN hat sie mit 10 px Absicht gesetzt
        // (die Zeilen sollen Luft bekommen, nicht 0 px).
        //
        // GEMESSENE ENTSCHEIDUNG, warum NUR `table` und `tr`: eine Zelle
        // bekommt bei der Linearisierung keinen eigenen Kasten (gemessen
        // 15:25, `team/PROBLEME.txt` 16:25) — ein `height` an `<td>` haette
        // keinen Platz, an dem es ankam. An `table` und `tr` gibt es den
        // Kasten, also dort die Angabe.
        //
        // GEMESSENE ENTSCHEIDUNG, warum wie bei `bgcolor` NUR wenn sonst
        // nichts gesagt wurde: praesentative Attribute haben nach CSS 2.1,
        // 6.4.4 die niedrigste Prioritaet. `Auto` heisst: der Autor hat keine
        // Hoehe gesagt.
        //
        // M4: `zahl_oder_null` (hteile.rs) begrenzt die Zahl VOR der
        // Uebernahme — ein `height="99999999999"` liefert 0 und damit
        // `Auto`.
        // Tabellenbreite aus Attributen (`width="85%"` oder `width="100"`).
        if box_stil.size.width == narsil_core::length::Length::Auto
            && matches!(klein.as_str(), "table" | "tr" | "td" | "th")
            && let Some(n) = attributes.get("width")
        {
            let n = n.trim();
            if let Some(pct) = n.strip_suffix('%') {
                if let Ok(p) = pct.trim().parse::<f32>() {
                    if p >= 0.0 {
                        box_stil.size.width = narsil_core::length::Length::Percent(p);
                    }
                }
            } else if let Some(h) = parse_px(n) {
                if h > 0.0 {
                    box_stil.size.width = narsil_core::length::Length::px(h);
                }
            }
        }

        if box_stil.size.height == narsil_core::length::Length::Auto
            //
            // 03.10. 19:45: `td`/`th` kommen dazu. GEMESSENE BEGRUENDUNG:
            // seit 19:10 ist die Zelle ein KASTEN (`ist_block`), also hat
            // eine Hoehe dort etwas, wo sie hingehört — vorher nicht, und
            // deshalb standen sie hier nicht. HTML 4, 17.12.3 erlaubt
            // `height` bei genau diesen fuenf Tags.
            && matches!(klein.as_str(), "table" | "tr" | "td" | "th")
            && let Some(n) = attributes.get("height")
            && let Some(h) = parse_px(n)
            && h > 0.0
        {
            box_stil.size.height = narsil_core::length::Length::px(h);
        }
        //
        // GEMESSENE ENTSCHEIDUNG (16:55), warum der Rumpf hier NICHT mit dem
        // Absatzabstand versehen wird: `ABSATZABSTAND` ist unser Ersatz fuer
        // die UA-Raender der INHALTSelemente (`div`, `p`, `ul`, …). `<body>`
        // hat nach CSS 2.1, 8.3.2 keinen — ein Browser setzt dort nichts, und
        // die Seite rueckte beim Bauen um 16 px ein, wenn wir es täten.
        //
        // 03.10. 18:20: `td`/`th` sind mit in der Liste und bekommen
        // trotzdem KEINEN Rand.
        //
        // GEMESSENE BEGRUENDUNG: `ABSATZABSTAND` (12 px) ist unser Ersatz
        // fuer die UA-Raender der INHALTSelemente. Eine Zelle ist ein
        // Kasten fuer INHALT; im UA-Stylesheet (CSS 2.1, Anhang D) steht
        // fuer `td` ein `padding`, kein `margin`. GEMESSEN, was der Rand
        // gekostet haette: 12 px links und rechts und 24 px in der Hoehe je
        // Zelle — bei `width="100%"`-Tabellen (HN, wikipedia) einen Umbruch,
        // den es vorher nicht gab.
        //
        // 03.10. 19:00: AUCH `tr` NICHT. GEMESSENE URSACHE der zu hohen
        // Zeilen auf news.ycombinator.com (18:50, echte Seite, Bild
        // `team/log/abnahme_1840_hn`): gemessen 144 px je Eintrag (Titel 337,
        // Subtext 379, naechster Titel 481). Ein `<tr>` bekam 12 px Rand oben
        // und unten, und ein Eintrag hat zwei Zeilen — 48 px davon waren
        // reiner Rand, den kein Browser setzt: im UA-Stylesheet (CSS 2.1,
        // Anhang D) hat `tr` KEIN `margin`.
        //
        // 03.10. 19:35: AUCH `table` NICHT. GEMESSENE BEGRUENDUNG: der Test
        // `hn_zeilen_und_zellen_haben_keinen_rand` fand einen Kasten
        // 928x2395 mit 12 px Rand — das ist `<table id="hnmain">` selbst. Ein
        // Browser gibt einer Tabelle keinen Rand (CSS 2.1, Anhang D: kein
        // `margin` fuer `table`), wir gaben ihr 12 px, also rueckte die
        // ganze Seite ein.
        //
        // ---- 03.10. 14:2x (Rolle CSS, UEBERNIMME CODEX-8): DER
        // PAUSCHALE `ABSATZABSTAND` IST ENTFALLEN. ----
        //
        // GEMESSENER BEFUND (der Auftrag nannte die Stelle): drei
        // Regressionstests in `crates/narsil-layout-engine/tests/
        // codex8_flex_masse.rs` waren mit `*{margin:0;padding:0}` ROT und
        // lieferten **64 statt 40**, **132 statt 40** und **48 statt 40**
        // — genau 12 px Rand je Seite bzw. 24 px in der Hoehe, also genau
        // dieser Block.  Ein Browser setzt keinen Rand, wenn das
        // Stylesheet `margin: 0` sagt.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Ersatz ueberfluessig ist:
        // `crates/narsil-css/src/ua.rs` traegt seit C5 die echten
        // UA-Raender (Zeilen 16–28: `body` 8 px, `p` 16 px 0,
        // `ul/ol` 16 px 0 + 40 px padding, `h1`–`h6`, `blockquote` 16/40,
        // `pre` 16 px 0).  `kaskade::stil_von_css` rechnet die ueber
        // `mit_ua` und schreibt sie nach `stil.margin`
        // (`crates/narsil-page/src/kaskade.rs:1023`), also ist der Rand
        // aus der Kaskade bereits da — und der pauschale Block hat ihn
        // nur ueberschrieben.
        //
        // GEMESSENE REGEL, die daraus folgt: **ein Ersatz fuer eine
        // CSS-Eigenschaft, die es inzwischen an der richtigen Stelle
        // gibt, ist kein Fallback, sondern eine zweite Wahrheit.**  Wer
        // eine Eigenschaft an zwei Orten berechnet, sieht an einem
        // nicht, welcher der beiden falsch ist — genau das ist der
        // Fehler, den die drei Tests gefunden haben.
        //
        // Was `ABSATZABSTAND` weiterhin bedeutet, steht an zwei
        // anderen Stellen: der Absatzabstand am Absatz (Zeile ~7720) und
        // der Listenpunkt (~7760).  Die bleiben unberuehrt.
        let _ = ABSATZABSTAND;
        // I2, RAHMEN: die Box bekommt die Rahmenfarbe als Hintergrund
        // und die Rahmenstaerke als `padding`. Der Inhalt wandert in
        // einen Kindknoten mit der Seitenfarbe, und was von der Box
        // sichtbar bleibt, ist genau der Rahmen.
        //
        // GEMESSENER FEHLER 30.09.2026 (Bildschirmfoto `test2.html`):
        // `.box { border: 2px solid var(--akzent) }` wurde nicht
        // gezeichnet — `Style` (narsil-core, nicht meine Crate) hat keine
        // Rahmenfelder.
        //
        // DIE ABWEICHUNG von "vier Rechtecke in der DisplayList" ist
        // eine Entscheidung: vier einzelne Rechtecke brauchten eine
        // absolute Positionierung, und die Layout-Engine stellt Kinder in
        // einen FLUSS. Ein Fluss kann einen Rahmen als "Rand um den
        // Inhalt" ausdruecken, nicht als vier Streifen. Das Bild ist
        // identisch, und es braucht zwei Knoten statt fuenf.
        //
        // DIE FUELLUNG BLEIBT STEHEN, bis sie der INHALT bekommt.
        //
        // GEMESSENER FEHLER 30.09.2026, 06:40: `padding: 8px` wurde
        // hier ueberschrieben, der Text beruehrte den Rahmen.
        // Korrektur um 06:45: Rahmen UND Fuellung in `box_stil.padding`
        // addiert — und damit war der Rahmen 10 px dick statt 2 px
        // (gemessen 30.09.2026, 08:50, Koordinationsbefund: "ca. 10 px
        // dicker Rahmen"). Grund: `box_stil.background` ist die
        // RAHMENFARBE und fuellt den GANZEN Border-Box — auch die
        // Fuellflaeche. 2 px Rahmen + 8 px Fuellung = 10 px Rahmen.
        //
        // Die Reihenfolge ist CSS 2.1, 8.3: der Rahmen liegt
        // AUSSERHALB, die Fuellung INNERHALB. Fuer diese Engine heisst
        // das: die BOX traegt nur den Rahmen als Padding, und der
        // INHALTSKNOTEN (unten) traegt die Fuellung UND verdeckt sie
        // mit der Seitenfarbe. Was sichtbar bleibt, ist der Rahmen in
        // seiner eigenen Dicke.
        //
        // Die Fuellung wird hier gesichert und unten weitergegeben.
        let css_fuellung = box_stil.padding;
        let knoten_id = self
            .doc
            .push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY);
        let knoten_id = match knoten_id {
            Ok(id) => {
                // W7C R3: der Kasten, in den der Layout-Plan gehoert.
                self.eigener_kasten = Some(id);
                // 03.10. 22:50: und derselbe Knoten auf den Kasten-Stapel.
                // GEMESSENE BEGRUENDUNG, warum es an BEIDEN Stellen steht
                // und nicht an einer: das sind zwei Wege, einen Kasten zu
                // erzeugen (ein Bild und ein Block), und der Plan muss an
                // beiden ankommen — die Bilder sind die einzigen Elemente, bei
                // denen es bis 22:50 ueberhaupt funktioniert hat
                // (GEMESSEN: `plan_knoten=[1,2,3]` auf der max-width-Seite).
                if let Some(platz) = self.kasten_stapel.last_mut() {
                    *platz = Some(id);
                }
                // PSEUDOELEMENTE (06.10.2026): DER `::before`-KASTEN GEHOERT
                // HIERHER — als ERSTES KIND des eigenen Kastens, bevor die
                // Kinder gebaut werden. GEMESSEN an de.wikipedia.org: das
                // `<a class="mw-logo">` ist `display:flex` und bekommt
                // diesen Kasten; die Kugel (`.mw-logo::before`) stand
                // vorher am ELTERN und damit ausserhalb (NodeId(70) bei
                // x=44, der Kasten des `<a>` bei x=90).
                let pseudo = self.pseudo_before_stapel.last_mut().and_then(|s| s.take());
                if let Some(p) = pseudo {
                    self.pseudo_anhaengen(p, Some(id));
                }
                // 02.10. 05:05: EIN BLOCK IN EINEM `<a>` IST EIN LINK —
                // `<a href=…><div class='votearrow'></div></a>` (die 30
                // Stimmpfeile auf news.ycombinator.com). GEMESSEN vorher:
                // `links` war **0**. Siehe `vermerke_link_in_marke`.
                self.vermerke_link_in_marke(id);
                // 02.10. 08:20: dieselbe Regel fuer den Blockkasten — und
                // `take()` aus demselben Grund (siehe dort).
                if let Some(n) = self.aktuelle_id.take() {
                    self.vermerke_id(n, id);
                }
                id
            }
            // Die Arena ist voll: 4 Milliarden Knoten. Kein Panic,
            // sondern die Seite wird ab hier abgeschnitten.
            Err(_) => return,
        };
        // 04.10.2026 (SCHRIFT): `.clone()` — `Style` ist nicht mehr `Copy`
        // (Feld `schriftfamilie`), und `box_stil` wird unterhalb noch
        // einmal gebraucht (Rahmen-Zweig, Zellen).
        let _ = self.doc.set_style(knoten_id, box_stil.clone());
        //
        // 03.10. 17:25: DER RUMPF FAERBT DIE SEITE (CSS 2.1, 2.3.2).
        //
        // GEMESSENER BEFUND, warum das noetig ist: der Rumpf hat jetzt
        // seinen Kasten und seine Farbe (`ist_block` enthaelt `body`,
        // 16:55) — aber der Kasten ist nur so hoch wie sein INHALT
        // (gemessen: 752 x 51 px bei einem Absatz). Unterhalb davon war die
        // weisse Seite der Wurzel zu sehen, und die Seite sah trotz
        // `background-color:#222` weiss aus. In einem Browser ist das
        // anders: der Hintergrund des Rumpfes wird auf die CANVAS
        // uebertragen und faerbt die ganze Seite.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die UEBERTRAGUNG hier und nicht im
        // Layout: die Canvas ist die Wurzel selbst (der Dokument-Root
        // traegt `FARBE_SEITE` und den Seitenrand), und der Bauer ist der
        // einzige, der weiss, dass ein Kasten zum Rumpf gehoert. Das Layout
        // sieht nur Rechtecke.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die vorhandene weisse Wurzel
        // ERSETZT und nicht ueberschrieben wird, sobald der Rumpf farbig
        // ist: `FARBE_SEITE` ist unser Ersatz fuer „der Browser malt weiss",
        // und ein Browser nimmt in diesem Fall ebenfalls den Rumpf
        // (CSS 2.1, 2.3.2: „the background of the root element is
        // propagated to the canvas; if the root has a transparent
        // background, the body's is used").
        //
        // 05.10. 23:55 — DER GEFUNDENE FEHLER: DIE BEIDEN `return` HIER HABEN
        // DIE SEITE GETOETET. GEMESSEN an www.rust-lang.org (Meldung der
        // Koordination 23:40: curl liefert 18 594 Byte, der Probe zeigt
        // 3 Text / 0 Links):
        //
        //     ohne die externen Blaetter   319 Knoten, 34 Links
        //     mit  app_17774143076088586802.css   3 Knoten,  0 Links
        //
        // GEMESSENE ISOLIERUNG: nur dieses eine Blatt (11 085 Byte) loescht
        // die Seite; `body > main { display: flex }` war es NICHT (GEMESSEN
        // mit und ohne). GEMESSENE ENTSCHEIDUNG, wo es passiert: dieses
        // `return` steht in `element_innen` und beendet damit die ganze
        // Funktion — die KINDER DES RUMPFES WERDEN NICHT BESUCHT.
        //
        // GEMESSENE EINORDNUNG, warum es erst bei diesem Blatt auftritt: der
        // zweite `return` feuert, wenn die Rumpf-Farbe GLEICH der Wurzel ist,
        // und `FARBE_SEITE` ist weiss. `body { background-color: white }`
        // (rust-lang, Regel 5 des Blattes) ist genau dieser Fall — und
        // weiss auf weiss ist die harmloseste Moeglichkeit, die es gibt.
        // GEMESSENE EINORDNUNG, warum es vorher niemand gesehen hat: ohne
        // `background-color` am Rumpf ist `box_stil.background.a == 0`, die
        // ganze `if` faellt weg, und die Seite ist ganz da — 11 von 12
        // gemessenen Seiten brauchen kein `background-color` am Rumpf.
        //
        // GEMESSENE EINORDNUNG, warum `background-color: white` ueberhaupt
        // harmlos sein MUSS: CSS 2.1, 2.3.2 — der Hintergrund des Wurzel-
        // Elements wird auf die CANVAS uebertragen; ist er durchsichtig, der
        // des Rumpfes.
        //
        // 05.10.2026 (Canvas): **DIE EINORDNUNG WAR FALSCH, UND DIE
        // UEBERTRAGUNG FEHLTE FUER `<html>`.** Sie stand hier: „Unsere Wurzel
        // ist schon weiss, also gibt es nichts zu tun." Richtig ist: es gibt
        // sehr wohl etwas zu tun, sobald **`<html>` selbst** eine Farbe
        // traegt — und die Wurzel ist nicht weiss, sondern **dann** die
        // Farbe des Wurzel-Elements.
        //
        // GEMESSENER BEFUND (05.10.2026, `team/testseiten/canvas_hintergrund.html`,
        // Fenster 900x650, `html { background:#d32f2f }`, Inhalt 60 px):
        //
        //     vorher:  y 141..625  #ffffff   <-- WEISS
        //     nachher: y 141..625  #d32f2f   <-- ROT
        //
        // GEMESSENE ENTSCHEIDUNG, warum BEIDE Zweige in EINEN `if` und nicht
        // in zwei: CSS 2.1, 2.3.2 nennt **eine** Regel mit zwei Kandidaten
        // (`<html>`, sonst `<body>`). Zwei Bloecke waeren zwei Faelle, in
        // denen jemand die Reihenfolge neu erfindet — und die Reihenfolge ist
        // hier genau das, was schon einmal falsch war.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `klein == "body"` **nicht** genuegt
        // und `canvas_von_html` noetig ist: `<html>` steht in der
        // Dokumentreihenfolge **vor** `<body>`, also gewinnt ohne die
        // Sperre der LETZTE. Gemessen an
        // `team/testseiten/canvas_body_zuerst.html` (`html` rot, `body`
        // blau): die Canvas wurde blau statt rot. Das Feld steht in `Bauer`.
        // Release 14.10.2026 (Agent CSS2): no propagation from a body with
        // `contain: paint` (on itself or on `<html>`) or without a box of its
        // own (`display: contents`). MEASURED before (WPT css-backgrounds
        // `background-color-body-propagation-007..009`): the canvas was
        // painted anyway.
        let (paint_contained, contents_only) = self.kaskade.as_ref().and_then(|k| k.werte.last()).map_or((false, false), |s| {
            let paint = s.schluessel.iter().any(|(k, v)| {
                k == "contain" && v.split_whitespace().any(|t| matches!(t, "paint" | "strict" | "content"))
            });
            (paint, s.nur_inhalt)
        });
        if klein == "html" && paint_contained {
            self.canvas_blocked = true;
        }
        if klein == "html" {
            self.root_has_image = self
                .kaskade
                .as_ref()
                .and_then(|k| k.werte.last())
                .is_some_and(|s| !matches!(s.bild, narsil_css::BildQuelle::Keins));
        }
        let canvas_kandidat = match klein.as_str() {
            "html" if box_stil.background.a != 0 => {
                self.canvas_von_html = true;
                Some(box_stil.background)
            }
            // `<body>` nur, wenn `<html>` nichts entschieden hat.
            "body"
                if box_stil.background.a != 0
                    && !self.canvas_von_html
                    && !self.canvas_blocked
                    && !paint_contained
                    && !contents_only =>
            {
                Some(box_stil.background)
            }
            _ => None,
        };
        if let Some(farbe) = canvas_kandidat
            && let Some(mut neu) = self.doc.get(self.doc.root()).map(|n| n.style.clone())
            && neu.background != farbe
        {
            neu.background = farbe;
            let _ = self.doc.set_style(self.doc.root(), neu);
        }

        // DER LINK-ABSATZ. GEMESSENER FEHLER 29.09.2026
        // (`link_wird_klickbar_mit_aufgeloester_adresse`): der Link-Stil
        // (blau) stand am CONTAINER, der Text steht aber im ABSATZ. Ohne
        // Vererbung war der Text in der Standardfarbe — der Link war da,
        // korrekt aufgeloest, und trotzdem unsichtbar als Link. Ein Browser,
        // in dem man nicht sieht, was anklickbar ist, hat keine Links.
        //
        // Also: der Absatz startet hier, BEVOR die Kinder kommen, und
        // er erbt den Stil. Die Adresse haengt am Absatz, nicht am
        // Container — sonst waere der Container fuer jeden Klick in der
        // Zeile treffbar, auch fuer den Teil, der kein Link ist.
        if let Some(ziel) = href.as_ref()
            && ist_link
        {
            // 04.10.2026 (SCHRIFT): `.clone()` — `Style` ist nicht mehr
            // `Copy` (Feld `schriftfamilie`); `stil` wird danach noch
            // fuer den Kasten gebraucht.
            //
            // Release 08.10.2026: nur der TEXTstil — Innenabstand, Rand,
            // Hintergrund und Rahmen traegt der Kasten des Links. GEMESSEN
            // vorher: apple.com `a.button { padding: 11px 21px }` hatte den
            // Innenabstand doppelt (Kasten UND Textzeile), die Pille war 65
            // statt 43 px hoch und der Text stand 21 px neben der Mitte.
            self.absatz_starten(erbbarer_textstil(&stil), Some(loese_link(ziel, self.basis)));
        }

        // Der Einzug dieser BOX zaehlt ab jetzt fuer ihre Kinder.
        //
        // DER EINZUG, und er hat zwei Faelle.
        //
        // GEMESSENER FEHLER 30.09.2026, 09:05: hier standen
        // `box_stil.padding` UND `css_fuellung` addiert. Bei einer Box
        // OHNE Rahmen sind die beiden aber derselbe Wert (die
        // Fuellung wird nur im Rahmenfall verschoben) — also war die
        // Fuellung doppelt im Einzug. Gemessen an `<ul>`: die Listen
        // standen 40 px statt 20 px eingerueckt, und die dritte
        // Listenzeile rutschte um 38 px statt 19 px (der Test
        // `listenpunkte_hatten_gleiche_grundlinienabstaende` fiel).
        //
        // OHNE Rahmen: Masse + Fuellung.
        // MIT Rahmen: Masse + Rahmen + Fuellung (die Fuellung sitzt dann
        // am Inhaltsknoten, nicht an der Box).
        let (f_laenge, f_breite) = (spanne_links(&css_fuellung), spanne_rechts(&css_fuellung));
        let eigener_einzug =
            spanne_links(&box_stil.margin) + spanne_rechts(&box_stil.margin) + f_laenge + f_breite;
        let textbreite_vorher = self.textbreite;
        // Release 10.10.2026: ein `position: fixed`-Kasten misst gegen das
        // FENSTER, nicht gegen seinen Eltern (CSS 2.1 §10.1: der umgebende
        // Block ist der Anzeigebereich). GEMESSEN vorher (heise.de): die
        // heise+-Fussleiste steckt in `#wtma-sticky-footer { width: 1px }`
        // — ihr Text wurde mit 1 px Breite umbrochen, jedes Wort eine Zeile.
        if self
            .plan_stapel
            .last()
            .and_then(|p| p.stil.as_ref())
            .is_some_and(|s| s.position == narsil_layout::PositionArt::Fest)
        {
            self.textbreite = (self.breite as f32).max(1.0);
        }
        self.textbreite = self.inhaltsbreite(&box_stil);
        self.textbreite_von_ord.insert(self.aktualer_ordinal, self.textbreite);
        self.einzug += eigener_einzug;
        //
        // W6G: DEN EINZUG DIESES ELEMENTS MERKEN (30.09.2026, 18:55).
        //
        // GEMESSENER FEHLER: ein per `createElement` erzeugter Listenpunkt
        // stand bei x = 48, ein GEParster bei x = 100 (Bildschirm-
        // messung, `erzeugte_liste.rs`). Der Unterschied ist der
        // Listeneinzug — und der lebt in `Bauer::einzug`, einem Zustand,
        // der beim Bauen des Containers wieder abgebaut wird. Ein Knoten,
        // der NACHHER angehaengt wird (W6F), kam also mit `einzug = 0`
        // heraus: kein Listen-Einzug, kein `padding-left` des `<ul>`.
        //
        // GEMESSEN, warum je VORLAUF-NUMMER und nicht als ein Feld: der
        // Einzug ist ein Zustand des Bauers, und zwischen dem Container
        // und dem spaeter angehaengten Kind liegen beliebig viele andere
        // Elemente (der `<li>` des Parse zum Beispiel). Ein einfaches
        // `gemerkt` wuerde den Einzug des LETZTEN Elements liefern.
        self.einzug_von_ord
            .insert(self.aktualer_ordinal, self.einzug);
        // Release 08.10.2026: ein Kasten (inline-block, Knopf, Link-Kasten),
        // dessen Inhalt ein Bild oder Symbol neben Text traegt, schreibt
        // seinen Inhalt als Zeilenlauf — derselbe Weg wie der Bildabsatz
        // oben. GEMESSEN vorher: in `<button><span>Platform</span> <svg/>`
        // stand das Symbol UNTER dem Text. Flex- und Rasterkaesten nicht:
        // dort ist jedes Kind ein eigenes Element des Containers.
        let traegt_kinder_selbst = self.plan_stapel.last().and_then(|p| p.anzeige).is_some_and(|a| {
            matches!(
                a,
                narsil_css::Anzeige::Flex | narsil_css::Anzeige::InlineFlex | narsil_css::Anzeige::Raster
            )
        });
        if !traegt_kinder_selbst
            && kinder.iter().any(|k| matches!(k, HtmlNode::Text(t) if !t.chars().all(css_leerraum))
                || matches!(k, HtmlNode::Element { tag, .. } if ist_inline(&tag.to_ascii_lowercase())))
            && kinder.iter().any(|k| matches!(k, HtmlNode::Element { tag, .. } if ist_ersetzt(tag)))
        {
            self.inline_bild_eltern.insert(knoten_id);
        }
        self.offen.push(knoten_id);
        // Release 11.10.2026 (Agent CSS2): ein Block im Block — die erste
        // Zeile des aeusseren ist vergeben, die des inneren steht aus.
        if let Some(t) = self.einzug_frei.last_mut() {
            *t = false;
        }
        self.einzug_frei.push(true);
        // Release 08.10.2026: ein `<li>`, das ein Kasten ist (siehe
        // `absatz_als_kasten`), behaelt seine Listenmarke, solange es
        // `display: list-item` ist.
        if klein == "li"
            && self.plan_stapel.last().and_then(|p| p.anzeige) == Some(narsil_css::Anzeige::ListenEintrag)
        {
            self.absatz_starten(erbbarer_textstil(&stil), None);
            self.listenmarke_schreiben(attributes);
        }

        // I2, RAHMEN. GEMESSENER FEHLER 30.09.2026 (Bildschirmfoto
        // `test2.html`): `.box { border: 2px solid var(--akzent) }`
        // wurde nicht gezeichnet — `Style` (narsil-core, nicht meine
        // Crate) hat keine Rahmenfelder.
        //
        // WIE EIN RAHMEN HIER ENTSTEHT, und warum das ein Rechteck ist
        // und nicht vier: die Box bekommt die RAHMENFARBE als
        // Hintergrund und die Rahmenstaerke als `padding`. Der Inhalt
        // wandert in einen Kindknoten mit der SEITENFARBE. Was von der
        // Box sichtbar bleibt, ist genau der Rahmen — oben, unten,
        // links, rechts, in Akzentfarbe.
        //
        // DIE ABWEICHUNG von "vier Rechtecke in der DisplayList" ist
        // eine Entscheidung, keine Verlegenheit: vier einzelne
        // Rechtecke brauchten eine absolute Positionierung, und die
        // Layout-Engine hat keine (sie stellt Kinder in einen Fluss).
        // Ein Fluss kann einen Rahmen als "Rand um den Inhalt"
        // ausdruecken, nicht als vier Streifen. Das BILD ist identisch.
        let ist_zelle = matches!(klein.as_str(), "td" | "th");
        // Release 08.10.2026: die Zelle traegt ihren ganzen Stil wie bisher
        // (`offene_stile`), jeder andere Kasten nur die vererbbaren Angaben.
        // Release 11.10.2026 (Agent CSS2): auch die Zelle nur die vererbbaren
        // Angaben — sie ist ein echter Kasten mit eigenem Innenabstand.
        // GEMESSEN vorher (WPT `fixed-table-layout-003a05`): die Zeilenbox in
        // der Zelle bekam deren `padding` ein zweites Mal, der Text stand
        // 120 px zu weit rechts ausserhalb der Zelle.
        self.kasten_textstile.push(erbbarer_textstil(&stil));
        if ist_zelle {
            self.offene_stile.push(stil);
        }
        // Release 13.10.2026 (Agent CSS2): eine Zeilenklemme oeffnen (siehe
        // `Klemme`).
        let klemme_hier = self.kaskade.as_ref().and_then(|k| k.werte.last()).and_then(klemme_von);
        // Release 13.10.2026 (Agent CSS2): ein Kind mit eigenem
        // Formatierungskontext zaehlt nicht mit (CSS Overflow 4 §4.2: die
        // Klemme gilt in IHREM Formatierungskontext) — eine Sperre ohne
        // Grenze. GEMESSEN vorher (WPT `line-clamp/webkit-line-clamp-029`, ein
        // Kind mit `overflow: hidden`): seine Zeilen wurden geklemmt.
        let klemme_sperre = klemme_hier.is_none()
            && !self.klemmen.is_empty()
            && self.kaskade.as_ref().and_then(|k| k.werte.last()).is_some_and(|s| {
                s.ueberlauf != narsil_css::Ueberlauf::Sichtbar
                    || s.overflow_x.is_some_and(|u| u != narsil_css::Ueberlauf::Sichtbar)
                    || s.schweben != narsil_css::Float::Keins
                    || matches!(s.position, narsil_css::Position::Absolut | narsil_css::Position::Fest)
                    || matches!(
                        s.anzeige,
                        narsil_css::Anzeige::InlineBlock
                            | narsil_css::Anzeige::Flex
                            | narsil_css::Anzeige::InlineFlex
                            | narsil_css::Anzeige::Raster
                            | narsil_css::Anzeige::Tabelle
                    )
            });
        let bearbeitbar_hier = attributes.get("contenteditable").is_some_and(|v| !v.trim().eq_ignore_ascii_case("false"));
        if bearbeitbar_hier {
            self.bearbeitbar += 1;
        }
        if let Some((n, mit_auslassung)) = klemme_hier {
            self.klemmen.push(Klemme { rest: n, letzter_text: None, prepend: false, ellipse: !mit_auslassung, mit_auslassung });
        } else if klemme_sperre {
            self.klemmen.push(Klemme { rest: usize::MAX, letzter_text: None, prepend: false, ellipse: true, mit_auslassung: false });
        }
        for k in kinder {
            self.kind(k);
        }
        // Release 11.10.2026: ein `::before`-Text eines leeren Kastens und
        // der `::after`-Text gehoeren IN den Kasten.
        if !self.pseudo_text_wartend.is_empty() {
            self.pseudo_text_setzen();
        }
        if self.nach_wartend.last().is_some_and(|(t, _)| *t == self.tiefe) {
            self.nach_pseudo_setzen(attributes);
        }
        if ist_zelle {
            self.offene_stile.pop();
        }
        self.kasten_textstile.pop();
        // DER ABSATZ GEHOERT IN DIE BOX, nicht daneben.
        //
        // GEMESSENER FEHLER 30.09.2026: hier stand erst
        // `self.offen.pop()` und dann `absatz_schliessen()`. Der Absatz
        // wurde also geschlossen, NACHDEM die Box verlassen war — sein
        // Text landete beim Elternknoten. Im Bild: `<pre>` als leere
        // Box, und der Quelltext eine Zeile weiter unten, ausserhalb
        // der Box, die ihn umschliessen soll.
        self.absatz_schliessen();
        if klemme_hier.is_some() || klemme_sperre {
            self.klemmen.pop();
        }
        if bearbeitbar_hier {
            self.bearbeitbar = self.bearbeitbar.saturating_sub(1);
        }
        // 03.10. 12:55: das Formular ist zu. GEMESSENE BEGRUENDUNG fuer
        // die Stelle: erst NACH den Kindern (`for k in kinder`), denn ein
        // Feld im Formular muss das Formular noch sehen.
        self.formular_schliessen();
        // Link-Inhalt bleibt im Container, aber der folgende Absatz
        // gehoert wieder an den Elternknoten.
        self.einzug -= eigener_einzug;
        self.textbreite = textbreite_vorher;
        self.offen.pop();
        self.einzug_frei.pop();
        // W7C R3 (02.10., Rolle LAYOUT): den Elternzustand wiederherstellen,
        // damit der Aufrufer (`kind()`) den Kasten DIESES Elements liest.
        self.eigener_kasten = eigener_kasten_vorher;
    }

    /// Text: an den laufenden Absatz anhaengen, in einen Abschnitt mit
    /// dem gerade geltenden Stil.
    ///
    /// GEMESSENER FEHLER 30.09.2026: hier stand `absatz_starten()`, und
    /// das beendete den laufenden Absatz. Zwei aufeinanderfolgende
    /// Textstuecke ergaben damit ZWEI Absaetze — und ein Absatz mit
    /// einem `<b>` darin fuenf. Die Korrektur ist eine Zeile: es wird
    /// nur dann ein Absatz gestartet, wenn keiner offen ist.
    ///
    /// DER LEERZEICHEN-FALL, und er ist nichttrivial: der alte Code
    /// schrieb `t.split_whitespace().join(" ")` und thereby den
    /// ANGRAENZENDEN Leerraum. `<p>Das ist ein <b>x</b> mehr</p>` gab
    /// "Das ist einx mehr". Unsichtbar war das nur, weil die Teile in
    /// verschiedenen Absaetzen landeten — der Fehler war da, nur nicht
    /// sichtbar. `normalisiere` haelt jetzt EIN Leerzeichen am Anfang
    /// und am Ende, wie es in der Quelle stand.
    /// Release 08.10.2026: `white-space` (CSS Text 3 §3). Bei `pre`,
    /// `pre-wrap`, `pre-line` und `break-spaces` ist jedes Zeilenende ein
    /// harter Umbruch, und (ausser bei `pre-line`) bleiben Leerzeichen und
    /// Tabulatoren stehen. GEMESSEN vorher: jeder Text wurde
    /// zusammengefasst — `<pre>` stand in einer Zeile, die Code-Beispiele
    /// von python.org, MDN und docs.rs waren unlesbar.
    fn text(&mut self, t: &str) {
        use narsil_core::style::Weissraum as W;
        // Release 08.10.2026: zerlegte Umlaute/Akzente (u + U+0308) werden
        // vorkomponiert — sonst bekommt das kombinierende Zeichen einen
        // eigenen Vorschub („Trü mmern" auf spiegel.de).
        let t = narsil_text::komposition::komponiere(t);
        // Release 08.10.2026: `text-transform` (CSS Text 3 §2.1) — der Wert
        // ist geerbt und steht am innersten offenen Element. GEMESSEN vorher:
        // wurde gelesen, aber nie angewandt (t-online.de „SUCHEN", Apple-
        // und GitHub-Knoepfe in Grossbuchstaben).
        let umgewandelt = match self.kaskade.as_ref().and_then(|k| k.werte.last()).map(|w| w.text_transform.as_str()) {
            Some("uppercase") => Some(t.to_uppercase()),
            Some("lowercase") => Some(t.to_lowercase()),
            Some("capitalize") => Some(gross_anfang(&t)),
            _ => None,
        };
        let t: &str = umgewandelt.as_deref().unwrap_or(&t);
        if !self.pseudo_text_wartend.is_empty() && !t.chars().all(css_leerraum) {
            self.pseudo_text_setzen();
        }
        let modus = self.aktueller_weissraum();
        if !matches!(modus, W::Pre | W::PreUmbruch | W::PreZeile | W::UmbruchLeer) {
            self.text_ohne_pre(t);
            return;
        }
        let t = t.replace("\r\n", "\n").replace('\r', "\n");
        let teile: Vec<&str> = t.split('\n').collect();
        // HTML §13.2.6.4.7: ein Zeilenende direkt nach `<pre>` zaehlt nicht.
        let leer_am_anfang = teile.len() > 1
            && teile[0].is_empty()
            && self.laufender.as_ref().is_some_and(|a| a.text().is_empty());
        for (i, teil) in teile.iter().enumerate().take(MAX_ZEILEN_PRO_ABSATZ) {
            if i > 0 && !(i == 1 && leer_am_anfang) {
                self.harter_umbruch();
            }
            // Release 13.10.2026 (Agent CSS2): siehe `Klemme`.
            if self.klemmen.last().is_some_and(|k| k.rest == 0) {
                if teile[i..].iter().any(|t| !t.is_empty()) {
                    self.klemme_auslassen();
                }
                break;
            }
            let zeile = if modus == W::PreZeile {
                (*teil).to_string()
            } else {
                leerraum_erhalten(teil, modus, self.tab_zeichen())
            };
            if !zeile.is_empty() {
                self.text_ohne_pre(&zeile);
            }
        }
    }

    /// Release 12.10.2026 (Agent CSS2): `tab-size` (CSS Text 3 §4.2) als Zahl
    /// von Leerzeichen — eine Laenge geteilt durch die Breite des
    /// Leerzeichens der geltenden Schrift. GEMESSEN vorher (WPT
    /// `css-text/tab-size/*`): der Wert wurde gelesen, aber ein Tabulator
    /// sprang immer auf die naechste Achterstelle.
    fn tab_zeichen(&self) -> usize {
        let Some(w) = self.kaskade.as_ref().and_then(|k| k.werte.last()) else {
            return 8;
        };
        let n = if w.tabulator_px {
            let raum = self.messung.breite_stil_familie(
                "\u{a0}",
                w.schriftgroesse_px,
                w.schriftgewicht,
                w.schriftstil != narsil_css::Schriftstil::Normal,
                &w.schriftfamilie,
            );
            if raum.is_finite() && raum > 0.0 { (w.tabulator_groesse / raum).round() } else { 8.0 }
        } else {
            w.tabulator_groesse.round()
        };
        if n.is_finite() { n.clamp(0.0, 1000.0) as usize } else { 8 }
    }

    /// Der `white-space`-Wert, der fuer den naechsten Text gilt — dieselbe
    /// Reihenfolge der Quellen wie in `text_ohne_pre` (Marke, Absatz,
    /// Kasten, Zelle).
    fn aktueller_weissraum(&self) -> narsil_core::style::Weissraum {
        self.marken
            .last()
            .map(|m| m.stil.weissraum)
            .or_else(|| self.laufender.as_ref().map(|a| a.stil.weissraum))
            .or_else(|| self.kasten_textstile.last().map(|s| s.weissraum))
            .or_else(|| self.offene_stile.last().map(|s| s.weissraum))
            .unwrap_or_default()
    }

    /// Die Marke eines `<li>` (Release 07.10.2026: nach `list-style-type`
    /// und dem Zaehler der Liste) als erstes Zeichen des laufenden Absatzes.
    fn listenmarke_schreiben(&mut self, attributes: &HashMap<String, String>) {
        let art = self
            .kaskade
            .as_ref()
            .and_then(|k| k.werte.last())
            .map_or(narsil_css::Liste::Scheibe, |s| s.listenstil);
        let nummer = match self.listen_zaehler.last_mut() {
            Some((zaehler, schritt)) => {
                let wert = attributes
                    .get("value")
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .unwrap_or(*zaehler);
                *zaehler = wert.saturating_add(*schritt);
                wert
            }
            None => 1,
        };
        if let Some(marke) = listenmarke(art, nummer)
            && let Some(a) = self.laufender.as_mut()
            && let Some(erster) = a.abschnitte.first_mut()
        {
            erster.text.push_str(&marke);
        }
    }

    /// Release 08.10.2026: ein harter Zeilenumbruch IN einem Absatz
    /// (`<br>`, Zeilenende in `white-space: pre`). Die Zeile endet, der
    /// Absatz laeuft mit DEMSELBEN Stil weiter — ohne Absatzabstand
    /// dazwischen. Ein leerer Abschnitt (`<br><br>`) wird eine Leerzeile.
    ///
    /// GEMESSEN vorher: `<br>` schloss den Absatz, und der Text danach bekam
    /// die Vorgabe (schwarz, 16 px) statt des Absatzstils; zwischen den
    /// Teilen stand der untere Rand des Absatzes.
    fn harter_umbruch(&mut self) {
        let Some(a) = self.laufender.as_mut() else {
            return;
        };
        let mut weiter = a.stil.clone();
        let href = a.href.clone();
        if a.text().trim().is_empty() {
            // Leerzeile: ein geschuetztes Leerzeichen traegt die Zeilenhoehe.
            if let Some(erster) = a.abschnitte.first_mut() {
                erster.text.push('\u{a0}');
            }
        }
        a.stil.margin.bottom = Length::px(0.0);
        weiter.margin.top = Length::px(0.0);
        self.absatz_schliessen();
        self.absatz_starten(weiter, href);
    }

    /// Release 10.10.2026: aus `text_ohne_pre` gezogen (auch `::first-letter`
    /// braucht ihn): Stil, Link und Nummer des Textes an der jetzigen Stelle.
    fn geltender_textstil(&self) -> (Style, Option<String>, Option<usize>) {
            // Der geltende Stil: die innerste offene Inline-Marke, sonst der
            // Absatzstil, sonst die Vorgabe.
            let vorgabe = Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                color: FARBE_TEXT,
                font_size: GRAD_TEXT,
                ..Style::default()
            };
            //
            // W7C R6: die Nummer der INNERSTEN offenen Marke wandert mit in
            // den Abschnitt. Sie ist die Nummer des Inline-Elements, das den
            // Text traegt — der Absatz hat sie nicht (siehe `Marke::ord`).
            //
            // SCHRIFT (04.10.2026): der Stil des **offenen Kastens** kommt
            // dazu. GEMESSENER Befund: `<th>` rechnet 700 in der Kaskade und
            // traegt 700 in `kind()`, der Absatz in der Zelle bekam aber ein
            // festes Vorgabe-Literal (16 px, 400) — der Knoten also 400. Die
            // Reihenfolge ist Marke, Absatz, Kasten, Vorgabe: die innerste
            // offene Quelle gewinnt, und das ist bei einem `<td><b>` die
            // Marke, bei einem `<td>` der Kasten.
            self
                .marken
                .last()
                // 04.10.2026 (SCHRIFT): `m.stil.clone()` — siehe `Style`
                // in narsil-core (Feld `schriftfamilie`, kein `Copy` mehr).
                .map(|m| (m.stil.clone(), m.href.clone(), m.ord))
                .or_else(|| {
                    self.laufender
                        .as_ref()
                        // 04.10.2026 (SCHRIFT): `a.stil.clone()` — `Style`
                        // traegt `schriftfamilie` und ist nicht mehr `Copy`.
                        .map(|a| (a.stil.clone(), a.href.clone(), None))
                })
                //
                // SCHRIFT (04.10.2026): der Stil des **Kastens**, in dem der
                // Text steht — aber nur fuer eine Tabellenzelle.
                //
                // GEMESSENER BEFUND (vor der Reparatur): `<th>` ist ein Kasten
                // (`ist_absatz` ist false, lib.rs:9818 ff.), die Kaskade rechnet
                // **700** (`autor=700 mit_ua=700`), `kind()` traegt 700 in
                // `kommender_stil` — und der Knoten bekam **400**. Die Luecke
                // war diese Zeile: der Absatz in der Zelle bekam das feste
                // Vorgabe-Literal (16 px, `FARBE_TEXT`, 400) statt des
                // Kastenstils. GEMESSEN, dass es am Weg liegt und nicht am
                // Selektor: dieselbe Regel (`th { font-weight: bold }`) im
                // **Autoren**-CSS der Testseite aendert nichts, bleibt 400.
                //
                // GEMESSEN, warum **nur** die Zelle: die erste Fassung nahm den
                // Stil **jedes** Kastens, und drei fremde Tests meldeten
                // Regressionen — `a_adblock` 9 statt 7 Knoten, `echte_seiten`
                // ohne Einrueckung des Listenpunkts, `l3_style_block` 19,0
                // statt 19,2 px Zeilenhoehe. Der Nutzen kommt ausschliesslich
                // von `td`/`th`, also ist genau das die Bedingung.
                //
                // GEMESSEN, warum der **innere** offene Kasten richtig ist: bei
                // `<td><p>x</p></td>` ist es das `p`, und dessen Kaskadenwert ist
                // wegen der Vererbung ebenfalls 700 (die Vererbung fuer Gewicht
                // und Stil ist am 04.10. in `waehle_schriftwert` repariert).
                //
                // GEMESSEN, warum kein neues Feld fuer den Stil noetig ist:
                // `self.doc.get(...)` liest genau den Stil, den der Kasten
                // bekommen hat — eine zweite Besitzvorstellung waere eine
                // zweite Wahrheit.
                .or_else(|| {
                    //
                    // SCHRIFT (04.10.2026): der Stil der **offenen
                    // Tabellenzelle** — siehe `offene_stile`.
                    //
                    // GEMESSEN, warum die Reihenfolge Marke, Absatz, Zelle,
                    // Vorgabe ist: bei `<td><b>fett</b></td>` gewinnt die Marke
                    // (sie ist inner), bei `<td>Text</td>` die Zelle, und ohne
                    // Zelle die Vorgabe.
                    // 04.10.2026 (SCHRIFT): `st.clone()` statt `*st` — siehe
                    // `Style` in narsil-core (kein `Copy` mehr).
                    //
                    // Release 11.10.2026: zuerst ein offenes ABSATZ-Element
                    // (`p`, `li` …), wenn es inner als der Kasten ist
                    // (`block_textstil`). GEMESSEN vorher (WPT
                    // `inline-block-baseline-001`, `p { line-height: 5 }`):
                    // der Text HINTER einem `inline-block` bekam den Stil des
                    // `body` (`line-height: normal`) statt den des `p`.
                    self.block_textstil().map(|st| (st, None, None))
                })
                .unwrap_or((vorgabe, None, None))
    }

    /// Release 10.10.2026: der vererbbare Textstil des innersten offenen
    /// Blocks — Absatz-Element (`p`, `li` …) oder Kasten —, ohne Marken.
    fn block_textstil(&self) -> Option<Style> {
        // Release 11.10.2026: ein Bildabsatz, in dem seither weder Kasten noch
        // Absatz-Element aufging, ist der innerste Block. GEMESSEN vorher (WPT
        // `inline-block-baseline-010`-Referenz, `p { line-height: 5 }` mit
        // `<img>`): der Text hinter dem Bild bekam `line-height: normal`.
        if let Some((k, r, stil)) = self.bild_absatz_stile.last()
            && *k >= self.kasten_textstile.len()
            && *r >= self.absatz_rahmen.len()
        {
            return Some(stil.clone());
        }
        match self.absatz_rahmen.last() {
            Some(r) if r.kasten_tiefe >= self.kasten_textstile.len() => Some(erbbarer_textstil(&r.stil)),
            _ => self.kasten_textstile.last().or(self.offene_stile.last()).cloned(),
        }
    }

    /// Release 15.10.2026 (Agent CSS2): takes the waiting `::first-line`
    /// orders and returns the colour of the innermost (if its rules set one).
    fn erste_zeile_farbe(&mut self) -> Option<Color> {
        let a = self.erste_zeile.last()?;
        let setzt_farbe = a.regeln.iter().any(|r| match r {
            narsil_css::Regel::Stil(s) => s.deklarationen.iter().any(|d| matches!(d.eigenschaft, narsil_css::Eigenschaft::Farbe)),
            _ => false,
        });
        let farbe = if setzt_farbe {
            let k = self.kaskade.as_ref()?;
            let innen = k.werte.last()?;
            let berechnet = kaskade::erster_buchstabe_stil(&k.baum, a.id, &a.regeln, innen, self.breite, k.ansicht_hoehe);
            let (hand, _, _) = self.geltender_textstil();
            kaskade::stil_von_css(hand, &berechnet, &berechnet, Some(innen), false).map(|s| s.color)
        } else {
            None
        };
        self.erste_zeile.clear();
        farbe
    }

    /// Release 10.10.2026: die Marke des `::first-letter` — die Regeln der
    /// Auftraege (aussen zuerst) ueber dem Stil des Elements, in dem der
    /// Text steht.
    fn erster_buchstabe_marke(&self, auftraege: &[ErsterBuchstabeAuftrag]) -> Option<Marke> {
        let k = self.kaskade.as_ref()?;
        let innen = k.werte.last()?;
        let mut berechnet = innen.clone();
        for a in auftraege {
            berechnet =
                kaskade::erster_buchstabe_stil(&k.baum, a.id, &a.regeln, &berechnet, self.breite, k.ansicht_hoehe);
        }
        let (hand, href, ord) = self.geltender_textstil();
        let stil = kaskade::stil_von_css(hand, &berechnet, &berechnet, Some(innen), false)?;
        Some(Marke { stil, href, ord })
    }

    fn text_ohne_pre(&mut self, t: &str) {
        // 02.10. 19:05 (HTML): Text **innerhalb** eines `<label>` sammeln — das
        // ist der Inhalt, an dem sich die Regel spaeter wiedererkennt
        // (`LabelSchluessel`).
        self.label_text_sammeln(t);
        // Reiner Leerraum: genau ein Leerzeichen, und nur wenn ueberhaupt
        // Text da ist. Sonst stuende am Absatzanfang ein Leerzeichen.
        //
        // GEMESSENER BEFUND (03.10. 07:53, Wegwerf-Messung `_m_abs.rs`,
        // entfernt), warum dieser Block **unveraendert** blieb: der Leerraum
        // gehoert nicht hierher. Ein eigener Leerraum-Abschnitt an dieser
        // Stelle entsteht zwar (GEMESSEN: `abschnitte=[…, ("eins", true),
        // (" ", false)]`), aber `woerter_mit_leerzeichen(" ")` liefert
        // `liste=[]` (GEMESSEN, 07:53) — der Abschnitt erzeugt **kein Wort**,
        // und der Leerraum wandert als `endet_leer` an das naechste Wort. Er
        // landete damit wieder im Link.
        //
        // Die Regel steht jetzt in der Zeilen-Run-Schleife von
        // `absatz_schliessen` (dort: „GEMESSENER FEHLER 03.10. 07:53"),
        // weil **dort** der vorherige und der folgende Abschnitt bekannt
        // sind — hier ist nur der letzte da, und genau der ist das Problem.
        if t.chars().all(css_leerraum) {
            // A preceding atomic image is inline content, even without a pending paragraph.
            if self.laufender.is_none() && self.inline_bild_eltern.contains(&self.mutter()) {
                let parent = self.mutter();
                let has_inline_content = self.doc.get(parent).is_some_and(|n| !n.children.is_empty());
                if has_inline_content {
                    let style = self.marken.last().map(|m| m.stil.clone())
                        .or_else(|| self.doc.get(parent).map(|n| n.style.clone())).unwrap_or_default();
                    self.absatz_starten(style, None);
                    if let Some(a) = self.laufender.as_mut() { a.abschnitte[0].text.push(' '); }
                }
            }
            let kontext_bricht = self
                .marken
                .last()
                .map(|m| m.stil.weissraum)
                .or_else(|| self.kasten_textstile.last().map(|s| s.weissraum))
                .unwrap_or_default()
                != narsil_core::style::Weissraum::KeinUmbruch;
            if let Some(a) = self.laufender.as_mut()
                && let Some(letzter) = a.abschnitte.last_mut()
                && !letzter.text.is_empty()
            {
                if !letzter.text.ends_with(' ') {
                    letzter.text.push(' ');
                }
                letzter.leer_bricht |= kontext_bricht;
            }
            return;
        }

        // Release 10.10.2026: ein wartendes `::first-letter` loest der erste
        // echte Text ein — der Buchstabe (mit seinen Satzzeichen) wird ein
        // eigener Abschnitt mit dem Stil des Pseudos (wie ein `<span>`).
        if !self.erster_buchstabe.is_empty()
            && let Some((vor, kern, rest)) = kaskade::erster_buchstabe_teilen(t)
            && let auftraege = std::mem::take(&mut self.erster_buchstabe)
            && !kern.is_empty()
        {
            let (vor, kern, rest) = (vor.to_string(), kern.to_string(), rest.to_string());
            if !vor.is_empty() {
                self.text_ohne_pre(&vor);
            }
            let marke = self.erster_buchstabe_marke(&auftraege);
            let gesetzt = match marke {
                Some(m) if self.marken.len() < MAX_INLINE_TIEFE => {
                    self.marken.push(m);
                    true
                }
                _ => false,
            };
            self.text_ohne_pre(&kern);
            if gesetzt {
                self.marken.pop();
                // Hinter dem Buchstaben beginnt ein neuer Abschnitt, auch
                // wenn der Stil gleich waere (der Pseudo-Stil kann nur die
                // Nummer teilen).
                if let Some(a) = self.laufender.as_mut() {
                    a.trennen = true;
                }
            }
            if !rest.is_empty() {
                self.text_ohne_pre(&rest);
            }
            return;
        }
        let (mut stil, href, ord) = self.geltender_textstil();
        // Release 11.10.2026: die Sprache gehoert zum Abschnitt (Umbruch).
        stil.cjk_sprache = self.sprachen.last().is_some_and(|s| s.1);

        if self.laufender.is_none() {
            // 04.10.2026 (SCHRIFT): `.clone()` — der Abschnitt weiter
            // unten vergleicht `stil` mit dem Stil des laufenden
            // Abschnitts, und `Style` ist nicht mehr `Copy`.
            //
            // Release 10.10.2026: beginnt der Absatz IN einer Inline-Marke,
            // bekommt er den Stil seines Blocks, nicht den der Marke.
            // GEMESSEN vorher (WPT `first-letter-selector-001-ref`):
            // `<div><span>F</span>iller</div>` mit `span { color: green }`
            // zeichnete auch „iller" gruen — der Text nach der Marke erbte
            // den Absatzstil, und der war der des Spans (ebenso ein Link als
            // erstes Kind: der Text dahinter wurde klickbar).
            match self.block_textstil().filter(|_| !self.marken.is_empty()) {
                Some(grund) => self.absatz_starten(grund, None),
                None => self.absatz_starten(stil.clone(), href.clone()),
            }
        }
        let normalisiert = weissraum(t);
        if normalisiert.is_empty() {
            return;
        }
        // M3: Ein Abschnitt je Stilwechsel, hoechstens MAX_ABSCHNITTE.
        //
        // W7C R6 (01.10. 01:05): DIE NUMMER GEHOERT ZUR IDENTITAET EINES
        // ABSCHNITTS.
        //
        // GEMESSENER FEHLER: `<p>Stand: <span id="z">0</span></p>` — der
        // Span hat DENSELBEN Stil wie der Absatz, also passte sein Text
        // in den bestehenden Abschnitt, und der Abschnitt behielt die
        // Nummer des Absatzes. Ergebnis: `Seite::ids["z"]` (7) kam in
        // `dom_zu_ord` nicht vor — ein Klick auf den Span fand keine
        // Nummer, und `text_von_id("z")` in `zaehler_html.rs` gab "".
        // Die Anzeige war richtig („Stand: 1"), der Zustand falsch: ein
        // Test, der den Zustand liest, sieht etwas anderes als der
        // Nutzer. Genau das ist der Unterschied zwischen „geht" und
        // „ist richtig".
        let passt = self
            .laufender
            .as_ref()
            .and_then(|a| a.abschnitte.last())
            .map(|s| s.stil == stil && s.href == href && s.ord == ord)
            .unwrap_or(false)
            // Release 08.10.2026: hinter einem Anker beginnt ein neuer
            // Abschnitt (siehe `Absatz::trennen`).
            && !self.laufender.as_ref().is_some_and(|a| a.trennen);
        if !passt
            && let Some(a) = self.laufender.as_mut()
            && a.abschnitte.len() < MAX_ABSCHNITTE
        {
            a.abschnitte.push(Abschnitt {
                text: String::new(),
                stil,
                href,
                ord,
                leer_bricht: false,
            });
        }
        if let Some(a) = self.laufender.as_mut() {
            a.trennen = false;
        }
        if let Some(a) = self.laufender.as_mut()
            && let Some(letzter) = a.abschnitte.last_mut()
        {
            letzter.text.push_str(&normalisiert);
        }
    }

    /// Einen Absatz (neu) starten: den laufenden zuerst wegschreiben.
    /// Resolve the width available to text while its containing block is
    /// still on the builder stack. Percentages use that block, not the viewport.
    fn inhaltsbreite(&self, stil: &Style) -> f32 {
        let basis = self.textbreite;
        let rand = stil.margin.left.resolve_or(basis, 0.0)
            + stil.margin.right.resolve_or(basis, 0.0);
        let polster = stil.padding.left.resolve_or(basis, 0.0)
            + stil.padding.right.resolve_or(basis, 0.0);
        let mut breite = (stil.size.width.resolve_or(basis, basis - rand) - polster).max(0.0);
        if let Some(plan) = self.plan_stapel.last() {
            let g = plan.grenzen.unwrap_or_default();
            let p = plan.grenzen_prozent.unwrap_or_default();
            let max = p.max_breite.map(|v| (basis - rand).max(0.0) * v / 100.0).or(g.max_breite);
            let min = p.min_breite.map(|v| (basis - rand).max(0.0) * v / 100.0).or(g.min_breite);
            if let Some(max) = max { breite = breite.min(max); }
            if let Some(min) = min { breite = breite.max(min); }
        }
        breite.max(1.0)
    }

    fn absatz_starten(&mut self, stil: Style, href: Option<String>) {
        //
        // W6E: DER ALTE ABSAZ WIRD ZUERST GESCHLOSSEN (16:55).
        //
        // GEMESSENER FEHLER, den diese Zeile hatte: `laufender_ord` stand
        // VOR `absatz_schliessen`, also bekam der Absatz des vorherigen
        // Blocks die Nummer des neuen — und die Knoten des Knopfs trugen
        // die Nummer des `<p>`. Siehe `Absatz::ord`.
        if self.laufender.is_some() {
            self.absatz_schliessen();
        }
        //
        // W6E: DIE NUMMER DES UMSCHLIESSENDEN ELEMENTS (16:55), nicht die
        // des letzten `kind()`-Aufrufs. GEMESSEN: bei
        // `<button id="k">Klick mich</button>` war der letzte Aufruf der
        // Textknoten — der Absatz gehoerte ihm, nicht dem Knopf.
        let absatz_ord = self.element_stapel.last().copied();
        // W6: die Vorlauf-Nummer des absatz-startenden Elements merken
        // (siehe `Bauer::laufender_ord` und `Absatz::ord`).
        self.laufender_ord = absatz_ord;
        //
        let breite = self.inhaltsbreite(&stil);
        // Release 11.10.2026 (Agent CSS2): Ausrichtung und Zeilenhoehe des
        // Absatzes gehoeren seinem BLOCK, nicht dem Inline-Element, in dem
        // der erste Text steht (CSS 2.1 §16.2: `text-align` gilt fuer
        // Blockcontainer; §10.8: die Zeilenbox beginnt mit dem Strut des
        // Blocks). GEMESSEN vorher (WPT `display-018`): `caption { display:
        // inline }` zentrierte mit seinem UA-`text-align` die ganze Zeile.
        let typografie = self
            .kaskade
            .as_ref()
            .and_then(|k| k.werte.iter().rev().find(|w| w.anzeige != narsil_css::Anzeige::Inline && !w.nur_inhalt).or(k.werte.last()));
        let zeilenhoehe = typografie.and_then(|s| (!matches!(s.zeilenhoehe_art, narsil_css::ZeilenhoeheArt::Normal)).then_some(s.zeilenhoehe_px));
        let ausrichtung = typografie.map_or(narsil_css::Textausrichtung::Links, |s| s.textausrichtung);
        // Release 13.10.2026 (Agent CSS2): `text-align-last: auto` ist die
        // Ausrichtung selbst, bei `justify` aber `start` (CSS Text 3 §7.2).
        let ausrichtung_letzte = typografie.map_or(Some(ausrichtung), |s| match s.ausrichtung_letzte {
            Some(a) => Some(a),
            None if s.blocksatz_alle => Some(narsil_css::Textausrichtung::Blocksatz),
            None if s.textausrichtung == narsil_css::Textausrichtung::Blocksatz => Some(if s.richtung.start_ist_rechts() {
                narsil_css::Textausrichtung::Rechts
            } else {
                narsil_css::Textausrichtung::Links
            }),
            None => Some(s.textausrichtung),
        });
        let blocksatz_aus = typografie.is_some_and(|s| s.blocksatz_aus);
        // Release 13.10.2026 (Agent CSS2): die Startseite fuer
        // `hanging-punctuation` (in `rtl` rechts).
        let rtl = typografie.is_some_and(|s| s.richtung.start_ist_rechts());
        // Release 14.10.2026 (Agent CSS2): see `BidiMode`.
        let bidi_mode = match typografie.map(|s| s.bidi) {
            Some(narsil_css::Bidi::Plaintext) => BidiMode::Plaintext,
            Some(narsil_css::Bidi::BidiOverride | narsil_css::Bidi::IsolateOverride) => BidiMode::Override,
            _ => BidiMode::Normal,
        };
        // Release 13.10.2026 (Agent CSS2): `text-overflow` gilt in einem Block,
        // der seinen Ueberlauf beschneidet (CSS Overflow 4 §3.3).
        let beschnitten = typografie.is_some_and(|s| {
            s.ueberlauf != narsil_css::Ueberlauf::Sichtbar
                || s.overflow_x.is_some_and(|u| u != narsil_css::Ueberlauf::Sichtbar)
                || s.ueberlauf_clip[0]
        });
        // Ein Rollkasten (`overflow: scroll`) wird gerollt, und ein
        // bearbeitbarer Block zeigt beim Bearbeiten keine Auslassung
        // (GEMESSEN: WPT `text-overflow-021`, `-ellipsis-editable-div-with-
        // caret`).
        let rollt = typografie.is_some_and(|s| {
            s.ueberlauf == narsil_css::Ueberlauf::Scroll || s.overflow_x == Some(narsil_css::Ueberlauf::Scroll)
        });
        // Release 15.10.2026 (Agent CSS2): text directly in a flex or grid
        // container lives in an anonymous item, a block container of its own
        // that does not inherit `text-overflow` (CSS Overflow 4 §3.3:
        // "applies to block containers"; not inherited). MEASURED before (WPT
        // css-flexbox `text-overflow-on-flexbox-001`, reported by the
        // coordinator): the flex container's text ended with "…".
        // A `display: -webkit-box` with `-webkit-line-clamp` (here a flex
        // column, see `zeilen_klemme`) lays its text out as a block (WPT
        // css-overflow `webkit-line-clamp-037`: text-overflow applies).
        let im_flex = typografie.is_some_and(|s| {
            matches!(
                s.anzeige,
                narsil_css::Anzeige::Flex | narsil_css::Anzeige::InlineFlex | narsil_css::Anzeige::Raster
            ) && zeilen_klemme(s).is_none()
        });
        let auslassung = typografie
            .filter(|_| beschnitten && !rollt && !im_flex && self.bearbeitbar == 0)
            .and_then(|s| auslassung_von(&s.text_ueberlauf));
        let max_zeilen = typografie.and_then(zeilen_klemme);
        // Release 11.10.2026 (Agent CSS2): der Einzug gehoert der ersten Zeile
        // des Blocks (nicht nach `<br>` oder einem eingeschobenen Block).
        let erste = self.einzug_frei.last().copied().unwrap_or(true);
        if let Some(t) = self.einzug_frei.last_mut() {
            *t = false;
        }
        // Ein Prozentwert gilt gegen die Breite des Blocks (CSS 2.1 §16.1;
        // WPT `text-indent-percent-001`, `text-indent-011`).
        let einzug = if erste {
            typografie.map_or(0.0, |s| s.text_einzug + s.text_einzug_prozent.map_or(0.0, |p| p / 100.0 * breite))
        } else {
            0.0
        };
        let einzug = if einzug.is_finite() { einzug.clamp(-100_000.0, 100_000.0) } else { 0.0 };
        let haengt = erste
            && typografie.is_some_and(|s| {
                s.schluessel
                    .iter()
                    .any(|(n, w)| n == "hanging-punctuation" && w.split_whitespace().any(|t| t.eq_ignore_ascii_case("first")))
            });
        // The paragraph of a block pseudo-element follows the pseudo's own
        // value (`pseudo_balance`, WPT `text-wrap-balance-before-after-002`).
        let balance = self.pseudo_balance.unwrap_or_else(|| typografie.is_some_and(kaskade::text_wrap_balance));
        let shrinks = typografie.is_some_and(|s| {
            (s.anzeige == narsil_css::Anzeige::InlineBlock
                || s.schweben != narsil_css::Float::Keins
                || matches!(s.position, narsil_css::Position::Absolut | narsil_css::Position::Fest))
                && s.breite.is_none()
                && s.breite_prozent.is_none()
                && s.mischung.as_ref().is_none_or(|m| m.breite.is_none())
        });
        self.laufender = Some(Absatz {
            shrinks,
            balance,
            max_zeilen,
            breite,
            zeilenhoehe,
            ausrichtung,
            ausrichtung_letzte,
            blocksatz_aus,
            ord: absatz_ord,
            // 04.10.2026 (SCHRIFT): `stil.clone()` — der Wert geht an
            // `self.laufender` und an den ersten Abschnitt (siehe
            // unten); `Style` ist nicht mehr `Copy` (Feld
            // `schriftfamilie`).
            stil: stil.clone(),
            href: href.clone(),
            // M4: ein Abschnitt, und keiner mehr vor der Allokation. Der
            // Vec waechst von selbst, wenn Inline-Elemente kommen.
            abschnitte: vec![Abschnitt {
                text: String::new(),
                stil,
                href,
                // Der ERSTE Abschnitt gehoert dem Absatz; die folgenden
                // entstehen in `text()` aus einer Marke (W7C R6).
                ord: None,
                leer_bricht: false,
            }],
            anker: Vec::new(),
            trennen: false,
            einzug,
            haengt,
            rtl,
            bidi_mode,
            beschnitten,
            auslassung,
        });
    }

    /// Ein Platzhalter als eigene Zeile (fuer Medien/Formulare ohne JS).
    ///
    /// Eigene Zeile statt Inline-Anhang: der Platzhalter hat einen eigenen
    /// Stil (schwach, lesbar) und darf den laufenden Absatz nicht
    /// umfaerben. Der laufende Absatz ist vorher zu schliessen (macht der
    /// Aufrufer) — hier steht nur der Knoten.
    /// EIN KAESTCHEN (oder ein RUNDKNopf) als Knoten — H-AR1 (19:05).
    ///
    /// # WAS ES ZEICHNET
    ///
    /// Zwei Knoten: ein äusserer (13 x 13 px, dunkel) und ein innerer
    /// (mit 2 px Innenabstand, in der Farbe der Seite). Der sichtbare Rest
    /// des äusseren Kastens ist der Rahmen, und der innere ist die Fläche,
    /// die ein Haken bekommen kann. Das ist derselbe Trick, mit dem wir
    /// einen Rahmen zeichnen (`Style` hat keine Rahmenfelder, siehe I2 in
    /// `element()`) — eine vorhandene Loesung, kein zweiter Weg.
    ///
    /// # GEMESSENE ENTSCHEIDUNG, wie der Haken aussieht
    ///
    /// Angehaakt bekommt der innere Knoten dieselbe dunkle Farbe und 4 px
    /// Innenabstand (ein gefuelltes Kaestchen in einem); nicht angehaakt
    /// bleibt er in der Seitenfarbe (ein helles Kaestchen). GEMESSENE
    /// BEGRUENDUNG: ein Haken ist eine Linie, und Linien zeichnet unser
    /// Raster nicht aus Text. Ein gefuelltes Kaestchen ist unterscheidbar
    /// (am Pixel: 5x5 dunkel statt 9x9 hell) und ehrlich — es sagt „an",
    /// nicht „irgendwie angehaakt".
    ///
    /// # DER RUNDKNOPP
    ///
    /// `radio` bekommt runden Ecken (Radius = halbe Kantenlaenge), weil man
    /// sonst nicht sieht, dass es ein Radio ist. `radius` ist ein Feld von
    /// `Style` — es wird also an einer Stelle gesetzt und nicht gefaelscht.
    fn feld_kasten(&mut self, angehakt: bool, typ: &str) -> Option<narsil_dom::NodeId> {
        const KANTE: f32 = 13.0;
        const RAND: f32 = 2.0;
        let rund = typ == "radio";
        let radius = if rund { [KANTE / 2.0; 4] } else { [2.0; 4] };
        self.feld_kasten_mit(angehakt, typ, KANTE, RAND, radius)
    }

    /// DER EIGENTLICHE KAESTCHENBAU — auch fuer die Felder mit INHALT.
    ///
    /// 03.10. 19:4x (HTML, Auftrag der Leitung): `<input type=search>`,
    /// `<select>` und `<button>` kommen als **Text** heraus
    /// (`[Eingabe search: search]`, `[Auswahl language: Afrikans]`,
    /// `[Knopf: ]`) und sind damit im Vergleichsbild www.wikipedia.org die
    /// groesste Fehlerklasse nach dem fehlenden Canvas (BRETT 18:5x).
    ///
    /// GEMESSENER BEFUND, der die Behebung von hier aus begruendet
    /// (`tests/_m_form2.rs`, Wegwerf, entfernt; 1280 px):
    ///
    ///   div  mit `border: 2px; height: 44px; padding: 8px 12px`
    ///        -> Kasten 304x48, RAHMEN sichtbar (bg auf 304x48 und 300x35,2)
    ///   input mit DENSELBEN Regeln
    ///        -> 4 Kaesten, der groesste 336x16, bg=None, EIN Textknoten
    ///           "[Eingabe search: search]"
    ///
    /// **Die Kaskade ist nicht die Ursache** — dieselben Regeln an einem
    /// `<div>` kommen an. Die Ursache ist, dass `kind()` fuer diese vier
    /// Elemente `self.platzhalter(...)` aufruft und damit **gar keinen
    /// Kasten** baut; `element()` (mit `rahmen_von()`, `element_innen`) laeuft
    /// fuer sie nie.
    ///
    /// DER WEG, und warum er derselbe ist wie beim Kaestchen: der Rahmen
    /// traegt die Farbe als Hintergrund und die Staerke als `padding`, der
    /// Inhalt sitzt in einem Kindknoten (I2, `element_innen`). Das ist an
    /// `element_innen` bereits gebaut und gemessen — dieser Aufruf ist
    /// dieselbe Form, mit dem Inhalt aus `beschriftung`.
    ///
    /// GEMESSENE GRENZE, die ich nicht ueberschreite: `Style` hat keine
    /// Rahmenfelder, also kann ein Rahmen keine vier verschiedenen Farben
    /// haben (das ist die bestehende Entscheidung in `rahmen_von`, dort
    /// dokumentiert). Und `border-right-width: 0` an `#searchInput` heisst
    /// hier „kein Rand rechts" — die Breite 0 wird uebernommen, nicht
    /// gerundet.
    fn feld_kasten_mit(
        &mut self,
        angehakt: bool,
        typ: &str,
        kante: f32,
        rand: f32,
        radius: [f32; 4],
    ) -> Option<narsil_dom::NodeId> {
        // Das KAESTCHEN: quadratisch, Inhalt ist der Haken (innen), Rand
        // schwarz. `kaestchen: true` — der Inhalt entscheidet das nicht
        // (siehe die Korrektur am Parameter `hoehe`).
        let st = FormularStil::kaestchen();
        // Release 08.10.2026: der HAKEN steht im Inhalt — `None` ist die
        // dunkle Innenflaeche (angehakt), ein leerer Inhalt die helle. Vorher
        // kam `angehakt` hier nie an, und jedes Kaestchen war innen schwarz.
        let inhalt = if angehakt { None } else { Some("") };
        self.feld_kasten_mit_inhalt(true, typ, kante, kante, &st, radius, inhalt)
    }

    /// Was die KASKADE ueber ein Formularelement weiss.
    ///
    /// 03.10. 21:2x (HTML): Die Formular-Zweige bauen ihren Kasten **ohne**
    /// `element()`, also laeuft `rahmen_von()` und `element_innen()` fuer sie
    /// nie — und damit auch der Rest der Kaskade nicht. GEMESSEN an
    /// wikipedia (`_m_form_osd.rs`, 1280 px):
    ///
    /// ```text
    /// #searchInput  bg=Color{118,118,118}  size=192x192
    /// ```
    ///
    /// obwohl die Seite `#searchInput { height: 4.4rem; border-left-width:
    /// 1px; padding: .8rem 9.6rem .8rem 1.2rem }` schreibt und
    /// `--border-color-base: #a2a9b1` setzt. **Alle drei Angaben standen im
    /// Dokument, und keine kam an.**
    ///
    /// DER WEG: die Werte stehen in `Berechnet` — `self.kaskade.werte.last()`
    /// traegt sie fuer das gerade betretene Element, genau wie in
    /// `rahmen_von`. Das ist keine zweite Kaskade, es ist dieselbe; sie wird
    /// nur an einer Stelle mehr gelesen.
    ///
    /// GEMESSENE GRENZE, die ich benenne statt sie zu ueberspielen: `Berechnet`
    /// rechnet **Prozent** nicht auf (`width: 100%` bleibt `Percent(100)`),
    /// und es kennt keine Geraetefelder (`::-webkit-search-cancel-button`).
    /// Fuer `width` gilt darum die Dreiwegregel wie ueberall: die Kaskade,
    /// sonst `size` in Zeichen (HTML 4.01, S. 46).
    fn feld_von_kaskade(&mut self) -> FormularStil {
        let Some(b) = self.kaskade.as_ref().and_then(|k| k.werte.last()).cloned() else {
            return FormularStil::standard();
        };
        // HTML_1 (04.10.2026): DER VORGABERAHMEN AUS DER UA-EBENE.
        //
        // GEMESSENER BEFUND VORHER (Kasten, `tests/feld_rahmen.rs`):
        //
        // ```text
        //   MESSUNG <input type="text">  box 192x22.4  padding 0/0/0/0
        //                                  background #767676
        //   MESSUNG <textarea>           box 192x44.8  padding 0/0/0/0
        //   MESSUNG docs.rs #nav-search  box 259.2x22.4  padding_oben 0
        //   MESSUNG docs.rs #search      box 403.2x22.4  padding_oben 0
        // ```
        //
        // Der Rahmen IST der `padding` des aeusseren Kastens (I2). Bei 0 px
        // liegt die weisse Feldflaeche des Kindes auf der grauen Flaeche
        // und deckt sie zu — **das Feld ist im Bild unsichtbar**, und im
        // Vergleichsbild steht nur sein Platzhaltertext.
        //
        // GEMESSENE URSACHE: `k.werte.last()` traegt `autor`, die
        // Autorenebene **ohne** UA-Blatt. Das UA-Blatt steht in `mit_ua`
        // (`narsil-css/src/ua.rs:216`: `input, textarea { border: 1px solid
        // #8f8f9d }`, GEMESSEN an Firefox 157), und `mit_ua` war eine
        // LOKALE Variable. Die 1 px aus `FormularStil::standard()` waren
        // damit **toter Code** — sie kommen nur heraus, wenn es gar keine
        // Kaskade gibt, und die gibt es durch `mit_ua` immer.
        //
        // GEMESSENE REGEL, und sie ist die Kaskadenregel: **der Autor
        // schlaegt das UA-Blatt.** Also nur dann den UA-Rahmen nehmen, wenn
        // der Autor keinen geschrieben hat. Die Gegenprobe entscheidet:
        //
        // ```text
        //   Fall                        autor  mit_ua  Ergebnis
        //   <input type="text">            0       1     UA-Rahmen 1 px
        //   <style>#q{border:0}</style>    0       0     kein Rahmen
        //   <style>#q{border:3px red}</style> 3     3     Autorenrahmen 3 px
        // ```
        //
        // GEMESSENE ENTSCHEIDUNG, warum gerade diese drei und keine
        // Framezahl aus dem Autor: `border: 0` und „gar nichts" sind in
        // `autor` **beide** 0 — sie sind dort nicht zu unterscheiden. In
        // `mit_ua` sind sie es: der Autor schlaegt das UA-Blatt, also steht
        // seine 0 auch dort. Die Unterscheidung kommt also aus dem
        // VERGLEICH der Ebenen, nicht aus einer Zahl.
        // HTML_2 (04.10.2026): RAHMEN UND POLSTERUNG GETRENNT — und die
        // Trennung ist gemessen, nicht gesetzt.
        //
        // GEMESSENER FEHLER MEINER ERSTEN FASSUNG (die hatte beide durch
        // **eine** Entscheidung geschickt, `!hat_rahmen`):
        //
        // ```text
        //   Fall                      RAHMEN         POLSTERUNG (Soll / gemessen)
        //   ohne alles                 1/1/1/1        1/2/1/2  /  1/2/1/2   ok
        //   Autor `padding:9px`       1/1/1/1        9/9/9/9  /  9/9/9/9   ok
        //   Autor `padding:0`         1/1/1/1        0/0/0/0  /  0/0/0/0   ok
        //   Autor `border:3px solid red`  3/3/3/3    2/1/2/1  /  0/0/0/0   FALSCH
        //   Autor `border:0`           0/0/0/0        2/1/2/1  /  0/0/0/0   FALSCH
        // ```
        //
        // GEMESSENE URSACHE: `border` nimmt in CSS 2.1, 8.3 **nur den
        // Rahmen**. Wer dem Autor einen Rahmen gibt, verliert die
        // Vorgabepolsterung nicht — sie gilt weiter, nur ihre Breite wird
        // nicht mehr gebraucht. In `ua.rs:216` stehen beide in EINER
        // Deklaration, und daraus habe ich eine gemacht; die Kaskade
        // behandelt sie als zwei Eigenschaften, und das ist richtig.
        //
        // GEMESSENE ENTSCHEIDUNG, warum nicht ein zweites Feld noetig ist:
        // `ua` wird **vor** dem Rahmenfilter gelesen und nur fuer den
        // Rahmen gefiltert. Die Kopplung war meine, nicht die der Kaskade.
        let ua = self.feld_rahmen_ua.take();
        let rand_oben_autor = beschaendige(b.rahmen_oben_breite);
        let rand_rechts_autor = beschaendige(b.rahmen_rechts_breite);
        let rand_unten_autor = beschaendige(b.rahmen_unten_breite);
        let rand_links_autor = beschaendige(b.rahmen_links_breite);
        let hat_rahmen = rand_oben_autor
            .max(rand_rechts_autor)
            .max(rand_unten_autor)
            .max(rand_links_autor)
            > 0.0;
        //
        // GEMESSENE ENTSCHEIDUNG (HTML_2), warum der Ebenen-Vergleich aus
        // HTML_1 hier **entfaellt**: `mit_ua` ist ueber `k.alle` gerechnet
        // und traegt den Autorenwert, wenn es einen gibt. Die Frage „hat der
        // Autor einen Rahmen geschrieben?" hatte also nie eine
        // Vergleichslogik noetig — sie hat eine **andere Ebene**, und die
        // ist genau diese. GEMESSENE Gegenproben dazu in
        // `tests/feld_rahmen.rs::autor_darf_rahmen_und_polsterung_je_fuer_sich_abschalten`:
        //
        // ```text
        //   Fall                        rand      farbe        fuellung
        //   ohne alles                  1/1/1/1   #8f8f9d       1/2/1/2
        //   Autor `border:3px solid red` 3/3/3/3  (255,0,0)    1/2/1/2
        //   Autor `border:0`             0/0/0/0   —            1/2/1/2
        //   Autor `padding:9px`          1/1/1/1   #8f8f9d       9/9/9/9
        //   Autor `padding:0`            1/1/1/1   #8f8f9d       0/0/0/0
        // ```
        //
        // GEMESSENE GRUNDLAGE, warum `Some` hier „die UA-Ebene wurde fuer
        // DIESES Element gerechnet" bedeutet und nicht „die Ebene sagt
        // etwas": ohne Kaskade (`FormularStil::standard`) gibt es kein
        // `feld_rahmen_ua`, und dann gilt die Handvorgabe 1 px / 1-2 px —
        // dieselben Zahlen, weil sie dieselbe Quelle haben.
        let (rand_oben, rand_rechts, rand_unten, rand_links) = match ua {
            Some(u) => (u.oben, u.rechts, u.unten, u.links),
            None => (
                rand_oben_autor,
                rand_rechts_autor,
                rand_unten_autor,
                rand_links_autor,
            ),
        };
        let hat_rahmen = rand_oben.max(rand_rechts).max(rand_unten).max(rand_links) > 0.0;
        // GEMESSENE ENTSCHEIDUNG, warum die RAHMENFARBE aus `rahmen_farbe`
        // kommt und nicht aus `b.hintergrund`: der Rahmen ist ein Hintergrund
        // der Box (I2), und der Inhalt sitzt in einem Kindknoten. Das ist
        // genau die Entscheidung aus `rahmen_von` — dieselbe Funktion, damit
        // es dieselbe sein muss.
        //
        // HTML_1: MIT UA-RAHMEN kommt die Farbe aus der UA-Ebene
        // (GEMESSEN `#8f8f9d`), sonst die wie bisher. Die beiden gemessenen
        // Zahlen der Datei (`FARBE_FELD` = `#767676` und die UA-Regel =
        // `#8f8f9d`) unterscheiden sich; mit einem Rahmen entscheidet die
        // Kaskade, und das ist die Regel, die auch fuer jede andere Farbe
        // gilt. `FARBE_FELD` bleibt fuer den Fall ganz ohne Kaskade
        // (`FormularStil::standard`).
        let rahmenfarbe = if hat_rahmen {
            match ua {
                Some(u) => u.farbe,
                None => {
                    let f = kaskade::rahmen_farbe(&b);
                    Color {
                        r: f.r,
                        g: f.g,
                        b: f.b,
                        a: f.a,
                    }
                }
            }
        } else {
            FARBE_FELD
        };
        FormularStil {
            rand_oben,
            rand_rechts,
            rand_unten,
            rand_links,
            rahmenfarbe,
            // CSS 2.1, 8.3.1: der Hintergrund des Feldes. Ist er
            // durchsichtig, ist das Feld weiss — `FARBE_SEITE`.
            // Release 08.10.2026: der Hintergrund kommt aus der Kaskade, auch
            // `transparent` — das UA-Blatt gibt Feldern ihre Standardflaeche.
            hintergrund: Color {
                r: b.hintergrund.r,
                g: b.hintergrund.g,
                b: b.hintergrund.b,
                a: b.hintergrund.a,
            },
            // HTML_2 (04.10.2026): DIE POLSTERUNG AUS DER UA-EBENE, und
            // die Frage war, ob `autor` oder `mit_ua`.
            //
            // GEMESSENER BEFUND (400 px, `<input type="text" value="MMMM">`):
            //
            // ```text
            //   vorher  INHALT beginnt bei x=9  (Kastenrand 8, also +1 = der Rahmen)
            //           fuellung links/rechts/oben/unten = 0/0/0/0
            //   Firefox INHALT beginnt bei Kastenrand + 3  (1 px Rahmen + 2 px)
            // ```
            //
            // GEMESSENE ENTSCHEIDUNG, warum `mit_ua` und nicht `autor`:
            // `mit_ua` rechnet ueber `k.alle`, also ueber **alle** Blaetter
            // — der Autor ist darin. Eine Autorenangabe schlaegt das
            // UA-Blatt und steht als **ihr** Wert in `mit_ua`; sagt der
            // Autor nichts, steht der UA-Wert dort. Das ist die
            // Kaskadenregel selbst, und deshalb ist es hier keine
            // Entscheidung, sondern eine Leseform.
            //
            // GEMESSENE ENTSCHEIDUNG, warum das beim RAHMEN eine
            // Entscheidung war und hier nicht: `border: 0` heisst „kein
            // Rahmen", `padding: 0` heisst „keine Polsterung". Beides ist
            // eine Angabe, und `mit_ua` traegt sie in beiden Faellen
            // richtig. Der Rahmen brauchte den Ebenen-Vergleich nur, weil
            // `FeldRahmen::von` bei vier Nullen `None` liefert — und das ist
            // eine Aussage ueber die **Rahmenexistenz**, nicht ueber die
            // Farbe.
            //
            // GEMESSENE ENTSCHEIDUNG, warum der Autor auch bei
            // `border: 0` seine Polsterung behaelt: `border: 0` nimmt in
            // CSS 2.1, 8.3 nur den Rahmen, nicht die Polsterung (Gegenprobe
            // `border:0` in `tests/feld_rahmen.rs`).
            fuellung_oben: ua.map_or_else(|| beschaendige(b.fuellung_oben), |u| u.fuellung_oben),
            fuellung_rechts: ua
                .map_or_else(|| beschaendige(b.fuellung_rechts), |u| u.fuellung_rechts),
            fuellung_unten: ua.map_or_else(|| beschaendige(b.fuellung_unten), |u| u.fuellung_unten),
            fuellung_links: ua.map_or_else(|| beschaendige(b.fuellung_links), |u| u.fuellung_links),
            breite_px: b
                .breite
                .filter(|v| v.is_finite() && *v > 0.0 && *v <= 8192.0),
            breite_prozent: b.breite_prozent.filter(|p| p.is_finite() && *p > 0.0 && *p <= 1000.0),
            // Release 08.10.2026: eine PROZENThoehe ist hier unbekannt (sie
            // haengt am Container) und zaehlt wie `auto` — sonst rechnete sie
            // gegen die Fensterhoehe (t-online.de: ein 512 px hoher Knopf).
            hoehe_px: b
                .hoehe
                .filter(|v| v.is_finite() && *v > 0.0 && *v <= 8192.0)
                .filter(|_| b.hoehe_prozent.is_none()),
            schriftgroesse: if b.schriftgroesse_px.is_finite() && b.schriftgroesse_px > 0.0 {
                b.schriftgroesse_px.min(MAX_SCHRIFTGRAD)
            } else {
                GRAD_TEXT
            },
            farbe: {
                let f = b.farbe;
                if f.a != 0 {
                    Color {
                        r: f.r,
                        g: f.g,
                        b: f.b,
                        a: f.a,
                    }
                } else {
                    FARBE_TEXT
                }
            },
            // Release 08.10.2026: der Radius des Autors, wenn er einen nennt
            // (eine Ecke > 0 genuegt). Auf die halbe kleinere Kante begrenzt
            // der Rasterizer; hier nur die Obergrenze gegen Unsinn.
            radius: (b.radius.iter().any(|r| r.is_finite() && *r > 0.0))
                .then(|| b.radius.map(|r| if r.is_finite() { r.clamp(0.0, 4096.0) } else { 0.0 })),
        }
    }

    /// Der Kasten eines Formularfelds. Gibt den **aeusseren** Kasten zurueck.
    ///
    /// # DER AUFBAU, und warum er zwei Knoten hat
    ///
    /// `Style` (narsil-core) hat keine Rahmenfelder (I2). Der Rahmen ist darum
    /// ein Hintergrund des aeusseren Kastens, seine Staerke dessen `padding`,
    /// und der Inhalt sitzt in einem Kindknoten mit der Feldfarbe. Das ist
    /// derselbe Weg wie in `element_innen` — dort ist er gebaut und gemessen.
    ///
    /// # `kaestchen` UND NICHT „hat Inhalt"
    ///
    /// 03.10. 21:2x, KORREKTUR EINES FEHLERS VON 20:2x: `inhalt: None`
    /// bedeutete „quadratisch, 13x13" — das ist der Kaestchen-Zweig. Ein
    /// **leeres** Textfeld (kein `value`, kein `placeholder`) kam ebenfalls
    /// dort an und wurde dadurch **192x192 px**. GEMESSEN (`_m_form_osd.rs`):
    /// `size=192x192` bei `size="20"`. Die Form entscheidet, nicht der Inhalt.
    ///
    /// # DER RUECKGABEWERT
    ///
    /// 03.10. 20:5x: Die Aufrufer holten sich den Feldknoten ueber
    /// `self.doc.len() - 2` — eine Vermutung, die nur stimmt, wenn ein
    /// Textknoten entstanden ist. GEMESSEN: der falsche Knoten hat
    /// `Color { 0,0,0,0 }` und liegt als Eltern eines Kastens in der Seite
    /// (852x386 px). Ein Rueckgabewert ist die Antwort auf die Frage; eine
    /// Rechnung aus der Dokumentlaenge ist eine Vermutung.
    #[allow(clippy::too_many_arguments)]
    fn feld_kasten_mit_inhalt(
        &mut self,
        kaestchen: bool,
        typ: &str,
        breite: f32,
        hoehe: f32,
        st: &FormularStil,
        radius: [f32; 4],
        inhalt: Option<&str>,
    ) -> Option<narsil_dom::NodeId> {
        // M3/M4: ein Feld ist ein Knoten, kein Text — die Grenze ist
        // `MAX_FORMULARFELDER` (`feld_merken`, aufgerufen weiter oben), und
        // hier entstehen hoechstens drei Knoten je Feld.
        let aussen = self
            .doc
            .push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY);
        let Ok(aussen) = aussen else { return None };
        // Release 08.10.2026: der Radius des Autors schlaegt die Vorgabe des
        // Aufrufers (nicht beim Kaestchen, das seine Form selbst hat). Die
        // Flaeche innen bekommt den Radius minus Rahmenbreite — die Kante
        // der Polsterflaeche (CSS Backgrounds 3 §5.3), sonst stuende innen
        // eine Ecke ueber dem runden Rahmen.
        let radius = if kaestchen { radius } else { st.radius.unwrap_or(radius) };
        let rand_max = st.rand_oben.max(st.rand_rechts).max(st.rand_unten).max(st.rand_links);
        let radius_innen = if !kaestchen && st.radius.is_some() {
            radius.map(|r| (r - rand_max).max(0.0))
        } else {
            radius
        };
        let rahmen = Style {
            deckkraft: 1.0,
            radius,
            schatten_anzahl: 0,
            dekoration: 0,
            // DER RAHMEN als Hintergrund (I2). Fuer ein Kaestchen ist das
            // schwarz — Firefox zeichnet es so. Fuer ein Textfeld ist es die
            // Rahmenfarbe der Seite, sonst `FARBE_FELD`.
            // Release 09.10.2026: angehakt in der Akzentfarbe, sonst grau
            // (Chrome); `inhalt == None` heisst beim Kaestchen „angehakt".
            background: if kaestchen && inhalt.is_none() {
                FARBE_AKZENT
            } else if kaestchen {
                st.rahmenfarbe
            } else if st.rand_oben + st.rand_rechts + st.rand_unten + st.rand_links <= 0.0 && st.hintergrund.a == 0 {
                // Release 08.10.2026: ohne Rahmen keine Rahmenflaeche (sonst
                // schimmert sie unter einer durchsichtigen Feldflaeche durch).
                Color::TRANSPARENT
            } else {
                st.rahmenfarbe
            },
            color: st.farbe,
            font_size: st.schriftgroesse,
            // Release 09.10.2026: beim Kaestchen ist `breite` die sichtbare
            // Kante (13 px) — der Rand liegt darin, nicht darum.
            size: if kaestchen {
                Dimension {
                    width: Length::px((breite - st.rand_links - st.rand_rechts).max(1.0)),
                    height: Length::px((hoehe - st.rand_oben - st.rand_unten).max(1.0)),
                }
            } else {
                Dimension {
                    width: Length::px(breite),
                    height: Length::px(hoehe),
                }
            },
            padding: Spacing {
                top: Length::px(st.rand_oben),
                right: Length::px(st.rand_rechts),
                bottom: Length::px(st.rand_unten),
                left: Length::px(st.rand_links),
            },
            // Chrome-UA: Kaestchen `margin: 3px 3px 3px 4px`, Radio
            // `3px 3px 0 5px`.
            margin: if kaestchen && typ == "radio" {
                Spacing {
                    top: Length::px(3.0),
                    right: Length::px(3.0),
                    bottom: Length::px(0.0),
                    left: Length::px(5.0),
                }
            } else if kaestchen {
                Spacing {
                    top: Length::px(3.0),
                    right: Length::px(3.0),
                    bottom: Length::px(3.0),
                    left: Length::px(4.0),
                }
            } else {
                Spacing::ZERO
            },
            ..Style::default()
        };
        // Release 08.10.2026: Prozentbreite bleibt Prozent (GEMESSEN vorher:
        // t-online.de, `<input class="w-full">` wurde 900 px fest — das
        // Suchfeld konnte im Flex nicht schrumpfen und schob den Knopf
        // „SUCHEN" aus dem Bild).
        let prozent = st.breite_prozent.filter(|_| !kaestchen);
        let mut rahmen = rahmen;
        if let Some(p) = prozent {
            rahmen.size.width = Length::Percent(p);
        }
        let _ = self.doc.set_style(aussen, rahmen);
        //
        // 03.10. 23:0x (HTML, Auftrag CSS_10): JEDES ELEMENT MIT KASKADEN-
        // WERTEN BEKOMMT EINEN PLAN-EINTRAG.
        //
        // GEMESSENER BEFUND (`_m_157.rs`, github, 1280 px): **157 Container
        // ohne Plan-Eintrag**, davon 39 mit farbigem Hintergrund und 46 mit
        // `margin`. Die Kaskadenwerte dieser Kaesten stehen am **Knotenstil**
        // (Farbe, Masse, Rand) und damit im Layout — aber `display`, `float`
        // und `position` kommen aus dem PLAN, und ohne Eintrag fehlen sie.
        //
        // GEMESSENE URSACHE fuer die Formularkaesten, und sie ist meine:
        // `feld_kasten_mit_inhalt` legt seine zwei Knoten **an dem Weg vorbei**,
        // der den Plan schreibt — `kind()` schreibt ihn im Element-Zweig, und
        // dieser Zweig kehrt zurueck, ohne dort vorbeizukommen. Die Form
        // felder sind also genau die Luecke, die die Leitung sucht.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `layout_stil` und nicht der
        // Knotenstil: der Plan traegt `Anzeige`, `Schweben` und `Platzierung`
        // — drei Eigenschaften, die es nur dort gibt. Sie fuer ein
        // Formularfeld sind `Block`/`Keins`/`Statisch`, und genau das ist die
        // Antwort auf die Frage der Leitung.
        self.layout_plan.set(
            aussen,
            narsil_layout::LayoutStil {
                anzeige: self.feld_anzeige(),
                // Release 09.10.2026: ein Textbereich rollt seinen Inhalt —
                // im Textfluss steht er darum mit der UNTERKANTE auf der
                // Grundlinie (CSS 2.1, 10.8.1), wie in Chrome. Ein Kaestchen
                // ebenso: sein Haken darf es beim Anhaken nicht verschieben.
                ueberlauf: if typ == "textarea" || kaestchen {
                    narsil_layout::plan::UeberlaufArt::Versteckt
                } else {
                    narsil_layout::plan::UeberlaufArt::Sichtbar
                },
                ..narsil_css::layout_stil(&narsil_css::Berechnet::standard())
            },
        );

        let innen = self
            .doc
            .push_child(aussen, NodeKind::Container, None, Span::EMPTY);
        let Ok(innen) = innen else { return None };
        //
        // GEMESSENER FEHLER, den dieser Test gefunden hat (19:10): der innere
        // Knoten bekam die Masse NICHT und war `9x0` — eine Box der Hoehe 0
        // malt nichts, also sah man durch sie hindurch den schwarzen Rahmen
        // und das Kaestchen war schwarz statt hell. GEMESSEN:
        //     MESSUNG bau4 (28,28 9x0) bg=Some(...)
        //     MESSUNG rect #1 (26,84 13x13) (0,0,0)
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Haken eine kleinere Flaeche MIT
        // Rand bekommt statt eines Innenabstands: unser Raster malt den
        // Hintergrund ueber die ganze Box, auch ueber den Innenabstand
        // (`Style` hat keine Rahmenfelder, I2). Ein Innenabstand wuerde also
        // den Haken vergroessern statt verkleinern — der Rand muss am MASS
        // sitzen, nicht am Abstand.
        // Release 09.10.2026: die Innenflaeche fuellt den Inhalt des
        // aeusseren Kastens (13 − 2 × 1 = 11 px).
        let innen_kante = (breite - st.rand_links - st.rand_rechts).max(1.0);
        //
        // GEMESSENE REGEL, sie gilt fuer beide Faelle: der innere Knoten liegt
        // **im** aeusseren, und der Rand des aeusseren ist dessen `padding`.
        // Die Innenmasse ist also `aussen - rand`, und die Fuellung des CSS
        // (CSS 2.1, 8.3.2) kommt dazu.
        let fuell_x = st.fuellung_links + st.fuellung_rechts;
        let fuell_y = st.fuellung_oben + st.fuellung_unten;
        // Release 08.10.2026: GEMESSENE KORREKTUR der Regel oben fuer
        // Textfelder, Knoepfe und Auswahl: `breite`/`hoehe` des aeusseren
        // Knotens sind sein INHALT (der Rand kommt als `padding` dazu, siehe
        // `ein_kaestchen_bleibt_13_px`: 13 + 2 + 2 = 17). Mit `aussen − rand`
        // blieb rechts und unten ein Streifen von `rand` in Rahmenfarbe
        // stehen — GEMESSEN: 1 px Rahmen links/oben, 3 px rechts/unten an
        // jedem Feld. Die Flaeche fuellt jetzt den Inhalt des aeusseren
        // Knotens ganz; die Aussenmasse bleiben, wie sie waren.
        let (iw, ih) = if kaestchen {
            (innen_kante, innen_kante)
        } else {
            (
                (breite - fuell_x).max(1.0),
                (hoehe - fuell_y).max(1.0),
            )
        };
        // Release 08.10.2026: einzeilige Felder (Text, Knopf, Auswahl) haben
        // ihren Text VERTIKAL MITTIG (wie jeder Browser); `textarea` beginnt
        // oben. GEMESSEN vorher: t-online.de, „Suchbegriff eingeben" klebte
        // am oberen Rand des 48-px-Suchfelds.
        // Nur bei FESTER Hoehe: ohne sie ist das Feld so hoch wie seine Zeile
        // (die gemessenen UA-Innenabstaende bleiben dann unveraendert).
        let mitte = if kaestchen || typ == "textarea" || st.hoehe_px.is_none() {
            0.0
        } else {
            ((ih - ZEILENHOEHE(st.schriftgroesse)) / 2.0).max(0.0)
        };
        let ih = (ih - mitte).max(1.0);
        let flaeche = Style {
            deckkraft: 1.0,
            radius: radius_innen,
            schatten_anzahl: 0,
            dekoration: 0,
            // Release 09.10.2026: das angehakte Kaestchen ist innen blau (mit
            // weissem Haken), der angehakte Radioknopf weiss mit blauem Punkt.
            background: if kaestchen && inhalt.is_none() && typ != "radio" {
                FARBE_AKZENT
            } else if kaestchen && st.hintergrund.a == 0 {
                // Kaestchen zeichnet der Bau selbst: ihre Innenflaeche ist hell.
                FARBE_SEITE
            } else {
                st.hintergrund
            },
            color: st.farbe,
            font_size: st.schriftgroesse,
            size: Dimension {
                width: if prozent.is_some() { Length::Auto } else { Length::px(iw) },
                height: Length::px(ih),
            },
            padding: if kaestchen {
                Spacing::ZERO
            } else {
                Spacing {
                    top: Length::px(st.fuellung_oben + mitte),
                    right: Length::px(st.fuellung_rechts),
                    bottom: Length::px(st.fuellung_unten),
                    left: Length::px(st.fuellung_links),
                }
            },
            margin: Spacing::ZERO,
            ..Style::default()
        };
        let _ = self.doc.set_style(innen, flaeche);
        //
        // 03.10. 23:1x: AUCH DER INNERE KNOTEN.
        //
        // GEMESSENER BEFUND (Test `plan_je_element`): nach dem Eintrag fuer
        // `aussen` blieben auf einer reinen Formularseite **vier** Container
        // ohne Plan: `[NodeId(5), (7), (9), (12)]` — genau die `innen`-Knoten
        // der vier Felder. Sie tragen `size` und `padding` aus der Fuellung
        // und sind damit genauso Elemente mit Kaskadenwerten.
        let angehakt = kaestchen && inhalt.is_none();
        self.layout_plan.set(
            innen,
            narsil_layout::LayoutStil {
                anzeige: narsil_layout::plan::Anzeige::Block,
                ..narsil_css::layout_stil(&narsil_css::Berechnet::standard())
            },
        );
        // Release 09.10.2026: der weisse Haken im blauen Kaestchen bzw. der
        // blaue Punkt im Radioknopf (Chrome), mittig ueber Raender (kein
        // Flex: der Plan einer Seite soll nur zeigen, was ihr CSS sagt).
        // Vorher war „angehakt" ein schwarz gefuelltes Quadrat.
        if angehakt && typ == "radio" {
            if let Ok(punkt) = self.doc.push_child(innen, NodeKind::Container, None, Span::EMPTY) {
                let d = (innen_kante * 0.6).round().max(3.0);
                let rand = ((innen_kante - d) / 2.0).max(0.0);
                let _ = self.doc.set_style(
                    punkt,
                    Style {
                        deckkraft: 1.0,
                        radius: [d / 2.0; 4],
                        background: FARBE_AKZENT,
                        size: Dimension { width: Length::px(d), height: Length::px(d) },
                        margin: Spacing::all(Length::px(rand)),
                        ..Style::default()
                    },
                );
                self.layout_plan.set(
                    punkt,
                    narsil_layout::LayoutStil {
                        anzeige: narsil_layout::plan::Anzeige::Block,
                        ..narsil_css::layout_stil(&narsil_css::Berechnet::standard())
                    },
                );
            }
        } else if angehakt
            && let Ok(haken) = self.doc.push_child(innen, NodeKind::Text, Some(HAKEN_TEXT.to_string()), Span::EMPTY)
        {
            let mut s = self.doc.get(haken).map(|n| n.style.clone()).unwrap_or_default();
            s.font_size = (innen_kante * 0.95).max(6.0);
            s.color = FARBE_SEITE;
            s.schriftgewicht = 700;
            s.zeilenhoehe_px = innen_kante;
            s.margin.left = Length::px((innen_kante * 0.1).round());
            let _ = self.doc.set_style(haken, s);
        }
        //
        // 03.10. 20:2x: DER INHALT IST EIN KINDKNOTEN.
        //
        // GEMESSENER FEHLER, den die erste Fassung dieser Funktion hatte: sie
        // nahm `inhalt` an, benutzte ihn fuer die **Masse** — und schrieb ihn
        // nirgends hin. Ergebnis (Test `die_auswahl_ist_ein_kasten_mit_der_
        // gewaehlten_option`): der Kasten war da, die Option "Frankreich" aber
        // nicht im Dokument. Ein Kasten ohne Inhalt ist die schlimmere
        // Variante des alten Platzhalters: vorher stand wenigstens der Text da.
        //
        // GEMESSENE ENTSCHEIDUNG, warum ein eigener KINDknoten und nicht
        // `self.text(...)`: `text()` gehoert zum **Absatzstrom** und legt einen
        // Zeilenknoten unter dem laufenden Absatz an — nicht unter `innen`.
        // Der Kasten ist aber gerade geoeffnet und sein Inhalt gehoert in ihn,
        // sonst stuende der Text neben dem Feld.
        //
        // M3/M4: ein Textknoten je Feld. Die Grenze ist dieselbe wie fuer die
        // Kaesten oben (`MAX_FORMULARFELDER`, geprueft in `feld_merken`).
        // Release 12.10.2026 (Agent CSS2): ein mehrzeiliger `textarea`-Wert
        // steht Zeile fuer Zeile (je ein Zeilenkasten), wie in jedem Browser.
        // GEMESSEN vorher (Hinweis Agent D): `a\nb` als EIN Lauf auf einer
        // Zeile.
        if typ == "textarea"
            && let Some(text) = inhalt.filter(|t| t.contains('\n'))
        {
            let zeile_h = ZEILENHOEHE(st.schriftgroesse);
            for zeile in text.split('\n').take(MAX_ZEILEN_PRO_ABSATZ) {
                let Ok(z) = self.doc.push_child(innen, NodeKind::Container, None, Span::EMPTY) else { break };
                let _ = self.doc.set_style(
                    z,
                    Style {
                        deckkraft: 1.0,
                        radius: [0.0; 4],
                        schatten_anzahl: 0,
                        dekoration: 0,
                        size: Dimension { width: Length::Auto, height: Length::px(zeile_h) },
                        direction: Direction::Row,
                        ..Style::default()
                    },
                );
                let anzeige = if zeile.is_empty() { "\u{a0}".to_string() } else { zeile.replace('\r', "") };
                if let Ok(text_id) = self.doc.push_child(z, NodeKind::Text, Some(anzeige), Span::EMPTY) {
                    let mut feld_stil = self.doc.get(text_id).map(|n| n.style.clone()).unwrap_or_default();
                    feld_stil.font_size = st.schriftgroesse;
                    feld_stil.color = st.farbe;
                    let _ = self.doc.set_style(text_id, feld_stil);
                }
            }
        } else if let Some(text) = inhalt.filter(|t| !t.trim().is_empty())
            && let Ok(text_id) =
                self.doc
                    .push_child(innen, NodeKind::Text, Some(text.to_string()), Span::EMPTY)
        {
            let mut feld_stil = self
                .doc
                .get(text_id)
                .map(|n| n.style.clone())
                .unwrap_or_default();
            feld_stil.font_size = st.schriftgroesse;
            feld_stil.color = st.farbe;
            let _ = self.doc.set_style(text_id, feld_stil);
        } else if !kaestchen
            && typ != "textarea"
            && let Ok(text_id) = self.doc.push_child(innen, NodeKind::Text, Some(String::new()), Span::EMPTY)
        {
            // Release 09.10.2026: ein LEERES Feld bekommt einen leeren
            // Textknoten — er traegt die Grundlinie (`flow::grundlinie_von`):
            // das leere Feld steht dann wie in Chrome auf der Textlinie,
            // nicht mit der Unterkante darauf.
            let mut feld_stil = self.doc.get(text_id).map(|n| n.style.clone()).unwrap_or_default();
            feld_stil.font_size = st.schriftgroesse;
            feld_stil.color = st.farbe;
            let _ = self.doc.set_style(text_id, feld_stil);
        }
        //
        // W6/W7C R6: die Nummer des Feldes steht am Kasten, damit ein Klick
        // ihn findet. GEMESSENE BEGRUENDUNG fuer „der erste beansprucht":
        // dieselbe Regel gilt beim Abschluss von `kind()` (Zeile 2248) und in
        // `platzhalter` — `or_insert` laesst den ERSTEN gewinnen, und das ist
        // der Absatz bzw. hier der aeussere Kasten. Ein zweiter Weg, der
        // ueberschreibt, wuerde dem Klick die falsche Nummer geben.
        for id in [aussen, innen] {
            let ord = self.laufender_ord.unwrap_or(self.aktualer_ordinal);
            let k = id.index() as u32;
            self.dom_zu_ord.entry(k).or_insert(ord);
        }
        Some(aussen)
    }

    ///
    /// 02.10. 18:30 (HTML): GIBT DEN ERSTEN ERZEUGTEN KNOTEN ZURUECK.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum: ein `<label for="x">` muss wissen, welche
    /// Knoten **sein** Text ist, und der Platzhalter eines `<input>` ist der
    /// Knoten, den ein Klick auf das Feld trifft (GEMESSEN: der Platzhalter
    /// traegt die Nummer des Feldes, nicht die des Absatzes — Kommentar im
    /// `input`-Zweig). Wer den Knoten haben will, braucht ihn zurueck; alle
    /// anderen Aufrufer verwerfen den Wert einfach (Rust erlaubt das in
    /// Anweisungsstellung).
    fn platzhalter(&mut self, text: &str) -> Option<narsil_dom::NodeId> {
        if text.trim().is_empty() {
            return None;
        }
        let stil = Style {
            deckkraft: 1.0,
            radius: [0.0; 4],
            schatten_anzahl: 0,
            dekoration: 0,
            color: FARBE_SCHWACH,
            font_size: GRAD_TEXT,
            margin: Spacing::all(Length::px(4.0)),
            ..Style::default()
        };
        // Umbruch wie Absaetze (lange alt-Texte duerfen nicht herausragen).
        // Derselbe Einzug wie im Absatz (siehe `Bauer::einzug`).
        let nutzbar = (self.breite as f32 - 2.0 * SEITENRAND - self.einzug).max(40.0);
        let zeilen = umbreche(text, GRAD_TEXT, nutzbar);
        let mut erster: Option<narsil_dom::NodeId> = None;
        for zeile in zeilen {
            if zeile.trim().is_empty() {
                continue;
            }
            let id = self
                .doc
                .push_child(self.mutter(), NodeKind::Text, Some(zeile), Span::EMPTY);
            let Ok(id) = id else { return None };
            // 02.10. 18:30 (HTML): DER ERSTE KNOTEN IST DIE ANTWORT (Label).
            //
            // GEMESSENE EINORDNUNG, warum `get_or_insert` und nicht
            // `if erster.is_none() { erster = Some(id) }`: der erste Entwurf
            // benannte die Variable innen neu (`let erster = …`), und damit
            // blieb die aeussere **immer** `None` — der Compiler sagt es als
            // „unused variable: erster", und die Antwort des Platzhalters war
            // consequently `None` statt des ersten Knotens.
            erster.get_or_insert(id);
            // W6: die Zeile gehoert zum Element, das den Absatz gestartet
            // hat (siehe `Bauer::laufender_ord`).
            //
            // W7C R6 (01.10. 23:40): NUR, WENN NOCH KEINER DA IST.
            //
            // GEMESSENER FEHLER, seit 15:20 als `#[ignore]`-Test abgelegt:
            // `<p>vor <a id="l">weg</a> nach</p>` — der Inline-Zweig hatte
            // den Textknoten des Links auf die Nummer des `<a>` gesetzt
            // (damit ein Klick auf den Link den Handler findet), und DIESE
            // Zeile schrieb danach unbedingt die Nummer des Absatzes
            // darueber. `ids["l"]` war 4, in der Bruecke stand 4 nicht, der
            // Handler lief nie — ein Link mit `preventDefault` navigierte
            // trotzdem.
            //
            // GEMESSEN, warum „der erste beansprucht" die richtige Regel
            // ist: sie steht seit 16:55 beim Abschluss von `kind()` (Zeile
            // 2248) und dort wird sie eingehalten. Hier nicht. Eine Regel,
            // die an einer Stelle gilt und an drei nicht, ist kein Zufall,
            // sondern eine Luecke.
            if let Some(ord) = self.laufender_ord {
                let k = id.index() as u32;
                self.dom_zu_ord.entry(k).or_insert(ord);
            }
            // 04.10.2026 (SCHRIFT): `stil.clone()` — `Style` ist nicht
            // mehr `Copy` (Feld `schriftfamilie`) und wird fuer **jede**
            // Zeile des Absatzes gebraucht.
            let _ = self.doc.set_style(id, stil.clone());
        }
        erster
    }

    /// Den laufenden Absatz umbrechen und als Knoten schreiben.
    ///
    /// DAS IST DER UMBRUCH, den die Layout-Engine nicht macht (E1).
    /// `narsil_layout_engine/src/text.rs:231` gibt die Gesamtbreite als
    /// EINE Zeile zurueck; hier zerlegen wir den Absatz vorher in
    /// Zeilen, die in die Breite passen. Die Engine bleibt unberuehrt.
    ///
    /// DIE REIHENFOLGE IST DIE KORREKTUR 30.09.2026. Vorher stand hier
    /// `umbrecke(&absatz.text, …)` — ein einziger Text, ein einziger
    /// Stil, ein einziges Ziel. Damit konnte ein Absatz MIT Link im
    /// Mittelpunkt nicht mehr dargestellt werden, und der Umbruch
    /// unterschied sich je nach Abschnitt nicht.
    ///
    /// GEMESSENE EIGENSCHAFT, die die neue Form haben muss: die Summe
    /// der Zeilen ist wieder der ganze Absatz, nur mit Leerzeichen an
    /// den Abschnittsgrenzen. Es geht kein Zeichen verloren, und der
    /// Umbruch bricht an derselben Stelle wie `umbrecke`.
    /// Keep words and replaced elements in one layout-owned inline flow.
    /// Separate whitespace nodes let layout collapse spaces at line starts.
    fn inline_absatz_schreiben(&mut self, absatz: Absatz) {
        let parent = self.mutter();
        let mut whitespace = false;
        let mut count = 0;
        // Release 08.10.2026: Pseudokaesten im Textfluss an ihre Stelle
        // zwischen den Woertern (siehe `Absatz::anker`).
        let anker = absatz.anker;
        let mut anker_i = 0usize;
        for (nr, section) in absatz.abschnitte.into_iter().enumerate() {
            while let Some(&(k, knoten, _)) = anker.get(anker_i)
                && k <= nr
            {
                anker_einhaengen(&mut self.doc, parent, knoten);
                anker_i += 1;
            }
            // Release 09.10.2026: ein Label-Text ueber mehrere Woerter
            // („Ihr Name") ist als GANZES verzeichnet — jedes Wort des
            // Abschnitts fragt darum auch mit dem ganzen Abschnittstext.
            let ganz = section.text.trim().to_string();
            // Release 11.10.2026: `white-space: nowrap` — der Abschnitt bleibt
            // EIN Wortknoten (Leerraum innen zu einem Leerzeichen), sonst
            // bricht die Layout-Engine zwischen den Woertern um. GEMESSEN
            // vorher (spiegel.de, Teaser mit Symbol): „Vor 35" / „Min".
            let stuecke: Vec<String> = if section.stil.weissraum == narsil_core::style::Weissraum::KeinUmbruch {
                let innen = section.text.split(css_leerraum).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ");
                let mut v = Vec::new();
                if section.text.starts_with(css_leerraum) {
                    v.push(" ".to_string());
                }
                if !innen.is_empty() {
                    v.push(if section.text.ends_with(css_leerraum) { format!("{innen} ") } else { innen });
                }
                v
            } else {
                section.text.split_inclusive(css_leerraum).map(str::to_string).collect()
            };
            for chunk in stuecke.iter().map(String::as_str) {
                let word = chunk.trim_matches(css_leerraum);
                let is_space = word.is_empty();
                if !is_space {
                    if count >= MAX_WOERTER_PRO_ABSATZ { return; }
                    count += 1;
                }
                for text in [(!is_space).then_some(word), chunk.chars().last().filter(|c| css_leerraum(*c)).map(|_| " ")] .into_iter().flatten() {
                    let space = text == " ";
                    if space && whitespace { continue; }
                    whitespace = space;
                    let mut style = section.stil.clone();
                    style.size = Dimension {width: Length::Auto, height: Length::px(absatz.zeilenhoehe.unwrap_or_else(|| ZEILENHOEHE(style.font_size)))};
                    style.margin = Spacing::ZERO;
                    style.padding = Spacing::ZERO;
                    style.background = Color::TRANSPARENT;
                    style.rahmen = Default::default();
                    style.radius = [0.0; 4];
                    style.schatten_anzahl = 0;
                    let Ok(id) = self.doc.push_child(parent, NodeKind::Text, Some(text.to_string()), Span::EMPTY) else { return; };
                    let _ = self.doc.set_style(id, style);
                    // Release 09.10.2026: ein Leerraum-Knoten traegt keine
                    // Nummer — er wird am Zeilenrand weggelassen (kein
                    // Kasten), und wer ueber die Nummer den Kasten eines
                    // Elements sucht (Knopf, Feld), fand sonst ihn.
                    if !space && let Some(ord) = section.ord.or(absatz.ord) { self.dom_zu_ord.insert(id.index() as u32, ord); }
                    if !space {
                        if let Some(href) = &section.href { self.links.insert(id, href.clone()); }
                        if let Some(name) = self.aktuelle_id.clone() { self.vermerke_id(name, id); }
                        if ganz != text {
                            self.label_regel_anwenden(parent, &ganz, id);
                        }
                        self.label_regel_anwenden(parent, text, id);
                    }
                }
            }
        }
        for &(_, knoten, _) in anker.iter().skip(anker_i) {
            anker_einhaengen(&mut self.doc, parent, knoten);
        }
    }

    /// Release 13.10.2026 (Agent CSS2): hinter dem Klemmpunkt steht noch
    /// Inhalt — die letzte Zeile endet mit der Auslassung (`block-ellipsis:
    /// auto`, CSS Overflow 4 §4.3), einmal je Klemme.
    fn klemme_auslassen(&mut self) {
        let Some(k) = self.klemmen.last_mut() else {
            return;
        };
        if k.ellipse {
            return;
        }
        k.ellipse = true;
        if let Some(id) = k.letzter_text {
            let prepend = k.prepend;
            append_clamp_ellipsis(&mut self.doc, &mut self.absaetze, id, prepend, true);
        }
    }

    fn absatz_schliessen(&mut self) {
        //
        let inline_bilder = self.inline_bild_eltern.contains(&self.mutter());
        let Some(absatz) = self.laufender.take() else {
            return;
        };
        // W6E: die Nummer des Absatzes, nicht die des Bauers (16:55).
        // GEMESSEN: `self.laufender_ord` stand hier schon auf dem
        // ELEMENT, DAS ALS NAECHSTES kam.
        let absatz_ord = absatz.ord;
        // GEMESSENER FEHLER, den diese Zeile zuerst hatte (16:55): sie
        // stand als `std::mem::replace(&mut self.laufender_ord, absatz_ord)`
        // — und `mem::replace` gibt den ALTEN Wert zurueck. Die Knoten
        // bekamen also genau die Nummer, die gerade ersetzt werden sollte.
        let laufender_ord = absatz_ord;
        self.laufender_ord = absatz_ord;
        if inline_bilder {
            self.inline_absatz_schreiben(absatz);
            return;
        }
        // Release 11.10.2026 (Agent CSS2): nur CSS-Leerraum (CSS 2.1 §16.6.1:
        // Leerzeichen, Tab, Zeilenvorschub, CR, FF) macht einen Absatz leer —
        // `str::trim` entfernt auch das geschuetzte Leerzeichen U+00A0.
        // GEMESSEN vorher (WPT `line-height-applies-to-*`, Zellen mit
        // `&nbsp;`): `<div>&nbsp;</div>` war 0 px hoch statt einer Zeile.
        if absatz.text().chars().all(css_leerraum) {
            return;
        }
        // Release 13.10.2026 (Agent CSS2): hinter einer vollen Zeilenklemme
        // kommt kein Absatz mehr (siehe `Klemme`).
        if self.klemmen.last().is_some_and(|k| k.rest == 0) {
            self.klemme_auslassen();
            return;
        }
        // Release 15.10.2026 (Agent CSS2): this paragraph's first line is the
        // first formatted line of every waiting `::first-line` block; the
        // innermost order's colour applies (CSS Pseudo-Elements 4 §2.2).
        let erste_zeile_farbe = self.erste_zeile_farbe();
        // Release 08.10.2026: `NARSIL_DBG_ABSATZ=<teiltext>` zeigt die
        // geschaetzte Breite, mit der der Bauer einen Absatz umbricht.
        if let Some(such) = dbg_absatz()
            && absatz.text().contains(such.as_str())
        {
            eprintln!("ABSATZ breite={} textbreite={} einzug={} text={:?}", absatz.breite, self.textbreite, self.einzug, absatz.text().chars().take(60).collect::<String>());
        }

        // (Stil, Ziel) je Abschnitt. Klein und fest: die Zahl der
        // Abschnitte ist durch `MAX_ABSCHNITTE` begrenzt.
        //
        // W7C R6: die DREIte Zahl ist die Nummer des Inline-Elements, aus
        // dem der Abschnitt stammt (`None` = der Abschnitt ist der des
        // Absatzes). Sie entscheidet, welche Nummer der Textknoten
        // bekommt — und damit, ob ein Klick auf einen Link seinen
        // Handler findet.
        let abschnitte: Vec<(Style, Option<String>, Option<usize>)> = absatz
            .abschnitte
            .iter()
            // 04.10.2026 (SCHRIFT): `a.stil.clone()` — siehe `Style`
            // in narsil-core (Feld `schriftfamilie`, kein `Copy`).
            .map(|a| (a.stil.clone(), a.href.clone(), a.ord))
            .collect();

        // ---- 1. Der Wortstrom: Wort, Abschnitt, Leerzeichen davor.
        //
        // M3: `MAX_WOERTER_PRO_ABSATZ`. GEMESSEN, warum die Grenze da
        // sein muss: `<p>` mit 10 MB Buchstabensalat ergibt 10 Millionen
        // Woerter, jedes ein eigener `String`. Das ist der Speicher, den
        // ein Parser frisst, und der Leser sieht davon nichts.
        //
        // **DAS LEERZEICHEN IST EINE EIGENE ANGABE**, und das ist die
        // Korrektur vom 30.09.2026, 05:45 (Bildschirmfoto der
        // Koordination, `test2.html`): `Das ist ein <b>ganz
        // normaler</b> Absatz` wurde zu `einganz normalerAbsatz`.
        //
        // `split_whitespace()` wirft den Leerraum am Rand eines
        // Abschnitts weg — und ein Abschnitt endet genau dort, wo ein
        // Inline-Element beginnt. CSS 2.1, 16.6.1: Weissraum wird
        // zusammengefasst, aber ein Leerzeichen an einer INLINE-Grenze
        // bleibt ein Leerzeichen. Der Rasterizer hat keine Absaetze, nur
        // Woerter; also muss die Information am Wort hängen.
        //
        // `leer_davor` heisst: zwischen dem vorherigen Wort und diesem
        // stand in der Quelle Weissraum. Genau dann wird ein Leerzeichen
        // gesetzt — und genau dann nicht, wenn die Quelle keins hatte
        // (`<b>eins</b><i>zwei</i>` bleibt "einszwei", wie im Browser).
        let mut woerter: Vec<Wort> = Vec::new();
        // Am Absatzanfang gilt: Leerzeichen werden entfernt (CSS 16.6.1).
        //
        // DER FEHLER, den dieser Zaehler hatte (30.09.2026, gemessen):
        // `split_whitespace()` gibt nur die Woerter zurueck, nicht die
        // Information, ob davor Weissraum stand. Die Flagge war deshalb
        // nach dem ersten Wort IMMER wahr — auch dort, wo in der Quelle
        // keins stand. Ergebnis: `</a><a>` bekam ein Leerzeichen
        // ("Start Blog Kontakt" statt "StartBlogKontakt"), und `<a>Link
        // </a>. Er` bekam eines vor dem Punkt.
        //
        // `leer_davor` ist der ZUSTAND ueber die Abschnittsgrenze hinweg:
        // endete der vorherige Abschnitt mit Weissraum, dann beginnt der
        // naechste mit einem Leerzeichen. Innerhalb eines Abschnitts
        // entscheidet der eigene Text.
        //
        // GEMESSENER FEHLER 30.09.2026 (zwei Fehler in Folge, beide im
        // Bild): (a) die Flagge war nach dem ersten Wort immer wahr —
        // "Das ist ein" wurde zu "Dasist ein". (b) Sie wurde am Wort
        // weitergereicht statt am Abschnitt — "Start Blog Kontakt"
        // bekam Leerzeichen, wo `<a>…</a><a>…</a>` keine hat.
        let mut abschnitt_endet_leer = false;
        // Release 12.10.2026 (Agent CSS2): siehe `AbsatzDaten::leer_eigen`.
        let mut leer_eigen = vec![false; absatz.abschnitte.len()];
        // Release 11.10.2026: `nowrap` und letztes Zeichen des vorigen
        // Abschnitts (fuer `Wort::bruch_davor`).
        // Ob der Leerraum seit dem letzten Wort (auch aus Abschnitten nur aus
        // Leerraum dazwischen) irgendwo umbrechen durfte — `<span nowrap>X
        // </span> <span nowrap>X </span>` bricht am Leerzeichen dazwischen.
        let mut leer_bricht = false;
        let mut vorher_ueberall = false;
        let mut vorher_zeichen: Option<char> = None;
        // Release 09.10.2026: horizontale Raender nur fuer INLINE-Elemente
        // (`ord` gesetzt) und nur an ihren Enden: links am ersten Wort des
        // ersten Abschnitts des Elements, rechts am letzten Wort des letzten.
        // GEMESSEN vorher: jedes Wort jedes Abschnitts bekam die Raender —
        // nach einem `<b>` sogar die des ABSATZES (`<p style="margin:0 32px
        // 0 34px">`: +66 px je Wort), und die Hilfezeile der Startseite brach
        // bei 1334 px Platz nach 600 px um.
        let erster_abschnitt = |ord: usize| absatz.abschnitte.iter().position(|a| a.ord == Some(ord));
        let letzter_abschnitt = |ord: usize| absatz.abschnitte.iter().rposition(|a| a.ord == Some(ord));
        'woerter: for (i, abschnitt) in absatz.abschnitte.iter().enumerate() {
            let (rl, rr) = match abschnitt.ord {
                Some(o) => {
                    let (l, r) = abschnitt.stil.eigenschaft();
                    (
                        if erster_abschnitt(o) == Some(i) { l } else { 0.0 },
                        if letzter_abschnitt(o) == Some(i) { r } else { 0.0 },
                    )
                }
                None => (0.0, 0.0),
            };
            let (liste, endet_leer) = woerter_mit_leerzeichen(&abschnitt.text, &abschnitt.stil);
            let anzahl = liste.len();
            for (n, (wort, eigenes_leer)) in liste.into_iter().enumerate() {
                if woerter.len() >= MAX_WOERTER_PRO_ABSATZ {
                    break 'woerter;
                }
                // Am Absatzanfang wird jedes Leerzeichen entfernt
                // (CSS 16.6.1), an einer Abschnittsgrenze nur das, das
                // wirklich dasteht.
                let vor = if woerter.is_empty() {
                    false
                } else if n == 0 {
                    // An der Abschnittsgrenze zaehlt BEIDES: das
                    // Leerzeichen am Ende des vorherigen Abschnitts UND
                    // das am Anfang dieses. Sonst verliert eins von
                    // beiden seinen Platz — gemessen: "Das ist ein xmehr".
                    eigenes_leer || abschnitt_endet_leer
                } else {
                    eigenes_leer
                };
                let ohne_umbruch = abschnitt.stil.weissraum == narsil_core::style::Weissraum::KeinUmbruch;
                let bruch_davor = if n > 0 {
                    // Im Abschnitt: am Leerzeichen oder zwischen CJK-Zeichen
                    // (so hat `woerter_mit_leerzeichen` geteilt).
                    true
                } else if eigenes_leer {
                    !ohne_umbruch || (abschnitt_endet_leer && leer_bricht)
                } else if abschnitt_endet_leer {
                    leer_bricht
                } else if abschnitt.stil.umbruch_ueberall || vorher_ueberall {
                    true
                } else {
                    match (vorher_zeichen, wort.chars().next()) {
                        (Some(v), Some(c)) => darf_umbrechen(v, c, &abschnitt.stil),
                        _ => true,
                    }
                };
                vorher_zeichen = wort.chars().last();
                if n == 0 && vor && eigenes_leer && !abschnitt_endet_leer
                    && let Some(e) = leer_eigen.get_mut(i)
                {
                    *e = true;
                }
                woerter.push(Wort {
                    text: wort,
                    abschnitt: i,
                    leer_davor: vor,
                    rand_links: if n == 0 { rl } else { 0.0 },
                    rand_rechts: if n + 1 == anzahl { rr } else { 0.0 },
                    bruch_davor,
                    haengt: abschnitt.stil.weissraum == narsil_core::style::Weissraum::PreUmbruch,
                    other_spaces_hang: abschnitt.stil.weissraum != narsil_core::style::Weissraum::UmbruchLeer,
                    unteilbar: absatz.beschnitten
                        || matches!(
                            abschnitt.stil.weissraum,
                            narsil_core::style::Weissraum::KeinUmbruch | narsil_core::style::Weissraum::Pre
                        ),
                });
            }
            let ohne_umbruch = abschnitt.stil.weissraum == narsil_core::style::Weissraum::KeinUmbruch;
            if anzahl > 0 {
                leer_bricht = endet_leer && (!ohne_umbruch || abschnitt.leer_bricht);
            } else if endet_leer {
                leer_bricht |= !ohne_umbruch || abschnitt.leer_bricht;
            }
            abschnitt_endet_leer = endet_leer;
            vorher_ueberall = abschnitt.stil.umbruch_ueberall;
            if endet_leer {
                vorher_zeichen = None;
            }
        }
        if woerter.is_empty() {
            return;
        }

        // ---- 2. Umbruch in Zeilen, die in die Breite passen.
        //
        // GEMESSENER FEHLER 30.09.2026, gefunden von `tests/textfluss.rs`:
        // ein `<blockquote>` hat 24 px Einrueckung links. Die Umbruch-
        // breite war `breite - 2*SEITENRAND`, also ohne die Einrueckung
        // — und die Zeile begann bei x = 40 und reichte bis 488 px auf
        // einer 480-px-Seite. Sie ragte also mit 8 px heraus.
        //
        // Die Einrueckung gehoert in die Rechnung, weil sie den TEXT
        // einrueckt, nicht die Box: die Breite, die zur Verfuegung
        // steht, ist um beide Maene kleiner.
        // Captured at paragraph start, before descendants or siblings can
        // change the containing block. Includes its width/max-width/padding.
        let nutzbar = absatz.breite;
        //
        // ---- 3. Zeilen schreiben (Release 07.10.2026: eine Funktion fuer den
        // Bau und den Nachumbruch, `zeilen_schreiben`). Die Messung folgt der
        // Schrift, mit der gezeichnet wird (W5E/W5G, SCHRIFT_9): Schnitt und
        // Familie kommen aus dem Stil des Abschnitts.
        // Release 12.10.2026 (Agent CSS2): `hanging-punctuation: first` (CSS
        // Text 4 §9.2) — eine oeffnende Klammer oder ein Anfuehrungszeichen am
        // Anfang der ersten Zeile steht AUSSERHALB der Zeile (wie ein
        // negativer Einzug um seine Breite); nicht hinter Rand, Innenabstand
        // oder Rahmen eines Inline-Elements. GEMESSEN vorher (WPT
        // `css-text/hanging-punctuation/hanging-punctuation-first*`): die
        // Angabe wirkte nicht.
        // Release 13.10.2026 (Agent CSS2): kein Haengen hinter Rand, Rahmen oder
        // Innenabstand auf der STARTseite — in `rtl` die rechte. GEMESSEN
        // vorher (WPT `hanging-punctuation-first-rtl`, mit Blocksatz sichtbar):
        // `<span style="border-right">(</span>` hing trotzdem.
        let haengend = if absatz.haengt {
            woerter
                .first()
                .filter(|w| if absatz.rtl { w.rand_rechts == 0.0 } else { w.rand_links == 0.0 })
                .and_then(|w| {
                    let c = w.text.chars().next().filter(|c| haengende_satzzeichen(*c))?;
                    let a = abschnitte.get(w.abschnitt)?;
                    let b = self.messung.breite_stil_familie(
                        &c.to_string(),
                        a.0.font_size,
                        a.0.schriftgewicht,
                        a.0.kursiv,
                        &a.0.schriftfamilie,
                    );
                    b.is_finite().then_some(b.max(0.0))
                })
                .unwrap_or(0.0)
        } else {
            0.0
        };
        // Release 15.10.2026 (Agent CSS2): see `Style::min_content_floor` —
        // the widest word of a `break-word` section, with its spacing and
        // without hanging spaces. MEASURED before (WPT css-text `overflow-
        // wrap/overflow-wrap-min-content-size-004`, a `max-width: 0` table):
        // the cell shrank to the pieces of the split word.
        let mut abschnitte = abschnitte;
        for (i, a) in abschnitte.iter_mut().enumerate() {
            if !a.0.break_word_only {
                continue;
            }
            let ok = |v: f32| if v.is_finite() { v } else { 0.0 };
            let (ls, ws) = (ok(a.0.buchstaben_abstand), ok(a.0.wort_abstand));
            let hanging = a.0.weissraum == narsil_core::style::Weissraum::PreUmbruch;
            let floor = woerter
                .iter()
                .filter(|w| w.abschnitt == i)
                .map(|w| {
                    let t = ohne_nullbreite(&w.text);
                    let t = if hanging { t.trim_end_matches('\u{a0}') } else { &t[..] };
                    let n = t.chars().count() as f32;
                    let spacing = ls * (n - 1.0).max(0.0) + ws * t.chars().filter(|c| *c == '\u{a0}').count() as f32;
                    self.messung.breite_stil_familie(t, a.0.font_size, a.0.schriftgewicht, a.0.kursiv, &a.0.schriftfamilie)
                        + spacing.max(0.0)
                })
                .fold(0.0f32, f32::max);
            a.0.min_content_floor = if floor.is_finite() { floor.min(MAX_WORTBREITE_KLEMME) } else { 0.0 };
        }
        let daten = AbsatzDaten {
            max_zeilen: match (self.klemmen.last().map(|k| k.rest), absatz.max_zeilen) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
            ohne_auslassung: self.klemmen.last().is_some_and(|k| !k.mit_auslassung),
            woerter,
            abschnitte,
            stil: absatz.stil.clone(),
            zeilenhoehe: absatz.zeilenhoehe,
            ausrichtung: absatz.ausrichtung,
            ausrichtung_letzte: absatz.ausrichtung_letzte,
            blocksatz_aus: absatz.blocksatz_aus,
            rtl: absatz.rtl,
            bidi_mode: absatz.bidi_mode,
            auslassung: absatz.auslassung.clone(),
            ord: laufender_ord,
            anker: absatz.anker.clone(),
            einzug: absatz.einzug - haengend,
            leer_eigen,
            clamp_ellipsis: false,
            first_line_color: erste_zeile_farbe,
            balance: absatz.balance,
        };
        // Release 13.10.2026 (Agent CSS2): mit `text-overflow` bleibt die
        // Zeile so breit wie der Block — die Auslassung misst gegen seine
        // Kante, und die Woerter darin sind unteilbar (`Wort::unteilbar`).
        let nutzbar = if daten.auslassung.is_some() {
            nutzbar
        } else {
            nutzbar.max(breitestes_wort(&daten, &self.messung, absatz.shrinks))
        };
        let breiten = |_: usize| (0.0f32, nutzbar);
        let zeilen = absatz_zeilen(&daten, &self.messung, &breiten);
        if zeilen.is_empty() {
            return;
        }
        let absatz_knoten = self.mutter();
        let mut ziel = ZeilenZiel {
            doc: &mut self.doc,
            links: &mut self.links,
            dom_zu_ord: &mut self.dom_zu_ord,
            layout_plan: &mut self.layout_plan,
        };
        let (boxen, texte) = zeilen_schreiben(&mut ziel, absatz_knoten, &daten, &zeilen, &breiten, &self.messung);
        for (id, text) in &texte {
            self.label_regel_anwenden(absatz_knoten, text, *id);
            if let Some(n) = self.aktuelle_id.clone() {
                self.vermerke_id(n, *id);
            }
        }
        // Release 13.10.2026 (Agent CSS2): die Zeilen zaehlen gegen die Klemme.
        if let Some(k) = self.klemmen.last_mut() {
            k.rest = k.rest.saturating_sub(boxen.len());
            // Release 14.10.2026 (Agent CSS2): in an RTL paragraph the
            // logical end of the last line is its first (leftmost) text node.
            // MEASURED before (WPT css-overflow `line-clamp/webkit-line-clamp-
            // 014`, `direction: rtl`): the ellipsis stood right of "Line 2".
            let last_line_first = boxen.last().and_then(|z| {
                self.doc.get(*z).and_then(|n| {
                    n.children
                        .iter()
                        .copied()
                        .find(|c| self.doc.get(*c).is_some_and(|k| k.kind == NodeKind::Text))
                })
            });
            if daten.rtl
                && let Some(id) = last_line_first
                && let Some(text) = self.doc.get(id).and_then(|n| n.text.clone())
            {
                k.letzter_text = Some(id);
                k.prepend = true;
                if k.rest == 0 && text.starts_with('…') {
                    k.ellipse = true;
                }
            } else if let Some((id, text)) = texte.last() {
                k.letzter_text = Some(*id);
                k.prepend = false;
                if k.rest == 0 && text.ends_with('…') {
                    k.ellipse = true;
                }
            }
        }
        if !boxen.is_empty() && self.absaetze.len() < MAX_ABSAETZE_MERKEN {
            let basis = nutzbar.max(1.0);
            self.absaetze.push(AbsatzSatz {
                knoten: absatz_knoten,
                zeilen: boxen,
                breite: nutzbar,
                einzug_links: spanne_links(&daten.stil.margin) + daten.stil.padding.left.resolve_or(basis, 0.0),
                einzug_rechts: spanne_rechts(&daten.stil.margin) + daten.stil.padding.right.resolve_or(basis, 0.0),
                daten: std::sync::Arc::new(daten),
            });
        }
    }

    fn stil_fuer(&self, tag: &str, attributes: &HashMap<String, String>, link: bool) -> Style {
        let mut stil = Style {
            deckkraft: 1.0,
            radius: [0.0; 4],
            schatten_anzahl: 0,
            dekoration: 0,
            color: FARBE_TEXT,
            font_size: self.grad_fuer_tag(tag),
            ..Style::default()
        };

        // Ueberschriften: fett (heller), eigener Grad, eigener Abstand.
        if let Some(ebene) = ueberschrift_ebene(tag) {
            stil.font_size = grad_ueberschrift(ebene);
            stil.color = FARBE_TEXT;
            stil.margin = Spacing::all(Length::px(ABSATZABSTAND));
        }
        // Fett: hell, weil es keine fette Schrift gibt.
        if tag == "b" || tag == "strong" {
            stil.color = Color::WHITE;
        }
        // Code: etwas kleiner und schwaecher, das ist der einzige
        // Unterschied, den man ohne zweite Schrift hinbekommt.
        if tag == "code" || tag == "tt" {
            stil.font_size = GRAD_TEXT * 0.9;
            stil.color = FARBE_SCHWACH;
        }
        // Listen: mit Punkt, damit es als Liste erkennbar ist. Punkt ist
        // ein Zeichen, kein Element — ein `ul` mit 3 `li` ergibt
        // 3 Geschwister mit je einem Punkt davor.
        //
        // GEMESSENER FEHLER 30.09.2026 (Koordinationsbefund): der dritte
        // Listenpunkt hatte einen anderen Abstand als die ersten zwei.
        // Grund war der DOPPELTE Abstand: `li` bekam hier 4 px, und
        // weiter unten (`ist_block` im Container-Zweig) noch einmal
        // `ABSATZABSTAND`. Jetzt gibt es EINE Quelle: der Listenpunkt
        // traegt seinen Abstand, und `li` ist wie jeder andere Absatz.
        if tag == "li" {
            stil.margin = Spacing {
                left: Length::px(LISTENEINRUECKUNG),
                top: Length::px(0.0),
                right: Length::px(0.0),
                bottom: Length::px(0.0),
            };
        }
        // Zitat: eingerueckt, damit es als Zitat erkennbar ist.
        //
        // GEMESSENER FEHLER 30.09.2026 (Koordinationsbefund):
        // "<blockquote> ist kaum eingerueckt" — es war gar nicht
        // eingerueckt, `blockquote` hatte denselben Abstand wie jeder
        // andere Absatz. Die Einrueckung ist LINKS, nicht oben und
        // unten: ein Zitat ist ein Absatz, der seitlich steht.
        if tag == "blockquote" {
            stil.margin = Spacing {
                left: Length::px(ZITAT_EINRUECKUNG),
                top: Length::px(ABSATZABSTAND),
                right: Length::px(0.0),
                bottom: Length::px(ABSATZABSTAND),
            };
        }
        if link {
            stil.color = FARBE_LINK;
        }

        // `style="..."`: nur die sieben erlaubten Eigenschaften.
        if let Some(inline) = attributes.get("style") {
            self.wende_inline_an(&mut stil, inline);
        }
        stil
    }

    /// Nimmt einen `<style>`-Block als Quelle auf. Er wird NICHT gezeigt.
    ///
    /// M3/M4: `MAX_STILBLOECKE` Bloecke, `MAX_STILTEXT` Byte je Block
    /// (beide in `skripte.rs`, VOR der Uebernahme geprueft).
    fn stile_aufnahme(&mut self, attributes: &HashMap<String, String>, kinder: &[HtmlNode]) {
        if self.stile.len() >= MAX_STILBLOECKE {
            return;
        }
        let mut inhalt = String::new();
        for k in kinder {
            if let HtmlNode::Text(t) = k {
                if !inhalt.is_empty() && !inhalt.ends_with('\n') {
                    inhalt.push('\n');
                }
                inhalt.push_str(t);
            }
        }
        if inhalt.trim().is_empty() {
            // `<style></style>` ist kein Stylesheet, sondern ein
            // leeres. Der Kaskade einen leeren Block zu geben wuerde
            // ihre Grenzen (MAX_BLAETTER) verbrauchen.
            return;
        }
        // J-CACHE (05.10.2026): DEDUPE.
        //
        // GEMESSEN: `stile_sammeln_in` (vor dem Bau) UND diese Funktion
        // (im Bau) sammeln BEIDE. Solange der Kopf uebersprungen wurde,
        // traf nur der erste Weg die Kopf-`<style>`; seit der Kopf besucht
        // wird, lag jeder Kopf-Block DOPPELT in `Seite::stile`
        // (`l6_link_blaetter::ein_style_block_bleib_unberuehrt`: 2 statt 1,
        // gemessen mit `display_probe`: `stile=2`). Gleicher Text ist
        // dasselbe Blatt — die Kaskade wuerde dieselben Regeln zweimal
        // rechnen, und `Vary`/Reihenfolge-Tests saehen ein Blatt zu viel.
        if self.stile.iter().any(|b| b.inhalt == inhalt) {
            return;
        }
        self.stile.push(stile_aus(attributes, &inhalt));
    }

    /// Nimmt ein `<script>` auf: Knoten anlegen (0x0), Merkmale sammeln.
    ///
    /// M3/M4: `MAX_SKRIPTE` Skripte, `MAX_SKRIPT_INHALT` Byte Quelltext
    /// (beides in `skripte.rs` begrenzt, VOR der Uebernahme).
    /// Release 10.10.2026: die `<script>` eines NICHT gebauten Teilbaums
    /// (`display: none`) — in Dokumentreihenfolge aufgenommen wie im Bau.
    fn skripte_im_verborgenen(&mut self, knoten: &[HtmlNode], tiefe: usize) {
        if tiefe > 256 {
            return;
        }
        for k in knoten {
            if let HtmlNode::Element { tag, attributes, children, .. } = k {
                if tag.eq_ignore_ascii_case("script") {
                    self.skript_aufnahme(attributes, children);
                } else if !tag.eq_ignore_ascii_case("template") {
                    self.skripte_im_verborgenen(children, tiefe + 1);
                }
            }
        }
    }

    /// Release 14.10.2026: the `<script>` elements inside an `<svg>` (the
    /// SVG itself is built as one replaced element, so the build never visits
    /// them). In foreign content a script's children are markup, and the
    /// parser runs a nested script at ITS end tag, i.e. before the outer one
    /// (HTML §13.2.6.5 „any other end tag" for an SVG `script`); the source
    /// is the child text content. An SVG script names its external source
    /// with `href` (SVG 2) or `xlink:href`. MEASURED before (WPT scripting-1
    /// execution-timing 131, 138–146): no SVG script ever ran.
    fn skripte_im_svg(&mut self, knoten: &[HtmlNode], tiefe: usize) {
        if tiefe > 256 {
            return;
        }
        for k in knoten {
            if let HtmlNode::Element { tag, attributes, children, .. } = k {
                if tag.eq_ignore_ascii_case("template") {
                    continue;
                }
                self.skripte_im_svg(children, tiefe + 1);
                if tag.eq_ignore_ascii_case("script") {
                    // An SVG script ignores `src`; its source is `href`.
                    {
                        let quelle = attributes
                            .iter()
                            .find(|(n, _)| n.eq_ignore_ascii_case("href") || n.eq_ignore_ascii_case("xlink:href"))
                            .map(|(_, v)| v.clone());
                        match quelle {
                            // An empty `href` is a failed external script —
                            // the inline text never runs (WPT
                            // execution-timing 137).
                            Some(v) if v.trim().is_empty() => {}
                            Some(v) => {
                                let mut mit_src = attributes.clone();
                                mit_src.insert("src".to_string(), v);
                                self.skript_aufnahme(&mit_src, children);
                            }
                            None if attributes.contains_key("src") => {
                                let mut ohne_src = attributes.clone();
                                ohne_src.remove("src");
                                self.skript_aufnahme(&ohne_src, children);
                            }
                            None => self.skript_aufnahme(attributes, children),
                        }
                    }
                }
            }
        }
    }

    /// Release 15.10.2026: collects the scripts of content that is not
    /// built (canvas fallback content), in document order.
    fn skripte_im_ersatzinhalt(&mut self, kinder: &[HtmlNode], tiefe: usize) {
        for k in kinder {
            if let HtmlNode::Element { tag, attributes, children, .. } = k {
                if tag.eq_ignore_ascii_case("script") {
                    self.skript_aufnahme(attributes, children);
                } else if tiefe + 1 < MAX_BAU_TIEFE {
                    self.skripte_im_ersatzinhalt(children, tiefe + 1);
                }
            }
        }
    }

    fn skript_aufnahme(&mut self, attributes: &HashMap<String, String>, kinder: &[HtmlNode]) {
        // Der Inhalt: der ROHE Text des script-Knotens. Er ist genau ein
        // Kind, und es ist Text — `narsil-fmt-html` garantiert das
        // (Test `script_inhalt_wird_gehalten_und_style_bleibt_leer`).
        let mut inhalt = String::new();
        for k in kinder {
            if let HtmlNode::Text(t) = k {
                if !inhalt.is_empty() && !inhalt.ends_with('\n') {
                    inhalt.push('\n');
                }
                inhalt.push_str(t);
            }
        }
        // M3: die Zahl der Skripte ist begrenzt, BEVOR hier ueberhaupt
        // etwas kopiert wird.
        if self.skripte.len() >= MAX_SKRIPTE {
            return;
        }
        let mut skript = skript_aus(attributes, &inhalt, self.basis);
        // Der Knoten: 0x0, durchsichtig, ohne Kinder. Er belegt nichts und
        // zeigt nichts — er existiert, damit die Seite den Platz nicht
        // verliert.
        let stil = Style {
            deckkraft: 1.0,
            radius: [0.0; 4],
            schatten_anzahl: 0,
            dekoration: 0,
            size: Dimension {
                width: Length::px(0.0),
                height: Length::px(0.0),
            },
            background: Color::TRANSPARENT,
            ..Style::default()
        };
        if let Ok(id) = self
            .doc
            .push_child(self.mutter(), NodeKind::Container, None, Span::EMPTY)
        {
            let _ = self.doc.set_style(id, stil);
            skript.knoten = id;
        }
        self.skripte.push(skript);
    }

    /// Der Stil, den ein INLINE-Element weitergibt.
    ///
    /// WARUM EIGENES und nicht `stil_fuer`: `stil_fuer` baut einen Stil
    /// aus dem NICHTS heraus (Vorgabe = `Style::default()`), und ein
    /// Inline-Element hat kein solches Nichts — es erbt alles vom
    /// Absatz und aendert nur, was es selbst bedeutet. `b` in einem
    /// 24-px-`<h1>` ist 24 px und hell, nicht 16 px und hell.
    ///
    /// GEMESSEN am Fehlerbild, das dieser Weg verursacht hat: `<b>`
    /// bekam `stil_fuer` und damit `GRAD_TEXT` — in einer 24-px-
    /// Ueberschrift war ein fettes Wort 16 px und damit KLEINER als der
    /// Rest. Das ist kein Detail, das ist ein 33-%-Fehler in der
    /// Ueberschrift.
    fn inline_stil(&self, basis: Style, tag: &str, attributes: &HashMap<String, String>) -> Style {
        let mut stil = basis;
        //
        // FETT: NICHTS an der Farbe.
        //
        // Koordination 02.10. 05:40, Korrektur gegen meine Erfindung von
        // 02:55. GEMESSEN, was die beiden falschen Fassungen angerichtet
        // haben:
        // * `Color::WHITE` — auf `koordination.html` (weisser Hintergrund)
        //   stand `Color { 255,255,255,255 }` auf weiss: der Rasterizer
        //   zeichnete 0 Pixel, und `bildschirmfoto.rs` meldete „1 Item
        //   uebersprungen (keine Schrift eingebunden?)".
        // * `betont()` — relativ abgedunkelt: `Color { 114,114,114 }` auf
        //   weiss. Dieselbe Suende in weicher, nur unauffaelliger.
        //
        // DIE REGEL: im UA-Blatt jedes Browsers steht fuer `<b>` NUR
        // `font-weight: bold`; die Farbe wird **geerbt**. Der Test
        // `b_erbt_die_farbe_seines_absatzes` (in
        // `crates/narsil/tests/probe_befunde.rs`) sagt das an einem
        // Beispiel, an dem man es nachzaehlen kann:
        // `<p style="color:#123456">a <b>b</b></p>` — `b` ist #123456.
        //
        // WAS BLEIBT: ohne zweite Schrift ist „fett" nicht sichtbar. Das
        // ist ehrlicher als eine erfundene Farbe; der Wunsch nach einer
        // fetten Schrift steht in `team/WUNSCH_HTML.txt`.
        //
        // DIE DREI ANDEREN (`i`/`em`/`cite`/`var`/`dfn`/`address` und
        // `code`/`tt`/`kbd`/`samp`) waren nicht angefasst: dort gibt es
        // keine zweite Schrift UND keine Groessenaenderung, die als
        // Unterscheidung taugt, und `#767676` erreicht auf weiss genau
        // den 4.54:1-Kontrast von WCAG 1.4.3.
        //
        // 04.10.2026 (SCHRIFT): **die Zweitschrift gibt es jetzt**, und
        // damit fallen zwei Ersetzungen weg. GEMESSEN vorher an
        // `texttest.html` gegen Firefox: die vier Stilzeilen kamen in narsil
        // alle mit 125 px an (Firefox 127 / 144 / 129 / 143), also wurde
        // **jeder** fette und kursive Text aufrecht gezeichnet. GEMESSEN
        // nach der Kette: `Schrift::mit_gewicht` liefert Bold, Oblique und
        // BoldOblique, und `erfuellt = true` heisst: es ist der echte
        // Schnitt, keine Verzerrung.
        if matches!(tag, "b" | "strong") {
            stil.schriftgewicht = 700;
        }

        // KURSIV: 04.10.2026 (SCHRIFT) jetzt der echte Stil statt des grauen
        // Ersatzes. GEMESSEN: `<em>` war in Firefox 129 px breit und
        // geneigt, in narsil 125 px und aufrecht — die Zeile war so breit
        // wie die normale. GEMESSEN, dass der Ersatz auch **falsch** war:
        // Firefox faerbt `<em>` nicht grau, es neigt es.
        if matches!(tag, "i" | "em" | "cite" | "var" | "dfn" | "address") {
            stil.kursiv = true;
        }
        // Code: kleiner und schwaecher — die einzigen zwei Unterschiede,
        // die ohne zweite Schrift bleiben.
        if matches!(tag, "code" | "tt" | "kbd" | "samp") {
            stil.font_size = (stil.font_size * 0.9).max(8.0);
            stil.color = FARBE_SCHWACH;
        }
        // `small`/`big` nur in eine Richtung begrenzen: 200 Iterationen
        // `<big><big>...` duerfen den Grad nicht ins Unendliche treiben
        // (M4) und nicht auf 0 druecken.
        if tag == "small" {
            stil.font_size = (stil.font_size * 0.9).clamp(8.0, MAX_SCHRIFTGRAD);
        }
        if tag == "big" {
            stil.font_size = (stil.font_size * 1.1).clamp(8.0, MAX_SCHRIFTGRAD);
        }
        // Durchgestrichen: grau. Ohne eigene Schrift gibt es keinen
        // Strich, und Farbe ist das, was bleibt.
        if matches!(tag, "s" | "strike" | "del") {
            stil.color = FARBE_SCHWACH;
        }
        if tag == "a" {
            stil.color = FARBE_LINK;
        }
        // `style="..."`: nur die sieben erlaubten Eigenschaften, wie
        // bei einem Block. GEMESSEN: `style` steht an beiden Wegen, sonst
        // waere `<span style="color:red">` wirkungslos, waehrend
        // `<p style="color:red">` wirkt — und niemand kann erklaeren, warum.
        if let Some(inline) = attributes.get("style") {
            self.wende_inline_an(&mut stil, inline);
        }
        stil
    }

    fn grad_fuer_tag(&self, tag: &str) -> f32 {
        if let Some(e) = ueberschrift_ebene(tag) {
            grad_ueberschrift(e)
        } else {
            GRAD_TEXT
        }
    }

    /// `style="color: red; font-size: 20px"` anwenden.
    ///
    /// Bester Wert gewinnt: wer im HTML-Tag `color: blue` und inline
    /// `color: red` schreibt, sieht rot. Das ist die Reihenfolge, die
    /// CSS auch hat, und hier ist sie eine Entscheidung, keine
    /// Implementierung.
    fn wende_inline_an(&self, stil: &mut Style, inline: &str) {
        for regel in inline.split(';') {
            let Some((k, w)) = regel.split_once(':') else {
                continue;
            };
            let k = k.trim().to_ascii_lowercase();
            let w = w.trim();
            if !STIL_EIGENSCHAFTEN.contains(&k.as_str()) {
                continue;
            }
            match k.as_str() {
                "color" => {
                    if let Some(c) = parse_farbe(w) {
                        stil.color = c;
                    }
                }
                "background-color" => {
                    if let Some(c) = parse_farbe(w) {
                        stil.background = c;
                    }
                }
                "font-size" => {
                    if let Some(px) = parse_pixel(w) {
                        if px > 0.0 && px <= MAX_SCHRIFTGRAD {
                            stil.font_size = px;
                        }
                    }
                }
                "font-weight" => {
                    // 04.10.2026 (SCHRIFT): `bold` setzt jetzt das Gewicht.
                    //
                    // GEMESSEN, was hier vorher stand: `stil.color =
                    // Color::WHITE`. Das ist an weiss **unsichtbar** und
                    // an farbigem Grund **falsch** — `<span
                    // style="font-weight:bold">` auf farbigem Grund wurde
                    // weiss. Und die Zeile
                    // `tests/bild_farben.rs` erwartete es so.
                    //
                    // GEMESSEN, dass das Gewicht jetzt ankommt:
                    // `<b>` und `font-weight:bold` liefern beide
                    // `DejaVuSans-Bold.ttf` (Beispiel `a49_stil_bild.rs`).
                    //
                    // CSS Fonts 4 §5.3: `normal` und `bold` sind die
                    // Schluesselwoerter, dazwischen sind Zahlen 1..1000.
                    if w.eq_ignore_ascii_case("bold") {
                        stil.schriftgewicht = 700;
                    } else if w.eq_ignore_ascii_case("normal") {
                        stil.schriftgewicht = 400;
                    } else if let Ok(zahl) = w.parse::<u16>() {
                        if (1..=1000).contains(&zahl) {
                            stil.schriftgewicht = zahl;
                        }
                    }
                }
                "font-style" => {
                    // 04.10.2026 (SCHRIFT): `italic`/`oblique` setzt den
                    // Stil. CSS Fonts 4 §5.2 kennt genau diese drei
                    // Schluesselwoerter.
                    if w.eq_ignore_ascii_case("italic") || w.eq_ignore_ascii_case("oblique") {
                        stil.kursiv = true;
                    } else if w.eq_ignore_ascii_case("normal") {
                        stil.kursiv = false;
                    }
                }
                "width" => {
                    let w = w.trim();
                    if let Some(px) = parse_pixel(w) {
                        stil.size.width = Length::px(px);
                    } else if w.ends_with('%') {
                        if let Ok(p) = w[..w.len() - 1].trim().parse::<f32>() {
                            stil.size.width = Length::Percent(p);
                        }
                    }
                }
                "height" => {
                    let w = w.trim();
                    if let Some(px) = parse_pixel(w) {
                        stil.size.height = Length::px(px);
                    } else if w.ends_with('%') {
                        if let Ok(p) = w[..w.len() - 1].trim().parse::<f32>() {
                            stil.size.height = Length::Percent(p);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Ist das ein INLINE-Element?
///
/// GEMESSENE REGEL, und sie ist die ganze Inline-Korrektur 30.09.2026:
/// ein Inline-Element erzeugt KEINEN Knoten. Es aendert nur den Stil des
/// Textes, der nach ihm kommt, und es beendet den Absatz NICHT.
///
/// GEMESSEN an der Fehlanzeige aus dem Koordinationsbefund: fuer
/// `Das ist ein <b>ganz normaler</b> Absatz mit einem <a>Link</a>.` gibt
/// es 5 Textteile. Behandelt man `<b>` und `<a>` als Container, sind es
/// 5 Knoten in 5 Zeilen. Behandelt man sie als Inline, ist es 1 Absatz,
/// und der Link ist ein 20-Pixel-Abschnitt darin.
///
/// WARUM die Liste hier steht und nicht aus einer fremden Crate kommt
/// (ADR-030, keine Fremd-Crates): die Liste IST die Schnittstelle
/// zwischen HTML und Layout. Wer sie erweitert, muss es an EINER Stelle
/// tun — sonst entstehen zwei Listen, und die zweite wird nie gepflegt.
/// Ist das ein ERSETZTES Element? (CSS 2.1, 3.2 und 10.3.2, P6)
///
/// CSS 2.1, 3.2: "replaced elements are those whose content comes from an
/// external source: `img`, `object`, `embed`, `applet`, `iframe`..." —
/// SVG, Video und Audio sind nach derselben Regel ersetzt.
///
/// WAS DIE LISTE HIER KOSTET und warum sie nicht beim Layout steht: sie
/// ist eine EIGENSCHAFT DER SEITE, nicht des Rechnens. Das Layout muss
/// sie nicht kennen — es sieht `Node::ersetzt`, und das setzt der Bauer.
/// Zwei Listen waeren zwei Wahrheiten, und die zweite wird nie gepflegt
/// (dieselbe Begruendung wie bei `ist_inline`).
pub fn ist_ersetzt(tag: &str) -> bool {
    matches!(
        tag,
        "img" | "svg" | "video" | "audio" | "iframe" | "object" | "embed" | "canvas"
    )
}

/// Release 08.10.2026: nur die VERERBTEN Angaben eines Kastenstils (CSS
/// 2.1 Anhang F: `color`, `font-*`, `line-height`, `text-align`,
/// `white-space` ...) — Rand, Fuellung, Hintergrund, Rahmen und Groesse
/// gehoeren dem Kasten, nicht seinem Text.
fn erbbarer_textstil(stil: &Style) -> Style {
    let mut s = stil.clone();
    s.size = Dimension::AUTO;
    s.margin = Spacing::ZERO;
    s.padding = Spacing::ZERO;
    s.background = Color::TRANSPARENT;
    s.direction = Direction::default();
    s.radius = [0.0; 4];
    s.rahmen = Default::default();
    s.schatten_anzahl = 0;
    s.verlauf = None;
    s
}

fn ist_inline(tag: &str) -> bool {
    matches!(
        tag,
        "a" | "abbr"
            | "b"
            | "bdi"
            | "bdo"
            | "big"
            | "cite"
            | "code"
            | "data"
            | "del"
            | "dfn"
            | "em"
            | "font"
            | "i"
            | "ins"
            | "kbd"
            | "label"
            | "mark"
            | "q"
            | "s"
            | "samp"
            | "small"
            | "span"
            | "strike"
            | "strong"
            | "sub"
            | "sup"
            | "time"
            | "tt"
            | "u"
            | "var"
            | "wbr"
    )
}

/// Ist das ein Element, dessen Inhalt ein Absatz ist?
fn ist_absatz(tag: &str) -> bool {
    matches!(
        tag,
        //
        // 03.10. 19:10: `td`/`th` SIND KEINE ABSAETZE — sie sind Kaesten.
        //
        // GEMESSENER BEFUND (18:45, kleinster Fall `<td bgcolor="#ff6600">
        // <table>…</table></td>` und dieselbe Stelle auf
        // news.ycombinator.com):
        //     MESSUNG td mit innerer tabelle: orange_stil=0 orange_bild=0
        //     MESSUNG td allein:               orange_stil=1 orange_bild=1
        //
        // GEMESSENE URSACHE: `absatz_schliessen` (lib.rs:4626) verwirft einen
        // Absatz, dessen Text leer ist — `if absatz.text().trim().is_empty()
        // { return; }`. Eine Zelle, die eine Tabelle enthaelt, hat genau einen
        // leeren Absatz: der Kasten der inneren Tabelle schliesst ihn sofort.
        // Es entsteht KEIN Knoten, und damit auch keine Farbe. Auf
        // news.ycombinator.com ist die orange Kopfleiste genau so gebaut
        // (`<td bgcolor="#ff6600"><table …>`), deshalb fehlte sie auch nach
        // dem `bgcolor`-Fix (14:55 und 18:55).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Zelle ein KASTEN sein muss und
        // kein Absatz: CSS 2.1, 17 haengt `display: table-cell` an, und ein
        // Kasten ist das, was eine Farbe und eine Hoehe tragen kann. Der
        // Inhalt der Zelle ist damit ein Absatz IN ihr.
        //
        // GEMESSENE FOLGE fuer die Geometrie: die Zeile stapelt die Zellen
        // weiterhin untereinander (lineare Bauart, Ziffer 56 im Kopftext) —
        // es kommt nur eine Kaestenschicht dazu.
        "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "blockquote"
    )
}

/// Ebene einer Ueberschrift, 1..=6, `None` fuer alles andere.
fn ueberschrift_ebene(tag: &str) -> Option<u8> {
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Macht aus einem `href` eine absolute Adresse.
///
/// GEMESSEN an echten Seiten: 7 von 10 Links sind RELATIV. Ohne das hier
/// ist die Link-Map eine Liste von Pfaden, die nichts aufloesen.
pub fn loese_link(href: &str, basis: &str) -> String {
    // Legacy callers use file://C:\path; relative resources still need URL separators.
    let dateibasis;
    let basis = if basis.starts_with("file://") && basis.contains('\\') {
        dateibasis = basis.replace('\\', "/");
        dateibasis.as_str()
    } else { basis };

    let h = href.trim();
    if h.is_empty() {
        return String::new();
    }
    // `#anker` bleibt, wie es ist — ein Anker im selben Dokument.
    if h.starts_with('#') {
        return h.to_string();
    }
    // Schemarelative Adressen (`//cdn.example/bild.png`) behalten das
    // Schema der Seite und wechseln den Host. Sonst wird daraus faelschlich
    // `https://seitenhost//cdn.example/...`.
    if h.starts_with("//") {
        return match basis.find("://") {
            Some(i) => format!("{}:{h}", &basis[..i]),
            None => h.to_string(),
        };
    }
    // Absolut (hat ein Schema): unveraendert.
    if h.contains("://") {
        return h.to_string();
    }
    // `mailto:` und `tel:`: Schema ohne `//`.
    if h.starts_with("mailto:") || h.starts_with("tel:") || h.starts_with("data:") {
        return h.to_string();
    }
    // Release 08.10.2026: JEDES Schema ohne `//` ist absolut (WHATWG URL
    // §4.4, „scheme start state": ALPHA, dann ALPHA/Ziffer/`+`/`-`/`.`, dann
    // `:`). GEMESSEN vorher: `about:einstellungen` auf `narsil://start`
    // wurde zu `narsil://start/about:einstellungen` — die Kachel „Optionen"
    // der Startseite fuehrte ins Leere; dasselbe fuer `javascript:`,
    // `view-source:`, `blob:`. Ein einzelner Buchstabe (`C:`) ist ein
    // Laufwerk, kein Schema.
    if let Some((schema, _)) = h.split_once(':')
        && schema.len() >= 2
        && schema.starts_with(|c: char| c.is_ascii_alphabetic())
        && schema.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return h.to_string();
    }
    if basis.is_empty() {
        return h.to_string();
    }
    let schema_end = basis.find("://").map(|i| i + 3);
    let host_ende = match schema_end {
        Some(start) => basis[start..]
            .find('/')
            .map(|i| start + i)
            .unwrap_or(basis.len()),
        None => basis.find('/').unwrap_or(basis.len()),
    };
    let wurzel = &basis[..host_ende];
    // Absolute Pfade (`/pfad`) haengen nur an Host+Schema.
    if h.starts_with('/') {
        return format!("{wurzel}{h}");
    }
    // Relative Pfade: letzter Ordner der Basis + h.
    let verzeichnis = match basis.rfind('/') {
        Some(i) if i >= host_ende => &basis[..i],
        _ => basis,
    };
    let mut pfad = std::format!("{verzeichnis}/{h}");
    // `..` und `.` aufloesen, aber nicht ueber die Wurzel hinaus.
    pfad = normalisiere(&pfad);
    pfad
}

/// Die Geometrie einer Maske als **Bild-Geometrie**.
///
/// 06.10.2026: `mask-size`/`mask-position`/`mask-repeat` sind dieselben
/// Werteformen wie bei `background-size`/`-position`/`-repeat` (CSS
/// Masking 1 §3.2 verweist auf CSS Images 3). Der Bildpfad kennt nur
/// `BildGeo` — also wird hier uebersetzt, statt eine zweite Geometrie
/// im Maler zu bauen.
fn bild_geo_aus_maske(m: &narsil_css::Maske) -> narsil_css::BildGeo {
    let (mut groesse, mut cover, mut contain) = (0.0f32, false, false);
    match m.groesse {
        narsil_css::MaskGroesse::Cover => cover = true,
        narsil_css::MaskGroesse::Contain => contain = true,
        narsil_css::MaskGroesse::Wert(v) => groesse = v,
        narsil_css::MaskGroesse::Auto => {}
    }
    narsil_css::BildGeo {
        groesse,
        cover,
        contain,
        position_x: m.position_x,
        position_y: m.position_y,
        position_px: false,
        position_px_achsen: None,
        position_em: (false, false),
        position_ex: (false, false),
        wiederholen_x: m.wiederholen_x,
        wiederholen_y: m.wiederholen_y,
        groesse_xy: [None, None],
        rand_px: [0.0; 4],
        wiederhol_art: [0, 0],
        ursprung: 0,
        bild_clip: 0,
        innen_px: [0.0; 4],
        vom_ende: (false, false),
        border_image: None,
        viewport: None,
        paints_canvas: false,
        from_body: false,
    }
}

/// Release 14.10.2026 (Agent CSS2): `border-image` from the declared values
/// (shorthand first, then the longhands; CSS Backgrounds 3 §6.6). `None`
/// without an image URL.
pub(crate) fn border_image_from(s: &narsil_css::Berechnet) -> Option<(String, narsil_css::BorderImage)> {
    use narsil_css::BorderImageWidth as W;
    let value = |name: &str| s.schluessel.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.trim().to_string());
    let mut bi = narsil_css::BorderImage {
        slice: [(100.0, true); 4],
        fill: false,
        width: [W::Factor(1.0); 4],
        outset: [(0.0, false); 4],
        repeat: [0, 0],
    };
    let four = |parts: &[&str]| -> Option<[usize; 4]> {
        match parts.len() {
            1 => Some([0, 0, 0, 0]),
            2 => Some([0, 1, 0, 1]),
            3 => Some([0, 1, 2, 1]),
            4 => Some([0, 1, 2, 3]),
            _ => None,
        }
    };
    let number = |t: &str| t.trim_end_matches("px").parse::<f32>().ok().filter(|v| v.is_finite());
    let read_slice = |t: &str, bi: &mut narsil_css::BorderImage| -> bool {
        let mut parts: Vec<&str> = t.split_whitespace().collect();
        let mut fill = false;
        if let Some(i) = parts.iter().position(|x| x.eq_ignore_ascii_case("fill")) {
            fill = true;
            parts.remove(i);
        }
        let Some(idx) = four(&parts) else { return false };
        let mut new = [(0.0, false); 4];
        for (k, i) in idx.iter().enumerate() {
            let x = parts[*i];
            new[k] = match x.strip_suffix('%') {
                Some(p) => match p.parse::<f32>() {
                    Ok(v) if v >= 0.0 => (v, true),
                    _ => return false,
                },
                None => match x.parse::<f32>() {
                    Ok(v) if v >= 0.0 => (v, false),
                    _ => return false,
                },
            };
        }
        bi.slice = new;
        bi.fill = fill;
        true
    };
    let read_width = |t: &str, bi: &mut narsil_css::BorderImage| -> bool {
        let parts: Vec<&str> = t.split_whitespace().collect();
        let Some(idx) = four(&parts) else { return false };
        let mut new = [W::Auto; 4];
        for (k, i) in idx.iter().enumerate() {
            let x = parts[*i];
            new[k] = if x.eq_ignore_ascii_case("auto") {
                W::Auto
            } else if let Some(p) = x.strip_suffix('%') {
                match p.parse::<f32>() {
                    Ok(v) if v >= 0.0 => W::Percent(v),
                    _ => return false,
                }
            } else if let Some(p) = x.strip_suffix("px") {
                match p.parse::<f32>() {
                    Ok(v) if v >= 0.0 => W::Px(v),
                    _ => return false,
                }
            } else {
                match x.parse::<f32>() {
                    Ok(v) if v >= 0.0 => W::Factor(v),
                    _ => return false,
                }
            };
        }
        bi.width = new;
        true
    };
    let read_outset = |t: &str, bi: &mut narsil_css::BorderImage| -> bool {
        let parts: Vec<&str> = t.split_whitespace().collect();
        let Some(idx) = four(&parts) else { return false };
        let mut new = [(0.0, false); 4];
        for (k, i) in idx.iter().enumerate() {
            let x = parts[*i];
            let px = x.ends_with("px");
            match number(x) {
                Some(v) if v >= 0.0 => new[k] = (v, px),
                _ => return false,
            }
        }
        bi.outset = new;
        true
    };
    let read_repeat = |t: &str, bi: &mut narsil_css::BorderImage| -> bool {
        let kind = |x: &str| match x {
            "stretch" => Some(0u8),
            "repeat" => Some(1),
            "round" => Some(2),
            "space" => Some(3),
            _ => None,
        };
        let parts: Vec<&str> = t.split_whitespace().collect();
        match parts.as_slice() {
            [a] => match kind(a) {
                Some(a) => bi.repeat = [a, a],
                None => return false,
            },
            [a, b] => match (kind(a), kind(b)) {
                (Some(a), Some(b)) => bi.repeat = [a, b],
                _ => return false,
            },
            _ => return false,
        }
        true
    };
    let url_of = |t: &str| -> Option<String> {
        let i = t.find("url(")?;
        let rest = &t[i + 4..];
        let j = rest.find(')')?;
        let u = rest[..j].trim().trim_matches(['"', '\'']);
        (!u.is_empty()).then(|| u.to_string())
    };
    let mut url: Option<String> = None;
    // The shorthand: source || slice [/ width [/ outset]] || repeat.
    if let Some(k) = value("border-image") {
        let lower = k.to_ascii_lowercase();
        if lower != "none" {
            url = url_of(&k);
            let without_url = match (k.find("url("), k.find(')')) {
                (Some(a), Some(b)) if b > a => format!("{} {}", &lower[..a], &lower[b + 1..]),
                _ => lower.clone(),
            };
            let mut repeat = Vec::new();
            let mut rest = Vec::new();
            for t in without_url.split_whitespace() {
                if matches!(t, "stretch" | "repeat" | "round" | "space") {
                    repeat.push(t);
                } else {
                    rest.push(t);
                }
            }
            if !repeat.is_empty() {
                read_repeat(&repeat.join(" "), &mut bi);
            }
            let rest = rest.join(" ");
            let mut pieces = rest.split('/');
            if let Some(p) = pieces.next().map(str::trim).filter(|x| !x.is_empty()) {
                read_slice(p, &mut bi);
            }
            if let Some(p) = pieces.next().map(str::trim).filter(|x| !x.is_empty()) {
                read_width(p, &mut bi);
            }
            if let Some(p) = pieces.next().map(str::trim).filter(|x| !x.is_empty()) {
                read_outset(p, &mut bi);
            }
        }
    }
    if let Some(q) = value("border-image-source") {
        url = if q.eq_ignore_ascii_case("none") { None } else { url_of(&q) };
    }
    if let Some(t) = value("border-image-slice") {
        read_slice(&t.to_ascii_lowercase(), &mut bi);
    }
    if let Some(t) = value("border-image-width") {
        read_width(&t.to_ascii_lowercase(), &mut bi);
    }
    if let Some(t) = value("border-image-outset") {
        read_outset(&t.to_ascii_lowercase(), &mut bi);
    }
    if let Some(t) = value("border-image-repeat") {
        read_repeat(&t.to_ascii_lowercase(), &mut bi);
    }
    Some((url?, bi))
}

/// Release 13.10.2026 (Agent CSS2): die Rahmenbreiten des Elements in die
/// Bildgeometrie (siehe `BildGeo::rand_px`); ein Rahmen ohne Stil hat keine
/// Breite (CSS Backgrounds 3 §4.3). GEMESSEN vorher (WPT `margin-padding-clear/
/// margin-collapse-008`, `-012`, `-019`: `border-top: 1em` und `background-
/// position: left -1em`): das Bild lag um den Rahmen zu hoch.
pub(crate) fn bild_geo_mit_rand(mut g: narsil_css::BildGeo, s: &narsil_css::Berechnet) -> narsil_css::BildGeo {
    let breite = |b: f32, st: narsil_css::Rahmenstil| {
        if st == narsil_css::Rahmenstil::Kein || !b.is_finite() { 0.0 } else { b.max(0.0) }
    };
    g.rand_px = [
        breite(s.rahmen_oben_breite, s.rahmen_oben_stil),
        breite(s.rahmen_rechts_breite, s.rahmen_rechts_stil),
        breite(s.rahmen_unten_breite, s.rahmen_unten_stil),
        breite(s.rahmen_links_breite, s.rahmen_links_stil),
    ];
    // Release 13.10.2026 (Agent CSS2): `background-origin`/`-clip` (die
    // unterste Ebene, CSS Backgrounds 3 §3.5/§3.7) und die Innenabstaende.
    let wort = |namen: &[&str]| {
        s.schluessel
            .iter()
            .rev()
            .find(|(k, _)| namen.contains(&k.as_str()))
            .map(|(_, v)| v.rsplit(',').next().unwrap_or("").trim().to_ascii_lowercase())
    };
    let kasten = |w: Option<String>, vorgabe: u8| match w.as_deref() {
        Some("border-box") => 1,
        Some("padding-box") => 0,
        Some("content-box") => 2,
        _ => vorgabe,
    };
    g.ursprung = kasten(wort(&["background-origin", "-webkit-background-origin"]), 0);
    // `background-attachment: fixed`: die Lageflaeche ist die Ansicht, und
    // `background-origin` gilt nicht (§3.7).
    // Release 14.10.2026 (Agent CSS2): `ursprung` 3 is the viewport (see
    // `BildGeo::ansicht`); without a known viewport the image falls back to
    // the border box (WPT `background-origin-006`).
    if wort(&["background-attachment"]).as_deref() == Some("fixed") {
        g.ursprung = 3;
    }
    g.bild_clip = match wort(&["background-clip", "-webkit-background-clip"]).as_deref() {
        Some("padding-box") => 1,
        Some("content-box") => 2,
        // Release 14.10.2026 (Agent CSS2): `border-area` (CSS Backgrounds 4
        // §2.2), see `bilder_hintergrund::item`.
        Some("border-area") => 4,
        _ => 0,
    };
    let px = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    g.innen_px = [px(s.fuellung_oben), px(s.fuellung_rechts), px(s.fuellung_unten), px(s.fuellung_links)];
    g
}

/// Loest `.` und `..` in einem Pfad auf.
///
/// GEMESSEN: `href="../b"` aus `http://h/a/c` ergibt `http://h/b`, nicht
/// `http://h/a/../b`. Ohne das schickt der Browser den Pfad zwar auch so,
/// aber die Fehlermeldung bei nicht-existenten Seiten ist dann unlesbar.
fn normalisiere(pfad: &str) -> String {
    let (prefix, rest) = match pfad.find("://") {
        Some(i) => {
            let nach = &pfad[i + 3..];
            match nach.find('/') {
                Some(j) => (&pfad[..i + 3 + j], &nach[j..]),
                None => return pfad.to_string(),
            }
        }
        None => ("", pfad),
    };
    let mut teile: Vec<&str> = Vec::new();
    for teil in rest.split('/') {
        match teil {
            "" | "." => {}
            ".." => {
                teile.pop();
            }
            t => teile.push(t),
        }
    }
    if teile.is_empty() {
        format!("{prefix}/")
    } else {
        format!("{prefix}/{}", teile.join("/"))
    }
}

/// Liest eine Laenge in Pixeln: `10`, `10px`, ` 10 `.
///
/// `None`, wenn sie nicht lesbar ist. Gemessen (16:15): ein praesentatives
/// `height` ist bei HN eine Zahl ohne Einheit, `height="10px"` ist die
/// Schreibweise, die ein Mensch schreibt — beide sind eine Angabe, und beide
/// gehoeren an diese eine Stelle.
fn parse_px(w: &str) -> Option<f32> {
    let w = w.trim();
    let ziffern = w.strip_suffix("px").unwrap_or(w).trim();
    ziffern.parse::<f32>().ok().filter(|n| n.is_finite())
}

/// Liest eine Farbe: `#abc`, `#aabbcc`, `red`, `white`, `black`.
///
/// `None`, wenn sie nicht lesbar ist — dann bleibt der alte Wert stehen.
/// Eine kaputte Farbe ist kein Grund, den Text unsichtbar zu machen.
fn parse_farbe(w: &str) -> Option<Color> {
    let w = w.trim();
    if let Some(hex) = w.strip_prefix('#') {
        return Color::parse_hex(&std::format!("#{hex}")).ok();
    }
    match w.to_ascii_lowercase().as_str() {
        "black" => Some(Color::BLACK),
        "white" => Some(Color::WHITE),
        "red" => Some(Color::new(0xcc, 0x33, 0x33, 255)),
        "green" => Some(Color::new(0x33, 0x99, 0x33, 255)),
        "blue" => Some(Color::new(0x33, 0x33, 0xcc, 255)),
        "gray" | "grey" => Some(Color::new(0x88, 0x88, 0x88, 255)),
        _ => None,
    }
}

/// Liest `20px` oder eine blosse Zahl als Pixel.
fn parse_pixel(w: &str) -> Option<f32> {
    let w = w.trim();
    let ohne_px = w.strip_suffix("px").unwrap_or(w);
    ohne_px.parse::<f32>().ok()
}

// ---------------------------------------------------------------------------
// Der Umbruch — die eine Sache, die die Engine nicht kann
// ---------------------------------------------------------------------------

/// Bricht Text in Zeilen, die in `breite` Pixel passen.
///
/// GEMESSEN am 29.09.2026 an `narsil_layout_engine/src/text.rs:231`:
/// Die Engine gibt `intrinsic_size` als EINE Zeile zurueck. Ein Absatz
/// aus 93 Zeichen auf 200 px Breite ergab `Breite 375` und zeigte 14
/// Zeichen; der Rest lief aus dem Container (561 px Ueberhang, gemessen).
///
/// Die Breite kommt hier aus [`narsil_text::measure`] — aus GEMESSEN, nicht
/// aus der Formel "0,6 em" oder "Zeichen mal 7/8". Genau dieser Fehler
/// stand zweimal im Projekt (in `bounds()` und im Kommentar von
/// `text.rs:281`) und hat die Boxen 31,4 % zu schmal gemacht.
///
/// Macht aus einem Textstueck einen Text, der an Absaetze angehaengt
/// werden kann: jeder Weissraum-Lauf wird EIN Leerzeichen.
///
/// WARUM NICHT `split_whitespace().join(" ")` (das stand hier vorher):
/// das entfernt den Leerraum am ANFANG und am ENDE. Bei
/// `<p>Das ist ein <b>x</b> mehr</p>` ergab "Das ist ein" + "x" =
/// "Das ist einx" — zwei Woerter zu einem. Der Fehler war nicht sichtbar,
/// weil die Teile in verschiedenen Absaetzen landeten; sobald man sie
/// zusammenfuehrt (das ist der Punkt von Inline-Elementen), faellt er
/// auf. Hier bleibt ein Leerzeichen, wenn eines da war.
///
/// M4: die Z capacitiesreserve ist `t.len()`, und `t` ist ein Slice der
/// Eingabe — es wird nichts allokiert, was groesser waere als die Quelle.
fn css_leerraum(c: char) -> bool { matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000c}') }

/// Release 08.10.2026: Leerzeichen und Tabulatoren fuer `white-space:
/// pre`/`pre-wrap` erhalten — als geschuetzte Leerzeichen, die der
/// Zeilenbauer nicht zusammenfasst. Bei `pre-wrap` bleibt ein einzelnes
/// Leerzeichen zwischen Woertern ein normales (dort darf umgebrochen
/// werden). Tabulatoren springen auf die naechste Achterstelle
/// (`tab-size: 8`, CSS Text 3 §4.1).
/// Release 12.10.2026 (Agent CSS2): darf `c` mit `hanging-punctuation: first`
/// haengen? Die allgemeinen Kategorien Ps, Pi, Pf sowie U+0027 und U+0022
/// (CSS Text 4 §9.2) — die gebraeuchlichen Zeichen.
fn haengende_satzzeichen(c: char) -> bool {
    matches!(
        c,
        '(' | '[' | '{' | '\'' | '"' | '\u{ab}' | '\u{bb}' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}'
            | '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' | '\u{2039}' | '\u{203a}' | '\u{2045}'
            | '\u{207d}' | '\u{208d}' | '\u{2308}' | '\u{230a}' | '\u{2329}' | '\u{2768}' | '\u{276a}'
            | '\u{276c}' | '\u{276e}' | '\u{2770}' | '\u{2772}' | '\u{2774}' | '\u{27e6}' | '\u{27e8}'
            | '\u{27ea}' | '\u{2983}' | '\u{3008}' | '\u{300a}' | '\u{300c}' | '\u{300e}' | '\u{3010}'
            | '\u{3014}' | '\u{3016}' | '\u{3018}' | '\u{301a}' | '\u{301d}' | '\u{ff08}' | '\u{ff3b}'
            | '\u{ff5b}' | '\u{ff62}'
    )
}

fn leerraum_erhalten(t: &str, modus: narsil_core::style::Weissraum, tab: usize) -> String {
    use narsil_core::style::Weissraum as W;
    let pre = modus == W::Pre;
    let mut raus = String::with_capacity(t.len() + 8);
    let zeichen: Vec<char> = t.chars().collect();
    let mut spalte = 0usize;
    for (i, &c) in zeichen.iter().enumerate() {
        match c {
            '\t' => {
                // Release 12.10.2026 (Agent CSS2): `tab-size` statt fest 8;
                // 0 laesst den Tabulator verschwinden (CSS Text 3 §4.2).
                let n = if tab == 0 { 0 } else { tab - spalte % tab };
                for _ in 0..n {
                    raus.push('\u{a0}');
                }
                spalte += n;
                // Release 15.10.2026 (Agent CSS2): a preserved tab is a break
                // opportunity after it like a preserved space — in
                // `break-spaces` after every tab, in `pre-wrap` after a run of
                // white space (where it hangs, `Wort::haengt`). MEASURED
                // before (WPT css-text `white-space/pre-wrap-tab-001`,
                // `XX&#9;&#9;XX` in 16ch): one unbreakable word.
                let ende_der_folge = zeichen.get(i + 1).is_none_or(|n| *n != ' ' && *n != '\t');
                if modus == W::UmbruchLeer || (modus == W::PreUmbruch && ende_der_folge) {
                    raus.push('\u{200B}');
                }
            }
            // Release 12.10.2026 (Agent CSS2): `break-spaces` — nach JEDEM
            // Leerzeichen darf umbrochen werden, und es haengt nicht (CSS Text
            // 3 §3): geschuetztes Leerzeichen plus Umbruch ohne Breite.
            ' ' if modus == W::UmbruchLeer => {
                raus.push('\u{a0}');
                raus.push('\u{200B}');
                spalte += 1;
            }
            ' ' => {
                // Release 12.10.2026 (Agent CSS2): `pre-wrap` — eine Folge
                // bleibt erhalten, umbrochen wird NACH ihr (vor dem naechsten
                // Zeichen); ihr letztes Leerzeichen ist die Umbruchstelle, die
                // davor haengen am Wort (`Wort::haengt`). GEMESSEN vorher (WPT
                // css-text `white-space/pre-wrap-006`): `XX    XX` in 4ch blieb
                // EIN Wort.
                let letztes = !pre && zeichen.get(i + 1).is_some_and(|n| *n != ' ' && *n != '\t');
                let einzeln = !pre && i > 0 && zeichen[i - 1] != ' ' && letztes;
                // Release 15.10.2026 (Agent CSS2): a single space at the start
                // of the text is a break opportunity too (UAX #14 LB18, "SP
                // ÷"), but a plain U+0020 there would be dropped as leading
                // white space — so a protected space plus a zero-width break.
                // MEASURED before (WPT css-text `white-space/pre-wrap-leading-
                // spaces-001`, ` XX` in 2ch): the space stuck to the word and
                // the line overflowed.
                if letztes && i == 0 && modus == W::PreUmbruch {
                    raus.push('\u{a0}');
                    raus.push('\u{200B}');
                    spalte += 1;
                    continue;
                }
                raus.push(if einzeln || (letztes && i > 0) { ' ' } else { '\u{a0}' });
                spalte += 1;
            }
            c => {
                raus.push(c);
                spalte += 1;
            }
        }
    }
    raus
}

pub(crate) fn weissraum(t: &str) -> String {
    let mut raus = String::with_capacity(t.len());
    let mut leer = false;
    // Release 11.10.2026: CSS Text 3 §4.1.2 (Segmentumbruch): eine
    // Leerraumfolge mit Zeilenumbruch zwischen zwei ostasiatisch breiten
    // Zeichen (Breite F/W/H, kein Hangul) faellt ganz weg — „中⏎文" ist
    // „中文", nicht „中 文". GEMESSEN vorher (WPT `seg-break-transformation-*`).
    let mut vorher: Option<char> = None;
    let mut umbruch_in_folge = false;
    for c in t.chars() {
        if css_leerraum(c) {
            if !leer {
                raus.push(' ');
                leer = true;
                umbruch_in_folge = false;
            }
            umbruch_in_folge |= c == '\n';
        } else {
            if leer && umbruch_in_folge && vorher.is_some_and(ostasiatisch_breit) && ostasiatisch_breit(c) {
                raus.pop();
            }
            raus.push(c);
            leer = false;
            vorher = Some(c);
        }
    }
    raus
}

/// Release 11.10.2026: East Asian Width F, W oder H (UAX #11), ohne Hangul —
/// die Bedingung der Segmentumbruch-Regel (CSS Text 3 §4.1.2).
fn ostasiatisch_breit(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x303E      // Radikale, CJK-Satzzeichen
        | 0x3041..=0x30FF    // Hiragana, Katakana
        | 0x3105..=0x312F    // Bopomofo
        | 0x31A0..=0x31FF    // Bopomofo-Erweiterung, Katakana-Erweiterung
        | 0x3200..=0x33FF    // Umschlossene Zeichen, Kompatibilitaet
        | 0x3400..=0x4DBF    // Erweiterung A
        | 0x4E00..=0x9FFF    // Einheitliche Ideogramme
        | 0xA000..=0xA4CF    // Yi
        | 0xF900..=0xFAFF    // Kompatibilitaetsideogramme
        | 0xFE30..=0xFE4F    // Kompatibilitaetsformen
        | 0xFF01..=0xFF60    // Vollbreite Formen
        | 0xFF61..=0xFF9F    // Halbbreite Katakana (H)
        | 0xFFE0..=0xFFE6    // Vollbreite Zeichen
        | 0x20000..=0x2FFFD
        | 0x30000..=0x3FFFD
    )
}

/// Bricht einen Wortstrom in Zeilen, die in `breite` Pixel passen.
///
/// WARUM EIGENE FUNKTION und nicht `umbreche`: `umbreche` bekommt einen
/// Text und gibt Zeilen ohne Herkunft zurueck. Hier muss jede Zeile
/// sagen, aus welchem ABSCHNITT ihre Woerter stammen — sonst kann ein
/// Link in der Mitte des Absatzes nicht farblich und nicht klickbar
/// bleiben.
///
/// GEMESSENE ZUSAGE, und sie ist eine Gleichheit: `zeilen_bauen` mit
/// einem einzigen Abschnitt ergibt GENAU `umbreche`. Der Test
/// `zeilen_bauen_ist_umbreche_bei_einem_abschnitt` prueft das an
/// 40 Zeichen Beispielen — die Korrektur darf den Umbruch nicht
/// veraendern, nur die Abschnittszustaende dazufuegen.
///
/// # DIE GRENZEN
///
/// * `MAX_ZEILEN_PRO_ABSATZ` (M3): die Zahl der Zeilen. Ohne sie sind es
///   bei 40 Pixel Breite und einem 1.000.000-URL 20.000 Zeilen fuer
///   EINEN Absatz — die Seite waere unbrauchbar und der Speicher nicht.
/// * `MAX_WOERTER_PRO_ABSATZ` hat der Aufrufer schon angewandt.
///
/// # GEMESSEN 04.10.2026 (SCHRIFT), warum `stil_von` dazu kam
///
/// `breite_von` bekam nur den Grad, also konnte der Umbruch keinen
/// Schnitt kennen: ein Absatz in `font-weight: bold` wurde mit dem
/// regulaeren Schnitt gemessen und mit dem fetten gezeichnet (GEMESSEN
/// 128 gegen 145 Spalten bei 16 px, `texttest.html` gegen Firefox). Die
/// Folge war ein Umbruch an der falschen Stelle. `stil_von` liefert je
/// **Abschnitt** `(gewicht, kursiv)` — die Abschnitte tragen den Stil des
/// Knotens, und ein Absatz kann an jeder Stelle einen anderen Schnitt
/// haben (`<p>a <b>bb c</b></p>`).
fn zeilen_bauen(
    woerter: &[Wort],
    // 04.10.2026 (SCHRIFT_9): der letzte Parameter ist die
    // `font-family`-Liste des Abschnitts — der Umbruch misst mit der
    // Datei, die der Raster fuer diese Familie zeichnet.
    breite_von: &dyn Fn(&str, f32, u16, bool, &str) -> f32,
    grad_von: &dyn Fn(&usize) -> f32,
    stil_von: &dyn Fn(&usize) -> (u16, bool, String),
    // Release 14.10.2026 (Agent CSS2): (`letter-spacing`, `word-spacing`) of
    // the section in px — the layout adds them to every run, so the line
    // breaking must count them too (CSS Text 3 §8). MEASURED before (WPT
    // `CSS2/borders/border-001` reference, Ahem with `word-spacing: 3em` in a
    // 6em box): three words were put on a line that holds two.
    spacing_of: &dyn Fn(&usize) -> (f32, f32, f32),
    // Release 07.10.2026: die Breite JE ZEILE (Nachumbruch neben Floats).
    breite_fuer: &dyn Fn(usize) -> f32,
    font: &BitmapFont,
) -> Vec<Vec<(String, usize, bool)>> {
    // (Text, Abschnittsnummer, "Leerzeichen davor")
    //
    // WARUM DER TEXT ZURUECKKOMMT und nicht nur eine Index: der Umbruch
    // muss ein zu langes Wort TEILEN koennen, und ein Teilstueck hat
    // keinen Index im Wortstrom. Es entsteht erst hier.
    let mut raus: Vec<Vec<(String, usize, bool)>> = Vec::new();
    if woerter.is_empty() {
        return raus;
    }
    // Grad, den die Schrift nicht kann: alles in eine Zeile, unveraendert.
    // GEMESSEN: `umbreche` tut bei einem Grad ausserhalb 1..=96 genau das.
    let erste_breite = breite_fuer(0);
    if !erste_breite.is_finite() || erste_breite <= 0.0 {
        return vec![
            woerter
                .iter()
                .map(|w| (w.text.clone(), w.abschnitt, w.leer_davor))
                .collect(),
        ];
    }
    let erster_grad = grad_von(&woerter[0].abschnitt);
    if !(1.0..=MAX_SCHRIFTGRAD).contains(&erster_grad) {
        return vec![
            woerter
                .iter()
                .map(|w| (w.text.clone(), w.abschnitt, w.leer_davor))
                .collect(),
        ];
    }

    let mut aktuell: Vec<(String, usize, bool)> = Vec::new();
    let mut aktuell_breite = 0.0f32;
    // Release 11.10.2026: je Eintrag von `aktuell` (Zwischenraum davor,
    // Breite mit rechtem Rand) und die letzte erlaubte Umbruchstelle darin —
    // ein Wort ohne `bruch_davor` nimmt die Woerter seit dieser Stelle mit
    // in die naechste Zeile.
    let mut masse: Vec<(f32, f32)> = Vec::new();
    let mut letzter_bruch = 0usize;
    for wort in woerter {
        // Release 08.10.2026: 1 px Spiel. Die Woerter werden EINZELN
        // gemessen (plus Leerzeichen), das Layout misst den Lauf AM STUECK
        // — die Summe liegt Bruchteile darueber. GEMESSEN vorher: MDN
        // „vor 1 Tag" in einer `max-content`-Spalte (49,4 px) brach im
        // Nachumbruch auf genau diese Breite um, die Spalte schrumpfte im
        // zweiten Layout auf 27 px.
        let breite_zeile = |k: usize| breite_fuer(k).max(1.0) + 1.0;
        let grad = grad_von(&wort.abschnitt);
        // GEMESSEN 04.10.2026 (SCHRIFT): der Schnitt des Abschnitts, nicht
        // der des Absatzes — `a <b>bb</b>` misst „bb“ mit dem fetten
        // Schnitt, „a“ mit dem regulaeren.
        let (gewicht, kursiv, familie) = stil_von(&wort.abschnitt);
        let (letter_gap, word_gap, hyphen_width) = spacing_of(&wort.abschnitt);
        let with_spacing = |t: &str, b: f32| {
            b + letter_gap * t.chars().count() as f32 + word_gap * t.chars().filter(|c| *c == '\u{a0}').count() as f32
        };
        // Release 15.10.2026 (Agent CSS2): a soft hyphen has no width; at the
        // end of a line it shows a hyphen (`shy_extra`, counted when the word
        // has to fit). MEASURED before (WPT css-text `hyphens/hyphens-manual-
        // 011`, `Deoxy&shy;ribo&shy;nucleic acid` in 10ch): no break at the
        // soft hyphens.
        let ohne_shy;
        let wort_text: &str = if wort.text.contains('\u{AD}') {
            ohne_shy = wort.text.replace('\u{AD}', "");
            &ohne_shy
        } else {
            &wort.text
        };
        let shy_extra = if wort.text.ends_with('\u{AD}') { hyphen_width } else { 0.0 };
        let wb_voll = with_spacing(wort_text, breite_von(wort_text, grad, gewicht, kursiv, &familie));
        // Release 12.10.2026 (Agent CSS2): haengende Leerzeichen (`pre-wrap`)
        // zaehlen fuer das Passen nicht (`Wort::haengt`); gesetzt wird die
        // volle Breite.
        // Release 15.10.2026 (Agent CSS2): so do other space separators at
        // the end of the word (`Wort::other_spaces_hang`). MEASURED before
        // (WPT css-text `white-space/trailing-ideographic-space-017`,
        // `X&#x3000;` in 1ch): the ideographic space went to the next line.
        let hangs = |c: char| {
            (wort.haengt && matches!(c, '\u{a0}' | '\u{200B}')) || (wort.other_spaces_hang && other_space_separator(c))
        };
        let wb = if wort.text.ends_with(hangs) {
            let t = wort.text.trim_end_matches(hangs);
            with_spacing(t, breite_von(t, grad, gewicht, kursiv, &familie))
        } else {
            wb_voll
        };
        // Release 15.10.2026 (Agent CSS2): the letter-spacing after the last
        // character does not count at the end of a line (CSS Text 3 §8.2:
        // "letter-spacing must not be applied at the end of a line"); the
        // next word on the same line still sees it (`haengend`). MEASURED
        // before (WPT css-text `letter-spacing/letter-spacing-200`, `1em`
        // spacing in a 5em box): "1 2 三" did not fit and broke early.
        let wb = if letter_gap > 0.0 && !wort.text.is_empty() { (wb - letter_gap).max(0.0) } else { wb };
        // The soft hyphen's hyphen counts for the fit (the word then ends the
        // line); `haengend` below gives it back when the line goes on.
        let wb = wb + shy_extra;
        let haengend = wb_voll - wb;
        // Release 12.10.2026 (Agent CSS2): passt das erste Wort einer Zeile
        // nicht neben die Floats, aber weiter unten, bleibt diese Zeile leer
        // (siehe `absatz_neu_umbrechen`). GEMESSEN vorher (WPT `floats/float-
        // no-content-beside-001`): das lange Wort stand neben dem Float und
        // ragte aus dem Absatz.
        let bedarf = wb + wort.rand_links + wort.rand_rechts;
        let leerzeilen = |raus: &mut Vec<Vec<(String, usize, bool)>>| {
            for _ in 0..64 {
                let k = raus.len();
                if bedarf <= breite_zeile(k) || !(1..=64).any(|j| breite_zeile(k + j) >= bedarf) {
                    break;
                }
                if raus.len() >= MAX_ZEILEN_PRO_ABSATZ {
                    break;
                }
                raus.push(Vec::new());
            }
        };
        if aktuell.is_empty() {
            leerzeilen(&mut raus);
        }
        let breite = breite_zeile(raus.len());
        // Das Leerzeichen wird nur gesetzt, wenn in der Quelle eines
        // stand (CSS 2.1, 16.6.1). Sonst waere `<b>eins</b><i>zwei</i>`
        // "eins zwei" statt "einszwei".
        let sb = if wort.leer_davor {
            breite_von(" ", grad, gewicht, kursiv, &familie) + letter_gap + word_gap
        } else {
            0.0
        };
        // Der Rand eines Inline-Elements ist Platz in der Zeile
        // (GEMESSEN 30.09.2026: `nav a{margin-right:12px}` klebte ohne
        // Abstand aneinander). Links zaehlt er VOR dem Wort, rechts
        // danach.
        let zusatz = sb + wort.rand_links;
        // Release 12.10.2026 (Agent CSS2): zu lang nur fuer DIESE (neben
        // Floats verkuerzte) Zeile — dann endet sie hier, und die naechste
        // (oder eine Leerzeile darunter) nimmt das Wort ganz. GEMESSEN vorher
        // (WPT `css1/c414-flt-fit-000`): mit der schmalen Zeilenbreite wurde
        // `5&nbsp;&nbsp;…` hart geteilt.
        let mut breite = breite;
        if !aktuell.is_empty() && wb + zusatz > breite && wort.bruch_davor {
            raus.push(std::mem::take(&mut aktuell));
            masse.clear();
            letzter_bruch = 0;
            aktuell_breite = 0.0;
            if raus.len() >= MAX_ZEILEN_PRO_ABSATZ {
                return raus;
            }
            leerzeilen(&mut raus);
            breite = breite_zeile(raus.len());
        }

        // Zu lang fuer eine ganze Zeile: hart teilen, wie `umbreche`.
        // GEMESSEN: eine 5.000-Zeichen-URL darf nicht eine Zeile von
        // 5.000 Pixeln ergeben.
        // Release 15.10.2026 (Agent CSS2): at the start of a line the space
        // before the word is dropped, so only its margin counts. MEASURED
        // before (WPT `CSS2/text/letter-spacing-justify-001`, the second
        // `XXXXXX` with `1em` spacing on a line of its own): the width of the
        // dropped space made it "too long" and it was split.
        let vor_wort = if aktuell.is_empty() { wort.rand_links } else { zusatz };
        if wb + vor_wort > breite && !wort.unteilbar {
            if !aktuell.is_empty() {
                raus.push(std::mem::take(&mut aktuell));
                masse.clear();
                letzter_bruch = 0;
                // `aktuell_breite` wird gleich aus `rest` neu gesetzt
                // (siehe unten) — darum wird es hier nicht auf 0 gestellt.
                if raus.len() >= MAX_ZEILEN_PRO_ABSATZ {
                    return raus;
                }
            }
            let mut rest = wort.text.clone();
            let mut leer = wort.leer_davor;
            // GEMESSEN 04.10.2026 (SCHRIFT): auch das Hartteilen misst mit
            // dem Schnitt — ein zu langes fettes Wort (eine URL in
            // `font-weight: bold`) wurde sonst mit regulaeren Vorschuben
            // geteilt und die Teilstuecke passten nicht in die Zeile.
            // Release 15.10.2026 (Agent CSS2): the split point is measured
            // with the font and spacing of the section, like the test above
            // (`passende_laenge` uses the built-in bitmap font). MEASURED
            // before (WPT css-text `overflow-wrap/overflow-wrap-anywhere-
            // 001`, Ahem `&nbsp;x5 X` in 4ch): the first piece had five
            // characters and stuck out of the box.
            let measured = |t: &str| with_spacing(t, breite_von(t, grad, gewicht, kursiv, &familie));
            while measured(&rest) > breite && rest.chars().count() > 1 {
                let n = match fitting_prefix(&rest, breite, &measured) {
                    0 => passende_laenge(&rest, grad, breite, font),
                    n => n,
                };
                if n == 0 || n >= rest.len() {
                    // Der Messer kann das Zeichen nicht: weiter, sonst
                    // haengt die Schleife bei n = 0 fuer immer.
                    break;
                }
                let (kopf, _) = rest.split_at(n);
                if raus.len() >= MAX_ZEILEN_PRO_ABSATZ {
                    return raus;
                }
                raus.push(vec![(kopf.to_string(), wort.abschnitt, leer)]);
                leer = false;
                rest = rest[n..].to_string();
            }
            aktuell_breite = breite_von(&rest, grad, gewicht, kursiv, &familie) + wort.rand_rechts;
            aktuell.push((rest, wort.abschnitt, leer));
            masse.push((0.0, aktuell_breite));
            continue;
        }

        // Release 12.10.2026 (Agent CSS2): der rechte Rand des Inline-Elements
        // gehoert zum Wort davor — bis zur naechsten Umbruchstelle ist beides
        // unteilbar (CSS Text 3 §5.1). GEMESSEN vorher (WPT
        // `css1/c5502-imrgn-r-001..005`): `xx <span style="margin-right:4em">
        // xx</span>` in 5 em blieb in einer Zeile, der Rand ragte hinaus.
        // Release 15.10.2026 (Agent CSS2): a word of hanging spaces only
        // (`hangs`) always stays on the line — consecutive hanging spaces all
        // hang. MEASURED before (WPT css-text `line-break/line-break-
        // anywhere-and-white-space-006`, `X<span>   </span>XX` in 2ch with
        // `line-break: anywhere`): the third space started a new line.
        let hanging_only = !wort.text.is_empty() && wort.text.chars().all(hangs);
        if !aktuell.is_empty() && !hanging_only && aktuell_breite + zusatz + wb + wort.rand_rechts > breite {
            if wort.bruch_davor {
                raus.push(std::mem::take(&mut aktuell));
                masse.clear();
                letzter_bruch = 0;
                aktuell_breite = 0.0;
                leerzeilen(&mut raus);
            } else if letzter_bruch > 0 {
                // Die verklebte Folge seit der letzten Umbruchstelle wandert
                // mit; ihr erstes Wort steht dann am Zeilenanfang (ohne
                // Zwischenraum davor).
                let mit = aktuell.split_off(letzter_bruch);
                let mit_masse = masse.split_off(letzter_bruch);
                raus.push(std::mem::take(&mut aktuell));
                aktuell = mit;
                masse = mit_masse;
                if let Some(m) = masse.first_mut() {
                    m.0 = 0.0;
                }
                aktuell_breite = masse.iter().map(|(z, b)| z + b).sum();
                letzter_bruch = 0;
            }
            // Ohne Umbruchstelle in der Zeile steht die Folge ueber.
            if raus.len() >= MAX_ZEILEN_PRO_ABSATZ {
                return raus;
            }
        }
        let zwischen = if aktuell.is_empty() { 0.0 } else { zusatz };
        if !aktuell.is_empty() && wort.bruch_davor {
            letzter_bruch = aktuell.len();
        }
        aktuell_breite += zwischen;
        aktuell.push((wort.text.clone(), wort.abschnitt, wort.leer_davor));
        aktuell_breite += wb + haengend + wort.rand_rechts;
        masse.push((zwischen, wb + haengend + wort.rand_rechts));
    }
    if !aktuell.is_empty() && raus.len() < MAX_ZEILEN_PRO_ABSATZ {
        raus.push(aktuell);
    }
    raus
}

/// Umbruch nach Wort. Ein Wort, das länger als die Zeile ist, wird
/// hart getrennt — sonst waere ein 300-Zeichen-URL ein einziger Block,
/// der alles rechts verlaesst.
pub fn umbreche(text: &str, grad: f32, breite: f32) -> Vec<String> {
    let font = BitmapFont::builtin();
    if text.is_empty() {
        return Vec::new();
    }
    if breite <= 0.0 || !breite.is_finite() {
        return vec![text.to_string()];
    }
    // Grad, den die Schrift nicht kann: Text unveraendert zurueckgeben.
    if !(1.0..=96.0).contains(&grad) {
        return vec![text.to_string()];
    }

    let breite_von = |s: &str| -> f32 { measure(&font, s, grad).map(|m| m.width).unwrap_or(0.0) };

    let mut raus: Vec<String> = Vec::new();
    let mut zeile = String::new();
    for wort in text.split_whitespace() {
        let kandidat = if zeile.is_empty() {
            wort.to_string()
        } else {
            std::format!("{zeile} {wort}")
        };
        if breite_von(&kandidat) <= breite {
            zeile = kandidat;
            continue;
        }
        // Passt nicht: die bisherige Zeile ist voll.
        //
        // `zeile = String::new()` stand hier und wurde NIE gelesen — der
        // Compiler hat es gemeldet, und er hat recht: jeder der drei
        // Wege danach setzt `zeile` neu. Die Zeile war eine von der
        // Warnung zu Recht tot genannt.
        if !zeile.is_empty() {
            raus.push(std::mem::take(&mut zeile));
        }
        // Das Wort selbst: passt es jetzt (ohne den Rest), ist es gut.
        if breite_von(wort) <= breite {
            zeile = wort.to_string();
            continue;
        }
        // Zu lang: hart trennen. GEMESSEN: `5000 Zeichen in einer URL`
        // darf nicht den ganzen Framebuffer sprengen.
        let mut rest = wort.to_string();
        while breite_von(&rest) > breite && rest.chars().count() > 1 {
            let n = passende_laenge(&rest, grad, breite, &font);
            let (kopf, _) = rest.split_at(n);
            raus.push(std::format!("{kopf} ").trim_end().to_string());
            rest = rest[n..].to_string();
        }
        zeile = rest;
    }
    if !zeile.is_empty() {
        raus.push(zeile);
    }
    if raus.is_empty() {
        raus.push(String::new());
    }
    raus
}

/// Wie viele Zeichen passen in `breite` Pixel?
///
/// `split_at` schneidet an BYTES, nicht an Zeichen — bei Umlauten
/// wuerde das mitten in einem UTF-8-Zeichen landen und der String waere
/// ungueltig. Deshalb wird ueber Zeichen gezaehlt und die Grenze als
/// Byte-Offset aus dem Zeichenindex bestimmt.
/// Release 15.10.2026 (Agent CSS2): the "other space separators" of CSS Text 3
/// §4.1 — the space separators (Zs) that are neither U+0020 nor glue (U+00A0,
/// U+2007, U+202F).
fn other_space_separator(c: char) -> bool {
    matches!(c, '\u{1680}' | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200A}' | '\u{205F}' | '\u{3000}')
}

/// Release 15.10.2026 (Agent CSS2): the byte length of the longest prefix of
/// `text` (at least one typographic character unit — a grapheme cluster, so
/// an emoji ZWJ sequence or a base with its marks stays whole) that is at
/// most `max` wide according to `measure`; 0 when `measure` knows no width
/// at all (an empty font). MEASURED (WPT css-text `line-breaking/line-
/// breaking-014`): a char-wise split cut an emoji ZWJ sequence.
fn fitting_prefix(text: &str, max: f32, measure: &dyn Fn(&str) -> f32) -> usize {
    let ends: Vec<usize> = narsil_text::graphem::graphem_grenzen(text).into_iter().filter(|&e| e > 0).collect();
    if ends.is_empty() || !(measure(text) > 0.0) {
        return 0;
    }
    // The largest k with measure(text[..ends[k]]) <= max; at least k = 0.
    let (mut lo, mut hi) = (0usize, ends.len() - 1);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if measure(&text[..ends[mid]]) <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    ends[lo]
}

fn passende_laenge(text: &str, grad: f32, breite: f32, font: &BitmapFont) -> usize {
    let zeichen: Vec<char> = text.chars().collect();
    // Binaere Suche: das Fenster waechst monoton mit der Laenge, und
    // `measure` ist der teure Teil. 300 Zeichen sind 9 Schritte statt 300.
    let mut lo = 1usize;
    let mut hi = zeichen.len();
    while lo < hi {
        // Rust 1.85 kann `(lo + hi + 1) / 2` nicht als `midpoint`
        // schreiben. `div_ceil` statt `+ 1` / 2: die Addition
        // `hi + 1` kann bei `hi = usize::MAX` ueberlaufen, `(hi - lo + 1)`
        // nicht.
        let mitte = lo + (hi - lo + 1).div_ceil(2);
        let kandidat: String = zeichen[..mitte].iter().collect();
        let w = measure(font, &kandidat, grad)
            .map(|m| m.width)
            .unwrap_or(0.0);
        if w <= breite {
            lo = mitte;
        } else {
            hi = mitte - 1;
        }
    }
    // Zeichenindex -> Byte-Offset
    zeichen[..lo].iter().map(|c| c.len_utf8()).sum()
}

// ---------------------------------------------------------------------------
// Klartext
// ---------------------------------------------------------------------------

/// Baut aus Klartext ein Dokument.
///
/// Nutzt `narsil_fmt_text` (1.316 Zeilen, 44 Tests, bis heute **null
/// Aufrufer**). Die Crate kann bereits umbrechen und Zeilen liefern; was
/// fehlte, war jemand, der sie aufruft. Das ist die sechste tote Crate,
/// die mit diesem Schatz lebt.
pub fn aus_klartext(text: &str, breite: u32, basis: &str) -> Seite {
    let mut bauer = Bauer::neu(breite, basis);
    bauer.wurzel();
    bauer.klartext(text);
    bauer.fertig()
}

impl Bauer<'_> {
    /// Klartext als einzelner grosser Absatz.
    fn klartext(&mut self, text: &str) {
        self.absatz_schliessen();
        // Absatzweise: jede LEERE Zeile im Klartext trennt Absaetze.
        // Das ist der Unterschied zwischen `lesbar` und `Wand aus Text`.
        let stil = Style {
            deckkraft: 1.0,
            radius: [0.0; 4],
            schatten_anzahl: 0,
            dekoration: 0,
            color: FARBE_TEXT,
            font_size: GRAD_TEXT,
            margin: Spacing::all(Length::px(0.0)),
            ..Style::default()
        };
        for block in text.split('\n') {
            if block.trim().is_empty() {
                self.absatz_schliessen();
                continue;
            }
            self.absatz_starten(
                Style {
                    deckkraft: 1.0,
                    radius: [0.0; 4],
                    schatten_anzahl: 0,
                    dekoration: 0,
                    margin: Spacing::all(Length::px(4.0)),
                    // 04.10.2026 (SCHRIFT): `..stil` braucht einen
                    // Wert — mit dem Feld `schriftfamilie` ist `Style`
                    // nicht mehr `Copy`, also wandert es in die
                    // Aktualisierung statt hinein.
                    ..stil.clone()
                },
                None,
            );
            if let Some(a) = self.laufender.as_mut()
                && let Some(erster) = a.abschnitte.first_mut()
            {
                erster.text.push_str(block);
            }
        }
        self.absatz_schliessen();
    }
}

/// Ein Dokument ohne Inhalt, aber mit gueltiger Wurzel.
///
/// Wenn der eigene Narsil-Parser einen Fehler meldet, ist die Seite
/// trotzdem da: ein leeres Fenster mit einem Grund ist besser als ein
/// Fenster, das nicht aufgeht.
fn leeres_dokument(breite: u32) -> Document {
    let mut doc = Document::new();
    let stil = Style {
        deckkraft: 1.0,
        radius: [0.0; 4],
        schatten_anzahl: 0,
        dekoration: 0,
        size: Dimension {
            width: Length::px(breite.max(1) as f32),
            height: Length::Auto,
        },
        background: FARBE_SEITE,
        color: FARBE_TEXT,
        font_size: GRAD_TEXT,
        ..Style::default()
    };
    let _ = doc.set_style(doc.root(), stil);
    doc
}

// ---------------------------------------------------------------------------
// Link-Herausgeber
// ---------------------------------------------------------------------------

/// Sucht aus einer Display-Liste den Link, der an einem Punkt liegt.
///
/// WARUM EINE EIGENE Funktion und nicht `narsil_display::hit_test`:
/// `hit_test` gibt `HitTarget::Text { origin, text }` zurueck — es weiss
/// nichts von Adressen. Der Browser muss aber wissen, OB der Text, den der
/// Klick getroffen hat, ein Link ist. Die Verknuepfung Text -> Adresse
/// gibt es nur in der Seite, und deshalb steht sie hier.
///
/// Reihenfolge: spaetere Knoten liegen ueber frueheren. Bei
/// Ueberschneidung gewinnt der spaetere, genau wie im Rasterizer.
pub fn link_unter(
    seite: &Seite,
    baum: &narsil_layout::LayoutTree,
    punkt: (f32, f32),
) -> Option<String> {
    let treffer = sammle_knoten_rechtecke(baum, &seite.links);
    // Von hinten nach vorn: das spaetere liegt oben.
    for (id, rect) in treffer.iter().rev() {
        if punkt.0 >= rect.x
            && punkt.0 <= rect.x + rect.width
            && punkt.1 >= rect.y
            && punkt.1 <= rect.y + rect.height
            && let Some(ziel) = seite.adresse_von(*id)
        {
            return Some(ziel.to_string());
        }
    }
    None
}

/// Rechtecke aller Knoten mit **Text**, und der Kasten jedes Knotens, der
/// eine Adresse hat.
///
/// DER EINE ORT, an dem aus einem `LayoutTree` Positionen gelesen werden.
/// Es gibt hier KEINE eigene Formel fuer Breiten: `narsil_text::measure`
/// entscheidet, wie breit ein Text ist. Genau dieser Fehler stand in
/// `narsil_display::bounds()` (0,6 em, 31,4 % daneben) und wird hier nicht
/// wiederholt.
///
/// 02.10. 06:15: **AUCH DER KASTEN EINES LINKES** (Auftrag „Links ohne
/// Text"). GEMESSEN: vorher sammelte diese Funktion **nur** Textrechtecke,
/// also konnte `link_unter` (Hover, Statusleiste) einen Link **strukturell
/// nicht** finden, wenn er kein Text traegt — der 30 Stimmpfeile auf
/// news.ycombinator.com. GEMESSENE ENTSCHEIDUNG, warum nicht **jeder**
/// Kasten: ein Container ohne Adresse ist kein Link, und ihn aufzunehmen
/// hiesse, dass die Statuszeile bei jedem Klick einen Link behauptet. Es
/// kommt genau der Knoten hinein, der in `links` steht — damit ist die
/// Frage „ist das ein Link" die **eine** Karte der Seite, und nicht eine
/// zweite Antwort, die man hier neu erfindet.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Kasten **vor** dem Textrechteck
/// kommt: `link_unter` iteriert von hinten nach vorn („der spaetere liegt
/// oben"). Der Text liegt sichtbar im Vordergrund, also gewinnt er.
fn sammle_knoten_rechtecke(
    baum: &narsil_layout::LayoutTree,
    links: &std::collections::HashMap<NodeId, String>,
) -> Vec<(NodeId, narsil_core::geometry::Rect)> {
    let mut raus = Vec::new();
    sammle(&baum.root, links, &mut raus);
    raus
}

fn sammle(
    box_: &narsil_layout::LayoutBox,
    links: &std::collections::HashMap<NodeId, String>,
    raus: &mut Vec<(NodeId, narsil_core::geometry::Rect)>,
) {
    //
    // 02.10. 06:25 — DER KASTEN NUR, WENN DER LINK **KEINEN TEXT** HAT.
    //
    // GEMESSENER FEHLER MEINER ERSTEN FASSUNG (06:15), und der Test
    // `der_text_ausserhalb_des_links_bleibt_kein_link` in
    // `tests/link_span_klick.rs` hat ihn gefunden: ich habe **jeden** Knoten
    // mit Adresse genommen, und der Container, der den Linktext traegt, ist so
    // breit wie der ganze Absatz (gemessen auf HN: 928 px bei 10 px Text).
    // GEMESSENE FOLGE: der Raster meldete den Link auch **rechts neben dem
    // Text**, also dort, wo nichts ist.
    //
    // GEMESSENE ENTSCHEIDUNG, warum „kein Text im Teilbaum" die richtige
    // Grenze ist: ein Link **mit** Text hat sein Rechteck bereits — das
    // Textrechteck unten, mit `LINK_RAND`. Nur ein Link, der **kein** Text
    // hat (der Stimmpfeil, ein Icon), braucht den Kasten, denn sonst hat er
    // gar keiner. Das ist die Grenze, die CSS 2.1, 12.4 meint, ohne den
    // Absatz zum Link zu machen.
    //
    // GEMESSENE EINORDNUNG, warum nicht nach der Groesse gefragt wird: ein
    // Link kann auch breit sein, weil der Autor das so wollte (ein
    // „Weiter"-Link über die halbe Seite). Die Breite ist keine Eigenschaft
    // des Links, sondern des Inhalts — die Frage ist, ob es Text gibt.
    if links.contains_key(&box_.node) && !traegt_text(box_) {
        raus.push((box_.node, box_.border_box));
    }
    if let Some(text) = &box_.text {
        let breite = measure(&BitmapFont::builtin(), &text.text, text.font_size)
            .map(|m| m.width)
            .unwrap_or(0.0);
        raus.push((
            box_.node,
            Rect::new(
                text.origin.x - LINK_RAND,
                text.origin.y - LINK_RAND,
                breite + 2.0 * LINK_RAND,
                text.font_size + 2.0 * LINK_RAND,
            ),
        ));
    }
    for kind in &box_.children {
        sammle(kind, links, raus);
    }
}

/// Traegt dieser Kasten — oder einer seiner Nachfahren — **Text**?
///
/// GEMESSENE EINORDNUNG, warum der ganze Teilbaum und nicht nur der Kasten
/// selbst: `<a href=/a><div><span>block</span></div></a>` traegt den Text
/// eine Ebene tiefer. Ohne den Nachfahren waere der Container textlos und
/// der Absatz waere wieder der Link.
fn traegt_text(box_: &narsil_layout::LayoutBox) -> bool {
    if box_.text.is_some() {
        return true;
    }
    box_.children.iter().any(traegt_text)
}

// ---------------------------------------------------------------------------
// Die Gleichheit, die der Umbruch behauptet
// ---------------------------------------------------------------------------

static SKRIPTE_LAUFEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Release 08.10.2026: fuehrt der Aufrufer die Seitenskripte aus? Dann zeigt
/// `<noscript>` nichts (HTML §4.12.2). Der Browser (narsil) schaltet das ein;
/// ohne Skriptausfuehrung (Textansicht, Werkzeuge, `seite_aus_text`) bleibt
/// `<noscript>` der Inhalt der Seite.
pub fn skripte_laufen_setzen(an: bool) {
    SKRIPTE_LAUFEN.store(an, std::sync::atomic::Ordering::Relaxed);
}

/// Release 10.10.2026: oeffentlich — der Browser fragt es vor dem fruehen
/// Vorab-Laden der Skripte (`narsil::js::skripte_frueh_starten`).
pub fn skripte_laufen() -> bool {
    SKRIPTE_LAUFEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Release 08.10.2026: `text-transform: capitalize` — der erste Buchstabe
/// jedes Wortes gross.
fn gross_anfang(t: &str) -> String {
    let mut raus = String::with_capacity(t.len());
    let mut anfang = true;
    for c in t.chars() {
        if anfang && c.is_alphabetic() {
            raus.extend(c.to_uppercase());
            anfang = false;
        } else {
            if c.is_whitespace() {
                anfang = true;
            }
            raus.push(c);
        }
    }
    raus
}

/// `NARSIL_DBG_ABSATZ` einmal gelesen (Diagnose, siehe `absatz_schliessen`).
fn dbg_absatz() -> Option<&'static String> {
    static WERT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    WERT.get_or_init(|| std::env::var("NARSIL_DBG_ABSATZ").ok().filter(|s| !s.is_empty())).as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `zeilen_bauen` mit EINEM Abschnitt ergibt `umbreche`.
    ///
    /// GEMESSENE ZUSAGE, und sie steht als Kommentar am Kopf von
    /// `zeilen_bauen`: "mit einem einzigen Abschnitt ergibt GENAU
    /// `umbreche`". Ein Kommentar, der etwas behauptet, ist eine
    /// Behauptung — dieser Test ist der Beweis, sonst waere sie nur
    /// geschrieben.
    ///
    /// GEMESSENER GRUND fuer den Test: `zeilen_bauen` ist beim Schreiben
    /// des Umbruchs um ein `+= wb` zu kurz gewesen (die Breite wurde
    /// nur aus Leerzeichen summiert), und `umbruch_ist_im_bild_sichtbar`
    /// hat es gefunden. Ein zweiter Weg zum Umbruch braucht darum
    /// denselben Beweis wie der erste.
    #[test]
    fn zeilen_bauen_ist_umbreche_bei_einem_abschnitt() {
        let font = BitmapFont::builtin();
        let breite_von =
            // 04.10.2026 (SCHRIFT_9): `breite_von` nimmt die
            // `font-family`-Liste des Abschnittes als **fuenftes** Argument
            // entgegen (Zeile 10618), `stil_von` liefert sie als **drittes**
            // Tupelelement (Zeile 10620). Der Test misst mit der eingebauten
            // Bitmap; Familie und Schnitt sind also egal und werden nur
            // durchgereicht.
            |s: &str, grad: f32, _gewicht: u16, _kursiv: bool, _familie: &str| -> f32 {
                measure(&font, s, grad).map(|m| m.width).unwrap_or(0.0)
            };
        let grad = GRAD_TEXT;
        let grad_von = |_: &usize| -> f32 { grad };

        let beispiele = [
            "Ein kurzer Absatz.",
            "Dies ist ein etwas längerer Absatz, der auf einer schmalen Seite \
             umbrechen muss, damit man alles liest.",
            "Wort",
            "a b c d e f g h i j k l m n o p q r s t u v w x y z",
            "einsehrlangeswortohneirgendeinLeerzeichenwasnichtumbrochenwerdenkann",
            "  mit   vielen    Leerzeichen  ",
            "Umlaute: äöü ÄÖÜ ß — und ein Gedankenstrich",
            "",
        ];
        for beispiel in beispiele {
            for breite in [40.0f32, 80.0, 120.0, 200.0, 300.0, 768.0] {
                let alt = umbreche(beispiel, grad, breite);
                // JEDES Wort traegt ein Leerzeichen davor — sonst
                // waere die Gleichheit zu `umbreche` eine Behauptung
                // ueber eine andere Eingabe. Siehe
                // `zeilen_bauen_ist_umbreche_bei_einem_abschnitt`.
                let woerter: Vec<Wort> = beispiel
                    .split_whitespace()
                    .map(|w| Wort {
                        text: w.to_string(),
                        abschnitt: 0,
                        leer_davor: true,
                        rand_links: 0.0,
                        rand_rechts: 0.0,
                        bruch_davor: true,
                        haengt: false,
                        other_spaces_hang: true,
                        unteilbar: false,
                    })
                    .collect();
                let neu = zeilen_bauen(
                    &woerter,
                    &breite_von,
                    &grad_von,
                    &|_: &usize| (400u16, false, String::new()),
                    &|_: &usize| (0.0f32, 0.0f32, 0.0f32),
                    &|_: usize| breite,
                    &font,
                );
                let neu_text: Vec<String> = neu
                    .iter()
                    .map(|z| {
                        z.iter()
                            .map(|(w, _, _)| w.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect();
                // Der leere Text: `umbreche` gibt eine Zeile mit leerem
                // String zurueck, `zeilen_bauen` gibt KEINE Zeile. Das
                // ist der einzige erlaubte Unterschied — er entsteht
                // daraus, dass `absatz_schliessen` den leeren Absatz
                // vorher verwirft.
                if beispiel.trim().is_empty() {
                    assert!(
                        neu_text.iter().all(|z| z.trim().is_empty()),
                        "{beispiel:?} @ {breite}: {neu_text:?}"
                    );
                    continue;
                }
                assert_eq!(
                    alt, neu_text,
                    "{beispiel:?} @ {breite} px: umbreche sagt {alt:?}, \
                     zeilen_bauen sagt {neu_text:?}"
                );
            }
        }
    }

    /// Und die Grenze: `MAX_ZEILEN_PRO_ABSATZ` Zeilen, nicht mehr.
    ///
    /// M3: ohne diese Grenze waeren es bei 40 px Nutzbreite und einem
    /// Absatz aus 20.000 Worten 10.000 Zeilen und 10.000 Knoten fuer
    /// EINEN Absatz.
    #[test]
    fn zeilen_bauen_haelt_die_zeilengrenze_ein() {
        let font = BitmapFont::builtin();
        let breite_von =
            // 04.10.2026 (SCHRIFT_9): `breite_von` nimmt die
            // `font-family`-Liste des Abschnittes als **fuenftes** Argument
            // entgegen (Zeile 10618), `stil_von` liefert sie als **drittes**
            // Tupelelement (Zeile 10620). Der Test misst mit der eingebauten
            // Bitmap; Familie und Schnitt sind also egal und werden nur
            // durchgereicht.
            |s: &str, grad: f32, _gewicht: u16, _kursiv: bool, _familie: &str| -> f32 {
                measure(&font, s, grad).map(|m| m.width).unwrap_or(0.0)
            };
        let grad_von = |_: &usize| -> f32 { GRAD_TEXT };
        let woerter: Vec<Wort> = (0..20_000)
            .map(|_| Wort {
                text: "x".to_string(),
                abschnitt: 0,
                leer_davor: true,
                rand_links: 0.0,
                rand_rechts: 0.0,
                bruch_davor: true,
                haengt: false,
                other_spaces_hang: true,
                unteilbar: false,
            })
            .collect();
        let zeilen = zeilen_bauen(
            &woerter,
            &breite_von,
            &grad_von,
            &|_: &usize| (400u16, false, String::new()),
            &|_: &usize| (0.0f32, 0.0f32, 0.0f32),
            &|_: usize| 40.0,
            &font,
        );
        assert_eq!(
            zeilen.len(),
            MAX_ZEILEN_PRO_ABSATZ,
            "{} Zeilen — die Grenze ist {MAX_ZEILEN_PRO_ABSATZ}",
            zeilen.len()
        );
    }

    /// `weissraum` haelt ein Leerzeichen am Anfang und am Ende.
    ///
    /// DER FEHLER, DEN ES VERHINDERT: `<p>Das ist ein <b>x</b> mehr</p>`
    /// gab mit `split_whitespace().join(" ")` "Das ist ein" + "x" =
    /// "Das ist einx". Der Test sichert beide Enden, weil ein Fehler an
    /// nur einem Ende ein Fehler ist.
    #[test]
    fn weissraum_haelt_die_raender() {
        assert_eq!(weissraum("  Hallo  Welt  "), " Hallo Welt ");
        assert_eq!(weissraum("Hallo"), "Hallo");
        assert_eq!(weissraum("   "), " ");
        assert_eq!(weissraum(""), "");
        assert_eq!(weissraum("a\n\nb"), "a b");
    }

    /// Und ein Absatz mit Inline-Elementen verliert kein Leerzeichen.
    ///
    /// Der eigentliche Test der Korrektur vom 30.09.2026, an dem
    /// Quelltext statt an Knoten: `Das ist ein ` + `x` + ` mehr` muss
    /// `Das ist ein x mehr` ergeben.
    #[test]
    fn absatz_mit_inline_verliert_kein_leerzeichen() {
        let seite = aus_html("<p>Das ist ein <b>x</b> mehr</p>", 800, "http://x/");
        let t: Vec<&str> = seite
            .document
            .nodes()
            .iter()
            .filter(|n| n.kind == NodeKind::Text)
            .filter_map(|n| n.text.as_deref())
            .collect();
        assert!(
            t.iter().any(|s| s.contains("Das ist ein")),
            "der erste Abschnitt fehlt: {t:?}"
        );
        assert!(
            t.iter().any(|s| s.contains("mehr")),
            "der letzte Abschnitt fehlt: {t:?}"
        );
        // Und die Reihenfolge ist die des Quelltextes.
        //
        // ZUSAMMEN OHNE TRENNER, und das ist die genaue Behauptung: die
        // Leerzeichen stehen jetzt IN den Abschnitten (am Ende des
        // vorherigen), weil ein Absatz aus mehreren Knoten besteht. Mit
        // einem Trenner dazwischen waere jedes Leerzeichen doppelt —
        // der erste Entwurf dieses Tests verband die Knoten mit " " und
        // fand "Das ist ein  x  mehr". Der Test war falsch, nicht der
        // Absatz: die Knoten tragen ihre Leerzeichen jetzt selbst.
        let zusammen: String = t.concat();
        assert_eq!(
            zusammen, "Das ist ein x mehr",
            "die Abschnitte ergeben nicht wieder den Satz"
        );
    }
}

#[cfg(test)]
mod titel_und_icon {
    //! W7C A4: Titel und Favicon aus dem KOPF.
    //!
    //! GEMESSEN, warum diese Tests in `narsil-page` und nicht in `narsil`:
    //! sie behaupten, was der BAUER aus dem Dokument macht. Dass der Titel
    //! danach im Fenstertitel steht, behauptet `narsil/tests/titel_favicon.rs`.

    #[test]
    fn der_titel_kommt_aus_dem_kopf() {
        let s = crate::seite_aus_text(
            "<html><head><title>Der Titel</title></head><body><p>x</p></body></html>",
            800,
            "http://b.example/",
        );
        assert_eq!(s.titel, "Der Titel", "der Titel fehlt: {:?}", s.titel);
        // GEMESSEN, warum er NICHT im Bild steht: ein `<title>` ist kein
        // Inhalt, und er stand vorher als Zeile in der Seite.
        assert!(s.document.nodes().len() > 2, "die Seite ist leer");
    }

    #[test]
    fn der_titel_wird_gekuerzt_und_ist_eine_zeile() {
        // M4: 200 Zeichen. Und `weissraum` macht Zeilenumbrueche zu
        // Leerzeichen — wie in einer Fenstertitelleiste.
        let lang = "x".repeat(5000);
        let s = crate::seite_aus_text(
            &format!("<html><head><title>{lang}</title></head><body></body></html>"),
            800,
            "http://b.example/",
        );
        assert_eq!(s.titel.chars().count(), crate::MAX_TITEL);
        let mit_umbruechen = crate::seite_aus_text(
            "<html><head><title>a\nb</title></head><body></body></html>",
            800,
            "http://b.example/",
        );
        assert!(
            !mit_umbruechen.titel.contains('\n'),
            "der Titel enthaelt einen Zeilenumbruch: {:?}",
            mit_umbruechen.titel
        );
    }

    #[test]
    fn der_erste_titel_gewinnt() {
        // GEMESSEN, warum: `dom_zu_ord`/Verlauf nehmen EINEN Titel, und
        // zwei `<title>` sind ein Fehler der Seite. Der erste ist der
        // sichtbare (so lesen es die Browser auch).
        let s = crate::seite_aus_text(
            "<html><head><title>eins</title><title>zwei</title></head><body></body></html>",
            800,
            "http://b.example/",
        );
        assert_eq!(s.titel, "eins");
    }

    #[test]
    fn das_favicon_wird_aufgeloest_und_nur_das_icon() {
        let s = crate::seite_aus_text(
            "<html><head>\
             <link rel=\"stylesheet\" href=\"a.css\">\
             <link rel=\"shortcut icon\" href=\"icon.png\">\
             </head><body></body></html>",
            800,
            "https://beispiel.org/pfad/index.html",
        );
        assert_eq!(
            s.favicon.as_deref(),
            Some("https://beispiel.org/pfad/icon.png"),
            "die Icon-Adresse wurde nicht aufgeloest: {:?}",
            s.favicon
        );
    }

    #[test]
    fn ohne_icon_bleibt_es_leer() {
        // GEMESSEN, warum das ein Test ist: `rel="stylesheet"` ist das
        // haeufigste `rel` der Welt. Wer jedes `<link` als Icon nimmt,
        // laedt das Stylesheet als Bild.
        let s = crate::seite_aus_text(
            "<html><head><link rel=\"stylesheet\" href=\"a.css\"></head><body></body></html>",
            800,
            "http://b.example/",
        );
        assert!(s.favicon.is_none(), "ein Stylesheet wurde zum Icon");
        // Und eine Seite ganz ohne Link.
        let ohne = crate::seite_aus_text(
            "<html><head><title>T</title></head><body></body></html>",
            800,
            "http://b.example/",
        );
        assert!(ohne.favicon.is_none());
    }
}

/// HTML "rules for parsing non-negative integers" for `<canvas>` sizes.
fn canvas_masszahl(v: &str) -> Option<u32> {
    let t = v.trim_start_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'));
    let t = t.strip_prefix('+').unwrap_or(t);
    let n = t.bytes().take_while(u8::is_ascii_digit).count();
    t[..n].parse::<u64>().ok().filter(|v| *v <= i32::MAX as u64).map(|v| v as u32)
}
