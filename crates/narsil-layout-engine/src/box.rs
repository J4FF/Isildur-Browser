//! Aufloesung der Boxgroessen: `width`, `height`, `margin`, `padding`.
//!
//! ## Auto-Aufloesung (TASK_0004, verbindlich)
//!
//! Fuer jeden Knoten gilt, wenn `width`/`height` nicht explizit gesetzt sind:
//!
//! * `direction: column` (Haupt-Achse = y):
//!   Hoehe = Summe der Kinderhoehen + deren Margins + eigenes Padding,
//!   Breite = Content-Breite des Elterns abzueglich eigener Margins.
//! * `direction: row` (Haupt-Achse = x):
//!   Breite = Summe der Kinderbreiten + deren Margins + eigenes Padding,
//!   Hoehe = Content-Hoehe des Elterns abzueglich eigener Margins.
//! * `page`-Root: fuellt den Viewport, ausser `width`/`height` sind explizit
//!   gesetzt. Dann gewinnen die expliziten Werte.
//! * Textknoten (Blatt): Breite = gemessene Textbreite, Hoehe = Zeilenhoehe
//!   des [`crate::TextMeasurer`]. Der Wert ist die INHALTSGROESSE; die
//!   Border-Box addiert das Padding.
//!
//! Kinder strecken sich also in der Querachse auf die Elternbreite/-hoehe,
//! in der Hauptachse werden sie summiert. Prozentwerte beziehen sich IMMER
//! auf die Content-Box des Elterns — auf der Hauptachse eines `auto`-groessen
//! Elterns ist diese waehrend der Summenbildung noch nicht bekannt; dort faellt
//! der Referenzwert auf den Raum zurueck, den der Container selbst von seinem
//! Eltern bekommt (siehe [`compute_sizes`]).
//!
//! ## Keine Rekursion
//!
//! [`compute_sizes`] arbeitet mit einem expliziten Stack im HEAP. Ein Dokument
//! mit 1000 verschachtelten Containern laeuft damit ohne Stack-Overflow,
//! unabhaengig von der Stackgroesse des Aufrufers.

use narsil_core::geometry::{Edges, Size};
use narsil_core::length::{Length, Spacing};
use narsil_core::style::Direction;
use narsil_dom::{Node, NodeId, NodeKind};

use crate::text;
use crate::{Ctx, LayoutError};

/// Achse, auf der die Kinder eines Containers summiert werden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

/// Aufgabe der Messphase.
enum SizeTask {
    /// Flex/Grid brauchen nach dem Messen ihrer Kinder eine eigene Autohoehe.
    Format {
        id: NodeId,
        width: f32,
        height: Option<f32>,
        padding: Edges,
        /// ZELLENBREITE NACHGEZOGEN? (06.10.2026)
        ///
        /// GEMESSENER FEHLER, den dieses Feld begrenzt: ein Raster-Kind
        /// wurde (wie jedes Kind) mit der Breite des **Containers**
        /// gemessen, sitzt aber in einer **Spalte**. GEMESSEN an
        /// de.wikipedia.org bei 1191 px: `.mw-body` (Spalte, 883 px)
        /// enthielt ein Raster, das mit 1103 px gemessen war — der ganze
        /// Artikeltext landete bei x=1447, also hinter dem Fensterrand.
        ///
        /// Die erste Format-Runde legt die Spuren fest (dafuer sind die
        /// Kinder gemessen), erkennt die falschen Breiten und stoesst
        /// **eine** Nachmessung mit der Zellenbreite an. `true` heisst:
        /// diese Runde ist die Nachmessung — sie stoesst keine zweite an,
        /// sonst gaebe es eine Schleife.
        nachmessung: bool,
    },
    /// Groesse EINES Knotens bestimmen (Quer-Achse und Kinder anstossen).
    Enter { id: NodeId, available: Size },
    /// Release 11.10.2026 (Agent CSS2): Tabelle, Spaltenphase — nach dem
    /// ersten Messen der Kinder (siehe `tabellen_leg::spalten_phase`).
    TabelleSpalten {
        id: NodeId,
        /// Angegebene Rahmenbreite.
        angabe: Option<f32>,
        /// Platz fuer den Rahmen.
        verfuegbar: f32,
        /// Rahmen + Innenabstand.
        rand: Edges,
        /// Angegebene Rahmenhoehe.
        hoehe: Option<f32>,
        /// Verfuegbare Hoehe fuer die Kinder.
        kind_hoehe: f32,
        /// `min-width`/`max-width` als Rahmenbreite.
        min_max: (Option<f32>, Option<f32>),
    },
    /// Release 11.10.2026 (Agent CSS2): Tabelle, Zeilenphase — nach dem
    /// Nachmessen der Zellen (siehe `tabellen_leg::zeilen_phase`).
    TabelleZeilen { id: NodeId, z: Box<crate::tabellen_leg::Zwischen> },
    /// Release 11.10.2026: `width: min-content | max-content | fit-content`
    /// aufloesen, sobald der Teilbaum gemessen ist, und den Knoten mit der
    /// Breite neu messen (CSS Sizing 3 §2.1).
    BreitenWort { id: NodeId, available: Size },
    /// Haupt-Achse nachtragen: Summe der Kinder plus eigenes Padding.
    Sum {
        id: NodeId,
        /// Fertige Quer-Achse der Border-Box.
        cross: f32,
        padding: Edges,
        /// Content-Box, gegen die Kinder gemessen wurden.
        child_available: Size,
        axis: Axis,
        /// Untergrenze fuer die Border-Box auf der Haupt-Achse.
        ///
        /// 0 fuer jeden normalen Container: `height:auto` heisst "so
        /// hoch wie die Kinder". Der `page`-Root hat eine echte
        /// Untergrenze (den Viewport) — siehe `NodeKind::Page`.
        mindest: f32,
        /// Der Kasten ist positioniert und hat KEINE Breite — er
        /// braucht SHRINK-TO-FIT (CSS 2.1, 10.3.7).
        ///
        /// L5 (02.10.2026 13:5x). GEMESSENER BEFUND, was vorher
        /// passierte: eine absolute Box ohne `width` bekam die volle
        /// Inhaltsbreite des enthaltenden Blocks (600 px statt 60 px bei
        /// einem 60-px-Kind, `tests/l5_absolute.rs`).
        schrumpft: bool,
        /// Die Hoehe ist GESETZT und darf nicht aus der Kindersumme
        /// entstehen.
        ///
        /// L5 (13:5x). GEMESSENER FEHLER, den dieses Feld behebt: Der
        /// Zweig `Some(height)` stapelt gar keinen `Sum`-Task — er setzt
        /// die Groesse und geht weiter. Ein shrink-to-fit-Kasten MIT
        /// gesetzter Hoehe braucht den Task aber fuer die BREITE, und
        /// ohne dieses Feld haette der `Sum`-Zweig die Hoehe aus der
        /// Kindersumme gemacht. Bei `height: auto` bleibt es `None`.
        feste_hoehe: Option<f32>,
    },
}

/// Ergebnis der Messphase: die Border-Box-Groesse jedes Knotens.
#[derive(Debug, Clone)]
pub(crate) struct Sizes {
    entries: Vec<Option<Size>>,
    /// **`min-content`-Breite je Knoten** (CSS Sizing 3 §5.2 / CSS 2.1
    /// §10.3.7): die Breite der **breitesten unumbrechbaren Stelle**.
    ///
    /// **WARUM EINE EIGENE SPALTE und kein Feld in `Size`:** `Size` ist
    /// die **Border-Box**, die dem Aufrufer (`flow.rs`) gehoert und dort
    /// weiterreist. `min-content` ist eine **Rechengroesse der
    /// Messphase** und geht nach aussen nicht heraus — sie wird nur
    /// innerhalb von `compute_sizes` gebraucht, um den **Boden** fuer
    /// die shrink-to-fit-Formel zu liefern.
    ///
    /// **GEMESSENER GRUND, und er ist der Befund von 00:52 in Zahlen:**
    /// auf spiegel.de stehen sechs Navigationstexte als **`0.00 x 790.40`**
    /// (08:04), ihr Flex-Item (#39) traegt `width: auto`, `PLAN
    /// eintrag=Some(Flex)` und **`grenzen` alle `None`** — und
    /// `box.rs` rechnet `min(max(mindest, verfuegbar), bevorzugt)` mit
    /// **`mindest = 0`**. **Bei `min-content = 0` faellt die Kappe auf 0.**
    min_content: Vec<f32>,
    /// Knoten, deren Stilangabe unbrauchbar war und die darum den
    /// Standard bekommen haben — mit der Begruendung, in der Reihenfolge
    /// des Dokumentbaums.
    ///
    /// M7 (02.10.2026): GEMESSENE REGRESSION. `https://archlinux.org/`
    /// liess sich mit der Bruecke aus `WUNSCH_LAYOUT.txt` nicht mehr
    /// laden: `Layout: Ungueltige Laenge bei Knoten NodeId(9): font-size:
    /// 0 muss endlich und > 0 sein`. EIN Knoten hat die ganze Seite
    /// verworfen. CSS 2.1, 10.1 sagt: "User agents must ignore the
    /// declaration" — die ERKLAERUNG faellt weg, nicht die Seite. Darum
    /// bekommen kaputte Knoten den Standard, und `Meldung::VerworfeneKnoten`
    /// sagt, wie viele es waren.
    verworfene: Vec<(NodeId, String)>,
    /// Release 09.10.2026: MARGIN-COLLAPSING (CSS 2.1, 8.3.1) — je Knoten
    /// die Raender seines ersten/letzten Kindes, die durch ihn hindurch
    /// nach aussen gewandert sind (`oben`, `unten`). Gesetzt in
    /// `sum_children`, gelesen in `flow::kind_platzieren`: beide Phasen
    /// rechnen mit DENSELBEN Werten.
    hochgezogen: std::collections::HashMap<u32, (Option<f32>, Option<f32>)>,
    /// Release 10.10.2026: der verfuegbare Platz, mit dem jeder Knoten
    /// zuletzt gemessen wurde — fuer das Nachmessen eines Teilbaums
    /// (`absolute_nachmessen`).
    verfuegbar: Vec<Size>,
    /// Release 10.10.2026: erzwungene Border-Box-Hoehen (absolute Kaesten,
    /// deren Hoehe erst gegen den fertig gemessenen enthaltenden Block
    /// feststeht, siehe `absolute_nachmessen`).
    /// Release 11.10.2026 (Agent CSS2): erzwungene Rahmenbreiten — die
    /// Zellbreite aus dem Tabellen-Layout, nur waehrend des Nachmessens der
    /// Zelle (siehe `tabellen_leg`).
    erzwungen_breite: std::collections::HashMap<u32, f32>,
    /// Release 11.10.2026 (Agent CSS2): die natuerliche Rahmenhoehe einer
    /// Zelle (Inhalt + Innenabstand, ohne ihre `height` als Mindestmass) —
    /// fuer `vertical-align: middle|bottom`.
    natuerlich: std::collections::HashMap<u32, f32>,
    /// Release 11.10.2026 (Agent CSS2): die Lage jeder gelegten Tabelle.
    tabellen: std::collections::HashMap<u32, crate::tabellen_leg::TabellenLage>,
    /// Release 11.10.2026 (Agent CSS2): Versatz des Zelleninhalts nach
    /// `vertical-align` (CSS 2.1 §17.5.3).
    zellen_versatz: std::collections::HashMap<u32, f32>,
    /// Release 11.10.2026 (Agent CSS2): wie weit die Floats eines Kastens
    /// OHNE eigenen Formatierungskontext unter seine Rahmenoberkante reichen
    /// (Rand-Unterkante), wenn sie aus ihm herausragen — fuer den Kasten,
    /// dessen Kontext sie angehoeren (CSS 2.1 §10.6.7).
    float_ueberhang: std::collections::HashMap<u32, f32>,
    erzwungen: std::collections::HashMap<u32, f32>,
    /// Release 10.10.2026: der Viewport — der enthaltende Block jedes
    /// `position: fixed` (CSS 2.1 §10.1, Punkt 3).
    fenster: Size,
    /// Release 11.10.2026: die aufgeloesten Inhaltsbreiten eines Kastens mit
    /// `width`/`min-width`/`max-width: min-content | max-content |
    /// fit-content` (CSS Sizing 3 §2.1), je Knoten mit der verfuegbaren
    /// Breite, fuer die sie gelten: (verfuegbar, width, min-width, max-width).
    breite_wort: std::collections::HashMap<u32, (f32, Option<f32>, Option<f32>, Option<f32>)>,
    /// Release 15.10.2026: the inputs of the measurement of a node that is
    /// running (`offen`) and of the last finished one (`fertig`), see
    /// [`MessSignatur`]. An `Enter` with the inputs of the last finished
    /// measurement skips the subtree: the result would be the same.
    messung_offen: std::collections::HashMap<u32, MessSignatur>,
    messung_fertig: std::collections::HashMap<u32, MessSignatur>,
    /// Release 15.10.2026: the parent of each entered node and, per node, the
    /// generation of the latest size change in its subtree (`set`, `set_min`
    /// walk up). An intrinsic width computed for a node stays valid while
    /// its subtree has not changed (`breiten_cache`).
    eltern: Vec<u32>,
    stand: Vec<u32>,
    generation: u32,
    breiten_cache: std::cell::RefCell<std::collections::HashMap<(u32, u8, u8), (f32, u32)>>,
    /// Release 11.10.2026: Kaesten, deren Hoehe aus `aspect-ratio` kommt und
    /// die mit ihrem Inhalt wachsen duerfen (CSS Sizing 4 §5.2: `min-height:
    /// auto` ist dann die Inhaltsgroesse, solange `overflow` sichtbar ist).
    verhaeltnis_wachsend: std::collections::HashSet<u32>,
    /// Release 14.10.2026: while set, the intrinsic width of the box at depth
    /// 0 is its OWN min-/max-content size, not clamped by its own
    /// `min-width`/`max-width` (`clamp_contribution` is for contributions to
    /// a parent). See [`Sizes::eigene_breite`].
    eigene_breite: std::cell::Cell<bool>,
}

impl Sizes {
    fn new(len: usize) -> Self {
        Self {
            entries: vec![None; len],
            min_content: vec![0.0; len],
            verworfene: Vec::new(),
            hochgezogen: std::collections::HashMap::new(),
            verfuegbar: vec![Size::new(0.0, 0.0); len],
            erzwungen: std::collections::HashMap::new(),
            float_ueberhang: std::collections::HashMap::new(),
            breite_wort: std::collections::HashMap::new(),
            messung_offen: std::collections::HashMap::new(),
            messung_fertig: std::collections::HashMap::new(),
            eltern: vec![u32::MAX; len],
            stand: vec![0; len],
            generation: 0,
            breiten_cache: std::cell::RefCell::new(std::collections::HashMap::new()),
            verhaeltnis_wachsend: std::collections::HashSet::new(),
            eigene_breite: std::cell::Cell::new(false),
            fenster: Size::new(0.0, 0.0),
            erzwungen_breite: std::collections::HashMap::new(),
            natuerlich: std::collections::HashMap::new(),
            tabellen: std::collections::HashMap::new(),
            zellen_versatz: std::collections::HashMap::new(),
        }
    }

    /// Release 11.10.2026 (Agent CSS2): siehe Feld `natuerlich`.
    pub(crate) fn natuerliche_hoehe(&self, id: NodeId) -> Option<f32> {
        self.natuerlich.get(&id.0).copied()
    }

    /// Release 11.10.2026 (Agent CSS2): die Lage einer gelegten Tabelle.
    pub(crate) fn tabellen_lage(&self, id: NodeId) -> Option<&crate::tabellen_leg::TabellenLage> {
        self.tabellen.get(&id.0)
    }

    /// Release 11.10.2026 (Agent CSS2): Versatz des Zelleninhalts.
    pub(crate) fn zellen_versatz(&self, id: NodeId) -> Option<f32> {
        self.zellen_versatz.get(&id.0).copied()
    }

    /// Release 11.10.2026 (Agent CSS2): siehe `Sizes::float_ueberhang`.
    pub(crate) fn float_ueberhang(&self, id: NodeId) -> Option<f32> {
        self.float_ueberhang.get(&id.0).copied()
    }

    /// Release 09.10.2026: die nach aussen gewanderten Kinderraender von
    /// `id` (oben, unten) — `None`, wo nichts wanderte.
    pub(crate) fn hochgezogen(&self, id: NodeId) -> (Option<f32>, Option<f32>) {
        self.hochgezogen.get(&id.0).copied().unwrap_or((None, None))
    }

    /// Release 09.10.2026: die WIRKSAMEN senkrechten Raender eines Kindes —
    /// seine eigenen, zusammengefallen mit denen, die aus seinem ersten
    /// bzw. letzten Kind nach aussen gewandert sind.
    pub(crate) fn wirksamer_rand(&self, id: NodeId, oben: f32, unten: f32) -> (f32, f32) {
        let (o, u) = self.hochgezogen(id);
        (
            o.map_or(oben, |o| rand_zusammen(oben, o)),
            u.map_or(unten, |u| rand_zusammen(unten, u)),
        )
    }

    /// Die Knoten, deren Angabe unbrauchbar war (P5, 02.10.2026).
    ///
    /// Der Aufrufer (`layout_mit_plan`) nimmt die Liste mit, damit
    /// `LayoutTree::verworfene` sie weitergeben kann. Sie ist fast immer
    /// leer, und `is_empty()` sagt das ohne Allokation.
    pub(crate) fn verworfene_knoten(&self) -> Vec<(NodeId, String)> {
        self.verworfene.clone()
    }

    /// Release 11.10.2026: der verfuegbare Platz der letzten Messung (fuer
    /// die Nachmessung der Rasterelemente, `gitter.rs`).
    pub(crate) fn verfuegbar_von(&self, id: NodeId) -> Size {
        self.verfuegbar.get(id.index()).copied().unwrap_or(Size::new(0.0, 0.0))
    }

    /// Groesse eines Knotens, `None` wenn der Knoten nie gemessen wurde.
    pub(crate) fn get(&self, id: NodeId) -> Option<Size> {
        self.entries.get(id.index()).copied().flatten()
    }

    /// Setzt die `min-content`-Breite eines Knotens.
    fn set_min(&mut self, id: NodeId, breite: f32) {
        self.geaendert(id);
        if let Some(stelle) = self.min_content.get_mut(id.0 as usize) {
            *stelle = non_negative(breite);
        }
    }

    /// Die `min-content`-Breite eines Kindes, `0.0` wenn es keine gibt.
    ///
    /// **0.0 ist hier eine echte Antwort und kein Platzhalter:** ein
    /// Knoten ohne Inhalt (leerer `div`, `hr`) **hat** keine
    /// unumbrechbare Stelle, und ein leerer Kasten darf keinen Boden
    /// bekommen. **Deshalb wird hier NICHT mit `unwrap_or(f32::MAX)`
    /// gearbeitet**, wie es bei einer Kind-Summe noetig waere.
    fn min_of(&self, id: NodeId) -> f32 {
        self.min_content.get(id.0 as usize).copied().unwrap_or(0.0)
    }

    /// **`min-content` eines Containers** ueber seine Kinder.
    ///
    /// **DIE REGEL, und sie ist der Kern von CSS Sizing 3 §5.2:**
    /// * **Block / Spalte** — die Kinder stehen **untereinander**, also
    ///   begrenzt das **breiteste** Kind die Breite. **Maximum.**
    /// * **Zeile / Flex** — die Kinder stehen **nebeneinander**, also
    ///   summieren sich ihre kleinsten Breiten. **Summe.**
    ///
    /// **UND DER UNTERSCHIED IST DER GANZ UNTERSCHIED**, weil ein
    /// Maximum **0** liefert, wenn ein Kind 0 hat (etwa ein leerer
    /// `div` neben dem Text) — **und die Summe nicht.** Auf spiegel.de
    /// hat der Kasten **2 Kinder** (`ELTER #39 kinder=2`), und eines
    /// davon ist ein reiner Zierstrich.
    // L4 (04.10.2026 03:2x):oeffentlich fuer den Float-Zweig in `flow.rs`
    // (CSS 2.1 §10.3.5, das `preferred minimum width` eines Float-Kastens).
    pub(crate) fn min_aggregat(&self, ctx: &Ctx<'_>, node: &Node, richtung: Direction) -> f32 {
        let mut acc = 0.0f32;
        for k in &node.children {
            let plan = ctx.stil_oder_standard(*k);
            if !plan.wird_gezeichnet()
                || matches!(
                    plan.position,
                    narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest
                )
            {
                continue;
            }
            let mut m = self.min_of(*k);
            // Release 12.10.2026 (Agent CSS2): ein Kind mit fester Breite traegt
            // seine Rahmenbreite bei (CSS Sizing 3 §5.1: die min-content-
            // Zuteilung eines Kastens mit bestimmter `width` ist diese Breite).
            // GEMESSEN vorher (WPT `normal-flow/inline-block-width-002a`,
            // `inline-table-width-001a`): ein `inline-block` um einen 20-em-
            // Block schrumpfte in 10 em auf 10 em.
            // Prozentuale Innenabstaende sind dabei zyklisch und zaehlen 0
            // (CSS Sizing 3 §5.2.1; WPT css-sizing `abspos-auto-sizing-fit-
            // content-percentage-003`, Test von Agent A).
            if let Some(kn) = ctx.document.get(*k)
                && let Length::Px(w) = kn.style.size.width
                && kn.kind == NodeKind::Container
                && !ist_zeilenbox(ctx, *k, kn)
                && w.is_finite()
            {
                let px = |l: Length| if let Length::Px(v) = l { v.max(0.0) } else { 0.0 };
                let r = rahmen_edges(&kn.style.rahmen);
                let rahmen_box = ctx.stil(*k).is_some_and(|p| p.border_box);
                let zugabe = if rahmen_box {
                    0.0
                } else {
                    px(kn.style.padding.left) + px(kn.style.padding.right) + r.left + r.right
                };
                m = m.max(w.max(0.0) + zugabe);
            }
            match richtung {
                Direction::Column => acc = acc.max(m),
                Direction::Row => acc += m,
            }
        }
        non_negative(acc)
    }

    /// Inhaltsbreite fuer auto-Flexbasis. Tiefe 64 begrenzt fremde DOM-Baeume.
    /// Release 14.10.2026: [`Sizes::preferred_width_raw`] limited by the min/max
    /// constraints that `min-height`/`max-height` transfer through
    /// `aspect-ratio` (CSS Sizing 4 §5.1, see `ratio_width_limits`).
    /// Release 15.10.2026: cached per layout pass while the subtree is
    /// unchanged (`Sizes::breite_aus_cache`). MEASURED before: https://www.chip.de/
    /// (nested flex lists) evaluated 171 000 intrinsic widths for 3 800 nodes per
    /// layout, 310 of 500 ms of `messen`.
    pub(crate) fn bevorzugte_breite(&self, ctx: &Ctx<'_>, id: NodeId, tiefe: usize) -> f32 {
        if let Some(v) = self.breite_aus_cache(id, 0, tiefe) {
            return v;
        }
        let w = self.bevorzugte_breite_neu(ctx, id, tiefe);
        self.breite_merken(id, 0, tiefe, w)
    }

    fn bevorzugte_breite_neu(&self, ctx: &Ctx<'_>, id: NodeId, tiefe: usize) -> f32 {
        let w = self.preferred_width_raw(ctx, id, tiefe);
        let w = self.fit_content_contribution(ctx, id, tiefe).unwrap_or(w);
        match ctx.document.get(id).and_then(|node| ratio_width_limits(ctx, self, id, node)) {
            Some((lo, hi)) => w.min(hi).max(lo),
            None => w,
        }
    }

    /// Release 14.10.2026: the box's OWN (min-content, max-content) width,
    /// without its own `min-width`/`max-width` (CSS Sizing 3 §5.1: the
    /// min-/max-content size; `clamp_contribution` turns it into a
    /// contribution). For `min-width: min-content`, the flex base size and the
    /// like. MEASURED before: WPT css-flexbox `flex-item-min-width-min-content-
    /// max-width` (`min-width: min-content; max-width: 50px` around 100 px:
    /// 50 instead of 100 px), `flex-base-size-ignores-max-width`.
    pub(crate) fn eigene_breite(&self, ctx: &Ctx<'_>, id: NodeId, max: bool) -> f32 {
        let alt = self.eigene_breite.replace(true);
        let w = if max { self.max_content_breite(ctx, id, 0) } else { self.min_inhalt_breite(ctx, id, 0) };
        self.eigene_breite.set(alt);
        w
    }

    /// Release 14.10.2026: the min-/max-content contribution of a box with
    /// `width: fit-content(<length>)` (no percentage): the box's size does not
    /// depend on the available space, so both contributions are
    /// `min(max-content, max(min-content, length))` (CSS Sizing 3 §2.1,
    /// §5.2). MEASURED before: WPT css-sizing `fit-content-length-percentage-
    /// 011`/`-014` (a `min-content` parent took the 60 px of one word).
    fn fit_content_contribution(&self, ctx: &Ctx<'_>, id: NodeId, tiefe: usize) -> Option<f32> {
        let st = ctx.stil(id)?;
        let lp = st.fit_content_arg[0].filter(|lp| !lp.hat_prozent)?;
        if st.groessen_art[0] != 3 {
            return None;
        }
        let node = ctx.document.get(id)?;
        let pad = resolve_spacing(node.style.padding, 0.0, id, "padding").unwrap_or(Edges::ZERO);
        let border = rahmen_edges(&node.style.rahmen);
        let aussen = pad.left + pad.right + border.left + border.right;
        let l = non_negative(lp.px);
        let l = if st.border_box { l.max(aussen) } else { l + aussen };
        let mi = self.min_content_width_raw(ctx, id, tiefe);
        let ma = self.max_content_width_raw(ctx, id, tiefe).max(mi);
        Some(ma.min(l.max(mi)))
    }

    fn preferred_width_raw(&self, ctx: &Ctx<'_>, id: NodeId, tiefe: usize) -> f32 {
        const MAX_INTRINSIK_TIEFE: usize = 64;
        let Some(node) = ctx.document.get(id) else {
            return 0.0;
        };
        // Release 14.10.2026: a box with `aspect-ratio` and a fixed height
        // contributes the width transferred through the ratio (CSS Sizing 4
        // §5.1). MEASURED before: WPT css-sizing
        // `aspect-ratio/intrinsic-size-001` (`width: min-content` around
        // `height: 100px; aspect-ratio: 1/1`: 0 px).
        if let Some(b) = ratio_width_contribution(ctx, self, id, node) {
            return b;
        }
        // Release 14.10.2026: a size-contained box contributes only its
        // `contain-intrinsic-size` (CSS Contain 2 §3.2).
        if let Some(b) = containment_width_contribution(ctx, id, node) {
            return b;
        }
        // Release 11.10.2026 (Agent CSS2): eine Tabelle will die Summe ihrer
        // Spalten (siehe `tabellen_leg::tabellen_inhaltsbreiten`), nicht das
        // Maximum ihrer Zeilen.
        if tiefe < MAX_INTRINSIK_TIEFE && crate::tabellen_leg::ist_tabelle(ctx, id) {
            return crate::tabellen_leg::tabellen_inhaltsbreiten(ctx, self, id, tiefe).1;
        }
        let gemessen = self.get(id).unwrap_or(Size::ZERO).width;
        // Release 08.10.2026: die feste Breite einer Zeilenbox ist nur ihre
        // Ausrichtungsbreite (siehe `ist_zeilenbox`), nicht ihr Wunsch.
        if tiefe >= MAX_INTRINSIK_TIEFE
            || node.kind == NodeKind::Text
            || node.ersetzt.is_some()
            || (node.style.size.width != Length::Auto && !ist_zeilenbox(ctx, id, node))
        {
            return gemessen;
        }
        // Release 11.10.2026: ein Raster misst seine Spalten (CSS Grid 1 §12.1).
        if let Some(b) = crate::gitter::intrinsisch(self, ctx, id, true, tiefe) {
            return b;
        }
        // Release 11.10.2026: ebenso ein Flex-Container (`flexbox.rs`).
        if let Some(b) = crate::flexbox::intrinsisch(self, ctx, id, true, tiefe) {
            return b;
        }
        let p = ctx.stil_oder_standard(id);
        let reihe = node.style.direction == Direction::Row
            || (p.anzeige == narsil_layout::plan::Anzeige::Flex
                && p.flex_richtung == narsil_layout::FlexRichtung::Reihe);
        let mut w = 0.0f32;
        let mut zeile = 0.0f32;
        let mut schwebend = 0.0f32;
        for &c in node.children.iter().take(crate::plan::MAX_KINDER) {
            let cp = ctx.stil_oder_standard(c);
            if !cp.wird_gezeichnet()
                || matches!(
                    cp.position,
                    narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest
                )
            {
                continue;
            }
            let Some(n) = ctx.document.get(c) else {
                continue;
            };
            // Release 14.10.2026: percentages in the margins of a child are
            // cyclic for an intrinsic size and count as 0; the fixed part of a
            // `calc()` stays (CSS Sizing 3 §5.2.1). MEASURED before: WPT
            // css-sizing `calc-margins-block` (`margin-left: calc(10% +
            // 100px)` in a `min-content` box added 10 % of the old width).
            let rand =
                resolve_spacing(n.style.margin, 0.0, c, "margin").unwrap_or(Edges::ZERO);
            let cw = self.bevorzugte_breite(ctx, c, tiefe + 1) + rand.left + rand.right;
            if reihe {
                w += cw;
            } else if crate::flow::kind_ist_inline(ctx, c, &n.kind) {
                // Release 08.10.2026: aufeinanderfolgende Inline-Kinder
                // (Woerter, Inline-Bloecke) stehen in EINER Zeile — ihre
                // max-content-Breite ist die SUMME (CSS Sizing 3 §5.1).
                // GEMESSEN vorher: spiegel.de, `<span>Ihr taegliches Extra
                // bei SPIEGEL+<span><svg></span></span>` als Flex-Element
                // bekam das breiteste Wort (92 px) als Basis und brach
                // nach jedem Wort um.
                // Release 11.10.2026 (Agent CSS2): ein kollabierbares
                // Leerzeichen am Anfang oder Ende der Zeile zaehlt nicht (CSS
                // 2.1 §16.6.1) — es wartet, bis ein Inhalt folgt. GEMESSEN
                // vorher (WPT `padding-right-applies-to-014`): ein Float mit
                // einer `inline-table` und Leerraum danach war 16 px breiter.
                if crate::flow::kollabierer_leerraum(ctx, c) {
                    if zeile > 0.0 {
                        schwebend += cw;
                    }
                } else {
                    zeile += schwebend + cw;
                    schwebend = 0.0;
                    w = w.max(zeile);
                }
            } else {
                zeile = 0.0;
                schwebend = 0.0;
                w = w.max(cw);
            }
        }
        let padding =
            resolve_spacing(node.style.padding, gemessen, id, "padding").unwrap_or(Edges::ZERO);
        // BILD_35: **der Rahmen ist Teil der Aussenmasse** (CSS 2.1 §10.1: der
        // `border-box` ist die Aussenmasse). Ohne diese Zeile war ein Kasten mit
        // `border: 8px` **8 px zu schmal** fuer seine Geschwister und fuer
        // `margin: auto`.
        let rahmen = rahmen_edges(&node.style.rahmen);
        let total = w + flex_luecken(ctx, node, &p) + padding.left + padding.right + rahmen.left + rahmen.right;
        if tiefe == 0 && self.eigene_breite.get() { total } else { clamp_contribution(ctx, id, total, padding.left + padding.right + rahmen.left + rahmen.right) }
    }

    /// **Max-Content-Breite fuer Inline-Bloecke** (P9, 05.10.2026).
    ///
    /// Wie [`Sizes::bevorzugte_breite`], aber **ohne** die Fruehabbruch-
    /// Bedingung auf `style.size.width`. GEMESSENER GRUND: `narsil-page` gibt
    /// dem Textabsatz eines Inline-Blocks eine **explizite** Breite (die
    /// Verfuegbare, damit der Text umbricht) — z. B. `Px(876)`. Damit liefert
    /// `bevorzugte_breite` fuer den Absatz die 876 statt der Textbreite, und
    /// die Schrumpfbreite waere wieder die ganze Zeile.
    ///
    /// Diese Funktion steigt in Container **immer** hinab und nimmt fuer
    /// Textknoten/ersetzte Elemente die gemessene Breite. Damit ist sie die
    /// tatsaechliche `preferred width` (CSS 2.1 §10.3.7/§10.3.9).
    ///
    /// Tiefe 64 begrenzt fremde DOM-Baeume.
    /// Release 14.10.2026: [`Sizes::max_content_width_raw`] limited by the min/max
    /// constraints that `min-height`/`max-height` transfer through
    /// `aspect-ratio` (CSS Sizing 4 §5.1, see `ratio_width_limits`).
    /// Release 15.10.2026: cached per layout pass while the subtree is
    /// unchanged (`Sizes::breite_aus_cache`). MEASURED before: https://www.chip.de/
    /// (nested flex lists) evaluated 171 000 intrinsic widths for 3 800 nodes per
    /// layout, 310 of 500 ms of `messen`.
    pub(crate) fn max_content_breite(&self, ctx: &Ctx<'_>, id: NodeId, tiefe: usize) -> f32 {
        if let Some(v) = self.breite_aus_cache(id, 1, tiefe) {
            return v;
        }
        let w = self.max_content_breite_neu(ctx, id, tiefe);
        self.breite_merken(id, 1, tiefe, w)
    }

    fn max_content_breite_neu(&self, ctx: &Ctx<'_>, id: NodeId, tiefe: usize) -> f32 {
        let w = self.max_content_width_raw(ctx, id, tiefe);
        let w = self.fit_content_contribution(ctx, id, tiefe).unwrap_or(w);
        match ctx.document.get(id).and_then(|node| ratio_width_limits(ctx, self, id, node)) {
            Some((lo, hi)) => w.min(hi).max(lo),
            None => w,
        }
    }

    fn max_content_width_raw(&self, ctx: &Ctx<'_>, id: NodeId, tiefe: usize) -> f32 {
        // (Zeilenbox: siehe `ist_zeilenbox`.)
        const MAX_INTRINSIK_TIEFE: usize = 64;
        let Some(node) = ctx.document.get(id) else {
            return 0.0;
        };
        // Release 14.10.2026: a box with `aspect-ratio` and a fixed height
        // contributes the width transferred through the ratio (CSS Sizing 4
        // §5.1). MEASURED before: WPT css-sizing
        // `aspect-ratio/intrinsic-size-001` (`width: min-content` around
        // `height: 100px; aspect-ratio: 1/1`: 0 px).
        if let Some(b) = ratio_width_contribution(ctx, self, id, node) {
            return b;
        }
        // Release 14.10.2026: a size-contained box contributes only its
        // `contain-intrinsic-size` (CSS Contain 2 §3.2).
        if let Some(b) = containment_width_contribution(ctx, id, node) {
            return b;
        }
        // Release 11.10.2026 (Agent CSS2): Tabelle, siehe `bevorzugte_breite`.
        if tiefe < MAX_INTRINSIK_TIEFE && crate::tabellen_leg::ist_tabelle(ctx, id) {
            return crate::tabellen_leg::tabellen_inhaltsbreiten(ctx, self, id, tiefe).1;
        }
        let gemessen = self.get(id).unwrap_or(Size::ZERO).width;
        // Release 07.10.2026: eine ZEILENBOX des Seitenbauers (nur Textkinder)
        // misst ihren Inhalt, auch wenn sie eine feste Breite trägt — die
        // bekommt sie nur für `text-align: center/right`, und sie ist die
        // geschätzte Absatzbreite. GEMESSEN: die rechtsbündige Rangzelle von
        // news.ycombinator.com meldete so ~1 800 px max-content.
        let zeilenbox = ist_zeilenbox(ctx, id, node);
        if !zeilenbox && let Some(b) = flex_feste_breite(ctx, id, node) {
            return b;
        }
        if tiefe >= MAX_INTRINSIK_TIEFE
            || node.kind == NodeKind::Text
            || node.ersetzt.is_some()
            || (matches!(node.style.size.width, Length::Px(_)) && !zeilenbox)
        {
            return gemessen;
        }
        // Release 11.10.2026: ein Raster misst seine Spalten (CSS Grid 1 §12.1).
        if let Some(b) = crate::gitter::intrinsisch(self, ctx, id, true, tiefe) {
            return b;
        }
        // Release 11.10.2026: ebenso ein Flex-Container (`flexbox.rs`).
        if let Some(b) = crate::flexbox::intrinsisch(self, ctx, id, true, tiefe) {
            return b;
        }
        let p = ctx.stil_oder_standard(id);
        let reihe = node.style.direction == Direction::Row
            || p.anzeige == narsil_layout::plan::Anzeige::InlineBlock
            || (p.anzeige == narsil_layout::plan::Anzeige::Flex
                && p.flex_richtung == narsil_layout::FlexRichtung::Reihe);
        let mut w = 0.0f32;
        let mut zeile = 0.0f32;
        let mut schwebend = 0.0f32;
        for &c in node.children.iter().take(crate::plan::MAX_KINDER) {
            let cp = ctx.stil_oder_standard(c);
            if !cp.wird_gezeichnet()
                || matches!(
                    cp.position,
                    narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest
                )
            {
                continue;
            }
            let Some(n) = ctx.document.get(c) else {
                continue;
            };
            // Release 14.10.2026: percentages in the margins of a child are
            // cyclic for an intrinsic size and count as 0; the fixed part of a
            // `calc()` stays (CSS Sizing 3 §5.2.1). MEASURED before: WPT
            // css-sizing `calc-margins-block` (`margin-left: calc(10% +
            // 100px)` in a `min-content` box added 10 % of the old width).
            let rand =
                resolve_spacing(n.style.margin, 0.0, c, "margin").unwrap_or(Edges::ZERO);
            let cw = self.max_content_breite(ctx, c, tiefe + 1) + rand.left + rand.right;
            if reihe {
                w += cw;
            } else if crate::flow::kind_ist_inline(ctx, c, &n.kind) {
                // Release 08.10.2026: aufeinanderfolgende Inline-Kinder
                // (Woerter, Inline-Bloecke) stehen in EINER Zeile — ihre
                // max-content-Breite ist die SUMME (CSS Sizing 3 §5.1).
                // GEMESSEN vorher: spiegel.de, `<span>Ihr taegliches Extra
                // bei SPIEGEL+<span><svg></span></span>` als Flex-Element
                // bekam das breiteste Wort (92 px) als Basis und brach
                // nach jedem Wort um.
                // Release 11.10.2026 (Agent CSS2): ein kollabierbares
                // Leerzeichen am Anfang oder Ende der Zeile zaehlt nicht (CSS
                // 2.1 §16.6.1) — es wartet, bis ein Inhalt folgt. GEMESSEN
                // vorher (WPT `padding-right-applies-to-014`): ein Float mit
                // einer `inline-table` und Leerraum danach war 16 px breiter.
                if crate::flow::kollabierer_leerraum(ctx, c) {
                    if zeile > 0.0 {
                        schwebend += cw;
                    }
                } else {
                    zeile += schwebend + cw;
                    schwebend = 0.0;
                    w = w.max(zeile);
                }
            } else {
                zeile = 0.0;
                schwebend = 0.0;
                w = w.max(cw);
            }
        }
        let padding =
            resolve_spacing(node.style.padding, gemessen, id, "padding").unwrap_or(Edges::ZERO);
        let rahmen = rahmen_edges(&node.style.rahmen);
        let total = w + flex_luecken(ctx, node, &p) + padding.left + padding.right + rahmen.left + rahmen.right;
        if tiefe == 0 && self.eigene_breite.get() { total } else { clamp_contribution(ctx, id, total, padding.left + padding.right + rahmen.left + rahmen.right) }
    }

