//! Lexer der Narsil-Sprache: Quelltext -> Token-Stream.
//!
//! Die Signaturen unten sind von Agent1 festgelegt und FROZEN.
//! TASK_0007 hat nur die Koerper gefuellt.
//!
//! Sprachspezifikation: docs/NARSIL_LANGUAGE_SPEC_v0.txt, Abschnitt 3.
//!
//! # Bytegrenzen
//!
//! Alle Positionen sind BYTE-OFFSETS, keine Zeichen-Indizes. Bei
//! mehrbyte-UTF-8 (z. B. `"Größe"`) sind die Spans deshalb weder
//! zeichen- noch codepoint-basiert, sondern schneiden immer an
//! Zeichen-Grenzen. Der Lexer arbeitet durchgaengig auf `&str`-Slices und
//! laeuft nie ueber einzelne Bytes, sodass `String::from_utf8` niemals
//! noetig wird.

use crate::token::{Token, TokenKind};
use narsil_core::color::Color;
use narsil_dom::Span;

/// Fehler beim Umwandeln von Zeichen in Tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    /// Was war falsch.
    pub message: String,
    /// Wo im Quelltext es falsch war.
    pub span: Span,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Narsil-Lexerfehler an Byte {} (Laenge {}): {}",
            self.span.start, self.span.len, self.message
        )
    }
}

impl LexError {
    /// Die Stelle als Zeile und Spalte.
    ///
    /// DIESELBE FAEHIGT wie `ParseError::position_in`, und aus demselben
    /// Grund: `span` ist ein Byte-Offset, und der Lexerfehler ist der
    /// ERSTE, den ein Benutzer sieht — er kommt aus der Datei, bevor der
    /// Parser ueberhaupt laeuft. Ein Byte-Offset an erster Stelle ist
    /// die schlechteste Stelle, um ihn zu zeigen.
    ///
    /// GEMESSEN (ABNAHME_SPRACHE.txt, Abschnitt 2): beide Fehlertypen
    /// trugen nur `message` und `span`, sonst nichts. Es gab keine Zeile
    /// und keine Spalte, fuer einen Menschen nirgends.
    pub fn position_in(&self, quelle: &str) -> narsil_dom::Position {
        narsil_dom::position(quelle, self.span.start)
    }
}

impl std::error::Error for LexError {}

/// Wandelt Quelltext in Tokens um.
///
/// Arbeitet zeichenweise ueber UTF-8 und respektiert die Bytegrenzen
/// fuer die `Span`-Angaben.
pub struct Lexer<'a> {
    source: &'a str,
    position: usize,
}

/// Zulaessige Hex-Laengen fuer Farben: 3, 4, 6 oder 8 Ziffern.
const COLOR_DIGIT_LENGTHS: [usize; 4] = [3, 4, 6, 8];

