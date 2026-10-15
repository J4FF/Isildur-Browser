//! CSS-Hintergrundbilder: Groesse, Position, Wiederholung und Zuschnitt.
use crate::bilder::GeladenesBild;
use narsil_core::geometry::Rect;
use narsil_display::DisplayItem;
use std::sync::Arc;

/// Release 13.10.2026 (Agent CSS2): ein Rechteck um Kanten (oben, rechts,
/// unten, links) verkleinert.
fn innen(r: Rect, k: [f32; 4]) -> Rect {
    let [o, re, u, l] = k;
    Rect::new(r.x + l, r.y + o, (r.width - l - re).max(0.0), (r.height - o - u).max(0.0))
}

/// Release 13.10.2026 (Agent CSS2): eine Achse der Kachelung — Anfang,
/// Kachelgroesse, Periode (Kachel plus Abstand) und ob wiederholt wird.
#[derive(Clone, Copy)]
struct Achse {
    anfang: f32,
    groesse: f32,
    periode: f32,
    wiederholt: bool,
}

impl Achse {
    /// Die Stelle in der Kachel (0..groesse) fuer die absolute Koordinate
    /// `p`, oder `None` (ausserhalb, im Abstand von `space`).
    fn stelle(&self, p: f32) -> Option<f32> {
        let rel = p - self.anfang;
        let s = if self.wiederholt { rel.rem_euclid(self.periode) } else { rel };
        (s >= 0.0 && s < self.groesse).then_some(s)
    }
}

/// Release 13.10.2026 (Agent CSS2): `space` (1) und `round` (2) einer Achse
/// (CSS Backgrounds 3 §3.4): bei `round` wird die Kachel so gestaucht, dass
/// eine ganze Zahl in die Lageflaeche passt; bei `space` stehen so viele
/// ganze Kacheln wie passen, gleichmaessig verteilt (passt keine zweite, gilt
/// die Position).
fn achse(art: u8, wiederholt: bool, lage_anfang: f32, lage_laenge: f32, groesse: f32, position: f32) -> Achse {
    if art == 1 && lage_laenge > 0.0 {
        let n = (lage_laenge / groesse).floor();
        if n >= 2.0 {
            let abstand = (lage_laenge - n * groesse) / (n - 1.0);
            return Achse { anfang: lage_anfang, groesse, periode: groesse + abstand, wiederholt: true };
        }
        return Achse { anfang: position, groesse, periode: groesse, wiederholt: false };
    }
    Achse { anfang: position, groesse, periode: groesse, wiederholt }
}

