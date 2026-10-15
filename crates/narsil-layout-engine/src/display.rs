//! Wandelt einen Layout-Baum in eine Display-List um.
//!
//! Die Display-Liste ist die Schnittstelle zwischen Layout und Renderer:
//! WAS gezeichnet wird, in welcher Reihenfolge — nie WIE.
//!
//! # Reihenfolge (Painter's Algorithm)
//!
//! 1. Hintergrund eines Knotens,
//! 2. Text desselben Knotens,
//! 3. Kinder in Dokumentreihenfolge (Tiefensuche).
//!
//! Damit liegt der Hintergrund hinter dem Text SEINES Knotens (gefordert) und
//! Eltern werden vor Kindern gezeichnet.
//!
//! # Was NICHT erzeugt wird
//!
//! * transparenter Hintergrund (`a == 0`) -> kein Rect-Item,
//! * leere Rechtecke (Breite oder Hoehe `<= 0`) -> kein Rect-Item,
//! * leerer Text -> kein Text-Item.
//!
//! # Warum der `Document` noetig ist
//!
//! `LayoutBox::background` speichert nur das RECHTECK, nicht die Farbe — der
//! Farbwert steht im Style des zugehoerigen DOM-Knotens. Darum hat
//! [`build_display_list`] gegenueber der Vorlage einen zweiten Parameter
//! (bewusste, dokumentierte Abweichung aus TASK_0004).

use narsil_display::{DisplayItem, DisplayList};
use narsil_dom::Document;
use narsil_layout::{LayoutBox, LayoutTree};

/// Wandelt einen Layout-Baum in eine Display-List.
///
/// Reihenfolge = Zeichnungsreihenfolge: Eltern-Background zuerst, dann der
/// Text desselben Knotens, dann die Kinder in Dokumentreihenfolge.
///
/// Knoten, die im Dokument nicht mehr existieren, werden uebersprungen
/// (kein Panic). Ein leerer Layout-Baum liefert eine leere Liste.
///
/// Die Tiefensuche laeuft ueber einen expliziten Stack, damit auch sehr tiefe
/// Baumstrukturen (1000 Ebenen) den Stapelspeicher nicht ueberlaufen.
pub fn build_display_list(tree: &LayoutTree, document: &Document) -> DisplayList {
    build_display_list_mit_bildern(tree, document, |_, _| {})
}

/// Pixelquellen werden beim zugehoerigen Kasten nach dessen Hintergrund
/// und vor seinen Texten/Kindern eingefuegt. So gilt dieselbe Stapelordnung.
pub fn build_display_list_mit_bildern(
    tree: &LayoutTree,
    document: &Document,
    bilder: impl Fn(&LayoutBox, &mut DisplayList),
) -> DisplayList {
    build_display_list_im_bereich(tree, document, bilder, None)
}

/// Release 08.10.2026: wie [`build_display_list_mit_bildern`], aber nur
/// fuer Kaesten, die den senkrechten `bereich` (Dokumentkoordinaten,
/// `(oben, unten)`) beruehren. Die Malreihenfolge bleibt dieselbe — es
/// fallen nur Eintraege weg, die ganz ausserhalb liegen.
///
/// GEMESSEN vorher (`NARSIL_LISTE_BENCH`, Wikipedia „Rust", 1888 x 954): die
/// ganze Seite wurde fuer JEDES Bild in eine Liste mit 4 486 Eintraegen
/// verwandelt (jeder Text eine neue Zeichenkette) — 12,4 ms je Bild, von
/// denen ~200 Eintraege sichtbar waren.
pub fn build_display_list_im_bereich(
    tree: &LayoutTree,
    document: &Document,
    bilder: impl Fn(&LayoutBox, &mut DisplayList),
    bereich: Option<(f32, f32)>,
) -> DisplayList {
    build_display_list_im_bereich_fest(tree, document, bilder, bereich, 0.0)
}

/// Release 09.10.2026: wie [`build_display_list_im_bereich`], aber alles,
/// was in einem `position: fixed`-Kasten liegt, um `fest_dy` verschoben —
/// der Aufrufer gibt den Rollversatz, damit feste Kaesten nach dem
/// Verschieben der ganzen Liste an ihrer Stelle im Fenster bleiben
/// (CSS 2.1, 9.6.1). GEMESSEN vorher: ein fester Kopf rollte mit der Seite
/// aus dem Bild.
pub fn build_display_list_im_bereich_fest(
    tree: &LayoutTree,
    document: &Document,
    bilder: impl Fn(&LayoutBox, &mut DisplayList),
    bereich: Option<(f32, f32)>,
    fest_dy: f32,
) -> DisplayList {
    build_display_list_mit_masken(tree, document, bilder, None, bereich, fest_dy)
}

