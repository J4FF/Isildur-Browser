//! Layout-Baum: das Ergebnis der Layout-Phase.
//!
//! Der Layout-Baum liegt zwischen `narsil_dom::Document` und
//! `narsil_display::DisplayList`. Er behaelt die Baumstruktur und die Zuordnung
//! zum DOM-Knoten (fuer spaeteres Event-Handling), aber die Geometrie ist
//! bereits absolut aufgeloest: keine Prozentwerte, kein `Auto`, keine
//! nachzuladenden Schriftmetriken mehr.
//!
//! WICHTIG: Dieser Crate enthaelt nur die DATENSTRUKTUR und die
//! Startausloesung. Die Layout-Logik (Flow, Grids, Flex) gehoert in
//! `narsil-layout-engine` (siehe TASK_0005).

use narsil_core::geometry::{Point, Rect, Size};
use narsil_core::style::Style;
use narsil_dom::NodeId;

/// Ein Stueck Text mit bereits gesetzter Ursprungposition.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// Linke OBERE Ecke der Zeile — NICHT die Grundlinie.
    ///
    /// An diesem Punkt beginnt die erste Glyphe, Pixel fuer Pixel.
    /// Die Grundlinie liegt `font_size * 5/7` Pixel darunter; sie wird
    /// nirgends in `origin` hineingerechnet und muss es auch nicht.
    ///
    /// Der Aufrufer von `narsil_text::layout_text` reicht dieses `origin`
    /// 1:1 durch. Wer zusaetzlich den Ascent addiert, setzt den Text
    /// um `font_size * 5/7` Pixel zu tief — bei 24 px sind das 17,14 px,
    /// bei 16 px immer noch 11,4 px. Gemessen in TASK_0013A, siehe
    /// `Agent4/ANTWORT_BASELINE.txt`.
    pub origin: Point,
    /// Der anzuzeigende Text.
    pub text: String,
    /// Textfarbe.
    pub color: narsil_core::color::Color,
    /// Schriftgroesse in Pixeln.
    pub font_size: f32,
    /// Textdekoration als Bitmaske (A3): 1 = `underline`, 2 = `overline`,
    /// 4 = `line-through`. Bitgleich zu `narsil_css::Berechnet::dekoration`
    /// und zu `narsil_core::style::Style::dekoration`.
    ///
    /// GEMESSEN 02.10.2026: die Kette hatte sieben Stationen, weil
    /// `node.style` in `make_text_run` `narsil_core::style::Style` ist und
    /// nicht `Berechnet`. Diese eine ist die letzte davor.
    pub deko: u8,
    /// `font-weight`, 100..=900 (CSS Fonts 4 §5.3). Aus `node.style`.
    ///
    /// GEMESSEN 04.10.2026 (SCHRIFT): ohne dieses Feld kam **jeder** Text
    /// als `normal` an — Firefox 127 / 144 / 129 / 143 px fuer normal /
    /// fett / kursiv / fett-kursiv, narsil 125 / 125 / 125 / 125. Der Weg
    /// ist additiv; ein Lauf ohne die Kaskade (Hand-Typografie) bekommt
    /// die Vorgabe 400.
    pub gewicht: u16,
    /// `font-style: italic` oder `oblique`. Aus `node.style`.
    pub kursiv: bool,
    /// `font-family` als CSS-Liste (CSS Fonts 4 §4.2), aus `node.style`.
    ///
    /// GEMESSEN 04.10.2026 (SCHRIFT, (c)): `code`, `kbd` und `samp`
    /// standen bei GEMESSEN 14,4 px **in der Fliesstextschrift**, obwohl
    /// das UA-Blatt fuer sie `monospace` sagt. Die Liste reist wie Gewicht
    /// und Stil am Lauf: sie entscheidet, **welche** Datei gezeichnet wird.
    pub schriftfamilie: String,
}

