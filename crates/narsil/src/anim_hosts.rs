//! Release 15.10.2026 (Agent C): the host functions of the animation module
//! (`animationen.js`: CSS Animations, CSS Transitions, Web Animations).
//!
//! The module keeps the timing model in JS; the host keeps the values:
//! - `__narsilAnimQuelle()`: the module source (loaded on first use).
//! - `__narsilAnimSetzen(el, [name, value, …])`: the animated values of an
//!   element (`Lebend::animated`; empty list: none). They form their own
//!   cascade layer above the normal declarations.
//! - `__narsilBasiswert(el, name)`: the computed value without the
//!   element's own animated values (the underlying value).
//! - `__narsilWertBerechnen(el, name, value)`: the computed value of a
//!   keyframe value in the element's context (`em`, `var()`, `inherit` …).
//! - `__narsilInterpolieren(name, a, b, p)`: interpolates two computed values
//!   (`narsil_css::cssom::interpolieren`); `__narsilKombinieren(name, a, b,
//!   accumulate)` adds them (composite `add`/`accumulate`).
//! - `__narsilAnimCss(generation)`: the elements with CSS animations or
//!   transitions and their animation properties; `__narsilBasiswerte`,
//!   `__narsilInterpolierbar`, `__narsilLangformen` serve transitions;
//!   `__narsilAnimNoetig(el)` tells whether the page declares any.
//!
//! Pages without animations never call them, so they pay nothing.

use crate::dom::{Geteilt, Stand};
use narsil_jsvm::Wert;

/// Most values one element may carry (one per property).
const MAX_VALUES: usize = 512;
/// Most changed elements one CSS scan looks at (more: a full scan).
const MAX_ROOTS: usize = 512;