/// Release 15.10.2026 (Agent F): like [`build_display_list_im_bereich_fest`],
/// and `masken` gives the `mask-image` coverage of a box (in list
/// coordinates before scrolling); the box and its subtree are masked with
/// it. `None`: the box has no mask of its own (a text box whose element
/// masks it already); a text box is masked only with its own entry (the
/// text of an inline element without a box). A box with a mask whose
/// coverage is unknown (`Some(None)`) is not painted: an image that is not
/// (yet) there masks everything away. Without `masken` (callers without
/// images), `mask-image` is not applied.
pub fn build_display_list_mit_masken(
    tree: &LayoutTree,
    document: &Document,
    bilder: impl Fn(&LayoutBox, &mut DisplayList),
    masken: Option<&dyn Fn(&LayoutBox) -> Option<Option<narsil_display::Maske>>>,
    bereich: Option<(f32, f32)>,
    fest_dy: f32,
) -> DisplayList {
    let mut list = DisplayList::new();
    //
    // P8 (02.10.2026): DIE MAL-REIHENFOLGE NACH CSS 2.1, ANHANG E.
    //
    // GEMESSENER FEHLER: hier stand eine reine Tiefensuche in
    // Dokumentreihenfolge. Die ist fuer eine Seite richtig, auf der
    // nichts positioniert ist — und falsch, sobald `position` oder
    // `z-index` vorkommen. Ein `position: absolute; z-index: 9999`
    // (github.com hat davon Dutzende) wurde VOR allen Kisten gezeichnet,
    // die nach ihm im Dokument standen, also lag es HINTER ihnen.
    //
    // `schicht_von` und `reihenfolge` (dieser Crate, `plan.rs:456`)
    // waren dafuer gebaut, exportiert und getestet — und von niemand
    // aufgerufen. Derselbe Befund wie der Vormerker (P7): ein vorhandener,
    // gebauter, unbenutzter Pfad.
    //
    // # WIE DER SCHLUESSEL AUSSIEHT
    //
    // Anhang E ordnet innerhalb eines Stapelkontexts in Schritten. Jeder
    // Knoten bekommt deshalb einen Schluessel aus Malgruppe, z-index und
    // Dokumentnummer je lokaler Ebene,
    // und die Schluessel werden lexikografisch verglichen; bei Gleichstand
    // entscheidet die Dokumentreihenfolge. Genau dieses Mitschreiben der
    // Ebenen ist der Punkt: ein `z-index: 5` an einer tiefen Box muss
    // ueber die Kisten neben ihrem GROSSVATER gehen, und das passiert
    // von selbst, weil an der Ebene des Vorfahren eine 6 steht und eine 2
    // daneben.
    //
    // | Schluessel | was |
    // |-----------|------|
    // | 1 | `z-index` negativ |
    // | 2 | im Fluss, Blockebene, nicht positioniert |
    // | 3 | nicht positionierter Float |
    // | 4 | im Fluss, Inline-Ebene |
    // | 5 | positioniert, `z-index` auto oder 0 |
    // | 6 | `z-index` positiv |
    //
    // Schritt 4 (Inline-Ebene) ist hier IMMER LEER, und das ist richtig:
    // `narsil-page` erzeugt aus einem Inline-Element keinen Knoten (siehe
    // `ist_inline` dort), es gibt also keine Inline-Kiste. Der Platz
    // bleibt in der Tabelle, damit die Nummerierung mit Anhang E
    // uebereinstimmt und jemand, der sie liest, nicht nachzaehlen muss.
    //
    // DER AUFWAND: eine `Vec<(u8, i32, usize)>` je Knoten und eine
    // Sortierung. Die Ebenenzahl ist durch `MAX_BAU_TIEFE` (64) gedeckelt,
    // also ist die Vec klein; und es ist EIN Sortieren ueber die Knoten,
    // kein Sortieren je Ebene.
    let mut teile: Vec<(
        Pfad,
        usize,
        &LayoutBox,
        Option<narsil_core::geometry::Rect>,
        f32,
        bool,
        Affin,
        Option<narsil_core::color::Color>,
        Option<narsil_display::RoundedClip>,
    )> = Vec::new();
    let mut ordnung = 0usize;
    sammle_reihenfolge(
        &tree.root,
        document,
        &mut teile,
        &mut ordnung,
        bereich,
        fest_dy,
    );
    // `sort_by` ist in Rust STABIL. Der zweite Schluessel ist damit
    // ueberfluessig — er macht die Absicht aber unabhaengig vom
    // Sortierverfahren lesbar, und ein nicht-stabiler Sortierer waere
    // die Fehlerquelle Nummer eins in dieser Datei.
    teile.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    // Release 15.10.2026 (Agent F): `clip-path` masks a box and its subtree.
    // The box is a stacking context, so the parts of its subtree follow it
    // in paint order: their keys extend its key.
    let ansicht = (tree.root.border_box.width, tree.root.border_box.height);
    let mut offene: Vec<(Pfad, usize, Option<narsil_display::Maske>)> = Vec::new();
    let mut open_filters: Vec<(Pfad, usize, narsil_display::FilterSpec, f32)> = Vec::new();
    // The effects that are open: further boxes of the same element (its
    // text, which carries a copy of the element's style, and anonymous
    // boxes) are inside them and do not apply them again (a second
    // `opacity()` or blur would compound). The values are compared by
    // pointer: a copied style shares them, another element has its own.
    let mut open_effects: Vec<(Pfad, narsil_dom::NodeId, usize)> = Vec::new();
    for (schluessel, _ordnung, bx, clip, deckkraft, fest, affin, text_ersatz, rund) in teile.iter() {
        while open_effects.last().is_some_and(|(p, _, _)| !schluessel.starts_with(p.as_slice())) {
            open_effects.pop();
        }
        let dom_node = document.get(bx.node);
        let is_text = dom_node.is_some_and(|n| n.kind == narsil_dom::NodeKind::Text);
        let effect_key = dom_node.and_then(|n| n.style.filter_value.as_ref().or(n.style.clip_pfad.as_ref()).or(n.style.backdrop_filter.as_ref()).map(|a| std::sync::Arc::as_ptr(a) as *const u8 as usize)).unwrap_or(0);
        let already_applied = open_effects.iter().any(|(_, k, id)| *k == bx.node || (effect_key != 0 && *id == effect_key));
        let mask_entry = match masken {
            Some(f) if !already_applied && dom_node.is_some_and(|n| n.style.maske_bild.is_some()) => match f(bx) {
                None if !is_text => Some(None),
                r => r,
            },
            _ => None,
        };
        let with_effects = !already_applied && (effect_key != 0 || mask_entry.is_some());
        if with_effects {
            open_effects.push((std::rc::Rc::clone(schluessel), bx.node, effect_key));
        }
        while offene.last().is_some_and(|(p, _, _)| !schluessel.starts_with(p.as_slice())) {
            if let Some((_, anfang, Some(m))) = offene.pop() {
                list.gruppe_mit_maske(anfang, m);
            }
        }
        while open_filters.last().is_some_and(|(p, _, _, _)| !schluessel.starts_with(p.as_slice())) {
            if let Some((_, anfang, spec, reach)) = open_filters.pop() {
                list.filter_group(anfang, spec, reach);
            }
        }
        if let Some(rect) = clip {
            list.push_clip(*rect);
        }
        let anfang = list.len();
        let dy = if *fest { fest_dy } else { 0.0 };
        // Release 15.10.2026 (Agent F): `filter` paints the box and its
        // subtree alone and filters them (the outermost effect: filter,
        // then clip-path, then mask, CSS Masking 1 §1).
        if let Some(n) = document.get(bx.node).filter(|_| with_effects)
            && let Some(value) = n.style.filter_value.clone()
        {
            let (s, ax, ay) = *affin;
            let b = bx.mal_kasten.unwrap_or(bx.border_box);
            let border_box = narsil_core::geometry::Rect::new(b.x * s + ax, b.y * s + ay + dy, b.width * s, b.height * s);
            let c = n.style.color;
            let reach = narsil_svg::filter_reach(&value, (f64::from(border_box.width), f64::from(border_box.height))) as f32;
            let spec = narsil_display::FilterSpec { value: value.to_string(), svg: n.style.filter_svg.clone(), border_box, current_color: [c.r, c.g, c.b], clip: *clip };
            open_filters.push((std::rc::Rc::clone(schluessel), anfang, spec, reach));
        }
        if let Some(wert) = document.get(bx.node).filter(|_| with_effects).and_then(|n| n.style.clip_pfad.clone()) {
            offene.push((std::rc::Rc::clone(schluessel), anfang, clip_maske(bx, document, &wert, *affin, dy, ansicht)));
        }
        if let Some(m) = mask_entry.filter(|_| with_effects) {
            let (s, ax, ay) = *affin;
            let m = m.map(|m| narsil_display::Maske { x: m.x * s + ax, y: m.y * s + ay + dy, ..m }).unwrap_or_else(|| {
                // Not (yet) loaded: a transparent layer masks everything.
                let b = bx.border_box;
                narsil_display::Maske { x: b.x, y: b.y + dy, breite: 1, hoehe: 1, alpha: std::sync::Arc::new(vec![0]) }
            });
            offene.push((std::rc::Rc::clone(schluessel), anfang, Some(m)));
        }
        // Release 15.10.2026 (Agent F): `backdrop-filter`, before the box's
        // background (inside its own effects).
        if let Some(n) = document.get(bx.node).filter(|_| with_effects)
            && let Some(value) = n.style.backdrop_filter.clone()
        {
            let (s, ax, ay) = *affin;
            let b = bx.border_box;
            let border_box = narsil_core::geometry::Rect::new(b.x * s + ax, b.y * s + ay + dy, b.width * s, b.height * s);
            let c = n.style.color;
            let spec = narsil_display::FilterSpec { value: value.to_string(), svg: n.style.backdrop_svg.clone(), border_box, current_color: [c.r, c.g, c.b], clip: *clip };
            let (rx, ry) = n.style.corner_radii(b.width, b.height);
            list.push_backdrop(spec, rx.map(|r| r * s), ry.map(|r| r * s));
        }
        let transformiert = *affin != AFFIN_EINS;
        if transformiert {
            // Eigene Clips des Kastens (Eingabefeld, Ersetztes) wandern mit.
            list.setze_abbildung(Some(*affin));
        }
        push_box_items(&mut list, bx, document, &bilder, *deckkraft, *text_ersatz);
        // Release 15.10.2026 (Agent F): inside a rounded overflow clip.
        if let Some(rc) = rund {
            list.set_rounded_clip(anfang, list.len(), *rc);
        }
        if transformiert {
            list.setze_abbildung(None);
            list.transformiere_bereich(anfang, list.len(), affin.0, affin.1, affin.2);
        }
        if *fest {
            list.schiebe_bereich(anfang, list.len(), fest_dy);
        }
        if clip.is_some() {
            list.pop_clip();
        }
    }
    while let Some((_, anfang, m)) = offene.pop() {
        if let Some(m) = m {
            list.gruppe_mit_maske(anfang, m);
        }
    }
    while let Some((_, anfang, spec, reach)) = open_filters.pop() {
        list.filter_group(anfang, spec, reach);
    }
    list
}

