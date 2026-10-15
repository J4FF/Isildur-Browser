//! CSS Color 4/5 functional notations for canvas colors: `rgb()`, `hsl()`,
//! `hwb()`, `lab()`, `lch()`, `oklab()`, `oklch()`, `color()`,
//! `color-mix()` and relative colors (`rgb(from red g r b)`), including
//! `none` and simple `calc()` expressions.
//!
//! Keywords and hex colors (`red`, `#0f0`, `currentcolor`, system colors)
//! are resolved by the caller through a callback, so this module needs no
//! named-color table.
//!
//! Colors in the legacy sRGB syntaxes (`rgb()`, `hsl()`, `hwb()`) keep the
//! HTML-compatible serialization (`#rrggbb` / `rgba()`); every other form
//! keeps its color space and channel values and serializes as CSS does
//! (`color(srgb 0.5 0 0.5)`, `lab(29 39 20)`).

use crate::paint::{Color, CssForm, CssSpace};

// ---- tokens ---------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Func(String),
    Num(f64),
    Pct(f64),
    Dim(f64, String),
    Hash(String),
    Comma,
    Slash,
    Open,
    Close,
    Delim(char),
}

fn is_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

fn is_name(c: u8) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == b'-'
}

/// A CSS `<number-token>` at the start of `b`: value and byte length.
fn number(b: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - start;
    }
    if int_digits == 0 && frac_digits == 0 {
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
    let v: f64 = std::str::from_utf8(&b[..i]).ok()?.parse().ok()?;
    Some((v, i))
}

fn tokenize(s: &str) -> Option<Vec<Tok>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        match c {
            b',' => out.push(Tok::Comma),
            b'/' => out.push(Tok::Slash),
            b'(' => out.push(Tok::Open),
            b')' => out.push(Tok::Close),
            b'#' => {
                let start = i + 1;
                let mut j = start;
                while j < b.len() && is_name(b[j]) {
                    j += 1;
                }
                if j == start {
                    return None;
                }
                out.push(Tok::Hash(s[start..j].to_string()));
                i = j;
                continue;
            }
            _ => {
                if let Some((v, len)) = number(&b[i..]) {
                    i += len;
                    if i < b.len() && b[i] == b'%' {
                        out.push(Tok::Pct(v));
                        i += 1;
                    } else if i < b.len() && is_name_start(b[i]) {
                        let start = i;
                        while i < b.len() && is_name(b[i]) {
                            i += 1;
                        }
                        out.push(Tok::Dim(v, s[start..i].to_ascii_lowercase()));
                    } else {
                        out.push(Tok::Num(v));
                    }
                    continue;
                }
                if is_name_start(c) || (c == b'-' && i + 1 < b.len() && (is_name_start(b[i + 1]) || b[i + 1] == b'-')) {
                    let start = i;
                    while i < b.len() && is_name(b[i]) {
                        i += 1;
                    }
                    let name = s[start..i].to_ascii_lowercase();
                    if i < b.len() && b[i] == b'(' {
                        i += 1;
                        out.push(Tok::Func(name));
                    } else {
                        out.push(Tok::Ident(name));
                    }
                    continue;
                }
                if matches!(c, b'+' | b'-' | b'*') {
                    out.push(Tok::Delim(c as char));
                } else {
                    return None;
                }
            }
        }
        i += 1;
    }
    Some(out)
}

// ---- color spaces ---------------------------------------------------------

type M3 = [[f64; 3]; 3];

