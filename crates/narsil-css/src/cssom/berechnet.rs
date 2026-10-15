//! Release 12.10.2026: der berechnete Wert aus dem angegebenen (CSS Cascade 5
//! §4.4 „computed value"; je Eigenschaft die Zeile „Computed value" ihrer
//! Spezifikation) — fuer `getComputedStyle`, wo die Kaskade eine Eigenschaft
//! nicht kennt oder ihre Werte verlustbehaftet speichert (`min-content`,
//! `safe center`, `repeat()` …).
//!
//! GEMESSEN vorher (WPT css/*/parsing/*-computed): „… doesn't seem to be
//! supported in the computed style" fuer `grid-template-columns`,
//! `background-image`, `text-autospace` u. v. a.; `max-width: min-content`
//! kam als `none` zurueck.

use super::mathe::Umgebung;
use super::syntax;
use super::werte::{self, Ber, Num, S};


/// Was der berechnete Wert vom Element braucht.
pub struct Kontext<'a> {
    /// Schriftgroesse des Elements (fuer `em`), der Wurzel, Ansicht.
    pub u: Umgebung,
    /// Schriftgroesse der Eltern (fuer `font-size` selbst).
    pub eltern_schrift: f64,
    /// Die berechnete Farbe des Elements (fuer `currentcolor`).
    pub farbe: Option<String>,
    /// Der berechnete Wert einer Eigenschaft an den Eltern (`inherit`).
    pub eltern: &'a dyn Fn(&str) -> Option<String>,
    /// Der angegebene Wert einer ANDEREN Eigenschaft am Element (z. B.
    /// `border-top-style` fuer `border-top-width`).
    pub andere: &'a dyn Fn(&str) -> Option<String>,
    /// Eine Adresse absolut machen.
    pub adresse: Option<&'a dyn Fn(&str) -> String>,
}

fn px(w: f64) -> String {
    format!("{}px", super::zahl::zahl_text(w))
}

/// Schluesselwort-Schriftgroessen (CSS Fonts 4 §2.5, Chrome-Werte bei 16px).
fn schrift_wort(w: &str, eltern: f64) -> Option<f64> {
    Some(match w {
        "xx-small" => 9.0,
        "x-small" => 10.0,
        "small" => 13.0,
        "medium" => 16.0,
        "large" => 18.0,
        "x-large" => 24.0,
        "xx-large" => 32.0,
        "xxx-large" => 48.0,
        "larger" => eltern * 1.2,
        "smaller" => eltern / 1.2,
        _ => return None,
    })
}

/// Der berechnete Wert von `name` (kanonisch, Langform) bei angegebenem Wert
/// `spez` (`None` = nicht angegeben). `None` = kann ich nicht.
pub fn berechnen(name: &str, spez: Option<&str>, k: &Kontext) -> Option<String> {
    let vererbt = super::vererbt(name);
    let spez = match spez.map(str::trim) {
        None | Some("") => {
            if vererbt && let Some(e) = (k.eltern)(name) {
                return Some(e);
            }
            super::anfangswert(name)?
        }
        Some(s) => s,
    };
    let klein = spez.to_ascii_lowercase();
    match klein.as_str() {
        "inherit" => return (k.eltern)(name).or_else(|| berechnen(name, super::anfangswert(name), k)),
        "initial" => return berechnen(name, super::anfangswert(name), k),
        "unset" | "revert" | "revert-layer" | "revert-rule" => {
            if vererbt && let Some(e) = (k.eltern)(name) {
                return Some(e);
            }
            return berechnen(name, super::anfangswert(name), k);
        }
        _ => {}
    }
    // Release 12.10.2026: Baumzaehl-Funktionen als Zahlen (siehe
    // `ersetzen::geschwister_einsetzen`), wo der Wert sie sonst nicht aufloest.
    let eingesetzt;
    let spez = if let Some(g) = k.u.geschwister
        && klein.contains("sibling-")
        && (klein.contains("rgb") || klein.contains("hsl") || klein.contains("gradient") || klein.contains("color") || klein.contains("lab(") || klein.contains("lch("))
    {
        eingesetzt = super::ersetzen::geschwister_einsetzen(spez, g);
        eingesetzt.as_str()
    } else {
        spez
    };
    let kv = syntax::komponenten(spez);
    if super::block::hat_ersetzung(&kv) {
        return None;
    }
    let s = super::lang_lesen(name, &kv)?;
    let ber = Ber { u: k.u, farbe: k.farbe.clone(), basis: k.adresse };
    Some(eigen(name, &s, &ber, k).unwrap_or_else(|| werte::berechnet_text(&s, &ber)))
}

