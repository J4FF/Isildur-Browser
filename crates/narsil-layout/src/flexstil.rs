//! Release 11.10.2026: CSS Flexbox 1 als Plan-Typen — `flex-basis` mit
//! Prozent und Schluesselwoertern, `flex-wrap: wrap-reverse` und die
//! Ausrichtungswerte (CSS Box Alignment 3) des Containers.
//!
//! GEMESSEN vorher (WPT css-flexbox, 548 von 1389 gruen): `flex-basis`
//! kannte nur px (Prozent gegen die Ansicht, `content` wurde verworfen),
//! `wrap-reverse` war `nowrap`, und `justify-content`/`align-*` kannten
//! weder `safe`/`unsafe` noch `space-evenly` oder die Grundlinie. Der neue
//! Flex-Algorithmus (`narsil-layout-engine`, `flexbox.rs`) liest diese Typen.

use crate::gitter::{Ausr, LaengeProzent};

/// `flex-basis` (CSS Flexbox 1 §7.3.3).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FlexBasis {
    /// `auto`: die Hauptgroesse des Elements (`width`/`height`), bei `auto`
    /// wie `content`.
    #[default]
    Auto,
    /// `content`: die Groesse aus dem Inhalt.
    Inhalt,
    /// `<length-percentage>` (Prozent gegen die innere Hauptgroesse des
    /// Containers).
    Laenge(LaengeProzent),
    /// `min-content`.
    MinInhalt,
    /// `max-content`.
    MaxInhalt,
    /// `fit-content`.
    Passend,
}

/// Die Flex-Eigenschaften eines Containers (`display: flex`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlexStil {
    /// `flex-direction: row | row-reverse` (sonst `column…`).
    pub reihe: bool,
    /// `row-reverse` / `column-reverse`.
    pub umgekehrt: bool,
    /// `flex-wrap: wrap | wrap-reverse`.
    pub umbruch: bool,
    /// `flex-wrap: wrap-reverse` (die Querachse laeuft umgekehrt).
    pub umbruch_umgekehrt: bool,
    /// `flex-wrap: balance` (CSS Flexbox 2): die Zeilen gleichmaessig fuellen.
    pub ausgleich: bool,
    /// `flex-line-count` (CSS Flexbox 2): die Mindestzahl der Zeilen beim
    /// Ausgleichen.
    pub zeilen_min: u32,
    /// `justify-content`.
    pub justify_content: Ausr,
    /// `align-content`.
    pub align_content: Ausr,
    /// `align-items`.
    pub align_items: Ausr,
    /// `column-gap` (`None` = `normal` = 0).
    pub luecke_spalte: Option<LaengeProzent>,
    /// `row-gap`.
    pub luecke_zeile: Option<LaengeProzent>,
    /// `direction: rtl` — eine Reihe laeuft von rechts nach links.
    pub rtl: bool,
}
