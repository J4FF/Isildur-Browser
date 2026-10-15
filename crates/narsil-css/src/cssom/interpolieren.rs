//! Release 15.10.2026 (Agent C): interpolation of computed values for
//! animations and transitions (CSS Values 4 §3 "combining values", CSS Color
//! 4 §12, CSS Transforms 1 §10).
//!
//! Both values are computed values as `getComputedStyle` serializes them.
//! They are walked component by component: numbers, dimensions of the same
//! unit, percentages and colors interpolate, identical words stay; anything
//! else is discrete (the first value below 50 %, the second from there on).
//! Transform lists with different functions interpolate as matrices.

use super::farbe;
use super::syntax::{self, Kv, Tok};
use super::zahl::zahl_text;

/// Integer-valued properties: the interpolated number is rounded (half
/// towards positive infinity, CSS Values 4 §3.1).
const INTEGER: &[&str] = &["z-index", "order", "column-count", "orphans", "widows", "-webkit-line-clamp", "counter-increment", "counter-reset", "counter-set"];

/// Properties whose values are never negative (a timing function may overshoot).
const NOT_NEGATIVE: &[&str] = &[
    "width", "height", "min-width", "min-height", "max-width", "max-height", "padding-top", "padding-right", "padding-bottom",
    "padding-left", "border-top-width", "border-right-width", "border-bottom-width", "border-left-width", "outline-width",
    "font-size", "line-height", "flex-grow", "flex-shrink", "column-gap", "row-gap", "column-width", "column-rule-width",
    "border-top-left-radius", "border-top-right-radius", "border-bottom-left-radius", "border-bottom-right-radius",
    "perspective", "stroke-width", "letter-spacing-never", "block-size", "inline-size", "font-weight",
];

/// The value at progress `p` between `a` (0) and `b` (1) for property `name`.
pub fn interpolieren(name: &str, a: &str, b: &str, p: f64) -> Option<String> {
    let (a, b) = (a.trim(), b.trim());
    if !p.is_finite() {
        return None;
    }
    if name == "visibility" {
        return Some(visibility(a, b, p));
    }
    if a == b {
        return Some(a.to_string());
    }
    let discrete = || Some(if p < 0.5 { a.to_string() } else { b.to_string() });
    if matches!(name, "display" | "content" | "font-family" | "position" | "float" | "overflow" | "overflow-x" | "overflow-y") {
        return discrete();
    }
    if (name == "transform" || name.ends_with("-transform")) && let Some(t) = transform(a, b, p) {
        return Some(t);
    }
    // Release 15.10.2026 (Agent F): `none` and a shorter filter list take
    // the initial values of the other list's functions (Filter Effects 1
    // §6.1), then the lists interpolate function by function.
    let padded;
    let (a, b) = if matches!(name, "filter" | "backdrop-filter")
        && let Some(pair) = pad_filter_lists(a, b)
    {
        padded = pair;
        (padded.0.as_str(), padded.1.as_str())
    } else {
        (a, b)
    };
    let ka = syntax::komponenten(a);
    let kb = syntax::komponenten(b);
    let mut opts = Opts { integer: INTEGER.contains(&name), not_negative: NOT_NEGATIVE.contains(&name), clamp01: matches!(name, "opacity" | "fill-opacity" | "stroke-opacity" | "flood-opacity" | "stop-opacity" | "shape-image-threshold"), sum: Combine::Lerp };
    if name == "font-weight" {
        opts.not_negative = true;
    }
    match liste(syntax::getrimmt(&ka), syntax::getrimmt(&kb), p, &opts) {
        Some(text) => Some(text),
        None => discrete(),
    }
}

/// The filter functions of a list (name, whole function text).
fn filter_functions(v: &str) -> Option<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut rest = v.trim();
    if rest.eq_ignore_ascii_case("none") {
        return Some(out);
    }
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = rest[..open].trim().to_ascii_lowercase();
        let mut depth = 0;
        let mut close = None;
        for (i, c) in rest[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close?;
        let mut text = rest[..=close].trim().to_string();
        // Amounts as numbers: `grayscale(100%)` interpolates with
        // `grayscale(0)`.
        if matches!(name.as_str(), "brightness" | "contrast" | "grayscale" | "invert" | "opacity" | "saturate" | "sepia") {
            let arg = rest[open + 1..close].trim();
            if let Some(p) = arg.strip_suffix('%').and_then(|z| z.trim().parse::<f64>().ok()) {
                text = format!("{name}({})", zahl_text(p / 100.0));
            }
        }
        out.push((name, text));
        rest = rest[close + 1..].trim_start();
    }
    Some(out)
}

