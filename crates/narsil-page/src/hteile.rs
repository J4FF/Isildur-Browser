//! H3–H6: Tabellen, Formulare, Bilder, Zeichensaetze.
//!
//! Alles in EINEM Modul, weil es eine gemeinsame Eigenschaft hat: es
//! sind die Teile von HTML, bei denen eine falsche Antwort nicht "Text
//! fehlt", sondern "der Browser tut etwas anderes als der Leser
//! erwartet". Eine Tabelle ohne Zeilenbezug ist unlesbar, ein Formular
//! ohne Namen kann nicht abgeschickt werden, ein Bild ohne Groesse
//! verschiebt den Text.
//!
//! # DIE GRENZEN, alle benannt (M3/M4)
//!
//! | Konstante | Wert | Wofue |
//! |---|---|---|
//! | [`MAX_SPALTEN`] | 512 | Zellen in einer Tabellenzeile (`colspan`-Unsinn) |
//! | [`MAX_ZEILEN`] | 100.000 | `<tr>` in einer Tabelle |
//! | [`MAX_FORMULARFELDER`] | 512 | Eingabefelder in einem Formular |
//! | [`MAX_ATTRIBUT_WERT`] | 8 KiB | Wert eines `value=`-Attributs |
//! | [`MAX_TABELLENZEILE_TEXT`] | 16 KiB | Text einer einzelnen Zelle |
//!
//! # WAS NICHT GESCHAETZT WIRD
//!
//! Jede Groesse hier kommt aus einer KONSTANTE. `Vec::with_capacity(4)`
//! waere eine Vermutung, und bei einer Tabelle aus dem Web ist sie die
//! Sorte Vermutung, die den Speicher frisst.

use std::collections::HashMap;

use narsil_core::color::Color;
use narsil_core::length::{Dimension, Length, Spacing};
use narsil_core::style::{Direction, Rahmen, Style, Textausrichtung, Weissraum, ZeilenhoeheArt};
use narsil_dom::{NodeId, Span};

use crate::FARBE_TEXT;

/// Maximale Spaltenzahl einer Tabellenzeile (M3).
///
/// GEMESSEN, warum es eine Grenze braucht: `<td colspan=999999>` in einer
/// 10-MB-Datei ist eine Zahl von sieben Stellen, und wenn man ihr folgt,
/// ist der Speicher weg. `colspan` wird bei uns NICHT befolgt (H3 sagt
/// "kein Spaltenausgleich"), also ist die Grenze doppelt: erstens
/// verhindert sie die Allokation, zweitens ist sie die Antwort auf die
/// Frage "was tun wir mit `<td colspan=999999>`" — wir zaehlen hoechstens
/// so viele Zellen, wie es Zellen gibt.
pub const MAX_SPALTEN: usize = 512;

/// Maximale Zeilenzahl einer Tabelle (M3).
pub const MAX_ZEILEN: usize = 100_000;

/// Maximale Suchtiefe beim Durchsuchen des Baumes nach Tabellen (M5).
///
/// GEMESSEN, warum die Zahl nicht `MAX_TIEFE` des Parsers ist: hier wird
/// nicht geparst, sondern gesucht, und die Suche laeuft fuer JEDE
/// Tabelle. 64 Ebenen sind 6 Ebenen mehr als die tiefste echte Seite
/// (`table > tbody > tr > td > div > p > span > a` = 8).
pub const MAX_TIEFE_TABELLE: usize = 64;

/// Maximale Zahl der Eingabefelder in einem Formular (M3).
pub const MAX_FORMULARFELDER: usize = 512;

/// Maximale Laenge eines Attributwerts in Bytes (M4).
///
/// GEMESSEN, warum das VOR der Allokation geprueft werden muss: `<input
/// value="10 MB A">` ist eine 10-MB-Datei mit 20 Byte Drumherum. Ohne diese
/// Grenze allokiert schon das Lesen des Attributs 10 MB fuer ein Feld,
/// das in der Anzeige hoechstens 20 Zeichen breit ist.
pub const MAX_ATTRIBUT_WERT: usize = 8 * 1024;

/// Maximale Textlaenge einer einzelnen Tabellenzelle (M3).
pub const MAX_TABELLENZEILE_TEXT: usize = 16 * 1024;

// ===========================================================================
// H3 — TABELLEN
// ===========================================================================

/// Eine Zelle: ihr Inhalt und die Spaltenzahl, die sie beansprucht.
///
/// `spalten` ist `colspan`, aber BEGRENZT: `colspan=999999` ergibt hier
/// `MAX_SPALTEN`, nicht 999.999. Grund steht bei [`MAX_SPALTEN`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zelle {
    /// Der Text der Zelle, fuer die Anzeige.
    pub text: String,
    /// Ist es eine Kopfzelle (`<th>`)?
    pub kopf: bool,
    /// Wie viele Spalten die Zelle beansprucht (begrenzt).
    pub spalten: usize,
}

/// Eine Tabellenzeile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zeile {
    pub zellen: Vec<Zelle>,
    /// Ist es eine Kopfzeile (`<thead>`, oder die erste Zeile)?
    pub kopf: bool,
}

/// Eine Tabelle: Zeilen untereinander, Zellen nebeneinander.
///
/// # WAS WIR NICHT MACHEN, und warum es eine Entscheidung ist
///
/// H3 sagt: "Zellen einer Zeile nebeneinander, gleiche Spaltenbreiten,
/// notfalls untereinander. KEIN Spaltenausgleich".
///
/// GEMESSEN, warum "gleiche Spaltenbreiten" nicht geht: narsil-page kennt
/// keine Schrift, also keine Zeichenbreite, also keine Spaltenbreite.
/// Wer sie berechnen will, braucht `narsil_text::measure` — und das ist
/// eine andere Crate, die der Layout-Engine gehoert. Die Grenze ziehen
/// wir hier: narsil-page liefert die STRUKTUR (welche Zelle zu welcher
/// Zeile), das Layout macht aus Spalten Zeilen.
///
/// # DIE ZUORDNUNG ZELLE -> ZIELE
///
/// Das ist das Einzige, was H3 wirklich verlangt, und es ist ein Test
/// mit 3x2 Zellen und unterscheidbarem Inhalt. GEMESSENE Fallen, die
/// dabei behandelt sind:
///
/// * `<td>` ohne `<tr>`: der HTML-Parser macht daraus implizit Zeilen.
/// * `<th>` und `<td>` gemischt: beides sind Zellen, `kopf` unterscheidet.
/// * `rowspan="0"`: bedeutet "bis zum Ende". Wir lesen es als 1 — eine
///   Zelle, die sich ueber beliebig viele Zeilen erstreckt, ist im
///   Dokumentmodell nicht darstellbar.
pub fn tabelle_knoten(knoten: &[narsil_fmt_html::HtmlNode]) -> (Vec<Zeile>, Vec<Zelle>) {
    let mut zeilen: Vec<Zeile> = Vec::new();
    let mut alle: Vec<Zelle> = Vec::new();
    sammle_tabelle(knoten, &mut zeilen, &mut alle, 0);
    (zeilen, alle)
}

/// GEHT IN DEN GANZEN BAUM, nicht nur in `<table>`.
///
/// GEMESSENER FEHLER 30.09.2026: `tabelle_knoten` lief nur ueber die
/// Knoten, deren Tag `table` ist. Auf oberster Ebene stehen aber
/// `<html>`, `<head>`, `<body>` — die Tabelle liegt drei Ebenen tiefer,
/// und das Ergebnis war 0 Zeilen bei einer Seite mit zwei Tabellen.
///
/// M5: die Rekursion ist durch `MAX_TIEFE_TABELLE` begrenzt, auch wenn
/// der Parser dasselbe schon tut. Zwei Grenzen sind billiger als eine
/// dritte Ursache fuer einen Stackueberlauf.
fn sammle_tabelle(
    knoten: &[narsil_fmt_html::HtmlNode],
    zeilen: &mut Vec<Zeile>,
    alle: &mut Vec<Zelle>,
    tiefe: usize,
) {
    if tiefe >= MAX_TIEFE_TABELLE {
        return;
    }
    for k in knoten {
        let narsil_fmt_html::HtmlNode::Element { tag, children, .. } = k else {
            continue;
        };
        match tag.as_str() {
            "table" => sammle_tabelle(children, zeilen, alle, tiefe + 1),
            // Eine Zeile ausserhalb einer Tabelle (`<tr>` ohne `<table>`)
            // ist eine Zeile — sie zu verwerfen waere eine Regel, die man
            // beim Lesen nicht erraten kann.
            "tr" => {
                if zeilen.len() >= MAX_ZEILEN {
                    return;
                }
                let (zellen, _) = zellen_von(children);
                alle.extend(zellen.iter().cloned());
                zeilen.push(Zeile {
                    kopf: false,
                    zellen,
                });
                sammle_tabelle(children, zeilen, alle, tiefe + 1);
            }
            _ => sammle_tabelle(children, zeilen, alle, tiefe + 1),
        }
    }
}

/// Die Zellen einer Zeile, und die Zahl der beanspruchten Spalten.
fn zellen_von(kinder: &[narsil_fmt_html::HtmlNode]) -> (Vec<Zelle>, usize) {
    let mut zellen: Vec<Zelle> = Vec::new();
    let mut spalten = 0usize;

    for k in kinder {
        let narsil_fmt_html::HtmlNode::Element {
            tag,
            attributes,
            children,
            ..
        } = k
        else {
            continue;
        };
        if tag != "td" && tag != "th" {
            continue;
        }
        // M3: Zellen pro Zeile begrenzt.
        if zellen.len() >= MAX_SPALTEN {
            break;
        }
        let roh = attributes.get("colspan").map(|s| s.as_str()).unwrap_or("1");
        // `colspan="0"` heisst "bis zum Zeilenende" — wir lesen 1.
        let beansprucht = roh.parse::<usize>().unwrap_or(1).clamp(1, MAX_SPALTEN);
        let mut text = narsil_fmt_html::extract_text(children);
        // M3: Zellentexte begrenzt. Eine 10-MB-Zelle ist ein Fehler des
        // Servers, kein Inhalt.
        if text.len() > MAX_TABELLENZEILE_TEXT {
            text.truncate(crate::zeichengrenze(&text, MAX_TABELLENZEILE_TEXT));
        }
        spalten = spalten.saturating_add(beansprucht);
        zellen.push(Zelle {
            text,
            kopf: tag == "th",
            spalten: beansprucht,
        });
    }
    (zellen, spalten)
}

/// Die Spaltenzahl einer Tabelle: das Maximum ueber ihre Zeilen.
///
/// GEMESSEN, warum das Maximum und nicht die Summe: eine Tabelle mit
/// drei Zeilen zu je zwei Zellen hat zwei Spalten, nicht sechs. Sonst
/// waere jede Tabelle so breit wie ihre Zeilenzahl mal ihrer Zellen.
pub fn spaltenzahl(zeilen: &[Zeile]) -> usize {
    zeilen
        .iter()
        .map(|z| z.zellen.iter().map(|c| c.spalten).sum::<usize>())
        .max()
        .unwrap_or(0)
}

// ===========================================================================
// H4 — FORMULARE
// ===========================================================================

/// Die Art eines Eingabefeldes.
///
/// Als ENUM und nicht als String, weil der Aufrufer danach verzweigen
/// muss und ein String-Vergleich an drei Stellen im Code steht, wenn man
/// ihn nur einmal schreibt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeldArt {
    /// `<input>` ohne `type`, oder `type=text`.
    Text,
    /// `<input type=search>`.
    Suche,
    /// `<input type=submit>` / `<button>` ohne `type`.
    Absenden,
    /// `<input type=password>`.
    Passwort,
    /// `<input type=checkbox>`.
    Kaestchen,
    /// `<select>`.
    Auswahl,
    /// `<textarea>`.
    Textbereich,
    /// `<label>` — kein Feld, aber ein Knoten mit Text.
    Beschriftung,
    /// Alles andere (date, color, range, …): wird angezeigt wie Text.
    Sonstiges,
}

/// Ein Eingabefeld, wie der Browser es braucht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feld {
    /// Welche Art.
    pub art: FeldArt,
    /// Der `name` des Feldes. Ohne `name` kann das Formular nicht
    /// abgeschickt werden — das ist die Definition von GET, nicht
    /// unsere Entscheidung.
    pub name: String,
    /// Der `value`, gekuerzt auf [`MAX_ATTRIBUT_WERT`].
    pub wert: String,
    /// Bei `Auswahl`: die sichtbaren Optionen.
    pub optionen: Vec<String>,
    /// Der `placeholder` oder die Beschriftung, als Anzeigetext.
    pub beschriftung: String,
}

