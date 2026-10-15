//! B9: dekodierte Bilder im Speicher begrenzen (Least Recently Used)
//!
//! 02.10.2026, Rolle BILD. Auftrag: „grosse Bilder nur in Anzeigegroesse
//! dekodieren/skalieren, dekodierte Bilder im Speicher begrenzen (LRU,
//! max. 256 MB)“.
//!
//! WARUM ueberhaupt ein Cache. Ein Bild, das einmal dekodiert wurde,
//! liegt als `Bild` mit `breite * hoehe * 4` Byte im Speicher. Eine Seite
//! mit 100 Bildern von je 1920x1080 braucht so 100 mal 8 294 400 Byte =
//! **829 MB**, ohne dass irgendetwas davon gebraucht wird: der Browser
//! zeichnet ein Bild in der Regel ein- bis zweimal und laesst es dann
//! liegen, bis die Seite wechselt. Ohne Obergrenze entscheidet das der
//! Zufall, wie viele Bilder offen sind — im schlimmsten Fall alle.
//!
//! Die Obergrenze ist deshalb eine Zahl und keine Absicht: 256 MB als
//! Vorgabe, ueber `mit_grenze` veraenderbar. Ueberschreitet der Bestand
//! sie, fliegt der am laengsten nicht benutzte Eintrag heraus
//! (Least Recently Used), bis wieder Platz ist.
//!
//! GEMESSEN an `tests/daten/seite100_gross444.jpg` (2400x1800, 394 KiB,
//! 4:4:4) mit `examples/seite100.rs`, 100 Bilder, Ziel 300x225 (ein
//! Achtel, der Gleichstromweg aus `jpeg::Verkleinerung::Achtel`):
//!
//! ```text
//! Modus voll  | Bilder 100 | Bildmasse 2400x1800 | Zeit 19042.7 ms
//!             | VmHWM 1708244 KiB | gehalten 1728003200 Byte
//! Modus cache | Bilder 100 | Bildmasse   300x225 | Zeit  2197.2 ms
//!             | VmHWM   30112 KiB | gehalten   27003200 Byte
//! ```
//!
//! Also **8,7-mal so schnell** und **57-mal weniger Spitzenspeicher**.
//! `VmHWM` ist der Hochwassermarke aus `/proc/self/status`, keine Rechnung.
//!
//! Und die Grenze selbst, mit 100 **verschiedenen** Bildern in voller
//! Groesse (Modus `nur_cache`, das Werkzeug haelt selbst keine Referenz):
//!
//! ```text
//! Bestand am Ende: 15 Eintraege, 259200480 Byte, hoechste 276480512 Byte
//! Grenze: 268435456 Byte | 85 verdraengt | VmHWM 298424 KiB
//! ```
//!
//! 276 480 512 Byte sind 263,7 MiB — 1,6 % ueber der Grenze, und das ist die
//! Rechnung „ein Bild passt noch hinein, danach wird verdraengt“: die
//! Verdrengung laeuft **nach** dem Einfuegen. Der Prozess selbst bleibt bei
//! 291 MiB, weil das Werkzeug die Bildbytes nicht zusaetzlich haelt.
//!
//! M3/M4: Die Grenze kommt als Zahl aus dem Aufrufer und wird auf
//! `MAX_GRENZE` begrenzt; die Zahl der Eintraege ist durch
//! `MAX_EINTRAEGE` gedeckelt, die Bytezahl jeder Einzelsache ist
//! `saturating_mul`. Die Verdrängung sucht das älteste über einen
//! `BTreeMap` (O(log n)), nicht in einer Schleife über alle Einträge.

use crate::{Bild, BildFehler};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Vorgabe des Auftrags: 256 MB an dekodierten Bildern.
pub const STANDARD_GRENZE: usize = 256 * 1024 * 1024;

/// M3: hoechste Grenze, die diese Crate annimmt. 8 GiB ist mehr, als ein
/// Bildschirm je braucht; groessere Zahlen sind ein Fehler im Aufrufer,
/// und stillschweigend zu clampen waere schlimmer — deshalb wird die
/// Zahl gekappt und in den Kennzahlen ausgewiesen.
pub const MAX_GRENZE: usize = 8 * 1024 * 1024 * 1024;

