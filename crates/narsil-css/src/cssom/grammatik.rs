//! Release 12.10.2026: Wertgrammatiken in der Schreibweise der
//! Spezifikationen (CSS Values 4 §2 „Value Definition Syntax") — gelesen
//! aus Text wie `none | [ <length [0,inf]> && inset? ]#`, und ein Abgleich,
//! der einen Wert gegen die Grammatik prueft und seine Stuecke in der
//! Reihenfolge der Grammatik liefert (das ist die kanonische Reihenfolge
//! fuer `||` und `&&`, CSSOM §6.7.2).
//!
//! Erweiterungen der Schreibweise: `<typ [min,max]>` (Bereich, `inf`),
//! `<custom-ident !a !b>` (ausgeschlossene Woerter), `name( … )` (Funktion
//! mit eigener Grammatik fuer den Inhalt).

use std::collections::HashMap;
use std::sync::OnceLock;

use super::syntax::{Kv, Tok};
use super::werte::{self, Art, S};

#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Num(Art),
    Farbe,
    Url,
    Text,
    CustomIdent,
    DashedIdent,
    Ident,
    LinienNamen,
}

#[derive(Debug, Clone, PartialEq)]
pub enum G {
    Wort(String),
    Komma,
    Strich,
    Typ(Prim, f64, f64, Vec<String>),
    Regel(String),
    Folge(Vec<G>),
    Alle(Vec<G>),
    Einige(Vec<G>),
    Eins(Vec<G>),
    Wdh { g: Box<G>, min: usize, max: usize, komma: bool },
    Funk(String, Box<G>),
}

// ---------------------------------------------------------------------------
// Grammatik lesen
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum GT {
    Auf,
    Zu,
    Oder,
    OderOder,
    UndUnd,
    Frage,
    Stern,
    Plus,
    Raute,
    Ausruf,
    Klammer(usize, usize),
    Typ(String),
    Wort(String),
    Funk(String),
    FZu,
    Komma,
    Strich,
}

fn g_tokens(t: &str) -> Vec<GT> {
    let z: Vec<char> = t.chars().collect();
    let mut i = 0;
    let mut raus = Vec::new();
    while i < z.len() {
        let c = z[i];
        match c {
            ' ' | '\t' | '\n' => i += 1,
            '[' => {
                raus.push(GT::Auf);
                i += 1;
            }
            ']' => {
                raus.push(GT::Zu);
                i += 1;
            }
            ')' => {
                raus.push(GT::FZu);
                i += 1;
            }
            '|' if z.get(i + 1) == Some(&'|') => {
                raus.push(GT::OderOder);
                i += 2;
            }
            '|' => {
                raus.push(GT::Oder);
                i += 1;
            }
            '&' => {
                raus.push(GT::UndUnd);
                i += 2;
            }
            '?' => {
                raus.push(GT::Frage);
                i += 1;
            }
            '*' => {
                raus.push(GT::Stern);
                i += 1;
            }
            '+' => {
                raus.push(GT::Plus);
                i += 1;
            }
            '#' => {
                raus.push(GT::Raute);
                i += 1;
            }
            '!' => {
                raus.push(GT::Ausruf);
                i += 1;
            }
            ',' => {
                raus.push(GT::Komma);
                i += 1;
            }
            '/' => {
                raus.push(GT::Strich);
                i += 1;
            }
            '{' => {
                let e = z[i..].iter().position(|&c| c == '}').map_or(z.len(), |p| i + p);
                let inhalt: String = z[i + 1..e].iter().collect();
                let (a, b) = match inhalt.split_once(',') {
                    Some((a, "")) => (a.trim().parse().unwrap_or(0), usize::MAX),
                    Some((a, b)) => (a.trim().parse().unwrap_or(0), b.trim().parse().unwrap_or(0)),
                    None => {
                        let n = inhalt.trim().parse().unwrap_or(0);
                        (n, n)
                    }
                };
                raus.push(GT::Klammer(a, b));
                i = e + 1;
            }
            '<' => {
                let e = z[i..].iter().position(|&c| c == '>').map_or(z.len(), |p| i + p);
                raus.push(GT::Typ(z[i + 1..e].iter().collect()));
                i = e + 1;
            }
            _ => {
                let s = i;
                while i < z.len() && !matches!(z[i], ' ' | '\t' | '\n' | '[' | ']' | '|' | '&' | '?' | '*' | '+' | '#' | '{' | '<' | '(' | ')' | ',' | '/' | '!') {
                    i += 1;
                }
                // `!` am Ende eines Worts ist der Multiplikator.
                let w: String = z[s..i].iter().collect();
                if i < z.len() && z[i] == '(' {
                    raus.push(GT::Funk(w));
                    i += 1;
                } else if w.is_empty() {
                    i += 1;
                } else {
                    raus.push(GT::Wort(w));
                }
            }
        }
    }
    raus
}

