//! Release 12.10.2026: Werte, deren Grammatik oder Schreibweise sich nicht
//! in der Grammatik-Schreibweise ausdruecken laesst: `<position>` und
//! `<bg-position>` (CSS Values 4 §8.3, CSS Backgrounds 3 §3.6), `display`
//! (CSS Display 3 §2, kuerzeste Form), `grid-template-areas` (CSS Grid 2
//! §7.3, Rechtecke), `font-family` (CSS Fonts 4 §3.1).

use super::syntax::{Kv, Tok, ident_text, text_text};
use super::werte::{self, Art, S};

const H: &[&str] = &["left", "right", "x-start", "x-end"];
const V: &[&str] = &["top", "bottom", "y-start", "y-end"];

fn wort<'a>(k: &Kv, aus: &[&'a str]) -> Option<&'a str> {
    let n = k.ident()?;
    aus.iter().find(|w| n.eq_ignore_ascii_case(w)).copied()
}

fn lp(k: &Kv) -> Option<S> {
    werte::numerisch(k, Art::LaengeProzent, f64::NEG_INFINITY, f64::INFINITY).map(S::Z)
}

/// Eine Seite einer Position: Schluesselwort und/oder Versatz.
fn seite(w: Option<&str>, n: Option<S>) -> Vec<S> {
    let mut v = Vec::new();
    if let Some(w) = w {
        v.push(S::W(w.to_string()));
    }
    if let Some(n) = n {
        v.push(n);
    }
    v
}

/// Alle Lesarten einer Position ab `pos` (laengste zuerst): (Ende, Stueck).
pub fn position(items: &[&Kv], pos: usize, bg: bool) -> Vec<(usize, Vec<S>)> {
    let mut raus = Vec::new();
    let it = |i: usize| items.get(pos + i).copied();
    let hk = |k: &Kv| wort(k, H);
    let vk = |k: &Kv| wort(k, V);
    let mitte = |k: &Kv| k.ist_wort("center");
    let mut fertig = |n: usize, h: Vec<S>, v: Vec<S>| raus.push((pos + n, vec![S::Pos(Box::new((h, v)))]));
    // 4 Werte: [H lp] && [V lp]
    if let (Some(a), Some(b), Some(c), Some(d)) = (it(0), it(1), it(2), it(3)) {
        if let (Some(ha), Some(nb), Some(vc), Some(nd)) = (hk(a), lp(b), vk(c), lp(d)) {
            fertig(4, seite(Some(ha), Some(nb)), seite(Some(vc), Some(nd)));
        } else if let (Some(va), Some(nb), Some(hc), Some(nd)) = (vk(a), lp(b), hk(c), lp(d)) {
            fertig(4, seite(Some(hc), Some(nd)), seite(Some(va), Some(nb)));
        }
    }
    // 3 Werte (nur <bg-position>): eine Seite mit Versatz, die andere ein Wort.
    if bg && let (Some(a), Some(b), Some(c)) = (it(0), it(1), it(2)) {
        let hw = |k: &Kv| hk(k).or_else(|| mitte(k).then_some("center"));
        let vw = |k: &Kv| vk(k).or_else(|| mitte(k).then_some("center"));
        // H lp V | V lp H | H V lp | V H lp
        if let (Some(ha), Some(nb), Some(vc)) = (hk(a), lp(b), vw(c)) {
            fertig(3, seite(Some(ha), Some(nb)), seite(Some(vc), None));
        } else if let (Some(va), Some(nb), Some(hc)) = (vk(a), lp(b), hw(c)) {
            fertig(3, seite(Some(hc), None), seite(Some(va), Some(nb)));
        } else if let (Some(ha), Some(vb), Some(nc)) = (hw(a), vk(b), lp(c)) {
            fertig(3, seite(Some(ha), None), seite(Some(vb), Some(nc)));
        } else if let (Some(va), Some(hb), Some(nc)) = (vw(a), hk(b), lp(c)) {
            fertig(3, seite(Some(hb), Some(nc)), seite(Some(va), None));
        }
    }
    // 2 Werte
    if let (Some(a), Some(b)) = (it(0), it(1)) {
        let h_a = hk(a).map(|w| seite(Some(w), None)).or_else(|| mitte(a).then(|| seite(Some("center"), None))).or_else(|| lp(a).map(|n| vec![n]));
        let v_b = vk(b).map(|w| seite(Some(w), None)).or_else(|| mitte(b).then(|| seite(Some("center"), None))).or_else(|| lp(b).map(|n| vec![n]));
        if let (Some(h), Some(v)) = (h_a, v_b) {
            fertig(2, h, v);
        } else {
            let v_a = vk(a).or_else(|| mitte(a).then_some("center"));
            let h_b = hk(b).or_else(|| mitte(b).then_some("center"));
            if let (Some(v), Some(h)) = (v_a, h_b) {
                fertig(2, seite(Some(h), None), seite(Some(v), None));
            }
        }
    }
    // 1 Wert
    if let Some(a) = it(0) {
        if let Some(w) = hk(a) {
            fertig(1, seite(Some(w), None), seite(Some("center"), None));
        } else if let Some(w) = vk(a) {
            fertig(1, seite(Some("center"), None), seite(Some(w), None));
        } else if mitte(a) {
            fertig(1, seite(Some("center"), None), seite(Some("center"), None));
        } else if let Some(n) = lp(a) {
            fertig(1, vec![n], seite(Some("center"), None));
        }
    }
    raus
}

