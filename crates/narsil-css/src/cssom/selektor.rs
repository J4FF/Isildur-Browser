//! Release 12.10.2026: Selektoren pruefen und kanonisch schreiben (Selectors 4
//! §16 Grammatik, CSSOM §6.6.3 „serialize a selector") — fuer
//! `querySelector` (SyntaxError bei Ungueltigem), `CSS.supports(selector())`,
//! `CSSStyleRule.selectorText` und das Verwerfen ungueltiger Regeln.
//!
//! GEMESSEN vorher (WPT css/selectors/parsing, css/cssom): `selectorText` war
//! der Rohtext (`[att=val]` statt `[att="val"]`, `:nth-child(1n+0)` statt
//! `:nth-child(n)`), `querySelector(':not()')` warf nicht, und
//! `CSS.supports('selector(…)')` war immer `false`.

use super::syntax::{self, Kv, Tok, ident_text, text_text};

/// Namensraeume eines Stylesheets (`@namespace`).
#[derive(Debug, Clone, Default)]
pub struct Namensraeume {
    /// Der Standard-Namensraum (`@namespace url(…)`), falls erklaert.
    pub standard: Option<String>,
    /// Erklaerte Praefixe -> Namensraum.
    pub praefixe: Vec<(String, String)>,
}

/// Was an einer Stelle erlaubt ist.
#[derive(Debug, Clone, Copy)]
struct Lage<'a> {
    ns: &'a Namensraeume,
    /// In `:not()/:is()/:where()/:has()`: keine Pseudoelemente.
    ohne_pseudoelement: bool,
    /// In `:host()`/`::slotted()` u. ae.: nur einfache Verbindungen.
    nur_einfach: bool,
    /// In `:has()`: kein weiteres `:has()`.
    in_has: bool,
    /// Vergebende Listen (`:is()`, `:where()`) streng pruefen (`CSS.supports`).
    streng: bool,
    /// Release 12.10.2026: im `:is()`/`:where()`/`:not()` hinter `::part()`:
    /// keine Typ-/Klassen-/Id-/Attributselektoren, keine strukturellen
    /// Pseudoklassen (CSS Shadow Parts §4.2).
    nach_part: bool,
    /// Hinter einem anderen Pseudoelement: nur Benutzeraktions-Pseudoklassen.
    nur_aktion: bool,
    /// Fuer den Abgleich: ungueltige Teile vergebender Listen fallen weg
    /// (sonst bleiben sie als Rohtext stehen, Selectors 4 §3.5).
    abgleich: bool,
}

/// Strukturelle Pseudoklassen (nicht hinter `::part()`).
const STRUKTURELL: &[&str] = &[
    "first-child", "last-child", "only-child", "first-of-type", "last-of-type", "only-of-type", "empty", "root", "scope",
];

type Karte = std::collections::HashMap<usize, (usize, usize)>;

thread_local! {
    /// Release 12.10.2026: der vorverarbeitete Quelltext und die Lage jedes
    /// Komponentenwerts (Adresse -> Zeichen `von..bis`) — ungueltige Teile
    /// einer vergebenden Liste werden woertlich geschrieben (WPT
    /// parse-is-where: `:is(.a, 123)`, `:is([attr='value'])`).
    static QUELLE: std::cell::RefCell<Option<(Vec<char>, Karte)>> = const { std::cell::RefCell::new(None) };
}

/// Die Lagen der Komponentenwerte `kv` (in der Reihenfolge der Token `t`).
fn lagen_sammeln(kv: &[Kv], t: &[(Tok, usize, usize)], i: &mut usize, karte: &mut Karte) {
    for k in kv {
        let Some(&(_, von, _)) = t.get(*i) else { return };
        *i += 1;
        match k {
            Kv::T(_) => {}
            Kv::F(_, inhalt) | Kv::B(_, inhalt) => {
                lagen_sammeln(inhalt, t, i, karte);
                let zu = match k {
                    Kv::B('[', _) => Tok::ZuE,
                    Kv::B('{', _) => Tok::ZuG,
                    _ => Tok::ZuR,
                };
                if t.get(*i).is_some_and(|(x, _, _)| *x == zu) {
                    *i += 1;
                }
            }
        }
        let bis = t.get(*i - 1).map_or(von, |x| x.2);
        karte.insert(k as *const Kv as usize, (von, bis));
    }
}

/// Komponentenwerte des Selektortexts; die Lagen kommen nach `QUELLE`.
fn mit_quelle<R>(text: &str, f: impl FnOnce(&[Kv]) -> R) -> R {
    let (z, t) = syntax::tokens_pos(text);
    let nur: Vec<Tok> = t.iter().map(|x| x.0.clone()).collect();
    let kv = syntax::komponenten_aus(&nur);
    let mut karte = Karte::new();
    let mut i = 0;
    lagen_sammeln(&kv, &t, &mut i, &mut karte);
    let alt = QUELLE.with(|q| q.replace(Some((z, karte))));
    let r = f(&kv);
    QUELLE.with(|q| *q.borrow_mut() = alt);
    r
}

