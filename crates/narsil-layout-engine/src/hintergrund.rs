//! HINTERGRUNDBILD: DIE GEOMETRIE. CSS Backgrounds 3 §3.3 (size), §3.4
//! (position), §3.5 (repeat).
//!
//! GEMESSENER GRUND FUER DIESE DATEI (06.10.2026 05:40): auf 28 Seiten
//! tragen **136 Elemente** ein `background-image: url(...)`, und **keines
//! wird geholt** (Werkzeug `NARSIL_HINTERGRUND=1`, `team/PROBLEME.txt`
//! 05:40). `narsil_css::BildGeo` traegt `groesse`, `cover`,
//! `contain`, `position_x`, `position_y`, `position_px`, `wiederholen_x`
//! und `wiederholen_y` — **die Geometrie ist vollstaendig im Wert
//! vorhanden, und es liest niemand.** Das ist die Luecke, die ich
//! schliesse: nicht das Abrufen und nicht das Zeichnen (das braucht eine
//! Zeile in einer fremden Crate, siehe `PROBLEME.txt` 05:40, Abschnitt
//! 9), sondern die **Rechnung**, die vorher niemand gemacht hat.
//!
//! WARUM EINE EIGENE DATEI und kein Zweig in `display.rs`: die Rechnung
//! ist eine **Rechnung ueber Werte** — kein Kasten, kein Knoten, kein
//! DOM. Sie ist damit unabhaengig pruefbar, und `display.rs` (das den
//! DOM braucht) und `flow.rs` (das den Kasten braucht) muessen sie nicht
//! kennen.
//!
//! **DIE GRENZE, und sie ist eine Grenze des WERTES, nicht der Datei:**
//! `BildGeo.groesse` ist **eine** Zahl, nicht zwei. `background-size:
//! auto 40px` (Hoehe fest, Breite frei) kann der Wert also nicht
//! ausdruecken, und diese Funktion kann es nicht rechnen. `0` heisst
//! `auto`. Das ist eine Luecke in `narsil-css`, keine in hier — und ich
//! baue sie nicht, weil sie eine fremde Crate aendern wuerde.

use narsil_core::geometry::Rect;

/// Release 13.10.2026 (Agent CSS2): Hoechstzahl der Kacheln EINES Verlaufs.
pub const MAX_VERLAUF_KACHELN: usize = 4096;

