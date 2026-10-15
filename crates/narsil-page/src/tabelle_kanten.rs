//! Release 11.10.2026 (Agent CSS2): das Modell der ZUSAMMENFALLENDEN Rahmen
//! (CSS 2.1 §17.6.2, `border-collapse: collapse`).
//!
//! Jede Kante zwischen zwei Zellen hat EINEN Rahmen: den staerksten unter
//! den Rahmen der beiden Zellen, ihrer Zeilen, Zeilengruppen, Spalten,
//! Spaltengruppen und (am Rand) der Tabelle (§17.6.2.1: breiter gewinnt,
//! dann der Stil — double vor solid vor dashed vor dotted vor ridge vor
//! outset vor groove vor inset —, dann die Herkunft Zelle vor Zeile vor
//! Zeilengruppe vor Spalte vor Spaltengruppe vor Tabelle).
//!
//! Der Weg: hier, nach den anonymen Tabellenobjekten, bekommt jede Zelle die
//! gewonnenen Rahmen als ihren eigenen Stil; Zeilen, Gruppen und Spalten
//! verlieren ihre Rahmen (sie sind in den Zellen aufgegangen), die Tabelle
//! verliert ihren Innenabstand (§17.6.2: „the table has no padding“) und
//! behaelt als (durchsichtigen) Rahmen die HALBEN aeusseren Zellrahmen —
//! so breit ist sie nach §17.6.2. Das Tabellen-Layout legt die Zellen dann
//! mit ihren Rahmen MITTIG auf die Rasterlinien (`tabellen_leg`).
//!
//! GEMESSEN vorher (WPT `border-*-applies-to-*` mit `border-collapse`,
//! `collapsing-border-model-*`, `border-conflict-element-*`): Zeilen- und
//! Gruppenrahmen fehlten, benachbarte Zellrahmen lagen doppelt nebeneinander.

use narsil_core::color::Color;
use narsil_core::style::{Rahmen, RahmenStil};
use narsil_dom::{Document, NodeId};
use narsil_layout::LayoutPlan;
use narsil_layout::plan::TabellenRolle as R;

use crate::tabelle_anonym::rolle;

/// Hoechste Spalten- und Zeilenzahl, die hier aufgeloest wird (M3/M4).
const MAX_SPALTEN: usize = 1000;
const MAX_ZEILEN: usize = 5000;

/// Ein Rahmen-Kandidat einer Kante.
#[derive(Debug, Clone, Copy)]
struct Kante {
    breite: f32,
    stil: RahmenStil,
    farbe: Color,
    /// Herkunft: Zelle 6, Zeile 5, Zeilengruppe 4, Spalte 3, Spaltengruppe 2,
    /// Tabelle 1.
    rang: u8,
}

const KEINE: Kante = Kante { breite: 0.0, stil: RahmenStil::Keiner, farbe: Color::TRANSPARENT, rang: 0 };

fn stil_rang(s: RahmenStil) -> u8 {
    match s {
        RahmenStil::Keiner => 0,
        RahmenStil::Innen => 1,
        RahmenStil::Furche => 2,
        RahmenStil::Aussen => 3,
        RahmenStil::Wulst => 4,
        RahmenStil::Gepunktet => 5,
        RahmenStil::Gestrichelt => 6,
        RahmenStil::Durchgezogen => 7,
        RahmenStil::Doppelt => 8,
    }
}

fn wirksam(k: &Kante) -> f32 {
    if k.stil == RahmenStil::Keiner { 0.0 } else { k.breite }
}

/// §17.6.2.1: der staerkere zweier Rahmen; bei Gleichstand der erste
/// (der weiter links bzw. oben liegt).
fn staerker(a: Kante, b: Kante) -> Kante {
    let (wa, wb) = (wirksam(&a), wirksam(&b));
    if (wa - wb).abs() > 0.001 {
        return if wa > wb { a } else { b };
    }
    let (sa, sb) = (stil_rang(a.stil), stil_rang(b.stil));
    if sa != sb {
        return if sa > sb { a } else { b };
    }
    if b.rang > a.rang { b } else { a }
}

