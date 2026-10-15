//! Release 13.10.2026 (Agent C): per-root `@scope` matching (CSS Cascade 6
//! §2.5) — scoping limits and scope proximity.
//!
//! `scope.rs` rewrites `@scope (A) to (B)` into ordinary style rules. Every
//! scoped subject carries `:-narsil-scope-in(A)`, followed by
//! `:-narsil-scope-limit(B')` when the scope has a limit
//! ([`Pseudo::ScopeIn`]). An element is in scope when some scoping root (an
//! inclusive ancestor matching `A`) exists with no scoping limit between that
//! root and the element (both inclusive). In `B'`, `:scope` (and the implicit
//! `:scope ` prefix) is `:-narsil-scope-anchor` ([`Pseudo::ScopeAnchor`]):
//! exactly the root currently being tested.
//!
//! MEASURED before (WPT css-cascade scope-evaluation "Multiple scopes, :scope
//! pseudo in to-selector", scope-nesting "Implicit :scope in <scope-end>"):
//! the limit applied to every root at once (`:not(:where(A) :where(B), …)`),
//! so an element in scope of an inner root dropped out as soon as an outer
//! root had a limit above it, and a root that itself matched `B` limited
//! itself.

use crate::typen::{Element, Selektor, Zustand, selektor_passt_mit};
use std::cell::Cell;

/// At most this many ancestors are considered (M3).
const MAX_ANCESTORS: usize = 1_024;

thread_local! {
    /// Depth (number of ancestors) of the scoping root being tested.
    static ANCHOR_DEPTH: Cell<Option<usize>> = const { Cell::new(None) };
    /// Proximity (generations between root and subject) of the innermost
    /// scope matched at the subject (read by `crate::kaskade::bieten`).
    static PROXIMITY: Cell<Option<u32>> = const { Cell::new(None) };
}

/// Before matching a selector: no scope matched yet.
pub(crate) fn clear_proximity() {
    PROXIMITY.with(|p| p.set(None));
}

/// Release 13.10.2026: scope proximity (CSS Cascade 6 §6.1) decides after
/// specificity and before order of appearance; unscoped rules have infinite
/// proximity. It lives in the low bits of the last specificity component —
/// for EVERY rule, so all rules compare alike. MEASURED before (WPT css-cascade
/// scope-proximity 2/5, scope-specificity): the later rule won instead of the
/// closer root.
pub fn specificity_with_proximity(spec: (u32, u32, u32)) -> (u32, u32, u32) {
    let proximity = PROXIMITY.with(Cell::get).map_or(0, |n| 0xFFFF - n.min(0xFFFE));
    (spec.0, spec.1, (spec.2.min(0xFFFF) << 16) | proximity)
}

/// Like `selektor_passt_mit`, returning the specificity including scope
/// proximity (for cascades outside this crate, e.g. `getComputedStyle` from
/// the sheets).
pub fn matches_with_proximity<E: Element>(sel: &Selektor, el: &E, state: &Zustand) -> Option<(u32, u32, u32)> {
    clear_proximity();
    selektor_passt_mit(sel, el, state).then(|| specificity_with_proximity(sel.spezifitaet))
}

fn depth<E: Element>(el: &E) -> usize {
    let mut n = 0;
    let mut x = el.eltern();
    while let Some(e) = x {
        n += 1;
        if n >= MAX_ANCESTORS {
            break;
        }
        x = e.eltern();
    }
    n
}

/// `:-narsil-scope-anchor`: is `el` the root currently being tested? Limits
/// are only tested along the ancestor chain of the subject, and `:scope` in a
/// limit sits on the ancestor chain of the limit — equal depth means the same
/// element.
pub(crate) fn is_anchor<E: Element>(el: &E) -> bool {
    ANCHOR_DEPTH.with(Cell::get).is_some_and(|d| depth(el) == d)
}

/// `:-narsil-scope-in(roots):-narsil-scope-limit(limits)` at `el`.
pub(crate) fn in_scope<E: Element>(el: &E, roots: &[Selektor], limits: &[Selektor], state: &Zustand) -> bool {
    let mut ancestors: Vec<E> = Vec::new();
    let mut x = el.eltern();
    while let Some(e) = x {
        if ancestors.len() >= MAX_ANCESTORS {
            break;
        }
        x = e.eltern();
        ancestors.push(e);
    }
    let d = ancestors.len();
    let at = |i: usize| -> &E { if i == 0 { el } else { &ancestors[i - 1] } };
    let anchor_before = ANCHOR_DEPTH.with(Cell::get);
    let proximity_before = PROXIMITY.with(Cell::get);
    let mut found = None;
    for i in 0..=d {
        let root = at(i);
        if !roots.iter().any(|s| selektor_passt_mit(s, root, state)) {
            continue;
        }
        if limits.is_empty() {
            found = Some(i);
            break;
        }
        ANCHOR_DEPTH.with(|a| a.set(Some(d - i)));
        let limited = (0..=i).any(|j| limits.iter().any(|g| selektor_passt_mit(g, at(j), state)));
        if !limited {
            found = Some(i);
            break;
        }
    }
    ANCHOR_DEPTH.with(|a| a.set(anchor_before));
    // The first scope at the subject is the innermost one (`scope.rs` puts
    // its own before the outer ones); whatever was set while testing roots
    // does not count.
    PROXIMITY.with(|p| p.set(proximity_before.or(found.map(|i| i as u32))));
    found.is_some()
}