/// Release 13.10.2026 (Agent CSS2): die Kacheln eines Verlaufs als
/// Hintergrundbild (CSS Backgrounds 3 §3.5/§3.6/§3.9): `lage` ist die
/// Lageflaeche (`background-origin`), `mal` die Malflaeche
/// (`background-clip`). Zurueck kommen die ungeschnittenen Kachelrechtecke,
/// die `mal` beruehren — hoechstens [`MAX_VERLAUF_KACHELN`].
pub fn verlauf_kacheln(lage: Rect, mal: Rect, k: &narsil_core::verlauf_typ::Kachel) -> Vec<Rect> {
    use narsil_core::verlauf_typ::KachelMass as M;
    let mass = |m: M, flaeche: f32| match m {
        M::Auto => flaeche,
        M::Px(v) => v,
        M::Anteil(a) => a * flaeche,
    };
    let (mut w, mut h) = (mass(k.groesse[0], lage.width), mass(k.groesse[1], lage.height));
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return Vec::new();
    }
    // Release 14.10.2026 (Agent CSS2): a repeated tile below one pixel is
    // drawn at least one pixel large, and tiles are enlarged until they fit the
    // tile limit (CSS Images 3 §3: the UA may approximate a tiny image);
    // otherwise the limit cut the area off. MEASURED before (WPT
    // css-backgrounds `background-size-near-zero-gradient`, 0.2px tiles in a
    // 100px box): only part of the box was filled.
    if k.wiederholen[0] && w < 1.0 {
        w = 1.0;
    }
    if k.wiederholen[1] && h < 1.0 {
        h = 1.0;
    }
    // Still too many tiles for the limit: enlarge them evenly (the drawn
    // colour is the tile average either way).
    let nx = if k.wiederholen[0] { (mal.width / w).ceil() + 1.0 } else { 1.0 };
    let ny = if k.wiederholen[1] { (mal.height / h).ceil() + 1.0 } else { 1.0 };
    if nx * ny > MAX_VERLAUF_KACHELN as f32 {
        let f = (nx * ny / MAX_VERLAUF_KACHELN as f32).sqrt() * 1.05;
        let f = if k.wiederholen[0] && k.wiederholen[1] { f } else { f * f };
        if k.wiederholen[0] {
            w *= f;
        }
        if k.wiederholen[1] {
            h *= f;
        }
    }
    // Release 13.10.2026 (Agent CSS2): `round` (CSS Backgrounds 3 §3.4).
    if k.art[0] == 2 && lage.width > 0.0 {
        w = lage.width / (lage.width / w).round().max(1.0);
    }
    if k.art[1] == 2 && lage.height > 0.0 {
        h = lage.height / (lage.height / h).round().max(1.0);
    }
    let ort = |(v, px): (f32, bool), ende: bool, flaeche: f32, kachel: f32| match (px, ende) {
        (true, true) => flaeche - kachel - v,
        (true, false) => v,
        _ => (flaeche - kachel) * v,
    };
    let x = lage.x + ort(k.lage[0], k.vom_ende[0], lage.width, w);
    let y = lage.y + ort(k.lage[1], k.vom_ende[1], lage.height, h);
    let achse = |start: f32, groesse: f32, periode: f32, wdh: bool, von: f32, bis: f32| -> Vec<f32> {
        if !wdh {
            return vec![start];
        }
        let mut p = start - ((start - von) / periode).ceil() * periode;
        let mut raus = Vec::new();
        while p < bis && raus.len() < MAX_VERLAUF_KACHELN {
            raus.push(p);
            p += periode;
        }
        let _ = groesse;
        raus
    };
    // Release 13.10.2026 (Agent CSS2): `space` — ganze Kacheln, gleichmaessig
    // verteilt, ab der Lagekante; passt keine zweite, gilt die Position.
    let raum = |art: u8, anfang: f32, laenge: f32, groesse: f32, pos: f32, wdh: bool| -> (f32, f32, bool) {
        if art == 1 {
            let n = (laenge / groesse).floor();
            if n >= 2.0 {
                return (anfang, groesse + (laenge - n * groesse) / (n - 1.0), true);
            }
            return (pos, groesse, false);
        }
        (pos, groesse, wdh)
    };
    let (x0, px, wx) = raum(k.art[0], lage.x, lage.width, w, x, k.wiederholen[0]);
    let (y0, py, wy) = raum(k.art[1], lage.y, lage.height, h, y, k.wiederholen[1]);
    let xs = achse(x0, w, px, wx, mal.x, mal.x + mal.width);
    let ys = achse(y0, h, py, wy, mal.y, mal.y + mal.height);
    let mut raus = Vec::new();
    for &ty in &ys {
        for &tx in &xs {
            if raus.len() >= MAX_VERLAUF_KACHELN {
                return raus;
            }
            if tx < mal.x + mal.width && tx + w > mal.x && ty < mal.y + mal.height && ty + h > mal.y {
                raus.push(Rect::new(tx, ty, w, h));
            }
        }
    }
    raus
}

/// Obergrenze der Kacheln je Element.
///
/// GEMESSENER GRUND fuer eine Grenze ueberhaupt: `background-repeat`
/// ist bei **`repeat` voreingestellt** (CSS Backgrounds 3 §3.5), und ein
/// 2x2-Pixel-Bild auf einer 900x650-Flaeche ergibt **146 250** Kacheln.
/// Ohne Grenze waere das eine Allokation von 2,3 MB Rechtecken fuer
/// ein Hintergrundbild, das man nicht sieht. Die Grenze wird **vor**
/// der Allokation geprueft und die Zahl der verworfenen Kacheln
/// **mitgegeben** (M3/M4 dieser Codebasis: mitzaehlen, nicht
/// verschweigen).
pub const MAX_KACHELN: usize = 256;