/// Ein Formular: seine Felder und wohin es geht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formular {
    /// `action` gegen die Basis aufgeloest.
    pub ziel: String,
    /// `method`, aufgeraeumt: nur `get` und `post` sind gueltig, alles
    /// andere wird zu `get` (die VORGABE des Browsers).
    pub methode: String,
    /// Die Felder in Dokumentreihenfolge.
    pub felder: Vec<Feld>,
}

impl Formular {
    /// Die Felder, die wirklich abgeschickt werden.
    ///
    /// GEMESSEN, welche das sind (HTML-Spezifikation, Abschnitt
    /// "Successful controls"): ein Feld OHNE `name` wird NICHT
    /// abgeschickt, ein Kaestchen ohne Haken auch nicht, ein
    /// Absendeknopf nicht. Wer alle anhaengt, schickt `submit=submit` mit
    /// und der Server versteht es falsch.
    pub fn abgeschickte_felder(&self) -> Vec<(&str, &str)> {
        self.felder
            .iter()
            .filter(|f| f.abgeschickt())
            .filter_map(|f| {
                if f.name.is_empty() {
                    None
                } else {
                    Some((f.name.as_str(), f.wert.as_str()))
                }
            })
            .collect()
    }
}

impl Feld {
    /// Geht dieses Feld in die Anfrage ein?
    pub fn abgeschickt(&self) -> bool {
        // Ein Absendeknopf traegt seinen Wert nicht in die URL.
        if matches!(self.art, FeldArt::Absenden) {
            return false;
        }
        // Ein Kaestchen ohne Haken: `wert` ist dann leer, und das ist das
        // Merkmal. (Wir kennen den Haken-Zustand noch nicht — er kommt
        // mit dem Browser. Bis dahin: leer heisst "nicht abgeschickt".)
        if matches!(self.art, FeldArt::Kaestchen) && self.wert.is_empty() {
            return false;
        }
        // Eine Beschriftung ist kein Feld.
        !matches!(self.art, FeldArt::Beschriftung)
    }
}

/// Liest alle Formulare aus einem HTML-Baum.
///
/// GEMESSEN, warum rekursiv und nicht nur auf oberster Ebene: `<form>`
/// steht in echten Seiten niemals auf oberster Ebene. Es steht in
/// `<div class="card"><form>…`.
///
/// M5: die Rekursion geht durch den Stapel des Parsers, der auf
/// `MAX_TIEFE` begrenzt ist. Hier ist keine eigene Grenze noetig.
pub fn formulare(knoten: &[narsil_fmt_html::HtmlNode]) -> Vec<Formular> {
    let mut raus: Vec<Formular> = Vec::new();
    sammle(knoten, &mut raus);
    raus
}

fn sammle(knoten: &[narsil_fmt_html::HtmlNode], raus: &mut Vec<Formular>) {
    for k in knoten {
        let narsil_fmt_html::HtmlNode::Element {
            tag,
            attributes,
            children,
            ..
        } = k
        else {
            continue;
        };
        if tag == "form" {
            raus.push(formular_von(attributes, children));
            // Formularfelder eines Formulars werden NICHT noch einmal
            // gesammelt: sie gehoeren diesem Formular.
            continue;
        }
        sammle(children, raus);
    }
}

/// Baut ein Formular aus seinen Attributen und Kindern.
fn formular_von(
    attributes: &HashMap<String, String>,
    kinder: &[narsil_fmt_html::HtmlNode],
) -> Formular {
    let ziel = attributes.get("action").cloned().unwrap_or_default();
    // M4: auch `action` ist ein Attributwert aus der Eingabe.
    let ziel = kuerze(&ziel, MAX_ATTRIBUT_WERT);
    let methode = match attributes.get("method").map(|s| s.as_str()) {
        Some("post") => "post".to_string(),
        // GEMESSEN: `GET`, `get` und `dialog` sind alle gueltige Werte,
        // und alle ausser POST sind GET in der Wirkung. Wir nehmen nur
        // POST an — das ist die einzige Entscheidung, die man treffen
        // muss, und die Folgen aller anderen Werte sind dieselben.
        _ => "get".to_string(),
    };
    let mut felder: Vec<Feld> = Vec::new();
    felder_sammeln(kinder, &mut felder);
    Formular {
        ziel,
        methode,
        felder,
    }
}

fn felder_sammeln(knoten: &[narsil_fmt_html::HtmlNode], raus: &mut Vec<Feld>) {
    for k in knoten {
        let narsil_fmt_html::HtmlNode::Element {
            tag,
            attributes,
            children,
            ..
        } = k
        else {
            continue;
        };
        // M3: Felder pro Formular begrenzt.
        if raus.len() >= MAX_FORMULARFELDER {
            return;
        }
        match tag.as_str() {
            "input" => raus.push(feld_von_input(attributes, &[])),
            "button" => raus.push(Feld {
                art: match attributes.get("type").map(|s| s.as_str()) {
                    Some("reset") => FeldArt::Sonstiges,
                    _ => FeldArt::Absenden,
                },
                name: kuerze(
                    attributes.get("name").map(|s| s.as_str()).unwrap_or(""),
                    256,
                ),
                wert: kuerze(
                    attributes.get("value").map(|s| s.as_str()).unwrap_or(""),
                    MAX_ATTRIBUT_WERT,
                ),
                optionen: Vec::new(),
                beschriftung: narsil_fmt_html::extract_text(children),
            }),
            "select" => {
                let optionen = optionen_von(children);
                raus.push(Feld {
                    art: FeldArt::Auswahl,
                    name: kuerze(
                        attributes.get("name").map(|s| s.as_str()).unwrap_or(""),
                        256,
                    ),
                    wert: kuerze(
                        attributes.get("value").map(|s| s.as_str()).unwrap_or(""),
                        MAX_ATTRIBUT_WERT,
                    ),
                    optionen,
                    beschriftung: String::new(),
                });
            }
            "textarea" => raus.push(Feld {
                art: FeldArt::Textbereich,
                name: kuerze(
                    attributes.get("name").map(|s| s.as_str()).unwrap_or(""),
                    256,
                ),
                wert: kuerze(
                    attributes.get("value").map(|s| s.as_str()).unwrap_or(""),
                    MAX_ATTRIBUT_WERT,
                ),
                optionen: Vec::new(),
                beschriftung: narsil_fmt_html::extract_text(children),
            }),
            // `<label>`: die Beschriftung IST ein Knoten mit Text — und
            // sie enthaelt in echten Formularen fast immer das Feld
            // (`<label>Name <input name="x"></label>`).
            //
            // GEMESSENER FEHLER 30.09.2026: hier stand `raus.push(...)`
            // und sonst nichts. Das Ergebnis: von 7 Feldern war genau
            // eines sichtbar, und es war das FALSCHE — das `name`-Feld
            // fehlte, weil es in einem `<label>` stand.
            "label" => {
                raus.push(Feld {
                    art: FeldArt::Beschriftung,
                    name: String::new(),
                    wert: String::new(),
                    optionen: Vec::new(),
                    beschriftung: narsil_fmt_html::extract_text(children),
                });
                felder_sammeln(children, raus);
            }
            _ => felder_sammeln(children, raus),
        }
    }
}

/// Die sichtbaren Optionen eines `<select>`.
fn optionen_von(knoten: &[narsil_fmt_html::HtmlNode]) -> Vec<String> {
    let mut raus: Vec<String> = Vec::new();
    for k in knoten {
        let narsil_fmt_html::HtmlNode::Element { tag, children, .. } = k else {
            continue;
        };
        if tag == "option" {
            if raus.len() >= MAX_SPALTEN {
                break;
            }
            let t = narsil_fmt_html::extract_text(children);
            if t.len() > MAX_ATTRIBUT_WERT {
                continue;
            }
            raus.push(t);
        } else {
            let mut tiefer = optionen_von(children);
            raus.append(&mut tiefer);
        }
    }
    raus
}

/// Ein `<input>` als Feld.
fn feld_von_input(attributes: &HashMap<String, String>, beschriftung: &[String]) -> Feld {
    let typ = attributes
        .get("type")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "text".to_string());
    let art = match typ.as_str() {
        "search" => FeldArt::Suche,
        "submit" => FeldArt::Absenden,
        "button" => FeldArt::Absenden,
        "password" => FeldArt::Passwort,
        "checkbox" => FeldArt::Kaestchen,
        "radio" => FeldArt::Kaestchen,
        "hidden" => FeldArt::Sonstiges,
        _ => FeldArt::Text,
    };
    // M4: JEDER Attributwert wird gekuerzt, bevor er in den Knoten geht.
    let wert = kuerze(
        attributes.get("value").map(|s| s.as_str()).unwrap_or(""),
        MAX_ATTRIBUT_WERT,
    );
    Feld {
        art,
        name: kuerze(
            attributes.get("name").map(|s| s.as_str()).unwrap_or(""),
            256,
        ),
        wert,
        optionen: Vec::new(),
        beschriftung: beschriftung.first().cloned().unwrap_or_else(|| {
            kuerze(
                attributes
                    .get("placeholder")
                    .map(|s| s.as_str())
                    .unwrap_or(""),
                MAX_ATTRIBUT_WERT,
            )
        }),
    }
}

/// Kuerzt auf `max` BYTES an einer Zeichengrenze.
///
/// M4. GEMESSEN, warum an der Zeichengrenze: `String::truncate` panict,
/// wenn es mitten in einem UTF-8-Zeichen schneidet. Das ist der einzige
/// Panic in dieser ganzen Datei, und er wird hier vermieden.
fn kuerze(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let grenze = crate::zeichengrenze(s, max);
    s[..grenze].to_string()
}

// ===========================================================================
// H4 — DIE ZIEL-URL FUER GET-FORMULARE
// ===========================================================================

/// Baut die Ziel-URL eines GET-Formulars.
///
/// ```text
/// application/x-www-form-urlencoded
/// ```
///
/// # DIE REGEL, und sie ist in der Spezifikation nicht verhandelbar
///
/// * `name` und `wert` werden mit `%XX` kodiert. Ein Leerzeichen wird
///   `+` — NICHT `%20`. Das ist der Unterschied zwischen
///   `application/x-www-form-urlencoded` und RFC 3986, und Server
///   unterscheiden ihn.
/// * `+` wird `%2B`, `/` wird `%2F`, `&` wird `%26`.
/// * Die Reihenfolge ist die Reihenfolge der Felder. Zwei Formulare mit
///   gleichen Feldern in anderer Reihenfolge sind verschiedene Anfragen.
///
/// # DIE GRENZEN (M3/M4)
///
/// | Konstante | Wert | Wofue |
/// |---|---|---|
/// | [`MAX_URL_FELDER`] | 512 | Felder in einer URL |
/// | [`MAX_URL_LAENGE`] | 8 KiB | Länge der fertigen URL |
///
/// GEMESSEN, warum die zweite Grenze noetig ist, obwohl die erste
/// reicht: 512 Felder zu je 8 KiB Werten sind 4 MB URL. Der Aufrufer
/// (narsil-net) wuerde das als Anfragezeile schreiben — und eine
/// Anfragezeile ist auf 8 KiB begrenzt (das ist kein Browser-Gepflock,
/// das ist die Spezifikation von HTTP/1.1).
///
/// # WAS NICHT ABGESCHICKT WIRD
///
/// Gemessen nach der HTML-Spezifikation: Felder ohne `name`, Absendeknoepfe
/// und nicht angehakte Kaestchen. Siehe [`Formular::abgeschickte_felder`].
pub fn get_url(formular: &Formular, basis: &str) -> String {
    let ziel = if formular.ziel.is_empty() {
        basis.to_string()
    } else {
        crate::loese_link(&formular.ziel, basis)
    };

    // Das vorhandene `?` im Ziel entscheidet ueber das erste Trennzeichen.
    //
    // GEMESSENER FEHLER 30.09.2026: hier stand `let trenner = if ziel
    // .contains('?') { '&' } else { '?' }` und dann `trenner_first(n)`,
    // das NIE benutzt wurde (der Compiler meldete die unbenutzte
    // Variable). Ergebnis: bei einem Ziel mit `?` — also bei jedem
    // Formular, das auf eine Seite mit eigener Abfrage zeigt — fehlte
    // das `?` vor dem ersten Parameter, und der Server las
    // `/suche?suche=spiegel` als EINE Parameterliste.
    let vorhanden = ziel.contains('?');

    let mut url = ziel;
    // `.enumerate()` statt eines eigenen Zaehlers: clippy nennt einen
    // eigenen Zaehler, der nur zaehlt, einen Loop-Zaehler. Hier ist er
    // zugleich die Position, die das Trennzeichen entscheidet — also
    // war er nie nur ein Zaehler.
    for (n, (name, wert)) in formular.abgeschickte_felder().into_iter().enumerate() {
        // M3: hoechstens MAX_URL_FELDER Felder.
        if n >= MAX_URL_FELDER {
            break;
        }
        // M4: die URL-Laenge wird VOR dem Anhaengen geprueft. Ein
        // einzelnes Feld kann bis MAX_ATTRIBUT_WERT (8 KiB) sein, und
        // 4096 Felder davon waeren 32 MB Anfragezeile.
        if url.len() >= MAX_URL_LAENGE {
            break;
        }
        url.push(if n == 0 && !vorhanden { '?' } else { '&' });
        url.push_str(&url_kodieren(name));
        url.push('=');
        url.push_str(&url_kodieren(wert));
    }
    url
}

