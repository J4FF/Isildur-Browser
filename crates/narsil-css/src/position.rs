//! Positionierung als reine Funktionen (C13/C14, W6).
//!
//! CSS 2.1, Kapitel 9 (relative/absolute/float/clear) und 10.3.7/10.6.4
//! (absolut positionierte Block-Boxen, nicht ersetzt), vereinfacht auf
//! dokumentierte Teilmengen. Reine Funktionen: keine Allokation (ausser
//! der Mal-Reihenfolge in Eingabelaenge), kein Panic — nicht-endliche
//! Eingaben werden zu 0, Uebermaesse an MAX_PX gekappt (M4).

use super::typen::{Float, Raeumung, Ueberlauf};
use super::{MAX_AHNEN, MAX_PX};

/// Saeubert eine Eingabelaenge (endlich, +-MAX_PX). Begruendung: Layout
/// rechnet mit px aus der Kaskade; inf/NaN duerfen nie eine Position
/// vergiften.
fn sauber(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    x.clamp(-MAX_PX, MAX_PX)
}

/// Achsenparalleles Rechteck in px (Position + Groesse, >= 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rechteck {
    /// Linke Kante.
    pub x: f32,
    /// Obere Kante.
    pub y: f32,
    /// Breite (>= 0).
    pub breite: f32,
    /// Hoehe (>= 0).
    pub hoehe: f32,
}

impl Rechteck {
    /// Baut ein Rechteck (negative Massen werden 0, CSS-Regel).
    pub fn neu(x: f32, y: f32, breite: f32, hoehe: f32) -> Rechteck {
        Rechteck {
            x: sauber(x),
            y: sauber(y),
            breite: sauber(breite).clamp(0.0, MAX_PX),
            hoehe: sauber(hoehe).clamp(0.0, MAX_PX),
        }
    }
    /// Schnittmenge (fuer Beschnitt, C14). Leer -> Breite/Hoehe 0.
    pub fn schnitt(&self, anderes: &Rechteck) -> Rechteck {
        let x1 = self.x.max(anderes.x);
        let y1 = self.y.max(anderes.y);
        let x2 = (self.x + self.breite).min(anderes.x + anderes.breite);
        let y2 = (self.y + self.hoehe).min(anderes.y + anderes.hoehe);
        Rechteck::neu(x1, y1, (x2 - x1).max(0.0), (y2 - y1).max(0.0))
    }
}

// ===========================================================================
// RELATIV (CSS 2.1, 9.4.3)
// ===========================================================================

/// Versatz einer relativ positionierten Box (dx, dy): top gewinnt gegen
/// bottom, left gegen rechts (CSS-Regel); auto-Seiten zaehlen 0.
/// Sticky (klebend) nutzt dieselbe Funktion (dokumentiert, C13).
pub fn relativ_versatz(
    oben: Option<f32>,
    rechts: Option<f32>,
    unten: Option<f32>,
    links: Option<f32>,
) -> (f32, f32) {
    let dy = match oben {
        Some(v) => sauber(v),
        None => -unten.map(sauber).unwrap_or(0.0),
    };
    let dx = match links {
        Some(v) => sauber(v),
        None => -rechts.map(sauber).unwrap_or(0.0),
    };
    (dx, dy)
}

// ===========================================================================
// ABSOLUT (CSS 2.1, 10.3.7/10.6.4, vereinfacht)
// ===========================================================================

/// Legt die horizontale Lage einer absolut positionierten Box:
/// (x, genutzte Breite). `behaelter` = Inhaltsbreite des enthaltenden
/// Blocks, `statisch` = statische Position (wo sie im Fluss stuende),
/// `inhalt` = Schrumpfbreite (Inhalt) fuer auto-Breiten.
/// Regeln (10.3.7, LTR, ohne Raender — die addiert der Aufrufer):
/// Breite auto + beide Seiten gesetzt -> Dehnung; Breite auto sonst ->
/// Inhalt; links auto -> rechts bzw. statisch; Ueberlauf bleibt stehen
/// (Aufrufer beschneidet ggf.). Alles O(1).
pub fn absolut_waagerecht(
    behaelter: f32,
    links: Option<f32>,
    breite: Option<f32>,
    rechts: Option<f32>,
    statisch: f32,
    inhalt: f32,
) -> (f32, f32) {
    let c = sauber(behaelter).clamp(0.0, MAX_PX);
    let s = sauber(statisch);
    let ib = sauber(inhalt).clamp(0.0, MAX_PX);
    let l = links.map(sauber);
    let r = rechts.map(sauber);
    // Breite bestimmen.
    let w = match (breite.map(sauber), l, r) {
        (Some(b), _, _) => b.clamp(0.0, MAX_PX),
        (None, Some(lv), Some(rv)) => (c - lv - rv).clamp(0.0, MAX_PX),
        (None, _, _) => ib,
    };
    // Lage bestimmen.
    let x = match (l, r) {
        (Some(lv), _) => lv,
        (None, Some(rv)) => c - rv - w,
        (None, None) => s,
    };
    (sauber(x), w)
}

