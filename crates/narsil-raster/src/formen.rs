//! RUNDE 3, S2 — `opacity`, `border-radius` und `box-shadow` ZEICHNEN.
//!
//! Die Werte kommen aus `narsil-css` (`Berechnet::deckkraft`, `::radius`,
//! `::schatten`, K6/K7/K8, 01.10.2026 04:00, Liste in `team/WUNSCH_CSS.txt`).
//! Dieses Modul ist die andere Seite davon: der Rasterizer.
//!
//! # Warum hier und nicht in der Kaskade
//!
//! `border-radius` ist eine Form, keine Farbe: sie kann nicht durch
//! Multiplizieren zweier Kanäle entstehen, sie braucht eine Entscheidung je
//! Pixel. `box-shadow` ist ein zweidimensionales Gauss, kein Abstand. Und
//! `opacity` muss die Kinder einschliessen — auch wenn sie an anderer Stelle
//! gezeichnet wurden. Alle drei brauchen den Framebuffer.
//!
//! # Die Zahlen sind gemessen, nicht geraten
//!
//! * `radius` wird nach CSS Backgrounds 3 §5.5 auf `min(breite, hoehe)/2`
//!   BEGRENZT und bei gemischten Ecken proportional verkleinert, nicht
//!   beschnitten.
//! * `weich` ist ein Gauß-Sigma in px. Der 2-Sigma-Bereich deckt rund 95 %
//!   der Energie (`exp(-2) = 0,135` je Seite, also 13,5 % ausserhalb —
//!   beidseitig 27 %, das ist die uebliche 2-Sigma-Naeherung; fuer 95 %
//!   zweitseitig braucht man 2,45 Sigma, siehe [`sigma_aus_weich`]).
//! * `opacity: 2` wird auf 1 GEKLEMMT, nicht abgewiesen (CSS Color 4 §3.2
//!   sagt "clamped").
//!
//! # Grenzen (M3), alle benannt
//!
//! Der Framebuffer ist 1280x900 im Projekt, ein Kasten kann aber groesser
//! sein. Jede Schleife ist gegen `MAX_KASTEN` begrenzt, und ein Kasten
//! ausserhalb liefert 0 Pixel statt einen Absturz.

use narsil_core::color::Color;
use narsil_core::geometry::Rect;
use narsil_core::style::{MAX_SCHATTEN, SchattenArt};

use crate::framebuffer::Framebuffer;

/// M3: hoechste Kantenlaenge eines Kastens, den diese Funktionen
/// zeichnen. Reicht fuer den groessten real benutzten Framebuffer
/// (1280x900) mit Rand und ueber several Verschiebungen.
pub const MAX_KASTEN: i64 = 8192;

/// M3: hoechste Zahl Pixel, die eine Operation an einem Kasten fasst.
/// Bei 8192x8192 sind das 67 Mio — das ist die Grenze, ab der ein
/// einzelnes Rechteck den Bildaufbau blockieren wuerde.
pub const MAX_PIXEL: i64 = 8192 * 8192;

/// M3: hoechster Radius, der gezeichnet wird. CSS begrenzt ihn selbst auf
/// `min(breite, hoehe)/2`; diese Grenze faengt zusaetzlich nicht-endliche
/// Werte ab.
pub const MAX_RADIUS: f32 = 4096.0;

/// M3: hoechstes Sigma des Gauss-Schattens. Weicht man weiter, sieht man
/// nichts mehr, und die Schleife waere nur langsamer.
pub const MAX_WEICH: f32 = 512.0;

/// Rechnet die Eckradien in die Form, die wirklich gezeichnet wird.
///
/// CSS Backgrounds 3 §5.5: die Summe der Radien JE Achse darf die Kanten-
/// laenge nicht uebersteigen; sind die Radien verschieden, werden sie
/// **proportional** auf den verbleibenden Platz verkleinert — nicht
/// beschnitten. Ein Kasten 40 breit und 10 hoch mit `radius = 8` an allen
/// Ecken bekommt also oben 5 und an den Seiten 5, nicht 8 und 8.
///
/// `radius` ist oben-links, oben-rechts, unten-rechts, unten-links
/// (die Reihenfolge aus `Berechnet::radius`).
#[must_use]
pub fn radius_form(radius: [f32; 4], breite: f32, hoehe: f32) -> [f32; 4] {
    if !breite.is_finite() || !hoehe.is_finite() || breite <= 0.0 || hoehe <= 0.0 {
        return [0.0; 4];
    }
    let mut r = [
        radius[0].max(0.0),
        radius[1].max(0.0),
        radius[2].max(0.0),
        radius[3].max(0.0),
    ];
    // M2/M3: nicht-endliche Werte waeren ein stiller NaN-Pfad.
    for v in r.iter_mut() {
        if !v.is_finite() {
            *v = 0.0;
        }
        *v = v.min(MAX_RADIUS);
    }
    // Kurzschluss: passt alles, ist nichts zu verkleinern.
    let summe_x = r[0] + r[1];
    let summe_y = r[2] + r[3];
    if summe_x <= breite && summe_y <= hoehe {
        return r;
    }
    // Faktor je Achse: das ist der Clamp, den die Spec als "falls die
    // Summe der Radien einer Kante deren Laenge uebersteigt, muss der
    // Faktor ... angewendet werden" beschreibt.
    let f_x = if summe_x > breite {
        breite / summe_x
    } else {
        1.0
    };
    let f_y = if summe_y > hoehe {
        hoehe / summe_y
    } else {
        1.0
    };
    // Obere Ecke von oben, untere von unten; links gehoert zu oben UND
    // unten, deshalb der kleinere der beiden Faktoren.
    r[0] *= f_x.min(f_y);
    r[1] *= f_x.min(f_y);
    r[2] *= f_x.min(f_y);
    r[3] *= f_x.min(f_y);
    r
}

/// Release 15.10.2026 (Agent F): the corner radii of a box (CSS
/// Backgrounds 3 §5.1): horizontal `x` and vertical `y`, in CSS order
/// (top-left, top-right, bottom-right, bottom-left). Equal arrays are
/// circular corners; a `[f32; 4]` converts to those.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Corners {
    pub x: [f32; 4],
    pub y: [f32; 4],
}

impl From<[f32; 4]> for Corners {
    fn from(r: [f32; 4]) -> Self {
        Corners { x: r, y: r }
    }
}

impl Corners {
    /// Horizontal and vertical radii of every corner are equal.
    #[must_use]
    pub fn is_circular(&self) -> bool {
        self.x == self.y
    }

    /// Some corner is rounded.
    #[must_use]
    pub fn is_rounded(&self) -> bool {
        (0..4).any(|i| self.x[i] > 0.0 && self.y[i] > 0.0)
    }

    /// The radii that are drawn (CSS Backgrounds 3 §5.5): a corner with a
    /// zero radius is square, and radii that do not fit are scaled down
    /// together. Circular corners take exactly [`radius_form`].
    #[must_use]
    pub fn fit(self, width: f32, height: f32) -> Corners {
        if self.is_circular() {
            return radius_form(self.x, width, height).into();
        }
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Corners::default();
        }
        let clean = |v: f32| if v.is_finite() { v.clamp(0.0, MAX_RADIUS) } else { 0.0 };
        let mut c = Corners { x: self.x.map(clean), y: self.y.map(clean) };
        for i in 0..4 {
            if c.x[i] <= 0.0 || c.y[i] <= 0.0 {
                c.x[i] = 0.0;
                c.y[i] = 0.0;
            }
        }
        let ratio = |len: f32, sum: f32| if sum > len { len / sum } else { 1.0 };
        let f = ratio(width, c.x[0] + c.x[1])
            .min(ratio(width, c.x[3] + c.x[2]))
            .min(ratio(height, c.y[0] + c.y[3]))
            .min(ratio(height, c.y[1] + c.y[2]));
        if f < 1.0 {
            c.x = c.x.map(|v| v * f);
            c.y = c.y.map(|v| v * f);
        }
        c
    }

    /// The corners `spread` px further out (a positive spread grows a
    /// rounded corner, a square corner stays square).
    #[must_use]
    pub fn spread(self, spread: f32) -> Corners {
        let grow = |v: f32| if v > 0.0 { (v + spread).max(0.0) } else { 0.0 };
        Corners { x: self.x.map(grow), y: self.y.map(grow) }
    }
}

