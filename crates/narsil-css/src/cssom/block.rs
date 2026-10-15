//! Release 12.10.2026: der CSS-Deklarationsblock (CSSOM §6.7 „CSS
//! declaration block"): Lesen aus Text (`style`-Attribut, `cssText`),
//! `setProperty`/`removeProperty`/`getPropertyValue`/`getPropertyPriority`,
//! und die Serialisierung (§6.7.2 „serialize a CSS declaration block", mit
//! Kurzformen, wo alle Langformen da sind).
//!
//! Der Block haelt nur LANGFORMEN (und Custom Properties); eine Kurzform wird
//! beim Setzen aufgeteilt. Ein Wert mit `var()` in einer Kurzform macht ihre
//! Langformen zu „pending-substitution"-Werten: sie lesen sich leer, die
//! Kurzform liest sich als der Originaltext.

use super::syntax::{self, Kv, Tok};
use super::{eigenschaften, kurz, werte};

/// Hoechstens so viele Deklarationen je Block (M3).
pub const MAX_DEKL: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct Dekl {
    /// Name: Langform (klein) oder Custom Property (wie geschrieben).
    pub name: String,
    /// Der angegebene Wert, kanonisch geschrieben (leer bei `kurz`).
    pub wert: String,
    pub wichtig: bool,
    /// Pending-substitution: (Kurzform, Originaltext mit `var()`).
    pub kurz: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Block {
    pub dekl: Vec<Dekl>,
    /// Release 12.10.2026: Deskriptoren statt Eigenschaften (`@font-face`,
    /// `@page`): jeder Name gilt, der Wert bleibt (normalisiert) roh.
    pub roh: bool,
    /// Waehrend `lesen`: spaetere Angaben ersetzen und ruecken ans Ende.
    lesen_modus: bool,
    /// Release 12.10.2026: der Zusammenhang des Blocks — 0 Stilregel,
    /// [`SEITE`] (`@page`, Randregeln), [`SCHLUESSELBILD`] (`@keyframes`).
    pub kontext: u8,
}

/// Block einer `@page`-Regel (oder ihrer Randregeln).
pub const SEITE: u8 = 1;
/// Block eines Schluesselbilds.
pub const SCHLUESSELBILD: u8 = 2;

/// Release 12.10.2026: Deskriptoren von `@page` (CSS Paged Media 3 §7) und
/// ihre Grammatik.
const SEITEN_DESKRIPTOREN: &[(&str, &str)] = &[
    (
        "size",
        "<length [0,inf]>{1,2} | auto | [ a5 | a4 | a3 | b5 | b4 | jis-b5 | jis-b4 | letter | legal | ledger ] || [ portrait | landscape ]",
    ),
    ("page-orientation", "upright | rotate-left | rotate-right"),
    ("marks", "none | [ crop || cross ]"),
    ("bleed", "auto | <length>"),
];

/// Gilt die Langform `n` im Zusammenhang `kontext`? (CSS Paged Media 3 §4.3:
/// in `@page` nur die Eigenschaften der Seitenbox; CSS Animations 1 §4: in
/// Schluesselbildern keine `animation-*` ausser Zeitfunktion/Zusammensetzung.
/// WPT cssom rule-restrictions, page-descriptors.)
fn im_kontext(kontext: u8, n: &str) -> bool {
    match kontext {
        SEITE => {
            n.starts_with("--")
                || n.starts_with("margin")
                || n.starts_with("padding")
                || n.starts_with("border")
                || n.starts_with("background")
                || n.starts_with("font")
                || n.starts_with("counter-")
                || SEITEN_DESKRIPTOREN.iter().any(|(d, _)| *d == n)
                || matches!(
                    n,
                    "color" | "line-height" | "width" | "height" | "min-width" | "min-height" | "max-width" | "max-height"
                        | "box-decoration-break" | "page" | "content" | "direction" | "writing-mode" | "letter-spacing"
                        | "word-spacing" | "text-align" | "vertical-align" | "visibility" | "opacity"
                )
        }
        SCHLUESSELBILD => !n.starts_with("animation") || matches!(n, "animation-timing-function" | "animation-composition"),
        _ => true,
    }
}

/// Ein `@page`-Deskriptor (geprueft, kanonisch geschrieben) oder `None`.
fn seiten_deskriptor(n: &str, wert: &str) -> Option<String> {
    let (_, g) = SEITEN_DESKRIPTOREN.iter().find(|(d, _)| *d == n)?;
    let kv = syntax::komponenten(wert);
    let kern = syntax::getrimmt(&kv);
    if let [Kv::T(Tok::Ident(w))] = kern
        && werte::ist_css_weit(w)
    {
        return Some(w.to_ascii_lowercase());
    }
    let gr = super::grammatik::lesen(g)?;
    let items: Vec<&Kv> = kern.iter().filter(|k| !k.ist_leer()).collect();
    let s = super::grammatik::abgleichen(&gr, &items)?;
    Some(werte::text(&s))
}

/// Was beim Lesen eines Werts herauskam.
#[derive(Debug, Clone, PartialEq)]
pub enum Gelesen {
    /// Eine Langform (oder Custom Property) mit ihrem Wert.
    Lang(String),
    /// Die Langformen einer Kurzform mit ihren Werten.
    Kurz(Vec<(&'static str, String)>),
    /// Eine Kurzform mit `var()`: Originaltext.
    KurzRoh(String),
}

/// Die Langformen von `all` (CSS Cascade 5 §3.2: alle ausser `direction`,
/// `unicode-bidi` und Custom Properties).
pub fn alle_langformen() -> Vec<&'static str> {
    eigenschaften::LANG
        .iter()
        .map(|e| e.0)
        .filter(|n| !matches!(*n, "direction" | "unicode-bidi") && kurz::langformen(n).is_none())
        .collect()
}

fn langformen(name: &str) -> Option<Vec<&'static str>> {
    if name == "all" {
        return Some(alle_langformen());
    }
    kurz::langformen(name).map(<[&str]>::to_vec)
}

/// Kennt das CSSOM-Modul die (kanonische) Eigenschaft mit Grammatik?
pub fn mit_grammatik(k: &str) -> bool {
    k == "all" || super::ist_lang(k) || kurz::langformen(k).is_some()
}

/// Der kanonische Name einer Eigenschaft, `None` = unbekannt. Eigenschaften
/// ohne Grammatik hier, die die Kaskade kennt, gelten auch (ihr Wert wird
/// dann von der Kaskade geprueft und roh behalten) — sonst gingen echten
/// Seiten Angaben verloren, die vorher ankamen.
/// Release 12.10.2026: die alten WebKit-Formen nehmen eine Zahl als px
/// (`-webkit-perspective: 1000`, `-webkit-transform: perspective(25)`; WPT
/// css-transforms webkit-*-valid.tentative). `calc()` bleibt ungueltig.
fn webkit_zahl(name: &str, wert: &str) -> String {
    fn px(kv: &[Kv], in_perspektive: bool) -> Vec<Kv> {
        kv.iter()
            .map(|k| match k {
                Kv::T(Tok::Zahl(w, _, _)) if in_perspektive => Kv::T(Tok::Dim(*w, false, String::from("px"), *w < 0.0)),
                Kv::F(n, a) if n.eq_ignore_ascii_case("perspective") && syntax::getrimmt(a).len() == 1 => Kv::F(n.clone(), px(a, true)),
                Kv::F(n, a) => Kv::F(n.clone(), a.clone()),
                k => k.clone(),
            })
            .collect()
    }
    if name.eq_ignore_ascii_case("-webkit-perspective") {
        let kv = syntax::komponenten(wert);
        if let [Kv::T(Tok::Zahl(w, _, _))] = syntax::getrimmt(&kv) {
            return format!("{}px", super::zahl::zahl_text(*w));
        }
    } else if name.eq_ignore_ascii_case("-webkit-transform") && wert.to_ascii_lowercase().contains("perspective(") {
        return syntax::kv_text(&px(&syntax::komponenten(wert), false));
    }
    wert.to_string()
}

pub fn name_kanonisch(n: &str) -> Option<String> {
    if n.starts_with("--") {
        return (n.len() > 2 || n == "--").then(|| n.to_string());
    }
    let k = n.to_ascii_lowercase();
    let k = eigenschaften::alias(&k).to_string();
    (mit_grammatik(&k) || crate::wert_gueltig(&k, "inherit").is_some()).then_some(k)
}

/// Ist das (kanonische) `name` eine Kurzform?
pub fn ist_kurz(name: &str) -> bool {
    name == "all" || kurz::langformen(name).is_some()
}

/// Enthaelt der Wert eine Ersetzungsfunktion (`var()`, `env()`, `attr()`,
/// `if()`, `inherit()`)? Dann entscheidet erst der berechnete Wert.
pub fn hat_ersetzung(kv: &[Kv]) -> bool {
    ["var", "env", "attr", "if", "inherit", "random-item", "ident"].iter().any(|f| syntax::enthaelt_funk(kv, f))
}

/// `var()` braucht als erstes Argument einen `--`-Namen (CSS Variables 1 §3).
fn ersetzung_gueltig(kv: &[Kv]) -> bool {
    kv.iter().all(|k| match k {
        Kv::F(n, a) if n.eq_ignore_ascii_case("random-item") => zufall_gueltig(a),
        // Release 12.10.2026: `ident( <declaration-value> )` — nicht leer,
        // ohne Komma und `{}` auf oberster Ebene (CSS Values 5 §8.1; WPT
        // ident-function-parsing).
        Kv::F(n, a) if n.eq_ignore_ascii_case("ident") => {
            !syntax::getrimmt(a).is_empty()
                && !a.iter().any(|k| k.ist_komma() || matches!(k, Kv::B('{', _)))
                && ersetzung_gueltig(a)
        }
        // Release 13.10.2026: with `var()` in its arguments the name or type
        // is only known at computed-value time (WPT css-values
        // attr-argument-grammar `attr(var(--x))`, `attr(data-foo var(--type))`).
        Kv::F(n, a) if n.eq_ignore_ascii_case("attr") => {
            (syntax::enthaelt_funk(a, "var") || super::ersetzen::attr_gueltig(a)) && ersetzung_gueltig(a)
        }
        Kv::F(n, a) if n.eq_ignore_ascii_case("var") || n.eq_ignore_ascii_case("inherit") => {
            let t = syntax::getrimmt(a);
            // Release 14.10.2026: the name may also come from `ident()` (CSS
            // Values 5 §8.1; WPT css-values ident-function-substitution
            // `background-image: var(ident("--" "myname"))`).
            (matches!(t.first(), Some(Kv::T(Tok::Ident(x))) if x.starts_with("--"))
                || matches!(t.first(), Some(Kv::F(f, _)) if f.eq_ignore_ascii_case("ident")))
                && t.get(1..).is_none_or(|r| {
                    let r = syntax::getrimmt(r);
                    r.is_empty() || r[0].ist_komma()
                })
                && ersetzung_gueltig(a)
        }
        Kv::F(_, a) | Kv::B(_, a) => ersetzung_gueltig(a),
        Kv::T(Tok::BadStr | Tok::BadUrl | Tok::ZuR | Tok::ZuE | Tok::ZuG) => false,
        _ => true,
    })
}

/// `random-item(<key>, <item>#)` (CSS Values 5 §9.2): der Schluessel ist
/// `auto`, ein `--`-Name, `element-scoped`/`property-scoped` oder `fixed <zahl>`;
/// ein Eintrag ist leer, ein einzelner `{}`-Block oder Token ohne `!`, `;`, `{}`.
fn zufall_gueltig(a: &[Kv]) -> bool {
    let mut teile: Vec<Vec<&Kv>> = vec![Vec::new()];
    for k in a {
        if k.ist_komma() {
            teile.push(Vec::new());
        } else if !k.ist_leer()
            && let Some(t) = teile.last_mut()
        {
            t.push(k);
        }
    }
    if teile.len() < 2 || teile[0].is_empty() {
        return false;
    }
    let schluessel_ok = teile[0].iter().all(|k| matches!(k, Kv::T(Tok::Ident(_) | Tok::Zahl(..)) | Kv::F(..)));
    schluessel_ok
        && teile[1..].iter().all(|t| match t.as_slice() {
            [Kv::B('{', _)] => true,
            t => t.iter().all(|k| !matches!(k, Kv::B('{', _) | Kv::T(Tok::Delim('!') | Tok::Semikolon | Tok::ZuG | Tok::ZuE | Tok::ZuR))),
        })
}

/// Ein Wert fuer eine Custom Property: jede Tokenfolge ohne kaputte Token
/// und ohne unpaarige Klammern; oberstes `!` ist erlaubt (nicht `!important`,
/// das schneidet der Aufrufer ab).
fn custom_gueltig(kv: &[Kv]) -> bool {
    kv.iter().all(|k| match k {
        Kv::T(Tok::BadStr | Tok::BadUrl | Tok::ZuR | Tok::ZuE | Tok::ZuG | Tok::Semikolon) => false,
        Kv::F(_, a) | Kv::B(_, a) => custom_gueltig_innen(a),
        _ => true,
    })
}

fn custom_gueltig_innen(kv: &[Kv]) -> bool {
    kv.iter().all(|k| match k {
        Kv::T(Tok::BadStr | Tok::BadUrl | Tok::ZuR | Tok::ZuE | Tok::ZuG) => false,
        Kv::F(_, a) | Kv::B(_, a) => custom_gueltig_innen(a),
        _ => true,
    })
}

/// Einen Wert fuer die (kanonische) Eigenschaft `name` lesen. `roh` ist der
/// Originaltext (ohne `!important`), `None` = ungueltig.
pub fn wert_lesen(name: &str, roh: &str) -> Option<Gelesen> {
    let roh = roh.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C'));
    let kv = syntax::komponenten(roh);
    let kern = syntax::getrimmt(&kv);
    if name.starts_with("--") {
        return custom_gueltig(kern).then(|| Gelesen::Lang(roh.to_string()));
    }
    if kern.is_empty() {
        return None;
    }
    // CSS-weite Schluesselwoerter allein (CSS Cascade 5 §7.3).
    if let [Kv::T(Tok::Ident(w))] = kern
        && werte::ist_css_weit(w)
    {
        let w = w.to_ascii_lowercase();
        return Some(match langformen(name) {
            Some(l) => Gelesen::Kurz(l.into_iter().map(|n| (n, w.clone())).collect()),
            None => Gelesen::Lang(w),
        });
    }
    // Release 12.10.2026: Systemschriften (`font: menu`) setzen die
    // Langformen auf Werte, die sich nicht einzeln schreiben lassen — wie
    // eine ausstehende Ersetzung (WPT cssom font-variant-shorthand-serialization).
    if name == "font"
        && let [Kv::T(Tok::Ident(w))] = kern
        && ["caption", "icon", "menu", "message-box", "small-caption", "status-bar"].iter().any(|s| w.eq_ignore_ascii_case(s))
    {
        return Some(Gelesen::KurzRoh(w.to_ascii_lowercase()));
    }
    if hat_ersetzung(kern) {
        if !ersetzung_gueltig(kern) {
            return None;
        }
        // Ein offener `{}`-Block (am Ende des Werts) ist kein gueltiger
        // Eintrag (WPT random-item-invalid: `random-item(auto, {serif)`).
        let offen = roh.chars().filter(|&c| c == '{').count() != roh.chars().filter(|&c| c == '}').count();
        if offen {
            return None;
        }
        return Some(if ist_kurz(name) { Gelesen::KurzRoh(roh.to_string()) } else { Gelesen::Lang(roh.to_string()) });
    }
    if name == "all" {
        return None;
    }
    if !mit_grammatik(name) {
        return (crate::wert_gueltig(name, roh) == Some(true)).then(|| Gelesen::Lang(syntax::kv_text(kern)));
    }
    if let Some(l) = kurz::langformen(name) {
        let v = kurz::lesen(name, kern)?;
        return Some(Gelesen::Kurz(l.iter().copied().zip(v).collect()));
    }
    let s = super::lang_lesen(name, kern)?;
    Some(Gelesen::Lang(werte::text(&s)))
}

/// Release 12.10.2026: die Abbildungslogik fuer die Serialisierung: 0 =
/// physisch, 1 = logisch in Blockrichtung, 2 = logisch in Zeilenrichtung
/// (WPT cssstyledeclaration-csstext: `margin-block` zwischen
/// `margin-inline-start` und `-end` verhindert `margin-inline`).
fn abbildung(n: &str) -> u8 {
    if n.contains("block") {
        1
    } else if n.contains("inline") || n.contains("-start-") || n.contains("-end-") {
        2
    } else {
        0
    }
}

/// Logische Eigenschaftsgruppe (CSS Logical 1 §4): (Gruppe, logisch?).
fn logische_gruppe(n: &str) -> Option<(&'static str, bool)> {
    let physisch = |s: &str| ["top", "right", "bottom", "left"].iter().any(|x| s == *x);
    let logisch = |s: &str| ["block-start", "block-end", "inline-start", "inline-end"].iter().any(|x| s == *x);
    for g in ["margin", "padding", "inset", "scroll-margin", "scroll-padding"] {
        if let Some(r) = n.strip_prefix(g).and_then(|r| r.strip_prefix('-')) {
            if physisch(r) && g != "inset" {
                return Some((g, false));
            }
            if logisch(r) {
                return Some((g, true));
            }
        }
    }
    if physisch(n) {
        return Some(("inset", false));
    }
    for (g, art) in [("border-width", "width"), ("border-style", "style"), ("border-color", "color")] {
        if let Some(r) = n.strip_prefix("border-").and_then(|r| r.strip_suffix(&format!("-{art}"))) {
            if physisch(r) {
                return Some((g, false));
            }
            if logisch(r) {
                return Some((g, true));
            }
        }
    }
    match n {
        "border-top-left-radius" | "border-top-right-radius" | "border-bottom-right-radius" | "border-bottom-left-radius" => Some(("border-radius", false)),
        "border-start-start-radius" | "border-start-end-radius" | "border-end-start-radius" | "border-end-end-radius" => Some(("border-radius", true)),
        "width" | "height" => Some(("size", false)),
        "inline-size" | "block-size" => Some(("size", true)),
        "min-width" | "min-height" => Some(("min-size", false)),
        "min-inline-size" | "min-block-size" => Some(("min-size", true)),
        "max-width" | "max-height" => Some(("max-size", false)),
        "max-inline-size" | "max-block-size" => Some(("max-size", true)),
        "overflow-x" | "overflow-y" => Some(("overflow", false)),
        "overflow-block" | "overflow-inline" => Some(("overflow", true)),
        _ => None,
    }
}

impl Block {
    /// §6.7 „parse a CSS declaration block" (CSS Syntax 3 §5.4.5 „consume a
    /// list of declarations"): ungueltige Deklarationen fallen weg, je Name
    /// gilt die letzte.
    pub fn lesen(text: &str) -> Block {
        Self::lesen_mit(text, false)
    }

    /// Ein Deskriptor-Block (`@font-face`, `@page`, `@property` …): Namen und
    /// Werte ohne Eigenschaftsgrammatik.
    pub fn lesen_roh(text: &str) -> Block {
        Self::lesen_mit(text, true)
    }

    /// Release 12.10.2026: ein Block in einem Zusammenhang ([`SEITE`],
    /// [`SCHLUESSELBILD`]): nur die dort geltenden Eigenschaften.
    pub fn lesen_im(text: &str, kontext: u8) -> Block {
        Self::lesen_mit_kontext(text, false, kontext)
    }

    fn lesen_mit(text: &str, roh: bool) -> Block {
        Self::lesen_mit_kontext(text, roh, 0)
    }

    fn lesen_mit_kontext(text: &str, roh: bool, kontext: u8) -> Block {
        let (z, t) = syntax::tokens_pos(text);
        let mut b = Block { roh, kontext, ..Block::default() };
        let mut i = 0;
        while i < t.len() {
            match &t[i].0 {
                Tok::Leer | Tok::Semikolon => i += 1,
                Tok::At(_) => {
                    // At-Regel bis `;` oder zum Ende ihres Blocks ueberspringen.
                    let mut tiefe = 0i32;
                    while i < t.len() {
                        match t[i].0 {
                            Tok::Semikolon if tiefe == 0 => break,
                            Tok::AufG | Tok::AufR | Tok::AufE | Tok::Funk(_) => tiefe += 1,
                            Tok::ZuG | Tok::ZuR | Tok::ZuE => {
                                tiefe -= 1;
                                if tiefe <= 0 && t[i].0 == Tok::ZuG {
                                    i += 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                }
                _ => {
                    // Bis zum obersten `;` sammeln.
                    let start = i;
                    let mut tiefe = 0i32;
                    while i < t.len() {
                        match t[i].0 {
                            Tok::Semikolon if tiefe == 0 => break,
                            Tok::AufG | Tok::AufR | Tok::AufE | Tok::Funk(_) => tiefe += 1,
                            Tok::ZuG | Tok::ZuR | Tok::ZuE => tiefe = (tiefe - 1).max(0),
                            _ => {}
                        }
                        i += 1;
                    }
                    b.deklaration(&z, &t[start..i]);
                    if b.dekl.len() >= MAX_DEKL {
                        break;
                    }
                }
            }
        }
        b
    }

    fn deklaration(&mut self, z: &[char], t: &[(Tok, usize, usize)]) {
        let Some((Tok::Ident(name), _, _)) = t.first() else { return };
        let mut j = 1;
        while j < t.len() && t[j].0 == Tok::Leer {
            j += 1;
        }
        if t.get(j).map(|x| &x.0) != Some(&Tok::Doppelpunkt) {
            return;
        }
        j += 1;
        // `!important` am Ende (Leerzeichen dazwischen erlaubt).
        let mut ende = t.len();
        while ende > j && t[ende - 1].0 == Tok::Leer {
            ende -= 1;
        }
        let mut wichtig = false;
        if ende >= j + 2
            && let Tok::Ident(w) = &t[ende - 1].0
            && w.eq_ignore_ascii_case("important")
        {
            let mut k = ende - 1;
            while k > j && t[k - 1].0 == Tok::Leer {
                k -= 1;
            }
            if k > j && t[k - 1].0 == Tok::Delim('!') {
                wichtig = true;
                ende = k - 1;
            }
        }
        let roh: String = if j < ende { z[t[j].1..t[ende - 1].2].iter().collect() } else { String::new() };
        // Release 12.10.2026: in Deskriptoren (`@font-face`, `@page` …) gibt es
        // kein Element — `sibling-index()`/`sibling-count()` sind dort ungueltig
        // (CSS Values 5 §11, csswg-drafts#10982; WPT tree-counting
        // sibling-function-descriptors).
        if (self.roh || self.kontext == SEITE) && !name.starts_with("--") {
            let klein = roh.to_ascii_lowercase();
            if klein.contains("sibling-index(") || klein.contains("sibling-count(") {
                return;
            }
        }
        // `@page`-Deskriptoren (`size` ist sonst eine Kurzform) eigens.
        if self.kontext == SEITE {
            let n = name.to_ascii_lowercase();
            if SEITEN_DESKRIPTOREN.iter().any(|(d, _)| *d == n) {
                if let Some(w) = seiten_deskriptor(&n, &roh) {
                    self.lesen_modus = true;
                    self.eine_setzen(&n, w, wichtig, None);
                    self.lesen_modus = false;
                }
                return;
            }
        }
        if self.roh {
            let n = if name.starts_with("--") { name.clone() } else { name.to_ascii_lowercase() };
            let w = syntax::kv_text(&syntax::komponenten(&roh));
            if !w.is_empty() {
                self.eine_setzen(&n, w, wichtig, None);
            }
            return;
        }
        // Ein `{}`-Block im Wert ist nur bei Custom Properties erlaubt.
        let Some(n) = name_kanonisch(name) else { return };
        if !n.starts_with("--") && t[j..ende].iter().any(|x| x.0 == Tok::AufG) {
            return;
        }
        let roh = webkit_zahl(name, &roh);
        if let Some(g) = wert_lesen(&n, &roh) {
            self.lesen_modus = true;
            self.anwenden(&n, g, wichtig);
            self.lesen_modus = false;
        }
    }

    fn finde(&self, n: &str) -> Option<usize> {
        self.dekl.iter().position(|d| d.name == n)
    }

    /// CSSOM „set a CSS declaration" — Rueckgabe: hat sich etwas geaendert?
    fn eine_setzen(&mut self, n: &str, wert: String, wichtig: bool, kurz: Option<(String, String)>) -> bool {
        if !im_kontext(self.kontext, n) {
            return false;
        }
        let neu = Dekl { name: n.to_string(), wert, wichtig, kurz };
        // Beim Lesen eines Blocks gewinnt die spaetere Angabe und rueckt ans
        // Ende — ausser eine wichtige steht schon da (CSS Cascade 5 §6.4;
        // WPT cssom shorthand-values: `border: 1px; border-top: 1px !important`).
        if self.lesen_modus
            && let Some(i) = self.finde(n)
        {
            if self.dekl[i].wichtig && !wichtig {
                return false;
            }
            self.dekl.remove(i);
            self.dekl.push(neu);
            return true;
        }
        if let Some(i) = self.finde(n) {
            // Eine spaetere Angabe derselben logischen Gruppe mit anderer
            // Abbildung: ans Ende (CSSOM §6.7, WPT setter-logical).
            let umstellen = logische_gruppe(n).is_some_and(|(g, l)| {
                self.dekl[i + 1..].iter().any(|d| logische_gruppe(&d.name).is_some_and(|(g2, l2)| g2 == g && l2 != l))
            });
            if umstellen {
                self.dekl.remove(i);
                self.dekl.push(neu);
                return true;
            }
            if self.dekl[i] == neu {
                return false;
            }
            self.dekl[i] = neu;
            return true;
        }
        if self.dekl.len() >= MAX_DEKL {
            return false;
        }
        self.dekl.push(neu);
        true
    }

    fn anwenden(&mut self, n: &str, g: Gelesen, wichtig: bool) -> bool {
        match g {
            Gelesen::Lang(w) => self.eine_setzen(n, w, wichtig, None),
            Gelesen::Kurz(v) => {
                let mut geaendert = false;
                for (l, w) in v {
                    geaendert |= self.eine_setzen(l, w, wichtig, None);
                }
                geaendert
            }
            Gelesen::KurzRoh(roh) => {
                let mut geaendert = false;
                for l in langformen(n).unwrap_or_default() {
                    geaendert |= self.eine_setzen(l, String::new(), wichtig, Some((n.to_string(), roh.clone())));
                }
                geaendert
            }
        }
    }

    /// `setProperty(name, wert, prioritaet)` (CSSOM §6.7). Rueckgabe: geaendert?
    pub fn setzen(&mut self, name: &str, wert: &str, prioritaet: &str) -> bool {
        if self.kontext == SEITE {
            let n = name.trim().to_ascii_lowercase();
            if SEITEN_DESKRIPTOREN.iter().any(|(d, _)| *d == n) {
                if wert.trim().is_empty() {
                    let vorher = self.dekl.len();
                    self.dekl.retain(|d| d.name != n);
                    return self.dekl.len() != vorher;
                }
                let Some(w) = seiten_deskriptor(&n, wert) else { return false };
                return self.eine_setzen(&n, w, prioritaet.eq_ignore_ascii_case("important"), None);
            }
        }
        if self.roh {
            let n = name.trim().to_ascii_lowercase();
            let w = syntax::kv_text(&syntax::komponenten(wert));
            if n.is_empty() {
                return false;
            }
            if w.is_empty() {
                let vorher = self.dekl.len();
                self.dekl.retain(|d| d.name != n);
                return self.dekl.len() != vorher;
            }
            return self.eine_setzen(&n, w, prioritaet.eq_ignore_ascii_case("important"), None);
        }
        let Some(n) = name_kanonisch(name.trim()) else { return false };
        if wert.trim().is_empty() {
            return self.entfernen(&n).1;
        }
        let wichtig = if prioritaet.is_empty() {
            false
        } else if prioritaet.eq_ignore_ascii_case("important") {
            true
        } else {
            return false;
        };
        match wert_lesen(&n, &webkit_zahl(name.trim(), wert)) {
            Some(g) => self.anwenden(&n, g, wichtig),
            None => false,
        }
    }

    /// `removeProperty(name)`: (alter Wert, geaendert?).
    pub fn entfernen(&mut self, name: &str) -> (String, bool) {
        let seite_d = self.kontext == SEITE && SEITEN_DESKRIPTOREN.iter().any(|(d, _)| d.eq_ignore_ascii_case(name.trim()));
        if self.roh || seite_d {
            let n = name.trim().to_ascii_lowercase();
            let alt = self.wert(&n);
            let vorher = self.dekl.len();
            self.dekl.retain(|d| d.name != n);
            return (alt, self.dekl.len() != vorher);
        }
        let Some(n) = name_kanonisch(name.trim()) else { return (String::new(), false) };
        let alt = self.wert(&n);
        let vorher = self.dekl.len();
        match langformen(&n) {
            Some(l) => self.dekl.retain(|d| !l.contains(&d.name.as_str())),
            None => self.dekl.retain(|d| d.name != n),
        }
        let geaendert = self.dekl.len() != vorher;
        (alt, geaendert)
    }

    /// `getPropertyValue(name)`.
    /// Release 12.10.2026: fuer eine Langform aus einer Kurzform mit `var()`
    /// (pending-substitution) die Kurzform und ihr Originaltext.
    pub fn ausstehend(&self, name: &str) -> Option<(String, String)> {
        let n = name_kanonisch(name.trim())?;
        self.finde(&n).and_then(|i| self.dekl[i].kurz.clone())
    }

    pub fn wert(&self, name: &str) -> String {
        if self.kontext == SEITE {
            let n = name.trim().to_ascii_lowercase();
            if SEITEN_DESKRIPTOREN.iter().any(|(d, _)| *d == n) {
                return self.finde(&n).map(|i| self.dekl[i].wert.clone()).unwrap_or_default();
            }
        }
        if self.roh {
            let n = name.trim().to_ascii_lowercase();
            return self.finde(&n).map(|i| self.dekl[i].wert.clone()).unwrap_or_default();
        }
        let Some(n) = name_kanonisch(name.trim()) else { return String::new() };
        if let Some(l) = langformen(&n) {
            return self.kurz_wert(&n, &l).unwrap_or_default();
        }
        match self.finde(&n) {
            Some(i) if self.dekl[i].kurz.is_none() => self.dekl[i].wert.clone(),
            _ => String::new(),
        }
    }

    /// Release 15.10.2026: [`Block::wert`] and [`Block::prioritaet`] for a name
    /// the caller has already made canonical (`name_kanonisch`) with its long
    /// forms (`langformen`), so a loop over thousands of rules does not
    /// canonicalize the same name once per rule. MEASURED (spiegel.de, the
    /// base values of the animation module): `name_kanonisch` per rule was
    /// most of 64 ms per call.
    pub fn wert_und_prioritaet_kanonisch(&self, name: &str, n: &str, l: Option<&[&str]>) -> (String, bool) {
        if self.kontext == SEITE || self.roh {
            return (self.wert(name), self.prioritaet(name) == "important");
        }
        match l {
            Some(l) => {
                let Some(w) = self.kurz_wert(n, l) else { return (String::new(), false) };
                let wichtig = l.iter().all(|x| self.finde(x).is_some_and(|i| self.dekl[i].wichtig));
                (w, wichtig)
            }
            None => match self.finde(n) {
                Some(i) if self.dekl[i].kurz.is_none() => (self.dekl[i].wert.clone(), self.dekl[i].wichtig),
                _ => (String::new(), false),
            },
        }
    }

    fn kurz_wert(&self, n: &str, l: &[&str]) -> Option<String> {
        let mut ds = Vec::new();
        for x in l {
            ds.push(&self.dekl[self.finde(x)?]);
        }
        let w0 = ds.first()?.wichtig;
        if ds.iter().any(|d| d.wichtig != w0) {
            return None;
        }
        // Pending-substitution: alle aus DIESER Kurzform mit demselben Text.
        if let Some((k, roh)) = &ds[0].kurz {
            if k == n && ds.iter().all(|d| d.kurz.as_ref() == Some(&(k.clone(), roh.clone()))) {
                return Some(roh.clone());
            }
            return None;
        }
        if ds.iter().any(|d| d.kurz.is_some()) {
            return None;
        }
        // Lauter dasselbe CSS-weite Wort -> das Wort.
        let w = &ds[0].wert;
        if werte::ist_css_weit(w) {
            return ds.iter().all(|d| d.wert == *w).then(|| w.clone());
        }
        if ds.iter().any(|d| werte::ist_css_weit(&d.wert)) || n == "all" {
            return None;
        }
        // Langformen mit `var()` (selbst gesetzt) lassen sich nicht zusammensetzen.
        if ds.iter().any(|d| hat_ersetzung(&syntax::komponenten(&d.wert))) {
            return None;
        }
        let werte_: Vec<String> = ds.iter().map(|d| d.wert.clone()).collect();
        kurz::schreiben(n, &werte_).filter(|s| !s.is_empty())
    }

    /// `getPropertyPriority(name)`.
    pub fn prioritaet(&self, name: &str) -> &'static str {
        let Some(n) = name_kanonisch(name.trim()) else { return "" };
        let wichtig = match langformen(&n) {
            Some(l) => {
                let ds: Vec<_> = l.iter().filter_map(|x| self.finde(x)).collect();
                ds.len() == l.len() && ds.iter().all(|&i| self.dekl[i].wichtig)
            }
            None => self.finde(&n).is_some_and(|i| self.dekl[i].wichtig),
        };
        if wichtig { "important" } else { "" }
    }

    pub fn laenge(&self) -> usize {
        self.dekl.len()
    }

    /// Release 12.10.2026: der Block als Text fuer den Vorspann (`rule.style`
    /// haelt ihn zwischen zwei Aufrufen): je Deklaration Name, Wert,
    /// Wichtigkeit, Kurzform, Originaltext — getrennt durch U+001F/U+001E.
    pub fn kodieren(&self) -> String {
        let mut s = String::new();
        if self.roh {
            s.push('\u{1d}');
        }
        if self.kontext != 0 {
            s.push('\u{1c}');
            s.push(char::from(b'0' + self.kontext));
        }
        for (i, d) in self.dekl.iter().enumerate() {
            if i > 0 {
                s.push('\u{1e}');
            }
            let (k, r) = d.kurz.clone().unwrap_or_default();
            s.push_str(&format!("{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}", d.name, d.wert, if d.wichtig { "1" } else { "" }, k, r));
        }
        s
    }

    /// Gegenstueck zu [`Self::kodieren`].
    pub fn dekodieren(s: &str) -> Block {
        let mut b = Block::default();
        let s = match s.strip_prefix('\u{1d}') {
            Some(r) => {
                b.roh = true;
                r
            }
            None => s,
        };
        let s = match s.strip_prefix('\u{1c}') {
            Some(r) => {
                let mut z = r.chars();
                b.kontext = z.next().and_then(|c| c.to_digit(10)).unwrap_or(0) as u8;
                z.as_str()
            }
            None => s,
        };
        if s.is_empty() {
            return b;
        }
        for teil in s.split('\u{1e}').take(MAX_DEKL) {
            let f: Vec<&str> = teil.split('\u{1f}').collect();
            if f.len() != 5 {
                continue;
            }
            let kurz = (!f[3].is_empty()).then(|| (f[3].to_string(), f[4].to_string()));
            b.dekl.push(Dekl { name: f[0].to_string(), wert: f[1].to_string(), wichtig: f[2] == "1", kurz });
        }
        b
    }

    pub fn eintrag(&self, i: usize) -> Option<&str> {
        self.dekl.get(i).map(|d| d.name.as_str())
    }

    /// §6.7.2 „serialize a CSS declaration block".
    pub fn text(&self) -> String {
        let mut liste: Vec<String> = Vec::new();
        let mut fertig: Vec<&str> = Vec::new();
        if self.roh {
            return self
                .dekl
                .iter()
                .map(|d| format!("{}: {}{};", d.name, d.wert, if d.wichtig { " !important" } else { "" }))
                .collect::<Vec<_>>()
                .join(" ");
        }
        // `all`: alle Langformen mit demselben CSS-weiten Wort — oder, wenn
        // spaeter einige ueberschrieben wurden (sie stehen dann hinter den
        // anderen), `all: <wort>` vorn und die ueberschriebenen danach. Das
        // ergibt dieselbe Kaskade (WPT cssom all-initial-csstext: `all:
        // initial; display: block` ueber `cssText` und `insertRule`).
        let alle = alle_langformen();
        let mut all_text: Option<String> = None;
        if alle.iter().all(|l| self.finde(l).is_some()) {
            let erster = &self.dekl[self.finde(alle[0]).unwrap_or(0)];
            let (w, wi) = (erster.wert.clone(), erster.wichtig);
            if werte::ist_css_weit(&w) {
                let gleich = |d: &Dekl| d.wert == w && d.wichtig == wi && d.kurz.is_none();
                // Die Stellen der gleichen und der ueberschriebenen Langformen.
                let mut letzte_gleiche = 0usize;
                let mut erste_andere = usize::MAX;
                for (i, d) in self.dekl.iter().enumerate() {
                    if !alle.contains(&d.name.as_str()) {
                        continue;
                    }
                    if gleich(d) {
                        letzte_gleiche = i;
                    } else {
                        erste_andere = erste_andere.min(i);
                    }
                }
                if erste_andere == usize::MAX || letzte_gleiche < erste_andere {
                    all_text = Some(format!("all: {w}{};", if wi { " !important" } else { "" }));
                    for d in &self.dekl {
                        if alle.contains(&d.name.as_str()) && gleich(d) {
                            fertig.push(&d.name);
                        }
                    }
                }
            }
        }
        'aussen: for d in &self.dekl {
            if fertig.contains(&d.name.as_str()) {
                // `all` steht an der Stelle seiner ersten Langform (WPT
                // cssstyledeclaration-csstext-all-shorthand).
                if alle.contains(&d.name.as_str())
                    && let Some(a) = all_text.take()
                {
                    liste.push(a);
                }
                continue;
            }
            if !d.name.starts_with("--") {
                for k in kurz::kurzformen_von(&d.name) {
                    let Some(l) = kurz::langformen(k) else { continue };
                    if l.iter().any(|x| fertig.contains(x) || self.finde(x).is_none()) {
                        continue;
                    }
                    // Steht zwischen den Langformen eine fremde Angabe derselben
                    // logischen Gruppe, aendert die Kurzform die Reihenfolge der
                    // Kaskade — dann nicht (CSSOM §6.7.2, WPT
                    // cssstyledeclaration-csstext „different logical groups").
                    if let Some((gruppe, _)) = logische_gruppe(l[0]) {
                        let stellen: Vec<usize> = l.iter().filter_map(|x| self.finde(x)).collect();
                        let (a, e) = (stellen.iter().min().copied().unwrap_or(0), stellen.iter().max().copied().unwrap_or(0));
                        let eigene: Vec<u8> = l.iter().map(|x| abbildung(x)).collect();
                        let fremd = self.dekl[a..=e].iter().any(|d| {
                            !l.contains(&d.name.as_str())
                                && logische_gruppe(&d.name).is_some_and(|(g, _)| g == gruppe)
                                && eigene.iter().any(|m| *m != abbildung(&d.name))
                        });
                        if fremd {
                            continue;
                        }
                    }
                    let Some(w) = self.kurz_wert(k, l) else { continue };
                    let wichtig = self.dekl[self.finde(l[0]).unwrap_or(0)].wichtig;
                    liste.push(format!("{k}: {w}{};", if wichtig { " !important" } else { "" }));
                    fertig.extend(l.iter().copied());
                    continue 'aussen;
                }
            }
            if d.kurz.is_some() {
                continue;
            }
            // Custom-Property-Namen als Bezeichner geschuetzt (`--a\;b`; CSSOM
            // §6.7.2, WPT variable-names).
            let n = if d.name.starts_with("--") { syntax::ident_text(&d.name) } else { d.name.clone() };
            liste.push(format!("{}: {}{};", n, d.wert, if d.wichtig { " !important" } else { "" }));
            fertig.push(&d.name);
        }
        liste.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_grundlagen() {
        let mut b = Block::default();
        assert!(b.setzen("margin", "1px 2px", ""));
        assert_eq!(b.laenge(), 4);
        assert_eq!(b.wert("margin-left"), "2px");
        assert_eq!(b.wert("margin"), "1px 2px");
        assert_eq!(b.text(), "margin: 1px 2px;");
        assert!(!b.setzen("color", "bogus", ""));
        assert!(b.setzen("color", "RED", "important"));
        assert_eq!(b.text(), "margin: 1px 2px; color: red !important;");
        let (alt, g) = b.entfernen("margin");
        assert_eq!((alt.as_str(), g), ("1px 2px", true));
        assert_eq!(b.text(), "color: red !important;");
        let b = Block::lesen("color: green!important; --X:  a  b ; width: 0; bogus: 1; margin: var(--x)");
        assert_eq!(b.wert("--X"), "a  b");
        assert_eq!(b.wert("width"), "0px");
        assert_eq!(b.wert("margin-top"), "");
        assert_eq!(b.wert("margin"), "var(--x)");
        assert_eq!(b.text(), "color: green !important; --X: a  b; width: 0px; margin: var(--x);");
        let mut b = Block::default();
        b.setzen("border", "1px solid red", "");
        assert_eq!(b.text(), "border: 1px solid red;");
        b.setzen("border-top-width", "2px", "");
        assert_eq!(b.wert("border"), "");
        assert_eq!(b.wert("grid-area"), "");
        let mut b = Block::default();
        b.setzen("grid-area", "a / b", "");
        assert_eq!(b.wert("grid-row-end"), "a");
        assert_eq!(b.wert("grid-area"), "a / b");
        b.setzen("flex", "1", "");
        assert_eq!(b.wert("flex"), "1 1 0%");
        assert_eq!(b.wert("flex-basis"), "0%");
    }
}
