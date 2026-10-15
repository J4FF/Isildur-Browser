//! Release 12.10.2026: die Stuecke eines gelesenen Werts und die
//! Grundtypen (CSS Values 4 §4–§7): Laenge, Prozent, Zahl, Ganzzahl, Winkel,
//! Zeit, Frequenz, Aufloesung, Flex, Bezeichner, Zeichenketten, URLs.
//!
//! Ein Wert wird als Folge von [`S`] gelesen — in der kanonischen
//! Reihenfolge der Grammatik — und daraus geschrieben: angegeben mit
//! [`text`], berechnet mit [`berechnet_text`].

use super::farbe::Farbe;
use super::mathe::{self, Mathe, Umgebung, Ziel};
use super::syntax::{Kv, Tok, ident_text, text_text, url_text};
use super::zahl::zahl_text;

/// Eine Zahl: woertlich (Wert, Einheit klein; `""` Zahl, `"%"` Prozent) oder
/// eine Mathe-Funktion.
#[derive(Debug, Clone, PartialEq)]
pub enum Num {
    L(f64, String),
    M(Box<Mathe>),
}

impl Num {
    pub fn text(&self) -> String {
        match self {
            Num::L(w, e) => format!("{}{}", zahl_text(*w), e),
            Num::M(m) => m.text(),
        }
    }
    /// Der woertliche Wert (oder der einer ganz vereinfachten Mathe-Funktion).
    pub fn wert(&self) -> Option<(f64, &str)> {
        match self {
            Num::L(w, e) => Some((*w, e.as_str())),
            Num::M(m) => m.zahlwert(),
        }
    }
    pub fn ist_null(&self) -> bool {
        matches!(self, Num::L(w, _) if *w == 0.0)
    }
}

/// Ein Stueck eines Werts.
#[derive(Debug, Clone, PartialEq)]
pub enum S {
    /// Schluesselwort (klein) oder Bezeichner (wie geschrieben, geschuetzt).
    W(String),
    Z(Num),
    Farbe(Farbe),
    /// Fertiger Text (Zeichenkette, …).
    Txt(String),
    /// `url(…)`: Adresse und die kanonischen Modifikatoren.
    Url(String, String),
    Komma,
    Strich,
    /// Funktion: Name (klein) und Inhalt.
    F(String, Vec<S>),
    /// `[a b]` (Rasterliniennamen).
    Namen(Vec<String>),
    /// Eine Position (`<position>`): waagerechte und senkrechte Seite.
    Pos(Box<(Vec<S>, Vec<S>)>),
}

/// Ein Stueck als Text (angegeben bzw. berechnet).
pub fn stueck_text(s: &S, ber: Option<&Ber>) -> String {
    match s {
        S::W(w) => w.clone(),
        S::Z(n) => match ber {
            Some(b) => num_berechnet(n, b),
            None => n.text(),
        },
        S::Farbe(f) => match ber {
            Some(b) => f.berechnet(b.farbe.as_deref()),
            None => f.text(),
        },
        S::Txt(t) => t.clone(),
        S::Url(u, m) => {
            let u = match ber.and_then(|b| b.basis.as_ref()) {
                Some(f) => f(u),
                None => u.clone(),
            };
            let mut t = url_text(&u);
            if !m.is_empty() {
                t.pop();
                t.push(' ');
                t.push_str(m);
                t.push(')');
            }
            t
        }
        S::Komma => String::from(","),
        S::Strich => String::from("/"),
        S::F(n, inhalt) => format!("{}({})", n, text_mit(inhalt, ber)),
        S::Namen(v) => format!("[{}]", v.join(" ")),
        S::Pos(p) => match ber {
            Some(b) => format!("{} {}", seite_berechnet(&p.0, b), seite_berechnet(&p.1, b)),
            None => format!("{} {}", text(&p.0), text(&p.1)),
        },
    }
}

fn baum(n: &Num) -> mathe::K {
    match n {
        Num::L(w, e) => mathe::K::N(*w, e.clone()),
        Num::M(m) => m.wurzel.clone(),
    }
}