/// Eine geometrisch aufgeloeste Box.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBox {
    /// Herkunftsknoten im Dokumentbaum.
    pub node: NodeId,
    /// Aussenmasse inklusive Border und Padding.
    pub border_box: Rect,
    /// Innenflaeche fuer Kind-Elemente und Text.
    pub content_box: Rect,
    /// Wo der Hintergrund gezeichnet wird (`None` = kein Hintergrund).
    pub background: Option<Rect>,
    /// Text, falls dieser Kasten ein Textknoten ist.
    pub text: Option<TextRun>,
    /// Kinder in Dokumentreihenfolge.
    pub children: Vec<LayoutBox>,
    /// `position` (P8, 02.10.2026).
    ///
    /// WARUM ES AM KASTEN UND NICHT NUR IM PLAN STEHT: zwei, die es
    /// brauchen, koennen den Plan nicht lesen. Die Display-Liste
    /// (`build_display_list`) bekommt nur Baum und Dokument, und
    /// `narsil` wendet den Scrollversatz auf die fertige Liste an. Beide
    /// brauchen `position` — fuer die MAL-REIHENFOLGE (CSS 2.1, Anhang E)
    /// und dafuer, dass ein `fixed`-Kasten beim Scrollen NICHT mitwandert.
    ///
    /// GEMESSENER FEHLER, den dieses Feld behebt: `schicht_von` und
    /// `reihenfolge` (CSS 2.1, Anhang E) waren gebaut, exportiert und
    /// getestet — und wurden von NIEMAND aufgerufen. `build_display_list
    /// lief in Tiefensuche Dokumentreihenfolge, also ueberholte ein
    /// `position: absolute` mit `z-index: 9999` alles, was nach ihm im
    /// Dokument stand. Ein vorhandener, gebauter, unbenutzter
    /// Schnellpfad — dieselbe Art von Befund wie der Vormerker (P7).
    pub position: plan::PositionArt,
    /// `z-index` (P8). `None` = `auto`.
    pub z_index: Option<i32>,
    /// Ist dieser Kasten ein **Flex-Container** (`display: flex`)?
    ///
    /// L5 (02.10.2026 13:2x), CSS Flexbox 1 §5.4: `z-index` gilt fuer
    /// Flex-Items — **auch** wenn sie `position: static` haben. Wer
    /// nicht weiss, ob ein Kasten Flex-Container ist, kann das nicht
    /// entscheiden: ein statischer Kasten ist einmal ein gewoehnliches
    /// Block-Kind (dort wirkt `z-index` nicht) und einmal ein Flex-Item
    /// (dort schon).
    ///
    /// GEMESSENER BEFUND, warum das Feld noetig ist und nicht eine
    /// Ableitung aus `children`: `build_display_list` bekommt NUR Baum und
    /// Dokument (siehe das Feld `position` oben, derselbe Grund), und
    /// `LayoutBox` hat keinen Zeiger auf den Elternknoten. Es gibt also
    /// keinen Weg, im Waehler von einem Kind aus zum Elternknoten zu
    /// kommen — `sammle_reihenfolge` weiss beim Elternknoten, dass er
    /// Flex-Container ist, und nicht spaeter.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `false` der Anfangswert ist: der
    /// Kasten kommt aus dem Plan, und ein Kasten OHNE Planeintrag ist
    /// gewoehnlicher Blockfluss. Damit bleibt ein alter Aufbau genau so
    /// wie vorher — additiv, wie `position` es auch war.
    pub ist_flex_container: bool,
    /// `transform` oder `filter` mit einem anderen Wert als `none`
    /// (CSS Transforms 1 §3, CSS Filter Effects 1 §2.1).
    ///
    /// L5 (02.10.2026 17:0x). Derselbe Grund wie bei `position`,
    /// `z_index` und `ist_flex_container`: `build_display_list` bekommt
    /// Baum und Dokument, nicht den Plan.
    pub transformt: bool,
    /// Release 11.10.2026: die Transformation selbst (`transform`,
    /// `translate`/`rotate`/`scale`) — angewandt beim Zeichnen.
    pub transform: Option<Box<plan::Transformation>>,
    /// Release 11.10.2026: `background-clip` (0 border-box, 1 padding-box,
    /// 2 content-box, 3 text).
    pub hintergrund_clip: u8,
    /// Begrenzung der Nachfahren beim Zeichnen.
    pub ueberlauf: plan::UeberlaufArt,
    /// Release 11.10.2026 (Agent CSS2): ein nicht positionierter Float
    /// (`float: left|right`). Die Mal-Reihenfolge braucht ihn (CSS 2.1
    /// Anhang E, Schritt 5: Floats nach den Hintergruenden der
    /// Blockkaesten im Fluss); `build_display_list` bekommt den Plan nicht.
    pub schwebt: bool,
    /// Release 11.10.2026 (Agent CSS2): wo Rahmen und Hintergrund gezeichnet
    /// werden, wenn das nicht die Rahmenbox ist — bei einer Tabelle mit
    /// Beschriftung das Raster (CSS 2.1 §17.4): die Rahmenbox ist dann die
    /// Huelle samt Beschriftungen (das melden `offsetHeight` und
    /// `getBoundingClientRect`), Rahmen und Hintergrund gehoeren dem Raster.
    pub mal_kasten: Option<Rect>,
    /// Release 12.10.2026 (Agent CSS2): `letter-spacing`/`word-spacing` — die
    /// Teile des Textlaufs (Byte-Anfang, x-Versatz vom Ursprung), gerechnet
    /// mit der Messung des Layouts; `None` ohne Abstand (siehe
    /// `narsil_display::DisplayList::versaetze_bei`).
    pub abstands_versaetze: Option<Box<[(u32, f32)]>>,
    /// Release 11.10.2026: `order` des Kastens — Flex- und Rasterelemente
    /// malen in `order`-Reihenfolge (CSS Display 3 „order-modified document
    /// order"); die Kinderliste bleibt in Dokumentreihenfolge.
    pub ordnung: i32,
}

