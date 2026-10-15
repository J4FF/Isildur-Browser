//! narsil-fmt-html — HTML lesen.
//!
//! Diese Crate liest HTML und gibt `Vec<HtmlNode>` zurueck — eine
//! Baumstruktur, die KEINE unserer Knoten ist. Zwischen dort und dem Bild
//! steht `narsil-page`.
//!
//! Sie versteht kein CSS, kein JavaScript. Sie liest die STRUKTUR.
//!
//! # WELLE 4 (30.09.2026): was sich geaendert hat
//!
//! **1. Der Speicher.** Die alte Fassung hatte
//!
//! ```ignore
//! fn starts_with(&self, s: &str) -> bool {
//!     let remaining: String = self.input[self.pos..].iter().collect();
//!     remaining.starts_with(s)
//! }
//! ```
//!
//! Das allokiert den GESAMTEN Rest der Eingabe bei JEDEM Aufruf, und
//! `starts_with_ci` allokierte ihn noch einmal plus `to_lowercase()`.
//! Dazu kam in `parse_children` pro Schleifendurchlauf ein
//! `format!("</{}", parent_tag)`. Fuer eine 10-MB-Seite mit 100.000 Tags
//! sind das rund 10^12 Byte Allokation. Das ist nicht "langsam", das ist
//! der Speicherabsturz, an dem die Maschine viermal gestorben ist.
//!
//! GEMESSEN, woher das kam: `input: Vec<char>` (4 Byte je Zeichen, 40 MB
//! fuer 10 MB Eingabe) plus die Kopien oben. Jetzt: `&str` plus
//! Byte-Index, und `rest()` liefert eine Teil-Referenz.
//!
//! **2. Jede Schleife und jede Allokation hat eine Obergrenze** (M3/M4).
//! Sie stehen als benannte Konstanten unten, mit der Begruendung.
//!
//! **3. Die Endlosschleife** ist an EINER Stelle behoben und dort auch
//! nachgewiesen: die Schleife prueft nicht nur `at_end()`, sondern auch,
//! ob sich die Position veraendert hat.
//!
//! **4. Implizite Schluesse** nach WHATWG: `<p>a<p>b` sind zwei Absaetze.
//!
//! **5. Entitaeten**: numerisch dezimal und hex, und die ~265
//! haeufigsten benannten.
//!
//! **6. CDATA**, Kommentare, DOCTYPE, Processing-Instructions, Attribute
//! ohne Anfuehrungszeichen.
//!
//! **7. `template`, `iframe`, `object`** gelten als unsichtbar.

/// Release 12.10.2026: lebende Bereiche (DOM §5 „live ranges").
pub mod bereich;
pub mod dom;
mod entities_tabelle;
pub mod kodierung;
/// Release 09.10.2026: das lebende DOM (Operationen auf dem Arena-Baum).
pub mod lebend;
pub mod spec_tokenizer;
pub mod treebuilder;
pub mod xhtml;
pub mod xml;

use std::collections::HashMap;

/// Der Grund, an dem eine Obergrenze gegriffen hat.
///
/// Wird nach aussen GEREICHT, nicht versteckt: `parse_html` gibt einen
/// abgeschnittenen, aber sichtbaren Baum zurueck (siehe
/// `parse_html_mit_fehler`), und ein Aufrufer soll unterscheiden
/// koennen zwischen "Seite ist ganz" und "Seite endet hier".
pub use treebuilder::BaumFehler;

// ===========================================================================
// DIE OBERGRENZEN
// ===========================================================================

/// Maximale Verschachtelungstiefe (M5).
///
/// GEMESSEN, warum 512: eine echte Seite mit verschachtelten Tabellen und
/// Listen kommt auf 40 bis 60. 512 laesst alles Sichtbare zu.
///
/// GEMESSEN, warum ueberhaupt: der alte Legacy-Weg (`parse_children`
/// und `baum_bauen`, gegenseitig rekursiv — beides seit 03.10.2026
/// ENTFERNT, Bericht _62) rief sich gegenseitig, und der Spec-Weg
/// ebenfalls rekursiv. 100.000 verschachtelte `<div>` sind ohne diese
/// Grenze ein Stackueberlauf — Signal 11, und das sieht ein
/// `catch_unwind` nicht.
pub const MAX_TIEFE: u32 = 512;

/// Maximale Knotenzahl im ganzen Dokument (M3).
///
/// GEMESSEN, warum global und nicht je Element: `<div><div>…` mit
/// geschlossenen Tags erzeugt auch bei Tiefe 2 Millionen Knoten, weil
/// jeder `</div>` wieder auf 0 geht. Eine Grenze je Ebene wuerde das
/// nicht fangen.
///
/// # ACHTUNG: DIESE KONSTANTE WIRKT NICHT (03.10.2026, Bericht _90)
///
/// Der Baumaufbau setzt `dom::MAX_KNOTEN` durch, und das ist 400 000 —
/// nicht der Wert hier. GEMESSEN per Bisektion am selben Tag:
///   200 005 Knoten (`<b></b>` x 200 001)  -> parst sauber
///   400 000 Knoten                          -> parst sauber
///   400 001 Knoten                          -> `BaumFehler::ZuVieleKnoten`
/// Wer sich auf diese Zahl verlaesst, wartet also auf eine Grenze, die
/// es nicht gibt. Ich aendere den Wert NICHT, weil die Konstante
/// `pub` ist und ein Konsument sie als eigene Budget-Vorgabe lesen
/// koennte; die Entscheidung gehoert der Koordination (PROBLEME.txt).
/// Die Zahl bleibt als historischer Stand stehen und ist ab jetzt als
/// unwirksam markiert.
pub const MAX_KNOTEN: usize = 200_000;

/// Maximale Attributzahl je Tag (M3).
///
/// `<div a=1 a=1 …>` mit 1.000.000 Attributen waere eine HashMap mit 1
/// Millionen Eintraegen bei 10 MB Dateigroesse.
pub const MAX_ATTRIBUTE: usize = 64;

/// Maximale Laenge eines Tag-Namens (M3).
pub const MAX_TAG_NAME: usize = 128;

/// Maximale Laenge eines Attributnamens (M3).
pub const MAX_ATTRIBUT_NAME: usize = 128;

/// Maximale Laenge einer Entitaet inklusive `&` und `;` (M3).
///
/// Die alte Fassung hatte hier 10. Das wird angehoben, weil `&thetasym;`
/// 9 Zeichen hat und `&nbsp;` 6 — 10 war knapp, und eine knappe Grenze
/// ist eine Grenze, die man im Streit wegr diskutiert.
pub const MAX_ENTITAET: usize = 32;

/// Maximale Laenge eines Rohtext-Stuecks je Aufruf (M3).
pub const MAX_RAUMINHALT: usize = 64 * 1024;

/// Hoechste Laenge eines allgemeinen HTML-Textknotens in Bytes.
/// Skripte haben eine eigene Grenze, weil Quelltext vollstaendig
/// sein muss oder explizit als zu gross abgelehnt werden muss.
pub const MAX_ROHINHALT: usize = 256 * 1024;
/// Vier Byte Reserve: auch nach dem Schnitt an einer UTF-8-Grenze
/// bleibt Ueberlaenge fuer narsil-page erkennbar.
///
/// Release 10.10.2026: 16 MiB wie `narsil_page::MAX_SKRIPT_INHALT` (beide
/// Zahlen gehoeren zusammen: ist diese kleiner, kommt ein zu grosses Skript
/// GEKUERZT und unerkannt an).
pub const MAX_SKRIPT_ROHINHALT: usize = 16 * 1024 * 1024 + 4;
/// Release 08.10.2026: Hoechste Laenge eines `<style>`-Inhalts — dieselbe
/// wie die Eingabegrenze des CSS-Parsers (4 MiB), plus Reserve.
pub const MAX_STIL_ROHINHALT: usize = 4 * 1024 * 1024 + 4;

// ===========================================================================
// DER BAUM
// ===========================================================================

/// Ein HTML-Element im Parse-Baum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlNode {
    /// Ein Element mit Tag-Name, Attributen und Kindern.
    Element {
        tag: String,
        attributes: HashMap<String, String>,
        children: Vec<HtmlNode>,
        /// Shadow Root Info (declarative Shadow DOM).
        shadow_root: Option<ShadowRootInfo>,
    },
    /// Textinhalt.
    Text(String),
}

/// Shadow Root Information fuer declarative Shadow DOM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowRootInfo {
    pub mode: ShadowRootMode,
    pub children: Vec<HtmlNode>,
    /// Release 12.10.2026: die Merkmale (`dom::SCHATTEN_*`).
    pub merkmale: u8,
}

/// Shadow Root Mode (open/closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowRootMode {
    Open,
    Closed,
}

impl HtmlNode {
    /// Ob das Element ein Block-Element ist (div, p, h1..h6, etc.).
    pub fn is_block(&self) -> bool {
        match self {
            HtmlNode::Element { tag, .. } => matches!(
                tag.as_str(),
                "div"
                    | "p"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "section"
                    | "article"
                    | "nav"
                    | "aside"
                    | "header"
                    | "footer"
                    | "main"
                    | "ul"
                    | "ol"
                    | "li"
                    | "dl"
                    | "dt"
                    | "dd"
                    | "table"
                    | "tr"
                    | "td"
                    | "th"
                    | "thead"
                    | "tbody"
                    | "tfoot"
                    | "form"
                    | "fieldset"
                    | "legend"
                    | "blockquote"
                    | "pre"
                    | "figure"
                    | "figcaption"
                    | "details"
                    | "summary"
                    | "address"
                    | "hr"
                    | "br"
                    | "caption"
            ),
            HtmlNode::Text(_) => false,
        }
    }

    /// Ob das Element ein Void-Element ist (kein schliessender Tag).
    ///
    /// GEQUELLE der Liste: die "void elements" in
    /// WHATWG HTML 4.10.2 / 13.2.5.x — `area`, `base`, `basefont`,
    /// `bgsound`, `br`, `col`, `embed`, `frame`, `hr`, `img`, `input`,
    /// `keygen`, `link`, `meta`, `param`, `source`, `track`, `wbr`.
    ///
    /// GEMESSENE ENTSCHEIDUNG: es fehlten `basefont`, `bgsound`, `frame`
    /// und `keygen`. `keygen` hat gleich zwei Stellen verraten, weil es
    /// auch in der Void-Liste von 13.2.6.4.7 (in body) steht: mit der
    /// Eingabe `doctype html`, dann `keygen`, dann `frameset`, dann
    /// `frame` erwartet der Test `frameset > frame`, und bekommen
    /// wurde `keygen > frameset > frame`: `keygen` lief vorher in den
    /// "sonst"-Zweig und wurde therefore zu einem Behaelter.
    /// zu einem Behaelter.)
    pub fn is_void(&self) -> bool {
        match self {
            HtmlNode::Element { tag, .. } => matches!(
                tag.as_str(),
                "area"
                    | "base"
                    | "basefont"
                    | "bgsound"
                    | "br"
                    | "col"
                    | "embed"
                    | "frame"
                    | "hr"
                    | "img"
                    | "input"
                    | "keygen"
                    | "link"
                    | "meta"
                    | "param"
                    | "source"
                    | "track"
                    | "wbr"
            ),
            HtmlNode::Text(_) => false,
        }
    }

    /// Ob das Element unsichtbar ist.
    ///
    /// WELLE 4, H1: `template`, `iframe` und `object` kommen dazu. Sie
    /// stehen in realen Seiten, und ihr Inhalt ist als Text Unsinn: ein
    /// `<template>` enthaelt Markup, das NICHT angezeigt wird, ein
    /// `<iframe>` eine ganze fremde Seite, ein `<object>` ein Plugin.
    ///
    /// `noscript` ist hier NICHT mehr: Welle 4 sagt ausdruecklich
    /// "`<noscript>` wird ANGEZEIGT". Sein Inhalt ist genau der, den ein
    /// Benutzer ohne JavaScript sehen soll — er ist die einzige
    /// Unterstuetzung, die manche Seiten anbieten.
    pub fn is_invisible(&self) -> bool {
        match self {
            HtmlNode::Element { tag, .. } => matches!(
                tag.as_str(),
                "script"
                    | "style"
                    | "head"
                    | "meta"
                    | "link"
                    | "title"
                    | "template"
                    | "iframe"
                    | "object"
            ),
            HtmlNode::Text(_) => false,
        }
    }

    /// Das erste Attribut mit diesem Namen.
    pub fn attribut(&self, name: &str) -> Option<&str> {
        match self {
            HtmlNode::Element { attributes, .. } => attributes.get(name).map(|s| s.as_str()),
            HtmlNode::Text(_) => None,
        }
    }
}

// ===========================================================================
// DER EINSTIEG
// ===========================================================================