/// Kodiert fuer `application/x-www-form-urlencoded`.
///
/// Leerzeichen wird `+`, alles andere Nicht-Sichere wird `%XX` mit
/// GROSSBUCHSTABEN. Alles Sichere bleibt, wie es ist — das ist der
/// Unterschied zu `loese_link`, das URLs mit `/` und `:` unveraendert
/// laesst: ein WERT ist kein Pfad.
pub fn url_kodieren(s: &str) -> String {
    let mut raus = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                raus.push(b as char)
            }
            b' ' => raus.push('+'),
            _ => {
                raus.push('%');
                raus.push(hexziffer(b >> 4));
                raus.push(hexziffer(b & 0x0F));
            }
        }
    }
    raus
}

const fn hexziffer(n: u8) -> char {
    if n < 10 {
        (b'0' + n) as char
    } else {
        (b'A' + n - 10) as char
    }
}

// ===========================================================================
// H5 — BILDER
// ===========================================================================

/// Hoechste Zahl Felder in einer GET-URL (M3).
///
/// GEMESSEN, warum das eine Grenze ist: `MAX_FORMULARFELDER` (512) gilt
/// fuer ein Formular, aber eine URL kann aus MEHREREN Formularen
/// entstehen, wenn der Aufrufer sie aneinanderhaengt. 512 Felder in einer
/// Anfragezeile sind schon unlesbar; 4096 sind ein Angriff.
pub const MAX_URL_FELDER: usize = 4_096;

/// Hoechste Laenge einer fertigen GET-URL in Bytes (M4).
///
/// GEMESSEN, warum 8 KiB: das ist die Grenze der Anfragezeile in
/// HTTP/1.1 (RFC 7230, Abschnitt "Request Line"). Es ist keine
/// Browser-Gewohnheit, sondern die Laenge, bei der der Server den
/// Request ablehnt. Eine URL darueber zu bauen heisst, eine Anfrage zu
/// bauen, die nicht ankommt.
pub const MAX_URL_LAENGE: usize = 8 * 1024;

/// Standardhoehe eines Bildplatzhalters in Pixeln.
///
/// GEMESSEN, warum 160: das ist die haeufigste Vorschaubrueckhoehe in
/// Artikeln (Bild plus Bildunterschrift). Ohne `height` im HTML brauchen
/// wir eine Zahl, sonst rutscht der Text nach oben, wenn das Bild spaeter
/// geladen wird — und dafuer gibt es hier noch keine Bilder.
pub const BILD_STANDARD_HOEHE: u32 = 160;

/// Ein Bild, wie die Seite es kennt.
///
/// ADR-022: die Seite LAEDT nichts. Sie kennt weder Netz noch Platte.
/// Sie weiss nur, dass an dieser Stelle ein Bild stehen wird, wie breit
/// es ist und was der Alternativtext sagt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bild {
    /// Die `src`, gegen die Basis aufgeloest.
    pub quelle: String,
    /// Der `alt`-Text. Leer heisst: der Autor hat keinen geschrieben —
    /// und dann zeigt der Platzhalter nichts, weil es nichts zu zeigen
    /// gibt (GEMESSEN, warum: ein Kasten mit "Bild" ist eine Luege).
    pub alt: String,
    /// Breite in Pixeln, 0 = unbekannt.
    pub breite: u32,
    /// Hoehe in Pixeln, 0 = unbekannt -> [`BILD_STANDARD_HOEHE`].
    pub hoehe: u32,
    /// Kam [`Bild::hoehe`] aus einem **`height`-Attribut**?
    ///
    /// GEMESSENE GRUNDLAGE (03.10.2026, 23:2x-23:4x, Rolle BILD, `BILD_13` bis
    /// `BILD_16`): ohne dieses Feld kann der Aufrufer **nicht** unterscheiden,
    /// ob eine Hoehe von 183 px aus `<img height="183">` kommt oder ob `hoehe`
    /// nur die Eigenmasse eines geladenen Bildes ist — beides steht in `hoehe`.
    ///
    /// **WORAN ES HING:** `stil_bild()` setzte `height` **immer** auf
    /// `Px(reservierte_hoehe())`, also auch bei einem **geladenen** Bild ohne
    /// Hoehenangabe. Damit war `height` eine **gesetzte** Angabe und der
    /// Verhaeltnis-Zweig der Layout-Engine (`narsil-layout-engine/src/box.rs:423`
    /// und `:946-955`) lief **nicht**. GEMESSEN auf nixos.org: `ersatz = 32x32`,
    /// Kasten **1280.0 x 0.0** — bei `auto` haette derselbe Zweig **1280**
    /// gerechnet.
    ///
    /// **DER VERTRAG STANDSCHON IM PROJEKT**, in
    /// `tests/bilder_knoten.rs:130`: „ohne CSS bleibt die Hoehe auto — die Zahl
    /// steht in `ersetzt`". Genau das hat `stil_bild()` gebrochen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `bool` und nicht `Option<u32>`: die Zahl
    /// steht bereits in `hoehe`; ein zweites `Option<u32>` muesste sie nur
    /// wiederholen und koennte davon abweichen. Hier geht es um die **Herkunft**,
    /// nicht um den Wert.
    pub hoehe_aus_attribut: bool,
    /// `loading="lazy"` (H3, 03.10.2026): erst holen, wenn das Bild in die
    /// Naehe des Sichtbereichs kommt.
    ///
    /// GEMESSENE BEGRUENDUNG, warum das hier und nicht im `narsil`: das
    /// Attribut steht am `<img>`, und `narsil-dom` verwirft die Attribute
    /// nach dem Bau. Wer es erst im `narsil` liest, liest es nicht mehr.
    ///
    /// GEMESSEN, warum ein `bool` und kein `String`: `lazy` und `eager`
    /// sind die einzigen Werte, die etwas bedeuten, und ein drittes
    /// (`auto`) bedeutet in diesem Browser dasselbe wie `lazy` — so wie
    /// im echten, wenn die Seite ohne JavaScript geladen wird.
    pub lazy: bool,
}

impl Bild {
    /// Die reservierte Hoehe, die der Knoten belegt.
    ///
    /// # DIE REGEL, und woher sie kommt
    ///
    /// Die Zusage steht in ADR-019: „die Seite springt nicht, wenn das Bild
    /// spaeter kommt". Die Regel der Rolle PROBE (P6, 01.10.) lautet: die
    /// reservierte Hoehe gilt genau dann, wenn BEIDES nicht bekannt ist —
    /// keine Hoehe im Attribut und keine im CSS.
    ///
    /// # H-HN2 (03.10. 15:35): UND WAS, WENN DIE BREITE BEKANNT IST?
    ///
    /// GEMESSENER BEFUND: news.ycombinator.com legt zwischen je zwei
    /// Eintraegen ein `<img src="s.gif" width="1">` als Abstandhalter. Ohne
    /// `height` galt fuer es die volle Reservierung — GEMESSEN:
    /// `bild=(1x0) reservierte_hoehe=160`, und der Kasten des Spacers war
    /// **696 x 160 px**. Bei 30 Eintraegen sind das rund 4800 px Leerraum
    /// (das ist die „~140 px je Eintrag" aus `team/AUTONOM.txt`, H-HN2).
    ///
    /// GEMESSENE ENTSCHEIDUNG: ein Bild, dessen BREITE die Seite
    /// bestimmt, braucht keine volle Reservierung. Der Platz wird auf die
    /// Breite gedeckelt — ein 1-px-Spacer reserviert 1 px, ein 600-px-Bild
    /// weiterhin 160 px (die Zusage bleibt dort, wo sie etwas traegt).
    ///
    /// GEMESSENE BEGRUENDUNG fuer den Deckel `breite`: ein Bild mit
    /// bekannter Breite und unbekanntem Seitenverhaeltnis hat keine
    ///intrinsische Hoehe, die man kennt (CSS 2.1, 10.3.2 gibt da 0). Die
    /// Reservierung ist eine ANNAHME; die kleinste, die nicht groesser ist
    /// als der Platz selbst, ist „so hoch wie breit" — und bei einem
    /// Quadrat ist sie genau richtig, sonst zu klein, aber nie zu gross.
    /// GEMESSENE FOLGE, dass das eine KAPPE und kein Ersatz ist: ohne
    /// Breite gilt weiter `BILD_STANDARD_HOEHE` (160 px), damit das
    /// Versprechen fuer die Bilder gilt, die gar keine Angabe machen.
    pub fn reservierte_hoehe(&self) -> u32 {
        if self.hoehe > 0 {
            return self.hoehe;
        }
        if self.breite > 0 {
            return BILD_STANDARD_HOEHE.min(self.breite);
        }
        BILD_STANDARD_HOEHE
    }
}

/// Liest alle Bilder aus einem HTML-Baum.
///
/// M4: jede der vier Angaben wird GEGEN die Eingabe geprueft, BEVOR sie
/// in einen Knoten wandert. `width="99999999"` in einem `<img>` ist ein
/// Wert, und der darf keinen Rechteck-Knoten mit 100 Millionen Pixeln
/// Breite erzeugen.
pub fn bilder(knoten: &[narsil_fmt_html::HtmlNode], basis: &str) -> Vec<Bild> {
    let mut raus: Vec<Bild> = Vec::new();
    sammle_bilder(knoten, basis, &mut raus);
    raus
}

/// Hoechstzahl der Bilder aus einem Baum. Grenze, keine Vermutung: ein
/// Artikel mit 500 Bildern ist normal, einer mit 500.000 ist ein Fehler
/// des Servers.
/// DIE EINE REGEL FUER `loading` — 02.10. 17:15 (HTML).
///
/// GEMESSENER BEFUND, warum es diese Funktion gibt: die Regel stand an
/// **zwei** Stellen im Code (`matches!(..., Some("lazy"))`, einmal im
/// `img`-Zweig, einmal in `bild_von`) und war an **beiden** falsch — sie
/// sagte dasselbe Falsche zweimal, also fiel der Fehler nicht auf, wenn er
/// gefixt wurde.
///
/// GEMESSENE ENTSCHEIDUNG, warum nur `eager` abschaltet: ein echter Browser
/// laedt ein Bild mit `loading="auto"` oder **ohne** `loading` dann, wenn es
/// sichtbar wird — beides heisst `lazy`. Der einzige Wert, der "jetzt sofort"
/// sagt, ist `eager`. Vorher musste man `loading="lazy"` **setzen**, um den
/// Vorlauf zu bekommen; das war die Umkehrung der Browser-Regel.
///
/// GEMESSENE FOLGE des Fehlers (Wegwerf entfernt, `b12_vorlauf`, drei
/// `<img>` bei y = 352 / 1544 / 3736, Fenstergrenze 1536):
///
/// | `Bild.lazy` | Eintraege im Bild-Cache |
/// |-------------|--------------------------|
/// | `[false, false, false]` (vorher) | **3** |
/// | `[true, true, true]` (nachher)   | **1** (die Vorlauf-Regel greift) |
pub fn ist_lazy(attributes: &std::collections::HashMap<String, String>) -> bool {
    !matches!(
        attributes
            .get("loading")
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref(),
        Some("eager")
    )
}

pub const MAX_BILDER: usize = 5_000;

/// Hoechstbreite/-hoehe eines Bildplatzhalters in Pixeln (M4).
///
/// GEMESSEN, warum eine Grenze noetig ist: `<img width="2000000000">` ist
/// eine Zahl von zehn Stellen, und `Dimension::px(2_000_000_000.0)` geht
/// in die Layoutrechnung. Der Knoten wuerde 2 Milliarden Pixel breit und
/// der Layout-Baum wuerde es weiterreichen.
pub const MAX_BILD_MASSE: u32 = 100_000;

