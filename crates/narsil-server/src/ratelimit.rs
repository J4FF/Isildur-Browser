//! Rate-Limit je IP: Token-Bucket in einer BEGRENZTEN Tabelle (M6).
//!
//! # WARUM EIN TOKEN-BUCKET
//!
//! Ein Zaehler, der nach einer Sekunde zuruecksetzt, laesst einen
//! Angreifer in einem Sekundenfenster alles hereinholen, was er will —
//! und in der naechsten Sekunde wieder. Der Eimer laesst stattdessen
//! Kontinuitaet zu: Nachschub pro Sekunde, Obergrenze als Fuellung.
//! Was nicht im Eimer ist, ist nicht erlaubt; was ungenutzt war, faellt
//! langsam zurueck.
//!
//! # WARUM DIE TABELLE BEGRENZT IST (M6)
//!
//! Eine Tabelle, die mit jedem neuen IP-Wachstum waechst, ist ein
//! Speicherleck mit Adresse. [`MAX_IP_TABELLE`] ist die harte Grenze:
//! ist sie voll, faellt der am laengsten inaktive Eintrag heraus (LRU).
//! Der Preis ist ehrlich benannt: wer die Tabelle mit wechselnden IPs
//! voll macht, kann andere Eintraege verdraengen. Die Alternative waere
//! ein unbeschraenkt waechsender Speicher — und der ist teurer als eine
//! verschobene Grenze.
//!
//! # WO ES GREIFT
//!
//! Als innerstes Glied der Middleware-Kette (siehe [`glied`]): ausserhalb
//! stehen Zaehlen, Protokoll, Sicherheits-Header und CORS, also bekommt
//! auch eine abgelehnte Anfrage eine Zeile im Protokoll und die
//! ueblichen Header. Und innerhalb liegt nichts Teures mehr — wer
//! ohnehin abgelehnt wird, soll den Router nicht sehen.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::http::Response;
use crate::middleware::Glied;

/// Maximal viele IP-Eintraege in der Tabelle.
///
/// 1024 Eintraege zu je hoechstens [`MAX_SCHLUESSEL`] Bytes Schluessel
/// sind rund 64 KiB Obergrenze — dazu die Eimer selbst. Das ist die
/// Antwort auf M6: die Tabelle wachst bis hierhin und nie darueber.
pub const MAX_IP_TABELLE: usize = 1024;

/// Maximale Laenge eines Tabellen-Schluessels in Bytes.
///
/// Die Peer-Adresse kommt vom Socket und ist ohnehin kurz; diese
/// Grenze deckelt sie trotzdem, bevor sie in die Tabelle wandert (M4).
pub const MAX_SCHLUESSEL: usize = 64;

/// Obergrenze der Nachschubrate (Anfragen pro Sekunde).
///
/// Eine Zahl ohne Obergrenze waere ein Konfigurationsfehler, der erst
/// zur Laufzeit auffaellt: `f64::INFINITY` wuerde jede Anfrage
/// erlauben und die Grenze zur Attrappe machen. Also wird hier
/// begrenzt, statt zu vertrauen.
pub const MAX_RATE_PRO_SEKUNDE: f64 = 100_000.0;

/// Obergrenze der Eimerfuellung. `0` wuerde gar nichts erlauben,
/// unendlich waere keine Grenze mehr — beides wird abgefangen.
pub const MAX_EIMER: f64 = 100_000.0;

/// Rate-Limit-Einstellung fuer EINE IP-Gruppe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLimit {
    /// Nachschub in Anfragen pro Sekunde (immer > 0).
    pub pro_sekunde: f64,
    /// Maximale Eimerfuellung: wie viele Anfragen in einer Wolke
    /// durchgehen, bevor der Nachschub zusaetzlich gebraucht wird.
    pub eimer: f64,
}