/// Release 15.10.2026 (Agent F): a `<clipPath>` reference on a box: the
/// box's user space starts at its border box (objectBoundingBox units are
/// the border box), the coverage covers the box and as much again around
/// it.
fn clip_maske_svg(quelle: &str, wert: &str, border: (f64, f64, f64, f64), m: &narsil_canvas::Matrix) -> Option<narsil_display::Maske> {
    const MAX_PIXEL: u64 = 64 * 1024 * 1024;
    let (bw, bh) = (border.2 - border.0, border.3 - border.1);
    let user = m.multiply(&narsil_canvas::Matrix::translate(border.0, border.1));
    let (x0, y0) = user.apply(-bw / 2.0, -bh / 2.0);
    let (x1, y1) = user.apply(bw * 1.5, bh * 1.5);
    let (x0, y0, x1, y1) = (x0.min(x1).floor(), y0.min(y1).floor(), x0.max(x1).ceil(), y0.max(y1).ceil());
    let (w, h) = ((x1 - x0).max(1.0) as u32, (y1 - y0).max(1.0) as u32);
    if u64::from(w) * u64::from(h) > MAX_PIXEL {
        return None;
    }
    let lm = narsil_canvas::Matrix::translate(-x0, -y0).multiply(&user);
    let alpha = narsil_svg::clip_path_coverage(quelle, wert, (bw, bh), lm, (w, h))?;
    Some(narsil_display::Maske { x: x0 as f32, y: y0 as f32, breite: w, hoehe: h, alpha: std::sync::Arc::new(alpha) })
}

/// Release 15.10.2026 (Agent F): the coverage of a `clip-path` value on a
/// box (CSS Masking 1 §5: basic shapes and reference boxes; the border box
/// is the default). `None`: the value does not clip (an unsupported or
/// invalid value, or an image too large to keep).
fn clip_maske(bx: &LayoutBox, document: &Document, wert: &str, affin: Affin, dy: f32, ansicht: (f32, f32)) -> Option<narsil_display::Maske> {
    const MAX_PIXEL: u64 = 64 * 1024 * 1024;
    let n = document.get(bx.node)?;
    let st = &n.style;
    let b = bx.mal_kasten.unwrap_or(bx.border_box);
    let f = |v: f32| f64::from(v);
    let border = (f(b.x), f(b.y), f(b.x + b.width), f(b.y + b.height));
    let [o, r, u, l] = [0, 1, 2, 3].map(|i| f(st.rahmen.gezeichnete_breite(i)));
    let padding = (border.0 + l, border.1 + o, border.2 - r, border.3 - u);
    let c = bx.content_box;
    let content = (f(c.x), f(c.y), f(c.x + c.width), f(c.y + c.height));
    let px = |len: &narsil_core::length::Length| match len {
        narsil_core::length::Length::Px(v) => f(*v),
        _ => 0.0,
    };
    let margin = (border.0 - px(&st.margin.left), border.1 - px(&st.margin.top), border.2 + px(&st.margin.right), border.3 + px(&st.margin.bottom));
    let mut boxes = narsil_svg::shapes::RefBoxes::css(content, padding, border, margin, f(st.font_size), (f(ansicht.0), f(ansicht.1)));
    // `rem` is the root element's font size.
    if let Some(wurzel) = document.get(document.root()).map(|n| n.style.font_size).filter(|v| *v > 0.0) {
        boxes.root_font_size = f(wurzel);
    }
    let (rx, ry) = st.corner_radii(b.width, b.height);
    boxes.border_radii = [0, 1, 2, 3].map(|i| (f(rx[i]), f(ry[i])));
    let (s, ax, ay) = affin;
    let m = narsil_canvas::Matrix::new(f(s), 0.0, 0.0, f(s), f(ax), f(ay) + f(dy));
    if wert.trim_start().to_ascii_lowercase().starts_with("url(") {
        return clip_maske_svg(st.clip_svg.as_deref()?, wert, border, &m);
    }
    let (pfad, regel) = narsil_svg::shapes::basic_shape(wert, &boxes, &m)?;
    let g = pfad.bounds();
    // An empty shape clips everything away: a one-pixel empty mask.
    let (x0, y0, x1, y1) = if g.x0.is_finite() && g.x1 > g.x0 && g.y1 > g.y0 { (g.x0.floor(), g.y0.floor(), g.x1.ceil(), g.y1.ceil()) } else { (border.0, border.1, border.0 + 1.0, border.1 + 1.0) };
    let (w, h) = ((x1 - x0).max(1.0) as u32, (y1 - y0).max(1.0) as u32);
    if u64::from(w) * u64::from(h) > MAX_PIXEL {
        return None;
    }
    // Release 15.10.2026 (Agent F): circles, ellipses and (rounded)
    // rectangles get the exact coverage the raster gives rounded boxes.
    // One pixel of room for the soft edge beyond the flattened bounds.
    if let Some(alpha) = narsil_svg::shapes::basic_shape_coverage(wert, &boxes, &m, (x0 - 1.0, y0 - 1.0), (w + 2, h + 2)) {
        return Some(narsil_display::Maske { x: (x0 - 1.0) as f32, y: (y0 - 1.0) as f32, breite: w + 2, hoehe: h + 2, alpha: std::sync::Arc::new(alpha) });
    }
    let mut ctx = narsil_canvas::Context2D::new(w, h, false)?;
    ctx.state.fill = narsil_canvas::Style::Color(narsil_canvas::Color::BLACK);
    if g.x0.is_finite() && g.x1 > g.x0 {
        ctx.fill(Some(&pfad.transformed(&narsil_canvas::Matrix::translate(-x0, -y0))), regel);
    }
    let alpha: Vec<u8> = ctx.surface.data.chunks_exact(4).map(|p| p[3]).collect();
    Some(narsil_display::Maske { x: x0 as f32, y: y0 as f32, breite: w, hoehe: h, alpha: std::sync::Arc::new(alpha) })
}

/// Beruehrt ein Kasten (mit seinem Text, der ueber den Kasten hinaus ragen
/// darf) den senkrechten Bereich? Die Zugabe deckt Schatten und
/// Unterlaengen ab — lieber ein Eintrag zu viel als eine fehlende Kante.
fn kasten_beruehrt(bx: &LayoutBox, oben: f32, unten: f32) -> bool {
    const ZUGABE: f32 = 256.0;
    let mut von = bx.border_box.y;
    let mut bis = bx.border_box.y + bx.border_box.height;
    if let Some(run) = &bx.text {
        von = von.min(run.origin.y - run.font_size * 2.0);
        bis = bis.max(run.origin.y + run.font_size * 2.0);
    }
    if !von.is_finite() || !bis.is_finite() {
        return true;
    }
    bis >= oben - ZUGABE && von <= unten + ZUGABE
}

