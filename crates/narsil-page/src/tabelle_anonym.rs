//! Release 11.10.2026: anonyme Tabellenobjekte (CSS 2.1 §17.2.1).
//!
//! Das Tabellen-Layout erwartet Tabelle -> Zeile (oder Zeilengruppe) ->
//! Zelle. Fehlt ein Glied — `display: table-cell` direkt in einem Block,
//! Zeilen ohne Tabelle —, ergaenzt der Standard anonyme Kaesten:
//!
//! 1. Ein Lauf aufeinanderfolgender Zellen, deren Eltern keine Zeile ist,
//!    bekommt eine anonyme Zeile.
//! 2. Ein Lauf aufeinanderfolgender Zeilen (und Zeilengruppen/Beschriftungen),
//!    deren Eltern weder Tabelle noch Zeilengruppe ist, bekommt eine anonyme
//!    Tabelle.
//!
//! Leerraum zwischen den Gliedern eines Laufs faellt weg (§17.2.1 Schritt 1).
//!
//! GEMESSEN vorher (WPT `css/CSS2/tables/table-anonymous-objects-*`, 156
//! Dateien): Zellen direkt in einem Block standen UNTEREINANDER wie Bloecke.
//!
//! Release 11.10.2026 (Agent CSS2): der VOLLSTAENDIGE Algorithmus in seinen
//! drei Schritten, nach der genauen Rolle (`LayoutPlan::tabelle`):
//!
//! 1. Irrelevantes entfernen: Kinder von Spalten, Nicht-Spalten in
//!    Spaltengruppen, reiner Leerraum direkt in Tabelle, Zeilengruppe, Zeile
//!    und Spaltengruppe.
//! 2. Fehlende Kinder: was in einer Tabelle kein eigentliches Tabellenkind
//!    ist, bekommt eine anonyme Zeile; was in einer Zeilengruppe keine Zeile
//!    ist, ebenso; was in einer Zeile keine Zelle ist, eine anonyme Zelle.
//! 3. Fehlende Eltern: Zellen ausserhalb einer Zeile bekommen eine Zeile,
//!    falsch eingehaengte Tabellenkinder (Zeilen, Gruppen, Beschriftungen,
//!    Spalten) eine anonyme Tabelle.
//!
//! GEMESSEN vorher (WPT `table-anonymous-objects-088..092`, `-147`,
//! `empty-cells-applies-to-008`): Text und Bloecke direkt in einer Zeile
//! standen neben den Zellen statt in einer anonymen Zelle; eine Zeilengruppe
//! in einer Zeile wurde zur Zelle.

use narsil_core::style::Style;
use narsil_dom::{Document, NodeId, NodeKind, Span};
use narsil_layout::plan::{Anzeige as A, TabellenAngabe, TabellenRolle as R};
use narsil_layout::{LayoutPlan, LayoutStil};

fn anzeige(plan: &LayoutPlan, id: NodeId) -> Option<A> {
    plan.hole(id).map(|s| s.anzeige)
}

/// Die genaue Rolle eines Knotens — aus der Nebentabelle, sonst aus der
/// `Anzeige` und der Struktur (handgebaute Plaene ohne Rolle).
pub(crate) fn rolle(doc: &Document, plan: &LayoutPlan, id: NodeId) -> R {
    let r = plan.tabelle(id).rolle;
    if r != R::Keine {
        return r;
    }
    match anzeige(plan, id) {
        Some(A::Tabelle) => R::Tabelle,
        Some(A::TabelleZelle) => R::Zelle,
        Some(A::TabelleZeile) => {
            let kinder = doc.get(id).map(|n| n.children.as_slice()).unwrap_or(&[]);
            if kinder.iter().any(|k| anzeige(plan, *k) == Some(A::TabelleZelle))
                || !kinder.iter().any(|k| anzeige(plan, *k) == Some(A::TabelleZeile))
            {
                R::Zeile
            } else {
                R::Rumpf
            }
        }
        Some(A::TabelleRand) => {
            if doc.get(id).is_some_and(|n| n.children.is_empty()) {
                R::Spalte
            } else {
                R::Beschriftung
            }
        }
        _ => R::Keine,
    }
}

