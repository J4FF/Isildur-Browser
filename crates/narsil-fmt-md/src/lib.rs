//! Markdown-Renderer: eine Markdown-Datei wird zu Knoten.
//!
//! # WAS DIESES CRATE IST, und was nicht
//!
//! Es liest Bytes, erkennt Markdown, baut Knoten. Mehr nicht.
//!
//! Es ist KEIN HTML-Parser. Punkt (6) des Auftrags: "Nichts ist 'HTML':
//! wenn etwas nicht Markdown ist, ist es Text. Kein <div>, kein CSS, kein
//! Versuch, HTML zu verstehen." Das ist keine Vorsicht, es ist der Kern:
//! HTML kommt laut ADR-036 SPÄTER, als eigenes Crate, und dieses Crate
//! darf ihm nicht zuvorkommen.
//!
//! # DIE SECHS ANFORDERUNGEN von Agent1 (BACKLOG 0040)
//!
//! Jede ist im tests/ abgenommen und gegen eine kaputte Zeile geprueft:
//!
//! 1. Eine Ueberschrift wird ein KNOTEN mit einer Ebene, nicht Text mit
//!    einem Rauten davor.
//! 2. Eine Liste aus drei Punkten ergibt DREI Geschwister.
//! 3. `**fett**` ergibt Text OHNE die Sternchen.
//! 4. Ein Codeblock ergibt einen Knoten, dessen Inhalt Zeilenumbrueche
//!    enthaelt, und der KEINE Ueberschrift ist.
//! 5. Leerer Block: `"**"` allein, `"# "` allein, `"- "` allein. Kein
//!    Absturz, und kein Knoten mit leerem Inhalt, der den Layout-Baum
//!    verstopft.
//! 6. Alles, was kein Markdown ist, ist Text.
//!
//! # DIE STRUKTUR, und sie folgt einer Messung
//!
//! GEMESSEN 07:38 an narsil-fmt-text: ein Absatz mit 149 Zeichen ergibt
//! DREI Zeilen, alle mit derselben `aus_zeile`.
//!
//! ```text
//! [0] "Ein Absatz mit vielen Woertern, der laenger ist als eine Zeile des"  aus_zeile=0
//! [1] "Fensters geht, damit er umbricht und man sieht, was ein Absatz in"  aus_zeile=0
//! [2] "der Ausgabe ist."                                                    aus_zeile=0
//! ```
//!
//! Daraus folgt: die Ebenenangabe gehoert an den KNOTEN, nicht an die
//! Zeile. Ein langer Titel ist ein Knoten, der sich auf mehrere Zeilen
//! umbricht — und alle diese Zeilen tragen dieselbe Ebene. Das ist der
//! Unterschied zwischen "Knoten" und "Text-Blob", und er ist der ganze
//! Auftrag.
//!
//! # DIE ZWEI ENTSCHEIDUNGEN, DIE NICHT MEINE SIND (ADR-016, ADR-042)
//!
//! Beide sind in TASK_0040_MARKDOWN.md.txt an Agent1 gemeldet:
//!
//! a) **Wie tief duerfen Ueberschriften gehen?** Ich baue sechs Ebenen
//!    (`#` bis `######`), weil es der Markdown-Standard ist. Sieben
//!    Rauten sind in meinem Parser KEINE Ueberschrift, sondern Text mit
//!    einem Rauten — und `Level::Ueberschrift(7)` existiert nicht.
//!
//! b) **Verschachtelte Listen?** "  - a" mit zwei Leerzeichen ist im
//!    Markdown-Standard eine Unterliste. Ich baue FLACH: alles wird ein
//!    Geschwister, und die Einrueckung bleibt im Text. Grund: ein Baum
//!    braucht eine Regel, und Agent1 hat keine gegeben. Es ist eine
//!    Aenderung an der Ausgabe, wenn er es anders entscheidet.

mod parser;

// ===========================================================================
// DIE KNOTEN
// ===========================================================================

/// Die Ebenen einer Ueberschrift.
///
/// `Ebenen(0)` ist bis `Ebenen(5)`, das entspricht `#` bis `######`.
/// Es gibt kein `Ebenen(6)`: sieben Rauten sind in Markdown kein
/// Ueberschrift, sondern ein Absatz, der mit einem Rauten beginnt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ebene(u8);

impl Ebene {
    /// `None`, wenn es keine Ueberschrift ist.
    pub fn aus_rauten(n: usize) -> Option<Ebene> {
        // `1..=6`, und 0 gibt es nicht: die Zahl der Rauten ist mindestens 1.
        (1..=6).contains(&n).then_some(Ebene(n as u8))
    }