impl<'a> Lexer<'a> {
    /// Erzeugt einen Lexer ueber die Quelle.
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
        }
    }

    /// Liefert das naechste Token.
    ///
    /// Am Ende der Quelle wird wiederholt `TokenKind::Eof` geliefert.
    pub fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_trivia();

        // Am Ende: Eof, und ab hier dauerhaft Eof (kein Panic, kein Fehler).
        let rest = self.remaining();
        if rest.is_empty() {
            return Ok(Token::new(TokenKind::Eof, self.eof_span()));
        }

        let start = self.position;
        let first = first_char(rest);

        // Kommentar: `//` bis zum Zeilenende.
        if rest.starts_with("//") {
            self.position += 2;
            let len = self
                .remaining()
                .find('\n')
                .unwrap_or_else(|| self.remaining().len());
            self.position += len;
            return self.next_token();
        }

        // Zahlen (auch mit Prozentzeichen).
        if first.is_ascii_digit() {
            return self.lex_number();
        }

        // Bezeichner und Keywords.
        if first.is_ascii_lowercase() {
            return Ok(self.lex_identifier());
        }

        // Restliche Sonderfaelle nach dem Zeichen.
        match first {
            '#' => self.lex_color(),
            '"' => self.lex_string(),
            '{' => self.lex_single(TokenKind::LBrace),
            '}' => self.lex_single(TokenKind::RBrace),
            ':' => self.lex_single(TokenKind::Colon),
            ';' => self.lex_single(TokenKind::Semicolon),
            // Alles andere ist ein Fehler. Grossbuchstaben, `-`, `@`, `$`,
            // `?`, `.` und control characters landen hier.
            other => {
                let ch = other.to_string();
                self.position += other.len_utf8();
                Err(LexError {
                    message: format!("unbekanntes Zeichen: '{ch}'"),
                    span: Span::new(start as u32, other.len_utf8() as u32),
                })
            }
        }
    }

    /// Ueberspringt Whitespace und `//`-Kommentare.
    fn skip_trivia(&mut self) {
        loop {
            let rest = self.remaining();
            // Whitespace: Leerzeichen, Tab, CR, LF.
            let trimmed = rest.trim_start_matches([' ', '\t', '\r', '\n']);
            self.position += rest.len() - trimmed.len();
            if self.remaining().starts_with("//") {
                let after_slashes = &self.remaining()[2..];
                let len = after_slashes.find('\n').unwrap_or(after_slashes.len());
                self.position += 2 + len;
                continue;
            }
            return;
        }
    }

    /// Rest der Quelle ab der aktuellen Position.
    fn remaining(&self) -> &'a str {
        &self.source[self.position..]
    }

    /// Span des Dateendes: eine Position, Laenge 0.
    fn eof_span(&self) -> Span {
        Span::new(self.source.len() as u32, 0)
    }

    /// Setzt `position` auf das Ende des aktuellen Zeichens und gibt
    /// einen Span zurueck.
    fn span_to_here(&self, start: usize) -> Span {
        Span::new(start as u32, (self.position - start) as u32)
    }

    /// Ein einzelnes Zeichen-Token.
    fn lex_single(&mut self, kind: TokenKind) -> Result<Token, LexError> {
        let start = self.position;
        // first_char ist guaranteed: next_token hat already geprueft.
        let ch = first_char(self.remaining());
        self.position += ch.len_utf8();
        Ok(Token::new(kind, self.span_to_here(start)))
    }

    /// `[a-z][a-z0-9_]*` — Bezeichner oder Keyword.
    ///
    /// ANMERKUNG ZUR SPEZIFIKATION: Abschnitt 4 definiert
    /// `IDENT := [a-z] ([a-z]|[0-9]|"_")*` und erlaubt damit keinen
    /// Bindestrich. Die Property `font-size` kommt in Abschnitt 6 jedoch
    /// an fuenf Stellen vor und wird in `examples/hello.narsil` verwendet.
    /// Mit der Buchstaben-Regel waere das kanonische Beispieldokument
    /// nicht lexbar.
    ///
    /// Der Bindestrich ist deshalb in der FORTSETZUNG zugelassen, nicht am
    /// Anfang: `font-size` wird ein Bezeichner, `-5` bleibt ein Fehler
    /// (dort folgt der Bindestrich auf eine Ziffer). Damit sind beide
    /// Regeln der Spec erfuellt, statt eine davon zu opfern.
    /// Siehe FERTIG_0007.txt.
    fn lex_identifier(&mut self) -> Token {
        let start = self.position;
        let rest = self.remaining();
        let len = rest
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'))
            .unwrap_or(rest.len());
        self.position += len;
        let name = &rest[..len];
        let kind = match name {
            "page" => TokenKind::Page,
            "container" => TokenKind::Container,
            "text" => TokenKind::Text,
            "auto" => TokenKind::Auto,
            "row" => TokenKind::Row,
            "column" => TokenKind::Column,
            other => TokenKind::Ident(other.to_string()),
        };
        Token::new(kind, self.span_to_here(start))
    }

    /// `DIGIT+ ("." DIGIT+)?`, optional direkt gefolgt von `%`.
    fn lex_number(&mut self) -> Result<Token, LexError> {
        let start = self.position;
        let rest = self.remaining();

        let int_len = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        // Der Integer-Teil ist nie leer: next_token prueft die erste Ziffer.
        let int_part = &rest[..int_len];

        // Punkt: nur erlaubt, wenn danach mindestens eine Ziffer folgt.
        let after_int = &rest[int_len..];
        let frac_part = match after_int.strip_prefix('.') {
            Some(after_dot) => {
                let frac_len = after_dot
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(after_dot.len());
                if frac_len == 0 {
                    // `24.` — Punkt ohne Ziffern danach. Der Punkt wird
                    // nicht konsumiert, damit die Meldung ihn nennt.
                    return Err(LexError {
                        message: "Zahl mit Punkt, aber ohne Ziffern danach".into(),
                        span: self.span_to_here(start),
                    });
                }
                &after_dot[..frac_len]
            }
            None => "",
        };

        let is_float = !frac_part.is_empty();
        // Gesamtlaenge der Zahl INKLUSIVE des Punktes. Ohne das `+ 1` wird
        // der Dezimalteil ein zweites Mal gelext (`3.5` -> Float + Number).
        let number_len = int_len + usize::from(is_float) + frac_part.len();

        // Prozentzeichen direkt nach der Zahl?
        let after_number = &rest[number_len..];
        let is_percent = after_number.starts_with('%');
        self.position += number_len + usize::from(is_percent);
        let span = self.span_to_here(start);

        // Ganzzahlen werden DIREKT aus dem Integer-Text geparst, damit kein
        // Rundungsverlust ueber f32 entsteht: u32::MAX laesst sich nicht
        // verlustfrei in die 24-Bit-Mantisse von f32 darstellen.
        if !is_float && let Ok(n) = int_part.parse::<u32>() {
            if is_percent {
                return Ok(Token::new(TokenKind::Percent(n as f32), span));
            }
            return Ok(Token::new(TokenKind::Number(n), span));
        }

        let text = if is_float {
            format!("{int_part}.{frac_part}")
        } else {
            int_part.to_string()
        };
        let value = parse_number_text(&text).ok_or_else(|| LexError {
            message: "unlesbare Zahl".into(),
            span,
        })?;

        if is_percent {
            Ok(Token::new(TokenKind::Percent(value), span))
        } else {
            Ok(Token::new(TokenKind::Float(value), span))
        }
    }

    /// `"..."` mit den Escapes `\"`, `\\` und `\n`.
    fn lex_string(&mut self) -> Result<Token, LexError> {
        let start = self.position;
        // Fuehrendes `"` ueberspringen.
        self.position += 1;
        let mut value = String::new();

        loop {
            let rest = self.remaining();
            if rest.is_empty() {
                return Err(LexError {
                    message: "String nicht terminiert: schliessendes '\"' fehlt".into(),
                    span: self.span_to_here(start),
                });
            }
            let ch = first_char(rest);
            match ch {
                '"' => {
                    self.position += 1;
                    return Ok(Token::new(
                        TokenKind::String(value),
                        self.span_to_here(start),
                    ));
                }
                '\\' => {
                    // Escape: das Zeichen danach bestimmen.
                    let after_backslash = &rest[1..];
                    if after_backslash.is_empty() {
                        return Err(LexError {
                            message: "String nicht terminiert: Escape am Ende".into(),
                            span: self.span_to_here(start),
                        });
                    }
                    let esc = first_char(after_backslash);
                    self.position += 1 + esc.len_utf8();
                    match esc {
                        '"' => value.push('"'),
                        '\\' => value.push('\\'),
                        'n' => value.push('\n'),
                        other => {
                            return Err(LexError {
                                message: format!("unbekannte Escape-Sequenz: '\\{other}'"),
                                span: Span::new(
                                    (self.position - 1 - other.len_utf8()) as u32,
                                    (1 + other.len_utf8()) as u32,
                                ),
                            });
                        }
                    }
                }
                other => {
                    // Steuerzeichen im String sind verboten (TASK_0023).
                    //
                    // WARUM: v0 hat keine Ausrichtung, keine Spalten und
                    // keine `white-space`-Eigenschaft. Ein Tabulator belegt
                    // im Text Platz und zeigt nichts — er sieht aus wie
                    // ein Fehler und ist keiner. Ein echtes Tabulator-Byte
                    // im Quelltext ist ausserdem ein Unfall: niemand
                    // schreibt es absichtlich, und die Escape-Sequenz
                    // `\t` ist schon verboten.
                    //
                    // `\n` ist ausgenommen: es ist der erlaubte
                    // Zeilenumbruch und kommt ueber die Escape-Sequenz
                    // hierher.
                    if other.is_control() {
                        self.position += other.len_utf8();
                        return Err(LexError {
                            message: format!(
                                "unzulaessiges Zeichen im Textinhalt: 0x{:02X} — \
                                 in v0 gibt es keine Ausrichtung, und ein Zeichen, \
                                 das Platz belegt und nichts anzeigt, sieht aus wie \
                                 ein Fehler. Fuer ein Leerzeichen schreiben Sie ein \
                                 Leerzeichen.",
                                other as u32
                            ),
                            span: Span::new(
                                (self.position - other.len_utf8()) as u32,
                                other.len_utf8() as u32,
                            ),
                        });
                    }
                    self.position += other.len_utf8();
                    value.push(other);
                }
            }
        }
    }

    /// `#` + 3/4/6/8 Hex-Ziffern. Das `#` bleibt Teil des Tokens, weil
    /// `Color::parse_hex` es erwartet.
    ///
    /// Zwei Fehlerarten, die getrennt gemeldet werden:
    ///
    /// 1. Falsche ANZAHL Hex-Ziffern (`#ff`, `#fffff`): der Span deckt
    ///    `#` und alle folgenden Hex-Ziffern ab.
    /// 2. Falsches ZEICHEN (`#xyz`, `#12g4`): die Hex-Ziffern enden, weil
    ///    ein Zeichen kommt, das keine Hex-Ziffer ist. Dann waere
    ///    `hex_len = 0` und die Meldung wuerde "gefunden 0" sagen, was
    ///    irrefuehrend ist — es sind drei Zeichen da, nur keine davon ist
    ///    eine Ziffer. Der Span deckt in dem Fall `#` PLUS das erste
    ///    falsche Zeichen ab, und die Meldung nennt es beim Namen.
    fn lex_color(&mut self) -> Result<Token, LexError> {
        let start = self.position;
        // `#` ueberspringen.
        self.position += 1;
        let hex_len = self
            .remaining()
            .find(|c: char| !c.is_ascii_hexdigit())
            .unwrap_or_else(|| self.remaining().len());

        // Fehlerart 2: nach `#` folgt sofort ein Zeichen, das keine
        // Hex-Ziffer ist. Das ist ein eigener Fall mit eigener Meldung.
        if hex_len == 0 {
            let rest = self.remaining();
            if let Some(ch) = rest.chars().next() {
                // Span: `#` plus das erste falsche Zeichen.
                let breite = ch.len_utf8();
                let unlesbar = &self.source[start + 1..start + 1 + breite];
                self.position = start + 1 + breite;
                return Err(LexError {
                    message: format!(
                        "ungueltige Farbe: '#{unlesbar}' — '{ch}' ist keine Hex-Ziffer \
                         (erwartet 3, 4, 6 oder 8 Hex-Ziffern nach dem '#')"
                    ),
                    span: Span::new(start as u32, (1 + breite) as u32),
                });
            }
            // `#` ganz am Ende der Quelle.
            self.position = start + 1;
            return Err(LexError {
                message: "ungueltige Farbe: '#' am Ende der Eingabe".into(),
                span: Span::new(start as u32, 1),
            });
        }

        // Der Span muss ALLE Hex-Ziffern umfassen, auch wenn zu viele da
        // sind — sonst waere die Fehlermeldung bei `#fffff` irrefuehrend.
        let span_digits = self.span_from(start, hex_len);
        let text = format!("#{}", &self.source[start + 1..start + 1 + hex_len]);

        if !COLOR_DIGIT_LENGTHS.contains(&hex_len) {
            self.position = start + 1 + hex_len;
            return Err(LexError {
                message: format!(
                    "ungueltige Farbe '{text}': erwartet 3, 4, 6 oder 8 Hex-Ziffern, gefunden {hex_len}"
                ),
                span: span_digits,
            });
        }

        // Validierung ueber narsil-core, damit Lexer und Parser dieselbe
        // Regel benutzen.
        if let Err(e) = Color::parse_hex(&text) {
            self.position = start + 1 + hex_len;
            return Err(LexError {
                message: format!("ungueltige Farbe '{text}': {e}"),
                span: span_digits,
            });
        }

        self.position = start + 1 + hex_len;
        Ok(Token::new(TokenKind::Color(text), self.span_to_here(start)))
    }

    /// Span ab `start` mit gegebener Nutzlast-Laenge.
    fn span_from(&self, start: usize, len: usize) -> Span {
        Span::new(start as u32, (1 + len) as u32)
    }
}