fn wort(s: &S) -> Option<&str> {
    match s {
        S::W(w) => Some(w),
        _ => None,
    }
}

/// Eigenheiten einzelner Eigenschaften; `None` = die allgemeine Regel.
fn eigen(name: &str, s: &[S], ber: &Ber, k: &Kontext) -> Option<String> {
    match name {
        // Release 15.10.2026: `linear()` lists its points with their inputs.
        "transition-timing-function" | "animation-timing-function" => {
            super::easing::computed_linear(s).map(|v| werte::berechnet_text(&v, ber))
        }
        "border-top-width" | "border-right-width" | "border-bottom-width" | "border-left-width" | "outline-width"
        | "column-rule-width" | "border-block-start-width" | "border-block-end-width" | "border-inline-start-width"
        | "border-inline-end-width" => {
            let stil = name.replace("-width", "-style");
            let st = (k.andere)(&stil).or_else(|| super::anfangswert(&stil).map(str::to_string)).unwrap_or_default();
            if matches!(st.as_str(), "none" | "hidden") && name != "outline-width" || (name == "outline-width" && st == "none") {
                return Some(String::from("0px"));
            }
            match s {
                [S::W(w)] => Some(String::from(match w.as_str() {
                    "thin" => "1px",
                    "medium" => "3px",
                    _ => "5px",
                })),
                _ => None,
            }
        }
        "font-weight" => match s {
            [S::W(w)] => Some(match w.as_str() {
                "normal" => String::from("400"),
                "bold" => String::from("700"),
                "bolder" | "lighter" => {
                    let e: f64 = (k.eltern)("font-weight").and_then(|e| e.parse().ok()).unwrap_or(400.0);
                    let w = if w == "bolder" {
                        if e < 350.0 { 400.0 } else if e < 550.0 { 700.0 } else { 900.0f64.max(e) }
                    } else if e < 550.0 {
                        100.0f64.min(e)
                    } else if e < 750.0 {
                        400.0
                    } else {
                        700.0
                    };
                    super::zahl::zahl_text(w)
                }
                _ => return None,
            }),
            _ => None,
        },
        "font-size" => {
            let e = k.eltern_schrift;
            let ber_e = Ber { u: Umgebung { em: e, ..ber.u }, farbe: None, basis: None };
            match s {
                [S::W(w)] => schrift_wort(w, e).map(px),
                [S::Z(Num::L(p, ein))] if ein == "%" => Some(px(e * p / 100.0)),
                [S::Z(n)] => {
                    let mut u = ber_e.u;
                    u.prozent = Some(e);
                    let b = Ber { u, farbe: None, basis: None };
                    Some(werte::num_berechnet(n, &b))
                }
                _ => None,
            }
        }
        "line-height" => match s {
            [S::Z(Num::L(p, e))] if e == "%" => Some(px(ber.u.em * p / 100.0)),
            _ => None,
        },
        "word-spacing" => match s {
            [S::W(w)] if w == "normal" => Some(String::from("0px")),
            _ => None,
        },
        "letter-spacing" => {
            let t = werte::berechnet_text(s, ber);
            Some(if t == "0px" { String::from("normal") } else { t })
        }
        "opacity" => match s {
            [S::Z(n)] => {
                let t = werte::num_berechnet(n, ber);
                let w: f64 = t.trim_end_matches('%').parse().ok()?;
                let w = if t.ends_with('%') { w / 100.0 } else { w };
                Some(super::zahl::zahl_text(w.clamp(0.0, 1.0)))
            }
            _ => None,
        },
        // Release 12.10.2026: der aufgeloeste Wert ist die Matrix.
        "transform" => super::matrix::transform_text(s, ber),
        // Release 12.10.2026: absolut/fest positionierte und schwebende
        // Kaesten werden blockartig (CSS Display 3 §2.7 „blockify"; WPT
        // display-computed: `inline-table` -> `table`).
        "display" => {
            let pos = (k.andere)("position").unwrap_or_default();
            let fl = (k.andere)("float").unwrap_or_default();
            let mut t = werte::text(s);
            // `-webkit-box` mit senkrechter Ausrichtung und Zeilenbegrenzung
            // wird `flow-root` (CSS Overflow 4 §5.3 „legacy -webkit-box";
            // WPT webkit-box-computed); `-webkit-flex` ist `flex`.
            match t.as_str() {
                "-webkit-flex" => t = String::from("flex"),
                "-webkit-inline-flex" => t = String::from("inline-flex"),
                "-webkit-box" | "-webkit-inline-box" => {
                    let senkrecht = (k.andere)("-webkit-box-orient").is_some_and(|o| o.trim().eq_ignore_ascii_case("vertical"));
                    let fort = (k.andere)("continue").unwrap_or_default();
                    let klemme = (k.andere)("-webkit-line-clamp").is_some_and(|v| !v.is_empty() && v != "none");
                    if senkrecht && (klemme || matches!(fort.as_str(), "collapse" | "discard" | "-webkit-legacy")) {
                        t = String::from(if t == "-webkit-box" { "flow-root" } else { "inline-block" });
                    }
                }
                _ => {}
            }
            // Release 12.10.2026: auch Kinder eines Flex-/Raster-Containers werden
            // blockartig (CSS Display 3 §2.7; WPT cssom getComputedStyle-pseudo
            // „Item-based blockification").
            let eltern_flex = (k.eltern)("display").is_some_and(|d| {
                matches!(d.trim(), "flex" | "inline-flex" | "grid" | "inline-grid" | "grid-lanes" | "inline-grid-lanes")
            });
            if !(matches!(pos.as_str(), "absolute" | "fixed") || (!fl.is_empty() && fl != "none") || eltern_flex) {
                return Some(t);
            }
            Some(String::from(match t.as_str() {
                "inline-table" => "table",
                "inline-flex" => "flex",
                "inline-grid" => "grid",
                "inline" | "inline-block" | "run-in" => "block",
                "inline-list-item" => "list-item",
                "-webkit-inline-box" => "-webkit-box",
                "inline-grid-lanes" => "grid-lanes",
                // Innere Tabellen- und Ruby-Kaesten werden `block`.
                "table-row-group" | "table-header-group" | "table-footer-group" | "table-row" | "table-cell"
                | "table-column-group" | "table-column" | "table-caption" | "ruby-base" | "ruby-text"
                | "ruby-base-container" | "ruby-text-container" => "block",
                // Zweiwortformen: `inline X` -> die Blockform von X.
                _ => match t.strip_prefix("inline ") {
                    Some("flow") => "block",
                    Some("ruby") => return Some(String::from("block ruby")),
                    Some(rest) => return Some(rest.to_string()),
                    None => return Some(t),
                },
            }))
        }
        // Release 12.10.2026: berechnet steht `fill-reverse` vor `track-reverse`
        // (WPT css-grid grid-lanes-direction-computed).
        "grid-lanes-direction" => {
            let mut w: Vec<String> = s.iter().filter_map(|p| wort(p).map(str::to_string)).collect();
            if w.len() == 3 && w[1] == "track-reverse" {
                w.swap(1, 2);
            }
            Some(w.join(" "))
        }
        // Release 12.10.2026: `transform-origin` berechnet: Schluesselwoerter
        // als Prozent, waagerecht vor senkrecht, ein z von 0 faellt weg (CSS
        // Transforms 1 §7; WPT css-transforms transform-origin-computed).
        "transform-origin" => {
            let teil = |p: &S, senkrecht: bool| -> String {
                match wort(p) {
                    Some("left") | Some("top") => String::from("0%"),
                    Some("center") => String::from("50%"),
                    Some("right") | Some("bottom") => String::from("100%"),
                    _ => {
                        let _ = senkrecht;
                        werte::berechnet_text(std::slice::from_ref(p), ber)
                    }
                }
            };
            let (x, y, z) = match s {
                [a] if matches!(wort(a), Some("top" | "bottom")) => (String::from("50%"), teil(a, true), None),
                [a] => (teil(a, false), String::from("50%"), None),
                [a, b] => (teil(a, false), teil(b, true), None),
                [a, b, c] => (teil(a, false), teil(b, true), Some(werte::berechnet_text(std::slice::from_ref(c), ber))),
                _ => return None,
            };
            Some(match z {
                Some(z) if z != "0px" => format!("{x} {y} {z}"),
                _ => format!("{x} {y}"),
            })
        }
        "scale" => {
            // Prozente werden Zahlen (CSS Transforms 2 §5.1), auch aus calc();
            // dann die kurze Form (`z = 1`, `y = x` fallen weg).
            let zahlen: Vec<S> = s
                .iter()
                .map(|p| {
                    let t = match p {
                        S::Z(Num::L(w, e)) if e == "%" => super::zahl::zahl_text(w / 100.0),
                        p => werte::berechnet_text(std::slice::from_ref(p), ber),
                    };
                    match t.strip_suffix('%').and_then(|x| x.parse::<f64>().ok()) {
                        Some(w) => S::Z(Num::L(w / 100.0, String::new())),
                        None => match t.parse::<f64>() {
                            Ok(w) => S::Z(Num::L(w, String::new())),
                            Err(_) => S::W(t),
                        },
                    }
                })
                .collect();
            let kurz = super::scale_kurz(zahlen);
            Some(kurz.iter().map(|p| werte::berechnet_text(std::slice::from_ref(p), ber)).collect::<Vec<_>>().join(" "))
        }
        // Release 12.10.2026: die logischen Woerter einer `<position>` werden
        // mit Schreibmodus und Richtung physisch (CSS Values 5 §4.2). GEMESSEN
        // vorher (WPT css-values position-computed): `x-start` blieb stehen.
        "object-position" if s.iter().any(|p| matches!(p, S::Pos(_))) => {
            let wm = (k.andere)("writing-mode").filter(|w| !w.trim().is_empty()).or_else(|| (k.eltern)("writing-mode")).unwrap_or_default();
            let rtl = (k.andere)("direction").filter(|d| !d.trim().is_empty()).or_else(|| (k.eltern)("direction")).is_some_and(|d| d.trim() == "rtl");
            let wm = wm.trim();
            let x_start = match wm {
                "vertical-rl" | "sideways-rl" => "right",
                "vertical-lr" | "sideways-lr" => "left",
                _ => if rtl { "right" } else { "left" },
            };
            let y_start = match wm {
                "vertical-rl" | "sideways-rl" | "vertical-lr" => if rtl { "bottom" } else { "top" },
                "sideways-lr" => if rtl { "top" } else { "bottom" },
                _ => "top",
            };
            let gegen = |w: &str| match w {
                "left" => "right",
                "right" => "left",
                "top" => "bottom",
                _ => "top",
            };
            let physisch = |seite: &[S]| -> Vec<S> {
                seite
                    .iter()
                    .map(|p| match p {
                        S::W(w) if w == "x-start" => S::W(x_start.to_string()),
                        S::W(w) if w == "x-end" => S::W(gegen(x_start).to_string()),
                        S::W(w) if w == "y-start" => S::W(y_start.to_string()),
                        S::W(w) if w == "y-end" => S::W(gegen(y_start).to_string()),
                        p => p.clone(),
                    })
                    .collect()
            };
            let teile: Vec<String> = s
                .iter()
                .map(|p| match p {
                    S::Pos(b) => {
                        let (x, y) = (physisch(&b.0), physisch(&b.1));
                        // Ein senkrechtes Wort auf der waagerechten Seite (oder umgekehrt) tauscht.
                        let senk = |v: &[S]| matches!(v.first(), Some(S::W(w)) if w == "top" || w == "bottom");
                        let waag = |v: &[S]| matches!(v.first(), Some(S::W(w)) if w == "left" || w == "right");
                        let (x, y) = if senk(&x) || waag(&y) { (y, x) } else { (x, y) };
                        format!("{} {}", werte::seite_berechnet(&x, ber), werte::seite_berechnet(&y, ber))
                    }
                    p => werte::berechnet_text(std::slice::from_ref(p), ber),
                })
                .collect();
            Some(teile.join(" "))
        }
        "background-position-x" | "background-position-y" => {
            let sch = super::schichten(s);
            let mehrere = sch.len() > 1;
            let teile: Vec<String> = sch
                .iter()
                .map(|g| {
                    let t = werte::seite_berechnet(g, ber);
                    // Release 12.10.2026: in einer Liste werden die logischen
                    // Woerter Prozente, allein bleiben sie stehen (WPT
                    // background-position-x-computed, beides).
                    match t.as_str() {
                        "x-start" | "y-start" if mehrere => String::from("0%"),
                        "x-end" | "y-end" if mehrere => String::from("100%"),
                        _ => t,
                    }
                })
                .collect();
            Some(teile.join(", "))
        }
        "box-shadow" | "text-shadow" => {
            if let [S::W(w)] = s
                && w == "none"
            {
                return Some(String::from("none"));
            }
            let mut schichten = Vec::new();
            for g in super::schichten(s) {
                let farbe = g.iter().find_map(|p| match p {
                    S::Farbe(f) => Some(f.berechnet(ber.farbe.as_deref())),
                    _ => None,
                });
                let farbe = farbe.unwrap_or_else(|| ber.farbe.clone().unwrap_or_else(|| String::from("rgb(0, 0, 0)")));
                let mut laengen: Vec<String> = g
                    .iter()
                    .filter_map(|p| match p {
                        S::Z(n) => Some(werte::num_berechnet(n, ber)),
                        _ => None,
                    })
                    .collect();
                let soll = if name == "box-shadow" { 4 } else { 3 };
                while laengen.len() < soll {
                    laengen.push(String::from("0px"));
                }
                let mut x = format!("{farbe} {}", laengen.join(" "));
                if g.iter().any(|p| wort(p) == Some("inset")) {
                    x.push_str(" inset");
                }
                schichten.push(x);
            }
            Some(schichten.join(", "))
        }
        "background-image" | "border-image-source" | "list-style-image" | "mask-image" => Some(bilder_berechnet(s, ber)),
        // CSS Align 3 §4: `flex-start`/`flex-end` berechnen sich zu
        // `flow-start`/`flow-end` (WPT css-align *-computed).
        "align-content" | "justify-content" | "align-items" | "justify-items" | "align-self" | "justify-self" => {
            if name == "justify-items" && matches!(s, [S::W(w)] if w == "legacy") {
                let e = (k.eltern)("justify-items").unwrap_or_default();
                return Some(if e.starts_with("legacy") { e } else { String::from("normal") });
            }
            let v: Vec<String> = s
                .iter()
                .map(|p| match wort(p) {
                    Some("flex-start") => String::from("flow-start"),
                    Some("flex-end") => String::from("flow-end"),
                    _ => werte::stueck_text(p, Some(ber)),
                })
                .collect();
            Some(v.join(" "))
        }
        // CSS Overflow 3 §3.1: `visible` wird `auto`, wenn die andere Achse
        // weder `visible` noch `clip` ist.
        "overflow-x" | "overflow-y" | "overflow-block" | "overflow-inline" => {
            let w = wort(s.first()?)?.to_string();
            let anders = match name {
                "overflow-x" => "overflow-y",
                "overflow-y" => "overflow-x",
                "overflow-block" => "overflow-inline",
                _ => "overflow-block",
            };
            let a = (k.andere)(anders).unwrap_or_else(|| String::from("visible"));
            if w == "visible" && !matches!(a.as_str(), "visible" | "clip") {
                return Some(String::from("auto"));
            }
            Some(w)
        }
        "scroll-marker-group" => {
            let mut v: Vec<String> = s.iter().map(|p| werte::stueck_text(p, Some(ber))).collect();
            if v.len() == 1 && v[0] != "none" {
                v.push(String::from("links"));
            }
            Some(v.join(" "))
        }
        // CSS Text 3 §7.1: `match-parent` berechnet sich wie der Wert der
        // Eltern, `start`/`end` dabei aufgeloest.
        "text-align" => match s {
            [S::W(w)] if w == "match-parent" => {
                let e = (k.eltern)("text-align").unwrap_or_else(|| String::from("start"));
                Some(match e.as_str() {
                    "start" => String::from("left"),
                    "end" => String::from("right"),
                    _ => e,
                })
            }
            _ => None,
        },
        "text-justify" => match s {
            [S::W(w)] if w == "distribute" => Some(String::from("inter-character")),
            _ => None,
        },
        "text-fit" => {
            let v: Vec<S> = s.iter().filter(|p| wort(p) != Some("consistent")).cloned().collect();
            Some(werte::berechnet_text(&v, ber))
        }
        _ => None,
    }
}

