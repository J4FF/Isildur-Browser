//! Release 12.10.2026: Tokenizer und Komponentenwerte nach CSS Syntax 3
//! (§4 „Tokenization", §5.4.7 „consume a component value") — fuer das CSSOM
//! (Werte pruefen und kanonisch schreiben). Der Kaskaden-Tokenizer
//! (`token.rs`) bleibt, wie er ist: er ist auf echte Seiten getrimmt
//! (Kleinschreibung, Fehlertoleranz) und kennt weder Vorzeichen noch
//! Ganzzahl-Flag noch `url(`-Token.
//!
//! GEMESSEN vorher (WPT `css/*/parsing`): `e.style[p] = v` speicherte den
//! Rohtext — `0` blieb `0` statt `0px`, `AUTO` blieb gross, ungueltige
//! Werte wurden gesetzt.

/// Hoechstens so viele Token je Wert (Grenze gegen Bomben, M3).
pub const MAX_TOKEN_WERT: usize = 65_536;
/// Hoechstens so tief verschachtelte Bloecke/Funktionen.
pub const MAX_TIEFE: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    /// `name(` — nur als Tokenizer-Ausgabe; im Komponentenwert wird es `Kv::F`.
    Funk(String),
    At(String),
    /// `#name`; `true` = Typ „id" (beginnt wie ein Bezeichner).
    Hash(String, bool),
    Str(String),
    BadStr,
    Url(String),
    BadUrl,
    Delim(char),
    /// Zahl: Wert, Ganzzahl-Flag, ausdrueckliches Vorzeichen.
    Zahl(f64, bool, bool),
    Prozent(f64, bool),
    /// Dimension: Wert, Ganzzahl-Flag, Einheit (wie geschrieben), Vorzeichen.
    Dim(f64, bool, String, bool),
    Leer,
    Cdo,
    Cdc,
    Doppelpunkt,
    Semikolon,
    Komma,
    AufE,
    ZuE,
    AufR,
    ZuR,
    AufG,
    ZuG,
}

/// Ein Komponentenwert (CSS Syntax 3 §5): Token, Funktion oder Block.
#[derive(Debug, Clone, PartialEq)]
pub enum Kv {
    T(Tok),
    /// Funktion: Name (wie geschrieben) und Inhalt (mit Leerzeichen).
    F(String, Vec<Kv>),
    /// Block: oeffnende Klammer (`[`, `(`, `{`) und Inhalt.
    B(char, Vec<Kv>),
}

impl Kv {
    pub fn ist_leer(&self) -> bool {
        matches!(self, Kv::T(Tok::Leer))
    }
    pub fn ident(&self) -> Option<&str> {
        match self {
            Kv::T(Tok::Ident(s)) => Some(s),
            _ => None,
        }
    }
    /// Bezeichner, ASCII-fallunabhaengig gleich `w`?
    pub fn ist_wort(&self, w: &str) -> bool {
        self.ident().is_some_and(|s| s.eq_ignore_ascii_case(w))
    }
    pub fn ist_delim(&self, c: char) -> bool {
        matches!(self, Kv::T(Tok::Delim(d)) if *d == c)
    }
    pub fn ist_komma(&self) -> bool {
        matches!(self, Kv::T(Tok::Komma))
    }
    pub fn funk(&self) -> Option<(&str, &[Kv])> {
        match self {
            Kv::F(n, a) => Some((n, a)),
            _ => None,
        }
    }
}

fn ist_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c as u32 >= 0x80
}

fn ist_name(c: char) -> bool {
    ist_name_start(c) || c.is_ascii_digit() || c == '-'
}

fn ist_leer(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n')
}

struct Leser {
    z: Vec<char>,
    i: usize,
}

