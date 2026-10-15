//! Box-Modell als reine Funktionen (C7, W5).
//!
//! CSS 2.1, Kapitel 10 (Block-Boxen, nicht ersetzt) und 8.3.1
//! (vertikaler Margin-Kollaps), vereinfacht auf das, was der Browser
//! braucht. Reine Funktionen: keine Allokation, kein Panic —
//! nicht-endliche Eingaben (NaN/inf) werden zu 0, Uebermaesse an
//! MAX_PX gekappt (M4).

use super::MAX_PX;

/// Saeubert eine Eingabelaenge: nicht-endlich -> 0, Uebermass -> +-MAX_PX.
/// Begruendung: Layout rechnet mit px aus der Kaskade; inf/NaN duerfen
/// nie eine Position vergiften (kein Panic, kein Verschwinden).
fn sauber(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    x.clamp(-MAX_PX, MAX_PX)
}

/// Prozent von einer Basis (z.B. Breite vom Container). Reine
/// Multiplikation mit denselben Schranken wie `sauber`.
pub fn anteil(prozent: f32, basis: f32) -> f32 {
    sauber(prozent) / 100.0 * sauber(basis).clamp(0.0, MAX_PX)
}

// ===========================================================================
// MARGIN-KOLLAPS (CSS 2.1, 8.3.1, vereinfacht)
// ===========================================================================

/// Kollabierter Vertikal-Abstand zweier benachbarter Raender (etwa
/// margin-bottom des oberen und margin-top des unteren Geschwisters).
/// Regel: beide >= 0 -> Maximum; beide <= 0 -> Minimum (staerkster
/// negativer); gemischt -> Summe. Nicht-endlich -> 0.
pub fn kollaps(obere: f32, untere: f32) -> f32 {
    let a = sauber(obere);
    let b = sauber(untere);
    if a >= 0.0 && b >= 0.0 {
        a.max(b)
    } else if a <= 0.0 && b <= 0.0 {
        a.min(b)
    } else {
        sauber(a + b)
    }
}

// ===========================================================================
// BLOCK-BREITE (CSS 2.1, 10.3.3, Block, nicht ersetzt, LTR)
// ===========================================================================

/// Vorgabe fuer [`lege_block_breite`]: alles in px ausser den zwei
/// Auto-Schaltern. Negative Raender sind erlaubt (CSS), negative
/// Fuellungen/Rahmen/Breiten werden auf 0 gekappt.
pub struct BlockVorgabe {
    /// width (None = auto). Negativ -> 0.
    pub breite: Option<f32>,
    /// max-width (None = keine). Negativ -> 0.
    pub max_breite: Option<f32>,
    /// min-width (None = keine). Negativ -> 0.
    pub min_breite: Option<f32>,
    /// margin-left ist auto (zentrieren/fuellen).
    pub auto_links: bool,
    /// margin-right ist auto.
    pub auto_rechts: bool,
    /// margin-left in px (wenn nicht auto).
    pub marge_links: f32,
    /// margin-right in px (wenn nicht auto).
    pub marge_rechts: f32,
    /// padding-left + border-left in px (fest, >= 0).
    pub fest_links: f32,
    /// padding-right + border-right in px (fest, >= 0).
    pub fest_rechts: f32,
}

/// Ergebnis: genutzte Inhaltsbreite und genutzte Raender in px.
pub struct BlockErgebnis {
    /// Genutzte Inhaltsbreite (>= 0).
    pub inhalt: f32,
    /// Genutzter linker Rand (kann negativ sein: Ueberlauf links).
    pub marge_links: f32,
    /// Genutzter rechter Rand.
    pub marge_rechts: f32,
}

