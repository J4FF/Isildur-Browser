//! Anordnung der Kinder: Flow-Positionen in absoluten Koordinaten.
//!
//! ## Regeln der Version 0
//!
//! * `row`: Kinder nebeneinander von links nach rechts. Der Cursor steigt um
//!   `breite + margin links + margin rechts`.
//! * `column`: Kinder untereinander von oben nach unten. Der Cursor steigt um
//!   `hoehe + margin oben + margin unten`.
//! * Ein `page`-Knoten verhaelt sich IMMER wie `column`, unabhaengig von einer
//!   gesetzten `direction`.
//!
//! # Margin-Collapsing (Release 09.10.2026)
//!
//! In Knoten MIT Plan, die ihre Kinder als Blockfluss legen, fallen
//! senkrechte Raender zusammen wie in CSS 2.1, 8.3.1: zwei Geschwister mit
//! `margin: 10` haben 10 Pixel Abstand, und der Rand des ersten/letzten
//! Kindes wandert durch einen Block ohne Rahmen/Fuellung nach aussen
//! (`<body><p>` beginnt bei 16 px, nicht bei 8 + 16). Die Messphase
//! (`box::sum_children`) merkt sich, was wanderte (`Sizes::hochgezogen`),
//! diese Datei legt mit denselben Werten. Version-0-Dokumente ohne Plan
//! addieren weiter: zwei Geschwister mit `margin: 10` haben dort 20 Pixel.
//! GEMESSEN vorher: `<p>A</p><p>B</p>` stand 51 px auseinander (Chrome:
//! 34), jede Textseite war damit um ein Drittel zu lang.
//!
//! # Absolute Koordinaten
//!
//! Alle Positionen sind ABSOLUT im Viewport-Koordinatensystem (Ursprung oben
//! links), nicht relativ zum Eltern. Das vereinfacht den Rasterizer.
//!
//! # Kein Overflow-Clipping
//!
//! Kinder duerfen ueber die Box ihres Elterns hinausragen (z. B. bei
//! `width: 150%`). Version 0 clippt NICHT — das uebernimmt spaeter der
//! Rasterizer.
//!
//! # Keine Rekursion
//!
//! [`build_tree`] arbeitet mit einem expliziten Stack im HEAP und setzt den
//! Baum anschliessend Bottom-Up zusammen. 1000 verschachtelte Container
//! brauchen deshalb konstant viel Stapelspeicher.

use narsil_core::geometry::{Point, Rect, Size};
use narsil_core::style::Direction;
use std::collections::HashMap;
// L4 (02.10.2026 22:5x): `RasterKind` kommt dazu, weil §8.3 eine Angabe
// PRO KIND ist — `grid-row: span N` steht am Kind, und ohne den Typ kann
// `flow.rs` sie nicht weitergeben.
// 06.10. 13:43 — **`KindQuer` FEHLTE IM IMPORT.** *`plan.rs:319` erweitert
// `verteile_flex_mit_selbst` um `kinder_quer: &[KindQuer]` (plan.rs 13:33:47), und der
// Aufrufer in `flow.rs` baut seit 13:43:01 `Vec<KindQuer>` — aber `KindQuer` war an keiner
// Stelle dieses Crates importiert. **Ein Name, der im `use` fehlt, kann das Verhalten nicht
// ändern: es ist die kleinstmögliche Korrektur an diesem Fehler, und sie gehört hierher,
// weil sie die Folge der Signaturverbreiterung in einer fremden Datei ist.** *Nicht verändert:
//   der Wert von `aufstieg` — der wird in `plan.rs` 310-441 nachweislich null Mal gelesen
//   (`grep -cE 'kinder_quer\.|aufstieg'` -> 0). Sobald das jemand tut, muss `flow.rs:1890`
//   `aufstieg_von` wirklich liefern — der Wert 0.0 hier wäre dann eine Behauptung.*
use narsil_css::{FlexAusrichtung, FlexBuendig, FlexKind, KindQuer, RasterKind};
use narsil_dom::{NodeId, NodeKind};
use narsil_layout::plan::{Anzeige, FlexUmbruch, FloatArt, PositionArt};
use narsil_layout::{LayoutBox, TextRun};

use crate::box_sizing::{Sizes, get_node, resolve_spacing};
use crate::plan::{
    FloatZustand,
    Platzierung,
    flex_basis,
    platz_absolut,
    versatz_relativ,
    verteile_flex_mit_selbst,
    // L4 (02.10.2026 08:1x): `verteile_raster` wird hier nicht mehr
    // importiert — der Laufweg nimmt `verteile_raster_bei`. Die kurze
    // Funktion bleibt oeffentlich, weil `tests/layout_echt.rs` sie benutzt
    // und ihr Verhalten behauptet.
    verteile_raster_bei,
    // L4 (02.10.2026 11:3x): der Laufweg nimmt jetzt
    // `verteile_raster_bei_fluss`, weil `grid-auto-flow` durchgereicht
    // wird. Die kurze Funktion bleibt importiert, weil
    // `tests/layout_echt.rs` sie benutzt und ihr Verhalten behauptet.
    verteile_raster_bei_spann, verteile_raster_mit_spuren,
};
use crate::{Ctx, LayoutError};

/// Positionsaufgabe: ein Knoten samt Ausgangspunkt und Elternknoten.
struct PlaceTask {
    id: narsil_dom::NodeId,
    /// Content-Box des Elterns (Referenz fuer Prozente und Auto-Regeln).
    available: Size,
    /// Linke obere Ecke der MARGIN-BOX.
    origin: Point,
    /// Index des Eltern-Eintrags in der Eintragsliste.
    parent: Option<usize>,
    /// Die Content-Box des naechsten POSITIONIERTEN Vorfahren in
    /// absoluten Koordinaten (CSS 2.1, 10.1). `None` = keiner, dann ist
    /// der enthaltende Block die Ansicht.
    ///
    /// W7 L3: Das ist der ganze Punkt von `position:absolute` — die Box
    /// steht nicht bei ihrem Eltern, sondern bei diesem Vorfahren. Ohne
    /// dieses Feld gaebe es keinen Unterschied zwischen einem
    /// positionierten und einem statischen Eltern, und ein `absolute`
    /// waere nur ein Versatz.
    enthaltend: Option<Rect>,
    /// W7 L1/L2: die Groesse, die der ELTERN festgelegt hat
    /// (`lege_flex`/`lege_raster`). `Some` heisst: die gemessene Groesse
    /// aus der Messphase gilt hier NICHT, sondern diese.
    ///
    /// GEMESSENER FEHLER 30.09.2026: ohne dieses Feld blieb ein Kind in
    /// einer `1fr`-Spalte 0 Pixel breit. Der Eltern hatte ihm 226,7 px
    /// gegeben, aber der Kasten des Kindes wurde weiter aus der
    /// Messphase gebaut — und die hatte `width:auto` gesehen. Das Kind
    /// war unsichtbar, obwohl die Grid-Zeile richtig stand.
    groesse: Option<Size>,
    /// Release 12.10.2026 (Agent CSS2): der Rollbereich (Innenabstandsbox) des
    /// naechsten Vorfahren mit `overflow` ungleich `visible` — das Rechteck,
    /// an dem `position: sticky` klebt (CSS Position 3 §3.4). `None` = die
    /// Ansicht.
    rollfeld: Option<Rect>,
}

/// Ein bereits positionierter Knoten, noch ohne Kinderboxen.
struct Entry {
    id: narsil_dom::NodeId,
    border: Rect,
    content: Rect,
    background: Option<Rect>,
    text: Option<TextRun>,
    /// Release 12.10.2026 (Agent CSS2): siehe `LayoutBox::abstands_versaetze`.
    versaetze: Option<Box<[(u32, f32)]>>,
    parent: Option<usize>,
    /// `position` und `z-index` (P8, 02.10.2026).
    ///
    /// DER PLAN WIRD HIER EINMAL GELESEN, nicht je Kasten gestapelt:
    /// `Ctx::stil_oder_standard` klont, und in der Tiefensuche mit bis zu
    /// 50 000 Knoten waere ein `LayoutStil`-Klon je Knoten der teuerste
    /// Posten der ganzen Phase. Es sind ZWEI Felder, also wird
    /// `Ctx::hole` benutzt — das leiht.
    position: narsil_layout::PositionArt,
    z_index: Option<i32>,
    /// `display: flex` (L5, 13:2x, Flexbox 1 §5.4).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier und nicht erst beim
    /// Zeichnen ermittelt: `build_display_list` bekommt Baum und
    /// Dokument, nicht den Plan. Der Weg vom Kind zum Elternknoten gibt
    /// es in `LayoutBox` nicht. Hier ist der Plan da — also ist dies die
    /// LETZTE Stelle, an der die Information aus dem Plan in den Baum
    /// wandern kann.
    ist_flex_container: bool,
    /// `transform`/`filter` erzeugen einen Stapelkontext (L5, 17:0x).
    transformt: bool,
    /// Release 11.10.2026: die Transformation selbst.
    transform: Option<Box<narsil_layout::Transformation>>,
    /// Release 11.10.2026: `background-clip`.
    hintergrund_clip: u8,
    ueberlauf: narsil_layout::plan::UeberlaufArt,
    /// Release 11.10.2026 (Agent CSS2): `float` fuer die Mal-Reihenfolge.
    schwebt: bool,
    /// Release 11.10.2026 (Agent CSS2): siehe `LayoutBox::mal_kasten`.
    mal_kasten: Option<Rect>,
    /// Release 11.10.2026: `order` — Flex- und Rasterelemente malen in
    /// `order`-Reihenfolge (CSS Display 3 „order-modified document order").
    ordnung: i32,
}

/// Baut den Layout-Baum: Positionsphase, iterativ.
///
/// # Fehler
///
/// [`LayoutError::UnknownNode`] und [`LayoutError::InvalidLength`].
/// Kein Panic und kein Stack-Overflow.
/// K17b: verschiebt die Teile EINER Zeile nach rechts (oder zur Mitte).
///
/// # WAS VERSCHOBEN WIRD
///
/// `aufgaben[von..]` — die `PlaceTask`s, die zu dieser Zeile gehoeren.
/// Der Versatz gilt fuer die ganze Zeile, nicht fuer jeden Teil
/// einzeln: eine Zeile aus drei Teilen wuerde sonst jeden Teil einzeln
/// nach rechts ruecken, und das sieht kaputt aus.
///
/// # DIE DREI FAELLE
///
/// * `Links`   — nichts. Das ist der Normalfall und der Grund, warum
///   das hier gefahrlos ist.
/// * `Rechts`  — `breite_inhalt - breite_zeile`. Bei `direction: rtl`
///   ist `start` genau das, weil die Kaskade `start` zu `Rechts`
///   aufgeloest hat, BEVOR es hier ankommt.
/// * `Zentriert` — die Haelfte des Restes.
///
/// # ZWEI RECHNUNGEN, DIE ICH FALSCH HATTE
///
/// * Negative Breite: wenn die Zeile breiter ist als der Inhalt
///   (Umbruch vor dem ersten Kind), waere der Versatz negativ und
///   wuerde die Zeile nach links schieben. `max(0.0)` verhindert das.
/// * Kein Versatz fuer eine leere Zeile: `breite_zeile == 0` heisst
///   „nichts geschrieben", und dann gibt es nichts zu verschieben.
///
/// # M3
///
/// Der Versatz ist auf `MAX_ZEILENVERSATZ` begrenzt: eine kaputte
/// Breitenangabe darf den Versatz nicht auf einen Wert von 1e30
/// treiben, der die Box aus dem Bild schiebt (M4).
fn zeile_ausrichten(
    aufgaben: &mut [PlaceTask],
    von: usize,
    breite_inhalt: f32,
    breite_zeile: f32,
    ausrichtung: narsil_layout::plan::Textausrichtung,
) {
    use narsil_layout::plan::Textausrichtung as A;
    if von >= aufgaben.len() || !breite_inhalt.is_finite() || !breite_zeile.is_finite() {
        return;
    }
    let rest = breite_inhalt - breite_zeile;
    if rest <= 0.0 {
        return;
    }
    let versatz = match ausrichtung {
        A::Links | A::Blocksatz => return,
        A::Rechts => rest,
        A::Zentriert => rest / 2.0,
    };
    let versatz = versatz.clamp(0.0, MAX_ZEILENVERSATZ);
    if versatz <= 0.0 {
        return;
    }
    for a in aufgaben.iter_mut().skip(von) {
        a.origin.x = (a.origin.x + versatz).min(MAX_ZEILENVERSATZ * 1000.0);
    }
}

/// Obergrenze fuer den Zeilenversatz (M3/M4).
///
/// Begruendung: eine kaputte `width` im Plan darf keinen Versatz von
/// 1e30 erzeugen. 100 000 px ist weiter, als ein Bild mit diesem
/// Browser sichtbar ist, also ist der Wert fuer das Layout gleich und
/// fuer die Rechnung endlich.
const MAX_ZEILENVERSATZ: f32 = 100_000.0;

pub(crate) fn build_tree(
    ctx: &Ctx<'_>,
    root: narsil_dom::NodeId,
    viewport: Size,
    sizes: &Sizes,
) -> Result<LayoutBox, LayoutError> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut stack = vec![PlaceTask {
        id: root,
        available: viewport,
        origin: Point::ZERO,
        parent: None,
        enthaltend: None,
        groesse: None,
        rollfeld: None,
    }];

    while let Some(task) = stack.pop() {
        let node = get_node(ctx, task.id)?;
        let size = sizes
            .get(task.id)
            .ok_or(LayoutError::UnknownNode(task.id))?;
        let margin = resolve_spacing(node.style.margin, task.available.width, task.id, "margin")?;
        let padding =
            resolve_spacing(node.style.padding, task.available.width, task.id, "padding")?;

        // W7 L1/L2: der Eltern hat die Groesse festgelegt (Flex/Raster),
        // dann gilt sie statt der aus der Messphase.
        let mut size = Size::new(size.width, size.height);
        if let Some(g) = task.groesse {
            size = Size::new(g.width, g.height);
        }
        //
        // GEMESSENER FEHLER 05.10.2026, 08:52: der Rand wird HIER
        // addiert — und der Plan-Weg in `kind_platzieren` hat ihn auch
        // schon addiert (`x: q.x + cm.left`). GEMESSEN: `margin-left:
        // 12 px` an einem Text in einer Flex-Reihe ergab x = 24 statt
        // x = 12.
        //
        // Der alte Weg addiert ihn NICHT (`Point::new(content.x, oben)`),
        // und deshalb stimmt `tests/margin_text.rs` seit dem 01.10.:
        // es geht ueber den alten Weg.
        //
        // CSS 2.1, 8.3.1: der Rand steht ausserhalb der Breite, und er
        // rueckt den Kasten genau einmal nach rechts. Wer ihn zweimal
        // addiert, rueckt ihn zweimal.
        let mut border = Rect::from_origin_size(
            Point::new(task.origin.x + margin.left, task.origin.y + margin.top),
            size,
        );
        // Release 11.10.2026 (Agent CSS2): der Kasten einer Tabelle traegt
        // ihre Beschriftungen mit (CSS 2.1 §17.4, „table wrapper box“); ihr
        // Rahmen und Hintergrund gehoeren nur dem Raster. Die Rahmenbox des
        // Kastens bleibt die Huelle (GEMESSEN vorher, WPT css-grid
        // `table-grid-item-005`: `offsetHeight` 381 statt 400 — ohne die
        // Beschriftung), gezeichnet wird am Raster (`mal_kasten`).
        let huelle = border;
        if let Some(l) = sizes.tabellen_lage(task.id) {
            border.y += l.raster_oben;
            border.height = l.raster_hoehe;
        }
        let mal_kasten = (border != huelle).then_some(border);
        let padding_box = border.inset(crate::r#box::rahmen_edges(&node.style.rahmen));
        let mut content = padding_box.inset(padding);
        // Release 11.10.2026 (Agent CSS2): `vertical-align` einer Zelle
        // verschiebt ihren Inhalt (CSS 2.1 §17.5.3).
        if let Some(dy) = sizes.zellen_versatz(task.id) {
            content.y += dy;
        }

        let index = entries.len();
        entries.push(Entry {
            id: task.id,
            border: huelle,
            mal_kasten,
            content,
            // Transparenter Hintergrund = kein Hintergrund; die Display-Liste
            // prueft das zusaetzlich (Defensiv-Regel aus TASK_0004).
            background: if node.style.background.is_transparent() {
                None
            } else {
                Some(border)
            },
            text: if node.kind == NodeKind::Text {
                crate::text::make_text_run(node, content)
            } else {
                None
            },
            versaetze: if node.kind == NodeKind::Text { abstands_versaetze(ctx, node) } else { None },
            parent: task.parent,
            // P8: aus dem Plan. `hole` leiht, `stil_oder_standard`
            // klont — siehe das Feld.
            position: ctx
                .plan
                .and_then(|p| p.hole(task.id))
                .map_or(narsil_layout::PositionArt::Statisch, |st| st.position),
            z_index: ctx
                .plan
                .and_then(|p| p.hole(task.id))
                .and_then(|st| st.z_index),
            // L5 (13:2x, Flexbox 1 §5.4): der Kasten traegt die Antwort
            // mit, weil `build_display_list` den Plan NICHT bekommt (siehe
            // das Feld `position` in narsil-layout) und `LayoutBox` keinen
            // Zeiger auf den Elternknoten hat.
            // Release 11.10.2026: auch Rasterelemente — `z-index` gilt fuer sie
            // ohne `position` (CSS Grid 1 §9.? „Z-axis Ordering"). GEMESSEN
            // vorher: WPT `grid-items/grid-z-axis-ordering-*`.
            ist_flex_container: ctx.plan.and_then(|p| p.hole(task.id)).is_some_and(|st| {
                matches!(st.anzeige, narsil_layout::plan::Anzeige::Flex | narsil_layout::plan::Anzeige::Raster)
            }),
            // Release 11.10.2026: `order` gilt nur fuer Flex-/Rasterelemente
            // im Fluss, nicht fuer absolut positionierte Kinder (CSS Flexbox 1
            // §4.1). GEMESSEN vorher: WPT `flexbox-paint-ordering-003`.
            ordnung: ctx
                .plan
                .and_then(|p| p.hole(task.id))
                .filter(|st| !matches!(st.position, PositionArt::Absolut | PositionArt::Fest))
                .map_or(0, |st| st.ordnung),
            transformt: ctx
                .plan
                .and_then(|p| p.hole(task.id))
                .is_some_and(|st| st.transformt),
            transform: ctx
                .plan
                .and_then(|p| p.hole(task.id))
                .and_then(|st| st.transform)
                .map(Box::new),
            hintergrund_clip: ctx.plan.and_then(|p| p.hole(task.id)).map_or(0, |st| st.hintergrund_clip),
            ueberlauf: ctx
                .plan
                .and_then(|p| p.hole(task.id))
                .map_or(narsil_layout::plan::UeberlaufArt::Sichtbar, |st| st.ueberlauf),
            schwebt: ctx
                .plan
                .and_then(|p| p.hole(task.id))
                .is_some_and(|st| st.schweben != narsil_layout::plan::FloatArt::Keins),
        });

        // Textknoten sind Blaetter (Spezifikation E13), auch wenn versehentlich
        // Kinder haengen: sie werden nicht positioniert.
        if node.kind == NodeKind::Text {
            continue;
        }

        // =================================================================
        // W7 L1–L4: FLEX, RASTER, POSITION, FLOAT
        //
        // DER WEG ENTSCHEIDET: `display:none` und `position:absolute`/
        // `fixed` verlassen den Fluss. `flex` und `grid` bekommen ihre
        // Kinder von `lege_flex_zeilen` bzw. `lege_raster` gestellt.
        // Alles andere laeuft unveraendert weiter (Block mit Cursor).
        //
        // `aus_ist` ist die gemeinsame Rueckgabe: die PLATZIERUNG
        // jedes Kindes plus die Liste der Aufgaben, die der Stack noch
        // abzuarbeiten hat. Sie ist an `MAX_KINDER` gebunden.
        // =================================================================
        if let Some(aufgaben) = kind_platzieren(
            ctx,
            node,
            task.id,
            index,
            &content,
            &padding_box,
            sizes,
            task.enthaltend,
            viewport,
            task.rollfeld,
        )? {
            match aufgaben {
                PlatzErgebnis::Fluss { tasks } => {
                    for t in tasks.into_iter().rev() {
                        stack.push(t);
                    }
                }
                PlatzErgebnis::Fertig => {}
            }
            continue;
        }

        // Ein page-Knoten verhaelt sich wie column.
        let direction = if node.kind == NodeKind::Page {
            Direction::Column
        } else {
            node.style.direction
        };

        // K17b: die AUSRICHTUNG dieses Knotens aus dem Plan. Sie kommt
        // aus `narsil_css::layout_plan_setze`, das sie je Knoten in die
        // Nebentabelle schreibt. Ohne Planeintrag gilt `Links` — und
        // damit bewegt sich nichts, das sich vorher bewegt hat.
        let ausrichtung = ctx
            .plan
            .as_ref()
            .map_or(narsil_layout::plan::Textausrichtung::Links, |p| {
                p.ausrichtung(task.id)
            });

        // Kinder-Positionen JETZT berechnen: die Groessen liegen aus der
        // Messphase vor, der Cursor laeuft wie in einem normalen Flow.
        let mut cursor = match direction {
            Direction::Column => content.y,
            Direction::Row => content.x,
        };
        // P9: derselbe Inline-Lauf wie im Plan-Weg. ZWEI Wege, EIN
        // Verhalten — sonst haengt die Frage davon ab, ob der Elternknoten
        // einen Planeintrag hat, und das ist eine Frage ueber den Stil,
        // nicht ueber das Layout.
        //
        // GEMESSENER FEHLER, als nur der Plan-Weg ihn hatte: in
        // `tests/inline_lauf.rs` stapelten die Inline-Bloecke untereinander
        // (y = 0 / 30 / 60), obwohl der Plan korrekt an allen drei
        // Kindern stand. Der Absatz selbst hatte keinen Planeintrag, also
        // lief der alte Weg — und der kannte nur Bloecke.
        let mut zeile = ZeilenCursor::neu(content.x, content.y);
        let mut child_tasks = Vec::with_capacity(node.children.len());
        // K17b (02.10.2026, 18:55, Freigabe aus `team/AUTONOM.txt`):
        // `text-align: right|start@rtl` und `center` verschieben die
        // ZEILE, nicht den einzelnen Teil.
        //
        // GEMESSENE GRUNDLAGE (und warum es ueberhaupt geht): Die Kinder
        // einer Zeile liegen alle in `child_tasks`, wenn die Zeile
        // bricht — sie werden erst DANACH als `Entry` geschrieben
        // (unten: `for child_task in child_tasks.into_iter().rev()`).
        // Ich hatte previously behauptet, der Versatz sei „nicht
        // klein", weil die Eintraege noch nicht existierten. Das war
        // richtig beobachtet und falsch gedeutet: man muss die
        // `PlaceTask` verschieben, nicht die `Entry`.
        let mut zeilen_start: usize = 0;
        // P9 (07:50): die Nummer des Kindes und die Zahl der Kinder, weil
        // die Zeilenvorschau ab dem laufenden Kind zaehlt. Der alte Weg
        // begrenzt die Kinder NICHT (`take(n)` waere hier eine
        // Verhaltensaenderung), also zaehlt er alle.
        let n_alle = node.children.len();
        // Release 11.10.2026: der halbe Durchschuss (CSS 2.1 §10.8.1). In
        // einer Zeilenbox (Hoehe = `line-height`) stehen die Textlaeufe auf
        // EINER Grundlinie, und der Platz ueber dem hoechsten Aufstieg und
        // unter dem tiefsten Abstieg teilt sich zu gleichen Teilen.
        // GEMESSEN vorher: jeder Lauf stand oben in seiner Zeile —
        // `line-height: 40px` zum Zentrieren in Knoepfen hob den Text um
        // 12 px, Ahem mit `30px/4` stand 45 px zu hoch (WPT
        // `absolute-non-replaced-width-019` u. v. a.).
        // Bei `line-height: normal` (die Zeile ist so hoch wie die normale
        // Hoehe der groessten Schrift) gibt es keinen Durchschuss — dort nur
        // die gemeinsame Grundlinie. Unsere normale Hoehe ist ein
        // allgemeiner Wert (16 px -> 19 px); mit Durchschuss rutschte eine
        // 1-em-Schrift wie Ahem 2 px nach unten, waehrend Chrome die Zeile
        // 16 px hoch macht (GEMESSEN: 25 WPT-Kipper in CSS2).
        let durchschuss = if direction == Direction::Row { zeilen_durchschuss(ctx, task.id, node, &content) } else { None };
        for (i, &child_id) in node.children.iter().enumerate() {
            let child = get_node(ctx, child_id)?;
            let child_margin =
                resolve_spacing(child.style.margin, content.width, child_id, "margin")?;
            let child_size = sizes
                .get(child_id)
                .ok_or(LayoutError::UnknownNode(child_id))?;
            // Release 11.10.2026: Textlauf in einer Zeilenbox — sein Kasten
            // ist der Inhaltsbereich (Aufstieg + Abstieg, dazu Innenabstand
            // und Rahmen des Inline-Elements), nicht die ganze Zeile.
            let mut groesse_lauf: Option<Size> = None;
            let origin = match direction {
                Direction::Column => {
                    // DIE ZEILENHOEHE IST DIE DES BLOCKS, nicht die des
                    // Kindes.
                    //
                    // GEMESSENER FEHLER 30.09.2026, gefunden von
                    // `crates/narsil-page/tests/textfluss.rs` (I1): ein
                    // 14-px-`<code>` in einem 16-px-Absatz rueckte den
                    // Cursor um 14 px vor. Die naechste Zeile begann
                    // 2 px zu hoch, und die zwei Rechtecke
                    // schnitten sich — im Bild zwei Texte uebereinander.
                    //
                    // GEMESSENE REGEL (CSS 2.1, 12.2 "Line boxes"): die
                    // Hoehe einer Zeile wird bestimmt von den groessten
                    // Buchstaben IN der Zeile und den darauf
                    // gesetzten Werten. Vereinfacht auf den Block: ein
                    // kleinerer Text in einem Block verkleinert dessen
                    // Zeilenhoehe NICHT. Fuer `column` heisst das: ein
                    // Textknoten rueckt mindestens um die Schriftgroesse
                    // seines ELTERN vor.
                    //
                    // WARUM NUR FUER TEXT: ein Container mit `height: 10`
                    // in einem 16-px-Block soll 10 Pixel hoch sein. Seine
                    // Box ist eine Fläche, keine Zeile.
                    // Zeilenhoehe des Blocks (line-height) als Minimum fuer Text-Knoten.
                    let block_line_height = if node.style.zeilenhoehe_px.is_finite() && node.style.zeilenhoehe_px > 0.0 {
                        node.style.zeilenhoehe_px
                    } else {
                        node.style.font_size
                    };
                    let hoehe = if child.kind == NodeKind::Text {
                        child_size.height.max(block_line_height)
                    } else {
                        child_size.height
                    };
                    // P9: Inline-Teil? Dann in die Zeile, nicht darunter.
                    let cst = ctx.stil_oder_standard(child_id);
                    if inline_kind(ctx, child_id, &cst, &child.kind) {
                        // WAR DIE ZEILE LEER? Dann ist ein
                        // kollabierbares Leerzeichen am Zeilenanfang und
                        // belegt keinen Platz (CSS 2.1, 16.6.1).
                        //
                        // DIE BREITE KOMMT AUS DER MESSPHASE: die Breite
                        // eines Textknotens ist seine gemessene Textbreite,
                        // und die Messphase weiss nicht, in welcher Zeile er
                        // landet. Darum bekommt der Kasten hier eine
                        // eigene Groesse mit — `build_tree` (Zeile 133)
                        // nimmt `task.groesse` anstelle der gemessenen.
                        let zeile_war_leer = !zeile.gleich;
                        let kollabiert = kollabierer_leerraum(ctx, child_id);
                        // P9 (08:10): `allein` VOR `inline_teil` merken.
                        // `inline_teil` kann umbrechen, und der Umbruch
                        // setzt `allein` zurueck — GEMESSEN: das
                        // Leerzeichen stand trotzdem bei y = -1,4285717.
                        let allein = zeile.allein;
                        // P9 (07:50): beginnt die Zeile, wird sie VORAUS
                        // gesehen. Grundlinie und Zeilenhoehe stehen dann
                        // fest, BEVOR der erste Teil gesetzt wird — sonst
                        // kann ein spaeter kommendes hoeheres Teil einen
                        // bereits gesetzten nicht mehr heben (CSS Inline 3,
                        // §2.3). `inline_teil` rechnet sie nicht neu.
                        if zeile_war_leer {
                            // Zeilenhoehe aus dem Style des Blocks (line-height).
                            let line_height = if node.style.zeilenhoehe_px.is_finite() && node.style.zeilenhoehe_px > 0.0 {
                                node.style.zeilenhoehe_px
                            } else {
                                node.style.font_size
                            };
                            let v = vorschau(
                                ctx,
                                node,
                                i,
                                sizes,
                                line_height,
                                content.width,
                                n_alle,
                            );
                            zeile.setze_vorschau(v);
                        }
                        // K17b: Zustand VOR dem Setzen merken. Bricht die
                        // Zeile in `inline_teil`, ist danach `gleich`
                        // wieder false — und dann gehoeren die Teile ab
                        // `zeilen_start` zur gerade voll gewordenen
                        // Zeile.
                        let zeile_war_offen = zeile.gleich;
                        let breite_vorher = zeile.braucht;
                        let p = inline_teil(
                            ctx,
                            child_id,
                            &InlineTeil {
                                kind: &child.kind,
                                rand: child_margin,
                                breite: sauber_pos(child_size.width),
                                hoehe,
                                content_breite: content.width,
                            },
                            &mut zeile,
                            &mut cursor,
                        );
                        if zeile_war_offen && !zeile.gleich {
                            zeile_ausrichten(
                                &mut child_tasks,
                                zeilen_start,
                                content.width,
                                breite_vorher,
                                ausrichtung,
                            );
                        }
                        // CSS 2.1, 16.6.1: "any sequence of collapsible
                        // spaces at the beginning or end of a line is
                        // REMOVED". Nicht auf 0 px breit setzen —
                        // ENTFERNEN. Es gibt keinen Kasten.
                        //
                        // GEMESSENER FEHLER der Fassung mit Breite 0: das
                        // Leerzeichen bekam einen Kasten bei
                        // y = -1,4285717, also 1,43 px UEBER dem Anfang
                        // der Zeile. Grund: seine Grundlinie (11,43 px)
                        // liegt ueber der der Zeile (10 px), und mit
                        // Breite 0 ist es trotzdem ein Kasten, der
                        // sichtbar waere — er nur nichts anzeigte.
                        //
                        // `zeile_war_leer` heisst "erstes Teil der Zeile",
                        // `war_letzter` heisst "letztes Teil" — das letzte
                        // weiss erst die Vorschau.
                        if kollabiert
                            && (zeile_war_leer
                                || allein
                                || (zeile.letzter_leer && zeile.war_letzter()))
                        {
                            continue;
                        }
                        // K17b: GEHT DIESES TEIL DIE ZEILE AN? Dann beginnt
                        // hier der zu verschiebende Bereich — und er
                        // enthaelt nur Inline-Teile.
                        //
                        // GEMESSENER FEHLER, den die Tests gefunden haben:
                        // Ich hatte den Bereich am Zeilenbruch beginnen
                        // lassen. Damit kam ein BLOCK-Kind mit hinein
                        // und wurde nach rechts geschoben: `layout.rs`
                        // meldete `parent.width: erwartet 400, bekommen
                        // 440`. Eine Zeile besteht aus Inline-Teilen;
                        // ein Block hat seine eigene Zeile.
                        if zeile_war_leer {
                            zeilen_start = child_tasks.len();
                        }
                        Point::new(p.x, p.y)
                    } else {
                        // GEMESSENER FEHLER: `origin` stand hier schon
                        // fest — es wurde oben aus `cursor` gebildet, BEVOR
                        // die offene Inline-Zeile den Cursor weiterrueckte.
                        // Ein Block nach einem 30-px-Inline-Teil stand
                        // dadurch bei y = 0, also OBEN auf der Zeile, die er
                        // doch beenden sollte. GEMESSEN: A y = 0,
                        // Block y = 0. Jetzt wird `origin` erst NACH dem
                        // Schluss gebildet.
                        zeile.schliesse_wenn_offen(&mut cursor);
                        let oben = cursor;
                        cursor += child_margin.top + hoehe + child_margin.bottom;
                        zeile.nach_block(cursor);
                        // K17b: ein Block beginnt seine eigene Zeile. Der
                        // Bereich fuer den Versatz beginnt also HINTER ihm,
                        // damit er nicht mitverschoben wird.
                        zeilen_start = child_tasks.len() + 1;
                        Point::new(
                            content.x
                                + auto_margin_versatz_x(
                                    child.style.margin,
                                    ctx.plan
                                        .and_then(|p| p.hole(child_id))
                                        .is_some_and(|s| s.marge_links_auto),
                                    ctx.plan
                                        .and_then(|p| p.hole(child_id))
                                        .is_some_and(|s| s.marge_rechts_auto),
                                    content.width,
                                    child_size.width,
                                    child_margin,
                                    ctx.plan.is_some_and(|p| p.richtung(task.id) == narsil_layout::Richtung::Rtl),
                                ),
                            oben,
                        )
                    }
                }
                Direction::Row => {
                    let (dy, g) = lauf_im_durchschuss(ctx, child, &content, durchschuss, child_size);
                    groesse_lauf = g;
                    let origin = Point::new(cursor, content.y + dy);
                    cursor += child_margin.left + child_size.width + child_margin.right;
                    origin
                }
            };
            child_tasks.push(PlaceTask {
                id: child_id,
                available: content.size(),
                origin,
                parent: Some(index),
                enthaltend: task.enthaltend,
                groesse: groesse_lauf,
                rollfeld: task.rollfeld,
            });
        }
        // P9: die letzte Inline-Zeile traegt die Hoehe des Eltern.
        //
        // K17b: vorher die letzte Zeile ausrichten — sie bricht hier,
        // und ihre Teile stehen ab `zeilen_start`.
        if zeile.gleich {
            zeile_ausrichten(
                &mut child_tasks,
                zeilen_start,
                content.width,
                zeile.braucht,
                ausrichtung,
            );
        }
        zeile.schliesse_wenn_offen(&mut cursor);
        // Umgekehrt anstossen, damit die Kinder in Dokumentreihenfolge
        // vom Stack genommen und in die Eintragsliste geschrieben werden.
        for child_task in child_tasks.into_iter().rev() {
            stack.push(child_task);
        }
    }

    assemble(&entries)
}