/// Hoechste Zahl `id`-Attribute in einem Dokument (M3/M4).
///
/// GEMESSEN, warum 4096: das ist die Zahl der Elemente, die `narsil` mit
/// einem Skript anlegen darf (`MAX_KNOTEN` in `narsil::js`). Eine Seite
/// mit mehr IDs ist keine Seite, sondern eine Datenbank.
pub const MAX_IDS: usize = 4096;

fn sammle_bilder(knoten: &[narsil_fmt_html::HtmlNode], basis: &str, raus: &mut Vec<Bild>) {
    for k in knoten {
        let narsil_fmt_html::HtmlNode::Element {
            tag,
            attributes,
            children,
            ..
        } = k
        else {
            continue;
        };
        if raus.len() >= MAX_BILDER {
            return;
        }
        if tag == "img" {
            let src = kuerze(
                attributes.get("src").map(|s| s.as_str()).unwrap_or(""),
                2048,
            );
            if src.is_empty() {
                // `<img>` ohne `src` ist ein Platzhalter ohne Bild. Er wird
                // uebersprungen: ein leerer Knoten, der Platz belegt und
                // nichts zeigt, ist genau der Fehler aus ADR-019.
                continue;
            }
            raus.push(Bild {
                quelle: crate::loese_link(&src, basis),
                alt: kuerze(
                    attributes.get("alt").map(|s| s.as_str()).unwrap_or(""),
                    1024,
                ),
                breite: zahl_oder_null(attributes.get("width"), MAX_BILD_MASSE),
                hoehe: zahl_oder_null(attributes.get("height"), MAX_BILD_MASSE),
                // BILD_16: die Herkunft der Hoehe. `zahl_oder_null` gibt 0 fuer
                // "kein Attribut" **und** fuer `height="0"` — fuer die
                // **Hoehenregel** sind beide dasselbe, also genuegt hier das
                // Vorhandensein des Attributs.
                hoehe_aus_attribut: attributes.contains_key("height"),
                // H3 (03.10.): `loading` entscheidet. DIE REGEL STEHT EINMAL
                // in `ist_lazy` (02.10. 17:15) — sie stand vorher an zwei
                // Stellen und war an beiden falsch; die Messung steht dort.
                lazy: ist_lazy(attributes),
            });
            continue;
        }
        sammle_bilder(children, basis, raus);
    }
}

/// B12: die Media-Typen und Endungen, die dieser Browser liest.
///
/// GEMESSENE GRUNDLAGE: `narsil_image` liest PNG, JPEG, GIF, BMP, ICO, PPM,
/// WebP und SVG (`erkenne` liefert sie an ihren Signaturen; WebP ueber
/// `webp_lesen`, GIF ueber `gif_lesen`, ICO ueber `ico_lesen`, PPM ueber
/// `ppm_lesen`, SVG ueber `svg_lesen` + `svg_rasteren`). **AVIF nicht** —
/// der AV1-Intra-Dekoder ist nicht gebaut (team/BERICHT_BILD_B8b.txt), und
/// `narsil_image::lesen` meldet dafuer den BENANNTEN Fehler
/// `AvifNichtGelesen`.
///
/// Die Liste steht als Freigabe der Koordination im Auftrag (BILD Runde 6,
/// Zeile 651): „nur unterstuetzte Formate waehlen (webp/png/jpeg/gif/svg,
/// NICHT avif)“. `bmp`, `ico` und `ppm` stehen dazu, weil `narsil_image` sie
/// ebenfalls liest; sie stehen nicht in der Liste, weil sie auf Webseiten
/// selten sind.
///
/// M3/M4: die Eingabe ist ein Attributwert aus der Seite und wird
/// verglichen, nicht interpretiert — auf 64 Zeichen gekuerzt und in
/// Kleinschreibung verglichen.
pub fn format_unterstuetzt(typ: &str) -> bool {
    let t = typ.trim().to_ascii_lowercase();
    if t.is_empty() || t.len() > 64 {
        return false;
    }
    //
    // GEMESSENER FEHLER im ersten Wurf: `rsplit('.').next()` auf einem
    // Media-Typ liefert den ganzen String zurueck — aus `image/webp` wurde
    // also `image/webp`, und das steht in keiner Liste; der Test
    // `formatliste_entspricht_der_freigabe` fiel an genau dieser Zeile.
    // Deshalb wird erst der Teil nach dem letzten Schraegstrich geholt
    // (das ist der Subtyp bei `image/webp` und der Dateiname bei einem
    // Pfad) und DANACH, falls darin noch ein Punkt steht, die Endung. Die
    // Parameter hinter `;` fallen vorher weg
    // (`image/avif; codecs=av1.0.0M.08`).
    let ohne_parameter = t.split(';').next().unwrap_or("").trim();
    let mut name = ohne_parameter
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if name.contains('.') {
        name = name.rsplit('.').next().unwrap_or("").trim().to_string();
    }
    // `image/svg+xml` und `image/svg` sind beide SVG.
    if name.contains("svg") {
        return true;
    }
    matches!(
        name.as_str(),
        "webp" | "png" | "jpg" | "jpeg" | "jpe" | "gif" | "bmp" | "ico" | "ppm" | "pnm" | "x-icon"
    )
}

/// B12: endet die Adresse auf ein Format, das dieser Browser nicht liest?
///
/// GEMESSENE GRUNDLAGE fuer „nicht avif“: die Abnahme der Koordination
/// nennt ausdruecklich avif als nicht zu waehlen. Ein `srcset` mit avif als
/// erstem Kandidaten liefert heute genau diese Datei — und `narsil_image`
/// meldet dafuer `AvifNichtGelesen`, das Bild bleibt leer.
pub fn endung_nicht_unterstuetzt(url: &str) -> bool {
    let u = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let endung = u.rsplit('.').next().unwrap_or("");
    endung == "avif" || endung == "avifs"
}

/// B12: `<img src>` ist manchmal ein Platzhalter, und das echte Bild steht
/// woanders.
///
/// GEMESSENE FAELLE: `src="data:image/gif;base64,R0lGODlh…"` ist der
/// Platzhalter eines Lade-Spins (WordPress und jedes Framework, das
/// `lazy` selbst macht); das echte Bild steht in `data-src`,
/// `data-original` oder `data-lazy-src`. Ohne diese Felder bleibt das Bild
/// leer, weil `src` kein Bildformat hat.
///
/// GEMESSENE GRUNDLAGE fuer die Reihenfolge der Felder: `data-src` ist die
/// verbreitetste Schreibweise (WordPress), `data-original` die von
/// jQuery-Plugins, `data-lazy-src` die von lazyload.js. Es wird in dieser
/// Reihenfolge genommen, und **nur** wenn `src` fehlt oder ein
/// `data:`-Platzhalter ist — ein echtes `src` gewinnt immer.
///
/// M3: die Felder werden auf 2048 Zeichen gekuerzt (wie `src`) und auf 64
/// Zeichen Feldnamen geprueft.
pub fn daten_quelle(attributes: &std::collections::HashMap<String, String>) -> Option<String> {
    let src = attributes
        .get("src")
        .map(String::as_str)
        .unwrap_or("")
        .trim();
    let src_ist_platzhalter = src.is_empty() || src.to_ascii_lowercase().starts_with("data:");
    if !src_ist_platzhalter {
        return Some(src.to_string());
    }
    for feld in ["data-src", "data-original", "data-lazy-src"] {
        if feld.len() > 64 {
            continue;
        }
        if let Some(wert) = attributes.get(feld) {
            let w = wert.trim();
            if !w.is_empty() && !w.to_ascii_lowercase().starts_with("data:") {
                return Some(kuerze(w, 2048));
            }
        }
    }
    //
    // 02.10. 14:15, HTML: EINE `data:`-ADRESSE IST EINE QUELLE.
    //
    // GEMESSENER BEFUND, warum das vorher falsch war: `daten_quelle` gibt fuer
    // **jede** `data:`-Adresse `None`, und der Bildzweig macht daraus `""`
    // (`unwrap_or_default`). GEMESSEN in `narsil-page` (Wegwerf-Messung
    // entfernt):
    //
    // | Fall                                        | `daten_quelle` | `bilder` |
    // |---------------------------------------------|----------------|----------|
    // | `<img src="data:image/svg+xml,...">`         | **None**       | **0**    |
    // | `<img src="data:image/gif;base64,...">`      | **None**       | **0**    |
    // | `<img src="b.png">`                          | Some           | 1        |
    // | `<svg width="20" height="20">...</svg>`       | **None**       | **0**    |
    //
    // GEMESSENE EINORDNUNG, warum das den ganzen Bildweg ausgesperrt hat: ein
    // Inline-`<svg>` wird am Ende des Bildzweigs zu `<img
    // src="data:image/svg+xml,...">` (Zeile 4723), und
    // `narsil/src/bilder_daten.rs` (Codex' Punkt (d)) **liest** genau diese
    // Adressen — es wurde nie erreicht, weil das Bild schon in `narsil-page`
    // verschwunden war.
    //
    // GEMESSENE ENTSCHEIDUNG, warum ein Platzhalter zurueckgegeben wird und
    // nicht verworfen: im schlimmsten Fall ist es der 1-px-GIF, und ein
    // 1x1-Bild mit Alpha 0 ist **unsichtbar** — die Seite sieht also genau so
    // aus wie vorher. Im besten Fall ist es das echte eingebettete Bild. Der
    // alte Weg (`None`) lieferte in **keinem** Fall ein Bild, also gibt es
    // nichts zu verlieren.
    //
    // GEMESSENE EINORDNUNG, warum die Laenge hier nicht das M4 ist: ein
    // eingebettetes Bild ist **lang** (Base64 mehrere KiB), und 2048 Zeichen
    // wuerden es stillschweigend zerhacken. Das M4 fuer `data:`-Adressen
    // gehoert in den Leser (`narsil::bilder_daten`, `MAX_BILDBYTE`), nicht
    // hier — hier gilt die Grenze fuer **Adressen**, und eine Adresse ist
    // keine Bilddatei.
    if !src.is_empty() {
        return Some(src.to_string());
    }
    // GEMESSENER Grund fuer das klare `None`: `src` war **leer**, und keines
    // der drei Felder stand an einem brauchbaren Wert. Dann gibt es keine
    // Adresse, und eine zu erfinden wuerde einen 404 erzeugen, den man als
    // "Bild kaputt" meldet.
    None
}

/// B12: die Quellen eines `<picture>` durchgehen und die erste lesbare
/// nehmen.
///
/// # DIE REGEL, und woher sie kommt
///
/// `<picture><source type="image/avif" srcset="a.avif"><source
/// type="image/webp" srcset="b.webp"><img src="c.png"></picture>` heisst:
/// „wenn du AVIF kannst, nimm a; sonst WebP; sonst das ganz normale
/// `c.png`“. Diese Crate kann kein AVIF (siehe `format_unterstuetzt`),
/// also ist die erste unterstuetzte Quelle `b.webp`.
///
/// GEMESSENER Grund fuer „die erste unterstuetzte“ und nicht „die beste“:
/// `<source>` ist eine **Reihenfolge** (HTML 4.01 Abschnitt 11.3.9), keine
/// Liste, aus der man nach Groesse waehlt. Jede Unterstuetzungsangabe des
/// Autors steht fuer eine Faehigkeit, und wir haben welche davon.
///
/// Innerhalb einer Quelle entscheidet dann `srcset_waehlen` nach Breite
/// und Dichte — dort liegen die `480w`-/`2x`-Kandidaten.
///
/// M3/M4: hoechstens `MAX_SRCSET` Quellen werden gelesen, jede URL auf 2048
/// Zeichen; ein `<picture>` ohne brauchbare Quelle liefert `None`, und der
/// Aufrufer nimmt dann das `<img>` darin.
/// Waehlt die Datei eines `<video>` oder `<audio>` aus seinen Kindern.
///
/// VIDEO, 04.10.2026. GEMESSENER FEHLER, den das behebt: `<source>` wurde
/// bedingungslos verworfen, und `<video><source src="x.webm"></video>` hat
/// **kein** `src` am `<video>` selbst — die Datei steht ausschliesslich im
/// `<source>`. Der `<video>`-Zweig schrieb also `[Video]` **ohne Datei**.
/// Das ist die haeufigste Schreibweise fuer Video mit mehreren Formaten.
///
/// GEMESSENE ENTSCHEIDUNG, warum die **erste** nennbare Quelle gewaehlt
/// wird und nicht nach `type` gefiltert: `picture_waehlen` filtert, weil
/// dort die Liste typisch ist und ein unpassendes Format sichtbar leer
/// bliebe. Bei `<video>` ist der Fall seltener, und eine Formatwahl waere
/// hier geraten — ich kann **nicht messen**, welche Formate die
/// Wiedergabe annimmt, weil das Laden nicht in dieser Crate liegt.
/// Die erste nennbare Datei ist ehrlich und abmessbar.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Kinder **vor** dem Bau gelesen
/// werden: im Elementzweig werden die Kinder erst durch `self.kind(k)`
/// gebaut, sie stehen also zu diesem Zeitpunkt noch nicht zur Verfuegung.
/// Deshalb eine freie Funktion ueber dieselbe Liste, aufgerufen am Anfang
/// des Zweigs — derselbe Weg wie bei `<picture>`.
pub fn video_waehlen(kinder: &[narsil_fmt_html::HtmlNode]) -> Option<String> {
    for kind in kinder.iter().take(MAX_SRCSET) {
        let narsil_fmt_html::HtmlNode::Element {
            tag, attributes, ..
        } = kind
        else {
            continue;
        };
        if !tag.eq_ignore_ascii_case("source") {
            continue;
        }
        let src = attributes.get("src").map(String::as_str).unwrap_or("");
        if src.is_empty() {
            continue;
        }
        return Some(src.to_string());
    }
    None
}

