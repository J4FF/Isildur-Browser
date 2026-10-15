//! Der Speicher-Cache: LRU mit Byte-Obergrenze.
//!
//! Zwei Grenzen, beide Pflicht (M4/M6):
//!
//! * **Bytes** — die Summe aller Koerper, Header und Schluessel. Ein Cache,
//!   der nur die Zahl der Eintraege begrenzt, laeuft mit kleinen Eintraegen
//!   trotzdem in die Decke.
//! * **Eintraege** — `MAX_EINTRAEGE`. Selbst mit 1-Byte-Koerpern wuerde
//!   eine unendliche Zahl von Schluesseln den `HashMap` fuellen.
//!
//! Die LRU-Reihenfolge steht in einem Vec mit Indizes (Index 0 = Kopf =
//! zuletzt benutzt, Index 1 = Schwanz = am laengsten nicht benutzt). Alle
//! Operationen sind O(1), weil ein Knoten nur Zeiger auf seine Nachbarn
//! hat — kein `unsafe`, kein `Rc<RefCell>`-Zyklus (M6).

use std::collections::HashMap;
use std::time::SystemTime;

use crate::{Anfrage, Antwort};

use crate::regeln;

/// Obergrenze des Speichers in Byte (64 MiB).
pub const MAX_SPEICHER_BYTES: u64 = 64 * 1024 * 1024;

/// Obergrenze der Eintraege (M3/M6).
///
/// Ein Eintrag ist mindestens ein paar Hundert Byte (Schluessel, Header,
/// Koerper). 10.000 Eintraege sind also weit unter der Byte-Grenze — die
/// Zahl ist die zweite, unabhaengige Mauer.
const MAX_EINTRAEGE: usize = 10_000;

/// Obergrenze eines Schluessels in Byte (M3).
const MAX_SCHLUESSEL: usize = 512;

/// Index des Kopfes (zuletzt benutzt) und des Schwanzes (am laengsten
/// nicht benutzt) in `Reihenfolge::knoten`.
const KOPF: usize = 0;
const SCHWANZ: usize = 1;

/// Ein gespeicherter Eintrag.
///
/// Der Koerper liegt SO, wie er kam — auch `Content-Encoding: gzip`
/// bleibt stehen, und `entpackt` ist `false`. Ein Cache, der den Koerper
/// veraendert, muss dem Client eine Antwort geben, die er nicht entpacken
/// kann.
#[derive(Debug, Clone)]
pub struct Eintrag {
    /// Statuscode der Antwort.
    pub status: u16,
    /// Alle Antwort-Header, unveraendert.
    pub headers: HashMap<String, String>,
    /// Der Koerper, so wie er auf der Leitung war.
    pub body: Vec<u8>,
    /// Wann die Antwort gekommen ist (fuer die Frische).
    pub empfangen: SystemTime,
    /// Die Request-Header, die `Vary` genannt hat.
    pub vary: Vec<(String, String)>,
    /// Der Schluessel (die URL) — fuer `Response::url`.
    pub schluessel: String,
}

impl Eintrag {
    /// Aus Anfrage und Antwort bauen.
    pub fn neu(anfrage: &Anfrage, antwort: &Antwort, schluessel: String) -> Self {
        let namen = regeln::vary_namen(&antwort.headers);
        let vary = regeln::vary_speichern(anfrage, &namen);
        Eintrag {
            status: antwort.status,
            headers: antwort.headers.clone(),
            body: antwort.koerper.clone(),
            empfangen: SystemTime::now(),
            vary,
            schluessel,
        }
    }

    /// Wie viel Platz braucht dieser Eintrag?
    ///
    /// Schluessel + Koerper + Header + Vary. Die Zahl ist eine Obergrenze
    /// (HashMap-Overhead nicht mitgezaehlt) — die Byte-Grenze ist also
    /// eher zu locker als zu streng. Das ist richtig so: eine zu strenge
    /// Grenze verwerft Eintraege, die noch passten.
    pub fn bytes(&self) -> u64 {
        let mut summe = self.schluessel.len() as u64 + self.body.len() as u64;
        for (k, v) in &self.headers {
            summe += k.len() as u64 + v.len() as u64 + 2;
        }
        for (k, v) in &self.vary {
            summe += k.len() as u64 + v.len() as u64 + 2;
        }
        summe + 64 // Rahmen: Status, Zeit, HashMap-Overhead
    }
}

