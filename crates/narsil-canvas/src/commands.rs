//! `Path2D` command lists: a flat `f64` encoding that the script side keeps,
//! replayed into a device-space [`Path`] under a transform at use time, and
//! the SVG path data parser for `new Path2D(string)`.

use crate::geom::Matrix;
use crate::path::{Path, PathError};

pub const MOVE: f64 = 0.0;
pub const LINE: f64 = 1.0;
pub const QUAD: f64 = 2.0;
pub const CUBIC: f64 = 3.0;
pub const ARC_TO: f64 = 4.0;
pub const ELLIPSE: f64 = 5.0;
pub const RECT: f64 = 6.0;
pub const ROUND_RECT: f64 = 7.0;
pub const CLOSE: f64 = 8.0;
/// `addPath(path, transform)`: followed by six matrix entries; the
/// commands up to the matching [`END_ADD`] form a separate path.
pub const BEGIN_ADD: f64 = 9.0;
pub const END_ADD: f64 = 10.0;

/// Operand count of each command.
#[must_use]
pub fn arity(op: f64) -> Option<usize> {
    Some(match op as u32 {
        0 | 1 => 2,
        2 => 4,
        3 => 6,
        4 => 5,
        5 => 8,
        6 => 4,
        7 => 12,
        8 => 0,
        9 => 6,
        10 => 0,
        _ => return None,
    })
}

/// Replays a command list into `path` with `base` as the transform.
/// Malformed tails are ignored.
pub fn replay(cmds: &[f64], base: &Matrix, path: &mut Path) -> Result<(), PathError> {
    let mut stack: Vec<(Path, Matrix)> = Vec::new();
    let mut m = *base;
    let mut i = 0;
    while i < cmds.len() {
        let op = cmds[i];
        let Some(n) = arity(op) else { break };
        if i + 1 + n > cmds.len() {
            break;
        }
        let a = &cmds[i + 1..i + 1 + n];
        i += 1 + n;
        let target: &mut Path = match stack.last_mut() {
            Some((p, _)) => p,
            None => path,
        };
        match op as u32 {
            0 => target.move_to(&m, a[0], a[1]),
            1 => target.line_to(&m, a[0], a[1]),
            2 => target.quad_to(&m, a[0], a[1], a[2], a[3]),
            3 => target.cubic_to(&m, a[0], a[1], a[2], a[3], a[4], a[5]),
            4 => target.arc_to(&m, a[0], a[1], a[2], a[3], a[4])?,
            5 => target.ellipse(&m, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7] != 0.0)?,
            6 => target.rect(&m, a[0], a[1], a[2], a[3]),
            7 => target.round_rect(&m, a[0], a[1], a[2], a[3], [(a[4], a[5]), (a[6], a[7]), (a[8], a[9]), (a[10], a[11])]),
            8 => target.close(),
            9 => {
                if stack.len() >= 64 {
                    break;
                }
                let t = Matrix::new(a[0], a[1], a[2], a[3], a[4], a[5]);
                let prev = m;
                m = m.multiply(&t);
                stack.push((Path::new(), prev));
            }
            10 => {
                if let Some((inner, prev)) = stack.pop() {
                    m = prev;
                    let outer: &mut Path = match stack.last_mut() {
                        Some((p, _)) => p,
                        None => path,
                    };
                    outer.append(&inner);
                }
            }
            _ => break,
        }
    }
    while let Some((inner, _)) = stack.pop() {
        let outer: &mut Path = match stack.last_mut() {
            Some((p, _)) => p,
            None => path,
        };
        outer.append(&inner);
    }
    Ok(())
}