/// Eine Seite einer Position als berechneter Wert (CSS Backgrounds 3 §3.6:
/// Prozent bzw. Laenge, von rechts/unten als `calc(100% - x)`).
pub fn seite_berechnet(s: &[S], b: &Ber) -> String {
    // Logische Woerter (`x-start` …) bleiben als Wort stehen (WPT
    // background-position-x-computed); als Versatz-Seite gelten sie physisch.
    let prozent = |w: &str| match w {
        "left" | "top" => Some(0.0),
        "center" => Some(50.0),
        "right" | "bottom" => Some(100.0),
        _ => None,
    };
    match s {
        [S::W(w)] => prozent(w).map_or_else(|| w.clone(), |p| format!("{}%", zahl_text(p))),
        [S::Z(n)] => num_berechnet(n, b),
        [S::W(w), S::Z(n)] if prozent(w) == Some(0.0) || w == "x-start" || w == "y-start" => num_berechnet(n, b),
        [S::W(_), S::Z(n)] => {
            let k = mathe::K::Summe(vec![mathe::K::N(100.0, String::from("%")), mathe::K::Neg(Box::new(baum(n)))]);
            let m = Mathe { wurzel: mathe::vereinfachen(k, Some(&b.u)), ziel: Ziel::LaengeProzent, typ: mathe::Typ::default(), bereich: (f64::NEG_INFINITY, f64::INFINITY) };
            match m.zahlwert() {
                Some((w, e)) => format!("{}{}", zahl_text(w), e),
                None => m.text(),
            }
        }
        _ => text_mit(s, Some(b)),
    }
}

/// Fuer den berechneten Wert: Umgebung, `currentcolor`, Adressaufloesung.
pub struct Ber<'a> {
    pub u: Umgebung,
    pub farbe: Option<String>,
    pub basis: Option<&'a dyn Fn(&str) -> String>,
}

/// Eine Zahl als berechneter Wert (relative Einheiten aufgeloest).
pub fn num_berechnet(n: &Num, b: &Ber) -> String {
    match n {
        Num::L(w, e) => {
            let m = Mathe { wurzel: mathe::K::N(*w, e.clone()), ziel: Ziel::Zahl, typ: mathe::Typ::default(), bereich: (f64::NEG_INFINITY, f64::INFINITY) };
            let a = m.aufgeloest(&b.u);
            match a.zahlwert() {
                Some((w, e)) => format!("{}{}", zahl_text(w), e),
                None => n.text(),
            }
        }
        Num::M(m) => {
            let a = m.aufgeloest(&b.u);
            match a.zahlwert() {
                Some((w, e)) if w.is_finite() => {
                    // Halbe Werte runden zu +unendlich (CSS Values 4 §10.9,
                    // WPT calc-z-index-fractions: `calc(-3 / 2)` -> -1).
                    let w = if a.ziel == Ziel::Ganz { (w + 0.5).floor() } else { w };
                    let w = if e == "%" { w } else { w.clamp(a.bereich.0, a.bereich.1) };
                    format!("{}{}", zahl_text(w), e)
                }
                Some((w, e)) if w.is_nan() => format!("0{e}"),
                Some((w, e)) => {
                    // Unendlich wird am Rand des Wertebereichs geklemmt.
                    let g = if e == "px" { 33554428.0 } else { f64::MAX };
                    format!("{}{}", zahl_text(if w > 0.0 { g } else { -g }), e)
                }
                None => a.text(),
            }
        }
    }
}

