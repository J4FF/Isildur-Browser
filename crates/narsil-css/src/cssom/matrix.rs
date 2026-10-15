//! Release 12.10.2026: der aufgeloeste Wert von `transform` (CSSOM §9
//! „resolved value": die Transformationsmatrix als `matrix()` bzw.
//! `matrix3d()`; Rechenregeln CSS Transforms 2 §13 „Mathematical Description
//! of Transform Functions").
//!
//! GEMESSEN vorher (WPT css-values *-serialize: `scale(pow(1,1))`,
//! `scale(min(.1))`, `rotate(min(90deg))` …): `getComputedStyle().transform`
//! gab die Funktionsliste zurueck (`scale(calc(1))`) statt
//! `matrix(1, 0, 0, 1, 0, 0)`.

use super::mathe::{self, Mathe, Ziel};
use super::werte::{Ber, Num, S};
use super::zahl::zahl_text;

/// 4×4-Matrix spaltenweise (`m[spalte * 4 + zeile]`, wie `matrix3d()`).
type M4 = [f64; 16];

const EINS: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// `a · b` (erst `b`, dann `a` — die Leserichtung der Funktionsliste).
fn mal(a: &M4, b: &M4) -> M4 {
    let mut r = [0.0; 16];
    for s in 0..4 {
        for z in 0..4 {
            r[s * 4 + z] = (0..4).map(|k| a[k * 4 + z] * b[s * 4 + k]).sum();
        }
    }
    r
}

/// Der aufgeloeste Zahlwert (Laengen in px, Winkel in deg). Ein `NaN` der
/// Rechnung gilt als 0 (CSS Values 4 §10.9).
fn wert(n: &Num, b: &Ber) -> Option<(f64, String)> {
    let m = match n {
        Num::L(w, e) => Mathe {
            wurzel: mathe::K::N(*w, e.clone()),
            ziel: Ziel::Zahl,
            typ: mathe::Typ::default(),
            bereich: (f64::NEG_INFINITY, f64::INFINITY),
        },
        Num::M(m) => (**m).clone(),
    };
    let a = m.aufgeloest(&b.u);
    let (w, e) = a.zahlwert()?;
    Some((if w.is_nan() { 0.0 } else { w }, e.to_string()))
}

fn laenge(s: &S, b: &Ber) -> Option<f64> {
    let S::Z(n) = s else { return None };
    let (w, e) = wert(n, b)?;
    match e.as_str() {
        "px" => Some(w),
        "" if w == 0.0 => Some(0.0),
        // Prozente brauchen die Rahmenbox (erst das Layout).
        _ => None,
    }
}

fn zahl(s: &S, b: &Ber) -> Option<f64> {
    let S::Z(n) = s else { return None };
    let (w, e) = wert(n, b)?;
    match e.as_str() {
        "" => Some(w),
        "%" => Some(w / 100.0),
        _ => None,
    }
}

/// Ein Winkel in Grad.
fn winkel(s: &S, b: &Ber) -> Option<f64> {
    let S::Z(n) = s else { return None };
    let (w, e) = wert(n, b)?;
    match e.as_str() {
        "deg" => Some(w),
        "" if w == 0.0 => Some(0.0),
        "grad" => Some(w * 0.9),
        "rad" => Some(w.to_degrees()),
        "turn" => Some(w * 360.0),
        _ => None,
    }
}

/// Sinus und Kosinus, bei Vielfachen von 90° genau (sonst bliebe bei 90°
/// ein Rest von 6e-17 stehen).
fn sin_cos(grad: f64) -> (f64, f64) {
    let r = grad.rem_euclid(360.0);
    if r == 0.0 {
        (0.0, 1.0)
    } else if r == 90.0 {
        (1.0, 0.0)
    } else if r == 180.0 {
        (0.0, -1.0)
    } else if r == 270.0 {
        (-1.0, 0.0)
    } else {
        grad.to_radians().sin_cos()
    }
}

fn tan(grad: f64) -> f64 {
    let (s, c) = sin_cos(grad);
    s / c
}

/// Drehung um die Achse (x, y, z) (CSS Transforms 2 §13, `rotate3d()`).
fn drehung(x: f64, y: f64, z: f64, grad: f64) -> M4 {
    let l = (x * x + y * y + z * z).sqrt();
    if l == 0.0 || !l.is_finite() {
        return EINS;
    }
    let (x, y, z) = (x / l, y / l, z / l);
    let (s, c) = sin_cos(grad);
    // sc = sin(a/2)·cos(a/2) = sin(a)/2, sq = sin²(a/2) = (1 − cos(a))/2
    let (sc, sq) = (s / 2.0, (1.0 - c) / 2.0);
    let mut m = EINS;
    // Zeile r, Spalte s -> m[s * 4 + r]
    m[0] = 1.0 - 2.0 * (y * y + z * z) * sq;
    m[4] = 2.0 * (x * y * sq - z * sc);
    m[8] = 2.0 * (x * z * sq + y * sc);
    m[1] = 2.0 * (x * y * sq + z * sc);
    m[5] = 1.0 - 2.0 * (x * x + z * z) * sq;
    m[9] = 2.0 * (y * z * sq - x * sc);
    m[2] = 2.0 * (x * z * sq - y * sc);
    m[6] = 2.0 * (y * z * sq + x * sc);
    m[10] = 1.0 - 2.0 * (x * x + y * y) * sq;
    m
}

fn verschiebung(x: f64, y: f64, z: f64) -> M4 {
    let mut m = EINS;
    m[12] = x;
    m[13] = y;
    m[14] = z;
    m
}

fn skalierung(x: f64, y: f64, z: f64) -> M4 {
    let mut m = EINS;
    m[0] = x;
    m[5] = y;
    m[10] = z;
    m
}

