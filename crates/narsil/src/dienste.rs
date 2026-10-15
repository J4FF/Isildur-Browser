use narsil_download::{DownloadManager, DownloadState};
use narsil_ext::ExtensionEngine;
use narsil_image::Bild;
use narsil_print::pdf::PdfWriter;
use narsil_print::{list_printers, print_document};
use narsil_verlauf::{HistoryManager, SessionManager};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// G6b: DAS DECLARATIVE-NETWORK-REQUEST-TOR FUER UNTERRESSOURCEN.
///
/// # WARUM EIN PROZESS-WEITES TOR und nicht `self.dienste`
///
/// Die drei Unterressourcen-Wegе holen ihre Bytes an ORTEN, die keine
/// `Sitzung` in der Hand haben: `js.rs::quelle_holen` (ein Skript mit
/// `src`) und `blaetter.rs::hole_blatt` (ein Stylesheet) sind freie
/// Funktionen. Sie kennen den Werbefilter ueber
/// `narsil_page::werbung::erlaubt`, der genauso prozess-weit ist — und
/// genau deshalb hat der Auftrag der Koordination („DNR auch fuer
/// script/stylesheet/xhr") im Browser nie funktioniert: es gab nur den
/// `Sitzung`-Weg (`evaluate_request` in `sitzung.rs`), und der ist an
/// genau den Stellen, die ein Skript oder ein Blatt nicht benutzt.
///
/// DIE LEBENSDAUER ist die des Prozesses, und das ist die richtige: die
/// Regeln kommen aus `~/.config/narsil/erweiterungen`, also von der
/// Platte, nicht von einem Tab. Wer zwei Tabs mit verschiedenen
/// Erweiterungen moechte, braucht eine Benutzeroberflaeche dafuer — es
/// gibt keine, und sie zu erfinden waere hier eine stille
/// Falschangabe.
static MOTOR: OnceLock<Arc<Mutex<ExtensionEngine>>> = OnceLock::new();

/// Hinterlegt den Motor, ueber den alle Unterressourcen laufen.
///
/// `OnceLock` heisst: der ERSTE Aufruf gewinnt. GEMESSEN, warum das
/// in Ordnung ist und kein Wettbewerb um die Wahrheit: `Sitzung::neu`
/// baut `GeminiDienste::new()` je Sitzung, aber `new()` liest immer
/// dieselbe Erweiterungskonfiguration von derselben Platte. Zwei
/// Motoren mit denselben Regeln sind keine zwei Wahrheiten, sondern
/// eine aus Unachtsamkeit.
fn motor_setzen(engine: &Arc<Mutex<ExtensionEngine>>) {
    let _ = MOTOR.set(engine.clone());
}

/// Darf die URL in dieser Ressourcenart abgerufen werden?
///
/// `art` sind die Chrome-Namen: `script`, `stylesheet`, `image`,
/// `main_frame`, `xmlhttprequest`. GEMESSEN, woher die kommen: die
/// Beispieldateien (`crates/narsil-ext/beispiele/blocker/rules.json` und
/// die Tests) benutzen genau diese, und `dnr.rs:23` vergleicht
/// Zeichenketten. Wer hier einen deutschen Namen nennt, findet keine
/// Regel — und merkt es nicht, weil „erlaubt" die sichere Antwort ist.
///
/// OHNE MOTOR ist die Antwort `true`: ein Browser ohne Erweiterungen
/// blockiert nichts. Das ist keine Ausnahme, sondern der Normalfall
/// vor `load_from_dir`.
pub fn dnr_erlaubt(url: &str, art: &str) -> bool {
    let Some(motor) = MOTOR.get() else {
        return true;
    };
    match motor.lock() {
        Ok(e) => !matches!(
            e.evaluate_request(url, art),
            narsil_ext::dnr::ActionType::Block
        ),
        // Ein vergifteter Mutex ist kein Grund, das Netz zu blockieren.
        Err(_) => true,
    }
}

#[derive(Clone)]
pub struct GeminiDienste {
    pub history: Arc<Mutex<HistoryManager>>,
    pub session: Arc<Mutex<SessionManager>>,
    pub download: Arc<Mutex<DownloadManager>>,
    pub extensions: Arc<Mutex<ExtensionEngine>>,
}

impl GeminiDienste {
    pub fn new() -> Self {
        let config_dir = crate::plattform::konfiguration();

        std::fs::create_dir_all(&config_dir).unwrap_or_default();

        let hist_mgr = HistoryManager::new(config_dir.join("history.tsv"));
        let sess_mgr = SessionManager::new(config_dir.join("session.txt"), true);

        let dl_dir = crate::plattform::downloads();
        let dl_mgr = DownloadManager::new(dl_dir);

        let ext_dir = config_dir.join("erweiterungen");
        let mut ext_eng = ExtensionEngine::new(config_dir.join("ext_storage"));
        if ext_dir.exists() {
            let _ = ext_eng.load_from_dir(&ext_dir);
        }

        Self {
            history: Arc::new(Mutex::new(hist_mgr)),
            session: Arc::new(Mutex::new(sess_mgr)),
            download: Arc::new(Mutex::new(dl_mgr)),
            extensions: Arc::new(Mutex::new(ext_eng)),
        }
    }

