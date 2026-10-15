//! Release 12.10.2026: Mathe-Funktionen nach CSS Values 4 §10 — `calc()`,
//! `min()/max()/clamp()`, `round()/mod()/rem()`, Trigonometrie,
//! `pow()/sqrt()/hypot()/log()/exp()`, `abs()/sign()`, `progress()`.
//!
//! Lesen (§10.8 Grammatik, `+`/`-` nur mit Leerzeichen), Typpruefung
//! (§10.9), Vereinfachung (§10.10) und Serialisierung (§10.12, Kinder
//! sortiert: Zahl, Prozent, Dimensionen nach Einheit, Rest). Mit einer
//! [`Umgebung`] werden relative Einheiten aufgeloest (berechneter Wert).
//!
//! GEMESSEN vorher (WPT css-values): `calc(10px + 20px)` blieb als Text
//! stehen (statt `calc(30px)`), `min()` ohne Argumente galt, `sin()`,
//! `round()`, `exp()` … waren ungueltig — rund 60 Testdateien rot.

use super::syntax::{Kv, Tok};
use super::zahl::zahl_text;

pub const LAENGE: usize = 0;
pub const WINKEL: usize = 1;
pub const ZEIT: usize = 2;
pub const FREQUENZ: usize = 3;
pub const AUFLOESUNG: usize = 4;
pub const FLEX: usize = 5;
pub const PROZENT: usize = 6;

/// Hoechstens so viele Knoten je Ausdruck (M3).
const MAX_KNOTEN: usize = 4096;
const MAX_TIEFE: usize = 32;

/// Ein CSS-Typ (§10.9.1): Exponent je Basistyp und Prozent-Hinweis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Typ {
    pub e: [i32; 7],
    pub hinweis: Option<usize>,
}

impl Typ {
    pub fn basis(b: usize) -> Typ {
        let mut t = Typ::default();
        t.e[b] = 1;
        t
    }
    fn ist_null(&self) -> bool {
        self.e.iter().all(|&x| x == 0)
    }
    fn hinweis_anwenden(&mut self, h: usize) {
        if h != PROZENT {
            self.e[h] += self.e[PROZENT];
            self.e[PROZENT] = 0;
        }
        self.hinweis = Some(h);
    }
    /// §10.9.1.1 „add two types".
    pub fn addieren(a: Typ, b: Typ) -> Option<Typ> {
        let (mut a, mut b) = (a, b);
        match (a.hinweis, b.hinweis) {
            (Some(x), Some(y)) if x != y => return None,
            (Some(x), None) => b.hinweis_anwenden(x),
            (None, Some(y)) => a.hinweis_anwenden(y),
            _ => {}
        }
        if a.e == b.e {
            return Some(a);
        }
        let hat_p = a.e[PROZENT] != 0 || b.e[PROZENT] != 0;
        let hat_anderes = (0..PROZENT).any(|i| a.e[i] != 0 || b.e[i] != 0);
        if hat_p && hat_anderes {
            for h in 0..PROZENT {
                let (mut x, mut y) = (a, b);
                x.hinweis_anwenden(h);
                y.hinweis_anwenden(h);
                if x.e == y.e {
                    return Some(x);
                }
            }
        }
        None
    }
    /// §10.9.1.2 „multiply two types".
    pub fn multiplizieren(a: Typ, b: Typ) -> Option<Typ> {
        let (mut a, mut b) = (a, b);
        match (a.hinweis, b.hinweis) {
            (Some(x), Some(y)) if x != y => return None,
            (Some(x), None) => b.hinweis_anwenden(x),
            (None, Some(y)) => a.hinweis_anwenden(y),
            _ => {}
        }
        for i in 0..7 {
            a.e[i] += b.e[i];
        }
        Some(a)
    }
    fn invertiert(mut self) -> Typ {
        for x in self.e.iter_mut() {
            *x = -*x;
        }
        self
    }
    pub fn ist_zahl(&self) -> bool {
        self.ist_null() && matches!(self.hinweis, None | Some(PROZENT))
    }
    /// Passt der Typ auf `<basis>` (ohne Prozent)?
    pub fn ist(&self, b: usize) -> bool {
        (0..7).all(|i| self.e[i] == i32::from(i == b)) && (self.hinweis.is_none() || self.hinweis == Some(b))
    }
}

/// Wofuer der Ausdruck steht — bestimmt Prozent-Typ und erlaubte Ergebnisse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ziel {
    Zahl,
    Ganz,
    Laenge,
    LaengeProzent,
    Prozent,
    Winkel,
    WinkelProzent,
    Zeit,
    Frequenz,
    Aufloesung,
    Flex,
    /// `<number> | <percentage>` (z. B. `opacity`): Prozent bleibt Prozent.
    ZahlProzent,
}

impl Ziel {
    fn prozent_typ(self) -> Typ {
        match self {
            Ziel::LaengeProzent => Typ { e: Typ::basis(LAENGE).e, hinweis: Some(LAENGE) },
            Ziel::WinkelProzent => Typ { e: Typ::basis(WINKEL).e, hinweis: Some(WINKEL) },
            _ => Typ { e: Typ::basis(PROZENT).e, hinweis: Some(PROZENT) },
        }
    }
    pub fn passt(self, t: &Typ) -> bool {
        let pz = (0..7).all(|i| t.e[i] == i32::from(i == PROZENT));
        match self {
            Ziel::Zahl | Ziel::Ganz => t.ist_zahl(),
            Ziel::Laenge => t.ist(LAENGE) && t.hinweis.is_none(),
            Ziel::LaengeProzent => t.ist(LAENGE) || pz,
            Ziel::Prozent => pz,
            Ziel::Winkel => t.ist(WINKEL) && t.hinweis.is_none(),
            Ziel::WinkelProzent => t.ist(WINKEL) || pz,
            Ziel::Zeit => t.ist(ZEIT) && t.hinweis.is_none(),
            Ziel::Frequenz => t.ist(FREQUENZ) && t.hinweis.is_none(),
            Ziel::Aufloesung => t.ist(AUFLOESUNG) && t.hinweis.is_none(),
            Ziel::Flex => t.ist(FLEX) && t.hinweis.is_none(),
            Ziel::ZahlProzent => t.ist_zahl() || pz,
        }
    }
    /// Die kanonische Einheit (fuer `calc(NaN * 1px)`).
    fn einheit(self) -> &'static str {
        match self {
            Ziel::Laenge | Ziel::LaengeProzent => "px",
            Ziel::Winkel | Ziel::WinkelProzent => "deg",
            Ziel::Zeit => "s",
            Ziel::Frequenz => "hz",
            Ziel::Aufloesung => "dppx",
            Ziel::Flex => "fr",
            Ziel::Prozent => "%",
            _ => "",
        }
    }
}