/// `staerker` mit dem Nachbarn `nb`; liegt er vorn (links/oben), gewinnt er
/// den Gleichstand (§17.6.2.1 Regel 4).
fn mit(w: Kante, nb: Kante, vorn: bool) -> Kante {
    if vorn { staerker(nb, w) } else { staerker(w, nb) }
}

/// Seite: 0 oben, 1 rechts, 2 unten, 3 links (Reihenfolge von `Rahmen`).
fn kante(doc: &Document, id: Option<NodeId>, seite: usize, rang: u8) -> Kante {
    let Some(n) = id.and_then(|i| doc.get(i)) else { return KEINE };
    let r = &n.style.rahmen;
    Kante { breite: r.breite[seite], stil: r.stil[seite], farbe: r.farbe[seite], rang }
}

#[derive(Clone, Copy)]
struct Zelle {
    id: NodeId,
    z: usize,
    s: usize,
    zs: usize,
    ss: usize,
}

/// Loest die Rahmen aller Tabellen mit `border-collapse: collapse` auf
/// (siehe Modulkopf).
pub(crate) fn aufloesen(doc: &mut Document, plan: &LayoutPlan) {
    let anzahl = doc.len();
    for i in 0..anzahl {
        let id = NodeId(i as u32);
        if matches!(rolle(doc, plan, id), R::Tabelle | R::InlineTabelle) && plan.tabelle(id).zusammen {
            tabelle(doc, plan, id);
        }
    }
}

fn sichtbar(plan: &LayoutPlan, id: NodeId) -> bool {
    plan.hole(id).is_none_or(|s| s.wird_gezeichnet())
}