impl Leser {
    fn an(&self, k: usize) -> Option<char> {
        self.z.get(self.i + k).copied()
    }
    fn gueltiges_escape(a: Option<char>, b: Option<char>) -> bool {
        a == Some('\\') && b.is_some_and(|b| b != '\n')
    }
    fn beginnt_ident(a: Option<char>, b: Option<char>, c: Option<char>) -> bool {
        match a {
            Some('-') => b.is_some_and(|b| ist_name_start(b) || b == '-') || Self::gueltiges_escape(b, c),
            Some(x) if ist_name_start(x) => true,
            Some('\\') => Self::gueltiges_escape(a, b),
            _ => false,
        }
    }
    fn beginnt_zahl(a: Option<char>, b: Option<char>, c: Option<char>) -> bool {
        match a {
            Some('+' | '-') => {
                b.is_some_and(|b| b.is_ascii_digit()) || (b == Some('.') && c.is_some_and(|c| c.is_ascii_digit()))
            }
            Some('.') => b.is_some_and(|b| b.is_ascii_digit()),
            Some(x) => x.is_ascii_digit(),
            None => false,
        }
    }
    /// §4.3.7 „consume an escaped code point" (nach dem `\`).
    fn escape(&mut self) -> char {
        let Some(c) = self.an(0) else { return '\u{FFFD}' };
        if c.is_ascii_hexdigit() {
            let mut h = 0u32;
            let mut n = 0;
            while n < 6 && self.an(0).is_some_and(|c| c.is_ascii_hexdigit()) {
                h = h * 16 + self.an(0).and_then(|c| c.to_digit(16)).unwrap_or(0);
                self.i += 1;
                n += 1;
            }
            if self.an(0).is_some_and(ist_leer) {
                self.i += 1;
            }
            if h == 0 || (0xD800..=0xDFFF).contains(&h) || h > 0x10FFFF {
                return '\u{FFFD}';
            }
            return char::from_u32(h).unwrap_or('\u{FFFD}');
        }
        self.i += 1;
        c
    }
    fn name(&mut self) -> String {
        let mut s = String::new();
        loop {
            match self.an(0) {
                Some(c) if ist_name(c) => {
                    s.push(c);
                    self.i += 1;
                }
                Some('\\') if Self::gueltiges_escape(self.an(0), self.an(1)) => {
                    self.i += 1;
                    s.push(self.escape());
                }
                _ => return s,
            }
        }
    }
    fn zahl(&mut self) -> (f64, bool, bool) {
        let mut s = String::new();
        let mut ganz = true;
        let vorz = matches!(self.an(0), Some('+' | '-'));
        if vorz {
            if self.an(0) == Some('-') {
                s.push('-');
            }
            self.i += 1;
        }
        while self.an(0).is_some_and(|c| c.is_ascii_digit()) {
            s.push(self.an(0).unwrap_or('0'));
            self.i += 1;
        }
        if self.an(0) == Some('.') && self.an(1).is_some_and(|c| c.is_ascii_digit()) {
            ganz = false;
            if s.is_empty() || s == "-" {
                s.push('0');
            }
            s.push('.');
            self.i += 1;
            while self.an(0).is_some_and(|c| c.is_ascii_digit()) {
                s.push(self.an(0).unwrap_or('0'));
                self.i += 1;
            }
        }
        if matches!(self.an(0), Some('e' | 'E'))
            && (self.an(1).is_some_and(|c| c.is_ascii_digit())
                || (matches!(self.an(1), Some('+' | '-')) && self.an(2).is_some_and(|c| c.is_ascii_digit())))
        {
            ganz = false;
            s.push('e');
            self.i += 1;
            if let Some(c @ ('+' | '-')) = self.an(0) {
                s.push(c);
                self.i += 1;
            }
            while self.an(0).is_some_and(|c| c.is_ascii_digit()) {
                s.push(self.an(0).unwrap_or('0'));
                self.i += 1;
            }
        }
        let w = s.parse::<f64>().unwrap_or(0.0);
        (w, ganz, vorz)
    }
    fn numerisch(&mut self) -> Tok {
        let (w, ganz, vorz) = self.zahl();
        if Self::beginnt_ident(self.an(0), self.an(1), self.an(2)) {
            let e = self.name();
            return Tok::Dim(w, ganz, e, vorz);
        }
        if self.an(0) == Some('%') {
            self.i += 1;
            return Tok::Prozent(w, vorz);
        }
        Tok::Zahl(w, ganz, vorz)
    }
    fn zeichenkette(&mut self, ende: char) -> Tok {
        let mut s = String::new();
        loop {
            match self.an(0) {
                None => return Tok::Str(s),
                Some(c) if c == ende => {
                    self.i += 1;
                    return Tok::Str(s);
                }
                Some('\n') => return Tok::BadStr,
                Some('\\') => match self.an(1) {
                    None => self.i += 1,
                    Some('\n') => self.i += 2,
                    Some(_) => {
                        self.i += 1;
                        s.push(self.escape());
                    }
                },
                Some(c) => {
                    s.push(c);
                    self.i += 1;
                }
            }
        }
    }
    fn url_rest(&mut self) {
        loop {
            match self.an(0) {
                None => return,
                Some(')') => {
                    self.i += 1;
                    return;
                }
                Some('\\') if Self::gueltiges_escape(self.an(0), self.an(1)) => {
                    self.i += 1;
                    self.escape();
                }
                _ => self.i += 1,
            }
        }
    }
    fn url(&mut self) -> Tok {
        let mut s = String::new();
        while self.an(0).is_some_and(ist_leer) {
            self.i += 1;
        }
        loop {
            match self.an(0) {
                None => return Tok::Url(s),
                Some(')') => {
                    self.i += 1;
                    return Tok::Url(s);
                }
                Some(c) if ist_leer(c) => {
                    while self.an(0).is_some_and(ist_leer) {
                        self.i += 1;
                    }
                    if matches!(self.an(0), None | Some(')')) {
                        if self.an(0).is_some() {
                            self.i += 1;
                        }
                        return Tok::Url(s);
                    }
                    self.url_rest();
                    return Tok::BadUrl;
                }
                Some('"' | '\'' | '(') => {
                    self.url_rest();
                    return Tok::BadUrl;
                }
                Some(c) if (c as u32) < 0x09 || c as u32 == 0x0B || (0x0E..=0x1F).contains(&(c as u32)) || c as u32 == 0x7F => {
                    self.url_rest();
                    return Tok::BadUrl;
                }
                Some('\\') => {
                    if Self::gueltiges_escape(self.an(0), self.an(1)) {
                        self.i += 1;
                        s.push(self.escape());
                    } else {
                        self.url_rest();
                        return Tok::BadUrl;
                    }
                }
                Some(c) => {
                    s.push(c);
                    self.i += 1;
                }
            }
        }
    }
    fn identartig(&mut self) -> Tok {
        let n = self.name();
        if n.eq_ignore_ascii_case("url") && self.an(0) == Some('(') {
            self.i += 1;
            while self.an(0).is_some_and(ist_leer) && self.an(1).is_some_and(ist_leer) {
                self.i += 1;
            }
            let q = |c: Option<char>| matches!(c, Some('"' | '\''));
            if q(self.an(0)) || (self.an(0).is_some_and(ist_leer) && q(self.an(1))) {
                return Tok::Funk(n);
            }
            return self.url();
        }
        if self.an(0) == Some('(') {
            self.i += 1;
            return Tok::Funk(n);
        }
        Tok::Ident(n)
    }
    fn token(&mut self) -> Option<Tok> {
        // Kommentare
        while self.an(0) == Some('/') && self.an(1) == Some('*') {
            self.i += 2;
            while self.an(0).is_some() && !(self.an(0) == Some('*') && self.an(1) == Some('/')) {
                self.i += 1;
            }
            self.i = (self.i + 2).min(self.z.len());
        }
        let c = self.an(0)?;
        if ist_leer(c) {
            while self.an(0).is_some_and(ist_leer) {
                self.i += 1;
            }
            return Some(Tok::Leer);
        }
        Some(match c {
            '"' | '\'' => {
                self.i += 1;
                self.zeichenkette(c)
            }
            '#' => {
                if self.an(1).is_some_and(ist_name) || Self::gueltiges_escape(self.an(1), self.an(2)) {
                    self.i += 1;
                    let id = Self::beginnt_ident(self.an(0), self.an(1), self.an(2));
                    Tok::Hash(self.name(), id)
                } else {
                    self.i += 1;
                    Tok::Delim('#')
                }
            }
            '(' => {
                self.i += 1;
                Tok::AufR
            }
            ')' => {
                self.i += 1;
                Tok::ZuR
            }
            '[' => {
                self.i += 1;
                Tok::AufE
            }
            ']' => {
                self.i += 1;
                Tok::ZuE
            }
            '{' => {
                self.i += 1;
                Tok::AufG
            }
            '}' => {
                self.i += 1;
                Tok::ZuG
            }
            ',' => {
                self.i += 1;
                Tok::Komma
            }
            ':' => {
                self.i += 1;
                Tok::Doppelpunkt
            }
            ';' => {
                self.i += 1;
                Tok::Semikolon
            }
            '+' | '.' => {
                if Self::beginnt_zahl(self.an(0), self.an(1), self.an(2)) {
                    self.numerisch()
                } else {
                    self.i += 1;
                    Tok::Delim(c)
                }
            }
            '-' => {
                if Self::beginnt_zahl(self.an(0), self.an(1), self.an(2)) {
                    self.numerisch()
                } else if self.an(1) == Some('-') && self.an(2) == Some('>') {
                    self.i += 3;
                    Tok::Cdc
                } else if Self::beginnt_ident(self.an(0), self.an(1), self.an(2)) {
                    self.identartig()
                } else {
                    self.i += 1;
                    Tok::Delim('-')
                }
            }
            '<' => {
                if self.an(1) == Some('!') && self.an(2) == Some('-') && self.an(3) == Some('-') {
                    self.i += 4;
                    Tok::Cdo
                } else {
                    self.i += 1;
                    Tok::Delim('<')
                }
            }
            '@' => {
                if Self::beginnt_ident(self.an(1), self.an(2), self.an(3)) {
                    self.i += 1;
                    Tok::At(self.name())
                } else {
                    self.i += 1;
                    Tok::Delim('@')
                }
            }
            '\\' => {
                if Self::gueltiges_escape(self.an(0), self.an(1)) {
                    self.identartig()
                } else {
                    self.i += 1;
                    Tok::Delim('\\')
                }
            }
            c if c.is_ascii_digit() => self.numerisch(),
            c if ist_name_start(c) => self.identartig(),
            c => {
                self.i += 1;
                Tok::Delim(c)
            }
        })
    }
}

