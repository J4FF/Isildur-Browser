//! Release 11.10.2026: CSS Transforms 1/2 — `transform`, die Einzel-
//! eigenschaften `translate`/`rotate`/`scale` und `transform-origin` als
//! 2D-Affinmatrix ([`Transformation`]). GEMESSEN vorher: `transform` machte
//! nur einen Stapelkontext (`erzeugt_stapelkontext`) und wurde nie
//! angewandt — `translate(-50%, -50%)` zum Zentrieren, eingefahrene Menues
//! (`translateX(-100%)`) und `scale(0)` standen am Platz des Layouts.
//!
//! Die Verschiebung haengt ueber Prozente von der Rahmenbox ab, die erst das
//! Layout kennt: sie bleibt deshalb linear in (Breite, Hoehe) offen, und
//! `Transformation::in_seite` loest sie auf. 3D-Funktionen gelten mit ihrem
//! 2D-Anteil (`translate3d`, `scale3d`, `matrix3d`), Drehungen um x/y und
//! `perspective` als Identitaet.

use narsil_layout::Transformation;

use crate::typen::Berechnet;

/// Eine Matrix mit offener Verschiebung: `lin = [a, b, c, d]`
/// (`x' = a·x + c·y + e`, `y' = b·x + d·y + f`), `tx`/`ty` je
/// `[px, Anteil der Breite, Anteil der Hoehe]`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix {
    lin: [f32; 4],
    tx: [f32; 3],
    ty: [f32; 3],
}

impl Matrix {
    const EINS: Matrix = Matrix { lin: [1.0, 0.0, 0.0, 1.0], tx: [0.0; 3], ty: [0.0; 3] };

    fn linear(a: f32, b: f32, c: f32, d: f32) -> Matrix {
        Matrix { lin: [a, b, c, d], ..Matrix::EINS }
    }

    fn verschiebung(tx: [f32; 3], ty: [f32; 3]) -> Matrix {
        Matrix { tx, ty, ..Matrix::EINS }
    }

    /// `self · b` — erst `b`, dann `self` (die Leserichtung von `transform`).
    fn mal(self, b: Matrix) -> Matrix {
        let [a1, b1, c1, d1] = self.lin;
        let [a2, b2, c2, d2] = b.lin;
        let mut tx = [0.0; 3];
        let mut ty = [0.0; 3];
        for i in 0..3 {
            tx[i] = a1 * b.tx[i] + c1 * b.ty[i] + self.tx[i];
            ty[i] = b1 * b.tx[i] + d1 * b.ty[i] + self.ty[i];
        }
        Matrix { lin: [a1 * a2 + c1 * b2, b1 * a2 + d1 * b2, a1 * c2 + c1 * d2, b1 * c2 + d1 * d2], tx, ty }
    }
}

/// Die Transformation eines Elements, `None` ohne (oder bei einem Wert, den
/// diese Rechnung nicht versteht — dann bleibt der Kasten wie bisher).
#[must_use]
pub fn transformation(b: &Berechnet) -> Option<Transformation> {
    let wert = |n: &str| b.schluessel.iter().rev().find(|(k, _)| k == n).map(|(_, v)| v.trim().to_ascii_lowercase());
    let em = if b.schriftgroesse_px.is_finite() && b.schriftgroesse_px > 0.0 { b.schriftgroesse_px } else { 16.0 };
    let rem = if b.wurzel_schrift.is_finite() && b.wurzel_schrift > 0.0 { b.wurzel_schrift } else { 16.0 };
    let e = Einheiten { em, rem };
    let mut m = Matrix::EINS;
    let mut gesetzt = false;
    // CSS Transforms 2 §6: translate, rotate, scale, dann `transform`.
    if let Some(v) = wert("translate").filter(|v| v != "none" && !v.is_empty()) {
        m = m.mal(einzel_translate(&v, &e)?);
        gesetzt = true;
    }
    if let Some(v) = wert("rotate").filter(|v| v != "none" && !v.is_empty()) {
        m = m.mal(einzel_rotate(&v)?);
        gesetzt = true;
    }
    if let Some(v) = wert("scale").filter(|v| v != "none" && !v.is_empty()) {
        m = m.mal(einzel_scale(&v)?);
        gesetzt = true;
    }
    if let Some(v) = wert("transform").filter(|v| v != "none" && !v.is_empty()) {
        m = m.mal(transform_liste(&v, &e)?);
        gesetzt = true;
    }
    if !gesetzt || m == Matrix::EINS {
        return None;
    }
    let ursprung = wert("transform-origin").and_then(|v| ursprung(&v, &e)).unwrap_or([0.0, 0.5, 0.0, 0.5]);
    Some(Transformation { lin: m.lin, tx: m.tx, ty: m.ty, ursprung })
}

