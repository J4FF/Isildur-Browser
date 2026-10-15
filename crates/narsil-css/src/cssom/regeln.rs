//! Release 12.10.2026: Regeln eines Stylesheets fuer das CSSOM lesen (CSS
//! Syntax 3 §5.4 „consume a list of rules", „consume a block's contents" mit
//! Verschachtelung) und schreiben (CSSOM §6.4.2 „serialize a CSS rule").
//!
//! Der Vorspann baut daraus die Regelobjekte (`CSSStyleRule`,
//! `CSSMediaRule`, `CSSImportRule`, `CSSLayerBlockRule` …).
//!
//! GEMESSEN vorher (WPT css/cssom, css/css-cascade/parsing): die Regeln wurden
//! im Vorspann mit Klammerzaehlen zerlegt — `@import` ohne `href`/`layerName`,
//! `@media` ohne normalisierte Abfrage, `@layer;` warf nicht, Schluesselbilder
//! hiessen `from` statt `0%`, verschachtelte Regeln fehlten.

use super::block::Block;
use super::selektor::{self, Namensraeume};
use super::syntax::{self, Kv, Tok, ident_text, text_text, url_text};

/// Eine Regel.
#[derive(Debug, Clone, PartialEq)]
pub struct Regel {
    /// `stil`, `media`, `supports`, `import`, `namespace`, `font-face`, `page`,
    /// `keyframes`, `keyframe`, `layer-block`, `layer-statement`, `container`,
    /// `scope`, `starting-style`, `property`, `counter-style`,
    /// `font-feature-values`, `font-palette-values`, `position-try`,
    /// `view-transition`, `nested-declarations`, `margin`, `charset`.
    pub art: &'static str,
    /// Die kanonische Kopfzeile: Selektor, Bedingung, Name, Schluesselzeit …
    pub kopf: String,
    /// Deklarationen (Stilregel, Deskriptoren).
    pub block: Option<Block>,
    /// Kinder (Gruppenregeln, Verschachtelung, Schluesselbilder).
    pub kinder: Option<Vec<Regel>>,
    /// Weitere Felder je Art (`href`, `layer`, `supports`, `medien`, `prefix`,
    /// `uri`, `name`, `start`, `ende` …).
    pub felder: Vec<(&'static str, String)>,
    /// Der Originaltext der Regel (der Vorspann spiegelt unveraenderte Regeln
    /// so, wie sie im Blatt standen, in die Kaskade).
    pub roh: String,
}

/// Wo eine Regelliste steht.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ort {
    /// Oberste Ebene eines Stylesheets.
    Blatt,
    /// In einer Gruppenregel auf oberster Ebene (`@media { … }`).
    Gruppe,
    /// In einer Stilregel oder in einer Gruppenregel darin (Verschachtelung).
    Verschachtelt,
    /// In `@keyframes`.
    Schluesselbilder,
}

struct P<'a> {
    z: &'a [char],
    t: &'a [(Tok, usize, usize)],
    ns: &'a Namensraeume,
    /// Wurde schon eine Regel gelesen, die kein `@import`/`@charset`/`@layer`-
    /// Statement ist? (dann sind `@import` und `@namespace` ungueltig)
    nach_import: bool,
    nach_namespace: bool,
    /// Release 12.10.2026: der naechste Vorfahr (ohne Gruppenregeln) ist eine
    /// Stilregel — dann schreibt sich `> a` als `& > a` (CSS Nesting 1 §5.2;
    /// direkt in `@scope` nicht; WPT css-cascade at-scope-relative-syntax).
    unter_stil: bool,
}

fn tok_kv(t: &[(Tok, usize, usize)]) -> Vec<Kv> {
    let toks: Vec<Tok> = t.iter().map(|x| x.0.clone()).collect();
    syntax::komponenten_aus(&toks)
}

/// Das Ende eines Blocks ab `i` (Token nach `{`); `i` zeigt danach hinter `}`.
fn block_ende(t: &[(Tok, usize, usize)], i: &mut usize) -> usize {
    let mut tiefe = 0i32;
    while *i < t.len() {
        match t[*i].0 {
            Tok::AufG | Tok::AufR | Tok::AufE | Tok::Funk(_) => tiefe += 1,
            Tok::ZuG if tiefe == 0 => {
                let e = *i;
                *i += 1;
                return e;
            }
            Tok::ZuG | Tok::ZuR | Tok::ZuE => tiefe = (tiefe - 1).max(0),
            _ => {}
        }
        *i += 1;
    }
    t.len()
}

/// Kopfzeile bis `;` oder `{` (auf oberster Klammerebene). Rueckgabe: Ende
/// der Kopfzeile, ob ein Block folgt.
fn kopf_ende(t: &[(Tok, usize, usize)], i: &mut usize, semikolon_endet: bool) -> (usize, bool) {
    let mut tiefe = 0i32;
    while *i < t.len() {
        match t[*i].0 {
            Tok::AufG if tiefe == 0 => {
                let e = *i;
                *i += 1;
                return (e, true);
            }
            Tok::Semikolon if tiefe == 0 && semikolon_endet => {
                let e = *i;
                *i += 1;
                return (e, false);
            }
            Tok::AufR | Tok::AufE | Tok::Funk(_) | Tok::AufG => tiefe += 1,
            Tok::ZuR | Tok::ZuE | Tok::ZuG => tiefe = (tiefe - 1).max(0),
            _ => {}
        }
        *i += 1;
    }
    (t.len(), false)
}