/// Release 15.10.2026 (Agent F): signed distance of (`px`, `py`) to the box
/// `r` with the (fitted) corners `c`; negative inside. Circular corners
/// take exactly `glas::abstand`; an elliptical corner uses the gradient
/// approximation of the distance to its ellipse.
#[must_use]
pub fn distance_xy(px: f32, py: f32, r: Rect, c: &Corners) -> f32 {
    if c.is_circular() {
        return crate::glas::abstand(px, py, r, c.x);
    }
    let hb = r.width / 2.0;
    let hh = r.height / 2.0;
    let i = match (px > r.x + hb, py > r.y + hh) {
        (false, false) => 0,
        (true, false) => 1,
        (true, true) => 2,
        (false, true) => 3,
    };
    let ax = if c.x[i].is_finite() { c.x[i].clamp(0.0, hb) } else { 0.0 };
    let ay = if c.y[i].is_finite() { c.y[i].clamp(0.0, hh) } else { 0.0 };
    let qx = (px - (r.x + hb)).abs();
    let qy = (py - (r.y + hh)).abs();
    if ax > 0.0 && ay > 0.0 {
        let dx = qx - (hb - ax);
        let dy = qy - (hh - ay);
        if dx > 0.0 && dy > 0.0 {
            let (nx, ny) = (dx / ax, dy / ay);
            let k0 = (nx * nx + ny * ny).sqrt();
            let (gx, gy) = (dx / (ax * ax), dy / (ay * ay));
            let k1 = (gx * gx + gy * gy).sqrt();
            return if k1 > 0.0 { k0 * (k0 - 1.0) / k1 } else { -ax.min(ay) };
        }
    }
    let ex = qx - hb;
    let ey = qy - hh;
    (ex.max(0.0).powi(2) + ey.max(0.0).powi(2)).sqrt() + ex.max(ey).min(0.0)
}

/// Release 15.10.2026 (Agent F): is the point (relative to the box's top
/// left corner) inside the box with the (fitted) corners `c`? Circular
/// corners take exactly [`punkt_im_kasten`].
#[must_use]
pub fn point_in_box_xy(x: f32, y: f32, width: f32, height: f32, c: &Corners) -> bool {
    if c.is_circular() {
        return punkt_im_kasten(x, y, width, height, c.x);
    }
    if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 || x > width || y > height {
        return false;
    }
    // Corner i: the ellipse centre and its radii.
    let centres = [
        (c.x[0], c.y[0], c.x[0], c.y[0]),
        (width - c.x[1], c.y[1], c.x[1], c.y[1]),
        (width - c.x[2], height - c.y[2], c.x[2], c.y[2]),
        (c.x[3], height - c.y[3], c.x[3], c.y[3]),
    ];
    for (i, (mx, my, ax, ay)) in centres.into_iter().enumerate() {
        if ax <= 0.0 || ay <= 0.0 {
            continue;
        }
        let in_zone = match i {
            0 => x < mx && y < my,
            1 => x > mx && y < my,
            2 => x > mx && y > my,
            _ => x < mx && y > my,
        };
        if in_zone {
            let (nx, ny) = ((x - mx) / ax, (y - my) / ay);
            if nx * nx + ny * ny > 1.0 {
                return false;
            }
        }
    }
    true
}

/// `true`, wenn der Punkt (x, y) — relativ zur linken oberen Kastenecke —
/// innerhalb eines Kastens mit den Eckradien `r` liegt.
///
/// Fuer `r = 0` ist das genau das Rechteck. Sonst wird an jeder Ecke der
/// Abstand zum Mittelpunkt des Schnittkreises geprueft.
#[must_use]
pub fn punkt_im_kasten(x: f32, y: f32, breite: f32, hoehe: f32, r: [f32; 4]) -> bool {
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    if x < 0.0 || y < 0.0 || x > breite || y > hoehe {
        return false;
    }
    // GEMESSEN 03.10.2026: die erste Fassung pruefte ALLE VIER Bedingungen
    // gegen `r[0]`. Bei einem Radius nur oben links ist `r[1] = 0`, damit
    // las jede Bedingung als `Abstand^2 <= 0` — und ein Punkt oben rechts
    // wurde aus der Form GEKIPPT, obwohl diese Ecke gar keine Rundung hat.
    //
    // Die Regel, die stimmt: zu jeder Ecke gehoert ein QUADRAT der Seite
    // `r_c` (oben-links [0,r]x[0,r], oben-rechts [w-r,w]x[0,r], ...) und
    // ein Punkt, der darin liegt, muss im Viertelkreis um (Eckenmitte)
    // liegen. Ausserhalb des Quadrats ist die Ecke fuer den Punkt nicht
    // zustaendig — dort gilt einfach das Rechteck.
    //
    // (x_min, y_min, x_max, y_max, mitte_x, mitte_y, radius)
    let ecken: [(f32, f32, f32, f32, f32, f32, f32); 4] = [
        (0.0, 0.0, r[0], r[0], r[0], r[0], r[0]),
        (breite - r[1], 0.0, breite, r[1], breite - r[1], r[1], r[1]),
        (
            breite - r[2],
            hoehe - r[2],
            breite,
            hoehe,
            breite - r[2],
            hoehe - r[2],
            r[2],
        ),
        (0.0, hoehe - r[3], r[3], hoehe, r[3], hoehe - r[3], r[3]),
    ];
    for (x0, y0, x1, y1, mx, my, rad) in ecken {
        if rad <= 0.0 {
            continue; // Ecke ist eckig — sie begrenzt nichts
        }
        if x >= x0 && x <= x1 && y >= y0 && y <= y1 {
            let dx = x - mx;
            let dy = y - my;
            if dx.mul_add(dx, dy.mul_add(dy, 0.0)) > rad * rad {
                return false;
            }
        }
    }
    true
}

