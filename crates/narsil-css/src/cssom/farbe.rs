//! Release 12.10.2026: Farben lesen und schreiben (CSS Color 4 §4–§15).
//!
//! Angegebener Wert: benannte Farben und Systemfarben bleiben das (klein
//! geschriebene) Wort, `#hex`/`rgb()`/`hsl()`/`hwb()` werden `rgb()`/`rgba()`,
//! `lab()`/`lch()`/`oklab()`/`oklch()`/`color()` behalten ihre Form mit Zahlen
//! (Prozente umgerechnet). Berechneter Wert: alles Legacy-sRGB wird
//! `rgb()`/`rgba()`. GEMESSEN vorher (WPT cssom/style-color*): `#00FF00` blieb
//! roh, `rgb(0%, 100%, 100%)` blieb Prozent, `CanvasText` blieb gross.

use super::mathe::{self, Ziel};
use super::syntax::{Kv, Tok, getrimmt, ident_text, kv_text};
use super::zahl::zahl_text;

/// Systemfarben (CSS Color 4 §6.2, mit den veralteten aus Anhang A).
const SYSTEM: &[(&str, (u8, u8, u8))] = &[
    ("accentcolor", (0, 117, 255)),
    ("accentcolortext", (255, 255, 255)),
    ("activetext", (255, 0, 0)),
    ("buttonborder", (118, 118, 118)),
    ("buttonface", (239, 239, 239)),
    ("buttontext", (0, 0, 0)),
    ("canvas", (255, 255, 255)),
    ("canvastext", (0, 0, 0)),
    ("field", (255, 255, 255)),
    ("fieldtext", (0, 0, 0)),
    ("graytext", (109, 109, 109)),
    ("highlight", (181, 213, 255)),
    ("highlighttext", (0, 0, 0)),
    ("linktext", (0, 0, 238)),
    ("mark", (255, 255, 0)),
    ("marktext", (0, 0, 0)),
    ("selecteditem", (0, 117, 255)),
    ("selecteditemtext", (255, 255, 255)),
    ("visitedtext", (85, 26, 139)),
    ("activeborder", (118, 118, 118)),
    ("activecaption", (255, 255, 255)),
    ("appworkspace", (255, 255, 255)),
    ("background", (255, 255, 255)),
    ("buttonhighlight", (239, 239, 239)),
    ("buttonshadow", (239, 239, 239)),
    ("captiontext", (0, 0, 0)),
    ("inactiveborder", (118, 118, 118)),
    ("inactivecaption", (255, 255, 255)),
    ("inactivecaptiontext", (128, 128, 128)),
    ("infobackground", (255, 255, 255)),
    ("infotext", (0, 0, 0)),
    ("menu", (255, 255, 255)),
    ("menutext", (0, 0, 0)),
    ("scrollbar", (255, 255, 255)),
    ("threeddarkshadow", (118, 118, 118)),
    ("threedface", (239, 239, 239)),
    ("threedhighlight", (118, 118, 118)),
    ("threedlightshadow", (118, 118, 118)),
    ("threedshadow", (118, 118, 118)),
    ("window", (255, 255, 255)),
    ("windowframe", (118, 118, 118)),
    ("windowtext", (0, 0, 0)),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Farbe {
    /// Benannte Farbe oder Systemfarbe (klein) mit ihrem sRGB-Wert.
    Name(String, (u8, u8, u8)),
    /// Legacy-sRGB (aus hex/rgb/hsl/hwb): 0..255 je Kanal, Alpha 0..1;
    /// `true` = Alpha kam als 8-Bit-Wert (hex).
    Rgb([f64; 3], f64, bool),
    Aktuell,
    Transparent,
    /// Eine Farbe mit eigener Form (`lab(…)`, `color(srgb …)`, `color-mix(…)`, …),
    /// schon kanonisch geschrieben; die zweite Form ist der berechnete Wert.
    Form(String, String),
}

fn byte(x: f64) -> u8 {
    if x.is_nan() {
        return 0;
    }
    x.round().clamp(0.0, 255.0) as u8
}

/// Alpha schreiben; aus 8 Bit: die kuerzeste Zahl, die dasselbe Byte ergibt.
fn alpha_text(a: f64, acht_bit: bool) -> String {
    if acht_bit {
        let b = (a * 255.0).round();
        for d in [2, 3, 4] {
            let f = 10f64.powi(d);
            let x = (b / 255.0 * f).round() / f;
            if (x * 255.0).round() == b {
                return zahl_text(x);
            }
        }
    }
    zahl_text(a)
}

impl Farbe {
    /// Der angegebene Wert als Text.
    pub fn text(&self) -> String {
        match self {
            Farbe::Name(n, _) => n.clone(),
            Farbe::Rgb(c, a, acht) => rgb_text(c, *a, *acht),
            Farbe::Aktuell => String::from("currentcolor"),
            Farbe::Transparent => String::from("transparent"),
            Farbe::Form(t, _) => t.clone(),
        }
    }
    /// Der berechnete Wert; `aktuell` ist die Farbe fuer `currentcolor`.
    pub fn berechnet(&self, aktuell: Option<&str>) -> String {
        match self {
            Farbe::Name(_, (r, g, b)) => format!("rgb({r}, {g}, {b})"),
            Farbe::Rgb(c, a, acht) => rgb_text(c, *a, *acht),
            Farbe::Aktuell => aktuell.map_or_else(|| String::from("currentcolor"), str::to_string),
            Farbe::Transparent => String::from("rgba(0, 0, 0, 0)"),
            Farbe::Form(_, b) => b.clone(),
        }
    }
}

fn rgb_text(c: &[f64; 3], a: f64, acht: bool) -> String {
    let (r, g, b) = (byte(c[0]), byte(c[1]), byte(c[2]));
    let a = if a.is_nan() { 0.0 } else { a.clamp(0.0, 1.0) };
    if a >= 1.0 {
        format!("rgb({r}, {g}, {b})")
    } else {
        format!("rgba({r}, {g}, {b}, {})", alpha_text(a, acht))
    }
}

fn hex(s: &str) -> Option<Farbe> {
    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v: Vec<u32> = s.chars().filter_map(|c| c.to_digit(16)).collect();
    let (r, g, b, a) = match v.len() {
        3 => (v[0] * 17, v[1] * 17, v[2] * 17, 255),
        4 => (v[0] * 17, v[1] * 17, v[2] * 17, v[3] * 17),
        6 => (v[0] * 16 + v[1], v[2] * 16 + v[3], v[4] * 16 + v[5], 255),
        8 => (v[0] * 16 + v[1], v[2] * 16 + v[3], v[4] * 16 + v[5], v[6] * 16 + v[7]),
        _ => return None,
    };
    Some(Farbe::Rgb([r as f64, g as f64, b as f64], a as f64 / 255.0, true))
}

/// Ein Komponentenwert: Zahl, Prozent, Winkel (Grad) oder `none`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Komp {
    Zahl(f64),
    Prozent(f64),
    Grad(f64),
    Keins,
}