fn ist_name(k: &Kv) -> Option<String> {
    k.ident().map(str::to_string)
}

/// `@layer`-Namen (`a.b.c`): ein Name je Komma-Teil.
fn layer_namen(kv: &[Kv]) -> Option<Vec<String>> {
    let mut raus = Vec::new();
    for teil in kv.split(|k| k.ist_komma()) {
        let t: Vec<&Kv> = teil.iter().filter(|k| !k.ist_leer()).collect();
        // Leerzeichen innerhalb eines Namens sind nicht erlaubt.
        let ohne_rand = syntax::getrimmt(teil);
        if ohne_rand.iter().any(Kv::ist_leer) {
            return None;
        }
        let mut name = String::new();
        let mut erwarte_name = true;
        for k in &t {
            if erwarte_name {
                let n = ist_name(k)?;
                if super::werte::ist_css_weit(&n) {
                    return None;
                }
                name.push_str(&ident_text(&n));
                erwarte_name = false;
            } else if k.ist_delim('.') {
                name.push('.');
                erwarte_name = true;
            } else {
                return None;
            }
        }
        if name.is_empty() || erwarte_name {
            return None;
        }
        raus.push(name);
    }
    Some(raus)
}

/// `<keyframe-selector>#` -> kanonische Schluesselzeiten (`from` -> `0%`).
fn schluessel(kv: &[Kv]) -> Option<String> {
    let mut raus = Vec::new();
    for teil in kv.split(|k| k.ist_komma()) {
        let t: Vec<&Kv> = teil.iter().filter(|k| !k.ist_leer()).collect();
        match t.as_slice() {
            [k] if k.ist_wort("from") => raus.push(String::from("0%")),
            [k] if k.ist_wort("to") => raus.push(String::from("100%")),
            [Kv::T(Tok::Prozent(w, _))] if (0.0..=100.0).contains(w) => raus.push(format!("{}%", super::zahl::zahl_text(*w))),
            // Zeitleisten-Bereiche (`entry 10%`, `cover 50%`)
            [Kv::T(Tok::Ident(n)), Kv::T(Tok::Prozent(w, _))]
                if ["entry", "exit", "cover", "contain", "entry-crossing", "exit-crossing"].iter().any(|x| n.eq_ignore_ascii_case(x)) =>
            {
                raus.push(format!("{} {}%", n.to_ascii_lowercase(), super::zahl::zahl_text(*w)))
            }
            _ => return None,
        }
    }
    Some(raus.join(", "))
}

/// Release 12.10.2026: eine Schluesselzeit-Liste als Text kanonisch
/// (`CSSKeyframeRule.keyText`, `findRule`), `None` = ungueltig.
pub fn schluessel_text(text: &str) -> Option<String> {
    let kv = syntax::komponenten(text);
    schluessel(syntax::getrimmt(&kv))
}

/// Die Bedingung von `@supports` als Text (Leerzeichen normalisiert).
fn bedingung_text(kv: &[Kv]) -> String {
    syntax::kv_text(kv)
}

