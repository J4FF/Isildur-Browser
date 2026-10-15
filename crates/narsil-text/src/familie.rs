//! Die **eine** Stelle, die aus einer `font-family`-Liste eine Datei und
//! einen Stapel macht — benutzt von **beiden** Enden.
//!
//! # WARUM EINE ORTSCHAFT
//!
//! GEMESSEN 04.10.2026 (SCHRIFT_9, Bericht): der **Umbruch** misst mit
//! DejaVu Sans, der **Raster** zeichnet in der verlangten Familie. Ein
//! 152-Zeichen-Serifstring war in beiden GEMESSEN 1292 px breit, unsere
//! Messung sagte GEMESSEN 1267,88 px — 24,12 px zu schmal, und deshalb
//! passte bei uns ein Wort auf die Zeile, das in Firefox nicht passt
//! (GEMESSEN „formal", 45 px).
//!
//! Ein Fix an **einer** Seite haette den Fehler nur verschoben: der
//! Raster nimmt `quelle_der_familie` (narsil-raster), der Umbruch eine
//! `Messung` (narsil-layout), und `narsil-layout` haengt an keinem Crate, das
//! Dateien lesen kann. Diese Datei ist deshalb in `narsil-text` — die Crate,
//! die **beide** kennen.
//!
//! # WAS GEMESSEN IST
//!
//! GEMESSEN: `find_system_fonts("/usr/share/fonts")` liefert GEMESSEN 109
//! Dateien. Das Lesen wird **einmal** gemacht und thread-local gehalten
//! (GEMESSEN 30.09.2026: `find_system_fonts` ist auf 1000 Dateien
//! begrenzt und geht das Verzeichnis rekursiv bis Tiefe 5 — das ist pro
//! Zeile zu teuer, einmal je Thread nicht).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// GEMESSEN, warum der Import so aussieht (die erste Fassung war E0432/
// E0603 rot): `FontCategory`, `SystemFont` und `find_system_fonts` liegen
// **nicht** in `stapel`, sondern in `system` — `stapel.rs:39` importiert
// sie nur. Und `Schriftstapel`/`Stapelschrift` sind in `stapel` **privat
// importiert**, nicht `pub use`d.
use crate::stapel::{Schriftstapel, Stapelschrift};
use crate::stapel::{familie_waehlen_mit_gewicht, gewicht_ist_fett, system_stapel};
use crate::system::{FontCategory, SystemFont, find_system_fonts};

/// Die Wurzeln, in denen nach Systemschriften gesucht wird.
///
/// Windows verwendet den Systemordner und den Benutzerordner fuer Fonts.
/// Alle Schriftpfade in Messung und Raster verwenden diese gemeinsame Liste.
pub fn systemschrift_wurzeln() -> Vec<PathBuf> {
    #[cfg(windows)]
    let mut wurzeln = {
        let mut wurzeln = Vec::new();
        if let Some(windows) = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")) {
            wurzeln.push(PathBuf::from(windows).join("Fonts"));
        }
        if let Some(lokal) = std::env::var_os("LOCALAPPDATA") {
            wurzeln.push(PathBuf::from(lokal).join("Microsoft/Windows/Fonts"));
        }
        wurzeln
    };
    #[cfg(not(windows))]
    let mut wurzeln: Vec<PathBuf> =
        ["/usr/share/fonts", "/usr/local/share/fonts", "/usr/share/fonts/truetype", "/usr/share/X11/fonts"]
            .iter().map(PathBuf::from).collect();
    // Release 10.10.2026: `NARSIL_SCHRIFTORDNER` (Liste wie `PATH`) — weitere
    // Ordner, deren Schriften als SYSTEMschriften gelten. Die Web-Platform-
    // Tests setzen die Testschrift „Ahem" als installiert voraus (die Hersteller
    // installieren sie fuer ihre Laeufe); der Laeufer zeigt hier auf
    // `wpt/fonts`, statt sie ins System zu kopieren.
    if let Some(liste) = std::env::var_os("NARSIL_SCHRIFTORDNER") {
        wurzeln.extend(std::env::split_paths(&liste));
    }
    wurzeln
}

