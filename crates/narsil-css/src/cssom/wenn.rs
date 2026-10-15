//! Release 12.10.2026 (Agent C): `if()` (CSS Values 5 §9.1) — eine
//! Ersetzungsfunktion: der Wert des ersten Zweigs, dessen Bedingung gilt.
//!
//! ```text
//! if( [ <if-condition> : <declaration-value>? ; ]* <if-condition> : <declaration-value>? ;? )
//! <if-condition> = <boolean-expr[ <if-test> ]> | else
//! <if-test> = supports( … ) | media( … ) | style( <style-query> )
//! ```
//!
//! Die Bedingungen fragen den Aufrufer ([`Bedingungen`]): den berechneten
//! Wert einer Custom Property am Element (und an den Eltern fuer `inherit`),
//! eine Medienabfrage, eine `@supports`-Bedingung. Werte in den Bedingungen
//! werden ueber [`Bedingungen::einsetzen`] aufgeloest (var()/attr()), damit
//! ein unaufloesbares `var()` nur die Bedingung falsch macht, nicht den
//! ganzen Wert. Gilt kein Zweig, ist das Ergebnis leer.
//!
//! GEMESSEN vorher (WPT css-values if-conditionals, if-initial-unregistered,
//! sibling-function-if-style-query): `if(…)` blieb woertlich stehen bzw. die
//! Angabe fiel weg.

use super::mathe::{self, Umgebung, Ziel};
use super::syntax::{self, Kv, Tok};

/// Was eine Bedingung wissen muss.
pub trait Bedingungen {
    /// Der berechnete Wert der Custom Property am Element; `None` = der
    /// garantiert ungueltige Wert (nicht gesetzt, `initial`).
    fn var_wert(&self, name: &str) -> Option<String>;
    /// Dasselbe am Elternelement (fuer `style(--x: inherit)`).
    fn eltern_var(&self, name: &str) -> Option<String>;
    /// `var()`/`attr()` in einem Bedingungswert einsetzen; `None` = ungueltig.
    fn einsetzen(&self, text: &str) -> Option<String>;
    /// Gilt die Medienabfrage?
    fn medien(&self, abfrage: &str) -> bool;
    /// Gilt die `@supports`-Bedingung?
    fn supports(&self, bedingung: &str) -> bool;
    /// Fuer Vergleiche (`style(sibling-index() = 2)`): Schriftgroesse und
    /// Geschwister des Elements.
    fn umgebung(&self) -> Umgebung;
    /// Der berechnete Wert `wert` fuer eine registrierte Custom Property
    /// `name` (kanonisch); `None` = nicht registriert (Text vergleichen).
    fn registriert_berechnet(&self, name: &str, wert: &str) -> Option<Option<String>> {
        let _ = (name, wert);
        None
    }
}

/// Release 12.10.2026: `wert` nach der Syntax einer registrierten Custom
/// Property (`@property … { syntax: … }`) als berechneter Wert (kanonisch);
/// `None` = passt nicht. `*` vergleicht den Text.
pub fn nach_syntax_berechnet(syntax_text: &str, wert: &str, u: Umgebung) -> Option<String> {
    let syn = syntax_text.trim().trim_matches(['"', '\'']).trim();
    let kv = syntax::komponenten(wert);
    let kern = syntax::getrimmt(&kv);
    if syn == "*" {
        return Some(syntax::kv_text(kern));
    }
    let g = super::grammatik::lesen(syn)?;
    let items: Vec<&Kv> = kern.iter().filter(|k| !k.ist_leer()).collect();
    let s = super::grammatik::abgleichen(&g, &items)?;
    let ber = super::werte::Ber { u, farbe: None, basis: None };
    Some(super::werte::berechnet_text(&s, &ber))
}

/// Hoechstens so viele `if()` je Wert (M3).
const MAX_WENN: usize = 256;