/// Der Rohtext eines (getrimmten) Teils.
fn roh_text(t: &[Kv]) -> String {
    let (Some(a), Some(b)) = (t.first(), t.last()) else { return String::new() };
    QUELLE
        .with(|q| {
            let q = q.borrow();
            let (z, karte) = q.as_ref()?;
            let von = karte.get(&(a as *const Kv as usize))?.0;
            let bis = karte.get(&(b as *const Kv as usize))?.1;
            (von <= bis && bis <= z.len()).then(|| z[von..bis].iter().collect::<String>())
        })
        .unwrap_or_else(|| syntax::kv_text(t))
}

/// Schlechte Token (`<any-value>` erlaubt sie nicht, CSS Syntax 3 §8.2):
/// ungepaarte schliessende Klammern, kaputte Zeichenketten/URLs.
fn hat_schlechte(t: &[Kv]) -> bool {
    t.iter().any(|k| match k {
        Kv::T(Tok::ZuR | Tok::ZuE | Tok::ZuG | Tok::BadStr | Tok::BadUrl) => true,
        Kv::T(_) => false,
        Kv::F(_, a) | Kv::B(_, a) => hat_schlechte(a),
    })
}

const PSEUDOKLASSEN: &[&str] = &[
    "active", "any-link", "autofill", "blank", "buffering", "checked", "closed", "current", "default", "defined", "disabled",
    "empty", "enabled", "first", "first-child", "first-of-type", "focus", "focus-visible", "focus-within", "fullscreen",
    "future", "has-slotted", "heading", "host", "hover", "in-range", "indeterminate", "invalid", "last-child", "last-of-type",
    "left", "link", "local-link", "modal", "muted", "only-child", "only-of-type", "open", "optional", "out-of-range", "past",
    "paused", "picture-in-picture", "placeholder-shown", "playing", "popover-open", "read-only", "read-write", "required",
    "right", "root", "scope", "seeking", "stalled", "target", "target-current", "target-within", "user-invalid", "user-valid",
    "valid", "visited", "volume-locked", "xr-overlay", "interest-source", "interest-target",
    "-webkit-autofill", "-webkit-any-link", "-moz-focusring", "-moz-placeholder", "-moz-ui-invalid", "-ms-input-placeholder",
    "-webkit-full-screen", "-moz-full-screen",
];

const PSEUDOELEMENTE: &[&str] = &[
    "before", "after", "marker", "placeholder", "selection", "first-line", "first-letter", "backdrop", "file-selector-button",
    "grammar-error", "spelling-error", "target-text", "cue", "cue-region", "view-transition", "details-content", "picker-icon",
    "checkmark", "scroll-marker", "scroll-marker-group", "column", "search-text", "-webkit-input-placeholder",
    "-moz-placeholder", "-moz-selection", "-webkit-scrollbar", "-webkit-scrollbar-thumb", "-webkit-scrollbar-track",
    "-webkit-scrollbar-track-piece", "-webkit-scrollbar-button", "-webkit-scrollbar-corner", "-webkit-resizer",
    "-webkit-file-upload-button", "-webkit-search-cancel-button", "-webkit-search-decoration", "-webkit-inner-spin-button",
    "-webkit-outer-spin-button", "-webkit-details-marker", "-webkit-slider-thumb", "-webkit-slider-runnable-track",
    "-webkit-progress-bar", "-webkit-progress-value", "-webkit-meter-bar", "-webkit-color-swatch",
    "-webkit-color-swatch-wrapper", "-webkit-calendar-picker-indicator", "-webkit-date-and-time-value", "-moz-range-thumb",
    "-moz-range-track", "-moz-focus-inner", "-ms-clear", "-ms-reveal", "-ms-expand",
];

/// Pseudoklassen, die nach einem Pseudoelement stehen duerfen (Benutzeraktion).
const NACH_PSEUDOELEMENT: &[&str] = &["hover", "focus", "focus-visible", "focus-within", "active"];

/// Alte Pseudoelemente, die auch mit EINEM Doppelpunkt gelten (Selectors 4 §3.6.1).
const ALTE_PSEUDOELEMENTE: &[&str] = &["before", "after", "first-line", "first-letter"];

struct L<'a> {
    t: &'a [Kv],
    i: usize,
}

impl<'a> L<'a> {
    fn an(&self) -> Option<&'a Kv> {
        self.t.get(self.i)
    }
    fn an_k(&self, k: usize) -> Option<&'a Kv> {
        self.t.get(self.i + k)
    }
    fn leer(&mut self) -> bool {
        let s = self.i;
        while self.an().is_some_and(Kv::ist_leer) {
            self.i += 1;
        }
        self.i > s
    }
    fn ende(&self) -> bool {
        self.i >= self.t.len()
    }
}

/// Eine Pseudoelement-Art (bestimmt, was danach erlaubt ist).
#[derive(Debug, Clone, Copy, PartialEq)]
enum PE {
    Normal,
    Part,
    Slotted,
    /// `::scroll-button()`: dahinter auch `:disabled`/`:enabled`.
    Knopf,
}

