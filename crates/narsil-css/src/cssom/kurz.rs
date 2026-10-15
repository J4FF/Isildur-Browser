//! Release 12.10.2026: Kurzformen — Aufteilen beim Setzen (die Langformen in
//! kanonischer Schreibweise) und Zusammensetzen beim Lesen (CSSOM §6.7.2
//! „serialize a CSS value" fuer Kurzformen: kuerzeste Form, die dieselben
//! Langformen ergibt; `None`, wenn sich die Langformen nicht als diese
//! Kurzform schreiben lassen).
//!
//! GEMESSEN vorher (WPT): `e.style['grid-area'] = 'auto'` setzte nur den
//! Rohtext `grid-area`, `grid-row-start` blieb leer; `border` setzte kein
//! `border-top-width` (rund 130 Teiltests `should set <longhand>`).

use super::syntax::{Kv, Tok};
use super::werte::{self, S};
use super::{kasten_kuerzen, lang_lesen_items};

/// Die Kurzformen und ihre Langformen (in der Reihenfolge, in der sie im
/// Block landen).
pub const KURZ: &[(&str, &[&str])] = &[
    ("margin", &["margin-top", "margin-right", "margin-bottom", "margin-left"]),
    ("padding", &["padding-top", "padding-right", "padding-bottom", "padding-left"]),
    ("inset", &["top", "right", "bottom", "left"]),
    ("margin-block", &["margin-block-start", "margin-block-end"]),
    ("margin-inline", &["margin-inline-start", "margin-inline-end"]),
    ("padding-block", &["padding-block-start", "padding-block-end"]),
    ("padding-inline", &["padding-inline-start", "padding-inline-end"]),
    ("inset-block", &["inset-block-start", "inset-block-end"]),
    ("inset-inline", &["inset-inline-start", "inset-inline-end"]),
    ("border-width", &["border-top-width", "border-right-width", "border-bottom-width", "border-left-width"]),
    ("border-style", &["border-top-style", "border-right-style", "border-bottom-style", "border-left-style"]),
    ("border-color", &["border-top-color", "border-right-color", "border-bottom-color", "border-left-color"]),
    ("border-top", &["border-top-width", "border-top-style", "border-top-color"]),
    ("border-right", &["border-right-width", "border-right-style", "border-right-color"]),
    ("border-bottom", &["border-bottom-width", "border-bottom-style", "border-bottom-color"]),
    ("border-left", &["border-left-width", "border-left-style", "border-left-color"]),
    ("border-block-start", &["border-block-start-width", "border-block-start-style", "border-block-start-color"]),
    ("border-block-end", &["border-block-end-width", "border-block-end-style", "border-block-end-color"]),
    ("border-inline-start", &["border-inline-start-width", "border-inline-start-style", "border-inline-start-color"]),
    ("border-inline-end", &["border-inline-end-width", "border-inline-end-style", "border-inline-end-color"]),
    ("border-block-width", &["border-block-start-width", "border-block-end-width"]),
    ("border-block-style", &["border-block-start-style", "border-block-end-style"]),
    ("border-block-color", &["border-block-start-color", "border-block-end-color"]),
    ("border-inline-width", &["border-inline-start-width", "border-inline-end-width"]),
    ("border-inline-style", &["border-inline-start-style", "border-inline-end-style"]),
    ("border-inline-color", &["border-inline-start-color", "border-inline-end-color"]),
    (
        "border-block",
        &[
            "border-block-start-width",
            "border-block-end-width",
            "border-block-start-style",
            "border-block-end-style",
            "border-block-start-color",
            "border-block-end-color",
        ],
    ),
    (
        "border-inline",
        &[
            "border-inline-start-width",
            "border-inline-end-width",
            "border-inline-start-style",
            "border-inline-end-style",
            "border-inline-start-color",
            "border-inline-end-color",
        ],
    ),
    (
        "border",
        &[
            "border-top-width",
            "border-right-width",
            "border-bottom-width",
            "border-left-width",
            "border-top-style",
            "border-right-style",
            "border-bottom-style",
            "border-left-style",
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
            "border-image-source",
            "border-image-slice",
            "border-image-width",
            "border-image-outset",
            "border-image-repeat",
        ],
    ),
    ("border-radius", &["border-top-left-radius", "border-top-right-radius", "border-bottom-right-radius", "border-bottom-left-radius"]),
    ("-webkit-border-radius", &["border-top-left-radius", "border-top-right-radius", "border-bottom-right-radius", "border-bottom-left-radius"]),
    ("border-image", &["border-image-source", "border-image-slice", "border-image-width", "border-image-outset", "border-image-repeat"]),
    ("outline", &["outline-color", "outline-style", "outline-width"]),
    ("column-rule", &["column-rule-width", "column-rule-style", "column-rule-color"]),
    ("columns", &["column-width", "column-count"]),
    ("flex", &["flex-grow", "flex-shrink", "flex-basis"]),
    ("size", &["width", "height"]),
    ("flex-flow", &["flex-direction", "flex-wrap"]),
    ("gap", &["row-gap", "column-gap"]),
    ("contain-intrinsic-size", &["contain-intrinsic-width", "contain-intrinsic-height"]),
    ("grid-gap", &["row-gap", "column-gap"]),
    ("place-content", &["align-content", "justify-content"]),
    ("place-items", &["align-items", "justify-items"]),
    ("place-self", &["align-self", "justify-self"]),
    ("overflow", &["overflow-x", "overflow-y"]),
    ("overscroll-behavior", &["overscroll-behavior-x", "overscroll-behavior-y"]),
    ("list-style", &["list-style-position", "list-style-image", "list-style-type"]),
    ("text-decoration", &["text-decoration-line", "text-decoration-thickness", "text-decoration-style", "text-decoration-color"]),
    ("text-wrap", &["text-wrap-mode", "text-wrap-style"]),
    ("white-space", &["white-space-collapse", "text-wrap-mode"]),
    ("text-spacing", &["text-spacing-trim", "text-autospace"]),
    ("line-clamp", &["max-lines", "block-ellipsis", "continue"]),
    ("grid-row", &["grid-row-start", "grid-row-end"]),
    ("grid-column", &["grid-column-start", "grid-column-end"]),
    ("grid-area", &["grid-row-start", "grid-column-start", "grid-row-end", "grid-column-end"]),
    ("grid-template", &["grid-template-rows", "grid-template-columns", "grid-template-areas"]),
    // Release 14.10.2026: CSS Grid 3 `grid-lanes` (WPT css-grid grid-lanes
    // tentative parsing).
    ("grid-lanes", &["grid-template-columns", "grid-template-rows", "grid-template-areas", "grid-lanes-direction"]),
    (
        "grid",
        &["grid-template-rows", "grid-template-columns", "grid-template-areas", "grid-auto-rows", "grid-auto-columns", "grid-auto-flow"],
    ),
    ("background-position", &["background-position-x", "background-position-y"]),
    (
        "background",
        &[
            "background-image",
            "background-position-x",
            "background-position-y",
            "background-size",
            "background-repeat",
            "background-attachment",
            "background-origin",
            "background-clip",
            "background-color",
        ],
    ),
    (
        "transition",
        &["transition-property", "transition-duration", "transition-timing-function", "transition-delay", "transition-behavior"],
    ),
    (
        "animation",
        &[
            "animation-duration",
            "animation-timing-function",
            "animation-delay",
            "animation-iteration-count",
            "animation-direction",
            "animation-fill-mode",
            "animation-play-state",
            "animation-name",
            // Release 15.10.2026: reset only (CSS Animations 2 §3.12).
            "animation-timeline",
            "animation-range-start",
            "animation-range-end",
        ],
    ),
    ("animation-range", &["animation-range-start", "animation-range-end"]),
    (
        "font",
        &[
            "font-style",
            "font-variant-caps",
            "font-weight",
            "font-stretch",
            "font-size",
            "line-height",
            "font-family",
            "font-variant-ligatures",
            "font-variant-alternates",
            "font-variant-numeric",
            "font-variant-east-asian",
            "font-variant-position",
            "font-variant-emoji",
            "font-kerning",
            "font-optical-sizing",
            "font-size-adjust",
            "font-feature-settings",
            "font-variation-settings",
        ],
    ),
    (
        "font-variant",
        &[
            "font-variant-ligatures",
            "font-variant-caps",
            "font-variant-alternates",
            "font-variant-numeric",
            "font-variant-east-asian",
            "font-variant-position",
            "font-variant-emoji",
        ],
    ),
];

/// Die Langformen der Kurzform `name`.
pub fn langformen(name: &str) -> Option<&'static [&'static str]> {
    KURZ.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

fn lv(name: &str, items: &[&Kv]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    lang_lesen_items(name, items).map(|s| werte::text(&s))
}

fn anfang(name: &str) -> String {
    super::anfangswert(name).unwrap_or("initial").to_string()
}

fn teilen<'a>(items: &[&'a Kv], trenner: fn(&Kv) -> bool) -> Vec<Vec<&'a Kv>> {
    let mut raus = vec![Vec::new()];
    for k in items {
        if trenner(k) {
            raus.push(Vec::new());
        } else if let Some(l) = raus.last_mut() {
            l.push(*k);
        }
    }
    raus
}