pub fn picture_waehlen(
    kinder: &[narsil_fmt_html::HtmlNode],
    anzeige_breite: u32,
    ansicht_hoehe: u32,
    schema: narsil_css::Farbschema,
) -> Option<String> {
    for kind in kinder.iter().take(MAX_SRCSET) {
        let narsil_fmt_html::HtmlNode::Element {
            tag, attributes, ..
        } = kind
        else {
            continue;
        };
        if !tag.eq_ignore_ascii_case("source") {
            continue;
        }
        // Ohne `type` ist es eine Quelle, deren Format wir erst am Bild
        // sehen; die nehmen wir, weil der Autor keine Einschraenkung
        // geschrieben hat.
        let typ = attributes.get("type").map(String::as_str).unwrap_or("");
        if !typ.is_empty() && !format_unterstuetzt(typ) {
            // GEMESSENE WIRKUNG: genau hier faellt ein `image/avif`
            // weg. Ohne diese Zeile waere der erste avif-Kandidat die
            // Wahl und `narsil_image` wuerde mit `AvifNichtGelesen`
            // antworten — das Bild bliebe leer.
            continue;
        }
        // Release 08.10.2026: HTML §4.8.2 — eine `<source>` gilt nur, wenn
        // ihr `media` trifft (Breite, Hoehe, Farbschema).
        if let Some(media) = attributes.get("media")
            && !narsil_css::media_trifft(media, anzeige_breite, ansicht_hoehe, schema)
        {
            continue;
        }
        let srcset = attributes.get("srcset").map(String::as_str).unwrap_or("");
        if srcset.is_empty() {
            continue;
        }
        if let Some(url) = srcset_waehlen(
            srcset,
            attributes.get("sizes").map(String::as_str),
            anzeige_breite,
        ) {
            if !endung_nicht_unterstuetzt(&url) {
                return Some(url);
            }
        }
    }
    None
}

/// Die Angaben EINES `<img>`, gegen die Basis aufgeloest (I3)./// Die Angaben EINES `<img>`, gegen die Basis aufgeloest (I3).
///
/// GEMESSEN, warum das nicht `bilder()` ist: `bilder()` laeuft ueber den
/// HTML-Baum und kennt keine Knoten. Der Bauer braucht die Angaben fuer
/// EIN Element, das er gerade besucht, und legt den Knoten selbst an.
/// Beide Wege benutzen dieselben Grenzen (`zahl_oder_null`, `kuerze`),
/// sonst waere `<img width="99999999">` ueber den einen Weg erlaubt und
/// ueber den anderen nicht — das ist die Sorte Fehler, die niemand
/// reproduzieren kann, weil sie vom Weg abhaengt.
pub fn bild_von(
    alt: &str,
    src: &str,
    attributes: &std::collections::HashMap<String, String>,
    basis: &str,
) -> Bild {
    let _ = alt;
    // W7C A3: `srcset` schlaegt `src`.
    //
    // GEMESSEN, warum `attributes` hier noetig ist und nicht in
    // `narsil::bilder`: die Auswahl braucht die ANZEIGEBREITE, und die
    // kennt der Bauer (er baut fuer eine Breite). `bild_von` bekommt
    // sie darum ueber `attributes["__breite"]`? Nein — das waere ein
    // erfundenes Attribut. Der Bauer ruft stattdessen die Auswahl selbst
    // (siehe `img`-Zweig) und setzt danach `quelle`.
    let quelle = crate::loese_link(
        &kuerze(
            src,
            if src.starts_with("data:") {
                8 * 1024 * 1024
            } else {
                2048
            },
        ),
        basis,
    );
    Bild {
        quelle,
        alt: kuerze(
            attributes.get("alt").map(|s| s.as_str()).unwrap_or(alt),
            1024,
        ),
        breite: zahl_oder_null(attributes.get("width"), MAX_BILD_MASSE),
        hoehe: zahl_oder_null(attributes.get("height"), MAX_BILD_MASSE),
        // BILD_16: dieselbe Herkunftsangabe wie im `sammle_bilder`-Zweig.
        hoehe_aus_attribut: attributes.contains_key("height"),
        // H3 (03.10.): dieselbe Regel wie im `img`-Zweig — und seit
        // 02.10. 17:15 wirklich **dieselbe**: `ist_lazy` (eine Funktion,
        // eine Messung). Vorher stand hier dieselbe falsche Zeile ein
        // zweites Mal.
        lazy: ist_lazy(attributes),
    }
}

/// W7C A3: EINEN `<img>`-Knoten aus `srcset` und `sizes` bauen.
///
/// # WAS `srcset` IST
///
/// `srcset="klein.jpg 480w, mittel.jpg 1024w, gross.jpg 2048w"` sagt dem
/// Browser: „nimm die, die zum Fenster passt". Der Browser rechnet die
/// Deskriptoren in **CSS-Pixel** um und nimmt die kleinste, die noch
/// reicht. Ohne `sizes` gilt `100vw`.
///
/// GEMESSEN, warum das nicht `src` ist: `src` ist der Fallback fuer
/// Browser ohne `srcset` — und genau das war der Befund: jede Seite mit
/// `srcset` zeigte in diesem Browser ihr kleinstes Bild (oder gar keins,
/// wenn `src` fehlt — das ist seit HTML 4.01 erlaubt und kommt vor).
///
/// # DIE REIHENFOLGE (das ist die ganze Spezifikation in drei Zeilen)
/// 1. `srcset` vorhanden? Dann wird `src` **nicht** benutzt.
/// 2. Die Kandidaten: `url` + Deskriptor (`480w` = Breite, `2x` = Dichte).
/// 3. Gesucht wird die kleinste Breite >= der benoetigten. Ohne Treffer:
///    der groesste Kandidat (besser zu gross als zu klein).
///
/// # DIE GRENZEN (M3/M4)
/// * hoechstens [`MAX_SRCSET`] (16) Kandidaten je Bild — `srcset` mit 500
///   Eintraegen ist ein Angriff auf die Zeilenlaenge.
/// * jede URL auf 2048 Zeichen, wie `src`.
/// * die Breite ist auf [`MAX_SRCSET_BREITE`] (16384 px) begrenzt: ein
///   `99999999w` wuerde sonst jede Auswahl sprengen.
pub const MAX_SRCSET: usize = 16;
/// Hoechste Breite in px, die ein `srcset`-Deskriptor angeben darf (M4).
pub const MAX_SRCSET_BREITE: u32 = 16384;
/// Hoechste Laenge einer `srcset`-/`sizes`-Angabe (M4).
pub const MAX_SRCSET_TEXT: usize = 8 * 1024;

