//! Release 08.10.2026: BILD-DIÄT — weniger Speicher je Tab und weniger Netz
//! bei bilderlastigen Seiten, ohne sichtbaren Qualitaetsverlust.
//!
//! # Was sie tut
//!
//! * **Anzeigegroesse statt Originalgroesse.** Ein Bild, das deutlich groesser
//!   dekodiert ist, als es gezeichnet wird (Layoutkasten, bei `object-fit:
//!   cover` die Deckgroesse), wird im Hintergrund mit dem Kastenfilter auf die
//!   gezeichnete Groesse gebracht. JPEG wird beim Neudekodieren direkt in der
//!   DCT verkleinert (1/2, 1/4, 1/8, `narsil_image::werkzeuge::lesen_klein`).
//! * **Sichtfenster.** Bilder weiter als zwei Bildschirmhoehen ober- oder
//!   unterhalb der Sicht verlieren ihren Pixelpuffer; die komprimierten
//!   Originalbytes bleiben im Bestand (`BildCache::original`). Kommt das Bild
//!   wieder in die Naehe, wird es im Hintergrund neu dekodiert — der Kasten
//!   behaelt seine Groesse, die Seite verschiebt sich nicht.
//! * **Groesser gebraucht** (Fenster breiter, anderes `object-fit`): neu
//!   dekodiert aus den Originalbytes, hoechstens bis zur natuerlichen Groesse.
//! * **Netz.** Bilder mit festem Kasten (Breite und Hoehe stehen schon fest)
//!   werden ausserhalb des Fensters auch dann erst bei Annaeherung geholt,
//!   wenn die Seite sie nicht als `loading="lazy"` markiert.
//!
//! # Was sie nie tut (Sicherheitsnetze)
//!
//! * SVG und Bilder unter [`MIN_KANTE`] (Symbole, UI-Grafik) werden nicht
//!   angefasst; Hintergrundbilder (Sprites, `background-size`) auch nicht.
//! * Steht `pixelated`/`crisp-edges` im CSS der Seite, wird nur um ganze
//!   Faktoren verkleinert.
//! * Canvas (`drawImage`/`getImageData`/`toDataURL`) liest in dieser Engine
//!   keine `<img>`-Puffer — es arbeitet nur mit Canvas-Pixeln, also immer
//!   mit der eigenen Originalaufloesung.
//! * „Bild speichern“ und „Konvertiert herunterladen“ holen die Bytes neu
//!   ueber die Adresse — also immer das Original.
//! * Domains auf der Ausnahmeliste (`narsil://bild-diaet`) bleiben
//!   unberuehrt.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};

/// Unter dieser Kante (Breite ODER Hoehe der natuerlichen Groesse) wird
/// nichts verkleinert oder verworfen: Symbole und UI-Grafik.
pub const MIN_KANTE: u32 = 64;
/// Erst ab dieser Ersparnis lohnt ein Auftrag (64 KiB = ein 128x128-Bild).
pub const MIN_ERSPARNIS: usize = 64 * 1024;
/// Verkleinert wird erst, wenn der Puffer um diesen Faktor groesser ist als
/// gebraucht (sonst wuerde jedes Fensterziehen neu rechnen).
pub const UEBERGROESSE: f32 = 1.25;
/// So viele Bildschirmhoehen ueber und unter der Sicht bleiben Pixel stehen.
pub const FENSTER: f32 = 2.0;
/// Takt der Pruefung in Millisekunden.
pub const TAKT_MS: u64 = 250;
/// Hoechstens so viele Auftraege laufen gleichzeitig.
pub const MAX_OFFEN: usize = 8;

/// Was der Hintergrundfaden tun soll.
pub enum Auftrag {
    /// Einen vorhandenen Puffer mit dem Kastenfilter verkleinern.
    Verkleinern { seite: usize, adresse: String, pixels: Arc<Vec<u8>>, breite: u32, hoehe: u32, ziel: (u32, u32) },
    /// Aus den Originalbytes neu dekodieren (verworfen oder groesser gebraucht).
    Dekodieren { seite: usize, adresse: String, bytes: Arc<Vec<u8>>, ziel: (u32, u32) },
}

/// Was zurueckkommt.
pub struct Ergebnis {
    pub seite: usize,
    pub adresse: String,
    pub art: Art,
    pub pixels: Arc<Vec<u8>>,
    pub breite: u32,
    pub hoehe: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    Verkleinert,
    Dekodiert,
    Fehler,
}