fn ist_strich(k: &Kv) -> bool {
    k.ist_delim('/')
}

fn ist_komma(k: &Kv) -> bool {
    k.ist_komma()
}

/// 1–4 Werte auf vier Seiten (`margin`, `border-width` …).
fn kasten(lang: &[&str], items: &[&Kv]) -> Option<Vec<String>> {
    if items.is_empty() || items.len() > 4 {
        return None;
    }
    let v: Vec<String> = items.iter().map(|k| lv(lang[0], &[*k])).collect::<Option<_>>()?;
    let (t, r, b, l) = match v.len() {
        1 => (v[0].clone(), v[0].clone(), v[0].clone(), v[0].clone()),
        2 => (v[0].clone(), v[1].clone(), v[0].clone(), v[1].clone()),
        3 => (v[0].clone(), v[1].clone(), v[2].clone(), v[1].clone()),
        _ => (v[0].clone(), v[1].clone(), v[2].clone(), v[3].clone()),
    };
    Some(vec![t, r, b, l])
}

fn kasten_text(v: &[String]) -> String {
    let s: Vec<S> = v.iter().map(|x| S::Txt(x.clone())).collect();
    werte::text(&kasten_kuerzen(s))
}

/// Zwei Langformen, jede ueber beliebig viele Stuecke: die erste Teilung, bei
/// der beide gelten; ein einzelner Wert gilt fuer beide (`zweite` bildet ihn um).
fn paar(a: &str, b: &str, items: &[&Kv], zweite: impl Fn(&str) -> Option<String>) -> Option<Vec<String>> {
    if items.is_empty() {
        return None;
    }
    if let Some(x) = lv(a, items) {
        let y = zweite(&x)?;
        return Some(vec![x, y]);
    }
    for k in 1..items.len() {
        if let (Some(x), Some(y)) = (lv(a, &items[..k]), lv(b, &items[k..])) {
            return Some(vec![x, y]);
        }
    }
    None
}

fn paar_text(v: &[String]) -> String {
    if v[0] == v[1] { v[0].clone() } else { format!("{} {}", v[0], v[1]) }
}

/// `a || b || c`: jede Langform hoechstens einmal, in beliebiger Folge;
/// fehlende bekommen den Anfangswert.
fn einige(lang: &[&str], items: &[&Kv]) -> Option<Vec<String>> {
    fn suche(lang: &[&str], items: &[&Kv], pos: usize, werte: &mut Vec<Option<String>>) -> bool {
        if pos == items.len() {
            return werte.iter().any(Option::is_some);
        }
        for i in 0..lang.len() {
            if werte[i].is_some() {
                continue;
            }
            for ende in (pos + 1..=items.len()).rev() {
                if let Some(w) = lv(lang[i], &items[pos..ende]) {
                    werte[i] = Some(w);
                    if suche(lang, items, ende, werte) {
                        return true;
                    }
                    werte[i] = None;
                }
            }
        }
        false
    }
    if items.is_empty() || items.len() > 16 {
        return None;
    }
    let mut werte = vec![None; lang.len()];
    if !suche(lang, items, 0, &mut werte) {
        return None;
    }
    Some(werte.into_iter().enumerate().map(|(i, w)| w.unwrap_or_else(|| anfang(lang[i]))).collect())
}

/// Die kuerzeste `||`-Form: Anfangswerte fallen weg; bleibt nichts, der
/// erste Wert (oder `none`, wenn einer `none` ist).
fn einige_text(lang: &[&str], v: &[String]) -> String {
    let teile: Vec<&str> = v.iter().enumerate().filter(|(i, w)| **w != anfang(lang[*i])).map(|(_, w)| w.as_str()).collect();
    if teile.is_empty() {
        if let Some(n) = v.iter().find(|w| *w == "none") {
            return n.clone();
        }
        return v[0].clone();
    }
    teile.join(" ")
}

fn ist_ident_linie(w: &str) -> bool {
    // Ein einzelner <custom-ident> (kein Schluesselwort, keine Zahl) — am
    // Token gemessen, denn `\31 st` ist EIN Bezeichner mit Leerzeichen.
    let kv = super::syntax::komponenten(w);
    matches!(super::syntax::getrimmt(&kv), [Kv::T(Tok::Ident(n))] if !n.eq_ignore_ascii_case("auto"))
}

fn linie_ende(start: &str) -> String {
    if ist_ident_linie(start) { start.to_string() } else { String::from("auto") }
}

fn flex(items: &[&Kv]) -> Option<Vec<String>> {
    if let [k] = items
        && k.ist_wort("none")
    {
        return Some(vec!["0".into(), "0".into(), "auto".into()]);
    }
    // Release 12.10.2026: `flex: 0` allein ist `0 1 0px` (wie Chrome; WPT
    // cssom flex-serialization).
    if let [Kv::T(Tok::Zahl(w, _, _))] = items
        && *w == 0.0
    {
        return Some(vec!["0".into(), "1".into(), "0px".into()]);
    }
    let zahl = |k: &Kv| -> Option<String> {
        match k {
            Kv::T(Tok::Zahl(..)) | Kv::F(..) => lv("flex-grow", &[k]),
            _ => None,
        }
    };
    let (mut g, mut s, mut b): (Option<String>, Option<String>, Option<String>) = (None, None, None);
    let mut i = 0;
    let mut zahlen_fertig = false;
    while i < items.len() {
        let k = items[i];
        if g.is_none() && !zahlen_fertig && let Some(z) = zahl(k) {
            g = Some(z);
            if let Some(z2) = items.get(i + 1).and_then(|k2| zahl(k2)) {
                // Eine einheitenlose 0 nach zwei Faktoren ist die Basis.
                s = Some(z2);
                i += 1;
            }
            zahlen_fertig = true;
            i += 1;
            continue;
        }
        if b.is_none() {
            if matches!(k, Kv::T(Tok::Zahl(w, _, _)) if *w != 0.0 || g.is_none() || s.is_none()) {
                return None;
            }
            let w = lv("flex-basis", &[k])?;
            b = Some(w);
            if g.is_some() {
                zahlen_fertig = true;
            }
            i += 1;
            continue;
        }
        return None;
    }
    if g.is_none() && b.is_none() {
        return None;
    }
    let basis_fehlt = b.is_none();
    Some(vec![
        g.unwrap_or_else(|| "1".into()),
        s.unwrap_or_else(|| "1".into()),
        b.unwrap_or_else(|| if basis_fehlt { "0%".into() } else { "auto".into() }),
    ])
}

fn grid_linien(items: &[&Kv], n: usize) -> Option<Vec<String>> {
    let teile = teilen(items, ist_strich);
    if teile.len() > n || teile.iter().any(Vec::is_empty) {
        return None;
    }
    let mut v: Vec<String> = Vec::new();
    for t in &teile {
        v.push(lv("grid-row-start", t)?);
    }
    Some(v)
}

fn grid_area(items: &[&Kv]) -> Option<Vec<String>> {
    let v = grid_linien(items, 4)?;
    let rs = v[0].clone();
    let cs = v.get(1).cloned().unwrap_or_else(|| linie_ende(&rs));
    let re = v.get(2).cloned().unwrap_or_else(|| linie_ende(&rs));
    let ce = v.get(3).cloned().unwrap_or_else(|| linie_ende(&cs));
    Some(vec![rs, cs, re, ce])
}

fn grid_area_text(v: &[String]) -> String {
    let mut t: Vec<&str> = v.iter().map(String::as_str).collect();
    if t[3] == linie_ende(t[1]) {
        t.pop();
        if t[2] == linie_ende(t[0]) {
            t.pop();
            if t[1] == linie_ende(t[0]) {
                t.pop();
            }
        }
    }
    t.join(" / ")
}

fn radius(items: &[&Kv]) -> Option<Vec<String>> {
    let teile = teilen(items, ist_strich);
    if teile.len() > 2 || teile.iter().any(|t| t.is_empty() || t.len() > 4) {
        return None;
    }
    let h = kasten(&["border-top-left-radius"], &teile[0])?;
    let v = match teile.get(1) {
        Some(t) => kasten(&["border-top-left-radius"], t)?,
        None => h.clone(),
    };
    Some((0..4).map(|i| if h[i] == v[i] { h[i].clone() } else { format!("{} {}", h[i], v[i]) }).collect())
}

/// Release 12.10.2026: Leerraum-getrennte Teile auf oberster Ebene
/// (`calc(1% + 1px) 2px` -> zwei Teile; WPT getComputedStyle-border-radius-003).
fn worte(x: &str) -> Vec<String> {
    let mut raus = Vec::new();
    let mut tiefe = 0i32;
    let mut akt = String::new();
    for ch in x.chars() {
        match ch {
            '(' => tiefe += 1,
            ')' => tiefe -= 1,
            ' ' if tiefe == 0 => {
                if !akt.is_empty() {
                    raus.push(std::mem::take(&mut akt));
                }
                continue;
            }
            _ => {}
        }
        akt.push(ch);
    }
    if !akt.is_empty() {
        raus.push(akt);
    }
    raus
}