    /// Die `min-content`-Breite eines Kastens, rekursiv (CSS Sizing 3 §5.1):
    /// die breiteste unumbrechbare Stelle, plus Füllung und Rahmen.
    ///
    /// Release 07.10.2026 für das automatische Tabellenlayout (CSS 2.1
    /// §17.5.2.2): `lege_tabelle` kann Spalten nach Inhalt verteilen, bekam
    /// aber nie eine Zahl. Eine Zeilenbox des Seitenbauers (nur Textkinder)
    /// zählt wie ein Absatz: umbrechbar zwischen ihren Läufen, also das
    /// Maximum, nicht die Summe.
    /// Release 14.10.2026: [`Sizes::min_content_width_raw`] limited by the min/max
    /// constraints that `min-height`/`max-height` transfer through
    /// `aspect-ratio` (CSS Sizing 4 §5.1, see `ratio_width_limits`).
    /// Release 15.10.2026: cached per layout pass while the subtree is
    /// unchanged (`Sizes::breite_aus_cache`). MEASURED before: https://www.chip.de/
    /// (nested flex lists) evaluated 171 000 intrinsic widths for 3 800 nodes per
    /// layout, 310 of 500 ms of `messen`.
    pub(crate) fn min_inhalt_breite(&self, ctx: &Ctx<'_>, id: NodeId, tiefe: usize) -> f32 {
        if let Some(v) = self.breite_aus_cache(id, 2, tiefe) {
            return v;
        }
        let w = self.min_inhalt_breite_neu(ctx, id, tiefe);
        self.breite_merken(id, 2, tiefe, w)
    }

    fn min_inhalt_breite_neu(&self, ctx: &Ctx<'_>, id: NodeId, tiefe: usize) -> f32 {
        let w = self.min_content_width_raw(ctx, id, tiefe);
        let w = self.fit_content_contribution(ctx, id, tiefe).unwrap_or(w);
        match ctx.document.get(id).and_then(|node| ratio_width_limits(ctx, self, id, node)) {
            Some((lo, hi)) => w.min(hi).max(lo),
            None => w,
        }
    }

    fn min_content_width_raw(&self, ctx: &Ctx<'_>, id: NodeId, tiefe: usize) -> f32 {
        const MAX_INTRINSIK_TIEFE: usize = 64;
        let Some(node) = ctx.document.get(id) else {
            return 0.0;
        };
        // Release 14.10.2026: a box with `aspect-ratio` and a fixed height
        // contributes the width transferred through the ratio (CSS Sizing 4
        // §5.1). MEASURED before: WPT css-sizing
        // `aspect-ratio/intrinsic-size-001` (`width: min-content` around
        // `height: 100px; aspect-ratio: 1/1`: 0 px).
        if let Some(b) = ratio_width_contribution(ctx, self, id, node) {
            return b;
        }
        // Release 14.10.2026: a size-contained box contributes only its
        // `contain-intrinsic-size` (CSS Contain 2 §3.2).
        if let Some(b) = containment_width_contribution(ctx, id, node) {
            return b;
        }
        // Release 11.10.2026 (Agent CSS2): Tabelle, siehe `bevorzugte_breite`.
        if tiefe < MAX_INTRINSIK_TIEFE && crate::tabellen_leg::ist_tabelle(ctx, id) {
            return crate::tabellen_leg::tabellen_inhaltsbreiten(ctx, self, id, tiefe).0;
        }
        let gemessen = self.get(id).unwrap_or(Size::ZERO).width;
        let padding =
            resolve_spacing(node.style.padding, gemessen, id, "padding").unwrap_or(Edges::ZERO);
        let rahmen = rahmen_edges(&node.style.rahmen);
        let rand_innen = padding.left + padding.right + rahmen.left + rahmen.right;
        if node.kind == NodeKind::Text {
            return self.min_of(id) + rand_innen;
        }
        // Release 08.10.2026: ein Kasten mit PROZENT-Breite traegt zur
        // min-content-Breite nur Fuellung und Rahmen bei (CSS Sizing 3
        // §5.2.1: Prozent sind zyklisch, ersetzte/Formular-Elemente sind
        // „compressible" mit Beitrag 0). GEMESSEN vorher: t-online.de,
        // `<input class="w-full">` gab seinem Flex-Element 900 px
        // Mindestbreite — das Suchfeld schob den Knopf „SUCHEN" aus dem Bild.
        if tiefe > 0 && node.style.size.width.hat_prozent() {
            return rand_innen;
        }
        let nur_text = ist_zeilenbox(ctx, id, node);
        if !nur_text && let Some(b) = flex_feste_breite(ctx, id, node) {
            return b;
        }
        // Eine Zeilenbox mit fester Breite (Ausrichtung) misst trotzdem ihren
        // Inhalt — siehe `max_content_breite`.
        if tiefe >= MAX_INTRINSIK_TIEFE
            || node.ersetzt.is_some()
            || (matches!(node.style.size.width, Length::Px(_)) && !nur_text)
        {
            return gemessen;
        }
        // Release 11.10.2026: ein Raster misst seine Spalten (CSS Grid 1 §12.1).
        if let Some(b) = crate::gitter::intrinsisch(self, ctx, id, false, tiefe) {
            return b;
        }
        if let Some(b) = crate::flexbox::intrinsisch(self, ctx, id, false, tiefe) {
            return b;
        }
        let p = ctx.stil_oder_standard(id);
        let reihe = !nur_text
            && (node.style.direction == Direction::Row
                || (p.anzeige == narsil_layout::plan::Anzeige::Flex
                    && p.flex_richtung == narsil_layout::FlexRichtung::Reihe
                    && p.flex_umbruch == narsil_layout::plan::FlexUmbruch::Keiner));
        let mut w = 0.0f32;
        for &c in node.children.iter().take(crate::plan::MAX_KINDER) {
            let cp = ctx.stil_oder_standard(c);
            if !cp.wird_gezeichnet()
                || matches!(
                    cp.position,
                    narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest
                )
            {
                continue;
            }
            let Some(n) = ctx.document.get(c) else {
                continue;
            };
            // Release 14.10.2026: percentages in the margins of a child are
            // cyclic for an intrinsic size and count as 0; the fixed part of a
            // `calc()` stays (CSS Sizing 3 §5.2.1). MEASURED before: WPT
            // css-sizing `calc-margins-block` (`margin-left: calc(10% +
            // 100px)` in a `min-content` box added 10 % of the old width).
            let rand =
                resolve_spacing(n.style.margin, 0.0, c, "margin").unwrap_or(Edges::ZERO);
            let cw = self.min_inhalt_breite(ctx, c, tiefe + 1) + rand.left + rand.right;
            if reihe {
                w += cw;
            } else {
                w = w.max(cw);
            }
        }
        if tiefe == 0 && self.eigene_breite.get() { w + rand_innen } else { clamp_contribution(ctx, id, w + rand_innen, rand_innen) }
    }

    /// Release 15.10.2026: a size in the subtree of `id` changed: mark `id`
    /// and its ancestors (see `breiten_cache`).
    fn geaendert(&mut self, id: NodeId) {
        self.generation = self.generation.wrapping_add(1);
        let mut k = id.0;
        for _ in 0..4096 {
            let Some(st) = self.stand.get_mut(k as usize) else { break };
            *st = self.generation;
            match self.eltern.get(k as usize) {
                Some(&e) if e != u32::MAX => k = e,
                _ => break,
            }
        }
    }

    /// Release 15.10.2026: a cached intrinsic width of `id` (kind 0
    /// preferred, 1 max-content, 2 min-content), valid while the subtree
    /// of `id` is unchanged. Only near the root of a query (`tiefe <= 16`):
    /// deeper down the depth limits of the intrinsic functions could matter.
    fn breite_aus_cache(&self, id: NodeId, art: u8, tiefe: usize) -> Option<f32> {
        if tiefe > 16 {
            return None;
        }
        let stand = *self.stand.get(id.index())?;
        match self.breiten_cache.borrow().get(&(id.0, art, self.cache_tiefe(tiefe))) {
            Some(&(v, st)) if st == stand => Some(v),
            _ => None,
        }
    }

    /// The depth class of a cached width: below the root of a query, at the
    /// root, at the root asking for the box's OWN size (`eigene_breite`).
    fn cache_tiefe(&self, tiefe: usize) -> u8 {
        match (tiefe, self.eigene_breite.get()) {
            (0, false) => 1,
            (0, true) => 2,
            _ => 0,
        }
    }

    fn breite_merken(&self, id: NodeId, art: u8, tiefe: usize, wert: f32) -> f32 {
        if tiefe <= 16
            && let Some(&stand) = self.stand.get(id.index())
        {
            self.breiten_cache.borrow_mut().insert((id.0, art, self.cache_tiefe(tiefe)), (wert, stand));
        }
        wert
    }

    fn set(&mut self, id: NodeId, size: Size) {
        if let Some(slot) = self.entries.get_mut(id.index()) {
            *slot = Some(size);
        }
        self.geaendert(id);
        // Release 15.10.2026: the measurement of `id` has produced a size; its
        // inputs are the ones of the running measurement (see `enter`).
        if let Some(sig) = self.messung_offen.get(&id.0) {
            self.messung_fertig.insert(id.0, *sig);
        }
    }
}

/// Release 15.10.2026 (Agent CSS2): an intrinsic width contribution of a box
/// with `width: auto` is clamped by its own `min-width`/`max-width`, and
/// `min-width` wins (CSS Sizing 3 §5.1, CSS 2.1 §10.4). Percentages count as
/// unset here (they are cyclic against an intrinsic size). MEASURED before
/// (WPT css-sizing `min-width-max-width-precedence`, an abspos around a block
/// with `min-width: 260px; max-width: 200px`): the abspos was 0 px wide.
fn clamp_contribution(ctx: &Ctx<'_>, id: NodeId, total: f32, fuellung: f32) -> f32 {
    let (min_b, max_b, _, _) = grenzen(ctx, id);
    // A percentage limit arrives in `grenzen` resolved against the viewport;
    // it is cyclic here and counts as unset (WPT css-sizing `percentage-min-
    // width`, `min-width: 100%` in a `fit-content` box).
    let (pm_b, pa_b, _, _) = grenzen_prozent(ctx, id, 0.0, 0.0);
    let min_b = if pm_b.is_some() { None } else { min_b };
    let max_b = if pa_b.is_some() { None } else { max_b };
    let border_box = ctx.stil(id).is_some_and(|p| p.border_box);
    let rahmen = |v: f32| non_negative(if border_box { v } else { v + fuellung });
    let t = max_b.map_or(total, |m| total.min(rahmen(m)));
    min_b.map_or(t, |m| t.max(rahmen(m)))
}

/// Release 09.10.2026: zwei aneinanderstossende senkrechte Raender fallen
/// zusammen (CSS 2.1, 8.3.1): beide positiv — der groessere; beide negativ
/// — der negativere; gemischt — die Summe.
pub(crate) fn rand_zusammen(a: f32, b: f32) -> f32 {
    if a >= 0.0 && b >= 0.0 {
        a.max(b)
    } else if a <= 0.0 && b <= 0.0 {
        a.min(b)
    } else {
        a + b
    }
}

/// Release 09.10.2026: legt `id` seine Kinder als BLOCKFLUSS untereinander
/// (und darf deshalb Geschwisterraender zusammenfallen lassen)? Nur
/// Knoten mit Plan — die Version-0-Dokumente ohne Plan addieren weiter.
pub(crate) fn ist_blockfluss(ctx: &Ctx<'_>, id: NodeId, node: &Node) -> bool {
    node.kind == NodeKind::Container
        && node.style.direction == Direction::Column
        && ctx.stil(id).is_some_and(|s| {
            matches!(
                s.anzeige,
                narsil_layout::plan::Anzeige::Block
                    | narsil_layout::plan::Anzeige::InlineBlock
                    | narsil_layout::plan::Anzeige::TabelleZelle
            )
        })
}

/// Release 09.10.2026: darf der Rand des ersten/letzten Kindes durch `id`
/// hindurch nach aussen wandern (CSS 2.1, 8.3.1)? Nur ein Block im
/// Blockfluss seines Elterns, der KEINEN neuen Formatierungskontext bildet
/// (kein `inline-block`, Flex-/Raster-Element, `float`, `absolute`,
/// `overflow` ungleich `visible`) und nicht das Wurzelelement ist.
/// Release 11.10.2026 (Agent CSS2): ein Blockkasten, der Floats MEIDET
/// (CSS 2.1 §9.5: "The border box of a table, a block-level replaced
/// element, or an element in the normal flow that establishes a new block
/// formatting context ... must not overlap the margin box of any floats"):
/// eine Tabelle, ein Flex-/Rastercontainer, ein Block mit `overflow` ungleich
/// `visible`, ein ersetztes Element als Block.
/// Release 12.10.2026 (Agent CSS2): aufeinanderfolgende Textlaeufe einer
/// Zeilenbox in derselben Schrift (ohne Rand, Innenabstand, Rahmen und
/// Abstaende dazwischen) werden als EIN Text gemessen — jeder Lauf ist so
/// breit wie der Zuwachs der Kette (Unterschneidung und Rundung ueber die
/// Grenze der Inline-Elemente hinweg, wie Browser den ganzen Inline-Text
/// formen). GEMESSEN vorher (Hinweis Agent A, WPT css-flexbox `flexbox-order-
/// only-flexitems`): drei Spans 127,99 px gegen 127,70 px als ein Lauf.
fn laeufe_als_kette(ctx: &Ctx<'_>, sizes: &mut Sizes, node: &Node) {
    const MAX_KETTE: usize = 24;
    const MAX_ZEICHEN: usize = 2000;
    let schluessel = |n: &Node| {
        let st = &n.style;
        let still = rahmen_edges(&st.rahmen) == Edges::ZERO
            && [st.margin.left, st.margin.right, st.padding.left, st.padding.right]
                .iter()
                .all(|l| l.resolve_or(0.0, 0.0) == 0.0)
            && st.buchstaben_abstand == 0.0
            && st.wort_abstand == 0.0;
        still.then(|| (st.font_size.to_bits(), st.schriftgewicht, st.kursiv, st.schriftfamilie.clone()))
    };
    let mut kette: Vec<NodeId> = Vec::new();
    let mut text = String::new();
    let mut schl: Option<(u32, u16, bool, String)> = None;
    let mut vorher = 0.0f32;
    let anpassen = |kette: &mut Vec<NodeId>, text: &mut String, vorher: &mut f32| {
        kette.clear();
        text.clear();
        *vorher = 0.0;
    };
    for &c in &node.children {
        let Some(n) = ctx.document.get(c) else { continue };
        let k = (n.kind == NodeKind::Text).then(|| schluessel(n)).flatten();
        let t = n.text.as_deref().unwrap_or("");
        let passt = k.is_some() && k == schl && kette.len() < MAX_KETTE && text.len() + t.len() <= MAX_ZEICHEN;
        if !passt {
            anpassen(&mut kette, &mut text, &mut vorher);
            schl = k.clone();
            if k.is_none() {
                continue;
            }
        }
        let Some((bits, gewicht, kursiv, familie)) = schl.clone() else { continue };
        text.push_str(t);
        let gesamt = ctx.measurer.measure_width_lauf(&text, f32::from_bits(bits), gewicht, kursiv, &familie);
        if !gesamt.is_finite() {
            anpassen(&mut kette, &mut text, &mut vorher);
            schl = None;
            continue;
        }
        if !kette.is_empty()
            && let Some(alt) = sizes.get(c)
        {
            let breite = (gesamt - vorher).max(0.0);
            sizes.set(c, Size::new(breite, alt.height));
        }
        kette.push(c);
        vorher = gesamt;
    }
}

/// Release 12.10.2026 (Agent CSS2): ein leerer Block, durch den seine Raender
/// zusammenfallen (CSS 2.1 §8.3.1: Hoehe 0, keine Rahmen/Fuellung — die
/// stecken in der Hoehe —, kein eigener Formatierungskontext, kein `clear`).
/// Mess- und Platzphase fragen dasselbe.
/// Hoehe 0 allein genuegt nicht (negative Raender der Kinder koennen die Summe
/// unter 0 druecken): der Kasten hat keinen Inhalt im Fluss — keine Zeile,
/// keine Kinder ausser absoluten, schwebenden, unsichtbaren, kollabierbarem
/// Leerraum und selbst leeren Bloecken. Floats darin zaehlen nicht als Inhalt,
/// aber ein solcher Kasten faellt nicht durch (ihr Platz haengt an seiner Lage,
/// WPT `floats/new-fc-separates-from-float`).
pub(crate) fn leer_durchlaessig(ctx: &Ctx<'_>, id: NodeId, node: &Node, size: Size, raeumt: bool) -> bool {
    !raeumt
        && size.height <= 0.0
        && node.kind == NodeKind::Container
        && !meidet_floats(ctx, id, node)
        && ohne_fluss_inhalt(ctx, node, 0)
}

/// Release 12.10.2026 (Agent CSS2): siehe [`leer_durchlaessig`].
fn ohne_fluss_inhalt(ctx: &Ctx<'_>, node: &Node, tiefe: usize) -> bool {
    if tiefe > 16 {
        return false;
    }
    node.children.iter().all(|&c| {
        let p = ctx.stil_oder_standard(c);
        if !p.wird_gezeichnet()
            || matches!(p.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)
        {
            return true;
        }
        // Floats und Kaesten mit `clear` (der Freiraum gibt ihnen Platz)
        // zaehlen als Inhalt.
        if p.schweben != narsil_layout::plan::FloatArt::Keins || p.raeumen != narsil_layout::plan::RaeumungArt::Keins {
            return false;
        }
        let Some(k) = ctx.document.get(c) else { return true };
        match k.kind {
            NodeKind::Text => crate::flow::kollabierer_leerraum(ctx, c) || k.text.as_deref().is_none_or(str::is_empty),
            NodeKind::Container => {
                k.style.size.height.resolve_or(0.0, 0.0) <= 0.0
                    && rahmen_edges(&k.style.rahmen).top <= 0.0
                    && rahmen_edges(&k.style.rahmen).bottom <= 0.0
                    && !meidet_floats(ctx, c, k)
                    && !crate::flow::kind_ist_inline(ctx, c, &k.kind)
                    && ohne_fluss_inhalt(ctx, k, tiefe + 1)
            }
            NodeKind::Page => false,
        }
    })
}

/// Release 12.10.2026 (Agent CSS2): hoechstens so oft wird ein Teilbaum fuer
/// geerbte Floats neu gemessen (M3).
const MAX_FLOAT_NACHMESSEN: usize = 4000;

/// Release 12.10.2026 (Agent CSS2): der Rahmen der Inhaltskante eines
/// Blockkindes im Inhalt des Eltern — (links, rechts, oben), oben ab der
/// Rahmenkante `kante`.
pub(crate) fn kind_rahmen(node: &Node, rand: Edges, kante: f32, bezug: f32) -> Result<(f32, f32, f32), LayoutError> {
    let r = rahmen_edges(&node.style.rahmen);
    let p = resolve_spacing(node.style.padding, bezug, node.id, "padding")?;
    Ok((rand.left + r.left + p.left, rand.right + r.right + p.right, kante + r.top + p.top))
}

/// Release 12.10.2026 (Agent CSS2): hat der Kasten einen waagerechten
/// `auto`-Rand (zentriert)? Dann erbt er keine Floats — seine Lage rechnet
/// erst die Platzphase (`auto_margin_versatz_x`).
pub(crate) fn auto_rand(ctx: &Ctx<'_>, id: NodeId) -> bool {
    ctx.stil(id).is_some_and(|s| s.marge_links_auto || s.marge_rechts_auto)
}

/// Release 12.10.2026 (Agent CSS2): reagiert der Teilbaum auf Floats des
/// umgebenden Kontexts — enthaelt er Floats, `clear` oder Kaesten, die Floats
/// meiden (bis in eigene Kontexte hinein nicht weiter)? Gemerkt je Knoten.
pub(crate) fn float_sensibel(ctx: &Ctx<'_>, id: NodeId) -> bool {
    if let Some(v) = ctx.float_sensibel.borrow().get(&id) {
        return *v;
    }
    let mut stapel = vec![(id, 0usize)];
    let mut gesehen = 0usize;
    let mut ja = false;
    while let Some((k, tiefe)) = stapel.pop() {
        gesehen += 1;
        if gesehen > 4096 || tiefe > 48 {
            ja = true;
            break;
        }
        let Some(n) = ctx.document.get(k) else { continue };
        if k != id {
            if let Some(s) = ctx.stil(k)
                && (s.schweben != narsil_layout::plan::FloatArt::Keins || s.raeumen != narsil_layout::plan::RaeumungArt::Keins)
            {
                ja = true;
                break;
            }
            if n.kind == NodeKind::Container && meidet_floats(ctx, k, n) {
                ja = true;
                break;
            }
        }
        for &c in &n.children {
            stapel.push((c, tiefe + 1));
        }
    }
    ctx.float_sensibel.borrow_mut().insert(id, ja);
    ja
}

pub(crate) fn meidet_floats(ctx: &Ctx<'_>, id: NodeId, node: &Node) -> bool {
    let Some(s) = ctx.stil(id) else { return false };
    if s.schweben != narsil_layout::plan::FloatArt::Keins
        || !matches!(s.position, narsil_layout::PositionArt::Statisch | narsil_layout::PositionArt::Relativ)
        || s.inline_aussen
    {
        return false;
    }
    use narsil_layout::plan::Anzeige as A;
    match s.anzeige {
        A::Tabelle | A::Flex | A::Raster => true,
        A::Block => s.ueberlauf != narsil_layout::plan::UeberlaufArt::Sichtbar || s.flow_root || node.ersetzt.is_some(),
        _ => false,
    }
}

pub(crate) fn darf_raender_durchlassen(ctx: &Ctx<'_>, id: NodeId, node: &Node) -> bool {
    let Some(s) = ctx.stil(id) else { return false };
    if node.kind != NodeKind::Container
        || node.style.direction != Direction::Column
        || s.anzeige != narsil_layout::plan::Anzeige::Block
        || !matches!(s.position, narsil_layout::PositionArt::Statisch | narsil_layout::PositionArt::Relativ)
        || s.schweben != narsil_layout::plan::FloatArt::Keins
        || s.ueberlauf != narsil_layout::plan::UeberlaufArt::Sichtbar
        // Release 11.10.2026 (Agent CSS2): `flow-root` ebenso.
        || s.flow_root
    {
        return false;
    }
    let Some(eltern) = node.parent else { return false };
    let Some(e) = ctx.document.get(eltern) else { return false };
    // Das Wurzelelement (Kind der Seite) laesst nichts durch.
    e.kind != NodeKind::Page && ist_blockfluss(ctx, eltern, e)
}

/// Begrenzt ein Groessen-Ergebnis auf `>= 0`.
///
/// Nient- und unendliche Werte (theoretisch bei extremen Summen) fallen
/// ebenfalls auf 0 zurueck, damit nie `NaN` in den Layout-Baum gelangt.
/// Ist `node` eine ZEILENBOX des Seitenbauers? Ihre feste Breite ist nur
/// die geschätzte Absatzbreite (für `text-align: center/right`), nicht
/// ihr Inhalt.
///
/// Release 08.10.2026: auch eine ausgerichtete Zeile, deren Läufe Links
/// sind (keine reinen Textkinder). GEMESSEN vorher: die zentrierten
/// `<li><a>`-Einträge der Navigation von rust-lang.org meldeten je
/// 1 168 px max-content — in der Flex-`<ul>` stand jeder Eintrag auf
/// einer eigenen Zeile.
pub(crate) fn ist_zeilenbox(ctx: &Ctx<'_>, id: NodeId, node: &Node) -> bool {
    if node.kind != NodeKind::Container || node.children.is_empty() {
        return false;
    }
    if node
        .children
        .iter()
        .all(|c| ctx.document.get(*c).is_some_and(|n| n.kind == NodeKind::Text))
    {
        return true;
    }
    node.style.direction == Direction::Row
        && ctx.plan.is_some_and(|p| {
            p.ausrichtung(id) != narsil_layout::Textausrichtung::Links
                && p
                    .hole(id)
                    .is_some_and(|st| st.anzeige == narsil_layout::plan::Anzeige::Block)
        })
}

/// Release 08.10.2026: steht der Kasten AUSSEN inline in der Zeile —
/// `inline-block` sowie `inline-flex`/`inline-grid` (`inline_aussen`)?
/// GEMESSEN vorher: nur `InlineBlock` zaehlte; `inline-flex`-Links und
/// `inline-grid`-Menuepunkte (t-online.de) wurden Bloecke und standen
/// untereinander.
pub(crate) fn inline_ebene(st: &narsil_layout::LayoutStil) -> bool {
    st.anzeige == narsil_layout::plan::Anzeige::InlineBlock
        || (st.inline_aussen
            && matches!(
                st.anzeige,
                // Release 11.10.2026 (Agent CSS2): auch `inline-table`.
                narsil_layout::plan::Anzeige::Flex | narsil_layout::plan::Anzeige::Raster | narsil_layout::plan::Anzeige::Tabelle
            ))
}

/// Release 08.10.2026: die Luecken (`gap`) einer Flex-REIHE gehoeren zu
/// ihrer Inhaltsbreite. GEMESSEN vorher: t-online.de, der Knopf „Suchen"
/// (`inline-flex gap-8 overflow-hidden`) war 8 px zu schmal, das „n" fiel weg.
fn flex_luecken(ctx: &Ctx<'_>, node: &Node, p: &narsil_layout::LayoutStil) -> f32 {
    if p.anzeige != narsil_layout::plan::Anzeige::Flex || p.flex_richtung != narsil_layout::FlexRichtung::Reihe {
        return 0.0;
    }
    let n = node
        .children
        .iter()
        .take(crate::plan::MAX_KINDER)
        .filter(|c| {
            let cp = ctx.stil_oder_standard(**c);
            cp.wird_gezeichnet() && !matches!(cp.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)
        })
        .count();
    non_negative(p.luecke) * n.saturating_sub(1) as f32
}

fn non_negative(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}

/// Baut den Fehlertyp fuer eine ungueltige Laenge.
fn invalid_length(node: NodeId, detail: String) -> LayoutError {
    LayoutError::InvalidLength { node, detail }
}

/// Liest einen Knoten aus dem Dokument.
pub(crate) fn get_node<'a>(ctx: &Ctx<'a>, id: NodeId) -> Result<&'a Node, LayoutError> {
    ctx.document.get(id).ok_or(LayoutError::UnknownNode(id))
}

/// Prueft den deklarierten Stil eines Knotens VOR der Aufloesung.
///
/// Geprueft wird dasselbe wie in `Style::is_valid()`, mit EINEM Unterschied:
/// `width`/`height` duerfen Prozentwerte ueber 100% haben (AKZEPTANZKRITERIUM:
/// `width: 150%` in einem 800px breiten Eltern ergibt 1200, ohne Fehler und
/// ohne Clipping). Fuer `margin`/`padding` bleibt 100% die Obergrenze, wie in
/// der API-Vorgabe zu [`LayoutError::InvalidLength`] beschrieben.
pub(crate) fn validate_style(node: &Node) -> Result<(), LayoutError> {
    // Die Referenz ist hier beliebig (1.0): geprueft wird nur die GUETE
    // des Werts, nicht sein Ergebnis.
    resolve_length(node.style.size.width, 1.0, node.id, "width")?;
    resolve_length(node.style.size.height, 1.0, node.id, "height")?;
    resolve_spacing(node.style.margin, 1.0, node.id, "margin")?;
    resolve_spacing(node.style.padding, 1.0, node.id, "padding")?;
    // `font-size: 0` ist GUELTIGES CSS (CSS 2.1, 10.1: "there is no
    // minimum or maximum value"; 0 ergibt eine Texthoehe von 0).
    // Seiten benutzen es fuer Icon-Text, Spacer und Inline-Block-Luecken.
    //
    // GEMESSENE REGRESSION 02.10.2026 (P5): hier stand `|| font_size <=
    // 0.0`. Mit der Bruecke aus `WUNSCH_LAYOUT.txt` (der Aufruf von
    // `layout_mit_plan` in `narsil`) traf das echte Seiten zum ersten Mal:
    //     https://archlinux.org/ laedt nicht mehr:
    //     Layout: Ungueltige Laenge bei Knoten NodeId(9):
    //     font-size: 0 muss endlich und > 0 sein
    // EIN KNOTEN hat die GANZE SEITE gekippt. CSS 2.1 sagt nichts
    // davon, dass 0 verboten waere.
    //
    // NUR NaN, unendlich und NEGATIV sind ein Fehler. 0 ergibt einen
    // Text ohne Hoehe — sichtbar als nichts, aber die Seite steht.
    if !node.style.font_size.is_finite() || node.style.font_size < 0.0 {
        return Err(invalid_length(
            node.id,
            format!(
                "font-size: {} muss endlich und >= 0 sein (0 ist gueltig, \
                 es ergibt einen Text ohne Hoehe)",
                node.style.font_size
            ),
        ));
    }
    Ok(())
}

/// Die benutzte Groesse eines ERSETZTEN Elements (CSS 2.1, 10.3.2).
///
/// `breite`/`hoehe` sind die gesetzten Werte, `None` = `auto`. `masse`
/// ist die intrinsische Masse (`Node::ersetzt`, P6).
///
/// # DIE REGEL, woertlich und in Reihenfolge
///
/// CSS 2.1, 10.3.2 fuer replaced elements, die kein Seitenverhaeltnis
/// aus einer `aspect-ratio`-Angabe bekommen:
///
/// 1. Sind Breite UND Hoehe nicht `auto`, sind die beiden benutzten
///    Werte einfach diese beiden.
/// 2. Ist die Breite `auto` und die Hoehe nicht, ist die benutzte Breite
///    "the intrinsic width times the ratio of the used height to the
///    intrinsic height".
/// 3. Ist die Hoehe `auto` und die Breite nicht, ist die benutzte Hoehe
///    dasselbe in der anderen Richtung.
/// 4. Sind beide `auto` und ist die Masse bekannt, gilt die Masse.
/// 5. Ist die Masse UNBEKANNT, bleibt es bei dem, was da war — die
///    Engine hat dann nichts, womit sie rechnen koennte, und raten waere
///    schlimmer als der bisherige Weg.
///
/// # GEMESSENER FALL (P6, 02.10.2026)
///
/// `https://www.nixos.org`:
/// `Node.ersetzt = (7129, 1995)`, `breite = None` (`.w-auto{width:auto}`),
/// `hoehe = Some(48)` (`.h-12{height:calc(var(--spacing) * 12)}`).
/// Zweig 2: `7129 * (48 / 1995) = 171.5` px. Vorher standen dort 7129.
///
/// # WARUM `None` ZURUECKKOMMT UND NICHT `Some(0.0)`
///
/// Ein Bild, das nicht geladen werden konnte, hat keine Masse. `Some(0.0)`
/// wuerde bedeuten "gemessen, und es ist nichts" — und wuerde die Seite
/// um eine unsichtbare Box kuerzen. Das ist ein Fehler, kein Ergebnis, und
/// `fehler.rs` wuerde ihn als `HART:nullhoehe` melden.
/// P12 (05.10.2026, 18:03): Soll die Masse ersetzter Elemente mitgedruckt
/// werden? Einmal gelesen und gemerkt (M1/M4).
fn ersatz_messen_an() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static GEPRUEFT: AtomicBool = AtomicBool::new(false);
    static AN: AtomicBool = AtomicBool::new(false);
    if !GEPRUEFT.load(Ordering::Relaxed) {
        let wert = std::env::var_os("NARSIL_ERSATZ_MESSEN")
            .map(|v| v == "1" || v == "ja" || v == "true")
            .unwrap_or(false);
        AN.store(wert, Ordering::Relaxed);
        GEPRUEFT.store(true, Ordering::Relaxed);
    }
    AN.load(Ordering::Relaxed)
}

pub(crate) fn ersetzte_groesse(
    masse: narsil_dom::ErsatzMasse,
    breite: Option<f32>,
    hoehe: Option<f32>,
) -> Option<(Option<f32>, Option<f32>)> {
    // Die gesetzten Werte muessen endlich und nicht negativ sein. Ein NaN
    // kommt aus einer kaputten Berechnung und darf hier nicht in ein
    // Verhaeltnis wandern.
    let ok = |v: Option<f32>| v.filter(|x| x.is_finite() && *x >= 0.0);
    let breite = ok(breite);
    let hoehe = ok(hoehe);
    match (breite, hoehe, masse.ist_vollstaendig()) {
        //
        // 4 und 5 (B11, 06.10.2026 01:12): BEIDES AUTO -> DIE MASSE GILT,
        // AUCH WENN SIE UNVOLLSTAENDIG IST.
        //
        // GEMESSENER FEHLER, github.com, 900 px (`NARSIL_BILD_MESSEN=1`,
        // 01:11, zwei Zeilen `BILD-LEER`):
        //
        //     BILD-LEER #839 ersatz=Some(ErsatzMasse { breite: None,
        //                  hoehe: Some(32.0) })
        //                  box=Some(Rect { x: 212.0, y: 6898.4,
        //                                   width: 318.0, height: **0.0** })
        //
        // Zwei Bilder auf 24, und beide **0 px hoch**, obwohl die Seite
        // ihre Hoehe kennt (32 px).
        //
        // WARUM: `ist_vollstaendig()` verlangt **beide** Seiten, und der
        // Zweig `false` gab `None` zurueck — also behielt die Engine
        // `height: auto`. Fuer einen `<img>` ist ein `<img>` ein
        // `NodeKind::Container` **ohne Kinder**, und im `auto`-Zweig wird
        // die Hoehe zur Summe der Kinder (`SizeTask::Sum`) — bei null
        // Kindern ist sie **0**. Der Platz, den die Seite fuer das Bild
        // reserviert hat, wurde weggeworfen.
        //
        // WAS CSS 2.1 §10.3.2 SAGT: sind `width` und `height` beide
        // `auto`, sind die benutzten Werte die intrinsischen. Ist nur eine
        // davon bekannt, ist **diese** die benutzte, und die andere bleibt
        // `auto` (der Kasten nimmt sie vom Container). „Nichts bekannt"
        // gilt nur fuer eine **leere** Masse, und die liefert
        // `Some((None, None))` — dieselbe Wirkung wie vorher, denn `None`
        // bedeutet `auto`.
        //
        // GEMESSENE GEFAHR, und sie ist geprueft: `masse.verhaeltnis()`
        // (Zeile 3 der Ratio-Aerme) bleibt unberuehrt — ein Verhaeltnis
        // aus einer halben Masse ist keine Zahl, und `?` gibt da weiter
        // `None`. Nur die Seite, die **bekannt** ist, wandert jetzt durch.
        (None, None, _) => Some((masse.breite, masse.hoehe)),
        // 2: Breite aus dem Verhaeltnis der benutzten Hoehe.
        (None, Some(h), true) => {
            let v = masse.verhaeltnis()?;
            Some((Some(v * h), Some(h)))
        }
        // 3: Hoehe aus dem Verhaeltnis der benutzten Breite.
        (Some(b), None, true) => {
            let v = masse.verhaeltnis()?;
            Some((Some(b), Some(b / v)))
        }
        // 1: beide gesetzt. Die Masse wird NICHT gebraucht — CSS 2.1, 10.3.2
        // sagt hier nichts von einem Verhaeltnis, und ein Bild darf
        // ausgedrueckt sein. (Das ist der Fall `<img width="100"
        // height="50">` ohne CSS: es bleibt 100x50.)
        //
        // BILD_21: DIE PRAESENTATIONSANGABE NAGELT NICHT BEIDE ACHSEN FEST.
        //
        // GEMESSEN (`tests/img_groesse_aus_der_kaskade.rs`): `<img width="7129"
        // height="1995">` mit `.gross{height:48px}` ergibt in Firefox einen
        // Kasten **171,5 x 48**, und bei uns **7129 x 48**. Grund: die Breite
        // kommt aus der **Praesentationsangabe** (7129), die Hoehe aus der
        // **CSS-Regel** (48). HTML 5 §5.9 macht die Attribute zu
        // Praesentationsangaben **und** zu einem `aspect-ratio: auto w/h` —
        // die Breite darf bei gesetzter CSS-Hoehe nicht festgenagelt bleiben.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `==` und kein Toleranzband: beide Zahlen
        // stammen aus **derselben** Quelle, dem `ErsatzMasse` desselben Knotens.
        // Ein Toleranzband wuerde auch `7129` und `7129.0001` als „gleich"
        // nehmen und damit eine Rechnung ausloesen, die es nicht gibt.
        //
        // **UND DER GEWICHTIGE FALL, der durchfaellt:** der HN-Spacer
        // `<img src="s.gif" width="1" height="1">` **ohne** CSS hat `h == masse.hoehe`
        // (1 == 1), greift also **nicht** hier und bleibt **1 x 1**.
        (b, h, _) if b.is_some() && h.is_some() => match (b, h, masse.breite, masse.hoehe) {
            (Some(bb), Some(hh), Some(mb), Some(mh)) if bb == mb && hh != mh => {
                // Nur die Hoehe ist eine **CSS**-Angabe; die Breite stammt aus
                // dem Attribut. Dann gilt das Seitenverhaeltnis der Quelle.
                match masse.verhaeltnis() {
                    Some(v) => Some((Some(hh * v), Some(hh))),
                    None => Some((b, h)),
                }
            }
            _ => Some((b, h)),
        },
        // 6: EINE Seite gesetzt, die Masse NICHT vollstaendig. Die
        // gesetzte Seite gilt unveraendert, die andere bleibt `None` —
        // NICHT 0. Eine erfundene 0 waere eine Behauptung ueber eine
        // Groesse, die wir nicht kennen, und wuerde im Befundmelder als
        // `HART:nullhoehe` auffallen.
        _ => Some((breite, hoehe)),
    }
}

#[cfg(test)]
mod b11_tests {
    //! B11 (06.10.2026, 01:14): die halb bekannte Masse.
    //!
    //! GEMESSENER BEFUND, github.com, 900 px, `NARSIL_BILD_MESSEN=1` (01:11):
    //!
    //! ```text
    //! BILD-LEER #839 ersatz=Some(ErsatzMasse { breite: None,
    //!              hoehe: Some(32.0) })
    //!              box=Some(Rect { x: 212.0, y: 6898.4,
    //!                           width: 318.0, height: 0.0 })
    //! BILD-SUMME img=24 mit_kasten=22 kasten_leer=2 in_liste=0
    //! ```
    //!
    //! Zwei Bilder mit **0 px Hoehe**, von denen die Seite die Hoehe
    //! kennt (32 px). `ist_vollstaendig()` verlangt beide Seiten, und der
    //! `false`-Zweig gab `None` zurueck — also blieb `height: auto`.
    //!
    //! WARUM DAS 0 PX WIRD: ein `<img>` ist ein `NodeKind::Container`
    //! **ohne Kinder**, und im `auto`-Zweig wird die Hoehe zur Summe der
    //! Kinder (`SizeTask::Sum`). Null Kinder, null Hoehe.
    //!
    //! WAS CSS 2.1 §10.3.2 SAGT: sind `width` und `height` beide `auto`,
    //! sind die benutzten Werte die intrinsischen; ist nur eine bekannt,
    //! ist diese die benutzte. „Nichts bekannt" gilt nur fuer eine LEERE
    //! Masse — und die liefert `Some((None, None))`, was `auto` bedeutet
    //! und damit genau das wie vorher.
    use super::ersetzte_groesse;
    use narsil_dom::ErsatzMasse;