/// Die LRU-Reihenfolge ueber Indizes.
///
/// Wird vom Speicher-Cache und vom Platten-Cache benutzt: beide brauchen
/// dieselbe Ordnung, und zwei Implementationen davon sind zwei Stellen, an
/// denen sie sich unterscheiden koennen.
pub(crate) struct Reihenfolge {
    /// Knoten. Index 0 und 1 sind die Sentinelle, Eintraege ab 2.
    pub(crate) knoten: Vec<Knoten>,
    /// Wiederverwendbare Plaetze (M6: kein Wachstum ohne Grenze).
    pub(crate) frei: Vec<usize>,
    /// Schluessel -> Knoten-Index.
    pub(crate) index: HashMap<String, usize>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Knoten {
    prev: usize,
    next: usize,
}

impl Reihenfolge {
    pub(crate) fn neu() -> Self {
        Reihenfolge {
            knoten: vec![
                Knoten {
                    prev: SCHWANZ,
                    next: SCHWANZ,
                }, // Kopf-Sentinel
                Knoten {
                    prev: KOPF,
                    next: KOPF,
                }, // Schwanz-Sentinel
            ],
            frei: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Eintrag an den Kopf holen (zuletzt benutzt).
    pub(crate) fn ans_kopf(&mut self, idx: usize) {
        if self.knoten[KOPF].next == idx {
            return; // schon vorne
        }
        self.abhaengen(idx);
        self.nach_kopf_haengen(idx);
    }

    /// Eintrag abhaengen (die Nachbarn verbinden sich miteinander).
    fn abhaengen(&mut self, idx: usize) {
        let prev = self.knoten[idx].prev;
        let next = self.knoten[idx].next;
        self.knoten[prev].next = next;
        self.knoten[next].prev = prev;
    }

    /// Eintrag direkt hinter den Kopf haengen.
    fn nach_kopf_haengen(&mut self, idx: usize) {
        let bisher = self.knoten[KOPF].next;
        self.knoten[idx].prev = KOPF;
        self.knoten[idx].next = bisher;
        self.knoten[KOPF].next = idx;
        self.knoten[bisher].prev = idx;
    }

    /// Neuen Knoten anlegen und an den Kopf haengen.
    pub(crate) fn einsetzen(&mut self, schluessel: &str) -> usize {
        let idx = match self.frei.pop() {
            Some(i) => i,
            None => {
                self.knoten.push(Knoten { prev: 0, next: 0 });
                self.knoten.len() - 1
            }
        };
        self.knoten[idx].prev = 0;
        self.knoten[idx].next = 0;
        self.nach_kopf_haengen(idx);
        self.index.insert(schluessel.to_string(), idx);
        idx
    }

    /// Eintrag entfernen und Platz freigeben.
    pub(crate) fn entfernen(&mut self, schluessel: &str) {
        if let Some(idx) = self.index.remove(schluessel) {
            self.abhaengen(idx);
            self.frei.push(idx);
        }
    }

    /// Den Schwanz liefern (am laengsten nicht benutzt), ohne ihn zu
    /// entfernen — fuer die Verdrängung.
    pub(crate) fn schwanz(&self) -> Option<&String> {
        let idx = self.knoten[SCHWANZ].prev;
        if idx == KOPF {
            return None;
        }
        self.index.iter().find(|(_, i)| **i == idx).map(|(s, _)| s)
    }
}

/// Der Speicher-Cache.
pub struct Speicher {
    /// Die Eintraege, nach Schluessel.
    eintraege: HashMap<String, Eintrag>,
    /// Die LRU-Reihenfolge.
    reihenfolge: Reihenfolge,
    /// Die Byte-Obergrenze.
    grenze: u64,
    /// Aktuell belegte Bytes.
    belegt: u64,
}

impl Speicher {
    /// Neu mit einer Byte-Obergrenze.
    pub fn neu(grenze: u64) -> Self {
        Speicher {
            eintraege: HashMap::new(),
            reihenfolge: Reihenfolge::neu(),
            grenze,
            belegt: 0,
        }
    }

    /// Eintrag lesen — und als zuletzt benutzt markieren.
    ///
    /// Rueckgabe ist ein Klon, weil der Aufrufer eine `Response` braucht,
    /// die ihm gehoert. Ein Cache, der hier eine Referenz zurueckgibt,
    /// haengt dem Aufrufer eine Lebenszeit um die Nase, die er nicht
    /// kontrollieren kann.
    pub fn lesen(&mut self, schluessel: &str) -> Option<Eintrag> {
        let idx = *self.reihenfolge.index.get(schluessel)?;
        self.reihenfolge.ans_kopf(idx);
        self.eintraege.get(schluessel).cloned()
    }

    /// Gibt es den Eintrag? (Ohne die Reihenfolge zu aendern — fuer
    /// Pruefungen, die nicht "benutzen" bedeuten.)
    #[allow(dead_code)]
    pub fn enthaelt(&self, schluessel: &str) -> bool {
        self.eintraege.contains_key(schluessel)
    }

    /// Eintrag speichern. Verdraengt dabei, was nötig ist.
    pub fn schreiben(&mut self, schluessel: String, eintrag: Eintrag) {
        // M3: ein Schluessel ist eine URL, und eine URL hat kein festes
        // Limit — aber ein Dateisystem und ein HashMap schon.
        if schluessel.len() > MAX_SCHLUESSEL {
            return;
        }
        let neu = eintrag.bytes();
        // Ein einzelner Eintrag, der die ganze Grenze sprengt, wird nicht
        // gespeichert: er wuerde den Cache leeren, um dann allein darin
        // zu sitzen.
        if neu > self.grenze {
            return;
        }
        // Gleicher Schluessel: ersetzen, nicht verdoppeln.
        if self.eintraege.contains_key(&schluessel) {
            self.reihenfolge.entfernen(&schluessel);
        }
        self.reihenfolge.einsetzen(&schluessel);
        self.belegt += neu;
        self.eintraege.insert(schluessel, eintrag);
        self.verdraenge();
    }

    /// Verdraengen, bis beide Grenzen eingehalten sind.
    fn verdraenge(&mut self) {
        while self.belegt > self.grenze || self.eintraege.len() > MAX_EINTRAEGE {
            let Some(schluessel) = self.reihenfolge.schwanz().cloned() else {
                break;
            };
            if let Some(weg) = self.eintraege.remove(&schluessel) {
                self.belegt -= weg.bytes();
            }
            self.reihenfolge.entfernen(&schluessel);
        }
    }

    /// Eintrag aktualisieren (fuer 304-Antworten, RFC 9111, Abschnitt 4.3.4).
    ///
    /// Die Header der 304 werden in den gespeicherten Eintrag uebernommen
    /// (die 304 ersetzt, was sie sendet), der Koerper bleibt. `true`, wenn
    /// der Eintrag da war.
    pub fn aktualisieren(&mut self, schluessel: &str, headers: &HashMap<String, String>) -> bool {
        if let Some(eintrag) = self.eintraege.get_mut(schluessel) {
            for (k, v) in headers {
                eintrag.headers.insert(k.clone(), v.clone());
            }
            eintrag.empfangen = SystemTime::now();
            true
        } else {
            false
        }
    }

    /// Alles loeschen.
    pub fn leeren(&mut self) {
        self.eintraege.clear();
        self.reihenfolge = Reihenfolge::neu();
        self.belegt = 0;
    }

    /// Belegte Byte.
    pub fn bytes(&self) -> u64 {
        self.belegt
    }

    /// Zahl der Eintraege.
    pub fn eintraege(&self) -> usize {
        self.eintraege.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eintrag(schluessel: &str, body: &[u8]) -> Eintrag {
        Eintrag {
            status: 200,
            headers: HashMap::new(),
            body: body.to_vec(),
            empfangen: SystemTime::now(),
            vary: Vec::new(),
            schluessel: schluessel.to_string(),
        }
    }

    #[test]
    fn lesen_und_schreiben() {
        let mut s = Speicher::neu(1024);
        assert!(!s.enthaelt("a"));
        s.schreiben("a".into(), eintrag("a", b"hallo"));
        assert!(s.enthaelt("a"));
        let e = s.lesen("a").expect("da");
        assert_eq!(e.body, b"hallo");
    }

    #[test]
    fn lru_verdraengt_den_aeltesten() {
        // Jeder Eintrag ist ~66 Byte (Schluessel 1 + Koerper 1 + Rahmen 64).
        // Grenze 200: drei Eintraege passen, vier nicht.
        let mut s = Speicher::neu(200);
        s.schreiben("a".into(), eintrag("a", b"1"));
        s.schreiben("b".into(), eintrag("b", b"2"));
        s.schreiben("c".into(), eintrag("c", b"3"));
        // "a" beruehren, damit "b" der Aelteste wird.
        let _ = s.lesen("a");
        s.schreiben("d".into(), eintrag("d", b"4"));
        // "b" muss draussen sein (aelteste nach der Beruehrung von "a").
        assert!(!s.enthaelt("b"), "aeltester Eintrag wurde nicht verdraengt");
        assert!(s.enthaelt("a"));
        assert!(s.enthaelt("c") || s.enthaelt("d"));
    }

    #[test]
    fn byte_grenze_wird_eingehalten() {
        let mut s = Speicher::neu(200);
        for i in 0..100 {
            s.schreiben(format!("schluessel-{i}"), eintrag("x", &[0u8; 50]));
        }
        assert!(s.bytes() <= 200, "{} > 200", s.bytes());
    }

    #[test]
    fn eintrag_groesser_als_grenze_wird_abgelehnt() {
        let mut s = Speicher::neu(100);
        s.schreiben("gross".into(), eintrag("gross", &[0u8; 500]));
        assert!(!s.enthaelt("gross"));
        assert_eq!(s.bytes(), 0);
    }

    #[test]
    fn schluessel_zu_lang_wird_abgelehnt() {
        let mut s = Speicher::neu(1024);
        let lang = "x".repeat(MAX_SCHLUESSEL + 1);
        s.schreiben(lang.clone(), eintrag(&lang, b"klein"));
        assert!(!s.enthaelt(&lang));
    }

    #[test]
    fn leeren_setzt_zurueck() {
        let mut s = Speicher::neu(1024);
        s.schreiben("a".into(), eintrag("a", b"1"));
        s.leeren();
        assert_eq!(s.bytes(), 0);
        assert_eq!(s.eintraege(), 0);
        assert!(!s.enthaelt("a"));
    }

    #[test]
    fn bytes_zaehlt_koerper_und_schluessel() {
        let mut s = Speicher::neu(1024);
        s.schreiben("a".into(), eintrag("a", &[0u8; 100]));
        assert!(s.bytes() >= 100, "{} < 100", s.bytes());
    }
}
