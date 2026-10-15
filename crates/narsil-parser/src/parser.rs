//! Recursive-Descent-Parser der Narsil-Sprache: Tokens -> Document-Baum.
//!
//! Die Signatur von [`parse`] ist von Agent1 festgelegt und FROZEN.
//! TASK_0008 hat nur den Koerper gefuellt.
//!
//! Sprachspezifikation: docs/NARSIL_LANGUAGE_SPEC_v0.txt, Abschnitt 4-8.

use crate::ParseError;
use crate::lexer::LexError;
use crate::token::{Token, TokenKind};
use narsil_core::color::Color;
use narsil_core::length::{Length, Spacing};
use narsil_core::style::{DEFAULT_FONT_SIZE, Direction, Style};
use narsil_dom::{Document, NodeId, NodeKind, Span};

/// Parst ein vollstaendiges Narsil-Dokument.
///
/// Der Einstiegspunkt der Sprache: Quelltext -> `narsil_dom::Document`.
///
/// Fehler:
///  * Lexerfehler werden uebernommen und als `ParseError` weitergereicht.
///  * Syntaxfehler liefern `ParseError` mit `span`.
///  * Es gibt KEINEN Fehlerpfad, der panict.
pub fn parse(source: &str) -> Result<Document, ParseError> {
    let tokens = crate::lexer::tokenize(source)?;
    parse_tokens(&tokens)
}

/// Parst eine bereits lexierte Token-Liste zu einem Dokument.
///
/// Diese Stufe ist von [`parse`] getrennt, damit der Parser unabhaengig
/// vom Lexer getestet werden kann: Der Lexer entsteht in TASK_0007
/// parallel zu diesem Task. Fuer Aufrufer ist [`parse`] der normale Weg.
///
/// Erwartet wird eine vollstaendige Token-Liste mit abschliessendem
/// `Eof`-Token — genau das liefert `tokenize`.
pub fn parse_tokens(tokens: &[Token]) -> Result<Document, ParseError> {
    let mut parser = Parser::new(tokens);
    let document = parser.parse_document()?;
    debug_assert!(parser.at_eof(), "Restliche Tokens nach dem Root");
    Ok(document)
}

/// Zustand des Recursive-Descent-Parsers.
///
/// Arbeitet auf einer vorgespeicherten Token-Liste mit einem
/// Positionszeiger. Bewusst kein Index-`Vec<Token>`-Klon pro Aufruf: der
/// Parser nimmt die Referenz und enthaelt die Tokens nicht.
struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

/// Properties, die `page` annehmen darf.
///
/// Spezifikation E17 nennt `color` als DAS Beispiel fuer eine Property,
/// die an `page` "akzeptiert und ignoriert" wird, und `direction`
/// ausdruecklich als erlaubt. Die Liste ist deshalb die volle Property-Menge
/// aus Abschnitt 6 MINUS `color` und `font-size`.
///
/// Warum nicht nur `width`/`height` (wie die erste Fassung dieses
/// Kommentars behauptete): das widersprache `examples/hello.narsil` nicht
/// direkt, aber dem Beispiel 7b der Spezifikation, das `background` an
/// `page` setzt. Der Abschnitt 6 listet die Properties ohne
/// Knotenart-Einschraenkung auf.
///
/// Warum `color` und `font-size` trotzdem herausfallen: E17 sagt
/// "ignoriert", und ignoriert heisst, der Wert steht NICHT im Baum. Das
/// ist die einzige Stelle der Sprache, an der "ignoriert" etwas
/// anderes meint als "geparst".
///
/// DIE ACHT LANGFORMEN (TASK_0029) sind hier die Langform von `margin`
/// und `padding` — dieselben Properties, die schon drin stehen. Wer
/// `margin: 20` auf `page` schreiben darf, muss auch `margin-top: 20`
/// schreiben duerfen; sonst waere die Langform an `page` verboten und an
/// `container` erlaubt, ohne dass die Sprache einen Grund dafuer hat.
const PAGE_PROPERTIES: &[&str] = &[
    "width",
    "height",
    "margin",
    "padding",
    "background",
    "direction",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
];

/// Properties, die `container` annehmen darf.
///
/// Spezifikation Abschnitt 5: "Definiert margin, padding, background,
/// direction", plus `width`/`height` aus Beispiel 7c.
///
/// `color` und `font-size` SIND erlaubt und werden gespeichert —
/// woertlich, Abschnitt 6: "color und font-size an einem container sind
/// erlaubt, werden in v0 aber noch nicht vererbt. Sie werden geparst und
/// gespeichert." Der einzige Unterschied zu `page` ist die Vererbung, und
/// die ist Sache der Layout-Engine, nicht des Parsers.
///
/// (Eine erste Fassung dieses Kommentars hat sie auch hier verworfen. Das
/// war ein Fehler: die Spec sagt "gespeichert".)
const CONTAINER_PROPERTIES: &[&str] = &[
    "width",
    "height",
    "margin",
    "padding",
    "background",
    "color",
    "font-size",
    "direction",
    // Langformen (TASK_0029). Dieselben acht wie bei ALL_PROPERTIES —
    // dort steht, warum sie nicht zweimal getippt werden.
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
];

/// Properties, die `text` annehmen darf.
///
/// Spezifikation Abschnitt 5, nach ADR-029 (Agent1, 05:24):
///
/// ```text
/// margin  AUF text: ERLAUBT.   padding AUF text: VERBOTEN.
/// ```
/// `margin` verschiebt die Box gegenueber der Umgebung, und das sieht man
/// am Bild. `padding` verschiebt den Inhalt in einen Innenraum — und ein
/// Textknoten hat keinen: keine Hintergrundfarbe, keinen Rand. `padding`
/// wuerde also einen Wert annehmen, pruefen und dann nichts tun. Genau das
/// ist der Fehlerfall, den wir die ganze Nacht bekämpfen (ADR-019).
///
/// `padding` steht hier NICHT, und das heisst "wird ignoriert" (E17) und
/// nicht "unbekannt": der Wert wird gelesen, geprueft und verworfen.
/// `padding: 20` an einem `text` ist damit gueltige Sprache, die
/// nichts tut — genau wie `color` an einem `page`.
///
/// GEMESSEN, warum das kein Zufall ist: `text "A" { margin: 20; }`
/// ergibt vor dieser Aenderung `Px(0.0)`, weil der Wert hier verworfen
/// wurde. Mit der Aenderung steht der Text bei (20, 20) statt bei (0, 0).
const TEXT_PROPERTIES: &[&str] = &[
    "color",
    "font-size",
    "margin",
    // DIE LANGFORMEN (BEFUND, an Agent1 06:12, nicht entschieden von mir).
    //
    // GEMESSEN, direkt nach dem Oeffnen von `margin`: `text "A" {
    // margin-top: 20; }` war "unbekannte Property", waehrend `margin:
    // 20;` angenommen wurde. Das ist dieselbe Property in zwei Formen,
    // und die eine geht, die andere nicht.
    //
    // Die Spezifikation (Abschnitt 5, Zeile 148) schreibt fuer `text`
    // "Properties: color, font-size, margin" — sie kennt die Kurzform
    // und schweigt ueber die acht Langformen. Sie ist damit unvollstaendig,
    // und die Frage "gehoeren die Langformen dazu?" ist offen.
    //
    // WARUM ICH SIE TROTZDEM AUFNEHME: die Begruendung von ADR-029 gilt
    // unabhaengig von der Form. `margin` verschiebt die Box, `margin-top`
    // verschiebt dieselbe Box nach oben. Wenn `margin` sichtbar ist,
    // dann ist es `margin-top` auch, und die Kurzform vorzuziehen heisst:
    // derselbe Effekt ist je nach Schreibweise erlaubt oder verboten. Das
    // ist genau die Inkonsistenz, die in TASK_0024 bei `page` aufgetreten
    // ist und dort behoben wurde.
    //
    // DER GEGENTEIL WAERE VERTRETBAR und ist eine echte Entscheidung: man
    // koennte sagen, `text` kennt nur die Kurzform, weil ein Textknoten
    // keinen Innenraum und keine Kanten hat. Das ist aber eine Formregel,
    // keine Sachregel, und sie muesste in der Spec stehen. Wenn Agent1 sie
    // anders entscheidet, ist das eine Zeile hier.
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
];

/// Alle bekannten Property-Namen, unabhängig von der Knotenart.
///
/// Nur fuer die Frage "ist das ueberhaupt eine Property?" (E5). Die
/// Frage "darf sie hier?" stellt die Liste der jeweiligen Knotenart.
///
/// DIE ACHT LANGFORMEN (TASK_0029) stehen hier und in den drei
/// Knotenart-Listen. Viermal getippt ist kein Komfort, es ist eine
/// Fehlerquelle: `margin-top` in `ALL_PROPERTIES` aber nicht in
/// `CONTAINER_PROPERTIES` ergibt "unbekannte Property" an
/// `container` und "bekannt, aber ignoriert" an `page` — zwei
/// verschiedene Fehler fuer eine Sache.
///
/// WARUM SIE NICHT AUS `SEITEN_PROPERTY` GEZOGEN WERDEN: die Tabelle
/// enthaelt `(name, Option<Seite>)`, und `contains(&name)` braucht
/// `&[&str]`. Die Namen einmal extrahieren waere moeglich, macht die
/// Listen aber unlesbar — und die Listen werden bei jedem Lesen
/// verglichen. Sie sind hier explizit, und der Test
/// `alle_property_namen_stehen_in_genau_einer_quelle` in
/// `tests/langformen.rs` haelt sie zusammen.
const ALL_PROPERTIES: &[&str] = &[
    "width",
    "height",
    "margin",
    "padding",
    "background",
    "color",
    "font-size",
    "direction",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
];

/// Property-Namen, fuer die `auto` erlaubt ist.
const AUTO_ALLOWED: &[&str] = &["width", "height"];

/// Die acht Langformen von TASK_0029, und welche Seite jede setzt.
///
/// `None` heisst: diese Property gehoert nicht zu `margin`/`padding`.
///
/// WARUM EINE TABELLE und nicht acht `if`-Zweige: die Langformen und die
/// Kurzformen muessen an DIESELBEN Stellen geprueft werden — die
/// Knotenart-Listen, `ALL_PROPERTIES` und die Zuordnung "welche Seite".
/// Drei Listen, die auseinanderlaufen, sind der Fehler, den ADR-021
/// ueber `DisplayList::bounds()` beschreibt. Eine Tabelle kann daran
/// nicht auseinanderlaufen, weil es nur eine gibt.
const SEITEN_PROPERTY: &[(&str, Option<Seite>)] = &[
    ("margin", None),
    ("padding", None),
    ("margin-top", Some(Seite::Top)),
    ("margin-right", Some(Seite::Right)),
    ("margin-bottom", Some(Seite::Bottom)),
    ("margin-left", Some(Seite::Left)),
    ("padding-top", Some(Seite::Top)),
    ("padding-right", Some(Seite::Right)),
    ("padding-bottom", Some(Seite::Bottom)),
    ("padding-left", Some(Seite::Left)),
];

