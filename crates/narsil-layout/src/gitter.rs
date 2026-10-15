//! Release 11.10.2026: CSS Grid 1/2 als Plan-Typen — Spurlisten mit
//! Liniennamen, Spurgroessen-Funktionen, Linienangaben der Kinder und die
//! Ausrichtungswerte (CSS Box Alignment 3), so wie die Kaskade sie liefert.
//!
//! GEMESSEN vorher (WPT css-grid, 564 von 2460 gruen): die alte Form
//! (`plan::Spalte`) kannte weder `min-content`/`max-content`/`fit-content()`
//! noch `minmax(auto, …)`, keine Liniennamen an den Kindern, keine
//! negativen impliziten Spuren und hoechstens 32 Spalten. Der neue
//! Rasteralgorithmus (`narsil-layout-engine`, `gitter.rs`) liest diese
//! Typen; die alte Form bleibt fuer Masonry und die alten Tests stehen.

/// Eine Laenge mit Prozentanteil (`calc(10px + 20%)` = `px 10, prozent 20`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LaengeProzent {
    /// Fester Anteil in px.
    pub px: f32,
    /// Prozentanteil (20 = 20 %).
    pub prozent: f32,
    /// Kommt ueberhaupt ein Prozent vor? (Dann braucht die Laenge eine Basis.)
    pub hat_prozent: bool,
}

impl LaengeProzent {
    /// Eine reine px-Laenge.
    #[must_use]
    pub const fn px(v: f32) -> Self {
        Self { px: v, prozent: 0.0, hat_prozent: false }
    }

    /// Gegen `basis` aufloesen; ohne Basis (`None`) und mit Prozent: `None`.
    #[must_use]
    pub fn aufloesen(&self, basis: Option<f32>) -> Option<f32> {
        if !self.hat_prozent {
            return Some(self.px);
        }
        let b = basis?;
        let v = self.px + self.prozent * b / 100.0;
        v.is_finite().then_some(v)
    }

    /// Wie [`aufloesen`](Self::aufloesen), aber ohne Basis zaehlt der
    /// Prozentanteil als 0 (CSS Grid 1 §10.1 fuer Luecken bei unbestimmter
    /// Groesse).
    #[must_use]
    pub fn oder_null(&self, basis: Option<f32>) -> f32 {
        match basis {
            Some(b) => self.aufloesen(Some(b)).unwrap_or(self.px),
            None => self.px,
        }
    }
}

/// Eine Spurgrenze (`<track-breadth>`, CSS Grid 1 §7.2.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpurGrenze {
    /// `<length-percentage>`.
    Laenge(LaengeProzent),
    /// `<flex>` (`fr`), nur als Obergrenze gueltig.
    Anteil(f32),
    /// `min-content`.
    MinInhalt,
    /// `max-content`.
    MaxInhalt,
    /// `auto`.
    Auto,
}

/// Eine Spurgroessen-Funktion (`minmax(min, max)`; eine einzelne Angabe ist
/// `minmax(x, x)`, ein `fr` allein `minmax(auto, Nfr)`, `fit-content(L)`
/// ist `minmax(auto, max-content)` mit der Kappe `L`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spur {
    /// Die Untergrenze.
    pub min: SpurGrenze,
    /// Die Obergrenze.
    pub max: SpurGrenze,
    /// Das Argument von `fit-content()`.
    pub passend: Option<LaengeProzent>,
}

impl Spur {
    /// `auto` (= `minmax(auto, auto)`), der Anfangswert impliziter Spuren.
    pub const AUTO: Spur = Spur { min: SpurGrenze::Auto, max: SpurGrenze::Auto, passend: None };

    /// Eine feste Spur.
    #[must_use]
    pub const fn fest(px: f32) -> Spur {
        Spur {
            min: SpurGrenze::Laenge(LaengeProzent::px(px)),
            max: SpurGrenze::Laenge(LaengeProzent::px(px)),
            passend: None,
        }
    }
}