/// Die Zaehler einer Sitzung (seit dem Start).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Zaehler {
    /// Verkleinerte Puffer.
    pub verkleinert: u64,
    /// Ausserhalb des Fensters verworfene Puffer.
    pub verworfen: u64,
    /// Neu dekodierte Puffer (Rueckkehr ins Fenster, groesser gebraucht).
    pub nachdekodiert: u64,
    /// Summe der freigegebenen Byte (jede Verkleinerung und jedes Verwerfen).
    pub freigegeben_summe: u64,
    /// Der groesste gleichzeitige Gewinn (Byte) auf einer Seite.
    pub gespart_spitze: usize,
}

/// Der Stand einer Seite (fuer `narsil://bild-diaet` und die Karte).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Seitenstand {
    pub adresse: String,
    /// Geladene Bilder mit Pixeln oder verworfen.
    pub bilder: usize,
    pub verkleinert: usize,
    pub verworfen: usize,
    /// Byte, die die Puffer jetzt belegen.
    pub puffer_bytes: usize,
    /// Byte, die sie in natuerlicher Groesse belegen wuerden.
    pub natur_bytes: usize,
    /// Bilder mit festem Kasten, die (noch) nicht geholt wurden.
    pub aufgeschoben: usize,
    /// Ausnahme fuer diese Domain?
    pub ausnahme: bool,
}

impl Seitenstand {
    /// Gesparte Byte jetzt.
    #[must_use]
    pub fn gespart(&self) -> usize {
        self.natur_bytes.saturating_sub(self.puffer_bytes)
    }
}

/// Der geteilte Bericht (alle Sitzungen eines Prozesses): Zaehler und der
/// Stand je Tab. Die eingebaute Seite und die Karte lesen ihn.
#[derive(Debug, Default, Clone)]
pub struct Bericht {
    pub zaehler: Zaehler,
    pub tabs: HashMap<u64, Seitenstand>,
}

static BERICHT: Mutex<Option<Bericht>> = Mutex::new(None);

/// Den Bericht lesen.
#[must_use]
pub fn bericht() -> Bericht {
    BERICHT.lock().ok().and_then(|b| b.clone()).unwrap_or_default()
}

fn bericht_aendern(f: impl FnOnce(&mut Bericht)) {
    if let Ok(mut b) = BERICHT.lock() {
        f(b.get_or_insert_with(Bericht::default));
    }
}

/// Den Stand eines Tabs eintragen.
pub fn stand_melden(tab: u64, stand: Seitenstand) {
    bericht_aendern(|b| {
        let spitze = stand.gespart();
        b.tabs.insert(tab, stand);
        b.zaehler.gespart_spitze = b.zaehler.gespart_spitze.max(spitze);
    });
}

/// Den Stand eines Tabs entfernen (Seite ohne Bilder, eingebaute Seite).
pub fn stand_entfernen(tab: u64) {
    bericht_aendern(|b| {
        b.tabs.remove(&tab);
    });
}

/// Nur die Tabs behalten, die es noch gibt.
pub fn tabs_behalten(ids: &[u64]) {
    bericht_aendern(|b| b.tabs.retain(|t, _| ids.contains(t)));
}

fn zaehler_aendern(f: impl FnOnce(&mut Zaehler)) {
    bericht_aendern(|b| f(&mut b.zaehler));
}

/// Eine Bytezahl fuer Menschen.
#[must_use]
pub fn mb(bytes: u64) -> String {
    let m = bytes as f64 / (1024.0 * 1024.0);
    if m >= 10.0 {
        format!("{m:.0} MB")
    } else {
        // German writes a decimal comma.
        let s = format!("{m:.1} MB");
        if narsil_i18n::current() == narsil_i18n::Language::German { s.replace('.', ",") } else { s }
    }
}

/// Die Zeile auf der Karte im Tab „Features“.
#[must_use]
pub fn kartentext() -> String {
    let b = bericht();
    let jetzt: usize = b.tabs.values().map(Seitenstand::gespart).sum();
    let aufgeschoben: usize = b.tabs.values().map(|s| s.aufgeschoben).sum();
    narsil_i18n::trf!(
        "saves {} RAM · session {} · {} later",
        mb(jetzt as u64),
        mb(b.zaehler.freigegeben_summe),
        aufgeschoben,
    )
}

// ---------------------------------------------------------------------------
// Ausnahmen je Domain
// ---------------------------------------------------------------------------

fn ausnahmen_pfad() -> std::path::PathBuf {
    crate::plattform::konfiguration().join("bild_diaet_ausnahmen.txt")
}

/// Die Ausnahmeliste (eine Domain je Zeile, klein geschrieben).
#[must_use]
pub fn ausnahmen() -> Vec<String> {
    std::fs::read_to_string(ausnahmen_pfad())
        .unwrap_or_default()
        .lines()
        .map(|z| z.trim().to_ascii_lowercase())
        .filter(|z| !z.is_empty() && !z.starts_with('#'))
        .take(500)
        .collect()
}

