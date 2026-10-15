//! Layout-Engine von Narsil: `Document` -> `LayoutTree` -> `DisplayList`.
//!
//! Der Crate ist das fehlende Glied zwischen Parser (Agent2) und Rasterizer
//! (Agent3). Er nimmt einen fertigen [`narsil_dom::Document`] entgegen und
//! liefert zwei Ergebnisse:
//!
//!   1. [`layout`] — eine geometrisch vollstaendig aufgeloeste Baumstruktur
//!      ([`narsil_layout::LayoutTree`]) mit absoluten Koordinaten.
//!   2. [`build_display_list`] — die geordnete Zeichenliste
//!      ([`narsil_display::DisplayList`]) fuer den Renderer.
//!
//! ## Regeln der Version 0
//!
//! * **Auto-Aufloesung:** Die Hauptachse eines Containers wird aus seinen
//!   Kindern summiert, die Querachse faellt auf die Content-Box des Elterns
//!   zurueck. Der `page`-Root fuellt stattdessen den Viewport.
//! * **Kein Margin-Collapsing:** Margins addieren sich immer.
//! * **Absolute Koordinaten:** Alle Boxen liegen im Viewport-System
//!   (Ursprung oben links), nicht relativ zum Eltern.
//! * **Kein Overflow-Clipping:** Kinder duerfen ueber ihre Box hinausragen;
//!   das clippt spaeter der Rasterizer.
//! * **Textmessung** laeuft ueber den [`TextMeasurer`]. [`layout`] benutzt
//!   [`BitmapMeasurer`] (echte 5x7-Bitmap-Schrift aus `narsil-text`,
//!   Grundlinie `ASCENT_RATIO` = 5/7), [`layout_with_measurer`] nimmt jeden
//!   beliebigen Messer entgegen — zum Einspeisen oder zum Vergleichen mit der
//!   Naeherung [`EstimatedTextMeasurer`].
//!
//! Mess- und Positionsphase laufen ITERATIV (expliziter Stack im Heap): 1000
//! verschachtelte Container brauchen keinen grossen Stapelspeicher.
//!
//! Die Einzelregeln sind an den jeweiligen Funktionen per `///` erklaert
//! (siehe `box::compute_sizes` und `flow::build_tree`).

// B11 (06.10.2026, 01:17): `box_sizing::compute_sizes` ist eine Funktion
// in `box.rs` (Zeile 1257), kein eigenes Modul. Ich hatte beim Freigeben
// von `ersetzte_groesse` eine `mod box_sizing;`-Zeile EINGEFUEGT, die es
// nie gab — sie liess `box.rs` zweimal einbinden und den Build rot werden.
// GEMESSENER FEHLER, und die Diagnose hat gedauert, weil `sed -n 33,40p`
// danach aussah wie das Original: eine eingefuegte Zeile sieht aus wie
// eine, die immer da war.
// B11 (06.10.2026, 01:24): `box_sizing.rs` ist nach `box.rs` MERGED
// worden (GEMESSEN: `Sizes` in `box.rs:71`, `get_node` in `box.rs:131` —
// die Datei `box_sizing.rs` gibt es nicht mehr, und `flow.rs:42` holt
// `crate::box_sizing`). Das ist fremde Arbeit in meiner Crate, und sie
// braucht genau eine Sache von mir: dass `crate::box_sizing` wieder
// existiert. Das ist ein `pub use` auf die Symbole in `box.rs` — additiv,
// ohne neue Datei, ohne neue Wahrheit.
//
// GEMESSENER FEHLER, den ich dabei gemacht habe: `box` ist ein
// reserviertes Bezeichner, `mod box;` baut nicht. Ich habe die Zeile
// dreimal angesehen (`sed -n 33,40p`) und sie sah nach jeder Korrektur
// wie das Original aus — weil eine EINGEFUEGTE Zeile das tut.
// `r#box` ist die Form, die baut; `#[path]` ist dafuer nicht noetig.
// B11 (06.10.2026, 01:25): `box_sizing.rs` ist nach `box.rs` gewandert
// (fremde Arbeit in dieser Crate, GEMESSEN 01:24: `Sizes` in `box.rs:71`,
// `get_node` in `box.rs:131`, und die Datei `box_sizing.rs` gibt es nicht
// mehr). `flow.rs:42` und `vorschau.rs:33` holen aber
// `crate::box_sizing::{…}`.
//
// GEMESSENE ENTSCHEIDUNG, warum eine VIERZEILIGE Datei und kein
// `pub use` hier: `box_sizing` muss ein **Modul** sein, weil zwei Dateien
// den Pfad `crate::box_sizing` benutzen. Die Weiche enthaelt keine Logik
// und keine Zahlen — sie zeigt nur auf `box.rs`. Damit gibt es genau einen
// Satz Symbole und genau eine Rechnung; die Weiche ist kein Zustand und
// keine zweite Wahrheit, sondern ein Name fuer dieselbe Sache.
//
// WAS ICH NICHT GETAN HABE: die Rechnung nach `box_sizing.rs`
// zurueckkopieren. Das waere eine zweite Wahrheit, und die kippt,
// sobald jemand eine der beiden aendert.
#[allow(non_snake_case)]
mod r#box;
#[path = "box_sizing_weiche.rs"]
mod box_sizing;
mod display;
mod flow;
// Release 11.10.2026: CSS Grid 1/2 als eigener Algorithmus.
mod gitter;
mod flexbox;
/// HINTERGRUNDBILD: die Geometrie (CSS Backgrounds 3 §3.3-3.5).
///
/// **OEFFENTLICH, weil noch niemand es aufruft** — und das ist der
/// ehrliche Zustand: der Aufrufer waere `display.rs` (das den DOM
/// braucht), und dem fehlt die **Quelle**. Die steht in der Kaskade,
/// und die Kaskade fuellt `LayoutPlan` in `narsil-page` (fremde Crate,
/// siehe deren Plan-Kommentar). **Ohne eine Zeile dort bleibt dieses
/// Modul ein gepruefter Rechner ohne Abrufer** — lieber so, als eine
/// Kopie der Rechnung an zwei Stellen.
pub mod hintergrund;
mod plan;
mod tabelle;
// Release 11.10.2026 (Agent CSS2): das Tabellen-Layout nach CSS 2.1 Kapitel 17.
mod tabellen_leg;
mod text;
// P9 (05.10.2026, 07:50): die Zeilenvorschau. Sie steht in einer eigenen
// Datei, weil sie die eine Stelle ist, an der die Teile einer Inline-Zeile
// VORHER gezählt werden. In `flow.rs` steht dieselbe Regel noch einmal
// (`inline_teil`), und beide gehoeren zusammen — siehe den Kopf der Datei.
mod vorschau;