/// Ein Teil einer Spurliste, in der Reihenfolge der Angabe. Feste
/// `repeat(N, …)` sind schon ausgeschrieben; nur `auto-fill`/`auto-fit`
/// bleibt als Gruppe stehen (die Zahl kennt erst das Layout).
#[derive(Debug, Clone, PartialEq)]
pub enum SpurTeil {
    /// `[a b]` — Liniennamen an der Linie an dieser Stelle.
    Namen(Vec<String>),
    /// Eine Spur.
    Spur(Spur),
    /// `repeat(auto-fill | auto-fit, …)`: die Teile (nur Namen und Spuren).
    Auto {
        /// `auto-fit` (leere Wiederholungen kollabieren, §7.2.3.2).
        fit: bool,
        /// Der Inhalt der Wiederholung.
        teile: Vec<SpurTeil>,
    },
}

/// `grid-template-columns`/`-rows` (CSS Grid 1 §7.2; `subgrid` aus Grid 2).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpurListe {
    /// Die Teile in Angabe-Reihenfolge (leer = `none`).
    pub teile: Vec<SpurTeil>,
    /// `subgrid` (dann tragen die Teile nur Namen).
    pub subgrid: bool,
}

impl SpurListe {
    /// Gibt es eine `auto-fill`/`auto-fit`-Gruppe?
    #[must_use]
    pub fn hat_auto_wiederholung(&self) -> bool {
        self.teile.iter().any(|t| matches!(t, SpurTeil::Auto { .. }))
    }
}

/// Eine Linienangabe (`grid-row-start` usw., CSS Grid 1 §8.3).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum GitterLinie {
    /// `auto`.
    #[default]
    Auto,
    /// `<integer> <custom-ident>?` (Zahl != 0).
    Zahl(i32, Option<String>),
    /// `<custom-ident>` allein.
    Name(String),
    /// `span <integer>? <custom-ident>?` (Zahl >= 1).
    Spanne(u32, Option<String>),
}

impl GitterLinie {
    /// Ist die Angabe `auto`?
    #[must_use]
    pub fn ist_auto(&self) -> bool {
        matches!(self, GitterLinie::Auto)
    }
}

/// Die Lage eines Rasterelements (`grid-row`/`grid-column`/`grid-area`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GitterPlatz {
    /// `grid-row-start`.
    pub zeile_start: GitterLinie,
    /// `grid-row-end`.
    pub zeile_ende: GitterLinie,
    /// `grid-column-start`.
    pub spalte_start: GitterLinie,
    /// `grid-column-end`.
    pub spalte_ende: GitterLinie,
}

impl GitterPlatz {
    /// Alle vier `auto`?
    #[must_use]
    pub fn ist_auto(&self) -> bool {
        self.zeile_start.ist_auto() && self.zeile_ende.ist_auto() && self.spalte_start.ist_auto() && self.spalte_ende.ist_auto()
    }
}

/// Die Lage einer Ausrichtung (CSS Box Alignment 3 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AusrLage {
    /// `normal` (bzw. `auto` bei `*-self`: nicht gesetzt).
    #[default]
    Normal,
    /// `auto` (nur `*-self`: den Wert von `*-items` des Containers nehmen).
    Auto,
    /// `stretch`.
    Strecken,
    /// `start`, `self-start`, `flex-start`.
    Anfang,
    /// `end`, `self-end`, `flex-end`.
    Ende,
    /// `center`.
    Mitte,
    /// `left` (physisch).
    Links,
    /// `right` (physisch).
    Rechts,
    /// `baseline`/`first baseline`.
    Grundlinie,
    /// `last baseline`.
    LetzteGrundlinie,
    /// `space-between`.
    Zwischen,
    /// `space-around`.
    Ringsum,
    /// `space-evenly`.
    Gleichmaessig,
}

