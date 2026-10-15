//! Release 12.10.2026: `attr()` einsetzen (CSS Values 5 §7.7 „Attribute
//! References") — beim berechneten Wert, mit dem Attribut des Elements.
//!
//! GEMESSEN vorher (WPT css-values attr-all-types, attr-length-specified):
//! `getComputedStyle` gab fuer `width: attr(data-foo type(<length>))` den
//! Anfangswert, `content: attr(data-foo)` den Rohtext.

use super::syntax::{self, Kv, Tok};

/// Hoechstens so viele `attr()` je Wert (M3).
const MAX_ATTR: usize = 256;

/// Einheiten, die `attr(x <einheit>)` erlaubt (§7.7.1 `<attr-unit>`).
const EINHEITEN: &[&str] = &[
    "em", "ex", "cap", "ch", "ic", "rem", "lh", "rlh", "vw", "vh", "vi", "vb", "vmin", "vmax", "px", "cm", "mm", "q", "in",
    "pt", "pc", "deg", "grad", "rad", "turn", "s", "ms", "hz", "khz", "dpi", "dpcm", "dppx", "x", "fr", "cqw", "cqh", "cqi",
    "cqb", "cqmin", "cqmax", "svw", "svh", "lvw", "lvh", "dvw", "dvh",
];

/// Alle `attr()` in `text` einsetzen; `attr` liefert den Attributwert.
/// `None` = ungueltig zur Berechnungszeit (IACVT).
pub fn attr_ersetzen(text: &str, attr: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let kv = syntax::komponenten(text);
    let mut zahl = 0usize;
    let neu = liste(&kv, attr, &mut zahl, false)?;
    Some(syntax::kv_text(&neu))
}