use narsil_core::geometry::Size;
use narsil_dom::{Document, NodeId, NodeKind};
use narsil_layout::LayoutPlan;
use narsil_layout::LayoutTree;

// P12 (05.10.2026, 14:22): `MAX_LAGE_PX` ist oeffentlich, weil ein Test
// den Unterschied zu `MAX_WERT_PX` festhalten soll — die beiden Zahlen
// sind die ganze Regel, und eine Regel ohne Test ist eine Behauptung.
pub use plan::{
    FloatZustand, MAX_KINDER, MAX_LAGE_PX, Platzierung, ScrollBereich, beschnitt_kasten,
    enthaltend, flex_basis, klemme_versatz, platz_absolut, reihenfolge, schicht_von,
    schicht_von_als, scroll_bereich, versatz_relativ, verteile_flex, verteile_flex_mit_selbst,
    verteile_raster,
};
// A1: die Tabellenrechnung (CSS 2.1, 17.6).
pub use tabelle::{
    MAX_SPALTEN, MAX_ZEILEN_LAYOUT, MAX_ZELLEN, Tabelle, Zeile as TabellenZeile,
    Zelle as TabellenZelle, ZelleEingabe, ZellenPlatz, lege_tabelle, lege_tabelle_mit_richtung,
    rowspan_0_heisst_rest, zellen_rect,
};

pub use display::{
    build_display_list, build_display_list_im_bereich, build_display_list_im_bereich_fest, build_display_list_mit_bildern, build_display_list_mit_masken,
};
//
// B11 (06.10.2026, 01:13): `ersetzte_groesse` FREIGEGEBEN.
//
// GEMESSENER GRUND: die Funktion ist eine reine Rechnung ueber eine
// `ErsatzMasse` und zwei `Option<f32>` — kein Zustand, keine Seiteneffekte.
// Sie war `pub(crate)` (Zeile 228 in `box.rs`), damit der Test
// `tests/b11_halbe_masse.rs` sie nicht erreichen konnte. Ein Test, der
// die Funktion nicht erreichen kann, prueft sie nicht — und der Fehler,
// den sie macht (ein `<img>` mit halb bekannter Masse wird 0 px hoch,
// github.com #839/#850, 01:11), ist genau die Sorte, die nur ein Test
// fangen kann.
//
// GEMESSENE GRENZE: `pub` heisst hier nicht „API", es heisst „an dieser
// Stelle der Engine kann man die Rechnung nachrechnen". Es gibt keine
// zweite Wahrheit und keine neue Abhaengigkeit.
//
// B11 (06.10.2026, 01:13): `ersetzte_groesse` ist eine reine Rechnung
// ueber eine `ErsatzMasse` und zwei `Option<f32>` — kein Zustand, keine
// Seiteneffekte. Sie war `pub(crate)`, damit der Test
// `tests/b11_halbe_masse.rs` sie nicht erreichen konnte. Ein Test, der
// die Funktion nicht erreichen kann, prueft sie nicht — und der Fehler,
// den sie macht (ein `<img>` mit halb bekannter Masse wird 0 px hoch,
// github.com #839/#850, 01:11), ist genau die Sorte, die nur ein Test
// fangen kann.
//
// GEMESSENE ENTSCHEIDUNG, warum kein `pub use` hier: `box` ist ein
// reserviertes Schluesselwort, und der Modulname laesst sich nur mit
// `r#box` schreiben — was eine Umbenennung an zwei Stellen und einen
// `#[path]`-Umweg fuer `box_sizing` bedeutet (das ist ein Untermodul von
// `box.rs`). Das ist groesser als der Nutzen. Der Test steht deshalb
// **IN** `box.rs` bei der Funktion, und das ist der Ort, an dem die
// Aenderung auch liegt.

