//! `<text>`, `<tspan>` and `<a>` inside text: white space handling,
//! per-character `x`/`y`/`dx`/`dy`/`rotate`, text chunks with
//! `text-anchor`, `dominant-baseline`, and glyph outlines from
//! `narsil_text`.

use crate::render::{font_size, BBox, Renderer, View};
use crate::units::{self, Axis};
use crate::xml::{Element, Node};
use narsil_canvas::{Context2D, Matrix, Path};
use narsil_text::{GlyphQuelle, Schriftstapel};
use std::collections::HashMap;
use std::sync::Arc;

/// Characters a text element may lay out.
const MAX_CHARS: usize = 20_000;

/// One positioned glyph.
struct Glyph {
    /// The element whose style paints the glyph.
    owner: usize,
    q: GlyphQuelle,
    font: usize,
    size: f64,
    x: f64,
    y: f64,
    advance: f64,
    rotate: f64,
    /// The `textPath` the glyph follows; `x` is then the distance along
    /// it until the glyphs are placed.
    path: Option<usize>,
    /// The path tangent angle (radians) once placed.
    angle: f64,
}

/// A flattened text path: points with cumulative lengths.
struct TextPath {
    points: Vec<(f64, f64)>,
    lengths: Vec<f64>,
    /// `startOffset` in user units (`pathLength` applied).
    start: f64,
}

impl TextPath {
    fn total(&self) -> f64 {
        self.lengths.last().copied().unwrap_or(0.0)
    }

    /// The point and tangent angle at distance `d`.
    fn at(&self, d: f64) -> Option<(f64, f64, f64)> {
        if self.points.len() < 2 || !(0.0..=self.total() + 1e-9).contains(&d) {
            return None;
        }
        let k = self.lengths.partition_point(|l| *l < d).clamp(1, self.points.len() - 1);
        let (a, b) = (self.points[k - 1], self.points[k]);
        let seg = self.lengths[k] - self.lengths[k - 1];
        let t = if seg > 0.0 { (d - self.lengths[k - 1]) / seg } else { 0.0 };
        Some((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, (b.1 - a.1).atan2(b.0 - a.0)))
    }
}

/// The path a `textPath` element follows (its `path` attribute, else the
/// element its `href` names), in the text's user space.
fn text_path(r: &Renderer, tp: usize, view: View) -> Option<TextPath> {
    let el = r.doc.items[tp].el;
    let mut segs = None;
    // The author's `pathLength` of the referenced element.
    let mut author_length: Option<f64> = None;
    if let Some(d) = el.attr("path") {
        let s = crate::render::segments_from_commands(&narsil_canvas::commands::parse_svg_path(d.trim()));
        if !s.is_empty() {
            segs = Some(s);
        }
    }
    if segs.is_none() {
        let target = el.attr("href").and_then(|h| r.doc.lookup(h))?;
        let s = r.shape_path(target, view)?;
        // The referenced element's transform maps its outline.
        let t = crate::transform::parse(r.doc.items[target].el.attr("transform").unwrap_or("")).unwrap_or(Matrix::IDENTITY);
        let s: Vec<crate::render::Seg> = if t.is_identity() { s } else { transform_segs(&s, &t) };
        author_length = r.doc.items[target].el.attr("pathLength").and_then(units::number).filter(|p| *p >= 0.0);
        segs = Some(s);
    }
    let segs = segs?;
    let mut sub = crate::render::flatten(&segs);
    if el.attr("side").map(str::trim) == Some("right") {
        sub.reverse();
        for (p, _) in &mut sub {
            p.reverse();
        }
    }
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut lengths: Vec<f64> = Vec::new();
    for (p, _) in sub {
        for (k, pt) in p.into_iter().enumerate() {
            match points.last() {
                // Subpaths continue each other without the jump.
                Some(last) if k > 0 => {
                    let l = lengths.last().copied().unwrap_or(0.0) + (pt.0 - last.0).hypot(pt.1 - last.1);
                    lengths.push(l);
                }
                Some(_) => {
                    lengths.push(lengths.last().copied().unwrap_or(0.0));
                }
                None => lengths.push(0.0),
            }
            points.push(pt);
        }
    }
    let total = lengths.last().copied().unwrap_or(0.0);
    let c = units::Ctx { vw: view.w, vh: view.h, font_size: font_size(r.style(tp), view) };
    // A length `startOffset` is in the author's `pathLength` units
    // (`pathLength="0"` scales infinitely); a percentage is of the path's
    // own length and not scaled (WPT svg/text
    // textpath-pathlength-zero-offset-percentage, as Chrome).
    let start = match el.attr("startOffset").map(str::trim) {
        Some(s) if s.ends_with('%') => units::fraction(s).map_or(0.0, |f| f * total),
        Some(s) => {
            let author = units::length(s, Axis::X, &c).unwrap_or(0.0);
            match author_length {
                _ if author == 0.0 => 0.0,
                Some(p) if p == 0.0 => author.signum() * f64::INFINITY,
                Some(p) => author * total / p,
                None => author,
            }
        }
        None => 0.0,
    };
    Some(TextPath { points, lengths, start })
}