/// Fuellt einen Framebuffer-Bereich mit einer Farbe, mit weichen Ecken.
///
/// Beruecksichtigt `deckkraft`: CSS Color 4 §3.2 nennt `opacity` eine
/// ELEMENTEIGENSCHAFT, die nicht vererbt wird, aber auf alle Kinder wirkt.
/// Deshalb wird hier die Deckkraft auf jeden gesetzten Pixel angewendet,
/// nicht auf den ganzen Kasten nachher.
///
/// Rueckgabe: Anzahl tatsaechlich gesetzter Pixel.
pub fn kasten_mit_radius(
    fb: &mut Framebuffer,
    rect: Rect,
    radius: impl Into<Corners>,
    farbe: Color,
    deckkraft: f32,
) -> u32 {
    let radius: Corners = radius.into();
    // GEMESSEN 03.10.2026: die erste Fassung pruefte NUR Breite und Hoehe
    // auf Endlichkeit. Ein `Rect { x: NaN, .. }` kam durch und zeichnete
    // 80 Pixel an Position 0 — ein Kasten, den es nicht gibt, hat etwas
    // gemalt.
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
    {
        return 0;
    }
    let d = deckkraft_klemmen(deckkraft);
    if d <= 0.0 || farbe.a == 0 {
        return 0;
    }
    //
    // GEMESSEN 03.10.2026: `deckkraft` war ein TOTER PARAMETER. Die
    // Deckkraft wurde berechnet (`deckkraft_klemmen`) und dann nicht mehr
    // benutzt — der Kasten bekam immer seine volle Farbe. GEMESSEN im Bild
    // crates/narsil-page/examples/s2_bild.rs: vier Kaesten mit `deckkraft`
    // 1.0, 0.75, 0.5 und 0.25 ergaben ALLE exakt (217, 27, 59).
    //
    // Warum kein Test das fand: der Test daneben heisst
    // `halbe_deckkraft_halbiert_den_alpha_aller_pixel` und prueft
    // `deckkraft_skalieren` — eine ANDERE Funktion, die schon gezeichnete
    // Pixel nachtraeglich skaliert. Zwei Funktionen mit aehnlichem Namen,
    // und getestet wurde die falsche. Das Bild hats gezeigt.
    //
    // CSS Color 4 §3.2: `opacity` ist eine Gruppen-Deckkraft. Hier wirkt
    // sie auf die ENGEFARBE dieses Kastens; die Kinderform liegt bei
    // narsil-layout-engine.
    let mut ziel = farbe;
    ziel.a = (farbe.a as f32 * d).round() as u8;
    if ziel.a == 0 {
        return 0;
    }
    // Mischer statt `set_pixel`: bei `deckkraft < 1` ist die Farbe
    // teilweise deckend, und `set_pixel` wuerde den Hintergrund ersetzen
    // statt sich mit ihm zu mischen. Fuer deckende Farben liefert
    // `blend_source_over` genau die Farbe selbst, also aendert sich am
    // deckenden Fall nichts.
    let mischen = ziel.a < 255;
    let r = radius.fit(rect.width, rect.height);
    let bw = rect.width.round() as i64;
    let bh = rect.height.round() as i64;
    if bw <= 0 || bh <= 0 {
        return 0;
    }
    // M3/M5: begrenzt vor der Schleife, nicht danach.
    if bw > MAX_KASTEN || bh > MAX_KASTEN || bw.saturating_mul(bh) > MAX_PIXEL {
        return 0;
    }
    // GEMESSEN 03.10.2026: `set_pixel` prueft nur die Framebuffer-Grenzen.
    // `rasterize_bereich` legt aber vorher einen Clipp auf den Stack, und
    // ein Rechteck ganz ausserhalb dieses Bereichs wurde trotzdem
    // gezeichnet. Deshalb wird das Rechteck hier am effektiven Clipp
    // geschnitten — an EINER Stelle, damit jede neue Zeichenfunktion
    // davon profitiert (siehe `Framebuffer::effektiver_clip`).
    let clip = fb.effektiver_clip();
    if clip.width <= 0.0 || clip.height <= 0.0 {
        return 0;
    }
    let x0 = rect.x.floor() as i64;
    let y0 = rect.y.floor() as i64;
    // Der Bereich in Kasten-Koordinaten, der den Clipp ueberdeckt.
    let cx0 = ((clip.x.floor() as i64) - x0).clamp(0, bw);
    let cy0 = ((clip.y.floor() as i64) - y0).clamp(0, bh);
    let cx1 = ((clip.right().ceil() as i64) - x0).clamp(0, bw);
    let cy1 = ((clip.bottom().ceil() as i64) - y0).clamp(0, bh);
    if cx1 <= cx0 || cy1 <= cy0 {
        return 0;
    }
    let mut gesetzt = 0u32;
    // Release 08.10.2026: ausserhalb der Eckenzonen liegt die ganze Zeile in
    // der Form — sie wird als Spanne gefuellt statt Pixel fuer Pixel.
    let ecke_oben = r.y[0].max(r.y[1]);
    let ecke_unten = r.y[2].max(r.y[3]);
    for y in cy0..cy1 {
        let my = y as f32 + 0.5;
        if my >= ecke_oben && my <= bh as f32 - ecke_unten && x0 + cx0 >= 0 && y0 + y >= 0 {
            gesetzt += fb.spanne_fuellen((y0 + y) as u32, (x0 + cx0) as u32, (x0 + cx1) as u32, ziel, mischen);
            continue;
        }
        // Release 09.10.2026: KANTENGEGLAETTETE RUNDUNG. Vorher entschied der
        // Pixelmittelpunkt allein (`punkt_im_kasten`): drinnen volle Farbe,
        // draussen nichts — an jeder Rundung eine Treppe. Jetzt deckt die
        // Abstandsfunktion der gerundeten Form den Randpixel anteilig
        // (1 px Kantenbreite); innen bleibt es exakt wie bisher.
        let kasten = Rect::new(0.0, 0.0, bw as f32, bh as f32);
        for x in cx0..cx1 {
            let d = distance_xy(x as f32 + 0.5, y as f32 + 0.5, kasten, &r);
            let deckung = (0.5 - d).clamp(0.0, 1.0);
            if deckung <= 0.0 {
                continue;
            }
            let px = (x0 + x) as u32;
            let py = (y0 + y) as u32;
            if deckung < 1.0 {
                let teil = Color::new(ziel.r, ziel.g, ziel.b, (f32::from(ziel.a) * deckung).round() as u8);
                // Gezaehlt wird ein Randpixel, wenn sein Mittelpunkt in der
                // Form liegt (Deckung ab 1/2) — wie bisher: die Zahl bleibt
                // die Flaeche, nicht die Zahl der beruehrten Pixel.
                if teil.a > 0
                    && let Some(alt) = fb.pixel(px, py)
                    && fb.set_pixel(px, py, crate::framebuffer::blend_source_over(teil, alt))
                    && deckung >= 0.5
                {
                    gesetzt += 1;
                }
                continue;
            }
            let geschrieben = if mischen {
                match fb.pixel(px, py) {
                    Some(alt) => {
                        let gemischt = crate::framebuffer::blend_source_over(ziel, alt);
                        fb.set_pixel(px, py, gemischt)
                    }
                    None => false,
                }
            } else {
                fb.set_pixel(px, py, ziel)
            };
            if geschrieben {
                gesetzt += 1;
            }
        }
    }
    gesetzt
}

/// Multipliziert die Deckkraft auf alle Pixel in einem Bereich.
///
/// Das ist der Fall, auf den `team/WUNSCH_CSS.txt` W8.1 hinweist: `opacity`
/// wirkt auf die Kinder. Wenn die Kinder schon gezeichnet sind, muss ihr
/// Alpha hierher skaliert werden.
///
/// Rueckgabe: Anzahl tatsaechlich geaenderter Pixel.
pub fn deckkraft_skalieren(
    fb: &mut Framebuffer,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    f: f32,
) -> u32 {
    let d = deckkraft_klemmen(f);
    if d >= 1.0 {
        return 0; // Nichts zu tun — und kein "0 geaendert" als Erfolg zaehlen.
    }
    if d <= 0.0 {
        return 0;
    }
    let (xa, ya) = (x0.max(0), y0.max(0));
    let (xb, yb) = (
        x1.min(i64::from(fb.width())),
        y1.min(i64::from(fb.height())),
    );
    if xb <= xa || yb <= ya {
        return 0;
    }
    if (xb - xa).saturating_mul(yb - ya) > MAX_PIXEL {
        return 0;
    }
    let mut geaendert = 0u32;
    for y in ya..yb {
        for x in xa..xb {
            if x < 0 || y < 0 {
                continue;
            }
            let (Some(r), Some(g), Some(b), Some(a)) = (
                fb.pixel(x as u32, y as u32).map(|c| c.r),
                fb.pixel(x as u32, y as u32).map(|c| c.g),
                fb.pixel(x as u32, y as u32).map(|c| c.b),
                fb.pixel(x as u32, y as u32).map(|c| c.a),
            ) else {
                continue;
            };
            if a == 0 {
                continue;
            }
            let neu = ((a as f32) * d).round().clamp(0.0, 255.0) as u8;
            if neu != a {
                geaendert += 1;
            }
            fb.set_pixel(x as u32, y as u32, Color { r, g, b, a: neu });
        }
    }
    geaendert
}

/// CSS Color 4 §3.2: `opacity` wird GEKLEMMT, nicht abgewiesen. Negative
/// Werte und 2 ergeben beide 0 bzw. 1. NaN ergibt 1 — ein nicht-endlicher
/// Wert darf kein stiller NaN-Pfad sein.
#[must_use]
pub fn deckkraft_klemmen(f: f32) -> f32 {
    if f.is_nan() {
        return 1.0;
    }
    f.clamp(0.0, 1.0)
}

/// Das Sigma eines Gauss-Schattens aus `blur`.
///
/// CSS Backgrounds 3 §5.3 definiert `blur` als die **Standardabweichung**
/// des Gauss, nicht als seinen 2-Sigma-Durchmesser. Wer `weich` als Radius
/// nimmt, bekommt einen zu weichen Schatten.
///
/// M3: nicht-endlich oder negativ ergibt 0 (kein Schatten).
#[must_use]
pub fn sigma_aus_weich(weich: f32) -> f32 {
    // GEMESSEN 03.10.2026: die erste Fassung gab bei `INFINITY` 0 zurueck,
    // weil `!is_finite()` NaN UND Unendlich zusammenwarf. Das ist dieselbe
    // Falle wie bei `deckkraft_klemmen`: NaN wird verworfen, Unendlich
    // wird GEKLEMMT.
    if weich.is_nan() {
        return 0.0;
    }
    if weich == f32::INFINITY {
        return MAX_WEICH;
    }
    if weich <= 0.0 {
        return 0.0;
    }
    (weich / 2.0).min(MAX_WEICH)
}

/// Der Gewichtsfaktor eines Gauss-Pixels: `exp(-d^2 / (2 sigma^2))`.
///
/// `sigma == 0` ergibt 1 fuer `d == 0` und 0 sonst — das ist die harte
/// Kante ohne Weichzeichnen, was CSS fuer `blur: 0` verlangt.
#[must_use]
pub fn gauß(sigma: f32, abstand: f32) -> f32 {
    // GEMESSEN 03.10.2026: `sigma <= 0.0` ist bei NaN FALSCH, also lief
    // `exp(NaN)` durch und lieferte NaN. NaN muss ausdruecklich abgefangen
    // werden — genau die Lektion aus `deckkraft_klemmen`.
    if sigma.is_nan() || abstand.is_nan() {
        return 0.0;
    }
    if sigma <= 0.0 {
        return if abstand == 0.0 { 1.0 } else { 0.0 };
    }
    let d2 = abstand * abstand;
    (-d2 / (2.0 * sigma * sigma)).exp()
}

// ---------------------------------------------------------------------------
// S2 Station 4: `box-shadow` ZEICHNEN. Das war der einzige fehlende Baustein.
// ---------------------------------------------------------------------------