// W5E (2): `TtMeasurer` misst mit derselben Systemschrift, mit der
// `narsil-page` umbricht und der Rasterizer zeichnet.
pub use text::{
    BitmapMeasurer, EstimatedTextMeasurer, MAX_ZEICHEN_GEMESSEN, MessungMesser, TextMeasurer,
    TtMeasurer, default_measurer,
};
// added 04.10.2026, Rolle SERVER (SERVER_12): `line-height: normal` als
// DIE EINE Quelle des Projekts. `narsil_page::ZEILENHOEHE` ist ein `pub use`
// davon — siehe `text::zeilenhoehe_normal`.
pub use text::zeilenhoehe_normal;

/// Fehlertyp der Layout-Phase.
///
/// Alle Fehler sind [`core::fmt::Display`] und [`std::error::Error`] —
/// die Engine panikt nie auf ungueltiger Eingabe.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutError {
    /// Knoten-Id, die es im Dokument nicht gibt (inklusive fehlender Root).
    UnknownNode(narsil_dom::NodeId),
    /// Viewport mit Breite oder Hoehe `<= 0`, NaN oder unendlich.
    InvalidViewport(Size),
    /// Stil mit ungueltiger Laenge: negativ, NaN, unendlich oder
    /// Prozentwert ausserhalb des erlaubten Bereichs.
    InvalidLength {
        /// Der Knoten, dessen Stil das Problem hat.
        node: narsil_dom::NodeId,
        /// Menschliche Beschreibung des Fehlers.
        detail: String,
    },
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayoutError::UnknownNode(id) => {
                write!(f, "Knoten {id:?} existiert im Dokument nicht")
            }
            LayoutError::InvalidViewport(size) => write!(
                f,
                "Viewport {}x{} ist ungueltig (muss endlich und > 0 sein)",
                size.width, size.height
            ),
            LayoutError::InvalidLength { node, detail } => {
                write!(f, "Ungueltige Laenge bei Knoten {node:?}: {detail}")
            }
        }
    }
}

impl std::error::Error for LayoutError {}

