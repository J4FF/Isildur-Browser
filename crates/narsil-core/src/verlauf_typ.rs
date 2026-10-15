//! BILD_44, Station 1: der **neutrale** Verlaufstyp.
//!
//! **WARUM EINE EIGENE DATEI und nicht `style.rs`:** `narsil-raster` darf **nicht**
//! an `narsil-css` hängen (das steht im Kommentar von
//! `crates/narsil-raster/Cargo.toml` — es gibt **keinen** Zyklus). Ein
//! Rasterizer braucht aber **Werte**. Also liegen sie hier, in der Crate, die
//! **alle** teilen, und `narsil-page/src/kaskade.rs` bildet `narsil_css::Verlauf`
//! darauf ab — **dieselbe Station wie `RahmenStil` in BILD_35**.
//!
//! **DIESE DATEI ENTHAELT NUR DATEN.** Keine Rechnung, keine Rasterung, kein
//! Eingriff in bestehende Typen — die Merkmale, die die Leitung für Schritt 1
//! genannt hat, sind hier vollständig umgesetzt und nichts sonst.
//!
//! **UND WAS DIE ZWEI GEMESSENEN FESTLEGEN, die dieser Typ tragen muss:**
//!
//! 1. **Die Position ist ein Anteil**, `0.0…=1.0`, und `None` bedeutet
//!    „zwischen den Nachbarn gleichmäßig". **GEMESSEN** (Firefox 157,
//!    `team/testseiten/verlauf_lin_ff.png`): der Verlauf läuft über die
//!    **Pixelmitten**, `t = (x + 0,5)/breite` — bei `x = 50` einer 100-px-Fläche
//!    steht **(126, 0, 129)** statt (128, 0, 128). Wer die Position in Pixeln
//!    statt als Anteil speichert, kann das gar nicht ausdrücken.
//! 2. **Die Farbe ist RGBA mit Alpha**, weil Firefox **premultipliziert**
//!    interpoliert. **GEMESSEN:** `#ff000000 → #0000ffff` über `#2040c0`
//!    ergibt bei `x = 50` **(16, 32, 224)**; premultipliziert gerechnet
//!    `(0, 0, 255t) + (1−t)·(32, 64, 192)` = (15,8 / 31,7 / 223,8) = **exakt**.
//!    **Ohne** Multiplikation käme **(79, 32, 160)** — **63** im Rot-Kanal. Wer
//!    das Alpha nicht mitführt, kann den Unterschied nicht machen.

use crate::color::Color;

/// Die **Art** des Verlaufs. CSS Images 3 §3; die drei `repeating-`-Formen
/// sind **keine** eigene Art, sondern [`Verlauf::wiederholt`] = `true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerlaufArt {
    /// `linear-gradient` — entlang einer geraden Linie.
    Linear,
    /// `radial-gradient` — vom Mittelpunkt nach außen.
    Radial,
    /// `conic-gradient` — um einen Punkt herum (CSS Images 4, §12).
    Kegel,
}

/// **Ein** Farbstopp: eine Farbe und ihre **optionale** Position.
///
/// `anteil` ist `None` bei `red` **ohne** Angabe (CSS Images 3 §3.5: die
/// Position wird dann zwischen den Nachbarn **gleichmäßig** verteilt) und
/// `Some(0.0…=1.0)` bei `red 40%`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stopp {
    /// Die Farbe an dieser Stelle — **mit Alpha**, weil Firefox
    /// premultipliziert interpoliert (siehe Kopf dieser Datei, Befund 2).
    pub farbe: Color,
    /// Position entlang der Verlaufslinie als **Anteil**, `None` = automatisch.
    pub anteil: Option<f32>,
}

/// **BILD_51: die Form des Radialverlaufs** — CSS Images 3 §3.3.
///
/// **WARUM EIN EIGENER Typ und keine Zahl:** Kreis und Ellipse sind **nicht**
/// dasselbe mit einem anderen Wert — sie haben **verschiedene Formeln** (eine
/// Ellipse braucht **zwei** Halbachsen). Wer sie in eine Zahl presst, muss sie
/// später auseinanderrechnen, und an welcher Stelle das geschieht, ist dann
/// nicht mehr ablesbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialForm {
    /// `circle` — **ein** Radius. GEMESSEN: `circle farthest-corner` auf
    /// 100 × 50, Mitte, gibt bei Pixel (60, 35) den Wert **(252, 252, 252)**;
    /// mit `√(50² + 25²) = 55,90` Radius und Pixelmitte **(60,5; 35,5)** folgt
    /// genau das: `255 · (1 − 0,707/55,90) = 251,8`.
    Kreis,
    /// `ellipse` — **zwei** Halbachsen. **CSS-Default**, wenn keine Form
    /// genannt wird.
    Ellipse,
}

