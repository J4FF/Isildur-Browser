//! Porter-Duff compositing and the W3C blend modes on premultiplied colors.

/// `globalCompositeOperation` values (Compositing and Blending Level 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CompositeOp {
    #[default]
    SourceOver,
    SourceIn,
    SourceOut,
    SourceAtop,
    DestinationOver,
    DestinationIn,
    DestinationOut,
    DestinationAtop,
    Lighter,
    Copy,
    Xor,
    Clear,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

const NAMES: &[(&str, CompositeOp)] = &[
    ("source-over", CompositeOp::SourceOver),
    ("source-in", CompositeOp::SourceIn),
    ("source-out", CompositeOp::SourceOut),
    ("source-atop", CompositeOp::SourceAtop),
    ("destination-over", CompositeOp::DestinationOver),
    ("destination-in", CompositeOp::DestinationIn),
    ("destination-out", CompositeOp::DestinationOut),
    ("destination-atop", CompositeOp::DestinationAtop),
    ("lighter", CompositeOp::Lighter),
    ("copy", CompositeOp::Copy),
    ("xor", CompositeOp::Xor),
    ("clear", CompositeOp::Clear),
    ("multiply", CompositeOp::Multiply),
    ("screen", CompositeOp::Screen),
    ("overlay", CompositeOp::Overlay),
    ("darken", CompositeOp::Darken),
    ("lighten", CompositeOp::Lighten),
    ("color-dodge", CompositeOp::ColorDodge),
    ("color-burn", CompositeOp::ColorBurn),
    ("hard-light", CompositeOp::HardLight),
    ("soft-light", CompositeOp::SoftLight),
    ("difference", CompositeOp::Difference),
    ("exclusion", CompositeOp::Exclusion),
    ("hue", CompositeOp::Hue),
    ("saturation", CompositeOp::Saturation),
    ("color", CompositeOp::Color),
    ("luminosity", CompositeOp::Luminosity),
];

impl CompositeOp {
    /// Parses a keyword exactly (the canvas attribute is case-sensitive).
    #[must_use]
    pub fn parse(s: &str) -> Option<CompositeOp> {
        NAMES.iter().find(|(n, _)| *n == s).map(|(_, o)| *o)
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        NAMES.iter().find(|(_, o)| *o == self).map_or("source-over", |(n, _)| n)
    }

    /// Operators for which a transparent source still changes the
    /// destination: drawing with them affects the whole clip region.
    #[must_use]
    pub fn is_unbounded(self) -> bool {
        matches!(
            self,
            CompositeOp::Copy | CompositeOp::SourceIn | CompositeOp::SourceOut | CompositeOp::DestinationIn | CompositeOp::DestinationAtop | CompositeOp::Clear
        )
    }

    #[must_use]
    pub fn is_blend_mode(self) -> bool {
        matches!(
            self,
            CompositeOp::Multiply
                | CompositeOp::Screen
                | CompositeOp::Overlay
                | CompositeOp::Darken
                | CompositeOp::Lighten
                | CompositeOp::ColorDodge
                | CompositeOp::ColorBurn
                | CompositeOp::HardLight
                | CompositeOp::SoftLight
                | CompositeOp::Difference
                | CompositeOp::Exclusion
                | CompositeOp::Hue
                | CompositeOp::Saturation
                | CompositeOp::Color
                | CompositeOp::Luminosity
        )
    }
}

/// Composites premultiplied `s` onto premultiplied `d` (both in `0..=1`).
#[inline]
#[must_use]
pub fn composite(op: CompositeOp, s: [f32; 4], d: [f32; 4]) -> [f32; 4] {
    let (sa, da) = (s[3], d[3]);
    let pd = |fa: f32, fb: f32| [s[0] * fa + d[0] * fb, s[1] * fa + d[1] * fb, s[2] * fa + d[2] * fb, sa * fa + da * fb];
    match op {
        CompositeOp::SourceOver => pd(1.0, 1.0 - sa),
        CompositeOp::SourceIn => pd(da, 0.0),
        CompositeOp::SourceOut => pd(1.0 - da, 0.0),
        CompositeOp::SourceAtop => pd(da, 1.0 - sa),
        CompositeOp::DestinationOver => pd(1.0 - da, 1.0),
        CompositeOp::DestinationIn => pd(0.0, sa),
        CompositeOp::DestinationOut => pd(0.0, 1.0 - sa),
        CompositeOp::DestinationAtop => pd(1.0 - da, sa),
        CompositeOp::Copy => s,
        CompositeOp::Xor => pd(1.0 - da, 1.0 - sa),
        CompositeOp::Clear => [0.0; 4],
        CompositeOp::Lighter => [(s[0] + d[0]).min(1.0), (s[1] + d[1]).min(1.0), (s[2] + d[2]).min(1.0), (sa + da).min(1.0)],
        _ => blend(op, s, d),
    }
}