struct GL {
    t: Vec<GT>,
    i: usize,
}

impl GL {
    fn an(&self) -> Option<&GT> {
        self.t.get(self.i)
    }
    fn eins(&mut self) -> Option<G> {
        let mut v = vec![self.einige()?];
        while self.an() == Some(&GT::Oder) {
            self.i += 1;
            v.push(self.einige()?);
        }
        Some(if v.len() == 1 { v.pop()? } else { G::Eins(v) })
    }
    fn einige(&mut self) -> Option<G> {
        let mut v = vec![self.alle()?];
        while self.an() == Some(&GT::OderOder) {
            self.i += 1;
            v.push(self.alle()?);
        }
        Some(if v.len() == 1 { v.pop()? } else { G::Einige(v) })
    }
    fn alle(&mut self) -> Option<G> {
        let mut v = vec![self.folge()?];
        while self.an() == Some(&GT::UndUnd) {
            self.i += 1;
            v.push(self.folge()?);
        }
        Some(if v.len() == 1 { v.pop()? } else { G::Alle(v) })
    }
    fn folge(&mut self) -> Option<G> {
        let mut v = Vec::new();
        while let Some(t) = self.an() {
            if matches!(t, GT::Zu | GT::FZu | GT::Oder | GT::OderOder | GT::UndUnd) {
                break;
            }
            v.push(self.mult()?);
        }
        if v.is_empty() {
            return None;
        }
        Some(if v.len() == 1 { v.pop()? } else { G::Folge(v) })
    }
    fn mult(&mut self) -> Option<G> {
        let mut g = self.atom()?;
        loop {
            let (min, max, komma) = match self.an() {
                Some(GT::Frage) => (0, 1, false),
                Some(GT::Stern) => (0, usize::MAX, false),
                Some(GT::Plus) => (1, usize::MAX, false),
                Some(GT::Raute) => {
                    self.i += 1;
                    if let Some(GT::Klammer(a, b)) = self.an().cloned() {
                        self.i += 1;
                        g = G::Wdh { g: Box::new(g), min: a, max: b, komma: true };
                    } else {
                        g = G::Wdh { g: Box::new(g), min: 1, max: usize::MAX, komma: true };
                    }
                    continue;
                }
                Some(GT::Klammer(a, b)) => (*a, *b, false),
                Some(GT::Ausruf) => {
                    // `[ … ]!`: mindestens ein Teil muss da sein — beim
                    // Abgleich ergibt eine leere Gruppe ohnehin nichts.
                    self.i += 1;
                    continue;
                }
                _ => return Some(g),
            };
            self.i += 1;
            g = G::Wdh { g: Box::new(g), min, max, komma };
        }
    }
    fn atom(&mut self) -> Option<G> {
        let t = self.an()?.clone();
        self.i += 1;
        Some(match t {
            GT::Auf => {
                let g = self.eins()?;
                if self.an() != Some(&GT::Zu) {
                    return None;
                }
                self.i += 1;
                g
            }
            GT::Komma => G::Komma,
            GT::Strich => G::Strich,
            GT::Wort(w) => G::Wort(w),
            GT::Funk(n) => {
                let g = self.eins()?;
                if self.an() != Some(&GT::FZu) {
                    return None;
                }
                self.i += 1;
                G::Funk(n, Box::new(g))
            }
            GT::Typ(t) => typ(&t),
            _ => return None,
        })
    }
}

