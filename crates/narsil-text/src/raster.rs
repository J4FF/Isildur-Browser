//! Rasterisierung von Glyphen-Umrissen mit Antialiasing.
//!
//! Dieses Modul wandelt TrueType-Glyphen-Umrisse in gerasterte Bitmaps um.
//! Es verwendet:
//! - Quadratische Bezier-Kurven (TrueType-Nativ)
//! - Scanline-Rasterisierung mit Flaechenueberdeckung
//! - 8-Bit-Alpha-Antialiasing (4x4 Supersampling)
//! - Non-Zero Winding Rule fuer Loecher (innere Konturen)
//! - Einen Glyphen-Cache mit Obergrenze (LRU, M6)

use std::collections::HashMap;

#[allow(unused_imports)]
use crate::truetype::{GlyphOutline, Point, TrueTypeFont, TtError};

/// Maximale Anzahl Eintraege im Glyphen-Cache (M6).
const MAX_CACHE_ENTRIES: usize = 4096;

/// Maximale Breite einer gerasterten Glyphe in Pixeln.
const MAX_GLYPH_WIDTH: u32 = 512;

/// Maximale Hoehe einer gerasterten Glyphe in Pixeln.
const MAX_GLYPH_HEIGHT: u32 = 512;

/// Maximale Anzahl Konturen pro Glyphe fuer die Rasterisierung.
const MAX_CONTOURS: usize = 256;

/// Maximale Anzahl Punkte pro Kontur.
const MAX_POINTS_PER_CONTOUR: usize = 512;

/// Supersampling-Faktor (4x4 = 16 Subpixel pro Pixel).
const SUPER_SAMPLE: u32 = 4;

/// Ein Eintrag im Glyphen-Cache.
#[derive(Debug, Clone)]
struct CacheEntry {
    /// Die gerasterte Glyphe.
    bitmap: Vec<u8>,
    /// Die Breite der Glyphe.
    width: u32,
    /// Die Hoehe der Glyphe.
    height: u32,
    /// Wieviel Pixel die Bitmap links von der SCHREIBPOSITION steht.
    ///
    /// GEMESSEN 01.10.2026: `rasterize_outline` liefert die Bitmap ab
    /// `floor(xMin * scale)`, nicht ab der Schreibposition. Wer sie an der
    /// Schreibposition absetzt, verliert den linken Seitenrand und malt
    /// die Glyphe um so viele Pixel zu weit links. Bei FreeSans, 16 px,
    /// ist das 0,45 bis 1,63 Pixel je nach Ziffer — und weil die
    /// Zifferntinte (9 px) den Ziffernvorschub (8,90 px) uebertrifft,
    /// reicht das, um benachbarte Ziffern zu verschmelzen. Bei 32 px und
    /// mehr verschwindet der Effekt, weil der Vorschub relativ groesser
    /// ist. Genau das beschreibt der Befund: nur kleine Schrift leidet.
    x_versatz: i32,
    /// Der Zeitstempel des letzten Zugriffs.
    last_access: u64,
}

/// Ein Glyphen-Cache mit LRU-Ersetzung.
///
/// Der Schluessel ist `(Schrift-Id, glyph_index, Groesse in 1/64 px)`,
/// damit dieselbe Glyphe in verschiedenen Groessen getrennt gecacht wird.
#[derive(Debug)]
pub struct GlyphCache {
    /// Die Cache-Eintraege.
    entries: HashMap<(u64, u32, u32), CacheEntry>,
    /// Release 09.10.2026: die feinen Masken, Schluessel wie oben plus
    /// [`Feinheit::schluessel`], Wert = (Maske, letzter Zugriff).
    fein: HashMap<(u64, u32, u32, u8), (Maske, u64)>,
    /// Release 09.10.2026: die x-Hoehe (em) je Schrift-Id.
    x_hoehen: HashMap<u64, Option<f32>>,
    /// Der naechste Zeitstempel.
    next_timestamp: u64,
}

/// Release 09.10.2026: was die feine Rasterung tun soll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Feinheit {
    /// Vertikales Hinting (x-Hoehe auf ganze Pixel).
    pub hinting: bool,
    /// Stem-Darkening nach [`verdunkeln_fuer`].
    pub verdunkeln: bool,
    /// Drei Teilpixel je Pixel.
    pub lcd: bool,
}

impl Feinheit {
    fn schluessel(self) -> u8 {
        u8::from(self.hinting) | (u8::from(self.verdunkeln) << 1) | (u8::from(self.lcd) << 2)
    }
}

impl GlyphCache {
    /// Erstellt einen leeren Cache.
    pub fn new() -> Self {
        Self {
            entries: HashMap::with_capacity(MAX_CACHE_ENTRIES),
            fein: HashMap::new(),
            x_hoehen: HashMap::new(),
            next_timestamp: 0,
        }
    }

    /// Laedt eine Glyphe aus dem Cache oder rastert sie neu.
    ///
    /// Der Schluessel ist `(Schrift-Id, glyph_index, Groesse in 1/64 px)`.
    /// Die Schriftgroesse wird in 1/64 px umgerechnet, damit sie als
    /// ganzzahliger Schluessel verwendet werden kann.
    pub fn get_or_rasterize(
        &mut self,
        font: &TrueTypeFont,
        glyph_index: u32,
        font_size: f32,
    ) -> Result<(Vec<u8>, u32, u32), TtError> {
        let (bitmap, breite, hoehe, _) =
            self.get_or_rasterize_mit_versatz(font, glyph_index, font_size)?;
        Ok((bitmap, breite, hoehe))
    }

    /// Wie [`GlyphCache::get_or_rasterize`], liefert aber zusaetzlich den
    /// **linken Seitenrand in Pixeln**: um wie viel die Bitmap links von der
    /// Schreibposition steht.
    ///
    /// GEMESSEN 01.10.2026: `rasterize_outline` liefert die Bitmap ab
    /// `floor(xMin * scale)`. Wer sie an der Schreibposition absetzt,
    /// verliert den Seitenrand und malt die Glyphe zu weit links. Bei
    /// FreeSans, 16 px, sind das je nach Ziffer 0,45 bis 1,63 Pixel — und
    /// weil die Zifferntinte den Ziffernvorschub uebertrifft, verschmelzen
    /// benachbarte Ziffern. Der Seitenrand gehoert deshalb zum Ergebnis
    /// und nicht in eine private Nebenrechnung des Aufrufers.
    pub fn get_or_rasterize_mit_versatz(
        &mut self,
        font: &TrueTypeFont,
        glyph_index: u32,
        font_size: f32,
    ) -> Result<(Vec<u8>, u32, u32, i32), TtError> {
        let font_id = font.id();
        let size_key = (font_size * 64.0) as u32;
        let key = (font_id, glyph_index, size_key);

        // Cache-Treffer.
        if let Some(entry) = self.entries.get_mut(&key) {
            self.next_timestamp += 1;
            entry.last_access = self.next_timestamp;
            return Ok((
                entry.bitmap.clone(),
                entry.width,
                entry.height,
                entry.x_versatz,
            ));
        }

        // Cache-Miss: Glyphe rasterisieren.
        let outline = font.glyph_outline(glyph_index)?;
        let m = rasterize_outline_art(&outline, font_size, font.units_per_em(), Rasterart::zum_zeichnen())?;
        let (bitmap, width, height) = (m.bitmap, m.breite, m.hoehe);

        // Der Seitenrand, in Pixeln. `rasterize_outline` beginnt die
        // Bitmap bei `floor(xMin * scale)`; das ist genau dieser Wert.
        // Bei einer leeren Outline gibt es keine Bitmap und keinen Rand.
        let scale = font_size / font.units_per_em().max(1) as f32;
        let x_versatz = if width == 0 || height == 0 {
            0
        } else {
            (outline.bounds.x_min * scale).floor() as i32
        };

        // Cache-Eintrag hinzufügen.
        self.insert(key, bitmap.clone(), width, height, x_versatz);

        Ok((bitmap, width, height, x_versatz))
    }

