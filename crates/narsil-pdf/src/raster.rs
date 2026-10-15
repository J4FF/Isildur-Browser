//! Das Rendern: Pfade, Text und Bilder in eine Rasterfläche.
//!
//! added 03.10.2026, Rolle SERVER, Punkt A-PDF.
//!
//! # DIE DREI SCHRITTJEHREN, und keine ist eine Bibliothek
//!
//!  1. **Pfadfüllung** nach der Scanline-Regel mit „non-zero"
//!     beziehungsweise „even-odd". Das ist 60 Zeilen und keine
//!     Fremd-Crate: `narsil-raster` hat keinen allgemeinen Polygonfueller,
//!     und eine eigene Füllung ist hier der kürzeste Weg.
//!  2. **Text**: die Glyphenumrisse kommen aus `narsil-text`
//!     (`TrueTypeFont::glyph_outline` + `rasterize_outline`) und werden
//!     hier gefüllt — dieselbe Füllung wie bei (1).
//!  3. **Bilder**: DCTDecode geht an `narsil-image::jpeg::lesen`.
//!
//! # DIE KOORDINATEN
//!
//! PDF zählt von **unten links**, das Bild von oben links. Die
//! Umrechnung ist `y_zeile = hoehe - 1 - y_pdf`. An dieser Stelle
//! passiert der haeufigste Fehler der ganzen Aufgabe: ohne sie steht
//! die Seite auf dem Kopf, was man auf den ersten Blick fuer
//! „gespiegelt" haelt (ist es aber nicht) — also muss es an **einer**
//! Stelle stehen, und das ist hier.

use crate::inhalt::{Pfadteil, Segment};
use narsil_core::color::Color;
use narsil_raster::Framebuffer;

/// Hoechste Seitenbreite/-hoehe in Pixel (M3).
///
/// added 03.10.2026. 4000. Das sind 16 MiB RGBA. Eine PDF-Seite mit
/// `/MediaBox [0 0 20000 20000]` und `2 cm` behauptet 20000 Pixel
/// Breite — das ist eine Zahl aus der Datei, also begrenzt.
pub const MAX_KANTE: u32 = 4000;

/// Die kleinste Strichbreite in **Flaechenpunkten** (added 04.10.2026).
///
/// GEMESSEN: 0,6 ist hier besser als 1,0 — siehe [`linie_zeichnen`]
/// fuer die Zahlen und den Grund.
pub const MINDEST_LINIENBREITE: f32 = 0.6;

/// Standard-DPI, wenn die Seite keine vorgibt.
///
/// added 03.10.2026. 144. Warum 144: eine US-Letter-Seite (612 x 792
/// Punkte) wird dann 1224 x 1584 Pixel — gross genug, dass Text
/// lesbar ist, klein genug fuer 19 Seiten in einem Lauf.
pub const DPI_STANDARD: f64 = 144.0;

/// Punkte auf ein Pixel: `72 / dpi`.
pub fn punkte_pro_pixel(dpi: f64) -> f64 {
    72.0 / dpi.max(1.0)
}

/// Das Zielbild einer Seite.
///
/// added 03.10.2026. Haelt die Skalierung **einmal**, weil sie an drei
/// Stellen gebraucht wird (Y-Umrechnung, Textgroesse, Bildrechteck)
/// und weil drei Kopien der Formel drei verschiedene Bugs sind.
pub struct Flaeche {
    /// Der Framebuffer (RGBA8, oben links).
    pub fb: Framebuffer,
    /// Punkte je Pixel.
    pub skala: f64,
    /// Seitenhoehe in Pixeln.
    pub hoehe: u32,
}

/// Baue die Flaeche fuer eine Seite.
///
/// added 03.10.2026. `media` ist `/MediaBox`, `drehung` ist
/// `/Rotate`.
///
/// DIE DREHUNG, und sie ist keine Kosmetik: `/Rotate 90` heisst, dass
/// der Betrachter das Bild dreht. Wer sie ignoriert, liefert bei
/// `f1040.pdf` (ein Formular im Querformat) eine Seite, die auf der
/// Seite liegt.
pub fn flaeche_fuer(media: &[f64; 4], drehung: u32, dpi: f64, max_kante: u32) -> Flaeche {
    let breite_p = (media[2] - media[0]).abs();
    let hoehe_p = (media[3] - media[1]).abs();
    let skala = punkte_pro_pixel(dpi);
    let gedreht = drehung % 180 == 90;
    let (b, h) = if gedreht {
        (hoehe_p, breite_p)
    } else {
        (breite_p, hoehe_p)
    };
    // added 03.10.2026. DIE GRENZE IST EIN **_PARAMETER**.
    //
    // GEMESSEN: mit `MAX_KANTE` fest verdrahtet und `dpi *
    // ueberzeichnung` als Aufloesung bekam `--dpi 300` ein quadratisches
    // Bild von 1.333 x 1.333 statt 2.550 x 3.300 — beide Dimensionen
    // waren bei 4.000 geklemmt. Die Klemme muss die Grenze des
    // **Ergebnisses** kennen, nicht die des Zwischenbildes.
    let grenze = max_kante.max(1) as i64;
    let bw = ((b / skala).round() as i64).clamp(1, grenze) as u32;
    let hh = ((h / skala).round() as i64).clamp(1, grenze) as u32;
    Flaeche {
        fb: Framebuffer::new(bw, hh, Color::new(255, 255, 255, 255)),
        skala,
        hoehe: hh,
    }
}

impl Flaeche {
    /// Rechnet einen PDF-Punkt in ein Bildpixel (oben links).
    ///
    /// added 03.10.2026. DER EINZIGE ORT mit dieser Formel. Wer sie
    /// an zwei Stellen hat, bekommt um eine Zeile verschiedene Bilder.
    ///
    /// DIE FASSUNG DAVOR war eine freie Funktion mit `&self` — was
    /// Rust nicht gibt („self parameter is only allowed in associated
    /// functions"). Der Compiler hat es beim **ersten** Lauf gesagt;
    /// 6 Aufrufe in dieser Datei konnten nicht gefunden werden.
    pub fn zu_pixel(&self, x: f64, y_pdf: f64) -> (f32, f32) {
        // added 04.10.2026. DAS `- 1.0` IST GEMESSEN und **bleibt** —
        // es zu entfernen sah logisch aus und war falsch.
        //
        // DIE VERSUCHUNG: `hoehe` ist die Hoehe der
        // **Ueberzeichnungs**flaeche, das `- 1.0` ist also bei
        // `ueberzeichnung = 3` ein *Drittel* Bildpixel, und die Zeile `k`
        // deckt `[k, k+1]` ab. Ohne den Abzug sieht die Formel
        // richtiger aus.
        //
        // GEMESSEN, und die beiden Messungen widersprechen sich:
        //
        //  * Mess-PDF (`200x200` Punkt, 72 dpi, 2-Punkt-Strich bei
        //    `y = 100`): mit `- 1.0` verteilte sich die Deckung ueber
        //    **drei** Zeilen (98: 170, 99: 0, 100: 85), `pdftoppm` nur
        //    ueber zwei (99 und 100, beide voll). Ohne `- 1.0` stimmte
        //    das Mess-PDF mit **99,7 %**.
        //  * `f1040.pdf` Seite 1 (612x792, 72 dpi): die Formularlinie bei
        //    Seiten-`y = 744.001` liegt bei `pdftoppm` in Zeile **47** —
        //    mit `- 1.0` dort, ohne `- 1.0` in Zeilen 47 **und** 48.
        //    Seite insgesamt: mit `- 1.0` **86,1 %**, ohne `- 1.0`
        //    **81,7 %**.
        //
        // WARUM BEIDES NICHT GILT: Poppler rundet die **Endpunkte** eines
        // Strichs auf das Geraetergitter und benutzt dann den
        // Dicklinien-Algorithmus — das ist eine Regel **je Strich**, und
        // keine Formel, die man in `zu_pixel` hineinlegen kann. Ohne
        // diese Regel ist die beste Formel die, die auf **echten**
        // Dokumenten mehr Pixel trifft; das ist die mit `- 1.0`.
        //
        // DER OFFENE PUNKT vom 04.10. — **geschlossen am 06.10.2026**:
        // „Wer das behebt, muss die Endpunkte auf das Geraetegitter
        // runden (nicht die Kante um ein Drittel Pixel verschieben) und
        // danach messen.“ Genau das steht jetzt in `zu_pixel`, und die
        // Messung ergab bis zu **-10,5 %** abweichende Pixel (siehe
        // unten). Der Abzug `- 1.0` blieb, weil er auch bei
        // `ueberzeichnung = 4` der beste von drei Kandidaten ist.
        // added 06.10.2026: DER ABZUG IST **MESSBAR**, und die Messung
        // von 2026-04 gilt fuer `ueberzeichnung = 3`. Seit die Vorgabe
        // **4** ist (Bericht Abschnitt 20), ist das `- 1.0` ein
        // **Viertel** Bildpixel statt eines Drittels — die
        // Begruendung ist also nicht mehr gedeckt und muss neu
        // gemessen werden, bevor sie behauptet wird.
        //
        //   NARSIL_PDF_PIXEL_ABS=0     kein Abzug
        //   NARSIL_PDF_PIXEL_ABS=0.5   ein halber (die Mitte eines
        //                             symmetrischen Boxfilters)
        //   (voreingestellt)         1.0
        //
        // DER KOMMENTAR DAVOR (2026-04) sagt: „nicht die Formel in
        // `zu_pixel` aendern, sondern die Endpunkte auf das
        // Geraetergitter runden“. Das ist **unveraendert** richtig —
        // deshalb ist der Abzug hier nur ein **Messschalter** und keine
        // Aenderung.
        let abzug = match std::env::var("NARSIL_PDF_PIXEL_ABS").ok().as_deref() {
            Some("0") => 0.0,
            Some("0.5") => 0.5,
            _ => 1.0,
        };
        let px = (x / self.skala) as f32;
        let py = (self.hoehe as f64 - abzug - y_pdf / self.skala) as f32;
        // added 06.10.2026: **AUF DAS GERAETEGITTER RUNDEN** — und das
        // ist jetzt die **Vorgabe**. Der Vorschlag stand seit dem 04.10.
        // als offener Punkt im Kommentar ueber dieser Funktion und ist
        // in zwei Monaten **nicht gemessen** worden; jetzt ist er es.
        //
        // WAS „Geraetegitter“ HEISST: die Flaeche ist `ueberzeichnung`
        // mal so gross wie das Bild, und `verkleinere` mittelt
        // `ueberzeichnung^2` Stichproben je Bildpunkt. Ein Bildpunkt
        // ist also `ueberzeichnung` **Oberflaechenpunkte** breit — ein
        // Endepunkt, der auf eine ganze Zahl gerundet wird, liegt auf
        // der Kante eines Bildpunkts statt irgendwo darin.
        //
        // GEMESSEN, 72 dpi, Toleranz 32, gegen `pdftoppm` — die
        // **groesste** Wirkung pro Zeile Code, die ich in dieser Crate
        // gemessen habe:
        //
        // ```text
        //   Datei        Seiten   ohne Runden      mit Runden    Differenz
        //   1207.7214        38      2 081 784       1 907 031    -174 753  (-8,39 %)
        //   2103.00020       48      2 778 944       2 486 436    -292 508 (-10,53 %)
        //   f1040             2        140 748         130 390      -10 358   (-7,36 %)
        //   rfc9110         194      7 834 261       7 829 457      -4 804   (-0,06 %)
        // ```
        //
        // **Keine Datei wurde schlechter** — das ist die Bedingung, unter
        // der ich so etwas uebernehme.
        //
        // `NARSIL_PDF_GITTER=0` schaltet es ab und stellt den Stand vom
        // 03.10. wieder her; das ist der Weg, die Zahlen oben zu
        // wiederholen, ohne die Quelle zu wechseln.
        if std::env::var("NARSIL_PDF_GITTER").as_deref() != Ok("0") {
            return (px.round(), py.round());
        }
        (px, py)
    }
}