pub(crate) fn register(engine: &mut narsil_jsvm::Engine, stand: &Geteilt) {
    // `NARSIL_ANIMATIONEN_AUS=1` (diagnosis): no animation module, only the
    // old stubs of the prelude — to measure pages with and without it.
    if std::env::var_os("NARSIL_ANIMATIONEN_AUS").is_some() {
        return;
    }
    engine.host_funktion("__narsilAnimQuelle", |_args: &[Wert]| Ok(Wert::Text(crate::js::ANIMATIONEN_JS.to_string())));
    engine.host_funktion("__narsilAnimSetzen", {
        let state = stand.clone();
        move |args: &[Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(Wert::Null);
            };
            let mut values: Vec<(String, String)> = Vec::new();
            if let Some(Wert::Feld(f)) = args.get(1) {
                for pair in f.chunks(2).take(MAX_VALUES) {
                    if let [n, v] = pair {
                        let name = n.als_text();
                        // Only property names; a value never closes the
                        // declaration it is written into.
                        if is_property_name(&name) {
                            values.push((name, sanitize_value(&v.als_text())));
                        }
                    }
                }
            }
            let pseudo = args.get(2).map(Wert::als_text).map(|p| p.trim_start_matches(':').to_ascii_lowercase()).filter(|p| !p.is_empty() && p != "null" && p != "undefined");
            let mut w = state.lock().map_err(|_| "DomStand poisoned".to_string())?;
            match pseudo {
                Some(art) => set_pseudo_values(&mut w, id, art, values),
                None => set_values(&mut w, id, values),
            }
            Ok(Wert::Null)
        }
    });
    engine.host_funktion("__narsilBasiswert", {
        let state = stand.clone();
        move |args: &[Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(Wert::Null);
            };
            let name = args.get(1).map(Wert::als_text).unwrap_or_default();
            let pseudo = args.get(2).map(Wert::als_text).map(|p| p.trim_start_matches(':').to_ascii_lowercase()).filter(|p| !p.is_empty() && p != "null" && p != "undefined");
            let mut w = state.lock().map_err(|_| "DomStand poisoned".to_string())?;
            if let Some(art) = pseudo {
                return Ok(pseudo_base_value(&mut w, id, &art, &name).map_or(Wert::Null, Wert::Text));
            }
            Ok(computed_with(&mut w, id, &name, None).map_or(Wert::Null, Wert::Text))
        }
    });
    engine.host_funktion("__narsilBasiswerte", {
        let state = stand.clone();
        move |args: &[Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(Wert::Null);
            };
            let names: Vec<String> = match args.get(1) {
                Some(Wert::Feld(f)) => f.iter().take(MAX_VALUES).map(Wert::als_text).collect(),
                _ => Vec::new(),
            };
            let pseudo = args.get(2).map(Wert::als_text).map(|p| p.trim_start_matches(':').to_ascii_lowercase()).filter(|p| !p.is_empty() && p != "null" && p != "undefined");
            let mut w = state.lock().map_err(|_| "DomStand poisoned".to_string())?;
            let values = match pseudo {
                Some(art) => names.iter().map(|n| pseudo_base_value(&mut w, id, &art, n)).collect(),
                None => base_values(&mut w, id, &names),
            };
            Ok(Wert::Feld(values.into_iter().map(|v| v.map_or(Wert::Null, Wert::Text)).collect()))
        }
    });
    engine.host_funktion("__narsilKombinieren", |args: &[Wert]| {
        let text = |i: usize| args.get(i).map(Wert::als_text).unwrap_or_default();
        let accumulate = matches!(args.get(3), Some(Wert::Wahr(true)));
        Ok(narsil_css::cssom::interpolieren::kombinieren(&text(0), &text(1), &text(2), accumulate).map_or(Wert::Null, Wert::Text))
    });
    engine.host_funktion("__narsilWertNormal", |args: &[Wert]| {
        let name = args.first().map(Wert::als_text).unwrap_or_default();
        let value = args.get(1).map(Wert::als_text).unwrap_or_default();
        Ok(normalized_value(&name, &value).map_or(Wert::Null, Wert::Text))
    });
    engine.host_funktion("__narsilLangformen", |args: &[Wert]| {
        let name = args.first().map(Wert::als_text).unwrap_or_default();
        Ok(Wert::Feld(longhands(&name).into_iter().map(Wert::Text).collect()))
    });
    engine.host_funktion("__narsilVorherwerte", {
        let state = stand.clone();
        move |args: &[Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(Wert::Null);
            };
            let names: Vec<String> = match args.get(1) {
                Some(Wert::Feld(f)) => f.iter().take(MAX_VALUES).map(Wert::als_text).collect(),
                _ => Vec::new(),
            };
            let w = state.lock().map_err(|_| "DomStand poisoned".to_string())?;
            // A box that was not rendered has no before-change style (CSS
            // Transitions 1 §3).
            let rendered = |k: usize| {
                let Some(l) = w.lebend.as_ref() else { return false };
                let mut x = Some(k);
                while let Some(e) = x {
                    if w.style_prev.get(&e).is_some_and(|b| b.anzeige == narsil_css::Anzeige::Keins) {
                        return false;
                    }
                    x = l.dok.eltern(e);
                }
                true
            };
            let prev = w.lebend.as_ref().and_then(|l| l.knoten(id)).filter(|k| rendered(*k)).and_then(|k| w.style_prev.get(&k));
            Ok(Wert::Feld(
                names
                    .iter()
                    .map(|n| {
                        prev.and_then(|b| {
                            // Custom properties live in the variables.
                            if n.starts_with("--") {
                                b.variablen.holen(n).map(|t| t.trim().to_string())
                            } else {
                                narsil_css::berechnet_css::berechneter_wert(b, n)
                            }
                        })
                        .map_or(Wert::Null, Wert::Text)
                    })
                    .collect(),
            ))
        }
    });
    engine.host_funktion("__narsilStartwerte", {
        let state = stand.clone();
        move |args: &[Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(Wert::Null);
            };
            let names: Vec<String> = match args.get(1) {
                Some(Wert::Feld(f)) => f.iter().take(MAX_VALUES).map(Wert::als_text).collect(),
                _ => Vec::new(),
            };
            let mut w = state.lock().map_err(|_| "DomStand poisoned".to_string())?;
            Ok(Wert::Feld(starting_values(&mut w, id, &names).into_iter().map(|v| v.map_or(Wert::Null, Wert::Text)).collect()))
        }
    });
    engine.host_funktion("__narsilInterpolierbar", |args: &[Wert]| {
        let text = |i: usize| args.get(i).map(Wert::als_text).unwrap_or_default();
        Ok(Wert::Wahr(narsil_css::cssom::interpolieren::interpolierbar(&text(0), &text(1), &text(2))))
    });
    engine.host_funktion("__narsilWertBerechnen", {
        let state = stand.clone();
        move |args: &[Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(Wert::Null);
            };
            let name = args.get(1).map(Wert::als_text).unwrap_or_default();
            let value = args.get(2).map(Wert::als_text).unwrap_or_default();
            if !is_property_name(&name) {
                return Ok(Wert::Null);
            }
            let pseudo = args.get(3).map(Wert::als_text).map(|p| p.trim_start_matches(':').to_ascii_lowercase()).filter(|p| !p.is_empty() && p != "null" && p != "undefined");
            let mut w = state.lock().map_err(|_| "DomStand poisoned".to_string())?;
            if let Some(art) = pseudo {
                crate::dom::refresh_styles(&mut w);
                let w = &mut *w;
                let r = match (w.lebend.as_ref(), w.stil_satz.as_ref()) {
                    (Some(l), Some(satz)) => l.knoten(id).and_then(|k| crate::lebend::pseudo_value_of(l, k, &art, &name, &sanitize_value(&value), satz, &mut w.stil_cache)),
                    _ => None,
                };
                return Ok(r.map_or(Wert::Null, Wert::Text));
            }
            Ok(computed_with(&mut w, id, &name, Some(&sanitize_value(&value))).map_or(Wert::Null, Wert::Text))
        }
    });
    engine.host_funktion("__narsilAnimCss", {
        let state = stand.clone();
        move |args: &[Wert]| {
            let seen = match args.first() {
                Some(Wert::Zahl(n)) => Some(*n),
                _ => None,
            };
            // `true` and the changed elements as single arguments (an array
            // of elements would cross the bridge with all their expandos).
            let roots: Option<Vec<u32>> = match args.get(1) {
                Some(Wert::Wahr(true)) => Some(args.iter().skip(2).take(MAX_ROOTS).filter_map(narsil_jsvm::host_id).collect()),
                _ => None,
            };
            let mut w = state.lock().map_err(|_| "DomStand poisoned".to_string())?;
            Ok(css_scan(&mut w, seen, roots))
        }
    });
    engine.host_funktion("__narsilAnimNoetig", {
        let state = stand.clone();
        move |args: &[Wert]| {
            let id = args.first().and_then(narsil_jsvm::host_id);
            let mut w = state.lock().map_err(|_| "DomStand poisoned".to_string())?;
            Ok(Wert::Wahr(needed(&mut w, id)))
        }
    });
    engine.host_funktion("__narsilInterpolieren", |args: &[Wert]| {
        let text = |i: usize| args.get(i).map(Wert::als_text).unwrap_or_default();
        let p = match args.get(3) {
            Some(Wert::Zahl(n)) => *n,
            _ => 0.0,
        };
        Ok(narsil_css::cssom::interpolieren::interpolieren(&text(0), &text(1), &text(2), p).map_or(Wert::Null, Wert::Text))
    });
}

