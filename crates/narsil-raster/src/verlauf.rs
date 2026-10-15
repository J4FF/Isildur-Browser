//! BILD_44, Station 3: **linearer** Farbverlauf, zwei Stopps.
//!
//! **WAS HIER GEBAUT WIRD — und was nicht.** Gebaut wird **genau eine Sache**:
//! ein linearer Verlauf mit **zwei** Stopps mit gesetzter Position, **ohne**
//! Wiederholung. Jedes andere liefert **0** zurueck und **nichts** wird gemalt —
//! das ist eine **Luecke mit Namen**, damit ein Aufrufer sie nicht fuer „der
//! Verlauf ist unsichtbar" haelt. **Nicht** gebaut und deshalb **nicht**
//! behauptet: radial, Kegel, `repeating-*`, Stopps **ohne** Position (die
//! Verteilung zwischen den Nachbarn), **mehr** als zwei Stopps, Hartkanten an
//! zwei Stopps mit gleicher Position. Jedes davon **einzeln**, mit eigenem
//! Pixelvergleich.
//!
//! **DIE ZWEI GEMESSENEN FESTLEGEN, die man nicht nachrechnen kann:**
//!
//! 1. **Der Verlauf laeuft ueber die PIXELMITTEN.** GEMESSEN (Firefox 157,
//!    `team/testseiten/verlauf_lin_ff.png`, Kasten 100 × 50): in der Mitte steht
//!    **(126, 0, 129)** — **nicht** (128, 0, 128). Das ist `255 · 0,495 = 126,2`
//!    und `255 · 0,505 = 128,8`, also `t = (x + 0,5)/breite`. An den Enden
//!    **(254, 0, 1)** und **(1, 0, 254)**. **Wer `t = x/breite` rechnet,
//!    verfehlt die Mitte um 2 und die Enden um 1** und glaubt, es sei eine
//!    Rundungsfrage.
//! 2. **Firefox interpoliert PREMULTIPLIZIERT**, und zwar auf die exakte
//!    8-Bit-Zahl: `#ff000000 → #0000ffff` ueber dem Grund `#2040c0` ergibt bei
//!    `x = 50` **(16, 32, 224)**. Premultipliziert gerechnet:
//!    `(0, 0, 255t) + (1−t)·(32, 64, 192)` = (15,8 / 31,7 / 223,8) = **(16, 32,
//!    224)** — **exakt**. **Ohne** Multiplikation kaeme **(79, 32, 160)**, ein
//!    Unterschied von **63** im Rot-Kanal. Wer die Farbe **vor** der Mischung
//!    normalisiert, macht genau das Gegenteil von dem, was Firefox tut.

use narsil_core::color::Color;
use narsil_core::geometry::Rect;
use narsil_core::verlauf_typ::{Verlauf, VerlaufArt};
// **IM CRATE SELBST** heisst der Pfad `crate::` — nicht `narsil_raster::`.
use crate::framebuffer::Framebuffer;

/// **Hoechstes** Seitenverhaeltnis, das diese Fassung zeichnet.
///
/// **WARUM ES DAS GIBT:** die Schleife laeuft ueber den Kasten, und ein
/// Verlauf auf einem **riesigen** Kasten waere eine Schleife ueber Milliarden
/// Pixel. Die anderen Rasterizer dieser Crate haben dieselbe Grenze
/// (`MAX_RAHMEN_PIXEL`, `MAX_SCHATTEN_PIXEL`), und sie ist **benannt** statt
/// still — eine Schleife ohne Grenze ist der Fehler, den der Leckwaechter
/// meldet.
const MAX_PIXEL: u64 = 40_000_000;