fn komp(kv: &Kv) -> Option<Komp> {
    match kv {
        Kv::T(Tok::Zahl(w, _, _)) => Some(Komp::Zahl(*w)),
        Kv::T(Tok::Prozent(w, _)) => Some(Komp::Prozent(*w)),
        Kv::T(Tok::Dim(w, _, e, _)) => {
            let (b, _, f) = mathe::einheit_info(&e.to_ascii_lowercase())?;
            (b == mathe::WINKEL).then_some(Komp::Grad(w * f))
        }
        Kv::T(Tok::Ident(n)) if n.eq_ignore_ascii_case("none") => Some(Komp::Keins),
        Kv::F(n, a) if mathe::ist_mathe(n) => {
            for z in [Ziel::Zahl, Ziel::Prozent, Ziel::Winkel] {
                if let Some(m) = mathe::lesen(n, a, z) {
                    let (w, e) = m.zahlwert()?;
                    return Some(match e {
                        "" => Komp::Zahl(w),
                        "%" => Komp::Prozent(w),
                        _ => Komp::Grad(w),
                    });
                }
            }
            None
        }
        _ => None,
    }
}

fn hue(k: Komp) -> Option<f64> {
    match k {
        Komp::Zahl(w) | Komp::Grad(w) => Some(w),
        Komp::Keins => Some(0.0),
        Komp::Prozent(_) => None,
    }
}