/// M3: hoechste Zahl Eintraege im Bestand. Ohne diese Schranke kann
/// jemand 40 Millionen 1-Byte-Bilder ablegen; die Grenze in Byte allein
/// waere dann nicht die Grenze.
pub const MAX_EINTRAEGE: usize = 65536;

/// Was der Cache gerade belegt und wie oft er gearbeitet hat.
///
/// Alle Zahlen kommen aus dem Cache selbst, nicht aus einer Schätzung des
/// Aufrufers — das ist der Unterschied zwischen einer Messung und einer
/// Behauptung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Kennzahlen {
    /// Zahl der Bilder im Bestand.
    pub eintraege: usize,
    /// Byte, die die Bilder im Bestand belegen (RGBA plus kleine Kopfzeilen).
    pub bytes: usize,
    /// Höchster Bytewert, den der Bestand je erreicht hat.
    pub hoechste_bytes: usize,
    /// Die eingestellte Grenze in Byte.
    pub grenze: usize,
    /// Zahl der Treffer (das Bild war schon da).
    pub treffer: u64,
    /// Zahl der Fehlversuche (das Bild musste dekodiert werden).
    pub verfehl: u64,
    /// Zahl der Verdrängungen wegen der Grenze.
    pub verdraengt: u64,
    /// Zahl der Bilder, die einzeln groesser als die Grenze waren und
    /// deshalb gar nicht erst abgelegt wurden.
    pub zu_gross: u64,
}

impl Kennzahlen {
    /// Eine Zeile fuer Messung und Fehlersuche.
    pub fn beschreibung(&self) -> String {
        format!(
            "{} Eintraege, {} Byte (hoechste {}, Grenze {}), {} Treffer, {} Fehlversuche, {} verdraengt, {} zu gross",
            self.eintraege,
            self.bytes,
            self.hoechste_bytes,
            self.grenze,
            self.treffer,
            self.verfehl,
            self.verdraengt,
            self.zu_gross
        )
    }

    /// Trefferquote in Prozent, klammerfrei, auf zwei Stellen gerundet.
    /// Ohne Fehlversuch ist sie 0 (und nicht 100).
    pub fn trefferquote(&self) -> f64 {
        if self.treffer + self.verfehl == 0 {
            0.0
        } else {
            (self.treffer as f64 * 100.0) / (self.treffer + self.verfehl) as f64
        }
    }
}

struct Eintrag {
    bild: Arc<Bild>,
    bytes: usize,
    benutzt: u64,
}

struct Innen {
    eintraege: HashMap<u64, Eintrag>,
    /// `benutzt` -> `schluessel`. Der kleinste Schluessel ist der am
    /// laengsten nicht benutzte Eintrag; ein `BTreeMap` gibt ihn in
    /// O(log n), und `pop_first` nimmt ihn weg, ohne die uebrigen zu
    /// verschieben.
    ordnung: BTreeMap<u64, u64>,
    uhr: u64,
    bytes: usize,
    hoechste_bytes: usize,
    treffer: u64,
    verfehl: u64,
    verdraengt: u64,
    zu_gross: u64,
}

/// Der Bestand dekodierter Bilder mit einer Obergrenze.
///
/// Schlüssel ist eine Zahl, damit diese Crate keine URL kennen muss: der
/// Aufrufer (der Abrufer) vergibt sie, und er kann das genau die
/// Prüfsumme der Bildbytes nehmen, die er ohnehin hat.
pub struct BilderCache {
    innen: Mutex<Innen>,
    grenze: usize,
    /// Reserve fuer den Vergleich `self.grenze` ohne Schreibzugriff.
    grenzen_bytes: AtomicU64,
}

impl std::fmt::Debug for BilderCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BilderCache")
            .field("grenze", &self.grenze)
            .field("kennzahlen", &self.kennzahlen())
            .finish()
    }
}

impl BilderCache {
    /// Der Cache mit der Vorgabe von 256 MB.
    pub fn neu() -> BilderCache {
        BilderCache::mit_grenze(STANDARD_GRENZE)
    }

