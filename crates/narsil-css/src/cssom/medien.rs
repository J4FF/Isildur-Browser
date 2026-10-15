//! Release 12.10.2026: Medienabfragen lesen und kanonisch schreiben (Media
//! Queries 4 §3, CSSOM §4.2 „serialize a media query list") — fuer
//! `MediaList.mediaText`, `appendMedium`/`deleteMedium` und die Kopfzeilen von
//! `@media`/`@import`.
//!
//! GEMESSEN vorher (WPT css/cssom MediaList-parsing, medialist-*): `mediaText`
//! war der Rohtext (`  screen  ,  print  `), ungueltige Abfragen wurden nicht
//! `not all`, `appendMedium` fehlte.

use super::syntax::{self, Kv, Tok, ident_text};
use super::zahl::zahl_text;

fn wort_klein(k: &Kv) -> Option<String> {
    k.ident().map(str::to_ascii_lowercase)
}

/// Ein Wert eines Medienmerkmals (`600px`, `2`, `16 / 9`, `landscape`).
fn mf_wert(t: &[&Kv]) -> Option<String> {
    match t {
        [Kv::T(Tok::Zahl(w, _, _))] => Some(zahl_text(*w)),
        [Kv::T(Tok::Dim(w, _, e, _))] => Some(format!("{}{}", zahl_text(*w), e.to_ascii_lowercase())),
        [Kv::T(Tok::Prozent(w, _))] => Some(format!("{}%", zahl_text(*w))),
        [Kv::T(Tok::Ident(w))] => Some(ident_text(&w.to_ascii_lowercase())),
        [Kv::T(Tok::Zahl(a, _, _)), s, Kv::T(Tok::Zahl(b, _, _))] if s.ist_delim('/') => Some(format!("{} / {}", zahl_text(*a), zahl_text(*b))),
        [k @ Kv::F(n, a)] if super::mathe::ist_mathe(n) => {
            for z in [super::mathe::Ziel::Laenge, super::mathe::Ziel::Zahl, super::mathe::Ziel::Aufloesung] {
                if let Some(m) = super::mathe::lesen(n, a, z) {
                    return Some(m.text());
                }
            }
            let _ = k;
            None
        }
        _ => None,
    }
}

fn vergleich(t: &[&Kv], i: usize) -> Option<(String, usize)> {
    match t.get(i)? {
        Kv::T(Tok::Delim(c @ ('<' | '>'))) => {
            if t.get(i + 1).is_some_and(|k| k.ist_delim('=')) {
                Some((format!("{c}="), 2))
            } else {
                Some((c.to_string(), 1))
            }
        }
        Kv::T(Tok::Delim('=')) => Some((String::from("="), 1)),
        _ => None,
    }
}

/// `( … )`: Merkmal, Bedingung in Klammern oder „general enclosed".
fn in_klammern(k: &Kv) -> Option<String> {
    match k {
        Kv::B('(', inhalt) => {
            let t: Vec<&Kv> = inhalt.iter().filter(|x| !x.ist_leer()).collect();
            // Verschachtelte Bedingung?
            if let Some(b) = bedingung(syntax::getrimmt(inhalt), true) {
                return Some(format!("({b})"));
            }
            // mf-boolean
            if let [Kv::T(Tok::Ident(n))] = t.as_slice() {
                return Some(format!("({})", ident_text(&n.to_ascii_lowercase())));
            }
            // mf-plain
            if let [Kv::T(Tok::Ident(n)), Kv::T(Tok::Doppelpunkt), rest @ ..] = t.as_slice() {
                let w = mf_wert(rest)?;
                return Some(format!("({}: {w})", ident_text(&n.to_ascii_lowercase())));
            }
            // mf-range: name op wert | wert op name | wert op name op wert
            if let Some(teile) = bereich(&t) {
                return Some(format!("({teile})"));
            }
            // Release 14.10.2026: anything else in parentheses is
            // <general-enclosed> (Media Queries 4 §3) and stays as written
            // (WPT cssom CSSImportRule-layer-false-positive `screen and (foo:
            // layer(bar))` became `not all`).
            let bad = |x: &&Kv| matches!(x, Kv::T(Tok::Semikolon | Tok::BadStr | Tok::BadUrl) | Kv::T(Tok::Delim('!')));
            (!t.is_empty() && !t.iter().any(bad)).then(|| syntax::kv_text(std::slice::from_ref(k)))
        }
        // general-enclosed: Funktion
        Kv::F(..) => Some(syntax::kv_text(std::slice::from_ref(k))),
        _ => None,
    }
}

