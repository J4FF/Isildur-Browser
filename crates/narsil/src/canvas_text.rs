//! Canvas text attributes: the CSS `font` shorthand as the canvas parses
//! and serializes it, and `filter` validation.

/// Formats a CSS number the way serializers do (no trailing zeros, at most
/// six significant fraction digits).
#[must_use]
pub fn css_number(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let mut s = format!("{v:.6}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s == "-0" { "0".into() } else { s }
}

/// Splits a CSS length into its number and lower-case unit
/// (`"1e2px"` -> `(100, "px")`, `"1ex"` -> `(1, "ex")`).
#[must_use]
pub fn split_length(t: &str) -> Option<(f64, String)> {
    let s = t.trim().as_bytes();
    let mut i = 0;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    if i < s.len() && s[i] == b'.' {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == digits_start || (i == digits_start + 1 && s[digits_start] == b'.') {
        return None;
    }
    if i + 1 < s.len() && (s[i] == b'e' || s[i] == b'E') {
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
    let text = std::str::from_utf8(s).ok()?;
    let v: f64 = text[..i].parse().ok()?;
    v.is_finite().then(|| (v, text[i..].to_ascii_lowercase()))
}

/// A length in pixels; `em`/`ex`/`ch` relative to `font_px`.
#[must_use]
pub fn length_in_px(v: f64, unit: &str, font_px: f64) -> Option<f64> {
    Some(match unit {
        "px" => v,
        "" if v == 0.0 => v,
        "em" => v * font_px,
        "rem" => v * 16.0,
        "ex" | "ch" => v * font_px * 0.5,
        "rex" | "rch" => v * 8.0,
        // `ic`: the advance of a CJK ideograph, 1em when the font has none.
        "ic" => v * font_px,
        "ric" => v * 16.0,
        "cap" => v * font_px * 0.7,
        "rcap" => v * 16.0 * 0.7,
        "lh" => v * font_px * 1.2,
        "rlh" => v * 16.0 * 1.2,
        "pt" => v * 4.0 / 3.0,
        "pc" => v * 16.0,
        "in" => v * 96.0,
        "cm" => v * 96.0 / 2.54,
        "mm" => v * 96.0 / 25.4,
        "q" => v * 96.0 / 101.6,
        "vw" | "vh" | "vmin" | "vmax" => v * 8.0,
        _ => return None,
    })
}

/// A parsed canvas font.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub style: String,
    pub variant: String,
    pub weight: u16,
    pub stretch: String,
    pub size_px: f64,
    pub families: Vec<String>,
}

impl Font {
    /// The serialization the `font` getter returns.
    #[must_use]
    pub fn serialize(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.style != "normal" {
            parts.push(self.style.clone());
        }
        if self.variant != "normal" {
            parts.push(self.variant.clone());
        }
        if self.weight != 400 {
            parts.push(if self.weight == 700 { "bold".into() } else { self.weight.to_string() });
        }
        if self.stretch != "normal" {
            parts.push(self.stretch.clone());
        }
        parts.push(format!("{}px", css_number(self.size_px)));
        let fams: Vec<String> = self.families.iter().map(|f| serialize_family(f)).collect();
        parts.push(fams.join(", "));
        parts.join(" ")
    }
}

const GENERIC: &[&str] = &[
    "serif", "sans-serif", "cursive", "fantasy", "monospace", "system-ui", "emoji", "math", "fangsong", "ui-serif", "ui-sans-serif", "ui-monospace", "ui-rounded",
];

fn serialize_family(f: &str) -> String {
    if let Some(g) = f.strip_prefix('\u{1}') {
        return g.to_string();
    }
    // Identifiers that are safe unquoted stay unquoted (CSSOM: serialize
    // as identifiers when possible, otherwise as a string).
    let simple = !f.is_empty()
        && f.split(' ').all(|w| {
            !w.is_empty()
                && w.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '-' || !c.is_ascii())
                && w.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || !c.is_ascii())
                && !w.starts_with("--")
                && !GENERIC.contains(&w.to_ascii_lowercase().as_str())
                && !matches!(w.to_ascii_lowercase().as_str(), "inherit" | "initial" | "unset" | "default" | "revert" | "revert-layer")
        });
    if simple {
        return f.to_string();
    }
    let mut s = String::from("\"");
    for c in f.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => s.push_str(&format!("\\{:x} ", c as u32)),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

/// Splits the family list; `None` when it is not valid CSS.
fn parse_families(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    loop {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            return None;
        }
        if chars[i] == '"' || chars[i] == '\'' {
            let q = chars[i];
            i += 1;
            let mut name = String::new();
            loop {
                let c = *chars.get(i)?;
                i += 1;
                if c == q {
                    break;
                }
                if c == '\\' {
                    let n = *chars.get(i)?;
                    if n.is_ascii_hexdigit() {
                        let mut hex = String::new();
                        while hex.len() < 6 && chars.get(i).is_some_and(char::is_ascii_hexdigit) {
                            hex.push(chars[i]);
                            i += 1;
                        }
                        if chars.get(i).is_some_and(|c| c.is_whitespace()) {
                            i += 1;
                        }
                        name.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?).unwrap_or('\u{fffd}'));
                    } else {
                        name.push(n);
                        i += 1;
                    }
                    continue;
                }
                name.push(c);
            }
            out.push(name);
        } else {
            let mut words: Vec<String> = Vec::new();
            loop {
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                if i >= chars.len() || chars[i] == ',' {
                    break;
                }
                let mut w = String::new();
                while i < chars.len() && !chars[i].is_whitespace() && chars[i] != ',' {
                    let c = chars[i];
                    if c == '\\' {
                        i += 1;
                        w.push(*chars.get(i)?);
                        i += 1;
                        continue;
                    }
                    if !(c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()) {
                        return None;
                    }
                    w.push(c);
                    i += 1;
                }
                if w.chars().next().is_some_and(|c| c.is_ascii_digit()) || w.starts_with("--") || (w.starts_with('-') && w.chars().nth(1).is_some_and(|c| c.is_ascii_digit())) {
                    return None;
                }
                words.push(w);
            }
            if words.is_empty() {
                return None;
            }
            let lower: Vec<String> = words.iter().map(|w| w.to_ascii_lowercase()).collect();
            if words.len() == 1 && GENERIC.contains(&lower[0].as_str()) {
                out.push(format!("\u{1}{}", lower[0]));
            } else {
                if lower.iter().any(|w| matches!(w.as_str(), "inherit" | "initial" | "unset" | "default" | "revert" | "revert-layer")) {
                    return None;
                }
                out.push(words.join(" "));
            }
        }
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        if chars[i] != ',' {
            return None;
        }
        i += 1;
    }
    Some(out)
}

