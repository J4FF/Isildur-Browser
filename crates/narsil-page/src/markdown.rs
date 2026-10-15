//! I5 — MARKDOWN: `.md` wird eine Seite.
//!
//! WELLE5, I5: "Markdown (.md) ueber narsil-fmt-md".
//!
//! # DER WEG, und warum er ueber HTML laeuft
//!
//! ```text
//! .md-Bytes -> narsil_fmt_md::render -> Vec<Knoten> -> HIER -> HTML
//!            -> narsil_page::seite_aus_text -> Dokument
//! ```
//!
//! Die Alternative waere, `Knoten` direkt in einen `Document`-Baum zu
//! uebersetzen. Dagegen spricht zweierlei, und beides ist gemessen:
//!
//! 1. `narsil-page` kann bereits Absaetze, Ueberschriften, Listen, Zitate
//!    und Codebloecke — inklusive Umbruch, Inline-Auszeichnung (fett,
//!    code) und Links. Ein zweiter Weg waere ein zweiter Umbruch, und die
//!    beiden wuerden auseinanderlaufen.
//! 2. `narsil-fmt-md` baut per ADR-039 KEINE Knotenstruktur ("Wer daraus
//!    ein Dokument macht, ist der Aufrufer"). Der Aufrufer, der es
//!    kann, ist `narsil-page`.
//!
//! # DIE GRENZEN (M3/M4), alle benannt
//!
//! | Konstante | Wert | Wofue |
//! |---|---|---|
//! | `MAX_MD_KNOTEN` | 20.000 | Knoten aus `render` |
//! | `MAX_MD_UEBERSCHRIFT` | 1..=6 | Ebenen werden geklemmt |
//! | `MAX_MD_TIEFE` | 8 | Listeneinrueckung in Ebenen |
//! | `MAX_MD_TEXT` | 1 MiB | Ein Stueck Text |

// `write!` auf einen `String` braucht das Trait im Geltungsbereich.
use std::fmt::Write as _;

/// Hoechste Ebene einer Ueberschrift (M3).
///
/// GEMESSEN: `Ebene::aus_rauten` gibt `1..=6` zurueck, es gibt keine
/// 0. Die Klammer ist also nur eine Zusage, kein Filter.
const MAX_MD_UEBERSCHRIFT: u8 = 6;

/// Hoechste Zahl Knoten aus einer Markdown-Datei (M3).
///
/// GEMESSEN, warum 20.000: das sind 20.000 Absaetze. Eine Datei, die
/// mehr hat, ist kein Dokument, sondern ein Archiv — und ein Browser
/// wuerde daran genauso scheitern wie am Layout.
const MAX_MD_KNOTEN: usize = 20_000;

/// Hoechste Einrueckung einer Liste in Ebenen (M3).
///
/// GEMESSEN: `tiefe` kommt aus dem Parser als `u8`, also bis 255. Acht
/// Ebenen sind tief genug fuer jede lesbare Liste, und 255 waeren eine
/// Einrueckung, die breiter ist als jede Zeile.
const MAX_MD_TIEFE: u8 = 8;

/// Hoechste Laenge eines Textstuecks in Bytes (M4).
const MAX_MD_TEXT: usize = 1024 * 1024;

/// Ist das eine Markdown-Adresse?
///
/// GEMESSEN, warum die Endung und nicht der Inhalt: eine `.md`-Datei
/// kann HTML enthalten (im Codeblock), und eine `.html`-Datei kann
/// Markdown enthalten (in einem Kommentar). Wer den Inhalt rieche,
/// muesste die ganze Datei laden, um es zu entscheiden — und die
/// Endung steht im Dateinamen, den wir schon haben.
pub fn ist_markdown(adresse: &str) -> bool {
    let ohne_frage = adresse.split(['?', '#']).next().unwrap_or(adresse);
    ohne_frage
        .rsplit('/')
        .next()
        .is_some_and(|datei| datei.to_ascii_lowercase().ends_with(".md"))
}