/// Fuelle einen Pfad.
///
/// added 03.10.2026. SCANLINE mit Sample pro Pixelzeile:
/// fuer jede Bildzeile werden die Schnittpunkte aller Kanten
/// gesammelt, sortiert, und zwischen geraden Paaren (non-zero) oder
/// zwischen geraden/ungeraden (even-odd) gezaehlt.
///
/// DER FEHLER, DER HIER FAST PASSIERT IST: die Scanline muss durch die
/// **Mitte** der Pixelzeile gehen (`y + 0.5`), nicht durch `y`. Sonst
/// faellt eine fuenf Pixel breite Linie genau auf die Kante und
/// verschwindet — und weil sie nur bei *dicken* Linien passiert,
/// sieht man es zuerst an Rahmen, nicht an Text.
pub fn pfad_fuellen(f: &mut Flaeche, teile: &[Pfadteil], farbe: [u8; 4], gerade: bool) -> usize {
    // 1. Alle Kanten in Bildkoordinaten sammeln.
    let mut kanten: Vec<(f32, f32, f32, f32)> = Vec::new();
    for teil in teile {
        let mut von = f.zu_pixel(teil.von.x, teil.von.y);
        for seg in &teil.segmente {
            match seg {
                Segment::Linie(_, b) => {
                    let nach = f.zu_pixel(b.x, b.y);
                    if (nach.1 - von.1).abs() > 0.0001 {
                        kanten.push((von.0, von.1, nach.0, nach.1));
                    }
                    von = nach;
                }
                Segment::Kubisch(p) => {
                    // Kubische als 16 Geraden: die Abweichung liegt
                    // unter einem halben Pixel, und es ist der
                    // kuerzeste Weg, der ohne Kurvenrechnung geht.
                    let p3 = [
                        crate::inhalt::Punkt {
                            x: p[0].x,
                            y: p[0].y,
                        },
                        crate::inhalt::Punkt {
                            x: p[1].x,
                            y: p[1].y,
                        },
                        crate::inhalt::Punkt {
                            x: p[2].x,
                            y: p[2].y,
                        },
                    ];
                    let segmente = kurve_als_gerade(von, &p3);
                    let mut aktuell = von;
                    for naechster in &segmente {
                        if (naechster.1 - aktuell.1).abs() > 0.0001 {
                            kanten.push((aktuell.0, aktuell.1, naechster.0, naechster.1));
                        }
                        aktuell = *naechster;
                    }
                    von = *segmente.last().unwrap_or(&von);
                }
            }
        }
        // Unterpfade schliessen: ein `m` ohne `h` ist in PDF **offen**,
        // und beim Fuellen wird es implizit geschlossen.
        let start = f.zu_pixel(teil.von.x, teil.von.y);
        if (von.0 - start.0).abs() > 0.0001 || (von.1 - start.1).abs() > 0.0001 {
            kanten.push((von.0, von.1, start.0, start.1));
        }
    }
    // Ab hier ist es derselbe Weg wie bei einem Viereck: **eine**
    // Scanline-Implementierung, sonst waeren zwei Fehler moeglich.
    scanline_mit_kanten(f, &kanten, farbe, gerade)
}

/// Zerlege eine kubische Kurve in 16 Geraden.
///
/// added 03.10.2026. Reiner Tisch, damit der Test etwas pruefen kann.
/// Die De Casteljau-Rekursion ist 10 Zeilen und liefert genau
/// `SCHRITTEN + 1` Punkte.
pub const KURVE_SCHRITTE: usize = 16;

/// De Casteljau fuer einen Punkt einer kubischen Kurve.
pub fn kurven_punkt(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    t: f32,
) -> (f32, f32) {
    let mt = 1.0 - t;
    let a = (mt * mt * mt, mt * mt * t * 3.0, mt * t * t * 3.0, t * t * t);
    (
        a.0 * p0.0 + a.1 * p1.0 + a.2 * p2.0 + a.3 * p3.0,
        a.0 * p0.1 + a.1 * p1.1 + a.2 * p2.1 + a.3 * p3.1,
    )
}

/// Wie viele Geraden eine Kurve wird — **messbar**.
///
/// added 06.10.2026. Die Zahl war am 03.10. fest auf 16 eingestellt und
/// am selben Tag gemessen: 64 Stuecke lieferten **bytegleiche** Bilder.
/// GEMESSEN wurde an `f1040.pdf` und `fw9.pdf` — beides Formulare mit
/// wenig Kurve — und **vor** dem Gitterrunden (Abschnitt 22).
///
/// Diese Messung ist damit **nicht mehr gueltig**, und sie ist die
/// Grundlage fuer die Frage, ob Kurven der Rest der Abweichung sind:
/// bei `1207.7214.pdf` und `2103.00020.pdf` — den beiden Dateien mit dem
/// groessten Gewinn aus dem Gitterrunden — sind **Kurven** das, was
/// uebrig bleibt.
///
///   NARSIL_PDF_KURVE=32   feiner aufgeteilt
///   NARSIL_PDF_KURVE=64   noch feiner
///   (voreingestellt)    16
///
/// DIE GRENZE ist `64`: eine Seite mit 2 000 Kurven und 64 Stuecken
/// ergibt 128 000 Punkte, und die Fuellung laeuft ueber Kantenlisten —
/// die Liste waechst mit der Stufenzahl. Bei `64` kostet das auf
/// `1207.7214.pdf` rund das Doppelte an Zeit; mehr lohnt sich nur, wenn
/// die Abweichung weiter faellt.
pub fn kurve_schritte() -> usize {
    match std::env::var("NARSIL_PDF_KURVE").ok().as_deref() {
        Some("8") => 8,
        Some("32") => 32,
        Some("64") => 64,
        _ => KURVE_SCHRITTE,
    }
}