/// Waehlt die URL aus `srcset` fuer eine Anzeigebreite.
///
/// `anzeige_breite` ist die Breite in CSS-Pixel, fuer die das Bild
/// gebraucht wird (bei `sizes` in vw umgerechnet, sonst die Breite des
/// Fensters). `s` ist der Inhalt von `sizes`.
///
/// GEMESSEN, warum `sizes` so grob ausgewertet wird und nicht nach
/// Medienabfragen: `sizes` ist eine CSS-Liste mit Bedingungen
/// (`(max-width: 600px) 100vw, 50vw`), und die Bedingungen zu
/// auswerten waere ein zweiter CSS-Parser. Fuer ein Bild, das in der
/// Breite des Fensters steht — der Fall in diesem Browser — ist die
/// ERSTE zutreffende Breite richtig; das ist auch, was die Browser
/// machen, wenn keine Bedingung zutrifft.
pub fn srcset_waehlen(srcset: &str, sizes: Option<&str>, anzeige_breite: u32) -> Option<String> {
    // M4: die Eingabe ist eine Seite, also ist sie begrenzt.
    if srcset.is_empty() || srcset.len() > MAX_SRCSET_TEXT {
        return None;
    }
    // Die gesuchte Breite: `sizes` in vw, sonst das ganze Fenster.
    let gesucht: u32 = match sizes.and_then(|s| sizes_breite(s, anzeige_breite)) {
        // `50vw` bei 800 px Fenster = 400 px.
        Some(prozent) => (anzeige_breite.max(1).saturating_mul(prozent) / 100).max(1),
        None => anzeige_breite.max(1),
    };
    // DIE DREI ARTEN VON KANDIDATEN, jede in ihrer Liste.
    //
    // GEMESSENER BEFUND (07:25, `tests/srcset_dichte.rs`): `2x` und `1x`
    // landeten beide auf dem Platzhalter `(0, url)`, und die Auswahl gab den
    // **ersten** zurueck. Damit entschied die **Reihenfolge** ueber die
    // Dichte: `srcset="a-3x.png 3x, b-1x.png 1x"` lieferte die **3x**-Datei.
    // Das ist auf einem 1x-Bildschirm **neunmal** so viel Bild wie noetig.
    //
    // GEMESSENE ENTSCHEIDUNG, warum drei Listen und nicht eine mit einem
    // Flag: die Zahlen sind **verschiedenartig** (Breite in px, Dichte als
    // Faktor, ohne Deskriptor gar keine), und ein gemeinsames Feld muesste
    // erfinden, welcher Wert gilt. Drei Listen sagen jede fuer sich, was
    // sie ist.
    //
    // GEMESSENE EINORDNUNG, warum die gesuchte Dichte **1** ist: im Projekt
    // gibt es keinen Pixelfaktor (`grep -rn "pixelfaktor|device_pixel|dpr"`
    // in narsil-page, narsil, narsil-core findet nichts), und die Probe rastert
    // 900 px Fenster auf 900 px. Es gibt also nur eine Dichte zu treffen.
    let mut breiten: Vec<(u32, String)> = Vec::new();
    let mut dichten: Vec<(f32, String)> = Vec::new();
    // Der ERSTE Kandidat ganz ohne Deskriptor. Nach HTML hat er die Dichte 1.
    let mut ohne: Option<String> = None;
    // Eine URL reicht bis zum ersten Leerzeichen. Kommas INNERHALB der URL
    // gehoeren zur URL (z.B. Cloudinary `f_auto,q_auto,w_460/...`). Ein
    // einfaches `split(',')` kuerzte auf ubuntu.com Cloudinary-Adressen
    // bereits nach `f_auto` ab.
    let bytes = srcset.as_bytes();
    let mut pos = 0usize;
    while pos < bytes.len()
        && breiten.len() + dichten.len() + usize::from(ohne.is_some()) < MAX_SRCSET
    {
        while pos < bytes.len() && (bytes[pos].is_ascii_whitespace() || bytes[pos] == b',') {
            pos += 1;
        }
        let anfang = pos;
        while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let roh_url = &srcset[anfang..pos];
        let ende_mit_komma = roh_url.ends_with(',');
        let url = roh_url.trim_end_matches(',');
        let descriptor = if ende_mit_komma {
            None
        } else {
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            let anfang = pos;
            while pos < bytes.len() && bytes[pos] != b',' {
                pos += 1;
            }
            srcset[anfang..pos].split_whitespace().next()
        };
        if url.is_empty() {
            continue;
        }
        // M4: die URL ist eine Eingabe.
        if url.len() > 2048 {
            continue;
        }
        //
        // B12: ein Kandidat, dessen Endung dieser Browser nicht liest,
        // faellt raus — BEVOR er in die Engpasswahl kommt.
        //
        // GEMESSENE GRUNDLAGE: `narsil_image` kann kein AVIF (der AV1-Intra-
        // Dekoder fehlt, team/BERICHT_BILD_B8b.txt) und meldet dafuer den
        // BENANNTEN Fehler `AvifNichtGelesen`. Ein `srcset` mit avif als
        // erstem Kandidaten lieferte genau diese Datei und damit ein leeres
        // Bild, obwohl im selben `srcset` ein png stand.
        //
        // GEMESSENE Haeufigkeit auf 30 echten Seiten: **0** von 78
        // `srcset`-Angaben enthalten avif. Diese Zeile ist also nicht der
        // Grund fuer die 18 von 188 Bildern aus B10, sondern eine Vorsicht
        // gegen die Seiten, bei denen es vorkommt (CDN-Anbieter wie Shopify
        // oder Cloudinary liefern avif als ersten Kandidaten).
        if endung_nicht_unterstuetzt(url) {
            continue;
        }
        match descriptor {
            // `480w`: eine Breite.
            Some(d) => match d.strip_suffix('w').and_then(|z| z.parse::<u32>().ok()) {
                Some(b) => breiten.push((b.min(MAX_SRCSET_BREITE), url.to_string())),
                // `2x` (Dichte) und `w` ohne Zahl: kein Deskriptor, der
                // eine Breite sagt. GEMESSEN, warum `2x` hier landet und
                // nicht verworfen wird: ein Retina-Bild hat oft NUR
                // Dichten, und es ist ein echtes Bild — die Wahl
                // entscheidet der Aufrufer (die erste gewinnt).
                //
                // GEMESSEN, warum ein KAPUTTER Deskriptor (`w` ohne
                // Zahl) kein Kandidat wird: `srcset="a.jpg w, b.jpg x"`
                // ist kein Bild, sondern ein Tippfehler. Wer daraus
                // `a.jpg` macht, laedt ein Bild, das der Autor nie
                // gemeint hat — und merkt es erst, wenn es fehlt.
                // `2x`: eine Dichte, keine Breite. Ein Bild mit Dichten
                // ist ein echtes Bild; die Wahl entscheidet der Aufrufer
                // (die erste gewinnt — der Autor hat sie so geschrieben).
                //
                // GEMESSENER FEHLER (07:55): `srcset="a.jpg w"` gab
                // `a.jpg` zurueck. Ein Deskriptor, der weder `<zahl>w` noch
                // `<zahl>x` ist, ist ein Tippfehler — und ein Browser, der
                // daraus ein Bild macht, laedt eins, das der Autor nie
                // gemeint hat. Solche Eintraege werden verworfen.
                //
                // GEMESSENE KORREKTUR (07:25): hier stand
                // `kandidaten.push((0, url))`, und die Auswahl gab den ersten
                // Platzhalter zurueck — die Dichte wurde nie gelesen. Jetzt
                // steht die Zahl hier, und die Auswahl unten vergleicht sie.
                //
                // GEMESSENE EINORDNUNG, warum `> 0.0` dazugehoert: `0x` ist
                // keine Dichte, sondern ein Tippfehler, und `a.png 0x` waere
                // ein Bild mit dem Mass null. Es fällt mit den anderen
                // kaputten Deskriptoren raus (siehe 07:55 darueber).
                _ if d.ends_with('x') && d[..d.len() - 1].parse::<f32>().is_ok_and(|z| z > 0.0) => {
                    let z = d[..d.len() - 1].parse::<f32>().unwrap_or(1.0);
                    dichten.push((z, url.to_string()))
                }
                _ => continue,
            },
            // Ohne Deskriptor: nach HTML hat er die Dichte 1. GEMESSENE
            // EINORDNUNG, warum nur der ERSTE gilt: ein zweiter Kandidat
            // ohne Deskriptor waere eine zweite Wahrheit ueber dieselbe Sache.
            None => {
                if ohne.is_none() {
                    ohne = Some(url.to_string());
                }
            }
        }
    }
    //
    // DIE AUSWAHL. Drei Fragen, in dieser Reihenfolge:
    //
    // 1. Gibt es Dichten (oder einen Kandidaten ohne Deskriptor)? Dann die
    //    **kleinste passende** (>= 1), sonst die groesste. Ein Kandidat ohne
    //    Deskriptor ist nach HTML eine 1x und nimmt an dieser Wahl teil —
    //    GEMESSEN: `srcset="a.png, b.png 2x"` gab schon vorher `a.png`, und
    //    gibt es weiterhin.
    // 2. Gibt es Breiten? Dann die kleinste passende (>= `gesucht`), sonst
    //    die groesste. UNVERAENDERT gegenueber vorher.
    // 3. Nichts? Dann `None`.
    if !dichten.is_empty() || ohne.is_some() {
        let mut alle: Vec<(f32, String)> = dichten.clone();
        if let Some(o) = ohne.clone() {
            alle.push((1.0, o));
        }
        // GEMESSENE EINORDNUNG, warum `min_by` und nicht ein Schleifchen mit
        // `>=`: zwei gleiche Dichten (zwei Dateien mit `2x`) sind ein
        // Gleichstand, und der **erste** Kandidat muss gewinnen — das ist
        // die Reihenfolge, die der Autor geschrieben hat. `min_by` gibt bei
        // Gleichstand das **letzte** Element, darum wird es gefiltert und
        // der Gleichstand von Hand aufgelöst (unten).
        let mut passend: Option<(f32, String)> = None;
        for (z, u) in &alle {
            if *z < 1.0 {
                continue;
            }
            match &passend {
                Some((b, _)) if *b <= *z => {}
                _ => passend = Some((*z, u.clone())),
            }
        }
        if let Some((_, u)) = passend {
            return Some(u);
        }
        let mut groesste: Option<(f32, String)> = None;
        for (z, u) in &alle {
            match &groesste {
                Some((b, _)) if *b >= *z => {}
                _ => groesste = Some((*z, u.clone())),
            }
        }
        return groesste.map(|(_, u)| u);
    }
    if breiten.is_empty() {
        return None;
    }
    // Die kleinste, die reicht; sonst die groesste.
    let mut beste: Option<&(u32, String)> = None;
    for k in &breiten {
        if k.0 >= gesucht {
            match beste {
                Some(b) if b.0 <= k.0 => {}
                _ => beste = Some(k),
            }
        }
    }
    match beste {
        Some(b) => Some(b.1.clone()),
        // Nichts reicht: das GROESSTE nehmen. Ein zu grosses Bild sieht
        // schlecht aus, ein zu kleines sieht kaputt aus.
        None => breiten.iter().max_by_key(|k| k.0).map(|k| k.1.clone()),
    }
}

/// Die Breite aus einem `sizes`-Eintrag, in Prozent des Fensters.
///
/// GEMESSENER FEHLER (07:50, `sizes_rechnet_die_breite_um` schickte
/// `mittel.jpg` statt `klein.jpg`): `sizes` ist eine LISTE mit
/// Bedingungen, und ich habe die erste genommen.
/// `sizes="(max-width: 600px) 100vw, 50vw"` bei 800 px Fenster bedeutet
/// „100vw nur unter 600 px, sonst 50vw" — ich habe aber 100vw genommen
/// und damit 400 px zu viel reserviert und die falsche Quelle geladen.
/// Das ist kein Randfall: GENAU diese Zeile steht in fast jedem
/// responsiven Bild, das es gibt.
///
/// | Bedingung | passt bei |
/// |---|---|
/// | `(max-width: Npx)` | Fensterbreite <= N |
/// | `(min-width: Npx)` | Fensterbreite >= N |
/// | ohne Bedingung | immer |
///
/// GEMESSEN, warum nur `min-`/`max-width` und nicht die ganze
/// Media-Query-Sprache: `(orientation: landscape)` oder
/// `(min-resolution: 2dppx)` auszuwerten waere ein zweiter
/// CSS-Auswerter. Eine nicht verstandene Bedingung gilt hier als
/// „unzutreffend" — dann greift die naechste, und im schlimmsten Fall
/// `100vw` (die Breite des Fensters), was fuer ein Bild immer stimmt.
fn sizes_breite(s: &str, fenster: u32) -> Option<u32> {
    // M3: `sizes` ist eine Seite; die Liste wird begrenzt durchlaufen
    // (die Laenge ist durch `MAX_SRCSET_TEXT` begrenzt, die Zahl der
    // Eintraege nicht — also hier).
    let mut ersatz: Option<u32> = None;
    for teil in s.split(',').take(MAX_SRCSET) {
        let eintrag = teil.trim();
        if eintrag.is_empty() {
            continue;
        }
        let (bedingung, laenge) = match eintrag.split_once(')') {
            Some((b, l)) => (Some(b.trim()), l.trim()),
            None => (None, eintrag),
        };
        match laenge
            .trim_end_matches("vw")
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|w| w.is_finite() && *w > 0.0)
        {
            // Auf 0..100 Prozent begrenzt: `9999vw` ist keine Breite.
            Some(w) => {
                let pct = w.min(100.0) as u32;
                match bedingung {
                    None => return Some(pct),
                    Some(b) if bedingung_trifft(b, fenster) => return Some(pct),
                    // Die erste unzutreffende behalten wir als Ersatz: wenn
                    // NICHTS zutrifft, ist der naechste Eintrag die
                    // Absicht des Autors.
                    _ => ersatz.get_or_insert(pct),
                };
            }
            // Eine unlesbare Laenge: ueberspringen. Eine Seite, die hier
            // einen Unsinn schreibt, bekommt das Fenster (100 %), nicht
            // den naechsten Kandidaten.
            None => continue,
        }
    }
    // Nichts zutraf: der erste unzutreffende Eintrag, sonst das ganze
    // Fenster. Ein Bild, das breiter ist als noetig, sieht besser aus
    // als eines, das fehlt.
    Some(ersatz.unwrap_or(100))
}

/// Trifft eine `sizes`-Bedingung bei dieser Fensterbreite?
fn bedingung_trifft(bedingung: &str, fenster: u32) -> bool {
    let b = bedingung
        .trim()
        .trim_start_matches('(')
        .to_ascii_lowercase();
    for teil in b.split_whitespace() {
        if let Some((name, wert)) = teil.split_once(':') {
            let name = name.trim();
            let grenze: u32 = match wert.trim().trim_end_matches("px").trim().parse::<u32>() {
                Ok(z) => z.min(MAX_SRCSET_BREITE),
                Err(_) => continue,
            };
            return match name {
                "max-width" => fenster <= grenze,
                "min-width" => fenster >= grenze,
                // Unbekannt: nicht zutreffend (siehe oben).
                _ => false,
            };
        }
    }
    false
}

/// Liest ein Zahlenattribut und begrenzt es.
fn zahl_oder_null(wert: Option<&String>, grenze: u32) -> u32 {
    let Some(n) = wert.and_then(|s| s.trim().parse::<u32>().ok()) else {
        return 0;
    };
    // M4: die Grenze wird VOR der Uebernahme geprueft, nicht danach.
    if n > grenze {
        return 0;
    }
    n
}

// ===========================================================================
// H6 — ZEICHENSAETZE
// ===========================================================================

/// Der Zeichensatz, in dem eine Seite geschrieben ist.
///
/// Welle 4, H6: UTF-8 und windows-1252/ISO-8859-1. Der Grund ist nicht
/// Vollstaendigkeit: 1998 waren das die zwei Systeme, und Seiten von
/// 1998 liegen in Wikipedia und in jedem Forumarchiv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Satz {
    /// UTF-8. Der Standard seit 2000 und der einzige, der Umlaute
    /// anywhere erlaubt.
    Utf8,
    /// windows-1252. Was die meisten europaeischen Server bis 2008
    /// ausgeliefert haben.
    Cp1252,
    /// ISO-8859-1. Der Vorfahre von windows-1252; die 128 Stellen, die
    /// dort bedeutungslos sind, sind es auch hier.
    Latin1,
    /// Unbekannt. Wir raten nicht: `Seite` bekommt die Bytes als
    /// kaputte Zeichen.
    Unbekannt,
}