/// Die Ausnahmeliste speichern.
pub fn ausnahmen_speichern(liste: &[String]) -> std::io::Result<()> {
    let p = ausnahmen_pfad();
    if let Some(o) = p.parent() {
        std::fs::create_dir_all(o)?;
    }
    let mut text = String::from("# Bild-Diät: diese Domains bleiben unberührt (eine je Zeile)\n");
    for d in liste {
        text.push_str(d);
        text.push('\n');
    }
    std::fs::write(p, text)
}

/// Eine Domain saeubern (`https://www.Beispiel.de/x` → `beispiel.de`).
#[must_use]
pub fn domain_aus(eingabe: &str) -> Option<String> {
    let t = eingabe.trim().to_ascii_lowercase();
    let ohne = t.split("://").nth(1).unwrap_or(&t);
    let host = ohne.split(['/', '?', '#', ':']).next().unwrap_or("").trim_start_matches("www.");
    (!host.is_empty() && host.contains('.') && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-')))
        .then(|| host.to_string())
}

/// Steht die Seite (Adresse) auf der Ausnahmeliste? Eine Domain gilt auch
/// fuer ihre Unterdomains.
#[must_use]
pub fn ist_ausnahme(adresse: &str, liste: &[String]) -> bool {
    let Some(host) = domain_aus(adresse) else { return false };
    liste.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

// ---------------------------------------------------------------------------
// Der Hintergrundfaden
// ---------------------------------------------------------------------------

/// Der Zustand der Bild-Diaet einer Sitzung.
pub struct Diaet {
    sender: Option<mpsc::Sender<Auftrag>>,
    empfang: Option<mpsc::Receiver<Ergebnis>>,
    /// Adressen mit laufendem Auftrag.
    pub offen: HashSet<String>,
    pub letzter_takt: Option<std::time::Instant>,
    /// Die Ausnahmeliste (beim Start und nach Aenderung gelesen).
    pub ausnahmen: Vec<String>,
    ausnahmen_gelesen: Option<std::time::Instant>,
    /// Der Abdruck der Lage beim letzten Takt (Seite, Versatz, Fenster,
    /// Zahl der geladenen Bilder) — aendert er sich, ist ein Takt faellig.
    pub abdruck: (usize, u32, u32, u32, usize),
    /// Hat der letzte Takt etwas zu tun gefunden (dann bald noch einmal)?
    pub arbeit: bool,
}

impl Default for Diaet {
    fn default() -> Self {
        Diaet {
            sender: None,
            empfang: None,
            offen: HashSet::new(),
            letzter_takt: None,
            ausnahmen: Vec::new(),
            ausnahmen_gelesen: None,
            abdruck: (0, 0, 0, 0, 0),
            arbeit: false,
        }
    }
}

impl Diaet {
    /// Die Ausnahmeliste hoechstens alle zwei Sekunden neu lesen (die Seite
    /// `narsil://bild-diaet` schreibt sie).
    pub fn ausnahmen_auffrischen(&mut self) {
        let alt = self.ausnahmen_gelesen.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2));
        if !alt {
            self.ausnahmen = ausnahmen();
            self.ausnahmen_gelesen = Some(std::time::Instant::now());
        }
    }

    /// Einen Auftrag abgeben (der Faden startet beim ersten Mal).
    pub fn abgeben(&mut self, auftrag: Auftrag) -> bool {
        if self.offen.len() >= MAX_OFFEN {
            return false;
        }
        if self.sender.is_none() {
            let (s, auftraege) = mpsc::channel::<Auftrag>();
            let (fertig, e) = mpsc::channel::<Ergebnis>();
            let gestartet = std::thread::Builder::new()
                .name("narsil-bild-diaet".into())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    while let Ok(a) = auftraege.recv() {
                        let ergebnis = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ausfuehren(a)));
                        if let Ok(r) = ergebnis
                            && fertig.send(r).is_err()
                        {
                            break;
                        }
                    }
                });
            if gestartet.is_err() {
                return false;
            }
            self.sender = Some(s);
            self.empfang = Some(e);
        }
        let adresse = match &auftrag {
            Auftrag::Verkleinern { adresse, .. } | Auftrag::Dekodieren { adresse, .. } => adresse.clone(),
        };
        if self.offen.contains(&adresse) {
            return false;
        }
        match self.sender.as_ref().map(|s| s.send(auftrag)) {
            Some(Ok(())) => {
                self.offen.insert(adresse);
                true
            }
            _ => false,
        }
    }

    /// Fertige Ergebnisse abholen.
    pub fn abholen(&mut self) -> Vec<Ergebnis> {
        let mut raus = Vec::new();
        if let Some(e) = self.empfang.as_ref() {
            while let Ok(r) = e.try_recv() {
                raus.push(r);
            }
        }
        for r in &raus {
            self.offen.remove(&r.adresse);
        }
        raus
    }
}