/// Die **Richtung** eines Winkels in Grad als Einheitsvektor — CSS Images 3
/// §3.1: `0deg` zeigt nach **oben**, positive Werte im **Uhrzeigersinn**.
///
/// **UND WARUM `(sin, −cos)` und nicht `(sin, cos)`:** im Bild wächst `y` nach
/// **unten**. „Nach oben" ist also `(0, −1)` — genau das gibt `cos(0) = 1` mit
/// dem **Minus** davor. Ohne das Minus waere `0deg` nach unten und **jeder**
/// Winkel gespiegelt.
fn richtung(winkel: f32) -> (f32, f32) {
    let rad = winkel.to_radians();
    (rad.sin(), -rad.cos())
}

/// Die **Laenge der Verlaufslinie** fuer einen Kasten (CSS Images 3 §10.1):
/// `|b · sin| + |h · cos|` — so lang, dass die Linie den Kasten in beiden
/// Richtungen trifft.
///
/// **GEMESSENE Probe, `winkel = 90deg`:** `|100 · 1| + |50 · 0| = 100` — und das
/// ist **genau** die Zahl, die den gemessenen Verlauf ueber **100 px** laufen
/// laesst.
fn linien_laenge(b: f32, h: f32, winkel: f32) -> f32 {
    let rad = winkel.to_radians();
    (b * rad.sin()).abs() + (h * rad.cos()).abs()
}

/// Die **Farbe eines Pixels** aus dem Verlauf, oder `None`, wenn der Verlauf
/// diesen Fall **nicht** abdeckt (fremde Art, falsche Stoppzahl, `wiederholt`).
///
/// **UND DER WICHTIGSTE TEIL:** die Interpolation laeuft ueber die
/// **premultiplizierten** Farben, und das Ergebnis wird **einmal** ueber den
/// Hintergrund gemischt — in dieser Reihenfolge. Wer erst die Farbe mischt und
/// dann den Alpha anwendet, bekommt (79, 32, 160) statt (16, 32, 224).
/// **Die Positionen der Stopps**, aufgeloest nach CSS Images 3 §10.4.
///
/// **DREI FAELLE, alle GEMESSEN** (`team/testseiten/verlauf_ohne-position_ff.png`
/// gegen `verlauf_3stopps_ff.png` — die beiden Bilder sind **identisch**):
///
/// 1. **Ohne Position** wird **gleichmäßig** verteilt: der erste bekommt **0**,
///    der letzte **1**, die dazwischen equally. **GEMESSEN: `linear-gradient(90deg,
///    #ff0000, #00ff00, #0000ff)` ergibt exakt dasselbe Bild wie
///    `… 0%, … 50%, … 100%`** — beide Zeilen y=+25 liefern
///    `(252,3,0) (125,130,0) (0,252,3) (0,125,130) (0,3,252)`.
/// 2. **Eine ausgeschriebene Position** gilt fuer die Nachbarn **so weit, wie sie
///    reicht** — ein `40%` in der Mitte begrenzt beide Seiten.
/// 3. **Harte Kante**: zwei Stopps an **derselben** Position. **GEMESSEN**
///    (`verlauf_hartkante_ff.png`): `90deg, #ff0000 50%, #0000ff 50%` liefert bei
///    x = 25 `(255,0,0)` und bei x = 50 `(0,0,255)` — der Sprung liegt also bei
///    `t = 0.5`, und `x = 50` hat `t = 0.505` und gehoert **schon** zur zweiten
///    Farbe.
#[must_use]
pub fn stopp_positionen(verlauf: &Verlauf) -> Vec<f32> {
    let n = verlauf.stopps.len();
    let mut pos: Vec<Option<f32>> = verlauf.stopps.iter().map(|s| s.anteil).collect();
    pos[0] = Some(pos[0].unwrap_or(0.0));
    pos[n - 1] = Some(pos[n - 1].unwrap_or(1.0));
    // Die Luecken zwischen zwei **bekannten** Positionen gleichmaessig fuellen.
    let mut i = 0;
    while i < n {
        if pos[i].is_none() {
            // Nach links bis zur letzten bekannten Position suchen.
            let mut l = i;
            while l > 0 && pos[l - 1].is_none() {
                l -= 1;
            }
            // Und nach rechts.
            let mut r = i;
            while r + 1 < n && pos[r + 1].is_none() {
                r += 1;
            }
            let a = pos[l - 1].unwrap_or(0.0);
            let b = pos[r + 1].unwrap_or(1.0);
            // **DIE ANZAHL IST DIE DER LUECKEN, nicht die der Luecken minus 1** —
            // das war mein Fehler: mit `anzahl - 1` bekam der mittlere Stopp
            // **1.0** statt **0.5**.
            // **GEMESSENE WIRKUNG:** `90deg, #ff0000, #00ff00, #0000ff` lieferte
            // bei (0, 0) **(254, 1, 0)** statt der **GEMESSENEN** Firefox-Farbe
            // **(252, 3, 0)** — der erste Pixel ist bei `t = 0,005` genau der,
            // an dem sich der Fehler am staerksten zeigt.
            let luecken = (r + 1 - (l - 1)) as f32;
            for k in (l - 1)..=(r + 1) {
                if pos[k].is_none() {
                    pos[k] = Some(a + (b - a) * (k - (l - 1)) as f32 / luecken);
                }
            }
            i = r + 2;
        } else {
            i += 1;
        }
    }
    // **Monoton**: CSS Images 3 §10.4 — Positionen duerfen nicht fallen.
    let mut roh: Vec<f32> = pos.into_iter().map(|v| v.unwrap_or(0.0)).collect();
    for k in 1..n {
        if roh[k] < roh[k - 1] {
            roh[k] = roh[k - 1];
        }
    }
    roh
}

