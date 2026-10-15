//! Tokenizer nach CSS Syntax Level 3, vereinfacht (C1).
//!
//! Kommentare (`/* */`) fallen weg (unabgeschlossen: Rest faellt weg).
//! Zeichenketten, Zahlen mit Einheiten, `#`-Werte, Funktionen,
//! `@`-Regeln und Einzelzeichen werden erkannt. Jede Schleife laeuft
//! vorwaerts (Position waechst strikt) und endet spaetestens an
//! MAX_TOKEN bzw. MAX_EINGABE — Eingaben koennen nicht haengen.

use super::{MAX_EINGABE, MAX_TOKEN, MAX_TOKEN_TEXT};

/// Ein Token mit seinem Rohtext. `Leer` fasst jede Leerfolge zu einem
/// Token zusammen (fuer Nachfahren-Kombinatoren und Wert-Trennung).
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// Bezeichner: Tags, Eigenschaften, Schluesselwoerter.
    Ident(String),
    /// `name(` — Funktionsaufruf (rgb(, hsl(, not(, ...).
    Funktion(String),
    /// `@media`, `@import`, ... (ohne @).
    At(String),
    /// `#id` (ohne #).
    Hash(String),
    /// `"..."` / `'...'` (ohne Anfuehrung, mit Escape-Aufloesung).
    Zeichenkette(String),
    /// Zahl mit Einheit roh: ("12.5", "px"), ("50", "%"), ("0", "").
    Zahl(String, String),
    /// Einzelnes Sonderzeichen (`. : ; { } ( ) [ ] , > + ~ * = ...`).
    Zeichen(char),
    /// Leerfolge (Leer, Tab, Zeilenumbruch).
    Leer,
}

impl Token {
    /// Rohtext zurueck (fuer Wert-Rekonstruktion im Parser).
    pub fn text(&self) -> String {
        match self {
            Token::Ident(s) => s.clone(),
            Token::Funktion(s) => format!("{s}("),
            Token::At(s) => format!("@{s}"),
            Token::Hash(s) => format!("#{s}"),
            // Anfuehrung nur angedeutet: der Parser vergleicht Werte
            // ohne sie, die Rekonstruktion bleibt eindeutig genug.
            // Release 11.10.2026: `"` und `\` im Text werden wieder
            // geschuetzt (CSS Syntax 3 §4.3.5) — GEMESSEN vorher: `quotes:
            // '"' '"'` kam als `""" """` an, ein Paar leerer Ketten.
            Token::Zeichenkette(s) if s.contains(['"', '\\', '\n']) => {
                let mut raus = String::with_capacity(s.len() + 4);
                raus.push('"');
                for c in s.chars() {
                    match c {
                        '"' => raus.push_str("\\\""),
                        '\\' => raus.push_str("\\\\"),
                        '\n' => raus.push_str("\\a "),
                        _ => raus.push(c),
                    }
                }
                raus.push('"');
                raus
            }
            Token::Zeichenkette(s) => format!("\"{s}\""),
            Token::Zahl(z, e) => format!("{z}{e}"),
            Token::Zeichen(c) => c.to_string(),
            Token::Leer => String::from(" "),
        }
    }
}

/// Kuerzt auf MAX_EINGABE Bytes an einer Zeichengrenze (liefert auch,
/// ob abgeschnitten wurde). Die Rueckwaertssuche ist auf 4 Schritte
/// begrenzt (UTF-8 ist hoechstens 4 Byte breit).
pub fn kuerzen(text: &str) -> (&str, bool) {
    if text.len() <= MAX_EINGABE {
        return (text, false);
    }
    let mut i = MAX_EINGABE;
    for _ in 0..4 {
        if text.is_char_boundary(i) {
            break;
        }
        i -= 1;
    }
    (text.split_at(i).0, true)
}

