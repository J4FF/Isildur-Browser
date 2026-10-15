//! Zugriffsprotokoll: eine Zeile je Antwort — im Speicher BEGRENZT
//! (M6) und auf Wunsch auch in einer Datei.
//!
//! # WARUM EIN RINGPUFFER UND KEINE ENDLOSE DATEI ALS STANDARD
//!
//! Ein Protokoll, das unbegrenzt wuesst, waere ein Speicherleck mit
//! Zeitstempel. [`MAX_PROTOKOLL_EINTRAEGE`] Eintraege als Ringpuffer,
//! das Aelteste faellt heraus — die Grenze steht, bevor die erste
//! Anfrage kommt.
//!
//! Wer es dauerhaft auf der Platte haben will, gibt
//! [`ProtokollConfig::datei`] an. AUCH diese Grenze steht vorher:
//! [`MAX_PROTOKOLLDATEI_BYTES`]. Danach wird nicht mehr geschrieben,
//! sondern gezaehlt ([`Protokoll::ausgelassen`]) — eine Festplatte,
//! die nie voll wird, ist dasselbe Versprechen wie ein Speicher, der
//! nie voll wird.
//!
//! # WAS DRINSTEHT
//!
//! Zeitpunkt (Unix-Sekunden), Peer, Methode, Pfad, Status,
//! Koerpergroesse, Dauer. Genau das, was eine Suche nach "warum war
//! das langsam" beantwortet — und nichts, was auf der Leitung waere.
//!
//! # WO ES EINGEHUNGEN WIRD
//!
//! Als zweites Glied der Kette, ausserhalb von Sicherheits-Header,
//! CORS und Rate-Limit (siehe `Server::mit_config`). So sieht das
//! Protokoll JEDE Antwort, auch `429` und `404` — ein Protokoll, das
//! nur Erfolge sieht, ist kein Protokoll.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use crate::http::{Request, Response};
use crate::middleware::Glied;
use std::sync::Arc;

/// Wie viele Eintraege der Puffer behaelt.
///
/// 1000 Eintraege zu hoechstens [`MAX_PFAD_ZEICHEN`] Bytes Pfad plus
/// Peer/Methode sind rund 400 KiB Obergrenze — immer da, nie mehr.
pub const MAX_PROTOKOLL_EINTRAEGE: usize = 1000;

/// Maximale Pfadlaenge EINES Protokolleintrags in Bytes.
///
/// Ein Pfad kann bis zu [`crate::MAX_ANFRAGEZEILE`] Bytes lang sein
/// (8 KiB). Tausend solcher Eintraege waeren 8 MiB — und die
/// Kuerzung muesste erst kommen, wenn der Speicher schon da ist.
/// Also: begrenzen VOR dem Speichern (M4).
pub const MAX_PFAD_ZEICHEN: usize = 256;

/// Maximale Peer-Laenge EINES Protokolleintrags in Bytes.
///
/// Vom Socket kommt eine Adresse, nicht ein Roman — aber
/// `Server::bediene` nimmt den Namen von aussen, und was von
/// aussen kommt, wird hier gekuerzt statt geglaubt (M4).
pub const MAX_PEER_ZEICHEN: usize = 64;

/// Obergrenze der Protokolldatei in Bytes.
///
/// Eine Zeile hat inklusive Pfad hoechstens ein paar hundert Bytes;
/// 4 MiB sind also zehntausende Zugriffe, bevor das Protokoll
/// anhaelt und nur noch zaehlt. Ohne diese Zahl waere `datei` eine
/// Einladung, die Platte bis zum Anschlag zu schreiben.
pub const MAX_PROTOKOLLDATEI_BYTES: u64 = 4 * 1024 * 1024;

/// Wie viele Generationen auf der Platte bleiben (added SERVER V8).
///
/// added 01.10.2026, SERVER V8: ohne Rotation ist die Alternative
/// „Datei voll, dann nichts mehr schreiben" — ein Protokoll, das
/// genau dann stirbt, wenn man es braucht. Mit Rotation gibt es
/// `server.log` (aktuell), `server.log.1` (vorher) und `server.log.2`
/// (vorvorher). Drei Generationen reichen, um einen Vorabend zu sehen,
/// und sind begrenzt, weil jede Datei bis [`MAX_PROTOKOLLDATEI_BYTES`]
/// wachsen kann.
pub const STANDARD_GENERATIONEN: u32 = 3;

/// Obergrenze fuer [`ProtokollConfig::generationen`] (M3/M6: eine
/// konfigurierbare Zahl braucht eine obere Grenze).
pub const MAX_GENERATIONEN: u32 = 16;