/// Senkrechte Lage (10.6.4, analog waagerecht; `statisch`+`inhalt` wie oben).
pub fn absolut_senkrecht(
    behaelter: f32,
    oben: Option<f32>,
    hoehe: Option<f32>,
    unten: Option<f32>,
    statisch: f32,
    inhalt: f32,
) -> (f32, f32) {
    absolut_waagerecht(behaelter, oben, hoehe, unten, statisch, inhalt)
}

// ===========================================================================
// ENTHALTENDER BLOCK (CSS 2.1, 10.1, vereinfacht)
// ===========================================================================

/// Ein Vorfahr in der Kette zum enthaltenden Block.
pub struct Vorfahr {
    /// True bei position relativ/absolut/fixed/sticky (nicht statisch).
    pub positioniert: bool,
    /// Sein Innenkasten (padding box) in px.
    pub kasten: Rechteck,
}

/// Enthaeltender Block fuer absolut positionierte Boxen: der naechste
/// positionierte Vorfahr (von innen nach aussen gegeben), sonst der
/// `anfang` (Anfangsblock/Ansicht). Kette gedeckelt durch MAX_AHNEN (M3).
pub fn enthaltender_block(anfang: Rechteck, vorfahren: &[Vorfahr]) -> Rechteck {
    // Von innen (Ende) nach aussen, hoechstens MAX_AHNEN Schritte.
    let n = vorfahren.len().min(MAX_AHNEN);
    for i in 0..n {
        let v = &vorfahren[vorfahren.len() - 1 - i];
        if v.positioniert {
            return v.kasten;
        }
    }
    anfang
}

// ===========================================================================
// STAPELREIHENFOLGE (CSS 2.1, Anhang E, vereinfacht)
// ===========================================================================

/// Mal-Schicht eines Elements (vereinfacht nach Anhang E).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schicht {
    /// Hintergrund/Rahmen der Stapelwurzel (immer zuerst).
    Hintergrund,
    /// Negativer z-index (aufsteigend sortiert).
    Negativ(i32),
    /// Block-Hintergruende im Fluss.
    Block,
    /// Floats (nicht positioniert).
    Schwebend,
    /// Inline-Inhalt im Fluss.
    Inline,
    /// Positioniert mit z-index auto/0 (in Dokumentreihenfolge).
    Positioniert,
    /// Positiver z-index (aufsteigend sortiert).
    Positiv(i32),
}

/// Rang einer Schicht (kleiner = weiter hinten). Innerhalb einer Schicht
/// entscheidet die Dokumentreihenfolge (stabiles Sortieren).
fn schicht_rang(s: &Schicht) -> (u8, i32) {
    match *s {
        Schicht::Hintergrund => (0, 0),
        Schicht::Negativ(z) => (1, z),
        Schicht::Block => (2, 0),
        Schicht::Schwebend => (3, 0),
        Schicht::Inline => (4, 0),
        Schicht::Positioniert => (5, 0),
        Schicht::Positiv(z) => (6, z),
    }
}

/// Mal-Reihenfolge: gibt die Indizes von `schichten` in der Reihenfolge
/// zurueck, in der zu zeichnen ist (stabil: gleiche Schicht behält die
/// Eingabereihenfolge). Laenge == Eingabelaenge (kein Element verloren).
/// O(n log n), eine Index-Allokation in Eingabelaenge (vom Aufrufer
/// gegeben, keine Eingabe-Allokation — M4 mit Begruendung im Moduldoc).
pub fn mal_reihenfolge(schichten: &[Schicht]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..schichten.len()).collect();
    // Stabiles Sortieren (merge sort): gleiche Schluessel behalten Ordnung.
    idx.sort_by_key(|&i| schicht_rang(&schichten[i]));
    idx
}