/// Die Schluesselzahl eines Kastens in seinem Stapelkontext (Anhang E).
///
/// `float` wird hier NICHT getrennt gefuehrt: `LayoutBox` traegt den
/// Float nicht, und ein Float, der zugleich positioniert ist, ist nach
/// CSS 2.1, 9.7 ohnehin geklammert und faellt in Schritt 8.
/// Release 11.10.2026 (Agent CSS2): `LayoutBox::schwebt` traegt ihn
/// inzwischen; die Gruppe 3 setzt `sammle_reihenfolge` (`als_float`).
///
/// L5 (02.10.2026 13:2x): `flex_item` beantwortet „ist der Elternknoten
/// ein Flex-Container" (CSS Flexbox 1 §5.4).
///
/// GEMESSENER BEFUND, warum das noetig ist: `z-index` gilt fuer
/// Flex-Items auch bei `position: static`. Ein statischer Kasten ohne
/// diese Ausnahme ist ein gewoehnlicher Fluss-Block. GEMESSEN
/// (`tests/mal_reihenfolge.rs`, `l5_z_index_gilt_auch_fuer_ein_
/// statisches_flex_item`): ein statisches Flex-Item mit `z-index: 1`
/// ergab `[10, 20, 30]` — ohne Wirkung. Nach §5.4 muss es `[20, 30, 10]`
/// sein.
///
/// GEMESSENE GRENZE, die ich benenne statt zu raten: `display: flex` auf
/// `html` oder `body` ist hier NICHT abgedeckt, weil der Wurzelschritt
/// keinen Elternknoten hat. GEMESSEN habe ich den Fall nicht.
fn schicht(bx: &LayoutBox, flex_item: bool) -> u8 {
    // L5 (02.10.2026 13:0x): DIE PRUEFUNG AUF `position` VORAN.
    //
    // GEMESSENER BEFUND (`tests/mal_reihenfolge.rs`, 13:0x):
    // `z-index: 9` an einem statischen Kasten ergab `[20, 30, 10]`.
    // Richtig ist `[10, 20, 30]`. CSS 2.1, 9.9.1: "this property
    // applies to positioned elements".
    //
    // GEMESSENE ENTSCHEIDUNG, warum HIER genauso wie in
    // `plan.rs::schicht_von`: die beiden Stellen tragen dieselbe Regel
    // zweimal. Das ist hier keine Glueck, sondern die Folge davon, dass
    // die eine die Display-Liste (Anhang E) und die andere den Weg ueber
    // `mal_reihenfolge` bedient — zwei Ausgaben derselben Spezifikation.
    // Wer eine aendert, muss die andere mit aendern; der Test
    // `l5_z_index_an_einem_statischen_kasten_bleibt_ohne_wirkung` laeuft
    // ueber die Display-Liste, `schicht_von` ist ueber `mal_reihenfolge`
    // erreichbar.
    // CSS 2.1, 9.9.1: `z-index` gilt fuer positionierte Elemente —
    // CSS Flexbox 1 §5.4: und fuer Flex-Items, auch ohne `position`.
    if matches!(bx.position, narsil_layout::PositionArt::Statisch) && !flex_item {
        return 2;
    }
    match bx.z_index {
        Some(z) if z < 0 => 1,
        Some(z) if z > 0 => 6,
        // positioniert mit `z-index: auto` oder `0`
        _ => 5,
    }
}

/// Bildet DIESEN Kasten einen Stapelkontext? (CSS 2.1, Anhang E)
///
/// Zwei Faelle, und der zweite ist der, an dem die erste Fassung des
/// Schluessels gescheitert ist:
///
///  * die Wurzel des Baums — die Seite selbst,
///  * ein positioniertes Element mit `z-index` ungleich `auto`.
///
/// GEMESSENER FEHLER DER ERSTEN FASSUNG (der Test
/// `negatives_z_index_an_einem_enkel_kommt_vor_dem_grossvater`):
/// Der Schluessel war eine Zahl je EBENE, nicht je STAPELKONTEXT. Ein
/// statisches Elet ist kein Stapelkontext, also gehoert das
/// `z-index: -1` eines Kindes in die negative Gruppe der WURZEL — und
/// die kommt VOR dem statischen Eltern. Mit der Ebenenvariante stand
/// dort `[2]` (Eltern) gegen `[2, 1]` (Kind), und weil `[2]` ein Praefix
/// von `[2, 1]` ist, gewann der Eltern: `[50, 60]` statt `[60, 50]`.
///
/// WAS HIER NICHT DRIN IST, ehrlich benannt: CSS 2.1 nennt zusaetzlich
/// `opacity < 1` und (in CSS 3) `transform`/`filter` als Stapelkontext.
/// `LayoutBox` traegt die Deckkraft nicht, und `transform`/`filter` gibt
/// es bei uns noch nicht — beides gehoert in den Kasten, wenn es
/// benutzt wird, und nicht in eine Liste hier.
///
/// L5 (02.10.2026 14:0x): `opacity < 1` ist auch ein Stapelkontext.
///
/// GEMESSENER BEFUND (`tests/mal_reihenfolge.rs`,
/// `l5_opacity_unter_eins_macht_einen_stapelkontext`): ein Kasten mit
/// `opacity: 0.5`, unter dem ein Kind mit `z-index: -1` liegt, ergab
/// die Farbfolge `[70, 50, 60]` — das negative Kind wurde in den
/// Stapelkontext der WURZEL gehoeben und vor allem anderen gezeichnet.
/// Nach CSS 2.1, Anhang E Schritt 8 und CSS Color 3 §3.2 ist der opake
/// Kasten ein eigener Stapelkontext, also `[50, 70, 60]`.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Deckkraft aus dem DOKUMENT kommt und
/// nicht aus dem Plan: `build_display_list` bekommt Baum und Dokument,
/// nicht den Plan. Die Deckkraft steht an `narsil_core::Style::deckkraft`
/// (RUNDE 3 S2, 03.10.2026), also ist sie hier erreichbar, ohne ein Feld
/// an `LayoutBox` zu erfinden — anders als bei `position`, `z_index` und
/// `ist_flex_container`, fuer die es keine Quelle im Dokument gab.
///
/// GEMESSENE ENTSCHEIDUNG, warum `deckkraft < 1.0` und nicht
/// `deckkraft > 0.0`: 1.0 ist der Anfangswert und heisst „deckend" — ein
/// Kasten mit 1.0 ist KEIN Stapelkontext. Die Gegenprobe
/// `l5_opacity_ist_kein_stapelkontext` behauptet genau das mit Zahlen.
fn ist_stapelkontext(bx: &LayoutBox, ist_wurzel: bool, document: &Document) -> bool {
    if ist_wurzel {
        return true;
    }
    // L5 (02.10.2026 17:0x): TRANSFORM ODER FILTER MIT ANDEREM WERT ALS
    // `none` erzeugen einen Stapelkontext.
    //
    // GEMESSEN, warum das am Kasten steht und nicht am Plan: hier ist
    // der Plan nicht verfuegbar (siehe `position`). GEMESSEN, wo die
    // Regel ausgewertet wird: in `narsil_css::erzeugt_stapelkontext`, weil
    // dort der Rohwert noch lesbar ist — im Plan steht nur das Ergebnis.
    //
    // GEMESSENE ENTSCHEIDUNG, warum das VOR `deckkraft` geprueft wird:
    // die Reihenfolge der Bedingungen ist ohne Bedeutung (es ist eine
    // Oder-Verknuepfung), aber die Zeile mit dem juengsten Datum steht
    // oben — dann findet sie der naechste Leser zuerst.
    if bx.transformt {
        return true;
    }
    if bx.position != narsil_layout::PositionArt::Statisch && bx.z_index.is_some() {
        return true;
    }
    // Release 15.10.2026 (Agent F): `clip-path` other than `none` makes a
    // stacking context (CSS Masking 1 §5.1).
    if document.get(bx.node).is_some_and(|n| n.style.clip_pfad.is_some() || n.style.maske_bild.is_some() || n.style.filter_value.is_some() || n.style.backdrop_filter.is_some()) {
        return true;
    }
    // Kein Planeintrag im Dokument: dann kann die Deckkraft nicht
    // kleiner als 1 sein, und der Vergleich ist die guenstigere
    // Antwort. GEMESSEN: `document.get` liefert fuer jeden Baumknoten
    // einen Knoten — der Fall ist trotzdem abgesichert, weil ein
    // `None` hier sonst stillschweigend als „kein Stapelkontext"
    // durchginge.
    document
        .get(bx.node)
        .is_some_and(|n| n.style.deckkraft < 1.0)
}

/// Sammelt alle Kaesten mit ihrem Schluessel.
///
/// `kontext` traegt die echten Stapelkontexte; `inhalt` traegt zusaetzlich
/// lokale Malgruppen fuer positionierte Boxen mit `z-index: auto`.
/// Deren normale Kinder und Text werden nach ihrem eigenen Hintergrund
/// gemalt. Positionierte Nachfahren und echte Stapelkontexte bleiben
/// dagegen im echten Elternkontext (CSS Anhang E, Schritt 8).
///
/// Jeder Schluesselschritt traegt Malgruppe, z-index und Dokumentnummer.
/// Die Nummer haelt lokale Inhalte zweier gleich geschichteter Geschwister
/// auseinander; z-index ordnet negative/positive Kontexte numerisch.
///
/// # M5: ITERATIV
///
/// Der Stapel ist ein `Vec`, kein Aufruf. Eine Seite kann 60 Ebenen tief
/// sein; das ist kein Grund fuer einen Stapelspeicherueberlauf.
/// Release 08.10.2026: der Malschluessel eines Kastens, GETEILT zwischen
/// Eltern und Kindern (`Rc`) — vorher wurde er je Kasten bis zu viermal
/// kopiert (Pfad, Eintrag, und je Kind Inhalt und Kontext).
type Pfad = std::rc::Rc<Vec<(u8, i32, usize)>>;