/// Die Matrix einer Transformationsfunktion.
fn funktion(name: &str, inhalt: &[S], b: &Ber) -> Option<M4> {
    let a: Vec<&S> = inhalt.iter().filter(|p| !matches!(p, S::Komma)).collect();
    let l = |i: usize| a.get(i).and_then(|s| laenge(s, b));
    let z = |i: usize| a.get(i).and_then(|s| zahl(s, b));
    let w = |i: usize| a.get(i).and_then(|s| winkel(s, b));
    Some(match name.to_ascii_lowercase().as_str() {
        "translate" => verschiebung(l(0)?, if a.len() > 1 { l(1)? } else { 0.0 }, 0.0),
        "translatex" => verschiebung(l(0)?, 0.0, 0.0),
        "translatey" => verschiebung(0.0, l(0)?, 0.0),
        "translatez" => verschiebung(0.0, 0.0, l(0)?),
        "translate3d" => verschiebung(l(0)?, l(1)?, l(2)?),
        "scale" => {
            let x = z(0)?;
            skalierung(x, if a.len() > 1 { z(1)? } else { x }, 1.0)
        }
        "scalex" => skalierung(z(0)?, 1.0, 1.0),
        "scaley" => skalierung(1.0, z(0)?, 1.0),
        "scalez" => skalierung(1.0, 1.0, z(0)?),
        "scale3d" => skalierung(z(0)?, z(1)?, z(2)?),
        "rotate" | "rotatez" => drehung(0.0, 0.0, 1.0, w(0)?),
        "rotatex" => drehung(1.0, 0.0, 0.0, w(0)?),
        "rotatey" => drehung(0.0, 1.0, 0.0, w(0)?),
        "rotate3d" => drehung(z(0)?, z(1)?, z(2)?, w(3)?),
        "skew" | "skewx" | "skewy" => {
            let (ax, ay) = match name.to_ascii_lowercase().as_str() {
                "skewx" => (w(0)?, 0.0),
                "skewy" => (0.0, w(0)?),
                _ => (w(0)?, if a.len() > 1 { w(1)? } else { 0.0 }),
            };
            let mut m = EINS;
            m[4] = tan(ax);
            m[1] = tan(ay);
            m
        }
        "perspective" => {
            let mut m = EINS;
            // `none` ist unendlich fern; Tiefen unter 1px gelten als 1px.
            if !matches!(a.first(), Some(S::W(_))) {
                m[11] = -1.0 / l(0)?.max(1.0);
            }
            m
        }
        "matrix" => {
            if a.len() != 6 {
                return None;
            }
            let v: Vec<f64> = (0..6).map(z).collect::<Option<_>>()?;
            let mut m = EINS;
            m[0] = v[0];
            m[1] = v[1];
            m[4] = v[2];
            m[5] = v[3];
            m[12] = v[4];
            m[13] = v[5];
            m
        }
        "matrix3d" => {
            if a.len() != 16 {
                return None;
            }
            let v: Vec<f64> = (0..16).map(z).collect::<Option<_>>()?;
            let mut m = [0.0; 16];
            m.copy_from_slice(&v);
            m
        }
        _ => return None,
    })
}

/// Der aufgeloeste Wert von `transform`: `none` oder die Matrix. `None` =
/// braucht die Rahmenbox (Prozente) oder ist unbekannt.
pub fn transform_text(s: &[S], b: &Ber) -> Option<String> {
    if let [S::W(w)] = s {
        return (w == "none").then(|| String::from("none"));
    }
    let mut m = EINS;
    for p in s {
        let S::F(name, inhalt) = p else { return None };
        m = mal(&m, &funktion(name, inhalt, b)?);
    }
    let t = |v: f64| zahl_text(if v == 0.0 { 0.0 } else { v });
    let flach = [2, 3, 6, 7, 8, 9, 11, 14].iter().all(|&i| m[i] == 0.0) && m[10] == 1.0 && m[15] == 1.0;
    Some(if flach {
        format!("matrix({}, {}, {}, {}, {}, {})", t(m[0]), t(m[1]), t(m[4]), t(m[5]), t(m[12]), t(m[13]))
    } else {
        format!("matrix3d({})", m.iter().map(|v| t(*v)).collect::<Vec<_>>().join(", "))
    })
}

#[cfg(test)]
mod tests {
    use super::super::mathe::Umgebung;
    use super::*;

    fn m(t: &str) -> Option<String> {
        let kv = super::super::syntax::komponenten(t);
        let s = super::super::lang_lesen("transform", &kv)?;
        let b = Ber { u: Umgebung::default(), farbe: None, basis: None };
        transform_text(&s, &b)
    }

    #[test]
    fn matrizen() {
        assert_eq!(m("none").as_deref(), Some("none"));
        assert_eq!(m("scale(pow(1,1))").as_deref(), Some("matrix(1, 0, 0, 1, 0, 0)"));
        assert_eq!(m("scale(min(.1))").as_deref(), Some("matrix(0.1, 0, 0, 0.1, 0, 0)"));
        assert_eq!(m("rotate(90deg)").as_deref(), Some("matrix(0, 1, -1, 0, 0, 0)"));
        assert_eq!(m("rotate(min(.25turn))"), m("rotate(90deg)"));
        assert_eq!(m("translate(10px, 1in) scale(2)").as_deref(), Some("matrix(2, 0, 0, 2, 10, 96)"));
        assert_eq!(m("scale(calc(mod(1,0)))").as_deref(), Some("matrix(0, 0, 0, 0, 0, 0)"));
        assert_eq!(
            m("translateZ(5px)").as_deref(),
            Some("matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 5, 1)")
        );
        assert_eq!(m("translate(50%)"), None);
    }
}