fn tabelle(doc: &mut Document, plan: &LayoutPlan, tab: NodeId) {
    let Some(kinder) = doc.get(tab).map(|n| n.children.clone()) else { return };
    // Die Teile in Anzeigereihenfolge (wie `tabellen_leg::gitter_bauen`).
    let mut kopf = None;
    let mut fuss = None;
    let mut teile: Vec<(NodeId, bool)> = Vec::new();
    let mut spalten: Vec<(Option<NodeId>, Option<NodeId>)> = Vec::new();
    let mut entfernen: Vec<NodeId> = Vec::new();
    for k in kinder {
        if !sichtbar(plan, k) {
            continue;
        }
        match rolle(doc, plan, k) {
            R::Spaltengruppe => {
                entfernen.push(k);
                let cols: Vec<NodeId> = doc
                    .get(k)
                    .map(|n| n.children.iter().copied().filter(|c| rolle(doc, plan, *c) == R::Spalte).collect())
                    .unwrap_or_default();
                if cols.is_empty() {
                    for _ in 0..plan.tabelle(k).spann.max(1) {
                        spalten.push((None, Some(k)));
                    }
                } else {
                    for c in cols {
                        entfernen.push(c);
                        for _ in 0..plan.tabelle(c).spann.max(1) {
                            spalten.push((Some(c), Some(k)));
                        }
                    }
                }
            }
            R::Spalte => {
                entfernen.push(k);
                for _ in 0..plan.tabelle(k).spann.max(1) {
                    spalten.push((Some(k), None));
                }
            }
            R::Kopf if kopf.is_none() => kopf = Some(k),
            R::Fuss if fuss.is_none() => fuss = Some(k),
            R::Kopf | R::Fuss | R::Rumpf => teile.push((k, true)),
            R::Zeile => teile.push((k, false)),
            _ => {}
        }
    }
    let mut ordnung = Vec::new();
    if let Some(k) = kopf {
        ordnung.push((k, true));
    }
    ordnung.extend(teile);
    if let Some(f) = fuss {
        ordnung.push((f, true));
    }
    // Zeilen mit ihrer Gruppe.
    let mut zeilen: Vec<(NodeId, Option<NodeId>)> = Vec::new();
    for (k, gruppe) in ordnung {
        if gruppe {
            entfernen.push(k);
            let rs: Vec<NodeId> = doc
                .get(k)
                .map(|n| n.children.iter().copied().filter(|z| sichtbar(plan, *z) && rolle(doc, plan, *z) == R::Zeile).collect())
                .unwrap_or_default();
            for z in rs {
                zeilen.push((z, Some(k)));
            }
        } else {
            zeilen.push((k, None));
        }
    }
    zeilen.truncate(MAX_ZEILEN);
    let nz = zeilen.len();
    // Die Zellen ins Raster.
    let mut zellen: Vec<Zelle> = Vec::new();
    let mut belegt: Vec<Vec<Option<usize>>> = vec![Vec::new(); nz];
    let mut n_spalten = spalten.len().min(MAX_SPALTEN);
    for zi in 0..nz {
        let (zeile, gruppe) = zeilen[zi];
        entfernen.push(zeile);
        let ende = (zi..nz).find(|j| zeilen[*j].1 != gruppe).unwrap_or(nz);
        let cs: Vec<NodeId> = doc
            .get(zeile)
            .map(|n| n.children.iter().copied().filter(|c| sichtbar(plan, *c) && rolle(doc, plan, *c) == R::Zelle).collect())
            .unwrap_or_default();
        let mut sp = 0usize;
        for c in cs {
            while sp < MAX_SPALTEN && belegt[zi].get(sp).copied().flatten().is_some() {
                sp += 1;
            }
            if sp >= MAX_SPALTEN {
                break;
            }
            let (ss, zs) = plan.hole(c).map_or((1, 1), |s| (s.colspan.max(1) as usize, s.rowspan.max(1) as usize));
            let ss = ss.min(MAX_SPALTEN - sp);
            let zs = zs.min(ende.saturating_sub(zi)).max(1);
            let nr = zellen.len();
            for dz in 0..zs {
                let reihe = &mut belegt[zi + dz];
                if reihe.len() < sp + ss {
                    reihe.resize(sp + ss, None);
                }
                for ds in 0..ss {
                    reihe[sp + ds] = Some(nr);
                }
            }
            zellen.push(Zelle { id: c, z: zi, s: sp, zs, ss });
            sp += ss;
            n_spalten = n_spalten.max(sp);
        }
    }
    let zelle_bei = |z: usize, s: usize| -> Option<usize> { belegt.get(z).and_then(|r| r.get(s)).copied().flatten() };
    let spalte = |s: usize| spalten.get(s).copied().unwrap_or((None, None));
    // Die Gewinner je Zelle und Seite.
    let mut ergebnis: Vec<(NodeId, [Kante; 4])> = Vec::with_capacity(zellen.len());
    for c in zellen.iter() {
        let mut seiten = [KEINE; 4];
        // Oben (0) und unten (2).
        for (seite, gegen, zeile_hier, zeile_dort) in [
            (0usize, 2usize, Some(c.z), c.z.checked_sub(1)),
            (2usize, 0usize, Some(c.z + c.zs - 1), (c.z + c.zs < nz).then_some(c.z + c.zs)),
        ] {
            let mut w = kante(doc, Some(c.id), seite, 6);
            // Oben liegt der Nachbar VOR der Zelle: bei Gleichstand gewinnt er.
            let vorn = seite == 0;
            for s in c.s..c.s + c.ss {
                if let Some(nb) = zeile_dort.and_then(|z| zelle_bei(z, s))
                    && let Some(n) = zellen.get(nb)
                {
                    w = mit(w, kante(doc, Some(n.id), gegen, 6), vorn);
                }
            }
            let hier = zeile_hier.and_then(|z| zeilen.get(z)).copied();
            let dort = zeile_dort.and_then(|z| zeilen.get(z)).copied();
            if let Some((zr, gr)) = hier {
                w = staerker(w, kante(doc, Some(zr), seite, 5));
                if dort.is_none_or(|(_, g2)| g2 != gr) {
                    w = staerker(w, kante(doc, gr, seite, 4));
                }
            }
            if let Some((zr, gr)) = dort {
                w = mit(w, kante(doc, Some(zr), gegen, 5), vorn);
                if hier.is_none_or(|(_, g1)| g1 != gr) {
                    w = mit(w, kante(doc, gr, gegen, 4), vorn);
                }
            }
            if dort.is_none() {
                for s in c.s..c.s + c.ss {
                    let (col, grp) = spalte(s);
                    w = staerker(w, kante(doc, col, seite, 3));
                    w = staerker(w, kante(doc, grp, seite, 2));
                }
                w = staerker(w, kante(doc, Some(tab), seite, 1));
            }
            seiten[seite] = w;
        }
        // Links (3) und rechts (1).
        for (seite, gegen, s_hier, s_dort) in [
            (3usize, 1usize, c.s, c.s.checked_sub(1)),
            (1usize, 3usize, c.s + c.ss - 1, (c.s + c.ss < n_spalten).then_some(c.s + c.ss)),
        ] {
            let mut w = kante(doc, Some(c.id), seite, 6);
            // Links liegt der Nachbar VOR der Zelle: bei Gleichstand gewinnt er.
            let vorn = seite == 3;
            for z in c.z..c.z + c.zs {
                if let Some(nb) = s_dort.and_then(|s| zelle_bei(z, s))
                    && let Some(n) = zellen.get(nb)
                {
                    w = mit(w, kante(doc, Some(n.id), gegen, 6), vorn);
                }
            }
            let (col_h, grp_h) = spalte(s_hier);
            w = staerker(w, kante(doc, col_h, seite, 3));
            if let Some(sd) = s_dort {
                let (col_d, grp_d) = spalte(sd);
                w = mit(w, kante(doc, col_d, gegen, 3), vorn);
                if grp_h != grp_d {
                    w = staerker(w, kante(doc, grp_h, seite, 2));
                    w = mit(w, kante(doc, grp_d, gegen, 2), vorn);
                }
            } else {
                w = staerker(w, kante(doc, grp_h, seite, 2));
                for z in c.z..c.z + c.zs {
                    if let Some((zr, gr)) = zeilen.get(z).copied() {
                        w = staerker(w, kante(doc, Some(zr), seite, 5));
                        w = staerker(w, kante(doc, gr, seite, 4));
                    }
                }
                w = staerker(w, kante(doc, Some(tab), seite, 1));
            }
            seiten[seite] = w;
        }
        ergebnis.push((c.id, seiten));
    }
    // Die aeusseren Rahmen der Tabelle: die Haelfte des breitesten
    // aufgeloesten Zellrahmens je Seite (§17.6.2).
    let mut aussen = [0.0f32; 4];
    for (c, (_, seiten)) in zellen.iter().zip(ergebnis.iter()) {
        if c.z == 0 {
            aussen[0] = aussen[0].max(wirksam(&seiten[0]));
        }
        if c.z + c.zs >= nz {
            aussen[2] = aussen[2].max(wirksam(&seiten[2]));
        }
        if c.s == 0 {
            aussen[3] = aussen[3].max(wirksam(&seiten[3]));
        }
        if c.s + c.ss >= n_spalten {
            aussen[1] = aussen[1].max(wirksam(&seiten[1]));
        }
    }
    // Ohne Zellen: der Rahmen der Tabelle selbst.
    if zellen.is_empty()
        && let Some(n) = doc.get(tab)
    {
        for i in 0..4 {
            aussen[i] = n.style.rahmen.gezeichnete_breite(i);
        }
    }
    // Schreiben.
    for (id, seiten) in ergebnis {
        if let Ok(n) = doc.node(id) {
            let mut st = n.style.clone();
            let mut r = Rahmen::default();
            for i in 0..4 {
                let k = seiten[i];
                if k.stil != RahmenStil::Keiner && k.breite > 0.0 {
                    r.breite[i] = k.breite;
                    r.stil[i] = k.stil;
                    r.farbe[i] = k.farbe;
                }
            }
            st.rahmen = r;
            let _ = doc.set_style(id, st);
        }
    }
    for id in entfernen {
        if let Ok(n) = doc.node(id) {
            let mut st = n.style.clone();
            st.rahmen = Rahmen::default();
            let _ = doc.set_style(id, st);
        }
    }
    if let Ok(n) = doc.node(tab) {
        let mut st = n.style.clone();
        let mut r = Rahmen::default();
        for i in 0..4 {
            if aussen[i] > 0.0 {
                r.breite[i] = aussen[i] / 2.0;
                r.stil[i] = RahmenStil::Durchgezogen;
                r.farbe[i] = Color::TRANSPARENT;
            }
        }
        st.rahmen = r;
        st.padding = narsil_core::length::Spacing::ZERO;
        let _ = doc.set_style(tab, st);
    }
}