impl P<'_> {
    fn text(&self, a: usize, b: usize) -> String {
        if a >= b || a >= self.t.len() {
            return String::new();
        }
        let von = self.t[a].1;
        let bis = self.t[(b - 1).min(self.t.len() - 1)].2;
        self.z[von..bis].iter().collect()
    }

    /// Eine Regelliste ab `i` bis `ende` (Tokenindex).
    fn liste(&mut self, i: &mut usize, ende: usize, ort: Ort) -> Vec<Regel> {
        let mut raus = Vec::new();
        while *i < ende {
            match &self.t[*i].0 {
                Tok::Leer | Tok::Semikolon => *i += 1,
                Tok::Cdo | Tok::Cdc if ort == Ort::Blatt => *i += 1,
                Tok::At(_) => {
                    if let Some(r) = self.at_regel(i, ende, ort) {
                        raus.push(r);
                    }
                }
                _ => {
                    if let Some(r) = self.qualifiziert(i, ende, ort) {
                        raus.push(r);
                    }
                }
            }
            if raus.len() > super::block::MAX_DEKL * 4 {
                break;
            }
        }
        raus
    }

    fn qualifiziert(&mut self, i: &mut usize, ende: usize, ort: Ort) -> Option<Regel> {
        let start = *i;
        let mut r = self.qualifiziert_innen(i, ende, ort)?;
        r.roh = self.text(start, *i);
        Some(r)
    }

    fn qualifiziert_innen(&mut self, i: &mut usize, ende: usize, ort: Ort) -> Option<Regel> {
        let start = *i;
        let mut j = *i;
        let sub = &self.t[..ende];
        let (k_ende, block) = kopf_ende(sub, &mut j, ort == Ort::Verschachtelt);
        *i = j;
        if !block {
            return None;
        }
        let b_start = j;
        let mut k = j;
        let b_ende = block_ende(sub, &mut k);
        *i = k;
        let kopf = tok_kv(&self.t[start..k_ende]);
        if ort == Ort::Schluesselbilder {
            let s = schluessel(syntax::getrimmt(&kopf))?;
            let block = Block::lesen_im(&self.text(b_start, b_ende), super::block::SCHLUESSELBILD);
            return Some(Regel { art: "keyframe", kopf: s, block: Some(block), kinder: None, felder: Vec::new(), roh: String::new() });
        }
        self.nach_import = true;
        self.nach_namespace = true;
        // Der Originaltext: `kv_text` schriebe `1.0` als `1` (`:heading(1.0)` ist ungueltig).
        let kopf_text = self.text(start, k_ende);
        let sel = if ort == Ort::Verschachtelt {
            // Verschachtelte Selektoren sind relativ (`> a`, `&.b`).
            let s = selektor::liste_relativ(&kopf_text, self.ns)?;
            if self.unter_stil {
                s.split(", ")
                    .map(|x| if x.starts_with(['>', '+', '~']) { format!("& {x}") } else { x.to_string() })
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                s
            }
        } else {
            selektor::liste(&kopf_text, self.ns)?
        };
        let (block, kinder) = self.stil_inhalt(b_start, b_ende);
        Some(Regel { art: "stil", kopf: sel, block: Some(block), kinder: Some(kinder), felder: Vec::new(), roh: String::new() })
    }

    /// Inhalt einer Stilregel: Deklarationen und verschachtelte Regeln (CSS
    /// Nesting 1 §3; Deklarationen nach einer Regel werden
    /// `CSSNestedDeclarations`).
    fn stil_inhalt(&mut self, a: usize, e: usize) -> (Block, Vec<Regel>) {
        let alt = std::mem::replace(&mut self.unter_stil, true);
        let r = self.stil_inhalt_innen(a, e);
        self.unter_stil = alt;
        r
    }

    fn stil_inhalt_innen(&mut self, a: usize, e: usize) -> (Block, Vec<Regel>) {
        let mut dekl_text = String::new();
        let mut kinder: Vec<Regel> = Vec::new();
        let mut nach_regel = String::new();
        let mut i = a;
        while i < e {
            match &self.t[i].0 {
                Tok::Leer | Tok::Semikolon => i += 1,
                Tok::At(_) => {
                    if !nach_regel.trim().is_empty() {
                        kinder.push(Regel { art: "nested-declarations", kopf: String::new(), block: Some(Block::lesen(&nach_regel)), kinder: None, felder: Vec::new(), roh: String::new() });
                        nach_regel.clear();
                    }
                    if let Some(r) = self.at_regel(&mut i, e, Ort::Verschachtelt) {
                        kinder.push(r);
                    }
                }
                _ => {
                    // Bis `;` oder `{` — `{` vorher: verschachtelte Regel.
                    let start = i;
                    let mut j = i;
                    let (k_ende, block) = kopf_ende(&self.t[..e], &mut j, true);
                    if block {
                        // Eine Deklaration mit `{}` im Wert (`--x: {a}`) ist keine Regel.
                        let ist_dekl = matches!(&self.t[start].0, Tok::Ident(n) if n.starts_with("--"))
                            && self.t[start + 1..k_ende].iter().find(|x| x.0 != Tok::Leer).is_some_and(|x| x.0 == Tok::Doppelpunkt);
                        if !ist_dekl {
                            if !nach_regel.trim().is_empty() {
                                kinder.push(Regel { art: "nested-declarations", kopf: String::new(), block: Some(Block::lesen(&nach_regel)), kinder: None, felder: Vec::new(), roh: String::new() });
                                nach_regel.clear();
                            }
                            i = start;
                            if let Some(r) = self.qualifiziert(&mut i, e, Ort::Verschachtelt) {
                                kinder.push(r);
                            }
                            continue;
                        }
                        // Bis zum Ende des Blocks und dann bis `;`.
                        let mut k = j;
                        block_ende(&self.t[..e], &mut k);
                        let mut m = k;
                        let _ = kopf_ende(&self.t[..e], &mut m, true);
                        let s = self.text(start, m.min(e));
                        if kinder.is_empty() { dekl_text.push_str(&s) } else { nach_regel.push_str(&s) }
                        if kinder.is_empty() { dekl_text.push(';') } else { nach_regel.push(';') }
                        i = m;
                        continue;
                    }
                    let s = self.text(start, k_ende);
                    if kinder.is_empty() {
                        dekl_text.push_str(&s);
                        dekl_text.push(';');
                    } else {
                        nach_regel.push_str(&s);
                        nach_regel.push(';');
                    }
                    i = j;
                }
            }
        }
        if !nach_regel.trim().is_empty() {
            let b = Block::lesen(&nach_regel);
            if b.laenge() > 0 {
                kinder.push(Regel { art: "nested-declarations", kopf: String::new(), block: Some(b), kinder: None, felder: Vec::new(), roh: String::new() });
            }
        }
        (Block::lesen(&dekl_text), kinder)
    }

    fn at_regel(&mut self, i: &mut usize, ende: usize, ort: Ort) -> Option<Regel> {
        let start = *i;
        let mut r = self.at_regel_innen(i, ende, ort)?;
        r.roh = self.text(start, *i);
        Some(r)
    }

    fn at_regel_innen(&mut self, i: &mut usize, ende: usize, ort: Ort) -> Option<Regel> {
        let Tok::At(name) = &self.t[*i].0 else { return None };
        let name = name.to_ascii_lowercase();
        *i += 1;
        let k_start = *i;
        let mut j = *i;
        let (k_ende, block) = kopf_ende(&self.t[..ende], &mut j, true);
        *i = j;
        let (b_start, b_ende) = if block {
            let mut k = j;
            let e = block_ende(&self.t[..ende], &mut k);
            *i = k;
            (j, e)
        } else {
            (0, 0)
        };
        let kopf = tok_kv(&self.t[k_start..k_ende]);
        let kopf = syntax::getrimmt(&kopf).to_vec();
        let gruppe_ort = if ort == Ort::Verschachtelt { Ort::Verschachtelt } else { Ort::Gruppe };
        let regel = |art: &'static str, kopf: String| Regel { art, kopf, block: None, kinder: None, felder: Vec::new(), roh: String::new() };
        match name.as_str() {
            "charset" => None,
            "import" => {
                if block || ort != Ort::Blatt || self.nach_import {
                    return None;
                }
                let mut r = self.import(&kopf)?;
                r.art = "import";
                Some(r)
            }
            "namespace" => {
                if block || ort != Ort::Blatt || self.nach_namespace {
                    return None;
                }
                self.nach_import = true;
                let t: Vec<&Kv> = kopf.iter().filter(|k| !k.ist_leer()).collect();
                let (prefix, uri) = match t.as_slice() {
                    [u] => (String::new(), url_von(u)?),
                    [Kv::T(Tok::Ident(p)), u] => (p.clone(), url_von(u)?),
                    _ => return None,
                };
                let mut r = regel("namespace", String::new());
                r.felder = vec![("prefix", prefix), ("uri", uri)];
                Some(r)
            }
            "media" => {
                if !block {
                    return None;
                }
                self.nach_import = true;
                self.nach_namespace = true;
                let medien = super::medien::liste(&syntax::kv_text(&kopf));
                let mut r = regel("media", medien.join(", "));
                let mut k = b_start;
                r.kinder = Some(self.inhalt(&mut k, b_ende, gruppe_ort));
                r.felder = vec![("medien", medien.join("\u{1f}"))];
                Some(r)
            }
            "supports" => {
                if !block || kopf.is_empty() {
                    return None;
                }
                // Die Bedingung muss sich lesen lassen (sonst ungueltige Regel).
                let sel = |s: &str| super::selektor_gueltig(s);
                super::bedingung::bedingung(&kopf, &sel)?;
                self.nach_import = true;
                self.nach_namespace = true;
                let mut r = regel("supports", bedingung_text(&kopf));
                let mut k = b_start;
                r.kinder = Some(self.inhalt(&mut k, b_ende, gruppe_ort));
                Some(r)
            }
            "layer" => {
                if block {
                    let namen = if kopf.is_empty() { Vec::new() } else { layer_namen(&kopf)? };
                    if namen.len() > 1 {
                        return None;
                    }
                    self.nach_import = true;
                    self.nach_namespace = true;
                    let mut r = regel("layer-block", namen.first().cloned().unwrap_or_default());
                    let mut k = b_start;
                    r.kinder = Some(self.inhalt(&mut k, b_ende, gruppe_ort));
                    Some(r)
                } else {
                    if kopf.is_empty() {
                        return None;
                    }
                    let namen = layer_namen(&kopf)?;
                    let mut r = regel("layer-statement", namen.join(", "));
                    r.felder = vec![("namen", namen.join("\u{1f}"))];
                    Some(r)
                }
            }
            "container" => {
                if !block || kopf.is_empty() {
                    return None;
                }
                self.nach_import = true;
                self.nach_namespace = true;
                let t: Vec<&Kv> = kopf.iter().filter(|k| !k.ist_leer()).collect();
                let (name, rest_ab) = match t.first() {
                    Some(Kv::T(Tok::Ident(n))) if !n.eq_ignore_ascii_case("not") && !n.eq_ignore_ascii_case("style") => (ident_text(n), 1),
                    _ => (String::new(), 0),
                };
                let rest: Vec<Kv> = t[rest_ab..].iter().map(|k| (*k).clone()).collect();
                let bed = syntax::kv_text(&rest);
                let kopf_text = if name.is_empty() { bed.clone() } else if bed.is_empty() { name.clone() } else { format!("{name} {bed}") };
                let mut r = regel("container", kopf_text);
                r.felder = vec![("name", name), ("bedingung", bed)];
                let mut k = b_start;
                r.kinder = Some(self.inhalt(&mut k, b_ende, gruppe_ort));
                Some(r)
            }
            "scope" => {
                if !block {
                    return None;
                }
                let t: Vec<&Kv> = kopf.iter().filter(|k| !k.ist_leer()).collect();
                let mut start = None;
                let mut ziel = None;
                let mut idx = 0;
                if let Some(Kv::B('(', a)) = t.first() {
                    start = Some(selektor::liste_scope(&syntax::kv_text(a), self.ns, false)?);
                    idx = 1;
                }
                if t.get(idx).is_some_and(|k| k.ist_wort("to")) {
                    let Some(Kv::B('(', a)) = t.get(idx + 1) else { return None };
                    ziel = Some(selektor::liste_scope(&syntax::kv_text(a), self.ns, true)?);
                    idx += 2;
                }
                if idx != t.len() {
                    return None;
                }
                self.nach_import = true;
                self.nach_namespace = true;
                let mut kopf_text = String::new();
                if let Some(s) = &start {
                    kopf_text.push_str(&format!("({s})"));
                }
                if let Some(z) = &ziel {
                    if !kopf_text.is_empty() {
                        kopf_text.push(' ');
                    }
                    kopf_text.push_str(&format!("to ({z})"));
                }
                let mut r = regel("scope", kopf_text);
                r.felder = vec![("start", start.unwrap_or_default()), ("ende", ziel.unwrap_or_default())];
                let mut k = b_start;
                // Im @scope stehen Deklarationen und relative Regeln.
                let alt = std::mem::replace(&mut self.unter_stil, false);
                r.kinder = Some(self.inhalt(&mut k, b_ende, Ort::Verschachtelt));
                self.unter_stil = alt;
                Some(r)
            }
            "starting-style" => {
                if !block || !kopf.is_empty() {
                    return None;
                }
                self.nach_import = true;
                self.nach_namespace = true;
                let mut r = regel("starting-style", String::new());
                let mut k = b_start;
                r.kinder = Some(self.inhalt(&mut k, b_ende, gruppe_ort));
                Some(r)
            }
            "keyframes" | "-webkit-keyframes" => {
                if !block || ort == Ort::Verschachtelt {
                    return None;
                }
                let t: Vec<&Kv> = kopf.iter().filter(|k| !k.ist_leer()).collect();
                let name = match t.as_slice() {
                    [Kv::T(Tok::Ident(n))] if !super::werte::ist_css_weit(n) && !n.eq_ignore_ascii_case("none") && !n.eq_ignore_ascii_case("default") => ident_text(n),
                    // Release 15.10.2026: not an empty string (WPT css-animations
                    // keyframes-name-invalid).
                    [Kv::T(Tok::Str(s))] if !s.is_empty() => text_text(s),
                    _ => return None,
                };
                self.nach_import = true;
                self.nach_namespace = true;
                let mut r = regel("keyframes", name);
                let mut k = b_start;
                r.kinder = Some(self.liste(&mut k, b_ende, Ort::Schluesselbilder));
                Some(r)
            }
            "font-face" | "page" | "property" | "counter-style" | "font-palette-values" | "position-try" | "view-transition"
            | "font-feature-values" | "top-left-corner" | "top-left" | "top-center" | "top-right" | "top-right-corner"
            | "bottom-left-corner" | "bottom-left" | "bottom-center" | "bottom-right" | "bottom-right-corner" | "left-top"
            | "left-middle" | "left-bottom" | "right-top" | "right-middle" | "right-bottom" => {
                if !block {
                    return None;
                }
                if ort == Ort::Verschachtelt && !name.contains('-') {
                    return None;
                }
                self.nach_import = true;
                self.nach_namespace = true;
                let art: &'static str = match name.as_str() {
                    "font-face" => "font-face",
                    "page" => "page",
                    "property" => "property",
                    "counter-style" => "counter-style",
                    "font-palette-values" => "font-palette-values",
                    "position-try" => "position-try",
                    "view-transition" => "view-transition",
                    "font-feature-values" => "font-feature-values",
                    _ => "margin",
                };
                let kopf_text = syntax::kv_text(&kopf);
                match art {
                    "font-face" | "view-transition" if !kopf.is_empty() => return None,
                    "property" if !(kopf.len() == 1 && kopf[0].ident().is_some_and(|n| n.starts_with("--"))) => return None,
                    "counter-style"
                        if !(kopf.len() == 1
                            && kopf[0].ident().is_some_and(|n| !n.eq_ignore_ascii_case("none") && !super::werte::ist_css_weit(n))) =>
                    {
                        return None;
                    }
                    _ => {}
                }
                let mut r = regel(art, if art == "margin" { name.clone() } else { kopf_text });
                // Release 12.10.2026: `@page` und Randregeln mit Eigenschaften
                // der Seitenbox und ihren Deskriptoren (WPT page-descriptors).
                r.block = Some(if matches!(art, "page" | "margin") {
                    Block::lesen_im(&self.text(b_start, b_ende), super::block::SEITE)
                } else {
                    Block::lesen_roh(&self.text(b_start, b_ende))
                });
                if art == "font-feature-values" {
                    r.felder = vec![("werte", super::merkmalwerte::kodieren(&self.text(b_start, b_ende)))];
                }
                Some(r)
            }
            _ => None,
        }
    }

    /// Inhalt einer Gruppenregel: verschachtelt mit Deklarationen, sonst eine Regelliste.
    fn inhalt(&mut self, i: &mut usize, ende: usize, ort: Ort) -> Vec<Regel> {
        if ort == Ort::Verschachtelt {
            // Gruppen und @scope erben bzw. setzen `unter_stil` selbst.
            let (block, mut kinder) = self.stil_inhalt_innen(*i, ende);
            *i = ende;
            if block.laenge() > 0 {
                kinder.insert(0, Regel { art: "nested-declarations", kopf: String::new(), block: Some(block), kinder: None, felder: Vec::new(), roh: String::new() });
            }
            return kinder;
        }
        self.liste(i, ende, ort)
    }

    fn import(&mut self, kopf: &[Kv]) -> Option<Regel> {
        let t: Vec<&Kv> = kopf.iter().filter(|k| !k.ist_leer()).collect();
        let href = url_von(t.first()?)?;
        let mut idx = 1;
        let mut layer: Option<String> = None;
        let mut supports: Option<String> = None;
        if let Some(k) = t.get(idx) {
            if k.ist_wort("layer") {
                layer = Some(String::new());
                idx += 1;
            } else if let Kv::F(n, a) = k
                && n.eq_ignore_ascii_case("layer")
                && let Some(namen) = layer_namen(syntax::getrimmt(a))
                && namen.len() == 1
            {
                layer = namen.into_iter().next();
                idx += 1;
            }
        }
        if let Some(Kv::F(n, a)) = t.get(idx)
            && n.eq_ignore_ascii_case("supports")
        {
            let a = syntax::getrimmt(a);
            // `supports(<supports-condition> | <declaration>)`
            let sel = |s: &str| super::selektor_gueltig(s);
            let ok = super::bedingung::bedingung(a, &sel).is_some() || super::bedingung::bedingung_text(&syntax::kv_text(a), &sel);
            let _ = ok;
            supports = Some(syntax::kv_text(a));
            idx += 1;
        }
        // Release 14.10.2026: the media query keeps its whitespace (WPT cssom
        // CSSImportRule-layer-media: `screen and (color)` became
        // `screenand(color)`).
        let start = kopf.iter().enumerate().filter(|(_, k)| !k.ist_leer()).nth(idx).map_or(kopf.len(), |(i, _)| i);
        let rest: Vec<Kv> = kopf[start..].to_vec();
        let medien = super::medien::liste(&syntax::kv_text(&rest));
        let mut kopf_text = url_text(&href);
        if let Some(l) = &layer {
            kopf_text.push_str(if l.is_empty() { " layer" } else { " layer(" });
            if !l.is_empty() {
                kopf_text.push_str(l);
                kopf_text.push(')');
            }
        }
        if let Some(s) = &supports {
            kopf_text.push_str(&format!(" supports({s})"));
        }
        if !medien.is_empty() {
            kopf_text.push(' ');
            kopf_text.push_str(&medien.join(", "));
        }
        let mut felder = vec![("href", href), ("medien", medien.join("\u{1f}"))];
        if let Some(l) = layer {
            felder.push(("layer", l));
        }
        if let Some(s) = supports {
            felder.push(("supports", s));
        }
        Some(Regel { art: "import", kopf: kopf_text, block: None, kinder: None, felder, roh: String::new() })
    }
}