// ---------------------------------------------------------------------------
// display
// ---------------------------------------------------------------------------

/// `display` lesen und in der kuerzesten Form schreiben (CSS Display 3 §2,
/// wie Chrome): `block flow` -> `block`, `inline flow-root` -> `inline-block`.
pub fn display(items: &[&Kv]) -> Option<Vec<S>> {
    let w: Vec<String> = items.iter().map(|k| k.ident().map(str::to_ascii_lowercase)).collect::<Option<_>>()?;
    let ein = |s: &str| S::W(s.to_string());
    if w.len() == 1 {
        let x = w[0].as_str();
        if matches!(
            x,
            "contents" | "none" | "table-row-group" | "table-header-group" | "table-footer-group" | "table-row" | "table-cell"
                | "table-column-group" | "table-column" | "table-caption" | "ruby-base" | "ruby-text" | "ruby-base-container"
                | "ruby-text-container" | "inline-block" | "inline-table" | "inline-flex" | "inline-grid" | "inline-grid-lanes"
                | "-webkit-box" | "-webkit-inline-box" | "-webkit-flex" | "-webkit-inline-flex" | "math"
        ) {
            return Some(vec![ein(x)]);
        }
    }
    if w.is_empty() || w.len() > 3 {
        return None;
    }
    let (mut aussen, mut innen, mut liste) = (None::<&str>, None::<&str>, false);
    for x in &w {
        match x.as_str() {
            "block" | "inline" | "run-in" if aussen.is_none() => aussen = Some(x),
            "flow" | "flow-root" | "table" | "flex" | "grid" | "ruby" | "grid-lanes" if innen.is_none() => innen = Some(x),
            "list-item" if !liste => liste = true,
            _ => return None,
        }
    }
    if liste {
        if !matches!(innen, None | Some("flow" | "flow-root")) {
            return None;
        }
        let mut v = Vec::new();
        match aussen {
            Some("block") | None => {}
            Some(a) => v.push(ein(a)),
        }
        if innen == Some("flow-root") {
            v.push(ein("flow-root"));
        }
        v.push(ein("list-item"));
        return Some(v);
    }
    let innen = innen.unwrap_or("flow");
    let aussen = aussen.unwrap_or(if innen == "ruby" { "inline" } else { "block" });
    let t: Vec<&str> = match (aussen, innen) {
        ("block", "flow") => vec!["block"],
        ("block", "ruby") => vec!["block", "ruby"],
        ("block", i) => vec![i],
        ("inline", "flow") => vec!["inline"],
        ("inline", "flow-root") => vec!["inline-block"],
        ("inline", "table") => vec!["inline-table"],
        ("inline", "flex") => vec!["inline-flex"],
        ("inline", "grid") => vec!["inline-grid"],
        ("inline", "ruby") => vec!["ruby"],
        ("inline", i) => vec!["inline", i],
        ("run-in", "flow") => vec!["run-in"],
        (a, i) => vec![a, i],
    };
    Some(t.into_iter().map(ein).collect())
}