struct Einheiten {
    em: f32,
    rem: f32,
}

/// Eine Laenge als `[px, Anteil der Breite, Anteil der Hoehe]`; Prozente
/// beziehen sich auf die Achse `waagerecht` (Breite) oder die Hoehe.
fn laenge(t: &str, e: &Einheiten, waagerecht: bool) -> Option<[f32; 3]> {
    let t = t.trim();
    if let Some(innen) = t.strip_prefix("calc(").and_then(|r| r.strip_suffix(')')) {
        return summe(innen, e, waagerecht);
    }
    if let Some(innen) = t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        return summe(innen, e, waagerecht);
    }
    let anteil = |p: f32| if waagerecht { [0.0, p / 100.0, 0.0] } else { [0.0, 0.0, p / 100.0] };
    if let Some(z) = t.strip_suffix('%') {
        return z.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map(anteil);
    }
    let (zahl, faktor) = [
        ("px", 1.0),
        ("rem", e.rem),
        ("em", e.em),
        ("cm", 96.0 / 2.54),
        ("mm", 96.0 / 25.4),
        ("in", 96.0),
        ("pt", 96.0 / 72.0),
        ("pc", 16.0),
        ("q", 96.0 / 101.6),
    ]
    .iter()
    .find_map(|(einheit, f)| t.strip_suffix(einheit).map(|z| (z, *f)))
    .unwrap_or((t, f32::NAN));
    let v = zahl.trim().parse::<f32>().ok().filter(|v| v.is_finite())?;
    if faktor.is_nan() {
        // Ohne Einheit nur die Null.
        return (v == 0.0).then_some([0.0; 3]);
    }
    Some([v * faktor, 0.0, 0.0])
}

/// `calc()`-Summe: Terme mit ` + `/` - ` dazwischen, je Term eine Laenge oder
/// `Zahl * Laenge` bzw. `Laenge * Zahl` / `Laenge / Zahl`.
fn summe(innen: &str, e: &Einheiten, waagerecht: bool) -> Option<[f32; 3]> {
    let mut raus = [0.0f32; 3];
    let mut vorzeichen = 1.0f32;
    let mut tiefe = 0i32;
    let mut anfang = 0usize;
    let zeichen: Vec<(usize, char)> = innen.char_indices().collect();
    let mut terme: Vec<(f32, &str)> = Vec::new();
    for (k, &(i, c)) in zeichen.iter().enumerate() {
        match c {
            '(' => tiefe += 1,
            ')' => tiefe -= 1,
            '+' | '-' if tiefe == 0 => {
                let davor = k > 0 && zeichen[k - 1].1.is_whitespace();
                let danach = zeichen.get(k + 1).is_some_and(|(_, d)| d.is_whitespace());
                if davor && danach {
                    terme.push((vorzeichen, &innen[anfang..i]));
                    vorzeichen = if c == '-' { -1.0 } else { 1.0 };
                    anfang = i + 1;
                }
            }
            _ => {}
        }
    }
    terme.push((vorzeichen, &innen[anfang..]));
    for (v, term) in terme {
        let wert = term_wert(term.trim(), e, waagerecht)?;
        for i in 0..3 {
            raus[i] += v * wert[i];
        }
    }
    Some(raus)
}