fn mul(m: &M3, v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

const SRGB_TO_XYZ: M3 = [
    [0.412_390_799_265_959_34, 0.357_584_339_383_878, 0.180_480_788_401_834_3],
    [0.212_639_005_871_510_27, 0.715_168_678_767_756, 0.072_192_315_360_733_71],
    [0.019_330_818_715_591_82, 0.119_194_779_794_625_98, 0.950_532_152_249_660_7],
];
const XYZ_TO_SRGB: M3 = [
    [3.240_969_941_904_522_6, -1.537_383_177_570_094, -0.498_610_760_293_003_4],
    [-0.969_243_636_280_879_6, 1.875_967_501_507_720_2, 0.041_555_057_407_175_59],
    [0.055_630_079_696_993_66, -0.203_976_958_888_976_52, 1.056_971_514_242_878_6],
];
const P3_TO_XYZ: M3 = [
    [0.486_570_948_648_216_2, 0.265_667_693_169_093_06, 0.198_217_285_234_362_5],
    [0.228_974_564_069_748_8, 0.691_738_521_836_506_4, 0.079_286_914_093_745],
    [0.0, 0.045_113_381_858_902_64, 1.043_944_368_900_976],
];
const XYZ_TO_P3: M3 = [
    [2.493_496_911_941_425, -0.931_383_617_919_123_9, -0.402_710_784_450_716_84],
    [-0.829_488_969_561_574_7, 1.762_664_060_318_346_3, 0.023_624_685_841_943_577],
    [0.035_845_830_243_784_47, -0.076_172_389_268_041_82, 0.956_884_524_007_687_2],
];
const A98_TO_XYZ: M3 = [
    [0.576_669_042_910_130_5, 0.185_558_237_906_546_3, 0.188_228_646_234_994_7],
    [0.297_344_975_250_536_05, 0.627_363_566_255_466_1, 0.075_291_458_493_997_88],
    [0.027_031_361_386_412_34, 0.070_688_852_535_827_23, 0.991_337_536_837_638_8],
];
const XYZ_TO_A98: M3 = [
    [2.041_587_903_810_746_5, -0.565_006_974_278_859_6, -0.344_731_350_778_329_56],
    [-0.969_243_636_280_879_5, 1.875_967_501_507_720_2, 0.041_555_057_407_175_57],
    [0.013_444_280_632_031_142, -0.118_362_392_231_018_38, 1.015_174_994_391_205_4],
];
const PROPHOTO_TO_XYZ50: M3 = [
    [0.797_760_489_672_302_7, 0.135_185_837_175_740_31, 0.031_349_349_581_524_8],
    [0.288_071_128_229_293_4, 0.711_843_217_810_101_4, 0.000_085_653_960_605_259_02],
    [0.0, 0.0, 0.825_104_602_510_460_1],
];
const XYZ50_TO_PROPHOTO: M3 = [
    [1.345_798_973_102_828_1, -0.255_580_100_079_975_34, -0.051_106_285_067_534_01],
    [-0.544_622_493_902_834_7, 1.508_232_741_313_278_1, 0.020_536_032_391_479_73],
    [0.0, 0.0, 1.211_967_545_638_945_4],
];
const REC2020_TO_XYZ: M3 = [
    [0.636_958_048_301_291_4, 0.144_616_903_586_208_32, 0.168_880_975_164_172_1],
    [0.262_700_212_011_267_1, 0.677_998_071_518_870_8, 0.059_301_716_469_861_96],
    [0.0, 0.028_072_693_049_087_428, 1.060_985_057_710_791],
];
const XYZ_TO_REC2020: M3 = [
    [1.716_651_187_971_268, -0.355_670_783_776_392, -0.253_366_281_373_66],
    [-0.666_684_351_832_489, 1.616_481_236_634_939, 0.015_768_545_813_911_1],
    [0.017_639_857_445_311, -0.042_770_613_257_809, 0.942_103_121_235_474],
];
const D65_TO_D50: M3 = [
    [1.047_929_820_840_548_8, 0.022_946_793_341_019_088, -0.050_192_229_543_135_57],
    [0.029_627_815_688_159_344, 0.990_434_484_573_249, -0.017_073_825_029_385_14],
    [-0.009_243_058_152_591_178, 0.015_055_144_896_577_895, 0.751_874_289_958_000_8],
];
const D50_TO_D65: M3 = [
    [0.955_473_452_704_218_2, -0.023_098_536_874_261_423, 0.063_259_308_661_021_7],
    [-0.028_369_706_963_208_136, 1.009_995_458_005_822_6, 0.021_041_398_966_943_008],
    [0.012_314_001_688_319_899, -0.020_507_696_433_477_912, 1.330_365_936_608_075_3],
];
const XYZ_TO_LMS: M3 = [
    [0.819_022_437_996_703, 0.361_906_260_052_890_4, -0.128_873_781_520_987_9],
    [0.032_983_653_932_388_5, 0.929_286_861_586_343_4, 0.036_144_666_350_642_4],
    [0.048_177_189_359_624_2, 0.264_239_531_752_730_8, 0.633_547_828_469_430_9],
];
const LMS_TO_OKLAB: M3 = [
    [0.210_454_268_309_314, 0.793_617_774_702_305_4, -0.004_072_043_011_619_3],
    [1.977_998_532_431_168_4, -2.428_592_242_048_579_9, 0.450_593_709_617_411],
    [0.025_904_042_465_547_8, 0.782_771_712_457_529_6, -0.808_675_754_923_077_4],
];
const OKLAB_TO_LMS: M3 = [
    [1.0, 0.396_337_777_376_174_9, 0.215_803_757_309_913_6],
    [1.0, -0.105_561_345_815_658_6, -0.063_854_172_825_813_3],
    [1.0, -0.089_484_177_529_811_9, -1.291_485_548_019_409_2],
];
const LMS_TO_XYZ: M3 = [
    [1.226_879_875_845_924_3, -0.557_814_994_460_217_1, 0.281_391_045_665_964_7],
    [-0.040_575_745_214_800_8, 1.112_286_803_280_317, -0.071_711_058_065_516_4],
    [-0.076_372_936_674_660_1, -0.421_493_332_402_243_2, 1.586_924_019_836_781_6],
];
const D50_WHITE: [f64; 3] = [0.964_295_676_429_567_7, 1.0, 0.825_104_602_510_460_2];

/// sRGB transfer function, extended to negative values by symmetry.
#[must_use]
pub fn srgb_to_linear(v: f64) -> f64 {
    let a = v.abs();
    let l = if a <= 0.04045 { a / 12.92 } else { ((a + 0.055) / 1.055).powf(2.4) };
    l.copysign(v)
}

#[must_use]
pub fn linear_to_srgb(v: f64) -> f64 {
    let a = v.abs();
    let e = if a <= 0.003_130_8 { a * 12.92 } else { 1.055 * a.powf(1.0 / 2.4) - 0.055 };
    e.copysign(v)
}

fn a98_to_linear(v: f64) -> f64 {
    v.abs().powf(563.0 / 256.0).copysign(v)
}

fn linear_to_a98(v: f64) -> f64 {
    v.abs().powf(256.0 / 563.0).copysign(v)
}

fn prophoto_to_linear(v: f64) -> f64 {
    let a = v.abs();
    if a <= 16.0 / 512.0 { v / 16.0 } else { a.powf(1.8).copysign(v) }
}

fn linear_to_prophoto(v: f64) -> f64 {
    let a = v.abs();
    if a >= 1.0 / 512.0 { a.powf(1.0 / 1.8).copysign(v) } else { v * 16.0 }
}

const REC_ALPHA: f64 = 1.099_296_826_809_44;
const REC_BETA: f64 = 0.018_053_968_510_807;

fn rec2020_to_linear(v: f64) -> f64 {
    let a = v.abs();
    if a < REC_BETA * 4.5 { v / 4.5 } else { ((a + REC_ALPHA - 1.0) / REC_ALPHA).powf(1.0 / 0.45).copysign(v) }
}

fn linear_to_rec2020(v: f64) -> f64 {
    let a = v.abs();
    if a > REC_BETA { (REC_ALPHA * a.powf(0.45) - (REC_ALPHA - 1.0)).copysign(v) } else { v * 4.5 }
}

fn map3(v: [f64; 3], f: fn(f64) -> f64) -> [f64; 3] {
    [f(v[0]), f(v[1]), f(v[2])]
}

fn hsl_to_srgb(h: f64, s: f64, l: f64) -> [f64; 3] {
    // s and l in 0..=1.
    let h = h.rem_euclid(360.0);
    let f = |n: f64| {
        let k = (n + h / 30.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).min(1.0).max(-1.0)
    };
    [f(0.0), f(8.0), f(4.0)]
}

fn hwb_to_srgb(h: f64, w: f64, b: f64) -> [f64; 3] {
    // w and b in 0..=1.
    if w + b >= 1.0 {
        let gray = w / (w + b);
        return [gray; 3];
    }
    let rgb = hsl_to_srgb(h, 1.0, 0.5);
    rgb.map(|c| c * (1.0 - w - b) + w)
}

/// sRGB to HSL: hue in degrees (NaN when powerless), saturation and
/// lightness in 0..=100.
fn srgb_to_hsl(c: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = c;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    let (mut h, mut s) = (f64::NAN, 0.0);
    if d != 0.0 {
        s = if l == 0.0 || l == 1.0 { 0.0 } else { (max - l) / l.min(1.0 - l) };
        h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        } * 60.0;
    }
    if s < 0.0 {
        h += 180.0;
        s = s.abs();
    }
    if h >= 360.0 {
        h -= 360.0;
    }
    [h, s * 100.0, l * 100.0]
}