    fn rechnung(
        masse: (Option<f32>, Option<f32>),
        breite: Option<f32>,
        hoehe: Option<f32>,
    ) -> (Option<f32>, Option<f32>) {
        let m = ErsatzMasse {
            breite: masse.0,
            hoehe: masse.1,
        };
        ersetzte_groesse(m, breite, hoehe).unwrap_or((None, None))
    }

    #[test]
    fn halbe_masse_der_hoehe_haelt_die_hoehe() {
        assert_eq!(
            rechnung((None, Some(32.0)), None, None),
            (None, Some(32.0)),
            "eine halb bekannte Masse gilt fuer die bekannte Seite; vorher \
             stand hier (None, None) und der Kasten wurde 0 px hoch"
        );
    }

    #[test]
    fn halbe_masse_der_breite_haelt_die_breite() {
        assert_eq!(
            rechnung((Some(64.0), None), None, None),
            (Some(64.0), None),
            "dieselbe Regel in der anderen Richtung"
        );
    }

    #[test]
    fn leere_masse_aendert_nichts() {
        assert_eq!(
            rechnung((None, None), None, None),
            (None, None),
            "eine leere Masse ist weiterhin 'nichts bekannt' — der \
             Unterschied zu vorher ist keiner, denn `None` heisst in \
             beiden Faellen `auto`"
        );
    }

    #[test]
    fn volle_masse_gilt_unveraendert() {
        // Der Fall seit dem 02.10.: das nixos-Logo 7129x1995.
        assert_eq!(
            rechnung((Some(7129.0), Some(1995.0)), None, None),
            (Some(7129.0), Some(1995.0))
        );
    }

    #[test]
    fn halbe_masse_und_gesetzte_breite_rechnet_kein_verhaeltnis() {
        // GEMESSENE GEFAHR, festgehalten: mit halber Masse gibt es KEIN
        // Seitenverhaeltnis, und es wird keines geraten.
        //
        // **MEIN TEST HAT MICH KORRIGIERT (01:25).** Ich hatte erwartet
        // `(Some(200), Some(32))` — also Breite gesetzt, Hoehe aus der
        // bekannten Masse. **Gemessen ist `(Some(200), None)`**, und das
        // ist richtig: der bestehende Zweig 6 (Zeile „EINE Seite gesetzt,
        // die Masse NICHT vollstaendig") sagt ausdruecklich, dass die
        // gesetzte Seite gilt und die andere `None` bleibt — NICHT 0, wie
        // sein Kommentar schreibt. Das ist auch nach CSS 2.1 §10.3.2 die
        // richtige Antwort: ohne Seitenverhaeltnis gibt es keine
        // benutzte Hoehe aus der Breite.
        //
        // Der Unterschied zu vorher: **vorher war die Masse in diesem
        // Zweig ueberhaupt nicht erreichbar** (siehe der Test mit leerer
        // Masse), und die Aenderung hat ihn nicht angefasst. Das ist hier
        // der Beleg, dass die Korrektur auf `(None, None, false)` begrenzt
        // ist und nicht auf die Ratio-Aerme.
        assert_eq!(
            rechnung((None, Some(32.0)), Some(200.0), None),
            (Some(200.0), None),
            "ohne Seitenverhaeltnis wird nicht gerechnet: die gesetzte \
             Breite gilt, die Hoehe bleibt auto"
        );
    }

    #[test]
    fn halbe_masse_und_gesetzte_hoehe_haelt_die_breite_auto() {
        // Die Umkehrung desselben Falls, und sie ist der Weg, ueber den
        // die github.com-Bilder kommen: Breite unbekannt, Hoehe gesetzt.
        assert_eq!(
            rechnung((None, None), Some(200.0), Some(50.0)),
            (Some(200.0), Some(50.0)),
            "beide Seiten gesetzt: die Masse ist nicht noetig (CSS 2.1 \
             10.3.2, der Zweib mit Attributen)"
        );
    }
}

/// Loest eine Laenge fuer `width`/`height` gegen `reference` auf.
///
/// `Auto` liefert `None` (die Groesse wird andersweitig bestimmt).
/// Prozentwerte ueber 100 sind erlaubt: Version 0 clippt nicht.
///
/// # Fehler
///
/// [`LayoutError::InvalidLength`] bei negativen, NaN oder unendlichen Werten.
pub(crate) fn resolve_length(
    len: Length,
    reference: f32,
    node: NodeId,
    property: &str,
) -> Result<Option<f32>, LayoutError> {
    match len {
        Length::Auto => Ok(None),
        Length::Px(v) => {
            if v.is_finite() && v >= 0.0 {
                Ok(Some(v))
            } else {
                Err(invalid_length(
                    node,
                    format!("{property}: {v} muss endlich und >= 0 sein"),
                ))
            }
        }
        // Release 11.10.2026: `calc()` mit Prozent — das Ergebnis wird bei 0
        // geklemmt (CSS Values 4 §10.12: eine negative Groesse ist 0).
        Length::Gemischt { px, prozent } if px.is_finite() && prozent.is_finite() => {
            Ok(Some(non_negative(px + reference * prozent / 100.0)))
        }
        Length::Gemischt { .. } => Err(invalid_length(node, format!("{property}: calc() muss endlich sein"))),
        Length::Percent(p) => {
            if p.is_finite() && p >= 0.0 {
                Ok(Some(reference * p / 100.0))
            } else {
                Err(invalid_length(
                    node,
                    format!("{property}: {p}% muss endlich und >= 0 sein"),
                ))
            }
        }
    }
}

/// Loest EINE Seitenlaenge von `margin` oder `padding` auf.
///
/// `Auto` ist fuer `margin`/`padding` in Version 0 unzulaessig (Spezifikation,
/// Fehler E15) und wird defensiv als 0 behandelt. Prozentwerte duerfen hier
/// 100% NICHT ueberschreiten.
///
/// # 15:56 — NEGATIVE MARGINS SIND ERLAUBT, NEGATIVE PADDINGS NICHT
///
/// `negativ_erlaubt` unterscheidet die beiden Faelle, und der Unterschied ist kein
/// Detail: **bis eben hat diese Funktion beide gleich behandelt und damit sechs von zehn
/// echten Webseiten unabmessbar gemacht.** CSS 2.1 8.3.1 laesst negative Margins
/// ausdruecklich zu — `margin-left: -1px` ist gewoehnliches CSS und kein Randfall —,
/// waehrend 8.3.2 negative Padding-Werte als ungueltig behandelt. `is_finite()` gilt fuer
/// beide: NaN und unendlich sind es, die hier abgelehnt werden sollen.
///
/// ***GEMESSEN, warum das keine Feinheit ist: `margin.top: -128` auf github.com,
/// `margin.left: -6` auf developer.mozilla.org, `margin.top: -40` auf debian.org. Die
/// Pruefung hat nicht einen Knoten bemerkt, sondern die ganze Seite verworfen — vor dem
/// Screenshot, also ohne dass es ueberhaupt eine Abweichung zu messen gaebe. Wer eine
/// Pruefung baut, die das Vorzeichen verlangt, sperrt nicht das rare Sonderzeichen aus,
/// sondern die Mehrheit des Netzes. `margin` ist ein VERSATZ und darf negativ sein;
/// `width`, `height` und `padding` sind GROESSEN und duerfen es nicht — dort unveraendert
/// `>= 0`, siehe [`resolve_length`].***
/// Die **Rahmenbreiten** als [`Edges`] — in derselben Reihenfolge wie
/// [`Edges`]: links, oben, rechts, unten.
///
/// **BILD_35, GEMESSENER BEFUND, WARUM ES DIE FUNKTION GAB:** in dieser Datei
/// wurde `padding` an **drei** Stellen addiert und abgezogen (248-250, 805, 885)
/// — und `border` an **keiner**. `box-sizing: border-box` war also fuer den
/// **Rahmen** nicht umgesetzt, nur fuer die **Polsterung**: `content =
/// border-box − padding − border` fehlte die **dritte** Kante.
///
/// **WARUM `gezeichnete_breite` UND NICHT `breite`:** CSS Backgrounds 3 §4.1 —
/// bei `border-style: none` **gilt die Breite nicht**. Ein Kasten mit
/// `border-width: 8px; border-style: none` hat also **keinen** Rahmen und darf
/// keinen Platz fuer ihn reservieren. `Rahmen::gezeichnete_breite` gibt genau
/// das zurueck (0 bei `none`).
///
/// **PROZENTE:** `border-width` in Prozent loest sich gegen die **Breite des
/// enthaltenden Kastens** (CSS 2.1 §10.3.2). **`Rahmen::breite` ist `f32` in px**
/// — der Prozentfall ist damit **nicht darstellbar**, und das ist eine Luecke
/// der Kaskade, nicht meine (siehe `Rahmen` in `narsil-core`).
pub(crate) fn rahmen_edges(rahmen: &narsil_core::style::Rahmen) -> Edges {
    Edges {
        left: rahmen.gezeichnete_breite(3),
        top: rahmen.gezeichnete_breite(0),
        right: rahmen.gezeichnete_breite(1),
        bottom: rahmen.gezeichnete_breite(2),
    }
}

pub(crate) fn resolve_spacing_length(
    len: Length,
    reference: f32,
    node: NodeId,
    property: &str,
    negativ_erlaubt: bool,
) -> Result<f32, LayoutError> {
    match len {
        Length::Auto => Ok(0.0),
        // Release 11.10.2026: `calc()` mit Prozent (Rand darf negativ sein,
        // Innenabstand wird bei 0 geklemmt).
        Length::Gemischt { px, prozent } if px.is_finite() && prozent.is_finite() => {
            let v = px + reference * prozent / 100.0;
            Ok(if negativ_erlaubt { v } else { non_negative(v) })
        }
        Length::Gemischt { .. } => Err(invalid_length(node, format!("{property}: calc() muss endlich sein"))),
        Length::Px(v) => {
            if v.is_finite() && (negativ_erlaubt || v >= 0.0) {
                Ok(v)
            } else {
                Err(invalid_length(
                    node,
                    if negativ_erlaubt {
                        format!("{property}: {v} muss endlich sein")
                    } else {
                        format!("{property}: {v} muss endlich und >= 0 sein")
                    },
                ))
            }
        }
        Length::Percent(p) => {
            // Release 08.10.2026: Prozent ueber 100 sind fuer `padding`
            // gueltig (CSS Box 3: nicht negativ, sonst frei) — der
            // Seitenverhaeltnis-Kniff `padding-top: 160%` ist verbreitet.
            // GEMESSEN vorher: amazon.de lud gar nicht („padding.top: 160%
            // muss zwischen 0 und 100 liegen").
            if p.is_finite() && (negativ_erlaubt || p >= 0.0) {
                Ok(reference * p / 100.0)
            } else {
                Err(invalid_length(
                    node,
                    if negativ_erlaubt {
                        format!("{property}: {p}% muss endlich sein")
                    } else {
                        format!("{property}: {p}% muss endlich und >= 0 sein")
                    },
                ))
            }
        }
    }
}

/// Loest `margin` oder `padding` (alle vier Seiten gleich) in [`Edges`].
///
/// Prozentwerte beziehen sich auf die BREITE der Content-Box des Elterns —
/// bei allen vier Seiten, wie in der Sprachspezifikation vereinbart.
///
/// 15:56 — **DIE ENTSCHEIDUNG FUELLT AN EINER STELLE, UND JEDE DER VIER SEITEN ERBT SIE.**
/// `property` ist hier `"margin"` oder `"padding"`; daraus wird einmal die Freigabe fuer
/// negative Werte abgeleitet. *An vier Aufrufen zu entscheiden waere vier Gelegenheiten,
/// es eine Seite falsch zu machen — und die Seiten einer Box sind dieselbe Aussage.*
pub(crate) fn resolve_spacing(
    spacing: Spacing,
    reference: f32,
    node: NodeId,
    property: &str,
) -> Result<Edges, LayoutError> {
    let negativ_erlaubt = property.starts_with("margin");
    // Release 15.10.2026: the side's name (`margin.left`) is formatted only
    // for an error. MEASURED before: four `format!` allocations per call, and
    // the intrinsic width passes call this for every child of every box they
    // visit (https://www.chip.de/, `messen`).
    let side = |len: Length, name: &str| {
        resolve_spacing_length(len, reference, node, property, negativ_erlaubt)
            .or_else(|_| resolve_spacing_length(len, reference, node, &format!("{property}.{name}"), negativ_erlaubt))
    };
    Ok(Edges::new(
        side(spacing.left, "left")?,
        side(spacing.top, "top")?,
        side(spacing.right, "right")?,
        side(spacing.bottom, "bottom")?,
    ))
}

/// Kind-Auftraege fuer die Messphase erzeugen.
///
/// `child_available` ist die FUER DIE MESSUNG angenommene Content-Box des
/// Containers: die Quer-Achse ist dort bereits bekannt, die Haupt-Achse faellt
/// auf den Raum zurueck, den dieser Container selbst von seinem Eltern bekommt
/// (Zirkulaeritaet: die eigene Content-Box auf der Haupt-Achse ist gerade die
/// Summe, die noch fehlt). In der Positionsphase ([`crate::flow::build_tree`])
/// wird die fertige Groesse uebernommen; weichen die Werte bei Prozenten auf
/// der Haupt-Achse ab, gilt die Messphase. Ueberlauf ist in Version 0 erlaubt.
fn push_children(
    stack: &mut Vec<SizeTask>,
    node: &Node,
    child_available: Size,
    after: Option<SizeTask>,
) {
    // Summe ZUERST anstossen: LIFO sorgt dann fuer "Kinder vor Summe".
    if let Some(task) = after {
        stack.push(task);
    }
    for &child in node.children.iter().rev() {
        stack.push(SizeTask::Enter {
            id: child,
            available: child_available,
        });
    }
}

/// Misst die Border-Box-Groesse EINES Knotens in der Enter-Phase.
///
/// Liefert die fertige Groesse (Quer- und/oder Haupt-Achse) sowie die
/// Kind-Auftraege, die noetig sind, um eine fehlende Haupt-Achse zu fuellen.
/// Release 15.10.2026: everything a node's measurement reads from outside its
/// subtree: the available size, the sizes forced on it (flex/grid
/// `flex_mass`, abspos and table cells), a resolved width keyword, whether its
/// own and its parent's height are definite and the parent's forced height
/// (for `height: P%` in flex and grid items, `LayoutStil::hoehe_bedingt`)
/// and the floats it inherits.
/// Equal inputs give an equal result, so a repeated `Enter` (the stages of a
/// flex or grid container re-measure their items; nested containers multiply
/// that) can keep the earlier result. MEASURED before: https://www.chip.de/
/// (nested flex navigation lists) entered 3811 nodes 19484 times per layout,
/// `messen` 550-650 ms instead of 150-300 ms on the 10.10. build.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MessSignatur {
    verfuegbar: (u32, u32),
    flex_mass: Option<(Option<u32>, Option<u32>)>,
    erzwungen: Option<u32>,
    erzwungen_breite: Option<u32>,
    breite_wort: Option<(u32, [Option<u32>; 3])>,
    definit: bool,
    eltern_definit: bool,
    eltern_mass: Option<(Option<u32>, Option<u32>)>,
    /// The floats of the enclosing formatting context a box without its own
    /// context sees (`Ctx::float_erbe`, set by the parent's `sum_children`).
    float_erbe: Option<crate::plan::FloatZustand>,
}

fn mess_signatur(sizes: &Sizes, ctx: &Ctx<'_>, id: NodeId, node: &Node, available: Size) -> MessSignatur {
    let bits = |v: Option<f32>| v.map(f32::to_bits);
    let mass = |k: NodeId| ctx.flex_mass.borrow().get(&k).map(|m| (bits(m.0), bits(m.1)));
    let bedingt = ctx.stil(id).is_some_and(|s| s.hoehe_bedingt.is_some());
    MessSignatur {
        verfuegbar: (available.width.to_bits(), available.height.to_bits()),
        flex_mass: mass(id),
        erzwungen: sizes.erzwungen.get(&id.0).map(|v| v.to_bits()),
        erzwungen_breite: sizes.erzwungen_breite.get(&id.0).map(|v| v.to_bits()),
        breite_wort: sizes.breite_wort.get(&id.0).map(|g| (g.0.to_bits(), [bits(g.1), bits(g.2), bits(g.3)])),
        // Only read by `height: P%` in flex and grid items (`hoehe_bedingt`):
        // the node's own definiteness for its children, its parent's for
        // itself.
        definit: node.children.iter().any(|&c| ctx.stil(c).is_some_and(|s| s.hoehe_bedingt.is_some()))
            && ctx.hoehe_definit.borrow().contains(&id),
        eltern_definit: bedingt && node.parent.is_some_and(|e| ctx.hoehe_definit.borrow().contains(&e)),
        eltern_mass: if bedingt { node.parent.and_then(mass) } else { None },
        float_erbe: ctx.float_erbe.borrow().get(&id).copied(),
    }
}