fn typ(t: &str) -> G {
    let mut teile = t.split_whitespace();
    let name = teile.next().unwrap_or("");
    let rest: Vec<&str> = teile.collect();
    let mut min = f64::NEG_INFINITY;
    let mut max = f64::INFINITY;
    let mut ohne = Vec::new();
    for r in &rest {
        if let Some(b) = r.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            let (a, c) = b.split_once(',').unwrap_or((b, b));
            let z = |s: &str| match s.trim() {
                "inf" | "∞" => f64::INFINITY,
                "-inf" | "-∞" => f64::NEG_INFINITY,
                s => s.parse().unwrap_or(0.0),
            };
            min = z(a);
            max = z(c);
        } else if let Some(w) = r.strip_prefix('!') {
            ohne.push(w.to_string());
        }
    }
    let p = match name {
        "length" => Prim::Num(Art::Laenge),
        "length-percentage" => Prim::Num(Art::LaengeProzent),
        "percentage" => Prim::Num(Art::Prozent),
        "number" => Prim::Num(Art::Zahl),
        "integer" => Prim::Num(Art::Ganz),
        "angle" => Prim::Num(Art::Winkel),
        "angle-percentage" => Prim::Num(Art::WinkelProzent),
        "angle-zero" => Prim::Num(Art::WinkelNull),
        "time" => Prim::Num(Art::Zeit),
        "frequency" => Prim::Num(Art::Frequenz),
        "resolution" => Prim::Num(Art::Aufloesung),
        "flex" => Prim::Num(Art::Flex),
        "number-percentage" => Prim::Num(Art::ZahlProzent),
        "color" => Prim::Farbe,
        "url" => Prim::Url,
        "string" => Prim::Text,
        "custom-ident" => Prim::CustomIdent,
        "dashed-ident" => Prim::DashedIdent,
        "ident" => Prim::Ident,
        "line-names" => Prim::LinienNamen,
        andere => return G::Regel(andere.to_string()),
    };
    G::Typ(p, min, max, ohne)
}

/// Eine Grammatik aus ihrer Schreibweise lesen.
pub fn lesen(text: &str) -> Option<G> {
    let mut l = GL { t: g_tokens(text), i: 0 };
    let g = l.eins()?;
    (l.i == l.t.len()).then_some(g)
}

// ---------------------------------------------------------------------------
// Benannte Regeln
// ---------------------------------------------------------------------------

/// Die benannten Teilgrammatiken (`<line-style>`, `<track-size>` …).
pub fn regeln() -> &'static HashMap<&'static str, G> {
    static R: OnceLock<HashMap<&'static str, G>> = OnceLock::new();
    R.get_or_init(|| {
        let mut m = HashMap::new();
        for (n, t) in super::eigenschaften::REGELN.iter().chain(super::eigenschaften::REGELN2) {
            if let Some(g) = lesen(t) {
                m.insert(*n, g);
            }
        }
        m
    })
}

// ---------------------------------------------------------------------------
// Abgleich
// ---------------------------------------------------------------------------

const MAX_ERGEBNISSE: usize = 48;
const MAX_SCHRITTE: usize = 200_000;

type Erg = Vec<(usize, Vec<S>)>;

struct M<'a> {
    items: &'a [&'a Kv],
    schritte: usize,
    tiefe: usize,
    /// `||`/`&&` in Grammatik-Reihenfolge schreiben (sonst wie angegeben).
    sortieren: bool,
}

fn eindeutig(v: Erg) -> Erg {
    let mut raus: Erg = Vec::new();
    for (p, o) in v {
        if !raus.iter().any(|(q, _)| *q == p) {
            raus.push((p, o));
            if raus.len() >= MAX_ERGEBNISSE {
                break;
            }
        }
    }
    raus
}

