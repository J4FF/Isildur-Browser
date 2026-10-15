//! Release 08.10.2026 (Nutzerwunsch): DIE GLAS-FORMENSPRACHE FUER DIE GANZE
//! OBERFLAECHE — dieselbe wie im Kontextmenue ([`crate::ktx_menue`]).
//!
//! Bausteine:
//!
//! * [`feld`] — das schwebende Glasfeld (Menue, Aufklappfeld): dunkles Glas,
//!   Ringschatten, Glanz, Lichtkante.
//! * [`veredeln`] — eine Flaeche, die schon ihren Grund hat (aktiver Tab,
//!   Adressfeld, Karte), bekommt Glanz und Lichtkante obendrauf. Der Grund
//!   (die Themenfarbe) bleibt: Kontrast und Lesbarkeit aendern sich nicht.
//! * [`knopf_flaeche`] — die runde Hover-/Druck-Flaeche eines Knopfes.
//! * [`hell`] — die helle, durchscheinende Hover-Flaeche auf Glas.
//! * [`ueberschrift`] — kleine graue Versalien mit Sperrung.
//!
//! Alle Masse kommen als logische Pixel und werden mit der Skalierung in
//! Prozent ([`crate::ktx_menue::skala_prozent`]) umgerechnet.

use crate::Rechteck;
use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect};
use narsil_display::{DisplayItem, DisplayList};

pub use crate::ktx_menue::{
    gesperrt, px_mit, skala_prozent, GLAS_AUS, GLAS_DECKKRAFT, GLAS_GRAU, GLAS_GRUND, GLAS_HOVER,
    GLAS_ICON, GLAS_LINIE, GLAS_TEXT,
};

/// Die Lichtkante einer hervorgehobenen Flaeche (oben, rechts, unten, links):
/// oben das Licht, unten fast nichts.
pub const KANTE_HELL: [u8; 4] = [72, 30, 14, 30];
/// Die Lichtkante einer ruhigen Flaeche (Knopf, Hover).
pub const KANTE_LEISE: [u8; 4] = [44, 18, 8, 18];
/// Der Glanz einer hervorgehobenen Flaeche (Weiss-Deckung oben, 0..255).
pub const GLANZ_HELL: u8 = 20;
/// Der Glanz einer ruhigen Flaeche.
pub const GLANZ_LEISE: u8 = 12;

/// Die Skalierung einer Chrome in Prozent.
pub fn prozent_von(chrome: &crate::Chrome) -> u16 {
    skala_prozent(chrome.sk())
}

fn schatten_leer() -> [narsil_core::style::SchattenArt; narsil_core::style::MAX_SCHATTEN] {
    [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN]
}

/// DAS SCHWEBENDE GLASFELD — wie das Kontextmenue (Ringschatten, Grund,
/// Glanz, Lichtkante).
pub fn feld(liste: &mut DisplayList, r: &Rechteck, prozent: u16) {
    crate::ktx_menue::glas_feld(liste, r, prozent);
}

/// DER GLANZ: ein Verlauf von Weiss (`alpha` oben) nach durchsichtig bei
/// `bis` (Anteil der Hoehe) — das Licht, das auf die Flaeche faellt.
pub fn glanz(rect: Rect, radius: [f32; 4], alpha: u8, bis: f32) -> DisplayItem {
    let mut v = narsil_core::verlauf_typ::Verlauf::linear_zwei(
        Color::new(255, 255, 255, alpha),
        Color::new(255, 255, 255, 0),
    );
    v.winkel = 180.0;
    v.stopps[1].anteil = Some(bis.clamp(0.05, 1.0));
    DisplayItem::Verlauf { rect, radius, verlauf: v }
}

/// DIE LICHTKANTE: ein Ring mit der Rundung der Flaeche, eine Skalierung
/// dick (mindestens 1 px), je Kante eine Weiss-Deckung (oben, rechts,
/// unten, links).
pub fn lichtkante(rect: Rect, radius: [f32; 4], prozent: u16, alpha: [u8; 4]) -> DisplayItem {
    use narsil_core::style::{Rahmen, RahmenStil};
    let d = (prozent.max(1) as f32 / 100.0).round().max(1.0);
    let weiss = |a: u8| Color::new(255, 255, 255, a);
    DisplayItem::Rand {
        rect,
        rahmen: Rahmen {
            breite: [d; 4],
            stil: [RahmenStil::Durchgezogen; 4],
            farbe: [weiss(alpha[0]), weiss(alpha[1]), weiss(alpha[2]), weiss(alpha[3])],
        },
        radius,
    }
}

