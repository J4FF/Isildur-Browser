//! Release 12.10.2026 (Agent C): `attr()` in der Kaskade (CSS Values 5
//! §7.7) — die Angaben mit `attr()` bleiben wie `var()` bis zur Kaskade
//! offen; dort werden sie mit den Attributen des Elements eingesetzt
//! (`cssom::ersetzen`, mit `type()`, Einheit, Rueckfall und dem Verbot in
//! Adressen).
//!
//! Die Attribute holt `element_setzen` je Element einmal: nur die Namen,
//! die irgendeine Angabe der Blaetter oder des Inline-Stils nennt (je
//! Blattsatz einmal gesammelt). Ohne `attr()` in den Blaettern kostet das
//! nichts.
//!
//! GEMESSEN vorher (WPT css-values attr-in-slotted, attr-*): `color:
//! attr(data-color type(<color>), yellow)` wurde als ungueltig verworfen.

use std::cell::RefCell;

use crate::typen::{Deklaration, Regel, StilRegel, Stylesheet, Variablen, VariablenDekl};

/// Hoechstens so viele verschiedene Attributnamen je Blattsatz (M3).
const MAX_NAMEN: usize = 256;

/// Release 15.10.2026: how many sheet sets keep their names. The page
/// builder computes every element twice, with the author sheets and with all
/// sheets (two different slices), so one entry was replaced on every call.
/// MEASURED before (youtube.com, 3.4 MB CSS, `kaskade_datei`): the
/// preparation scanned every declaration twice per element — 6.4 of 7.0 s
/// of the style computation for 259 elements.
const GEMERKTE_SAETZE: usize = 4;

thread_local! {
    /// (Adresse und Laenge des Blattsatzes, die genannten Attribute, `inherit()`
    /// irgendwo), die zuletzt benutzten vorne.
    static NAMEN: RefCell<Vec<(usize, usize, Vec<String>, bool)>> = const { RefCell::new(Vec::new()) };
    /// Release 12.10.2026: die Variablen der Eltern, wenn irgendwo `inherit()`
    /// steht (CSS Values 5 §7.4).
    static ERBEN: RefCell<Option<Variablen>> = const { RefCell::new(None) };
    /// Release 12.10.2026: Ansicht und Farbschema fuer `if(media(...))`.
    static ANSICHT: std::cell::Cell<(u32, u32, Option<crate::typen::Farbschema>)> = const { std::cell::Cell::new((800, 600, None)) };
    /// Die Attribute des Elements, das gerade gerechnet wird.
    static WERTE: RefCell<Option<Vec<(String, Option<String>)>>> = const { RefCell::new(None) };
}

/// Die Namen aller `attr(NAME …)` in `roh` an `namen` anhaengen.
fn namen_aus(roh: &str, namen: &mut Vec<String>) {
    // Ohne Kopie (je Inline-Angabe und Element; M4).
    if !crate::rechnen::hat_funktion(roh, "attr(") {
        return;
    }
    let klein = roh.to_ascii_lowercase();
    let z: Vec<char> = roh.chars().collect();
    let kz: Vec<char> = klein.chars().collect();
    let mut i = 0;
    while i + 5 <= kz.len() {
        if kz[i..i + 5] == ['a', 't', 't', 'r', '('] && (i == 0 || !(kz[i - 1].is_alphanumeric() || kz[i - 1] == '-')) {
            let mut j = i + 5;
            while j < z.len() && z[j].is_whitespace() {
                j += 1;
            }
            // Release 13.10.2026: `attr(var(--x))` — the name comes from a
            // custom property (see `VAR_NAMES`).
            if kz[j..].starts_with(&['v', 'a', 'r', '(']) {
                if !namen.iter().any(|n| n == VAR_NAMES) {
                    namen.push(String::from(VAR_NAMES));
                }
                i = j;
                continue;
            }
            let mut name = String::new();
            while j < z.len() && (z[j].is_alphanumeric() || z[j] == '-' || z[j] == '_' || z[j] as u32 >= 0x80) {
                name.push(z[j]);
                j += 1;
            }
            // `ns|name`: der lokale Name
            if j < z.len() && z[j] == '|' {
                name.clear();
                j += 1;
                while j < z.len() && (z[j].is_alphanumeric() || z[j] == '-' || z[j] == '_' || z[j] as u32 >= 0x80) {
                    name.push(z[j]);
                    j += 1;
                }
            }
            if !name.is_empty() && !namen.contains(&name) && namen.len() < MAX_NAMEN {
                namen.push(name);
            }
            i = j;
            continue;
        }
        i += 1;
    }
}

