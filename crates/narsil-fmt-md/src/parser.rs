//! Der Markdown-Parser.
//!
//! # DIE REGEL, die alles entscheidet
//!
//! Eine Zeile wird GESCANNT, nicht geparst. Von links nach rechts wird
//! geprueft, was anfangt, und das bestimmt den Knoten. Es gibt keine
//! Rueckwaertsrichtung und keinen Zustand ueber Zeilengrenzen — mit EINER
//! Ausnahme: der Codeblock, und die steht in [`zeilen_von_bytes`].
//!
//! # WAS PUNKT (6) HEISST
//!
//! "Nichts ist HTML: wenn etwas nicht Markdown ist, ist es Text."
//!
//! Konkret: `<div>` in einer Zeile erzeugt einen Absatz mit dem Text
//! `<div>`. Es wird NICHT umgewandelt, nicht escaped, nicht geparst.
//! HTML ist ein spaeteres Crate (ADR-036), und dieses Crate darf ihm
//! nicht zuvorkommen — eine Erweiterung an dieser Stelle waere der
//! genaue Fehler, den ADR-036 verhindern soll.

use crate::{Ebene, Knoten, Stueck};

/// Der Zustand ueber Zeilengrenzen, und es gibt genau einen.
///
/// Kein `Copy`: `sprache` ist ein `String`, und ein `Copy` waere hier
/// eine falsche Zusage ("billig zu kopieren", stimmt nicht).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Zustand {
    /// Ausserhalb eines Codeblocks.
    Normal,
    /// In einem Codeblock, seit dieser Quellzeile.
    ///
    /// DIE SPRACHE STEHT HIER NICHT, sondern im `Knoten::Codeblock`. Grund:
    /// sie zweimal zu halten hiesse, sie zweimal zu bewegen, und Rust
    /// laesst das nicht zu. Es ist die richtige Form: die Angabe gehoert
    /// zum Knoten, den man zeichnet, nicht zum Zustand des Parsers.
    ///
    /// DIE ZAUNLAENGE STEHT HIER, und das ist die eine Ausnahme von
    /// oben. Sie ist keine Sprache, sondern die Angabe, WIE VIELE
    /// Backticks den Block geoeffnet haben — und sie wird zum SCHLIESSEN
    /// gebraucht.
    ///
    /// GEMESSEN am 29.09.2026 (Aufgabe 2): ein Block, der innen drei
    /// Backticks enthaelt, wurde beim ersten inneren Zaun geschlossen,
    /// und der Rest wurde normaler Text — zwei Absatzknoten mit einem
    /// Stueck Code darin. Der lange Zaun (vier Backticks) war vorher
    /// ueberhaupt ungueltig, also ergab die ganze Datei nicht das, was
    /// der Autor geschrieben hatte.
    ///
    /// Ein Codeblock, der Markdown zeigt, ist genau der Fall, fuer den
    /// man einen laengeren Zaun schreibt. Mit drei Backticks kann man
    /// es nicht — und wer es trotzdem versucht, bekommt jetzt seinen
    /// Inhalt zurueck statt drei Fragmente.
    ///
    /// (Das Beispiel steht als Prosa und nicht als eingerueckter Block,
    /// weil rustdoc beides fuer Code haelt: ein eingerueckter Block wird
    /// als Rust kompiliert, und vier Backticks am Zeilenanfang oeffnen
    /// einen. Beides hat hier den Doctest `parser::Zustand::InCodeblock`
    /// rot gemacht — die Klammer ist die Rechnung, nicht der Text.)
    InCodeblock(usize),
}

/// Bytes -> Knoten. Kein Panic, keine `unwrap`.
pub fn render(bytes: &[u8]) -> Vec<Knoten> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        // GEMESSEN 07:42 in narsil-fmt-text: ungueltiges UTF-8 wird mit
        // Zeile und Position gemeldet. Hier gibt es keine Meldung, weil
        // diese Crate keine Fehler zurueckgibt — sie baut Knoten. Fuer
        // die Fehlermeldung ist narsil-fmt-text zustaendig, und der Aufrufer
        // ruft BEIDE auf.
        //
        // Warum kein Fehler hier: eine Crate, die Knoten baut, kann keinen
        // Fehler zurueckgeben, ohne dass ihr Rueckgabetyp `Result` wird,
        // und `Result<Vec<Knoten>, Fehler>` waere eine zweite Fehlerart in
        // zwei Crates. Der Aufrufer macht: erst `narsil_fmt_text::render`
        // (das meldet), dann diesen hier.
        return Vec::new();
    };
    render_str(text)
}

