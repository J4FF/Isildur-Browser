//! Release 09.10.2026: ANIMATIONEN der Oberflaeche — kurz, ease-out, an die
//! Bildrate gebunden, und nur solange etwas laeuft.
//!
//! * Tabwechsel: Ueberblendung 140 ms (das alte Bild in das neue).
//! * Menues und Popover: oeffnen mit Skalierung 0,96 → 1 und Einblenden
//!   (120 ms), schliessen schneller (80 ms, Ausblenden).
//! * Hover: Ueberblendung 80 ms.
//! * Panels (Downloads): gleiten ein (12 px) und blenden ein.
//! * Tabs: ein neuer Tab waechst auf seine Breite, ein geschlossener
//!   schrumpft weg (140 ms).
//!
//! Zeichnen nur waehrend einer Bewegung: [`Bewegungen::laeuft`] sagt der
//! Schleife, ob sie im Takt der Bildrate zeichnen muss; danach wartet sie
//! wieder auf Ereignisse (Leerlauf ~0 % CPU).
//!
//! ABSCHALTBAR: Windows „Animationseffekte" aus (`SPI_GETCLIENTAREAANIMATION`,
//! das Gegenstueck zu `prefers-reduced-motion`) oder `NARSIL_ANIMATION=0`.
//! Cargo-Testprogramme laufen ohne Animation — ihre Bilder sind sofort fertig.

use std::time::{Duration, Instant};

use narsil_core::geometry::Rect;

/// Dauer der Tab-Ueberblendung.
pub const TABWECHSEL: Duration = Duration::from_millis(140);
/// Dauer des Oeffnens (Menue, Popover, Panel).
pub const OEFFNEN: Duration = Duration::from_millis(120);
/// Dauer des Schliessens.
pub const SCHLIESSEN: Duration = Duration::from_millis(80);
/// Dauer einer Hover-Ueberblendung.
pub const HOVER: Duration = Duration::from_millis(80);
/// Dauer des Wachsens/Schrumpfens eines Tabs.
pub const TAB_BREITE: Duration = Duration::from_millis(140);
/// Startskalierung eines Menues.
pub const SKALA_START: f32 = 0.96;
/// So weit gleitet ein Panel herein (logische Pixel).
pub const GLEITEN_PX: f32 = 12.0;

/// ease-out (kubisch): schnell los, weich an.
#[must_use]
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// 0 = nach Umgebung und System, 1 = an, 2 = aus.
static VORGABE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Legt fest, ob animiert wird — fuer Werkzeuge ohne Fenster
/// (`website_diagnose` zeichnet ohne `NARSIL_ANIMATION=1` sofort fertige
/// Bilder, sonst stuende ein Menue im Bild halb eingeblendet).
pub fn setze_erlaubt(an: bool) {
    VORGABE.store(if an { 1 } else { 2 }, std::sync::atomic::Ordering::Relaxed);
}

/// Sind Animationen erlaubt? Einmal je Prozess gelesen.
#[must_use]
pub fn erlaubt() -> bool {
    match VORGABE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => return true,
        2 => return false,
        _ => {}
    }
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| {
        if std::env::var("NARSIL_ANIMATION").is_ok_and(|v| v.trim() == "0") {
            return false;
        }
        if std::env::var("NARSIL_ANIMATION").is_ok_and(|v| v.trim() == "1") {
            return true;
        }
        if narsil_chrome::einstellungen::laeuft_als_test() {
            return false;
        }
        system_erlaubt()
    })
}

/// Zeitdehnung fuer Messbilder (`NARSIL_ANIMATION_DEHNUNG`, 1..=50, Vorgabe 1).
fn dehnung() -> f32 {
    static D: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *D.get_or_init(|| {
        std::env::var("NARSIL_ANIMATION_DEHNUNG")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .map_or(1.0, |v| v.clamp(1.0, 50.0))
    })
}

#[cfg(all(windows, feature = "desktop"))]
fn system_erlaubt() -> bool {
    narsil_window::win32::animationen_an()
}

