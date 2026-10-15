// BILD_33, Schritt 5: der **Rahmen** als Ring um den Kasten.
//
// **WAS HIER GEBAUT WIRD:** eine reine Funktion ueber Zahlen — welche Pixel eines
// Kastens gehoeren zum Rahmen, welche zum Inhalt, welche zu gar nichts. Sie
// braucht **keinen** Stil, **keinen** Baum und **kein** `narsil-css`, und sie ist
// gegen die Firefox-Referenz messbar.
//
// **DIE GEMESSENE GEOMETRIE** (`team/testseiten/deko_ff/23.png`, Firefox 157,
// 04.10.2026): `border: 8px solid #000833; border-radius: 24px`, Kasten
// (10,6)-(170,76), Fuellung `#ffdd00`:
//
// ```text
//  Spalte x=80:  y=0..5 Zelle · y=6..13 #000833 · y=14+ #ffdd00
//  Zeile  y=35:  x=0..9 Zelle · x=10..17 #000833 · x=18+ #ffdd00
// ```
//
// **Der Rahmen ist also EXAKT 8 px breit** und sitzt zwischen `border-box` und
// `content-box`.
//
// **UND DIE ECKE, GEMESSEN** (Zeile y=14, x ab 10): der Rahmen beginnt bei
// **x = 15**, die Fuellung bei **x = 34**. Die **AUSSENkurve** ist der Kreis um
// **(34, 30)** mit r = 24 = `border-box`-Rand plus `border-radius`; vorhergesagt
// aus der Kreisgleichung bei y = 14,5 waere `34 - sqrt(24² - 15,5²) = 15,7` —
// **gemessen 15.** Die **INNENkurve** hat r = 24 − 8 = **16** und **denselben**
// Mittelpunkt (34, 30).
//
// **WAS DIESE DATEI NOCH NICHT KANN, und es ist der wichtigste Vorbehalt:**
// `dashed`, `dotted`, `double`, `groove`, `ridge`, `inset` und `outset` sind
// **Mustermalerei**. Sie brauchen die Lauflaenge entlang der Kante und — bei den
// Ecken — den **Kurzschnitt**, den Firefox anwendet. Hier wird der Ring
// **vollstaendig** als `solid` behandelt, und **`Ort` sagt nichts darueber, ob
// gerade ein Strich oder eine Luecke faellt.** Das ist der naechste Schritt, und
// es ist mit dieser Datei **nicht** getestet.

use crate::formen::Corners;
use narsil_core::color::Color;
use narsil_core::geometry::Rect;
use narsil_core::style::{Rahmen, RahmenStil};

/// Hoechstzahl gezeichneter Pixel je Rahmen (M3).
///
/// **GEMESSENE ENTSCHEIDUNG:** ein Rahmen ist **durch die Bildkante begrenzt** —
/// `0` ist der Anfangswert und ein geklemmter Wert wird **gemeldet**, nicht
/// stillschweigend gezaehlt. `MAX_RAHMEN_PIXEL` ist so gewaehlt, dass ein
/// 4000x4000-Kasten mit 8 px Rahmen darunter liegt (~128 000 px), aber ein
/// 30 000 x 30 000-Kasten darueber.
pub const MAX_RAHMEN_PIXEL: usize = 4_000_000;

/// Wo liegt ein Punkt im Kasten?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ort {
    /// **Ausserhalb** — der Rahmen endet hier; es wird nichts gemalt.
    Aussen,
    /// **Im Ring** — das ist Rahmen.
    Ring,
    /// **Im Inhalt** — Fuellung, Text, Bild.
    Innen,
}