impl LayoutBox {
    /// Release 11.10.2026: die Abbildung `p -> s·p + (dx, dy)`, mit der
    /// dieser Kasten (samt Teilbaum) gezeichnet wird, wenn sein Vorfahr mit
    /// `aussen` gezeichnet wird (`(1, 0, 0)` an der Wurzel). Verschiebung und
    /// gleichmaessige Skalierung sind genau; Drehung, Scherung und ungleiche
    /// Skalierung werden durch die Skalierung mit derselben Flaeche
    /// (`sqrt(|det|)`) um den Punkt ersetzt, auf den die Matrix die
    /// Kastenmitte bringt — so bleibt ein gedrehtes Symbol an seinem Platz,
    /// und `scaleY(0)` verschwindet.
    #[must_use]
    pub fn abbildung(&self, aussen: (f32, f32, f32)) -> (f32, f32, f32) {
        let Some(t) = self.transform.as_deref() else { return aussen };
        let r = self.border_box;
        let [a, b, c, d, e, f] = t.in_seite(r.x, r.y, r.width, r.height);
        let lokal = if b.abs() < 1e-6 && c.abs() < 1e-6 && (a - d).abs() < 1e-6 && a >= 0.0 {
            (a, e, f)
        } else {
            let s = (a * d - b * c).abs().sqrt();
            let s = if (s - 1.0).abs() < 1e-5 { 1.0 } else { s };
            let (mx, my) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
            let (zx, zy) = (a * mx + c * my + e, b * mx + d * my + f);
            (s, zx - s * mx, zy - s * my)
        };
        let glatt = |v: f32| if v.abs() < 1e-3 { 0.0 } else { v };
        let lokal = (lokal.0, glatt(lokal.1), glatt(lokal.2));
        let neu = (aussen.0 * lokal.0, aussen.0 * lokal.1 + aussen.1, aussen.0 * lokal.2 + aussen.2);
        if neu.0.is_finite() && neu.1.is_finite() && neu.2.is_finite() { neu } else { aussen }
    }

    /// Konstruiert eine Box ohne Kinder und ohne Text.
    pub fn new(node: NodeId, border_box: Rect) -> Self {
        Self {
            node,
            content_box: border_box,
            border_box,
            background: None,
            text: None,
            children: Vec::new(),
            // P8: der Anfangswert ist ein gewöhnlicher Block im
            // Normalfluss. Ein Kasten, der nicht aus dem Plan kommt,
            // verhaelt sich damit wie vorher — das ist der Grund, warum
            // dieses Feld additiv sein darf.
            position: plan::PositionArt::Statisch,
            z_index: None,
            ist_flex_container: false,
            transformt: false,
            transform: None,
            hintergrund_clip: 0,
            ueberlauf: plan::UeberlaufArt::Sichtbar,
            schwebt: false,
            mal_kasten: None,
            abstands_versaetze: None,
            ordnung: 0,
        }
    }

    /// Setzt Kind-Boxen.
    pub fn with_children(mut self, children: Vec<LayoutBox>) -> Self {
        self.children = children;
        self
    }

    /// Setzt den Hintergrund auf das Border-Box-Rechteck.
    pub fn with_background(mut self, rect: Rect) -> Self {
        self.background = Some(rect);
        self
    }

