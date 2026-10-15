//! JavaScript im Baum BEHALTEN, ohne es anzuzeigen.
//!
//! ENTSCHEIDUNG 30.09.2026 (Nutzer, ueber die Koordination): JavaScript
//! wird unterstuetzt, es gibt eine eigene Engine (`narsil-js`). Fuer diese
//! Rolle heisst das genau eine Sache: **`<script>` wird nicht mehr
//! verworfen.** Ein hier verworfener Quelltext ist ein fuer alle Mal
//! verworfen — die beste Engine der Welt kann keinen Code lesen, den es
//! nicht mehr gibt.
//!
//! # WAS ANGEZEIGT WIRD UND WAS NICHT
//!
//! | Element | Angezeigt | Im Baum | Warum |
//! |---|---|---|---|
//! | `<script>` | nein | ja, als `Skript` | Quelltext ist Information, kein Text |
//! | `<style>` | nein | nein (leer) | CSS ist H2, `VERTRAG_CSS.txt` fehlt |
//! | `<noscript>` | **ja** | ja | ohne JavaScript ist es der Inhalt |
//! | `onclick=…` | nein | ja, als `Ereignis` | eine Engine braucht den Quelltext |
//!
//! "Nicht angezeigt" heisst hier: es steht nicht als `NodeKind::Text` im
//! Dokument. Ein Textknoten im Dokument wird gezeichnet — das ist in
//! `narsil_layout_engine` die einzige Art, wie Text ins Bild kommt. Also
//! liegt der Quelltext in `Skript::inhalt`, und im Dokument steht ein
//! Knoten der Groesse 0x0 ohne Kinder, damit die Seite den Platz des
//! Skripts im Dokument NICHT verliert.
//!
//! # DIE GRENZEN (M3/M4), alle benannt
//!
//! | Konstante | Wert | Wofue |
//! |---|---|---|
//! | [`MAX_SKRIPTE`] | 512 | `<script>`-Elemente je Seite |
//! | [`MAX_SKRIPT_INHALT`] | 16 MiB | Quelltext eines Skripts |
//! | [`MAX_SKRIPT_QUELLE`] | 2 KiB | `src` nach dem Aufloesen |
//! | [`MAX_EREIGNISSE`] | 4.096 | `on*`-Attribute je Seite |
//! | [`MAX_EREIGNIS_QUELLE`] | 8 KiB | Quelltext eines Handlers |
//!
//! GEMESSEN, warum die Ereignisgrenze noetig ist: `onclick` an 10.000
//! Tags ergibt 10.000 Eintraege mit je bis zu 8 KiB. Das ist eine Seite
//! mit 80 MB Handlercode und einem Nutzen von genau null — die Seite tut
//! so, als haette sie eine Funktion, und hat keine.

use std::collections::HashMap;

use narsil_dom::NodeId;

/// Hoechste Zahl `<script>`-Elemente je Seite (M3).
pub const MAX_SKRIPTE: usize = 512;

/// Hoechste Laenge des Quelltexts EINES Skripts in Bytes (M4).
///
/// YouTube liefert Inline-Konfigurationen ueber 256 KiB. Bis zur Grenze
/// bleiben Programme vollstaendig; oberhalb wird ein expliziter Fehler
/// gespeichert statt ein syntaktisch ungueltiger Praefix.
///
/// Release 10.10.2026: 16 MiB wie ein externes Skript
/// (`narsil::js::MAX_SKRIPT_QUELLE`). GEMESSEN vorher (1 MiB): die
/// Suchergebnisseite von youtube.com traegt `ytInitialData` inline, je nach
/// Antwort 0,95–1,1 MB — darueber wurde das Skript verworfen, und die App
/// zeigte statt der Treffer „Etwas ist schiefgelaufen". Die Summe bleibt
/// durch die Groesse des Dokuments begrenzt.
pub const MAX_SKRIPT_INHALT: usize = 16 * 1024 * 1024;