/// Legt eine Block-Box in `container` (Inhaltsbreite des Containers).
/// Ablauf nach 10.3.3: erst Knebel aus Breite/Auto, dann max-width
/// (10.4), dann min-width (10.7 — min schlaegt max). Ueberbestimmt
/// (alles fest, zu breit): LTR-Regel, margin-left faengt den Rest auf.
/// Alles O(1), keine Allokation.
pub fn lege_block_breite(container: f32, v: &BlockVorgabe) -> BlockErgebnis {
    let c = sauber(container).clamp(0.0, MAX_PX);
    let fest = sauber(v.fest_links).clamp(0.0, MAX_PX) + sauber(v.fest_rechts).clamp(0.0, MAX_PX);
    let ml_spez = sauber(v.marge_links);
    let mr_spez = sauber(v.marge_rechts);
    let max = v.max_breite.map(|x| sauber(x).clamp(0.0, MAX_PX));
    let min = v.min_breite.map(|x| sauber(x).clamp(0.0, MAX_PX));

    // Schritt 1: Breite + Raender nach Auto-Lage (10.3.3).
    let (mut w, mut ml, mut mr) = match (v.breite, v.auto_links, v.auto_rechts) {
        // Breite auto: Raender wie angegeben (auto -> 0), Rest fuellt.
        (None, _, _) => {
            let l = if v.auto_links { 0.0 } else { ml_spez };
            let r = if v.auto_rechts { 0.0 } else { mr_spez };
            ((c - fest - l - r).clamp(0.0, MAX_PX), l, r)
        }
        // Beide auto: zentrieren (Rest halbe-halbe; negativ -> 0/0).
        (Some(b), true, true) => {
            let w = sauber(b).clamp(0.0, MAX_PX);
            let rest = c - fest - w;
            if rest >= 0.0 {
                (w, rest / 2.0, rest / 2.0)
            } else {
                (w, 0.0, 0.0)
            }
        }
        // Ein auto: nimmt den Rest (darf negativ sein — CSS-Regel).
        (Some(b), true, false) => {
            let w = sauber(b).clamp(0.0, MAX_PX);
            (w, sauber(c - fest - w - mr_spez), mr_spez)
        }
        (Some(b), false, true) => {
            let w = sauber(b).clamp(0.0, MAX_PX);
            (w, ml_spez, sauber(c - fest - w - ml_spez))
        }
        // Alles fest: Ueberlauf faengt links auf (LTR, 10.3.3).
        (Some(b), false, false) => {
            let w = sauber(b).clamp(0.0, MAX_PX);
            let rest = c - fest - w - ml_spez - mr_spez;
            (w, sauber(ml_spez + rest.min(0.0)), mr_spez)
        }
    };

    // Schritt 2: max-width (10.4), dann min-width (10.7, schlaegt max).
    // Nach dem Knebel werden Auto-Raender wie feste 0 behandelt und der
    // Rest neu verteilt (ein Durchgang genuegt — kein Fixpunkt noetig).
    if let Some(m) = max {
        if w > m {
            w = m;
            let r = knebel_raender(c, fest, w, v.auto_links, v.auto_rechts, ml_spez, mr_spez);
            ml = r.0;
            mr = r.1;
        }
    }
    if let Some(m) = min {
        if w < m {
            w = m;
            let r = knebel_raender(c, fest, w, v.auto_links, v.auto_rechts, ml_spez, mr_spez);
            ml = r.0;
            mr = r.1;
        }
    }
    BlockErgebnis {
        inhalt: w,
        marge_links: sauber(ml),
        marge_rechts: sauber(mr),
    }
}

/// Verteilt den Rest bei fester Breite (fuer max-/min-Knebel).
fn knebel_raender(
    c: f32,
    fest: f32,
    w: f32,
    auto_links: bool,
    auto_rechts: bool,
    ml_spez: f32,
    mr_spez: f32,
) -> (f32, f32) {
    let rest = c - fest - w;
    match (auto_links, auto_rechts) {
        (true, true) => {
            if rest >= 0.0 {
                (rest / 2.0, rest / 2.0)
            } else {
                (0.0, 0.0)
            }
        }
        (true, false) => (sauber(rest - mr_spez), mr_spez),
        (false, true) => (ml_spez, sauber(rest - ml_spez)),
        (false, false) => (sauber(ml_spez + rest.min(0.0)), mr_spez),
    }
}

// ===========================================================================
// BLOCK-HOEHE (CSS 2.1, 10.6.3/10.7, vereinfacht)
// ===========================================================================