/// Die vier Seiten einer Box, in der Reihenfolge der Kurzform.
///
/// DIE REIHENFOLGE IST VERBINDLICH. `margin: 10 20 30 40` bedeutet oben
/// 10, rechts 20, unten 30, links 40 — das ist im Uhrzeigersinn, wie in
/// CSS. `margin: 10 20 30` bedeutet oben 10, links/rechts 20, unten 30.
///
/// `Top` steht hier ZUERST, weil es in der Kurzform zuerst kommt. Das
/// ist keine Geschmacksfrage: wer die Liste von `left` beginnt und in
/// `margin: 10 20` beide Werte vertauscht, baut eine Sprache, in der
/// oben und links gleich heissen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seite {
    Top,
    Right,
    Bottom,
    Left,
}

impl Seite {
    /// Setzt diese Seite in `spacing` und laesst die anderen unberuehrt.
    ///
    /// DAS ist der Unterschied zwischen Lang- und Kurzform: eine
    /// Langform setzt EIN Feld, eine Kurzform setzt alle vier. Beide
    /// benutzen dieselbe `Spacing`-Struktur, weil `Spacing` vier
    /// unabhaengige Felder hat (`narsil-core/src/length.rs:137`).
    const fn setze(self, spacing: &mut Spacing, value: Length) {
        match self {
            Seite::Top => spacing.top = value,
            Seite::Right => spacing.right = value,
            Seite::Bottom => spacing.bottom = value,
            Seite::Left => spacing.left = value,
        }
    }
}

/// `"margin"` oder `"padding"` — die Property, aus der ein Name kommt.
///
/// Die EINZIGE Stelle, die entscheidet, wohin ein Name gehoert. Der
/// Vergleich `name.starts_with("padding")` steht an drei Stellen im Code;
/// wenn die drei auseinanderlaufen, schreibt `padding-top` nach `margin`
/// und niemand merkt es, weil die Werte plausibel aussehen.
fn gruppe_von(name: &str) -> &'static str {
    if name.starts_with("padding") {
        "padding"
    } else {
        "margin"
    }
}

/// Der Wert EINER Seite aus einer fertigen `Spacing`.
///
/// Wird gebraucht, weil die Kurzform seitenweise geschrieben wird (eine
/// bereits per Langform gesetzte Seite darf nicht ueberschrieben werden).
/// Statt die ganze `Spacing` zu ersetzen, geht jede Seite einzeln.
fn aus_kurzform_seite(spacing: &Spacing, seite: Seite) -> Length {
    match seite {
        Seite::Top => spacing.top,
        Seite::Right => spacing.right,
        Seite::Bottom => spacing.bottom,
        Seite::Left => spacing.left,
    }
}

/// Die Kurzform: ein, zwei, drei oder vier Werte auf vier Seiten.
///
/// DIE REGEL, und sie ist in CSS uebernommen, weil sie dort seit 1996
/// steht und jeder sie kennt:
///
/// ```text
/// 1 Wert   10           -> oben, rechts, unten, links = 10
/// 2 Werte  10 20        -> oben = unten = 10, rechts = links = 20
/// 3 Werte  10 20 30     -> oben = 10, rechts = links = 20, unten = 30
/// 4 Werte  10 20 30 40  -> oben, rechts, unten, links
/// ```
///
/// Merksatz: die Werte laufen im Uhrzeigersinn und werden von aussen
/// nach innen paarweise gleich gesetzt. Zwei Werte heissen oben/unten
/// und links/rechts, NICHT oben/links.
///
/// `None` bei null oder fuenf und mehr Werten: `margin: 1 2 3 4 5` ist
/// ein ParseError, kein fuenfter Standardwert. Eine Sprache, die fuenf
/// Werte annimmt und den fuenften ignoriert, sagt dem Benutzer nicht,
/// dass er sich geirrt hat.
fn aus_kurzform(werte: &[Length]) -> Option<Spacing> {
    match werte {
        [a] => Some(Spacing::all(*a)),
        [vertikal, horizontal] => Some(Spacing {
            top: *vertikal,
            bottom: *vertikal,
            left: *horizontal,
            right: *horizontal,
        }),
        [oben, seitlich, unten] => Some(Spacing {
            top: *oben,
            left: *seitlich,
            right: *seitlich,
            bottom: *unten,
        }),
        [oben, rechts, unten, links] => Some(Spacing {
            top: *oben,
            right: *rechts,
            bottom: *unten,
            left: *links,
        }),
        _ => None,
    }
}

/// Die Seite, die eine Langform setzt, oder `None` fuer Kurzformen.
///
/// Die Liste `SEITEN_PROPERTY` ist die EINZIGE Quelle. Property-Listen,
/// `ALL_PROPERTIES` und diese Funktion muessen nicht zusammenpassen —
/// sie koennen gar nicht auseinanderlaufen, weil sie dieselbe Tabelle
/// benutzen.
fn langform_seite(name: &str) -> Option<Seite> {
    SEITEN_PROPERTY
        .iter()
        .find(|(p, _)| *p == name)
        .and_then(|(_, s)| *s)
}

/// `true` fuer `margin` und `padding` — die beiden Kurzformen.
fn ist_kurzform(name: &str) -> bool {
    name == "margin" || name == "padding"
}

/// Welche Seiten in diesem Block per LANGFORM gesetzt wurden.
///
/// Agent1, TASK_0029: "Langform schlaegt Kurzform im selben Block, egal in
/// welcher Reihenfolge sie steht. Grund: sie ist laenger und damit
/// eindeutiger; wer sie schreibt, meint sie."
///
/// GEMESSEN, warum das nicht automatisch stimmt: bevor diese Regel da war,
/// hing es an der Reihenfolge.
///
/// ```text
/// margin: 10 20 30 40; margin-top: 99;   ->  top = 99   (richtig)
/// margin-top: 99; margin: 10 20 30 40;   ->  top = 10   (falsch)
/// ```
///
/// ZWEI FELDER, nicht eins: `margin` und `padding` sind getrennte
/// Properties mit getrennten Langformen. `margin-top: 99` und
/// `padding-top: 5` duerfen sich nicht gegenseitig beeinflussen, und ein
/// einziges Boolean je Seite koennte das nicht ausdruecken.
///
/// `None` heisst "von keiner Langform gesetzt". Ein `Some(quelle)` heisst
/// "diese Seite hat Vorrag, egal was die Kurzform danach sagt".
#[derive(Debug, Clone, Copy, Default)]
struct LangformSeiten {
    margin: [bool; 4],
    padding: [bool; 4],
}

impl LangformSeiten {
    /// Der Index einer Seite in der Reihenfolge der Kurzform:
    /// 0 = oben, 1 = rechts, 2 = unten, 3 = links.
    const fn index(seite: Seite) -> usize {
        match seite {
            Seite::Top => 0,
            Seite::Right => 1,
            Seite::Bottom => 2,
            Seite::Left => 3,
        }
    }

    /// Vermerkt, dass `seite` per Langform gesetzt wurde.
    ///
    /// `bool` und nicht der Name der Property: der Name wird fuer die
    /// Fehlermeldung nicht gebraucht (die Langform selbst meldet ihre
    /// eigenen Fehler), und ein `&'static str` haette hier eine
    /// Lebensdauer-Frage aufgeworfen, die nichts loest. GEMESSEN: der
    /// erste Versuch mit `Option<&'static str>` brauchte `'static` in
    /// einer Methode, die nur `&str` kennt.
    fn setze(&mut self, gruppe: &str, seite: Seite) {
        let i = Self::index(seite);
        let ziel = if gruppe == "padding" {
            &mut self.padding
        } else {
            &mut self.margin
        };
        ziel[i] = true;
    }

    /// `true`, wenn diese Seite per Langform gesetzt wurde.
    fn ist_gesetzt(&self, gruppe: &str, seite: Seite) -> bool {
        let i = Self::index(seite);
        if gruppe == "padding" {
            self.padding[i]
        } else {
            self.margin[i]
        }
    }
}

/// Der `Spacing`-Slot, den eine Langform oder Kurzform schreibt.
///
/// `margin-*` schreibt `style.margin`, `padding-*` `style.padding`.
/// Beide Langformen und beide Kurzformen teilen sich diesen Zugriff,
/// damit es nicht zwei Wege zum selben Feld gibt.
///
/// Der Name entscheidet: alles, was mit `padding` beginnt, geht in
/// `padding`, alles andere in `margin`. Das ist eine
/// Praefixentscheidung, und sie ist in `SEITEN_PROPERTY` nachlesbar —
/// dort stehen genau die Namen, die hier durchkommen.
fn spacing_von<'a>(style: &'a mut Style, name: &str) -> &'a mut Spacing {
    if gruppe_von(name) == "padding" {
        &mut style.padding
    } else {
        &mut style.margin
    }
}

/// Groesste erlaubte Schriftgroesse in Pixeln.
///
/// TASK_0024: `font-size: 99999` war ein gueltiger Parse. Erst der
/// Rasterizer machte daraus eine Breite von 0, und der Text verschwand
/// ersatzlos aus dem Bild — ohne Fehlermeldung (ADR-019 an einer zweiten
/// Stelle). Die Grenze gehoert an die SPRACHE, und die Sprache ist dieser
/// Parser.
///
/// DIESE ZAHL STEHT AUCH IN `narsil_text::MAX_FONT_SIZE`.
///
/// WARUM DIE DUPLIKATION UND NICHT EINE DEPENDENCY: `narsil-parser` haengt
/// an `narsil-core` und `narsil-dom`. Eine Abhaengigkeit von `narsil-text` waere
/// moeglich und erzeugt keine Zyklik — aber sie waere die erste
/// Cargo-Aenderung, die jemand an diesem Crate vornimmt, und sie muesste
/// an zwei Orten gepflegt werden (hier und in `narsil-text`). Das ist genau
/// der Fehler, den ADR-021 ueber `DisplayList::bounds()` beschreibt:
/// dieselbe Zahl an zwei Orten laeuft auseinander.
///
/// Der Test `max_font_size_stimmt_mit_narsil_text_ueberein` in
/// `tests/fontsizes.rs` vergleicht die beiden Zahlen direkt. Wer eine
/// aendert, bekommt einen roten Test statt eines stillen Unterschieds.
/// Das ist die billigere Form der "zweiten hartkodierten Breite", die
/// Agent1 in ENTSCHEIDUNG_0017 abgelehnt hat — mit dem Unterschied, dass
/// hier ein Test den Abstand misst.
const MAX_FONT_SIZE: f32 = 4096.0;