fn srgb_to_hwb(c: [f64; 3]) -> [f64; 3] {
    let h = srgb_to_hsl(c)[0];
    let w = c[0].min(c[1]).min(c[2]);
    let b = 1.0 - c[0].max(c[1]).max(c[2]);
    let h = if w + b >= 1.0 - 1e-9 { f64::NAN } else { h };
    [h, w * 100.0, b * 100.0]
}

fn xyz50_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    const E: f64 = 216.0 / 24389.0;
    const K: f64 = 24389.0 / 27.0;
    let f = |t: f64| if t > E { t.cbrt() } else { (K * t + 16.0) / 116.0 };
    let [fx, fy, fz] = [f(xyz[0] / D50_WHITE[0]), f(xyz[1] / D50_WHITE[1]), f(xyz[2] / D50_WHITE[2])];
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn lab_to_xyz50(lab: [f64; 3]) -> [f64; 3] {
    const E: f64 = 216.0 / 24389.0;
    const K: f64 = 24389.0 / 27.0;
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = lab[1] / 500.0 + fy;
    let fz = fy - lab[2] / 200.0;
    let x = if fx.powi(3) > E { fx.powi(3) } else { (116.0 * fx - 16.0) / K };
    let y = if lab[0] > K * E { fy.powi(3) } else { lab[0] / K };
    let z = if fz.powi(3) > E { fz.powi(3) } else { (116.0 * fz - 16.0) / K };
    [x * D50_WHITE[0], y * D50_WHITE[1], z * D50_WHITE[2]]
}

fn to_polar(v: [f64; 3], epsilon: f64) -> [f64; 3] {
    let c = (v[1] * v[1] + v[2] * v[2]).sqrt();
    let h = if c <= epsilon { f64::NAN } else { v[2].atan2(v[1]).to_degrees().rem_euclid(360.0) };
    [v[0], c, h]
}

fn from_polar(v: [f64; 3]) -> [f64; 3] {
    let h = if v[2].is_nan() { 0.0 } else { v[2].to_radians() };
    [v[0], v[1] * h.cos(), v[1] * h.sin()]
}

fn xyz65_to_oklab(xyz: [f64; 3]) -> [f64; 3] {
    let lms = mul(&XYZ_TO_LMS, xyz).map(f64::cbrt);
    mul(&LMS_TO_OKLAB, lms)
}

fn oklab_to_xyz65(lab: [f64; 3]) -> [f64; 3] {
    let lms = mul(&OKLAB_TO_LMS, lab).map(|v| v * v * v);
    mul(&LMS_TO_XYZ, lms)
}

/// Channels in `space` to CIE XYZ (D65). Powerless hues (NaN) count as 0.
#[must_use]
pub fn to_xyz65(space: CssSpace, c: [f64; 3]) -> [f64; 3] {
    let h0 = |h: f64| if h.is_nan() { 0.0 } else { h };
    match space {
        CssSpace::Srgb => mul(&SRGB_TO_XYZ, map3(c, srgb_to_linear)),
        CssSpace::SrgbLinear => mul(&SRGB_TO_XYZ, c),
        CssSpace::DisplayP3 => mul(&P3_TO_XYZ, map3(c, srgb_to_linear)),
        CssSpace::A98Rgb => mul(&A98_TO_XYZ, map3(c, a98_to_linear)),
        CssSpace::ProphotoRgb => mul(&D50_TO_D65, mul(&PROPHOTO_TO_XYZ50, map3(c, prophoto_to_linear))),
        CssSpace::Rec2020 => mul(&REC2020_TO_XYZ, map3(c, rec2020_to_linear)),
        CssSpace::XyzD50 => mul(&D50_TO_D65, c),
        CssSpace::XyzD65 => c,
        CssSpace::Lab => mul(&D50_TO_D65, lab_to_xyz50(c)),
        CssSpace::Lch => mul(&D50_TO_D65, lab_to_xyz50(from_polar(c))),
        CssSpace::Oklab => oklab_to_xyz65(c),
        CssSpace::Oklch => oklab_to_xyz65(from_polar(c)),
        CssSpace::Hsl => to_xyz65(CssSpace::Srgb, hsl_to_srgb(h0(c[0]), c[1] / 100.0, c[2] / 100.0)),
        CssSpace::Hwb => to_xyz65(CssSpace::Srgb, hwb_to_srgb(h0(c[0]), c[1] / 100.0, c[2] / 100.0)),
    }
}