/// §3.3 Vorverarbeitung: CR/FF/CRLF -> LF, NUL -> U+FFFD.
fn vorverarbeiten(text: &str) -> Vec<char> {
    let mut z = Vec::with_capacity(text.len().min(1 << 20));
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\r' => {
                if it.peek() == Some(&'\n') {
                    it.next();
                }
                z.push('\n');
            }
            '\x0C' => z.push('\n'),
            '\0' => z.push('\u{FFFD}'),
            c => z.push(c),
        }
    }
    z
}

/// Den Text in Token zerlegen (hoechstens `MAX_TOKEN_WERT`).
pub fn tokens(text: &str) -> Vec<Tok> {
    let mut l = Leser { z: vorverarbeiten(text), i: 0 };
    let mut raus = Vec::new();
    while let Some(t) = l.token() {
        raus.push(t);
        if raus.len() >= MAX_TOKEN_WERT {
            break;
        }
    }
    raus
}

/// Wie [`tokens`], dazu je Token seine Lage (Zeichenindizes `von..bis` im
/// vorverarbeiteten Text, der mit zurueckkommt) — fuer Werte, die woertlich
/// bleiben (Custom Properties, `var()`).
pub fn tokens_pos(text: &str) -> (Vec<char>, Vec<(Tok, usize, usize)>) {
    let mut l = Leser { z: vorverarbeiten(text), i: 0 };
    let mut raus = Vec::new();
    loop {
        // Kommentare gehoeren zu keinem Token.
        while l.an(0) == Some('/') && l.an(1) == Some('*') {
            l.i += 2;
            while l.an(0).is_some() && !(l.an(0) == Some('*') && l.an(1) == Some('/')) {
                l.i += 1;
            }
            l.i = (l.i + 2).min(l.z.len());
        }
        let von = l.i;
        let Some(t) = l.token() else { break };
        raus.push((t, von, l.i));
        if raus.len() >= MAX_TOKEN_WERT {
            break;
        }
    }
    (l.z, raus)
}

