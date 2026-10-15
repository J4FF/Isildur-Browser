//! Release 07.10.2026 — NACHUMBRUCH: Absätze mit der GEMESSENEN Breite umbrechen.
//!
//! # DER BEFUND
//!
//! `narsil-page` bricht jeden Absatz beim Bau in Zeilen um, mit einer
//! SCHÄTZUNG der Breite (`inhaltsbreite`: `width`, `padding`, `max-width`).
//! Grid-Spalten, Flex-Verteilung und Floats kennt der Bauer nicht.
//! GEMESSEN am 07.10. auf de./en.wikipedia.org (Fenster 1888 px): der
//! Artikeltext lief über die 960-px-Spalte bis hinter den Fensterrand und
//! unter die Infobox (`float: right`).
//!
//! # DER WEG
//!
//! Nach dem ersten Layout kennt jeder Absatz die Inhaltsbreite seines
//! Kastens und die Floats daneben. Genau die Absätze, deren Breite davon
//! abweicht, werden mit `narsil_page::absatz_neu_umbrechen` neu in Zeilen
//! gesetzt — ohne zweite Kaskade —, danach folgt ein zweites Layout.
//! Zeilen neben einem Float bekommen die Restbreite (rechts) bzw. einen
//! Versatz (links); Zeilen unterhalb wieder die volle Breite.

use std::collections::HashMap;

use narsil_core::geometry::{Rect, Size};
use narsil_dom::NodeId;
use narsil_layout::plan::{Anzeige, FloatArt};
use narsil_layout::{LayoutBox, LayoutTree};

/// Abweichung in px, ab der ein Absatz neu umbrochen wird.
const TOLERANZ: f32 = 1.5;

/// Höchstens so viele Absätze je Durchgang (M3).
const MAX_ABSAETZE: usize = 20_000;

/// Layout mit Nachumbruch: Layout, Absätze mit gemessener Breite neu
/// umbrechen, Layout. Ohne Abweichung bleibt es bei einem Layout.
pub fn layout(
    seite: &mut narsil_page::Seite,
    viewport: Size,
    messung: &narsil_layout::Messung,
) -> Result<LayoutTree, narsil_layout_engine::LayoutError> {
    let messer = narsil_layout_engine::MessungMesser::new(messung.clone());
    let baum =
        narsil_layout_engine::layout_mit_plan(&seite.document, viewport, &messer, &seite.layout_plan)?;
    if std::env::var_os("NARSIL_NACHUMBRUCH_AUS").is_some() || !umbrechen(seite, &baum, messung) {
        return Ok(baum);
    }
    narsil_layout_engine::layout_mit_plan(&seite.document, viewport, &messer, &seite.layout_plan)
}

/// Ein Float im ersten Layout: Rand-Kasten und Seite.
struct Schwimmer {
    knoten: NodeId,
    kasten: Rect,
    art: FloatArt,
}

fn sammeln<'a>(
    b: &'a LayoutBox,
    plan: &narsil_layout::LayoutPlan,
    karte: &mut HashMap<NodeId, &'a LayoutBox>,
    schwimmer: &mut Vec<Schwimmer>,
    tiefe: usize,
) {
    karte.entry(b.node).or_insert(b);
    if let Some(p) = plan.hole(b.node)
        && p.schweben != FloatArt::Keins
        && b.border_box.width > 0.0
        && b.border_box.height > 0.0
    {
        schwimmer.push(Schwimmer { knoten: b.node, kasten: b.border_box, art: p.schweben });
    }
    if tiefe > 4096 {
        return;
    }
    for c in &b.children {
        sammeln(c, plan, karte, schwimmer, tiefe + 1);
    }
}

fn px(l: narsil_core::length::Length, basis: f32) -> f32 {
    l.resolve_or(basis, 0.0).max(0.0)
}

/// Ist `vorfahr` ein Vorfahr von `knoten` (oder `knoten` selbst)?
fn ist_vorfahr(doc: &narsil_dom::Document, vorfahr: NodeId, mut knoten: NodeId) -> bool {
    for _ in 0..4096 {
        if knoten == vorfahr {
            return true;
        }
        match doc.get(knoten).and_then(|n| n.parent) {
            Some(p) => knoten = p,
            None => return false,
        }
    }
    false
}