fn transform_segs(s: &[crate::render::Seg], t: &Matrix) -> Vec<crate::render::Seg> {
    use crate::render::Seg;
    let p = |x: f64, y: f64| t.apply(x, y);
    let mut out = Vec::with_capacity(s.len());
    for seg in s {
        out.push(match *seg {
            Seg::Move(x, y) => {
                let (a, b) = p(x, y);
                Seg::Move(a, b)
            }
            Seg::Line(x, y) => {
                let (a, b) = p(x, y);
                Seg::Line(a, b)
            }
            Seg::Quad(x1, y1, x, y) => {
                let ((a, b), (c, d)) = (p(x1, y1), p(x, y));
                Seg::Quad(a, b, c, d)
            }
            Seg::Cubic(x1, y1, x2, y2, x, y) => {
                let ((a, b), (c, d), (e, f)) = (p(x1, y1), p(x2, y2), p(x, y));
                Seg::Cubic(a, b, c, d, e, f)
            }
            Seg::Arc(..) => {
                // Arcs are flattened through a path first.
                return crate::render::segments_from_polyline(&crate::render::flatten(s), t);
            }
            Seg::Close => Seg::Close,
        });
    }
    out
}

/// Font stacks with their vertical metrics (ascent, descent, x-height).
struct FontSet {
    stacks: Vec<(Arc<Schriftstapel>, f64, f64, f64)>,
    by_key: HashMap<(String, u16, bool), usize>,
}

impl FontSet {
    fn get(&mut self, family: &str, weight: u16, italic: bool) -> Option<usize> {
        let key = (family.to_string(), weight, italic);
        if let Some(&i) = self.by_key.get(&key) {
            return Some(i);
        }
        let stack = narsil_text::stapel_fuer_familie(family, weight, italic).or_else(|| narsil_text::stapel_fuer_familie("sans-serif", weight, italic))?;
        let (ascent, descent, x_height) = metrics(&stack);
        self.stacks.push((stack, ascent, descent, x_height));
        let i = self.stacks.len() - 1;
        self.by_key.insert(key, i);
        Some(i)
    }
}

fn be_i16(d: &[u8], at: usize) -> Option<i16> {
    d.get(at..at + 2).map(|b| i16::from_be_bytes([b[0], b[1]]))
}

/// Ascent, descent (positive down) and x-height in em.
fn metrics(stack: &Schriftstapel) -> (f64, f64, f64) {
    let Some(font) = stack.schrift(0).map(|s| &s.font) else { return (0.8, 0.2, 0.5) };
    let upem = f64::from(font.units_per_em().max(1));
    let (a, d) = font
        .table(b"hhea")
        .and_then(|t| Some((f64::from(be_i16(t, 4)?), f64::from(be_i16(t, 6)?))))
        .map_or_else(
            || {
                let z = font.zeilenmetriken();
                (f64::from(z.ascent_em), -f64::from(z.descent_em))
            },
            |(a, d)| (a / upem, -d / upem),
        );
    let x_height = font.table(b"OS/2").filter(|t| be_i16(t, 0).is_some_and(|v| v >= 2)).and_then(|t| be_i16(t, 86)).filter(|v| *v > 0).map_or(0.5, |v| f64::from(v) / upem);
    (a, d, x_height)
}