/// Parst HTML-Text in einen Baum.
///
/// Der EINZIGE oeffentliche Einstieg. Fehlertolerant: kaputtes HTML ist im
/// Web die Norm, nicht die Ausnahme.
///
/// # DIE ENDLICHKEIT, und wo sie nachgewiesen ist
///
/// Der alte `baum_bauen` (seit 03.10.2026 entfernt, Bericht _62) pruefte
/// in jeder Runde: (a) ist die Eingabe zu Ende, (b) hat sich
/// `Tokenizer::position()` veraendert. Punkt (b) ist die Antwort
/// auf den gemessenen Endlosschleifenbefund (`parse_html("</a>")` kehrt
/// nie zurueck). Punkt (a) allein genuegt nicht, weil `naechstes()` auch
/// bei einem unerwarteten `</a>` `None` liefern kann, ohne die Position zu
/// bewegen.
///
/// Zusaetzlich begrenzt: `MAX_KNOTEN` (M3) und `MAX_TIEFE` (M5).
pub fn parse_html(html: &str) -> Vec<HtmlNode> {
    // AENDERUNG 03.10.2026 (Vorgabe der Koordination, A3): der Weg
    // geht ueber den Spec-Baumaufbau, nicht ueber `tokenizer.rs` +
    // `baum_bauen` (heute ENTFERNT, Bericht _62). GEMESSEN, was das
    // kostet und was es bringt:
    //   vorher (Legacy, `baum_bauen`): 0 von 631 vergleichbaren
    //     html5lib-Faellen = 0,000 % — und zwar in BEIDEN Vergleichen,
    //     auch ohne html/head/body-Rahmen (Bericht _42).
    //   nachher: Weg (a) aus `tests/weg_a_messung.rs`, 595 von 784
    //     = 75,893 % (03.10.2026 neu gemessen; hier standen 589 von
    //     784 = 75,128 % und in `tokenizer.rs` 588 von 715 = 82,238 %
    //     — zwei Zahlen, die sich widersprachen); der Spec-Baum
    //     selbst haelt 1771/1784 = 99,271 %.
    // Die Differenz zu 99,271 % ist KEINE Parsergrenze: `HtmlNode` kennt
    // nur `Element` und `Text` und kann Kommentar, Doctype, PI,
    // Fremdinhalt (svg/math), Fremdattribut und Template-Inhalt nicht
    // tragen — 949 der 1784 Korpusfaelle (davon 681 Doctypes,
    // Bericht _40). Das ist die API, und ihre Erweiterung gehoert zur
    // Koordination (PROBLEME.txt, PRODUKTPFAD).
    //
    // DIE SIGNATUR BLEIBT: `fn(&str) -> Vec<HtmlNode>`. Damit muss
    // `narsil-page` nichts aendern — der Auftrag der Koordination nennt
    // die Aufrufstelle dort, aber eine unveraenderte Signatur macht
    // das unnoetig.
    let (baum, _verluste) = parse_html_mit_verlusten(html);
    baum
}

/// Was der Adapter nicht in `HtmlNode` tragen kann.
///
/// GEMESSEN ueber den html5lib-Korpus (Bericht _40): 949 der 1784
/// Faelle betreffen diese Liste, davon 681 Doctypes. Jede Zahl hier ist
/// eine Teilmenge — ein Fall kann zwei Eintraege haben.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Verluste {
    /// Kommentar-Knoten (`<!-- … -->`).
    pub kommentar: usize,
    /// Doctype-Knoten.
    pub doctype: usize,
    /// Processing-Instruction-Knoten (`<? … ?>`).
    pub pi: usize,
    /// Elemente im SVG- oder MathML-Namensraum samt Nachfahren.
    pub fremd: usize,
    /// Attribute im XML-, XMLNS- oder XLink-Namensraum.
    pub fremdattribut: usize,
    /// Inhalte von `<template>` (liegen im Spec-Baum in einem eigenen
    /// Fragment, 13.2.6.4.16).
    pub template_inhalt: usize,
}

impl Verluste {
    /// Die Summe — also die Zahl der Knoten, die der Adapter nicht
    /// weitergeben konnte.
    pub fn gesamt(&self) -> usize {
        self.kommentar
            + self.doctype
            + self.pi
            + self.fremd
            + self.fremdattribut
            + self.template_inhalt
    }
}

/// Wie `parse_html`, aber mit der Zaehlung der Knoten, die
/// `HtmlNode` nicht tragen kann.
///
/// WOFUER: eine Zahl, die man zitieren kann, ohne sie zu schaetzen.
/// Ein stiller Verlust sieht aus wie ein Erfolg — das ist der ganze
/// Grund fuer diese Funktion.
thread_local! {
    /// Release 2026-10-15: while set, `sammeln_knoten` records the source
    /// order of the attributes (see [`parse_html_with_attribute_order`]).
    static ATTRIBUTE_ORDER: std::cell::RefCell<Option<lebend::AttributeOrder>> = const { std::cell::RefCell::new(None) };
}

/// Release 2026-10-15: like [`parse_html`], plus the source order of the
/// attributes, which `HtmlNode` (a `HashMap`) cannot carry. The live DOM of
/// the page restores it from this (see [`lebend::AttributeOrder`]).
pub fn parse_html_with_attribute_order(html: &str) -> (Vec<HtmlNode>, lebend::AttributeOrder) {
    ATTRIBUTE_ORDER.with(|o| *o.borrow_mut() = Some(lebend::AttributeOrder::default()));
    let baum = parse_html(html);
    let order = ATTRIBUTE_ORDER.with(|o| o.borrow_mut().take()).unwrap_or_default();
    (baum, order)
}

pub fn parse_html_mit_verlusten(html: &str) -> (Vec<HtmlNode>, Verluste) {
    let (baum, verluste, _grenze) = parse_html_mit_fehler(html);
    (baum, verluste)
}

/// Wie [`parse_html`], aber mit der Zaehlung der Knoten, die
/// `HtmlNode` nicht tragen kann, UND mit dem Grund, an dem eine Grenze
/// gegriffen hat.
///
/// Der Grund ist nicht Beiwerk: `None` heisst "die Seite ist ganz
/// gelesen". `Some(_)` heisst "ab hier fehlt Inhalt" — der Baum ist
/// trotzdem da (der Teil bis zur Grenze), aber ein Aufrufer, der eine
/// Seite vollstaendig darstellen will, sollte das wissen. Vor der
/// Umstellung (A3) war beides nicht unterscheidbar: ein Fehler kam als
/// `Err` heraus und der Aufrufer sah einen leeren Baum (GEMESSEN an
/// `tests/bomben.rs`, siehe `parse_dokument_mit_fehler`).
pub fn parse_html_mit_fehler(html: &str) -> (Vec<HtmlNode>, Verluste, Option<BaumFehler>) {
    let ((dok, wurzel), grenze) = treebuilder::parse_dokument_mit_fehler(html, false);
    let (baum, verluste) = spec_in_htmlnode(&dok, wurzel);
    (baum, verluste, grenze)
}

/// Parst HTML aus Rohbytes mit automatischer Kodierungserkennung.
///
/// Nimmt rohe Bytes, erkennt die Kodierung (BOM, Transport-Hinweis,
/// `<meta charset>`, Fallback windows-1252), dekodiert zu Text und
/// parst dann den HTML-Baum.
///
/// Unterstuetzte Kodierungslabels (case-insensitiv):
/// - `"utf-8"`, `"utf-16"`, `"utf-16le"`, `"utf-16be"`
/// - Alle WHATWG-Labels fuer windows-1252, iso-8859-15, shift_jis, euc-kr, gb18030
///
/// Wenn `transport_hinweis` `Some` ist, wird er als `Content-Type`-Wert
/// oder reines Etikett behandelt (wie in `kodierung::erkenne`).
pub fn parse_html_mit_kodierung(
    rohdaten: &[u8],
    transport_hinweis: Option<&str>,
) -> (Vec<HtmlNode>, Verluste, Option<BaumFehler>) {
    use crate::kodierung::{dekodiere, erkenne};

    let kodierung = erkenne(rohdaten, transport_hinweis);
    let text = dekodiere(rohdaten, kodierung);
    parse_html_mit_fehler(&text)
}

/// Der Spec-Baum in `HtmlNode` — die HUELLE, die A3 verlangt.
///
/// Alles, was `HtmlNode` tragen kann, wird 1:1 uebernommen; alles
/// andere wird gezaehlt und weggelassen. Der Aufrufer soll die Zaehler
/// sehen, damit ein Verlust nicht wie ein Erfolg aussieht.
fn spec_in_htmlnode(dok: &dom::Dokument, wurzel: usize) -> (Vec<HtmlNode>, Verluste) {
    let mut v = Verluste::default();
    let mut raus = Vec::new();
    // Release 12.10.2026: Wirt -> Shadow Root (ein `Art::ShadowRoot`-Knoten,
    // dessen Eltern der Wirt ist), EINMAL je Umwandlung.
    let schatten: HashMap<usize, usize> = dok
        .knoten
        .iter()
        .enumerate()
        .filter_map(|(i, n)| match (&n.art, n.eltern) {
            (dom::Art::ShadowRoot { .. }, Some(wirt)) => Some((wirt, i)),
            _ => None,
        })
        .collect();
    sammeln(dok, wurzel, &mut raus, &mut v, false, &schatten);
    (raus, v)
}

fn sammeln(
    dok: &dom::Dokument,
    k: usize,
    raus: &mut Vec<HtmlNode>,
    v: &mut Verluste,
    in_fremd: bool,
    schatten: &HashMap<usize, usize>,
) {
    for &kind in &dok.knoten[k].kinder {
        if let Some(node) = sammeln_knoten(dok, kind, v, in_fremd, schatten) {
            raus.push(node);
        }
    }
}

/// Verarbeitet einen einzelnen Knoten und gibt `HtmlNode` zurueck (oder `None` bei Verlusten).
fn sammeln_knoten(
    dok: &dom::Dokument,
    k: usize,
    v: &mut Verluste,
    in_fremd: bool,
    schatten: &HashMap<usize, usize>,
) -> Option<HtmlNode> {
    match &dok.knoten[k].art {
        dom::Art::Text(t) => Some(HtmlNode::Text(t.clone())),
        dom::Art::Kommentar(_) => { v.kommentar += 1; None }
        dom::Art::Doctype { .. } => { v.doctype += 1; None }
        dom::Art::Pi { .. } => { v.pi += 1; None }
        dom::Art::Dokument | dom::Art::Fragment => {
            let mut kinder = Vec::new();
            sammeln(dok, k, &mut kinder, v, in_fremd, schatten);
            // Dokument/Fragment selbst werden nicht als Knoten dargestellt,
            // aber ihre Kinder werden verarbeitet. Wir geben None zurueck,
            // weil der Aufrufer die Kinder schon in `raus` hat.
            None
        }
        dom::Art::ShadowRoot { .. } => {
            // ShadowRoot wird nicht als eigener Knoten im Baum gespeichert,
            // sondern als Feld auf dem Host-Element. Dieser Fall sollte
            // nie erreicht werden, aber wir behandeln ihn fuer Vollstaendigkeit.
            v.fremd += 1;
            None
        }
        dom::Art::Element {
            raum,
            name,
            attribute,
            template_inhalt,
            shadow_root,
        } => {
            let fremd_hier = matches!(raum, dom::Namensraum::Svg | dom::Namensraum::MathMl);
            let in_fremd_neu = in_fremd || fremd_hier;
            if fremd_hier {
                v.fremd += 1;
            }
            let mut m: HashMap<String, String> = HashMap::with_capacity(attribute.len());
            for a in attribute {
                if !matches!(a.raum, dom::Namensraum::Html) {
                    v.fremdattribut += 1;
                }
                m.insert(xml_attributname(a), a.wert.clone());
            }
            if attribute.len() >= 2 {
                ATTRIBUTE_ORDER.with(|o| {
                    if let Some(order) = o.borrow_mut().as_mut() {
                        let paare: Vec<(String, String)> = attribute.iter().map(|a| (xml_attributname(a), a.wert.clone())).collect();
                        order.record(name, &paare);
                    }
                });
            }
            let mut kinder = Vec::new();
            sammeln(dok, k, &mut kinder, v, in_fremd_neu, schatten);
            if let Some(inhalt) = template_inhalt {
                v.template_inhalt += 1;
                sammeln(dok, *inhalt, &mut kinder, v, false, schatten);
            }
            // Release 12.10.2026: die Shadow Root ist ein eigener Knoten (siehe
            // `treebuilder::dsd_template`); die Liste am Wirt ist nur der Merker.
            let shadow_root_info = shadow_root.as_ref().map(|sr| {
                let mut sr_kinder = Vec::new();
                let liste: &[usize] = match schatten.get(&k) {
                    Some(&w) => &dok.knoten[w].kinder,
                    None => &sr.children,
                };
                for &child in liste {
                    if let Some(node) = sammeln_knoten(dok, child, v, false, schatten) {
                        sr_kinder.push(node);
                    }
                }
                ShadowRootInfo {
                    mode: match sr.mode {
                        dom::ShadowRootMode::Open => ShadowRootMode::Open,
                        dom::ShadowRootMode::Closed => ShadowRootMode::Closed,
                    },
                    children: sr_kinder,
                    merkmale: sr.merkmale,
                }
            });
            Some(HtmlNode::Element {
                tag: name.clone(),
                attributes: m,
                children: kinder,
                shadow_root: shadow_root_info,
            })
        }
    }
}

/// Der Attributname in der Form, die 13.3 beim Serialisieren
/// vorschreibt: `xml:base`, `xmlns:xlink`, `xlink:href`.
///
/// GEMESSEN, warum nicht `dom::Attribut::anzeige()`: die schreibt
/// `xml base` mit Leerzeichen — das ist die Schreibweise des
/// TESTFORMATS von html5lib. Als Schluessel einer `HashMap` waere sie
/// eindeutig, aber sie erzeugt ein Attribut, das kein Browser
/// zuruecklesen kann. Mit Doppelpunkt ist es die Form, die wieder
/// eingelesen wird.
fn xml_attributname(a: &dom::Attribut) -> String {
    match a.raum {
        dom::Namensraum::Html => a.name.clone(),
        dom::Namensraum::Xml => format!("xml:{}", a.name),
        dom::Namensraum::Xmlns if a.name == "xmlns" => "xmlns".to_string(),
        dom::Namensraum::Xmlns => format!("xmlns:{}", a.name),
        dom::Namensraum::Xlink => format!("xlink:{}", a.name),
        dom::Namensraum::Svg | dom::Namensraum::MathMl | dom::Namensraum::Fremd => a.name.clone(),
    }
}