/// Komponentenwerte aus einer Token-Folge.
pub fn komponenten_aus(t: &[Tok]) -> Vec<Kv> {
    let mut i = 0;
    let mut raus = Vec::new();
    while i < t.len() {
        raus.push(verbrauche(t, &mut i, 0));
    }
    raus
}

fn schliesser(c: char) -> Tok {
    match c {
        '[' => Tok::ZuE,
        '(' => Tok::ZuR,
        _ => Tok::ZuG,
    }
}

fn verbrauche(t: &[Tok], i: &mut usize, tiefe: usize) -> Kv {
    let tok = t[*i].clone();
    *i += 1;
    let block = match &tok {
        Tok::AufE => Some('['),
        Tok::AufR => Some('('),
        Tok::AufG => Some('{'),
        _ => None,
    };
    if let Some(b) = block {
        let mut inhalt = Vec::new();
        let zu = schliesser(b);
        while *i < t.len() {
            if t[*i] == zu {
                *i += 1;
                break;
            }
            if tiefe >= MAX_TIEFE {
                *i += 1;
                continue;
            }
            inhalt.push(verbrauche(t, i, tiefe + 1));
        }
        return Kv::B(b, inhalt);
    }
    if let Tok::Funk(n) = tok {
        let mut inhalt = Vec::new();
        while *i < t.len() {
            if t[*i] == Tok::ZuR {
                *i += 1;
                break;
            }
            if tiefe >= MAX_TIEFE {
                *i += 1;
                continue;
            }
            inhalt.push(verbrauche(t, i, tiefe + 1));
        }
        return Kv::F(n, inhalt);
    }
    Kv::T(tok)
}

