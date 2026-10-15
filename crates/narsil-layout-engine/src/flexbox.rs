//! Release 11.10.2026: CSS Flexible Box Layout 1 als eigener Algorithmus —
//! Flex-Basis und hypothetische Hauptgroesse (§9.2), Zeilen (§9.3), die
//! Aufloesung der flexiblen Laengen mit Einfrieren (§9.7), Quergroessen mit
//! Grundlinien (§9.4), `align-content`, Auto-Raender, `justify-content` und
//! `align-self` (§9.5/§9.6, CSS Box Alignment 3), die automatische
//! Mindestgroesse (§4.5), `*-reverse` und `wrap-reverse` (§5).
//!
//! GEMESSEN vorher (WPT css-flexbox, 548 von 1389 gruen): die alte Rechnung
//! (`flow.rs`, `verteile_flex_alle`) mass Spalten-Elemente nicht nach dem
//! Flexen nach, kannte keine Prozent-Basis gegen den Container, kein
//! Einfrieren nach §9.7, kein `wrap-reverse`, kein `space-evenly` und keine
//! Grundlinien in der Zeilenhoehe.
//!
//! Ablauf (wie beim Raster, `gitter.rs`): die Messphase (`box.rs`,
//! `SizeTask::Format`) ruft [`messen`] in Stufen auf und laesst die Elemente
//! mit ihrer benutzten Groesse nachmessen (`Ctx::flex_mass`, in `enter`
//! gelesen); die Platzierung (`flow.rs`) ruft [`verteilen`] und nimmt das
//! abgelegte Ergebnis.

use crate::box_sizing::{Sizes, get_node};
use crate::gitter::{Masse, Messung};
use crate::plan::Platzierung;
use crate::{Ctx, LayoutError};
use narsil_core::geometry::{Rect, Size};
use narsil_core::length::Length;
use narsil_dom::{NodeId, NodeKind};
use narsil_layout::flexstil::{FlexBasis, FlexStil};
use narsil_layout::gitter::{Ausr, AusrLage};
use narsil_layout::plan::PositionArt;

const UNENDLICH: f32 = f32::INFINITY;

/// Die Basisgroessen eines Elements (Rahmenmasse auf der Hauptachse).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Basis {
    /// Die Flex-Basis (§9.2 Schritt 3).
    pub basis: f32,
    /// Die Inhaltsgroesse auf der Hauptachse (fuer die automatische
    /// Mindestgroesse, §4.5).
    pub inhalt: f32,
    /// Die hypothetische Hauptgroesse samt Raendern (fuer die
    /// Inhaltshoehe einer Spalte, [`inhalt_hoehe`]).
    pub hyp_aussen: f32,
}

/// Das Ergebnis eines Flex-Containers (Messphase -> Platzierung).
#[derive(Debug, Clone, Default)]
pub(crate) struct Ergebnis {
    /// Die Inhaltsbreite, mit der gerechnet wurde.
    pub breite: f32,
    /// Die definite Inhaltshoehe (`None` = aus dem Inhalt).
    pub hoehe: Option<f32>,
    /// Die Flex-Elemente (Index im DOM, Knoten) in `order`-Reihenfolge.
    pub kinder: Vec<(usize, NodeId)>,
    /// Je Element die Basisgroessen (in den spaeteren Stufen
    /// wiederverwendet: die Messung ist dann schon die benutzte Groesse).
    pub basen: Vec<Basis>,
    /// Je Element der Rahmenkasten relativ zur Inhaltsbox.
    pub kaesten: Vec<Rect>,
    /// Je Element: wird es auf der Querachse gedehnt?
    pub gedehnt: Vec<bool>,
    /// Die Inhaltshoehe des Containers.
    pub inhalt_hoehe: f32,
    /// Die naechste Messstufe (siehe [`messen`]).
    pub stufe: u8,
}

/// Ein Flex-Element waehrend der Rechnung (Flex-Koordinaten: die
/// Hauptachse laeuft vom Haupt-Anfang, die Querachse vom Quer-Anfang).
#[derive(Debug, Clone)]
struct El {
    id: NodeId,
    m: Masse,
    anonym: bool,
    /// Raender am Haupt-Anfang/-Ende und ob sie `auto` sind.
    r0: f32,
    r1: f32,
    a0: bool,
    a1: bool,
    /// Raender am Quer-Anfang/-Ende.
    q0: f32,
    q1: f32,
    qa0: bool,
    qa1: bool,
    basis: f32,
    inhalt: f32,
    hyp: f32,
    min: f32,
    max: f32,
    wachsen: f32,
    schrumpfen: f32,
    fuell_haupt: f32,
    ziel: f32,
    gefroren: bool,
    /// Hypothetische Quergroesse (Rahmen) und ihre Grenzen.
    quer: f32,
    quer_min: Option<f32>,
    quer_max: Option<f32>,
    quer_auto: bool,
    /// Release 15.10.2026: the cross size is `stretch` (CSS Sizing 4 §3.1,
    /// `LayoutStil::stretch_bits` 64/128).
    streck: bool,
    ausr: AusrLage,
    sicher: Option<bool>,
    /// Grundlinien: Abstand vom Quer-Anfang des Randkastens (erste) bzw.
    /// vom Quer-Ende (letzte).
    grund: f32,
}

impl El {
    fn aussen_ziel(&self) -> f32 {
        self.ziel + self.r0 + self.r1
    }
    fn aussen_hyp(&self) -> f32 {
        self.hyp + self.r0 + self.r1
    }
    fn dehnt(&self) -> bool {
        self.streck || (matches!(self.ausr, AusrLage::Normal | AusrLage::Strecken) && self.quer_auto && !self.qa0 && !self.qa1)
    }
}

/// Die Achsen-Spiegelungen eines Containers: (Hauptachse, Querachse).
fn spiegel(fs: &FlexStil) -> (bool, bool) {
    if fs.reihe {
        (fs.umgekehrt != fs.rtl, fs.umbruch_umgekehrt)
    } else {
        (fs.umgekehrt, fs.rtl != fs.umbruch_umgekehrt)
    }
}

/// Die angegebene Groesse (`width`/`height`) als Rahmenmass, wenn sie
/// definit ist (Prozent gegen `basis`).
fn angabe(l: Length, basis: Option<f32>, fuell: f32, border_box: bool) -> Option<f32> {
    let v = match l {
        Length::Px(v) if v.is_finite() => v,
        Length::Percent(p) if p.is_finite() => basis? * p / 100.0,
        Length::Gemischt { px, prozent } if px.is_finite() && prozent.is_finite() => px + basis? * prozent / 100.0,
        _ => return None,
    };
    let inhalt = if border_box { (v - fuell).max(0.0) } else { v.max(0.0) };
    Some(inhalt + fuell)
}

/// Die natuerliche Groesse eines ersetzten Elements nach CSS 2.1 §10.3.2
/// (Rahmenmasse), aus Angabe, Verhaeltnis und natuerlicher Groesse.
fn ersetzt_groesse(node: &narsil_dom::Node, m: &Masse, basis_b: Option<f32>, basis_h: Option<f32>, verh: Option<f32>) -> (f32, f32) {
    let (fh, fv) = m.fuell;
    let w = angabe(node.style.size.width, basis_b, fh, m.border_box).map(|w| w - fh);
    let h = angabe(node.style.size.height, basis_h, fv, m.border_box).map(|h| h - fv);
    let natur = node.ersetzt.unwrap_or_default();
    let (nw, nh) = (natur.breite.unwrap_or(300.0), natur.hoehe.unwrap_or(150.0));
    // Die Grenzen (Inhaltsmasse) wirken vor der Uebertragung durch das
    // Verhaeltnis (CSS 2.1 §10.4). GEMESSEN vorher: WPT
    // `flex-aspect-ratio-img-column-005` (`width:500px; max-width:100%`:
    // 500 statt 100 px hoch).
    let grenze = |v: f32, mn: Option<f32>, mx: Option<f32>, f: f32| {
        let mut v = v;
        if let Some(x) = mx {
            v = v.min((x - f).max(0.0));
        }
        if let Some(x) = mn {
            v = v.max((x - f).max(0.0));
        }
        v
    };
    let w = w.map(|w| grenze(w, m.min_b, m.max_b, fh));
    let h = h.map(|h| grenze(h, m.min_h, m.max_h, fv));
    let (iw, ih) = match (w, h, verh) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(r)) => (w, grenze(w / r, m.min_h, m.max_h, fv)),
        (None, Some(h), Some(r)) => (grenze(h * r, m.min_b, m.max_b, fh), h),
        (Some(w), None, None) => (w, nh),
        (None, Some(h), None) => (nw, h),
        (None, None, Some(r)) => {
            let iw = grenze(nw, m.min_b, m.max_b, fh);
            let ih = if (iw - nw).abs() > 0.01 { iw / r } else { nh };
            let ih2 = grenze(ih, m.min_h, m.max_h, fv);
            let iw = if (ih2 - ih).abs() > 0.01 { ih2 * r } else { iw };
            (iw, ih2)
        }
        (None, None, None) => (grenze(nw, m.min_b, m.max_b, fh), grenze(nh, m.min_h, m.max_h, fv)),
    };
    (iw + fh, ih + fv)
}

/// Release 11.10.2026: das natuerliche Seitenverhaeltnis (Breite / Hoehe)
/// eines ersetzten Elements, falls es eines hat.
fn ersetzt_verhaeltnis(ctx: &Ctx<'_>, id: NodeId) -> Option<f32> {
    let node = ctx.document.get(id)?;
    node.ersetzt?;
    crate::r#box::wirksames_verhaeltnis(ctx, id, node)
}

/// min-content-Breite (Rahmen) eines Elements ohne seine eigene Angabe.
fn min_inhalt(ctx: &Ctx<'_>, sizes: &Sizes, node: &narsil_dom::Node, m: &Masse) -> f32 {
    if matches!(node.style.size.width, Length::Px(_)) {
        let mut w = 0.0f32;
        for &c in node.children.iter().take(crate::plan::MAX_KINDER) {
            let Some(cs) = ctx.stil(c) else {
                w = w.max(sizes.min_inhalt_breite(ctx, c, 1));
                continue;
            };
            if !cs.wird_gezeichnet() || matches!(cs.position, PositionArt::Absolut | PositionArt::Fest) {
                continue;
            }
            w = w.max(sizes.min_inhalt_breite(ctx, c, 1));
        }
        w + m.fuell.0
    } else {
        sizes.min_inhalt_breite(ctx, node.id, 0)
    }
}