/// Lesender Zugriff auf Dokument und Textmessung waehrend eines Layout-Laufs.
pub(crate) struct Ctx<'a> {
    pub document: &'a Document,
    pub measurer: &'a dyn TextMeasurer,
    /// Was die Kaskade ueber die Anzeigeart weiss (W7 L1-L4).
    ///
    /// `None` bedeutet: es wurde kein Plan gegeben, und dann ist jeder
    /// Knoten ein Block. Das ist der Zustand vor dem 30.09.2026 und der
    /// Grund, warum diese Funktion additiv bleiben konnte — ein leerer
    /// Plan verhaelt sich exakt wie der alte Weg.
    pub plan: Option<&'a LayoutPlan>,
    /// added 05.10.2026, Rolle SERVER (SERVER_14): **der Transport fuer
    /// die Tabellen-Geometrie.**
    ///
    /// GEMESSENER GRUND, warum es dieses Feld geben musste: die
    /// Zellgeometrie entsteht auf **Tabellen**-Ebene — `lege_tabelle`
    /// rechnet alle Zeilen und Spalten auf einmal — und wird auf
    /// **Zellen**-Ebene gebraucht. Das sind zwei Ebenen auseinander, und
    /// `flow.rs` arbeitet mit einem Stapel, der **eine** Ebene pro
    /// Durchlauf kennt (`kind_platzieren`: ein Knoten, Aufgaben fuer seine
    /// Kinder). Ohne Ablage gab es keinen Ort, an dem die Spaltenbreiten
    /// liegen konnten.
    ///
    /// **WARUM SO EINE MAP und nicht ein Feld im Knoten:** die Tabelle wird
    /// **vor** ihren Zeilen gelegt (so verlangt es CSS 2.1, 17.6 — die
    /// Spaltenbreiten brauchen alle Zeilen). Die Zeile wird danach
    /// besucht und schlaegt nach. Ein Feld im Knoten kaeme zu spaet.
    ///
    /// **WARUM `RefCell`:** `Ctx` wird ueberall **als Referenz** gereicht,
    /// und der Aenderungsweg darf die Signaturen nicht anfassen (ADR-012,
    /// dieselbe Begruendung wie bei `TextMeasurer::measure_width_stil`).
    /// Innere Unveraenderlichkeit ist hier richtig: die Karte wird
    /// geschrieben, gelesen und **nicht** weitergegeben.
    pub tabellen: std::cell::RefCell<std::collections::HashMap<NodeId, Tabelle>>,
    /// Release 11.10.2026 (Agent CSS2): die absoluten Rechtecke der Teile
    /// gelegter Tabellen (Gruppen, Zeilen, Zellen, Beschriftungen) — die
    /// Platzphase der Tabelle schreibt, ihre Nachfahren lesen.
    pub tabellen_plaetze: std::cell::RefCell<std::collections::HashMap<NodeId, narsil_core::geometry::Rect>>,
    /// Release 11.10.2026: der Rasterbereich (CSS Grid 1 §9.1) eines absolut
    /// positionierten Kindes eines positionierten Rasters, mit seiner
    /// Ausrichtung (senkrecht, waagerecht) — `verteile_raster_alle` schreibt,
    /// die Abspos-Platzierung liest.
    /// Release 11.10.2026: das Ende der expliziten Zeilen eines Rasters
    /// (relativ zur Inhaltsbox) — leere Vorlagenzeilen zaehlen zur Hoehe.
    pub raster_hoehe: std::cell::RefCell<std::collections::HashMap<NodeId, f32>>,
    pub raster_bereiche: std::cell::RefCell<
        std::collections::HashMap<NodeId, (narsil_core::geometry::Rect, narsil_css::FlexAusrichtung, narsil_css::FlexAusrichtung)>,
    >,    /// Release 11.10.2026: das Ergebnis des neuen Rasteralgorithmus je Raster
    /// (`gitter.rs`) — die Messphase schreibt, die Platzierung liest, damit
    /// beide mit denselben Spuren legen.
    pub gitter: std::cell::RefCell<std::collections::HashMap<NodeId, crate::gitter::Ergebnis>>,
    /// Release 11.10.2026: das Rechteck der statischen Lage eines absolut
    /// positionierten Kindes eines NICHT positionierten Rasters (CSS Grid 2
    /// §9.2: die Inhaltsbox, ausgerichtet mit `justify-self`/`align-self`).
    /// Release 11.10.2026: die Groesse des Rasterbereichs eines absolut
    /// positionierten Kindes eines positionierten Rasters (Messphase).
    pub gitter_abs: std::cell::RefCell<std::collections::HashMap<NodeId, Size>>,
    /// Release 11.10.2026: der Rasterbereich eines Rasterelements (Bezug
    /// seiner Prozentraender, CSS Grid 1 §6.2), von `gitter::verteilen`.
    pub gitter_flaeche: std::cell::RefCell<std::collections::HashMap<NodeId, Size>>,
    pub gitter_statisch: std::cell::RefCell<
        std::collections::HashMap<NodeId, (narsil_core::geometry::Rect, narsil_css::FlexAusrichtung, narsil_css::FlexAusrichtung)>,
    >,
    /// Release 11.10.2026: absolut positionierte NACHFAHREN (keine Kinder)
    /// eines positionierten Rasters — ihr Rasterbereich ist nur der
    /// enthaltende Block, die statische Lage bleibt die im Fluss.
    pub gitter_nachfahren: std::cell::RefCell<std::collections::HashSet<NodeId>>,
    /// Release 11.10.2026: das Ergebnis des neuen Flex-Algorithmus je
    /// Container (`flexbox.rs`) — Messphase schreibt, Platzierung liest.
    pub flex: std::cell::RefCell<std::collections::HashMap<NodeId, crate::flexbox::Ergebnis>>,
    /// Release 11.10.2026: die benutzte Groesse eines Flex-Elements
    /// (Rahmenmasse Breite, Hoehe), mit der `enter` es misst.
    pub flex_mass: std::cell::RefCell<std::collections::HashMap<NodeId, (Option<f32>, Option<f32>)>>,
    /// Release 11.10.2026: Flex-/Rasterelemente, deren gesetzte Hoehe
    /// (`flex_mass`) DEFINIT ist (gedehnt, CSS Flexbox 1 §9.4 Schritt 11 und
    /// §9.8; Rasterbereich) — gegen sie loest sich `hoehe_bedingt` der
    /// Kinder auf (`box.rs`, `enter`).
    pub hoehe_definit: std::cell::RefCell<std::collections::HashSet<NodeId>>,
    /// Release 12.10.2026 (Agent CSS2): die Floats des umgebenden
    /// Formatierungskontexts, die ein Kasten OHNE eigenen Kontext sieht (im
    /// Rahmen seiner Inhaltskante) — die Messphase schreibt (`sum_children`
    /// des Eltern), beide Phasen lesen. CSS 2.1 §9.5: Floats wirken auf alle
    /// Zeilen und Kaesten ihres Kontexts, auch in verschachtelten Bloecken.
    pub float_erbe: std::cell::RefCell<std::collections::HashMap<NodeId, crate::plan::FloatZustand>>,
    /// Release 12.10.2026 (Agent CSS2): die Floats, die ein Kasten ohne eigenen
    /// Kontext selbst (oder in Nachfahren) setzt — sie gelten fuer die
    /// folgenden Geschwister (Rahmen: seine Inhaltskante).
    pub float_aus: std::cell::RefCell<std::collections::HashMap<NodeId, crate::plan::FloatZustand>>,
    /// Release 12.10.2026 (Agent CSS2): Merker, ob ein Teilbaum auf Floats
    /// reagiert (`box::float_sensibel`), und wie oft fuer geerbte Floats neu
    /// gemessen wurde (M3).
    pub float_sensibel: std::cell::RefCell<std::collections::HashMap<NodeId, bool>>,
    pub float_nachmessen: std::cell::Cell<usize>,
}