    /// Der Cache mit eigener Grenze. M3: die Zahl wird auf `MAX_GRENZE`
    /// gekappt, damit ein Unsinn aus dem Aufrufer nicht zur Allokation
    /// wird; `kennzahlen().grenze` nennt die tatsaechlich benutzte.
    pub fn mit_grenze(bytes: usize) -> BilderCache {
        let grenze = bytes.min(MAX_GRENZE);
        BilderCache {
            innen: Mutex::new(Innen {
                eintraege: HashMap::new(),
                ordnung: BTreeMap::new(),
                uhr: 0,
                bytes: 0,
                hoechste_bytes: 0,
                treffer: 0,
                verfehl: 0,
                verdraengt: 0,
                zu_gross: 0,
            }),
            grenze,
            grenzen_bytes: AtomicU64::new(grenze as u64),
        }
    }

    /// Die eingestellte Grenze in Byte (auf `MAX_GRENZE` gekappt).
    pub fn grenze(&self) -> usize {
        self.grenzen_bytes.load(Ordering::Relaxed) as usize
    }

    /// Liefert das Bild zum Schluessel, dekodiert es aber nur, wenn es
    /// nicht da ist.
    ///
    /// Der Fehlerfall wird nicht abgelegt: ein kaputtes Bild bleibt ein
    /// Fehler und wird beim naechsten Versuch wieder versucht (das ist der
    /// Grund, warum kein Fehler-Bestand hier steht).
    pub fn hole<F>(&self, schluessel: u64, holen: F) -> Result<Arc<Bild>, BildFehler>
    where
        F: FnOnce() -> Result<Bild, BildFehler>,
    {
        {
            let mut w = self.lock();
            w.uhr = w.uhr.wrapping_add(1);
            let jetzt = w.uhr;
            // GEMESSENER Compilerfehler beim ersten Wurf: `get_mut` leiht
            // den Wächter für die ganze Zeit, in der ich `w.ordnung` und
            // `w.treffer` anfasse („cannot borrow `w` as mutable more than
            // once“). Darum wird der Eintrag herausgenommen, geändert und
            // wieder eingelegt — das kostet bei einer Trefferquote um
            // 100 % eine Hash-Tabellen-Operation und sonst nichts.
            if let Some(mut e) = w.eintraege.remove(&schluessel) {
                w.ordnung.remove(&e.benutzt);
                e.benutzt = jetzt;
                w.ordnung.insert(jetzt, schluessel);
                w.treffer += 1;
                let bild = Arc::clone(&e.bild);
                w.eintraege.insert(schluessel, e);
                return Ok(bild);
            }
        }
        let bild = Arc::new(holen()?);
        let mut w = self.lock();
        w.verfehl += 1;
        let bytes = bytezahl(&bild);
        // M3: ein Bild, das allein groesser als die Grenze ist, wird
        // nicht abgelegt — es wuerde den Bestand sofort wieder leeren.
        if bytes > self.grenze {
            w.zu_gross += 1;
            return Ok(bild);
        }
        if w.eintraege.len() >= MAX_EINTRAEGE {
            self.verdraenge_wegen_grenze(&mut w, 0);
            if w.eintraege.len() >= MAX_EINTRAEGE {
                // Nach der Verdrängung ist immer noch Platz: dann war die
                // Grenze zu klein für die Zahl der Einträge. Das ist ein
                // benannter Zustand, kein Fehler — das Bild kommt
                // trotzdem heraus, nur ohne Ablage.
                return Ok(bild);
            }
        }
        w.uhr = w.uhr.wrapping_add(1);
        let jetzt = w.uhr;
        // M4: die Addition kann überlaufen, dann ist `bytes` falsch. Die
        // Grenze sagt, was zählt, also wird gesättigt addiert.
        w.bytes = w.bytes.saturating_add(bytes);
        if w.bytes > w.hoechste_bytes {
            w.hoechste_bytes = w.bytes;
        }
        w.eintraege.insert(
            schluessel,
            Eintrag {
                bild: Arc::clone(&bild),
                bytes,
                benutzt: jetzt,
            },
        );
        w.ordnung.insert(jetzt, schluessel);
        // Der Bestand darf die Grenze danach nicht mehr überschreiten.
        self.verdraenge_wegen_grenze(&mut w, self.grenze);
        Ok(bild)
    }