thread_local! {
    /// Im Wert einer `subgrid`-Liste (auch in ihrem `repeat()`).
    static SUBGRID: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn text_mit(s: &[S], ber: Option<&Ber>) -> String {
    // `subgrid []` behaelt seine leeren Namenslisten, auch in `repeat()`
    // (WPT css-grid subgrid/grid-template-valid).
    let oben = matches!(s.first(), Some(S::W(w)) if w == "subgrid");
    let alt = SUBGRID.with(|c| c.get());
    if oben {
        SUBGRID.with(|c| c.set(true));
    }
    let r = text_mit_innen(s, ber, alt || oben);
    if oben {
        SUBGRID.with(|c| c.set(alt));
    }
    r
}

fn text_mit_innen(s: &[S], ber: Option<&Ber>, subgrid: bool) -> String {
    let mut out = String::new();
    for p in s {
        match p {
            S::Komma => {
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push_str(", ");
            }
            S::Strich => {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str("/ ");
            }
            // Leere Liniennamen `[]` fallen weg (WPT css-grid: `[] 150px []` -> `150px`).
            S::Namen(v) if v.is_empty() && !subgrid => {}
            _ => {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(&stueck_text(p, ber));
            }
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Die Stuecke als angegebener Wert.
pub fn text(s: &[S]) -> String {
    text_mit(s, None)
}

/// Die Stuecke als berechneter Wert.
pub fn berechnet_text(s: &[S], b: &Ber) -> String {
    text_mit(s, Some(b))
}

/// Welche Zahl gesucht ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    Laenge,
    LaengeProzent,
    Prozent,
    Zahl,
    Ganz,
    Winkel,
    WinkelProzent,
    /// Winkel oder einheitenlose 0 (`linear-gradient(0, …)`, `skew(0)`).
    WinkelNull,
    Zeit,
    Frequenz,
    Aufloesung,
    Flex,
    ZahlProzent,
}

impl Art {
    fn ziel(self) -> Option<Ziel> {
        Some(match self {
            Art::Laenge => Ziel::Laenge,
            Art::LaengeProzent => Ziel::LaengeProzent,
            Art::Prozent => Ziel::Prozent,
            Art::Zahl => Ziel::Zahl,
            Art::Ganz => Ziel::Ganz,
            Art::Winkel | Art::WinkelNull => Ziel::Winkel,
            Art::WinkelProzent => Ziel::WinkelProzent,
            Art::Zeit => Ziel::Zeit,
            Art::Frequenz => Ziel::Frequenz,
            Art::Aufloesung => Ziel::Aufloesung,
            Art::Flex => return None,
            Art::ZahlProzent => Ziel::ZahlProzent,
        })
    }
    fn basis(self) -> Option<usize> {
        Some(match self {
            Art::Laenge | Art::LaengeProzent => mathe::LAENGE,
            Art::Winkel | Art::WinkelProzent | Art::WinkelNull => mathe::WINKEL,
            Art::Zeit => mathe::ZEIT,
            Art::Frequenz => mathe::FREQUENZ,
            Art::Aufloesung => mathe::AUFLOESUNG,
            Art::Flex => mathe::FLEX,
            _ => return None,
        })
    }
    fn prozent(self) -> bool {
        matches!(self, Art::LaengeProzent | Art::Prozent | Art::WinkelProzent | Art::ZahlProzent)
    }
}

/// Eine Zahl der Art `art` im Bereich `[min, max]` (nur fuer woertliche
/// Werte; Mathe-Funktionen werden erst berechnet geklemmt).
pub fn numerisch(kv: &Kv, art: Art, min: f64, max: f64) -> Option<Num> {
    let im = |w: f64| w >= min && w <= max;
    match kv {
        Kv::T(Tok::Dim(w, _, e, _)) => {
            let e = e.to_ascii_lowercase();
            let (b, _, _) = mathe::einheit_info(&e)?;
            (Some(b) == art.basis() && im(*w)).then(|| Num::L(*w, e))
        }
        Kv::T(Tok::Prozent(w, _)) => (art.prozent() && im(*w)).then(|| Num::L(*w, String::from("%"))),
        Kv::T(Tok::Zahl(w, ganz, _)) => match art {
            Art::Zahl | Art::ZahlProzent => im(*w).then(|| Num::L(*w, String::new())),
            Art::Ganz => (*ganz && im(*w)).then(|| Num::L(*w, String::new())),
            Art::Laenge | Art::LaengeProzent => (*w == 0.0).then(|| Num::L(0.0, String::from("px"))),
            Art::WinkelNull => (*w == 0.0).then(|| Num::L(0.0, String::from("deg"))),
            _ => None,
        },
        Kv::F(n, a) => {
            let z = art.ziel()?;
            let mut m = mathe::lesen(n, a, z)?;
            m.bereich = (min, max);
            Some(Num::M(Box::new(m)))
        }
        _ => None,
    }
}

/// CSS-weite Schluesselwoerter (CSS Cascade 5 §7.3).
pub fn ist_css_weit(w: &str) -> bool {
    ["initial", "inherit", "unset", "revert", "revert-layer", "revert-rule"].iter().any(|k| w.eq_ignore_ascii_case(k))
}

/// `<custom-ident>` (CSS Values 4 §4.2): kein CSS-weites Wort, kein `default`,
/// keines aus `ohne`.
pub fn custom_ident(kv: &Kv, ohne: &[String]) -> Option<S> {
    let n = kv.ident()?;
    if ist_css_weit(n) || n.eq_ignore_ascii_case("default") || ohne.iter().any(|o| n.eq_ignore_ascii_case(o)) {
        return None;
    }
    Some(S::W(ident_text(n)))
}

/// `<url>` mit Modifikatoren (CSS Values 4 §4.5.1): `cross-origin()`,
/// `integrity()`, `referrer-policy()` — je hoechstens einmal, kanonisch in
/// dieser Reihenfolge.
pub fn url(kv: &Kv) -> Option<S> {
    match kv {
        Kv::T(Tok::Url(u)) => Some(S::Url(u.clone(), String::new())),
        Kv::F(n, a) if n.eq_ignore_ascii_case("url") || n.eq_ignore_ascii_case("src") => {
            let teile: Vec<&Kv> = a.iter().filter(|k| !k.ist_leer()).collect();
            let Some(Kv::T(Tok::Str(u))) = teile.first() else { return None };
            let mut mods: [Option<String>; 3] = [None, None, None];
            for t in &teile[1..] {
                let Kv::F(m, inhalt) = t else { return None };
                let inhalt: Vec<&Kv> = inhalt.iter().filter(|k| !k.ist_leer()).collect();
                let [arg] = inhalt.as_slice() else { return None };
                let m = m.to_ascii_lowercase();
                let (i, text) = match m.as_str() {
                    "cross-origin" => {
                        let w = arg.ident()?.to_ascii_lowercase();
                        if !matches!(w.as_str(), "anonymous" | "use-credentials") {
                            return None;
                        }
                        (0, format!("cross-origin({w})"))
                    }
                    "integrity" => {
                        let Kv::T(Tok::Str(s)) = arg else { return None };
                        (1, format!("integrity({})", text_text(s)))
                    }
                    "referrer-policy" => {
                        let w = arg.ident()?.to_ascii_lowercase();
                        const P: &[&str] = &[
                            "no-referrer",
                            "no-referrer-when-downgrade",
                            "same-origin",
                            "origin",
                            "strict-origin",
                            "origin-when-cross-origin",
                            "strict-origin-when-cross-origin",
                            "unsafe-url",
                        ];
                        if !P.contains(&w.as_str()) {
                            return None;
                        }
                        (2, format!("referrer-policy({w})"))
                    }
                    _ => return None,
                };
                if mods[i].is_some() {
                    return None;
                }
                mods[i] = Some(text);
            }
            let m: Vec<String> = mods.into_iter().flatten().collect();
            // Release 12.10.2026: `src("…")` bleibt `src()` (berechnet
            // `src(url("…"))`; WPT css-values attr-security).
            if n.eq_ignore_ascii_case("src") && m.is_empty() {
                return Some(S::F(String::from("src"), vec![S::Txt(text_text(u))]));
            }
            Some(S::Url(u.clone(), m.join(" ")))
        }
        _ => None,
    }
}

/// `<string>`.
pub fn zeichenkette(kv: &Kv) -> Option<S> {
    match kv {
        Kv::T(Tok::Str(s)) => Some(S::Txt(text_text(s))),
        _ => None,
    }
}