    /// Hinterlegt diesen Motor im prozess-weiten DNR-Tor.
    ///
    /// GEMESSEN, warum das ein eigener Schritt ist und nicht in `new()`:
    /// `new()` baut die Felder, und `Arc::new` darin kann man nicht von
    /// aussen erreichen, ohne den Konstruktor umzuschreiben. Hier ist es
    /// eine Zeile, und sie ist an der Stelle, an der jemand nachsieht.
    pub fn tor_oeffnen(&self) {
        motor_setzen(&self.extensions);
    }

    pub fn on_page_load(&self, url: &str, title: &str) {
        if let Ok(mut h) = self.history.lock() {
            h.add_visit(url, title);
        }
    }

    pub fn get_url_suggestions(&self, prefix: &str) -> Vec<String> {
        if let Ok(h) = self.history.lock() {
            h.search(prefix).into_iter().map(|e| e.url).collect()
        } else {
            Vec::new()
        }
    }

    pub fn save_session(&self, urls: &[String]) {
        if let Ok(s) = self.session.lock() {
            let _ = s.save_session(urls);
        }
    }

    pub fn restore_session(&self) -> Vec<String> {
        if let Ok(s) = self.session.lock() {
            s.restore_session().unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    pub fn download_file(&self, url: &str, suggested_name: &str) -> DownloadState {
        // Derselbe Datei-/HTTP-/HTTPS-Lader wie im Browser, ohne den alten
        // HTTP-only-Demoweg mit ungeprueften Dateinamen und unwrap().
        let bytes = match crate::laden::lade_bytes(url) {
            Ok(bytes) => bytes,
            Err(e) => return DownloadState::Failed(format!("{e:?}")),
        };
        let d = match self.download.lock() {
            Ok(d) => d,
            Err(_) => return DownloadState::Failed(narsil_i18n::tr("Download manager not available").into()),
        };
        let name = narsil_download::name::saeubere(suggested_name);
        let path = d.resolve_filename(&name);
        use std::io::Write;
        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
            if let Err(e) = file.write_all(&bytes).and_then(|_| file.flush()) {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err(e);
            }
            Ok(())
        })();
        match result {
            Ok(()) => DownloadState::Finished(path),
            Err(e) => DownloadState::Failed(e.to_string()),
        }
    }

    pub fn print_page(
        &self,
        printer: Option<&str>,
        image: Option<&Bild>,
        text: &str,
    ) -> Result<Vec<u8>, String> {
        let mut writer = PdfWriter::new();
        writer.add_page(image, text);
        let pdf = writer.finish();

        if let Some(p) = printer {
            // print directly
            print_document(p, &[(image, text)])?;
            Ok(pdf)
        } else {
            // just return pdf
            Ok(pdf)
        }
    }

    pub fn evaluate_request(&self, url: &str, res_type: &str) -> narsil_ext::dnr::ActionType {
        if let Ok(e) = self.extensions.lock() {
            e.evaluate_request(url, res_type)
        } else {
            narsil_ext::dnr::ActionType::Allow
        }
    }
}