/// Release 11.10.2026: der halbe Durchschuss einer Zeilenbox (CSS 2.1
/// §10.8.1) — (groesster Aufstieg, groesster Abstieg, `line-height: normal`?)
/// ihrer Textlaeufe, `None`, wenn `node` keine Zeilenbox mit Text ist.
/// Aus `build_tree` gezogen, damit auch eine Zeilenbox MIT Planeintrag (die
/// erste Zeile eines `<p>`, ausgerichtete Zeilen) ihn bekommt. GEMESSEN
/// vorher (WPT `separated-border-model-007..009`): die Referenz
/// `p { font: 1em/2 serif }` stand 8 px zu hoch, der Test (ohne Plan) nicht.
pub(crate) fn zeilen_durchschuss(ctx: &Ctx<'_>, id: NodeId, node: &narsil_dom::Node, content: &Rect) -> Option<(f32, f32, bool)> {
    // Release 12.10.2026 (Agent CSS2): nur auf dem Weg mit Plan — die
    // Zeilenboxen des Seitenbaus kommen immer mit Plan; ein handgebautes
    // Dokument ohne Plan (Narsil-Sprache, `layout()`) hat Reihen, keine
    // Zeilen. GEMESSEN vorher (Koordinator, `layout::hello_narsil_datei_
    // laeuft_durch_parser_und_layout`): der Text einer 40-px-Reihe stand
    // mittig (92 statt 80).
    if ctx.plan.is_none() || !crate::r#box::ist_zeilenbox(ctx, id, node) {
        return None;
    }
    let mut a_max = 0.0f32;
    let mut d_max = 0.0f32;
    let mut fs_max = 0.0f32;
    for &k in node.children.iter() {
        if let Ok(n) = get_node(ctx, k)
            && n.kind == NodeKind::Text
            && n.text.as_deref().is_some_and(|t| !t.is_empty())
        {
            let (a, d) = ctx.measurer.zeilen_metrik(
                n.style.font_size,
                n.style.schriftgewicht,
                n.style.kursiv,
                &n.style.schriftfamilie,
            );
            // Release 11.10.2026 (Agent CSS2): ein gehobener Lauf reicht
            // um seine Hebung hoeher und weniger tief (`vertical-align`).
            let r = hebung(n);
            a_max = a_max.max(a + r);
            d_max = d_max.max(d - r);
            fs_max = fs_max.max(n.style.font_size);
        }
    }
    let normal_zeile = (content.height - crate::text::zeilenhoehe_normal(fs_max)).abs() < 0.5;
    // Release 11.10.2026 (Agent CSS2): auch eine Zeile der Hoehe 0
    // (`line-height: 0`): der Inhaltsbereich ragt dann zur Haelfte darueber
    // (negativer Durchschuss, §10.8.1). GEMESSEN vorher (WPT
    // `line-height-004` u. a.): der Text stand UNTER seiner 0-px-Zeile.
    (a_max + d_max > 0.0 && content.height.is_finite() && content.height >= 0.0).then_some((a_max, d_max, normal_zeile))
}

/// Release 11.10.2026 (Agent CSS2): die Hebung eines Textlaufs durch
/// `vertical-align` (siehe `Style::vertikal_hoch`).
fn hebung(n: &narsil_dom::Node) -> f32 {
    let r = n.style.vertikal_hoch;
    if r.is_finite() { r } else { 0.0 }
}

/// Release 11.10.2026 (Agent CSS2): die Hoehe einer Zeilenbox mit
/// gehobenen Laeufen — ihre Inline-Kaesten (jeder `line-height` hoch, um den
/// halben Durchschuss um seinen Inhaltsbereich gelegt) und der Strut des
/// Blocks, auf eine Grundlinie gestellt (CSS 2.1 §10.8.1). `None`, wenn kein
/// Lauf gehoben ist (dann bleibt die Hoehe des Seitenbaus).
pub(crate) fn zeile_mit_hebung(ctx: &Ctx<'_>, node: &narsil_dom::Node) -> Option<f32> {
    let mut gehoben = false;
    let mut oben = 0.0f32;
    let mut unten = 0.0f32;
    let mut kasten = |n: &narsil_dom::Node, r: f32| {
        let (a, d) = ctx.measurer.zeilen_metrik(n.style.font_size, n.style.schriftgewicht, n.style.kursiv, &n.style.schriftfamilie);
        if !(a + d).is_finite() {
            return;
        }
        let l = if n.style.zeilenhoehe_art != narsil_core::style::ZeilenhoeheArt::Normal && n.style.zeilenhoehe_px.is_finite() {
            n.style.zeilenhoehe_px
        } else {
            crate::text::zeilenhoehe_normal(n.style.font_size).max(a + d)
        };
        let halb = (l - (a + d)) / 2.0;
        oben = oben.max(halb + a + r);
        unten = unten.max(l - halb - a - r);
    };
    for &k in node.children.iter() {
        if let Ok(n) = get_node(ctx, k)
            && n.kind == NodeKind::Text
            && n.text.as_deref().is_some_and(|t| !t.is_empty())
        {
            let r = hebung(n);
            if r != 0.0 {
                gehoben = true;
            }
            kasten(n, r);
        }
    }
    if !gehoben {
        return None;
    }
    kasten(node, 0.0);
    let h = oben + unten;
    (h.is_finite() && h > 0.0).then_some(h)
}

/// Release 11.10.2026: senkrechter Versatz eines Kindes einer Zeilenbox im
/// halben Durchschuss und — ausser bei `normal` — sein Kasten (der
/// Inhaltsbereich: Aufstieg + Abstieg, Innenabstand und Rahmen des
/// Inline-Elements). Siehe [`zeilen_durchschuss`].
fn lauf_im_durchschuss(
    ctx: &Ctx<'_>,
    child: &narsil_dom::Node,
    content: &Rect,
    durchschuss: Option<(f32, f32, bool)>,
    child_size: Size,
) -> (f32, Option<Size>) {
    let Some((a_max, d_max, normal_zeile)) = durchschuss else {
        return (0.0, None);
    };
    if child.kind != NodeKind::Text {
        return (0.0, None);
    }
    let (a, d) = ctx.measurer.zeilen_metrik(
        child.style.font_size,
        child.style.schriftgewicht,
        child.style.kursiv,
        &child.style.schriftfamilie,
    );
    let px = |l: narsil_core::length::Length| match l {
        narsil_core::length::Length::Px(v) if v.is_finite() => v.max(0.0),
        _ => 0.0,
    };
    let (pt, pb) = (px(child.style.padding.top), px(child.style.padding.bottom));
    let (bt, bb) = (child.style.rahmen.gezeichnete_breite(0), child.style.rahmen.gezeichnete_breite(2));
    // Auf ganze Pixel wie die Grundlinie in Browsern — ein Bruchteil (bei
    // `normal` oft 0,2 px) rastete sonst je nach Umgebung verschieden.
    let halb = if normal_zeile { 0.0 } else { (content.height - (a_max + d_max)) / 2.0 };
    let glyphe_oben = (halb + (a_max - a - hebung(child))).round();
    let hoehe = a + d + pt + pb + bt + bb;
    if normal_zeile {
        // `normal`: nur die gemeinsame Grundlinie; der Kasten bleibt
        // zeilenhoch (unsere normale Hoehe ist noch keine Schriftmetrik).
        // Release 11.10.2026 (Agent CSS2): Innenabstand und Rahmen eines
        // Inline-Elements liegen UM den Text, sie verschieben ihn nicht (CSS
        // 2.1 §10.6.1) — der Kasten beginnt um sie hoeher. GEMESSEN vorher
        // (`<span style="padding:5px;border:2px solid">`): der Text stand
        // 7 px unter seiner Zeile.
        (if glyphe_oben.is_finite() { glyphe_oben - pt - bt } else { 0.0 }, None)
    } else if glyphe_oben.is_finite() && hoehe.is_finite() && hoehe > 0.0 {
        (glyphe_oben - pt - bt, Some(Size::new(child_size.width, hoehe)))
    } else {
        (0.0, None)
    }
}

/// Release 11.10.2026: der INHALTSBEREICH eines Textlaufs im Inline-Fluss
/// (CSS 2.1 §10.8.1): (Versatz seiner Oberkante unter der Oberkante der
/// `line-height`-Box der Hoehe `hoehe`, seine Hoehe — Aufstieg + Abstieg mit
/// Innenabstand und Rahmen des Inline-Elements —, Grundlinie ab der
/// Oberkante der `line-height`-Box). Nur bei AUSDRUECKLICHER `line-height`;
/// bei `normal` bleibt der Lauf zeilenhoch (siehe `zeilen_durchschuss`).
/// GEMESSEN vorher (WPT `inline-block-baseline-001`, `p { line-height: 5 }`
/// mit einem `inline-block` in der Zeile): die Woerter standen oben in ihrer
/// 75-px-Zeile statt 30 px tiefer auf der gemeinsamen Grundlinie.
pub(crate) fn text_inhaltsbereich(ctx: &Ctx<'_>, node: &narsil_dom::Node, hoehe: f32) -> Option<(f32, f32, f32)> {
    if node.kind != NodeKind::Text
        || node.style.zeilenhoehe_art == narsil_core::style::ZeilenhoeheArt::Normal
        || !hoehe.is_finite()
        || hoehe < 0.0
        || node.text.as_deref().is_none_or(str::is_empty)
    {
        return None;
    }
    let (a, d) = ctx.measurer.zeilen_metrik(
        node.style.font_size,
        node.style.schriftgewicht,
        node.style.kursiv,
        &node.style.schriftfamilie,
    );
    if !(a + d).is_finite() || a + d <= 0.0 {
        return None;
    }
    let px = |l: narsil_core::length::Length| match l {
        narsil_core::length::Length::Px(v) if v.is_finite() => v.max(0.0),
        _ => 0.0,
    };
    let (pt, pb) = (px(node.style.padding.top), px(node.style.padding.bottom));
    let (bt, bb) = (node.style.rahmen.gezeichnete_breite(0), node.style.rahmen.gezeichnete_breite(2));
    let halb = ((hoehe - (a + d)) / 2.0).round();
    Some((halb - pt - bt, a + d + pt + pb + bt + bb, halb + a))
}

/// Das Ergebnis der Plan-Platzierung: entweder Aufgaben fuer den Stack
/// oder „nichts mehr zu tun".
enum PlatzErgebnis {
    /// Die Kinder stehen im Fluss und werden vom Stack abgearbeitet.
    Fluss { tasks: Vec<PlaceTask> },
    /// Fertig: alle Kinder sind absolut gesetzt (oder unsichtbar).
    Fertig,
}

/// Verteilt den freien waagerechten Platz auf `auto`-Raender eines Blocks.
/// Der linke Rand wird erst in `build_tree` auf den Ursprung addiert;
/// hier kommt nur sein Anteil am noch nicht vergebenen Rest hinzu.
///
/// Release 10.10.2026: `eltern_rtl` — ist kein Rand `auto` (ueberbestimmt),
/// wird in einem `rtl`-Container der LINKE Rand ignoriert (CSS 2.1
/// §10.3.3), der Block steht rechts. GEMESSEN vorher (Web-Platform-Tests,
/// `absolute-replaced-width-034`): er stand links.
fn auto_margin_versatz_x(
    rand: narsil_core::length::Spacing,
    plan_links_auto: bool,
    plan_rechts_auto: bool,
    eltern_breite: f32,
    kind_breite: f32,
    aufgeloest: narsil_core::geometry::Edges,
    eltern_rtl: bool,
) -> f32 {
    let links = plan_links_auto || rand.left == narsil_core::length::Length::Auto;
    let rechts = plan_rechts_auto || rand.right == narsil_core::length::Length::Auto;
    let rest = eltern_breite - kind_breite - aufgeloest.left - aufgeloest.right;
    let frei = rest.max(0.0);
    if !links {
        // Release 12.10.2026 (Agent CSS2): ueberbestimmt in einem `rtl`-Block
        // weicht `margin-left` (CSS 2.1 §10.3.3) — auch ins Negative: der
        // Kasten steht `margin-right` vor der rechten Kante. GEMESSEN vorher
        // (WPT `margin-padding-clear/margin-right-008` u. a.): bei negativem
        // Rest stand er links am Rand.
        return if eltern_rtl && !rechts { if rest.is_finite() { rest } else { 0.0 } } else { 0.0 };
    }
    if rechts { frei / 2.0 } else { frei }
}

fn offset_prozent(anteil: f32, basis: f32) -> f32 {
    if !anteil.is_finite() || !basis.is_finite() {
        return 0.0;
    }
    (anteil * basis / 100.0).clamp(-crate::plan::MAX_LAGE_PX, crate::plan::MAX_LAGE_PX)
}

#[cfg(test)]
mod auto_margin_tests {
    use narsil_core::geometry::Size;
    use narsil_core::length::{Dimension, Length, Spacing};
    use narsil_core::style::Style;
    use narsil_dom::{Document, NodeKind, Span};

    #[test]
    fn block_mit_zwei_auto_raendern_steht_in_der_mitte() {
        let mut doc = Document::new();
        let root = doc.root();
        let kind = doc
            .push_child(root, NodeKind::Container, None, Span::EMPTY)
            .expect("Kind anlegbar");
        doc.set_style(
            kind,
            Style {
                size: Dimension::fixed(200.0, 20.0),
                margin: Spacing {
                    left: Length::Auto,
                    right: Length::Auto,
                    top: Length::px(0.0),
                    bottom: Length::px(0.0),
                },
                ..Style::default()
            },
        )
        .expect("Stil anlegbar");
        let baum = crate::layout(&doc, Size::new(400.0, 100.0)).expect("Layout");
        let kasten = &baum.root.children[0];
        assert_eq!(kasten.border_box.x, 100.0);
        assert_eq!(kasten.border_box.width, 200.0);
    }

    #[test]
    fn plan_auto_raender_zentrieren_trotz_numerischem_nullrand() {
        let mut doc = Document::new();
        let root = doc.root();
        let kind = doc
            .push_child(root, NodeKind::Container, None, Span::EMPTY)
            .expect("Kind anlegbar");
        doc.set_style(
            kind,
            Style {
                size: Dimension::fixed(200.0, 20.0),
                ..Style::default()
            },
        )
        .expect("Stil anlegbar");
        let mut plan = narsil_layout::LayoutPlan::leer();
        let mut s = narsil_layout::LayoutStil::standard();
        s.marge_links_auto = true;
        s.marge_rechts_auto = true;
        plan.set(kind, s);
        let baum = crate::layout_mit_plan(
            &doc,
            Size::new(400.0, 100.0),
            &crate::default_measurer(),
            &plan,
        )
        .expect("Layout");
        assert_eq!(baum.root.children[0].border_box.x, 100.0);
    }

    #[test]
    fn prozent_offsets_nutzen_positionierten_vorfahren() {
        let mut doc = Document::new();
        let root = doc.root();
        let eltern = doc
            .push_child(root, NodeKind::Container, None, Span::EMPTY)
            .expect("Eltern");
        let kind = doc
            .push_child(eltern, NodeKind::Container, None, Span::EMPTY)
            .expect("Kind");
        doc.set_style(
            eltern,
            Style {
                size: Dimension::fixed(300.0, 200.0),
                ..Style::default()
            },
        )
        .expect("Elternstil");
        doc.set_style(
            kind,
            Style {
                size: Dimension::fixed(50.0, 20.0),
                ..Style::default()
            },
        )
        .expect("Kindstil");
        let mut plan = narsil_layout::LayoutPlan::leer();
        let mut e = narsil_layout::LayoutStil::standard();
        e.position = narsil_layout::PositionArt::Relativ;
        plan.set(eltern, e);
        let mut k = narsil_layout::LayoutStil::standard();
        k.position = narsil_layout::PositionArt::Absolut;
        k.oben = Some(120.0); // absichtlich falsche Viewport-Naeherung
        k.rechts = Some(80.0);
        k.oben_prozent = Some(20.0);
        k.rechts_prozent = Some(10.0);
        plan.set(kind, k);
        let baum = crate::layout_mit_plan(
            &doc,
            Size::new(800.0, 600.0),
            &crate::default_measurer(),
            &plan,
        )
        .expect("Layout");
        let rect = baum.root.children[0].children[0].border_box;
        assert_eq!(rect.x, 220.0);
        assert_eq!(rect.y, 40.0);
    }

    #[test]
    fn float_beginnt_an_der_inhaltsbox_seines_elterns() {
        use narsil_layout::{FloatArt, LayoutPlan, LayoutStil};
        let mut doc = Document::new();
        let root = doc.root();
        let vor = doc
            .push_child(root, NodeKind::Container, None, Span::EMPTY)
            .expect("Vorgaenger");
        doc.set_style(
            vor,
            Style {
                size: Dimension::fixed(400.0, 50.0),
                ..Style::default()
            },
        )
        .expect("Stil Vorgaenger");
        let eltern = doc
            .push_child(root, NodeKind::Container, None, Span::EMPTY)
            .expect("Eltern");
        doc.set_style(
            eltern,
            Style {
                size: Dimension::fixed(200.0, 100.0),
                margin: Spacing {
                    left: Length::px(30.0),
                    right: Length::px(0.0),
                    top: Length::px(0.0),
                    bottom: Length::px(0.0),
                },
                ..Style::default()
            },
        )
        .expect("Stil Eltern");
        let schwimmer = doc
            .push_child(eltern, NodeKind::Container, None, Span::EMPTY)
            .expect("Float");
        doc.set_style(
            schwimmer,
            Style {
                size: Dimension::fixed(40.0, 20.0),
                ..Style::default()
            },
        )
        .expect("Stil Float");
        let mut plan = LayoutPlan::leer();
        plan.set(eltern, LayoutStil::standard());
        let mut stil = LayoutStil::standard();
        stil.schweben = FloatArt::Links;
        plan.set(schwimmer, stil);
        let baum = crate::layout_mit_plan(
            &doc,
            Size::new(400.0, 400.0),
            &crate::default_measurer(),
            &plan,
        )
        .expect("Layout");
        let kasten = &baum.root.children[1].children[0];
        assert_eq!(kasten.border_box.x, 30.0);
        assert_eq!(kasten.border_box.y, 50.0);
    }

    #[test]
    fn absolutes_kind_belegt_keinen_flex_platz() {
        use narsil_layout::plan::Anzeige;
        use narsil_layout::{LayoutPlan, LayoutStil, PositionArt};
        let mut doc = Document::new();
        let root = doc.root();
        let reihe = doc
            .push_child(root, NodeKind::Container, None, Span::EMPTY)
            .expect("Flex-Container");
        doc.set_style(
            reihe,
            Style {
                size: Dimension::fixed(300.0, 40.0),
                ..Style::default()
            },
        )
        .expect("Flex-Stil");
        let erstes = doc
            .push_child(reihe, NodeKind::Container, None, Span::EMPTY)
            .expect("erstes Kind");
        let absolut = doc
            .push_child(reihe, NodeKind::Container, None, Span::EMPTY)
            .expect("absolutes Kind");
        let zweites = doc
            .push_child(reihe, NodeKind::Container, None, Span::EMPTY)
            .expect("zweites Kind");
        for (id, breite) in [(erstes, 50.0), (absolut, 200.0), (zweites, 50.0)] {
            doc.set_style(
                id,
                Style {
                    size: Dimension::fixed(breite, 20.0),
                    ..Style::default()
                },
            )
            .expect("Kind-Stil");
        }
        let mut plan = LayoutPlan::leer();
        let mut flex = LayoutStil::standard();
        flex.anzeige = Anzeige::Flex;
        plan.set(reihe, flex);
        let mut abs = LayoutStil::standard();
        abs.position = PositionArt::Absolut;
        plan.set(absolut, abs);
        let baum = crate::layout_mit_plan(
            &doc,
            Size::new(300.0, 100.0),
            &crate::default_measurer(),
            &plan,
        )
        .expect("Layout");
        let kinder = &baum.root.children[0].children;
        assert_eq!(kinder[0].border_box.x, 0.0);
        assert_eq!(kinder[1].border_box.width, 200.0);
        assert_eq!(kinder[2].border_box.x, 50.0);
    }
}