/// The initial value of a filter function for interpolation (Filter
/// Effects 1 §6.1); `None` for `url()`.
fn filter_initial_value(name: &str) -> Option<&'static str> {
    Some(match name {
        "blur" => "blur(0px)",
        "brightness" => "brightness(1)",
        "contrast" => "contrast(1)",
        "drop-shadow" => "drop-shadow(rgba(0, 0, 0, 0) 0px 0px 0px)",
        "grayscale" => "grayscale(0)",
        "hue-rotate" => "hue-rotate(0deg)",
        "invert" => "invert(0)",
        "opacity" => "opacity(1)",
        "saturate" => "saturate(1)",
        "sepia" => "sepia(0)",
        _ => return None,
    })
}

/// Pads `a` and `b` to function lists of the same shape: the shorter list
/// (or `none`) gets the initial values of the other's remaining functions.
/// `None` when the lists do not match function by function.
fn pad_filter_lists(a: &str, b: &str) -> Option<(String, String)> {
    let (fa, fb) = (filter_functions(a)?, filter_functions(b)?);
    let (short, long, a_short) = if fa.len() < fb.len() { (&fa, &fb, true) } else { (&fb, &fa, false) };
    if short.iter().zip(long.iter()).any(|(x, y)| x.0 != y.0) {
        return None;
    }
    let mut padded: Vec<String> = short.iter().map(|f| f.1.clone()).collect();
    for f in &long[short.len()..] {
        padded.push(filter_initial_value(&f.0)?.to_string());
    }
    let long_text = long.iter().map(|f| f.1.clone()).collect::<Vec<_>>().join(" ");
    let padded = padded.join(" ");
    Some(if a_short { (padded, long_text) } else { (long_text, padded) })
}

/// Release 15.10.2026: can `a` and `b` interpolate smoothly (not only
/// discretely)? A CSS transition starts only then (CSS Transitions 1 §3;
/// `transition-behavior: allow-discrete` aside).
pub fn interpolierbar(name: &str, a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    if a == b || name == "visibility" {
        return true;
    }
    if matches!(name, "display" | "content" | "font-family" | "position" | "float" | "overflow" | "overflow-x" | "overflow-y") {
        return false;
    }
    if (name == "transform" || name.ends_with("-transform")) && transform(a, b, 0.5).is_some() {
        return true;
    }
    let padded;
    let (a, b) = if matches!(name, "filter" | "backdrop-filter")
        && let Some(pair) = pad_filter_lists(a, b)
    {
        padded = pair;
        (padded.0.as_str(), padded.1.as_str())
    } else {
        (a, b)
    };
    let ka = syntax::komponenten(a);
    let kb = syntax::komponenten(b);
    let opts = Opts { integer: false, not_negative: false, clamp01: false, sum: Combine::Lerp };
    liste(syntax::getrimmt(&ka), syntax::getrimmt(&kb), 0.5, &opts).is_some()
}

struct Opts {
    integer: bool,
    not_negative: bool,
    clamp01: bool,
    /// Release 15.10.2026: add instead of interpolating (`kombinieren`).
    sum: Combine,
}

/// How two numbers combine.
#[derive(Clone, Copy, PartialEq)]
enum Combine {
    /// `a + (b - a) * p`.
    Lerp,
    /// `a + b` (composite `add`/`accumulate`).
    Add,
    /// `a + b - 1` (accumulating scale factors, CSS Transforms 2 §16).
    AddScale,
    /// `a * b` (adding scale factors).
    Multiply,
}

impl Opts {
    fn lerp_only(&self) -> bool {
        self.sum == Combine::Lerp
    }

    fn mix(&self, a: f64, b: f64, p: f64) -> f64 {
        match self.sum {
            Combine::Lerp => lerp(a, b, p),
            Combine::Add => a + b,
            Combine::AddScale => a + b - 1.0,
            Combine::Multiply => a * b,
        }
    }
}

