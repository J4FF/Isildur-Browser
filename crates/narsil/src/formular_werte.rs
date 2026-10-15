//! Release 11.10.2026: die Werte der Formularfelder nach HTML §4.10.5
//! („value sanitization algorithm" je `type`, die Wertmodi `value`,
//! `default`, `default/on`, `filename`).
//!
//! GEMESSEN vorher (WPT html/semantics/forms/the-input-element, 97 von 126
//! gescheitert): ein per Skript erzeugtes `<input>` verlor jeden gesetzten
//! Wert (alle teilten sich die Vorlauf-Nummer 0), und kein Typ bereinigte
//! (`type=color` mit `#ABCDEF` blieb gross, `number` nahm „abc").

/// Der Wertmodus eines `<input>`-Typs (HTML §4.10.5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wertmodus {
    /// Der Wert ist ein eigener, „schmutziger" Wert (Textfelder, Zahlen …).
    Wert,
    /// `value` spiegelt das Attribut (hidden, submit, image, reset, button).
    Vorgabe,
    /// Wie `Vorgabe`, ohne Attribut aber `"on"` (checkbox, radio).
    VorgabeAn,
    /// `file`: der Dateiname (hier immer leer).
    Dateiname,
}

#[must_use]
pub fn wertmodus(typ: &str) -> Wertmodus {
    match typ {
        "hidden" | "submit" | "image" | "reset" | "button" => Wertmodus::Vorgabe,
        "checkbox" | "radio" => Wertmodus::VorgabeAn,
        "file" => Wertmodus::Dateiname,
        _ => Wertmodus::Wert,
    }
}

fn ohne_umbrueche(s: &str) -> String {
    s.chars().filter(|c| *c != '\n' && *c != '\r').collect()
}

fn ascii_leer(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{c}' | '\r')
}