pub(crate) fn item(
    rect: Rect,
    b: &GeladenesBild,
    geo: narsil_css::BildGeo,
    budget: &std::cell::Cell<usize>,
) -> Option<DisplayItem> {
    if !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width <= 0.0
        || rect.height <= 0.0
        || b.breite == 0
        || b.hoehe == 0
    {
        return None;
    }
    if let Some(bi) = geo.border_image.as_ref() {
        return border_image_item(rect, b, &geo, bi, budget);
    }
    // Release 14.10.2026 (Agent CSS2): sizing uses the natural size; the
    // buffer may be smaller (image diet), sampling below uses the buffer.
    let (bw, bh) = (b.natur_breite.max(1) as f32, b.natur_hoehe.max(1) as f32);
    // Release 13.10.2026 (Agent CSS2): die Lageflaeche nach `background-origin`
    // (Anfangswert: Innenkante des Rahmens, CSS Backgrounds 3 §3.7), die
    // Malflaeche nach `background-clip` (Anfangswert: Aussenkante, §3.5).
    // Siehe `BildGeo::rand_px`, `innen_px`.
    let saeubern = |k: [f32; 4]| k.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    let rand = saeubern(geo.rand_px);
    let fuell = saeubern(geo.innen_px);
    let rand_und_fuell = [rand[0] + fuell[0], rand[1] + fuell[1], rand[2] + fuell[2], rand[3] + fuell[3]];
    // Release 14.10.2026 (Agent CSS2): see `BildGeo::from_body` — the root
    // box: viewport wide, down to the body's bottom plus its top margin.
    let root_area = match (geo.from_body, geo.viewport) {
        (true, Some((vw, _))) => Rect::new(0.0, 0.0, vw, rect.y + rect.height + rect.y.max(0.0)),
        _ => rect,
    };
    let rand = if geo.from_body { [0.0; 4] } else { rand };
    let rand_und_fuell = if geo.from_body { [0.0; 4] } else { rand_und_fuell };
    let lage = match geo.ursprung {
        3 => geo.viewport.map_or(rect, |(w, h)| Rect::new(0.0, 0.0, w, h)),
        _ if geo.from_body => root_area,
        1 => rect,
        2 => innen(rect, rand_und_fuell),
        _ => innen(rect, rand),
    };
    let mal = match geo.bild_clip {
        1 => innen(rect, rand),
        2 => innen(rect, rand_und_fuell),
        _ => rect,
    };
    // Release 14.10.2026 (Agent CSS2): see `BildGeo::paints_canvas` — the canvas
    // reaches at least over the viewport and the root box.
    let mal = match (geo.paints_canvas, geo.viewport) {
        (true, Some((vw, vh))) => {
            let rechts = vw.max(rect.x + rect.width);
            let unten = vh.max(rect.y + rect.height);
            Rect::new(0.0, 0.0, rechts, unten)
        }
        _ => mal,
    };
    if mal.width <= 0.0 || mal.height <= 0.0 {
        return None;
    }
    let w = mal.width.ceil() as u32;
    let h = mal.height.ceil() as u32;
    let anzahl = (w as usize).checked_mul(h as usize)?;
    if anzahl > 4_000_000 || anzahl > budget.get() {
        return None;
    }
    budget.set(budget.get() - anzahl);
    let (lw, lh) = (lage.width, lage.height);
    // Release 14.10.2026 (Agent CSS2): a vector image may lack an intrinsic
    // width, height or ratio; the concrete size then follows the default
    // sizing algorithm with the positioning area as default object size (CSS
    // Images 3 §5.3, CSS Backgrounds 3 §3.9). MEASURED before (WPT
    // css-backgrounds `background-size/vector/*`, 182 of 206 red): every SVG
    // was sized like a bitmap of its raster size.
    let vector_size = b.vector.map(|v| concrete_vector_size(&geo, v.width(), v.height(), v.ratio(), lw, lh));
    let (mut sw, mut sh) = if let Some(size) = vector_size {
        size
    } else if geo.cover {
        let f = (lw / bw).max(lh / bh);
        (bw * f, bh * f)
    } else if geo.contain {
        let f = (lw / bw).min(lh / bh);
        (bw * f, bh * f)
    } else if geo.groesse_xy == [None, None] {
        let f = if geo.groesse > 0.0 { geo.groesse / bw } else { 1.0 };
        (bw * f, bh * f)
    } else {
        // Release 13.10.2026 (Agent CSS2): beide Achsen, `%` gegen die
        // Lageflaeche, `auto` nach dem Seitenverhaeltnis (§3.9).
        let mass = |l: Option<narsil_css::Laenge>, flaeche: f32| {
            l.map(|l| if l.einheit == narsil_css::Einheit::Prozent { l.wert / 100.0 * flaeche } else { l.wert })
        };
        match (mass(geo.groesse_xy[0], lw), mass(geo.groesse_xy[1], lh)) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, bh * w / bw),
            (None, Some(h)) => (bw * h / bh, h),
            (None, None) => (bw, bh),
        }
    };
    if !sw.is_finite() || !sh.is_finite() || sw <= 0.0 || sh <= 0.0 {
        return None;
    }
    // Release 13.10.2026 (Agent CSS2): `round` — eine ganze Zahl Kacheln; die
    // andere Achse mit `auto` behaelt das Seitenverhaeltnis (§3.4).
    let [art_x, art_y] = geo.wiederhol_art;
    let (alt_w, alt_h) = (sw, sh);
    if art_x == 2 && lw > 0.0 {
        sw = lw / (lw / sw).round().max(1.0);
    }
    if art_y == 2 && lh > 0.0 {
        sh = lh / (lh / sh).round().max(1.0);
    }
    let ohne_angabe = |i: usize| {
        geo.groesse_xy[i].is_none() && !geo.cover && !geo.contain && b.vector.is_none_or(|v| v.ratio().is_some())
    };
    if art_x == 2 && art_y != 2 && ohne_angabe(1) {
        sh = alt_h * sw / alt_w;
    } else if art_y == 2 && art_x != 2 && ohne_angabe(0) {
        sw = alt_w * sh / alt_h;
    }
    // Release 11.10.2026 (Agent CSS2): Pixel je Achse (`px_achsen`).
    let (px_x, px_y) = geo.px_achsen();
    // Release 13.10.2026 (Agent CSS2): `right 10px` zaehlt von der Endkante.
    let pos_x = lage.x
        + match (px_x, geo.vom_ende.0) {
            (true, true) => lw - sw - geo.position_x,
            (true, false) => geo.position_x,
            _ => (lw - sw) * geo.position_x,
        };
    let pos_y = lage.y
        + match (px_y, geo.vom_ende.1) {
            (true, true) => lh - sh - geo.position_y,
            (true, false) => geo.position_y,
            _ => (lh - sh) * geo.position_y,
        };
    let ax = achse(art_x, geo.wiederholen_x, lage.x, lw, sw, pos_x);
    let ay = achse(art_y, geo.wiederholen_y, lage.y, lh, sh, pos_y);
    // Release 14.10.2026 (Agent CSS2): `background-clip: border-area` paints
    // only where the border is: the border box minus the padding box (CSS
    // Backgrounds 4 §2.2; plain ring, no border styles). MEASURED before (WPT
    // css-backgrounds `background-clip/clip-border-area-solid`): the image
    // filled the whole box.
    let hole = (geo.bild_clip == 4).then(|| innen(rect, rand));
    let mut pixels = vec![0u8; anzahl * 4];
    for y in 0..h {
        let Some(sy) = ay.stelle(mal.y + y as f32 + 0.5) else { continue };
        for x in 0..w {
            if let Some(l) = hole {
                let (px, py) = (mal.x + x as f32 + 0.5, mal.y + y as f32 + 0.5);
                if px >= l.x && px < l.x + l.width && py >= l.y && py < l.y + l.height {
                    continue;
                }
            }
            let Some(sx) = ax.stelle(mal.x + x as f32 + 0.5) else { continue };
            let ix = ((sx / sw * b.breite as f32) as u32).min(b.breite - 1);
            let iy = ((sy / sh * b.hoehe as f32) as u32).min(b.hoehe - 1);
            let von = ((iy * b.breite + ix) * 4) as usize;
            let nach = ((y * w + x) * 4) as usize;
            if let Some(p) = b.pixels.get(von..von + 4) {
                pixels[nach..nach + 4].copy_from_slice(p);
            }
        }
    }
    Some(DisplayItem::Bild {
        rect: mal,
        breite: w,
        hoehe: h,
        pixels: Arc::new(pixels),
    })
}