fn weight_of(v: &str) -> u16 {
    match v.trim().to_ascii_lowercase().as_str() {
        "bold" | "bolder" => 700,
        "lighter" => 300,
        "normal" => 400,
        other => other.parse::<u16>().ok().filter(|w| (1..=1000).contains(w)).unwrap_or(400),
    }
}

/// Positional attributes of one element over its characters.
struct Positions {
    order: usize,
    start: usize,
    end: usize,
    x: Vec<f64>,
    y: Vec<f64>,
    dx: Vec<f64>,
    dy: Vec<f64>,
    rotate: Vec<f64>,
}

struct Collector<'r, 'd, 'a> {
    r: &'r Renderer<'d, 'a>,
    view: View,
    chars: Vec<(char, usize, Option<usize>)>,
    positions: Vec<Positions>,
    last_space: bool,
}

impl Collector<'_, '_, '_> {
    fn lengths(&self, i: usize, name: &str, axis: Axis) -> Vec<f64> {
        let Some(v) = self.r.doc.items[i].el.attr(name) else { return Vec::new() };
        let c = units::Ctx { vw: self.view.w, vh: self.view.h, font_size: font_size(&self.r.style(i), self.view) };
        units::length_list(v, axis, &c).unwrap_or_default()
    }

    fn walk(&mut self, el: &Element, owner: usize, preserve: bool, depth: usize, in_path: Option<usize>) {
        if depth > 32 {
            return;
        }
        let order = self.positions.len();
        self.positions.push(Positions { order, start: self.chars.len(), end: 0, x: Vec::new(), y: Vec::new(), dx: Vec::new(), dy: Vec::new(), rotate: Vec::new() });
        for n in &el.children {
            match n {
                Node::Text(t) => {
                    for mut c in t.chars() {
                        if self.chars.len() >= MAX_CHARS {
                            break;
                        }
                        if preserve {
                            if matches!(c, '\n' | '\r' | '\t') {
                                c = ' ';
                            }
                        } else {
                            if matches!(c, '\n' | '\r') {
                                continue;
                            }
                            if c == '\t' {
                                c = ' ';
                            }
                            if c == ' ' && (self.last_space || self.chars.is_empty()) {
                                continue;
                            }
                        }
                        self.last_space = c == ' ';
                        self.chars.push((c, owner, in_path));
                    }
                }
                Node::Element(e) => {
                    let Some(ci) = self.r.doc.index_of(e) else { continue };
                    if !matches!(e.name.as_str(), "tspan" | "a" | "textPath") {
                        continue;
                    }
                    if self.r.style(ci).get("display").eq_ignore_ascii_case("none") {
                        continue;
                    }
                    let p = e.attr("xml:space").map_or(preserve, |v| v == "preserve");
                    // A text path inside a text path is not rendered.
                    if e.name == "textPath" && in_path.is_some() {
                        continue;
                    }
                    let path = if e.name == "textPath" { Some(ci) } else { in_path };
                    self.walk(e, ci, p, depth + 1, path);
                }
            }
        }
        let p = &mut self.positions[order];
        p.end = self.chars.len();
        if matches!(self.r.doc.items[owner].el.name.as_str(), "text" | "tspan") {
            let (x, y) = (self.lengths(owner, "x", Axis::X), self.lengths(owner, "y", Axis::Y));
            let (dx, dy) = (self.lengths(owner, "dx", Axis::X), self.lengths(owner, "dy", Axis::Y));
            let rotate = self.r.doc.items[owner].el.attr("rotate").map(units::number_list).unwrap_or_default();
            let p = &mut self.positions[order];
            (p.x, p.y, p.dx, p.dy, p.rotate) = (x, y, dx, dy, rotate);
        }
    }
}