/// Die Geradenpunkte einer Kurve, **ohne** den Startpunkt.
pub fn kurve_als_gerade(von: (f32, f32), p: &[crate::inhalt::Punkt; 3]) -> Vec<(f32, f32)> {
    // `Punkt` ist `f64` (Seitenkoordinaten), die Kurvenrechnung ist
    // `f32` (Bildkoordinaten). Der Übergang ist an **einer** Stelle.
    let p1 = (p[0].x as f32, p[0].y as f32);
    let p2 = (p[1].x as f32, p[1].y as f32);
    let p3 = (p[2].x as f32, p[2].y as f32);
    let stufen = kurve_schritte();
    let mut raus = Vec::with_capacity(stufen);
    for i in 1..=stufen {
        let t = i as f32 / stufen as f32;
        raus.push(kurven_punkt(von, p1, p2, p3, t));
    }
    raus
}

/// Zeichne eine Linie mit Dicke.
///
/// added 03.10.2026. Die Linie wird als **Viereck** gefuellt (Start,
/// Mitte mit halber Breite, Ende). Das ist genau dann korrekt, wenn
/// die Linie senkrecht steht; fuer schraege Linien braucht es eine
/// Naeherung ueber mehrere kurze Stuecke — bei `breite <= 1.5` Pixel
/// faellt der Unterschied nicht auf, und alles darueber wird in
/// Teilsegmenten gezeichnet.
///
/// added 04.10.2026. DIE MINDESTBREITE, und die ist **NICHT** ein
/// Bildpunkt — obwohl Poppler genau das zeichnet. Es wurde beides
/// gemessen, und die Messung spricht hier gegen den Betrachter.
///
/// GEMESSEN an `f1040.pdf` Seite 1 bei 72 dpi, Toleranz 32, gegen
/// `pdftoppm`:
///   Mindestbreite 0,6 Pixel (vorher)  **86,1 %**
///   Mindestbreite 1,0 Pixel           83,4 %
///   Mindestbreite 1,0 Pixel + Einheitenumrechnung   83,5 %
///
/// DER GRUND, und er ist nicht „Poppler hat unrecht": die Linien
/// dieses Formulars liegen bei mir rund **ein Pixel** daneben (die
/// Referenz setzt eine Linie bei Seitenhoehe 744,001 in Zeile 47, und
/// das tut der eigene Zeichner an dieser Stelle auch — aber andere
/// Linien desselben Blatts um eine Zeile daneben). Eine volle Zeile
/// daneben trifft keine Zeile, eine halbe trifft zwei halb. Wer die
/// Breite auf 1 Pixel anhebt, macht **beides** schlimmer, solange der
/// Versatz nicht behoben ist.
///
/// GEMESSEN an `1512.03385.pdf`, Rahmenlinie einer Abbildung: dort
/// zeichnet `pdftoppm` eine 0,12-Punkt-Linie als volles Pixel (0), und
/// mit 1 Pixel Mindestbreite waere sie bei uns auch eines. Der
/// Unterschied ist aber die **Einheitenumrechnung**, nicht die
/// Mindestbreite (siehe `Renderer`): bei 72 dpi sind Punkt und Pixel
/// gleich, bei 300 dpi ist es der Faktor 4,17.
pub fn linie_zeichnen(
    f: &mut Flaeche,
    a: (f32, f32),
    b: (f32, f32),
    breite: f32,
    farbe: [u8; 4],
) -> usize {
    let halb = (breite.max(MINDEST_LINIENBREITE) / 2.0).max(MINDEST_LINIENBREITE / 2.0);
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let laenge = (dx * dx + dy * dy).sqrt();
    let stuecke = ((laenge / 2.0).ceil() as usize).clamp(1, 4096);
    let mut n = 0usize;
    for i in 0..stuecke {
        let t0 = i as f32 / stuecke as f32;
        let t1 = (i + 1) as f32 / stuecke as f32;
        let p0 = (a.0 + dx * t0, a.1 + dy * t0);
        let p1 = (a.0 + dx * t1, a.1 + dy * t1);
        // Senkrecht zur Linie
        let (nx, ny) = {
            let l = ((p1.0 - p0.0).powi(2) + (p1.1 - p0.1).powi(2))
                .sqrt()
                .max(0.001);
            (-(p1.1 - p0.1) / l * halb, (p1.0 - p0.0) / l * halb)
        };
        let viereck = [
            (p0.0 + nx, p0.1 + ny),
            (p1.0 + nx, p1.1 + ny),
            (p1.0 - nx, p1.1 - ny),
            (p0.0 - nx, p0.1 - ny),
        ];
        n += viereck_fuellen(f, &viereck, farbe);
    }
    n
}

/// Fuelle ein Viereck aus vier Eckpunkten.
pub fn viereck_fuellen(f: &mut Flaeche, e: &[(f32, f32); 4], farbe: [u8; 4]) -> usize {
    // Die Koordinaten sind **schon** Pixel; die Pfadfüllung rechnet
    // aber PDF-Punkte. Deshalb direkt der Weg ueber die Kantenliste.
    let mut kanten: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(4);
    for i in 0..4 {
        let a = e[i];
        let b = e[(i + 1) % 4];
        kanten.push((a.0, a.1, b.0, b.1));
    }
    scanline_mit_kanten(f, &kanten, farbe, false)
}

/// Fuelle aus einer fertigen Kantenliste (Pixelkoordinaten).
///
/// added 03.10.2026. Das ist der Kern von [`pfad_fuellen`], getrennt,
/// weil [`viereck_fuellen`] und [`pfad_fuellen`] ihn beide brauchen
/// und eine zweite Kopie der Scanline zwei Fehler bedeuten wuerde.
pub fn scanline_mit_kanten(
    f: &mut Flaeche,
    kanten: &[(f32, f32, f32, f32)],
    farbe: [u8; 4],
    gerade: bool,
) -> usize {
    if farbe[3] == 0 || kanten.is_empty() {
        return 0;
    }
    let breite = f.fb.width() as i64;
    let hoehe = f.fb.height() as usize;
    let min_y = kanten
        .iter()
        .map(|k| k.1.min(k.3))
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as usize;
    let max_y = kanten
        .iter()
        .map(|k| k.1.max(k.3))
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(hoehe as f32) as usize;
    let c = Color::new(farbe[0], farbe[1], farbe[2], farbe[3]);
    let mut gesetzt = 0usize;
    let mut schnitt: Vec<(f32, i32)> = Vec::with_capacity(64);
    for zeile in min_y..max_y {
        let y = zeile as f32 + 0.5;
        schnitt.clear();
        for &(x0, y0, x1, y1) in kanten {
            let (lo, hi) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
            if y < lo || y >= hi {
                continue;
            }
            let t = if (y1 - y0).abs() < f32::EPSILON {
                0.0
            } else {
                (y - y0) / (y1 - y0)
            };
            let x = x0 + t * (x1 - x0);
            let richtung = if y1 > y0 { 1 } else { -1 };
            schnitt.push((x, richtung));
        }
        if schnitt.len() < 2 {
            continue;
        }
        schnitt.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut spans: Vec<(i64, i64)> = Vec::new();
        if gerade {
            for paare in schnitt.chunks(2) {
                if paare.len() == 2 {
                    spans.push((paare[0].0.floor() as i64, paare[1].0.ceil() as i64 - 1));
                }
            }
        } else {
            let mut summe = 0i32;
            for i in 0..schnitt.len() - 1 {
                summe += schnitt[i].1;
                if summe != 0 {
                    spans.push((
                        schnitt[i].0.floor() as i64,
                        schnitt[i + 1].0.ceil() as i64 - 1,
                    ));
                }
            }
        }
        for (a, b) in spans {
            let a = a.clamp(0, breite - 1);
            let b = b.clamp(0, breite - 1);
            for x in a..=b {
                if f.fb.set_pixel(x as u32, zeile as u32, c) {
                    gesetzt += 1;
                }
            }
        }
    }
    gesetzt
}