/// Liegt der Punkt in dem abgerundeten Rechteck?
///
/// `r` ist in CSS-REIHENFOLGE (oben-links, oben-rechts, unten-rechts,
/// unten-links). Ein Radius, der groesser als die halbe Kante ist, wird auf die
/// Haelfte **begrenzt** — CSS Backgrounds 3 §6.2.1 sagt genau das, und ohne die
/// Begrenzung waere ein 24-px-Radius auf einem 20-px-Rand **negativ**, und die
/// Kurve laeuft nach aussen.
fn innen_abgerundet(p: (f32, f32), rect: Rect, r: impl Into<Corners>) -> bool {
    let c: Corners = r.into();
    if !c.is_circular() {
        // Release 15.10.2026 (Agent F): elliptical corners.
        let c = c.fit(rect.width, rect.height);
        return crate::formen::point_in_box_xy(p.0 - rect.x, p.1 - rect.y, rect.width, rect.height, &c);
    }
    let r = c.x;
    let links = rect.x;
    let rechts = rect.x + rect.width;
    let oben = rect.y;
    let unten = rect.y + rect.height;
    let (px, py) = p;
    if !(px >= links && px <= rechts && py >= oben && py <= unten) {
        return false;
    }
    // Release 08.10.2026: CSS Backgrounds 3 §5.5 — zu grosse Radien werden
    // GEMEINSAM verkleinert (Breite UND Hoehe), wie bei der Fuellung
    // (`formen::radius_form`). GEMESSEN vorher: nur gegen die halbe BREITE
    // begrenzt — apple.com „Kaufen" (`border-radius: 980px`, 330 x 65)
    // wurde eine Linse statt einer Pille.
    let r = crate::formen::radius_form(r, rect.width, rect.height);
    let rl = r[0].max(0.0);
    let rr = r[1].max(0.0);
    let ru = r[2].max(0.0);
    let run = r[3].max(0.0);
    // Die Eckmittelpunkte: **je Ecke** aus Rand und Radius, nicht aus der Mitte.
    let e0 = (links + rl, oben + rl);
    let e1 = (rechts - rr, oben + rr);
    let e2 = (rechts - ru, unten - ru);
    let e3 = (links + run, unten - run);
    let d = |c: (f32, f32)| {
        let (dx, dy) = (px - c.0, py - c.1);
        (dx * dx + dy * dy).sqrt()
    };
    // **GEMESSENE ENTSCHEIDUNG, warum `<` statt `<=`:** ein Punkt **genau auf**
    // der Kante gehoert noch zur Flaeche; `<=` wuerde ihn mitnehmen und die
    // Kante eine Pixelbreite nach aussen schieben. Beim Test gegen Firefox
    // entscheidet genau diese eine Zeile, ob die Ecke um 1 px daneben liegt.
    if px < e0.0 && py < e0.1 && d(e0) > rl {
        return false;
    }
    if px > e1.0 && py < e1.1 && d(e1) > rr {
        return false;
    }
    if px > e2.0 && py > e2.1 && d(e2) > ru {
        return false;
    }
    if px < e3.0 && py > e3.1 && d(e3) > run {
        return false;
    }
    true
}