/// **BILD_51: die Endform** — CSS Images 3 §3.3. Das ist **die** Angabe, die
/// entscheidet, **wie weit** der Verlauf reicht.
///
/// **UND HIER STEHT DIE GEMESSENE FORMEL, weil sie der Punkt ist, an dem
/// `farthest-corner` am leichtesten falsch gelesen wird:**
/// * **Kreis:** `r` = **der** Abstand zum Punkt (Seite oder Ecke) — bei
///   `closest-side` der kleinste, bei `farthest-corner` der grösste.
/// * **Ellipse:** `rx = dx · √2`, `ry = dy · √2`, wobei `dx`/`dy` der
///   waagerechte/senkrechte Abstand zur **Ecke** sind. **GEMESSEN** an
///   `ellipse farthest-corner` (100 × 50, Mitte), Pixel (60, 10) → **(78)**:
///   `√((0,5/70,71)² + (24,5/35,36)²) = 0,6928`, `255 · 0,3072 = 78,3` = **78**.
///   Mit dem **falschen** `dy = 25` statt der **Pixelmitte** 24,5 käme 75 —
///   **3 Einheiten** daneben, und `√2` waere dann nicht zu widerlegen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialGroesse {
    /// `closest-side` — die **nächste** Seite. GEMESSEN: Mitte
    /// (100 × 50, Mitte) → Radius **25**, Pixel (60, 70) oben → **(5)**.
    NaechsteSeite,
    /// `closest-corner` — die **nächste** Ecke.
    NaechsteEcke,
    /// `farthest-side` — die **fernste** Seite.
    FernsteSeite,
    /// `farthest-corner` — die **fernste** Ecke. **CSS-Default**, wenn keine
    /// Endform genannt wird.
    FernsteEcke,
}

/// **BILD_51: was ein Radialverlauf **über** den Stopps noch braucht.**
///
/// **DER GRUND FÜR EINE EIGENE STRUKTUR:** [`Verlauf`] hatte nur `winkel`,
/// `stopps`, `wiederholt` — **keine** Form, **keine** Endform, **keinen**
/// Mittelpunkt. Für `linear-gradient` reicht der Winkel; für `radial-gradient`
/// reicht er **nicht**, und die Mitte ist **nicht** die Kastenmitte, wenn
/// `at 30% 40%` dasteht. **GEMESSEN** (`circle farthest-corner at 30% 40%`,
/// 100 × 50, Mitte (60, 274) → **(185)**, oben (60, 250) → **(160)**): die Mitte
/// liegt bei **(40, 30)** des Kastens — `0,30 · 100` und `0,40 · 50`. Wer die
/// Mitte immer in die Kastenmitte legt, ist bei dieser Zeile um **36** daneben.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radial {
    /// Kreis oder Ellipse.
    pub form: RadialForm,
    /// Die Endform.
    pub groesse: RadialGroesse,
    /// **Waagerechte** Mitte als **Anteil** der Kastenbreite, `0.0…=1.0`.
    /// CSS-Default ist `0.5`.
    pub anteil_x: f32,
    /// **Senkrechte** Mitte als **Anteil** der Kastenhoehe, `0.0…=1.0`.
    /// CSS-Default ist `0.5`.
    pub anteil_y: f32,
}

impl Default for Radial {
    /// **DIE CSS-ANFANGSWERTE**, Images 3 §3.3: Form **Ellipse**, Endform
    /// **`farthest-corner`**, Mitte **(0,5; 0,5)**. Ohne diese drei Angeben ist
    /// `radial-gradient(#fff, #000)` **nicht** dasselbe wie
    /// `radial-gradient(ellipse farthest-corner at 50% 50%, #fff, #000)` —
    /// das ist der Grund, warum sie **hier** und nicht im Aufrufer stehen.
    fn default() -> Self {
        Radial {
            form: RadialForm::Ellipse,
            groesse: RadialGroesse::FernsteEcke,
            anteil_x: 0.5,
            anteil_y: 0.5,
        }
    }
}

/// Release 13.10.2026 (Agent CSS2): Groesse EINER Achse der Kachel eines
/// Verlaufs (`background-size`, CSS Backgrounds 3 §3.9).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum KachelMass {
    /// `auto` — ein Verlauf hat keine eigene Groesse, also die der
    /// Lageflaeche (§3.9: kein natuerliches Mass, kein Verhaeltnis).
    #[default]
    Auto,
    /// Feste Laenge in px.
    Px(f32),
    /// Anteil der Lageflaeche (1.0 = 100 %).
    Anteil(f32),
}

/// Release 13.10.2026 (Agent CSS2): Groesse, Lage und Wiederholung eines
/// Verlaufs als Hintergrundbild (CSS Backgrounds 3 §3.5, §3.6, §3.9).
/// GEMESSEN vorher (WPT css-text `white-space/pre-wrap-006` u. a., 31 rote
/// css-text-Dateien mit `linear-gradient(red, red) 0 0/2ch 2ch no-repeat`):
/// der Verlauf fuellte immer den ganzen Kasten.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kachel {
    /// `background-size` je Achse (x, y).
    pub groesse: [KachelMass; 2],
    /// `background-position` je Achse: (Wert, `true` = px, sonst Anteil 0..1).
    pub lage: [(f32, bool); 2],
    /// `background-repeat` je Achse.
    pub wiederholen: [bool; 2],
    /// Release 13.10.2026 (Agent CSS2): `space` (1) / `round` (2) je Achse.
    pub art: [u8; 2],
    /// Release 13.10.2026 (Agent CSS2): `background-origin` — 0 `padding-box`,
    /// 1 `border-box`, 2 `content-box`.
    pub ursprung: u8,
    /// Release 13.10.2026 (Agent CSS2): die px-Lage zaehlt von der Endkante.
    pub vom_ende: [bool; 2],
}