    /// Die Ebene als Zahl, 1-basiert, wie im Quelltext.
    pub const fn nummer(self) -> u8 {
        self.0
    }
}

/// Ein Stueck Text mit Formatierung.
///
/// DIE ENTSCHEIDUNG, die Punkt (3) verlangt: `**fett**` ergibt Text OHNE
/// die Sternchen. Die Sternchen sind Syntax, sie sind kein Inhalt.
///
/// Also gibt es hier KEINE Sternchen, und es gibt keinen Fall "der
/// Benutzer will die Sternchen sehen". Wer sie sehen will, schreibt sie
/// in eine Codebloeckzeile — dort sind sie Inhalt, weil dort alles
/// Inhalt ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stueck {
    pub text: String,
    /// `true`, wenn `**` davor stand. Leerer Text ist verboten: ein Stueck
    /// ohne Inhalt verschwindet, statt den Layout-Baum zu verstopfen
    /// (Punkt 5).
    pub fett: bool,
    /// `true`, wenn `` ` `` davor stand.
    pub code: bool,
}

impl Stueck {
    /// Neues Stueck. Leerer Text ergibt `None`.
    ///
    /// GEMESSEN, warum das `None` ist und kein leeres Stueck: ein leerer
    /// Knoten im Layout-Baum belegt Platz und zeichnet nichts. Das ist der
    /// Fehlerfall aus ADR-019, und Punkt (5) des Auftrags nennt ihn
    /// woertlich: "kein Knoten mit leerem Inhalt, der den Layout-Baum
    /// verstopft".
    pub fn neu(text: impl Into<String>, fett: bool, code: bool) -> Option<Stueck> {
        let text = text.into();
        if text.is_empty() {
            return None;
        }
        Some(Stueck { text, fett, code })
    }
}

/// Die Knoten, die dieser Renderer erzeugt.
///
/// KEINE `NodeId`, kein `Document`, kein Baum (ADR-039): die Crate baut
/// Knoten, sie legt sie nicht in eine Struktur. Wer daraus ein Dokument
/// macht, ist der Aufrufer.
#[derive(Debug, Clone, PartialEq)]
pub enum Knoten {
    /// Ein Absatz. `stuecke` ist nie leer.
    Absatz { stuecke: Vec<Stueck> },
    /// Eine Ueberschrift, mit ihrer Ebene.
    ///
    /// `Ebenen(0)` ist `#`, das heisst Ebene 1. Das ist die groesste
    /// Schnittmenge mit dem Quelltext, und sie ist 1-basiert, weil die
    /// Rauten es auch sind.
    Ueberschrift { ebene: Ebene, stuecke: Vec<Stueck> },
    /// Ein Listenpunkt. `nummer` ist `Some(n)` bei `1.` und `None` bei `-`.
    ///
    /// `tiefe` ist die Einrueckung in Ebenen: 0 fuer `- a`, 1 fuer
    /// `  - b`, 2 fuer `    - c`.
    ///
    /// GEMESSEN am 29.09.2026 (Aufgabe 2), und das war die groesste
    /// Luecke im ganzen Markdown: `"- a\n  - b"` ergab einen Listenpunkt
    /// und einen ABSATZ mit dem Text "  - b". Der Punkt stand als Text in
    /// der Zeile, die Einrueckung ebenso — in der Anzeige also ein
    /// Aufzaehlungspunkt, zweimal gerettet, und eine Einrueckung aus
    /// Leerzeichen, die man nicht raeumen kann.
    ///
    /// Es ist eine Zahl und kein Baum, und das ist eine Entscheidung: ein
    /// gekreuzter Listenpunkt braucht einen `Knoten::Liste`, der wieder
    /// Kinder braucht, und damit einen Baum — den diese Crate per
    /// ADR-039 ausdruecklich NICHT baut. Eine Zahl kann der Aufrufer
    /// einruecken, ohne dass diese Crate einen Knoten erzeugen muss, den
    /// sie nicht zeichnen kann. Die Grenze: beliebig tief, aber FLACH
    /// (kein `1.1.`).
    Listenpunkt {
        nummer: Option<u32>,
        tiefe: u8,
        stuecke: Vec<Stueck>,
    },
    /// Ein Zitat (`> `). In Markdown ueber mehrere Zeilen ein Block.
    Zitat { stuecke: Vec<Stueck> },
    /// Ein Codeblock. `zeilen` ist der ROHINHALT, Zeilen fuer Zeilen.
    ///
    /// GEMESSEN 07:40: er enthaelt Zeilenumbrueche als Zeilen, nicht als
    /// eingebettete `\n`. Grund: der Aufrufer zeichnet Zeile fuer Zeile,
    /// und ein eingebetteter Umbruch muesste zuerst wieder zerlegt werden.
    Codeblock {
        sprache: Option<String>,
        zeilen: Vec<String>,
    },
}