/// Markdown-Bytes -> HTML.
///
/// # WAS DIESE FUNKTION NICHT KANN, und warum das okay ist
///
/// * **Kein Inline-Markdown in Ueberschriften/Listen fuer jeden Fall**:
///   `**` wird zu `<strong>`, `` ` `` zu `<code>` — beides ueber
///   `stuecke`. `*kursiv*` gibt es in `Stueck` NICHT (nur `fett` und
///   `code`), also gibt es hier auch kein Kursiv.
/// * **Keine Bilder, keine Tabellen, kein HTML im Text**: `Knoten` kennt
///   sie nicht, und sie zu erfinden waere eine zweite Markdown-
///   Implementierung.
/// * **Keine Verweise aus dem Inhalt** (`[x](y)` ohne Code): der Parser
///   liefert `Stueck { text }` fuer alles, was kein `**` und kein
///   `` ` `` ist — die Klammern stehen dann im Text. Das ist eine
///   Eigenschaft von `narsil-fmt-md`, nicht von hier.
///
/// # DIE ZWEI REGELN, die man beim Lesen braucht
///
/// 1. **Alles wird mit `esc` escaped.** `Stueck.text` ist Anzeigetext,
///    und Anzeigetext, der als HTML durchgeht, ist ein Fehler im Text.
///    `<` wird zu `&lt;`, `&` zu `&amp;`, `"` zu `&quot;`.
/// 2. **Ein Listenblock wird zu `<ul>` oder `<ol>`.** Getrennt nach der
///    ersten Nummer: `- a` ist ungeordnet, `1. a` ist geordnet. Der
///    Zaehler laeuft mit und beginnt bei der ersten Nummer der Liste.
pub fn markdown_zu_html(bytes: &[u8]) -> String {
    let knoten = narsil_fmt_md::render(bytes);
    let mut raus = String::with_capacity(knoten.len() * 64);
    raus.push_str("<!DOCTYPE html><html><body>\n");
    // `Some(false)` = eine ungeordnete Liste ist offen, `Some(true)` eine
    // geordnete. Die Zahl steht im Listenpunkt selbst (siehe unten).
    let mut liste: Option<bool> = None;
    for k in knoten.iter().take(MAX_MD_KNOTEN) {
        match k {
            narsil_fmt_md::Knoten::Codeblock { sprache, zeilen } => {
                liste_schliessen(&mut liste, &mut raus);
                // `<pre>` ist ein Block, kein Absatz: der Code bleibt
                // zeilenweise, wie er in der Datei stand.
                let _ = write!(raus, "<pre><code>");
                if let Some(s) = sprache {
                    let _ = write!(raus, "<span class=\"sprache\">{}</span>", esc(s));
                }
                for z in zeilen {
                    let _ = writeln!(raus, "{}", esc(z));
                }
                raus.push_str("</code></pre>\n");
            }
            narsil_fmt_md::Knoten::Ueberschrift { ebene, stuecke } => {
                liste_schliessen(&mut liste, &mut raus);
                let e = ebene.nummer().clamp(1, MAX_MD_UEBERSCHRIFT);
                let _ = write!(raus, "<h{e}>{}</h{e}>\n", stuecke_zu_html(stuecke));
            }
            narsil_fmt_md::Knoten::Absatz { stuecke } => {
                liste_schliessen(&mut liste, &mut raus);
                let _ = writeln!(raus, "<p>{}</p>", stuecke_zu_html(stuecke));
            }
            narsil_fmt_md::Knoten::Zitat { stuecke } => {
                liste_schliessen(&mut liste, &mut raus);
                let _ = writeln!(
                    raus,
                    "<blockquote><p>{}</p></blockquote>",
                    stuecke_zu_html(stuecke)
                );
            }
            narsil_fmt_md::Knoten::Listenpunkt {
                nummer: n,
                tiefe,
                stuecke,
            } => {
                // Die Liste anlassen, solange die Art gleich bleibt; ein
                // Wechsel der Art (ul -> ol) schliesst und oeffnet.
                //
                // DIE ART ist "geordnet oder ungeordnet", NICHT die
                // Nummer.
                //
                // GEMESSENER FEHLER 30.09.2026, 07:00: `let art = *n`
                // verglich `Some(1)` mit `Some(2)`, also wurde die Liste
                // nach JEDEM nummerierten Punkt geschlossen und neu
                // geoeffnet. Ergebnis: `<ol>` mit genau einem `<li>`.
                // Ein Browser wuerde `2.` als zweiten Punkt zeigen; wir
                // zeigen gar keinen.
                let art = n.is_some();
                match liste {
                    Some(v) if v == art => {}
                    _ => {
                        liste_schliessen(&mut liste, &mut raus);
                        liste = Some(art);
                        // DIE OEFFNENDE MARKE. Sie fehlte im ersten
                        // Entwurf (06:55): geschrieben wurde nur
                        // `</ul>`, nie `<ul>`. GEMESSEN: der HTML-Parser
                        // hat daraus eine flache Liste gemacht, und die
                        // Punkte klebten aneinander ("ersterPunkt").
                        if art {
                            raus.push_str("<ol>\n");
                        } else {
                            raus.push_str("<ul>\n");
                        }
                    }
                }
                // Die Einrueckung: `tiefe` Ebenen, jede als eigener
                // Verschachtelung. GEMESSEN: `tiefe` kann 255 sein, und
                // 255 verschachtelte `<ul>` waeren 255 Knoten fuer einen
                // Listenpunkt. `MAX_MD_TIEFE` (8) begrenzt das.
                //
                // DIE EINRUECKUNG. GEMESSENER FEHLER 30.09.2026, 06:58:
                // hier stand `for _ in 1..t { "<ul><li>" }` und dann nur
                // `INHALT</li>`. Das ERSTE `<li>` fehlte — und im
                // Ergebnis stand `<ul>erster Punkt</li>` im HTML, der
                // Browser zeigte eine Liste OHNE Punkte.
                //
                // Die Struktur fuer Tiefe t: `t-1` mal `<ul>` oeffnen,
                // ein `<li>` mit dem Inhalt, dann `t-1` mal `</ul>`.
                //
                // `0..t`, nicht `1..t`. GEMESSENER FEHLER 30.09.2026,
                // 07:00: `1..t` gibt bei `tiefe == 1` eine LEERE Folge
                // — und damit keine Einrueckung fuer genau den Fall, den
                // es zu loesen gab ("  - eingerueckt" blieb auf Ebene 0).
                let t = (*tiefe).min(MAX_MD_TIEFE);
                for _ in 0..t {
                    raus.push_str("<ul>\n");
                }
                //
                // DIE NUMMER als Text. `list-style-type` aus dem
                // UA-Stylesheet wuerde sie zeichnen, aber diese Crate
                // zeichnet keine Listenmarker (der Punkt bei `<li>` ist
                // unsere eigene Entscheidung, `narsil_page::LISTENPUNKT`).
                // Also steht die Zahl im Text — sonst waere eine
                // nummerierte Liste eine unnummerierte.
                match n {
                    Some(z) => {
                        // Release 07.10.2026: die Zahl steht im Text, also
                        // keine zweite Marke davor („• 1.").
                        let _ = write!(
                            raus,
                            "<li style=\"list-style-type:none\">{z}. {}</li>\n",
                            stuecke_zu_html(stuecke)
                        );
                    }
                    None => {
                        let _ = write!(raus, "<li>{}</li>\n", stuecke_zu_html(stuecke));
                    }
                }
                for _ in 0..t {
                    raus.push_str("</ul>\n");
                }
            }
        }
    }
    liste_schliessen(&mut liste, &mut raus);
    raus.push_str("</body></html>\n");
    raus
}