/// Ersetzt die Entitaeten in EINEM Text.
/// Die groesste Zeichengrenze <= `max` (Bytes).
///
/// WARUM EIGENSTATT `str::floor_char_boundary`: das ist erst seit Rust
/// 1.91 stabil, der Workspace nennt `rust-version = 1.85`
/// (`Cargo.toml`). clippy sagt es laut ("current MSRV is 1.85.0 but this
/// item is stable since 1.91.0"), und ein Aufruf davon waere eine
/// gebrochene Zusage an alle, die mit der angegebenen Version bauen.
///
/// GEMESSEN, warum es hier noetig ist: `String::truncate` PANICT, wenn
/// es mitten in einem UTF-8-Zeichen schneidet. Ein `<script>` mit einem
/// Umlaut an Byte 65.535 und einem Grenzwert von 65.536 ist genau der
/// Fall — und ein Panic in einem Skript ist ein Absturz des Browsers.
pub(crate) fn zeichengrenze(s: &str, max: usize) -> usize {
    if s.len() <= max {
        return s.len();
    }
    // Von `max` rueckwaerts bis zur Zeichengrenze: hoechstens drei
    // Schritte, denn ein UTF-8-Zeichen ist hoechstens vier Byte breit.
    let mut i = max;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn entitaet_ersetzen_einzeln(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut raus = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        raus.push_str(&rest[..i]);
        let nach = &rest[i..];
        // M3: `MAX_ENTITAET` Zeichen. Ohne diese Grenze liest `&` + 10 MB
        // Buchstaben den ganzen Rest.
        // An einer ZEICHENGRENZE abschneiden (Koordination 30.09., gefunden
        // mit narsil-fuzz): `&amp;` + '„' an Byte MAX_ENTITAET liess die feste
        // Bytegrenze mitten in ein Mehrbyte-Zeichen fallen -> Panic.
        let fenster = &nach[..zeichengrenze(nach, MAX_ENTITAET)];
        match entitaet_ersetzen(fenster) {
            Some((ersetzt, laenge)) => {
                raus.push_str(&ersetzt);
                rest = &nach[laenge..];
            }
            None => {
                // Unbekannte Entitaet: sie bleibt TEXT. Sie verschwindet
                // nicht, und sie loest keinen Panic aus — `&foobar;` ist
                // in echten Texten ein Wort, kein Fehler.
                raus.push('&');
                rest = &nach[1..];
            }
        }
    }
    raus.push_str(rest);
    raus
}

// ===========================================================================
// DIE ENTIITAETEN
// ===========================================================================

/// Loest eine Entitaet auf: `&ouml;` -> `ö`.
///
/// Rueckgabe: `(ersetzter_text, wie_viele_bytes_der_aufruf_hatte)`.
///
/// `None`, wenn es keine Entitaet ist — dann bleibt der Text, wie er ist.
///
/// # REGELN
///
/// 1. **Namen enden auf `;`.** HTML verlangt das. Ohne `;` ist `&amp`
///    der Text "&amp" — so zeigen es alle Browser, und eine Ausnahme
///    wuerde bei `AT&T` oder `Tom&Jerry` den Text zerstoeren.
/// 2. **Numerisch dezimal** `&#252;` und **hexadezimal** `&#xFC;`.
///    Gross-/Kleinschreibung bei `x` und `A-F` ist egal.
/// 3. **Ungueltige Nummern** (0, zu gross, kein Zeichen) ergeben das
///    ERSTETZEZEICHEN des Codes — so macht es die Spezifikation. Das ist
///    besser als ein Panic und besser als `&notanumber;` stehen lassen.
pub fn entitaet_ersetzen(text: &str) -> Option<(String, usize)> {
    let rest = text.strip_prefix('&')?;
    // Regel 1: ohne `;` ist es Text.
    let semikolon = rest.find(';')?;
    let name = &rest[..semikolon];
    let laenge = 1 + name.len() + 1;

    // Numerisch
    if let Some(rumpf) = name.strip_prefix('#') {
        if let Some(hex) = rumpf.strip_prefix('x').or_else(|| rumpf.strip_prefix('X')) {
            if !hex.is_empty() {
                return match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                    Some(c) => Some((c.to_string(), laenge)),
                    // Regel 3: Windows-1252-Sonderzeichen. `&#146;` ist im
                    // Web ein typografisches Anfuehrungszeichen, nicht
                    // U+0092 (ein Steuerzeichen). Ohne diese Tabelle
                    // zerfallen 96 Stellen zu Steuerzeichen.
                    None => windows1252(u32::from_str_radix(hex, 16).unwrap_or(0))
                        .map(|c| (c.to_string(), laenge)),
                };
            }
        } else if !rumpf.is_empty() {
            return match rumpf.parse::<u32>().ok().and_then(char::from_u32) {
                Some(c) => Some((c.to_string(), laenge)),
                None => {
                    windows1252(rumpf.parse::<u32>().unwrap_or(0)).map(|c| (c.to_string(), laenge))
                }
            };
        }
        return None;
    }

    // Benannt, case-insensitiv (HTML erlaubt `&AMP;` und `&amp;`).
    let klein = name.to_ascii_lowercase();
    benannte_entitaet(&klein).map(|c| (c.to_string(), laenge))
}

/// Welcher oeffnende Tag wird implizit geschlossen, wenn dieser hier
/// oeffnet wird.
///
/// WHATWG, vereinfacht (Welle 4, H1). GEMESSEN, warum das noetig ist:
/// `<p>a<p>b` steht in jedem Forum und in jedem Blog. Ohne diese Regel
/// werden aus zwei Absaetzen einer — und die Seite sieht aus wie kaputt,
/// obwohl sie in jedem Browser korrekt ist.
///
/// Die Funktion statt einer Tabelle, weil sie fuer jedes offene Element
/// aufgerufen wird und deshalb keine Allokation braucht.
///
/// GEMESSEN, welcher Fall am haeufigsten ist: `<li>` (Foren, Doku) und
/// `<p>` (Blog). Beide sind drin.
pub fn schliesst_implizit(offen: &str, neu: &str) -> bool {
    match (offen, neu) {
        // Listen
        ("li", "li") => true,
        ("dt", "dt") | ("dt", "dd") | ("dd", "dt") | ("dd", "dd") => true,
        // Absatz: wird von jedem Block geschlossen, der kein Inline ist.
        ("p", "p")
        | ("p", "div")
        | ("p", "ul")
        | ("p", "ol")
        | ("p", "table")
        | ("p", "blockquote")
        | ("p", "pre")
        | ("p", "h1")
        | ("p", "h2")
        | ("p", "h3")
        | ("p", "h4")
        | ("p", "h5")
        | ("p", "h6")
        | ("p", "hr")
        | ("p", "section")
        | ("p", "article")
        | ("p", "figure")
        | ("p", "dl")
        | ("p", "form")
        | ("p", "fieldset")
        | ("p", "address")
        | ("p", "main")
        | ("p", "nav")
        | ("p", "header")
        | ("p", "footer")
        | ("p", "aside")
        | ("p", "details")
        | ("p", "summary") => true,
        // Tabellen: `<table><tr><td>x` ist haeufiger als geschlossen
        ("td", "td")
        | ("td", "th")
        | ("td", "tr")
        | ("td", "tbody")
        | ("td", "thead")
        | ("td", "tfoot") => true,
        ("th", "th")
        | ("th", "td")
        | ("th", "tr")
        | ("th", "tbody")
        | ("th", "thead")
        | ("th", "tfoot") => true,
        ("tr", "tr") | ("tr", "tbody") | ("tr", "thead") | ("tr", "tfoot") => true,
        // Optionen
        ("option", "option") | ("option", "optgroup") => true,
        // Listen
        ("ul", "ul") | ("ol", "ol") | ("ul", "ol") | ("ol", "ul") => true,
        // Tabellenteile
        ("thead", "tbody") | ("thead", "tfoot") => true,
        ("tbody", "tbody") | ("tbody", "tfoot") => true,
        _ => false,
    }
}

/// Tags, deren Inhalt ROH ist und nicht geparst wird.
///
/// `<script>`, `<style>`, `<textarea>` und `<title>`: dort ist `<` nur ein
/// Zeichen, kein Tag-Anfang.
///
/// WELLE 4, H1: `template`, `iframe` und `object` sind hier NICHT dabei.
/// Sie werden wie normale Elemente geparst und der AUFRUFER verwirft sie
/// ueber `HtmlNode::is_invisible`. Grund: wir koennen nicht entscheiden, ob
/// ein `<template>`-Inhalt sichtbar sein soll, wenn das Template scheitert —
/// das ist eine Darstellung, keine Frage des Parsers.
pub fn ist_roher_inhalt(tag: &str) -> bool {
    matches!(tag, "script" | "style" | "textarea" | "title")
}

/// Die 96 Stellen, an denen windows-1252 und Unicode auseinandergehen.///
/// GEMESSEN, warum das eine Tabelle und keine Regel ist: `&#146;` ist in
/// HTML ein typografisches Anfuehrungszeichen. `char::from_u32(146)`
/// liefert U+0092, ein STEUERZEICHEN — unsichtbar und damit ein
/// Datenverlust. Die Abbildung gibt es in der HTML-Spezifikation
/// (Abschnitt "Character reference"), und sie ist die einzige richtige
/// Antwort.
fn windows1252(n: u32) -> Option<String> {
    let c = match n {
        0x80 => '\u{20AC}', // Euro
        0x82 => '\u{201A}', // einzelnes tiefes Komma
        0x83 => '\u{0192}', // f
        0x84 => '\u{201E}', // doppeltes tiefes Anfuehrungszeichen
        0x85 => '\u{2026}', // Auslassungspunkte
        0x86 => '\u{2020}', // Dolch
        0x87 => '\u{2021}', // Geviert
        0x88 => '\u{02C6}', // Spitzhat
        0x89 => '\u{2030}', // Promille
        0x8A => '\u{0160}', // S mit Haken
        0x8B => '\u{2039}', // einzelnes spitzes Klammerzeichen
        0x8C => '\u{0152}', // OE
        0x8E => '\u{017D}', // Z mit Haken
        0x91 => '\u{2018}', // einzelnes hochkomma
        0x92 => '\u{2019}', // einzelnes Hochkomma
        0x93 => '\u{201C}', // doppeltes hochkomma
        0x94 => '\u{201D}', // doppeltes Hochkomma
        0x95 => '\u{2022}', // kleiner Punkt
        0x96 => '\u{2013}', // Halbgeviertstrich
        0x97 => '\u{2014}', // Geviertstrich
        0x98 => '\u{02DC}', // kleine Tilde
        0x99 => '\u{2122}', // Marke
        0x9A => '\u{0161}', // s mit Haken
        0x9B => '\u{203A}', // einzelnes spitzes Klammerzeichen
        0x9C => '\u{0153}', // oe
        0x9E => '\u{017E}', // z mit Haken
        0x9F => '\u{0178}', // Y mit Haken
        _ => return None,
    };
    Some(c.to_string())
}