/// M3: hoechste Kantenlaenge eines Schattenrahmens.
///
/// Warum ein eigener Wert und nicht `MAX_KASTEN`: ein Schatten wird mit
/// Versatz, Ausbreitung UND Weichzeichnung gezeichnet. Bei
/// `MAX_KASTEN` als Kantenlaenge des RAHMENS waere die Innenkastenlaenge
/// `MAX_KASTEN - 2*(versatz + ausbreitung + 3*sigma)`, und bei
/// `weich = 40` (sigma 20, drei Sigma 60) waere die schon negativ — der
/// Schatten waere unsichtbar, obwohl er gezeichnet werden sollte. Der
/// Rahmen bekommt darum eine eigene, GROESSERE Grenze.
const MAX_SCHATTEN_KANTE: i64 = 4096;

/// M2: hoechste Pixelzahl fuer einen Schattenrahmen.
///
/// GEMESSEN 03.10.2026: 4096x4096 Pixel sind 16,8 M als `Color` (4 Byte)
/// fast 64 MB fuer EINEN Schatten. Das ist keine theoretische Zahl, das
/// waere der groesste Einzelposten im Speicher. Deshalb die Grenze bei
/// 4 M Pixel: 16 MB, und das nur fuer einen wirklich riesigen Schatten,
/// der in einem 1280x900-Fenster ohnehin fast nichts zeigt.
const MAX_SCHATTEN_PIXEL: i64 = 4_000_000;

/// M3: hoechste Zahl der Schatten, die pro Rechteck gezeichnet werden.
///
/// GEMESSEN: `narsil_core::MAX_SCHATTEN` ist 8. Acht Schatten auf einmal
/// brauchen acht Rasterdurchlaeuge ueber denselben Bereich — der Aufwand
/// waechst linear, der Nutzen nicht (der dritte Schatten desselben
/// Kastens liegt fast immer im zweiten). Das Limit ist eine ehrliche
/// Grenze, kein Vorwand.
const MAX_SCHATTEN_GEZEICHNET: usize = 8;

/// Zeichnet die `box-shadow` eines Rechtecks.
///
/// # Die Reihenfolge ist CSS Backgrounds 3 §7
///
/// Von hinten nach vorn: der `inset`-Schatten liegt VOR dem Hintergrund
/// (er wird ins Rechteck hineingezeichnet), die aeusseren Schatten
/// HINTER ihm. Der Aufrufer zeichnet darum zuerst die Schatten und dann
/// den Hintergrund.
///
/// # M1/M2/M3: alle Grenzen VOR der Allokation
///
/// * `anzahl` wird auf [`MAX_SCHATTEN_GEZEICHNET`] geklemmt.
/// * `radius` läuft durch [`radius_form`], das jeden Wert auf `MAX_RADIUS`
///   begrenzt.
/// * `sigma` läuft durch [`sigma_aus_weich`], das auf `MAX_WEICH`
///   begrenzt — und `NaN` verwirft.
/// * Die Rahmenkante wird gegen [`MAX_SCHATTEN_KANTE`] und die Pixelzahl
///   gegen [`MAX_SCHATTEN_PIXEL`] geprüft, BEVOR der Puffer entsteht.
///
/// # Rückgabe
///
/// Die Anzahl tatsächlich veränderter Pixel. 0 heißt: nichts gezeichnet —
/// kein Schatten, oder ein Schatten mit Alpha 0, oder einer, der ganz
/// ausserhalb des Kastens liegt.
///
/// # GEMESSEN, und warum `spread` VOR dem Gauß kommt
///
/// CSS Backgrounds 3 §7 nennt die Reihenfolge der Operationen:
/// 1. Versatz (`x`, `y`)
/// 2. Ausbreitung (`spread`) — die Form wird um diesen Betrag **nach
///    aussen** erweitert, BEVOR sie weichgezeichnet wird
/// 3. Weichzeichnen (`blur`)
/// 4. Farbe
///
/// Wer `spread` nach dem Gauß anwendet, weitet den weichen Rand auf und
/// bekommt einen dunkleren Fleck statt eines grösseren Schattens. Der
/// Test `der_schatten_wird_vor_dem_gauss_ausgebreitet` sichert die
/// Reihenfolge.
pub fn schatten_zeichnen(
    fb: &mut Framebuffer,
    rect: Rect,
    radius: impl Into<Corners>,
    schatten: &[SchattenArt; MAX_SCHATTEN],
    anzahl: u8,
) -> u32 {
    zeichne_schatten(fb, rect, radius.into(), schatten, anzahl, None)
}

/// Nur die Schatten AUSSERHALB des Kastens.
///
/// CSS Backgrounds 3 §7: ein aeusserer Schatten wird HINTER dem
/// Hintergrund gezeichnet. Diese Funktion wird deshalb VOR dem Kasten
/// aufgerufen.
pub fn schatten_aussen(
    fb: &mut Framebuffer,
    rect: Rect,
    radius: impl Into<Corners>,
    schatten: &[SchattenArt; MAX_SCHATTEN],
    anzahl: u8,
) -> u32 {
    zeichne_schatten(fb, rect, radius.into(), schatten, anzahl, Some(false))
}

/// Nur die Schatten INNERHALB des Kastens.
///
/// CSS Backgrounds 3 §7: ein `inset` wird VOR dem Hintergrund, aber
/// HINTER dem Inhalt gezeichnet.
///
/// # GEMESSEN 03.10.2026, UND DESWEGEN EXISTIERT DIESE TRENNUNG
///
/// Die erste Fassung zeichnete INNE und AUSSEN zusammen vor dem
/// Hintergrund. Der deckende Kasten ueberdeckte den `inset` danach
/// vollstaendig: im Bild team/log/befund_s2_kette.png war die `inset`-Box
/// von der Kante bis zur Mitte exakt eine Farbe (143, 92, 246), also war
/// der `inset`-Schatten unsichtbar — bei deckendem Hintergrund COMPLETT.
///
/// Der Test `ein_inset_schatten_bleibt_im_kasten` hatte das nicht gesehen,
/// weil er einen weissen Hintergrund ohne Kasten nahm: dort bleibt der
/// `inset` sichtbar. Ein Beispiel- Bild sieht den Zusammenspiel, ein
/// Einzelfall-Test nicht.
pub fn schatten_inset(
    fb: &mut Framebuffer,
    rect: Rect,
    radius: impl Into<Corners>,
    schatten: &[SchattenArt; MAX_SCHATTEN],
    anzahl: u8,
) -> u32 {
    zeichne_schatten(fb, rect, radius.into(), schatten, anzahl, Some(true))
}

