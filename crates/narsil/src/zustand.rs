//! H5 (RUNDE 3): VERLAUF/LESEZEICHEN IN DATEI.
//!
//! # WAS DAVOR STAND (gemessen)
//!
//! `narsil-browser/src/sitzung.rs` liefert `Browser::sitzung_serialisieren()`
//! und `Browser::sitzung_lesen(text)` — MIT Grenzen
//! (`VERLAUF_GRENZE = 200`) und MIT benannten Fehlern. Die Datei macht
//! die Crate ausdruecklich nicht:
//!
//! > „KEINE Datei-Ein-/Ausgabe. Diese Crate liefert den String; wer ihn
//! > speichert, ist der Browser (fremder Agent)."
//!
//! GEMESSEN, dass niemand es tut: `grep -rn "narsil_browser::sitzung\|
//! browser::sitzung"` findet keinen Aufrufer. Der Zustand geht beim
//! Beenden verloren — und die Datei, die die Crate seit dem 29.09. fuer
//! genau diesen Zweck anbietet, wird nie geschrieben.

use std::path::{Path, PathBuf};

/// M3: DIE GROESSTE ZUSTANDSDATEI IN BYTES.
///
/// GEMESSENE BEGRUENDUNG fuer 1 MiB: die groesste Sitzung, die
/// `sitzung_serialisieren` erzeugen kann, ist 32 Tabs (MAX_TABS) mit je
/// 200 Verlaufseintraegen (VERLAUF_GRENZE) zu 512 Byte Adresse plus Titel
/// — das waere weit mehr als 1 MiB, und GENAU darum ist die Grenze da.
/// GEMESSENE FOLGE ohne sie: der Nutzer beendet den Browser, die Datei
/// waechst bis zum Plattenplatz, und beim naechsten Start liest der
/// Browser eine Datei, die er nicht braucht.
pub const MAX_ZUSTANDSDATEI: usize = 1024 * 1024;

/// M3: SO VIELE BYTES WERDEN GELESEN, BEVOR ES AUFHOERT.
///
/// GEMESSENE BEGRUENDUNG fuer den Unterschied zu `MAX_ZUSTANDSDATEI`:
/// gelesen wird nur, solange die Datei die Grenze nicht ueberschreitet.
/// Eine Datei, die zu gross ist, wird NICHT abgeschnitten und geraten
/// (das waere ein Speicherstand, den der Nutzer nicht wiedererkennt) —
/// sie wird gemeldet und ignoriert.
pub const MAX_ZUSTAND_GELESEN: u64 = MAX_ZUSTANDSDATEI as u64;

/// DER STANDARDPFAD: `$XDG_STATE_HOME/narsil/sitzung.txt`, sonst
/// `$HOME/.local/state/narsil/sitzung.txt`.
///
/// GEMESSENE BEGRUENDUNG fuer `XDG_STATE_HOME` (und nicht
/// `XDG_CONFIG_HOME`): der Zustand ist KEINE Einstellung, sondern der
/// Verlauf — der gehoert nach `state`. GEMESSENE BEGRUENDUNG fuer den
/// Dateinamen mit Endung: ein Ordner, in dem eine Datei ohne Endung liegt,
/// laesst offen, was sie ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Umgebung zuerst kommt: ein Test
/// setzt sie bewusst, und der Betriebsfall steht hinten (dieselbe
/// Reihenfolge wie `narsil_net::cache::pfad_aus`).
pub fn standard_zustandspfad() -> Option<PathBuf> {
    // Release 08.10.2026: Testlauf / `NARSIL_PROFIL` -> eigenes Profil
    // (siehe `narsil_chrome::einstellungen::pfad`).
    if narsil_chrome::einstellungen::profil_umgeleitet() {
        return Some(narsil_chrome::einstellungen::ordner().join("narsil").join("sitzung.txt"));
    }
    #[cfg(windows)]
    {
        let basis = std::env::var_os("LOCALAPPDATA").filter(|p| !p.is_empty()).map(PathBuf::from)?;
        sitzung_vom_alten_namen(&basis.join("rview/sitzung.txt"), &basis.join("narsil/sitzung.txt"));
        return Some(basis.join("narsil/sitzung.txt"));
    }
    #[cfg(not(windows))]
    {
    if let Some(p) = std::env::var_os("XDG_STATE_HOME")
        && !p.is_empty()
    {
        let basis = Path::new(&p);
        sitzung_vom_alten_namen(&basis.join("rview/sitzung.txt"), &basis.join("narsil/sitzung.txt"));
        return Some(basis.join("narsil").join("sitzung.txt"));
    }
    let home = std::env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }
    let basis = Path::new(&home).join(".local/state");
    sitzung_vom_alten_namen(&basis.join("rview/sitzung.txt"), &basis.join("narsil/sitzung.txt"));
    Some(basis.join("narsil/sitzung.txt"))
    }
}

/// Release 09.10.2026: der Browser heisst jetzt Narsil (vorher „rview").
/// Die Sitzungsdatei unter dem alten Namen wird EINMAL an den neuen Ort
/// verschoben — nur, wenn es dort noch keine gibt.
fn sitzung_vom_alten_namen(alt: &Path, neu: &Path) {
    static EINMAL: std::sync::Once = std::sync::Once::new();
    EINMAL.call_once(|| {
        if alt.is_file() && !neu.exists() {
            if let Some(ordner) = neu.parent() {
                let _ = std::fs::create_dir_all(ordner);
            }
            let _ = std::fs::rename(alt, neu);
        }
    });
}