/// Release 13.10.2026: marker in the name list — an `attr()` takes its name
/// from `var()`, so every word in a custom property value is a candidate name.
const VAR_NAMES: &str = "\u{1}var";

/// The words of a custom property value as candidate attribute names.
fn candidate_names(raw: &str, names: &mut Vec<String>) {
    for word in raw.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_' || c as u32 >= 0x80)) {
        if names.len() >= MAX_NAMEN {
            return;
        }
        if !word.is_empty() && !word.starts_with("--") && !names.iter().any(|n| n == word) {
            names.push(word.to_string());
        }
    }
}

/// Steht `inherit(` (oder `if(`, das die Eltern fuer `inherit` fragt) im Rohtext?
fn mit_erben(roh: &str) -> bool {
    crate::rechnen::hat_funktion(roh, "inherit(") || crate::rechnen::hat_funktion(roh, "if(")
}

fn regeln_namen(regeln: &[StilRegel], namen: &mut Vec<String>, erben: &mut bool) {
    for r in regeln {
        for d in &r.deklarationen {
            namen_aus(&d.roh, namen);
            *erben |= mit_erben(&d.roh);
        }
        for v in &r.variablen {
            namen_aus(&v.roh, namen);
            *erben |= mit_erben(&v.roh);
        }
    }
}

/// Die Attribute des Elements fuer die folgende Kaskade bereitlegen.
pub(crate) fn element_setzen(
    blaetter: &[Stylesheet],
    inline: &[Deklaration],
    inline_vars: &[VariablenDekl],
    eltern_vars: Option<&Variablen>,
    ansicht: (u32, u32, Option<crate::typen::Farbschema>),
    attribut: impl Fn(&str) -> Option<String>,
) {
    ANSICHT.with(|a| a.set(ansicht));
    let schluessel = (blaetter.as_ptr() as usize, blaetter.len());
    let mut gemerkt = NAMEN.with(|n| {
        let mut n = n.borrow_mut();
        let i = n.iter().position(|e| (e.0, e.1) == schluessel)?;
        if i > 0 {
            let e = n.remove(i);
            n.insert(0, e);
        }
        Some((n[0].2.clone(), n[0].3))
    });
    if gemerkt.is_none() {
        let mut v = Vec::new();
        let mut erben = false;
        for b in blaetter {
            for r in &b.regeln {
                match r {
                    Regel::Stil(s) => regeln_namen(std::slice::from_ref(s), &mut v, &mut erben),
                    Regel::Media { regeln, .. } | Regel::Supports { regeln, .. } | Regel::Layer { regeln, .. } => {
                        regeln_namen(regeln, &mut v, &mut erben)
                    }
                    _ => {}
                }
            }
        }
        NAMEN.with(|n| {
            let mut n = n.borrow_mut();
            n.truncate(GEMERKTE_SAETZE - 1);
            n.insert(0, (schluessel.0, schluessel.1, v.clone(), erben));
        });
        gemerkt = Some((v, erben));
    }
    let (mut namen, mut erben) = gemerkt.unwrap_or_default();
    for d in inline {
        namen_aus(&d.roh, &mut namen);
        erben |= mit_erben(&d.roh);
    }
    for v in inline_vars {
        namen_aus(&v.roh, &mut namen);
        erben |= mit_erben(&v.roh);
    }
    if let Some(i) = namen.iter().position(|n| n == VAR_NAMES) {
        namen.remove(i);
        for v in inline_vars {
            candidate_names(&v.roh, &mut namen);
        }
        for b in blaetter {
            for r in &b.regeln {
                let regeln: &[StilRegel] = match r {
                    Regel::Stil(s) => std::slice::from_ref(s),
                    Regel::Media { regeln, .. } | Regel::Supports { regeln, .. } | Regel::Layer { regeln, .. } => regeln,
                    _ => &[],
                };
                for s in regeln {
                    for v in &s.variablen {
                        candidate_names(&v.roh, &mut namen);
                    }
                }
            }
        }
    }
    ERBEN.with(|e| *e.borrow_mut() = if erben { Some(eltern_vars.cloned().unwrap_or_default()) } else { None });
    let werte = (!namen.is_empty()).then(|| namen.into_iter().map(|n| {
        let w = attribut(&n);
        (n, w)
    }).collect());
    WERTE.with(|w| *w.borrow_mut() = werte);
}