/// Was EINE Antwort im Protokoll kostet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eintrag {
    /// Wann die Antwort geschrieben wurde.
    pub zeit: SystemTime,
    /// Peer-Adresse wie vom Socket (kann leer sein).
    pub peer: String,
    /// Methode (`GET`, `POST`, ...).
    pub methode: String,
    /// Pfad ohne Query, auf [`MAX_PFAD_ZEICHEN`] Bytes gekuerzt.
    pub pfad: String,
    /// Statuscode.
    pub status: u16,
    /// Koerper in Bytes — bei einer Dateianswort die ausgelieferte
    /// Laenge, nicht die Dateigroesse auf der Platte.
    pub koerper: u64,
    /// Bearbeitungsdauer ab dem ersten Byte der Anfrage bis zur
    /// fertigen Antwort.
    pub dauer: Duration,
    /// Die Protokollversion der Anfrage (`HTTP/1.1`, `HTTP/2.0`,
    /// `HTTP/3.0`).
    ///
    /// added 02.10.2026, SERVER V9. DER BEFUND: der Test
    /// `h3_antwortet_durch_die_ganze_kette` prueft, dass ein HTTP/3-
    /// Zugriff protokolliert wird, und fand eine Zeile, in der
    /// nirgends stand, ueber welches Protokoll sie kam — dieselbe
    /// Zeile waere fuer einen HTTP/1.1-Zugriff ununterscheidbar.
    /// Bei drei Protokollen ist das kein Komfort, sondern die Frage
    /// „woher kommen diese 40 % 4xx?".
    ///
    /// Leer heisst „unbekannt" (z. B. bei einem Test, der die
    /// Struktur prueft). In [`Eintrag::zeile`] steht die Version nur
    /// dann, wenn sie nicht `HTTP/1.1` ist — sonst waere die
    /// haeufigste Zeile laenger als noetig und die Suche nach
    /// HTTP/1.1-Zugriffen braeuchte ein Muster, das nur
    /// gelegentlich greift.
    pub version: String,
}

impl Eintrag {
    /// Eine Zeile in der Form, die `grep` und `cut` verstehen:
    ///
    /// ```text
    /// 1790764800 | 127.0.0.1:54321 | GET /index.html | 200 | 1234 | 3
    /// ```
    ///
    /// Zeit in Unix-Sekunden: ohne Fremdcrate gibt es kein
    /// Datumsformat, das mehr wuerde als das hier — und Unix-Sekunden
    /// sind sortierbar, ohne dass jemand rechnen muss.
    pub fn zeile(&self) -> String {
        let zeit = self
            .zeit
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Die Version nur nennen, wenn sie von HTTP/1.1 abweicht (siehe
        // `Eintrag::version`).
        let version = if self.version.is_empty() || self.version == "HTTP/1.1" {
            String::new()
        } else {
            format!(" {}", self.version)
        };
        format!(
            "{} | {} | {} {}{} | {} | {} | {}",
            zeit,
            if self.peer.is_empty() {
                "-"
            } else {
                &self.peer
            },
            self.methode,
            if self.pfad.is_empty() {
                "-"
            } else {
                &self.pfad
            },
            version,
            self.status,
            self.koerper,
            self.dauer.as_millis()
        )
    }
}

/// Einstellungen fuer das Protokoll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtokollConfig {
    /// Eingeschaltet? `false` = kein Glied in der Kette, kein
    /// gespeicherter Eintrag — und damit auch kein Aufwand.
    pub an: bool,
    /// Optionale Datei, in die jede Antwort als Zeile kommt.
    /// `None` = nur im Speicher.
    pub datei: Option<PathBuf>,
    /// Groessengrenze der Datei in Byte. `None` =
    /// [`MAX_PROTOKOLLDATEI_BYTES`].
    ///
    /// added 01.10.2026, SERVER V8: konfigurierbar, weil ein
    /// Test eine kleine Datei braucht (sonst muesste er 4 MiB
    /// schreiben, um eine Rotation zu sehen — 8 MiB auf der Platte
    /// fuer einen Test ueber Dateigroessen).
    pub datei_max_bytes: Option<u64>,
    /// Anzahl der Generationen auf der Platte. `None` =
    /// [`STANDARD_GENERATIONEN`]. `0` = **keine** Rotation (die alte
    /// Form: bei voller Datei nur noch zaehlen).
    ///
    /// added SERVER V8.
    pub generationen: Option<u32>,
}

impl ProtokollConfig {
    /// Nur im Speicher.
    pub fn speicher() -> Self {
        ProtokollConfig {
            an: true,
            datei: None,
            datei_max_bytes: None,
            generationen: None,
        }
    }

