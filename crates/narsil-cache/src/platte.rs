//! Der Platte-Cache unter `~/.cache/narsil` (oder `$XDG_CACHE_HOME/narsil`).
//!
//! **Warum eine Platte:** der Speicher-Cache ist fluechtig — ein Neustart
//! des Browsers loescht ihn. Die Platte ueberlebt den Neustart, und genau
//! das ist der Unterschied zwischen "nochmal alles laden" und "sofort da".
//!
//! **Warum ein Hash als Dateiname:** eine URL als Dateiname ist (a) laenger
//! als das Dateisystem erlaubt, (b) ein Weg aus dem Verzeichnis (`..`, `/`)
//! und (c) eine Kollisionsstelle. `SHA-256(url)` in Hex ist 64 Zeichen,
//! passt ueberall und kollidiert nicht.
//!
//! **Das Dateiformat** ist length-praefixed, damit der Index beim Start
//! aus den ersten 36 Bytes gelesen werden kann, ohne die Datei zu parsen:
//!
//! ```text
//! [4]  Magie "RWC1"
//! [8]  Empfangszeit in Unix-Millisekunden (u64 LE)
//! [2]  Statuscode (u16 LE)
//! [2]  reserviert
//! [4]  Anzahl Header (u32 LE)
//! [4]  Laenge des Header-Blocks (u32 LE)
//! [4]  Anzahl Vary-Paare (u32 LE)
//! [4]  Laenge des Vary-Blocks (u32 LE)
//! [4]  Laenge des Koerpers (u32 LE)
//! ...  Header-Block: je [4] name_len, name, [4] wert_len, wert
//! ...  Vary-Block:   dasselbe
//! ...  Koerper
//! ```
//!
//! Alle Laengen werden VOR jeder Allokation geprueft (M4). Eine Datei, die
//! mehr behauptet, als `MAX_DATEI_BYTES` erlaubt, wird ignoriert, nicht
//! gelesen.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::sha256::sha256_hex;
use crate::speicher::{Eintrag, Reihenfolge};

/// Obergrenze einer einzelnen Cache-Datei (M4).
///
/// Eine Antwort, die groesser ist, kommt nicht auf die Platte. Das verhindert,
/// dass ein einzelner Eintrag das ganze Kontingent frisst.
const MAX_DATEI_BYTES: u64 = 64 * 1024 * 1024;

/// Obergrenze der Dateien im Verzeichnis (M3/M6).
const MAX_DATEIEN: usize = 10_000;

/// Der Datei-Kopf in Byte — die ersten 36 Bytes jeder Cache-Datei.
const KOPF_LAENGE: usize = 36;

/// Release 10.10.2026: das Verzeichnis der GEPRUEFTEN Dateien (Name, Groesse,
/// Aenderungszeit, Laenge aus dem Kopf). Beim Start wird nur geoeffnet, was
/// darin fehlt oder sich seitdem geaendert hat.
///
/// GEMESSEN vorher: der Start oeffnete JEDE Datei des Caches und las ihren
/// Kopf — bei 3 024 Dateien im Profil des Nutzers 1,1 s, bezahlt von der
/// ersten Anfrage jedes Programmstarts.
const INDEX_DATEI: &str = "index.rwi";