/// Ein Farbverlauf, **neutral** — ohne `narsil_css` im Weg.
#[derive(Debug, Clone, PartialEq)]
pub struct Verlauf {
    /// Linear, radial oder Kegel.
    pub art: VerlaufArt,
    /// **Richtungswinkel in Grad** — CSS Images 3, §3.1: `0deg` zeigt nach
    /// **oben**, positive Werte im Uhrzeigersinn, `linear-gradient` **ohne**
    /// Angabe ist `180deg` (nach unten).
    ///
    /// **WARUM WINKEL und nicht `to`-Richtung:** `to rechts` und `90deg` sind
    /// dieselbe Angabe, und der **Parser** rechnet sie bereits um (in
    /// `narsil_css::Verlauf` steht „ohne Richtungsangabe geht der Verlauf von
    /// oben nach unten"). Ein zweiter, eigener Ausdruck für dasselbe wäre eine
    /// zweite Wahrheit — und die Liemen des `narsil_css`-Typs sind die **eine**
    /// Stelle, an der `to`-Richtungen falsch gelesen werden können.
    pub winkel: f32,
    /// Die Farbstopps in Schreibrichtung, **mindestens zwei**.
    pub stopps: Vec<Stopp>,
    /// `repeating-linear-gradient` und die anderen `repeating-`-Formen.
    pub wiederholt: bool,
    /// **BILD_51: Form, Endform und Mitte eines `radial-gradient`.**
    ///
    /// **`None` bei Linear und Kegel.** GEMESSEN, warum das ein `Option` und
    /// kein Default-Feld ist: bei einem linearen Verlauf waere eine Form
    /// **bedeutungslos**, aber **da** — und ein Feld, das nur bei einer von
    /// drei Arten etwas bedeutet, ist genau die Art Feld, das spaeter
    /// versehentlich auch an der falschen Stelle gelesen wird. `None` sagt
    /// **keine Angabe**, das ist der Anfangswert von `background-image` auf
    /// einer Flaeche, die gar keinen Radialverlauf hat.
    pub radial: Option<Radial>,
}

impl Verlauf {
    /// Der **kürzeste** lineare Verlauf, den es gibt: zwei Stopps, ganz links
    /// und ganz rechts, **ohne** Wiederholung.
    ///
    /// **DER WEG ZUM ERSTEN GEMALTEN VERLAUF** — und der ist Absicht: eine
    /// Messung mit **zwei** Stopps lässt sich **analytisch** nachrechnen,
    /// jede Fehlentscheidung ist damit **benennbar**. Alles Weitere (mehrere
    /// Stopps, Hartkanten, `to`-Richtungen, `repeating-*`, radial, Kegel)
    /// kommt danach und **je einzeln**.
    pub fn linear_zwei(links: Color, rechts: Color) -> Verlauf {
        Verlauf {
            art: VerlaufArt::Linear,
            // 90deg = nach rechts (CSS Images 3 §3.1: 0 ist oben, im
            // Uhrzeigersinn — 90 ist also rechts).
            winkel: 90.0,
            // Linear hat **keine** Form, keine Endform und keine Mitte.
            radial: None,
            stopps: vec![
                Stopp {
                    farbe: links,
                    anteil: Some(0.0),
                },
                Stopp {
                    farbe: rechts,
                    anteil: Some(1.0),
                },
            ],
            wiederholt: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Verlauf traegt **zwei** Stopps mit **Position** und **RGBA** — das ist
    /// die Merkmalsliste der Leitung fuer Schritt 1, als Test.
    #[test]
    fn linear_zwei_traegt_position_und_rgba() {
        let v = Verlauf::linear_zwei(Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        assert_eq!(v.art, VerlaufArt::Linear);
        assert_eq!(v.winkel, 90.0, "90deg ist nach rechts");
        assert_eq!(v.stopps.len(), 2);
        assert_eq!(v.stopps[0].anteil, Some(0.0));
        assert_eq!(v.stopps[1].anteil, Some(1.0));
        assert_eq!(v.stopps[0].farbe.r, 255);
        assert_eq!(v.stopps[1].farbe.b, 255);
        assert!(!v.wiederholt);
    }

    /// **Und der Alpha-Kanal ist wirklich da** — ohne ihn ist der
    /// premultiplizierte Fall nicht ausdrueckbar.
    #[test]
    fn der_alpha_kanon_steht_in_der_farbe() {
        let durchsichtig = Color {
            r: 255,
            g: 0,
            b: 0,
            a: 0,
        };
        let v = Verlauf::linear_zwei(durchsichtig, Color::rgb(0, 0, 255));
        assert_eq!(v.stopps[0].farbe.a, 0, "Alpha muss durchkommen");
    }
}