// ===========================================================================
// FLOAT + CLEAR (CSS 2.1, 9.5/9.5.2, einfache Fassung)
// ===========================================================================

/// X-Position einer Float-Box: links an `belegt_links`, rechts an
/// `container - breite - belegt_rechts` (jeweils >= 0). Vereinfacht: der
/// Aufrufer fuehrt die belegten Kanten (kein Float-Stapel im Modul).
pub fn float_x(
    container_breite: f32,
    box_breite: f32,
    seite: Float,
    belegt_links: f32,
    belegt_rechts: f32,
) -> f32 {
    let c = sauber(container_breite).clamp(0.0, MAX_PX);
    let b = sauber(box_breite).clamp(0.0, MAX_PX);
    match seite {
        Float::Links => sauber(belegt_links).clamp(0.0, MAX_PX),
        Float::Rechts => (c - b - sauber(belegt_rechts)).clamp(0.0, MAX_PX),
        Float::Keins => 0.0,
    }
}

/// Y nach clear (9.5.2, vereinfacht): faellt unter die Unterkanten der
/// Floats der geraeumten Seiten (oder bleibt).
pub fn clear_y(raeumung: Raeumung, y: f32, float_unten_links: f32, float_unten_rechts: f32) -> f32 {
    let y = sauber(y);
    let li = sauber(float_unten_links);
    let re = sauber(float_unten_rechts);
    match raeumung {
        Raeumung::Keins => y,
        Raeumung::Links => y.max(li),
        Raeumung::Rechts => y.max(re),
        Raeumung::Beide => y.max(li).max(re),
    }
}

// ===========================================================================
// BESCHNITT (overflow, C14)
// ===========================================================================

/// Beschnitt-Rechteck fuer overflow hidden/auto/scroll: Schnitt von
/// `behaelter` und `innen` (Innenkasten in Behaelter-Koordinaten). Bei
/// Sichtbar (`verborgen` false): None (kein Beschnitt). Sonst
/// Some(Schnitt), ggf. leer (0x0).
/// visibility steckt in Berechnet.sichtbar (keine Funktion noetig —
/// unsichtbar wird nicht gezeichnet, belegt aber Platz).
pub fn beschnitt(behaelter: &Rechteck, innen: &Rechteck, verborgen: bool) -> Option<Rechteck> {
    if !verborgen {
        return None;
    }
    Some(behaelter.schnitt(innen))
}

/// Beschneidet dieser `overflow`-Wert den Inhalt? (CSS 2.1, 11.1.1)
///
/// `hidden`, `auto` und `scroll` beschneiden ALLE drei — sie unterscheiden
/// sich nur darin, ob ein Rollbalken dazukommt, und das ist Sache des
/// Aufrufers (der Rasterizer), nicht der Boxrechnung. `visible`
/// beschneidet nie.
///
/// Das ist die Bruecke von `Berechnet.ueberlauf` (ein Schluesselwort) zu
/// [`beschnitt`] (ein `bool`), damit der Aufrufer nicht selbst eine
/// `match`-Tabelle fuer vier Werte pflegen muss.
pub fn ueberlauf_verbirgt(u: Ueberlauf) -> bool {
    match u {
        Ueberlauf::Versteckt | Ueberlauf::Auto | Ueberlauf::Scroll => true,
        Ueberlauf::Sichtbar => false,
    }
}

/// Beschnitt-Rechteck direkt aus dem `overflow`-Wert. Kurzform fuer den
/// Aufrufer: `beschnitt_ueberlauf(&box, &innen, box.ueberlauf)`.
pub fn beschnitt_ueberlauf(
    behaelter: &Rechteck,
    innen: &Rechteck,
    u: Ueberlauf,
) -> Option<Rechteck> {
    beschnitt(behaelter, innen, ueberlauf_verbirgt(u))
}

