//! Release 12.10.2026: das CSSOM der Werte (CSSOM §6.7, CSS Values 4).
//!
//! Was hier steht und warum ein eigenes Modul: die Kaskade (`parse.rs`,
//! `kaskade.rs`) liest Werte fehlertolerant fuer das Zeichnen. Das CSSOM
//! braucht dagegen die GENAUE Grammatik je Eigenschaft (ungueltig heisst:
//! nicht setzen), die kanonische Schreibweise des angegebenen Werts
//! (`0` -> `0px`, Schluesselwoerter klein, Farben, `calc()` vereinfacht),
//! Kurzformen (Aufteilen beim Setzen, Zusammensetzen beim Lesen) und den
//! Deklarationsblock (`cssText`, `setProperty`, `removeProperty`).
//!
//! GEMESSEN vorher (WPT, Lauf c1 ueber css/*/parsing, css/cssom,
//! css/css-values, css/css-cascade): 298 von 1349 Dateien gruen.

pub mod eigenschaften;
pub mod ersetzen;
pub mod farbe;
pub mod grammatik;
pub mod kurz;
pub mod matrix;
pub mod bedingung;
pub mod berechnet;
pub mod block;
/// Release 15.10.2026: computed `linear()` easing functions.
pub mod easing;
/// Release 15.10.2026: invalid transition/animation declarations.
pub mod anim_check;
pub mod mathe;
pub mod medien;
pub mod interpolieren;
pub mod merkmalwerte;
pub mod regeln;
pub mod selektor;
pub mod sonder;
pub mod syntax;
pub mod werte;
pub mod wenn;
pub mod zahl;

use std::collections::HashMap;
use std::sync::OnceLock;

use grammatik::G;
use syntax::Kv;
use werte::S;

/// Release 12.10.2026: ist `s` eine gueltige Selektorliste (fuer
/// `CSS.supports('selector(…)')`: vergebende Listen streng)?
pub fn selektor_gueltig(s: &str) -> bool {
    selektor::liste_mit(s, &selektor::Namensraeume::default(), true).is_some()
}

/// Die gelesenen Grammatiken der Langformen.
fn grammatiken() -> &'static HashMap<&'static str, G> {
    static GR: OnceLock<HashMap<&'static str, G>> = OnceLock::new();
    GR.get_or_init(|| {
        let mut m = HashMap::new();
        for (n, g, _, _) in eigenschaften::LANG {
            if let Some(g) = grammatik::lesen(g) {
                m.insert(*n, g);
            }
        }
        m
    })
}

/// Ist `name` (kanonisch, klein) eine bekannte Langform?
pub fn ist_lang(name: &str) -> bool {
    eigenschaften::lang(name).is_some() && kurz::langformen(name).is_none()
}

/// Release 12.10.2026: alle Langformen in der Reihenfolge von
/// `getComputedStyle` (CSSOM §9: lexikographisch, Herstellerpraefixe zuletzt).
pub fn langformen_sortiert() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = eigenschaften::LANG.iter().map(|e| e.0).filter(|n| ist_lang(n)).collect();
    v.sort_by(|a, b| (a.starts_with('-'), *a).cmp(&(b.starts_with('-'), *b)));
    v.dedup();
    v
}

/// Der Anfangswert einer Langform (angegeben).
pub fn anfangswert(name: &str) -> Option<&'static str> {
    eigenschaften::lang(name).map(|e| e.2)
}

/// Wird die Langform vererbt?
pub fn vererbt(name: &str) -> bool {
    eigenschaften::lang(name).is_some_and(|e| e.3)
}

/// Den Wert einer Langform gegen ihre Grammatik lesen; die Stuecke in
/// kanonischer Form. `None` = ungueltig (CSS-weite Woerter und `var()`
/// behandelt der Aufrufer).
pub fn lang_lesen(name: &str, kv: &[Kv]) -> Option<Vec<S>> {
    let items: Vec<&Kv> = kv.iter().filter(|k| !k.ist_leer()).collect();
    lang_lesen_items(name, &items)
}

/// Wie [`lang_lesen`], fuer schon von Leerzeichen befreite Stuecke.
pub fn lang_lesen_items(name: &str, items: &[&Kv]) -> Option<Vec<S>> {
    let g = grammatiken().get(name)?;
    if items.is_empty() {
        return None;
    }
    let sortieren = !matches!(name, "hanging-punctuation" | "word-space-transform" | "grid-lanes-direction" | "animation-range-start" | "animation-range-end");
    let s = grammatik::abgleichen_mit(g, items, sortieren)?;
    if !pruefen(name, &s) {
        return None;
    }
    Some(nach(name, s))
}

