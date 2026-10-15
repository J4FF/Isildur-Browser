//! Release 12.10.2026 (Agent C): `@scope` (CSS Cascade 6 §2.5) fuer die
//! Kaskade — als umgeschriebene Stilregeln.
//!
//! `@scope (A) to (B) { S { … } }` wird zu
//! `:where(A) S:not(:where(A) :where(B), :where(A) :where(B) *) { … }`:
//! der Selektor gilt relativ zur Wurzel (Nachfahre), `:scope` ist die Wurzel
//! (Spezifitaet einer Pseudoklasse), `&` ist `:is(A)`, die Grenze `B` und ihr
//! Inneres sind ausgenommen. Deklarationen direkt im Block gelten der Wurzel
//! (Spezifitaet 0). Die Naehe (scope proximity) bleibt unberuecksichtigt;
//! `@scope` ohne Kopf (implizite Wurzel) faellt weg.
//!
//! GEMESSEN vorher (WPT css-cascade scope-*): `@scope` wurde wie eine
//! unbekannte @-Regel uebersprungen — keine seiner Regeln galt.

use crate::cssom::syntax::{self, Kv, Tok};

/// Hoechstens so viele Teile je Block (M3).
const MAX_TEILE: usize = 4_096;

/// Der Kopf `(A) to (B)`: (Wurzel, Grenze) als Selektortext.
fn kopf(vorspann: &str) -> Option<(String, Option<String>)> {
    let kv = syntax::komponenten(vorspann);
    let t: Vec<&Kv> = kv.iter().filter(|k| !k.ist_leer()).collect();
    let (a, rest) = match t.as_slice() {
        [Kv::B('(', a), rest @ ..] => (syntax::kv_text(syntax::getrimmt(a)), rest),
        _ => return None,
    };
    if a.trim().is_empty() {
        return None;
    }
    match rest {
        [] => Some((a, None)),
        [w, Kv::B('(', b)] if w.ist_wort("to") => {
            let b = syntax::kv_text(syntax::getrimmt(b));
            Some((a, (!b.trim().is_empty()).then_some(b)))
        }
        _ => None,
    }
}

struct Kontext {
    /// `:where(A)`
    wurzel: String,
    /// `:is(A)` (fuer `&`)
    ist: String,
    /// `:-narsil-scope-in(…):-narsil-scope-limit(…)` for the limit, else empty.
    grenze: String,
    /// Release 13.10.2026: without an own limit `:-narsil-scope-in(A)` — for
    /// scope proximity and to keep the subject in scope (`crate::scope_match`).
    proximity: String,
}

impl Kontext {
    /// The suffix for every subject: this scope first, then the outer ones.
    fn subject_suffix(&self) -> String {
        format!("{}{}", self.proximity, self.grenze)
    }
}