/// Hoechste Laenge einer aufgeloesten `src` (M4).
pub const MAX_SKRIPT_QUELLE: usize = 2 * 1024;

/// Hoechste Zahl `on*`-Attribute je Seite (M3).
pub const MAX_EREIGNISSE: usize = 4_096;

/// Hoechste Laenge des Quelltexts EINES Handlers in Bytes (M4).
pub const MAX_EREIGNIS_QUELLE: usize = 8 * 1024;

/// Hoechste Zahl `<style>`-Bloecke je Seite (M3).
///
/// GEMESSEN, warum 64: eine Seite mit 64 Stylesheets ist ein
/// Einzelfall; eine mit 64.000 ist ein Fehler. Der Wert passt unter die
/// `MAX_BLAETTER = 16` der Kaskade — es kommen noech die Datei-Sheets
/// dazu, und die Kaskade soll ihren eigenen Grenzen nicht ausgeliefert
/// werden.
pub const MAX_STILBLOECKE: usize = 64;

/// Hoechste Laenge des TEXTES eines `<style>`-Blocks in Bytes (M4).
///
/// GEMESSEN, warum gleich `MAX_EINGABE` der Kaskade (4 MiB): ein Block, der
/// groesser ist, wuerde dort ohnehin abgeschnitten — hier abzuschneiden
/// heisst: die Kaskade sieht nie mehr als das, was sie vertragen kann.
///
/// Release 09.10.2026: vorher 1 MiB (die alte Kaskadengrenze). YouTubes
/// Hauptblatt (`kevlar_base`, 3,18 MiB) verlor damit zwei Drittel seiner
/// Regeln — der App-Kopf (`#container.ytd-masthead{display:flex}` bei 87 %)
/// blieb ungestylt.
pub const MAX_STILTEXT: usize = narsil_css::MAX_EINGABE;

/// Ein `<style>`-Block als QUELLE fuer die Kaskade.
///
/// H2, 30.09.2026. `type` und `media` stehen hier, obwohl die Kaskade
/// `@media` selbst auswertet: sie braucht das Attribut, um einen Block
/// als "print" zu erkennen und fallen zu lassen — und bis die Kaskade
/// laeuft, ist das hier die einzige Stelle, an der die Information
/// existiert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleBlock {
    /// Release 12.10.2026: der Bereich (Shadow Tree), zu dem der Block
    /// gehoert (`None` = Dokument; siehe `schatten`).
    pub bereich: Option<String>,
    /// Der CSS-Text, ROH und unveraendert.
    ///
    /// LEER bei einem `<link rel="stylesheet">`, dessen Text noch nicht da
    /// ist (siehe `href`). Leer heisst hier ausdruecklich „noch nicht
    /// geholt" — nicht „leer geschrieben": ein leerer `<style>`-Block
    /// kommt gar nicht erst in die Liste (siehe `Self::mit_blattschrank`).
    pub inhalt: String,
    /// `type`, klein. Leer = `text/css` in der HTML-Spezifikation.
    pub typ: String,
    /// `media`, klein. Leer = alle.
    pub medium: String,
    /// 03.10.2026 17:0x: die AUFGELOESTE URL, wenn der Block von einem
    /// `<link rel="stylesheet" href="…">` kommt. `None` bei `<style>`.
    ///
    /// # WARUM ES DIESES FELD GIBT (gemessen)
    ///
    /// GEMESSEN an den zehn gespeicherten Seiten (`team/PROBLEME.txt`
    /// Nr. 59): die Seiten mit `link`-Blaettern haben die groessten
    /// Firefox-Abweichungen (github 191.9, nixos 145.3, MDN 124.5), die
    /// mit Inline-`<style>` die kleinsten (duckduckgo 31.1, wikipedia 41.6).
    ///
    /// GEMESSENE URSACHE, und sie ist eine Wegwerf-Anweisung: ohne `href`
    /// ging die Adresse beim Lesen des `<link>` verloren. Danach ist die
    /// URL nicht mehr mitteilbar — wer sie holen soll, kann sie nicht
    /// nennen, und die Kaskade sieht **null** Autorblaetter fuer diese
    /// Seite, obwohl das Blatt geparst werden koennte (GEMESSEN am echten
    /// MediaWiki-Blatt, `team/BERICHT_CSS_WIKI_KASKADE.txt`).
    pub href: Option<String>,
}