/// Kleinste erlaubte Schriftgroesse in Pixeln.
///
/// TASK_0029. Bis 05:18 war die Untergrenze "mehr als 0", und
/// `font-size: 0.5` war ein gueltiger Parse.
///
/// GEMESSEN ueber das FERTIGE BILD (100x50, `text "x"` weiss):
///
/// ```text
/// 0.1 -> 0 Pixel     0.5 -> 0 Pixel     0.9 -> 1 Pixel
/// 0.4 -> 0 Pixel     0.6 -> 0 Pixel     1.0 -> 1 Pixel
/// ```
///
/// Der Grund ist nicht "0.5 ist klein", sondern: die Sprache nahm einen
/// Wert an, den sie beim Runden still verlor. 0.5 wird zu 1.0, ohne dass
/// jemand etwas gesagt haette. `0.9` und `1.1` ergeben dieselbe 1x1-
/// Glyphe — zwei verschiedene Angaben, ein Ergebnis, keine Warnung.
///
/// WARUM 1 UND NICHT 4.5, obwohl 4.5 die erste Groesse ist, bei der ALLE
/// 94 druckbaren ASCII-Zeichen Tinte haben (GEMESSEN): siehe
/// `tests/das_bild.rs`, Test `die_grenze_behebt_das_problem_nicht_und_
/// ist_trotzdem_richtig`. Ein Wert, der an 96 von 351 Stellen im Bereich
/// 4.50..8.00 wieder leer wird ("!", "_", "i", "|"), ist keine Zusage. Die
/// Grenze soll einen BENENNBAREN Fehler verhindern — "0.5" ist ein
/// Tippfehler, den der Benutzer erkennt. "5.9 verliert das Zeichen '|'"
/// ist keine Eigenschaft der Sprache, sondern eine der Rasterung, und die
/// gehoert in `narsil-text` gemeldet, nicht hierher.
const MIN_FONT_SIZE: f32 = 1.0;

/// `true`, wenn `size` eine erlaubte Schriftgroesse ist.
///
/// EINE Funktion fuer BEIDE Kanten, und das ist der Punkt.
///
/// Bis 05:40 standen hier drei Pruefstellen fuer dieselbe Eigenschaft:
/// `value <= 0.0`, `value < MIN_FONT_SIZE` und `!is_valid_font_size(value)`.
/// Ich hatte die Untergrenze zweimal eingebaut, weil die erste Fassung
/// nur `> 0` kannte. GEMESSEN: mit dreifacher Pruefung war die
/// Obergrenzen-Meldung "mehr als 0" — eine Zahl, die seit TASK_0029
/// falsch war und trotzdem in jedem Fehler stand.
///
/// Drei Pruefstellen fuer eine Regel sind zwei zu viel. Wer sie
/// vereinigt, muss an EINER Stelle nachsehen, und eine Meldung kann nur
/// einen Zahlensatz nennen.
///
/// DIE GRENZEN SIND NICHT DIE VON `narsil_text::is_valid_font_size`, und das
/// ist Absicht: `narsil-text` nimmt `> 0`, weil es eine Bibliothek ist, die
/// auch halbe Pixel erlauben darf. Die SPRACHE nimmt `>= 1`, weil ein
/// Pixelgitter keine halben Pixel kennt. Die Zahlen duerfen
/// auseinanderlaufen; sie messen verschiedene Dinge.
fn is_valid_font_size(size: f32) -> bool {
    size.is_finite() && (MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&size)
}

