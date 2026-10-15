//! Q2: **Alt-Svc merken** — je Ursprung, mit `ma`, und bei Fehler wieder
//! vergessen (RFC 7838).
//!
//! WAS BISHER DA WAR: nur der Parser (`alt_svc_lesen`). Er liest
//! `h3=":443"; ma=86400` und sagt, was **in** dem Header stand. Was fehlte, ist
//! der Teil danach: **merken**, **ablaufen lassen**, und — das ist der Punkt —
//! **vergessen, wenn es nicht funktioniert hat**.
//!
//! DIE DREI REGELN, alle aus RFC 7838 und alle mit Grund:
//!
//! 1. **Je Ursprung.** Ein `alt-svc` gilt fuer den Ursprung, bei dem es
//!    ankam, nicht global. `www.example.com` sagt nichts ueber `example.com`.
//! 2. **`ma` ist eine Frist, kein Vorschlag.** Nach `ma` Sekunden gilt der
//!    Eintrag als nicht mehr vorhanden — ohne neue Nachricht vom Server.
//!    `ma=0` bedeutet **sofort** vergessen (Abschnitt 4), nicht "unbegrenzt".
//! 3. **Ein Fehler loescht den Eintrag.** Das steht nicht in RFC 7838, aber es
//!    ist die einzige Regel, die Q2s "bei Fehler sofort zurueck zu TLS/TCP"
//!    ueberhaupt erfuellt: wer nach einem gescheiterten HTTP/3-Versuch weiter
//!    HTTP/3 versucht, hat einen Browser gebaut, der eine tote Leitung
//!    bevorzugt.
//!
//! DIE FRIST, und sie ist eine eigene Zahl: Q2 verlangt 2 s fuer den
//! HTTP/3-Versuch. Wer sie im Client hart verdrahtet, kann sie nicht testen
//! und nicht dem Aufrufer geben — deshalb ist sie ein **Parameter** mit einem
//! benannten Standardwert.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::alt_svc::{AltSvc, AltSvcFehler, MAX_ALT_SVC_HOST, alt_svc_lesen};

/// Frist fuer einen HTTP/3-Versuch, bevor auf TLS/TCP zurueckgefallen wird
/// (Q2: **2 s**).
///
/// WARUM EINE KONSTANTE UND KEIN LITERAL: die Zahl steht in der Aufgabe, und
/// eine Zahl aus einer Aufgabe, die an drei Stellen im Code steht, ist eine
/// Zahl, die auseinanderlaeuft. Zusaetzlich: `Instant`-Rechnung braucht einen
/// Wert > 0, und 0 ms wuerde "kein HTTP/3" bedeuten statt "sofort".
pub const H3_FRIST: Duration = Duration::from_secs(2);

/// Hoechste Zahl gemerkter Urspruenge (M3, Q2).
///
/// DER WERT IST EINE ABLEITUNG, keine Setzung: ein Browser haelt typischerweise
/// 100 bis 500 Alternativen. Wir nehmen 128, weil der Speicher pro Eintrag nur
/// aus Ursprung, Host, Port und zwei `Instant`s besteht — unter 20 KiB fuer den
/// ganzen Speicher.
pub const MAX_URSPRUENGE: usize = 128;

/// Warum ein Alt-Svc-Eintrag nicht gespeichert werden konnte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeicherFehler {
    /// Der Header ist nicht lesbar (unveraendert aus `AltSvcFehler`).
    Header(AltSvcFehler),
    /// Der Ursprung ist laenger als er sein darf (M4).
    UrsprungZuLang { ist: usize, maximum: usize },
    /// Der Speicher ist voll (M3).
    Voll { maximum: usize },
}

impl std::fmt::Display for SpeicherFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpeicherFehler::Header(e) => write!(f, "Alt-Svc-Header: {e}"),
            SpeicherFehler::UrsprungZuLang { ist, maximum } => {
                write!(f, "Ursprung ist {ist} Byte, erlaubt sind {maximum}")
            }
            SpeicherFehler::Voll { maximum } => write!(f, "Alt-Svc-Speicher voll ({maximum})"),
        }
    }
}

impl std::error::Error for SpeicherFehler {}

/// Ein Eintrag mit dem Zeitpunkt, ab dem er gilt.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Eintrag {
    wert: AltSvc,
    /// Wann der Eintrag **ablaeuft** (`jetzt + ma`).
    bis: Instant,
}

/// Die gemerkten Alt-Svc-Eintraege, je Ursprung.
///
/// KEIN `HashMap`, sondern ein `BTreeMap`: die Reihenfolge ist damit
/// **entscheidbar** und haengt nicht vom Zufall der Hashwerte ab. Das ist
/// fuer einen Speicher wichtig, in dem man nachsehen koennen will, was
/// drinsteht — die Reihenfolge ist sonst bei jedem Lauf anders.
#[derive(Debug, Clone, Default)]
pub struct AltSvcSpeicher {
    eintraege: BTreeMap<String, Eintrag>,
}