impl M<'_> {
    fn passe(&mut self, g: &G, pos: usize) -> Erg {
        self.schritte += 1;
        if self.schritte > MAX_SCHRITTE || self.tiefe > 64 {
            return Vec::new();
        }
        let it = self.items.get(pos).copied();
        match g {
            G::Wort(w) => match it {
                Some(k) if k.ist_wort(w) => vec![(pos + 1, vec![S::W(w.to_ascii_lowercase())])],
                _ => Vec::new(),
            },
            G::Komma => match it {
                Some(k) if k.ist_komma() => vec![(pos + 1, vec![S::Komma])],
                _ => Vec::new(),
            },
            G::Strich => match it {
                Some(k) if k.ist_delim('/') => vec![(pos + 1, vec![S::Strich])],
                _ => Vec::new(),
            },
            G::Typ(p, min, max, ohne) => {
                let Some(k) = it else { return Vec::new() };
                match prim(p, *min, *max, ohne, k) {
                    Some(s) => vec![(pos + 1, vec![s])],
                    None => Vec::new(),
                }
            }
            G::Regel(n) => {
                match n.as_str() {
                    "position" | "bg-position" => return super::sonder::position(self.items, pos, n == "bg-position"),
                    "calc-size" | "calc-size-max" | "calc-size-flex" => {
                        let Some(k) = it else { return Vec::new() };
                        return super::sonder::calc_size(k, n != "calc-size-max", n == "calc-size-flex")
                            .map(|s| vec![(pos + 1, vec![s])])
                            .unwrap_or_default();
                    }
                    "display" | "areas" | "font-family" => {
                        if pos != 0 {
                            return Vec::new();
                        }
                        let r = match n.as_str() {
                            "display" => super::sonder::display(self.items),
                            "areas" => super::sonder::areas(self.items),
                            _ => super::sonder::font_family(self.items),
                        };
                        return r.map(|o| vec![(self.items.len(), o)]).unwrap_or_default();
                    }
                    _ => {}
                }
                let Some(r) = regeln().get(n.as_str()) else { return Vec::new() };
                self.tiefe += 1;
                let e = self.passe(r, pos);
                self.tiefe -= 1;
                e
            }
            G::Funk(n, inner) => {
                let Some(Kv::F(f, args)) = it else { return Vec::new() };
                if !f.eq_ignore_ascii_case(n) {
                    return Vec::new();
                }
                let innen: Vec<&Kv> = args.iter().filter(|k| !k.ist_leer()).collect();
                let mut m = M { items: &innen, schritte: self.schritte, tiefe: self.tiefe + 1, sortieren: self.sortieren };
                let e = m.passe(inner, 0);
                self.schritte = m.schritte;
                match e.into_iter().find(|(p, _)| *p == innen.len()) {
                    Some((_, o)) => vec![(pos + 1, vec![S::F(funk_name(n), o)])],
                    None => Vec::new(),
                }
            }
            G::Folge(v) => {
                let mut stand: Erg = vec![(pos, Vec::new())];
                for sub in v {
                    let mut neu: Erg = Vec::new();
                    for (p, o) in &stand {
                        for (p2, o2) in self.passe(sub, *p) {
                            let mut o = o.clone();
                            o.extend(o2);
                            neu.push((p2, o));
                        }
                    }
                    stand = eindeutig(neu);
                    if stand.is_empty() {
                        break;
                    }
                }
                stand
            }
            G::Eins(v) => {
                let mut raus = Vec::new();
                for sub in v {
                    raus.extend(self.passe(sub, pos));
                }
                eindeutig(raus)
            }
            G::Einige(v) | G::Alle(v) => {
                let alle = matches!(g, G::Alle(_));
                let mut raus: Vec<(usize, Vec<(usize, Vec<S>)>)> = Vec::new();
                self.kombi(v, pos, 0, Vec::new(), alle, &mut raus);
                // Laengste zuerst, dann in Grammatik-Reihenfolge zusammensetzen.
                raus.sort_by(|a, b| b.0.cmp(&a.0));
                let sortieren = self.sortieren;
                let fertig: Erg = raus
                    .into_iter()
                    .map(|(p, mut teile)| {
                        if sortieren {
                            teile.sort_by_key(|(i, _)| *i);
                        }
                        (p, teile.into_iter().flat_map(|(_, o)| o).collect())
                    })
                    .collect();
                eindeutig(fertig)
            }
            G::Wdh { g: sub, min, max, komma } => {
                let mut raus: Erg = Vec::new();
                let mut stand: Erg = vec![(pos, Vec::new())];
                if *min == 0 {
                    raus.push((pos, Vec::new()));
                }
                let mut n = 0usize;
                while !stand.is_empty() && n < *max && n < 1024 {
                    let mut neu: Erg = Vec::new();
                    for (p, o) in &stand {
                        let mut start = *p;
                        let mut o = o.clone();
                        if *komma && n > 0 {
                            match self.items.get(start) {
                                Some(k) if k.ist_komma() => {
                                    start += 1;
                                    o.push(S::Komma);
                                }
                                _ => continue,
                            }
                        }
                        for (p2, o2) in self.passe(sub, start) {
                            if p2 == *p {
                                continue;
                            }
                            let mut oo = o.clone();
                            oo.extend(o2);
                            neu.push((p2, oo));
                        }
                    }
                    n += 1;
                    stand = eindeutig(neu);
                    if n >= *min {
                        // Gierig: mehr Wiederholungen vorn.
                        let mut v = stand.clone();
                        v.extend(raus);
                        raus = v;
                    }
                }
                eindeutig(raus)
            }
        }
    }

    fn kombi(&mut self, v: &[G], pos: usize, benutzt: u64, acc: Vec<(usize, Vec<S>)>, alle: bool, raus: &mut Vec<(usize, Vec<(usize, Vec<S>)>)>) {
        if raus.len() > MAX_ERGEBNISSE * 4 || self.schritte > MAX_SCHRITTE {
            return;
        }
        let voll = benutzt.count_ones() as usize == v.len();
        if (alle && voll) || (!alle && benutzt != 0) {
            raus.push((pos, acc.clone()));
        }
        if voll {
            return;
        }
        for (i, sub) in v.iter().enumerate() {
            if benutzt & (1 << i) != 0 {
                continue;
            }
            for (p2, o2) in self.passe(sub, pos) {
                // Ein leerer Treffer zaehlt nur bei `&&` (optionale Teile).
                if p2 == pos && !alle {
                    continue;
                }
                let mut a = acc.clone();
                a.push((i, o2));
                self.kombi(v, p2, benutzt | (1 << i), a, alle, raus);
            }
        }
    }
}