thread_local! {
    /// Die Systemschriften, **einmal** gelesen.
    static SCHRIFTEN: RefCell<Option<Arc<Vec<SystemFont>>>> = const { RefCell::new(None) };
    /// Leichte Dateiauswahl: vermeidet einen Familienscan pro Textknoten.
    static DATEIEN: RefCell<HashMap<(String, u16, bool), Option<PathBuf>>> =
        RefCell::new(HashMap::new());
    /// Derselbe ausgewaehlte Schnitt teilt seinen Stapel, auch wenn mehrere
    /// numerische CSS-Gewichte auf diese Datei zeigen.
    static STAPEL: RefCell<HashMap<(PathBuf, bool, bool), Arc<Schriftstapel>>> =
        RefCell::new(HashMap::new());
    /// Release 15.10.2026: the result of [`stapel_fuer_familie`] per family
    /// list (looked up without allocating), weight and style, valid for one
    /// generation of the web font register (`webschrift::stand`). MEASURED
    /// before: every text measurement and line metric of the layout went
    /// through the web font register, a `String` key and a `PathBuf` key
    /// (`familie.rs` among the hottest frames of the layout of
    /// https://www.chip.de/, cdb samples).
    static FERTIG: RefCell<(u64, HashMap<String, Vec<(u16, bool, Option<Arc<Schriftstapel>>)>>)> =
        RefCell::new((u64::MAX, HashMap::new()));
}

/// Die Systemschriften (GEMESSEN 109 Dateien).
///
/// Release 08.10.2026: EINMAL je Prozess, nicht je Faden. Der Scan oeffnet
/// jede Datei des Schriftverzeichnisses (unter Windows mehrere hundert);
/// UI-, Seitenlader- und Bildfaden haben ihn vorher je fuer sich gemacht.
pub fn systemschriften() -> Arc<Vec<SystemFont>> {
    static PROZESS: std::sync::OnceLock<Arc<Vec<SystemFont>>> = std::sync::OnceLock::new();
    SCHRIFTEN.with(|c| {
        let mut slot = c.borrow_mut();
        if let Some(liste) = slot.as_ref() {
            return Arc::clone(liste);
        }
        if let Some(liste) = PROZESS.get() {
            *slot = Some(Arc::clone(liste));
            return Arc::clone(liste);
        }
        let mut alle: Vec<SystemFont> = Vec::new();
        for wurzel in systemschrift_wurzeln() {
            if let Ok(liste) = find_system_fonts(&wurzel) {
                for f in liste {
                    // Doppelte Wurzeln (`/usr/share/fonts` und
                    // `…/fonts/truetype`) liefern dieselben Dateien
                    // zweimal; GEMESSEN ist das bei `/usr/share/fonts`
                    // und `/usr/share/fonts/truetype` der Fall.
                    if !alle.iter().any(|g| g.path == f.path) {
                        alle.push(f);
                    }
                }
            }
        }
        let arc = Arc::clone(PROZESS.get_or_init(|| Arc::new(alle)));
        *slot = Some(Arc::clone(&arc));
        arc
    })
}

/// Die **Datei**, die eine Familienliste ergibt — ohne sie zu laden.
///
/// GEMESSEN, warum die Auswahl hier und nicht in `narsil-raster`: der
/// Umbruch braucht dieselbe Datei, und `narsil-layout` kommt an
/// `narsil-raster` nicht heran. `Schrift::suche_familie` ruft genau diese
/// Funktion, also gibt es keine zweite Auswahl.
///
/// `None`, wenn die Liste leer ist oder nichts davon da ist — das ist kein
/// Fehler, sondern der Fall, in dem der Aufrufer die Vorgabe nimmt
/// (CSS Fonts 4 §4.2).
#[must_use]
pub fn datei_fuer_familie(familie: &str, fett: bool, kursiv: bool) -> Option<PathBuf> {
    datei_fuer_familie_mit_gewicht(familie, if fett { 700 } else { 400 }, kursiv)
}

