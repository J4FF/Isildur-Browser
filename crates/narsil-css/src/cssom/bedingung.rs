//! Release 12.10.2026: `CSS.supports()` (CSS Conditional 3 §8.1, 4/5) und die
//! Bedingung von `@supports` nach CSSOM-Massstab: eine Deklaration gilt, wenn
//! das CSSOM sie setzen wuerde.
//!
//! GEMESSEN vorher: `CSS.supports` war eine Liste aus ~64 Namen ohne
//! Wertpruefung (`js.rs`, „cssTrifftZu") — `CSS.supports('display', 'grid')`
//! war `true`, `CSS.supports('display', 'foo')` auch, und jede WPT-Hilfe
//! `test_computed_value` scheiterte an `'…' is a supported value for …`
//! fuer Eigenschaften ausserhalb der Liste.

use super::block::{name_kanonisch, wert_lesen};
use super::syntax::{self, Kv, Tok};

/// `CSS.supports(property, value)`.
pub fn eigenschaft(name: &str, wert: &str) -> bool {
    // `!important` gehoert nicht zum Wert.
    let kv = syntax::komponenten(wert);
    if kv.iter().any(|k| k.ist_delim('!')) && !name.starts_with("--") {
        return false;
    }
    let n = if name.starts_with("--") { name.to_string() } else { name.to_ascii_lowercase() };
    // Release 12.10.2026: `--` allein ist reserviert (CSS Variables 1 §2;
    // WPT variable-empty-name-reserved).
    if n == "--" {
        return false;
    }
    match name_kanonisch(&n) {
        Some(n) => wert_lesen(&n, wert).is_some(),
        None => false,
    }
}

/// `CSS.supports(conditionText)`: als Bedingung, sonst in Klammern.
pub fn bedingung_text(text: &str, selektor: &dyn Fn(&str) -> bool) -> bool {
    let kv = syntax::komponenten(text);
    if bedingung(syntax::getrimmt(&kv), selektor) == Some(true) {
        return true;
    }
    let kv = syntax::komponenten(&format!("({text})"));
    bedingung(syntax::getrimmt(&kv), selektor) == Some(true)
}

/// `<supports-condition>`: `Some(wahr/falsch)`, `None` = Syntaxfehler.
pub fn bedingung(kv: &[Kv], selektor: &dyn Fn(&str) -> bool) -> Option<bool> {
    let t: Vec<&Kv> = kv.iter().filter(|k| !k.ist_leer()).collect();
    if t.is_empty() {
        return None;
    }
    if t[0].ist_wort("not") {
        if t.len() != 2 || !kein_leer_vor(kv, t[1]) {
            return None;
        }
        return in_klammern(t[1], selektor).map(|b| !b);
    }
    let mut erg = in_klammern(t[0], selektor)?;
    let mut op: Option<&str> = None;
    let mut i = 1;
    while i < t.len() {
        let w = t[i].ident().map(str::to_ascii_lowercase)?;
        if w != "and" && w != "or" {
            return None;
        }
        if op.is_some_and(|o| o != w) {
            return None;
        }
        let rechts = in_klammern(t.get(i + 1)?, selektor)?;
        erg = if w == "and" { erg && rechts } else { erg || rechts };
        op = Some(if w == "and" { "and" } else { "or" });
        i += 2;
    }
    Some(erg)
}

/// Zwischen `not`/`and`/`or` und der Klammer muss ein Leerzeichen stehen
/// (sonst ist `not(` eine Funktion). Hier: die Klammer ist kein `Kv::F`.
fn kein_leer_vor(_kv: &[Kv], k: &Kv) -> bool {
    !matches!(k, Kv::F(n, _) if n.eq_ignore_ascii_case("not") || n.eq_ignore_ascii_case("and") || n.eq_ignore_ascii_case("or"))
}

fn in_klammern(k: &Kv, selektor: &dyn Fn(&str) -> bool) -> Option<bool> {
    match k {
        Kv::B('(', inhalt) => {
            let t = syntax::getrimmt(inhalt);
            // Deklaration?
            if let [Kv::T(Tok::Ident(n)), rest @ ..] = t {
                let rest = syntax::getrimmt(rest);
                if let [Kv::T(Tok::Doppelpunkt), wert @ ..] = rest {
                    let w = syntax::kv_text(wert);
                    // `!important` darf in @supports stehen.
                    let w = w.trim_end();
                    let w = match w.to_ascii_lowercase().rfind("!important") {
                        Some(p) if w[p + 10..].trim().is_empty() => w[..p].trim_end().to_string(),
                        _ => w.to_string(),
                    };
                    return Some(eigenschaft_roh(n, &w, wert));
                }
            }
            // Verschachtelte Bedingung, sonst „general enclosed" (falsch).
            match bedingung(t, selektor) {
                Some(b) => Some(b),
                None => Some(false),
            }
        }
        Kv::F(n, a) => {
            let n = n.to_ascii_lowercase();
            Some(match n.as_str() {
                "selector" => selektor(&syntax::kv_text(a)),
                "font-tech" | "font-format" => {
                    let w: Vec<&Kv> = a.iter().filter(|k| !k.ist_leer()).collect();
                    match w.as_slice() {
                        [k] => k.ident().is_some_and(|w| {
                            let w = w.to_ascii_lowercase();
                            if n == "font-format" {
                                matches!(w.as_str(), "collection" | "opentype" | "truetype" | "woff" | "woff2")
                            } else {
                                matches!(w.as_str(), "features-opentype" | "variations" | "color-colrv0" | "color-sbix" | "palettes" | "incremental")
                            }
                        }),
                        _ => false,
                    }
                }
                _ => false,
            })
        }
        _ => None,
    }
}

fn eigenschaft_roh(name: &str, wert: &str, kv: &[Kv]) -> bool {
    if name.starts_with("--") {
        return true;
    }
    if syntax::getrimmt(kv).is_empty() {
        return false;
    }
    eigenschaft(name, wert)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports() {
        let sel = |s: &str| !s.is_empty();
        assert!(eigenschaft("display", "grid"));
        assert!(!eigenschaft("display", "foo"));
        assert!(eigenschaft("--x", "anything"));
        assert!(bedingung_text("(display: flex) and (color: red)", &sel));
        assert!(bedingung_text("display: flex", &sel));
        assert!(!bedingung_text("(display: flex) and (color: bogus)", &sel));
        assert!(bedingung_text("not (color: bogus)", &sel));
        assert!(!bedingung_text("(a) and (b) or (c)", &sel));
        assert!(bedingung_text("selector(div)", &sel));
    }
}
