//! SVG documents rendered on `narsil_canvas`: XML parsing, the CSS cascade
//! for SVG properties, and painting with paint servers, markers, text,
//! clipping, masking and filters.

pub mod clip;
pub mod color;
pub mod css;
pub mod filter;
pub mod image;
pub mod marker;
pub mod render;
pub mod shapes;
pub mod style;
pub mod text;
pub mod transform;
pub mod units;
pub mod xml;

use narsil_canvas::{Context2D, Image, Matrix};

/// The largest edge a rendered SVG may have, in pixels.
pub const MAX_EDGE: u32 = 16_384;
/// The most pixels a rendered SVG may have.
pub const MAX_PIXELS: u64 = 40_000_000;
/// The default object size (CSS Images 3 §4.2: 300 x 150).
const DEFAULT_SIZE: (f64, f64) = (300.0, 150.0);

/// The intrinsic dimensions of an SVG document's root element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Intrinsic {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub ratio: Option<f64>,
}

/// A rendered document: premultiplied pixels and the CSS size they cover.
pub struct Rendered {
    pub image: Image,
    pub css_width: f64,
    pub css_height: f64,
}

fn absolute_length(v: Option<&str>) -> Option<f64> {
    let v = v?.trim();
    if v.ends_with('%') {
        return None;
    }
    let c = units::Ctx { vw: 0.0, vh: 0.0, font_size: 16.0 };
    units::length(v, units::Axis::X, &c).filter(|l| *l >= 0.0)
}

/// The intrinsic size of a parsed root element.
#[must_use]
pub fn intrinsic_of(root: &xml::Element) -> Intrinsic {
    let width = absolute_length(root.attr("width"));
    let height = absolute_length(root.attr("height"));
    let vb = root.attr("viewBox").and_then(transform::view_box);
    let ratio = match (width, height) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some(w / h),
        _ => vb.map(|v| v[2] / v[3]),
    };
    Intrinsic { width, height, ratio }
}

/// The intrinsic size of an SVG source.
#[must_use]
pub fn intrinsic_size(source: &str) -> Option<Intrinsic> {
    let root = xml::parse(source)?;
    (root.name == "svg").then(|| intrinsic_of(&root))
}

/// The CSS size an SVG image is drawn at when nothing sizes it.
#[must_use]
pub fn natural_size(i: Intrinsic) -> (f64, f64) {
    match (i.width, i.height, i.ratio) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(r)) => (w, w / r),
        (None, Some(h), Some(r)) => (h * r, h),
        (Some(w), None, None) => (w, DEFAULT_SIZE.1),
        (None, Some(h), None) => (DEFAULT_SIZE.0, h),
        (None, None, Some(r)) => {
            // Contain the ratio in the default object size.
            if r >= DEFAULT_SIZE.0 / DEFAULT_SIZE.1 { (DEFAULT_SIZE.0, DEFAULT_SIZE.0 / r) } else { (DEFAULT_SIZE.1 * r, DEFAULT_SIZE.1) }
        }
        (None, None, None) => DEFAULT_SIZE,
    }
}

/// Renders an SVG document. `size` is the pixel size to render at (the
/// natural size scaled to it); `None` renders at the natural size.
///
/// # Errors
/// When the source is not an SVG document or the size is unusable.
pub fn render(source: &str, size: Option<(u32, u32)>) -> Result<Rendered, String> {
    let root = xml::parse(source).ok_or("not well-formed XML")?;
    if root.name != "svg" {
        return Err(format!("root element is <{}>, not <svg>", root.name));
    }
    let (css_w, css_h) = natural_size(intrinsic_of(&root));
    render_root(&root, css_w, css_h, size, "")
}

/// Renders an SVG image into a box of `pixels` that is `viewport` CSS
/// pixels large (an `<img>`, a background, an inline `<svg>` box): the box
/// is the root's viewport. A root without `viewBox` but with a width and a
/// height is stretched to the box, as if `viewBox="0 0 width height"
/// preserveAspectRatio="none"` were given.
///
/// # Errors
/// When the source is not an SVG document or the size is unusable.
pub fn render_viewport(source: &str, viewport: (f64, f64), pixels: (u32, u32)) -> Result<Rendered, String> {
    let root = xml::parse(source).ok_or("not well-formed XML")?;
    if root.name != "svg" {
        return Err(format!("root element is <{}>, not <svg>", root.name));
    }
    let i = intrinsic_of(&root);
    let has_view_box = root.attr("viewBox").and_then(transform::view_box).is_some();
    let (vw, vh) = match (i.width, i.height) {
        (Some(w), Some(h)) if !has_view_box && w > 0.0 && h > 0.0 => (w, h),
        _ => viewport,
    };
    render_root(&root, vw, vh, Some(pixels), "")
}