/// Die gemeinsame Dateiauswahl mit der vollstaendigen CSS-Gewichtszahl.
#[must_use]
pub fn datei_fuer_familie_mit_gewicht(familie: &str, gewicht: u16, kursiv: bool) -> Option<PathBuf> {
    let liste = familie.trim();
    if liste.is_empty() { return None; }
    let gewicht = crate::system::normalisiere_gewicht(gewicht);
    DATEIEN.with(|c| {
        let schluessel = (liste.to_string(), gewicht, kursiv);
        if let Some(pfad) = c.borrow().get(&schluessel) {
            return pfad.clone();
        }
        let schriften = systemschriften();
        let pfad = familie_waehlen_mit_gewicht(&schriften, liste, gewicht, kursiv)
            .map(|f| f.path.clone());
        let mut karte = c.borrow_mut();
        const MAX_GEPUFFERTE_DATEIEN: usize = 256;
        if karte.len() >= MAX_GEPUFFERTE_DATEIEN { karte.clear(); }
        karte.insert(schluessel, pfad.clone());
        pfad
    })
}

/// Der **Stapel** fuer die Messung: die gewaehlte Datei zuerst, der
/// Systemstapel dahinter.
///
/// GEMESSEN, warum der Systemstapel dahinter gehoert: fehlt einem Zeichen
/// die Glyphe in der Hauptfamilie (Pfeile, CJK, Emoji), misst die Messung
/// sonst das Leerzeichen — und der Raster zeichnet es aus dem Stapel. Ein
/// Umbruch, der den Zeichenfall nicht kennt, produziert die Kasten aus
/// DEVPANEL_6.
#[must_use]
pub fn stapel_fuer_familie(familie: &str, gewicht: u16, kursiv: bool) -> Option<Arc<Schriftstapel>> {
    let stand = crate::webschrift::stand();
    let gemerkt = FERTIG.with(|c| {
        let c = c.borrow();
        if c.0 != stand {
            return None;
        }
        c.1.get(familie)?.iter().find(|e| e.0 == gewicht && e.1 == kursiv).map(|e| e.2.clone())
    });
    if let Some(s) = gemerkt {
        return s;
    }
    let s = stapel_suchen(familie, gewicht, kursiv);
    FERTIG.with(|c| {
        let mut c = c.borrow_mut();
        const MAX_FAMILY_LISTS: usize = 256;
        if c.0 != stand || c.1.len() >= MAX_FAMILY_LISTS {
            c.0 = stand;
            c.1.clear();
        }
        c.1.entry(familie.to_string()).or_default().push((gewicht, kursiv, s.clone()));
    });
    s
}

fn stapel_suchen(familie: &str, gewicht: u16, kursiv: bool) -> Option<Arc<Schriftstapel>> {
    // Release 09.10.2026: angemeldete Webschriften (`@font-face`) zuerst —
    // in der Reihenfolge der Liste (siehe `webschrift::finden`).
    if let Some(t) = crate::webschrift::finden(familie, gewicht, kursiv) {
        return Some(t.stapel);
    }
    let gewicht = crate::system::normalisiere_gewicht(gewicht);
    let pfad = datei_fuer_familie_mit_gewicht(familie, gewicht, kursiv)?;
    let fett = gewicht_ist_fett(gewicht);
    STAPEL.with(|c| {
        let schluessel = (pfad.clone(), fett, kursiv);
        if let Some(stapel) = c.borrow().get(&schluessel) {
            return Some(Arc::clone(stapel));
        }
        let stapel = stapel_bauen(&pfad, fett, kursiv)?;
        let mut karte = c.borrow_mut();
        // Fontdateien und Tabellen sind gross. Eine Seite mit immer neuen
        // Familien darf den thread-lokalen Cache nicht unbegrenzt fuellen.
        const MAX_GEPUFFERTE_STAPEL: usize = 16;
        if karte.len() >= MAX_GEPUFFERTE_STAPEL { karte.clear(); }
        karte.insert(schluessel, Arc::clone(&stapel));
        Some(stapel)
    })
}

fn stapel_bauen(pfad: &Path, fett: bool, kursiv: bool) -> Option<Arc<Schriftstapel>> {
    let mut stapel = Schriftstapel::neu();
    let kategorie = kategorie_von(pfad, fett, kursiv).unwrap_or(FontCategory::SansSerif);
    // Die gewaehlte Hauptschrift muss laden: ein leerer Ersatzstapel darf
    // nicht als Erfolg einer anderen Familie ausgegeben werden.
    let haupt = Stapelschrift::von_systemfont_pfad("haupt", pfad, kategorie)?;
    let _ = stapel.hinzufuegen(haupt);
    if let Ok(system) = system_stapel(kategorie, fett, kursiv) {
        for i in 0..system.len() {
            if let Some(sch) = system.schrift(i) {
                let _ = stapel.hinzufuegen(sch.clone());
            }
        }
    }
    Some(Arc::new(stapel))
}