/// Alle `if()` in `text` aufloesen; `None` = ungueltig (kaputtes `if()`).
pub fn ersetzen(text: &str, b: &dyn Bedingungen) -> Option<String> {
    if !text.to_ascii_lowercase().contains("if(") {
        return Some(text.to_string());
    }
    let kv = syntax::komponenten(text);
    let mut zahl = 0usize;
    let neu = liste(&kv, b, &mut zahl)?;
    let mut text = syntax::kv_text(&neu);
    // Release 13.10.2026: `kv_text` trims; trailing whitespace of a chosen
    // branch stays (WPT if-conditionals 1).
    if neu.last().is_some_and(Kv::ist_leer) && !text.is_empty() {
        text.push(' ');
    }
    Some(text)
}

/// Wuerden zwei aufeinanderfolgende Token beim erneuten Lesen verschmelzen
/// (`a` `b` -> `ab`)? Dann steht `/**/` dazwischen (CSS Syntax 3 §9).
fn verschmilzt(a: &Kv, b: &Kv) -> bool {
    let wortartig = |k: &Kv| {
        matches!(
            k,
            Kv::T(Tok::Ident(_) | Tok::Zahl(..) | Tok::Dim(..) | Tok::Prozent(..) | Tok::Hash(..) | Tok::Url(_)) | Kv::F(..)
        )
    };
    wortartig(a) && (wortartig(b) && !matches!(a, Kv::F(..)) || matches!(b, Kv::T(Tok::Ident(_) | Tok::Zahl(..))) && matches!(a, Kv::F(..)))
}

fn kommentar(raus: &mut Vec<Kv>) {
    for c in ['/', '*', '*', '/'] {
        raus.push(Kv::T(Tok::Delim(c)));
    }
}

fn liste(kv: &[Kv], b: &dyn Bedingungen, zahl: &mut usize) -> Option<Vec<Kv>> {
    let mut raus: Vec<Kv> = Vec::with_capacity(kv.len());
    let mut nach_wenn = false;
    for k in kv {
        match k {
            Kv::F(n, a) if n.eq_ignore_ascii_case("if") => {
                *zahl += 1;
                if *zahl > MAX_WENN {
                    return None;
                }
                let gewaehlt = wenn(a, b)?;
                let teil = liste(&gewaehlt, b, zahl)?;
                if let (Some(l), Some(f)) = (raus.last(), teil.first())
                    && verschmilzt(l, f)
                {
                    kommentar(&mut raus);
                }
                if !teil.is_empty() {
                    raus.extend(teil);
                    nach_wenn = true;
                }
                continue;
            }
            Kv::F(n, a) => {
                if nach_wenn && raus.last().is_some_and(|l| verschmilzt(l, k)) {
                    kommentar(&mut raus);
                }
                raus.push(Kv::F(n.clone(), liste(a, b, zahl)?));
            }
            Kv::B(c, a) => raus.push(Kv::B(*c, liste(a, b, zahl)?)),
            k => {
                if nach_wenn && raus.last().is_some_and(|l| verschmilzt(l, k)) {
                    kommentar(&mut raus);
                }
                raus.push(k.clone());
            }
        }
        nach_wenn = false;
    }
    Some(raus)
}