    /// Im Speicher UND in einer Datei (anhaengend, wird bei
    /// Ueberschreiten von [`MAX_PROTOKOLLDATEI_BYTES`] still).
    pub fn mit_datei(pfad: impl Into<PathBuf>) -> Self {
        ProtokollConfig {
            an: true,
            datei: Some(pfad.into()),
            datei_max_bytes: None,
            generationen: None,
        }
    }

    /// Datei mit kleiner Grenze und Rotation (added SERVER V8).
    ///
    /// Der Weg fuer Tests und fuer kleine Installationen: die Grenze
    /// steht in Bytes, die Generationen in Stueck. Beides zusammen
    /// ergibt die Zusage „die Platte haelt hoechstens
    /// `max_bytes * (generationen + 1)`".
    pub fn mit_datei_und(pfad: impl Into<PathBuf>, max_bytes: u64, generationen: u32) -> Self {
        ProtokollConfig {
            an: true,
            datei: Some(pfad.into()),
            datei_max_bytes: Some(max_bytes.max(1)),
            generationen: Some(generationen.min(MAX_GENERATIONEN)),
        }
    }

    /// Aus. Die Rueckgabe ist gleichwertig zu `None` als Config-Wert,
    /// aber als Wert da — damit `ServerConfig` nie ein `Option<...>`
    /// zweiter Ordnung braucht.
    pub fn aus() -> Self {
        ProtokollConfig {
            an: false,
            datei: None,
            datei_max_bytes: None,
            generationen: None,
        }
    }
}

impl Default for ProtokollConfig {
    /// An, nur im Speicher: ein Protokoll ohne Aufwand auf der Platte.
    fn default() -> Self {
        ProtokollConfig::speicher()
    }
}

/// Alles unter EINEM Schloss — Datei mitzaehlend, weil Reihenfolge
/// und Groesse zusammen entscheiden, ob die Grenze greift.
#[derive(Default)]
struct Zustand {
    eintraege: VecDeque<Eintrag>,
    /// Alle Eintraege inklusive der herausgefallenen. Ohne diesen
    /// Zaehler waere "1000 von 12000" unsichtbar.
    gesamt: u64,
    /// Die Datei, wenn gewuenscht und wenn sie offen werden konnte.
    datei: Option<File>,
    /// Pfad, damit bei offenem Fehler nicht jeder Eintrag neu probiert.
    datei_pfad: Option<PathBuf>,
    /// Geschriebene Bytes (auch aus frueheren Laeufen, gemessen am
    /// vorhandenen Dateiende).
    datei_bytes: u64,
    /// Zeilen, die aus Grnd der Groessengrenze NICHT geschrieben
    /// wurden, plus solche, die die Datei nicht annahm.
    ausgelassen: u64,
    /// Der Versuch, die Datei zu oeffnen, ist gescheitert — nicht
    /// jeder Eintrag darf es noch einmal probieren.
    datei_defekt: bool,
    /// Groessengrenze in Byte (added SERVER V8).
    datei_max_bytes: u64,
    /// Generationen, die auf der Platte bleiben (added SERVER V8).
    generationen: u32,
    /// Wie oft rotiert wurde (added SERVER V8).
    rotationen: u64,
}

/// Der Puffer. Ein Schloss, weil eine Anfrage schreibt und der
/// Status-Aufruf liest — und weil ein vergiftetes Schloss den Server
/// nicht fuer immer stumm machen darf.
pub struct Protokoll {
    zustand: Mutex<Zustand>,
}

impl Protokoll {
    /// Leerer Puffer ohne Datei.
    pub fn neu() -> Self {
        Protokoll::mit_config(&ProtokollConfig::aus())
    }

    /// Nach Konfiguration: Puffer und (falls gewuenscht) Datei.
    pub fn mit_config(config: &ProtokollConfig) -> Self {
        let z = Zustand {
            datei_pfad: config.datei.clone(),
            datei_max_bytes: config
                .datei_max_bytes
                .unwrap_or(MAX_PROTOKOLLDATEI_BYTES)
                .max(1),
            generationen: config
                .generationen
                .unwrap_or(STANDARD_GENERATIONEN)
                .min(MAX_GENERATIONEN),
            ..Zustand::default()
        };
        Protokoll {
            zustand: Mutex::new(z),
        }
    }