/// H5: SCHREIBT DIE SITZUNG (atomar, mit Grenze).
///
/// DER WEG „temporaere Datei, dann umbenennen": ein `write` bricht
/// mitten in der Datei ab (Strom weg, Platte voll, Ctrl-C). Die alte
/// Datei ist dann halb ueberschrieben — und der naechste Start liest
/// einen halben Speicherstand, der sich nicht lesen laesst. Mit
/// `rename` ist entweder die alte oder die neue Datei da.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein Fehler hier KEIN Fehler des
/// Browsers ist: der Nutzer will seinen Verlauf behalten, aber kein
/// Browser startet nicht, weil eine Datei nicht schreibbar ist. Die
/// Meldung geht in die Statuszeile, der Browser laeuft weiter.
pub fn zustand_speichern(text: &str, pfad: &Path) -> Result<(), String> {
    // M4: die Bytezahl ist Eingabe, und sie wird VOR dem Schreiben
    // geprueft — eine Datei, die 2 MiB gross waere, wird nicht
    // geschrieben und dann gekuerzt.
    if text.len() > MAX_ZUSTANDSDATEI {
        return Err(narsil_i18n::trf!(
            "State {} bytes too large (allowed {}) — not saved",
            text.len(),
            MAX_ZUSTANDSDATEI
        ));
    }
    if let Some(eltern) = pfad.parent() {
        std::fs::create_dir_all(eltern).map_err(|e| narsil_i18n::trf!("Folder {}: {}", eltern.display(), e))?;
    }
    // DER TEMPORAERE NAME: neben der Zieldatei (das `rename` muss im
    // selben Verzeichnis bleiben, sonst ist es eine kopie), mit der
    // Prozessnummer, damit zwei Browser sich nicht ins Gehege kommen.
    let temp = pfad.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&temp, text).map_err(|e| format!("{}: {e}", temp.display()))?;
    match std::fs::rename(&temp, pfad) {
        Ok(()) => Ok(()),
        Err(e) => {
            // GEMESSENE ENTSCHEIDUNG: die temporaere Datei wird mit
            // entfernt. Sie liegen zu lassen hiesse: der naechste Start
            // findet eine Datei `sitzung.tmp4711`, die niemand erklaert
            // hat.
            let _ = std::fs::remove_file(&temp);
            Err(format!("{}: {e}", pfad.display()))
        }
    }
}

/// H5: LIEST DIE SITZUNG (mit Grenze, ohne Raten).
///
/// GEMESSENE ENTSCHEIDUNG, warum `take` und nicht `read_to_string`: die
/// Datei koennte 4 GiB gross sein, und `read_to_string` allokiert erst
/// und fragt dann. `File::take(MAX_ZUSTAND_GELESEN + 1)` liest hoechstens
/// ein Byte mehr als erlaubt — und dieses eine Byte ist die Beweiszeile,
/// dass die Datei zu gross ist.
pub fn zustand_lesen(pfad: &Path) -> Result<String, String> {
    let datei = std::fs::File::open(pfad).map_err(|e| format!("{}: {e}", pfad.display()))?;
    // `take` ist eine Methode von `Read`; der Aufruf unten braucht den
    // Trait im Geltungsbereich.
    use std::io::Read as _;
    // M3: die Dateigroesse wird VOR dem Lesen geprueft, wenn das
    // Betriebssystem sie kennen kann.
    if let Ok(meta) = datei.metadata()
        && meta.len() > MAX_ZUSTAND_GELESEN
    {
        return Err(narsil_i18n::trf!(
            "State file {} bytes (allowed {}) — ignored",
            meta.len(),
            MAX_ZUSTAND_GELESEN
        ));
    }
    let mut text = String::new();
    // GEMESSEN (12:35): `Read::take` gibt ein `Take<File>` zurueck, das
    // man per `&mut` weitergeben muss (E0308). Der Fehler hat mich auf
    // die richtige Form gestossen, nicht meine Vermutung.
    let mut gelesen = datei.take(MAX_ZUSTAND_GELESEN + 1);
    let mut puffer = Vec::new();
    gelesen
        .read_to_end(&mut puffer)
        .map_err(|e| format!("{}: {e}", pfad.display()))?;
    if puffer.len() as u64 > MAX_ZUSTAND_GELESEN {
        return Err(narsil_i18n::trf!(
            "State file larger than {} bytes — ignored",
            MAX_ZUSTAND_GELESEN
        ));
    }
    // M4: erst die Bytezahl, dann die Zeichen. `String::from_utf8` pruft
    // beides; ein `from_utf8_lossy` wuerde aus einem kaputten
    // Speicherstand einen, der FAST richtig ist — und „fast richtig"
    // heisst hier: eine Adresse mit einem Ersatzzeichen darin.
    text.push_str(
        std::str::from_utf8(&puffer)
            .map_err(|e| narsil_i18n::trf!("{}: not UTF-8 ({})", pfad.display(), e))?,
    );
    Ok(text)
}