/// Der Inhalt eines `if( … )`: die Zweige der Reihe nach.
fn wenn(a: &[Kv], b: &dyn Bedingungen) -> Option<Vec<Kv>> {
    let mut zweige: Vec<&[Kv]> = Vec::new();
    let mut s = 0;
    for (i, x) in a.iter().enumerate() {
        if matches!(x, Kv::T(Tok::Semikolon)) {
            zweige.push(&a[s..i]);
            s = i + 1;
        }
    }
    zweige.push(&a[s..]);
    // Release 13.10.2026: a `!` in ANY branch value makes the whole if()
    // invalid, also in a branch that is not taken (WPT if-conditionals 189:
    // `else: false_value!` with a true first branch).
    for z in &zweige {
        let z = syntax::getrimmt(z);
        if let Some(p) = z.iter().position(|x| matches!(x, Kv::T(Tok::Doppelpunkt)))
            && z[p + 1..].iter().any(|x| x.ist_delim('!'))
        {
            return None;
        }
    }
    let mut gesehen = false;
    for z in zweige {
        // Release 13.10.2026: only leading whitespace goes; the last branch
        // keeps its trailing whitespace (WPT if-conditionals 1: `if( style(
        // --x : 3 ) : true_value )` gives `true_value `).
        let z = &z[z.iter().position(|x| !x.ist_leer()).unwrap_or(z.len())..];
        if z.is_empty() {
            continue;
        }
        let p = z.iter().position(|x| matches!(x, Kv::T(Tok::Doppelpunkt)))?;
        // `var()`/`attr()` in der Bedingung zuerst (WPT if-conditionals 202:
        // `if(var(--else): …)`).
        let bed_kv: Vec<Kv>;
        let bed_teil: &[Kv] = if z[..p].iter().any(|x| matches!(x, Kv::F(n, _) if n.eq_ignore_ascii_case("var") || n.eq_ignore_ascii_case("attr"))) {
            let text = b.einsetzen(&syntax::kv_text(&z[..p]))?;
            bed_kv = syntax::komponenten(&text);
            &bed_kv
        } else {
            &z[..p]
        };
        let bed: Vec<&Kv> = bed_teil.iter().filter(|x| !x.ist_leer()).collect();
        if bed.is_empty() {
            return None;
        }
        gesehen = true;
        if bedingung(&bed, b)? {
            // Der Wert behaelt hinten seinen Leerraum (wie Chrome).
            let w = &z[p + 1..];
            let anfang = w.iter().position(|x| !x.ist_leer()).unwrap_or(w.len());
            return Some(w[anfang..].to_vec());
        }
    }
    gesehen.then(Vec::new)
}

/// `<boolean-expr[ <if-test> ]> | else`.
fn bedingung(t: &[&Kv], b: &dyn Bedingungen) -> Option<bool> {
    if let [k] = t
        && k.ist_wort("else")
    {
        return Some(true);
    }
    // Release 13.10.2026: a bare unknown word (`invalid`) does not parse as
    // an <if-condition>, so the whole condition is false — also next to `or`
    // (WPT if-conditionals 192: `invalid or style(--x)`).
    if t.iter().any(|x| matches!(x, Kv::T(Tok::Ident(n)) if !matches!(n.to_ascii_lowercase().as_str(), "and" | "or" | "not"))) {
        return Some(false);
    }
    bool_ausdruck(t, b, &test)
}

/// `not X`, `X [and X]*`, `X [or X]*` (nicht gemischt), `( … )`.
fn bool_ausdruck(t: &[&Kv], b: &dyn Bedingungen, einzeln: &dyn Fn(&Kv, &dyn Bedingungen) -> Option<bool>) -> Option<bool> {
    match t {
        [] => None,
        [n, rest @ ..] if n.ist_wort("not") => {
            let [x] = rest else { return None };
            Some(!glied(x, b, einzeln)?)
        }
        [x] => glied(x, b, einzeln),
        _ => {
            // Abwechselnd Glied und Verbinder.
            let verbinder = t.get(1)?;
            let und = if verbinder.ist_wort("and") {
                true
            } else if verbinder.ist_wort("or") {
                false
            } else {
                return None;
            };
            let mut ergebnis = und;
            for (i, x) in t.iter().enumerate() {
                if i % 2 == 1 {
                    if !(if und { x.ist_wort("and") } else { x.ist_wort("or") }) {
                        return None;
                    }
                    continue;
                }
                let w = glied(x, b, einzeln)?;
                ergebnis = if und { ergebnis && w } else { ergebnis || w };
            }
            (t.len() % 2 == 1).then_some(ergebnis)
        }
    }
}