/// **WO LIEGT DER PUNKT?** Das ist die ganze Rechnung.
///
/// Der Ring ist die Menge zwischen der **Aussenkurve** (`border-box` mit
/// `border-radius`) und der **INNENkurve** (`border-box` um die vier
/// Rahmenbreiten eingezogen, mit dem Radius **um dieselbe Breite kleiner**).
#[must_use]
pub fn ort(p: (f32, f32), rect: Rect, rahmen: &Rahmen, radius: impl Into<Corners>) -> Ort {
    let radius: Corners = radius.into();
    if !rahmen.ist_gezeichnet() {
        // Kein Rahmen: alles innerhalb des Kastens ist Inhalt. Das ist der
        // Fall fuer **jede** Seite ohne `border` — und es muss billig sein,
        // weil es der Normalfall ist.
        return if innen_abgerundet(p, rect, radius) {
            Ort::Innen
        } else {
            Ort::Aussen
        };
    }
    if !innen_abgerundet(p, rect, radius) {
        return Ort::Aussen;
    }
    let innen = Rect::new(
        rect.x + rahmen.gezeichnete_breite(3),
        rect.y + rahmen.gezeichnete_breite(0),
        rect.width - rahmen.gezeichnete_breite(1) - rahmen.gezeichnete_breite(3),
        rect.height - rahmen.gezeichnete_breite(0) - rahmen.gezeichnete_breite(2),
    );
    // **DER INNERE RADIUS:** `border-radius` **minus** die Rahmenbreite, je Ecke
    // die **grössere** der beiden anliegenden Seiten. CSS Backgrounds 3 §6.2.2
    // sagt „subtract the border width"; bei **unterschiedlichen** Breiten ist
    // „die anliegende" nicht eindeutig, und **die groessere** ist die, die
    // BAUFIX 04.10.2026 (SCHRIFT, nach BRETT 03:4x): `#![...]` ist ein
    // INNERES Attribut und darf nicht mitten in einer Funktion stehen
    // (E0658/aelter: „an inner attribute is not permitted in this
    // context"). Aussen ist es eine normale Drosselung.
    #[allow(clippy::needless_range_loop)]
    let innen_r: Corners = if !radius.is_circular() {
        // Release 15.10.2026 (Agent F): an elliptical inner corner loses
        // the side border widths horizontally and the top and bottom ones
        // vertically (CSS Backgrounds 3 §5.2).
        let w = |i: usize| rahmen.gezeichnete_breite(i);
        Corners {
            x: [radius.x[0] - w(3), radius.x[1] - w(1), radius.x[2] - w(1), radius.x[3] - w(3)].map(|v| v.max(0.0)),
            y: [radius.y[0] - w(0), radius.y[1] - w(0), radius.y[2] - w(2), radius.y[3] - w(2)].map(|v| v.max(0.0)),
        }
    } else {
        let radius = radius.x;
        [
        (radius[0]
            - rahmen
                .gezeichnete_breite(0)
                .max(rahmen.gezeichnete_breite(3)))
        .max(0.0),
        (radius[1]
            - rahmen
                .gezeichnete_breite(0)
                .max(rahmen.gezeichnete_breite(1)))
        .max(0.0),
        (radius[2]
            - rahmen
                .gezeichnete_breite(2)
                .max(rahmen.gezeichnete_breite(1)))
        .max(0.0),
        (radius[3]
            - rahmen
                .gezeichnete_breite(3)
                .max(rahmen.gezeichnete_breite(2)))
        .max(0.0),
        ]
        .into()
    };
    if innen.width <= 0.0 || innen.height <= 0.0 {
        // Der Inhalt ist weg (ein 40-px-Rahmen in einem 40-px-Kasten): dann
        // ist **alles** Ring. `None` waere ein Loch.
        return Ort::Ring;
    }
    if innen_abgerundet(p, innen, innen_r) {
        Ort::Innen
    } else {
        Ort::Ring
    }
}

/// Zwei Drittel — die Schattenfarbe einer 3-D-Rahmenkante.
const ZWEI_DRITTEL: f32 = 2.0 / 3.0;

/// Die **Schattenfarbe** einer 3-D-Rahmenkante — **je Kanal `floor(farbe × ⅔)`**.
///
/// **GEMESSEN** an Firefox 157 am 04.10.2026. **Und warum das eine eigene
/// Messung ist und nicht `× ⅔` hingesetzt:** `floor` und `round` unterscheiden
/// sich **nur** bei Kanälen, deren Rest >= ½ ist. `#8333dd` aus
/// `team/testseiten/deko_ff/27.png` **kann es nicht**: 131·⅔ = 87,33 -> 87 in
/// **beiden**, 51·⅔ = 34,00 **exakt**, 221·⅔ = 147,33 -> 147 in beiden.
/// Also vier Farben mit Kanalwerten, die **alle Rest ⅔** haben
/// (`52, 100, 199, 253`), gemessen in `team/testseiten/ua_ff/`:
///
/// | Kanal | Rechnung | **GEMESSEN** | `floor` | `round` |
/// |---|---|---|---|---|
/// | `#000034` B | 52·⅔ = 34,67 | **34** | 34 | 35 |
/// | `#006400` G | 100·⅔ = 66,67 | **66** | 66 | 67 |
/// | `#340000` R | 52·⅔ = 34,67 | **34** | 34 | 35 |
/// | `#0000fd` B | 253·⅔ = 168,67 | **168** | 168 | 169 |
///
/// **Alle vier `floor`, kein einziges `round`.**
#[must_use]
pub fn schatten(farbe: Color) -> Color {
    Color {
        r: (f32::from(u16::from(farbe.r)) * ZWEI_DRITTEL).floor() as u8,
        g: (f32::from(u16::from(farbe.g)) * ZWEI_DRITTEL).floor() as u8,
        b: (f32::from(u16::from(farbe.b)) * ZWEI_DRITTEL).floor() as u8,
        a: farbe.a,
    }
}