/// Parses SVG path data (SVG 2 §9.3.9) into canvas commands. Parsing stops
/// at the first error; everything before it is kept.
#[must_use]
pub fn parse_svg_path(d: &str) -> Vec<f64> {
    let mut p = SvgParser { s: d.as_bytes(), i: 0 };
    let mut out: Vec<f64> = Vec::new();
    let (mut cx, mut cy) = (0.0f64, 0.0f64);
    let (mut sx, mut sy) = (0.0f64, 0.0f64);
    // Last control point for smooth curves, and the kind of the last segment.
    let mut last_ctrl: Option<(f64, f64)> = None;
    let mut last_kind = b' ';
    let mut cmd: Option<u8> = None;
    p.skip_ws();
    let mut first = true;
    while p.i < p.s.len() {
        let c = p.s[p.i];
        let explicit = c.is_ascii_alphabetic();
        if explicit {
            p.i += 1;
            cmd = Some(c);
        } else if cmd.is_none() {
            break;
        }
        let Some(mut k) = cmd else { break };
        if first && !matches!(k, b'M' | b'm') {
            break;
        }
        first = false;
        let rel = k.is_ascii_lowercase();
        let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
        p.skip_ws();
        match k.to_ascii_uppercase() {
            b'Z' => {
                out.push(CLOSE);
                cx = sx;
                cy = sy;
                last_ctrl = None;
                last_kind = b'Z';
                p.skip_ws();
                // Z takes no arguments; a number after it is an error.
                if p.i < p.s.len() && !p.s[p.i].is_ascii_alphabetic() {
                    break;
                }
                continue;
            }
            b'M' => {
                let Some((x, y)) = p.pair() else { break };
                cx = ox + x;
                cy = oy + y;
                sx = cx;
                sy = cy;
                out.extend_from_slice(&[MOVE, cx, cy]);
                last_ctrl = None;
                last_kind = b'M';
                // Further pairs are implicit line-tos.
                k = if rel { b'l' } else { b'L' };
                cmd = Some(k);
            }
            b'L' => {
                let Some((x, y)) = p.pair() else { break };
                cx = ox + x;
                cy = oy + y;
                out.extend_from_slice(&[LINE, cx, cy]);
                last_ctrl = None;
                last_kind = b'L';
            }
            b'H' => {
                let Some(x) = p.number() else { break };
                cx = ox + x;
                out.extend_from_slice(&[LINE, cx, cy]);
                last_ctrl = None;
                last_kind = b'L';
            }
            b'V' => {
                let Some(y) = p.number() else { break };
                cy = oy + y;
                out.extend_from_slice(&[LINE, cx, cy]);
                last_ctrl = None;
                last_kind = b'L';
            }
            b'C' => {
                let Some(v) = p.numbers::<6>() else { break };
                let (x1, y1, x2, y2, x, y) = (ox + v[0], oy + v[1], ox + v[2], oy + v[3], ox + v[4], oy + v[5]);
                out.extend_from_slice(&[CUBIC, x1, y1, x2, y2, x, y]);
                last_ctrl = Some((x2, y2));
                last_kind = b'C';
                cx = x;
                cy = y;
            }
            b'S' => {
                let Some(v) = p.numbers::<4>() else { break };
                let (x1, y1) = match (last_kind, last_ctrl) {
                    (b'C', Some((lx, ly))) => (2.0 * cx - lx, 2.0 * cy - ly),
                    _ => (cx, cy),
                };
                let (x2, y2, x, y) = (ox + v[0], oy + v[1], ox + v[2], oy + v[3]);
                out.extend_from_slice(&[CUBIC, x1, y1, x2, y2, x, y]);
                last_ctrl = Some((x2, y2));
                last_kind = b'C';
                cx = x;
                cy = y;
            }
            b'Q' => {
                let Some(v) = p.numbers::<4>() else { break };
                let (x1, y1, x, y) = (ox + v[0], oy + v[1], ox + v[2], oy + v[3]);
                out.extend_from_slice(&[QUAD, x1, y1, x, y]);
                last_ctrl = Some((x1, y1));
                last_kind = b'Q';
                cx = x;
                cy = y;
            }
            b'T' => {
                let Some((x, y)) = p.pair() else { break };
                let (x1, y1) = match (last_kind, last_ctrl) {
                    (b'Q', Some((lx, ly))) => (2.0 * cx - lx, 2.0 * cy - ly),
                    _ => (cx, cy),
                };
                let (x, y) = (ox + x, oy + y);
                out.extend_from_slice(&[QUAD, x1, y1, x, y]);
                last_ctrl = Some((x1, y1));
                last_kind = b'Q';
                cx = x;
                cy = y;
            }
            b'A' => {
                let Some(rx) = p.number() else { break };
                p.comma();
                let Some(ry) = p.number() else { break };
                p.comma();
                let Some(rot) = p.number() else { break };
                p.comma();
                let Some(large) = p.flag() else { break };
                p.comma();
                let Some(sweep) = p.flag() else { break };
                p.comma();
                let Some((x, y)) = p.pair() else { break };
                let (x, y) = (ox + x, oy + y);
                svg_arc(&mut out, cx, cy, rx, ry, rot, large, sweep, x, y);
                cx = x;
                cy = y;
                last_ctrl = None;
                last_kind = b'A';
            }
            _ => break,
        }
        p.comma();
    }
    out
}