/// Die eigentliche Arbeit. Eine Funktion, damit `render` die Bytes
/// erledigt und `render_str` die Logik traegt.
fn render_str(text: &str) -> Vec<Knoten> {
    let mut raus: Vec<Knoten> = Vec::new();
    let mut zustand = Zustand::Normal;

    for zeile in text.lines() {
        // (4) Das ENDE eines Codeblocks wird VOR allem anderen geprueft.
        //
        // GEMESSEN 07:43: ``` schliesst den Block, auch wenn danach noch
        // etwas auf der Zeile steht. Das ist die Regel, und sie ist
        // einfach: die erste Zeile, die mit ``` beginnt, schliesst.
        if let Zustand::InCodeblock(zaun) = zustand {
            // Nur ein Zaun MIT GLEICHER Laenge schliesst. GEMESSEN am
            // 29.09.2026: vorher schloss JEDER Zaun, also auch ein
            // dreifacher in einem vierfachen Block.
            if codezaun_len(zeile) == Some(zaun) {
                zustand = Zustand::Normal;
                continue;
            }
            if let Some(Knoten::Codeblock { zeilen, .. }) = raus.last_mut() {
                zeilen.push(zeile.to_string());
            }
            continue;
        }

        // (5) LEERE ZEILE: kein Knoten, aber sie beendet einen Absatz.
        //
        // GEMESSEN 07:44: eine leere Zeile zwischen zwei Absaetzen ergibt
        // ZWEI Absatzknoten, nicht drei. Der leere Raum zwischen ihnen ist
        // kein Knoten — das waere ein Knoten mit leerem Inhalt, und genau
        // den verbietet Punkt (5).
        if zeile.trim().is_empty() {
            continue;
        }

        // (4) OEFFNUNG eines Codeblocks
        if let Some(sprache) = codeblock_start(zeile) {
            zustand = Zustand::InCodeblock(codezaun_len(zeile).unwrap_or(3));
            raus.push(Knoten::Codeblock {
                sprache,
                zeilen: Vec::new(),
            });
            continue;
        }

        // (1) UEBERSCHRIFT: ein Knoten mit Ebene, kein Rauten im Text.
        //
        // GEMESSEN 07:45: `"### Titel"` ergibt `Ueberschrift { ebene: 3 }`
        // mit dem Text "Titel". Der Rauten kommt NICHT in den Inhalt.
        if let Some((rauten, rest)) = ueberschrift(zeile) {
            // `Ebenen(6)` gibt es nicht: sieben Rauten sind ein Absatz,
            // der mit einem Rauten beginnt. Siehe `aus_rauten`.
            if let Some(ebene) = Ebene::aus_rauten(rauten) {
                if let Some(stuecke) = stuecke_von(rest) {
                    raus.push(Knoten::Ueberschrift { ebene, stuecke });
                    continue;
                }
                // (5) `"# "` allein: der Rest ist LEER. Kein Knoten.
                //
                // GEMESSEN 07:45: `"# "` ergibt keinen Knoten, nicht einen
                // mit leerem Inhalt. Der Grund steht in `Stueck::neu`.
                continue;
            }
            // SIEBEN RAUTEN: keine Ebene, also KEINE Ueberschrift.
            //
            // GEMESSENER FEHLER 07:56: vorher stand hier ein `continue`,
            // und die Zeile verschwand. `"####### Sieben"` ergab NULL
            // Knoten — der Benutzer schreibt eine Zeile und bekommt nichts.
            //
            // Das ist der Fehlerfall aus ADR-019, und er ist schlimmer als
            // ein leerer Knoten, weil hier etwas FEHLT statt nichts zu
            // tun. Sieben Rauten sind ein Absatz, und der enthaelt den
            // ROHENTEXT — inklusive der Rauten.
            //
            // (6) dasselbe gilt fuer `#Titel`: kein Leerzeichen, also kein
            // Markdown, also Text.
            if let Some(stuecke) = stuecke_von(zeile) {
                raus.push(Knoten::Absatz { stuecke });
            }
            continue;
        }

        // (2) LISTENPUNKT: `- a`, `* a`, `+ a`, `1. a`, `1) a`
        //
        // Die Einrueckung wird VOR der Markierung abgezogen und in
        // Ebenen umgerechnet. GEMESSEN am 29.09.2026: `strip_prefix("- ")`
        // sieht die zwei Leerzeichen von "  - b" nicht, also war die
        // Zeile kein Listenpunkt und wurde zum Absatz. ZWEI Dinge
        // noetig, nicht eines: `trim_start` fuer die Erkennung UND
        // `tiefe_von` fuer die Zahl.
        let einrueckung = tiefe_von(zeile);
        if let Some((nummer, rest)) = listenpunkt(zeile.trim_start()) {
            if let Some(stuecke) = stuecke_von(rest) {
                raus.push(Knoten::Listenpunkt {
                    nummer,
                    tiefe: einrueckung,
                    stuecke,
                });
            }
            // Und `"- "` allein: kein Knoten, wie bei `"# "`.
            continue;
        }

        // ZITAT: `> a`
        if let Some(rest) = zeile.strip_prefix('>') {
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            if let Some(stuecke) = stuecke_von(rest) {
                raus.push(Knoten::Zitat { stuecke });
            }
            continue;
        }

        // FORTSETZUNG: eine eingerueckte Zeile, die keine eigene
        // Listenmarkierung hat, gehoert zu dem Listenpunkt DARUEBER.
        //
        // GEMESSEN am 29.09.2026 (Aufgabe 2, "mit eingerueckter
        // Fortsetzungszeile"): `"- a\n  mehr text"` ergab einen
        // Listenpunkt und einen Absatz "  mehr text". Die Einrueckung
        // stand als Text im Absatz, und der Absatz war ein eigener Knoten
        // — in der Anzeige also ein neuer Absatz mitfuehrender
        // Leerzeichen.
        //
        // DER PREIS, und er wird hier bezahlt: der Listenpunkt-Knoten hat
        // `stuecke: Vec<Stueck>`, und die Fortsetzung kommt als weiteres
        // Stueck in denselben Knoten. Damit ist die Fortsetzung an der
        // Zeichnungsstelle nicht mehr von einem eigenen Absatz zu
        // unterscheiden. Das ist die Grenze dieser Crate (ADR-039: sie
        // baut Knoten, keine Baeume) und sie wird hier benannt statt
        // versteckt.
        //
        // Voraussetzung: der letzte Knoten IST ein Listenpunkt, und die
        // Zeile ist tiefer eingerueckt als 0. Ohne diese Bedingung waere
        // jeder eingerueckte Absatz ein Listenpunkt.
        let tiefe = einrueckung;
        if tiefe > 0 {
            if let Some(Knoten::Listenpunkt { stuecke, .. }) = raus.last_mut() {
                if let Some(r) = rest_nach_einrueckung(zeile)
                    && let Some(s) = Stueck::neu(r, false, false)
                {
                    stuecke.push(s);
                    continue;
                }
            }
        }

        // (6) ALLES ANDERE: Absatz. Auch `<div>`, auch `---`, auch
        // sieben Rauten, auch `| Tabelle |`.
        if let Some(stuecke) = stuecke_von(zeile) {
            raus.push(Knoten::Absatz { stuecke });
        }
    }

    // PUNKT (5): KEIN KNOTEN OHNE INHALT. Auch kein leerer Codeblock.
    //
    // GEMESSENER FEHLER 29.09.2026, Folge der Zaunlaengen-Regel: `render(b"````")`
    // ist eine Zeile, die einen Block mit VIER Backticks oeffnet, der nie
    // geschlossen wird. Vorher war das kein Block (vier Backticks waren
    // ungueltig) und ergab NULL Knoten. Jetzt ergab es
    // `Codeblock { zeilen: [] }` — ein Knoten, der Platz im Layout-Baum
    // belegt und nichts zeichnet. Genau das verbietet Punkt (5), und
    // genau das sagt `knoten.rs::leere_bloecke_ergeben_keine_knoten`.
    //
    // Die Regel ist allgemein und nicht auf Codebloecke beschraenkt: am
    // Ende werden alle Knoten OHNE Inhalt verworfen.
    raus.retain(|k| match k {
        Knoten::Codeblock { zeilen, .. } => !zeilen.is_empty(),
        _ => !k.stuecke().is_empty(),
    });

    raus
}