fn length_px(t: &str, parent: f64, parent_lh: f64) -> Option<f64> {
    let t = t.to_ascii_lowercase();
    let keyword = match t.as_str() {
        "xx-small" => Some(9.0),
        "x-small" => Some(10.0),
        "small" => Some(13.0),
        "medium" => Some(16.0),
        "large" => Some(18.0),
        "x-large" => Some(24.0),
        "xx-large" => Some(32.0),
        "xxx-large" => Some(48.0),
        "larger" => Some(parent * 1.2),
        "smaller" => Some(parent / 1.2),
        _ => None,
    };
    if keyword.is_some() {
        return keyword;
    }
    let end = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '+' || c == '-' || c == 'e')).unwrap_or(t.len());
    let (n, unit) = t.split_at(end);
    // A trailing `e` belongs to `em`/`ex`, not to an exponent.
    let (n, unit) = if n.ends_with('e') && !unit.is_empty() { (&n[..n.len() - 1], &t[end - 1..]) } else { (n, unit) };
    let v: f64 = n.parse().ok()?;
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    let px = match unit {
        "px" => v,
        "%" => v * parent / 100.0,
        "em" => v * parent,
        "rem" => v * 16.0,
        "ex" => v * parent * 0.5,
        "ch" => v * parent * 0.5,
        "ic" => v * parent,
        "cap" => v * parent * 0.7,
        "lh" => v * parent_lh,
        "rlh" => v * 16.0 * 1.2,
        "pt" => v * 4.0 / 3.0,
        "pc" => v * 16.0,
        "in" => v * 96.0,
        "cm" => v * 96.0 / 2.54,
        "mm" => v * 96.0 / 25.4,
        "q" => v * 96.0 / 101.6,
        "" if v == 0.0 => 0.0,
        _ => return None,
    };
    Some(px)
}

const SYSTEM_FONTS: &[&str] = &["caption", "icon", "menu", "message-box", "small-caption", "status-bar"];

/// Parses a canvas `font` value relative to `parent_px` (the canvas
/// element's font size); `None` keeps the previous font.
#[must_use]
pub fn parse_font_with(value: &str, parent_px: f64) -> Option<Font> {
    parse_font_in(value, parent_px, parent_px * 1.2)
}

/// [`parse_font_with`] with the parent's line height (for `lh` units).
#[must_use]
pub fn parse_font_in(value: &str, parent_px: f64, parent_lh: f64) -> Option<Font> {
    let v = value.trim();
    if v.is_empty() || v.contains(['{', '}', ';', '!']) || v.to_ascii_lowercase().contains("var(") || v.contains('(') {
        return None;
    }
    let lower = v.to_ascii_lowercase();
    if SYSTEM_FONTS.contains(&lower.as_str()) {
        return Some(Font { style: "normal".into(), variant: "normal".into(), weight: 400, stretch: "normal".into(), size_px: 13.333333333333334, families: vec!["\u{1}system-ui".into()] });
    }
    let mut font = Font { style: "normal".into(), variant: "normal".into(), weight: 400, stretch: "normal".into(), size_px: parent_px, families: Vec::new() };
    let mut rest = v;
    let (mut style, mut variant, mut weight, mut stretch) = (false, false, false, false);
    let mut normals = 0;
    loop {
        let rest_trim = rest.trim_start();
        let word_end = rest_trim.find(char::is_whitespace).unwrap_or(rest_trim.len());
        let word = &rest_trim[..word_end];
        let lw = word.to_ascii_lowercase();
        let after = &rest_trim[word_end..];
        match lw.as_str() {
            "normal" => normals += 1,
            "italic" | "oblique" if !style => {
                style = true;
                font.style = lw.clone();
            }
            "small-caps" if !variant => {
                variant = true;
                font.variant = lw.clone();
            }
            "bold" | "bolder" | "lighter" if !weight => {
                weight = true;
                font.weight = match lw.as_str() {
                    "bold" | "bolder" => 700,
                    _ => 100,
                };
            }
            "ultra-condensed" | "extra-condensed" | "condensed" | "semi-condensed" | "semi-expanded" | "expanded" | "extra-expanded" | "ultra-expanded" if !stretch => {
                stretch = true;
                font.stretch = lw.clone();
            }
            w if !weight && !w.is_empty() && w.chars().all(|c| c.is_ascii_digit()) && !after.trim().is_empty() => {
                let n: u16 = w.parse().ok()?;
                if !(1..=1000).contains(&n) {
                    return None;
                }
                weight = true;
                font.weight = n;
            }
            _ => break,
        }
        rest = after;
        if normals > 4 {
            return None;
        }
    }
    let rest = rest.trim_start();
    // font-size [/ line-height]
    let size_end = rest.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(rest.len());
    let size = &rest[..size_end];
    font.size_px = length_px(size, parent_px, parent_lh)?;
    let mut rest = rest[size_end..].trim_start();
    if let Some(r) = rest.strip_prefix('/') {
        let r = r.trim_start();
        let lh_end = r.find(char::is_whitespace).unwrap_or(r.len());
        let lh = &r[..lh_end];
        let ok = lh.eq_ignore_ascii_case("normal") || lh.parse::<f64>().is_ok() || length_px(lh, parent_px, parent_lh).is_some();
        if !ok {
            return None;
        }
        rest = r[lh_end..].trim_start();
    }
    font.families = parse_families(rest)?;
    Some(font)
}

