//! Weiche: die Rechnung liegt in `box.rs`, der alte Modulname bleibt.
//!
//! B11 (06.10.2026, 01:25): `box_sizing.rs` ist nach `box.rs` gewandert
//! (fremde Arbeit in dieser Crate). `flow.rs` und `vorschau.rs` benutzen
//! den Pfad `crate::box_sizing::…` weiter, und diese Datei haelt ihn am
//! Leben — **ohne** eine zweite Kopie der Rechnung.
//!
//! GEMESSENE REGEL, die hier gilt: eine Weiche, die den Namen weiterleitet,
//! ist additiv. Eine Weiche, die den Inhalt kopiert, ist eine zweite
//! Wahrheit. Deshalb stehen hier drei `use`-Zeilen und sonst nichts.

pub(crate) use super::r#box::{Sizes, get_node, resolve_spacing};
// `compute_sizes` und `validate_style` werden von KEINEM `use`
// gebraucht (GEMESSEN 05:59: `flow.rs` und `vorschau.rs` nehmen nur die
// drei darueber). **Sie stehen hier trotzdem, und `clippy` sagt es:**
// `plan.rs` 5 und `lib.rs` 34 verweisen mit
// `[`crate::box_sizing::compute_sizes`]` darauf, und ein Doku-Link ohne
// Ziel ist ein Warn-Eintrag. Die Alternative waere, die Links umzuschreiben
// — dann waere der alte Name aus der Doku verschwunden, obwohl er in
// `flow.rs` weiter lebt. **Ein toter Re-Export mit Grund ist besser als
// ein lebender Name, den niemand mehr findet.**
#[allow(unused_imports)]
pub(crate) use super::r#box::{compute_sizes, validate_style};