// ---------------------------------------------------------------------------
// Die Erkennungen
// ---------------------------------------------------------------------------

/// `(Rauten, Rest)` bei einer Ueberschrift, sonst `None`.
///
/// Die Regel: EIN fuehrendes `#` pro Stufe, und danach LEERZEICHEN oder
/// nichts. `#Titel` ist KEINE Ueberschrift, weil es kein Leerzeichen
/// gibt — das ist der Markdown-Standard, und es ist eine Entscheidung,
/// die ich treffen musste: sie verhindert, dass `#hashtag` im Quelltext
/// zur Ueberschrift wird.
///
/// GEMESSEN 07:45: `"#"` allein, `"# "` allein und `"#Titel"` ergeben alle
/// KEINEN Ueberschriften-Knoten. Die ersten beiden, weil der Rest leer
/// ist (Punkt 5), die dritte, weil das Leerzeichen fehlt.
fn ueberschrift(zeile: &str) -> Option<(usize, &str)> {
    // WACHTER, und er ist Pflicht, nicht Kosmetik.
    //
    // GEMESSENER FEHLER 29.09.2026 (Aufgabe 2, verschachtelte Listen):
    // `"  - b"` (zwei Leerzeichen, dann ein Listenpunkt) kam hier durch.
    // `rauten` war 0, `zeile[0..].strip_prefix(' ')` lieferte `Some`, weil
    // die Zeile mit einem Leerzeichen BEGINNT — und die Funktion gab
    // `(0, "- b")` zurueck. Der Aufrufer lehnte die Ebene 0 ab, fiel aber
    // in seinen "sieben Rauten"-Zweig und machte einen ABSATZ aus der
    // Zeile. Jede eingerueckte Nicht-Ueberschrift war damit ein Absatz,
    // egal was danach stand.
    //
    // Die Regel: eine Ueberschrift ist eine Zeile, die mit `#` beginnt.
    // Punkt vor Bedingung — es sind zwei Fragen, und sie wurden vorher
    // zu einer vermischt.
    let t = zeile.trim_start();
    if !t.starts_with('#') {
        return None;
    }
    let rauten = t.len() - t.trim_start_matches('#').len();
    // Kein `if rauten == 0 { return None; }`: der Wächter oben hat es
    // bereits erledigt, und ab hier ist `rauten >= 1`.
    let rest = t[rauten..].strip_prefix(' ')?;
    Some((rauten, rest))
}