fn radius_text(v: &[String]) -> Option<String> {
    let mut h = Vec::new();
    let mut w = Vec::new();
    for x in v {
        let wt = worte(x);
        let mut t = wt.iter();
        let a = t.next()?.to_string();
        let b = t.next().map_or_else(|| a.clone(), String::to_string);
        h.push(a);
        w.push(b);
    }
    let ht = kasten_text(&h);
    if h == w {
        return Some(ht);
    }
    Some(format!("{} / {}", ht, kasten_text(&w)))
}

fn border_image(items: &[&Kv]) -> Option<Vec<String>> {
    let teile = teilen(items, ist_strich);
    if teile.len() > 3 {
        return None;
    }
    // Quelle und Wiederholung koennen vor oder nach der Scheibe stehen; die
    // Scheibe steht direkt vor dem ersten `/`.
    let (mut quelle, mut wdh, mut scheibe): (Option<String>, Option<String>, Option<String>) = (None, None, None);
    let erster = &teile[0];
    let mut i = 0;
    let mut scheibe_ende = false;
    while i < erster.len() {
        let k = erster[i];
        if quelle.is_none() && let Some(q) = lv("border-image-source", &[k]) {
            quelle = Some(q);
            i += 1;
            if scheibe.is_some() {
                scheibe_ende = true;
            }
            continue;
        }
        if wdh.is_none() {
            let mut e = (i + 2).min(erster.len());
            let mut gefunden = None;
            while e > i {
                if let Some(w) = lv("border-image-repeat", &erster[i..e]) {
                    gefunden = Some((w, e));
                    break;
                }
                e -= 1;
            }
            if let Some((w, e)) = gefunden {
                wdh = Some(w);
                i = e;
                if scheibe.is_some() {
                    scheibe_ende = true;
                }
                continue;
            }
        }
        if scheibe.is_none() && !scheibe_ende {
            let mut e = erster.len();
            let mut gefunden = None;
            while e > i {
                if let Some(w) = lv("border-image-slice", &erster[i..e]) {
                    gefunden = Some((w, e));
                    break;
                }
                e -= 1;
            }
            let (w, e) = gefunden?;
            scheibe = Some(w);
            i = e;
            continue;
        }
        return None;
    }
    if teile.len() > 1 && (scheibe.is_none() || scheibe_ende) {
        return None;
    }
    let mut breite = None;
    let mut versatz = None;
    for (j, t) in teile.iter().enumerate().skip(1) {
        let letzte = j == teile.len() - 1;
        let mut t: Vec<&Kv> = t.clone();
        if letzte {
            // Nachgestellte Wiederholung/Quelle abtrennen.
            while let Some(k) = t.last().copied() {
                if wdh.is_none() && lv("border-image-repeat", &[k]).is_some() {
                    let mut e = t.len() - 1;
                    if e > 0 && lv("border-image-repeat", &t[e - 1..]).is_some() {
                        e -= 1;
                    }
                    wdh = lv("border-image-repeat", &t[e..]);
                    t.truncate(e);
                } else if quelle.is_none() && lv("border-image-source", &[k]).is_some() {
                    quelle = lv("border-image-source", &[k]);
                    t.pop();
                } else {
                    break;
                }
            }
        }
        if j == 1 {
            if t.is_empty() {
                if teile.len() == 2 {
                    return None;
                }
                continue;
            }
            breite = Some(lv("border-image-width", &t)?);
        } else {
            if t.is_empty() {
                return None;
            }
            versatz = Some(lv("border-image-outset", &t)?);
        }
    }
    if quelle.is_none() && wdh.is_none() && scheibe.is_none() {
        return None;
    }
    Some(vec![
        quelle.unwrap_or_else(|| anfang("border-image-source")),
        scheibe.unwrap_or_else(|| anfang("border-image-slice")),
        breite.unwrap_or_else(|| anfang("border-image-width")),
        versatz.unwrap_or_else(|| anfang("border-image-outset")),
        wdh.unwrap_or_else(|| anfang("border-image-repeat")),
    ])
}

fn border_image_text(v: &[String]) -> String {
    let d = ["none", "100%", "1", "0", "stretch"];
    if v.iter().zip(d).all(|(a, b)| a == b) {
        return String::from("none");
    }
    let mut t: Vec<String> = Vec::new();
    if v[0] != d[0] {
        t.push(v[0].clone());
    }
    let b = v[2] != d[2];
    let o = v[3] != d[3];
    if v[1] != d[1] || b || o {
        t.push(v[1].clone());
    }
    if b || o {
        t.push(String::from("/"));
        if b {
            t.push(v[2].clone());
        }
    }
    if o {
        t.push(String::from("/"));
        t.push(v[3].clone());
    }
    if v[4] != d[4] {
        t.push(v[4].clone());
    }
    t.join(" ")
}

const PLACE_BASELINE: &[&str] = &["baseline", "first baseline", "last baseline"];

fn text_wrap_text(v: &[String]) -> String {
    match (v[0].as_str(), v[1].as_str()) {
        (m, "auto") => m.to_string(),
        ("wrap", s) => s.to_string(),
        (m, s) => format!("{m} {s}"),
    }
}

fn white_space(items: &[&Kv]) -> Option<Vec<String>> {
    if let [k] = items {
        let w = k.ident().map(str::to_ascii_lowercase);
        let p = match w.as_deref() {
            Some("normal") => Some(("collapse", "wrap")),
            Some("pre") => Some(("preserve", "nowrap")),
            Some("pre-wrap") => Some(("preserve", "wrap")),
            Some("pre-line") => Some(("preserve-breaks", "wrap")),
            _ => None,
        };
        if let Some((a, b)) = p {
            return Some(vec![a.into(), b.into()]);
        }
    }
    einige(&["white-space-collapse", "text-wrap-mode"], items)
}

fn white_space_text(v: &[String]) -> String {
    match (v[0].as_str(), v[1].as_str()) {
        ("collapse", "wrap") => "normal".into(),
        ("preserve", "nowrap") => "pre".into(),
        ("preserve", "wrap") => "pre-wrap".into(),
        ("preserve-breaks", "wrap") => "pre-line".into(),
        ("collapse", m) => m.to_string(),
        (c, "wrap") => c.to_string(),
        (c, m) => format!("{c} {m}"),
    }
}

fn text_spacing(items: &[&Kv]) -> Option<Vec<String>> {
    if let [k] = items {
        if k.ist_wort("none") {
            return Some(vec!["space-all".into(), "no-autospace".into()]);
        }
        if k.ist_wort("auto") {
            return Some(vec!["auto".into(), "auto".into()]);
        }
    }
    // `normal` fuellt einen fehlenden Teil (beide sind vorgegeben `normal`);
    // hoechstens so oft, wie Teile fehlen.
    let mut trim: Option<String> = None;
    let mut auto: Option<String> = None;
    let mut normals = 0;
    let mut i = 0;
    while i < items.len() {
        let k = items[i];
        if k.ist_wort("normal") {
            normals += 1;
            i += 1;
            continue;
        }
        if trim.is_none() && !k.ist_wort("auto") && let Some(w) = lv("text-spacing-trim", &[k]) {
            trim = Some(w);
            i += 1;
            continue;
        }
        if auto.is_none() {
            let mut e = items.len();
            let mut gefunden = None;
            while e > i {
                if items[i..e].iter().all(|k| !k.ist_wort("auto") && !k.ist_wort("normal"))
                    && let Some(w) = lv("text-autospace", &items[i..e])
                {
                    gefunden = Some((w, e));
                    break;
                }
                e -= 1;
            }
            let (w, e) = gefunden?;
            auto = Some(w);
            i = e;
            continue;
        }
        return None;
    }
    let fehlend = usize::from(trim.is_none()) + usize::from(auto.is_none());
    if normals > fehlend {
        return None;
    }
    Some(vec![trim.unwrap_or_else(|| "normal".into()), auto.unwrap_or_else(|| "normal".into())])
}

fn text_spacing_text(v: &[String]) -> String {
    match (v[0].as_str(), v[1].as_str()) {
        ("space-all", "no-autospace") => "none".into(),
        ("auto", "auto") => "auto".into(),
        ("normal", "normal") => "normal".into(),
        (t, "normal") => t.to_string(),
        ("normal", a) => a.to_string(),
        (t, a) => format!("{t} {a}"),
    }
}

fn list_style(items: &[&Kv]) -> Option<Vec<String>> {
    // `none` passt auf Bild und Art: erst ohne `none` lesen, dann verteilen.
    let nones = items.iter().filter(|k| k.ist_wort("none")).count();
    let rest: Vec<&Kv> = items.iter().copied().filter(|k| !k.ist_wort("none")).collect();
    let mut v = if rest.is_empty() {
        vec![anfang("list-style-position"), anfang("list-style-image"), anfang("list-style-type")]
    } else {
        einige(&["list-style-position", "list-style-image", "list-style-type"], &rest)?
    };
    let bild_gesetzt = v[1] != "none";
    let art_gesetzt = rest.iter().any(|k| lv("list-style-type", &[k]).is_some_and(|w| w == v[2]) && lv("list-style-position", &[k]).is_none());
    match nones {
        0 => {}
        1 => {
            if !bild_gesetzt {
                v[1] = "none".into();
                if !art_gesetzt {
                    v[2] = "none".into();
                }
            } else if !art_gesetzt {
                v[2] = "none".into();
            } else {
                return None;
            }
        }
        2 => {
            if bild_gesetzt || art_gesetzt {
                return None;
            }
            v[1] = "none".into();
            v[2] = "none".into();
        }
        _ => return None,
    }
    Some(v)
}