/// SVG elliptical arc (endpoint parameterization, SVG 2 appendix B.2.4).
#[allow(clippy::too_many_arguments)]
fn svg_arc(out: &mut Vec<f64>, x1: f64, y1: f64, rx: f64, ry: f64, rot_deg: f64, large: bool, sweep: bool, x2: f64, y2: f64) {
    if x1 == x2 && y1 == y2 {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        out.extend_from_slice(&[LINE, x2, y2]);
        return;
    }
    let phi = rot_deg.to_radians();
    let (sp, cp) = phi.sin_cos();
    let (dx, dy) = ((x1 - x2) / 2.0, (y1 - y2) / 2.0);
    let x1p = cp * dx + sp * dy;
    let y1p = -sp * dx + cp * dy;
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coef = if den == 0.0 { 0.0 } else { (num / den).max(0.0).sqrt() };
    if large == sweep {
        coef = -coef;
    }
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;
    let cx = cp * cxp - sp * cyp + (x1 + x2) / 2.0;
    let cy = sp * cxp + cp * cyp + (y1 + y2) / 2.0;
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        a
    };
    let t1 = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dt = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    let tau = std::f64::consts::TAU;
    if !sweep && dt > 0.0 {
        dt -= tau;
    } else if sweep && dt < 0.0 {
        dt += tau;
    }
    out.extend_from_slice(&[ELLIPSE, cx, cy, rx, ry, phi, t1, t1 + dt, if dt < 0.0 { 1.0 } else { 0.0 }]);
}

struct SvgParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl SvgParser<'_> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c') {
            self.i += 1;
        }
    }

    fn comma(&mut self) {
        self.skip_ws();
        if self.i < self.s.len() && self.s[self.i] == b',' {
            self.i += 1;
        }
        self.skip_ws();
    }

    fn flag(&mut self) -> Option<bool> {
        self.skip_ws();
        let c = *self.s.get(self.i)?;
        self.i += 1;
        match c {
            b'0' => Some(false),
            b'1' => Some(true),
            _ => None,
        }
    }

    fn pair(&mut self) -> Option<(f64, f64)> {
        let x = self.number()?;
        self.comma();
        let y = self.number()?;
        Some((x, y))
    }

    fn numbers<const N: usize>(&mut self) -> Option<[f64; N]> {
        let mut v = [0.0; N];
        for (k, slot) in v.iter_mut().enumerate() {
            if k > 0 {
                self.comma();
            }
            *slot = self.number()?;
        }
        Some(v)
    }

    fn number(&mut self) -> Option<f64> {
        self.skip_ws();
        let start = self.i;
        let s = self.s;
        let mut i = self.i;
        if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
            i += 1;
        }
        let mut digits = 0;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
        if i < s.len() && s[i] == b'.' {
            i += 1;
            while i < s.len() && s[i].is_ascii_digit() {
                i += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return None;
        }
        if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
            let mut j = i + 1;
            if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
                j += 1;
            }
            if j < s.len() && s[j].is_ascii_digit() {
                while j < s.len() && s[j].is_ascii_digit() {
                    j += 1;
                }
                i = j;
            }
        }
        let text = std::str::from_utf8(&s[start..i]).ok()?;
        let v: f64 = text.parse().ok()?;
        self.i = i;
        v.is_finite().then_some(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_svg_paths() {
        let c = parse_svg_path("M 10 20 L30,40 h10 v-5 z");
        assert_eq!(c, vec![MOVE, 10.0, 20.0, LINE, 30.0, 40.0, LINE, 40.0, 40.0, LINE, 40.0, 35.0, CLOSE]);
        let c = parse_svg_path("m1 1 2 2");
        assert_eq!(c, vec![MOVE, 1.0, 1.0, LINE, 3.0, 3.0]);
        assert!(parse_svg_path("L 1 1").is_empty());
        let c = parse_svg_path("M0 0L10 0 x L 5 5");
        assert_eq!(c, vec![MOVE, 0.0, 0.0, LINE, 10.0, 0.0]);
    }

    #[test]
    fn replays_add_path_as_separate_subpaths() {
        let inner = vec![LINE, 10.0, 10.0];
        let mut cmds = vec![MOVE, 0.0, 0.0, LINE, 5.0, 0.0, BEGIN_ADD, 1.0, 0.0, 0.0, 1.0, 100.0, 0.0];
        cmds.extend(inner);
        cmds.push(END_ADD);
        let mut p = Path::new();
        replay(&cmds, &Matrix::IDENTITY, &mut p).unwrap();
        assert_eq!(p.subpaths.len(), 3);
        assert_eq!(p.subpaths[1].points, vec![(110.0, 10.0)]);
        assert_eq!(p.last_point(), Some((5.0, 0.0)));
    }
}
