//! JS2, JSON: die Regeln des JSON-Lesers und des JSON-Schreibers.
//!
//! ## WARUM EINE EIGENE DATEI
//!
//! `JSON.parse` und `JSON.stringify` liegen in `builtin3.rs` — einer Datei, in
//! der auch Promise und RegExp liegen. Die **Regeln** (welcher Leerraum
//! erlaubt ist, wie `space` eingerückt wird, was ein `replacer`-Array
//! bedeutet) stehen hier, und `builtin3.rs` liest sie. Grund ist derselbe wie
//! bei `eingebaut_menge.rs`: eine Regel, die man an zwei Orten wiederholen
//! muss, ist eine Regel, die auseinanderläuft.
//!
//! ## DER ERSTE BEFUND (gemessen, 02.10.2026, 00:05)
//!
//! Fünf rote test262-Fälle in `built-ins/JSON/parse`, alle mit derselben
//! Meldung:
//!
//! ```text
//! 15.12.1.1-0-2.js  JSON.parse('\u000b1234')  -> kein Fehler
//! 15.12.1.1-0-3.js  JSON.parse('\u000c1234')  -> kein Fehler
//! 15.12.1.1-0-4.js  JSON.parse('\u00a01234')  -> kein Fehler
//! 15.12.1.1-0-8.js  JSON.parse('\u2028\u20291234') -> kein Fehler
//! invalid-whitespace.js (U+1680, U+2009, U+200A, U+202F, U+205F, U+3000)
//! ```
//!
//! Ursache, eine Zeile in `builtin3.rs`:
//!
//! ```text
//! while self.pos < self.zeichen.len() && self.zeichen[self.pos].is_whitespace() {
//! ```
//!
//! `char::is_whitespace()` ist die **Unicode**-Eigenschaft `White_Space`, und
//! die umfasst viel mehr als JSON erlaubt.
//!
//! ## DIE REGEL, und woher sie kommt
//!
//! JSON (ECMA-262 25.5.1, gleichlautend RFC 8259) erlaubt als Leerraum
//! **genau vier** Zeichen:
//!
//! ```text
//! U+0020  SPACE
//! U+0009  CHARACTER TABULATION
//! U+000A  LINE FEED
//! U+000D  CARRIAGE RETURN
//! ```
//!
//! `node 22`, gegengemessen (alle acht Zeichen, nicht die vier günstigen):
//!
//! ```text
//! "\u000b" -> SyntaxError      "\u000c" -> SyntaxError
//! "\u00a0" -> SyntaxError      "\u2028\u2029" -> SyntaxError
//! "  "     -> KEIN Fehler     "\t" -> KEIN Fehler
//! "\n"     -> KEIN Fehler     "\r" -> KEIN Fehler
//! ```
//!
//! Das ist die **Unterscheidung**, die zählt: `trim_start_matches(char::is_whitespace)`
//! wäre genauso falsch wie `is_whitespace`, nur an anderer Stelle. Die Liste
//! steht deshalb ausgeschrieben und nicht als „irgendwas, was aussieht wie
//! Leerraum".
//!
//! M3/M7: Die Funktion bekommt ein `char` aus **fremden** Daten (aus der
//! geparsten Zeichenkette). Es gibt keine Schleife und keine Allokation —
//! der Aufrufer iteriert, diese Funktion entscheidet nur. Also kein
//! `MAX_...`; der Name der Funktion *ist* die Aussage.

/// Ist `c` Leerraum **in JSON**? Genau vier Zeichen, sonst nicht.
///
/// DIE ABSICHT DER ENGEREN FASSUNG: `char::is_whitespace()` beantwortet
/// „ist das ein weißes Zeichen in Unicode?" — das ist eine andere Frage als
/// „darf das hier stehen?". U+00A0 (NO-BREAK SPACE) sieht aus wie ein
/// Leerzeichen und ist keines; U+2028/U+2029 sind Zeilentrenner in JavaScript,
/// in JSON aber ein Syntaxfehler.
pub(crate) fn ist_json_leerraum(c: char) -> bool {
    // M3: Kein `matches!` mit einer Liste, sondern vier Vergleiche — die
    // Liste waere gleich, aber `matches!` versteckt die Frage in einer
    // Syntax. Hier ist die Frage sichtbar: sind es diese vier?
    c == ' ' || c == '\t' || c == '\n' || c == '\r'
}