/// Absolut/fest positioniert: kein Teil der Tabellenstruktur, bekommt keine
/// anonyme Zeile oder Zelle (GEMESSEN vorher: WPT `top-applies-to-004`, die
/// absolut positionierte, zum Block gewordene Zeile landete in einer
/// 0 px breiten anonymen Zelle und schrumpfte auf 0 px).
fn aus_dem_fluss(plan: &LayoutPlan, id: NodeId) -> bool {
    plan.hole(id).is_some_and(|s| matches!(s.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest))
}

fn ist_tabelle(r: R) -> bool {
    matches!(r, R::Tabelle | R::InlineTabelle)
}

fn ist_gruppe(r: R) -> bool {
    matches!(r, R::Rumpf | R::Kopf | R::Fuss)
}

/// CSS 2.1 §17.2: „proper table child“.
fn ist_tabellenkind(r: R) -> bool {
    ist_gruppe(r) || matches!(r, R::Zeile | R::Beschriftung | R::Spalte | R::Spaltengruppe)
}

/// Reiner Leerraum: ein Textknoten nur aus Weissraum, oder eine Zeilenbox
/// des Seitenbauers (ohne eigenen Plan), deren Texte nur Weissraum sind.
fn ist_leerraum(doc: &Document, plan: &LayoutPlan, id: NodeId) -> bool {
    let Some(n) = doc.get(id) else { return false };
    let leer_text = |n: &narsil_dom::Node| n.kind == NodeKind::Text && n.text.as_deref().is_none_or(|t| t.trim().is_empty());
    match n.kind {
        NodeKind::Text => leer_text(n),
        NodeKind::Container => {
            plan.hole(id).is_none_or(|s| *s == LayoutStil::default())
                && !n.children.is_empty()
                && n.children.iter().all(|k| doc.get(*k).is_some_and(leer_text))
                && n.style.background.a == 0
                && !n.style.rahmen.ist_gezeichnet()
        }
        NodeKind::Page => false,
    }
}

/// Der Stil eines anonymen Kastens: die vererbbaren Eigenschaften des
/// Elternkastens, keine eigenen Kasten-Eigenschaften (CSS 2.1 §17.2.1:
/// „anonymous boxes inherit … from their parent“).
fn anonymer_stil(doc: &Document, eltern: NodeId) -> Style {
    let mut s = doc.get(eltern).map(|n| n.style.clone()).unwrap_or_default();
    s.size = narsil_core::length::Dimension::AUTO;
    s.margin = narsil_core::length::Spacing::ZERO;
    s.padding = narsil_core::length::Spacing::ZERO;
    s.background = narsil_core::color::Color::TRANSPARENT;
    s.direction = narsil_core::style::Direction::Column;
    s.radius = [0.0; 4];
    s.rahmen = Default::default();
    s.schatten_anzahl = 0;
    s.verlauf = None;
    s.deckkraft = 1.0;
    s
}

fn anonym(doc: &mut Document, plan: &mut LayoutPlan, eltern: NodeId, rolle: R) -> Option<NodeId> {
    // Release 11.10.2026 (Agent CSS2): steht die Tabelle in einem Textfluss
    // (der Eltern hat Text als Kinder — ein Inline-Fluss des Seitenbauers),
    // ist sie eine `inline-table` (§17.2.1 Schritt 3.2).
    let im_text = doc.get(eltern).is_some_and(|n| {
        n.children.iter().any(|k| {
            doc.get(*k).is_some_and(|c| c.kind == NodeKind::Text && c.text.as_deref().is_some_and(|t| !t.trim().is_empty()))
        })
    });
    let rolle = if rolle == R::Tabelle && im_text { R::InlineTabelle } else { rolle };
    let stil = anonymer_stil(doc, eltern);
    let huelle = doc.push_child(eltern, NodeKind::Container, None, Span::EMPTY).ok()?;
    let _ = doc.set_style(huelle, stil);
    let art = match rolle {
        R::Tabelle | R::InlineTabelle => A::Tabelle,
        R::Zelle => A::TabelleZelle,
        _ => A::TabelleZeile,
    };
    plan.set(huelle, LayoutStil { anzeige: art, inline_aussen: rolle == R::InlineTabelle, ..LayoutStil::default() });
    plan.set_tabelle(huelle, TabellenAngabe { rolle, ..TabellenAngabe::default() });
    Some(huelle)
}