/// Maximale Verschachtelungstiefe von Bloecken.
///
/// Recursive Descent nutzt den Aufrufstapel. Ein Tiefenlimit ist noetig,
/// damit ein tief verschachteltes Dokument (oder ein Parser-Bug mit
/// Endlos-Rekursion) sauber als `ParseError` endet, statt den Stapel zu
/// ueberlaufen.
///
/// Der Wert liegt ueber der in der Task geforderten Tiefe von 1000. Da
/// die Verschachtelung im Heap-Stapel liegt, ist die Grenze eine bewusste
/// Schutzgrenze gegen Endlos-Eingaben und keine Stapelgrenze.
const MAX_DEPTH: usize = 2000;

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    // -- Token-Zugriff -----------------------------------------------------

    /// Aktuelles Token, oder ein synthetisches `Eof` am Ende der Liste.
    ///
    /// Der synthetische `Eof` traegt den Span des letzten Tokens. Das
    /// passiert nur, wenn jemand `parse_tokens` ohne abschliessendes `Eof`
    /// aufruft; der Parser meldet dann „unerwartetes Dateiende", statt zu
    /// panicen. Ein `static` Wert traegt keinen dynamischen Span, deshalb
    /// wird hier der Wert statt einer Referenz zurueckgegeben.
    fn peek(&self) -> Token {
        self.tokens
            .get(self.pos)
            .cloned()
            .unwrap_or_else(|| Token::new(TokenKind::Eof, self.eof_span()))
    }

    /// Span des Dateendes: Ende des letzten Tokens, sonst 0.
    fn eof_span(&self) -> Span {
        self.tokens
            .last()
            .map_or(Span::EMPTY, |t| Span::new(t.span.end(), 0))
    }

    fn at_eof(&self) -> bool {
        self.pos >= self.tokens.len() || self.peek().kind == TokenKind::Eof
    }

    /// Naechstes Token konsumieren.
    fn advance(&mut self) -> Token {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token.unwrap_or_else(|| Token::new(TokenKind::Eof, self.eof_span()))
    }

    /// Prueft die Tokenart, ohne zu konsumieren.
    fn check(&self, kind: &TokenKind) -> bool {
        self.peek().kind == *kind
    }

    /// Konsumiert ein Token der geforderten Art, sonst Fehler.
    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<Token, ParseError> {
        if self.check(kind) {
            return Ok(self.advance());
        }
        Err(ParseError::new(
            format!(
                "erwartet: {what}, gefunden: {}",
                describe(&self.peek().kind)
            ),
            self.peek().span,
        ))
    }

    // -- Grammatik ---------------------------------------------------------

    /// `document := page_block EOF`
    fn parse_document(&mut self) -> Result<Document, ParseError> {
        if self.at_eof() {
            return Err(ParseError::new(
                "leeres Dokument: es wird genau ein 'page'-Block erwartet",
                Span::new(0, 0),
            ));
        }

        // Genau ein `page` als erste Konstruktion (E2, E3).
        if !self.check(&TokenKind::Page) {
            return Err(ParseError::new(
                format!(
                    "Dokument muss mit 'page' beginnen, gefunden: {}",
                    describe(&self.peek().kind)
                ),
                self.peek().span,
            ));
        }
        // Das `page` wird hier konsumiert: `page_block := "page" block`.
        self.advance();

        let block = self.parse_block(true)?;

        // Der Zwischenbaum wird in der Reihenfolge des Quelltextes in die
        // Arena geschrieben. Rekursiv in Vor-Reihenfolge: erst der Knoten
        // selbst, dann seine Kinder. Das entspricht der Dokumentreihenfolge
        // und der Tiefensuche von `Document::iter_depth_first`.
        let mut document = Document::new();
        document
            .set_style(document.root(), block.style)
            .map_err(dom_error)?;
        let root_id = document.root();
        for child in &block.children {
            write_node(&mut document, root_id, child)?;
        }

        // Trailing Input nach dem Root ist ein Fehler (E-artig: Eingabe nach
        // dem einzigen `page`-Block).
        if !self.at_eof() {
            return Err(ParseError::new(
                format!(
                    "unerwartete Eingabe nach dem 'page'-Block: {}",
                    describe(&self.peek().kind)
                ),
                self.peek().span,
            ));
        }

        document.validate().map_err(dom_error)?;
        Ok(document)
    }

    /// `block := "{" statement* "}"`
    ///
    /// Das fuehrende `page`/`container` wurde bereits konsumiert; hier
    /// beginnt direkt der Block.
    ///
    /// ACHTUNG: Die Verschachtelung wird bewusst ITERATIV verarbeitet. Ein
    /// rekursiver Aufbau waere einfacher zu schreiben, laeuft aber mit der
    /// Verschachtelungstiefe in den Aufrufstapel. Bei 1000 Ebenen
    /// verschachtelter `container` (von der Task gefordert) waere das
    /// ein Stack-Overflow. Der Stapel liegt hier stattdessen im Heap.
    ///
    /// `is_root` steuert nur die Fehlermeldung, nicht die Grammatik.
    fn parse_block(&mut self, is_root: bool) -> Result<Block, ParseError> {
        let open = self.expect(&TokenKind::LBrace, "'{'")?;

        // Ein offener Block pro Ebene. Der Stapel liegt im Heap, damit die
        // Verschachtelungstiefe nicht durch den Aufrufstapel begrenzt ist.
        struct Open {
            /// Art des Knotens, dessen Block dies ist.
            kind: NodeKind,
            /// Textinhalt, nur bei `NodeKind::Text`.
            text: Option<String>,
            /// Style und bisher gefundene Kinder.
            block: Block,
            /// Namen der in diesem Block gesehenen Properties (E11).
            seen: Vec<String>,
            /// Welche Seiten per LANGFORM gesetzt wurden (TASK_0029).
            ///
            /// Agent1: "Langform schlaegt Kurzform im selben Block, egal
            /// in welcher Reihenfolge sie steht." Ohne diese Liste
            /// haengt es an der Reihenfolge — GEMESSEN 05:58:
            ///
            /// ```text
            /// margin: 10 20 30 40; margin-top: 99;  -> top = 99
            /// margin-top: 99; margin: 10 20 30 40;  -> top = 10  <== FALSCH
            /// ```
            /// Die Liste traegt je Seite: gesetzt von margin-lang,
            /// von padding-lang, oder von keiner.
            langform_seiten: LangformSeiten,
            /// Start der `{`, fuer den Block-Span.
            lbrace: u32,
        }

        let mut stack: Vec<Open> = vec![Open {
            kind: NodeKind::Page,
            text: None,
            block: Block::default(),
            seen: Vec::new(),
            langform_seiten: LangformSeiten::default(),
            lbrace: open.span.start,
        }];

        loop {
            // Der oberste Eintrag ist der aktuelle Block. `is_some` ist
            // durch die Invariante garantiert: der Stack wird nie leer
            // gelesen, sondern nur am Ende beim Poppen verlassen.
            let is_text_block = match stack.last() {
                Some(open) => open.kind.is_text(),
                None => {
                    return Err(ParseError::new(
                        "unerwartetes Dateiende: '}' fehlt",
                        self.eof_span(),
                    ));
                }
            };

            if self.check(&TokenKind::RBrace) {
                let close = self.advance();
                let mut finished = match stack.pop() {
                    Some(open) => open,
                    None => {
                        return Err(ParseError::new(
                            "unerwartetes Dateiende: '}' fehlt",
                            self.eof_span(),
                        ));
                    }
                };
                // Span: von `{` bis `}` inklusive, halb-offen gerechnet.
                finished.block.span = span_from(finished.lbrace, close.span.end());

                if stack.is_empty() {
                    // Der Root-Block ist fertig; sein Style und seine Kinder
                    // sind das Ergebnis.
                    return Ok(finished.block);
                }

                let node = ChildNode {
                    kind: finished.kind,
                    text: finished.text,
                    style: finished.block.style,
                    children: finished.block.children,
                    span: finished.block.span,
                };
                if let Some(parent) = stack.last_mut() {
                    parent.block.children.push(node);
                }
                continue;
            }

            if self.at_eof() {
                return Err(ParseError::new(
                    if is_root {
                        "unerwartetes Dateiende: '}' fehlt"
                    } else {
                        "unerwartetes Dateiende: Block nicht geschlossen"
                    },
                    self.eof_span(),
                ));
            }

            if matches!(self.peek().kind, TokenKind::Ident(_)) {
                // Property im aktuellen Block. Der Stack ist hier nicht
                // leer, siehe Invariante oben.
                if let Some(current) = stack.last_mut() {
                    let kind = current.kind;
                    self.parse_property(
                        &mut current.block,
                        &mut current.seen,
                        &mut current.langform_seiten,
                        kind,
                    )?;
                }
                continue;
            }

            // Ein Kindknoten beginnt.
            if is_text_block {
                // E13: `text` hat keine Kinder.
                return Err(ParseError::new(
                    "'text' darf keine Kinder enthalten",
                    self.peek().span,
                ));
            }

            let head = self.advance();
            let (kind, text) = match &head.kind {
                TokenKind::Container => (NodeKind::Container, None),
                TokenKind::Page => {
                    return Err(ParseError::new(
                        "ein Dokument darf nur ein 'page' enthalten",
                        head.span,
                    ));
                }
                TokenKind::Text => {
                    // `text` braucht direkt danach einen String (E12).
                    if !matches!(self.peek().kind, TokenKind::String(_)) {
                        return Err(ParseError::new(
                            format!(
                                "'text' erwartet einen Textinhalt in Anfuehrungszeichen, gefunden: {}",
                                describe(&self.peek().kind)
                            ),
                            self.peek().span,
                        ));
                    }
                    let text_token = self.advance();
                    let text = match text_token.kind {
                        TokenKind::String(s) => s,
                        _ => String::new(),
                    };
                    (NodeKind::Text, Some(text))
                }
                other => {
                    return Err(ParseError::new(
                        format!(
                            "erwartet: 'page', 'container' oder 'text', gefunden: {}",
                            describe(other)
                        ),
                        head.span,
                    ));
                }
            };

            if stack.len() >= MAX_DEPTH {
                return Err(ParseError::new(
                    format!("Verschachtelung zu tief (maximal {MAX_DEPTH} Bloecke)"),
                    head.span,
                ));
            }

            let lbrace = self.expect(&TokenKind::LBrace, "'{'")?.span.start;
            stack.push(Open {
                kind,
                text,
                block: Block::default(),
                seen: Vec::new(),
                langform_seiten: LangformSeiten::default(),
                lbrace,
            });
        }
    }

    /// `property := IDENT ":" value ";"`
    ///
    /// Wird nur aufgerufen, wenn das aktuelle Token ein `Ident` ist; die
    /// Aufrufstelle in `parse_block` prueft das.
    ///
    /// `kind` ist die Art des Knotens, dessen Block gerade geparst wird.
    /// Er bestimmt, welche Properties gelten (E17).
    fn parse_property(
        &mut self,
        block: &mut Block,
        seen: &mut Vec<String>,
        langform_seiten: &mut LangformSeiten,
        kind: NodeKind,
    ) -> Result<(), ParseError> {
        let name_token = self.advance();
        let name = match &name_token.kind {
            TokenKind::Ident(n) => n.clone(),
            // unreachable: parse_block prueft vorher auf Ident
            other => {
                return Err(ParseError::new(
                    format!("erwartet: Property-Name, gefunden: {}", describe(other)),
                    name_token.span,
                ));
            }
        };

        // Unbekannte Property: kein stilles Ignorieren (E5).
        //
        // Bekannt, aber an dieser Knotenart nicht erlaubt (E17): der Wert
        // wird gelesen, geprueft und dann VERWORFEN. Die Property ist
        // damit ignoriert, wie die Spezifikation es verlangt — sie steht
        // nicht im Baum, und sie ist auch kein Fehler.
        let erlaubt = match kind {
            NodeKind::Page => PAGE_PROPERTIES,
            NodeKind::Container => CONTAINER_PROPERTIES,
            NodeKind::Text => TEXT_PROPERTIES,
        };
        if !erlaubt.contains(&name.as_str()) {
            if ALL_PROPERTIES.contains(&name.as_str()) {
                // Wird ignoriert: Wert konsumieren, ohne zu speichern.
                self.skip_ignored_value(&name, &name_token.span)?;
                // Und nicht als " gesehen " markieren — ein spaeteres
                // Vorkommen ist dann ebenfalls nur ein Ignorieren.
                return Ok(());
            }
            return Err(ParseError::new(
                format!("unbekannte Property: '{name}'"),
                name_token.span,
            ));
        }

        // Doppelte Property im selben Block (E11).
        if seen.iter().any(|s| s == &name) {
            return Err(ParseError::new(
                format!("Property '{name}' steht mehrfach im selben Block"),
                name_token.span,
            ));
        }
        seen.push(name.clone());

        self.expect(&TokenKind::Colon, &format!("':' nach '{name}'"))?;

        // LANGFORM: `margin-top: 20;` setzt genau EINE Seite.
        //
        // Sie bekommt den normalen Ein-Wert-Weg. Das ist Absicht: wer
        // `margin-top: 20` schreibt, meint 20, und ein zweiter Wert waere
        // ein Fehler, den man sofort sehen soll. Nur die Kurzformen von
        // `margin`/`padding` duerfen mehrere Werte lesen.
        if let Some(seite) = langform_seite(&name) {
            let wert = to_length(&name, &self.take_value(&name)?)?;
            seite.setze(spacing_von(&mut block.style, &name), wert);
            langform_seiten.setze(gruppe_von(&name), seite);
            Self::pruefe_style(&block.style, &name, &name_token)?;
            self.expect(
                &TokenKind::Semicolon,
                &format!("';' am Ende der Property '{name}'"),
            )?;
            return Ok(());
        }

        // KURZFORM: `margin: 10 20 30 40;` liest EIN bis VIER Werte.
        let wert_token = self.take_value(&name)?;
        if ist_kurzform(&name) {
            let werte = self.take_weitere_werte(&name, wert_token)?;
            let spacing = aus_kurzform(&werte).ok_or_else(|| {
                ParseError::new(
                    format!(
                        "Property '{name}' erwartet 1 bis 4 Werte, bekam {}",
                        werte.len()
                    ),
                    self.peek().span,
                )
            })?;
            // LANGFORM GEWINNT, egal in welcher Reihenfolge (Agent1).
            //
            // Deshalb wird die Kurzform nicht zugewiesen, sondern
            // SEITENWEISE: eine Seite, die eine Langform gesetzt hat,
            // bleibt stehen. GEMESSEN ohne das: die Reihenfolge
            // entschied, und die Kurzform am Ende ueberschrieb die
            // Langform.
            let gruppe = if name.starts_with("padding") {
                "padding"
            } else {
                "margin"
            };
            let ziel = spacing_von(&mut block.style, &name);
            for seite in [Seite::Top, Seite::Right, Seite::Bottom, Seite::Left] {
                if !langform_seiten.ist_gesetzt(gruppe, seite) {
                    seite.setze(ziel, aus_kurzform_seite(&spacing, seite));
                }
            }
            // DIE 100%-OBERGRENZE (ADR-021), und sie ist hier hingeraten.
            //
            // GEMESSEN 05:52: `margin: 150%` wurde ANGENOMMEN, nachdem ich
            // den Kurzform-Pfad eingebaut hatte — der bestehende Test
            // `percent_above_100_is_valid_for_size_and_invalid_for_spacing`
            // wurde rot. Die Pruefung sass in `apply_value`, und der neue
            // Pfad ging daran vorbei.
            //
            // Agent1 hat genau davor gewarnt: "Die 100-Prozent-Obergrenze
            // aus ADR-021 gilt fuer ALLE acht Formen, nicht nur fuer die
            // Kurzform. Sonst haette man `margin-top: 150%` als Hintertuer."
            // Es war die KURZFORM, die die Grenze verloren hat, nicht die
            // Langform — aber der Grund ist derselbe: eine Regel, die an
            // einem Pfad haengt, haengt an einem Pfad.
            Self::pruefe_style(&block.style, &name, &name_token)?;
        } else {
            apply_value(&name, &wert_token, &mut block.style)?;
        }

        self.expect(
            &TokenKind::Semicolon,
            &format!("';' am Ende der Property '{name}'"),
        )?;

        Ok(())
    }

    /// Konsumiert den Wert nach `:` und prueft, dass ueberhaupt einer da ist.
    ///
    /// E8 ist "Property ohne Wert". Drei Faelle zaehlen dazu:
    ///   * `width: }`   — der Block endet sofort
    ///   * `width:` am Ende der Datei
    ///   * `width: ;`   — direkt das Semikolon
    ///
    /// Der dritte Fall war vorher ein Fehler mit der Meldung "erwartet
    /// eine Zahl ... gefunden ';'", obwohl der Wert sehr wohl fehlt. Beide
    /// Pfade (gespeicherte und ignorierte Property, E17) benutzen jetzt
    /// DIESE Methode, damit `color: ;` und `width: ;` dieselbe Meldung
    /// bekommen.
    fn take_value(&mut self, name: &str) -> Result<Token, ParseError> {
        let value_token = self.peek().clone();
        let ist_ende = matches!(value_token.kind, TokenKind::RBrace)
            || value_token.is_exactly(&TokenKind::Eof)
            || value_token.is_exactly(&TokenKind::Semicolon);
        if ist_ende {
            return Err(ParseError::new(
                format!("Property '{name}' hat keinen Wert"),
                value_token.span,
            ));
        }
        Ok(self.advance())
    }

    /// Endpruefung der Style-Invarianten, aufgerufen von BEIDEN Wegen.
    ///
    /// `margin: 150%` und `margin-top: 150%` muessen beide abgelehnt
    /// werden (ADR-021), und beide muessen es an derselben Stelle
    /// pruefen. Diese Funktion ist diese Stelle.
    ///
    /// GEMESSEN 05:52: vorher sass die Pruefung am Ende von
    /// `apply_value`. Der neue Kurzform-Pfad ging daran vorbei, und
    /// `margin: 150%` wurde angenommen — der bestehende Test wurde rot
    /// und hat den Verlust gemeldet. Genau dafuer steht er hier.
    fn pruefe_style(style: &Style, name: &str, token: &Token) -> Result<(), ParseError> {
        if !style.is_valid() || !narsil_spacing_gueltig(style) {
            return Err(ParseError::new(
                format!("Property '{name}' ergibt einen ungueltigen Wert"),
                token.span,
            ));
        }
        Ok(())
    }

    /// Liest alle Werte einer Kurzform bis zum `;`.
    ///
    /// `margin: 10 20 30 40;` ergibt vier `Length`. Der erste Token ist
    /// schon gelesen (`erster`), die restlichen kommen hier.
    ///
    /// WARUM BIS FUENF GELESEN und nicht bis vier: `margin: 1 2 3 4 5`
    /// muss ein ParseError sein mit der Zahl 5 in der Meldung. Bricht
    /// man nach dem vierten ab, sieht der Benutzer
    /// "erwartet: ';' ..., gefunden: Zahl '5'" und muss sich die Restzahl
    /// selbst zusammenrechnen. Fuenf ist eine freundliche Obergrenze
    /// fuer die Diagnose; `aus_kurzform` lehnt alles ab, was nicht
    /// 1..=4 ist, also kann eine laengere Liste nicht durchrutschen.
    fn take_weitere_werte(&mut self, name: &str, erster: Token) -> Result<Vec<Length>, ParseError> {
        let mut werte = vec![to_length(name, &erster)?];
        // Der erste Wert muss eine Zahl sein. `margin: #ff0000;` scheitert
        // hier mit der richtigen Meldung statt mit "1 bis 4 Werte".
        while !matches!(
            self.peek().kind,
            TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof
        ) {
            werte.push(to_length(name, &self.advance())?);
            if werte.len() > 5 {
                return Err(ParseError::new(
                    format!(
                        "Property '{name}' erwartet 1 bis 4 Werte — \
                         nach der vierten Zahl kommt nichts mehr, \
                         sondern das Ende der Property"
                    ),
                    erster.span,
                ));
            }
        }
        Ok(werte)
    }

    /// Liest `IDENT ":" value ";"` und verwirft den Wert (E17).
    ///
    /// Die Property ist an dieser Knotenart nicht erlaubt, aber sie ist
    /// syntaktisch gueltig. Sie wird vollstaendig konsumiert — inklusive
    /// Typpruefung des Wertes, damit `color: #nix` auch hier ein Fehler
    /// ist — aber nicht gespeichert.
    fn skip_ignored_value(&mut self, name: &str, name_span: &Span) -> Result<(), ParseError> {
        let _ = name_span;
        self.expect(
            &TokenKind::Colon,
            &format!("':' nach '{name}' (Property wird hier ignoriert)"),
        )?;

        let value_token = self.take_value(name)?;

        // Typpruefung ohne Speicherung.
        let mut wegwerf = Style::default();
        apply_value(name, &value_token, &mut wegwerf)?;

        self.expect(
            &TokenKind::Semicolon,
            &format!("';' am Ende der Property '{name}'"),
        )?;
        Ok(())
    }
}