/// Einheit -> (Basistyp, kanonische Einheit, Faktor; `NaN` = relativ).
pub fn einheit_info(e: &str) -> Option<(usize, &'static str, f64)> {
    let r = f64::NAN;
    Some(match e {
        "px" => (LAENGE, "px", 1.0),
        "cm" => (LAENGE, "px", 96.0 / 2.54),
        "mm" => (LAENGE, "px", 96.0 / 25.4),
        "q" => (LAENGE, "px", 96.0 / 101.6),
        "in" => (LAENGE, "px", 96.0),
        "pt" => (LAENGE, "px", 96.0 / 72.0),
        "pc" => (LAENGE, "px", 16.0),
        "em" | "rem" | "ex" | "rex" | "ch" | "rch" | "cap" | "rcap" | "ic" | "ric" | "lh" | "rlh" | "vw" | "vh"
        | "vi" | "vb" | "vmin" | "vmax" | "svw" | "svh" | "svi" | "svb" | "svmin" | "svmax" | "lvw" | "lvh"
        | "lvi" | "lvb" | "lvmin" | "lvmax" | "dvw" | "dvh" | "dvi" | "dvb" | "dvmin" | "dvmax" | "cqw" | "cqh"
        | "cqi" | "cqb" | "cqmin" | "cqmax" => (LAENGE, "px", r),
        "deg" => (WINKEL, "deg", 1.0),
        "grad" => (WINKEL, "deg", 0.9),
        "rad" => (WINKEL, "deg", 180.0 / std::f64::consts::PI),
        "turn" => (WINKEL, "deg", 360.0),
        "s" => (ZEIT, "s", 1.0),
        "ms" => (ZEIT, "s", 0.001),
        "hz" => (FREQUENZ, "hz", 1.0),
        "khz" => (FREQUENZ, "hz", 1000.0),
        "dppx" | "x" => (AUFLOESUNG, "dppx", 1.0),
        "dpi" => (AUFLOESUNG, "dppx", 1.0 / 96.0),
        "dpcm" => (AUFLOESUNG, "dppx", 2.54 / 96.0),
        "fr" => (FLEX, "fr", 1.0),
        _ => return None,
    })
}

/// Werte, gegen die relative Einheiten und Prozente aufgeloest werden.
#[derive(Debug, Clone, Copy)]
pub struct Umgebung {
    pub em: f64,
    pub rem: f64,
    pub ex: f64,
    pub ch: f64,
    pub cap: f64,
    pub ic: f64,
    pub lh: f64,
    pub rlh: f64,
    pub vw: f64,
    pub vh: f64,
    /// Bezug fuer `%` (in kanonischer Einheit), falls bekannt.
    pub prozent: Option<f64>,
    /// Release 12.10.2026: Breite und Hoehe des naechsten Groessen-Containers
    /// in px (`cqw`/`cqh` …, CSS Contain 3 §6.1); ohne gelten die kleinen
    /// Ansichtseinheiten.
    pub cq: (Option<f64>, Option<f64>),
    /// Release 12.10.2026: `sibling-index()` und `sibling-count()` des
    /// Elements (CSS Values 5 §11), falls bekannt.
    pub geschwister: Option<(f64, f64)>,
}

impl Default for Umgebung {
    fn default() -> Self {
        Umgebung { em: 16.0, rem: 16.0, ex: 8.0, ch: 8.0, cap: 11.2, ic: 16.0, lh: 19.2, rlh: 19.2, vw: 800.0, vh: 600.0, prozent: None, cq: (None, None), geschwister: None }
    }
}