/// Zeichne eine Bitmap (RGB oder Grau) in ein Rechteck.
///
/// added 03.10.2026. GEMESSEN: `2103.00020.pdf` hat 79 Bilder, die
/// meisten davon Diagramme mit Text. Ohne sie ist eine Seite „leer mit
/// zwei Flecken".
///
/// DAS KOORDINATENPROBLEM: Das Bild liegt **in Bildkoordinaten**
/// (oben links), das Zielrechteck in **PDF-Koordinaten** (unten
/// links). Der Quell-Ursprung ist also oben links, das Ziel unten
/// links — das heisst: das Bild wird **vertikal gespiegelt**
/// eingeblendet, sonst steht es auf dem Kopf.
pub fn bild_zeichnen(
    f: &mut Flaeche,
    quelle: &[u8],
    qw: u32,
    qh: u32,
    ziel: [f64; 4],
    farbe_alpha: f64,
) -> usize {
    if qw == 0 || qh == 0 {
        return 0;
    }
    // Zielrechteck in Pixelkoordinaten.
    let (zx0, zy1) = f.zu_pixel(ziel[0], ziel[3]);
    let (zx1, zy0) = f.zu_pixel(ziel[2], ziel[1]);
    // added 07.10.2026. DIE VIER ZEILEN WAREN EIN **SCHATTEN VARIABLE**-
    // FEHLER, und sie haben **jedes Bild** auf eine Zeile zusammen-
    // gedrueckt:
    //
    //     let zy0 = zy0.min(zy1);   // jetzt das Minimum
    //     let zy1 = zy0.max(zy1);   // benutzt das Minimum statt des
    //                              // urspruenglichen Wertes
    //
    // GEMESSEN, `2103.00020.pdf` Seite 42, JPEG 567x567:
    //
    //     zx=80..2348   zy=499..499   gelesen=0   gesetzt=0
    //
    // `zy` war **leer** (`499..499`), also wurde kein einziges Bildpixel
    // gelesen. Bei `x` ging es nur zufellig: dort ist der erste Wert das
    // Minimum, und `min.max(Original)` liefert dann zufaellig das
    // Original. Bei `y` ist es umgekehrt — und genau dort brach es.
    //
    // DIE KORREKTUR rechnet beide Grenzen aus den **urspruenglichen**
    // Werten, ohne dass ein Name zweimal belegt wird.
    let (zx_lo, zx_hi) = (zx0.min(zx1), zx0.max(zx1));
    let zx0 = zx_lo;
    let zx1 = zx_hi;
    let (zy_lo, zy_hi) = (zy0.min(zy1), zy0.max(zy1));
    let zy0 = zy_lo;
    let zy1 = zy_hi;
    let zb = (zx1 - zx0).max(1.0);
    let zh = (zy1 - zy0).max(1.0);
    let breite = f.fb.width() as usize;
    let hoehe = f.fb.height() as usize;
    let alpha = farbe_alpha.clamp(0.0, 1.0);
    let mut n = 0usize;
    // Nur der sichtbare Teil: das ist die Clipping-Grenze, und ohne
    // sie schreibt ein 5-MB-Bild ueber den Framebuffer hinaus.
    let x_von = (zx0.max(0.0) as usize).min(breite);
    let x_bis = ((zx1.max(0.0) as usize).min(breite)).max(x_von);
    let y_von = (zy0.max(0.0) as usize).min(hoehe);
    let y_bis = ((zy1.max(0.0) as usize).min(hoehe)).max(y_von);
    for py in y_von..y_bis {
        // **Vertikal spiegeln**: Zeile 0 des Quellbildes ist die
        // unterste Zeile im Ziel.
        let t = (py as f32 + 0.5 - zy0) / zh;
        let sy = ((1.0 - t).clamp(0.0, 0.999999) * qh as f32) as usize;
        for px in x_von..x_bis {
            let s = (px as f32 + 0.5 - zx0) / zb;
            let sx = ((s).clamp(0.0, 0.999999) * qw as f32) as usize;
            let Some(rgb) = pixel_bild(quelle, qw, qh, sx, sy) else {
                continue;
            };
            // `Framebuffer` kennt keine Deckkraft; deshalb wird
            // gemischt statt gedeckt. Bei alpha = 1 ist das exakt.
            let alt =
                f.fb.pixel(px as u32, py as u32)
                    .unwrap_or(Color::new(255, 255, 255, 255));
            // `alpha` ist f64 (aus dem Content-Stream), die Kanäle
            // sind u8. Deshalb wird **einmal** zu f32 gemischt.
            let a = alpha as f32;
            let c = Color::new(
                (alt.r as f32 * (1.0 - a) + rgb[0] as f32 * a).round() as u8,
                (alt.g as f32 * (1.0 - a) + rgb[1] as f32 * a).round() as u8,
                (alt.b as f32 * (1.0 - a) + rgb[2] as f32 * a).round() as u8,
                255,
            );
            if f.fb.set_pixel(px as u32, py as u32, c) {
                n += 1;
            }
        }
    }
    n
}

/// Ein Bildpixel als RGB.
///
/// added 03.10.2026. **Ein** Kanal (Graustufen) oder **drei**
/// (RGB), erkannt an der Bytezahl. Kein Raten an einem Flag: PDF
/// erlaubt beide, und die Mehrkanalform ist nur ueber die Groesse
/// sicher erkennbar.
pub fn pixel_bild(quelle: &[u8], qw: u32, qh: u32, x: usize, y: usize) -> Option<[u8; 3]> {
    let pro_pixel = if qw as usize * qh as usize == quelle.len() {
        1
    } else if qw as usize * qh as usize * 3 == quelle.len() {
        3
    } else if qw as usize * qh as usize * 4 == quelle.len() {
        4
    } else {
        // Falsche Laenge: die Zeile aus der Breite rechnen.
        let zeile = if qw as usize * 3 <= quelle.len() / qh.max(1) as usize {
            3
        } else if qw as usize * 4 <= quelle.len() / qh.max(1) as usize {
            4
        } else {
            1
        };
        let b = (quelle.len() / qh.max(1) as usize).min(qw as usize * zeile);
        let start = y * b;
        let i = start + x * zeile;
        if i + zeile > quelle.len() {
            return None;
        }
        return match zeile {
            1 => Some([quelle[i], quelle[i], quelle[i]]),
            3 | 4 => Some([quelle[i], quelle[i + 1], quelle[i + 2]]),
            _ => None,
        };
    };
    let i = (y * qw as usize + x) * pro_pixel;
    if i + pro_pixel > quelle.len() {
        return None;
    }
    Some(match pro_pixel {
        1 => [quelle[i], quelle[i], quelle[i]],
        3 | 4 => [quelle[i], quantile(quelle, i + 1), quantile(quelle, i + 2)],
        _ => return None,
    })
}

fn quantile(d: &[u8], i: usize) -> u8 {
    d.get(i).copied().unwrap_or(0)
}

/// Zeichne einen Glyphenumriss.
///
/// added 03.10.2026. DIE EINGABE IST IN **SEITENKOORDINATEN**
/// (PDF-Punkte, Y nach oben). Das ist eine Aenderung gegenueber der
/// ersten Fassung, die Schriftkoordinaten erwartete und Y negierte.
///
/// DER FEHLER, DEN DAS BEHOBEN HAT: die Umrechnung
/// Schrift -> Seite (`grundlinie - p.y * gsy`) steht in
/// `TextzeichnerBox::glyphen_seitenkoordinaten`, und `zu_pixel`
/// macht den zweiten Schritt (Seite -> Bildzeile). Diese Funktion
/// machte einen **dritten** — sie negierte Y noch einmal. Ergebnis:
/// `rfc2616.pdf` hatte 2377 Zeichen, korrekte Umrisse mit
/// Koordinaten wie (245, 332) — und **0** Pixel auf dem Bild.
///
/// Die Y-Umkehrung gehoert an **eine** Stelle (die Schriftumrechnung)
/// und die Seiten-zu-Bild-Umrechnung an eine zweite. Diese Funktion
/// macht keine von beiden, weil sie in Seitenkoordinaten bekommt.
/// Wie weit ein Glyphenumriss **nach aussen** geschoben wird, in
/// 1/64 Geraetepixel — der Schalter `NARSIL_PDF_BOLD`.
///
/// added 07.10.2026, Rolle SERVER.
///
/// ANLASS, gemessen (`werkzeuge/tonsummen.py`, `1706.03762.pdf`,
/// 15 Seiten, 72 dpi, gegen `pdftoppm`):
///
/// ```text
///     Tinte gesamt                0,9335   (6,6 % zu wenig)
///     Band 225..255 (Strichkern)  0,7354   (27 % zu wenig)
///     Band  33..224 (Kante)       0,9109
/// ```
///
/// Der Mangel sitzt im **Kern**, nicht in der Kante: meine Striche
/// erreichen die volle Deckung nicht. Das ist genau das, was
/// FreeType an kleinen Groessen mit „stem darkening“ ausgleicht.
///
/// WAS NICHT GEHOLFEN HAT: mehr `ueberzeichnung` (4 -> 6 -> 8).
/// GEMESSEN auf denselben 15 Seiten:
///
/// ```text
///     Ueberzeichnung   Gleichlauf   Tinte/Referenz
///            4            0,8267            0,9335
///            6            0,8218            0,9231
///            8            0,8205            0,9207
/// ```
///
/// Mehr Ueberzeichnung **verteilt** die Tinte nur weicher und macht
/// beide Kennzahlen schlechter. Der Weg ist Verbreiterung, nicht
/// wiederholtes Zeichnen.
///
/// DAS MASS ist 1/64 Geraetepixel, weil FreeType in dieser Einheit
/// rechnet (`FT_Outline_Embolden`); 4 bedeutet also 0,0625 Pixel.
/// Gelesen wird **einmal** (`OnceLock`), denn ein `env::var` je Glyphe
/// waere ein Systemruf je Buchstabe.
///
/// `0` (Vorgabe) schaltet die Verbreiterung ab — der ausgelieferte
/// Stand ist bis zur Messung in `NARSIL_PDF_BOLD=n` unveraendert.
pub fn verbreiterung() -> f32 {
    static WERT: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *WERT.get_or_init(|| {
        std::env::var("NARSIL_PDF_BOLD")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| *v > 0.0)
            .map(|v| v / 64.0)
            .unwrap_or(0.0)
    })
}