fn term_wert(term: &str, e: &Einheiten, waagerecht: bool) -> Option<[f32; 3]> {
    if let Some((a, b)) = term.split_once('*') {
        let (a, b) = (a.trim(), b.trim());
        if let Ok(z) = a.parse::<f32>() {
            return laenge(b, e, waagerecht).map(|l| l.map(|x| x * z));
        }
        let z = b.parse::<f32>().ok()?;
        return laenge(a, e, waagerecht).map(|l| l.map(|x| x * z));
    }
    if let Some((a, b)) = term.split_once('/') {
        let z = b.trim().parse::<f32>().ok().filter(|z| *z != 0.0)?;
        return laenge(a.trim(), e, waagerecht).map(|l| l.map(|x| x / z));
    }
    laenge(term, e, waagerecht)
}

/// Ein Winkel in Bogenmass.
fn winkel(t: &str) -> Option<f32> {
    let t = t.trim();
    let (zahl, faktor) = [
        ("deg", std::f32::consts::PI / 180.0),
        ("grad", std::f32::consts::PI / 200.0),
        ("rad", 1.0),
        ("turn", 2.0 * std::f32::consts::PI),
    ]
    .iter()
    .find_map(|(einheit, f)| t.strip_suffix(einheit).map(|z| (z, *f)))
    .unwrap_or((t, f32::NAN));
    let v = zahl.trim().parse::<f32>().ok().filter(|v| v.is_finite())?;
    if faktor.is_nan() {
        return (v == 0.0).then_some(0.0);
    }
    Some(v * faktor)
}