#[cfg(not(all(windows, feature = "desktop")))]
fn system_erlaubt() -> bool {
    true
}

/// Ein laufender Uebergang (Start und Dauer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Uebergang {
    /// Beginn.
    pub start: Instant,
    /// Dauer.
    pub dauer: Duration,
}

impl Uebergang {
    /// Beginnt jetzt. `NARSIL_ANIMATION_DEHNUNG=10` zieht jede Dauer auf das
    /// Zehnfache (Einzelbilder einer Animation fuer Messung und Abnahme).
    #[must_use]
    pub fn neu(jetzt: Instant, dauer: Duration) -> Self {
        Self { start: jetzt, dauer: dauer.mul_f32(dehnung()) }
    }

    /// Roher Fortschritt 0..=1.
    #[must_use]
    pub fn roh(&self, jetzt: Instant) -> f32 {
        if self.dauer.is_zero() {
            return 1.0;
        }
        (jetzt.saturating_duration_since(self.start).as_secs_f32() / self.dauer.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// Fortschritt mit ease-out.
    #[must_use]
    pub fn wert(&self, jetzt: Instant) -> f32 {
        ease_out(self.roh(jetzt))
    }

    /// Laeuft noch?
    #[must_use]
    pub fn laeuft(&self, jetzt: Instant) -> bool {
        self.roh(jetzt) < 1.0
    }
}

/// Eine Ueberblendung vom zuletzt gezeigten Bild in das neue, in einem
/// Bereich (Bildschirmpixel; `None` = ganzes Fenster). Das Fenster
/// (`main.rs`) fuehrt sie aus: es kennt das alte Bild.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blende {
    /// Bereich, `None` = alles.
    pub bereich: Option<Rect>,
    /// Dauer.
    pub dauer: Duration,
}

/// Der Zustand aller Bewegungen der Sitzung.
#[derive(Debug, Default)]
pub struct Bewegungen {
    /// Neue Ueberblendungen, die das Fenster abholt ([`Bewegungen::blenden_nehmen`]).
    blenden: Vec<Blende>,
    /// Wie lange das Fenster noch ueberblendet (gemeldet von `main.rs`).
    pub blende_bis: Option<Instant>,
    /// Oeffnen des Kontextmenues.
    pub menue: Option<Uebergang>,
    /// Oeffnen des Tabmenues.
    pub tabmenue: Option<Uebergang>,
    /// Oeffnen des Leistenmenues.
    pub leisten_menue: Option<Uebergang>,
    /// Einblenden des Download-Panels.
    pub downloads: Option<Uebergang>,
    /// Ein Tab waechst (Nummer) oder ein geschlossener schrumpft (Stelle).
    pub tab: Option<(Uebergang, TabBewegung)>,
}

/// Wie sich die Tableiste gerade bewegt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabBewegung {
    /// Tab `nr` ist neu und waechst.
    Auf(usize),
    /// Vor Stelle `nr` schliesst sich die Luecke eines geschlossenen Tabs.
    Zu(usize),
}

impl Bewegungen {
    /// Meldet eine Ueberblendung an (nur wenn Animationen erlaubt sind).
    pub fn blende(&mut self, bereich: Option<Rect>, dauer: Duration) {
        if erlaubt() {
            self.blenden.push(Blende { bereich, dauer });
        }
    }

    /// Das Fenster holt die neuen Ueberblendungen ab.
    pub fn blenden_nehmen(&mut self) -> Vec<Blende> {
        std::mem::take(&mut self.blenden)
    }

    /// Startet einen Uebergang, wenn Animationen erlaubt sind.
    #[must_use]
    pub fn starte(jetzt: Instant, dauer: Duration) -> Option<Uebergang> {
        erlaubt().then(|| Uebergang::neu(jetzt, dauer))
    }