/// Der Strich, in dem `abs` liegt: `(Mitte auf der Kante, Laenge)`.
///
/// **GEMESSENE REGEL, an drei Seitenlaengen geprueft** (Firefox 157, 8 px
/// Rahmen, `team/testseiten/deko_ff/24.png` und die Messseite in
/// `team/testseiten/ua_ff/`): die **Luecke steht fest** bei `luecke`, und der
/// **Rest der Kante wird gleichmaessig auf die uebrigen Striche verteilt**.
///
/// | Seite | `luecke` | Striche | Rest je Strich | **GEMESSEN** |
/// |---|---|---|---|---|
/// | 160 px | 3w = 24 | 4 | (160 − 3·24)/4 = **22,0** | 22 |
/// | 240 px | 3w = 24 | 6 | (240 − 5·24)/6 = **20,0** | 19…22 |
/// | 100 px | 3w = 24 | 3 | (100 − 2·24)/3 = **17,3** | 18 |
///
/// **UND WAS NICHT GEMESSEN IST:** Firefox setzt die Strichgrenzen **halbzahlig**
/// — an den Lueckenrandpixeln steht ein **50-%-Mischwert** (`#777b90` zwischen
/// `#000833` und `#eeeeee`, alle drei Kanaele bei 0,50). Die Luecke misst man
/// deshalb aus ganzzahligen Pixeln mit **±1** Unsicherheit. Diese Regel ist das
/// Beste, was **drei** Laengen hergeben; sie ist **nicht** die
/// Firefox-Fittingformel, und der Test sagt das **mit Messwert**.
fn strich(abs: f32, laenge: f32, luecke: f32) -> Option<(f32, f32)> {
    if laenge <= 0.0 || luecke <= 0.0 {
        return None;
    }
    let perioden = (laenge / (2.0 * luecke)).floor().max(0.0);
    let striche = perioden + 1.0;
    let laengs = (laenge - perioden * luecke).max(0.0) / striche;
    if laengs <= 0.0 {
        return None;
    }
    let start = (abs / (laengs + luecke)).floor() * (laengs + luecke);
    Some((start + laengs / 2.0, laengs))
}