/// The style sheets changed: like `styles_changed`, and the next animation
/// scan looks at every element.
pub(crate) fn sheets_changed(w: &mut Stand) {
    w.sheet_generation = w.sheet_generation.wrapping_add(1);
    styles_changed(w);
}

/// Most computed styles kept from before the last change.
const MAX_PREV_STYLES: usize = 4096;

/// The computed styles are no longer valid (a DOM write, a DOM method, a
/// style sheet). They move to `style_prev` (the before-change style of CSS
/// transitions), and the style generation moves on.
pub(crate) fn styles_changed(w: &mut Stand) {
    if w.style_prev.len() + w.stil_cache.len() > MAX_PREV_STYLES {
        w.style_prev.clear();
    }
    if w.stil_cache.len() <= MAX_PREV_STYLES {
        w.style_prev.extend(w.stil_cache.drain());
    } else {
        w.stil_cache.clear();
    }
    w.style_generation = w.style_generation.wrapping_add(1);
}

/// A keyframe value as its property serializes it (`rgb(1,2,3)` ->
/// `rgb(1, 2, 3)`); `None` when the property is unknown or the value invalid
/// for it (Web Animations 1 §6.6.2 "process a keyframes argument").
fn normalized_value(name: &str, value: &str) -> Option<String> {
    if !is_property_name(name) || value.contains(';') || value.contains('{') || value.contains('}') {
        return None;
    }
    if name.starts_with("--") {
        return Some(value.trim().to_string());
    }
    let block = narsil_css::cssom::block::Block::lesen(&format!("{name}: {value}"));
    let w = block.wert(name);
    (!w.is_empty()).then_some(w)
}