/// Platziert die Kinder nach dem Plan. `None` heisst: normaler
/// Blockfluss, bitte den alten Weg gehen.
///
/// # DIE EINE SCHLEIFE, und warum es sie gibt
///
/// GEMESSENER FEHLER 30.09.2026: die erste Fassung hatte einen Zweig pro
/// Anzeigeart und las `left`/`top` aus dem Stil des ELTERN. Das ist
/// falsch — `left:30px` ist eine Angabe des BADGE, nicht seines Traegers.
/// Das Badge blieb bei x = 0.
///
/// Die Richtigfassung geht EINMAL ueber die Kinder und fragt fuer jedes
/// einzelne: was sagt der Plan ueber DIESES Kind?
///
/// 1. `display:none` -> das Kind wird nicht gebaut.
/// 2. `position:absolute`/`fixed` -> gegen den enthaltenden Block, aus
///    dem Fluss (CSS 2.1, 9.5: es belegt keinen Platz fuer die
///    Geschwister).
/// 3. Eltern ist `flex`/`grid` -> `verteile_flex`/`verteile_raster`.
/// 4. Kind schwebt -> `FloatZustand`.
/// 5. sonst der lineare Cursor, wie am 30.09. davor.
///
/// # DER ENTHALTENDE BLOCK
///
/// `inhalt` ist die Content-Box des naechsten positionierten Vorfahren in
/// absoluten Koordinaten, `None` = keiner, dann die Ansicht. Das Feld
/// wird beim Stapeln weitergegeben: ein positionierter Knoten macht sich
/// fuer seine Kinder zum enthaltenden Block, ein statischer reicht den
/// seines Elterns weiter (CSS 2.1, 10.1).
#[allow(clippy::too_many_arguments)]
fn kind_platzieren(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    id: narsil_dom::NodeId,
    index: usize,
    content: &Rect,
    padding_box: &Rect,
    sizes: &Sizes,
    inhalt: Option<Rect>,
    viewport: Size,
    rollfeld: Option<Rect>,
) -> Result<Option<PlatzErgebnis>, LayoutError> {
    let eltern_stil = match ctx.stil(id) {
        Some(s) => s.clone(),
        // Kein Plan fuer diesen Knoten: der alte Weg. Das ist der Fall
        // von vor dem 30.09. und der Grund fuer die Rueckwaerts-
        // Kompatibilitaet.
        None => return Ok(None),
    };

    // 1. `display:none` am ELTEN: der ganze Kasten entfaellt.
    if !eltern_stil.wird_gezeichnet() {
        return Ok(Some(PlatzErgebnis::Fertig));
    }

    // W7 L3: was dieser Knoten fuer seine Kinder ist.
    let weiter = match eltern_stil.position {
        PositionArt::Statisch => inhalt,
        _ => Some(*padding_box),
    };
    // Ohne positionierten Vorfahren ist der enthaltende Block die Ansicht
    // (CSS 2.1, 10.1).
    //
    // L5 (02.10.2026 12:4x): FUER DIE KINDER IST ES `weiter`, NICHT
    // `inhalt`.
    //
    // GEMESSENER BEFUND (`examples/l5_position.rs`, A2 und A3, Kasten
    // 600x400 mit einem positionierten Kind 300x200 darin):
    //
    //     A2 `top:20; right:30`   GEMESSEN x = 518   RICHTIG waere 218
    //     A3 `bottom:20; left:30`  GEMESSEN y = 388   RICHTIG waere 188
    //
    // GEMESSENE URSACHE, und sie ist eine Zeile: `behaelter` ist der
    // enthaltende Block fuer `node` SELBST — das ist der Parameter
    // `inhalt`, und der kam vom Elternknoten. Fuer die Kinder ist es
    // `weiter`, das zwei Zeilen oben genau das ist, was sein
    // Kommentar sagt ("was dieser Knoten fuer seine Kinder ist"):
    // positioniert -> die eigene Inhaltsbox, statisch -> weitergereicht.
    //
    // GEMESSENE ENTSCHEIDUNG, warum es an 518 und 388 erkennbar ist und
    // nicht an 218 und 188: die Differenz ist 300 bzw. 200, also genau
    // die Breite und die Hoehe des Kind-Kastens 300x200 — der
    // INNERE Kasten. `inhalt` war die 600x400-Flaeche des aeusseren.
    // A1 (`top`+`left`) hat es nicht gezeigt, weil dort der Randwert
    // gewinnt und die Kastengroesse nicht eingeht; A4/A5 ebenfalls
    // nicht. Erst A2 und A3, bei denen NUR die Kastengroesse entscheidet.
    let behaelter = weiter.unwrap_or_else(|| Rect::from_origin_size(Point::ZERO, viewport));

    let n = node.children.len().min(crate::plan::MAX_KINDER);
    let mut tasks: Vec<PlaceTask> = Vec::with_capacity(n);
    // Der lineare Cursor des alten Weges (Column/Row).
    let richtung_alt = if node.kind == NodeKind::Page {
        Direction::Column
    } else {
        node.style.direction
    };
    let mut cursor = match richtung_alt {
        Direction::Column => content.y,
        Direction::Row => content.x,
    };
    // P9: der Zustand der laufenden INLINE-ZEILE. Er wird nur fuer
    // `Direction::Column` benutzt — im Zeilenfluss (`Row`) macht die
    // Kachellogik die Sache schon.
    let mut zeile = ZeilenCursor::neu(content.x, content.y);
    // Release 11.10.2026: (Index in `tasks`, Zeile) je Inline-Teil — fuer die
    // Ausrichtung der Zeilen am Ende (siehe unten).
    let mut inline_zeilen: Vec<(usize, usize)> = Vec::new();

    // Der Float-Zustand wird nur angelegt, wenn es Float-Kinder gibt.
    // Release 12.10.2026 (Agent CSS2): mit den geerbten Floats des umgebenden
    // Kontexts (`Ctx::float_erbe`, von der Messphase gesetzt).
    let erbe = ctx.float_erbe.borrow().get(&id).copied();
    let mut hat_floats = erbe.is_some()
        || node.children.iter().any(|&c| {
            ctx.stil(c)
                .map(|s| s.schweben != FloatArt::Keins)
                .unwrap_or(false)
        });
    let mut floats = erbe.unwrap_or_else(FloatZustand::neu);
    // Release 11.10.2026: halber Durchschuss auch fuer Zeilenboxen mit
    // Planeintrag (siehe `zeilen_durchschuss`).
    let durchschuss = if richtung_alt == Direction::Row { zeilen_durchschuss(ctx, id, node, content) } else { None };
    // Release 09.10.2026: Margin-Collapsing im Blockfluss (siehe Kopf der
    // Datei) — dieselbe Regel wie `box::sum_children`.
    let kollaps = richtung_alt == Direction::Column
        && !matches!(eltern_stil.anzeige, Anzeige::Tabelle | Anzeige::TabelleZeile | Anzeige::Flex | Anzeige::Raster)
        && crate::r#box::ist_blockfluss(ctx, id, node);
    let eltern_hoch_oben = kollaps && sizes.hochgezogen(id).0.is_some();
    let mut vorige_unten: Option<f32> = None;
    // Release 11.10.2026 (Agent CSS2): wie tief die Floats aus frueheren
    // Kindern ohne eigenen Kontext reichen (relativ zum Inhaltsanfang; siehe
    // `Sizes::float_ueberhang`) — fuer `clear` (CSS 2.1 §9.5.2: alle Floats
    // desselben Formatierungskontexts).
    let mut float_tief = 0.0f32;
    let mut inhalt_davor = false;

    // added 05.10.2026, Rolle SERVER (SERVER_14): **DER TABELLEN-LEG.**
    //
    // GEMESSENER BEFUND, den dieser Zweig schliesst: `lege_tabelle` hatte
    // **0 Aufrufer aus `src/`**. Die Rechnung (CSS 2.1, 17.6) war fertig
    // und wurde von niemandem gerufen; jede Tabelle wurde statt dessen als
    // eine Liste von Bloecken linearisiert.
    //
    // DIE REIHENFOLGE ist CSS 2.1, 17.6 und nicht frei gewaehlt: die
    // Tabelle wird **vor** ihren Zeilen gelegt, weil die Spaltenbreiten
    // aus **allen** Zeilen entstehen. Erst dann wird jede Zeile besucht.
    // Das Ergebnis liegt in `ctx.tabellen`, damit die Zeile ihre
    // Spaltenbreite findet, ohne sie neu zu rechnen (GEMESSEN: eine
    // Tabelle, die das nicht tut, ist drei getrennte Zeilen).
    //
    // `lege_tabelle_alle` gibt die **Zellrechtecke** und legt die
    // **Zeilenrechtecke** unter `ctx.tabellen` ab — beide werden gebraucht,
    // und beide brauchen nur das eine Feld, das freigegeben ist.
    // Release 11.10.2026 (Agent CSS2): die Tabelle hat ihre Lage aus der
    // Messphase (`tabellen_leg`); hier werden nur die absoluten Rechtecke
    // ihrer Teile eingetragen. Der alte Leg bleibt fuer Tabellen ohne Lage.
    let neue_lage = sizes.tabellen_lage(id);
    if let Some(l) = neue_lage {
        let r = crate::r#box::rahmen_edges(&node.style.rahmen);
        let plaetze = crate::tabellen_leg::absolute_plaetze(l, padding_box.x - r.left, padding_box.y - r.top);
        ctx.tabellen_plaetze.borrow_mut().extend(plaetze);
    }
    let mut tabelle_rects: Option<HashMap<NodeId, Rect>> = match eltern_stil.anzeige {
        Anzeige::Tabelle if neue_lage.is_none() => Some(lege_tabelle_alle(ctx, node, content, &eltern_stil, sizes)?),
        _ => None,
    };

    // Die Flex-/Raster-Verteilung wird EINMAL fuer alle Kinder berechnet,
    // nicht je Kind — das ist der Grund, warum es diese Funktionen gibt.
    // added 05.10.2026, SERVER_14: **DIE ZEILE** holt ihre Spaltenbreiten
    // bei ihrer Tabelle. Sie findet sie ueber `node.parent`, nicht ueber
    // einen zweiten Parameter — die Zeile weiss ihren Vater, und die
    // Tabelle hat das Raster unter `ctx.tabellen` abgelegt.
    let mut zeilen_rects: Option<Vec<(NodeId, Rect)>> = match eltern_stil.anzeige {
        // added 05.10.2026, SERVER_15: **JEDE ZELLE, nicht die erste.**
        //
        // GEMESSENER FEHLER, den das behebt: `zeilen_rects_aus` gab **ein**
        // Rechteck zurueck — das des ersten Kindes. Im Bild lagen die
        // beiden Zellen einer Zeile deshalb **untereinander** (y 0 und
        // y 21) statt nebeneinander. Jetzt kommt jedes Kind aus der Karte,
        // ueber seine **eigene** Nummer. Das ist der ganze Unterschied,
        // und er besteht in einer Zeile.
        Anzeige::TabelleZeile => tabelle_ueber_zeile(ctx, node.id).and_then(|tabelle| {
            let t = ctx.tabellen.borrow();
            let tabelle = t.get(&tabelle)?;
            Some(
                node.children
                    .iter()
                    .filter_map(|&zellen_id| {
                        tabelle
                            .zellen_rechtecke
                            .get(&(zellen_id.index() as u32))
                            .copied()
                            .map(|r| (zellen_id, r))
                    })
                    .collect(),
            )
        }),
        _ => None,
    };

    // added 05.10.2026, SERVER_14: **DER ZEILENKASTEN SELBST.**
    //
    // Bis hierher bekommen nur die Zellen ihr Rechteck aus dem Raster; der
    // Zeilenkasten stuende weiterhin im Fluss und waere ein Block ueber
    // Blocks. Und der 5-px-Spacer (`<tr>` **ohne** Zellen) bekaeme gar
    // keins, also **0 px** statt 5 px — genau daran liegt der Abstand
    // zwischen zwei HN-Meldungen.
    //
    // **OHNE NEUES FELD:** die Zeile liest ihre Hoehe aus
    // `Tabelle::zeilenhoehen`. Das ist ein `Vec<f32>` nach Zeilenindex —
    // und die Rechnung hat sie in **Dokumentreihenfolge** abgelegt, also
    // ist der Index der Kindnummer der Zeile gleich. (GEMESSEN an der
    // Infobox: ihre Zeilen stehen in Dokumentreihenfolge.)
    // added 05.10.2026, SERVER_15: **die Zeile schlaegt genauso nach**,
    // ueber `node.parent` — einen Schritt, denn sie liegt direkt unter der
    // Tabelle. Sie holt ihr **Rechteck** aus `zeilen_rechtecke` und nicht
    // aus `zeilenhoehen`: dort steht nur die Hoehe, und der Zeilenkasten
    // braucht auch seine x-Position und seine Breite.
    let eigenes_zeilen_rect: Option<Rect> = match eltern_stil.anzeige {
        Anzeige::TabelleZeile => tabelle_ueber_zeile(ctx, node.id).and_then(|tabelle| {
            ctx.tabellen
                .borrow()
                .get(&tabelle)?
                .zeilen_rechtecke
                .get(&(node.id.index() as u32))
                .copied()
        }),
        _ => None,
    };

    let verteilung: Option<Vec<Platzierung>> = match eltern_stil.anzeige {
        // Release 11.10.2026: der neue Flex-Algorithmus (`flexbox.rs`).
        Anzeige::Flex => Some(match eltern_stil.flex.as_deref() {
            Some(fs) => crate::flexbox::verteilen(ctx, node, content, &eltern_stil, fs, sizes)?,
            None => verteile_flex_alle(ctx, node, content, &eltern_stil, sizes)?,
        }),
        Anzeige::Raster => Some(verteile_raster_alle(
            ctx,
            node,
            content,
            &eltern_stil,
            sizes,
        )?),
        _ => None,
    };

    for (i, &child_id) in node.children.iter().take(n).enumerate() {
        let child = get_node(ctx, child_id)?;
        // Release 15.10.2026: borrowed; copied only for resolved percentage
        // offsets (see `Ctx::stil_oder_standard`).
        let mut cst = std::borrow::Cow::Borrowed(ctx.stil_oder_standard(child_id));
        let child_size = sizes
            .get(child_id)
            .ok_or(LayoutError::UnknownNode(child_id))?;
        // Release 11.10.2026: ein Rasterelement bezieht Prozentraender auf
        // seinen Rasterbereich (CSS Grid 1 §6.2).
        let flaeche = if eltern_stil.anzeige == Anzeige::Raster && eltern_stil.gitter.is_some() && eltern_stil.lanes == 0 {
            ctx.gitter_flaeche.borrow().get(&child_id).copied()
        } else {
            None
        };
        let rand_basis = match cst.position {
            PositionArt::Absolut => behaelter.width,
            PositionArt::Fest => viewport.width,
            _ => flaeche.map_or(content.width, |f| f.width),
        };
        let cm = resolve_spacing(child.style.margin, rand_basis, child_id, "margin")?;

        // 1. `display:none` -> kein Kind, kein Platz.
        if !cst.wird_gezeichnet() {
            continue;
        }
        // Die Kaskade bewahrt den Rohanteil; erst hier ist der
        // tatsaechlich enthaltende Block bekannt.
        let offset_basis = match cst.position {
            PositionArt::Fest => viewport,
            PositionArt::Absolut => behaelter.size(),
            _ => content.size(),
        };
        if let Some(p) = cst.oben_prozent {
            cst.to_mut().oben = Some(offset_prozent(p, offset_basis.height));
        }
        if let Some(p) = cst.rechts_prozent {
            cst.to_mut().rechts = Some(offset_prozent(p, offset_basis.width));
        }
        if let Some(p) = cst.unten_prozent {
            cst.to_mut().unten = Some(offset_prozent(p, offset_basis.height));
        }
        if let Some(p) = cst.links_prozent {
            cst.to_mut().links = Some(offset_prozent(p, offset_basis.width));
        }
        let (dx, dy) = relative_versatz_fuer(&cst);
        // L5 (14:6x): `sticky` ist KEIN `relative` mehr (siehe
        // `relative_versatz_fuer`). Die Kleb-Regel laeuft weiter unten,
        // NACH der normalen Platzierung — sie braucht die statische Lage.
        let ist_klebend = matches!(cst.position, PositionArt::Klebend);

        // P9 (07:45, 08:05): hier stand `leer_am_anfang`, und der Kasten
        // bekam am Zeilenanfang eine eigene Groesse (0 px). Das ist nicht
        // mehr noetig: CSS 2.1, 16.6.1 sagt "removed" — ein kollabierbares
        // Leerzeichen am Rand der Zeile bekommt gar keinen Kasten, und der
        // Rand-Zweig oben (im Column-Zweig, vor `inline_teil`) macht das.
        // GEMESSENER FEHLER des Zwischenstands mit Breite 0: der Kasten
        // stand bei y = -1,4285717, 1,43 px ueber dem Anfang seiner Zeile.
        // added 05.10.2026, SERVER_14: **DIE TABELLE** entscheidet hier.
        //
        // Die Zeile bekommt ihr Rechteck aus `lege_tabelle` — nicht aus
        // dem Fluss-Cursor. Ohne diesen Zweig stapeln sich die Zeilen
        // untereinander, weil sie Bloecke sind; **das ist der Fehler, den
        // `lege_tabelle` bis heute unbemerbt verdeckt hat**: die Rechnung
        // war fertig und laeuft nie.
        // DER KASTEN DIESES KNOTENS, wenn er eine Zeile ist.
        // added 05.10.2026, SERVER_15: **DIE ZELLE BRAUCHT IHREN EIGENEN
        // PLATZ, nicht den der Zeile.**
        //
        // GEMESSENER FEHLER, den das behebt: der Zweig nahm fuer **jedes**
        // Kind `eigenes_zeilen_rect`, sobald der Elternknoten eine
        // `TabelleZeile` war — und `eigenes_zeilen_rect` ist **ein**
        // Rechteck fuer die **ganze Zeile**. Beide Zellen bekamen damit
        // denselben Wert (`x 201, breite 400`) statt ihrer eigenen
        // (`x 0, breite 200` und `x 210, breite 200`).
        //
        // Die Reihenfolge ist deshalb umgekehrt: **erst die Zelle**, dann
        // die Zeile. `zeilen_rects` traegt fuer die Kinder der Zeile deren
        // eigene Rechtecke; die Zeile selbst nimmt ihres.
        // Release 11.10.2026 (Agent CSS2): zuerst die Lage aus `tabellen_leg`.
        let tabellen_platz = ctx
            .tabellen_plaetze
            .borrow()
            .get(&child_id)
            .copied()
            .or_else(|| zeilen_rects.as_ref().and_then(|m| m.iter().find(|(id, _)| *id == child_id)).map(|(_, r)| *r))
            .or_else(|| {
                // added 04.10.2026, Rolle LAYOUT (LAYOUT_1): **DAS
                // RECHTECK DER ZEILE SELBST.** Der Zweig davor liefert die
                // Rechtecke der **Zellen** einer Zeile — er schlaegt nur,
                // wenn der Elternknoten eine Zeile ist. Die Zeile selbst hat
                // aber als Kind der Tabelle (oder ihrer Zeilengruppe) einen
                // Elternknoten, der **keine** Zeile ist, und damit bekam sie
                // gar nichts und wurde im Fluss als Block gestapelt.
                //
                // GEMESSEN (2 Zeilen, 3 Zellen, 600 px, echtes HTML mit
                // `<tbody>`, vorher/nachher `tests/tabelle_html_ist.rs`):
                //
                //   vorher:  Zeile 0 y 0, Zeile 1 y 0   <- beide dieselbe
                //   nachher:  Zeile 0 y 0, Zeile 1 y 21
                //
                // Die **Zelle** holt sich ihr Rechteck ueber `zeilen_rects`
                // (erster Zweig), die **Zeile** ueber diesen. Beide kommen
                // aus `ctx.tabellen`, beide sind relativ zum Inhaltskasten
                // der Tabelle.
                if cst.anzeige != Anzeige::TabelleZeile {
                    return None;
                }
                let tabelle = tabelle_ueber_zeile(ctx, child_id)?;
                ctx.tabellen
                    .borrow()
                    .get(&tabelle)?
                    .zeilen_rechtecke
                    .get(&(child_id.index() as u32))
                    .copied()
            })
            .or_else(|| {
                if eltern_stil.anzeige == Anzeige::TabelleZeile {
                    eigenes_zeilen_rect
                } else {
                    None
                }
            })
            .filter(|_| !matches!(cst.position, PositionArt::Absolut | PositionArt::Fest));

        let mut groesse_lauf: Option<Size> = None;
        let p: Platzierung = if let Some(r) = tabellen_platz {
            // Release 11.10.2026 (Agent CSS2): die Lage aus `tabellen_leg`
            // ist die RAHMENkante; `build_tree` addiert den Rand noch, also
            // hier abziehen (Tabellenteile haben keinen Rand, CSS 2.1 §17.5;
            // die Beschriftung hat ihn in der Lage schon).
            let (rl, ro) = if ctx.tabellen_plaetze.borrow().contains_key(&child_id) { (cm.left, cm.top) } else { (0.0, 0.0) };
            Platzierung {
                x: r.x + dx - rl,
                y: r.y + dy - ro,
                breite: r.width,
                hoehe: r.height,
            }
        } else if let Some(v) = verteilung
            .as_ref()
            .filter(|_| !matches!(cst.position, PositionArt::Absolut | PositionArt::Fest))
        {
            // 3. Flex oder Raster: die Verteilung des Eltern entscheidet.
            let q = v.get(i).copied().unwrap_or(Platzierung::WEG);
            Platzierung {
                // P9-NACHFOLGER (08:52): der Rand wird in `build_tree`
                // addiert (Zeile 137) — hier nicht noch einmal. Siehe
                // dort die Begruendung und die Messung.
                x: q.x + dx,
                y: q.y + dy,
                breite: q.breite,
                hoehe: q.hoehe,
            }
        } else if matches!(cst.position, PositionArt::Absolut | PositionArt::Fest) {
            // 2. absolute / fixed: gegen den enthaltenden Block, aus dem
            // Fluss. Die vier Seiten kommen aus dem Stil des KINDES.
            //
            // L5 (02.10.2026 12:2x): DIE STATISCHE POSITION.
            //
            // GEMESSENER BEFUND, was hier vorher stand: `Platzierung::WEG`,
            // und das ist (0, 0, 0, 0) (GEMESSEN an der Konstante in
            // `plan.rs:183`). `platz_absolut` rechnet daraus
            // `statisch.x - behaelter.x`, also `-behaelter.x`, und setzt
            // dann `behaelter.x + (-behaelter.x) = 0`. GEMESSEN: JEDE
            // absolute Box ohne `top`/`left` landete bei (0, 0) — am
            // Seitenursprung, nicht an ihrer Stelle.
            //
            // GEMESSENE WIRKUNG auf der Abnahme (wiki.html, 1400x900,
            // `examples/l5_position.rs`):
            //     `plan_stil.position = Statisch` (Zeile in narsil-page):
            //         32 sichtbare Kaesten
            //     ohne die Zeile, mit `Platzierung::WEG`:
            //         12 sichtbare Kaesten
            // Die Zeile in narsil-page war also richtig — sie hat einen
            // echten Fehler verdeckt, nicht einen erfundenen. Genau das
            // steht in ihrem Kommentar (05.10. 21:55), und es stimmt.
            //
            // GEMESSENE ENTSCHEIDUNG, warum der CURSOR die statische
            // Position ist: der Cursor ist genau die Stelle, an der der
            // Kasten im Fluss stuende. Er wird von den vorherigen
            // Geschwistern weitergezaehlt, und eine absolute Box zaehlt
            // ihn nicht weiter (sie ist nicht im Fluss) — das steht
            // seit langem im Kommentar am Zeilenzweig. Damit ist der
            // Cursor fuer die Kaiser-Position die richtige Groesse, ohne
            // dass man den Normalfluss zweimal laufen laesst.
            //
            // GEMESSENE GRENZE, die ehrlich benannt wird: fuer ein
            // Inline-Kind in einer Absatzzeile waere die statische
            // Position der Anfang seiner ersten Zeile (CSS 2.1, 10.3.7),
            // nicht der Cursor des Blockflusses. Die Kachel ist hier der
            // Blockcursor — in einem Block-Fluss, der der Fall ist, in dem
            // `position` bei Wikipedia benutzt wird.
            // Release 08.10.2026: in einer ZEILE (`Row`) ist der Cursor die
            // x-Position — dort stuende der Kasten im Fluss (MDN-Symbol
            // „externer Link" als `::after { position: absolute }`).
            let statisch = Platzierung {
                x: if richtung_alt == Direction::Row { cursor } else { content.x },
                y: if richtung_alt == Direction::Column {
                    cursor
                } else {
                    content.y
                },
                breite: sauber_pos(child_size.width),
                hoehe: sauber_pos(child_size.height),
            };
            // Release 11.10.2026 (Agent CSS2): in einem `rtl`-Block steht die
            // statische Lage am RECHTEN Rand (CSS 2.1 §10.3.7: "set 'right'
            // to the static position"). GEMESSEN vorher (WPT
            // `absolute-non-replaced-width-002`): der Kasten stand links.
            let eltern_rtl = ctx.plan.is_some_and(|p| p.richtung(id) == narsil_layout::Richtung::Rtl);
            let mut statisch = statisch;
            if eltern_rtl && richtung_alt == Direction::Column {
                statisch.x = content.x + content.width - statisch.breite - cm.left - cm.right;
            }
            //
            // L5 (02.10.2026 13:4x): `fixed` RECHNET GEGEN DIE ANSICHT.
            //
            // CSS 2.1, 10.1 woertlich: "For a fixed positioned element,
            // the containing block is established by the viewport."
            //
            // GEMESSENER BEFUND, was vorher passierte
            // (`tests/l5_absolute.rs`, `l5_fixed_rechnet_gegen_die_
            // ansicht_nicht_gegen_den_vorfahren`): `right: 30; bottom:
            // 20` in einem positionierten Vorfahren 300x200 ergab
            // (170, 140) — `300-30-100` und `200-20-40`, also gegen
            // den Vorfahren. Nach 10.1 ist es (670, 540):
            // `800-30-100` und `600-20-40`, gegen die 800x600-Ansicht.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die statische Position
            // UNVERAENDERT mitgegeben wird: §10.3.7 gilt auch fuer
            // `fixed`. Sind `top` und `left` beide `auto`, steht der
            // Kasten an seiner statischen Position — und die ist der
            // Cursor, unveraendert. Die Korrektur betrifft nur die
            // Bezugsflaeche der Randangaben.
            //
            // GEMESSENE ENTSCHEIDUNG, warum hier die Ansicht und nicht
            // die Entstehungs-Box steht: §10.1 macht beides gleich, weil
            // die Entstehungs-Box in unseren Faellen bei (0, 0) mit
            // Ansichtsbreite beginnt. Die Ansicht ist die von beiden
            // Loesungen, die ohne den zweiten Begriff auskommt — und
            // `Rect::from_origin_size` steht zwei Zeilen weiter oben
            // schon fuer denselben Fall.
            let behaelter_fuer = if matches!(cst.position, PositionArt::Fest) {
                Rect::from_origin_size(Point::ZERO, viewport)
            } else {
                behaelter
            };
            // Release 11.10.2026: im positionierten Raster ist der
            // Rasterbereich der enthaltende Block (CSS Grid 1 §9.1), und
            // ohne Versatzangaben richtet `justify-self`/`align-self` den
            // Kasten darin aus. GEMESSEN vorher (WPT css-grid/abspos,
            // alignment/*positioned*): das Element stand am Anfang des
            // ganzen Rasters.
            let raster = if matches!(cst.position, PositionArt::Absolut) {
                ctx.raster_bereiche.borrow().get(&child_id).copied()
            } else {
                None
            };
            let behaelter_fuer = raster.map_or(behaelter_fuer, |(r, _, _)| r);
            let mut statisch = statisch;
            // Release 11.10.2026: im NICHT positionierten Raster ist nur die
            // statische Lage die Inhaltsbox (CSS Grid 2 §9.2), nicht der Block.
            // Ein Nachfahre (kein Kind) des Rasters behaelt seine statische
            // Lage im Fluss (CSS Grid 1 §9.1 aendert nur den Block).
            let raster_lage = raster.filter(|_| !ctx.gitter_nachfahren.borrow().contains(&child_id));
            let raster_statisch = if raster_lage.is_none() && matches!(cst.position, PositionArt::Absolut) {
                ctx.gitter_statisch.borrow().get(&child_id).copied()
            } else {
                None
            };
            if let Some((r, av, ah)) = raster_lage.or(raster_statisch) {
                let lage = |a: narsil_css::FlexAusrichtung, anfang: f32, platz: f32, eigen: f32| match a {
                    narsil_css::FlexAusrichtung::Mitte => anfang + (platz - eigen) / 2.0,
                    narsil_css::FlexAusrichtung::Ende => anfang + platz - eigen,
                    _ => anfang,
                };
                statisch.x = lage(ah, r.x, r.width, statisch.breite + cm.left + cm.right);
                statisch.y = lage(av, r.y, r.height, statisch.hoehe + cm.top + cm.bottom);
            }
            // Release 12.10.2026 (Agent CSS2): ein positioniertes Inline-Element
            // ohne Kasten als enthaltender Block — Rechteck 0 x 0 an der
            // statischen Lage (`LayoutStil::cb_inline`).
            let behaelter_fuer = if cst.cb_inline && matches!(cst.position, PositionArt::Absolut) {
                Rect::new(statisch.x, statisch.y, 0.0, 0.0)
            } else {
                behaelter_fuer
            };
            // Solver erhält Border-Maße; der spätere Baum addiert die Marge einmal.
            statisch.x += cm.left;
            statisch.y += cm.top;
            // Release 11.10.2026: Selbstausrichtung im inset-modifizierten
            // enthaltenden Block (CSS Box Alignment 3 §6, CSS Position 3 §4.1):
            // mit einer Lage (start, center, end, …) passend statt gedehnt.
            let (ausr_b, ausr_h) = crate::r#box::abs_selbst_lage(ctx, child_id);
            let mut lage = platz_absolut(
                behaelter_fuer.inset(cm),
                cst.links,
                cst.oben,
                // Release 11.10.2026 (Agent CSS2): ein Prozentwert aus dem
                // Stylesheet steht nur im Plan (siehe `absolute_nachmessen`).
                // `fit-content` und Verwandte: die gemessene Groesse, keine
                // Dehnung zwischen den Versaetzen.
                child.style.size.width.resolve(behaelter_fuer.width).map(|_| child_size.width)
                    .or(cst.breite_anteil.map(|_| child_size.width))
                    .or(cst.breite_inhalt.then_some(child_size.width))
                    .or(ausr_b.map(|_| child_size.width))
                    // Release 14.10.2026: a replaced box keeps its measured
                    // size (an explicit `stretch` is already in it, see
                    // `box.rs`), the insets do not stretch it (CSS 2.1
                    // §10.3.8, §10.6.5).
                    .or(child.ersetzt.is_some().then_some(child_size.width)),
                child.style.size.height.resolve(behaelter_fuer.height).map(|_| child_size.height)
                    .or(cst.hoehe_anteil.map(|_| child_size.height))
                    .or(cst.hoehe_inhalt.then_some(child_size.height))
                    .or(ausr_h.map(|_| child_size.height))
                    .or(child.ersetzt.is_some().then_some(child_size.height))
                    // Release 14.10.2026: a box with `aspect-ratio` and a
                    // definite width keeps the height from the ratio (see
                    // `absolute_nachmessen`; WPT css-sizing `aspect-ratio/abspos-005`).
                    .or((cst.seitenverhaeltnis.is_finite()
                        && cst.seitenverhaeltnis != 0.0
                        && matches!(child.style.size.width, narsil_core::length::Length::Px(_)))
                    .then_some(child_size.height)),
                cst.rechts,
                cst.unten,
                statisch,
                Size::new(sauber_pos(child_size.width), sauber_pos(child_size.height)),
            );
            // Release 10.10.2026: CSS 2.1 §10.3.7 mit beiden Versaetzen —
            // die Breite der Messphase (dort schon durch min-/max-width
            // begrenzt) und der freie Rest auf die `auto`-Raender (beide:
            // mittig). Vorher: immer volle Dehnung, `margin:auto` ignoriert.
            if let (Some(l), Some(r)) = (cst.links, cst.rechts) {
                let rand_auto = |x: narsil_core::length::Length| x == narsil_core::length::Length::Auto;
                let ml_auto = cst.marge_links_auto || rand_auto(child.style.margin.left);
                let mr_auto = cst.marge_rechts_auto || rand_auto(child.style.margin.right);
                let breite_auto = child.style.size.width == narsil_core::length::Length::Auto;
                let soll = if breite_auto { sauber_pos(child_size.width) } else { lage.breite };
                let innen = behaelter_fuer.inset(cm);
                let frei = innen.width - l - r - soll;
                if breite_auto {
                    lage.breite = soll;
                }
                // Release 11.10.2026 (Agent CSS2): der Rest der Regel —
                // beide `auto` und negativ: in `ltr` wird margin-left 0, in
                // `rtl` margin-right 0; nur margin-left `auto`: es nimmt den
                // Rest auch negativ; ueberbestimmt in `rtl`: `left` faellt
                // weg. GEMESSEN vorher (WPT `absolute-non-replaced-width-005`,
                // `-007`): der Kasten stand an `left`.
                if frei.is_finite() {
                    if ml_auto && mr_auto {
                        if frei > 0.0 {
                            lage.x = innen.x + l + frei / 2.0;
                        } else if eltern_rtl {
                            lage.x = innen.x + l + frei;
                        }
                    } else if ml_auto || (!mr_auto && eltern_rtl) {
                        lage.x = innen.x + l + frei;
                    }
                }
            }
            // Release 11.10.2026 (Agent CSS2): dasselbe senkrecht (CSS 2.1
            // §10.6.4): bei top, height und bottom ungleich `auto` nehmen
            // die `auto`-Raender den Rest — beide gleich viel, auch
            // negativ. GEMESSEN vorher (WPT `absolute-non-replaced-height-
            // 003..012`): der Kasten klebte oben am Abstand `top`.
            // Release 15.10.2026 (Agent CSS2): a replaced box has its height
            // from its content even with `height: auto` (CSS 2.1 §10.6.5).
            // MEASURED before (WPT `CSS2/positioning/absolute-replaced-height-
            // 036` after the merge of 1c807c41, where an img no longer carries
            // its natural height as `height`): the auto margins stayed 0.
            if let (Some(o), Some(u)) = (cst.oben, cst.unten)
                && (child.style.size.height != narsil_core::length::Length::Auto
                    || cst.hoehe_anteil.is_some()
                    || cst.hoehe_inhalt
                    || child.ersetzt.is_some())
                && (cst.marge_oben_auto || cst.marge_unten_auto)
            {
                let innen = behaelter_fuer.inset(cm);
                let frei = innen.height - o - u - lage.hoehe;
                if frei.is_finite() {
                    if cst.marge_oben_auto && cst.marge_unten_auto {
                        lage.y = innen.y + o + frei / 2.0;
                    } else if cst.marge_oben_auto {
                        lage.y = innen.y + o + frei;
                    }
                }
            }
            // Release 11.10.2026: die Lage im inset-modifizierten Block (beide
            // Versaetze gesetzt, keine `auto`-Raender): start/center/end je
            // Achse, waagerecht nach der Richtung des Blocks; `safe` bleibt
            // bei Ueberlauf am Anfang. GEMESSEN vorher: WPT css-align
            // `abspos/align-self-htb-ltr-htb`, `justify-self-htb-*-htb` (der
            // Kasten fuellte den Block).
            {
                use narsil_layout::gitter::AusrLage as L;
                let innen = behaelter_fuer.inset(cm);
                // Release 14.10.2026: the default overflow alignment (neither
                // `safe` nor `unsafe`, CSS Box Alignment 3 §4.4 with CSS
                // Position 3 §4.4): a box that overflows stays inside the
                // union of the inset-modified and the original containing
                // block; if it is larger than that, it sits at its start edge.
                // MEASURED before: WPT css-align
                // `abspos/align-self-default-overflow-htb-ltr-htb` (a 95 px
                // box centred in 70 px reached 2.5 px past the bottom).
                let ueberlauf = |pos: f32, groesse: f32, anfang: f32, ende: f32, rueckwaerts: bool| -> f32 {
                    if !(pos.is_finite() && groesse.is_finite() && anfang.is_finite() && ende.is_finite()) {
                        return pos;
                    }
                    if groesse > ende - anfang {
                        if rueckwaerts { ende - groesse } else { anfang }
                    } else {
                        pos.clamp(anfang, ende - groesse)
                    }
                };
                let faktor = |a: narsil_layout::gitter::Ausr, rtl: bool| -> f32 {
                    let f = match a.lage {
                        L::Mitte => 0.5,
                        L::Ende | L::LetzteGrundlinie => 1.0,
                        L::Rechts => return 1.0,
                        L::Links => return 0.0,
                        _ => 0.0,
                    };
                    if rtl { 1.0 - f } else { f }
                };
                let rand_auto = |x: narsil_core::length::Length| x == narsil_core::length::Length::Auto;
                if let (Some(a), Some(l), Some(r)) = (ausr_b, cst.links, cst.rechts)
                    && !(cst.marge_links_auto || rand_auto(child.style.margin.left))
                    && !(cst.marge_rechts_auto || rand_auto(child.style.margin.right))
                {
                    let frei = innen.width - l - r - lage.breite;
                    if frei.is_finite() {
                        // Release 14.10.2026: `self-start`/`self-end` follow the
                        // box's own writing mode (CSS Box Alignment 3 §4.3): its
                        // direction, or for a vertical box its block flow
                        // (`vertical-lr` from the left, `vertical-rl` from the
                        // right). MEASURED before: WPT css-align
                        // `abspos/justify-self-htb-ltr-htb` (`direction: rtl;
                        // justify-self: self-start` stood left).
                        let eigen_rtl = match cst.vertical_writing {
                            1 => false,
                            2 => true,
                            _ => cst.rtl,
                        };
                        let f = faktor(a, if a.selbst_bezug { eigen_rtl } else { eltern_rtl });
                        let f = if a.sicher == Some(true) && frei < 0.0 { if eltern_rtl { 1.0 } else { 0.0 } } else { f };
                        lage.x = innen.x + l + frei * f;
                        if a.sicher.is_none() && frei < 0.0 {
                            lage.x = ueberlauf(lage.x, lage.breite, innen.x + l.min(0.0), innen.x + innen.width - r.min(0.0), eltern_rtl);
                        }
                    }
                }
                if let (Some(a), Some(o), Some(u)) = (ausr_h, cst.oben, cst.unten)
                    && !cst.marge_oben_auto
                    && !cst.marge_unten_auto
                {
                    let frei = innen.height - o - u - lage.hoehe;
                    if frei.is_finite() {
                        let f = if a.sicher == Some(true) && frei < 0.0 { 0.0 } else { faktor(a, false) };
                        lage.y = innen.y + o + frei * f;
                        if a.sicher.is_none() && frei < 0.0 {
                            lage.y = ueberlauf(lage.y, lage.hoehe, innen.y + o.min(0.0), innen.y + innen.height - u.min(0.0), false);
                        }
                    }
                }
            }
            lage.x -= cm.left;
            lage.y -= cm.top;
            lage
        } else if hat_floats && cst.schweben != FloatArt::Keins {
            // 4. Float: links oder rechts neben dem, was belegt ist.
            // FloatZustand liefert lokale Koordinaten. Ein Float nach
            // normalen Geschwistern beginnt am aktuellen Flusscursor.
            // Release 11.10.2026 (Agent CSS2): die RANDbox belegt den Platz
            // (CSS 2.1 §9.5.1: "the margin box of a left-floating box"), und
            // `build_tree` addiert den Rand danach einmal. GEMESSEN vorher
            // (Hinweis Agent A, css-flexbox `flex-abspos-staticpos-margin-*`):
            // zwei `float:left; width:20px; margin-right:10px` standen bei 0
            // und 20 statt 0 und 30, `margin-top` zaehlte doppelt.
            let y_rel = floats.clear_y(cst.raeumen, cursor - content.y);
            // CSS 2.1, 9.5.1: passt der Float nicht neben die
            // schwebenden, rueckt er so weit, bis er passt.
            //
            // CSS 2.1 §9.5 / §10.3.5: DIE BREITE EINES FLOATS IST DIE
            // SCHRUMPFBREITE, nicht die Füllbreite.
            //
            //     width = min(max(vorzug_min, verfuegbar), vorzug_max)
            //
            // GEMESSENER FEHLER (HTML_12, groesster Hebel): `<div
            // style="float:left">` bekam **1264 px** — die volle
            // Containerbreite — und der Bruder darunter stand darunter.
            // Firefox: **14 px** und der Bruder daneben. Vorher stand hier
            // `sauber_pos(child_size.width)`, und das ist die ausgerechnete
            // Fiillbreite eines Block-Kastens, also genau die, die §9.5
            // ausdruecklich NICHT ist.
            //
            // `min_aggregat` ist das min-content (§10.3.5 "preferred minimum
            // width"), `bevorzugte_breite` das max-content. Die Rechnung ist
            // die umgekehrte Form derselben Formel; sie ist gleich, solange
            // `vorzug_min <= vorzug_max` — und das ist nach Definition
            // der Fall (das minimum kann nie groesser sein als das maximum).
            let verfuegbar = content.width;
            // Release 13.10.2026 (Agent CSS2): dazu die min-content-Breite des
            // Inhalts samt Zeilenboxen und Texten (`min_aggregat` kennt nur feste
            // Kinderbreiten). GEMESSEN vorher (WPT `CSS2/text/text-align-white-
            // space-005` u. a.): ein Float um `aaabbb` in einem 10 px breiten Block
            // war 10 px breit, der Text ragte hinaus.
            let vorzug_min = sizes
                .min_aggregat(ctx, child, Direction::Column)
                .max(sizes.min_inhalt_breite(ctx, child_id, 0));
            let vorzug_max = sizes.bevorzugte_breite(ctx, child_id, 0);
            //
            // DIE ZAHL, und sie ist die ganze Antwort auf HTML/CSS_18 (a):
            // `measure_width("kurz", 16)` ist in allen drei Messern
            // **56.00** (`tests/float_text_annahme.rs`). GEMESSEN wird der
            // Float-Rahmen mit **56** und der Inhalt darin mit **48**, also
            // mit 56 minus den 8 px Polster.
            //
            // `cm` ist der **Rand**, nicht die Fuellung (Zeile 1014:
            // `resolve_spacing(child.style.margin, …)`) — die Fuellung wird
            // erst weiter oben (Zeile 233) und dann in `build_tree` addiert.
            // Deshalb steht hier **keine** Addition: sie waere ein No-op.
            //
            // DIE OFFENE FRAGE, genau benannt: §10.3.5 liefert ein
            // **Inhalts**mass (§9.5 setzt `width` darauf), der Rahmen ist
            // Inhalt plus Fuellung (§10.1). GEMESSEN laeuft die 56 aber als
            // **Rahmen**masse durch — sonst waere der Rahmen 64. Wer sie
            // zur Inhaltsbreite macht, muss die Fuellung an der Stelle
            // addieren, an der `Platzierung::breite` zum `border_box` wird.
            // Release 13.10.2026 (Agent CSS2): ein Float mit bestimmter
            // `width` schrumpft nicht (CSS 2.1 §10.3.5 gilt nur fuer `auto`):
            // breitere Kinder ragen hinaus. GEMESSEN vorher (Koordinator, WPT
            // css-flexbox `overflow-area-001/002`): `float:left; width:80px`
            // um ein 90 px breites Kind wurde 90 px breit, seit `min_aggregat`
            // feste Kinderbreiten zaehlt.
            let fest = child.style.size.width != narsil_core::length::Length::Auto
                && !crate::r#box::ist_zeilenbox(ctx, child_id, child);
            // Release 13.10.2026 (Agent CSS2): auch nicht auf die verfuegbare
            // Breite geklemmt — ein breiterer Float ragt hinaus (CSS 2.1 §9.5.1
            // Regel 3). GEMESSEN vorher (WPT `floats/floats-rule3-outside-left-
            // 001`, `-right-001`): ein 425 px breiter Float wurde 400 px breit.
            let schrumpf_breite = if fest {
                sauber_pos(vorzug_max)
            } else {
                sauber_pos(verfuegbar.min(vorzug_max).max(vorzug_min))
            };
            // Release 14.10.2026 (Agent CSS2): `max-width`/`min-width` clamp
            // the shrink-to-fit width, after the min-content floor (CSS 2.1
            // §10.4). MEASURED before (WPT `CSS2/positioning/absolute-non-
            // replaced-width-019`, a float with `max-width: 4em` around an
            // unbreakable 8em word): the float was 8em wide.
            let schrumpf_breite = if fest {
                schrumpf_breite
            } else {
                let fuellung = resolve_spacing(child.style.padding, content.width, child_id, "padding")
                    .map(|pd| {
                        let k = crate::r#box::rahmen_edges(&child.style.rahmen);
                        pd.left + pd.right + k.left + k.right
                    })
                    .unwrap_or(0.0);
                let (min_b, max_b, _, _) =
                    crate::r#box::grenzen_rahmen(ctx, child_id, content.size(), (fuellung, 0.0));
                let w = max_b.map_or(schrumpf_breite, |m| schrumpf_breite.min(m));
                sauber_pos(min_b.map_or(w, |m| w.max(m)))
            };
            // Release 14.10.2026: a float with `aspect-ratio` and `height:
            // auto` takes the height from its shrink-to-fit width; the
            // contents stay the automatic minimum unless it is a scroll
            // container (CSS Sizing 4 §5.1/§5.2). MEASURED before: WPT
            // css-sizing `aspect-ratio/block-aspect-ratio-019` (100 x 0).
            let float_hoehe = match ctx.stil(child_id) {
                Some(st)
                    if child.ersetzt.is_none()
                        && child.style.size.height == narsil_core::length::Length::Auto
                        && st.seitenverhaeltnis.is_finite()
                        && st.seitenverhaeltnis != 0.0 =>
                {
                    let v = st.seitenverhaeltnis.abs();
                    let pad = resolve_spacing(child.style.padding, content.width, child_id, "padding").unwrap_or(narsil_core::geometry::Edges::ZERO);
                    let k = crate::r#box::rahmen_edges(&child.style.rahmen);
                    let rand_x = pad.left + pad.right + k.left + k.right;
                    let rand_y = pad.top + pad.bottom + k.top + k.bottom;
                    let rh = if st.border_box && st.seitenverhaeltnis > 0.0 { schrumpf_breite / v } else { (schrumpf_breite - rand_x).max(0.0) / v + rand_y };
                    let rh = if st.rollkasten { rh } else { rh.max(sauber_pos(child_size.height)) };
                    sauber_pos(rh)
                }
                _ => sauber_pos(child_size.height),
            };
            let (q, _rueckte) = floats.platziere_mit_umbruch(
                content.width,
                cm.left + schrumpf_breite + cm.right,
                cm.top + float_hoehe + cm.bottom,
                y_rel,
                cst.schweben,
                eltern_stil.luecke,
            );
            Platzierung {
                // P9-NACHFOLGER (08:52): siehe der Flex-Zweig.
                x: content.x + q.x + dx,
                y: content.y + q.y + dy,
                breite: schrumpf_breite,
                hoehe: float_hoehe,
            }
        } else {
            // 5. Der alte lineare Cursor. Eine absolute Box hat hier
            // nichts zu tun — sie hat den Cursor schon oben nicht
            // weitergezaehlt.
            //
            // L4 (04.10.2026 04:1x): `clear` GILT AUCH FUER KASTEN, DIE
            // SELBST NICHT SCHWEBEN.
            //
            // GEMESSENER FEHLER (`CSS_16.txt`, f4): `<div style="float:left;
            // width:300px; height:100px">` und darunter ein `<div
            // style="clear:both">`. GEMESSEN stand der `clear`-Kasten bei
            // **y 900** — auf gleicher Hoehe wie der Float, nicht darunter.
            // Firefox: **y 1000**, aus `ff_float.png`.
            //
            // GEMESSENE URSACHE, und sie ist eine Zeile: `grep -n "raeumen\|
            // clear_y" crates/narsil-layout-engine/src/flow.rs` findet genau
            // **einen** Treffer, Zeile 1164 — und der liegt im FLOAT-Zweig.
            // Ein Kasten mit `clear`, der selbst keiner ist, landet hier in
            // Zweig 5 und behaelt den linearen Cursor; `clear_y` wird nie
            // gefragt.
            //
            // CSS 2.1 §9.5.2 gilt fuer **jeden** Kasten mit `clear`, nicht
            // nur fuer schwebende. Der Aufruf ist derselbe wie im
            // Float-Zweig: `y.max(unten_links).max(unten_rechts)`.
            //
            // DIE KOORDINATE, und sie ist dieselbe wie im Float-Zweig:
            // `clear_y` rechnet **relativ zu `content.y`** — der Float steht
            // dort bei `y_rel = 0` und hoehe 100, also ist `unten_links` 100
            // und nicht 1000. GEMESSEN am Fall f4: `cursor = 900`,
            // `content.y = 900`, also `clear_y` liefert 100 und der neue
            // Cursor ist `900 + 100 = 1000` — Firefox.
            // Release 11.10.2026 (Agent CSS2): `clear` wirkt unten im
            // Blockzweig auf die RAHMENkante (siehe dort).
            // Zeilenhoehe des Blocks (line-height) als Minimum fuer Text-Knoten.
            let block_line_height = if node.style.zeilenhoehe_art == narsil_core::style::ZeilenhoeheArt::Normal && ctx.stil(node.id).is_some() {
                // Release 12.10.2026 (Agent CSS2): `normal` wie Strut und Textlauf (`zeilenhoehe_normal`).
                crate::text::zeilenhoehe_normal(node.style.font_size)
            } else if node.style.zeilenhoehe_px.is_finite() && node.style.zeilenhoehe_px > 0.0 {
                node.style.zeilenhoehe_px
            } else {
                node.style.font_size
            };
            let hoehe = if child.kind == NodeKind::Text {
                child_size.height.max(block_line_height)
            } else {
                child_size.height
            };
            match richtung_alt {
                Direction::Column => {
                    // P9 (05.10.2026): DER INLINE-LAUF.
                    //
                    // CSS Inline 3, §2: eine Folge von Inline-Boxen
                    // bildet eine Zeile. Sie stehen NEBENEINANDER, und
                    // die Zeile ist so hoch wie der hoechste Teil darin,
                    // ausgerichtet an der Grundlinie.
                    //
                    // GEMESSENER FEHLER, den das behebt
                    // (`/tmp/opencode/p9/probe.html`):
                    //     .ib { display:inline-block; width:60px;
                    //            height:30px }
                    //     <div><span class="ib">A</span></div>
                    // Vorher war der Kasten 11x16 — eine eigene Zeile,
                    // 11 px breit (die gemessene Textbreite).
                    //
                    // WER ALS INLINE GEZAHLT WIRD, und warum nicht mehr:
                    // * `NodeKind::Text` — ein Text ist per Definition
                    //   Inline (CSS 2.1, 9.2.2.1).
                    // * `Anzeige::InlineBlock` — sieht aus wie inline,
                    //   verhaelt sich wie ein Block (9.2.1.1).
                    // * `NodeKind::Page` und `Anzeige::Block` NICHT: ein
                    //   Block beendet die Zeile (CSS Inline 3, §2.1).
                    if inline_kind(ctx, child_id, &cst, &child.kind) {
                        // Release 12.10.2026 (Agent CSS2): neben Floats ist
                        // die Zeile verkuerzt (CSS 2.1 §9.5) — passt der Teil
                        // nicht mehr hinein, endet sie hier, und die naechste
                        // fragt die Floats neu. GEMESSEN vorher (WPT
                        // `floats-wrap-top-below-inline-001l/r`): zwei
                        // `inline-block` standen ueber den Floats.
                        let teil_breite = cm.left + sauber_pos(child_size.width) + cm.right;
                        if hat_floats
                            && zeile.gleich
                            && !kollabierer_leerraum(ctx, child_id)
                            && zeile.braucht + teil_breite > zeile.verfuegbar.unwrap_or(content.width)
                        {
                            zeile.schliesse_wenn_offen(&mut cursor);
                        }
                        // P9 (07:50): Zeilenanfang -> Vorschau. Siehe den
                        // alten Weg; die Regel steht in `vorschau.rs`.
                        let zeile_war_leer = !zeile.gleich;
                        if zeile_war_leer {
                            // Zeilenhoehe des Blocks (line-height).
                            let line_height = if node.style.zeilenhoehe_art == narsil_core::style::ZeilenhoeheArt::Normal && ctx.stil(node.id).is_some() {
                                // Release 12.10.2026 (Agent CSS2): `normal` wie Strut und Textlauf (`zeilenhoehe_normal`).
                                crate::text::zeilenhoehe_normal(node.style.font_size)
                            } else if node.style.zeilenhoehe_px.is_finite() && node.style.zeilenhoehe_px > 0.0 {
                                node.style.zeilenhoehe_px
                            } else {
                                node.style.font_size
                            };
                            let v = vorschau(
                                ctx,
                                node,
                                i,
                                sizes,
                                line_height,
                                content.width,
                                n,
                            );
                            zeile.setze_vorschau(v);
                            if hat_floats {
                                // Das Band der Zeile neben den Floats; passt der
                                // erste Teil nicht daneben, rueckt die Zeile an
                                // die naechste Float-Unterkante.
                                let h = zeile.zeilen_hoehe.max(1.0);
                                let mut y_rel = (zeile.y_start - content.y).max(0.0);
                                let mut kanten = floats.kanten_im_band(y_rel, h);
                                for _ in 0..32 {
                                    let frei = content.width - kanten.0 - kanten.1;
                                    if teil_breite <= frei + 0.01 || (kanten.0 <= 0.0 && kanten.1 <= 0.0) {
                                        break;
                                    }
                                    match floats.naechste_unterkante(y_rel, h) {
                                        Some(u) if u > y_rel => {
                                            y_rel = u;
                                            kanten = floats.kanten_im_band(y_rel, h);
                                        }
                                        _ => break,
                                    }
                                }
                                if content.y + y_rel > zeile.y_start {
                                    zeile.nach_block(content.y + y_rel);
                                }
                                zeile.x_start = content.x + kanten.0;
                                zeile.verfuegbar = Some((content.width - kanten.0 - kanten.1).max(0.0));
                            }
                        }
                        // CSS 2.1, 16.6.1: Rand-Leerzeichen werden
                        // ENTFERNT, nicht auf 0 px gesetzt. Siehe den
                        // alten Weg fuer die Begruendung mit dem gemessenen
                        // y = -1,4285717.
                        if kollabierer_leerraum(ctx, child_id)
                            && (zeile_war_leer
                                || zeile.allein
                                || (zeile.letzter_leer && zeile.war_letzter()))
                        {
                            continue;
                        }
                        let p = inline_teil(
                            ctx,
                            child_id,
                            &InlineTeil {
                                kind: &child.kind,
                                rand: cm,
                                breite: sauber_pos(child_size.width),
                                hoehe,
                                content_breite: zeile.verfuegbar.unwrap_or(content.width),
                            },
                            &mut zeile,
                            &mut cursor,
                        );
                        if inline_zeilen.len() < 100_000 {
                            inline_zeilen.push((tasks.len(), zeile.breiten.len()));
                        }
                        // Inhalt in der Zeile trennt die Raender der Bloecke
                        // davor und danach (wie in `sum_children`).
                        if kollaps && !kollabierer_leerraum(ctx, child_id) {
                            vorige_unten = None;
                            inhalt_davor = true;
                        }
                        // Release 11.10.2026: ein Textlauf mit
                        // ausdruecklicher `line-height` belegt nur seinen
                        // Inhaltsbereich (siehe `text_inhaltsbereich`; nur
                        // auf dem Weg mit Plan — die handgebauten Dokumente
                        // ohne Plan behalten zeilenhohe Textkaesten).
                        let mut p = p;
                        if let Some((versatz, h, _)) = text_inhaltsbereich(ctx, child, hoehe) {
                            p.y += versatz;
                            p.hoehe = h;
                            groesse_lauf = Some(Size::new(sauber_pos(child_size.width), h));
                        } else if child.kind == NodeKind::Text {
                            // Release 11.10.2026 (Agent CSS2): wie in
                            // `lauf_im_durchschuss` — Innenabstand und Rahmen
                            // des Inline-Elements liegen um den Text.
                            let px = |l: narsil_core::length::Length| match l {
                                narsil_core::length::Length::Px(v) if v.is_finite() => v.max(0.0),
                                _ => 0.0,
                            };
                            p.y -= px(child.style.padding.top) + child.style.rahmen.gezeichnete_breite(0);
                        }
                        Platzierung {
                            x: p.x + dx,
                            y: p.y + dy,
                            breite: p.breite,
                            hoehe: p.hoehe,
                        }
                    } else {
                        // Der Block beendet die Zeile: alles, was in der
                        // Zeile stand, muss jetzt abgeschlossen werden.
                        zeile.schliesse_wenn_offen(&mut cursor);
                        // Release 09.10.2026: der wirksame obere Abstand —
                        // mit dem Vorgaenger zusammengefallen, an den
                        // Eltern abgegeben (erstes Kind) oder der eigene.
                        // `o` bleibt die Randkante: `build_tree` addiert
                        // `cm.top` selbst.
                        let (oben, unten) = if kollaps {
                            let (t, b) = sizes.wirksamer_rand(child_id, cm.top, cm.bottom);
                            let raeumt = hat_floats && cst.raeumen != narsil_layout::plan::RaeumungArt::Keins;
                            // Release 12.10.2026 (Agent CSS2): leerer Block am
                            // Anfang der Kette — wie `box::sum_children`.
                            let leer_anfang = vorige_unten.is_none()
                                && crate::r#box::leer_durchlaessig(ctx, child_id, child, child_size, raeumt);
                            // Mit dem Eltern nach aussen: an dessen Inhaltsanfang;
                            // sonst steht die Rahmenkante, als haette der Kasten
                            // einen unteren Rahmen (CSS 2.1 §8.3.1).
                            let leer_hoch = leer_anfang && !inhalt_davor && eltern_hoch_oben;
                            let oben = match vorige_unten {
                                _ if leer_hoch => 0.0,
                                _ if leer_anfang => t,
                                // Release 12.10.2026 (Agent CSS2): auch mit `clear`
                                // (siehe `box::sum_children`).
                                Some(u) => crate::r#box::rand_zusammen(u, t) - u,
                                _ if !inhalt_davor
                                    // Eigene Floats wandern mit dem Rand mit, geerbte nicht
                                    // (WPT `floats-clear/margin-collapse-142`).
                                    && (!raeumt
                                        || (cursor - content.y + t >= erbe.map_or(0.0, |e| e.clear_y(cst.raeumen, 0.0))
                                            && cursor - content.y
                                                >= floats
                                                    .zurueck(erbe.map_or(0, |e| e.anzahl()), 0.0, 0.0, 0.0)
                                                    .clear_y(cst.raeumen, 0.0)
                                                    .max(float_tief)))
                                    && eltern_hoch_oben =>
                                {
                                    0.0
                                }
                                _ => t,
                            };
                            // Leerer Block: beide Raender fallen mit der
                            // Kette zusammen (wie `box::sum_children`).
                            let (unten, kette) = match vorige_unten {
                                Some(u) if !raeumt && child_size.height <= 0.0 && child.kind == NodeKind::Container => {
                                    let k = crate::r#box::rand_zusammen(u, crate::r#box::rand_zusammen(t, b));
                                    (k - u - oben, k)
                                }
                                _ if leer_anfang => {
                                    let k = crate::r#box::rand_zusammen(t, b);
                                    (k - oben, k)
                                }
                                _ => (b, b),
                            };
                            if leer_hoch {
                                // Wandert mit nach aussen: kein Platz, die Kette
                                // bleibt offen.
                                (0.0, 0.0)
                            } else {
                                vorige_unten = Some(kette);
                                inhalt_davor = true;
                                (oben, unten)
                            }
                        } else {
                            (cm.top, cm.bottom)
                        };
                        // Release 11.10.2026 (Agent CSS2): `clear` setzt die
                        // RAHMENkante unter die Floats, oder laesst sie an
                        // ihrer gedachten Stelle, wenn die tiefer liegt (CSS
                        // 2.1 §9.5.2: Freiraum = das groessere von beidem) —
                        // derselbe Ausdruck wie in `box::sum_children`.
                        // GEMESSEN vorher: der obere Rand kam UNTER dem Float
                        // noch einmal dazu (bzw. zweimal, wenn kein Float im
                        // Weg war).
                        let mut rand_kante = cursor + oben;
                        if hat_floats && cst.raeumen != narsil_layout::plan::RaeumungArt::Keins {
                            rand_kante = content.y + floats.clear_y(cst.raeumen, rand_kante - content.y);
                        }
                        if cst.raeumen != narsil_layout::plan::RaeumungArt::Keins && float_tief > 0.0 {
                            rand_kante = rand_kante.max(content.y + float_tief);
                        }
                        // Release 11.10.2026 (Agent CSS2): ein Kasten, der
                        // Floats meidet, neben die Floats (sein Mass hat die
                        // Messphase schon angepasst) oder darunter — dieselbe
                        // Rechnung wie in `box::sum_children`.
                        let mut neben: Option<f32> = None;
                        if hat_floats && crate::r#box::meidet_floats(ctx, child_id, child) {
                            // Release 12.10.2026 (Agent CSS2): das Band des
                            // Kastens und schrittweise nach unten — wie
                            // `box::sum_children`.
                            for _ in 0..12 {
                                let (lo, ro) = floats.kanten_im_band_roh(rand_kante - content.y, sauber_pos(child_size.height));
                                let (l, r) = (lo.unwrap_or(0.0).max(0.0), ro.unwrap_or(0.0).max(0.0));
                                let (links, rechts) = (l.max(cm.left), r.max(cm.right));
                                // Release 12.10.2026 (Agent CSS2): siehe
                                // `box::sum_children` (`ragt_in_float`).
                                let ragt_in_float = ro.is_some_and(|rk| links + child_size.width > content.width - rk + 0.5)
                                    || (child.style.size.width != narsil_core::length::Length::Auto && lo.is_some_and(|lk| cm.left < 0.0 && cm.left < lk - 0.5));
                                if l <= 0.0 && r <= 0.0 && !ragt_in_float {
                                    break;
                                }
                                let roh = content.width - links - rechts;
                                if roh < 0.0 || (child_size.width > roh.max(0.0) + 0.5 && (l > 0.0 || r > 0.0)) || ragt_in_float {
                                    match floats.naechste_unterkante(rand_kante - content.y, sauber_pos(child_size.height)) {
                                        Some(u) if content.y + u > rand_kante => {
                                            rand_kante = content.y + u;
                                            continue;
                                        }
                                        _ => break,
                                    }
                                }
                                // Die Rahmenkante rechts neben den linken
                                // Floats (`build_tree` addiert `cm.left`).
                                neben = Some(links - cm.left);
                                break;
                            }
                        }
                        let o = rand_kante - cm.top;
                        if let Some(u) = sizes.float_ueberhang(child_id) {
                            float_tief = float_tief.max(rand_kante - content.y + u);
                        }
                        // Release 12.10.2026 (Agent CSS2): die Floats des Kindes
                        // gelten fuer die folgenden Geschwister (wie
                        // `box::sum_children`).
                        if kollaps
                            && crate::r#box::darf_raender_durchlassen(ctx, child_id, child)
                            && !crate::r#box::auto_rand(ctx, child_id)
                            && let Some(aus) = ctx.float_aus.borrow().get(&child_id).copied()
                        {
                            let (dl, dr, dy) = crate::r#box::kind_rahmen(child, cm, rand_kante - content.y, content.width)?;
                            let ab = ctx.float_erbe.borrow().get(&child_id).map_or(0, |e| e.anzahl());
                            floats.uebernehme(&aus.zurueck(ab, dl, dr, dy));
                            hat_floats = true;
                        }
                        cursor = rand_kante + hoehe + unten;
                        zeile.nach_block(cursor);
                        Platzierung {
                            // P9-NACHFOLGER (08:52): der Rand wird in
                            // `build_tree` addiert, nicht hier.
                            x: content.x
                                + neben.unwrap_or_else(|| {
                                    // Release 11.10.2026: `justify-self` als Lage
                                    // (CSS Box Alignment 3 §6.1), ohne `auto`-Raender.
                                    let rtl = ctx.plan.is_some_and(|p| p.richtung(id) == narsil_layout::Richtung::Rtl);
                                    if let Some(a) = crate::r#box::block_selbst_lage(ctx, child_id)
                                        && !cst.marge_links_auto
                                        && !cst.marge_rechts_auto
                                    {
                                        use narsil_layout::gitter::AusrLage as L;
                                        let frei = content.width - child_size.width - cm.left - cm.right;
                                        let f = match a.lage {
                                            L::Links => 0.0,
                                            L::Rechts => 1.0,
                                            L::Mitte => 0.5,
                                            L::Ende => if rtl { 0.0 } else { 1.0 },
                                            _ => if rtl { 1.0 } else { 0.0 },
                                        };
                                        let f = if a.sicher == Some(true) && frei < 0.0 { if rtl { 1.0 } else { 0.0 } } else { f };
                                        return frei * f;
                                    }
                                    auto_margin_versatz_x(
                                        child.style.margin,
                                        cst.marge_links_auto,
                                        cst.marge_rechts_auto,
                                        content.width,
                                        child_size.width,
                                        cm,
                                        ctx.plan.is_some_and(|p| p.richtung(id) == narsil_layout::Richtung::Rtl),
                                    )
                                })
                                + dx,
                            y: o + dy,
                            breite: sauber_pos(child_size.width),
                            hoehe,
                        }
                    }
                }
                Direction::Row => {
                    let o = cursor;
                    cursor += cm.left + child_size.width + cm.right;
                    let (dz, g) = lauf_im_durchschuss(ctx, child, content, durchschuss, child_size);
                    groesse_lauf = g;
                    Platzierung {
                        // P9-NACHFOLGER (08:52): siehe der Column-Zweig.
                        x: o + dx,
                        y: content.y + dz + dy,
                        breite: sauber_pos(child_size.width),
                        hoehe,
                    }
                }
            }
        };

        // L5 (02.10.2026 14:6x): DIE KLEB-REGEL, nach der normalen
        // Platzierung.
        //
        // GEMESSENE ENTSCHEIDUNG, warum HIER und nicht vorher: Sie
        // braucht die statische Lage, also das Ergebnis dieses `let`.
        // `relative_versatz_fuer` liefert fuer `sticky` (0, 0) — dort
        // stuende kein Versatz, den man kappen koennte.
        let p = if ist_klebend {
            let hoehe = sauber_pos(child_size.height);
            Platzierung {
                x: p.x,
                y: kleb_y(p.y, hoehe, cst.oben, cst.unten, content, if eltern_stil.ueberlauf != narsil_layout::plan::UeberlaufArt::Sichtbar {
                    *padding_box
                } else {
                    rollfeld.unwrap_or(Rect::new(0.0, 0.0, viewport.width, viewport.height))
                }),
                breite: p.breite,
                hoehe: p.hoehe,
            }
        } else {
            p
        };

        // W7 L1/L2: bei Flex und Raster legt der Eltern die GROESSE
        // fest, nicht nur die Stelle. Das ist der Unterschied zwischen
        // `1fr` (die Spalte teilt den Rest) und `auto` (die Box nimmt,
        // was sie braucht).
        //
        // GEMESSENER FEHLER 05.10.2026, 08:45 (19 harte Befunde auf
        // go.dev, `text-nichtsichtbar`): hier stand
        //     (p.breite - cm.left - cm.right).max(0.0)
        // Der Rand wurde abgezogen, aber er war in `p.breite` NIE drin.
        // `lege_zeile` rechnet mit `basis`, `wachstum`, `schrumpf` und
        // `gap` — es kennt gar keine Randwerte; `verteile_raster`
        // ebenso (Spaltenbreiten, `gap`, Hoehen). Der Rand kommt erst
        // bei der PLATZIERUNG dazu (`x: q.x + cm.left`), und
        // `build_tree` setzt ihn auf den Ursprung.
        //
        // GEMESSEN (crates/narsil-layout-engine/tests/flex_rand_zweimal.rs):
        //   Text 19,2 px, `margin-left: 12 px`, in einer Flex-Reihe
        //     Breite 16,80 - 12 = 4,80 px   (erwartet 16,80 px)
        //   dasselbe an der Hoehe
        //     Hoehe 20 - 12 = 8 px          (erwartet 20 px)
        //   und auf go.dev, wo der Rand GROESSER ist als der Teil:
        //     6,22 - 12 < 0 -> 0,00 px, ein Kasten ohne Breite
        //
        // CSS 2.1, 8.3.1: der Rand steht AUSSERHALB der Breite. Und
        // 10.3.2: die Basis eines `flex-basis: auto` ist die `width`
        // des Teils — die Breite ohne Rand.
        // added 05.10.2026, SERVER_14: **DIE TABELLE ZWINGT DIE GROESSE.**
        //
        // GEMESSENER FEHLER, den das behebt: `groesse` war `None`, weil
        // `verteilung` nur bei Flex und Raster gefuellt wird. Die Zelle
        // behielt dann ihre **gemessene** Breite — volle Tabellenbreite,
        // 400 px — und standen **untereinander**, jede 400 px breit.
        // `tabellen_leg.rs` hat es gemeldet: „Zelle 0: x 0,0 b 400,0 /
        // Zelle 1: x 0,0 b 400,0“.
        //
        // Die Regel: ein Kasten, dessen Platz aus dem Raster kommt, nimmt
        // **die** Groesse — das ist der Sinn des Feldes `PlaceTask::
        // groesse` (siehe dessen Kommentar: ein Kind in einer `1fr`-
        // Spalte behielt sonst die Breite aus der Messphase).
        let aus_raster = verteilung.is_some() || tabellen_platz.is_some();
        let groesse = if groesse_lauf.is_some() && !aus_raster {
                groesse_lauf
            } else if matches!(cst.position, PositionArt::Absolut | PositionArt::Fest) || aus_raster || cst.schweben != FloatArt::Keins {
                Some(Size::new(p.breite.max(0.0), p.hoehe.max(0.0)))
            } else {
                verteilung
                    .as_ref()
                    .map(|_| Size::new(p.breite.max(0.0), p.hoehe.max(0.0)))
                    .or_else(|| block_breite_im_eltern(ctx, child_id, child, &child_size, content, &cm))
            };
        // P9 (07:45): ein kollabiertes Leerzeichen am ZeilenANFANG bekam
        // hier eine eigene Groesse (0 px). Seit 08:05 gibt es den Kasten
        // dafuer nicht mehr (CSS 2.1, 16.6.1: "removed"), also bleibt die
        // Zeile ohne Nutzen. Sie bleibt stehen, weil sie den Weg zeigt,
        // den die Regel zuerst genommen hat.
        tasks.push(PlaceTask {
            id: child_id,
            available: if matches!(cst.position, PositionArt::Absolut | PositionArt::Fest) {
                if matches!(cst.position, PositionArt::Fest) {
                    viewport
                } else {
                    behaelter.size()
                }
            } else {
                flaeche.unwrap_or(content.size())
            },
            origin: Point::new(p.x, p.y),
            parent: Some(index),
            enthaltend: weiter,
            groesse,
            rollfeld: if eltern_stil.ueberlauf != narsil_layout::plan::UeberlaufArt::Sichtbar {
                Some(*padding_box)
            } else {
                rollfeld
            },
        });
    }

    // P9: die LETZTE Inline-Zeile muss noch abgeschlossen werden, sonst
    // traegt der Eltern ihre Hoehe nicht und die Seite schrumpft auf die
    // letzte Blockzeile. GEMESSEN wird das in
    // tests/inline_lauf.rs: zwei Inline-Bloecke und danach nichts.
    zeile.schliesse_wenn_offen(&mut cursor);

    // Release 11.10.2026: `text-align: center|right` im Inline-Fluss (Zeilen
    // mit Bildern, `inline-block`, `inline-flex`): jede Zeile wird um den
    // Rest ihrer Breite verschoben. GEMESSEN vorher: alle standen links
    // (WPT CSS2/positioning `right-*`-Referenzen, zentrierte Knoepfe/Logos).
    if richtung_alt == Direction::Column && verteilung.is_none() && !inline_zeilen.is_empty() {
        let a = ctx.plan.map(|p| p.ausrichtung(id));
        if matches!(a, Some(narsil_layout::Textausrichtung::Zentriert | narsil_layout::Textausrichtung::Rechts)) {
            for &(ti, nr) in &inline_zeilen {
                let Some(&b) = zeile.breiten.get(nr) else { continue };
                let rest = (content.width - b).max(0.0);
                let dx = if a == Some(narsil_layout::Textausrichtung::Zentriert) { rest / 2.0 } else { rest };
                if let Some(t) = tasks.get_mut(ti)
                    && ctx
                        .stil(t.id)
                        .is_none_or(|s| !matches!(s.position, PositionArt::Absolut | PositionArt::Fest) && s.schweben == FloatArt::Keins)
                {
                    t.origin.x += dx.min(MAX_ZEILENVERSATZ);
                }
            }
        }
    }

    // Generated paragraph rows carry the alignment of their HTML block.
    // Shift the complete row, keeping inline runs and their hit boxes together.
    if richtung_alt == Direction::Row && verteilung.is_none() {
        let a = ctx.plan.map(|p| p.ausrichtung(id));
        let rest = (content.x + content.width - cursor).max(0.0);
        let dx = match a {
            Some(narsil_layout::Textausrichtung::Zentriert) => rest / 2.0,
            Some(narsil_layout::Textausrichtung::Rechts) => rest,
            _ => 0.0,
        };
        for task in &mut tasks {
            if ctx.stil(task.id).is_none_or(|s| !matches!(s.position, PositionArt::Absolut | PositionArt::Fest)) {
                task.origin.x += dx;
            }
        }
    }
    Ok(Some(PlatzErgebnis::Fluss { tasks }))
}

