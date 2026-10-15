//! User paths shared by downloads, profiles and JavaScript storage.
use std::path::PathBuf;
pub fn heimverzeichnis() -> PathBuf {
    #[cfg(windows)]
    let name = "USERPROFILE";
    #[cfg(not(windows))]
    let name = "HOME";
    std::env::var_os(name).filter(|p| !p.is_empty())
        .map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}
/// Der Download-Ordner.
///
/// Release 08.10.2026: `NARSIL_DOWNLOADS` setzt ihn ausdruecklich (Messlaeufe,
/// `website_diagnose`), und ein Cargo-Testprogramm schreibt in den Ordner
/// `Downloads` SEINES Testprofils statt in den echten des Nutzers.
/// GEMESSEN vorher: „Konvertiert herunterladen", Drucken und „Seite
/// speichern" legten in Tests Dateien in `%USERPROFILE%\Downloads` ab.
pub fn downloads() -> PathBuf {
    if let Some(d) = std::env::var_os("NARSIL_DOWNLOADS").filter(|p| !p.is_empty()) {
        return PathBuf::from(d);
    }
    // Release 09.10.2026: die Einstellung „Downloads-Ordner" gilt. GEMESSEN
    // vorher: der Wert stand in `einstellungen.txt`, aber niemand las ihn —
    // Downloads, „Konvertiert herunterladen", Drucken und „Seite speichern"
    // landeten immer im Standardordner. Gelesen wird die Datei (jede Aenderung
    // wird sofort gespeichert), damit eine Aenderung ohne Neustart gilt.
    let e = narsil_chrome::einstellungen::lesen_von(&narsil_chrome::einstellungen::pfad()).unwrap_or_default();
    if let Some(d) = eingestellter_ordner(&e.downloads_ordner) {
        return d;
    }
    if narsil_chrome::einstellungen::laeuft_als_test() {
        return narsil_chrome::einstellungen::ordner().join("Downloads");
    }
    heimverzeichnis().join("Downloads")
}

/// Release 09.10.2026: der eingestellte Download-Ordner — `~` am Anfang ist
/// das Heimverzeichnis; leer (Systemvorgabe) oder nicht anlegbar heisst
/// `None` (dann gilt der Standardordner, statt dass der Download scheitert).
pub fn eingestellter_ordner(wert: &str) -> Option<PathBuf> {
    let w = wert.trim();
    if w.is_empty() {
        return None;
    }
    let p = match w.strip_prefix('~') {
        Some(rest) => heimverzeichnis().join(rest.trim_start_matches(['/', '\\'])),
        None => PathBuf::from(w),
    };
    std::fs::create_dir_all(&p).ok()?;
    p.is_dir().then_some(p)
}
pub fn konfiguration() -> PathBuf { narsil_chrome::einstellungen::ordner() }

/// Release 09.10.2026: Subpixel-Text (LCD) fuer die Leiste nach der
/// Systemeinstellung — unter Windows „ClearType-Text" mit seiner
/// Teilpixel-Reihenfolge, sonst Graustufen. `NARSIL_LCD=0|rgb|bgr`
/// entscheidet ausdruecklich (Messlaeufe, Vorher/Nachher-Bilder).
pub fn leisten_lcd_einstellen() {
    use narsil_raster::LcdOrdnung;
    let wahl = match std::env::var("NARSIL_LCD").ok().as_deref().map(str::trim) {
        Some("0") | Some("aus") => None,
        Some("rgb") | Some("1") => Some(LcdOrdnung::Rgb),
        Some("bgr") => Some(LcdOrdnung::Bgr),
        _ => system_lcd(),
    };
    narsil_raster::setze_lcd(wahl);
}

#[cfg(all(windows, feature = "desktop"))]
fn system_lcd() -> Option<narsil_raster::LcdOrdnung> {
    narsil_window::win32::cleartype_bgr().map(|bgr| {
        if bgr { narsil_raster::LcdOrdnung::Bgr } else { narsil_raster::LcdOrdnung::Rgb }
    })
}

#[cfg(not(all(windows, feature = "desktop")))]
fn system_lcd() -> Option<narsil_raster::LcdOrdnung> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Release 09.10.2026: die Einstellung „Downloads-Ordner" wird gelesen —
    /// leer heisst Standard, `~` ist das Heimverzeichnis, ein Pfad wird
    /// angelegt.
    #[test]
    fn der_eingestellte_download_ordner_gilt() {
        assert_eq!(eingestellter_ordner(""), None);
        assert_eq!(eingestellter_ordner("   "), None);
        let basis = std::env::temp_dir().join(format!("narsil-test-dl-ordner-{}", std::process::id()));
        let ziel = basis.join("neu");
        let _ = std::fs::remove_dir_all(&basis);
        let gefunden = eingestellter_ordner(&ziel.display().to_string()).expect("Ordner");
        assert_eq!(gefunden, ziel);
        assert!(ziel.is_dir(), "angelegt");
        assert_eq!(eingestellter_ordner("~"), Some(heimverzeichnis().join("")));
        let _ = std::fs::remove_dir_all(&basis);
    }
}