    /// Einen Eintrag aufnehmen. Alles Aeltere faellt heraus; die
    /// Dateizeile entsteht hier, wenn eine da ist und Platz ist.
    pub fn schreibe(&self, e: Eintrag) {
        let mut z = self.lock();
        // --- Datei zuerst: sie bekommt IMMER die vollstaendige Zeile,
        // auch wenn der Speicherpuffer sie schon herausgeloest haette.
        if z.datei_pfad.is_some() && !z.datei_defekt {
            let zeile = e.zeile();
            self.datei_schreibe(&mut z, &zeile);
        }
        // --- dann der Puffer mit fester Grenze (M6).
        if z.eintraege.len() >= MAX_PROTOKOLL_EINTRAEGE {
            z.eintraege.pop_front();
        }
        z.eintraege.push_back(e);
        z.gesamt = z.gesamt.saturating_add(1);
    }

    /// Die letzten Eintraege, aelteste zuerst.
    pub fn eintraege(&self) -> Vec<Eintrag> {
        self.lock().eintraege.iter().cloned().collect()
    }

    /// Wie viele Eintraege gerade im Puffer liegen.
    pub fn anzahl(&self) -> usize {
        self.lock().eintraege.len()
    }

    /// Wie viele Eintraege insgesamt kamen (inkl. herausgefallener).
    pub fn gesamt(&self) -> u64 {
        self.lock().gesamt
    }

    /// Wie viele Zeilen die Datei NICHT angenommen hat (Grenze oder
    /// Schreibfehler). Null heisst: alles ist auf der Platte.
    pub fn ausgelassen(&self) -> u64 {
        self.lock().ausgelassen
    }

    /// Wie viele Bytes die Protokolldatei jetzt hat (auch aus
    /// frueheren Laeufen). Null, wenn keine gewuenscht wird.
    pub fn datei_bytes(&self) -> u64 {
        self.lock().datei_bytes
    }

    /// Den Puffer leeren (Datei bleibt, was dasteht, steht dasteht).
    pub fn leere(&self) {
        let mut z = self.lock();
        z.eintraege.clear();
        z.gesamt = 0;
    }

    /// Alle Zeilen als String — der Weg nach draussen fuer alle, die
    /// selbst entscheiden wollen, wohin es geht.
    pub fn als_zeilen(&self) -> String {
        let mut raus = String::new();
        for e in self.lock().eintraege.iter() {
            raus.push_str(&e.zeile());
            raus.push('\n');
        }
        raus
    }