/// P9 (05.10.2026): der Zustand EINER Inline-Zeile.
///
/// # WARUM EINE EIGENE STRUKTUR
///
/// Der Block-Cursor (`cursor`) zaehlt die Y-Position fuer Bloecke. Eine
/// Inline-Zeile hat eigene Groessen: sie beginnt an einer X-Position,
/// sie ist so breit wie ihre Teile zusammen, und sie ist so hoch wie der
/// hoechste Teil. Das in den Block-Cursor zu pressen hiesse, vier
/// Groessen in einer zu fuehren — und die Zeile waere dann nicht mehr
/// von einem Block zu unterscheiden, was genau der Fehler ist, den
/// P9 behebt.
struct ZeilenCursor {
    /// X, an dem die Zeile beginnt (der Inhaltsanfang des Eltern).
    ///
    /// GEMESSENER FEHLER: hier stand zuerst `content.y`. Ein Absatz mit
    /// Innenabstand stand damit um seinen linken Innenabstand zu weit
    /// links — bei `padding-left: 40` um 40 px.
    x_start: f32,
    /// Y der Oberkante der Zeile.
    y_start: f32,
    /// Wie breit die Zeile bisher belegt ist.
    braucht: f32,
    /// `true`, sobald mindestens ein Teil in der Zeile liegt.
    gleich: bool,
    /// Der groesste Grundlinien-Abstand der Teile (CSS Inline 3, §2.3).
    grundlinie: f32,
    /// Die Hoehe der Zeile: das groesste Mass eines Teils.
    zeilen_hoehe: f32,
    /// Die Y-Position, an der die Zeile endet (`y_start` + Hoehe).
    unten: f32,
    /// Wie viele Teile die Zeile nach der VORSCHAU noch hat, der
    /// aufrufende eingerechnet (P9, 07:50).
    ///
    /// Damit weiss der Lauf, ob der gerade gesetzte Teil der LETZTE der
    /// Zeile ist — und ob er ein kollabierbares Leerzeichen ist, das
    /// keinen Platz belegt (CSS 2.1, 16.6.1).
    rest: usize,
    /// Ist der letzte Teil der Zeile ein kollabierbares Leerzeichen?
    letzter_leer: bool,
    /// Steht das aufrufende Teil ALLEIN in seiner Zeile?
    ///
    /// GEMESSENER FEHLER, den es ohne dieses Feld gab: die Vorschau
    /// zaehlte EIN Teil, weil das zweite nicht mehr passte (20 + 70 > 60
    /// bei einem 60-px-Absatz). Der Lauf brach daraufhin um und setzte
    /// das Leerzeichen allein in die neue Zeile — ein Kasten bei
    /// y = -1,4285717, 1,43 px UEBER dem Anfang seiner Zeile. CSS 2.1,
    /// 16.6.1 sagt fuer eine solche Zeile: sie wird entfernt.
    ///
    /// Die Vorschau kann das nicht ueber `letzter_leer` sagen, denn sie
    /// endet VOR dem Teil, um das es geht.
    allein: bool,
    /// `true`, wenn `grundlinie` und `zeilen_hoehe` aus der Vorschau
    /// kommen und deshalb beim Setzen nicht neu gerechnet werden.
    aus_vorschau: bool,
    /// Release 11.10.2026: die Breiten der geschlossenen Zeilen (fuer
    /// `text-align` im Inline-Fluss, siehe `platziere_kinder`).
    breiten: Vec<f32>,
    /// Release 12.10.2026 (Agent CSS2): die Breite DIESER Zeile neben den
    /// Floats (CSS 2.1 §9.5: Zeilenboxen neben Floats sind verkuerzt);
    /// `None` = die volle Inhaltsbreite.
    verfuegbar: Option<f32>,
    /// Release 15.10.2026 (Agent CSS2): the part placed last (see
    /// `glued_to_previous`).
    prev: Option<narsil_dom::NodeId>,
}

impl ZeilenCursor {
    fn neu(x_start: f32, y_start: f32) -> Self {
        Self {
            x_start,
            y_start,
            braucht: 0.0,
            gleich: false,
            grundlinie: 0.0,
            zeilen_hoehe: 0.0,
            unten: y_start,
            rest: 1,
            letzter_leer: false,
            allein: false,
            aus_vorschau: false,
            breiten: Vec::new(),
            verfuegbar: None,
            prev: None,
        }
    }

    /// P9 (07:50): uebernimmt Grundlinie, Hoehe und Teilezahl aus der
    /// Vorschau (`crate::vorschau::zeilen_vorschau`).
    fn setze_vorschau(&mut self, v: crate::vorschau::Vorschau) {
        self.grundlinie = v.grundlinie;
        self.zeilen_hoehe = v.hoehe;
        self.rest = v.anzahl;
        self.letzter_leer = v.letzter_leer;
        self.allein = v.anzahl == 1;
        self.aus_vorschau = true;
    }

    /// Wie viele Teile stehen noch in dieser Zeile, der gerade gesetzte
    /// NICHT eingerechnet?
    fn uebrige(&self) -> usize {
        self.rest.saturating_sub(1)
    }

    /// War der gerade gesetzte Teil der LETZTE der Zeile?
    ///
    /// Nach einem Umbruch gehoert der neue Teil allein hinein, also ist er
    /// der letzte — und fuer ihn gab es keine Vorschau.
    fn war_letzter(&self) -> bool {
        self.rest <= 1
    }

    /// Setzt den Anfang der naechsten Zeile.
    ///
    /// NOETIG, WEIL EIN BLOCK DIE CURSOR-BEWEGUNG IN DER LUFT LAESST:
    /// `schliesse_wenn_offen` stellt `y_start` auf den Wert, den der
    /// Cursor beim Schluss der Zeile hatte. Danach koennte ein Block den
    /// Cursor noch weiterschieben, und die Zeile, die danach beginnt,
    /// wuerde dort anfangen, wo der Block schon vorbei ist.
    ///
    /// GEMESSENER FEHLER ohne diese Methode: A (30 px inline) y = 0,
    /// Block (40 px) y = 30, B (inline) y = 30 — B sass OBEN AUF dem
    /// Block, obwohl es als Zweites in Dokumentordnung im Fluss steht
    /// (CSS Inline 3, §2.1: der Block beendet die Zeile).
    fn nach_block(&mut self, y: f32) {
        self.y_start = y;
        self.unten = y;
    }

    /// Schliesst eine offene Zeile ab: sie waechst auf ihre gemessene
    /// Hoehe, und der Block-Cursor rueckt darunter.
    ///
    /// `cursor` wird nur WEITER gesetzt, nie zurueck: eine Inline-Zeile
    /// kann hoeher sein als der Block-Cursor, aber sie kann ihn nicht
    /// verkleinern — das ist der Unterschied zwischen „die Zeile hat
    /// Platz gebraucht" und „ein Block stand schon dort".
    fn schliesse_wenn_offen(&mut self, cursor: &mut f32) {
        if !self.gleich {
            return;
        }
        if self.breiten.len() < 100_000 {
            self.breiten.push(self.braucht);
        }
        self.unten = self.y_start + self.zeilen_hoehe;
        if *cursor < self.unten {
            *cursor = self.unten;
        }
        self.y_start = *cursor;
        self.unten = *cursor;
        self.braucht = 0.0;
        self.gleich = false;
        self.grundlinie = 0.0;
        self.zeilen_hoehe = 0.0;
        self.rest = 1;
        self.letzter_leer = false;
        self.allein = false;
        self.aus_vorschau = false;
    }
}

/// P9: Gehoert dieser Knoten in die Inline-Zeile?
///
/// * `NodeKind::Text` — ein Text ist per Definition Inline
///   (CSS 2.1, 9.2.2.1).
/// * `Anzeige::InlineBlock` — sieht aus wie inline, verhaelt sich wie
///   ein Block (CSS 2.1, 9.2.1.1).
/// * `NodeKind::Page` und `Anzeige::Block` NICHT: ein Block beendet die
///   Zeile (CSS Inline 3, §2.1).
pub(crate) fn inline_kind(
    ctx: &Ctx<'_>,
    id: narsil_dom::NodeId,
    cst: &narsil_layout::LayoutStil,
    kind: &narsil_dom::NodeKind,
) -> bool {
    match kind {
        narsil_dom::NodeKind::Text => true,
        narsil_dom::NodeKind::Container => {
            let _ = (ctx, id, cst);
            crate::r#box::inline_ebene(cst)
        }
        narsil_dom::NodeKind::Page => false,
    }
}

/// P9: gehoert der Knoten in die Inline-Zeile? Fuer die Messphase
/// (`box::sum_children`), die den Plan nicht hat.
///
/// In der Messphase wird der Stil ueber `stil_oder_standard` gelesen,
/// nicht ueber `ctx.stil` — ein Knoten ohne Planeintrag behaelt dort
/// seinen Standard, und `Anzeige::InlineBlock` ist nie der Standard.
pub(crate) fn kind_ist_inline(
    ctx: &Ctx<'_>,
    id: narsil_dom::NodeId,
    kind: &narsil_dom::NodeKind,
) -> bool {
    let cst = ctx.stil_oder_standard(id);
    inline_kind(ctx, id, &cst, kind)
}

/// P9: die Masse, die ein Inline-Teil zum Platzieren braucht.
///
/// Ein eigenes Gebilde statt neun Argumenten: `clippy` meldete
/// `too_many_arguments (9/7)`, und die Liste war nicht lesbar — man
/// musste beim Aufruf zaehlen, welches Argument welche Zahl ist. Zwei
/// Fehler an derselben Stelle sind damit einer.
struct InlineTeil<'a> {
    /// Knotenart: Text oder Kasten. Entscheidet die Grundlinie.
    kind: &'a narsil_dom::NodeKind,
    /// Aufgeloeste Randwerte (Margin).
    rand: narsil_core::geometry::Edges,
    /// Border-Box-Breite ohne Rand.
    breite: f32,
    /// Border-Box-Hoehe ohne Rand.
    hoehe: f32,
    /// Wie breit der Inhaltsbereich des Eltern ist (fuer den Umbruch).
    content_breite: f32,
}