/// **BILD_51: der Anteil `t` eines `radial-gradient` an einem Punkt**, oder
/// `None`, wenn der Verlauf **kein** Radialverlauf ist.
///
/// **DIE SECHS FÄLLE, und jede Zeile ist gegen Firefox gemessen** (Bild
/// `team/testseiten/radial_fluss_ff.png`, Quelle `radial_fluss.html`,
/// Kaesten 100 × 50 im Seitenraster, Mitte **vor** der Pixelmitte berechnet):
///
/// | Fall | GEMESSEN (Firefox) | Wert, den diese Formel liefert |
/// |---|---|---|
/// | `circle farthest-corner`, Mitte | Pixel (60, 35) → **(252)** | 255·(1 − 0,707/55,90) = **251,8** |
/// | `circle closest-side`, Mitte | (60, 95) → **(248)**, (60, 70) → **(5)** | r = **25** |
/// | `ellipse farthest-corner`, Mitte | (60, 140) → **(251)**, (60, 130) → **(78)** | rx = 50·√2, ry = 25·√2 |
/// | `ellipse closest-side`, Mitte | (60, 200) → **(249)**, (60, 190) → **(5)** | rx = 50, ry = 25 |
/// | `circle farthest-corner at 30% 40%` | (60, 274) → **(185)**, (60, 250) → **(160)** | Mitte **(40, 30)**, r = **76,16** |
///
/// **DER PUNKT, AN DEM ES AM EHAUFIGSTEN SCHIEF GEHT:** `√2`. Eine Ellipse, die
/// durch die Ecke laeuft, hat **beide** Halbachsen mit demselben Faktor
/// √2 — **nicht** `rx = dx` und `ry = dy` (das waere eine Ellipse, die die Ecke
/// **nicht** erreicht) und **nicht** `r = √(dx² + dy²)` (das waere ein **Kreis**
/// und damit die andere Form).
///
/// **UND DIE PIXELMITTE, wie beim linearen Verlauf:** GEMESSEN an
/// `ellipse farthest-corner`, Pixel (60, 10): der senkrechte Abstand ist
/// **24,5**, nicht 25. Mit 25 kaeme **(75)** statt **(78)** — **3** Einheiten,
/// und `√2` waere damit scheinbar widerlegt. Deshalb wird hier **immer** die
/// Pixelmitte `(x + 0,5, y + 0,5)` gerechnet, und der Aufrufer rechnet sie.
fn radial_anteil(verlauf: &Verlauf, rect: Rect, p: (f32, f32)) -> Option<f32> {
    let r = verlauf.radial.as_ref()?;
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    // **Die Mitte ist NICHT die Kastenmitte**, wenn `at 30% 40%` dasteht.
    let mx = rect.x + r.anteil_x * rect.width;
    let my = rect.y + r.anteil_y * rect.height;
    let links = mx - rect.x;
    let rechts = rect.x + rect.width - mx;
    let oben = my - rect.y;
    let unten = rect.y + rect.height - my;
    let seiten = [
        links.max(0.0),
        rechts.max(0.0),
        oben.max(0.0),
        unten.max(0.0),
    ];
    // **Die vier Ecken als (waagerecht, senkrecht)** — fuer Kreis die Strecke,
    // fuer Ellipse die beiden Halbachsen.
    let ecken = [
        (links, oben),
        (links, unten),
        (rechts, oben),
        (rechts, unten),
    ];

    use narsil_core::verlauf_typ::{RadialForm as F, RadialGroesse as G};
    let (dx, dy) = (p.0 - mx, p.1 - my);
    let vierte_wurzel_zwei = std::f32::consts::SQRT_2;

    let t = match (r.form, r.groesse) {
        // --- KREIS: **eine** Strecke, und `t` ist der Quotient aus zwei.
        (F::Kreis, G::NaechsteSeite) => {
            let q = seiten.iter().copied().fold(f32::INFINITY, f32::min);
            (dx * dx + dy * dy).sqrt() / q
        }
        (F::Kreis, G::FernsteSeite) => {
            let q = seiten.iter().copied().fold(0.0, f32::max);
            (dx * dx + dy * dy).sqrt() / q
        }
        (F::Kreis, G::NaechsteEcke) => {
            let q = ecken
                .iter()
                .map(|(a, b)| (a * a + b * b).sqrt())
                .fold(f32::INFINITY, f32::min);
            (dx * dx + dy * dy).sqrt() / q
        }
        (F::Kreis, G::FernsteEcke) => {
            let q = ecken
                .iter()
                .map(|(a, b)| (a * a + b * b).sqrt())
                .fold(0.0, f32::max);
            (dx * dx + dy * dy).sqrt() / q
        }
        // --- ELLIPSE: **zwei** Halbachsen, und `t` ist die **normierte** Strecke.
        (F::Ellipse, G::NaechsteSeite) => {
            let rx = links.min(rechts).max(1e-6);
            let ry = oben.min(unten).max(1e-6);
            ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt()
        }
        (F::Ellipse, G::FernsteSeite) => {
            let rx = links.max(rechts).max(1e-6);
            let ry = oben.max(unten).max(1e-6);
            ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt()
        }
        // **UND HIER IST √2:** die Ellipse laeuft durch **diese** Ecke, und
        // dafuer bekommt **beide** Halbachsen denselben Faktor — GEMESSEN oben.
        (F::Ellipse, G::NaechsteEcke) => {
            let (a, b) = *ecken.iter().min_by(|p, q| {
                (p.0 * p.0 + p.1 * p.1)
                    .partial_cmp(&(q.0 * q.0 + q.1 * q.1))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })?;
            let rx = a.abs() * vierte_wurzel_zwei;
            let ry = b.abs() * vierte_wurzel_zwei;
            ((dx / rx.max(1e-6)).powi(2) + (dy / ry.max(1e-6)).powi(2)).sqrt()
        }
        (F::Ellipse, G::FernsteEcke) => {
            let (a, b) = *ecken.iter().max_by(|p, q| {
                (p.0 * p.0 + p.1 * p.1)
                    .partial_cmp(&(q.0 * q.0 + q.1 * q.1))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })?;
            let rx = a.abs() * vierte_wurzel_zwei;
            let ry = b.abs() * vierte_wurzel_zwei;
            ((dx / rx.max(1e-6)).powi(2) + (dy / ry.max(1e-6)).powi(2)).sqrt()
        }
    };
    if !t.is_finite() {
        return None;
    }
    // **BILD_52: NICHT MEHR HIER KLEMMEN.** Bei `repeating-radial-gradient` muss
    // `t` ueber 1 hinauslaufen; das Klemmen gehoert **einmal** nach dem Wickeln
    // in `farbe_bei`. Sonst waere jede wiederholte Radiale ein Bild, das
    // **nach dem ersten Band** einfach die letzte Farbe traegt.
    Some(t)
}