impl Umgebung {
    fn px(&self, e: &str) -> Option<f64> {
        let (vw, vh) = (self.vw / 100.0, self.vh / 100.0);
        let (cw, ch) = (self.cq.0.map_or(vw, |w| w / 100.0), self.cq.1.map_or(vh, |h| h / 100.0));
        Some(match e {
            "cqw" | "cqi" => cw,
            "cqh" | "cqb" => ch,
            "cqmin" => cw.min(ch),
            "cqmax" => cw.max(ch),
            "em" => self.em,
            "rem" => self.rem,
            "ex" => self.ex,
            "rex" => self.rem * 0.5,
            "ch" => self.ch,
            "rch" => self.rem * 0.5,
            "cap" => self.cap,
            "rcap" => self.rem * 0.7,
            "ic" => self.ic,
            "ric" => self.rem,
            "lh" => self.lh,
            "rlh" => self.rlh,
            "vw" | "vi" | "svw" | "svi" | "lvw" | "lvi" | "dvw" | "dvi" | "cqw" | "cqi" => vw,
            "vh" | "vb" | "svh" | "svb" | "lvh" | "lvb" | "dvh" | "dvb" | "cqh" | "cqb" => vh,
            "vmin" | "svmin" | "lvmin" | "dvmin" | "cqmin" => vw.min(vh),
            "vmax" | "svmax" | "lvmax" | "dvmax" | "cqmax" => vw.max(vh),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runden {
    Naechste,
    Auf,
    Ab,
    NachNull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MF {
    Min,
    Max,
    /// `clamp()`; die Grenzen koennen `none` sein (Bits: 1 = untere, 2 = obere fehlt).
    Clamp(u8),
    Round(Runden),
    Mod,
    Rem,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Pow,
    Sqrt,
    Hypot,
    Log,
    Exp,
    Abs,
    Sign,
    /// `true` = `no-clamp` (CSS Values 5 §8.1).
    Progress(bool),
}

impl MF {
    fn name(self) -> &'static str {
        match self {
            MF::Min => "min",
            MF::Max => "max",
            MF::Clamp(_) => "clamp",
            MF::Round(_) => "round",
            MF::Mod => "mod",
            MF::Rem => "rem",
            MF::Sin => "sin",
            MF::Cos => "cos",
            MF::Tan => "tan",
            MF::Asin => "asin",
            MF::Acos => "acos",
            MF::Atan => "atan",
            MF::Atan2 => "atan2",
            MF::Pow => "pow",
            MF::Sqrt => "sqrt",
            MF::Hypot => "hypot",
            MF::Log => "log",
            MF::Exp => "exp",
            MF::Abs => "abs",
            MF::Sign => "sign",
            MF::Progress(_) => "progress",
        }
    }
}

/// Ein Knoten des Rechenbaums (§10.8).
#[derive(Debug, Clone, PartialEq)]
pub enum K {
    /// Zahlwert: Wert und Einheit (klein; `""` = Zahl, `"%"` = Prozent).
    N(f64, String),
    Summe(Vec<K>),
    Produkt(Vec<K>),
    Neg(Box<K>),
    Inv(Box<K>),
    F(MF, Vec<K>),
    /// Eine Nicht-Mathe-Funktion als Blatt (z. B. `sibling-index()`): Text und Typ.
    Fremd(String, Typ),
}

/// Ist `name` eine Mathe-Funktion, die einen Rechenbaum beginnt?
pub fn ist_mathe(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "calc" | "-webkit-calc" | "min" | "max" | "clamp" | "round" | "mod" | "rem" | "sin" | "cos" | "tan" | "asin"
            | "acos" | "atan" | "atan2" | "pow" | "sqrt" | "hypot" | "log" | "exp" | "abs" | "sign" | "progress"
    )
}

thread_local! {
    /// Release 12.10.2026: waehrend `calc-size()` darf `size` als Laenge stehen.
    static GROESSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct Leser<'a> {
    kv: &'a [Kv],
    i: usize,
    ziel: Ziel,
    knoten: &'a mut usize,
}

fn leer_ueberspringen(kv: &[Kv], i: &mut usize) -> bool {
    let s = *i;
    while *i < kv.len() && kv[*i].ist_leer() {
        *i += 1;
    }
    *i > s
}

impl Leser<'_> {
    fn zaehlen(&mut self) -> Option<()> {
        *self.knoten += 1;
        (*self.knoten < MAX_KNOTEN).then_some(())
    }
    fn summe(&mut self, tiefe: usize) -> Option<(K, Typ)> {
        if tiefe > MAX_TIEFE {
            return None;
        }
        let (a, mut ta) = self.produkt(tiefe)?;
        let mut teile = vec![a];
        loop {
            let vor = self.i;
            let leer_vor = leer_ueberspringen(self.kv, &mut self.i);
            let op = match self.kv.get(self.i) {
                Some(Kv::T(Tok::Delim(c @ ('+' | '-')))) if leer_vor => *c,
                _ => {
                    self.i = vor;
                    break;
                }
            };
            self.i += 1;
            if !leer_ueberspringen(self.kv, &mut self.i) {
                return None;
            }
            let (b, tb) = self.produkt(tiefe)?;
            ta = Typ::addieren(ta, tb)?;
            teile.push(if op == '-' { K::Neg(Box::new(b)) } else { b });
            self.zaehlen()?;
        }
        Some((if teile.len() == 1 { teile.pop()? } else { K::Summe(teile) }, ta))
    }
    fn produkt(&mut self, tiefe: usize) -> Option<(K, Typ)> {
        leer_ueberspringen(self.kv, &mut self.i);
        let (a, mut ta) = self.wert(tiefe)?;
        let mut teile = vec![a];
        loop {
            let vor = self.i;
            leer_ueberspringen(self.kv, &mut self.i);
            let op = match self.kv.get(self.i) {
                Some(Kv::T(Tok::Delim(c @ ('*' | '/')))) => *c,
                _ => {
                    self.i = vor;
                    break;
                }
            };
            self.i += 1;
            leer_ueberspringen(self.kv, &mut self.i);
            let (b, tb) = self.wert(tiefe)?;
            if op == '/' {
                ta = Typ::multiplizieren(ta, tb.invertiert())?;
                teile.push(K::Inv(Box::new(b)));
            } else {
                ta = Typ::multiplizieren(ta, tb)?;
                teile.push(b);
            }
            self.zaehlen()?;
        }
        Some((if teile.len() == 1 { teile.pop()? } else { K::Produkt(teile) }, ta))
    }
    fn wert(&mut self, tiefe: usize) -> Option<(K, Typ)> {
        let kv = self.kv.get(self.i)?;
        self.i += 1;
        self.zaehlen()?;
        match kv {
            Kv::T(Tok::Zahl(w, _, _)) => Some((K::N(*w, String::new()), Typ::default())),
            // Prozente nur, wo sie einen Bezug haben (CSS Values 4 §10.9:
            // sonst ungueltig, z. B. `rotate: calc(sign(50%) * 1deg)`).
            Kv::T(Tok::Prozent(w, _)) => {
                if !matches!(self.ziel, Ziel::LaengeProzent | Ziel::Prozent | Ziel::WinkelProzent | Ziel::ZahlProzent) {
                    return None;
                }
                Some((K::N(*w, String::from("%")), self.ziel.prozent_typ()))
            }
            Kv::T(Tok::Dim(w, _, e, _)) => {
                let e = e.to_ascii_lowercase();
                let (b, _, _) = einheit_info(&e)?;
                Some((K::N(*w, e), Typ::basis(b)))
            }
            Kv::T(Tok::Ident(n)) => {
                if n.eq_ignore_ascii_case("size") && GROESSE.with(std::cell::Cell::get) {
                    return Some((K::Fremd(String::from("size"), Typ::basis(LAENGE)), Typ::basis(LAENGE)));
                }
                let w = match n.to_ascii_lowercase().as_str() {
                    "e" => std::f64::consts::E,
                    "pi" => std::f64::consts::PI,
                    "infinity" => f64::INFINITY,
                    "-infinity" => f64::NEG_INFINITY,
                    "nan" => f64::NAN,
                    _ => return None,
                };
                Some((K::N(w, String::new()), Typ::default()))
            }
            Kv::B('(', inhalt) => {
                let mut l = Leser { kv: inhalt, i: 0, ziel: self.ziel, knoten: self.knoten };
                let r = l.summe(tiefe + 1)?;
                leer_ueberspringen(inhalt, &mut l.i);
                (l.i == inhalt.len()).then_some(r)
            }
            Kv::F(name, args) => funktion(name, args, self.ziel, self.knoten, tiefe + 1),
            _ => None,
        }
    }
}

/// Die durch Kommas getrennten Argumente einer Funktion.
fn argumente(args: &[Kv]) -> Vec<&[Kv]> {
    let mut raus = Vec::new();
    let mut start = 0;
    for (i, k) in args.iter().enumerate() {
        if k.ist_komma() {
            raus.push(&args[start..i]);
            start = i + 1;
        }
    }
    raus.push(&args[start..]);
    raus
}

fn ist_none(a: &[Kv]) -> bool {
    let t = super::syntax::getrimmt(a);
    t.len() == 1 && t[0].ist_wort("none")
}

fn arg_lesen(a: &[Kv], ziel: Ziel, knoten: &mut usize, tiefe: usize) -> Option<(K, Typ)> {
    let mut l = Leser { kv: a, i: 0, ziel, knoten };
    let r = l.summe(tiefe)?;
    leer_ueberspringen(a, &mut l.i);
    (l.i == a.len()).then_some(r)
}

/// Eine Funktion im Rechenbaum lesen (Mathe- oder erlaubte Fremdfunktion).
fn funktion(name: &str, args: &[Kv], ziel: Ziel, knoten: &mut usize, tiefe: usize) -> Option<(K, Typ)> {
    if tiefe > MAX_TIEFE {
        return None;
    }
    let n = name.to_ascii_lowercase();
    // Baumzaehlende Funktionen (CSS Values 5 §11): ganze Zahlen.
    if matches!(n.as_str(), "sibling-index" | "sibling-count") {
        if !super::syntax::getrimmt(args).is_empty() {
            return None;
        }
        return Some((K::Fremd(format!("{n}()"), Typ::default()), Typ::default()));
    }
    let teile = argumente(args);
    let leer = teile.len() == 1 && super::syntax::getrimmt(teile[0]).is_empty();
    if leer {
        return None;
    }
    let einige = |knoten: &mut usize| -> Option<(Vec<K>, Typ)> {
        let mut ks = Vec::new();
        let mut typ: Option<Typ> = None;
        for t in &teile {
            let (k, ty) = arg_lesen(t, ziel, knoten, tiefe)?;
            typ = Some(match typ {
                None => ty,
                Some(a) => Typ::addieren(a, ty)?,
            });
            ks.push(k);
        }
        Some((ks, typ?))
    };
    let zahl = |k: &(K, Typ)| k.1.ist_zahl();
    Some(match n.as_str() {
        "calc" | "-webkit-calc" => {
            if teile.len() != 1 {
                return None;
            }
            arg_lesen(teile[0], ziel, knoten, tiefe)?
        }
        "min" | "max" | "hypot" => {
            let (ks, t) = einige(knoten)?;
            let f = match n.as_str() {
                "min" => MF::Min,
                "max" => MF::Max,
                _ => MF::Hypot,
            };
            (K::F(f, ks), t)
        }
        "clamp" => {
            if teile.len() != 3 {
                return None;
            }
            let mut bits = 0u8;
            let mut ks = Vec::new();
            let mut typ: Option<Typ> = None;
            for (i, t) in teile.iter().enumerate() {
                if i != 1 && ist_none(t) {
                    bits |= if i == 0 { 1 } else { 2 };
                    ks.push(K::N(0.0, String::new()));
                    continue;
                }
                let (k, ty) = arg_lesen(t, ziel, knoten, tiefe)?;
                typ = Some(match typ {
                    None => ty,
                    Some(a) => Typ::addieren(a, ty)?,
                });
                ks.push(k);
            }
            (K::F(MF::Clamp(bits), ks), typ?)
        }
        "round" => {
            let mut teile = teile.clone();
            let mut art = Runden::Naechste;
            if let [k] = super::syntax::getrimmt(teile[0]) {
                let s = k.ident().map(str::to_ascii_lowercase);
                let a = match s.as_deref() {
                    Some("nearest") => Some(Runden::Naechste),
                    Some("up") => Some(Runden::Auf),
                    Some("down") => Some(Runden::Ab),
                    Some("to-zero") => Some(Runden::NachNull),
                    _ => None,
                };
                if let Some(a) = a {
                    art = a;
                    teile.remove(0);
                }
            }
            if teile.is_empty() || teile.len() > 2 {
                return None;
            }
            let a = arg_lesen(teile[0], ziel, knoten, tiefe)?;
            let b = match teile.get(1) {
                Some(t) => arg_lesen(t, ziel, knoten, tiefe)?,
                None => {
                    if !zahl(&a) {
                        return None;
                    }
                    (K::N(1.0, String::new()), Typ::default())
                }
            };
            let t = Typ::addieren(a.1, b.1)?;
            (K::F(MF::Round(art), vec![a.0, b.0]), t)
        }
        "mod" | "rem" | "atan2" => {
            if teile.len() != 2 {
                return None;
            }
            let a = arg_lesen(teile[0], ziel, knoten, tiefe)?;
            let b = arg_lesen(teile[1], ziel, knoten, tiefe)?;
            let t = Typ::addieren(a.1, b.1)?;
            match n.as_str() {
                "mod" => (K::F(MF::Mod, vec![a.0, b.0]), t),
                "rem" => (K::F(MF::Rem, vec![a.0, b.0]), t),
                _ => (K::F(MF::Atan2, vec![a.0, b.0]), Typ::basis(WINKEL)),
            }
        }
        "sin" | "cos" | "tan" => {
            if teile.len() != 1 {
                return None;
            }
            let a = arg_lesen(teile[0], Ziel::Zahl, knoten, tiefe)?;
            if !(a.1.ist_zahl() || (a.1.ist(WINKEL) && a.1.hinweis.is_none())) {
                return None;
            }
            let f = match n.as_str() {
                "sin" => MF::Sin,
                "cos" => MF::Cos,
                _ => MF::Tan,
            };
            (K::F(f, vec![a.0]), Typ::default())
        }
        "asin" | "acos" | "atan" | "sqrt" | "exp" => {
            if teile.len() != 1 {
                return None;
            }
            let a = arg_lesen(teile[0], Ziel::Zahl, knoten, tiefe)?;
            if !a.1.ist_zahl() {
                return None;
            }
            let (f, t) = match n.as_str() {
                "asin" => (MF::Asin, Typ::basis(WINKEL)),
                "acos" => (MF::Acos, Typ::basis(WINKEL)),
                "atan" => (MF::Atan, Typ::basis(WINKEL)),
                "sqrt" => (MF::Sqrt, Typ::default()),
                _ => (MF::Exp, Typ::default()),
            };
            (K::F(f, vec![a.0]), t)
        }
        "pow" | "log" => {
            if (n == "pow" && teile.len() != 2) || (n == "log" && !(1..=2).contains(&teile.len())) {
                return None;
            }
            let mut ks = Vec::new();
            for t in &teile {
                let a = arg_lesen(t, Ziel::Zahl, knoten, tiefe)?;
                if !a.1.ist_zahl() {
                    return None;
                }
                ks.push(a.0);
            }
            (K::F(if n == "pow" { MF::Pow } else { MF::Log }, ks), Typ::default())
        }
        "abs" | "sign" => {
            if teile.len() != 1 {
                return None;
            }
            let a = arg_lesen(teile[0], ziel, knoten, tiefe)?;
            // Release 12.10.2026: Prozente brauchen einen Bezug (WPT
            // percentage-without-context: `sign(50%)` in einem Winkel).
            if (a.1.e[PROZENT] != 0 || a.1.hinweis.is_some_and(|h| h != PROZENT))
                && !matches!(ziel, Ziel::LaengeProzent | Ziel::Prozent | Ziel::WinkelProzent | Ziel::ZahlProzent)
            {
                return None;
            }
            if n == "abs" {
                (K::F(MF::Abs, vec![a.0]), a.1)
            } else {
                (K::F(MF::Sign, vec![a.0]), Typ::default())
            }
        }
        "progress" => {
            if teile.len() != 3 {
                return None;
            }
            // Release 12.10.2026: `progress(no-clamp …)` (CSS Values 5 §8.1;
            // WPT progress-serialize).
            let erst = super::syntax::getrimmt(teile[0]);
            let frei = matches!(erst.first(), Some(Kv::T(Tok::Ident(w))) if w.eq_ignore_ascii_case("no-clamp"));
            let mut ks = Vec::new();
            let mut typ: Option<Typ> = None;
            for (i, x) in teile.iter().enumerate() {
                let x: &[Kv] = if i == 0 && frei { super::syntax::getrimmt(&erst[1..]) } else { x };
                if x.is_empty() {
                    return None;
                }
                let (k, ty) = arg_lesen(x, ziel, knoten, tiefe)?;
                typ = Some(match typ {
                    None => ty,
                    Some(a) => Typ::addieren(a, ty)?,
                });
                ks.push(k);
            }
            let t = typ?;
            // Release 12.10.2026: drei Werte EINES Grundtyps; Prozente als
            // anderer Typ nur, wo sie einen Bezug haben (WPT progress-invalid:
            // `progress(5%, 0deg, 8deg)` in `opacity`, `10px * 10px`).
            if t.e.iter().filter(|&&x| x != 0).count() > 1 || t.e.iter().any(|&x| x != 0 && x != 1) {
                return None;
            }
            if let Some(h) = t.hinweis
                && h != PROZENT
                && !matches!((ziel, h), (Ziel::LaengeProzent, LAENGE) | (Ziel::WinkelProzent, WINKEL))
            {
                return None;
            }
            (K::F(MF::Progress(frei), ks), Typ::default())
        }
        _ => return None,
    })
}

/// Release 12.10.2026: eine Rechnung wie im Inneren von `calc()` (fuer
/// `calc-size()`), vereinfacht und OHNE `calc(` geschrieben. `groesse`:
/// `size` ist erlaubt.
pub fn summe_text(kv: &[Kv], ziel: Ziel, groesse: bool) -> Option<String> {
    let vorher = GROESSE.with(|g| g.replace(groesse));
    let mut knoten = 0usize;
    let r = arg_lesen(kv, ziel, &mut knoten, 0);
    GROESSE.with(|g| g.set(vorher));
    let (k, t) = r?;
    if !ziel.passt(&t) {
        return None;
    }
    let k = vereinfachen(k, None);
    Some(baum_text(&k, true))
}

/// Ein Ausdruck: der (vereinfachte) Baum und sein Ziel.
#[derive(Debug, Clone, PartialEq)]
pub struct Mathe {
    pub wurzel: K,
    pub ziel: Ziel,
    /// Der Ergebnistyp (vor dem Vereinfachen bestimmt).
    pub typ: Typ,
    /// Der erlaubte Bereich der Eigenschaft: der berechnete Wert wird darauf
    /// geklemmt (CSS Values 4 §10.12, „clamp to the range allowed").
    pub bereich: (f64, f64),
}

/// Eine Mathe-Funktion (`calc(…)`, `min(…)` …) fuer `ziel` lesen und fuer
/// den angegebenen Wert vereinfachen. `None` = ungueltig.
pub fn lesen(name: &str, args: &[Kv], ziel: Ziel) -> Option<Mathe> {
    if !ist_mathe(name) && !name.eq_ignore_ascii_case("sibling-index") && !name.eq_ignore_ascii_case("sibling-count") {
        return None;
    }
    let mut knoten = 0usize;
    let (k, t) = funktion(name, args, ziel, &mut knoten, 0)?;
    if !ziel.passt(&t) {
        return None;
    }
    // Mischungen aus Prozent und Laenge (`calc(10% + 1px)`) brauchen ein
    // Ziel, das Prozente gegen eine Laenge aufloest.
    Some(Mathe { wurzel: vereinfachen(k, None), ziel, typ: t, bereich: (f64::NEG_INFINITY, f64::INFINITY) })
}

fn ist_num(k: &K) -> bool {
    matches!(k, K::N(..))
}

/// Auf die kanonische Einheit bringen (soweit ohne Umgebung moeglich).
fn kanonisch(w: f64, e: &str, u: Option<&Umgebung>) -> (f64, String) {
    if e.is_empty() || e == "%" {
        if e == "%"
            && let Some(p) = u.and_then(|u| u.prozent)
        {
            return (w * p / 100.0, String::from("px"));
        }
        return (w, e.to_string());
    }
    match einheit_info(e) {
        Some((_, k, f)) if !f.is_nan() => (w * f, k.to_string()),
        Some((_, k, _)) => match u.and_then(|u| u.px(e)) {
            Some(px) => (w * px, k.to_string()),
            None => (w, e.to_string()),
        },
        None => (w, e.to_string()),
    }
}

fn trig_arg(k: &K) -> Option<f64> {
    match k {
        K::N(w, e) if e.is_empty() => Some(*w),
        K::N(w, e) if e == "deg" => Some(w.to_radians()),
        _ => None,
    }
}

fn gleiche_einheit(ks: &[K]) -> Option<(Vec<f64>, String)> {
    let mut e0: Option<&str> = None;
    let mut ws = Vec::new();
    for k in ks {
        let K::N(w, e) = k else { return None };
        match e0 {
            None => e0 = Some(e),
            Some(x) if x == e => {}
            _ => return None,
        }
        ws.push(*w);
    }
    Some((ws, e0?.to_string()))
}

fn runden(a: f64, b: f64, art: Runden) -> f64 {
    if b == 0.0 || (a.is_infinite() && b.is_infinite()) || a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a.is_infinite() {
        return a;
    }
    if b.is_infinite() {
        return match art {
            Runden::Naechste | Runden::NachNull => {
                if a.is_sign_negative() {
                    -0.0
                } else {
                    0.0
                }
            }
            Runden::Auf => {
                if a > 0.0 {
                    f64::INFINITY
                } else if a.is_sign_negative() {
                    -0.0
                } else {
                    0.0
                }
            }
            Runden::Ab => {
                if a < 0.0 {
                    f64::NEG_INFINITY
                } else if a.is_sign_negative() {
                    -0.0
                } else {
                    0.0
                }
            }
        };
    }
    let b = b.abs();
    let q = a / b;
    if q == q.trunc() {
        return a;
    }
    let unten = q.floor() * b;
    let oben = unten + b;
    let r = match art {
        Runden::Naechste => {
            if (a - unten) < (oben - a) {
                unten
            } else {
                oben
            }
        }
        Runden::Auf => oben,
        Runden::Ab => unten,
        Runden::NachNull => {
            if unten.abs() < oben.abs() {
                unten
            } else {
                oben
            }
        }
    };
    // Das Vorzeichen einer Null bleibt das von A (§10.6.2).
    if r == 0.0 && a.is_sign_negative() { -0.0 } else { r }
}

/// Eine Mathe-Funktion mit Zahl-Argumenten ausrechnen.
fn rechnen(f: MF, ks: &[K]) -> Option<K> {
    match f {
        MF::Min | MF::Max | MF::Hypot | MF::Round(_) | MF::Mod | MF::Rem | MF::Abs | MF::Sign | MF::Atan2 | MF::Clamp(_) | MF::Progress(_) => {
            let (ws, e) = gleiche_einheit(ks)?;
            let w = match f {
                MF::Min => ws.iter().copied().fold(f64::INFINITY, |a, b| if a.is_nan() || b.is_nan() { f64::NAN } else if b < a || (b == 0.0 && a == 0.0 && b.is_sign_negative()) { b } else { a }),
                MF::Max => ws.iter().copied().fold(f64::NEG_INFINITY, |a, b| if a.is_nan() || b.is_nan() { f64::NAN } else if b > a || (b == 0.0 && a == 0.0 && a.is_sign_negative()) { b } else { a }),
                MF::Hypot => ws.iter().map(|w| w * w).sum::<f64>().sqrt(),
                MF::Clamp(_) => {
                    let (lo, v, hi) = (ws[0], ws[1], ws[2]);
                    if lo.is_nan() || v.is_nan() || hi.is_nan() {
                        f64::NAN
                    } else {
                        let m = if v < hi || (v == 0.0 && hi == 0.0 && v.is_sign_negative()) { v } else { hi };
                        if lo > m || (lo == 0.0 && m == 0.0 && m.is_sign_negative() && !lo.is_sign_negative()) { lo } else { m }
                    }
                }
                MF::Round(art) => runden(ws[0], ws[1], art),
                MF::Mod => {
                    let (a, b) = (ws[0], ws[1]);
                    if b == 0.0 || a.is_infinite() {
                        f64::NAN
                    } else if b.is_infinite() {
                        if a == 0.0 || (a > 0.0) == (b > 0.0) { a } else { f64::NAN }
                    } else {
                        let r = a - b * (a / b).floor();
                        if r == 0.0 { if b < 0.0 { -0.0 } else { 0.0 } } else { r }
                    }
                }
                MF::Rem => {
                    let (a, b) = (ws[0], ws[1]);
                    if b == 0.0 || a.is_infinite() {
                        f64::NAN
                    } else if b.is_infinite() {
                        a
                    } else {
                        let r = a % b;
                        if r == 0.0 && a.is_sign_negative() { -0.0 } else { r }
                    }
                }
                MF::Abs => ws[0].abs(),
                MF::Sign => {
                    let a = ws[0];
                    let s = if a.is_nan() || a == 0.0 { a } else { a.signum() };
                    return Some(K::N(s, String::new()));
                }
                MF::Atan2 => return Some(K::N(ws[0].atan2(ws[1]).to_degrees(), String::from("deg"))),
                MF::Progress(frei) => {
                    let (v, a, b) = (ws[0], ws[1], ws[2]);
                    let p = (v - a) / (b - a);
                    // Gleicher Anfang und Ende: geklemmt 0, sonst ±∞ (0/0 -> 0).
                    let p = if p.is_nan() {
                        0.0
                    } else if frei {
                        p
                    } else if a == b {
                        0.0
                    } else {
                        p.clamp(0.0, 1.0)
                    };
                    return Some(K::N(p, String::new()));
                }
                _ => return None,
            };
            Some(K::N(w, e))
        }
        MF::Sin | MF::Cos | MF::Tan => {
            let x = trig_arg(ks.first()?)?;
            let w = match f {
                MF::Sin => x.sin(),
                MF::Cos => x.cos(),
                _ => {
                    // Asymptoten genau (§10.7.1): 90deg -> +∞, -90deg -> -∞.
                    let grad = x.to_degrees();
                    let r = grad.rem_euclid(360.0);
                    if (r - 90.0).abs() < 1e-9 {
                        f64::INFINITY
                    } else if (r - 270.0).abs() < 1e-9 {
                        f64::NEG_INFINITY
                    } else {
                        x.tan()
                    }
                }
            };
            Some(K::N(if w == 0.0 && x.is_sign_negative() && f != MF::Cos { -0.0 } else { w }, String::new()))
        }
        MF::Asin | MF::Acos | MF::Atan | MF::Sqrt | MF::Exp | MF::Pow | MF::Log => {
            let mut ws = Vec::new();
            for k in ks {
                match k {
                    K::N(w, e) if e.is_empty() => ws.push(*w),
                    _ => return None,
                }
            }
            let a = *ws.first()?;
            Some(match f {
                MF::Asin => K::N(a.asin().to_degrees(), String::from("deg")),
                MF::Acos => K::N(a.acos().to_degrees(), String::from("deg")),
                MF::Atan => K::N(a.atan().to_degrees(), String::from("deg")),
                MF::Sqrt => K::N(a.sqrt(), String::new()),
                MF::Exp => K::N(a.exp(), String::new()),
                MF::Pow => K::N(a.powf(*ws.get(1)?), String::new()),
                _ => K::N(
                    match ws.get(1) {
                        Some(b) => a.ln() / b.ln(),
                        None => a.ln(),
                    },
                    String::new(),
                ),
            })
        }
    }
}

/// Einheitenexponenten eines Produkts aus Zahlwerten (fuer §10.10 Schritt 9).
fn produkt_ausrechnen(ks: &[K]) -> Option<K> {
    let mut w = 1.0f64;
    let mut einheiten: Vec<(String, i32)> = Vec::new();
    for k in ks {
        let (x, e, exp) = match k {
            K::N(x, e) => (*x, e.clone(), 1),
            K::Inv(b) => match &**b {
                K::N(x, e) => (1.0 / *x, e.clone(), -1),
                _ => return None,
            },
            _ => return None,
        };
        w *= x;
        if !e.is_empty() {
            match einheiten.iter_mut().find(|(n, _)| *n == e) {
                Some(p) => p.1 += exp,
                None => einheiten.push((e, exp)),
            }
        }
    }
    einheiten.retain(|(_, x)| *x != 0);
    match einheiten.as_slice() {
        [] => Some(K::N(w, String::new())),
        [(e, 1)] => Some(K::N(w, e.clone())),
        _ => None,
    }
}

/// §10.10 „simplify a calculation tree". Mit `u` werden relative Einheiten
/// (und Prozente, falls `u.prozent`) aufgeloest.
pub fn vereinfachen(k: K, u: Option<&Umgebung>) -> K {
    match k {
        K::N(w, e) => {
            let (w, e) = kanonisch(w, &e, u);
            K::N(w, e)
        }
        K::Fremd(ref t, _) => match (u.and_then(|u| u.geschwister), t.as_str()) {
            (Some((i, _)), "sibling-index()") => K::N(i, String::new()),
            (Some((_, n)), "sibling-count()") => K::N(n, String::new()),
            _ => k,
        },
        K::Neg(c) => match vereinfachen(*c, u) {
            K::N(w, e) => K::N(-w, e),
            K::Neg(cc) => *cc,
            // Release 12.10.2026: eine Summe aus Zahlen wird ausmultipliziert
            // (wie Chrome; WPT css-values calc-nesting-002:
            // `300px - 1 * (0% + 100px)` -> `calc(0% + 200px)`).
            K::Summe(s) if s.iter().all(|x| matches!(x, K::N(..))) => K::Summe(
                s.into_iter()
                    .map(|x| match x {
                        K::N(w, e) => K::N(-w, e),
                        x => x,
                    })
                    .collect(),
            ),
            c => K::Neg(Box::new(c)),
        },
        K::Inv(c) => match vereinfachen(*c, u) {
            K::N(w, e) if e.is_empty() => K::N(1.0 / w, e),
            K::Inv(cc) => *cc,
            c => K::Inv(Box::new(c)),
        },
        K::Summe(ks) => {
            let mut flach: Vec<K> = Vec::new();
            for c in ks {
                match vereinfachen(c, u) {
                    K::Summe(inner) => flach.extend(inner),
                    c => flach.push(c),
                }
            }
            let mut raus: Vec<K> = Vec::new();
            for c in flach {
                if let K::N(w, e) = &c
                    && let Some(K::N(w2, _)) = raus.iter_mut().find(|r| matches!(r, K::N(_, e2) if e2 == e))
                {
                    *w2 += *w;
                    continue;
                }
                raus.push(c);
            }
            // Berechnet (mit Umgebung): eine Null-Laenge neben anderen Termen
            // faellt weg (`calc(10% + 0px)` -> `10%`, wie Chrome).
            if u.is_some() && raus.len() > 1 {
                raus.retain(|c| !matches!(c, K::N(w, e) if *w == 0.0 && !e.is_empty() && e != "%"));
                if raus.is_empty() {
                    raus.push(K::N(0.0, String::from("px")));
                }
            }
            if raus.len() == 1 { raus.pop().unwrap_or(K::N(0.0, String::new())) } else { K::Summe(raus) }
        }
        K::Produkt(ks) => {
            let mut flach: Vec<K> = Vec::new();
            for c in ks {
                match vereinfachen(c, u) {
                    K::Produkt(inner) => flach.extend(inner),
                    c => flach.push(c),
                }
            }
            // Zahlen zusammenfassen; steht genau EINE Dimension (oder ein
            // Prozent) dabei, geht die Zahl in sie ein (`2rad * pi * x` ->
            // `360deg * x`, wie Chrome).
            let mut zahl: Option<f64> = None;
            let mut rest: Vec<K> = Vec::new();
            for c in flach {
                match c {
                    K::N(w, e) if e.is_empty() => zahl = Some(zahl.unwrap_or(1.0) * w),
                    c => rest.push(c),
                }
            }
            let dims: Vec<usize> = rest.iter().enumerate().filter(|(_, c)| matches!(c, K::N(..))).map(|(i, _)| i).collect();
            if let (Some(z), [i]) = (zahl, dims.as_slice())
                && rest.len() > 1
                && let K::N(w, _) = &mut rest[*i]
            {
                *w *= z;
                zahl = None;
            }
            let mut raus: Vec<K> = Vec::new();
            if let Some(z) = zahl {
                raus.push(K::N(z, String::new()));
            }
            raus.extend(rest);
            if raus.len() == 1 {
                return raus.pop().unwrap_or(K::N(1.0, String::new()));
            }
            if raus.len() == 2
                && let (Some(K::N(z, ze)), Some(K::Summe(s))) = (raus.first(), raus.get(1))
                && ze.is_empty()
                && s.iter().all(ist_num)
            {
                let z = *z;
                return K::Summe(
                    s.iter()
                        .map(|c| match c {
                            K::N(w, e) => K::N(w * z, e.clone()),
                            c => c.clone(),
                        })
                        .collect(),
                );
            }
            if let Some(r) = produkt_ausrechnen(&raus) {
                return r;
            }
            K::Produkt(raus)
        }
        K::F(f, ks) => {
            let ks: Vec<K> = ks.into_iter().map(|c| vereinfachen(c, u)).collect();
            // `clamp(none, …)` ist `min()`/`max()` bzw. der Wert selbst.
            if let MF::Clamp(bits) = f
                && bits != 0
            {
                let mut ks = ks;
                let hi = ks.pop().unwrap_or(K::N(0.0, String::new()));
                let v = ks.pop().unwrap_or(K::N(0.0, String::new()));
                let lo = ks.pop().unwrap_or(K::N(0.0, String::new()));
                let neu = match bits {
                    1 => K::F(MF::Min, vec![v, hi]),
                    2 => K::F(MF::Max, vec![lo, v]),
                    _ => return v,
                };
                return vereinfachen(neu, u);
            }
            // Release 12.10.2026: Prozente in min()/max()/clamp() bleiben stehen,
            // bis sie aufgeloest sind — ihr Bezug kann negativ sein (CSS Values 4
            // §10.10; WPT minmax-percentage-serialize: `min(1%, 2%, 3%)`).
            let prozent_offen = matches!(f, MF::Min | MF::Max | MF::Clamp(_))
                && ks.len() > 1
                && ks.iter().any(|c| matches!(c, K::N(_, e) if e == "%"));
            if ks.iter().all(ist_num)
                && !prozent_offen
                && let Some(r) = rechnen(f, &ks)
            {
                return r;
            }
            // §10.10 Schritt 5: min()/max() teilweise zusammenfassen.
            if matches!(f, MF::Min | MF::Max) {
                let mut raus: Vec<K> = Vec::new();
                for c in ks {
                    if let K::N(w, e) = &c
                        && e != "%"
                        && let Some(K::N(w2, _)) = raus.iter_mut().find(|r| matches!(r, K::N(_, e2) if e2 == e))
                    {
                        *w2 = if f == MF::Min { w2.min(*w) } else { w2.max(*w) };
                        continue;
                    }
                    raus.push(c);
                }
                if raus.len() == 1 {
                    return raus.pop().unwrap_or(K::N(0.0, String::new()));
                }
                return K::F(f, raus);
            }
            K::F(f, ks)
        }
    }
}

fn rang(k: &K) -> (u8, String) {
    match k {
        K::N(_, e) if e.is_empty() => (0, String::new()),
        K::N(_, e) if e == "%" => (1, String::new()),
        K::N(_, e) => (2, e.clone()),
        _ => (3, String::new()),
    }
}

/// §10.12 „sort a calculation's children" (stabil).
fn sortiert(ks: &[K]) -> Vec<&K> {
    let mut v: Vec<&K> = ks.iter().collect();
    v.sort_by(|a, b| {
        let (ra, ea) = rang(a);
        let (rb, eb) = rang(b);
        if ra == 3 && rb == 3 { std::cmp::Ordering::Equal } else { (ra, ea).cmp(&(rb, eb)) }
    });
    v
}

fn num_text(w: f64, e: &str) -> String {
    if !w.is_finite() {
        let s = zahl_text(w);
        return if e.is_empty() { s } else { format!("{s} * 1{e}") };
    }
    format!("{}{}", zahl_text(w), e)
}

/// §10.12 „serialize a calculation tree". `oben`: ohne aeussere Klammern.
fn baum_text(k: &K, oben: bool) -> String {
    match k {
        K::N(w, e) => {
            if !w.is_finite() && !e.is_empty() && !oben {
                format!("({})", num_text(*w, e))
            } else {
                num_text(*w, e)
            }
        }
        K::Fremd(t, _) => t.clone(),
        K::F(f, ks) => {
            let mut s = String::from(f.name());
            s.push('(');
            if let MF::Progress(true) = f {
                s.push_str("no-clamp ");
            }
            if let MF::Round(art) = f {
                match art {
                    Runden::Naechste => {}
                    Runden::Auf => s.push_str("up, "),
                    Runden::Ab => s.push_str("down, "),
                    Runden::NachNull => s.push_str("to-zero, "),
                }
            }
            let teile: Vec<String> = ks.iter().map(|c| baum_text(c, true)).collect();
            s.push_str(&teile.join(", "));
            s.push(')');
            s
        }
        K::Neg(c) => format!("(-1 * {})", baum_text(c, false)),
        K::Inv(c) => format!("(1 / {})", baum_text(c, false)),
        K::Summe(ks) => {
            let v = sortiert(ks);
            let mut s = String::new();
            for (i, c) in v.iter().enumerate() {
                if i == 0 {
                    s.push_str(&baum_text(c, false));
                    continue;
                }
                match c {
                    K::Neg(cc) => {
                        s.push_str(" - ");
                        s.push_str(&baum_text(cc, false));
                    }
                    K::N(w, e) if *w < 0.0 || (*w == 0.0 && w.is_sign_negative()) => {
                        s.push_str(" - ");
                        s.push_str(&baum_text(&K::N(-*w, e.clone()), false));
                    }
                    c => {
                        s.push_str(" + ");
                        s.push_str(&baum_text(c, false));
                    }
                }
            }
            if oben { s } else { format!("({s})") }
        }
        K::Produkt(ks) => {
            let v = sortiert(ks);
            let mut s = String::new();
            for (i, c) in v.iter().enumerate() {
                if i == 0 {
                    s.push_str(&baum_text(c, false));
                    continue;
                }
                match c {
                    K::Inv(cc) => {
                        s.push_str(" / ");
                        s.push_str(&baum_text(cc, false));
                    }
                    c => {
                        s.push_str(" * ");
                        s.push_str(&baum_text(c, false));
                    }
                }
            }
            if oben { s } else { format!("({s})") }
        }
    }
}

impl Mathe {
    /// §10.12 „serialize a math function" (angegebener Wert).
    pub fn text(&self) -> String {
        match &self.wurzel {
            K::N(w, e) if !w.is_finite() => {
                let s = zahl_text(*w);
                if e.is_empty() { format!("calc({s})") } else { format!("calc({s} * 1{e})") }
            }
            // Eine Baumzaehl-Funktion allein steht ohne `calc()` (WPT
            // tree-counting: `calc(sibling-index())` -> `sibling-index()`).
            K::Fremd(t, _) => t.clone(),
            K::N(..) | K::Summe(_) | K::Produkt(_) | K::Neg(_) | K::Inv(_) => {
                format!("calc({})", baum_text(&self.wurzel, true))
            }
            // Release 12.10.2026: die Exponentialfunktionen bleiben in `calc()`
            // (wie Chrome; WPT css-values calc-complex-unresolved-serialize:
            // `calc(pow(2, sign(1em - 18px)))`).
            K::F(MF::Pow | MF::Sqrt | MF::Log | MF::Exp, _) => format!("calc({})", baum_text(&self.wurzel, true)),
            K::F(..) => baum_text(&self.wurzel, true),
        }
    }

    /// Mit Umgebung vereinfacht (berechneter Wert).
    pub fn aufgeloest(&self, u: &Umgebung) -> Mathe {
        Mathe { wurzel: vereinfachen(self.wurzel.clone(), Some(u)), ziel: self.ziel, typ: self.typ, bereich: self.bereich }
    }

    /// Der Wert, wenn der Baum ein einzelner Zahlwert ist.
    pub fn zahlwert(&self) -> Option<(f64, &str)> {
        match &self.wurzel {
            K::N(w, e) => Some((*w, e.as_str())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::syntax::komponenten;
    use super::*;

    fn s(t: &str, z: Ziel) -> String {
        let kv = komponenten(t);
        match kv.as_slice() {
            [Kv::F(n, a)] => lesen(n, a, z).map(|m| m.text()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    #[test]
    fn grundformen() {
        assert_eq!(s("calc(10px + 20px)", Ziel::Laenge), "calc(30px)");
        assert_eq!(s("calc(calc(300px - 1 * (0% + 100px)))", Ziel::LaengeProzent), "calc(0% + 200px)");
        assert_eq!(s("calc(calc(300px - (0% + 100px) * -1))", Ziel::LaengeProzent), "calc(0% + 400px)");
        assert_eq!(s("min(1%, 2%, 3%)", Ziel::LaengeProzent), "min(1%, 2%, 3%)");
        assert_eq!(s("min(1%)", Ziel::LaengeProzent), "calc(1%)");
        assert_eq!(s("progress(no-clamp 100px, 0px, 100px)", Ziel::Zahl), "calc(1)");
        assert_eq!(s("progress(no-clamp 10em, 0px, 10em)", Ziel::Zahl), "progress(no-clamp 10em, 0px, 10em)");
        assert_eq!(s("progress(2rad, 1rad, 1rad)", Ziel::Zahl), "calc(0)");
        assert_eq!(s("progress(no-clamp 2rad, 1rad, 1rad)", Ziel::Zahl), "calc(infinity)");
        assert_eq!(s("calc(0.5 * progress(no-clamp -100px, 0px, 100px))", Ziel::Zahl), "calc(-0.5)");
        assert_eq!(s("calc(10px + 1vmin + 10%)", Ziel::LaengeProzent), "calc(10% + 10px + 1vmin)");
        assert_eq!(s("min(1PX)", Ziel::Laenge), "calc(1px)");
        assert_eq!(s("min(1in)", Ziel::Laenge), "calc(96px)");
        assert_eq!(s("min(50px, 1in + 1px)", Ziel::Laenge), "calc(50px)");
        assert_eq!(s("min(1px, 1em)", Ziel::Laenge), "min(1px, 1em)");
        assert_eq!(
            s("calc(min(1px, 1in) + max(100px + 1em, 10px + 1in) + 1px)", Ziel::Laenge),
            "calc(2px + max(1em + 100px, 106px))"
        );
        assert_eq!(s("calc(2 * (.2 * min(1em, 1px)) + 1px)", Ziel::Laenge), "calc(1px + (0.4 * min(1em, 1px)))");
        assert_eq!(s("min(180px, calc(80vw - 24px))", Ziel::Laenge), "min(180px, -24px + 80vw)");
        assert_eq!(s("calc(1px+2px)", Ziel::Laenge), "");
        assert_eq!(s("min()", Ziel::Laenge), "");
        assert_eq!(s("calc(1px * NaN)", Ziel::Laenge), "calc(NaN * 1px)");
        assert_eq!(s("calc(1 * NaN)", Ziel::Zahl), "calc(NaN)");
        assert_eq!(s("calc(20px + calc(80px))", Ziel::Laenge), "calc(100px)");
        assert_eq!(s("clamp(none, 2px, 3em)", Ziel::Laenge), "min(2px, 3em)");
        assert_eq!(s("clamp(clamp(none, 2em, none), 4px, clamp(none, 6em, none))", Ziel::Laenge), "clamp(2em, 4px, 6em)");
        assert_eq!(s("round(1.1,1)", Ziel::Zahl), "calc(1)");
        assert_eq!(s("cos(0)", Ziel::Zahl), "calc(1)");
        assert_eq!(s("calc(1px * sibling-index())", Ziel::Laenge), "calc(1px * sibling-index())");
        assert_eq!(s("calc(10px / 1px * 1px)", Ziel::Laenge), "calc(10px)");
        assert_eq!(s("calc(1px + 1)", Ziel::Laenge), "");
        assert_eq!(s("min(1ms)", Ziel::Zeit), "calc(0.001s)");
        assert_eq!(s("calc(1vh + 2px + 3%)", Ziel::LaengeProzent), "calc(3% + 2px + 1vh)");
    }
}