/// Bricht alle Absätze neu um, deren gemessene Breite abweicht.
/// `true`, wenn sich das Dokument geändert hat.
pub fn umbrechen(
    seite: &mut narsil_page::Seite,
    baum: &LayoutTree,
    messung: &narsil_layout::Messung,
) -> bool {
    if seite.absaetze.is_empty() {
        return false;
    }
    let mut karte: HashMap<NodeId, &LayoutBox> = HashMap::new();
    let mut schwimmer: Vec<Schwimmer> = Vec::new();
    sammeln(&baum.root, &seite.layout_plan, &mut karte, &mut schwimmer, 0);

    // Der Rand eines Floats zählt mit (Firefox bricht am Margin-Kasten um).
    let float_raender: Vec<(f32, f32)> = schwimmer
        .iter()
        .map(|s| {
            let st = seite.document.get(s.knoten).map(|n| n.style.margin);
            match st {
                Some(m) => (px(m.left, s.kasten.width), px(m.right, s.kasten.width)),
                None => (0.0, 0.0),
            }
        })
        .collect();

    // Aufträge zuerst sammeln (die Karte leiht den Baum), dann umbrechen.
    struct Auftrag {
        index: usize,
        baender: Vec<(f32, f32)>,
        rest: (f32, f32),
    }
    let mut auftraege: Vec<Auftrag> = Vec::new();
    // Wachstum je Container: frühere, schmaler gewordene Absätze schieben
    // die späteren nach unten (der Float bleibt stehen).
    let mut wachstum: HashMap<NodeId, f32> = HashMap::new();

    for (index, satz) in seite.absaetze.iter().enumerate().take(MAX_ABSAETZE) {
        // Release 08.10.2026: `NARSIL_DBG_NACHUMBRUCH=<knoten>` zeigt, warum
        // ein Absatz (nicht) neu umbrochen wird.
        let dbg = std::env::var("NARSIL_DBG_NACHUMBRUCH").ok().and_then(|v| v.parse::<u32>().ok()) == Some(satz.knoten.index() as u32);
        if dbg {
            eprintln!("NACHUMBRUCH knoten={} breite={} zeilen={:?} kasten={:?}", satz.knoten.index(), satz.breite, satz.zeilen, karte.get(&satz.knoten).map(|k| k.content_box));
        }
        let Some(kasten) = karte.get(&satz.knoten) else { continue };
        // Text direkt in einem Flex-/Grid-Container: die Zeilen sind dort
        // Flex-/Grid-Elemente, ihre Breite verteilt der Container.
        //
        // Release 10.10.2026: AUSSER in einem Flex-Container in SPALTEN-
        // Richtung — dort liegen die Zeilen untereinander und sind (wie in
        // einem Block) so breit wie der Container, das Umbrechen auf seine
        // Breite ist kein Zirkel. GEMESSEN vorher (chip.de, Anrisstexte mit
        // `display:-webkit-box;-webkit-box-orient:vertical`): geschaetzt mit
        // 956 px, im Layout 388 px breit — die erste Zeile lief abgeschnitten
        // aus dem Kasten.
        let plan = seite.layout_plan.hole(satz.knoten);
        let spalte = plan.is_some_and(|p| {
            p.anzeige == Anzeige::Flex && p.flex_richtung == narsil_layout::plan::FlexRichtung::Spalte
        });
        if !spalte
            && (kasten.ist_flex_container
                || plan.is_some_and(|p| matches!(p.anzeige, Anzeige::Flex | Anzeige::Raster | Anzeige::Inline)))
        {
            continue;
        }
        // Release 08.10.2026: ein SCHRUMPFENDER Kasten (inline-block, Float
        // oder absolut, jeweils ohne eigene Breite) ist so breit wie sein
        // Text — ihn auf diese Breite neu umzubrechen ist ein Zirkel.
        // GEMESSEN vorher: Wikipedias `hlist`-Eintraege (`li` inline-block)
        // brachen nach jedem Wort um, weil 106,19 px Text in 106 px Kasten
        // nicht passten.
        let schrumpft = seite.layout_plan.hole(satz.knoten).is_some_and(|p| {
            p.anzeige == Anzeige::InlineBlock
                || p.schweben != FloatArt::Keins
                || matches!(p.position, narsil_layout::plan::PositionArt::Absolut | narsil_layout::plan::PositionArt::Fest)
        }) && seite
            .document
            .get(satz.knoten)
            .is_some_and(|n| n.style.size.width == narsil_core::length::Length::Auto);
        if schrumpft {
            continue;
        }
        let zeilen: Vec<&LayoutBox> = satz.zeilen.iter().filter_map(|z| karte.get(z).copied()).collect();
        let (Some(erste), Some(letzte)) = (zeilen.first(), zeilen.last()) else { continue };
        let inhalt = kasten.content_box;
        let links0 = inhalt.x + satz.einzug_links;
        let rechts0 = inhalt.x + inhalt.width - satz.einzug_rechts;
        if !(rechts0 - links0).is_finite() || rechts0 - links0 < 8.0 {
            continue;
        }
        let zeilen_hoehe = erste.border_box.height.max(1.0);
        let oben = erste.border_box.y + wachstum.get(&satz.knoten).copied().unwrap_or(0.0);
        let alt_hoehe = letzte.border_box.y + letzte.border_box.height - erste.border_box.y;

        // Die Floats, die diesen Absatz betreffen: waagerecht im Band des
        // Absatzes und nicht sein eigener Vorfahr (Text IN der Infobox).
        let betroffen: Vec<(Rect, FloatArt, f32, f32)> = schwimmer
            .iter()
            .zip(float_raender.iter())
            .filter(|(s, _)| s.kasten.x < rechts0 && s.kasten.x + s.kasten.width > links0)
            .filter(|(s, _)| s.kasten.y + s.kasten.height > oben)
            .filter(|(s, _)| !ist_vorfahr(&seite.document, s.knoten, satz.knoten))
            // Release 12.10.2026 (Agent CSS2): nur Floats VOR dem Absatz (in
            // Dokumentordnung, also mit kleinerer Knotennummer als seine erste
            // Zeile) — ein spaeterer Float steht im ersten Layout zwar im Band,
            // kann aber keine fruehere Zeile verkuerzen (CSS 2.1 §9.5.1 Regel 5
            // umgekehrt). GEMESSEN vorher (WPT `floats/float-no-content-beside-
            // 001`): die Floats der FOLGENDEN Absaetze verkuerzten den ersten.
            .filter(|(s, _)| satz.zeilen.first().is_none_or(|z| s.knoten.index() < z.index()))
            .map(|(s, (ml, mr))| (s.kasten, s.art, *ml, *mr))
            .collect();

        let breite_bei = |y: f32| -> (f32, f32) {
            let (mut l, mut r) = (links0, rechts0);
            for (k, art, ml, mr) in &betroffen {
                if y + zeilen_hoehe <= k.y || y >= k.y + k.height {
                    continue;
                }
                match art {
                    FloatArt::Rechts => r = r.min(k.x - ml),
                    FloatArt::Links => l = l.max(k.x + k.width + mr),
                    FloatArt::Keins => {}
                }
            }
            (l - links0, (r - l).max(1.0))
        };
        // Genug Zeilen planen, dass auch ein schmaler gewordener Absatz
        // vollständig neben dem Float liegt.
        let planzeilen = (zeilen.len() * 4 + 8).min(narsil_page::MAX_ZEILEN_PRO_ABSATZ);
        let baender: Vec<(f32, f32)> =
            (0..planzeilen).map(|k| breite_bei(oben + k as f32 * zeilen_hoehe)).collect();
        let rest = breite_bei(oben + planzeilen as f32 * zeilen_hoehe + 1.0e6);
        let weicht_ab = baender
            .iter()
            .take(zeilen.len() + 1)
            .any(|(v, b)| *v > 0.5 || (b - satz.breite).abs() > TOLERANZ);
        if !weicht_ab {
            continue;
        }
        // Schätzung der neuen Höhe für die nachfolgenden Absätze: das
        // Verhältnis der Breiten, mindestens die alte Höhe.
        let neue_breite = baender.first().map_or(satz.breite, |b| b.1).max(1.0);
        // Release 12.10.2026 (Agent CSS2): die Zeilenzahl mit den neuen Breiten
        // statt des Breitenverhaeltnisses. GEMESSEN vorher (WPT `floats/float-
        // no-content-beside-001-ref`): ein einzeiliger Absatz neben dem Float
        // wurde auf 2,2 Zeilen geschaetzt, der naechste sah den Float zu frueh
        // enden.
        let zeilen_neu = {
            let breiten = |k: usize| baender.get(k).copied().unwrap_or(rest);
            narsil_page::absatz_zeilen_anzahl(seite, index, messung, &breiten)
        };
        let schaetzung = match zeilen_neu {
            Some(n) => (n as f32 * zeilen_hoehe).max(alt_hoehe),
            None => (alt_hoehe * (satz.breite / neue_breite).max(1.0)).min(alt_hoehe * 8.0),
        };
        *wachstum.entry(satz.knoten).or_insert(0.0) += (schaetzung - alt_hoehe).max(0.0);
        auftraege.push(Auftrag { index, baender, rest });
    }

    let mut geaendert = false;
    for a in auftraege {
        let breiten = |k: usize| a.baender.get(k).copied().unwrap_or(a.rest);
        geaendert |= narsil_page::absatz_neu_umbrechen(seite, a.index, messung, &breiten);
    }
    geaendert
}