fn bereich(t: &[&Kv]) -> Option<String> {
    // Stellen der Vergleichsoperatoren suchen.
    let mut ops = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if let Some((o, n)) = vergleich(t, i) {
            ops.push((i, o, n));
            i += n;
        } else {
            i += 1;
        }
    }
    let teil = |a: usize, b: usize| -> Option<String> {
        let s = &t[a..b];
        if let [Kv::T(Tok::Ident(n))] = s {
            return Some(ident_text(&n.to_ascii_lowercase()));
        }
        mf_wert(s)
    };
    match ops.as_slice() {
        [(i, o, n)] => Some(format!("{} {o} {}", teil(0, *i)?, teil(i + n, t.len())?)),
        [(i, o, n), (j, p, m)] => {
            if (o.starts_with('<') && p.starts_with('>')) || (o.starts_with('>') && p.starts_with('<')) || o == "=" || p == "=" {
                return None;
            }
            Some(format!("{} {o} {} {p} {}", teil(0, *i)?, teil(i + n, *j)?, teil(j + m, t.len())?))
        }
        _ => None,
    }
}

/// `<media-condition>`; `oder`: `or` erlaubt.
fn bedingung(kv: &[Kv], oder: bool) -> Option<String> {
    let t: Vec<&Kv> = kv.iter().filter(|x| !x.ist_leer()).collect();
    if t.is_empty() {
        return None;
    }
    if t[0].ist_wort("not") {
        if t.len() != 2 {
            return None;
        }
        return Some(format!("not {}", in_klammern(t[1])?));
    }
    let mut s = in_klammern(t[0])?;
    let mut op: Option<String> = None;
    let mut i = 1;
    while i < t.len() {
        let w = wort_klein(t[i])?;
        if !(w == "and" || (w == "or" && oder)) || op.as_ref().is_some_and(|o| *o != w) {
            return None;
        }
        s.push_str(&format!(" {w} {}", in_klammern(t.get(i + 1)?)?));
        op = Some(w);
        i += 2;
    }
    Some(s)
}

/// Eine Medienabfrage kanonisch; `None` = ungueltig (wird `not all`).
pub fn abfrage(kv: &[Kv]) -> Option<String> {
    let t: Vec<&Kv> = kv.iter().filter(|x| !x.ist_leer()).collect();
    if t.is_empty() {
        return None;
    }
    if let Some(b) = bedingung(syntax::getrimmt(kv), true) {
        return Some(b);
    }
    let mut i = 0;
    let mut vor = String::new();
    if let Some(w) = wort_klein(t[0])
        && (w == "not" || w == "only")
    {
        vor = format!("{w} ");
        i = 1;
    }
    let typ = wort_klein(t.get(i)?)?;
    if matches!(typ.as_str(), "not" | "only" | "and" | "or" | "layer") {
        return None;
    }
    i += 1;
    if i == t.len() {
        return Some(format!("{vor}{}", ident_text(&typ)));
    }
    if !t[i].ist_wort("and") {
        return None;
    }
    // Der Rest ist eine Bedingung ohne `or`.
    let rest: Vec<Kv> = t[i + 1..].iter().map(|k| (*k).clone()).collect();
    let b = bedingung(&rest, false)?;
    if vor.is_empty() && typ == "all" {
        return Some(b);
    }
    Some(format!("{vor}{} and {b}", ident_text(&typ)))
}

/// Eine Medienabfrageliste: je Abfrage ihr Text (`not all` fuer ungueltige).
pub fn liste(text: &str) -> Vec<String> {
    let kv = syntax::komponenten(text);
    let kv = syntax::getrimmt(&kv);
    if kv.is_empty() {
        return Vec::new();
    }
    let mut raus = Vec::new();
    let mut s = 0;
    for i in 0..=kv.len() {
        if i == kv.len() || kv[i].ist_komma() {
            raus.push(abfrage(&kv[s..i]).unwrap_or_else(|| String::from("not all")));
            s = i + 1;
        }
    }
    raus
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn medien() {
        assert_eq!(liste("  screen  ,  print  "), vec!["screen", "print"]);
        assert_eq!(liste("NOT  PRINT"), vec!["not print"]);
        assert_eq!(liste("screen, bad$, print"), vec!["screen", "not all", "print"]);
        assert_eq!(liste("all and (min-width:600px)"), vec!["(min-width: 600px)"]);
        assert_eq!(liste("screen and (min-width: 1px) and (color)"), vec!["screen and (min-width: 1px) and (color)"]);
        assert_eq!(liste("(width>=600px)"), vec!["(width >= 600px)"]);
        assert_eq!(liste("(aspect-ratio: 16/9)"), vec!["(aspect-ratio: 16 / 9)"]);
        assert!(liste("").is_empty());
    }
}