/// Zusaetzliche Gueltigkeitsregeln, die die Grammatik nicht ausdrueckt.
fn pruefen(name: &str, s: &[S]) -> bool {
    match name {
        // CSS Grid 2 §8.3: eine woertliche 0 ist keine Rasterlinie.
        "grid-row-start" | "grid-row-end" | "grid-column-start" | "grid-column-end" => {
            !s.iter().any(|p| matches!(p, S::Z(werte::Num::L(w, _)) if *w == 0.0))
        }
        // Release 15.10.2026: an empty string names no keyframes (CSS
        // Animations 1 §3.2; WPT css-animations animation-name-invalid).
        "animation-name" => !s.iter().any(|p| matches!(p, S::Txt(t) if t == "\"\"")),
        _ => true,
    }
}

/// Den Wert einer Langform als kanonischen Text (angegebener Wert).
pub fn lang_text(name: &str, wert: &str) -> Option<String> {
    let kv = syntax::komponenten(wert);
    lang_lesen(name, &kv).map(|s| werte::text(&s))
}

/// Die Stuecke nach Kommas in Gruppen teilen.
pub fn schichten(s: &[S]) -> Vec<Vec<S>> {
    let mut raus = vec![Vec::new()];
    for p in s {
        if *p == S::Komma {
            raus.push(Vec::new());
        } else if let Some(l) = raus.last_mut() {
            l.push(p.clone());
        }
    }
    raus
}

fn verbinden(gruppen: Vec<Vec<S>>) -> Vec<S> {
    let mut raus = Vec::new();
    for (i, g) in gruppen.into_iter().enumerate() {
        if i > 0 {
            raus.push(S::Komma);
        }
        raus.extend(g);
    }
    raus
}

/// Vier Seitenwerte auf die kuerzeste Form bringen (CSSOM, „margin: 1px").
pub fn kasten_kuerzen(mut v: Vec<S>) -> Vec<S> {
    if v.len() == 4 && v[3] == v[1] {
        v.pop();
    }
    if v.len() == 3 && v[2] == v[0] {
        v.pop();
    }
    if v.len() == 2 && v[1] == v[0] {
        v.pop();
    }
    v
}

fn wort(s: &S) -> Option<&str> {
    match s {
        S::W(w) => Some(w),
        _ => None,
    }
}

/// Kanonische Kuerzungen einzelner Langformen (wie Chrome/Firefox sie schreiben).
/// Release 12.10.2026: eine woertliche Zahl (ohne Einheit).
fn zahl_von(p: &S) -> Option<f64> {
    match p {
        S::Z(werte::Num::L(w, e)) if e.is_empty() => Some(*w),
        _ => None,
    }
}

/// Release 12.10.2026: eine woertliche Null-Laenge (nicht `0%`).
fn null_laenge(p: &S) -> bool {
    matches!(p, S::Z(werte::Num::L(w, e)) if *w == 0.0 && e != "%")
}

/// Prozent -> Zahl (`scale`, `scale()`; CSS Transforms 2 §5.1).
fn prozent_zahl(p: S) -> S {
    match p {
        S::Z(werte::Num::L(w, e)) if e == "%" => S::Z(werte::Num::L(w / 100.0, String::new())),
        p => p,
    }
}

/// Release 12.10.2026: die kanonische Form von `scale` (CSS Transforms 2
/// §5.1: Prozent als Zahl, `z = 1` und `y = x` fallen weg; WPT css-transforms
/// scale-parsing-valid).
pub(crate) fn scale_kurz(v: Vec<S>) -> Vec<S> {
    let mut v: Vec<S> = v.into_iter().map(prozent_zahl).collect();
    if v.len() == 3 && zahl_von(&v[2]) == Some(1.0) {
        v.pop();
    }
    if v.len() == 2 && zahl_von(&v[0]).is_some() && zahl_von(&v[0]) == zahl_von(&v[1]) {
        v.pop();
    }
    v
}