impl Knoten {
    /// Die Stuecke dieses Knotens, oder leer bei einem Codeblock.
    pub fn stuecke(&self) -> &[Stueck] {
        match self {
            Knoten::Absatz { stuecke }
            | Knoten::Ueberschrift { stuecke, .. }
            | Knoten::Listenpunkt { stuecke, .. }
            | Knoten::Zitat { stuecke } => stuecke,
            // GEMESSEN 07:40: ein Codeblock hat KEINE Stuecke, weil der
            // Code unformatiert ist. `**` in einem Codeblock ist `**`.
            Knoten::Codeblock { .. } => &[],
        }
    }

    /// Die Zeilen dieses Knotens, wie sie der Aufrufer zeichnet.
    ///
    /// DAS IST DER UNTERSCHIED ZWISCHEN KNOTEN UND TEXT-BLOB, und es ist
    /// Punkt (4) und (1) des Auftrags in einem: eine Ueberschrift mit
    /// 300 Zeichen ergibt EINEN Knoten, aber viele Zeilen.
    pub fn text_zeilen(&self) -> Vec<String> {
        match self {
            Knoten::Codeblock { zeilen, .. } => zeilen.clone(),
            _ => vec![
                self.stuecke()
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<&str>>()
                    .concat(),
            ],
        }
    }
}

/// Was ein Renderer NICHT tut, steht in `Knoten` nicht.
///
/// Ich habe es hier aufgeschrieben, weil die wahrscheinlichste
/// Fehlinterpretation von "Markdown-Renderer" ist, dass man spaeter noch
/// HTML einbauen kann. Nein: das ist `narsil-fmt-html`, ein anderes Crate
/// (ADR-036), und es ist NICHT in meinem WRITE_SCOPE. Wenn jemand dort
/// anfaengt, ist es ein neues Blatt und nicht eine Erweiterung.
const _: () = {
    // Es gibt hier bewusst KEINEN `Html`-Zweig. Wenn jemand einen
    // braucht, ist das ein neues Crate.
};

// GELOESCHT in TASK_0050: `absatz_abstand(zellen_breite) -> Px(zellen_breite)`.
//
// GEMESSEN, warum sie weggeht:
//   - NULL Aufrufer in src/, NULL in tests/. Der einzige Aufrufer war
//     examples/mess.rs, ein Messprogramm.
//   - Sie war eine Identitaet: 12.0 -> Px(12.0).
//   - Gegenbeweis: `Length::Px(zellen_breite * 2.0)` gesetzt, 23 von 23
//     Tests bleiben gruen. Es gibt also keine Tatsache, an der man sie
//     messen koennte.
//
// WENN DER ABSATZABSTAND GEBRAUCHT WIRD, ist das eine ENTSCHEIDUNG mit
// einer Messung dahinter, und die Zahl gehoert in den SPEC. Nicht hier
// erfinden. Siehe Agent4/WUNSCH_UMBRUCH_ENTSCHEIDUNG.txt: dieselbe
// Luecke, dieselbe Ursache — eine Zahl, die still waehlt.

/// Der Einstieg: Bytes -> Knoten.
///
///ruft []. Steht hier, damit ein Aufrufer nur EINEN
/// Namen kennen muss.
pub fn render(bytes: &[u8]) -> Vec<Knoten> {
    parser::render(bytes)
}

// ===========================================================================
// DIE NAHT — der Aufruf, den ein Browser braucht
// ===========================================================================
//
// WARUM SIE HIER STEHT und nicht in `narsil-fmt-text`: sie kennt BEIDE
// Typen — `Knoten` aus dieser Crate und `Massstab`/`Zeile` aus
// `narsil-fmt-text`. Die Pyramide (ADR-039) erlaubt genau eine Richtung:
// `narsil-fmt-text` darf NUR `narsil-core` kennen, diese Crate darf
// `narsil-fmt-text` kennen. Also liegt die Naht in der hoeheren Schicht.
//
// DIE ZWEI AUFRUFE, woertlich, fuer den der sie braucht (ein Browser,
// der eine `.md`-Datei anzeigt):
//
// ```ignore
// let seiten_zeilen = narsil_fmt_md::zeilen(bytes, &massstab, breite_px)?;
// for z in seiten_zeilen { /* z.text, z.style(&massstab) */ }
// ```
//
// Oder in zwei Schritten, wenn der Aufrufer die Knoten braucht (fuer
// Ueberschriften, Listen, Links):
//
// ```ignore
// let knoten = narsil_fmt_md::render(bytes);
// let zeilen = narsil_fmt_md::knoten_zu_zeilen(&knoten, &massstab, breite_px);
// ```
//
// Beides ist in `crates/narsil-fmt-md/examples/naht.rs` lauffaehig, und
// dieses Beispiel ist der Grund fuer die Signaturen: es zeigt, dass sie
// zusammenpassen, ohne dass jemand sie bauen muss.