    /// Eine Zeile in die Datei schreiben — mit allen Grenzen, die
    /// davor stehen.
    fn datei_schreibe(&self, z: &mut Zustand, zeile: &str) {
        let bedarf = zeile.len() as u64 + 1; // + '\n'
        // Offen, wenn noch nie probiert. Fehler zaehlen, NICHT panic:
        // ein Protokoll, das den Server abstuerzen laesst, ist der
        // falsche Dienst.
        if z.datei.is_none() {
            if let Some(pfad) = z.datei_pfad.clone() {
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&pfad)
                {
                    Ok(f) => {
                        // Vorhandene Groesse zaehlt mit — sonst waere
                        // die Grenze bei jedem Neustart wieder offen.
                        z.datei_bytes = f.metadata().map(|m| m.len()).unwrap_or(0);
                        z.datei = Some(f);
                    }
                    Err(_) => {
                        z.datei_defekt = true;
                        z.ausgelassen = z.ausgelassen.saturating_add(1);
                        return;
                    }
                }
            }
        }
        if z.datei_bytes.saturating_add(bedarf) > z.datei_max_bytes {
            // added 01.10.2026, SERVER V8: ROTATION statt Stille.
            //
            // DER BEFUND: ohne Rotation ist die Alternative „Datei voll,
            // dann nichts mehr schreiben" — ein Protokoll, das genau
            // dann stirbt, wenn der Server etwas Interessantes tut.
            // Also: `log` -> `log.1`, `log.1` -> `log.2`, …, und die
            // aelteste Generation faellt raus. Danach ist die Datei
            // leer und es geht weiter.
            //
            // DIE REIHENFOLGE ist absteigend, damit keine Datei zwei
            // Namen hat: erst `.2`, dann `.1`, dann die aktuelle. Wer
            // es aufsteigend macht, verschwindet der Inhalt von `log`
            // in `log.2`, und `log.1` ist leer.
            //
            // `generationen == 0` heisst ausdruecklich: keine
            // Rotation (die alte Form, mit `ausgelassen`-Zaehler).
            if z.generationen == 0 {
                z.ausgelassen = z.ausgelassen.saturating_add(1);
                return;
            }
            // Der offene Handle muss weg, BEVOR umbenannt wird: unter
            // Windows verhindert das eine Rename, unter Linux nicht,
            // aber der Portabilitaetsgrund reicht.
            z.datei = None;
            if let Some(pfad) = z.datei_pfad.clone() {
                // Die aelteste Generation erst entfernen, dann
                // absteigend umbenennen. `u32` statt `usize`, weil die
                // Zahl begrenzt ist (MAX_GENERATIONEN).
                let mut g = z.generationen;
                while g >= 1 {
                    let ziel = pfad.with_extension(format!(
                        "{}.{g}",
                        pfad.extension().and_then(|e| e.to_str()).unwrap_or("log")
                    ));
                    if g == z.generationen {
                        let _ = std::fs::remove_file(&ziel);
                    } else {
                        let von = pfad.with_extension(format!(
                            "{}.{g}",
                            pfad.extension().and_then(|e| e.to_str()).unwrap_or("log")
                        ));
                        let _ = std::fs::rename(&von, &ziel);
                    }
                    if g == 1 {
                        let _ = std::fs::rename(&pfad, &ziel);
                        break;
                    }
                    g -= 1;
                }
                // Und die aktuelle Datei neu anlegen. Schlaegt das
                // fehl, ist `datei_defekt` die Folge und der Puffer
                // laeuft weiter — ein Protokoll, das den Server
                // anhaelt, waere der falsche Dienst.
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&pfad)
                {
                    Ok(f) => {
                        z.datei_bytes = f.metadata().map(|m| m.len()).unwrap_or(0);
                        z.datei = Some(f);
                        z.rotationen = z.rotationen.saturating_add(1);
                    }
                    Err(_) => {
                        z.datei_defekt = true;
                        z.ausgelassen = z.ausgelassen.saturating_add(1);
                        return;
                    }
                }
            }
        }
        if let Some(f) = z.datei.as_mut() {
            if f.write_all(zeile.as_bytes()).is_ok() && f.write_all(b"\n").is_ok() {
                z.datei_bytes = z.datei_bytes.saturating_add(bedarf);
            } else {
                z.ausgelassen = z.ausgelassen.saturating_add(1);
            }
        }
    }

    /// Wie oft rotiert wurde (added SERVER V8).
    ///
    /// Null heisst „nie" — auch dann, wenn das Protokoll auf der
    /// Platte steht und nichts ausgelassen hat. Ein Test liest das
    /// hier, statt an Dateinamen zu raten.
    pub fn rotationen(&self) -> u64 {
        self.lock().rotationen
    }

    /// Die konfigurierte Groessengrenze der Datei in Byte (added V8).
    pub fn datei_max_bytes(&self) -> u64 {
        self.lock().datei_max_bytes
    }

    /// Die konfigurierte Zahl der Generationen (added V8).
    pub fn generationen(&self) -> u32 {
        self.lock().generationen
    }

    /// Schloss holen, ohne paniken zu koennen (gleiche Regel wie
    /// `server::Warteschlange::lock`).
    fn lock(&self) -> std::sync::MutexGuard<'_, Zustand> {
        self.zustand.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Default for Protokoll {
    fn default() -> Self {
        Protokoll::neu()
    }
}

// ---------------------------------------------------------------------------
// Das Middleware-Glied
// ---------------------------------------------------------------------------

/// Ein Glied, das jede Antwort mit Dauer und Groesse ins Protokoll
/// schreibt.
///
/// Die Uhr laeuft um `weiter()` herum — also um Router, Handler und
/// alle inneren Glieder. Was ausserhalb steht (Zaehlen, dies hier),
/// kostet die Dauer nicht, sonst misst das Protokoll sich selbst.
pub fn glied(protokoll: Arc<Protokoll>) -> Glied {
    Arc::new(move |anfrage, weiter| {
        let start = Instant::now();
        let antwort = weiter(anfrage);
        protokoll.schreibe(eintrag_aus(anfrage, &antwort, start.elapsed()));
        antwort
    })
}

/// Eintrag aus Anfrage + Antwort bauen. Fuer Tests und fuer das Glied.
///
/// Die Koerpergroesse kommt bei einer Dateianswort aus `datei.laenge`,
/// nicht aus `body` — sonst protokollierte jede statische Seite
/// "0 Bytes", waehrend auf der Leitung Tausende waren.
pub fn eintrag_aus(anfrage: &Request, antwort: &Response, dauer: Duration) -> Eintrag {
    let koerper = match &antwort.datei {
        Some(d) => d.laenge,
        None => antwort.body.len() as u64,
    };
    Eintrag {
        zeit: SystemTime::now(),
        peer: kuerze(&anfrage.peer, MAX_PEER_ZEICHEN),
        methode: anfrage.methode.als_str().to_string(),
        pfad: kuerze(&anfrage.pfad, MAX_PFAD_ZEICHEN),
        status: antwort.status.0,
        koerper,
        dauer,
        version: anfrage.version.clone(),
    }
}