fn ist_delim(k: Option<&Kv>, c: char) -> bool {
    k.is_some_and(|k| k.ist_delim(c))
}

/// `[ns|]name` bzw. `[ns|]*`: (Praefix-Art, Name); Praefix: None = keiner,
/// Some("") = leer (`|x`), Some("*") = jeder.
fn qname(l: &mut L) -> Option<(Option<String>, String)> {
    let erstes = |k: &Kv| -> Option<String> {
        match k {
            Kv::T(Tok::Ident(n)) => Some(n.clone()),
            Kv::T(Tok::Delim('*')) => Some(String::from("*")),
            _ => None,
        }
    };
    let start = l.i;
    // `|name` (leerer Namensraum)
    if ist_delim(l.an(), '|') && !ist_delim(l.an_k(1), '|') {
        if let Some(n) = l.an_k(1).and_then(erstes) {
            l.i += 2;
            return Some((Some(String::new()), n));
        }
        return None;
    }
    let a = erstes(l.an()?)?;
    l.i += 1;
    if ist_delim(l.an(), '|') && !ist_delim(l.an_k(1), '|') {
        if let Some(n) = l.an_k(1).and_then(erstes) {
            l.i += 2;
            return Some((Some(a), n));
        }
        // `[att|=val]`: der Strich gehoert zum Vergleich (WPT parse-attribute).
        if ist_delim(l.an_k(1), '=') {
            return Some((None, a));
        }
        l.i = start;
        return None;
    }
    Some((None, a))
}

fn praefix_text(p: &Option<String>, ns: &Namensraeume) -> Option<String> {
    Some(match p {
        None => String::new(),
        Some(x) if x.is_empty() => String::from("|"),
        Some(x) if x == "*" => {
            // Ohne Standard-Namensraum ist `*|x` dasselbe wie `x`.
            if ns.standard.is_some() { String::from("*|") } else { String::new() }
        }
        Some(x) => {
            let (_, uri) = ns.praefixe.iter().find(|(p, _)| p == x)?;
            // Ein Praefix fuer den Standard-Namensraum faellt weg (CSSOM
            // §6.6.3, WPT serialize-namespaced-type-selectors).
            if ns.standard.as_deref() == Some(uri.as_str()) {
                return Some(String::new());
            }
            format!("{}|", ident_text(x))
        }
    })
}

/// An+B (CSS Syntax 3 §6): Rueckgabe (a, b) und die Zahl verbrauchter Stuecke.
fn anb(t: &[&Kv]) -> Option<((i64, i64), usize)> {
    let ganz = |w: f64| (w == w.trunc() && w.abs() < 1e15).then_some(w as i64);
    let n_rest = |s: &str| -> Option<(i64, Option<i64>)> {
        // `n`, `n-`, `n-5` (Bezeichner-Rest nach dem Koeffizienten)
        let s = s.to_ascii_lowercase();
        let r = s.strip_prefix('n')?;
        if r.is_empty() {
            return Some((1, None));
        }
        if r == "-" {
            return Some((1, Some(i64::MIN)));
        }
        let z = r.strip_prefix('-')?;
        if z.is_empty() || !z.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some((1, Some(-z.parse::<i64>().ok()?)))
    };
    let k0 = t.first()?;
    // odd / even
    if let Kv::T(Tok::Ident(w)) = k0 {
        if w.eq_ignore_ascii_case("odd") {
            return Some(((2, 1), 1));
        }
        if w.eq_ignore_ascii_case("even") {
            return Some(((2, 0), 1));
        }
    }
    // reine Zahl
    if let Kv::T(Tok::Zahl(w, true, _)) = k0 {
        return Some(((0, ganz(*w)?), 1));
    }
    // a und der Rest des n-Teils
    let (a, rest, plus) = match k0 {
        Kv::T(Tok::Dim(w, true, e, _)) => {
            let (_, r) = n_rest(e)?;
            (ganz(*w)?, r, false)
        }
        Kv::T(Tok::Ident(w)) => {
            let w2 = w.to_ascii_lowercase();
            if let Some(r) = w2.strip_prefix('-') {
                let (_, r) = n_rest(r)?;
                (-1, r, false)
            } else {
                let (_, r) = n_rest(&w2)?;
                (1, r, false)
            }
        }
        Kv::T(Tok::Delim('+')) => {
            // `+n…` (ohne Leerzeichen dazwischen)
            let Some(Kv::T(Tok::Ident(w))) = t.get(1) else { return None };
            let w2 = w.to_ascii_lowercase();
            let (_, r) = n_rest(&w2)?;
            (1, r, true)
        }
        _ => return None,
    };
    let mut n = if plus { 2 } else { 1 };
    match rest {
        Some(i64::MIN) => {
            // `n- 5`: es folgt eine Zahl ohne Vorzeichen
            let mut j = n;
            while t.get(j).is_some_and(|k| k.ist_leer()) {
                j += 1;
            }
            let Some(Kv::T(Tok::Zahl(w, true, false))) = t.get(j) else { return None };
            Some(((a, -ganz(*w)?), j + 1))
        }
        Some(b) => Some(((a, b), n)),
        None => {
            // optional: [+|-] b oder ein Zahl mit Vorzeichen
            let mut j = n;
            while t.get(j).is_some_and(|k| k.ist_leer()) {
                j += 1;
            }
            match t.get(j) {
                Some(Kv::T(Tok::Zahl(w, true, true))) => {
                    n = j + 1;
                    Some(((a, ganz(*w)?), n))
                }
                Some(Kv::T(Tok::Delim(c @ ('+' | '-')))) => {
                    let mut m = j + 1;
                    while t.get(m).is_some_and(|k| k.ist_leer()) {
                        m += 1;
                    }
                    let Some(Kv::T(Tok::Zahl(w, true, false))) = t.get(m) else { return None };
                    let b = ganz(*w)?;
                    Some(((a, if *c == '-' { -b } else { b }), m + 1))
                }
                _ => Some(((a, 0), n)),
            }
        }
    }
}