/// §5.3.10 „parse a list of component values".
pub fn komponenten(text: &str) -> Vec<Kv> {
    let t = tokens(text);
    let mut i = 0;
    let mut raus = Vec::new();
    while i < t.len() {
        raus.push(verbrauche(&t, &mut i, 0));
    }
    raus
}

/// Komponentenwerte ohne Leerzeichen am Anfang und Ende.
pub fn getrimmt(kv: &[Kv]) -> &[Kv] {
    let a = kv.iter().position(|k| !k.ist_leer()).unwrap_or(kv.len());
    let e = kv.iter().rposition(|k| !k.ist_leer()).map_or(a, |p| p + 1);
    &kv[a..e.max(a)]
}

/// Komponentenwerte ohne jedes (oberste) Leerzeichen.
pub fn ohne_leer(kv: &[Kv]) -> Vec<&Kv> {
    kv.iter().filter(|k| !k.ist_leer()).collect()
}

/// Kommt irgendwo (auch verschachtelt) eine Funktion `name` vor?
pub fn enthaelt_funk(kv: &[Kv], name: &str) -> bool {
    kv.iter().any(|k| match k {
        Kv::F(n, a) => n.eq_ignore_ascii_case(name) || enthaelt_funk(a, name),
        Kv::B(_, a) => enthaelt_funk(a, name),
        _ => false,
    })
}

/// CSSOM §2.1 „serialize an identifier".
pub fn ident_text(s: &str) -> String {
    let mut r = String::with_capacity(s.len() + 2);
    let z: Vec<char> = s.chars().collect();
    for (i, &c) in z.iter().enumerate() {
        let u = c as u32;
        if u == 0 {
            r.push('\u{FFFD}');
        } else if (1..=0x1F).contains(&u) || u == 0x7F || (i == 0 && c.is_ascii_digit()) || (i == 1 && c.is_ascii_digit() && z[0] == '-') {
            r.push_str(&format!("\\{:x} ", u));
        } else if i == 0 && c == '-' && z.len() == 1 {
            r.push_str("\\-");
        } else if u >= 0x80 || c == '-' || c == '_' || c.is_ascii_alphanumeric() {
            r.push(c);
        } else {
            r.push('\\');
            r.push(c);
        }
    }
    r
}

/// CSSOM §2.1 „serialize a string".
pub fn text_text(s: &str) -> String {
    let mut r = String::with_capacity(s.len() + 2);
    r.push('"');
    for c in s.chars() {
        let u = c as u32;
        if u == 0 {
            r.push('\u{FFFD}');
        } else if (1..=0x1F).contains(&u) || u == 0x7F {
            r.push_str(&format!("\\{:x} ", u));
        } else if c == '"' || c == '\\' {
            r.push('\\');
            r.push(c);
        } else {
            r.push(c);
        }
    }
    r.push('"');
    r
}