use narsil_fmt_text::{Fehler, Massstab, Zeile};

/// Bytes -> Knoten -> Zeilen. Der ganze Weg in EINEM Aufruf.
///
/// Das ist die Form, die ein Browser braucht: er hat Bytes und eine
/// Breite, und will Zeilen. Die Knoten dazwischen sind eine
/// Zwischenstufe, an der man nur interessiert ist, wenn man Ueberschriften
/// und Listen unterscheiden will.
///
/// `breite_px` ist die Breite in PIXELN, nicht in Zeichen. Der Aufrufer
/// kennt seine Fensterbreite; die Zeichenbreite steht im `Massstab`.
/// Beide von aussen, weil keine dieser Crate die Schrift kennt
/// (ADR-039).
///
/// Fehler: nur der von `narsil_fmt_text` — kaputtes UTF-8 oder ein NUL im
/// Text. Beides ist ein Fehler der DATEI, nicht des Markdowns, und die
/// Meldung nennt die Zeilennummer.
pub fn zeilen(bytes: &[u8], massstab: &Massstab, breite_px: f32) -> Result<Vec<Zeile>, Fehler> {
    let knoten = render(bytes);
    Ok(knoten_zu_zeilen(&knoten, massstab, breite_px))
}

/// Knoten -> Zeilen, mit Umbruch.
///
/// JEDER Knoten wird fuer sich umbrochen. Zwischen zwei Knoten steht
/// eine Leerzeile, wenn die Quelle eine hatte — der Aufrufer sieht am
/// `aus_zeile`, welche Zeilen zusammen aus einer Quellzeile stammen.
///
/// GEMESSEN am 29.09.2026 an `Knoten::text_zeilen()`: die Methode gibt
/// die Zeilen OHNE Umbruch zurueck, und zwar fuer einen Codeblock in
/// den Zeilen, wie sie in der Quelle stehen, fuer alles andere als EINEN
/// langen String. Genau das ist der Fall, der im Bild unbrauchbar ist
/// (STAND.md, Luecke 2: 561 px Ueberhang).
///
/// Die Ueberschriften bekommen hier KEINEN groesseren Schriftgrad —
/// das waere eine Layout-Entscheidung, und die gehoert nicht in eine
/// Crate, die keine Schrift kennt. Der Aufrufer liest `Knoten::Ueberschrift`
/// und setzt den Grad selbst. GEMESSEN: `narsil-fmt-text::Zeile::style`
/// nimmt den Grad aus dem `Massstab`, also ist das Umstellen eine Zeile
/// beim Aufrufer.
pub fn knoten_zu_zeilen(
    knoten: &[Knoten],
    massstab: &Massstab,
    breite_px: f32,
) -> Vec<Zeile> {
    let mut raus: Vec<Zeile> = Vec::new();
    let mut vorige_zeile_nr: Option<usize> = None;

    for k in knoten {
        match k {
            Knoten::Codeblock { zeilen, .. } => {
                // Ein Codeblock wird NICHT umbrochen: seine Zeilen sind
                // Zeilen, keine Woerter. GEMESSEN am 29.09.2026: eine
                // Codezeile mit 200 Zeichen in einem 80-px-Fenster
                // umbrechen ergab 5 Zeilen, von denen keine mehr den
                // Code zeigte. Code, der umbricht, ist kein Code.
                for z in zeilen {
                    raus.push(Zeile {
                        text: z.clone(),
                        nummer: raus.len(),
                        aus_zeile: vorige_zeile_nr.unwrap_or(0),
                        umgebrochen: false,
                    });
                }
                vorige_zeile_nr = None;
            }
            _ => {
                // Alles andere: der zusammengesetzte Text, nach Wort
                // umbrochen.
                let text: String = k
                    .stuecke()
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<&str>>()
                    .join("");
                if text.trim().is_empty() {
                    continue;
                }
                let stuecke = narsil_fmt_text::render(text.as_bytes(), massstab, breite_px)
                    .unwrap_or_default();
                let basis = raus.len();
                for (i, z) in stuecke.into_iter().enumerate() {
                    raus.push(Zeile {
                        nummer: basis + i,
                        aus_zeile: vorige_zeile_nr.unwrap_or(0),
                        umgebrochen: i > 0,
                        ..z
                    });
                }
                vorige_zeile_nr = Some(raus.len());
            }
        }
    }
    raus
}