impl<'a> Ctx<'a> {
    /// **DER EINZIGE WEG, EINEN `Ctx` ZU BAUEN.** added 05.10.2026,
    /// SERVER_14.
    ///
    /// GEMESSENER Grund: `Ctx` hat jetzt vier Felder, und die
    /// Struct-Literale in `lib.rs` und `text.rs` (acht Stueck) mussten
    /// alle nachgezogen werden. Beim naechsten Feld wieder. Diese
    /// Funktion ist die Antwort darauf: ein neues Feld wird **hier**
    /// eingetragen, und sonst nirgends.
    ///
    /// **DAS IST AUCH DIE LEHRE AUS DEN ROTEN ALARMEN DIESER NACHT:**
    /// `Style` kostete **131** Literale, `TextRun` drei, `LayoutStil` zwei,
    /// und jedes Mal lief der Compilerfehler bis in fremde Crate und
    /// blockierte sie rollenweise. Ein Konstruktor macht das einmal.
    pub(crate) fn neu(
        document: &'a Document,
        measurer: &'a dyn TextMeasurer,
        plan: Option<&'a LayoutPlan>,
    ) -> Self {
        // Release 15.10.2026: O(1) plan lookups for the whole layout (see
        // `LayoutPlan::build_index`).
        if let Some(p) = plan {
            p.build_index();
        }
        Self {
            document,
            measurer,
            plan,
            tabellen: std::cell::RefCell::new(std::collections::HashMap::new()),
            tabellen_plaetze: std::cell::RefCell::new(std::collections::HashMap::new()),
            raster_bereiche: std::cell::RefCell::new(std::collections::HashMap::new()),
            raster_hoehe: std::cell::RefCell::new(std::collections::HashMap::new()),
            gitter: std::cell::RefCell::new(std::collections::HashMap::new()),
            gitter_statisch: std::cell::RefCell::new(std::collections::HashMap::new()),
            gitter_abs: std::cell::RefCell::new(std::collections::HashMap::new()),
            gitter_flaeche: std::cell::RefCell::new(std::collections::HashMap::new()),
            gitter_nachfahren: std::cell::RefCell::new(std::collections::HashSet::new()),
            flex: std::cell::RefCell::new(std::collections::HashMap::new()),
            flex_mass: std::cell::RefCell::new(std::collections::HashMap::new()),
            hoehe_definit: std::cell::RefCell::new(std::collections::HashSet::new()),
            float_erbe: std::cell::RefCell::new(std::collections::HashMap::new()),
            float_aus: std::cell::RefCell::new(std::collections::HashMap::new()),
            float_sensibel: std::cell::RefCell::new(std::collections::HashMap::new()),
            float_nachmessen: std::cell::Cell::new(0),
        }
    }

    /// Der Layout-Stil eines Knotens, oder `None` fuer „Block".
    ///
    /// DER EINE ORT, an dem der Plan gelesen wird. Alles andere in der
    /// Engine sieht nur Block und kann den Plan nicht versehentlich
    /// doppelt anwenden.
    pub(crate) fn stil(&self, id: narsil_dom::NodeId) -> Option<&narsil_layout::LayoutStil> {
        self.plan?.hole(id)
    }

