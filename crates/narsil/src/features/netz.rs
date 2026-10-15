//! Release 08.10.2026: FEATURE 3 — NETZ-SANDBOX JE TAB und das
//! Verrauschen von Canvas-Auslesungen.
//!
//! * Die Regel (frei / nur lokal / gesperrt) steht je Tab im
//!   [`super::Zustand`] und wird beim Tabwechsel als prozessweite Regel an
//!   `narsil_net::haken` gegeben — dort greift sie fuer jede Anfrage.
//! * `toDataURL` und `getImageData` liefern bei eingeschalteter Funktion
//!   Pixel mit leisem, je Sitzung und Host festem Rauschen (ein Bit in jedem
//!   zehnten Pixel). Das Bild auf dem Schirm bleibt unveraendert; ein
//!   Canvas-Fingerabdruck ist zwischen Sitzungen und Seiten nicht mehr
//!   derselbe, innerhalb einer Seite aber stabil (zweimal lesen = gleich).

use std::cell::Cell;
use std::sync::OnceLock;

thread_local! {
    static RAUSCHEN: Cell<bool> = const { Cell::new(false) };
}

/// Das Verrauschen fuer diesen Faden ein- oder ausschalten.
pub fn rauschen_setzen(an: bool) {
    RAUSCHEN.with(|r| r.set(an));
}

/// Ist das Verrauschen in diesem Faden an?
pub fn rauschen_an() -> bool {
    RAUSCHEN.with(Cell::get)
}

/// Das Salz dieser Sitzung (einmal je Prozess, aus dem Zufall des Systems).
fn salz() -> u64 {
    static S: OnceLock<u64> = OnceLock::new();
    *S.get_or_init(|| {
        let b = narsil_tls::zufall::bytes(8).unwrap_or_else(|_| {
            super::jetzt_s().to_le_bytes().to_vec()
        });
        let mut a = [0u8; 8];
        a.copy_from_slice(&b[..8]);
        u64::from_le_bytes(a) | 1
    })
}

fn mische(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn host_wert(host: &str) -> u64 {
    host.bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01B3))
}

/// RGBA-Pixel verrauschen: `breite` ist die Zeilenlaenge des Ausschnitts,
/// `(x0, y0)` seine Lage im Canvas (damit dieselbe Stelle immer dasselbe
/// Rauschen bekommt, egal welcher Ausschnitt gelesen wird).
pub fn verrauschen_mit(px: &mut [u8], breite: u32, x0: i64, y0: i64, host: &str, salz: u64) {
    if breite == 0 {
        return;
    }
    let grund = salz ^ host_wert(host);
    for (i, p) in px.chunks_exact_mut(4).enumerate() {
        if p[3] == 0 {
            continue;
        }
        let x = x0 + (i as u32 % breite) as i64;
        let y = y0 + (i as u32 / breite) as i64;
        let h = mische(grund ^ ((x as u64) << 32 | (y as u64 & 0xFFFF_FFFF)));
        if h % 10 == 0 {
            let kanal = ((h >> 8) % 3) as usize;
            p[kanal] ^= 1;
        }
    }
}

/// Wie [`verrauschen_mit`], mit dem Salz dieser Sitzung.
pub fn verrauschen(px: &mut [u8], breite: u32, x0: i64, y0: i64, host: &str) {
    verrauschen_mit(px, breite, x0, y0, host, salz());
}

impl crate::Sitzung {
    /// Die Netzregel des aktiven Tabs (0 frei, 1 nur lokal, 2 gesperrt).
    pub fn netzregel_aktiver_tab(&self) -> u8 {
        self.browser
            .aktive_tab_id()
            .and_then(|t| self.features.netz.get(&t.0).copied())
            .unwrap_or(0)
    }

    /// Die Regel des aktiven Tabs an die Netzschicht geben.
    pub fn netz_anwenden(&self) {
        let an = self.einstellungen.features.an(super::Feature::NetzSandbox);
        let r = if an { self.netzregel_aktiver_tab() } else { 0 };
        narsil_net::haken::regel_setzen(narsil_net::haken::Netzregel::aus_zahl(r));
        rauschen_setzen(an);
    }

