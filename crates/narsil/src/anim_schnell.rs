//! Release 15.10.2026 (Agent C): animation frames without a page rebuild.
//!
//! An animation frame changes the animated values of a few elements
//! (`Lebend::animated`). A full rebuild (cascade and layout of the whole
//! page) costs up to seconds on big pages, so a running spinner or shimmer
//! kept the page rebuilding. Some properties need neither:
//! - `opacity` and `background-color`: the display list is built on every
//!   paint from the layout tree and the node styles of the page document
//!   (`Style::deckkraft`, `Style::background`);
//! - `transform`, `translate`, `rotate`, `scale`: layout only copies the
//!   matrix into `LayoutBox::transform`; painting, geometry (`masse_rechnen`)
//!   and hit testing read it from there (`LayoutBox::abbildung`).
//! This module writes the new values into the shown page and asks for a
//! repaint.
//!
//! Everything else (layout-affecting properties, a background on a box that
//! had none when it was laid out, a transform that appears or goes away —
//! it makes or ends a stacking context —, elements the page does not show)
//! takes the full rebuild as before.
//!
//! MEASURED before (website_diagnose, heise.de): one rebuild 0.6-1.2 s; an
//! animation frame set `Lebend::geaendert`, so every frame asked for one
//! (throttled to 250 ms).

use std::collections::HashMap;

use crate::dom::Stand;
use crate::sitzung::Angezeigt;

/// The properties painted without a rebuild.
pub(crate) fn paint_only(name: &str) -> bool {
    matches!(name, "opacity" | "background-color") || is_transform(name)
}

fn is_transform(name: &str) -> bool {
    matches!(name, "transform" | "translate" | "rotate" | "scale")
}

/// The outcome of [`after_scripts`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// No animated values were waiting.
    Nothing,
    /// The new values are in the shown page: repaint, no rebuild.
    Painted,
    /// The fast path could not apply them: rebuild the page (the page HTML
    /// is already renewed).
    Rebuild,
}

/// How often the fast path painted (tests, diagnosis).
pub(crate) static PAINTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// After scripts ran: paint the waiting animated values into the shown page
/// (`rebuilt`: a full rebuild comes anyway, which carries them).
pub(crate) fn after_scripts(a: &mut Angezeigt, rebuilt: bool) -> Outcome {
    let pending: Vec<usize> = {
        let mut w = a.stand.lock().expect("DOM");
        let Some(l) = w.lebend.as_mut() else { return Outcome::Nothing };
        if l.anim_dirty.is_empty() {
            return Outcome::Nothing;
        }
        l.anim_dirty.drain().collect()
    };
    if rebuilt {
        return Outcome::Nothing;
    }
    let painted = apply(a, &pending);
    if painted {
        PAINTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    // The page HTML carries the animated values too, so that a rebuild for
    // another reason (window size, images) keeps them. That is a walk over
    // the DOM, no cascade and no layout.
    if let Some(l) = a.stand.lock().expect("DOM").lebend.as_mut() {
        l.geaendert = true;
    }
    if let Some(html) = crate::js::lebend_abschliessen(&a.stand, &mut a.patch) {
        a.html = std::sync::Arc::new(html);
    }
    if painted { Outcome::Painted } else { Outcome::Rebuild }
}

/// The value of `name` for element `k`: the animated value, or (when the
/// animation no longer sets it) the value without animations.
fn value_of(w: &mut Stand, k: usize, name: &str) -> Option<String> {
    let animated = w.lebend.as_ref()?.animated.get(&k).and_then(|v| v.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v.clone()));
    match animated {
        Some(v) => Some(v.strip_suffix(" !important").unwrap_or(&v).to_string()),
        None => {
            let host = w.lebend.as_mut()?.host(k)?;
            crate::anim_hosts::base_value(w, host, name)
        }
    }
}

/// The new transform of an element: the CSS value is `none` everywhere, or
/// the matrix (`None`: the identity).
enum NewTransform {
    NoneValue,
    Matrix(Option<narsil_layout::Transformation>),
}

/// What one element gets.
struct Update {
    node: narsil_dom::NodeId,
    opacity: f32,
    background: narsil_core::color::Color,
    transform: Option<NewTransform>,
}