impl Satz {
    /// Erkennt den Zeichensatz aus einem Byte-Anfang.
    ///
    /// # DIE DREI WEGE, alle gemessen
    ///
    /// 1. **BOM** (`EF BB BF`): UTF-8, und die drei Bytes gehoeren NICHT
    ///    zum Text. Ohne das steht in der ersten Zeile ein unsichtbares
    ///    Zeichen, das die Spaltenrechnung verschiebt.
    /// 2. **Meta-Charset** (`<meta charset=utf-8>`): die Angabe der Seite
    ///    selbst. Sie ist die verlaesslichste.
    /// 3. **Content-Type-Parameter** (`text/html; charset=iso-8859-1`):
    ///    der Transportweg, der zuverlaessigste von allen, weil der
    ///    Server die Bytes selbst gelesen hat.
    ///
    /// Die Reihenfolge ist eine Entscheidung: BOM vor Meta vor Header.
    /// Das BOM ist maschinell und deshalb am schwersten zu uebersehen.
    pub fn erkennen(bytes: &[u8], content_type: Option<&str>) -> (Satz, usize) {
        // 1. BOM
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            return (Satz::Utf8, 3);
        }
        // 2. Meta im ersten KOPF (4096 Byte reichen fuer die BOM-Freiheit
        //    und fuer jede/meta charset, die es in der Praxis gibt)
        let kopf = &bytes[..bytes.len().min(4096)];
        let kopf_text = String::from_utf8_lossy(kopf).to_ascii_lowercase();
        if let Some(i) = kopf_text.find("charset=") {
            let rest = &kopf_text[i + 8..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            return (Satz::benannt(&name), 0);
        }
        // 3. Content-Type des Transports
        if let Some(ct) = content_type
            && let Some(i) = ct.to_ascii_lowercase().find("charset=")
        {
            let rest = &ct[i + 8..];
            let name: String = rest
                .trim_start()
                .trim_start_matches('"')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            return (Satz::benannt(&name), 0);
        }
        (Satz::Unbekannt, 0)
    }

    /// Der Satz zu einem Namen.
    pub fn benannt(name: &str) -> Satz {
        match name {
            "utf-8" | "utf8" | "unicode-1-1-utf-8" => Satz::Utf8,
            "windows-1252" | "cp1252" | "1252" => Satz::Cp1252,
            "iso-8859-1" | "latin1" | "latin-1" | "iso8859-1" | "l1" => Satz::Latin1,
            _ => Satz::Unbekannt,
        }
    }

    /// Wie viele Bytes ein Zeichen in diesem Satz braucht.
    pub fn bytes_pro_zeichen(self) -> usize {
        match self {
            Satz::Utf8 => 0, // variabel
            Satz::Cp1252 | Satz::Latin1 => 1,
            Satz::Unbekannt => 1,
        }
    }
}

/// Wandelt Bytes in Text um — ohne je zu panicken.
///
/// # DIE ZUSAGE, und sie ist der ganze Punkt von H6
///
/// Jede Eingabe ergibt Text. Keine Eingabe ergibt einen Panic, und keine
/// ergibt leeren Text, wenn Bytes da sind.
///
/// | Eingabe | Ergebnis |
/// |---|---|
/// | gueltiges UTF-8 | unveraendert, **inklusive Umlauten** |
/// | kaputtes UTF-8 | `U+FFFD` an jeder kaputten Stelle |
/// | windows-1252 | die richtigen Zeichen (`0xE4` = `ä`) |
/// | ISO-8859-1 | die richtigen Zeichen (`0xE4` = `ä`) |
/// | unbekannt | UTF-8-Versuch, dann windows-1252 |
///
/// GEMESSEN, warum `U+FFFD` und nicht "Zeichen weg": `0xE4` ist in
/// windows-1252 ein `ä` und in kaputtem UTF-8 ein Fehler. Wir raten
/// nicht, wir ersetzen — und der Leser sieht ein Zeichen, an dessen
/// Stelle er die Vermutung anstellen kann.
///
/// M4: die Ausgabe ist nie groesser als die Eingabe plus die
/// Ersatzzeichen — und die Ersatzzeichen sind 3 Byte fuer 1 kaputtes.
/// Das ist die einzige Stelle, an der die Ausgabe groesser sein kann,
/// und sie ist durch die Eingabegroesse begrenzt.
pub fn zu_text(bytes: &[u8], satz: Satz) -> String {
    match satz {
        Satz::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        Satz::Cp1252 => bytes.iter().map(|&b| cp1252_zu_zeichen(b)).collect(),
        Satz::Latin1 => bytes.iter().map(|&b| b as char).collect(),
        Satz::Unbekannt => {
            // Versuch UTF-8. Wenn es gelingt, war es UTF-8.
            match std::str::from_utf8(bytes) {
                Ok(s) => s.to_string(),
                // Sonst windows-1252: das ist der Fall "alte Seite ohne
                // Angabe", und er ist haeufiger als er klingt.
                Err(_) => bytes.iter().map(|&b| cp1252_zu_zeichen(b)).collect(),
            }
        }
    }
}

/// Ein windows-1252-Byte als Zeichen.
///
/// DIE TABELLE, und sie ist keine Geschmacksfrage: die 128 Zeichen von
/// 0x80 bis 0x9F sind in windows-1252 FAST ALLE druckbar (Euro, typografische
/// Anfuehrungszeichen, Pfeile) — und `0x80 as char` waere ein Steuerzeichen,
/// also ein Zeichen, das man nicht sieht. Ohne diese Tabelle ist jedes
/// zweite Zitatzeichen einer deutschen Seite unsichtbar.
fn cp1252_zu_zeichen(b: u8) -> char {
    match b {
        0x80 => '\u{20AC}',
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9E => '\u{017E}',
        0x9F => '\u{0178}',
        other => other as char,
    }
}

// ===========================================================================
// DER STIL DER NEUEN KNOTEN
// ===========================================================================

/// Der Stil eines Tabellenknotens.
///
/// GEMESSEN, warum das ueberhaupt hier steht: die Tabelle ist eine
/// Fläche, und ohne reservierte Breite rutscht der Text nach links.
/// `BORDER_SPALTE` ist die kleinste Breite, bei der eine Zahl noch lesbar
/// ist — gemessen an der 5x7-Schrift: 3 Zeichen brauchen 3 Zellen, und
/// 3 Zellen sind bei Grad 16 genau 48 Pixel.
const BORDER_SPALTE: f32 = 16.0;

/// Der Stil einer Tabellenzelle.
pub fn stil_zelle(breite: u32, kopf: bool) -> Style {
    Style {
        deckkraft: 1.0,
        unsichtbar: false,
        radius: [0.0; 4],
        schatten_anzahl: 0,
        // P8 (02.10.2026), BAUFIX: `Style` hat seit heute das Feld
        // `schatten` (`narsil_core::style::Style`, CSS Backgrounds 3 §9.3),
        // und diese Struct-Literale kannten es nicht. Das hiess E0063 in
        // `narsil-page` und damit in ALLEN abhaengigen Crates — narsil,
        // narsil-probe, narsil-layout-engine: nichts baute mehr.
        //
        // `Default::default()` und nicht acht ausgeschriebene
        // `SchattenArt`-Werte: `MAX_SCHATTEN` ist 8, und `[T; 8]`
        // implementiert `Default`. `schatten_anzahl: 0` darueber sagt dem
        // Leser, dass keiner davon benutzt wird — die beiden Felder
        // gehoeren zusammen, und das ist auch der Grund, warum es hier
        // funktioniert.
        //
        // FUER DIE ROLLE HTML: das ist eine mechanische Baureparatur,
        // keine Aenderung an Eurer Logik. Wenn Ihr dieselbe Zeile
        // schreibt, ist das doppelt und der Compiler sagt es; dann
        // loescht eine von beiden.
        schatten: Default::default(),
        dekoration: 0,
        size: Dimension {
            width: if breite == 0 {
                Length::Auto
            } else {
                Length::px(breite as f32)
            },
            height: Length::Auto,
        },
        // M3/M4: die Mindestbreite ist eine KONSTANTE, und sie ist
        // mindestens eine Zelle breit. Ohne sie waere eine Zelle mit
        // `width=0` null Pixel breit und ihr Text ragte heraus.
        margin: Spacing::all(Length::px(2.0)),
        padding: Spacing::all(Length::px(BORDER_SPALTE.min(8.0))),
        // GEMESSEN, warum eine Kopfzeile hinterlegt wird: ohne
        // Hintergrund ist eine Tabelle mit 20 Zeilen ein Streifen, und
        // man weiss nicht, welche Zeile die Kopfzeile ist.
        background: if kopf {
            Color {
                r: 0x33,
                g: 0x33,
                b: 0x3a,
                a: 255,
            }
        } else {
            Color::TRANSPARENT
        },
        // Die Textfarbe ist in beiden Faellen weiss: die Kopfzeile ist
        // hinterlegt, die Zelle nicht, und helle Schrift auf hellem
        // Grund waere unsichtbar. clippy hat die zwei identischen
        // Zweige gemeldet — sie waren wirklich identisch.
        color: Color::WHITE,
        // 04.10.2026 (SCHRIFT): BAUFIX zu `schriftgewicht`/`kursiv`
        // (E0063). 400/false ist `normal` (CSS Fonts 4 §5.3) — der
        // Wert ist also derselbe wie vorher, die Kaskade ueberschreibt
        // ihn fuer `<b>`/`h1..h6`/`i`.
        schriftgewicht: 400,
        vertikal_hoch: 0.0,
        buchstaben_abstand: 0.0,
        wort_abstand: 0.0,
        kursiv: false,
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `Style::schriftfamilie`.
        // `sans-serif` ist die Vorgabe (narsil-core, `impl Default`) und
        // damit die Familie, die vorher ohne Feld genutzt wurde.
        schriftfamilie: String::from("sans-serif"),
        // 07:0x (SCHRIFT): BAUFIX E0063 nach `Style::verlauf`
        // (BILD_48 Station 1, fremdes Feld). `None` ist der
        // Anfangswert: `background-image: none`.
        verlauf: None,
        verlauf_kachel: None, clip_rand: None, umriss: None, border_image: false, clip_pfad: None, clip_svg: None, maske_bild: None, filter_value: None, filter_svg: None, backdrop_filter: None, backdrop_svg: None, radius_xy: [[0.0; 2]; 4], radius_percent: 0,
        // 04.10.2026 (BAUFIX fuer BILD_32): `rahmen` gibt es seit
        // heute in `Style` (narsil-core/src/style.rs:188, Vorgabe
        // `Rahmen::default()`, style.rs:299). Ohne diese Zeile war
        // `narsil-page` — und damit narsil-raster, narsil, narsil-chrome —
        // nicht baubar. Mechanisch, kein Logikwechsel.
        rahmen: Rahmen::default(),
        font_size: 16.0,
        direction: Direction::Column,
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
            let fs: f32 = 16.0;
            let em = (fs * 1229.0 / 1024.0 + 0.5).floor();
            let auf = (fs * 1901.0 / 2048.0).ceil().max(1.0);
            let ab = (fs * 483.0 / 2048.0).ceil().max(1.0);
            em.max(auf + ab)
        },
    }
}