/// Release 14.10.2026 (Agent CSS2): `border-image` (CSS Backgrounds 3 §6).
/// The image is cut into nine parts (`border-image-slice`) and laid into the
/// border image area (border box plus `border-image-outset`): corners are
/// stretched, edges follow `border-image-repeat`, the middle is drawn only
/// with `fill`. MEASURED before (WPT css-backgrounds `border-image-*`, about
/// 50 files): the plain border was drawn instead of the image.
fn border_image_item(
    rect: Rect,
    b: &GeladenesBild,
    geo: &narsil_css::BildGeo,
    bi: &narsil_css::BorderImage,
    budget: &std::cell::Cell<usize>,
) -> Option<DisplayItem> {
    use narsil_css::BorderImageWidth as W;
    let (iw, ih) = (b.breite as f32, b.hoehe as f32);
    if b.breite == 0 || b.hoehe == 0 {
        return None;
    }
    let border = geo.rand_px.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    // Outset: a length or a multiple of the border width.
    let outset: [f32; 4] = std::array::from_fn(|i| {
        let (v, px) = bi.outset[i];
        let v = if px { v } else { v * border[i] };
        if v.is_finite() { v.max(0.0) } else { 0.0 }
    });
    let area = Rect::new(
        rect.x - outset[3],
        rect.y - outset[0],
        rect.width + outset[1] + outset[3],
        rect.height + outset[0] + outset[2],
    );
    if !(area.width > 0.0 && area.height > 0.0) {
        return None;
    }
    // Release 14.10.2026 (Agent CSS2): the concrete size of the image, which
    // slice numbers refer to: a vector image without intrinsic dimensions
    // takes the border image area (CSS Backgrounds 3 §6.2, default sizing);
    // a raster image its natural size. MEASURED before (WPT css-backgrounds
    // `border-image-image-type-001`, a viewBox-only SVG with slice `40 30 20
    // 10` in a 200px area): the numbers were taken as raster pixels.
    let (concrete_w, concrete_h) = match b.vector {
        Some(v) => match (v.width(), v.height(), v.ratio()) {
            (Some(w), Some(h), _) => (w, h),
            (Some(w), None, Some(r)) => (w, w / r),
            (None, Some(h), Some(r)) => (h * r, h),
            (Some(w), None, None) => (w, area.height),
            (None, Some(h), None) => (area.width, h),
            (None, None, Some(r)) => {
                if area.width / area.height > r { (area.height * r, area.height) } else { (area.width, area.width / r) }
            }
            (None, None, None) => (area.width, area.height),
        },
        None => (b.natur_breite.max(1) as f32, b.natur_hoehe.max(1) as f32),
    };
    // Slice offsets in buffer pixels; numbers are concrete image pixels,
    // percentages refer to the image size.
    let slice: [f32; 4] = std::array::from_fn(|i| {
        let (v, percent) = bi.slice[i];
        let (base, concrete) = if i % 2 == 0 { (ih, concrete_h) } else { (iw, concrete_w) };
        let v = if percent {
            v / 100.0 * base
        } else if concrete > 0.0 && concrete.is_finite() {
            v * base / concrete
        } else {
            v
        };
        v.clamp(0.0, base)
    });
    // Widths of the border image parts.
    let mut width: [f32; 4] = std::array::from_fn(|i| {
        let base = if i % 2 == 0 { area.height } else { area.width };
        match bi.width[i] {
            W::Auto => slice[i],
            W::Px(v) => v,
            W::Factor(f) => f * border[i],
            W::Percent(p) => p / 100.0 * base,
        }
        .max(0.0)
    });
    // Opposite parts that overlap scale all parts down (§6.2).
    let f = (area.width / (width[1] + width[3])).min(area.height / (width[0] + width[2]));
    if f.is_finite() && f < 1.0 {
        for w in width.iter_mut() {
            *w *= f;
        }
    }
    let w = area.width.ceil() as u32;
    let h = area.height.ceil() as u32;
    let count = (w as usize).checked_mul(h as usize)?;
    if count > 4_000_000 || count > budget.get() {
        return None;
    }
    budget.set(budget.get() - count);
    let [wt, wr, wb, wl] = width;
    let [st, sr, sb, sl] = slice;
    // Per axis: (tile size, start relative to the target range, period).
    // `kind`: 0 stretch, 1 repeat, 2 round, 3 space.
    let middle = |kind: u8, target: f32, source: f32, scale: f32| -> (f32, f32, f32) {
        if source <= 0.0 || target <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        let tile = if scale > 0.0 && scale.is_finite() { source * scale } else { target };
        match kind {
            1 => {
                // Tiles centred in the range.
                let n = (target / tile).ceil();
                let start = (target - n * tile) / 2.0;
                (tile, start, tile)
            }
            2 => {
                let n = (target / tile).round().max(1.0);
                (target / n, 0.0, target / n)
            }
            3 => {
                let n = (target / tile).floor();
                if n < 1.0 {
                    return (0.0, 0.0, 0.0);
                }
                let gap = (target - n * tile) / (n + 1.0);
                (tile, gap, tile + gap)
            }
            _ => (target, 0.0, target),
        }
    };
    let (mw, mh) = ((area.width - wl - wr).max(0.0), (area.height - wt - wb).max(0.0));
    let (qw, qh) = ((iw - sl - sr).max(0.0), (ih - st - sb).max(0.0));
    // Scales: horizontal edges follow the top (bottom) part, vertical edges
    // the left (right) part (§6.5).
    let sc_t = if st > 0.0 { wt / st } else { 0.0 };
    let sc_b = if sb > 0.0 { wb / sb } else { 0.0 };
    let sc_l = if sl > 0.0 { wl / sl } else { 0.0 };
    let sc_r = if sr > 0.0 { wr / sr } else { 0.0 };
    let [kind_x, kind_y] = bi.repeat;
    let tx_t = middle(kind_x, mw, qw, sc_t);
    let tx_b = middle(kind_x, mw, qw, sc_b);
    let ty_l = middle(kind_y, mh, qh, sc_l);
    let ty_r = middle(kind_y, mh, qh, sc_r);
    let tx_m = middle(kind_x, mw, qw, if sc_t > 0.0 { sc_t } else { sc_b });
    let ty_m = middle(kind_y, mh, qh, if sc_l > 0.0 { sc_l } else { sc_r });
    // Position inside the source range of a middle axis (`None` in a gap).
    let in_middle = |p: f32, t: (f32, f32, f32), source_start: f32, source: f32| -> Option<f32> {
        let (tile, start, period) = t;
        if tile <= 0.0 || period <= 0.0 {
            return None;
        }
        // `space`: a gap precedes the first tile.
        if period > tile && p < start {
            return None;
        }
        let r = (p - start).rem_euclid(period);
        (r < tile).then(|| source_start + r / tile * source)
    };
    let mut pixels = vec![0u8; count * 4];
    for y in 0..h {
        let py = y as f32 + 0.5;
        // Row: 0 top, 1 middle, 2 bottom; plus the local y.
        let (row, ry) = if py < wt {
            (0u8, py)
        } else if py >= area.height - wb {
            (2u8, py - (area.height - wb))
        } else {
            (1u8, py - wt)
        };
        for x in 0..w {
            let px = x as f32 + 0.5;
            let (col, rx) = if px < wl {
                (0u8, px)
            } else if px >= area.width - wr {
                (2u8, px - (area.width - wr))
            } else {
                (1u8, px - wl)
            };
            if row == 1 && col == 1 && !bi.fill {
                continue;
            }
            let u = match col {
                0 => Some(if wl > 0.0 { rx / wl * sl } else { continue }),
                2 => Some(if wr > 0.0 { iw - sr + rx / wr * sr } else { continue }),
                _ => match row {
                    0 => in_middle(rx, tx_t, sl, qw),
                    2 => in_middle(rx, tx_b, sl, qw),
                    _ => in_middle(rx, tx_m, sl, qw),
                },
            };
            let v = match row {
                0 => Some(if wt > 0.0 { ry / wt * st } else { continue }),
                2 => Some(if wb > 0.0 { ih - sb + ry / wb * sb } else { continue }),
                _ => match col {
                    0 => in_middle(ry, ty_l, st, qh),
                    2 => in_middle(ry, ty_r, st, qh),
                    _ => in_middle(ry, ty_m, st, qh),
                },
            };
            let (Some(u), Some(v)) = (u, v) else { continue };
            let ix = (u.max(0.0) as u32).min(b.breite - 1);
            let iy = (v.max(0.0) as u32).min(b.hoehe - 1);
            let from = ((iy * b.breite + ix) * 4) as usize;
            let to = ((y * w + x) * 4) as usize;
            if let Some(p) = b.pixels.get(from..from + 4) {
                pixels[to..to + 4].copy_from_slice(p);
            }
        }
    }
    Some(DisplayItem::Bild {
        rect: area,
        breite: w,
        hoehe: h,
        pixels: Arc::new(pixels),
    })
}