// =====================================================================
// DER ZWEITE BEFUND: `JSON.stringify(…, space)`
//
// Drei rote test262-Faelle: `space-number-object.js`, `space-number-float.js`,
// `space-string-object.js`. Alle drei mit derselben Form: `actual` ist die
// **kompakte** Ausgabe, `expected` die eingerueckte.
//
// DIE URSACHE, und sie ist nicht die Rechnung: die Rechnung war schon da und
// **richtig**. `builtin3.rs` hatte fuer `space` einen eigenen `match`:
//
//     GVal::Zahl(n) if n.is_finite() => " ".repeat((n.max(0.0).min(10.0)) as usize),
//     GVal::Ref(r) => match holen(r) {
//         HObj::Zeichenkette(s) => s.chars().take(10).collect(),
//         _ => String::new(),          // <-- HIER
//     },
//
// Eine **Box** (`new Number(5.11111)`, `new String('xxx')`) ist `HObj::Verpackt`
// und landete in `_` — also kein Einruecken. Die drei Tests benutzen aber
// genau Boxen, weil sie pruefen wollen, dass die Box wie ihr Inhalt
// behandelt wird.
//
// GEMESSEN, dass es **nur** daran liegt (00:22, `examples/js2_probe.rs`,
// danach entfernt) — sieben Proben mit einer **primitiven** Zahl:
// `2` -> zwei Leerzeichen, `11` -> zehn, `1.5` -> eins, `0`/`-1`/`''` -> keins,
// `'xy'` -> xy, verschachtelt -> korrekt eingerueckt. **Sieben von sieben wie
// node.** Die Rechnung stimmt; nur die Box fehlt.
//
// UND DIE GUTE NACHRICHT, auch gemessen: `ToPrimitive` kann der Motor schon.
// `String(box)` benutzt ein ueberschriebenes `toString`, `Number(box)` ein
// ueberschriebenes `valueOf`, und ein absichtlich werfendes `toString` wirft
// auch. Es fehlt **kein** ToPrimitive — es fehlt nur der Aufruf.
// =====================================================================

/// `gap` aus einer **Zahl** (ECMA-262 25.2.3, Schritt „If Type(space) is
/// Number").
///
/// DIE DREI REGELN, alle drei aus `node 22` gegengessen (00:22):
///
///  * **abschneiden**, nicht runden — `1.5` gibt **ein** Leerzeichen,
///    `5.11111` fuenf;
///  * **begrenzen** auf 0..10 — `11` gibt zehn, `-1.99999` keins;
///  * `NaN`/unendlich: kein Leerraum.
///
/// M3: `n` kommt aus fremden Daten. `" ".repeat(k)` mit `k <= 10` ist die
/// einzige Allokation, und `10` steht als Name in der Zeile darunter —
// ein `MAX_...` waere hier ueberfluss, weil die Grenze in **zwei** Regeln
/// steckt (Abschneiden und Begrenzen) und nicht in einer Schleife.
pub(crate) fn einrueck_aus_zahl(n: f64) -> String {
    /// ECMA-262 25.2.3: „If space is a Number, then let gap be the result of
    /// clamping space between 0 and 10. If gap is non-zero, let gap be the
    /// String value containing gap occurrences of the code unit 0x0020."
    const MAX_LEERZEICHEN: usize = 10;
    if !n.is_finite() {
        return String::new();
    }
    // `as usize` schneidet in Richtung null — das ist `trunc`, was die Norm
    // verlangt, und **nicht** `round`, was `f64::round` tun wuerde.
    let k = n.trunc().max(0.0).min(MAX_LEERZEICHEN as f64) as usize;
    " ".repeat(k)
}

