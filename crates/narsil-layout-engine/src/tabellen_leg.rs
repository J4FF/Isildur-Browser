//! Release 11.10.2026 (Agent CSS2): DAS TABELLEN-LAYOUT nach CSS 2.1
//! Kapitel 17 — Raster, Spaltenbreiten (automatisch §17.5.2.2 und fest
//! §17.5.2.1), Zeilenhoehen (§17.5.3), senkrechte Ausrichtung der Zellen,
//! Beschriftungen (§17.4) und `border-spacing` (§17.6.1).
//!
//! # DER WEG
//!
//! Die Messphase (`box.rs`) misst die Kinder einer Tabelle einmal mit der
//! ganzen Breite (fuer die Inhaltsbreiten), ruft dann [`spalten_phase`]:
//! Raster bauen, Spaltenbreiten verteilen. Danach misst sie jede Zelle mit
//! IHRER Breite neu und ruft [`zeilen_phase`]: Zeilenhoehen, Lage aller
//! Gruppen, Zeilen, Zellen und Beschriftungen. Die Lage steht in `Sizes`
//! (relativ zur Tabelle), die Platzphase (`flow.rs`) setzt sie nur noch.
//!
//! GEMESSEN vorher (WPT css/CSS2/tables, 374 Dateien, 111 gruen): die
//! Spalten bekamen `border-spacing` nur zwischen den Zellen (nicht am Rand),
//! Kopf- und Fussgruppen standen in Dokumentreihenfolge, eine Zelle wurde
//! mit der ganzen Tabellenbreite gemessen (falsche Hoehe), `vertical-align`
//! und Beschriftungen gab es nicht, `table-layout: fixed` ebenso nicht.
//!
//! Die alte Rechnung (`tabelle.rs`, `lege_tabelle`) bleibt als oeffentliche
//! Schnittstelle bestehen; das Layout benutzt sie nicht mehr.

use std::collections::HashMap;

use narsil_core::geometry::{Edges, Rect, Size};
use narsil_core::length::Length;
use narsil_dom::{NodeId, NodeKind};
use narsil_layout::plan::{Anzeige, TabellenRolle as R, Vertikal};

use crate::Ctx;
use crate::r#box::{Sizes, rahmen_edges, resolve_spacing};

/// Hoechste Spaltenzahl (M3/M4; HTML erlaubt `colspan` bis 1000).
const MAX_SPALTEN: usize = 1000;
/// Hoechste Zeilenzahl eines Rasters (M3/M4, wie `tabelle.rs`).
const MAX_ZEILEN: usize = 20_000;
/// Hoechste Zellenzahl (Belegungsraster `MAX_SPALTEN x MAX_ZEILEN` waere zu
/// gross; die Belegung waechst mit den Zellen).
const MAX_ZELLEN: usize = 400_000;

fn nn(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}

/// Die genaue Rolle — aus der Nebentabelle, sonst aus `Anzeige` und Struktur
/// (dieselbe Regel wie `narsil_page::tabelle_anonym::rolle`).
pub(crate) fn rolle(ctx: &Ctx<'_>, id: NodeId) -> R {
    let Some(plan) = ctx.plan else { return R::Keine };
    let r = plan.tabelle(id).rolle;
    if r != R::Keine {
        return r;
    }
    let anzeige = |k: NodeId| plan.hole(k).map(|s| s.anzeige);
    match anzeige(id) {
        Some(Anzeige::Tabelle) => R::Tabelle,
        Some(Anzeige::TabelleZelle) => R::Zelle,
        Some(Anzeige::TabelleZeile) => {
            let kinder = ctx.document.get(id).map(|n| n.children.as_slice()).unwrap_or(&[]);
            if kinder.iter().any(|k| anzeige(*k) == Some(Anzeige::TabelleZelle))
                || !kinder.iter().any(|k| anzeige(*k) == Some(Anzeige::TabelleZeile))
            {
                R::Zeile
            } else {
                R::Rumpf
            }
        }
        Some(Anzeige::TabelleRand) => {
            if ctx.document.get(id).is_some_and(|n| n.children.is_empty()) {
                R::Spalte
            } else {
                R::Beschriftung
            }
        }
        _ => R::Keine,
    }
}

/// Ist `id` eine Tabelle (fuer das neue Layout)?
pub(crate) fn ist_tabelle(ctx: &Ctx<'_>, id: NodeId) -> bool {
    ctx.stil(id).is_some_and(|s| s.anzeige == Anzeige::Tabelle)
}

/// Nimmt der Knoten am Raster teil (gezeichnet, im Fluss)?
fn im_fluss(ctx: &Ctx<'_>, id: NodeId) -> bool {
    let s = ctx.stil_oder_standard(id);
    s.wird_gezeichnet() && !matches!(s.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)
}

/// Eine Breiten- oder Hoehenangabe einer Spalte/Zelle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Mass {
    Auto,
    Px(f32),
    Prozent(f32),
}

/// Eine Zelle im Raster.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GZelle {
    pub knoten: NodeId,
    pub zeile: usize,
    pub spalte: usize,
    pub zeilen: usize,
    pub spalten: usize,
}

/// Eine Zeile im Raster (in Anzeigereihenfolge).
#[derive(Debug, Clone, Copy)]
pub(crate) struct GZeile {
    pub knoten: NodeId,
    pub gruppe: Option<usize>,
}

/// Eine Zeilengruppe: ihre Zeilen `von..bis`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GGruppe {
    pub knoten: NodeId,
    pub von: usize,
    pub bis: usize,
}

/// Das Raster einer Tabelle (CSS 2.1 §17.5).
#[derive(Debug, Clone, Default)]
pub(crate) struct Gitter {
    pub zeilen: Vec<GZeile>,
    pub zellen: Vec<GZelle>,
    pub gruppen: Vec<GGruppe>,
    pub n_spalten: usize,
    /// Breitenangabe je Spalte aus `table-column`/`<col>`.
    pub spalten_mass: Vec<Mass>,
    /// Spalten- und Spaltengruppenkaesten mit ihrem Spaltenbereich.
    pub spalten_knoten: Vec<(NodeId, usize, usize)>,
    /// Beschriftungen (Knoten, unten?).
    pub beschriftungen: Vec<(NodeId, bool)>,
}