/// Release 15.10.2026: `a` combined with `b` for the composite operations
/// `add` and `accumulate` (CSS Values 4 §3.1 "value addition", Web Animations
/// 1 §5.4.4). Values that cannot be added are replaced by `b`.
pub fn kombinieren(name: &str, a: &str, b: &str, akkumulieren: bool) -> Option<String> {
    let (a, b) = (a.trim(), b.trim());
    let transformish = name == "transform" || name.ends_with("-transform");
    let base = Opts { integer: false, not_negative: false, clamp01: false, sum: Combine::Add };
    if transformish {
        if a == "none" {
            return Some(b.to_string());
        }
        if b == "none" {
            return Some(a.to_string());
        }
        if !akkumulieren {
            return Some(format!("{a} {b}"));
        }
        let ka = syntax::komponenten(a);
        let kb = syntax::komponenten(b);
        return liste(syntax::getrimmt(&ka), syntax::getrimmt(&kb), 0.0, &base).or_else(|| Some(format!("{a} {b}")));
    }
    if matches!(name, "box-shadow" | "text-shadow" | "filter" | "backdrop-filter") && !akkumulieren {
        if a == "none" {
            return Some(b.to_string());
        }
        if b == "none" {
            return Some(a.to_string());
        }
        let sep = if name.ends_with("shadow") { ", " } else { " " };
        return Some(format!("{a}{sep}{b}"));
    }
    if name == "visibility" || matches!(name, "display" | "content" | "font-family" | "position" | "float" | "overflow" | "overflow-x" | "overflow-y") {
        return Some(b.to_string());
    }
    let sum = if name == "scale" { if akkumulieren { Combine::AddScale } else { Combine::Multiply } } else { Combine::Add };
    let opts = Opts {
        integer: INTEGER.contains(&name),
        not_negative: NOT_NEGATIVE.contains(&name),
        clamp01: matches!(name, "opacity" | "fill-opacity" | "stroke-opacity" | "flood-opacity" | "stop-opacity" | "shape-image-threshold"),
        sum,
    };
    let ka = syntax::komponenten(a);
    let kb = syntax::komponenten(b);
    liste(syntax::getrimmt(&ka), syntax::getrimmt(&kb), 0.0, &opts).or_else(|| Some(b.to_string()))
}

fn visibility(a: &str, b: &str, p: f64) -> String {
    // CSS Values 4: `visible` wins while one end is visible.
    if p > 0.0 && p < 1.0 && (a == "visible" || b == "visible") {
        return String::from("visible");
    }
    if p <= 0.0 { a.to_string() } else if p >= 1.0 { b.to_string() } else if p < 0.5 { a.to_string() } else { b.to_string() }
}

fn lerp(a: f64, b: f64, p: f64) -> f64 {
    a + (b - a) * p
}

fn zahl(v: f64, o: &Opts) -> f64 {
    let mut v = v;
    if o.integer {
        v = (v + 0.5).floor();
    }
    if o.clamp01 {
        v = v.clamp(0.0, 1.0);
    }
    if o.not_negative && v < 0.0 {
        v = 0.0;
    }
    v
}

/// A color component value, if it is one.
fn ist_farbe(k: &Kv) -> Option<([f64; 3], f64)> {
    match k {
        Kv::T(Tok::Ident(n)) if !n.eq_ignore_ascii_case("currentcolor") => farbe_rgba(k).filter(|_| crate::farben::name_suchen(&n.to_ascii_lowercase()).is_some() || n.eq_ignore_ascii_case("transparent")),
        Kv::T(Tok::Hash(..)) => farbe_rgba(k),
        Kv::F(n, _) if matches!(n.to_ascii_lowercase().as_str(), "rgb" | "rgba" | "hsl" | "hsla" | "hwb" | "color") => farbe_rgba(k),
        _ => None,
    }
}

fn farbe_rgba(k: &Kv) -> Option<([f64; 3], f64)> {
    let f = farbe::lesen(k)?;
    let text = f.berechnet(None);
    let kv = syntax::komponenten(&text);
    let [Kv::F(n, args)] = syntax::getrimmt(&kv) else { return None };
    let zahlen: Vec<f64> = args
        .iter()
        .filter_map(|x| match x {
            Kv::T(Tok::Zahl(w, _, _)) => Some(*w),
            _ => None,
        })
        .collect();
    match n.to_ascii_lowercase().as_str() {
        "rgb" if zahlen.len() == 3 => Some(([zahlen[0], zahlen[1], zahlen[2]], 1.0)),
        "rgba" if zahlen.len() == 4 => Some(([zahlen[0], zahlen[1], zahlen[2]], zahlen[3])),
        "color" if args.iter().any(|x| x.ist_wort("srgb")) => {
            let a = if zahlen.len() >= 4 { zahlen[3] } else { 1.0 };
            (zahlen.len() >= 3).then(|| ([zahlen[0] * 255.0, zahlen[1] * 255.0, zahlen[2] * 255.0], a))
        }
        _ => None,
    }
}