/// Die Farbe eines Kantenpunkts — **alle sieben Stile**.
///
/// **BILD_37: die fruehere Einschraenkung „nur `solid` korrekt" ist aufgehoben**,
/// und zwar an **gemessenen** Regeln statt an vermuteten (Firefox 157,
/// `team/testseiten/deko_ff/24..27.png` + die Messseite in `ua_ff/`):
///
/// | Stil | **GEMESSENE REGEL** (alle bei 8 px) |
/// |---|---|
/// | `solid` | die ganze Kante in der Farbe |
/// | `double` | **3 / 2 / 3** — Linie, Luecke, Linie |
/// | `groove` | je Kante **zwei Haelften**: aussen `floor(⅔)`, innen voll |
/// | `ridge` | je Kante zwei Haelften: **aussen voll, innen `floor(⅔)`** |
/// | `inset` | Kanten **oben und links** ganz `floor(⅔)`, **unten und rechts** ganz voll |
/// | `outset` | **genau vertauscht** gegenueber `inset` |
/// | `dashed` | Luecke **3w** fest, Rest gleichmaessig (siehe [`strich`]) |
/// | `dotted` | Punkt mit **Durchmesser = w**, als **Kreis** in der Kantenmitte |
///
/// **UND ZWEI DINGE, DIE ICH NICHT BEHAUPTE:** (1) Die Kantenreihenfolge hier ist
/// **oben, rechts, unten, links** — die CSS-Reihenfolge, nicht die des Bildes;
/// `inset`/`outset` haengen davon ab, welche Kante geschaetzt wird. (2) Firefox
/// laesst das Muster **um die Ecke laufen**; hier laeuft es **an jeder Kante neu**.
/// Fuer `dashed` heisst das an einer Ecke bis **3w** Versatz, fuer die
/// 3-D-Stile **nichts** (die sind ortsunabhaengig).
/// **UND `radius` IST KEIN OPTIONALER ZUSATZ — es ist die Ursache eines
/// gemessenen Fehlers (BILD_38).** Diese Funktion hat lange **ohne** Radius
/// gearbeitet und `ort(p, rect, rahmen, [0.0; 4])` aufgerufen, also mit der
/// **ungerundeten** Innenkante. GEMESSEN, `border-radius: 24px` mit
/// `border: 8px solid` (Firefox-Vergleich `team/testseiten/rad.html`): die
/// **Rahmenecken waren quadratisch** statt gerundet — die Innenkante stand
/// **konstant** bei 8 px, statt nach `r − w = 16` auszulaufen.
/// **`ort` selbst war richtig** (GEMESSEN: `ort((12.5, 12.5)) = Ring`), nur
/// `ring_farbe` Questions mit `[0.0; 4]` zurueck und **malte deshalb nichts**.
/// Wer hier einen Radius weglässt, bekommt **einfarbige** Ecken.
#[must_use]
pub fn ring_farbe(p: (f32, f32), rect: Rect, rahmen: &Rahmen, radius: impl Into<Corners>) -> Option<Color> {
    let radius: Corners = radius.into();
    if ort(p, rect, rahmen, radius) != Ort::Ring {
        return None;
    }
    let oben = rect.y;
    let unten = rect.y + rect.height;
    let links = rect.x;
    let rechts = rect.x + rect.width;
    let (px, py) = p;
    // Welche Kante? **GEMESSENE ENTSCHEIDUNG:** bei einem **dicken** Rahmen
    // (dicker als die halbe Kante) waeren die Kanten nicht mehr
    // disjunkt; der Ring gehoert dann der **mit dem groessten Anteil**.
    //
    // Release 10.10.2026: der Abstand wird durch die BREITE der Kante geteilt
    // — die Ecke teilt die Linie von der aeusseren zur INNEREN Ecke (CSS
    // Backgrounds 3 §5.4), nicht die 45-Grad-Diagonale. Eine Kante ohne
    // Breite bekommt nie ein Pixel. GEMESSEN vorher (Web-Platform-Tests,
    // `css/CSS2/borders`): `border-bottom: 1in solid` ohne Seitenrahmen wurde
    // ein DREIECK.
    let anteil = |d: f32, i: usize| {
        let w = rahmen.gezeichnete_breite(i);
        if w > 0.0 { d / w } else { f32::INFINITY }
    };
    let werte = [anteil(py - oben, 0), anteil(rechts - px, 1), anteil(unten - py, 2), anteil(px - links, 3)];
    let mut i = 0;
    for k in 1..4 {
        if werte[k] < werte[i] {
            i = k;
        }
    }
    let breite = rahmen.gezeichnete_breite(i);
    if breite <= 0.0 {
        return None;
    }
    let farbe = rahmen.farbe[i];
    // `t` = Abstand von der **Aussenkante** der Kante hinein, `s` = laengs der
    // Kante, gemessen von der linken bzw. oberen Kastenkante.
    let (t, s, laenge) = match i {
        0 => (py - oben, px - links, rect.width),
        1 => (rechts - px, py - oben, rect.height),
        2 => (unten - py, px - links, rect.width),
        _ => (px - links, py - oben, rect.height),
    };
    match rahmen.stil[i] {
        RahmenStil::Keiner => None,
        RahmenStil::Durchgezogen => Some(farbe),
        RahmenStil::Doppelt => {
            // **GEMESSEN 3 / 2 / 3 bei w = 8.** Die Linie ist so breit wie
            // moeglich, ohne dass die Luecke verschwindet; CSS Backgrounds 3
            // §4.4 sagt zusaetzlich: unter 3 px Breite **faellt eine Linie weg**.
            let linie = ((breite as i32 + 1) / 3).max(1);
            let luecke = (breite as i32 - 2 * linie).max(0) as f32;
            let t = t as i32 as f32;
            if t < linie as f32 || t >= linie as f32 + luecke {
                Some(farbe)
            } else {
                None
            }
        }
        RahmenStil::Furche => {
            if t < breite / 2.0 {
                Some(schatten(farbe))
            } else {
                Some(farbe)
            }
        }
        RahmenStil::Wulst => {
            if t < breite / 2.0 {
                Some(farbe)
            } else {
                Some(schatten(farbe))
            }
        }
        // **GEMESSEN:** `inset` schattiert die Kanten **oben (0) und links (3)**
        // — **ganz**, nicht nur die Aussenhälfte; `outset` genau umgekehrt.
        RahmenStil::Innen => {
            if i == 0 || i == 3 {
                Some(schatten(farbe))
            } else {
                Some(farbe)
            }
        }
        RahmenStil::Aussen => {
            if i == 0 || i == 3 {
                Some(farbe)
            } else {
                Some(schatten(farbe))
            }
        }
        RahmenStil::Gestrichelt => match strich(s, laenge, 3.0 * breite) {
            Some((mitte, laengs)) if (s - mitte).abs() <= laengs / 2.0 => Some(farbe),
            _ => None,
        },
        RahmenStil::Gepunktet => {
            // **GEMESSEN:** der Punkt hat den **Durchmesser der Rahmenbreite**
            // und liegt **mittig** in der Kante: in der Mitte der Kante misst
            // er die vollen 8 px quer, an seinem linken Rand 4 px — das ist ein
            // **Kreis**, kein Quadrat.
            let (mitte, radius) = match strich(s, laenge, breite) {
                Some((mitte, laengs)) => (mitte, laengs / 2.0),
                None => return None,
            };
            let laengs = (s - mitte).abs();
            let quer = (t - breite / 2.0).abs();
            if laengs * laengs + quer * quer <= radius * radius {
                Some(farbe)
            } else {
                None
            }
        }
    }
}