/// Die Aenderungszeit als Zahl (ns seit 1970) fuer das Verzeichnis.
fn zeitstempel(t: SystemTime) -> u128 {
    t.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

/// Platten-Cache in einem Verzeichnis.
pub struct Platte {
    pfad: PathBuf,
    grenze: u64,
    belegt: u64,
    reihenfolge: Reihenfolge,
    /// Dateigroesse je Schluessel — damit `verdraenge` nicht auf
    /// `fs::metadata` angewiesen ist (das die Datei nach dem Schreiben
    /// nicht immer findet, was ein dauerhafter Zustand war).
    groessen: std::collections::HashMap<String, u64>,
    /// Die Zaehler der Abnahme (18:30, Koordination: „zweiter Besuch
    /// laedt aus dem Cache (Zaehler + Ladezeit vorher/nachher)").
    ///
    /// GEMESSENER ANLASS: `Platte` hatte KEINE Treffer-/Miss-Zaehlung.
    /// Die Abnahme verlangt eine Zahl, und es gab keine — also war
    /// „laedt aus dem Cache" bisher nur an einer Datei zu sehen, nicht
    /// zu messen.
    ///
    /// M6: feste Zahlenfelder, kein `Arc<Mutex>`, keine Datei. Wer
    /// zaehlt, verliert beim Zaehlen nichts und kann nichts leaken.
    zaehler: Zaehler,
}

/// Die vier Zahlen, die die Abnahme braucht.
///
/// M7: alle vier sind `u64` und werden zurueckgegeben, nicht
/// hochgezaehlt im Verborgenen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Zaehler {
    /// `lesen` gab einen Eintrag zurueck.
    pub treffer: u64,
    /// `lesen` fand nichts (Schluessel unbekannt).
    pub fehl: u64,
    /// `lesen` fand die Datei, aber sie war kaputt.
    pub kaputt: u64,
    /// `schreiben` hat etwas abgelegt.
    pub geschrieben: u64,
    /// `schreiben` hat abgelehnt (zu gross, Verzeichnis voll).
    pub abgelehnt: u64,
    /// Ein Eintrag fiel der Groessengrenze zum Opfer.
    pub verdraengt: u64,
}

impl Platte {
    /// Neu unter einem Verzeichnis, mit Byte-Obergrenze.
    ///
    /// Das Verzeichnis wird angelegt, wenn es nicht da ist. Eine Datei, die
    /// nicht gelesen werden kann, wird uebersprungen — ein Cache, der beim
    /// Start mault, ist schlimmer als einer, der leer anfaengt.
    pub fn neu(pfad: &Path, grenze: u64) -> io::Result<Self> {
        fs::create_dir_all(pfad)?;
        let mut platte = Platte {
            pfad: pfad.to_path_buf(),
            grenze,
            belegt: 0,
            reihenfolge: Reihenfolge::neu(),
            groessen: std::collections::HashMap::new(),
            zaehler: Zaehler::default(),
        };
        platte.index_aufbauen()?;
        Ok(platte)
    }