fn blend(op: CompositeOp, s: [f32; 4], d: [f32; 4]) -> [f32; 4] {
    let (sa, da) = (s[3], d[3]);
    if sa <= 0.0 {
        return d;
    }
    if da <= 0.0 {
        return s;
    }
    let cs = [s[0] / sa, s[1] / sa, s[2] / sa];
    let cb = [d[0] / da, d[1] / da, d[2] / da];
    let b = match op {
        CompositeOp::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        CompositeOp::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        CompositeOp::Color => set_lum(cs, lum(cb)),
        CompositeOp::Luminosity => set_lum(cb, lum(cs)),
        _ => [separable(op, cb[0], cs[0]), separable(op, cb[1], cs[1]), separable(op, cb[2], cs[2])],
    };
    let both = sa * da;
    let mut o = [0.0f32; 4];
    for i in 0..3 {
        o[i] = s[i] * (1.0 - da) + d[i] * (1.0 - sa) + both * b[i];
    }
    o[3] = sa + da * (1.0 - sa);
    o
}

fn separable(op: CompositeOp, cb: f32, cs: f32) -> f32 {
    match op {
        CompositeOp::Multiply => cb * cs,
        CompositeOp::Screen => cb + cs - cb * cs,
        CompositeOp::Overlay => hard_light(cs, cb),
        CompositeOp::Darken => cb.min(cs),
        CompositeOp::Lighten => cb.max(cs),
        CompositeOp::ColorDodge => {
            if cb == 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        CompositeOp::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        CompositeOp::HardLight => hard_light(cb, cs),
        CompositeOp::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let dcb = if cb <= 0.25 { ((16.0 * cb - 12.0) * cb + 4.0) * cb } else { cb.sqrt() };
                cb + (2.0 * cs - 1.0) * (dcb - cb)
            }
        }
        CompositeOp::Difference => (cb - cs).abs(),
        CompositeOp::Exclusion => cb + cs - 2.0 * cb * cs,
        _ => cs,
    }
}

fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 { cb * 2.0 * cs } else { let t = 2.0 * cs - 1.0; cb + t - cb * t }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 {
        let k = l - n;
        for v in &mut c {
            *v = if k == 0.0 { l } else { l + (*v - l) * l / k };
        }
    }
    if x > 1.0 {
        let k = x - l;
        for v in &mut c {
            *v = if k == 0.0 { l } else { l + (*v - l) * (1.0 - l) / k };
        }
    }
    c
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|&a, &b| c[a].total_cmp(&c[b]));
    let (min, mid, max) = (idx[0], idx[1], idx[2]);
    let mut out = [0.0f32; 3];
    if c[max] > c[min] {
        out[mid] = (c[mid] - c[min]) * s / (c[max] - c[min]);
        out[max] = s;
    }
    out[min] = 0.0;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_are_case_sensitive() {
        for (n, o) in NAMES {
            assert_eq!(CompositeOp::parse(n), Some(*o));
            assert_eq!(o.name(), *n);
        }
        assert_eq!(CompositeOp::parse("Source-Over"), None);
        assert_eq!(CompositeOp::parse("darker"), None);
    }

    #[test]
    fn destination_atop_matches_the_wpt_expectation() {
        // rgba(0,255,0,0.5) under rgba(0,0,255,0.75) -> about (0,128,128,191).
        let d = [0.0, 0.5, 0.0, 0.5];
        let s = [0.0, 0.0, 0.75, 0.75];
        let o = composite(CompositeOp::DestinationAtop, s, d);
        let a = o[3];
        let px = [o[0] / a * 255.0, o[1] / a * 255.0, o[2] / a * 255.0, a * 255.0];
        assert!((px[1] - 127.5).abs() < 3.0 && (px[2] - 127.5).abs() < 3.0 && (px[3] - 191.25).abs() < 1.0, "{px:?}");
    }

    #[test]
    fn multiply_of_opaque_colors() {
        let o = composite(CompositeOp::Multiply, [1.0, 0.5, 0.0, 1.0], [0.5, 1.0, 1.0, 1.0]);
        assert!((o[0] - 0.5).abs() < 1e-6 && (o[1] - 0.5).abs() < 1e-6 && o[2].abs() < 1e-6 && (o[3] - 1.0).abs() < 1e-6);
    }
}