fn hsl_rgb(h: f64, s: f64, l: f64) -> [f64; 3] {
    let h = h.rem_euclid(360.0);
    let (s, l) = (s.clamp(0.0, 100.0) / 100.0, l.clamp(0.0, 100.0) / 100.0);
    let f = |n: f64| {
        let k = (n + h / 30.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    [f(0.0) * 255.0, f(8.0) * 255.0, f(4.0) * 255.0]
}

fn hwb_rgb(h: f64, w: f64, b: f64) -> [f64; 3] {
    let (w, b) = (w.clamp(0.0, 100.0) / 100.0, b.clamp(0.0, 100.0) / 100.0);
    if w + b >= 1.0 {
        let g = w / (w + b) * 255.0;
        return [g, g, g];
    }
    let rgb = hsl_rgb(h, 100.0, 50.0);
    let f = 1.0 - w - b;
    let w = w * 255.0;
    [rgb[0] * f + w, rgb[1] * f + w, rgb[2] * f + w]
}

/// Die Argumente einer Farbfunktion: Komponenten, Alpha und ob die alte
/// Kommaschreibweise benutzt wurde.
fn argumente(args: &[Kv]) -> Option<(Vec<Komp>, Option<Komp>, bool)> {
    let teile: Vec<&Kv> = args.iter().filter(|k| !k.ist_leer()).collect();
    let komma = teile.iter().any(|k| k.ist_komma());
    let mut komps = Vec::new();
    let mut alpha = None;
    if komma {
        for (i, k) in teile.iter().enumerate() {
            if i % 2 == 1 {
                if !k.ist_komma() {
                    return None;
                }
                continue;
            }
            let c = komp(k)?;
            if c == Komp::Keins {
                return None;
            }
            komps.push(c);
        }
        if teile.len() % 2 == 0 {
            return None;
        }
        if komps.len() == 4 {
            alpha = komps.pop();
        }
        if komps.len() != 3 {
            return None;
        }
        return Some((komps, alpha, true));
    }
    let mut nach_strich = false;
    for k in teile {
        if k.ist_delim('/') {
            if nach_strich || komps.len() != 3 {
                return None;
            }
            nach_strich = true;
            continue;
        }
        let c = komp(k)?;
        if nach_strich {
            if alpha.is_some() {
                return None;
            }
            alpha = Some(c);
        } else {
            komps.push(c);
        }
    }
    if komps.len() != 3 || (nach_strich && alpha.is_none()) {
        return None;
    }
    Some((komps, alpha, false))
}

fn alpha_wert(a: Option<Komp>) -> Option<f64> {
    Some(match a {
        None => 1.0,
        Some(Komp::Zahl(w)) => w.clamp(0.0, 1.0),
        Some(Komp::Prozent(p)) => (p / 100.0).clamp(0.0, 1.0),
        Some(Komp::Keins) => 0.0,
        Some(Komp::Grad(_)) => return None,
    })
}

/// Komponente fuer lab/lch/oklab/oklch/color(): `faktor` fuer 100 %.
fn modern_text(k: Komp, faktor: f64) -> Option<String> {
    Some(match k {
        Komp::Zahl(w) => zahl_text(w),
        Komp::Prozent(p) => zahl_text(p / 100.0 * faktor),
        Komp::Keins => String::from("none"),
        Komp::Grad(_) => return None,
    })
}

fn hue_text(k: Komp) -> Option<String> {
    Some(match k {
        Komp::Zahl(w) | Komp::Grad(w) => zahl_text(w),
        Komp::Keins => String::from("none"),
        Komp::Prozent(_) => return None,
    })
}

fn alpha_modern(a: Option<Komp>) -> Option<String> {
    Some(match a {
        None => String::new(),
        Some(Komp::Keins) => String::from(" / none"),
        Some(k) => {
            let w = alpha_wert(Some(k))?;
            if w >= 1.0 { String::new() } else { format!(" / {}", zahl_text(w)) }
        }
    })
}

const RAEUME: &[&str] = &[
    "srgb", "srgb-linear", "display-p3", "display-p3-linear", "a98-rgb", "prophoto-rgb", "rec2020", "xyz", "xyz-d50", "xyz-d65",
];

/// Eine Farbe aus einem Komponentenwert lesen.
pub fn lesen(kv: &Kv) -> Option<Farbe> {
    match kv {
        Kv::T(Tok::Ident(n)) => {
            let n = n.to_ascii_lowercase();
            match n.as_str() {
                "currentcolor" => return Some(Farbe::Aktuell),
                "transparent" => return Some(Farbe::Transparent),
                _ => {}
            }
            if let Some((_, c)) = SYSTEM.iter().find(|(s, _)| *s == n) {
                return Some(Farbe::Name(n, *c));
            }
            let c = crate::farben::name_suchen(&n)?;
            Some(Farbe::Name(n, c))
        }
        Kv::T(Tok::Hash(h, _)) => hex(h),
        Kv::F(name, args) => funktion(name, args),
        _ => None,
    }
}

fn funktion(name: &str, args: &[Kv]) -> Option<Farbe> {
    let n = name.to_ascii_lowercase();
    // Relative Farben (`rgb(from red r g b)`) und Funktionen mit Teilen, die
    // erst spaeter feststehen (`sibling-index()`): kanonischer Text.
    let erstes = getrimmt(args).first();
    if erstes.is_some_and(|k| k.ist_wort("from")) {
        return relativ(&n, args);
    }
    match n.as_str() {
        "rgb" | "rgba" => {
            let Some((k, a, komma)) = argumente(args) else { return spaet(&n, args) };
            let alle_p = k.iter().all(|c| matches!(c, Komp::Prozent(_)));
            let alle_z = k.iter().all(|c| matches!(c, Komp::Zahl(_)));
            if komma && !(alle_p || alle_z) {
                return None;
            }
            let mut c = [0.0; 3];
            for (i, x) in k.iter().enumerate() {
                c[i] = match x {
                    Komp::Zahl(w) => *w,
                    Komp::Prozent(p) => p * 2.55,
                    Komp::Keins => 0.0,
                    Komp::Grad(_) => return None,
                };
            }
            Some(Farbe::Rgb(c, alpha_wert(a)?, false))
        }
        "hsl" | "hsla" => {
            let Some((k, a, komma)) = argumente(args) else { return spaet(&n, args) };
            if komma && !(matches!(k[1], Komp::Prozent(_)) && matches!(k[2], Komp::Prozent(_))) {
                return None;
            }
            let h = hue(k[0])?;
            let p = |c: Komp| match c {
                Komp::Zahl(w) | Komp::Prozent(w) => Some(w),
                Komp::Keins => Some(0.0),
                Komp::Grad(_) => None,
            };
            Some(Farbe::Rgb(hsl_rgb(h, p(k[1])?, p(k[2])?), alpha_wert(a)?, false))
        }
        "hwb" => {
            let Some((k, a, komma)) = argumente(args) else { return spaet(&n, args) };
            if komma {
                return None;
            }
            let h = hue(k[0])?;
            let p = |c: Komp| match c {
                Komp::Zahl(w) | Komp::Prozent(w) => Some(w),
                Komp::Keins => Some(0.0),
                Komp::Grad(_) => None,
            };
            Some(Farbe::Rgb(hwb_rgb(h, p(k[1])?, p(k[2])?), alpha_wert(a)?, false))
        }
        "lab" | "lch" | "oklab" | "oklch" => {
            let Some((k, a, komma)) = argumente(args) else { return spaet(&n, args) };
            if komma {
                return None;
            }
            let (fl, f2, polar) = match n.as_str() {
                "lab" => (100.0, 125.0, false),
                "lch" => (100.0, 150.0, true),
                "oklab" => (1.0, 0.4, false),
                _ => (1.0, 0.4, true),
            };
            // L wird auf [0, max] geklemmt, C auf >= 0 (§8, §9).
            let l = match k[0] {
                Komp::Zahl(w) => Komp::Zahl(w.clamp(0.0, fl)),
                Komp::Prozent(p) => Komp::Zahl((p / 100.0 * fl).clamp(0.0, fl)),
                x => x,
            };
            let t1 = modern_text(l, fl)?;
            let (t2, t3) = if polar {
                let c = match k[1] {
                    Komp::Zahl(w) => Komp::Zahl(w.max(0.0)),
                    Komp::Prozent(p) => Komp::Zahl((p / 100.0 * f2).max(0.0)),
                    x => x,
                };
                (modern_text(c, f2)?, hue_text(k[2])?)
            } else {
                (modern_text(k[1], f2)?, modern_text(k[2], f2)?)
            };
            let t = format!("{n}({t1} {t2} {t3}{})", alpha_modern(a)?);
            Some(Farbe::Form(t.clone(), t))
        }
        "color" => {
            let teile: Vec<&Kv> = args.iter().filter(|k| !k.ist_leer()).collect();
            let raum = teile.first()?.ident()?.to_ascii_lowercase();
            if !RAEUME.contains(&raum.as_str()) {
                return None;
            }
            let rest: Vec<Kv> = args
                .iter()
                .skip_while(|k| k.ist_leer())
                .skip(1)
                .cloned()
                .collect();
            let Some((k, a, komma)) = argumente(&rest) else { return spaet(&n, args) };
            if komma {
                return None;
            }
            let mut ts = Vec::new();
            for c in k {
                ts.push(modern_text(c, 1.0)?);
            }
            let raum = if raum == "xyz" { String::from("xyz-d65") } else { raum };
            let t = format!("color({raum} {}{})", ts.join(" "), alpha_modern(a)?);
            Some(Farbe::Form(t.clone(), t))
        }
        "color-mix" => farbmischung(args),
        "light-dark" => {
            let teile = teile_komma(args);
            if teile.len() != 2 {
                return None;
            }
            let a = einzel(teile[0])?;
            let b = einzel(teile[1])?;
            let t = format!("light-dark({}, {})", a.text(), b.text());
            Some(Farbe::Form(t, a.berechnet(None)))
        }
        _ => None,
    }
}

fn teile_komma(args: &[Kv]) -> Vec<&[Kv]> {
    let mut raus = Vec::new();
    let mut s = 0;
    for (i, k) in args.iter().enumerate() {
        if k.ist_komma() {
            raus.push(&args[s..i]);
            s = i + 1;
        }
    }
    raus.push(&args[s..]);
    raus
}

fn einzel(kv: &[Kv]) -> Option<Farbe> {
    match getrimmt(kv) {
        [k] => lesen(k),
        _ => None,
    }
}

/// Farbfunktion mit Teilen, die erst berechnet feststehen (`sibling-index()`,
/// `random()`): pruefen, dass die Form stimmt, dann als Text behalten.
fn spaet(n: &str, args: &[Kv]) -> Option<Farbe> {
    if !(super::syntax::enthaelt_funk(args, "sibling-index")
        || super::syntax::enthaelt_funk(args, "sibling-count")
        || super::syntax::enthaelt_funk(args, "random"))
    {
        return None;
    }
    // Jede Komponente muss eine Zahl ergeben koennen (keine Laenge).
    for k in args.iter().filter(|k| !k.ist_leer() && !k.ist_komma() && !k.ist_delim('/')) {
        if let Kv::F(f, a) = k
            && mathe::ist_mathe(f)
            && mathe::lesen(f, a, Ziel::Zahl).is_none()
            && mathe::lesen(f, a, Ziel::Winkel).is_none()
            && mathe::lesen(f, a, Ziel::Prozent).is_none()
        {
            return None;
        }
    }
    let t = format!("{}({})", n, kv_text(args));
    Some(Farbe::Form(t.clone(), t))
}

fn relativ(n: &str, args: &[Kv]) -> Option<Farbe> {
    if !matches!(n, "rgb" | "rgba" | "hsl" | "hsla" | "hwb" | "lab" | "lch" | "oklab" | "oklch" | "color") {
        return None;
    }
    let teile: Vec<&Kv> = args.iter().filter(|k| !k.ist_leer()).collect();
    let basis = lesen(teile.get(1)?)?;
    let n = match n {
        "rgba" => "rgb",
        "hsla" => "hsl",
        x => x,
    };
    let rest: Vec<Kv> = args.iter().skip_while(|k| k.ist_leer()).skip(1).skip_while(|k| k.ist_leer()).skip(1).cloned().collect();
    let t = format!("{}(from {} {})", n, basis.text(), kv_text(&rest));
    // Release 14.10.2026: the computed value of a relative `rgb()` (CSS Color
    // 5 §4.1): the channels of the origin color as numbers, the result as
    // `color(srgb …)`. MEASURED before (WPT css-values clamp-color-computed):
    // the computed value was the specified text.
    let computed = (n == "rgb").then(|| relative_rgb(&basis, &rest)).flatten();
    Some(Farbe::Form(t.clone(), computed.unwrap_or(t)))
}

/// The sRGB channels (0..255) and alpha of an origin color, if known.
fn srgb_of(f: &Farbe) -> Option<([f64; 3], f64)> {
    match f {
        Farbe::Name(_, (r, g, b)) => Some(([f64::from(*r), f64::from(*g), f64::from(*b)], 1.0)),
        Farbe::Rgb(c, a, _) => Some((*c, *a)),
        Farbe::Transparent => Some(([0.0; 3], 0.0)),
        _ => None,
    }
}

/// `rgb(from <origin> R G B [/ A])`: each channel is a number, a percentage,
/// a channel keyword (`r`, `g`, `b`, `alpha`) or a math function over them.
fn relative_rgb(origin: &Farbe, rest: &[Kv]) -> Option<String> {
    let (c, alpha) = srgb_of(origin)?;
    let channel = |name: &str| -> Option<f64> {
        match name.to_ascii_lowercase().as_str() {
            "r" => Some(c[0]),
            "g" => Some(c[1]),
            "b" => Some(c[2]),
            "alpha" => Some(alpha),
            _ => None,
        }
    };
    fn replace(kv: &[Kv], channel: &dyn Fn(&str) -> Option<f64>) -> Vec<Kv> {
        kv.iter()
            .map(|k| match k {
                Kv::T(Tok::Ident(n)) => channel(n).map_or_else(|| k.clone(), |v| Kv::T(Tok::Zahl(v, false, false))),
                Kv::F(n, a) => Kv::F(n.clone(), replace(a, channel)),
                Kv::B(b, a) => Kv::B(*b, replace(a, channel)),
                k => k.clone(),
            })
            .collect()
    }
    let parts: Vec<&Kv> = rest.iter().filter(|k| !k.ist_leer()).collect();
    let (channels, alpha_part) = match parts.iter().position(|k| k.ist_delim('/')) {
        Some(i) => (&parts[..i], parts.get(i + 1..)),
        None => (&parts[..], None),
    };
    if channels.len() != 3 || alpha_part.is_some_and(|a| a.len() != 1) {
        return None;
    }
    let value = |k: &Kv, scale: f64| -> Option<f64> {
        match k {
            Kv::T(Tok::Zahl(w, _, _)) => Some(*w),
            Kv::T(Tok::Prozent(w, _)) => Some(w / 100.0 * scale),
            Kv::T(Tok::Ident(n)) => channel(n),
            Kv::F(name, args) => {
                let args = replace(args, &channel);
                let m = mathe::lesen(name, &args, Ziel::Zahl)?;
                m.aufgeloest(&mathe::Umgebung::default()).zahlwert().map(|(w, _)| w)
            }
            _ => None,
        }
    };
    let mut out = [0.0; 3];
    for (i, k) in channels.iter().enumerate() {
        out[i] = value(k, 255.0)?;
    }
    let a = match alpha_part {
        Some(a) => value(a[0], 1.0)?,
        None => alpha,
    }
    .clamp(0.0, 1.0);
    let num = |x: f64| zahl_text((x / 255.0 * 1e6).round() / 1e6);
    let mut text = format!("color(srgb {} {} {}", num(out[0]), num(out[1]), num(out[2]));
    if a < 1.0 {
        text.push_str(&format!(" / {}", zahl_text((a * 1e6).round() / 1e6)));
    }
    text.push(')');
    Some(text)
}

fn farbmischung(args: &[Kv]) -> Option<Farbe> {
    let teile = teile_komma(args);
    if teile.len() < 2 {
        return None;
    }
    let kopf: Vec<&Kv> = teile[0].iter().filter(|k| !k.ist_leer()).collect();
    if !kopf.first()?.ist_wort("in") || kopf.len() < 2 {
        return None;
    }
    let raum: Vec<String> = kopf[1..].iter().map(|k| k.ident().map(|s| ident_text(&s.to_ascii_lowercase()))).collect::<Option<_>>()?;
    let mut stuecke = Vec::new();
    for t in &teile[1..] {
        let ks: Vec<&Kv> = t.iter().filter(|k| !k.ist_leer()).collect();
        let mut farbe = None;
        let mut p = None;
        for k in ks {
            if let Kv::T(Tok::Prozent(w, _)) = k {
                if p.is_some() || !(0.0..=100.0).contains(w) {
                    return None;
                }
                p = Some(*w);
            } else if farbe.is_none() {
                farbe = Some(lesen(k)?);
            } else {
                return None;
            }
        }
        stuecke.push((farbe?, p));
    }
    let mut t = format!("color-mix(in {}", raum.join(" "));
    for (f, p) in &stuecke {
        t.push_str(", ");
        t.push_str(&f.text());
        if let Some(p) = p {
            t.push(' ');
            t.push_str(&zahl_text(*p));
            t.push('%');
        }
    }
    t.push(')');
    Some(Farbe::Form(t.clone(), t))
}

#[cfg(test)]
mod tests {
    use super::super::syntax::komponenten;
    use super::*;

    fn f(t: &str) -> (String, String) {
        let k = komponenten(t);
        let k = getrimmt(&k);
        lesen(&k[0]).map(|f| (f.text(), f.berechnet(None))).unwrap_or_default()
    }

    #[test]
    fn farben() {
        assert_eq!(f("RED").0, "red");
        assert_eq!(f("#00FF00").0, "rgb(0, 255, 0)");
        assert_eq!(f("#ff000080").0, "rgba(255, 0, 0, 0.5)");
        assert_eq!(f("rgb(100%, 100%, 0%)").0, "rgb(255, 255, 0)");
        assert_eq!(f("hsl(120, 100%, 50%)").0, "rgb(0, 255, 0)");
        assert_eq!(f("rgb(0 0 0 / 50%)").0, "rgba(0, 0, 0, 0.5)");
        assert_eq!(f("CanvasText").0, "canvastext");
        assert_eq!(f("transparent").1, "rgba(0, 0, 0, 0)");
        assert_eq!(f("oklch(76% .23 150)").0, "oklch(0.76 0.23 150)");
        assert_eq!(f("rgb(calc(0), calc(255 + 0), calc(140 - 139 - 1))").0, "rgb(0, 255, 0)");
        assert_eq!(f("rgb(1, 2)").0, "");
        assert_eq!(f("rgb(1, 2 3)").0, "");
        assert_eq!(f("rgb(1%, 2, 3)").0, "");
    }
}