/// Release 11.10.2026: die Abbildung `p -> s·p + (dx, dy)`, mit der ein
/// Kasten (und sein Teilbaum) wegen `transform` gezeichnet wird.
type Affin = (f32, f32, f32);
const AFFIN_EINS: Affin = (1.0, 0.0, 0.0);

/// Release 11.10.2026: siehe `LayoutBox::abbildung` (narsil-layout).
fn affin_von(bx: &LayoutBox, aussen: Affin) -> Affin {
    bx.abbildung(aussen)
}

fn sammle_reihenfolge<'a>(
    wurzel: &'a LayoutBox,
    document: &Document,
    teile: &mut Vec<(
        Pfad,
        usize,
        &'a LayoutBox,
        Option<narsil_core::geometry::Rect>,
        f32,
        bool,
        Affin,
        Option<narsil_core::color::Color>,
        Option<narsil_display::RoundedClip>,
    )>,
    ordnung: &mut usize,
    bereich: Option<(f32, f32)>,
    fest_dy: f32,
) {
    struct Schritt<'a> {
        kasten: &'a LayoutBox,
        /// Lokale Malgruppe einschliesslich auto-positionierter Vorfahren.
        inhalt: Pfad,
        /// Echte Stapelkontexte; positionierte Nachfahren entkommen auto.
        kontext: Pfad,
        ist_wurzel: bool,
        /// Ist der ELTERNKASTEN ein Flex-Container? (L5, 13:2x,
        /// Flexbox 1 §5.4). Es steht am Schritt des Kindes, weil man es
        /// nur dort weiss: `LayoutBox` hat keinen Zeiger nach oben.
        eltern_ist_flex: bool,
        /// Clip der Vorfahren; die eigene Box wird erst fuer Kinder begrenzt.
        clip: Option<narsil_core::geometry::Rect>,
        /// Release 13.10.2026 (Agent CSS2): der Clip am naechsten
        /// positionierten Vorfahren (samt dessen eigenem `overflow`) — ein
        /// absolut positionierter Kasten entkommt dem `overflow` der nicht
        /// positionierten Kaesten zwischen sich und seinem enthaltenden Block
        /// (CSS 2.1 §11.1.1). GEMESSEN vorher (WPT `positioning/abspos-
        /// overflow-001..009`): er wurde vom Rollkasten beschnitten.
        clip_abs: Option<narsil_core::geometry::Rect>,
        /// Release 08.10.2026: die Deckkraft der Vorfahren (aufmultipliziert).
        deckkraft: f32,
        /// Release 09.10.2026: in einem `position: fixed`-Kasten.
        fest: bool,
        /// Release 11.10.2026: die Abbildung der Vorfahren (`transform`).
        affin: Affin,
        /// Release 11.10.2026: `background-clip: text` eines Vorfahren — die
        /// Farbe, in der durchsichtiger Text gezeichnet wird.
        text_ersatz: Option<narsil_core::color::Color>,
        // Release 15.10.2026 (Agent F): the rounded overflow clip (and the
        // one for absolute descendants), like `clip`/`clip_abs`.
        rund: Option<narsil_display::RoundedClip>,
        rund_abs: Option<narsil_display::RoundedClip>,
    }
    let mut stapel: Vec<Schritt<'a>> = Vec::new();
    stapel.push(Schritt {
        kasten: wurzel,
        inhalt: Pfad::default(),
        kontext: Pfad::default(),
        ist_wurzel: true,
        // Die Wurzel hat keinen Elternknoten im Baum; sie ist nie ein
        // Flex-Item. `display: flex` auf `html`/`body` waere dann nicht
        // abgedeckt — das ist eine echte Luecke und steht als solche im
        // Bericht, statt hier stillschweigend geraten zu werden.
        eltern_ist_flex: false,
        clip: None,
        clip_abs: None,
        deckkraft: 1.0,
        fest: false,
        affin: AFFIN_EINS,
        text_ersatz: None,
        rund: None,
        rund_abs: None,
    });
    while let Some(mut st) = stapel.pop() {
        // Release 13.10.2026 (Agent CSS2): siehe `Schritt::clip_abs`; ein
        // fester Kasten haengt an der Ansicht.
        match st.kasten.position {
            narsil_layout::PositionArt::Absolut => {
                st.clip = st.clip_abs;
                st.rund = st.rund_abs;
            }
            narsil_layout::PositionArt::Fest => {
                st.clip = None;
                st.rund = None;
            }
            _ => {}
        }
        // Release 08.10.2026: `opacity` wirkt auf den GANZEN Teilbaum (CSS
        // Color 4 §3.2). Ohne Gruppen-Compositing wird sie entlang des Baums
        // aufmultipliziert; bei 0 entfaellt der Teilbaum ganz. GEMESSEN
        // vorher: nur der eigene Hintergrund wurde durchsichtig, Text und
        // Kinder blieben — t-online.de zeigte jedes versteckte
        // `opacity-0`-Ausklappmenue mitten auf der Seite.
        let eigen = document
            .get(st.kasten.node)
            .map(|n| n.style.deckkraft)
            .filter(|d| d.is_finite())
            .map_or(1.0, |d| d.clamp(0.0, 1.0));
        let deckkraft = st.deckkraft * eigen;
        if deckkraft <= 0.002 {
            continue;
        }
        let echt = ist_stapelkontext(st.kasten, st.ist_wurzel, document)
            || (st.eltern_ist_flex && st.kasten.z_index.is_some());
        let positioniert = st.kasten.position != narsil_layout::PositionArt::Statisch;
        // CSS Anhang E Schritt 8: auto-positionierte Boxen halten ihre
        // normalen Kinder bei sich, erzeugen aber keinen echten Kontext.
        // Positionierte Nachfahren/echte Kontexte bleiben beim Elternkontext.
        let basis = if positioniert || echt { &st.kontext } else { &st.inhalt };
        let mut neu_pfad = Vec::with_capacity(basis.len() + 1);
        neu_pfad.extend_from_slice(basis);
        let gruppe = schicht(st.kasten, st.eltern_ist_flex);
        // Release 11.10.2026 (Agent CSS2): ein nicht positionierter Float
        // malt nach den Hintergruenden ALLER Blockkaesten im Fluss seines
        // Stapelkontexts, und sein Teilbaum malt mit ihm wie ein eigener
        // Kontext (CSS 2.1 Anhang E, E.2 Schritt 5; positionierte
        // Nachfahren bleiben beim Elternkontext). GEMESSEN vorher (WPT
        // `floats-clear/clear-initial-001`, `float-005`, `clear-004`): der
        // spaetere Block im Fluss malte seinen Hintergrund ueber den Float.
        let als_float = gruppe == 2 && st.kasten.schwebt && !st.eltern_ist_flex;
        let gruppe = if als_float { 3 } else { gruppe };
        let z = if gruppe == 1 || gruppe == 6 { st.kasten.z_index.unwrap_or(0) } else { 0 };
        // Dokumentnummer im Gruppenschluessel verhindert, dass die normale
        // Kindgruppe einer Box erst nach ihrem naechsten Geschwister malt.
        neu_pfad.push((gruppe, z, *ordnung));
        let pfad: Pfad = std::rc::Rc::new(neu_pfad);
        // Release 08.10.2026: ausserhalb des Bereichs kein Eintrag (die
        // Kinder werden trotzdem besucht — sie duerfen herausragen).
        let fest = st.fest || st.kasten.position == narsil_layout::PositionArt::Fest;
        // Ein fester Kasten steht nach dem Verschieben `fest_dy` tiefer —
        // der sichtbare Bereich liegt fuer ihn um so viel hoeher.
        let versatz = if fest { fest_dy } else { 0.0 };
        // Release 11.10.2026: `transform`. Bei `scale(0)` und Verwandten
        // ist der Teilbaum nicht zu sehen.
        let affin = affin_von(st.kasten, st.affin);
        if affin.0 <= 0.001 {
            continue;
        }
        // `background-clip: text`: der Hintergrund faerbt die Schrift. Ohne
        // Masken-Zeichnen bekommt durchsichtiger Text die Farbe des Verlaufs
        // (seine mittlere Stufe) bzw. des Hintergrunds.
        let text_ersatz = if st.kasten.hintergrund_clip == 3 {
            document
                .get(st.kasten.node)
                .and_then(|n| {
                    n.style
                        .verlauf
                        .as_ref()
                        .and_then(|v| v.stopps.get(v.stopps.len() / 2).map(|s| s.farbe))
                        .or_else(|| (n.style.background.a > 0).then_some(n.style.background))
                })
                .or(st.text_ersatz)
        } else {
            st.text_ersatz
        };
        let (s, _, ay) = affin;
        if bereich.is_none_or(|(oben, unten)| {
            kasten_beruehrt(st.kasten, (oben - versatz - ay) / s, (unten - versatz - ay) / s)
        }) {
            teile.push((std::rc::Rc::clone(&pfad), *ordnung, st.kasten, st.clip, deckkraft, fest, affin, text_ersatz, st.rund));
        }
        *ordnung += 1;
        let clip_rand = document.get(st.kasten.node).and_then(|n| n.style.clip_rand);
        let kind_clip = if st.kasten.ueberlauf.beschneidet() || clip_rand.is_some() {
            // Der Clip in Bildschirmlage: mit dem Kasten transformiert.
            // Release 13.10.2026 (Agent CSS2): an der INNENKANTE DES RAHMENS
            // (CSS Overflow 3 §3: der Ueberlauf wird an der Padding-Box
            // beschnitten), nicht an der Inhaltskante. GEMESSEN vorher (WPT
            // css-overflow, `CSS2/ui/overflow-applies-to-*`): Inhalt im
            // Innenabstand eines Rollkastens war weggeschnitten.
            let cb = match document.get(st.kasten.node) {
                Some(n) => {
                    let rahmen = &n.style.rahmen;
                    let [o, re, u, l] = [0, 1, 2, 3].map(|i| rahmen.gezeichnete_breite(i));
                    let b = st.kasten.mal_kasten.unwrap_or(st.kasten.border_box);
                    let innen = narsil_core::geometry::Rect::new(
                        b.x + l,
                        b.y + o,
                        (b.width - l - re).max(0.0),
                        (b.height - o - u).max(0.0),
                    );
                    // Release 13.10.2026 (Agent CSS2): `overflow-clip-margin`
                    // (siehe `Style::clip_rand`).
                    match clip_rand {
                        Some((kante, px)) => {
                            let r = match kante {
                                1 => b,
                                2 => st.kasten.content_box,
                                _ => innen,
                            };
                            narsil_core::geometry::Rect::new(r.x - px, r.y - px, (r.width + 2.0 * px).max(0.0), (r.height + 2.0 * px).max(0.0))
                        }
                        None => innen,
                    }
                }
                None => st.kasten.content_box,
            };
            let cb = if affin == AFFIN_EINS {
                cb
            } else {
                narsil_core::geometry::Rect::new(
                    affin.0 * cb.x + affin.1,
                    affin.0 * cb.y + affin.2,
                    affin.0 * cb.width,
                    affin.0 * cb.height,
                )
            };
            Some(st.clip.map_or(cb, |outer| {
                outer
                    .intersect(cb)
                    .unwrap_or(narsil_core::geometry::Rect::new(0.0, 0.0, 0.0, 0.0))
            }))
        } else {
            st.clip
        };
        // Release 15.10.2026 (Agent F): an overflow clip with rounded
        // corners masks the children to its rounded padding box (CSS
        // Backgrounds 3 §5.3); the innermost rounded clip wins.
        let kind_rund = if st.kasten.ueberlauf.beschneidet() && clip_rand.is_none() {
            document
                .get(st.kasten.node)
                .and_then(|n| {
                    let b = st.kasten.mal_kasten.unwrap_or(st.kasten.border_box);
                    let (rx, ry) = n.style.corner_radii(b.width, b.height);
                    if !(0..4).any(|i| rx[i] > 0.0 && ry[i] > 0.0) {
                        return None;
                    }
                    let rahmen = &n.style.rahmen;
                    let [o, re, u, l] = [0, 1, 2, 3].map(|i| rahmen.gezeichnete_breite(i));
                    let (ix, iy) = inner_corners(rx, ry, l, o, re, u);
                    let (s, ax, ay) = affin;
                    Some(narsil_display::RoundedClip {
                        rect: narsil_core::geometry::Rect::new(
                            s * (b.x + l) + ax,
                            s * (b.y + o) + ay,
                            s * (b.width - l - re).max(0.0),
                            s * (b.height - o - u).max(0.0),
                        ),
                        radius_x: ix.map(|v| v * s),
                        radius_y: iy.map(|v| v * s),
                    })
                })
                .or(st.rund)
        } else {
            st.rund
        };
        let kind_kontext = if echt { std::rc::Rc::clone(&pfad) } else { st.kontext };
        let kind_inhalt = if echt || positioniert || st.eltern_ist_flex || als_float {
            pfad
        } else {
            st.inhalt
        };
        // Release 11.10.2026: Flex- und Rasterelemente malen in `order`-
        // Reihenfolge (CSS Display 3, stabil). GEMESSEN vorher: WPT
        // `css-grid/grid-items/grid-order-property-painting-*`.
        let mut folge: Vec<&LayoutBox> = st.kasten.children.iter().collect();
        if st.kasten.ist_flex_container && folge.iter().any(|k| k.ordnung != 0) {
            folge.sort_by_key(|k| k.ordnung);
        }
        for k in folge.into_iter().rev() {
            stapel.push(Schritt {
                kasten: k,
                inhalt: std::rc::Rc::clone(&kind_inhalt),
                kontext: std::rc::Rc::clone(&kind_kontext),
                ist_wurzel: false,
                eltern_ist_flex: st.kasten.ist_flex_container,
                clip: kind_clip,
                clip_abs: if positioniert { kind_clip } else { st.clip_abs },
                deckkraft,
                fest,
                affin,
                text_ersatz,
                rund: kind_rund,
                rund_abs: if positioniert { kind_rund } else { st.rund_abs },
            });
        }
    }
}