/// Release 15.10.2026: a timeline range name with its default offset drops
/// the offset (`cover 0%` -> `cover` for the start, `cover 100%` for the
/// end; Scroll-driven Animations 1 §4.1).
fn range_kurz(s: Vec<S>, standard: f64) -> Vec<S> {
    let mut raus: Vec<S> = Vec::with_capacity(s.len());
    let mut nach_name = false;
    for p in s {
        let ist_standard = matches!(&p, S::Z(werte::Num::L(w, e)) if *w == standard && e == "%");
        if nach_name && ist_standard {
            nach_name = false;
            continue;
        }
        nach_name = matches!(&p, S::W(w) if matches!(w.as_str(), "cover" | "contain" | "entry" | "exit" | "entry-crossing" | "exit-crossing"));
        raus.push(p);
    }
    raus
}

/// Release 15.10.2026: `step-start` is `steps(1, start)`, `step-end` and a
/// `steps()` ending at the end drop the default position (CSS Easing 1
/// §2.3; WPT css-transitions transition-timing-function-valid).
fn steps_canonical(s: Vec<S>) -> Vec<S> {
    let one = || S::Z(werte::Num::L(1.0, String::new()));
    s.into_iter()
        .map(|p| match p {
            S::W(w) if w == "step-start" => S::F(String::from("steps"), vec![one(), S::Komma, S::W(String::from("start"))]),
            S::W(w) if w == "step-end" => S::F(String::from("steps"), vec![one()]),
            S::F(n, a) if n == "steps" && a.len() == 3 && matches!(&a[2], S::W(w) if w == "end" || w == "jump-end") => {
                S::F(n, a.into_iter().take(1).collect())
            }
            p => p,
        })
        .collect()
}

/// Release 15.10.2026: the text of a serialized string (`text_text`).
fn string_value(t: &str) -> Option<String> {
    let inner = t.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 && chars.peek().is_some_and(char::is_ascii_hexdigit) {
            hex.extend(chars.next());
        }
        if hex.is_empty() {
            out.extend(chars.next());
        } else {
            if chars.peek() == Some(&' ') {
                chars.next();
            }
            out.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('\u{FFFD}'));
        }
    }
    Some(out)
}

/// Release 15.10.2026: a keyframes name given as a string that is a valid
/// `<custom-ident>` serializes as that identifier (as in Chrome; WPT
/// css-animations animation-name-valid: `"multi word string"` ->
/// `multi\ word\ string`).
fn names_canonical(s: Vec<S>) -> Vec<S> {
    s.into_iter()
        .map(|p| match p {
            S::Txt(t) => match string_value(&t) {
                Some(v)
                    if !v.is_empty()
                        && !werte::ist_css_weit(&v)
                        && !v.eq_ignore_ascii_case("none")
                        && !v.eq_ignore_ascii_case("default") =>
                {
                    S::W(syntax::ident_text(&v))
                }
                _ => S::Txt(t),
            },
            p => p,
        })
        .collect()
}