    /// Die Dateien einlesen und die LRU-Reihenfolge nach der
    /// Aenderzeit aufbauen (juengste = vorne).
    fn index_aufbauen(&mut self) -> io::Result<()> {
        let eintraege = match fs::read_dir(&self.pfad) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };
        let bekannt = self.index_datei_lesen();
        // Release 15.10.2026: first only the directory entries (name, size,
        // modification time; no file is opened), newest first. What does not
        // fit (more than `MAX_DATEIEN` files, or more bytes than `grenze`; the
        // oldest go first) is removed from the disk without being read.
        // MEASURED before: several processes share the directory (browser,
        // tests, WPT runs), each one only evicted inside its own view, and the
        // directory had grown to 28 187 files. The index file only holds
        // 10 000 of them, so every start opened the other 18 000 files to read
        // their headers: 4.8 s before the first request of every program start
        // (WPT runner, website_diagnose; the test `die_seite_erscheint_vor_
        // ihren_langsamen_skripten` saw its preview after 5.8 s instead of
        // before 1.5 s).
        let mut dateien: Vec<(String, u64, SystemTime)> = Vec::new();
        for eintrag in eintraege.flatten() {
            // Release 10.10.2026: die Angaben aus dem Verzeichniseintrag —
            // unter Windows ohne eigenen Systemaufruf je Datei (vorher
            // `is_file()` und `metadata()`, je ein Aufruf).
            let Ok(meta) = eintrag.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let Ok(name) = eintrag.file_name().into_string() else {
                continue;
            };
            if name.starts_with(INDEX_DATEI) {
                continue;
            }
            dateien.push((name, meta.len(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        }
        // Juengste zuerst (vorne in der LRU).
        dateien.sort_by_key(|x| std::cmp::Reverse(x.2));
        let mut geprueft: Vec<(String, u64, u128, u64)> = Vec::new();
        let mut ueberzaehlig: Vec<(String, SystemTime)> = Vec::new();
        for (name, groesse, modifiziert) in dateien {
            // M3: auch die Zahl der Dateien ist begrenzt.
            if self.reihenfolge.index.len() >= MAX_DATEIEN || self.belegt.saturating_add(groesse) > self.grenze {
                ueberzaehlig.push((name, modifiziert));
                continue;
            }
            let stempel = zeitstempel(modifiziert);
            // Release 10.10.2026: eine Datei, die schon geprueft wurde und
            // sich seitdem nicht geaendert hat, wird nicht geoeffnet.
            let laenge = match bekannt.get(&name) {
                Some(&(g, st, laenge)) if g == groesse && st == stempel => laenge,
                // Nur wasser Dateien lesen, die unser Format haben.
                //
                // GEMESSENE KORREKTUR an mir: Ich hatte den Zaehler
                // `kaputt` zuerst in `lesen()` gesetzt. Dort kommt er nie
                // an — weil `index_aufbauen` beim Start alle Dateien
                // durchsieht und eine beschaedigte SOFORT wieder loescht.
                // Die Abnahme fragt nach kaputten Dateien, also sitzt der
                // Zaehler hier, an der Stelle, an der wirklich entfernt wird.
                _ => {
                    let pfad = self.pfad.join(&name);
                    let Ok(laenge) = self.kopf_lesen(&pfad) else {
                        let _ = fs::remove_file(&pfad);
                        self.zaehler.kaputt += 1;
                        continue;
                    };
                    laenge
                }
            };
            geprueft.push((name.clone(), groesse, stempel, laenge));
            self.reihenfolge.einsetzen(&name);
            self.groessen.insert(name, laenge);
            self.belegt += laenge;
        }
        self.index_datei_schreiben(&geprueft);
        self.zaehler.verdraengt += ueberzaehlig.len() as u64;
        Self::alte_dateien_loeschen(self.pfad.clone(), ueberzaehlig);
        Ok(())
    }

    /// Release 15.10.2026: removes the files `index_aufbauen` did not keep.
    /// A few go right away; a large backlog (a directory that grew past the
    /// limit) is removed in the background so the first request does not
    /// wait for it. A file that changed since the scan (another process
    /// wrote the same entry again) is left alone.
    fn alte_dateien_loeschen(pfad: PathBuf, liste: Vec<(String, SystemTime)>) {
        const SOFORT: usize = 256;
        let sofort = liste.len() <= SOFORT;
        let loeschen = move || {
            for (name, modifiziert) in liste {
                let datei = pfad.join(&name);
                let unveraendert = fs::metadata(&datei)
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t == modifiziert);
                if unveraendert {
                    let _ = fs::remove_file(&datei);
                }
            }
        };
        if sofort {
            loeschen();
        } else if std::thread::Builder::new()
            .name("narsil-cache-aufraeumen".into())
            .spawn(loeschen)
            .is_err()
        {
            // No thread: the files stay until the next start.
        }
    }

    /// Release 10.10.2026: das Verzeichnis der geprueften Dateien lesen
    /// (`name \t groesse \t zeit \t laenge` je Zeile). Fehlt es oder ist es
    /// kaputt, ist es leer — dann wird wie frueher jede Datei geprueft.
    fn index_datei_lesen(&self) -> HashMap<String, (u64, u128, u64)> {
        let mut raus = HashMap::new();
        let Ok(text) = fs::read_to_string(self.pfad.join(INDEX_DATEI)) else {
            return raus;
        };
        for zeile in text.lines().take(MAX_DATEIEN) {
            let mut teile = zeile.split('\t');
            let (Some(name), Some(g), Some(z), Some(l)) = (teile.next(), teile.next(), teile.next(), teile.next()) else {
                continue;
            };
            if let (Ok(g), Ok(z), Ok(l)) = (g.parse(), z.parse(), l.parse()) {
                raus.insert(name.to_string(), (g, z, l));
            }
        }
        raus
    }

    /// Release 10.10.2026: das Verzeichnis schreiben (ueber eine Zwischen-
    /// datei, damit ein Abbruch kein halbes Verzeichnis hinterlaesst).
    fn index_datei_schreiben(&self, liste: &[(String, u64, u128, u64)]) {
        let mut text = String::with_capacity(liste.len() * 100);
        for (name, groesse, zeit, laenge) in liste {
            if name.contains(['\t', '\n']) {
                continue;
            }
            text.push_str(&format!("{name}\t{groesse}\t{zeit}\t{laenge}\n"));
        }
        let neu = self.pfad.join(format!("{INDEX_DATEI}.neu"));
        if fs::write(&neu, text).is_ok() {
            let _ = fs::rename(&neu, self.pfad.join(INDEX_DATEI));
        }
    }

    /// Den Kopf einer Datei lesen und die Gesamtlaenge zurueckgeben.
    fn kopf_lesen(&self, pfad: &Path) -> io::Result<u64> {
        let mut datei = File::open(pfad)?;
        let mut kopf = [0u8; KOPF_LAENGE];
        datei.read_exact(&mut kopf)?;
        if &kopf[0..4] != b"RWC1" {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Magie fehlt"));
        }
        let headers_len = u32::from_le_bytes(kopf[20..24].try_into().unwrap()) as usize;
        let vary_len = u32::from_le_bytes(kopf[28..32].try_into().unwrap()) as usize;
        let body_len = u32::from_le_bytes(kopf[32..36].try_into().unwrap()) as usize;
        // M4: die behauptete Laenge pruefen, bevor sie irgendwohin fliesst.
        let gesamt = KOPF_LAENGE
            .saturating_add(headers_len)
            .saturating_add(vary_len)
            .saturating_add(body_len);
        if gesamt as u64 > MAX_DATEI_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "zu gross"));
        }
        Ok(gesamt as u64)
    }

    /// Den Dateinamen aus dem Schluessel machen (SHA-256 in Hex).
    fn dateiname(schluessel: &str) -> String {
        format!("{}.rwc", sha256_hex(schluessel.as_bytes()))
    }

    /// Eintrag von der Platte lesen.
    ///
    /// `None` heisst: nicht da, kaputt, oder zu gross. Ein kaputter Eintrag
    /// wird geloescht, nicht weitergegeben.
    pub fn lesen(&mut self, schluessel: &str) -> Option<Eintrag> {
        // EIN Schluessel fuer Index, Groessen und Datei: der Dateiname.
        // Nach einem Neustart kennt `index_aufbauen` nur die Dateinamen
        // (der Hash laesst sich nicht zur URL zurueckrechnen) - wer hier
        // mit der URL sucht, findet nach dem Neustart nie etwas.
        // (Koordination, 30.09.: Test platte_ueberlebt_neustart war rot.)
        let name = Self::dateiname(schluessel);
        // Zaehler (18:30): vier Faelle, vier Zahlen. `?` gibt es nicht
        // mehr, weil jeder davon etwas zaehlt.
        let Some(idx) = self.reihenfolge.index.get(&name).copied() else {
            self.zaehler.fehl += 1;
            return None;
        };
        let pfad = self.pfad.join(&name);
        match self.eintrag_lesen(&pfad) {
            Ok(Some(eintrag)) => {
                self.zaehler.treffer += 1;
                self.reihenfolge.ans_kopf(idx);
                // Aenderzeit auffrischen, damit die LRU-Ordnung auf der
                // Platte weiterlebt. Release 10.10.2026: wirklich die Zeit
                // setzen — vorher `open` + `sync_all()`: ein fsync bei JEDEM
                // Treffer (unter Windows Millisekunden), und die Zeit blieb
                // trotzdem stehen.
                let _ = fs::OpenOptions::new()
                    .write(true)
                    .open(&pfad)
                    .and_then(|d| d.set_modified(SystemTime::now()));
                Some(eintrag)
            }
            Ok(None) => {
                self.zaehler.fehl += 1;
                None
            }
            Err(_) => {
                // Kaputt: entfernen, damit der naechste Start es nicht
                // erneut versucht.
                self.zaehler.kaputt += 1;
                self.entfernen(schluessel);
                None
            }
        }
    }

    /// Die Zaehler der Abnahme zurueckgeben (M7).
    #[must_use]
    pub fn zaehler(&self) -> Zaehler {
        self.zaehler
    }

    /// Eine Datei als Eintrag lesen.
    fn eintrag_lesen(&self, pfad: &Path) -> io::Result<Option<Eintrag>> {
        let mut datei = match File::open(pfad) {
            Ok(d) => d,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut kopf = [0u8; KOPF_LAENGE];
        datei
            .read_exact(&mut kopf)
            .map_err(|_| io::Error::new(io::ErrorKind::UnexpectedEof, "Kopf zu kurz"))?;
        if &kopf[0..4] != b"RWC1" {
            return Ok(None);
        }
        let empfangen_millis = u64::from_le_bytes(kopf[4..12].try_into().unwrap());
        let status = u16::from_le_bytes(kopf[12..14].try_into().unwrap());
        let header_anzahl = u32::from_le_bytes(kopf[16..20].try_into().unwrap()) as usize;
        let headers_len = u32::from_le_bytes(kopf[20..24].try_into().unwrap()) as usize;
        let vary_anzahl = u32::from_le_bytes(kopf[24..28].try_into().unwrap()) as usize;
        let vary_len = u32::from_le_bytes(kopf[28..32].try_into().unwrap()) as usize;
        let body_len = u32::from_le_bytes(kopf[32..36].try_into().unwrap()) as usize;

        // M3: die Anzahl der Header ist begrenzt.
        if header_anzahl > 10_000 || vary_anzahl > 10_000 {
            return Ok(None);
        }
        // M4: die Laengen zusammenrechnen und gegen die Grenze halten.
        // `saturating_add`, damit ein ueberlauf auf 32-Bit nicht zu einer
        // kleinen Zahl wird.
        let gesamt = headers_len
            .saturating_add(vary_len)
            .saturating_add(body_len);
        if gesamt as u64 > MAX_DATEI_BYTES {
            return Ok(None);
        }
        let mut rest = vec![0u8; gesamt];
        datei
            .read_exact(&mut rest)
            .map_err(|_| io::Error::new(io::ErrorKind::UnexpectedEof, "Daten zu kurz"))?;

        let (headers, vary, body) = self.blocke_parsen(&rest, headers_len, vary_len, body_len)?;

        let empfangen = SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(empfangen_millis);
        Ok(Some(Eintrag {
            status,
            headers,
            body,
            empfangen,
            vary,
            schluessel: String::new(), // wird vom Aufrufer gesetzt
        }))
    }

    /// Die drei Bloecke aus dem Rest herausschneiden.
    fn blocke_parsen(
        &self,
        rest: &[u8],
        headers_len: usize,
        vary_len: usize,
        body_len: usize,
    ) -> io::Result<Blocke> {
        let mut pos = 0;
        let headers = self.header_block_lesen(rest, &mut pos, headers_len)?;
        let vary = self.header_block_lesen_vary(rest, &mut pos, vary_len)?;
        // Der Koerper ist der Rest — kein Längenpraefix nötig, die Laenge
        // steht im Kopf.
        let body = rest[pos..pos + body_len].to_vec();
        Ok((headers, vary, body))
    }

    /// Einen Block von `name: wert`-Paaren lesen.
    fn header_block_lesen(
        &self,
        rest: &[u8],
        pos: &mut usize,
        laenge: usize,
    ) -> io::Result<HashMap<String, String>> {
        let mut raus = HashMap::new();
        let ende = (*pos + laenge).min(rest.len());
        let mut p = *pos;
        while p < ende {
            if p + 4 > ende {
                break;
            }
            let name_len = u32::from_le_bytes(rest[p..p + 4].try_into().unwrap()) as usize;
            p += 4;
            if p + name_len > ende {
                break;
            }
            let name = String::from_utf8_lossy(&rest[p..p + name_len]).to_string();
            p += name_len;
            if p + 4 > ende {
                break;
            }
            let wert_len = u32::from_le_bytes(rest[p..p + 4].try_into().unwrap()) as usize;
            p += 4;
            if p + wert_len > ende {
                break;
            }
            let wert = String::from_utf8_lossy(&rest[p..p + wert_len]).to_string();
            p += wert_len;
            raus.insert(name.to_ascii_lowercase(), wert);
        }
        *pos = ende;
        Ok(raus)
    }

    /// Den Vary-Block lesen — gleiche Kodierung wie die Header.
    fn header_block_lesen_vary(
        &self,
        rest: &[u8],
        pos: &mut usize,
        laenge: usize,
    ) -> io::Result<Vec<(String, String)>> {
        let mut raus = Vec::new();
        let ende = (*pos + laenge).min(rest.len());
        let mut p = *pos;
        while p < ende {
            if p + 4 > ende {
                break;
            }
            let name_len = u32::from_le_bytes(rest[p..p + 4].try_into().unwrap()) as usize;
            p += 4;
            if p + name_len > ende {
                break;
            }
            let name = String::from_utf8_lossy(&rest[p..p + name_len]).to_string();
            p += name_len;
            if p + 4 > ende {
                break;
            }
            let wert_len = u32::from_le_bytes(rest[p..p + 4].try_into().unwrap()) as usize;
            p += 4;
            if p + wert_len > ende {
                break;
            }
            let wert = String::from_utf8_lossy(&rest[p..p + wert_len]).to_string();
            p += wert_len;
            raus.push((name, wert));
        }
        *pos = ende;
        Ok(raus)
    }

    /// Eintrag auf die Platte schreiben und verdraengen.
    pub fn schreiben(&mut self, schluessel: &str, eintrag: &Eintrag) {
        if eintrag.body.len() as u64 > MAX_DATEI_BYTES {
            // Zu gross: GEMESSEN und gezaehlt, damit „der Cache
            // nimmt nichts an" eine Zahl ist und keine Vermutung.
            self.zaehler.abgelehnt += 1;
            return;
        }
        let name = Self::dateiname(schluessel);
        let pfad = self.pfad.join(&name);
        match self.eintrag_schreiben(&pfad, eintrag) {
            Ok(laenge) => {
                // Ueberschreiben desselben Schluessels: alte Groesse abziehen.
                if let Some(alt) = self.groessen.remove(&name) {
                    self.belegt = self.belegt.saturating_sub(alt);
                }
                self.reihenfolge.einsetzen(&name);
                self.belegt += laenge;
                self.groessen.insert(name, laenge);
                self.zaehler.geschrieben += 1;
                self.verdraenge();
            }
            Err(_) => {
                self.zaehler.abgelehnt += 1;
                // Schreibfehler (Platte voll, Rechte): der Speicher-Cache
                // hat den Eintrag trotzdem. Das ist der bessere Fehler.
                let _ = fs::remove_file(&pfad);
            }
        }
    }

    /// Eine Datei schreiben, zurueck die Groesse.
    fn eintrag_schreiben(&self, pfad: &Path, eintrag: &Eintrag) -> io::Result<u64> {
        let empfangen_millis = eintrag
            .empfangen
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut headers = Vec::new();
        for (k, v) in &eintrag.headers {
            headers.extend_from_slice(&(k.len() as u32).to_le_bytes());
            headers.extend_from_slice(k.as_bytes());
            headers.extend_from_slice(&(v.len() as u32).to_le_bytes());
            headers.extend_from_slice(v.as_bytes());
        }
        let mut vary = Vec::new();
        for (k, v) in &eintrag.vary {
            vary.extend_from_slice(&(k.len() as u32).to_le_bytes());
            vary.extend_from_slice(k.as_bytes());
            vary.extend_from_slice(&(v.len() as u32).to_le_bytes());
            vary.extend_from_slice(v.as_bytes());
        }
        let mut kopf = Vec::with_capacity(KOPF_LAENGE);
        kopf.extend_from_slice(b"RWC1");
        kopf.extend_from_slice(&empfangen_millis.to_le_bytes());
        kopf.extend_from_slice(&eintrag.status.to_le_bytes());
        kopf.extend_from_slice(&0u16.to_le_bytes());
        kopf.extend_from_slice(&(eintrag.headers.len() as u32).to_le_bytes());
        kopf.extend_from_slice(&(headers.len() as u32).to_le_bytes());
        kopf.extend_from_slice(&(eintrag.vary.len() as u32).to_le_bytes());
        kopf.extend_from_slice(&(vary.len() as u32).to_le_bytes());
        kopf.extend_from_slice(&(eintrag.body.len() as u32).to_le_bytes());
        let gesamt = (KOPF_LAENGE + headers.len() + vary.len() + eintrag.body.len()) as u64;
        if gesamt > MAX_DATEI_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "zu gross"));
        }
        let mut datei = File::create(pfad)?;
        datei.write_all(&kopf)?;
        datei.write_all(&headers)?;
        datei.write_all(&vary)?;
        datei.write_all(&eintrag.body)?;
        Ok(gesamt)
    }

    /// Verdraengen, bis die Obergrenze eingehalten ist.
    fn verdraenge(&mut self) {
        // (der Zaehler sitzt an der Stelle, an der wirklich entfernt wird)

        while self.belegt > self.grenze || self.reihenfolge.index.len() > MAX_DATEIEN {
            let Some(name) = self.reihenfolge.schwanz().cloned() else {
                break;
            };
            let pfad = self.pfad.join(&name);
            if let Some(g) = self.groessen.remove(&name) {
                self.belegt = self.belegt.saturating_sub(g);
            }
            let _ = fs::remove_file(&pfad);
            self.zaehler.verdraengt += 1;
            self.reihenfolge.entfernen(&name);
        }
    }

    /// Eintrag entfernen.
    pub fn entfernen(&mut self, schluessel: &str) {
        let name = Self::dateiname(schluessel);
        let pfad = self.pfad.join(&name);
        if let Some(g) = self.groessen.remove(&name) {
            self.belegt = self.belegt.saturating_sub(g);
        }
        let _ = fs::remove_file(&pfad);
        self.reihenfolge.entfernen(&name);
    }

    /// Alle Dateien loeschen.
    pub fn leeren(&mut self) {
        let _ = fs::read_dir(&self.pfad).map(|eintraege| {
            for e in eintraege.flatten() {
                let _ = fs::remove_file(e.path());
            }
        });
        self.reihenfolge = Reihenfolge::neu();
        self.belegt = 0;
        self.groessen.clear();
    }

    /// Belegte Byte auf der Platte.
    pub fn bytes(&self) -> u64 {
        self.belegt
    }

    /// Zahl der Dateien.
    pub fn eintraege(&self) -> usize {
        self.reihenfolge.index.len()
    }
}

