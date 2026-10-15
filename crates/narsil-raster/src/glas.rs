//! Release 09.10.2026: LIQUID GLASS — echtes Hinterglas statt Abdunkeln.
//!
//! Eine Glasflaeche ([`narsil_display::Glas`]) wird in EINEM Durchgang je
//! Pixel gezeichnet, alles mit derselben Abstandsfunktion (SDF) des
//! gerundeten Rechtecks — dadurch sind Rundung, Kanten und Schatten
//! kantengeglaettet und passen genau aufeinander:
//!
//! 1. **Schatten**: weich (Gauss, ueber die Fehlerfunktion), nach unten
//!    versetzt, nur AUSSERHALB der Form — durch das Glas sieht man ihn nicht.
//! 2. **Hintergrund**: der Bereich unter der Form wird verkleinert
//!    (Kastenmittel), dreimal waagerecht und senkrecht kastengeglaettet
//!    (≈ Gauss), bilinear vergroessert, leicht gesaettigt, getoent und mit
//!    feinem, ortsfestem Rauschen gegen Banding versehen. GEMERKT wird das
//!    Ergebnis mit einer Pruefsumme des Untergrunds: aendert der sich nicht
//!    (Hover im Menue, Cursor), wird nicht neu weichgezeichnet.
//! 3. **Glanz**: ein sehr leiser Verlauf von oben.
//! 4. **Doppelte Lichtkante**: innen eine helle Linie (oben am hellsten,
//!    unten fast weg), aussen eine dunkle 1-px-Linie.

use std::cell::RefCell;

use narsil_core::color::Color;
use narsil_core::geometry::Rect;
use narsil_display::Glas;

use crate::framebuffer::{Framebuffer, blend_source_over};

/// M3: groesste Kante einer Glasflaeche in Pixeln.
const MAX_KANTE: f32 = 8192.0;
/// M1: so viele weichgezeichnete Untergruende werden gemerkt.
const MAX_GEMERKT: usize = 8;

/// Die vorzeichenbehaftete Entfernung zum gerundeten Rechteck (negativ =
/// innen), fuer einen Pixelmittelpunkt `(px, py)`. `radius` je Ecke:
/// oben-links, oben-rechts, unten-rechts, unten-links (wie CSS).
#[must_use]
pub fn abstand(px: f32, py: f32, r: Rect, radius: [f32; 4]) -> f32 {
    let hb = r.width / 2.0;
    let hh = r.height / 2.0;
    let rechts = px > r.x + hb;
    let unten = py > r.y + hh;
    let ecke = match (rechts, unten) {
        (false, false) => radius[0],
        (true, false) => radius[1],
        (true, true) => radius[2],
        (false, true) => radius[3],
    };
    let rad = if ecke.is_finite() { ecke.clamp(0.0, hb.min(hh)) } else { 0.0 };
    let qx = (px - (r.x + hb)).abs() - (hb - rad);
    let qy = (py - (r.y + hh)).abs() - (hh - rad);
    let aussen = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    aussen + qx.max(qy).min(0.0) - rad
}

/// Deckung einer Form an einem Pixel mit Abstand `d` (1 px Kantenbreite).
#[inline]
fn deckung(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// erfc-Naeherung (Abramowitz/Stegun 7.1.26, Fehler < 1,5e-7).
fn erfc(x: f32) -> f32 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let y = t
        * (0.254_829_6
            + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152_1 + t * 1.061_405_4))))
        * (-z * z).exp();
    if x >= 0.0 { y } else { 2.0 - y }
}

/// Ortsfestes Rauschen in [-1, 1] je Pixel (kein Flimmern beim Neuzeichnen).
#[inline]
fn rauschen(x: i64, y: i64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    ((h & 0xFFFF) as f32 / 32_767.5) - 1.0
}

