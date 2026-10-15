//! Release 15.10.2026: computed values of easing functions.
//!
//! The computed value of `linear()` lists every control point with its
//! input (CSS Easing 2 §2.1.1): a stop with two percentages is two points,
//! the first and last points without an input get 0% and 100%, an input
//! below an earlier one is raised to it, and runs without inputs are spread
//! evenly between their neighbours (WPT css-transitions transition-001).

use super::werte::{Num, S};

/// The easing list `s` with every `linear()` in its computed form; `None`
/// when it has no `linear()` or one with a stop this cannot resolve.
pub fn computed_linear(s: &[S]) -> Option<Vec<S>> {
    if !s.iter().any(|p| matches!(p, S::F(n, _) if n == "linear")) {
        return None;
    }
    s.iter()
        .map(|p| match p {
            S::F(n, args) if n == "linear" => linear_points(args).map(|a| S::F(n.clone(), a)),
            other => Some(other.clone()),
        })
        .collect()
}

fn literal(p: &S) -> Option<(f64, bool)> {
    match p {
        S::Z(Num::L(v, unit)) if unit.is_empty() => Some((*v, false)),
        S::Z(Num::L(v, unit)) if unit == "%" => Some((*v, true)),
        _ => None,
    }
}

fn linear_points(args: &[S]) -> Option<Vec<S>> {
    // (output, input in percent)
    let mut points: Vec<(f64, Option<f64>)> = Vec::new();
    for stop in args.split(|p| matches!(p, S::Komma)) {
        let mut output = None;
        let mut inputs = Vec::new();
        for p in stop {
            match literal(p)? {
                (v, false) if output.is_none() => output = Some(v),
                (v, true) if inputs.len() < 2 => inputs.push(v),
                _ => return None,
            }
        }
        let output = output?;
        if inputs.is_empty() {
            points.push((output, None));
        } else {
            points.extend(inputs.into_iter().map(|i| (output, Some(i))));
        }
    }
    if points.is_empty() {
        return None;
    }
    if points[0].1.is_none() {
        points[0].1 = Some(0.0);
    }
    let last = points.len() - 1;
    if points[last].1.is_none() {
        points[last].1 = Some(100.0_f64.max(points.iter().filter_map(|p| p.1).fold(f64::MIN, f64::max)));
    }
    let mut largest = f64::MIN;
    for p in &mut points {
        if let Some(i) = p.1.as_mut() {
            *i = i.max(largest);
            largest = *i;
        }
    }
    let mut i = 0;
    while i < points.len() {
        if points[i].1.is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let mut end = i;
        while points[end].1.is_none() {
            end += 1;
        }
        let (a, b) = (points[start].1.unwrap_or(0.0), points[end].1.unwrap_or(100.0));
        let steps = (end - start) as f64;
        for (n, p) in points[start + 1..end].iter_mut().enumerate() {
            p.1 = Some(a + (b - a) * (n + 1) as f64 / steps);
        }
        i = end;
    }
    let mut out = Vec::with_capacity(points.len() * 3);
    for (n, (output, input)) in points.into_iter().enumerate() {
        if n > 0 {
            out.push(S::Komma);
        }
        out.push(S::Z(Num::L(output, String::new())));
        out.push(S::Z(Num::L(input.unwrap_or(0.0), String::from("%"))));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> String {
        let kv = super::super::syntax::komponenten(s);
        let parsed = super::super::lang_lesen("transition-timing-function", &kv).expect("parses");
        let v = computed_linear(&parsed).expect("linear");
        super::super::werte::stueck_text(&v[0], None)
    }

    #[test]
    fn linear_points_get_their_inputs() {
        assert_eq!(text("linear(0, .5 10% 20%, 1, .5 50%, 1)"), "linear(0 0%, 0.5 10%, 0.5 20%, 1 35%, 0.5 50%, 1 100%)");
        assert_eq!(text("linear(0, 1)"), "linear(0 0%, 1 100%)");
        assert_eq!(text("linear(0 20%, 0.5 10%, 1)"), "linear(0 20%, 0.5 20%, 1 100%)");
    }
}