/// Das Ergebnis: die Kachelrechtecke und — falls die Grenze griff — die
/// Zahl der verworfenen.
#[derive(Debug, Clone, PartialEq)]
pub struct Kacheln {
    /// Die Rechtecke, **auf die Flaeche beschnitten**. Leer ist
    /// moeglich und normal: ein Hintergrundbild, das vollstaendig
    /// ausserhalb liegt, liefert nichts.
    pub rechtecke: Vec<Rect>,
    /// Kacheln, die es mathematisch gab, aber **nicht gebaut** wurden,
    /// weil die Obergrenze griff. `0` heisst „alles gebaut".
    ///
    /// GEMESSENER FEHLER, den ich selbst behoben habe: die erste Fassung
    /// rechnete `roh - roh` und meldete deshalb **immer 0**. Ein Zaehler,
    /// der nie zaehlt, ist schlimmer als keiner.
    pub verworfen: usize,
    /// Kacheln, die gebaut, aber vom Schnitt **verworfen** wurden — sie
    /// lagen ganz oder teilweise ausserhalb der Flaeche.
    ///
    /// WARUM DAS EIGENE FELD und nicht in `verworfen`: die beiden Zahlen
    /// sind zwei verschiedene Verluste, und `background-position: right`
    /// erzeugt den zweiten bei jedem Lauf. Wer beides in eine Zahl
    /// schmiert, kann nicht mehr sagen, ob die Grenze oder die Position
    /// gegriffen hat. **Die vollstaendige Rechnung ist damit:**
    /// `roh = gebaut + verworfen` und `gebaut = sichtbar +
    /// weggeschnitten`.
    pub weggeschnitten: usize,
    /// Kachelbreite und -hoehe in px, **nach** `background-size`.
    /// `0.0` heisst: gar keine Kachel berechenbar (Bild ohne Masse).
    pub kachel: (f32, f32),
}

/// Die Kachelmasse nach `background-size` (CSS Backgrounds 3 §3.3).
///
/// `natuerlich` ist die **Pixelmasse des dekodierten Bildes** (nicht die
/// `width`-/`height`-Attribute im HTML — die sind ein Hinweis, kein
/// Mass, und ein Browser nimmt das dekodierte).
///
/// **GEMESSENE ENTSCHEIDUNG, `background-size` mit EINER Laenge:** CSS
/// sagt `background-size: 30px` = 30 px **Breite**, Hoehe aus dem
/// Seitenverhaeltnis. `BildGeo.groesse` hat nur ein Feld, also ist es
/// die Breite; ich rechne die Hoehe aus dem Verhaeltnis. Wer das anders
/// liest, haelt die andere Deutung fuer korrekt — deshalb steht es hier
/// und nicht in einem Kommentar am Rand.
pub fn kachelmasse(flaeche: Rect, geo: &narsil_css::BildGeo, natuerlich: (f32, f32)) -> (f32, f32) {
    let (nw, nh) = natuerlich;
    if nw <= 0.0 || nh <= 0.0 {
        return (0.0, 0.0);
    }
    // `cover` und `contain` schlagen die Laenge: sie sind spaeter im
    // Kaskadenblatt und damit specificoerer. `BildGeo` kann beides
    // gleichzeitig true haben (zwei Deklarationen); in dem Fall gewinnt
    // `cover`, und die Begruendung steht hier: es ist die
    // ueberraschendere Wahl, also die, die man melden muss.
    if geo.cover {
        let s = (flaeche.width / nw).max(flaeche.height / nh);
        return (nw * s, nh * s);
    }
    if geo.contain {
        let s = (flaeche.width / nw).min(flaeche.height / nh);
        return (nw * s, nh * s);
    }
    if geo.groesse > 0.0 {
        return (geo.groesse, geo.groesse * nh / nw);
    }
    // `0` = `auto`: die natuerliche Masse.
    (nw, nh)
}

