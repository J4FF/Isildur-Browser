//! Predefined color spaces of canvas bitmaps and `ImageData`
//! (`srgb`, `srgb-linear`, `display-p3`, `display-p3-linear`).
//!
//! Bitmaps are stored in sRGB; `getImageData`/`putImageData` convert at the
//! boundary when the image data names another color space.

/// A predefined color space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorSpace {
    #[default]
    Srgb,
    SrgbLinear,
    DisplayP3,
    DisplayP3Linear,
}

impl ColorSpace {
    #[must_use]
    pub fn parse(s: &str) -> Option<ColorSpace> {
        Some(match s {
            "srgb" => ColorSpace::Srgb,
            "srgb-linear" => ColorSpace::SrgbLinear,
            "display-p3" => ColorSpace::DisplayP3,
            "display-p3-linear" => ColorSpace::DisplayP3Linear,
            _ => return None,
        })
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ColorSpace::Srgb => "srgb",
            ColorSpace::SrgbLinear => "srgb-linear",
            ColorSpace::DisplayP3 => "display-p3",
            ColorSpace::DisplayP3Linear => "display-p3-linear",
        }
    }

    fn is_p3(self) -> bool {
        matches!(self, ColorSpace::DisplayP3 | ColorSpace::DisplayP3Linear)
    }

    fn is_linear(self) -> bool {
        matches!(self, ColorSpace::SrgbLinear | ColorSpace::DisplayP3Linear)
    }
}

fn to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(c: f64) -> f64 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

const SRGB_TO_P3: [[f64; 3]; 3] = [
    [0.822_462_1, 0.177_538_0, 0.0],
    [0.033_194_1, 0.966_805_8, 0.0],
    [0.017_082_7, 0.072_397_4, 0.910_519_9],
];
const P3_TO_SRGB: [[f64; 3]; 3] = [
    [1.224_940_1, -0.224_940_4, 0.0],
    [-0.042_056_9, 1.042_057_1, 0.0],
    [-0.019_637_6, -0.078_636_1, 1.098_273_5],
];

/// An extended-range, gamma-encoded sRGB color (CSS colors after parsing)
/// in `to`, clamped to `0..=1`.
#[must_use]
pub fn from_extended_srgb(c: [f64; 3], to: ColorSpace) -> [f64; 3] {
    let lin = c.map(|v| to_linear(v.abs()).copysign(v));
    from_linear_srgb(lin, to)
}

/// Linear-light sRGB (extended range) encoded for `to`, clamped.
#[must_use]
pub fn from_linear_srgb(lin: [f64; 3], to: ColorSpace) -> [f64; 3] {
    let mapped = if to.is_p3() {
        let m = SRGB_TO_P3;
        [
            m[0][0] * lin[0] + m[0][1] * lin[1] + m[0][2] * lin[2],
            m[1][0] * lin[0] + m[1][1] * lin[1] + m[1][2] * lin[2],
            m[2][0] * lin[0] + m[2][1] * lin[1] + m[2][2] * lin[2],
        ]
    } else {
        lin
    };
    mapped.map(|v| {
        let v = v.clamp(0.0, 1.0);
        if to.is_linear() { v } else { from_linear(v) }
    })
}

/// Converts premultiplied RGBA8 pixels in place (images drawn into a
/// canvas with another storage space).
pub fn convert_premultiplied(px: &mut [u8], from: ColorSpace, to: ColorSpace) {
    if from == to {
        return;
    }
    for p in px.chunks_exact_mut(4) {
        let a = p[3];
        if a == 0 {
            continue;
        }
        let k = 255.0 / a as f32;
        let mut u = [(p[0] as f32 * k).round().min(255.0) as u8, (p[1] as f32 * k).round().min(255.0) as u8, (p[2] as f32 * k).round().min(255.0) as u8, a];
        convert(&mut u, from, to);
        let f = a as f32 / 255.0;
        p[0] = (u[0] as f32 * f).round() as u8;
        p[1] = (u[1] as f32 * f).round() as u8;
        p[2] = (u[2] as f32 * f).round() as u8;
    }
}

/// Converts un-premultiplied RGBA8 pixels in place.
pub fn convert(px: &mut [u8], from: ColorSpace, to: ColorSpace) {
    if from == to {
        return;
    }
    // Per-channel decode table for the source transfer function.
    let mut decode = [0.0f64; 256];
    for (i, d) in decode.iter_mut().enumerate() {
        let c = i as f64 / 255.0;
        *d = if from.is_linear() { c } else { to_linear(c) };
    }
    let matrix = match (from.is_p3(), to.is_p3()) {
        (false, true) => Some(SRGB_TO_P3),
        (true, false) => Some(P3_TO_SRGB),
        _ => None,
    };
    for p in px.chunks_exact_mut(4) {
        let lin = [decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize]];
        let mapped = match matrix {
            Some(m) => [
                m[0][0] * lin[0] + m[0][1] * lin[1] + m[0][2] * lin[2],
                m[1][0] * lin[0] + m[1][1] * lin[1] + m[1][2] * lin[2],
                m[2][0] * lin[0] + m[2][1] * lin[1] + m[2][2] * lin[2],
            ],
            None => lin,
        };
        for k in 0..3 {
            let v = mapped[k].clamp(0.0, 1.0);
            let e = if to.is_linear() { v } else { from_linear(v) };
            p[k] = (e * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_red_in_display_p3() {
        let mut px = [255, 0, 0, 255];
        convert(&mut px, ColorSpace::Srgb, ColorSpace::DisplayP3);
        assert_eq!(px, [234, 51, 35, 255]);
        convert(&mut px, ColorSpace::DisplayP3, ColorSpace::Srgb);
        assert!(px[0] >= 254 && px[1] <= 2 && px[2] <= 2, "{px:?}");
    }
}