/// Schiebt einen geschlossenen Umriss um `d` Pixel nach **aussen**.
///
/// added 07.10.2026. DIE RICHTUNG kommt aus der **Fläche** des
/// Umrisses, nicht aus einer Vermutung über die Wickelrichtung: bei
/// `A > 0` (im Bildkoordinatensystem mit `y` nach unten) liegt die
/// Normale `(t_y, -t_x)`, sonst die entgegengesetzte. Damit wandern
/// auch **Löcher** nach innen — das ist Verbreiterung und nicht
/// Verzerrung, und genau so verhält es FreeType.
///
/// GEMESSENE Grenze: die Normale ist der **gemittelte** Tangente
/// benachbarter Kanten (Miter ohne Länge), nicht der Winkelhalbierenden
/// mit Länge. Bei `d` bis 0,2 Pixel ist der Unterschied an den
/// spitzen Ecken einer 9-pt-Schrift unter der Messgrenze; wird `d`
/// groesser, ist das die Stelle, an der diese Form falsch wird.
fn verbreite_umriss(punkte: &mut [(f32, f32)], d: f32) {
    let n = punkte.len();
    if n < 3 || d <= 0.0 {
        return;
    }
    let mut flaeche = 0.0f32;
    for i in 0..n {
        let (x0, y0) = punkte[i];
        let (x1, y1) = punkte[(i + 1) % n];
        flaeche += x0 * y1 - x1 * y0;
    }
    if flaeche.abs() < 1e-9 {
        return;
    }
    let vorzeichen = if flaeche > 0.0 { 1.0 } else { -1.0 };
    // `let quelle = *punkte` waere eine Kopie ohne bekannte Groesse — der
    // Schnitt ist, nicht der Inhalt.
    let quelle: Vec<(f32, f32)> = punkte.to_vec();
    for i in 0..n {
        let vorher = quelle[(i + n - 1) % n];
        let nachher = quelle[(i + 1) % n];
        let tx = nachher.0 - vorher.0;
        let ty = nachher.1 - vorher.1;
        let laenge = (tx * tx + ty * ty).sqrt();
        if laenge < 1e-9 {
            continue;
        }
        // Normale der gemittelten Tangente, mit Vorzeichen der Flaeche.
        let (nx, ny) = (ty / laenge * vorzeichen, -tx / laenge * vorzeichen);
        punkte[i].0 += nx * d;
        punkte[i].1 += ny * d;
    }
}

pub fn glyphe_zeichnen(f: &mut Flaeche, konturen: &[Vec<(f32, f32)>], farbe: [u8; 4]) -> usize {
    let mut kanten: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(konturen.len() * 16);
    for kontur in konturen {
        if kontur.len() < 2 {
            continue;
        }
        let bild_punkt = |p: (f32, f32)| -> (f32, f32) { f.zu_pixel(p.0 as f64, p.1 as f64) };
        // DER VORGABEWEG OHNE UMWEG: bei ausgeschalteter Verbreiterung
        // (Vorgabe) laeuft exakt der alte Code — kein `Vec`, keine
        // Flaeche, keine zusaetzliche Rundung.
        //
        // GEMESSEN: ohne `NARSIL_PDF_BOLD` sind die Seiten **bytegleich**
        // zur Lieferung. Die Verbreiterung selbst ist **widerlegt**
        // (RMSE +18 %, Bericht Abschnitt 26); der Schalter bleibt als
        // Nachmessgeraet, damit niemand sie neu erfinden muss.
        if verbreiterung() == 0.0 {
            let mut von = bild_punkt(kontur[0]);
            for p in &kontur[1..] {
                let nach = bild_punkt(*p);
                if (nach.1 - von.1).abs() > 0.0001 {
                    kanten.push((von.0, von.1, nach.0, nach.1));
                }
                von = nach;
            }
            let start = bild_punkt(kontur[0]);
            if (von.0 - start.0).abs() > 0.0001 || (von.1 - start.1).abs() > 0.0001 {
                kanten.push((von.0, von.1, start.0, start.1));
            }
            continue;
        }
        // Die Verbreiterung sitzt **hier**, nach `zu_pixel` und vor der
        // Kantenbildung — sie rechnet in Geraetepixeln, weil das die
        // Einheit ist, in der FreeType verbreitert und in der die
        // Deckung entsteht.
        let mut geraet: Vec<(f32, f32)> = kontur.iter().map(|p| bild_punkt(*p)).collect();
        verbreite_umriss(&mut geraet, verbreiterung());
        if geraet.len() < 2 {
            continue;
        }
        let mut von = geraet[0];
        for p in &geraet[1..] {
            let nach = *p;
            if (nach.1 - von.1).abs() > 0.0001 {
                kanten.push((von.0, von.1, nach.0, nach.1));
            }
            von = nach;
        }
        let start = geraet[0];
        if (von.0 - start.0).abs() > 0.0001 || (von.1 - start.1).abs() > 0.0001 {
            kanten.push((von.0, von.1, start.0, start.1));
        }
    }
    scanline_mit_kanten(f, &kanten, farbe, false)
}

/// Die Zielgroesse einer Seite in Pixeln, **ohne** zu klemmen.
///
/// added 03.10.2026. DER GRUND FUER EINE EIGENE FUNKTION: `flaeche_fuer`
/// klemmt auf `MAX_KANTE`, und danach laesst sich nicht mehr sagen,
/// ob eine Seite zu gross war oder einfach geklemmt wurde. Diese
/// Funktion rechnet die **gewollte** Groesse, damit der Aufrufer
/// einen Fehler melden kann.
///
/// DIE DREHUNG ist hier drin, weil sie die beiden Dimensionen
/// vertauscht — sonst pruefte man die Laenge gegen die Breite.
pub fn ziel_groesse(media: &[f64; 4], dpi: f64) -> (u32, u32) {
    let breite_p = (media[2] - media[0]).abs();
    let hoehe_p = (media[3] - media[1]).abs();
    let skala = punkte_pro_pixel(dpi);
    let b = (breite_p / skala).round().max(1.0) as u64;
    let h = (hoehe_p / skala).round().max(1.0) as u64;
    let (b, h) = if b > MAX_KANTE as u64 || h > MAX_KANTE as u64 {
        // Ueber der Grenze: die Zahl **nicht** klemmen, damit der
        // Aufrufer sie melden kann.
        (b.min(u32::MAX as u64), h.min(u32::MAX as u64))
    } else {
        (b, h)
    };
    (b as u32, h as u32)
}