/// P9: haengt EINEN Teil an die laufende Zeile.
///
/// Das ist die Regel, die CSS Inline 3 §2.2 und §6 in einem Stueck
/// sagt: passt er noch, sonst neue Zeile; dann wird die Grundlinie
/// gesetzt und der Versatz nach `vertical-align` berechnet.
///
/// # DIE UMBRUCHSCHRANKE, UND WARUM SIE DA IST
///
/// `self.gleich` verlangt der Umbruch absichtlich. Ohne sie wuerde ein
/// einzelnes Teil, das breiter ist als der Absatz, umbrechen, die Zeile
/// waere immer noch leer, sie wuerde wieder umbrechen — eine
/// Endlosschleife je Zeile. GEMESSEN in `tests/inline_lauf.rs`,
/// `ein_einzelnes_zu_breites_inline_element_bricht_nicht`.
///
/// MIT `!self.gleich` (also „Umbruch nur, wenn die Zeile noch leer
/// ist") war es die andere Richtung des Fehlers: `self.gleich` ist nach
/// dem ersten Teil wahr, und damit konnte der ZWEIT-Platz NIE
/// umbrechen. GEMESSEN: `60 + 40` px in einem 150-px-Absatz passt,
/// B stand trotzdem auf Zeile zwei (y = 30 statt y = 0).
fn inline_teil(
    ctx: &Ctx<'_>,
    id: narsil_dom::NodeId,
    teil: &InlineTeil<'_>,
    zeile: &mut ZeilenCursor,
    cursor: &mut f32,
) -> Platzierung {
    let InlineTeil {
        kind,
        rand,
        mut breite,
        hoehe,
        content_breite,
    } = *teil;

    // KOLLAPSIBLE LEERZEICHEN (CSS 2.1, 16.6.1). P9 (07:35).
    //
    // WAS BISHER PASSIERT: `narsil-fmt-html/src/tokenizer.rs:711` verwirft
    // Textknoten, die nur aus Leerzeichen bestehen. Die Begruendung dort
    // war richtig, solange jedes Kind eine eigene Zeile belegte: ein
    // Knoten aus einem Leerzeichen nahm eine Zeile Platz und zeigte
    // nichts. Seit dem Inline-Lauf ist das Gegenteil der Fall — er IST
    // der Zwischenraum zwischen zwei Woertern. Sichtbar auf wikipedia:
    // "English7,245,000+" statt "English 7,245,000+".
    //
    // WAS HIER GESCHIEHT, damit die Meldung an den Tokenizer reicht:
    //   * Ein solcher Teil am ANFANG der Zeile belegt keine Breite
    //     (CSS 2.1, 16.6.1: "a sequence of collapsible spaces at the
    //     beginning of a line is removed").
    //   * Er traegt NICHT zur Grundlinie und NICHT zur Zeilenhoehe bei —
    //     in der Mitte wie am Ende. Sonst wuerde ein 16-px-Leerzeichen am
    //     Zeilenende eine 10-px-Zeile auf 16 px strecken.
    //   * In der Mitte der Zeile bleibt seine Breite: das ist genau der
    //     Zwischenraum, um den es geht.
    //
    // GEMESSEN in `tests/leerzeichen_zeile.rs`.
    let kollabiert = kollabierer_leerraum(ctx, id);
    if kollabiert && !zeile.gleich {
        breite = 0.0;
    }
    let mit_rand = rand.left + breite + rand.right;
    if zeile.gleich && zeile.braucht + mit_rand > content_breite && !glued_to_previous(ctx, zeile.prev, id) {
        zeile.schliesse_wenn_offen(cursor);
    }
    zeile.prev = Some(id);
    // DIE GRUNDLINIE der Zeile ist das GROSSTE Grundlinien-Abstand der
    // Teile: ein 16-px-Text (Ascent 5/7 von 16 = 11,43 px) und eine
    // 30-px-Box (Grundlinie an der Unterkante, CSS 2.1, 10.8.1) heben
    // sich gegenseitig.
    // KOLLAPSIBLE LEERZEICHEN TRAGEN NICHT BEI (siehe oben), UND: nur
    // wenn die Zeile KEINE Vorschau hat. Grundlinie und Zeilenhoehe kommen
    // dann aus `zeilen_vorschau` und gelten fuer die GANZE Zeile.
    //
    // NUR `baseline` (und der Anfangswert) traegt zur Grundlinie bei: ein
    // `top` oder `middle` richtet sich NACH der Zeile, nicht die Zeile
    // nach ihm (CSS Inline 3, §6.1). GEMESSENER FEHLER der ersten Fassung
    // dieser Regel: ein 30-px-Kasten mit `top` neben 16-px-Text hob die
    // Grundlinie auf 30 und schob den Text 18,57 px nach unten, obwohl
    // `top` ihn an den Zeilenanfang haette kleben sollen.
    let (hoehe_rand, _) = teil_mit_rand(ctx, id, kind, hoehe, rand.top, rand.bottom);
    if !kollabiert && !zeile.aus_vorschau {
        if vertikal_von(ctx, id) == narsil_layout::Vertikal::Grundlinie {
            let (_, eigen) = teil_mit_rand(ctx, id, kind, hoehe, rand.top, rand.bottom);
            if eigen > zeile.grundlinie {
                zeile.grundlinie = eigen;
            }
        }
        if hoehe_rand > zeile.zeilen_hoehe {
            zeile.zeilen_hoehe = hoehe_rand;
        }
    }
    // DIE RANDWERTE STEHEN NICHT IM PLATZ.
    //
    // GEMESSENER FEHLER: hier stand `x_start + braucht + rand.left` und
    // `... + rand.top`. Die Randwerte kommen aber schon in
    // `build_tree` (Zeile 137: `task.origin + margin`) dazu — sie zu
    // addieren heisst, sie zu verdoppeln. GEMESSEN: `margin: 20` auf
    // einem Textknoten ergab (40, 40) statt (20, 20), in
    // `tests/margin_text.rs` (das diese Zahl seit dem 01.10. festhält).
    //
    // Fuer den VORSCHUB der Zeile zaehlen sie aber sehr wohl: ein Teil
    // mit 20 px Rand belegt 20 + breite + 20 px.
    let x = zeile.x_start + zeile.braucht;
    zeile.braucht += mit_rand;
    zeile.gleich = true;
    // P9 (07:50): der Teil zaehlt sich von der Zeile ab. Nach einem
    // Umbruch steht `rest` auf 0 — der Teil ist dann allein in der Zeile.
    zeile.rest = zeile.uebrige();
    Platzierung {
        x,
        y: vertikal_versatz(ctx, id, zeile, kind, hoehe, rand),
        breite,
        hoehe,
    }
}

/// P9: ist dieser Knoten ein kollabierbares Leerzeichen?
///
/// CSS 2.1, 16.6.1: eine Folge weisser Zeichen, die umgebrochen werden
/// kann. Ein Textknoten, der NICHHT LEER ist und nur aus solchen
/// Zeichen besteht, ist genau das. Ein leerer Knoten ist es nicht — der
/// nimmt ohnehin keinen Platz.
///
/// Die Aufrufer brauchen es ausserhalb von `inline_teil`, weil die
/// Breite aus der MESSPHASE kommt (siehe `PlaceTask.groesse` in
/// `build_tree`).
/// Release 15.10.2026 (Agent CSS2): is there NO soft wrap opportunity
/// between the inline part `prev` and the next part `next`, because one of
/// them is an atomic inline and the text on the other side glues to it?
/// UAX #14 (an atomic inline counts like an ideograph, CSS Text 3 §5.1):
/// nothing breaks after GL, WJ or ZWJ (LB8a, LB11, LB12), nor before GL, WJ,
/// ZWJ, combining marks or closing punctuation (LB9, LB11, LB12a, LB13).
/// Also between two text parts (the texts around an atomic inline are parts
/// of their own); not between two atomic inlines, and U+00A0 does not glue
/// (the web-compatible quirk, WPT `line-breaking-replaced-001`). MEASURED
/// before (WPT
/// css-text `line-breaking/line-breaking-atomic-003..027`, `A&#8239;<span
/// style="display:inline-block">B</span>` in 1ch): the inline-block went to
/// a second line.
pub(crate) fn glued_to_previous(ctx: &Ctx<'_>, prev: Option<narsil_dom::NodeId>, next: narsil_dom::NodeId) -> bool {
    use narsil_text::unicode_daten::linebreak_klassen::{Umbruchklasse as K, umbruch_klasse};
    let Some(prev) = prev else {
        return false;
    };
    let text_of = |id: narsil_dom::NodeId| {
        get_node(ctx, id).ok().filter(|n| n.kind == narsil_dom::NodeKind::Text).and_then(|n| n.text.as_deref())
    };
    let (vor, nach) = (text_of(prev), text_of(next));
    if vor.is_none() && nach.is_none() {
        return false;
    }
    if let Some(c) = vor.and_then(|t| t.chars().last())
        && c != '\u{a0}'
        && matches!(umbruch_klasse(c), K::Gl | K::Wj | K::Zwj)
    {
        return true;
    }
    // Closing punctuation only glues to text: after an atomic inline a line
    // may break before it (WPT `line-breaking-atomic-009`, `replaced-006`).
    if let Some(c) = nach.and_then(|t| t.chars().next())
        && c != '\u{a0}'
        && match umbruch_klasse(c) {
            K::Gl | K::Wj | K::Zwj | K::Cm => true,
            K::Cl | K::Cp | K::Ex | K::Is | K::Sy => vor.is_some(),
            _ => false,
        }
    {
        return true;
    }
    false
}

pub(crate) fn kollabierer_leerraum(ctx: &Ctx<'_>, id: narsil_dom::NodeId) -> bool {
    match get_node(ctx, id) {
        Ok(n) => match (&n.kind, n.text.as_deref()) {
            (narsil_dom::NodeKind::Text, Some(t)) => {
                !t.is_empty() && t.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000c}'))
            }
            _ => false,
        },
        Err(_) => false,
    }
}

/// P9 (07:50): sieht die Zeile voraus und fuellt den Cursor damit.
///
/// Ein dünner Rumpf um [`crate::vorschau::zeilen_vorschau`], damit beide
/// Wege (mit und ohne Planeintrag am Eltern) dieselbe Regel benutzen und
/// nicht je eine eigene.
fn vorschau(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    ab: usize,
    sizes: &Sizes,
    line_height: f32,
    content_breite: f32,
    n: usize,
) -> crate::vorschau::Vorschau {
    crate::vorschau::zeilen_vorschau(ctx, node, ab, sizes, line_height, content_breite, n)
}

/// P9: der `vertical-align`-Wert eines Knotens aus dem Plan.
///
/// Ohne Plan (der alte Weg) gilt der Anfangswert.
///
/// DIE EINE STELLE, DIE DEN PLAN DAFUER LIEST. `vertikal_versatz` und die
/// Zeilenvorschau (`crate::vorschau`) fragen beide hier — sonst koennte
/// der eine den Anfangswert nehmen und der andere nicht.
pub(crate) fn vertikal_von(ctx: &Ctx<'_>, id: narsil_dom::NodeId) -> narsil_layout::Vertikal {
    match ctx.plan {
        Some(p) => p.vertikal(id),
        None => narsil_layout::Vertikal::Grundlinie,
    }
}

/// Release 11.10.2026 (Agent CSS2): der STRUT einer Zeile im Inline-Fluss
/// (CSS 2.1 §10.8.1: jede Zeilenbox beginnt mit einer unsichtbaren
/// Inline-Box in Schrift und `line-height` des Blocks) — (Abstand der
/// Grundlinie von der Oberkante, Rest darunter). Dieselbe Rechnung wie fuer
/// einen Textlauf: bei `normal` die Grundlinie am Aufstieg und die Zeile
/// `zeilenhoehe_normal` hoch, sonst mit halbem Durchschuss. GEMESSEN vorher
/// (WPT `inline-replaced-width-001` u. a.): eine Zeile nur mit einem Bild
/// war so hoch wie das Bild — die Unterlaenge unter der Grundlinie fehlte.
pub(crate) fn strut(ctx: &Ctx<'_>, node: &narsil_dom::Node) -> (f32, f32) {
    let fs = node.style.font_size;
    // Nur auf dem Weg mit Plan (wie `text_inhaltsbereich`): die
    // handgebauten Dokumente ohne Plan behalten Zeilen so hoch wie ihr
    // hoechstes Teil.
    if !fs.is_finite() || fs <= 0.0 || ctx.stil(node.id).is_none() {
        return (0.0, 0.0);
    }
    let explizit = node.style.zeilenhoehe_art != narsil_core::style::ZeilenhoeheArt::Normal
        && node.style.zeilenhoehe_px.is_finite()
        && node.style.zeilenhoehe_px >= 0.0;
    // Auf ganze Pixel wie die Grundlinie in Browsern (siehe
    // `lauf_im_durchschuss`) — eine Zeile mit Bild bekaeme sonst eine
    // Bruchhoehe und alles darunter eine halbe Pixelkante.
    if !explizit {
        let l = crate::text::zeilenhoehe_normal(fs);
        let a = ctx.measurer.ascent(fs).round();
        return (a.max(0.0), (l - a).max(0.0));
    }
    let (a, d) =
        ctx.measurer.zeilen_metrik(fs, node.style.schriftgewicht, node.style.kursiv, &node.style.schriftfamilie);
    if !(a + d).is_finite() {
        return (0.0, 0.0);
    }
    let l = node.style.zeilenhoehe_px;
    let oben = (((l - (a + d)) / 2.0).round() + a).round();
    (oben, l - oben)
}

/// Release 11.10.2026 (Agent CSS2): die Grundlinie eines Textes auf ganze
/// Pixel — nur auf dem Weg mit Plan (wie `strut`; die handgebauten Dokumente
/// ohne Plan behalten die genaue Zahl).
fn ganz_mit_plan(ctx: &Ctx<'_>, node: &narsil_dom::Node, g: f32) -> f32 {
    if node.parent.is_some_and(|p| ctx.stil(p).is_some()) { g.round() } else { g }
}

/// Release 11.10.2026 (Agent CSS2): Hoehe und Grundlinie eines Teils der
/// Zeile mit seinen senkrechten RAENDERN (CSS 2.1 §10.8.1: fuer ersetzte
/// Elemente und `inline-block` zaehlt die Randbox; ohne eigene Grundlinie
/// steht die untere RANDkante auf der Grundlinie). Text bleibt, wie er ist.
/// Die Grundlinie zaehlt ab der oberen Randkante. GEMESSEN vorher (Hinweis
/// Koordinator): `<div><img style="margin-top:100px"></div>` war so hoch wie
/// das Bild, der naechste Block ueberlappte.
pub(crate) fn teil_mit_rand(
    ctx: &Ctx<'_>,
    id: narsil_dom::NodeId,
    kind: &narsil_dom::NodeKind,
    hoehe: f32,
    oben: f32,
    unten: f32,
) -> (f32, f32) {
    let g = grundlinie_von(ctx, id, kind, hoehe);
    if *kind == narsil_dom::NodeKind::Text {
        return (hoehe, g);
    }
    // `vertical-align: <Laenge>` hebt auch einen atomaren Kasten (der Text
    // traegt seine Hebung schon in `grundlinie_von`).
    let r = ctx.document.get(id).map_or(0.0, hebung);
    let gesamt = (oben + hoehe + unten).max(0.0);
    if g >= hoehe - 0.01 { (gesamt, gesamt + r) } else { (gesamt, oben + g + r) }
}

/// P9: der Grundlinien-Abstand eines Teils, vom oberen Kastenrand.
///
/// Text uses the same font-size ascent as its TextRun and rasterizer.
/// Its line-height or an explicit box height does not scale the glyphs.
/// Atomic boxes retain the existing bottom-edge baseline; deriving an
/// inline-block's baseline from its last line is a separate contract.
pub(crate) fn grundlinie_von(
    ctx: &Ctx<'_>,
    id: narsil_dom::NodeId,
    kind: &narsil_dom::NodeKind,
    hoehe: f32,
) -> f32 {
    match kind {
        // Release 11.10.2026: bei ausdruecklicher `line-height` liegt die
        // Grundlinie um den halben Durchschuss tiefer (`text_inhaltsbereich`).
        // Release 11.10.2026 (Agent CSS2): ein gehobener Text (`vertical-align`)
        // steht um seine Hebung hoeher — seine Grundlinie liegt tiefer im
        // Kasten. Die Grundlinie auf ganze Pixel wie der Strut (`strut`) —
        // sonst bekommt eine Zeile mit Text und Strut eine Bruchhoehe.
        narsil_dom::NodeKind::Text => ctx.document.get(id).map_or(0.0, |node| {
            ganz_mit_plan(ctx, node, text_inhaltsbereich(ctx, node, hoehe).map_or_else(|| ctx.measurer.ascent(node.style.font_size), |(_, _, g)| g))
                + hebung(node)
        }),
        // Release 09.10.2026: ein `inline-block`/`inline-flex` mit Text
        // richtet sich nach der Grundlinie seines Textes (CSS 2.1, 10.8.1),
        // nicht nach seiner Unterkante. GEMESSEN vorher: `Name <input>
        // <button>OK</button>` — Feld und Knopf standen ueber der Zeile,
        // der Text daneben an ihrer Unterkante (Chrome: Feldtext, Knopftext
        // und Fliesstext auf einer Linie). Genaehert mit dem ERSTEN Text
        // (die Kaesten im Fluss sind fast immer einzeilig: Knoepfe, Felder,
        // Marken); ohne Text bleibt die Unterkante.
        narsil_dom::NodeKind::Container
            if ctx.stil(id).is_some_and(|s| {
                (s.anzeige == narsil_layout::plan::Anzeige::InlineBlock
                    || (s.inline_aussen && s.anzeige == narsil_layout::plan::Anzeige::Flex)
                    // Release 11.10.2026 (Agent CSS2): `inline-table` — die
                    // Grundlinie ihrer ersten Zeile (CSS 2.1 §17.5.3).
                    || (s.inline_aussen && s.anzeige == narsil_layout::plan::Anzeige::Tabelle))
                    // `overflow` ungleich `visible`: die Unterkante (10.8.1).
                    && s.ueberlauf == narsil_layout::plan::UeberlaufArt::Sichtbar
            }) =>
        {
            // Release 11.10.2026 (Agent CSS2): ein `inline-block` richtet
            // sich nach seiner LETZTEN Zeile (CSS 2.1 §10.8.1), wenn sein
            // Inhalt aus Zeilenkaesten mit bekannter Hoehe besteht. GEMESSEN
            // vorher (WPT `inline-block-baseline-002`, `<span>&nbsp;<br/>
            // words</span>`): die Grundlinie der ersten Zeile.
            let ist_block = ctx.stil(id).is_some_and(|s| s.anzeige == narsil_layout::plan::Anzeige::InlineBlock);
            ist_block
                .then(|| letzte_zeilen_grundlinie(ctx, id))
                .flatten()
                .or_else(|| innere_grundlinie(ctx, id, 0))
                // Release 12.10.2026 (Agent CSS2): auf ganze Pixel wie die
                // Grundlinie eines Textlaufs (`ganz_mit_plan`) — sonst stehen
                // Text und `inline-block` mit demselben Text um den Bruchteil
                // versetzt und die Zeile wird 19,48 statt 19 px hoch (Hinweis
                // Agent A, WPT css-flexbox `flexbox-anonymous-items-001-ref`).
                .map(|g| if ctx.stil(id).is_some() { g.round() } else { g })
                .map_or(hoehe, |g| g.min(hoehe))
        }
        _ => hoehe,
    }
}

/// Release 11.10.2026 (Agent CSS2): die Grundlinie der letzten Zeile in
/// `id`, von der Rahmenoberkante — nur wenn alle Kinder im Fluss Kaesten mit
/// fester Hoehe sind (die Zeilenkaesten des Seitenbauers); sonst `None`.
fn letzte_zeilen_grundlinie(ctx: &Ctx<'_>, id: narsil_dom::NodeId) -> Option<f32> {
    let node = ctx.document.get(id)?;
    if node.children.len() < 2 || node.children.len() > 64 {
        return None;
    }
    let oben = crate::r#box::rahmen_edges(&node.style.rahmen).top
        + node.style.padding.top.resolve_or(0.0, 0.0).max(0.0);
    let mut y = 0.0f32;
    let mut letzte = None;
    for &k in &node.children {
        let kind = ctx.document.get(k)?;
        let st = ctx.stil_oder_standard(k);
        if !st.wird_gezeichnet()
            || matches!(st.position, PositionArt::Absolut | PositionArt::Fest)
            || st.schweben != FloatArt::Keins
        {
            continue;
        }
        if kind.kind != narsil_dom::NodeKind::Container {
            return None;
        }
        let narsil_core::length::Length::Px(h) = kind.style.size.height else {
            return None;
        };
        let rand_oben = kind.style.margin.top.resolve_or(0.0, 0.0);
        let rand_unten = kind.style.margin.bottom.resolve_or(0.0, 0.0);
        if let Some(g) = innere_grundlinie(ctx, k, 1) {
            letzte = Some(y + rand_oben + g);
        }
        y += rand_oben + h + rand_unten;
    }
    letzte.map(|g| oben + g)
}

/// Release 09.10.2026: Abstand der Grundlinie des ersten Textes in `id`
/// von dessen Rahmenoberkante — entlang der ersten Kinder (Rahmen, Fuellung
/// und Rand oben addiert), `None` ohne Text. Ein LEERER Textknoten zaehlt
/// mit: ein leeres Eingabefeld traegt einen (seine Grundlinie ist die
/// seines unsichtbaren Textes, wie in Chrome).
fn innere_grundlinie(ctx: &Ctx<'_>, id: narsil_dom::NodeId, tiefe: usize) -> Option<f32> {
    if tiefe > 8 {
        return None;
    }
    let node = ctx.document.get(id)?;
    let oben = crate::r#box::rahmen_edges(&node.style.rahmen).top
        + node.style.padding.top.resolve_or(0.0, 0.0).max(0.0);
    for &k in node.children.iter().take(4) {
        let kind = ctx.document.get(k)?;
        match kind.kind {
            narsil_dom::NodeKind::Text => {
                // Release 11.10.2026: mit dem halben Durchschuss der Zeile
                // (siehe `text_inhaltsbereich`) — die Zeilenbox ist so hoch
                // wie ihre `line-height`.
                let zeile = match node.style.size.height {
                    narsil_core::length::Length::Px(h) if h.is_finite() && h > 0.0 => h,
                    _ => kind.style.zeilenhoehe_px,
                };
                let g = text_inhaltsbereich(ctx, kind, zeile)
                    .map_or_else(|| ctx.measurer.ascent(kind.style.font_size), |(_, _, g)| g);
                // Release 11.10.2026 (Agent CSS2): auf ganze Pixel wie der Strut.
                let g = ganz_mit_plan(ctx, kind, g);
                return Some(oben + g);
            }
            narsil_dom::NodeKind::Container => {
                let rand = kind.style.margin.top.resolve_or(0.0, 0.0);
                if let Some(g) = innere_grundlinie(ctx, k, tiefe + 1) {
                    return Some(oben + rand + g);
                }
                return None;
            }
            _ => return None,
        }
    }
    None
}

/// P9: der Y-Versatz eines Teils nach seinem `vertical-align`
/// (CSS Inline 3, §6).
///
/// * `Grundlinie` — die Grundlinie des Teils auf die der Zeile.
/// * `Mitte` — die Mitte des Teils auf die Mitte der Zeile.
/// * `Oben` / `Unten` — die Kanten aneinander.
///
/// `zeile.y_start` STEHT IN JEDEM ZWEIG, und das ist ein gemessener
/// Fehler: die erste Fassung rechnete den Versatz RELATIV zur Zeile
/// und vergab dann, ihn zur Zeile zu addieren. Bei einem 30-px-Kasten
/// mit Grundlinie an der Unterkante ergab das `0 - 30 = -30`, und der
/// Kasten stand 30 px UEBER dem Anfang seiner Zeile. GEMESSEN: Bild
/// y = 30, Absatz y = 0, Text y = 16 — die Teile lagen in drei
/// verschiedenen Zeilen, und der Absatz war 20 px hoch statt 30.
fn vertikal_versatz(
    ctx: &Ctx<'_>,
    id: narsil_dom::NodeId,
    zeile: &ZeilenCursor,
    kind: &narsil_dom::NodeKind,
    hoehe: f32,
    rand: narsil_core::geometry::Edges,
) -> f32 {
    let v = vertikal_von(ctx, id);
    let o = zeile.y_start;
    // Release 11.10.2026 (Agent CSS2): mit der Randbox (`teil_mit_rand`);
    // das Ergebnis ist die obere RANDkante, `build_tree` addiert den Rand.
    let (h, g) = teil_mit_rand(ctx, id, kind, hoehe, rand.top, rand.bottom);
    match v {
        narsil_layout::Vertikal::Grundlinie => o + zeile.grundlinie - g,
        narsil_layout::Vertikal::Mitte => o + (zeile.zeilen_hoehe - h) / 2.0,
        narsil_layout::Vertikal::Oben => o,
        narsil_layout::Vertikal::Unten => o + zeile.zeilen_hoehe - h,
    }
}

/// Verteilt die Kinder eines Flex-Containers (L1).
/// **DER TABELLEN-LEG**, aufgerufen fuer einen Knoten mit
/// `display: table`.
///
/// added 05.10.2026, Rolle SERVER (SERVER_14). Das ist der erste Aufruf
/// von `lege_tabelle` aus `src/` — GEMESSEN vorher: null.
///
/// # WAS ZURUECKGEGEBEN WIRD
///
/// Eine Karte `Zelle -> Rect` **in absoluten Koordinaten**, plus der
/// Eintrag in `ctx.tabellen`. Die Karte wird hier gebraucht, damit die
/// Tabelle ihre Zeilen gleich richtig aufsetzen kann; `ctx.tabellen`
/// braucht die Zeile spaeter, wenn sie selbst besucht wird.
///
/// # DIE DREI WERTE, DIE VON `lege_tabelle` HEREIN KOMMEN
///
/// * **Zeilenhöhe** = das Maximum der Zellhöhen (CSS 2.1, 17.6) — das
///   macht `lege_tabelle` schon.
/// * **Spaltenbreite** = aus allen Zeilen zusammen.
/// * **`border-spacing`** — GEMESSEN an Firefox 157 (`SERVER_10`): **2 px**
///   zwischen jeder Zeile. Das ist der Abstand aus dem UA-Blatt
///   (`table { border-spacing: 2px }`), und es sind die **2.00 px**, um die
///   die HN-Liste von uns gegenueber Firefox noch zuruecklag (34.00 gegen
///   36.00 px je Meldung).
fn lege_tabelle_alle(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    content: &Rect,
    eltern_stil: &narsil_layout::LayoutStil,
    sizes: &Sizes,
) -> Result<HashMap<NodeId, Rect>, LayoutError> {
    use crate::tabelle::{Zelle, lege_tabelle};

    // 1. DIE ZEILEN und ihre Zellen einsammeln.
    //
    // DIE REIHENFOLGE ist die Dokumentreihenfolge — nicht die
    // Structreihenfolge, nicht die Broessenordnung. `lege_tabelle`
    // braucht die Zeilen in der Reihenfolge, in der sie im HTML stehen.
    //
    // added 04.10.2026, Rolle LAYOUT (LAYOUT_1): **DURCH ZEILENGRUPPEN
    // HINDURCH.** `zeilen_von_tabelle` statt `node.children` — der
    // Grund ist GEMESSEN, siehe dort.
    let mut zeilen: Vec<crate::tabelle::Zeile> = Vec::new();
    let mut register: ZellRegister = Vec::new();
    let zeilen_ids = zeilen_von_tabelle(ctx, node.id)?;

    for &zeilen_id in zeilen_ids.iter() {
        let zeilen_node = get_node(ctx, zeilen_id)?;
        let mut zellen: Vec<Zelle> = Vec::new();
        let mut ids: Vec<(NodeId, u32)> = Vec::new();
        // DIE LAUFENDE SPALTE: sie waechst um `colspan`, nicht um 1.
        let mut spalte: u32 = 0;
        for &zellen_id in zeilen_node.children.iter() {
            let z_rolle = ctx.stil(zellen_id).map_or(Anzeige::Block, |s| s.anzeige);
            if z_rolle != Anzeige::TabelleZelle {
                continue;
            }
            let (colspan, rowspan) = ctx
                .stil(zellen_id)
                .map_or((1, 1), |s| (s.colspan.max(1), s.rowspan.max(1)));
            // DIE ZELLHOEHE: das, was die Messphase fuer diese Zelle
            // ermittelt hat. `lege_tabelle` macht daraus das Maximum der
            // Zeile (CSS 2.1, 17.6).
            let hoehe = sizes.get(zellen_id).map_or(0.0, |s| s.height);
            // Release 07.10.2026: die Spalten nach INHALT verteilen (CSS 2.1
            // §17.5.2.2). Hier stand `min_breite/max_breite: None` — und
            // `lege_tabelle` machte daraus gleich breite Spalten. GEMESSEN auf
            // news.ycombinator.com (1888 px): Rang-, Pfeil- und Titelspalte je
            // ~630 px, Titel bei x=1259, Unterzeile bei x=633.
            let min = sizes.min_inhalt_breite(ctx, zellen_id, 0);
            let max = sizes.max_content_breite(ctx, zellen_id, 0).max(min);
            let feste_breite = ctx.document.get(zellen_id).and_then(|n| match n.style.size.width {
                narsil_core::length::Length::Px(v) if v.is_finite() && v > 0.0 => Some(v),
                _ => None,
            });
            zellen.push(Zelle {
                spalten: colspan,
                zeilen: rowspan,
                breite: feste_breite,
                hoehe,
                min_breite: Some(min),
                max_breite: Some(max),
            });
            ids.push((zellen_id, spalte));
            spalte += colspan.max(1) as u32;
        }
        // added 04.10.2026, Rolle LAYOUT (LAYOUT_1): **DIE ZEILE BLEIBT IN
        // DER LISTE, AUCH WENN SIE KEINE ZELLE HAT.** Vorher stand hier
        // `if zellen.is_empty() { continue; }`, und die zweite Schleife lief
        // ueber `node.children` — die beiden Schleifen zaehlten also
        // **verschiedene** Zeilen. Ab der ersten leeren Zeile (dem
        // HN-`<tr class="spacer">`) war `zeilen_index` um eins daneben, und
        // `Tabelle::zeilenhoehen[index]` gehoerte zur falschen Zeile.
        //
        // GEMESSENE GRENZE dieser Entscheidung: **beide** Schleifen benutzen
        // jetzt dieselbe Liste (`zeilen_ids`), also stimmen die Indizes
        // ueberein. **Offen bleibt** die 5-px-Hoehe des Spacers: sie kommt
        // nicht aus den Zellen, sondern aus `sizes`, und die Frage, welche
        // Hoehe eine Zeile ganz ohne Zelle bekommen soll, gehoert an
        // `tabelle.rs` (Rolle SERVER) — nicht hierher.
        zeilen.push(crate::tabelle::Zeile { zellen });
        register.push(ids);
    }

    if zeilen.is_empty() {
        return Ok(HashMap::new());
    }

    // 2. DER LEG. `border-spacing` ist der GEMESSENE Wert aus dem
    //    UA-Blatt: 2 px zwischen jeder Zeile.
    // Release 11.10.2026: `border-spacing` aus dem Plan (UA-Blatt:
    // `table { border-spacing: 2px }`; `display: table` und anonyme
    // Tabellen: 0). Ohne Planeintrag (Seite ohne Kaskade) die UA-Zahl.
    let luecke = ctx
        .stil(id_von(ctx, node))
        .map_or(BORDER_SPACING_UA, |s| s.luecke);
    let tabelle = lege_tabelle(content.width, &zeilen, luecke, None);

    // 3. DIE ZUORDNUNG: `ZellenPlatz` zaehlt Zeilen und Spalten, wir
    //    brauchen die **Knoten**. Deshalb laeuft `zellen_von_zeile` im
    //    gleichen Schritt mit — ein Zellenplatz ohne seinen Knoten waere
    //    nichts, was man setzen koennte.
    // 3. DIE ZUORDNUNG: `lege_tabelle` zaehlt Zeilen und Spalten, wir
    //    brauchen die **Knoten**. Das `register` traegt zu jeder Zeile die
    //    Zellen **mit der Spalte, auf der sie beginnt** — mit `colspan`
    //    stimmt der Listenindex nicht mit dem Spaltenindex ueberein.
    //
    //    `zeilen_ecke` haelt je Zeile Ursprung, Breite, Hoehe und die
    //    Zellrechtecke; daraus wird unten das Rechteck der Zeile.
    let mut raus: HashMap<NodeId, Rect> = HashMap::new();
    struct ZeilenEcke {
        x: f32,
        // added 04.10.2026, Rolle LAYOUT (LAYOUT_1): **die y der Zeile.**
        // Ohne sie bekam jede Zeile `content.y` (siehe Schritt 4).
        y: f32,
        breite: f32,
        hoehe: f32,
    }
    // added 05.10.2026, SERVER_15: die Karten tragen die **absoluten**
    // Koordinaten; `kind_platzieren` schreibt sie unveraendert in die
    // `PlaceTask` (`origin: Point::new(p.x, p.y)`). added 04.10.2026, Rolle
    // LAYOUT (LAYOUT_2): gegenueber SERVER_15 **wieder absolut** — siehe die
    // Begruendung in der Schleife ueber `tabelle.zellen`, dort ist sie
    // ausgeschrieben.
    let mut zeilen_ecke: Vec<ZeilenEcke> = Vec::new();
    for platz in tabelle.zellen.iter() {
        let zi = platz.zeile as usize;
        while zeilen_ecke.len() <= zi {
            zeilen_ecke.push(ZeilenEcke {
                x: f32::INFINITY,
                y: f32::INFINITY,
                breite: 0.0,
                hoehe: 0.0,
            });
        }
        let e = &mut zeilen_ecke[zi];
        // added 04.10.2026, Rolle LAYOUT (LAYOUT_2): **ABSOLUT, und das ist
        // die ganze Correction gegenueber SERVER_15.**
        //
        // GEMESSENER FEHLER, den das behebt (`body { margin: 40px }`, eine
        // 300-px-Tabelle mit 2 Zellen, `tests/tabelle_hoehe_ist.rs::
        // tabelle_respektiert_den_rand_des_body`):
        //
        //   Tabelle        x  40,00  <- richtig
        //   Zeile          x   0,00  <- FALSCH: der Rand des `body` weg
        //   Zelle 0        x   0,00  <- FALSCH
        //   Zelle 1        x 151,00  <- FALSCH
        //
        // **WARUM SERVER_15 auf relativ ging und es trotzdem falsch war:**
        // die Begruendung dort lautete „die Platzierung addiert den
        // Elternursprung **noch einmal**“. Sie tut es **nicht**:
        // `kind_platzieren` schreibt `origin: Point::new(p.x, p.y)` direkt
        // in die `PlaceTask` (Zeile 1612), und `p` ist damit **absolut** —
        // der Zweig `5` rechnet selbst `content.x + q.x + dx`. Ein relativer
        // Wert im Karteneintrag verliert deshalb genau **eine** Sache: den
        // Rand aller Vorfahren.
        //
        // **UND DIE MESSUNG VON SERVER_15 WAR ZUOERST SCHON EIN ANDERER
        // FEHLER:** dort lagen *beide* Zellen auf x = 201. Zwei Zellen auf
        // derselben x sind nicht das Relative-vs-Absolut-Problem, sondern
        // der `zi == 0`-Zweig in derselben Schleife (siehe weiter unten),
        // der `e.x` bei jeder Zelle der ersten Zeile ueberschrieben hat.
        // Der Rand des `body` stand in diesem Test auf **0**, deshalb war
        // relativ und absolut dort **nicht unterscheidbar**. Beide Fehler
        // sind jetzt behoben und der zweite ist gemessen.
        let r = Rect {
            x: content.x + platz.x,
            y: content.y + platz.y,
            width: platz.breite,
            height: platz.hoehe,
        };
        // added 04.10.2026, Rolle LAYOUT (LAYOUT_1): **das kleinste x, und
        // nichts sonst.**
        //
        // GEMESSENER FEHLER, den das behebt (2 Zeilen, 3 Spalten, 600 px,
        // echtes HTML mit `<tbody>`, vorher/nachher
        // `tests/tabelle_html_ist.rs`):
        //
        //   vorher:  Zeile 0 x 401,33   <- die x der DRITTEN Zelle
        //   nachher: Zeile 0 x 0,00
        //
        // Die Bedingung war `if zi == 0 || r.x < e.x`. Fuer die **erste**
        // Zeile war der erste Teil **immer** wahr, also uebernahm `e.x` bei
        // jeder Zelle den Wert der jeweils **letzten** — die Zeile stand
        // dort, wo ihre rechte Spalte beginnt, und ihre Zellen lagen bis auf
        // die erste **ausserhalb** ihres Kastens. Bei `zi > 0` wirkte die
        // zweite Haelfte, und `e.x` blieb bei `0.0` stehen — das war
        // zufaellig richtig, solange die erste Spalte bei 0 beginnt, und
        // falsch fuer jede eingerueckte Tabelle.
        //
        // Der Anfangswert ist darum `f32::INFINITY` und nicht `0.0`: das
        // kleinste x der Zellen ist auch dann richtig, wenn die Tabelle
        // nicht bei 0 beginnt.
        if r.x < e.x {
            e.x = r.x;
        }
        // added 04.10.2026, Rolle LAYOUT (LAYOUT_1): das kleinste y der
        // Zellen dieser Zeile **ist** die y der Zeile (CSS 2.1, 17.6: die
        // Zeile beginnt, wo ihre erste Zelle beginnt).
        if r.y < e.y {
            e.y = r.y;
        }
        e.breite = e.breite.max(r.x + r.width - e.x);
        e.hoehe = e.hoehe.max(r.height);
        if let Some(id) = zelle_in_register(&register, zi, platz.spalte) {
            raus.insert(id, r);
        }
    }

    // 4. DIE ZEILEN bekommen ihre Rechtecke.
    //
    //    Eine Zeile **ohne** Zellen — der HN-`<tr class="spacer">` — hat
    //    kein Rechteck aus Zellen, braucht aber ihre **5 px Hoehe**,
    //    sonst verschwindet der Abstand zwischen zwei Meldungen. Deshalb
    //    wird hier ueber `tabelle.zeilenhoehen` gelaufen und **nicht** nur
    //    ueber die Zellen.
    let mut zeilen_rechtecke: Vec<(NodeId, Rect)> = Vec::new();
    let mut zeilen_index = 0usize;
    // added 04.10.2026, Rolle LAYOUT (LAYOUT_1): `zeilen_ids` und nicht
    // `node.children` — dieselbe Liste wie in Schleife 1, damit
    // `zeilen_index` auf `Tabelle::zeilenhoehen` zeigt. Siehe dort.
    for &zeilen_id in zeilen_ids.iter() {
        let hoehe = tabelle
            .zeilenhoehen
            .get(zeilen_index)
            .copied()
            .unwrap_or(0.0);
        let (x, y, breite) = zeilen_ecke
            .get(zeilen_index)
            .map_or((content.x, content.y, content.width), |e| {
                (e.x, e.y, e.breite)
            });
        zeilen_index += 1;
        zeilen_rechtecke.push((
            zeilen_id,
            Rect {
                // added 04.10.2026, Rolle LAYOUT (LAYOUT_1): **die y der
                // Zeile**, nicht `content.y` fuer alle.
                //
                // GEMESSENER FEHLER (2 Zeilen, 3 Zellen, 600 px, echtes HTML
                // mit `<tbody>`, `tests/tabelle_html_ist.rs`):
                //
                //   vorher:  Zeile 0 y 0,  Zeile 1 y 57  (Zeile 1 lag
                //            INNERHALB von Zeile 0)
                //   nachher: Zeile 0 y 0,  Zeile 1 y 21
                //
                // added 04.10.2026, Rolle LAYOUT (LAYOUT_2): `content.x` /
                // `content.y` sind **wieder** drin (der Rueckfall), denn die
                // Karten sind absolut — siehe die Schleife ueber
                // `tabelle.zellen`. `e.x`/`e.y` tragen den Ursprung des
                // Tabellen-Inhaltskastens **schon**, weil sie aus `r`
                // gebildet werden.
                x,
                y,
                width: breite,
                height: hoehe,
            },
        ));
    }

    // added 05.10.2026, SERVER_14/15: **beide Rechteckkarten in die
    // Rechnung schreiben.**
    //
    // Sie gehoeren zum Ergebnis, weil Zeile **und** Zelle sie ueber ihre
    // Tabelle finden und nicht ueber einen zweiten Weg. `raus` enthaelt die
    // **Zellen**rechtecke nach `NodeId`, `zeilen_rechtecke` die der
    // Zeilen — beide in der Struktur, die die Rechnung zurueckgibt.
    //
    // GEMESSENE URSACHE fuer zwei Karten (SERVER_14): `eltern_stil` ist der
    // Stil des Knotens **selbst**. Die Zeile sieht `TabelleZeile` und
    // erreicht ihre Tabelle ueber `node.parent`; die Zelle sieht
    // `TabelleZelle` und liegt **einen Schritt tiefer`. Mit Karten nach
    // Knoten muss jede Ebene nur noch ihre **eigene** Nummer kennen.
    let mut t_mit_karten = tabelle.clone();
    t_mit_karten.zeilen_rechtecke = zeilen_rechtecke
        .iter()
        .map(|(id, r)| ((*id).index() as u32, *r))
        .collect();
    t_mit_karten.zellen_rechtecke = raus
        .iter()
        .map(|(id, r)| ((*id).index() as u32, *r))
        .collect();

    ctx.tabellen
        .borrow_mut()
        .insert(id_von(ctx, node), t_mit_karten);
    Ok(raus)
}