/// Premultiplied sRGB interpolation (CSS Color 4 §12.3, legacy colors).
fn farbe_mischen(a: ([f64; 3], f64), b: ([f64; 3], f64), p: f64) -> String {
    let alpha = lerp(a.1, b.1, p).clamp(0.0, 1.0);
    let mut c = [0.0; 3];
    for i in 0..3 {
        let v = lerp(a.0[i] * a.1, b.0[i] * b.1, p);
        c[i] = if alpha > 0.0 { (v / alpha).clamp(0.0, 255.0) } else { 0.0 };
    }
    let r = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    if alpha >= 1.0 {
        format!("rgb({}, {}, {})", r(c[0]), r(c[1]), r(c[2]))
    } else {
        format!("rgba({}, {}, {}, {})", r(c[0]), r(c[1]), r(c[2]), zahl_text((alpha * 1000.0).round() / 1000.0))
    }
}

fn ohne_leer(kv: &[Kv]) -> Vec<&Kv> {
    kv.iter().filter(|k| !k.ist_leer()).collect()
}

/// Two component lists of the same shape, interpolated; `None` = not.
fn liste(a: &[Kv], b: &[Kv], p: f64, o: &Opts) -> Option<String> {
    let ta = ohne_leer(a);
    let tb = ohne_leer(b);
    if ta.len() != tb.len() || ta.is_empty() {
        return None;
    }
    let mut teile: Vec<String> = Vec::new();
    for (x, y) in ta.iter().zip(&tb) {
        let t = einzeln(x, y, p, o)?;
        match teile.last_mut() {
            Some(l) if t == "," => l.push(','),
            _ => teile.push(t),
        }
    }
    Some(teile.join(" ").replace(" ,", ","))
}

fn einzeln(x: &Kv, y: &Kv, p: f64, o: &Opts) -> Option<String> {
    if let (Some(fa), Some(fb)) = (ist_farbe(x), ist_farbe(y)) {
        if !o.lerp_only() {
            return Some(farbe_addieren(fa, fb));
        }
        return Some(farbe_mischen(fa, fb, p));
    }
    match (x, y) {
        (Kv::T(Tok::Zahl(a, _, _)), Kv::T(Tok::Zahl(b, _, _))) => Some(zahl_text(zahl(o.mix(*a, *b, p), o))),
        (Kv::T(Tok::Prozent(a, _)), Kv::T(Tok::Prozent(b, _))) => Some(format!("{}%", zahl_text(zahl(o.mix(*a, *b, p), o)))),
        (Kv::T(Tok::Dim(a, _, ea, _)), Kv::T(Tok::Dim(b, _, eb, _))) if ea.eq_ignore_ascii_case(eb) => {
            Some(format!("{}{}", zahl_text(zahl(o.mix(*a, *b, p), o)), ea.to_ascii_lowercase()))
        }
        // A unitless zero next to a dimension (`0` and `10px`).
        (Kv::T(Tok::Zahl(a, _, _)), Kv::T(Tok::Dim(b, _, e, _))) if *a == 0.0 => Some(format!("{}{}", zahl_text(zahl(o.mix(0.0, *b, p), o)), e)),
        (Kv::T(Tok::Dim(a, _, e, _)), Kv::T(Tok::Zahl(b, _, _))) if *b == 0.0 => Some(format!("{}{}", zahl_text(zahl(o.mix(*a, 0.0, p), o)), e)),
        // A length and a percentage: `calc()` of both (CSS Values 4 §10.9).
        (Kv::T(Tok::Dim(a, _, e, _)), Kv::T(Tok::Prozent(b, _))) if e.eq_ignore_ascii_case("px") => Some(calc_mix(0.0, *a, *b, 0.0, p, o)),
        (Kv::T(Tok::Prozent(a, _)), Kv::T(Tok::Dim(b, _, e, _))) if e.eq_ignore_ascii_case("px") => Some(calc_mix(*a, 0.0, 0.0, *b, p, o)),
        (Kv::F(na, aa), Kv::F(nb, ab)) if na.eq_ignore_ascii_case(nb) => {
            // Accumulated scale functions add their factors minus one.
            let scale = Opts { integer: o.integer, not_negative: o.not_negative, clamp01: o.clamp01, sum: Combine::AddScale };
            let innen = if o.sum == Combine::Add && na.to_ascii_lowercase().starts_with("scale") { liste(aa, ab, p, &scale)? } else { liste(aa, ab, p, o)? };
            Some(format!("{}({innen})", na.to_ascii_lowercase()))
        }
        (Kv::T(Tok::Komma), Kv::T(Tok::Komma)) => Some(String::from(",")),
        (Kv::T(Tok::Delim(c)), Kv::T(Tok::Delim(d))) if c == d => Some(c.to_string()),
        (Kv::T(Tok::Ident(a)), Kv::T(Tok::Ident(b))) if a.eq_ignore_ascii_case(b) => Some(a.clone()),
        (Kv::T(Tok::Str(a)), Kv::T(Tok::Str(b))) if a == b => Some(syntax::kv_text(std::slice::from_ref(x))),
        _ => None,
    }
}