/// CIE XYZ (D65) to channels in `space`; powerless hues come back as NaN.
#[must_use]
pub fn from_xyz65(space: CssSpace, xyz: [f64; 3]) -> [f64; 3] {
    match space {
        CssSpace::Srgb => map3(mul(&XYZ_TO_SRGB, xyz), linear_to_srgb),
        CssSpace::SrgbLinear => mul(&XYZ_TO_SRGB, xyz),
        CssSpace::DisplayP3 => map3(mul(&XYZ_TO_P3, xyz), linear_to_srgb),
        CssSpace::A98Rgb => map3(mul(&XYZ_TO_A98, xyz), linear_to_a98),
        CssSpace::ProphotoRgb => map3(mul(&XYZ50_TO_PROPHOTO, mul(&D65_TO_D50, xyz)), linear_to_prophoto),
        CssSpace::Rec2020 => map3(mul(&XYZ_TO_REC2020, xyz), linear_to_rec2020),
        CssSpace::XyzD50 => mul(&D65_TO_D50, xyz),
        CssSpace::XyzD65 => xyz,
        CssSpace::Lab => xyz50_to_lab(mul(&D65_TO_D50, xyz)),
        CssSpace::Lch => to_polar(xyz50_to_lab(mul(&D65_TO_D50, xyz)), 0.0015),
        CssSpace::Oklab => xyz65_to_oklab(xyz),
        CssSpace::Oklch => to_polar(xyz65_to_oklab(xyz), 0.000_004),
        CssSpace::Hsl => srgb_to_hsl(from_xyz65(CssSpace::Srgb, xyz)),
        CssSpace::Hwb => srgb_to_hwb(from_xyz65(CssSpace::Srgb, xyz)),
    }
}

impl CssSpace {
    fn parse(s: &str) -> Option<CssSpace> {
        Some(match s {
            "srgb" => CssSpace::Srgb,
            "srgb-linear" => CssSpace::SrgbLinear,
            "display-p3" => CssSpace::DisplayP3,
            "a98-rgb" => CssSpace::A98Rgb,
            "prophoto-rgb" => CssSpace::ProphotoRgb,
            "rec2020" => CssSpace::Rec2020,
            "xyz" | "xyz-d65" => CssSpace::XyzD65,
            "xyz-d50" => CssSpace::XyzD50,
            "lab" => CssSpace::Lab,
            "lch" => CssSpace::Lch,
            "oklab" => CssSpace::Oklab,
            "oklch" => CssSpace::Oklch,
            "hsl" => CssSpace::Hsl,
            "hwb" => CssSpace::Hwb,
            _ => return None,
        })
    }

    /// Index of the hue channel of polar spaces.
    fn hue(self) -> Option<usize> {
        match self {
            CssSpace::Lch | CssSpace::Oklch => Some(2),
            CssSpace::Hsl | CssSpace::Hwb => Some(0),
            _ => None,
        }
    }

    fn channel_names(self) -> [&'static str; 3] {
        match self {
            CssSpace::XyzD50 | CssSpace::XyzD65 => ["x", "y", "z"],
            CssSpace::Lab | CssSpace::Oklab => ["l", "a", "b"],
            CssSpace::Lch | CssSpace::Oklch => ["l", "c", "h"],
            CssSpace::Hsl => ["h", "s", "l"],
            CssSpace::Hwb => ["h", "w", "b"],
            _ => ["r", "g", "b"],
        }
    }
}

/// A color's channels in `space` (`None` for missing channels), plus
/// alpha. Missing channels survive only when no conversion is needed.
fn channels_in(c: &Color, space: CssSpace) -> ([Option<f64>; 3], Option<f64>) {
    let alpha = if c.css.is_some_and(|f| f.none & 8 != 0) { None } else { Some(c.a as f64) };
    if let Some(f) = c.css {
        if f.space == space {
            let v = [0, 1, 2].map(|i| if f.none & (1 << i) != 0 { None } else { Some(f.c[i] as f64) });
            return (v, alpha);
        }
    }
    let xyz = match c.css {
        Some(f) => to_xyz65(f.space, [0, 1, 2].map(|i| if f.none & (1 << i) != 0 { 0.0 } else { f.c[i] as f64 })),
        None => to_xyz65(CssSpace::Srgb, [c.r as f64, c.g as f64, c.b as f64]),
    };
    let v = from_xyz65(space, xyz);
    (v.map(|x| if x.is_nan() { None } else { Some(x) }), alpha)
}

/// A non-legacy color from channels in `space`.
fn css_color(space: CssSpace, c: [Option<f64>; 3], alpha: Option<f64>) -> Color {
    let vals = c.map(|v| v.unwrap_or(0.0));
    let rgb = from_xyz65(CssSpace::Srgb, to_xyz65(space, vals));
    let mut none = 0u8;
    for (i, v) in c.iter().enumerate() {
        if v.is_none() {
            none |= 1 << i;
        }
    }
    if alpha.is_none() {
        none |= 8;
    }
    let a = alpha.unwrap_or(0.0);
    let a = if a.is_nan() { 0.0 } else { a.clamp(0.0, 1.0) };
    let f = |v: f64| if v.is_finite() { v as f32 } else { 0.0 };
    Color {
        r: f(rgb[0]),
        g: f(rgb[1]),
        b: f(rgb[2]),
        a: a as f32,
        css: Some(CssForm { space, c: vals.map(f), none }),
    }
}

/// A legacy sRGB color (clamped channels, HTML serialization).
fn legacy(rgb: [f64; 3], alpha: f64) -> Color {
    let q = |v: f64| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) as f32 };
    Color { r: q(rgb[0]), g: q(rgb[1]), b: q(rgb[2]), a: q(alpha), css: None }
}

// ---- parser ---------------------------------------------------------------

/// A parsed component value.
#[derive(Clone, Copy, Debug)]
enum V {
    Num(f64),
    Pct(f64),
    Deg(f64),
    None,
}