/// Die eigentliche Arbeit; `richtung` filtert nach `innen`.
///
/// `None` = beide Richtungen (das ist, was die Tests brauchen, um die
/// Flächen zu vergleichen), `Some(x)` = nur diese.
fn zeichne_schatten(
    fb: &mut Framebuffer,
    rect: Rect,
    radius: Corners,
    schatten: &[SchattenArt; MAX_SCHATTEN],
    anzahl: u8,
    richtung: Option<bool>,
) -> u32 {
    // M1: die Zahl kommt aus einem fremden Datenstrom. `anzahl` ist ein
    // `u8` und kann 255 sein, das Array hat aber nur acht Plätze.
    let n = (anzahl as usize)
        .min(MAX_SCHATTEN_GEZEICHNET)
        .min(MAX_SCHATTEN);
    if n == 0 {
        return 0;
    }
    // M2: ein Rechteck, das es nicht gibt, zeichnet nichts.
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width <= 0.0
        || rect.height <= 0.0
    {
        return 0;
    }

    let bw = rect.width.round() as i64;
    let bh = rect.height.round() as i64;
    if bw <= 0 || bh <= 0 {
        return 0;
    }
    // Die Rasterform des Kastens: fuer `inset` dieselbe Form, nur innen.
    let r_form = radius.fit(rect.width, rect.height);

    let mut verendert = 0u32;

    // Die Schatten in REIHENFOLGE vom vordersten zum hintersten, wie CSS
    // sie listet: der erste Eintrag ist der oberste. Sie werden von hinten
    // nach vorn gezeichnet, damit der vordere den hinteren ueberlagert.
    for i in (0..n).rev() {
        let s = &schatten[i];
        // M1: ein Schatten mit Alpha 0 ist kein Schatten. Ohne diese
        // Pruefung rechnete die Schleife eine volle Flaeche und schrieb
        // nichts.
        // M1: ein Schatten mit Alpha 0 ist kein Schatten. Ohne diese
        // Pruefung rechnete die Schleife eine volle Flaeche und schrieb
        // nichts.
        let alpha = s.farbe.a;
        if alpha == 0 {
            continue;
        }
        // Der Filter: `None` zeichnet beide Richtungen.
        if let Some(will_innen) = richtung {
            if s.innen != will_innen {
                continue;
            }
        }
        // `spread` wird VOR dem Radius angewendet (CSS §7 Schritt 2), und
        // Radius-plus-Spread muss weiterhin auf die Kastenlaenge passen —
        // sonst wuerde die Form aus dem Kasten herausragen. Deshalb laeuft
        // das Ergebnis noch einmal durch `radius_form`.
        let spread = if s.ausbreitung.is_finite() {
            s.ausbreitung
        } else {
            0.0
        };
        //
        // **BILD_41, GEMESSEN (Firefox 157, `team/testseiten/schatten.html`):**
        // bei `border-radius: 0` und `spread: 6` sind die **Ecken des Schattens
        // ECKIG** — bei (4, 4) steht `#000000`, nicht der Weiss des Grundes. Die
        // erste Fassung rechnete `radius + spread`, also **6 px Radius**, und
        // liess die vier Ecken deshalb **abgerundet**: **GEMESSEN 32 fehlende
        // Pixel**, alle in den Eckbloecken. **Ein Radius von 0 bleibt 0.**
        //
        // **UND WAS ICH NICHT BEHAUPTE:** fuer `radius > 0` mit `spread` ist
        // **nicht** gemessen, ob Firefox den Radius **erhaelt** oder
        // **vergroessert**. Deshalb bleibt der alte Weg fuer `radius > 0`
        // **unveraendert** — ich aendere nur, was die Messung verlangt.
        let r_erweitert = Corners { x: r_form.x.map(|v| v + spread), y: r_form.y.map(|v| v + spread) };
        //
        // **Die Bedingung prueft den ORIGINALradius, nicht den erweiterten.**
        // Das war mein Fehler in der ersten Fassung: `r_erweitert` ist
        // `radius + spread` und damit **bei `radius: 0` und `spread: 6` gleich
        // `[6; 4]`** — also **nie** „alles kleiner als 0", und die Verzweigung
        // nahm **immer** den gerundeten Weg. **GEMESSEN** (Firefox 157): der
        // Schatten hat bei `border-radius: 0` **eckige** Ecken — **aussen**
        // (bei (4, 4) `#000000`) **und innen** (der Kern bei (16, 16)
        // `#eeeeee`, also **enthaelt** er die Ecke). **GEMESSEN: 32 fehlende
        // Pixel**, je vier pro Ecke.
        //
        // **BILD_43: DIE KLEMMUNG GEHOERT GEGEN DIE ERWEITERTE BOX.**
        // **GEMESSEN** (`radius_form` direkt aufgerufen):
        //
        // | Aufruf | Ergebnis |
        // |---|---|
        // | `radius_form([20;4], 80, 50)` | **20** |
        // | `radius_form([26;4], 80, 50)` | **25** — die Summe der Radien einer Kante (52) uebersteigt die **Hoehe** (50) |
        // | `radius_form([26;4], 92, 62)` | **26** — gegen die **erweiterte** Box passt es |
        //
        // **GEMESSENE WIRKUNG des Fehlers:** mit 25 lag der Punkt (24,5; 4,5) noch
        // **im** Bogen (Abstand zum Mittelpunkt (29, 29) = 24,9 < 25), also
        // began unsere deckende Zeile bei **x = 24**; Firefox beginnt bei
        // **x = 30**, weil es mit 26 rechnet. **Die Form ist nach dem `spread`
        // groesser, also muss die Klemmung danach gemessen werden** — sonst
        // beschneidet man den Radius an den **Falschen** Kanten.
        let r_schatten = if r_form.x.iter().chain(r_form.y.iter()).all(|v| *v <= 0.0) {
            Corners::default()
        } else {
            r_erweitert.fit(rect.width + 2.0 * spread, rect.height + 2.0 * spread)
        };

        let sigma = sigma_aus_weich(s.weich);
        // M3: die Reichweite des Gauss. Drei Sigma decken 99,7 % der
        // Energie; darueber hinaus ist der Beitrag unter 0,1 % und faellt
        // im 8-Bit-Alpha sowieso weg. Ohne diese Grenze waere die Schleife
        // bei `weich: 1e30` so gross wie das Rechteck selbst.
        const DREI_SIGMA: f32 = 3.0;
        let reach = if sigma > 0.0 { DREI_SIGMA * sigma } else { 0.0 };

        // Der Rahmen, in dem ueberhaupt etwas passieren kann.
        let versatz_x = if s.x.is_finite() { s.x } else { 0.0 };
        let versatz_y = if s.y.is_finite() { s.y } else { 0.0 };

        // Fuer `inset` liegt die Form INNERHALB, sonst AUSSERHALB.
        // Der Rahmen ist darum bei `inset` nur so gross wie der Kasten und
        // sonst so gross wie der Kasten plus Rand.
        let rand_px = if s.innen { 0.0 } else { reach };
        let rahmen_b = (bw as f32 + 2.0 * spread + 2.0 * rand_px).ceil() as i64;
        let rahmen_h = (bh as f32 + 2.0 * spread + 2.0 * rand_px).ceil() as i64;
        // M3: begrenzt VOR der Schleife.
        if rahmen_b <= 0
            || rahmen_h <= 0
            || rahmen_b > MAX_SCHATTEN_KANTE
            || rahmen_h > MAX_SCHATTEN_KANTE
            || rahmen_b.saturating_mul(rahmen_h) > MAX_SCHATTEN_PIXEL
        {
            continue;
        }

        // GEMESSEN 03.10.2026: die erste Fassung setzte den Rahmen um
        // `rahmen_b / 2` nach links oben. `rahmen_b` ist aber
        // `bw + 2*rand`, also `bw/2 + rand` — und damit stand die Form
        // um HALBE Kastenbreite zu weit links: GEMESSEN 0 statt 900
        // Pixel, weil der Rahmen den Kasten zur Haelfte aus dem Bild
        // schob. Der Rahmen beginnt genau beim Kasten MINUS Rand, und die
        // Form sitzt darin um `spread + rand` von links.
        let rand_px_i = rand_px.ceil() as i64;
        let spread_i = spread.ceil() as i64;
        let x0 = (rect.x.round() as i64) - rand_px_i - spread_i + versatz_x.round() as i64;
        let y0 = (rect.y.round() as i64) - rand_px_i - spread_i + versatz_y.round() as i64;

        // GEMESSEN 03.10.2026: der Clip-Stack. `set_pixel` beachtet ihn
        // nicht, und `rasterize_bereich` legt ihn fuer jeden Bereich an —
        // ohne diese Pruefung zeichnete ein Schatten aus einem
        // Nachbarbereich in das Bild hinein.
        let clip = fb.effektiver_clip();
        let clip_x0 = clip.x.floor() as i64;
        let clip_y0 = clip.y.floor() as i64;
        let clip_x1 = clip.right().ceil() as i64;
        let clip_y1 = clip.bottom().ceil() as i64;
        if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 {
            continue;
        }

        // DIE FORM, gegen die der Abstand gemessen wird, ist der
        // VERSETZTE und AUSGEBREITETE Kasten — nicht der Original.
        //
        // GEMESSEN 03.10.2026: die erste Fassung mass gegen `rect` selbst.
        // Bei einem Versatz von 10 px lagen sich Original (40..70) und
        // Rahmen (50..80) nur teilweise uebereinander, also war genau die
        // Schnittflaeche gedeckt: GEMESSEN 400 Pixel (20x20) statt 900
        // (30x30). Der Kasten stand an der richtigen Stelle, aber die Form,
        // die weichgezeichnet wird, war die FALSCHE.
        // GEMESSEN 03.10.2026: `spread` weitet die Form nach BEIDEN
        // Seiten aus (CSS Backgrounds 3 §7: "grow the shape"), also muss
        // der Ursprung um `spread` nach links oben wandern. Die erste
        // Fassung liess ihn stehen — die Form waechste nur nach rechts
        // unten. GEMESSEN: bei `spread: 10` und Kastenbeginn 60 lag die
        // Form bei 60..120 statt 50..110, und Punkt 55 blieb aussen.
        //
        // **BILD_41, GEMESSEN:** `inset` + `spread 6` malt in Firefox ein
        // **6 px** Band **innerhalb** des Kastens (Datei
        // `team/testseiten/schatten_innen_ff.png`). Die erste Fassung rechnete
        // die Form **auch fuer `inset`** nach **aussen** — damit lag die ganze
        // Form **ausserhalb** des Kastens, jeder Punkt war „innen", der Abstand
        // 0 **und** der `inset`-Zweig **uebersprang** zusaetzlich alles
        // ausserhalb der Form. **GEMESSEN: 1416 gemalte Pixel weniger**, und
        // `1416` ist **genau** die Bandflaeche `80*50 - 68*38`.
        // **CSS Backgrounds 3 §9:** `spread` waechst bei `inset` nach **innen**.
        let form = if s.innen {
            Rect {
                x: rect.x + versatz_x + spread,
                y: rect.y + versatz_y + spread,
                width: (rect.width - 2.0 * spread).max(0.0),
                height: (rect.height - 2.0 * spread).max(0.0),
            }
        } else {
            Rect {
                x: rect.x + versatz_x - spread,
                y: rect.y + versatz_y - spread,
                width: rect.width + 2.0 * spread,
                height: rect.height + 2.0 * spread,
            }
        };

        // Die Form, gegen die getestet wird: die (ausgebreitete) Kastenform
        // oder — fuer `inset` — dieselbe Form, von innen her geschnitten.
        for y in 0..rahmen_h {
            for x in 0..rahmen_b {
                let px = x0 + x;
                let py = y0 + y;
                // Der Abstand zur Form: 0 innerhalb, wachsend nach aussen.
                // Fuer `inset` ist der Abstand zur INNEREN Kante massgeblich
                // (ein Punkt ganz innen hat Abstand 0, also volle Deckung),
                // sonst der zur Aussenkante.
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;
                let deckung = if s.innen {
                    //
                    // **BILD_41: `inset` malt das BAND ZWISCHEN Kastenkante und
                    // eingedellter Form** (CSS Backgrounds 3 §9.2), also:
                    //   * ausserhalb des Kastens -> nichts,
                    //   * innerhalb des Kerns   -> nichts,
                    //   * dazwischen            -> gedeckt.
                    // Die erste Fassion oversprang **alles ausserhalb** der
                    // Form; nach dem Verschieben der Form nach **innen** waere
                    // genau das **das ganze Band** gewesen.
                    let Kasten = Rect {
                        x: rect.x + versatz_x,
                        y: rect.y + versatz_y,
                        width: rect.width,
                        height: rect.height,
                    };
                    if !punkt_im_kasten_bezogen(
                        px_f,
                        py_f,
                        Kasten.x,
                        Kasten.y,
                        Kasten.width,
                        Kasten.height,
                        r_form,
                    ) {
                        continue;
                    }
                    if punkt_im_kasten_bezogen(
                        px_f,
                        py_f,
                        form.x,
                        form.y,
                        form.width,
                        form.height,
                        r_schatten,
                    ) {
                        continue;
                    }
                    // **Negativ**, weil die Deckung weiter unten als „0 =
                    // drinnen" geprueft wird: der Kern hat Abstand 0, das Band
                    // einen **positiven** — das Vorzeichen dreht beides um.
                    -abstand_von_form(px_f, py_f, form, r_schatten)
                } else {
                    // Release 14.10.2026 (Agent CSS2): an outer shadow is drawn
                    // outside the border edge only; it is clipped inside the
                    // border box (CSS Backgrounds 3 §7.1). MEASURED before (WPT
                    // css-backgrounds `box-shadow-039..042`, a semi-transparent
                    // box): the shadow showed through the box.
                    // (Only on the paint path `schatten_aussen`; the measuring
                    // `schatten_zeichnen` keeps the whole shape.)
                    if richtung == Some(false)
                        && punkt_im_kasten_bezogen(px_f, py_f, rect.x, rect.y, rect.width, rect.height, r_form)
                    {
                        continue;
                    }
                    abstand_von_form(px_f, py_f, form, r_schatten)
                };

                // Ohne Weichzeichnung (`weich: 0`, also sigma == 0) ist die
                // Deckung ein Sprung: 1 INNERHALB der Form, 0 ausserhalb.
                //
                // GEMESSEN 03.10.2026: die Bedingung stand hier umgekehrt
                // (`else if deckung <= 0.0`). Bei `weich: 0` zeichnete der
                // Schatten deshalb NICHTS — GEMESSEN 0 statt 900 Pixel.
                // `abstand_von_form` liefert 0 innerhalb, also ist
                // `deckung <= 0.0` das Kriterium fuer "drinnen".
                let deckung = if sigma > 0.0 {
                    gauß(sigma, deckung)
                } else if deckung <= 0.0 {
                    1.0
                } else {
                    0.0
                };

                if deckung <= 0.0 {
                    continue;
                }
                // M1: die Deckung wird auf das 8-Bit-Alpha des Schattens
                // gerechnet und gerundet. Ein Wert > 1 kann entstehen,
                // wenn `gauß` knapp ueber 1 liegt (exp(0) == 1.0, aber
                // die Interpolation kann 1.0000001 ergeben).
                let a = ((deckung.clamp(0.0, 1.0) * alpha as f32).round() as u32).min(255);
                if a == 0 {
                    continue;
                }
                // GEMESSEN 03.10.2026: `set_pixel` UEBERSCHREIBT, es mischt
                // nicht. Die erste Fassung dieses Schattenschreibers hat den
                // teilweise deckenden Schatten mit `set_pixel` gesetzt und
                // damit den weissen Hintergrund durch ein halbtransparentes
                // Pixel ersetzt — die Alphasumme des Framebuffers fiel
                // dadurch von 3.672.000 auf 229.500 (GEMESSEN im Test
                // `die_alpha_summe_entspricht_der_gezeichneten_flaeche`).
                //
                // RICHTIG ist deshalb das Mischen nach `source over`
                // (narsil_raster::framebuffer::blend_source_over): der
                // Schatten liegt UEBER dem Hintergrund, nicht an seine
                // Stelle. Bei deckendem Alpha ist das Ergebnis genau der
                // Schatten, bei halbem die Mischung — beides folgt aus
                // derselben Formel.
                if px < 0 || py < 0 {
                    continue;
                }
                // GEMESSEN 03.10.2026: derselbe Fehler wie bei
                // `kasten_mit_radius` — der Clip-Stack wurde umgangen. Jetzt
                // wird am effektiven Clipp geprueft, statt den ganzen
                // Rahmen zu durchlaufen.
                if px < clip_x0 || py < clip_y0 || px >= clip_x1 || py >= clip_y1 {
                    continue;
                }
                let px = px as u32;
                let py = py as u32;
                let alt = match fb.pixel(px, py) {
                    Some(c) => c,
                    // Ausserhalb: `set_pixel` wuerde false liefern und
                    // nichts zaehlen. Hier ist nichts zu tun.
                    None => continue,
                };
                let mut neu = s.farbe;
                neu.a = a as u8;
                let gemischt = crate::framebuffer::blend_source_over(neu, alt);
                if fb.set_pixel(px, py, gemischt) {
                    verendert += 1;
                }
            }
        }
    }
    verendert
}