/// The laid out text of element `i` in its user space.
fn layout(r: &Renderer, i: usize, view: View, fonts: &mut FontSet) -> Vec<Glyph> {
    let el = r.doc.items[i].el;
    let preserve = el.attr("xml:space") == Some("preserve");
    let mut col = Collector { r, view, chars: Vec::new(), positions: Vec::new(), last_space: false };
    col.walk(el, i, preserve, 0, None);
    let mut chars = col.chars;
    if !preserve && chars.last().is_some_and(|c| c.0 == ' ') {
        chars.pop();
    }
    let n = chars.len();
    let mut xs = vec![None; n];
    let mut ys = vec![None; n];
    let mut dxs = vec![0.0; n];
    let mut dys = vec![0.0; n];
    let mut rot = vec![0.0; n];
    let mut positions = col.positions;
    positions.sort_by_key(|p| p.order);
    for p in &positions {
        let end = p.end.min(n);
        let span = end.saturating_sub(p.start);
        for (k, v) in p.x.iter().take(span).enumerate() {
            xs[p.start + k] = Some(*v);
        }
        for (k, v) in p.y.iter().take(span).enumerate() {
            ys[p.start + k] = Some(*v);
        }
        for (k, v) in p.dx.iter().take(span).enumerate() {
            dxs[p.start + k] = *v;
        }
        for (k, v) in p.dy.iter().take(span).enumerate() {
            dys[p.start + k] = *v;
        }
        if let Some(last) = p.rotate.last() {
            for k in 0..span {
                rot[p.start + k] = *p.rotate.get(k).unwrap_or(last);
            }
        }
    }
    let rtl = r.style(i).get("direction").eq_ignore_ascii_case("rtl");
    let override_bidi = r.style(i).get("unicode-bidi").to_ascii_lowercase().ends_with("override");
    let mut out = Vec::with_capacity(n);
    let (mut px, mut py) = (0.0f64, 0.0f64);
    let mut chunk_start = 0usize;
    let mut prev: Option<(usize, usize, GlyphQuelle)> = None;
    let finish_chunk = |out: &mut Vec<Glyph>, from: usize| {
        if from >= out.len() {
            return;
        }
        let first = &out[from];
        let anchor = r.style(first.owner).get("text-anchor").to_ascii_lowercase();
        let x0 = first.x;
        let last = &out[out.len() - 1];
        let width = last.x + last.advance - x0;
        let shift = match (anchor.as_str(), rtl) {
            ("middle", _) => -width / 2.0,
            ("end", false) | ("start", true) => -width,
            _ => 0.0,
        };
        if shift != 0.0 {
            for g in &mut out[from..] {
                g.x += shift;
            }
        }
    };
    let mut paths: HashMap<usize, Option<TextPath>> = HashMap::new();
    let mut cur_path: Option<usize> = None;
    // Text chunks (an absolute position or a text path starts one), each
    // laid out in its bidi visual order.
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut chunk_of = vec![0usize; n];
    let mut s0 = 0;
    for k in 1..=n {
        if k == n || xs[k].is_some() || ys[k].is_some() || chars[k].2 != chars[k - 1].2 {
            let part: Vec<char> = chars[s0..k].iter().map(|c| c.0).collect();
            if override_bidi {
                // `unicode-bidi: bidi-override`: the logical order in the
                // base direction.
                if rtl { order.extend((s0..k).rev()) } else { order.extend(s0..k) }
            } else if !rtl && part.iter().all(|c| (*c as u32) < 0x0590) {
                order.extend(s0..k);
            } else {
                let dir = if rtl { narsil_text::bidi::Absatzrichtung::Rtl } else { narsil_text::bidi::Absatzrichtung::Ltr };
                let a = narsil_text::bidi::analysiere_zeichen(&part, dir);
                let mut seen = vec![false; part.len()];
                for &v in &a.reihenfolge {
                    if v < part.len() && !seen[v] {
                        seen[v] = true;
                        order.push(s0 + v);
                    }
                }
                order.extend((0..part.len()).filter(|v| !seen[*v]).map(|v| s0 + v));
            }
            for c in &mut chunk_of[s0..k] {
                *c = s0;
            }
            s0 = k;
        }
    }
    for (pos, &k) in order.iter().enumerate() {
        let (ch, owner, in_path) = chars[k];
        let chunk = chunk_of[k];
        let new_chunk = pos == 0 || chunk_of[order[pos - 1]] != chunk;
        let path_change = in_path != cur_path;
        if pos > 0 && new_chunk {
            finish_chunk(&mut out, chunk_start);
            chunk_start = out.len();
        }
        if path_change {
            if let Some(tp) = in_path {
                let start = paths.entry(tp).or_insert_with(|| text_path(r, tp, view)).as_ref().map_or(f64::NAN, |p| p.start);
                px = start;
                py = 0.0;
            } else if let (Some(last), Some(old)) = (out.last(), cur_path) {
                // After a text path, text continues where its last glyph ends.
                if let Some((x, y, _)) = paths.get(&old).and_then(|p| p.as_ref()).and_then(|p| p.at((last.x + last.advance).min(p.total()))) {
                    (px, py) = (x, y);
                }
            }
            cur_path = in_path;
        }
        if new_chunk {
            if let Some(x) = xs[chunk] {
                px = x;
            }
            if let Some(y) = ys[chunk].filter(|_| in_path.is_none()) {
                py = y;
            }
        }
        px += dxs[k];
        py += dys[k];
        let st = &r.style(owner);
        let size = font_size(st, view);
        let family = st.get("font-family").to_string();
        let italic = !st.get("font-style").eq_ignore_ascii_case("normal");
        let Some(font) = fonts.get(&family, weight_of(st.get("font-weight")), italic) else { continue };
        let stack = &fonts.stacks[font].0;
        let q = stack.glyphe_fuer(ch).unwrap_or(GlyphQuelle::NOTDEF);
        if let Some((po, pf, pq)) = prev {
            if po == owner && pf == font {
                px += f64::from(stack.kern(pq, q, size as f32));
            }
        }
        let c = units::Ctx { vw: view.w, vh: view.h, font_size: size };
        let spacing = |name: &str| -> f64 {
            let v = st.get(name);
            if v.eq_ignore_ascii_case("normal") { 0.0 } else { units::length(v, Axis::X, &c).unwrap_or(0.0) }
        };
        // Bidi controls and other format characters take no room.
        let invisible = matches!(ch as u32, 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2069 | 0xFEFF);
        if invisible {
            out.push(Glyph { owner, q: GlyphQuelle::NOTDEF, font, size, x: px, y: py, advance: 0.0, rotate: rot[k], path: in_path, angle: 0.0 });
            continue;
        }
        let mut advance = f64::from(stack.vorschub(q, size as f32)) + spacing("letter-spacing");
        if ch == ' ' {
            advance += spacing("word-spacing");
        }
        let baseline = baseline_shift(r.style(i).get("dominant-baseline"), &fonts.stacks[font], size);
        out.push(Glyph { owner, q: if ch.is_whitespace() { GlyphQuelle::NOTDEF } else { q }, font, size, x: px, y: py + baseline, advance, rotate: rot[k], path: in_path, angle: 0.0 });
        px += advance;
        prev = Some((owner, font, q));
    }
    finish_chunk(&mut out, chunk_start);
    // Glyphs on a text path sit at the point of their middle, rotated with
    // the tangent; those whose middle is off the path are not rendered.
    // A mark without advance (a combining character) stays with its base
    // glyph: same anchor, same angle.
    let mut base: Option<(usize, f64, f64, f64, f64, f64)> = None;
    out.retain_mut(|g| {
        let Some(tp) = g.path else { return true };
        let Some(Some(p)) = paths.get(&tp) else { return false };
        if g.advance == 0.0 {
            if let Some((_, lx, ly, bx, by, a)) = base.filter(|b| b.0 == tp) {
                let (s, c) = a.sin_cos();
                let (dx, dy) = (g.x - lx, g.y - ly);
                g.x = bx + c * dx - s * dy;
                g.y = by + s * dx + c * dy;
                g.angle = a;
                return true;
            }
        }
        let mid = g.x + g.advance / 2.0;
        let Some((x, y, a)) = mid.is_finite().then(|| p.at(mid)).flatten() else {
            base = None;
            return false;
        };
        let (s, c) = a.sin_cos();
        let (lx, ly) = (g.x, g.y);
        g.x = x - c * g.advance / 2.0 - s * g.y;
        g.y = y - s * g.advance / 2.0 + c * g.y;
        g.angle = a;
        base = Some((tp, lx, ly, g.x, g.y, a));
        true
    });
    out
}