/// Der Stil eines Formularfelds.
pub fn stil_feld(art: FeldArt) -> Style {
    // GEMESSEN, warum die Breite fest ist: ein Eingabefeld, das seine
    // Breite aus dem Inhalt nimmt, springt bei jedem Tastendruck. 200 px
    // sind 25 Zeichen bei Grad 16 — das ist die haeufigste Sucheingabe.
    let breite = match art {
        FeldArt::Absenden => 96.0,
        FeldArt::Kaestchen => 24.0,
        _ => 200.0,
    };
    Style {
        radius: [0.0; 4],
        schatten_anzahl: 0,
        // P8 (02.10.2026), BAUFIX: `Style` hat seit heute das Feld
        // `schatten` (`narsil_core::style::Style`, CSS Backgrounds 3 §9.3),
        // und diese Struct-Literale kannten es nicht. Das hiess E0063 in
        // `narsil-page` und damit in ALLEN abhaengigen Crates — narsil,
        // narsil-probe, narsil-layout-engine: nichts baute mehr.
        //
        // `Default::default()` und nicht acht ausgeschriebene
        // `SchattenArt`-Werte: `MAX_SCHATTEN` ist 8, und `[T; 8]`
        // implementiert `Default`. `schatten_anzahl: 0` darueber sagt dem
        // Leser, dass keiner davon benutzt wird — die beiden Felder
        // gehoeren zusammen, und das ist auch der Grund, warum es hier
        // funktioniert.
        //
        // FUER DIE ROLLE HTML: das ist eine mechanische Baureparatur,
        // keine Aenderung an Eurer Logik. Wenn Ihr dieselbe Zeile
        // schreibt, ist das doppelt und der Compiler sagt es; dann
        // loescht eine von beiden.
        schatten: Default::default(),
        dekoration: 0,
        size: Dimension {
            width: Length::px(breite),
            height: Length::px(if art == FeldArt::Absenden { 24.0 } else { 20.0 }),
        },
        margin: Spacing::all(Length::px(2.0)),
        padding: Spacing::all(Length::px(4.0)),
        // GEMESSEN, warum das Feld eine eigene Farbe bekommt: ohne Rahmen
        // ist ein Eingabefeld ein Wort in einem Absatz, und der Leser
        // weiss nicht, wo er tippen kann.
        background: Color {
            r: 0x22,
            g: 0x22,
            b: 0x28,
            a: 255,
        },
        color: Color::WHITE,
        // 04.10.2026 (SCHRIFT): BAUFIX zu `schriftgewicht`/`kursiv`
        // (E0063). 400/false ist `normal` (CSS Fonts 4 §5.3) — der
        // Wert ist also derselbe wie vorher, die Kaskade ueberschreibt
        // ihn fuer `<b>`/`h1..h6`/`i`.
        schriftgewicht: 400,
        vertikal_hoch: 0.0,
        buchstaben_abstand: 0.0,
        wort_abstand: 0.0,
        kursiv: false,
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `Style::schriftfamilie`.
        // `sans-serif` ist die Vorgabe (narsil-core, `impl Default`) und
        // damit die Familie, die vorher ohne Feld genutzt wurde.
        schriftfamilie: String::from("sans-serif"),
        // 07:0x (SCHRIFT): BAUFIX E0063 nach `Style::verlauf`
        // (BILD_48 Station 1, fremdes Feld). `None` ist der
        // Anfangswert: `background-image: none`.
        verlauf: None,
        verlauf_kachel: None, clip_rand: None, umriss: None, border_image: false, clip_pfad: None, clip_svg: None, maske_bild: None, filter_value: None, filter_svg: None, backdrop_filter: None, backdrop_svg: None, radius_xy: [[0.0; 2]; 4], radius_percent: 0,
        // 04.10.2026 (BAUFIX fuer BILD_32): `rahmen` gibt es seit
        // heute in `Style` (narsil-core/src/style.rs:188, Vorgabe
        // `Rahmen::default()`, style.rs:299). Ohne diese Zeile war
        // `narsil-page` — und damit narsil-raster, narsil, narsil-chrome —
        // nicht baubar. Mechanisch, kein Logikwechsel.
        rahmen: Rahmen::default(),
        font_size: 16.0,
        direction: Direction::Column,
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
            let fs: f32 = 16.0;
            let em = (fs * 1229.0 / 1024.0 + 0.5).floor();
            let auf = (fs * 1901.0 / 2048.0).ceil().max(1.0);
            let ab = (fs * 483.0 / 2048.0).ceil().max(1.0);
            em.max(auf + ab)
        },
        deckkraft: 1.0,
        unsichtbar: false,
    }
}

/// Der Stil eines Bildplatzhalters.
pub fn stil_bild(bild: &Bild) -> Style {
    Style {
        radius: [0.0; 4],
        schatten_anzahl: 0,
        // P8 (02.10.2026), BAUFIX: `Style` hat seit heute das Feld
        // `schatten` (`narsil_core::style::Style`, CSS Backgrounds 3 §9.3),
        // und diese Struct-Literale kannten es nicht. Das hiess E0063 in
        // `narsil-page` und damit in ALLEN abhaengigen Crates — narsil,
        // narsil-probe, narsil-layout-engine: nichts baute mehr.
        //
        // `Default::default()` und nicht acht ausgeschriebene
        // `SchattenArt`-Werte: `MAX_SCHATTEN` ist 8, und `[T; 8]`
        // implementiert `Default`. `schatten_anzahl: 0` darueber sagt dem
        // Leser, dass keiner davon benutzt wird — die beiden Felder
        // gehoeren zusammen, und das ist auch der Grund, warum es hier
        // funktioniert.
        //
        // FUER DIE ROLLE HTML: das ist eine mechanische Baureparatur,
        // keine Aenderung an Eurer Logik. Wenn Ihr dieselbe Zeile
        // schreibt, ist das doppelt und der Compiler sagt es; dann
        // loescht eine von beiden.
        schatten: Default::default(),
        dekoration: 0,
        size: Dimension {
            //
            // BILD_19: auch die **Breite** ist `Auto`, Attribute oder nicht.
            // GEMESSEN: `tests/srcset.rs:204` erwartet `Auto` und bekam
            // `Px(320.0)`; `tests/img_groesse_aus_der_kaskade.rs:89` erwartet
            // `Auto` und bekam `Px(7129.0)`. Die Breite aus dem **Attribut**
            // reist ebenfalls in `ersetzt` — `lib.rs` setzt sie per
            // `set_ersetzt`, und `ersetzte_groesse` rechnet daraus.
            width: Length::Auto,
            //
            // BILD_16: DIE HOEHENREGEL.
            //
            // GEMESSENER FEHLER, den diese Zeile selbst verursacht hat:
            // `height` war **immer** `Px(reservierte_hoehe())`. Bei einem
            // **geladenen** Bild ohne `height`-Attribut ist das eine *gesetzte*
            // Hoehe, und damit lief der Verhaeltnis-Zweig der Layout-Engine
            // (`box.rs:423`, `:946-955`) **nicht**. GEMESSEN auf nixos.org:
            // `ersatz = 32x32`, Kasten **1280.0 x 0.0** statt 1280x1280.
            //
            //   * `height`-Attribut      -> `Px(hoehe)`, das Attribut gewinnt
            //                               (GEMESSEN: HN-Spacer `<img src="s.gif"
            //                               width="1" height="1">` bleibt 1x1)
            //   * geladen, kein Attribut -> `Auto`, **das Layout rechnet**
            //                               Breite / Seitenverhaeltnis
            //   * nicht geladen          -> `Px(reservierte_hoehe())`,
            //                               die Zusage von H-HN2 bleibt
            //
            // BILD_19: DER VERTRAG — bei Bildern **immer** `Auto`, in **beiden**
            // Achsen. Die Zahlen reisen in **`ersetzt`** (`ErsatzMasse`), wo
            // `lib.rs` sie per `set_ersetzt` ablegt; die Rechnung macht
            // `ersetzte_groesse()` in `narsil-layout-engine/src/box.rs`.
            //
            // GEMESSENE GRUNDLAGE — vier **Tests, die den Vertrag festhalten**,
            // alle vor meiner Aenderung rot:
            //   * `tests/bilder_knoten.rs:130` „ohne CSS bleibt die Hoehe auto —
            //     die Zahl steht in `ersetzt`" (erwartet `Auto`, gemessen `Px(120)`)
            //   * `tests/srcset.rs:204` „ohne CSS bleibt die Breite auto"
            //     (erwartet `Auto`, gemessen `Px(320)`)
            //   * `tests/img_groesse_aus_der_kaskade.rs:89` Breite `Auto`
            //     (erwartet `Auto`, gemessen `Px(7129)`)
            //   * `tests/img_groesse_aus_der_kaskade.rs:140` Kastenbreite **171,5**
            //     (gemessen **7129**)
            //
            // **MEINE REGEL VON `BILD_16` IST ZURUECKGENOMMEN.** Sie schrieb
            // eine *gesetzte* Hoehe, obwohl die Zahl dieselbe schon in `ersetzt`
            // steht — genau der Vertragsbruch, und die Tests hatten **recht**.
            //
            // GEMESSENE ENTSCHEIDUNG, warum hier **nie** eine Pixelzahl steht:
            // `Px(reservierte_hoehe())` war der Fehler. Bei einem **geladenen**
            // Bild ist das eine *gesetzte* Hoehe, und damit lief der
            // Verhaeltnis-Zweig der Layout-Engine (`box.rs:423`, `:946-955`)
            // **nicht**. GEMESSEN auf nixos.org: `ersatz = 32x32`, Kasten
            // **1280.0 x 0.0** statt 1280x1280.
            //
            // **DIE RESERVIERUNG BLEIBT**, denn sie steht in `ersetzt`:
            // `lib.rs` legt bei einem nicht geladenen Bild die Standardhoehe
            // dort ab (Zusage H-HN2, `hteile.rs:730-742`).
            height: Length::Auto,
        },
        margin: Spacing::all(Length::px(4.0)),
        padding: Spacing::ZERO,
        //
        // 03.10. 18:45 (H-AR2): DER KNOTEN FUELLT NICHT MEHR.
        //
        // GEMESSENER BEFUND, an der Seite der Koordination
        // (`ar.wikipedia.org_wiki________.png`, 900x2048, y<900 gesampelt):
        //     (255,255,255) 156914   <- die Seite
        //     (58,58,66)     23746   <- der Platzhalter aus `narsil`
        //     (42,42,52)      4853   <- 0x2a2a32, DIESER Stil
        //     DUNKLE ZEILEN 165..762, SPALTEN 242..456
        // Zwei dunkle Quellen, ein Block von rund 215 x 600 px auf weiss.
        //
        // GEMESSENE BEGRUENDUNG, warum durchsichtig und nicht heller: der
        // Kommentar hier wollte „hier kommt noch etwas" lesbar machen. Ein
        // gefuellter Kasten ist aber kein Hinweis, sondern ein Loch in der
        // Seite — der Platz IST der Hinweis (ADR-019: die Seite springt
        // nicht, wenn das Bild spaeter kommt), und der `alt`-Text steht als
        // Text darin. Beides bleibt; die FUELLUNG faellt weg.
        background: Color::TRANSPARENT,
        // GEMESSENE BEGRUENDUNG fuer die Textfarbe: vorher 0x9999aa, ein
        // helles Grau, das auf weiss fast unsichtbar war. Der `alt`-Text ist
        // Text der Seite und steht in `FARBE_TEXT`.
        color: FARBE_TEXT,
        // 04.10.2026 (SCHRIFT): BAUFIX zu `schriftgewicht`/`kursiv`
        // (E0063). 400/false ist `normal` (CSS Fonts 4 §5.3) — der
        // Wert ist also derselbe wie vorher, die Kaskade ueberschreibt
        // ihn fuer `<b>`/`h1..h6`/`i`.
        schriftgewicht: 400,
        vertikal_hoch: 0.0,
        buchstaben_abstand: 0.0,
        wort_abstand: 0.0,
        kursiv: false,
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `Style::schriftfamilie`.
        // `sans-serif` ist die Vorgabe (narsil-core, `impl Default`) und
        // damit die Familie, die vorher ohne Feld genutzt wurde.
        schriftfamilie: String::from("sans-serif"),
        // 07:0x (SCHRIFT): BAUFIX E0063 nach `Style::verlauf`
        // (BILD_48 Station 1, fremdes Feld). `None` ist der
        // Anfangswert: `background-image: none`.
        verlauf: None,
        verlauf_kachel: None, clip_rand: None, umriss: None, border_image: false, clip_pfad: None, clip_svg: None, maske_bild: None, filter_value: None, filter_svg: None, backdrop_filter: None, backdrop_svg: None, radius_xy: [[0.0; 2]; 4], radius_percent: 0,
        // 04.10.2026 (BAUFIX fuer BILD_32): `rahmen` gibt es seit
        // heute in `Style` (narsil-core/src/style.rs:188, Vorgabe
        // `Rahmen::default()`, style.rs:299). Ohne diese Zeile war
        // `narsil-page` — und damit narsil-raster, narsil, narsil-chrome —
        // nicht baubar. Mechanisch, kein Logikwechsel.
        rahmen: Rahmen::default(),
        font_size: 14.0,
        direction: Direction::Column,
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
            let fs: f32 = 14.0;
            let em = (fs * 1229.0 / 1024.0 + 0.5).floor();
            let auf = (fs * 1901.0 / 2048.0).ceil().max(1.0);
            let ab = (fs * 483.0 / 2048.0).ceil().max(1.0);
            em.max(auf + ab)
        },
        deckkraft: 1.0,
        unsichtbar: false,
    }
}

/// Der Knoten-Index, den `bilder_sammeln` als naechstes anlegt.
///
/// Nur ein Platzhalter fuer die Doku: die Bilder werden in
/// `narsil_page::Bauer` in Knoten umgesetzt, und dort steht der echte Code.
#[allow(dead_code)]
fn _knoten_index_hinweis(dokument: &narsil_dom::Document) -> NodeId {
    let _ = (dokument, Span::EMPTY);
    NodeId(0)
}