thread_local! {
    /// Ohne Adress-Pruefung (Custom Properties).
    static FREI: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn attr_ersetzen_frei(text: &str, attr: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let alt = FREI.with(|f| f.replace(true));
    let r = attr_ersetzen(text, attr);
    FREI.with(|f| f.set(alt));
    r
}

/// Funktionen, deren Inhalt eine Adresse ist (attr() darf dort nicht
/// stehen: „attr-tainted", CSS Values 5 §7.7.3).
const ADRESS_FUNKTIONEN: &[&str] = &["url", "src", "image-set", "-webkit-image-set", "image", "cross-fade"];

/// Ein eingesetzter Wert, der selbst eine Adresse traegt, ist ebenso unzulaessig.
fn traegt_adresse(kv: &[Kv]) -> bool {
    kv.iter().any(|k| match k {
        Kv::T(Tok::Url(_) | Tok::BadUrl) => true,
        Kv::F(n, a) => ADRESS_FUNKTIONEN.iter().any(|f| n.eq_ignore_ascii_case(f)) || traegt_adresse(a),
        Kv::B(_, a) => traegt_adresse(a),
        _ => false,
    })
}

fn liste(kv: &[Kv], attr: &dyn Fn(&str) -> Option<String>, zahl: &mut usize, in_adresse: bool) -> Option<Vec<Kv>> {
    let mut raus = Vec::with_capacity(kv.len());
    for k in kv {
        match k {
            Kv::F(n, a) if n.eq_ignore_ascii_case("attr") => {
                *zahl += 1;
                let frei = FREI.with(std::cell::Cell::get);
                if *zahl > MAX_ATTR || (in_adresse && !frei) {
                    return None;
                }
                let w = eins(a, attr, zahl)?;
                // Release 12.10.2026: ein eingesetzter Wert mit `attr()` wird
                // wieder eingesetzt (CSS Values 5 §7.7; Zyklen enden an
                // `MAX_ATTR`). GEMESSEN vorher (WPT attr-all-types 75/76):
                // `data-foo="attr(data-bar px)"` blieb woertlich stehen.
                let w = if syntax::enthaelt_funk(&w, "attr") { liste(&w, attr, zahl, in_adresse)? } else { w };
                if !frei && traegt_adresse(&w) {
                    return None;
                }
                raus.extend(w);
            }
            Kv::F(n, a) => {
                let adr = in_adresse || ADRESS_FUNKTIONEN.iter().any(|f| n.eq_ignore_ascii_case(f));
                raus.push(Kv::F(n.clone(), liste(a, attr, zahl, adr)?));
            }
            Kv::B(c, a) => raus.push(Kv::B(*c, liste(a, attr, zahl, in_adresse)?)),
            k => raus.push(k.clone()),
        }
    }
    Some(raus)
}

/// Ein `attr( <attr-name> <attr-type>? , <declaration-value>? )`.
fn eins(a: &[Kv], attr: &dyn Fn(&str) -> Option<String>, zahl: &mut usize) -> Option<Vec<Kv>> {
    let komma = a.iter().position(Kv::ist_komma);
    let (kopf, rueckfall) = match komma {
        Some(p) => (&a[..p], Some(&a[p + 1..])),
        None => (a, None),
    };
    let t: Vec<&Kv> = kopf.iter().filter(|k| !k.ist_leer()).collect();
    // Name, wahlweise mit Namensraum (`ns|name`, `*|name`, `|name`).
    let (name, rest) = match t.as_slice() {
        // Nur der leere Namensraum (`|name`) ist hier bekannt: ein Attribut
        // in einem anderen fehlt.
        [Kv::T(Tok::Ident(_)), d, Kv::T(Tok::Ident(_)), ..] if d.ist_delim('|') => {
            return rueckfall_wert(rueckfall, attr, zahl);
        }
        [d, Kv::T(Tok::Ident(n)), rest @ ..] if d.ist_delim('|') => (n.clone(), rest),
        [Kv::T(Tok::Ident(n)), rest @ ..] => (n.clone(), rest),
        _ => return None,
    };
    let wert = attr(&name);
    // Release 12.10.2026: ein fehlendes Attribut ohne Typ und ohne Ersatz ist
    // die leere Zeichenkette (CSS Values 5 §7.7.3; WPT attr-all-types 12).
    if wert.is_none() && rueckfall.is_none() && (rest.is_empty() || matches!(rest, [k] if k.ist_wort("raw-string"))) {
        return Some(vec![Kv::T(Tok::Str(String::new()))]);
    }
    let Some(wert) = wert else { return rueckfall_wert(rueckfall, attr, zahl) };
    let ergebnis: Option<Vec<Kv>> = match rest {
        // Ohne Typ (und `raw-string`): der Wert als Zeichenkette.
        [] => Some(vec![Kv::T(Tok::Str(wert))]),
        [k] if k.ist_wort("raw-string") => Some(vec![Kv::T(Tok::Str(wert))]),
        // `attr(x px)`: eine Zahl mit dieser Einheit.
        [Kv::T(Tok::Ident(e))] if EINHEITEN.contains(&e.to_ascii_lowercase().as_str()) => {
            zahl_mit(&wert).map(|(w, g)| vec![Kv::T(Tok::Dim(w, g, e.to_ascii_lowercase(), w < 0.0))])
        }
        [k] if k.ist_delim('%') => zahl_mit(&wert).map(|(w, g)| vec![Kv::T(Tok::Prozent(w, g))]),
        [k] if k.ist_wort("number") => zahl_mit(&wert).map(|(w, g)| vec![Kv::T(Tok::Zahl(w, g, w < 0.0))]),
        [Kv::F(f, s)] if f.eq_ignore_ascii_case("type") => getypt(&wert, s),
        // Release 12.10.2026: ein unbekannter Typname nimmt den Ersatz (WPT
        // attr-all-types 126/127: `attr(data-foo xx, 3px)`).
        [Kv::T(Tok::Ident(_))] => None,
        _ => return None,
    };
    match ergebnis {
        Some(v) => Some(v),
        None => rueckfall_wert(rueckfall, attr, zahl),
    }
}

/// Release 12.10.2026: ist ein `attr( … )` beim Lesen gueltig (CSS Values 5
/// §7.7.1)? Name (wahlweise mit Namensraum), dann hoechstens ein Typ: ein
/// Bezeichner, `%` oder `type(<syntax>)` mit gueltiger Syntax. GEMESSEN vorher
/// (WPT attr-all-types 58/62/128/164/172-174): `attr(x type(< length >))`,
/// `attr(x &, 3px)` und `attr(())` galten und setzten den Ersatz.
pub fn attr_gueltig(a: &[Kv]) -> bool {
    let kopf = match a.iter().position(Kv::ist_komma) {
        Some(p) => &a[..p],
        None => a,
    };
    let t: Vec<&Kv> = kopf.iter().filter(|k| !k.ist_leer()).collect();
    let rest: &[&Kv] = match t.as_slice() {
        [Kv::T(Tok::Ident(_)), d, Kv::T(Tok::Ident(_)), r @ ..] if d.ist_delim('|') => r,
        [d, Kv::T(Tok::Ident(_)), r @ ..] if d.ist_delim('|') => r,
        [s, d, Kv::T(Tok::Ident(_)), r @ ..] if s.ist_delim('*') && d.ist_delim('|') => r,
        [Kv::T(Tok::Ident(_)), r @ ..] => r,
        _ => return false,
    };
    match rest {
        [] => true,
        [Kv::T(Tok::Ident(_))] => true,
        [k] if k.ist_delim('%') => true,
        [Kv::F(f, s)] if f.eq_ignore_ascii_case("type") => {
            let syn = syntax::kv_text(syntax::getrimmt(s));
            let syn = syn.trim();
            if syn == "*" {
                return true;
            }
            // Keine Leerzeichen in `< >` und vor einem Vervielfacher.
            let z: Vec<char> = syn.chars().collect();
            let streng = z.iter().enumerate().all(|(i, &c)| match c {
                '<' => z.get(i + 1).is_some_and(|n| !n.is_whitespace()),
                '>' | '+' | '#' => i > 0 && !z[i - 1].is_whitespace(),
                _ => true,
            });
            streng && super::grammatik::lesen(syn).is_some()
        }
        _ => false,
    }
}

/// Der Rueckfallwert (selbst mit `attr()` darin); ohne: ungueltig.
fn rueckfall_wert(r: Option<&[Kv]>, attr: &dyn Fn(&str) -> Option<String>, zahl: &mut usize) -> Option<Vec<Kv>> {
    let r = syntax::getrimmt(r?);
    liste(r, attr, zahl, false)
}

/// Der Attributwert als genau eine Zahl.
fn zahl_mit(w: &str) -> Option<(f64, bool)> {
    match syntax::getrimmt(&syntax::komponenten(w)) {
        [Kv::T(Tok::Zahl(z, g, _))] => Some((*z, *g)),
        _ => None,
    }
}

/// `type(<syntax>)`: der Attributwert, gelesen nach der Syntax (§7.7.2).
fn getypt(wert: &str, s: &[Kv]) -> Option<Vec<Kv>> {
    // Release 12.10.2026: ein unvollstaendiger Wert (offene Klammer) taugt nicht
    // (WPT attr-all-types 81: `data-foo="attr("` nimmt den Ersatz).
    if !klammern_zu(wert) {
        return None;
    }
    let kv = syntax::komponenten(wert);
    let kern = syntax::getrimmt(&kv).to_vec();
    if kern.is_empty() || kern.iter().any(schlecht) {
        return None;
    }
    let syn = syntax::kv_text(syntax::getrimmt(s));
    // Release 12.10.2026: `<url>` ist in `type()` nicht erlaubt (CSS Values 5
    // §7.7.1, csswg-drafts#13946) — auch in einer Custom Property nicht.
    // GEMESSEN vorher (WPT attr-parse-url): `--x: attr(d type(<url>))` setzte
    // die Adresse ein.
    if syn.to_ascii_lowercase().contains("<url>") {
        return None;
    }
    if syn.trim() == "*" {
        return Some(kern);
    }
    let g = super::grammatik::lesen(&syn)?;
    let items: Vec<&Kv> = kern.iter().filter(|k| !k.ist_leer()).collect();
    let s = super::grammatik::abgleichen(&g, &items)?;
    // Kanonisch geschrieben (`3EM` -> `3em`; WPT attr-length-specified).
    Some(syntax::komponenten(&super::werte::text(&s)))
}

/// Sind alle Klammern eines Textes geschlossen (ausserhalb von Zeichenketten)?
fn klammern_zu(t: &str) -> bool {
    let mut stapel: Vec<char> = Vec::new();
    let mut in_text: Option<char> = None;
    let mut z = t.chars();
    while let Some(c) = z.next() {
        if let Some(q) = in_text {
            if c == '\\' {
                z.next();
            } else if c == q {
                in_text = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => in_text = Some(c),
            '\\' => {
                z.next();
            }
            '(' => stapel.push(')'),
            '[' => stapel.push(']'),
            '{' => stapel.push('}'),
            ')' | ']' | '}' => {
                if stapel.pop() != Some(c) {
                    return false;
                }
            }
            _ => {}
        }
    }
    stapel.is_empty() && in_text.is_none()
}

fn schlecht(k: &Kv) -> bool {
    match k {
        Kv::T(Tok::BadStr | Tok::BadUrl | Tok::ZuR | Tok::ZuE | Tok::ZuG | Tok::Semikolon) => true,
        Kv::T(Tok::Delim('!')) => true,
        Kv::F(_, a) | Kv::B(_, a) => a.iter().any(schlecht),
        _ => false,
    }
}

/// Release 12.10.2026: `random-item()` einsetzen (CSS Values 5 §9.2): mit
/// `fixed <zahl>` der Eintrag `floor(zahl * anzahl)`, sonst der erste.
/// Verschachtelte Aufrufe werden nach der Wahl aufgeloest. `None` =
/// ungueltig zur Berechnungszeit.
pub fn zufall_ersetzen(text: &str) -> Option<String> {
    let kv = syntax::komponenten(text);
    let mut zahl = 0usize;
    let neu = zufall_liste(&kv, &mut zahl)?;
    Some(syntax::kv_text(&neu))
}

fn zufall_liste(kv: &[Kv], zahl: &mut usize) -> Option<Vec<Kv>> {
    let mut raus = Vec::with_capacity(kv.len());
    for k in kv {
        match k {
            Kv::F(n, a) if n.eq_ignore_ascii_case("random-item") => {
                *zahl += 1;
                if *zahl > MAX_ATTR {
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
                if teile.len() < 2 {
                    return None;
                }
                let schluessel: Vec<&Kv> = teile[0].iter().filter(|x| !x.ist_leer()).collect();
                let anzahl = teile.len() - 1;
                let index = match schluessel.as_slice() {
                    [f, Kv::T(Tok::Zahl(w, _, _))] if f.ist_wort("fixed") => ((w.clamp(0.0, 1.0) * anzahl as f64).floor() as usize).min(anzahl - 1),
                    _ => 0,
                };
                let eintrag = syntax::getrimmt(teile[index + 1]);
                let inhalt: Vec<Kv> = match eintrag {
                    [Kv::B('{', innen)] => innen.clone(),
                    e => e.to_vec(),
                };
                raus.extend(zufall_liste(&inhalt, zahl)?);
            }
            Kv::F(n, a) => raus.push(Kv::F(n.clone(), zufall_liste(a, zahl)?)),
            Kv::B(c, a) => raus.push(Kv::B(*c, zufall_liste(a, zahl)?)),
            k => raus.push(k.clone()),
        }
    }
    Some(raus)
}

/// Release 12.10.2026: alle Ersetzungsfunktionen nach `var()` — `attr()` mit
/// den Attributen des Elements und `random-item()`.
pub fn alles_ersetzen(text: &str, attr: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    alles_ersetzen_mit(text, attr, true)
}

/// Wie [`alles_ersetzen`]; `adressen_pruefen = false` fuer Custom
/// Properties (ihr Wert ist noch keine Adresse; die Pruefung kommt beim
/// Einsetzen ueber `var()`).
pub fn alles_ersetzen_mit(text: &str, attr: &dyn Fn(&str) -> Option<String>, adressen_pruefen: bool) -> Option<String> {
    let klein = text.to_ascii_lowercase();
    let mut t = text.to_string();
    if klein.contains("attr(") {
        t = if adressen_pruefen { attr_ersetzen(&t, attr)? } else { attr_ersetzen_frei(&t, attr)? };
    }
    if klein.contains("random-item(") {
        t = zufall_ersetzen(&t)?;
    }
    if klein.contains("ident(") {
        t = ident_ersetzen(&t)?;
    }
    Some(t)
}

thread_local! {
    /// Release 12.10.2026: `sibling-index()`/`sibling-count()` des Elements,
    /// dessen Wert gerade eingesetzt wird (fuer `ident()`).
    static GESCHWISTER: std::cell::Cell<Option<(f64, f64)>> = const { std::cell::Cell::new(None) };
}

/// Release 12.10.2026: `sibling-index()`/`sibling-count()` als Zahlen einsetzen
/// (fuer den berechneten Wert, CSS Values 5 §11) — auch in Farben und
/// Verlaeufen, die sonst unaufgeloest blieben. GEMESSEN vorher (WPT
/// tree-counting sibling-index-linear-gradient-gcs): `rgb(0 0 calc(sibling-index()
/// * 200))` blieb im berechneten Wert stehen.
pub fn geschwister_einsetzen(text: &str, g: (f64, f64)) -> String {
    fn liste(kv: &[Kv], g: (f64, f64)) -> Vec<Kv> {
        kv.iter()
            .map(|k| match k {
                Kv::F(n, a) if syntax::getrimmt(a).is_empty() && n.eq_ignore_ascii_case("sibling-index") => Kv::T(Tok::Zahl(g.0, true, false)),
                Kv::F(n, a) if syntax::getrimmt(a).is_empty() && n.eq_ignore_ascii_case("sibling-count") => Kv::T(Tok::Zahl(g.1, true, false)),
                Kv::F(n, a) => Kv::F(n.clone(), liste(a, g)),
                Kv::B(c, a) => Kv::B(*c, liste(a, g)),
                k => k.clone(),
            })
            .collect()
    }
    syntax::kv_text(&liste(&syntax::komponenten(text), g))
}

/// Die Geschwisterzahlen fuer die folgenden Ersetzungen setzen (`None` = unbekannt).
pub fn geschwister_setzen(g: Option<(f64, f64)>) {
    GESCHWISTER.with(|x| x.set(g));
}

/// Release 12.10.2026: `ident( <ident-arg>+ )` einsetzen (CSS Values 5 §8.1;
/// `<ident-arg> = <string> | <integer> | <ident>`, auch Mathe-Funktionen
/// mit ganzzahligem Ergebnis): die Teile aneinandergehaengt als
/// Bezeichner. `None` = ungueltig zur Berechnungszeit. GEMESSEN vorher (WPT
/// css-values ident-function-computed): `view-transition-name:
/// ident(myident)` blieb woertlich stehen.
pub fn ident_ersetzen(text: &str) -> Option<String> {
    let kv = syntax::komponenten(text);
    let mut zahl = 0usize;
    let neu = ident_liste(&kv, &mut zahl)?;
    Some(syntax::kv_text(&neu))
}

fn ident_liste(kv: &[Kv], zahl: &mut usize) -> Option<Vec<Kv>> {
    let mut raus = Vec::with_capacity(kv.len());
    for k in kv {
        match k {
            Kv::F(n, a) if n.eq_ignore_ascii_case("ident") => {
                *zahl += 1;
                if *zahl > MAX_ATTR {
                    return None;
                }
                let innen = ident_liste(a, zahl)?;
                let mut name = String::new();
                for x in innen.iter().filter(|x| !x.ist_leer()) {
                    match x {
                        Kv::T(Tok::Str(s)) => name.push_str(s),
                        Kv::T(Tok::Ident(s)) => name.push_str(s),
                        Kv::T(Tok::Zahl(w, true, _)) => name.push_str(&format!("{}", *w as i64)),
                        Kv::F(f, args) => {
                            let m = super::mathe::lesen(f, args, super::mathe::Ziel::Ganz)?;
                            let u = super::mathe::Umgebung { geschwister: GESCHWISTER.with(std::cell::Cell::get), ..Default::default() };
                            let gel = m.aufgeloest(&u);
                            let (w, e) = gel.zahlwert()?;
                            if !e.is_empty() || !w.is_finite() {
                                return None;
                            }
                            name.push_str(&format!("{}", w.round() as i64));
                        }
                        _ => return None,
                    }
                }
                // Das Ergebnis muss ein Bezeichner sein (kein Leerwert, keine Ziffer vorn).
                let ok = !name.is_empty()
                    && syntax::getrimmt(&syntax::komponenten(&syntax::ident_text(&name))).len() == 1;
                if !ok {
                    return None;
                }
                raus.push(Kv::T(Tok::Ident(name)));
            }
            Kv::F(n, a) => raus.push(Kv::F(n.clone(), ident_liste(a, zahl)?)),
            Kv::B(c, a) => raus.push(Kv::B(*c, ident_liste(a, zahl)?)),
            k => raus.push(k.clone()),
        }
    }
    Some(raus)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(t: &str) -> Option<String> {
        let a = |n: &str| match n {
            "data-foo" => Some(String::from("3")),
            "data-s" => Some(String::from("abc")),
            "data-l" => Some(String::from("10px")),
            "data-p" => Some(String::from("10%")),
            _ => None,
        };
        attr_ersetzen(t, &a)
    }

    #[test]
    fn einsetzen() {
        assert_eq!(e("attr(data-s)").as_deref(), Some("\"abc\""));
        assert_eq!(e("attr(data-foo px)").as_deref(), Some("3px"));
        assert_eq!(e("attr(data-p type(<length> | <percentage>))").as_deref(), Some("10%"));
        assert_eq!(e("attr(data-l type(<length>))").as_deref(), Some("10px"));
        assert_eq!(e("attr(data-s type(<length>), 5px)").as_deref(), Some("5px"));
        assert_eq!(e("attr(data-s type(<length>))"), None);
        assert_eq!(e("attr(data-nix, 1em)").as_deref(), Some("1em"));
        assert_eq!(e("calc(attr(data-foo number) * 2px)").as_deref(), Some("calc(3 * 2px)"));
        assert_eq!(e("image-set(attr(data-s))"), None);
        assert_eq!(zufall_ersetzen("random-item(fixed 0, random-item(fixed 0.999, red, blue), green)").as_deref(), Some("blue"));
        assert_eq!(zufall_ersetzen("random-item(fixed 0, {1px 2px}, 3px)").as_deref(), Some("1px 2px"));
        assert_eq!(ident_ersetzen("ident(myident)").as_deref(), Some("myident"));
        assert_eq!(ident_ersetzen("ident(\"my\" 3 ident)").as_deref(), Some("my3ident"));
        assert_eq!(ident_ersetzen("ident(\"--prop\" calc(1 + 2))").as_deref(), Some("--prop3"));
        assert_eq!(ident_ersetzen("ident(ident(\"my\") ident(\"name\"))").as_deref(), Some("myname"));
        assert_eq!(ident_ersetzen("ident(5px)"), None);
        assert_eq!(ident_ersetzen("ident(3 \"myident\")").as_deref(), Some("\\33 myident"));
    }
}