/// Renders a parsed root at a CSS viewport size with extra style sheet text.
///
/// # Errors
/// When the size is zero or too large.
pub fn render_root(root: &xml::Element, css_w: f64, css_h: f64, size: Option<(u32, u32)>, extra_css: &str) -> Result<Rendered, String> {
    let (pw, ph) = match size {
        Some((w, h)) => (w, h),
        None => (css_w.round().max(0.0) as u32, css_h.round().max(0.0) as u32),
    };
    if pw == 0 || ph == 0 || !(css_w > 0.0) || !(css_h > 0.0) {
        return Err("zero size".into());
    }
    if pw > MAX_EDGE || ph > MAX_EDGE || u64::from(pw) * u64::from(ph) > MAX_PIXELS {
        return Err(format!("{pw}x{ph} is too large"));
    }
    let doc = style::Doc::new(root, extra_css);
    let mut ctx = Context2D::new(pw, ph, false).ok_or("no surface")?;
    let mut r = render::Renderer::new(&doc, pw, ph);
    let m = Matrix::scale(f64::from(pw) / css_w, f64::from(ph) / css_h);
    r.draw(&mut ctx, 0, m, render::View { w: css_w, h: css_h });
    Ok(Rendered { image: ctx.surface.snapshot(), css_width: css_w, css_height: css_h })
}

/// The coverage (one alpha byte per pixel, `size` pixels) of the
/// `<clipPath>` `reference` (`url(#id)`) in `source` for a CSS box of
/// `box_size` CSS pixels: the box's user space has its origin at the box,
/// and `m` maps it to the pixels. `None` when the reference is not a clip
/// path (the box is then not clipped, CSS Masking 1 §5.1).
#[must_use]
pub fn clip_path_coverage(source: &str, reference: &str, box_size: (f64, f64), m: Matrix, size: (u32, u32)) -> Option<Vec<u8>> {
    let root = xml::parse(source)?;
    let doc = style::Doc::new(&root, "");
    let cp = doc.lookup(reference).filter(|c| doc.items[*c].el.name == "clipPath")?;
    let mut r = render::Renderer::new(&doc, size.0, size.1);
    let layer = clip::clip_path_layer(&mut r, cp, m, render::View { w: box_size.0, h: box_size.1 }, Some((0.0, 0.0, box_size.0, box_size.1)))?;
    Some(layer.surface.data.chunks_exact(4).map(|p| p[3]).collect())
}

/// The coverage of the `<mask>` `reference` in `source` for a CSS box (as
/// [`clip_path_coverage`]): its luminance, or its alpha under
/// `mask-type: alpha`.
#[must_use]
pub fn mask_coverage(source: &str, reference: &str, box_size: (f64, f64), m: Matrix, size: (u32, u32)) -> Option<Vec<u8>> {
    let root = xml::parse(source)?;
    let doc = style::Doc::new(&root, "");
    let mut r = render::Renderer::new(&doc, size.0, size.1);
    let (layer, luminance) = clip::mask(&mut r, reference, 0, m, render::View { w: box_size.0, h: box_size.1 }, Some((0.0, 0.0, box_size.0, box_size.1)))?;
    Some(
        layer
            .surface
            .data
            .chunks_exact(4)
            .map(|p| if luminance { (0.2125 * f32::from(p[0]) + 0.7154 * f32::from(p[1]) + 0.0721 * f32::from(p[2]) + 0.5) as u8 } else { p[3] })
            .collect(),
    )
}

/// What a `filter` on a CSS box did to its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxFilter {
    /// The pixels were filtered.
    Applied,
    /// The box is not rendered (an empty filter region).
    Hide,
    /// The value does not filter (invalid, or a missing reference).
    Ignore,
}

/// Applies a `filter` value to the isolated pixels of a CSS box:
/// premultiplied RGBA `data` of `size`, in which the box's border box is
/// `border` (x0, y0, x1, y1). The box's user space starts at its border
/// box; `filters` is an SVG document with the `<filter>` elements `url()`
/// references name; `color` is `currentColor`.
#[must_use]
pub fn filter_box(value: &str, filters: Option<&str>, data: &mut [u8], size: (u32, u32), border: (f64, f64, f64, f64), color: [u8; 3]) -> BoxFilter {
    let source = filters.unwrap_or("<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
    let Some(mut root) = xml::parse(source) else { return BoxFilter::Ignore };
    // The referencing element's `color` for `currentColor`.
    root.attrs.retain(|(n, _)| n != "color");
    root.attrs.push(("color".into(), format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])));
    let doc = style::Doc::new(&root, "");
    let Some(mut layer) = Context2D::new(size.0, size.1, false) else { return BoxFilter::Ignore };
    if layer.surface.data.len() != data.len() {
        return BoxFilter::Ignore;
    }
    std::sync::Arc::make_mut(&mut layer.surface.data).copy_from_slice(data);
    let (bw, bh) = (border.2 - border.0, border.3 - border.1);
    let mut r = render::Renderer::new(&doc, size.0, size.1);
    let m = Matrix::translate(border.0, border.1);
    let view = render::View { w: bw, h: bh };
    let out = match filter::apply(&mut r, &mut layer, value, 0, m, view, Some((0.0, 0.0, bw, bh))) {
        filter::Outcome::Applied => BoxFilter::Applied,
        filter::Outcome::Hide => return BoxFilter::Hide,
        filter::Outcome::Ignore => return BoxFilter::Ignore,
    };
    data.copy_from_slice(&layer.surface.data);
    out
}

