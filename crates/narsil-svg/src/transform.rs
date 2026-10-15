//! `transform` attribute/property lists, `viewBox` and
//! `preserveAspectRatio`.

use narsil_canvas::Matrix;

/// Parses a transform list (`translate(10) rotate(45 5 5) matrix(...)`,
/// also CSS functions with units). `None` for invalid lists.
#[must_use]
pub fn parse(s: &str) -> Option<Matrix> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return Some(Matrix::IDENTITY);
    }
    let mut m = Matrix::IDENTITY;
    let mut rest = s;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if rest.is_empty() {
            break;
        }
        let open = rest.find('(')?;
        let name = rest[..open].trim().to_ascii_lowercase();
        let close = rest[open..].find(')')? + open;
        let args: Vec<&str> = rest[open + 1..close].split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).collect();
        rest = &rest[close + 1..];
        let num = |i: usize| -> Option<f64> { args.get(i).and_then(|t| crate::units::split_number(t)).map(|(v, _)| v) };
        let ang = |i: usize| -> Option<f64> { args.get(i).and_then(|t| crate::units::angle(t)) };
        let t = match name.as_str() {
            "matrix" if args.len() == 6 => Matrix::new(num(0)?, num(1)?, num(2)?, num(3)?, num(4)?, num(5)?),
            "translate" if !args.is_empty() => Matrix::translate(num(0)?, num(1).unwrap_or(0.0)),
            "translatex" => Matrix::translate(num(0)?, 0.0),
            "translatey" => Matrix::translate(0.0, num(0)?),
            "scale" if !args.is_empty() => {
                let x = num(0)?;
                Matrix::scale(x, num(1).unwrap_or(x))
            }
            "scalex" => Matrix::scale(num(0)?, 1.0),
            "scaley" => Matrix::scale(1.0, num(0)?),
            "rotate" if !args.is_empty() => {
                let a = ang(0)?.to_radians();
                match (num(1), num(2)) {
                    (Some(cx), Some(cy)) => Matrix::translate(cx, cy).multiply(&Matrix::rotate(a)).multiply(&Matrix::translate(-cx, -cy)),
                    _ => Matrix::rotate(a),
                }
            }
            "skewx" => Matrix::new(1.0, 0.0, ang(0)?.to_radians().tan(), 1.0, 0.0, 0.0),
            "skewy" => Matrix::new(1.0, ang(0)?.to_radians().tan(), 0.0, 1.0, 0.0, 0.0),
            "skew" => Matrix::new(1.0, ang(1).unwrap_or(0.0).to_radians().tan(), ang(0)?.to_radians().tan(), 1.0, 0.0, 0.0),
            _ => return None,
        };
        m = m.multiply(&t);
    }
    Some(m)
}

/// A `viewBox`: (min-x, min-y, width, height); `None` if invalid or
/// degenerate.
#[must_use]
pub fn view_box(s: &str) -> Option<[f64; 4]> {
    let v = crate::units::number_list(s);
    (v.len() == 4 && v[2] > 0.0 && v[3] > 0.0).then(|| [v[0], v[1], v[2], v[3]])
}

/// A `viewBox` with a zero width or height disables rendering of its
/// element (a negative one is an error and is ignored).
#[must_use]
pub fn view_box_disables(s: Option<&str>) -> bool {
    let Some(s) = s else { return false };
    let v = crate::units::number_list(s);
    v.len() == 4 && v[2] >= 0.0 && v[3] >= 0.0 && (v[2] == 0.0 || v[3] == 0.0)
}

/// The transform that maps a `viewBox` into a viewport of `w` x `h` at
/// (x, y) under `preserveAspectRatio`.
#[must_use]
pub fn view_box_transform(vb: [f64; 4], par: &str, x: f64, y: f64, w: f64, h: f64) -> Matrix {
    let mut words = par.split_whitespace();
    let mut align = words.next().unwrap_or("xMidYMid");
    if align == "defer" {
        align = words.next().unwrap_or("xMidYMid");
    }
    let slice = words.next() == Some("slice");
    let (sx, sy) = (w / vb[2], h / vb[3]);
    if align == "none" {
        return Matrix::new(sx, 0.0, 0.0, sy, x - vb[0] * sx, y - vb[1] * sy);
    }
    let s = if slice { sx.max(sy) } else { sx.min(sy) };
    let (vw, vh) = (vb[2] * s, vb[3] * s);
    let ax = if align.contains("xMin") { 0.0 } else if align.contains("xMax") { w - vw } else { (w - vw) / 2.0 };
    let ay = if align.contains("YMin") { 0.0 } else if align.contains("YMax") { h - vh } else { (h - vh) / 2.0 };
    Matrix::new(s, 0.0, 0.0, s, x + ax - vb[0] * s, y + ay - vb[1] * s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_lists() {
        let m = parse("translate(10,20) scale(2)").unwrap();
        assert_eq!(m.apply(1.0, 1.0), (12.0, 22.0));
        let r = parse("rotate(90 10 10)").unwrap();
        let (x, y) = r.apply(20.0, 10.0);
        assert!((x - 10.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9);
        assert!(parse("bogus(1)").is_none());
        let v = view_box_transform([0.0, 0.0, 10.0, 20.0], "xMidYMid", 0.0, 0.0, 100.0, 100.0);
        assert_eq!(v.apply(0.0, 0.0), (25.0, 0.0));
    }
}