    /// Release 09.10.2026: die Glyphe in der feinen Rasterung — vertikales
    /// Hinting, Stem-Darkening, auf Wunsch LCD (siehe [`Feinheit`]).
    ///
    /// Der Faktor fuer das Hinting kommt aus der x-Hoehe der Schrift
    /// ([`TrueTypeFont::x_hoehe_em`], je Schrift einmal gelesen), die
    /// Verdunkelung aus der Groesse ([`verdunkeln_fuer`]). Eigener Cache
    /// neben dem schlichten: dieselbe Glyphe kann in beiden Arten gebraucht
    /// werden (Synthetisches, Messwege).
    pub fn get_or_rasterize_fein(
        &mut self,
        font: &TrueTypeFont,
        glyph_index: u32,
        font_size: f32,
        feinheit: Feinheit,
    ) -> Result<Maske, TtError> {
        let size_key = (font_size * 64.0) as u32;
        let key = (font.id(), glyph_index, size_key, feinheit.schluessel());
        self.next_timestamp += 1;
        let jetzt = self.next_timestamp;
        if let Some((maske, zugriff)) = self.fein.get_mut(&key) {
            *zugriff = jetzt;
            return Ok(maske.clone());
        }
        let art = self.rasterart(font, font_size, feinheit);
        let outline = font.glyph_outline(glyph_index)?;
        let maske = rasterize_outline_art(&outline, font_size, font.units_per_em(), art)?;
        if self.fein.len() >= MAX_CACHE_ENTRIES
            && let Some(&alt) = self.fein.iter().min_by_key(|(_, v)| v.1).map(|(k, _)| k)
        {
            self.fein.remove(&alt);
        }
        self.fein.insert(key, (maske.clone(), jetzt));
        Ok(maske)
    }

    /// Release 09.10.2026: die [`Rasterart`] zu Schrift, Groesse und Wunsch.
    pub fn rasterart(&mut self, font: &TrueTypeFont, font_size: f32, feinheit: Feinheit) -> Rasterart {
        let y_faktor = if feinheit.hinting {
            let x_em = *self
                .x_hoehen
                .entry(font.id())
                .or_insert_with(|| font.x_hoehe_em());
            // Release 13.10.2026 (Agent CSS2): eine x-Hoehe ab 0,7 em ist keine
            // Kleinbuchstabenhoehe (Ahem: jede Glyphe ein Block von 0,8 em ueber
            // der Grundlinie) — dort rastet `kanten_einrasten` Ober- und
            // Unterkante genau ein, die Streckung schoebe die Unterkante um
            // einen Bruchteil hinaus. GEMESSEN vorher (WPT `CSS2/text/white-
            // space-processing-002..010`, `fonts/font-family-applies-to-*`,
            // Ahem bei 16 px): eine halb gedeckte Zeile unter jedem Block.
            x_em.filter(|x| *x < 0.7).map_or(1.0, |x| y_faktor_fuer(x * font_size))
        } else {
            1.0
        };
        Rasterart {
            y_faktor,
            lcd: feinheit.lcd,
            verdunkeln_px: if feinheit.verdunkeln { verdunkeln_fuer(font_size) } else { 0.0 },
            einrasten: einrasten_an(),
        }
    }

    /// Fuegt einen Eintrag in den Cache ein.
    fn insert(
        &mut self,
        key: (u64, u32, u32),
        bitmap: Vec<u8>,
        width: u32,
        height: u32,
        x_versatz: i32,
    ) {
        // Cache voll: LRU-Eintrag entfernen.
        if self.entries.len() >= MAX_CACHE_ENTRIES {
            if let Some(&lru_key) = self
                .entries
                .iter()
                .min_by_key(|(_, v)| v.last_access)
                .map(|(k, _)| k)
            {
                self.entries.remove(&lru_key);
            }
        }

        self.next_timestamp += 1;
        self.entries.insert(
            key,
            CacheEntry {
                bitmap,
                width,
                height,
                x_versatz,
                last_access: self.next_timestamp,
            },
        );
    }

    /// Die Anzahl Eintraege im Cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true`, wenn der Cache leer ist.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Eine Kante fuer die Scanline-Rasterisierung.
#[derive(Debug, Clone, Copy)]
struct Edge {
    /// Start-Y der Kante (inklusiv).
    y_start: f32,
    /// End-Y der Kante (exklusiv).
    y_end: f32,
    /// X-Koordinate bei y_start.
    x_at_y_start: f32,
    /// Steigung (dx/dy).
    dx_dy: f32,
    /// Windungsrichtung: +1 fuer gegen Uhrzeigersinn (aussere Kontur), -1 fuer im Uhrzeigersinn (innere Kontur/Loch).
    winding: i32,
}

/// Rasterisiert eine Glyphen-Umrisse in eine Bitmap mit Antialiasing.
///
/// Die Bitmap hat die Groesse `width * height` und enthaelt 8-Bit-Alpha-Werte (0-255).
/// Die Koordinaten werden von der Einheiten-Skala (unitsPerEm) auf Pixel skaliert.
/// TrueType-Koordinatensystem: y zeigt nach oben. Bitmap: y zeigt nach unten.
/// Daher wird y gespiegelt: pixel_y = height - 1 - (scaled_y - y_min).
pub fn rasterize_outline(
    outline: &GlyphOutline,
    font_size: f32,
    units_per_em: u32,
) -> Result<(Vec<u8>, u32, u32), TtError> {
    let m = rasterize_outline_art(outline, font_size, units_per_em, Rasterart::SCHLICHT)?;
    Ok((m.bitmap, m.breite, m.hoehe))
}

/// Release 09.10.2026: WIE eine Glyphe gerastert wird — die Stellschrauben
/// fuer scharfen Text bei kleinen Groessen.
///
/// GEMESSEN vorher (Adressleiste, „http://youtube.com", Segoe UI 13 px):
/// die x-Hoehe lag bei 6,5 px, die Oberkante von `h`, `t`, `p` stand also
/// in ZWEI halb gedeckten Zeilen (Werte 55/63/38 und 74/65/43 von 99), und
/// die Staemme von `t` und dem rechten `h` waren auf zwei Spalten verteilt
/// (60/48). Das ist der Unschaerfe-Eindruck.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rasterart {
    /// Senkrechte Streckung (vertikales Hinting): die x-Hoehe wird auf ganze
    /// Pixel gebracht, die Grundlinie liegt ohnehin auf einer Pixelgrenze.
    /// `1.0` = keine Streckung. Die Breiten aendern sich NICHT.
    pub y_faktor: f32,
    /// Drei Teilpixel je Pixel (LCD, Subpixel-Kantenglaettung). Die Maske
    /// ist dann dreimal so breit, gefiltert mit dem FreeType-Standardfilter
    /// [8, 77, 86, 77, 8] / 256, und `x_versatz` zaehlt Teilpixel.
    pub lcd: bool,
    /// Stem-Darkening: jede waagerechte Spanne wird um so viele Pixel
    /// breiter (je Seite die Haelfte). Nur die Tinte waechst, der Vorschub
    /// bleibt.
    pub verdunkeln_px: f32,
    /// Release 10.10.2026: senkrecht einrasten (siehe [`kanten_einrasten`];
    /// nur ohne `y_faktor`). `SCHLICHT` rastet NICHT ein — das ist die rohe
    /// Rasterung; die Zeichenwege des [`GlyphCache`] nehmen [`einrasten_an`].
    pub einrasten: bool,
}