/// Zerlegt CSS-Quelltext in Token. Rueckgabe: (token, vollständig)? —
/// `vollstaendig` ist false, wenn die Token-Grenze griff (der Rest der
/// Eingabe wurde nicht tokenisiert). Nie Panic, nie mehr als MAX_TOKEN.
pub fn tokenisieren(text: &str) -> (Vec<Token>, bool) {
    let bytes = text.as_bytes();
    let n = bytes.len();
    let mut token: Vec<Token> = Vec::new();
    let mut pos = 0usize;
    // Jede Runde verbraucht mindestens 1 Byte (siehe Zweige unten) und
    // hoert bei MAX_TOKEN auf — terminiert immer (M3).
    while pos < n && token.len() < MAX_TOKEN {
        let b = bytes[pos];
        // 1. Leerfolge zu einem Leer-Token zusammenfassen.
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == 0x0C {
            while pos < n {
                let c = bytes[pos];
                if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == 0x0C {
                    pos += 1;
                } else {
                    break;
                }
            }
            token.push(Token::Leer);
            continue;
        }
        // 2. Kommentar: bis */ oder Eingabeende ueberspringen.
        if b == b'/' && pos + 1 < n && bytes[pos + 1] == b'*' {
            pos += 2;
            while pos + 1 < n && !(bytes[pos] == b'*' && bytes[pos + 1] == b'/') {
                pos += 1;
            }
            // Unabgeschlossenes Kommentar (C6-Bombe): Rest faellt weg.
            pos = if pos + 1 < n { pos + 2 } else { n };
            continue;
        }
        // 3. Zeichenkette mit Escape-Aufloesung.
        if b == b'"' || b == b'\'' {
            let ende = b;
            pos += 1;
            let mut s = String::new();
            while pos < n && bytes[pos] != ende {
                if bytes[pos] == b'\\' && pos + 1 < n {
                    // Release 08.10.2026: CSS Syntax 3 §4.3.7 — `\` plus 1–6
                    // Hexziffern ist ein Codepunkt, `\` vor einem
                    // Zeilenumbruch eine Fortsetzung, sonst das Zeichen
                    // woertlich. GEMESSEN vorher: `content: "\f00c"` wurde
                    // zum Text „f00c", `"\2022"` zu „2022" statt „•".
                    if matches!(bytes[pos + 1], b'\n' | 0x0C) {
                        pos += 2;
                    } else if bytes[pos + 1] == b'\r' {
                        pos += if bytes.get(pos + 2) == Some(&b'\n') { 3 } else { 2 };
                    } else {
                        let (c, neu) = escape_lesen(text, pos);
                        s.push(c);
                        pos = neu;
                    }
                } else if bytes[pos] == b'\n' {
                    // Zeilenumbruch im String: String endet (CSS-Regel).
                    break;
                } else if bytes[pos] < 0x80 {
                    s.push(bytes[pos] as char);
                    pos += 1;
                } else {
                    // Mehrbyte-Zeichen (Umlaute in font-family etc.).
                    match text[pos..].chars().next() {
                        Some(c) => {
                            s.push(c);
                            pos += c.len_utf8();
                        }
                        None => {
                            // Ungueltiges UTF-8: Ersatzzeichen statt Panic.
                            s.push('\u{FFFD}');
                            pos += 1;
                        }
                    }
                }
                // Lange Strings kuermen wir: mehr als MAX_TOKEN_TEXT
                // Zeichen kann kein Attributwert der Teilmenge sein.
                if s.len() >= MAX_TOKEN_TEXT {
                    // Rest des Strings ueberspringen (bis Ende-Zeichen).
                    while pos < n && bytes[pos] != ende {
                        if bytes[pos] == b'\\' && pos + 1 < n {
                            pos += 2;
                        } else {
                            pos += 1;
                        }
                    }
                    break;
                }
            }
            if pos < n && bytes[pos] == ende {
                pos += 1;
            }
            token.push(Token::Zeichenkette(s));
            continue;
        }
        // 4. Zahl: Vorzeichen, Ziffern, Punkt, Exponent, Einheit.
        // Beginnt bei Ziffer, bei Punkt+Ziffer oder bei Vorzeichen
        // gefolgt von Ziffer bzw. Punkt+Ziffer ("-.5px" ist gueltig).
        let zahl_start = b.is_ascii_digit()
            || (b == b'.' && pos + 1 < n && bytes[pos + 1].is_ascii_digit())
            || ((b == b'+' || b == b'-')
                && pos + 1 < n
                && (bytes[pos + 1].is_ascii_digit()
                    || (bytes[pos + 1] == b'.' && pos + 2 < n && bytes[pos + 2].is_ascii_digit())));
        if zahl_start {
            let start = pos;
            if bytes[pos] == b'+' || bytes[pos] == b'-' {
                pos += 1;
            }
            while pos < n && bytes[pos].is_ascii_digit() {
                pos += 1;
            }
            if pos < n && bytes[pos] == b'.' && pos + 1 < n && bytes[pos + 1].is_ascii_digit() {
                pos += 2;
                while pos < n && bytes[pos].is_ascii_digit() {
                    pos += 1;
                }
            }
            if pos < n && (bytes[pos] == b'e' || bytes[pos] == b'E') {
                let mut q = pos + 1;
                if q < n && (bytes[q] == b'+' || bytes[q] == b'-') {
                    q += 1;
                }
                if q < n && bytes[q].is_ascii_digit() {
                    pos = q + 1;
                    while pos < n && bytes[pos].is_ascii_digit() {
                        pos += 1;
                    }
                }
            }
            let zahl = &text[start..pos];
            // Einheit danach: % oder Bezeichner.
            let mut einheit = String::new();
            if pos < n && bytes[pos] == b'%' {
                einheit.push('%');
                pos += 1;
            } else if pos < n && ist_namensstart(bytes[pos]) {
                let ustart = pos;
                while pos < n && ist_namenszeichen(bytes[pos]) {
                    pos += 1;
                }
                einheit.push_str(&text[ustart..pos]);
                if einheit.len() > 16 {
                    // An einer Zeichengrenze kuerzen (Koordination 30.09.,
                    // gefunden mit narsil-fuzz: Nicht-ASCII-Einheit -> Panic).
                    let mut g = 16;
                    while g > 0 && !einheit.is_char_boundary(g) {
                        g -= 1;
                    }
                    einheit.truncate(g);
                }
            }
            let mut z = String::from(zahl);
            if z.len() > 32 {
                z.truncate(32);
            }
            token.push(Token::Zahl(z, einheit));
            continue;
        }
        // 5. #name -> Hash.
        if b == b'#' && pos + 1 < n && (ist_namenszeichen(bytes[pos + 1]) || ist_escape(bytes, pos + 1)) {
            let (name, neu) = name_lesen(text, pos + 1);
            pos = neu;
            token.push(Token::Hash(name));
            continue;
        }
        // 6. @name -> At-Regel.
        if b == b'@' && pos + 1 < n && ist_namensstart(bytes[pos + 1]) {
            let (name, neu) = name_lesen(text, pos + 1);
            pos = neu;
            token.push(Token::At(name));
            continue;
        }
        // 7. Name oder Funktion: Buchstabe/_/- + Folgzeichen, evtl. `(`.
        //
        // Release 08.10.2026: mit CSS-ESCAPES (CSS Syntax 3 §4.3.7). GEMESSEN
        // vorher: `.sm\:hidden`, `.w-1\/2`, `.\31 0` wurden verworfen — das
        // sind die Klassen von Tailwind (`sm:hidden`, `lg:flex`, `w-1/2`), auf
        // vielen modernen Seiten die Mehrzahl aller Regeln.
        if ist_namensstart(b) || ist_escape(bytes, pos) {
            let (name, neu) = name_lesen(text, pos);
            pos = neu;
            if pos < n && bytes[pos] == b'(' {
                pos += 1;
                // Release 14.10.2026 (Agent CSS2): an unquoted `url(` is ONE
                // token up to the closing parenthesis (CSS Syntax 3 §4.3.6).
                // Otherwise `100x100-blue-and-orange.png` became a number with
                // a 16-character unit and arrived as `100x100-blue-and-or.png`.
                // MEASURED before (WPT css-backgrounds `background-size-028`
                // and others, every image named `NxN-...png`): the image was
                // missing.
                let is_url = name.eq_ignore_ascii_case("url");
                token.push(Token::Funktion(name));
                if is_url {
                    let mut q = pos;
                    while q < n && bytes[q].is_ascii_whitespace() {
                        q += 1;
                    }
                    if q < n && bytes[q] != b'"' && bytes[q] != b'\'' {
                        let start = q;
                        while q < n && bytes[q] != b')' {
                            if bytes[q] == b'\\' && q + 1 < n {
                                q += 1;
                            }
                            q += 1;
                        }
                        let q = q.min(n);
                        let raw = text[start..q].trim_end();
                        if !raw.is_empty() {
                            // Release 14.10.2026 (Agent CSS2): escapes in an
                            // unquoted URL stand for their character (CSS
                            // Syntax 3 §4.3.7); a URL that then holds a space,
                            // quote or parenthesis travels as a string so the
                            // value stays one piece. MEASURED before (WPT
                            // `CSS2/syntax/uri-005`, `url(support/\\'green\\
                            // block.png)`): the backslashes stayed in the URL.
                            let url = unescape_url(raw);
                            if url.contains([' ', '\'', '"', '(', ')', '\\']) {
                                token.push(Token::Zeichenkette(url));
                            } else {
                                token.push(Token::Ident(url));
                            }
                        }
                        pos = q;
                    }
                }
            } else {
                token.push(Token::Ident(name));
            }
            continue;
        }
        // 8. Sonst: einzelnes Zeichen (Unicode als char).
        let rest = &text[pos..];
        match rest.chars().next() {
            None => break,
            Some(c) => {
                token.push(Token::Zeichen(c));
                pos += c.len_utf8();
            }
        }
    }
    (token, pos >= n)
}