impl RateLimit {
    /// Neu mit Pruefung der Eingaben — nie ein Wert, der die Grenze
    /// unschaedlich machen koennte.
    ///
    /// * `pro_sekunde` `NaN`, `0` oder negativ -> `1.0` (eine Anfrage
    ///   pro Sekunde ist der sanfteste Wert, der noch limittiert),
    /// * `eimer` `NaN` oder `< 1` -> `1.0` (mindestens EINE Anfrage
    ///   darf immer durchgehen, sonst ist es kein Limit, sondern eine
    ///   Sperre),
    /// * alles oberhalb der Maxima — einschliesslich `+inf`, das
    ///   "unbegrenzt" bedeutet und deshalb als OBERGRENZE gelesen
    ///   wird, nicht als "kein Limit" — wird auf die Maxima gezogen.
    pub fn neu(pro_sekunde: f64, eimer: f64) -> Self {
        RateLimit {
            pro_sekunde: if pro_sekunde.is_nan() || pro_sekunde <= 0.0 {
                1.0
            } else {
                pro_sekunde.min(MAX_RATE_PRO_SEKUNDE)
            },
            eimer: if eimer.is_nan() || eimer < 1.0 {
                1.0
            } else {
                eimer.min(MAX_EIMER)
            },
        }
    }
}

impl Default for RateLimit {
    /// Zehn Anfragen pro Sekunde, Eimer auf fuenf — eine Grenze, die
    /// einen echten Nutzer nie sieht und einen Spammer nach fuenf
    /// Anfragen bremst.
    fn default() -> Self {
        RateLimit::neu(10.0, 5.0)
    }
}

// ---------------------------------------------------------------------------
// Der Eimer
// ---------------------------------------------------------------------------

/// Ein Token-Bucket EINER IP. Nicht thread-faehig, wird unter dem
/// Schloss der Tabelle benutzt.
#[derive(Debug, Clone, Copy)]
struct Eimer {
    /// Verfuegbare Anfragen (steigt durch Nachschub, nie ueber `eimer`).
    tokens: f64,
    /// Letzter Zugriff: Grundlage fuer Nachschub UND fuer LRU.
    letzter: Instant,
}

impl Eimer {
    /// Eimer ZU DEN ANFANGSBEDINGUNGEN einer neuen IP: voll, und der
    /// erste Zugriff zieht sofort einen Token.
    ///
    /// Leere Fuellung waere falsch: eine neue Adresse duerfte nicht
    /// einmal EINE Anfrage schicken, waehrend sie ohnehin unbekannt ist.
    fn neu(jetzt: Instant, grenze: &RateLimit) -> Self {
        Eimer {
            tokens: grenze.eimer,
            letzter: jetzt,
        }
    }

    /// Einen Token ziehen. `true` = erlaubt.
    ///
    /// Der Nachschub rechnet mit der VERGANGENEN Zeit und faellt nie
    /// ueber `eimer` hinaus — sonst waere die Obergrenze nur ein
    /// Vorschub, keine Grenze mehr. `saturating_duration_since`
    /// verhindert, dass eine zuruecklaufende Uhr (oder ein in der
    /// Vergangenheit konstruierter Zeitpunkt) hier paniken koennte.
    fn nehme(&mut self, jetzt: Instant, grenze: &RateLimit) -> bool {
        let vergangen = jetzt.saturating_duration_since(self.letzter).as_secs_f64();
        self.tokens = (self.tokens + vergangen * grenze.pro_sekunde).min(grenze.eimer);
        self.letzter = jetzt;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Wie lange es dauert, bis wieder EIN Token da ist.
    fn wartezeit(&self, grenze: &RateLimit) -> Duration {
        let fehlend = (1.0 - self.tokens).max(0.0);
        let sek = fehlend / grenze.pro_sekunde;
        if sek.is_finite() && sek > 0.0 {
            Duration::from_secs_f64(sek.min(3600.0))
        } else {
            Duration::ZERO
        }
    }
}

// ---------------------------------------------------------------------------
// Die Tabelle
// ---------------------------------------------------------------------------

/// Der Inhalt unter dem Schloss — getrennt von der Tabelle, damit
/// `Debug` und Zaehler das Schloss nicht brauchen.
#[derive(Default)]
struct Karte {
    karte: HashMap<String, Eimer>,
}

/// Die Tabelle aller Eimer.
///
/// Ein Schloss, weil zwei Faelle zusammentreffen: derselbe IP
/// gleichzeitig (Nachschub muss exakt bleiben) und zwei verschiedene
/// IPs gleichzeitig (LRU-Verdraengung darf niemals dasselbe zweimal
/// entfernen).
pub struct Tabelle {
    grenze: RateLimit,
    karte: Mutex<Karte>,
    /// Wie viele Anfragen durchgekommen sind.
    zugelassen: AtomicU64,
    /// Wie viele abgelehnt wurden.
    abgelehnt: AtomicU64,
}

impl Tabelle {
    /// Neue Tabelle mit gepruefter Grenze.
    pub fn neu(grenze: RateLimit) -> Self {
        Tabelle {
            grenze,
            karte: Mutex::new(Karte::default()),
            zugelassen: AtomicU64::new(0),
            abgelehnt: AtomicU64::new(0),
        }
    }