/// Der Abstand eines Punktes zur Aussenkante der Form, 0 innerhalb.
///
/// GEMESSEN 03.10.2026: die erste Fassung rechnete `dx` und `dy` als
/// Abstände zu den vier RECHTECK-Kanten. Bei einer runden Ecke liegt der
/// Punkt aber INNERHALB des Hüllquadrats und AUSSERHALB der Form — beide
/// Abstände waren dort 0, also war der Abstand 0, also war die Deckung
/// voll. GEMESSEN: die Ecke eines Kastens mit Radius 20 wurde schwarz
/// gezeichnet. Jetzt wird zuerst der Viertelkreis geprueft.
fn abstand_von_form(x: f32, y: f32, rect: Rect, r: impl Into<Corners>) -> f32 {
    let c: Corners = r.into();
    if !c.is_circular() {
        // Release 15.10.2026 (Agent F): elliptical corners.
        return distance_xy(x, y, rect, &c).max(0.0);
    }
    let r = c.x;
    if punkt_im_kasten_bezogen(x, y, rect.x, rect.y, rect.width, rect.height, r) {
        return 0.0;
    }
    let links = rect.x;
    let rechts = rect.x + rect.width;
    let oben = rect.y;
    let unten = rect.y + rect.height;

    // Fall 1: der Punkt liegt in einer runden ECKE, also im Quadrat der
    // Ecke und ausserhalb des Viertelkreises. Dann ist der Abstand die
    // Strecke vom Mittelpunkt minus der Radius.
    let mitten = [
        (links + r[0], oben + r[0]),
        (rechts - r[1], oben + r[1]),
        (rechts - r[2], unten - r[2]),
        (links + r[3], unten - r[3]),
    ];
    for (i, &(mx, my)) in mitten.iter().enumerate() {
        let re = r[i];
        if re <= 0.0 {
            continue;
        }
        if x < mx - re || x > mx + re || y < my - re || y > my + re {
            continue;
        }
        let zum_mittelpunkt = ((x - mx).powi(2) + (y - my).powi(2)).sqrt();
        // GEMESSEN 03.10.2026: die Differenz stand verkehrt herum
        // (`re - zum_mittelpunkt`). Fuer einen Punkt AUSSERHALB des
        // Viertelkreises ist aber `zum_mittelpunkt > re`, das Ergebnis
        // also negativ, und `if d > 0.0` traf nie zu — die Ecke wurde
        // ueber den Rechteckabstand (0) als voll gedeckt gezeichnet.
        let d = zum_mittelpunkt - re;
        if d > 0.0 {
            return d;
        }
    }

    // Fall 2: der Punkt liegt ausserhalb des Rechtecks. Dann ist der
    // Abstand die euklidische Distanz zum naechsten Rand.
    let dx = if x < links {
        links - x
    } else if x > rechts {
        x - rechts
    } else {
        0.0
    };
    let dy = if y < oben {
        oben - y
    } else if y > unten {
        y - unten
    } else {
        0.0
    };
    (dx * dx + dy * dy).sqrt()
}