/// Bilder als berechneter Wert: Verlaeufe ohne die Vorgaben (`to bottom`,
/// `180deg`, `at 50% 50%`, `ellipse farthest-corner`) — wie Chrome sie schreibt.
fn bilder_berechnet(s: &[S], ber: &Ber) -> String {
    let mut raus = Vec::new();
    for p in s {
        raus.push(match p {
            S::F(n, inner) if n.contains("gradient") => {
                // Die Kopf-Gruppe bis zum ersten Komma.
                let pos = inner.iter().position(|x| *x == S::Komma);
                let (kopf, rest) = match pos {
                    Some(i) => (&inner[..i], &inner[i + 1..]),
                    None => (&inner[..0], &inner[..]),
                };
                let mut kopf_text: Vec<String> = Vec::new();
                let mut i = 0;
                while i < kopf.len() {
                    let x = &kopf[i];
                    if wort(x) == Some("at")
                        && let Some(S::Pos(_)) = kopf.get(i + 1)
                    {
                        let pt = werte::stueck_text(&kopf[i + 1], Some(ber));
                        if pt != "50% 50%" {
                            kopf_text.push(format!("at {pt}"));
                        }
                        i += 2;
                        continue;
                    }
                    kopf_text.push(werte::stueck_text(x, Some(ber)));
                    i += 1;
                }
                let mut k = kopf_text.join(" ");
                if n.contains("radial") {
                    // Release 12.10.2026: `circle` vor EINER Laenge faellt weg (CSS
                    // Images 3 §3.2.1; WPT css-backgrounds background-image-computed).
                    let kreis = matches!(kopf.first(), Some(S::W(w)) if w == "circle")
                        && matches!(kopf.get(1), Some(S::Z(_)))
                        && !matches!(kopf.get(2), Some(S::Z(_)));
                    let v: Vec<&str> = kopf_text
                        .iter()
                        .map(String::as_str)
                        .filter(|w| !matches!(*w, "ellipse" | "farthest-corner") && !(kreis && *w == "circle"))
                        .collect();
                    k = v.join(" ");
                } else if n.contains("conic") {
                    k = k.strip_prefix("from 0deg").map_or(k.clone(), |r| r.trim_start().to_string());
                } else if matches!(k.as_str(), "to bottom" | "180deg") {
                    k.clear();
                }
                let r = werte::berechnet_text(rest, ber);
                if k.is_empty() { format!("{n}({r})") } else { format!("{n}({k}, {r})") }
            }
            // Release 12.10.2026: `light-dark()` mit Bildern waehlt im hellen
            // Schema das erste; `none` wird ein durchsichtiges Bild (wie Chrome;
            // WPT css-backgrounds background-image-computed).
            S::F(n, inner) if n == "light-dark" => {
                let erstes = super::schichten(inner).into_iter().next().unwrap_or_default();
                match erstes.as_slice() {
                    [S::W(w)] if w == "none" => String::from("image(rgba(0, 0, 0, 0))"),
                    e => bilder_berechnet(e, ber),
                }
            }
            // Release 12.10.2026: Zeichenketten in image-set()/src() sind
            // Adressen — berechnet als url(), image-set() mit Aufloesung
            // (Vorgabe 1dppx; CSS Images 4 §2.2, WPT css-values attr-security).
            S::F(n, inner) if n == "image-set" || n == "-webkit-image-set" || n == "src" => {
                let als_url = |x: &S| -> Option<String> {
                    let S::Txt(t) = x else { return None };
                    let roh = super::syntax::komponenten(t);
                    let [super::syntax::Kv::T(super::syntax::Tok::Str(s))] = super::syntax::getrimmt(&roh) else { return None };
                    let u = match ber.basis.as_ref() {
                        Some(f) => f(s),
                        None => s.clone(),
                    };
                    Some(super::syntax::url_text(&u))
                };
                let mut teile: Vec<String> = Vec::new();
                for opt in super::schichten(inner) {
                    let mut stuecke: Vec<String> = Vec::new();
                    let mut aufloesung = false;
                    for x in &opt {
                        if let Some(u) = als_url(x) {
                            stuecke.push(u);
                            continue;
                        }
                        let t = werte::stueck_text(x, Some(ber));
                        if matches!(x, S::Z(_)) {
                            aufloesung = true;
                        }
                        stuecke.push(t);
                    }
                    if n != "src" && !aufloesung {
                        stuecke.push(String::from("1dppx"));
                    }
                    teile.push(stuecke.join(" "));
                }
                format!("{n}({})", teile.join(", "))
            }
            S::Komma => String::from(","),
            p => werte::stueck_text(p, Some(ber)),
        });
    }
    raus.join(" ").replace(" ,", ",")
}