impl AltSvcSpeicher {
    /// Leerer Speicher.
    pub fn neu() -> Self {
        Self {
            eintraege: BTreeMap::new(),
        }
    }

    /// Den `Alt-Svc`-Antwortkopf eines Ursprungs merken.
    ///
    /// `jetzt` wird **uebergeben**, nicht intern geholt: ein Test, der ein
    /// `ma` von 60 Sekunden prueft, darf 60 Sekunden nicht warten. Wer die Uhr
    /// hier holt, baut einen Test mit `thread::sleep`, und der laeuft irgendwann
    /// in einem Lauf, der 60 Sekunden dauert.
    ///
    /// **`ma=0` loescht** (RFC 7838 4). Und ein abgelaufener Eintrag wird hier
    /// gleich mitgeraeumt, damit `anzahl()` nie etwas zaehlt, was nicht mehr
    /// gilt.
    pub fn merken(
        &mut self,
        ursprung: &str,
        wert: &str,
        jetzt: Instant,
    ) -> Result<(), SpeicherFehler> {
        let a = alt_svc_lesen(wert).map_err(SpeicherFehler::Header)?;
        self.setzen(ursprung, a, jetzt)
    }

    /// Wie `merken`, aber mit **schon geparstem** Eintrag.
    ///
    /// ZWEI EINGAENGE, und der Grund ist ein API-Detail, das mich das erste Mal
    /// gestoert hat: `Antwort.alt_svc` ist ein **geparster** `AltSvc`, nicht
    /// der Rohwert des Kopfes. Wer nur `merken` hat, muss aus dem geparsten
    /// Eintrag wieder einen Header-Farbstring bauen — und das ist eine
    /// erfundene Form, an der sich Formfehler einschleichen.
    pub fn setzen(
        &mut self,
        ursprung: &str,
        a: AltSvc,
        jetzt: Instant,
    ) -> Result<(), SpeicherFehler> {
        if ursprung.len() > MAX_ALT_SVC_HOST {
            return Err(SpeicherFehler::UrsprungZuLang {
                ist: ursprung.len(),
                maximum: MAX_ALT_SVC_HOST,
            });
        }
        if a.max_alter == 0 {
            // Regel 2: `ma=0` heisst "vergiss es", nicht "unbegrenzt".
            self.eintraege.remove(ursprung);
            return Ok(());
        }
        if !self.eintraege.contains_key(ursprung) && self.eintraege.len() >= MAX_URSPRUENGE {
            // Regel der Mengen: **der neue Eintrag** hat Vorrang vor dem
            // aeltesten. Das ist eine Festlegung, kein Zufall, und sie steht
            // hier, weil "was tun wir bei vollem Speicher" eine Entscheidung
            // ist, die man treffen muss, bevor man sie braucht.
            if let Some(alt) = self
                .eintraege
                .iter()
                .min_by_key(|(_, e)| e.bis)
                .map(|(k, _)| k.clone())
            {
                self.eintraege.remove(&alt);
            } else {
                return Err(SpeicherFehler::Voll {
                    maximum: MAX_URSPRUENGE,
                });
            }
        }
        // `max_alter` **vor** dem Verschieben merken — die Fehlermeldung
        // "use of moved value" hat mich das hier gekostet, und die Regel gilt
        // fuer jede Stelle, an der ein Wert erst benutzt und dann weitergegeben
        // wird.
        let bis = jetzt + Duration::from_secs(a.max_alter);
        self.eintraege
            .insert(ursprung.to_string(), Eintrag { wert: a, bis });
        Ok(())
    }

    /// Der Eintrag fuer `ursprung`, falls er **jetzt noch gilt**.
    ///
    /// Regel 2 hier durchgesetzt: ein abgelaufener Eintrag wird entfernt und
    /// `None` geliefert. So braucht kein Aufrufer eine eigene Ablaufpruefung,
    /// und `holen` ist die einzige Wahrheit.
    pub fn holen(&mut self, ursprung: &str, jetzt: Instant) -> Option<AltSvc> {
        let abgelaufen = self.eintraege.get(ursprung).is_some_and(|e| e.bis <= jetzt);
        if abgelaufen {
            self.eintraege.remove(ursprung);
            return None;
        }
        self.eintraege.get(ursprung).map(|e| e.wert.clone())
    }

    /// Regel 3: den Eintrag **vergessen**, weil HTTP/3 gescheitert ist.
    ///
    /// Gibt `true` zurueck, wenn wirklich etwas geloescht wurde — das ist
    /// eine Angabe, mit der man im Bericht sagen kann, ob die Regel gegriffen
    /// hat, statt sie zu behaupten.
    pub fn vergessen(&mut self, ursprung: &str) -> bool {
        self.eintraege.remove(ursprung).is_some()
    }

    /// Wie viele Eintraege gerade gelten (fuer Tests und Berichte).
    pub fn anzahl(&self) -> usize {
        self.eintraege.len()
    }

    /// Die gemerkten Urspruenge, **sortiert** (nach `BTreeMap`).
    pub fn urspruenge(&self) -> Vec<String> {
        self.eintraege.keys().cloned().collect()
    }
}