fn enter(
    stack: &mut Vec<SizeTask>,
    sizes: &mut Sizes,
    ctx: &Ctx<'_>,
    id: NodeId,
    available: Size,
) -> Result<(), LayoutError> {
    let node = get_node(ctx, id)?;
    // Release 10.10.2026: ein `fixed`-Kasten misst gegen den VIEWPORT, nicht
    // gegen den Inhalt seines Elternteils. GEMESSEN vorher (heise.de): die
    // Fussleiste `<a-sticky-footer>` (fixed, left/right 0) lag in einem
    // fixen 1x1-px-Kasten und wurde 1 px schmal — ein senkrechter Streifen
    // mitten auf der Seite, ein Zeichen je Zeile.
    let available = if sizes.fenster.width > 0.0
        && matches!(ctx.stil_oder_standard(id).position, narsil_layout::PositionArt::Fest)
    {
        sizes.fenster
    } else {
        available
    };
    if let Some(v) = sizes.verfuegbar.get_mut(id.index()) {
        *v = available;
    }
    if let (Some(slot), Some(e)) = (sizes.eltern.get_mut(id.index()), node.parent) {
        *slot = e.0;
    }
    // Release 15.10.2026: the same inputs as the last finished measurement:
    // keep its result (see `MessSignatur`). Not for text runs (their widths
    // are adjusted by the line box, `laeufe_als_kette`) and table cells (the
    // row phase sets their size).
    let memo = node.kind == NodeKind::Container
        && !ctx.plan.is_some_and(|p| p.tabelle(id).rolle == narsil_layout::plan::TabellenRolle::Zelle);
    if memo {
        let sig = mess_signatur(sizes, ctx, id, node, available);
        if sizes.messung_fertig.get(&id.0) == Some(&sig) && sizes.get(id).is_some() {
            return Ok(());
        }
        sizes.messung_fertig.remove(&id.0);
        sizes.messung_offen.insert(id.0, sig);
    }
    //
    // P5 (02.10.2026): EIN KNOTEN DARF NICHT DIE SEITE KIPPEN.
    //
    // GEMESSENE REGRESSION: `https://archlinux.org/` liess sich mit der
    // Bruecke aus `WUNSCH_LAYOUT.txt` nicht mehr laden —
    //     Layout: Ungueltige Laenge bei Knoten NodeId(9)
    // Ein einziger Knoten mit einer kaputten Angabe hat die ganze Seite
    // verworfen. Das ist die falsche Antwort auf die Frage "was ist, wenn
    // eine Angabe unsinnig ist": die Antwort ist NICHT "nichts", sondern
    // "dieser Kasten bekommt den Standardwert, der Rest steht".
    //
    // CSS 2.1, 10.1 sagt zu ungueltigen Werten ausdruecklich: "User
    // agents must ignore the declaration" — die Erklaerung fällt weg, die
    // Seite NICHT.
    //
    // M7: `verworfene_knoten` zaehlt mit, was passiert ist. Ohne diese
    // Zahl waere ein stillschweigend repariertes Stylesheet nicht von
    // einem sauberen zu unterscheiden.
    // Der Stil, mit dem gerechnet wird. Bei einem kaputten Wert der
    // STANDARD des Projekts — ein Block ohne Rand, ohne Fuellung, mit
    // 16 px Schrift (CSS 2.1, 10.1: die ERKLAERUNG faellt weg, nicht die
    // Seite).
    //
    // `OnceLock`, weil `Style` keinen `const`-Konstruktor hat und ein
    // `Style::default()` je Knoten eine Allokation waere (bei 50 000
    // Knoten 50 000 Allokationen fuer einen Wert, der fast nie gebraucht
    // wird — M2).
    static STANDARD_STIL: std::sync::OnceLock<narsil_core::style::Style> = std::sync::OnceLock::new();
    let standard = STANDARD_STIL.get_or_init(narsil_core::style::Style::default);
    let mut stil_fehlerhaft = false;
    if let Err(e) = validate_style(node) {
        let verworfene = &mut sizes.verworfene;
        stil_fehlerhaft = true;
        verworfene.push((id, e.to_string()));
    }
    let stil: &narsil_core::style::Style = if stil_fehlerhaft {
        standard
    } else {
        &node.style
    };

    let margin = resolve_spacing(stil.margin, available.width, id, "margin")?;
    let mut padding = resolve_spacing(stil.padding, available.width, id, "padding")?;
    let rahmen = rahmen_edges(&stil.rahmen);
    padding.left += rahmen.left;
    padding.right += rahmen.right;
    padding.top += rahmen.top;
    padding.bottom += rahmen.bottom;
    let quer_abstand = padding.left + padding.right;
    let hoch_abstand = padding.top + padding.bottom;
    let border_box = ctx.stil(id).is_some_and(|p| p.border_box);
    let inhalt_wert = |v: f32, abstand: f32| {
        non_negative(if border_box { v - abstand } else { v })
    };
    let (min_b, max_b, min_h, max_h) = grenzen(ctx, id);
    let (pm_b, pm_a, pm_h, pm_o) = grenzen_prozent(ctx, id, available.width, available.height);
    let (min_b, max_b, min_h, max_h) = (
        cmp_or(pm_b, min_b).map(|v| inhalt_wert(v, quer_abstand)),
        cmp_or(pm_a, max_b).map(|v| inhalt_wert(v, quer_abstand)),
        cmp_or(pm_h, min_h).map(|v| inhalt_wert(v, hoch_abstand)),
        cmp_or(pm_o, max_h).map(|v| inhalt_wert(v, hoch_abstand)),
    );
    // Release 11.10.2026: `width`/`min-width`/`max-width` als
    // Schluesselwort (CSS Sizing 3 §2.1). Erst wird wie bei `auto` gemessen,
    // danach (`SizeTask::BreitenWort`) aus dem Inhalt bestimmt und der Knoten
    // mit den Breiten neu gemessen. GEMESSEN vorher: die Schluesselwoerter
    // fielen weg (WPT: 384 Raster-Tests nutzen die Klassen `min-content` usw.).
    let worte = if node.kind == NodeKind::Container { ctx.stil(id).map_or([0; 4], |s| s.groessen_art) } else { [0; 4] };
    let hat_wort = worte[0] != 0 || worte[1] != 0 || worte[2] != 0;
    let gemerkt = sizes.breite_wort.get(&id.0).copied().filter(|g| hat_wort && (g.0 - available.width).abs() < 0.5);
    if hat_wort && gemerkt.is_none() {
        stack.push(SizeTask::BreitenWort { id, available });
    }
    let (min_b, max_b) = match gemerkt {
        Some((_, _, mn, mx)) => (mn.or(min_b), mx.or(max_b)),
        None => (min_b, max_b),
    };
    // Release 14.10.2026: `stretch` (CSS Sizing 4 §3.1,
    // `LayoutStil::stretch_bits`): the margin box fills the containing block,
    // auto margins count as 0; an absolutely positioned box loses its insets
    // (both `auto`: from the static position, here the parent's padding when
    // the parent is the containing block). The page already cleared the
    // block-axis bits against an indefinite height. MEASURED before: WPT
    // css-sizing `stretch/stretch-block-size-001` (`block-size: stretch`
    // stayed `auto`).
    let stretch_bits = if node.kind == NodeKind::Container { ctx.stil(id).map_or(0, |s| s.stretch_bits) } else { 0 };
    let (stretch_w, stretch_h) = if stretch_bits == 0 {
        (0.0, None)
    } else {
        let (vb, vh) = stretch_insets(ctx, id, node, available.width);
        (
            non_negative(available.width - margin.left - margin.right - vb - quer_abstand),
            available.height.is_finite().then(|| non_negative(available.height - margin.top - margin.bottom - vh - hoch_abstand)),
        )
    };
    let min_b = if stretch_bits & 2 != 0 { Some(stretch_w) } else { min_b };
    let max_b = if stretch_bits & 4 != 0 { Some(stretch_w) } else { max_b };
    let min_h = if stretch_bits & 16 != 0 { stretch_h.or(min_h) } else { min_h };
    let max_h = if stretch_bits & 32 != 0 { stretch_h.or(max_h) } else { max_h };
    let explicit_width = clamp_length(
        resolve_length(stil.size.width, available.width, id, "width")?.map(|v| inhalt_wert(v, quer_abstand)),
        min_b, max_b,
    );
    let explicit_width = if stretch_bits & 1 != 0 && gemerkt.is_none() { clamp_length(Some(stretch_w), min_b, max_b) } else { explicit_width };
    // Release 14.10.2026: the fixed width of a line box of the page builder
    // is only its estimated paragraph width (see `ist_zeilenbox`); in a
    // narrower containing block (a grid or flex item with an intrinsic
    // width) it is the available width, so `text-align` and wrapping follow
    // the real box. MEASURED before: WPT html/canvas
    // `compositing/2d.composite.grid.*` (in a 125 px max-content grid cell
    // the centred label stood in an 800 px line box, at x = 337).
    let explicit_width = match explicit_width {
        Some(w) if matches!(stil.size.width, Length::Px(_)) && available.width.is_finite() && ist_zeilenbox(ctx, id, node) => {
            let platz = non_negative(available.width - margin.left - margin.right - quer_abstand);
            Some(w.min(platz))
        }
        w => w,
    };
    let explicit_width = match gemerkt {
        Some((_, Some(w), _, _)) => clamp_length(Some(w), min_b, max_b),
        _ => explicit_width,
    };
    let explicit_height = clamp_length(
        resolve_length(stil.size.height, available.height, id, "height")?.map(|v| inhalt_wert(v, hoch_abstand)),
        min_h, max_h,
    );
    let explicit_height = match stretch_h {
        Some(h) if stretch_bits & 8 != 0 => clamp_length(Some(h), min_h, max_h),
        _ => explicit_height,
    };
    // Release 14.10.2026: size containment in the block axis (CSS Contain 2
    // §3.2): `height: auto` is the height from `contain-intrinsic-size` (else
    // 0), the contents do not count. MEASURED before: WPT css-sizing
    // `contain-intrinsic-size/contain-intrinsic-size-001` (0 instead of 222 px).
    let explicit_height = match ctx.stil(id).filter(|s| s.size_contained & 2 != 0 && node.kind == NodeKind::Container && node.ersetzt.is_none()) {
        Some(s) if explicit_height.is_none() => clamp_length(Some(s.contain_size[1].unwrap_or(0.0)), min_h, max_h),
        _ => explicit_height,
    };
    // Release 11.10.2026: `height: P%` im Flex-/Rasterelement, sobald dessen
    // Hoehe definit gesetzt ist (`LayoutStil::hoehe_bedingt`,
    // `Ctx::hoehe_definit`; `available.height` ist dann seine Inhaltshoehe).
    // GEMESSEN vorher: WPT css-flexbox `percentage-heights-001` (in einem
    // gedehnten Element blieb `height: 50%` auto, 50 statt 25 px) und
    // css-grid `grid-lanes/alignment/column-align-items-*`.
    let explicit_height = match (explicit_height, ctx.stil(id).and_then(|s| s.hoehe_bedingt)) {
        (None, Some(p))
            if available.height.is_finite()
                && node.parent.is_some_and(|e| {
                    ctx.hoehe_definit.borrow().contains(&e)
                        && ctx.flex_mass.borrow().get(&e).is_some_and(|m| m.1.is_some())
                        // Ein Rasterelement gilt gegen seinen Bereich (`gitter.rs`,
                        // Teil 2), nicht gegen das Raster.
                        && ctx.stil(e).is_none_or(|s| s.anzeige != narsil_layout::plan::Anzeige::Raster)
                }) =>
        {
            clamp_length(Some(inhalt_wert(non_negative(available.height) * p / 100.0, hoch_abstand)), min_h, max_h)
        }
        (h, _) => h,
    };
    // Release 10.10.2026: eine erzwungene Hoehe (siehe `absolute_nachmessen`).
    let explicit_height = match sizes.erzwungen.get(&id.0) {
        Some(h) => Some(*h),
        None => explicit_height,
    };
    // Release 11.10.2026 (Agent CSS2): die Zellbreite aus dem Tabellen-Layout
    // (eine Rahmenbreite; hier ist `explicit_width` noch die Inhaltsbreite).
    let explicit_width = match sizes.erzwungen_breite.get(&id.0) {
        Some(w) => Some(non_negative(*w - quer_abstand)),
        None => explicit_width,
    };
    // Release 11.10.2026: die benutzte Groesse eines Flex-Elements nach dem
    // Flex-Algorithmus (`flexbox.rs`, Rahmenmasse; hier Inhaltsmasse).
    let (explicit_width, explicit_height) = match ctx.flex_mass.borrow().get(&id) {
        Some(&(w, h)) => (
            w.map(|w| non_negative(w - quer_abstand)).or(explicit_width),
            h.map(|h| non_negative(h - hoch_abstand)).or(explicit_height),
        ),
        None => (explicit_width, explicit_height),
    };
    let quer = |w: f32| {
        clamp(non_negative(w - quer_abstand), min_b, max_b) + quer_abstand
    };
    let hoch = |h: f32| {
        clamp(non_negative(h - hoch_abstand), min_h, max_h) + hoch_abstand
    };

    //
    // P6 (02.10.2026): ERSETZTE ELEMENTE — CSS 2.1, 10.3.2.
    //
    // GEMESSENER FEHLER, den das behebt: `https://www.nixos.org`, Knoten
    // 10, Kasten 7129 x 1995, LEER, `node.style.size.width == Px(7129)`.
    // Woertlich im Quelltext der Seite:
    //     <img src="/_astro/nixos-logo-...svg" width="7129"
    //          height="1995" class="h-12 w-auto">
    // Die Seite schreibt `.w-auto{width:auto}` und `.h-12{height:3rem}`,
    // also 48 px. Ein Browser rechnet nach CSS 2.1, 10.3.2:
    //
    //     "If 'width' is 'auto' and 'height' is not 'auto', the used value
    //      of 'width' is the intrinsic width times the ratio of the used
    //      height to the intrinsic height."
    //
    //     7129 * (48 / 1995) = 171 px
    //
    // Die Regel ist dreifach verzweigt, und alle drei Zweige stehen hier,
    // weil die anderen beiden genauso selten vorkommen und genauso
    // falsch waeren:
    //
    //   Breite UND Hoehe nicht auto -> die beiden benutzten Werte. Das
    //     Attribut traegt in diesem Fall die Breite, und das ist richtig.
    //   Breite auto, Hoehe nicht      -> Breite = Masse * (Hoehe/intr.Hoehe)
    //     das ist der gemeldete Fall.
    //   Hoehe auto, Breite nicht      -> Hoehe = Masse * (Breite/intr.Breite)
    //   beide auto                     -> die intrinsische Masse selbst.
    //
    // WARUM DAS HIER UND NICHT IM TEXT: die Masse gehoert zum KNOTEN
    // (`Node::ersetzt`, narsil-dom, P6-Freigabe), und die beiden
    // gesetzten Werte sind `stil.size`. Beides ist hier schon da.
    //
    // `min-*`/`max-*` greifen ZUR NACHT: `clamp_length` ist oben schon
    // gelaufen, aber es hat die(auto) Masse gar nicht gesehen — sie ist
    // ja erst hier entstanden. Deshalb wird hier noch einmal geholt und
    // geklemmt. Das ist zweimal geklemmt, und das ist richtig: die erste
    // Klammer gilt fuer die gesetzte Angabe, die zweite fuer die aus dem
    // Verhaeltnis.
    //
    // P12 (14:52): WIE WAR DIE HOEHE VOR `ersetzte_groesse`?
    //
    // GEMESSENER FEHLER IM ERSTEN VERSUCH (14:51): `ersetzte_groesse`
    // liefert im Fall "beide auto" die ERSATZmasse zurueck — als
    // `Some(1079)`, nicht als `None`. Mein Muster `(breite, None,
    // verhaeltnis)` griff darum nie, und der Test schlug fehl
    // (`1079.00 statt 505.78`). Die Frage lautet nicht "ist die Hoehe
    // bekannt", sondern "war die Hoehe GESETZT".
    // Release 14.10.2026: `min-height`/`max-height` as a content keyword on a
    // replaced box: its content height is the height transferred from the
    // width through its ratio, else the natural height (CSS Sizing 3 §5.1).
    // MEASURED before: WPT css-sizing `replaced-min-height-min-content`
    // (`width: 100px; height: 10px; min-height: min-content` on a 10 x 10
    // canvas stayed 10 px high).
    let (min_h, max_h) = match (node.ersetzt, ctx.stil(id).map_or(0, |s| s.height_keyword_limits)) {
        (Some(m), bits) if bits != 0 => {
            let natur = match (explicit_width, wirksames_verhaeltnis(ctx, id, node)) {
                (Some(w), Some(r)) if r > 0.0 => Some(w / r),
                _ => m.hoehe,
            };
            (
                if bits & 1 != 0 { natur.or(min_h) } else { min_h },
                if bits & 2 != 0 { natur.or(max_h) } else { max_h },
            )
        }
        _ => (min_h, max_h),
    };
    let explicit_height = match (node.ersetzt.is_some(), explicit_height) {
        (true, Some(h)) => clamp_length(Some(h), min_h, max_h),
        (_, h) => h,
    };
    // Release 14.10.2026: an absolutely positioned REPLACED box with an
    // explicit `justify-self`/`align-self: stretch` and both insets fills the
    // inset-modified containing block in that axis (CSS Box Alignment 3 §6.1,
    // CSS Position 3 §5.2: `normal` does not stretch replaced boxes, `stretch`
    // does). MEASURED before: WPT css-align `abspos/stretch-intrinsic-size-
    // htb-htb` (a 10 x 10 canvas with `justify-self: stretch` stayed 10 px).
    let (explicit_width, explicit_height) = match (node.ersetzt.is_some(), ctx.stil(id)) {
        (true, Some(st)) => {
            let (sw, sh) = abs_explicit_stretch(ctx, id);
            let w = match (sw, st.links, st.rechts) {
                (true, Some(l), Some(r)) if explicit_width.is_none() && available.width.is_finite() => {
                    clamp_length(Some(non_negative(available.width - l - r - margin.left - margin.right - quer_abstand)), min_b, max_b)
                }
                _ => explicit_width,
            };
            let h = match (sh, st.oben, st.unten) {
                (true, Some(o), Some(u)) if explicit_height.is_none() && available.height.is_finite() => {
                    clamp_length(Some(non_negative(available.height - o - u - margin.top - margin.bottom - hoch_abstand)), min_h, max_h)
                }
                _ => explicit_height,
            };
            (w, h)
        }
        _ => (explicit_width, explicit_height),
    };
    let hoehe_gesetzt = explicit_height.is_some();
    let breite_gesetzt = explicit_width.is_some();
    // Release 11.10.2026: negativ = `auto && <ratio>` — ein ersetztes Element
    // mit natuerlichem Verhaeltnis nimmt dann dieses (CSS Sizing 4 §5.1).
    let css_verhaeltnis = ctx.stil(id).map(|s| s.seitenverhaeltnis).and_then(|v| {
        if !v.is_finite() || v == 0.0 {
            None
        } else if v > 0.0 {
            Some(v)
        } else if node.ersetzt.and_then(|m| m.verhaeltnis()).is_some() {
            None
        } else {
            Some(-v)
        }
    });
    let (explicit_width, explicit_height) = match node
        .ersetzt
        .and_then(|m| ersetzte_groesse(m, explicit_width, explicit_height))
    {
        //
        // P12 (05.10.2026, 13:45): DIE KLEMME GEHOERT AUCH HIERHER.
        //
        // GEMESSEN, debian.org (gespeichertes Dokument, `ZUBREIT` mit
        // Eltern):
        //
        //   ZUBREIT Hart 124 px knoten 51 kasten=[124,859.40 1024.00x751.00]
        //     ELTERN #47 [120,770.60 660.00x843.80] kinder=2  plan=Block
        //   ZUBREIT Hart 382 px knoten 66 kasten=[124,3111.80 1282.00x777.00]
        //
        // 1024 px in einer 660-px-Spalte, fuenfmal dieselbe Zahl. Der
        // Kasten traegt `ersetzte` Masse (ein `<img>`), also kommt die
        // Breite aus `ersetzte_groesse` — und die kam OHNE KLEMME in
        // den Baum.
        //
        // CSS 2.1, 10.4: "used width = max(min-width, min(width,
        // max-width))". Das gilt fuer die BENUTZTE Breite, egal woher sie
        // kommt. `clamp_length` oben hat nur die ERKLARTE Angabe geklemmt;
        // die aus dem Verhaeltnis und die aus den Ersatzmassen kamen
        // ungeklemmt durch. Der Kommentar zwei Absaetze oberhalb sagt "min-*
        // /max-* greifen ZUR NACHT" — zur Nacht ja, aber nur fuer den
        // Sum-Zweig, nicht fuer diesen.
        Some((b, h)) => {
            //
            // P12 (05.10.2026, 14:51): `max-width` AENDERT AUCH DIE
            // HOEHE, wenn `height: auto` ist.
            //
            // GEMESSEN, heise.de, Kette von #3250 (14:50):
            //
            //   [0] #3250 [120.00,270558.94 900.00x1079.00] height=Auto
            //         grenzen=Grenzen { max_breite: Some(900.0), … }
            //         ersatz=Some(Some(ErsatzMasse { breite: Some(1920.0),
            //                                          hoehe: Some(1079.0) }))
            //         kinder=0
            //
            // Ein BILD mit der natuerlichen Groesse 1920 x 1079 und
            // `max-width: 900`. Die Breite wurde auf 900 gekappt — die
            // Hoehe blieb bei 1079. Bei 900 px Breite gehoert aber eine
            // Hoehe von 900 / (1920/1079) = 505,9 px dazu.
            //
            // CSS 2.1 §10.4, die Tabelle der Randbedingungen: eine
            // Verletzung von `max-width` bei `width != auto` heisst
            // "recalculate the height using the 'width' and the
            // intrinsic ratio", sofern `height: auto` ist.
            //
            // DIE WIRKUNG WAR ENORM: 15 Luecken ueber 3 000 px auf
            // heise.de, eine Seite von 294 000 px statt rund 4 000.
            //
            // `min_breite` wirkt genauso (es vergroessert die Breite),
            // und `min_hoehe`/`max_hoehe` greifen danach wie bisher.
            let verhaeltnis =
                css_verhaeltnis.or_else(|| node.ersetzt.as_ref().and_then(|m| m.verhaeltnis()));
            let breite = if !breite_gesetzt && hoehe_gesetzt {
                match (h, css_verhaeltnis) {
                    (Some(h), Some(v)) => Some(h * v),
                    _ => b,
                }
            } else {
                b
            };
            // L4 (03.10.2026 23:1x): das zweite `clamp` ist weg. `quer`
            // klemmt seit der Aenderung oben gegen den INHALTSkasten und
            // addiert die Fuellung wieder; ein `clamp` darueber herum
            // wuerde die Rahmenbreite klemmen und denselben Fehler
            // zurueckbringen.
            // Release 11.10.2026 (Agent CSS2): `b`/`h` sind hier schon
            // INHALTSmasse (`explicit_*` und die natuerliche Groesse); die
            // Fuellung kommt erst unten (P12) dazu. `quer`/`hoch` erwarten
            // die Rahmenbox und blaehten ein Bild, das schmaler ist als
            // seine Fuellung, auf die Fuellung auf. GEMESSEN vorher (WPT-
            // Referenzen `border-applies-to-*-ref`: `<img width=4
            // style="padding-left:96px">`): 192 statt 100 px breit, das
            // zweite Bild brach in die naechste Zeile.
            let breite_gekappt = breite.map(|w| clamp(non_negative(w), min_b, max_b));
            let hoehe = if !hoehe_gesetzt {
                // `height: auto` — die Hoehe folgt der (gekappten)
                // Breite, solange es ein Verhaeltnis gibt.
                match (breite_gekappt, verhaeltnis) {
                    (Some(nb), Some(v)) if v > 0.0 => Some(nb / v),
                    _ => h.map(|h| clamp(non_negative(h), min_h, max_h)),
                }
            } else {
                h.map(|h| clamp(non_negative(h), min_h, max_h))
            };
            // Release 14.10.2026: with both sizes `auto`, a height raised by
            // `min-height` makes the width follow through the ratio (CSS 2.1
            // §10.4, the constraint table; `max-height` keeps the old way).
            // MEASURED before: WPT css-sizing
            // `stretch/replaced-stretch-height-001` (a 50 x 50 canvas with
            // `min-height: stretch` of 200 px stayed 50 px wide).
            match (breite_gekappt, hoehe, verhaeltnis) {
                (Some(w), Some(hh), Some(v)) if !breite_gesetzt && !hoehe_gesetzt && v > 0.0 && hh.is_finite() => {
                    let hc = clamp(non_negative(hh), min_h, max_h);
                    if hc > hh + 0.01 {
                        (Some(clamp(non_negative(hc * v), min_b, max_b)), Some(hc))
                    } else {
                        (Some(w), Some(hh))
                    }
                }
                _ => (breite_gekappt, hoehe),
            }
        }
        //
        // L4 (04.10.2026 03:0x): `aspect-ratio` FUELLT AUCH DIE HOEHE —
        // und es fehlt nur hier.
        //
        // GEMESSENER FEHLER (`CSS_14.txt`, f4): `<div style="width:200px;
        // aspect-ratio:2/1; padding:25px">` ergibt bei uns den Rahmen
        // 250 x **50**, in Firefox 250 x **150**. GEMESSEN in vier Varianten —
        // `height` fehlt, `height:auto`, `height:0`, alles zusammen — und
        // **alle vier** geben 50. Es ist also nicht die Schreibweise von
        // `height`, sondern: der ganze Verhaeltnis-Zweig liegt in diesem
        // `match` im **Ersetzungs-Element**-Zweig, und ein `<div>` hat
        // `node.ersetzt == None` — er kam nie dort vorbei.
        //
        // §10.4 (CSS Sizing 4 §4.1): das Verhaeltnis gilt fuer den
        // INHALTSkasten und liefert die fehlende Achse, **wenn diese
        // `auto` ist**. Beide Bedingungen sind hier erfuellt: die Breite ist
        // gesetzt, die Hoehe nicht. Also `hoehe = breite / verhaeltnis`.
        None => {
        // Release 12.10.2026: CSS Sizing 4 §5 — das Verhaeltnis gilt fuer den
        // `box-sizing`-Kasten (bei `auto && <ratio>` fuer den Inhalt); mit
        // definiter Hoehe folgt eine `auto`-Breite dem Verhaeltnis, und ein
        // Block im Fluss mit `auto`-Breite fuellt erst und bekommt dann die
        // Hoehe. GEMESSEN vorher: WPT css-sizing
        // `aspect-ratio/block-aspect-ratio-002/-008/-004` (die Breite blieb
        // die Fuellbreite bzw. die Hoehe 0, `padding` zaehlte nicht).
        let auf_rahmen = border_box && !ctx.stil(id).is_some_and(|s| s.seitenverhaeltnis < 0.0);
        let (qa, ha) = if auf_rahmen { (quer_abstand, hoch_abstand) } else { (0.0, 0.0) };
        let fuellt = ctx.stil(id).is_some_and(|s| {
            !matches!(s.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)
                && s.schweben == narsil_layout::plan::FloatArt::Keins
                && !inline_ebene(s)
                && matches!(s.anzeige, narsil_layout::plan::Anzeige::Block | narsil_layout::plan::Anzeige::Flex | narsil_layout::plan::Anzeige::Raster)
        }) && node.parent.and_then(|e| ctx.stil(e)).is_none_or(|e| {
            !matches!(e.anzeige, narsil_layout::plan::Anzeige::Flex | narsil_layout::plan::Anzeige::Raster)
        }) && node.ersetzt.is_none();
        let explicit_width = match (explicit_width, explicit_height, css_verhaeltnis) {
            (None, Some(bh), Some(v)) if v > 0.0 && bh.is_finite() && node.ersetzt.is_none() => {
                // Ohne `min-width` (und mit sichtbarem Ueberlauf) ist die
                // automatische Mindestbreite die min-content-Breite (CSS Sizing
                // 4 §5.2). GEMESSEN vorher: WPT css-sizing
                // `aspect-ratio/abspos-013` (50 statt 100 px).
                let w = non_negative((bh + ha) * v - qa);
                let w = if min_b.is_none() && !ctx.stil(id).is_some_and(|s| s.rollkasten) { w.max(vorab_min_inhalt(ctx, node)) } else { w };
                clamp_length(Some(w), min_b, max_b)
            }
            (None, None, Some(v)) if v > 0.0 && fuellt && available.width.is_finite() => {
                // Release 14.10.2026: `min-height`/`max-height` limit the
                // fill width through the ratio (CSS Sizing 4 §5.1). MEASURED
                // before: WPT css-sizing `aspect-ratio/block-aspect-ratio-020`
                // (`max-height: 100%` of 100 px: 800 instead of 100 px wide).
                let transfer = |h: f32| non_negative((h + ha) * v - qa);
                let max_t = max_h.filter(|h| h.is_finite()).map(transfer);
                let min_t = min_h.filter(|h| h.is_finite()).map(transfer);
                let w = clamp_length(Some(non_negative(available.width - margin.left - margin.right - quer_abstand)), min_b, max_b);
                let w = match max_t {
                    Some(m) => w.map(|w| w.min(m)),
                    None => w,
                };
                let w = match min_t {
                    Some(m) => w.map(|w| w.max(m)),
                    None => w,
                };
                // The automatic minimum is still the min-content width.
                if min_b.is_none() && !ctx.stil(id).is_some_and(|s| s.rollkasten) { w.map(|w| w.max(vorab_min_inhalt(ctx, node))) } else { w }
            }
            (w, _, _) => w,
        };
        match (explicit_height, explicit_width, css_verhaeltnis) {
            (None, Some(bw), Some(v)) if v > 0.0 && bw.is_finite() && bw >= 0.0 => {
                // Release 11.10.2026: ohne `min-height` und mit sichtbarem
                // Ueberlauf waechst der Kasten mit seinem Inhalt (CSS Sizing 4
                // §5.2). GEMESSEN vorher: WPT css-grid
                // `alignment/grid-content-distribution-029` (ein Raster mit
                // `aspect-ratio: 2` blieb 25 px hoch, die 100-px-Zeile ragte
                // hinaus).
                if min_h.is_none() && !ctx.stil(id).is_some_and(|s| s.rollkasten) {
                    sizes.verhaeltnis_wachsend.insert(id.0);
                }
                (Some(bw), clamp_length(Some(non_negative((bw + qa) / v - ha)), min_h, max_h))
            }
            _ => (explicit_width, explicit_height),
        }
        }
    };

    //
    // P12 (05.10.2026, 18:45): `width`/`height` SIND INHALTSMASSE.
    //
    // CSS 2.1 §10.3.3 und §10.5, woertlich: "the value is the width of
    // the content area" und "the used value of 'height' is the value of
    // 'height' plus the border and padding". §10.1 nennt die
    // Voreinstellung: `box-sizing: content-box`.
    //
    // GEMESSENER FEHLER (developer.mozilla.org, Korpus MIT CSS, 900 px,
    // 18:43): ein Listenpunkt 48 px hoch in einem Elternteil mit
    // gesetzter Hoehe 19,2 px und **0 px Inhaltshoehe**.
    //
    //   [9]  #726 [ 89.00,6650.20 271.44x19.20] height=Some(Px(19.2))
    //   [10] #727 [117.00,6666.20  42.22x48.00] height=Some(Auto)
    //
    // Die Fuellung des Listenpunkts (16 px oben) ist groesser als die
    // gesetzte Hoehe des Elternteils, und weil die gesetzte Groesse hier
    // als AUSSENkasten gerechnet wurde, fiel der Inhalt auf 0.
    //
    // DIE STELLE: alle nachfolgenden Zweige rechnen mit dem
    // AUSSENkasten weiter (`child_available = groesse - fuellung`,
    // `sizes.set(Size::new(breite, hoehe))`, `flow.rs` zieht die
    // Fuellung vom Aussenkasten ab). Deshalb wird hier EINMAL
    // umgerechnet und danach nichts mehr angefasst.
    //
    // DIE KLEMME BLEIBT VORHER RICHTIG: `clamp_length` und der
    // `ersetzte_groesse`-Zweig rechnen `min-*`/`max-*` gegen die
    // BENUTZTE Breite, und die ist nach §10.4 der **Inhalts**kasten.
    // Deshalb steht diese Umrechnung bewusst HINTER beiden.
    //
    // `non_negative` unten: der Aussenkasten kann nicht kleiner als die
    // Fuellung sein, sonst waere der Inhalt negativ. Das ist genau der
    // Fall, der vorher auf 0 fiel.
    let (explicit_width, explicit_height) = (
        explicit_width.map(|w| non_negative(w + padding.left + padding.right)),
        explicit_height.map(|h| non_negative(h + padding.top + padding.bottom)),
    );

    // Release 11.10.2026 (Agent CSS2): DIE TABELLE (CSS 2.1 Kapitel 17,
    // `tabellen_leg`). Ihre Kinder werden einmal mit der ganzen Breite
    // gemessen (fuer die Inhaltsbreiten), dann legt die Spaltenphase die
    // Spalten, misst jede Zelle mit ihrer Breite nach, und die Zeilenphase
    // legt alles. `width`/`height` einer Tabelle sind RAHMENmasse (GEMESSEN:
    // WPT `fixed-table-layout-002a` rechnet 23 + 19 + 382 + 19 + 262 + 19 +
    // 34 = 758 px = `width`).
    if node.kind == NodeKind::Container && !stil_fehlerhaft && crate::tabellen_leg::ist_tabelle(ctx, id) {
        let roh_b = resolve_length(stil.size.width, available.width, id, "width")?;
        let roh_h = resolve_length(stil.size.height, available.height, id, "height")?;
        let (gmin_b, gmax_b, gmin_h, gmax_h) = grenzen(ctx, id);
        let (pmin_b, pmax_b, pmin_h, pmax_h) = grenzen_prozent(ctx, id, available.width, available.height);
        let verfuegbar = non_negative(available.width - margin.left - margin.right);
        // `box-sizing` wie bei jedem Kasten; `<table>` hat im UA-Blatt
        // `box-sizing: border-box` (HTML §15.3.8), `display: table` nicht
        // (GEMESSEN: WPT `height-width-table-001`, 50 px + 2 x 25 px Rahmen
        // = 100 px).
        let rahmen_mass = |v: f32, abstand: f32| if border_box { non_negative(v).max(abstand) } else { non_negative(v) + abstand };
        let angabe = roh_b.map(|w| rahmen_mass(w, quer_abstand));
        let hoehe = match sizes.erzwungen.get(&id.0) {
            Some(h) => Some(*h),
            None => roh_h.map(|h| rahmen_mass(h, hoch_abstand)),
        };
        // `max-height`/`min-height` (CSS 2.1 §10.7) begrenzen die angegebene
        // Hoehe (GEMESSEN: WPT `max-height-applies-to-013`).
        let hoehe = match cmp_or(pmax_h, gmax_h).map(|m| rahmen_mass(m, hoch_abstand)) {
            Some(m) => hoehe.map(|h| h.min(m)),
            None => hoehe,
        };
        let hoehe = match cmp_or(pmin_h, gmin_h).map(|m| rahmen_mass(m, hoch_abstand)) {
            Some(m) => Some(hoehe.unwrap_or(0.0).max(m)),
            None => hoehe,
        };
        // Release 11.10.2026: als Flex-Element gilt die benutzte Groesse aus
        // dem Flex-Algorithmus (`flexbox.rs`, Rahmenmasse, schon begrenzt)
        // statt `width`/`height` (CSS Flexbox 1 §9.7; die angegebene Hoehe
        // einer Tabelle zaehlt nicht als weitere Mindesthoehe). GEMESSEN
        // vorher: nach dem Tabellen-Umbau kippten WPT css-flexbox
        // `table-as-item-specified-height`, `-inflexible-in-column-1`,
        // `-narrow-content-2`, `-specified-width-vertical` und
        // `table-with-percent-intrinsic-width` (die Tabelle behielt 500 px
        // bzw. 100 % statt der Flex-Groesse).
        let (angabe, hoehe) = match ctx.flex_mass.borrow().get(&id) {
            Some(&(fb, fh)) => (fb.or(angabe), fh.or(hoehe)),
            None => (angabe, hoehe),
        };
        let kind_breite = non_negative(angabe.unwrap_or(verfuegbar) - quer_abstand);
        let kind_hoehe = hoehe.map_or(available.height, |h| non_negative(h - hoch_abstand));
        push_children(
            stack,
            node,
            Size::new(kind_breite, kind_hoehe),
            Some(SizeTask::TabelleSpalten {
                id,
                angabe,
                verfuegbar,
                rand: padding,
                hoehe,
                kind_hoehe,
                min_max: (
                    cmp_or(pmin_b, gmin_b).map(|v| rahmen_mass(v, quer_abstand)),
                    cmp_or(pmax_b, gmax_b).map(|v| rahmen_mass(v, quer_abstand)),
                ),
            }),
        );
        return Ok(());
    }

    //
    // P12 (05.10.2026, 18:03): WERKZEUG — die Groesse ERSETZTER
    // Elemente mit allen dazugehoerenden Zahlen drucken.
    //
    // GEMESSENER WIDERSPRUCH, der dieses Werkzeug noetig macht:
    // derselbe Knoten (Container, `size = {Auto, Auto}`,
    // `ersetzt = 7129x1995`, Plan `max_breite = 900`) kommt auf der
    // gespeicherten nixos-Seite als **7129 x 1995** heraus, im Test
    // `tests/bild_max_breite.rs` dagegen als 780 px (die Breite des
    // Eltern). Beide Male ist derselbe Code gelaufen — also kann es
    // nicht an der Regel liegen, sondern an einer Zahl, die vorher
    // anders ist. Solange ich nur das Endergebnis habe, ratte ich.
    //
    // Ohne die Variable: ein `var_os` je Knoten mit `ersetzt`, nicht
    // je Knoten ueberhaupt.
    if ersatz_messen_an() {
        if let Some(m) = node.ersetzt {
            eprintln!(
                "ERSATZ #{} art={:?} stil={:?}x{:?} masse={:?} min_b={:?} max_b={:?} \
                 explicit={:?}x{:?} verfuegbar={:?} stil_richtung={:?} plan={:?}",
                id.0,
                node.kind,
                stil.size.width,
                stil.size.height,
                m,
                min_b,
                max_b,
                explicit_width,
                explicit_height,
                available.width,
                stil.direction,
                ctx.plan.map(|p| p.grenzen(id)),
            );
        }
    }

    if node.kind == NodeKind::Container
        && ctx.stil(id).is_some_and(|p| {
            matches!(
                p.anzeige,
                narsil_layout::plan::Anzeige::Flex | narsil_layout::plan::Anzeige::Raster
            )
        })
    {
        let width = quer(non_negative(
            explicit_width.unwrap_or(available.width - margin.left - margin.right),
        ));
        push_children(
            stack,
            node,
            Size::new(
                non_negative(width - padding.left - padding.right),
                // Release 11.10.2026: die INHALTShoehe (hier ist
                // `explicit_height` schon das Rahmenmass). GEMESSEN vorher:
                // WPT `flex-grow-001` (`height:100%` im Flex-Element mit
                // Rahmen am Container: 62 statt 60 px).
                explicit_height.map_or(0.0, |h| non_negative(h - padding.top - padding.bottom)),
            ),
            Some(SizeTask::Format {
                id,
                width,
                height: explicit_height,
                padding,
                nachmessung: false,
            }),
        );
        return Ok(());
    }

    match node.kind {
        NodeKind::Text => {
            // Blatt: Breite = gemessene Textbreite, Hoehe = Zeilenhoehe.
            // Beides ist die INHALTSGROESSE, die Border-Box addiert Padding.
            // `intrinsic_size` prueft hier noch einmal die Schriftgroesse,
            // bevor gemessen wird (TASK_0025, AUFGABE 2) — daher `?`.
            let intrinsic = text::intrinsic_size(ctx, node)?;
            //
            // **09:03: DER TEXTKNOTEN TRAEGT SEIN `min-content` ein.**
            // `intrinsic_size` ist die **Zielbreite** (die ganze Zeile);
            // der **Boden** ist die breiteste unumbrechbare Stelle — und
            // das ist genau der Unterschied, der auf spiegel.de 0 gegen
            // 168 px ausmacht. `text::min_content_breite` rechnet es nach
            // CSS Sizing 3 §5.2 (inkl. Bindestrich als Umbruchpunkt).
            sizes.set_min(id, text::min_content_breite(ctx, node));
            sizes.set(
                id,
                Size::new(
                    quer(non_negative(
                        explicit_width.unwrap_or(intrinsic.width + padding.left + padding.right),
                    )),
                    hoch(non_negative(
                        explicit_height.unwrap_or(intrinsic.height + padding.top + padding.bottom),
                    )),
                ),
            );
            // Textknoten sind in Version 0 Blaetter (Spezifikation E13).
        }
        NodeKind::Page => {
            // GEMESSENER FEHLER 30.09.2026 (A4, 30 echte Seiten): der
            // `page`-Root wurde auf die Viewport-HOEHE festgenagelt
            // (`available.height - margin.top - margin.bottom`), egal
            // wie viel Inhalt darin stand. Auf kernel.org ist der
            // Inhalt 4 027 px hoch, der Root 810 — die Kinder ragten
            // 3 217 px aus ihm heraus. Diese Zahl stand als
            // `KindRaus 3217` in `probe/a4/ERGEBNIS.txt`, und sie war
            // auf ALLEN 30 Seiten da.
            //
            // CSS 2.1, 10.6: die Breite und Hoehe des Wurzel-
            // Elements bestimmen den Scroll-Bereich. `height:auto`
            // heisst fuer den Root wie fuer jeden anderen Container
            // "so hoch wie der Inhalt".
            //
            // DIE UNTERGRENZE BLEIBT: ein leeres Dokument soll den
            // Viewport fuellen (der Hintergrund des Roots faellt auf die
            // Leinwand durch, CSS 2.1, 12.1). Deshalb `max`, nicht
            // einfach die Summe — der Doctest in `lib.rs` behauptet
            // `height == 600` fuer ein leeres Dokument bei 600 px
            // Viewport, und der ist richtig.
            let width = non_negative(
                explicit_width.unwrap_or(available.width - margin.left - margin.right),
            );
            match explicit_height {
                Some(height) => {
                    let size = Size::new(width, hoch(non_negative(height)));
                    sizes.set(id, size);
                    let child_available = Size::new(
                        non_negative(size.width - padding.left - padding.right),
                        non_negative(size.height - padding.top - padding.bottom),
                    );
                    push_children(stack, node, child_available, None);
                }
                None => {
                    // Kinder gegen die Viewport-Hoehe messen (nicht gegen
                    // eine noch unbekannte Root-Hoehe), dann die Summe
                    // gegen den Viewport halten.
                    let kind_hoehe = available.height - margin.top - margin.bottom;
                    let child_available = Size::new(
                        non_negative(width - padding.left - padding.right),
                        non_negative(kind_hoehe - padding.top - padding.bottom),
                    );
                    push_children(
                        stack,
                        node,
                        child_available,
                        Some(SizeTask::Sum {
                            id,
                            cross: width,
                            padding,
                            child_available,
                            axis: Axis::Y,
                            // L5 (13:5x): die Seitenwurzel ist nie
                            // positioniert — sie ist der enthaltende
                            // Block fuer alles darunter.
                            schrumpft: false,
                            feste_hoehe: None,
                            mindest: non_negative(kind_hoehe),
                        }),
                    );
                }
            }
        }
        NodeKind::Container => match stil.direction {
            Direction::Column => {
                // L5 (02.10.2026 13:5x): SHRINK-TO-FIT, und wann sie
                // gilt. CSS 2.1, 10.3.7: bei `width: auto` hat eine
                // absolute Box `min(max(geringste, verfuegbare),
                // bevorzugte)`. Ein gewöhnlicher Block hat stattdessen
                // die Breite des enthaltenden Blocks.
                //
                // GEMESSENER BEFUND, was vorher passierte
                // (`tests/l5_absolute.rs`,
                // `l5_absolute_ohne_breite_bekommt_shrink_to_fit`): eine
                // absolute Box ohne `width` bekam 600 px statt der 60 px
                // ihres Kindes.
                //
                // GEMESSENE ENTSCHEIDUNG, warum die Dreier-Bedingung
                // reicht und nicht ein eigenes Feld im Plan: Der Plan
                // traegt `position`, und `width: auto` steht schon in
                // `explicit_width == None`. Eine neue Eigenschaft im
                // Plan waere eine dritte Kopie derselben Information.
                // Release 10.10.2026: `left` UND `right` gesetzt -> die Breite
                // ist der enthaltende Block minus beide Versaetze und Raender
                // (CSS 2.1 §10.3.7, Regel 5), begrenzt durch min-/max-width —
                // KEINE Schrumpfbreite. GEMESSEN vorher: `position:fixed;
                // left:0; right:0; margin:0 auto; max-width:600px` schrumpfte
                // erst auf den Inhalt und wurde dann voll breit (Chrome: 600 px,
                // mittig).
                // Release 11.10.2026 (Agent CSS2): nicht bei `width:
                // fit-content` und Verwandten (`breite_inhalt`).
                let abs_dehnt = explicit_width.is_none()
                    && ctx.plan.and_then(|p| p.hole(id)).is_some_and(|st| {
                        matches!(st.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)
                            && st.links.is_some()
                            && st.rechts.is_some()
                            && !st.breite_inhalt
                    })
                    // Release 11.10.2026: mit `justify-self: start` usw.
                    // passend statt gedehnt (CSS Box Alignment 3 §6).
                    && abs_selbst_lage(ctx, id).0.is_none();
                let abs_versatz = if abs_dehnt {
                    ctx.plan
                        .and_then(|p| p.hole(id))
                        .map(|st| st.links.unwrap_or(0.0) + st.rechts.unwrap_or(0.0))
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
                // Release 11.10.2026: eine Tabelle schrumpft auch zwischen `left`
                // und `right` (CSS 2.1 §17.5.2.2: MAX, wenn es passt).
                let ist_tabelle = ctx.plan.and_then(|p| p.hole(id)).is_some_and(|st| st.anzeige == narsil_layout::plan::Anzeige::Tabelle);
                let schrumpft_breite = explicit_width.is_none()
                    && (!abs_dehnt || ist_tabelle)
                    && ctx
                        .plan
                        .and_then(|p| p.hole(id))
                        .is_some_and(|st| {
                            matches!(st.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)
                                // P9 (05.10.2026): ein INLINE-BLOCK mit
                                // `width:auto` bekommt ebenfalls die
                                // Schrumpfbreite (CSS 2.1, 10.3.9: dieselbe
                                // Formel wie 10.3.7). Ohne sie waere der
                                // Kasten so breit wie der enthaltende Block
                                // (im Zeilenlauf: die ganze Zeile) und
                                // stapelte.
                                || inline_ebene(st)
                                // Release 11.10.2026: eine Tabelle mit `width: auto`
                                // schrumpft auf ihre Spalten (CSS 2.1 §17.5.2.2, siehe
                                // `tabellen_spaltenbreiten`).
                                || st.anzeige == narsil_layout::plan::Anzeige::Tabelle
                        })
                    // Release 11.10.2026: ein Block mit `justify-self` als Lage
                    // (CSS Box Alignment 3 §6.1). GEMESSEN vorher: WPT css-align
                    // `blocks/justify-self-htb-ltr-htb` (der Block fuellte den
                    // Container).
                    || explicit_width.is_none() && block_selbst_lage(ctx, id).is_some();
                // Quer-Achse (x) ist bekannt, bevor die Summe auf der
                // Haupt-Achse (y) entsteht.
                let width = quer(non_negative(
                    explicit_width.unwrap_or(available.width - margin.left - margin.right - abs_versatz),
                ));
                //
                // P19 (05.10.2026, 19:41): DIE HOEHE DER KINDER, UNTEN
                // IST DIE DES KASTENS, NICHT DIE DER ANSICHT.
                //
                // GEMESSENER FEHLER: `height: 100%` in einem Block von
                // 600 px ergab **650 px** — die Ansichtshoehe.
                // (`tests/p19_prozent_gegen_block.rs`, rot erwartet.)
                //
                // CSS 2.1 §10.5, woertlich: "the percentage is calculated
                // with respect to the height of the generated box's
                // containing block". Der umgebende Block ist der Eltern-
                // kasten, und dessen Inhaltshoehe ist bei gesetzter
                // Hoehe bekannt — `height` minus Fuellung. Vorher stand
                // hier `available.height`, die durchgereichte Ansichts-
                // hoehe, und die ist nur ZUFALLIG richtig, wenn der
                // Eltern die ganze Ansicht fuellt (der Wurzelknoten).
                //
                // Bei `height: auto` bleibt es bei `available.height`:
                // die Hoehe des Inhalts kennt hier niemand, sie entsteht
                // erst aus der Summe der Kinder. CSS 2.1 §10.5 sagt fuer
                // diesen Fall ohnehin: "the value computes to 'auto'".
                let kind_hoehe = explicit_height
                    .map(|h| non_negative(hoch(h) - padding.top - padding.bottom))
                    .unwrap_or(available.height);
                let child_available = Size::new(
                    non_negative(width - padding.left - padding.right),
                    kind_hoehe,
                );
                // Release 11.10.2026 (Agent CSS2): die `height` einer Zelle ist
                // ein Mindestmass (CSS 2.1 §17.5.3) — der Inhalt waechst darueber
                // hinaus, und die natuerliche Hoehe wird fuer `vertical-align`
                // gebraucht (`sum_children` merkt sie).
                let ist_zelle = ctx.stil(id).is_some_and(|s| s.anzeige == narsil_layout::plan::Anzeige::TabelleZelle);
                let zellen_mindest = if ist_zelle { explicit_height.map_or(0.0, hoch) } else { 0.0 };
                match explicit_height.filter(|_| !ist_zelle) {
                    Some(height) => {
                        sizes.set(id, Size::new(width, hoch(height)));
                        // L5 (13:5x): bei shrink-to-fit wird der Task
                        // trotzdem gebraucht — fuer die BREITE. Ohne ihn
                        // bliebe die volle Inhaltsbreite des
                        // enthaltenden Blocks stehen, GEMESSEN 600 px
                        // statt 60 (`l5_absolute_ohne_breite_bekommt_
                        // shrink_to_fit`).
                        // Release 11.10.2026 (Agent CSS2): und bei Float-
                        // Kindern — `sum_children` stellt Kaesten, die Floats
                        // meiden, daneben (und misst sie dafuer neu).
                        // GEMESSEN vorher (WPT `new-fc-beside-float-with-
                        // min-width`): in einem Block mit `height` lag der
                        // `overflow:hidden`-Kasten unter dem Float.
                        let float_kinder = node.children.iter().any(|&c| {
                            ctx.stil(c).is_some_and(|s| s.schweben != narsil_layout::plan::FloatArt::Keins)
                        });
                        // Release 12.10.2026 (Agent CSS2): und wenn der obere
                        // Rand des ersten Kindes durch den Kasten nach aussen
                        // wandern kann — `height` verhindert das oben nicht
                        // (CSS 2.1 §8.3.1, nur unten mit `height: auto`), und
                        // `sum_children` merkt es (`Sizes::hochgezogen`).
                        // GEMESSEN vorher (WPT `css1/c5502-imrgn-r-000`):
                        // `<div style="height:2em"><p>` — der Absatz stand
                        // 1 em unter der Oberkante.
                        let rand_durch = padding.top <= 0.0 && darf_raender_durchlassen(ctx, id, node);
                        // Release 14.10.2026: and when the height comes from
                        // `aspect-ratio` and may grow with the contents
                        // (`Sizes::verhaeltnis_wachsend`), also in a box whose
                        // margins do not collapse through (a flex item). MEASURED
                        // before: WPT css-sizing `aspect-ratio/flex-aspect-
                        // ratio-004` (a column item with `aspect-ratio: 2/1`
                        // around 100 px of content was 50 px high).
                        let waechst = sizes.verhaeltnis_wachsend.contains(&id.0)
                            || ctx.stil(id).is_some_and(|s| s.height_keyword_limits != 0);
                        let nach = if schrumpft_breite || float_kinder || rand_durch || waechst {
                            Some(SizeTask::Sum {
                                id,
                                cross: width,
                                padding,
                                child_available,
                                axis: Axis::Y,
                                mindest: 0.0,
                                schrumpft: schrumpft_breite,
                                feste_hoehe: Some(hoch(non_negative(height))),
                            })
                        } else {
                            None
                        };
                        push_children(stack, node, child_available, nach);
                    }
                    None => push_children(
                        stack,
                        node,
                        child_available,
                        Some(SizeTask::Sum {
                            id,
                            cross: width,
                            padding,
                            child_available,
                            axis: Axis::Y,
                            // **09:03: DER BODEN IST JETZT DER
                            // BODEN AUS DER SPEZIFIKATION** und nicht 0.
                            // `min(max(mindest, verfuegbar),
                            // bevorzugt)` (CSS 2.1 §10.3.7) kann bei
                            // `verfuegbar = 0` nur dann etwas anderes
                            // als 0 liefern, wenn `mindest` das
                            // `min-content` der Kinder ist — **Block:
                            // Maximum, Zeile: Summe**
                            // (`Sizes::min_aggregat`).
                            // **Hier bleibt 0.0** — der Boden wird in
                            // `sum_children` berechnet, wo die Kinder
                            // gemessen sind (09:04). Auf der Querachse
                            // ist das ohnehin die Mindesthoehe, nicht
                            // die min-content-Breite.
                            // Release 11.10.2026 (Agent CSS2): bei einer
                            // Zelle ihre `height` (siehe `zellen_mindest`).
                            mindest: zellen_mindest,
                            //
                            // L5 (02.10.2026 13:5x): SHRINK-TO-FIT fuer
                            // einen positionierten Kasten ohne Breite
                            // (CSS 2.1, 10.3.7). Der Boden bleibt hier 0;
                            // der shrink-to-fit-Wert entsteht in
                            // `sum_children`, wo die Kinder gemessen sind
                            // — dasselbe Argument wie beim Boden (09:04).
                            schrumpft: schrumpft_breite,
                            feste_hoehe: None,
                        }),
                    ),
                }
            }
            Direction::Row => {
                let height = hoch(non_negative(
                    explicit_height.unwrap_or(available.height - margin.top - margin.bottom),
                ));
                // Release 11.10.2026 (Agent CSS2): eine Zeilenbox mit gehobenen
                // oder gesenkten Laeufen (`vertical-align`) waechst auf die
                // Hoehe ihrer Inline-Kaesten (CSS 2.1 §10.8.1).
                let height = if ist_zeilenbox(ctx, id, node) {
                    height.max(crate::flow::zeile_mit_hebung(ctx, node).unwrap_or(0.0) + padding.top + padding.bottom)
                } else {
                    height
                };
                let child_available = Size::new(
                    available.width,
                    non_negative(height - padding.top - padding.bottom),
                );
                match explicit_width {
                    Some(width) => {
                        sizes.set(id, Size::new(quer(width), height));
                        push_children(stack, node, child_available, None);
                    }
                    None => push_children(
                        stack,
                        node,
                        child_available,
                        Some(SizeTask::Sum {
                            id,
                            cross: height,
                            padding,
                            child_available,
                            axis: Axis::X,
                            //
                            // **Dieselbe Regel, andere Achse: bei einer
                            // Zeile summieren sich die kleinsten Breiten
                            // der Kinder**, weil sie nebeneinander
                            // stehen. **Auf spiegel.de ist genau dieser
                            // Zweig schuld** (`PLAN #39 eintrag=Some(Flex)`
                            // heisst `Direction::Row`).
                            // **Hier bleibt 0.0** — siehe die
                            // Achse-Y-Stelle: der Boden entsteht in
                            // `sum_children`, nach den Kindern (09:04).
                            mindest: 0.0,
                            //
                            // L5 (13:5x): hier NICHT noetig. Der
                            // Zeilenzweig rechnet die Breite ueber die
                            // X-Achse, und die Formel dort ist bereits
                            // `bevorzugt.min(verfuegbar.max(boden))` —
                            // shrink-to-fit, unabhaengig von `position`.
                            schrumpft: false,
                            feste_hoehe: None,
                        }),
                    ),
                }
            }
        },
    }
    Ok(())
}

/// Fuellt die Haupt-Achse eines Containers nach, wenn alle Kinder gemessen sind.
/// `#[allow(clippy::too_many_arguments)]` ist hier NOTIG: die Untergrenze
/// kam mit dem `page`-Root dazu, und sie ist ein echter Parameter — sie in
/// einen kleinen Aufgabentyp zu stecken, wuerde den Aufgabenstapel
/// schlimmer machen, nicht besser.
#[allow(clippy::too_many_arguments)]
fn sum_children(
    sizes: &mut Sizes,
    ctx: &Ctx<'_>,
    id: NodeId,
    cross: f32,
    padding: Edges,
    child_available: Size,
    axis: Axis,
    mindest: f32,
    schrumpft: bool,
    feste_hoehe: Option<f32>,
) -> Result<(), LayoutError> {
    let node = get_node(ctx, id)?;
    if axis == Axis::X && ist_zeilenbox(ctx, id, node) {
        laeufe_als_kette(ctx, sizes, node);
    }
    //
    // **09:04: DER BODEN WIRD HIER UND NICHT BEIM AUFBAU BERECHNET.**
    // Mein erster Versuch hat `mindest` **an der Aufrufstelle**
    // (`push_children(… Some(SizeTask::Sum { mindest: … }))`) gesetzt —
    // und das ist **zu frueh**: dort sind die Kinder **noch nicht
    // gemessen**, ihre `min_content`-Spalte steht also auf 0, und der
    // Boden bleibt 0. **GEMESSEN: mein spiegel-geformter Test blieb
    // gruen und damit der Fehler unentdeckt** — waere er nicht als
    // Test geschrieben gewesen, haette ich die Reparatur fuer eine
    // gemeldet.
    // **Hier dagegen sind die Kinder fertig** (darum gibt es diese
    // Aufgabe ueberhaupt), also ist der Boden **jetzt** berechenbar.
    //
    // **UND NUR AUF DER HAUPTACHSE.** `mindest` ist auf der
    // Querachse die **Mindesthoehe** des Inhalts (`kind_hoehe`), und
    // die bleibt unangetastet — eine `min-content`-Breite waere dort
    // eine andere Groesse.
    let boden = match axis {
        Axis::X => sizes.min_aggregat(ctx, node, Direction::Row),
        Axis::Y => mindest,
    };

    let mut sum = 0.0;
    // added 04.10.2026, Rolle LAYOUT (LAYOUT_2): **DIE TABELLEN-ACHSE.**
    //
    // GEMESSENER FEHLER (2 Zeilen, 3 Zellen, 600 px, echtes HTML mit
    // `<tbody>` — `tests/tabelle_hoehe_ist.rs`):
    //
    //   Zelle   198,67 x 19,00   <- 6 davon, richtig im Raster (flow.rs)
    //   Zeile        600 x 19,00  <- richtig, aus `zeilen_rechtecke`
    //   tbody        600 x 114,00 <- FALSCH
    //   Tabelle      600 x 114,00 <- FALSCH
    //   Absatz danach     y 114,00 <- 74 px zu tief
    //
    // **DIE URSACHE, genau hier:** die Schleife unten addiert die Kinder
    // **untereinander**, weil sie von Bloecken ausgeht (Zeile 1561-1564).
    // In einer Tabellen**zeile** stehen die Zellen aber **nebeneinander** —
    // `flow.rs` legt sie im Raster, und diese Schleife rechnet sie als
    // Bloecke. 114 = 2 Zeilen x (3 Zellen x 19) = 57 + 57.
    //
    // Die Korrektur ist **kein Sonderfall fuer `border-collapse`** und keine
    // zweite Tabelle: es sind die zwei Regeln, die CSS 2.1, 17.6 fuer die
    // Hauptrichtung sagt —
    //
    //   * die Hoehe einer Zeile ist das MAXIMUM ihrer Zellen,
    //   * die Zeilen einer Tabelle stehen untereinander, getrennt durch
    //     `border-spacing`.
    //
    // Beide brauchen **kein** neues Feld und **keinen** Aufruf nach
    // `tabelle.rs` — sie sind eine Frage der Richtung, in der die Kinder
    // liegen. `tabellen_hoehe_gerade_y` beantwortet genau das und gibt
    // `None` zurueck, wenn der Knoten kein Tabellen-Kasten ist.
    let tabellen_hoehe = if axis == Axis::Y {
        tabellen_hoehe_gerade_y(ctx, sizes, node, child_available)?
    } else {
        None
    };
    // P9 (05.10.2026): DER INLINE-LAUF IN DER MESSPHASE.
    //
    // Ohne das waere die Auto-Hoehe eines Absatzes die SUMME aller
    // Inline-Teile, waehrend die Platzphase sie nebeneinander legt. Die
    // beiden Phasen widersprachen sich dann: die Kaesten standen auf
    // EINER Zeile (y = 0 und y = 0), der Absatz war aber so hoch wie
    // zwei Zeilen (GEMESSEN 60 px statt 30). Im Bild ist das ein Absatz,
    // der 30 px Inhalt hat und 60 px Platz reserviert.
    //
    // GEMESSENE ZAHLEN, bevor das hier stand (Absatz 400 px breit):
    //   zwei 30-px-Inline-Bloecke   -> Absatz 60 px  (statt 30)
    //   60 + 40 + 80 in 150 px      -> Absatz 80 px  (statt 50)
    //
    // Die Regel ist dieselbe wie in `flow::inline_teil`: eine Folge von
    // Inline-Teilen bildet eine Zeile, die Zeile ist so hoch wie ihr
    // hoechstes Teil, ein Block beendet die Zeile, und ein Umbruch
    // entsteht, wenn das naechste Teil nicht mehr passt.
    // Release 11.10.2026 (Agent CSS2): jede Zeile beginnt mit dem Strut des
    // Blocks, ihre Hoehe ist Grundlinie plus groesster Rest darunter — wie
    // in `vorschau::zeilen_vorschau` (sonst widersprechen sich Mess- und
    // Platzphase).
    let strut = crate::flow::strut(ctx, node);
    let text_zeile = if node.style.zeilenhoehe_art == narsil_core::style::ZeilenhoeheArt::Normal && ctx.stil(node.id).is_some() {
        // Release 12.10.2026 (Agent CSS2): `normal` wie Strut und Textlauf (`zeilenhoehe_normal`).
        crate::text::zeilenhoehe_normal(node.style.font_size)
    } else if node.style.zeilenhoehe_px.is_finite() && node.style.zeilenhoehe_px > 0.0 {
        node.style.zeilenhoehe_px
    } else {
        node.style.font_size
    };
    let mut zeile = MessZeile::mit_strut(strut);
    // Release 15.10.2026 (Agent CSS2): the inline part before (for
    // `flow::glued_to_previous`, as in the placement).
    let mut prev_inline: Option<NodeId> = None;
    // added 04.10.2026, Rolle LAYOUT (LAYOUT_2): die Schleife unten ist die
    // **Blockrechnung** (Kinder untereinander). Eine Tabelle gehoert nicht
    // dazu — siehe `tabellen_hoehe_gerade_y`. Deshalb laeuft sie nur, wenn
    // das kein Tabellen-Kasten ist.
    //
    // Release 09.10.2026: MARGIN-COLLAPSING (siehe `Sizes::hochgezogen`).
    // `kollaps`: dieser Knoten legt seine Kinder als Blockfluss; dann gilt
    // dieselbe Regel wie in `flow::kind_platzieren`.
    let kollaps = axis == Axis::Y && tabellen_hoehe.is_none() && ist_blockfluss(ctx, id, node);
    let durchlass = kollaps && darf_raender_durchlassen(ctx, id, node);
    // Release 12.10.2026 (Agent CSS2): die geerbten Floats des umgebenden
    // Kontexts (`Ctx::float_erbe`) zaehlen wie eigene.
    let erbe = if kollaps { ctx.float_erbe.borrow().get(&id).copied() } else { None };
    let erbe_n = erbe.map_or(0, |e| e.anzahl());
    let mut hat_floats = kollaps
        && (erbe_n > 0
            || node.children.iter().any(|&c| ctx.stil(c).is_some_and(|s| s.schweben != narsil_layout::plan::FloatArt::Keins)));
    let mut vorige_unten: Option<f32> = None;
    let mut inhalt_davor = false;
    let mut oben_hoch: Option<f32> = None;
    // Release 12.10.2026 (Agent CSS2): die offene Randkette am Ende stammt von
    // einem leeren Kasten MIT `clear` (und leeren Geschwistern danach) — sie
    // faellt dann nicht mit dem unteren Rand des Eltern zusammen (CSS 2.1
    // §8.3.1: "that resulting margin does not collapse with the bottom margin
    // of the parent block"). GEMESSEN vorher (WPT `floats-clear/margin-
    // collapse-033..035`): der Eltern war 1 statt 100 px hoch.
    let mut kette_mit_freiraum = false;
    // Release 11.10.2026 (Agent CSS2): die Unterkanten der Floats (links,
    // rechts) relativ zum Inhaltsanfang. Ein Float nimmt keinen Platz im
    // Fluss ein (CSS 2.1 §9.5): er steht an der Stelle des Flusses und
    // zaehlt nur fuer einen Kasten mit EIGENEM Formatierungskontext zur
    // Hoehe (§10.6.7), und ein `clear`-Kasten rueckt unter ihn (§9.5.2).
    // GEMESSEN vorher (WPT `floats-clear/clear-initial-001` u. a.): jeder
    // Float wurde zur Hoehe ADDIERT — `<div><float 100px/><div 100px/>
    // </div>` war 200 statt 100 px hoch.
    let mut float_unten = (0.0f32, 0.0f32);
    // Floats aus Nachfahren ohne eigenen Kontext (nur fuer die Hoehe).
    let mut float_tief = 0.0f32;
    // Release 11.10.2026 (Agent CSS2): dieselbe Float-Belegung wie in
    // `flow::kind_platzieren` — fuer Kaesten, die Floats meiden.
    let mut fz = erbe.unwrap_or_default();
    let luecke = ctx.stil(id).map_or(0.0, |s| s.luecke);
    for &child in &node.children {
        if tabellen_hoehe.is_some() {
            break;
        }
        // Absolut/fest positionierte und unsichtbare Kinder nehmen keinen
        // Platz im Normalfluss ein. Ihre Groesse wird trotzdem separat
        // gemessen und spaeter gegen den enthaltenden Block platziert.
        let plan = ctx.stil_oder_standard(child);
        if !plan.wird_gezeichnet()
            || matches!(
                plan.position,
                narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest
            )
        {
            continue;
        }
        let child_node = get_node(ctx, child)?;
        let size = sizes.get(child).ok_or(LayoutError::UnknownNode(child))?;
        let margin = resolve_spacing(
            child_node.style.margin,
            child_available.width,
            child,
            "margin",
        )?;
        let inline = axis == Axis::Y && crate::flow::kind_ist_inline(ctx, child, &child_node.kind);
        if inline {
            // Release 11.10.2026 (Agent CSS2): ein kollabierbares Leerzeichen
            // am Zeilenanfang oder dort, wo es nicht mehr passt, faellt weg
            // (CSS 2.1 §16.6.1) — wie in der Platzphase (`inline_teil`); und es
            // macht keine Zeile hoeher. GEMESSEN vorher (WPT
            // `padding-bottom-applies-to-014`): das Leerzeichen hinter einer
            // zeilenbreiten `inline-table` eroeffnete eine zweite Zeile (19 px).
            let leer = crate::flow::kollabierer_leerraum(ctx, child);
            let passt = zeile.braucht + size.width + margin.left + margin.right <= child_available.width
                || (!leer && crate::flow::glued_to_previous(ctx, prev_inline, child));
            if leer && (!zeile.gleich || !passt) {
                continue;
            }
            prev_inline = Some(child);
            if zeile.gleich && !passt {
                sum += zeile.hoehe();
                zeile = MessZeile::mit_strut(strut);
            }
            // Release 11.10.2026 (Agent CSS2): atomare Teile mit ihren
            // senkrechten Raendern (`flow::teil_mit_rand`).
            let h = if child_node.kind == NodeKind::Text { size.height.max(text_zeile) } else { size.height };
            let (h_rand, g) = crate::flow::teil_mit_rand(ctx, child, &child_node.kind, h, margin.top, margin.bottom);
            let h_zeile = if child_node.kind == NodeKind::Text { size.height } else { h_rand };
            zeile.anhaenge(size.width + margin.left + margin.right, if leer { 0.0 } else { h_zeile });
            if !leer && crate::flow::vertikal_von(ctx, child) == narsil_layout::Vertikal::Grundlinie {
                zeile.grundlinie(g, h_rand);
            }
            if kollaps && !crate::flow::kollabierer_leerraum(ctx, child) {
                inhalt_davor = true;
                vorige_unten = None;
            }
        } else {
            // Ein Block beendet die Zeile — und kommt in voller Hoehe
            // unter sie.
            prev_inline = None;
            sum += zeile.hoehe();
            zeile = MessZeile::mit_strut(strut);
            let schwebt = ctx.stil(child).is_some_and(|s| s.schweben != narsil_layout::plan::FloatArt::Keins);
            if kollaps && schwebt {
                let st = ctx.stil_oder_standard(child);
                let y_rel = fz.clear_y(st.raeumen, sum);
                let (q, _) = fz.platziere_mit_umbruch(
                    child_available.width,
                    margin.left + size.width + margin.right,
                    margin.top + size.height + margin.bottom,
                    y_rel,
                    st.schweben,
                    luecke,
                );
                let unten = non_negative(q.y + q.hoehe);
                if ctx.stil(child).is_some_and(|s| s.schweben == narsil_layout::plan::FloatArt::Rechts) {
                    float_unten.1 = float_unten.1.max(unten);
                } else {
                    float_unten.0 = float_unten.0.max(unten);
                }
            } else if kollaps {
                let (t, b) = sizes.wirksamer_rand(child, margin.top, margin.bottom);
                let raeumung = ctx.stil(child).map_or(narsil_layout::plan::RaeumungArt::Keins, |s| s.raeumen);
                let raeumt = hat_floats && raeumung != narsil_layout::plan::RaeumungArt::Keins;
                // Release 12.10.2026 (Agent CSS2): ein LEERER Block am Anfang der
                // Kette (CSS 2.1 §8.3.1: seine Raender fallen durch ihn hindurch
                // zusammen) — er beendet die Kette nicht. Im durchlaessigen
                // Eltern wandern seine Raender mit denen des naechsten Kindes nach
                // aussen; sonst beginnt mit ihm die Kette. GEMESSEN vorher (WPT
                // `floats-clear/clear-applies-to-*-ref`): `<div><img abs/></div>
                // <p style="margin-top:24px">` — der Absatz stand 8 px tiefer, der
                // Rand des `body` fiel nicht mit dem des Absatzes zusammen.
                if vorige_unten.is_none() && leer_durchlaessig(ctx, child, child_node, size, raeumt) {
                    let k = rand_zusammen(t, b);
                    if let Some(u) = sizes.float_ueberhang.get(&child.0).copied() {
                        float_tief = float_tief.max(sum + u);
                    }
                    if !inhalt_davor && durchlass && padding.top <= 0.0 {
                        oben_hoch = Some(oben_hoch.map_or(k, |o| rand_zusammen(o, k)));
                    } else {
                        sum += k;
                        vorige_unten = Some(k);
                        inhalt_davor = true;
                    }
                    continue;
                }
                // Release 12.10.2026 (Agent CSS2): auch mit `clear` zuerst die
                // gedachte Lage MIT zusammenfallenden Raendern; der Freiraum hebt
                // die Rahmenkante nur, wo sie ueber den Floats laege (CSS 2.1
                // §9.5.2: das Groessere von beidem). GEMESSEN vorher (WPT
                // `floats-clear/margin-collapse-123`): `<float/><leer 2em/>
                // <clear margin-top:1em>` stand bei 3 statt 2 em.
                let oben = match vorige_unten {
                    Some(u) => rand_zusammen(u, t) - u,
                    // Release 12.10.2026 (Agent CSS2): `clear` ohne noetigen Freiraum
                    // (der eigene Rand traegt schon unter die Floats) laesst den Rand
                    // durch (WPT `floats-clear/no-clearance-adjoining-opposite-float`).
                    _ if !inhalt_davor
                        && (!raeumt
                            || (sum + t >= erbe.map_or(0.0, |e| e.clear_y(raeumung, 0.0))
                                && sum >= fz.zurueck(erbe_n, 0.0, 0.0, 0.0).clear_y(raeumung, 0.0).max(float_tief)))
                        && durchlass
                        && padding.top <= 0.0 =>
                    {
                        oben_hoch = Some(oben_hoch.map_or(t, |o| rand_zusammen(o, t)));
                        0.0
                    }
                    _ => t,
                };
                // `clear`: die Rahmenkante unter die betroffenen Floats.
                // Release 11.10.2026 (Agent CSS2): auch unter die Floats, die
                // aus frueheren Geschwistern ohne eigenen Kontext ragen (sie
                // gehoeren demselben Formatierungskontext; die Seite ist dort
                // nicht bekannt — beide zaehlen).
                let raeumt = raeumt || (raeumung != narsil_layout::plan::RaeumungArt::Keins && float_tief > 0.0);
                if raeumt {
                    let grenze = match raeumung {
                        narsil_layout::plan::RaeumungArt::Links => float_unten.0,
                        narsil_layout::plan::RaeumungArt::Rechts => float_unten.1,
                        _ => float_unten.0.max(float_unten.1),
                    }
                    .max(float_tief)
                    // Release 12.10.2026 (Agent CSS2): auch geerbte und aus
                    // Nachfahren uebernommene Floats.
                    .max(fz.clear_y(raeumung, 0.0));
                    if grenze > sum + oben {
                        sum = grenze - oben;
                    }
                }
                // Release 11.10.2026 (Agent CSS2): ein Kasten, der Floats
                // meidet, steht NEBEN ihnen — mit `width: auto` so schmal wie
                // der Rest (er wird mit dieser Breite neu gemessen) — oder,
                // wenn er dort nicht hinpasst, unter ihnen (CSS 2.1 §9.5).
                // GEMESSEN vorher (WPT `floats-clear/floats-bfc-001`,
                // `floats-wrap-bfc-*`): der Kasten lag unter dem Float.
                let mut size = size;
                if hat_floats && meidet_floats(ctx, child, child_node) {
                    // Release 12.10.2026 (Agent CSS2): die Floats im BAND des
                    // Kastens (seine ganze Hoehe, CSS 2.1 §9.5: die Rahmenbox
                    // darf keine Randbox eines Floats ueberlappen); passt er
                    // nicht daneben, rueckt er an die naechste Float-Unterkante
                    // (nicht gleich unter alle), und ein geschrumpfter Kasten
                    // wird dort neu gemessen. GEMESSEN vorher (WPT `floats-clear/
                    // table-among-floats-001`, `floats/zero-width-floats-
                    // positioning`): nur die Kante am oberen Rand zaehlte.
                    let mut kante = sum + oben;
                    let mut geschrumpft = false;
                    // Release 12.10.2026 (Agent CSS2): negativer Rand neben einem Float nur bei
                    // fester Breite (WPT `new-fc-beside-float-with-margin-rtl`).
                    let auto_breite_bfc = child_node.style.size.width == Length::Auto;
                    for _ in 0..12 {
                        let (lo, ro) = fz.kanten_im_band_roh(kante, size.height.max(0.0));
                        if lo.is_none() && ro.is_none() && !geschrumpft {
                            break;
                        }
                        let (l, r) = (lo.unwrap_or(0.0).max(0.0), ro.unwrap_or(0.0).max(0.0));
                        // Release 12.10.2026 (Agent CSS2): ein Kasten mit fester
                        // Breite, der ueber seinen Block hinausragt, darf auch
                        // dort keinen Float ueberlappen (die rohe Kante liegt
                        // dann ausserhalb). GEMESSEN vorher (WPT `floats/floats-
                        // wrap-bfc-with-margin-008/009`).
                        let ragt_in_float = ro.is_some_and(|rk| {
                            l.max(margin.left) + size.width > child_available.width - rk + 0.5
                        }) || (!auto_breite_bfc && lo.is_some_and(|lk| margin.left < 0.0 && margin.left < lk - 0.5));
                        if l <= 0.0 && r <= 0.0 && !geschrumpft && !ragt_in_float {
                            break;
                        }
                        // Die RAHMENkante meidet die Floats; die Raender
                        // duerfen unter ihnen liegen (§9.5).
                        // Ein negativer Rest heisst: er passt gar nicht daneben.
                        let roh = child_available.width - l.max(margin.left) - r.max(margin.right);
                        let platz = non_negative(roh);
                        let auto_breite = child_node.style.size.width == Length::Auto;
                        if roh >= 0.0 && auto_breite && (size.width > platz + 0.5 || (geschrumpft && size.width < platz - 0.5)) {
                            let min = sizes.min_inhalt_breite(ctx, child, 0);
                            if min <= platz + 0.5 {
                                let alt = std::mem::replace(sizes, Sizes::new(0));
                                let verfuegbar = Size::new(platz + margin.left + margin.right, child_available.height);
                                *sizes = sizes_lauf(ctx, alt, child, verfuegbar)?;
                                let neu = sizes.get(child).unwrap_or(size);
                                geschrumpft = l > 0.0 || r > 0.0;
                                if (neu.height - size.height).abs() > 0.5 {
                                    // Andere Hoehe, anderes Band: noch einmal.
                                    size = neu;
                                    continue;
                                }
                                size = neu;
                            }
                        }
                        // Mit negativem linkem Rand neben einem linken Float:
                        // kein Ausweichen nach rechts, die Rahmenkante laege auf
                        // dem Float (WPT `floats-wrap-bfc-with-margin-009`).
                        let ragt_in_float = ro.is_some_and(|rk| {
                            l.max(margin.left) + size.width > child_available.width - rk + 0.5
                        }) || (!auto_breite_bfc && lo.is_some_and(|lk| margin.left < 0.0 && margin.left < lk - 0.5));
                        if roh < 0.0 || (size.width > platz + 0.5 && (l > 0.0 || r > 0.0)) || ragt_in_float {
                            match fz.naechste_unterkante(kante, size.height.max(0.0)) {
                                Some(u) if u > kante => {
                                    kante = u;
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        break;
                    }
                    if kante > sum + oben {
                        sum = kante - oben;
                    }
                }
                // Ein LEERER Block (Hoehe 0) laesst seine beiden Raender
                // zusammenfallen und mit der Kette davor (8.3.1, „collapse
                // through"); dieselbe Regel in `flow::kind_platzieren`.
                let (unten, kette) = match vorige_unten {
                    Some(u) if !raeumt && size.height <= 0.0 && child_node.kind == NodeKind::Container => {
                        let k = rand_zusammen(u, rand_zusammen(t, b));
                        (k - u - oben, k)
                    }
                    _ => (b, b),
                };
                // Release 12.10.2026 (Agent CSS2): ein Kind ohne eigenen Kontext
                // sieht die Floats dieses Kontexts (neu gemessen, wenn sie es
                // betreffen), und seine eigenen Floats gelten danach hier.
                if darf_raender_durchlassen(ctx, child, child_node) && !auto_rand(ctx, child) {
                    let (dl, dr, dy) = kind_rahmen(child_node, margin, sum + oben, child_available.width)?;
                    let neu_erbe = (!fz.ist_leer()).then(|| fz.verschoben(dl, dr, dy)).filter(|z| !z.ist_leer());
                    let alt_erbe = ctx.float_erbe.borrow().get(&child).copied();
                    if neu_erbe != alt_erbe && (neu_erbe.is_none() || float_sensibel(ctx, child)) {
                        match neu_erbe {
                            Some(z) => {
                                ctx.float_erbe.borrow_mut().insert(child, z);
                            }
                            None => {
                                ctx.float_erbe.borrow_mut().remove(&child);
                            }
                        }
                        if ctx.float_nachmessen.get() < MAX_FLOAT_NACHMESSEN {
                            ctx.float_nachmessen.set(ctx.float_nachmessen.get() + 1);
                            let verfuegbar = sizes.verfuegbar.get(child.index()).copied().unwrap_or(child_available);
                            let alt = std::mem::replace(sizes, Sizes::new(0));
                            *sizes = sizes_lauf(ctx, alt, child, verfuegbar)?;
                            size = sizes.get(child).unwrap_or(size);
                        }
                    }
                    if let Some(aus) = ctx.float_aus.borrow().get(&child).copied() {
                        let ab = ctx.float_erbe.borrow().get(&child).map_or(0, |e| e.anzahl());
                        fz.uebernehme(&aus.zurueck(ab, dl, dr, dy));
                        hat_floats = true;
                    }
                }
                let leer = size.height <= 0.0 && child_node.kind == NodeKind::Container;
                kette_mit_freiraum = leer && (raeumt || (kette_mit_freiraum && vorige_unten.is_some()));
                if let Some(u) = sizes.float_ueberhang.get(&child.0).copied() {
                    float_tief = float_tief.max(sum + oben + u);
                }
                sum += oben + size.height + unten;
                vorige_unten = Some(kette);
                inhalt_davor = true;
            } else {
                sum += match axis {
                    Axis::X => size.width + margin.left + margin.right,
                    Axis::Y => size.height + margin.top + margin.bottom,
                };
            }
        }
    }
    // Release 09.10.2026: der untere Rand des letzten Kindes wandert nach
    // aussen, wenn der Knoten unten weder Rahmen noch Fuellung hat und
    // seine Hoehe aus dem Inhalt kommt (CSS 2.1, 8.3.1).
    let mut unten_hoch: Option<f32> = None;
    if durchlass
        && !zeile.gleich
        && !kette_mit_freiraum
        && feste_hoehe.is_none()
        && padding.bottom <= 0.0
        // Release 13.10.2026 (Agent CSS2): `min-height` haelt den Rand nur auf,
        // wenn sie die Hoehe bestimmt (der Inhalt kleiner ist). GEMESSEN vorher
        // (WPT `margin-padding-clear/margin-collapse-025`, `-037`): mit
        // `min-height` unter der Inhaltshoehe blieb der Rand des letzten Kindes
        // im Kasten; `normal-flow/margin-collapse-min-height-002` haelt ihn bei
        // groesserer `min-height` drinnen.
        && let Some(u) = vorige_unten
        && grenzen(ctx, id).2.is_none_or(|m| m <= 0.0 || sum - u >= m - 0.01)
    {
        sum -= u;
        unten_hoch = Some(u);
    }
    if kollaps {
        if oben_hoch.is_some() || unten_hoch.is_some() {
            sizes.hochgezogen.insert(id.0, (oben_hoch, unten_hoch));
        } else {
            sizes.hochgezogen.remove(&id.0);
        }
    }
    sum += zeile.hoehe();
    // Ein eigener Formatierungskontext umschliesst seine Floats (§10.6.7),
    // auch die seiner Nachfahren ohne eigenen Kontext; ohne eigenen Kontext
    // reichen sie ueber den Kasten hinaus und werden dem Eltern gemeldet.
    // Release 12.10.2026 (Agent CSS2): die eigenen Floats (ohne die geerbten)
    // fuer die folgenden Geschwister; ein eigener Kontext behaelt sie.
    if kollaps && durchlass && fz.anzahl() > erbe_n {
        ctx.float_aus.borrow_mut().insert(id, fz.zurueck(erbe_n, 0.0, 0.0, 0.0));
    } else {
        ctx.float_aus.borrow_mut().remove(&id);
    }
    let float_bis = float_unten.0.max(float_unten.1).max(float_tief);
    let float_bis = if kollaps && !durchlass { float_bis.max(fz.mindest_hoehe()) } else { float_bis };
    if kollaps && !durchlass {
        sum = sum.max(float_bis);
        sizes.float_ueberhang.remove(&id.0);
    } else if kollaps && float_bis > sum {
        sizes.float_ueberhang.insert(id.0, float_bis + padding.top);
    } else {
        sizes.float_ueberhang.remove(&id.0);
    }
    // added 04.10.2026, Rolle LAYOUT (LAYOUT_2): die Tabellenrechnung gewinnt
    // gegen die Blockrechnung. `zeile` ist zu diesem Zeitpunkt leer, weil die
    // Schleife oben gar nicht gelaufen ist — die `+=` darueber ist damit ein
    // `+ 0`, und sie bleibt stehen, damit der Normalfall unveraendert ist.
    if let Some(h) = tabellen_hoehe {
        sum = h;
    }
    // P2: die Grenzen gelten fuer die BENUTZTE Groesse, nicht nur fuer
    // eine gesetzte. CSS 2.1, 10.4: "used width = max(min-width,
    // min(width, max-width))" — und `width: auto` heisst hier
    // "die Summe der Kinder".
    let (min_b, max_b, min_h, max_h) = grenzen(ctx, id);
    //
    // L4 (03.10.2026 23:2x): AUCH HIER GEGEN DEN INHALTSKASTEN.
    //
    // GEMESSENER FEHLER, dieselbe Seite wie oben, aber mit `height: auto`:
    // mit gesetzter Hoehe ging es durch `sizes.set(Size::new(width, …))` und
    // war richtig (Rahmen 850 bei x = 215), ohne Hoehe aber ueber
    // `SizeTask::Sum` — und dort stand dreimal `clamp(…, min_b, max_b)` auf
    // der RAHMENbreite. Gemessen mit `height: auto`, `max-width: 780px`,
    // `padding: 0 35px`: Rahmen 780 statt 850, Inhalt 710 statt 780, und
    // der Inhalt stand bei x = 285 statt 250.
    //
    // §10.4 gilt fuer die `min-*`/`max-*` genauso wie fuer `width` selbst:
    // sie rechnen gegen den Inhaltskasten. Also hier dasselbe Muster wie
    // in `quer`/`hoch` — Fuellung abziehen, klemmen, Fuellung addieren.
    // Fuer `padding: 0` ist es der alte Ausdruck.
    let border_box = ctx.stil(id).is_some_and(|p| p.border_box);
    let abziehen = |v: Option<f32>, abstand: f32| v.map(|v| {
        non_negative(if border_box { v - abstand } else { v })
    });
    let (min_b, max_b, min_h, max_h) = (
        abziehen(min_b, padding.left + padding.right),
        abziehen(max_b, padding.left + padding.right),
        abziehen(min_h, padding.top + padding.bottom),
        abziehen(max_h, padding.top + padding.bottom),
    );
    // Release 14.10.2026: `min-height`/`max-height: stretch` and the widths
    // also here (as in `enter`). MEASURED before: WPT css-sizing
    // `stretch/block-height-001` (`min-height: stretch` in a 100 px block:
    // 20 instead of 100 px).
    let (min_b, max_b, min_h, max_h) = stretch_limits(ctx, sizes, id, node, padding, (min_b, max_b, min_h, max_h));

    let q_b = |w: f32| {
        clamp(non_negative(w - padding.left - padding.right), min_b, max_b)
            + padding.left
            + padding.right
    };
    let q_h = |h: f32| {
        clamp(non_negative(h - padding.top - padding.bottom), min_h, max_h)
            + padding.top
            + padding.bottom
    };
    // L5 (02.10.2026 13:5x): DIE BREITE EINES POSITIONIERTEN KASTENS
    // OHNE `width`.
    //
    // CSS 2.1, 10.3.7 woertlich:
    //
    //     used width = min(max(preferred minimum width, available
    //                         width), preferred width)
    //
    // GEMESSENER BEFUND, was vorher passierte
    // (`tests/l5_absolute.rs`, `l5_absolute_ohne_breite_bekommt_
    // shrink_to_fit`): eine absolute Box ohne `width` bekam 600 px —
    // die Inhaltsbreite des enthaltenden Blocks. GEMESSEN korrekt waeren
    // 60 px, die Breite ihres 60-px-Kindes.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die drei Groessen SO genommen sind:
    //
    //  * `bevorzugt` = das MAXIMUM der Kinderbreiten. In einer Spalte
    //    stehen die Kinder untereinander, die bevorzugte Breite ist also
    //    die breiteste, nicht die Summe. (Die Summe gehoert in den
    //    Zeilenzweig — dort steht sie seit P12 in `bevorzugt`.)
    //  * `verfuegbar` = `child_available.width`, also die Breite, gegen
    //    die die Kinder gemessen wurden.
    //  * `geringste` = `sizes.min_aggregat(node, Direction::Column)`,
    //    dieselbe Quelle wie der Boden eine Zeile hoeher. Die
    //    min-content-Breite ist die groesste unumbruchbare Stelle im
    //    Inhalt; sie zu nehmen ist die untere Schranke der Formel.
    //
    // GEMESSENE ENTSCHEIDUNG, warum das HIER und nicht im `Enter`-Zweig:
    // Hier sind die Kinder gemessen, dort nicht. Das ist genau das
    // Argument, das der Kommentar zum Boden (09:04) fuer `mindest`
    // macht — und der Grund, warum es als Feld im Aufgabentyp steht
    // statt als Zeile im Aufbau.
    let schrumpf_breite = |bevorzugt: f32, geringste: f32| {
        let verfuegbar = non_negative(child_available.width);
        clamp(
            non_negative(bevorzugt.min(verfuegbar.max(geringste))),
            min_b,
            max_b,
        )
    };
    let size = match axis {
        Axis::Y => {
            // L5 (13:5x): eine GESETZTE Hoehe gewinnt gegen die
            // Kindersumme. GEMESSENER BEFUND, warum das hier noetig ist:
            // Der `Sum`-Zweig wird fuer shrink-to-fit auch dann gestapelt,
            // wenn `height` gesetzt ist — sonst gaebe es keinen Weg, die
            // Breite nachzutragen. Ohne diese Zeile wuerde die gesetzte
            // Hoehe durch die Kindersumme ersetzt.
            let h = match feste_hoehe {
                Some(v) if sizes.verhaeltnis_wachsend.contains(&id.0) => non_negative(v).max(non_negative(sum + padding.top + padding.bottom)),
                Some(v) => non_negative(v),
                None => non_negative((sum + padding.top + padding.bottom).max(mindest)),
            };
            // Release 14.10.2026: `min-height`/`max-height` as a content
            // keyword is the content height (see `height_keyword_limits`).
            let h = match ctx.stil(id).map_or(0, |s| s.height_keyword_limits) {
                0 => h,
                bits => {
                    let inhalt = non_negative(sum + padding.top + padding.bottom);
                    let h = if bits & 2 != 0 { h.min(inhalt) } else { h };
                    if bits & 1 != 0 { h.max(inhalt) } else { h }
                }
            };
            // Release 11.10.2026 (Agent CSS2): die natuerliche Hoehe einer Zelle
            // (siehe `Sizes::natuerlich`).
            if ctx.stil(id).is_some_and(|s| s.anzeige == narsil_layout::plan::Anzeige::TabelleZelle) {
                sizes.natuerlich.insert(id.0, non_negative(sum + padding.top + padding.bottom));
            }
            let tabelle = schrumpft
                && ctx
                    .plan
                    .and_then(|p| p.hole(id))
                    .is_some_and(|st| st.anzeige == narsil_layout::plan::Anzeige::Tabelle);
            let b = if tabelle {
                // Release 11.10.2026: die Tabelle schrumpft auf die Summe ihrer
                // Spalten (Maxima; geringstens die Minima) samt `border-spacing`
                // zwischen den Spalten — dieselbe Rechnung wie `lege_tabelle`.
                // GEMESSEN vorher (WPT `table-anonymous-objects`): die Tabelle
                // war so breit wie ihr Block, und `lege_tabelle` verteilte den
                // ganzen Rest auf die Spalten.
                let (geringste, bevorzugt) = tabellen_spaltenbreiten(ctx, sizes, node);
                schrumpf_breite(bevorzugt + padding.left + padding.right, geringste + padding.left + padding.right)
            } else if schrumpft {
                // P9 (05.10.2026): die BEVORZUGTE Breite ist die
                // **Max-Content**-Breite der Kinder (`bevorzugte_breite`),
                // NICHT die benutzte Breite (`sizes.get().width`).
                //
                // GEMESSEN (Muster-Seite): ein `<span style="display:inline-
                // block">` mit dem Text „A" wurde mit `sizes.get().width`
                // **876 px** breit (= die ganze Zeile, weil das Textkind
                // gegen die volle Breite gemessen wurde) und stapelte.
                // `bevorzugte_breite` liefert die Breite, die der Inhalt
                // OHNE Umbruch braucht — genau die `preferred width` der
                // Formel aus CSS 2.1 §10.3.7/§10.3.9.
                let inline_block = ctx
                    .plan
                    .and_then(|p| p.hole(id))
                    .is_some_and(inline_ebene);
                // Release 08.10.2026: INLINE-Kinder (Text, inline-block)
                // stehen in EINER Zeile nebeneinander — ihre Breiten werden
                // addiert, ein Block-Kind beendet die Zeile (CSS 2.1
                // §10.3.7: die bevorzugte Breite ist die ohne Umbruch).
                // GEMESSEN vorher: das Maximum der Kinder — in
                // `<button><span>Platform</span> <svg/></button>` passte das
                // Symbol nicht mehr neben den Text und brach um.
                let mut zeile_summe = 0.0f32;
                let mut bevorzugt = 0.0f32;
                let mut schwebend = 0.0f32;
                for &c in node.children.iter() {
                    let plan = ctx.stil_oder_standard(c);
                    if !plan.wird_gezeichnet()
                        || matches!(
                            plan.position,
                            narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest
                        )
                    {
                        continue;
                    }
                    if inline_block {
                        // P9: Max-Content (siehe `max_content_breite`).
                        let w = sizes.max_content_breite(ctx, c, 0);
                        let inline = ctx
                            .document
                            .get(c)
                            .is_some_and(|k| crate::flow::kind_ist_inline(ctx, c, &k.kind));
                        if inline {
                            zeile_summe += w;
                        } else {
                            bevorzugt = bevorzugt.max(zeile_summe).max(w);
                            zeile_summe = 0.0;
                        }
                    } else {
                        // L5: fuer POSITIONIERTE Kaesten bleibt es die
                        // benutzte Kindbreite (`l5_absolute` misst
                        // genau das).
                        // Release 11.10.2026 (Agent CSS2): samt den
                        // waagerechten Raendern des Kindes (CSS 2.1 §10.3.7:
                        // die bevorzugte Breite ist die der Randbox).
                        // GEMESSEN vorher (WPT `margin-001..009`): der
                        // absolute Kasten war um beide Raender zu schmal.
                        let rand = ctx
                            .document
                            .get(c)
                            .and_then(|k| resolve_spacing(k.style.margin, child_available.width, c, "margin").ok())
                            .unwrap_or(Edges::ZERO);
                        // Eine Zeilenbox des Seitenbauers traegt ihren Inhalt bei,
                        // nicht ihre Ausrichtungsbreite (siehe `ist_zeilenbox`;
                        // WPT `CSS2/text/text-align-white-space-005`).
                        // Release 13.10.2026 (Agent CSS2): ebenso ein Block-Kind —
                        // seine benutzte Breite ist die verfuegbare, sein Beitrag
                        // zur Schrumpfbreite aber sein max-content (CSS 2.1
                        // §10.3.7). GEMESSEN vorher (WPT `margin-padding-clear/
                        // margin-collapse-102`): ein leerer Block mit Raendern machte
                        // den absoluten Kasten 784 statt 100 px breit.
                        let benutzt = sizes.get(c).map_or(0.0, |s| s.width);
                        let w = match ctx.document.get(c) {
                            Some(k) if ist_zeilenbox(ctx, c, k) && k.style.size.width != Length::Auto => {
                                sizes.max_content_breite(ctx, c, 0)
                            }
                            // Hoechstens die benutzte Breite: sie traegt schon
                            // `max-width` (GEMESSEN: WPT `positioning/absolute-
                            // non-replaced-width-019`, ein Float mit `max-width:
                            // 4em`).
                            Some(k)
                                if k.kind == NodeKind::Container
                                    && k.ersetzt.is_none()
                                    && !crate::flow::kind_ist_inline(ctx, c, &k.kind) =>
                            {
                                sizes.max_content_breite(ctx, c, 0).min(benutzt)
                            }
                            _ => benutzt,
                        } + rand.left + rand.right;
                        // Release 13.10.2026 (Agent CSS2): aufeinanderfolgende
                        // Inline-Kinder (Texte, `inline-block`, `inline-table`)
                        // stehen in EINER Zeile — ihre bevorzugte Breite ist die
                        // Summe, ohne kollabierbaren Leerraum am Zeilenrand (CSS
                        // 2.1 §10.3.7, §16.6.1; wie `bevorzugte_breite`).
                        // GEMESSEN vorher (WPT `tables/table-anonymous-objects-
                        // 168..196`): `<span>a <cell>b</cell><cell>c</cell>
                        // d</span>` im absoluten Kasten stand in vier Zeilen.
                        let inline = ctx
                            .document
                            .get(c)
                            .is_some_and(|k| crate::flow::kind_ist_inline(ctx, c, &k.kind));
                        if inline {
                            if crate::flow::kollabierer_leerraum(ctx, c) {
                                if zeile_summe > 0.0 {
                                    schwebend += w;
                                }
                            } else {
                                zeile_summe += schwebend + w;
                                schwebend = 0.0;
                            }
                        } else {
                            bevorzugt = bevorzugt.max(zeile_summe).max(w);
                            zeile_summe = 0.0;
                            schwebend = 0.0;
                        }
                    }
                }
                let bevorzugt = bevorzugt.max(zeile_summe) + padding.left + padding.right;
                // Release 11.10.2026: die bevorzugte Breite ist nie kleiner
                // als die geringste (CSS 2.1 §10.3.7: `min(max(geringste,
                // verfuegbar), bevorzugt)` setzt bevorzugt >= geringste
                // voraus). Fuer positionierte Kaesten ist `bevorzugt` die
                // BENUTZTE Kindbreite (L5) — in einem 0 px breiten Block ist
                // die eines Textes 0. GEMESSEN vorher: WPT css-grid
                // `abspos/empty-grid-001` (`XXXX` in einem leeren
                // `min-content`-Raster: 0 statt 40 px breit, sichtbar seit
                // die Masse nur noch aus dem aeussersten Kasten kommen).
                // GEMESSEN (11.10.2026): die min-content-Breite des ganzen
                // Inhalts hier als Untergrenze rechnete zyklische Prozent-
                // Innenabstaende mit (WPT css-sizing
                // `abspos-auto-sizing-fit-content-percentage-003`: 150 statt
                // 100 px); deshalb nur die Texte (gleich darunter).
                let geringste = sizes.min_aggregat(ctx, node, Direction::Column);
                // Ein ABSOLUTER Kasten: dazu die min-content-Breite seiner
                // Text-Kinder (Texte und Zeilenboxen; `min_aggregat` zaehlt
                // sie nicht). GEMESSEN vorher: WPT css-grid
                // `abspos/empty-grid-001` (`XXXX` in 0 px: 0 statt 40 px).
                let absolut = ctx.plan.and_then(|p| p.hole(id)).is_some_and(|st| {
                    matches!(st.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)
                });
                let geringste = if absolut {
                    let mut text = 0.0f32;
                    for &c in node.children.iter().take(crate::plan::MAX_KINDER) {
                        match ctx.document.get(c) {
                            Some(k) if k.kind == NodeKind::Text => text = text.max(sizes.min_of(c)),
                            Some(k) if ist_zeilenbox(ctx, c, k) => text = text.max(sizes.min_inhalt_breite(ctx, c, 1)),
                            _ => {}
                        }
                    }
                    geringste.max(if text > 0.0 { text + padding.left + padding.right } else { 0.0 })
                } else {
                    geringste
                };
                schrumpf_breite(bevorzugt.max(geringste), geringste)
            } else if feste_hoehe.is_some() {
                // Release 12.10.2026 (Agent CSS2): mit gesetzter Hoehe laeuft
                // `Sum` nur fuer Floats und durchgereichte Raender — die Breite
                // hat der Eintritt schon geklemmt (`quer`, mit Prozenten gegen
                // den enthaltenden Block); `grenzen` kennt nur Pixel. GEMESSEN
                // vorher (WPT `normal-flow/min-width-094`): `min-width: 100%`
                // im 1-in-Block wurde 800 px breit.
                cross
            } else {
                q_b(cross)
            };
            // Release 14.10.2026: a shrink-to-fit box (float, inline-block,
            // absolutely positioned) with `aspect-ratio` and `height: auto`
            // takes the height from its final width; the contents still
            // count as the automatic minimum unless it is a scroll container
            // (CSS Sizing 4 §5.1/§5.2). MEASURED before: WPT css-sizing
            // `aspect-ratio/block-aspect-ratio-019` (a float around a 100 px
            // child was 100 x 0).
            let h = match ctx.stil(id) {
                Some(st)
                    if schrumpft
                        && feste_hoehe.is_none()
                        && node.ersetzt.is_none()
                        && node.style.size.height == Length::Auto
                        && st.seitenverhaeltnis.is_finite()
                        && st.seitenverhaeltnis != 0.0 =>
                {
                    let v = st.seitenverhaeltnis.abs();
                    let rand_x = padding.left + padding.right;
                    let rand_y = padding.top + padding.bottom;
                    let rh = if st.border_box && st.seitenverhaeltnis > 0.0 { b / v } else { non_negative(b - rand_x) / v + rand_y };
                    if st.rollkasten { rh } else { rh.max(h) }
                }
                _ => h,
            };
            Size::new(b, q_h(h))        }
        Axis::X => {
            //
            // P12 (05.10.2026, 13:45): DIE OBERGRENZE FEHLTE, und sie
            // ist die Ursache von 41 der 43 `seitlich-raus`-Meldungen
            // auf github.com.
            //
            // GEMESSEN (P12, 13:33/13:34, gespeichertes github-Dokument,
            // Werkzeug `probe/p12.sh` und `SEITLICH`-Ausgabe):
            //
            //   SEITLICH Weich 148 px knoten 573 kasten=[172,4733.6 876.00x0.00]
            //     ELTERN #572 [172,4733.6 0.00x88.00]  kinder=12  plan=Block
            //   SEITLICH Weich 160 px knoten 571 kasten=[160,4721.6 900.00x88.00]
            //     ELTERN #570 [148,4709.6 604.00x88.00]  kinder=2   plan=Flex
            //
            // Ein Kind von 876 px Breite in einem Elternteil von 0,00 px.
            //
            // `Direction::Row` heisst Flex, und ein Flex-Container mit
            // `width: auto` hat seine Breite per SHRINK-TO-FIT
            // (CSS Flexbox 1 §9.7, Schritt 1; CSS 2.1 §10.3.7):
            //
            //     benutzte Breite = min(max(geringste Breite,
            //                                 verfuegbare Breite),
            //                             bevorzugte Breite)
            //
            // Bisher stand dort nur die BEVORZUGTE Breite — die Summe
            // der Kinder — und `mindest` stand auf 0. Damit durfte der
            // Container breiter werden als der Platz, den er bekommen
            // hat. Genau das ist `seitlich-raus`.
            //
            // DIE KAPPE, und wovor sie NICHT schuetzt: `mindest` ist 0,
            // also wird die GERINGSTE Breite nicht gerechnet. Fuer
            // `available.width = 0` liefert die Kappe 0, die Spezifikation
            // den min-content. Das ist zu eng, aber nie zu weit — und
            // "nie breiter als der Platz" ist die Eigenschaft, um die es
            // hier geht. Die min-content-Rechnung ist eine eigene
            // Aufgabe (siehe PROBLEME.txt, 13:46).
            let verfuegbar = non_negative(child_available.width);
            let bevorzugt = non_negative(sum + padding.left + padding.right);
            // **`boden`, nicht `mindest`** — siehe die Begruendung oben
            // (09:04). Mit `mindest` steht hier 0, und `verfuegbar` ist
            // bei einem Flex-Item in einem 0-px-Eltern ebenfalls 0 —
            // **das ist genau der gemessene 0.00-Kasten auf spiegel.de.**
            let b = non_negative(bevorzugt.min(verfuegbar.max(boden)));
            Size::new(q_b(b), q_h(cross))
        }
    };
    sizes.set(id, size);
    Ok(())
}

/// Die Hoehe eines Knotens **in der Hauptrichtung**, wenn er zur Tabelle
/// gehoert — `None`, wenn er keiner ist.
///
/// added 04.10.2026, Rolle LAYOUT (LAYOUT_2).
///
/// # WAS HIER FALSCH WAR
///
/// `sum_children` addiert die Kinder **untereinander**, weil es von
/// **Bloecken** ausgeht. In einer Tabelle stimmt das an genau **einer**
/// Stelle nicht: die Zellen einer Zeile stehen **nebeneinander**. GEMESSEN
/// (`tests/tabelle_hoehe_ist.rs`, 2 Zeilen x 3 Zellen, 600 px, echtes HTML
/// mit `<tbody>`):
///
/// ```text
///   Zelle  198,67 x 19,00   <- 6 davon, richtig (flow.rs legt das Raster)
///   Zeile       600 x 19,00 <- richtig, aus `zeilen_rechtecke`
///   tbody       600 x 114,00 <- FALSCH
///   Tabelle     600 x 114,00 <- FALSCH
///   Absatz danach    y 114,00 <- 74 px zu tief
/// ```
///
/// 114 = 2 x (3 x 19) = 57 + 57: **jede** Zeile wurde so gerechnet, als
/// stuenden ihre drei Zellen untereinander.
///
/// # DIE ZWEI REGELN, UND WOHER SIE KOMMEN
///
/// CSS 2.1, 17.6 (Tables, "Table layout algorithm") und 17.5:
///
///  * **Die Zeile ist so hoch wie ihre hoechste Zelle.** 17.6 rechnet die
///    Zeilenhoehen als Maximum der Zellhoehen; die Platzphase macht dasselbe
///    (`lege_tabelle` -> `Tabelle::zeilenhoehen`, in `flow.rs`). Wenn die
///    Messphase addiert und die Platzphase maximumiert, widersprechen sich
///    dieselben zwei Phasen, die der P9-Kommentar fuer **Inline-Teile**
///    (Zeile 1540ff) ausdruecklich in Einklang bringen wollte.
///  * **Die Zeilen einer Tabelle stehen untereinander**, getrennt durch
///    `border-spacing` (2 px aus dem UA-Blatt, `BORDER_SPACING_UA`). Das
///    ist die einzige Stelle, an der die Tabelle in der Hauptrichtung etwas
///    **zusaetzlich** zu den Kindern braucht.
///
/// # WARUM DIE STRUKTUR UND NICHT DIE ROLLE
///
/// `Anzeige::TabelleZeile` traegt **beide** Rollen: `<tr>` **und**
/// `<tbody>` (`narsil-css/src/kaskade.rs:3647`). Ein Knoten ist genau dann eine
/// **Zeile**, wenn er **Zellen** als Kinder hat — dieselbe Unterscheidung,
/// die `flow.rs` in `zeilen_von_tabelle` macht (dort aus dem Grund, dass der
/// Parser ein `<tbody>` einsetzt). Diese Funktion fragt deshalb dieselbe
/// Frage, statt eine Rolle zu raten.
///
/// # WAS DIESE FUNKTION NICHT KANN
///
/// **Kein `rowspan`.** Eine Zelle mit `rowspan="2"` macht die erste Zeile
/// so hoch, dass sie beide ueberlappt; das entscheidet `lege_tabelle`
/// (`Tabelle::zeilenhoehen`) in `tabelle.rs`, und diese Rechnung sieht die
/// Zelle nur einzel fuer sich. Der Kasten der Zeile stimmt dann (er kommt
/// aus `zeilen_rechtecke`), nur ihre **Platzreservierung** in dieser Phase
/// nicht. Das ist eine Frage des Gitters, nicht des Aufrufers.
/// Release 11.10.2026: (Summe der Spalten-Minima, Summe der Spalten-Maxima)
/// einer Tabelle, je mit `border-spacing` zwischen den Spalten — die Eingaben
/// wie in `flow::lege_tabelle_alle` (`min_inhalt_breite`/`max_content_breite`
/// der Zellen, `colspan` gleichmaessig verteilt).
fn tabellen_spaltenbreiten(ctx: &Ctx<'_>, sizes: &Sizes, node: &Node) -> (f32, f32) {
    use narsil_layout::plan::Anzeige;
    let rolle = |k: NodeId| ctx.stil(k).map_or(Anzeige::Block, |s| s.anzeige);
    let mut zeilen: Vec<NodeId> = Vec::new();
    for &k in node.children.iter() {
        if rolle(k) != Anzeige::TabelleZeile {
            continue;
        }
        let gruppe: Vec<NodeId> = ctx
            .document
            .get(k)
            .map_or(Vec::new(), |g| g.children.iter().copied().filter(|&z| rolle(z) == Anzeige::TabelleZeile).collect());
        if gruppe.is_empty() {
            zeilen.push(k);
        } else {
            zeilen.extend(gruppe);
        }
    }
    let (mut mins, mut maxs): (Vec<f32>, Vec<f32>) = (Vec::new(), Vec::new());
    for z in zeilen.into_iter().take(10_000) {
        let Some(zeile) = ctx.document.get(z) else { continue };
        let mut spalte = 0usize;
        for &zelle in zeile.children.iter() {
            if rolle(zelle) != Anzeige::TabelleZelle {
                continue;
            }
            let n = ctx.stil(zelle).map_or(1, |s| s.colspan.max(1)) as usize;
            let mn = sizes.min_inhalt_breite(ctx, zelle, 0);
            let mx = sizes.max_content_breite(ctx, zelle, 0).max(mn);
            let n = n.min(1000);
            if mins.len() < spalte + n {
                mins.resize(spalte + n, 0.0);
                maxs.resize(spalte + n, 0.0);
            }
            for i in 0..n {
                mins[spalte + i] = mins[spalte + i].max(mn / n as f32);
                maxs[spalte + i] = maxs[spalte + i].max(mx / n as f32);
            }
            spalte += n;
        }
    }
    if maxs.is_empty() {
        return (0.0, 0.0);
    }
    let luecke = ctx.stil(node.id).map_or(crate::flow::BORDER_SPACING_UA, |s| s.luecke);
    let luecken = luecke * (maxs.len() - 1) as f32;
    (non_negative(mins.iter().sum::<f32>() + luecken), non_negative(maxs.iter().sum::<f32>() + luecken))
}

fn tabellen_hoehe_gerade_y(
    ctx: &Ctx<'_>,
    sizes: &Sizes,
    node: &Node,
    child_available: Size,
) -> Result<Option<f32>, LayoutError> {
    use narsil_layout::plan::Anzeige;

    let rolle = |k: NodeId| ctx.stil(k).map_or(Anzeige::Block, |s| s.anzeige);
    // Dieselbe Filterung wie die Block-Schleife: unsichtbare und
    // absolut/fest positionierte Kinder nehmen keinen Platz im Fluss.
    let kinder: Vec<NodeId> = node
        .children
        .iter()
        .copied()
        .filter(|&c| {
            let plan = ctx.stil_oder_standard(c);
            plan.wird_gezeichnet()
                && !matches!(
                    plan.position,
                    narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest
                )
        })
        .collect();
    // Die Hoehe, die dieses Kind im Fluss einnimmt — mit seinen Raendern,
    // so wie die Block-Schleife es rechnet.
    let hoehe_von = |c: NodeId| -> Result<f32, LayoutError> {
        let size = sizes.get(c).ok_or(LayoutError::UnknownNode(c))?;
        let cn = get_node(ctx, c)?;
        let m = resolve_spacing(cn.style.margin, child_available.width, c, "margin")?;
        Ok(size.height + m.top + m.bottom)
    };

    // 1. EINE ZEILE — sie hat Zellen. Hoehe = MAXIMUM (CSS 2.1, 17.6).
    let zellen: Vec<NodeId> = kinder
        .iter()
        .copied()
        .filter(|&c| rolle(c) == Anzeige::TabelleZelle)
        .collect();
    if !zellen.is_empty() {
        let hoechste = zellen
            .iter()
            .map(|&c| hoehe_von(c))
            .collect::<Result<Vec<f32>, LayoutError>>()?
            .into_iter()
            .fold(0.0f32, f32::max);
        return Ok(Some(hoechste));
    }

    // 2. EIN KASTEN, DER ZEILEN TRAEGT — die Tabelle selbst oder eine
    //    Zeilengruppe (`<tbody>`). Hoehe = SUMME der Zeilen plus der
    //    Abstand dazwischen.
    //
    //    **WARUM DER ABSTAND HIER UND NICHT IN DER TABELLE:** ein Knoten
    //    kann Zeilen ueber eine Gruppe hinweg tragen
    //    (`<table><tbody><tr>`), und dann trägt die **Gruppe** die Zeilen
    //    unmittelbar. Der Kasten, dessen Kinder **Zeilen** sind, ist
    //    genau der, der den Abstand dazwischen rechnen muss — sonst
    //    zaehlte ihn die Tabelle ein zweites Mal.
    let zeilen: Vec<NodeId> = kinder
        .iter()
        .copied()
        .filter(|&c| rolle(c) == Anzeige::TabelleZeile && crate::flow::ist_tabellen_zeile(ctx, c))
        .collect();
    if zeilen.is_empty() {
        // Keine Zeile, keine Zelle: das ist ein ganz gewoehnlicher Block.
        return Ok(None);
    }
    let mut summe = 0.0f32;
    for &z in zeilen.iter() {
        summe += hoehe_von(z)?;
    }
    // Release 11.10.2026: der senkrechte `border-spacing` der TABELLE (der
    // Knoten kann auch ihre Zeilengruppe sein, `<tbody>`).
    let mut tabelle = Some(node.id);
    for _ in 0..3 {
        match tabelle {
            Some(t) if rolle(t) != Anzeige::Tabelle => tabelle = ctx.document.get(t).and_then(|n| n.parent),
            _ => break,
        }
    }
    let abstand = tabelle
        .filter(|t| rolle(*t) == Anzeige::Tabelle)
        .and_then(|t| ctx.stil(t))
        .map_or(crate::flow::BORDER_SPACING_UA, |s| s.luecke_quer);
    let abstaende = zeilen.len().saturating_sub(1) as f32 * abstand;
    Ok(Some(summe + abstaende))
}

/// Die vier Grenzen eines Knotens aus dem Plan.
///
/// `None` heisst "nicht gesetzt". Fuer `min-*` ist "nicht gesetzt" nach
/// CSS 2.1, 10.4 gleich 0, fuer `max-*` gleich unendlich — beides
/// wird hier schon passend gemacht, damit `clamp` nicht Sonderfaelle
/// kennen muss.
///
/// P19 (20:37): der Prozentanteil gewinnt vor der Zahl aus der Kaskade.
///
/// `a` ist der Anteil (frisch gerechnet), `b` die Zahl der Kaskade
/// (gegen die Ansicht gerechnet und deshalb als Groesse falsch). Ein
/// vorhandener Anteil ist die bessere Auskunft; fehlt er, gilt `b`.
fn cmp_or(a: Option<f32>, b: Option<f32>) -> Option<f32> {
    a.or(b)
}

/// P19 (05.10.2026, 20:37): DIE PROZENTANTEILE, aufgeloest gegen den
/// Inhalt des Eltern.
///
/// GEMESSENER FEHLER (20:32): `img, video { max-width: 100% }` (nixos.org,
/// woertlich) mit einem 2000-px-Bild in einem 852-px-Eltern ergab
/// **900 px** — die Ansicht, nicht der Eltern. Der Weg dorthin: die
/// Kaskade rechnet `100%` sofort (`kaskade.rs::breiten_wert`), und ab
/// da ist es eine absolute Zahl.
///
/// `reference` ist der freie Platz fuer die Kinder, also der Inhalts-
/// kasten des Eltern (CSS 2.1 §10.2 nennt fuer Prozentangaben genau
/// ihn). `Some(100.0)` ergibt also 100 % von 852 = 852 px.
///
/// WAS DAS FUER DIE KLEMME HEISST, und es ist der wichtige Teil: nach
/// §10.4 ist die Formel `max(min-width, min(width, max-width))` — die
/// Klamme gilt fuer die **Inhaltsbreite**. Also wird der Prozentanteil
/// zu einer Zahl gemacht und die Zahl geklemmt; es bleibt bei EINEM
/// `clamp`, nicht bei zweien.
fn grenzen_prozent(
    ctx: &Ctx<'_>,
    id: NodeId,
    breite: f32,
    hoehe: f32,
) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
    let Some(p) = ctx.plan else {
        return (None, None, None, None);
    };
    let g = p.grenzen_prozent(id);
    let b = |v: Option<f32>, basis: f32| v.map(|x| basis * x / 100.0);
    (
        b(g.min_breite, breite),
        b(g.max_breite, breite),
        b(g.min_hoehe, hoehe),
        b(g.max_hoehe, hoehe),
    )
}

/// Release 08.10.2026: die vier Grenzen als RAHMENmasse (border-box),
/// Prozente gegen `bezug` (den Inhaltskasten des Eltern) — fuer den
/// Flex-Algorithmus, der mit Rahmenmassen rechnet. `fuellung` ist
/// Innenabstand plus Rahmen (waagerecht, senkrecht).
pub(crate) fn grenzen_rahmen(
    ctx: &Ctx<'_>,
    id: NodeId,
    bezug: Size,
    fuellung: (f32, f32),
) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
    let (min_b, max_b, min_h, max_h) = grenzen(ctx, id);
    let (pm_b, pa_b, pm_h, pa_h) = grenzen_prozent(ctx, id, bezug.width, bezug.height);
    let border_box = ctx.stil(id).is_some_and(|p| p.border_box);
    let rahmen = |v: Option<f32>, f: f32| v.map(|v| non_negative(if border_box { v } else { v + f }));
    (
        rahmen(cmp_or(pm_b, min_b), fuellung.0),
        rahmen(cmp_or(pa_b, max_b), fuellung.0),
        rahmen(cmp_or(pm_h, min_h), fuellung.1),
        rahmen(cmp_or(pa_h, max_h), fuellung.1),
    )
}

fn grenzen(ctx: &Ctx<'_>, id: NodeId) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
    // Die vier Werte stehen in der Nebentabelle `Grenzen`, gefuellt von
    // `narsil_css::layout_grenzen` (01.10.2026). Ein leerer Plan oder ein
    // Knoten ohne Eintrag bedeutet: keine Beschraenkung.
    match ctx.plan {
        Some(p) => {
            let g = p.grenzen(id);
            (g.min_breite, g.max_breite, g.min_hoehe, g.max_hoehe)
        }
        None => (None, None, None, None),
    }
}

/// P9: eine Inline-Zeile in der Messphase.
///
/// Nur Hoehen, keine Positionen: die Platzphase setzt die Kaesten, hier
/// geht es darum, wie hoch der Eltern wird. Die Zahlen kommen aus
/// [`flow::inline_teil`] — dieselbe Regel an derselben Stelle, sonst
/// widersprechen sich die Phasen (siehe `sum_children`).
struct MessZeile {
    /// `true`, sobald ein Teil in der Zeile liegt.
    gleich: bool,
    /// Wie breit die Zeile belegt ist (fuer den Umbruch).
    braucht: f32,
    /// Das groesste Mass eines Teils = die Hoehe der Zeile.
    hoch: f32,
    /// Release 11.10.2026 (Agent CSS2): groesster Grundlinienabstand und
    /// groesster Rest darunter (Strut eingerechnet).
    oben: f32,
    unten: f32,
}

impl MessZeile {
    fn mit_strut(strut: (f32, f32)) -> Self {
        Self {
            gleich: false,
            braucht: 0.0,
            hoch: 0.0,
            oben: strut.0,
            unten: strut.1,
        }
    }

    /// Ein Teil auf der Grundlinie: `g` ab seiner Oberkante, Hoehe `h`.
    fn grundlinie(&mut self, g: f32, h: f32) {
        if g.is_finite() && h.is_finite() {
            self.oben = self.oben.max(g);
            self.unten = self.unten.max(h - g);
        }
    }

    fn anhaenge(&mut self, breite: f32, hoehe: f32) {
        self.gleich = true;
        self.braucht += breite;
        self.hoch = self.hoch.max(hoehe);
    }

    /// Die Beitragshoehe: eine leere Zeile zaehlt 0, sonst das groesste
    /// Mass darin. Ohne das wuerde jede leere Zeile die Zeilenhhoehe
    /// einmal zu viel dazuzaehlen.
    fn hoehe(&self) -> f32 {
        if self.gleich { self.hoch.max(self.oben + self.unten) } else { 0.0 }
    }
}

/// CSS 2.1, 10.4: `used = min(max-width, max(min-width, width))`.
fn clamp(wert: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    let mut w = wert;
    if let Some(m) = min {
        w = w.max(m);
    }
    if let Some(m) = max {
        w = w.min(m);
    }
    // `max` gewinnt, wenn es kleiner ist als `min` (CSS 2.1, 10.4:
    // "min-width wins over max-width").
    if let (Some(a), Some(b)) = (min, max) {
        w = w.max(a).min(b.max(a));
    }
    non_negative(w)
}

/// Wie [`clamp`], aber fuer eine Laenge, die noch gar nicht da ist.
///
/// `None` heisst "keine gesetzte Groesse" — dann wird nichts begrenzt,
/// weil die Beschraenkung spaeter auf die berechnete Groesse wirkt (siehe
/// [`sum_children`]).
fn clamp_length(wert: Option<f32>, min: Option<f32>, max: Option<f32>) -> Option<f32> {
    wert.map(|w| clamp(w, min, max))
}

/// Berechnet die BORDER-BOX-Groesse EINES jeden Knotens — iterativ.
///
/// # Auto-Aufloesung
///
/// * Haupt-Achse: Summe der Kinder (Border plus deren Margins) plus eigenes
///   Padding.
/// * Quer-Achse: Content-Box des Elterns abzueglich der eigenen Margins —
///   die MARGIN-BOX des Kindes fuellt die Content-Box des Elterns.
/// * `page`: fuellt den verfuegbaren Raum (Viewport) abzueglich der Margins.
/// * Text: fixe Inhaltsbreite (gemessen) und Zeilenhoehe.
///
/// Explizite `width`/`height` gewinnen immer. Das Ergebnis ist nie `NaN`
/// und nie negativ. Jeder Knoten wird EXAKT EINMAL gemessen.
///
/// # Fehler
///
/// [`LayoutError::UnknownNode`] und [`LayoutError::InvalidLength`].
/// Kein Panic und kein Stack-Overflow, auch nicht bei 1000 verschachtelten
/// Containern — die Aufgaben liegen auf einem Heap-Stack.
pub(crate) fn compute_sizes(
    ctx: &Ctx<'_>,
    root: NodeId,
    viewport: Size,
) -> Result<Sizes, LayoutError> {
    // M7: die Liste der Knoten mit kaputter Angabe haengt an `Sizes`,
    // weil sie zum Lauf gehoert und `layout_mit_plan` sie dort liest.
    let mut neu = Sizes::new(ctx.document.len());
    neu.fenster = viewport;
    let sizes = sizes_lauf(ctx, neu, root, viewport)?;
    absolute_nachmessen(ctx, sizes, root, viewport)
}

/// Release 10.10.2026: hoechstens so viele absolute Teilbaeume werden je
/// Layout nachgemessen (M3).
const MAX_NACHMESSEN: usize = 4096;

/// Release 10.10.2026: ABSOLUTE KAESTEN NACHMESSEN (CSS 2.1 §10.6.4).
///
/// Ein absoluter Kasten wird im Fluss seines Elternteils gemessen — zu dem
/// Zeitpunkt steht die Hoehe seines enthaltenden Blocks oft noch nicht fest
/// (sie entsteht aus dessen Inhalt). Haengt seine Hoehe an diesem Block
/// (`top` UND `bottom` bei `height: auto`, oder `height: P%`), wird sie hier
/// gegen den FERTIG gemessenen Block gerechnet und der Teilbaum mit ihr als
/// fester Hoehe neu gemessen — sonst loesen sich `height: P%` darin gegen
/// den falschen Platz auf.
///
/// GEMESSEN vorher (youtube.com, das Muster jeder Vorschau mit festem
/// Seitenverhaeltnis): `<ytd-thumbnail>` mit `:before{padding-top:56.25%}`,
/// darueber `<a style="position:absolute;top:0;bottom:0">`, darin
/// `<img style="height:100%">` — das Bild wurde 1 px (bzw. die Fensterhoehe)
/// statt 281 px hoch.
///
/// Release 11.10.2026 (Agent CSS2): `width`/`height: fit-content` (und
/// `min-`/`max-content`) dehnen nicht zwischen den Versaetzen
/// (`LayoutStil::breite_inhalt`/`hoehe_inhalt`; der UA-Stil von `<dialog>`).
fn absolute_nachmessen(ctx: &Ctx<'_>, mut sizes: Sizes, root: NodeId, viewport: Size) -> Result<Sizes, LayoutError> {
    // Release 11.10.2026: dazu die BREITE des enthaltenden Blocks — fuer
    // `width: P%` absoluter Kaesten (CSS 2.1 §10.3.7: gegen den enthaltenden
    // Block, nicht gegen den Eltern).
    let mut stapel: Vec<(NodeId, f32, f32)> = vec![(root, viewport.height, viewport.width)];
    let mut gemessen = 0usize;
    while let Some((id, block_hoehe, block_breite)) = stapel.pop() {
        let Ok(node) = get_node(ctx, id) else { continue };
        let Some(groesse) = sizes.get(id) else { continue };
        let p = ctx.stil_oder_standard(id);
        let positioniert = !matches!(p.position, narsil_layout::PositionArt::Statisch);
        if matches!(p.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)
            && gemessen < MAX_NACHMESSEN
        {
            let bezug = if matches!(p.position, narsil_layout::PositionArt::Fest) { viewport.height } else { block_hoehe };
            // Release 12.10.2026 (Agent CSS2): siehe `LayoutStil::cb_inline`.
            let bezug = if p.cb_inline && !matches!(p.position, narsil_layout::PositionArt::Fest) { 0.0 } else { bezug };
            // Release 11.10.2026: im positionierten Raster ist der Rasterbereich
            // der enthaltende Block (CSS Grid 1 §9.1, `gitter.rs`).
            let bezug = ctx.gitter_abs.borrow().get(&id).map_or(bezug, |g| g.height);
            let verfuegbar = sizes.verfuegbar.get(id.index()).copied().unwrap_or(viewport);
            let mut stretch_edges = 0.0f32;
            let soll = match node.style.size.height {
                Length::Auto => match (p.oben, p.unten) {
                    // Release 11.10.2026 (Agent CSS2): `height: fit-content`
                    // dehnt nicht (siehe `LayoutStil::hoehe_inhalt`).
                    // Release 12.10.2026 (Agent CSS2): ein ersetztes Element
                    // auch nicht — es behaelt seine natuerliche Groesse (CSS 2.1
                    // §10.6.5, §10.3.8). GEMESSEN vorher (WPT `positioning/
                    // abspos-026`): ein 15-px-Bild mit `top`/`bottom` wurde
                    // 472 px gross.
                    // Release 14.10.2026: not for a box with `aspect-ratio`
                    // and a definite width — its height follows the ratio
                    // (CSS Sizing 4 §5.1). MEASURED before: WPT css-sizing
                    // `aspect-ratio/abspos-005` (`width: 100px; aspect-ratio:
                    // 1/1` between `top: 0` and `bottom: 0` was 500 px high).
                    (Some(o), Some(u))
                        if !p.hoehe_inhalt
                            && (node.ersetzt.is_none() || abs_explicit_stretch(ctx, id).1)
                            && abs_selbst_lage(ctx, id).1.is_none()
                            && !(node.ersetzt.is_none()
                                && p.seitenverhaeltnis.is_finite()
                                && p.seitenverhaeltnis != 0.0
                                && matches!(node.style.size.width, Length::Px(_))) =>
                    {
                        let rand = resolve_spacing(node.style.margin, verfuegbar.width, id, "margin")?;
                        Some(non_negative(bezug - o - u - rand.top - rand.bottom))
                    }
                    _ => None,
                },
                // Release 14.10.2026: `height: stretch` (the page writes
                // `100%`): the margin box fills the containing block minus the
                // insets (see `stretch_insets`).
                Length::Percent(_) if p.stretch_bits & 8 != 0 => {
                    // (The forced height is the content height; it is compared
                    // with the border box below, `stretch_edges`.)
                    let rand = resolve_spacing(node.style.margin, verfuegbar.width, id, "margin")?;
                    let fuell = resolve_spacing(node.style.padding, verfuegbar.width, id, "padding")?;
                    let r = rahmen_edges(&node.style.rahmen);
                    let (_, vh) = stretch_insets(ctx, id, node, verfuegbar.width);
                    stretch_edges = fuell.top + fuell.bottom + r.top + r.bottom;
                    Some(non_negative(bezug - vh - rand.top - rand.bottom - stretch_edges))
                }
                Length::Percent(pz) if pz.is_finite() => Some(non_negative(bezug * pz / 100.0)),
                // Release 15.10.2026 (Agent CSS2): `calc()` with a percentage
                // (`Length::Gemischt`) against the containing block as well.
                // MEASURED before (heise.de carousel fades, `height: calc(50% *
                // 2)` on an abspos `::before`): the viewport height was used.
                Length::Gemischt { px, prozent } if px.is_finite() && prozent.is_finite() => {
                    Some(non_negative(px + bezug * prozent / 100.0))
                }
                _ => None,
            };
            // Release 11.10.2026: eine Prozentbreite gegen die Breite des
            // enthaltenden Blocks (im positionierten Raster: der Rasterbereich).
            let breite_bezug = if matches!(p.position, narsil_layout::PositionArt::Fest) { viewport.width } else { block_breite };
            let breite_bezug = ctx.gitter_abs.borrow().get(&id).map_or(breite_bezug, |g| g.width);
            let breite_bezug = if p.cb_inline && !matches!(p.position, narsil_layout::PositionArt::Fest) { 0.0 } else { breite_bezug };
            // Release 11.10.2026: ebenso `width: auto` zwischen `left` und
            // `right` (§10.3.7 Regel 5: gegen den enthaltenden Block, nicht
            // gegen den Eltern). GEMESSEN vorher: WPT css-flexbox
            // `order-abs-children-painting-order` (Eltern 0 px breit, der
            // absolute Kasten 0 statt 100 px).
            let dehnt = node.style.size.width == Length::Auto
                && p.links.is_some()
                && p.rechts.is_some()
                && !p.breite_inhalt
                && (node.ersetzt.is_none() || abs_explicit_stretch(ctx, id).0)
                && abs_selbst_lage(ctx, id).0.is_none();
            let neue_breite = match node.style.size.width {
                Length::Percent(_) | Length::Gemischt { .. } if (breite_bezug - verfuegbar.width).abs() > 0.5 => {
                    Some(breite_bezug)
                }
                Length::Auto if dehnt && (breite_bezug - verfuegbar.width).abs() > 0.5 => Some(breite_bezug),
                _ => None,
            };
            let hoehe_neu = soll.filter(|s| s.is_finite() && (s + stretch_edges - groesse.height).abs() > 0.5);
            // Release 14.10.2026: `min-*`/`max-*: stretch` measure against the
            // containing block, so the box is measured once more with it as
            // the available size (see `stretch_limits`). MEASURED before: WPT
            // css-sizing `stretch/stretch-min-block-size-001` (an abspos box
            // with `inset-block: 0; min-block-size: stretch` was 45 instead of
            // 55 px, the parent's content box instead of its padding box).
            let streck_grenzen = p.stretch_bits & (2 | 4 | 16 | 32) != 0;
            let neue_breite = neue_breite.or_else(|| (streck_grenzen && (breite_bezug - verfuegbar.width).abs() > 0.5).then_some(breite_bezug));
            let hoehe_platz = (streck_grenzen && bezug.is_finite() && (bezug - verfuegbar.height).abs() > 0.5).then_some(bezug);
            if hoehe_neu.is_some() || neue_breite.is_some() || hoehe_platz.is_some() {
                gemessen += 1;
                if let Some(h) = hoehe_neu {
                    sizes.erzwungen.insert(id.0, h);
                }
                let h = hoehe_neu.or(hoehe_platz).unwrap_or(verfuegbar.height);
                sizes = sizes_lauf(ctx, sizes, id, Size::new(neue_breite.unwrap_or(verfuegbar.width), h))?;
            }
        }
        // Release 14.10.2026: the containing block of an absolutely
        // positioned box is the PADDING box (CSS 2.1 §10.1), without the
        // border, as for the width. MEASURED before: WPT css-sizing
        // `stretch/auto-margins-2` (`height: stretch` in a box with a 3px
        // border: 206 instead of 200 px).
        let eigene = sizes.get(id).map_or(0.0, |g| g.height) - rahmen_edges(&node.style.rahmen).top - rahmen_edges(&node.style.rahmen).bottom;
        let eigene_b = sizes.get(id).map_or(0.0, |g| g.width) - rahmen_edges(&node.style.rahmen).left - rahmen_edges(&node.style.rahmen).right;
        let fuer_kinder = if positioniert { eigene } else { block_hoehe };
        let fuer_kinder_b = if positioniert { eigene_b } else { block_breite };
        for &kind in node.children.iter().rev() {
            stapel.push((kind, fuer_kinder, fuer_kinder_b));
        }
    }
    Ok(sizes)
}

/// Release 11.10.2026: die feste Breite (Rahmen) eines Kastens mit
/// `width: <px>` nach seiner ANGABE (an `min-width`/`max-width` geklemmt),
/// wenn seine gemessene Breite vom Flex-Algorithmus gesetzt wurde
/// (`Ctx::flex_mass`) — die geflexte Breite ist kein Inhaltsbeitrag (CSS
/// Sizing 3 §5.2: der Beitrag eines Kastens mit fester Breite ist diese
/// Breite). GEMESSEN vorher: WPT css-flexbox
/// `flex-minimum-width-flex-items-014` (die auf 5 px geschrumpften
/// Elemente meldeten 5 statt 20 px).
pub(crate) fn flex_feste_breite(ctx: &Ctx<'_>, id: NodeId, node: &narsil_dom::Node) -> Option<f32> {
    let Length::Px(w) = node.style.size.width else { return None };
    if node.kind != NodeKind::Container || node.ersetzt.is_some() || !w.is_finite() {
        return None;
    }
    if !ctx.flex_mass.borrow().get(&id).is_some_and(|m| m.0.is_some()) {
        return None;
    }
    let fuell = resolve_spacing(node.style.padding, 0.0, id, "padding").unwrap_or(Edges::ZERO);
    let rahmen = rahmen_edges(&node.style.rahmen);
    let aussen = fuell.left + fuell.right + rahmen.left + rahmen.right;
    let border_box = ctx.stil(id).is_some_and(|p| p.border_box);
    let kasten = |v: f32| if border_box { v } else { v + aussen };
    let (min_b, max_b, _, _) = grenzen(ctx, id);
    let mut b = kasten(w.max(0.0));
    if let Some(m) = max_b {
        b = b.min(kasten(m));
    }
    if let Some(m) = min_b {
        b = b.max(kasten(m));
    }
    Some(b.max(aussen))
}

/// Release 11.10.2026: die Selbstausrichtung eines absolut positionierten
/// Kastens ausserhalb von Flex/Raster (CSS Box Alignment 3 §6, CSS Position 3
/// §4.1: im inset-modifizierten enthaltenden Block) — je Achse
/// (`justify-self`, `align-self`) eine LAGE (start, center, end, …), die den
/// Kasten nicht dehnt; `None` fuer `auto`/`normal`/`stretch`.
/// Release 14.10.2026: explicit `justify-self`/`align-self: stretch` of an
/// absolutely positioned box whose containing block is a block container
/// (horizontal, vertical); see `abs_selbst_lage`.
pub(crate) fn abs_explicit_stretch(ctx: &Ctx<'_>, id: NodeId) -> (bool, bool) {
    use narsil_layout::gitter::AusrLage as L;
    let Some(st) = ctx.stil(id) else { return (false, false) };
    if !matches!(st.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest) {
        return (false, false);
    }
    let Some(k) = st.gitter_kind.as_deref() else { return (false, false) };
    let w = k.justify_self.lage == L::Strecken;
    let h = k.align_self.lage == L::Strecken;
    if !w && !h {
        return (false, false);
    }
    let mut ancestor = ctx.document.get(id).and_then(|n| n.parent);
    let mut steps = 0;
    while let Some(v) = ancestor {
        steps += 1;
        let vs = ctx.stil(v);
        if vs.is_some_and(|s| s.position != narsil_layout::PositionArt::Statisch) || steps > 256 {
            if vs.is_some_and(|s| matches!(s.anzeige, narsil_layout::plan::Anzeige::Flex | narsil_layout::plan::Anzeige::Raster)) {
                return (false, false);
            }
            break;
        }
        ancestor = ctx.document.get(v).and_then(|n| n.parent);
    }
    (w, h)
}

pub(crate) fn abs_selbst_lage(ctx: &Ctx<'_>, id: NodeId) -> (Option<narsil_layout::gitter::Ausr>, Option<narsil_layout::gitter::Ausr>) {
    use narsil_layout::gitter::AusrLage as L;
    let Some(st) = ctx.stil(id) else { return (None, None) };
    if !matches!(st.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest) {
        return (None, None);
    }
    // Der enthaltende Block: der naechste positionierte Vorfahr. Ist er ein
    // Flex- oder Rastercontainer, richten `flexbox.rs`/`gitter.rs` aus (WPT
    // css-align `abspos/align-self-with-flex-grid-parent`: ein Flex-ELTERN,
    // der nicht positioniert ist, zaehlt nicht).
    let mut vorfahr = ctx.document.get(id).and_then(|n| n.parent);
    let mut schritte = 0;
    while let Some(v) = vorfahr {
        schritte += 1;
        let vs = ctx.stil(v);
        if vs.is_some_and(|s| s.position != narsil_layout::PositionArt::Statisch) || schritte > 256 {
            if vs.is_some_and(|s| matches!(s.anzeige, narsil_layout::plan::Anzeige::Flex | narsil_layout::plan::Anzeige::Raster)) {
                return (None, None);
            }
            break;
        }
        vorfahr = ctx.document.get(v).and_then(|n| n.parent);
    }
    let Some(k) = st.gitter_kind.as_deref() else { return (None, None) };
    let lage = |a: narsil_layout::gitter::Ausr| (!matches!(a.lage, L::Auto | L::Normal | L::Strecken)).then_some(a);
    (lage(k.justify_self), lage(k.align_self))
}

/// Release 11.10.2026: `justify-self` eines Blocks im Fluss eines
/// Blockcontainers (CSS Box Alignment 3 §6.1): eine LAGE (start, center,
/// end, left, right, …) macht ihn passend breit und richtet ihn aus; `None`
/// fuer `auto`/`normal`/`stretch` und ausserhalb eines Blockcontainers.
pub(crate) fn block_selbst_lage(ctx: &Ctx<'_>, id: NodeId) -> Option<narsil_layout::gitter::Ausr> {
    use narsil_layout::gitter::AusrLage as L;
    use narsil_layout::plan::Anzeige as A;
    let st = ctx.stil(id)?;
    if matches!(st.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)
        || st.schweben != narsil_layout::plan::FloatArt::Keins
        || inline_ebene(st)
        || !matches!(st.anzeige, A::Block | A::Flex | A::Raster)
    {
        return None;
    }
    let node = ctx.document.get(id)?;
    if node.style.size.width != Length::Auto || node.ersetzt.is_some() {
        return None;
    }
    let eltern = ctx.stil(node.parent?)?;
    if !matches!(eltern.anzeige, A::Block | A::InlineBlock) {
        return None;
    }
    let a = st.gitter_kind.as_deref()?.justify_self;
    (!matches!(a.lage, L::Auto | L::Normal | L::Strecken | L::Grundlinie | L::LetzteGrundlinie)).then_some(a)
}

/// Release 14.10.2026: the width contribution (border box) of a box with size
/// containment in the inline axis and `width: auto` (CSS Contain 2 §3.2): the
/// width from `contain-intrinsic-size` (else 0) plus padding and border,
/// clamped by `min-`/`max-width`; the contents do not count. MEASURED before:
/// WPT css-sizing `contain-intrinsic-size/contain-intrinsic-size-002`.
fn containment_width_contribution(ctx: &Ctx<'_>, id: NodeId, node: &Node) -> Option<f32> {
    let st = ctx.stil(id)?;
    if st.size_contained & 1 == 0 || node.kind != NodeKind::Container || node.ersetzt.is_some() || node.style.size.width != Length::Auto {
        return None;
    }
    let pad = resolve_spacing(node.style.padding, 0.0, id, "padding").unwrap_or(Edges::ZERO);
    let border = rahmen_edges(&node.style.rahmen);
    let edges_x = pad.left + pad.right + border.left + border.right;
    let (min_w, max_w, _, _) = grenzen(ctx, id);
    let as_border_box = |x: f32| if st.border_box { x.max(edges_x) } else { x + edges_x };
    let mut b = st.contain_size[0].unwrap_or(0.0) + edges_x;
    if let Some(m) = max_w {
        b = b.min(as_border_box(m));
    }
    if let Some(m) = min_w {
        b = b.max(as_border_box(m));
    }
    Some(b)
}

/// Release 14.10.2026: `min-*`/`max-*: stretch` (`LayoutStil::stretch_bits`)
/// as content-box limits for the passes after `enter` (`sum_children`, the
/// final clamp of `Format`): the margin box fills the space the box was
/// measured with. `kanten` are padding plus border.
fn stretch_limits(
    ctx: &Ctx<'_>,
    sizes: &Sizes,
    id: NodeId,
    node: &Node,
    kanten: Edges,
    limits: (Option<f32>, Option<f32>, Option<f32>, Option<f32>),
) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
    let bits = ctx.stil(id).map_or(0, |s| s.stretch_bits);
    if bits & (2 | 4 | 16 | 32) == 0 || node.kind != NodeKind::Container {
        return limits;
    }
    let av = sizes.verfuegbar_von(id);
    let rand = resolve_spacing(node.style.margin, av.width, id, "margin").unwrap_or(Edges::ZERO);
    let (vb, vh) = stretch_insets(ctx, id, node, av.width);
    let sw = non_negative(av.width - rand.left - rand.right - vb - kanten.left - kanten.right);
    let sh = av.height.is_finite().then(|| non_negative(av.height - rand.top - rand.bottom - vh - kanten.top - kanten.bottom));
    let (min_b, max_b, min_h, max_h) = limits;
    (
        if bits & 2 != 0 { Some(sw) } else { min_b },
        if bits & 4 != 0 { Some(sw) } else { max_b },
        if bits & 16 != 0 { sh.or(min_h) } else { min_h },
        if bits & 32 != 0 { sh.or(max_h) } else { max_h },
    )
}

/// Release 14.10.2026: the insets that shorten `stretch` for an absolutely
/// positioned box (horizontal, vertical; CSS Sizing 4 §3.1 with CSS Position
/// 3 §4.1): both set gives their sum, one set gives that one, none gives the
/// static position, i.e. the parent's padding when the parent is positioned
/// (then it is the containing block). An in-flow box returns (0, 0).
fn stretch_insets(ctx: &Ctx<'_>, id: NodeId, node: &Node, width: f32) -> (f32, f32) {
    let Some(st) = ctx.stil(id) else { return (0.0, 0.0) };
    if !matches!(st.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest) {
        return (0.0, 0.0);
    }
    let parent = node.parent.and_then(|e| ctx.document.get(e).map(|n| (e, n)));
    let static_offset = |horizontal: bool| -> f32 {
        let Some((e, n)) = parent else { return 0.0 };
        if ctx.stil(e).is_none_or(|s| matches!(s.position, narsil_layout::PositionArt::Statisch))
            || ctx.stil(e).is_some_and(|s| s.anzeige == narsil_layout::plan::Anzeige::Raster)
        {
            return 0.0;
        }
        let pad = resolve_spacing(n.style.padding, width, e, "padding").unwrap_or(Edges::ZERO);
        if horizontal { pad.left } else { pad.top }
    };
    let axis = |a: Option<f32>, b: Option<f32>, horizontal: bool| match (a, b) {
        (None, None) => static_offset(horizontal),
        (a, b) => a.unwrap_or(0.0) + b.unwrap_or(0.0),
    };
    (axis(st.links, st.rechts, true), axis(st.oben, st.unten, false))
}

/// Release 14.10.2026: the content-box height of a box when it is definite
/// before layout: a fixed `height`, or a percentage of a definite parent
/// height (the page keeps `height: P%` only against a definite parent).
fn definite_content_height(ctx: &Ctx<'_>, id: NodeId, depth: usize) -> Option<f32> {
    if depth > 32 {
        return None;
    }
    let node = ctx.document.get(id)?;
    let st = ctx.stil(id);
    let specified = match node.style.size.height {
        Length::Px(h) => h,
        Length::Percent(p) if st.is_none_or(|s| s.stretch_bits & 8 == 0) => definite_content_height(ctx, node.parent?, depth + 1)? * p / 100.0,
        _ => return None,
    };
    if !specified.is_finite() {
        return None;
    }
    if st.is_some_and(|s| s.border_box) {
        let pad = resolve_spacing(node.style.padding, 0.0, id, "padding").unwrap_or(Edges::ZERO);
        let border = rahmen_edges(&node.style.rahmen);
        Some((specified - pad.top - pad.bottom - border.top - border.bottom).max(0.0))
    } else {
        Some(specified.max(0.0))
    }
}

/// Release 14.10.2026: the border-box width limits (min, max) that
/// `min-height`/`max-height` transfer through `aspect-ratio` to a
/// non-replaced box with `width: auto` whose height is not definite (CSS
/// Sizing 4 §5.1); percentages need a definite parent height. MEASURED
/// before: WPT css-sizing `aspect-ratio/block-aspect-ratio-021` (`max-height:
/// 100px; aspect-ratio: 1/1; width: max-content` around a 200px child was
/// 200 px wide), `-024`, `-028` (`min-height: 100%`).
fn ratio_width_limits(ctx: &Ctx<'_>, sizes: &Sizes, id: NodeId, node: &Node) -> Option<(f32, f32)> {
    let st = ctx.stil(id)?;
    let v = st.seitenverhaeltnis;
    if node.kind != NodeKind::Container || node.ersetzt.is_some() || !v.is_finite() || v == 0.0 || node.style.size.width != Length::Auto {
        return None;
    }
    use narsil_layout::plan::TabellenRolle as R;
    let role = ctx.plan.map_or(R::Keine, |p| p.tabelle(id).rolle);
    if st.rollkasten || !matches!(role, R::Keine | R::Tabelle | R::InlineTabelle) {
        return None;
    }
    // A definite height is handled by `ratio_width_contribution`.
    if matches!(node.style.size.height, Length::Px(_)) || ratio_width_contribution(ctx, sizes, id, node).is_some() {
        return None;
    }
    let (_, _, min_h, max_h) = grenzen(ctx, id);
    let parent_h = node.parent.and_then(|e| definite_content_height(ctx, e, 0));
    let (pmin_h, pmax_h) = match parent_h {
        Some(h) => {
            let (_, _, a, b) = grenzen_prozent(ctx, id, 0.0, h);
            (a, b)
        }
        None => (None, None),
    };
    let g = ctx.plan.map(|p| p.grenzen_prozent(id));
    // A percentage without a definite parent height does not count (and its
    // px fallback against the viewport neither).
    let min_h = if g.is_some_and(|g| g.min_hoehe.is_some()) { pmin_h } else { min_h };
    let max_h = if g.is_some_and(|g| g.max_hoehe.is_some()) { pmax_h } else { max_h };
    if min_h.is_none() && max_h.is_none() {
        return None;
    }
    let pad = resolve_spacing(node.style.padding, 0.0, id, "padding").unwrap_or(Edges::ZERO);
    let border = rahmen_edges(&node.style.rahmen);
    let edges_x = pad.left + pad.right + border.left + border.right;
    let edges_y = pad.top + pad.bottom + border.top + border.bottom;
    let ratio = v.abs();
    let on_border_box = st.border_box && v > 0.0;
    let transfer = |h: f32| -> f32 {
        let content_h = if st.border_box { (h - edges_y).max(0.0) } else { h.max(0.0) };
        if on_border_box { ((content_h + edges_y) * ratio).max(edges_x) } else { content_h * ratio + edges_x }
    };
    let lo = min_h.filter(|h| h.is_finite()).map_or(0.0, transfer);
    let hi = max_h.filter(|h| h.is_finite()).map_or(f32::INFINITY, transfer);
    Some((lo, hi.max(lo)))
}

/// Release 14.10.2026: the width contribution (border box) of a non-replaced
/// box with `aspect-ratio`, `width: auto` and a fixed height: the height
/// transferred through the ratio (in the `box-sizing` box; with `auto &&
/// <ratio>` in the content box), at least the content, clamped by
/// `min-`/`max-width`.
fn ratio_width_contribution(ctx: &Ctx<'_>, sizes: &Sizes, id: NodeId, node: &Node) -> Option<f32> {
    let st = ctx.stil(id)?;
    let v = st.seitenverhaeltnis;
    if node.kind != NodeKind::Container || node.ersetzt.is_some() || !v.is_finite() || v == 0.0 || node.style.size.width != Length::Auto {
        return None;
    }
    // Not for scroll containers (WPT `aspect-ratio/grid-aspect-ratio-027`:
    // their contribution stays 0) and not for internal table boxes (rows,
    // cells: `aspect-ratio` does not apply there,
    // `aspect-ratio/table-element-001`).
    use narsil_layout::plan::TabellenRolle as R;
    let role = ctx.plan.map_or(R::Keine, |p| p.tabelle(id).rolle);
    if st.rollkasten || !matches!(role, R::Keine | R::Tabelle | R::InlineTabelle) {
        return None;
    }
    // A fixed height, or a percentage of a definite parent height (WPT
    // `aspect-ratio/intrinsic-size-006`). When the parent's height is only
    // known after layout (an absolutely positioned box between `top` and
    // `bottom`), the measured height counts: the page keeps `height: P%` only
    // against a definite height. MEASURED before: WPT css-align
    // `abspos/align-self-stretch-auto-margins-aspect-ratio` (the
    // `::before {height: 100%; aspect-ratio: 1}` contributed 0).
    let h = match node.style.size.height {
        Length::Px(h) => h,
        Length::Percent(p) if st.stretch_bits & 8 == 0 => match definite_content_height(ctx, node.parent?, 0) {
            Some(ph) => ph * p / 100.0,
            None => {
                let gemessen = sizes.get(id)?.height;
                if gemessen <= 0.0 {
                    return None;
                }
                if st.border_box {
                    gemessen
                } else {
                    let pad = resolve_spacing(node.style.padding, 0.0, id, "padding").unwrap_or(Edges::ZERO);
                    let border = rahmen_edges(&node.style.rahmen);
                    (gemessen - pad.top - pad.bottom - border.top - border.bottom).max(0.0)
                }
            }
        },
        _ => return None,
    };
    if !h.is_finite() || h < 0.0 {
        return None;
    }
    let pad = resolve_spacing(node.style.padding, 0.0, id, "padding").unwrap_or(Edges::ZERO);
    let border = rahmen_edges(&node.style.rahmen);
    let edges_x = pad.left + pad.right + border.left + border.right;
    let edges_y = pad.top + pad.bottom + border.top + border.bottom;
    let ratio = v.abs();
    let on_border_box = st.border_box && v > 0.0;
    // Clamp the height by `min-`/`max-height` first (WPT
    // `aspect-ratio/intrinsic-size-012`), then take the content height per
    // `box-sizing`.
    let (min_w, max_w, min_h, max_h) = grenzen(ctx, id);
    let h = h.min(max_h.unwrap_or(f32::INFINITY)).max(min_h.unwrap_or(0.0));
    let content_h = if st.border_box { (h - edges_y).max(0.0) } else { h };
    let width = if on_border_box { ((content_h + edges_y) * ratio).max(edges_x) } else { content_h * ratio + edges_x };
    let as_border_box = |x: f32| if st.border_box { x } else { x + edges_x };
    let mut b = width.max(vorab_min_inhalt(ctx, node) + edges_x);
    if let Some(m) = max_w {
        b = b.min(as_border_box(m));
    }
    if let Some(m) = min_w {
        b = b.max(as_border_box(m));
    }
    Some(b)
}

/// Release 12.10.2026: die min-content-Breite (Inhaltskasten) eines Kastens,
/// BEVOR seine Kinder gemessen sind: Kinder mit fester Breite mit ihrer
/// Angabe (samt Innenabstand, Rahmen, Raendern), Texte mit ihrem laengsten
/// Wort; das breiteste zaehlt. Fuer die automatische Mindestgroesse eines
/// Kastens mit `aspect-ratio` (CSS Sizing 4 §5.2).
fn vorab_min_inhalt(ctx: &Ctx<'_>, node: &Node) -> f32 {
    let mut w = 0.0f32;
    for &c in node.children.iter().take(crate::plan::MAX_KINDER) {
        let Some(k) = ctx.document.get(c) else { continue };
        let st = ctx.stil(c);
        if st.is_some_and(|s| !s.wird_gezeichnet() || matches!(s.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)) {
            continue;
        }
        let v = match k.kind {
            NodeKind::Text => crate::text::min_content_breite(ctx, k),
            _ => match k.style.size.width {
                Length::Px(px) if px.is_finite() => {
                    let rand = resolve_spacing(k.style.margin, 0.0, c, "margin").unwrap_or(Edges::ZERO);
                    let fuell = resolve_spacing(k.style.padding, 0.0, c, "padding").unwrap_or(Edges::ZERO);
                    let rahmen = rahmen_edges(&k.style.rahmen);
                    let aussen = fuell.left + fuell.right + rahmen.left + rahmen.right;
                    let kasten = if st.is_some_and(|s| s.border_box) { px.max(aussen) } else { px + aussen };
                    kasten + rand.left + rand.right
                }
                _ => 0.0,
            },
        };
        w = w.max(v);
    }
    w
}

/// Release 11.10.2026: das wirksame Seitenverhaeltnis (Breite / Hoehe) eines
/// Kastens: `aspect-ratio: <ratio>` vor dem natuerlichen eines ersetzten
/// Elements, `auto && <ratio>` (negativ gespeichert) nach ihm (CSS Sizing 4
/// §5.1).
pub(crate) fn wirksames_verhaeltnis(ctx: &Ctx<'_>, id: NodeId, node: &narsil_dom::Node) -> Option<f32> {
    let v = ctx.stil(id).map_or(0.0, |s| s.seitenverhaeltnis);
    let natur = node.ersetzt.and_then(|m| m.verhaeltnis()).filter(|r| r.is_finite() && *r > 0.0);
    if !v.is_finite() || v == 0.0 {
        natur
    } else if v > 0.0 {
        Some(v)
    } else {
        natur.or(Some(-v))
    }
}

/// Release 10.10.2026: die Messschleife ab `root` (vorher der Rumpf von
/// `compute_sizes`) — auch fuer das Nachmessen eines Teilbaums.
fn sizes_lauf(ctx: &Ctx<'_>, mut sizes: Sizes, root: NodeId, viewport: Size) -> Result<Sizes, LayoutError> {
    let mut stack = vec![SizeTask::Enter {
        id: root,
        available: viewport,
    }];

    while let Some(task) = stack.pop() {
        match task {
            SizeTask::Enter { id, available } => enter(&mut stack, &mut sizes, ctx, id, available)?,
            // Release 11.10.2026 (Agent CSS2): die beiden Tabellenphasen
            // (siehe `tabellen_leg`).
            SizeTask::TabelleSpalten { id, angabe, verfuegbar, rand, hoehe, kind_hoehe, min_max } => {
                let z = crate::tabellen_leg::spalten_phase(ctx, &sizes, id, angabe, verfuegbar, rand, hoehe, min_max);
                let mut auftraege: Vec<(NodeId, f32, bool)> = Vec::new();
                for (i, c) in z.gitter.zellen.iter().enumerate() {
                    auftraege.push((c.knoten, z.zellen_breite(i), true));
                }
                for (k, _) in z.gitter.beschriftungen.iter() {
                    auftraege.push((*k, z.breite, false));
                }
                stack.push(SizeTask::TabelleZeilen { id, z: Box::new(z) });
                for (k, w, zelle) in auftraege.into_iter().rev() {
                    if zelle {
                        sizes.erzwungen_breite.insert(k.0, w);
                    }
                    stack.push(SizeTask::Enter { id: k, available: Size::new(w, kind_hoehe) });
                }
            }
            SizeTask::TabelleZeilen { id, mut z } => {
                // Release 11.10.2026: eine Flex- oder Rastergroesse
                // (`Ctx::flex_mass`) gilt fuer den Tabellenkasten MIT den
                // Beschriftungen (CSS 2.1 §17.4 „table wrapper box“ ist das
                // Element); dem Raster bleibt der Rest. GEMESSEN vorher: WPT
                // css-grid `table-grid-item-005` (Tabelle mit Beschriftung).
                if ctx.flex_mass.borrow().get(&id).is_some_and(|m| m.1.is_some()) {
                    let mut beschriftung = 0.0f32;
                    for (k, _) in z.gitter.beschriftungen.iter() {
                        let Some(kn) = ctx.document.get(*k) else { continue };
                        let m = resolve_spacing(kn.style.margin, z.breite, *k, "margin").unwrap_or(Edges::ZERO);
                        beschriftung += m.top + m.bottom + sizes.get(*k).map_or(0.0, |s| s.height);
                    }
                    z.hoehe = z.hoehe.map(|h| non_negative(h - beschriftung));
                }
                for c in z.gitter.zellen.iter() {
                    sizes.erzwungen_breite.remove(&c.knoten.0);
                    sizes.zellen_versatz.remove(&c.knoten.0);
                }
                let (lage, setzen, groesse, versatz) = crate::tabellen_leg::zeilen_phase(ctx, &sizes, &z);
                // Release 11.10.2026: die natuerliche Hoehe der Tabelle (ohne
                // ihre angegebene Hoehe) — fuer die Inhaltsgroesse eines
                // Flex-Elements, in dem `height: P%` als `auto` gilt (CSS
                // Flexbox 1 §4.5). GEMESSEN vorher: WPT
                // `flex-minimum-height-flex-items-025` (Fall 4: 196 statt 100 px).
                let natur = match z.hoehe {
                    Some(_) => {
                        let mut ohne = (*z).clone();
                        ohne.hoehe = None;
                        crate::tabellen_leg::zeilen_phase(ctx, &sizes, &ohne).2.height
                    }
                    None => groesse.height,
                };
                sizes.natuerlich.insert(id.0, natur);
                for (k, s) in setzen {
                    sizes.set(k, s);
                }
                for (k, v) in versatz {
                    sizes.zellen_versatz.insert(k.0, v);
                }
                sizes.set(id, groesse);
                sizes.tabellen.insert(id.0, lage);
            }
            SizeTask::BreitenWort { id, available } => {
                let node = get_node(ctx, id)?;
                let worte = ctx.stil(id).map_or([0; 4], |s| s.groessen_art);
                let rand = resolve_spacing(node.style.margin, available.width, id, "margin")?;
                let fuell = resolve_spacing(node.style.padding, available.width, id, "padding")?;
                let rahmen = rahmen_edges(&node.style.rahmen);
                // Release 14.10.2026: the box's OWN sizes (see
                // `Sizes::eigene_breite`).
                let mi = sizes.eigene_breite(ctx, id, false);
                let ma = sizes.eigene_breite(ctx, id, true).max(mi);
                // Release 11.10.2026: ein Flex-/Rastercontainer mit fester
                // Breite misst fuer die Schluesselwoerter seinen INHALT (CSS
                // Sizing 3 §5.1), nicht die eigene Breite. GEMESSEN vorher:
                // WPT css-flexbox `flex-minimum-width-flex-items-014`
                // (`width: 10px; min-width: min-content`: 784 statt 20 px).
                let (mi, ma) = if matches!(node.style.size.width, Length::Px(_)) {
                    let inhalt = |max: bool| {
                        crate::gitter::intrinsisch_inhalt(&sizes, ctx, id, max, 0)
                            .or_else(|| crate::flexbox::intrinsisch_inhalt(&sizes, ctx, id, max, 0))
                    };
                    match (inhalt(false), inhalt(true)) {
                        (Some(a), Some(b)) => (a, b.max(a)),
                        _ => {
                            // Release 14.10.2026: a block with a fixed width
                            // measures its CONTENT as well: the widest
                            // min-content contribution of its children. MEASURED
                            // before: WPT css-sizing `fit-content-length-
                            // percentage-007` (`width: 10px; min-width:
                            // fit-content(100px)` stayed 10 px).
                            let (mut inhalt_min, mut inhalt_max, mut zeile) = (0.0f32, 0.0f32, 0.0f32);
                            for &c in node.children.iter().take(crate::plan::MAX_KINDER) {
                                if ctx.stil(c).is_some_and(|s| !s.wird_gezeichnet() || matches!(s.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)) {
                                    continue;
                                }
                                let Some(k) = ctx.document.get(c) else { continue };
                                let r = resolve_spacing(k.style.margin, available.width, c, "margin").unwrap_or(Edges::ZERO);
                                inhalt_min = inhalt_min.max(sizes.min_inhalt_breite(ctx, c, 1) + r.left + r.right);
                                let w = sizes.max_content_breite(ctx, c, 1) + r.left + r.right;
                                if crate::flow::kind_ist_inline(ctx, c, &k.kind) {
                                    if !crate::flow::kollabierer_leerraum(ctx, c) {
                                        zeile += w;
                                        inhalt_max = inhalt_max.max(zeile);
                                    }
                                } else {
                                    zeile = 0.0;
                                    inhalt_max = inhalt_max.max(w);
                                }
                            }
                            let kanten = fuell.left + fuell.right + rahmen.left + rahmen.right;
                            let inhalt_min = inhalt_min + kanten;
                            (inhalt_min, (inhalt_max + kanten).max(inhalt_min))
                        }
                    }
                } else {
                    (mi, ma)
                };
                let aussen = fuell.left + fuell.right + rahmen.left + rahmen.right;
                // Release 14.10.2026: the min-/max-content width of a replaced
                // box is its natural width, or the width transferred through
                // its ratio from a definite height (CSS Sizing 3 §5.1, CSS
                // Sizing 4 §5.1). MEASURED before: WPT css-sizing
                // `replaced-max-width-min-content` (`width: 1000px; height:
                // 100px; max-width: min-content` on a 10 x 10 canvas stayed
                // 1000 px wide).
                // (Not for an inline `<svg>` with only a `viewBox`: the page
                // gives it `width: 100%`, WPT `svg-intrinsic-size-003`.)
                let (mi, ma) = match node.ersetzt.filter(|_| !node.style.size.width.hat_prozent()) {
                    Some(m) => {
                        let border_box = ctx.stil(id).is_some_and(|s| s.border_box);
                        let rand_y = fuell.top + fuell.bottom + rahmen.top + rahmen.bottom;
                        let h = match node.style.size.height {
                            Length::Px(h) if h.is_finite() => Some(if border_box { non_negative(h - rand_y) } else { h }),
                            _ => None,
                        };
                        let r = wirksames_verhaeltnis(ctx, id, node).filter(|r| *r > 0.0);
                        let w = match (h, r, m.breite) {
                            (Some(h), Some(r), _) => Some(h * r),
                            // The natural width; a natural height outside
                            // `min-height`/`max-height` transfers through the
                            // ratio (WPT `replaced-aspect-ratio-intrinsic-size-
                            // 001`: a 1 x 1 image with `min-height: 100px`).
                            (None, r, Some(b)) => {
                                let (_, _, gmin_h, gmax_h) = grenzen(ctx, id);
                                let innen = |v: f32| if border_box { non_negative(v - rand_y) } else { v };
                                match (r, m.hoehe) {
                                    (Some(r), Some(nh)) => {
                                        let hc = clamp(nh, gmin_h.map(innen), gmax_h.map(innen));
                                        Some(if (hc - nh).abs() > 0.01 { hc * r } else { b })
                                    }
                                    _ => Some(b),
                                }
                            }
                            // Nothing to go on: keep the measured size.
                            _ => None,
                        };
                        match w {
                            Some(w) => (w + aussen, w + aussen),
                            None => (mi, ma),
                        }
                    }
                    None => (mi, ma),
                };
                // Release 14.10.2026: `fit-content(<length-percentage>)` puts
                // its argument (in the `box-sizing` box, percentages against
                // the containing block) in place of the available space (CSS
                // Sizing 3 §2.1). MEASURED before: WPT css-sizing
                // `fit-content-length-percentage-001..016`.
                let argumente = ctx.stil(id).map_or([None; 3], |s| s.fit_content_arg);
                let border_box = ctx.stil(id).is_some_and(|s| s.border_box);
                let wert = |art: u8, k: usize| -> Option<f32> {
                    let w = match art {
                        1 => mi,
                        2 => ma,
                        3 => match argumente[k] {
                            Some(lp) => {
                                let l = non_negative(lp.px + lp.prozent / 100.0 * available.width);
                                let l = if border_box { l.max(aussen) } else { l + aussen };
                                ma.min(l.max(mi))
                            }
                            None => ma.min(non_negative(available.width - rand.left - rand.right).max(mi)),
                        },
                        _ => return None,
                    };
                    Some(non_negative(w - aussen))
                };
                sizes.breite_wort.insert(id.0, (available.width, wert(worte[0], 0), wert(worte[1], 1), wert(worte[2], 2)));
                stack.push(SizeTask::Enter { id, available });
            }
            SizeTask::Format {
                id,
                width,
                height,
                padding,
                nachmessung,
            } => {
                let node = get_node(ctx, id)?;
                let p = ctx.stil_oder_standard(id);
                // Eine intrinsische Zeilenbox waechst nur bis zu ihrem
                // Inhalt. Die Kinder sind erst in diesem Nachlauf gemessen.
                let width = if p.anzeige == narsil_layout::plan::Anzeige::Flex
                    && node.style.direction == Direction::Row
                    && node.style.size.width == Length::Auto
                {
                    width.min(sizes.bevorzugte_breite(ctx, id, 0))
                } else if inline_ebene(&p) && node.style.size.width == Length::Auto {
                    // Release 08.10.2026: `inline-flex`/`inline-grid` ohne
                    // Breite schrumpfen wie ein Inline-Block (CSS 2.1
                    // §10.3.9) — sonst fuellt der Kasten die Zeile und bricht
                    // um.
                    // (Die Flex-Luecken stecken in `max_content_breite`.)
                    width.min(sizes.max_content_breite(ctx, id, 0).max(sizes.min_inhalt_breite(ctx, id, 0)))
                } else {
                    width
                };
                let innen = non_negative(width - padding.left - padding.right);
                let mut hoch = 0.0f32;
                // Release 11.10.2026: der neue Rasteralgorithmus (`gitter.rs`,
                // CSS Grid 1 §11/§12): Teil 1 legt die Spalten fest und laesst
                // die Elemente mit ihrer Bereichsbreite neu messen, Teil 2
                // rechnet die Zeilen. GEMESSEN vorher: WPT css-grid 564/2460.
                let gitter_neu = p.gitter.as_deref().filter(|_| p.anzeige == narsil_layout::plan::Anzeige::Raster && p.lanes == 0);
                // Release 11.10.2026: der neue Flex-Algorithmus (`flexbox.rs`,
                // CSS Flexbox 1 §9), in Stufen mit Nachmessung der Elemente.
                // GEMESSEN vorher: WPT css-flexbox 550/1389.
                let flex_neu = p.flex.as_deref().filter(|_| p.anzeige == narsil_layout::plan::Anzeige::Flex);
                if let Some(fs) = flex_neu {
                    let innen_h = height.map(|h| non_negative(h - padding.top - padding.bottom));
                    let (_, _, gmin_h, gmax_h) = grenzen(ctx, id);
                    // Prozentgrenzen gegen den Platz, mit dem der Container
                    // gemessen wurde (wie in `enter`). GEMESSEN vorher: WPT
                    // `flex-outer-flexbox-column-recalculate-height-on-resize-001`
                    // (`max-height: 100%` wirkte erst nach dem Legen der Elemente).
                    let platz = sizes.verfuegbar_von(id);
                    let (_, _, pm_h, pm_o) = grenzen_prozent(ctx, id, platz.width, platz.height);
                    let (gmin_h, gmax_h) = (cmp_or(pm_h, gmin_h), cmp_or(pm_o, gmax_h));
                    let abz = |v: Option<f32>| v.map(|v| non_negative(if p.border_box { v - padding.top - padding.bottom } else { v }));
                    match crate::flexbox::messen(ctx, node, fs, &sizes, innen, innen_h, abz(gmin_h), abz(gmax_h), nachmessung)? {
                        crate::gitter::Messung::Nachmessen(liste) => {
                            stack.push(SizeTask::Format { id, width, height, padding, nachmessung: true });
                            for (child, groesse) in liste.into_iter().rev() {
                                stack.push(SizeTask::Enter { id: child, available: groesse });
                            }
                            continue;
                        }
                        crate::gitter::Messung::Fertig(h) => {
                            hoch = h;
                            // Release 14.10.2026: a height from `aspect-ratio`
                            // stays definite for the items (lines, stretch), but
                            // the container grows to the extent of a single line
                            // of its items (`Sizes::verhaeltnis_wachsend`, CSS
                            // Sizing 4 §5.2). MEASURED before: WPT css-sizing
                            // `aspect-ratio/flex-aspect-ratio-040`/`-041`/`-043`/
                            // `-044` (`aspect-ratio: 2/1` around a 100 px or
                            // 200 px item stayed 50 px).
                            if sizes.verhaeltnis_wachsend.contains(&id.0) && !fs.umbruch {
                                let mut ausdehnung = 0.0f32;
                                let mut n = 0usize;
                                for &c in node.children.iter().take(crate::plan::MAX_KINDER) {
                                    let Some(k) = ctx.document.get(c) else { continue };
                                    if ctx.stil(c).is_some_and(|s| !s.wird_gezeichnet() || matches!(s.position, narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest)) {
                                        continue;
                                    }
                                    let rand = resolve_spacing(k.style.margin, innen, c, "margin").unwrap_or(Edges::ZERO);
                                    let a = sizes.get(c).map_or(0.0, |g| g.height) + rand.top + rand.bottom;
                                    if fs.reihe { ausdehnung = ausdehnung.max(a) } else { ausdehnung += a }
                                    n += 1;
                                }
                                if !fs.reihe && n > 1 {
                                    ausdehnung += crate::gitter::luecke_px(fs.luecke_zeile, None, 0.0) * (n - 1) as f32;
                                }
                                hoch = hoch.max(ausdehnung);
                            }
                        }
                    }
                } else if let Some(gs) = gitter_neu {
                    let innen_h = height.map(|h| non_negative(h - padding.top - padding.bottom));
                    let (_, _, gmin_h, gmax_h) = grenzen(ctx, id);
                    // Release 11.10.2026: Prozentgrenzen gegen den Platz, mit
                    // dem das Raster gemessen wurde (wie beim Flex-Container);
                    // `grenzen` allein traegt fuer Prozente eine Zahl gegen die
                    // Ansicht. GEMESSEN vorher: WPT css-grid
                    // `grid-lanes/track-sizing/auto-repeat/row-auto-repeat-008`.
                    let platz = sizes.verfuegbar_von(id);
                    let (_, _, pm_h, pm_o) = grenzen_prozent(ctx, id, platz.width, platz.height);
                    let (gmin_h, gmax_h) = (cmp_or(pm_h, gmin_h), cmp_or(pm_o, gmax_h));
                    let abz = |v: Option<f32>| v.map(|v| non_negative(if p.border_box { v - padding.top - padding.bottom } else { v }));
                    match crate::gitter::messen(ctx, node, gs, &sizes, innen, innen_h, abz(gmin_h), abz(gmax_h), nachmessung)? {
                        crate::gitter::Messung::Nachmessen(liste) => {
                            stack.push(SizeTask::Format { id, width, height, padding, nachmessung: true });
                            for (child, groesse) in liste.into_iter().rev() {
                                stack.push(SizeTask::Enter { id: child, available: groesse });
                            }
                            continue;
                        }
                        crate::gitter::Messung::Fertig(h) => hoch = h,
                    }
                } else if p.anzeige == narsil_layout::plan::Anzeige::Raster {
                    let platz = crate::flow::verteile_raster_alle(
                        ctx,
                        node,
                        &narsil_core::geometry::Rect::new(0.0, 0.0, innen, height.unwrap_or(0.0)),
                        &p,
                        &sizes,
                    )?;
                    for q in platz.iter() {
                        hoch = hoch.max(q.y + q.hoehe);
                    }
                    // Release 11.10.2026: leere Vorlagenzeilen zaehlen mit.
                    if let Some(ende) = ctx.raster_hoehe.borrow().get(&node.id) {
                        hoch = hoch.max(*ende);
                    }
                    // =================================================
                    // ZELLENBREITE NACHZIEHEN (06.10.2026)
                    // =================================================
                    //
                    // GEMESSENER FEHLER AM LEBENDEN BEISPIEL (1191 px):
                    // `.mw-body` liegt in einer 883-px-Spalte, wurde aber
                    // mit den 1103 px seines Containers gemessen; sein
                    // Kind-Raster ebenso, und der Artikeltext landete bei
                    // x=1447 — hinter dem Fensterrand. Die Spuren stehen
                    // jetzt fest (`platz`), also kennt jedes Kind seine
                    // ZELLENBREITE, und nur die Kinder mit abweichender
                    // Breite werden EINMAL neu gemessen.
                    //
                    // GEMESSENE ENTSCHEIDUNG, warum nur EINMAL: die
                    // Nachmessung aendert die Kindbreiten und damit
                    // moeglicherweise die Spuren (eine `auto`-Spur misst
                    // ihren Inhalt). Eine zweite Runde waere eine
                    // Schleife; eine Runde bringt den Fehler von 220 px
                    // auf den Rest, der aus der Spuraenderung folgt.
                    if !nachmessung {
                        let mut ziehen: Vec<(NodeId, f32, f32)> = Vec::new();
                        for (child, q) in node
                            .children
                            .iter()
                            .take(crate::plan::MAX_KINDER)
                            .zip(platz.iter())
                        {
                            if q.breite <= 0.0 {
                                continue;
                            }
                            let Some(mass) = sizes.get(*child) else {
                                continue;
                            };
                            // Release 11.10.2026: ein Subgrid (Masonry-Spuren) immer
                            // nachmessen — erst jetzt kennt es seine Spuren.
                            let sub = crate::gitter::subgrid_achsen(ctx, *child);
                            if (q.breite - mass.width).abs() > 0.5 || sub.0 || sub.1 {
                                ziehen.push((*child, q.breite, mass.height));
                            }
                        }
                        if !ziehen.is_empty() {
                            stack.push(SizeTask::Format {
                                id,
                                width,
                                height,
                                padding,
                                nachmessung: true,
                            });
                            for (child, breite, hoehe) in ziehen.into_iter().rev() {
                                stack.push(SizeTask::Enter {
                                    id: child,
                                    available: Size::new(breite, hoehe),
                                });
                            }
                            // KEINE Groesse in dieser Runde: die kommt aus
                            // der Nachmessung (derselbe Format-Task noch
                            // einmal, dann mit `nachmessung: true`).
                            continue;
                        }
                    }
                } else {
                    let reihe = p.flex_richtung == narsil_layout::FlexRichtung::Reihe;
                    let umbruch = p.flex_umbruch != narsil_layout::FlexUmbruch::Keiner;
                    let mut belegt = 0.0f32;
                    let mut zeile = 0.0f32;
                    let mut anzahl = 0;
                    for &child in node.children.iter().take(crate::plan::MAX_KINDER) {
                        let c = get_node(ctx, child)?;
                        let cp = ctx.stil_oder_standard(child);
                        if !cp.wird_gezeichnet()
                            || matches!(
                                cp.position,
                                narsil_layout::PositionArt::Absolut | narsil_layout::PositionArt::Fest
                            )
                        {
                            continue;
                        }
                        let mass = sizes.get(child).ok_or(LayoutError::UnknownNode(child))?;
                        let rand = resolve_spacing(c.style.margin, innen, child, "margin")?;
                        if reihe {
                            // Release 10.10.2026: dieselbe Basis wie die
                            // Verteilung (`flow.rs`, `flex_basis`): bei
                            // `width:auto` die Max-Content-Breite, sonst die
                            // gemessene. GEMESSEN vorher (github.com,
                            // `.CtaForm`): diese Schaetzung nahm
                            // `bevorzugte_breite`, sah ZWEI Zeilen, der
                            // Container wurde doppelt so hoch, und
                            // `align-items:flex-end` schob die eine echte
                            // Zeile 68 px nach unten.
                            let basis = cp.flex_basis.unwrap_or_else(|| {
                                if c.style.size.width == narsil_core::Length::Auto {
                                    sizes.max_content_breite(ctx, child, 0)
                                } else {
                                    mass.width
                                }
                            });
                            let breit = basis + rand.left + rand.right;
                            if umbruch && anzahl > 0 && belegt + p.luecke + breit > innen {
                                hoch += zeile + p.luecke_quer;
                                zeile = 0.0;
                                belegt = 0.0;
                                anzahl = 0;
                            }
                            belegt += breit + if anzahl > 0 { p.luecke } else { 0.0 };
                            zeile = zeile.max(mass.height + rand.top + rand.bottom);
                        } else {
                            hoch += mass.height
                                + rand.top
                                + rand.bottom
                                + if anzahl > 0 { p.luecke } else { 0.0 };
                        }
                        anzahl += 1;
                    }
                    if reihe {
                        hoch += zeile;
                    }
                }
                let (min_b, max_b, min_h, max_h) = grenzen(ctx, id);
                // Release 11.10.2026: Prozentgrenzen wie in `enter` gegen den
                // Platz, mit dem der Kasten gemessen wurde (`grenzen` traegt
                // fuer sie eine Zahl gegen die Ansicht). GEMESSEN vorher: WPT
                // css-grid `grid-lanes/.../row-auto-repeat-008` (`min-height:
                // 80%` in 250 px: 640 statt 200 px hoch).
                let platz = sizes.verfuegbar_von(id);
                let (pm_b, pa_b, pm_h, pa_h) = grenzen_prozent(ctx, id, platz.width, platz.height);
                let (min_b, max_b, min_h, max_h) = (cmp_or(pm_b, min_b), cmp_or(pa_b, max_b), cmp_or(pm_h, min_h), cmp_or(pa_h, max_h));
                let abziehen = |v: Option<f32>, abstand: f32| v.map(|v| {
                    non_negative(if p.border_box { v - abstand } else { v })
                });
                let (min_b, max_b, min_h, max_h) = (
                    abziehen(min_b, padding.left + padding.right),
                    abziehen(max_b, padding.left + padding.right),
                    abziehen(min_h, padding.top + padding.bottom),
                    abziehen(max_h, padding.top + padding.bottom),
                );
                // Release 14.10.2026: `stretch` limits (see `stretch_limits`).
                let (min_b, max_b, min_h, max_h) = stretch_limits(ctx, &sizes, id, node, padding, (min_b, max_b, min_h, max_h));
                // Release 14.10.2026: `min-height`/`max-height` as a content
                // keyword: the content height of the flex/grid container.
                // MEASURED before: WPT css-sizing `block-size-with-min-or-max-
                // content-4` (a 100 px grid row with `height: 0; min-height:
                // max-content` stayed 0).
                let (min_h, max_h) = match p.height_keyword_limits {
                    0 => (min_h, max_h),
                    bits => (
                        if bits & 1 != 0 { Some(non_negative(hoch)) } else { min_h },
                        if bits & 2 != 0 { Some(non_negative(hoch)) } else { max_h },
                    ),
                };
                // L4 (03.10.2026 23:2x): dasselbe wie in `quer`/`hoch` und
                // in `sum_children` — `width` und `height` sind hier die
                // RAHMENmasse (zwei Zeilen weiter oben: `innen = width -
                // padding.left - padding.right`), die Grenzen rechnen nach
                // §10.4 gegen den INHALTSkasten.
                sizes.set(
                    id,
                    Size::new(
                        clamp(
                            non_negative(width - padding.left - padding.right),
                            min_b,
                            max_b,
                        ) + padding.left
                            + padding.right,
                        clamp(
                            non_negative(
                                height
                                    .map(|h| if sizes.verhaeltnis_wachsend.contains(&id.0) { h.max(hoch + padding.top + padding.bottom) } else { h })
                                    .unwrap_or(hoch + padding.top + padding.bottom)
                                    - padding.top
                                    - padding.bottom,
                            ),
                            min_h,
                            max_h,
                        ) + padding.top
                            + padding.bottom,
                    ),
                );
            }
            SizeTask::Sum {
                id,
                cross,
                padding,
                child_available,
                axis,
                mindest,
                schrumpft,
                feste_hoehe,
            } => sum_children(
                &mut sizes,
                ctx,
                id,
                cross,
                padding,
                child_available,
                axis,
                mindest,
                schrumpft,
                feste_hoehe,
            )?,
        }
    }
    Ok(sizes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use narsil_core::style::Style;
    use narsil_dom::Document;

    fn ctx<'a>(document: &'a Document) -> Ctx<'a> {
        static MEASURER: crate::EstimatedTextMeasurer = crate::EstimatedTextMeasurer;
        Ctx::neu(document, &MEASURER, None)
    }

    #[test]
    fn resolve_length_accepts_px_and_percent() {
        let id = NodeId(0);
        assert_eq!(
            resolve_length(Length::px(12.0), 100.0, id, "width"),
            Ok(Some(12.0))
        );
        assert_eq!(
            resolve_length(Length::percent(50.0), 800.0, id, "width"),
            Ok(Some(400.0))
        );
        assert_eq!(resolve_length(Length::AUTO, 100.0, id, "width"), Ok(None));
        // 150% ist fuer width/height erlaubt.
        assert_eq!(
            resolve_length(Length::percent(150.0), 800.0, id, "width"),
            Ok(Some(1200.0))
        );
    }

    #[test]
    fn resolve_length_rejects_invalid_values() {
        let id = NodeId(0);
        for bad in [
            Length::px(-5.0),
            Length::px(f32::NAN),
            Length::px(f32::INFINITY),
            Length::percent(-1.0),
            Length::percent(f32::NAN),
        ] {
            assert!(
                matches!(
                    resolve_length(bad, 100.0, id, "width"),
                    Err(LayoutError::InvalidLength { node, .. }) if node == id
                ),
                "{bad:?} muss einen Fehler liefern"
            );
        }
    }

    #[test]
    fn resolve_spacing_caps_percent_at_100() {
        let id = NodeId(0);
        // `negativ_erlaubt: false` = der PADDING-Fall: eine Groesse, keine Verschiebung.
        assert_eq!(
            resolve_spacing_length(Length::percent(50.0), 200.0, id, "padding.left", false),
            Ok(100.0)
        );
        // Release 08.10.2026: ueber 100 % ist gueltig (amazon.de: 160 %).
        assert_eq!(
            resolve_spacing_length(Length::percent(150.0), 200.0, id, "padding.left", false),
            Ok(300.0)
        );
        assert!(
            resolve_spacing_length(Length::percent(-1.0), 200.0, id, "padding.left", false)
                .is_err()
        );
        // auto ist unzlaessig, aber defensiv 0 statt Fehler.
        assert_eq!(
            resolve_spacing_length(Length::AUTO, 200.0, id, "padding.left", false),
            Ok(0.0)
        );
    }

    /// **15:56 — DER TEST, DEN DIESER FEHLER GEBRAUCHT HATTE, EXISTIERTE NICHT.**
    /// *Bis eben kannte die Pruefung nur eine Antwort: "negativ heisst falsch". Damit war
    /// fuer `margin-left: -1px` — gewoehnliches CSS, 6 von 10 Seiten — keine Antwort
    /// vorgesehen, also wurde die Seite verworfen. Ein Test, der nur die erlaubte Richtung
    /// kennt, sieht die falsche Richtung nicht.*
    ///
    /// *Diese Grenze ist an einem echten Befund festgemacht: `margin.top: -128`
    /// (github.com), `margin.left: -6` (developer.mozilla.org), `margin.top: -40`
    /// (debian.org). Die Werte stehen hier, weil sie im Netz vorkommen und nicht, weil sie
    /// bequem sind — **`-128` ist der groesste, und er war mitverantwortlich fuer sechs
    /// nicht messbare Seiten.***
    #[test]
    fn margin_darf_negativ_sein_und_padding_nicht() {
        let id = NodeId(0);
        // MARGIN: negativ ist erlaubt, und zwar in jedem Ausmass.
        for v in [-1.0f32, -6.0, -40.0, -128.0] {
            assert_eq!(
                resolve_spacing_length(Length::px(v), 200.0, id, "margin.top", true),
                Ok(v),
                "margin.top: {v} ist gueltiges CSS 2.1 8.3.1"
            );
        }
        // PADDING: dieselbe Zahl ist ungueltig (CSS 2.1 8.3.2).
        for v in [-1.0f32, -6.0, -40.0, -128.0] {
            assert!(
                resolve_spacing_length(Length::px(v), 200.0, id, "padding.top", false).is_err(),
                "padding.top: {v} muss abgelehnt werden"
            );
        }
        // Und was auf BEIDEN Seiten gleich bleibt: NaN und unendlich.
        for erlaubt in [true, false] {
            assert!(
                resolve_spacing_length(Length::px(f32::NAN), 200.0, id, "margin.top", erlaubt)
                    .is_err(),
                "NaN ist nie eine Laenge"
            );
            assert!(
                resolve_spacing_length(Length::px(f32::INFINITY), 200.0, id, "margin.top", erlaubt)
                    .is_err(),
                "unendlich ist nie eine Laenge"
            );
        }
    }

    /// Und die Vier-Seiten-Box erbt dieselbe Entscheidung — `resolve_spacing` entscheidet
    /// einmal und gibt sie weiter. *Ein Test an der Funktion allein wuerde eine Entscheidung
    /// pruefen, die an vier Stellen wiederholt werden koennte und dreimal falsch ausfaellt.*
    #[test]
    fn resolve_spacing_entscheidet_einmal_fuer_alle_vier_seiten() {
        let id = NodeId(0);
        let negativ = Spacing {
            left: Length::px(-128.0),
            top: Length::px(-1.0),
            right: Length::px(-6.0),
            bottom: Length::px(-40.0),
        };
        let m = resolve_spacing(negativ, 200.0, id, "margin").expect("margin darf negativ sein");
        assert_eq!(
            (m.left, m.top, m.right, m.bottom),
            (-128.0, -1.0, -6.0, -40.0)
        );
        assert!(
            resolve_spacing(negativ, 200.0, id, "padding").is_err(),
            "dieselben Werte als padding bleiben ungueltig"
        );
    }

    #[test]
    /// `font-size: 0` ist GUELTIG und `NaN` nicht — seit 02.10.2026 (P5).
    ///
    /// GEMESSENE REGRESSION: dieser Test stand auf `font_size: 0.0` und
    /// behauptete, das sei ungueltig. Deshalb hat `https://archlinux.org/`
    /// nicht mehr geladen (NodeId(9)). CSS 2.1, 10.1: "there is no minimum
    /// or maximum value" fuer `font-size`; 0 ergibt einen Text ohne Hoehe.
    /// `NaN` dagegen ist nie ein Ergebnis einer gueltigen Berechnung.
    fn null_font_size_ist_gueltig_aber_nan_nicht() {
        let mut doc = Document::new();
        let id = doc.root();
        doc.set_style(
            id,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                font_size: 0.0,
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        assert!(
            validate_style(doc.get(id).expect("Root existiert")).is_ok(),
            "font-size: 0 ist gueltiges CSS (Icon-Text, Spacer)"
        );

        let mut doc2 = Document::new();
        let id2 = doc2.root();
        doc2.set_style(
            id2,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                font_size: 10.0,
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        assert!(
            validate_style(doc2.get(id2).expect("Root existiert")).is_ok(),
            "10 px ist gueltig"
        );

        // NaN muss ein Fehler bleiben — der Fall, den `fehler.rs` als
        // `Art::Unendlich` meldet, weil NaN jede Schwellenpruefung
        // passiert.
        let mut doc3 = Document::new();
        let id3 = doc3.root();
        doc3.set_style(
            id3,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                font_size: f32::NAN,
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        assert!(
            matches!(
                validate_style(doc3.get(id3).expect("Root existiert")),
                Err(LayoutError::InvalidLength { .. })
            ),
            "NaN ist nie ein Ergebnis"
        );
        // Und negativ ebenso.
        let mut doc4 = Document::new();
        let id4 = doc4.root();
        doc4.set_style(
            id4,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                font_size: -1.0,
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        assert!(validate_style(doc4.get(id4).expect("Root existiert")).is_err());
    }

    #[test]
    fn unknown_node_yields_unknown_node_error() {
        let doc = Document::new();
        let context = ctx(&doc);
        assert_eq!(
            get_node(&context, NodeId(4711)),
            Err(LayoutError::UnknownNode(NodeId(4711)))
        );
    }

    #[test]
    fn sizes_are_missing_for_unmeasured_nodes() {
        let sizes = Sizes::new(3);
        assert_eq!(sizes.get(NodeId(0)), None);
        assert_eq!(sizes.get(NodeId(99)), None);
    }

    #[test]
    fn non_negative_clamps() {
        assert_eq!(non_negative(-3.0), 0.0);
        assert_eq!(non_negative(f32::NAN), 0.0);
        assert_eq!(non_negative(f32::INFINITY), 0.0);
        assert_eq!(non_negative(7.5), 7.5);
    }

    #[test]
    fn compute_sizes_measures_a_small_tree() {
        let mut doc = Document::new();
        let root = doc.root();
        let child = doc
            .push_child(root, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("Kind anlegbar");
        doc.set_style(
            child,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                size: narsil_core::length::Dimension::fixed(40.0, 20.0),
                ..Style::default()
            },
        )
        .expect("Style setzbar");

        let context = ctx(&doc);
        let sizes = compute_sizes(&context, root, Size::new(800.0, 600.0)).expect("Messbar");
        assert_eq!(sizes.get(root), Some(Size::new(800.0, 600.0)));
        assert_eq!(sizes.get(child), Some(Size::new(40.0, 20.0)));
    }

    /// **08:20: DER TEST ZUR BREITE 0.00** — der Messpunkt, den ich um
    /// 08:16 als naechsten benannt habe.
    ///
    /// **GEMESSENER BEFUND AUF spiegel.de (08:04/08:14):** saemtliche
    /// Navigationstexte stehen als **`0.00 x 790.40`** auf `x = 129.60`.
    /// Im Dump kommt **`width: Px(0` nullmal** vor, und alle Container
    /// tragen `width: Auto` — **es gibt also keine Randbedingung, die 0
    /// erzeugt.** Die 0.00 entsteht **im Text-Layout**, also hier.
    ///
    /// **DIE FRAGE, und sie ist eine Frage an die Spezifikation:** wie
    /// breit ist ein Textlauf in einem Container der Breite 0? **CSS
    /// 2.1, 10.8.1** laesst den Umbruch an den Zeilenumbruchpunkten
    /// (`white-space: normal`) zu — und zwischen zwei Umbruchpunkten
    /// **darf der Text nicht umbrechen** (das ist der einzige Fall, in dem
    /// `min-content` NICHT Null werden kann, sonst waere jede deutsche
    /// Seite unlesbar). **Also: mindestens die Breite des laengsten
    /// unbrechbaren Stuecks, und fuer `* ` mindestens ein Zeichen.**
    ///
    /// **DIE ZAHL, DIE ICH ERWARTE, ist darum `> 0` — und der Test
    /// schlaegt genau dann fehl, wenn die 0.00 entsteht.** Das ist der
    /// Unterschied zwischen „der Test weiss es nicht" und „der Test
    /// weiss es jetzt": **er behauptet nichts, er misst.**
    #[test]
    fn textlauf_in_einem_container_der_breite_null_ist_nicht_null_breit() {
        let mut doc = Document::new();
        let root = doc.root();
        // Der Container: Breite ausdruecklich **0**.
        let container = doc
            .push_child(root, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("Container anlegbar");
        doc.set_style(
            container,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                size: narsil_core::length::Dimension::fixed(0.0, 0.0),
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        // Der Text: zwei Worte, die NICHT an einem Wortumbruchpunkt
        // getrennt werden duerfen.
        let text = doc
            .push_child(
                container,
                NodeKind::Text,
                Some("Schlagzeilen".to_string()),
                narsil_dom::Span::EMPTY,
            )
            .expect("Text anlegbar");

        let context = ctx(&doc);
        let sizes = compute_sizes(&context, root, Size::new(800.0, 600.0)).expect("Messbar");
        let s = sizes.get(text).unwrap_or_else(|| {
            panic!("kein Mass fuer den Textknoten — dann ist es kein Breite-Fehler, sondern ein Walk-Fehler")
        });
        // **DIE EINE BEHAUPTUNG.** Nicht „gleich 12 x 16", sondern
        // „**breiter als 0**" — die Zahl, um die es geht.
        assert!(
            s.width > 0.0,
            "Ein Textlauf in einem Container der Breite 0 muss mindestens \
             ein Zeichen breit sein (CSS 2.1, 10.8.1: zwischen zwei \
             Umbruchpunkten wird nicht umbrochen). GEMESSEN: {s:?}"
        );
    }

    /// **DER TEST FUER DEN GANZEN WEG (08:21) — und der Grund, warum der
    /// erste Test nicht reichte.**
    ///
    /// `compute_sizes` gibt einem Textlauf in einem 0-px-Container eine
    /// **positive** Breite (der Test oben ist **gruen**, 08:20). **Die
    /// 0.00 auf spiegel.de ist also NICHT in der Messphase**, sondern
    /// in der **Fliessphase** (`flow.rs`), wo der Kasten in seinen
    /// Eltern gesetzt wird.
    ///
    /// **UND HIER IST AUCH DIE RICHTIGE UNTERSCHEIDUNG:** `TextRun`
    /// (`narsil-layout/src/lib.rs`) hat **keine Breite und keine Hoehe** —
    /// die Felder sind `origin`, `text`, `color`, `font_size`, `deko`,
    /// `node`. **Die `0.00 x 790.40` aus dem Dump sind also die BOX des
    /// Textknotens** (`border_box`/`content_box`), nicht der Run.
    ///
    /// **DIE BEHAUPTUNG, und sie ist dieselbe wie oben:** ein Textknoten
    /// erbt die Breite **seines Textes**, nicht die Breite des
    /// Containers. Bei Containerbreite 0 muss die Box **schmal, aber
    /// nicht null** sein, und ihre Hoehe **die des Textes in so vielen
    /// Zeilen, wie der Umbruch braucht** — bei „Schlagzeilen" in einem
    /// 0-px-Container also **mehr als eine** Zeile. **790.40 px sind ~49
    /// Zeilen**, und das ist die Zahl, die ich auf spiegel.de gemessen
    /// habe.
    #[test]
    fn textknoten_in_breite_null_erbt_die_breite_seines_textes() {
        let mut doc = Document::new();
        let root = doc.root();
        let container = doc
            .push_child(root, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("Container anlegbar");
        doc.set_style(
            container,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                size: narsil_core::length::Dimension::fixed(0.0, 0.0),
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        let text = doc
            .push_child(
                container,
                NodeKind::Text,
                Some("Schlagzeilen".to_string()),
                narsil_dom::Span::EMPTY,
            )
            .expect("Text anlegbar");

        static M: crate::EstimatedTextMeasurer = crate::EstimatedTextMeasurer;
        let baum = crate::layout(&doc, Size::new(800.0, 600.0)).expect("Layout laeuft");
        let mut gefunden = false;
        // Gezielt: die Box des Textknotens. Der Baum ist ein Vec, also
        // laufe ich ihn einmal und pruefe **jede** Box — das ist
        // billiger als eine Suche nach der richtigen und **es sieht
        // jede**, auch wenn ich den Aufbau falsch verstehe.
        stack_alle_textknoten(&baum.root, text, &mut |b| {
            gefunden = true;
            assert!(
                b.border_box.width > 0.0,
                "Die Box eines Textknotens mit 12 Zeichen darf nicht 0 breit \
                 sein. GEMESSEN: {:?} content={:?}",
                b.border_box,
                b.content_box
            );
        });
        assert!(
            gefunden,
            "die Box des Textknotens wurde nicht gefunden — \
             dann prueft dieser Test nichts, und das ist schlimmer als ein Fehlschlag"
        );
    }

    /// **08:22: DIE REPRODUKTION, DIE ZUM BEFUND PASST.** Die beiden
    /// Tests oben setzen `width: 0px` und sind **gruen** — der Fehler
    /// entsteht dort also **nicht**. **Und im Dump tragen die Container
    /// `width: Auto`** (75x `Auto/Auto`, 52x `Auto/Px(19.2)`), **nicht
    /// `0 px`** (nullmal im ganzen Dump).
    ///
    /// **Damit ist die Form benannt, die ich vorher nicht hatte:** ein
    /// **`width: Auto`**-Container **in einem 0-px-Eltern**. `Auto`
    /// heisst „so breit wie der Inhalt, höchstens der verfügbare Raum"
    /// (CSS 2.1, 10.3.5) — und bei **0 px verfuegbarem Raum** ist die
    /// Aufloesung dann die Frage.
    ///
    /// **DIE ZWEI KLASSISCHEN FALSCHEN ANTWORTEN**, und ich benenne sie
    /// beide, weil ein Test sonst nur eine davon findet:
    /// * **`0.00`** — der Kasten bekommt die Breite des **verfuegbaren
    ///   Raums** statt die **seines Inhalts**. Auf spiegel.de **gemessen**.
    /// * **die Containerbreite** — der Kasten wird so breit wie der
    ///   Eltern, also **0**, und der Text bricht bei 0 um; **das erklaert
    ///   die 790.40 px Hoehe** (~49 Zeilen bei 16 px).
    ///
    /// **Richtig ist die dritte:** `Auto` bei Inhalt ist **die Breite des
    /// Inhalts** — bei einem unbrechbaren Stueck **mindestens dessen
    /// Breite** (CSS 2.1, 10.8.1), auch wenn der Platz 0 ist.
    #[test]
    fn auto_block_in_einem_null_eltern_ist_null_breit_und_das_ist_richtig() {
        let mut doc = Document::new();
        let root = doc.root();
        let aussen = doc
            .push_child(root, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("aussen anlegbar");
        doc.set_style(
            aussen,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                size: narsil_core::length::Dimension::fixed(0.0, 0.0),
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        // **Das ist der Unterschied zu den beiden Tests oben: `Auto`.**
        let innen = doc
            .push_child(aussen, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("innen anlegbar");
        doc.set_style(
            innen,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                size: narsil_core::length::Dimension::default(),
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        let text = doc
            .push_child(
                innen,
                NodeKind::Text,
                Some("Schlagzeilen".to_string()),
                narsil_dom::Span::EMPTY,
            )
            .expect("Text anlegbar");

        static M: crate::EstimatedTextMeasurer = crate::EstimatedTextMeasurer;
        let baum = crate::layout(&doc, Size::new(800.0, 600.0)).expect("Layout laeuft");
        let mut gesehen = 0usize;
        let mut null_breit = 0usize;
        // **Rekursion statt eines Hilfsprogramms:** `crate::kasten_alle`
        // gibt es nicht, und **ein Test, der sich auf ein Werkzeug
        // verlaesst, das es nicht gibt, prueft nichts** — er faellt
        // naemlich nicht durch, er ueberspringt das Messen.
        alle_boxen(&baum.root, innen, &mut |b| {
            gesehen += 1;
            if b.border_box.width <= 0.0 {
                null_breit += 1;
            }
        });
        assert!(
            gesehen > 0,
            "die Box des Auto-Containers wurde nicht gefunden — \
             dann prueft dieser Test nichts"
        );
        // **DIE MESSUNG MIT INS TESTERGEBNIS, weil eine Fehlermeldung
        // wegscrollt und ein Wert nicht.** Beide Boxen mit their Zahl:
        let mut bericht = String::new();
        alle_boxen(&baum.root, aussen, &mut |b| {
            bericht.push_str(&format!(
                "\n  AUSSEN #{} {:?} content={:?}",
                b.node.0, b.border_box, b.content_box
            ));
        });
        alle_boxen(&baum.root, innen, &mut |b| {
            bericht.push_str(&format!(
                "\n  INNEN  #{} {:?} content={:?} text={:?}",
                b.node.0,
                b.border_box,
                b.content_box,
                b.text.as_ref().map(|t| t.text.clone())
            ));
        });
        //
        // **08:23: ICH HABE MEINEN EIGENEN TEST KORRIGIERT, und das ist
        // der wichtigere Schritt als sein Fehlschlag.**
        //
        // Der Test **schlug fehl** und meldete `INNEN #2 = 0.0 x 16.0`.
        // **Und die 0.00 ist fuer einen BLOCK-Rahmen RICHTIG** (CSS 2.1,
        // 10.3.5: `width: auto` auf einem Block heisst „fuelle den
        // umgebenden Block" — auch wenn der 0 px breit ist). **Meine
        // Behauptung im Test war also zu weit**, und ein Test, der mehr
        // behauptet als die Spezifikation sagt, **faellt fuer den falschen
        // Grund durch**.
        //
        // **DIE REGEL, die ich mir daraus schreibe:** ein Test, der
        // scheitert, beweist **zuerst einmal, dass meine Erwartung falsch
        // war** — nicht, dass der Code falsch ist. Beides prueft man an
        // **einer Spezifikationsstelle**, und die steht hier: 10.3.5.
        //
        // **DER BLOCKE-RAND IST DAMIT GESETZT, und die offene Frage
        // bleibt:** auf spiegel.de ist der Elternknoten der Navigation
        // laut Plan **`Flex`** (`UEBERLAPPUNG-ELTER #31 plan=Flex`), und
        // **fuer ein FLEX-ITEM ist `width: auto` nicht „fuelle den
        // Eltern", sondern „so breit wie der Inhalt"**. **Wenn eines der
        // sechs Elemente ein Flex-Item ist, ist die 0.00 dort falsch; wenn
        // es ein Block ist, ist sie richtig.** **Das entscheidet die
        // Anzeige des Kastens, und die ist der naechste Messpunkt.**
        assert!(
            null_breit > 0,
            "VERMUTUNG WIDERLEGT: ein Block mit `width: auto` in einem \
             0-px-Eltern ist 0 breit — das ist CSS 2.1, 10.3.5. Wenn dieser \
             Test meldet, dass es 0 breite Auto-Bloecke gibt, dann ist \
             `null_breit == 0` und diese Erwartung war falsch.\n{bericht}"
        );
    }

    /// Geht **jede** Box des Baums durch und ruft `f` fuer die mit
    /// `gesucht`. Bewusst ohne Rueckgabe: ein Test, der nur die
    /// erste passende Box sieht, haelt einen Fehler in der zweiten fuer
    /// eine Messung.
    fn alle_boxen(
        b: &narsil_layout::LayoutBox,
        gesucht: narsil_dom::NodeId,
        f: &mut impl FnMut(&narsil_layout::LayoutBox),
    ) {
        if b.node == gesucht {
            f(b);
        }
        for k in &b.children {
            alle_boxen(k, gesucht, f);
        }
    }

    fn stack_alle_textknoten(
        b: &narsil_layout::LayoutBox,
        gesucht: narsil_dom::NodeId,
        f: &mut impl FnMut(&narsil_layout::LayoutBox),
    ) {
        if b.node == gesucht {
            f(b);
        }
        for k in &b.children {
            stack_alle_textknoten(k, gesucht, f);
        }
    }

    /// **09:06: DER TEST FUER DEN TATSECHLICHEN FALL — ein FLEX-ITEM.**
    ///
    /// **WAS ICH BISHER GEMACHT HABE und was es nicht traf:** mein
    /// spiegel-geformter Test baut `0px` > `auto` > Text **ohne Plan** —
    /// und **ohne Plan ist jeder Knoten ein Block**, also `Direction::
    /// Column`, also der **Y-Zweig**. **GEMESSEN (09:04, mit
    /// `sum_children`-Ausgabe): `BODEN #2 axis=Y`** — der X-Zweig wurde
    /// **nie betreten.**
    ///
    /// **UND DAS IST RICHTIG, denn es bestaetigt meine Korrektur von
    /// 08:23:** fuer einen **Block** ist 0 die richtige Breite (CSS 2.1,
    /// §10.3.7: `width: auto` auf einem Block heisst „fuelle den
    /// umgebenden Block"). **Der Fehler ist also nicht der Blockpfad,
    /// sondern der Zeilenpfad** — und der wird nur ueber einen
    /// Plan-Eintrag `Flex` erreicht (`Direction::Row`).
    ///
    /// **DARAUS FOLGT DER TEST:** ein Flex-Item mit `width: auto` in
    /// einem 0-px-Eltern muss die **`min-content`-Breite seines Textes**
    /// bekommen, **nicht 0** — CSS Flexbox 1 §9.2.1: die automatische
    /// Mindestbreite eines Flex-Items ist der `min-content`-Wert und
    /// damit ein **Boden**. **GEMESSENER Sollwert hier: 168 px**
    /// („Schlagzeilen" bei 16 px, 12 Zeichen, 14 px je Zelle).
    /// **09:21: BEWEIS, NICHT BEHAUPTUNG — die deklarierte Querbreite des
    /// Kindes ist in der PLATZphase erreichbar.**
    ///
    /// **WORUM ES GEHT (09:18, `team/PROBLEME.txt`):** `flow.rs:1358`
    /// setzt `quer: None` fuer jedes Flex-Kind, und der Vermerk sagt, die
    /// Angabe stehe nicht zur Verfuegung. **Ich habe zuerst behauptet,
    /// `cst.size.height` sei da — und das war FALSCH:**
    /// `Ctx::stil_oder_standard` (lib.rs:220) gibt ein
    /// **`narsil_layout::LayoutStil`** zurueck, **nicht** den DOM-Style, und
    /// `LayoutStil` hat **kein** `size`-Feld. **Meine Zeile haette nicht
    /// uebersetzt** — und ich hatte sie trotzdem gemeldet, in demselben
    /// Fenster, in dem ich es bei `narsil_layout::Anzeige` schon getan hatte.
    /// **Ein unuebersetzter Vorschlag ist eine Vermutung mit Code-Stil.**
    ///
    /// **DER WEG, der stimmt, ist derselbe, den `box.rs` selbst geht:**
    /// `node.style.size.height` — **Zeile 206/207**, mit
    /// `resolve_length(...)`. **Die Angabe ist also nicht im System weg,
    /// sie ist nur nicht in der Flex-Struktur.**
    ///
    /// **UND `flow.rs` kann sie holen:** `flow.rs:357` und `:661` rufen
    /// `get_node(ctx, child_id)?` — der Dokumentzugriff ist in der
    /// Platzphase **belegt**, nicht behauptet.
    ///
    /// **WAS DIESER TEST BEWEIST:** dass der deklarierte Wert **19,2 px**
    /// unveraendert aus dem DOM-Style herauskommt — dieselbe Zahl, die
    /// `NARSIL_NULLBREITE` am 09:13 auf spiegel.de als
    /// `Dimension { width: Auto, height: Px(19.2) }` ausgelesen hat und
    /// die das Flex-Item **statt 790,40** bekommen muesste.
    #[test]
    fn kind_hat_seine_deklarierte_querbreite_noch_im_dom_stil() {
        let mut doc = Document::new();
        let root = doc.root();
        let kind = doc
            .push_child(root, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("Kind anlegbar");
        doc.set_style(
            kind,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                // **DIE DEKLARIERTE QUERBREITE**, genau der Wert aus
                // `team/log/nullbreite_spiegel_0913.log`.
                size: narsil_core::length::Dimension {
                    width: narsil_core::length::Length::Auto,
                    height: narsil_core::length::Length::Px(19.2),
                },
                ..Style::default()
            },
        )
        .expect("Style setzbar");

        // **DER WEG, den `flow.rs` gehen wuerde** (`get_node` dort: 357,
        // 661) — und derselbe wie `box.rs:206`.
        let node = doc.get(kind).expect("Kind im Dokument");
        assert_eq!(
            node.style.size.height,
            narsil_core::length::Length::Px(19.2),
            "der deklarierte Wert muss unveraendert aus dem DOM-Style \
             kommen — er wird nur nicht in die Flex-Struktur getragen"
        );
        // **UND `auto` muss `auto` bleiben**, weil `auto` bei `quer`
        // `stretch` bedeutet und kein fester Wert ist: dieselbe
        // Semantik wie `laenge_px` (flow.rs:1502).
        assert_eq!(
            node.style.size.width,
            narsil_core::length::Length::Auto,
            "`auto` auf der Hauptachse ist kein Boden fuer `quer`"
        );
    }

    /// **09:09: DIESER TEST LAEUFT, ABER ER NIMMT NICHTS WAHR — und das
    /// ist der Grund, warum er auf `#[ignore]` steht.**
    ///
    /// **GEMESSEN 09:07, mit und ohne meine Aenderung gruen** (ich habe
    /// `boden` auf `mindest` zurueckgesetzt und neu gebaut: **1 passed
    /// in beiden Laeufen**). **Ein Test, der immer gruen ist, prueft
    /// nichts** — und ich hatte heute drei solche Fallen.
    ///
    /// **DER GRUND, und es ist meine eigene Erkenntnis von 00:52:** was
    /// `sum_children` fuer das Item berechnet, **setzt `verteile_flex`
    /// danach noch einmal** — und **der Flexzweig (CSS) rechnet ohne
    /// `min-content`** („Item 324,80 px bei Kind 77,15 px"). **Meine
    /// Aenderung ist damit auf diesem Pfad ein wirkungsloser Umbau.**
    /// Sie ist **richtig** (die `min_content`-Spalte in `Sizes` ist
    /// korrekt und geprueft), aber sie **erreicht die sichtbare
    /// Fehlstelle nicht.**
    ///
    /// **WARUM `#[ignore]` und nicht loeschen:** der Zustand ist der
    /// Massstab, und ein geloeschter Test verliert ihn. So steht er als
    /// ausfuehrbare Behauptung da, **sobald `verteile_flex` den Boden
    /// benutzt, schlaegt er fehl** — und dann ist er ein
    /// Regressionstest, ohne dass jemand ihn neu schreiben muss.
    /// (Dasselbe Muster steht bei P19 an vier Tests in
    /// `narsil-page/tests/p19_groessen_aus_der_kaskade.rs`.)
    ///
    /// **L5 (02.10.2026 16:0x): DIE SPERRE IST WEG, und der Grund
    /// stand nicht mehr.** GEMESSEN mit
    /// `werkzeuge/bau.sh narsil-layout-engine test --lib -- --ignored`:
    /// der Test laeuft durch (1 passed), also trifft die Behauptung von
    /// 09:09 nicht mehr zu.
    ///
    /// GEMESSEN, woran es liegt, statt es zu vermuten:
    /// `grep -n "min_aggregat|min_of|min_content"
    /// crates/narsil-layout-engine/src/flow.rs` liefert **null Treffer**.
    /// `verteile_flex` benutzt den Boden also **gar nicht** — und genau
    /// das war die Begruendung der Sperre („der Boden aus min_content
    /// erreicht das Flex-Item nicht"). Die Begruendung ist damit
    /// ueberholt an beiden Enden: der Boden erreicht das Item heute ueber
    /// die **Groessenphase** (`box.rs`, `set_min` in der Textzeile und
    /// `min_aggregat` im `Sum`-Zweig), und der Flexzweig verteilt nur
    /// noch Positionen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ich die Sperre entferne statt sie
    /// stehen zu lassen: Ein `#[ignore]` auf einem gruenen Test ist
    /// Ballast, der eine Behauptung traegt, die nicht mehr stimmt. Und
    /// der Test **kann** fallen: er behauptet `border_box.width > 0.0`
    /// in einem Container der Breite **0** — ohne den Boden waere die
    /// Breite 0. Die Gegenprobe (dieselbe Schrift in einem breiten
    /// Container = eine Zeile) ist ein eigener Test und laeuft bereits.
    ///
    /// GEMESSENE EHRICHTEIT, die dazugehoert: **welche** Aenderung die
    /// Behauptung erfuellt hat, habe ich NICHT isoliert. Meine vier
    /// L5-Punkte von heute beruehren `min_aggregat`s Quelle nicht (sie
    /// aendern `SizeTask::Sum`, `flow.rs`-Zweige und die Schichtenlogik),
    /// und der Test baut seinen Baum von Hand aus `Document::new()` —
    /// der Breitenweg aus `narsil-page` ist an ihm nicht beteiligt. Ich
    /// schreibe also nicht „meine Aenderung", sondern „die Sperre traf
    /// nicht mehr zu".
    #[test]
    fn flex_item_mit_auto_breite_bekommt_seinen_min_content() {
        let mut doc = Document::new();
        let root = doc.root();
        let aussen = doc
            .push_child(root, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("aussen anlegbar");
        doc.set_style(
            aussen,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                size: narsil_core::length::Dimension::fixed(0.0, 0.0),
                // **09:07: DER ZEILEN-ZWEIG HAENGT AN `Style.direction`
                // DES CONTAINERS**, nicht am Plan — GEMESSEN: ohne das
                // blieb `axis=Y` und der X-Zweig wurde nie betreten.
                // `narsil-page` setzt `Direction::Row` genau dort
                // (lib.rs:6574), wo ein Flex-Container seine Kinder
                // nebeneinander legt.
                direction: Direction::Row,
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        // Das Item: `width: auto`, **im Plan als Flex**.
        let item = doc
            .push_child(aussen, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("item anlegbar");
        doc.set_style(
            item,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                size: narsil_core::length::Dimension::default(),
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        let text = doc
            .push_child(
                item,
                NodeKind::Text,
                Some("Schlagzeilen".to_string()),
                narsil_dom::Span::EMPTY,
            )
            .expect("Text anlegbar");
        doc.set_style(
            text,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                font_size: 16.0,
                ..Style::default()
            },
        )
        .expect("Style setzbar");

        // **DER PLAN IST DER WEG ZUM X-ZWEIG**, und `LayoutStil::flex`
        // ist oeffentlich (narsil-layout/src/plan.rs).
        let mut plan = narsil_layout::LayoutPlan::leer();
        // **`FlexRichtung::Reihe`** ist der horizontale Zweig
        // (`narsil_css::typen.rs:1447`) — und **genau der ist der
        // spiegel-Fall** (`PLAN #39 eintrag=Some(Flex)` heisst in
        // `box.rs` `Direction::Row` heisst Zeile).
        plan.set(
            item,
            narsil_layout::LayoutStil::flex(narsil_layout::FlexRichtung::Reihe),
        );
        // **KEINE `grenzen`**, und das ist der Massstab: auf spiegel.de
        // sind sie **alle `None`** (`grenzen=Grenzen { min_breite: None,
        // … }`) — es gibt also **keinen anderen Boden**, und der kommt
        // aus dem `min-content`.

        static M: crate::EstimatedTextMeasurer = crate::EstimatedTextMeasurer;
        let baum = crate::layout_mit_plan(&doc, Size::new(800.0, 600.0), &M, &plan)
            .expect("Layout laeuft");
        let mut gesehen = 0usize;
        alle_boxen(&baum.root, item, &mut |b| {
            gesehen += 1;
            assert!(
                b.border_box.width > 0.0,
                "Ein Flex-Item mit `width: auto` muss mindestens so breit sein \
                 wie sein laengstes Wort (CSS Flexbox 1 §9.2.1). GEMESSEN: {:?}",
                b.border_box
            );
        });
        assert!(
            gesehen > 0,
            "die Box des Items wurde nicht gefunden — dann \
             prueft dieser Test nichts"
        );
    }

    /// **DIE GEGENPROBE, damit der Test nicht aus einem Unfall gruen
    /// ist:** derselbe Text in einem **breiten** Container muss die
    /// **intrinsische** Breite bekommen (eine Zeile), nicht die
    /// Containerbreite. Sonst wuerde der Test oben auch bei einem
    /// Container von 800 px greifen, und wir haetten den Fehler nicht
    /// gefunden, sondern **neu erzeugt**.
    #[test]
    fn derselbe_text_in_einem_breiten_container_bleibt_eine_zeile() {
        let mut doc = Document::new();
        let root = doc.root();
        let container = doc
            .push_child(root, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("Container anlegbar");
        doc.set_style(
            container,
            Style {
                deckkraft: 1.0,
                radius: [0.0; 4],
                schatten_anzahl: 0,
                dekoration: 0,
                size: narsil_core::length::Dimension::fixed(800.0, 0.0),
                ..Style::default()
            },
        )
        .expect("Style setzbar");
        let text = doc
            .push_child(
                container,
                NodeKind::Text,
                Some("Schlagzeilen".to_string()),
                narsil_dom::Span::EMPTY,
            )
            .expect("Text anlegbar");

        let context = ctx(&doc);
        let sizes = compute_sizes(&context, root, Size::new(800.0, 600.0)).expect("Messbar");
        let s = sizes.get(text).expect("Text hat ein Mass");
        assert!(s.width > 0.0, "im breiten Container: {s:?}");
        // **UND die Hoehe bleibt EINER Linie.** 790.40 px bei 0 Breite
        // heisst „jedes Zeichen eine Zeile" — das ist die Zahl, die ich
        // auf spiegel.de gesehen habe. **Eine Zeile ist die richtige
        /// Antwort, und die Breite gehoert zum Text, nicht zum
        /// Container.**
        let lines = s.height / 16.0;
        assert!(
            lines < 3.0,
            "zwölf Zeichen in einem 800-px-Container sind EINER Zeile hoch, \
             nicht {lines} Zeilen. GEMESSEN: {s:?}"
        );
    }

    #[test]
    fn absoluter_nachfahre_vergroessert_autohoehe_nicht() {
        use narsil_core::length::Dimension;
        use narsil_layout::{LayoutPlan, LayoutStil, PositionArt};
        let mut doc = Document::new();
        let root = doc.root();
        let eltern = doc
            .push_child(root, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("Eltern anlegbar");
        let absolut = doc
            .push_child(eltern, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("positioniertes Kind anlegbar");
        let normal = doc
            .push_child(eltern, NodeKind::Container, None, narsil_dom::Span::EMPTY)
            .expect("normales Kind anlegbar");
        doc.set_style(
            absolut,
            Style {
                size: Dimension::fixed(100.0, 300.0),
                ..Style::default()
            },
        )
        .expect("Stil anlegbar");
        doc.set_style(
            normal,
            Style {
                size: Dimension::fixed(100.0, 20.0),
                ..Style::default()
            },
        )
        .expect("Stil anlegbar");
        let mut plan = LayoutPlan::leer();
        let mut abs_stil = LayoutStil::standard();
        abs_stil.position = PositionArt::Absolut;
        plan.set(absolut, abs_stil);
        static MESSER: crate::EstimatedTextMeasurer = crate::EstimatedTextMeasurer;
        let context = Ctx::neu(&doc, &MESSER, Some(&plan));
        let sizes = compute_sizes(&context, root, Size::new(400.0, 400.0)).expect("Messung");
        assert_eq!(sizes.get(eltern).expect("Eltern gemessen").height, 20.0);
        assert_eq!(
            sizes.get(absolut).expect("Absolutes Kind gemessen").height,
            300.0
        );
    }
}