fn anb_text(a: i64, b: i64) -> String {
    if a == 0 {
        return b.to_string();
    }
    let mut s = match a {
        1 => String::from("n"),
        -1 => String::from("-n"),
        _ => format!("{a}n"),
    };
    if b > 0 {
        s.push_str(&format!("+{b}"));
    } else if b < 0 {
        s.push_str(&b.to_string());
    }
    s
}

/// Die Argumente einer Funktion, getrimmt und ohne Leerzeichen-Token.
fn ohne_leer(a: &[Kv]) -> Vec<&Kv> {
    a.iter().filter(|k| !k.ist_leer()).collect()
}

fn komma_teile(a: &[Kv]) -> Vec<&[Kv]> {
    let mut raus = Vec::new();
    let mut s = 0;
    for (i, k) in a.iter().enumerate() {
        if k.ist_komma() {
            raus.push(&a[s..i]);
            s = i + 1;
        }
    }
    raus.push(&a[s..]);
    raus
}

/// Eine (komplexe) Selektorliste; `vergebend` laesst ungueltige Teile weg.
fn liste_text(a: &[Kv], lage: Lage, vergebend: bool, relativ: bool) -> Option<String> {
    let mut teile = Vec::new();
    for t in komma_teile(a) {
        let t = syntax::getrimmt(t);
        match komplex(t, lage, relativ) {
            Some(s) => teile.push(s),
            // Release 12.10.2026: ein ungueltiger Teil bleibt woertlich stehen
            // (Selectors 4 §3.5 „forgiving selector list"); schlechte Token
            // machen die ganze Liste ungueltig. Fuer den Abgleich faellt er weg.
            None if vergebend && !lage.streng => {
                if hat_schlechte(t) {
                    return None;
                }
                if !lage.abgleich {
                    teile.push(roh_text(t));
                }
            }
            None => return None,
        }
    }
    if teile.is_empty() && !vergebend {
        return None;
    }
    if teile.is_empty() && lage.abgleich {
        return Some(String::from(":not(*)"));
    }
    Some(teile.join(", "))
}

fn funktion_text(name: &str, a: &[Kv], lage: Lage, nach_pe: Option<PE>) -> Option<String> {
    let n = name.to_ascii_lowercase();
    let mut innen = Lage { ohne_pseudoelement: true, ..lage };
    match nach_pe {
        Some(PE::Part) => {
            innen.nach_part = true;
            innen.nur_einfach = true;
        }
        Some(PE::Normal | PE::Knopf) => {
            innen.nur_aktion = true;
            innen.nur_einfach = true;
        }
        _ => {}
    }
    let args = syntax::getrimmt(a);
    let inhalt = match n.as_str() {
        "not" => {
            if args.is_empty() {
                return None;
            }
            liste_text(args, innen, false, false)?
        }
        "is" | "where" | "matches" | "-webkit-any" | "-moz-any" => {
            let s = liste_text(args, innen, true, false)?;
            if n == "matches" || n.starts_with('-') {
                return Some(format!(":is({s})"));
            }
            s
        }
        "has" => {
            if lage.in_has || args.is_empty() || nach_pe.is_some() {
                return None;
            }
            liste_text(args, Lage { in_has: true, nur_einfach: false, ..innen }, false, true)?
        }
        "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" | "nth-col" | "nth-last-col" => {
            let t = ohne_leer_mit_leer(args);
            let ((a_, b_), n_) = anb(&t)?;
            let mut s = anb_text(a_, b_);
            let mut rest: &[&Kv] = &t[n_..];
            while rest.first().is_some_and(|k| k.ist_leer()) {
                rest = &rest[1..];
            }
            if !rest.is_empty() {
                if !(n == "nth-child" || n == "nth-last-child") || !rest[0].ist_wort("of") {
                    return None;
                }
                // Den Rest als Selektorliste lesen (ab hinter `of`).
                let pos = a.len() - rest.len() + 1;
                let sel = liste_text(syntax::getrimmt(&a[pos..]), innen, false, false)?;
                s.push_str(" of ");
                s.push_str(&sel);
            }
            s
        }
        "lang" => {
            let mut v = Vec::new();
            for t in komma_teile(args) {
                match syntax::getrimmt(t) {
                    [Kv::T(Tok::Ident(w))] => v.push(ident_text(w)),
                    [Kv::T(Tok::Str(w))] => v.push(text_text(w)),
                    _ => return None,
                }
            }
            v.join(", ")
        }
        "dir" => match ohne_leer(args).as_slice() {
            [k] if k.ist_wort("ltr") || k.ist_wort("rtl") => k.ident()?.to_ascii_lowercase(),
            _ => return None,
        },
        "host" | "host-context" | "has-slotted" => {
            let s = komplex(args, Lage { nur_einfach: true, ..innen }, false)?;
            s
        }
        "state" => match ohne_leer(args).as_slice() {
            [Kv::T(Tok::Ident(w))] => ident_text(w),
            _ => return None,
        },
        "heading" => {
            let mut v = Vec::new();
            for t in komma_teile(args) {
                match syntax::getrimmt(t) {
                    [Kv::T(Tok::Zahl(w, true, _))] => v.push(super::zahl::zahl_text(*w)),
                    _ => return None,
                }
            }
            v.join(", ")
        }
        "active-view-transition-type" => {
            let mut v = Vec::new();
            for t in komma_teile(args) {
                match syntax::getrimmt(t) {
                    [Kv::T(Tok::Ident(w))] => v.push(ident_text(w)),
                    _ => return None,
                }
            }
            v.join(", ")
        }
        _ => return None,
    };
    Some(format!(":{n}({inhalt})"))
}

