//! Release 09.10.2026: gamma-korrekte Textueberblendung und Subpixel-Text.
//!
//! # WARUM
//!
//! GEMESSEN vorher (Adressleiste, „http://youtube.com", heller Text auf
//! dunklem Glas): die Deckung einer Glyphe wurde LINEAR in sRGB gemischt
//! (`blend_source_over`). Eine halb gedeckte Kante hellen Textes auf dunklem
//! Grund bekam damit den sRGB-Wert der Mitte — das ist physikalisch nur gut
//! ein Fuenftel der Helligkeit. Heller Text auf dunklem Grund wirkt so duenn
//! und verwaschen, dunkler Text auf hellem Grund dagegen kraeftig.
//!
//! # WIE
//!
//! Gemischt wird in einem Gamma-Raum (γ = 1,8, der Vorgabewert von
//! DirectWrite fuer Text) mit Kontrastanhebung der Deckung (`a + (1−a)·k·a`,
//! wie Skia), wobei `k` mit der Helligkeit des ECHTEN Hintergrunds waechst:
//! dunkler Text auf hellem Grund bleibt so kraeftig wie bisher (GEMESSEN:
//! halbe Deckung Schwarz auf Weiss 118 statt 127), heller Text auf dunklem
//! Grund wird hell genug (halbe Deckung Weiss auf Schwarz 174 statt 128).
//!
//! Nur auf DECKENDEM Grund: ist der Hintergrund durchsichtig (Tests, Ebenen),
//! gibt es keine Farbe, gegen die man rechnen koennte — dann bleibt es bei
//! `blend_source_over`, bitgenau wie bisher.

use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use narsil_core::color::Color;

/// Der Exponent des Mischraums.
const GAMMA: f32 = 1.8;
/// Die Kontrastanhebung der Deckung (DirectWrite: `GrayscaleEnhancedContrast` 1,0).
const KONTRAST: f32 = 1.0;
/// Stufen der Rueckrechnung (linear -> 8 Bit).
const STUFEN: usize = 65_536;

struct Tafeln {
    /// 8 Bit -> linear (x^γ).
    lin: [f32; 256],
    /// linear (in 1/65535) -> 8 Bit.
    zurueck: Vec<u8>,
}

fn tafeln() -> &'static Tafeln {
    static T: OnceLock<Tafeln> = OnceLock::new();
    T.get_or_init(|| {
        let mut lin = [0f32; 256];
        for (i, l) in lin.iter_mut().enumerate() {
            *l = (i as f32 / 255.0).powf(GAMMA);
        }
        let zurueck = (0..STUFEN)
            .map(|j| {
                let v = (j as f32 / (STUFEN - 1) as f32).powf(1.0 / GAMMA);
                (v * 255.0).round().clamp(0.0, 255.0) as u8
            })
            .collect();
        Tafeln { lin, zurueck }
    })
}

/// Mischt Text der Farbe `text` mit der Deckung `deckung` (je Kanal R, G, B;
/// bei Graustufen dreimal derselbe Wert) ueber `grund`.
///
/// Auf durchsichtigem oder halb durchsichtigem Grund: `blend_source_over`
/// mit der mittleren Deckung — so wie bisher.
#[must_use]
pub fn mische(text: Color, deckung: [u8; 3], grund: Color) -> Color {
    if deckung == [0, 0, 0] {
        return grund;
    }
    if grund.a != 255 {
        let mittel = ((u16::from(deckung[0]) + u16::from(deckung[1]) + u16::from(deckung[2]) + 1) / 3) as u8;
        return crate::framebuffer::blend_source_over(Color::new(text.r, text.g, text.b, mittel), grund);
    }
    let t = tafeln();
    let g = [t.lin[grund.r as usize], t.lin[grund.g as usize], t.lin[grund.b as usize]];
    let s = [text.r, text.g, text.b];
    let d = [grund.r, grund.g, grund.b];
    // Die Helligkeit des Grundes (Rec. 709) bestimmt die Kontrastanhebung.
    let k = KONTRAST * (0.2126 * g[0] + 0.7152 * g[1] + 0.0722 * g[2]);
    let mut aus = [0u8; 3];
    for c in 0..3 {
        let a = f32::from(deckung[c]) / 255.0;
        if a <= 0.0 || s[c] == d[c] {
            aus[c] = d[c];
            continue;
        }
        if a >= 1.0 {
            aus[c] = s[c];
            continue;
        }
        let a = (a + (1.0 - a) * k * a).min(1.0);
        let l = t.lin[s[c] as usize] * a + g[c] * (1.0 - a);
        let j = (l * (STUFEN - 1) as f32 + 0.5) as usize;
        aus[c] = t.zurueck[j.min(STUFEN - 1)];
    }
    Color::new(aus[0], aus[1], aus[2], 255)
}

/// 0 = noch nicht gelesen, 1 = an, 2 = aus.
static FEIN: AtomicU8 = AtomicU8::new(0);