/// Die haeufigsten benannten Entitaeten.
///
/// GEMESSEN, warum ~265 und nicht alle 2231: `&CounterClockwiseContourIntegral;`
/// steht in keiner Seite der Welt. Die Liste hier deckt ab, was in
/// deutschsprachigen und englischen Texten tatsaechlich vorkommt — die
/// Umlaute, die Anfuehrungszeichen, das Eurozeichen, das kaufmaennische
/// Und, das NBSP, Pfeile, Mathematik und griechische Grossbuchstaben.
///
/// UNBEKANNTE ENTITAETEN (nicht in dieser Liste) bleiben als Text stehen.
/// Das ist die Regel in Regel 1 oben, und sie ist wichtig: `&foobar;`
/// ist ein Wort, und ein stilles Wegwerfen waere Datenverlust.
fn benannte_entitaet(name: &str) -> Option<char> {
    Some(match name {
        // --- Grundzeichen ---
        //
        // GEMESSENER FEHLER 29.09.2026: `lt`, `gt`, `quot`, `amp` und
        // `apos` waren beim Neuschreiben der Liste VERLOREN gegangen, und
        // `lt`/`gt` sind die ZWEI haeufigsten Entitaeten der Welt — sie
        // stehen in jedem Text, der ein `<` oder `>` erwaehnt. Fuenf
        // Tests wurden rot, und der Compiler hat nichts gemeldet: eine
        // fehlende Tabellenzeile ist kein Typfehler.
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "amp" => '&',
        "apos" => '\'',
        "not" => '\u{ac}',
        "macr" => '\u{af}',
        "acute" => '\u{b4}',
        "cedil" => '\u{b8}',
        "uml" => '\u{a8}',
        "fnof" => '\u{192}',
        "brvbar" => '\u{a6}',
        // --- Umlaute und Eszett (deutsch) ---
        "auml" => '\u{e4}',
        "ouml" => '\u{f6}',
        "uuml" => '\u{fc}',
        "szlig" => '\u{df}',
        "Auml" => '\u{c4}',
        "Ouml" => '\u{d6}',
        "Uuml" => '\u{dc}',
        // --- Franzosisch ---
        "eacute" => '\u{e9}',
        "egrave" => '\u{e8}',
        "ecirc" => '\u{ea}',
        "agrave" => '\u{e0}',
        // "auml" stand hier ZWEIMAL: einmal im deutschen, einmal im
        // franzoesischen Block. Der zweite Arm war toter Code — er
        // koennte nie erreicht werden, weil der erste gewinnt.
        // clippy hat es gemeldet; die Werte waren identisch, also ist
        // beim Entfernen nichts verloren gegangen.
        "acirc" => '\u{e2}',
        "ccedil" => '\u{e7}',
        "ugrave" => '\u{f9}',
        "ucirc" => '\u{fb}',
        "icirc" => '\u{ee}',
        "igrave" => '\u{ef}',
        "euml" => '\u{eb}',
        "iuml" => '\u{ef}',
        "yuml" => '\u{ff}',
        "OElig" => '\u{152}',
        "oelig" => '\u{153}',
        "Eacute" => '\u{c9}',
        "Egrave" => '\u{c8}',
        "Ecirc" => '\u{ca}',
        "Aacute" => '\u{c1}',
        "Agrave" => '\u{c0}',
        "Acirc" => '\u{c2}',
        "Ccedil" => '\u{c7}',
        "Ugrave" => '\u{d9}',
        "Ucirc" => '\u{db}',
        "Iacute" => '\u{cd}',
        "Igrave" => '\u{cc}',
        "Icirc" => '\u{ce}',
        "Euml" => '\u{cb}',
        "Iuml" => '\u{cf}',
        // Fehlende franzoesische Grossbuchstaben (Latin-1)
        "Ograve" => '\u{d2}',
        "Oacute" => '\u{d3}',
        "Ocirc" => '\u{d4}',
        "Otilde" => '\u{d5}',
        // Fehlende franzoesische Kleinbuchstaben (Latin-1)
        "iacute" => '\u{ed}',
        "ograve" => '\u{f2}',
        "ocirc" => '\u{f4}',
        "otilde" => '\u{f5}',
        // --- Spanisch ---
        "ntilde" => '\u{f1}',
        "Ntilde" => '\u{d1}',
        "aacute" => '\u{e1}',
        "oacute" => '\u{f3}',
        "uacute" => '\u{fa}',
        "yacute" => '\u{fd}',
        "iquest" => '\u{bf}',
        "iexcl" => '\u{a1}',
        "ordf" => '\u{aa}',
        "ordm" => '\u{ba}',
        // --- Skandinavisch und Islaendisch ---
        "aelig" => '\u{e6}',
        "AElig" => '\u{c6}',
        "aring" => '\u{e5}',
        "Aring" => '\u{c5}',
        "oslash" => '\u{f8}',
        "Oslash" => '\u{d8}',
        "eth" => '\u{f0}',
        "ETH" => '\u{d0}',
        "thorn" => '\u{fe}',
        "THORN" => '\u{de}',
        "scaron" => '\u{161}',
        "Scaron" => '\u{160}',
        // --- Portugiesisch ---
        "atilde" => '\u{e3}',
        "Atilde" => '\u{c3}',
        // --- Anfuehrungszeichen (typografisch) ---
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201C}',
        "rdquo" => '\u{201D}',
        "sbquo" => '\u{201A}',
        "bdquo" => '\u{201E}',
        "laquo" => '\u{ab}',
        "raquo" => '\u{bb}',
        "lsaquo" => '\u{2039}',
        "rsaquo" => '\u{203A}',
        "dagger" => '\u{2020}',
        "Dagger" => '\u{2021}',
        "bull" => '\u{2022}',
        "prime" => '\u{2032}',
        "Prime" => '\u{2033}',
        "oline" => '\u{203E}',
        // --- Gedankenstrich und Auslassung ---
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "minus" => '\u{2212}',
        "hellip" => '\u{2026}',
        "middot" => '\u{b7}',
        "centerdot" => '\u{b7}',
        "divide" => '\u{f7}',
        "times" => '\u{d7}',
        "plusmn" => '\u{b1}',
        "frac12" => '\u{bd}',
        "frac14" => '\u{bc}',
        "frac34" => '\u{be}',
        "sup2" => '\u{b2}',
        "sup3" => '\u{b3}',
        "sup1" => '\u{b9}',
        "micro" => '\u{b5}',
        "para" => '\u{b6}',
        "sect" => '\u{a7}',
        "deg" => '\u{b0}',
        "permil" => '\u{2030}',
        // --- Leerzeichen ---
        // GEMESSENE ENTSCHEIDUNG, und sie ist die des Altbestands
        // (29.09.): diese Leerzeichen werden zu einem NORMALEN
        // Leerzeichen. Grund: `narsil-text` hat fuer U+00A0 keine Glyphe
        // und wuerde den Platzhalterkasten zeichnen — ein sichtbares
        // Kästchen, wo der Leser ein Leerzeichen erwartet. Fuer einen
        // Browser ohne die zweite Schrift ist ein echtes Leerzeichen
        // die richtige Antwort. Wer U+00A0 braucht, braucht eine
        // Schrift, die es hat (Aufgabe SCHRIFT, Welle 4).
        "nbsp" => ' ',
        "ensp" => ' ',
        "emsp" => ' ',
        "thinsp" => ' ',
        "shy" => '\u{ad}',
        // --- Waehrung ---
        "euro" => '\u{20AC}',
        "cent" => '\u{a2}',
        "pound" => '\u{a3}',
        "yen" => '\u{a5}',
        "curren" => '\u{a4}',
        // --- Pfeile ---
        "larr" => '\u{2190}',
        "uarr" => '\u{2191}',
        "rarr" => '\u{2192}',
        "darr" => '\u{2193}',
        "harr" => '\u{2194}',
        "lArr" => '\u{21D0}',
        "uArr" => '\u{21D1}',
        "rArr" => '\u{21D2}',
        "dArr" => '\u{21D3}',
        "hArr" => '\u{21D4}',
        "crarr" => '\u{21B5}',
        // --- Mathematik ---
        "le" => '\u{2264}',
        "ge" => '\u{2265}',
        "ne" => '\u{2260}',
        "asymp" => '\u{2248}',
        "equiv" => '\u{2261}',
        "infin" => '\u{221E}',
        "sum" => '\u{2211}',
        "prod" => '\u{220F}',
        "radic" => '\u{221A}',
        "int" => '\u{222B}',
        "part" => '\u{2202}',
        "forall" => '\u{2200}',
        "exist" => '\u{2203}',
        "empty" => '\u{2205}',
        "isin" => '\u{2208}',
        "notin" => '\u{2209}',
        "ni" => '\u{220B}',
        "sub" => '\u{2282}',
        "sup" => '\u{2283}',
        "cap" => '\u{2229}',
        "cup" => '\u{222A}',
        "and" => '\u{2227}',
        "or" => '\u{2228}',
        "nabla" => '\u{2207}',
        "prop" => '\u{221D}',
        "ang" => '\u{2220}',
        "sdot" => '\u{22C5}',
        "lceil" => '\u{2308}',
        "rceil" => '\u{2309}',
        "lfloor" => '\u{230A}',
        "rfloor" => '\u{230B}',
        "lang" => '\u{27E8}',
        "rang" => '\u{27E9}',
        "sim" => '\u{223C}',
        "cong" => '\u{2245}',
        "nsub" => '\u{2284}',
        "sube" => '\u{2286}',
        "supe" => '\u{2287}',
        "oplus" => '\u{2295}',
        "otimes" => '\u{2297}',
        "perp" => '\u{22A5}',
        "frasl" => '\u{2044}',
        "image" => '\u{2111}',
        "alefsym" => '\u{2135}',
        "weierp" => '\u{2118}',
        "real" => '\u{211C}',
        "trade" => '\u{2122}',
        "there4" => '\u{2234}',
        "alpha" => '\u{3b1}',
        "beta" => '\u{3b2}',
        "gamma" => '\u{3b3}',
        "delta" => '\u{3b4}',
        "epsilon" => '\u{3b5}',
        "zeta" => '\u{3b6}',
        "eta" => '\u{3b7}',
        "theta" => '\u{3b8}',
        "iota" => '\u{3b9}',
        "kappa" => '\u{3ba}',
        "lambda" => '\u{3bb}',
        "mu" => '\u{3bc}',
        "nu" => '\u{3bd}',
        "xi" => '\u{3be}',
        "omicron" => '\u{3bf}',
        "pi" => '\u{3c0}',
        "rho" => '\u{3c1}',
        "sigma" => '\u{3c3}',
        "sigmaf" => '\u{3c2}',
        "tau" => '\u{3c4}',
        "upsilon" => '\u{3c5}',
        "phi" => '\u{3c6}',
        "chi" => '\u{3c7}',
        "psi" => '\u{3c8}',
        "omega" => '\u{3c9}',
        // --- Griechische Grossbuchstaben ---
        "Alpha" => '\u{391}',
        "Beta" => '\u{392}',
        "Gamma" => '\u{393}',
        "Delta" => '\u{394}',
        "Epsilon" => '\u{395}',
        "Zeta" => '\u{396}',
        "Eta" => '\u{397}',
        "Theta" => '\u{398}',
        "Iota" => '\u{399}',
        "Kappa" => '\u{39a}',
        "Lambda" => '\u{39b}',
        "Mu" => '\u{39c}',
        "Nu" => '\u{39d}',
        "Xi" => '\u{39e}',
        "Omicron" => '\u{39f}',
        "Pi" => '\u{3a0}',
        "Rho" => '\u{3a1}',
        "Sigma" => '\u{3a3}',
        "Tau" => '\u{3a4}',
        "Upsilon" => '\u{3a5}',
        "Phi" => '\u{3a6}',
        "Chi" => '\u{3a7}',
        "Psi" => '\u{3a8}',
        "Omega" => '\u{3a9}',
        // --- Pfeile und Symbole fuer die Oberflaeche ---
        "lsaquo2" => '\u{ab}',
        "copy" => '\u{a9}',
        "reg" => '\u{ae}',
        "check" => '\u{2713}',
        "cross" => '\u{2717}',
        "star" => '\u{2605}',
        "starf" => '\u{2605}',
        "phone" => '\u{260E}',
        "sung" => '\u{266A}',
        "flat" => '\u{266D}',
        "natural" => '\u{266E}',
        "sharp" => '\u{266F}',
        "hearts" => '\u{2665}',
        "spades" => '\u{2660}',
        "clubs" => '\u{2663}',
        "diams" => '\u{2666}',
        "loz" => '\u{25CA}',
        "spadesuit" => '\u{2660}',
        "zwj" => '\u{200D}',
        "zwnj" => '\u{200C}',
        "lrm" => '\u{200E}',
        "rlm" => '\u{200F}',
        "ensp2" => '\u{2002}',
        "emsp2" => '\u{2003}',
        "sol" => '/',
        "bsol" => '\\',
        "verbar" => '|',
        "lobrk" => '\u{27e6}',
        "robrk" => '\u{27e7}',
        // --- Maenner und Frauen, Pfeile fuer Fussnoten ---
        "male" => '\u{2642}',
        "female" => '\u{2640}',
        _ => return None,
    })
}

// ===========================================================================
// DIE ALTEN FUNKTIONEN, unveraendert (der Aufrufer braucht sie)
// ===========================================================================

/// Extrahiert den sichtbaren Text aus einem HTML-Baum (rekursiv).
///
/// Unsichtbare Elemente (`script`, `style`, `head`, `template`, `iframe`,
/// `object`) werden uebersprungen. `noscript` NICHT — sein Inhalt ist
/// genau der, den ein Benutzer ohne JavaScript sehen soll.
///
/// DER TEXT WIRD KOLLABIERT (03.10.2026), und der Grund ist eine Messung
/// gegen meine eigene Aenderung:
///
/// GEMESSEN, vorher (alter Hand-Parser) und nachher (Spec-Baum) auf den
/// 30 Korpusseiten: der extrahierte Text war auf 28 von 30 Seiten
/// LAENGER, auf spiegel.de 40575 gegen 65990 Zeichen. GEMESSENE
/// Ursache: der alte Parser hat den Leerraum zwischen den Elementen
/// verworfen, der Spec-Baum nicht — 13.2.5.1 macht jedes Zwischen-
/// zeichen zu einem Zeichen-Token, und die Einrueckung einer Seite ist
/// solches Zeichen. In dem Text steckten 27215 Trennzeichen gegen 1785
/// vorher, also fast genau die Differenz.
///
/// Ein Browser zeigt keine Einrueckung an: CSS `white-space: normal`
/// kollabiert Folgen von Leerraum zu einem. Wer Text vergleicht
/// (Test, Suche, Messung), will deshalb den kollabierten — und genau
/// den hatte man vor der Umstellung. `collapse_whitespace` ist die
/// vorhandene Funktion dafuer; sie ist seit dem 30.09. im Crate und
/// wurde nur nicht hier gerufen.
///
/// DIE GRENZE BLEIBT: `MAX_AUSGABE`/`MAX_TEXTKNOTEN` greifen vorher,
/// ein Rumpf aus 10 MB Text kostet also dieselbe Zeit wie vorher und
/// nicht mehr.
pub fn extract_text(nodes: &[HtmlNode]) -> String {
    let mut text = String::new();
    for node in nodes {
        match node {
            HtmlNode::Text(t) => {
                if !text.is_empty() && !text.ends_with(' ') {
                    text.push(' ');
                }
                text.push_str(t);
            }
            HtmlNode::Element { children, shadow_root, .. } => {
                if node.is_invisible() {
                    continue;
                }
                let sub = extract_text(children);
                if !sub.is_empty() {
                    if !text.is_empty() && !text.ends_with(' ') {
                        text.push(' ');
                    }
                    text.push_str(&sub);
                }
                // Also extract text from shadow root (declarative Shadow DOM)
                if let Some(sr) = shadow_root {
                    let sub = extract_text(&sr.children);
                    if !sub.is_empty() {
                        if !text.is_empty() && !text.ends_with(' ') {
                            text.push(' ');
                        }
                        text.push_str(&sub);
                    }
                }
            }
        }
    }
    // DER LETZTE SCHRITT: kollabieren (Begruendung am Dateikopf).
    collapse_whitespace(&text)
}

/// Loest die Entitaeten EINES Textes auf.
///
/// DER NAME ist geblieben, obwohl die Funktion neu ist: `decode_entities`
/// war oeffentlich, und `narsil-page` koennte es benutzen. Ein oeffentlicher
/// Name zu aendern, weil die Internen besser geworden sind, ist eine
/// Aenderung ohne Nutzen fuer den Aufrufer.
///
/// Die Arbeit macht `entitaet_ersetzen_einzeln`; das hier ist der Weg
/// fuer einen einzelnen String ohne Baum.
pub fn decode_entities(text: &str) -> String {
    entitaet_ersetzen_einzeln(text)
}

/// Kollabiert Whitespace: mehrere Leerzeichen/Tabs/Newlines werden zu einem.
///
/// Wird NICHT mehr im Parser benutzt (der Tokenizer liefert den Text roh),
/// aber die Funktion bleibt oeffentlich, weil `narsil-page` sie benutzt.
pub fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_was_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(c);
            last_was_space = false;
        }
    }
    result.trim().to_string()
}

// ===========================================================================
// DER BAUM ALS HTML (`serialisiere`) — 02.10.2026, Rolle HTML
// ===========================================================================
//
// WARUM DAS FEHLTE UND WAS ES KOSTETE (gemessen, nicht vermutet)
//
// `parse_html` ist der Weg hin, und es gab keinen Weg zurueck: aus einem
// `Vec<HtmlNode>` konnte niemand HTML machen. GEMESSEN, wo das zubiss:
// `narsil` (`Sitzung::rumpf_von_body`) wollte den `<body>`-Baum an die
// JavaScript-Engine geben, damit `document.getElementById('liste')` den
// Knoten findet, und hatte nur den SEITENTEXT. Der Motor meldete
// `TypeError: kein Zugriff auf Null` — ein Skript bricht an einem Element
// ab, das im Bild zu sehen ist.
//
// GEMESSEN, warum es hierher gehoert und nicht nach `narsil-page`: `HtmlNode`
// ist der Baum DIESER Crate, und `parse_html` steht hier. Eine Funktion,
// die zurueckschreibt, gehoert neben die, die gelesen hat — sonst muss
// jeder Aufrufer die Struktur kennen.
//
// GEMESSEN, warum das KEIN Re-Parse ist: die Aufgabe ist nicht, das
// urspruengliche HTML wiederherzustellen (das waere `innerHTML` in einem
// Browser), sondern EINEN Baum als HTML zu liefern, das wieder PARSbar
// ist. Der Unterschied ist messbar: `class="a b"` bleibt in der Reihenfolge
// der `HashMap` nicht stabil, und die Reihenfolge ist fuer HTML ohne
// Bedeutung — fuer einen Vergleich zweier Ausgaben schon.