/// **BILD_52: der Anteil `t` eines `conic-gradient` an einem Punkt** (CSS
/// Images 4 §12).
///
/// **DIE DREI FESTLEGEN, ALLE GEMESSEN** an
/// `team/testseiten/kegel_wiederholt_ff.png` (Quelle `kegel_witterholt.html`),
/// Kasten **100 × 50** bei (10, 10), Mitte **(60, 35)**, Pixel **10,5 / 35,5**:
///
/// 1. **`0deg` zeigt nach OBEN**, im Uhrzeigersinn — wie bei `linear-gradient`.
///    GEMESSEN Spalte x = 60: y = 10 → **(254, 0, 1)**, das ist `t = 0,004`.
///    Waere 0 Grad **rechts**, stuende dort `(128, 0, 127)`.
/// 2. **Der Winkel ist `atan2(dx, −dy)`** — das **Minus** ist der ganze
///    Unterschied: bei einem Punkt **oben** muss 0 herauskommen, und `dy` ist
///    dort **negativ**. GEMESSEN Pixel (10,5; 10,5): `atan2(−49,5; 24,5)` =
///    **−63,7°**, auf [0, 360) **296,3°**, `t = 0,823` → **(45, 0, 210)**.
///    GEMESSEN ist **(45, 0, 210)** — auf 3 Nachkommastellen.
/// 3. **`from <winkel>` verschiebt, es dreht nicht**: `t = ((θ − from) mod
///    360) / 360`. GEMESSEN `from 90deg`, Pixel (60,5; 95,5): θ = 135°,
///    `(135 − 90)/360 = 0,125` → **(223, 0, 32)**. **GEMESSEN: (223, 0, 32)**.
///
/// **UND AM MITTELPUNKT:** dort ist der Winkel **nicht** definiert, und Firefox
/// zeichnet **kein** Loch: GEMESSEN Pixel (60,5; 35,5), 0,5 px rechts und 0,5 px
/// unter der Mitte → θ = 135° → `t = 0,375` → **(159, 0, 96)**. **GEMESSEN:
/// (159, 0, 96)**. Wer dort `t = 0` setzt, malt einen **roten Punkt** in die
/// Mitte des Kastens.
fn conic_anteil(verlauf: &Verlauf, rect: Rect, p: (f32, f32)) -> Option<f32> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let mx = rect.x + rect.width / 2.0;
    let my = rect.y + rect.height / 2.0;
    let dx = p.0 - mx;
    let dy = p.1 - my;
    // **GEMESSEN, siehe Punkt 2 oben: das Minus an `dy`.**
    let winkel = dx.atan2(-dy).to_degrees();
    // **`rem_euclid`, nicht `%`:** ein Winkel kann **negativ** sein (-63,7°), und
    // ein negatives `t` faellt aus der Stoppsuche heraus. CSS Images 4 §12:
    // der Winkel laeuft **immer** weiter, er wird nicht abgeschnitten.
    let t = (winkel - verlauf.winkel).rem_euclid(360.0) / 360.0;
    if !t.is_finite() {
        return None;
    }
    Some(t)
}