/// How far the alphabetic baseline sits below the requested baseline.
fn baseline_shift(v: &str, font: &(Arc<Schriftstapel>, f64, f64, f64), size: f64) -> f64 {
    let (_, ascent, descent, x_height) = font;
    match v.to_ascii_lowercase().as_str() {
        "middle" => x_height / 2.0 * size,
        "central" => (ascent - descent) / 2.0 * size,
        "hanging" => 0.8 * ascent * size,
        "text-before-edge" | "text-top" => ascent * size,
        "text-after-edge" | "text-bottom" | "ideographic" => -descent * size,
        "mathematical" => 0.5 * ascent * size,
        _ => 0.0,
    }
}

fn glyph_outline(path: &mut Path, font: &narsil_text::TrueTypeFont, gid: u32, m: &Matrix) {
    let Ok(outline) = font.glyph_outline(gid) else { return };
    for contour in &outline.contours {
        let pts = &contour.points;
        let n = pts.len();
        if n == 0 {
            continue;
        }
        let p = |i: usize| (f64::from(pts[i % n].x), f64::from(pts[i % n].y));
        let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
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

fn new_fonts() -> FontSet {
    FontSet { stacks: Vec::new(), by_key: HashMap::new() }
}

/// The glyph cells' union in the coordinate system `m` maps into.
fn cells_bbox(glyphs: &[Glyph], fonts: &FontSet, m: &Matrix) -> Option<BBox> {
    let mut acc: Option<BBox> = None;
    for g in glyphs {
        let (_, ascent, descent, _) = &fonts.stacks[g.font];
        let (s, c) = g.angle.sin_cos();
        let (y0, y1) = (-ascent * g.size, descent * g.size);
        for (u, v) in [(0.0, y0), (g.advance, y0), (0.0, y1), (g.advance, y1)] {
            let (x, y) = (g.x + u * c - v * s, g.y + u * s + v * c);
            let (dx, dy) = m.apply(x, y);
            acc = Some(match acc {
                None => (dx, dy, dx, dy),
                Some(a) => (a.0.min(dx), a.1.min(dy), a.2.max(dx), a.3.max(dy)),
            });
        }
    }
    acc
}

/// The object bounding box of a text element.
pub fn text_bbox(r: &Renderer, i: usize, m: Matrix, view: View) -> Option<BBox> {
    let text = text_root(r, i);
    let mut fonts = new_fonts();
    let glyphs = layout(r, text, view, &mut fonts);
    let own: Vec<Glyph> = if text == i { glyphs } else { glyphs.into_iter().filter(|g| is_inside(r, g.owner, i)).collect() };
    cells_bbox(&own, &fonts, &m)
}

fn text_root(r: &Renderer, mut i: usize) -> usize {
    while r.doc.items[i].el.name != "text" {
        match r.doc.items[i].parent {
            Some(p) => i = p,
            None => break,
        }
    }
    i
}

fn is_inside(r: &Renderer, mut a: usize, b: usize) -> bool {
    loop {
        if a == b {
            return true;
        }
        match r.doc.items[a].parent {
            Some(p) => a = p,
            None => return false,
        }
    }
}

/// The glyph outlines of a text element under `m` (clip paths).
pub fn text_outline(r: &Renderer, i: usize, m: Matrix, view: View) -> Path {
    let mut fonts = new_fonts();
    let glyphs = layout(r, i, view, &mut fonts);
    let mut path = Path::new();
    for g in &glyphs {
        if g.q == GlyphQuelle::NOTDEF || r.style(g.owner).get("visibility").to_ascii_lowercase() != "visible" {
            continue;
        }
        add_glyph(&mut path, &fonts, g, &m);
    }
    path
}

fn add_glyph(path: &mut Path, fonts: &FontSet, g: &Glyph, m: &Matrix) {
    let stack = &fonts.stacks[g.font].0;
    let Some(font) = stack.schrift(g.q.schrift).map(|f| &f.font) else { return };
    let scale = g.size / f64::from(font.units_per_em().max(1));
    let mut gm = Matrix::translate(g.x, g.y);
    if g.angle != 0.0 || g.rotate != 0.0 {
        gm = gm.multiply(&Matrix::rotate(g.angle + g.rotate.to_radians()));
    }
    let gm = m.multiply(&gm).multiply(&Matrix::new(scale, 0.0, 0.0, -scale, 0.0, 0.0));
    glyph_outline(path, font, g.q.glyph, &gm);
}

/// Paints a `<text>` element.
pub fn draw_text(r: &mut Renderer, ctx: &mut Context2D, i: usize, m: Matrix, view: View) {
    let mut fonts = new_fonts();
    let glyphs = layout(r, i, view, &mut fonts);
    if glyphs.is_empty() {
        return;
    }
    let bbox = cells_bbox(&glyphs, &fonts, &Matrix::IDENTITY);
    // Runs of glyphs with the same owner paint together.
    let mut start = 0;
    while start < glyphs.len() {
        let owner = glyphs[start].owner;
        let mut end = start + 1;
        while end < glyphs.len() && glyphs[end].owner == owner {
            end += 1;
        }
        let run = &glyphs[start..end];
        start = end;
        if r.style(owner).get("visibility").to_ascii_lowercase() != "visible" {
            continue;
        }
        let mut path = Path::new();
        for g in run {
            if g.q != GlyphQuelle::NOTDEF {
                add_glyph(&mut path, &fonts, g, &m);
            }
        }
        // `opacity` on text content children (tspan, textPath, a).
        let mut k = 1.0;
        let mut e = owner;
        while e != i {
            k *= units::fraction(r.style(e).get("opacity")).unwrap_or(1.0).clamp(0.0, 1.0);
            match r.doc.items[e].parent {
                Some(p) => e = p,
                None => break,
            }
        }
        let fill = r.paint_style(owner, true, bbox, m, view);
        let stroke = r.paint_style(owner, false, bbox, m, view);
        let order = r.style(owner).get("paint-order").to_ascii_lowercase();
        let steps = crate::render::paint_order(&order);
        let stroke_first = steps.iter().position(|c| *c == 's') < steps.iter().position(|c| *c == 'f');
        // A run with group opacity paints into a layer first.
        let mut layer = if k < 1.0 { Context2D::new(ctx.surface.width, ctx.surface.height, false) } else { None };
        {
            let target: &mut Context2D = match layer.as_mut() {
                Some(l) => l,
                None => &mut *ctx,
            };
            if stroke_first {
                r.stroke_path(target, owner, &path, m, view, stroke, 1.0);
                r.fill_path(target, owner, &path, m, fill);
            } else {
                r.fill_path(target, owner, &path, m, fill);
                r.stroke_path(target, owner, &path, m, view, stroke, 1.0);
            }
            decorate(r, target, owner, run, &fonts, m, view, bbox);
        }
        if let Some(l) = layer {
            let img = l.surface.snapshot();
            ctx.save();
            ctx.state.transform = Matrix::IDENTITY;
            ctx.state.global_alpha = k;
            ctx.state.smoothing = false;
            let (w, h) = (f64::from(img.width), f64::from(img.height));
            ctx.draw_image(&img, 0.0, 0.0, w, h, 0.0, 0.0, w, h);
            let _ = ctx.restore();
        }
    }
}

/// `text-decoration` underline / overline / line-through of a run.
#[allow(clippy::too_many_arguments)]
fn decorate(r: &mut Renderer, ctx: &mut Context2D, owner: usize, run: &[Glyph], fonts: &FontSet, m: Matrix, view: View, bbox: Option<BBox>) {
    // The decoration comes from the element that declares it.
    let mut el = owner;
    let mut lines = String::new();
    loop {
        let st = &r.style(el);
        if st.is_set("text-decoration") {
            lines = st.get("text-decoration").to_ascii_lowercase();
            break;
        }
        match r.doc.items[el].parent {
            Some(p) if matches!(r.doc.items[el].el.name.as_str(), "tspan" | "a" | "textPath") => el = p,
            _ => break,
        }
    }
    if lines.is_empty() || lines == "none" {
        return;
    }
    let first = &run[0];
    let last = &run[run.len() - 1];
    let (_, ascent, _, _) = &fonts.stacks[first.font];
    let size = first.size;
    let thickness = size / 14.0;
    let mut path = Path::new();
    let (x0, x1) = (first.x, last.x + last.advance);
    let mut add = |y: f64| {
        path.rect(&m, x0, y - thickness / 2.0, x1 - x0, thickness);
    };
    if lines.contains("underline") {
        add(first.y + size * 0.1);
    }
    if lines.contains("overline") {
        add(first.y - ascent * size);
    }
    if lines.contains("line-through") {
        add(first.y - size * 0.3);
    }
    let fill = r.paint_style(el, true, bbox, m, view);
    r.fill_path(ctx, el, &path, m, fill);
}