/// The longhands of a property (itself for a longhand, empty when unknown).
fn longhands(name: &str) -> Vec<String> {
    if name.starts_with("--") {
        return vec![name.to_string()];
    }
    let Some(canonical) = narsil_css::cssom::block::name_kanonisch(name) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    let mut todo = vec![canonical];
    let mut steps = 0;
    while let Some(n) = todo.pop() {
        steps += 1;
        if steps > 256 {
            break;
        }
        match narsil_css::cssom::kurz::langformen(&n) {
            Some(list) => todo.extend(list.iter().rev().map(|s| s.to_string())),
            None => {
                if narsil_css::cssom::ist_lang(&n) && !out.contains(&n) {
                    out.push(n);
                }
            }
        }
    }
    out
}

/// A property name: letters, digits, `-` and `_` (custom properties too).
fn is_property_name(name: &str) -> bool {
    !name.is_empty() && name.len() < 256 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A value cut at the first `;`, `{` or `}` outside a string or a block, so
/// it stays one declaration when appended to a `style` attribute.
fn sanitize_value(value: &str) -> String {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut out = String::with_capacity(value.len());
    let mut escaped = false;
    for c in value.chars() {
        if escaped {
            escaped = false;
            out.push(c);
            continue;
        }
        match (quote, c) {
            (_, '\\') => escaped = true,
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth = (depth - 1).max(0),
            (None, ';' | '{' | '}') if depth == 0 => break,
            (None, '{' | '}') => break,
            _ => {}
        }
        out.push(c);
    }
    if quote.is_some() || escaped {
        return String::new();
    }
    out.trim().to_string()
}

/// Stores the animated values of an element; on a change the page tree and
/// the computed styles are rebuilt.
fn set_values(w: &mut Stand, id: u32, values: Vec<(String, String)>) {
    // `None`: nothing changed; `Some(k, paint)`: changed, and whether only
    // paint-only properties did (`anim_schnell`: no rebuild, no layout).
    let changed = match w.lebend.as_mut() {
        Some(l) => match l.knoten(id) {
            Some(k) => {
                let old = l.animated.get(&k).cloned().unwrap_or_default();
                if old == values {
                    None
                } else {
                    let paint = old.iter().chain(values.iter()).all(|(n, _)| crate::anim_schnell::paint_only(n));
                    if values.is_empty() {
                        l.animated.remove(&k);
                    } else {
                        l.animated.insert(k, values);
                    }
                    if paint {
                        l.anim_dirty.insert(k);
                    } else {
                        l.geaendert = true;
                    }
                    Some((k, paint))
                }
            }
            None => None,
        },
        None => None,
    };
    match changed {
        // Opacity and background color are not inherited and move nothing:
        // only the element's own computed style is stale.
        Some((k, true)) => {
            w.stil_cache.remove(&k);
        }
        Some((_, false)) => {
            w.stil_cache.clear();
            w.layout_veraltet = true;
        }
        None => {}
    }
}

/// The computed value of `name` on element `id` without its own animated
/// values (`anim_schnell`, when an animation stops setting it).
pub(crate) fn base_value(w: &mut Stand, id: u32, name: &str) -> Option<String> {
    computed_with(w, id, name, None)
}

/// Stores the animated values of a pseudo-element of an element.
fn set_pseudo_values(w: &mut Stand, id: u32, art: String, values: Vec<(String, String)>) {
    let Some(l) = w.lebend.as_mut() else { return };
    let Some(k) = l.knoten(id) else { return };
    let key = (k, art);
    let same = match l.animated_pseudo.get(&key) {
        Some(old) => *old == values,
        None => values.is_empty(),
    };
    if same {
        return;
    }
    if values.is_empty() {
        l.animated_pseudo.remove(&key);
    } else {
        l.animated_pseudo.insert(key, values);
    }
    l.geaendert = true;
    w.stil_cache.clear();
    w.layout_veraltet = true;
}

/// The computed value of `name` on a pseudo-element without its animated
/// values.
fn pseudo_base_value(w: &mut Stand, id: u32, art: &str, name: &str) -> Option<String> {
    crate::dom::refresh_styles(w);
    let k = w.lebend.as_ref().and_then(|l| l.knoten(id))?;
    let key = (k, art.to_string());
    let saved = w.lebend.as_mut().and_then(|l| l.animated_pseudo.remove(&key));
    let r = match (w.lebend.as_ref(), w.stil_satz.as_ref()) {
        (Some(l), Some(satz)) => crate::lebend::pseudo_berechnet(l, k, art, name, satz, &mut w.stil_cache),
        _ => None,
    };
    if let (Some(s), Some(l)) = (saved, w.lebend.as_mut()) {
        l.animated_pseudo.insert(key, s);
    }
    r
}

/// The computed values of `names` on the element without its own animated
/// values (one cascade pass for all of them).
fn base_values(w: &mut Stand, id: u32, names: &[String]) -> Vec<Option<String>> {
    crate::dom::refresh_styles(w);
    let Some(k) = w.lebend.as_ref().and_then(|l| l.knoten(id)) else {
        return vec![None; names.len()];
    };
    let saved = w.lebend.as_mut().and_then(|l| l.animated.remove(&k));
    let old_entry = if saved.is_some() { w.stil_cache.remove(&k) } else { None };
    let result = match (w.lebend.as_ref(), w.stil_satz.as_ref()) {
        (Some(l), Some(satz)) => match crate::lebend::berechnet(l, k, satz, &mut w.stil_cache) {
            Some(b) => names
                .iter()
                .map(|name| {
                    if name.starts_with("--") {
                        return b.variablen.holen(name).map(|t| t.trim().to_string());
                    }
                    let name = narsil_css::cssom::block::name_kanonisch(name).unwrap_or_else(|| name.clone());
                    // A property nobody declares for the element and that does
                    // not inherit has its initial value — without the search
                    // through the style sheets (`transition: all` asks for
                    // dozens of them).
                    let cascade_known = narsil_css::berechnet_css::berechneter_wert(&b, &name);
                    if cascade_known.is_none()
                        && !narsil_css::cssom::vererbt(&name)
                        && !b.schluessel.iter().any(|(n, _)| *n == name || narsil_css::cssom::kurz::langformen(n).is_some_and(|ls| ls.contains(&name.as_str())))
                        && crate::lebend::stil_spez(l, k, &name).is_none()
                        && let Some(initial) = narsil_css::cssom::anfangswert(&name)
                    {
                        return Some(initial.to_string());
                    }
                    // `currentcolor` computes to itself for every property but
                    // `color` (CSS Color 4 §4.4): a transition sees the keyword.
                    if name != "color"
                        && name.ends_with("color")
                        && crate::lebend::stil_spez(l, k, &name).is_some_and(|s| s.trim().eq_ignore_ascii_case("currentcolor"))
                    {
                        return Some(String::from("currentcolor"));
                    }
                    let cascade = narsil_css::berechnet_css::berechneter_wert(&b, &name);
                    crate::lebend::cssom_berechnet(l, k, &name, cascade.as_deref(), satz, &mut w.stil_cache).or(cascade)
                })
                .collect(),
            None => vec![None; names.len()],
        },
        _ => vec![None; names.len()],
    };
    if let Some(s) = saved {
        if let Some(l) = w.lebend.as_mut() {
            l.animated.insert(k, s);
        }
        w.stil_cache.remove(&k);
        if let Some(e) = old_entry {
            w.stil_cache.insert(k, e);
        }
    }
    result
}

/// The computed values of `names` in the element's starting style
/// (`@starting-style`); `None` where no such rule decides the value.
fn starting_values(w: &mut Stand, id: u32, names: &[String]) -> Vec<Option<String>> {
    crate::dom::refresh_styles(w);
    // Whether any sheet has `@starting-style`, once per sheet state.
    let key = (w.sheet_generation, w.stil_satz.as_ref().map_or(0, |s| std::sync::Arc::as_ptr(&s.blaetter) as usize));
    let (last, any) = STARTING_CHECKED.with(std::cell::Cell::get);
    let any = if last == key {
        any
    } else {
        let any = w.lebend.as_ref().is_some_and(crate::lebend::has_starting_style);
        STARTING_CHECKED.with(|c| c.set((key, any)));
        any
    };
    if !any {
        return vec![None; names.len()];
    }
    let specified: Vec<Option<String>> = match (w.lebend.as_ref(), w.stil_satz.as_ref()) {
        (Some(l), Some(satz)) => match l.knoten(id) {
            // A starting style without a box (`display: none`) starts nothing.
            Some(k) if crate::lebend::starting_style_value(l, k, "display", &satz.zustand).is_some_and(|d| d.trim().eq_ignore_ascii_case("none")) => {
                return vec![None; names.len()];
            }
            Some(k) => names.iter().map(|n| crate::lebend::starting_style_value(l, k, n, &satz.zustand)).collect(),
            None => return vec![None; names.len()],
        },
        _ => return vec![None; names.len()],
    };
    names
        .iter()
        .zip(specified)
        .map(|(n, s)| s.and_then(|v| computed_with(w, id, n, Some(&sanitize_value(&v)))))
        .collect()
}

/// The computed value of `name` on the element when its animated values are
/// replaced by `value` (`None`: by nothing). Only the element's own cache
/// entry is touched; its descendants do not read it here. No layout is
/// forced: an animation interpolates computed values, not used ones.
fn computed_with(w: &mut Stand, id: u32, name: &str, value: Option<&str>) -> Option<String> {
    crate::dom::refresh_styles(w);
    let k = w.lebend.as_ref().and_then(|l| l.knoten(id))?;
    let name = narsil_css::cssom::block::name_kanonisch(name).unwrap_or_else(|| name.to_string());
    let saved = {
        let l = w.lebend.as_mut()?;
        match value {
            // Marked like a transition value, so no declaration of the page
            // (`!important` ones included) hides it.
            Some(v) => l.animated.insert(k, vec![(name.clone(), format!("{v} !important"))]),
            None => l.animated.remove(&k),
        }
    };
    let touched = value.is_some() || saved.is_some();
    let old_entry = if touched { w.stil_cache.remove(&k) } else { None };
    let result = match (w.lebend.as_ref(), w.stil_satz.as_ref()) {
        (Some(l), Some(satz)) => crate::lebend::berechnet(l, k, satz, &mut w.stil_cache).and_then(|b| {
            if name.starts_with("--") {
                return b.variablen.holen(&name).map(|t| t.trim().to_string());
            }
            let cascade = narsil_css::berechnet_css::berechneter_wert(&b, &name);
            crate::lebend::cssom_berechnet(l, k, &name, cascade.as_deref(), satz, &mut w.stil_cache).or(cascade)
        }),
        _ => None,
    };
    if let Some(l) = w.lebend.as_mut() {
        match saved {
            Some(s) => {
                l.animated.insert(k, s);
            }
            None => {
                l.animated.remove(&k);
            }
        }
    }
    if touched {
        w.stil_cache.remove(&k);
        if let Some(e) = old_entry {
            w.stil_cache.insert(k, e);
        }
    }
    result
}

/// The animation properties a CSS animation or transition is made of, in
/// the order `css_scan` reports them.
const CSS_PROPERTIES: [&str; 14] = [
    "animation-name",
    "animation-duration",
    "animation-timing-function",
    "animation-iteration-count",
    "animation-direction",
    "animation-fill-mode",
    "animation-play-state",
    "animation-delay",
    "animation-composition",
    "transition-property",
    "transition-duration",
    "transition-timing-function",
    "transition-delay",
    "transition-behavior",
];

/// Most elements one scan reports, and most nodes it visits.
const MAX_SCAN_ELEMENTS: usize = 20_000;
const MAX_SCAN_NODES: usize = 500_000;

fn is_animation_property(name: &str) -> bool {
    name.starts_with("animation") || name.starts_with("transition")
}

/// The elements with CSS animation or transition properties, in tree order:
/// `[generation, full, el, [values of CSS_PROPERTIES], el, […], …]`.
/// Elements in a `display: none` subtree have none (CSS Animations 1 §3).
/// With `roots` (the elements the DOM changed at) only their subtrees are
/// scanned (`full` false), unless the style sheets changed since the last
/// full scan. `Null` when the styles did not change since generation `seen`.
fn css_scan(w: &mut Stand, seen: Option<f64>, roots: Option<Vec<u32>>) -> Wert {
    let t0 = std::time::Instant::now();
    let zeiten = std::env::var_os("NARSIL_ANIM_ZEITEN").is_some();
    crate::dom::refresh_styles(w);
    let generation = w.style_generation;
    if seen == Some(generation as f64) {
        return Wert::Null;
    }
    let t_refresh = t0.elapsed();
    let n_roots = roots.as_ref().map_or(0, Vec::len);
    let full = roots.is_none() || w.anim_full_scan != w.sheet_generation.wrapping_add(1);
    let sheet_key = (w.sheet_generation, w.stil_satz.as_ref().map_or(0, |s| std::sync::Arc::as_ptr(&s.blaetter) as usize));
    if full {
        w.anim_full_scan = w.sheet_generation.wrapping_add(1);
    }
    let mut found: Vec<(usize, Vec<String>)> = Vec::new();
    let mut pseudo_found: Vec<(usize, Vec<String>)> = Vec::new();
    let mut found_pseudo: Vec<(usize, &'static str)> = Vec::new();
    {
        let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref()) else {
            return Wert::Feld(vec![Wert::Zahl(generation as f64), Wert::Wahr(full)]);
        };
        let cache = &mut w.stil_cache;
        let mut stack: Vec<usize> = Vec::new();
        if full {
            stack.push(l.wurzel);
        } else {
            for id in roots.iter().flatten().rev() {
                let Some(k) = l.knoten(*id) else { continue };
                // Not below a `display: none` ancestor.
                let mut hidden = false;
                let mut x = l.dok.eltern(k);
                while let Some(e) = x {
                    if l.dok.ist_element_knoten(e) && crate::lebend::berechnet(l, e, satz, cache).is_some_and(|b| b.anzeige == narsil_css::Anzeige::Keins) {
                        hidden = true;
                        break;
                    }
                    x = l.dok.eltern(e);
                }
                if !hidden {
                    stack.push(k);
                }
            }
        }
        let mut done: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut visited = 0usize;
        // Pseudo-elements with animation properties (rare: the rules are
        // read once per sheet state, the properties only for matching
        // elements).
        let pseudo_rules = pseudo_rules_cached(l, sheet_key);
        while let Some(k) = stack.pop() {
            visited += 1;
            if visited > MAX_SCAN_NODES || found.len() >= MAX_SCAN_ELEMENTS {
                break;
            }
            if !full && !done.insert(k) {
                continue;
            }
            if l.dok.ist_element_knoten(k) {
                let Some(b) = crate::lebend::berechnet(l, k, satz, cache) else { continue };
                if b.anzeige == narsil_css::Anzeige::Keins {
                    continue;
                }
                for (sels, art) in pseudo_rules.iter() {
                    if found_pseudo.contains(&(k, *art)) || !crate::lebend::element_matches(l, k, sels, &satz.zustand) {
                        continue;
                    }
                    found_pseudo.push((k, *art));
                    let mut values: Vec<String> = CSS_PROPERTIES
                        .iter()
                        .map(|p| crate::lebend::pseudo_berechnet(l, k, art, p, satz, cache).unwrap_or_default())
                        .collect();
                    values.push(format!("::{art}"));
                    pseudo_found.push((k, values));
                }
                if b.schluessel.iter().any(|(n, _)| is_animation_property(n)) {
                    // A group (`animation-*`, `transition-*`) the element does
                    // not declare has its initial values — without the
                    // search through the style sheets.
                    let declares = |group: &str| b.schluessel.iter().any(|(n, _)| n.starts_with(group));
                    let (animations, transitions) = (declares("animation"), declares("transition"));
                    let values = CSS_PROPERTIES
                        .iter()
                        .map(|p| {
                            let declared = if p.starts_with("animation") { animations } else { transitions };
                            if !declared {
                                return narsil_css::cssom::anfangswert(p).unwrap_or_default().to_string();
                            }
                            let cascade = narsil_css::berechnet_css::berechneter_wert(&b, p);
                            crate::lebend::cssom_berechnet(l, k, p, cascade.as_deref(), satz, cache)
                                .or(cascade)
                                .unwrap_or_default()
                        })
                        .collect();
                    found.push((k, values));
                }
            }
            for &c in l.dok.kinder(k).iter().rev() {
                stack.push(c);
            }
        }
    }
    if zeiten {
        eprintln!("ANIM-SCAN full={full} roots={n_roots} found={} refresh={:?} total={:?}", found.len(), t_refresh, t0.elapsed());
    }
    let mut out = Vec::with_capacity(found.len() * 2 + 2);
    out.push(Wert::Zahl(generation as f64));
    out.push(Wert::Wahr(full));
    found.extend(pseudo_found);
    if let Some(l) = w.lebend.as_mut() {
        for (k, values) in found {
            let Some(id) = l.host(k) else { continue };
            out.push(crate::dom::host_wert(id));
            out.push(Wert::Feld(values.into_iter().map(Wert::Text).collect()));
        }
    }
    Wert::Feld(out)
}

type PseudoRules = std::rc::Rc<Vec<(Vec<narsil_css::Selektor>, &'static str)>>;

thread_local! {
    /// The pseudo-element animation rules of the last sheet state.
    static PSEUDO_RULES: std::cell::RefCell<Option<((u64, usize), PseudoRules)>> = const { std::cell::RefCell::new(None) };
}

/// `lebend::pseudo_animation_rules`, once per sheet state (parsing the
/// sheets for them costs as much as a style computation).
fn pseudo_rules_cached(l: &crate::lebend::Lebend, key: (u64, usize)) -> PseudoRules {
    if let Some(r) = PSEUDO_RULES.with(|c| c.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, r)| r.clone())) {
        return r;
    }
    let r: PseudoRules = std::rc::Rc::new(crate::lebend::pseudo_animation_rules(l));
    PSEUDO_RULES.with(|c| *c.borrow_mut() = Some((key, r.clone())));
    r
}