fn mische(a: Color, b: [f32; 3], t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let k = |x: u8, y: f32| (f32::from(x) + (y - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color::new(k(a.r, b[0]), k(a.g, b[1]), k(a.b, b[2]), a.a)
}

struct Gemerkt {
    schluessel: [u32; 9],
    pruefsumme: u64,
    x0: i64,
    y0: i64,
    breite: usize,
    pixel: Vec<[f32; 3]>,
}

thread_local! {
    static GEMERKT: RefCell<Vec<Gemerkt>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    /// Zaehlt je Faden, wie oft wirklich weichgezeichnet wurde (wie das
    /// Gedaechtnis selbst je Faden).
    static WEICHGEZEICHNET: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Wie oft in diesem Faden bisher weichgezeichnet wurde (der Rest kam aus
/// dem Gedaechtnis).
#[must_use]
pub fn weichgezeichnet() -> u64 {
    WEICHGEZEICHNET.with(std::cell::Cell::get)
}

/// Der weichgezeichnete, gesaettigte und getoente Untergrund im Bereich
/// `x0..x1 × y0..y1` (Framebuffer-Pixel), zeilenweise.
fn untergrund(fb: &Framebuffer, g: &Glas, x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<[f32; 3]> {
    let breite = (x1 - x0).max(0) as usize;
    let hoehe = (y1 - y0).max(0) as usize;
    let sigma = g.unschaerfe.clamp(0.0, 64.0);
    // Der Rand, aus dem der Weichzeichner liest (sonst dunkelt die Kante ab).
    let rand = (sigma * 2.0).ceil() as i64;
    let (fw, fh) = (i64::from(fb.width()), i64::from(fb.height()));
    let qx0 = (x0 - rand).max(0);
    let qy0 = (y0 - rand).max(0);
    let qx1 = (x1 + rand).min(fw);
    let qy1 = (y1 + rand).min(fh);
    let pixel = |x: i64, y: i64| fb.pixel(x as u32, y as u32).unwrap_or(Color::TRANSPARENT);

    // Pruefsumme des Untergrunds (FNV-1a) — gleich = gemerktes Ergebnis.
    let mut pruefsumme: u64 = 0xcbf2_9ce4_8422_2325;
    for y in qy0..qy1 {
        for x in qx0..qx1 {
            let c = pixel(x, y);
            for b in [c.r, c.g, c.b] {
                pruefsumme ^= u64::from(b);
                pruefsumme = pruefsumme.wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    let schluessel = [
        x0 as u32,
        y0 as u32,
        x1 as u32,
        y1 as u32,
        g.unschaerfe.to_bits(),
        g.saettigung.to_bits(),
        u32::from_be_bytes([g.toenung.r, g.toenung.g, g.toenung.b, g.toenung.a]),
        g.rauschen.to_bits(),
        fb.width(),
    ];
    if let Some(p) = GEMERKT.with(|m| {
        m.borrow()
            .iter()
            .find(|e| e.schluessel == schluessel && e.pruefsumme == pruefsumme && e.x0 == x0 && e.y0 == y0 && e.breite == breite)
            .map(|e| e.pixel.clone())
    }) {
        return p;
    }
    WEICHGEZEICHNET.with(|z| z.set(z.get() + 1));

    // 1) Verkleinern: Kastenmittel je f×f.
    let f: i64 = if sigma >= 3.0 { ((sigma / 3.0).floor() as i64).clamp(1, 8) } else { 1 };
    let sw = ((qx1 - qx0 + f - 1) / f).max(1) as usize;
    let sh = ((qy1 - qy0 + f - 1) / f).max(1) as usize;
    let mut klein = vec![[0f32; 3]; sw * sh];
    for ky in 0..sh {
        for kx in 0..sw {
            let mut s = [0f32; 3];
            let mut n = 0f32;
            for y in (qy0 + ky as i64 * f)..(qy0 + (ky as i64 + 1) * f).min(qy1) {
                for x in (qx0 + kx as i64 * f)..(qx0 + (kx as i64 + 1) * f).min(qx1) {
                    let c = pixel(x, y);
                    s[0] += f32::from(c.r);
                    s[1] += f32::from(c.g);
                    s[2] += f32::from(c.b);
                    n += 1.0;
                }
            }
            if n > 0.0 {
                klein[ky * sw + kx] = [s[0] / n, s[1] / n, s[2] / n];
            }
        }
    }
    // 2) Drei Kastendurchgaenge je Richtung ≈ Gauss mit sigma.
    if sigma > 0.5 {
        let s_klein = sigma / f as f32;
        let r = ((s_klein * s_klein + 0.25).sqrt() - 0.5).round().max(1.0) as i64;
        let mut tmp = vec![[0f32; 3]; sw * sh];
        for _ in 0..3 {
            kasten(&klein, &mut tmp, sw, sh, r, true);
            kasten(&tmp, &mut klein, sw, sh, r, false);
        }
    }
    // 3) Bilinear zurueck auf den Bereich, Saettigung, Toenung, Rauschen.
    let toen = [f32::from(g.toenung.r), f32::from(g.toenung.g), f32::from(g.toenung.b)];
    let ta = f32::from(g.toenung.a) / 255.0;
    let sat = g.saettigung.clamp(0.0, 3.0);
    let mut aus = Vec::with_capacity(breite * hoehe);
    for y in y0..y1 {
        let sy = ((y - qy0) as f32 + 0.5) / f as f32 - 0.5;
        let yi = sy.floor().clamp(0.0, (sh - 1) as f32) as usize;
        let yj = (yi + 1).min(sh - 1);
        let ty = (sy - yi as f32).clamp(0.0, 1.0);
        for x in x0..x1 {
            let sx = ((x - qx0) as f32 + 0.5) / f as f32 - 0.5;
            let xi = sx.floor().clamp(0.0, (sw - 1) as f32) as usize;
            let xj = (xi + 1).min(sw - 1);
            let tx = (sx - xi as f32).clamp(0.0, 1.0);
            let mut c = [0f32; 3];
            for (k, ck) in c.iter_mut().enumerate() {
                let a = klein[yi * sw + xi][k] * (1.0 - tx) + klein[yi * sw + xj][k] * tx;
                let b = klein[yj * sw + xi][k] * (1.0 - tx) + klein[yj * sw + xj][k] * tx;
                *ck = a * (1.0 - ty) + b * ty;
            }
            let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            let n = rauschen(x, y) * g.rauschen;
            for k in 0..3 {
                let v = l + (c[k] - l) * sat;
                c[k] = (v + (toen[k] - v) * ta + n).clamp(0.0, 255.0);
            }
            aus.push(c);
        }
    }
    GEMERKT.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(|e| e.schluessel != schluessel);
        if m.len() >= MAX_GEMERKT {
            m.remove(0);
        }
        m.push(Gemerkt { schluessel, pruefsumme, x0, y0, breite, pixel: aus.clone() });
    });
    aus
}

/// Ein Kastendurchgang mit Radius `r` (waagerecht oder senkrecht), Rand
/// geklemmt.
fn kasten(ein: &[[f32; 3]], aus: &mut [[f32; 3]], w: usize, h: usize, r: i64, waagerecht: bool) {
    let (laenge, zeilen) = if waagerecht { (w, h) } else { (h, w) };
    let n = (2 * r + 1) as f32;
    let idx = |z: usize, i: usize| if waagerecht { z * w + i } else { i * w + z };
    for z in 0..zeilen {
        let hol = |i: i64| ein[idx(z, i.clamp(0, laenge as i64 - 1) as usize)];
        let mut s = [0f32; 3];
        for i in -r..=r {
            let c = hol(i);
            s[0] += c[0];
            s[1] += c[1];
            s[2] += c[2];
        }
        for i in 0..laenge {
            aus[idx(z, i)] = [s[0] / n, s[1] / n, s[2] / n];
            let weg = hol(i as i64 - r);
            let dazu = hol(i as i64 + r + 1);
            for k in 0..3 {
                s[k] += dazu[k] - weg[k];
            }
        }
    }
}

/// Zeichnet eine Glasflaeche. Rueckgabe: veraenderte Pixel.
pub fn zeichne(fb: &mut Framebuffer, g: &Glas) -> usize {
    let r = g.rect;
    if !(r.x.is_finite() && r.y.is_finite() && r.width.is_finite() && r.height.is_finite())
        || r.width <= 0.0
        || r.height <= 0.0
        || r.width > MAX_KANTE
        || r.height > MAX_KANTE
    {
        return 0;
    }
    let clip = fb.effektiver_clip();
    let (fw, fh) = (i64::from(fb.width()), i64::from(fb.height()));
    let schnitt = |x0: f32, y0: f32, x1: f32, y1: f32| {
        (
            (x0.floor() as i64).max(clip.x.floor() as i64).max(0),
            (y0.floor() as i64).max(clip.y.floor() as i64).max(0),
            (x1.ceil() as i64).min(clip.right().ceil() as i64).min(fw),
            (y1.ceil() as i64).min(clip.bottom().ceil() as i64).min(fh),
        )
    };
    let radius = g.radius;
    let linie = g.linie.clamp(0.5, 4.0);
    let mut veraendert = 0usize;

    // 1) DER SCHATTEN — nur ausserhalb der Form.
    if g.schatten > 0.0 && g.schatten_weite > 0.0 {
        let w = g.schatten_weite.min(64.0);
        let weit = 3.0 * w + g.schatten_y.abs();
        let (x0, y0, x1, y1) = schnitt(r.x - weit, r.y - weit, r.right() + weit, r.bottom() + weit);
        let geschoben = Rect::new(r.x, r.y + g.schatten_y, r.width, r.height);
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let innen = deckung(abstand(px, py, r, radius));
                if innen >= 1.0 {
                    continue;
                }
                let d = abstand(px, py, geschoben, radius);
                let a = g.schatten * 0.5 * erfc(d / (w * std::f32::consts::SQRT_2)) * (1.0 - innen);
                if a < 1.0 / 255.0 {
                    continue;
                }
                let Some(alt) = fb.pixel(x as u32, y as u32) else { continue };
                let neu = blend_source_over(Color::new(0, 0, 0, (a * 255.0).round() as u8), alt);
                if neu != alt && fb.set_pixel(x as u32, y as u32, neu) {
                    veraendert += 1;
                }
            }
        }
    }

    // 2) – 4) HINTERGRUND, GLANZ, KANTEN — in einem Durchgang.
    let (x0, y0, x1, y1) = schnitt(r.x - linie - 1.0, r.y - linie - 1.0, r.right() + linie + 1.0, r.bottom() + linie + 1.0);
    if x1 <= x0 || y1 <= y0 {
        return veraendert;
    }
    // Ohne Weichzeichnung, Saettigung, Toenung und Rauschen ist der
    // Untergrund der Framebuffer selbst — dann nur Glanz und Kanten.
    let nur_kanten = g.unschaerfe <= 0.0 && (g.saettigung - 1.0).abs() < 1e-3 && g.toenung.a == 0 && g.rauschen <= 0.0;
    let grund = if nur_kanten { Vec::new() } else { untergrund(fb, g, x0, y0, x1, y1) };
    let breite = (x1 - x0) as usize;
    for y in y0..y1 {
        let py = y as f32 + 0.5;
        // Anteil der Hoehe (0 oben, 1 unten) fuer Glanz und Lichtkante.
        let t = ((py - r.y) / r.height).clamp(0.0, 1.0);
        let glanz = g.glanz * (1.0 - t / 0.5).max(0.0).powf(1.5);
        let licht = g.kante_hell * (0.18 + 0.82 * (1.0 - t).powi(3));
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            let d = abstand(px, py, r, radius);
            let innen = deckung(d);
            let aussen_band = deckung(d - linie) - innen;
            let innen_band = innen - deckung(d + linie);
            if innen <= 0.0 && aussen_band <= 0.0 {
                continue;
            }
            let Some(alt) = fb.pixel(x as u32, y as u32) else { continue };
            // Durchsichtiger Grund (Ebenen, Testbilder): es gibt nichts
            // weichzuzeichnen — das Glas ist dann seine Toenung, Glanz und
            // Kanten werden darueber GEMISCHT (mit Alpha), nicht eingefaerbt.
            if alt.a < 255 {
                let weiss = |a: f32| Color::new(255, 255, 255, (a.clamp(0.0, 1.0) * 255.0).round() as u8);
                let mut c = alt;
                if innen > 0.0 {
                    let ta = f32::from(g.toenung.a) / 255.0 * innen;
                    c = blend_source_over(Color::new(g.toenung.r, g.toenung.g, g.toenung.b, (ta * 255.0).round() as u8), c);
                    c = blend_source_over(weiss(glanz * innen), c);
                    c = blend_source_over(weiss(licht * innen_band.max(0.0)), c);
                }
                if aussen_band > 0.0 {
                    let a = (g.kante_dunkel * aussen_band * 255.0).round() as u8;
                    c = blend_source_over(Color::new(0, 0, 0, a), c);
                }
                if c != alt && fb.set_pixel(x as u32, y as u32, c) {
                    veraendert += 1;
                }
                continue;
            }
            let mut c = alt;
            if innen > 0.0 {
                if !nur_kanten {
                    let u = grund[(y - y0) as usize * breite + (x - x0) as usize];
                    c = mische(c, u, innen);
                }
                if glanz > 0.0 {
                    c = mische(c, [255.0; 3], glanz * innen);
                }
                if innen_band > 0.0 && licht > 0.0 {
                    c = mische(c, [255.0; 3], licht * innen_band);
                }
            }
            if aussen_band > 0.0 && g.kante_dunkel > 0.0 {
                c = mische(c, [0.0; 3], g.kante_dunkel * aussen_band);
            }
            if c != alt && fb.set_pixel(x as u32, y as u32, c) {
                veraendert += 1;
            }
        }
    }
    veraendert
}

/// Gruppen-Deckkraft: mischt `neu` (der Bereich nach der Gruppe) mit `alt`
/// (davor) im Verhaeltnis `deckkraft`.
pub(crate) struct Ebene {
    pub bis: usize,
    pub x0: u32,
    pub y0: u32,
    pub breite: u32,
    pub hoehe: u32,
    pub deckkraft: f32,
    /// Release 15.10.2026: the rounded mask (`bereich` and its radii), if
    /// any corner is rounded.
    pub rund: Option<(Rect, crate::formen::Corners)>,
    /// Release 15.10.2026 (Agent F): a coverage mask (`clip-path`).
    pub maske: Option<narsil_display::Maske>,
    pub alt: Vec<Color>,
}

/// Merkt den Bereich vor einer Gruppe.
pub(crate) fn ebene_beginnen(fb: &Framebuffer, bereich: Rect, bis: usize, deckkraft: f32, radius: crate::formen::Corners) -> Ebene {
    let x0 = bereich.x.floor().max(0.0) as u32;
    let y0 = bereich.y.floor().max(0.0) as u32;
    let x1 = (bereich.right().ceil().max(0.0) as u32).min(fb.width());
    let y1 = (bereich.bottom().ceil().max(0.0) as u32).min(fb.height());
    let breite = x1.saturating_sub(x0);
    let hoehe = y1.saturating_sub(y0);
    let mut alt = Vec::with_capacity((breite * hoehe) as usize);
    for y in y0..y0 + hoehe {
        for x in x0..x0 + breite {
            alt.push(fb.pixel(x, y).unwrap_or(Color::TRANSPARENT));
        }
    }
    // Release 15.10.2026 (Agent F): elliptical corners are fitted here; a
    // circular mask keeps its radii as given.
    let rund = radius.is_rounded().then(|| (bereich, if radius.is_circular() { radius } else { radius.fit(bereich.width, bereich.height) }));
    Ebene { bis, x0, y0, breite, hoehe, deckkraft: deckkraft.clamp(0.0, 1.0), rund, maske: None, alt }
}

/// Release 15.10.2026 (Agent F): filters the group painted alone in
/// `group` over `area` and composites it (source-over) onto `fb`.
/// `view` is the visible area (the page's viewport within the window): the
/// filter sees the content beyond it as continuing with its edge.
pub(crate) fn composite_filtered(fb: &mut Framebuffer, group: &Framebuffer, area: Rect, spec: &narsil_display::FilterSpec, view: Option<Rect>) {
    let (bw, bh) = (fb.width().min(group.width()), fb.height().min(group.height()));
    if bw == 0 || bh == 0 {
        return;
    }
    // The visible area in buffer pixels: (sx0, sy0) .. (fw, fh).
    let (sx0, sy0, fw, fh) = match view {
        Some(s) => (
            (s.x.max(0.0).floor() as u32).min(bw),
            (s.y.max(0.0).floor() as u32).min(bh),
            (s.right().max(0.0).ceil() as u32).min(bw),
            (s.bottom().max(0.0).ceil() as u32).min(bh),
        ),
        None => (0, 0, bw, bh),
    };
    if fw <= sx0 || fh <= sy0 {
        return;
    }
    // The filter sees content beyond the viewport, too: a margin of the
    // filter's reach past each edge, filled with the edge pixels (the
    // content usually continues there; WPT fixed-pos-filter-clip-*).
    const MAX_PAD: f32 = 256.0;
    let pad_l = (sx0 as f32 - area.x).clamp(0.0, MAX_PAD).ceil() as i64;
    let pad_t = (sy0 as f32 - area.y).clamp(0.0, MAX_PAD).ceil() as i64;
    let pad_r = (area.right() - fw as f32).clamp(0.0, MAX_PAD).ceil() as i64;
    let pad_b = (area.bottom() - fh as f32).clamp(0.0, MAX_PAD).ceil() as i64;
    let x0 = i64::from(area.x.floor().max(sx0 as f32) as u32).min(i64::from(fw)) - pad_l;
    let y0 = i64::from(area.y.floor().max(sy0 as f32) as u32).min(i64::from(fh)) - pad_t;
    let x1 = i64::from((area.right().ceil().max(sx0 as f32) as u32).min(fw)) + pad_r;
    let y1 = i64::from((area.bottom().ceil().max(sy0 as f32) as u32).min(fh)) + pad_b;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
    if u64::from(w) * u64::from(h) > 64 * 1024 * 1024 {
        return;
    }
    // Premultiplied RGBA of the area. Outside the visible area: its edge
    // where the box itself reaches past that edge, transparent elsewhere
    // (a box that ends at the edge ends there; WPT filters-drop-shadow-001).
    let k = spec.border_box;
    let extends = [k.x < sx0 as f32, k.y < sy0 as f32, k.right() > fw as f32, k.bottom() > fh as f32];
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let yy = y0 + i64::from(y);
        if (yy < i64::from(sy0) && !extends[1]) || (yy >= i64::from(fh) && !extends[3]) {
            continue;
        }
        let sy = yy.clamp(i64::from(sy0), i64::from(fh) - 1) as u32;
        for x in 0..w {
            let xx = x0 + i64::from(x);
            if (xx < i64::from(sx0) && !extends[0]) || (xx >= i64::from(fw) && !extends[2]) {
                continue;
            }
            let sx = xx.clamp(i64::from(sx0), i64::from(fw) - 1) as u32;
            let Some(c) = group.pixel(sx, sy) else { continue };
            let o = ((y * w + x) * 4) as usize;
            let a = u32::from(c.a);
            data[o] = ((u32::from(c.r) * a + 127) / 255) as u8;
            data[o + 1] = ((u32::from(c.g) * a + 127) / 255) as u8;
            data[o + 2] = ((u32::from(c.b) * a + 127) / 255) as u8;
            data[o + 3] = c.a;
        }
    }
    let (ox, oy) = (x0 as f32, y0 as f32);
    let bbox = (f64::from(k.x - ox), f64::from(k.y - oy), f64::from(k.x - ox + k.width), f64::from(k.y - oy + k.height));
    match narsil_svg::filter_box(&spec.value, spec.svg.as_deref(), &mut data, (w, h), bbox, spec.current_color) {
        // Not rendered: the area stays as it was before the group.
        narsil_svg::BoxFilter::Hide => return,
        narsil_svg::BoxFilter::Applied | narsil_svg::BoxFilter::Ignore => {}
    }
    for y in 0..h {
        let py = y0 + i64::from(y);
        if py < i64::from(sy0) || py >= i64::from(fh) {
            continue;
        }
        for x in 0..w {
            let px = x0 + i64::from(x);
            if px < i64::from(sx0) || px >= i64::from(fw) {
                continue;
            }
            let o = ((y * w + x) * 4) as usize;
            let sa = u32::from(data[o + 3]);
            if sa == 0 {
                continue;
            }
            // The ancestors' clip bounds the filtered result.
            if let Some(c) = spec.clip {
                let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
                if cx < c.x || cy < c.y || cx > c.right() || cy > c.bottom() {
                    continue;
                }
            }
            let (px, py) = (px as u32, py as u32);
            let Some(d) = fb.pixel(px, py) else { continue };
            // Source-over with a premultiplied source onto a straight
            // destination.
            let da = u32::from(d.a);
            let oa = sa * 255 + da * (255 - sa);
            if oa == 0 {
                continue;
            }
            let channel = |s: u8, dc: u8| -> u8 { ((u32::from(s) * 255 * 255 + u32::from(dc) * da * (255 - sa) + oa / 2) / oa).min(255) as u8 };
            let c = Color::new(channel(data[o], d.r), channel(data[o + 1], d.g), channel(data[o + 2], d.b), ((oa + 127) / 255).min(255) as u8);
            fb.set_pixel(px, py, c);
        }
    }
}

/// Release 15.10.2026 (Agent F): `backdrop-filter`: filters the pixels of
/// `source` (or of `fb` itself) inside the border box of `spec` and
/// composites the result (source-over) onto `fb`, inside the rounded
/// border box and the clip of the box's ancestors. The filter samples
/// beyond the border box as its mirror image (Filter Effects 2 §3.1).
pub(crate) fn composite_backdrop(fb: &mut Framebuffer, source: Option<&Framebuffer>, spec: &narsil_display::FilterSpec, radius: crate::formen::Corners) {
    let (bw, bh) = (fb.width(), fb.height());
    let b = spec.border_box;
    // The border box in whole pixels, inside the buffer and the clip.
    let mut visible = Rect::new(0.0, 0.0, bw as f32, bh as f32);
    if let Some(c) = spec.clip {
        visible = visible.intersect(c).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
    }
    let Some(r) = b.intersect(visible) else { return };
    let (x0, y0) = (r.x.floor().max(0.0) as i64, r.y.floor().max(0.0) as i64);
    let (x1, y1) = ((r.right().ceil() as i64).min(i64::from(bw)), (r.bottom().ceil() as i64).min(i64::from(bh)));
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let pad = (narsil_svg::filter_reach(&spec.value, (f64::from(b.width), f64::from(b.height))).min(256.0)) as i64;
    let (dw, dh) = (w + 2 * pad, h + 2 * pad);
    if dw * dh > 64 * 1024 * 1024 {
        return;
    }
    // Mirror an offset into 0..n (… 2 1 0 | 0 1 2 … n-1 | n-1 n-2 …).
    let mirror = |v: i64, n: i64| -> i64 {
        let p = 2 * n;
        let m = v.rem_euclid(p);
        if m < n { m } else { p - 1 - m }
    };
    let src = source.unwrap_or(&*fb);
    let mut data = vec![0u8; (dw * dh * 4) as usize];
    for y in 0..dh {
        let sy = (y0 + mirror(y - pad, h)) as u32;
        for x in 0..dw {
            let sx = (x0 + mirror(x - pad, w)) as u32;
            let Some(c) = src.pixel(sx, sy) else { continue };
            let o = ((y * dw + x) * 4) as usize;
            let a = u32::from(c.a);
            data[o] = ((u32::from(c.r) * a + 127) / 255) as u8;
            data[o + 1] = ((u32::from(c.g) * a + 127) / 255) as u8;
            data[o + 2] = ((u32::from(c.b) * a + 127) / 255) as u8;
            data[o + 3] = c.a;
        }
    }
    let (ox, oy) = ((x0 - pad) as f64, (y0 - pad) as f64);
    let bbox = (f64::from(b.x) - ox, f64::from(b.y) - oy, f64::from(b.right()) - ox, f64::from(b.bottom()) - oy);
    if narsil_svg::filter_box(&spec.value, spec.svg.as_deref(), &mut data, (dw as u32, dh as u32), bbox, spec.current_color) != narsil_svg::BoxFilter::Applied {
        return;
    }
    let corners = radius.fit(b.width, b.height);
    let rounded = corners.is_rounded();
    for y in 0..h {
        let py = y0 + y;
        for x in 0..w {
            let px = x0 + x;
            if rounded && !crate::formen::point_in_box_xy(px as f32 + 0.5 - b.x, py as f32 + 0.5 - b.y, b.width, b.height, &corners) {
                continue;
            }
            let o = (((y + pad) * dw + x + pad) * 4) as usize;
            let sa = u32::from(data[o + 3]);
            if sa == 0 {
                continue;
            }
            let (px, py) = (px as u32, py as u32);
            let Some(d) = fb.pixel(px, py) else { continue };
            let da = u32::from(d.a);
            let oa = sa * 255 + da * (255 - sa);
            if oa == 0 {
                continue;
            }
            let channel = |s: u8, dc: u8| -> u8 { ((u32::from(s) * 255 * 255 + u32::from(dc) * da * (255 - sa) + oa / 2) / oa).min(255) as u8 };
            let c = Color::new(channel(data[o], d.r), channel(data[o + 1], d.g), channel(data[o + 2], d.b), ((oa + 127) / 255).min(255) as u8);
            fb.set_pixel(px, py, c);
        }
    }
}

/// Release 15.10.2026 (Agent F): a layer for item `index` when it lies in
/// a rounded `overflow` clip and touches its corners (see
/// `DisplayList::rounded_clip_at`); ending it (`ebene_beenden`) masks
/// what the item painted to the rounded shape. Items away from the corners
/// need no layer (the rectangular clip is enough).
pub(crate) fn rounded_clip_layer(fb: &Framebuffer, list: &narsil_display::DisplayList, index: usize) -> Option<Ebene> {
    let rc = list.rounded_clip_at(index)?;
    let r = rc.rect;
    let corners = crate::formen::Corners { x: rc.radius_x, y: rc.radius_y }.fit(r.width, r.height);
    if !corners.is_rounded() {
        return None;
    }
    let area = match list.item_bounds_at(index) {
        Some(b) => {
            let zones = [
                Rect::new(r.x, r.y, corners.x[0], corners.y[0]),
                Rect::new(r.right() - corners.x[1], r.y, corners.x[1], corners.y[1]),
                Rect::new(r.right() - corners.x[2], r.bottom() - corners.y[2], corners.x[2], corners.y[2]),
                Rect::new(r.x, r.bottom() - corners.y[3], corners.x[3], corners.y[3]),
            ];
            // Shadows reach beyond the item: some room around it.
            let b = Rect::new(b.x - 64.0, b.y - 64.0, b.width + 128.0, b.height + 128.0);
            if !zones.iter().any(|z| z.width > 0.0 && z.height > 0.0 && z.intersect(b).is_some()) {
                return None;
            }
            b.intersect(r)?
        }
        None => r,
    };
    let mut e = ebene_beginnen(fb, area, index + 1, 1.0, crate::formen::Corners::default());
    e.rund = Some((r, corners));
    Some(e)
}

/// Mischt die Gruppe mit ihrer Deckkraft ueber den gemerkten Bereich.
pub(crate) fn ebene_beenden(fb: &mut Framebuffer, e: Ebene) {
    if e.deckkraft >= 1.0 && e.rund.is_none() && e.maske.is_none() {
        return;
    }
    let k = |a: u8, b: u8, d: f32| (f32::from(a) + (f32::from(b) - f32::from(a)) * d).round() as u8;
    // Der Rasterer ruft das zwischen zwei Items, also ohne Item-Clip: die
    // Gruppe hat ihre Clips schon beachtet, `alt` ist der Zustand davor.
    for y in 0..e.hoehe {
        for x in 0..e.breite {
            let alt = e.alt[(y * e.breite + x) as usize];
            let (px, py) = (e.x0 + x, e.y0 + y);
            let Some(neu) = fb.pixel(px, py) else { continue };
            if neu == alt {
                continue;
            }
            // Release 15.10.2026: outside the rounded corners the group does
            // not paint (CSS Backgrounds 3 §5.3, the image of a replaced
            // element is clipped by the curve, too); one pixel of soft edge.
            let d = match e.rund {
                Some((r, radius)) => {
                    let a = crate::formen::distance_xy(px as f32 + 0.5, py as f32 + 0.5, r, &radius);
                    e.deckkraft * (0.5 - a).clamp(0.0, 1.0)
                }
                None => e.deckkraft,
            };
            let d = match &e.maske {
                Some(m) => d * m.bei(px as f32 + 0.5, py as f32 + 0.5),
                None => d,
            };
            if d >= 1.0 {
                continue;
            }
            fb.set_pixel(px, py, Color::new(k(alt.r, neu.r, d), k(alt.g, neu.g, d), k(alt.b, neu.b, d), k(alt.a, neu.a, d)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_abstand_ist_null_auf_der_kante_und_negativ_innen() {
        let r = Rect::new(10.0, 10.0, 100.0, 50.0);
        assert!(abstand(60.0, 35.0, r, [8.0; 4]) < -20.0);
        assert!(abstand(10.0, 35.0, r, [8.0; 4]).abs() < 1e-4);
        assert!(abstand(5.0, 35.0, r, [8.0; 4]) > 4.9);
        // In der Ecke zaehlt der Radius: der Eckpunkt liegt ausserhalb.
        assert!(abstand(10.5, 10.5, r, [8.0; 4]) > 1.0);
        // Je Ecke: unten eckig, oben rund (aktiver Tab).
        assert!(abstand(10.5, 59.5, r, [8.0, 8.0, 0.0, 0.0]) < 0.0);
        assert!(abstand(10.5, 10.5, r, [8.0, 8.0, 0.0, 0.0]) > 1.0);
    }

    #[test]
    fn erfc_stimmt() {
        assert!((erfc(0.0) - 1.0).abs() < 1e-5);
        assert!(erfc(3.0) < 1e-4);
        assert!((erfc(-3.0) - 2.0).abs() < 1e-4);
    }
}
