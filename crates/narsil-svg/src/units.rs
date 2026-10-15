//! Lengths, numbers and lists in SVG attributes and properties.

/// Which viewport dimension a percentage refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    /// sqrt((w² + h²) / 2) (radii, stroke widths).
    Diagonal,
}

/// The context lengths resolve against.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub vw: f64,
    pub vh: f64,
    pub font_size: f64,
}

impl Ctx {
    fn percent_base(&self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.vw,
            Axis::Y => self.vh,
            Axis::Diagonal => ((self.vw * self.vw + self.vh * self.vh) / 2.0).sqrt(),
        }
    }
}

/// Splits a number from its unit (`12.5px` -> (12.5, "px")).
#[must_use]
pub fn split_number(s: &str) -> Option<(f64, &str)> {
    let s = s.trim();
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == digits_start || (i == digits_start + 1 && b[digits_start] == b'.') {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    let v: f64 = s[..i].parse().ok()?;
    v.is_finite().then_some((v, s[i..].trim()))
}

/// A length in user units; `None` for invalid input.
#[must_use]
pub fn length(s: &str, axis: Axis, c: &Ctx) -> Option<f64> {
    let (v, unit) = split_number(s)?;
    Some(match unit.to_ascii_lowercase().as_str() {
        "" | "px" => v,
        "%" => v / 100.0 * c.percent_base(axis),
        "em" => v * c.font_size,
        "ex" => v * c.font_size * 0.5,
        "rem" => v * 16.0,
        "ch" => v * c.font_size * 0.5,
        "pt" => v * 4.0 / 3.0,
        "pc" => v * 16.0,
        "in" => v * 96.0,
        "cm" => v * 96.0 / 2.54,
        "mm" => v * 96.0 / 25.4,
        "q" => v * 96.0 / 101.6,
        "vw" => v * c.vw / 100.0,
        "vh" => v * c.vh / 100.0,
        "vmin" => v * c.vw.min(c.vh) / 100.0,
        "vmax" => v * c.vw.max(c.vh) / 100.0,
        _ => return None,
    })
}

/// A plain number (no unit).
#[must_use]
pub fn number(s: &str) -> Option<f64> {
    let (v, unit) = split_number(s)?;
    unit.is_empty().then_some(v)
}

/// A number or percentage as a fraction (`50%` -> 0.5, `0.5` -> 0.5).
#[must_use]
pub fn fraction(s: &str) -> Option<f64> {
    let (v, unit) = split_number(s)?;
    match unit {
        "" => Some(v),
        "%" => Some(v / 100.0),
        _ => None,
    }
}

/// Numbers separated by whitespace and/or commas.
#[must_use]
pub fn number_list(s: &str) -> Vec<f64> {
    s.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).filter_map(|t| t.parse::<f64>().ok()).filter(|v| v.is_finite()).collect()
}

/// Lengths separated by whitespace and/or commas.
#[must_use]
pub fn length_list(s: &str, axis: Axis, c: &Ctx) -> Option<Vec<f64>> {
    s.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).map(|t| length(t, axis, c)).collect()
}

/// An angle in degrees (`deg`, `rad`, `grad`, `turn`, or a bare number).
#[must_use]
pub fn angle(s: &str) -> Option<f64> {
    let (v, unit) = split_number(s)?;
    Some(match unit.to_ascii_lowercase().as_str() {
        "" | "deg" => v,
        "rad" => v.to_degrees(),
        "grad" => v * 0.9,
        "turn" => v * 360.0,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_resolve() {
        let c = Ctx { vw: 200.0, vh: 100.0, font_size: 10.0 };
        assert_eq!(length("50%", Axis::X, &c), Some(100.0));
        assert_eq!(length("2em", Axis::X, &c), Some(20.0));
        assert_eq!(length("1in", Axis::X, &c), Some(96.0));
        assert_eq!(length("1e1", Axis::X, &c), Some(10.0));
        assert_eq!(length("x", Axis::X, &c), None);
        assert_eq!(number_list("1,2 3"), vec![1.0, 2.0, 3.0]);
    }
}