    /// Der Layout-Stil eines Knotens, oder der Standard.
    ///
    /// Release 15.10.2026: borrowed, not cloned. MEASURED before: the clone
    /// (a `LayoutStil` with boxed flex/grid parts) ran for every child in
    /// every intrinsic width pass (`max_content_width_raw`) — a heap copy per
    /// node and query on https://www.chip.de/.
    pub(crate) fn stil_oder_standard(&self, id: narsil_dom::NodeId) -> &'a narsil_layout::LayoutStil {
        match self.plan {
            Some(p) => p.stil_oder_standard(id),
            None => narsil_layout::LayoutStil::default_ref(),
        }
    }
}

/// Berechnet das Layout fuer ein Dokument.
///
/// `viewport` ist die verfuegbare Flaeche; der `page`-Root fuellt sie, solange
/// `width`/`height` nicht explizit gesetzt sind.
///
/// # Textmessung
///
/// Diese Funktion rechnet mit [`BitmapMeasurer`], also der eingebauten
/// Bitmap-Schrift aus `narsil-text`: Textbreiten sind die echten Zeichen-
/// vorschuebe, die Grundlinie liegt auf `narsil_text::ASCENT_RATIO` (5/7).
/// Wer anders messen will, nimmt [`layout_with_measurer`].
///
/// # Fehler
///
/// * [`LayoutError::InvalidViewport`] — Breite oder Hoehe `<= 0`, NaN oder
///   unendlich. Es entsteht nie ein Panic und nie ein NaN im Ergebnis.
/// * [`LayoutError::UnknownNode`] — der Dokument-Root fehlt oder ist kein
///   `page`-Knoten.
/// * [`LayoutError::InvalidLength`] — ein Stil enthaelt eine ungueltige Laenge.
pub fn layout(document: &Document, viewport: Size) -> Result<LayoutTree, LayoutError> {
    layout_with_measurer(document, viewport, &BitmapMeasurer::new())
}

/// Wie [`layout`], aber mit dem WEG DER KASKADE (W7, Rolle LAYOUT).
///
/// # WAS DER PLAN AENDERT
///
/// `plan` sagt je Knoten, was die Kaskade berechnet hat:
/// `display:flex`, `display:grid`, `position`, `float`, `clear`,
/// `overflow`. Damit bekommen
///
/// * **L1** `display:flex` — die Kinder werden mit `lege_flex_zeilen`
///   verteilt statt mit einem linearen Cursor. Das ist der Unterschied
///   zwischen einer horizontalen Navigationsleiste und einer senkrechten
///   Liste.
/// * **L2** `display:grid` — die Kinder werden mit `lege_raster`
///   verteilt (px/fr/repeat, gap).
/// * **L3** `position` — `relative` verschiebt, `absolute`/`fixed`
///   stehen gegen den enthaltenden Block und verlassen den Fluss.
/// * **L4** `float`/`clear` — schwebende Boxen stehen links oder
///   rechts, `clear` rueckt unter sie.
///
/// # RUECKWAERTS-KOMPATIBEL (das ist der Punkt)
///
/// Ein **leerer** Plan ergibt fuer jeden Knoten `display:block` — also
/// genau das Verhalten von vor dem 30.09.2026. Diese Funktion ist
/// deshalb additiv: sie wurde neben [`layout_with_measurer`] gebaut und
/// nicht hinein.
///
/// ```
/// use narsil_core::geometry::Size;
/// use narsil_dom::Document;
/// use narsil_layout::LayoutPlan;
/// use narsil_layout_engine::{layout_mit_plan, layout_with_measurer};
///
/// let doc = Document::new();
/// let vp = Size::new(800.0, 600.0);
///
/// // Ohne Plan: Block wie immer.
/// let a = layout_with_measurer(&doc, vp, &narsil_layout_engine::default_measurer())
///     .expect("layout");
/// // Mit leerem Plan: Bit fuer Bit dasselbe.
/// let b = layout_mit_plan(&doc, vp, &narsil_layout_engine::default_measurer(), &LayoutPlan::leer())
///     .expect("layout");
/// assert_eq!(a, b, "ein leerer Plan darf nichts aendern");
/// ```
/// P12 (05.10.2026, 18:10): WERKZEUG — jeder Layout-Lauf meldet, OB
/// ER EINEN PLAN HAT.
///
/// # DER BEFUND, DER DIESES WERKZEUG NOETIG MACHT
///
/// `narsil-page` liefert die Kaskade in zwei Haeutern: `LayoutStil` und
/// `Grenzen` (min/max-width/height). Beide liegen in `seite.layout_plan`,
/// und `layout_mit_plan` bekommt diesen Plan. **`layout_with_measurer`
/// bekommt keinen** — es ist derselbe Layoutlauf ohne Plan.
///
/// GEMESSEN am 05.10. 18:04, gespeicherte nixos-Seite mit CSS:
///
/// ```text
/// ERSATZ #6 … max_b=Some(900.0)  explicit=Some(900.0)xSome(251.86)
/// ERSATZ #6 … max_b=None         explicit=Some(7129.0)xSome(1995.0)
/// ```
///
/// Zwei Laeufe, und der ZWEITE ist der, dessen Baum am Ende im Bild
/// steht (das Bild ist 7129 px breit, `ZUBREIT Hart 6229 px`). Der
/// Plan war also da — und wurde nachher weggeworfen.
///
/// DESHALB DER ZAEHLER: ein Werkzeug, das nur den ENDBAUM zeigt, kann
/// die beiden Laeufe nicht unterscheiden. Wer die Frage stellt
/// "wirkt der Plan noch?", braucht die Antwort je Lauf.
///
/// Ohne die Variable kostet der Aufruf einen `var_os` je Lauf (nicht
/// je Knoten) — der Zaehler laeuft trotzdem, das ist ein `fetch_add`.
fn planlauf_messen_an() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static GEPRUEFT: AtomicBool = AtomicBool::new(false);
    static AN: AtomicBool = AtomicBool::new(false);
    if !GEPRUEFT.load(Ordering::Relaxed) {
        let wert = std::env::var_os("NARSIL_PLANLAUF")
            .map(|v| v == "1" || v == "ja" || v == "true")
            .unwrap_or(false);
        AN.store(wert, Ordering::Relaxed);
        GEPRUEFT.store(true, Ordering::Relaxed);
    }
    AN.load(Ordering::Relaxed)
}