fn url_von(k: &Kv) -> Option<String> {
    match k {
        Kv::T(Tok::Url(u)) | Kv::T(Tok::Str(u)) => Some(u.clone()),
        Kv::F(n, a) if n.eq_ignore_ascii_case("url") => match syntax::getrimmt(a) {
            [Kv::T(Tok::Str(u))] => Some(u.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// Alle Regeln eines Stylesheets.
pub fn blatt_lesen(text: &str, ns: &Namensraeume) -> Vec<Regel> {
    let (z, t) = syntax::tokens_pos(text);
    let mut ns_eigen = ns.clone();
    // Zwei Laeufe: zuerst die `@namespace`-Regeln (fuer die Selektoren).
    {
        let mut p = P { z: &z, t: &t, ns: ns, nach_import: false, nach_namespace: false, unter_stil: false };
        let mut i = 0;
        for r in p.liste(&mut i, t.len(), Ort::Blatt) {
            if r.art == "namespace" {
                let prefix = r.felder.iter().find(|f| f.0 == "prefix").map(|f| f.1.clone()).unwrap_or_default();
                let uri = r.felder.iter().find(|f| f.0 == "uri").map(|f| f.1.clone()).unwrap_or_default();
                if prefix.is_empty() {
                    ns_eigen.standard = Some(uri);
                } else {
                    ns_eigen.praefixe.push((prefix, uri));
                }
            }
        }
    }
    let mut p = P { z: &z, t: &t, ns: &ns_eigen, nach_import: false, nach_namespace: false, unter_stil: false };
    let mut i = 0;
    p.liste(&mut i, t.len(), Ort::Blatt)
}

/// Die Fehler von `insertRule` (CSSOM §6.4.4 „insert a CSS rule").
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fehler {
    Syntax,
    Hierarchie,
}

/// Genau EINE Regel lesen (CSSOM „parse a CSS rule"); `ort` und ob davor
/// schon Nicht-Import- bzw. Nicht-Namespace-Regeln stehen.
pub fn regel_lesen(text: &str, ns: &Namensraeume, ort: Ort, nach_import: bool, nach_namespace: bool) -> Result<Regel, Fehler> {
    regel_lesen_mit(text, ns, ort, nach_import, nach_namespace, false)
}

/// Wie `regel_lesen`; `unter_stil`: der naechste Vorfahr (ohne Gruppen)
/// ist eine Stilregel (dann `> a` → `& > a`).
pub fn regel_lesen_mit(
    text: &str,
    ns: &Namensraeume,
    ort: Ort,
    nach_import: bool,
    nach_namespace: bool,
    unter_stil: bool,
) -> Result<Regel, Fehler> {
    let (z, t) = syntax::tokens_pos(text);
    let mut p = P { z: &z, t: &t, ns, nach_import: false, nach_namespace: false, unter_stil };
    let mut i = 0;
    while i < t.len() && t[i].0 == Tok::Leer {
        i += 1;
    }
    if i >= t.len() {
        return Err(Fehler::Syntax);
    }
    let r = if matches!(t[i].0, Tok::At(_)) { p.at_regel(&mut i, t.len(), ort) } else { p.qualifiziert(&mut i, t.len(), ort) };
    while i < t.len() && t[i].0 == Tok::Leer {
        i += 1;
    }
    // Release 13.10.2026: in a nested context a declaration list becomes a
    // `CSSNestedDeclarations` (CSSOM §6.4.3 "insert a CSS rule", nested; WPT
    // css-cascade scope-nesting `insertRule('color: green')` in @scope).
    if (r.is_none() || i < t.len()) && ort == Ort::Verschachtelt && !t.iter().any(|x| matches!(x.0, Tok::AufG)) {
        let declarations = Block::lesen(text);
        if declarations.laenge() > 0 {
            return Ok(Regel { art: "nested-declarations", kopf: String::new(), block: Some(declarations), kinder: None, felder: Vec::new(), roh: String::new() });
        }
    }
    if i < t.len() {
        return Err(Fehler::Syntax);
    }
    let r = r.ok_or(Fehler::Syntax)?;
    if (r.art == "import" && nach_import) || (r.art == "namespace" && nach_namespace) {
        return Err(Fehler::Hierarchie);
    }
    Ok(r)
}

fn einruecken(s: &str) -> String {
    s.lines().map(|z| format!("  {z}")).collect::<Vec<_>>().join("\n")
}

/// §6.4.2 „serialize a CSS rule".
pub fn regel_text(r: &Regel) -> String {
    let kinder_text = |r: &Regel| -> String {
        let k = r.kinder.as_deref().unwrap_or(&[]);
        let mut s = String::from(" {\n");
        for c in k {
            s.push_str(&einruecken(&regel_text(c)));
            s.push('\n');
        }
        s.push('}');
        s
    };
    match r.art {
        "stil" => {
            let d = r.block.as_ref().map(Block::text).unwrap_or_default();
            let k = r.kinder.as_deref().unwrap_or(&[]);
            if k.is_empty() {
                return if d.is_empty() { format!("{} {{ }}", r.kopf) } else { format!("{} {{ {d} }}", r.kopf) };
            }
            let mut s = format!("{} {{\n", r.kopf);
            if !d.is_empty() {
                s.push_str(&format!("  {d}\n"));
            }
            for c in k {
                s.push_str(&einruecken(&regel_text(c)));
                s.push('\n');
            }
            s.push('}');
            s
        }
        "nested-declarations" => r.block.as_ref().map(Block::text).unwrap_or_default(),
        "font-feature-values" => {
            let w = r.felder.iter().find(|(n, _)| *n == "werte").map(|(_, v)| v.as_str()).unwrap_or("");
            let mut teile = String::new();
            for art in super::merkmalwerte::ARTEN {
                let e: Vec<String> = w
                    .split('\u{1e}')
                    .filter_map(|x| {
                        let mut f = x.split('\u{1f}');
                        (f.next() == Some(art)).then(|| format!("{}: {};", f.next().unwrap_or(""), f.next().unwrap_or("")))
                    })
                    .collect();
                if !e.is_empty() {
                    teile.push_str(&format!(" @{art} {{ {} }}", e.join(" ")));
                }
            }
            format!("@font-feature-values {} {{{teile} }}", r.kopf)
        }
        "keyframe" | "font-face" | "page" | "property" | "counter-style" | "font-palette-values" | "position-try" | "view-transition"
        | "margin" => {
            let d = r.block.as_ref().map(Block::text).unwrap_or_default();
            let kopf = match r.art {
                "keyframe" => r.kopf.clone(),
                "margin" => format!("@{}", r.kopf),
                art => {
                    if r.kopf.is_empty() {
                        format!("@{art}")
                    } else {
                        format!("@{art} {}", r.kopf)
                    }
                }
            };
            if d.is_empty() { format!("{kopf} {{ }}") } else { format!("{kopf} {{ {d} }}") }
        }
        "media" => format!("@media {}{}", r.kopf, kinder_text(r)),
        "supports" => format!("@supports {}{}", r.kopf, kinder_text(r)),
        "container" => format!("@container {}{}", r.kopf, kinder_text(r)),
        "scope" => {
            if r.kopf.is_empty() {
                format!("@scope{}", kinder_text(r))
            } else {
                format!("@scope {}{}", r.kopf, kinder_text(r))
            }
        }
        "starting-style" => format!("@starting-style{}", kinder_text(r)),
        "layer-block" => {
            if r.kopf.is_empty() {
                format!("@layer{}", kinder_text(r))
            } else {
                format!("@layer {}{}", r.kopf, kinder_text(r))
            }
        }
        "layer-statement" => format!("@layer {};", r.kopf),
        "keyframes" => format!("@keyframes {}{}", r.kopf, kinder_text(r)),
        "import" => format!("@import {};", r.kopf),
        "namespace" => {
            let p = r.felder.iter().find(|f| f.0 == "prefix").map(|f| f.1.clone()).unwrap_or_default();
            let u = r.felder.iter().find(|f| f.0 == "uri").map(|f| f.1.clone()).unwrap_or_default();
            if p.is_empty() { format!("@namespace {};", url_text(&u)) } else { format!("@namespace {} {};", ident_text(&p), url_text(&u)) }
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texte(s: &str) -> Vec<String> {
        blatt_lesen(s, &Namensraeume::default()).iter().map(regel_text).collect()
    }

    #[test]
    fn regeln() {
        assert_eq!(texte("body { width: 50% } #foo{height:100px}"), vec!["body { width: 50%; }", "#foo { height: 100px; }"]);
        assert_eq!(texte("@media  SCREEN  { p { color: red } }"), vec!["@media screen {\n  p { color: red; }\n}"]);
        assert_eq!(texte("@media print {}"), vec!["@media print {\n}"]);
        assert_eq!(texte("@import 'a.css' layer(x) screen;"), vec!["@import url(\"a.css\") layer(x) screen;"]);
        assert_eq!(texte("@namespace svg url(http://servo);"), vec!["@namespace svg url(\"http://servo\");"]);
        assert_eq!(texte("@keyframes foo { from { top: 0 } to { top: 200px } }"), vec!["@keyframes foo {\n  0% { top: 0px; }\n  100% { top: 200px; }\n}"]);
        assert_eq!(texte("@layer a, b; @layer {}"), vec!["@layer a, b;", "@layer {\n}"]);
        assert_eq!(texte("@layer;"), Vec::<String>::new());
        assert_eq!(texte("a:bogus { color: red } b { color: blue }"), vec!["b { color: blue; }"]);
        assert_eq!(texte(".a { color: red; .b { color: blue } }"), vec![".a {\n  color: red;\n  .b { color: blue; }\n}"]);
        assert_eq!(texte("p { color: red } @import 'x.css';"), vec!["p { color: red; }"]);
        assert!(regel_lesen("p { color: red; } garbage", &Namensraeume::default(), Ort::Blatt, false, false).is_err());
        assert_eq!(regel_lesen("@import 'x';", &Namensraeume::default(), Ort::Blatt, true, true), Err(Fehler::Hierarchie));
    }

    /// Release 12.10.2026: `> a` unter einer Stilregel wird `& > a`, direkt
    /// im @scope bleibt es relativ (WPT at-scope-relative-syntax).
    #[test]
    fn relativ_unter_stil() {
        assert_eq!(texte("@scope { .nest { > .foo {} } }"), vec!["@scope {
  .nest {
    & > .foo { }
  }
}"]);
        assert_eq!(texte(".nest { @scope { > .foo {} } }"), vec![".nest {
  @scope {
    > .foo { }
  }
}"]);
        let ns = Namensraeume::default();
        let r = regel_lesen_mit("> .foo {}", &ns, Ort::Verschachtelt, false, false, true).unwrap();
        assert_eq!(regel_text(&r), "& > .foo { }");
        let r = regel_lesen_mit("@media screen { + .x {} }", &ns, Ort::Verschachtelt, false, false, true).unwrap();
        assert_eq!(regel_text(&r), "@media screen {
  & + .x { }
}");
    }
}