/// „valid floating-point number" (HTML §2.3.4.3).
#[must_use]
pub fn gueltige_zahl(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut i = 0;
    if b[i] == b'-' {
        i += 1;
    }
    let ziffern_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let ganz = i > ziffern_start;
    let mut bruch = false;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let s2 = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        bruch = i > s2;
        if !bruch {
            return None;
        }
    }
    if !ganz && !bruch {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
            i += 1;
        }
        let s3 = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == s3 {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn tage_im_monat(jahr: i64, monat: u32) -> u32 {
    match monat {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (jahr % 4 == 0 && jahr % 100 != 0) || jahr % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

fn ziffern(s: &str, n: usize) -> Option<u32> {
    (s.len() == n && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

/// `YYYY-MM` (Jahr mindestens vier Ziffern, > 0).
fn monat_lesen(s: &str) -> Option<(i64, u32)> {
    let (j, m) = s.rsplit_once('-')?;
    if j.len() < 4 || !j.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let jahr: i64 = j.parse().ok()?;
    let monat = ziffern(m, 2)?;
    (jahr > 0 && (1..=12).contains(&monat)).then_some((jahr, monat))
}

/// `YYYY-MM-DD`.
#[must_use]
pub fn datum_lesen(s: &str) -> Option<(i64, u32, u32)> {
    let (jm, t) = s.rsplit_once('-')?;
    let (jahr, monat) = monat_lesen(jm)?;
    let tag = ziffern(t, 2)?;
    (tag >= 1 && tag <= tage_im_monat(jahr, monat)).then_some((jahr, monat, tag))
}

/// `HH:MM[:SS[.s{1,3}]]` -> Millisekunden seit Mitternacht.
#[must_use]
pub fn zeit_lesen(s: &str) -> Option<f64> {
    let teile: Vec<&str> = s.split(':').collect();
    if teile.len() < 2 || teile.len() > 3 {
        return None;
    }
    let h = ziffern(teile[0], 2)?;
    let m = ziffern(teile[1], 2)?;
    if h > 23 || m > 59 {
        return None;
    }
    let mut ms = (h as f64 * 3600.0 + m as f64 * 60.0) * 1000.0;
    if teile.len() == 3 {
        let (sek, bruch) = match teile[2].split_once('.') {
            Some((a, b)) => (a, Some(b)),
            None => (teile[2], None),
        };
        let s2 = ziffern(sek, 2)?;
        if s2 > 59 {
            return None;
        }
        ms += s2 as f64 * 1000.0;
        if let Some(b) = bruch {
            if b.is_empty() || b.len() > 3 || !b.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let wert: f64 = format!("0.{b}").parse().ok()?;
            ms += (wert * 1000.0).round();
        }
    }
    Some(ms)
}

/// `YYYY-Www` (ISO-Woche; Jahre mit 53 Wochen erlaubt).
fn woche_lesen(s: &str) -> Option<(i64, u32)> {
    let (j, w) = s.split_once("-W")?;
    if j.len() < 4 || !j.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let jahr: i64 = j.parse().ok()?;
    let woche = ziffern(w, 2)?;
    if jahr <= 0 || woche < 1 {
        return None;
    }
    // Ein Jahr hat 53 Wochen, wenn der 1. Januar ein Donnerstag ist oder ein
    // Schaltjahr mit dem 1. Januar am Mittwoch beginnt.
    let wt = wochentag(jahr, 1, 1); // 0 = Montag
    let schalt = (jahr % 4 == 0 && jahr % 100 != 0) || jahr % 400 == 0;
    let max = if wt == 3 || (schalt && wt == 2) { 53 } else { 52 };
    (woche <= max).then_some((jahr, woche))
}

/// Wochentag (0 = Montag) nach Zeller/Sakamoto.
fn wochentag(jahr: i64, monat: u32, tag: u32) -> u32 {
    const T: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let j = if monat < 3 { jahr - 1 } else { jahr };
    let w = (j + j / 4 - j / 100 + j / 400 + T[(monat - 1) as usize] + tag as i64).rem_euclid(7);
    // Sakamoto: 0 = Sonntag -> auf Montag = 0 umrechnen.
    ((w + 6) % 7) as u32
}

/// Ein Wert, der als „normalized local date and time string" gilt.
fn datum_zeit_normalisieren(s: &str) -> Option<String> {
    let (d, z) = s.split_once(['T', ' '])?;
    let (jahr, monat, tag) = datum_lesen(d)?;
    let ms = zeit_lesen(z)?;
    let ganz = (ms / 1000.0).floor() as u64;
    let rest = (ms - (ganz as f64) * 1000.0).round() as u64;
    let (h, m, sek) = (ganz / 3600, (ganz / 60) % 60, ganz % 60);
    let jahr_text = format!("{jahr:04}");
    let mut t = format!("{jahr_text}-{monat:02}-{tag:02}T{h:02}:{m:02}");
    if sek != 0 || rest != 0 {
        t.push_str(&format!(":{sek:02}"));
        if rest != 0 {
            let mut b = format!("{rest:03}");
            while b.ends_with('0') {
                b.pop();
            }
            t.push('.');
            t.push_str(&b);
        }
    }
    Some(t)
}

/// Die Bereinigung eines Werts fuer `type` (HTML §4.10.5.1.x). `attr` liest
/// ein Attribut des Elements (fuer `range`: `min`/`max`).
#[must_use]
/// Release 11.10.2026: HTML §4.10.5.1.15 (Color) mit `colorspace` und
/// `alpha` — der Wert ist eine CSS-Farbe (sonst Schwarz), ohne `alpha`
/// deckend, im Farbraum des Elements; `limited-srgb` ohne Deckkraft als
/// `#rrggbb`, sonst `color(srgb|display-p3 r g b[ / a])`. GEMESSEN vorher
/// (WPT the-input-element/color.window, 14/196): nur `#rrggbb` galt, `crimson`
/// und `rgb(…)` wurden schwarz.
fn farbe_bereinigen(wert: &str, alpha: bool, p3: bool) -> String {
    let f = narsil_css::farbe_aus(wert)
        .or_else(|| farbfunktion_lesen(wert))
        .unwrap_or(narsil_css::Farbe { r: 0, g: 0, b: 0, a: 255 });
    let a = if alpha { f64::from(f.a) / 255.0 } else { 1.0 };
    let (r, g, b) = (f64::from(f.r) / 255.0, f64::from(f.g) / 255.0, f64::from(f.b) / 255.0);
    if !p3 && !alpha {
        return format!("#{:02x}{:02x}{:02x}", f.r, f.g, f.b);
    }
    let (raum, r, g, b) = if p3 {
        // sRGB -> linear -> Display P3 (linear) -> P3-Kodierung (CSS Color 4 §10).
        let lin = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let gam = |c: f64| if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        let (lr, lg, lb) = (lin(r), lin(g), lin(b));
        let pr = 0.822_462_1 * lr + 0.177_538_0 * lg;
        let pg = 0.033_194_1 * lr + 0.966_805_8 * lg;
        let pb = 0.017_082_7 * lr + 0.072_397_4 * lg + 0.910_519_9 * lb;
        ("display-p3", gam(pr), gam(pg), gam(pb))
    } else {
        ("srgb", r, g, b)
    };
    let zahl = |v: f64| {
        let t = format!("{:.6}", (v.clamp(0.0, 1.0) * 1e6).round() / 1e6);
        let t = t.trim_end_matches('0').trim_end_matches('.').to_string();
        if t.is_empty() || t == "-0" { "0".to_string() } else { t }
    };
    if a < 1.0 {
        format!("color({raum} {} {} {} / {})", zahl(r), zahl(g), zahl(b), zahl(a))
    } else {
        format!("color({raum} {} {} {})", zahl(r), zahl(g), zahl(b))
    }
}

/// `color(srgb|display-p3 r g b [/ a])` (die eigene Ausgabe von
/// [`farbe_bereinigen`]) zurueck nach 8-Bit-sRGB.
fn farbfunktion_lesen(wert: &str) -> Option<narsil_css::Farbe> {
    let innen = wert.trim().strip_prefix("color(")?.strip_suffix(')')?;
    let (teile, a) = match innen.split_once('/') {
        Some((t, a)) => (t, a.trim().parse::<f64>().ok()?),
        None => (innen, 1.0),
    };
    let mut it = teile.split_ascii_whitespace();
    let raum = it.next()?;
    let w: Vec<f64> = it.map(|z| z.parse::<f64>()).collect::<Result<_, _>>().ok()?;
    if w.len() != 3 {
        return None;
    }
    let (r, g, b) = match raum {
        "srgb" => (w[0], w[1], w[2]),
        "display-p3" => {
            let lin = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
            let gam = |c: f64| if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
            let (pr, pg, pb) = (lin(w[0]), lin(w[1]), lin(w[2]));
            (
                gam(1.224_940_2 * pr - 0.224_940_2 * pg),
                gam(-0.042_056_9 * pr + 1.042_056_9 * pg),
                gam(-0.019_637_6 * pr - 0.078_636_1 * pg + 1.098_273_7 * pb),
            )
        }
        _ => return None,
    };
    let acht = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(narsil_css::Farbe { r: acht(r), g: acht(g), b: acht(b), a: acht(a) })
}

pub fn bereinigen(typ: &str, wert: &str, attr: &dyn Fn(&str) -> Option<String>) -> String {
    match typ {
        "text" | "search" | "tel" | "password" => ohne_umbrueche(wert),
        "url" => ohne_umbrueche(wert).trim_matches(ascii_leer).to_string(),
        "email" => {
            let w = ohne_umbrueche(wert);
            if attr("multiple").is_some() {
                w.split(',').map(|t| t.trim_matches(ascii_leer)).collect::<Vec<_>>().join(",")
            } else {
                w.trim_matches(ascii_leer).to_string()
            }
        }
        "number" => {
            if gueltige_zahl(wert).is_some() {
                wert.to_string()
            } else {
                String::new()
            }
        }
        "range" => {
            let min = attr("min").and_then(|v| gueltige_zahl(&v)).unwrap_or(0.0);
            let mut max = attr("max").and_then(|v| gueltige_zahl(&v)).unwrap_or(100.0);
            if max < min {
                max = min;
            }
            let v = match gueltige_zahl(wert) {
                Some(v) => v.clamp(min, max),
                None => min + (max - min) / 2.0,
            };
            let schritt = match attr("step").map(|s| s.to_ascii_lowercase()) {
                Some(s) if s == "any" => None,
                Some(s) => gueltige_zahl(&s).filter(|v| *v > 0.0).or(Some(1.0)),
                None => Some(1.0),
            };
            let v = match schritt {
                Some(st) => {
                    let n = ((v - min) / st).round();
                    let mut r = min + n * st;
                    if r > max {
                        r -= st;
                    }
                    r
                }
                None => v,
            };
            zahl_text(v)
        }
        "color" => farbe_bereinigen(
            wert,
            attr("alpha").is_some(),
            attr("colorspace").is_some_and(|c| c.eq_ignore_ascii_case("display-p3")),
        ),
        "date" => {
            if datum_lesen(wert).is_some() {
                wert.to_string()
            } else {
                String::new()
            }
        }
        "month" => {
            if monat_lesen(wert).is_some() {
                wert.to_string()
            } else {
                String::new()
            }
        }
        "week" => {
            if woche_lesen(wert).is_some() {
                wert.to_string()
            } else {
                String::new()
            }
        }
        "time" => {
            if zeit_lesen(wert).is_some() {
                wert.to_string()
            } else {
                String::new()
            }
        }
        "datetime-local" => datum_zeit_normalisieren(wert).unwrap_or_default(),
        _ => wert.to_string(),
    }
}

/// Eine Zahl so kurz wie JavaScript sie schreibt (`50`, `0.5`).
#[must_use]
pub fn zahl_text(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ohne(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn bereinigung_je_typ() {
        assert_eq!(bereinigen("text", "a\r\nb", &ohne), "ab");
        assert_eq!(bereinigen("url", "  http://x/ \n", &ohne), "http://x/");
        assert_eq!(bereinigen("color", "#ABCDEF", &ohne), "#abcdef");
        assert_eq!(bereinigen("color", "red", &ohne), "#ff0000");
        assert_eq!(bereinigen("color", "crimson", &|n: &str| (n == "colorspace").then(|| "display-p3".to_string())), "color(display-p3 0.791711 0.191507 0.257367)");
        assert_eq!(bereinigen("color", "rgb(1,1,1,0.5)", &|n: &str| (n == "alpha").then(String::new)), "color(srgb 0.003922 0.003922 0.003922 / 0.501961)");
        assert_eq!(bereinigen("number", "1e3", &ohne), "1e3");
        assert_eq!(bereinigen("number", "abc", &ohne), "");
        assert_eq!(bereinigen("number", "1.", &ohne), "");
        assert_eq!(bereinigen("range", "", &ohne), "50");
        assert_eq!(bereinigen("range", "150", &ohne), "100");
        assert_eq!(bereinigen("date", "2026-02-29", &ohne), "");
        assert_eq!(bereinigen("date", "2024-02-29", &ohne), "2024-02-29");
        assert_eq!(bereinigen("month", "2026-13", &ohne), "");
        assert_eq!(bereinigen("week", "2026-W53", &ohne), "2026-W53");
        assert_eq!(bereinigen("week", "2027-W53", &ohne), "");
        assert_eq!(bereinigen("time", "12:30:05.5", &ohne), "12:30:05.5");
        assert_eq!(bereinigen("time", "24:00", &ohne), "");
        assert_eq!(bereinigen("datetime-local", "2026-10-11 08:05:00", &ohne), "2026-10-11T08:05");
    }
}