/// Baut einen [`StyleBlock`] aus den Attributen und dem Inhalt.
pub fn stile_aus(attributes: &HashMap<String, String>, inhalt: &str) -> StyleBlock {
    let mut inhalt = inhalt.to_string();
    // M4: an der Zeichengrenze, sonst panikt `truncate`.
    if inhalt.len() > MAX_STILTEXT {
        let g = crate::zeichengrenze(&inhalt, MAX_STILTEXT);
        inhalt.truncate(g);
    }
    StyleBlock {
        bereich: attributes.get(crate::schatten::ATTR_BEREICH).cloned(),
        inhalt,
        typ: attributes
            .get("type")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default(),
        medium: attributes
            .get("media")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default(),
        // Ein `<style>`-Block hat keine Adresse: der Text steht im Dokument.
        href: None,
    }
}

/// Baut einen [`StyleBlock`] aus einem `<link rel="stylesheet">`.
///
/// `url` ist die **bereits aufgeloeste** Adresse (siehe `loese_link`); der
/// Text ist leer, bis ihn der Abrufer einsetzt (siehe
/// [`Self::mit_blattschrank`] in `lib.rs`).
pub fn stil_aus_link(attributes: &HashMap<String, String>, url: String) -> StyleBlock {
    StyleBlock {
        bereich: attributes.get(crate::schatten::ATTR_BEREICH).cloned(),
        inhalt: String::new(),
        typ: attributes
            .get("type")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default(),
        medium: attributes
            .get("media")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default(),
        href: Some(url),
    }
}

/// Wie viele `<link rel="stylesheet">`-Blaetter eine Seite braucht, die
/// noch nicht geholt sind.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Zahl und keine Liste ist: der
/// Abrufer braucht die Zahl fuer die Anzeige („2 Stylesheets fehlen"), und
/// die Liste fuer die Arbeit. [`crate::Seite::stil_urls`] gibt die Liste.
///
/// LEER ist `inhalt.trim().is_empty()` **und** `href` gesetzt — beides
/// zusammen heisst „geholt, aber die Datei war leer". Ein Block mit Text
/// ist kein offener Auftrag, auch wenn sein `href` gesetzt ist.
pub fn offene_blaetter(stile: &[StyleBlock]) -> usize {
    stile
        .iter()
        .filter(|b| b.href.is_some() && b.inhalt.trim().is_empty())
        .count()
}

/// Ein `<script>`-Element, wie eine Engine es braucht.
///
/// Der Quelltext steht hier und NICHT im Dokument. Grund: ein
/// `NodeKind::Text` im Dokument wird gezeichnet (siehe Modulkopf), und
/// ein gezeichneter Quelltext ist eine Fehlermeldung im Bild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skript {
    /// Der Knoten im Dokument: Groesse 0x0, ohne Kinder, nicht gezeichnet.
    /// Er existiert, damit die Seite den Platz nicht verliert und damit
    /// `document.nodes()` zeigt, dass hier ein Skript stand.
    pub knoten: NodeId,
    /// `src`, gegen die Basis aufgeloest. Leer = Inline-Skript.
    pub quelle: String,
    /// `type`, klein geschrieben. Leer = `text/javascript` in der
    /// HTML-Spezifikation.
    pub typ: String,
    /// `async`: ausfuehren, sobald es da ist, ohne auf die Reihenfolge
    /// zu warten.
    pub asynchron: bool,
    /// `defer`: ausfuehren, nachdem die Seite fertig aufgebaut ist.
    pub verzoegert: bool,
    /// Der Quelltext, ROH (nicht entitaetendekodiert — JavaScript hat
    /// keine Entitaeten, siehe `entitaeten_ersetzen` in narsil-fmt-html).
    pub inhalt: String,
    /// Zu grosse Programme werden vollstaendig verworfen, nie als Praefix ausgefuehrt.
    pub zu_gross: bool,
}