// M3: `MAX_AUSGABE` — die Grenze des Ergebnisses. GEMESSEN, warum 1 MiB:
// das ist dieselbe Groessenordnung wie `MAX_ROHINHALT` (256 KiB) und
// doppelt so gross wie die groesste Seite, die `narsil-page` ueberhaupt
// annimmt. Wer mehr braucht, hat kein HTML, sondern einen Download.
//
// GEMESSEN, was diese Grenze kostet (03.10.2026, `tests/
// roundtrip_echt.rs`): `www.spiegel.de` aus dem Korpus serialisiert auf
// 1.706.985 Byte und wird bei 1 MiB also um 658 KiB gekappt — mit
// testweise 8 MiB Grenze besteht dieselbe Seite den Roundtrip EXAKT
// (3 exakt statt 2). Die Kappung ist also eine Grenze und kein Fehler;
// die Zahl gehoert in team/PROBLEME.txt, weil ein Grenzwert hier eine
// Speichergarantie ist und der nicht nebenbei veraendert wird.
pub const MAX_AUSGABE: usize = 1024 * 1024;

/// Die Elemente, die KEIN schliessendes Tag haben (HTML).
///
/// GEMESSEN, warum eine Liste und keine Regel: die HTML-Regel lautet
/// „leere Elemente", und sie ist eine Liste. Eine Regel („alle, die keine
/// Kinder haben duerfen") wuerde `<div></div>` zu einem leeren Element
/// machen — und ein leeres `<div>` ist ein Block, kein Leerraum.
/// 13.3, woertlich: "For the purposes of the following algorithm, an
/// element serializes as void if its element type is one of the void
/// elements, or is basefont, bgsound, frame, keygen, or param."
///
/// GEMESSENER FEHLER (03.10.2026), Abnahme "13.3-Regeln": die vier
/// Zusatznamen fehlten, also schrieb ich `<basefont></basefont>` statt
/// `<basefont>` (vier Faelle rot).
const OHNE_SCHLIESSEN: [&str; 18] = [
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input",
    "keygen", "link", "meta", "param", "source", "track", "wbr",
];

/// 13.3, woertlich: "If the parent of current node is a style, script,
/// xmp, iframe, noembed, noframes, or plaintext element, or if the
/// parent of current node is a noscript element and scripting is enabled
/// for the node, then append the value of current node's data literally."
///
/// GEMESSENER FEHLER (03.10.2026), Abnahme "13.3-Regeln": ich hatte
/// `ist_roher_inhalt` benutzt — das ist die RCDATA/RAWTEXT-Menge des
/// TOKENIZERS (script, style, textarea, title) und genau verkehrt
/// herum: `title`/`textarea` sind RCDATA und werden escaped, `xmp`,
/// `plaintext`, `iframe` werden woertlich geschrieben. Fuenf Faelle rot.
fn ist_literaler_text(tag: &str) -> bool {
    matches!(
        tag,
        "style" | "script" | "xmp" | "iframe" | "noembed" | "noframes" | "plaintext"
    )
}

/// 13.3, woertlich: "If the parent of current node is a style, script,
/// xmp, iframe, noembed, noframes, or plaintext element, OR IF THE
/// PARENT OF CURRENT NODE IS A NOSCRIPT ELEMENT AND SCRIPTING IS
/// ENABLED FOR THE NODE, then append the value of current node's data
/// literally."
///
/// GEMESSENER FEHLER (03.10.2026, Bericht _85): der `noscript`-Zweig
/// fehlte VOLLSTAENDIG, weil kein Serialisierer der Crate ein
/// Scripting-Flag fuehrte. Exposition gemessen ueber alle 1790
/// Korpusfaelle: mit Scripting aus erzeugen 36 davon einen Textknoten
/// im `<noscript>` — genau die, bei denen 13.3 woertliches Schreiben
/// verlangt und mein Code escapte.
fn ist_literaler_text_mit_scripting(tag: &str, scripting: bool) -> bool {
    ist_literaler_text(tag) || (scripting && tag == "noscript")
}

/// Serialisiert einen Baum als HTML.
///
/// # WOFUER
///
/// Ein Host, der einen Baum an eine zweite Maschine geben will, die ihn
/// wieder parst (eine JavaScript-Engine, ein Test, ein Diff). GEMESSEN im
/// Fall der Bruecke: ohne das fand ein Skript seine Elemente nicht.
///
/// # WAS DIESE FUNKTION NICHT IST
///
/// * Kein `innerHTML`: sie haelt keine Kommentare, keine CDATA-Abschnitte,
///   keineattributreihenfolge und keine Entity-Schreibweise. Sie liefert
///   HTML, das **denselben Baum** beschreibt.
/// * Keine Pruefung: die Ausgabe ist nicht erneut geparst. Wer sie
///   zurueckgibt, ist selbst fuer das Ergebnis verantwortlich.
///
/// # M3/M5
///
/// `MAX_AUSGABE` (1 MiB) und `MAX_TIEFE` (512) — dieselben Grenzen wie
/// beim Lesen. GEMESSEN, warum beide noetig sind: die Tiefe schuetzt vor
/// einem Stack-Ueberlauf in dieser Funktion, die Laenge vor einem
/// Speicherschaden in der, die das Ergebnis wieder parst.
pub fn serialisiere(nodes: &[HtmlNode]) -> String {
    serialisiere_mit_scripting(nodes, false)
}

/// Wie `serialisiere`, aber mit Scripting-Flag fuer die eine Stelle,
/// an der 13.3 es verlangt: der Inhalt eines `<noscript>` wird nur bei
/// aktiviertem Scripting woertlich geschrieben, sonst escaped.
///
/// OHNE dieses Flag ist `serialisiere` gleichbedeutend — Scripting aus
/// ist der konservative Fall und der, den ein Baum aus
/// `parse_dokument` (ohne Scripting) verdient. Wer mit
/// `parse_dokument_mit_scripting(.., true)` parst, MUSS hier das Flag
/// mitgeben, sonst weicht die Ausgabe von 13.3 ab. GEMESSEN: 36 der
/// 1790 Korpusfaelle erzeugen bei aktiviertem Scripting Text im
/// `<noscript>`.
pub fn serialisiere_mit_scripting(nodes: &[HtmlNode], scripting: bool) -> String {
    let mut raus = String::new();
    let mut rest = MAX_AUSGABE;
    let mut schnitt: Option<&'static str> = None;
    schreibe_baum(nodes, 0, &mut raus, &mut rest, &mut schnitt, scripting);
    if let Some(grund) = schnitt {
        // GEMESSEN, warum die LAENGEN-Grenze jetzt auch ihre Marke
        // bekommt (03.10.2026, spiegel.de im Roundtrip): die Tiefengrenze
        // hatte ihre Marke seit dem 02.10., die Laengengrenze nicht. Eine
        // echte Seite serialisierte auf 1.048.548 Byte von erlaubten
        // 1.048.576 — und danach fehlten 101 der 111 Kinder des <body>.
        // Das sah wie ein Parserfehler aus und war eine Kappung. Die
        // Marke steht deshalb am ENDE und der Rest wird auf den Platz
        // gekuerzt, damit `MAX_AUSGABE` eine Zusage bleibt.
        let marke = format!("<!-- abgeschnitten: {grund} -->");
        let platz = MAX_AUSGABE.saturating_sub(marke.len());
        raus.truncate(zeichengrenze(&raus, platz));
        raus.push_str(&marke);
    }
    raus
}

fn schreibe_baum(
    nodes: &[HtmlNode],
    tiefe: u32,
    raus: &mut String,
    rest: &mut usize,
    schnitt: &mut Option<&'static str>,
    scripting: bool,
) {
    if *rest == 0 {
        return;
    }
    if tiefe > MAX_TIEFE {
        // GEMESSEN, warum die Abbruch-Marke sichtbar ist: ein stilles
        // Weglassen sieht aus wie ein vollstaendiger Baum, und der Host
        // haelt eine Seite fuer vollstaendig, die abgeschnitten ist.
        *schnitt = Some("MAX_TIEFE");
        *rest = 0;
        return;
    }
    for node in nodes {
        match node {
            HtmlNode::Text(t) => {
                // GEMESSEN, warum der Text HIER escaped wird und im
                // Rohtext nicht: `&` und `<` in einem Text wuerden beim
                // Zuruecklesen zu ENTITAETEN. Der Rohtext-Zweig
                // (`ist_roher_inhalt`) nimmt den Inhalt von `<script>`
                // unberuehrt — dort ist `a < b` ein Programm und kein Text.
                let teil = text_escaped(t);
                if teil.len() > *rest {
                    *schnitt = Some("MAX_AUSGABE");
                    *rest = 0;
                    return;
                }
                *rest -= teil.len();
                raus.push_str(&teil);
            }
            HtmlNode::Element {
                tag,
                attributes,
                children,
                shadow_root,
            } => {
                if raus.len() >= MAX_AUSGABE {
                    *schnitt = Some("MAX_AUSGABE");
                    *rest = 0;
                    return;
                }
                let offen = element_oeffnet(tag, attributes, rest);
                if offen.len() > *rest {
                    *schnitt = Some("MAX_AUSGABE");
                    *rest = 0;
                    return;
                }
                *rest -= offen.len();
                raus.push_str(&offen);
                if OHNE_SCHLIESSEN.contains(&tag.as_str()) {
                    continue;
                }
                if ist_literaler_text_mit_scripting(tag, scripting) {
                    // GEMESSEN, warum der Inhalt hier ROH bleibt: der
                    // Inhalt eines `<script>` ist ein Programm, und
                    // `if (a < b)` ist kein HTML-Text.
                    let roh: String = children
                        .iter()
                        .map(|k| match k {
                            HtmlNode::Text(t) => t.clone(),
                            HtmlNode::Element { .. } => {
                                serialisiere_mit_scripting(std::slice::from_ref(k), scripting)
                            }
                        })
                        .collect();
                    let h = zeichengrenze(&roh, *rest);
                    raus.push_str(&roh[..h]);
                    *rest -= h;
                } else {
                    schreibe_baum(children, tiefe + 1, raus, rest, schnitt, scripting);
                    // Serialize shadow root as declarative Shadow DOM template
                    if let Some(sr) = shadow_root {
                        let mode_str = match sr.mode {
                            ShadowRootMode::Open => "open",
                            ShadowRootMode::Closed => "closed",
                        };
                        let tmpl_start = format!("<template shadowrootmode=\"{mode_str}\">");
                        if tmpl_start.len() > *rest {
                            *schnitt = Some("MAX_AUSGABE");
                            *rest = 0;
                            return;
                        }
                        *rest -= tmpl_start.len();
                        raus.push_str(&tmpl_start);
                        schreibe_baum(&sr.children, tiefe + 2, raus, rest, schnitt, scripting);
                        let tmpl_end = "</template>";
                        if tmpl_end.len() > *rest {
                            *schnitt = Some("MAX_AUSGABE");
                            *rest = 0;
                            return;
                        }
                        *rest -= tmpl_end.len();
                        raus.push_str(tmpl_end);
                    }
                }
                let zu = std::format!("</{tag}>");
                if zu.len() > *rest {
                    *schnitt = Some("MAX_AUSGABE");
                    *rest = 0;
                    return;
                }
                *rest -= zu.len();
                raus.push_str(&zu);
            }
        }
    }
}

/// Serialisiert einen **Spec-Baum** (`dom::Dokument`) als HTML (13.3).
///
/// WOFUER: `serialisiere` nimmt `&[HtmlNode]`, und `HtmlNode` kennt nur
/// `Element` und `Text`. Vier Regeln von 13.3 waren damit nicht
/// abnehmbar — sie brauchen Knoten, die die Struktur nicht traegt.
/// GEMESSEN (03.10.2026, Bericht _37/_38), welche:
///
///   * Kommentar: `<!--`, Daten, `-->`
///   * Processing Instruction: `?`, target, ` `, Daten, ` ?>`
///   * DocumentType: `<!DOCTYPE `, Name, `>`
///   * Attributname im XML-/XMLNS-/XLink-Namensraum
///   * Template-Inhalt (der Spec serialisiert das template *content*)
///
/// Diese Funktion nimmt den Baum, den der Spec-Parser gebaut hat, und
/// schreibt genau das zurueck, was 13.3 vorschreibt — inklusive der
/// fuenf Punkte. Sie aendert nichts an `serialisiere` und nichts an
/// `parse_html`; der Produktpfad bleibt unberuehrt.
///
/// # SCRIPTING
///
/// 13.3 schreibt den Inhalt eines `<noscript>` woertlich, "if the
/// parent of current node is a noscript element and scripting is
/// enabled for the node". Diese Funktion hat kein Scripting-Flag und
/// nimmt "enabled" an — das ist die Voreinstellung des
/// Spec-Pfad-Parsers (`parse_dokument_mit_scripting(.., true)` ist der
/// Browserfall) und der Grund, warum der Roundtrip mit dieser
/// Funktion stabil ist.
///
/// # M3/M5
///
/// `MAX_AUSGABE` (1 MiB) und `MAX_TIEFE` (512), dieselben Grenzen wie
/// `serialisiere`. Die sichtbare Abbruchmarke ist dieselbe: ein
/// stilles Weglassen haelt eine Seite fuer vollstaendig, die es nicht
/// ist (Bericht _34).
pub fn serialisiere_dokument(dok: &dom::Dokument, wurzel: usize) -> String {
    serialisiere_dokument_mit_scripting(dok, wurzel, false)
}