// ===========================================================================
// ZWISCHENBAUM UND HELFER
// ===========================================================================

/// Ergebnis eines Block-Parsings.
///
/// Zwischenbaum: der Parser baut erst diese Struktur und schreibt sie dann
/// in die `Document`-Arena. Das haelt das Parsen unabhaengig vom DOM und
/// erlaubt sauberes Rueckgeben bei Fehlern, ohne halbfertige Knoten im
/// Dokument zu hinterlassen.
#[derive(Debug, Default)]
struct Block {
    style: Style,
    children: Vec<ChildNode>,
    span: Span,
}

/// Ein innerhalb eines Blocks gefundener Kindknoten.
#[derive(Debug)]
struct ChildNode {
    kind: NodeKind,
    text: Option<String>,
    style: Style,
    children: Vec<ChildNode>,
    span: Span,
}

/// Schreibt einen Zwischenbaum-Knoten samt Kindern in die Arena.
///
/// Die Rekursion folgt der Quelltexttiefe, die der Parser bereits gegen
/// `MAX_DEPTH` begrenzt hat.
fn write_node(document: &mut Document, parent: NodeId, node: &ChildNode) -> Result<(), ParseError> {
    let id = document
        .push_child(parent, node.kind, node.text.clone(), node.span)
        .map_err(dom_error)?;
    // 04.10.2026 (SCHRIFT): `.clone()` wegen `Style::schriftfamilie` —
    // `Style` ist damit nicht mehr `Copy` (siehe die Doku an `Style` in
    // narsil-core). GEMESSEN, warum hier geklont und nicht verschoben:
    // `node` ist hinter einer Referenz, und `write_node` laeuft fuer
    // **jeden** Knoten — ein `clone()` je Knoten ist die Masse, ein
    // `take()` wuerde den Aufrufer veraendern.
    // Release 09.10.2026: Version 0 kennt kein `line-height` — die
    // Zeilenhoehe eines Textes kommt vom Messer (Spezifikation 7, „text":
    // Hoehe = Schriftgroesse beim Bitmap-Messer). `Style::default()` traegt
    // aber den CSS-Wert `normal` fuer 16 px (19 px), und der gewann: jeder
    // Text war 19 px hoch, auch bei `font-size: 24` (GEMESSEN:
    // examples/hello.narsil, der Knopf stand bei y 75 statt 80). 0 heisst
    // „nicht gesetzt" — Layout und Zeilenbau nehmen dann den Messer.
    let mut stil = node.style.clone();
    stil.zeilenhoehe_px = 0.0;
    document.set_style(id, stil).map_err(dom_error)?;
    for child in &node.children {
        write_node(document, id, child)?;
    }
    Ok(())
}

/// Trägt einen Wert-Token in die passende Style-Zeile ein.
///
/// Mapping nach Spezifikation Abschnitt 6. Bei falschem Wert-Typ fuer die
/// Property wird ein `ParseError` mit dem Span des Tokens erzeugt — nicht
/// stillschweigend ignoriert.
fn apply_value(name: &str, token: &Token, style: &mut Style) -> Result<(), ParseError> {
    match name {
        "width" | "height" => {
            let length = to_length(name, token)?;
            if name == "width" {
                style.size.width = length;
            } else {
                style.size.height = length;
            }
        }
        "margin" => style.margin = to_spacing("margin", token)?,
        "padding" => style.padding = to_spacing("padding", token)?,
        "background" => style.background = to_color("background", token)?,
        "color" => style.color = to_color("color", token)?,
        "font-size" => {
            let length = to_length("font-size", token)?;
            // `Style::font_size` ist ein `f32`, kein `Length`. Die
            // Prozentform wird gegen `DEFAULT_FONT_SIZE` (16 px) als
            // Referenz aufgeloest — die Schriftgroesse kennt im
            // Quelltext keine eigene Bezugsgroesse. `auto` ist hier nicht
            // erlaubt (E15), daher greift `DEFAULT_FONT_SIZE` nur als
            // Sicherheitsnetz fuer nicht aufloesbare Laengen.
            let value = length.resolve_or(DEFAULT_FONT_SIZE, DEFAULT_FONT_SIZE);

            // DIE BEIDEN KANTEN, in EINER Pruefung (TASK_0024, TASK_0029).
            //
            // `font-size: 99999` (TASK_0024) war ein gueltiger Parse und im
            // fertigen Bild ein leeres: 0 weisse Pixel, Exit 0.
            // `font-size: 0.5` (TASK_0029) war dasselbe mit einer Zahl, die
            // man fuer einen Tippfehler haelt. Beides GEMESSEN ueber das
            // Bild, nicht ueber die Glyphe — die Glyphe war 1x1 und damit
            // nie die Ursache.
            //
            // EINE Pruefung, EINE Meldung. Bis 05:40 standen hier drei
            // (`<= 0.0`, `< MIN_FONT_SIZE`, `!is_valid_font_size(...)`), und
            // die Obergrenzen-Meldung nannte "mehr als 0" — eine Zahl, die
            // seit TASK_0029 falsch war und trotzdem in jedem Fehler stand.
            //
            // Die Meldung nennt BEIDE Grenzwerte, weil der Benutzer sonst
            // nicht entscheiden kann, welche Kante er verletzt hat: bei 0.5
            // ist es die untere, bei 99999 die obere.
            if !is_valid_font_size(value) {
                return Err(ParseError::new(
                    format!(
                        "ungueltige Schriftgroesse: '{name}' ist {}, \
                         erlaubt sind Werte von {} bis {} Pixel — ein \
                         Pixelgitter kennt keine halben Pixel, und ein Wert \
                         unter 1 wird beim Runden still zu 1",
                        number_text(&token.kind),
                        MIN_FONT_SIZE,
                        MAX_FONT_SIZE
                    ),
                    token.span,
                ));
            }
            style.font_size = value;
        }
        "direction" => style.direction = to_direction(token)?,
        other => {
            return Err(ParseError::new(
                format!("unbekannte Property: '{other}'"),
                token.span,
            ));
        }
    }

    // Endpruefung: Style-Invarianten muessen gelten (z. B. Percent > 100
    // oder eine ungueltige Schriftgroesse).
    if !style.is_valid() || !narsil_spacing_gueltig(style) {
        return Err(ParseError::new(
            format!("Property '{name}' ergibt einen ungueltigen Wert"),
            token.span,
        ));
    }
    Ok(())
}

// The .narsil language caps spacing at 100%; CSS Style deliberately allows
// negative margins and larger percentages. Keep this language rule local.
fn narsil_spacing_gueltig(style: &Style) -> bool {
    [style.margin.left, style.margin.top, style.margin.right, style.margin.bottom,
     style.padding.left, style.padding.top, style.padding.right, style.padding.bottom]
        .iter().all(|length| length.is_valid_spacing())
}

/// Wert-Token -> `Length`.
fn to_length(property: &str, token: &Token) -> Result<Length, ParseError> {
    match &token.kind {
        TokenKind::Number(n) => Ok(Length::Px(*n as f32)),
        TokenKind::Float(f) => Ok(Length::Px(*f)),
        TokenKind::Percent(p) => Ok(Length::Percent(*p)),
        TokenKind::Auto => {
            // `auto` ist nur fuer width/height erlaubt (E15).
            if AUTO_ALLOWED.contains(&property) {
                Ok(Length::Auto)
            } else {
                Err(ParseError::new(
                    format!("'auto' ist fuer '{property}' nicht erlaubt"),
                    token.span,
                ))
            }
        }
        other => Err(ParseError::new(
            format!(
                "'{property}' erwartet eine Zahl, Prozent oder 'auto', gefunden: {}",
                describe(other)
            ),
            token.span,
        )),
    }
}