fn list_style_text(v: &[String]) -> String {
    let mut t: Vec<&str> = Vec::new();
    if v[0] != "outside" {
        t.push(&v[0]);
    }
    if v[1] != "none" {
        t.push(&v[1]);
    }
    if v[2] != "disc" {
        t.push(&v[2]);
    }
    if t.is_empty() {
        return String::from("outside");
    }
    if v[1] == "none" && v[2] == "none" {
        // `none` steht dann fuer beide.
        t.retain(|x| *x != "none");
        t.push("none");
    }
    t.join(" ")
}

/// Schichten einer Langform als Texte (Komma-Liste).
fn schicht_texte(w: &str) -> Vec<String> {
    let kv = super::syntax::komponenten(w);
    let mut raus = vec![String::new()];
    let mut teil: Vec<Kv> = Vec::new();
    for k in kv {
        if k.ist_komma() {
            raus.push(String::new());
            let n = raus.len();
            raus[n - 2] = super::syntax::kv_text(&teil);
            teil.clear();
        } else {
            teil.push(k);
        }
    }
    let n = raus.len();
    raus[n - 1] = super::syntax::kv_text(&teil);
    raus
}

fn background(items: &[&Kv]) -> Option<Vec<String>> {
    let schichten = teilen(items, ist_komma);
    let n = schichten.len();
    let mut spalten: Vec<Vec<String>> = vec![Vec::new(); 9];
    for (si, sch) in schichten.iter().enumerate() {
        if sch.is_empty() {
            return None;
        }
        let letzte = si == n - 1;
        let (mut bild, mut pos, mut groesse, mut wdh, mut anh, mut farbe) = (None, None::<(String, String)>, None, None, None, None);
        let mut boxen: Vec<String> = Vec::new();
        let mut i = 0;
        while i < sch.len() {
            let k = sch[i];
            if bild.is_none() && let Some(w) = lv("background-image", &[k]) {
                bild = Some(w);
                i += 1;
                continue;
            }
            if pos.is_none() {
                let erg = super::sonder::position(sch, i, true);
                if let Some((e, st)) = erg.into_iter().next() {
                    let S::Pos(p) = &st[0] else { return None };
                    let (h, v) = (werte::text(&p.0), werte::text(&p.1));
                    i = e;
                    pos = Some((h, v));
                    if sch.get(i).is_some_and(|k| k.ist_delim('/')) {
                        // `/ <bg-size>` (ein oder zwei Werte)
                        let mut gefunden = None;
                        for e2 in [(i + 3).min(sch.len()), (i + 2).min(sch.len())] {
                            if e2 > i + 1 && let Some(w) = lv("background-size", &sch[i + 1..e2]) {
                                gefunden = Some((w, e2));
                                break;
                            }
                        }
                        let (w, e2) = gefunden?;
                        groesse = Some(w);
                        i = e2;
                    }
                    continue;
                }
            }
            if wdh.is_none() {
                let mut gefunden = None;
                for e2 in [(i + 2).min(sch.len()), i + 1] {
                    if let Some(w) = lv("background-repeat", &sch[i..e2]) {
                        gefunden = Some((w, e2));
                        break;
                    }
                }
                if let Some((w, e2)) = gefunden {
                    wdh = Some(w);
                    i = e2;
                    continue;
                }
            }
            if anh.is_none() && let Some(w) = lv("background-attachment", &[k]) {
                anh = Some(w);
                i += 1;
                continue;
            }
            // `border-area text` ist EIN clip-Wert.
            if boxen.len() < 2
                && sch.get(i + 1).is_some()
                && lv("background-origin", &[k]).is_none()
                && let Some(w) = lv("background-clip", &sch[i..i + 2])
            {
                boxen.push(w);
                i += 2;
                continue;
            }
            if boxen.len() < 2 && let Some(w) = lv("background-clip", &[k]) {
                if boxen.is_empty() || lv("background-origin", &[k]).is_some() || boxen.len() == 1 {
                    boxen.push(w);
                    i += 1;
                    continue;
                }
            }
            if letzte && farbe.is_none() && let Some(w) = lv("background-color", &[k]) {
                farbe = Some(w);
                i += 1;
                continue;
            }
            return None;
        }
        // Eine Box gilt fuer origin und clip; `border-area`/`text` nur fuer clip.
        let (origin, clip) = match boxen.as_slice() {
            [] => (anfang("background-origin"), anfang("background-clip")),
            [a] if !a.contains(' ') && lv("background-origin", &[&Kv::T(Tok::Ident(a.clone()))]).is_some() => (a.clone(), a.clone()),
            [a] => (String::from("border-box"), a.clone()),
            [a, b] => {
                let ist_origin = |x: &String| !x.contains(' ') && lv("background-origin", &[&Kv::T(Tok::Ident(x.clone()))]).is_some();
                if ist_origin(a) {
                    (a.clone(), b.clone())
                } else if ist_origin(b) {
                    // `border-area border-box`: border-area ist nur clip
                    // (WPT background-shorthand-serialization).
                    (b.clone(), a.clone())
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        let (px, py) = pos.unwrap_or_else(|| (String::from("0%"), String::from("0%")));
        let bild_da = bild.is_some();
        let werte_ = [
            bild.unwrap_or_else(|| anfang("background-image")),
            px,
            py,
            groesse.unwrap_or_else(|| anfang("background-size")),
            wdh.unwrap_or_else(|| anfang("background-repeat")),
            anh.unwrap_or_else(|| anfang("background-attachment")),
            origin,
            clip,
        ];
        for (j, w) in werte_.into_iter().enumerate() {
            spalten[j].push(w);
        }
        if letzte {
            // Die Kurzform setzt den Anfangswert als Farbe (wie Chrome; WPT
            // background-valid: `background: none` -> `rgba(0, 0, 0, 0)`, ohne
            // Bild `border-box content-box` -> `transparent`).
            let vorgabe = if bild_da { "rgba(0, 0, 0, 0)" } else { "transparent" };
            spalten[8].push(farbe.unwrap_or_else(|| String::from(vorgabe)));
        }
    }
    let mut raus: Vec<String> = spalten.iter().take(8).map(|s| s.join(", ")).collect();
    raus.push(spalten[8].first().cloned().unwrap_or_else(|| anfang("background-color")));
    Some(raus)
}

fn background_text(v: &[String]) -> Option<String> {
    let sp: Vec<Vec<String>> = v.iter().take(8).map(|w| schicht_texte(w)).collect();
    let n = sp[0].len();
    if sp.iter().any(|s| s.len() != n) {
        return None;
    }
    let mut schichten = Vec::new();
    for i in 0..n {
        let g = |j: usize| sp[j][i].as_str();
        let mut t: Vec<String> = Vec::new();
        if g(0) != "none" {
            t.push(g(0).to_string());
        }
        let pos_default = g(1) == "0%" && g(2) == "0%";
        if !pos_default || g(3) != "auto" {
            t.push(format!("{} {}", g(1), g(2)));
        }
        if g(3) != "auto" {
            t.push(format!("/ {}", g(3)));
        }
        if g(4) != "repeat" {
            t.push(g(4).to_string());
        }
        if g(5) != "scroll" {
            t.push(g(5).to_string());
        }
        if g(6) != "padding-box" || g(7) != "border-box" {
            let nur_clip = g(7).contains("border-area") || g(7) == "text";
            if g(6) == g(7) || (nur_clip && g(6) == "border-box") {
                t.push(g(7).to_string());
            } else {
                t.push(g(6).to_string());
                t.push(g(7).to_string());
            }
        }
        if i == n - 1 && v[8] != "transparent" && v[8] != "rgba(0, 0, 0, 0)" {
            t.push(v[8].clone());
        }
        if t.is_empty() {
            t.push(String::from("none"));
        }
        schichten.push(t.join(" "));
    }
    Some(schichten.join(", "))
}

fn bg_position(items: &[&Kv]) -> Option<Vec<String>> {
    let s = lang_lesen_items("background-position", items)?;
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for p in s {
        match p {
            S::Pos(b) => {
                xs.push(werte::text(&b.0));
                ys.push(werte::text(&b.1));
            }
            S::Komma => {}
            _ => return None,
        }
    }
    Some(vec![xs.join(", "), ys.join(", ")])
}

fn bg_position_text(v: &[String]) -> Option<String> {
    let x = schicht_texte(&v[0]);
    let y = schicht_texte(&v[1]);
    if x.len() != y.len() {
        return None;
    }
    Some(x.iter().zip(&y).map(|(a, b)| format!("{a} {b}")).collect::<Vec<_>>().join(", "))
}

fn liste_gruppen(lang: &[&str], items: &[&Kv], zuweisen: impl Fn(&[&Kv]) -> Option<Vec<Option<String>>>) -> Option<Vec<String>> {
    let schichten = teilen(items, ist_komma);
    let mut spalten: Vec<Vec<String>> = vec![Vec::new(); lang.len()];
    for sch in &schichten {
        let w = zuweisen(sch)?;
        for (j, x) in w.into_iter().enumerate() {
            spalten[j].push(x.unwrap_or_else(|| {
                let a = anfang(lang[j]);
                if lang[j] == "animation-duration" { String::from("auto") } else { a }
            }));
        }
    }
    Some(spalten.into_iter().map(|s| s.join(", ")).collect())
}

fn transition(items: &[&Kv]) -> Option<Vec<String>> {
    let lang = ["transition-property", "transition-duration", "transition-timing-function", "transition-delay", "transition-behavior"];
    let n = teilen(items, ist_komma).len();
    liste_gruppen(&lang, items, |sch| {
        let mut w: Vec<Option<String>> = vec![None; 5];
        let mut i = 0;
        while i < sch.len() {
            let k = sch[i];
            let mut weiter = false;
            // Zeiten: die erste ist die Dauer, die zweite die Verzoegerung.
            if let Some(z) = lv("transition-delay", &[k]) {
                if w[1].is_none() && lv("transition-duration", &[k]).is_some() {
                    w[1] = Some(z);
                } else if w[3].is_none() {
                    w[3] = Some(z);
                } else {
                    return None;
                }
                i += 1;
                continue;
            }
            if w[2].is_none() && let Some(z) = lv("transition-timing-function", &[k]) {
                w[2] = Some(z);
                weiter = true;
            } else if w[4].is_none() && let Some(z) = lv("transition-behavior", &[k]) {
                w[4] = Some(z);
                weiter = true;
            } else if w[0].is_none() {
                if k.ist_wort("none") && n > 1 {
                    return None;
                }
                let z = if k.ist_wort("none") { Some(String::from("none")) } else { lv("transition-property", &[k]) };
                if let Some(z) = z {
                    w[0] = Some(z);
                    weiter = true;
                }
            }
            if !weiter {
                return None;
            }
            i += 1;
        }
        Some(w)
    })
}

fn transition_text(v: &[String]) -> Option<String> {
    let sp: Vec<Vec<String>> = v.iter().map(|w| schicht_texte(w)).collect();
    let n = sp[0].len();
    if sp.iter().any(|s| s.len() != n) {
        return None;
    }
    let mut raus = Vec::new();
    for i in 0..n {
        let g = |j: usize| sp[j][i].as_str();
        let mut t: Vec<&str> = Vec::new();
        if g(0) != "all" {
            t.push(g(0));
        }
        if g(1) != "0s" || g(3) != "0s" {
            t.push(g(1));
        }
        if g(2) != "ease" {
            t.push(g(2));
        }
        if g(3) != "0s" {
            t.push(g(3));
        }
        if g(4) != "normal" {
            t.push(g(4));
        }
        if t.is_empty() {
            t.push("all");
        }
        raus.push(t.join(" "));
    }
    Some(raus.join(", "))
}

fn animation(items: &[&Kv]) -> Option<Vec<String>> {
    let lang = [
        "animation-duration",
        "animation-timing-function",
        "animation-delay",
        "animation-iteration-count",
        "animation-direction",
        "animation-fill-mode",
        "animation-play-state",
        "animation-name",
    ];
    liste_gruppen(&lang, items, |sch| {
        let mut w: Vec<Option<String>> = vec![None; 8];
        for k in sch {
            if let Some(z) = lv("animation-delay", &[k]) {
                if w[0].is_none() && lv("animation-duration", &[k]).is_some() {
                    w[0] = Some(z);
                } else if w[2].is_none() {
                    w[2] = Some(z);
                } else {
                    return None;
                }
                continue;
            }
            let mut ok = false;
            for j in [1usize, 3, 4, 5, 6, 7] {
                if w[j].is_none() && let Some(z) = lv(lang[j], &[k]) {
                    w[j] = Some(z);
                    ok = true;
                    break;
                }
            }
            if !ok {
                return None;
            }
        }
        Some(w)
    })
}

/// Release 15.10.2026: the timeline range name of a range start or end
/// (`entry 10%` -> `entry`).
fn range_name(v: &str) -> Option<&str> {
    let w = v.split_ascii_whitespace().next()?;
    matches!(w, "cover" | "contain" | "entry" | "exit" | "entry-crossing" | "exit-crossing").then_some(w)
}

/// `animation-range` (Scroll-driven Animations 1 §4.1): a start and an
/// optional end per layer; an omitted end is the start's range name (at
/// 100%), or `normal`.
fn animation_range(items: &[&Kv]) -> Option<Vec<String>> {
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for sch in teilen(items, ist_komma) {
        if sch.is_empty() || sch.len() > 4 {
            return None;
        }
        let mut found = None;
        for n in (1..=sch.len().min(2)).rev() {
            let Some(s) = lv("animation-range-start", &sch[..n]) else { continue };
            let rest = &sch[n..];
            if rest.is_empty() {
                found = Some((s, None));
                break;
            }
            if let Some(e) = lv("animation-range-end", rest) {
                found = Some((s, Some(e)));
                break;
            }
        }
        let (s, e) = found?;
        let e = e.unwrap_or_else(|| range_name(&s).map_or_else(|| String::from("normal"), str::to_string));
        starts.push(s);
        ends.push(e);
    }
    Some(vec![starts.join(", "), ends.join(", ")])
}

/// The end is left out where it is the one the start implies.
fn animation_range_text(v: &[String]) -> Option<String> {
    let s = schicht_texte(&v[0]);
    let e = schicht_texte(&v[1]);
    if s.len() != e.len() {
        return None;
    }
    let teile: Vec<String> = s
        .iter()
        .zip(&e)
        .map(|(start, end)| {
            // Computed values come with their offsets (`calc(0%)` -> `0%`).
            let start = match (range_name(start), start.strip_suffix(" 0%")) {
                (Some(_), Some(s)) => s.to_string(),
                _ => start.clone(),
            };
            let end = match (range_name(end), end.strip_suffix(" 100%")) {
                (Some(_), Some(e)) => e.to_string(),
                _ => end.clone(),
            };
            let (start, end) = (&start, &end);
            let weg = match range_name(start) {
                Some(n) => end == n,
                None => end == "normal" || end == "100%",
            };
            if weg { start.clone() } else { format!("{start} {end}") }
        })
        .collect();
    Some(teile.join(", "))
}

fn animation_text(v: &[String]) -> Option<String> {
    let sp: Vec<Vec<String>> = v.iter().map(|w| schicht_texte(w)).collect();
    let n = sp[0].len();
    if sp.iter().any(|s| s.len() != n) {
        return None;
    }
    let d = ["auto", "ease", "0s", "1", "normal", "none", "running", "none"];
    let mut raus = Vec::new();
    for i in 0..n {
        let g = |j: usize| sp[j][i].as_str();
        let mut t: Vec<&str> = Vec::new();
        if g(0) != d[0] || g(2) != d[2] {
            t.push(g(0));
        }
        // Release 15.10.2026: a name that reads as a keyword of another
        // longhand keeps that longhand in front of it, so the text parses
        // back the same (CSS Animations 1 §3.11; WPT css-animations
        // style-animation-parsing: `none forwards`, `normal normal`).
        let name = g(7).to_ascii_lowercase();
        let verwechselbar = |j: usize| match j {
            1 => matches!(name.as_str(), "ease" | "linear" | "ease-in" | "ease-out" | "ease-in-out" | "step-start" | "step-end"),
            3 => name == "infinite",
            4 => matches!(name.as_str(), "normal" | "reverse" | "alternate" | "alternate-reverse"),
            5 => matches!(name.as_str(), "forwards" | "backwards" | "both"),
            6 => matches!(name.as_str(), "running" | "paused"),
            _ => false,
        };
        for j in 1..8 {
            if g(j) != d[j] || verwechselbar(j) {
                t.push(g(j));
            }
        }
        if t.is_empty() {
            t.push("none");
        }
        raus.push(t.join(" "));
    }
    Some(raus.join(", "))
}

fn font(items: &[&Kv]) -> Option<Vec<String>> {
    if let [k] = items
        && let Some(w) = k.ident()
        && ["caption", "icon", "menu", "message-box", "small-caption", "status-bar"].iter().any(|s| w.eq_ignore_ascii_case(s))
    {
        return None;
    }
    let mut w: [Option<String>; 4] = [None, None, None, None];
    let mut i = 0;
    let mut normals = 0;
    // Stil, Variante, Gewicht, Breite in beliebiger Folge bis zur Groesse.
    while i < items.len() {
        let k = items[i];
        if k.ist_wort("normal") {
            normals += 1;
            i += 1;
            continue;
        }
        let mut ok = false;
        if w[0].is_none() {
            let mut e = (i + 2).min(items.len());
            while e > i {
                if let Some(z) = lv("font-style", &items[i..e]) {
                    w[0] = Some(z);
                    i = e;
                    ok = true;
                    break;
                }
                e -= 1;
            }
            if ok {
                continue;
            }
        }
        if w[1].is_none() && (k.ist_wort("small-caps")) {
            w[1] = Some(String::from("small-caps"));
            i += 1;
            continue;
        }
        if w[2].is_none() && lv("font-size", &[k]).is_none() && let Some(z) = lv("font-weight", &[k]) {
            w[2] = Some(z);
            i += 1;
            continue;
        }
        // In der Kurzform nur die Schluesselwoerter von font-stretch (CSS Fonts 4
        // §2.9 `<font-stretch-css3>`; WPT calc-percentage-position:
        // `font: calc(50%) serif` ist eine Groesse).
        if w[3].is_none() && k.ident().is_some() && let Some(z) = lv("font-stretch", &[k]) {
            w[3] = Some(z);
            i += 1;
            continue;
        }
        break;
    }
    let gesetzt = w.iter().filter(|x| x.is_some()).count();
    if gesetzt + normals > 4 {
        return None;
    }
    let groesse = lv("font-size", &[items.get(i)?])?;
    i += 1;
    let mut lh = String::from("normal");
    if items.get(i).is_some_and(|k| k.ist_delim('/')) {
        lh = lv("line-height", &[items.get(i + 1)?])?;
        i += 2;
    }
    let familie = lv("font-family", &items[i..])?;
    let mut v = vec![
        w[0].clone().unwrap_or_else(|| "normal".into()),
        w[1].clone().unwrap_or_else(|| "normal".into()),
        w[2].clone().unwrap_or_else(|| "normal".into()),
        w[3].clone().unwrap_or_else(|| "normal".into()),
        groesse,
        lh,
        familie,
    ];
    // `font` setzt die uebrigen Schrift-Langformen auf ihren Anfangswert
    // (CSS Fonts 4 §2.8).
    for l in &langformen("font")?[7..] {
        v.push(anfang(l));
    }
    Some(v)
}

/// `font-variant` (CSS Fonts 4 §6.11): jedes Wort gehoert zu genau einer
/// Langform; `none` setzt die Ligaturen auf `none`, `normal` alle auf `normal`.
fn font_variant(items: &[&Kv]) -> Option<Vec<String>> {
    let lang = langformen("font-variant")?;
    if let [k] = items {
        if k.ist_wort("normal") {
            return Some(lang.iter().map(|_| String::from("normal")).collect());
        }
        if k.ist_wort("none") {
            let mut v: Vec<String> = lang.iter().map(|_| String::from("normal")).collect();
            v[0] = String::from("none");
            return Some(v);
        }
    }
    let mut gruppen: Vec<Vec<&Kv>> = vec![Vec::new(); lang.len()];
    for k in items {
        if k.ist_wort("normal") || k.ist_wort("none") {
            return None;
        }
        let i = lang.iter().position(|l| lv(l, &[*k]).is_some())?;
        gruppen[i].push(*k);
    }
    let mut v = Vec::new();
    for (i, g) in gruppen.iter().enumerate() {
        v.push(if g.is_empty() { String::from("normal") } else { lv(lang[i], g)? });
    }
    Some(v)
}

fn font_variant_text(v: &[String]) -> Option<String> {
    if v.iter().all(|w| w == "normal") {
        return Some(String::from("normal"));
    }
    if v[0] == "none" {
        return v[1..].iter().all(|w| w == "normal").then(|| String::from("none"));
    }
    Some(v.iter().filter(|w| *w != "normal").cloned().collect::<Vec<_>>().join(" "))
}

fn font_text(v: &[String]) -> Option<String> {
    if v[1] != "normal" && v[1] != "small-caps" {
        return None;
    }
    let lang = langformen("font")?;
    if lang[7..].iter().zip(&v[7..]).any(|(l, w)| *w != anfang(l)) {
        return None;
    }
    if !(v[3] == "normal" || lv("font-stretch", &[&Kv::T(Tok::Ident(v[3].clone()))]).is_some()) {
        return None;
    }
    let mut t: Vec<String> = Vec::new();
    for (i, x) in v.iter().take(4).enumerate() {
        if x != "normal" {
            t.push(if i == 2 && x == "bold" { "bold".into() } else { x.clone() });
        }
    }
    t.push(if v[5] == "normal" { v[4].clone() } else { format!("{} / {}", v[4], v[5]) });
    t.push(v[6].clone());
    Some(t.join(" "))
}

/// Release 14.10.2026: `grid-lanes: none | [ <string> || <track-list> ] ||
/// <'grid-lanes-direction'>` (CSS Grid 3): the track list goes to the rows
/// with a `row` direction, else to the columns; the area names then stand
/// one per row. Without a direction it is `column`.
fn grid_lanes(items: &[&Kv]) -> Option<Vec<String>> {
    if let [k] = items
        && k.ist_wort("none")
    {
        return Some(vec![String::from("none"), String::from("none"), String::from("none"), String::from("column")]);
    }
    let mut strings: Vec<&str> = Vec::new();
    let mut direction: Vec<&Kv> = Vec::new();
    let mut tracks: Vec<&Kv> = Vec::new();
    for &k in items {
        match k {
            Kv::T(Tok::Str(s)) => strings.push(s),
            k if ["row", "column", "track-reverse", "fill-reverse", "normal"].iter().any(|w| k.ist_wort(w)) => direction.push(k),
            k => tracks.push(k),
        }
    }
    if strings.len() > 1 {
        return None;
    }
    let direction = if direction.is_empty() { String::from("column") } else { lv("grid-lanes-direction", &direction)? };
    let row = direction.starts_with("row");
    let track_list = if tracks.is_empty() {
        String::from("none")
    } else {
        lv(if row { "grid-template-rows" } else { "grid-template-columns" }, &tracks)?
    };
    let areas = match strings.first() {
        None => String::from("none"),
        Some(s) => {
            let names: Vec<&str> = s.split_whitespace().collect();
            if names.is_empty() {
                return None;
            }
            if row {
                names.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(" ")
            } else {
                format!("\"{}\"", names.join(" "))
            }
        }
    };
    let (columns, rows) = if row { (String::from("none"), track_list) } else { (track_list, String::from("none")) };
    Some(vec![columns, rows, areas, direction])
}

/// The strings of a `grid-template-areas` value.
fn area_strings(areas: &str) -> Vec<&str> {
    areas.split('"').skip(1).step_by(2).collect()
}

fn grid_lanes_text(v: &[String]) -> Option<String> {
    let (columns, rows, areas, direction) = (&v[0], &v[1], &v[2], &v[3]);
    let row = direction.starts_with("row");
    // The other axis is not part of `grid-lanes` (WPT
    // grid-lanes-shorthand-serialization: `normal 100px`, then
    // `grid-template-rows: 200px` stays `100px normal`); the direction is
    // always written.
    let tracks = if row { rows } else { columns };
    let mut parts: Vec<String> = Vec::new();
    if areas != "none" {
        let strings = area_strings(areas);
        if row {
            if strings.iter().any(|s| s.split_whitespace().count() != 1) {
                return None;
            }
            parts.push(format!("\"{}\"", strings.iter().map(|s| s.trim()).collect::<Vec<_>>().join(" ")));
        } else {
            if strings.len() != 1 {
                return None;
            }
            parts.push(areas.clone());
        }
    }
    if tracks != "none" {
        parts.push(tracks.clone());
    }
    parts.push(direction.clone());
    Some(parts.join(" "))
}

fn grid_template(items: &[&Kv]) -> Option<Vec<String>> {
    if let [k] = items
        && k.ist_wort("none")
    {
        return Some(vec!["none".into(), "none".into(), "none".into()]);
    }
    let teile = teilen(items, ist_strich);
    // Form 2: <rows> / <columns>
    if teile.len() == 2 && !teile[0].iter().any(|k| matches!(k, Kv::T(Tok::Str(_)))) {
        let r = lv("grid-template-rows", &teile[0])?;
        let c = lv("grid-template-columns", &teile[1])?;
        return Some(vec![r, c, "none".into()]);
    }
    if teile.len() > 2 {
        return None;
    }
    // Form 3: [ <line-names>? <string> <track-size>? <line-names>? ]+ [ / <explicit-track-list> ]?
    let zeilen_items = &teile[0];
    let mut zeilen: Vec<String> = Vec::new();
    let mut flaechen: Vec<Vec<String>> = Vec::new();
    let mut flaechen_text: Vec<String> = Vec::new();
    let mut i = 0;
    let mut namen_davor: Option<Vec<String>> = None;
    while i < zeilen_items.len() {
        let mut stueck: Vec<String> = Vec::new();
        if let Kv::B('[', _) = zeilen_items[i] {
            let n = lv_namen(zeilen_items[i])?;
            let mut alle = namen_davor.take().unwrap_or_default();
            alle.extend(n);
            // Release 12.10.2026: leere Liniennamen `[]` fallen weg (WPT
            // grid-template-shorthand: `[] auto [] 1fr []` -> `auto 1fr`).
            if !alle.is_empty() {
                stueck.push(format!("[{}]", alle.join(" ")));
            }
            i += 1;
        } else if let Some(n) = namen_davor.take().filter(|n| !n.is_empty()) {
            stueck.push(format!("[{}]", n.join(" ")));
        }
        let Some(Kv::T(Tok::Str(s))) = zeilen_items.get(i) else { return None };
        let z = super::sonder::areas_zeile(s)?;
        flaechen_text.push(super::syntax::text_text(&z.join(" ")));
        flaechen.push(z);
        i += 1;
        let mut groesse = String::from("auto");
        if let Some(k) = zeilen_items.get(i)
            && !matches!(k, Kv::B('[', _) | Kv::T(Tok::Str(_)))
        {
            if zeilen_items[i].funk().is_some_and(|(n, _)| n.eq_ignore_ascii_case("repeat")) {
                return None;
            }
            groesse = lv("grid-auto-rows", &[k])?;
            i += 1;
        }
        stueck.push(groesse);
        if let Some(Kv::B('[', _)) = zeilen_items.get(i) {
            let n = lv_namen(zeilen_items[i])?;
            i += 1;
            // Namen am Zeilenende verbinden sich mit denen der naechsten Zeile.
            if zeilen_items.get(i).is_some_and(|k| matches!(k, Kv::B('[', _))) {
                namen_davor = Some(n);
            } else if zeilen_items.get(i).is_some() {
                namen_davor = Some(n);
            } else if !n.is_empty() {
                stueck.push(format!("[{}]", n.join(" ")));
            }
        }
        zeilen.push(stueck.join(" "));
    }
    if let Some(n) = namen_davor.filter(|n| !n.is_empty()) {
        zeilen.push(format!("[{}]", n.join(" ")));
    }
    if flaechen.is_empty() || !super::sonder::areas_pruefen(&flaechen) {
        return None;
    }
    let spalten = match teile.get(1) {
        Some(t) => {
            let c = lv("grid-template-columns", t)?;
            if c == "none" || t.iter().any(|k| k.funk().is_some_and(|(n, a)| n.eq_ignore_ascii_case("repeat") && a.iter().any(|x| x.ist_wort("auto-fill") || x.ist_wort("auto-fit")))) {
                return None;
            }
            c
        }
        None => String::from("none"),
    };
    // Spalten und Flaechen muessen zusammenpassen? (Nein — nur die Zeilen.)
    let rows = zeilen.join(" ");
    let rows = rows.replace("] [", " ");
    Some(vec![rows, spalten, flaechen_text.join(" ")])
}

fn lv_namen(k: &Kv) -> Option<Vec<String>> {
    match lang_lesen_items("grid-template-rows", &[k, &Kv::T(Tok::Ident("auto".into()))])?.first()? {
        S::Namen(n) => Some(n.clone()),
        _ => None,
    }
}

fn grid_template_text(v: &[String]) -> Option<String> {
    let (rows, cols, areas) = (&v[0], &v[1], &v[2]);
    // Flaechen ohne Zeilen lassen sich nicht schreiben (WPT
    // grid-template-shorthand-areas-valid).
    if areas != "none" && rows == "none" {
        return None;
    }
    if areas == "none" {
        if rows == "none" && cols == "none" {
            return Some(String::from("none"));
        }
        // Leere Liniennamen fallen weg (`[] 10px []` -> `10px`).
        let ohne = |t: &str| {
            let kv = super::syntax::komponenten(t);
            let v: Vec<Kv> = kv.into_iter().filter(|k| !matches!(k, Kv::B('[', a) if a.iter().all(Kv::ist_leer))).collect();
            super::syntax::kv_text(&v)
        };
        return Some(format!("{} / {}", ohne(rows), ohne(cols)));
    }
    // Zeilen mit den Flaechen verflechten (leere `[]` fallen weg).
    let flaechen: Vec<String> = {
        let kv = super::syntax::komponenten(areas);
        kv.iter()
            .filter_map(|k| match k {
                Kv::T(Tok::Str(s)) => Some(super::syntax::text_text(s)),
                _ => None,
            })
            .collect()
    };
    let kv = super::syntax::komponenten(rows);
    let teile: Vec<&Kv> = kv.iter().filter(|k| !k.ist_leer()).collect();
    if teile.iter().any(|k| k.funk().is_some()) && teile.iter().any(|k| k.funk().is_some_and(|(n, _)| n.eq_ignore_ascii_case("repeat"))) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut fi = 0;
    let mut i = 0;
    while i < teile.len() {
        let k = teile[i];
        if let Kv::B('[', a) = k {
            if !a.iter().all(Kv::ist_leer) {
                out.push(super::syntax::kv_text(&[k.clone()]));
            }
            i += 1;
            continue;
        }
        let f = flaechen.get(fi)?;
        fi += 1;
        out.push(f.clone());
        let t = super::syntax::kv_text(&[k.clone()]);
        if t != "auto" {
            out.push(t);
        }
        i += 1;
    }
    if fi != flaechen.len() {
        return None;
    }
    let mut s = out.join(" ");
    if cols != "none" {
        s.push_str(" / ");
        s.push_str(cols);
    }
    Some(s)
}

fn grid(items: &[&Kv]) -> Option<Vec<String>> {
    if let Some(t) = grid_template(items) {
        return Some(vec![t[0].clone(), t[1].clone(), t[2].clone(), "auto".into(), "auto".into(), "row".into()]);
    }
    let teile = teilen(items, ist_strich);
    if teile.len() != 2 {
        return None;
    }
    let flow = |t: &[&Kv]| -> Option<(bool, usize)> {
        // [ auto-flow && dense? ] am Anfang; Rueckgabe (dense, Laenge)
        let mut af = false;
        let mut dense = false;
        let mut n = 0;
        for k in t.iter().take(2) {
            if k.ist_wort("auto-flow") && !af {
                af = true;
                n += 1;
            } else if k.ist_wort("dense") && !dense {
                dense = true;
                n += 1;
            } else {
                break;
            }
        }
        af.then_some((dense, n))
    };
    if let Some((dense, n)) = flow(&teile[0]) {
        let rows = if n == teile[0].len() { "auto".into() } else { lv("grid-auto-rows", &teile[0][n..])? };
        let cols = lv("grid-template-columns", &teile[1])?;
        return Some(vec!["none".into(), cols, "none".into(), rows, "auto".into(), if dense { "dense".into() } else { "row".into() }]);
    }
    if let Some((dense, n)) = flow(&teile[1]) {
        let rows = lv("grid-template-rows", &teile[0])?;
        let cols = if n == teile[1].len() { "auto".into() } else { lv("grid-auto-columns", &teile[1][n..])? };
        return Some(vec![rows, "none".into(), "none".into(), "auto".into(), cols, if dense { "column dense".into() } else { "column".into() }]);
    }
    None
}

fn grid_text(v: &[String]) -> Option<String> {
    let (ar, ac, af) = (&v[3], &v[4], &v[5]);
    if ar == "auto" && ac == "auto" && af == "row" {
        return grid_template_text(&v[..3]);
    }
    if v[2] != "none" {
        return None;
    }
    let dense = af.contains("dense");
    let spalte = af.starts_with("column");
    if spalte {
        if ar != "auto" || v[1] != "none" {
            return None;
        }
        let mut t = String::from("auto-flow");
        if dense {
            t.push_str(" dense");
        }
        if ac != "auto" {
            t.push(' ');
            t.push_str(ac);
        }
        return Some(format!("{} / {}", v[0], t));
    }
    if ac != "auto" || v[0] != "none" {
        return None;
    }
    let mut t = String::from("auto-flow");
    if dense {
        t.push_str(" dense");
    }
    if ar != "auto" {
        t.push(' ');
        t.push_str(ar);
    }
    Some(format!("{} / {}", t, v[1]))
}

fn line_clamp(items: &[&Kv]) -> Option<Vec<String>> {
    if let [k] = items
        && k.ist_wort("none")
    {
        return Some(vec!["none".into(), "no-ellipsis".into(), "auto".into()]);
    }
    let mut zahl = None;
    let mut ell = None;
    let mut legacy = false;
    for (i, k) in items.iter().enumerate() {
        if k.ist_wort("-webkit-legacy") && i == items.len() - 1 {
            legacy = true;
            continue;
        }
        if zahl.is_none() && let Some(z) = lv("max-lines", &[k]) {
            if z == "none" {
                return None;
            }
            zahl = Some(z);
            continue;
        }
        if ell.is_none() {
            if k.ist_wort("ellipsis") {
                ell = Some(String::from("auto"));
                continue;
            }
            if let Some(z) = lv("block-ellipsis", &[k]) {
                ell = Some(z);
                continue;
            }
        }
        return None;
    }
    if legacy && zahl.is_none() && ell.is_none() {
        return None;
    }
    if zahl.is_none() && ell.is_none() {
        return None;
    }
    let fort = if legacy { "-webkit-legacy" } else { "collapse" };
    Some(vec![zahl.unwrap_or_else(|| "none".into()), ell.unwrap_or_else(|| "auto".into()), fort.into()])
}

fn line_clamp_text(v: &[String]) -> Option<String> {
    match (v[0].as_str(), v[1].as_str(), v[2].as_str()) {
        ("none", "no-ellipsis", "auto") => Some("none".into()),
        (z, e, f) => {
            let mut t: Vec<&str> = Vec::new();
            if z != "none" {
                t.push(z);
            }
            if e != "auto" || z == "none" {
                t.push(e);
            }
            if f == "-webkit-legacy" {
                t.push(f);
            } else if f != "collapse" {
                return None;
            }
            Some(t.join(" "))
        }
    }
}

/// Eine Kurzform lesen: die Werte ihrer Langformen (Reihenfolge wie
/// [`langformen`]). `None` = ungueltig.
pub fn lesen(name: &str, kv: &[Kv]) -> Option<Vec<String>> {
    let lang = langformen(name)?;
    let items: Vec<&Kv> = kv.iter().filter(|k| !k.ist_leer()).collect();
    if items.is_empty() {
        return None;
    }
    let items = items.as_slice();
    match name {
        "margin" | "padding" | "inset" | "border-width" | "border-style" | "border-color" => kasten(lang, items),
        "margin-block" | "margin-inline" | "padding-block" | "padding-inline" | "inset-block" | "inset-inline" | "border-block-width"
        | "border-block-style" | "border-block-color" | "border-inline-width" | "border-inline-style" | "border-inline-color" => {
            if items.len() > 2 {
                return None;
            }
            let a = lv(lang[0], &items[..1])?;
            let b = match items.get(1) {
                Some(k) => lv(lang[1], &[*k])?,
                None => a.clone(),
            };
            Some(vec![a, b])
        }
        "overflow" | "overscroll-behavior" => {
            if items.len() > 2 {
                return None;
            }
            let a = lv(lang[0], &items[..1])?;
            let b = match items.get(1) {
                Some(k) => lv(lang[1], &[*k])?,
                None => a.clone(),
            };
            Some(vec![a, b])
        }
        "size" => {
            if items.len() > 2 {
                return None;
            }
            let a = lv("width", &items[..1])?;
            let b = match items.get(1) {
                Some(k) => lv("height", &[*k])?,
                None => a.clone(),
            };
            Some(vec![a, b])
        }
        // Release 12.10.2026: `contain-intrinsic-size` (CSS Sizing 4 §5.1): ein
        // Wert gilt fuer beide Achsen.
        "contain-intrinsic-size" => paar("contain-intrinsic-width", "contain-intrinsic-height", items, |a| Some(a.to_string())),
        "gap" | "grid-gap" => {
            if items.len() > 2 {
                return None;
            }
            let a = lv("row-gap", &items[..1])?;
            let b = match items.get(1) {
                Some(k) => lv("column-gap", &[*k])?,
                None => a.clone(),
            };
            Some(vec![a, b])
        }
        "place-content" => paar("align-content", "justify-content", items, |a| {
            if PLACE_BASELINE.contains(&a) { Some(String::from("start")) } else { lv("justify-content", items) }
        }),
        "place-items" => paar("align-items", "justify-items", items, |_| lv("justify-items", items)),
        "place-self" => paar("align-self", "justify-self", items, |_| lv("justify-self", items)),
        "border-top" | "border-right" | "border-bottom" | "border-left" | "border-block-start" | "border-block-end" | "border-inline-start"
        | "border-inline-end" | "outline" | "column-rule" | "columns" | "flex-flow" | "text-decoration" => einige(lang, items),
        "border" => {
            let v = einige(&["border-top-width", "border-top-style", "border-top-color"], items)?;
            let mut raus = Vec::new();
            for j in 0..3 {
                for _ in 0..4 {
                    raus.push(v[j].clone());
                }
            }
            for l in &lang[12..] {
                raus.push(anfang(l));
            }
            Some(raus)
        }
        "border-block" | "border-inline" => {
            let v = einige(&[lang[0], lang[2], lang[4]], items)?;
            Some(vec![v[0].clone(), v[0].clone(), v[1].clone(), v[1].clone(), v[2].clone(), v[2].clone()])
        }
        "border-radius" | "-webkit-border-radius" => {
            if name == "-webkit-border-radius" && items.len() == 2 && !items.iter().any(|k| k.ist_delim('/')) {
                // WebKit: zwei Werte ohne `/` sind waagerecht und senkrecht.
                let a = lv("border-top-left-radius", &items[..1])?;
                let b = lv("border-top-left-radius", &items[1..])?;
                let w = if a == b { a } else { format!("{a} {b}") };
                return Some(vec![w.clone(), w.clone(), w.clone(), w]);
            }
            radius(items)
        }
        "border-image" => border_image(items),
        "flex" => flex(items),
        "list-style" => list_style(items),
        "text-wrap" => einige(lang, items),
        "white-space" => white_space(items),
        "text-spacing" => text_spacing(items),
        "line-clamp" => line_clamp(items),
        "grid-row" | "grid-column" => {
            let v = grid_linien(items, 2)?;
            let e = v.get(1).cloned().unwrap_or_else(|| linie_ende(&v[0]));
            Some(vec![v[0].clone(), e])
        }
        "grid-area" => grid_area(items),
        "grid-template" => grid_template(items),
        "grid" => grid(items),
        "grid-lanes" => grid_lanes(items),
        "background-position" => bg_position(items),
        "background" => background(items),
        "transition" => transition(items),
        "animation" => animation(items).map(|mut v| {
            // The reset-only longhands take their initial values.
            v.extend([String::from("auto"), String::from("normal"), String::from("normal")]);
            v
        }),
        "animation-range" => animation_range(items),
        "font" => font(items),
        "font-variant" => font_variant(items),
        _ => None,
    }
}

/// Eine Kurzform aus den Werten ihrer Langformen schreiben. `None` = nicht
/// darstellbar.
pub fn schreiben(name: &str, v: &[String]) -> Option<String> {
    let lang = langformen(name)?;
    if v.len() != lang.len() {
        return None;
    }
    Some(match name {
        "margin" | "padding" | "inset" | "border-width" | "border-style" | "border-color" => kasten_text(v),
        "margin-block" | "margin-inline" | "padding-block" | "padding-inline" | "inset-block" | "inset-inline" | "border-block-width"
        | "border-block-style" | "border-block-color" | "border-inline-width" | "border-inline-style" | "border-inline-color"
        | "overflow" | "overscroll-behavior" | "gap" | "grid-gap" | "place-items" | "place-self" | "size"
        | "contain-intrinsic-size" => paar_text(v),
        "place-content" => {
            if PLACE_BASELINE.contains(&v[0].as_str()) && v[1] == "start" {
                format!("{} start", v[0])
            } else {
                paar_text(v)
            }
        }
        "border-top" | "border-right" | "border-bottom" | "border-left" | "border-block-start" | "border-block-end" | "border-inline-start"
        | "border-inline-end" | "outline" | "column-rule" | "columns" | "flex-flow" | "text-decoration" => einige_text(lang, v),
        "border" => {
            for j in 0..3 {
                if (1..4).any(|s| v[j * 4 + s] != v[j * 4]) {
                    return None;
                }
            }
            if lang[12..].iter().zip(&v[12..]).any(|(l, w)| *w != anfang(l)) {
                return None;
            }
            einige_text(&["border-top-width", "border-top-style", "border-top-color"], &[v[0].clone(), v[4].clone(), v[8].clone()])
        }
        "border-block" | "border-inline" => {
            if v[0] != v[1] || v[2] != v[3] || v[4] != v[5] {
                return None;
            }
            einige_text(&[lang[0], lang[2], lang[4]], &[v[0].clone(), v[2].clone(), v[4].clone()])
        }
        "border-radius" => radius_text(v)?,
        // Release 12.10.2026: zwei Werte ohne `/` liest `-webkit-border-radius`
        // als `a / b` — geschrieben werden sie darum dreifach (`a b a`), damit
        // der Text wieder dieselben Langformen ergibt (WPT
        // webkit-border-radius-valid).
        "-webkit-border-radius" => {
            let t = radius_text(v)?;
            let wt = worte(&t);
            let teile: Vec<&str> = wt.iter().map(String::as_str).collect();
            if teile.len() == 2 && !t.contains('/') { format!("{} {} {}", teile[0], teile[1], teile[0]) } else { t }
        }
        "border-image" => border_image_text(v),
        // `flex: content` bleibt so (WPT css-flexbox flex-valid).
        "flex" if v[0] == "1" && v[1] == "1" && v[2] == "content" => String::from("content"),
        "flex" => v.join(" "),
        "list-style" => list_style_text(v),
        "text-wrap" => text_wrap_text(v),
        "white-space" => white_space_text(v),
        "text-spacing" => text_spacing_text(v),
        "line-clamp" => line_clamp_text(v)?,
        "grid-row" | "grid-column" => {
            if v[1] == linie_ende(&v[0]) {
                v[0].clone()
            } else {
                format!("{} / {}", v[0], v[1])
            }
        }
        "grid-area" => grid_area_text(v),
        "grid-template" => grid_template_text(v)?,
        "grid" => grid_text(v)?,
        "grid-lanes" => grid_lanes_text(v)?,
        "background-position" => bg_position_text(v)?,
        "background" => background_text(v)?,
        "transition" => transition_text(v)?,
        // The reset-only longhands must have their initial values.
        "animation" if v.len() > 8 && (v[8] != "auto" || v[9] != "normal" || v[10] != "normal") => return None,
        "animation" => animation_text(&v[..8])?,
        "animation-range" => animation_range_text(v)?,
        "font" => font_text(v)?,
        "font-variant" => font_variant_text(v)?,
        _ => return None,
    })
}

/// Alle Kurzformen, zu denen `lang` gehoert — in bevorzugter Reihenfolge
/// (CSSOM §6.7.2: die mit den meisten Langformen zuerst).
pub fn kurzformen_von(lang: &str) -> Vec<&'static str> {
    let mut v: Vec<(&str, usize)> = KURZ
        .iter()
        .filter(|(n, l)| l.contains(&lang) && !matches!(*n, "-webkit-border-radius" | "grid-gap" | "size"))
        .map(|(n, l)| (*n, l.len()))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.into_iter().map(|(n, _)| n).collect()
}