/// Die **Farbe eines Pixels**, oder `None`, wenn dieser Verlauf den Fall nicht
/// abdeckt (fremde Art, `wiederholt`).
///
/// **DER WICHTIGSTE TEIL:** die Interpolation laeuft ueber die
/// **premultiplizierten** Farben, und das Ergebnis wird **einmal** ueber den
/// Hintergrund gemischt — in dieser Reihenfolge. GEMESSEN: `#ff000000 →
/// #0000ffff` ueber `#2040c0` ergibt bei `x = 50` **(16, 32, 224)**; **ohne**
/// Multiplikation kaeme **(79, 32, 160)**.
#[must_use]
pub fn farbe_bei(verlauf: &Verlauf, rect: Rect, p: (f32, f32), grund: Color) -> Option<Color> {
    if verlauf.stopps.len() < 2 {
        return None;
    }
    farbe_bei_vorab(verlauf, &Vorab::neu(verlauf, rect), rect, p, grund)
}

/// Release 09.10.2026: was je Verlauf und Kasten EINMAL gerechnet wird.
///
/// GEMESSEN vorher (Release-Bau, 1100 x 700): die Startseite brauchte
/// 155 ms und die Einstellungsseite 212 ms nur fuer ihre Verlaeufe —
/// `farbe_bei` legte JE PIXEL die Stoppliste neu an (`Vec`) und rechnete
/// Richtung und Linienlaenge (Winkelfunktionen) neu. Die Rechnung bleibt
/// dieselbe, nur einmal statt je Pixel.
struct Vorab {
    pos: Vec<f32>,
    /// Linear: (dx, dy, laenge) — `None` bei Laenge 0.
    linear: Option<(f32, f32, f32)>,
}

