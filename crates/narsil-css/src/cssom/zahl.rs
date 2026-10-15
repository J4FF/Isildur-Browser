//! Release 12.10.2026: Zahlen schreiben (CSSOM §6.7.2 „serialize a CSS
//! component value": <number>, <percentage>, <dimension>).
//!
//! Hoechstens sechs gueltige Stellen (wie Chrome, `String::Number`), ohne
//! Exponentenschreibweise, ohne ueberfluessige Nullen; `-0` wird `0`.

/// Eine Zahl als CSS-Text.
pub fn zahl_text(f: f64) -> String {
    if f.is_nan() {
        return String::from("NaN");
    }
    if f.is_infinite() {
        return String::from(if f > 0.0 { "infinity" } else { "-infinity" });
    }
    if f == 0.0 {
        return String::from("0");
    }
    if f == f.trunc() && f.abs() < 1e15 {
        return format!("{}", f as i64);
    }
    // Sechs gueltige Stellen: ueber die Exponentenform runden.
    let e = format!("{:.5e}", f);
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let neg = mant.starts_with('-');
    let ziffern: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    if exp < 0 {
        s.push_str("0.");
        for _ in 0..(-exp - 1) {
            s.push('0');
        }
        s.push_str(&ziffern);
    } else {
        let p = (exp + 1) as usize;
        if ziffern.len() <= p {
            s.push_str(&ziffern);
            for _ in ziffern.len()..p {
                s.push('0');
            }
        } else {
            s.push_str(&ziffern[..p]);
            s.push('.');
            s.push_str(&ziffern[p..]);
        }
    }
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        return String::from("0");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zahlen() {
        assert_eq!(zahl_text(16.0), "16");
        assert_eq!(zahl_text(-0.0), "0");
        assert_eq!(zahl_text(0.5), "0.5");
        assert_eq!(zahl_text(1.0 / 3.0), "0.333333");
        assert_eq!(zahl_text(13.333333), "13.3333");
        assert_eq!(zahl_text(0.001), "0.001");
        assert_eq!(zahl_text(0.30000000000000004), "0.3");
        assert_eq!(zahl_text(-2.5), "-2.5");
        assert_eq!(zahl_text(1234.5678), "1234.57");
        assert_eq!(zahl_text(0.000123456), "0.000123456");
        assert_eq!(zahl_text(f64::INFINITY), "infinity");
    }
}