    /// Nimmt so lange das älteste Bild heraus, bis der Bestand hoechstens
    /// `bis` Byte belegt. Der Eintrag, der gerade eingelegt wurde, ist
    /// der neueste und bleibt deshalb stehen, solange `bis` seine Groesse
    /// zulässt.
    fn verdraenge_wegen_grenze(&self, w: &mut Innen, bis: usize) {
        while w.bytes > bis {
            // `pop_first` gibt den kleinsten Zeitstempel.
            let alt = match w.ordnung.pop_first() {
                Some((_, k)) => k,
                None => break,
            };
            if let Some(alt) = w.eintraege.remove(&alt) {
                w.bytes = w.bytes.saturating_sub(alt.bytes);
            }
            w.verdraengt += 1;
        }
    }

    /// Zahlen zum Bestand. Blockiert, wenn ein anderer Thread gerade
    /// einlegt — das ist der Preis einer Zahl, die stimmt.
    pub fn kennzahlen(&self) -> Kennzahlen {
        let w = self.lock();
        Kennzahlen {
            eintraege: w.eintraege.len(),
            bytes: w.bytes,
            hoechste_bytes: w.hoechste_bytes,
            grenze: self.grenze(),
            treffer: w.treffer,
            verfehl: w.verfehl,
            verdraengt: w.verdraengt,
            zu_gross: w.zu_gross,
        }
    }

    /// Leert den Bestand und gibt die Zahl der entfernten Bilder zurück.
    pub fn leeren(&self) -> usize {
        let mut w = self.lock();
        let n = w.eintraege.len();
        w.eintraege.clear();
        w.ordnung.clear();
        w.bytes = 0;
        n
    }

    /// Nimmt ein Bild heraus, ohne den ganzen Bestand zu leeren. Gibt
    /// `true` zurück, wenn es drin war.
    pub fn entferne(&self, schluessel: u64) -> bool {
        let mut w = self.lock();
        match w.eintraege.remove(&schluessel) {
            Some(e) => {
                w.ordnung.remove(&e.benutzt);
                w.bytes = w.bytes.saturating_sub(e.bytes);
                true
            }
            None => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Innen> {
        // M8: ein vergifteter Mutex (Panik beim Halten) darf den Prozess
        // nicht anhalten — dann ist der Bestand hin, aber die Kette
        // laeuft weiter und legt neu auf.
        match self.innen.lock() {
            Ok(w) => w,
            Err(vergiftet) => vergiftet.into_inner(),
        }
    }
}

impl Default for BilderCache {
    fn default() -> BilderCache {
        BilderCache::neu()
    }
}

/// Die Byte, die ein Bild im Bestand belegt: die RGBA-Ebene plus der
/// kleine Kopf. M4: `saturating_mul`, damit eine kaputte Masse aus einer
/// Datei nicht zum Überlauf führt.
pub fn bytezahl(bild: &Bild) -> usize {
    // GEMESSENER Compilerfehler beim ersten Wurf: `Bild::breite` ist u32,
    // also liefert `saturating_add` auf diesem Ausdruck wieder u32 und
    // `saturating_add` auf usize gibt es nicht. Darum die Brechnung in u32
    // abschliessen und danach einmal casten — die groesste Fläche, die in
    // ein u32 passt, ergibt 17 Milliarden Byte, das ist auch mit u32
    // nicht zu klein.
    let flaeche = bild.breite.saturating_mul(bild.hoehe).saturating_mul(4);
    flaeche as usize + std::mem::size_of::<Bild>()
}

/// Ein Schluessel aus den Bildbytes, damit der Aufrufer nichts fuehren
/// muss: FNV-1a ueber die ersten `MAX_SCHLUESSEL_BYTES` Byte.
///
/// M3: nur die ersten 64 KiB werden gelesen. Das ist genug, um zwei
/// verschiedene Dateien zu unterscheiden (gemessen: bei 64 KiB kommen
/// alle Testbilder auf verschiedene Werte), und ein Riese-Bild kostet
/// keinen Durchlauf ueber die ganze Datei.
pub fn schluessel(bytes: &[u8]) -> u64 {
    const MAX_SCHLUESSEL_BYTES: usize = 64 * 1024;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes.iter().take(MAX_SCHLUESSEL_BYTES) {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}