impl Vorab {
    fn neu(verlauf: &Verlauf, rect: Rect) -> Self {
        let linear = if verlauf.art == VerlaufArt::Linear {
            let (dx, dy) = richtung(verlauf.winkel);
            let laenge = linien_laenge(rect.width, rect.height, verlauf.winkel);
            (laenge > 0.0).then_some((dx, dy, laenge))
        } else {
            None
        };
        Self { pos: stopp_positionen(verlauf), linear }
    }
}

fn farbe_bei_vorab(verlauf: &Verlauf, vorab: &Vorab, rect: Rect, p: (f32, f32), grund: Color) -> Option<Color> {
    let n = verlauf.stopps.len();
    if n < 2 {
        return None;
    }
    let pos = &vorab.pos;

    // **BILD_51: DIE ART ENTSCHEIDET UEBER `t`, NICHT UEBER DIE FARBE.**
    // Die Farbsuche darunter (Segment, harte Kante, Premultiplizieren) ist
    // **beiden** Arten gemeinsam — und genau das ist der Grund, warum der
    // Radialanteil eine eigene Funktion ist und kein zweiter Farbpfad.
    // **BILD_52: HIER WIRD NICHT GEKLEMMT.** Bei `wiederholt` muss `t` ueber 1
    // hinauslaufen duerfen — das **ist** die Wiederholung. Erst nach dem Wickeln
    // wird geklemmt.
    let t_roh = match verlauf.art {
        VerlaufArt::Linear => {
            let (dx, dy, laenge) = vorab.linear?;
            let mx = rect.x + rect.width / 2.0;
            let my = rect.y + rect.height / 2.0;
            // `t` ueber die **Pixelmitte** — GEMESSEN, siehe Kopf dieser Datei.
            (p.0 - mx) * dx / laenge + (p.1 - my) * dy / laenge + 0.5
        }
        VerlaufArt::Radial => radial_anteil(verlauf, rect, p)?,
        // **BILD_52: der Kegel.** GEMESSEN in `conic_anteil`, drei Festlegungen
        // mit den Werten daneben.
        VerlaufArt::Kegel => conic_anteil(verlauf, rect, p)?,
    };

    // **BILD_52: DIE WIEDERHOLUNG, und sie ist eine einzige Zeile Rechnung.**
    //
    // **GEMESSEN** an `repeating-linear-gradient(90deg, #f00 0 10px, #00f 10px
    // 20px)` auf 100 px: die Baender wechseln alle **20 px**, nicht alle 10 —
    // die **Periode ist die Spanne vom ersten bis zum letzten Stopp**
    // (`20px − 0px`), und `10px` ist die **Farbgrenze innerhalb** einer Periode.
    // Wer die Periode als „Abstand zweier benachbarter Stopps“ nimmt, macht
    // **halb** so viele Baender und bekommt **jede** Farbe an **anderer** Stelle.
    let t = if verlauf.wiederholt {
        let spanne = pos[n - 1] - pos[0];
        if spanne <= f32::EPSILON {
            // **Alle Stopps an einer Stelle:** es gibt nichts zu wiederholen,
            // und `rem_euclid(0)` waere eine Division durch Null.
            return None;
        }
        pos[0] + (t_roh - pos[0]).rem_euclid(spanne)
    } else {
        t_roh
    };
    let t = t.clamp(0.0, 1.0);

    // **Das Segment**, in dem `t` liegt.
    let mut seg = n - 2;
    for k in 0..(n - 1) {
        if t < pos[k + 1] {
            seg = k;
            break;
        }
    }
    // **Harte Kante:** gleiche Positionen -> der Sprung liegt **auf** der
    // Position, und `t >= pos` gehoert **schon** zur naechsten Farbe. GEMESSEN:
    // x = 49 (t = 0.495) ist rot, x = 50 (t = 0.505) ist blau.
    let (c0, c1, u) = if (pos[seg + 1] - pos[seg]).abs() < f32::EPSILON {
        let which = if t >= pos[seg] { seg + 1 } else { seg };
        (verlauf.stopps[which], verlauf.stopps[which], 0.0)
    } else {
        let u = ((t - pos[seg]) / (pos[seg + 1] - pos[seg])).clamp(0.0, 1.0);
        (verlauf.stopps[seg], verlauf.stopps[seg + 1], u)
    };
    let w0 = 1.0 - u;
    let w1 = u;

    // **PREMULTIPLIZIERT:** die Farbe wird mit ihrem eigenen Alpha gewichtet.
    let a0 = f32::from(c0.farbe.a) / 255.0;
    let a1 = f32::from(c1.farbe.a) / 255.0;
    let alpha = a0 * w0 + a1 * w1;
    // Beide Terme in **0…255** — ein `/255` hier liefert 0…1 und die Mischung
    // weiter unten rechnet in 0…255 (das war mein Fehler, GEMESSENE WIRKUNG
    // (16, 32, 96) statt (16, 32, 224)).
    let mische = |c0: u8, c1: u8| -> f32 { f32::from(c0) * a0 * w0 + f32::from(c1) * a1 * w1 };
    let pr = mische(c0.farbe.r, c1.farbe.r);
    let pg = mische(c0.farbe.g, c1.farbe.g);
    let pb = mische(c0.farbe.b, c1.farbe.b);

    // **DIE RUNDUNG: .5 NACH GERADE, nicht weg von null.**
    // **GEMESSEN** an `45deg`, Pixel (56, 1): Firefox **76**, wir mit
    // `f32::round` **77** — der Wert war genau **76,5**. Rust rundet `.5` **weg
    // von null**, Firefox **auf die gerade Zahl** — und das ist an **jedem**
    // Kanal **jedes** Bildes ein moeglicher Unterschied von **1**.
    let rgbfn = |v: f32, d: u8| -> u8 {
        (v + (1.0 - alpha) * f32::from(d))
            .round_ties_even()
            .clamp(0.0, 255.0) as u8
    };
    let da = f32::from(grund.a) / 255.0;
    Some(Color {
        r: rgbfn(pr, grund.r),
        g: rgbfn(pg, grund.g),
        b: rgbfn(pb, grund.b),
        a: ((alpha + (1.0 - alpha) * da) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8,
    })
}

/// Malt den Verlauf in einen Framebuffer und **zaehlt** die gemalten Pixel.
///
/// **WER NICHT GEMALT WIRD:** alles, was `farbe_bei` mit `None` beantwortet —
/// fremde Art, andere Stoppzahl, Stopps ohne Position, Hartkante,
/// `wiederholt`. Siehe Kopf.
pub fn zeichne(fb: &mut Framebuffer, rect: Rect, radius: impl Into<crate::formen::Corners>, verlauf: &Verlauf) -> usize {
    let radius: crate::formen::Corners = radius.into();
    let b = rect.width.ceil().max(0.0) as u64;
    let h = rect.height.ceil().max(0.0) as u64;
    if b == 0 || h == 0 || b.saturating_mul(h) > MAX_PIXEL {
        return 0;
    }
    let x0 = rect.x.floor().max(0.0) as u32;
    let y0 = rect.y.floor().max(0.0) as u32;
    let x1 = ((rect.x + rect.width).ceil() as u32).min(fb.width());
    let y1 = ((rect.y + rect.height).ceil() as u32).min(fb.height());
    let clip = fb.effektiver_clip();
    let mut gemalt = 0usize;
    if verlauf.stopps.len() < 2 {
        return 0;
    }
    // Release 09.10.2026: einmal vorrechnen (siehe `Vorab`), Clip einmal
    // schneiden statt je Pixel pruefen, Ecken kantengeglaettet.
    let vorab = Vorab::neu(verlauf, rect);
    let x0 = x0.max(clip.x.floor().max(0.0) as u32);
    let y0 = y0.max(clip.y.floor().max(0.0) as u32);
    let x1 = x1.min(clip.right().ceil().max(0.0) as u32);
    let y1 = y1.min(clip.bottom().ceil().max(0.0) as u32);
    let rund = radius.x.iter().chain(radius.y.iter()).any(|r| *r > 0.0);
    let form_radius = radius.fit(rect.width, rect.height);
    let kasten = Rect::new(0.0, 0.0, rect.width, rect.height);
    for y in y0..y1 {
        for x in x0..x1 {
            // `set_pixel` **ueberschreibt**; der Hintergrund wird gelesen und
            // die Mischung rechnet ihn ein.
            let grund = fb.pixel(x, y).unwrap_or(Color::TRANSPARENT);
            // Die **Pixelmitte** — GEMESSEN, siehe Kopf.
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            //
            // **BILD_49, PUNKT 3: DIE RUNDUNG.** `DisplayItem::Verlauf` traegt
            // `radius`, und bis hierher wurde er **weggeworfen** — der Verlauf in
            // einem Kasten mit `border-radius` waere **eckig** gezeichnet worden.
            // **GEMESSEN, warum das eine echte Luecke ist:** mein Test
            // `verlauf_item.rs` (BILD_49 Station 3) **besteht darauf**, dass das
            // Item den Radius **traegt** — und der Zeichner hat ihn **nicht**
            // benutzt. Ein Item-Feld, das niemand liest, ist eine halbe Station.
            //
            // **DIE REGEL IST DIE DES HINTERGRUNDS:** `border-radius` schneidet
            // **jede** Fuellung desselben Kastens an **denselben** Kanten ab —
            // das ist derselbe Weg, den `formen::kasten_mit_radius` fuer den
            // Hintergrund geht (CSS Backgrounds 3 §6.1). **Wir sind also nicht
            // strenger als der Hintergrund**, sondern **genauso**.
            //
            // **UND: `radius == 0` bleibt der alte Weg**, ohne einen Aufruf —
            // das ist der Normalfall und darf nicht teurer werden.
            let deckung = if rund {
                (0.5 - crate::formen::distance_xy(p.0 - rect.x, p.1 - rect.y, kasten, &form_radius)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            if deckung <= 0.0 {
                continue;
            }
            if let Some(f) = farbe_bei_vorab(verlauf, &vorab, rect, p, grund) {
                // Am Rand der Rundung anteilig ueber den Grund.
                let f = if deckung < 1.0 {
                    let k = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * deckung).round() as u8;
                    Color { r: k(grund.r, f.r), g: k(grund.g, f.g), b: k(grund.b, f.b), a: k(grund.a, f.a) }
                } else {
                    f
                };
                if fb.set_pixel(x, y, f) {
                    gemalt += 1;
                }
            }
        }
    }
    gemalt
}