/// Die Kategorie einer Datei — **gemessen** an ihren Zeichen, nicht am
/// Namen.
///
/// GEMESSEN 03.10.2026: `classify_font` (narsil-text system.rs) liest den
/// Dateinamen, und `NotoSansMono-Bold.ttf` traegt dort
/// `FontCategory::Monospace`. Fuer die Auswahl einer Familie ist aber die
/// **tatsaechliche** Breite die Eigenschaft, die zaehlt: ein Text in
/// `monospace` ist ein Text in einer Festbreitenschrift.
fn kategorie_von(pfad: &Path, fett: bool, kursiv: bool) -> Option<FontCategory> {
    let schriften = systemschriften();
    schriften
        .iter()
        .find(|f| f.path == pfad)
        .map(|f| f.category)
        .or_else(|| {
            let _ = (fett, kursiv);
            None
        })
}

#[cfg(test)]
mod font_contract_cache {
    use super::*;

    struct Schriftzustand {
        vorher: Option<Arc<Vec<SystemFont>>>,
        dateien: HashMap<(String, u16, bool), Option<PathBuf>>,
    }

    impl Schriftzustand {
        fn kontrolliert() -> Self {
            let fonts = [400, 600, 700].into_iter().map(|gewicht| SystemFont {
                path: PathBuf::from(format!("Alpha-{gewicht}.ttf")),
                family: "Alpha".into(),
                bold: gewicht >= 600,
                italic: false,
                category: FontCategory::SansSerif,
                us_weight_class: Some(gewicht),
                name_familie: Some("Alpha".into()),
                name_schnitt: Some("Regular".into()),
                name_typofamilie: Some("Alpha".into()),
                hat_buchstaben: true,
                hat_umrisse: true,
            }).collect();
            Self {
                vorher: SCHRIFTEN.with(|c| c.replace(Some(Arc::new(fonts)))),
                dateien: DATEIEN.with(|c| c.take()),
            }
        }
    }

    impl Drop for Schriftzustand {
        fn drop(&mut self) {
            SCHRIFTEN.with(|c| c.replace(self.vorher.take()));
            DATEIEN.with(|c| c.replace(std::mem::take(&mut self.dateien)));
        }
    }

    #[test]
    fn numeric_requests_do_not_alias_in_the_shared_file_cache() {
        let _zustand = Schriftzustand::kontrolliert();
        for _ in 0..2 {
            assert_eq!(datei_fuer_familie_mit_gewicht("Alpha", 600, false), Some(PathBuf::from("Alpha-600.ttf")));
            assert_eq!(datei_fuer_familie_mit_gewicht("Alpha", 700, false), Some(PathBuf::from("Alpha-700.ttf")));
            assert_eq!(datei_fuer_familie("Alpha", true, false), Some(PathBuf::from("Alpha-700.ttf")));
        }
        DATEIEN.with(|c| assert_eq!(c.borrow().len(), 2));
    }

    #[test]
    fn invalid_weight_and_normal_share_the_same_cache_entry() {
        let _zustand = Schriftzustand::kontrolliert();
        for gewicht in [0, 1001, u16::MAX, 400] {
            assert_eq!(datei_fuer_familie_mit_gewicht("Alpha", gewicht, false), Some(PathBuf::from("Alpha-400.ttf")));
        }
        DATEIEN.with(|c| assert_eq!(c.borrow().len(), 1));
    }

    #[test]
    fn numeric_weight_requests_cannot_grow_the_file_cache_without_bound() {
        let _zustand = Schriftzustand::kontrolliert();
        for gewicht in 1..=1000 {
            assert!(datei_fuer_familie_mit_gewicht("Alpha", gewicht, false).is_some());
            DATEIEN.with(|c| assert!(c.borrow().len() <= 256));
        }
    }
}