/// Writes the values into the shown page; `false` when one element needs
/// the full rebuild.
fn apply(a: &mut Angezeigt, pending: &[usize]) -> bool {
    let mut w = a.stand.lock().expect("DOM");
    let ord_of_host: HashMap<u32, usize> = w.ids.iter().map(|(o, h)| (*h, *o)).collect();
    // The page containers of the elements' ordinals, in one pass.
    let mut wanted: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for &k in pending {
        if let Some(h) = w.lebend.as_mut().and_then(|l| l.host(k))
            && let Some(o) = ord_of_host.get(&h)
        {
            wanted.insert(*o);
        }
    }
    let mut containers: HashMap<usize, Vec<narsil_dom::NodeId>> = HashMap::new();
    for n in a.seite.document.nodes() {
        if matches!(n.kind, narsil_dom::NodeKind::Container)
            && let Some(ord) = a.seite.dom_zu_ord.get(&(n.id.index() as u32))
            && wanted.contains(ord)
        {
            containers.entry(*ord).or_default().push(n.id);
        }
    }
    let mut updates: Vec<Update> = Vec::new();
    for &k in pending {
        let Some(l) = w.lebend.as_mut() else { return false };
        let names: Vec<String> = l.animated.get(&k).map(|v| v.iter().map(|(n, _)| n.clone()).collect()).unwrap_or_default();
        // A changed property that is not paint-only needs the rebuild.
        if names.iter().any(|n| !paint_only(n)) {
            return false;
        }
        let Some(host) = l.host(k) else { return false };
        let Some(&ord) = ord_of_host.get(&host) else {
            // Not in the page (yet): nothing to paint, the next rebuild
            // carries the values.
            continue;
        };
        let Some(list) = containers.get(&ord) else { continue };
        // The element's own box: the container whose parent has another
        // ordinal; the others are anonymous boxes inside it.
        let doc = &a.seite.document;
        let Some(&node) = list.iter().find(|id| {
            doc.get(**id).and_then(|n| n.parent).is_none_or(|p| a.seite.dom_zu_ord.get(&(p.index() as u32)) != Some(&ord))
        }) else {
            continue;
        };
        let Some(opacity) = value_of(&mut w, k, "opacity").and_then(|o| o.trim().parse::<f32>().ok()).filter(|o| o.is_finite()) else {
            return false;
        };
        let Some(bg) = value_of(&mut w, k, "background-color").and_then(|v| narsil_css::farbe_aus(&v)) else {
            return false;
        };
        // The transform only when one of its properties is animated (or was:
        // the element is dirty, its values may have gone).
        let transform = match names.iter().any(|n| is_transform(n)) || had_transform_animation(a, node) {
            true => match transform_of(&mut w, k) {
                Some(t) => Some(t),
                None => return false,
            },
            false => None,
        };
        updates.push(Update {
            node,
            opacity: opacity.clamp(0.0, 1.0),
            background: narsil_core::color::Color { r: bg.r, g: bg.g, b: bg.b, a: bg.a },
            transform,
        });
    }
    // Checks before any write.
    for u in &updates {
        let Some(n) = a.seite.document.get(u.node) else { continue };
        // A background on a box laid out without one has no area to paint
        // into (`LayoutBox::background`).
        if !u.background.is_transparent() && n.style.background.is_transparent() && !find_box(&a.baum.root, u.node, 0, &mut |bx| bx.background.is_some()) {
            return false;
        }
        // A transform needs the slot of the box it was laid out with: one
        // that appears or goes away changes the stacking context.
        if let Some(t) = &u.transform {
            let had = find_box(&a.baum.root, u.node, 0, &mut |bx| bx.transform.is_some());
            if !had || matches!(t, NewTransform::NoneValue) {
                return false;
            }
        }
    }
    let mut geometry = false;
    for u in updates {
        let Some(n) = a.seite.document.get(u.node) else { continue };
        if n.style.deckkraft != u.opacity || n.style.background != u.background {
            let mut style = n.style.clone();
            style.deckkraft = u.opacity;
            style.background = u.background;
            if a.seite.document.set_style(u.node, style).is_err() {
                return false;
            }
        }
        if let Some(NewTransform::Matrix(m)) = u.transform {
            set_transform(&mut a.baum.root, u.node, m, 0, &mut geometry);
        }
    }
    // Geometry for scripts (`getBoundingClientRect` …) follows the matrix.
    if geometry {
        let masse = crate::sitzung::masse_rechnen(&a.baum, &a.seite, &w.ids);
        w.masse = masse;
    }
    true
}

/// Has the element's box a transform (so an animation may have set it)?
fn had_transform_animation(a: &Angezeigt, node: narsil_dom::NodeId) -> bool {
    find_box(&a.baum.root, node, 0, &mut |bx| bx.transform.is_some())
}