/// How far (CSS pixels) a `filter` value can move content beyond its box:
/// the room an isolated filter layer needs around the box. `url()`
/// filters may reach to their filter region (10 % around the box by
/// default) and are given a fifth of the box size more.
#[must_use]
pub fn filter_reach(value: &str, box_size: (f64, f64)) -> f64 {
    let lower = value.to_ascii_lowercase();
    let mut reach: f64 = 0.0;
    if lower.contains("url(") {
        reach = reach.max(box_size.0.max(box_size.1) * 0.2 + 32.0);
    }
    for part in lower.split(')') {
        let Some((name, args)) = part.split_once('(') else { continue };
        let name = name.trim().rsplit(char::is_whitespace).next().unwrap_or("");
        let lens: Vec<f64> = args.split_whitespace().filter_map(|w| units::split_number(w).map(|v| v.0.abs())).collect();
        match name {
            "blur" => reach += 3.0 * lens.first().copied().unwrap_or(0.0),
            "drop-shadow" => {
                let off = lens.first().copied().unwrap_or(0.0).max(lens.get(1).copied().unwrap_or(0.0));
                reach += off + 1.5 * lens.get(2).copied().unwrap_or(0.0);
            }
            _ => {}
        }
    }
    reach.ceil() + 2.0
}

/// Straight (not premultiplied) RGBA bytes of a rendered image.
#[must_use]
pub fn straight_rgba(img: &Image) -> Vec<u8> {
    let mut out = img.data.as_ref().clone();
    for px in out.chunks_exact_mut(4) {
        let a = u32::from(px[3]);
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(r: &Rendered, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * r.image.width + x) * 4) as usize;
        let d = &r.image.data;
        [d[i], d[i + 1], d[i + 2], d[i + 3]]
    }

    #[test]
    fn renders_shapes_with_viewbox() {
        let r = render(r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10" viewBox="0 0 2 1"><rect width="1" height="1" fill="lime"/><circle cx="1.5" cy="0.5" r="0.5" fill="blue"/></svg>"#, None).unwrap();
        assert_eq!((r.image.width, r.image.height), (20, 10));
        assert_eq!(pixel(&r, 5, 5), [0, 255, 0, 255]);
        assert_eq!(pixel(&r, 15, 5), [0, 0, 255, 255]);
        assert_eq!(pixel(&r, 19, 0)[3], 0);
    }

    #[test]
    fn gradients_use_groups_and_opacity() {
        let src = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="20">
          <defs><linearGradient id="g"><stop offset="0" stop-color="black"/><stop offset="1" stop-color="white"/></linearGradient>
          <rect id="r" width="10" height="10" fill="red"/></defs>
          <rect width="100" height="10" fill="url(#g)"/>
          <g opacity="0.5"><use xlink:href="#r" y="10" style="fill:blue"/></g></svg>"##;
        let r = render(src, None).unwrap();
        let mid = pixel(&r, 50, 5);
        assert!((120..=135).contains(&mid[0]), "{mid:?}");
        assert!(pixel(&r, 2, 5)[0] < 10);
        // The used rect keeps its own fill; the group halves its alpha.
        let used = pixel(&r, 5, 15);
        assert_eq!(used[3], 128);
        assert_eq!(used[0], 128);
    }

    #[test]
    fn clip_paths_masks_and_markers() {
        let src = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
          <clipPath id="c"><rect width="5" height="20"/></clipPath>
          <mask id="m"><rect x="20" width="20" height="20" fill="white"/></mask>
          <marker id="k" markerWidth="4" markerHeight="4" refX="2" refY="2" markerUnits="userSpaceOnUse"><rect width="4" height="4" fill="lime"/></marker>
          <rect width="20" height="20" fill="blue" clip-path="url(#c)"/>
          <rect x="20" width="20" height="20" fill="blue" mask="url(#m)"/>
          <path d="M10 10 L30 10" stroke="none" marker-end="url(#k)"/></svg>"##;
        let r = render(src, None).unwrap();
        assert_eq!(pixel(&r, 2, 2), [0, 0, 255, 255]);
        assert_eq!(pixel(&r, 8, 2)[3], 0);
        assert_eq!(pixel(&r, 25, 2), [0, 0, 255, 255]);
        assert_eq!(pixel(&r, 30, 10), [0, 255, 0, 255]);
    }

    #[test]
    fn use_instances_inherit_from_use_and_set_the_context_paint() {
        // paint-order and fill inherit through the instance tree; markers
        // clip to their viewport by default; a zero viewBox disables.
        let src = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="60" height="20">
          <defs><rect id="r" width="10" height="10" fill="context-fill"/><rect id="p" width="10" height="10"/></defs>
          <use xlink:href="#r" fill="lime"/>
          <use xlink:href="#p" x="20" fill="blue"/>
          <svg x="40" width="10" height="10" viewBox="0 0 0 10"><rect width="10" height="10" fill="red"/></svg>
          <svg x="50" width="5" height="5"><rect width="10" height="10" fill="lime"/></svg></svg>"##;
        let r = render(src, None).unwrap();
        assert_eq!(pixel(&r, 5, 5), [0, 255, 0, 255]);
        assert_eq!(pixel(&r, 25, 5), [0, 0, 255, 255]);
        assert_eq!(pixel(&r, 45, 5)[3], 0);
        assert_eq!(pixel(&r, 52, 2), [0, 255, 0, 255]);
        assert_eq!(pixel(&r, 57, 7)[3], 0, "nested svg clips to its viewport");
    }

    #[test]
    fn path_length_scales_dashes() {
        let src = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="10">
          <path d="M0 5 H100" stroke="black" stroke-width="4" stroke-dasharray="1 1" pathLength="2"/></svg>"#;
        let r = render(src, None).unwrap();
        // Dashes of 50 user units: on to 50, off after.
        assert_eq!(pixel(&r, 25, 5)[3], 255);
        assert_eq!(pixel(&r, 75, 5)[3], 0);
    }

    #[test]
    fn text_path_places_glyphs_along_the_path() {
        let src = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200">
          <path id="v" d="M100 10 V190"/>
          <text font-size="20"><textPath href="#v">IIIIIIII</textPath></text></svg>"##;
        let r = render(src, None).unwrap();
        // The vertical path turns the glyphs: ink near x = 100 below y = 10.
        let ink = |x0: u32, x1: u32, y0: u32, y1: u32| (y0..y1).any(|y| (x0..x1).any(|x| pixel(&r, x, y)[3] > 0));
        assert!(ink(90, 111, 20, 100));
        assert!(!ink(0, 80, 0, 200));
    }

    #[test]
    fn filters_flood_and_offset() {
        let src = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40">
          <filter id="f" x="0" y="0" width="1" height="1"><feFlood flood-color="lime"/></filter>
          <filter id="o" filterUnits="userSpaceOnUse" x="0" y="0" width="40" height="40"><feOffset dx="10" dy="0"/></filter>
          <rect width="10" height="10" fill="red" filter="url(#f)"/>
          <rect y="20" width="10" height="10" fill="blue" filter="url(#o)"/></svg>"##;
        let r = render(src, None).unwrap();
        assert_eq!(pixel(&r, 5, 5), [0, 255, 0, 255]);
        assert_eq!(pixel(&r, 5, 25)[3], 0);
        assert_eq!(pixel(&r, 15, 25), [0, 0, 255, 255]);
    }

    #[test]
    fn natural_size_falls_back_to_the_default_object_size() {
        assert_eq!(natural_size(Intrinsic { width: None, height: None, ratio: None }), (300.0, 150.0));
        assert_eq!(natural_size(Intrinsic { width: Some(50.0), height: None, ratio: Some(2.0) }), (50.0, 25.0));
    }
}

#[cfg(test)]
mod box_filter_tests {
    use super::*;

    #[test]
    fn data_document_filters_blur_a_box() {
        let doc = "<svg xmlns='http://www.w3.org/2000/svg'><filter id='blur'><feGaussianBlur stdDeviation='4' /></filter></svg>";
        let value = format!("url(\"data:image/svg+xml;utf8,{doc}#blur\")");
        let (w, h) = (40u32, 40u32);
        let mut data = vec![0u8; (w * h * 4) as usize];
        for y in 10..30 {
            for x in 10..30 {
                let o = ((y * w + x) * 4) as usize;
                data[o..o + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
        let r = filter_box(&value, Some(doc), &mut data, (w, h), (10.0, 10.0, 30.0, 30.0), [0, 0, 0]);
        assert_eq!(r, BoxFilter::Applied);
        // The edge is soft now.
        assert!(data[((20 * w + 10) * 4 + 3) as usize] < 250);
    }
}