/// Die Kinder von `eltern` neu ordnen: jeder Lauf von Knoten mit
/// `gehoert(knoten)` kommt in einen neuen anonymen Knoten mit `rolle`.
/// `leer_im_lauf`: reiner Leerraum zwischen zwei Gliedern gehoert zu
/// niemandem und faellt weg (sonst bleibt er, wo er ist).
fn gruppieren(
    doc: &mut Document,
    plan: &mut LayoutPlan,
    eltern: NodeId,
    gehoert: &dyn Fn(&Document, &LayoutPlan, NodeId) -> bool,
    rolle: R,
    leer_im_lauf: bool,
) {
    let Some(kinder) = doc.get(eltern).map(|n| n.children.clone()) else { return };
    let mut neu: Vec<NodeId> = Vec::with_capacity(kinder.len());
    let mut lauf: Vec<NodeId> = Vec::new();
    let mut leer_danach: Vec<NodeId> = Vec::new();
    let mut aendern = false;
    let abschliessen = |doc: &mut Document, plan: &mut LayoutPlan, lauf: &mut Vec<NodeId>, neu: &mut Vec<NodeId>| {
        if lauf.is_empty() {
            return;
        }
        let Some(huelle) = anonym(doc, plan, eltern, rolle) else {
            neu.append(lauf);
            return;
        };
        let _ = doc.kinder_setzen(huelle, std::mem::take(lauf));
        neu.push(huelle);
    };
    for k in kinder {
        if gehoert(doc, plan, k) {
            // Leerraum zwischen zwei Gliedern gehoert zu niemandem.
            leer_danach.clear();
            lauf.push(k);
            aendern = true;
        } else if leer_im_lauf && !lauf.is_empty() && ist_leerraum(doc, plan, k) {
            leer_danach.push(k);
        } else {
            abschliessen(doc, plan, &mut lauf, &mut neu);
            neu.append(&mut leer_danach);
            neu.push(k);
        }
    }
    abschliessen(doc, plan, &mut lauf, &mut neu);
    neu.append(&mut leer_danach);
    if aendern {
        let _ = doc.kinder_setzen(eltern, neu);
    }
}

/// Behaelt nur die Kinder, fuer die `bleibt` gilt.
fn behalten(doc: &mut Document, plan: &LayoutPlan, eltern: NodeId, bleibt: &dyn Fn(&Document, &LayoutPlan, NodeId) -> bool) {
    let Some(kinder) = doc.get(eltern).map(|n| n.children.clone()) else { return };
    let neu: Vec<NodeId> = kinder.iter().copied().filter(|k| bleibt(doc, plan, *k)).collect();
    if neu.len() != kinder.len() {
        let _ = doc.kinder_setzen(eltern, neu);
    }
}