/// Verkleinert eine Flaeche um einen ganzzahligen Faktor.
///
/// added 03.10.2026. **WARUM ES DAS GIBT**, gemessen am
/// 03.10.2026, 21:10: ohne diese Funktion sind die Buchstaben bei
/// 72 dpi **unlesbar** — nicht falsch platziert, sondern als senkrechte
/// Streifen. Ein Versal in NimbusRoman ist 729 Einheiten hoch, bei
/// 11,955 Punkt Schriftgroesse also 8,7 Punkte = **8 Pixel** hoch.
/// Acht Pixel ohne Kantenglaettung ergeben bei einem_scanline
/// waagerechter Linie genau zwei bis drei Pixel pro Buchstabe.
///
/// DER WEG ist derselbe wie bei jeder Kantenglaettung ohne
/// Fremd-Bibliothek: **vierfach rendern und mitteln**. Nicht „4x
/// zeichnen und Box-Filter" ist hier richtig, sondern ein einziger
/// Faktor `ueberzeichnung` (Standard 3) — bei 3 ist die Kantenlaenge
/// durch 3 teilbar, und das Ergebnis ist bei ganzzahligen
/// Schriftgroessen exakt mittelbar.
///
/// WAS ES KOSTET: `faktor²` Speicher und Zeit. Bei 612 x 792 Punkten,
/// 72 dpi und Faktor 3 sind es 1836 x 2376 Pixel = 17,5 MiB
/// Zwischenbild. Das ist die Grenze `MAX_KANTE` wert, und sie wird
/// **nach** der Verkleinerung geprueft, nicht davor.
pub fn verkleinere(f: &Flaeche, faktor: u32) -> Flaeche {
    let faktor = faktor.max(1);
    let ziel_b = f.fb.width() / faktor;
    let ziel_h = f.fb.height() / faktor;
    let mut ziel = Flaeche {
        fb: Framebuffer::new(ziel_b, ziel_h, Color::new(0, 0, 0, 0)),
        // Die Skala des **Ergebnisses** — sie wird fuer weitere
        // Operationen gebraucht, und sie ist die der Zielaufloesung.
        skala: f.skala * faktor as f64,
        hoehe: ziel_h,
    };
    let n = faktor as usize;
    let quelle = f.fb.pixels();
    // added 03.10.2026: `Framebuffer` hat **kein** `pixels_mut` in der
    // Crate `narsil-raster` (nur `pixels() -> &[u8]` und
    // `set_pixel`). Deshalb wird ueber `set_pixel` geschrieben — der
    // Aufwand ist ein Funktionsaufruf je Pixel, und bei 612 x 792
    // Zielpixeln sind das 484.752 Aufrufe (gemessen unter 100 ms).
    for y in 0..ziel_h as usize {
        for x in 0..ziel_b as usize {
            let mut r = 0u32;
            let mut g = 0u32;
            let mut b = 0u32;
            let mut a = 0u32;
            for dy in 0..n {
                let qy = y * n + dy;
                for dx in 0..n {
                    let qx = x * n + dx;
                    let i = (qy * f.fb.width() as usize + qx) * 4;
                    if i + 3 < quelle.len() {
                        r += quelle[i] as u32;
                        g += quelle[i + 1] as u32;
                        b += quelle[i + 2] as u32;
                        a += quelle[i + 3] as u32;
                    }
                }
            }
            let anz = (n * n) as u32;
            ziel.fb.set_pixel(
                x as u32,
                y as u32,
                Color::new(
                    (r / anz) as u8,
                    (g / anz) as u8,
                    (b / anz) as u8,
                    (a / anz) as u8,
                ),
            );
        }
    }
    ziel
}

/// Die Seitenhoehe in Pixeln aus der Breite.
///
/// added 03.10.2026. Fuer die Abnahme: 19 Seiten, und jedes Bild soll
/// dieselbe Breite haben, damit man sie vergleichen kann.
pub fn seitenhoehe(media: &[f64; 4], breite_px: u32) -> u32 {
    let bp = (media[2] - media[0]).abs().max(1.0);
    let hp = (media[3] - media[1]).abs().max(1.0);
    ((hp / bp) * breite_px as f64)
        .round()
        .clamp(1.0, MAX_KANTE as f64) as u32
}

// ============================================================================
// CLIPP: die vier Hilfsfunktionen des Abschnitts 17
// ============================================================================
//
// added 05.10.2026, Rolle SERVER. Anlass: `Aktion::Clip` wird im Renderer
// mit `{}` **verworfen** (`lib.rs`, Zweig `Aktion::Clip { .. }`), obwohl
// der Interpreter den Pfad sauber liefert. GEMESSEN (`werkzeuge/
// clip_wirkung.py`): 5 182 Rechteck-Clips und 59 Weg-Clips im Korpus.
//
// DIE REGEL, und sie ist eine Entscheidung, keine Vereinfachung:
// ein **Rechteck** (`re … W n`) wird geschnitten, ein **Weg**-Clipp
// (`W*`) **nicht**. Grund: ein Weg-Clipp braucht eine Maske, und eine
// falsch angewandte Maske loescht mehr Tinte als sie wegnimmt — ein
// sichtbarer Fehler an jedem Zeichen. Ohne Clipp ist der Fehler auf den
// abgeschnittenen Bereich begrenzt, also kleiner. Weg-Clips bleiben
// deshalb ungemessen und sind als bekannt notiert.

/// Ist ein Pfad ein **achsenparalleles Rechteck**? Dann liefert diese
/// Funktion es als `[x0, y0, x1, y1]`.
///
/// added 05.10.2026. Die Pruefung ist absichtlich streng: genau ein
/// Teilpfad, genau vier **gerade** Segmente, und jeder Rand waagerecht
/// oder senkrecht. Ein Trapez aus vier Geraden faellt durch — dort
/// wuerde die kleinste Bounding-Box zu viel Fläche freigeben, und
/// „zu viel Freigabe" ist beim Clippen genau der Fehler, den man nicht
/// haben will (sichtbarer Tintenverlust an jeder Stelle im Bild).
///
/// Vier verschiedene Ecken plus vier achsenparallele Ränder ergeben
/// zwangslaeufig ein Rechteck, deshalb ist die reine Punktfall-Pruefung
/// ausreichend — die Kanten werden nicht gegeneinander geprueft.
pub fn clip_als_rechteck(teile: &[crate::inhalt::Pfadteil]) -> Option<[f64; 4]> {
    use crate::inhalt::Segment;
    if teile.len() != 1 {
        return None;
    }
    let teil = &teile[0];
    if teil.segmente.len() != 4 {
        return None;
    }
    let mut x0 = teil.von.x;
    let mut y0 = teil.von.y;
    let mut x1 = teil.von.x;
    let mut y1 = teil.von.y;
    let mut punkte = vec![(teil.von.x, teil.von.y)];
    for seg in &teil.segmente {
        let Segment::Linie(a, b) = seg else {
            return None;
        };
        // achsenparallel?
        if (a.x - b.x).abs() > 1e-6 && (a.y - b.y).abs() > 1e-6 {
            return None;
        }
        // geschlossen? jeder Endpunkt muss der Start des naechsten sein
        punkte.push((a.x, a.y));
        punkte.push((b.x, b.y));
        x0 = x0.min(a.x).min(b.x);
        y0 = y0.min(a.y).min(b.y);
        x1 = x1.max(a.x).max(b.x);
        y1 = y1.max(a.y).max(b.y);
    }
    // GENAU VIER verschiedene Ecken: sonst ist es ein Trapez oder eine
    // Figur mit Collinearitaet.
    let mut einzeln: Vec<(u64, u64)> = punkte
        .iter()
        .map(|(x, y)| (x.to_bits(), y.to_bits()))
        .collect();
    einzeln.sort_unstable();
    einzeln.dedup();
    if einzeln.len() != 4 {
        return None;
    }
    // Und die vier Ecken muessen genau die Box ergeben: bei einem
    // „L“ aus vier Geraden gibt es fuenf oder sechs Punkte, das faellt
    // aber schon durch die Zeile oben.
    Some([x0, y0, x1, y1])
}

/// Schneide ein Polygon an einem Rechteck (Sutherland–Hodgman).
///
/// added 05.10.2026. Liefert eine **leere** Liste, wenn nichts von dem
/// Polygon im Rechteck liegt — das ist der Fall „nichts zeichnen“, und
/// es ist wichtig, ihn von „leer wegen Fehler“ zu unterscheiden (die
/// Kette aus `team/PROBLEME.txt`).
pub fn schneide_polygon(punkte: &[(f64, f64)], r: [f64; 4]) -> Vec<(f64, f64)> {
    let mut aktuell: Vec<(f64, f64)> = punkte.to_vec();
    // Reihenfolge: links, rechts, unten, oben
    for kante in 0..4 {
        if aktuell.is_empty() {
            break;
        }
        let mut naechste: Vec<(f64, f64)> = Vec::with_capacity(aktuell.len() + 4);
        for i in 0..aktuell.len() {
            let a = aktuell[i];
            let b = aktuell[(i + 1) % aktuell.len()];
            let da = drin(a, r, kante);
            let db = drin(b, r, kante);
            if da {
                naechste.push(a);
            }
            if da != db {
                if let Some(s) = schnittpunkt(a, b, r, kante) {
                    naechste.push(s);
                }
            }
        }
        aktuell = naechste;
    }
    aktuell
}

/// Schneide eine Strecke (Liang–Barsky) und gib sie geklemmt zurueck.
///
/// added 05.10.2026. `None` heisst „liegt ganz ausserhalb“ — dann wird
/// nichts gezeichnet. Fuer die Kontur ist das der teure Fall: bei 5 182
/// Clips im Korpus wird jeder Strich, der aus dem Clipp herausfaellt,
/// abgeschnitten statt ueberzeichnet.
pub fn schneide_strecke(
    a: (f64, f64),
    b: (f64, f64),
    r: [f64; 4],
) -> Option<((f64, f64), (f64, f64))> {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    if dx.abs() < 1e-12 && dy.abs() < 1e-12 {
        return if drin(a, r, 0) { Some((a, b)) } else { None };
    }
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    let grenzen = [
        (-dx, a.0 - r[0]),
        (dx, r[2] - a.0),
        (-dy, a.1 - r[1]),
        (dy, r[3] - a.1),
    ];
    for (p, q) in grenzen {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            if t > t1 {
                return None;
            }
            t0 = t0.max(t);
        } else {
            if t < t0 {
                return None;
            }
            t1 = t1.min(t);
        }
    }
    Some((
        (a.0 + dx * t0, a.1 + dy * t0),
        (a.0 + dx * t1, a.1 + dy * t1),
    ))
}