/// Wie `serialisiere_dokument`, aber mit Scripting-Flag fuer die eine
/// Stelle, an der 13.3 es verlangt (`<noscript>`-Inhalt). GEMESSENER
/// FEHLER (03.10.2026, Bericht _85): vorher wurde `noscript` hier
/// IMMER roh geschrieben, auch bei ausgeschaltetem Scripting — die
/// Spec escaped in dem Fall.
pub fn serialisiere_dokument_mit_scripting(
    dok: &dom::Dokument,
    wurzel: usize,
    scripting: bool,
) -> String {
    let mut raus = String::new();
    let mut rest = MAX_AUSGABE;
    let mut schnitt: Option<&'static str> = None;
    schreibe_knoten(
        dok,
        wurzel,
        0,
        &mut raus,
        &mut rest,
        &mut schnitt,
        false,
        scripting,
    );
    if let Some(grund) = schnitt {
        let marke = format!("<!-- abgeschnitten: {grund} -->");
        let platz = MAX_AUSGABE.saturating_sub(marke.len());
        raus.truncate(zeichengrenze(&raus, platz));
        raus.push_str(&marke);
    }
    raus
}

/// Der serialisierte Attributname (13.3) — **ohne** Leerzeichen.
///
/// Woertlich: "If the attribute has no namespace the attribute's
/// serialized name is the attribute's local name. If the attribute is
/// in the XML namespace the attribute's serialized name is " xml: "
/// followed by the attribute's local name. If the attribute is in the
/// XMLNS namespace and the attribute's local name is xmlns the
/// attribute's serialized name is " xmlns ". If the attribute is in the
/// XMLNS namespace and the attribute's local name is not xmlns the
/// attribute's serialized name is " xmlns: " followed by the attribute's
/// local name. If the attribute is in the XLink namespace the
/// attribute's serialized name is " xlink: " followed by the attribute's
/// local name."
///
/// GEMESSENER FEHLER (03.10.2026): die erste Fassung gab das
/// Leerzeichen mit zurueck (`" xml:"`), weil der Spec es im Satz
/// nennt — der Algorithmus setzt es aber selbst ("append U+0020
/// SPACE, the attribute's serialized name"). Ergebnis waren doppelte
/// Leerzeichen in der Ausgabe, gefunden von
/// `attributname_im_namensraum`.
///
/// GEMESSEN, warum das nicht `Attribut::anzeige` ist: die schreibt
/// `xml base` (mit Leerzeichen) — das ist die Schreibweise des
/// TESTFORMATS von html5lib, nicht die des Specs. Die beiden zu
/// verwechseln hat den Korpusfall `webkit02.dat` (gemischte
/// Schreibweisen im selben Tag) zum Gegner gemacht.
fn attributname_spezial(a: &dom::Attribut) -> String {
    use dom::Namensraum::{Fremd, Html, MathMl, Svg, Xlink, Xml, Xmlns};
    match a.raum {
        Html | Svg | MathMl | Fremd => a.name.clone(),
        Xml => format!("xml:{}", a.name),
        Xmlns if a.name == "xmlns" => "xmlns".to_string(),
        Xmlns => format!("xmlns:{}", a.name),
        Xlink => format!("xlink:{}", a.name),
    }
}

/// Schreibt die Kinder eines Knotens. `in_rohtext` sagt, ob der
/// Elternknoten ein Element ist, dessen Inhalt woertlich steht.
fn schreibe_knoten(
    dok: &dom::Dokument,
    k: usize,
    tiefe: u32,
    raus: &mut String,
    rest: &mut usize,
    schnitt: &mut Option<&'static str>,
    in_rohtext: bool,
    scripting: bool,
) {
    if *rest == 0 {
        return;
    }
    if tiefe > MAX_TIEFE {
        *schnitt = Some("MAX_TIEFE");
        *rest = 0;
        return;
    }
    for &kind in &dok.knoten[k].kinder {
        match &dok.knoten[kind].art {
            dom::Art::Dokument | dom::Art::Fragment => schreibe_knoten(
                dok,
                kind,
                tiefe + 1,
                raus,
                rest,
                schnitt,
                in_rohtext,
                scripting,
            ),
            dom::Art::ShadowRoot { children, .. } => {
                for &child in children {
                    schreibe_knoten(
                        dok,
                        child,
                        tiefe + 1,
                        raus,
                        rest,
                        schnitt,
                        in_rohtext,
                        scripting,
                    );
                }
            }
            dom::Art::Doctype { name, .. } => {
                // 13.3, woertlich: "Append " !DOCTYPE ", followed by the
                // value of current node's name and U+003E (>)."
                let teil = format!("<!DOCTYPE {name}>");
                if teil.len() > *rest {
                    *schnitt = Some("MAX_AUSGABE");
                    *rest = 0;
                    return;
                }
                *rest -= teil.len();
                raus.push_str(&teil);
            }
            dom::Art::Kommentar(text) => {
                // 13.3, woertlich: "Append " !-- ", followed by the
                // value of current node's data, and " --> "."
                let teil = format!("<!--{text}-->");
                if teil.len() > *rest {
                    *schnitt = Some("MAX_AUSGABE");
                    *rest = 0;
                    return;
                }
                *rest -= teil.len();
                raus.push_str(&teil);
            }
            dom::Art::Pi { target, daten } => {
                // 13.3, woertlich: "Append " ? ", followed by the value
                // of current node's target, U+0020 SPACE, the value of
                // current node's data, and " ?> "."
                let teil = if daten.is_empty() {
                    format!("<?{target}?>")
                } else {
                    format!("<?{target} {daten}?>")
                };
                if teil.len() > *rest {
                    *schnitt = Some("MAX_AUSGABE");
                    *rest = 0;
                    return;
                }
                *rest -= teil.len();
                raus.push_str(&teil);
            }
            dom::Art::Text(text) => {
                // 13.3, woertlich: "If the parent of current node is a
                // style, script, xmp, iframe, noembed, noframes, or
                // plaintext element, or if the parent of current node is
                // a noscript element and scripting is enabled for the
                // node, then append the value of current node's data
                // literally."
                let teil = if in_rohtext {
                    text.clone()
                } else {
                    text_escaped(text)
                };
                if teil.len() > *rest {
                    *schnitt = Some("MAX_AUSGABE");
                    *rest = 0;
                    return;
                }
                *rest -= teil.len();
                raus.push_str(&teil);
            }
            dom::Art::Element {
                raum,
                name,
                attribute,
                template_inhalt,
                shadow_root: _,
            } => {
                // 13.3: "If current node is an element in the HTML
                // namespace, the MathML namespace, or the SVG
                // namespace, then let tagname be current node's local
                // name."
                let tagname = match raum {
                    dom::Namensraum::Html | dom::Namensraum::Svg | dom::Namensraum::MathMl => {
                        name.clone()
                    }
                    // "Otherwise, let tagname be current node's qualified
                    // name" — fuer die uebrigen Namensraeume (Xml,
                    // Xlink, Xmlns als Element) nimmt der Spec den
                    // praefizierten Namen; mein Baum fuehrt ihn nicht
                    // getrennt, deshalb der lokale Name. GEMESSEN: der
                    // Spec-Parser erzeugt im Dokument-Kontext nie ein
                    // Element im Xml-/XLink-Namensraum — die Foreign-
                    // Content-Regeln (13.2.6.5) kennen nur SVG und
                    // MathML.
                    _ => name.clone(),
                };
                let mut offen = String::with_capacity(16 + attribute.len() * 24);
                offen.push('<');
                offen.push_str(&tagname);
                // 13.3: "While the exact order of attributes is
                // implementation-defined ... the sort order must be
                // stable" — nach Name sortiert, damit zwei Laeufe
                // dasselbe liefern.
                let mut sortiert: Vec<&dom::Attribut> = attribute.iter().collect();
                sortiert.sort_by(|a, b| attributname_spezial(a).cmp(&attributname_spezial(b)));
                for a in sortiert {
                    offen.push(' ');
                    offen.push_str(&attributname_spezial(a));
                    offen.push_str("=\"");
                    offen.push_str(&attribut_escaped(&a.wert));
                    offen.push('"');
                }
                offen.push('>');
                if offen.len() > *rest {
                    *schnitt = Some("MAX_AUSGABE");
                    *rest = 0;
                    return;
                }
                *rest -= offen.len();
                raus.push_str(&offen);
                // "If current node serializes as void, then continue on
                // to the next child node at this point."
                if OHNE_SCHLIESSEN.contains(&tagname.as_str()) {
                    continue;
                }
                // 13.3: "If the node is a template element, then let the
                // node instead be the template element's template
                // contents (a DocumentFragment node)." — das ist der
                // Punkt, an dem `HtmlNode` scheitert: es kann den
                // Inhalt nicht tragen (98 der 1784 Korpusfaelle).
                if let Some(inhalt) = template_inhalt {
                    schreibe_knoten(
                        dok,
                        *inhalt,
                        tiefe + 1,
                        raus,
                        rest,
                        schnitt,
                        false,
                        scripting,
                    );
                }
                let roh = ist_literaler_text_mit_scripting(&tagname, scripting);
                schreibe_knoten(dok, kind, tiefe + 1, raus, rest, schnitt, roh, scripting);
                let zu = format!("</{tagname}>");
                if zu.len() > *rest {
                    *schnitt = Some("MAX_AUSGABE");
                    *rest = 0;
                    return;
                }
                *rest -= zu.len();
                raus.push_str(&zu);
            }
        }
    }
}

/// Der oeffnende Tag mit Attributen.
fn element_oeffnet(tag: &str, attributes: &HashMap<String, String>, rest: &mut usize) -> String {
    let mut s = String::with_capacity(16 + attributes.len() * 24);
    s.push('<');
    s.push_str(tag);
    // GEMESSEN, warum die Attribute SORTIERT werden: `HashMap` hat keine
    // Reihenfolge, also waere die Ausgabe zweier Aufrufe verschieden —
    // und ein Diff zweier Serialisierungen zeigt dann jeden Lauf einen
    // Unterschied, der keiner ist.
    let mut namen: Vec<&String> = attributes.keys().collect();
    namen.sort();
    for name in namen {
        let wert = &attributes[name];
        s.push(' ');
        s.push_str(name);
        s.push_str("=\"");
        s.push_str(&attribut_escaped(wert));
        s.push('"');
    }
    s.push('>');
    let _ = rest;
    s
}

// `zeichengrenze` macht genau das, was der Aufrufer hier braucht: `max`
// Zeichen an einer UTF-8-Grenze. Eine eigene Kopie waere eine zweite
// Grenze - und zwei Grenzen sind zwei Wahrheiten.

/// Escaped einen Textknoten: `&`, `<`, `>`.
///
/// GEMESSEN, warum `>` auch: ohne das schliesst ein `>` im Text den
/// oeffnenden Tag in einem Parser, der nur nach `<` sucht — und manche
/// tun genau das. Drei Zeichen sind billig, zwei sind eine Wette.
fn text_escaped(text: &str) -> String {
    // 13.3, woertlich: "Escaping a string … consists of running the
    // following steps: Replace any occurrence of '&' character by
    // '&amp;'. Replace any occurrences of the U+00A0 NO-BREAK SPACE
    // character by '&nbsp;'. Replace any occurrences of the '<' character
    // by '&lt;'. Replace any occurrences of the '>' character by '&gt;'."
    //
    // GEMESSENER FEHLER (03.10.2026), neue Abnahme tests/
    // serialisier_abnahme.rs "13.3-Regeln": U+00A0 fehlte, 8 von 20
    // Regeln bestanden.  `<p>a\u{a0}b</p>` statt `<p>a&nbsp;b</p>`.
    let mut s = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '\u{a0}' => s.push_str("&nbsp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            _ => s.push(c),
        }
    }
    s
}