    /// Raeumt fertige Uebergaenge ab; `true`, solange etwas laeuft (dann
    /// zeichnet die Schleife im Takt der Bildrate).
    pub fn takt(&mut self, jetzt: Instant) -> bool {
        let mut laeuft = !self.blenden.is_empty();
        for u in [&mut self.menue, &mut self.tabmenue, &mut self.leisten_menue, &mut self.downloads] {
            if u.is_some_and(|x| !x.laeuft(jetzt)) {
                *u = None;
            }
            laeuft |= u.is_some();
        }
        if self.tab.is_some_and(|(u, _)| !u.laeuft(jetzt)) {
            self.tab = None;
        }
        laeuft |= self.tab.is_some();
        if self.blende_bis.is_some_and(|b| jetzt >= b) {
            self.blende_bis = None;
        }
        laeuft | self.blende_bis.is_some()
    }

    /// Laeuft irgendetwas?
    #[must_use]
    pub fn laeuft(&self, jetzt: Instant) -> bool {
        !self.blenden.is_empty()
            || [self.menue, self.tabmenue, self.leisten_menue, self.downloads]
                .iter()
                .any(|u| u.is_some_and(|x| x.laeuft(jetzt)))
            || self.tab.is_some_and(|(u, _)| u.laeuft(jetzt))
            || self.blende_bis.is_some_and(|b| jetzt < b)
    }
}

/// Eine laufende Ueberblendung im Fenster: das alte Bild im Bereich.
struct Laufend {
    x0: usize,
    y0: usize,
    breite: usize,
    hoehe: usize,
    alt: Vec<u8>,
    u: Uebergang,
}

/// Release 09.10.2026: DER UEBERBLENDER des Fensters. Er haelt das zuletzt
/// gezeigte Bild; eine neue [`Blende`] merkt sich daraus den Bereich und
/// mischt ihn bis zum Ende ihrer Dauer in die neuen Bilder.
///
/// GEMESSENE ENTSCHEIDUNG, warum im Fenster und nicht im Rasterer: nur das
/// Fenster kennt das Bild, das gerade zu SEHEN ist — der Rasterer zeichnet
/// immer den Zustand von jetzt.
#[derive(Default)]
pub struct Ueberblender {
    letztes: Vec<u8>,
    breite: usize,
    hoehe: usize,
    laufend: Vec<Laufend>,
}

impl Ueberblender {
    /// Nimmt neue Blenden an, mischt alle laufenden in `bild` (RGBA,
    /// `breite × hoehe`) und merkt sich das Ergebnis als „zuletzt gezeigt".
    /// Rueckgabe: bis wann noch ueberblendet wird.
    pub fn anwenden(&mut self, neue: Vec<Blende>, bild: &mut Vec<u8>, breite: usize, hoehe: usize, jetzt: Instant) -> Option<Instant> {
        if breite != self.breite || hoehe != self.hoehe || bild.len() != breite * hoehe * 4 {
            // Neue Groesse: kein altes Bild, das passt.
            self.laufend.clear();
            self.letztes.clear();
        }
        if self.letztes.len() == bild.len() {
            for b in neue {
                let (x0, y0, x1, y1) = match b.bereich {
                    None => (0, 0, breite, hoehe),
                    Some(r) => (
                        (r.x.floor().max(0.0) as usize).min(breite),
                        (r.y.floor().max(0.0) as usize).min(hoehe),
                        (r.right().ceil().max(0.0) as usize).min(breite),
                        (r.bottom().ceil().max(0.0) as usize).min(hoehe),
                    ),
                };
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                let (bw, bh) = (x1 - x0, y1 - y0);
                let mut alt = Vec::with_capacity(bw * bh * 4);
                for y in y0..y1 {
                    let i = (y * breite + x0) * 4;
                    alt.extend_from_slice(&self.letztes[i..i + bw * 4]);
                }
                self.laufend.push(Laufend { x0, y0, breite: bw, hoehe: bh, alt, u: Uebergang::neu(jetzt, b.dauer) });
            }
        }
        self.laufend.retain(|l| l.u.laeuft(jetzt));
        for l in &self.laufend {
            let t = l.u.wert(jetzt);
            let k = |a: u8, n: u8| (f32::from(a) + (f32::from(n) - f32::from(a)) * t).round() as u8;
            for y in 0..l.hoehe {
                let z = ((l.y0 + y) * breite + l.x0) * 4;
                let a = y * l.breite * 4;
                for i in 0..l.breite * 4 {
                    let n = bild[z + i];
                    let o = l.alt[a + i];
                    if n != o {
                        bild[z + i] = k(o, n);
                    }
                }
            }
        }
        self.breite = breite;
        self.hoehe = hoehe;
        self.letztes.clear();
        self.letztes.extend_from_slice(bild);
        self.laufend.iter().map(|l| l.u.start + l.u.dauer).max()
    }
}