/// EINE FLAECHE VEREDELN: auf ihren vorhandenen Grund kommen Glanz (bis zur
/// halben Hoehe) und Lichtkante.
pub fn veredeln(
    liste: &mut DisplayList,
    rect: Rect,
    radius: [f32; 4],
    prozent: u16,
    glanz_alpha: u8,
    kante: [u8; 4],
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    // Release 09.10.2026 (Liquid Glass): Glanz und DOPPELTE Lichtkante —
    // innen hell (oben am hellsten, unten fast weg), aussen eine dunkle
    // Linie — kantengeglaettet ueber den Glas-Effekt des Rasterers. Vorher
    // ein Verlauf und ein Rand mit harten Eckpixeln.
    let s = prozent.max(1) as f32 / 100.0;
    liste.glas(narsil_display::Glas::kanten(
        rect,
        radius,
        s,
        f32::from(glanz_alpha) / 255.0 * 0.8,
        f32::from(kante[0]) / 255.0,
        0.28,
    ));
}

/// Release 09.10.2026: eine FLACHE GLASFLAECHE auf der Oberflaeche
/// (Kategorienliste, Karten): weichgezeichneter Untergrund, leichte
/// Aufhellung, leiser Schatten, doppelte Lichtkante.
pub fn flaeche(liste: &mut DisplayList, rect: Rect, radius: [f32; 4], prozent: u16) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let s = prozent.max(1) as f32 / 100.0;
    liste.glas(narsil_display::Glas::flach(rect, radius, s));
}

/// Release 09.10.2026 (Liquid Glass): DER SCHIMMER hinter dem Glas — zwei
/// sehr leise farbige Lichthoefe (Blau oben links, Violett unten rechts) auf
/// dem dunklen Grund `grund` einer Oberflaechenseite (Einstellungen). Ohne ihn
/// ist Glas auf einfarbigem Grund nur eine graue Flaeche: erst ein Untergrund
/// mit Verlauf zeigt, dass das Glas weichzeichnet und saettigt.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein BILD und keine zwei Radialverlaeufe:
/// gemessen (Release, 1100 x 700) kosteten die zwei seitengrossen Verlaeufe
/// 70 ms je Bild — bei jedem Hover. Das Bild wird je Groesse und Farbe EINMAL
/// gerechnet (mit feinem Rauschen gegen Stufen) und danach nur kopiert.
pub fn schimmer(liste: &mut DisplayList, rect: Rect, grund: Color) {
    if rect.width < 1.0 || rect.height < 1.0 {
        return;
    }
    let (b, h) = (rect.width.round() as u32, rect.height.round() as u32);
    if b > 8192 || h > 8192 {
        return;
    }
    let pixels = schimmer_pixel(b, h, grund);
    liste.push(DisplayItem::Bild { rect: Rect::new(rect.x, rect.y, b as f32, h as f32), breite: b, hoehe: h, pixels });
}

/// Die Pixel des Schimmers (deckend), je (Breite, Hoehe, Grund) gemerkt.
fn schimmer_pixel(b: u32, h: u32, grund: Color) -> std::sync::Arc<Vec<u8>> {
    use std::sync::{Arc, Mutex};
    type Gemerkt = Option<((u32, u32, [u8; 4]), Arc<Vec<u8>>)>;
    static GEMERKT: Mutex<Gemerkt> = Mutex::new(None);
    let schluessel = (b, h, [grund.r, grund.g, grund.b, grund.a]);
    if let Ok(g) = GEMERKT.lock()
        && let Some((k, p)) = g.as_ref()
        && *k == schluessel
    {
        return Arc::clone(p);
    }
    // Zwei Hoefe: (Farbe, Staerke, Mitte x, Mitte y, Reichweite als Anteil).
    let hoefe = [([77.0f32, 142.0, 244.0], 34.0f32 / 255.0, 0.12f32, 0.0f32, 0.55f32), ([150.0, 96.0, 232.0], 26.0 / 255.0, 0.92, 1.0, 0.6)];
    let (bf, hf) = (b as f32, h as f32);
    let mut px = vec![0u8; (b as usize) * (h as usize) * 4];
    for y in 0..h {
        for x in 0..b {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut c = [f32::from(grund.r), f32::from(grund.g), f32::from(grund.b)];
            for (farbe, staerke, mx, my, bis) in hoefe {
                // Ellipse durch die fernste Ecke (wie `radial-gradient`).
                let rx = (mx * bf).max(bf - mx * bf) * std::f32::consts::SQRT_2;
                let ry = (my * hf).max(hf - my * hf) * std::f32::consts::SQRT_2;
                let d = (((fx - mx * bf) / rx.max(1.0)).powi(2) + ((fy - my * hf) / ry.max(1.0)).powi(2)).sqrt();
                let a = staerke * (1.0 - d / bis).clamp(0.0, 1.0);
                for k in 0..3 {
                    c[k] += (farbe[k] - c[k]) * a;
                }
            }
            // Feines, ortsfestes Rauschen (±0,5 Stufen) gegen Banding.
            let n = (((x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) % 1000) as f32 / 1000.0) - 0.5;
            let i = ((y * b + x) * 4) as usize;
            for k in 0..3 {
                px[i + k] = (c[k] + n).round().clamp(0.0, 255.0) as u8;
            }
            px[i + 3] = 255;
        }
    }
    let px = Arc::new(px);
    if let Ok(mut g) = GEMERKT.lock() {
        *g = Some((schluessel, Arc::clone(&px)));
    }
    px
}