/// CSSOM §2.1 „serialize a URL".
pub fn url_text(s: &str) -> String {
    format!("url({})", text_text(s))
}

/// Ein Token als Text (fuer die generische Serialisierung).
pub fn tok_text(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => ident_text(s),
        Tok::Funk(s) => format!("{}(", ident_text(s)),
        Tok::At(s) => format!("@{}", ident_text(s)),
        Tok::Hash(s, _) => format!("#{s}"),
        Tok::Str(s) => text_text(s),
        Tok::BadStr => String::new(),
        Tok::Url(s) => url_text(s),
        Tok::BadUrl => String::from("url()"),
        Tok::Delim(c) => c.to_string(),
        Tok::Zahl(w, _, _) => super::zahl::zahl_text(*w),
        Tok::Prozent(w, _) => format!("{}%", super::zahl::zahl_text(*w)),
        Tok::Dim(w, _, e, _) => format!("{}{}", super::zahl::zahl_text(*w), e),
        Tok::Leer => String::from(" "),
        Tok::Cdo => String::from("<!--"),
        Tok::Cdc => String::from("-->"),
        Tok::Doppelpunkt => String::from(":"),
        Tok::Semikolon => String::from(";"),
        Tok::Komma => String::from(","),
        Tok::AufE => String::from("["),
        Tok::ZuE => String::from("]"),
        Tok::AufR => String::from("("),
        Tok::ZuR => String::from(")"),
        Tok::AufG => String::from("{"),
        Tok::ZuG => String::from("}"),
    }
}

/// Komponentenwerte als Text: Leerfolgen werden ein Leerzeichen, nach
/// einem Komma steht genau eines (vor keinem), an den Raendern keines.
pub fn kv_text(kv: &[Kv]) -> String {
    let mut s = String::new();
    let kv = getrimmt(kv);
    let mut i = 0;
    while i < kv.len() {
        let k = &kv[i];
        if k.ist_leer() {
            // Leer vor Komma und schliessender Klammer faellt weg.
            let naechst = kv[i + 1..].iter().find(|k| !k.ist_leer());
            if !naechst.is_some_and(|n| n.ist_komma()) && !s.ends_with(' ') {
                s.push(' ');
            }
            i += 1;
            continue;
        }
        match k {
            Kv::T(Tok::Komma) => {
                while s.ends_with(' ') {
                    s.pop();
                }
                s.push_str(", ");
                i += 1;
                while i < kv.len() && kv[i].ist_leer() {
                    i += 1;
                }
                continue;
            }
            Kv::T(t) => s.push_str(&tok_text(t)),
            Kv::F(n, a) => {
                s.push_str(&ident_text(n));
                s.push('(');
                s.push_str(&kv_text(a));
                s.push(')');
            }
            Kv::B(b, a) => {
                s.push(*b);
                s.push_str(&kv_text(a));
                s.push(match b {
                    '[' => ']',
                    '(' => ')',
                    _ => '}',
                });
            }
        }
        i += 1;
    }
    while s.ends_with(' ') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grundtoken() {
        let t = tokens("1px -2.5e1% +3 calc( a ) url( x.png ) url(\"y\") #a1 'z'");
        assert!(matches!(t[0], Tok::Dim(w, true, ref e, false) if w == 1.0 && e == "px"));
        assert!(matches!(t[2], Tok::Prozent(w, true) if w == -25.0));
        assert!(matches!(t[4], Tok::Zahl(w, true, true) if w == 3.0));
        assert!(matches!(t[6], Tok::Funk(ref n) if n == "calc"));
        assert!(t.contains(&Tok::Url("x.png".into())));
        assert!(t.contains(&Tok::Funk("url".into())));
        assert!(t.contains(&Tok::Hash("a1".into(), true)));
        assert!(t.contains(&Tok::Str("z".into())));
    }

    #[test]
    fn komponenten_und_text() {
        let k = komponenten("rgb( 1 ,2,  3 )  solid");
        assert_eq!(kv_text(&k), "rgb(1, 2, 3) solid");
        assert_eq!(ident_text("1st"), "\\31 st");
        assert_eq!(text_text("a\"b"), "\"a\\\"b\"");
    }
}