/// Release 15.10.2026 (Agent F): the corners of an inner edge `left`,
/// `top`, `right`, `bottom` px inside a box with the corners (`x`, `y`):
/// each radius less the adjacent inset (CSS Backgrounds 3 §5.2).
/// Circular corners keep the larger of the two insets, as before.
fn inner_corners(x: [f32; 4], y: [f32; 4], left: f32, top: f32, right: f32, bottom: f32) -> ([f32; 4], [f32; 4]) {
    if x == y {
        let r = [
            (x[0] - left.max(top)).max(0.0),
            (x[1] - right.max(top)).max(0.0),
            (x[2] - right.max(bottom)).max(0.0),
            (x[3] - left.max(bottom)).max(0.0),
        ];
        return (r, r);
    }
    (
        [x[0] - left, x[1] - right, x[2] - right, x[3] - left].map(|v| v.max(0.0)),
        [y[0] - top, y[1] - top, y[2] - bottom, y[3] - bottom].map(|v| v.max(0.0)),
    )
}

/// Haengt Hintergrund und Text EINES Knotens an die Liste an.
fn push_box_items(
    list: &mut DisplayList,
    bx: &LayoutBox,
    document: &Document,
    bilder: &impl Fn(&LayoutBox, &mut DisplayList),
    deckkraft: f32,
    text_ersatz: Option<narsil_core::color::Color>,
) {
    // Release 11.10.2026 (Agent CSS2): Rahmen und Hintergrund am Raster einer
    // Tabelle mit Beschriftung (siehe `LayoutBox::mal_kasten`).
    let mal = bx.mal_kasten.unwrap_or(bx.border_box);
    // Release 08.10.2026: `deckkraft` ist die aufmultiplizierte Deckkraft
    // dieses Kastens samt Vorfahren (siehe `sammle_reihenfolge`).
    let alpha = |a: u8| -> u8 { (f32::from(a) * deckkraft).round().clamp(0.0, 255.0) as u8 };
    // Release 10.10.2026: `visibility: hidden` — der Kasten zeichnet nichts
    // von sich; seine Kinder entscheiden selbst (`Style::unsichtbar`).
    if document.get(bx.node).is_some_and(|n| n.style.unsichtbar) {
        return;
    }
    // Release 11.10.2026: `background-clip` — die Flaeche des Hintergrunds
    // (Farbe und Verlauf) und ihr Radius; bei `text` keine Flaeche.
    // Release 15.10.2026 (Agent F): with the vertical radii of elliptical
    // corners (`border-radius: 50%` on a box that is not square).
    let flaeche = |rect: narsil_core::geometry::Rect, node: &narsil_dom::Node| -> (narsil_core::geometry::Rect, [f32; 4], [f32; 4]) {
        let (r, ry) = node.style.corner_radii(mal.width, mal.height);
        match bx.hintergrund_clip {
            1 | 2 => {
                let rahmen = &node.style.rahmen;
                let [o, re, u, l] = [0, 1, 2, 3].map(|i| rahmen.gezeichnete_breite(i));
                let innen = if bx.hintergrund_clip == 2 {
                    bx.content_box
                } else {
                    narsil_core::geometry::Rect::new(
                        mal.x + l,
                        mal.y + o,
                        (mal.width - l - re).max(0.0),
                        (mal.height - o - u).max(0.0),
                    )
                };
                let (dl, dob) = (innen.x - mal.x, innen.y - mal.y);
                let dr = mal.x + mal.width - (innen.x + innen.width);
                let du = mal.y + mal.height - (innen.y + innen.height);
                let (ix, iy) = inner_corners(r, ry, dl, dob, dr, du);
                (innen, ix, iy)
            }
            _ => (rect, r, ry),
        }
    };
    let mut shadow_drawn = false;
    if let Some(rect) = bx.background.filter(|_| bx.hintergrund_clip != 3) {
        // Farbe steht im DOM, nicht in der Box.
        if !rect.is_empty() {
            if let Some(node) = document.get(bx.node) {
                let color = node.style.background;
                let (rect, radius, radius_y) = flaeche(rect, node);
                if color.a != 0 && !rect.is_empty() {
                    // S2 Station 3: die drei Felder mitnehmen, die der
                    // Zeichner fuer runde Ecken und Schatten braucht.
                    //
                    // Sie kommen aus `node.style` — also aus DER KASKADE,
                    // nicht aus einer zweiten Berechnung hier. Das ist der
                    // ganze Zweck der Station: die Werte sind in
                    // `narsil_core::Style` bereits gesetzt, sie mussten nur
                    // noch bis zum Zeichner laufen.
                    list.push(DisplayItem::Rect {
                        rect,
                        color,
                        radius,
                        deckkraft,
                        schatten: node.style.schatten,
                        schatten_anzahl: node.style.schatten_anzahl,
                    });
                    if radius_y != radius {
                        list.set_vertical_radii_for_last(radius_y);
                    }
                    shadow_drawn = true;
                }
            }
        }
    }
    // Release 14.10.2026 (Agent CSS2): a box shadow does not need a
    // background; without one it is drawn from the border box with a
    // transparent fill (CSS Backgrounds 3 §7.1). MEASURED before (WPT
    // css-backgrounds `box-shadow-outset-without-border-radius-001`, a box
    // with a transparent border and no background): no shadow.
    if !shadow_drawn
        && !mal.is_empty()
        && let Some(node) = document.get(bx.node)
        && node.kind != narsil_dom::NodeKind::Text
        && node.style.schatten_anzahl > 0
    {
        let (radius, radius_y) = node.style.corner_radii(mal.width, mal.height);
        list.push(DisplayItem::Rect {
            rect: mal,
            color: narsil_core::color::Color::TRANSPARENT,
            radius,
            deckkraft,
            schatten: node.style.schatten,
            schatten_anzahl: node.style.schatten_anzahl,
        });
        if radius_y != radius {
            list.set_vertical_radii_for_last(radius_y);
        }
    }

    //
    // BILD_49, Station 3: der **Verlauf** als eigener Eintrag — **nach der
    // `background-color`, vor dem Text**, wie es die Reihenfolge in CSS 2.1
    // Anhang E verlangt (Hintergrund, dann Inhalt).
    //
    // **UND WIE BEIM RAHMEN: `bx.border_box` (bzw. `mal`), NICHT `bx.background`.** GEMESSEN
    // in `flow.rs:269` — `background` ist `None`, sobald der Hintergrund
    // transparent ist. Ein Kasten mit `background-color: transparent;
    // background-image: linear-gradient(...)` waere sonst **ohne** Verlauf, und
    // genau das ist **jeder** Verlauf ueber transparentem Grund.
    //
    // **DIE FARBE STEHT IM DOM, nicht in der Box** — wie beim Hintergrund: der
    // Verlauf kommt aus `node.style.verlauf`, also aus **DER KASKADE**
    // (BILD_48 Station 2), nicht aus einer zweiten Berechnung hier.
    if let Some(node) = document.get(bx.node).filter(|_| bx.hintergrund_clip != 3) {
        if let Some(verlauf) = node.style.verlauf.as_ref() {
            let (rect, radius, radius_y) = flaeche(mal, node);
            if !rect.is_empty() {
                if let Some(kachel) = node.style.verlauf_kachel {
                    // Release 13.10.2026 (Agent CSS2): Kacheln nach
                    // `background-size`/`-position`/`-repeat`, gelegt in die
                    // Innenkante des Rahmens (`background-origin: padding-box`),
                    // geschnitten auf die Malflaeche (CSS Backgrounds 3 §3.5).
                    let rahmen = &node.style.rahmen;
                    let [mut o, mut re, mut u, mut l] = [0, 1, 2, 3].map(|i| rahmen.gezeichnete_breite(i));
                    // Release 13.10.2026 (Agent CSS2): `background-origin`.
                    match kachel.ursprung {
                        1 | 3 => (o, re, u, l) = (0.0, 0.0, 0.0, 0.0),
                        2 => {
                            let p = node.style.padding;
                            let b = mal.width;
                            o += p.top.resolve_or(b, 0.0).max(0.0);
                            re += p.right.resolve_or(b, 0.0).max(0.0);
                            u += p.bottom.resolve_or(b, 0.0).max(0.0);
                            l += p.left.resolve_or(b, 0.0).max(0.0);
                        }
                        _ => {}
                    }
                    let lage = narsil_core::geometry::Rect::new(
                        mal.x + l,
                        mal.y + o,
                        (mal.width - l - re).max(0.0),
                        (mal.height - o - u).max(0.0),
                    );
                    for k in crate::hintergrund::verlauf_kacheln(lage, rect, &kachel) {
                        let innen = k.x >= rect.x
                            && k.y >= rect.y
                            && k.x + k.width <= rect.x + rect.width
                            && k.y + k.height <= rect.y + rect.height;
                        list.push_mit_clip(
                            DisplayItem::Verlauf { rect: k, radius: [0.0; 4], verlauf: verlauf.clone() },
                            (!innen).then_some(rect),
                        );
                    }
                } else {
                    list.push(DisplayItem::Verlauf {
                        rect,
                        radius,
                        verlauf: verlauf.clone(),
                    });
                    if radius_y != radius {
                        list.set_vertical_radii_for_last(radius_y);
                    }
                }
            }
        }
    }

    // Release 14.10.2026 (Agent CSS2): background images (and the image of a
    // replaced element, which sits inside the border) come BEFORE the border
    // (CSS 2.1 Appendix E: background colour, background image, border).
    // MEASURED before (WPT css-backgrounds `background-origin/origin-border-
    // box`, a 40 % transparent border over a border-box image): the image
    // covered the border.
    let bilder_anfang = list.len();
    bilder(bx, list);
    // Release 15.10.2026: the images of this box (its replaced content and
    // its background images) take the box's `opacity` and are clipped to its
    // rounded corners. MEASURED before: `<img style="border-radius:50%">`
    // stayed square and `opacity` on an `<img>` had no effect (round avatars
    // on github.com, youtube.com and others).
    // The mask is the painting area of these images: the background-clip
    // area with its radii for a box (`flaeche`, as for the background
    // colour), the content box with the inner radii for a replaced element.
    // MEASURED before this correction (Agent B, WPT css-backgrounds
    // background-origin/origin-padding-box_with_radius): the outer radius on
    // the border box let 6 px of a padding-box image show in the corners.
    if let Some(node) = document.get(bx.node) {
        let (bereich, radius, radius_y) = if node.ersetzt.is_some() {
            let c = bx.content_box;
            let (r, ry) = node.style.corner_radii(mal.width, mal.height);
            let (dl, dob) = (c.x - mal.x, c.y - mal.y);
            let dr = mal.x + mal.width - (c.x + c.width);
            let du = mal.y + mal.height - (c.y + c.height);
            let (ix, iy) = inner_corners(r, ry, dl, dob, dr, du);
            (c, ix, iy)
        } else {
            flaeche(mal, node)
        };
        list.group_from_with_corners(bilder_anfang, bereich, deckkraft, radius, radius_y);
    }
    //
    // BILD_34: der **Rahmen** als eigener Eintrag, zwischen Hintergrund und
    // Inhalt (CSS 2.1 Anhang E: Hintergrund, dann Rahmen, dann Inhalt).
    //
    // **UND ES STEHT HIER EIGENTLICH NEBEN `bx.background`, NICHT DARIN:**
    // GEMESSEN in `flow.rs:269` — `background` ist `None`, wenn der
    // Hintergrund transparent ist (`is_transparent()`). Ein Kasten mit
    // `background: transparent; border: 1px solid #ccc` — **das ist genau das
    // Eingabefeld auf github und das Suchfeld von DuckDuckGo** — haette also
    // **keinen** Hintergrund und darum **keinen** Rand. **Deshalb `bx.border_box` (bzw. `mal`)
    // und nicht `bx.background`:** der Rahmen ist eine Eigenschaft des
    // Kastens, nicht der Fuellung.
    //
    // **`ist_gezeichnet()` ist die Bedingung**, weil `border-style: none` die
    // Breite nicht gelten laesst (CSS Backgrounds 3 §4.1) — `Border` mit
    // `breite: 0` waere ein Ring der Breiche 0, und `rahmen::zeichne` gibt
    // dafuer 0 zurueck, aber das Item stuende trotzdem in der Liste.
    if let Some(node) = document.get(bx.node) {
        if node.style.rahmen.ist_gezeichnet() && !mal.is_empty() && !node.style.border_image {
            let mut rahmen = node.style.rahmen;
            if deckkraft < 1.0 {
                for f in rahmen.farbe.iter_mut() {
                    f.a = alpha(f.a);
                }
            }
            let (radius, radius_y) = node.style.corner_radii(mal.width, mal.height);
            list.push(DisplayItem::Rand {
                rect: mal,
                rahmen,
                // BILD_32: der Radius gehoert zum Ring, nicht zur Fuellung —
                // die AUSSENkanten des Rings tragen dieselben 24 px.
                radius,
            });
            if radius_y != radius {
                list.set_vertical_radii_for_last(radius_y);
            }
        }
    }

    // Release 13.10.2026 (Agent CSS2): `outline` — ein Ring ausserhalb der
    // Rahmenkante (CSS UI 4 §6), mit dem Kasten gezeichnet.
    // Release 14.10.2026 (Agent CSS2): text runs carry their element's style
    // but draw no ring of their own.
    if let Some(node) = document.get(bx.node)
        && node.kind != narsil_dom::NodeKind::Text
        && let Some((breite, farbe)) = node.style.umriss
        && breite > 0.0
        && !mal.is_empty()
    {
        let mut farbe = farbe;
        farbe.a = alpha(farbe.a);
        // Release 15.10.2026 (Agent F): the ring follows the box's rounded
        // corners, `breite` further out (as in browsers).
        let (rx, ry) = node.style.corner_radii(mal.width, mal.height);
        let grow = |v: f32| if v > 0.0 { v + breite } else { 0.0 };
        let (rx, ry) = (rx.map(grow), ry.map(grow));
        list.push(DisplayItem::Rand {
            rect: narsil_core::geometry::Rect::new(
                mal.x - breite,
                mal.y - breite,
                mal.width + 2.0 * breite,
                mal.height + 2.0 * breite,
            ),
            rahmen: narsil_core::style::Rahmen {
                breite: [breite; 4],
                stil: [narsil_core::style::RahmenStil::Durchgezogen; 4],
                farbe: [farbe; 4],
            },
            radius: rx,
        });
        if ry != rx {
            list.set_vertical_radii_for_last(ry);
        }
    }
    if let Some(run) = &bx.text {
        if !run.text.is_empty() {
            list.push(DisplayItem::Text {
                origin: run.origin,
                text: run.text.clone(),
                // Release 11.10.2026: durchsichtige Schrift unter
                // `background-clip: text` in der Farbe des Hintergrunds.
                color: match text_ersatz {
                    Some(f) if run.color.a < 16 => narsil_core::color::Color { a: alpha(f.a), ..f },
                    _ => narsil_core::color::Color { a: alpha(run.color.a), ..run.color },
                },
                font_size: run.font_size,
                deko: run.deko,
                gewicht: run.gewicht,
                kursiv: run.kursiv,
                // 04.10.2026 (SCHRIFT, (c)): die Familie mit — sie ist
                // das, was der Raster zur Wahl der Datei braucht.
                schriftfamilie: run.schriftfamilie.clone(),
            });
            // Release 12.10.2026 (Agent CSS2): `letter-spacing`/`word-spacing`
            // (CSS 2.1 §16.4) — der Lauf bleibt EIN Item, der Raster setzt die
            // Zeichen auseinander (`DisplayList::versaetze_bei`). GEMESSEN vorher
            // (Koordinator, `ui_bild::ui2_die_startseite_hat_suchfeld_kacheln_
            // und_verlauf`): die Ueberschrift „Narsil" kam als sechs Items an.
            if let Some(v) = &bx.abstands_versaetze {
                list.versaetze_fuer_letztes(v.clone());
            }
        }
    }
}