/// DIE RUNDE KNOPF-FLAECHE (Hover oder gedrueckt): eine Pille bzw. ein Kreis
/// in `farbe`, mit leiser Lichtkante.
pub fn knopf_flaeche(liste: &mut DisplayList, rect: Rect, farbe: Color, prozent: u16) {
    let r = (rect.width.min(rect.height) / 2.0).max(0.0);
    liste.push(DisplayItem::Rect {
        rect,
        color: farbe,
        radius: [r; 4],
        deckkraft: 1.0,
        schatten: schatten_leer(),
        schatten_anzahl: 0,
    });
    liste.push(lichtkante(rect, [r; 4], prozent, KANTE_LEISE));
}

/// DIE HELLE, DURCHSCHEINENDE FLAECHE (Hover auf Glas): Weiss mit
/// `deckkraft`.
pub fn hell(rect: Rect, radius: [f32; 4], deckkraft: f32) -> DisplayItem {
    DisplayItem::Rect {
        rect,
        color: Color::WHITE,
        radius,
        deckkraft,
        schatten: schatten_leer(),
        schatten_anzahl: 0,
    }
}

/// EINE GRUPPENUEBERSCHRIFT: kleine graue Versalien mit Sperrung.
pub fn ueberschrift(x: f32, y: f32, text: &str, groesse: f32, farbe: Color) -> DisplayItem {
    DisplayItem::Text {
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        origin: Point::new(x, y),
        text: gesperrt(&text.to_uppercase()),
        color: farbe,
        font_size: groesse,
        gewicht: 600,
        kursiv: false,
    }
}

/// DAS DUNKLE GLAS ALS FARBE MIT DECKKRAFT (fuer Flaechen ohne Schatten).
pub fn grund(rect: Rect, radius: [f32; 4]) -> DisplayItem {
    DisplayItem::Rect {
        rect,
        color: GLAS_GRUND,
        radius,
        deckkraft: GLAS_DECKKRAFT,
        schatten: schatten_leer(),
        schatten_anzahl: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_bausteine_haben_die_form_der_flaeche() {
        let r = Rect::new(10.0, 20.0, 100.0, 32.0);
        let mut l = DisplayList::new();
        veredeln(&mut l, r, [8.0; 4], 150, GLANZ_HELL, KANTE_HELL);
        // Release 09.10.2026: ein Glas-Effekt (Nebentabelle), kein Item.
        assert_eq!(l.len(), 0);
        match l.effekte() {
            [(0, None, narsil_display::Effekt::Glas(g))] => {
                assert_eq!(g.rect, r);
                assert_eq!(g.radius, [8.0; 4]);
                // bei 1,5x ist die Kante 2 px (gerundet), kein Weichzeichnen
                assert_eq!(g.linie, 2.0);
                assert_eq!(g.unschaerfe, 0.0);
                assert!(g.kante_hell > 0.0 && g.kante_dunkel > 0.0);
            }
            andere => panic!("kein Glas: {andere:?}"),
        }
        // Der Knopf ist rund (halbe kurze Kante).
        let mut k = DisplayList::new();
        knopf_flaeche(&mut k, Rect::new(0.0, 0.0, 32.0, 32.0), Color::rgb(1, 2, 3), 100);
        assert!(matches!(k.get(0), Some(DisplayItem::Rect { radius, .. }) if *radius == [16.0; 4]));
        // Die Ueberschrift ist gesperrte Versalschrift.
        match ueberschrift(0.0, 0.0, "Allgemein", 10.5, GLAS_GRAU) {
            DisplayItem::Text { text, .. } => assert_eq!(text, gesperrt("ALLGEMEIN")),
            _ => panic!("kein Text"),
        }
    }
}