/// The element's transform as the page builder computes it (its cascade
/// with the animated values, `narsil_css::layout_stil`).
fn transform_of(w: &mut Stand, k: usize) -> Option<NewTransform> {
    crate::dom::refresh_styles(w);
    let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref()) else { return None };
    let b = crate::lebend::berechnet(l, k, satz, &mut w.stil_cache)?;
    let css = b
        .schluessel
        .iter()
        .any(|(n, v)| is_transform(n) && !v.trim().eq_ignore_ascii_case("none") && !v.trim().is_empty());
    if !css {
        return Some(NewTransform::NoneValue);
    }
    Some(NewTransform::Matrix(narsil_css::layout_stil(&b).transform))
}

fn find_box(bx: &narsil_layout::LayoutBox, node: narsil_dom::NodeId, depth: usize, test: &mut dyn FnMut(&narsil_layout::LayoutBox) -> bool) -> bool {
    if bx.node == node && test(bx) {
        return true;
    }
    depth < 512 && bx.children.iter().any(|c| find_box(c, node, depth + 1, test))
}

/// Writes the matrix into the element's boxes that have a transform slot;
/// the identity stays a matrix (the stacking context of the CSS value
/// stays, and so does the slot for the next frame).
fn set_transform(bx: &mut narsil_layout::LayoutBox, node: narsil_dom::NodeId, t: Option<narsil_layout::Transformation>, depth: usize, changed: &mut bool) {
    if bx.node == node
        && let Some(alt) = bx.transform.as_deref()
    {
        let neu = t.unwrap_or(narsil_layout::Transformation { lin: [1.0, 0.0, 0.0, 1.0], tx: [0.0; 3], ty: [0.0; 3], ursprung: alt.ursprung });
        if *alt != neu {
            bx.transform = Some(Box::new(neu));
            *changed = true;
        }
    }
    if depth < 512 {
        for c in &mut bx.children {
            set_transform(c, node, t, depth + 1, changed);
        }
    }
}
#[cfg(test)]
mod tests {
    use crate::Sitzung;

    fn tick(s: &mut Sitzung, ms: u64) {
        let ende = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        while std::time::Instant::now() < ende {
            s.tick(std::time::Instant::now());
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
    }

    /// Loads `html`, lets its timer start a paused animation, and compares
    /// the fast path's display list with a full rebuild of the same frame.
    fn compare(html: &str, name: &str) {
        let path = std::env::temp_dir().join(format!("narsil_anim_schnell_{name}_{}.html", std::process::id()));
        std::fs::write(&path, html).expect("write page");
        let mut s = Sitzung::neu(400, 300);
        s.gehe_zu(&format!("file://{}", path.to_string_lossy())).expect("load page");
        let before = super::PAINTED.load(std::sync::atomic::Ordering::SeqCst);
        tick(&mut s, 700);
        let _ = std::fs::remove_file(&path);
        assert!(super::PAINTED.load(std::sync::atomic::Ordering::SeqCst) > before, "the fast path did not paint");
        let fast = format!("{:?}", s.sichtliste());
        s.seite_neu_bauen_sofort();
        let full = format!("{:?}", s.sichtliste());
        assert_eq!(fast, full);
    }

    /// A paused opacity/background-color animation frame painted by the fast
    /// path gives the same display list as a full rebuild of that frame.
    #[test]
    fn fast_path_paints_like_a_rebuild() {
        compare(
            r#"<!doctype html><body style="margin:0"><div id="d" style="width:100px;height:100px;background-color:rgb(200,0,0)">x</div>
<script>
setTimeout(function () {
  var a = document.getElementById('d').animate({ opacity: [0, 1], backgroundColor: ['rgb(200, 0, 0)', 'rgb(0, 0, 200)'] }, 1000);
  a.pause();
  a.currentTime = 500;
}, 50);
</script></body>"#,
            "paint",
        );
    }

    /// A transform frame (the box was laid out with a transform) is painted
    /// like a rebuild, translate and rotate alike.
    #[test]
    fn fast_path_transforms_like_a_rebuild() {
        compare(
            r#"<!doctype html><body style="margin:0"><div id="d" style="width:100px;height:100px;background:rgb(0,120,200);transform:translateX(5px)">x</div>
<script>
setTimeout(function () {
  var a = document.getElementById('d').animate({ transform: ['translateX(10px) rotate(0deg)', 'translateX(110px) rotate(90deg)'] }, 1000);
  a.pause();
  a.currentTime = 300;
}, 300);
</script></body>"#,
            "transform",
        );
    }
}