/// Eine Zahl oder ein Prozentwert (fuer `scale`).
fn faktor(t: &str) -> Option<f32> {
    let t = t.trim();
    if let Some(z) = t.strip_suffix('%') {
        return z.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map(|v| v / 100.0);
    }
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn drehung(w: f32) -> Matrix {
    // Vielfache von 90° genau (sonst bleibt bei 180° ein Rest von 1e-7).
    let glatt = |v: f32| if v.abs() < 1e-6 { 0.0 } else { v };
    let (s, c) = w.sin_cos();
    let (s, c) = (glatt(s), glatt(c));
    Matrix::linear(c, s, -s, c)
}

/// Die Funktionsliste von `transform` (`translate(…) rotate(…) …`).
fn transform_liste(v: &str, e: &Einheiten) -> Option<Matrix> {
    let mut m = Matrix::EINS;
    let mut rest = v.trim();
    let mut anzahl = 0;
    while !rest.is_empty() {
        let auf = rest.find('(')?;
        let name = rest[..auf].trim();
        let mut tiefe = 0i32;
        let mut zu = None;
        for (i, c) in rest[auf..].char_indices() {
            match c {
                '(' => tiefe += 1,
                ')' => {
                    tiefe -= 1;
                    if tiefe == 0 {
                        zu = Some(auf + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let zu = zu?;
        let innen = &rest[auf + 1..zu];
        m = m.mal(funktion(&name.to_ascii_lowercase(), innen, e)?);
        rest = rest[zu + 1..].trim_start();
        anzahl += 1;
        if anzahl > 64 {
            return None;
        }
    }
    Some(m)
}

/// Die Argumente einer Funktion (mit Kommas; ohne Kommas wie bei den
/// Einzeleigenschaften nach Leerraum), Klammern bleiben zusammen.
fn argumente(innen: &str) -> Vec<&str> {
    let mut raus = Vec::new();
    let mut tiefe = 0i32;
    let mut anfang = 0usize;
    let komma = innen.contains(',');
    for (i, c) in innen.char_indices() {
        match c {
            '(' => tiefe += 1,
            ')' => tiefe -= 1,
            ',' if tiefe == 0 && komma => {
                raus.push(innen[anfang..i].trim());
                anfang = i + 1;
            }
            c if c.is_whitespace() && tiefe == 0 && !komma => {
                if !innen[anfang..i].trim().is_empty() {
                    raus.push(innen[anfang..i].trim());
                }
                anfang = i + 1;
            }
            _ => {}
        }
    }
    if !innen[anfang..].trim().is_empty() {
        raus.push(innen[anfang..].trim());
    }
    raus
}

fn funktion(name: &str, innen: &str, e: &Einheiten) -> Option<Matrix> {
    let a = argumente(innen);
    let l = |i: usize, waagerecht: bool| a.get(i).and_then(|t| laenge(t, e, waagerecht));
    let z = |i: usize| a.get(i).and_then(|t| faktor(t));
    let w = |i: usize| a.get(i).and_then(|t| winkel(t));
    Some(match name {
        "translate" => {
            let x = l(0, true)?;
            let y = if a.len() > 1 { l(1, false)? } else { [0.0; 3] };
            Matrix::verschiebung(x, y)
        }
        "translate3d" => Matrix::verschiebung(l(0, true)?, l(1, false)?),
        "translatex" => Matrix::verschiebung(l(0, true)?, [0.0; 3]),
        "translatey" => Matrix::verschiebung([0.0; 3], l(0, false)?),
        "translatez" | "perspective" | "rotatex" | "rotatey" => Matrix::EINS,
        "scale" => {
            let sx = z(0)?;
            let sy = if a.len() > 1 { z(1)? } else { sx };
            Matrix::linear(sx, 0.0, 0.0, sy)
        }
        "scale3d" => Matrix::linear(z(0)?, 0.0, 0.0, z(1)?),
        "scalex" => Matrix::linear(z(0)?, 0.0, 0.0, 1.0),
        "scaley" => Matrix::linear(1.0, 0.0, 0.0, z(0)?),
        "scalez" => Matrix::EINS,
        "rotate" | "rotatez" => drehung(w(0)?),
        "rotate3d" => {
            // Nur die Drehung um die z-Achse ist eine 2D-Drehung.
            let (x, y, zz) = (z(0)?, z(1)?, z(2)?);
            if x == 0.0 && y == 0.0 && zz != 0.0 {
                drehung(w(3)? * zz.signum())
            } else {
                Matrix::EINS
            }
        }
        "skew" => {
            let ax = w(0)?;
            let ay = if a.len() > 1 { w(1)? } else { 0.0 };
            Matrix::linear(1.0, ay.tan(), ax.tan(), 1.0)
        }
        "skewx" => Matrix::linear(1.0, 0.0, w(0)?.tan(), 1.0),
        "skewy" => Matrix::linear(1.0, w(0)?.tan(), 0.0, 1.0),
        "matrix" => {
            if a.len() != 6 {
                return None;
            }
            let v: Vec<f32> = a.iter().map(|t| t.parse::<f32>().ok().filter(|v| v.is_finite())).collect::<Option<_>>()?;
            Matrix { lin: [v[0], v[1], v[2], v[3]], tx: [v[4], 0.0, 0.0], ty: [v[5], 0.0, 0.0] }
        }
        "matrix3d" => {
            if a.len() != 16 {
                return None;
            }
            let v: Vec<f32> = a.iter().map(|t| t.parse::<f32>().ok().filter(|v| v.is_finite())).collect::<Option<_>>()?;
            Matrix { lin: [v[0], v[1], v[4], v[5]], tx: [v[12], 0.0, 0.0], ty: [v[13], 0.0, 0.0] }
        }
        _ => return None,
    })
}

/// `translate: x [y [z]]`.
fn einzel_translate(v: &str, e: &Einheiten) -> Option<Matrix> {
    let a = argumente(v);
    let x = laenge(a.first()?, e, true)?;
    let y = match a.get(1) {
        Some(t) => laenge(t, e, false)?,
        None => [0.0; 3],
    };
    Some(Matrix::verschiebung(x, y))
}

/// `rotate: <winkel>` oder `rotate: z <winkel>` (andere Achsen: Identitaet).
fn einzel_rotate(v: &str) -> Option<Matrix> {
    let a = argumente(v);
    match a.as_slice() {
        [w] => Some(drehung(winkel(w)?)),
        [achse, w] if *achse == "z" => Some(drehung(winkel(w)?)),
        [w, achse] if *achse == "z" => Some(drehung(winkel(w)?)),
        [_, _] => Some(Matrix::EINS),
        [x, y, z, w] => {
            let (x, y, z) = (faktor(x)?, faktor(y)?, faktor(z)?);
            if x == 0.0 && y == 0.0 && z != 0.0 {
                Some(drehung(winkel(w)? * z.signum()))
            } else {
                Some(Matrix::EINS)
            }
        }
        _ => None,
    }
}

/// `scale: sx [sy [sz]]`.
fn einzel_scale(v: &str) -> Option<Matrix> {
    let a = argumente(v);
    let sx = faktor(a.first()?)?;
    let sy = match a.get(1) {
        Some(t) => faktor(t)?,
        None => sx,
    };
    Some(Matrix::linear(sx, 0.0, 0.0, sy))
}

/// `transform-origin` als `[x px, x Anteil der Breite, y px, y Anteil der
/// Hoehe]` (Anfangswert `50% 50%`).
fn ursprung(v: &str, e: &Einheiten) -> Option<[f32; 4]> {
    let a = argumente(v);
    let wort_x = |t: &str| match t {
        "left" => Some(0.0),
        "center" => Some(0.5),
        "right" => Some(1.0),
        _ => None,
    };
    let wort_y = |t: &str| match t {
        "top" => Some(0.0),
        "center" => Some(0.5),
        "bottom" => Some(1.0),
        _ => None,
    };
    let x_von = |t: &str| -> Option<[f32; 2]> {
        if let Some(p) = wort_x(t) {
            return Some([0.0, p]);
        }
        let l = laenge(t, e, true)?;
        Some([l[0], l[1]])
    };
    let y_von = |t: &str| -> Option<[f32; 2]> {
        if let Some(p) = wort_y(t) {
            return Some([0.0, p]);
        }
        let l = laenge(t, e, false)?;
        Some([l[0], l[2]])
    };
    let (x, y) = match a.as_slice() {
        [] => return None,
        [t] => {
            if let Some(p) = wort_y(t).filter(|_| *t != "center") {
                ([0.0, 0.5], [0.0, p])
            } else {
                (x_von(t)?, [0.0, 0.5])
            }
        }
        [t, u, ..] => {
            // `top left`: das senkrechte Wort zuerst.
            if wort_y(t).is_some() && *t != "center" && (wort_x(u).is_some()) {
                (x_von(u)?, y_von(t)?)
            } else {
                (x_von(t)?, y_von(u)?)
            }
        }
    };
    Some([x[0], x[1], y[0], y[1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn liste(v: &str) -> Matrix {
        transform_liste(v, &Einheiten { em: 16.0, rem: 16.0 }).expect(v)
    }

    #[test]
    fn verschiebung_mit_prozent() {
        let m = liste("translate(-50%, -50%)");
        assert_eq!(m.tx, [0.0, -0.5, 0.0]);
        assert_eq!(m.ty, [0.0, 0.0, -0.5]);
        let m = liste("translateX(calc(-100% - 2em))");
        assert_eq!(m.tx, [-32.0, -1.0, 0.0]);
    }

    #[test]
    fn reihenfolge_und_skalierung() {
        // Erst skalieren, dann verschieben: `translate(10px) scale(2)`
        // verschiebt um 10 px, die Skalierung wirkt auf das Element.
        let m = liste("translate(10px) scale(2)");
        assert_eq!(m.lin, [2.0, 0.0, 0.0, 2.0]);
        assert_eq!(m.tx[0], 10.0);
        // Umgekehrt wird die Verschiebung mitskaliert.
        let m = liste("scale(2) translate(10px)");
        assert_eq!(m.tx[0], 20.0);
    }

    #[test]
    fn drehung_und_unbekanntes() {
        let m = liste("rotate(90deg)");
        assert!((m.lin[0]).abs() < 1e-6 && (m.lin[1] - 1.0).abs() < 1e-6);
        assert!(transform_liste("translate(10vw)", &Einheiten { em: 16.0, rem: 16.0 }).is_none());
        assert!(transform_liste("frob(1)", &Einheiten { em: 16.0, rem: 16.0 }).is_none());
    }

    #[test]
    fn ursprung_woerter() {
        let e = Einheiten { em: 16.0, rem: 16.0 };
        assert_eq!(ursprung("top left", &e), Some([0.0, 0.0, 0.0, 0.0]));
        assert_eq!(ursprung("right", &e), Some([0.0, 1.0, 0.0, 0.5]));
        assert_eq!(ursprung("10px 20%", &e), Some([10.0, 0.0, 0.0, 0.2]));
    }
}