fn prim(p: &Prim, min: f64, max: f64, ohne: &[String], k: &Kv) -> Option<S> {
    match p {
        Prim::Num(a) => werte::numerisch(k, *a, min, max).map(S::Z),
        Prim::Farbe => super::farbe::lesen(k).map(S::Farbe),
        Prim::Url => werte::url(k),
        Prim::Text => werte::zeichenkette(k),
        Prim::CustomIdent => werte::custom_ident(k, ohne),
        Prim::DashedIdent => {
            let n = k.ident()?;
            n.starts_with("--").then(|| S::W(super::syntax::ident_text(n)))
        }
        Prim::Ident => {
            let n = k.ident()?;
            Some(S::W(super::syntax::ident_text(n)))
        }
        Prim::LinienNamen => {
            let Kv::B('[', inhalt) = k else { return None };
            let mut namen = Vec::new();
            for x in inhalt.iter().filter(|x| !x.ist_leer()) {
                match werte::custom_ident(x, &[String::from("span"), String::from("auto")]) {
                    Some(S::W(n)) => namen.push(n),
                    _ => return None,
                }
            }
            Some(S::Namen(namen))
        }
    }
}

/// Den ganzen Wert `items` gegen `g` pruefen; die Stuecke des ersten
/// vollstaendigen Treffers.
pub fn abgleichen(g: &G, items: &[&Kv]) -> Option<Vec<S>> {
    abgleichen_mit(g, items, true)
}

/// Wie [`abgleichen`]; `sortieren = false` behaelt die angegebene Folge der
/// `||`-Teile (`hanging-punctuation`, `word-space-transform`: so schreibt
/// Chrome sie, WPT css-text).
pub fn abgleichen_mit(g: &G, items: &[&Kv], sortieren: bool) -> Option<Vec<S>> {
    let mut m = M { items, schritte: 0, tiefe: 0, sortieren };
    m.passe(g, 0).into_iter().find(|(p, _)| *p == items.len()).map(|(_, o)| o)
}

/// Der kanonische Funktionsname: klein, Transform-Funktionen in der
/// Schreibweise der Spezifikation (`translateX`, wie Chrome sie schreibt).
fn funk_name(n: &str) -> String {
    const GEMISCHT: &[&str] = &[
        "translateX", "translateY", "translateZ", "translate3d", "scaleX", "scaleY", "scaleZ", "scale3d", "rotateX", "rotateY",
        "rotateZ", "rotate3d", "skewX", "skewY", "matrix3d",
    ];
    GEMISCHT.iter().find(|g| g.eq_ignore_ascii_case(n)).map_or_else(|| n.to_ascii_lowercase(), |g| (*g).to_string())
}

/// Ein Token ist ein Wort? (fuer Aufrufer)
pub fn ist_tok_wort(k: &Kv) -> bool {
    matches!(k, Kv::T(Tok::Ident(_)))
}