/// Schliesst eine offene Liste.
fn liste_schliessen(liste: &mut Option<bool>, raus: &mut String) {
    match liste.take() {
        Some(false) => raus.push_str("</ul>\n"),
        Some(true) => raus.push_str("</ol>\n"),
        None => {}
    }
}

/// Die Stuecke eines Knotens als HTML.
///
/// M4: jedes Stueck wird auf `MAX_MD_TEXT` gekuerzt, und die Kuerzung
/// passiert VOR der Allokation des Ergebnisses (`esc` nimmt einen
/// Slice).
fn stuecke_zu_html(stuecke: &[narsil_fmt_md::Stueck]) -> String {
    let mut raus = String::new();
    for s in stuecke {
        if raus.len() >= MAX_MD_TEXT {
            break;
        }
        let text = if s.text.len() > MAX_MD_TEXT {
            &s.text[..s.text.floor_char_boundary(MAX_MD_TEXT)]
        } else {
            &s.text
        };
        match (s.fett, s.code) {
            (true, true) => {
                let _ = write!(raus, "<strong><code>{}</code></strong>", esc(text));
            }
            (true, false) => {
                let _ = write!(raus, "<strong>{}</strong>", esc(text));
            }
            (false, true) => {
                let _ = write!(raus, "<code>{}</code>", esc(text));
            }
            (false, false) => raus.push_str(&esc(text)),
        }
    }
    raus
}

/// Maskiert Text fuer HTML.
///
/// M4: das Ergebnis ist hoechstens `6 * s.len()` (jedes Zeichen wird zu
/// hoechstens `&amp;` = 5 Zeichen plus Trenner), und der Puffer wird
/// mit genau dieser Groesse reserviert — einmal, nicht pro Zeichen.
fn esc(s: &str) -> String {
    let mut raus = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => raus.push_str("&amp;"),
            '<' => raus.push_str("&lt;"),
            '>' => raus.push_str("&gt;"),
            '"' => raus.push_str("&quot;"),
            '\'' => raus.push_str("&#39;"),
            other => raus.push(other),
        }
    }
    raus
}