/// Ergaenzt die fehlenden Zeilen und Tabellen (siehe Modulkopf).
pub(crate) fn ergaenzen(doc: &mut Document, plan: &mut LayoutPlan) {
    if !plan_hat_tabellenteile(plan) {
        return;
    }
    // Schritt 1: Irrelevantes entfernen.
    let anzahl = doc.len();
    for i in 0..anzahl {
        let id = NodeId(i as u32);
        match rolle(doc, plan, id) {
            R::Spalte => behalten(doc, plan, id, &|_, _, _| false),
            R::Spaltengruppe => behalten(doc, plan, id, &|d, p, k| rolle(d, p, k) == R::Spalte),
            r if ist_tabelle(r) || ist_gruppe(r) || r == R::Zeile => {
                behalten(doc, plan, id, &|d, p, k| !ist_leerraum(d, p, k));
            }
            _ => {}
        }
    }
    // Schritt 2: fehlende Kinder — erst Zeilen (in Tabellen und Gruppen),
    // dann Zellen (in allen Zeilen, auch den neuen).
    let anzahl = doc.len();
    for i in 0..anzahl {
        let id = NodeId(i as u32);
        let r = rolle(doc, plan, id);
        if ist_tabelle(r) {
            gruppieren(doc, plan, id, &|d, p, k| !ist_tabellenkind(rolle(d, p, k)) && !aus_dem_fluss(p, k), R::Zeile, false);
        } else if ist_gruppe(r) {
            gruppieren(doc, plan, id, &|d, p, k| rolle(d, p, k) != R::Zeile && !aus_dem_fluss(p, k), R::Zeile, false);
        }
    }
    let anzahl = doc.len();
    for i in 0..anzahl {
        let id = NodeId(i as u32);
        if rolle(doc, plan, id) == R::Zeile {
            gruppieren(doc, plan, id, &|d, p, k| rolle(d, p, k) != R::Zelle && !aus_dem_fluss(p, k), R::Zelle, false);
        }
    }
    // Schritt 3a: Zellen ausserhalb einer Zeile bekommen eine Zeile.
    let anzahl = doc.len();
    for i in 0..anzahl {
        let id = NodeId(i as u32);
        if rolle(doc, plan, id) == R::Zeile {
            continue;
        }
        let hat_zelle = doc.get(id).is_some_and(|n| n.children.iter().any(|k| rolle(doc, plan, *k) == R::Zelle));
        if hat_zelle {
            gruppieren(doc, plan, id, &|d, p, k| rolle(d, p, k) == R::Zelle, R::Zeile, true);
        }
    }
    // Schritt 3b: falsch eingehaengte Tabellenkinder bekommen eine Tabelle.
    let anzahl = doc.len();
    for i in 0..anzahl {
        let id = NodeId(i as u32);
        let e = rolle(doc, plan, id);
        if ist_tabelle(e) {
            continue;
        }
        let falsch = move |d: &Document, p: &LayoutPlan, k: NodeId| {
            let r = rolle(d, p, k);
            ist_tabellenkind(r) && !(r == R::Zeile && ist_gruppe(e)) && !(r == R::Spalte && e == R::Spaltengruppe)
        };
        let hat = doc.get(id).is_some_and(|n| n.children.iter().any(|k| falsch(doc, plan, *k)));
        if hat {
            gruppieren(doc, plan, id, &falsch, R::Tabelle, true);
        }
    }
    // Spalten und Spaltengruppen nach vorn: ihr Hintergrund liegt UNTER den
    // Zeilen und Zellen (CSS 2.1 §17.5.1), und gezeichnet wird in
    // Baumreihenfolge. Ihre Reihenfolge untereinander (= die Spalten) bleibt.
    // GEMESSEN vorher (WPT `table-anonymous-objects-093`): eine Spalte nach
    // der ersten Zeile uebermalte deren Text.
    let anzahl = doc.len();
    for i in 0..anzahl {
        let id = NodeId(i as u32);
        if !ist_tabelle(rolle(doc, plan, id)) {
            continue;
        }
        let Some(kinder) = doc.get(id).map(|n| n.children.clone()) else { continue };
        let ist_spalte = |k: &NodeId| matches!(rolle(doc, plan, *k), R::Spalte | R::Spaltengruppe);
        let mut neu: Vec<NodeId> = kinder.iter().copied().filter(|k| ist_spalte(k)).collect();
        if neu.is_empty() || kinder.iter().take(neu.len()).all(|k| ist_spalte(k)) {
            continue;
        }
        neu.extend(kinder.iter().copied().filter(|k| !ist_spalte(k)));
        let _ = doc.kinder_setzen(id, neu);
    }
}

fn plan_hat_tabellenteile(plan: &LayoutPlan) -> bool {
    plan.eintraege().any(|s| matches!(s.anzeige, A::Tabelle | A::TabelleZelle | A::TabelleZeile | A::TabelleRand))
}