    /// Setzt den Text-Inhalt.
    pub fn with_text(mut self, run: TextRun) -> Self {
        self.text = Some(run);
        self
    }
}

/// Der vollstaendige Layout-Baum eines Dokuments.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutTree {
    /// Wurzel-Box (entspricht dem `page`-Knoten).
    pub root: LayoutBox,
    /// Verfuegbare Flaeche des Viewports.
    pub viewport: Size,
    /// Knoten, deren Stilangabe unbrauchbar war und die darum den
    /// Standard bekommen haben — mit der Begruendung, in Dokumentreihenfolge.
    ///
    /// GEMESSENE REGRESSION 02.10.2026 (P5): `https://archlinux.org/`
    /// liess sich mit der Bruecke aus `WUNSCH_LAYOUT.txt` nicht mehr
    /// laden — `Layout: Ungueltige Laenge bei Knoten NodeId(9):
    /// font-size: 0 muss endlich und > 0 sein`. EIN Knoten hat die ganze
    /// Seite verworfen. Jetzt bekommt nur er den Standardstil, und diese
    /// Liste sagt, wie viele es waren (M7: mitzaehlen, nicht
    /// schweigend reparieren).
    pub verworfene: Vec<(NodeId, String)>,
    /// Release 11.10.2026: der AUFGELOESTE Wert von `grid-template-columns`
    /// und `grid-template-rows` je Raster (CSS Grid 1 §7.2.6: jede Spur
    /// einzeln in px, mit den Liniennamen der expliziten Linien) — fuer
    /// `getComputedStyle`. (Knoten, Spalten, Zeilen).
    pub raster_vorlagen: Vec<(NodeId, String, String)>,
}

impl LayoutTree {
    /// Konstruiert einen Layout-Baum aus Wurzel und Viewport.
    pub fn new(root: LayoutBox, viewport: Size) -> Self {
        Self {
            root,
            viewport,
            verworfene: Vec::new(),
            raster_vorlagen: Vec::new(),
        }
    }

    /// Release 11.10.2026: haengt die aufgeloesten Rastervorlagen an.
    pub fn mit_raster_vorlagen(mut self, v: Vec<(NodeId, String, String)>) -> Self {
        self.raster_vorlagen = v;
        self
    }

    /// Haengt Knoten an, deren Angabe unbrauchbar war (P5, 02.10.2026).
    ///
    /// Additiv: ein Aufrufer, der sich nicht fuer die Liste interessiert,
    /// sieht eine leere.
    pub fn mit_verworfenen(mut self, v: Vec<(NodeId, String)>) -> Self {
        self.verworfene = v;
        self
    }

    /// Kleinstes Rechteck, das alle SICHTBAREN Teile einhüllt.
    ///
    /// SICHTBAR heisst: hat einen Hintergrund ODER Text. Ein durchsichtiger
    /// Container geht nicht ein — auch seine Kinder nicht. Ein Kind bei
    /// (500,500) der Groesse 100x100 ohne Hintergrund, unter einer Wurzel
    /// mit Hintergrund (0,0,100,100), ergibt deshalb `(0,0,100,100)` und
    /// nicht `(0,0,600,600)`. Gemessen in TASK_0013A, Agent4, Befund 6.
    ///
    /// Vorher stand hier "das die gesamte Box einhüllt". Das war falsch
    /// und auf drei Arten gefährlich: die Formulierung sagt das Gegenteil,
    /// das Verhalten ist beabsichtigt, und nichts im Crate sagte, was
    /// "sichtbar" bedeutet.
    ///
    /// ENTSCHEIDUNG, nicht Versehen: eine unsichtbare Box soll die
    /// Bounding-Box nicht aufblaehen. Fuer TASK_0009 ist `bounds()`
    /// ohnehin die falsche Groesse — dort zaehlt der Viewport, nicht
    /// der Inhalt, sonst wird ein leeres Dokument zu 0x0.
    ///
    /// `None`, wenn der Baum leer ist (Wurzel ohne Kinder und ohne Text).
    pub fn bounds(&self) -> Option<Rect> {
        fn union_box(b: &LayoutBox) -> Option<Rect> {
            let mut acc = b.background;
            if b.text.is_some() && acc.is_none() {
                acc = Some(b.border_box);
            }
            for child in &b.children {
                acc = match (acc, union_box(child)) {
                    (None, Some(r)) => Some(r),
                    (Some(a), Some(r)) => a.union(r),
                    (a, None) => a,
                };
            }
            acc
        }
        union_box(&self.root)
    }
}