/// Channel keywords of a relative color (`r`, `g`, `b`, `alpha`, …).
struct Channels {
    names: [&'static str; 3],
    values: [f64; 3],
    alpha: f64,
}

impl Channels {
    fn get(&self, name: &str) -> Option<f64> {
        if name == "alpha" {
            return Some(self.alpha);
        }
        self.names.iter().position(|n| *n == name).map(|i| self.values[i])
    }
}

struct Parser<'a> {
    t: &'a [Tok],
    i: usize,
    keyword: &'a dyn Fn(&str) -> Option<Color>,
    depth: u32,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.t.get(self.i).cloned();
        self.i += 1;
        t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn eat_ident(&mut self, name: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Ident(n)) if n == name) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    /// A closing parenthesis; the end of input closes open functions.
    fn close(&mut self) -> bool {
        match self.peek() {
            None => true,
            Some(Tok::Close) => {
                self.i += 1;
                true
            }
            _ => false,
        }
    }

    fn color(&mut self) -> Option<Color> {
        self.depth += 1;
        if self.depth > 16 {
            return None;
        }
        let c = match self.next()? {
            Tok::Hash(h) => (self.keyword)(&format!("#{h}")),
            Tok::Ident(name) => (self.keyword)(&name),
            Tok::Func(name) => match name.as_str() {
                "rgb" | "rgba" => self.rgb(),
                "hsl" | "hsla" => self.hsl_hwb(CssSpace::Hsl),
                "hwb" => self.hsl_hwb(CssSpace::Hwb),
                "lab" => self.lab_like(CssSpace::Lab),
                "lch" => self.lab_like(CssSpace::Lch),
                "oklab" => self.lab_like(CssSpace::Oklab),
                "oklch" => self.lab_like(CssSpace::Oklch),
                "color" => self.color_function(),
                "color-mix" => self.color_mix(),
                _ => None,
            },
            _ => None,
        };
        self.depth -= 1;
        c
    }

    /// One component: number, percentage, angle, `none`, a channel keyword
    /// or `calc()`.
    fn value(&mut self, ch: Option<&Channels>) -> Option<V> {
        match self.next()? {
            Tok::Num(v) => Some(V::Num(v)),
            Tok::Pct(v) => Some(V::Pct(v)),
            Tok::Dim(v, unit) => angle(v, &unit).map(V::Deg),
            Tok::Ident(name) if name == "none" => Some(V::None),
            Tok::Ident(name) => ch.and_then(|c| c.get(&name)).map(V::Num),
            Tok::Func(name) if name == "calc" => {
                let v = self.sum(ch)?;
                if !self.close() {
                    return None;
                }
                Some(V::Num(v))
            }
            _ => None,
        }
    }

    // calc(): sum := product (('+' | '-') product)*
    fn sum(&mut self, ch: Option<&Channels>) -> Option<f64> {
        let mut v = self.product(ch)?;
        loop {
            match self.peek() {
                Some(Tok::Delim('+')) => {
                    self.i += 1;
                    v += self.product(ch)?;
                }
                Some(Tok::Delim('-')) => {
                    self.i += 1;
                    v -= self.product(ch)?;
                }
                _ => return Some(v),
            }
        }
    }

    fn product(&mut self, ch: Option<&Channels>) -> Option<f64> {
        let mut v = self.unit(ch)?;
        loop {
            match self.peek() {
                Some(Tok::Delim('*')) => {
                    self.i += 1;
                    v *= self.unit(ch)?;
                }
                Some(Tok::Slash) => {
                    self.i += 1;
                    v /= self.unit(ch)?;
                }
                _ => return Some(v),
            }
        }
    }

    fn unit(&mut self, ch: Option<&Channels>) -> Option<f64> {
        match self.next()? {
            Tok::Num(v) => Some(v),
            Tok::Dim(v, unit) => angle(v, &unit),
            Tok::Ident(name) => match name.as_str() {
                "e" => Some(std::f64::consts::E),
                "pi" => Some(std::f64::consts::PI),
                "infinity" => Some(f64::INFINITY),
                "-infinity" => Some(f64::NEG_INFINITY),
                "nan" => Some(f64::NAN),
                _ => ch.and_then(|c| c.get(&name)),
            },
            Tok::Open => {
                let v = self.sum(ch)?;
                self.eat(&Tok::Close).then_some(v)
            }
            Tok::Func(name) if name == "calc" => {
                let v = self.sum(ch)?;
                self.eat(&Tok::Close).then_some(v)
            }
            _ => None,
        }
    }

    /// `[ / <alpha> ]? )` of the modern syntax; `None` alpha is `none`.
    fn modern_tail(&mut self, ch: Option<&Channels>, default: Option<f64>) -> Option<Option<f64>> {
        let alpha = if self.eat(&Tok::Slash) {
            match self.value(ch)? {
                V::Num(v) => Some(v),
                V::Pct(p) => Some(p / 100.0),
                V::None => None,
                V::Deg(_) => return None,
            }
        } else {
            default
        };
        self.close().then_some(alpha)
    }

    /// `from <color>` of a relative color, converted to `space`.
    fn origin(&mut self, space: CssSpace) -> Option<Option<Channels>> {
        if !self.eat_ident("from") {
            return Some(None);
        }
        let origin = self.color()?;
        let (v, a) = channels_in(&origin, space);
        Some(Some(Channels { names: space.channel_names(), values: v.map(|x| x.unwrap_or(0.0)), alpha: a.unwrap_or(0.0) }))
    }

    fn rgb(&mut self) -> Option<Color> {
        if let Some(ch) = self.origin(CssSpace::Srgb)? {
            let scaled = Channels { names: ch.names, values: ch.values.map(|v| v * 255.0), alpha: ch.alpha };
            let mut c = [None; 3];
            for slot in &mut c {
                *slot = match self.value(Some(&scaled))? {
                    V::Num(v) => Some(v / 255.0),
                    V::Pct(p) => Some(p / 100.0),
                    V::None => None,
                    V::Deg(_) => return None,
                };
            }
            let alpha = self.modern_tail(Some(&scaled), Some(scaled.alpha))?;
            return Some(css_color(CssSpace::Srgb, c, alpha));
        }
        let first = self.value(None)?;
        let mut comps = [first, V::None, V::None];
        let alpha;
        if self.peek() == Some(&Tok::Comma) {
            // Legacy syntax: all numbers or all percentages, no `none`.
            let pct = match first {
                V::Num(_) => false,
                V::Pct(_) => true,
                _ => return None,
            };
            for slot in comps.iter_mut().skip(1) {
                if !self.eat(&Tok::Comma) {
                    return None;
                }
                *slot = self.value(None)?;
                if !matches!((*slot, pct), (V::Num(_), false) | (V::Pct(_), true)) {
                    return None;
                }
            }
            alpha = if self.eat(&Tok::Comma) { self.legacy_alpha()? } else { 1.0 };
            if !self.close() {
                return None;
            }
        } else {
            for slot in comps.iter_mut().skip(1) {
                *slot = self.value(None)?;
            }
            alpha = self.modern_tail(None, Some(1.0))?.unwrap_or(0.0);
        }
        let mut rgb = [0.0; 3];
        for (o, v) in rgb.iter_mut().zip(comps) {
            *o = match v {
                V::Num(n) => n / 255.0,
                V::Pct(p) => p / 100.0,
                V::None => 0.0,
                V::Deg(_) => return None,
            };
        }
        Some(legacy(rgb, alpha))
    }

    fn legacy_alpha(&mut self) -> Option<f64> {
        match self.value(None)? {
            V::Num(v) => Some(v),
            V::Pct(p) => Some(p / 100.0),
            _ => None,
        }
    }

    fn hsl_hwb(&mut self, space: CssSpace) -> Option<Color> {
        let hwb = space == CssSpace::Hwb;
        let rel = self.origin(space)?;
        let first = self.value(rel.as_ref())?;
        let hue = |v: V| match v {
            V::Num(h) | V::Deg(h) => Some(Some(h)),
            V::None => Some(None),
            V::Pct(_) => None,
        };
        let h = hue(first)?;
        let (s, l, alpha);
        if rel.is_none() && !hwb && self.peek() == Some(&Tok::Comma) {
            // Legacy hsl(): hue, two percentages, optional alpha.
            h?;
            let mut pcts = [0.0; 2];
            for p in &mut pcts {
                if !self.eat(&Tok::Comma) {
                    return None;
                }
                let V::Pct(v) = self.value(None)? else { return None };
                *p = v;
            }
            s = Some(pcts[0]);
            l = Some(pcts[1]);
            alpha = Some(if self.eat(&Tok::Comma) { self.legacy_alpha()? } else { 1.0 });
            if !self.close() {
                return None;
            }
        } else {
            let mut pair = [None; 2];
            for p in &mut pair {
                *p = match self.value(rel.as_ref())? {
                    V::Num(v) | V::Pct(v) => Some(v),
                    V::None => None,
                    V::Deg(_) => return None,
                };
            }
            s = pair[0];
            l = pair[1];
            alpha = self.modern_tail(rel.as_ref(), Some(rel.as_ref().map_or(1.0, |c| c.alpha)))?;
        }
        let (hv, sv, lv) = (h.unwrap_or(0.0), s.unwrap_or(0.0), l.unwrap_or(0.0));
        let rgb = if hwb { hwb_to_srgb(hv, sv / 100.0, lv / 100.0) } else { hsl_to_srgb(hv, sv.max(0.0) / 100.0, lv.clamp(0.0, 100.0) / 100.0) };
        if rel.is_some() {
            // Relative legacy colors serialize as `color(srgb …)`.
            return Some(css_color(CssSpace::Srgb, rgb.map(Some), alpha));
        }
        Some(legacy(rgb, alpha.unwrap_or(0.0)))
    }

    fn lab_like(&mut self, space: CssSpace) -> Option<Color> {
        // Percentage references and lightness range per CSS Color 4.
        let (l_max, ab_ref, c_ref) = match space {
            CssSpace::Lab | CssSpace::Lch => (100.0, 125.0, 150.0),
            _ => (1.0, 0.4, 0.4),
        };
        let polar = space.hue().is_some();
        let rel = self.origin(space)?;
        let mut c = [None; 3];
        for (i, slot) in c.iter_mut().enumerate() {
            let v = self.value(rel.as_ref())?;
            *slot = match (i, v) {
                (_, V::None) => None,
                (0, V::Num(n)) => Some(n),
                (0, V::Pct(p)) => Some(p / 100.0 * l_max),
                (2, V::Num(h) | V::Deg(h)) if polar => Some(h),
                (2, V::Pct(_)) if polar => return None,
                (_, V::Num(n)) => Some(n),
                (_, V::Pct(p)) => Some(p / 100.0 * if polar { c_ref } else { ab_ref }),
                (_, V::Deg(_)) => return None,
            };
        }
        if let Some(l) = &mut c[0] {
            *l = l.clamp(0.0, l_max);
        }
        if polar {
            if let Some(ch) = &mut c[1] {
                *ch = ch.max(0.0);
            }
        }
        let alpha = self.modern_tail(rel.as_ref(), Some(rel.as_ref().map_or(1.0, |c| c.alpha)))?;
        Some(css_color(space, c, alpha))
    }

    fn color_function(&mut self) -> Option<Color> {
        let origin = if self.eat_ident("from") { Some(self.color()?) } else { None };
        let Some(Tok::Ident(name)) = self.next() else { return None };
        let space = CssSpace::parse(&name).filter(|s| s.hue().is_none() && !matches!(s, CssSpace::Lab | CssSpace::Oklab))?;
        let rel = origin.map(|o| {
            let (v, a) = channels_in(&o, space);
            Channels { names: space.channel_names(), values: v.map(|x| x.unwrap_or(0.0)), alpha: a.unwrap_or(0.0) }
        });
        let mut c = [None; 3];
        for slot in &mut c {
            *slot = match self.value(rel.as_ref())? {
                V::Num(n) => Some(n),
                V::Pct(p) => Some(p / 100.0),
                V::None => None,
                V::Deg(_) => return None,
            };
        }
        let alpha = self.modern_tail(rel.as_ref(), Some(rel.as_ref().map_or(1.0, |c| c.alpha)))?;
        Some(css_color(space, c, alpha))
    }

    fn color_mix(&mut self) -> Option<Color> {
        let mut space = CssSpace::Oklab;
        let mut hue_method = "shorter";
        if self.eat_ident("in") {
            let Some(Tok::Ident(name)) = self.next() else { return None };
            space = CssSpace::parse(&name)?;
            if let Some(Tok::Ident(m)) = self.peek().cloned() {
                if matches!(m.as_str(), "shorter" | "longer" | "increasing" | "decreasing") {
                    space.hue()?;
                    self.i += 1;
                    if !self.eat_ident("hue") {
                        return None;
                    }
                    hue_method = match m.as_str() {
                        "longer" => "longer",
                        "increasing" => "increasing",
                        "decreasing" => "decreasing",
                        _ => "shorter",
                    };
                }
            }
            if !self.eat(&Tok::Comma) {
                return None;
            }
        }
        let mut items: Vec<(Color, Option<f64>)> = Vec::with_capacity(2);
        loop {
            let mut pct = None;
            if let Some(Tok::Pct(p)) = self.peek().cloned() {
                self.i += 1;
                pct = Some(p);
            }
            let color = self.color()?;
            if pct.is_none() {
                if let Some(Tok::Pct(p)) = self.peek().cloned() {
                    self.i += 1;
                    pct = Some(p);
                }
            }
            if pct.is_some_and(|p| !(0.0..=100.0).contains(&p)) {
                return None;
            }
            items.push((color, pct));
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        if items.len() != 2 || !self.close() {
            return None;
        }
        let (p1, p2) = match (items[0].1, items[1].1) {
            (None, None) => (50.0, 50.0),
            (Some(a), None) => (a, 100.0 - a),
            (None, Some(b)) => (100.0 - b, b),
            (Some(a), Some(b)) => (a, b),
        };
        let total = p1 + p2;
        if total <= 0.0 {
            return None;
        }
        let alpha_mult = if total < 100.0 { total / 100.0 } else { 1.0 };
        let t = p2 / total;
        let (mut c1, a1) = channels_in(&items[0].0, space);
        let (mut c2, a2) = channels_in(&items[1].0, space);
        // Missing components take the other color's value.
        for i in 0..3 {
            match (c1[i], c2[i]) {
                (None, Some(v)) => c1[i] = Some(v),
                (Some(v), None) => c2[i] = Some(v),
                _ => {}
            }
        }
        let (a1, a2) = match (a1, a2) {
            (None, Some(v)) => (v, v),
            (Some(v), None) => (v, v),
            (Some(a), Some(b)) => (a, b),
            (None, None) => (1.0, 1.0),
        };
        let alpha_missing = items[0].0.css.is_some_and(|f| f.none & 8 != 0) && items[1].0.css.is_some_and(|f| f.none & 8 != 0);
        let hue = space.hue();
        if let (Some(h), Some(x), Some(y)) = (hue, c1[hue.unwrap_or(0)], c2[hue.unwrap_or(0)]) {
            let (mut x, mut y) = (x.rem_euclid(360.0), y.rem_euclid(360.0));
            let d = y - x;
            match hue_method {
                "longer" => {
                    if 0.0 < d && d < 180.0 {
                        x += 360.0;
                    } else if -180.0 < d && d <= 0.0 {
                        y += 360.0;
                    }
                }
                "increasing" => {
                    if y < x {
                        y += 360.0;
                    }
                }
                "decreasing" => {
                    if x < y {
                        x += 360.0;
                    }
                }
                _ => {
                    if d > 180.0 {
                        x += 360.0;
                    } else if d < -180.0 {
                        y += 360.0;
                    }
                }
            }
            c1[h] = Some(x);
            c2[h] = Some(y);
        }
        let alpha = a1 + (a2 - a1) * t;
        let mut out = [None; 3];
        for i in 0..3 {
            let (Some(x), Some(y)) = (c1[i], c2[i]) else { continue };
            out[i] = Some(if Some(i) == hue {
                (x + (y - x) * t).rem_euclid(360.0)
            } else {
                // Premultiplied interpolation.
                let v = x * a1 + (y * a2 - x * a1) * t;
                if alpha > 0.0 { v / alpha } else { v }
            });
        }
        let alpha = if alpha_missing { None } else { Some(alpha * alpha_mult) };
        if matches!(space, CssSpace::Hsl | CssSpace::Hwb) {
            let v = out.map(|x| x.unwrap_or(0.0));
            let rgb = if space == CssSpace::Hsl { hsl_to_srgb(v[0], v[1] / 100.0, v[2] / 100.0) } else { hwb_to_srgb(v[0], v[1] / 100.0, v[2] / 100.0) };
            return Some(legacy(rgb, alpha.unwrap_or(0.0)));
        }
        Some(css_color(space, out, alpha))
    }
}