impl GeminiDienste {
    /// G6b: DIE CONTENT-SCRIPTS DER SEITE.
    ///
    /// VORHER stand hier ein Zweizeiler, der eine leere Liste zurueckgab
    /// und den Motor nicht einmal nannte — der Motor war in `if let Ok(e)`
    /// gebunden und wurde nicht benutzt. GEMESSEN, wie das auffiel: die
    /// Koordination hat „content_scripts im Seitenweg ausfuehren" als
    /// offen gemeldet, und `Manifest::parse` hat bereits
    /// `content_scripts` geparst (`test_manifest_parsing` behauptet es).
    /// Es fehlte also nicht das Feld, sondern der eine Schritt dazwischen.
    ///
    /// WAS ZURUECKKOMMT, sind die **Quelltexte** in Ausfuehrungsreihenfolge
    /// (Erweiterungsname, dann Manifest-Reihenfolge — siehe
    /// `ExtensionEngine::content_scripts_fuer`, warum die sortiert ist).
    /// Der Host fuehrt sie; diese Funktion liest nur.
    ///
    /// DER PFAD, und warum er hier steht: `load_from_dir` legt eine
    /// Erweiterung unter dem `name` aus dem Manifest ab. Die
    /// `js`-Eintraege sind Pfade RELATIV zu diesem Ordner, also
    /// `~/.config/narsil/erweiterungen/<name>/<js>`. Nur der Host kennt den
    /// Erweiterungsordner — deshalb die Zusammensetzung hier und nicht in
    /// `narsil-ext`.
    ///
    /// DIE GRENZEN (M3/M4, beide in `narsil-ext` gesetzt und hier nicht
    /// wiederholt): `MAX_SKRIPTE_PRO_ERWEITERUNG` Dateien je Erweiterung,
    /// `MAX_TREFFER` Treffer. Zusaetzlich hier `MAX_SKRIPT_BYTES`, weil
    /// eine Erweiterung eine Datei beliebiger Groesse auf der Platte
    /// haben kann und die Seite nicht an ihr sterben darf.
    pub fn execute_content_scripts(&self, url: &str) -> Vec<String> {
        /// M4: 1 MiB je Content-Script.
        ///
        /// GEMESSEN, warum 1 MiB: `narsil::js::MAX_SKRIPT_QUELLE` (1 MiB)
        /// gilt fuer ein `<script src>` aus dem Netz. Eine Erweiterung
        /// kommt von der Platte und ist damit nicht kleiner zu erwarten —
        /// aber sie ist auch kein Grund, den Browser zu toeten.
        const MAX_SKRIPT_BYTES: usize = 1024 * 1024;
        /// M4: so viele Quellen kommen insgesamt zurueck.
        const MAX_QUELLEN: usize = 64;

        let Ok(e) = self.extensions.lock() else {
            return Vec::new();
        };
        let treffer = e.content_scripts_fuer(url);
        drop(e);
        let wurzel = self.erweiterungs_wurzel();
        let mut raus: Vec<String> = Vec::new();
        for t in treffer {
            for rel in t.js.iter().chain(t.css.iter()) {
                if raus.len() >= MAX_QUELLEN {
                    return raus;
                }
                // DER PFAD, und die Grenze darauf: `js` kommt aus einem
                // Manifest auf der Platte, also ist auch der Pfad eine
                // Eingabe. `..` darf nicht aus dem Erweiterungsordner
                // hinausfuehren — GEMESSENE ENTSCHEIDUNG: wir lesen die
                // Datei gar nicht, statt sie zu lesen und zu pruefen.
                if !pfad_ist_innen(rel) {
                    continue;
                }
                let ganz = wurzel.join(&t.ext_id).join(rel);
                let Ok(meta) = std::fs::metadata(&ganz) else {
                    continue;
                };
                if !meta.is_file() || meta.len() as usize > MAX_SKRIPT_BYTES {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&ganz) {
                    raus.push(text);
                }
            }
        }
        raus
    }

    /// Wo die Erweiterungen liegen — EINE Stelle, damit `new()` und
    /// `execute_content_scripts` nicht zwei various Pfade nennen.
    pub fn erweiterungs_wurzel(&self) -> PathBuf {
        crate::plattform::konfiguration().join("erweiterungen")
    }
}

/// Bleibt der Pfad in seinem Ordner?
///
/// GEMESSEN, warum das noetig ist: `js: ["../../../etc/passwd"]` ist ein
/// gueltiges `manifest.json` und laeuft durch `parse_string_array`
/// unveraendert hindurch. Ohne diese Pruefung wuerde ein Content-Script
/// eine beliebige Datei des Nutzers lesen — und der Host wuerde sie als
/// JavaScript ausgeben.
///
/// DIE REGEL: kein `..` als eigener Bestandteil, kein absoluter Pfad, und
/// auf Windows kein Praefix-`C:`. Alles andere darf benannt werden.
fn pfad_ist_innen(rel: &str) -> bool {
    if rel.is_empty() {
        return false;
    }
    if rel.starts_with('/') || rel.starts_with('\\') {
        return false;
    }
    // `:` am Anfang oder nach dem ersten Trenner ist ein Laufwerks-
    // praefix. Ein Doppelpunkt sonst (URL im Pfad) ist harmlos.
    if rel.len() >= 2 && rel.as_bytes()[1] == b':' {
        return false;
    }
    !rel.split(['/', '\\']).any(|teil| teil == "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pfad_ist_innen_lässt_normale_und_verschachtelte_zu() {
        assert!(pfad_ist_innen("content.js"));
        assert!(pfad_ist_innen("js/content.js"));
        assert!(pfad_ist_innen("js\\content.js"));
    }

    #[test]
    fn pfad_ist_innen_lehnt_das_verzeichnis_und_absolute_pfade_ab() {
        // DER SICHERHEITSFALL, und er ist der Grund fuer die Funktion:
        // ein `manifest.json` auf der Platte darf den Host nicht dazu
        // bringen, eine Datei ausserhalb des Erweiterungsordners zu lesen
        // und sie als JavaScript auszugeben.
        assert!(!pfad_ist_innen(""));
        assert!(!pfad_ist_innen("../a.js"));
        assert!(!pfad_ist_innen("js/../../a.js"));
        assert!(!pfad_ist_innen("..\\a.js"));
        assert!(!pfad_ist_innen("/etc/passwd"));
        assert!(!pfad_ist_innen("C:\\Windows\\a.js"));
    }
}