/// Die drei Bloecke einer Cache-Datei: Header, Vary, Koerper.
type Blocke = (
    std::collections::HashMap<String, String>,
    Vec<(String, String)>,
    Vec<u8>,
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speicher::Speicher;

    fn test_eintrag(body: &[u8]) -> Eintrag {
        Eintrag {
            status: 200,
            headers: HashMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: body.to_vec(),
            empfangen: SystemTime::now(),
            vary: Vec::new(),
            schluessel: "https://example.com/".to_string(),
        }
    }

    #[test]
    fn schreiben_und_lesen() {
        let pfad = Path::new("/home/jaff/mutation_md/cache/platte_rundlauf");
        let _ = fs::remove_dir_all(pfad);
        let mut p = Platte::neu(pfad, 64 * 1024 * 1024).expect("anlegbar");
        let e = test_eintrag(b"hallo welt");
        p.schreiben("https://example.com/", &e);
        let gelesen = p.lesen("https://example.com/").expect("da");
        assert_eq!(gelesen.body, b"hallo welt");
        assert_eq!(gelesen.status, 200);
        assert_eq!(gelesen.headers.get("content-type").unwrap(), "text/html");
    }

    #[test]
    fn dateiname_ist_hash() {
        let name = Platte::dateiname("https://example.com/");
        assert_eq!(name.len(), 64 + 4); // 64 Hex + ".rwc"
        assert!(name.ends_with(".rwc"));
        assert_ne!(
            Platte::dateiname("https://example.com/a"),
            Platte::dateiname("https://example.com/b")
        );
    }

    #[test]
    fn verdraengt_bei_grenze() {
        let pfad = Path::new("/home/jaff/mutation_md/cache/platte_grenze");
        let _ = fs::remove_dir_all(pfad);
        // Jede Datei ist ~165 Byte; Grenze 500 -> maximal 3 Dateien.
        let mut p = Platte::neu(pfad, 500).expect("anlegbar");
        for i in 0..20 {
            p.schreiben(
                &format!("https://example.com/{i}"),
                &test_eintrag(&[0u8; 100]),
            );
        }
        assert!(p.bytes() <= 500, "{} > 500", p.bytes());
        assert!(p.eintraege() < 20, "nicht verdraengt");
    }

    /// Release 15.10.2026: a directory that holds more than the limit is
    /// trimmed when it is opened (the oldest files go), instead of every
    /// start reading the surplus again.
    #[test]
    fn beim_oeffnen_wird_auf_die_grenze_gekuerzt() {
        let pfad = std::env::temp_dir().join(format!("narsil_platte_kuerzen_{}", std::process::id()));
        let _ = fs::remove_dir_all(&pfad);
        let mut p = Platte::neu(&pfad, 64 * 1024 * 1024).expect("anlegbar");
        for i in 0..20 {
            p.schreiben(&format!("https://example.com/{i}"), &test_eintrag(&[0u8; 100]));
        }
        assert_eq!(p.eintraege(), 20);
        let jede = p.bytes() / 20;
        drop(p);
        let p2 = Platte::neu(&pfad, jede * 5).expect("anlegbar");
        assert_eq!(p2.eintraege(), 5);
        let dateien = fs::read_dir(&pfad)
            .expect("lesbar")
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "rwc"))
            .count();
        assert_eq!(dateien, 5, "the surplus files are removed from the disk");
        let _ = fs::remove_dir_all(&pfad);
    }

    #[test]
    fn leeren_loescht_alles() {
        let pfad = Path::new("/home/jaff/mutation_md/cache/platte_leeren");
        let _ = fs::remove_dir_all(pfad);
        let mut p = Platte::neu(pfad, 64 * 1024 * 1024).expect("anlegbar");
        p.schreiben("https://example.com/", &test_eintrag(b"x"));
        assert_eq!(p.eintraege(), 1);
        p.leeren();
        assert_eq!(p.eintraege(), 0);
        assert_eq!(p.bytes(), 0);
    }

    #[test]
    fn kaputte_datei_wird_ignoriert() {
        let pfad = Path::new("/home/jaff/mutation_md/cache/platte_kaputt");
        let _ = fs::remove_dir_all(pfad);
        let _p = Platte::neu(pfad, 64 * 1024 * 1024).expect("anlegbar");
        // Eine Datei mit falscher Magie.
        let name = Platte::dateiname("https://example.com/kaputt");
        fs::write(pfad.join(&name), b"keine cache datei").expect("schreibbar");
        // Nach dem Neuaufbau ist sie weg.
        let mut p2 = Platte::neu(pfad, 64 * 1024 * 1024).expect("anlegbar");
        assert_eq!(p2.eintraege(), 0);
        assert!(p2.lesen("https://example.com/kaputt").is_none());
    }

    #[test]
    fn speicher_und_platte_teilen_die_reihenfolge() {
        // Die LRU-Struktur muss in beiden Caches funktionieren.
        let mut s = Speicher::neu(1024);
        s.schreiben(
            "a".into(),
            crate::speicher::Eintrag {
                status: 200,
                headers: HashMap::new(),
                body: b"1".to_vec(),
                empfangen: SystemTime::now(),
                vary: Vec::new(),
                schluessel: "a".into(),
            },
        );
        assert!(s.enthaelt("a"));
    }
}