/// Deckkraft und Skalierung eines oeffnenden Menues zum Fortschritt `w`.
#[must_use]
pub fn menue_form(w: f32) -> (f32, f32) {
    let w = w.clamp(0.0, 1.0);
    (w, SKALA_START + (1.0 - SKALA_START) * w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_ist_schnell_los_und_weich_an() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.5) > 0.8, "{}", ease_out(0.5));
        assert!(ease_out(0.9) > 0.99);
    }

    #[test]
    fn ein_uebergang_endet_nach_seiner_dauer() {
        let t0 = Instant::now();
        let u = Uebergang::neu(t0, OEFFNEN);
        assert!(u.laeuft(t0));
        assert!(u.wert(t0) < 0.01);
        assert!(!u.laeuft(t0 + OEFFNEN));
        assert_eq!(u.wert(t0 + OEFFNEN + Duration::from_millis(5)), 1.0);
    }

    #[test]
    fn das_menue_waechst_von_096_auf_1() {
        assert_eq!(menue_form(0.0), (0.0, 0.96));
        let (d, s) = menue_form(1.0);
        assert_eq!((d, s), (1.0, 1.0));
    }

    #[test]
    fn der_ueberblender_mischt_alt_in_neu_und_hoert_auf() {
        let t0 = Instant::now();
        let mut u = Ueberblender::default();
        // Erstes Bild: schwarz. Keine Blende.
        let mut b = vec![0u8; 4 * 4 * 4];
        assert_eq!(u.anwenden(Vec::new(), &mut b, 4, 4, t0), None);
        // Zweites Bild: weiss, mit Blende ueber das ganze Fenster.
        let mut w = vec![255u8; 4 * 4 * 4];
        let ende = u.anwenden(vec![Blende { bereich: None, dauer: OEFFNEN }], &mut w, 4, 4, t0);
        assert_eq!(ende, Some(t0 + OEFFNEN));
        assert_eq!(w[0], 0, "am Anfang steht das alte Bild");
        // Mitte: ein Grau dazwischen (ease-out: schon weit).
        let mut w2 = vec![255u8; 4 * 4 * 4];
        u.anwenden(Vec::new(), &mut w2, 4, 4, t0 + OEFFNEN / 2);
        assert!(w2[0] > 128 && w2[0] < 255, "{}", w2[0]);
        // Ende: das neue Bild, keine Blende mehr.
        let mut w3 = vec![255u8; 4 * 4 * 4];
        assert_eq!(u.anwenden(Vec::new(), &mut w3, 4, 4, t0 + OEFFNEN), None);
        assert_eq!(w3[0], 255);
    }

    #[test]
    fn eine_blende_im_bereich_laesst_den_rest_in_ruhe() {
        let t0 = Instant::now();
        let mut u = Ueberblender::default();
        let mut b = vec![0u8; 4 * 4 * 4];
        u.anwenden(Vec::new(), &mut b, 4, 4, t0);
        let mut w = vec![255u8; 4 * 4 * 4];
        u.anwenden(vec![Blende { bereich: Some(Rect::new(0.0, 0.0, 2.0, 2.0)), dauer: HOVER }], &mut w, 4, 4, t0);
        assert_eq!(w[0], 0, "im Bereich: alt");
        assert_eq!(w[(3 * 4 + 3) * 4], 255, "ausserhalb: neu");
    }

    #[test]
    fn im_test_sind_animationen_aus() {
        // Cargo-Testprogramme bekommen sofort fertige Bilder.
        assert!(!erlaubt() || std::env::var("NARSIL_ANIMATION").is_ok());
    }
}