/// Erstes Zeichen des Restes als `char`.
fn first_char(s: &str) -> char {
    s.chars().next().unwrap_or('\0')
}

/// Wandelt einen numerischen Text in `f32` um.
fn parse_number_text(text: &str) -> Option<f32> {
    text.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// Lexert die ganze Quelle in eine Token-Liste.
///
/// Das ist die bequeme Form fuer den Parser: einmal aufrufen, dann
/// ueber `Vec<Token>` iterieren.
pub fn tokenize(source: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next_token()?;
        let is_eof = token.kind == TokenKind::Eof;
        tokens.push(token);
        if is_eof {
            return Ok(tokens);
        }
    }
}

// ===========================================================================
// TESTS
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Shorthand: die Tokenarten einer erfolgreichen Lexierung.
    fn kinds(source: &str) -> Vec<TokenKind> {
        tokenize(source)
            .unwrap_or_else(|e| panic!("Lexen von {source:?} schlug fehl: {e}"))
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    /// Shorthand: alle Tokens einer erfolgreichen Lexierung.
    fn all(source: &str) -> Vec<Token> {
        tokenize(source).unwrap_or_else(|e| panic!("Lexen von {source:?} schlug fehl: {e}"))
    }

    /// Der Quelltext, den ein Token abdeckt.
    fn slice(source: &str, span: Span) -> &str {
        &source[span.start as usize..span.end() as usize]
    }

    // -- Grundlagen ---------------------------------------------------------

    #[test]
    fn empty_source_yields_only_eof() {
        assert_eq!(kinds(""), vec![TokenKind::Eof],);
        let tokens = all("");
        assert_eq!(tokens[0].span, Span::new(0, 0));
    }

    #[test]
    fn whitespace_only_yields_only_eof() {
        for src in ["   ", "\t\n\r\n", " \t "] {
            assert_eq!(kinds(src), vec![TokenKind::Eof], "{src:?}");
        }
    }

    #[test]
    fn minimal_page_has_four_tokens() {
        assert_eq!(
            kinds("page { }"),
            vec![
                TokenKind::Page,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn eof_appears_exactly_once() {
        let tokens = all("page { }");
        let eof_count = tokens.iter().filter(|t| t.kind == TokenKind::Eof).count();
        assert_eq!(eof_count, 1, "genau ein Eof");
        assert_eq!(tokens.last().map(|t| &t.kind), Some(&TokenKind::Eof));
    }

    #[test]
    fn next_token_keeps_returning_eof() {
        // Nach dem Ende: dauerhaft Eof, kein Panic, kein Fehler.
        let mut lexer = Lexer::new("page");
        assert_eq!(lexer.next_token().expect("page").kind, TokenKind::Page);
        for _ in 0..5 {
            assert_eq!(lexer.next_token().expect("eof").kind, TokenKind::Eof);
        }
    }

    #[test]
    fn lexer_terminates_without_infinite_loop() {
        // 10.000 Statements: muss enden.
        let source = format!("page {{ {} }}", "width: 800; ".repeat(10_000));
        let tokens = tokenize(&source).expect("sollte lexen");
        assert!(tokens.len() > 20_000);
    }

    // -- Keywords und Bezeichner -------------------------------------------

    #[test]
    fn all_six_keywords_are_recognized() {
        let source = "page container text auto row column";
        assert_eq!(
            kinds(source),
            vec![
                TokenKind::Page,
                TokenKind::Container,
                TokenKind::Text,
                TokenKind::Auto,
                TokenKind::Row,
                TokenKind::Column,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn other_identifiers_become_ident_tokens() {
        assert_eq!(
            kinds("width font-size my_prop x9"),
            vec![
                TokenKind::Ident("width".into()),
                TokenKind::Ident("font-size".into()),
                TokenKind::Ident("my_prop".into()),
                TokenKind::Ident("x9".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn identifiers_are_lowercase_with_digits_and_underscore() {
        // a z 0 9 _ sind in der Fortsetzung erlaubt.
        assert_eq!(
            kinds("abc z09 a_b a0_b"),
            vec![
                TokenKind::Ident("abc".into()),
                TokenKind::Ident("z09".into()),
                TokenKind::Ident("a_b".into()),
                TokenKind::Ident("a0_b".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn identifier_must_start_with_a_letter() {
        // `_` ist laut Spec `[a-z]` am Anfang — also ungueltig.
        let e = tokenize("_x").expect_err("muss scheitern");
        assert!(e.message.contains("unbekanntes Zeichen"), "{}", e.message);
    }

    #[test]
    fn hyphen_is_allowed_inside_identifiers() {
        // `font-size` ist eine definierte Property und kommt in
        // examples/hello.narsil vor. Der Bindestrich ist deshalb in der
        // Fortsetzung erlaubt.
        assert_eq!(
            kinds("font-size"),
            vec![TokenKind::Ident("font-size".into()), TokenKind::Eof]
        );
    }

    #[test]
    fn hyphen_still_fails_after_a_digit() {
        // `-5` bleibt ein Fehler: der Bindestrich folgt auf eine Ziffer.
        let e = tokenize("5-3").expect_err("muss scheitern");
        assert!(e.message.contains("unbekanntes Zeichen"), "{}", e.message);
    }

    #[test]
    fn uppercase_letters_are_unknown() {
        // Spezifikation Abschnitt 3: keine Grossbuchstaben in v0.
        let e = tokenize("Width").expect_err("muss scheitern");
        assert!(e.message.contains("unbekanntes Zeichen"), "{}", e.message);
    }

    // -- Zahlen -------------------------------------------------------------

    #[test]
    fn integers_and_floats() {
        assert_eq!(
            kinds("0 800 24 3.5 0.5"),
            vec![
                TokenKind::Number(0),
                TokenKind::Number(800),
                TokenKind::Number(24),
                TokenKind::Float(3.5),
                TokenKind::Float(0.5),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn percent_after_number() {
        assert_eq!(
            kinds("50% 100%"),
            vec![
                TokenKind::Percent(50.0),
                TokenKind::Percent(100.0),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn percent_after_float() {
        assert_eq!(kinds("3.5%"), vec![TokenKind::Percent(3.5), TokenKind::Eof]);
    }

    #[test]
    fn trailing_dot_is_an_error() {
        // `24.` — Punkt ohne Ziffern danach.
        let e = tokenize("24.").expect_err("muss scheitern");
        assert!(e.message.contains("ohne Ziffern"), "{}", e.message);
    }

    #[test]
    fn leading_dot_is_an_error() {
        // `.5` — `.` ist kein bekanntes Zeichen.
        let e = tokenize(".5").expect_err("muss scheitern");
        assert!(e.message.contains("unbekanntes Zeichen"), "{}", e.message);
    }

    #[test]
    fn minus_sign_is_an_error() {
        // `-5`: negative Zahlen sind in v0 verboten.
        let e = tokenize("-5").expect_err("muss scheitern");
        assert!(e.message.contains("unbekanntes Zeichen"), "{}", e.message);
    }

    #[test]
    fn exponent_notation_is_not_a_lexer_error() {
        // Task-Vorgabe: `1e3` ist KEIN Lexerfehler, sondern Number(1)
        // gefolgt von Ident("e3"). Der Parser meldet es.
        assert_eq!(
            kinds("1e3"),
            vec![
                TokenKind::Number(1),
                TokenKind::Ident("e3".into()),
                TokenKind::Eof
            ]
        );
    }

    // -- Farben -------------------------------------------------------------

    #[test]
    fn colors_keep_the_hash_sign() {
        let tokens = all("color: #101014;");
        let color = tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Color(_)))
            .expect("Farbe");
        assert_eq!(color.kind, TokenKind::Color("#101014".into()));
        // Das `#` ist Teil des Tokens, weil parse_hex es erwartet.
        assert!(Color::parse_hex("#101014").is_ok());
    }

    #[test]
    fn all_four_color_forms() {
        for src in ["#f00", "#0f08", "#101014", "#10101480"] {
            let tokens = all(src);
            assert_eq!(tokens.len(), 2, "{src}: Farbe + Eof");
            let text = match &tokens[0].kind {
                TokenKind::Color(s) => s.clone(),
                other => panic!("{src}: erwartet Color, war {other:?}"),
            };
            assert_eq!(text, src, "Farbwert muss exakt dem Quelltext entsprechen");
            assert!(Color::parse_hex(&text).is_ok());
        }
    }

    #[test]
    fn wrong_color_length_is_an_error() {
        for src in ["#f", "#ff", "#fffff", "#fffffff"] {
            let e = tokenize(src).expect_err("{src} muss scheitern");
            assert!(e.message.contains("Farbe"), "{}", e.message);
        }
    }

    #[test]
    fn non_hex_digit_in_color_is_an_error() {
        let e = tokenize("#gggggg").expect_err("muss scheitern");
        assert!(e.message.contains("Farbe"), "{}", e.message);
    }

    #[test]
    fn hash_without_digits_is_an_error() {
        let e = tokenize("#").expect_err("muss scheitern");
        assert!(e.message.contains("Farbe"), "{}", e.message);
    }

    // -- Strings ------------------------------------------------------------

    #[test]
    fn plain_string_without_quotes_in_value() {
        let tokens = all("\"Hallo Welt\"");
        assert_eq!(tokens[0].kind, TokenKind::String("Hallo Welt".into()));
    }

    #[test]
    fn escaped_quote_in_string() {
        let tokens = all(r#""a\"b""#);
        assert_eq!(tokens[0].kind, TokenKind::String("a\"b".into()));
    }

    #[test]
    fn escaped_backslash_in_string() {
        let tokens = all(r#""a\\b""#);
        assert_eq!(tokens[0].kind, TokenKind::String("a\\b".into()));
    }

    #[test]
    fn escaped_newline_in_string() {
        let tokens = all(r#""a\nb""#);
        assert_eq!(tokens[0].kind, TokenKind::String("a\nb".into()));
    }

    #[test]
    fn unterminated_string_is_an_error() {
        let e = tokenize("\"abc").expect_err("muss scheitern");
        assert!(e.message.contains("nicht terminiert"), "{}", e.message);
    }

    #[test]
    fn unknown_escape_is_an_error() {
        let e = tokenize(r#""\q""#).expect_err("muss scheitern");
        assert!(e.message.contains("Escape"), "{}", e.message);
    }

    #[test]
    fn empty_string_is_allowed() {
        let tokens = all("\"\"");
        assert_eq!(tokens[0].kind, TokenKind::String(String::new()));
    }

    #[test]
    fn german_text_is_one_string_token() {
        // Mehrbyte-UTF-8 in einem String.
        let tokens = all("\"Größe\"");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::String("Größe".into()));
    }

    // -- Kommentare ---------------------------------------------------------

    #[test]
    fn comment_is_skipped() {
        assert_eq!(
            kinds("page {} // fertig"),
            vec![
                TokenKind::Page,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn comment_only_source_yields_eof() {
        for src in ["// nur ein Kommentar", "//", "  // x  \n"] {
            assert_eq!(kinds(src), vec![TokenKind::Eof], "{src:?}");
        }
    }

    #[test]
    fn comment_before_and_after_code() {
        let source = "// vorn\npage { // mitten\n} // hinten";
        assert_eq!(
            kinds(source),
            vec![
                TokenKind::Page,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn double_slash_inside_string_is_no_comment() {
        // WICHTIG: `//` innerhalb eines Strings ist KEIN Kommentar.
        let tokens = all("\"a // b\"");
        assert_eq!(tokens.len(), 2, "ein String + Eof");
        assert_eq!(tokens[0].kind, TokenKind::String("a // b".into()));
    }

    #[test]
    fn single_slash_is_not_a_comment() {
        // `/` allein ist kein bekanntes Zeichen.
        let e = tokenize("/").expect_err("muss scheitern");
        assert!(e.message.contains("unbekanntes Zeichen"), "{}", e.message);
    }

    // -- Sonderzeichen ------------------------------------------------------

    #[test]
    fn punctuation_tokens() {
        assert_eq!(
            kinds("{ } : ;"),
            vec![
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::Colon,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn unknown_characters_are_errors() {
        for src in ["@", "$", "?", "&", "*"] {
            let e = tokenize(src).unwrap_err();
            assert!(e.message.contains("unbekanntes Zeichen"), "{}", e.message);
        }
    }

    // -- Spans --------------------------------------------------------------

    #[test]
    fn spans_cover_exactly_the_token_text() {
        let source = "page { width: 800; }";
        let tokens = all(source);
        for token in &tokens {
            if token.kind == TokenKind::Eof {
                continue;
            }
            let text = slice(source, token.span);
            assert!(!text.is_empty(), "leerer Span bei {token:?}");
        }
        // Die Tokens liegen aufsteigend und UEBERLAPPEN NICHT. Zwischen
        // ihnen liegt Whitespace, der zu keinem Token gehoert — deshalb
        // sind Luecken erlaubt, Ueberschneidungen nicht.
        let mut previous_end = 0u32;
        for token in &tokens {
            if token.kind == TokenKind::Eof {
                continue;
            }
            assert!(
                token.span.start >= previous_end,
                "Ueberschneidung bei {token:?} (vorheriges Ende {previous_end})"
            );
            previous_end = token.span.end();
        }
    }

    #[test]
    fn keyword_span_is_four_bytes() {
        let source = "page { }";
        let tokens = all(source);
        assert_eq!(tokens[0].span, Span::new(0, 4));
        assert_eq!(slice(source, tokens[0].span), "page");
    }

    #[test]
    fn number_span_covers_its_digits() {
        let source = "width: 800;";
        let tokens = all(source);
        let number = tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Number(_)))
            .expect("Zahl");
        assert_eq!(slice(source, number.span), "800");
        assert_eq!(number.span, Span::new(7, 3));
    }

    #[test]
    fn color_span_includes_hash_and_all_digits() {
        let source = "background: #1b1b24;";
        let tokens = all(source);
        let color = tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Color(_)))
            .expect("Farbe");
        assert_eq!(slice(source, color.span), "#1b1b24");
        assert_eq!(color.span.len, 7);
    }

    #[test]
    fn string_span_includes_the_quotes() {
        // Der Span umfasst ALLE Bytes inklusive der Anfuehrungszeichen,
        // der Wert selbst aber nicht.
        let source = "text \"Hallo\"";
        let tokens = all(source);
        let string = tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::String(_)))
            .expect("String");
        assert_eq!(slice(source, string.span), "\"Hallo\"");
        assert_eq!(string.span.len, 7);
    }

    #[test]
    fn multibyte_utf8_produces_byte_spans() {
        // Bytegrenzen, nicht Zeichenzahlen.
        let source = "text \"Größe\"";
        let tokens = all(source);
        let string = tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::String(_)))
            .expect("String");
        // "Größe" = G r ö ß e -> 5 Zeichen, aber 7 Bytes.
        assert_eq!(string.span.len, 9, "2 Anfuehrungszeichen + 7 Bytes");
        assert_eq!(slice(source, string.span), "\"Größe\"");
    }

    #[test]
    fn whitespace_belongs_to_no_token() {
        let source = "page    {     }";
        let tokens = all(source);
        for token in &tokens {
            if token.kind == TokenKind::Eof {
                continue;
            }
            let text = slice(source, token.span);
            assert!(!text.contains(' '), "Whitespace im Span: {text:?}");
        }
    }

    // -- Gesamtdokument -----------------------------------------------------

    #[test]
    fn full_document_lexes_without_error() {
        // Spezifikation Abschnitt 7b, kompakt.
        let source = "page { width: 800; height: 600; background: #101014; }";
        let tokens = all(source);
        assert_eq!(tokens.last().map(|t| &t.kind), Some(&TokenKind::Eof));
    }

    #[test]
    fn hello_narsil_example_lexes() {
        // Die kanonische Beispieldatei muss fehlerfrei lexbar sein.
        let source = include_str!("../../../examples/hello.narsil");
        let tokens = tokenize(source).expect("hello.narsil muss lexbar sein");
        assert!(tokens.len() > 40, "erwartet viele Tokens");
        // Genau ein Eof, am Ende.
        let eofs = tokens.iter().filter(|t| t.kind == TokenKind::Eof).count();
        assert_eq!(eofs, 1);
        assert_eq!(tokens.last().map(|t| &t.kind), Some(&TokenKind::Eof));
    }

    // -- Robustheit: kein Panic, egal was ----------------------------------

    #[test]
    fn hostile_input_never_panics() {
        // Nur pruefen: kein Panic. Ob Ok oder Err, ist hier egal.
        let cases = [
            "",
            "   ",
            "//",
            "\"",
            "#",
            "\u{0}",
            "\u{1F600}",
            "page { width: -5; }",
            "text \"unterminiert",
            "text \"\\q\"",
            "color: #12345;",
            "1e3 1E3 .5 5.",
            "@#$%^&*",
            "page { page { page { } } }",
            "\"\\",
            "0%",
            "00000000000000000000000001",
        ];
        for src in cases {
            let _ = tokenize(src);
        }
    }

    #[test]
    fn ten_thousand_characters_do_not_panic() {
        let source = "a".repeat(10_000);
        let _ = tokenize(&source);
        let source = "page { ".repeat(1_000) + &"}".repeat(1_000);
        let _ = tokenize(&source);
    }

    #[test]
    fn lex_error_display_is_readable() {
        let e = tokenize("@").expect_err("muss scheitern");
        let text = e.to_string();
        assert!(text.contains("Byte"), "{text}");
        assert!(text.contains("unbekanntes Zeichen"), "{text}");
    }

    #[test]
    fn error_span_points_at_the_offending_character() {
        let source = "page { @ }";
        let e = tokenize(source).expect_err("muss scheitern");
        assert_eq!(slice(source, e.span), "@");
    }

    #[test]
    fn string_value_is_decoded_not_raw() {
        // Der Wert enthaelt echte Zeichen, keine Escape-Sequenzen.
        let tokens = all(r#""a\"b\\c\nd""#);
        assert_eq!(tokens[0].kind, TokenKind::String("a\"b\\c\nd".into()));
    }

    #[test]
    fn percent_value_is_f32() {
        let tokens = all("50%");
        match &tokens[0].kind {
            TokenKind::Percent(v) => assert_eq!(*v, 50.0),
            other => panic!("erwartet Percent, war {other:?}"),
        }
    }

    #[test]
    fn number_spanning_many_digits() {
        // Ganzzahlen, die in u32 passen, bleiben Number.
        assert_eq!(
            kinds("4294967295"),
            vec![TokenKind::Number(4294967295), TokenKind::Eof]
        );
        // Groessere Werte werden Float, nicht Fehler.
        assert!(matches!(all("99999999999999")[0].kind, TokenKind::Float(_)));
    }
}