fn angle(v: f64, unit: &str) -> Option<f64> {
    Some(match unit {
        "deg" => v,
        "rad" => v.to_degrees(),
        "grad" => v * 0.9,
        "turn" => v * 360.0,
        _ => return None,
    })
}

/// Parses a CSS color. `keyword` resolves identifiers and hex colors
/// (`#rgb`, names, `transparent`, `currentcolor`, system colors).
#[must_use]
pub fn parse_color(s: &str, keyword: &dyn Fn(&str) -> Option<Color>) -> Option<Color> {
    let toks = tokenize(s.trim())?;
    let mut p = Parser { t: &toks, i: 0, keyword, depth: 0 };
    let c = p.color()?;
    (p.i == toks.len()).then_some(c)
}

/// Formats a channel value the way browsers serialize CSS colors: at most
/// six significant digits, no trailing zeros.
#[must_use]
pub fn format_number(v: f64) -> String {
    if !v.is_finite() || v.abs() < 1e-12 {
        return "0".into();
    }
    let magnitude = v.abs().log10().floor() as i32;
    let decimals = (5 - magnitude).clamp(0, 15) as usize;
    let mut s = format!("{v:.decimals$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

/// CSS serialization of a non-legacy color.
#[must_use]
pub fn serialize_css(f: &CssForm, alpha: f32) -> String {
    let comp = |i: usize| if f.none & (1 << i) != 0 { "none".to_string() } else { format_number(f.c[i] as f64) };
    let body = format!("{} {} {}", comp(0), comp(1), comp(2));
    let alpha_part = if f.none & 8 != 0 {
        " / none".to_string()
    } else if alpha < 1.0 {
        format!(" / {}", format_number(alpha as f64))
    } else {
        String::new()
    };
    let name = match f.space {
        CssSpace::Lab => "lab",
        CssSpace::Lch => "lch",
        CssSpace::Oklab => "oklab",
        CssSpace::Oklch => "oklch",
        CssSpace::Srgb => return format!("color(srgb {body}{alpha_part})"),
        CssSpace::SrgbLinear => return format!("color(srgb-linear {body}{alpha_part})"),
        CssSpace::DisplayP3 => return format!("color(display-p3 {body}{alpha_part})"),
        CssSpace::A98Rgb => return format!("color(a98-rgb {body}{alpha_part})"),
        CssSpace::ProphotoRgb => return format!("color(prophoto-rgb {body}{alpha_part})"),
        CssSpace::Rec2020 => return format!("color(rec2020 {body}{alpha_part})"),
        CssSpace::XyzD50 => return format!("color(xyz-d50 {body}{alpha_part})"),
        CssSpace::XyzD65 => return format!("color(xyz-d65 {body}{alpha_part})"),
        // Never stored: hsl()/hwb() results are legacy colors.
        CssSpace::Hsl | CssSpace::Hwb => return format!("color(srgb {body}{alpha_part})"),
    };
    format!("{name}({body}{alpha_part})")
}

/// sRGB (gamma-encoded, extended range) to OKLab.
#[must_use]
pub fn srgb_to_oklab(c: [f64; 3]) -> [f64; 3] {
    xyz65_to_oklab(mul(&SRGB_TO_XYZ, map3(c, srgb_to_linear)))
}

/// OKLab to linear sRGB (extended range).
#[must_use]
pub fn oklab_to_linear_srgb(lab: [f64; 3]) -> [f64; 3] {
    mul(&XYZ_TO_SRGB, oklab_to_xyz65(lab))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw(s: &str) -> Option<Color> {
        match s {
            "red" => Some(Color::from_rgba8(255, 0, 0, 255)),
            "blue" => Some(Color::from_rgba8(0, 0, 255, 255)),
            "black" => Some(Color::BLACK),
            "lime" | "#0f0" => Some(Color::from_rgba8(0, 255, 0, 255)),
            "#00f" => Some(Color::from_rgba8(0, 0, 255, 255)),
            "currentcolor" => Some(Color::from_rgba8(255, 0, 255, 255)),
            _ => None,
        }
    }

    fn ser(s: &str) -> Option<String> {
        parse_color(s, &kw).map(|c| c.serialize())
    }

    #[test]
    fn serializations_match_browsers() {
        assert_eq!(ser("rgb(255, 0, 255)").as_deref(), Some("#ff00ff"));
        assert_eq!(ser("rgba(255, 0, 255,    0)").as_deref(), Some("rgba(255, 0, 255, 0)"));
        assert_eq!(ser("color(srgb 1 0 1)").as_deref(), Some("color(srgb 1 0 1)"));
        assert_eq!(ser("color(srgb 1 0 1 / 1)").as_deref(), Some("color(srgb 1 0 1)"));
        assert_eq!(ser("color(srgb 1 0 1/0)").as_deref(), Some("color(srgb 1 0 1 / 0)"));
        assert_eq!(ser("color(srgb 1 none 1)").as_deref(), Some("color(srgb 1 none 1)"));
        assert_eq!(ser("color(srgb none -1 2 / 3)").as_deref(), Some("color(srgb none -1 2)"));
        assert_eq!(ser("color(display-p3 0.96 0.76  \t 0.79)").as_deref(), Some("color(display-p3 0.96 0.76 0.79)"));
        assert_eq!(ser("lab(29% 39 20)").as_deref(), Some("lab(29 39 20)"));
        assert_eq!(ser("rgb(none none none)").as_deref(), Some("#000000"));
        assert_eq!(ser("rgb(300 none 400)").as_deref(), Some("#ff00ff"));
    }

    #[test]
    fn mixes_and_relative_colors() {
        assert_eq!(ser("color-mix(in srgb, red, blue)").as_deref(), Some("color(srgb 0.5 0 0.5)"));
        assert_eq!(ser("color-mix(in srgb, red, color(srgb 1 0 0))").as_deref(), Some("color(srgb 1 0 0)"));
        assert_eq!(ser("color-mix(in srgb, black, currentcolor)").as_deref(), Some("color(srgb 0.5 0 0.5)"));
        assert_eq!(ser("rgb(from red g r b)").as_deref(), Some("color(srgb 0 1 0)"));
        assert_eq!(ser("color(from color(srgb 0.25 0.5 0.75 / 0.5) srgb r g b / alpha)").as_deref(), Some("color(srgb 0.25 0.5 0.75 / 0.5)"));
        assert_eq!(ser("rgb(from currentcolor g r b)").as_deref(), Some("color(srgb 0 1 1)"));
        assert_eq!(ser("rgb(from red calc(r / 2) g b)").as_deref(), Some("color(srgb 0.5 0 0)"));
    }

    #[test]
    fn legacy_syntax_is_strict() {
        assert_eq!(ser("hsl(0, 100.%, 50%)"), None);
        assert_eq!(ser("rgb(100%, 0, 0)"), None);
        assert_eq!(ser("rgba(255, 0, 0, 1.)"), None);
        assert_eq!(ser("rgb(0, 255, 0").as_deref(), Some("#00ff00"));
        assert_eq!(ser("hsl(120, 100%, 50%)").as_deref(), Some("#00ff00"));
        assert_eq!(ser("hsla(120deg, 100%, 50%, 0.5)").as_deref(), Some("rgba(0, 255, 0, 0.5)"));
        assert_eq!(ser("hwb(120 0% 0%)").as_deref(), Some("#00ff00"));
        assert_eq!(ser("rgb(1 2 3 4)"), None);
    }

    #[test]
    fn oklab_round_trips_srgb() {
        let lab = srgb_to_oklab([1.0, 0.0, 0.0]);
        assert!((lab[0] - 0.627_955).abs() < 1e-4, "{lab:?}");
        let back = oklab_to_linear_srgb(lab);
        assert!((back[0] - 1.0).abs() < 1e-6 && back[1].abs() < 1e-6, "{back:?}");
    }
}