/// Der Abstand zwischen Zellen und Zeilen aus dem UA-Blatt.
///
/// GEMESSEN an Firefox 157 (`SERVER_10`): **2 px**. Die Rekonstruktion der
/// Itemlist zeigte genau diese 2 px zwischen jeder Zeile — sie sind die
/// **2.00 px**, um die unsere HN-Liste von Firefox (36.00 px) zuruecklag.
///
/// added 04.10.2026, Rolle LAYOUT (LAYOUT_2): `pub(crate)`, weil die
/// Messphase in `box.rs` den Abstand zwischen den Zeilen rechnen muss und
/// ihn nicht **zweimal** festlegen darf.
pub(crate) const BORDER_SPACING_UA: f32 = 2.0;

/// Wie viele Ebenen zwischen einer Zeile und ihrer Tabelle liegen.
///
/// added 04.10.2026, Rolle LAYOUT (LAYOUT_1).
///
/// GEMESSEN an echter HTML-Tabelle (`tests/tabelle_html_ist.rs`,
/// Produktionsweg `seite_aus_text` -> `layout_mit_plan`):
///
/// ```text
/// Knoten 3 Tabelle      parent = Some(2)
/// Knoten 4              <- <tbody>, vom HTML-Parser EINGESETZT
/// Knoten 5 TabelleZeile parent = Some(4)      <- NICHT Some(3)
/// Knoten 6 TabelleZelle parent = Some(5)
/// ```
///
/// Der HTML-Parser setzt die Zeilengruppe auch dann, wenn sie im Quelltext
/// nicht steht (HTML5, die „in body"-Regeln). Deshalb ist die Zeile in
/// echten Seiten **nicht** ein Kind der Tabelle, und `node.parent` allein
/// findet sie dort nie.
///
/// **WARUM DIES HIER UND NICHT IM PLAN:** `Anzeige` unterscheidet `<tr>`
/// und `<tbody>` **nicht** — beide tragen `Anzeige::TabelleZeile`
/// (`narsil-css/src/kaskade.rs:3647`: „`table-row` | `table-row-group` |
/// `table-header-group` | `table-footer-group`"). Die Kaskade hat die
/// Information also **nicht**; sie muesste dort neu aufgenommen werden, und
/// das ist `narsil-css` und `narsil-layout` (Rollen CSS und LAYOUT) — nicht
/// `flow.rs`.
///
/// **DER WEG, DEN ICH STATTDESSEN GEHE:** die Unterscheidung ist in der
/// **Struktur** enthalten, und die braucht kein neues Feld. Eine Zeile ist,
/// was **Zellen** hat; eine Zeilengruppe ist, was Zeilen hat.
/// [`zeilen_von_tabelle`] entscheidet danach, und
/// [`tabelle_ueber_zeile`] laeuft nach oben, bis es in `ctx.tabellen` — der
/// Map, in der nur **Tabellen** stehen — etwas findet. Das ist eindeutig,
/// weil `lege_tabelle_alle` als Einziges dort eintruegt.
const TABELLE_UEBER_ZEILE: usize = 3;

/// Die Tabelle, zu der diese Zeile gehoert — auch ueber eine Zeilengruppe.
///
/// added 04.10.2026, Rolle LAYOUT (LAYOUT_1). Ersetzt `node.parent` an den
/// zwei Stellen in `kind_platzieren`, weil das in echten Seiten die
/// Zeilengruppe trifft und nicht die Tabelle (siehe
/// [`TABELLE_UEBER_ZEILE`]).
///
/// **WAS DIE SUCHE EINDEUTIG MACHT:** `ctx.tabellen` behaelt ausschliesslich
/// Eintraege von `lege_tabelle_alle`, und das ist ausschliesslich eine
/// Tabelle. Die erste Uebereinstimmung nach oben ist deshalb die richtige,
/// auch wenn zwischen Zeile und Tabelle eine Gruppe liegt.
fn tabelle_ueber_zeile(ctx: &Ctx<'_>, zeilen_id: NodeId) -> Option<NodeId> {
    let t = ctx.tabellen.borrow();
    let mut kandidat = zeilen_id;
    for _ in 0..TABELLE_UEBER_ZEILE {
        kandidat = get_node(ctx, kandidat).ok()?.parent?;
        if t.contains_key(&kandidat) {
            return Some(kandidat);
        }
    }
    None
}

/// Die Zeilen einer Tabelle in Dokumentreihenfolge — **durch Zeilengruppen
/// hindurch**.
///
/// added 04.10.2026, Rolle LAYOUT (LAYOUT_1). Der Ersatz fuer
/// `node.children` in `lege_tabelle_alle`.
///
/// # DER GEMESSENE FEHLER, DEN DAS BEHEBT
///
/// `lege_tabelle_alle` sah nur die **direkten** Kinder der Tabelle und
/// behielt nur, was die Rolle `TabelleZeile` oder `TabelleRand` hatte. In
/// echten Seiten sitzt dazwischen das vom Parser eingesetzte `<tbody>`.
/// GEMESSEN (`tests/tabelle_html_ist.rs`, vor der Aenderung): die Tabelle
/// bekam **0** Zeilen, `lege_tabelle_alle` lief ueber
/// `if zeilen.is_empty() { return Ok(HashMap::new()) }` hinaus und trug
/// **nichts** in `ctx.tabellen` ein. Damit hatte der ganze Tabellenpfad
/// keine Wirkung — und die Seite sah aus wie drei Bloecke ueber
/// Bloecke. Genau das ist das Bild, das der Befund aus
/// `team/STATUS_SERVER.txt` (Zeile 52) beschreibt.
///
/// # WER ALS GRUPPE ZAEHLT
///
/// Ein Kind mit Zeilenrolle, dessen eigene Kinder **Zellen** tragen, ist
/// eine Zeile. Sonst ist es eine Zeilengruppe, und die Suche geht eine
/// Ebene tiefer. Das braucht kein neues Feld und trifft beide Faelle:
/// * echtes HTML — `<table><tbody><tr><td>`: `tbody` hat keine Zellen,
///   geht tiefer, `tr` hat welche, ist eine Zeile;
/// * der Hand-Plan aus `tests/tabellen_leg.rs` — `<table><tr><td>` ohne
///   Gruppe: `tr` hat Zellen, ist eine Zeile, es wird gar nicht tiefer
///   gegangen.
///
/// # DIE TIEFE
///
/// `ZEILENGRUPPEN_TIEFE` begrenzt die Suche. Sie ist eine **Schranke
/// gegen das Pathologische**, nicht die erwartete Form: HTML kennt genau
/// eine Gruppenebene, und die gemessene Seite hat genau eine.
const ZEILENGRUPPEN_TIEFE: usize = 3;

/// Die Zeilen unterhalb von `tabelle_id`, in Dokumentreihenfolge.
fn zeilen_von_tabelle(ctx: &Ctx<'_>, tabelle_id: NodeId) -> Result<Vec<NodeId>, LayoutError> {
    let mut raus: Vec<NodeId> = Vec::new();
    sammle_zeilen(ctx, tabelle_id, ZEILENGRUPPEN_TIEFE, &mut raus)?;
    Ok(raus)
}

/// Hat dieser Knoten **Zellen** als Kinder — ist er also eine `<tr>` und
/// keine Zeilengruppe?
///
/// added 04.10.2026, Rolle LAYOUT (LAYOUT_2), nach LAYOUT_1.
///
/// `Anzeige::TabelleZeile` traegt **beide** Rollen (`<tr>` und `<tbody>`,
/// `narsil-css/src/kaskade.rs:3647`), also muss die Unterscheidung aus der
/// **Struktur** kommen. Die Frage steht an zwei Stellen — hier und in
/// `box.rs::tabellen_hoehe_gerade_y`, wo sie die **Hoehe** der Zeile
/// entscheidet — und sie hat an beiden Stellen **dieselbe** Antwort, sonst
/// waere die Zeile in der einen Phase 19 px hoch und in der anderen 57.
///
/// added 05.10.2026, SERVER_14 (die Fassung ohne Plan, direkt in
/// `sammle_zeilen`).
pub(crate) fn ist_tabellen_zeile(ctx: &Ctx<'_>, id: NodeId) -> bool {
    get_node(ctx, id).is_ok_and(|k| {
        k.children
            .iter()
            .any(|&c| ctx.stil(c).map_or(Anzeige::Block, |s| s.anzeige) == Anzeige::TabelleZelle)
    })
}

fn sammle_zeilen(
    ctx: &Ctx<'_>,
    eltern_id: NodeId,
    tiefe: usize,
    raus: &mut Vec<NodeId>,
) -> Result<(), LayoutError> {
    if tiefe == 0 {
        return Ok(());
    }
    let eltern = get_node(ctx, eltern_id)?;
    for &kind_id in eltern.children.iter() {
        let rolle = ctx.stil(kind_id).map_or(Anzeige::Block, |s| s.anzeige);
        if !matches!(rolle, Anzeige::TabelleZeile | Anzeige::TabelleRand) {
            continue;
        }
        // DIE ENTSCHEIDUNG: Zellen = Zeile, sonst Zeilengruppe.
        if ist_tabellen_zeile(ctx, kind_id) {
            raus.push(kind_id);
        } else {
            sammle_zeilen(ctx, kind_id, tiefe - 1, raus)?;
        }
    }
    Ok(())
}

/// Der Index dieser Zeile **innerhalb der Rechnung**.
///
/// added 05.10.2026, SERVER_14: `lege_tabelle` zaehlt nur die Kinder mit
/// der Rolle `TabelleZeile` — ein `<caption>` dazwischen wuerde sonst
/// alles um eins verschieben. Der Index ist deshalb die Zahl der
/// Zeilenkinder **vor** diesem Knoten.
fn zeilen_index_bei_zeile(ctx: &Ctx<'_>, zeilen_node: &narsil_dom::Node) -> usize {
    let Some(tabelle_id) = zeilen_node.parent else {
        return 0;
    };
    let Ok(tabelle) = get_node(ctx, tabelle_id) else {
        return 0;
    };
    tabelle
        .children
        .iter()
        .take_while(|&&k| k != zeilen_node.id)
        .filter(|&&k| {
            matches!(
                ctx.stil(k).map_or(Anzeige::Block, |s| s.anzeige),
                Anzeige::TabelleZeile | Anzeige::TabelleRand
            )
        })
        .count()
}

/// Das Rechteck **einer Zelle** — ueber **zwei** Ebenen nachgeschlagen.
///
/// added 05.10.2026, Rolle SERVER (SERVER_15). Auftrag der Leitung:
/// „Zelle -> parent (Zeile) -> parent (Tabelle) in `ctx.tabellen`
/// nachschlagen“.
///
/// GEMESSENE URSACHE, warum zwei Schritte noetig sind (SERVER_14): in
/// `kind_platzieren` ist `eltern_stil` der Stil des Knotens **selbst**.
/// Die Zeile sieht `Anzeige::TabelleZeile` und erreicht ihre Tabelle ueber
/// `node.parent`; die Zelle sieht `Anzeige::TabelleZelle` und liegt **ein
/// level tiefer**. Mit einem Schritt bekam sie ihre Zeile statt ihrer
/// Tabelle — und damit nichts.
///
/// **WAS DARAUS FOLGT:** die Rechnung fuehrt die Karten nach **Knotennummer**
/// (`Tabelle::zeilen_rechtecke`, `Tabelle::zellen_rechtecke`), nicht nach
/// (Zeilenindex, Spaltenindex). Wer erst hier verrechnen muesste, haette
/// die Zuordnung an zwei Stellen — und genau dort waere sie mit
/// `colspan` auseinandergegangen.
fn zellen_rect_der_zelle(ctx: &Ctx<'_>, zellen_node: &narsil_dom::Node) -> Option<Rect> {
    let zeile_id = zellen_node.parent?;
    let zeile = get_node(ctx, zeile_id).ok()?;
    let tabelle_id = zeile.parent?;
    let t = ctx.tabellen.borrow();
    let tabelle = t.get(&tabelle_id)?;
    tabelle
        .zellen_rechtecke
        .get(&(zellen_node.id.index() as u32))
        .copied()
}

fn id_von(_ctx: &Ctx<'_>, node: &narsil_dom::Node) -> NodeId {
    node.id
}

/// Die Zellen einer Zeile, mit der **Spalte**, auf der jede beginnt.
///
/// added 05.10.2026, SERVER_14: `lege_tabelle` liefert den **Spaltenindex**
/// eines Platzes, nicht den Listenindex der Zelle. Mit `colspan` stimmen die
/// beiden nicht ueberein — deshalb schreibt diese Struktur die laufende
/// Spalte beim Einsammeln mit, statt sie spaeter zu erraten.
///
/// GEMESSENE KORREKTUR: mein erster Versatz hat die Spalten im Aufrufer
/// mitgezaehlt und war an zwei Stellen dieselbe Zahl zweimal im Programm.
/// Eine Struktur mit der Spalte drin kann das nicht.
///
/// `(NodeId, spalte_beginnt)`
type ZellRegister = Vec<Vec<(NodeId, u32)>>;

/// Die Zelle, die auf `spalte` **beginnt** — oder die naechste, wenn
/// `spalte` mitten in einem `colspan` liegt.
fn zelle_in_register(reg: &ZellRegister, zeile: usize, spalte: u32) -> Option<NodeId> {
    let zellen = reg.get(zeile)?;
    // 1. Exakter Anfang.
    if let Some((id, s)) = zellen.iter().find(|(_, s)| *s == spalte) {
        return Some(*id);
    }
    // 2. `spalte` liegt in einem `colspan`: dann gehoert die Zelle dazu.
    zellen
        .iter()
        .find(|(_, s)| *s < spalte && spalte < *s + 1)
        .map(|(id, _)| *id)
        .or_else(|| zellen.first().map(|(id, _)| *id))
}

/// Die Rechtecke der Zeile, aus dem gelegten Raster.
///
/// added 05.10.2026, SERVER_14: die Zeile schlaegt ihre Zellen nach, statt
/// neu zu rechnen. Das ist der Unterschied zwischen einer Tabelle und
/// drei getrennten Zeilen, und es ist der Grund fuer das Feld
/// `Ctx::tabellen`.
fn zeilen_rects_aus(
    t: &crate::tabelle::Tabelle,
    zeilen_node: &narsil_dom::Node,
    eltern_inhalt: &Rect,
) -> HashMap<NodeId, Rect> {
    let mut raus = HashMap::new();
    for (i, &zellen_id) in zeilen_node.children.iter().enumerate() {
        // `p.zeile` ist der Index der Zeile **in der Tabelle**; `i` laeuft
        // ueber die Kinder der Zeile. Sie sind gleich, solange die Zeile
        // keine Kinder hat, die keine Zellen sind — und das ist genau der
        // Fall, den `lege_tabelle` auch gesehen hat.
        let Some(p) = t.zellen.iter().find(|p| p.zeile as usize == i) else {
            continue;
        };
        raus.insert(
            zellen_id,
            Rect {
                x: eltern_inhalt.x + p.x,
                y: eltern_inhalt.y + p.y,
                width: p.breite,
                height: p.hoehe,
            },
        );
    }
    raus
}

fn verteile_flex_alle(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    content: &Rect,
    stil: &narsil_layout::LayoutStil,
    sizes: &Sizes,
) -> Result<Vec<Platzierung>, LayoutError> {
    let richtung = stil.flex_richtung;
    let n = node.children.len().min(crate::plan::MAX_KINDER);
    let mut kinder: Vec<FlexKind> = Vec::with_capacity(n);
    let mut indices = Vec::with_capacity(n);
    // Release 09.10.2026: die Hauptachsen-Raender je Kind (vorn, hinten) —
    // fuer die Spiegelung bei `row-reverse`/`column-reverse`.
    let mut raender: Vec<(f32, f32)> = Vec::with_capacity(n);
    for (index, child_id) in kinder_in_ordnung(ctx, node, n) {
        let cst = ctx.stil_oder_standard(child_id);
        if !cst.wird_gezeichnet()
            || matches!(cst.position, PositionArt::Absolut | PositionArt::Fest)
        {
            continue;
        }
        let cs = sizes
            .get(child_id)
            .ok_or(LayoutError::UnknownNode(child_id))?;
        indices.push(index);
        //
        // L4 (04.10.2026 02:4x): DIE AUTO-RAENDER, richtungsaufgeloest.
        //
        // GEMESSENER FEHLER (`CSS_14.txt`, f8/f9): `margin:auto` an einem
        // Flex-Kind zentrierte nicht. `FlexKind` hatte kein Randfeld, also
        // konnte der Rand nicht ankommen — in `narsil-css/src/flex.rs` sind
        // jetzt vier Felder da, und hier werden sie gefuellt.
        //
        // `row` -> Hauptachse ist waagerecht (links/rechts), `column` ist
        // senkrecht (oben/unten); die Querachse ist jeweils die andere.
        let rand = &get_node(ctx, child_id)?.style.margin;
        let rand_auto = |x: narsil_core::length::Length| x == narsil_core::length::Length::Auto;
        let (haupt_vorn, haupt_hinten, quer_vorn, quer_hinten) = match richtung {
            narsil_layout::plan::FlexRichtung::Reihe => (
                cst.marge_links_auto || rand_auto(rand.left),
                cst.marge_rechts_auto || rand_auto(rand.right),
                // §10.6.3: ein vertikales `margin:auto` zentriert im
                // Blockfluss NICHT; im Flex nimmt es den freien Platz.
                // Release 11.10.2026: der Plan traegt es (`marge_oben_auto`),
                // denn im Stil steht es schon als 0.
                cst.marge_oben_auto || rand_auto(rand.top),
                cst.marge_unten_auto || rand_auto(rand.bottom),
            ),
            _ => (
                // Release 11.10.2026: siehe oben (`marge_oben_auto`).
                cst.marge_oben_auto || rand_auto(rand.top),
                cst.marge_unten_auto || rand_auto(rand.bottom),
                cst.marge_links_auto || rand_auto(rand.left),
                cst.marge_rechts_auto || rand_auto(rand.right),
            ),
        };
        // Release 08.10.2026: die automatische Mindestgroesse (CSS Flexbox 1
        // §4.5) — in einer Reihe die min-content-Breite eines Kindes mit
        // `width: auto`, das nicht beschneidet (ein Rollkasten darf auf 0).
        let min_haupt = if richtung == narsil_layout::plan::FlexRichtung::Reihe
            && get_node(ctx, child_id)?.style.size.width == narsil_core::length::Length::Auto
            && !cst.ueberlauf.beschneidet()
        {
            sauber_pos(sizes.min_inhalt_breite(ctx, child_id, 0))
        } else {
            0.0
        };
        // Release 08.10.2026: Zahl-Raender und `min-*`/`max-*` des Kindes
        // (Rahmenmasse) — die Verteilung rechnet mit den aeusseren Groessen
        // und klemmt an die Grenzen (§9.7). Die Raender werden spaeter in
        // `build_tree` an die Position gelegt; hier gehen sie nur in den
        // Platzbedarf ein.
        let kind_stil = &get_node(ctx, child_id)?.style;
        let rand_px = resolve_spacing(kind_stil.margin, content.width, child_id, "margin")?;
        let mut fuell = resolve_spacing(kind_stil.padding, content.width, child_id, "padding")?;
        let kante = crate::r#box::rahmen_edges(&kind_stil.rahmen);
        fuell.left += kante.left;
        fuell.right += kante.right;
        fuell.top += kante.top;
        fuell.bottom += kante.bottom;
        let (g_min_b, g_max_b, g_min_h, g_max_h) = crate::r#box::grenzen_rahmen(
            ctx,
            child_id,
            content.size(),
            (fuell.left + fuell.right, fuell.top + fuell.bottom),
        );
        let reihe = richtung == narsil_layout::plan::FlexRichtung::Reihe;
        raender.push(if reihe { (rand_px.left, rand_px.right) } else { (rand_px.top, rand_px.bottom) });
        let (rand_haupt, rand_quer) = if reihe {
            (rand_px.left + rand_px.right, rand_px.top + rand_px.bottom)
        } else {
            (rand_px.top + rand_px.bottom, rand_px.left + rand_px.right)
        };
        let (min_angabe, max_haupt, quer_min, quer_max) = if reihe {
            (g_min_b, g_max_b, g_min_h, g_max_h)
        } else {
            (g_min_h, g_max_h, g_min_b, g_max_b)
        };
        // Eine angegebene Mindestgroesse ersetzt die automatische (§4.5 gilt
        // nur fuer `min-*: auto`).
        let min_haupt = min_angabe.unwrap_or(min_haupt);
        kinder.push(FlexKind {
            max: max_haupt,
            rand_haupt,
            rand_quer,
            // Release 08.10.2026: die gemessene Querausdehnung (Hoehe in
            // einer Reihe) fuer die Zeilenhoehe mehrzeiliger Container.
            quer_inhalt: if reihe { cs.height } else { cs.width },
            min: min_haupt,
            haupt_vorn_auto: haupt_vorn,
            haupt_hinten_auto: haupt_hinten,
            quer_vorn_auto: quer_vorn,
            quer_hinten_auto: quer_hinten,
            basis: flex_basis(
                cst.flex_basis,
                {
                    // P9 (05.10.2026): EINE FUELLENDE BREITE IST `auto`.
                    //
                    // GEMESSENER FEHLER: `narsil-page` gibt einer
                    // `text-align:center`-ZEILENBOX eine **explizite**
                    // Breite (`Px(884)`) — die Breite der Verfuegbaren, fuer
                    // die Zentrierung. `<li>` ist bei uns ein Absatz
                    // (`ist_absatz("li")`), seine Zeilenboxen sind damit die
                    // Kinder der Flex-`<ul>`. Der alte Test `width == Auto`
                    // sah darin eine feste Breite und setzte die
                    // **Flex-Basis** auf `cs.width` (900) — acht Kinder zu
                    // 900 px brechen in einer 884-px-Zeile um und stapeln.
                    //
                    // DIE REGEL: `auto` ODER eine feste Breite, die genau
                    // die des enthaltenden Inhalts ist, bedeutet "fuellen"
                    // und bekommt die **Inhaltsbreite** (`max_content_breite`
                    // liefert GEMESSEN 120..210 statt 900). Eine echte
                    // andere Angabe (`width:100px`, `Percent(100)`) bleibt
                    // unangetastet.
                    let kind_knoten = get_node(ctx, child_id)?;
                    let w = kind_knoten.style.size.width;
                    // Release 08.10.2026: eine Zeilenbox des Seitenbauers
                    // traegt die GESCHAETZTE Absatzbreite (oft die der
                    // ganzen Seite, nicht die dieses Containers) — sie
                    // fuellt immer. GEMESSEN: rust-lang.org, zentrierte
                    // `<li>` mit 1 168 px Basis in einer 295-px-`<ul>`.
                    let fuellt = matches!(
                        w,
                        narsil_core::Length::Px(v) if (v - content.width).abs() < 1.0
                    ) || (matches!(w, narsil_core::Length::Px(_))
                        && crate::r#box::ist_zeilenbox(ctx, child_id, kind_knoten));
                    if w == narsil_core::Length::Auto || fuellt {
                        sizes.max_content_breite(ctx, child_id, 0)
                    } else {
                        cs.width
                    }
                },
                cs.height,
                richtung,
            ),
            wachstum: sauber_pos(cst.flex_wachstum),
            schrumpf: sauber_pos(cst.flex_schrumpf),
            // L3 (02.10.2026 06:2x): HIER STAND BISHER `quer: None`, und dieser
            // Wert ist die Ursache eines GEMESSENEN Fehlers. Er bleibt
            // `None`, weil die Information, die man braucht, im Plan
            // nicht steht.
            //
            // GEMESSENER BEFEUND (`examples/l3_flex.rs`): vier Kinder zu
            // `width:150px; height:20px` in einem 400-px-Kasten. Der
            // Zeilenversatz betrug **88 px statt 20**. `lege_flex_zeilen`
            // bestimmt die Zeilenhoehe aus genau diesem Feld (`flex.rs`:
            // `if let Some(m) = k.quer { hoch = hoch.max(...) }`), und mit
            // `None` fuer jedes Kind blieb ein Notfallwert.
            //
            // GEMESSENE ENTSCHEIDUNG, warum `Some` hier trotzdem nicht
            // steht — und das ist der eigentliche Befund: Es braucht
            // die ANGABE des Kindes, nicht die Messung.
            //
            //   * Aus `Sizes` (`cs.width`/`cs.height`, die Messung):
            //     GEMESSEN falsch. `tests/flex_rand_zweimal.rs::
            //     ein_rand_groesser_als_die_hoehe_loescht_die_hoehe_nicht`
            //     wurde 19.2 px statt 20 px ROT — die Messung ist auch
            //     dann eine Zahl, wenn das Kind `auto` sagt, und
            //     `quermass_zeile` benutzt `quer`, um `align-items:
            //     stretch` auszuhebeln.
            //   * Aus dem Stil des Kindes: geht nicht. `LayoutStil` hat
            //     **kein** `breite`-/`hoehe`-Feld; die 21 Felder sind
            //     gezaehlt.
            //
            // Die Zeilenhoehe braucht also eine Angabe im Plan, die es
            // nicht gibt. Das ist dieselbe Luecke wie bei
            // `flex-basis: auto/content`: der Typ kann die drei
            // Bedeutungen nicht tragen. Wer das fixt, fixt beides.
            // L4 (03.10.2026 13:5x): das Quer-Mass kommt aus der
            // ANGABE des Kindes, nicht aus der Messung (§6.4/§9.2 —
            // die hypothetische Quermass-Groesse).
            //
            // GEMESSENER BEFUND, dass es noetig ist: Block H des
            // Beispiels `examples/l3_flex.rs` — `align-items:
            // flex-start`, `center` und `flex-end` ergaben alle `y 48`.
            // Mit `quer: None` ist `wunsch` in `quermass_zeile` gleich
            // der Zeilenhoehe, und damit liefern alle vier Werte denselben
            // Versatz 0.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die ANGABE und nicht `Sizes`:
            // die Messphase gibt einem `height:auto` in einem gestreckten
            // Kasten die Zeilenhoehe (hier 19,2 px statt 20 px, L3 06:2x)
            // — `quermass_zeile` wuerde dann einen Versatz aus einer Zahl
            // bauen, die selbst aus dem Versatz entstanden ist.
            quer: {
                // Die Kaskade kennt nur den Viewport, nicht den Flex-Container.
                // Prozentgroessen daher hier aufloesen; intrinsische Bildmasse
                // und nicht gestreckte auto-Kinder behalten ihre gemessene Hoehe.
                let ns = &get_node(ctx, child_id)?.style.size;
                let (angabe, anteil, laenge, bezug, gemessen) = match richtung {
                    narsil_layout::FlexRichtung::Reihe => (cst.hoehe_angabe, cst.hoehe_anteil, ns.height, content.height, cs.height),
                    narsil_layout::FlexRichtung::Spalte => (cst.breite_angabe, cst.breite_anteil, ns.width, content.width, cs.width),
                };
                let aus = cst.selbst_vertikal.map(ausrichtung_raster)
                    .unwrap_or_else(|| css_ausrichtung(stil.flex_ausrichtung));
                // Release 08.10.2026: die Angabe gilt geklemmt an `min-*`/
                // `max-*` der Querachse. GEMESSEN vorher: rust-lang.org,
                // `<header class="w-100 mw9-l center">` in einer Flex-Spalte
                // war 1 888 statt 1 152 px breit und stand links statt mittig.
                let klemmen = |v: f32| {
                    let v = quer_max.map_or(v, |m| v.min(m));
                    sauber_pos(quer_min.map_or(v, |m| v.max(m)))
                };
                // Release 09.10.2026: eine GESETZTE Querlaenge ist bei
                // `box-sizing: content-box` die Inhaltsgroesse — Polster (und
                // der Rahmen, der hier als Polster steht) kommen dazu.
                // GEMESSEN vorher: `height:40px; padding:10px` im Flex-Kasten
                // 40 px hoch, im Blockfluss 60 px (Firefox/Chrome: 60); die
                // untere Rahmenlinie von Formularfeldern in Flex-Zeilen fehlte.
                let polster_quer = if ctx.stil(child_id).is_some_and(|p| p.border_box) {
                    0.0
                } else {
                    let pad = resolve_spacing(get_node(ctx, child_id)?.style.padding, content.width, child_id, "padding")?;
                    match richtung {
                        narsil_layout::FlexRichtung::Reihe => pad.top + pad.bottom,
                        narsil_layout::FlexRichtung::Spalte => pad.left + pad.right,
                    }
                };
                anteil.map(|p| sauber_pos(p * bezug / 100.0 + polster_quer)).or(angabe.map(|a| sauber_pos(a + polster_quer))).or_else(|| match laenge {
                    narsil_core::length::Length::Px(v) => Some(sauber_pos(v + polster_quer)),
                    narsil_core::length::Length::Percent(p) => Some(sauber_pos(p * bezug / 100.0 + polster_quer)),
                    // Release 08.10.2026: in einer SPALTE ist ein nicht
                    // gestrecktes Kind ohne Breite fit-content (CSS Flexbox
                    // §9.4 Schritt 11 / Sizing 3): `min(max-content,
                    // verfuegbar)` — die Messphase gab ihm die volle
                    // Blockbreite. GEMESSEN vorher: apple.com, die Buttons
                    // in `.tile-content { flex-direction: column;
                    // align-items: center }` waren 900 statt ~150 px breit
                    // und ragten aus dem 320-px-Container.
                    _ if aus != narsil_css::FlexAusrichtung::Strecke
                        && richtung == narsil_layout::FlexRichtung::Spalte
                        && node_ist_block(ctx, child_id) =>
                    {
                        Some(sauber_pos(sizes.max_content_breite(ctx, child_id, 0)).min(gemessen).min(bezug.max(0.0)))
                    }
                    _ if aus != narsil_css::FlexAusrichtung::Strecke => Some(gemessen),
                    _ => None,
                }).map(klemmen)
            },
        });
    }
    //
    // P12 (05.10.2026, 17:03): die Basis und das Ergebnis der
    // Verteilung NEBENEINANDER drucken.
    //
    // GEMESSENER GRUND: auf github.com gibt ein Flex-Container seinen
    // beiden Kindern 0 px und 900 px, und die Kinder des einen sind
    // gegen 876 px gemessen worden (900 minus 24). Die Basis entsteht
    // aus der Messphase (`cs.width`), das Ergebnis aus
    // `lege_flex_zeilen`. Solange ich nur das Ergebnis kenne, weiss
    // ich nicht, welche der beiden Zahlen falsch ist — und das ist
    // die ganze Frage.
    //
    // Ohne die Variable kostet das einen `var_os` je Container, nicht
    // je Kind (die Pruefung steht hier, nicht in der Schleife).
    let messen = flex_messen_an();
    if messen {
        eprintln!(
            "FLEX #{} {:?} inhalt={:?} luecke={} kinder={}",
            node.id.0,
            richtung,
            content,
            stil.luecke,
            kinder.len()
        );
        for (i, k) in kinder.iter().enumerate() {
            eprintln!(
                "  KIND {i}: basis={:.2} wachstum={:.2} schrumpf={:.2}",
                k.basis, k.wachstum, k.schrumpf
            );
        }
    }
    // L4 (03.10.2026 14:2x): §9.2 — die Ausrichtung JE KIND
    // (`align-self`).  `None` = `auto` (§6.3) = der Wert des
    // Containers, und genau das rechnet `lege_flex_zeilen`.
    //
    // GEMESSENER BEFUND, dass es fehlte: `examples/l3_flex.rs`, H5 —
    // `align-self: center` am Kind ergab `y 48`, genau wie
    // `align-items: flex-start`.
    let selbst: Vec<KindQuer> = indices
        .iter()
        .map(|&index| {
            let child_id = node.children[index];
            let cst = ctx.stil_oder_standard(child_id);
            KindQuer {
                selbst: cst.selbst_vertikal.map(ausrichtung_raster),
                aufstieg: aufstieg_von(ctx, child_id, &cst),
            }
        })
        .collect();
    let raus = verteile_flex_mit_selbst(
        *content,
        richtung,
        css_buendig(stil.flex_buendig),
        css_ausrichtung(stil.flex_ausrichtung),
        stil.luecke,
        // L3 (02.10.2026 04:5x): der Querachsen-Lueckenwert kommt
        // jetzt an. GEMESSENE ENTSCHEIDUNG, warum gerade hier und nicht
        // in `verteile_flex_alle`: dort steht die Messung, die die Basis
        // baut; `verteile_flex` rechnet nur die Verteilung. Die
        // Umrechnung der Achsen hat die Kaskade in `layout_plan.rs`
        // gemacht (CSS Flexbox 1 §8.3), weil sie die CSS-Richtung
        // kennt — diese Funktion kennt nur noch `Reihe`/`Spalte`.
        stil.luecke_quer,
        matches!(
            stil.flex_umbruch,
            FlexUmbruch::Umbruch | FlexUmbruch::Umgekehrt
        ),
        &kinder,
        &selbst,
    );
    // Release 09.10.2026: `row-reverse` / `column-reverse` (CSS Flexbox 1
    // §5.1). Die Verteilung legt von vorn; die Umkehrung belegt die
    // Hauptachse von hinten — das ist genau die Spiegelung jedes Platzes
    // (Rand eingeschlossen) an der Mitte des Inhaltskastens. Auch
    // `justify-content` spiegelt sich damit richtig (`flex-start` = rechts).
    // GEMESSEN vorher: spiegel.de, `flex lg:flex-row-reverse` — der
    // Aufmacher stand links statt rechts neben seinem Text.
    let mut raus = raus;
    if stil.flex_umgekehrt {
        let reihe = richtung == narsil_layout::plan::FlexRichtung::Reihe;
        for (p, &(vorn, hinten)) in raus.iter_mut().zip(raender.iter()) {
            if reihe {
                p.x = 2.0 * content.x + content.width - p.x - (p.breite + vorn + hinten);
            } else {
                p.y = 2.0 * content.y + content.height - p.y - (p.hoehe + vorn + hinten);
            }
        }
    }
    if messen {
        for (i, p) in raus.iter().enumerate() {
            eprintln!("  ERGEBNIS {i}: x={:.2} breite={:.2}", p.x, p.breite);
        }
    }
    let mut alle = vec![Platzierung::WEG; n];
    for (index, platz) in indices.into_iter().zip(raus) {
        alle[index] = platz;
    }
    Ok(alle)
}