/// Die Stuecke ohne fuehrende/abschliessende Leerzeichen, Leerzeichen innen bleiben.
fn ohne_leer_mit_leer(a: &[Kv]) -> Vec<&Kv> {
    syntax::getrimmt(a).iter().collect()
}

fn pseudoelement_funktion(name: &str, a: &[Kv], lage: Lage) -> Option<(String, PE)> {
    let n = name.to_ascii_lowercase();
    let args = syntax::getrimmt(a);
    let innen = Lage { ohne_pseudoelement: true, nur_einfach: true, ..lage };
    Some(match n.as_str() {
        "slotted" => (format!("::slotted({})", komplex(args, innen, false)?), PE::Slotted),
        "part" => {
            let t = ohne_leer(args);
            if t.is_empty() {
                return None;
            }
            let v: Vec<String> = t.iter().map(|k| k.ident().map(ident_text)).collect::<Option<_>>()?;
            (format!("::part({})", v.join(" ")), PE::Part)
        }
        "highlight" => match ohne_leer(args).as_slice() {
            [Kv::T(Tok::Ident(w))] => (format!("::highlight({})", ident_text(w)), PE::Normal),
            _ => return None,
        },
        "cue" | "cue-region" => (format!("::{n}({})", liste_text(args, innen, false, false)?), PE::Normal),
        "scroll-button" => match ohne_leer(args).as_slice() {
            [k] if k.ist_delim('*') => (String::from("::scroll-button(*)"), PE::Knopf),
            [Kv::T(Tok::Ident(w))]
                if ["up", "down", "left", "right", "block-start", "block-end", "inline-start", "inline-end", "prev", "next"]
                    .iter()
                    .any(|x| w.eq_ignore_ascii_case(x)) =>
            {
                (format!("::scroll-button({})", w.to_ascii_lowercase()), PE::Knopf)
            }
            _ => return None,
        },
        "picker" => match ohne_leer(args).as_slice() {
            [Kv::T(Tok::Ident(w))] => (format!("::picker({})", ident_text(w)), PE::Normal),
            _ => return None,
        },
        "view-transition-group" | "view-transition-image-pair" | "view-transition-old" | "view-transition-new"
        | "view-transition-group-children" => {
            let t = ohne_leer(args);
            let mut s = String::new();
            for (i, k) in t.iter().enumerate() {
                match k {
                    Kv::T(Tok::Delim('*')) if i == 0 => s.push('*'),
                    Kv::T(Tok::Ident(w)) if i == 0 => s.push_str(&ident_text(w)),
                    Kv::T(Tok::Delim('.')) => s.push('.'),
                    Kv::T(Tok::Ident(w)) if s.ends_with('.') => s.push_str(&ident_text(w)),
                    _ => return None,
                }
            }
            if s.is_empty() || s.ends_with('.') {
                return None;
            }
            (format!("::{n}({s})"), PE::Normal)
        }
        _ => return None,
    })
}