/// Ist an `pos` ein gueltiges Escape (`\` ohne folgenden Zeilenumbruch)?
fn ist_escape(bytes: &[u8], pos: usize) -> bool {
    pos + 1 < bytes.len()
        && bytes[pos] == b'\\'
        && !matches!(bytes[pos + 1], b'\n' | b'\r' | 0x0C)
}

/// Liest ein Escape ab dem `\` an `pos`: 1–6 Hexziffern (plus ein
/// Leerraum) oder genau ein Zeichen woertlich. Rueckgabe: Zeichen, neue Lage.
fn escape_lesen(text: &str, pos: usize) -> (char, usize) {
    let bytes = text.as_bytes();
    let mut p = pos + 1;
    let start = p;
    let mut wert: u32 = 0;
    while p < bytes.len() && p - start < 6 && bytes[p].is_ascii_hexdigit() {
        wert = wert * 16 + (bytes[p] as char).to_digit(16).unwrap_or(0);
        p += 1;
    }
    if p > start {
        if p < bytes.len() && matches!(bytes[p], b' ' | b'\t' | b'\n' | b'\r' | 0x0C) {
            p += 1;
        }
        let c = char::from_u32(wert).filter(|_| wert != 0).unwrap_or('\u{FFFD}');
        (c, p)
    } else {
        match text.get(p..).and_then(|r| r.chars().next()) {
            Some(c) => (c, p + c.len_utf8()),
            None => ('\u{FFFD}', bytes.len()),
        }
    }
}