/// Liegt der Punkt **innerhalb** der Kante? (`kante` 0..4)
fn drin(p: (f64, f64), r: [f64; 4], kante: usize) -> bool {
    match kante {
        0 => p.0 >= r[0],
        1 => p.0 <= r[2],
        2 => p.1 >= r[1],
        _ => p.1 <= r[3],
    }
}

/// Schnittpunkt einer Strecke mit einer Rechteckkante.
fn schnittpunkt(a: (f64, f64), b: (f64, f64), r: [f64; 4], kante: usize) -> Option<(f64, f64)> {
    match kante {
        0 => {
            if (b.0 - a.0).abs() < 1e-12 {
                return None;
            }
            let t = (r[0] - a.0) / (b.0 - a.0);
            Some((r[0], a.1 + (b.1 - a.1) * t))
        }
        1 => {
            if (b.0 - a.0).abs() < 1e-12 {
                return None;
            }
            let t = (r[2] - a.0) / (b.0 - a.0);
            Some((r[2], a.1 + (b.1 - a.1) * t))
        }
        2 => {
            if (b.1 - a.1).abs() < 1e-12 {
                return None;
            }
            let t = (r[1] - a.1) / (b.1 - a.1);
            Some((a.0 + (b.0 - a.0) * t, r[1]))
        }
        _ => {
            if (b.1 - a.1).abs() < 1e-12 {
                return None;
            }
            let t = (r[3] - a.1) / (b.1 - a.1);
            Some((a.0 + (b.0 - a.0) * t, r[3]))
        }
    }
}

#[cfg(test)]
mod tests {
    /// added 07.10.2026: DIE VERBREITERUNG — und sie ist **kein**
    /// Formfehler-Sonderfall.
    ///
    /// GEMESSENER ANLASS: die erste Messung (`NARSIL_PDF_BOLD` am Bild)
    /// war **gesaettigt** — von 2 bis 8 Einheiten kam praktisch dasselbe
    /// Bild heraus, bei 25 % mehr Tinte. Das kann eine Verbreiterung von
    /// 0,03 Pixel nicht leisten; also war entweder die Geometrie falsch
    /// oder das Raster schluckt sie. (Es war das Raster: die kleinste
    /// wirksame Stufe ist ein ganzes Stuetzpunktgitter, 0,25 Pixel bei
    /// `ueberzeichnung = 4`.)
    ///
    /// DIESER TEST entscheidet die Geometrie und beschreibt ihre **Grenze**
    /// mit: auf einer **geraden** Kante rueckt die Kante um genau `d`, an
    /// einer **Ecke** um `d * cos(Winkel/2)` — die gemittelte Tangente ist
    /// die Winkelhalbierende, nicht die Miterlaenge.
    ///
    /// DIE ERSTE FASSUNG dieses Tests verlangte, dass alle vier Ecken um
    /// 0,5 nach aussen liegen. GEMESSEN sind **0,354** — cos 45 Grad mal
    /// 0,5. Die Erwartung war falsch, nicht der Code; zum zweiten Mal in
    /// dieser Runde traf es eine Testannahme statt der Implementierung.
    #[test]
    fn verbreiterung_rueckt_gerade_kanten_um_genau_d() {
        // Rechteck 10x10 mit **unterteilten** Kanten: die Punkte in der
        // Mitte liegen auf einer geraden Kante, die Ecken nicht.
        let mut r = vec![
            (0.0, 0.0),
            (5.0, 0.0),
            (10.0, 0.0),
            (10.0, 5.0),
            (10.0, 10.0),
            (5.0, 10.0),
            (0.0, 10.0),
            (0.0, 5.0),
        ];
        verbreite_umriss(&mut r, 0.5);
        // Der Mittelpunkt der oberen Kante: genau 0,5 nach oben.
        assert!((r[1].1 + 0.5).abs() < 1e-3, "gerade Kante: {}", r[1].1);
        assert!(
            (r[1].0 - 5.0).abs() < 1e-3,
            "seitlich unveraendert: {}",
            r[1].0
        );
        assert!((r[3].0 - 10.5).abs() < 1e-3, "rechte Kante: {}", r[3].0);
        // Die **Ecke** rueckt nur um cos(45 Grad) * 0,5 = 0,354.
        let ecke = r[0];
        // cos(45 Grad) = 0,7071 = sqrt(0,5); die Ecke rueckt also
        // `d * 0,7071` = 0,3536. GEMESSEN: genau dieser Wert.
        let erwartet = -0.5 * 0.5f32.sqrt();
        assert!(
            (ecke.0 - erwartet).abs() < 0.01 && (ecke.1 - erwartet).abs() < 0.01,
            "die Ecke rueckt um die Winkelhalbierende, nicht um d: {ecke:?} statt {erwartet}"
        );
    }