    /// Die Regel des aktiven Tabs weiterschalten (frei → nur lokal →
    /// gesperrt → frei) — aus dem Tab-Kontextmenue.
    pub fn netz_umschalten(&mut self) {
        let Some(t) = self.browser.aktive_tab_id() else {
            return;
        };
        let neu = (self.netzregel_aktiver_tab() + 1) % 3;
        if neu == 0 {
            self.features.netz.remove(&t.0);
        } else {
            self.features.netz.insert(t.0, neu);
        }
        self.netz_anwenden();
        self.meldung = Some(narsil_i18n::trf!(
            "Network sandbox: this tab is now “{}”",
            narsil_chrome::ktx_menue::netzregel_name(neu)
        ));
        self.schmutzig = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn das_rauschen_ist_leise_fest_und_je_host_verschieden() {
        let mut a = vec![200u8; 64 * 64 * 4];
        let original = a.clone();
        verrauschen_mit(&mut a, 64, 0, 0, "a.example", 12345);
        let veraendert = a.iter().zip(&original).filter(|(x, y)| x != y).count();
        // etwa jedes zehnte Pixel, je ein Kanal, je ein Bit
        assert!(veraendert > 64 * 64 / 20 && veraendert < 64 * 64 / 5, "{veraendert}");
        assert!(a.iter().zip(&original).all(|(x, y)| (*x as i16 - *y as i16).abs() <= 1));
        // fest: zweimal dasselbe
        let mut b = original.clone();
        verrauschen_mit(&mut b, 64, 0, 0, "a.example", 12345);
        assert_eq!(a, b);
        // ein Ausschnitt bekommt an derselben Stelle dasselbe Rauschen
        let mut ausschnitt = vec![200u8; 8 * 8 * 4];
        verrauschen_mit(&mut ausschnitt, 8, 10, 20, "a.example", 12345);
        for y in 0..8 {
            for x in 0..8 {
                let i = ((y * 8 + x) * 4) as usize;
                let j = (((20 + y) * 64 + 10 + x) * 4) as usize;
                assert_eq!(&ausschnitt[i..i + 4], &a[j..j + 4]);
            }
        }
        // je Host und je Sitzung verschieden
        let mut c = original.clone();
        verrauschen_mit(&mut c, 64, 0, 0, "b.example", 12345);
        assert_ne!(a, c);
        let mut d = original.clone();
        verrauschen_mit(&mut d, 64, 0, 0, "a.example", 999);
        assert_ne!(a, d);
        // durchsichtige Pixel bleiben unberuehrt
        let mut leer = vec![0u8; 16 * 4];
        verrauschen_mit(&mut leer, 4, 0, 0, "a.example", 1);
        assert!(leer.iter().all(|b| *b == 0));
    }

    /// Die Regel steht je Tab, das Tabmenue zeigt sie, und die Regel des
    /// aktiven Tabs sperrt in der Netzschicht. (Die prozessweite Regel wird
    /// hier nicht gesetzt — parallele Tests laden selbst aus dem Netz.)
    #[test]
    fn die_regel_gilt_je_tab_und_steht_im_tabmenue() {
        let mut s = crate::Sitzung::neu(900, 700);
        s.einstellungen.features.setzen(super::super::Feature::NetzSandbox, true);
        let erster = s.browser.aktive_tab_id().expect("Tab").0;
        s.features.netz.insert(erster, 2);
        assert_eq!(s.netzregel_aktiver_tab(), 2);
        assert!(narsil_net::haken::erlaubt(narsil_net::haken::Netzregel::aus_zahl(s.netzregel_aktiver_tab()), "example.com").is_err());
        s.features.netz.insert(erster, 1);
        assert!(narsil_net::haken::erlaubt(narsil_net::haken::Netzregel::aus_zahl(s.netzregel_aktiver_tab()), "127.0.0.1").is_ok());
        // Ein zweiter Tab hat seine eigene (freie) Regel.
        s.browser.anwenden(&narsil_browser::Navigation::TabNeu);
        assert_eq!(s.netzregel_aktiver_tab(), 0);
        // Das Tabmenue mit eingeschalteter Funktion hat die Netzzeile mit dem Zustand.
        let mut tm = narsil_chrome::ktx_menue::TabMenue::oeffne(0, Default::default(), &narsil_core::geometry::Point::new(10.0, 10.0));
        let ohne = tm.eintraege().len();
        tm.setze_features(Some(1), true);
        assert_eq!(tm.eintraege().len(), ohne + 2);
        let netz = tm.eintraege().iter().position(|e| *e == narsil_chrome::ktx_menue::TabEintrag::TabNetz).expect("Netzzeile");
        assert_eq!(tm.beschriftung(tm.eintraege()[netz]), "Network: local only → blocked");
    }

    #[test]
    fn das_rauschen_gilt_je_faden() {
        rauschen_setzen(true);
        assert!(rauschen_an());
        let anderer = std::thread::spawn(rauschen_an).join().expect("Faden");
        assert!(!anderer);
        rauschen_setzen(false);
    }
}