/// Eine Ausrichtung samt `safe`/`unsafe` (CSS Box Alignment 3 §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ausr {
    /// Die Lage.
    pub lage: AusrLage,
    /// `Some(true)` = `safe`, `Some(false)` = `unsafe`, `None` = keine Angabe.
    pub sicher: Option<bool>,
    /// Bei `*-content` mit Verteilung: die Ausweichlage nach `<content-position>`.
    pub ausweich: Option<AusrLage>,
    /// Release 11.10.2026: `flex-start`/`flex-end` (bezogen auf die
    /// Flex-Achse) statt `start`/`end` (bezogen auf die Schreibrichtung) —
    /// im Flex-Container mit `*-reverse`/`wrap-reverse` verschieden (CSS Box
    /// Alignment 3 §4.2). Fuers Raster gleich.
    pub flex_bezug: bool,
    /// Release 11.10.2026: `self-start`/`self-end` — bezogen auf die
    /// Schreibrichtung des Elements selbst, nicht des Containers.
    pub selbst_bezug: bool,
}

impl Ausr {
    /// Der Anfangswert (`normal`).
    pub const NORMAL: Ausr = Ausr { lage: AusrLage::Normal, sicher: None, ausweich: None, flex_bezug: false, selbst_bezug: false };
    /// Der Anfangswert der `*-self`-Eigenschaften (`auto`).
    pub const AUTO: Ausr = Ausr { lage: AusrLage::Auto, sicher: None, ausweich: None, flex_bezug: false, selbst_bezug: false };
}

/// Die Rastereigenschaften eines Containers (`display: grid`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GitterStil {
    /// `grid-template-columns`.
    pub spalten: SpurListe,
    /// `grid-template-rows`.
    pub zeilen: SpurListe,
    /// `grid-auto-columns` (leer = `auto`).
    pub auto_spalten: Vec<Spur>,
    /// `grid-auto-rows` (leer = `auto`).
    pub auto_zeilen: Vec<Spur>,
    /// `grid-template-areas`: Zeilen von Namen (leer = Punkt-Zelle).
    pub gebiete: Vec<Vec<String>>,
    /// `grid-auto-flow: column`.
    pub fluss_spalte: bool,
    /// `grid-auto-flow: dense`.
    pub dicht: bool,
    /// `column-gap` (`None` = `normal` = 0 im Raster).
    pub luecke_spalte: Option<LaengeProzent>,
    /// `row-gap`.
    pub luecke_zeile: Option<LaengeProzent>,
    /// `justify-content`.
    pub justify_content: Ausr,
    /// `align-content`.
    pub align_content: Ausr,
    /// `justify-items`.
    pub justify_items: Ausr,
    /// `align-items`.
    pub align_items: Ausr,
    /// `direction: rtl` — die Spalten laufen von rechts nach links.
    pub rtl: bool,
}

/// Die Rastereigenschaften eines Kindes (Lage und `*-self`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GitterKindStil {
    /// Die vier Linienangaben.
    pub platz: GitterPlatz,
    /// `justify-self`.
    pub justify_self: Ausr,
    /// `align-self`.
    pub align_self: Ausr,
}

/// Die Rasterwerte eines Elements, wie die Kaskade sie berechnet (alle
/// Anfangswerte = `Default`). Daraus baut der Layoutplan [`GitterStil`] und
/// [`GitterKindStil`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GitterWerte {
    /// `grid-template-columns`.
    pub spalten: SpurListe,
    /// `grid-template-rows`.
    pub zeilen: SpurListe,
    /// `grid-auto-columns`.
    pub auto_spalten: Vec<Spur>,
    /// `grid-auto-rows`.
    pub auto_zeilen: Vec<Spur>,
    /// `grid-template-areas`.
    pub gebiete: Vec<Vec<String>>,
    /// Die Linienangaben als Rasterelement.
    pub platz: GitterPlatz,
    /// `justify-content`.
    pub justify_content: Ausr,
    /// `align-content`.
    pub align_content: Ausr,
    /// `justify-items`.
    pub justify_items: Ausr,
    /// `align-items`.
    pub align_items: Ausr,
    /// `justify-self`.
    pub justify_self: Ausr,
    /// `align-self`.
    pub align_self: Ausr,
    /// `column-gap` mit Prozentanteil (`None` = `normal`).
    pub luecke_spalte: Option<LaengeProzent>,
    /// `row-gap` mit Prozentanteil.
    pub luecke_zeile: Option<LaengeProzent>,
}