/// Wie viele Laeufe es gab. Die Reihenfolge der Drucke ist die
/// Reihenfolge der Laeufe, also ist der LETZTE Druck die Antwort auf
/// "welcher Baum steht am Ende im Bild".
static PLANLAEUFE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// EIN Druck: Laufnummer, Plan ja/nein, Knoten, Planeintraege.
fn planlauf_zeige(mit_plan: bool, knoten: usize, eintraege: usize) {
    use std::sync::atomic::Ordering;
    let n = PLANLAEUFE.fetch_add(1, Ordering::Relaxed) + 1;
    if planlauf_messen_an() {
        eprintln!(
            "PLANLAUF {n} {} eintraege={eintraege} knoten={knoten}",
            if mit_plan { "MIT_PLAN " } else { "OHNE_PLAN" }
        );
    }
}

pub fn layout_mit_plan(
    document: &Document,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    plan: &LayoutPlan,
) -> Result<LayoutTree, LayoutError> {
    planlauf_zeige(true, document.len(), plan.len());
    if !viewport.width.is_finite()
        || viewport.width <= 0.0
        || !viewport.height.is_finite()
        || viewport.height <= 0.0
    {
        return Err(LayoutError::InvalidViewport(viewport));
    }

    let root_id = document.root();
    let root_node = document
        .get(root_id)
        .ok_or(LayoutError::UnknownNode(root_id))?;
    if root_node.kind != NodeKind::Page {
        return Err(LayoutError::UnknownNode(root_id));
    }

    let ctx = Ctx::neu(document, measurer, Some(plan));
    rechnen(&ctx, root_id, viewport)
}

/// Wie [`layout`], aber mit injizierter Textmessung.
///
/// Damit kann eine andere Schrift eingespeist werden, ohne dass sich die
/// Layout-Logik aendert. Das ist der Einstieg fuer Folge-Tasks, die eine
/// eigene Messung haben:
///
/// ```
/// use narsil_core::geometry::Size;
/// use narsil_dom::Document;
/// use narsil_layout_engine::{EstimatedTextMeasurer, layout_with_measurer};
///
/// let doc = Document::new();
/// let viewport = Size::new(800.0, 600.0);
///
/// // Die Naeherung einspeisen — genauso ginge es mit einem eigenen
/// // TextMeasurer, ohne den Code dieser Funktion anzufassen.
/// let tree = layout_with_measurer(&doc, viewport, &EstimatedTextMeasurer)
///     .expect("gueltiges Dokument layoutet");
///
/// // Der page-Root ohne explizite Groesse fuellt den Viewport.
/// assert_eq!(tree.root.border_box.width, 800.0);
/// assert_eq!(tree.root.border_box.height, 600.0);
/// ```
///
/// Beispiele in den Tests: `FixedMeasurer` (Injektion), `default_measurer()`
/// (Naeherung) und `BitmapMeasurer::new()` (Standard von [`layout`]).
pub fn layout_with_measurer(
    document: &Document,
    viewport: Size,
    measurer: &dyn TextMeasurer,
) -> Result<LayoutTree, LayoutError> {
    // DIESER AUFRUF HAT KEINEN PLAN. `narsil-page` steckt die Kaskade in
    // `LayoutStil` und `Grenzen`, und beides liegt in `seite.layout_plan`
    // — dieser Einstieg sieht davon nichts. Wer `min-width`/`max-width`
    // (und jede andere Plantabelle) im Bild haben will, muss
    // `layout_mit_plan` rufen. GEMESSEN 18:04 auf nixos.org: der Plan
    // war beim ersten Lauf da (900 px) und beim zweiten weg (7129 px).
    planlauf_zeige(false, document.len(), 0);
    if !viewport.width.is_finite()
        || viewport.width <= 0.0
        || !viewport.height.is_finite()
        || viewport.height <= 0.0
    {
        return Err(LayoutError::InvalidViewport(viewport));
    }

    let root_id = document.root();
    let root_node = document
        .get(root_id)
        .ok_or(LayoutError::UnknownNode(root_id))?;
    if root_node.kind != NodeKind::Page {
        // Die Dokument-Invariante schliesst das eigentlich aus; der Vertrag
        // kennt aber keine eigene Variante (siehe FERTIG_0004.txt).
        return Err(LayoutError::UnknownNode(root_id));
    }

    let ctx = Ctx::neu(document, measurer, None);
    rechnen(&ctx, root_id, viewport)
}