/// Gibt es fuer das aktuelle Element ueberhaupt `attr()`-Namen?
pub(crate) fn aktiv() -> bool {
    WERTE.with(|w| w.borrow().is_some())
}

/// Steht in den Blaettern oder im Inline-Stil irgendwo `inherit()`?
pub(crate) fn erben_aktiv() -> bool {
    ERBEN.with(|e| e.borrow().is_some())
}

/// `inherit()` in `roh` mit den Variablen der Eltern einsetzen.
pub(crate) fn erben_ersetzen(roh: &str) -> Option<String> {
    ERBEN.with(|e| crate::rechnen::ersetze_inherit(roh, e.borrow().as_ref()))
}

/// Release 12.10.2026: die Bedingungen von `if()` in der Kaskade (siehe
/// `crate::cssom::wenn`): Variablen des Elements bzw. der Eltern, Ansicht.
struct KaskadenWenn<'a> {
    vars: &'a Variablen,
}

fn var_aus(vars: &Variablen, name: &str) -> Option<String> {
    let w = crate::rechnen::ersetze_var(&format!("var({name})"), vars, 0)?;
    let w = ersetzen(&w).unwrap_or(w);
    (!w.trim().eq_ignore_ascii_case("initial")).then(|| w.trim().to_string())
}

impl crate::cssom::wenn::Bedingungen for KaskadenWenn<'_> {
    fn var_wert(&self, name: &str) -> Option<String> {
        var_aus(self.vars, name)
    }
    fn eltern_var(&self, name: &str) -> Option<String> {
        ERBEN.with(|e| e.borrow().as_ref().and_then(|v| var_aus(v, name)))
    }
    fn einsetzen(&self, text: &str) -> Option<String> {
        let w = crate::rechnen::ersetze_var(text, self.vars, 0)?;
        // Release 13.10.2026: also `inherit()` (WPT css-values
        // inherit-function-basic `style(--z > inherit(--z))`).
        let w = if crate::rechnen::hat_funktion(&w, "inherit(") { erben_ersetzen(&w)? } else { w };
        if aktiv() { ersetzen(&w) } else { Some(w) }
    }
    fn medien(&self, abfrage: &str) -> bool {
        let (b, h, s) = ANSICHT.with(std::cell::Cell::get);
        crate::parse::media_trifft(abfrage, b, h, s.unwrap_or_default())
    }
    fn supports(&self, bedingung: &str) -> bool {
        crate::supports::supports_trifft(&format!("({bedingung})")) || crate::supports::supports_trifft(bedingung)
    }
    fn umgebung(&self) -> crate::cssom::mathe::Umgebung {
        crate::cssom::mathe::Umgebung::default()
    }
}

/// `if()` in `roh` aufloesen (mit den Variablen des Elements).
pub(crate) fn wenn_ersetzen(roh: &str, vars: &Variablen) -> Option<String> {
    crate::cssom::wenn::ersetzen(roh, &KaskadenWenn { vars })
}

/// `attr()` (und `random-item()`) in `roh` einsetzen. `None` = ungueltig
/// zur Berechnungszeit.
pub(crate) fn ersetzen(roh: &str) -> Option<String> {
    WERTE.with(|w| {
        let w = w.borrow();
        let leser = |n: &str| -> Option<String> {
            w.as_ref()?.iter().find(|(k, _)| k == n).and_then(|(_, v)| v.clone())
        };
        crate::cssom::ersetzen::alles_ersetzen(roh, &leser)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namen() {
        let mut v = Vec::new();
        namen_aus("attr(data-color type(<color>), yellow) ATTR( data-x px)", &mut v);
        assert_eq!(v, vec!["data-color", "data-x"]);
    }
}