fn glied(x: &Kv, b: &dyn Bedingungen, einzeln: &dyn Fn(&Kv, &dyn Bedingungen) -> Option<bool>) -> Option<bool> {
    match x {
        // Eine Klammer, die keine gueltige Bedingung ist, ist „general-enclosed":
        // unbekannt, also falsch (CSS Conditional 5 §3.2).
        Kv::B('(', innen) => einzeln(x, b)
            .or_else(|| {
                let t: Vec<&Kv> = innen.iter().filter(|k| !k.ist_leer()).collect();
                bool_ausdruck(&t, b, einzeln)
            })
            .or(Some(false)),
        x => einzeln(x, b),
    }
}

/// `style( … )`, `media( … )`, `supports( … )`.
fn test(x: &Kv, b: &dyn Bedingungen) -> Option<bool> {
    let Kv::F(n, a) = x else { return None };
    let n = n.to_ascii_lowercase();
    match n.as_str() {
        "media" => {
            let q = syntax::kv_text(syntax::getrimmt(a));
            // `media(width > 1px)`: ein Merkmal ohne Klammern.
            Some(if q.trim_start().starts_with('(') { b.medien(&q) } else { b.medien(&format!("({q})")) || b.medien(&q) })
        }
        "supports" => Some(b.supports(&syntax::kv_text(syntax::getrimmt(a)))),
        "style" => {
            let t: Vec<&Kv> = a.iter().filter(|k| !k.ist_leer()).collect();
            // Eine ungueltige Abfrage ist unbekannt, also falsch.
            Some(stil_abfrage(&t, a, b).unwrap_or(false))
        }
        _ => None,
    }
}

/// Eine Stilabfrage: Verknuepfung von `( … )`, sonst EIN Merkmal.
fn stil_abfrage(t: &[&Kv], roh: &[Kv], b: &dyn Bedingungen) -> Option<bool> {
    let verknuepft = t.iter().any(|k| k.ist_wort("and") || k.ist_wort("or")) || t.first().is_some_and(|k| k.ist_wort("not"));
    if verknuepft || matches!(t, [Kv::B('(', _)]) {
        return bool_ausdruck(t, b, &|x, b| match x {
            Kv::B('(', innen) => {
                let tt: Vec<&Kv> = innen.iter().filter(|k| !k.ist_leer()).collect();
                stil_abfrage(&tt, innen, b)
            }
            _ => None,
        });
    }
    merkmal(roh, b)
}

/// `--x`, `--x: wert`, oder ein Bereich `a = b` / `a < b` …
fn merkmal(roh: &[Kv], b: &dyn Bedingungen) -> Option<bool> {
    let roh = syntax::getrimmt(roh);
    if let Some(p) = roh.iter().position(|x| matches!(x, Kv::T(Tok::Doppelpunkt))) {
        let name: Vec<&Kv> = roh[..p].iter().filter(|k| !k.ist_leer()).collect();
        let [Kv::T(Tok::Ident(name))] = name.as_slice() else { return None };
        if !name.starts_with("--") {
            // `style(color: green)`: nur Custom Properties (CSS Conditional 5).
            return Some(false);
        }
        let wert = syntax::getrimmt(&roh[p + 1..]);
        if wert.iter().any(|k| k.ist_delim('!')) {
            return Some(false);
        }
        let wert_text = syntax::kv_text(wert);
        let ist = b.var_wert(name);
        let klein = wert_text.trim().to_ascii_lowercase();
        return Some(match klein.as_str() {
            "initial" => match b.registriert_berechnet(name, "initial") {
                Some(anfang) => ist.and_then(|i| b.registriert_berechnet(name, &i).flatten()) == anfang,
                None => ist.is_none(),
            },
            "inherit" => {
                let e = b.eltern_var(name);
                match (&ist, &e) {
                    (Some(i), Some(e)) => gleich(name, i, e, b),
                    (None, None) => b.registriert_berechnet(name, "x").is_none(),
                    _ => false,
                }
            }
            "unset" => {
                // Unregistriert (und registriert vererbt): wie `inherit`.
                let e = b.eltern_var(name);
                match b.registriert_berechnet(name, "initial") {
                    Some(anfang) if !erbt(name, b) => ist.and_then(|i| b.registriert_berechnet(name, &i).flatten()) == anfang,
                    _ => match (&ist, &e) {
                        (Some(i), Some(e)) => gleich(name, i, e, b),
                        (None, None) => true,
                        _ => false,
                    },
                }
            }
            "revert" | "revert-layer" | "revert-rule" => false,
            _ => {
                let Some(soll) = b.einsetzen(&wert_text) else { return Some(false) };
                match ist {
                    Some(i) => gleich(name, &i, &soll, b),
                    None => false,
                }
            }
        });
    }
    // `style(--x)`: gesetzt (nicht der garantiert ungueltige Wert)?
    let t: Vec<&Kv> = roh.iter().filter(|k| !k.ist_leer()).collect();
    if let [Kv::T(Tok::Ident(name))] = t.as_slice() {
        if !name.starts_with("--") {
            return None;
        }
        return Some(b.var_wert(name).is_some());
    }
    bereich(roh, b)
}