/// `gap` aus einem **Text** (Schritt „If Type(space) is String").
///
/// node 22, 00:22: `JSON.stringify({a:1}, null, 'xy')` -> `{\nxy"a": 1\n}`.
/// Und `'xxxxxxxxxxxxxxxx'` (16 Zeichen) wird auf **zehn** gekuerzt.
pub(crate) fn einrueck_aus_text(s: &str) -> String {
    const MAX_LEERZEICHEN: usize = 10;
    s.chars().take(MAX_LEERZEICHEN).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DER TEST DER REGEL, und er ist eine Liste, weil die Regel eine Liste
    /// ist.
    ///
    /// `node 22`, gegengemessen 00:05. Links „erlaubt", rechts „SyntaxError".
    /// Ich habe die Liste aus `node` **und** aus den test262-Dateien; die
    /// test262-Dateien nennen U+000B, U+000C, U+00A0, U+2028, U+2029, U+1680,
    /// U+2009, U+200A, U+202F, U+205F, U+3000 — elf verbotene Zeichen.
    #[test]
    fn genau_vier_zeichen_sind_leerraum() {
        // Die vier erlaubten.
        for c in [' ', '\t', '\n', '\r'] {
            assert!(
                ist_json_leerraum(c),
                "U+{:04X} ist JSON-Leerraum — node laesst {c:?} durch",
                c as u32
            );
        }
        // Die verbotenen, mit der Nummer, die test262 benutzt.
        for c in [
            '\u{000b}', '\u{000c}', '\u{00a0}', '\u{1680}', '\u{2000}', '\u{2009}', '\u{200a}',
            '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}', '\u{3000}',
        ] {
            assert!(
                !ist_json_leerraum(c),
                "U+{:04X} ist KEIN JSON-Leerraum — `char::is_whitespace()` sagt \
                 ja, node sagt SyntaxError",
                c as u32
            );
        }
    }

    /// Und die **Gegenprobe** an der Funktion selbst: `char::is_whitespace()`
    /// nimmt mindestens U+00A0 und U+2028 mit. Ohne diese Zeile koennte
    /// jemand „beide sind doch Leerraum" denken und zur alten Fassung
    /// zurueckgehen — dann waeren **diese** Tests rot, nicht der naechste
    /// Fund.
    #[test]
    fn die_unicode_eigenschaft_ist_streenger_als_json() {
        // Rust sagt fuer diese Zeichen „ja".
        assert!('\u{00a0}'.is_whitespace());
        assert!('\u{2028}'.is_whitespace());
        assert!('\u{3000}'.is_whitespace());
        // JSON sagt „nein".
        for c in ['\u{00a0}', '\u{2028}', '\u{3000}'] {
            assert!(
                !ist_json_leerraum(c),
                "U+{:04X}: Rust is_whitespace() und JSON muessen hier \
                 verschieden sein",
                c as u32
            );
        }
    }

    // -----------------------------------------------------------------
    // `space` — die drei Regeln, gegen node gemessen
    // -----------------------------------------------------------------

    /// `node 22`, 00:22, als Laengen gemessen (`JSON.stringify({a:1},null,X)`
    /// hat die Laenge `2 + 2*gap + len("{a:1}") - 1` … gemessen habe ich
    /// direkt die Ausgabe):
    ///
    /// ```text
    /// null, 0     -> {"a":1}              (kein gap)
    /// null, 2     -> {\n  "a": 1\n}      (2)
    /// null, 11    -> 10 Leerzeichen       (begrenzt)
    /// null, 1.5   -> 1 Leerzeichen       (abgeschnitten, nicht gerundet!)
    /// null, -1.99999 -> kein gap
    /// ```
    ///
    /// DER PUNKT AN `1.5`: `f64::round(1.5)` waere **2**, und der Test
    /// `space-number-float.js` vergleicht `6.99999` mit `6` — das heisst
    /// **abschneiden**. Wer `round` benutzt, besteht `-1.99999`/`-1` und
    /// scheitert an `6.99999`/`6`.
    #[test]
    fn einrueck_aus_zahl_schneidet_ab_und_begrenzt() {
        for (n, gap) in [
            (0.0, ""),
            (-1.0, ""),
            (-1.99999, ""),
            (1.0, " "),
            (1.5, " "),
            (2.0, "  "),
            (5.11111, "     "),
            (6.99999, "      "),
            (10.0, "          "),
            (11.0, "          "),
            (99.0, "          "),
        ] {
            assert_eq!(
                einrueck_aus_zahl(n),
                gap,
                "JSON.stringify(space={n}) muss {gap:?} einruecken"
            );
        }
    }

    /// `NaN` und unendlich geben **keinen** Einrueck — node sagt
    /// `{"a":1}`. (Eine Schleife mit `n as usize` wuerde `NaN` zu `0`
    /// machen und `INFINITY` zu `usize::MAX` — die zweite Umwandlung
    /// braucht also die `is_finite`-Abfrage, nicht das `min`.)
    #[test]
    fn einrueck_aus_zahl_ignoriert_nan_und_unendlich() {
        for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                einrueck_aus_zahl(n),
                "",
                "{n} ist keine endliche Zahl und gibt keinen Einrueck"
            );
        }
        // Die Umwandlung, die ohne die Abfrage schiefgeht:
        assert_eq!(
            f64::INFINITY as usize,
            usize::MAX,
            "das ist der Grund fuer `is_finite` — `INFINITY as usize` waere              eine riesige Allokation"
        );
    }

    /// Der Text: die ersten **zehn** Zeichen, `node 22`.
    ///
    /// `chars()` und nicht `bytes()`: `take(10)` auf `bytes()` koennte mitten
    /// in einem Mehrbyte-Zeichen abschneiden und damit einen **kaputten**
    /// Text erzeugen. Das ist keine theoretische Sorge, das ist genau der
    /// Grund, warum man `chars()` nimmt.
    #[test]
    fn einrueck_aus_text_nimmt_zehn_zeichen() {
        assert_eq!(einrueck_aus_text(""), "");
        assert_eq!(einrueck_aus_text("x"), "x");
        assert_eq!(einrueck_aus_text("xy"), "xy");
        assert_eq!(einrueck_aus_text("xxxxxxxxxx"), "xxxxxxxxxx");
        assert_eq!(einrueck_aus_text("xxxxxxxxxxx"), "xxxxxxxxxx");
        assert_eq!(einrueck_aus_text("xxxxxxxxxxxxxxxx"), "xxxxxxxxxx");
        // Und der Mehrbyte-Fall: 12 Zeichen 'a\u00e4' ergibt 10 davon.
        let text = "a\u{00e4}".repeat(12);
        let zehn = einrueck_aus_text(&text);
        assert_eq!(zehn.chars().count(), 10, "{zehn:?}");
        assert!(
            zehn.is_char_boundary(zehn.len()),
            "kein abgeschnittener UTF-8"
        );
    }

    /// Was **sonst** noch kein Leerraum ist — die Frage dreht sich nicht nur
    /// um unsichtbare Zeichen.
    #[test]
    fn sichtbare_zeichen_sind_kein_leerraum() {
        for c in ['0', '"', '{', '[', ':', ',', 'a', '\\', '/', '\u{feff}'] {
            assert!(
                !ist_json_leerraum(c),
                "{c:?} ist sichtbar und kein Leerraum"
            );
        }
        // U+FEFF (BOM) steht hier, weil es eine eigene Falle ist: es sieht
        // unsichtbar aus, ist aber nicht einmal White_Space. Wer
        // `!c.is_ascii()` als Test nimmt, laesst es durch.
        assert!(
            !'\u{feff}'.is_whitespace(),
            "U+FEFF ist nicht einmal White_Space — der Gegenfall"
        );
    }
}