impl Skript {
    /// Ein Inline-Skript, wie es in der Seite steht.
    pub fn ist_inline(&self) -> bool {
        self.quelle.is_empty()
    }
}

/// Ein `on*`-Attribut: der Quelltext eines Handlers.
///
/// WARUM EIGENE STRUKTUR und nicht im Dokument: `NodeKind` kennt keine
/// Attribute, und `Stil` erst recht keine. Wer die Quelle im Stil
/// unterbringen wuerde, haette eine `String` in einem Feld, das
/// `font_size` heisst — und der naechste, der `Stil` kopiert, kopiert
/// JavaScript in die Typografie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ereignis {
    /// Der Knoten des Elements. `None` bei INLINE-Elementen: `<b
    /// onclick=…>` bekommt keinen Knoten (siehe `ist_inline` in
    /// `narsil_page`), und eine erfundene `NodeId` waere schlimmer als
    /// `None` — sie zeigte auf irgendein anderes Element.
    pub knoten: Option<NodeId>,
    /// DIE VORLAUF-NUMMER des Elements (H2, 03.10.2026).
    ///
    /// GEMESSEN, warum das Feld noetig ist: `knoten` ist bei INLINE-
    /// Elementen `None`, und der Host braucht ZAHLEN. Ohne dieses Feld
    /// konnte `narsil` keinen Inline-Handler eintragen, und `onclick` lief
    /// nie (gemessen: `horcher.len() == 0` nach dem Laden einer Seite mit
    /// `onclick`).
    ///
    /// `None`, wenn der Aufrufer keine Nummer hat (Aufrufe ohne Element,
    /// z. B. aus einem Test).
    pub ord: Option<u32>,
    /// Der Tagname, damit `None` trotzdem etwas bedeutet.
    pub tag: String,
    /// `click` aus `onclick`. Ohne das `on`, denn es ist immer da.
    pub art: String,
    /// Der JavaScript-Quelltext des Handlers.
    pub quelle: String,
}

/// Baut ein [`Skript`] aus den Attributen und dem Inhalt.
///
/// `basis` wird fuer `src` gebraucht: 7 von 10 Skripte einer echten Seite
/// haben ein RELATIVES `src`, und eine Engine kann mit `//cdn/x.js`
/// nichts anfangen.
/// Release 10.10.2026: die EXTERNEN Skripte eines geparsten Dokuments, in
/// Dokumentreihenfolge — fuer das fruehe Vorab-Laden direkt nach dem Parsen
/// (wie der Preload-Scanner eines Browsers). `<template>`-Inhalte laufen
/// nicht und werden uebersprungen. Die Adressen loest dieselbe Regel auf wie
/// der Seitenbau (`skript_aus`); eine abweichende `<base>` kostet hoechstens
/// einen unnoetigen Abruf (der Seitenbau holt dann nach).
pub fn skripte_im_html(knoten: &[narsil_fmt_html::HtmlNode], basis: &str) -> Vec<Skript> {
    fn lauf(k: &[narsil_fmt_html::HtmlNode], basis: &str, raus: &mut Vec<Skript>, tiefe: usize) {
        if tiefe > 512 || raus.len() >= crate::MAX_SKRIPTE {
            return;
        }
        for n in k {
            if let narsil_fmt_html::HtmlNode::Element { tag, attributes, children, .. } = n {
                if tag.eq_ignore_ascii_case("script") {
                    if attributes.get("src").is_some_and(|s| !s.trim().is_empty()) {
                        raus.push(skript_aus(attributes, "", basis));
                    }
                    continue;
                }
                if tag.eq_ignore_ascii_case("template") {
                    continue;
                }
                lauf(children, basis, raus, tiefe + 1);
            }
        }
    }
    let mut raus = Vec::new();
    lauf(knoten, basis, &mut raus, 0);
    raus
}