/// Wert-Token -> `Spacing` auf allen vier Seiten.
fn to_spacing(property: &str, token: &Token) -> Result<Spacing, ParseError> {
    // `auto` wird von to_length abgelehnt (margin/padding stehen nicht in
    // AUTO_ALLOWED) — E15.
    let length = to_length(property, token)?;
    Ok(Spacing::all(length))
}

/// Wert-Token -> `Color`.
fn to_color(property: &str, token: &Token) -> Result<Color, ParseError> {
    let raw = match &token.kind {
        TokenKind::Color(s) => s,
        other => {
            return Err(ParseError::new(
                format!(
                    "'{property}' erwartet eine Farbe wie '#101014', gefunden: {}",
                    describe(other)
                ),
                token.span,
            ));
        }
    };
    Color::parse_hex(raw)
        .map_err(|e| ParseError::new(format!("ungueltige Farbe '{raw}': {e}"), token.span))
}

/// Wert-Token -> `Direction`.
fn to_direction(token: &Token) -> Result<Direction, ParseError> {
    match &token.kind {
        TokenKind::Row => Ok(Direction::Row),
        TokenKind::Column => Ok(Direction::Column),
        other => Err(ParseError::new(
            format!(
                "'direction' erwartet 'row' oder 'column', gefunden: {}",
                describe(other)
            ),
            token.span,
        )),
    }
}

/// Span von `start` bis `end` (halb-offen: `end` ist exklusiv).
fn span_from(start: u32, end: u32) -> Span {
    Span::new(start, end.saturating_sub(start))
}

/// Zahl so schreiben, wie der Benutzer sie gelesen hat.
///
/// GEMESSEN (TASK_0024, 05:2x): `format!("{f}")` auf `f32` beschneidet auf
/// 9 signifikante Stellen. Der Benutzer schreibt `font-size: 2147483648` und
/// liest in der Fehlermeldung `2147483600` — eine Zahl, die in seinem
/// Dokument nicht steht. Er sucht sie, findet sie nicht, und der Fehler
/// ist wertlos.
///
/// Die Ursache ist nicht der Zahlenwert, sondern die Anzeige: die f32,
/// deren Bits `0x4f000000` sind, ist exakt 2^31. Nur die Ausgabe rundet.
///
/// DESHALB ist die Reihenfolge hier umgekehrt wie naheliegend: zuerst der
/// Exaktwert aus dem Token, und nur wenn es keine Ganzzahl ist, das f32.
/// `TokenKind::Number` traegt ein `u32` — das ist der getippte Text,
/// ohne Rundung. `Float` und `Percent` haben `u32` nicht, dort bleibt
/// nur das f32, und da ist die Rundung unvermeidbar (TASK_0024 §7).
fn number_text(kind: &TokenKind) -> String {
    match kind {
        // Ganzzahl: der Token traegt die Zahl ohne Verlust.
        TokenKind::Number(n) => n.to_string(),
        // Dezimalzahl und Prozent: es gibt kein u32, also das f32.
        TokenKind::Float(f) => format!("{f}"),
        TokenKind::Percent(p) => format!("{p}%"),
        other => describe(other),
    }
}

/// Kurze, lesbare Beschreibung einer Tokenart fuer Fehlermeldungen.
fn describe(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Page => "'page'".into(),
        TokenKind::Container => "'container'".into(),
        TokenKind::Text => "'text'".into(),
        TokenKind::Ident(s) => format!("Bezeichner '{s}'"),
        TokenKind::Number(_) | TokenKind::Float(_) | TokenKind::Percent(_) => {
            format!("Wert '{}'", number_text(kind))
        }
        TokenKind::Auto => "'auto'".into(),
        TokenKind::Color(s) => format!("Farbe '{s}'"),
        TokenKind::String(_) => "Textinhalt".into(),
        TokenKind::LBrace => "'{'".into(),
        TokenKind::RBrace => "'}'".into(),
        TokenKind::Colon => "':'".into(),
        TokenKind::Semicolon => "';'".into(),
        TokenKind::Row => "'row'".into(),
        TokenKind::Column => "'column'".into(),
        TokenKind::Eof => "Dateiende".into(),
    }
}

/// Wandelt einen DOM-Fehler in einen `ParseError` um.
///
/// Die DOM-Invarianten (z. B. `Text` mit Inhalt) haelt der Parser selbst
/// ein, deshalb ist dieser Pfad praktisch unerreichbar. Er existiert,
/// damit kein `?` auf einem `DomError` ohne Karte stehen bleibt.
fn dom_error(err: narsil_dom::DomError) -> ParseError {
    ParseError::new(format!("Dokumentbaum ungueltig: {err}"), Span::EMPTY)
}