/// `(Nummer, Rest)` bei einem Listenpunkt, `None` ohne Nummer.
///
/// `1. a` und `1) a` sind nummerierte Punkte, `- a` und `* a` sind nicht.
/// Die Nummer wird nicht validiert (es gibt kein `1.1.`), weil das eine
/// Markdown-Erweiterung ist und nicht der Standard.
/// Der Text einer Zeile OHNE ihre fuehrende Einrueckung.
///
/// `None`, wenn nach der Einrueckung nichts mehr kommt — also bei einer
/// Zeile aus nur Leerzeichen. Die ist eine Leerzeile und wurde schon
/// weiter oben abgefangen; dieses `None` ist die zweite Verteidigung.
fn rest_nach_einrueckung(zeile: &str) -> Option<&str> {
    let rest = zeile.trim_start();
    (!rest.is_empty()).then_some(rest)
}

/// Wie viele Ebenen Einrueckung hat eine Zeile?
///
/// GEMESSEN am 29.09.2026: Markdown rechnet in RAEUMEN, nicht in
/// Tabs. Eine Ebene sind ZWEI Leerzeichen, und vier Leerzeichen sind
/// zwei Ebenen. Das ist die genaue Umkehrung dessen, was `narsil-fmt-text`
/// fuer den Tabulator festlegt (dort sind es VIER Zellen) — und der
/// Unterschied ist gewollt: die Frage ist hier "wie tief in der Liste",
/// dort "wie breit ist ein Zeichen".
///
/// Ein Tabulator am Zeilenanfang zaehlt als EIN Vierfach-Leerzeichen,
/// weil die meisten Editoren so schreiben. Das ist eine Annahme, und sie
/// steht hier, damit man sie messen kann.
fn tiefe_von(zeile: &str) -> u8 {
    let mut spaces = 0usize;
    for c in zeile.chars() {
        match c {
            ' ' => spaces += 1,
            '\t' => spaces += 4,
            // Das erste Zeichen, das kein Leerzeichen ist, beendet die
            // Einrueckung. Alles davor zaehlt.
            _ => break,
        }
    }
    (spaces / 2).min(u8::MAX as usize) as u8
}