/// Ob eine registrierte Property erbt (ohne Registrierung: ja).
fn erbt(name: &str, b: &dyn Bedingungen) -> bool {
    b.registriert_berechnet(name, "\u{1}erbt").is_none_or(|v| v.is_some())
}

fn normal(t: &str) -> String {
    syntax::kv_text(syntax::getrimmt(&syntax::komponenten(t)))
}

fn gleich(name: &str, a: &str, bb: &str, b: &dyn Bedingungen) -> bool {
    match (b.registriert_berechnet(name, a), b.registriert_berechnet(name, bb)) {
        (Some(x), Some(y)) => x.is_some() && x == y,
        _ => normal(a) == normal(bb),
    }
}

/// `style(a = b)`, `style(a < b <= c)` … mit Zahlen/Laengen.
fn bereich(roh: &[Kv], b: &dyn Bedingungen) -> Option<bool> {
    let mut teile: Vec<Vec<Kv>> = vec![Vec::new()];
    let mut ops: Vec<String> = Vec::new();
    let mut i = 0;
    while i < roh.len() {
        let x = &roh[i];
        let op = if x.ist_delim('<') || x.ist_delim('>') {
            let mut o = if x.ist_delim('<') { String::from("<") } else { String::from(">") };
            if roh.get(i + 1).is_some_and(|n| n.ist_delim('=')) {
                o.push('=');
                i += 1;
            }
            Some(o)
        } else if x.ist_delim('=') {
            Some(String::from("="))
        } else {
            None
        };
        match op {
            Some(o) => {
                ops.push(o);
                teile.push(Vec::new());
            }
            None => {
                if let Some(t) = teile.last_mut() {
                    t.push(x.clone());
                }
            }
        }
        i += 1;
    }
    if ops.is_empty() || teile.len() != ops.len() + 1 {
        return None;
    }
    let mut gelesen: Vec<(f64, Ziel)> = Vec::new();
    for t in &teile {
        let Some(wz) = zahl_von(&syntax::kv_text(syntax::getrimmt(t)), b) else { return Some(false) };
        gelesen.push(wz);
    }
    // Release 13.10.2026: a unitless zero is also a <length> (WPT
    // if-conditionals 114/116: `0 = 0px`, `0 < 3px`; `0 = 0%` stays false).
    if gelesen.iter().any(|(_, z)| *z == Ziel::Laenge) {
        for (w, z) in &mut gelesen {
            if *z == Ziel::Zahl && *w == 0.0 {
                *z = Ziel::Laenge;
            }
        }
    }
    let mut werte: Vec<f64> = Vec::new();
    let mut typ: Option<Ziel> = None;
    for (w, z) in gelesen {
        // Verschiedene Typen (`0 = 0%`, `3em > 3deg`) vergleichen nie.
        if typ.is_some_and(|x| x != z) {
            return Some(false);
        }
        typ = Some(z);
        werte.push(w);
    }
    Some(ops.iter().enumerate().all(|(j, o)| {
        let (l, r) = (werte[j], werte[j + 1]);
        match o.as_str() {
            "=" => (l - r).abs() < 1e-9,
            "<" => l < r,
            "<=" => l <= r,
            ">" => l > r,
            _ => l >= r,
        }
    }))
}