/// Die Rechtecke, auf die ein Hintergrundbild gelegt wird.
///
/// **DER GANZE INHALT** ist CSS Backgrounds 3 §3.3–3.5 auf einen
/// Bildpunktfall heruntergerechnet: eine Kachelmasse, ein Versatz, und
/// die Kacheln im Band der Flaeche.
pub fn kacheln(flaeche: Rect, geo: &narsil_css::BildGeo, natuerlich: (f32, f32)) -> Kacheln {
    if flaeche.width <= 0.0 || flaeche.height <= 0.0 {
        return Kacheln {
            rechtecke: Vec::new(),
            verworfen: 0,
            weggeschnitten: 0,
            kachel: (0.0, 0.0),
        };
    }
    let kachel = kachelmasse(flaeche, geo, natuerlich);
    let (kw, kh) = kachel;
    if kw <= 0.0 || kh <= 0.0 {
        return Kacheln {
            rechtecke: Vec::new(),
            verworfen: 0,
            weggeschnitten: 0,
            kachel,
        };
    }

    //
    // §3.4 POSITION. Zwei Formen, und `position_px` sagt, welche gilt:
    // * Anteil (0..1): vom **Rest** aus, den die Kaelge braucht — so
    //   macht `center` (0.5) die Kachel mittig, ohne die Flaechenmasse zu
    //   kennen.
    // * Pixel: vom **Rand** aus. CSS `background-position: 30px` heisst
    //   30 px von links, nicht 30 px vom Rest.
    // Release 11.10.2026 (Agent CSS2): je Achse (`px_achsen`).
    let (px_x, px_y) = geo.px_achsen();
    let versatz_x = if px_x {
        geo.position_x
    } else {
        (flaeche.width - kw) * geo.position_x
    };
    let versatz_y = if px_y {
        geo.position_y
    } else {
        (flaeche.height - kh) * geo.position_y
    };

    let anzahl_x = if geo.wiederholen_x {
        (flaeche.width / kw).ceil() + 1.0
    } else {
        1.0
    };
    let anzahl_y = if geo.wiederholen_y {
        (flaeche.height / kh).ceil() + 1.0
    } else {
        1.0
    };
    // `+1` oben und unten ist Absicht: bei einem Versatz **im** Band
    // (z. B. `right`) beginnt die erste Kachel rechts vom Rand, und
    // ohne die eine fehlende Kachel bleibt die linke Ecke leer.
    //
    // DIE GRENZE GREIFT **GESAMT**, nicht je Achse.
    //
    // **MEIN EIGENER FEHLER, gefunden von meinem eigenen Test
    // (`die_obergrenze_greift`):** die erste Fassung klemmte `n_x` und
    // `n_y` einzeln auf `MAX_KACHELN`. Bei 2x2-Pixeln auf 900x650 sind
    // das 451 x 326, geklemmt 256 x 256 = **65 536 Kacheln** — vierundzwanzigmal
    // mehr als die Grenme verspricht, und die gemeldete `verworfen`-Zahl
    // war **immer 0**, weil `n_x * n_y` nach dem Klemmen dasselbe war wie
    // davor. Ein Zaehler, der nie zaehlt, ist schlimmer als keiner.
    //
    // **PROPORTIONAL KLEMMEN, und nicht eine Achse:** bei 2x2-Pixeln auf
    // 900x650 waeren 18 x 14 Kacheln gebaut — das Muster bleibt in
    // beiden Richtungen gleichmaessig. Eine einzige Achse zu klemmen
    // (nur y) ergaebe 451 x 1: ein Streifen am oberen Rand, und der Rest
    // der Flaeche waere mit *nichts* gefuellt, statt mit *verkleinerten*
    // Kacheln. Das ist der Unterschied zwischen „zu wenig" und
    // „falsch verteilt".
    let n_x_roh = (anzahl_x as usize).max(1);
    let n_y_roh = (anzahl_y as usize).max(1);
    let (n_x, n_y) = if n_x_roh.saturating_mul(n_y_roh) <= MAX_KACHELN {
        (n_x_roh, n_y_roh)
    } else {
        let faktor = (MAX_KACHELN as f32 / (n_x_roh as f32 * n_y_roh as f32)).sqrt();
        let bx = ((n_x_roh as f32) * faktor).floor().max(1.0) as usize;
        let by = (MAX_KACHELN / bx).max(1);
        (bx, by)
    };
    // `+1` ist Absicht: bei einem Versatz **im** Band (z. B. `right`)
    // beginnt die erste Kachel rechts vom Rand, und ohne die eine
    // fehlende Kachel bleibt die linke Ecke leer.
    let gebaut = n_x.saturating_mul(n_y);
    let verworfen = n_x_roh.saturating_mul(n_y_roh).saturating_sub(gebaut);

    let mut rechtecke = Vec::with_capacity(n_x * n_y);
    let mut weggeschnitten = 0usize;
    for iy in 0..n_y {
        for ix in 0..n_x {
            let y = versatz_y + iy as f32 * kh;
            let x = versatz_x + ix as f32 * kw;
            if let Some(r) = schneide(
                Rect {
                    x,
                    y,
                    width: kw,
                    height: kh,
                },
                flaeche,
            ) {
                rechtecke.push(r);
            } else {
                weggeschnitten += 1;
            }
        }
    }
    Kacheln {
        rechtecke,
        verworfen,
        weggeschnitten,
        kachel,
    }
}