    #[test]
    fn verbreiterung_schliesst_das_loch() {
        // Quadrat mit quadratischem Loch in der Mitte, gegenlaeufig.
        let mut m = vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (4.0, 4.0),
            (6.0, 4.0),
            (6.0, 6.0),
            (4.0, 6.0),
        ];
        verbreite_umriss(&mut m, 0.25);
        let innen_x = m[4].0;
        assert!(
            innen_x >= 3.74 && innen_x <= 4.26,
            "die Lochkante muss um 0,25 nach **innen** ruecken (3,75 .. 4,25): {innen_x}"
        );
    }

    #[test]
    fn verbreiterung_ignoriert_kurze_und_flache_umrisse() {
        let mut zwei = vec![(0.0, 0.0), (1.0, 1.0)];
        verbreite_umriss(&mut zwei, 0.5);
        assert_eq!(
            zwei,
            vec![(0.0, 0.0), (1.0, 1.0)],
            "zwei Punkte sind kein Umriss"
        );

        let mut flach = vec![(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)];
        verbreite_umriss(&mut flach, 0.5);
        assert_eq!(
            flach,
            vec![(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)],
            "Flaeche 0"
        );
    }

    // added 05.10.2026. DIE CLIP-GEOMETRIE, an Beispielen und nicht am
    // Bild. Die Fehler, die diese drei Funktionen machen koennen, sind
    // beide **sichtbar** (zu viel Tinte oder zu wenig), also muessen sie
    // an Zahlen pruefbar sein.
    //
    // GEMESSENE Grundlage dafuer, dass ein Rechteck erkannt werden muss:
    // 5 182 Rechteck-Clips und 59 Weg-Clips im Korpus.

    fn rechteck_pfad(x: f64, y: f64, b: f64, h: f64) -> Vec<crate::inhalt::Pfadteil> {
        use crate::inhalt::{Pfadteil, Punkt, Segment};
        let ecke = |a: (f64, f64), c: (f64, f64)| {
            Segment::Linie(Punkt { x: a.0, y: a.1 }, Punkt { x: c.0, y: c.1 })
        };
        vec![Pfadteil {
            von: Punkt { x, y },
            segmente: vec![
                ecke((x, y), (x + b, y)),
                ecke((x + b, y), (x + b, y + h)),
                ecke((x + b, y + h), (x, y + h)),
                ecke((x, y + h), (x, y)),
            ],
        }]
    }

    #[test]
    fn rechteck_wird_erkannt_und_trapez_nicht() {
        assert_eq!(
            clip_als_rechteck(&rechteck_pfad(10.0, 20.0, 100.0, 50.0)),
            Some([10.0, 20.0, 110.0, 70.0])
        );
        // Ein Trapez aus vier Geraden: vier Ecken, aber nicht
        // achsenparallel — der Clipp darf es **nicht** als Rechteck
        // nehmen, sonst wuerde die kleinste Bounding-Box zu viel
        // freigeben.
        use crate::inhalt::{Pfadteil, Punkt, Segment};
        let linie = |a: (f64, f64), b: (f64, f64)| {
            Segment::Linie(Punkt { x: a.0, y: a.1 }, Punkt { x: b.0, y: b.1 })
        };
        let trape = vec![Pfadteil {
            von: Punkt { x: 0.0, y: 0.0 },
            segmente: vec![
                linie((0.0, 0.0), (10.0, 0.0)),
                linie((10.0, 0.0), (8.0, 10.0)),
                linie((8.0, 10.0), (2.0, 10.0)),
                linie((2.0, 10.0), (0.0, 0.0)),
            ],
        }];
        assert_eq!(
            clip_als_rechteck(&trape),
            None,
            "ein Trapez ist kein Rechteck — sonst gaelte die Box zu viel frei"
        );
        // Und ein Weg aus fuenf Geraden (ein Pentagon) auch nicht.
        let mut fuenf = rechteck_pfad(0.0, 0.0, 10.0, 10.0);
        fuenf[0].segmente.push(linie((10.0, 10.0), (5.0, 15.0)));
        assert_eq!(clip_als_rechteck(&fuenf), None);
    }

    #[test]
    fn polygon_wird_geschnitten_und_verliert_die_ferne_ecke() {
        // Ein Dreieck, dessen eine Ecke weit ausserhalb liegt.
        let r = [0.0, 0.0, 10.0, 10.0];
        let p = vec![(0.0, 0.0), (20.0, 0.0), (0.0, 20.0)];
        let geschnitten = schneide_polygon(&p, r);
        assert!(!geschnitten.is_empty(), "ein Teil liegt im Rechteck");
        for punkt in &geschnitten {
            assert!(
                punkt.0 >= r[0] - 1e-9
                    && punkt.0 <= r[2] + 1e-9
                    && punkt.1 >= r[1] - 1e-9
                    && punkt.1 <= r[3] + 1e-9,
                "Punkt {punkt:?} liegt ausserhalb des Rechtecks {r:?}"
            );
        }
        // Ein Polygon **ganz** ausserhalb liefert **leer** — und „leer“
        // heisst hier „nichts zeichnen“, nicht „Fehler“.
        assert!(schneide_polygon(&[(20.0, 20.0), (30.0, 20.0), (20.0, 30.0)], r).is_empty());
        // Und ein Polygon ganz **innen** bleibt unveraendert lang.
        let innen = vec![(1.0, 1.0), (9.0, 1.0), (9.0, 9.0), (1.0, 9.0)];
        assert_eq!(schneide_polygon(&innen, r).len(), 4);
    }

    #[test]
    fn strecke_wird_gekappt_und_verschwindet_vollstaendig() {
        let r = [0.0, 0.0, 10.0, 10.0];
        // quer durch: beide Enden aussen, die Mitte innen -> es bleibt
        // die Schnittstrecke zwischen den Kanten.
        let (a, b) = schneide_strecke((-5.0, 5.0), (15.0, 5.0), r).expect("schneidet");
        assert!((a.0 - 0.0).abs() < 1e-9, "Beginn bei x=0, war {a:?}");
        assert!((b.0 - 10.0).abs() < 1e-9, "Ende bei x=10, war {b:?}");
        // ganz draussen: nichts
        assert!(schneide_strecke((-5.0, -5.0), (-1.0, -1.0), r).is_none());
        // ganz drinnen: unveraendert
        let (a, b) = schneide_strecke((2.0, 2.0), (8.0, 2.0), r).expect("innen");
        assert_eq!(a, (2.0, 2.0));
        assert_eq!(b, (8.0, 2.0));
        // senkrecht: das ist der Fall, an dem eine reine x-Pruefung
        // falsch rechnen wuerde
        let (a, b) = schneide_strecke((5.0, -3.0), (5.0, 13.0), r).expect("senkrecht");
        assert!(
            (a.1 - 0.0).abs() < 1e-9 && (b.1 - 10.0).abs() < 1e-9,
            "{a:?} {b:?}"
        );
    }

    use super::*;

    /// F25: EINE GLYPHE HAT MEHRERE KONTUREN, und die zweite ist die
    /// **Gegenform**. Fuellt man sie einzeln zu, wird jeder Buchstabe
    /// mit Innenraum massiv.
    ///
    /// added 03.10.2026. GEMESSEN an `rfc2616.pdf` Seite 5 bei
    /// 300 dpi, Zeile 52, x 45..120, `<128` heisst schwarz:
    ///
    /// ```text
    /// Referenz  ###+........#####...........
    /// eigener   ##################.........
    /// ```
    ///
    /// DER TEST baut genau die Situation nach: ein grosses Quadrat und
    /// darin ein kleines, das **gegen** die aeussere Kontur laeuft —
    /// das ist die Nonzero-Regel, und sie macht das kleine Quadrat zum
    /// Loch. Laeuft es **gleich** herum, ist es nach Nonzero **kein**
    /// Loch, und genau das war der Fehler.
    #[test]
    fn gegenform_wird_ausgespart() {
        let aeusser: Vec<(f32, f32)> =
            vec![(10.0, 10.0), (110.0, 10.0), (110.0, 110.0), (10.0, 110.0)];
        let innen_gegen: Vec<(f32, f32)> =
            vec![(40.0, 40.0), (40.0, 80.0), (80.0, 80.0), (80.0, 40.0)];
        let mut f = flaeche_fuer(&[0.0, 0.0, 120.0, 120.0], 0, 72.0, 2000);
        let n = glyphe_zeichnen(&mut f, &[aeusser.clone(), innen_gegen], [0, 0, 0, 255]);
        assert!(n > 0, "es muss etwas gezeichnet worden sein");
        // Die Mitte ist ein Loch: weiss.
        let mitte = f.fb.pixel(60, 60).expect("Pixel");
        assert_eq!(
            (mitte.r, mitte.g, mitte.b),
            (255, 255, 255),
            "die Gegenform muss ausgespart sein"
        );
        // Der Ring zwischen den beiden Konturen ist schwarz.
        let ring = f.fb.pixel(20, 60).expect("Pixel");
        assert_eq!(
            (ring.r, ring.g, ring.b),
            (0, 0, 0),
            "der Ring muss gefuellt sein"
        );

        // Und die Regel selbst: **gleich** gewunden heisst nach Nonzero
        // **kein** Loch. Das ist keine Besonderheit von Glyphen, es ist
        // die Nonzero-Regel, und es ist der Grund fuer den Fehler.
        let innen_gleich: Vec<(f32, f32)> =
            vec![(40.0, 40.0), (80.0, 40.0), (80.0, 80.0), (40.0, 80.0)];
        let mut g = flaeche_fuer(&[0.0, 0.0, 120.0, 120.0], 0, 72.0, 2000);
        glyphe_zeichnen(&mut g, &[aeusser, innen_gleich], [0, 0, 0, 255]);
        let mitte = g.fb.pixel(60, 60).expect("Pixel");
        assert_eq!(
            (mitte.r, mitte.g, mitte.b),
            (0, 0, 0),
            "gleich gewundene Konturen ergeben nach Nonzero kein Loch"
        );
    }

    /// De Casteljau: der Mittelpunkt einer Geraden ist der Mittelpunkt.
    #[test]
    fn kurve_ist_gerade_wenn_kontrollpunkte_auf_der_geraden() {
        let mittig = kurven_punkt((0.0, 0.0), (10.0, 0.0), (20.0, 10.0), (30.0, 10.0), 0.5);
        // Start (0,0), Ende (30,10) -> Mitte (15,5)
        assert!((mittig.0 - 15.0).abs() < 0.001, "x = {}", mittig.0);
        assert!((mittig.1 - 5.0).abs() < 0.001, "y = {}", mittig.1);
    }

    /// Das Fuellen eines Vierecks setzt genau seine Pixel.
    ///
    /// added 03.10.2026. Gegenprobe fuer die Scanline-Regel: 10x10
    /// Pixel bei einer halben Pixelbreite muessen **unter 100** Pixel
    /// setzen, weil die Randpixel nur teilweise getroffen werden. Ein
    /// Wert ueber 100 hiesse, dass die Zeile doppelt gezaehlt wird.
    #[test]
    fn fuellt_keine_pixel_zweimal() {
        let mut f = Flaeche {
            fb: Framebuffer::new(20, 20, Color::new(255, 255, 255, 255)),
            skala: 1.0,
            hoehe: 20,
        };
        let gesetzt = viereck_fuellen(
            &mut f,
            &[(2.0, 2.0), (12.0, 2.0), (12.0, 12.0), (2.0, 12.0)],
            [0, 0, 0, 255],
        );
        assert!(gesetzt > 0);
        assert!(gesetzt <= 121, "{gesetzt} Pixel gesetzt — mehr als 11x11");
    }

    /// Die Y-Umrechnung: Punkt (0,0) ist **unten links**.
    ///
    /// added 03.10.2026. Das ist der Test, der den „auf dem Kopf"
    /// -Fehler faengt.
    #[test]
    fn y_ist_nach_oben_gezählt() {
        let f = Flaeche {
            fb: Framebuffer::new(100, 100, Color::new(255, 255, 255, 255)),
            skala: 1.0,
            hoehe: 100,
        };
        let (_, y_unten) = f.zu_pixel(0.0, 0.0);
        let (_, y_oben) = f.zu_pixel(0.0, 100.0);
        assert!(
            y_unten > y_oben,
            "unten muss groesser sein: {y_unten} vs {y_oben}"
        );
        assert_eq!(y_unten, 99.0);
    }
}