/// Rollbalken braucht dieser `overflow`-Wert? (`auto` und `scroll`,
/// CSS 2.1, 11.1.3). `hidden` hat welchen, `visible` nicht — die
/// Reihenfolge ist genau die aus der Spezifikation.
pub fn ueberlauf_rollt(u: Ueberlauf) -> bool {
    matches!(u, Ueberlauf::Auto | Ueberlauf::Scroll)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relativ_oben_links_gewinnt() {
        assert_eq!(
            relativ_versatz(Some(10.0), Some(99.0), Some(99.0), Some(5.0)),
            (5.0, 10.0)
        );
    }
    #[test]
    fn relativ_unten_rechts() {
        // Ohne top/left zaehlen bottom/right negativ.
        assert_eq!(
            relativ_versatz(None, Some(5.0), Some(10.0), None),
            (-5.0, -10.0)
        );
    }
    #[test]
    fn relativ_auto_null() {
        assert_eq!(relativ_versatz(None, None, None, None), (0.0, 0.0));
    }
    #[test]
    fn relativ_nicht_endlich() {
        assert_eq!(
            relativ_versatz(Some(f32::NAN), None, None, None),
            (0.0, 0.0)
        );
    }

    #[test]
    fn absolut_dehnung() {
        // links+rechts gesetzt, Breite auto -> Dehnung (10.3.7).
        assert_eq!(
            absolut_waagerecht(800.0, Some(10.0), None, Some(20.0), 0.0, 50.0),
            (10.0, 770.0)
        );
    }
    #[test]
    fn absolut_links_und_breite() {
        assert_eq!(
            absolut_waagerecht(800.0, Some(10.0), Some(100.0), Some(20.0), 0.0, 50.0),
            (10.0, 100.0)
        );
    }
    #[test]
    fn absolut_nur_rechts() {
        // x = 800 - 20 - 100 = 680.
        assert_eq!(
            absolut_waagerecht(800.0, None, Some(100.0), Some(20.0), 0.0, 50.0),
            (680.0, 100.0)
        );
    }
    #[test]
    fn absolut_statisch() {
        // Alles auto -> statische Position + Inhaltsbreite.
        assert_eq!(
            absolut_waagerecht(800.0, None, None, None, 33.0, 50.0),
            (33.0, 50.0)
        );
    }
    #[test]
    fn absolut_senkrecht_dehnung() {
        assert_eq!(
            absolut_senkrecht(600.0, Some(5.0), None, Some(15.0), 0.0, 40.0),
            (5.0, 580.0)
        );
    }
    #[test]
    fn absolut_negativ_breite_null() {
        assert_eq!(
            absolut_waagerecht(800.0, Some(0.0), Some(-5.0), None, 0.0, 10.0),
            (0.0, 0.0)
        );
    }

    #[test]
    fn enthaltend_positioniert() {
        let anfang = Rechteck::neu(0.0, 0.0, 800.0, 600.0);
        let aussen = Vorfahr {
            positioniert: false,
            kasten: Rechteck::neu(0.0, 0.0, 700.0, 500.0),
        };
        let innen = Vorfahr {
            positioniert: true,
            kasten: Rechteck::neu(10.0, 10.0, 300.0, 200.0),
        };
        // Kette von aussen nach innen: der naechste positionierte gewinnt.
        let e = enthaltender_block(anfang, &[aussen, innen]);
        assert_eq!(e, Rechteck::neu(10.0, 10.0, 300.0, 200.0));
    }
    #[test]
    fn enthaltend_anfang() {
        let anfang = Rechteck::neu(0.0, 0.0, 800.0, 600.0);
        let a = Vorfahr {
            positioniert: false,
            kasten: Rechteck::neu(0.0, 0.0, 1.0, 1.0),
        };
        assert_eq!(enthaltender_block(anfang, &[a]), anfang);
        assert_eq!(enthaltender_block(anfang, &[]), anfang);
    }

    #[test]
    fn stapel_reihenfolge() {
        // Eingabe: Inline(0), Positiv(1), Negativ(-1), Block(2), Hintergrund(3).
        let s = [
            Schicht::Inline,
            Schicht::Positiv(1),
            Schicht::Negativ(-1),
            Schicht::Block,
            Schicht::Hintergrund,
        ];
        assert_eq!(mal_reihenfolge(&s), vec![4, 2, 3, 0, 1]);
    }
    #[test]
    fn stapel_sortiert_z() {
        let s = [
            Schicht::Positiv(2),
            Schicht::Positiv(1),
            Schicht::Negativ(5),
        ];
        assert_eq!(mal_reihenfolge(&s), vec![2, 1, 0]);
    }
    #[test]
    fn stapel_stabil() {
        // Gleiche Schicht: Eingabereihenfolge bleibt.
        let s = [Schicht::Inline, Schicht::Inline, Schicht::Block];
        assert_eq!(mal_reihenfolge(&s), vec![2, 0, 1]);
    }
    #[test]
    fn stapel_leer() {
        assert!(mal_reihenfolge(&[]).is_empty());
    }

    #[test]
    fn float_positionen() {
        assert_eq!(float_x(800.0, 100.0, Float::Links, 20.0, 0.0), 20.0);
        assert_eq!(float_x(800.0, 100.0, Float::Rechts, 0.0, 30.0), 670.0);
        assert_eq!(float_x(800.0, 100.0, Float::Keins, 0.0, 0.0), 0.0);
    }
    #[test]
    fn clear_faelle() {
        assert_eq!(clear_y(Raeumung::Keins, 10.0, 50.0, 60.0), 10.0);
        assert_eq!(clear_y(Raeumung::Links, 10.0, 50.0, 60.0), 50.0);
        assert_eq!(clear_y(Raeumung::Rechts, 10.0, 50.0, 60.0), 60.0);
        assert_eq!(clear_y(Raeumung::Beide, 10.0, 50.0, 60.0), 60.0);
        assert_eq!(clear_y(Raeumung::Beide, 99.0, 50.0, 60.0), 99.0);
    }

    #[test]
    fn beschnitt_sichtbar_keiner() {
        let b = Rechteck::neu(0.0, 0.0, 100.0, 100.0);
        let i = Rechteck::neu(50.0, 50.0, 100.0, 100.0);
        assert_eq!(beschnitt(&b, &i, false), None);
        assert_eq!(
            beschnitt(&b, &i, true),
            Some(Rechteck::neu(50.0, 50.0, 50.0, 50.0))
        );
    }
    #[test]
    fn schnitt_leer() {
        let b = Rechteck::neu(0.0, 0.0, 10.0, 10.0);
        let i = Rechteck::neu(50.0, 50.0, 10.0, 10.0);
        assert_eq!(
            beschnitt(&b, &i, true),
            Some(Rechteck::neu(50.0, 50.0, 0.0, 0.0))
        );
    }

    #[test]
    fn ueberlauf_verbirgt_alle_vier() {
        // hidden/auto/scroll beschneiden, visible nicht.
        assert!(ueberlauf_verbirgt(Ueberlauf::Versteckt));
        assert!(ueberlauf_verbirgt(Ueberlauf::Auto));
        assert!(ueberlauf_verbirgt(Ueberlauf::Scroll));
        assert!(!ueberlauf_verbirgt(Ueberlauf::Sichtbar));
    }
    #[test]
    fn ueberlauf_rollt_nur_auto_und_scroll() {
        // 11.1.3: hidden hat einen Rollbalken, der IMMER sichtbar ist —
        // der wird aber nicht gerollt, also "rollt" nur auto/scroll.
        assert!(ueberlauf_rollt(Ueberlauf::Auto));
        assert!(ueberlauf_rollt(Ueberlauf::Scroll));
        assert!(!ueberlauf_rollt(Ueberlauf::Versteckt));
        assert!(!ueberlauf_rollt(Ueberlauf::Sichtbar));
    }
    #[test]
    fn beschnitt_ueberlauf_gleiche_zahlen() {
        let b = Rechteck::neu(0.0, 0.0, 100.0, 100.0);
        let i = Rechteck::neu(0.0, 0.0, 200.0, 50.0);
        assert_eq!(beschnitt_ueberlauf(&b, &i, Ueberlauf::Sichtbar), None);
        assert_eq!(
            beschnitt_ueberlauf(&b, &i, Ueberlauf::Versteckt),
            beschnitt_ueberlauf(&b, &i, Ueberlauf::Auto)
        );
        assert_eq!(
            beschnitt_ueberlauf(&b, &i, Ueberlauf::Scroll),
            Some(Rechteck::neu(0.0, 0.0, 100.0, 50.0))
        );
    }
}