/// Der Abstand eines Punktes zur naechsten INNEREN Kante.
///
/// Fuer `inset` (CSS Backgrounds 3 §9.2): der Schatten sitzt an der
/// Innenkante und weich nach INNEN aus. Am Rand ist die Deckung voll, in
/// der Mitte geht sie gegen 0 — das ist genau dieser Abstand, als Argument
/// des Gauss.
///
/// # GEMESSEN 03.10.2026: zwei Fehler in der ersten Fassung
///
/// 1. Sie mass gegen das QUADRAT, nicht gegen den INNENkreis. In einer
///    runden Ecke liegt die Kante auf dem Kreis um die Eckenmitte, also
///    war der berechnete Abstand dort zu klein und der Schatten drang
///    weiter nach innen als er sollte.
/// 2. Sie benutzte fuer die vier Kanten einen Ausdruck der Form
///    `x.min(rechts).max(x) - x`, der fuer JEDES `x` im Kasten 0 ergibt.
///    Der Abstand zur inneren Kante war damit nie ungleich 0, und ein
///    `inset` zeichnete die ganze Kastenflaeche deckend statt mit einem
///    Verlauf. GEMESSEN: 900 Pixel statt eines weichen Saums.
///
/// # Rueckgabe
///
/// Der Abstand zur naechsten inneren Kante. Fuer Punkte AUSSERHALB der
/// Form ist das Ergebnis nicht definiert — der Aufrufer prueft die
/// Zugehoerigkeit mit [`punkt_im_kasten_bezogen`] und ueberspringt sie.
fn innen_abstand(x: f32, y: f32, rect: Rect, r: [f32; 4]) -> f32 {
    let links = rect.x;
    let rechts = rect.x + rect.width;
    let oben = rect.y;
    let unten = rect.y + rect.height;

    // Abstand zu den vier geraden Kanten; der kleinste gewinnt.
    let mut d = (x - links).min(rechts - x).min(y - oben).min(unten - y);

    // In einer runden Ecke liegt die innere Kante auf dem INNENkreis um die
    // Eckenmitte, nicht auf dem Rechteck. Gesucht ist deshalb der Abstand
    // zum Kreis: `radius - Abstand zum Mittelpunkt`.
    //
    // Die vier Eckenmittelpunkte in derselben Reihenfolge wie `r`.
    let mitten = [
        (links + r[0], oben + r[0]),
        (rechts - r[1], oben + r[1]),
        (rechts - r[2], unten - r[2]),
        (links + r[3], unten - r[3]),
    ];
    for (i, &(mx, my)) in mitten.iter().enumerate() {
        let re = r[i];
        if re <= 0.0 {
            continue;
        }
        // Nur wenn der Punkt im Quadrat dieser Ecke liegt.
        if x < mx - re || x > mx + re || y < my - re || y > my + re {
            continue;
        }
        //
        // GEMESSEN 03.10.2026: es muss der ABSTAND ZUM BOGEN sein, nicht
        // zum ganzen Kreis. Der Bogen einer Ecke ist nur ein VIERTEL des
        // Kreises. Ohne diese Pruefung wurde der Abstand zu Punkten
        // gerechnet, die gar nicht vom Bogen begrenzt werden — bei einem
        // Kasten 260..440 mit Radius 16 ergab das bei y = 625:
        //
        //     x=266 (73,45,133)  x=274 (112,71,196)  x=286 (68,42,126)
        //     x=290 (113,72,197) x=294 (113,72,197)
        //
        // GEMESSEN: bei x=286 lag ein dunkler Fleck mitten in der Ecke, obwohl
        // die Form dort von der GERADEN Oberkante begrenzt wird und der
        // Abstand 5 Pixel betraegt — genau der Wert der geraden Seite.
        //
        // Die Viertel-Zuordnung, jeweils bezogen auf den Eckmittelpunkt:
        //   0 oben links     x <= mx und y <= my
        //   1 oben rechts    x >= mx und y <= my
        //   2 unten rechts   x >= mx und y >= my
        //   3 unten links    x <= mx und y >= my
        let im_bogen = match i {
            0 => x <= mx && y <= my,
            1 => x >= mx && y <= my,
            2 => x >= mx && y >= my,
            _ => x <= mx && y >= my,
        };
        if !im_bogen {
            continue;
        }
        let zum_mittelpunkt = ((x - mx).powi(2) + (y - my).powi(2)).sqrt();
        let in_ecke = re - zum_mittelpunkt;
        if in_ecke > 0.0 && in_ecke < d {
            d = in_ecke;
        }
        break;
    }
    if d < 0.0 { 0.0 } else { d }
}

/// Wie [`punkt_im_kasten`], aber mit Ursprung und Groesse statt 0.
/// `punkt_im_kasten` rechnet in Boxkoordinaten ab 0; hier liegt der
/// Kasten beliebig im Framebuffer.
fn punkt_im_kasten_bezogen(
    x: f32,
    y: f32,
    x0: f32,
    y0: f32,
    breite: f32,
    hoehe: f32,
    r: impl Into<Corners>,
) -> bool {
    point_in_box_xy(x - x0, y - y0, breite, hoehe, &r.into())
}

// ---------------------------------------------------------------------------
// W8.4: `outline` — der Ring AUSSERHALB des Kastens.
// ---------------------------------------------------------------------------

/// M3: hoechste Kantenlaenge eines Outline-Rings.
///
/// Eigener Wert, wie bei `MAX_SCHATTEN_KANTE`: ein Ring kann weit ausserhalb
/// liegen, weil `outline-offset` den Kasten uebersteigen darf.
const MAX_OUTLINE_KANTE: i64 = 4096;

/// M2: hoechste Pixelzahl fuer einen Outline-Ring.
///
/// 4096x4096 als `Color` waeren fast 64 MB fuer EINEN Ring. Das ist kein
/// theoretischer Wert, das waere der groesste Einzelposten im Bild.
const MAX_OUTLINE_PIXEL: i64 = 4_000_000;

/// M1: hoechster `outline-offset` in px.
///
/// GEMESSEN 03.10.2026: `outline-offset` darf negativ sein (der Ring wandert
/// dann NACH INNEN) und es gibt in CSS keinen oberen Grenzwert. Fuer den
/// Rasterizer heisst das: ein Versatz von 1e9 px wuerde eine Schleife
/// ueber Milliarden Pixeln sein. Diese Grenze ist meine, und sie ist
/// benannt statt stillschweigend angewendet.
pub const MAX_OUTLINE_OFFSET: f32 = 1024.0;