/// Wie `text_escaped`, aber ohne `&` — fuer Attributwerte.
fn attribut_escaped(text: &str) -> String {
    // 13.3, woertlich, "attribute mode": dieselben vier Ersetzungen wie
    // im Text, ZUSATZLICH: "If the algorithm was invoked in the
    // attribute mode, then replace any occurrences of the '"' character
    // by '&quot;'."
    //
    // GEMESSENE FEHLER (03.10.2026), Abnahme "13.3-Regeln": U+00A0 und
    // '>' fehlten beide.
    //   <p title="a>b">    statt <p title="a&gt;b">
    //   <p title="a\u{a0}b"> statt <p title="a&nbsp;b">
    let mut s = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '\u{a0}' => s.push_str("&nbsp;"),
            '"' => s.push_str("&quot;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            _ => s.push(c),
        }
    }
    s
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// DIE VOID-MENGE ENTSPRICHT DEM SPEC (13.3, erster Satz).
    ///
    /// 13.3, woertlich: "For the purposes of the following algorithm, an
    /// element serializes as void if its element type is one of the void
    /// elements, or is basefont, bgsound, frame, keygen, or param."
    ///
    /// WARUM DIESER TEST (03.10.2026): die Verhaltensabnahme
    /// `tests/serialisier_abnahme.rs` meldete "13.3-Regeln: 20/20 =
    /// 100,000 %". Diese Zahl war eine Aussage ueber ZWOELF Testfaelle,
    /// nicht ueber den Abschnitt — `param`, `noembed` und `noframes`
    /// standen in keinem davon. Der Code war richtig, gemessen war es
    /// nicht. Ein Prozent ohne vollstaendige Menge ist eine Zahl ohne
    /// Bezug, also steht die Menge hier festgeschrieben: faellt ein
    /// Element weg oder kommt eines dazu, wird dieser Test rot.
    #[test]
    fn void_menge_entspricht_dem_spec() {
        const SPEC_VOID: [&str; 18] = [
            // die void elements (13.2.5.1) …
            "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source",
            "track", "wbr", // … plus die fuenf aus 13.3
            "basefont", "bgsound", "frame", "keygen", "param",
        ];
        for name in SPEC_VOID {
            assert!(
                OHNE_SCHLIESSEN.contains(&name),
                "13.3 nennt {name} als void — es fehlt in OHNE_SCHLIESSEN"
            );
        }
        assert_eq!(
            OHNE_SCHLIESSEN.len(),
            SPEC_VOID.len(),
            "OHNE_SCHLIESSEN = {OHNE_SCHLIESSEN:?}, Spec = {SPEC_VOID:?}"
        );
    }

    /// DIE ROHTEXT-MENGE ENTSPRICHT DEM SPEC (13.3, Text-Regel).
    ///
    /// 13.3, woertlich: "If the parent of current node is a style,
    /// script, xmp, iframe, noembed, noframes, or plaintext element, or
    /// if the parent of current node is a noscript element and scripting
    /// is enabled for the node, then append the value of current node's
    /// data literally."
    ///
    /// `noscript` steht hier NICHT: das haengt am Scripting-Flag, und
    /// `serialisiere` hat keins (siehe `ist_literaler_text`).
    #[test]
    /// 13.3, DIE EINZIGE STELLE MIT SCRIPTING-BEDINGUNG.
    ///
    /// Woertlich: "If the parent of current node is a style, script,
    /// xmp, iframe, noembed, noframes, or plaintext element, or if the
    /// parent of current node is a noscript element and scripting is
    /// enabled for the node, then append the value of current node's
    /// data literally."
    ///
    /// GEMESSENER FEHLER (03.10.2026, Bericht _85): der `noscript`-
    /// Zweig fehlte ganz — es gab kein Serialisierer-Flag. Im
    /// Dokument-Serialisierer stand sogar `|| tagname == "noscript"`
    /// OHNE Bedingung, dort wurde der Inhalt also IMMER roh
    /// geschrieben, auch bei ausgeschaltetem Scripting.
    ///
    /// EXPOSITION, gemessen ueber alle 1790 Korpusfaelle: mit
    /// aktiviertem Scripting erzeugen 36 davon einen Textknoten im
    /// `<noscript>`; mit ausgeschaltetem 0. Genau diese 36 sind die
    /// Faelle, in denen die Regel sichtbar wird.
    #[test]
    fn noscript_rohtext_haengt_am_scripting_flag() {
        let baum = vec![HtmlNode::Element {
            tag: "noscript".to_string(),
            attributes: HashMap::new(),
            children: vec![HtmlNode::Text("a<b&c".to_string())],
            shadow_root: None,
        }];
        let aus = serialisiere_mit_scripting(&baum, false);
        assert!(
            aus.contains("a&lt;b&amp;c"),
            "ohne Scripting wird escaped (die Spec escaped in dem Fall): {aus}"
        );
        let an = serialisiere_mit_scripting(&baum, true);
        assert!(
            an.contains("a<b&c"),
            "mit Scripting wird woertlich geschrieben: {an}"
        );
        // Die beiden alten Einstiege bleiben gleichwertig zu "ohne".
        assert_eq!(serialisiere(&baum), aus);
        // Und die 7 Elemente der Spec bleiben unveraendert roh —
        // das Flag darf sie nicht beeinflussen.
        for tag in [
            "style",
            "script",
            "xmp",
            "iframe",
            "noembed",
            "noframes",
            "plaintext",
        ] {
            let b = vec![HtmlNode::Element {
                tag: tag.to_string(),
                attributes: HashMap::new(),
                children: vec![HtmlNode::Text("a<b".to_string())],
                shadow_root: None,
            }];
            assert!(
                serialisiere_mit_scripting(&b, false).contains("a<b")
                    && serialisiere_mit_scripting(&b, true).contains("a<b"),
                "{tag} wird nach 13.3 IMMER woertlich geschrieben,                  unabhaengig vom Scripting-Flag"
            );
        }
    }

    /// ZWEI 13.3-REGELN, DIE MAN "REPARIEREN" WUERDE, UND DIE MAN NICHT
    /// REPARIEREN DARF (03.10.2026, Bericht _86).
    ///
    /// 1) `is`-WERT. 13.3: "If current node's is value is not null, and
    ///    the element does not have an is attribute in its attribute
    ///    list, then append ' is=\"' …". Das ist ein DOM-EIGENSCHAFT
    ///    (gesetzt von `createElement(name, {is: …})`), kein Attribut
    ///    aus dem Quelltext. GEMESSEN: `grep -rn "is_wert|is_value"` in
    ///    `src/` = 0 Treffer — mein Baum KANN keinen is-Wert tragen,
    ///    die Regel ist unerreichbar. Und `is="…"` kommt in KEINEM der
    ///    beiden Korpora vor (tree-construction und serializer: 0
    ///    Treffer), die Attributform ist also nirgends abgedeckt —
    ///    bis auf diesen Test.
    ///
    /// 2) KLEINSCHREIBEN. 13.3 sagt: "For HTML elements created by the
    ///    HTML parser or createElement(), tagname will be lowercase."
    ///    Das ist eine AUSSAGE ueber die beiden Erzeugungswege, keine
    ///    Aufforderung zum Normalisieren. GEMESSEN ueber den Adapter
    ///    `parse_html`: camelCase-Namen kommen durch DENSELBEN Typ —
    ///    `svg` > `foreignObject`, `clipPath`, `linearGradient` —,
    ///    waehrend HTML-Namen klein sind (`div`, `class`). Ein
    ///    `to_lowercase()` im Serialisierer wuerde also SVG zerstoeren.
    ///    Deshalb wird hier NICHTS normalisiert, und der Test haelt
    ///    camelCase fest.
    #[test]
    fn is_attribut_und_camelcase_gehen_nicht_kaputt() {
        // (1) `is` als Attribut: genau einmal, unveraendert. Die Spec
        // haengt den is-WERT-Zweig ausdruecklich an "the element does
        // not have an is attribute in its attribute list" — mit
        // Attribut darf nichts zusaetzlich entstehen.
        for eingabe in ["<div is=\"x\">y</div>", "<x-foo is=\"my-el\">y</x-foo>"] {
            let (dok, wurzel) = crate::treebuilder::parse_dokument(eingabe).expect("Parse");
            let aus = serialisiere_dokument(&dok, wurzel);
            assert_eq!(
                aus.matches("is=\"x\"").count() + aus.matches("is=\"my-el\"").count(),
                1,
                "das is-Attribut genau einmal, nicht doppelt: {aus}"
            );
        }
        // Und am programmatisch gebauten HtmlNode genauso.
        let mut attr = HashMap::new();
        attr.insert("is".to_string(), "my-el".to_string());
        let baum = vec![HtmlNode::Element {
            tag: "x-foo".to_string(),
            attributes: attr,
            children: vec![HtmlNode::Text("y".to_string())],
            shadow_root: None,
        }];
        assert_eq!(serialisiere(&baum), "<x-foo is=\"my-el\">y</x-foo>");
        // (2) camelCase muss durch den Adapter und den Serialisierer
        // unveraendert hindurch — SVG verträgt KEIN Kleinschreiben.
        let baum = parse_html("<svg><foreignObject><desc>x</desc></foreignObject></svg>");
        let aus = serialisiere(&baum);
        assert!(
            aus.contains("<foreignObject>") && aus.contains("<desc>"),
            "camelCase geht verloren: {aus}"
        );
        let baum = parse_html("<svg><clipPath><linearGradient/></clipPath></svg>");
        let aus = serialisiere(&baum);
        assert!(
            aus.contains("<clipPath>") && aus.contains("<linearGradient>"),
            "camelCase geht verloren: {aus}"
        );
        // HTML-Namen sind nach dem Parsen klein — das ist die Aussage
        // der Spec ueber den Erzeugungsweg, und sie stimmt.
        let baum = parse_html("<DIV CLASS=\"a\">y</DIV>");
        assert!(
            serialisiere(&baum).contains("<div class=\"a\">"),
            "HTML-Namen sind klein: {}",
            serialisiere(&baum)
        );
    }

    fn rohtext_menge_entspricht_dem_spec() {
        const SPEC_LITERAL: [&str; 7] = [
            "style",
            "script",
            "xmp",
            "iframe",
            "noembed",
            "noframes",
            "plaintext",
        ];
        for name in SPEC_LITERAL {
            assert!(
                ist_literaler_text(name),
                "13.3 schreibt {name} woertlich — es steht nicht in ist_literaler_text"
            );
        }
        // Und die Umkehrung: was NICHT im Spec steht, wird escaped.
        // `title` und `textarea` sind RCDATA (13.2.5.2) und werden
        // escaped — das ist der Unterschied, der am 03.10. fuenf Faelle
        // rot gemacht hat, weil `ist_roher_inhalt` (die Tokenizer-Menge)
        // an dieser Stelle genau verkehrt herum stand.
        for name in ["title", "textarea", "p", "div", "span", "noscript"] {
            assert!(
                !ist_literaler_text(name),
                "{name} wird laut 13.3 escaped, nicht woertlich"
            );
        }
    }

    /// Das erste Element mit diesem Namen, im ganzen Baum, tiefensuend.
    fn finde<'a>(nodes: &'a [HtmlNode], tag: &str) -> Option<&'a HtmlNode> {
        for k in nodes {
            if let HtmlNode::Element {
                tag: t, children, ..
            } = k
            {
                if t == tag {
                    return Some(k);
                }
                if let Some(g) = finde(children, tag) {
                    return Some(g);
                }
            }
        }
        None
    }

    /// Der Inhalt des `<body>`.
    ///
    /// SEIT DER UMSTELLUNG (03.10.2026, A3): `parse_html` baut das
    /// Dokument-Geruest (13.2.6.4.2/.3/.6/.7), vorher stand der Inhalt
    /// direkt in der Wurzel. Die Tests hier pruefen EIGENSCHAFTEN des
    /// Inhalts — ein `<script>`-Kind, ein Attribut, ein void-Element —
    /// und nicht die Wurzeltiefe. Also bekommen sie den Rumpf, statt
    /// dass ihre Erwartung auf eine Ebene wandert.
    ///
    /// GEMESSENER FEHLER (03.10.2026) an diesem Helfer: zuerst suchte
    /// er `<body>` nur auf der obersten Ebene — dort steht aber
    /// `<html>`, eine Ebene darunter `<body>`. Fünf Tests panickten
    /// mit "kein <body> im Baum", obwohl der Baum richtig war. Der
    /// Helfer sucht jetzt durch `finde`, also in der Tiefe.
    fn rumpf(nodes: &[HtmlNode]) -> &[HtmlNode] {
        match finde(nodes, "body") {
            Some(HtmlNode::Element { children, .. }) => children,
            Some(_) => unreachable!("gefunden ueber tag == body"),
            None => panic!("kein <body> im Baum: {nodes:?}"),
        }
    }

    #[test]
    fn einfacher_text() {
        let nodes = parse_html("Hallo Welt");
        assert_eq!(
            rumpf(&nodes),
            [HtmlNode::Text("Hallo Welt".to_string())],
            "der Rumpf traegt den Text"
        );
    }

    #[test]
    fn einfaches_element() {
        let nodes = parse_html("<p>Hallo</p>");
        assert_eq!(rumpf(&nodes).len(), 1, "ein Knoten im Rumpf: {nodes:?}");
        match &rumpf(&nodes)[0] {
            HtmlNode::Element { tag, children, .. } => {
                assert_eq!(tag, "p");
                assert_eq!(children.len(), 1);
                assert_eq!(children[0], HtmlNode::Text("Hallo".to_string()));
            }
            _ => panic!("erwartet Element"),
        }
    }

    #[test]
    fn verschachteltes_html() {
        let html = "<div><p>Eins</p><p>Zwei</p></div>";
        let nodes = parse_html(html);
        assert_eq!(rumpf(&nodes).len(), 1, "ein div im Rumpf: {nodes:?}");
        match &rumpf(&nodes)[0] {
            HtmlNode::Element { tag, children, .. } => {
                assert_eq!(tag, "div");
                assert_eq!(children.len(), 2);
            }
            _ => panic!("erwartet div"),
        }
    }

    #[test]
    fn attribute_werden_gelesen() {
        let html = r#"<a href="https://example.com" class="link">Klick</a>"#;
        let nodes = parse_html(html);
        match &rumpf(&nodes)[0] {
            HtmlNode::Element {
                tag,
                attributes,
                children,
                shadow_root: _,
            } => {
                assert_eq!(tag, "a");
                assert_eq!(
                    attributes.get("href").map(String::as_str),
                    Some("https://example.com")
                );
                assert_eq!(attributes.get("class").map(String::as_str), Some("link"));
                assert_eq!(children.len(), 1);
            }
            _ => panic!("erwartet a-Element"),
        }
    }

    #[test]
    fn void_elemente() {
        let html = "<br><hr><img src=\"bild.png\"><br/>";
        let nodes = parse_html(html);
        let rumpf_knoten = rumpf(&nodes);
        assert!(rumpf_knoten.len() >= 3, "{rumpf_knoten:?}");
        for node in rumpf_knoten {
            if let HtmlNode::Element { children, .. } = node {
                assert!(children.is_empty(), "Void-Element hat keine Kinder");
            }
        }
    }

    #[test]
    fn kommentare_werden_uebersprungen() {
        // SEIT DER UMSTELLUNG (03.10.2026) ist der Name genauer als
        // vorher: `HtmlNode` kann keinen Kommentar tragen, der Spec-Baum
        // schon (277 der Korpusfaelle haben einen). Der Kommentar wird
        // also nicht mehr "uebersprungen", sondern vom Adapter
        // weggezaehlt — siehe `parse_html_mit_verlusten`.
        let html = "<!-- das ist ein Kommentar --><p>Text</p>";
        let (nodes, verluste) = parse_html_mit_verlusten(html);
        assert_eq!(verluste.kommentar, 1, "der Kommentar wird gezaehlt");
        assert_eq!(rumpf(&nodes).len(), 1, "{nodes:?}");
        match &rumpf(&nodes)[0] {
            HtmlNode::Element { tag, .. } => assert_eq!(tag, "p"),
            _ => panic!("erwartet p"),
        }
    }

    #[test]
    fn doctype_wird_uebersprungen() {
        let html = "<!DOCTYPE html><html><body>Hallo</body></html>";
        let nodes = parse_html(html);
        assert!(!nodes.is_empty());
    }

    /// `<script>` UND `<style>`: Inhalt BEHALTEN, nicht ausgewertet.
    ///
    /// ENTSCHEIDUNG 30.09.2026 (Nutzer, ueber die Koordination): es gibt
    /// eine eigene JavaScript-Engine (`narsil-js`). Ein hier verworfener
    /// Inhalt ist ein fuer alle Mal verworfen — die beste Engine der Welt
    /// kann keinen Code lesen, den es nicht mehr gibt. Also bleibt der
    /// Quelltext als Kind des `script`-Knotens stehen.
    ///
    /// DIESER TEST HAT SICH GEAENDERT, und das ist die einzige Aenderung
    /// an einem Alt-Test in dieser Crate: vorher stand hier
    /// `assert!(children.is_empty())` mit dem Kommentar "Inhalt
    /// uebersprungen". Diese Behauptung ist jetzt FALSCH und waere der
    /// Grund, warum `narsil-js` nie laufen kann.
    ///
    /// `<style>` kam am selben Tag dazu, aus dem anderen Grund: H2
    /// (CSS) wartet nicht auf den Inhalt, sondern nur auf `narsil-css`.
    /// Wer ihn verwirft, muss ihn beim Verdrahten neu holen — aus einer
    /// Datei, die es dann nicht mehr gibt.
    ///
    /// Was UNVERAENDERT bleibt: nichts wird ausgewertet, nicht
    /// entitaetendekodiert und nicht zu Text der Seite. Das Zeichnen
    /// entscheidet `narsil-page` (dort: `Seite::stile`).
    #[test]
    fn script_und_style_inhalt_werden_gehalten() {
        let html = "<p>Vorher</p><script>alert('bos'); a &lt; b;</script>                    <style>h1 { color: #c33 }</style><p>Nachher</p>";
        let nodes = parse_html(html);
        // SEIT DER UMSTELLUNG (03.10.2026, A3): das Geruest ist da, und
        // `<script>`/`<style>` landen im RUMPF, nicht im `<head>` — weil
        // das erste Token `<p>` ist: "in head" bedeutet bei einem
        // "anything else"-Token "Kopf schliessen, Body anlegen" (13.2.6.4.9
        // / 13.2.6.4.14), und ab da gilt "in body". GEMESSEN, nicht
        // behauptet: der Baum traegt `p, script, Text, style, p` als
        // Geschwister im Rumpf. Steht am Anfang nur `<script>`, landet
        // es im Kopf — siehe `rohinhalt_wird_begrenzt`.
        let rumpf_knoten = rumpf(&nodes);
        assert_eq!(
            rumpf_knoten.len(),
            5,
            "fünf Knoten im Rumpf: {rumpf_knoten:?}"
        );

        match &rumpf_knoten[1] {
            HtmlNode::Element {
                tag,
                attributes,
                children,
                shadow_root: _,
            } => {
                assert_eq!(tag, "script");
                // GENAU ein Kind, und es ist der Quelltext.
                assert_eq!(
                    children.len(),
                    1,
                    "der script-Knoten hat {} Kinder — er hat keinen (Inhalt \
                     verworfen) oder zu viele (Inhalt doppelt)",
                    children.len()
                );
                match &children[0] {
                    HtmlNode::Text(t) => {
                        // Roh, nicht dekodiert: `&lt;` steht noch als
                        // `&lt;` drin. Eine Engine liest den Quelltext,
                        // keine Entitaetentabelle.
                        assert_eq!(t, "alert('bos'); a &lt; b;");
                    }
                    _ => panic!("erwartet Text im script"),
                }
                // Die Attribute bleiben erhalten: `src`, `type`, `async`,
                // `defer` sind das, was eine Engine zum Laden braucht.
                let _ = attributes;
            }
            _ => panic!("erwartet script"),
        }

        // `<style>`: derselbe Weg, mit dem CSS.
        match &rumpf_knoten[3] {
            HtmlNode::Element { tag, children, .. } => {
                assert_eq!(tag, "style");
                assert_eq!(children.len(), 1, "style hat {} Kinder", children.len());
                match &children[0] {
                    HtmlNode::Text(t) => assert_eq!(t, "h1 { color: #c33 }"),
                    _ => panic!("erwartet Text im style"),
                }
            }
            _ => panic!("erwartet style"),
        }

        // Und die Seite sieht weder Skript noch CSS als Text.
        //
        // SEIT DER UMSTELLUNG (03.10.2026, A3) steht zwischen `</script>`
        // und `<style>` ein echter Textknoten mit 20 Leerzeichen — der
        // Spec-Baum verwirft Whitespace im Rumpf nicht, der alte Weg
        // tat es (der Test behauptete oben "vier Knoten oben" und
        // meinte damit: kein Leerraumknoten). GEMESSEN: `extract_text`
        // fuegt zwischen Knoten ein Leerzeichen ein, kollabiert aber
        // nicht, also wird hier der kollabierte Text verglichen. Das
        // ist KEIN Verlust an Information: die Leerzeichen stehen im
        // Baum, nur nicht im zusammengefassten Text.
        assert_eq!(
            collapse_whitespace(&extract_text(&nodes)),
            "Vorher Nachher",
            "Skript und CSS sind kein Seitentext"
        );
    }

    /// Uebergrosse Skripte behalten eine Reserve zur Erkennung der Ueberlaenge.
    ///
    /// M4: geprueft wird VOR der Kopie. Ohne die Grenze waere ein
    /// `<script>` mit 50 MB Quelltext eine 50-MB-`String` in einem Knoten.
    #[test]
    fn rohinhalt_wird_begrenzt() {
        let lang = "x".repeat(MAX_SKRIPT_ROHINHALT + 100);
        let html = std::format!("<script>{lang}</script>");
        let nodes = parse_html(&html);
        // SEIT DER UMSTELLUNG (03.10.2026): hier steht am Anfang NUR ein
        // `<script>`, deshalb legt "before head" den Kopf an und "in
        // head" nimmt das Skript auf (13.2.6.4.5). Vorher stand es in
        // der Wurzel. Der Test sucht es deshalb, statt auf die Ebene zu
        // zeigen — seine Aussage ist die Grenze, nicht die Wurzeltiefe.
        let script = finde(&nodes, "script").unwrap_or_else(|| panic!("kein script: {nodes:?}"));
        match script {
            HtmlNode::Element { children, .. } => match &children[0] {
                HtmlNode::Text(t) => assert_eq!(
                    t.len(),
                    MAX_SKRIPT_ROHINHALT,
                    "erwartet {MAX_SKRIPT_ROHINHALT} Byte, sind {}",
                    t.len()
                ),
                _ => panic!("erwartet Text"),
            },
            _ => panic!("erwartet script"),
        }
    }

    #[test]
    fn entities_werden_dekodiert() {
        assert_eq!(decode_entities("&amp;"), "&");
        assert_eq!(decode_entities("&lt;p&gt;"), "<p>");
        assert_eq!(decode_entities("&quot;hi&quot;"), "\"hi\"");
        assert_eq!(decode_entities("&#65;"), "A");
        assert_eq!(decode_entities("&#x41;"), "A");
        assert_eq!(decode_entities("&nbsp;"), " ");
    }

    #[test]
    fn whitespace_kollabierung() {
        assert_eq!(collapse_whitespace("  Hallo   Welt  "), "Hallo Welt");
        assert_eq!(collapse_whitespace("a\n\n\nb"), "a b");
        assert_eq!(collapse_whitespace("\t\t"), "");
    }

    #[test]
    fn text_extraktion() {
        let html = "<html><head><title>T</title></head><body><p>Hallo</p><p>Welt</p></body></html>";
        let nodes = parse_html(html);
        let text = extract_text(&nodes);
        assert!(text.contains("Hallo"));
        assert!(text.contains("Welt"));
        assert!(!text.contains("T")); // title ist in head, also unsichtbar
    }

    #[test]
    fn kaputtes_html_bricht_nicht_ab() {
        // Fehlende schliessende Tags
        let nodes = parse_html("<p>Eins<p>Zwei");
        assert_eq!(rumpf(&nodes).len(), 2, "zwei <p>: {nodes:?}");

        // Leerer Input.
        //
        // SEIT DER UMSTELLUNG (03.10.2026, A3) ist das NICHT mehr leer,
        // und das ist die Regel, keine Nachsicht: die Baumkonstruktion
        // legt bei EOF erst den `html`-Knoten an, dann `head`, dann
        // `body` (13.2.6.4.3 "before html", .4 "before head", .6
        // "in head", .10 "after head" — jede dieser Modi leitet EOF
        // weiter). GEGEN DEN KORPUS geprueft, nicht gegen meine
        // Vorstellung: `tests1.dat` Zeile 870 hat ein leeres `#data`
        // und erwartet genau
        //     | <html>
        //     |   <head>
        //     |   <body>
        let nodes = parse_html("");
        let html = finde(&nodes, "html").unwrap_or_else(|| panic!("kein html: {nodes:?}"));
        assert_eq!(
            rumpf(&nodes),
            &[][..],
            "der Rumpf des leeren Dokuments ist leer: {nodes:?}"
        );
        assert_eq!(
            finde(&nodes, "head").is_some(),
            true,
            "auch der Kopf entsteht: {nodes:?}"
        );
        assert_eq!(
            html,
            &HtmlNode::Element {
                tag: "html".to_string(),
                attributes: HashMap::new(),
                children: vec![
                    HtmlNode::Element {
                        tag: "head".to_string(),
                        attributes: HashMap::new(),
                        children: Vec::new(),
                        shadow_root: None,
                    },
                    HtmlNode::Element {
                        tag: "body".to_string(),
                        attributes: HashMap::new(),
                        children: Vec::new(),
                        shadow_root: None,
                    },
                ],
                shadow_root: None,
            }
        );

        // Nur Whitespace: das Geruest, sonst nichts.
        let nodes = parse_html("   \n\t  ");
        assert_eq!(
            rumpf(&nodes),
            &[][..],
            "Whitespace fuellt nichts: {nodes:?}"
        );

        // Nur Tags ohne Inhalt.
        //
        // `<><>` ist KEIN Text, den man wegwerfen darf — es ist Text,
        // den man behalten muss. 13.2.5.6 "Tag open state", woertlich
        // (parsing.html, frisch geholt 03.10.2026):
        //   "Anything else This is an invalid-first-character-of-tag-name
        //    parse error. Emit a U+003C LESS-THAN SIGN character token.
        //    Reconsume in the data state."
        // Das erste `<` wird also als ZEICHEN ausgegeben, das `>` dann
        // im Data-State als Zeichen — zusammen der Text `<><>`.
        // GEMESSEN: der alte Weg (Legacy) gab hier einen leeren Baum.
        // Er lag falsch; der Test lag mit ihm.
        let nodes = parse_html("<><>");
        assert_eq!(
            rumpf(&nodes),
            [HtmlNode::Text("<><>".to_string())],
            "{nodes:?}"
        );
    }

    #[test]
    fn block_und_inline_erkennung() {
        let div = HtmlNode::Element {
            tag: "div".to_string(),
            attributes: HashMap::new(),
            children: Vec::new(),
            shadow_root: None,
        };
        assert!(div.is_block());

        let span = HtmlNode::Element {
            tag: "span".to_string(),
            attributes: HashMap::new(),
            children: Vec::new(),
            shadow_root: None,
        };
        assert!(!span.is_block());

        let br = HtmlNode::Element {
            tag: "br".to_string(),
            attributes: HashMap::new(),
            children: Vec::new(),
            shadow_root: None,
        };
        assert!(br.is_void());
    }

    #[test]
    fn unsichtbare_elemente() {
        let script = HtmlNode::Element {
            tag: "script".to_string(),
            attributes: HashMap::new(),
            children: Vec::new(),
            shadow_root: None,
        };
        assert!(script.is_invisible());

        let style = HtmlNode::Element {
            tag: "style".to_string(),
            attributes: HashMap::new(),
            children: Vec::new(),
            shadow_root: None,
        };
        assert!(style.is_invisible());

        let p = HtmlNode::Element {
            tag: "p".to_string(),
            attributes: HashMap::new(),
            children: Vec::new(),
            shadow_root: None,
        };
        assert!(!p.is_invisible());
    }

    /// DER KLEINSTE BEWEIS, dass der Parser endet.
    #[test]
    fn ein_schliessendes_tag_allein_beendet_den_parser() {
        for html in ["</a>", "<p>x</p></p>", "</p></div></span>", "<b></i>"] {
            // Ohne Timeout: haengt der Test, statt gruen zu werden.
            let n = parse_html(html);
            // Und die Regel: was nicht zu einem Knoten gehoert, wird
            // uebersprungen, nicht erfunden.
            assert!(
                n.iter()
                    .all(|k| !matches!(k, HtmlNode::Element { tag, .. } if tag.is_empty())),
                "{html:?}: kein leerer Tag-Namen erfunden"
            );
        }
    }

    /// Und das Gegenteil: der Parser verarbeitet weiter, er bricht nicht ab.
    #[test]
    fn nach_einem_ueberzaehligen_tag_kommt_der_echte_inhalt() {
        // Ein streunendes </p> mitten im Dokument, danach echter Text.
        // Vor dem Fix waere das eine Endlosschleife gewesen.
        let nodes = parse_html("</p>Danach steht Text.");
        let text = extract_text(&nodes);
        assert!(
            text.contains("Danach steht Text"),
            "der Inhalt nach dem streunenden Tag muss da sein, war \"{text}\""
        );
    }

    #[test]
    fn echtes_html_dokument() {
        let html = r#"<!DOCTYPE html>
<html lang="de">
<head>
    <meta charset="utf-8">
    <title>Test</title>
    <style>body { color: red; }</style>
</head>
<body>
    <h1>Ueberschrift</h1>
    <p>Ein <strong>wichtiger</strong> Absatz.</p>
    <ul>
        <li>Punkt 1</li>
        <li>Punkt 2</li>
    </ul>
    <script>console.log("weg damit");</script>
</body>
</html>"#;
        let nodes = parse_html(html);
        assert!(!nodes.is_empty());
        let text = extract_text(&nodes);
        assert!(text.contains("Ueberschrift"));
        assert!(text.contains("wichtiger"));
        assert!(text.contains("Punkt 1"));
        assert!(!text.contains("console.log")); // Script uebersprungen
        assert!(!text.contains("color: red")); // Style uebersprungen
    }
}