/// Der deklarierte Stil-Typ wird hier nur weitergereicht.
pub type BoxStyle = Style;

/// DIE MESSUNG: eine Schrift, eine Zahl, drei Verbraucher (W5G).
///
/// Siehe [`messung::Messung`]. `narsil-page` (Umbruch),
/// `narsil-layout-engine` (Boxbreiten) und `narsil-raster` (Zeichnen)
/// benutzen dieselben Funktionen — das ist der ganze Zweck.
pub mod messung;

/// DER LAYOUT-PLAN: `display:flex/grid`, `position`, `float` als Daten,
/// die von der Kaskade ins Layout kommen.
///
/// Siehe das Modul-Doc: der Plan ist eine Nebentabelle neben
/// `narsil_core::style::Style`, weil `Style` diese Felder nicht hat und
/// ein neues Feld 25 Struct-Literale in fuenf fremden Crates brechen
/// wuerde (Welle 7, Rolle LAYOUT).
pub mod plan;

/// Release 11.10.2026: CSS Grid als Plan-Typen (Spurlisten, Linien,
/// Ausrichtung) fuer den neuen Rasteralgorithmus.
pub mod gitter;
pub mod flexstil;

// 04.10.2026 (SCHRIFT_9): `FamilienStapeln` ist der Haken, mit dem
// die **Messung** eine `font-family` aufloest — GEMESSENER Anlass:
// Seriftext wurde mit Sans-Metriken umbrochen (1267,88 px gegen
// 1292 px Tinte auf 152 Zeichen).
pub use messung::{FamilienStapeln, MAX_ZEICHEN, Messung, SCHNITTE, schnitt_index};
pub use plan::{
    Ausrichtung, Buendig, FlexRichtung, FlexUmbruch, FloatArt, Grenzen, LayoutPlan, LayoutStil,
    MAX_FLEX_KINDER, MAX_PLAN_KNOTEN, MAX_RASTER_SPALTEN, MAX_RICHTUNGEN, PositionArt, RaeumungArt,
    Richtung, Spalte, Textausrichtung, Transformation, UeberlaufArt, Vertikal,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_box_defaults() {
        let b = LayoutBox::new(NodeId(0), Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(b.content_box, b.border_box);
        assert!(b.background.is_none());
        assert!(b.text.is_none());
        assert!(b.children.is_empty());
    }

    #[test]
    fn layout_box_builders() {
        let b = LayoutBox::new(NodeId(1), Rect::new(0.0, 0.0, 10.0, 10.0))
            .with_background(Rect::new(0.0, 0.0, 10.0, 10.0))
            .with_text(TextRun {
                schriftfamilie: String::from("sans-serif"),
                deko: 0,
                origin: Point::new(1.0, 1.0),
                text: "x".into(),
                color: narsil_core::color::Color::WHITE,
                font_size: 12.0,
                // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `TextRun`.
                // 400/false ist `normal` (CSS Fonts 4 §5.3) — dieselbe
                // Vorgabe wie in `Style`, aus der der Lauf gefuellt wird.
                gewicht: 400,
                kursiv: false,
            });
        assert!(b.background.is_some());
        assert_eq!(b.text.unwrap().text, "x");
    }

    #[test]
    fn tree_bounds_union_children() {
        let child = LayoutBox::new(NodeId(1), Rect::new(50.0, 60.0, 10.0, 10.0))
            .with_background(Rect::new(50.0, 60.0, 10.0, 10.0));
        let root = LayoutBox::new(NodeId(0), Rect::new(0.0, 0.0, 10.0, 10.0))
            .with_background(Rect::new(0.0, 0.0, 10.0, 10.0))
            .with_children(vec![child]);
        let tree = LayoutTree::new(root, Size::new(800.0, 600.0));
        let b = tree.bounds().unwrap();
        assert_eq!(b, Rect::new(0.0, 0.0, 60.0, 70.0));
    }

    #[test]
    fn tree_bounds_none_when_empty() {
        let root = LayoutBox::new(NodeId(0), Rect::ZERO);
        let tree = LayoutTree::new(root, Size::new(100.0, 100.0));
        assert!(tree.bounds().is_none());
    }
}