// ---------------------------------------------------------------------------
// grid-template-areas
// ---------------------------------------------------------------------------

/// Eine Zeile von `grid-template-areas` in Namen zerlegen (`.`-Folgen sind
/// eine leere Zelle); `None` bei Muell-Zeichen.
pub fn areas_zeile(s: &str) -> Option<Vec<String>> {
    let mut raus = Vec::new();
    let z: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < z.len() {
        let c = z[i];
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C') {
            i += 1;
        } else if c == '.' {
            while i < z.len() && z[i] == '.' {
                i += 1;
            }
            raus.push(String::from("."));
        } else if c.is_alphanumeric() || c == '-' || c == '_' || c as u32 >= 0x80 {
            let s = i;
            while i < z.len() && (z[i].is_alphanumeric() || z[i] == '-' || z[i] == '_' || z[i] as u32 >= 0x80) {
                i += 1;
            }
            raus.push(z[s..i].iter().collect());
        } else {
            return None;
        }
    }
    Some(raus)
}

/// Die Zeilen von `grid-template-areas` pruefen: gleich lang, Gebiete sind Rechtecke.
pub fn areas_pruefen(zeilen: &[Vec<String>]) -> bool {
    let breite = zeilen.first().map_or(0, Vec::len);
    if breite == 0 || zeilen.iter().any(|z| z.len() != breite) {
        return false;
    }
    let mut namen: Vec<&str> = Vec::new();
    for z in zeilen {
        for n in z {
            if n != "." && !namen.contains(&n.as_str()) {
                namen.push(n);
            }
        }
    }
    for n in namen {
        let (mut r0, mut r1, mut c0, mut c1) = (usize::MAX, 0, usize::MAX, 0);
        let mut zahl = 0;
        for (r, z) in zeilen.iter().enumerate() {
            for (c, x) in z.iter().enumerate() {
                if x == n {
                    r0 = r0.min(r);
                    r1 = r1.max(r);
                    c0 = c0.min(c);
                    c1 = c1.max(c);
                    zahl += 1;
                }
            }
        }
        if zahl != (r1 - r0 + 1) * (c1 - c0 + 1) {
            return false;
        }
        for z in &zeilen[r0..=r1] {
            if z[c0..=c1].iter().any(|x| x != n) {
                return false;
            }
        }
    }
    true
}

pub fn areas(items: &[&Kv]) -> Option<Vec<S>> {
    if let [k] = items
        && k.ist_wort("none")
    {
        return Some(vec![S::W(String::from("none"))]);
    }
    let mut zeilen = Vec::new();
    for k in items {
        let Kv::T(Tok::Str(s)) = k else { return None };
        zeilen.push(areas_zeile(s)?);
    }
    if !areas_pruefen(&zeilen) {
        return None;
    }
    Some(zeilen.into_iter().map(|z| S::Txt(text_text(&z.join(" ")))).collect())
}

// ---------------------------------------------------------------------------
// font-family
// ---------------------------------------------------------------------------

const GENERISCH: &[&str] = &[
    "serif", "sans-serif", "cursive", "fantasy", "monospace", "system-ui", "emoji", "math", "fangsong", "ui-serif",
    "ui-sans-serif", "ui-monospace", "ui-rounded", "-webkit-body", "-webkit-pictograph",
];