/// Auf `max` Bytes kuerzen, auf CHARAKTERGRENZE, nie mitten im UTF-8.
///
/// Es entsteht kein zusammenhaengender `Vec` mit der vollen Laenge —
/// gekuerzt wird, bevor gespeichert wird (M4).
fn kuerze(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut g = max;
    while g > 0 && !text.is_char_boundary(g) {
        g -= 1;
    }
    text[..g].to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, Status};

    fn anfrage(pfad: &str) -> Request {
        Request {
            methode: Method::Get,
            methode_roh: "GET".into(),
            pfad: pfad.to_string(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: Default::default(),
            body: Vec::new(),
            params: Default::default(),
            peer: "127.0.0.1:1234".into(),
        }
    }

    fn antwort() -> Response {
        Response {
            status: Status(200),
            headers: Vec::new(),
            body: vec![0u8; 42],
            datei: None,
        }
    }

    fn test_verzeichnis(name: &str) -> PathBuf {
        use std::path::Path;
        // M9: nie /tmp — Testdaten unter /home/jaff/mutation_md/server/.
        let ordner = Path::new("/home/jaff/mutation_md/server");
        std::fs::create_dir_all(ordner).expect("Testordner anlegbar");
        let ziel = ordner.join(name);
        let _ = std::fs::remove_file(&ziel);
        ziel
    }

    #[test]
    fn puffer_waechst_nie_ueber_max_eintraege_hinaus() {
        let p = Protokoll::neu();
        for i in 0..(MAX_PROTOKOLL_EINTRAEGE + 250) {
            p.schreibe(eintrag_aus(
                &anfrage(&format!("/p{i}")),
                &antwort(),
                Duration::ZERO,
            ));
            assert!(p.anzahl() <= MAX_PROTOKOLL_EINTRAEGE);
        }
        assert_eq!(p.anzahl(), MAX_PROTOKOLL_EINTRAEGE);
        assert_eq!(p.gesamt(), (MAX_PROTOKOLL_EINTRAEGE + 250) as u64);
        // Der aelteste ist raus, der juengste drin.
        let e = p.eintraege();
        assert_eq!(e.first().unwrap().pfad, "/p250");
        assert_eq!(
            e.last().unwrap().pfad,
            format!("/p{}", MAX_PROTOKOLL_EINTRAEGE + 249)
        );
    }

    #[test]
    fn pfad_und_peer_werden_vor_dem_speichern_gekuerzt() {
        let p = Protokoll::neu();
        let lang = "/".to_string() + &"a".repeat(20_000);
        p.schreibe(eintrag_aus(&anfrage(&lang), &antwort(), Duration::ZERO));
        assert_eq!(p.eintraege()[0].pfad.len(), MAX_PFAD_ZEICHEN);

        // Umlaute: gekuerzt, aber NIE mitten im UTF-8.
        let umlaute = "/".to_string() + &"ä".repeat(5_000);
        let mut a = anfrage(&umlaute);
        a.peer = "p".repeat(5_000);
        p.schreibe(eintrag_aus(&a, &antwort(), Duration::ZERO));
        let e = p.eintraege();
        let letzter = e.last().unwrap();
        assert!(letzter.pfad.len() <= MAX_PFAD_ZEICHEN);
        assert!(letzter.peer.len() <= MAX_PEER_ZEICHEN);
        assert!(std::str::from_utf8(letzter.pfad.as_bytes()).is_ok());
        assert!(std::str::from_utf8(letzter.peer.as_bytes()).is_ok());
    }

    #[test]
    fn dateianswort_protokolliert_die_ausgelieferte_laenge() {
        use crate::http::DateiLesen;
        let mut a = antwort();
        a.body.clear(); // Koerper steckt in `datei`, nicht in `body`
        a.datei = Some(DateiLesen {
            pfad: PathBuf::from("/home/jaff/Bilder/Browser/projekt/bilder/messung.png"),
            offset: 0,
            laenge: 7_654_321,
            quelle: None,
        });
        let e = eintrag_aus(&anfrage("/bild.png"), &a, Duration::ZERO);
        assert_eq!(e.koerper, 7_654_321, "nicht 0, obwohl body leer ist");
    }

    #[test]
    fn zeile_enthält_alles_wofuer_eine_suche_steht() {
        let p = Protokoll::neu();
        p.schreibe(eintrag_aus(
            &anfrage("/lade/langsam"),
            &antwort(),
            Duration::from_millis(123),
        ));
        let zeile = p.als_zeilen();
        assert!(zeile.contains("| GET /lade/langsam |"), "{zeile}");
        assert!(zeile.contains("| 200 | 42 | 123"), "{zeile}");
        assert!(zeile.contains("127.0.0.1:1234"), "{zeile}");
        assert!(zeile.ends_with('\n'));
        // Genau EINE Zeile — kein Protokoll, das sich selbst vervielfacht.
        assert_eq!(zeile.lines().count(), 1);
    }

    #[test]
    fn leeren_setzt_auch_den_gesamtzaehler_zurueck() {
        let p = Protokoll::neu();
        for i in 0..10 {
            p.schreibe(eintrag_aus(
                &anfrage(&format!("/{i}")),
                &antwort(),
                Duration::ZERO,
            ));
        }
        assert_eq!(p.gesamt(), 10);
        p.leere();
        assert_eq!(p.anzahl(), 0);
        assert_eq!(p.gesamt(), 0);
        assert_eq!(p.als_zeilen(), "");
    }

    #[test]
    fn datei_bekommt_eine_zeile_je_antwort() {
        let ziel = test_verzeichnis("protokoll_echte_zeilen.log");
        let p = Protokoll::mit_config(&ProtokollConfig::mit_datei(ziel.clone()));
        for i in 0..5 {
            p.schreibe(eintrag_aus(
                &anfrage(&format!("/seite{i}")),
                &antwort(),
                Duration::from_millis(i),
            ));
        }
        assert_eq!(p.ausgelassen(), 0, "die Grenze liegt weit daneben");
        assert!(p.datei_bytes() > 0);

        let text = std::fs::read_to_string(&ziel).expect("Datei lesbar");
        assert_eq!(text.lines().count(), 5, "{text}");
        assert!(text.contains("| GET /seite0 | 200 | 42 | 0"), "{text}");
        assert!(text.contains("| GET /seite4 | 200 | 42 | 4"), "{text}");

        // Anhaengend: ein zweiter Lauf fuegt hinzu, ueberschreibt nicht.
        p.schreibe(eintrag_aus(&anfrage("/spater"), &antwort(), Duration::ZERO));
        let text = std::fs::read_to_string(&ziel).expect("Datei lesbar");
        assert_eq!(text.lines().count(), 6);
        assert!(text.lines().next().unwrap().contains("/seite0"));

        // Alles weg: nur die Rueckfrage, nicht der Zustand der Datei.
        p.leere();
        assert_eq!(p.gesamt(), 0);
        assert_eq!(
            std::fs::read_to_string(&ziel).unwrap().lines().count(),
            6,
            "leeren loescht NICHT die Datei"
        );
        let _ = std::fs::remove_file(&ziel);
    }

    #[test]
    fn leere_verzeichnis_wird_gezaehlt_nicht_panikt() {
        // Schreibunmoeglicher Pfad: der Server darf nicht sterben.
        let p = Protokoll::mit_config(&ProtokollConfig::mit_datei(
            "/home/jaff/mutation_md/server/gibts_nicht_als_ordner/x.log",
        ));
        p.schreibe(eintrag_aus(&anfrage("/probe"), &antwort(), Duration::ZERO));
        assert_eq!(p.anzahl(), 1, "Puffer funktioniert trotzdem");
        assert_eq!(p.ausgelassen(), 1, "Fehler wird gezaehlt, nicht gepanikt");
        // Beim ZWEITEN Eintrag wird nicht erneut probiert.
        p.schreibe(eintrag_aus(&anfrage("/zwei"), &antwort(), Duration::ZERO));
        assert_eq!(p.ausgelassen(), 1, "kein Versuch pro Eintrag");
    }

    /// V8: DIE ROTATION, mit echten Dateien und einer kleinen Grenze.
    ///
    /// DER WEG, und warum die Grenze 512 Byte ist: bei 4 MiB (der
    /// Vorgabe) muesste der Test 8 MiB schreiben, um drei Rotationen
    /// zu sehen. Eine Zeile ist rund 60 Byte, also braucht 512 Byte
    /// etwa acht Zeilen — schnell genug, um vor dem Testende fertig
    /// zu sein, und klein genug, um auf der Platte nichts zu
    /// hinterlassen.
    #[test]
    fn datei_rotiert_statt_zu_verstummen() {
        use std::path::Path;
        let ordner = Path::new("/home/jaff/mutation_md/server");
        std::fs::create_dir_all(ordner).expect("Testordner");
        let ziel = ordner.join("v8_rotation.log");
        for g in 0..=STANDARD_GENERATIONEN {
            let _ = std::fs::remove_file(ordner.join(format!("v8_rotation.log.{g}")));
        }
        let _ = std::fs::remove_file(&ziel);

        let grenze = 512u64;
        let p = Protokoll::mit_config(&ProtokollConfig::mit_datei_und(
            ziel.clone(),
            grenze,
            STANDARD_GENERATIONEN,
        ));
        assert_eq!(p.datei_max_bytes(), grenze);
        assert_eq!(p.generationen(), STANDARD_GENERATIONEN);

        // 60 Zeilen: rund 3.600 Byte, das sind bei 512 Byte etwa
        // sieben Rotationen — mehr als `STANDARD_GENERATIONEN`, also
        // muss die aelteste Generation verschwinden.
        for i in 0..60 {
            p.schreibe(eintrag_aus(
                &anfrage(&format!("/seite/{i}")),
                &antwort(),
                Duration::from_millis(i as u64),
            ));
        }
        let rotationen = p.rotationen();
        assert!(
            rotationen >= 3,
            "nur {rotationen} Rotationen bei 60 Zeilen und 512 Byte Grenze"
        );
        assert_eq!(
            p.ausgelassen(),
            0,
            "mit Rotation wird nichts ausgelassen — das ist der ganze Unterschied"
        );

        // Und jetzt die Zusage der Platte: hoechstens
        // `generationen + 1` Dateien, jede hoechstens `grenze` gross
        // (die letzte kann angebrochen sein, das ist erlaubt).
        let mut vorhanden: Vec<(String, u64)> = Vec::new();
        if let Ok(eintraege) = std::fs::read_dir(ordner) {
            for e in eintraege.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with("v8_rotation.log") {
                    let groesse = e.metadata().map(|m| m.len()).unwrap_or(0);
                    vorhanden.push((name, groesse));
                }
            }
        }
        vorhanden.sort();
        assert!(
            vorhanden.len() as u32 <= STANDARD_GENERATIONEN + 1,
            "{} Dateien auf der Platte: {:?}",
            vorhanden.len(),
            vorhanden
        );
        for (name, groesse) in &vorhanden {
            assert!(
                *groesse <= grenze,
                "{name} ist {groesse} Byte gross, erlaubt sind {grenze}"
            );
        }
        println!("Rotation: {vorhanden:?}");

        // Und die inhaltliche Probe: die aktuelle Datei traegt die
        // LETZTEN Zeilen, nicht die ersten. Sonst waere die Rotation
        // eine Kopie, und das Log zeigte einen Anfang ohne Ende.
        let aktuell = std::fs::read_to_string(&ziel).expect("aktuelle Datei");
        let zeilen: Vec<&str> = aktuell.lines().collect();
        assert!(
            !zeilen.is_empty(),
            "die aktuelle Datei ist leer — es wurde nicht weitergeschrieben"
        );
        assert!(
            zeilen.last().unwrap().contains("/seite/59"),
            "die letzte Zeile ist {:?}, erwartet /seite/59",
            zeilen.last()
        );

        // Und die Kette: `.1` ist aelter als die aktuelle Datei. Das
        // prueft man am Inhalt, nicht an den Namen.
        let eins =
            std::fs::read_to_string(ordner.join("v8_rotation.log.1")).expect("Generation .1");
        assert!(
            eins.lines().last().unwrap() < zeilen.last().unwrap(),
            "die Generationen sind nicht in Zeitordnung"
        );

        for g in 0..=STANDARD_GENERATIONEN {
            let _ = std::fs::remove_file(ordner.join(format!("v8_rotation.log.{g}")));
        }
        let _ = std::fs::remove_file(&ziel);
    }

    /// `generationen = 0` heisst ausdruecklich „keine Rotation" — die
    /// alte Form, mit Zaehler. Ohne diesen Test waere `0` eine
    /// stille Verschlechterung.
    #[test]
    fn generationen_null_zaehlt_statt_zu_rotieren() {
        use std::path::Path;
        let ordner = Path::new("/home/jaff/mutation_md/server");
        std::fs::create_dir_all(ordner).expect("Testordner");
        let ziel = ordner.join("v8_keine_rotation.log");
        let _ = std::fs::remove_file(&ziel);
        let _ = std::fs::remove_file(ordner.join("v8_keine_rotation.log.1"));

        let p = Protokoll::mit_config(&ProtokollConfig::mit_datei_und(ziel.clone(), 512, 0));
        for i in 0..60 {
            p.schreibe(eintrag_aus(
                &anfrage(&format!("/seite/{i}")),
                &antwort(),
                Duration::ZERO,
            ));
        }
        assert_eq!(p.rotationen(), 0, "keine Rotation bei generationen = 0");
        assert!(
            p.ausgelassen() > 0,
            "ohne Rotation muss ausgelassen gezaehlt werden"
        );
        assert!(
            !ordner.join("v8_keine_rotation.log.1").exists(),
            "es darf keine .1-Datei geben"
        );
        let _ = std::fs::remove_file(&ziel);
    }
}