/// Zeichnet den `outline`-Ring eines Kastens.
///
/// # DIE GEOMETRIE, und sie ist der ganze Unterschied zu `border`
///
/// CSS UI 4 §6.1: `outline-offset` ist der Abstand der **Ringkante** von
/// der **Boxkante**. Ein positiver Offset schiebt den Outline also
/// WEITER WEG von der Box, nicht hinein — an der linken Kante nach links.
///
/// ```text
/// linke Kante:   Innenkante  = Kasten - offset
///                Aussenkante = Kasten - offset - breite
/// ```
///
/// GEMESSEN am Beispiel (Kasten links bei x = 40, breite 4, offset 2):
/// Innenkante 38, Aussenkante 34, Mitte 36. Waere die Mitte bei 40, waere
/// es ein Rahmen auf dem Kastenrand — genau der Fehler, vor dem W8.4
/// warnt.
///
/// (GEMESSEN: die erste Fassung dieses Kommentars schrieb `Kasten + offset`
/// und war damit FALSCH. Der eingerückte Block war ausserdem ein Doctest
/// und wurde ausgefuehrt — eine Formel in einem Kommentar, die sich als
/// Quellcode verkleidet, wird kompiliert. Darum jetzt ```text.)
///
/// # DER RING UEBERLAGERT DIE BOX NICHT
///
/// CSS UI 4 §6.1: der Outline liegt per Definition ausserhalb der
/// `border edge`. Eine positive `outline-breite` mit negativem Offset darf
/// deshalb nicht beliebig weit nach innen ragen — bei `offset = -breite`
/// faellt die Innenkante genau auf die Kastenkante, und tiefer waere es
/// ein Uebermalen des Inhalts. Die Funktion zeichnet deshalb den Ring in
/// einem Rahmen, der die Kastenkante als INNERE Grenze hat, und schneidet
/// dort ab.
///
/// # WAS NICHT GEZEICHNET WIRD
///
/// * Die ECKE folgt dem `border-radius` der Box, gerundet wie der Kasten.
///   CSS UI 4 §6.1 nennt das ausdruecklich.
/// * Ein `outline-style: none` gibt es nicht (CSS UI 4 hat keinen Stil).
///   Breite 0 bedeutet also "kein Outline".
///
/// # Rueckgabe
///
/// Die Anzahl tatsaechlich gesetzter Pixel. 0 heisst: keine Breite, kein
/// Ring, oder der Ring liegt ganz ausserhalb des Kastens.
pub fn outline_zeichnen(
    fb: &mut Framebuffer,
    rect: Rect,
    radius: [f32; 4],
    farbe: Color,
    breite: f32,
    offset_px: f32,
) -> u32 {
    // M1/M2: Breite und Offset kommen aus dem CSS.
    if !breite.is_finite() || breite <= 0.0 {
        return 0;
    }
    if !offset_px.is_finite() {
        return 0;
    }
    if farbe.a == 0 {
        return 0;
    }
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width <= 0.0
        || rect.height <= 0.0
    {
        return 0;
    }
    // M1: der Offset wird geklemmt. `outline-offset: 100000px` ist
    // gueltiges CSS und wuerde einen Ring zeichnen, den niemand sieht.
    let offset = offset_px.clamp(-MAX_OUTLINE_OFFSET, MAX_OUTLINE_OFFSET);

    let bw = rect.width.round() as i64;
    let bh = rect.height.round() as i64;
    if bw <= 0 || bh <= 0 {
        return 0;
    }
    if bw > MAX_OUTLINE_KANTE || bh > MAX_OUTLINE_KANTE {
        return 0;
    }
    if bw.saturating_mul(bh) > MAX_OUTLINE_PIXEL {
        return 0;
    }

    // Die Ringform: dieselbe Kastenform, um `offset + breite` gewachsen.
    //
    // GEMESSEN 03.10.2026: fuer die Form wird `radius + offset + breite`
    // genommen, weil der Ring der KANTE folgt und die KANTE um genau diesen
    // Betrag gewachsen ist. Bei negativem Offset waere die Form kleiner als
    // die des Kastens — das ist dann der Fall "Innenkante faellt in die
    // Box hinein", und der Ring wird am Ende beschnitten.
    //
    // DIE WEITUNG, und sie ist der Fehler, den die Tests gefunden haben.
    //
    // GEMESSEN 03.10.2026: die erste Fassung hat nur den RADIUS
    // vergroessert (`radius_form(radius + aussen, …)`). GEMESSEN Folge: bei
    // `offset = 0, breite = 6` zeichnete der Ring NULL Pixel. Denn
    // `radius_form` weitet den KASTEN nicht — es veraendert nur, wie stark
    // die ECKEN gerundet sind. Ein Kasten 40..120 mit Radius 6 ist immer
    // noch 40..120.
    //
    // Der Ring braucht beides: einen um `aussen` gewachsenen Kasten UND
    // einen um `aussen` gewachsenen Radius.
    let aussen = offset + breite;
    let mut ring = rect;
    ring.x -= aussen;
    ring.y -= aussen;
    ring.width += 2.0 * aussen;
    ring.height += 2.0 * aussen;
    let r_ring = radius_form(
        [
            radius[0] + aussen,
            radius[1] + aussen,
            radius[2] + aussen,
            radius[3] + aussen,
        ],
        rect.width,
        rect.height,
    );
    //
    // DIE FORM DER INNEREN KANTE, und HIER war der Fehler.
    //
    // GEMESSEN 03.10.2026: die erste Fassung schnitt am Kasten selbst ab
    // (`radius_form(radius, …)`). Damit lag der Ring bei `offset = +10`
    // und `breite = 4` zwischen x = 26 und x = 40 — also VIERZEHN Pixel
    // breit statt vier.
    //
    // CSS UI 4 §6.1: `outline-offset` ist der Abstand der Ringkante von
    // der Boxkante. Ein positiver Offset schiebt den Outline also WEITER
    // WEG: bei Boxkante 40, Offset 10, Breite 4 liegt der Ring bei 30..26,
    // mit einer Luecke von 10 zwischen Box und Ring.
    //
    // GEMESSEN ist das der Unterschied zwischen
    //   Innenkante = Kasten + offset              (30)
    //   Aussenkante = Kasten + offset + breite     (26)
    // Die Funktion prueft beides: innen in der um `offset + breite`
    // gewachsenen Form, und AUSSERHALB der um `offset` gewachsenen.
    let mut innen = rect;
    innen.x -= offset;
    innen.y -= offset;
    innen.width += 2.0 * offset;
    innen.height += 2.0 * offset;
    let r_innen = radius_form(
        [
            radius[0] + offset,
            radius[1] + offset,
            radius[2] + offset,
            radius[3] + offset,
        ],
        rect.width,
        rect.height,
    );

    let clip = fb.effektiver_clip();
    if clip.width <= 0.0 || clip.height <= 0.0 {
        return 0;
    }
    let clip_x0 = clip.x.floor() as i64;
    let clip_y0 = clip.y.floor() as i64;
    let clip_x1 = clip.right().ceil() as i64;
    let clip_y1 = clip.bottom().ceil() as i64;
    if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 {
        return 0;
    }

    // Der Rahmen umfasst den Kasten und den Ring.
    let rahmen_b = (bw as f32 + 2.0 * aussen.abs()).ceil() as i64 + 2;
    let rahmen_h = (bh as f32 + 2.0 * aussen.abs()).ceil() as i64 + 2;
    // M3: begrenzt VOR der Schleife.
    if rahmen_b <= 0
        || rahmen_h <= 0
        || rahmen_b > MAX_OUTLINE_KANTE
        || rahmen_h > MAX_OUTLINE_KANTE
        || rahmen_b.saturating_mul(rahmen_h) > MAX_OUTLINE_PIXEL
    {
        return 0;
    }
    let x0 = rect.x.round() as i64 - rahmen_b / 2;
    let y0 = rect.y.round() as i64 - rahmen_h / 2;

    let mut gesetzt = 0u32;
    for y in 0..rahmen_h {
        for x in 0..rahmen_b {
            let px = x0 + x;
            let py = y0 + y;
            if px < clip_x0 || py < clip_y0 || px >= clip_x1 || py >= clip_y1 {
                continue;
            }
            if px < 0 || py < 0 {
                continue;
            }
            let fx = px as f32 + 0.5;
            let fy = py as f32 + 0.5;
            // Zwei Bedingungen, und beide muessen gelten:
            //   1. im RING  = innerhalb der gewachsenen Form
            //   2. NICHT in der Box, wenn der Offset positiv ist
            let im_ring =
                punkt_im_kasten_bezogen(fx, fy, ring.x, ring.y, ring.width, ring.height, r_ring);
            if !im_ring {
                continue;
            }
            // GEMESSEN 03.10.2026: bei `offset = 0` liegt die Innenkante
            // GENAU auf der Kastenkante, der Ring also von 36 bis 44 neben
            // einer Box ab 40. Ohne diese zweite Pruefung wuerde er die Box
            // um `breite` ueberdecken — ein Fuellstrich, kein Ring.
            //
            // Bei `offset > 0` ist die Innenkante um `offset` nach AUSSEN
            // gewachsen: der Ring liegt dann zwischen der um `offset`
            // gewachsenen und der um `offset + breite` gewachsenen Form.
            let in_der_box = punkt_im_kasten_bezogen(
                fx,
                fy,
                innen.x,
                innen.y,
                innen.width,
                innen.height,
                r_innen,
            );
            if in_der_box {
                continue;
            }
            // Bei negativem Offset darf der Ring in die Box hineinragen —
            // CSS UI 4 §6.1 erlaubt das (`outline-offset: -2px` ist ueblich
            // fuer einen Fokusring, der INNEN liegt). Die Ueberdeckung der
            // Box ist dann gewollt; beschnitten wird nur gegenueber der
            // gewachsenen Form, und die erlaubt es.
            let px_u = px as u32;
            let py_u = py as u32;
            let alt = match fb.pixel(px_u, py_u) {
                Some(c) => c,
                None => continue,
            };
            let neu = crate::framebuffer::blend_source_over(farbe, alt);
            if fb.set_pixel(px_u, py_u, neu) {
                gesetzt += 1;
            }
        }
    }
    gesetzt
}