/// Release 08.10.2026: die Breite eines BLOCKkindes im Normalfluss, wenn
/// die Messphase sie gegen einen breiteren Platz gerechnet hat.
///
/// Ein Schrumpfkasten (`inline-block`, `float`, `absolute` mit `width:
/// auto`) misst seine Kinder gegen den verfuegbaren Platz und schrumpft
/// DANACH auf ihre bevorzugte Breite. Seine Blockkinder behielten die
/// gemessene Breite — GEMESSEN auf rust-lang.org: `<header
/// style="display:inline-block">` 188 px breit, das `<h2>` darin 1 128 px
/// und der Balken `.highlight{width:120%}` 1 353 px statt 226 px.
/// CSS 2.1 §10.3.3: ein Block mit `width:auto` ist so breit wie der
/// Inhalt seines enthaltenden Blocks (minus Raender), ein Prozent rechnet
/// gegen ihn. Die Hoehe bleibt die gemessene: die Schrumpfbreite ist
/// mindestens die bevorzugte Breite der Kinder, ihr Umbruch aendert sich
/// also nicht.
/// Release 08.10.2026: ein Container ohne Ersatzmasse (kein Bild, kein
/// Video) — dessen Breite ergibt sich aus dem Inhalt.
fn node_ist_block(ctx: &Ctx<'_>, id: NodeId) -> bool {
    ctx.document.get(id).is_some_and(|n| n.kind == NodeKind::Container && n.ersetzt.is_none())
}

fn block_breite_im_eltern(
    ctx: &Ctx<'_>,
    child_id: NodeId,
    child: &narsil_dom::Node,
    gemessen: &Size,
    content: &Rect,
    cm: &narsil_core::geometry::Edges,
) -> Option<Size> {
    // Release 14.10.2026: a replaced box keeps its measured width (CSS 2.1
    // §10.3.2/§10.3.4: `width: auto` is the natural width, not the containing
    // block). MEASURED before: a `<canvas width=300>` in a 100 px block was
    // 100 x 50 (since the canvas has its layout plan); an `<img>` without a
    // `width` attribute the same.
    if child.kind != NodeKind::Container || child.ersetzt.is_some() {
        return None;
    }
    let st = ctx.plan.and_then(|p| p.hole(child_id))?;
    // Release 08.10.2026: auch Flex-/Raster-Container im Blockfluss.
    // GEMESSEN vorher: spiegel.de, `<h2 class="w-full mr-24"><a class="flex">`
    // — das Flex-Element `h2` schrumpfte auf 765 px, sein `a.flex` blieb
    // bei den 852 px der Messung und ragte ueber den Nachbarn.
    if !matches!(st.anzeige, Anzeige::Block | Anzeige::Flex | Anzeige::Raster)
        || st.schweben != FloatArt::Keins
        || !matches!(st.position, PositionArt::Statisch | PositionArt::Relativ | PositionArt::Klebend)
        || crate::r#box::ist_zeilenbox(ctx, child_id, child)
    {
        return None;
    }
    let platz = (content.width - cm.left - cm.right).max(0.0);
    // Nur wenn die Messung BREITER war als der Platz: im gewoehnlichen
    // Fluss stimmen beide ueberein, und dann bleibt alles, wie es ist.
    if gemessen.width <= platz + 0.5 {
        return None;
    }
    let breite = match child.style.size.width {
        // Release 15.10.2026 (Agent CSS2): the containing block's width,
        // but never below `min-width` (nor above `max-width`) — CSS 2.1
        // §10.4 applies to the auto width as to any other. MEASURED before
        // (coordinator probe `<div style="width:100px"><div
        // style="min-width:260px">`, WPT css-sizing `min-width-max-width-
        // precedence`): the block was 100 px wide, the abspos around it 0.
        narsil_core::length::Length::Auto => {
            let fuellung = {
                let pd = resolve_spacing(child.style.padding, content.width, child_id, "padding").ok()?;
                let k = crate::r#box::rahmen_edges(&child.style.rahmen);
                pd.left + pd.right + k.left + k.right
            };
            let (min_b, max_b, _, _) =
                crate::r#box::grenzen_rahmen(ctx, child_id, content.size(), (fuellung, 0.0));
            let b = max_b.map_or(platz, |m| platz.min(m));
            let b = min_b.map_or(b, |m| b.max(m));
            if b >= gemessen.width - 0.5 {
                return None;
            }
            b
        }
        narsil_core::length::Length::Percent(p) if p.is_finite() => {
            let roh = (content.width * p / 100.0).max(0.0);
            let fuellung = {
                let pd = resolve_spacing(child.style.padding, content.width, child_id, "padding").ok()?;
                let k = crate::r#box::rahmen_edges(&child.style.rahmen);
                pd.left + pd.right + k.left + k.right
            };
            let roh = if st.border_box { roh } else { roh + fuellung };
            let (min_b, max_b, _, _) =
                crate::r#box::grenzen_rahmen(ctx, child_id, content.size(), (fuellung, 0.0));
            let roh = max_b.map_or(roh, |m| roh.min(m));
            min_b.map_or(roh, |m| roh.max(m))
        }
        _ => return None,
    };
    Some(Size::new(breite, gemessen.height))
}

/// Soll die Flex-Basis mitgedruckt werden? Wird einmal gelesen und
/// gemerkt (M1/M4: `var_os` in einer Schleife waere Verschwendung).
fn flex_messen_an() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static GEPRUEFT: AtomicBool = AtomicBool::new(false);
    static AN: AtomicBool = AtomicBool::new(false);
    if !GEPRUEFT.load(Ordering::Relaxed) {
        let wert = std::env::var_os("NARSIL_FLEX_MESSEN")
            .map(|v| v == "1" || v == "ja" || v == "true")
            .unwrap_or(false);
        AN.store(wert, Ordering::Relaxed);
        GEPRUEFT.store(true, Ordering::Relaxed);
    }
    AN.load(Ordering::Relaxed)
}

/// Verteilt die Kinder eines Raster-Containers (L2).
///
/// L4 (02.10.2026 22:5x): `ctx` kommt dazu, weil §8.3 eine Angabe PRO
/// KIND ist (`grid-row: span N` steht am Kind, nicht am Raster). Ohne den
/// Plan gibt es diese Angabe nicht — GEMESSENER Befund: sie kam in
/// `Berechnet::gitter_zeile_start` an und wurde von niemandem benutzt.
/// `narsil_layout::plan::Ausrichtung` → `narsil_css::FlexAusrichtung`.
///
/// GEMESSENER FEHLER MEINER ERSTEN FASSUNG: `narsil_css::typen::…` —
/// das Modul `typen` ist privat (`lib.rs:90` re-exportiert die Typen).
///
/// L4 (03.10.2026 09:5x). GEMESSENE ENTSCHEIDUNG, warum die Uebersetzung
/// HIER steht und nicht in `narsil-layout`: die beiden Crate kennen sich
/// nicht (`narsil-css` baut den Plan, `narsil-layout-engine` legt ihn), und
/// `flow.rs` ist die einzige Stelle, die beide Typen sieht. GEMESSEN: mit
/// einer direkten Zuweisung gab es `mismatched types` in Zeile 1628.
fn ausrichtung_raster(a: narsil_layout::plan::Ausrichtung) -> narsil_css::FlexAusrichtung {
    use narsil_css::FlexAusrichtung as F;
    match a {
        narsil_layout::plan::Ausrichtung::Start => F::Start,
        narsil_layout::plan::Ausrichtung::Mitte => F::Mitte,
        narsil_layout::plan::Ausrichtung::Ende => F::Ende,
        // L4 (13:5x): `Grundlinie` gibt es jetzt im Plan. Der alte
        // Kommentar an dieser Stelle sagte „kennt es nicht" — das war
        // gemessen und stimmte bis 13:5x.
        narsil_layout::plan::Ausrichtung::Grundlinie => F::Grundlinie,
        narsil_layout::plan::Ausrichtung::Strecke => F::Strecke,
    }
}

/// Release 11.10.2026: die Kinder eines Flex- oder Raster-Containers in
/// `order`-Reihenfolge (CSS Display 3 „order-modified document order":
/// aufsteigend nach `order`, gleiche Werte in Dokumentreihenfolge), mit
/// ihrem Index im Dokument. GEMESSEN vorher: `order` wurde berechnet
/// (`getComputedStyle` stimmte), aber nie angewandt — WPT css-flexbox/order
/// und css-grid/grid-items standen in Dokumentreihenfolge.
fn kinder_in_ordnung(ctx: &Ctx<'_>, node: &narsil_dom::Node, n: usize) -> Vec<(usize, NodeId)> {
    let mut folge: Vec<(usize, NodeId)> = node.children.iter().take(n).copied().enumerate().collect();
    let ordnung = |id: NodeId| ctx.plan.map_or(0, |p| p.stil_oder_standard(id).ordnung);
    if folge.iter().any(|&(_, id)| ordnung(id) != 0) {
        folge.sort_by_key(|&(i, id)| (ordnung(id), i));
    }
    folge
}

pub(crate) fn verteile_raster_alle(
    ctx: &Ctx,
    node: &narsil_dom::Node,
    content: &Rect,
    stil: &narsil_layout::LayoutStil,
    sizes: &Sizes,
) -> Result<Vec<Platzierung>, LayoutError> {
    // Release 11.10.2026: der neue Rasteralgorithmus (`gitter.rs`); Masonry
    // bleibt bei der alten Rechnung.
    if let Some(gs) = stil.gitter.as_deref().filter(|_| stil.lanes == 0) {
        return crate::gitter::verteilen(ctx, node, content, stil, gs, sizes);
    }
    let n = node.children.len().min(crate::plan::MAX_KINDER);
    let mut abspos_kinder: Vec<NodeId> = Vec::new();
    // L4 (02.10.2026 22:5x): statt nur der Hoehe kommt das GANZE
    // `RasterKind` — mit den Spannweiten aus §8.3, die am Kind stehen.
    let mut arten: Vec<RasterKind> = Vec::with_capacity(n);
    let mut indices = Vec::with_capacity(n);
    for (index, child_id) in kinder_in_ordnung(ctx, node, n) {
        let kind_stil = ctx.stil_oder_standard(child_id);
        if kind_stil.wird_gezeichnet() && kind_stil.position == PositionArt::Absolut {
            abspos_kinder.push(child_id);
        }
        if !kind_stil.wird_gezeichnet()
            || matches!(kind_stil.position, PositionArt::Absolut | PositionArt::Fest)
        {
            continue;
        }
        let cs = sizes
            .get(child_id)
            .ok_or(LayoutError::UnknownNode(child_id))?;
        indices.push(index);
        let breite_angabe = get_node(ctx, child_id)?.style.size.width;
        let prozent_breite = matches!(breite_angabe, narsil_core::length::Length::Percent(_));
        let breite_wie_auto = prozent_breite || breite_angabe == narsil_core::length::Length::Auto;
        let min_angabe = ctx
            .plan
            .and_then(|p| p.grenzen(child_id).min_breite)
            .filter(|v| v.is_finite() && *v > 0.0);
        arten.push(RasterKind {
            hoch: sauber_pos(cs.height),
            // L4 (04.10.2026 05:2x): §6.6 — `stretch` nur bei `auto`.
            // GEMESSENER FEHLER (`CSS_22.txt` Fall 5): `height:86px` mit
            // `grid-row: span 2` kam auf 178 statt 86, weil
            // `richte_kind` immer streckte und nicht wusste, dass hier
            // eine Groesse GESETZT ist.
            hoch_auto: get_node(ctx, child_id)?.style.size.height
                == narsil_core::length::Length::Auto,
            breit_auto: get_node(ctx, child_id)?.style.size.width
                == narsil_core::length::Length::Auto,
            // L4 (03.10.2026 05:4x): die INHALTSBREITE kommt an, damit
            // die `auto`-Spur (§7.2) ihre Breite kennt und §11.8 den Rest
            // gleichmaessig teilen kann. Dieselbe Quelle wie `hoch` — und
            // dieselbe Form: eine Bruecke, die ein Feld kopiert.
            //
            // Release 08.10.2026: ein Kind mit `width: auto` meldet seinen
            // INHALT (max-content), nicht die gemessene Fuellbreite.
            // GEMESSEN vorher: das Logo von developer.mozilla.org in einer
            // `min-content`-Spur meldete 1 414 px (die ganze Zeile) und
            // stand mitten in der Kopfleiste.
            //
            // Release 09.10.2026: CSS Grid 1 §11.5 — eine Breite, die vom
            // Container abhaengt (Prozent), zaehlt fuer die Spurgroesse wie
            // `auto`: Beitrag = Inhalt (max-content), Mindestbeitrag =
            // `min-width`, falls gesetzt, sonst min-content. GEMESSEN vorher:
            // t-online.de, `grid-template-areas: "hero companion"` ohne
            // Spaltenvorlage; der Aufmacher (`width:100%; min-width:20em`)
            // meldete 997 px (die ganze Zeile), die Seitenleiste (348 px)
            // rutschte aus dem Bild. Chrome: 647 | 348.
            breit: if breite_wie_auto {
                sauber_pos(sizes.max_content_breite(ctx, child_id, 0)).max(min_angabe.unwrap_or(0.0))
            } else {
                sauber_pos(cs.width)
            },
            breit_prozent: match breite_angabe {
                narsil_core::length::Length::Percent(pz) if pz.is_finite() => {
                    Some((pz, (cs.width - content.width * pz / 100.0).max(0.0)))
                }
                _ => None,
            },
            breit_min: breite_wie_auto.then(|| match (prozent_breite, min_angabe) {
                (true, Some(m)) => sauber_pos(m),
                _ => sauber_pos(sizes.min_inhalt_breite(ctx, child_id, 0)).max(min_angabe.unwrap_or(0.0)),
            }),
            // L4 (03.10.2026 09:5x): `align-self`/`justify-self` kommen an
            // (§6.6). GEMESSENER BEFUND, dass es fehlt: Release, Block AA —
            // alle fuenf Werte ergaben dieselbe Zelle.
            // L4 (03.10.2026 11:5x): §6.3 — `auto` bedeutet „den Wert des
            // CONTAINERS benutzen".  Die Aufloesung steht in
            // `raster_vorgabe` (unten) und ist dort gemessen.
            selbst_vertikal: raster_vorgabe(&kind_stil, &stil).0,
            selbst_waagerecht: raster_vorgabe(&kind_stil, &stil).1,
            spann_zeilen: kind_stil.spann_zeilen,
            spann_spalten: kind_stil.spann_spalten,
            // L4 (03.10.2026 01:6x): die festen LINIEN (§8.5), roh.
            //
            // GEMESSENE ENTSCHEIDUNG, warum sie durchgereicht und nicht
            // hier aufgeloest werden: eine negative Linie (`-1`) braucht
            // die Zeilenzahl der VORLAGE, und die kennt erst
            // `lege_raster_bei_autom`. GEMESSEN: der Versuch, sie hier
            // aufzuloesen, lieferte Zeile 1 statt Zeile 4.
            zeile_start: kind_stil.zeile_start,
            zeile_ende: kind_stil.zeile_ende,
            spalte_start: kind_stil.spalte_start,
            spalte_ende: kind_stil.spalte_ende,
            // L4 (03.10.2026 02:10x): der Gebietsname (§8.4). Die Bruecke
            // von `narsil_layout::plan::Gebiet` nach `narsil_css::Gebiet`
            // ist ein Feld-Kopieren — dieselbe Form wie `bridge_spalte`.
            gebiet: narsil_css::Gebiet(kind_stil.gebiet.0),
            sicher: (false, false),
        });
        // Release 11.10.2026: `margin: auto` am Rasterelement (CSS Grid 1
        // §10.2) nimmt den freien Platz der Zelle und geht `align-self`/
        // `justify-self` vor (auch vor `stretch`): beide Seiten automatisch
        // = mittig, nur vorn = ans Ende, nur hinten = an den Anfang.
        // GEMESSEN vorher (WPT `grid-*-axis-alignment-auto-margins-*`): die
        // Elemente standen oben links in ihrer Zelle.
        let rand = &get_node(ctx, child_id)?.style.margin;
        let auto = |x: narsil_core::length::Length| x == narsil_core::length::Length::Auto;
        let (o, u) = (auto(rand.top) || kind_stil.marge_oben_auto, auto(rand.bottom) || kind_stil.marge_unten_auto);
        let (l, r) = (auto(rand.left) || kind_stil.marge_links_auto, auto(rand.right) || kind_stil.marge_rechts_auto);
        if let Some(k) = arten.last_mut() {
            k.sicher = (l || r, o || u);
            if o || u {
                k.selbst_vertikal = match (o, u) {
                    (true, true) => FlexAusrichtung::Mitte,
                    (true, false) => FlexAusrichtung::Ende,
                    _ => FlexAusrichtung::Start,
                };
            }
            if l || r {
                k.selbst_waagerecht = match (l, r) {
                    (true, true) => FlexAusrichtung::Mitte,
                    (true, false) => FlexAusrichtung::Ende,
                    _ => FlexAusrichtung::Start,
                };
            }
        }
    }
    // L4 (02.10.2026 08:1x): der Querachsenwert kommt an.
    // GEMESSENE ENTSCHEIDUNG, warum die Zuordnung NICHT hier passiert:
    // Ein Flex-Container dreht seine Achsen mit `flex-direction`, ein
    // Grid nicht — `column-gap` ist immer waagerecht. Diese Zuordnung
    // gehoert also neben die CSS-Richtung, und die kennt nur
    // `layout_plan.rs` in `narsil-css`.
    // L4 (02.10.2026 19:4x): `stil.zeilen` kommt an (CSS Grid 1 §8.3).
    //
    // GEMESSENER BEFUND, was ohne das passiert: `grid-template-rows`
    // kam bis hierher vorbei (`LayoutStil` hatte das Feld ab 19:4x, der
    // Weg reichte es nicht weiter), und die Zeilenhoehe war das groesste
    // Kind. GEMESSEN in `narsil-css/tests/position.rs`:
    // `grid-template-rows: 200px 200px` ergab 20-px-Zeilen bei 20 px
    // Inhalt.
    //
    // L4 (02.10.2026 21:6x): `grid-auto-rows` (§7.5) und
    // `grid-auto-columns` (§7.3) kommen an. GEMESSENER BEFUND, was
    // vorher passierte: beide kamen als String in `gitter_automatik`
    // an — zusammen mit `grid-auto-flow` — und wurden nie gelesen;
    // `grid-auto-rows:100px` liess die implizite Zeile bei ihrem Inhalt
    // (GEMESSEN 20 px).
    //
    // L4 (22:5x): `&arten` statt `&hoehen` — die Kinds tragen ihre
    // Spannweite (§8.3).
    // Release 11.10.2026: Masonry (`display: grid-lanes`, CSS Grid 3)
    // legt die Elemente in Spuren statt in Zellen (`lanes_verteilen`).
    // Release 11.10.2026: der Platz der Zeilen — definit bei `height`,
    // Prozenthoehe oder `min-height` (dann ist die Inhaltsbox hier schon so
    // hoch), sonst 0 (eine Wiederholung, kein `fr`-Rest).
    let hoehe_definit = stil.hoehe_angabe.is_some()
        || stil.hoehe_anteil.is_some()
        || ctx.plan.and_then(|p| p.grenzen(node.id).min_hoehe).is_some_and(|m| m > 0.0)
        || ctx.plan.is_some_and(|p| p.grenzen_prozent(node.id).min_hoehe.is_some_and(|m| m > 0.0));
    let zeilen_platz = Some(if hoehe_definit && content.height.is_finite() { content.height.max(0.0) } else { 0.0 });
    let mut spalten_spuren: Vec<(f32, f32)> = Vec::new();
    let mut zeilen_spuren: Vec<(f32, f32)> = Vec::new();
    // Release 11.10.2026: auch Zeilen-Spuren (`lanes == 2`) gehen durch den
    // Masonry-Weg, seit die Spuren aus dem neuen Rasteralgorithmus kommen.
    let verteilt = if stil.lanes == 1 || (stil.lanes == 2 && stil.gitter.is_some()) {
        lanes_verteilen(ctx, node, sizes, content, stil, &arten)?
    } else {
        let (v, sp, ze) = verteile_raster_mit_spuren(
            *content,
            &stil.spalten,
            &stil.zeilen,
            &stil.auto_spalten,
            &stil.auto_zeilen,
            &stil.gebiete,
            stil.luecke,
            stil.luecke_quer,
            &arten,
            stil.spaltenfluss,
            // L4 (03.10.2026 04:4x): `dense` (§8.5). GEMESSENER BEFUND, was
            // vorher passierte: nur das erste Wort des Werts wurde gelesen
            // (11:3x), also verhielt sich `row dense` wie `row`.
            stil.dicht,
            zeilen_platz,
        );
        spalten_spuren = sp;
        zeilen_spuren = ze;
        v
    };
    // Release 11.10.2026: das Ende der expliziten Zeilen (ohne
    // Wiederholungen, deren Zahl erst die Rechnung kennt) fuer die Hoehe.
    let explizit = stil.zeilen.len();
    if explizit > 0
        && zeilen_spuren.len() >= explizit
        && !stil.zeilen.iter().any(|z| matches!(z, narsil_layout::plan::Spalte::Auto { .. }))
    {
        let (y, h) = zeilen_spuren[explizit - 1];
        let ende = y + h - content.y;
        if ende.is_finite() && ende > 0.0 {
            ctx.raster_hoehe.borrow_mut().insert(node.id, ende);
        }
    }
    // Release 11.10.2026: der Rasterbereich absolut positionierter Kinder
    // eines POSITIONIERTEN Rasters (CSS Grid 1 §9.1): ihre Linien gegen die
    // Spuren, `auto`-Linien an der Innenabstandskante.
    // Release 11.10.2026: Masonry mit den neuen Spuren — die Bereiche aus
    // dem abgelegten Ergebnis (`gitter::lanes_abspos`).
    if let Some(gs) = stil.gitter.as_deref() {
        crate::gitter::lanes_abspos(ctx, node, stil, gs, content)?;
    }
    if stil.gitter.is_none() && stil.position != PositionArt::Statisch && !abspos_kinder.is_empty() {
        let px = |l: narsil_core::length::Length| match l {
            narsil_core::length::Length::Px(v) if v.is_finite() => v,
            _ => 0.0,
        };
        let pad = get_node(ctx, node.id)?.style.padding;
        // Ohne Kinder im Fluss liefert die Rasterrechnung keine Spuren — dann
        // mit einem leeren Platzhalter, damit die Spuren der Vorlage entstehen.
        if arten.is_empty() {
            let platzhalter = [RasterKind { hoch: 0.0, breit: 0.0, ..RasterKind::default() }];
            let (_, sp, ze) = verteile_raster_mit_spuren(
                *content,
                &stil.spalten,
                &stil.zeilen,
                &stil.auto_spalten,
                &stil.auto_zeilen,
                &stil.gebiete,
                stil.luecke,
                stil.luecke_quer,
                &platzhalter,
                stil.spaltenfluss,
                stil.dicht,
                zeilen_platz,
            );
            spalten_spuren = sp;
            zeilen_spuren = ze;
        }
        let innen = Rect::new(
            content.x - px(pad.left),
            content.y - px(pad.top),
            content.width + px(pad.left) + px(pad.right),
            content.height + px(pad.top) + px(pad.bottom),
        );
        for &kind in &abspos_kinder {
            let ks = ctx.stil_oder_standard(kind);
            let (x0, x1) = raster_bereich_achse(ks.spalte_start, ks.spalte_ende, ks.spann_spalten, &spalten_spuren, innen.x, innen.x + innen.width);
            let (y0, y1) = raster_bereich_achse(ks.zeile_start, ks.zeile_ende, ks.spann_zeilen, &zeilen_spuren, innen.y, innen.y + innen.height);
            let (av, ah) = raster_vorgabe(&ks, stil);
            ctx.raster_bereiche
                .borrow_mut()
                .insert(kind, (Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)), av, ah));
        }
    }
    // Release 09.10.2026: `justify-content: center | end` (CSS Grid 1
    // §10.5) — sind die Spuren schmaler als der Container, wandert das
    // ganze Raster. GEMESSEN vorher: `grid-template-columns: repeat(3,
    // 100px); justify-content: center` klebte links (Chrome: mittig).
    let anteil = match stil.flex_buendig {
        narsil_layout::plan::Buendig::Mitte => 0.5,
        narsil_layout::plan::Buendig::Ende => 1.0,
        _ => 0.0,
    };
    let mut verteilt = verteilt;
    // Release 11.10.2026: mit den neuen Spuren verteilt `lanes_spuren` schon.
    if anteil > 0.0 && stil.gitter.is_none() {
        let rechts = verteilt
            .iter()
            .filter(|p| p.breite > 0.0)
            .map(|p| p.x + p.breite)
            .fold(f32::MIN, f32::max);
        let frei = content.x + content.width - rechts;
        if rechts > f32::MIN && frei > 0.5 {
            let dx = sauber_pos(frei * anteil);
            for p in verteilt.iter_mut() {
                p.x += dx;
            }
        }
    }
    let mut alle = vec![Platzierung::WEG; n];
    for (index, platz) in indices.into_iter().zip(verteilt) {
        alle[index] = platz;
    }
    Ok(alle)
}