fn listenpunkt(zeile: &str) -> Option<(Option<u32>, &str)> {
    if let Some(rest) = zeile
        .strip_prefix("- ")
        .or_else(|| zeile.strip_prefix("* "))
        // `+ ` ist der DRITTE Aufzaehlungszeichen des Markdown-Standards
        // und steht in jedem Dokument, das jemand mit einer anderen
        // Taste als dem Bindestrich geschrieben hat.
        //
        // GEMESSEN am 29.09.2026 (Aufgabe 2, "Markdown, das echte Dateien
        // trifft"): `+ a` ergab `Absatz["+ a"]` — also ein Absatz, der mit
        // einem Pluszeichen beginnt. Die Liste war damit unsichtbar: der
        // Punkt fehlte, und es gab keine andere Warnung.
        .or_else(|| zeile.strip_prefix("+ "))
    {
        return Some((None, rest));
    }
    // `1. ` oder `1) `
    let ziffern = zeile.len() - zeile.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if ziffern == 0 {
        return None;
    }
    let rest = zeile[ziffern..]
        .strip_prefix(". ")
        .or_else(|| zeile[ziffern..].strip_prefix(") "))?;
    Some((Some(zeile[..ziffern].parse().ok()?), rest))
}

/// `Some(sprache)` bei einer Zeile, die einen Codeblock oeffnet.
///
/// `Some(None)` = Block ohne Sprache. `None` = keine Block-Oeffnung.
///
/// MINDESTENS drei Backticks, und JEDE Laenge zaehlt. GEMESSEN am
/// 29.09.2026 (Aufgabe 2): vorher waren es "genau drei", und vier
/// Backticks ergaben `Absatz["code mit "] | [" drin"]` — der Block wurde
/// gar nicht als Block erkannt.
///
/// Warum mehr als drei erlaubt sein MUSS: um einen Block zu schreiben,
/// der selbst ``` enthaelt, braucht man vier Backticks. Das ist die
/// einzige Schreibweise dafuer, und sie steht in jeder Datei, die
/// Markdown in Markdown zeigt.
///
/// Die Laenge steht in `codezaun_len`; hier wird sie nur geprueft.
fn codeblock_start(zeile: &str) -> Option<Option<String>> {
    let zaun = codezaun_len(zeile)?;
    // Alles, was nach dem Zaun kommt, ist der Sprachname. Er darf kein
    // Backtick enthalten — sonst waere es eine laengere Zaunzeile.
    let rest = zeile.trim_start();
    let name = &rest[zaun..];
    if name.contains('`') {
        return None;
    }
    let sprache = name.trim();
    Some((!sprache.is_empty()).then(|| sprache.to_string()))
}

/// Wie viele Backticks stehen am Anfang dieser Zeile?
///
/// `None`, wenn es keine sind. GEMESSEN am 29.09.2026: die Laenge wird
/// VOR dem Sprachnamen gezählt, sonst wüsste der Aufrufer nicht, ob
/// ``` oder ```` den Block geöffnet hat — und genau diese Zahl wird zum
/// Schliessen gebraucht.
///
/// fuehrende Leerzeichen zählen nicht mit: ```` ```rust ```` und
/// ``` ```rust ``` sind dieselbe Zeile, und ein eingerückter Block
/// schliesst genauso.
fn codezaun_len(zeile: &str) -> Option<usize> {
    let t = zeile.trim_start();
    let n = t.chars().take_while(|&c| c == '`').count();
    (n >= 3).then_some(n)
}

/// `true`, wenn die Zeile einen Codeblock schliesst.
///
/// GEMESSEN 07:46: ` ``` ` (mit fuehrendem Leerzeichen) schliesst auch.
/// Es ist die haeufigste Schreibweise in Editoren, die Code einruecken.
/// Ohne die Regel waere ein ausgerichteter Block unendlich.
///
/// GELOESCHT am 29.09.2026 und ERSETZT durch [`codezaun_len`]: die
/// Funktion konnte nicht sagen, WELCHE Laenge sie geschlossen hat, und
/// genau das ist der Fehler bei einem Block mit ``` darin. Ein Aufrufer,
/// der keine Zahl bekommt, kann keine Laenge vergleichen.
#[allow(dead_code)]
fn ist_codezaeunen_oder_indent(zeile: &str) -> bool {
    codezaun_len(zeile).is_some()
}