fn ausfuehren(a: Auftrag) -> Ergebnis {
    match a {
        Auftrag::Verkleinern { seite, adresse, pixels, breite, hoehe, ziel } => {
            let quelle = narsil_image::Bild { breite, hoehe, rgba: pixels.as_ref().clone() };
            let klein = narsil_image::skalieren::skalieren(&quelle, ziel.0.max(1), ziel.1.max(1), narsil_image::skalieren::Filter::Box);
            Ergebnis { seite, adresse, art: Art::Verkleinert, breite: klein.breite, hoehe: klein.hoehe, pixels: Arc::new(klein.rgba) }
        }
        Auftrag::Dekodieren { seite, adresse, bytes, ziel } => {
            match crate::bilder::dekodieren_mit_groesse(&bytes, ziel.0.max(1), ziel.1.max(1)) {
                Ok(b) => Ergebnis { seite, adresse, art: Art::Dekodiert, breite: b.breite, hoehe: b.hoehe, pixels: b.pixels },
                Err(_) => Ergebnis { seite, adresse, art: Art::Fehler, breite: 0, hoehe: 0, pixels: Arc::new(Vec::new()) },
            }
        }
    }
}

/// Zaehlt eine erledigte Verkleinerung bzw. ein Verwerfen.
pub fn gezaehlt(art: Art, freigegeben: usize) {
    zaehler_aendern(|z| {
        match art {
            Art::Verkleinert => z.verkleinert += 1,
            Art::Dekodiert => z.nachdekodiert += 1,
            Art::Fehler => {}
        }
        z.freigegeben_summe += freigegeben as u64;
    });
}

/// Zaehlt ein Verwerfen.
pub fn verworfen_gezaehlt(freigegeben: usize) {
    zaehler_aendern(|z| {
        z.verworfen += 1;
        z.freigegeben_summe += freigegeben as u64;
    });
}

/// Die Zielgroesse aus natuerlicher Groesse und gebrauchter Groesse: das
/// Seitenverhaeltnis bleibt, nie ueber die natuerliche Groesse. Mit
/// `ganzzahlig` nur ganze Teiler (fuer Pixelgrafik).
#[must_use]
pub fn zielgroesse(natur: (u32, u32), gebraucht: (f32, f32), ganzzahlig: bool) -> (u32, u32) {
    let (nb, nh) = (natur.0.max(1) as f32, natur.1.max(1) as f32);
    let s = (gebraucht.0 / nb).max(gebraucht.1 / nh).clamp(0.0, 1.0);
    if ganzzahlig {
        let k = (1.0 / s.max(1e-6)).floor().max(1.0);
        return ((nb / k).ceil() as u32, (nh / k).ceil() as u32);
    }
    (((nb * s).ceil() as u32).max(1), ((nh * s).ceil() as u32).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_zielgroesse_haelt_das_seitenverhaeltnis() {
        assert_eq!(zielgroesse((2400, 1800), (400.0, 300.0), false), (400, 300));
        // `cover`: die groessere Ausdehnung zaehlt.
        assert_eq!(zielgroesse((2400, 1800), (400.0, 400.0), false), (534, 400));
        // nie groesser als natuerlich
        assert_eq!(zielgroesse((800, 600), (1600.0, 1200.0), false), (800, 600));
        // ganzzahlig: 2400/400 = 6 → Teiler 6
        assert_eq!(zielgroesse((2400, 1800), (400.0, 300.0), true), (400, 300));
        assert_eq!(zielgroesse((2400, 1800), (500.0, 300.0), true), (600, 450));
    }

    #[test]
    fn domains_und_ausnahmen() {
        assert_eq!(domain_aus("https://www.Spiegel.de/politik?x=1").as_deref(), Some("spiegel.de"));
        assert_eq!(domain_aus("t-online.de").as_deref(), Some("t-online.de"));
        assert_eq!(domain_aus("kein host"), None);
        let liste = vec!["spiegel.de".to_string()];
        assert!(ist_ausnahme("https://www.spiegel.de/a", &liste));
        assert!(ist_ausnahme("https://cdn.spiegel.de/a", &liste));
        assert!(!ist_ausnahme("https://notspiegel.de/a", &liste));
    }
}