/// `font` setter: the serialized value, or `None` to ignore the value.
#[must_use]
pub fn parse_font(value: &str) -> Option<String> {
    parse_font_with(value, 10.0).map(|f| f.serialize())
}

/// `filter`: `none` or a list of filter functions / `url()` references.
#[must_use]
pub fn valid_filter(value: &str) -> bool {
    let v = value.trim();
    if v.eq_ignore_ascii_case("none") {
        return true;
    }
    if v.is_empty() {
        return false;
    }
    let mut rest = v;
    let names = ["blur", "brightness", "contrast", "drop-shadow", "grayscale", "hue-rotate", "invert", "opacity", "saturate", "sepia", "url"];
    while !rest.is_empty() {
        let Some(open) = rest.find('(') else { return false };
        let name = rest[..open].trim().to_ascii_lowercase();
        if !names.contains(&name.as_str()) {
            return false;
        }
        let mut depth = 0;
        let mut close = None;
        for (i, c) in rest[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { return false };
        rest = rest[close + 1..].trim_start();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_round_trips() {
        assert_eq!(parse_font("20px serif").as_deref(), Some("20px serif"));
        assert_eq!(parse_font("20PX   SERIF").as_deref(), Some("20px serif"));
        assert_eq!(parse_font("small-caps italic 400 12px/2 \"Unknown Font #2\", sans-serif").as_deref(), Some("italic small-caps 12px \"Unknown Font #2\", sans-serif"));
        assert_eq!(parse_font("bogus"), None);
        assert_eq!(parse_font("inherit"), None);
        assert_eq!(parse_font("10px inherit"), None);
        assert_eq!(parse_font("10px {bogus}"), None);
        assert_eq!(parse_font("1px sans-serif").as_deref(), Some("1px sans-serif"));
        assert_eq!(parse_font("bold 10px x").as_deref(), Some("bold 10px x"));
        assert_eq!(parse_font("small-caps italic 400 12px/2 Unknown Font, sans-serif").as_deref(), Some("italic small-caps 12px Unknown Font, sans-serif"));
        assert_eq!(parse_font("italic 400 12px serif").as_deref(), Some("italic 12px serif"));
    }

    #[test]
    fn spacing_lengths() {
        assert_eq!(split_length("1ex"), Some((1.0, "ex".into())));
        assert_eq!(split_length("1e1px"), Some((10.0, "px".into())));
        assert_eq!(split_length("-2.5em"), Some((-2.5, "em".into())));
        assert_eq!(length_in_px(1.0, "em", 20.0), Some(20.0));
        assert_eq!(split_length("px"), None);
    }

    #[test]
    fn filter_validation() {
        assert!(valid_filter("none"));
        assert!(valid_filter("blur(5px) drop-shadow(1px 1px red)"));
        assert!(!valid_filter("bogus(1)"));
        assert!(!valid_filter(""));
    }
}

// --- drawing and measuring text (HTML §4.12.5.1.11) --------------------------

use narsil_text::{GlyphQuelle, Schriftstapel};
use std::sync::Arc;

/// Vertical metrics of the primary font, in pixels (positive up from the
/// alphabetic baseline, descents positive downwards).
#[derive(Clone, Copy, Debug)]
struct VMetrics {
    ascent: f64,
    descent: f64,
    em_ascent: f64,
    em_descent: f64,
    hanging: f64,
    ideographic: f64,
}

/// A laid out line of text.
struct Shaped {
    stapel: Arc<Schriftstapel>,
    size: f64,
    /// Glyphs, pen positions and glyph scale (synthesized small caps) in
    /// visual order.
    glyphs: Vec<(GlyphQuelle, f64, f64)>,
    /// Pen position of every character in logical order (text clusters).
    logical_pens: Vec<f64>,
    /// Own advance of every character (no kerning towards the next one):
    /// a text cluster drawn alone with `right`/`center` lines up with it.
    logical_advances: Vec<f64>,
    width: f64,
    v: VMetrics,
}

fn be_i16(d: &[u8], at: usize) -> Option<i16> {
    d.get(at..at + 2).map(|b| i16::from_be_bytes([b[0], b[1]]))
}

fn be_u16(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

/// `hang` and `ideo` coordinates (font units) from the OpenType `BASE`
/// table's horizontal axis (first script record).
fn base_baselines(font: &narsil_text::TrueTypeFont) -> (Option<i16>, Option<i16>) {
    let Some(d) = font.table(b"BASE") else { return (None, None) };
    let parse = || -> Option<(Option<i16>, Option<i16>)> {
        let horiz = be_u16(d, 4)? as usize;
        if horiz == 0 {
            return None;
        }
        let tag_list = be_u16(d, horiz)? as usize;
        let script_list = be_u16(d, horiz + 2)? as usize;
        let tags_at = horiz + tag_list;
        let ntags = be_u16(d, tags_at)? as usize;
        let mut tags = Vec::with_capacity(ntags);
        for i in 0..ntags {
            tags.push(d.get(tags_at + 2 + i * 4..tags_at + 6 + i * 4)?.to_vec());
        }
        let sl = horiz + script_list;
        let nscripts = be_u16(d, sl)? as usize;
        // Prefer `latn`, else the first script.
        let mut pick = None;
        for i in 0..nscripts {
            let rec = sl + 2 + i * 6;
            let tag = d.get(rec..rec + 4)?;
            let off = be_u16(d, rec + 4)? as usize;
            if pick.is_none() || tag == b"latn" {
                pick = Some(sl + off);
            }
        }
        let script = pick?;
        let values = be_u16(d, script)? as usize;
        if values == 0 {
            return None;
        }
        let bv = script + values;
        let ncoords = be_u16(d, bv + 2)? as usize;
        let mut hang = None;
        let mut ideo = None;
        for i in 0..ncoords.min(tags.len()) {
            let off = be_u16(d, bv + 4 + i * 2)? as usize;
            let coord = be_i16(d, bv + off + 2)?;
            match tags[i].as_slice() {
                b"hang" => hang = Some(coord),
                b"ideo" => ideo = Some(coord),
                _ => {}
            }
        }
        Some((hang, ideo))
    };
    parse().unwrap_or((None, None))
}

fn vertical_metrics(stapel: &Schriftstapel, size: f64) -> VMetrics {
    let Some(font) = stapel.schrift(0).map(|s| &s.font) else {
        return VMetrics { ascent: size * 0.8, descent: size * 0.2, em_ascent: size * 0.8, em_descent: size * 0.2, hanging: size * 0.64, ideographic: -size * 0.2 };
    };
    let upem = font.units_per_em().max(1) as f64;
    // hhea ascender/descender, exact (font units).
    let (ha, hd) = font
        .table(b"hhea")
        .and_then(|d| Some((be_i16(d, 4)? as f64, be_i16(d, 6)? as f64)))
        .unwrap_or_else(|| {
            let z = font.zeilenmetriken();
            (z.ascent_em as f64 * upem, z.descent_em as f64 * upem)
        });
    let os2 = font.table(b"OS/2");
    // OS/2 sTypoAscender/sTypoDescender define the em box (normalized to 1em).
    let typo = os2.and_then(|d| Some((be_i16(d, 68)? as f64, be_i16(d, 70)? as f64)));
    // fsSelection bit 7 (USE_TYPO_METRICS): the typo values are the line metrics.
    let use_typo = os2.and_then(|d| be_u16(d, 62)).is_some_and(|f| f & 0x80 != 0);
    let (ascent, descent) = match typo {
        Some((a, d)) if use_typo => (a / upem * size, -d / upem * size),
        _ => (ha / upem * size, -hd / upem * size),
    };
    let (ta, td) = typo.filter(|(a, d)| a - d > 0.0).unwrap_or((ha, hd));
    let em_ascent = ta / (ta - td) * size;
    let em_descent = size - em_ascent;
    let (hang, ideo) = base_baselines(font);
    let hanging = hang.map_or(ascent * 0.8, |h| h as f64 / upem * size);
    let ideographic = ideo.map_or(-descent, |i| i as f64 / upem * size);
    VMetrics { ascent, descent, em_ascent, em_descent, hanging, ideographic }
}

/// The parsed font of the state and its font stack.
fn resolve_font(font: &str, stretch: &str) -> Option<(Font, Arc<Schriftstapel>)> {
    let f = parse_font_with(font, 10.0)?;
    let families: Vec<String> = f.families.iter().map(|s| s.trim_start_matches('\u{1}').to_string()).collect();
    let list = families.join(", ");
    let italic = f.style != "normal";
    let stretch = stretch_tenths(stretch);
    if let Some(t) = narsil_text::webschrift::find_with_stretch(&list, f.weight, italic, stretch) {
        return Some((f, t.stapel));
    }
    let stapel = narsil_text::stapel_fuer_familie(&list, f.weight, italic).or_else(|| narsil_text::stapel_fuer_familie("sans-serif", f.weight, italic))?;
    Some((f, stapel))
}

/// Canvas `fontStretch` keyword in tenths of a percent.
fn stretch_tenths(s: &str) -> u16 {
    match s {
        "ultra-condensed" => 500,
        "extra-condensed" => 625,
        "condensed" => 750,
        "semi-condensed" => 875,
        "semi-expanded" => 1125,
        "expanded" => 1250,
        "extra-expanded" => 1500,
        "ultra-expanded" => 2000,
        _ => narsil_text::webschrift::NORMAL_STRETCH,
    }
}

/// Mirrored glyph for characters at right-to-left levels (UAX #9 L4,
/// the common paired brackets).
fn mirrored(ch: char) -> char {
    match ch {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '\u{ab}' => '\u{bb}',
        '\u{bb}' => '\u{ab}',
        '\u{2039}' => '\u{203a}',
        '\u{203a}' => '\u{2039}',
        c => c,
    }
}

fn is_variation_selector(ch: char) -> bool {
    matches!(ch as u32, 0xFE00..=0xFE0F | 0xE0100..=0xE01EF)
}

/// The glyph for `ch`; `Some(true)` prefers a color (emoji) font,
/// `Some(false)` a plain one (VS16 / VS15).
fn glyph_with_presentation(stapel: &Schriftstapel, ch: char, emoji: Option<bool>) -> GlyphQuelle {
    if let Some(want_color) = emoji {
        for i in 0..stapel.len() {
            let Some(f) = stapel.schrift(i) else { continue };
            let color = [b"COLR", b"CBDT", b"sbix", b"SVG "].iter().any(|t| f.font.table(t).is_some());
            if color != want_color {
                continue;
            }
            if let Some(g) = f.font.glyph_index(ch).filter(|g| *g != 0) {
                return GlyphQuelle { schrift: i, glyph: g };
            }
        }
    }
    stapel.glyphe_fuer(ch).unwrap_or(GlyphQuelle::NOTDEF)
}

/// A character (or a ligature of several) on its way to a glyph.
struct Item {
    /// Logical index of the character; `merged` lists the others a
    /// ligature consumed.
    li: usize,
    merged: Vec<usize>,
    ch: char,
    q: GlyphQuelle,
    scale: f64,
}

/// Applies GSUB to the runs of items that share a font.
fn substitute_runs(stapel: &Schriftstapel, items: Vec<Item>, script: &[u8; 4], lang: Option<&[u8; 4]>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::with_capacity(items.len());
    let mut run: Vec<Item> = Vec::new();
    let flush = |run: &mut Vec<Item>, out: &mut Vec<Item>| {
        if run.is_empty() {
            return;
        }
        let gsub = stapel.schrift(run[0].q.schrift).and_then(|f| f.font.table(b"GSUB"));
        let Some(gsub) = gsub.filter(|_| run.len() > 1) else {
            out.append(run);
            return;
        };
        let mut glyphs: Vec<u32> = run.iter().map(|it| it.q.glyph).collect();
        let mut clusters: Vec<Vec<usize>> = (0..run.len()).map(|i| vec![i]).collect();
        crate::canvas_gsub::substitute(gsub, script, lang, &mut glyphs, &mut clusters);
        for (g, cl) in glyphs.into_iter().zip(clusters) {
            let first = &run[cl[0]];
            let mut merged = first.merged.clone();
            for &k in &cl[1..] {
                merged.push(run[k].li);
                merged.extend(run[k].merged.iter().copied());
            }
            out.push(Item { li: first.li, merged, ch: first.ch, q: GlyphQuelle { schrift: first.q.schrift, glyph: g }, scale: first.scale });
        }
        run.clear();
    };
    for it in items {
        let joins = it.q != GlyphQuelle::NOTDEF && it.scale == 1.0 && it.ch != '\0';
        if !joins {
            flush(&mut run, &mut out);
            out.push(it);
            continue;
        }
        if run.last().is_some_and(|r| r.q.schrift != it.q.schrift) {
            flush(&mut run, &mut out);
        }
        run.push(it);
    }
    flush(&mut run, &mut out);
    out
}

/// Lays out `text`: UAX #9 reordering for the paragraph direction, then
/// glyphs (with OpenType ligatures for `lang`), advances, kerning and
/// spacing in visual order.
fn shape(state: &narsil_canvas::State, text: &str, rtl: bool, lang: Option<&str>) -> Option<Shaped> {
    let (font, stapel) = resolve_font(&state.text.font, &state.text.font_stretch)?;
    let size = font.size_px;
    let kerning = state.text.font_kerning != "none";
    let px = |s: &str| split_length(s).and_then(|(v, u)| length_in_px(v, &u, size)).unwrap_or(0.0);
    let (ls, ws) = (px(&state.text.letter_spacing), px(&state.text.word_spacing));
    // Small caps are synthesized: lower-case letters (all letters for the
    // `all-` values) become capitals at 70 % size.
    let caps = state.text.font_variant_caps.as_str();
    let small_lower = matches!(caps, "small-caps" | "petite-caps" | "all-small-caps" | "all-petite-caps");
    let small_upper = matches!(caps, "all-small-caps" | "all-petite-caps");
    // "Replace all ASCII whitespace in text with U+0020 SPACE".
    let chars: Vec<char> = text.chars().map(|c| if matches!(c, '\t' | '\n' | '\x0c' | '\r') { ' ' } else { c }).collect();
    let (order, levels): (Vec<usize>, Vec<u8>) = if !rtl && text.is_ascii() {
        ((0..chars.len()).collect(), vec![0; chars.len()])
    } else {
        let dir = if rtl { narsil_text::bidi::Absatzrichtung::Rtl } else { narsil_text::bidi::Absatzrichtung::Ltr };
        let a = narsil_text::bidi::analysiere_zeichen(&chars, dir);
        let levels = a.ebenen.iter().map(|l| l.unwrap_or(0)).collect();
        (a.reihenfolge, levels)
    };
    // One item per character, in visual order.
    let mut items: Vec<Item> = Vec::with_capacity(order.len());
    for &li in &order {
        let mut ch = chars[li];
        if levels.get(li).is_some_and(|l| l % 2 == 1) {
            ch = mirrored(ch);
        }
        let mut scale = 1.0;
        if (small_lower && ch.is_lowercase()) || (small_upper && ch.is_uppercase()) {
            let mut up = ch.to_uppercase();
            if let (Some(u), None) = (up.next(), up.next()) {
                ch = u;
                scale = 0.7;
            }
        }
        // Variation selectors take no room; VS15/VS16 before them pick a
        // text or an emoji font for the base character.
        if is_variation_selector(ch) {
            items.push(Item { li, merged: Vec::new(), ch: '\0', q: GlyphQuelle::NOTDEF, scale });
            continue;
        }
        let presentation = match chars.get(li + 1) {
            Some('\u{fe0f}') => Some(true),
            Some('\u{fe0e}') => Some(false),
            _ => None,
        };
        let q = if ch == '\0' { GlyphQuelle::NOTDEF } else { glyph_with_presentation(&stapel, ch, presentation) };
        items.push(Item { li, merged: Vec::new(), ch, q, scale });
    }
    // OpenType ligatures (GSUB) in left-to-right text; letter spacing and
    // `textRendering: optimizeSpeed` turn them off.
    if ls == 0.0 && state.text.text_rendering != "optimizeSpeed" && levels.iter().all(|l| l % 2 == 0) {
        let script = crate::canvas_gsub::script_for(&chars);
        let lang_tag = lang.and_then(crate::canvas_gsub::language_tag);
        items = substitute_runs(&stapel, items, &script, lang_tag.as_ref());
    }
    let mut glyphs = Vec::with_capacity(items.len());
    let mut logical_pens = vec![f64::NAN; chars.len()];
    let mut logical_advances = vec![0.0f64; chars.len()];
    let mut x = 0.0f64;
    let mut prev: Option<GlyphQuelle> = None;
    for it in &items {
        let set_pens = |pens: &mut Vec<f64>, x: f64| {
            for &li in std::iter::once(&it.li).chain(it.merged.iter()) {
                if li < pens.len() {
                    pens[li] = x;
                }
            }
        };
        set_pens(&mut logical_pens, x);
        // NUL has no advance (`measureText('\0').width` is 0); other control
        // characters keep their (.notdef) glyph and are not collapsed.
        if it.ch == '\0' {
            glyphs.push((GlyphQuelle::NOTDEF, x, 1.0));
            continue;
        }
        let q = it.q;
        if kerning && let Some(p) = prev {
            x += stapel.kern(p, q, size as f32) as f64;
        }
        set_pens(&mut logical_pens, x);
        // White space advances but has no ink (a fallback .notdef box for
        // U+3000 must not count in actualBoundingBox*).
        glyphs.push((if it.ch.is_whitespace() { GlyphQuelle::NOTDEF } else { q }, x, it.scale));
        let before = x;
        x += stapel.vorschub(q, size as f32) as f64 * it.scale + ls;
        if it.ch == ' ' {
            x += ws;
        }
        if it.li < logical_advances.len() {
            logical_advances[it.li] = x - before;
        }
        prev = Some(q);
    }
    // Characters removed by the bidi algorithm sit at the end.
    for p in &mut logical_pens {
        if p.is_nan() {
            *p = x;
        }
    }
    let v = vertical_metrics(&stapel, size);
    Some(Shaped { stapel, size, glyphs, logical_pens, logical_advances, width: x, v })
}

/// The named baseline's height above the alphabetic baseline.
fn baseline_offset(baseline: &str, v: &VMetrics) -> f64 {
    match baseline {
        "top" => v.em_ascent,
        "hanging" => v.hanging,
        "middle" => (v.em_ascent - v.em_descent) / 2.0,
        "ideographic" => v.ideographic,
        "bottom" => -v.em_descent,
        _ => 0.0,
    }
}

/// `direction` resolved against the canvas element (`inherit`).
fn is_rtl(w: &mut crate::dom::Stand, id: u32) -> bool {
    let Ok(s) = crate::canvas::surface(w, id) else { return false };
    match s.ctx.state.text.direction.as_str() {
        "rtl" => true,
        "ltr" => false,
        _ => match s.style_element() {
            Some(el) => element_rtl(w, el),
            None => false,
        },
    }
}

/// The language a canvas inherits (`lang: inherit`): the nearest `lang`
/// attribute of the canvas element or its ancestors.
fn element_lang(w: &mut crate::dom::Stand, id: u32) -> Option<String> {
    let el = crate::canvas::surface(w, id).ok()?.style_element();
    let l = w.lebend.as_ref()?;
    // An OffscreenCanvas made in a document inherits the document's lang.
    let Some(el) = el else {
        let root = l.dok.kinder(l.wurzel).iter().copied().find(|k| l.dok.element_name(*k).is_some_and(|n| n.eq_ignore_ascii_case("html")))?;
        return l.dok.attribut(root, "lang").map(|v| v.trim().to_string());
    };
    let mut k = l.knoten(el)?;
    for _ in 0..256 {
        if let Some(v) = l.dok.attribut(k, "lang") {
            return Some(v.trim().to_string());
        }
        k = l.dok.eltern(k)?;
    }
    None
}

/// The `direction` of an element: its computed style, else the nearest
/// `dir` attribute (HTML: `dir=rtl` maps to `direction: rtl`).
fn element_rtl(w: &mut crate::dom::Stand, el: u32) -> bool {
    // An explicit `direction: rtl` in the cascade wins; the computed style
    // does not map `dir` attributes yet, so those are walked below.
    if crate::dom::computed_value(w, el, "direction").is_some_and(|d| d.trim() == "rtl") {
        return true;
    }
    if let Some(l) = w.lebend.as_ref()
        && let Some(mut k) = l.knoten(el)
    {
        for _ in 0..256 {
            if let Some(d) = l.dok.attribut(k, "dir") {
                match d.trim().to_ascii_lowercase().as_str() {
                    "rtl" => return true,
                    "ltr" => return false,
                    _ => {}
                }
            }
            match l.dok.eltern(k) {
                Some(p) => k = p,
                None => break,
            }
        }
    }
    false
}

/// Horizontal offset of the text start from the anchor point.
fn align_offset(align: &str, rtl: bool, width: f64) -> f64 {
    match (align, rtl) {
        ("left", _) | ("start", false) | ("end", true) => 0.0,
        ("right", _) | ("end", false) | ("start", true) => -width,
        _ => -width / 2.0,
    }
}

/// Adds the glyph outlines to `path` (font units mapped by `m`).
fn glyph_path(path: &mut narsil_canvas::Path, font: &narsil_text::TrueTypeFont, gid: u32, m: &narsil_canvas::Matrix) {
    let Ok(outline) = font.glyph_outline(gid) else { return };
    for contour in &outline.contours {
        let pts = &contour.points;
        let n = pts.len();
        if n == 0 {
            continue;
        }
        let p = |i: usize| (pts[i % n].x as f64, pts[i % n].y as f64);
        let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        // Start at an on-curve point (or between two off-curve points).
        let (start, first) = match (0..n).find(|&i| pts[i].on_curve) {
            Some(i) => (p(i), i + 1),
            None => (mid(p(0), p(n - 1)), 0),
        };
        path.move_to(m, start.0, start.1);
        let mut ctrl: Option<(f64, f64)> = None;
        for k in 0..n {
            let i = (first + k) % n;
            let q = p(i);
            if pts[i].on_curve {
                match ctrl.take() {
                    Some(c) => path.quad_to(m, c.0, c.1, q.0, q.1),
                    None => path.line_to(m, q.0, q.1),
                }
            } else {
                if let Some(c) = ctrl {
                    let mm = mid(c, q);
                    path.quad_to(m, c.0, c.1, mm.0, mm.1);
                }
                ctrl = Some(q);
            }
        }
        match ctrl {
            Some(c) => path.quad_to(m, c.0, c.1, start.0, start.1),
            None => path.line_to(m, start.0, start.1),
        }
        path.close();
    }
}

/// `fillText`/`strokeText` (id, text, x, y, maxWidth).
pub(crate) fn draw_text(w: &mut crate::dom::Stand, id: u32, a: &[narsil_jsvm::Wert], stroke: bool) -> Result<narsil_jsvm::Wert, String> {
    use narsil_jsvm::Wert;
    let num = |i: usize| match a.get(i) {
        Some(Wert::Zahl(n)) => *n,
        _ => f64::NAN,
    };
    let text = a.get(1).map(|v| v.als_text()).unwrap_or_default();
    let (x, y) = (num(2), num(3));
    let max_width = match a.get(4) {
        Some(Wert::Zahl(n)) => Some(*n),
        _ => None,
    };
    crate::canvas::surface(w, id)?;
    if !(x.is_finite() && y.is_finite()) || max_width.is_some_and(|m| !(m > 0.0) || m.is_nan()) {
        return Ok(Wert::Undefiniert);
    }
    let rtl = is_rtl(w, id);
    let s = crate::canvas::surface(w, id)?;
    let lang = s.ctx.state.text.lang.clone();
    let lang = if lang.is_empty() || lang == "inherit" { element_lang(w, id) } else { Some(lang) };
    let s = crate::canvas::surface(w, id)?;
    let Some(shaped) = shape(&s.ctx.state, &text, rtl, lang.as_deref()) else { return Ok(Wert::Undefiniert) };
    if shaped.size <= 0.0 || shaped.glyphs.is_empty() {
        return Ok(Wert::Undefiniert);
    }
    let fit = match max_width {
        Some(m) if m < shaped.width => m / shaped.width,
        _ => 1.0,
    };
    let state = &s.ctx.state;
    let x0 = x + align_offset(&state.text.text_align, rtl, shaped.width * fit);
    let y_alpha = y + baseline_offset(&state.text.text_baseline, &shaped.v);
    let ctm = state.transform;
    let mut path = narsil_canvas::Path::new();
    for (q, pen, k) in &shaped.glyphs {
        if *q == GlyphQuelle::NOTDEF {
            continue;
        }
        let Some(font) = shaped.stapel.schrift(q.schrift).map(|f| &f.font) else { continue };
        let scale = shaped.size * k / font.units_per_em().max(1) as f64;
        let g = ctm.multiply(&narsil_canvas::Matrix::new(scale * fit, 0.0, 0.0, -scale, x0 + pen * fit, y_alpha));
        glyph_path(&mut path, font, q.glyph, &g);
    }
    if stroke {
        s.ctx.stroke(Some(&path));
    } else {
        s.ctx.fill(Some(&path), narsil_canvas::FillRule::NonZero);
    }
    Ok(Wert::Undefiniert)
}

/// `measureText`: the `TextMetrics` values in interface order.
pub(crate) fn measure_text(w: &mut crate::dom::Stand, id: u32, text: &str) -> Result<narsil_jsvm::Wert, String> {
    use narsil_jsvm::Wert;
    crate::canvas::surface(w, id)?;
    let rtl = is_rtl(w, id);
    let s = crate::canvas::surface(w, id)?;
    let state = &s.ctx.state;
    let lang = state.text.lang.clone();
    let lang = if lang.is_empty() || lang == "inherit" { element_lang(w, id) } else { Some(lang) };
    let s = crate::canvas::surface(w, id)?;
    let state = &s.ctx.state;
    let Some(shaped) = shape(state, text, rtl, lang.as_deref()) else {
        return Ok(Wert::Feld(vec![Wert::Zahl(0.0); 12]));
    };
    let base = baseline_offset(&state.text.text_baseline, &shaped.v);
    let start = align_offset(&state.text.text_align, rtl, shaped.width);
    // Ink bounds in run coordinates (y up).
    let (mut left, mut right, mut top, mut bottom) = (f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::INFINITY);
    for (q, pen, k) in &shaped.glyphs {
        if *q == GlyphQuelle::NOTDEF {
            continue;
        }
        let Some(font) = shaped.stapel.schrift(q.schrift).map(|f| &f.font) else { continue };
        let Ok(outline) = font.glyph_outline(q.glyph) else { continue };
        if outline.contours.is_empty() {
            continue;
        }
        let scale = shaped.size * k / font.units_per_em().max(1) as f64;
        let b = outline.bounds;
        left = left.min(pen + b.x_min as f64 * scale);
        right = right.max(pen + b.x_max as f64 * scale);
        top = top.max(b.y_max as f64 * scale);
        bottom = bottom.min(b.y_min as f64 * scale);
    }
    let v = &shaped.v;
    let (abl, abr, aba, abd) = if left.is_finite() { (-(start + left), start + right, top - base, base - bottom) } else { (0.0, 0.0, 0.0, 0.0) };
    let vals = [
        shaped.width,
        abl,
        abr,
        v.ascent - base,
        v.descent + base,
        aba,
        abd,
        v.em_ascent - base,
        v.em_descent + base,
        v.hanging - base,
        -base,
        v.ideographic - base,
    ];
    let mut out: Vec<Wert> = vals.iter().map(|x| Wert::Zahl(*x)).collect();
    // For text clusters: the text start relative to the anchor, the
    // baseline heights (em top, em bottom, hanging, ideographic) and the pen
    // position of every code point.
    out.push(Wert::Zahl(start));
    out.push(Wert::Feld(vec![Wert::Zahl(v.em_ascent), Wert::Zahl(v.em_descent), Wert::Zahl(v.hanging), Wert::Zahl(v.ideographic)]));
    out.push(Wert::Feld(shaped.logical_pens.iter().map(|pen| Wert::Zahl(*pen)).collect()));
    out.push(Wert::Feld(shaped.logical_advances.iter().map(|a| Wert::Zahl(*a)).collect()));
    Ok(Wert::Feld(out))
}

// --- filters ---------------------------------------------------------------

fn filter_length(t: &str) -> Option<f64> {
    let t = t.trim().to_ascii_lowercase();
    if t == "0" {
        return Some(0.0);
    }
    let end = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(t.len());
    let (n, unit) = t.split_at(end);
    let v: f64 = n.parse().ok()?;
    let k = match unit {
        "px" => 1.0,
        "em" | "rem" => 16.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        "ex" | "ch" => 8.0,
        _ => return None,
    };
    v.is_finite().then_some(v * k)
}

fn filter_amount(t: &str) -> Option<f32> {
    let t = t.trim();
    if t.is_empty() {
        return Some(1.0);
    }
    let v = if let Some(p) = t.strip_suffix('%') { p.trim().parse::<f32>().ok()? / 100.0 } else { t.parse::<f32>().ok()? };
    (v.is_finite() && v >= 0.0).then_some(v)
}

fn filter_angle(t: &str) -> Option<f64> {
    let t = t.trim().to_ascii_lowercase();
    if t.is_empty() || t == "0" {
        return Some(0.0);
    }
    for (u, k) in [("deg", 1.0), ("grad", 0.9), ("rad", 180.0 / std::f64::consts::PI), ("turn", 360.0)] {
        if let Some(n) = t.strip_suffix(u) {
            if u == "rad" && t.ends_with("grad") {
                continue;
            }
            let v: f64 = n.trim().parse().ok()?;
            return v.is_finite().then_some(v * k);
        }
    }
    None
}

/// Splits on top-level whitespace (keeps `rgb(1 2 3)` together).
fn top_level_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Parses a canvas `filter` value: `None` = invalid (keep the old one),
/// `Some(None)` = `none`, `Some(Some(f))` = the filter list.
#[must_use]
pub fn parse_filter(value: &str, current_color: narsil_canvas::Color) -> Option<Option<narsil_canvas::filter::Filter>> {
    use narsil_canvas::filter::{FilterOp, grayscale, hue_rotate, saturate, sepia};
    let v = value.trim();
    if v.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    if v.is_empty() {
        return None;
    }
    let mut ops = Vec::new();
    let mut rest = v;
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = rest[..open].trim().to_ascii_lowercase();
        let mut depth = 0;
        let mut close = None;
        for (i, c) in rest[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close?;
        let arg = rest[open + 1..close].trim();
        rest = rest[close + 1..].trim_start();
        match name.as_str() {
            "blur" => {
                let r = if arg.is_empty() { 0.0 } else { filter_length(arg)? };
                if r < 0.0 {
                    return None;
                }
                ops.push(FilterOp::Blur(r, r));
            }
            "brightness" => {
                let a = filter_amount(arg)?;
                ops.push(FilterOp::Linear { slope: a, intercept: 0.0 });
            }
            "contrast" => {
                let a = filter_amount(arg)?;
                ops.push(FilterOp::Linear { slope: a, intercept: 0.5 - 0.5 * a });
            }
            "grayscale" => ops.push(FilterOp::ColorMatrix(grayscale(filter_amount(arg)?.min(1.0)))),
            "sepia" => ops.push(FilterOp::ColorMatrix(sepia(filter_amount(arg)?.min(1.0)))),
            "saturate" => ops.push(FilterOp::ColorMatrix(saturate(filter_amount(arg)?))),
            "invert" => {
                let a = filter_amount(arg)?.min(1.0);
                ops.push(FilterOp::Linear { slope: 1.0 - 2.0 * a, intercept: a });
            }
            "opacity" => ops.push(FilterOp::Opacity(filter_amount(arg)?.min(1.0))),
            "hue-rotate" => ops.push(FilterOp::ColorMatrix(hue_rotate(filter_angle(arg)?))),
            "drop-shadow" => {
                let mut lengths = Vec::new();
                let mut color = None;
                for word in top_level_words(arg) {
                    if let Some(l) = filter_length(&word) {
                        lengths.push(l);
                    } else if color.is_none() {
                        color = Some(crate::canvas::parse_css_color_with(&word, current_color)?);
                    } else {
                        return None;
                    }
                }
                if !(2..=3).contains(&lengths.len()) || lengths.get(2).is_some_and(|b| *b < 0.0) {
                    return None;
                }
                let c = color.unwrap_or(current_color);
                let sigma = lengths.get(2).copied().unwrap_or(0.0) / 2.0;
                ops.push(FilterOp::DropShadow { dx: lengths[0], dy: lengths[1], sigma, sigma_y: sigma, color: c.premultiplied() });
            }
            "url" => {}
            _ => return None,
        }
    }
    Some(Some(narsil_canvas::filter::Filter { ops }))
}