/// Premultiplied sRGB addition, clamped (CSS Color 4 §12.4).
fn farbe_addieren(a: ([f64; 3], f64), b: ([f64; 3], f64)) -> String {
    let alpha = (a.1 + b.1).clamp(0.0, 1.0);
    let mut c = [0.0; 3];
    for i in 0..3 {
        let v = a.0[i] * a.1 + b.0[i] * b.1;
        c[i] = if alpha > 0.0 { (v / alpha).clamp(0.0, 255.0) } else { 0.0 };
    }
    let r = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    if alpha >= 1.0 {
        format!("rgb({}, {}, {})", r(c[0]), r(c[1]), r(c[2]))
    } else {
        format!("rgba({}, {}, {}, {})", r(c[0]), r(c[1]), r(c[2]), zahl_text((alpha * 1000.0).round() / 1000.0))
    }
}

/// `calc(P% + Lpx)` between (percent a, px a) and (percent b, px b).
fn calc_mix(pa: f64, la: f64, pb: f64, lb: f64, p: f64, o: &Opts) -> String {
    let pc = if o.lerp_only() { lerp(pa, pb, p) } else { pa + pb };
    let px = if o.lerp_only() { lerp(la, lb, p) } else { la + lb };
    if px == 0.0 {
        return format!("{}%", zahl_text(pc));
    }
    if pc == 0.0 {
        return format!("{}px", zahl_text(px));
    }
    if px < 0.0 {
        format!("calc({}% - {}px)", zahl_text(pc), zahl_text(-px))
    } else {
        format!("calc({}% + {}px)", zahl_text(pc), zahl_text(px))
    }
}

// --- transforms ---------------------------------------------------------