/// Malt den Rahmen in einen Framebuffer und **zaehlt** die gemalten Pixel.
///
/// **BILD_37: es wird jetzt jeder der sieben Stile gemalt**, nach den in
/// [`ring_farbe`] **gemessenen** Regeln. Vorher stand hier eine Schranke, die
/// alles ausser `solid` mit **0** und „nichts gemalt" beantwortete — das war
/// eine **Luecke mit Namen**, und sie ist geschlossen.
pub fn zeichne(
    fb: &mut crate::framebuffer::Framebuffer,
    rect: Rect,
    rahmen: &Rahmen,
    radius: impl Into<Corners>,
) -> usize {
    let radius: Corners = radius.into();
    if !rahmen.ist_gezeichnet() {
        return 0;
    }
    // Release 08.10.2026: die Radien EINMAL nach §5.5 verkleinern — dann
    // stimmen Aussen- und Innenkurve, und die Suchbaender unten richten
    // sich nach dem wirklichen Radius (nicht nach 980 px).
    let radius = radius.fit(rect.width, rect.height);
    let x0 = rect.x.floor().max(0.0) as u32;
    let y0 = rect.y.floor().max(0.0) as u32;
    let x1 = ((rect.x + rect.width).ceil() as u32).min(fb.width());
    let y1 = ((rect.y + rect.height).ceil() as u32).min(fb.height());
    let mut gemalt = 0usize;
    // Release 08.10.2026: nur der RING wird abgesucht — oben und unten die
    // Baender (Rahmenbreite bzw. Eckradius, plus ein Pixel), dazwischen nur
    // der linke und der rechte Streifen. GEMESSEN vorher: jedes Pixel des
    // ganzen Kastens lief durch `ort` — 13–16 ms je Bild auf Wikipedia
    // (1888 x 954), fuer Rahmen von einem Pixel.
    // Release 15.10.2026 (Agent F): horizontal and vertical radii (equal
    // for circular corners).
    let radx = |i: usize| if radius.x[i].is_finite() { radius.x[i].max(0.0) } else { 0.0 };
    let rady = |i: usize| if radius.y[i].is_finite() { radius.y[i].max(0.0) } else { 0.0 };
    let band_oben = rahmen.gezeichnete_breite(0).max(rady(0)).max(rady(1)) + 1.0;
    let band_unten = rahmen.gezeichnete_breite(2).max(rady(2)).max(rady(3)) + 1.0;
    let streifen_links = rahmen.gezeichnete_breite(3).max(radx(0)).max(radx(3)) + 1.0;
    let streifen_rechts = rahmen.gezeichnete_breite(1).max(radx(1)).max(radx(2)) + 1.0;
    let oben_bis = rect.y + band_oben;
    let unten_ab = rect.y + rect.height - band_unten;
    let links_bis = ((rect.x + streifen_links).ceil().max(0.0) as u32).clamp(x0, x1.max(x0));
    let rechts_ab = ((rect.x + rect.width - streifen_rechts).floor().max(0.0) as u32).clamp(links_bis, x1.max(links_bis));
    // Release 09.10.2026: KANTENGEGLAETTETE ECKEN. In einer gerundeten Ecke,
    // deren beide Kanten durchgezogen und gleichfarbig sind (der Normalfall:
    // Knopf, Karte, Eingabefeld), deckt die Abstandsfunktion den Pixel
    // anteilig: Aussenform minus Innenform. Sonst (gestrichelt, zwei Farben,
    // eckig) bleibt es beim Mittelpunkt-Test wie bisher.
    let w = |i: usize| rahmen.gezeichnete_breite(i);
    let innen = Rect::new(
        rect.x + w(3),
        rect.y + w(0),
        (rect.width - w(1) - w(3)).max(0.0),
        (rect.height - w(0) - w(2)).max(0.0),
    );
    // Ecke i: (waagerechte Kante, senkrechte Kante).
    const KANTEN: [(usize, usize); 4] = [(0, 3), (0, 1), (2, 1), (2, 3)];
    let radius_innen: Corners = if radius.is_circular() {
        [0, 1, 2, 3]
            .map(|i| {
                let (a, b) = KANTEN[i];
                (radx(i) - w(a).max(w(b))).max(0.0)
            })
            .into()
    } else {
        Corners {
            x: [0, 1, 2, 3].map(|i| (radx(i) - w(KANTEN[i].1)).max(0.0)),
            y: [0, 1, 2, 3].map(|i| (rady(i) - w(KANTEN[i].0)).max(0.0)),
        }
    };
    let glatt = [0, 1, 2, 3].map(|i| {
        let (a, b) = KANTEN[i];
        radx(i) > 0.0
            && rady(i) > 0.0
            && rahmen.stil[a] == RahmenStil::Durchgezogen
            && rahmen.stil[b] == RahmenStil::Durchgezogen
            && w(a) > 0.0
            && w(b) > 0.0
    });
    let ecke_von = |px: f32, py: f32| -> Option<usize> {
        let links = px < rect.x + radx(0).max(radx(3));
        let rechts = px > rect.x + rect.width - radx(1).max(radx(2));
        let oben = py < rect.y + rady(0).max(rady(1));
        let unten = py > rect.y + rect.height - rady(2).max(rady(3));
        let i = match (links, rechts, oben, unten) {
            (true, _, true, _) => 0,
            (_, true, true, _) => 1,
            (_, true, _, true) => 2,
            (true, _, _, true) => 3,
            _ => return None,
        };
        // Nur im Quadrat der Ecke selbst.
        let (rx, ry) = (radx(i), rady(i));
        let (ex, ey) = match i {
            0 => (rect.x, rect.y),
            1 => (rect.x + rect.width - rx, rect.y),
            2 => (rect.x + rect.width - rx, rect.y + rect.height - ry),
            _ => (rect.x, rect.y + rect.height - ry),
        };
        (px >= ex && px <= ex + rx && py >= ey && py <= ey + ry).then_some(i)
    };
    let mut pixel = |x: u32, y: u32, gemalt: &mut usize| {
        // **GEMESSENE ENTSCHEIDUNG:** der Punkt liegt in der **Mitte** des
        // Pixels (x + 0,5). Ohne das verschiebt sich die Kante um ein halbes
        // Pixel, und genau daran misst man gegen Firefox.
        let p = (x as f32 + 0.5, y as f32 + 0.5);
        if let Some(i) = ecke_von(p.0, p.1)
            && glatt[i]
        {
            let aussen = (0.5 - crate::formen::distance_xy(p.0, p.1, rect, &radius)).clamp(0.0, 1.0);
            let drin = if innen.width > 0.0 && innen.height > 0.0 {
                (0.5 - crate::formen::distance_xy(p.0, p.1, innen, &radius_innen)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let deckung = (aussen - drin).clamp(0.0, 1.0);
            // Zwei Farben: die Grenze laeuft diagonal von der Aussen- zur
            // Innenecke (CSS Backgrounds 3 §4.5), je Seite ihre Farbe.
            let (a, b) = KANTEN[i];
            let dx = if i == 0 || i == 3 { p.0 - rect.x } else { rect.x + rect.width - p.0 };
            let dy = if i < 2 { p.1 - rect.y } else { rect.y + rect.height - p.1 };
            let f = if rahmen.farbe[a] == rahmen.farbe[b] || dy * w(b) < dx * w(a) {
                rahmen.farbe[a]
            } else {
                rahmen.farbe[b]
            };
            let a = (f32::from(f.a) * deckung).round() as u8;
            if a == 0 {
                return;
            }
            if let Some(unter) = fb.pixel(x, y)
                && fb.set_pixel(x, y, crate::framebuffer::blend_source_over(Color::new(f.r, f.g, f.b, a), unter))
                && deckung >= 0.5
            {
                *gemalt += 1;
            }
            return;
        }
        if ort(p, rect, rahmen, radius) != Ort::Ring {
            return;
        }
        if let Some(f) = ring_farbe(p, rect, rahmen, radius) {
            // Release 08.10.2026: eine (halb)durchsichtige Rahmenfarbe wird
            // GEMISCHT, eine ganz durchsichtige malt nichts. GEMESSEN vorher:
            // `border: 1px solid transparent` (Wikipedia-, GitHub-,
            // tagesschau-Knoepfe) schrieb (0,0,0,0) in den Puffer — im Fenster
            // (BGRA ohne Alpha) eine schwarze Linie um jeden Knopf.
            let f = match f.a {
                0 => return,
                255 => f,
                _ => match fb.pixel(x, y) {
                    Some(unter) => crate::framebuffer::blend_source_over(f, unter),
                    None => return,
                },
            };
            if fb.set_pixel(x, y, f) {
                *gemalt += 1;
            }
        }
    };
    for y in y0..y1 {
        let yf = y as f32;
        let im_band = yf < oben_bis || yf + 1.0 > unten_ab;
        if im_band {
            for x in x0..x1 {
                if gemalt >= MAX_RAHMEN_PIXEL {
                    return gemalt;
                }
                pixel(x, y, &mut gemalt);
            }
        } else {
            for x in (x0..links_bis).chain(rechts_ab..x1) {
                if gemalt >= MAX_RAHMEN_PIXEL {
                    return gemalt;
                }
                pixel(x, y, &mut gemalt);
            }
        }
    }
    gemalt
}