/// Release 14.10.2026 (Agent CSS2): the concrete size of a vector background
/// image (CSS Images 3 §5.3 default sizing with the positioning area
/// `area_w` x `area_h` as default object size; `cover`/`contain` without a
/// ratio fill the area, CSS Backgrounds 3 §3.9).
fn concrete_vector_size(
    geo: &narsil_css::BildGeo,
    width: Option<f32>,
    height: Option<f32>,
    ratio: Option<f32>,
    area_w: f32,
    area_h: f32,
) -> (f32, f32) {
    if geo.cover || geo.contain {
        return match ratio {
            Some(r) => {
                let wide = area_w / area_h > r;
                if wide == geo.cover { (area_w, area_w / r) } else { (area_h * r, area_h) }
            }
            None => (area_w, area_h),
        };
    }
    let specified = |l: Option<narsil_css::Laenge>, area: f32| {
        l.map(|l| if l.einheit == narsil_css::Einheit::Prozent { l.wert / 100.0 * area } else { l.wert })
    };
    let mut spec_w = specified(geo.groesse_xy[0], area_w);
    let spec_h = specified(geo.groesse_xy[1], area_h);
    if geo.groesse_xy == [None, None] && geo.groesse > 0.0 {
        spec_w = Some(geo.groesse);
    }
    match (spec_w, spec_h) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, ratio.map_or(height.unwrap_or(area_h), |r| w / r)),
        (None, Some(h)) => (ratio.map_or(width.unwrap_or(area_w), |r| h * r), h),
        (None, None) => match (width, height, ratio) {
            (Some(w), Some(h), _) => (w, h),
            (Some(w), None, Some(r)) => (w, w / r),
            (None, Some(h), Some(r)) => (h * r, h),
            (Some(w), None, None) => (w, area_h),
            (None, Some(h), None) => (area_w, h),
            (None, None, Some(r)) => {
                // `contain` inside the area.
                if area_w / area_h > r { (area_h * r, area_h) } else { (area_w, area_w / r) }
            }
            (None, None, None) => (area_w, area_h),
        },
    }
}