/// Schneidet `r` auf `flaeche` zu und verwirft, was leer bleibt.
///
/// **WARUM DAS KEIN `Rect::schneide` IST:** ein Schnitt kann ein Rechteck
/// ergeben, das **ausserhalb** liegt (etwa weil `background-position`
/// eine Kachel ganz rechts aus dem Fenster schiebt). Eine Nullflaeche ist
/// kein Fehler, sie ist „nichts sichtbar" — und `None` sagt genau das.
fn schneide(r: Rect, flaeche: Rect) -> Option<Rect> {
    let x = r.x.max(flaeche.x);
    let y = r.y.max(flaeche.y);
    let rechts = (r.x + r.width).min(flaeche.x + flaeche.width);
    let unten = (r.y + r.height).min(flaeche.y + flaeche.height);
    if rechts <= x || unten <= y {
        return None;
    }
    Some(Rect {
        x,
        y,
        width: rechts - x,
        height: unten - y,
    })
}

// ===========================================================================
// TESTS — jede Zahl hier ist eine Behauptung, die man nachrechnen kann.
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use narsil_css::BildGeo;

    fn geo() -> BildGeo {
        BildGeo::default()
    }

    fn flaeche() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        }
    }

    /// GEMESSENE GRUNDLAGE (CSS Backgrounds 3 §3.5): `background-repeat`
    /// ist **`repeat` voreingestellt**. Wer das nicht weiss, haelt 100x50
    /// bei einem 20x20-Bild fuer „ein Bild" und nicht fuer fuenf.
    #[test]
    fn voreinstellung_ist_wiederholen_in_beiden_achsen() {
        let g = geo();
        assert!(g.wiederholen_x && g.wiederholen_y, "CSS sagt repeat");
        let k = kacheln(flaeche(), &g, (20.0, 20.0));
        //
        // **DIE ZAHLEN SIND EXAKT, nicht "> viele"** — und sie sind erst
        // nach meinem eigenen Testfehler genau geworden. Ein 20x20-Bild
        // auf 100x50: 5 volle Spalten, 2 volle Zeilen und eine halbe =
        // **15 sichtbare** Kacheln. Die 6. Spalte (x = 100) und die 4.
        // Zeile (y = 60) liegen ganz ausserhalb und werden verworfen —
        // **9 von 24**. Wer 20 oder 30 erwartet, hat Randkacheln
        // mitgezaehlt, die es nicht gibt: genau mein erster Fehler hier.
        assert_eq!(k.rechtecke.len(), 15, "sichtbar: {:?}", k.rechtecke);
        assert_eq!(k.weggeschnitten, 9, "6. Spalte (5) + 4. Zeile (3) + 1");
        assert_eq!(k.verworfen, 0, "die Grenze hat nicht gegriffen");
    }

    #[test]
    fn auto_nimmt_die_natuerliche_masse() {
        let g = geo();
        g_ohne_schritt(&g);
        let k = kacheln(flaeche(), &g, (30.0, 10.0));
        assert_eq!(k.kachel, (30.0, 10.0), "kein scaling ohne background-size");
    }

    /// `groesse` ist die **Breite** (CSS Backgrounds 3 §3.3), die Hoehe
    /// folgt aus dem Seitenverhaeltnis. Ein 40x20-Bild mit
    /// `background-size: 10px` ergibt 10x5, nicht 10x10.
    #[test]
    fn eine_lange_ist_die_breite_und_hoehe_folgt_aus_dem_verhaeltnis() {
        let mut g = geo();
        g.groesse = 10.0;
        g.wiederholen_x = false;
        g.wiederholen_y = false;
        let k = kacheln(flaeche(), &g, (40.0, 20.0));
        assert_eq!(k.kachel, (10.0, 5.0), "40x20 auf 10 px Breite = 10x5");
    }

    #[test]
    fn cover_fuellt_die_flaeche_und_haelt_das_verhaeltnis() {
        let mut g = geo();
        g.cover = true;
        // Flaeche 100x50 ist 2:1, das Bild 40x20 ist 2:1 -> deckungsgleich.
        let k = kacheln(flaeche(), &g, (40.0, 20.0));
        assert!(
            (k.kachel.0 - 100.0).abs() < 0.01,
            "Breite = 100: {}",
            k.kachel.0
        );
        assert!(
            (k.kachel.1 - 50.0).abs() < 0.01,
            "Hoehe = 50: {}",
            k.kachel.1
        );
    }

    /// `cover` muss die **kleinere** Achse ueberfuellen: Flaeche 100x50,
    /// Bild 10x10 (quadratisch) -> 100x100, nicht 50x50 (das waere
    /// `contain`).
    #[test]
    fn cover_ueberfuellt_und_contain_lässt_luft() {
        let mut c = geo();
        c.cover = true;
        let kc = kacheln(flaeche(), &c, (10.0, 10.0));
        assert!(
            (kc.kachel.0 - 100.0).abs() < 0.01,
            "cover Breite: {}",
            kc.kachel.0
        );
        assert!(
            (kc.kachel.1 - 100.0).abs() < 0.01,
            "cover Hoehe: {}",
            kc.kachel.1
        );

        let mut ct = geo();
        ct.contain = true;
        let kt = kacheln(flaeche(), &ct, (10.0, 10.0));
        assert!(
            (kt.kachel.0 - 50.0).abs() < 0.01,
            "contain = 50: {}",
            kt.kachel.0
        );
        assert!(
            (kt.kachel.1 - 50.0).abs() < 0.01,
            "contain = 50: {}",
            kt.kachel.1
        );
    }

    /// `center` ist 0.5 (§3.4) und **der Rest** ist die Grundlage: bei
    /// einer 30 px breiten Kachel in 100 px bleibt 70 px Rest, und 0.5
    /// davon sind 35 px.
    #[test]
    fn center_rechnet_vom_rest_nicht_von_der_kante() {
        let mut g = geo();
        g.wiederholen_x = false;
        g.wiederholen_y = false;
        g.position_x = 0.5;
        g.position_y = 0.5;
        let k = kacheln(flaeche(), &g, (30.0, 10.0));
        assert_eq!(k.rechtecke.len(), 1, "kein repeat -> eine Kachel");
        let r = k.rechtecke[0];
        assert!((r.x - 35.0).abs() < 0.01, "x = 35: {}", r.x);
        assert!((r.y - 20.0).abs() < 0.01, "y = 20: {}", r.y);
    }

    /// `position_px` heisst: **vom Rand**, nicht vom Rest. `10px` auf
    /// einer 30-px-Kachel in 100 px ist x = 10, nicht 7.
    #[test]
    fn pixel_position_ist_vom_rand() {
        let mut g = geo();
        g.wiederholen_x = false;
        g.wiederholen_y = false;
        g.position_px = true;
        g.position_x = 10.0;
        g.position_y = 4.0;
        let k = kacheln(flaeche(), &g, (30.0, 10.0));
        let r = k.rechtecke[0];
        assert!((r.x - 10.0).abs() < 0.01, "x = 10: {}", r.x);
        assert!((r.y - 4.0).abs() < 0.01, "y = 4: {}", r.y);
    }

    /// Eine Kachel, die **ganz** ausserhalb liegt, liefert `None` und
    /// damit kein Rechteck — nicht ein Rechteck mit Breite 0, das der
    /// Zeichner als „da" liest.
    #[test]
    fn komplett_ausserhalb_ergibt_kein_rechteck() {
        let mut g = geo();
        g.wiederholen_x = false;
        g.wiederholen_y = false;
        g.position_px = true;
        g.position_x = 500.0;
        g.position_y = 500.0;
        let k = kacheln(flaeche(), &g, (30.0, 10.0));
        assert!(k.rechtecke.is_empty(), "nichts sichtbar: {:?}", k.rechtecke);
    }

    /// `background-repeat: no-repeat` heisst **eine** Kachel, auch wenn
    /// sie kleiner ist als die Flaeche.
    #[test]
    fn kein_repeat_ist_genau_eine_kachel() {
        let mut g = geo();
        g.wiederholen_x = false;
        g.wiederholen_y = false;
        let k = kacheln(flaeche(), &g, (30.0, 10.0));
        assert_eq!(k.rechtecke.len(), 1, "{:?}", k.rechtecke);
    }

    /// NUR x wiederholt: eine Zeile, aber viele Spalten.
    #[test]
    fn nur_x_wiederholen_ergibt_eine_zeile() {
        let mut g = geo();
        g.wiederholen_y = false;
        let k = kacheln(flaeche(), &g, (20.0, 20.0));
        let ys: Vec<f32> = k.rechtecke.iter().map(|r| r.y).collect();
        let gleiche = ys.iter().all(|y| (*y - ys[0]).abs() < 0.01);
        assert!(gleiche, "alle Kacheln in einer Zeile: {ys:?}");
        assert!(k.rechtecke.len() >= 5, "aber viele: {}", k.rechtecke.len());
    }

    /// DIE GRENZE GREIFT: 2x2-Pixel auf 900x650 waeren 146 250 Kacheln.
    /// Es werden `MAX_KACHELN` gebaut und der Rest **gezaehlt**.
    #[test]
    fn die_obergrenze_greift_und_zaehlt_statt_zu_verbergen() {
        let f = Rect {
            x: 0.0,
            y: 0.0,
            width: 900.0,
            height: 650.0,
        };
        let k = kacheln(f, &geo(), (2.0, 2.0));
        assert!(
            k.rechtecke.len() <= MAX_KACHELN,
            "Grenze: {}",
            k.rechtecke.len()
        );
        assert!(
            k.verworfen > 0,
            "der Rest muss genannt werden: {}",
            k.verworfen
        );
        // Und die Zahl muss groesser sein als das, was gebaut wurde —
        // sonst haette man 146 250 Kacheln und 256 gebaut und behauptet,
        // es sei alles da.
        assert!(
            k.verworfen > k.rechtecke.len(),
            "verworfen={} gebaut={}",
            k.verworfen,
            k.rechtecke.len()
        );
        //
        // **DIE VOLLSTAENDIGE RECHNUNG**, weil eine davon falsch sein
        // koennte und keine der vier Zahlen sonst auftaucht:
        // 2x2 px auf 900x650 -> 451 x 326 roh = 147 026 Kacheln.
        let roh = 451 * 326;
        let gebaut = k.rechtecke.len() + k.weggeschnitten;
        assert_eq!(
            roh,
            gebaut + k.verworfen,
            "roh={roh} gebaut={gebaut} verworfen={}",
            k.verworfen
        );
    }

    /// Ein Bild **ohne Masse** (Dekodierfehler, SVG ohne Groesse) darf
    /// keine Kachelmasse ergeben, sonst teilt man durch 0 oder malt ein
    /// 0x0-Bild und nennt es ein Hintergrundbild.
    #[test]
    fn bild_ohne_masse_ergibt_nichts() {
        let k = kacheln(flaeche(), &geo(), (0.0, 0.0));
        assert!(k.rechtecke.is_empty(), "{:?}", k.rechtecke);
        assert_eq!(k.kachel, (0.0, 0.0));
    }

    /// Die Rechtecke liegen **alle** in der Flaeche — das ist die
    /// Zusicherung, auf die sich der Zeichner verlaesst.
    #[test]
    fn alle_rechtecke_liegen_in_der_flaeche() {
        let f = flaeche();
        let k = kacheln(f, &geo(), (7.0, 13.0));
        assert!(!k.rechtecke.is_empty());
        for r in &k.rechtecke {
            assert!(r.x >= f.x - 0.001 && r.y >= f.y - 0.001, "{r:?}");
            assert!(
                r.x + r.width <= f.x + f.width + 0.001 && r.y + r.height <= f.y + f.height + 0.001,
                "{r:?} verlaesst {f:?}"
            );
            assert!(r.width > 0.0 && r.height > 0.0, "leer: {r:?}");
        }
    }

    /// `cover` schlaegt die Laenge, weil es spaeter im Kaskadenblatt
    /// steht (specificoerer) — und `BildGeo` kann beides true haben.
    /// Der Test haelt die Entscheidung fest, damit sie nicht stillschweigend
    /// kippt.
    #[test]
    fn cover_gewinnt_gegen_die_lange() {
        let mut g = geo();
        g.cover = true;
        g.groesse = 5.0;
        let k = kacheln(flaeche(), &g, (40.0, 20.0));
        assert!(
            (k.kachel.0 - 100.0).abs() < 0.01,
            "cover, nicht 5 px: {}",
            k.kachel.0
        );
    }

    /// Kleiner Hilfsblock, damit `g_ohne_schritt` nicht wie eine
    /// Seiteneffekt-Funktion aussieht: `geo()` ist schon der
    /// Anfangswert, und der Test ruft sie nur, um es sichtbar zu machen.
    fn g_ohne_schritt(_g: &BildGeo) {}
}