/// A 2D matrix `[a, b, c, d, e, f]` of a computed transform (`none`,
/// `matrix(…)` or a list of 2D functions in px/deg).
fn matrix_von(t: &str) -> Option<[f64; 6]> {
    let t = t.trim();
    if t == "none" {
        return Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }
    let kv = syntax::komponenten(t);
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for f in ohne_leer(&kv) {
        let Kv::F(name, args) = f else { return None };
        let werte: Vec<(f64, String)> = args
            .iter()
            .filter_map(|x| match x {
                Kv::T(Tok::Zahl(w, _, _)) => Some((*w, String::new())),
                Kv::T(Tok::Dim(w, _, e, _)) => Some((*w, e.to_ascii_lowercase())),
                _ => None,
            })
            .collect();
        let winkel = |i: usize| -> Option<f64> {
            let (w, e) = werte.get(i)?;
            Some(match e.as_str() {
                "deg" => w.to_radians(),
                "rad" | "" => *w,
                "grad" => w * std::f64::consts::PI / 200.0,
                "turn" => w * 2.0 * std::f64::consts::PI,
                _ => return None,
            })
        };
        let px = |i: usize| -> Option<f64> {
            match werte.get(i) {
                Some((w, e)) if e == "px" || (e.is_empty() && *w == 0.0) => Some(*w),
                None => Some(0.0),
                _ => None,
            }
        };
        let n = match name.to_ascii_lowercase().as_str() {
            "matrix" if werte.len() == 6 => [werte[0].0, werte[1].0, werte[2].0, werte[3].0, werte[4].0, werte[5].0],
            "translate" => [1.0, 0.0, 0.0, 1.0, px(0)?, px(1)?],
            "translatex" => [1.0, 0.0, 0.0, 1.0, px(0)?, 0.0],
            "translatey" => [1.0, 0.0, 0.0, 1.0, 0.0, px(0)?],
            "scale" => {
                let sx = werte.first()?.0;
                let sy = werte.get(1).map_or(sx, |v| v.0);
                [sx, 0.0, 0.0, sy, 0.0, 0.0]
            }
            "scalex" => [werte.first()?.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            "scaley" => [1.0, 0.0, 0.0, werte.first()?.0, 0.0, 0.0],
            "rotate" | "rotatez" => {
                let w = winkel(0)?;
                [w.cos(), w.sin(), -w.sin(), w.cos(), 0.0, 0.0]
            }
            "skew" => [1.0, winkel(1).unwrap_or(0.0).tan(), winkel(0)?.tan(), 1.0, 0.0, 0.0],
            "skewx" => [1.0, 0.0, winkel(0)?.tan(), 1.0, 0.0, 0.0],
            "skewy" => [1.0, winkel(0)?.tan(), 0.0, 1.0, 0.0, 0.0],
            _ => return None,
        };
        m = mal(m, n);
    }
    Some(m)
}

fn mal(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

/// 2D decomposition (CSS Transforms 1 §10.2.4 "unmatrix").
fn zerlegen(m: [f64; 6]) -> Option<[f64; 6]> {
    let (mut a, mut b, mut c, mut d) = (m[0], m[1], m[2], m[3]);
    let (tx, ty) = (m[4], m[5]);
    let mut sx = (a * a + b * b).sqrt();
    if sx == 0.0 {
        return None;
    }
    a /= sx;
    b /= sx;
    let mut shear = a * c + b * d;
    c -= a * shear;
    d -= b * shear;
    let sy = (c * c + d * d).sqrt();
    if sy == 0.0 {
        return None;
    }
    c /= sy;
    d /= sy;
    shear /= sy;
    if a * d < b * c {
        a = -a;
        b = -b;
        shear = -shear;
        sx = -sx;
    }
    let _ = (c, d);
    let winkel = b.atan2(a);
    Some([tx, ty, sx, sy, winkel, shear])
}

fn zusammensetzen(z: [f64; 6]) -> [f64; 6] {
    let [tx, ty, sx, sy, winkel, shear] = z;
    let (s, c) = winkel.sin_cos();
    let rot = [c, s, -s, c, 0.0, 0.0];
    let mut m = mal([1.0, 0.0, 0.0, 1.0, tx, ty], rot);
    m = mal(m, [1.0, 0.0, shear, 1.0, 0.0, 0.0]);
    mal(m, [sx, 0.0, 0.0, sy, 0.0, 0.0])
}

fn matrix_text(m: [f64; 6]) -> String {
    let r = |v: f64| zahl_text((v * 1e6).round() / 1e6);
    format!("matrix({}, {}, {}, {}, {}, {})", r(m[0]), r(m[1]), r(m[2]), r(m[3]), r(m[4]), r(m[5]))
}

/// Transforms: the same function list interpolates function by function
/// (`liste`); otherwise both as 2D matrices, decomposed.
fn transform(a: &str, b: &str, p: f64) -> Option<String> {
    let o = Opts { integer: false, not_negative: false, clamp01: false, sum: Combine::Lerp };
    let ka = syntax::komponenten(a);
    let kb = syntax::komponenten(b);
    // `none` against a function list: the identity functions of the other.
    if a == "none" || b == "none" {
        let (andere, rueck) = if a == "none" { (&kb, false) } else { (&ka, true) };
        if let Some(id) = identitaet(andere) {
            let idkv = syntax::komponenten(&id);
            let r = if rueck { liste(syntax::getrimmt(&ka), &idkv, p, &o) } else { liste(&idkv, syntax::getrimmt(&kb), p, &o) };
            if r.is_some() {
                return r;
            }
        }
    }
    if let Some(r) = liste(syntax::getrimmt(&ka), syntax::getrimmt(&kb), p, &o) {
        return Some(r);
    }
    let (ma, mb) = (matrix_von(a)?, matrix_von(b)?);
    let (za, zb) = (zerlegen(ma)?, zerlegen(mb)?);
    let mut z = [0.0; 6];
    let mut wa = za[4];
    let wb = zb[4];
    // The shorter way round.
    if (wa - wb).abs() > std::f64::consts::PI {
        if wa > wb {
            wa -= 2.0 * std::f64::consts::PI;
        } else {
            wa += 2.0 * std::f64::consts::PI;
        }
    }
    for i in 0..6 {
        z[i] = lerp(if i == 4 { wa } else { za[i] }, zb[i], p);
    }
    Some(matrix_text(zusammensetzen(z)))
}

/// The identity transform functions for a list (`translate(0px)` for
/// `translate(…)`, `scale(1)` …).
fn identitaet(kv: &[Kv]) -> Option<String> {
    let mut raus = Vec::new();
    for f in ohne_leer(kv) {
        let Kv::F(name, args) = f else { return None };
        let n = name.to_ascii_lowercase();
        let teile = ohne_leer(args).iter().filter(|k| !k.ist_komma()).count();
        let null = |e: &str| (0..teile.max(1)).map(|_| format!("0{e}")).collect::<Vec<_>>().join(", ");
        raus.push(match n.as_str() {
            "translate" | "translatex" | "translatey" | "translatez" | "translate3d" => format!("{n}({})", null("px")),
            "scale" | "scalex" | "scaley" | "scalez" | "scale3d" => format!("{n}({})", (0..teile.max(1)).map(|_| "1").collect::<Vec<_>>().join(", ")),
            "rotate" | "rotatex" | "rotatey" | "rotatez" | "skew" | "skewx" | "skewy" => format!("{n}({})", null("deg")),
            _ => return None,
        });
    }
    Some(raus.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addition() {
        assert_eq!(kombinieren("width", "10px", "20px", false).as_deref(), Some("30px"));
        assert_eq!(kombinieren("opacity", "0.5", "0.75", false).as_deref(), Some("1"));
        assert_eq!(kombinieren("transform", "translateX(10px)", "rotate(45deg)", false).as_deref(), Some("translateX(10px) rotate(45deg)"));
        assert_eq!(kombinieren("transform", "scale(2)", "scale(3)", true).as_deref(), Some("scale(4)"));
        assert_eq!(kombinieren("color", "rgb(10, 20, 30)", "rgb(100, 200, 250)", false).as_deref(), Some("rgb(110, 220, 255)"));
        assert_eq!(kombinieren("display", "block", "none", false).as_deref(), Some("none"));
    }

    #[test]
    fn grundformen() {
        assert_eq!(interpolieren("opacity", "0", "1", 0.25).as_deref(), Some("0.25"));
        assert_eq!(interpolieren("left", "0px", "100px", 0.5).as_deref(), Some("50px"));
        assert_eq!(interpolieren("color", "rgb(0, 0, 0)", "rgb(255, 255, 255)", 0.5).as_deref(), Some("rgb(128, 128, 128)"));
        assert_eq!(interpolieren("z-index", "1", "2", 0.5).as_deref(), Some("2"));
        assert_eq!(interpolieren("display", "block", "none", 0.4).as_deref(), Some("block"));
        assert_eq!(interpolieren("visibility", "hidden", "visible", 0.1).as_deref(), Some("visible"));
        assert_eq!(interpolieren("transform", "translate(0px)", "translate(100px)", 0.5).as_deref(), Some("translate(50px)"));
        assert_eq!(interpolieren("transform", "none", "rotate(90deg)", 0.5).as_deref(), Some("rotate(45deg)"));
        assert_eq!(interpolieren("width", "10%", "20px", 0.5).as_deref(), Some("calc(5% + 10px)"));
        assert_eq!(interpolieren("box-shadow", "rgb(0, 0, 0) 0px 0px 0px 0px", "rgb(255, 0, 0) 10px 10px 4px 0px", 0.5).as_deref(), Some("rgb(128, 0, 0) 5px 5px 2px 0px"));
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;

    #[test]
    fn filter_lists_pad_with_initial_values() {
        assert_eq!(interpolieren("filter", "blur(20px)", "none", 0.5).as_deref(), Some("blur(10px)"));
        assert_eq!(interpolieren("filter", "none", "grayscale(1)", 0.25).as_deref(), Some("grayscale(0.25)"));
    }

    #[test]
    fn filter_amount_percentages_interpolate_as_numbers() {
        assert_eq!(interpolieren("filter", "grayscale(100%)", "none", 0.5).as_deref(), Some("grayscale(0.5)"));
    }
}