fn nach(name: &str, s: Vec<S>) -> Vec<S> {
    match name {
        "animation-name" => names_canonical(s),
        "transition-timing-function" | "animation-timing-function" => steps_canonical(s),
        "animation-range-start" => range_kurz(s, 0.0),
        "animation-range-end" => range_kurz(s, 100.0),
        // Release 12.10.2026: kanonische Formen der einzelnen Transformationen
        // (CSS Transforms 2 §5; WPT css-transforms rotate/scale/translate-parsing).
        "scale" => scale_kurz(s),
        // Release 12.10.2026: ein einzelner Ursprung bekommt die andere Achse
        // (`left` -> `left center`; WPT css-transforms transform-origin-valid).
        "transform-origin" if s.len() == 1 => {
            let mut v = s;
            if matches!(wort(&v[0]), Some("top" | "bottom")) {
                v.insert(0, S::W(String::from("center")));
            } else {
                v.push(S::W(String::from("center")));
            }
            v
        }
        "translate" => {
            let mut v = s;
            if v.len() == 3 && null_laenge(&v[2]) {
                v.pop();
            }
            if v.len() == 2 && null_laenge(&v[1]) {
                v.pop();
            }
            v
        }
        "rotate" => {
            // Achse als drei Zahlen: eine Hauptachse wird `x`/`y`/(z: weg), eine
            // negative dreht den Winkel um.
            let achse: Vec<f64> = s.iter().filter_map(zahl_von).collect();
            let winkel = s.iter().position(|p| matches!(p, S::Z(werte::Num::L(_, e)) if !e.is_empty()));
            if achse.len() == 3
                && let Some(wi) = winkel
                && let S::Z(werte::Num::L(w, e)) = &s[wi]
            {
                let (a, b, c) = (achse[0], achse[1], achse[2]);
                let haupt = match (a != 0.0, b != 0.0, c != 0.0) {
                    (true, false, false) => Some(("x", a)),
                    (false, true, false) => Some(("y", b)),
                    (false, false, true) => Some(("z", c)),
                    _ => None,
                };
                if let Some((n, z)) = haupt {
                    let w = if z < 0.0 { -*w } else { *w };
                    let wi = S::Z(werte::Num::L(w, e.clone()));
                    return if n == "z" { vec![wi] } else { vec![S::W(n.to_string()), wi] };
                }
                return s;
            }
            // `z` ist die Vorgabe und faellt weg.
            s.into_iter().filter(|p| wort(p) != Some("z")).collect()
        }
        "transform" => s
            .into_iter()
            .map(|p| match p {
                S::F(n, a) => {
                    // Wie Chrome: scale*/skew* klein, translate*/rotate* wie geschrieben
                    // (WPT css-transforms transform-valid).
                    let klein = n.to_ascii_lowercase();
                    let n = if klein.starts_with("scale") || klein.starts_with("skew") { klein } else { n };
                    let a = if matches!(n.as_str(), "scale" | "scalex" | "scaley" | "scalez" | "scale3d") {
                        a.into_iter().map(prozent_zahl).collect()
                    } else {
                        a
                    };
                    S::F(n, a)
                }
                p => p,
            })
            .collect(),
        // Release 12.10.2026: ein `<ratio>` steht immer mit Nenner (CSS Values
        // 4 §7.6: `16` -> `16 / 1`; WPT css-sizing aspect-ratio-valid).
        "aspect-ratio" => {
            let mut v = s;
            if !v.contains(&S::Strich)
                && let Some(i) = v.iter().position(|p| matches!(p, S::Z(_)))
            {
                v.insert(i + 1, S::Strich);
                v.insert(i + 2, S::Z(werte::Num::L(1.0, String::new())));
            }
            v
        }
        // Release 12.10.2026: `counter(x, decimal)` -> `counter(x)` (der
        // Vorgabestil faellt weg; WPT cssom serialize-values).
        "content" => s
            .into_iter()
            .map(|p| match p {
                S::F(n, mut a) if (n == "counter" || n == "counters") && a.len() >= 3 && wort(&a[a.len() - 1]) == Some("decimal") => {
                    a.truncate(a.len() - 2);
                    S::F(n, a)
                }
                p => p,
            })
            .collect(),
        "justify-content" | "align-content" | "justify-items" | "align-items" | "justify-self" | "align-self" => {
            let mut v = s;
            if v.len() == 2 && wort(&v[0]) == Some("first") && wort(&v[1]) == Some("baseline") {
                v.remove(0);
            }
            v
        }
        "background-size" | "mask-size" => verbinden(
            schichten(&s)
                .into_iter()
                .map(|mut g| {
                    if g.len() == 2 && g[0] == g[1] && wort(&g[0]) == Some("auto") {
                        g.pop();
                    } else if g.len() == 1 && !matches!(wort(&g[0]), Some("auto" | "cover" | "contain")) {
                        g.push(S::W(String::from("auto")));
                    }
                    g
                })
                .collect(),
        ),
        "background-repeat" | "mask-repeat" => verbinden(
            schichten(&s)
                .into_iter()
                .map(|g| {
                    if g.len() == 2 {
                        match (wort(&g[0]), wort(&g[1])) {
                            (Some("repeat"), Some("no-repeat")) => return vec![S::W(String::from("repeat-x"))],
                            (Some("no-repeat"), Some("repeat")) => return vec![S::W(String::from("repeat-y"))],
                            (a, b) if a == b => return vec![g[0].clone()],
                            _ => {}
                        }
                    }
                    g
                })
                .collect(),
        ),
        "grid-auto-flow" => {
            let mut v = s;
            if v.len() == 2 && wort(&v[0]) == Some("row") {
                v.remove(0);
            }
            v
        }
        "border-top-left-radius" | "border-top-right-radius" | "border-bottom-right-radius" | "border-bottom-left-radius"
        | "border-start-start-radius" | "border-start-end-radius" | "border-end-start-radius" | "border-end-end-radius"
        | "border-spacing" => {
            let mut v = s;
            if v.len() == 2 && v[0] == v[1] {
                v.pop();
            }
            v
        }
        "border-image-width" | "border-image-outset" => kasten_kuerzen(s),
        "border-image-slice" => {
            let mut v = s;
            let fill = v.last().and_then(wort) == Some("fill");
            if fill {
                v.pop();
            }
            let mut v = kasten_kuerzen(v);
            if fill {
                v.push(S::W(String::from("fill")));
            }
            v
        }
        "border-image-repeat" | "scroll-snap-align" => {
            let mut v = s;
            if v.len() == 2 && v[0] == v[1] {
                v.pop();
            }
            v
        }
        "overflow-clip-margin" => {
            // `padding-box` ist die Vorgabe und faellt weg, eine 0 neben einer
            // Box auch; allein bleibt `0px` (WPT css-overflow).
            let mut v = s;
            if v.first().and_then(wort) == Some("padding-box") {
                v.remove(0);
            }
            if v.len() == 2 && matches!(&v[1], S::Z(n) if n.ist_null()) {
                v.pop();
            }
            if v.is_empty() {
                v.push(S::Z(werte::Num::L(0.0, String::from("px"))));
            }
            v
        }
        "grid-row-start" | "grid-row-end" | "grid-column-start" | "grid-column-end" => {
            // `span 1 i` -> `span i`.
            let mut v = s;
            if v.len() == 3 && wort(&v[0]) == Some("span") && matches!(&v[1], S::Z(werte::Num::L(w, _)) if *w == 1.0) {
                v.remove(1);
            }
            v
        }
        "hyphenate-limit-chars" => {
            // Der dritte Wert ist vorgegeben der zweite (CSS Text 4 §6.2).
            let mut v = s;
            if v.len() == 3 && v[2] == v[1] {
                v.pop();
            }
            if v.len() == 2 && v[1] == v[0] {
                v.pop();
            }
            v
        }
        "margin-trim" => {
            let mut v = s;
            if v.len() == 2 {
                v = vec![S::W(String::from("block"))];
            }
            v
        }
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: &str, w: &str) -> String {
        lang_text(n, w).unwrap_or_default()
    }

    #[test]
    fn langformen() {
        assert_eq!(t("max-width", "min-content"), "min-content");
        assert_eq!(t("min-width", "0"), "0px");
        assert_eq!(t("width", "-1px"), "");
        assert_eq!(t("box-shadow", "inset 1px 2px red"), "red 1px 2px inset");
        assert_eq!(t("box-shadow", "1px"), "");
        assert_eq!(t("text-autospace", "insert punctuation ideograph-alpha"), "ideograph-alpha punctuation insert");
        assert_eq!(t("display", "flow-root inline"), "inline-block");
        assert_eq!(t("display", "block block"), "");
        assert_eq!(t("background-position", "bottom 16% left"), "left bottom 16%");
        assert_eq!(t("background-position", "1px"), "1px center");
        assert_eq!(t("grid-template-columns", "repeat(2, fit-content(20%) [three four] 30px 40px [five six])"), "repeat(2, fit-content(20%) [three four] 30px 40px [five six])");
        assert_eq!(t("grid-template-columns", "[one]"), "");
        assert_eq!(t("justify-items", "left legacy"), "legacy left");
        assert_eq!(t("justify-items", "first baseline"), "baseline");
        assert_eq!(t("grid-auto-flow", "dense row"), "dense");
        assert_eq!(t("grid-row-start", "span 2 foo"), "span 2 foo");
        assert_eq!(t("grid-row-start", "0"), "");
        assert_eq!(t("background-size", "1px"), "1px auto");
        assert_eq!(t("z-index", "calc(1.5)"), "calc(1.5)");
        assert_eq!(t("color", "#FFF"), "rgb(255, 255, 255)");
        assert_eq!(t("background-image", "linear-gradient(to left bottom, red, blue)"), "linear-gradient(to left bottom, red, blue)");
        assert_eq!(t("font-family", "Arial Black, 'x y', SERIF"), "Arial Black, \"x y\", serif");
        assert_eq!(t("grid-template-areas", "\"  a  \t  b  \""), "\"a b\"");
        assert_eq!(t("transform", "translateX(10px) rotate(0)"), "translateX(10px) rotate(0deg)");
    }
}