thread_local! {
    /// The style sheets last checked by `needed` (their address) and the answer.
    static SHEETS_CHECKED: std::cell::Cell<(usize, bool)> = const { std::cell::Cell::new((0, false)) };
    /// The sheet state last checked for `@starting-style` and the answer.
    static STARTING_CHECKED: std::cell::Cell<((u64, usize), bool)> = const { std::cell::Cell::new(((u64::MAX, 0), false)) };
}

/// Does the page need the animation module before a script asks for it? When
/// a style sheet declares an animation or transition property, or the
/// element `id` does so in its `style` attribute.
fn needed(w: &mut Stand, id: Option<u32>) -> bool {
    let mentions = |style: &str| {
        let low = style.to_ascii_lowercase();
        low.contains("animation") || low.contains("transition")
    };
    if let Some(l) = w.lebend.as_ref() {
        match id {
            Some(id) => {
                if let Some(k) = l.knoten(id)
                    && l.dok.attribut(k, "style").is_some_and(mentions)
                {
                    return true;
                }
            }
            // The whole document: any element's `style` attribute.
            None => {
                let n = l.dok.knoten.len().min(MAX_SCAN_NODES);
                if (0..n).any(|k| l.dok.ist_element_knoten(k) && l.dok.attribut(k, "style").is_some_and(mentions)) {
                    return true;
                }
            }
        }
    }
    crate::dom::refresh_styles(w);
    let Some(satz) = w.stil_satz.as_ref() else { return false };
    let key = std::sync::Arc::as_ptr(&satz.blaetter) as usize;
    let (last, answer) = SHEETS_CHECKED.with(std::cell::Cell::get);
    if last == key {
        return answer;
    }
    let answer = satz.blaetter.iter().any(sheet_mentions_animations);
    SHEETS_CHECKED.with(|c| c.set((key, answer)));
    answer
}

fn sheet_mentions_animations(sheet: &narsil_css::Stylesheet) -> bool {
    use narsil_css::{Regel, StilRegel, Wert as CssWert};
    let rule = |r: &StilRegel| r.deklarationen.iter().any(|d| matches!(&d.wert, CssWert::Schluessel(n, _) if is_animation_property(n)));
    sheet.regeln.iter().any(|r| match r {
        Regel::Stil(s) => rule(s),
        Regel::Media { regeln, .. } | Regel::Supports { regeln, .. } | Regel::Layer { regeln, .. } => regeln.iter().any(rule),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_stay_one_declaration() {
        assert_eq!(sanitize_value("10px; color: red"), "10px");
        assert_eq!(sanitize_value("url(\"a;b\") 1px"), "url(\"a;b\") 1px");
        assert_eq!(sanitize_value("var(--x, {a})"), "var(--x,");
        assert_eq!(sanitize_value("\"open"), "");
        assert!(is_property_name("--my_prop"));
        assert!(!is_property_name("color:red"));
    }
}