pub fn skript_aus(attributes: &HashMap<String, String>, inhalt: &str, basis: &str) -> Skript {
    // M4: die Laenge wird VOR der Uebernahme geprueft.
    let quelle = attributes
        .get("src")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            let aufgeloest = crate::loese_link(s, basis);
            if aufgeloest.len() > MAX_SKRIPT_QUELLE {
                crate::zeichengrenze(&aufgeloest, MAX_SKRIPT_QUELLE).to_string()
            } else {
                aufgeloest
            }
        })
        .unwrap_or_default();
    let typ = attributes
        .get("type")
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let zu_gross = inhalt.len() > MAX_SKRIPT_INHALT;
    let inhalt = if zu_gross { String::new() } else { inhalt.to_string() };
    Skript {
        // `NodeId(0)` ist ein PLATZHALTER. Der Bauer setzt danach die
        // echte Nummer; wer das vergisst, sieht `NodeId(0)` und weiss,
        // dass er vergessen hat. Besser als eine Zahl, die aussieht wie
        // eine echte.
        knoten: NodeId(0),
        quelle,
        typ,
        asynchron: attributes.contains_key("async"),
        verzoegert: attributes.contains_key("defer"),
        inhalt,
        zu_gross,
    }
}

/// Die `on*`-Attribute eines Elements.
///
/// GEMESSENE REGEL: nur `onclick`, `oninput` und die aehnlichen. Der
/// kleinste gemeinsame Nenner ist: der Attributname beginnt mit `on`.
/// Alles andere (etwas `once` oder `only` in einem Attributnamen) ist
/// keine Seite von 2026, sondern ein Fehler — und ein Fehler, der eine
/// `String` in den Speicher schreibt, ist ein Fehler mit Speicher.
pub fn ereignisse_aus(
    tag: &str,
    attributes: &HashMap<String, String>,
    eigene_ord: Option<u32>,
) -> Vec<Ereignis> {
    let mut raus: Vec<Ereignis> = Vec::new();
    for (name, wert) in attributes {
        if !name.starts_with("on") || name.len() <= 2 {
            continue;
        }
        if wert.trim().is_empty() {
            // `onclick=""` ist dasselbe wie kein `onclick`.
            continue;
        }
        let mut quelle = wert.clone();
        if quelle.len() > MAX_EREIGNIS_QUELLE {
            let g = crate::zeichengrenze(&quelle, MAX_EREIGNIS_QUELLE);
            quelle.truncate(g);
        }
        raus.push(Ereignis {
            knoten: None,
            // H2 (03.10., gemessen): die VORLAUF-NUMMER des Elements.
            //
            // GEMESSENER FEHLER, und er ist der Grund, warum `onclick` bei
            // diesem Browser NIE lief: `knoten` ist hier immer `None`, und
            // `narsil` kann `None` nicht eintragen (ein Handler ohne Nummer
            // gehoert zu keinem Knoten). `narsil-page` hatte die Nummer
            // aber die ganze Zeit — `element()` sichert sie als
            // `eigene_ord`, BEVOR die Kinder gebaut werden, und genau die
            // benutzt auch die `id`-Tabelle (`self.ids.insert(name,
            // self.aktualer_ordinal)`).
            //
            // Also: `eigene_ord` wandert mit. GEMESSEN, warum ZWEI
            // Felder (`knoten` und `ord`) und nicht eines: `knoten` ist
            // die Dokument-Nummer und wird fuer INLINE-Elemente
            // (`<b onclick=…>`) nie vergeben — dort bleibt `knoten` zu
            // Recht `None`, und `ord` ist die Zahl, die der Host
            // braucht. Ein Feld fuer beides muesste eine Union sein.
            ord: eigene_ord,
            tag: tag.to_string(),
            art: name[2..].to_ascii_lowercase(),
            quelle,
        });
    }
    raus
}