#[cfg(test)]
mod clip_tests {
    use super::*;
    use narsil_core::geometry::{Point, Rect, Size};
    use narsil_layout::plan::UeberlaufArt;

    #[test]
    fn nullhoehe_mit_verstecktem_ueberlauf_verdeckt_kind() {
        let doc = Document::new();
        let mut root = LayoutBox::new(doc.root(), Rect::new(0.0, 0.0, 100.0, 100.0));
        let mut container = LayoutBox::new(doc.root(), Rect::new(0.0, 10.0, 100.0, 0.0));
        container.ueberlauf = UeberlaufArt::Versteckt;
        container.content_box = Rect::new(0.0, 10.0, 100.0, 0.0);
        let mut child = LayoutBox::new(doc.root(), Rect::new(0.0, 10.0, 50.0, 20.0));
        child.text = Some(narsil_layout::TextRun {
            origin: Point::new(0.0, 10.0),
            text: "sichtbar?".into(),
            color: narsil_core::color::Color::BLACK,
            font_size: 16.0,
            deko: 0,
            // 04.10.2026 (SCHRIFT, (c)): BAUFIX E0063 nach `TextRun`.
            // Die Vorgabe der Kaskade (CSS Fonts 4 §4.2) — dieselbe
            // Familie, die ein Lauf ohne `font-family` bekommt.
            schriftfamilie: String::from("sans-serif"),
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `TextRun`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — dieselbe
            // Vorgabe wie in `Style`, aus der der Lauf gefuellt wird.
            gewicht: 400,
            kursiv: false,
        });
        container.children.push(child);
        root.children.push(container);
        let list = build_display_list(&LayoutTree::new(root, Size::new(100.0, 100.0)), &doc);
        let text_index = list
            .iter()
            .position(|item| matches!(item, DisplayItem::Text { .. }))
            .expect("Text im Layout");
        assert!(list.clip_at(text_index).is_some_and(|rect| rect.is_empty()));
    }
}