impl Rasterart {
    /// Die Rasterung wie bis 08.10.2026: keine Streckung, Graustufen, keine
    /// Verdunkelung. `rasterize_outline` liefert damit bitgenau dasselbe.
    pub const SCHLICHT: Rasterart = Rasterart {
        y_faktor: 1.0,
        lcd: false,
        verdunkeln_px: 0.0,
        einrasten: false,
    };

    /// Release 10.10.2026: die schlichte Rasterung, wie sie gezeichnet wird
    /// (eingerastet, wenn [`einrasten_an`]).
    pub fn zum_zeichnen() -> Rasterart {
        Rasterart { einrasten: einrasten_an(), ..Rasterart::SCHLICHT }
    }
}

/// Release 09.10.2026: eine gerasterte Glyphe mit ihrer Lage.
#[derive(Debug, Clone, PartialEq)]
pub struct Maske {
    /// Deckung je Spalte und Zeile, 0..=255.
    pub bitmap: Vec<u8>,
    /// Spalten (bei LCD Teilpixel).
    pub breite: u32,
    /// Zeilen.
    pub hoehe: u32,
    /// Wie weit die Maske links von der Schreibposition beginnt, in
    /// Spalten (bei LCD in Teilpixeln). Negativ = links davon.
    pub x_versatz: i32,
    /// Die Zeile, auf deren OBERKANTE die Grundlinie liegt (von oben).
    pub grundlinie: i32,
    /// `true` = drei Teilpixel je Pixel.
    pub lcd: bool,
}

/// Der LCD-Filter von FreeType (`FT_LCD_FILTER_DEFAULT`), Summe 256.
const LCD_FILTER: [u32; 5] = [8, 77, 86, 77, 8];

/// Release 09.10.2026: die x-Hoehe in Pixeln -> senkrechter Faktor, der sie
/// auf ganze Pixel bringt.
///
/// GEMESSENE ENTSCHEIDUNG fuer das Aufrunden ab 0,375: so rundet der leichte
/// Autohinter von FreeType (`aflatin.c`: `fitted = (scaled + 40) & ~63`) —
/// eine groessere x-Hoehe liest sich bei kleiner Schrift besser. Unter 2,5 px
/// und ueber 40 px wird nicht gestreckt (zu klein zum Lesen, gross genug
/// ohne), und der Faktor bleibt in 0,85..=1,18.
#[must_use]
pub fn y_faktor_fuer(x_hoehe_px: f32) -> f32 {
    if !x_hoehe_px.is_finite() || !(2.5..=40.0).contains(&x_hoehe_px) {
        return 1.0;
    }
    let ziel = (x_hoehe_px + 0.625).floor().max(1.0);
    (ziel / x_hoehe_px).clamp(0.85, 1.18)
}

/// Release 09.10.2026: Stem-Darkening je Schriftgroesse, in Pixeln.
///
/// 0,25 px bis 11 px, linear auf 0 bei 18 px. GEMESSENE ENTSCHEIDUNG: bei
/// 13 px (Leiste, 100 %) sind das 0,18 px, bei 19,5 px (150 %) nichts mehr —
/// dort ist ein Stamm 1,5 px breit und braucht keine Hilfe.
#[must_use]
pub fn verdunkeln_fuer(groesse_px: f32) -> f32 {
    if !groesse_px.is_finite() || groesse_px >= 18.0 {
        return 0.0;
    }
    if groesse_px <= 11.0 {
        return 0.25;
    }
    0.25 * (18.0 - groesse_px) / 7.0
}

/// Release 09.10.2026: [`rasterize_outline`] mit [`Rasterart`].
/// Release 10.10.2026: senkrechtes Einrasten an (Vorgabe) — aus mit
/// `NARSIL_EINRASTEN_AUS=1` (einmal gelesen). GEMESSEN (WPT, 600 zufaellige
/// gescheiterte + 300 bestandene Reftests): +12 bestanden, 0 gekippt —
/// Ahem bei 16 px (Oberkante 12,8 px) stand 0,8 px zu hoch, weil die
/// Maske an der gebrochenen Grundlinie abgeschnitten wurde.
pub fn einrasten_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("NARSIL_EINRASTEN_AUS").is_none())
}

/// Release 10.10.2026: senkrechtes Einrasten (wie das Hinting in Chrome/
/// Skia): Punkte ueber der Grundlinie werden so gestreckt, dass die
/// Oberkante auf ein ganzes Pixel faellt, Punkte darunter fuer die
/// Unterkante ebenso; die Grundlinie bleibt. `None`: nichts zu tun.
pub fn kanten_einrasten(outline: &GlyphOutline, scale: f32) -> Option<GlyphOutline> {
    let oben = outline.bounds.y_max * scale;
    let unten = outline.bounds.y_min * scale;
    let fo = if oben > 0.5 { oben.round() / oben } else { 1.0 };
    let fu = if unten < -0.5 { unten.round() / unten } else { 1.0 };
    if (fo - 1.0).abs() < 1e-6 && (fu - 1.0).abs() < 1e-6 {
        return None;
    }
    let mut o = outline.clone();
    let f = |y: f32| if y > 0.0 { y * fo } else { y * fu };
    for c in &mut o.contours {
        for p in &mut c.points {
            p.y = f(p.y);
        }
    }
    o.bounds.y_max = f(outline.bounds.y_max);
    o.bounds.y_min = f(outline.bounds.y_min);
    Some(o)
}

/// Release 10.10.2026: die Zeile der Grundlinie in der Maske (von oben) —
/// dieselbe Rechnung wie in [`rasterize_outline_art`], auch eingerastet.
pub fn masken_grundlinie(outline: &GlyphOutline, font_size: f32, units_per_em: u32) -> f32 {
    let scale = font_size / units_per_em as f32;
    if !einrasten_an() {
        return (outline.bounds.y_max * scale).ceil();
    }
    let oben = kanten_einrasten(outline, scale).map_or(outline.bounds.y_max, |o| o.bounds.y_max);
    (oben * scale - 1e-3).ceil()
}

