//! Release 12.10.2026 (Agent C): der Inhalt von `@font-feature-values`
//! (CSS Fonts 4 §6.9) fuer das CSSOM (`CSSFontFeatureValuesRule`): die
//! Bloecke `@annotation`, `@ornaments`, `@stylistic`, `@swash`,
//! `@character-variant` und `@styleset` mit ihren Eintraegen
//! `name: <integer>+;`.
//!
//! Kodiert als `typ \x1f name \x1f zahlen` je Eintrag, getrennt durch
//! `\x1e` (die Zahlen durch Leerzeichen).
//!
//! GEMESSEN vorher (WPT cssom/CSSFontFeatureValuesRule): `rule.annotation`
//! war `undefined` — die Bloecke wurden wie Deklarationen gelesen und fielen.

use super::syntax::{self, Kv, Tok};

/// Die bekannten Merkmalarten.
pub const ARTEN: &[&str] = &["annotation", "ornaments", "stylistic", "swash", "character-variant", "styleset"];

/// Hoechstens so viele Eintraege je Regel (M3).
const MAX_EINTRAEGE: usize = 4_096;

/// Den Blockinhalt lesen und kodieren.
pub fn kodieren(text: &str) -> String {
    let kv = syntax::komponenten(text);
    let mut raus: Vec<String> = Vec::new();
    let mut kopf: Vec<&Kv> = Vec::new();
    for k in &kv {
        match k {
            Kv::B('{', innen) => {
                let k2: Vec<&&Kv> = kopf.iter().filter(|x| !x.ist_leer()).collect();
                if let [Kv::T(Tok::At(n))] = k2.as_slice() {
                    let n = n.to_ascii_lowercase();
                    if ARTEN.contains(&n.as_str()) {
                        eintraege(&n, innen, &mut raus);
                    }
                }
                kopf.clear();
            }
            Kv::T(Tok::Semikolon) => kopf.clear(),
            k => kopf.push(k),
        }
    }
    raus.join("\u{1e}")
}

fn eintraege(art: &str, innen: &[Kv], raus: &mut Vec<String>) {
    for teil in innen.split(|k| matches!(k, Kv::T(Tok::Semikolon))) {
        if raus.len() >= MAX_EINTRAEGE {
            return;
        }
        let t: Vec<&Kv> = teil.iter().filter(|k| !k.ist_leer()).collect();
        let [Kv::T(Tok::Ident(name)), d, rest @ ..] = t.as_slice() else { continue };
        if !matches!(d, Kv::T(Tok::Doppelpunkt)) || rest.is_empty() {
            continue;
        }
        let mut zahlen = Vec::new();
        for z in rest {
            match z {
                Kv::T(Tok::Zahl(w, true, _)) if *w >= 0.0 && *w <= u32::MAX as f64 => zahlen.push(format!("{}", *w as u64)),
                _ => {
                    zahlen.clear();
                    break;
                }
            }
        }
        // §6.9.1: einzelne Werte fuer annotation/ornaments/stylistic/swash,
        // einer oder zwei fuer character-variant.
        let passt = match art {
            "character-variant" => (1..=2).contains(&zahlen.len()),
            "styleset" => !zahlen.is_empty(),
            _ => zahlen.len() == 1,
        };
        if !passt {
            continue;
        }
        // Ein spaeterer Eintrag gleichen Namens ersetzt den frueheren.
        let vorn = format!("{art}\u{1f}{name}\u{1f}");
        raus.retain(|e| !e.starts_with(&vorn));
        raus.push(format!("{vorn}{}", zahlen.join(" ")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloecke() {
        let t = kodieren("@annotation { the_first: 6; } @styleset { yo: 7; di: 10 9 4 5; bad: -1; } @swash { s: 1 2; }");
        assert_eq!(t, "annotation\u{1f}the_first\u{1f}6\u{1e}styleset\u{1f}yo\u{1f}7\u{1e}styleset\u{1f}di\u{1f}10 9 4 5");
    }
}