/// DER EINZIGE RECHENWEG (P5, 02.10.2026).
///
/// Vorher standen Phase 1 und Phase 2 in `layout_mit_plan` UND in
/// `layout_with_measurer` — zweimal derselbe Aufruf. Das ist an sich
/// harmlos, hat aber beim Bauen der Bruecke zugeschnitten: nur
/// `layout_mit_plan` haengte die Knoten mit kaputter Angabe an den
/// Baum, `layout` (der Weg, den fast alle Tests nehmen) nicht. Zwei
/// Aufrufstellen, eine Regel — jetzt ist es eine.
fn rechnen(ctx: &Ctx<'_>, root_id: NodeId, viewport: Size) -> Result<LayoutTree, LayoutError> {
    // Phase 1: Groessen messen (iterativ). Phase 2: positionieren (iterativ).
    //
    // P14 (05.10.2026, 13:20): MITMESSEN, aber nur auf Ansage.
    //
    // GEMESSENER GRUND: `NARSIL_ZEITEN=1` in `narsil` misst eine Phase
    // namens `layout` — und die umfasst bei github.com beide Schritte
    // hier. Wer wissen will, ob die Mess- oder die Positionierphase
    // teuer ist, braucht die zwei Zahlen getrennt.
    //
    // Die Messung kostet ohne die Umgebungsvariable einen
    // `var_os`-Vergleich je Lauf, nicht je Knoten: sie steht hier und
    // nicht in `compute_sizes` oder `build_tree`, denn dort waere sie
    // pro Knoten.
    let messen = zeit_ang();
    let t1 = messen.then(std::time::Instant::now);
    // B11 (01:24): `compute_sizes` ist aus `box_sizing.rs` nach `box.rs`
    // gewandert (GEMESSEN: `box.rs:1257`). Der Aufrufer bleibt, der Weg
    // nicht — sonst waere es ein zweiter Name fuer dieselbe Rechnung.
    let sizes = r#box::compute_sizes(ctx, root_id, viewport)?;
    let t2 = messen.then(std::time::Instant::now);
    let root = flow::build_tree(ctx, root_id, viewport, &sizes)?;
    if messen && let (Some(a), Some(b)) = (t1, t2) {
        // stderr, nicht stdout: die Tabelle des Pruefstands geht nach
        // stdout, und eine Zusatzzeile darin wuerde sie zerlegen.
        eprintln!(
            "NARSIL_LAYOUT messen={:.2}ms positionieren={:.2}ms knoten={}",
            a.elapsed().as_secs_f64() * 1000.0,
            b.elapsed().as_secs_f64() * 1000.0,
            ctx.document.len()
        );
    }
    // Release 11.10.2026: die aufgeloesten Rastervorlagen fuer
    // `getComputedStyle` (CSS Grid 1 §7.2.6).
    let vorlagen = gitter::aufgeloeste_vorlagen(ctx);
    Ok(LayoutTree::new(root, viewport).mit_verworfenen(sizes.verworfene_knoten()).mit_raster_vorlagen(vorlagen))
}

/// Ist die Zeitmessung eingeschaltet? Wird einmal gelesen und
/// gemerkt — `std::env::var_os` in einer Layout-Schleife waere
/// Verschwendung.
fn zeit_ang() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static GEPRUEFT: AtomicBool = AtomicBool::new(false);
    static AN: AtomicBool = AtomicBool::new(false);
    if !GEPRUEFT.load(Ordering::Relaxed) {
        let wert = std::env::var_os("NARSIL_LAYOUT_ZEIT")
            .map(|v| v == "1" || v == "ja" || v == "true")
            .unwrap_or(false);
        AN.store(wert, Ordering::Relaxed);
        GEPRUEFT.store(true, Ordering::Relaxed);
    }
    AN.load(Ordering::Relaxed)
}