    /// Ist diese IP gerade erlaubt?
    ///
    /// Rueckgabe: `None` = ja, `Some(wartezeit)` = nein, samt Dauer,
    /// die dem Client als `Retry-After` mitgegeben werden kann.
    ///
    /// `jetzt` ist ein Parameter, damit die Rechnung ohne Warten
    /// testbar ist (Determinismus statt `sleep`).
    pub fn erlaubt(&self, ip: &str, jetzt: Instant) -> Option<Duration> {
        // Leere Adresse wuerde alle Unbekannten in EINEN Eimer legen;
        // Kuerzen deckelt die Laenge, bevor sie gespeichert wird (M4).
        let roh = if ip.is_empty() { "unbekannt" } else { ip };
        let schluessel = kuerzen(roh);
        let mut karte = self.schloss();

        // Vorhanden? Dann entscheidet der Eimer.
        if let Some(e) = karte.karte.get_mut(schluessel) {
            if e.nehme(jetzt, &self.grenze) {
                self.zugelassen.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            let w = e.wartezeit(&self.grenze);
            self.abgelehnt.fetch_add(1, Ordering::Relaxed);
            return Some(w);
        }

        // Neu? Dann braucht der Eintrag Platz.
        if karte.karte.len() >= MAX_IP_TABELLE {
            // LRU: der am laengsten unbenutzte Eintrag faellt heraus.
            // Die Schleife laeuft ueber hoechstens MAX_IP_TABELLE
            // Eintraege — eine Grenze aus der KONFIGURATION, nicht aus
            // der Eingabe (M3).
            if let Some(alt) = karte
                .karte
                .iter()
                .min_by_key(|(_, e)| e.letzter)
                .map(|(k, _)| k.clone())
            {
                karte.karte.remove(&alt);
            }
        }
        let mut e = Eimer::neu(jetzt, &self.grenze);
        let erlaubt = e.nehme(jetzt, &self.grenze);
        karte.karte.insert(schluessel.to_string(), e);
        if erlaubt {
            self.zugelassen.fetch_add(1, Ordering::Relaxed);
            None
        } else {
            // Nicht erreichbar, solange `eimer >= 1` gilt (durch
            // `RateLimit::neu` erzwungen) — der Pfad bleibt trotzdem
            // vollstaendig, damit keine Konfiguration ihn umgeht.
            let w = Duration::from_secs_f64(1.0 / self.grenze.pro_sekunde);
            self.abgelehnt.fetch_add(1, Ordering::Relaxed);
            Some(w)
        }
    }

    /// Wie viele IP-Eintraege gerade drin sind (fuer Tests und Status).
    pub fn eintraege(&self) -> usize {
        self.schloss().karte.len()
    }

    /// Anfragen, die durchgekommen sind.
    pub fn zugelassen(&self) -> u64 {
        self.zugelassen.load(Ordering::Relaxed)
    }

    /// Anfragen, die abgelehnt wurden.
    pub fn abgelehnt(&self) -> u64 {
        self.abgelehnt.load(Ordering::Relaxed)
    }

    /// Schloss holen, ohne paniken zu koennen: ein panikender Thread
    /// duerfte das Schloss nicht fuer alle Zukunft vergiften (gleiche
    /// Regel wie `server::Warteschlange::lock`).
    fn schloss(&self) -> MutexGuard<'_, Karte> {
        self.karte.lock().unwrap_or_else(|p| p.into_inner())
    }
}

// ---------------------------------------------------------------------------
// Die Peer-Adresse zum Schluessel
// ---------------------------------------------------------------------------

/// Aus `127.0.0.1:54321` wird `127.0.0.1`, aus `[::1]:80` wird `[::1]`.
///
/// Ohne diese Stelle wuerde jeder PORT eine eigene Eimer-Gruppe haben —
/// ein Angreifer bekaeme mit einer Verbindung pro Port unbegrenzt
/// Anfragen. Der Schluessel wird ausserdem auf [`MAX_SCHLUESSEL`]
/// Bytes gekuerzt (M4), auf Zeichengrenze, nie mitten im UTF-8.
pub fn ip_schluessel(peer: &str) -> &str {
    let roh = if let Some(innen) = peer.strip_prefix('[') {
        // [::1]:8080 -> [::1]
        match innen.find(']') {
            Some(i) => &peer[..=i + 1],
            None => peer,
        }
    } else if let Some(i) = peer.rfind(':') {
        // 127.0.0.1:8080 -> 127.0.0.1
        &peer[..i]
    } else {
        peer
    };
    kuerzen(roh)
}

/// Auf [`MAX_SCHLUESSEL`] Bytes kuerzen, aber auf CHARAKTERGRENZE.
fn kuerzen(s: &str) -> &str {
    if s.len() <= MAX_SCHLUESSEL {
        return s;
    }
    let mut g = MAX_SCHLUESSEL;
    while g > 0 && !s.is_char_boundary(g) {
        g -= 1;
    }
    &s[..g]
}

// ---------------------------------------------------------------------------
// Das Middleware-Glied
// ---------------------------------------------------------------------------

/// Ein Glied, das ab der N-ten Anfrage einer IP mit `429` antwortet.
///
/// Der `Retry-After`-Header nennt die Sekunden bis zum naechsten
/// Token — ein Client, der ihn liest, kann warten; einer, der ihn
/// ignoriert, bekommt dieselbe Antwort wieder, bis der Eimer
/// nachgefuellt ist.
pub fn glied(tabelle: Arc<Tabelle>) -> Glied {
    Arc::new(move |anfrage, weiter| {
        let jetzt = Instant::now();
        match tabelle.erlaubt(ip_schluessel(&anfrage.peer), jetzt) {
            None => weiter(anfrage),
            Some(warte) => {
                let sek = warte.as_secs().max(1);
                Response::fehler(429, "Zu viele Anfragen von dieser Adresse")
                    .mit_header("Retry-After", &sek.to_string())
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const GRENZE: RateLimit = RateLimit {
        pro_sekunde: 2.0,
        eimer: 2.0,
    };

    #[test]
    fn eimer_zaehlt_ab_und_faellt_nach() {
        let t = Tabelle::neu(GRENZE);
        let t0 = Instant::now();
        assert!(t.erlaubt("10.0.0.1:1000", t0).is_none());
        assert!(t.erlaubt("10.0.0.1:1000", t0).is_none());
        // Dritte Anfrage im selben Moment: Eimer leer.
        let w = t
            .erlaubt("10.0.0.1:1000", t0)
            .expect("muss abgelehnt werden");
        assert!(!w.is_zero(), "Retry-After muss eine Dauer nennen");
        // Nach 500 ms bei 2/s -> 1.0 Token -> wieder erlaubt.
        let spaeter = t0 + Duration::from_millis(500);
        assert!(t.erlaubt("10.0.0.1:1000", spaeter).is_none());
        assert_eq!(t.zugelassen(), 3);
        assert_eq!(t.abgelehnt(), 1);
    }

    #[test]
    fn eimer_steigt_nie_ueber_die_obergrenze() {
        let t = Tabelle::neu(GRENZE);
        let t0 = Instant::now();
        assert!(t.erlaubt("a:1", t0).is_none());
        assert!(t.erlaubt("a:1", t0).is_none());
        assert!(t.erlaubt("a:1", t0).is_some());
        // Eine Stunde Pause: maximal EIMER Tokens, nicht 7200.
        let spaeter = t0 + Duration::from_secs(3600);
        assert!(t.erlaubt("a:1", spaeter).is_none());
        assert!(t.erlaubt("a:1", spaeter).is_none());
        assert!(t.erlaubt("a:1", spaeter).is_some(), "Obergrenze gehalten");
    }

    #[test]
    fn neue_ip_darf_eine_anfrage() {
        let t = Tabelle::neu(GRENZE);
        assert!(
            t.erlaubt("frisch.example:9", Instant::now()).is_none(),
            "eine unbekannte Adresse startet mit vollem Eimer"
        );
    }

    #[test]
    fn verschiedene_ips_sind_getrennt() {
        let t = Tabelle::neu(GRENZE);
        let t0 = Instant::now();
        assert!(t.erlaubt("1.1.1.1:1", t0).is_none());
        assert!(t.erlaubt("1.1.1.1:1", t0).is_none());
        assert!(t.erlaubt("1.1.1.1:1", t0).is_some());
        // Andere IP, volle Rechte.
        assert!(t.erlaubt("2.2.2.2:1", t0).is_none());
        assert_eq!(t.eintraege(), 2);
    }

    #[test]
    fn tabelle_waechst_nie_ueber_max_ip_tabelle_hinaus() {
        let t = Tabelle::neu(GRENZE);
        let t0 = Instant::now();
        // 5.000 IPs: dreifach so viele wie Platz ist.
        for i in 0..5_000 {
            let ip = format!("10.{}.{}.{}:7", i / 65_536, (i / 256) % 256, i % 256);
            assert!(t.erlaubt(&ip, t0).is_none());
            assert!(t.eintraege() <= MAX_IP_TABELLE);
        }
        assert_eq!(t.eintraege(), MAX_IP_TABELLE);
    }

    #[test]
    fn schluessel_kennt_nur_die_ip_nicht_den_port() {
        assert_eq!(ip_schluessel("127.0.0.1:8080"), "127.0.0.1");
        assert_eq!(ip_schluessel("[::1]:8080"), "[::1]");
        assert_eq!(ip_schluessel("[fe80::1%eth0]:80"), "[fe80::1%eth0]");
        assert_eq!(ip_schluessel("ohne-port"), "ohne-port");
        assert_eq!(ip_schluessel(""), "");
        // Zwei Ports derselben Adresse sind EIN Eimer.
        let t = Tabelle::neu(GRENZE);
        let t0 = Instant::now();
        assert!(t.erlaubt(ip_schluessel("9.9.9.9:1000"), t0).is_none());
        assert!(t.erlaubt(ip_schluessel("9.9.9.9:2000"), t0).is_none());
        assert!(t.erlaubt(ip_schluessel("9.9.9.9:3000"), t0).is_some());
    }

    #[test]
    fn schluessel_wird_auf_max_kuerzt_und_zerstoert_kein_utf8() {
        let lang = "ä".repeat(200); // 400 Bytes UTF-8
        let roh = format!("{lang}:80");
        let k = ip_schluessel(&roh);
        assert!(k.len() <= MAX_SCHLUESSEL);
        assert!(k.is_char_boundary(k.len()), "kein abgeschnittenes Zeichen");
        // Auch der direkte Weg durch die Tabelle ist gekuerzt.
        let t = Tabelle::neu(GRENZE);
        assert!(t.erlaubt(&lang, Instant::now()).is_none());
        assert!(t.eintraege() == 1);
    }

    #[test]
    fn unsinnige_konfiguration_wird_zu_einer_grenze() {
        for (rate, eimer) in [
            (0.0, 0.0),
            (-5.0, -1.0),
            (f64::NAN, f64::NAN),
            (f64::INFINITY, f64::INFINITY),
        ] {
            let g = RateLimit::neu(rate, eimer);
            assert!(g.pro_sekunde.is_finite() && g.pro_sekunde > 0.0, "{rate}");
            assert!(g.eimer.is_finite() && g.eimer >= 1.0, "{eimer}");
            assert!(g.pro_sekunde <= MAX_RATE_PRO_SEKUNDE);
            assert!(g.eimer <= MAX_EIMER);
        }
        // Oben raus: begrenzt statt unendlich.
        let g = RateLimit::neu(f64::INFINITY, 1e9);
        assert_eq!(g.pro_sekunde, MAX_RATE_PRO_SEKUNDE);
        assert_eq!(g.eimer, MAX_EIMER);
    }

    #[test]
    fn lru_verdraengt_den_aeltesten_nicht_den_juengsten() {
        // Sehr langsame Nachschub (1 Token je 1000 s) UND Eimer auf
        // EINS: dann faellt nichts zusaetzlich nach und der Stand ist
        // nach JEDEM Aufruf bekannt — erlaubt heisst "Eimer war voll,
        // ist jetzt leer", abgelehnt heisst "war schon leer". Ohne
        // `sleep`, ohne Warten, ohne Vermutungen.
        let langsam = RateLimit::neu(0.001, 1.0);
        let t = Tabelle::neu(langsam);
        let t0 = Instant::now();
        for i in 0..MAX_IP_TABELLE {
            // Zeitstempel in Mikrosekunden: eindeutige Reihenfolge,
            // aber zu kurz fuer messbaren Nachschub.
            let jetzt = t0 + Duration::from_micros(i as u64);
            let ip = format!("i{i}.0.0.1:1");
            assert!(t.erlaubt(&ip, jetzt).is_none()); // 1 -> 0, erlaubt
            assert!(t.erlaubt(&ip, jetzt).is_some()); // leer -> abgelehnt
        }
        assert_eq!(t.eintraege(), MAX_IP_TABELLE);

        // Jetzt ist die Tabelle voll UND alle Eimer leer (0 Tokens).
        // Ein neuer Eintrag verdraengt den am laengsten inaktiven: i0.
        let jetzt = t0 + Duration::from_secs(60);
        let neu = format!("neu{}.0.0.1:1", MAX_IP_TABELLE);
        assert!(t.erlaubt(&neu, jetzt).is_none());
        assert_eq!(t.eintraege(), MAX_IP_TABELLE);

        // i1 ist noch da: leerer Eimer -> abgelehnt. Waere er raus,
        // begaennte die Tabelle ihn neu mit vollem Eimer -> erlaubt.
        assert!(
            t.erlaubt("i1.0.0.1:1", jetzt).is_some(),
            "zweitaeltester Eintrag haette noch da sein muessen"
        );
        // i1023 ist auch noch da (juengster, zuletzt benutzt).
        assert!(t.erlaubt("i1023.0.0.1:1", jetzt).is_some());
        // i0 ist raus: er kehrt mit vollem Eimer zurueck.
        assert!(
            t.erlaubt("i0.0.0.1:1", jetzt).is_none(),
            "aeltester Eintrag haette verdraengt sein muessen"
        );
        assert_eq!(t.eintraege(), MAX_IP_TABELLE);
    }
}