/// Ein Vergleichswert als Zahl in der kanonischen Einheit seines Typs
/// (Laengen px, Aufloesungen dppx …): `--x` wird eingesetzt.
fn zahl_von(t: &str, b: &dyn Bedingungen) -> Option<(f64, Ziel)> {
    let t = t.trim();
    let t = if t.starts_with("--") { b.var_wert(t)? } else { b.einsetzen(t)? };
    let u = b.umgebung();
    let kv = syntax::komponenten(&format!("calc({t})"));
    let [Kv::F(n, a)] = syntax::getrimmt(&kv) else { return None };
    for ziel in [Ziel::Zahl, Ziel::Laenge, Ziel::Winkel, Ziel::Zeit, Ziel::Frequenz, Ziel::Aufloesung, Ziel::Prozent] {
        if let Some(m) = mathe::lesen(n, a, ziel) {
            let m = m.aufgeloest(&u);
            if let Some((w, _)) = m.zahlwert() {
                return Some((w, ziel));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct B;
    impl Bedingungen for B {
        fn var_wert(&self, name: &str) -> Option<String> {
            match name {
                "--x" => Some(String::from("3")),
                "--y" => Some(String::from("red")),
                _ => None,
            }
        }
        fn eltern_var(&self, _: &str) -> Option<String> {
            None
        }
        fn einsetzen(&self, text: &str) -> Option<String> {
            (!text.contains("var(")).then(|| text.to_string())
        }
        fn medien(&self, a: &str) -> bool {
            a.contains("screen")
        }
        fn supports(&self, a: &str) -> bool {
            a.contains("display")
        }
        fn umgebung(&self) -> Umgebung {
            Umgebung { geschwister: Some((2.0, 3.0)), ..Default::default() }
        }
    }

    fn e(t: &str) -> Option<String> {
        ersetzen(t, &B)
    }

    #[test]
    fn trailing_whitespace_of_the_last_branch_stays() {
        assert_eq!(e("if( style( --x : 3 ) : true_value )").as_deref(), Some("true_value "));
        assert_eq!(e("if(style(--x: 3): a; else: b!)"), None);
        assert_eq!(e("if(invalid or style(--x): a; else: b)").as_deref(), Some("b"));
        assert_eq!(e("if(style(0 = 0px): a; else: b)").as_deref(), Some("a"));
        assert_eq!(e("if(style(0 = 0%): a; else: b)").as_deref(), Some("b"));
    }

    #[test]
    fn zweige() {
        assert_eq!(e("if(style(--x: 3): ja)").as_deref(), Some("ja"));
        assert_eq!(e("if(style(--x: 0): ja; else: nein)").as_deref(), Some("nein"));
        assert_eq!(e("if(style(--x: 0): ja)").as_deref(), Some(""));
        assert_eq!(e("if(style(--nix): ja; style(--x): zwei; else: drei)").as_deref(), Some("zwei"));
        assert_eq!(e("if(not style(--nix): ja; else: nein)").as_deref(), Some("ja"));
        assert_eq!(e("if(style((--x: 3) and (not (--y: blue))): ja; else: nein)").as_deref(), Some("ja"));
        assert_eq!(e("if(style(--nix: initial): ja; else: nein)").as_deref(), Some("ja"));
        assert_eq!(e("if(media(screen): a; else: b) if(supports(display: grid): c)").as_deref(), Some("a c"));
        assert_eq!(e("if(style(sibling-index() = 2): ja; else: nein)").as_deref(), Some("ja"));
        assert_eq!(e("if(style(--x: var(--q)): ja; else: nein)").as_deref(), Some("nein"));
        assert_eq!(e("if(style(color: green): ja; else: nein)").as_deref(), Some("nein"));
    }
}