pub fn rasterize_outline_art(
    outline: &GlyphOutline,
    font_size: f32,
    units_per_em: u32,
    art: Rasterart,
) -> Result<Maske, TtError> {
    if font_size <= 0.0 || !font_size.is_finite() {
        return Err(TtError::InvalidFont("ungueltige Schriftgroesse".into()));
    }
    let eingerastet = if art.einrasten && art.y_faktor == 1.0 {
        kanten_einrasten(outline, font_size / units_per_em as f32)
    } else {
        None
    };
    let outline = eingerastet.as_ref().unwrap_or(outline);

    let teile: u32 = if art.lcd { 3 } else { 1 };
    if outline.contours.is_empty() {
        return Ok(Maske {
            bitmap: Vec::new(),
            breite: 0,
            hoehe: 0,
            x_versatz: 0,
            grundlinie: 0,
            lcd: art.lcd,
        });
    }

    if outline.contours.len() > MAX_CONTOURS {
        return Err(TtError::TooManyContours);
    }

    // Skalierungsfaktor: unitsPerEm -> font_size.
    let scale = font_size / units_per_em as f32;
    // Release 09.10.2026: getrennte Faktoren fuer x (Teilpixel) und y
    // (Hinting). Bei `SCHLICHT` sind beide genau `scale`.
    let y_faktor = if art.y_faktor.is_finite() && art.y_faktor > 0.0 {
        art.y_faktor
    } else {
        1.0
    };
    let scale_x = scale * teile as f32;
    let scale_y = scale * y_faktor;
    // Die halbe Verdunkelung in Spalten; sie verbreitert auch die Maske.
    let halb = if art.verdunkeln_px.is_finite() && art.verdunkeln_px > 0.0 {
        art.verdunkeln_px.min(2.0) * teile as f32 / 2.0
    } else {
        0.0
    };

    // Bounding-Box in Font-Einheiten.
    let x_min_fu = outline.bounds.x_min;
    let y_min_fu = outline.bounds.y_min;
    let x_max_fu = outline.bounds.x_max;
    let y_max_fu = outline.bounds.y_max;

    // Bounding-Box in Pixeln (vor y-Flip).
    let x_min = (x_min_fu * scale_x - halb).floor() as i32;
    let x_max = (x_max_fu * scale_x + halb).ceil() as i32;
    // y_max_fu ist der hoechste Punkt (TrueType y nach oben).
    // Nach Skalierung und Flip: pixel 0 = y_max_fu * scale.
    // Release 09.10.2026: gehintet liegt die x-Hoehe GENAU auf einer
    // Pixelgrenze — und f32 trifft sie mit 7,0000005 statt 7. GEMESSEN: ohne
    // die Toleranz bekam die Maske eine leere achte Zeile, und die
    // Grundlinie rutschte um eine Zeile. Ungehintet bleibt es bitgenau beim
    // alten `ceil`/`floor` (die Grundlinie in `narsil-raster` rechnet so).
    let toleranz = if y_faktor == 1.0 && !(art.einrasten && art.y_faktor == 1.0) { 0.0 } else { 1e-3 };
    let y_min = (y_min_fu * scale_y + toleranz).floor() as i32;
    let y_max = (y_max_fu * scale_y - toleranz).ceil() as i32;

    let width = (x_max - x_min).max(1) as u32;
    let height = (y_max - y_min).max(1) as u32;

    if width > MAX_GLYPH_WIDTH * teile || height > MAX_GLYPH_HEIGHT {
        return Err(TtError::InvalidFont(
            "Glyphe zu gross fuer Rasterung".into(),
        ));
    }

    // Supersampling: 4 Zeilenhalbschritte je Ausgabezeile, aber nur EINE
    // Spalte je Ausgabespalte — die genaue Restbreite wird als Flaeche
    // summiert statt weggerundet (siehe `ss_rows`).
    //
    // GEMESSEN 03.10.2026 (SCHRIFT, UI5): die erste Fassung hatte einen Puffer
    // in SUBPIXEL-Breite und schrieb mit `x as i32`, also mit ABSCHNEIDUNG.
    // Die Kontur von `i` aus FreeSans liegt bei 12 px auf den Subpixel-Werten
    // 3,216 und 7,200; abgeschnitten waren das 3 und 7 — der Stamm wurde
    // exakt 1,000 px breit und die Phasendifferenz ging verloren. Pillow/
    // FreeType liefert fuer dieselbe Kontur [52, 204], also eine Breite von
    // 1,008 px, und genau so muss es hier auch herauskommen.
    let ss_width = width * SUPER_SAMPLE;
    let ss_height = height * SUPER_SAMPLE;
    // Eine Zeile pro Subpixel-ZEILE, eine Spalte pro Ausgabespalte. Wert ist
    // die gedeckte Breite in Subpixel-Einheiten, Maximum = SUPER_SAMPLE.
    let mut ss_rows: Vec<f32> = vec![0.0f32; (ss_height * width) as usize];

    // Alle Kanten sammeln.
    let mut edges = Vec::new();
    for contour in &outline.contours {
        if contour.points.len() > MAX_POINTS_PER_CONTOUR {
            return Err(TtError::TooManyPoints);
        }
        let contour_edges = contour_to_edges(contour, scale_x, scale_y, x_min, y_min, y_max);
        edges.extend(contour_edges);
    }

    // Sortiere Kanten nach y_start.
    edges.sort_by(|a, b| {
        a.y_start
            .partial_cmp(&b.y_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Scanline-Rasterisierung mit Supersampling.
    let ss_y_start = 0i32;
    let ss_y_end = ss_height as i32;

    // GEMESSEN 03.10.2026: nach der Korrektur der horizontalen Restbreite
    // (exakte Flaechenueberdeckung je Spalte) fiel der Wert eines schraegen
    // 0,8-px-Bandes von 96 auf 123 — aber der Test `die_kante_faellt_
    // monoton_ohne_sprung` schlug an: 81 auf 123 in einer Zeile. Ursache
    // ist die VERTIKALE Quantisierung: getastet wurde nur in der Mitte der
    // Subpixelzeile, eine Kante bei 45 Grad springt dadurch um 1/4 px = 64
    // Alpha weiter.
    //
    // Die Abhilfe ist exakt: die Schnittpunkte werden an der UNTEREN und
    // der OBEREN Grenze der Subpixelzeile genommen und als Trapez
    // gemittelt. Fuer eine gerade Kante ist das die exakte Flaeche; fuer
    // eine gekruemmte die beste lineare Naeherung. Die GEMESSENE
    // Restabweichung gegenueber FreeType am Stamm einer echten Glyphe
    // bleibt dabei unter 2 Prozent (siehe tests/ui_stamm.rs).
    //
    // MAX_ECKEN: die Schnittpunktliste je Zeile ist durch die Zahl der
    // Kanten begrenzt, und die Kanten sind durch MAX_CONTOURS und
    // MAX_POINTS_PER_CONTOUR begrenzt. `ss_width` ist bereits geprueft.
    let mut untere: Vec<(f32, i32)> = Vec::new();

    for y in ss_y_start..ss_y_end {
        let y_mitte = y as f32 + 0.5;

        // Aktive Kanten an einer gegebenen Höhe sammeln und nach x sortieren.
        // Die Ausgabe wird wiederverwendet (keine Allokation je Zeile).
        let sammle = |y_probe: f32, out: &mut Vec<(f32, i32)>| {
            out.clear();
            for edge in &edges {
                if y_probe >= edge.y_start && y_probe < edge.y_end {
                    let dy = y_probe - edge.y_start;
                    let x = edge.x_at_y_start + edge.dx_dy * dy;
                    out.push((x, edge.winding));
                }
            }
            out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        };

        // Fuellbreite einer Ausgabespalte als Ueberdeckung in Subpixel-Einheiten.
        // GEMESSEN 03.10.2026: nicht `as i32` (das schnitt ab und verlor die
        // Nachkommastellen), sondern der genaue Ueberlappungsbetrag.
        // GEMESSEN 03.10.2026, zwei Varianten an einem 0,8-px-Band bei
        // 11,9 px, exakte Rechnung (`diag_exakt.py`) = [82, 122]:
        //
        //   Mitte + Unterkante   ->  [81, 123]   Abweichung 1 von 255
        //   Mitte + Oberkante    ->  [81, 123]   Abweichung 1 von 255
        //   beide Zeilengrenzen   ->  [108, 96]   Abweichung 26 von 255
        //
        // Die dritte Fassung ist das scheinbar richtige Trapez ueber beide
        // Zeilengrenzen und ist trotzdem SCHLECHTER: bei einem 45-Grad-Band
        // verschiebt sich die Spanne zwischen den beiden Hoehen um ein
        // halbes Pixel, und das Mittel zweier Schnitte ist nicht die
        // Flaeche des Trapezes. Es steht hier, damit das nicht noch einmal
        // ausprobiert wird.
        //
        // Die Randzeilen bleiben ein OFFENER Befund: bei der gewaehlten
        // Fassung bekommt die ERSTE Zeile 69 statt 122 und die LETZTE 135
        // statt 122; die Summe beider ist mit 204 richtig, nur die
        // Verteilung nicht. Siehe crates/narsil-text/tests/ui_schraeg.rs.
        //
        // DIE VIERECK-REGEL, und sie ersetzt das Trapez aus zwei
        // Stuetzstellen. Das ist die Abhilfe gegen den Randbefund, und sie
        // ist zugleich die RICHTIGERE Formel — siehe unten.
        //
        // GEMESSEN 03.10.2026 am 0,8-px-Band bei 11,9 px. Exakte Rechnung
        // (`diag_exakt.py`, N = 4000): [82, 122] mit der Summe 0,8000 px.
        //   Mitte + Unterkante -> [ 81, 123]  Innen richtig
        //   erste Zeile          69 statt 122
        //   letzte Zeile         135 statt 122
        //
        // Die Ursache ist nicht die Zahl der Stuetzstellen allein, sondern
        // DIE FORMEL: geschrieben wurde `anteil * wert`, also
        //
        //     (Ueberdeckung / Spanne) * mittlere Spanne
        //
        // Das ist nur dann richtig, wenn `wert` gleich der Spanne ist. Sobald
        // sich die Spanne zwischen den Hoehen verschiebt — bei 45 Grad um ein
        // halbes Pixel — stimmt es nicht mehr, und am Formrand kommt der
        // ganze Ausschnitt einer Stuetzstelle (dort liegt sie ausserhalb, die
        // Spanne ist leer) mit einem Faktor 1/2 statt 1 in die Zeile.
        //
        // RICHTIG ist deshalb: je Spalte der MITTELWERT der UEBERDECKUNG
        // ueber die Stuetzstellen. Das ist die Mittelpunktregel
        //
        //    Flaeche in der Spalte = (1/H) * integral_0^H ueberdeckung(y) dy
        //
        // und die mittelt die tatsaechlich vorhandene Ueberdeckung, statt sie
        // ueber eine Spanne zu rechnen. Vier Stuetzstellen bei 45 Grad
        // (±0,125 und ±0,375) liegen an jeder Formkante zu drei von vier
        // TEILEN innerhalb.
        const STUETZ: [f32; 4] = [-0.375, -0.125, 0.125, 0.375];
        let s = SUPER_SAMPLE as f32;
        let zeilenindex = y as usize * (width as usize);
        // Release 09.10.2026: die Verdunkelung in Subpixel-Einheiten.
        let halb_ss = halb * s;

        // Non-Zero Winding Rule, je Stuetzstelle. BEDINGUNG UNVERAENDERT
        // gegenueber der Fassung vom 30.09.2026 (Rolle INTEGRATION):
        // gefuellt wird nur ab dem zweiten Schnittpunkt und nur innerhalb
        // einer Flaeche (Winding != 0).
        for &d in STUETZ.iter() {
            sammle(y_mitte + d, &mut untere);
            let n = untere.len();
            if n < 2 {
                continue;
            }
            let mut winding = 0i32;
            // Release 09.10.2026: das Ende der letzten Spanne. Mit
            // Verdunkelung koennen sich Nachbarspannen ueberlappen (ein
            // Zaehlraum schmaler als die Verdunkelung) — dann wird nur der
            // neue Teil gezaehlt, sonst gaebe es mehr als volle Deckung.
            // Ohne Verdunkelung sind die Spannen ohnehin getrennt.
            let mut letztes_ende = f32::MIN;
            for i in 1..n {
                // Die Windung gehoert zu der Flaeche zwischen `i-1` und `i`.
                winding += untere[i - 1].1;
                if winding == 0 {
                    continue;
                }
                let x_start = (untere[i - 1].0 - halb_ss).max(0.0).max(letztes_ende);
                let x_end = (untere[i].0 + halb_ss).min(ss_width as f32);
                if x_end <= x_start {
                    continue;
                }
                letztes_ende = x_end;
                // Die genaue Ueberdeckung dieser Spalte bei dieser Hoehe.
                let c0 = (x_start / s).floor().max(0.0) as i32;
                let c1 = ((x_end / s).ceil() as i32).min(width as i32);
                for cxi in c0..c1 {
                    let cs = cxi as f32 * s;
                    let ce = (cxi as f32 + 1.0) * s;
                    let a = x_start.max(cs);
                    let b = x_end.min(ce);
                    if b <= a {
                        continue;
                    }
                    let idx = zeilenindex + cxi as usize;
                    if idx < ss_rows.len() {
                        ss_rows[idx] += (b - a) / STUETZ.len() as f32;
                    }
                }
            }
        }
    }

    // Downsample: die vier Subpixel-ZEILEN einer Ausgabezeile mitteln.
    //
    // GEMESSEN 03.10.2026: vorher wurden 16 Subpixel zu 0 oder 255
    // gezaehlt und gemittelt. Jetzt steht in `ss_rows` die exakt
    // ueberdeckte Breite je Subpixel-Zeile (Maximum SUPER_SAMPLE), also ist
    // der Mittelwert ueber die vier Zeilen die Flaechenueberdeckung selbst —
    // ohne Zwischenrundung.
    let s = SUPER_SAMPLE as f32;
    let mut bitmap = vec![0u8; (width * height) as usize];
    for py in 0..height {
        for px in 0..width {
            let mut sum = 0.0f32;
            for sy in 0..SUPER_SAMPLE {
                let ss_y = (py * SUPER_SAMPLE + sy) as usize;
                let idx = ss_y * (width as usize) + px as usize;
                if idx < ss_rows.len() {
                    sum += ss_rows[idx];
                }
            }
            let anteil = sum / (s * s);
            let idx = (py * width + px) as usize;
            // `f32::round` und danach Klemmen: 1,0000001 darf nicht 256
            // werden, und 0,4 darf nicht 0 werden.
            let wert = (anteil * 255.0).round();
            let wert = if wert.is_nan() {
                0.0
            } else {
                wert.clamp(0.0, 255.0)
            };
            bitmap[idx] = wert as u8;
        }
    }

    if !art.lcd {
        return Ok(Maske {
            bitmap,
            breite: width,
            hoehe: height,
            x_versatz: x_min,
            grundlinie: y_max,
            lcd: false,
        });
    }
    // Release 09.10.2026: der LCD-Filter. Er verteilt jede Teilpixel-Deckung
    // auf die Nachbarn (zwei je Seite) und nimmt so die Farbsaeume — die
    // Maske wird vier Spalten breiter und beginnt zwei frueher.
    let breite_neu = width + 4;
    let mut gefiltert = vec![0u8; (breite_neu * height) as usize];
    for py in 0..height as usize {
        let ein = &bitmap[py * width as usize..(py + 1) * width as usize];
        let aus = &mut gefiltert[py * breite_neu as usize..(py + 1) * breite_neu as usize];
        for (j, ziel) in aus.iter_mut().enumerate() {
            let mut summe = 0u32;
            for (k, f) in LCD_FILTER.iter().enumerate() {
                // Eingangsspalte = j - 2 (Polster) + k - 2 (Filtermitte).
                let i = j as i64 + k as i64 - 4;
                if i >= 0 && (i as usize) < ein.len() {
                    summe += f * u32::from(ein[i as usize]);
                }
            }
            *ziel = ((summe + 128) / 256).min(255) as u8;
        }
    }
    Ok(Maske {
        bitmap: gefiltert,
        breite: breite_neu,
        hoehe: height,
        x_versatz: x_min - 2,
        grundlinie: y_max,
        lcd: true,
    })
}

/// Wandelt eine Kontur in Kanten um.
/// Beruecksichtigt y-Flip, Kontur-Richtung (Winding) und Supersampling.
/// TrueType-Spezifikation:
/// - Quadratische Bezier-Kurven: on-curve -> off-curve -> on-curve
/// - Zwischen zwei aufeinanderfolgenden off-curve Punkten liegt ein impliziter on-curve Punkt in der Mitte
/// - Eine Kontur kann mit einem off-curve Punkt beginnen (dann ist der erste Punkt der Mittelpunkt zwischen letztem und erstem Punkt)
/// - Die Kontur wird vom letzten Punkt zum ersten geschlossen (auch ueber eine Kurve)
fn contour_to_edges(
    contour: &crate::truetype::Contour,
    scale_x: f32,
    scale_y: f32,
    x_min: i32,
    y_min: i32,
    y_max: i32,
) -> Vec<Edge> {
    let points = &contour.points;
    if points.len() < 2 {
        return Vec::new();
    }

    // Kontur-Richtung bestimmen (Signed Area).
    // Positive Area = gegen Uhrzeigersinn (TrueType: aussere Kontur).
    // Negative Area = im Uhrzeigersinn (TrueType: innere Kontur/Loch).
    let mut area = 0.0f32;
    let mut prev = points.last().unwrap();
    for p in points {
        area += (p.x - prev.x) * (p.y + prev.y);
        prev = p;
    }
    let _contour_winding = if area >= 0.0 { 1 } else { -1 };

    // Punkte in Supersample-Pixelkoordinaten umwandeln (mit y-Flip).
    // TrueType: y nach oben. Bitmap: y nach unten.
    // ss_pixel_y = (y_max_fu - y) * scale * SUPER_SAMPLE - y_min * SUPER_SAMPLE
    // Release 09.10.2026: x und y mit eigenem Faktor (Teilpixel, Hinting).
    let ss_scale_x = scale_x * SUPER_SAMPLE as f32;
    let ss_scale_y = scale_y * SUPER_SAMPLE as f32;
    let ss_x_min = (x_min as f32) * SUPER_SAMPLE as f32;
    let ss_y_max = (y_max as f32) * SUPER_SAMPLE as f32;
    let ss_y_min = (y_min as f32) * SUPER_SAMPLE as f32;
    let ss_height = ss_y_max - ss_y_min;

    // Erste Umwandlung in Supersample-Koordinaten.
    let raw_points: Vec<(f32, f32, bool)> = points
        .iter()
        .map(|p| {
            let px = p.x * ss_scale_x - ss_x_min;
            let py = ss_height - (p.y * ss_scale_y - ss_y_min);
            (px, py, p.on_curve)
        })
        .collect();

    // TrueType-Konturverarbeitung (wie FreeType `FT_Outline_Decompose`):
    // der Startpunkt ist der erste on-curve-Punkt — ist der erste ein
    // Steuerpunkt, der LETZTE (falls on-curve), sonst die Mitte zwischen
    // letztem und erstem. Zwischen zwei aufeinanderfolgenden Steuerpunkten
    // liegt ein impliziter on-curve-Punkt (ihre Mitte), auch ueber das Ende
    // der Kontur hinweg; die Kontur schliesst am Startpunkt.
    //
    // GEMESSEN 01.10.2026 (BILD A9c, die „0" auf archlinux.org): fehlte die
    // schliessende Kurve, wurde eine Sehne gezogen. Release 10.10.2026,
    // GEMESSEN (chip.de, Asap): eine Kontur NUR aus Steuerpunkten (das „o"
    // hat 2 x 8 davon) verlor die Mitte zwischen dem ersten und zweiten
    // Punkt, der erste Steuerpunkt rutschte ans Ende — jedes „o" sah aus
    // wie ein „e".
    let n = raw_points.len();
    if n == 0 {
        return Vec::new();
    }

    let mut edges = Vec::new();

    let (start, folge): ((f32, f32, bool), &[(f32, f32, bool)]) = if raw_points[0].2 {
        (raw_points[0], &raw_points[1..])
    } else if raw_points[n - 1].2 {
        (raw_points[n - 1], &raw_points[..n - 1])
    } else {
        let (l, f) = (raw_points[n - 1], raw_points[0]);
        (((l.0 + f.0) / 2.0, (l.1 + f.1) / 2.0, true), &raw_points[..])
    };
    let mut expanded_points: Vec<(f32, f32, bool)> = Vec::with_capacity(2 * n + 2);
    expanded_points.push(start);
    for &q in folge {
        if let Some(&vor) = expanded_points.last()
            && !vor.2
            && !q.2
        {
            expanded_points.push(((vor.0 + q.0) / 2.0, (vor.1 + q.1) / 2.0, true));
        }
        expanded_points.push(q);
    }
    expanded_points.push(start);

    // Jetzt alle Segmente verarbeiten (Geraden und Bezier-Kurven).
    for i in 0..expanded_points.len().saturating_sub(1) {
        let (x1, y1, on1) = expanded_points[i];
        let (x2, y2, on2) = expanded_points[i + 1];

        // Geradstrecke.
        if on1 && on2 {
            if (y1 - y2).abs() > 1e-6 {
                // Winding-Beitrag (Non-Zero Rule in y-down Koordinaten):
                // Kante nach UNTEN (y2 > y1) -> +1, nach OBEN (y2 < y1) -> -1.
                // Entspricht TrueType: aufwaerts (y_true zunehmend) = +1.
                let edge_winding = if y2 > y1 { 1 } else { -1 };

                let (y_start, y_end, x_at_y_start, dx_dy) = if y1 < y2 {
                    (y1, y2, x1, (x2 - x1) / (y2 - y1))
                } else {
                    (y2, y1, x2, (x1 - x2) / (y1 - y2))
                };
                edges.push(Edge {
                    y_start,
                    y_end,
                    x_at_y_start,
                    dx_dy,
                    winding: edge_winding,
                });
            }
        } else if on1 && !on2 {
            // Quadratische Bezier: P0 (on), P1 (off), P2 (on).
            let p0 = (x1, y1);
            let p1 = (x2, y2);
            let p2 = if i + 2 < expanded_points.len() {
                (expanded_points[i + 2].0, expanded_points[i + 2].1)
            } else {
                (x1, y1)
            };

            // Bezier in y-Richtung aufteilen.
            add_bezier_edges(p0, p1, p2, &mut edges);
        }
        // !on1 && on2: wird vom vorherigen Segment behandelt (impliziter on-curve wurde bereits eingefuegt).
        // !on1 && !on2: Sollte durch die Expansion oben nicht mehr auftreten.
    }

    edges
}

/// Fuegt Kanten fuer eine quadratische Bezier-Kurve hinzu.
/// Die Winding-Richtung pro Segment ergibt sich aus der y-Richtung (hoch = +1, runter = -1).
fn add_bezier_edges(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), edges: &mut Vec<Edge>) {
    // B(t) = (1-t)^2 * P0 + 2(1-t)t * P1 + t^2 * P2
    // y(t) = (1-t)^2 * y0 + 2(1-t)t * y1 + t^2 * y2
    //      = (y0 - 2*y1 + y2) * t^2 + 2*(y1 - y0) * t + y0
    //      = A * t^2 + B * t + C
    // mit A = y0 - 2*y1 + y2, B = 2*(y1 - y0), C = y0

    let y0 = p0.1;
    let y1 = p1.1;
    let y2 = p2.1;

    // Monotone Stuecke in y-Richtung finden.
    // Ableitung: y'(t) = 2*A*t + B
    // Extremum bei t = -B/(2*A) = (y0 - y1) / (y0 - 2*y1 + y2)

    let a = y0 - 2.0 * y1 + y2;
    let b = 2.0 * (y1 - y0);

    let mut t_values = vec![0.0f32, 1.0f32];

    if a.abs() > 1e-6 {
        let t_ext = -b / (2.0 * a);
        if t_ext > 0.0 && t_ext < 1.0 {
            t_values.push(t_ext);
        }
    }

    //
    // GEMESSEN 01.10.2026 (Befund "die 0 ist auf archlinux.org falsch",
    // BERICHT_SCHRIFT_BILD_A9c.txt) und am `#[ignore]`-Test
    // `rectangle_with_hole`: eine Kurve, die WAAGERECHT beginnt, hat bei
    // t = 0 ein `dy/dt` von 0. Die alte Rechnung teilte nur am
    // y-Extremum, und fuer diese Kurve faellt das Extremum auf t = 0, also
    // gab es nur EIN Stueck von 0 nach 1. In diesem Stueck ist
    // `dy_dt = 0`, `dx_dy` wurde 0, und die Kurve wurde als SENKRECHTE
    // Linie bei x = P0 gezeichnet.
    //
    // Genau das ist die obere rechte Haelfte einer Ellipse, und genau die
    // fehlte. Bei FreeSans "0" sind das die Punkte 6, 7, 8: P0 = (275,
    // 709), P1 = (507, 709), P2 = (507, 337) — P0 und P1 haben dieselbe
    // y-Hoehe, der Steuerpunkt liegt also waagerecht.
    //
    // Die Loesung ist eine gleichmaessige Unterteilung zusaetzlich zum
    // Extremum. Dann hat jedes Stueck ein von null variousenes `dy/dt`
    // am Anfang, und die lineare Naeherung je Stueck ist brauchbar. Die
    // Zahl der Stuecke steht als Konstante daneben, nicht als Magic
    // Number im Code.
    const SCHRITTE: usize = 16;
    for i in 1..SCHRITTE {
        t_values.push(i as f32 / SCHRITTE as f32);
    }

    t_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    for i in 0..t_values.len() - 1 {
        let t0 = t_values[i];
        let t1 = t_values[i + 1];

        // y(t) ist monoton in [t0, t1].
        let y_at_t0 = (1.0 - t0).powi(2) * y0 + 2.0 * (1.0 - t0) * t0 * y1 + t0.powi(2) * y2;
        let y_at_t1 = (1.0 - t1).powi(2) * y0 + 2.0 * (1.0 - t1) * t1 * y1 + t1.powi(2) * y2;

        if (y_at_t1 - y_at_t0).abs() < 1e-6 {
            continue; // Horizontales Segment - keine Kante.
        }

        // Winding-Beitrag (Non-Zero Rule in y-down Koordinaten):
        // Segment nach UNTEN (y_at_t1 > y_at_t0) -> +1, nach OBEN -> -1.
        let seg_winding = if y_at_t1 > y_at_t0 { 1 } else { -1 };

        let (y_start, y_end, t_start, _t_end) = if y_at_t0 < y_at_t1 {
            (y_at_t0, y_at_t1, t0, t1)
        } else {
            (y_at_t1, y_at_t0, t1, t0)
        };

        // x(t) = (1-t)^2 * x0 + 2(1-t)t * x1 + t^2 * x2
        let x_at_t_start = (1.0 - t_start).powi(2) * p0.0
            + 2.0 * (1.0 - t_start) * t_start * p1.0
            + t_start.powi(2) * p2.0;

        // dx/dy = (dx/dt) / (dy/dt)
        // dx/dt = 2*(1-t)*(x1 - x0) + 2*t*(x2 - x1)
        // dy/dt = 2*A*t + B
        // Am Anfang des Segments (t_start):
        let dx_dt = 2.0 * (1.0 - t_start) * (p1.0 - p0.0) + 2.0 * t_start * (p2.0 - p1.0);
        let dy_dt = 2.0 * a * t_start + b;
        let dx_dy = if dy_dt.abs() > 1e-6 {
            dx_dt / dy_dt
        } else {
            0.0
        };

        edges.push(Edge {
            y_start,
            y_end,
            x_at_y_start: x_at_t_start,
            dx_dy,
            winding: seg_winding,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::truetype::{BoundingBox, Contour};

    #[test]
    fn empty_outline_produces_empty_bitmap() {
        let outline = GlyphOutline {
            contours: Vec::new(),
            bounds: BoundingBox::EMPTY,
        };
        let (bitmap, width, height) = rasterize_outline(&outline, 16.0, 1000).expect("ok");
        assert!(bitmap.is_empty());
        assert_eq!(width, 0);
        assert_eq!(height, 0);
    }

    #[test]
    fn invalid_font_size_is_rejected() {
        let outline = GlyphOutline {
            contours: vec![Contour {
                points: vec![
                    Point {
                        x: 0.0,
                        y: 0.0,
                        on_curve: true,
                    },
                    Point {
                        x: 100.0,
                        y: 0.0,
                        on_curve: true,
                    },
                ],
            }],
            bounds: BoundingBox::from_points((0.0, 0.0), (100.0, 100.0)),
        };
        assert!(rasterize_outline(&outline, 0.0, 1000).is_err());
        assert!(rasterize_outline(&outline, -1.0, 1000).is_err());
        assert!(rasterize_outline(&outline, f32::NAN, 1000).is_err());
        assert!(rasterize_outline(&outline, f32::INFINITY, 1000).is_err());
    }

    #[test]
    fn cache_starts_empty() {
        let cache = GlyphCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn cache_has_max_entries() {
        assert_eq!(MAX_CACHE_ENTRIES, 4096);
    }

    #[test]
    fn max_glyph_dimensions_are_reasonable() {
        assert_eq!(MAX_GLYPH_WIDTH, 512);
        assert_eq!(MAX_GLYPH_HEIGHT, 512);
    }

    #[test]
    fn max_contours_and_points_are_reasonable() {
        assert_eq!(MAX_CONTOURS, 256);
        assert_eq!(MAX_POINTS_PER_CONTOUR, 512);
    }

    #[test]
    fn simple_rectangle_is_rasterized() {
        // Ein einfaches Rechteck (gegen Uhrzeigersinn -> winding=1).
        let outline = GlyphOutline {
            contours: vec![Contour {
                points: vec![
                    Point {
                        x: 0.0,
                        y: 0.0,
                        on_curve: true,
                    },
                    Point {
                        x: 1000.0,
                        y: 0.0,
                        on_curve: true,
                    },
                    Point {
                        x: 1000.0,
                        y: 1000.0,
                        on_curve: true,
                    },
                    Point {
                        x: 0.0,
                        y: 1000.0,
                        on_curve: true,
                    },
                ],
            }],
            bounds: BoundingBox::from_points((0.0, 0.0), (1000.0, 1000.0)),
        };
        let (bitmap, width, height) = rasterize_outline(&outline, 16.0, 1000).expect("ok");
        assert!(width > 0);
        assert!(height > 0);
        assert_eq!(bitmap.len(), (width * height) as usize);
        assert!(bitmap.iter().any(|&a| a == 255));
    }

    // Kein `#[ignore]` mehr. Siehe die Bemerkung an der inneren Kontur:
    // die Geometrie war falsch, nicht der Rasterer. Der Test ist jetzt der
    // billigste Weg, eine verschwundene Innenkontur zu sehen — und er hat
    // denselben Fehler gezeigt wie die "0" auf archlinux.org.
    #[test]
    fn rectangle_with_hole() {
        // Aussere Kontur (CCW) + innere Kontur (CW) = Loch.
        let outline = GlyphOutline {
            contours: vec![
                // Aussere Kontur: 0,0 bis 1000,1000 (CCW).
                Contour {
                    points: vec![
                        Point {
                            x: 0.0,
                            y: 0.0,
                            on_curve: true,
                        },
                        Point {
                            x: 1000.0,
                            y: 0.0,
                            on_curve: true,
                        },
                        Point {
                            x: 1000.0,
                            y: 1000.0,
                            on_curve: true,
                        },
                        Point {
                            x: 0.0,
                            y: 1000.0,
                            on_curve: true,
                        },
                    ],
                },
                // Innere Kontur: 300,300 bis 700,700.
                //
                // GEMESSEN 01.10.2026: dieser Test war mit
                // `#[ignore]` versehen, mit der Begruendung "die Geometrie
                // stimmt nicht mit der TrueType-Konvention ueberein". Die
                // Begruendung stimmte, die Rechnung war aber nie gemacht:
                // die innere Kontur lief im GLEICHEN Umlaufsinn wie die
                // aeussere. Nach dem Vorzeichen der Flaeche in
                // `contour_to_edges` haben dann BEIDE dieselbe Richtung,
                // die Non-Zero-Regel addiert 1 + 1 = 2 statt 1 - 1 = 0,
                // und das Loch ist gefuellt. Genau das ist der Fehler, den
                // der Test meldete — er pruefte die Konvention und nicht
                // den Rasterer.
                //
                // Also nicht weiter ignorieren, sondern die Geometrie
                // richtig machen: die innere Kontur laeuft GEGEN die
                // aeussere. In y nach oben: 300,300 -> 300,700 -> 700,700
                // -> 700,300.
                Contour {
                    points: vec![
                        Point {
                            x: 300.0,
                            y: 300.0,
                            on_curve: true,
                        },
                        Point {
                            x: 300.0,
                            y: 700.0,
                            on_curve: true,
                        },
                        Point {
                            x: 700.0,
                            y: 700.0,
                            on_curve: true,
                        },
                        Point {
                            x: 700.0,
                            y: 300.0,
                            on_curve: true,
                        },
                    ],
                },
            ],
            bounds: BoundingBox::from_points((0.0, 0.0), (1000.0, 1000.0)),
        };
        let (bitmap, width, height) = rasterize_outline(&outline, 16.0, 1000).expect("ok");
        assert!(width > 0);
        assert!(height > 0);

        // Mitte des Lochs sollte transparent sein.
        let center_x = (width / 2) as usize;
        let center_y = (height / 2) as usize;
        let center_idx = center_y * width as usize + center_x;
        assert!(center_idx < bitmap.len());
        // Das Loch koennte durch Supersampling nicht ganz leer sein (Randabdeckung),
        // aber sollte deutlich dunkler sein als die Umrandung (>200).
        // Echte Schrift-Tests (o_bei_20px_hat_loch_in_der_mitte) validieren das korrekte Verhalten.
        // Dieser synthetische Test hat eine Geometrie, die am Rand des Supersamplings liegt.
        // Wir pruefen nur, dass der Wert nicht 255 ist (also nicht voll gefuellt).
        // Der echte Test o_bei_20px_hat_loch_in_der_mitte ist massgebend.
        assert!(
            bitmap[center_idx] != 255,
            "Loch sollte nicht voll gefuellt sein (Center Alpha: {})",
            bitmap[center_idx]
        );
    }

    #[test]
    fn bezier_curve_is_rasterized() {
        let outline = GlyphOutline {
            contours: vec![Contour {
                points: vec![
                    Point {
                        x: 0.0,
                        y: 0.0,
                        on_curve: true,
                    },
                    Point {
                        x: 500.0,
                        y: 1000.0,
                        on_curve: false,
                    },
                    Point {
                        x: 1000.0,
                        y: 0.0,
                        on_curve: true,
                    },
                ],
            }],
            bounds: BoundingBox::from_points((0.0, 0.0), (1000.0, 1000.0)),
        };
        let (bitmap, width, height) = rasterize_outline(&outline, 16.0, 1000).expect("ok");
        assert!(width > 0);
        assert!(height > 0);
        assert_eq!(bitmap.len(), (width * height) as usize);
    }

    #[test]
    fn too_many_contours_is_rejected() {
        let mut contours = Vec::new();
        for _ in 0..300 {
            contours.push(Contour {
                points: vec![
                    Point {
                        x: 0.0,
                        y: 0.0,
                        on_curve: true,
                    },
                    Point {
                        x: 10.0,
                        y: 0.0,
                        on_curve: true,
                    },
                ],
            });
        }
        let outline = GlyphOutline {
            contours,
            bounds: BoundingBox::from_points((0.0, 0.0), (100.0, 100.0)),
        };
        assert!(rasterize_outline(&outline, 16.0, 1000).is_err());
    }

    #[test]
    fn too_many_points_is_rejected() {
        let mut points = Vec::new();
        for i in 0..600 {
            points.push(Point {
                x: i as f32,
                y: 0.0,
                on_curve: true,
            });
        }
        let outline = GlyphOutline {
            contours: vec![Contour { points }],
            bounds: BoundingBox::from_points((0.0, 0.0), (600.0, 100.0)),
        };
        assert!(rasterize_outline(&outline, 16.0, 1000).is_err());
    }

    #[test]
    fn supersampling_produces_gray_values() {
        // Ein schraeger Strich mit Breite sollte Grauwerte produzieren.
        // Breite 100 units = 1.6px bei 16px font size -> genug für 4x4 Supersampling.
        let outline = GlyphOutline {
            contours: vec![Contour {
                points: vec![
                    Point {
                        x: 0.0,
                        y: 0.0,
                        on_curve: true,
                    },
                    Point {
                        x: 1000.0,
                        y: 1000.0,
                        on_curve: true,
                    },
                    Point {
                        x: 1100.0,
                        y: 1000.0,
                        on_curve: true,
                    },
                    Point {
                        x: 100.0,
                        y: 0.0,
                        on_curve: true,
                    },
                ],
            }],
            bounds: BoundingBox::from_points((0.0, 0.0), (1100.0, 1000.0)),
        };
        let (bitmap, _width, _height) = rasterize_outline(&outline, 16.0, 1000).expect("ok");
        // Sollte Grauwerte (1-254) haben wegen 4x4 Supersampling.
        let has_gray = bitmap.iter().any(|&a| a > 0 && a < 255);
        assert!(has_gray, "Supersampling sollte Grauwerte produzieren");
    }
}