/// Release 11.10.2026: die Inhaltshoehe eines Kastens ohne seine eigene
/// Hoehenangabe (Rahmen) — fuer die automatische Mindesthoehe (§4.5: die
/// min-content-Groesse; Prozenthoehen darin zaehlen als `auto`). GEMESSEN
/// vorher: WPT `flex-flow-007` (vier 50-px-Elemente in 100 px schrumpften
/// nicht, die Messung war die angegebene Hoehe).
fn inhalt_hoehe(ctx: &Ctx<'_>, sizes: &Sizes, id: NodeId, tiefe: usize) -> f32 {
    let Some(node) = ctx.document.get(id) else { return 0.0 };
    let gemessen = sizes.get(id).map_or(0.0, |g| g.height);
    let definit = matches!(node.style.size.height, Length::Px(_) | Length::Percent(_) | Length::Gemischt { .. })
        || ctx.stil(id).is_some_and(|s| s.hoehe_bedingt.is_some());
    if !definit || tiefe > 8 || node.kind != NodeKind::Container || node.ersetzt.is_some() {
        return gemessen;
    }
    let breite = sizes.get(id).map_or(0.0, |g| g.width);
    let fuell = crate::box_sizing::resolve_spacing(node.style.padding, breite, id, "padding").unwrap_or(narsil_core::geometry::Edges::ZERO);
    let rahmen = crate::r#box::rahmen_edges(&node.style.rahmen);
    let aussen = fuell.top + fuell.bottom + rahmen.top + rahmen.bottom;
    // Eine Flex-Spalte: die Summe der hypothetischen Hauptgroessen ihrer
    // Elemente (nicht der geflexten). GEMESSEN vorher: WPT
    // `flex-minimum-height-flex-items-026` (200 statt 0 px Inhalt).
    if let Some(fs) = ctx.stil(id).and_then(|s| s.flex.as_deref())
        && !fs.reihe
        && let Some(e) = ctx.flex.borrow().get(&id)
        && !e.basen.is_empty()
    {
        let luecke = crate::gitter::luecke_px(fs.luecke_zeile, None, 0.0);
        let n = e.basen.len();
        return e.basen.iter().map(|b| b.hyp_aussen).sum::<f32>() + luecke * n.saturating_sub(1) as f32 + aussen;
    }
    let reihe = node.style.direction == narsil_core::style::Direction::Row
        || ctx.stil(id).is_some_and(|s| s.flex.as_deref().is_some_and(|f| f.reihe));
    let mut h = 0.0f32;
    for &c in node.children.iter().take(crate::plan::MAX_KINDER) {
        if ctx.stil(c).is_some_and(|s| !s.wird_gezeichnet() || matches!(s.position, PositionArt::Absolut | PositionArt::Fest)) {
            continue;
        }
        let Some(k) = ctx.document.get(c) else { continue };
        let rand = crate::box_sizing::resolve_spacing(k.style.margin, breite, c, "margin").unwrap_or(narsil_core::geometry::Edges::ZERO);
        let kh = if prozent_hoehe(ctx, k, c) { inhalt_hoehe_auto(ctx, sizes, c, tiefe + 1) } else { sizes.get(c).map_or(0.0, |g| g.height) };
        let a = kh + rand.top + rand.bottom;
        if reihe { h = h.max(a) } else { h += a }
    }
    h + aussen
}

/// Release 11.10.2026: hat der Knoten eine Prozenthoehe (auch eine, die
/// erst das Layout aufloest, `LayoutStil::hoehe_bedingt`)?
fn prozent_hoehe(ctx: &Ctx<'_>, k: &narsil_dom::Node, c: NodeId) -> bool {
    matches!(k.style.size.height, Length::Percent(_)) || ctx.stil(c).is_some_and(|s| s.hoehe_bedingt.is_some())
}

/// Wie [`inhalt_hoehe`], aber die Hoehe des Kastens selbst zaehlt als
/// `auto` (eine Prozenthoehe in einem Inhaltsmass).
fn inhalt_hoehe_auto(ctx: &Ctx<'_>, sizes: &Sizes, id: NodeId, tiefe: usize) -> f32 {
    let Some(node) = ctx.document.get(id) else { return 0.0 };
    // Eine Tabelle: ihre natuerliche Hoehe (ohne die angegebene, `box.rs`
    // `SizeTask::TabelleZeilen`).
    if crate::tabellen_leg::ist_tabelle(ctx, id) {
        return sizes.natuerliche_hoehe(id).or_else(|| sizes.get(id).map(|g| g.height)).unwrap_or(0.0);
    }
    if tiefe > 8 || node.kind != NodeKind::Container {
        return sizes.get(id).map_or(0.0, |g| g.height);
    }
    let breite = sizes.get(id).map_or(0.0, |g| g.width);
    let fuell = crate::box_sizing::resolve_spacing(node.style.padding, breite, id, "padding").unwrap_or(narsil_core::geometry::Edges::ZERO);
    let rahmen = crate::r#box::rahmen_edges(&node.style.rahmen);
    let aussen = fuell.top + fuell.bottom + rahmen.top + rahmen.bottom;
    let reihe = node.style.direction == narsil_core::style::Direction::Row;
    let mut h = 0.0f32;
    for &c in node.children.iter().take(crate::plan::MAX_KINDER) {
        if ctx.stil(c).is_some_and(|s| !s.wird_gezeichnet() || matches!(s.position, PositionArt::Absolut | PositionArt::Fest)) {
            continue;
        }
        let Some(k) = ctx.document.get(c) else { continue };
        let rand = crate::box_sizing::resolve_spacing(k.style.margin, breite, c, "margin").unwrap_or(narsil_core::geometry::Edges::ZERO);
        let kh = if prozent_hoehe(ctx, k, c) { inhalt_hoehe_auto(ctx, sizes, c, tiefe + 1) } else { sizes.get(c).map_or(0.0, |g| g.height) };
        let a = kh + rand.top + rand.bottom;
        if reihe { h = h.max(a) } else { h += a }
    }
    // `min-height`/`max-height` gelten auch fuer die `auto`-Hoehe.
    // GEMESSEN vorher: WPT `flex-minimum-height-flex-items-015`
    // (`height: 100%; min-height: 80px` trug 0 px bei).
    let border_box = ctx.stil(id).is_some_and(|s| s.border_box);
    let kasten = |v: f32| if border_box { v } else { v + aussen };
    let mut h = h + aussen;
    if let Some(p) = ctx.plan {
        let g = p.grenzen(id);
        if let Some(m) = g.max_hoehe {
            h = h.min(kasten(m));
        }
        if let Some(m) = g.min_hoehe {
            h = h.max(kasten(m));
        }
    }
    h
}

/// max-content-Breite (Rahmen) eines Elements ohne seine eigene Angabe
/// (fuer `flex-basis: content`, §7.3.3).
fn max_inhalt(ctx: &Ctx<'_>, sizes: &Sizes, node: &narsil_dom::Node, m: &Masse) -> f32 {
    if !matches!(node.style.size.width, Length::Px(_)) {
        return sizes.eigene_breite(ctx, node.id, true);
    }
    let reihe = node.style.direction == narsil_core::style::Direction::Row;
    let mut w = 0.0f32;
    for &c in node.children.iter().take(crate::plan::MAX_KINDER) {
        if ctx.stil(c).is_some_and(|s| !s.wird_gezeichnet() || matches!(s.position, PositionArt::Absolut | PositionArt::Fest)) {
            continue;
        }
        let v = sizes.max_content_breite(ctx, c, 1);
        if reihe { w += v } else { w = w.max(v) }
    }
    w + m.fuell.0
}

/// Die Groesse eines anonymen Elements (seine Glieder untereinander).
fn anonym_groesse(sizes: &Sizes, id: NodeId, m: &Masse) -> Size {
    match &m.glieder {
        Some(g) => {
            let mut s = Size::new(0.0, 0.0);
            for &k in g.iter() {
                let x = sizes.get(k).unwrap_or(Size::new(0.0, 0.0));
                s.width = s.width.max(x.width);
                s.height += x.height;
            }
            s
        }
        None => sizes.get(id).unwrap_or(Size::new(0.0, 0.0)),
    }
}

/// Die Mess-Eingabe fuer [`rechnen`].
#[derive(Clone, Copy)]
struct Lauf<'a, 'b> {
    ctx: &'a Ctx<'b>,
    node: &'a narsil_dom::Node,
    fs: &'a FlexStil,
    sizes: &'a Sizes,
    /// Inhaltsbreite (definit).
    breite: f32,
    /// Definite Inhaltshoehe.
    hoehe: Option<f32>,
    /// Grenzen der Inhaltshoehe (bei `height: auto`).
    min_h: Option<f32>,
    max_h: Option<f32>,
    /// Basisgroessen aus einer frueheren Stufe.
    basen: Option<&'a [Basis]>,
    /// Spalte, Stufe 0: die Querbreiten sind noch zu bestimmen (die
    /// Messung gilt dafuer nicht).
    quer_wunsch: bool,
    /// Die Probe fuer die Inhaltsbreite (`intrinsisch`): Prozentbreiten der
    /// Elemente sind dort zyklisch (unbestimmt).
    probe: bool,
}