/// Liest einen Namen (Namenszeichen und Escapes) ab `pos`, hoechstens
/// MAX_TOKEN_TEXT Bytes behaltend; der Rest des Namens wird verbraucht.
fn name_lesen(text: &str, mut pos: usize) -> (String, usize) {
    let bytes = text.as_bytes();
    let mut s = String::new();
    while pos < bytes.len() {
        let c = if ist_namenszeichen(bytes[pos]) {
            match text.get(pos..).and_then(|r| r.chars().next()) {
                Some(c) => {
                    pos += c.len_utf8();
                    c
                }
                None => {
                    pos += 1;
                    '\u{FFFD}'
                }
            }
        } else if ist_escape(bytes, pos) {
            let (c, neu) = escape_lesen(text, pos);
            pos = neu;
            c
        } else {
            break;
        };
        if s.len() < MAX_TOKEN_TEXT {
            s.push(c);
        }
    }
    (s, pos)
}

/// Startzeichen eines Namens: Buchstabe, Unterstrich, Nicht-ASCII, Bindestrich.
fn ist_namensstart(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'-' || b >= 0x80
}

/// Folgezeichen eines Namens: Startzeichen plus Ziffern.
fn ist_namenszeichen(b: u8) -> bool {
    ist_namensstart(b) || b.is_ascii_digit()
}

/// Release 14.10.2026 (Agent CSS2): resolves the escapes of an unquoted URL
/// (CSS Syntax 3 §4.3.7): `\\` plus up to six hex digits (and one following
/// space) is that code point, `\\` plus any other character is the character.
fn unescape_url(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 && chars.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
            hex.push(chars.next().unwrap_or('0'));
        }
        if hex.is_empty() {
            if let Some(ch) = chars.next() {
                out.push(ch);
            }
            continue;
        }
        if chars.peek().is_some_and(|ch| ch.is_ascii_whitespace()) {
            chars.next();
        }
        let value = u32::from_str_radix(&hex, 16).unwrap_or(0xFFFD);
        out.push(char::from_u32(value).filter(|ch| *ch != '\0').unwrap_or('\u{FFFD}'));
    }
    out
}