fn mass_von(l: Length) -> Mass {
    match l {
        Length::Px(v) if v.is_finite() && v >= 0.0 => Mass::Px(v),
        Length::Percent(p) if p.is_finite() && p >= 0.0 => Mass::Prozent(p),
        _ => Mass::Auto,
    }
}

/// Baut das Raster: Kopfgruppe zuerst, Fussgruppe zuletzt (CSS 2.1
/// §17.2: „table-header-group … always displayed before all other rows“),
/// Zellen nach `colspan`/`rowspan` in die freien Felder.
pub(crate) fn gitter_bauen(ctx: &Ctx<'_>, tabelle: NodeId) -> Gitter {
    let mut g = Gitter::default();
    let Some(t) = ctx.document.get(tabelle) else { return g };
    let plan = ctx.plan;
    let mut kopf: Option<NodeId> = None;
    let mut fuss: Option<NodeId> = None;
    // (Knoten, ist Gruppe)
    let mut teile: Vec<(NodeId, bool)> = Vec::new();
    let mut spalte = 0usize;
    for &k in t.children.iter() {
        if !im_fluss(ctx, k) {
            continue;
        }
        let r = rolle(ctx, k);
        match r {
            R::Beschriftung => {
                g.beschriftungen.push((k, plan.is_some_and(|p| p.tabelle(k).unten)));
            }
            R::Spalte | R::Spaltengruppe => {
                let start = spalte;
                let kinder: Vec<NodeId> = if r == R::Spaltengruppe {
                    ctx.document
                        .get(k)
                        .map(|n| n.children.iter().copied().filter(|c| rolle(ctx, *c) == R::Spalte).collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let gruppen_mass = ctx.document.get(k).map_or(Mass::Auto, |n| mass_von(n.style.size.width));
                if kinder.is_empty() {
                    let n = plan.map_or(1, |p| p.tabelle(k).spann.max(1) as usize).min(MAX_SPALTEN);
                    for _ in 0..n {
                        if g.spalten_mass.len() < MAX_SPALTEN {
                            g.spalten_mass.push(gruppen_mass);
                        }
                    }
                    spalte = (spalte + n).min(MAX_SPALTEN);
                } else {
                    for c in kinder {
                        let n = plan.map_or(1, |p| p.tabelle(c).spann.max(1) as usize).min(MAX_SPALTEN);
                        let m = ctx.document.get(c).map_or(Mass::Auto, |n| mass_von(n.style.size.width));
                        let m = if m == Mass::Auto { gruppen_mass } else { m };
                        let s0 = spalte;
                        for _ in 0..n {
                            if g.spalten_mass.len() < MAX_SPALTEN {
                                g.spalten_mass.push(m);
                            }
                        }
                        spalte = (spalte + n).min(MAX_SPALTEN);
                        g.spalten_knoten.push((c, s0, spalte));
                    }
                }
                g.spalten_knoten.push((k, start, spalte));
            }
            R::Kopf if kopf.is_none() => kopf = Some(k),
            R::Fuss if fuss.is_none() => fuss = Some(k),
            R::Kopf | R::Fuss | R::Rumpf => teile.push((k, true)),
            R::Zeile => teile.push((k, false)),
            _ => {}
        }
    }
    let mut ordnung: Vec<(NodeId, bool)> = Vec::with_capacity(teile.len() + 2);
    if let Some(k) = kopf {
        ordnung.push((k, true));
    }
    ordnung.extend(teile);
    if let Some(f) = fuss {
        ordnung.push((f, true));
    }
    for (k, gruppe) in ordnung {
        if gruppe {
            let von = g.zeilen.len();
            let gi = g.gruppen.len();
            if let Some(n) = ctx.document.get(k) {
                for &z in n.children.iter() {
                    if g.zeilen.len() >= MAX_ZEILEN {
                        break;
                    }
                    if im_fluss(ctx, z) && rolle(ctx, z) == R::Zeile {
                        g.zeilen.push(GZeile { knoten: z, gruppe: Some(gi) });
                    }
                }
            }
            g.gruppen.push(GGruppe { knoten: k, von, bis: g.zeilen.len() });
        } else if g.zeilen.len() < MAX_ZEILEN {
            g.zeilen.push(GZeile { knoten: k, gruppe: None });
        }
    }
    // Die Zellen in die freien Felder (HTML §4.9.12, „forming a table“).
    let nz = g.zeilen.len();
    let mut belegt: Vec<Vec<bool>> = vec![Vec::new(); nz];
    let mut n_spalten = g.spalten_mass.len();
    for zi in 0..nz {
        let zeile = g.zeilen[zi];
        let ende_gruppe = zeile.gruppe.and_then(|gi| g.gruppen.get(gi)).map_or(nz, |gr| gr.bis);
        let Some(n) = ctx.document.get(zeile.knoten) else { continue };
        let mut sp = 0usize;
        for &c in n.children.iter() {
            if g.zellen.len() >= MAX_ZELLEN {
                break;
            }
            if !im_fluss(ctx, c) || rolle(ctx, c) != R::Zelle {
                continue;
            }
            while sp < MAX_SPALTEN && belegt[zi].get(sp).copied().unwrap_or(false) {
                sp += 1;
            }
            if sp >= MAX_SPALTEN {
                break;
            }
            let (cs, rs) = ctx.stil(c).map_or((1, 1), |s| (s.colspan.max(1) as usize, s.rowspan.max(1) as usize));
            let cs = cs.min(MAX_SPALTEN - sp);
            let rs = rs.min(ende_gruppe.saturating_sub(zi)).max(1);
            for dz in 0..rs {
                let reihe = &mut belegt[zi + dz];
                if reihe.len() < sp + cs {
                    reihe.resize(sp + cs, false);
                }
                for ds in 0..cs {
                    reihe[sp + ds] = true;
                }
            }
            g.zellen.push(GZelle { knoten: c, zeile: zi, spalte: sp, zeilen: rs, spalten: cs });
            sp += cs;
            n_spalten = n_spalten.max(sp);
        }
    }
    g.n_spalten = n_spalten.min(MAX_SPALTEN);
    g
}

/// Rahmen + Innenabstand eines Kastens (je Seite).
fn innen_rand(ctx: &Ctx<'_>, id: NodeId, basis: f32) -> Edges {
    let Some(n) = ctx.document.get(id) else { return Edges::ZERO };
    let p = resolve_spacing(n.style.padding, basis, id, "padding").unwrap_or(Edges::ZERO);
    let r = rahmen_edges(&n.style.rahmen);
    Edges { top: p.top + r.top, right: p.right + r.right, bottom: p.bottom + r.bottom, left: p.left + r.left }
}

/// `border-spacing` (waagerecht, senkrecht) — im Modell `collapse` 0.
fn abstand(ctx: &Ctx<'_>, tabelle: NodeId) -> (f32, f32) {
    if ctx.plan.is_some_and(|p| p.tabelle(tabelle).zusammen) {
        return (0.0, 0.0);
    }
    ctx.stil(tabelle).map_or((crate::flow::BORDER_SPACING_UA, crate::flow::BORDER_SPACING_UA), |s| (nn(s.luecke), nn(s.luecke_quer)))
}

/// Min-/Max-Breite und Angabe je Spalte (CSS 2.1 §17.5.2.2, Schritte 1-3).
struct SpaltenMasse {
    min: Vec<f32>,
    max: Vec<f32>,
    art: Vec<Mass>,
}

fn zellen_mass(ctx: &Ctx<'_>, id: NodeId, basis: f32) -> (Mass, f32) {
    let Some(n) = ctx.document.get(id) else { return (Mass::Auto, 0.0) };
    let rand = innen_rand(ctx, id, basis);
    let quer = rand.left + rand.right;
    let border_box = ctx.stil(id).is_some_and(|s| s.border_box);
    match mass_von(n.style.size.width) {
        Mass::Px(w) => (Mass::Px(if border_box { w.max(quer) } else { w + quer }), quer),
        m => (m, quer),
    }
}

/// Release 11.10.2026: im Modell `collapse` (CSS 2.1 §17.6.2) die HALBEN
/// Rahmen jeder Zelle (oben, rechts, unten, links) — so weit ragt die Zelle
/// ueber ihre Rasterlinien; sonst 0.
fn halbe_rahmen(ctx: &Ctx<'_>, tabelle: NodeId, g: &Gitter) -> Vec<[f32; 4]> {
    if !ctx.plan.is_some_and(|p| p.tabelle(tabelle).zusammen) {
        return vec![[0.0; 4]; g.zellen.len()];
    }
    g.zellen
        .iter()
        .map(|z| {
            let r = ctx.document.get(z.knoten).map_or(Edges::ZERO, |n| rahmen_edges(&n.style.rahmen));
            [r.top / 2.0, r.right / 2.0, r.bottom / 2.0, r.left / 2.0]
        })
        .collect()
}

fn spalten_masse(ctx: &Ctx<'_>, sizes: &Sizes, g: &Gitter, h: f32, tiefe: usize, halb: &[[f32; 4]]) -> SpaltenMasse {
    let n = g.n_spalten;
    let mut m = SpaltenMasse { min: vec![0.0; n], max: vec![0.0; n], art: vec![Mass::Auto; n] };
    // Spaltenangaben aus `<col>`.
    for (i, a) in g.spalten_mass.iter().enumerate().take(n) {
        if let Mass::Px(w) = a {
            m.art[i] = Mass::Px(*w);
            m.max[i] = m.max[i].max(*w);
        } else if let Mass::Prozent(p) = a {
            m.art[i] = Mass::Prozent(*p);
        }
    }
    let mut spannend: Vec<(GZelle, f32, f32, Mass)> = Vec::new();
    for (zi, z) in g.zellen.iter().enumerate() {
        // Im Modell `collapse` liegt die Haelfte jedes Seitenrahmens jenseits
        // der Rasterlinie (CSS 2.1 §17.6.2).
        let ueber = halb.get(zi).map_or(0.0, |b| b[1] + b[3]);
        let cmin = nn(sizes.min_inhalt_breite(ctx, z.knoten, tiefe) - ueber);
        let cmax = nn(sizes.max_content_breite(ctx, z.knoten, tiefe) - ueber).max(cmin);
        let (art, _) = zellen_mass(ctx, z.knoten, 0.0);
        let art = match art {
            Mass::Px(w) => Mass::Px(nn(w - ueber)),
            a => a,
        };
        let (cmin, cmax) = match art {
            // Feste Zellbreite: Mindestbreite, und mehr will die Zelle nicht.
            Mass::Px(w) => (cmin.max(w), cmin.max(w)),
            _ => (cmin, cmax),
        };
        if z.spalten == 1 {
            let i = z.spalte;
            if i >= n {
                continue;
            }
            m.min[i] = m.min[i].max(cmin);
            m.max[i] = m.max[i].max(cmax);
            m.art[i] = match (m.art[i], art) {
                (Mass::Prozent(a), Mass::Prozent(b)) => Mass::Prozent(a.max(b)),
                (Mass::Prozent(a), _) => Mass::Prozent(a),
                (_, Mass::Prozent(b)) => Mass::Prozent(b),
                (Mass::Px(a), Mass::Px(b)) => Mass::Px(a.max(b)),
                (Mass::Px(a), _) => Mass::Px(a),
                (_, Mass::Px(b)) => Mass::Px(b),
                _ => Mass::Auto,
            };
        } else {
            spannend.push((*z, cmin, cmax, art));
        }
    }
    // Feste Spalten wollen nicht mehr als ihre Angabe (aber mindestens ihr Minimum).
    for i in 0..n {
        if let Mass::Px(w) = m.art[i] {
            m.max[i] = w.max(m.min[i]);
        }
        m.max[i] = m.max[i].max(m.min[i]);
    }
    // Schritt 3: spannende Zellen, die kuerzeren zuerst. Der Ueberschuss geht
    // anteilig nach der Max-Breite an die ueberspannten Spalten (gleich,
    // wenn alle 0).
    spannend.sort_by_key(|(z, ..)| z.spalten);
    for (z, cmin, cmax, _) in spannend {
        let von = z.spalte.min(n);
        let bis = (z.spalte + z.spalten).min(n);
        if bis <= von {
            continue;
        }
        let innen = h * (bis - von - 1) as f32;
        let verteilen = |werte: &mut Vec<f32>, gewicht: &Vec<f32>, ziel: f32| {
            let summe: f32 = werte[von..bis].iter().sum::<f32>() + innen;
            if ziel <= summe {
                return;
            }
            let fehlt = ziel - summe;
            let gsum: f32 = gewicht[von..bis].iter().sum();
            let k = (bis - von) as f32;
            for i in von..bis {
                let anteil = if gsum > 0.0 { gewicht[i] / gsum } else { 1.0 / k };
                werte[i] += fehlt * anteil;
            }
        };
        let gew = m.max.clone();
        verteilen(&mut m.min, &gew, cmin);
        let gew = m.max.clone();
        verteilen(&mut m.max, &gew, cmax);
        for i in von..bis {
            m.max[i] = m.max[i].max(m.min[i]);
        }
    }
    m
}

/// (geringste, bevorzugte) Rahmenbreite der Tabelle — fuer die
/// Inhaltsbreiten ihrer Vorfahren (Schrumpfbreite, Floats, Zellen).
pub(crate) fn tabellen_inhaltsbreiten(ctx: &Ctx<'_>, sizes: &Sizes, tabelle: NodeId, tiefe: usize) -> (f32, f32) {
    let g = gitter_bauen(ctx, tabelle);
    let (h, _) = abstand(ctx, tabelle);
    let halb = halbe_rahmen(ctx, tabelle, &g);
    let m = spalten_masse(ctx, sizes, &g, h, tiefe + 1, &halb);
    let rand = innen_rand(ctx, tabelle, 0.0);
    let luecken = if g.n_spalten > 0 { h * (g.n_spalten + 1) as f32 } else { 0.0 };
    let pb = rand.left + rand.right;
    let capmin = beschriftung_min(ctx, sizes, &g, tiefe + 1);
    let min = (m.min.iter().sum::<f32>() + luecken + pb).max(capmin);
    let max = (max_mit_prozent(&m) + luecken + pb).max(capmin);
    let fest = ctx.document.get(tabelle).map_or(Mass::Auto, |n| mass_von(n.style.size.width));
    let border_box = ctx.stil(tabelle).is_some_and(|s| s.border_box);
    match fest {
        Mass::Px(w) => {
            let w = if border_box { w } else { w + pb };
            let w = w.max(min);
            (w, w)
        }
        _ => (min, max.max(min)),
    }
}

/// Die bevorzugte Summe der Spalten mit Prozentspalten (die Tabelle muss so
/// breit sein, dass jede Prozentspalte ihren Anteil bekommt).
fn max_mit_prozent(m: &SpaltenMasse) -> f32 {
    let summe: f32 = m.max.iter().sum();
    let mut proz = 0.0f32;
    let mut rest = 0.0f32;
    let mut noetig = summe;
    for i in 0..m.max.len() {
        match m.art[i] {
            Mass::Prozent(p) if p > 0.0 => {
                proz += p;
                noetig = noetig.max(m.max[i] * 100.0 / p);
            }
            _ => rest += m.max[i],
        }
    }
    if proz > 0.0 && proz < 100.0 {
        noetig = noetig.max(rest * 100.0 / (100.0 - proz));
    }
    noetig
}

/// Verteilt `innen` (Rahmenbreite der Tabelle ohne Rand und Abstaende) auf die
/// Spalten — automatisches Layout (CSS 2.1 §17.5.2.2, verteilt wie die
/// Browser: erst die Minima, dann Prozent-, feste und automatische Spalten
/// bis zu ihrem Wunsch, der Rest an die automatischen nach ihrer Max-Breite).
fn verteilen_auto(m: &SpaltenMasse, innen: f32) -> Vec<f32> {
    let n = m.min.len();
    let mut w: Vec<f32> = m.min.clone();
    let mut rest = innen - w.iter().sum::<f32>();
    if rest <= 0.0 || n == 0 {
        return w;
    }
    // 1. Prozentspalten bis zu ihrem Anteil.
    for i in 0..n {
        if let Mass::Prozent(p) = m.art[i] {
            let ziel = (innen * p / 100.0).max(m.min[i]);
            let mehr = (ziel - w[i]).clamp(0.0, rest);
            w[i] += mehr;
            rest -= mehr;
        }
    }
    // 2. Feste Spalten bis zu ihrer Angabe.
    for i in 0..n {
        if let Mass::Px(f) = m.art[i] {
            let mehr = (f.max(m.min[i]) - w[i]).clamp(0.0, rest);
            w[i] += mehr;
            rest -= mehr;
        }
    }
    // 3. Automatische Spalten bis zu ihrer Max-Breite, anteilig am Weg.
    let weg: Vec<f32> = (0..n).map(|i| if m.art[i] == Mass::Auto { (m.max[i] - w[i]).max(0.0) } else { 0.0 }).collect();
    let weg_summe: f32 = weg.iter().sum();
    if weg_summe > 0.0 {
        let teil = rest.min(weg_summe);
        for i in 0..n {
            w[i] += teil * weg[i] / weg_summe;
        }
        rest -= teil;
    }
    if rest <= 0.0 {
        return w;
    }
    // 4. Der Rest: automatische Spalten nach Max-Breite (gleich, wenn alle
    // 0); ohne solche die festen nach Breite, sonst die Prozentspalten.
    let auto: Vec<usize> = (0..n).filter(|i| m.art[*i] == Mass::Auto).collect();
    let feste: Vec<usize> = (0..n).filter(|i| matches!(m.art[*i], Mass::Px(_))).collect();
    let gruppe = if !auto.is_empty() {
        auto
    } else if !feste.is_empty() {
        feste
    } else {
        (0..n).collect()
    };
    let gsum: f32 = gruppe.iter().map(|i| w[*i]).sum();
    let k = gruppe.len() as f32;
    for i in gruppe.iter().copied() {
        let anteil = if gsum > 0.0 && !gruppe.iter().all(|j| m.art[*j] == Mass::Auto) {
            w[i] / gsum
        } else {
            let msum: f32 = gruppe.iter().map(|j| m.max[*j]).sum();
            if msum > 0.0 { m.max[i] / msum } else { 1.0 / k }
        };
        w[i] += rest * anteil;
    }
    w
}

/// Festes Layout (CSS 2.1 §17.5.2.1): Breiten aus den Spalten und der
/// ERSTEN Zeile, der Rest gleich auf die uebrigen Spalten; der Inhalt zaehlt
/// nicht.
fn verteilen_fest(ctx: &Ctx<'_>, g: &Gitter, innen: f32, halb: &[[f32; 4]]) -> Vec<f32> {
    let n = g.n_spalten;
    let mut w: Vec<Option<f32>> = vec![None; n];
    for (i, a) in g.spalten_mass.iter().enumerate().take(n) {
        match a {
            Mass::Px(v) => w[i] = Some(*v),
            Mass::Prozent(p) => w[i] = Some(innen * p / 100.0),
            Mass::Auto => {}
        }
    }
    for (zi, z) in g.zellen.iter().enumerate().filter(|(_, z)| z.zeile == 0) {
        let (art, quer) = zellen_mass(ctx, z.knoten, innen);
        // Modell `collapse`: die halben Seitenrahmen liegen jenseits der
        // Rasterlinien (GEMESSEN: WPT `fixed-table-layout-003e05`).
        let ueber = halb.get(zi).map_or(0.0, |b| b[1] + b[3]);
        let gesamt = match art {
            Mass::Px(v) => Some(v - ueber),
            Mass::Prozent(p) => Some(innen * p / 100.0 + quer - ueber),
            Mass::Auto => None,
        };
        if let Some(gesamt) = gesamt {
            let k = z.spalten.max(1) as f32;
            for i in z.spalte..(z.spalte + z.spalten).min(n) {
                if w[i].is_none() {
                    w[i] = Some(gesamt / k);
                }
            }
        }
    }
    let fest: f32 = w.iter().flatten().sum();
    let offen = w.iter().filter(|x| x.is_none()).count();
    let rest = (innen - fest).max(0.0);
    let mut raus: Vec<f32> = w.iter().map(|x| x.unwrap_or(if offen > 0 { rest / offen as f32 } else { 0.0 })).collect();
    let summe: f32 = raus.iter().sum();
    if offen == 0 && summe < innen && n > 0 {
        // Breiter als die Spalten: der Ueberschuss anteilig.
        let extra = innen - summe;
        for x in raus.iter_mut() {
            *x += if summe > 0.0 { extra * *x / summe } else { extra / n as f32 };
        }
    }
    raus
}

/// Das Ergebnis der Spaltenphase — der Auftrag fuer die Zeilenphase.
#[derive(Debug, Clone)]
pub(crate) struct Zwischen {
    pub gitter: Gitter,
    pub spalten: Vec<f32>,
    /// Rahmenbreite der Tabelle (Raster).
    pub breite: f32,
    /// Rahmen + Innenabstand der Tabelle.
    pub rand: Edges,
    pub h: f32,
    pub v: f32,
    /// Angegebene Rahmenhoehe der Tabelle.
    pub hoehe: Option<f32>,
    /// `direction: rtl` der Tabelle: die erste Spalte steht rechts (CSS 2.1
    /// §17.5).
    pub rtl: bool,
    /// Halbe Rahmen je Zelle (Modell `collapse`, sonst 0) — siehe
    /// `halbe_rahmen`.
    pub halb: Vec<[f32; 4]>,
}

impl Zwischen {
    /// Die Rahmenbreite einer Zelle.
    pub(crate) fn zellen_breite(&self, i: usize) -> f32 {
        let Some(z) = self.gitter.zellen.get(i) else { return 0.0 };
        let bis = (z.spalte + z.spalten).min(self.spalten.len());
        let von = z.spalte.min(bis);
        let halb = self.halb.get(i).map_or(0.0, |b| b[1] + b[3]);
        self.spalten[von..bis].iter().sum::<f32>() + self.h * (bis - von).saturating_sub(1) as f32 + halb
    }
}

/// Die Spaltenphase: Raster, Breite der Tabelle, Spaltenbreiten.
///
/// `angabe`: angegebene Rahmenbreite (schon gegen den Block aufgeloest), `verfuegbar`:
/// Platz fuer den Rahmen der Tabelle, `rand`: Rahmen + Innenabstand.
pub(crate) fn spalten_phase(
    ctx: &Ctx<'_>,
    sizes: &Sizes,
    tabelle: NodeId,
    angabe: Option<f32>,
    verfuegbar: f32,
    rand: Edges,
    hoehe: Option<f32>,
    min_max: (Option<f32>, Option<f32>),
) -> Zwischen {
    let g = gitter_bauen(ctx, tabelle);
    let halb = halbe_rahmen(ctx, tabelle, &g);
    let (h, v) = abstand(ctx, tabelle);
    let n = g.n_spalten;
    let luecken = if n > 0 { h * (n + 1) as f32 } else { 0.0 };
    let pb = rand.left + rand.right;
    let fest = ctx.plan.is_some_and(|p| p.tabelle(tabelle).fest) && angabe.is_some();
    let klemme = |w: f32| {
        let (lo, hi) = min_max;
        let w = hi.map_or(w, |m| w.min(m));
        lo.map_or(w, |m| w.max(m))
    };
    let (breite, spalten) = if fest {
        let b = klemme(angabe.unwrap_or(0.0)).max(pb + luecken);
        let s = verteilen_fest(ctx, &g, nn(b - pb - luecken), &halb);
        let summe: f32 = s.iter().sum();
        (b.max(summe + pb + luecken), s)
    } else {
        let m = spalten_masse(ctx, sizes, &g, h, 0, &halb);
        let min = m.min.iter().sum::<f32>() + luecken + pb;
        let max = max_mit_prozent(&m) + luecken + pb;
        let b = match angabe {
            Some(a) => klemme(a).max(min),
            None => klemme(max.min(verfuegbar.max(min))).max(min),
        };
        let s = verteilen_auto(&m, nn(b - pb - luecken));
        if std::env::var_os("NARSIL_DBG_TABELLE").is_some() {
            eprintln!("TABELLE {} min={:?} max={:?} art={:?} b={b} pb={pb} luecken={luecken} verfuegbar={verfuegbar} angabe={angabe:?} halb={halb:?} -> {s:?}", tabelle.0, m.min, m.max, m.art);
        }
        (b, s)
    };
    // CSS 2.1 §17.5.2.2: die Tabelle ist mindestens so breit wie die
    // breiteste Beschriftung (CAPMIN); der Ueberschuss geht an die Spalten.
    // GEMESSEN vorher (WPT `vertical-align-baseline-009`): eine Tabelle nur
    // mit Beschriftung war 0 px breit, die Beschriftung zentrierte ins Leere.
    let capmin = beschriftung_min(ctx, sizes, &g, 0);
    let (breite, spalten) = if capmin > breite {
        let s = if fest {
            verteilen_fest(ctx, &g, nn(capmin - pb - luecken), &halb)
        } else {
            verteilen_auto(&spalten_masse(ctx, sizes, &g, h, 0, &halb), nn(capmin - pb - luecken))
        };
        (capmin, s)
    } else {
        (breite, spalten)
    };
    let rtl = ctx.plan.is_some_and(|p| p.richtung(tabelle) == narsil_layout::Richtung::Rtl);
    Zwischen { gitter: g, spalten, breite, rand, h, v, hoehe, rtl, halb }
}

/// Die groesste Mindestbreite der Beschriftungen samt ihren Raendern.
fn beschriftung_min(ctx: &Ctx<'_>, sizes: &Sizes, g: &Gitter, tiefe: usize) -> f32 {
    g.beschriftungen
        .iter()
        .map(|(k, _)| {
            let m = ctx
                .document
                .get(*k)
                .and_then(|n| resolve_spacing(n.style.margin, 0.0, *k, "margin").ok())
                .unwrap_or(Edges::ZERO);
            nn(sizes.min_inhalt_breite(ctx, *k, tiefe)) + m.left + m.right
        })
        .fold(0.0, f32::max)
}

/// Die Lage einer gelegten Tabelle, relativ zur oberen linken Ecke ihres
/// Kastens (inklusive der Beschriftungen oben).
#[derive(Debug, Clone, Default)]
pub(crate) struct TabellenLage {
    /// Wie weit der Rahmen des Rasters unter der Kastenoberkante beginnt
    /// (die Beschriftungen oben).
    pub raster_oben: f32,
    /// Rahmenhoehe des Rasters.
    pub raster_hoehe: f32,
    /// Rechteck je Gruppe, Zeile, Zelle, Beschriftung, Spalte (relativ).
    pub plaetze: Vec<(NodeId, Rect)>,
}

/// Abstand der ersten Grundlinie von der Rahmenoberkante eines Kastens
/// (CSS 2.1 §17.5.3: die Grundlinie einer Zelle ist die ihrer ersten
/// Zeilenbox) — `None` ohne Zeile.
fn erste_grundlinie(ctx: &Ctx<'_>, sizes: &Sizes, id: NodeId, tiefe: usize) -> Option<f32> {
    if tiefe > 12 {
        return None;
    }
    let n = ctx.document.get(id)?;
    let oben = innen_rand(ctx, id, 0.0).top;
    if n.style.direction == narsil_core::style::Direction::Row && crate::r#box::ist_zeilenbox(ctx, id, n) {
        let h = sizes.get(id).map_or(0.0, |s| s.height) - innen_rand(ctx, id, 0.0).top - innen_rand(ctx, id, 0.0).bottom;
        let inhalt = Rect::new(0.0, 0.0, 0.0, h.max(0.0));
        if let Some((a, d, normal)) = crate::flow::zeilen_durchschuss(ctx, id, n, &inhalt) {
            let halb = if normal { 0.0 } else { ((h - (a + d)) / 2.0).round() };
            return Some(oben + halb + a);
        }
        return None;
    }
    for &k in n.children.iter() {
        if !im_fluss(ctx, k) {
            continue;
        }
        if ctx.stil(k).is_some_and(|s| s.schweben != narsil_layout::plan::FloatArt::Keins) {
            continue;
        }
        let kn = ctx.document.get(k)?;
        match kn.kind {
            NodeKind::Text => {
                if kn.text.as_deref().is_some_and(|t| t.trim().is_empty()) {
                    continue;
                }
                let hoehe = sizes.get(k).map_or(kn.style.zeilenhoehe_px, |s| s.height);
                // Der Aufstieg der Schrift, mit der gezeichnet wird (wie in der
                // Zeilenbox, `zeilen_durchschuss`) — sonst stehen Zellen mit
                // Text direkt und Zellen mit Zeilenbox um die Differenz der
                // beiden Masse versetzt (GEMESSEN: 3,37 px bei Ahem 32 px,
                // WPT `table-anonymous-objects-201`).
                let a = ctx
                    .measurer
                    .zeilen_metrik(kn.style.font_size, kn.style.schriftgewicht, kn.style.kursiv, &kn.style.schriftfamilie)
                    .0;
                let g = crate::flow::text_inhaltsbereich(ctx, kn, hoehe).map_or(a, |(_, _, g)| g);
                return Some(oben + g);
            }
            NodeKind::Container => {
                let rand = resolve_spacing(kn.style.margin, 0.0, k, "margin").map_or(0.0, |m| m.top);
                if let Some(g) = erste_grundlinie(ctx, sizes, k, tiefe + 1) {
                    return Some(oben + rand + g);
                }
                // Ein Block ohne Zeile davor: weiter mit dem naechsten
                // nur, wenn er leer ist (CSS: die ERSTE Zeilenbox).
                if sizes.get(k).is_some_and(|s| s.height > 0.0) {
                    return None;
                }
            }
            NodeKind::Page => return None,
        }
    }
    None
}

/// Die Zeilenphase (nach dem Nachmessen der Zellen): Zeilenhoehen,
/// senkrechte Ausrichtung, Lage aller Teile. Liefert die Lage, die Groessen
/// der Teile, die Groesse des Tabellenkastens und die Inhaltsversaetze der
/// Zellen.
pub(crate) fn zeilen_phase(
    ctx: &Ctx<'_>,
    sizes: &Sizes,
    z: &Zwischen,
) -> (TabellenLage, Vec<(NodeId, Size)>, Size, Vec<(NodeId, f32)>) {
    let g = &z.gitter;
    let nz = g.zeilen.len();
    let n = z.spalten.len();
    let mut setzen: Vec<(NodeId, Size)> = Vec::new();
    let mut versatz: Vec<(NodeId, f32)> = Vec::new();
    let mut lage = TabellenLage::default();
    // Zellhoehen, Grundlinien, natuerliche Hoehen.
    struct Z {
        h: f32,
        natuerlich: f32,
        grund: f32,
        art: Vertikal,
    }
    let mut info: Vec<Z> = Vec::with_capacity(g.zellen.len());
    for (ci, c) in g.zellen.iter().enumerate() {
        let hb = sizes.get(c.knoten).map_or(0.0, |s| s.height);
        let natuerlich = sizes.natuerliche_hoehe(c.knoten).unwrap_or(hb).min(hb);
        let rand = innen_rand(ctx, c.knoten, 0.0);
        let grund = erste_grundlinie(ctx, sizes, c.knoten, 0).unwrap_or(nn(natuerlich - rand.bottom));
        let art = ctx.plan.map_or(Vertikal::Grundlinie, |p| p.vertikal(c.knoten));
        // Im Modell `collapse` zaehlen fuer die Zeilen nur die Teile
        // innerhalb der Rasterlinien (siehe `halbe_rahmen`).
        let b = z.halb.get(ci).copied().unwrap_or([0.0; 4]);
        info.push(Z { h: nn(hb - b[0] - b[2]), natuerlich, grund: grund - b[0], art });
    }
    // Zeilenhoehen (CSS 2.1 §17.5.3).
    let mut hoehen = vec![0.0f32; nz];
    let mut grundlinien = vec![0.0f32; nz];
    for (zi, zeile) in g.zeilen.iter().enumerate() {
        if let Some(Mass::Px(v)) = ctx.document.get(zeile.knoten).map(|n| mass_von(n.style.size.height)) {
            hoehen[zi] = v;
        }
    }
    for (c, i) in g.zellen.iter().zip(info.iter()) {
        if i.art == Vertikal::Grundlinie {
            grundlinien[c.zeile] = grundlinien[c.zeile].max(i.grund);
        }
    }
    for (c, i) in g.zellen.iter().zip(info.iter()) {
        if c.zeilen != 1 {
            continue;
        }
        let noetig = if i.art == Vertikal::Grundlinie { i.h.max(grundlinien[c.zeile] - i.grund + i.h) } else { i.h };
        hoehen[c.zeile] = hoehen[c.zeile].max(noetig);
    }
    // Zellen ueber mehrere Zeilen: was fehlt, gleich auf ihre Zeilen.
    for (c, i) in g.zellen.iter().zip(info.iter()) {
        if c.zeilen <= 1 {
            continue;
        }
        let bis = (c.zeile + c.zeilen).min(nz);
        let summe: f32 = hoehen[c.zeile..bis].iter().sum::<f32>() + z.v * (bis - c.zeile).saturating_sub(1) as f32;
        if i.h > summe {
            let je = (i.h - summe) / (bis - c.zeile) as f32;
            for h in hoehen[c.zeile..bis].iter_mut() {
                *h += je;
            }
        }
    }
    // Die angegebene Tabellenhoehe: der Ueberschuss auf die Zeilen.
    let luecken_v = if nz > 0 { z.v * (nz + 1) as f32 } else { 0.0 };
    let summe: f32 = hoehen.iter().sum();
    let mut raster = summe + luecken_v + z.rand.top + z.rand.bottom;
    if let Some(soll) = z.hoehe
        && soll > raster
    {
        let extra = soll - raster;
        if nz > 0 {
            for h in hoehen.iter_mut() {
                *h += if summe > 0.0 { extra * *h / summe } else { extra / nz as f32 };
            }
        }
        raster = soll;
    }
    // Spalten- und Zeilenlage (relativ zum Rahmen des Rasters).
    let mut sx = vec![0.0f32; n + 1];
    {
        let mut x = z.rand.left + z.h;
        for i in 0..n {
            sx[i] = x;
            x += z.spalten[i] + z.h;
        }
        sx[n] = x;
    }
    let mut zy = vec![0.0f32; nz + 1];
    {
        let mut y = z.rand.top + z.v;
        for i in 0..nz {
            zy[i] = y;
            y += hoehen[i] + z.v;
        }
        zy[nz] = y;
    }
    // Beschriftungen oben.
    let mut oben = 0.0f32;
    for (k, unten) in g.beschriftungen.iter() {
        if *unten {
            continue;
        }
        let Some(kn) = ctx.document.get(*k) else { continue };
        let m = resolve_spacing(kn.style.margin, z.breite, *k, "margin").unwrap_or(Edges::ZERO);
        let s = sizes.get(*k).unwrap_or(Size::ZERO);
        lage.plaetze.push((*k, Rect::new(m.left, oben + m.top, s.width, s.height)));
        oben += m.top + s.height + m.bottom;
    }
    lage.raster_oben = oben;
    lage.raster_hoehe = raster;
    let x0 = if n > 0 { sx[0] } else { z.rand.left };
    let zeilen_breite = if n > 0 { sx[n] - z.h - sx[0] } else { 0.0 };
    // Zeilen und Gruppen.
    for (zi, zeile) in g.zeilen.iter().enumerate() {
        let r = Rect::new(x0, oben + zy[zi], nn(zeilen_breite), hoehen[zi]);
        lage.plaetze.push((zeile.knoten, r));
        setzen.push((zeile.knoten, Size::new(r.width, r.height)));
    }
    for gr in g.gruppen.iter() {
        let r = if gr.bis > gr.von {
            let y0 = zy[gr.von];
            let y1 = zy[gr.bis - 1] + hoehen[gr.bis - 1];
            Rect::new(x0, oben + y0, nn(zeilen_breite), nn(y1 - y0))
        } else {
            Rect::new(x0, oben + zy[gr.von.min(nz)], nn(zeilen_breite), 0.0)
        };
        lage.plaetze.push((gr.knoten, r));
        setzen.push((gr.knoten, Size::new(r.width, r.height)));
    }
    // Zellen.
    for (ci, (c, i)) in g.zellen.iter().zip(info.iter()).enumerate() {
        let b = z.halb.get(ci).copied().unwrap_or([0.0; 4]);
        let von = c.spalte.min(n);
        let bis = (c.spalte + c.spalten).min(n);
        let x = sx[von] - b[3];
        let w = if bis > von { sx[bis] - z.h - sx[von] } else { 0.0 } + b[1] + b[3];
        let zb = (c.zeile + c.zeilen).min(nz);
        let y = zy[c.zeile] - b[0];
        let h = if zb > c.zeile { zy[zb] - z.v - zy[c.zeile] } else { 0.0 } + b[0] + b[2];
        lage.plaetze.push((c.knoten, Rect::new(x, oben + y, nn(w), nn(h))));
        setzen.push((c.knoten, Size::new(nn(w), nn(h))));
        let dy = match i.art {
            Vertikal::Grundlinie => grundlinien[c.zeile] - i.grund,
            Vertikal::Oben => 0.0,
            Vertikal::Mitte => (h - i.natuerlich) / 2.0,
            Vertikal::Unten => h - i.natuerlich,
        };
        if dy.is_finite() && dy > 0.0 {
            versatz.push((c.knoten, dy));
        }
    }
    // Spalten und Spaltengruppen: von der ersten bis zur letzten Zeile (CSS
    // 2.1 §17.5.1; ihr Hintergrund liegt unter den Zellen, die vor ihnen —
    // in Dokumentreihenfolge danach — gezeichnet werden). Naeherung: der
    // Hintergrund deckt auch den senkrechten Zellenabstand.
    let (sy0, sy1) = if nz > 0 { (zy[0], zy[nz - 1] + hoehen[nz - 1]) } else { (0.0, 0.0) };
    for (k, von, bis) in g.spalten_knoten.iter() {
        let von = (*von).min(n);
        let bis = (*bis).min(n);
        let x = sx[von];
        let w = if bis > von { sx[bis] - z.h - x } else { 0.0 };
        lage.plaetze.push((*k, Rect::new(x, oben + sy0, nn(w), nn(sy1 - sy0))));
        setzen.push((*k, Size::new(nn(w), nn(sy1 - sy0))));
    }
    // Beschriftungen unten.
    let mut unten = oben + raster;
    for (k, u) in g.beschriftungen.iter() {
        if !*u {
            continue;
        }
        let Some(kn) = ctx.document.get(*k) else { continue };
        let m = resolve_spacing(kn.style.margin, z.breite, *k, "margin").unwrap_or(Edges::ZERO);
        let s = sizes.get(*k).unwrap_or(Size::ZERO);
        lage.plaetze.push((*k, Rect::new(m.left, unten + m.top, s.width, s.height)));
        unten += m.top + s.height + m.bottom;
    }
    // `direction: rtl`: die Spalten laufen von rechts (GEMESSEN vorher: WPT
    // `direction-applies-to-013`, die erste Zelle stand links).
    if z.rtl {
        let beschriftungen: Vec<NodeId> = g.beschriftungen.iter().map(|(k, _)| *k).collect();
        for (k, r) in lage.plaetze.iter_mut() {
            if !beschriftungen.contains(k) {
                r.x = z.breite - (r.x + r.width);
            }
        }
    }
    (lage, setzen, Size::new(z.breite, unten), versatz)
}

/// Fuer die Platzphase: die absoluten Rechtecke aller Teile einer Tabelle,
/// deren Raster-Rahmen bei `rahmen` liegt.
pub(crate) fn absolute_plaetze(lage: &TabellenLage, rahmen_x: f32, rahmen_y: f32) -> HashMap<NodeId, Rect> {
    let oy = rahmen_y - lage.raster_oben;
    lage.plaetze
        .iter()
        .map(|(k, r)| (*k, Rect::new(rahmen_x + r.x, oy + r.y, r.width, r.height)))
        .collect()
}