/// Der Flex-Algorithmus (§9) gegen die aktuellen Messungen.
fn rechnen(l: &Lauf<'_, '_>) -> Result<Ergebnis, LayoutError> {
    let ctx = l.ctx;
    let fs = l.fs;
    let sizes = l.sizes;
    let reihe = fs.reihe;
    let (spiegel_haupt, spiegel_quer) = spiegel(fs);
    let (kinder, _) = crate::gitter::rasterelemente(ctx, l.node);
    let haupt_def = if reihe { Some(l.breite) } else { l.hoehe };
    let quer_def = if reihe { l.hoehe } else { Some(l.breite) };
    let (lh, lq) = if reihe { (fs.luecke_spalte, fs.luecke_zeile) } else { (fs.luecke_zeile, fs.luecke_spalte) };
    let luecke = crate::gitter::luecke_px(lh, haupt_def, 0.0);
    let luecke_quer = crate::gitter::luecke_px(lq, quer_def, 0.0);
    let grenzen_p = |id: NodeId| ctx.plan.map(|p| p.grenzen_prozent(id));

    // §9.2 Schritt 3: Flex-Basis und hypothetische Hauptgroesse.
    let mut el: Vec<El> = Vec::with_capacity(kinder.len());
    for (k, &(_, id)) in kinder.iter().enumerate() {
        let node = get_node(ctx, id)?;
        let mut m = crate::gitter::masse(ctx, id, l.breite, l.hoehe.unwrap_or(0.0))?;
        // `min-width`/`max-width: min-content | max-content | fit-content`
        // (CSS Sizing 3 §2.1). GEMESSEN vorher: WPT
        // `flex-item-min-width-min-content-max-width`.
        if !m.text {
            m = crate::gitter::mit_grenz_worten(ctx, sizes, id, &m, 0);
        }
        // Prozentgrenzen der Hoehe ohne definite Hoehe gelten nicht.
        if l.hoehe.is_none()
            && let Some(g) = grenzen_p(id)
        {
            if g.min_hoehe.is_some() {
                m.min_h = None;
            }
            if g.max_hoehe.is_some() {
                m.max_h = None;
            }
        }
        let gemessen = sizes.get(id).unwrap_or(Size::new(0.0, 0.0));
        let st = ctx.stil(id);
        let anonym = m.text;
        let r = m.rand;
        let ra = m.rand_auto;
        let (r0, r1, a0, a1, q0, q1, qa0, qa1) = if reihe {
            let (h0, h1, ha0, ha1) = if spiegel_haupt { (r.right, r.left, ra[2], ra[0]) } else { (r.left, r.right, ra[0], ra[2]) };
            let (v0, v1, va0, va1) = if spiegel_quer { (r.bottom, r.top, ra[3], ra[1]) } else { (r.top, r.bottom, ra[1], ra[3]) };
            (h0, h1, ha0, ha1, v0, v1, va0, va1)
        } else {
            let (v0, v1, va0, va1) = if spiegel_haupt { (r.bottom, r.top, ra[3], ra[1]) } else { (r.top, r.bottom, ra[1], ra[3]) };
            let (h0, h1, ha0, ha1) = if spiegel_quer { (r.right, r.left, ra[2], ra[0]) } else { (r.left, r.right, ra[0], ra[2]) };
            (v0, v1, va0, va1, h0, h1, ha0, ha1)
        };
        let (fuell_haupt, fuell_quer) = if reihe { (m.fuell.0, m.fuell.1) } else { (m.fuell.1, m.fuell.0) };
        let (mn, mx) = if reihe { (m.min_b, m.max_b) } else { (m.min_h, m.max_h) };
        let (qmn, qmx) = if reihe { (m.min_h, m.max_h) } else { (m.min_b, m.max_b) };
        let max = mx.unwrap_or(UNENDLICH).max(fuell_haupt);
        let (wachsen, schrumpfen) = if anonym { (0.0, 1.0) } else { st.map_or((0.0, 1.0), |s| (s.flex_wachstum.max(0.0), s.flex_schrumpf.max(0.0))) };
        let haupt_angabe = if anonym {
            None
        } else if reihe {
            if m.breite_wort != 0 { None } else { angabe(node.style.size.width, Some(l.breite), fuell_haupt, m.border_box) }
        } else if st.is_some_and(|s| s.groessen_art[3] != 0) {
            None
        } else {
            angabe(node.style.size.height, l.hoehe, fuell_haupt, m.border_box)
        };
        let quer_angabe = if anonym {
            None
        } else if reihe {
            angabe(node.style.size.height, l.hoehe, fuell_quer, m.border_box)
        } else if m.breite_wort != 0 {
            None
        } else {
            angabe(node.style.size.width, (!l.probe).then_some(l.breite), fuell_quer, m.border_box)
        };
        let ersetzt = node.ersetzt.is_some();
        let ks = st.and_then(|s| s.gitter_kind.as_deref());
        let selbst = schreib_bezug(crate::gitter::selbst_aufloesen(ks.map_or(Ausr::AUTO, |k| k.align_self), fs.align_items), fs.umbruch_umgekehrt);
        let selbst = selbst_richtung(selbst, st.is_some_and(|s| s.rtl), fs.rtl, !reihe);
        // Release 15.10.2026: a cross size `stretch` is the container's inner
        // cross size (when definite) minus the margins in the measure pass and
        // the line's cross size in the end, whatever `align-self` says (CSS
        // Sizing 4 §3.1, csswg-drafts#11784). MEASURED before: WPT css-sizing
        // `stretch/flex-line-003` (26 instead of 100 px), `stretch/flexbox-
        // flex-base-size-001` (no transfer through the ratio: 100 x 100).
        let streck = !anonym && quer_angabe.is_none() && st.is_some_and(|s| s.stretch_bits & if reihe { 128 } else { 64 } != 0);
        // §9.2 Schritt 3 B: ein ersetztes Element mit Seitenverhaeltnis und
        // definiter Quergroesse — auch der gedehnten einer einzeiligen
        // Zeile mit definiter Quergroesse (§9.8.1) — bekommt die Basis aus
        // dem Verhaeltnis. GEMESSEN vorher: WPT `image-as-flexitem-size-003`
        // (16 px statt 40 px).
        // `aspect-ratio` an einem nicht ersetzten Element (CSS Sizing 4 §5).
        let css_r = st.map_or(0.0, |s| s.seitenverhaeltnis.abs());
        let css_r = (css_r.is_finite() && css_r > 0.0 && !ersetzt && !anonym).then_some(css_r);
        let gedehnte_quer = if (ersetzt || css_r.is_some())
            && quer_angabe.is_none()
            && (streck || (!fs.umbruch && matches!(selbst.lage, AusrLage::Normal | AusrLage::Strecken) && !qa0 && !qa1))
        {
            quer_def.map(|q| crate::gitter::klemmen(q - q0 - q1, qmn, qmx).max(fuell_quer))
        } else {
            None
        };
        // Release 11.10.2026: das wirksame Verhaeltnis (`aspect-ratio` vor dem
        // natuerlichen, siehe `box.rs`). GEMESSEN vorher: WPT css-sizing
        // `aspect-ratio/flex-aspect-ratio-015..018`.
        let verhaeltnis = if ersetzt { crate::r#box::wirksames_verhaeltnis(ctx, id, node) } else { None };
        // Die Inhaltsgroesse auf der Hauptachse.
        let inhalt_jetzt = || -> f32 {
            if anonym {
                let g = anonym_groesse(sizes, id, &m);
                return if reihe { sizes.max_content_breite(ctx, id, 0).max(g.width) } else { g.height };
            }
            if ersetzt {
                if let (Some(q), Some(r)) = (gedehnte_quer, verhaeltnis) {
                    let innen = (q - fuell_quer).max(0.0);
                    return if reihe { innen * r + fuell_haupt } else { innen / r + fuell_haupt };
                }
                let (w, h) = ersetzt_groesse(node, &m, Some(l.breite), l.hoehe, verhaeltnis);
                return if reihe { w } else { h };
            }
            // Release 14.10.2026: in a column without a definite cross size
            // the cross size is the fit-content width (as in stage 0 below),
            // and the main content size follows it through the ratio. MEASURED
            // before: WPT css-sizing `aspect-ratio/flex-aspect-ratio-037`
            // (`align-items: start`, a 100 px wide child: 0 x 0).
            let quer_passend = if !reihe && css_r.is_some() && quer_angabe.is_none() && gedehnte_quer.is_none() && m.breite_wort == 0 {
                let platz = (l.breite - q0 - q1).max(0.0);
                let mi = sizes.eigene_breite(ctx, id, false);
                let ma = sizes.eigene_breite(ctx, id, true).max(mi);
                Some(crate::gitter::klemmen(ma.min(platz.max(mi)), qmn, qmx).max(fuell_quer))
            } else {
                None
            };
            if let (Some(r), Some(q)) = (css_r, quer_angabe.or(gedehnte_quer).or(quer_passend)) {
                // Das Verhaeltnis gilt fuer den Kasten nach `box-sizing`.
                return if m.border_box {
                    if reihe { q * r } else { q / r }
                } else {
                    let innen = (q - fuell_quer).max(0.0);
                    (if reihe { innen * r } else { innen / r }) + fuell_haupt
                };
            }
            if reihe {
                match m.breite_wort {
                    // Release 14.10.2026: the flex base size ignores the item's
                    // own min/max width (`Sizes::eigene_breite`, §9.2.3).
                    1 => sizes.eigene_breite(ctx, id, false),
                    _ => sizes.eigene_breite(ctx, id, true),
                }
            } else {
                gemessen.height
            }
        };
        // Die Inhaltshoehe ohne die eigene Angabe (fuer §4.5).
        let inhalt_ohne_angabe = || -> f32 { if !reihe && !anonym && !ersetzt { inhalt_hoehe(ctx, sizes, id, 0) } else { inhalt_jetzt() } };
        let (basis, inhalt) = match l.basen.and_then(|b| b.get(k)) {
            Some(b) => (b.basis, b.inhalt),
            None => {
                let inhalt_basis = inhalt_jetzt();
                let inhalt = inhalt_ohne_angabe();
                let basis_art = st.map_or(FlexBasis::Auto, |s| s.flex_basis_art);
                let rahmen = |v: f32| if m.border_box { v.max(fuell_haupt) } else { v + fuell_haupt };
                // `flex-basis: content`: die Inhaltsgroesse ohne die eigene
                // Hauptangabe (§7.3.3). GEMESSEN vorher: WPT
                // `flexbox-flex-basis-content-001a` (die Breite galt).
                let inhalt_ohne = || -> f32 {
                    if anonym {
                        inhalt
                    } else if ersetzt {
                        let quer = quer_angabe.or(gedehnte_quer);
                        let natur = node.ersetzt.unwrap_or_default();
                        match (quer, verhaeltnis) {
                            (Some(q), Some(r)) => {
                                let innen = (q - fuell_quer).max(0.0);
                                (if reihe { innen * r } else { innen / r }) + fuell_haupt
                            }
                            _ => (if reihe { natur.breite.unwrap_or(300.0) } else { natur.hoehe.unwrap_or(150.0) }) + fuell_haupt,
                        }
                    } else if let (Some(r), Some(q)) = (css_r, quer_angabe.or(gedehnte_quer)) {
                        // Release 14.10.2026: `aspect-ratio` with a definite
                        // cross size: the content size is the transferred one
                        // (CSS Sizing 4 §5.1). MEASURED before: WPT css-sizing
                        // `aspect-ratio/flex-aspect-ratio-021` (`flex-basis:
                        // content; height: 100px; aspect-ratio: 1/1`: 0 px wide).
                        if m.border_box {
                            if reihe { q * r } else { q / r }
                        } else {
                            let innen = (q - fuell_quer).max(0.0);
                            (if reihe { innen * r } else { innen / r }) + fuell_haupt
                        }
                    } else if reihe {
                        max_inhalt(ctx, sizes, node, &m)
                    } else {
                        inhalt
                    }
                };
                let basis = match basis_art {
                    FlexBasis::Laenge(lp) => match lp.aufloesen(haupt_def) {
                        Some(v) => rahmen(v.max(0.0)),
                        None => haupt_angabe.unwrap_or(inhalt_basis),
                    },
                    FlexBasis::Auto => haupt_angabe.unwrap_or(inhalt_basis),
                    FlexBasis::MinInhalt if reihe && !anonym && !ersetzt => min_inhalt(ctx, sizes, node, &m),
                    _ => inhalt_ohne(),
                };
                (basis, inhalt)
            }
        };
        // §4.5: die automatische Mindestgroesse.
        let min = match mn {
            Some(v) => v,
            None if m.scroll || anonym => 0.0,
            None => {
                // Ein ersetztes Element: die natuerliche Groesse, mit
                // definiter Quergroesse durch das Verhaeltnis uebertragen —
                // nie die eigene Hauptangabe (§4.5 „content size
                // suggestion"). GEMESSEN vorher: WPT
                // `flexbox-min-width-auto-002a` (100 statt 30 px).
                let inhalt_min = if ersetzt {
                    let quer = quer_angabe.or(gedehnte_quer);
                    let natur = node.ersetzt.unwrap_or_default();
                    // Durch das Verhaeltnis: eine Quergroesse (Rahmen) als
                    // Hauptgroesse (Rahmen).
                    let uebertragen = |q: f32, r: f32| {
                        let innen = (q - fuell_quer).max(0.0);
                        (if reihe { innen * r } else { innen / r }) + fuell_haupt
                    };
                    match (quer, verhaeltnis) {
                        (Some(q), Some(r)) => uebertragen(q, r),
                        // §4.5: die natuerliche Groesse, mit Verhaeltnis an
                        // `min-*`/`max-*` der Querachse geklemmt (durch das
                        // Verhaeltnis uebertragen). GEMESSEN vorher: WPT
                        // `flex-aspect-ratio-img-column-011` (`max-height:
                        // 5px` an einem 100-px-Bild: 100 statt 5 px breit).
                        (None, Some(r)) => {
                            let mut v = (if reihe { natur.breite.unwrap_or(300.0) } else { natur.hoehe.unwrap_or(150.0) }) + fuell_haupt;
                            if let Some(x) = qmx {
                                v = v.min(uebertragen(x, r));
                            }
                            if let Some(x) = qmn {
                                v = v.max(uebertragen(x, r));
                            }
                            v
                        }
                        _ => (if reihe { natur.breite.unwrap_or(300.0) } else { natur.hoehe.unwrap_or(150.0) }) + fuell_haupt,
                    }
                } else if let (Some(r), Some(q)) = (css_r, quer_angabe.or(gedehnte_quer)) {
                    // Release 14.10.2026: with a definite (or stretched)
                    // cross size the min-content size of an `aspect-ratio`
                    // box is the transferred one (CSS Sizing 4 §5.1), at least
                    // its contents. MEASURED before: WPT css-sizing
                    // `aspect-ratio/flex-aspect-ratio-045` (a stretched 100 px
                    // high item in a 0 px wide row shrank to 0).
                    let t = if m.border_box {
                        if reihe { q * r } else { q / r }
                    } else {
                        let innen = (q - fuell_quer).max(0.0);
                        (if reihe { innen * r } else { innen / r }) + fuell_haupt
                    };
                    t.max(if reihe { min_inhalt(ctx, sizes, node, &m) } else { inhalt })
                } else if reihe {
                    min_inhalt(ctx, sizes, node, &m)
                } else {
                    inhalt
                };
                let inhalt_min = inhalt_min.min(max);
                match haupt_angabe {
                    Some(a) => a.min(inhalt_min),
                    None => inhalt_min,
                }
            }
        }
        .max(fuell_haupt);
        let hyp = basis.min(max).max(min);
        // Die hypothetische Quergroesse.
        let quer_auto = !anonym
            && quer_angabe.is_none()
            && if reihe { m.hoehe_auto || node.style.size.height.hat_prozent() } else { m.breite_auto || m.breite_prozent };
        let quer = if anonym {
            let g = anonym_groesse(sizes, id, &m);
            if reihe { g.height } else { g.width }
        } else if let (true, true, Some(h)) = (streck, reihe, l.hoehe) {
            crate::gitter::klemmen(h - q0 - q1, qmn, qmx).max(fuell_quer)
        } else if reihe {
            gemessen.height
        } else {
            gemessen.width
        };
        let ausr = match selbst.lage {
            AusrLage::Grundlinie | AusrLage::LetzteGrundlinie if !reihe => AusrLage::Anfang,
            l => l,
        };
        el.push(El {
            id,
            m,
            anonym,
            r0,
            r1,
            a0,
            a1,
            q0,
            q1,
            qa0,
            qa1,
            basis,
            inhalt,
            hyp,
            min,
            max,
            wachsen,
            schrumpfen,
            fuell_haupt,
            ziel: hyp,
            gefroren: false,
            quer: quer.max(0.0),
            quer_min: qmn,
            quer_max: qmx,
            quer_auto,
            streck,
            ausr,
            sicher: selbst.sicher,
            grund: 0.0,
        });
    }

    // Spalte, Stufe 0: die Querbreite jedes Elements (gedehnt oder
    // `fit-content`, §9.4 Schritt 7 und §9.8).
    if l.quer_wunsch {
        for e in el.iter_mut().filter(|e| !e.anonym) {
            let node = get_node(ctx, e.id)?;
            let platz = (l.breite - e.q0 - e.q1).max(0.0);
            let qa = angabe(node.style.size.width, Some(l.breite), e.m.fuell.0, e.m.border_box).filter(|_| e.m.breite_wort == 0);
            let w = if let Some(a) = qa {
                a
            } else if e.dehnt() && (!fs.umbruch || e.streck) {
                platz
            } else {
                let mi = sizes.min_inhalt_breite(ctx, e.id, 0);
                let ma = sizes.max_content_breite(ctx, e.id, 0).max(mi);
                match e.m.breite_wort {
                    1 => mi,
                    2 => ma,
                    _ => ma.min(platz.max(mi)),
                }
            };
            e.quer = crate::gitter::klemmen(w, e.quer_min, e.quer_max).max(e.m.fuell.0);
        }
    }

    // §9.3: Zeilen.
    let verfuegbar = haupt_def.or(if reihe { None } else { l.max_h }).unwrap_or(UNENDLICH);
    let mut zeilen: Vec<std::ops::Range<usize>> = Vec::new();
    if fs.umbruch {
        let mut start = 0usize;
        let mut summe = 0.0f32;
        for (i, e) in el.iter().enumerate() {
            let a = e.aussen_hyp();
            if i > start && summe + luecke + a > verfuegbar + 0.01 {
                zeilen.push(start..i);
                start = i;
                summe = a;
            } else {
                summe += if i > start { luecke + a } else { a };
            }
        }
        if start < el.len() || zeilen.is_empty() {
            zeilen.push(start..el.len());
        }
        // `flex-line-count` setzt die Mindestzahl der Zeilen (hoechstens
        // eine je Element). GEMESSEN vorher: WPT
        // `balance/balance-min-line-count-*`.
        let soll = zeilen.len().max(fs.zeilen_min as usize).min(el.len().max(1));
        if fs.ausgleich && soll > 1 && verfuegbar.is_finite() {
            zeilen = ausgleichen(&el, soll, verfuegbar, luecke);
        }
    } else {
        zeilen.push(0..el.len());
    }

    // Die innere Hauptgroesse des Containers.
    let haupt_innen = match haupt_def {
        Some(v) => v,
        None => {
            let mut g = 0.0f32;
            for z in &zeilen {
                let n = z.len();
                let s: f32 = el[z.clone()].iter().map(El::aussen_hyp).sum::<f32>() + luecke * n.saturating_sub(1) as f32;
                g = g.max(s);
            }
            crate::gitter::klemmen(g, l.min_h, l.max_h)
        }
    };

    // §9.7: die flexiblen Laengen je Zeile.
    for z in &zeilen {
        flexen(&mut el[z.clone()], haupt_innen, luecke);
    }

    // §9.4 Schritt 7: die hypothetische Quergroesse folgt der BENUTZTEN
    // Hauptgroesse — bei einem ersetzten Element mit Seitenverhaeltnis und
    // `auto`-Quergroesse (nicht gedehnt) durch das Verhaeltnis, an
    // `min-*`/`max-*` der Querachse geklemmt. GEMESSEN vorher: WPT
    // `image-as-flexitem-size-006` (Spalte, `height: 30px` auf 40 px
    // geflext: 30 statt 40 px breit) und `-007` (Reihe, Rahmenkasten:
    // `min-width: 34px` gab 38 statt 32 px Hoehe).
    for e in el.iter_mut() {
        if e.anonym || !e.quer_auto {
            continue;
        }
        let Some(node) = ctx.document.get(e.id) else { continue };
        // Release 14.10.2026: also a non-replaced item with `aspect-ratio`
        // (in the `box-sizing` box; `auto && <ratio>` in the content box),
        // and also when it will be stretched (the line takes its size from
        // it). MEASURED before: WPT css-sizing
        // `aspect-ratio/flex-aspect-ratio-012` (`flex: 1 1 50px` grown to
        // 100 px stayed 0 px high), `-013`/`-014` (columns).
        let css = ctx.stil(e.id).map_or(0.0, |s| s.seitenverhaeltnis);
        let nicht_ersetzt = node.ersetzt.is_none() && css.is_finite() && css != 0.0;
        if e.dehnt() && !nicht_ersetzt {
            continue;
        }
        let r = if nicht_ersetzt {
            css.abs()
        } else {
            let Some(r) = ersetzt_verhaeltnis(ctx, e.id) else { continue };
            r
        };
        let quer_frei = if reihe {
            node.style.size.height == Length::Auto
        } else {
            node.style.size.width == Length::Auto && e.m.breite_wort == 0
        };
        if !quer_frei {
            continue;
        }
        let fuell_quer = if reihe { e.m.fuell.1 } else { e.m.fuell.0 };
        let q = if nicht_ersetzt && e.m.border_box && css > 0.0 {
            if reihe { e.ziel / r } else { e.ziel * r }
        } else {
            let innen = (e.ziel - e.fuell_haupt).max(0.0);
            (if reihe { innen / r } else { innen * r }) + fuell_quer
        };
        e.quer = crate::gitter::klemmen(q, e.quer_min, e.quer_max).max(fuell_quer);
    }

    // §9.4: Quergroessen der Zeilen (mit Grundlinien in einer Reihe).
    let mut zeilen_quer: Vec<f32> = Vec::with_capacity(zeilen.len());
    for z in &zeilen {
        let mut groesste = 0.0f32;
        // Die groessten Abstaende ueber und unter der Grundlinie; ein
        // negativer Rest (Grundlinie ausserhalb des Kastens) zaehlt mit
        // (§9.4 Schritt 8). GEMESSEN vorher: WPT
        // `baseline-outside-flex-item` (Zeile 212.8 statt 100 px).
        let (mut auf, mut ab) = (f32::MIN, f32::MIN);
        let (mut auf_l, mut ab_l) = (f32::MIN, f32::MIN);
        for e in el[z.clone()].iter_mut() {
            let quer = crate::gitter::klemmen(e.quer, e.quer_min, e.quer_max);
            e.quer = quer;
            let aussen = quer + e.q0 + e.q1;
            let grund = reihe && !e.qa0 && !e.qa1 && matches!(e.ausr, AusrLage::Grundlinie | AusrLage::LetzteGrundlinie);
            if grund && !e.anonym {
                let letzte = e.ausr == AusrLage::LetzteGrundlinie;
                let b = crate::gitter::grundlinie(ctx, sizes, e.id, letzte, 0);
                if letzte {
                    let von_ende = e.q1 + b.map_or(0.0, |b| (quer - b).max(0.0));
                    e.grund = von_ende;
                    ab_l = ab_l.max(von_ende);
                    auf_l = auf_l.max(aussen - von_ende);
                } else {
                    let von_anfang = e.q0 + b.unwrap_or(quer);
                    e.grund = von_anfang;
                    auf = auf.max(von_anfang);
                    ab = ab.max(aussen - von_anfang);
                }
            } else {
                groesste = groesste.max(aussen);
            }
        }
        let paar = |a: f32, b: f32| if a > f32::MIN && b > f32::MIN { a + b } else { 0.0 };
        zeilen_quer.push(groesste.max(paar(auf, ab)).max(paar(auf_l, ab_l)));
    }
    if !fs.umbruch && zeilen_quer.len() == 1 {
        match quer_def {
            Some(q) => zeilen_quer[0] = q,
            None => zeilen_quer[0] = crate::gitter::klemmen(zeilen_quer[0], l.min_h, l.max_h),
        }
    }
    let n_z = zeilen.len();
    let summe_z: f32 = zeilen_quer.iter().sum::<f32>() + luecke_quer * n_z.saturating_sub(1) as f32;
    let quer_innen = match quer_def {
        Some(q) => q,
        None => crate::gitter::klemmen(summe_z, l.min_h, l.max_h),
    };

    // `align-content` (nur mehrzeilig, §9.4 Schritt 15).
    let mut zeilen_lage: Vec<f32> = vec![0.0; n_z];
    {
        let frei = quer_innen - summe_z;
        let (mut pos, mut extra) = (0.0f32, 0.0f32);
        if fs.umbruch {
            match fs.align_content.lage {
                AusrLage::Normal | AusrLage::Strecken => {
                    if frei > 0.0 && n_z > 0 {
                        let je = frei / n_z as f32;
                        for q in zeilen_quer.iter_mut() {
                            *q += je;
                        }
                    }
                }
                _ => {
                    let a = schreib_bezug(flex_inhalt(fs.align_content, false), fs.umbruch_umgekehrt);
                    let (p, e) = verteilen_sicher(a, frei, n_z, false, fs.umbruch_umgekehrt);
                    pos = p;
                    extra = e;
                }
            }
        }
        // In f64 aufsummiert (wie die Hauptachse, WPT
        // `align-content-rounding`, `align-content-stretch-rounding`).
        let mut p64 = f64::from(pos);
        for (i, z) in zeilen_lage.iter_mut().enumerate() {
            *z = p64 as f32;
            p64 += f64::from(zeilen_quer[i]) + f64::from(luecke_quer) + f64::from(extra);
        }
    }

    // §9.5/§9.6: Lage auf der Haupt- und der Querachse.
    let mut kaesten = vec![Rect::new(0.0, 0.0, 0.0, 0.0); el.len()];
    let mut gedehnt = vec![false; el.len()];
    let jc = schreib_bezug(flex_inhalt(fs.justify_content, reihe), fs.umgekehrt);
    for (zi, z) in zeilen.iter().enumerate() {
        let n = z.len();
        let linie = &mut el[z.clone()];
        let belegt: f32 = linie.iter().map(El::aussen_ziel).sum::<f32>() + luecke * n.saturating_sub(1) as f32;
        let mut frei = haupt_innen - belegt;
        let autos = linie.iter().map(|e| usize::from(e.a0) + usize::from(e.a1)).sum::<usize>();
        let je_auto = if frei > 0.0 && autos > 0 {
            let v = frei / autos as f32;
            frei = 0.0;
            v
        } else {
            0.0
        };
        let (pos, extra) = if autos > 0 && je_auto > 0.0 {
            (0.0, 0.0)
        } else {
            verteilen_sicher(jc, frei, n, spiegel_haupt, fs.umgekehrt)
        };
        // Release 11.10.2026: die Lagen in f64 aufsummieren — in f32 lag das
        // letzte Element bei `space-between` um Rundungsreste neben dem
        // Ende (WPT `justify-content-rounding`, `main-axis-margin-rounding`:
        // 48.100002 statt 48.099998).
        let (mut pos, extra, je_auto, luecke_h) = (f64::from(pos), f64::from(extra), f64::from(je_auto), f64::from(luecke));
        let zq = zeilen_quer[zi];
        // Die gemeinsamen Grundlinien der Zeile.
        let auf = linie.iter().filter(|e| e.ausr == AusrLage::Grundlinie && reihe && !e.qa0 && !e.qa1 && !e.anonym).map(|e| e.grund).fold(f32::MIN, f32::max);
        let ab_l = linie.iter().filter(|e| e.ausr == AusrLage::LetzteGrundlinie && reihe && !e.qa0 && !e.qa1 && !e.anonym).map(|e| e.grund).fold(f32::MIN, f32::max);
        for (k, e) in linie.iter_mut().enumerate() {
            if e.a0 {
                pos += je_auto;
            }
            pos += f64::from(e.r0);
            let haupt_start = pos as f32;
            pos += f64::from(e.ziel) + f64::from(e.r1);
            if e.a1 {
                pos += je_auto;
            }
            pos += luecke_h + extra;
            // Querachse.
            let dehnt = e.dehnt();
            let quer = if dehnt {
                crate::gitter::klemmen(zq - e.q0 - e.q1, e.quer_min, e.quer_max).max(if reihe { e.m.fuell.1 } else { e.m.fuell.0 })
            } else {
                e.quer
            };
            let aussen = quer + e.q0 + e.q1;
            let platz = zq - aussen;
            let versatz = if e.qa0 || e.qa1 {
                if platz > 0.0 {
                    match (e.qa0, e.qa1) {
                        (true, true) => platz / 2.0,
                        (true, false) => platz,
                        _ => 0.0,
                    }
                } else {
                    0.0
                }
            } else if dehnt {
                0.0
            } else {
                let v = match e.ausr {
                    AusrLage::Ende | AusrLage::Rechts => platz,
                    AusrLage::Mitte => platz / 2.0,
                    AusrLage::Grundlinie if reihe && !e.anonym => auf - e.grund,
                    AusrLage::LetzteGrundlinie if reihe && !e.anonym => platz - (ab_l - e.grund),
                    _ => 0.0,
                };
                if e.sicher == Some(true) && platz < 0.0 {
                    if fs.umbruch_umgekehrt { platz } else { 0.0 }
                } else {
                    v
                }
            };
            let quer_start = zeilen_lage[zi] + versatz + e.q0;
            let haupt_phys = if spiegel_haupt { haupt_innen - haupt_start - e.ziel } else { haupt_start };
            let quer_phys = if spiegel_quer { quer_innen - quer_start - quer } else { quer_start };
            let i = z.start + k;
            kaesten[i] = if reihe {
                Rect::new(haupt_phys, quer_phys, e.ziel.max(0.0), quer.max(0.0))
            } else {
                Rect::new(quer_phys, haupt_phys, quer.max(0.0), e.ziel.max(0.0))
            };
            gedehnt[i] = dehnt;
        }
    }
    Ok(Ergebnis {
        breite: l.breite,
        hoehe: l.hoehe,
        kinder,
        basen: el.iter().map(|e| Basis { basis: e.basis, inhalt: e.inhalt, hyp_aussen: e.aussen_hyp() }).collect(),
        kaesten,
        gedehnt,
        inhalt_hoehe: if reihe { quer_innen } else { haupt_innen },
        stufe: 0,
    })
}

/// Release 11.10.2026: der freie Platz vor dem ersten Element bzw. der
/// ersten Zeile; ein `safe`, das greift (Ueberlauf), richtet am Anfang der
/// SCHREIBRICHTUNG aus (CSS Box Alignment 3 §4.4) — bei gespiegelter Achse
/// (`*-reverse` bzw. `wrap-reverse`) ist das das Flex-Ende. GEMESSEN vorher: WPT
/// `flexbox-safe-overflow-position-002/005`.
fn verteilen_sicher(a: Ausr, frei: f32, n: usize, umgekehrt: bool, gespiegelt: bool) -> (f32, f32) {
    let (p, e) = crate::gitter::inhalt_verteilen(a, frei, n, umgekehrt);
    let verteilung = matches!(a.lage, AusrLage::Zwischen | AusrLage::Ringsum | AusrLage::Gleichmaessig);
    let sicher = a.sicher == Some(true) || (verteilung && a.sicher.is_none());
    if gespiegelt && sicher && frei < 0.0 { (frei, e) } else { (p, e) }
}

/// Release 11.10.2026: `flex-wrap: balance` (CSS Flexbox 2): dieselbe
/// Zahl Zeilen wie beim gierigen Umbruch, aber die Aufteilung mit der
/// kleinsten Summe der quadrierten Restplaetze; bei Gleichstand bekommt die
/// erste Zeile die meisten Elemente, dann die zweite usw. GEMESSEN vorher:
/// WPT `css-flexbox/balance/*` (der Wert galt nicht).
fn ausgleichen(el: &[El], zeilen: usize, platz: f32, luecke: f32) -> Vec<std::ops::Range<usize>> {
    let n = el.len();
    let breite = |a: usize, b: usize| -> f32 { el[a..b].iter().map(El::aussen_hyp).sum::<f32>() + luecke * (b - a).saturating_sub(1) as f32 };
    // kosten[k][i]: kleinster Fehler fuer die Elemente ab i in k Zeilen.
    let mut kosten = vec![vec![f32::INFINITY; n + 1]; zeilen + 1];
    let mut wahl = vec![vec![usize::MAX; n + 1]; zeilen + 1];
    kosten[0][n] = 0.0;
    for k in 1..=zeilen {
        for i in (0..n).rev() {
            // Die laengste Zeile zuerst: bei Gleichstand gewinnt sie.
            for j in (i + 1..=n).rev() {
                let b = breite(i, j);
                if j > i + 1 && b > platz + 0.01 {
                    continue;
                }
                let rest = kosten[k - 1][j];
                if !rest.is_finite() {
                    continue;
                }
                let frei = platz - b;
                let c = frei * frei + rest;
                if c < kosten[k][i] - 0.001 {
                    kosten[k][i] = c;
                    wahl[k][i] = j;
                }
            }
        }
    }
    if !kosten[zeilen][0].is_finite() {
        return vec![0..n];
    }
    let mut raus = Vec::with_capacity(zeilen);
    let mut i = 0usize;
    for k in (1..=zeilen).rev() {
        let j = wahl[k][i];
        if j == usize::MAX {
            break;
        }
        raus.push(i..j);
        i = j;
    }
    if i < n {
        raus.push(i..n);
    }
    raus
}

/// Release 11.10.2026: `start`/`end` beziehen sich auf die Schreibrichtung,
/// `flex-start`/`flex-end` auf die Flex-Achse; laeuft die Achse umgekehrt
/// (`*-reverse` bzw. `wrap-reverse`), tauschen `start` und `end` in
/// Flex-Koordinaten. GEMESSEN vorher: WPT
/// `flex-abspos-staticpos-align-self-006` (`align-self: start` in
/// `column wrap-reverse` stand rechts).
fn schreib_bezug(a: Ausr, umgekehrt: bool) -> Ausr {
    if !umgekehrt || a.flex_bezug {
        return a;
    }
    let tausch = |l: AusrLage| match l {
        AusrLage::Anfang => AusrLage::Ende,
        AusrLage::Ende => AusrLage::Anfang,
        l => l,
    };
    Ausr { lage: tausch(a.lage), ausweich: a.ausweich.map(tausch), ..a }
}

/// Release 11.10.2026: `self-start`/`self-end` auf der Inline-Achse eines
/// Elements mit anderer Schreibrichtung als der Container tauschen.
/// GEMESSEN vorher: WPT `flex-abspos-staticpos-align-self-rtl-004`.
fn selbst_richtung(a: Ausr, element_rtl: bool, container_rtl: bool, inline_achse: bool) -> Ausr {
    if !a.selbst_bezug || !inline_achse || element_rtl == container_rtl {
        return a;
    }
    let lage = match a.lage {
        AusrLage::Anfang => AusrLage::Ende,
        AusrLage::Ende => AusrLage::Anfang,
        l => l,
    };
    Ausr { lage, ..a }
}

/// `justify-content`/`align-content` im Flex-Container (CSS Box Alignment
/// 3 §5.1): `normal`/`stretch` sind `flex-start`; auf der Spaltenachse sind
/// `left`/`right` der Anfang.
fn flex_inhalt(a: Ausr, waagerecht: bool) -> Ausr {
    match a.lage {
        // `normal`/`stretch` sind `flex-start` (Flex-Achse).
        AusrLage::Normal | AusrLage::Strecken | AusrLage::Grundlinie | AusrLage::LetzteGrundlinie | AusrLage::Auto => {
            Ausr { lage: AusrLage::Anfang, flex_bezug: true, ..a }
        }
        AusrLage::Links | AusrLage::Rechts if !waagerecht => Ausr { lage: AusrLage::Anfang, ..a },
        _ => a,
    }
}

/// §9.7: die flexiblen Laengen einer Zeile aufloesen (mit Einfrieren der
/// Elemente, die an `min`/`max` stossen).
fn flexen(linie: &mut [El], innen: f32, luecke: f32) {
    let n = linie.len();
    if n == 0 {
        return;
    }
    let luecken = luecke * (n - 1) as f32;
    let summe_hyp: f32 = linie.iter().map(El::aussen_hyp).sum::<f32>() + luecken;
    let wachsen = summe_hyp < innen;
    // Schritt 2: unflexible Elemente einfrieren.
    for e in linie.iter_mut() {
        e.ziel = e.hyp;
        let faktor = if wachsen { e.wachsen } else { e.schrumpfen };
        e.gefroren = e.anonym || faktor <= 0.0 || (wachsen && e.basis > e.hyp) || (!wachsen && e.basis < e.hyp);
    }
    let frei_von = |linie: &[El]| -> f32 {
        innen - luecken - linie.iter().map(|e| if e.gefroren { e.ziel } else { e.basis } + e.r0 + e.r1).sum::<f32>()
    };
    let anfang = frei_von(linie);
    for _ in 0..(n + 2) {
        if linie.iter().all(|e| e.gefroren) {
            break;
        }
        let mut frei = frei_von(linie);
        let faktoren: f32 = linie.iter().filter(|e| !e.gefroren).map(|e| if wachsen { e.wachsen } else { e.schrumpfen }).sum();
        if faktoren < 1.0 {
            let v = anfang * faktoren;
            if v.abs() < frei.abs() {
                frei = v;
            }
        }
        if frei.is_finite() && frei != 0.0 {
            if wachsen {
                if faktoren > 0.0 {
                    for e in linie.iter_mut().filter(|e| !e.gefroren) {
                        e.ziel = e.basis + frei * e.wachsen / faktoren;
                    }
                }
            } else {
                let skaliert: f32 = linie.iter().filter(|e| !e.gefroren).map(|e| e.schrumpfen * (e.basis - e.fuell_haupt).max(0.0)).sum();
                for e in linie.iter_mut().filter(|e| !e.gefroren) {
                    e.ziel = if skaliert > 0.0 {
                        e.basis + frei * e.schrumpfen * (e.basis - e.fuell_haupt).max(0.0) / skaliert
                    } else {
                        e.basis
                    };
                }
            }
        } else {
            for e in linie.iter_mut().filter(|e| !e.gefroren) {
                e.ziel = e.basis;
            }
        }
        // Schritt 4d/e: an min/max klemmen und nach der Summe der
        // Verletzungen einfrieren.
        let mut summe = 0.0f32;
        let mut verletzt: Vec<i8> = vec![0; n];
        for (i, e) in linie.iter_mut().enumerate() {
            if e.gefroren {
                continue;
            }
            let geklemmt = e.ziel.min(e.max).max(e.min);
            if geklemmt > e.ziel + 0.001 {
                verletzt[i] = -1;
            } else if geklemmt < e.ziel - 0.001 {
                verletzt[i] = 1;
            }
            summe += geklemmt - e.ziel;
            e.ziel = geklemmt;
        }
        if summe.abs() < 0.001 {
            for e in linie.iter_mut() {
                e.gefroren = true;
            }
        } else if summe > 0.0 {
            for (i, e) in linie.iter_mut().enumerate() {
                if verletzt[i] == -1 {
                    e.gefroren = true;
                }
            }
        } else {
            for (i, e) in linie.iter_mut().enumerate() {
                if verletzt[i] == 1 {
                    e.gefroren = true;
                }
            }
        }
    }
    for e in linie.iter_mut() {
        if !e.ziel.is_finite() {
            e.ziel = e.hyp;
        }
        e.ziel = e.ziel.max(0.0);
    }
}

/// Ist ein Element von seiner Hoehe abhaengig (Flex/Raster oder Kinder mit
/// Prozenthoehe)? Nur solche werden nach dem Dehnen nachgemessen.
fn haengt_an_hoehe(ctx: &Ctx<'_>, id: NodeId) -> bool {
    let Some(s) = ctx.stil(id) else { return false };
    // Release 11.10.2026: auch eine Tabelle — ihre Zeilen verteilen die
    // gedehnte Hoehe (CSS 2.1 §17.5.3). GEMESSEN vorher: eine gedehnte
    // Tabelle in einer Flex-Reihe blieb 0 px hoch (WPT
    // `table-as-item-specified-width-vertical`).
    if matches!(s.anzeige, narsil_layout::plan::Anzeige::Flex | narsil_layout::plan::Anzeige::Raster | narsil_layout::plan::Anzeige::Tabelle) {
        return true;
    }
    let Some(n) = ctx.document.get(id) else { return false };
    n.children.iter().take(crate::plan::MAX_KINDER).any(|&c| {
        ctx.document.get(c).is_some_and(|k| matches!(k.style.size.height, Length::Percent(_)))
            || ctx.stil(c).is_some_and(|s| s.hoehe_anteil.is_some() || s.hoehe_bedingt.is_some())
            || ctx.plan.is_some_and(|p| {
                let g = p.grenzen_prozent(c);
                g.min_hoehe.is_some() || g.max_hoehe.is_some()
            })
    })
}

/// Die Messphase eines Flex-Containers (`box.rs`, `SizeTask::Format`).
///
/// Reihe: Stufe 0 bestimmt die Hauptgroessen (Breiten) und laesst die
/// Elemente damit nachmessen; Stufe 1 kennt die Hoehen, dehnt auf die
/// Zeilenhoehe und misst hoehenabhaengige Elemente nach; Stufe 2 legt ab.
/// Spalte: Stufe 0 bestimmt die Querbreiten, Stufe 1 die Basis aus den
/// Hoehen und die geflexten Hoehen, Stufe 2 legt ab.
#[allow(clippy::too_many_arguments)]
pub(crate) fn messen(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    fs: &FlexStil,
    sizes: &Sizes,
    breite: f32,
    hoehe: Option<f32>,
    min_h: Option<f32>,
    max_h: Option<f32>,
    nachmessung: bool,
) -> Result<Messung, LayoutError> {
    let alt = ctx.flex.borrow().get(&node.id).cloned();
    let stufe = if nachmessung {
        alt.as_ref().filter(|e| (e.breite - breite).abs() < 0.5).map_or(0, |e| e.stufe)
    } else {
        0
    };
    let behaelter = Size::new(breite, hoehe.unwrap_or(0.0));
    let basen_alt = alt.as_ref().filter(|_| stufe > 0).map(|e| e.basen.clone());
    let lauf0 = Lauf {
        ctx,
        node,
        fs,
        sizes,
        breite,
        hoehe,
        min_h,
        max_h,
        basen: None,
        quer_wunsch: false,
        probe: false,
    };
    // Die Groesse eines Elements fuer die Nachmessung setzen; `true`, wenn
    // es nachgemessen werden muss.
    let setzen = |id: NodeId, w: Option<f32>, h: Option<f32>| -> bool {
        let vorher = ctx.flex_mass.borrow_mut().insert(id, (w, h));
        let g = sizes.get(id).unwrap_or(Size::new(0.0, 0.0));
        let anders_b = w.is_some_and(|w| (w - g.width).abs() > 0.5);
        // Eine neu gesetzte Hoehe macht sie definit (§9.8.1): auch bei
        // gleicher Groesse nachmessen, damit Prozente darin gelten.
        let neu_h = |h: f32| vorher.is_none_or(|(_, vh)| vh.is_none_or(|vh| (vh - h).abs() > 0.5));
        let anders_h = h.is_some_and(|h| (h - g.height).abs() > 0.5 || neu_h(h));
        let war_h = vorher.is_some_and(|(_, vh)| vh.is_some()) && h.is_none();
        anders_b || anders_h || war_h
    };
    if fs.reihe {
        match stufe {
            0 => {
                let mut e = rechnen(&lauf0)?;
                // Release 11.10.2026: CSS Flexbox 1 §9.8 Punkt 1 — in einer
                // einzeiligen Reihe mit definiter Hoehe ist die gedehnte
                // Hoehe eines Elements schon VOR den Hauptgroessen definit:
                // hoehenabhaengige Elemente (Prozenthoehen darin) werden
                // zuerst damit gemessen, damit ihre Inhaltsbreite stimmt.
                // GEMESSEN vorher: WPT `flex-basis-013` (ein 5 x 5-Canvas mit
                // `height: 100%` gab dem Element 5 statt 50 px Breite).
                let vorab = |d: bool, id: NodeId| !fs.umbruch && hoehe.is_some() && d && !crate::gitter::ist_anonym(ctx, id) && haengt_an_hoehe(ctx, id);
                let mut vor = Vec::new();
                for (((_, id), k), d) in e.kinder.iter().zip(&e.kaesten).zip(&e.gedehnt) {
                    if !vorab(*d, *id) {
                        continue;
                    }
                    let gesetzt = ctx.flex_mass.borrow().get(id).and_then(|m| m.1).is_some_and(|h| (h - k.height).abs() < 0.5)
                        && ctx.hoehe_definit.borrow().contains(id);
                    if !gesetzt {
                        ctx.flex_mass.borrow_mut().insert(*id, (None, Some(k.height)));
                        ctx.hoehe_definit.borrow_mut().insert(*id);
                        vor.push((*id, behaelter));
                    }
                }
                if !vor.is_empty() {
                    e.stufe = 0;
                    ctx.flex.borrow_mut().insert(node.id, e);
                    return Ok(Messung::Nachmessen(vor));
                }
                let mut nach = Vec::new();
                for (((_, id), k), d) in e.kinder.iter().zip(&e.kaesten).zip(&e.gedehnt) {
                    if crate::gitter::ist_anonym(ctx, *id) {
                        continue;
                    }
                    let h = vorab(*d, *id).then_some(k.height);
                    if setzen(*id, Some(k.width), h) {
                        nach.push((*id, behaelter));
                    }
                }
                e.stufe = 1;
                ctx.flex.borrow_mut().insert(node.id, e);
                if !nach.is_empty() {
                    return Ok(Messung::Nachmessen(nach));
                }
                messen(ctx, node, fs, sizes, breite, hoehe, min_h, max_h, true)
            }
            1 => {
                let mut e = rechnen(&Lauf { basen: basen_alt.as_deref(), ..lauf0 })?;
                let mut nach = Vec::new();
                for (((_, id), k), d) in e.kinder.iter().zip(&e.kaesten).zip(&e.gedehnt) {
                    if !*d || crate::gitter::ist_anonym(ctx, *id) || !haengt_an_hoehe(ctx, *id) {
                        continue;
                    }
                    // Gedehnt ist die Hoehe definit (§9.4 Schritt 11).
                    ctx.hoehe_definit.borrow_mut().insert(*id);
                    if setzen(*id, Some(k.width), Some(k.height)) {
                        nach.push((*id, behaelter));
                    }
                }
                e.stufe = 2;
                ctx.flex.borrow_mut().insert(node.id, e);
                if !nach.is_empty() {
                    return Ok(Messung::Nachmessen(nach));
                }
                messen(ctx, node, fs, sizes, breite, hoehe, min_h, max_h, true)
            }
            _ => {
                let mut e = rechnen(&Lauf { basen: basen_alt.as_deref(), ..lauf0 })?;
                e.stufe = 3;
                let h = e.inhalt_hoehe;
                ctx.flex.borrow_mut().insert(node.id, e);
                Ok(Messung::Fertig(h))
            }
        }
    } else {
        match stufe {
            0 => {
                let mut e = rechnen(&Lauf { quer_wunsch: true, ..lauf0 })?;
                let mut nach = Vec::new();
                for ((_, id), k) in e.kinder.iter().zip(&e.kaesten) {
                    if crate::gitter::ist_anonym(ctx, *id) {
                        continue;
                    }
                    if setzen(*id, Some(k.width), None) {
                        nach.push((*id, behaelter));
                    }
                }
                e.stufe = 1;
                e.basen.clear();
                ctx.flex.borrow_mut().insert(node.id, e);
                if !nach.is_empty() {
                    return Ok(Messung::Nachmessen(nach));
                }
                messen(ctx, node, fs, sizes, breite, hoehe, min_h, max_h, true)
            }
            1 => {
                let mut e = rechnen(&lauf0)?;
                let mut nach = Vec::new();
                for (((_, id), k), d) in e.kinder.iter().zip(&e.kaesten).zip(&e.gedehnt) {
                    if crate::gitter::ist_anonym(ctx, *id) {
                        continue;
                    }
                    // Mehrzeilig: die gedehnte Breite kommt erst jetzt; ein
                    // ersetztes Element mit Verhaeltnis folgt seiner Hoehe.
                    let w = if *d || fs.umbruch || ersetzt_verhaeltnis(ctx, *id).is_some() {
                        k.width
                    } else {
                        sizes.get(*id).map_or(k.width, |g| g.width)
                    };
                    // Die geflexte Hoehe ist definit, wenn es die des
                    // Containers ist (§9.8 Punkt 1).
                    // Release 14.10.2026: or when it comes from `aspect-ratio`
                    // and a definite width (stretched or fixed; CSS Sizing 4
                    // §5.1). MEASURED before: WPT css-sizing
                    // `aspect-ratio/flex-aspect-ratio-032` (`height: 100%` in
                    // a 100 x 100 ratio item stayed 0).
                    let durch_verhaeltnis = ctx.stil(*id).is_some_and(|s| s.seitenverhaeltnis.is_finite() && s.seitenverhaeltnis != 0.0)
                        && ctx.document.get(*id).is_some_and(|n| {
                            n.ersetzt.is_none() && n.style.size.height == Length::Auto && (*d || matches!(n.style.size.width, Length::Px(_)))
                        });
                    if hoehe.is_some() || durch_verhaeltnis {
                        ctx.hoehe_definit.borrow_mut().insert(*id);
                    } else {
                        ctx.hoehe_definit.borrow_mut().remove(id);
                    }
                    if setzen(*id, Some(w), Some(k.height)) {
                        nach.push((*id, behaelter));
                    }
                }
                e.stufe = 2;
                ctx.flex.borrow_mut().insert(node.id, e);
                if !nach.is_empty() {
                    return Ok(Messung::Nachmessen(nach));
                }
                messen(ctx, node, fs, sizes, breite, hoehe, min_h, max_h, true)
            }
            _ => {
                let mut e = rechnen(&Lauf { basen: basen_alt.as_deref(), ..lauf0 })?;
                e.stufe = 3;
                let h = e.inhalt_hoehe;
                ctx.flex.borrow_mut().insert(node.id, e);
                Ok(Messung::Fertig(h))
            }
        }
    }
}

/// Release 11.10.2026: die min-/max-content-Breite eines Flex-Containers
/// (Rahmen; CSS Flexbox 1 §9.9.1): in einer Reihe die Summe der Beitraege
/// der Elemente (mehrzeilig fuer min-content der groesste), in einer Spalte
/// der groesste — die Beitraege mit Rand und an `min-width`/`max-width`
/// geklemmt. GEMESSEN vorher: WPT css/cssom-view
/// `scrollWidthHeight-overflow-visible-margin-collapsing` (`inline-flex`:
/// leere Elemente mit `min-width: 20px` zaehlten mit 0 px, der Container
/// war schmaler als ihre Randkaesten).
pub(crate) fn intrinsisch(sizes: &Sizes, ctx: &Ctx<'_>, id: NodeId, max: bool, tiefe: usize) -> Option<f32> {
    if ctx.document.get(id).is_some_and(|n| matches!(n.style.size.width, Length::Px(_))) {
        return None;
    }
    intrinsisch_inhalt(sizes, ctx, id, max, tiefe)
}

/// Release 11.10.2026: wie [`intrinsisch`], aber auch bei fester eigener
/// Breite — die Inhaltsgroesse fuer `min-width: min-content` usw. (CSS
/// Sizing 3 §5.1).
pub(crate) fn intrinsisch_inhalt(sizes: &Sizes, ctx: &Ctx<'_>, id: NodeId, max: bool, tiefe: usize) -> Option<f32> {
    let st = ctx.stil(id)?;
    let fs = st.flex.as_deref()?;
    if st.anzeige != narsil_layout::plan::Anzeige::Flex || tiefe > 48 {
        return None;
    }
    let node = ctx.document.get(id)?;
    let (kinder, _) = crate::gitter::rasterelemente(ctx, node);
    let gemessen = sizes.get(id).map_or(0.0, |s| s.width);
    let fuell = crate::box_sizing::resolve_spacing(node.style.padding, gemessen, id, "padding").unwrap_or(narsil_core::geometry::Edges::ZERO);
    let rahmen = crate::r#box::rahmen_edges(&node.style.rahmen);
    let aussen = fuell.left + fuell.right + rahmen.left + rahmen.right;
    let fv = fuell.top + fuell.bottom + rahmen.top + rahmen.bottom;
    // Die definite Inhaltshoehe (Angabe; eine Prozenthoehe: die gemessene).
    let hoehe_def = st
        .hoehe_angabe
        .map(|h| if st.border_box { (h - fv).max(0.0) } else { h.max(0.0) })
        .or_else(|| st.hoehe_anteil.and(sizes.get(id).map(|g| (g.height - fv).max(0.0))).filter(|h| *h > 0.0));
    let mut summe = 0.0f32;
    let mut groesste = 0.0f32;
    for (_, k) in &kinder {
        let Ok(m) = crate::gitter::masse(ctx, *k, 0.0, 0.0) else { continue };
        let b = crate::gitter::spalten_beitrag(ctx, sizes, *k, &m, tiefe + 1);
        let v = if max { b.max } else { b.min };
        // Eine Mindestbreite gilt auch fuer den Beitrag eines Elements ohne
        // Inhalt (die Beitraege sind schon geklemmt, das Minimum zaehlt mit).
        let mut v = v.max(if m.min_b.is_some() { b.minimum } else { 0.0 });
        // Release 11.10.2026: ein Element mit Seitenverhaeltnis (`aspect-ratio`
        // oder ersetzt) und definiter bzw. gedehnter Hoehe traegt in einer
        // Reihe die uebertragene Breite bei (CSS Sizing 4 §5). GEMESSEN
        // vorher: WPT `aspect-ratio-intrinsic-size-003/008/010`.
        if fs.reihe
            && let Some(kn) = ctx.document.get(*k)
            && m.breite_auto
            && !m.breite_prozent
            && m.breite_wort == 0
        {
            let ks = ctx.stil(*k);
            // Das wirksame Verhaeltnis; die Angabe zaehlt fuer `box-sizing`
            // nur, wenn nicht das natuerliche eines ersetzten Elements gilt.
            let roh = ks.map_or(0.0, |s| s.seitenverhaeltnis);
            let css_r = if kn.ersetzt.is_some() && roh <= 0.0 { 0.0 } else { roh.abs() };
            let r = crate::r#box::wirksames_verhaeltnis(ctx, *k, kn);
            let selbst = crate::gitter::selbst_aufloesen(ks.and_then(|s| s.gitter_kind.as_deref()).map_or(Ausr::AUTO, |g| g.align_self), fs.align_items);
            let dehnt = matches!(selbst.lage, AusrLage::Normal | AusrLage::Strecken) && !m.rand_auto[1] && !m.rand_auto[3];
            let eigen_h = match kn.style.size.height {
                Length::Px(h) if h.is_finite() => Some(if m.border_box { h } else { h + m.fuell.1 }),
                _ => None,
            };
            let rand_v = m.rand.top + m.rand.bottom;
            let hoehe_kasten = eigen_h.or_else(|| if dehnt && m.hoehe_auto { hoehe_def.map(|h| (h - rand_v).max(0.0)) } else { None });
            if let (Some(r), Some(hb)) = (r, hoehe_kasten) {
                let (fh, fvk) = m.fuell;
                let breite = if m.border_box && css_r > 0.0 { hb * r } else { (hb - fvk).max(0.0) * r + fh };
                v = breite + m.rand.left + m.rand.right;
            }
        }
        summe += v;
        groesste = groesste.max(v);
    }
    let n = kinder.len();
    // Eine mehrzeilige Spalte mit definiter Hoehe: die Breite ihrer Zeilen
    // nebeneinander (die Zeilen aus der Probe mit max-content-Breiten).
    // GEMESSEN vorher: WPT `intrinsic-size/col-wrap-*` (die breiteste
    // Spalte statt aller).
    if !fs.reihe && fs.umbruch && max {
        // Eine Prozenthoehe: die gemessene (WPT `balance/balance-002`).
        if let Some(h) = hoehe_def {
            let mut probe = fs.clone();
            probe.align_content = Ausr { lage: AusrLage::Anfang, ..Ausr::NORMAL };
            probe.rtl = false;
            probe.umbruch_umgekehrt = false;
            let lauf = Lauf {
                ctx,
                node,
                fs: &probe,
                sizes,
                breite: 1.0e6,
                hoehe: Some(h),
                min_h: None,
                max_h: None,
                basen: None,
                quer_wunsch: true,
                probe: true,
            };
            if let Ok(e) = rechnen(&lauf) {
                // Die Zeilen (gleiche Lage quer) mit dem groessten
                // max-content-Beitrag je Zeile; Prozentbreiten sind dabei
                // zyklisch (Beitrag ohne sie). GEMESSEN: WPT `col-wrap-004`.
                let mut zeilen: Vec<(f32, f32)> = Vec::new();
                for ((_, k), r) in e.kinder.iter().zip(&e.kaesten) {
                    let Ok(m) = crate::gitter::masse(ctx, *k, 0.0, 0.0) else { continue };
                    let b = crate::gitter::spalten_beitrag(ctx, sizes, *k, &m, tiefe + 1).max;
                    let x = r.x - m.rand.left;
                    match zeilen.iter_mut().find(|z| (z.0 - x).abs() < 0.5) {
                        Some(z) => z.1 = z.1.max(b),
                        None => zeilen.push((x, b)),
                    }
                }
                let luecke = crate::gitter::luecke_px(fs.luecke_spalte, None, 0.0);
                let n = zeilen.len();
                return Some(zeilen.iter().map(|z| z.1).sum::<f32>() + luecke * n.saturating_sub(1) as f32 + aussen);
            }
        }
    }
    let w = if fs.reihe {
        if !max && fs.umbruch {
            groesste
        } else {
            let luecke = crate::gitter::luecke_px(fs.luecke_spalte, None, 0.0);
            summe + luecke * n.saturating_sub(1) as f32
        }
    } else {
        groesste
    };
    Some(w + aussen)
}

/// Die Platzierung (`flow.rs`): die Kinder eines Flex-Containers, Index =
/// Kindindex; absolut positionierte Kinder bekommen ihre statische Lage
/// (§4.1: als einziges Flex-Element im Container).
pub(crate) fn verteilen(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    content: &Rect,
    stil: &narsil_layout::LayoutStil,
    fs: &FlexStil,
    sizes: &Sizes,
) -> Result<Vec<Platzierung>, LayoutError> {
    let n = node.children.len().min(crate::plan::MAX_KINDER);
    let gemerkt = ctx.flex.borrow().get(&node.id).cloned();
    let e = match gemerkt {
        // Dieselbe Breite und Hoehe wie in der Messphase: das Ergebnis gilt.
        Some(e)
            if (e.breite - content.width).abs() < 0.5
                && e.stufe >= 3
                && (!content.height.is_finite() || (e.inhalt_hoehe - content.height).abs() < 0.5) =>
        {
            e
        }
        alt => {
            let h = content.height.is_finite().then_some(content.height.max(0.0));
            let basen = alt.as_ref().filter(|a| a.stufe >= 2 && !a.basen.is_empty()).map(|a| a.basen.clone());
            let lauf = Lauf {
                ctx,
                node,
                fs,
                sizes,
                breite: content.width.max(0.0),
                hoehe: h,
                min_h: None,
                max_h: None,
                basen: basen.as_deref(),
                quer_wunsch: false,
                probe: false,
            };
            let mut e = rechnen(&lauf)?;
            e.stufe = 3;
            e
        }
    };
    let mut alle = vec![Platzierung::WEG; n];
    for ((index, id), k) in e.kinder.iter().zip(&e.kaesten) {
        let m = crate::gitter::masse(ctx, *id, content.width, content.height.max(0.0))?;
        if let Some(glieder) = &m.glieder {
            let mut y = content.y + k.y;
            for &g in glieder.iter() {
                let Some(gi) = node.children.iter().position(|c| *c == g) else { continue };
                let gm = sizes.get(g).unwrap_or(Size::new(0.0, 0.0));
                let text = ctx.document.get(g).is_some_and(|x| x.kind == NodeKind::Text);
                let gb = if text { gm.width } else { k.width };
                if gi < n {
                    alle[gi] = Platzierung { x: content.x + k.x, y, breite: gb, hoehe: gm.height };
                }
                y += gm.height;
            }
            continue;
        }
        if *index < n {
            alle[*index] = Platzierung {
                x: content.x + k.x - m.rand.left,
                y: content.y + k.y - m.rand.top,
                breite: k.width,
                hoehe: k.height,
            };
        }
    }
    // §4.1: die statische Lage absolut positionierter Kinder — die
    // Inhaltsbox, ausgerichtet nach `justify-content` und `align-self`.
    let (_, abspos) = crate::gitter::rasterelemente(ctx, node);
    if !abspos.is_empty() {
        let (spiegel_haupt, spiegel_quer) = spiegel(fs);
        let lage = |l: AusrLage, spiegel: bool| -> narsil_css::FlexAusrichtung {
            let l = match l {
                AusrLage::Zwischen => AusrLage::Anfang,
                AusrLage::Ringsum | AusrLage::Gleichmaessig => AusrLage::Mitte,
                l => l,
            };
            match (l, spiegel) {
                (AusrLage::Mitte, _) => narsil_css::FlexAusrichtung::Mitte,
                (AusrLage::Ende, false) | (AusrLage::Anfang | AusrLage::Normal | AusrLage::Strecken, true) => narsil_css::FlexAusrichtung::Ende,
                _ => narsil_css::FlexAusrichtung::Start,
            }
        };
        let haupt = schreib_bezug(flex_inhalt(fs.justify_content, fs.reihe), fs.umgekehrt);
        // `left`/`right` sind physisch.
        let (haupt_lage, haupt_spiegel) = match haupt.lage {
            AusrLage::Links => (AusrLage::Anfang, false),
            AusrLage::Rechts => (AusrLage::Ende, false),
            l => (l, spiegel_haupt),
        };
        for id in abspos {
            let ks = ctx.stil(id).and_then(|s| s.gitter_kind.as_deref());
            // Die Grundlinien fallen auf `start`/`end` der Schreibrichtung
            // zurueck (§8.3: `last baseline` -> `safe end`).
            let roh = crate::gitter::selbst_aufloesen(ks.map_or(Ausr::AUTO, |k| k.align_self), fs.align_items);
            let roh = match roh.lage {
                AusrLage::Grundlinie => Ausr { lage: AusrLage::Anfang, flex_bezug: false, ..roh },
                AusrLage::LetzteGrundlinie => Ausr { lage: AusrLage::Ende, flex_bezug: false, ..roh },
                _ => roh,
            };
            let selbst = schreib_bezug(roh, fs.umbruch_umgekehrt);
            let selbst = selbst_richtung(selbst, ctx.stil(id).is_some_and(|s| s.rtl), fs.rtl, !fs.reihe);
            let quer = match selbst.lage {
                AusrLage::Rechts | AusrLage::LetzteGrundlinie => AusrLage::Ende,
                AusrLage::Mitte | AusrLage::Ende => selbst.lage,
                _ => AusrLage::Anfang,
            };
            // `safe`: ein ueberstehender Kasten steht am Anfang (CSS Box
            // Alignment 3 §4.4). GEMESSEN vorher: WPT
            // `flex-abspos-staticpos-align-self-safe-001/002`.
            let g = sizes.get(id).unwrap_or(Size::new(0.0, 0.0));
            let m = crate::gitter::masse(ctx, id, content.width, content.height.max(0.0))?;
            let aussen_b = g.width + m.rand.left + m.rand.right;
            let aussen_h = g.height + m.rand.top + m.rand.bottom;
            let (haupt_platz, quer_platz, haupt_aussen, quer_aussen) = if fs.reihe {
                (content.width, content.height, aussen_b, aussen_h)
            } else {
                (content.height, content.width, aussen_h, aussen_b)
            };
            // Ein greifendes `safe` steht am Anfang der Schreibrichtung —
            // es greift, wenn der Kasten aus dem enthaltenden Block ragt;
            // der ist hier nur bekannt, wenn der Container selbst
            // positioniert ist (sonst ein aeusserer, vorlaeufig: WPT
            // `flex-abspos-align-self-safe-outer-cb-003`).
            let positioniert = stil.position != PositionArt::Statisch;
            let haupt_aussen = if positioniert { haupt_aussen } else { 0.0 };
            let quer_aussen = if positioniert { quer_aussen } else { 0.0 };
            let schreib_anfang = |rtl: bool| if rtl { narsil_css::FlexAusrichtung::Ende } else { narsil_css::FlexAusrichtung::Start };
            let h = if haupt.sicher == Some(true) && haupt_aussen > haupt_platz {
                schreib_anfang(fs.reihe && fs.rtl)
            } else {
                lage(haupt_lage, haupt_spiegel)
            };
            let q = if selbst.sicher == Some(true) && quer_aussen > quer_platz {
                schreib_anfang(!fs.reihe && fs.rtl)
            } else {
                lage(quer, spiegel_quer)
            };
            let (av, ah) = if fs.reihe { (q, h) } else { (h, q) };
            ctx.gitter_statisch.borrow_mut().insert(id, (*content, av, ah));
        }
    }
    ctx.flex.borrow_mut().insert(node.id, e);
    Ok(alle)
}