/// Release 09.10.2026: ist die feine Textrasterung an (Hinting, Verdunkelung,
/// Gamma)? Vorgabe an; `NARSIL_SCHRIFT_FEIN=0` schaltet sie ab — fuer
/// Vorher/Nachher-Bilder mit demselben Bau.
#[must_use]
pub fn fein_an() -> bool {
    match FEIN.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let an = std::env::var("NARSIL_SCHRIFT_FEIN").map_or(true, |v| v.trim() != "0");
            FEIN.store(if an { 1 } else { 2 }, Ordering::Relaxed);
            an
        }
    }
}

/// Schaltet die feine Textrasterung (fuer Tests und Messungen).
pub fn setze_fein(an: bool) {
    FEIN.store(if an { 1 } else { 2 }, Ordering::Relaxed);
}

/// Die Anordnung der Teilpixel eines LCD-Bildschirms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LcdOrdnung {
    /// Rot links, Blau rechts (die meisten Bildschirme).
    Rgb,
    /// Blau links, Rot rechts.
    Bgr,
}

/// 0 = aus, 1 = RGB, 2 = BGR.
static LCD: AtomicU8 = AtomicU8::new(0);

/// Release 09.10.2026: Subpixel-Text fuer die LEISTE ein- oder ausschalten.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Vorgabe AUS ist: die Tests und jeder
/// Aufrufer ohne Fenster bekommen dieselben Pixel wie bisher. Das Fenster
/// (`narsil::main`) fragt Windows (ClearType an? RGB oder BGR?) und setzt
/// es danach — so wie Firefox und Chrome der Systemeinstellung folgen.
pub fn setze_lcd(ordnung: Option<LcdOrdnung>) {
    LCD.store(
        match ordnung {
            None => 0,
            Some(LcdOrdnung::Rgb) => 1,
            Some(LcdOrdnung::Bgr) => 2,
        },
        Ordering::Relaxed,
    );
}

/// Die eingestellte LCD-Anordnung (`None` = Graustufen).
#[must_use]
pub fn lcd() -> Option<LcdOrdnung> {
    match LCD.load(Ordering::Relaxed) {
        1 => Some(LcdOrdnung::Rgb),
        2 => Some(LcdOrdnung::Bgr),
        _ => None,
    }
}

thread_local! {
    /// Wird gerade Leistentext gezeichnet? (gesetzt vom Rasterer je Text)
    static LEISTE: Cell<bool> = const { Cell::new(false) };
}

/// Der Rasterer markiert damit Leistentext (siehe [`ist_leistenfamilie`]).
pub fn setze_leiste(an: bool) {
    LEISTE.with(|l| l.set(an));
}

/// Ist der laufende Text Leistentext?
#[must_use]
pub fn ist_leiste() -> bool {
    LEISTE.with(Cell::get)
}

/// Traegt die Familienliste die Leistenmarke?
/// ([`narsil_display::LEISTEN_SCHRIFT`] beginnt damit.)
#[must_use]
pub fn ist_leistenfamilie(familie: &str) -> bool {
    familie.trim_start().starts_with(narsil_display::LEISTEN_MARKE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schwarz_auf_weiss_bleibt_kraeftig_weiss_auf_schwarz_wird_heller() {
        let halb = [128u8; 3];
        let schwarz_auf_weiss = mische(Color::BLACK, halb, Color::WHITE);
        let weiss_auf_schwarz = mische(Color::WHITE, halb, Color::BLACK);
        // Bisher (linear in sRGB): 127 und 128.
        assert!(schwarz_auf_weiss.r <= 127, "Schwarz auf Weiss nicht heller als bisher: {schwarz_auf_weiss:?}");
        assert!(schwarz_auf_weiss.r >= 100, "aber nicht fetter als noetig: {schwarz_auf_weiss:?}");
        assert!(weiss_auf_schwarz.r >= 160, "Weiss auf Schwarz heller als bisher: {weiss_auf_schwarz:?}");
    }

    #[test]
    fn volle_und_keine_deckung_sind_genau() {
        let grund = Color::rgb(0x20, 0x22, 0x29);
        let text = Color::rgb(0xe8, 0xe8, 0xea);
        assert_eq!(mische(text, [255; 3], grund), text);
        assert_eq!(mische(text, [0; 3], grund), grund);
        // Gleiche Kanaele bleiben gleich (kein Rundungsrauschen).
        let gleich = mische(Color::rgb(0x20, 0x90, 0x29), [100; 3], grund);
        assert_eq!((gleich.r, gleich.b), (0x20, 0x29));
    }

    #[test]
    fn durchsichtiger_grund_mischt_wie_bisher() {
        let text = Color::rgb(10, 20, 30);
        let grund = Color::TRANSPARENT;
        assert_eq!(
            mische(text, [77; 3], grund),
            crate::framebuffer::blend_source_over(Color::new(10, 20, 30, 77), grund)
        );
    }

    #[test]
    fn die_deckung_steigt_monoton() {
        let mut vorher = 0u8;
        for a in 0..=255u8 {
            let c = mische(Color::WHITE, [a; 3], Color::BLACK);
            assert!(c.r >= vorher, "bei {a}: {} < {vorher}", c.r);
            vorher = c.r;
        }
        let mut vorher = 255u8;
        for a in 0..=255u8 {
            let c = mische(Color::BLACK, [a; 3], Color::WHITE);
            assert!(c.r <= vorher, "bei {a}: {} > {vorher}", c.r);
            vorher = c.r;
        }
    }
}