/// Block-Hoehe: auto -> natuerliche Inhaltshoehe, sonst Knebel mit
/// min/max (min schlaegt max). Immer >= 0.
pub fn lege_block_hoehe(
    hoehe: Option<f32>,
    natuerlich: f32,
    min: Option<f32>,
    max: Option<f32>,
) -> f32 {
    let mut h = match hoehe {
        None => sauber(natuerlich).clamp(0.0, MAX_PX),
        Some(v) => sauber(v).clamp(0.0, MAX_PX),
    };
    if let Some(m) = max {
        h = h.min(sauber(m).clamp(0.0, MAX_PX));
    }
    if let Some(m) = min {
        h = h.max(sauber(m).clamp(0.0, MAX_PX));
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kollaps_positiv() {
        assert_eq!(kollaps(10.0, 20.0), 20.0);
    }
    #[test]
    fn kollaps_negativ() {
        assert_eq!(kollaps(-10.0, -20.0), -20.0);
    }
    #[test]
    fn kollaps_gemischt() {
        assert_eq!(kollaps(10.0, -4.0), 6.0);
    }
    #[test]
    fn kollaps_null() {
        assert_eq!(kollaps(0.0, 0.0), 0.0);
    }
    #[test]
    fn kollaps_nicht_endlich() {
        assert_eq!(kollaps(f32::NAN, 5.0), 5.0);
        assert_eq!(kollaps(f32::INFINITY, 5.0), 5.0);
    }
    #[test]
    fn kollaps_riesig() {
        assert_eq!(kollaps(1e30, 1.0), MAX_PX);
    }

    fn vorgabe() -> BlockVorgabe {
        BlockVorgabe {
            breite: None,
            max_breite: None,
            min_breite: None,
            auto_links: false,
            auto_rechts: false,
            marge_links: 0.0,
            marge_rechts: 0.0,
            fest_links: 0.0,
            fest_rechts: 0.0,
        }
    }

    #[test]
    fn auto_fuellt() {
        let e = lege_block_breite(800.0, &vorgabe());
        assert_eq!((e.inhalt, e.marge_links, e.marge_rechts), (800.0, 0.0, 0.0));
    }
    #[test]
    fn auto_mit_fest() {
        let mut v = vorgabe();
        v.fest_links = 10.0;
        v.fest_rechts = 10.0;
        v.marge_links = 5.0;
        let e = lege_block_breite(800.0, &v);
        assert_eq!(e.inhalt, 775.0);
    }
    #[test]
    fn zentrieren() {
        let mut v = vorgabe();
        v.breite = Some(500.0);
        v.auto_links = true;
        v.auto_rechts = true;
        let e = lege_block_breite(800.0, &v);
        assert_eq!(
            (e.inhalt, e.marge_links, e.marge_rechts),
            (500.0, 150.0, 150.0)
        );
    }
    #[test]
    fn ein_auto_links() {
        let mut v = vorgabe();
        v.breite = Some(500.0);
        v.auto_links = true;
        v.marge_rechts = 50.0;
        let e = lege_block_breite(800.0, &v);
        assert_eq!(
            (e.inhalt, e.marge_links, e.marge_rechts),
            (500.0, 250.0, 50.0)
        );
    }
    #[test]
    fn ein_auto_rechts() {
        let mut v = vorgabe();
        v.breite = Some(500.0);
        v.auto_rechts = true;
        v.marge_links = 50.0;
        let e = lege_block_breite(800.0, &v);
        assert_eq!(
            (e.inhalt, e.marge_links, e.marge_rechts),
            (500.0, 50.0, 250.0)
        );
    }
    #[test]
    fn ueberbestimmt() {
        // 900 + 0 + 0 in 800: LTR faengt links auf (ml = -100).
        let mut v = vorgabe();
        v.breite = Some(900.0);
        let e = lege_block_breite(800.0, &v);
        assert_eq!(
            (e.inhalt, e.marge_links, e.marge_rechts),
            (900.0, -100.0, 0.0)
        );
    }
    #[test]
    fn max_knebelt() {
        let mut v = vorgabe();
        v.breite = Some(900.0);
        v.max_breite = Some(400.0);
        v.auto_links = true;
        v.auto_rechts = true;
        let e = lege_block_breite(800.0, &v);
        assert_eq!(
            (e.inhalt, e.marge_links, e.marge_rechts),
            (400.0, 200.0, 200.0)
        );
    }
    #[test]
    fn min_schlaegt_max() {
        // min > max: min gewinnt (10.7).
        let mut v = vorgabe();
        v.breite = Some(100.0);
        v.max_breite = Some(200.0);
        v.min_breite = Some(300.0);
        let e = lege_block_breite(800.0, &v);
        assert_eq!(e.inhalt, 300.0);
    }
    #[test]
    fn negativ_breite_null() {
        let mut v = vorgabe();
        v.breite = Some(-50.0);
        let e = lege_block_breite(800.0, &v);
        assert_eq!(e.inhalt, 0.0);
    }
    #[test]
    fn container_null_oder_nan() {
        let e = lege_block_breite(0.0, &vorgabe());
        assert_eq!(e.inhalt, 0.0);
        let e = lege_block_breite(f32::NAN, &vorgabe());
        assert_eq!(e.inhalt, 0.0);
    }
    #[test]
    fn anteil_prozent() {
        assert_eq!(anteil(50.0, 800.0), 400.0);
        assert_eq!(anteil(f32::NAN, 800.0), 0.0);
    }
    #[test]
    fn hoehe_auto_und_knebel() {
        assert_eq!(lege_block_hoehe(None, 120.0, None, None), 120.0);
        assert_eq!(
            lege_block_hoehe(Some(500.0), 120.0, None, Some(400.0)),
            400.0
        );
        assert_eq!(lege_block_hoehe(Some(-5.0), 120.0, None, None), 0.0);
        // min schlaegt max.
        assert_eq!(
            lege_block_hoehe(Some(100.0), 0.0, Some(300.0), Some(200.0)),
            300.0
        );
    }
}

#[cfg(test)]
mod tests_extra {
    use super::*;

    fn v() -> BlockVorgabe {
        BlockVorgabe {
            breite: None,
            max_breite: None,
            min_breite: None,
            auto_links: false,
            auto_rechts: false,
            marge_links: 0.0,
            marge_rechts: 0.0,
            fest_links: 0.0,
            fest_rechts: 0.0,
        }
    }

    #[test]
    fn zentrieren_uebervoll() {
        // Zu breit zum Zentrieren: Raender 0/0, Inhalt bleibt.
        let mut x = v();
        x.breite = Some(900.0);
        x.auto_links = true;
        x.auto_rechts = true;
        let e = lege_block_breite(800.0, &x);
        assert_eq!((e.inhalt, e.marge_links, e.marge_rechts), (900.0, 0.0, 0.0));
    }
    #[test]
    fn auto_links_negativ() {
        // Ein Auto-Rand darf negativ werden (CSS-Regel).
        let mut x = v();
        x.breite = Some(900.0);
        x.auto_links = true;
        let e = lege_block_breite(800.0, &x);
        assert_eq!(
            (e.inhalt, e.marge_links, e.marge_rechts),
            (900.0, -100.0, 0.0)
        );
    }
    #[test]
    fn min_ohne_max() {
        let mut x = v();
        x.breite = Some(100.0);
        x.min_breite = Some(300.0);
        let e = lege_block_breite(800.0, &x);
        assert_eq!(e.inhalt, 300.0);
    }
    #[test]
    fn max_auto_einseitig() {
        let mut x = v();
        x.breite = Some(900.0);
        x.max_breite = Some(400.0);
        x.auto_links = true;
        x.marge_rechts = 50.0;
        let e = lege_block_breite(800.0, &x);
        assert_eq!(
            (e.inhalt, e.marge_links, e.marge_rechts),
            (400.0, 350.0, 50.0)
        );
    }
    #[test]
    fn anteil_sonderfaelle() {
        assert_eq!(anteil(-50.0, 800.0), -400.0);
        assert_eq!(anteil(50.0, 1e30), 50_000.0);
    }
    #[test]
    fn hoehe_min_hebt() {
        assert_eq!(lege_block_hoehe(None, 50.0, Some(100.0), None), 100.0);
        assert_eq!(lege_block_hoehe(None, -50.0, None, None), 0.0);
    }
}