impl From<LexError> for ParseError {
    fn from(err: LexError) -> Self {
        ParseError::new(err.message, err.span)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(kind: TokenKind) -> Token {
        Token::new(kind, Span::EMPTY)
    }

    /// Baut eine Token-Liste mit `Eof` am Ende, wie `tokenize` es liefert.
    fn toks(kinds: Vec<TokenKind>) -> Vec<Token> {
        let mut v: Vec<Token> = kinds.into_iter().map(tok).collect();
        v.push(tok(TokenKind::Eof));
        v
    }

    /// Minimal gueltiges Dokument: `page { }`.
    fn empty_page() -> Vec<Token> {
        toks(vec![TokenKind::Page, TokenKind::LBrace, TokenKind::RBrace])
    }

    fn parse_ok(kinds: Vec<TokenKind>) -> Document {
        parse_tokens(&toks(kinds)).expect("sollte erfolgreich parsen")
    }

    fn parse_err(kinds: Vec<TokenKind>) -> ParseError {
        parse_tokens(&toks(kinds)).expect_err("sollte einen Fehler liefern")
    }

    // -- Happy Path --------------------------------------------------------

    #[test]
    fn minimal_page_parses() {
        let doc = parse_ok(vec![TokenKind::Page, TokenKind::LBrace, TokenKind::RBrace]);
        assert_eq!(doc.len(), 1, "nur der Root");
        assert_eq!(doc.count_of_kind(NodeKind::Page), 1);
        assert!(doc.validate().is_ok());
    }

    #[test]
    fn properties_land_in_the_right_style_fields() {
        // `font-size` wird hier an einem CONTAINER geprueft, nicht am
        // `page`-Root: laut E17 wird `font-size` an `page` ignoriert.
        // Siehe `e17_color_an_page_is_ignored`.
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Container,
            TokenKind::LBrace,
            TokenKind::Ident("width".into()),
            TokenKind::Colon,
            TokenKind::Number(800),
            TokenKind::Semicolon,
            TokenKind::Ident("height".into()),
            TokenKind::Colon,
            TokenKind::Number(600),
            TokenKind::Semicolon,
            TokenKind::Ident("background".into()),
            TokenKind::Colon,
            TokenKind::Color("#101014".into()),
            TokenKind::Semicolon,
            TokenKind::Ident("font-size".into()),
            TokenKind::Colon,
            TokenKind::Number(24),
            TokenKind::Semicolon,
            TokenKind::Ident("direction".into()),
            TokenKind::Colon,
            TokenKind::Row,
            TokenKind::Semicolon,
            TokenKind::RBrace,
            TokenKind::RBrace,
        ]);
        let container = doc
            .iter_depth_first()
            .find(|n| n.kind == NodeKind::Container)
            .expect("Container");
    // 04.10.2026 (SCHRIFT, (c)): `.clone()` — `Style` traegt jetzt
    // `schriftfamilie: String` und ist nicht mehr `Copy` (narsil-core).
        let style = container.style.clone();
        assert_eq!(style.size.width, Length::Px(800.0));
        assert_eq!(style.size.height, Length::Px(600.0));
        assert_eq!(style.background, Color::new(0x10, 0x10, 0x14, 255));
        assert_eq!(style.font_size, 24.0);
        assert_eq!(style.direction, Direction::Row);
    }

    #[test]
    fn e17_color_an_page_is_ignored() {
        // Spezifikation E17: "`color` an `page` wird in v0 akzeptiert und
        // IGNORIERT — nur syntaktische Fehler sind in v0 Pflicht".
        //
        // "Ignoriert" heisst: der Wert landet NICHT im Baum. Der Knoten
        // behaelt `Style::default().color`. "Geparst und vergessen" sieht
        // gleich aus und ist nicht dasselbe — deshalb wird hier der
        // ERGEBNISBAUM geprueft, nicht nur `is_ok()`.
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("color".into()),
            TokenKind::Colon,
            TokenKind::Color("#ff0000".into()),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
    // 04.10.2026 (SCHRIFT, (c)): `.clone()` — `Style` traegt jetzt
    // `schriftfamilie: String` und ist nicht mehr `Copy` (narsil-core).
        let style = doc.node(doc.root()).expect("root").style.clone();
        assert_eq!(
            style.color,
            Style::default().color,
            "E17: color an page wird ignoriert, nicht gespeichert"
        );
        assert_ne!(
            style.color,
            Color::rgb(255, 0, 0),
            "der Wert ist NICHT im Baum"
        );
    }

    #[test]
    fn e17_font_size_an_page_is_ignored() {
        // Gleiche Regel fuer `font-size`: an `page` ignoriert.
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("font-size".into()),
            TokenKind::Colon,
            TokenKind::Number(24),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
    // 04.10.2026 (SCHRIFT, (c)): `.clone()` — `Style` traegt jetzt
    // `schriftfamilie: String` und ist nicht mehr `Copy` (narsil-core).
        let style = doc.node(doc.root()).expect("root").style.clone();
        assert_eq!(
            style.font_size,
            Style::default().font_size,
            "E17: font-size an page wird ignoriert"
        );
        assert_ne!(style.font_size, 24.0);
    }

    #[test]
    fn e17_ignorierte_property_wird_typgeprueft() {
        // Eine ignorierte Property wird nicht blind uebersprungen:
        // `color: ;` ist auch an `page` ein Syntaxfehler.
        let e = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("color".into()),
            TokenKind::Colon,
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
        assert!(e.message.contains("keinen Wert"), "{}", e.message);
    }

    #[test]
    fn e17_ignorierte_property_braucht_auch_semikolon() {
        // Konsistent: die Property wird vollstaendig gelesen.
        let e = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("color".into()),
            TokenKind::Colon,
            TokenKind::Color("#fff".into()),
            TokenKind::RBrace,
        ]);
        assert!(e.message.contains("';'"), "{}", e.message);
    }

    #[test]
    fn e17_ignorierte_property_haelt_doppelte_property_nicht_auf() {
        // Zweimal `color` an `page`: beide werden ignoriert, es ist also
        // KEIN "doppelte Property"-Fehler. Die Liste der gesehenen
        // Properties fuehrt ignorierte Werte nicht.
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("color".into()),
            TokenKind::Colon,
            TokenKind::Color("#fff".into()),
            TokenKind::Semicolon,
            TokenKind::Ident("color".into()),
            TokenKind::Colon,
            TokenKind::Color("#000".into()),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
        assert_eq!(
            doc.node(doc.root()).expect("root").style.color,
            Style::default().color
        );
    }

    #[test]
    fn e17_background_an_page_ist_erlaubt() {
        // Gegenprobe: `background` an `page` ist ERLAUBT (E17 nennt
        // `direction` ausdruecklich als erlaubt, und Beispiel 7b setzt
        // `background` an `page`). Nur `color` und `font-size` fallen
        // heraus.
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("background".into()),
            TokenKind::Colon,
            TokenKind::Color("#101014".into()),
            TokenKind::Semicolon,
            TokenKind::Ident("direction".into()),
            TokenKind::Colon,
            TokenKind::Row,
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
    // 04.10.2026 (SCHRIFT, (c)): `.clone()` — `Style` traegt jetzt
    // `schriftfamilie: String` und ist nicht mehr `Copy` (narsil-core).
        let style = doc.node(doc.root()).expect("root").style.clone();
        assert_eq!(
            style.background,
            Color::new(0x10, 0x10, 0x14, 255),
            "bleibt erhalten"
        );
        assert_eq!(style.direction, Direction::Row, "bleibt erhalten");
    }

    #[test]
    fn margin_and_padding_set_all_four_sides() {
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("margin".into()),
            TokenKind::Colon,
            TokenKind::Number(40),
            TokenKind::Semicolon,
            TokenKind::Ident("padding".into()),
            TokenKind::Colon,
            TokenKind::Percent(5.0),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
    // 04.10.2026 (SCHRIFT, (c)): `.clone()` — `Style` traegt jetzt
    // `schriftfamilie: String` und ist nicht mehr `Copy` (narsil-core).
        let style = doc.node(doc.root()).expect("root").style.clone();
        let expected_margin = Spacing::all(Length::Px(40.0));
        let expected_padding = Spacing::all(Length::Percent(5.0));
        assert_eq!(style.margin, expected_margin);
        assert_eq!(style.margin.left, Length::Px(40.0));
        assert_eq!(style.margin.bottom, Length::Px(40.0));
        assert_eq!(style.padding, expected_padding);
    }

    #[test]
    fn auto_is_allowed_for_width_and_height() {
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("width".into()),
            TokenKind::Colon,
            TokenKind::Auto,
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
    // 04.10.2026 (SCHRIFT, (c)): `.clone()` — `Style` traegt jetzt
    // `schriftfamilie: String` und ist nicht mehr `Copy` (narsil-core).
        let style = doc.node(doc.root()).expect("root").style.clone();
        assert_eq!(style.size.width, Length::Auto);
    }

    #[test]
    fn nested_tree_from_spec_7c_has_five_nodes() {
        // Spezifikation Abschnitt 9: genau 5 Knoten in dieser Reihenfolge.
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            // container 1
            TokenKind::Container,
            TokenKind::LBrace,
            // text 1
            TokenKind::Text,
            TokenKind::String("Hallo Narsil".into()),
            TokenKind::LBrace,
            TokenKind::RBrace,
            // container 2
            TokenKind::Container,
            TokenKind::LBrace,
            // text 2
            TokenKind::Text,
            TokenKind::String("Button".into()),
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::RBrace, // container 2
            TokenKind::RBrace, // container 1
            TokenKind::RBrace, // page
        ]);
        assert_eq!(doc.len(), 5);
        assert_eq!(doc.count_of_kind(NodeKind::Page), 1);
        assert_eq!(doc.count_of_kind(NodeKind::Container), 2);
        assert_eq!(doc.count_of_kind(NodeKind::Text), 2);
        assert!(doc.validate().is_ok());
    }

    #[test]
    fn children_keep_source_order() {
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Text,
            TokenKind::String("a".into()),
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::Text,
            TokenKind::String("b".into()),
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::Text,
            TokenKind::String("c".into()),
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::RBrace,
        ]);
        let texts: Vec<&str> = doc
            .iter_depth_first()
            .filter(|n| n.kind.is_text())
            .filter_map(|n| n.text.as_deref())
            .collect();
        assert_eq!(texts, vec!["a", "b", "c"]);
    }

    #[test]
    fn block_spans_cover_lbrace_to_rbrace() {
        // `{` an 10, `}` endet bei 21 -> Span::new(10, 11), end() == 21.
        let tokens = vec![
            Token::new(TokenKind::Page, Span::new(0, 4)),
            Token::new(TokenKind::LBrace, Span::new(10, 1)),
            Token::new(TokenKind::RBrace, Span::new(20, 1)),
            Token::new(TokenKind::Eof, Span::new(21, 0)),
        ];
        let doc = parse_tokens(&tokens).expect("ok");
        let root = doc.node(doc.root()).expect("root");
        // Der Root-Knoten wird von `Document::new()` erzeugt und bekommt
        // dort keinen Span. `Document::set_span` (narsil-dom/src/lib.rs:238)
        // EXISTIERT und ist oeffentlich, aber der Parser ruft sie fuer den
        // Root nicht auf — GEMESSEN null Aufrufe in crates/narsil-parser
        // (TASK_0062). Der Root-Span bleibt deshalb `Span::EMPTY`.
        //
        // Die fruehere Notiz "narsil-dom bietet kein `set_span`"
        // (FERTIG_0008.txt) ist seit dem Bau von set_span ueberholt. Die
        // Luecke liegt damit nicht in narsil-dom, sondern in einem Aufruf
        // hier — und sie beruehrt die Frage, welche Quelltextstelle ein
        // Fehler nennt (TASK_0012, bei Agent6).
        assert_eq!(root.span, Span::EMPTY);

        // Der Span des Root-BLOCKS wird aber berechnet — sichtbar an einem
        // Kindknoten:
        let tokens = vec![
            Token::new(TokenKind::Page, Span::new(0, 4)),
            Token::new(TokenKind::LBrace, Span::new(10, 1)),
            Token::new(TokenKind::Container, Span::new(12, 9)),
            Token::new(TokenKind::LBrace, Span::new(23, 1)),
            Token::new(TokenKind::RBrace, Span::new(40, 1)),
            Token::new(TokenKind::RBrace, Span::new(42, 1)),
            Token::new(TokenKind::Eof, Span::new(43, 0)),
        ];
        let doc = parse_tokens(&tokens).expect("ok");
        let container = doc
            .iter_depth_first()
            .find(|n| n.kind == NodeKind::Container)
            .expect("Container");
        // `{` an 23, `}` endet bei 41 -> len 18.
        assert_eq!(container.span, Span::new(23, 18));
        assert_eq!(container.span.end(), 41);
    }

    #[test]
    fn child_spans_cover_their_own_block() {
        let tokens = vec![
            Token::new(TokenKind::Page, Span::new(0, 4)),
            Token::new(TokenKind::LBrace, Span::new(5, 1)),
            Token::new(TokenKind::Text, Span::new(7, 4)),
            Token::new(TokenKind::String("hi".into()), Span::new(12, 4)),
            Token::new(TokenKind::LBrace, Span::new(17, 1)),
            Token::new(TokenKind::RBrace, Span::new(19, 1)),
            Token::new(TokenKind::RBrace, Span::new(21, 1)),
            Token::new(TokenKind::Eof, Span::new(22, 0)),
        ];
        let doc = parse_tokens(&tokens).expect("ok");
        let text = doc
            .iter_depth_first()
            .find(|n| n.kind.is_text())
            .expect("Textknoten");
        assert_eq!(text.span, Span::new(17, 3));
    }

    #[test]
    fn every_node_but_the_root_has_a_span() {
        // Der Root bleibt ausgenommen (siehe vorheriger Test); alle
        // selbst erzeugten Knoten tragen einen Span. Hier mit echten
        // Spans, damit der Test aussagekraeftig ist.
        let tokens = vec![
            Token::new(TokenKind::Page, Span::new(0, 4)),
            Token::new(TokenKind::LBrace, Span::new(5, 1)),
            Token::new(TokenKind::Container, Span::new(7, 9)),
            Token::new(TokenKind::LBrace, Span::new(18, 1)),
            Token::new(TokenKind::RBrace, Span::new(35, 1)),
            Token::new(TokenKind::Text, Span::new(37, 4)),
            Token::new(TokenKind::String("x".into()), Span::new(42, 3)),
            Token::new(TokenKind::LBrace, Span::new(47, 1)),
            Token::new(TokenKind::RBrace, Span::new(49, 1)),
            Token::new(TokenKind::RBrace, Span::new(51, 1)),
            Token::new(TokenKind::Eof, Span::new(52, 0)),
        ];
        let doc = parse_tokens(&tokens).expect("ok");
        for node in doc.nodes() {
            if node.id != doc.root() {
                assert_ne!(node.span, Span::EMPTY, "Knoten {:?} ohne Span", node.id);
            }
        }
    }

    #[test]
    fn font_size_percent_resolves_against_default() {
        // 50% von DEFAULT_FONT_SIZE (16) = 8.
        // An einem `text`-Knoten, weil `font-size` an `page` laut E17
        // ignoriert wird. Siehe `e17_font_size_an_page_is_ignored`.
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Text,
            TokenKind::String("x".into()),
            TokenKind::LBrace,
            TokenKind::Ident("font-size".into()),
            TokenKind::Colon,
            TokenKind::Percent(50.0),
            TokenKind::Semicolon,
            TokenKind::RBrace,
            TokenKind::RBrace,
        ]);
        let text = doc
            .iter_depth_first()
            .find(|n| n.kind.is_text())
            .expect("Textknoten");
        assert_eq!(text.style.font_size, 8.0);
    }

    // -- Fehlerfaelle (E1-E17) --------------------------------------------

    #[test]
    fn e1_empty_input_is_error() {
        let err = parse_err(vec![]);
        assert!(err.message.contains("page"), "{}", err.message);
    }

    #[test]
    fn e2_container_as_root_is_error() {
        let err = parse_err(vec![
            TokenKind::Container,
            TokenKind::LBrace,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("page"), "{}", err.message);
    }

    #[test]
    fn e3_text_as_root_is_error() {
        let err = parse_err(vec![
            TokenKind::Text,
            TokenKind::String("x".into()),
            TokenKind::LBrace,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("page"), "{}", err.message);
    }

    #[test]
    fn e4_missing_rbrace_is_error() {
        let err = parse_err(vec![TokenKind::Page, TokenKind::LBrace]);
        assert!(err.message.contains("Datei"), "{}", err.message);
    }

    #[test]
    fn e5_unknown_property_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("unsupported".into()),
            TokenKind::Colon,
            TokenKind::Number(1),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
        assert!(
            err.message.contains("unbekannte Property"),
            "{}",
            err.message
        );
    }

    #[test]
    fn e6_property_without_colon_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("width".into()),
            TokenKind::Number(800),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("':"), "{}", err.message);
    }

    #[test]
    fn e7_property_without_semicolon_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("width".into()),
            TokenKind::Colon,
            TokenKind::Number(800),
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("';'"), "{}", err.message);
    }

    #[test]
    fn e8_property_without_value_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("width".into()),
            TokenKind::Colon,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("keinen Wert"), "{}", err.message);
    }

    #[test]
    fn e9_invalid_color_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("color".into()),
            TokenKind::Colon,
            TokenKind::Color("#nothex".into()),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("Farbe"), "{}", err.message);
    }

    #[test]
    fn e11_duplicate_property_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("width".into()),
            TokenKind::Colon,
            TokenKind::Number(1),
            TokenKind::Semicolon,
            TokenKind::Ident("width".into()),
            TokenKind::Colon,
            TokenKind::Number(2),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("mehrfach"), "{}", err.message);
    }

    #[test]
    fn e12_text_without_string_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Text,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("Textinhalt"), "{}", err.message);
    }

    #[test]
    fn e13_text_with_children_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Text,
            TokenKind::String("x".into()),
            TokenKind::LBrace,
            TokenKind::Container,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::RBrace,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("Kinder"), "{}", err.message);
    }

    #[test]
    fn e15_auto_on_margin_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("margin".into()),
            TokenKind::Colon,
            TokenKind::Auto,
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("auto"), "{}", err.message);
    }

    #[test]
    fn e15_auto_on_font_size_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("font-size".into()),
            TokenKind::Colon,
            TokenKind::Auto,
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("auto"), "{}", err.message);
    }

    #[test]
    fn trailing_input_after_root_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::Text,
            TokenKind::String("x".into()),
            TokenKind::LBrace,
            TokenKind::RBrace,
        ]);
        assert!(
            err.message.contains("unerwartete Eingabe"),
            "{}",
            err.message
        );
    }

    #[test]
    fn second_page_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("nur ein 'page'"), "{}", err.message);
    }

    #[test]
    fn wrong_value_type_is_error() {
        // width erwartet eine Zahl, bekommt aber eine Farbe.
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("width".into()),
            TokenKind::Colon,
            TokenKind::Color("#fff".into()),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("width"), "{}", err.message);
    }

    #[test]
    fn invalid_direction_value_is_error() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("direction".into()),
            TokenKind::Colon,
            TokenKind::Number(1),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("direction"), "{}", err.message);
    }

    #[test]
    fn percent_above_100_is_valid_for_size_and_invalid_for_spacing() {
        // DIESELBE Zahl, zwei Antworten — und das ist die Aussage.
        //
        // `Length::is_valid()` (fuer `width`/`height`) laesst jeden
        // Prozentwert >= 0 zu: `width: 150%` in einem 800 px breiten
        // Eltern ergibt 1200 px, und v0 clippt nicht.
        // `is_valid_spacing()` (fuer `margin`/`padding`) hat die
        // Obergrenze 100 %: 150 % Abstand auf allen vier Seiten schiebt
        // den Inhalt aus dem Dokument, und es gibt keinen Weg, es zu
        // bemerken.
        //
        // Die Trennung liegt in `narsil-core` (style.rs Zeile 62-64). Der
        // Parser muss davon nichts wissen — er fragt nur `style.is_valid()`.

        // --- size: gueltig ---
        for wert in [50.0f32, 100.0, 150.0, 1000.0] {
            let doc = parse_ok(vec![
                TokenKind::Page,
                TokenKind::LBrace,
                TokenKind::Ident("width".into()),
                TokenKind::Colon,
                TokenKind::Percent(wert),
                TokenKind::Semicolon,
                TokenKind::RBrace,
            ]);
            assert_eq!(
                doc.node(doc.root()).expect("root").style.size.width,
                Length::Percent(wert),
                "width: {wert}% ist gueltig"
            );
        }

        // --- margin und padding: ParseError, und zwar als BENUTZERFEHLER ---
        for property in ["margin", "padding"] {
            for wert in [150.0f32, 200.0, 1000.0] {
                let err = parse_err(vec![
                    TokenKind::Page,
                    TokenKind::LBrace,
                    TokenKind::Ident(property.into()),
                    TokenKind::Colon,
                    TokenKind::Percent(wert),
                    TokenKind::Semicolon,
                    TokenKind::RBrace,
                ]);
                assert!(
                    err.message.contains("ungueltig"),
                    "{property}: {wert}% muss abgelehnt werden: {}",
                    err.message
                );
                // Es ist ein BENUTZERFEHLER, kein Layout-Fehler: der
                // Parser sagt es, und er sagt es beim Parsen.
                assert!(
                    err.message.contains(property),
                    "die Meldung nennt die Property: {}",
                    err.message
                );
            }
        }

        // --- die Grenze selbst ist erlaubt ---
        // Sonst koennte jemand die Regel auf "< 100" kuerzen, und dann
        // kaeme `padding: 100%` mit.
        let rand = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("width".into()),
            TokenKind::Colon,
            TokenKind::Percent(100.0),
            TokenKind::Semicolon,
            TokenKind::Ident("padding".into()),
            TokenKind::Colon,
            TokenKind::Percent(100.0),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
    // 04.10.2026 (SCHRIFT, (c)): `.clone()` — `Style` traegt jetzt
    // `schriftfamilie: String` und ist nicht mehr `Copy` (narsil-core).
        let s = rand.node(rand.root()).expect("root").style.clone();
        assert_eq!(
            s.size.width,
            Length::Percent(100.0),
            "width: 100% ist erlaubt"
        );
        assert_eq!(
            s.padding,
            Spacing::all(Length::Percent(100.0)),
            "padding: 100% ist erlaubt — die Grenze gehoert dazu"
        );
    }

    #[test]
    fn zero_font_size_is_rejected() {
        let err = parse_err(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Ident("font-size".into()),
            TokenKind::Colon,
            TokenKind::Number(0),
            TokenKind::Semicolon,
            TokenKind::RBrace,
        ]);
        assert!(err.message.contains("Schriftgroesse"), "{}", err.message);
    }

    // -- Robustheit --------------------------------------------------------

    #[test]
    fn errors_carry_a_span() {
        let tokens = vec![
            Token::new(TokenKind::Page, Span::new(0, 4)),
            Token::new(TokenKind::LBrace, Span::new(5, 1)),
            Token::new(TokenKind::Ident("unsupported".into()), Span::new(7, 11)),
            Token::new(TokenKind::Colon, Span::new(18, 1)),
            Token::new(TokenKind::Number(1), Span::new(20, 1)),
            Token::new(TokenKind::Semicolon, Span::new(21, 1)),
            Token::new(TokenKind::RBrace, Span::new(22, 1)),
            Token::new(TokenKind::Eof, Span::new(23, 0)),
        ];
        let err = parse_tokens(&tokens).expect_err("sollte scheitern");
        assert_eq!(
            err.span,
            Span::new(7, 11),
            "Span muss den Property-Namen treffen"
        );
    }

    #[test]
    fn empty_token_list_does_not_panic() {
        // Ohne Eof und ohne Tokens: synthetisches Eof, sauberer Fehler.
        let err = parse_tokens(&[]).expect_err("sollte scheitern");
        assert!(!err.message.is_empty());
    }

    #[test]
    fn token_list_without_eof_does_not_panic() {
        let tokens = vec![TokenKind::Page, TokenKind::LBrace, TokenKind::RBrace]
            .into_iter()
            .map(tok)
            .collect::<Vec<_>>();
        // Der Root ist vollstaendig, danach ist Ende -> sollte trotzdem
        // gelingen, weil das synthetische Eof als Dateiende gilt.
        let doc = parse_tokens(&tokens).expect("sollte gelingen");
        assert_eq!(doc.len(), 1);
    }

    #[test]
    fn deep_nesting_does_not_overflow() {
        // 1000 verschachtelte Container.
        let mut kinds = vec![TokenKind::Page, TokenKind::LBrace];
        for _ in 0..1000 {
            kinds.push(TokenKind::Container);
            kinds.push(TokenKind::LBrace);
        }
        for _ in 0..1000 {
            kinds.push(TokenKind::RBrace);
        }
        kinds.push(TokenKind::RBrace);
        let doc = parse_ok(kinds);
        assert_eq!(doc.count_of_kind(NodeKind::Container), 1000);
        assert!(doc.validate().is_ok());
    }

    #[test]
    fn many_statements_parse_quickly() {
        let mut kinds = vec![TokenKind::Page, TokenKind::LBrace];
        for i in 0..5000 {
            kinds.push(TokenKind::Text);
            kinds.push(TokenKind::String(format!("t{i}")));
            kinds.push(TokenKind::LBrace);
            kinds.push(TokenKind::RBrace);
        }
        kinds.push(TokenKind::RBrace);
        let doc = parse_ok(kinds);
        assert_eq!(doc.count_of_kind(NodeKind::Text), 5000);
    }

    #[test]
    fn undeclared_properties_stay_at_default() {
        // Vererbung ist nicht Aufgabe des Parsers: nicht deklarierte
        // Properties bleiben exakt auf Style::default().
        let doc = parse_ok(vec![TokenKind::Page, TokenKind::LBrace, TokenKind::RBrace]);
    // 04.10.2026 (SCHRIFT, (c)): `.clone()` — `Style` traegt jetzt
    // `schriftfamilie: String` und ist nicht mehr `Copy` (narsil-core).
        let style = doc.node(doc.root()).expect("root").style.clone();
        assert_eq!(style.color, Style::default().color);
        assert_eq!(style.font_size, Style::default().font_size);
    }

    #[test]
    fn text_node_carries_its_content() {
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Text,
            TokenKind::String("Hallo Welt".into()),
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::RBrace,
        ]);
        let text_node = doc
            .iter_depth_first()
            .find(|n| n.kind.is_text())
            .expect("Textknoten");
        assert_eq!(text_node.text.as_deref(), Some("Hallo Welt"));
    }

    #[test]
    fn container_node_has_no_text() {
        let doc = parse_ok(vec![
            TokenKind::Page,
            TokenKind::LBrace,
            TokenKind::Container,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::RBrace,
        ]);
        let container = doc
            .iter_depth_first()
            .find(|n| n.kind == NodeKind::Container)
            .expect("Container");
        assert!(container.text.is_none());
    }

    #[test]
    fn parse_tokens_is_reexported_for_tests() {
        // Sanity: die Funktion existiert und ist aufrufbar.
        let _ = parse_tokens(&empty_page()).expect("ok");
    }
}