/// Release 11.10.2026: Masonry (CSS Grid 3 §„Grid Lanes"). Zeilen-Spuren
/// werden GEDREHT gerechnet (die Zeilen werden zu Spalten, die Elemente
/// tauschen Breite und Hoehe), damit beide Achsen denselben Weg nehmen —
/// auch die feste Platzierung (`grid-row: 4 / span 2`), die der
/// Spaltenfluss des Rasters nicht kennt. GEMESSEN vorher: mit dem
/// Spaltenfluss kippten 38 Zeilen-Tests in css-grid/grid-lanes.
fn lanes_verteilen(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    sizes: &Sizes,
    content: &Rect,
    stil: &narsil_layout::LayoutStil,
    arten: &[RasterKind],
) -> Result<Vec<Platzierung>, LayoutError> {
    let zeilen_spuren = stil.lanes == 2;
    let drehen = |r: Rect| Rect { x: r.y, y: r.x, width: r.height, height: r.width };
    let feld = if zeilen_spuren {
        let mut f = drehen(*content);
        if !f.width.is_finite() || f.width < 0.0 {
            f.width = 0.0;
        }
        f
    } else {
        *content
    };
    // Release 11.10.2026: die Elemente in derselben Folge wie `arten` (siehe
    // `verteile_raster_alle`).
    let ids: Vec<NodeId> = kinder_in_ordnung(ctx, node, node.children.len().min(crate::plan::MAX_KINDER))
        .into_iter()
        .map(|(_, id)| id)
        .filter(|&id| {
            let s = ctx.stil_oder_standard(id);
            s.wird_gezeichnet() && !matches!(s.position, PositionArt::Absolut | PositionArt::Fest)
        })
        .collect();
    let kinder: Vec<RasterKind> = if zeilen_spuren {
        arten
            .iter()
            .enumerate()
            .map(|(i, k)| RasterKind {
                // Release 11.10.2026: eine Prozentbreite gilt auf der
                // (waagerechten) Masonry-Achse mit ihrer gemessenen Breite
                // (gegen den Container, an min-/max-width geklemmt; in der
                // Spurrechnung zaehlt sie wie `auto`). GEMESSEN vorher: WPT
                // `grid-lanes/alignment/grid-lanes-align-content-refinalize-
                // row-geometry-001` (`width: 100%`: 0 px).
                hoch: match (k.breit_prozent, ids.get(i).and_then(|id| sizes.get(*id))) {
                    (Some(_), Some(g)) => sauber_pos(g.width),
                    _ => k.breit,
                },
                breit: k.hoch,
                hoch_auto: k.breit_auto,
                breit_auto: k.hoch_auto,
                breit_min: None,
                breit_prozent: None,
                selbst_vertikal: k.selbst_waagerecht,
                selbst_waagerecht: k.selbst_vertikal,
                spann_zeilen: k.spann_spalten,
                spann_spalten: k.spann_zeilen,
                zeile_start: k.spalte_start,
                zeile_ende: k.spalte_ende,
                spalte_start: k.zeile_start,
                spalte_ende: k.zeile_ende,
                gebiet: k.gebiet,
                sicher: (k.sicher.1, k.sicher.0),
            })
            .collect()
    } else {
        arten.to_vec()
    };
    // Die Spur-Achse und die Luecken der (gedrehten) Rechnung: zwischen den
    // Spuren und entlang der Spuren.
    let (spuren_vorlage, auto_spuren, zwischen, entlang) = if zeilen_spuren {
        (&stil.zeilen, &stil.auto_zeilen, stil.luecke_quer, stil.luecke)
    } else {
        (&stil.spalten, &stil.auto_spalten, stil.luecke, stil.luecke_quer)
    };
    let gestreckt: Vec<RasterKind> = kinder
        .iter()
        .map(|k| RasterKind {
            selbst_vertikal: FlexAusrichtung::Strecke,
            selbst_waagerecht: FlexAusrichtung::Strecke,
            hoch_auto: true,
            breit_auto: true,
            ..*k
        })
        .collect();
    // Die Masonry-Achse hat keine Vorlage (§„Grid Lanes": sie ist `none`).
    let (mut zellen, mut spuren, _) = verteile_raster_mit_spuren(
        feld, spuren_vorlage, &[], auto_spuren, &[], &[], zwischen, 0.0, &gestreckt, false, stil.dicht, None,
    );
    let mut normal = verteile_raster_bei_spann(
        feld, spuren_vorlage, &[], auto_spuren, &[], &[], zwischen, 0.0, &kinder, false, stil.dicht,
    );
    // Release 11.10.2026: die Spuren der Rasterachse aus dem neuen
    // Rasteralgorithmus (CSS Grid 3 „Grid Lanes": wie im Raster, jedes
    // automatisch platzierte Element traegt zu jeder Lage bei). GEMESSEN
    // vorher: die Referenzen (`display: grid`) rechnen seitdem mit `auto`-Spuren
    // nach §12, Masonry noch mit der alten Rechnung — 72 Tests kippten.
    let mut kinder = kinder;
    let mut lanes_daten: Option<crate::gitter::Lanes> = None;
    if let Some(gs) = stil.gitter.as_deref() {
        let platz = if zeilen_spuren {
            // Release 11.10.2026: auch mit `min-height` (die Inhaltsbox ist
            // hier schon so hoch) — `align-content` richtet gegen die fertige
            // Hoehe aus. GEMESSEN vorher: WPT
            // `grid-lanes/alignment/grid-lanes-align-content-refinalize-row-geometry-001`.
            let min_hoehe = ctx.plan.is_some_and(|p| {
                p.grenzen(node.id).min_hoehe.is_some_and(|m| m > 0.0) || p.grenzen_prozent(node.id).min_hoehe.is_some_and(|m| m > 0.0)
            });
            if stil.hoehe_angabe.is_some() || stil.hoehe_anteil.is_some() || (min_hoehe && content.height.is_finite() && content.height > 0.0) {
                crate::gitter::Platz::Definit(content.height.max(0.0))
            } else {
                crate::gitter::Platz::MaxInhalt
            }
        } else {
            crate::gitter::Platz::Definit(content.width.max(0.0))
        };
        let anfang = if zeilen_spuren { content.y } else { content.x };
        let l = crate::gitter::lanes_spuren(ctx, node, gs, sizes, zeilen_spuren, platz, anfang, 0)?;
        lanes_daten = Some(l.clone());
        if l.elemente.len() == kinder.len() {
            spuren = l.spuren.clone();
            for (i, &(z0, zg, x, g, fest, spann)) in l.elemente.iter().enumerate() {
                if let (Some(z), Some(p)) = (zellen.get_mut(i), normal.get_mut(i)) {
                    z.x = z0;
                    z.breite = zg;
                    p.x = x;
                    p.breite = g;
                }
                if let Some(k) = kinder.get_mut(i) {
                    k.spalte_start = i32::from(fest);
                    k.spann_spalten = spann.max(1);
                    // Release 11.10.2026: in Zeilen-Spuren ist die Masonry-Achse
                    // waagerecht — eine Breite als Schluesselwort (`width:
                    // min-content`) gilt mit der gemessenen Breite. GEMESSEN
                    // vorher: WPT grid-lanes `row-fill-reverse-align-content-001`
                    // (96 px max-content statt 64 px min-content).
                    if zeilen_spuren
                        && let Some(&(_, id)) = l.kinder.get(i)
                        && ctx.stil(id).is_some_and(|s| s.groessen_art[0] != 0)
                        && let Some(g) = sizes.get(id)
                    {
                        k.hoch = g.width;
                    }
                }
            }
        }
    }
    let kinder = kinder;
    // Hat der Container eine feste Groesse entlang der Spuren?
    let feld_fest = if zeilen_spuren {
        !stil.inline_aussen || stil.breite_angabe.is_some() || stil.breite_anteil.is_some()
    } else {
        stil.hoehe_angabe.is_some() || stil.hoehe_anteil.is_some()
    };
    let stapel: Option<Vec<u8>> = lanes_daten.as_ref().map(|l| l.stapel.clone());
    let mut gelegt = lanes_legen(&feld, feld_fest, stil, entlang, &kinder, &zellen, &spuren, normal, stapel.as_deref());
    // Release 11.10.2026: Grundlinien auf der Rasterachse, wenn die Spuren
    // feststehen (`gitter::lanes_grundlinien`).
    if let Some(l) = lanes_daten.as_ref() {
        crate::gitter::lanes_grundlinien(l, &mut gelegt);
    }
    // Release 11.10.2026: fuer Subgrids in den Spuren das Ergebnis ablegen.
    if let Some(l) = lanes_daten {
        let starts: Vec<f32> = gelegt.iter().map(|p| p.x).collect();
        let hoehe = (stil.hoehe_angabe.is_some() || stil.hoehe_anteil.is_some()).then_some(content.height);
        crate::gitter::lanes_ablegen(ctx, node, &l, zeilen_spuren, &starts, content.width, hoehe);
    }
    Ok(if zeilen_spuren {
        gelegt
            .into_iter()
            .map(|p| Platzierung { x: p.y, y: p.x, breite: p.hoehe, hoehe: p.breite })
            .collect()
    } else {
        gelegt
    })
}

/// Die Masonry-Platzierung in Spalten-Spuren (die gedrehte Rechnung fuer
/// Zeilen-Spuren steht in `lanes_verteilen`). Jedes Element kommt in
/// Reihenfolge in die kuerzeste Spur (Spuren, die hoechstens
/// `flow-tolerance` laenger sind, gelten als gleich — dann die erste, bei
/// `track-reverse` die letzte), feste Platzierungen (`grid-column: 2`)
/// behalten ihre Spur. Am Ende werden die letzten Elemente jeder Spur mit
/// `align-self: stretch` (und `auto`-Groesse) bis zum Ende der laengsten
/// Spur gedehnt; `fill-reverse` fuellt vom Ende her.
#[allow(clippy::too_many_arguments)]
fn lanes_legen(
    feld: &Rect,
    feld_fest: bool,
    stil: &narsil_layout::LayoutStil,
    luecke: f32,
    arten: &[RasterKind],
    zellen: &[Platzierung],
    spuren: &[(f32, f32)],
    normal: Vec<Platzierung>,
    // Release 11.10.2026: die Ausrichtung auf der Masonry-Achse je Element
    // (`gitter::Lanes::stapel`), `None` = die alte Regel.
    stapel: Option<&[u8]>,
) -> Vec<Platzierung> {
    // Die Spuren des Rasters, dazu implizite (aus festen Platzierungen
    // hinter der Vorlage), die nur an ihren Zellen zu sehen sind.
    let mut lanes: Vec<(f32, f32)> = spuren.iter().copied().filter(|(x, b)| x.is_finite() && b.is_finite()).collect();
    for (k, z) in arten.iter().zip(zellen.iter()) {
        if k.spann_spalten <= 1 && z.breite > 0.0 && lanes.len() < 4096 && !lanes.iter().any(|(x, _)| (x - z.x).abs() < 0.5) {
            lanes.push((z.x, z.breite));
        }
    }
    if lanes.is_empty() {
        return normal;
    }
    lanes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let n = lanes.len();
    let luecke = if luecke.is_finite() && luecke > 0.0 { luecke } else { 0.0 };
    let start = feld.y;
    let mut pos = vec![start; n];
    let mut letzte: Vec<Option<usize>> = vec![None; n];
    let mut raus = normal.clone();
    let spur_umkehr = stil.lanes_umkehr & 2 != 0;
    let toleranz = if stil.lanes_toleranz.is_nan() { 0.0 } else { stil.lanes_toleranz };
    // Die Spur, in der ein Punkt liegt.
    let spur_bei = |a: f32| lanes.iter().rposition(|(x, _)| *x <= a + 0.5).unwrap_or(0);
    // Kollabierte Spuren (`auto-fit`, Breite 0) bekommen keine
    // automatisch platzierten Elemente.
    let offen = |j: usize, spann: usize| lanes[j..j + spann].iter().any(|(_, b)| *b > 0.0);
    // Der Platzierungs-Cursor: Ende (bzw. bei `track-reverse` Anfang) des
    // vorigen automatisch platzierten Elements.
    let mut cursor_vor = 0usize;
    let mut cursor_rueck = n;
    // `grid-lanes-pack: dense`: die Luecken `[von, bis)` je Spur, die ein
    // breiteres Element ueber der Laufposition offen gelassen hat.
    let dicht = stil.lanes_umkehr & 4 != 0;
    let mut luecken: Vec<Vec<(f32, f32)>> = vec![Vec::new(); n];
    for (i, (k, p)) in arten.iter().zip(normal.iter()).enumerate() {
        let Some(zelle) = zellen.get(i) else { continue };
        let spann = k.spann_spalten.max(1).min(n);
        let eigen = if k.hoch.is_finite() && k.hoch > 0.0 { k.hoch } else { 0.0 };
        let fest_lage = (k.spalte_start != 0).then(|| spur_bei(zelle.x).min(n - spann));
        // Dicht: die HOECHSTE Luecke, in die das Element (mit der Luecke
        // danach) passt; bei Gleichstand die erste Spur. GEMESSEN an WPT
        // `column-dense-packing-005` („the highest track-opening over the
        // start-most").
        let in_luecke = if dicht {
            let mut beste: Option<(f32, usize)> = None;
            let lagen: Vec<usize> = match fest_lage {
                Some(j) => vec![j],
                None => (0..=n - spann).filter(|&j| offen(j, spann)).collect(),
            };
            for j in lagen {
                for l in j..j + spann {
                    for &(y, _) in &luecken[l] {
                        let passt = (j..j + spann).all(|m| {
                            luecken[m].iter().any(|&(von, bis)| von <= y + 0.01 && y + eigen + luecke <= bis + 0.01)
                        });
                        if passt && beste.is_none_or(|(by, bj)| y < by - 0.01 || ((y - by).abs() <= 0.01 && j < bj)) {
                            beste = Some((y, j));
                        }
                    }
                }
            }
            beste
        } else {
            None
        };
        if let Some((y, j)) = in_luecke {
            for l in j..j + spann {
                if let Some(w) = luecken[l].iter().position(|&(von, bis)| von <= y + 0.01 && y + eigen + luecke <= bis + 0.01) {
                    let (von, bis) = luecken[l].remove(w);
                    if y - von > 0.01 {
                        luecken[l].push((von, y));
                    }
                    if bis - (y + eigen + luecke) > 0.01 {
                        luecken[l].push((y + eigen + luecke, bis));
                    }
                }
            }
            let anfang = lanes[j].0;
            let (ende_x, ende_b) = lanes[j + spann - 1];
            let mut q = *p;
            if (p.breite - zelle.breite).abs() < 0.5 && (p.x - zelle.x).abs() < 0.5 {
                q.x = anfang;
                q.breite = (ende_x + ende_b - anfang).max(0.0);
            } else {
                q.x = anfang + (p.x - zelle.x);
            }
            q.y = y;
            q.hoehe = eigen;
            raus[i] = q;
            continue;
        }
        let j = if let Some(j) = fest_lage {
            j
        } else {
            let mut min = f32::INFINITY;
            for j in 0..=n - spann {
                if offen(j, spann) {
                    min = min.min(pos[j..j + spann].iter().copied().fold(f32::MIN, f32::max));
                }
            }
            let passt = |j: usize| {
                offen(j, spann) && pos[j..j + spann].iter().copied().fold(f32::MIN, f32::max) <= min + toleranz + 0.01
            };
            // Unter den gleich kurzen gewinnt die erste Lage HINTER dem
            // vorigen automatisch platzierten Element (mit Umlauf), bei
            // `track-reverse` die erste davor. GEMESSEN an drei WPT-
            // Referenzen (`column-initial-flow-tolerance`, `column-order-
            // property-auto-placement-001`, `row-auto-repeat-019`): die
            // erste Lage ueberhaupt traf nur die erste davon.
            let letzte_lage = n - spann;
            let gewaehlt = if spur_umkehr {
                let ab = cursor_rueck.checked_sub(spann).map(|c| c.min(letzte_lage));
                ab.into_iter()
                    .flat_map(|c| (0..=c).rev())
                    .chain((ab.map_or(0, |c| c + 1)..=letzte_lage).rev())
                    .find(|&j| passt(j))
                    .unwrap_or(letzte_lage)
            } else {
                let ab = cursor_vor.min(letzte_lage + 1);
                (ab..=letzte_lage).chain(0..ab).find(|&j| passt(j)).unwrap_or(0)
            };
            cursor_vor = gewaehlt + spann;
            cursor_rueck = gewaehlt;
            gewaehlt
        };
        let hier = pos[j..j + spann].iter().copied().fold(f32::MIN, f32::max);
        if dicht {
            for l in j..j + spann {
                if pos[l] < hier - 0.01 {
                    luecken[l].push((pos[l], hier));
                }
            }
        }
        let anfang = lanes[j].0;
        let (ende_x, ende_b) = lanes[j + spann - 1];
        let spann_breite = (ende_x + ende_b - anfang).max(0.0);
        let mut q = *p;
        // Quer: gestreckt fuellt das Element die Spuren; sonst behaelt es
        // seinen Versatz in der Zelle (`justify-self`).
        if (p.breite - zelle.breite).abs() < 0.5 && (p.x - zelle.x).abs() < 0.5 {
            q.x = anfang;
            q.breite = spann_breite;
        } else {
            q.x = anfang + (p.x - zelle.x);
        }
        q.y = hier;
        q.hoehe = eigen;
        raus[i] = q;
        for l in j..j + spann {
            pos[l] = hier + eigen + luecke;
            letzte[l] = Some(i);
        }
    }
    // Das Ende der laengsten Spur (ohne die letzte Luecke), mindestens der
    // Inhalt des Containers, wenn er eine Groesse hat (fuer `fill-reverse`).
    let mut ende = pos.iter().map(|p| p - luecke).fold(start, f32::max);
    if feld.height.is_finite() && feld.height > 0.0 {
        ende = ende.max(start + feld.height);
    }
    // `stretch` dehnt das letzte Element jeder Spur nur bis zum Ende eines
    // FESTEN Containers — nicht bis zur laengsten Spur. GEMESSEN an WPT:
    // `independent-formatting-context` (feste 100 px: gedehnt) gegen
    // `column-subgrid-grid-gap-001` (`auto`: `C` behaelt seine Hoehe).
    // Release 11.10.2026: mit den Ausrichtungen der neuen Rechnung richtet
    // sich das letzte Element jeder Spur im Rest bis zum Ende aus: `stretch`
    // dehnt (auch bei unbestimmter Groesse), `center`/`end` verschieben,
    // `normal` dehnt nur in einem festen Container. GEMESSEN vorher: WPT
    // `grid-lanes/alignment/column-align-items-*` (die Referenzen legen den
    // Rest per Flex auf das letzte Element).
    if let Some(st) = stapel.filter(|s| s.len() == arten.len()) {
        for l in 0..n {
            let Some(i) = letzte[l] else { continue };
            let k = &arten[i];
            let q = &mut raus[i];
            let frei = ende - (q.y + q.hoehe);
            // Bei `fill-reverse` wird danach gespiegelt: Anfang und Ende tauschen.
            let code = match (st[i], stil.lanes_umkehr & 1 != 0) {
                (4, true) => 2,
                (2, true) => 4,
                (c, _) => c,
            };
            match code {
                1 if k.hoch_auto => q.hoehe = (ende - q.y).max(q.hoehe),
                // `normal` dehnt nur Spalten-Spuren (die Masonry-Achse ist die
                // Blockachse); in Zeilen-Spuren sind die Elemente `fit-content`
                // (GEMESSEN: WPT `grid-lanes/grid-placement/row-explicit-placement-*`).
                0 if feld_fest && k.hoch_auto && stil.lanes != 2 => q.hoehe = (ende - q.y).max(q.hoehe),
                3 if frei > 0.0 => q.y += frei / 2.0,
                4 if frei > 0.0 => q.y += frei,
                _ => {}
            }
        }
    } else {
        for l in (0..n).filter(|_| feld_fest) {
            let Some(i) = letzte[l] else { continue };
            let k = &arten[i];
            if k.selbst_vertikal == FlexAusrichtung::Strecke && k.hoch_auto {
                let q = &mut raus[i];
                q.hoehe = (ende - q.y).max(q.hoehe);
            }
        }
    }
    if stil.lanes_umkehr & 1 != 0 {
        for q in raus.iter_mut() {
            q.y = start + (ende - (q.y + q.hoehe));
        }
    }
    raus
}

/// Der relative Versatz eines Knotens (0,0 ohne `relative`/`sticky`).
///
/// L5 (02.10.2026 14:6x): `Klebend` ist HIER RAUS.
///
/// GEMESSENER BEFUND, was vorher hier stand und was es tat
/// (`tests/l5_absolute.rs`, `l5_sticky_…`): `Klebend` wurde wie
/// `Relativ` behandelt, also:addiert. GEMESSEN:
///
/// ```text
/// sticky; top: -50  als erstes Kind   y = -50   (aus dem Elternteil heraus)
/// sticky; top: 100  als letztes Kind  y = 500   (aus dem Elternteil heraus)
/// ```
///
/// Beides ist falsch: CSS Position 3, §3.4 erlaubt dem Klebekasten
/// nicht, seinen enthaltenden Block zu verlassen, und er verschiebt
/// sich nur in die Richtung, in der die Insets es verlangen.
///
/// GEMESSENE ENTSCHEIDUNG, warum `sticky` hier KEINEN Versatz
/// bekommt und die Regel spaeter laeuft: der relative Versatz ist
/// „immer addieren". Die Kleb-Regel ist „nur wenn noetig, und dann
/// gekappt". Ein Versatz an dieser Stelle koennte nicht mehr
/// rueckgenommen werden — die Summe laege dann schon im Ergebnis.
fn relative_versatz_fuer(stil: &narsil_layout::LayoutStil) -> (f32, f32) {
    if matches!(stil.position, PositionArt::Relativ) {
        versatz_relativ(stil.oben, stil.rechts, stil.unten, stil.links)
    } else {
        (0.0, 0.0)
    }
}

/// Die Kleb-Position nach CSS Position 3, §3.4.
///
/// GEMESSENE REGEL, in der Form, die die Tests brauchen:
///
/// ```text
/// y = statische Lage
/// wenn `top` gesetzt ist und y < top:  y = min(top, Eltern_unten - hoehe)
/// wenn `bottom` gesetzt ist und y + hoehe > Ansicht_unten - bottom:
///     y = max(Ansicht_unten - bottom - hoehe, Eltern_oben)
/// zuletzt: y auf [Eltern_oben, Eltern_unten - hoehe] klemmen
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum `min(top, …)` beim Vorschieben: Ein
/// Kasten, der 400 px ueber dem Elternteil herausgeschoben wuerde, ist
/// kein Klebekasten mehr, sondern ein zweiter. Die Klammer gehoert an
/// die Stelle, an der sie noetig ist — beim Schieben, nicht erst
/// danach.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Ansicht fuer `bottom` und NICHT
/// fuer `top` gilt: In der statischen Lage ist der Scrollversatz 0,
/// also liegt der Scrollport bei (0, 0). Fuer `top` ist die Ansicht
/// damit der Nullpunkt und die Bedingung `y < top` bereits der ganze
/// Fall; fuer `bottom` traegt sie die Rechnung.
fn kleb_y(
    statisch_y: f32,
    hoehe: f32,
    oben: Option<f32>,
    unten: Option<f32>,
    eltern: &Rect,
    // Release 12.10.2026 (Agent CSS2): der Rollbereich statt immer der
    // Ansicht (CSS Position 3 §3.4: das Klebe-Rechteck ist der Rollbereich des
    // naechsten rollenden Vorfahren). GEMESSEN vorher (WPT css-position
    // `sticky/position-sticky-margins-002`): `bottom: 0` in einem 100-px-
    // Kasten mit `overflow: hidden` klebte am unteren Rand der Ansicht.
    feld: Rect,
) -> f32 {
    let mut y = statisch_y;
    if let Some(t) = oben {
        if y < feld.y + t {
            y = (feld.y + t).min(eltern.y + eltern.height - hoehe);
        }
    }
    if let Some(b) = unten {
        let grenze = feld.y + feld.height - b - hoehe;
        if y + hoehe > grenze {
            y = grenze.max(eltern.y);
        }
    }
    y.clamp(eltern.y, (eltern.y + eltern.height - hoehe).max(eltern.y))
}

/// `justify-content` aus dem Plan in das CSS-Ziel.
fn css_buendig(b: narsil_layout::plan::Buendig) -> FlexBuendig {
    use narsil_layout::plan::Buendig as B;
    match b {
        B::Start => FlexBuendig::Start,
        B::Mitte => FlexBuendig::Mitte,
        B::Ende => FlexBuendig::Ende,
        B::Zwischen => FlexBuendig::Zwischen,
        B::Ringsum => FlexBuendig::Ringsum,
        B::Gleichmaessig => FlexBuendig::Gleichmaessig,
    }
}

/// `align-items` aus dem Plan in das CSS-Ziel.
fn css_ausrichtung(a: narsil_layout::plan::Ausrichtung) -> FlexAusrichtung {
    use narsil_layout::plan::Ausrichtung as A;
    match a {
        A::Start => FlexAusrichtung::Start,
        A::Mitte => FlexAusrichtung::Mitte,
        A::Ende => FlexAusrichtung::Ende,
        // L4 (13:5x): `baseline` kommt an (§6.4). GEMESSENER BEFUND,
        // dass es noetig ist: `examples/l3_flex.rs`, Block H6 —
        // `align-items: baseline` war nicht von `flex-start` zu
        // unterscheiden, weil der Plan kein `baseline` kennen konnte.
        A::Grundlinie => FlexAusrichtung::Grundlinie,
        A::Strecke => FlexAusrichtung::Strecke,
    }
}

/// Eine gesetzte Laenge als Zahl; `Auto` (und alles Unbekannte) wird
/// `None`, weil `width:auto` in 10.3.7 die Dehnung ausloest und nicht
/// eine Zahl von 0.
fn laenge_px(l: narsil_core::length::Length) -> Option<f32> {
    match l {
        narsil_core::length::Length::Px(v) if v.is_finite() => Some(v),
        _ => None,
    }
}

/// Kappt eine Zahl auf >= 0.
/// L4 (03.10.2026 13:4x): §6.4 — der Aufstieg der Schrift im Kind, also
/// der Abstand von der Oberkante bis zur ERSTEN Grundlinie.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Aufstieg hier und nicht in
/// `narsil-css::flex`: hier kennt man den DOM, und nur der weiss, ob im
/// Kind ueberhaupt Text steht.  Ein Kasten ohne Text hat keine
/// Grundlinie; fuer ihn gilt die Oberkante (0).
///
/// GEMESSENE ENTSCHEIDUNG, warum `schriftgrad` und nicht die gemessene
/// Hoehe: ein Kind mit `height: 40px` und einer Zeile 16-px-Schrift hat
/// einen Aufstieg von 16 px * 5/7 = 11,4 px, nicht 40 * 5/7 = 28,6.  Die
/// vorhandene Naeherung `grundlinie_von(kind, hoehe)` rechnet mit der
/// KASTENhoehe und damit bei einem Kind mit eigener Hoehe daneben.
///
/// GEMESSENE REGEL fuer die Tiefe: `hat_text` laeuft bis `TIEFE` (5),
/// weil die Kette aus einem selbst geschriebenen Bomben-Test (M2/M3)
/// keine Tiefenbegrenzung hat.
fn aufstieg_von(ctx: &Ctx<'_>, id: narsil_dom::NodeId, cst: &narsil_layout::LayoutStil) -> f32 {
    if !hat_text(ctx, id, 5) {
        return 0.0;
    }
    narsil_text::ASCENT_RATIO * cst.schriftgrad
}

/// Steht im Teilbaum von `id` ein Textknoten? Tiefensgrenze `tiefen`.
fn hat_text(ctx: &Ctx<'_>, id: narsil_dom::NodeId, tiefen: u8) -> bool {
    if tiefen == 0 {
        return false;
    }
    // GEMESSEN an `Ctx` (`narsil-layout-engine/src/lib.rs:198`): es gibt
    // `stil(id)` und `stil_oder_standard(id)`, aber KEIN `node(id)`.
    // Der Baum steht in `ctx.document`.
    let Some(node) = ctx.document.get(id) else {
        return false;
    };
    if matches!(node.kind, narsil_dom::NodeKind::Text) {
        return true;
    }
    node.children
        .iter()
        .any(|&kind| hat_text(ctx, kind, tiefen - 1))
}

/// L4 (03.10.2026 11:5x): §6.3 / §6.2 — der VORGABEWERT des Kindes.
///
/// `None` heisst `auto`, und `auto` heisst „den Wert des CONTAINERS
/// benutzen" (`align-items` / `justify-items`).  Sonst gewinnt der Wert
/// des Kindes (`align-self` / `justify-self`).
///
/// GEMESSENER BEFUND, dass es noetig ist: Release,
/// `crates/narsil-page/examples/l4_raster.rs`, Block AB (11:5x) —
/// `align-items: center` und `flex-end` ergaben beide `h 100` bei einer
/// Spur von 100 px, also genau das, was `stretch` tut.  Der Wert kam an
/// (`Berechnet::flex_ausrichtung`), wurde aber von der
/// Raster-Platzierung nicht gelesen.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine EIGENE Funktion und kein
/// `unwrap_or_else` inline: `kind_platzieren` braucht ein `Ctx`, also
/// ist der Schritt sonst nur ueber die ganze Seite messbar.  Als
/// benannte Funktion hat er drei Tests (unten) und damit eine Zahl,
/// die man nachstellen kann, ohne den Browser.
/// Release 11.10.2026: eine Achse des Rasterbereichs eines absolut
/// positionierten Elements (CSS Grid 1 §9.1): Linie `l > 0` ist der Anfang
/// der Spur `l - 1`, `l < 0` zaehlt vom Ende, `0` (`auto`) ist die
/// Innenabstandskante (`kante_anfang`/`kante_ende`); eine Spanne ergaenzt
/// die fehlende Linie.
fn raster_bereich_achse(
    start: i32,
    ende: i32,
    spann: usize,
    spuren: &[(f32, f32)],
    kante_anfang: f32,
    kante_ende: f32,
) -> (f32, f32) {
    let n = spuren.len() as i32;
    if n == 0 {
        return (kante_anfang, kante_ende);
    }
    let index = |l: i32| -> Option<i32> {
        if l > 0 {
            Some(l - 1)
        } else if l < 0 {
            Some((n + 1 + l).max(0))
        } else {
            None
        }
    };
    let spann = spann.max(1) as i32;
    let mut s = index(start);
    let mut e = index(ende);
    if e.is_none() && start != 0 {
        e = s.map(|k| k + spann);
    }
    if s.is_none() && ende != 0 {
        s = e.map(|k| (k - spann).max(0));
    }
    let anfang = |k: i32| -> f32 {
        if k >= n {
            let (x, w) = spuren[(n - 1) as usize];
            x + w
        } else {
            spuren[k.max(0) as usize].0
        }
    };
    let schluss = |k: i32| -> f32 {
        if k <= 0 {
            spuren[0].0
        } else {
            let (x, w) = spuren[(k.min(n) - 1) as usize];
            x + w
        }
    };
    let a = s.map_or(kante_anfang, anfang);
    let b = e.map_or(kante_ende, schluss);
    (a, b.max(a))
}

fn raster_vorgabe(
    kind_stil: &narsil_layout::LayoutStil,
    eltern_stil: &narsil_layout::LayoutStil,
) -> (narsil_css::FlexAusrichtung, narsil_css::FlexAusrichtung) {
    let vertikal = match kind_stil.selbst_vertikal {
        Some(a) => ausrichtung_raster(a),
        None => ausrichtung_raster(eltern_stil.flex_ausrichtung),
    };
    let waagerecht = match kind_stil.selbst_waagerecht {
        Some(a) => ausrichtung_raster(a),
        None => ausrichtung_raster(eltern_stil.justify_ausrichtung),
    };
    (vertikal, waagerecht)
}

#[cfg(test)]
mod tests_vorgabe {
    use super::raster_vorgabe;
    use narsil_css::FlexAusrichtung::{Ende, Mitte, Start, Strecke};
    use narsil_layout::{Ausrichtung, LayoutStil};

    fn stil() -> LayoutStil {
        LayoutStil::standard()
    }

    /// §6.3: ohne eigenes `align-self` gewinnt `align-items`.
    #[test]
    fn auto_nimmt_den_wert_des_containers() {
        let mut eltern = stil();
        eltern.flex_ausrichtung = Ausrichtung::Mitte;
        eltern.justify_ausrichtung = Ausrichtung::Ende;
        let kind = stil(); // `selbst_vertikal`/`selbst_waagerecht` = None
        assert_eq!(
            raster_vorgabe(&kind, &eltern),
            (Mitte, Ende),
            "`auto` = der Wert des Containers, auf beiden Achsen"
        );
    }

    /// §6.6 + §6.3: `align-self` am Kind schlaegt `align-items`.
    #[test]
    fn der_wert_des_kindes_schlaegt_den_container() {
        let mut eltern = stil();
        eltern.flex_ausrichtung = Ausrichtung::Mitte;
        let mut kind = stil();
        kind.selbst_vertikal = Some(Ausrichtung::Start);
        assert_eq!(
            raster_vorgabe(&kind, &eltern).0,
            Start,
            "der Wert des Kindes gewinnt"
        );
    }

    /// Der Anfangswert: kein `align-items` heisst `stretch` (§6.2 — der
    /// Anfangswert von `align-items` ist `normal`, und `normal` verhaelt
    /// sich im Raster wie `stretch`).
    ///
    /// GEMESSENE FALLE, die dieser Test umgeht: `LayoutStil::standard()`
    /// traegt `flex_ausrichtung: Start` (`narsil-layout/src/plan.rs:566`),
    /// waehrend die KASKADE `FlexAusrichtung::Strecke` als Anfangswert
    /// setzt (`narsil-css/src/typen.rs:2348`).  Deshalb wird der Container
    /// hier ausdruecklich auf `Strecke` gestellt und nicht aus
    /// `standard()` gelesen — der Unterschied ist gemessen, nicht
    /// geraten, und der Test soll ihn nicht einschliessen.
    #[test]
    fn ohne_alles_bleibt_strecke() {
        let mut eltern = stil();
        eltern.flex_ausrichtung = Ausrichtung::Strecke;
        eltern.justify_ausrichtung = Ausrichtung::Strecke;
        assert_eq!(raster_vorgabe(&stil(), &eltern), (Strecke, Strecke));
    }
}

pub(crate) fn sauber_pos(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, crate::plan::MAX_WERT_PX)
    } else {
        0.0
    }
}

/// Release 12.10.2026 (Agent CSS2): `letter-spacing`/`word-spacing` (CSS 2.1
/// §16.4) — die Teile des Laufs (`narsil_display::abstands_teile`) und ihre
/// x-Versaetze, gemessen wie die Breite des Laufs (`text::abstand_zugabe`).
/// Der Lauf bleibt EIN Kasten und EIN Text-Item; der Raster setzt die Teile.
/// GEMESSEN vorher (Koordinator, `ui_bild::ui2_die_startseite_hat_suchfeld_
/// kacheln_und_verlauf`): je Zeichen ein eigener Lauf und ein eigenes Item.
fn abstands_versaetze(ctx: &Ctx<'_>, node: &narsil_dom::Node) -> Option<Box<[(u32, f32)]>> {
    let text = node.text.as_deref()?;
    let teile = narsil_display::abstands_teile(text, node.style.buchstaben_abstand, node.style.wort_abstand);
    if teile.is_empty() || teile.len() > 4096 {
        return None;
    }
    let mut x = 0.0f32;
    let mut raus = Vec::with_capacity(teile.len());
    for (a, e, zugabe) in teile {
        raus.push((u32::try_from(a).ok()?, x));
        let w = ctx.measurer.measure_width_lauf(
            &text[a..e],
            node.style.font_size,
            node.style.schriftgewicht,
            node.style.kursiv,
            &node.style.schriftfamilie,
        );
        x += if w.is_finite() { w } else { 0.0 } + zugabe;
    }
    Some(raus.into_boxed_slice())
}

/// Setzt die positionierten Eintraege Bottom-Up zu einem Baum zusammen.
///
/// Reihenfolge: die Eintraege liegen in Tiefensuche-Vorwaertsreihenfolge vor,
/// also haben Kinder IMMER groessere Indizes als ihr Eltern. Beim Zuruecklaufen
/// sind die Kinder fertig, bevor das Elterngebäude gebaut wird.
fn assemble(entries: &[Entry]) -> Result<LayoutBox, LayoutError> {
    let mut children: Vec<Vec<LayoutBox>> = vec![Vec::new(); entries.len()];
    let mut root: Option<LayoutBox> = None;

    for index in (0..entries.len()).rev() {
        let entry = &entries[index];
        let mut bx = LayoutBox::new(entry.id, entry.border);
        bx.content_box = entry.content;
        bx.background = entry.background;
        bx.text = entry.text.clone();
        // P8: die Mal-Reihenfolge braucht sie, und `narsil` braucht sie
        // fuer den Scrollversatz.
        bx.position = entry.position;
        bx.z_index = entry.z_index;
        bx.ist_flex_container = entry.ist_flex_container;
        bx.transformt = entry.transformt;
        bx.transform = entry.transform.clone();
        bx.hintergrund_clip = entry.hintergrund_clip;
        bx.ueberlauf = entry.ueberlauf;
        bx.schwebt = entry.schwebt;
        bx.mal_kasten = entry.mal_kasten;
        bx.abstands_versaetze = entry.versaetze.clone();
        let mut kids = std::mem::take(&mut children[index]);
        // Kinder wurden in umgekehrter Reihenfolge angehaengt.
        kids.reverse();
        bx.children = kids;
        // Release 11.10.2026: `order` fuer die Malreihenfolge (display.rs).
        bx.ordnung = entry.ordnung;

        match entry.parent {
            Some(parent) => children[parent].push(bx),
            None => root = Some(bx),
        }
    }

    root.ok_or(LayoutError::UnknownNode(narsil_dom::NodeId(0)))
}