/// Ein zusammengesetzter Selektor (`a.b:c::d`). Rueckgabe: Text und ob er
/// mit einem Pseudoelement endet.
fn verbindung(l: &mut L, lage: Lage) -> Option<(String, Option<PE>)> {
    let mut typ: Option<String> = None;
    let mut universal_text: Option<String> = None;
    let mut rest = String::new();
    let mut pe: Option<PE> = None;
    let mut irgendwas = false;
    let start = l.i;
    let eng = lage.nach_part || lage.nur_aktion;
    if let Some((p, n)) = qname(l) {
        if eng {
            return None;
        }
        let pt = praefix_text(&p, lage.ns)?;
        if n == "*" {
            universal_text = Some(format!("{pt}*"));
        } else {
            typ = Some(format!("{pt}{}", ident_text(&n)));
        }
        irgendwas = true;
    } else {
        l.i = start;
    }
    loop {
        let Some(k) = l.an() else { break };
        match k {
            Kv::T(Tok::Leer | Tok::Komma) => break,
            Kv::T(Tok::Delim('>' | '+' | '~')) => break,
            Kv::T(Tok::Delim('|')) if ist_delim(l.an_k(1), '|') => break,
            Kv::T(Tok::Delim('&' | '.')) | Kv::T(Tok::Hash(..)) | Kv::B('[', _) if eng => return None,
            Kv::T(Tok::Delim('&')) => {
                if pe.is_some() {
                    return None;
                }
                rest.push('&');
                l.i += 1;
            }
            Kv::T(Tok::Hash(h, id)) => {
                if !*id || pe.is_some() {
                    return None;
                }
                rest.push('#');
                rest.push_str(&ident_text(h));
                l.i += 1;
            }
            Kv::T(Tok::Delim('.')) => {
                let Some(Kv::T(Tok::Ident(n))) = l.an_k(1) else { return None };
                if pe.is_some() {
                    return None;
                }
                rest.push('.');
                rest.push_str(&ident_text(n));
                l.i += 2;
                // Release 14.10.2026: the class prefix selector `.foo-*`
                // (Selectors 5, tentative): a dash-terminated name directly
                // followed by `*`.
                if n.ends_with('-') && n != "-" && ist_delim(l.an(), '*') {
                    rest.push('*');
                    l.i += 1;
                }
            }
            Kv::B('[', inhalt) => {
                if pe.is_some() {
                    return None;
                }
                rest.push_str(&attribut(inhalt, lage.ns)?);
                l.i += 1;
            }
            Kv::T(Tok::Doppelpunkt) => {
                let zwei = matches!(l.an_k(1), Some(Kv::T(Tok::Doppelpunkt)));
                let k2 = if zwei { l.an_k(2) } else { l.an_k(1) }?;
                l.i += if zwei { 3 } else { 2 };
                match (zwei, k2) {
                    (true, Kv::T(Tok::Ident(n))) => {
                        let n = n.to_ascii_lowercase();
                        if lage.ohne_pseudoelement || pe.is_some_and(|p| p != PE::Part) || !PSEUDOELEMENTE.contains(&n.as_str()) {
                            return None;
                        }
                        if pe == Some(PE::Part)
                            && !["before", "after", "placeholder", "first-line", "first-letter", "file-selector-button", "marker", "selection", "backdrop", "details-content"].contains(&n.as_str())
                        {
                            return None;
                        }
                        rest.push_str("::");
                        rest.push_str(&n);
                        pe = Some(PE::Normal);
                    }
                    (true, Kv::F(n, a)) => {
                        if lage.ohne_pseudoelement || pe.is_some_and(|p| p != PE::Part) {
                            return None;
                        }
                        let (t, art) = pseudoelement_funktion(n, a, lage)?;
                        rest.push_str(&t);
                        pe = Some(art);
                    }
                    (false, Kv::T(Tok::Ident(n))) => {
                        let n = n.to_ascii_lowercase();
                        if ALTE_PSEUDOELEMENTE.contains(&n.as_str()) {
                            if lage.ohne_pseudoelement || pe.is_some() {
                                return None;
                            }
                            rest.push_str("::");
                            rest.push_str(&n);
                            pe = Some(PE::Normal);
                        } else {
                            if !PSEUDOKLASSEN.contains(&n.as_str()) {
                                return None;
                            }
                            match pe {
                                Some(PE::Slotted) => return None,
                                Some(PE::Normal) if !NACH_PSEUDOELEMENT.contains(&n.as_str()) => return None,
                                Some(PE::Knopf)
                                    if !NACH_PSEUDOELEMENT.contains(&n.as_str()) && !matches!(n.as_str(), "disabled" | "enabled") =>
                                {
                                    return None;
                                }
                                _ => {}
                            }
                            if lage.nur_aktion && !NACH_PSEUDOELEMENT.contains(&n.as_str()) {
                                return None;
                            }
                            if lage.nach_part && STRUKTURELL.contains(&n.as_str()) {
                                return None;
                            }
                            rest.push(':');
                            rest.push_str(&n);
                        }
                    }
                    (false, Kv::F(n, a)) => {
                        let nk = n.to_ascii_lowercase();
                        if lage.nur_aktion && !matches!(nk.as_str(), "is" | "where" | "not") {
                            return None;
                        }
                        if lage.nach_part && matches!(nk.as_str(), "has" | "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type") {
                            return None;
                        }
                        match pe {
                            Some(PE::Slotted) => return None,
                            Some(PE::Normal | PE::Knopf) if !matches!(nk.as_str(), "is" | "where" | "not") => return None,
                            Some(PE::Part) if matches!(nk.as_str(), "has" | "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type") => return None,
                            _ => {}
                        }
                        if nk == "state" && matches!(pe, Some(PE::Normal | PE::Knopf)) {
                            return None;
                        }
                        rest.push_str(&funktion_text(n, a, lage, pe)?);
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
        irgendwas = true;
    }
    if !irgendwas {
        return None;
    }
    // §6.6.3: ein Universal-Selektor nur, wenn er allein steht (oder einen
    // Namensraum traegt).
    let mut s = String::new();
    if let Some(t) = typ {
        s.push_str(&t);
    } else if let Some(u) = universal_text
        && (rest.is_empty() || u != "*")
    {
        s.push_str(&u);
    }
    s.push_str(&rest);
    Some((s, pe))
}

fn attribut(inhalt: &[Kv], ns: &Namensraeume) -> Option<String> {
    let t = syntax::getrimmt(inhalt);
    let mut l = L { t, i: 0 };
    let (p, n) = qname(&mut l)?;
    if n == "*" {
        return None;
    }
    let pt = match &p {
        None => String::new(),
        Some(x) if x.is_empty() => String::new(),
        Some(x) if x == "*" => String::from("*|"),
        Some(x) => {
            ns.praefixe.iter().find(|(q, _)| q == x)?;
            format!("{}|", ident_text(x))
        }
    };
    let name = format!("{pt}{}", ident_text(&n));
    l.leer();
    if l.ende() {
        return Some(format!("[{name}]"));
    }
    let op = match l.an()? {
        Kv::T(Tok::Delim('=')) => {
            l.i += 1;
            "="
        }
        Kv::T(Tok::Delim(c @ ('~' | '|' | '^' | '$' | '*'))) if ist_delim(l.an_k(1), '=') => {
            l.i += 2;
            match c {
                '~' => "~=",
                '|' => "|=",
                '^' => "^=",
                '$' => "$=",
                _ => "*=",
            }
        }
        _ => return None,
    };
    l.leer();
    let wert = match l.an()? {
        Kv::T(Tok::Ident(w)) => w.clone(),
        Kv::T(Tok::Str(w)) => w.clone(),
        _ => return None,
    };
    l.i += 1;
    l.leer();
    let mut modif = String::new();
    if let Some(Kv::T(Tok::Ident(m))) = l.an() {
        if !(m.eq_ignore_ascii_case("i") || m.eq_ignore_ascii_case("s")) {
            return None;
        }
        modif = format!(" {}", m.to_ascii_lowercase());
        l.i += 1;
        l.leer();
    }
    if !l.ende() {
        return None;
    }
    Some(format!("[{name}{op}{}{modif}]", text_text(&wert)))
}

/// Ein komplexer Selektor (`a > b c`); `relativ`: darf mit einem
/// Kombinator beginnen (`:has(> a)`).
fn komplex(t: &[Kv], lage: Lage, relativ: bool) -> Option<String> {
    let t = syntax::getrimmt(t);
    if t.is_empty() {
        return None;
    }
    let mut l = L { t, i: 0 };
    let mut s = String::new();
    if relativ
        && let Some(Kv::T(Tok::Delim(c @ ('>' | '+' | '~')))) = l.an()
    {
        s.push(*c);
        s.push(' ');
        l.i += 1;
        l.leer();
    }
    loop {
        let (v, pe) = verbindung(&mut l, lage)?;
        s.push_str(&v);
        let leer = l.leer();
        if l.ende() {
            break;
        }
        if pe.is_some() {
            return None;
        }
        let komb = match l.an()? {
            Kv::T(Tok::Delim(c @ ('>' | '+' | '~'))) => {
                l.i += 1;
                Some(*c)
            }
            Kv::T(Tok::Delim('|')) => return None,
            _ if leer => None,
            _ => return None,
        };
        if lage.nur_einfach {
            return None;
        }
        l.leer();
        if l.ende() {
            return None;
        }
        match komb {
            Some(c) => {
                s.push(' ');
                s.push(c);
                s.push(' ');
            }
            None => s.push(' '),
        }
    }
    Some(s)
}

/// Eine Selektorliste pruefen und kanonisch schreiben. `None` = ungueltig.
pub fn liste(text: &str, ns: &Namensraeume) -> Option<String> {
    liste_mit(text, ns, false)
}

/// Release 12.10.2026: eine Liste RELATIVER Selektoren (verschachtelte
/// Stilregeln, `@scope (…) to (…)`): ein Kombinator am Anfang ist erlaubt.
pub fn liste_relativ(text: &str, ns: &Namensraeume) -> Option<String> {
    mit_quelle(text, |kv| liste_text(syntax::getrimmt(kv), Lage::neu(ns, false), false, true))
}

/// Release 12.10.2026: die Selektorliste von `@scope (…) to (…)` — ohne
/// Pseudoelemente (CSS Cascade 6 §2.5.1; WPT at-scope-parsing).
pub fn liste_scope(text: &str, ns: &Namensraeume, relativ: bool) -> Option<String> {
    mit_quelle(text, |kv| {
        let lage = Lage { ohne_pseudoelement: true, ..Lage::neu(ns, false) };
        liste_text(syntax::getrimmt(kv), lage, false, relativ)
    })
}

/// Release 12.10.2026: die Liste fuer den Abgleich — ungueltige Teile
/// vergebender Listen fallen weg, eine leere wird `:not(*)` (trifft nie;
/// WPT parse-has-disallow-nesting-has-inside-has). `None` = ungueltig.
pub fn liste_abgleich(text: &str, ns: &Namensraeume) -> Option<String> {
    mit_quelle(text, |kv| {
        let lage = Lage { abgleich: true, ..Lage::neu(ns, false) };
        liste_text(syntax::getrimmt(kv), lage, false, false)
    })
}

impl<'a> Lage<'a> {
    fn neu(ns: &'a Namensraeume, streng: bool) -> Lage<'a> {
        Lage {
            ns,
            ohne_pseudoelement: false,
            nur_einfach: false,
            in_has: false,
            streng,
            nach_part: false,
            nur_aktion: false,
            abgleich: false,
        }
    }
}

/// Wie [`liste`]; `streng`: vergebende Listen (`:is()`) duerfen keine
/// ungueltigen Teile haben (`CSS.supports('selector(…)')`).
pub fn liste_mit(text: &str, ns: &Namensraeume, streng: bool) -> Option<String> {
    mit_quelle(text, |kv| liste_text(syntax::getrimmt(kv), Lage::neu(ns, streng), false, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        liste(t, &Namensraeume::default()).unwrap_or_else(|| String::from("UNGUELTIG"))
    }

    #[test]
    fn selektoren() {
        assert_eq!(s("[att=val]"), "[att=\"val\"]");
        assert_eq!(s(":nth-child(1n+0)"), ":nth-child(n)");
        assert_eq!(s(":nth-child( +n + 7 )"), ":nth-child(n+7)");
        assert_eq!(s(":nth-child(  N- 123)"), ":nth-child(n-123)");
        assert_eq!(s(":nth-child(-n\n- 1)"), ":nth-child(-n-1)");
        assert_eq!(s(":nth-child(n- 1 2)"), "UNGUELTIG");
        assert_eq!(s(":nth-child(+ 1n)"), "UNGUELTIG");
        assert_eq!(s(":nth-child(odd of .a)"), ":nth-child(2n+1 of .a)");
        assert_eq!(s("*|*:not(*)"), ":not(*)");
        assert_eq!(s(":not(*|*)"), ":not(*)");
        assert_eq!(s(":not()"), "UNGUELTIG");
        assert_eq!(s(":not(::before)"), "UNGUELTIG");
        assert_eq!(s("div ol>li p"), "div ol > li p");
        assert_eq!(s(":is(ul,ol,.list) > [hidden]"), ":is(ul, ol, .list) > [hidden]");
        assert_eq!(s(":is(.a, 123)"), ":is(.a, 123)");
        assert_eq!(s(":is(,,,)"), ":is(, , , )");
        assert_eq!(s(":host(:is(.a, .b+.c, .d))"), ":host(:is(.a, .b+.c, .d))");
        assert_eq!(s("::part(foo):is([attr='value'])"), "::part(foo):is([attr='value'])");
        assert_eq!(s(":is(::before:HOVER  )"), ":is(::before:HOVER)");
        assert_eq!(s("[att|=val]"), "[att|=\"val\"]");
        assert_eq!(s("::scroll-button(up):disabled"), "::scroll-button(up):disabled");
        assert_eq!(liste_abgleich(":has(:is(:has(*), script))", &Namensraeume::default()).as_deref(), Some(":has(:is(script))"));
        assert_eq!(liste_abgleich(":has(:is(:has(*)))", &Namensraeume::default()).as_deref(), Some(":has(:is(:not(*)))"));
        assert!(liste_mit("::part(foo):is(ul)", &Namensraeume::default(), true).is_none());
        assert!(liste_mit("::part(foo):is(:focus)", &Namensraeume::default(), true).is_some());
        assert!(liste_mit(":is(.a, 123)", &Namensraeume::default(), true).is_none());
        assert_eq!(s("a:before"), "a::before");
        assert_eq!(s("::part(foo):hover"), "::part(foo):hover");
        assert_eq!(s("::slotted(foo):hover"), "UNGUELTIG");
        assert_eq!(s(".a:has(.b:has(.c))"), "UNGUELTIG");
        assert_eq!(s(":has(> span)"), ":has(> span)");
        assert_eq!(s("#000"), "UNGUELTIG");
        assert_eq!(s(".foo*"), "UNGUELTIG");
        assert_eq!(s("::has-slotted(foo)"), "UNGUELTIG");
        assert_eq!(s(":heading(0, 1, 2)"), ":heading(0, 1, 2)");
        assert_eq!(s(":heading(2n)"), "UNGUELTIG");
        assert_eq!(s(":host(:not(.a .b))"), "UNGUELTIG");
        assert_eq!(s("ns|a"), "UNGUELTIG");
        assert_eq!(s("[|att]"), "[att]");
        assert_eq!(s("[att i]"), "UNGUELTIG");
        assert_eq!(s("[att=\"x\" I]"), "[att=\"x\" i]");
        assert_eq!(s("::scroll-button( up)"), "::scroll-button(up)");
    }
}