pub fn font_family(items: &[&Kv]) -> Option<Vec<S>> {
    let mut raus = Vec::new();
    let mut teil: Vec<&Kv> = Vec::new();
    let mut gruppen: Vec<Vec<&Kv>> = Vec::new();
    for k in items {
        if k.ist_komma() {
            gruppen.push(std::mem::take(&mut teil));
        } else {
            teil.push(k);
        }
    }
    gruppen.push(teil);
    for (i, g) in gruppen.iter().enumerate() {
        if i > 0 {
            raus.push(S::Komma);
        }
        match g.as_slice() {
            [Kv::T(Tok::Str(s))] => raus.push(S::Txt(text_text(s))),
            [] => return None,
            [einer] if einer.ident().is_some_and(|n| GENERISCH.iter().any(|g| n.eq_ignore_ascii_case(g))) => {
                raus.push(S::W(einer.ident()?.to_ascii_lowercase()));
            }
            woerter => {
                let mut namen = Vec::new();
                for w in woerter {
                    let n = w.ident()?;
                    if werte::ist_css_weit(n) || n.eq_ignore_ascii_case("default") {
                        return None;
                    }
                    namen.push(ident_text(n));
                }
                raus.push(S::Txt(namen.join(" ")));
            }
        }
    }
    Some(raus)
}

// ---------------------------------------------------------------------------
// calc-size()
// ---------------------------------------------------------------------------

/// Release 12.10.2026: `calc-size(<basis>, <rechnung>)` (CSS Values 5 §8):
/// die Basis ist ein Groessen-Schluesselwort, `any`, ein weiteres
/// `calc-size()` oder eine Laenge; die Rechnung darf `size` benutzen (nicht
/// bei `any`). `auto` nicht in `max-*`, `content` nur in `flex-basis`.
pub fn calc_size(k: &Kv, auto_ok: bool, content_ok: bool) -> Option<S> {
    let Kv::F(n, a) = k else { return None };
    if !n.eq_ignore_ascii_case("calc-size") {
        return None;
    }
    let mut teile: Vec<&[Kv]> = Vec::new();
    let mut s = 0;
    for (i, x) in a.iter().enumerate() {
        if x.ist_komma() {
            teile.push(&a[s..i]);
            s = i + 1;
        }
    }
    teile.push(&a[s..]);
    if teile.len() != 2 {
        return None;
    }
    let basis_kv = super::syntax::getrimmt(teile[0]);
    let (basis, any) = match basis_kv {
        [Kv::T(Tok::Ident(w))] => {
            let w = w.to_ascii_lowercase();
            let ok = matches!(w.as_str(), "min-content" | "max-content" | "fit-content" | "stretch" | "any")
                || (w == "auto" && auto_ok)
                || (w == "content" && content_ok);
            if !ok {
                return None;
            }
            (w.clone(), w == "any")
        }
        [x @ Kv::F(f, _)] if f.eq_ignore_ascii_case("calc-size") => (super::werte::text(&[calc_size(x, auto_ok, content_ok)?]), false),
        kv => {
            // Eine Laenge (keine einheitenlose 0).
            if matches!(kv, [Kv::T(Tok::Zahl(..))]) {
                return None;
            }
            (super::mathe::summe_text(kv, super::mathe::Ziel::LaengeProzent, false)?, false)
        }
    };
    let rechnung_kv = super::syntax::getrimmt(teile[1]);
    if matches!(rechnung_kv, [Kv::T(Tok::Zahl(..))]) || rechnung_kv.is_empty() {
        return None;
    }
    let rechnung = super::mathe::summe_text(rechnung_kv, super::mathe::Ziel::LaengeProzent, !any)?;
    Some(S::F(String::from("calc-size"), vec![S::Txt(basis), S::Komma, S::Txt(rechnung)]))
}