/// Einen Selektor der Liste relativ zur Wurzel umschreiben.
/// Release 12.10.2026: kann ein Selektor mit `:scope` je treffen? Die Wurzel
/// muss auf der Ahnenkette des Subjekts liegen: `:scope` in zwei Teilen
/// (`:scope > :scope`) oder direkt vor `+`/`~` (`:scope + .c`) trifft nie
/// (WPT css-cascade scope-evaluation).
fn kann_treffen(s: &[Kv]) -> bool {
    let mut teil = 0usize;
    let mut scope_teile: Vec<usize> = Vec::new();
    let mut nach_teil: Vec<char> = Vec::new();
    let mut i = 0;
    let mut zuletzt_kombinator = true;
    while i < s.len() {
        let x = &s[i];
        let komb = if x.ist_leer() {
            Some(' ')
        } else if x.ist_delim('>') {
            Some('>')
        } else if x.ist_delim('+') {
            Some('+')
        } else if x.ist_delim('~') {
            Some('~')
        } else {
            None
        };
        if let Some(kb) = komb {
            if !zuletzt_kombinator {
                nach_teil.push(kb);
                teil += 1;
            } else if kb != ' '
                && let Some(l) = nach_teil.last_mut()
            {
                *l = kb;
            }
            zuletzt_kombinator = true;
            i += 1;
            continue;
        }
        zuletzt_kombinator = false;
        // Release 13.10.2026: `&` is `:where(:scope)` here (WPT css-cascade
        // scope-import-parent-pseudo: `& > & > .u` never matches).
        if x.ist_delim('&') {
            if !scope_teile.contains(&teil) {
                scope_teile.push(teil);
            }
            i += 1;
            continue;
        }
        if matches!(x, Kv::T(Tok::Doppelpunkt)) && s.get(i + 1).is_some_and(|n| n.ist_wort("scope")) {
            if !scope_teile.contains(&teil) {
                scope_teile.push(teil);
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    if scope_teile.len() > 1 {
        return false;
    }
    match scope_teile.first() {
        Some(&t) => !matches!(nach_teil.get(t), Some('+' | '~')),
        None => true,
    }
}

/// Steht `:scope` oder `&` im Selektor (auch in `:where()`/`:is()`/`:not()`)?
fn hat_bezug(s: &[Kv]) -> bool {
    s.iter().enumerate().any(|(i, x)| {
        x.ist_delim('&')
            || (matches!(x, Kv::T(Tok::Doppelpunkt)) && s.get(i + 1).is_some_and(|n| n.ist_wort("scope")))
            || matches!(x, Kv::F(n, a) if !n.eq_ignore_ascii_case("has") && hat_bezug(a))
    })
}

/// Der Selektortext mit `:scope` (die Wurzel mit der Spezifitaet einer
/// Pseudoklasse) und `&` (`:where(:scope)`), auch in Funktionen. Release
/// 12.10.2026: vorher nur auf oberster Ebene (WPT scope-declarations
/// `:where(:scope)`).
fn bezug_text(s: &[Kv], k: &Kontext) -> String {
    let mut text = String::new();
    let mut i = 0;
    while i < s.len() {
        let x = &s[i];
        if x.ist_delim('&') {
            text.push_str(&k.ist);
            i += 1;
            continue;
        }
        if matches!(x, Kv::T(Tok::Doppelpunkt)) && s.get(i + 1).is_some_and(|n| n.ist_wort("scope")) {
            // Release 13.10.2026: `:is(W, .nie)` instead of `W:not(.nie)` —
            // inside a `:not()` (`.a:not(:scope)`) the nested `:not` was
            // invalid and the rule was dropped (WPT css-cascade scope-nesting).
            text.push_str(":is(");
            text.push_str(&k.wurzel);
            text.push_str(", .narsil-scope-nie)");
            i += 2;
            continue;
        }
        match x {
            Kv::F(n, a) => {
                text.push_str(&syntax::ident_text(n));
                text.push('(');
                text.push_str(&bezug_text(a, k));
                text.push(')');
            }
            x if x.ist_leer() => text.push(' '),
            x => text.push_str(&syntax::kv_text(std::slice::from_ref(x))),
        }
        i += 1;
    }
    text
}

fn selektor(s: &[Kv], k: &Kontext) -> Option<String> {
    let s = syntax::getrimmt(s);
    if !kann_treffen(s) {
        return None;
    }
    let hat_bezug = hat_bezug(s);
    let text = bezug_text(s, k);
    let raus = if hat_bezug { text } else { format!("{} {}", k.wurzel, text) };
    // Release 13.10.2026: with `:scope` in the selector the subject must still
    // be in scope (WPT css-cascade scope-invalidation `:not(:scope)`: an
    // element outside the root matched) — the scope suffix checks that.
    Some(append_to_subject(&raus, &k.subject_suffix()))
}

/// Append `suffix` to the subject of `sel` — before a trailing pseudo-element
/// (`div::before:is(…)` would be invalid).
fn append_to_subject(sel: &str, suffix: &str) -> String {
    let kv = syntax::komponenten(sel);
    let mut pos = None;
    for (i, x) in kv.iter().enumerate() {
        if x.ist_leer() || x.ist_delim('>') || x.ist_delim('+') || x.ist_delim('~') {
            pos = None;
        } else if matches!(x, Kv::T(Tok::Doppelpunkt))
            && kv.get(i + 1).is_some_and(|n| matches!(n, Kv::T(Tok::Doppelpunkt)))
            && pos.is_none()
        {
            pos = Some(i);
        }
    }
    match pos {
        Some(i) => format!("{}{suffix}{}", syntax::kv_text(&kv[..i]), syntax::kv_text(&kv[i..])),
        None => format!("{sel}{suffix}"),
    }
}

fn selektorliste(p: &[Kv], k: &Kontext) -> String {
    let mut teile = Vec::new();
    let mut s = 0;
    for (i, x) in p.iter().enumerate() {
        if x.ist_komma() {
            teile.extend(selektor(&p[s..i], k));
            s = i + 1;
        }
    }
    teile.extend(selektor(&p[s..], k));
    teile.join(", ")
}

/// Den Inhalt eines `@scope`-Blocks (oder einer Gruppe darin) umschreiben.
fn rumpf(kv: &[Kv], k: &Kontext, tiefe: usize) -> String {
    let mut raus = String::new();
    let mut dekl = String::new();
    let mut stueck: Vec<Kv> = Vec::new();
    let mut teile = 0usize;
    for x in kv {
        teile += 1;
        if teile > MAX_TEILE {
            break;
        }
        match x {
            Kv::T(Tok::Semikolon) => {
                let t = syntax::kv_text(syntax::getrimmt(&stueck));
                if !t.is_empty() && !matches!(stueck.iter().find(|y| !y.ist_leer()), Some(Kv::T(Tok::At(_)))) {
                    dekl.push_str(&t);
                    dekl.push_str("; ");
                } else if !t.is_empty() {
                    raus.push_str(&t);
                    raus.push_str(";\n");
                }
                stueck.clear();
            }
            Kv::B('{', innen) => {
                let p = syntax::getrimmt(&stueck).to_vec();
                stueck.clear();
                // Release 12.10.2026: Deklarationen vor einer Regel gelten an
                // ihrer Stelle (Reihenfolge; WPT scope-declarations „zero specificity").
                if !dekl.trim().is_empty() {
                    raus.push_str(&format!("{}{} {{{}}}\n", k.wurzel, k.subject_suffix(), dekl));
                    dekl.clear();
                }
                match p.first() {
                    Some(Kv::T(Tok::At(n))) => {
                        let n = n.to_ascii_lowercase();
                        let kopf = syntax::kv_text(syntax::getrimmt(&p[1..]));
                        if matches!(n.as_str(), "media" | "supports" | "layer" | "container" | "starting-style") && tiefe < 16 {
                            raus.push_str(&format!("@{n} {kopf} {{\n{}}}\n", rumpf(innen, k, tiefe + 1)));
                        } else if n == "scope" && tiefe < 128 {
                            // Release 12.10.2026: verschachteltes @scope — die
                            // innere Wurzel liegt im aeusseren Bereich (CSS
                            // Cascade 6 §2.5.2; WPT scope-evaluation „Nested scopes").
                            if let Some(k2) = innerer(k, &kopf) {
                                raus.push_str(&rumpf(innen, &k2, tiefe + 1));
                            }
                        } else {
                            raus.push_str(&format!("@{n} {kopf} {{{}}}\n", syntax::kv_text(innen)));
                        }
                    }
                    Some(_) => {
                        let liste = selektorliste(&p, k);
                        if liste.is_empty() {
                        } else if innen.iter().any(|x| matches!(x, Kv::B('{', _))) && tiefe < 16 {
                            // Release 13.10.2026: resolve nested rules here
                            // (see `nested_rule_block`).
                            let parent = p
                                .split(|x| x.ist_komma())
                                .map(|s| bezug_text(syntax::getrimmt(s), k))
                                .collect::<Vec<_>>()
                                .join(", ");
                            raus.push_str(&nested_rule_block(&liste, &parent, innen, k, tiefe + 1));
                        } else {
                            raus.push_str(&format!("{} {{{}}}\n", liste, syntax::kv_text(innen)));
                        }
                    }
                    None => {}
                }
            }
            x => stueck.push(x.clone()),
        }
    }
    let t = syntax::kv_text(syntax::getrimmt(&stueck));
    if !t.is_empty() {
        dekl.push_str(&t);
    }
    if !dekl.trim().is_empty() {
        // Deklarationen direkt im Block gelten der Wurzel (`:where(:scope)`).
        raus.push_str(&format!("{}{} {{{}}}\n", k.wurzel, k.subject_suffix(), dekl));
    }
    raus
}

/// Release 13.10.2026: the block of a scoped style rule that contains nested
/// rules. `selector` is the rewritten selector of the rule (for its
/// declarations), `parent` the selector for `&` — WITHOUT the implicit root
/// prefix and without the scope suffix: `&` means the parent rule as written;
/// the nested subject is itself in scope, and `:scope` in it is the root.
/// MEASURED before (WPT css-cascade scope-nesting ":scope within nested and
/// scoped rule"): the block went to nesting unchanged, `:scope` in it was
/// `:root` and `&` required the root above the parent rule.
fn nested_rule_block(selector: &str, parent: &str, block: &[Kv], k: &Kontext, depth: usize) -> String {
    fn flush_declarations(selector: &str, declarations: &mut String, out: &mut String) {
        if !declarations.trim().is_empty() {
            out.push_str(&format!("{selector} {{{declarations}}}\n"));
        }
        declarations.clear();
    }
    let mut out = String::new();
    let mut declarations = String::new();
    let mut pending: Vec<Kv> = Vec::new();
    for x in block.iter().take(MAX_TEILE) {
        match x {
            Kv::T(Tok::Semikolon) => {
                let text = syntax::kv_text(syntax::getrimmt(&pending));
                if !text.is_empty() {
                    declarations.push_str(&text);
                    declarations.push_str("; ");
                }
                pending.clear();
            }
            Kv::B('{', inner) => {
                let prelude = syntax::getrimmt(&pending).to_vec();
                pending.clear();
                flush_declarations(selector, &mut declarations, &mut out);
                match prelude.first() {
                    Some(Kv::T(Tok::At(name))) => {
                        let name = name.to_ascii_lowercase();
                        let head = syntax::kv_text(syntax::getrimmt(&prelude[1..]));
                        if matches!(name.as_str(), "media" | "supports" | "layer" | "container" | "starting-style") && depth < 16 {
                            out.push_str(&format!("@{name} {head} {{\n{}}}\n", nested_rule_block(selector, parent, inner, k, depth + 1)));
                        } else {
                            out.push_str(&format!("{selector} {{@{name} {head} {{{}}}}}\n", syntax::kv_text(inner)));
                        }
                    }
                    Some(_) => {
                        let parts: Vec<String> = prelude
                            .split(|x| x.ist_komma())
                            .map(|s| nested_selector_text(syntax::getrimmt(s), parent, k))
                            .collect();
                        let inner_parent = parts.join(", ");
                        let inner_selector =
                            parts.iter().map(|s| append_to_subject(s, &k.subject_suffix())).collect::<Vec<_>>().join(", ");
                        if depth < 16 {
                            out.push_str(&nested_rule_block(&inner_selector, &inner_parent, inner, k, depth + 1));
                        }
                    }
                    None => {}
                }
            }
            x => pending.push(x.clone()),
        }
    }
    let text = syntax::kv_text(syntax::getrimmt(&pending));
    if !text.is_empty() {
        declarations.push_str(&text);
    }
    flush_declarations(selector, &mut declarations, &mut out);
    out
}

/// A nested selector below `parent`: `&` is `:is(parent)`, without `&` it is
/// prefixed with `:is(parent) `; `:scope` is the root (as in `bezug_text`).
fn nested_selector_text(s: &[Kv], parent: &str, k: &Kontext) -> String {
    fn walk(s: &[Kv], parent: &str, k: &Kontext, text: &mut String) {
        let mut i = 0;
        while i < s.len() {
            let x = &s[i];
            if x.ist_delim('&') {
                text.push_str(&format!(":is({parent})"));
                i += 1;
                continue;
            }
            if matches!(x, Kv::T(Tok::Doppelpunkt)) && s.get(i + 1).is_some_and(|n| n.ist_wort("scope")) {
                text.push_str(&format!(":is({}, .narsil-scope-nie)", k.wurzel));
                i += 2;
                continue;
            }
            match x {
                Kv::F(name, args) => {
                    text.push_str(&syntax::ident_text(name));
                    text.push('(');
                    walk(args, parent, k, text);
                    text.push(')');
                }
                x if x.ist_leer() => text.push(' '),
                x => text.push_str(&syntax::kv_text(std::slice::from_ref(x))),
            }
            i += 1;
        }
    }
    fn has_ampersand(s: &[Kv]) -> bool {
        s.iter().any(|x| x.ist_delim('&') || matches!(x, Kv::F(_, a) if has_ampersand(a)))
    }
    let mut text = String::new();
    walk(s, parent, k, &mut text);
    if has_ampersand(s) { text } else { format!(":is({parent}) {text}") }
}

/// `:scope` und `&` in einem Kopf-Selektor durch die Wurzel des Bereichs.
fn bezug_ersetzen(text: &str, wurzel: &str, ist: &str) -> String {
    let kv = syntax::komponenten(text);
    let mut raus = String::new();
    let mut i = 0;
    while i < kv.len() {
        let x = &kv[i];
        if x.ist_delim('&') {
            raus.push_str(ist);
            i += 1;
            continue;
        }
        if matches!(x, Kv::T(Tok::Doppelpunkt)) && kv.get(i + 1).is_some_and(|n| n.ist_wort("scope")) {
            raus.push_str(wurzel);
            i += 2;
            continue;
        }
        match x {
            Kv::F(n, a) => {
                raus.push_str(n);
                raus.push('(');
                raus.push_str(&bezug_ersetzen(&syntax::kv_text(a), wurzel, ist));
                raus.push(')');
            }
            Kv::B(k, a) if *k == '(' || *k == '[' => {
                raus.push(*k);
                raus.push_str(&bezug_ersetzen(&syntax::kv_text(a), wurzel, ist));
                raus.push(if *k == '(' { ')' } else { ']' });
            }
            x if x.ist_leer() => raus.push(' '),
            x => raus.push_str(&syntax::kv_text(std::slice::from_ref(x))),
        }
        i += 1;
    }
    raus
}

/// Release 13.10.2026: the limit selectors `b` relative to the root
/// (`crate::scope_match`): `:scope` and `&` become `:-narsil-scope-anchor`;
/// without them `:-narsil-scope-anchor ` is prepended (also before a leading
/// combinator).
fn limit_selectors(b: &str) -> String {
    fn replace(s: &[Kv]) -> String {
        let mut text = String::new();
        let mut i = 0;
        while i < s.len() {
            let x = &s[i];
            if x.ist_delim('&') {
                text.push_str(":-narsil-scope-anchor");
                i += 1;
                continue;
            }
            if matches!(x, Kv::T(Tok::Doppelpunkt)) && s.get(i + 1).is_some_and(|n| n.ist_wort("scope")) {
                text.push_str(":-narsil-scope-anchor");
                i += 2;
                continue;
            }
            match x {
                Kv::F(name, args) => {
                    text.push_str(&syntax::ident_text(name));
                    text.push('(');
                    text.push_str(&replace(args));
                    text.push(')');
                }
                x if x.ist_leer() => text.push(' '),
                x => text.push_str(&syntax::kv_text(std::slice::from_ref(x))),
            }
            i += 1;
        }
        text
    }
    let kv = syntax::komponenten(b);
    kv.split(|x| x.ist_komma())
        .map(|s| {
            let s = syntax::getrimmt(s);
            if hat_bezug(s) { replace(s) } else { format!(":-narsil-scope-anchor {}", replace(s)) }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The limit as a subject suffix: `roots` is the root selector list.
fn limit_suffix(roots: &str, b: &str) -> String {
    format!(":-narsil-scope-in({roots}):-narsil-scope-limit({})", limit_selectors(b))
}

/// Der Bereich eines inneren `@scope` im Bereich `k`: seine Wurzeln sind
/// Elemente des aeusseren Bereichs (die aeussere Wurzel eingeschlossen, nicht
/// unter ihrer Grenze); die aeussere Grenze gilt weiter.
fn innerer(k: &Kontext, vorspann: &str) -> Option<Kontext> {
    let (a, b) = kopf(vorspann)?;
    // Release 13.10.2026: a relative start (`@scope (> #child)`) is relative
    // to the outer root (WPT css-cascade scope-nesting).
    let a = syntax::komponenten(&a)
        .split(|x| x.ist_komma())
        .map(|s| {
            let s = syntax::getrimmt(s);
            let t = syntax::kv_text(s);
            if s.first().is_some_and(|x| x.ist_delim('>') || x.ist_delim('+') || x.ist_delim('~')) {
                format!(":scope {t}")
            } else {
                t
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    // Mit `:scope`/`&` im Kopf steht der Bezug darin; sonst ist die innere
    // Wurzel ein Nachfahre der aeusseren (die aeussere selbst nur ueber
    // `:scope`). So kommt die aeussere Wurzel nur EINMAL in den Text — der
    // Text waechst nicht exponentiell (WPT scope-deep: 90 Ebenen).
    let mit_bezug = hat_bezug(&syntax::komponenten(&a));
    let a = bezug_ersetzen(&a, &k.wurzel, &k.ist);
    if a.len() > 65_536 {
        return None;
    }
    // Ohne Grenze und ohne Komma flach weiterschreiben (`:where(X :is(B))`
    // statt `:where(:where(X) :is(B))`): die Schachtelungstiefe der Selektoren
    // bleibt fest.
    let flach = k
        .grenze
        .is_empty()
        .then(|| k.wurzel.strip_prefix(":where(").and_then(|r| r.strip_suffix(')')))
        .flatten()
        .filter(|innen| !syntax::komponenten(innen).iter().any(Kv::ist_komma));
    let wurzel = if mit_bezug {
        format!(":where(:is({a}){g})", g = k.grenze)
    } else if let Some(innen) = flach {
        format!(":where({innen} :is({a}))")
    } else {
        format!(":where({w} :is({a}){g})", w = k.wurzel, g = k.grenze)
    };
    let ist = wurzel.clone();
    let mut grenze = match &b {
        Some(b) => limit_suffix(&wurzel, b),
        None => String::new(),
    };
    grenze.push_str(&k.grenze);
    let proximity = if b.is_none() { format!(":-narsil-scope-in({wurzel})") } else { String::new() };
    Some(Kontext { wurzel, ist, grenze, proximity })
}

/// Release 12.10.2026: `@scope` in einer Stilregel (CSS Cascade 6 §2.5.1):
/// `&` im Kopf ist der Elternselektor, ohne Kopf ist die Wurzel das Element
/// der Elternregel (WPT css-cascade scope-nesting).
pub(crate) fn umschreiben_verschachtelt(eltern: &str, vorspann: &str, rumpf_text: &str) -> Option<String> {
    let ist = format!(":is({eltern})");
    if vorspann.trim().is_empty() {
        return umschreiben(&format!("({ist})"), rumpf_text);
    }
    // Der Anfangsselektor ist relativ zum Elternselektor (wie eine
    // verschachtelte Stilregel: ohne `&` steht `& ` davor).
    let (a, b) = kopf(vorspann)?;
    let kv = syntax::komponenten(&a);
    let teile: Vec<String> = kv
        .split(|k| k.ist_komma())
        .map(|s| {
            let s = syntax::getrimmt(s);
            let t = syntax::kv_text(s);
            if s.iter().any(|k| k.ist_delim('&')) { t } else { format!("& {t}") }
        })
        .collect();
    let a = bezug_ersetzen(&teile.join(", "), &ist, &ist);
    let v = match b {
        Some(b) => format!("({a}) to ({b})"),
        None => format!("({a})"),
    };
    umschreiben(&v, rumpf_text)
}

/// `@scope <vorspann> { <rumpf> }` als gewoehnliche Regeln; `None` = faellt weg.
pub(crate) fn umschreiben(vorspann: &str, rumpf_text: &str) -> Option<String> {
    let (a, b) = kopf(vorspann)?;
    let wurzel = format!(":where({a})");
    // Release 13.10.2026: the limit is tested per root (`crate::scope_match`);
    // `to (:scope)` limits at the root itself — the scope is empty.
    let grenze = match &b {
        Some(b) => limit_suffix(&a, b),
        None => String::new(),
    };
    // Release 12.10.2026: `&` ist im Bereich `:where(:scope)` — ohne
    // Spezifitaet (csswg-drafts#10196; WPT scope-specificity).
    let proximity = if grenze.is_empty() { format!(":-narsil-scope-in({a})") } else { String::new() };
    let k = Kontext { ist: format!(":where({a})"), wurzel, grenze, proximity };
    let kv = syntax::komponenten(rumpf_text);
    Some(rumpf(&kv, &k, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn umschreiben_einfach() {
        let t = umschreiben("(.a)", ":scope { z-index: 1 } .b { color: green }").unwrap();
        assert!(t.contains(":is(:where(.a), .narsil-scope-nie):-narsil-scope-in(.a) {"), "{t}");
        assert!(t.contains(":where(.a) .b:-narsil-scope-in(.a) {"), "{t}");
        let t = umschreiben("(.a) to (.c)", ".b { color: green }").unwrap();
        assert!(t.contains(":where(.a) .b:-narsil-scope-in(.a):-narsil-scope-limit(:-narsil-scope-anchor .c) {"), "{t}");
        let t = umschreiben("(.a) to (& > .b, .c:scope, > .d)", ".x { color: green }").unwrap();
        assert!(t.contains(":-narsil-scope-limit(:-narsil-scope-anchor > .b, .c:-narsil-scope-anchor, :-narsil-scope-anchor > .d) {"), "{t}");
        let t = umschreiben("(.a)", "@media (width > 0px) { .b { color: green } }").unwrap();
        assert!(t.contains("@media"), "{t}");
        assert!(t.contains(":where(.a) .b"), "{t}");
        assert!(umschreiben("", ".b { color: red }").is_none());
        let t = umschreiben("(#main)", " :scope .b { z-index: 1 } ").unwrap();
        assert!(t.contains(":is(:where(#main), .narsil-scope-nie) .b:-narsil-scope-in(#main) {"), "{t}");
        let t = umschreiben("(.a)", "z-index: 1;").unwrap();
        assert!(t.starts_with(":where(.a):-narsil-scope-in(.a) {"), "{t}");
        // Verschachtelt: die innere Wurzel im aeusseren Bereich.
        let t = umschreiben("(.a)", "@scope (.b) { span { color: green } }").unwrap();
        assert!(t.contains(":where(.a :is(.b)) span:-narsil-scope-in(:where(.a :is(.b))) {"), "{t}");
        let t = umschreiben("(.a)", "@scope (:scope > .b) { span { color: green } }").unwrap();
        assert!(t.contains(":where(:is(:where(.a) > .b)) span"), "{t}");
        let t = umschreiben("(.a)", ":where(:scope) { z-index: 1 }").unwrap();
        assert!(t.contains(":where(:is(:where(.a), .narsil-scope-nie)):-narsil-scope-in(.a) {"), "{t}");
        let t = umschreiben("(.a)", ":scope::before { z-index: 1 }").unwrap();
        assert!(t.contains("nie):-narsil-scope-in(.a)::before {"), "{t}");
        assert!(!umschreiben("(.a)", ":scope + .c { color: green } :scope > :scope { color: red }").unwrap().contains("color"));
        assert!(umschreiben("(.a)", ":scope > .c { color: green }").unwrap().contains("color"));
        assert!(!umschreiben("(.a)", "& > & > .u { color: red }").unwrap().contains("color"));
        assert!(umschreiben("(.a)", "& > .w { color: green }").unwrap().contains("color"));
        let t = umschreiben("(.b)", ".a:not(:scope) { & :scope { z-index: 1 } > .c { z-index: 2 } }").unwrap();
        assert!(t.contains(":is(.a:not(:is(:where(.b), .narsil-scope-nie))) :is(:where(.b), .narsil-scope-nie):-narsil-scope-in(.b) {z-index: 1"), "{t}");
        assert!(t.contains(":is(.a:not(:is(:where(.b), .narsil-scope-nie))) > .c:-narsil-scope-in(.b) {z-index: 2"), "{t}");
        let t = umschreiben("(.a)", ".b { @media (width) { z-index: 1; } }").unwrap();
        assert!(t.contains("@media (width) {\n:where(.a) .b:-narsil-scope-in(.a) {z-index: 1; }"), "{t}");
        let t = umschreiben_verschachtelt(".x", "", ".y { color: green }").unwrap();
        assert!(t.contains(":where(:is(.x)) .y:-narsil-scope-in(:is(.x)) {"), "{t}");
        let t = umschreiben_verschachtelt(".x", "(.y .z, :is(.u, .v))", ".q { color: red }").unwrap();
        assert!(t.contains(":where(:is(.x) .y .z, :is(.x) :is(.u, .v)) .q"), "{t}");
    }
}