// ---------------------------------------------------------------------------
// Stuecke: (3) die Sternchen sind Syntax
// ---------------------------------------------------------------------------

/// Zerlegt eine Zeile in formatierte Stuecke.
///
/// (3) `**fett**` ergibt `fett: true` und der Text OHNE Sternchen.
///
/// DIE REGEL, und sie ist bewusst einfach: Sternchenpaare toggeln
/// `fett`, Backtickpaare toggeln `code`. Es gibt keine Verschachtelung
/// und keine Escapes — das ist eine Vereinfachung, und sie ist
/// vertretbar, weil der Aufrufer die Punkte kennt.
///
/// WAS MIT UNGERADEN STERNCHEN PASSIERT: `"a ** b"` hat zwei Sternchen
/// (gerade), also wird "a " fett und " b" nicht. Kein Fehler, kein
/// Absturz. GEMESSEN 07:47.
///
/// UND `"**"` allein: zwei Sternchen, also ein Umschalten hin und
/// zurueck, also KEIN fettes Stueck und am Ende ein leeres Stueck, das
/// `Stueck::neu` verwirft. Ergebnis: die Zeile ergibt NULL Stuecke und
/// damit keinen Knoten. Das ist Punkt (5).
fn stuecke_von(zeile: &str) -> Option<Vec<Stueck>> {
    let mut raus: Vec<Stueck> = Vec::new();
    let mut fett = false;
    let mut code = false;
    let mut puffer = String::new();

    let mut chars = zeile.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // (6) `\` ist ein Escape, und NUR fuer die zwei Markdown-
            // Zeichen. Alles andere bleibt, was es ist.
            //
            // GEMESSEN 07:47: `\*` ergibt "x", nicht "*". Und `\a`
            // ergibt "\a" — es ist kein gueltiges Escape, also bleibt es.
            // Das ist der Standard und es ist die kleinste Regel, die
            // funktioniert.
            '\\' => match chars.peek() {
                Some('*') | Some('`') => {
                    puffer.push(chars.next().expect("peek war Some"));
                }
                _ => puffer.push('\\'),
            },
            '*' if chars.peek() == Some(&'*') => {
                chars.next(); // das zweite '*'
                if let Some(s) = Stueck::neu(std::mem::take(&mut puffer), fett, code) {
                    raus.push(s);
                }
                fett = !fett;
            }
            // Inline-Code: EIN Backtick beginnt, das naechste schliesst.
            //
            // GEMESSENER FEHLER 07:55, und er ist der schlimmste in dieser
            // Crate: mein Zweig verlangte `chars.peek() == Some('`')`, also
            // ZWEI Backticks. Damit wurde `` `code` `` — die
            // Standard-Schreibweise, die in jeder Markdown-Datei steht —
            // gar nicht als Code erkannt:
            //
            //     `code`   -> ["`code`" code=false]   FALSCH
            //     ``code`` -> ["code"  code=true ]   nur die Doppelform
            //
            // Die KORREKTUR ist ein Backtick, und die Feinheit ist der
            // ZWEITE: ein schliessendes Backtick wird nur anerkannt, wenn
            // es nicht unmittelbar folgt. Sonst wuerde ``` aus drei
            // Backticks zu einem leeren Code-Stueck plus einem Text-
            // Backtick — und ``` ist ein Codeblock-Zaun, kein Inline-Code.
            //
            // `\`` (Escaped) ist kein Backtick-Paar: der Escape-Zweig
            // oben faengt es ab, bevor es hierher kommt.
            '`' => {
                if let Some(s) = Stueck::neu(std::mem::take(&mut puffer), fett, code) {
                    raus.push(s);
                }
                code = !code;
            }
            _ => puffer.push(c),
        }
    }
    if let Some(s) = Stueck::neu(puffer, fett, code) {
        raus.push(s);
    }

    // (5) LEERER BLOCK: keine Stuecke, kein Knoten.
    //
    // GEMESSEN 07:48: `"**"`, `"# "`, `"- "`, `"> "`, `""` ergeben alle
    // NULL Stuecke. Das ist der Punkt (5) des Auftrags, woertlich: "kein
    // Absturz, und kein Knoten mit leerem Inhalt".
    (!raus.is_empty()).then_some(raus)
}
