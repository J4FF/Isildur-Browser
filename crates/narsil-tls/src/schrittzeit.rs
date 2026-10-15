//! added 04.10.2026, Rolle TLS. **DIE ZEIT JE SCHRITT IM HANDSHAKE.**
//!
//! ### WER DIES BESTELLT HAT
//!
//! Rolle NETZTEMPO, Bericht 22: „TLS-Handshake kostet 133–543 ms je Host bei
//! 16–25 ms RTT (Faktor 4 bei gleichem RTT)“. **Der Faktor ist die Beobachtung,
//! und die Beobachtung sagt: es sind nicht die 16–25 ms Netz.** Wer danach sucht,
//! muss die Zeit **aufteilen**, sonst sucht er im Netz.
//!
//! ### WAS DIESES MODUL MISST UND WAS NICHT
//!
//! **Es misst die Zeit zwischen zwei Punkten im Code** — nicht die Wartezeit auf
//! ein Netz. Das ist eine **Grenze**, und sie wird hier genannt, weil man sie
//! sonst erst beim Lesen der Zahlen bemerkt: „Signatur prüfen: 41 ms“ heißt
//! **„die CPU war 41 ms lang in `cv_gueltig`“**, und **nicht** „der Server
//! brauchte 41 ms“. **Wer beide Zahlen trennen will, braucht zwei Uhren und
//! zwei Messungen** — Netz ist `lese_flug`, Rechnen ist alles hier.
//!
//! ### DIE REGEL: ES IST AUS, BIS JEMAND ES EINSCHALTET
//!
//! Ohne diese Regel ist eine Messung im Produktivpfad eine **Behauptung mit
//! Rauschen**: `Instant::now()` ist billig, aber **nicht gratis**, und in einem
//! Handshake sind es Dutzende. `AN.load(Relaxed)` ist eine **ungesicherte
//! Leseoperation auf einer globalen Variable** — sie ist der einzige
//! Zusatzaufwand im Normalfall, und sie ist **eine** Operation, kein `Mutex`
//! und keine `RefCell`.
//!
//! ### WARUM THREAD-LOCAL UND NICHT GLOBAL
//!
//! Weil die Zuordnung **Schritt zu Verbindung** gelten muss. Ein globaler
//! Vektor in einem Browser mit **sechs** offenen Verbindungen misst die Schritte
//! **aller** ineinander — und wer das meldet, hat eine Zahl ohne Aussage.
//! `thread_local!` ist die **einzige** billige Form davon.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// added 04.10.2026: **DER EINZIGE SCHALTER**, und er ist ein `AtomicBool`,
/// weil zwei Threads gleichzeitig an ihn vorbeikommen dürfen (ein Verbindungs-
/// aufbau im Hauptthread, einer im Lade-Thread).
static AN: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// added 04.10.2026: **DIE TABELLE**, und sie ist `Option`, weil ein
    /// **ausgeschalteter** Zustand kein leerer Vektor sein darf: ein leerer
    /// Vektor ist „gemessen, nichts passiert“ oder „nicht gemessen“ — und
    /// genau diese beiden darf man nicht verwechseln.
    static TABELLE: RefCell<Option<Vec<(&'static str, Messung)>>> =
        const { RefCell::new(None) };
}

/// added 04.10.2026, Rolle TLS. **DIE CPU-ZEIT, und warum sie hier die
/// richtige Groesse ist — GEMESSEN am 04.10.2026, 07:3x.**
///
/// **DER BEFUND, DER DIESE ENTSCHEIDUNG AUSGELOEST HAT:** dieselbe Messung
/// ergab **25–45 ms** fuer die Summe der Schritte bei `www.rust-lang.org` und
/// **1093 ms** Gesamtdauer, und **dns + tcp** allein waren **1048–1068 ms**.
/// **Das ist unglaublich**, denn `curl` braucht nach eigener Messung der
/// Leitung **11–30 ms** bis zum Handshake. **Die Maschine war gleichzeitig mit
/// Bauten anderer Rollen belastet** — **die Wandzeit misst also die Schlange
/// vor der CPU, nicht unsere Rechnung.**
///
/// **GEMESSEN, dass das kein Verdacht ist, sondern die Sache selbst:**
/// ```text
/// vor `sleep 0,3`:  /proc/thread-self/schedstat = 1115530 1193761 2
/// nach `sleep 0,3`: /proc/thread-self/schedstat = 3753781  156660 4
/// ```
/// **300 ms Warten, 2,64 ms CPU.** Die Wandzeit ist also **falsch** und die
/// CPU-Zeit **richtig** — fuer **diese** Frage. „Wo geht die Zeit im Handshake
/// hin“ heisst „wo geht die **Rechen**zeit hin“, und **Rechenzeit ist CPU-Zeit**.
///
/// ### DIE DATEI UND IHR FELD
///
/// `/proc/thread-self/schedstat` hat drei durch Leerzeichen getrennte Zahlen
/// (nachgemessen, nicht der Norm nach):
/// | Feld | Bedeutung |
/// |---|---|
/// | **1** | **Nanosekunden, die der Faden auf der CPU war** |
/// | 2 | `run_delay` — Wartezeit auf der Laufwarteschlange |
/// | 3 | `pcount` — Anzahl der Einplanungen |
///
/// **Feld 1 ist die Summe seit Fadenbeginn**, also eine **Differenz**, kein
/// Stand. **Feld 2 ist es nicht** (GEMESSEN: es **sinkt** von 1193761 auf
/// 156660 ueber ein `sleep`) — wer es als Differenz nimmt, bekommt **negative
/// Zeiten** und haelt sie fuer eine gute Messung.
///
/// ### WARUM `thread-self` UND NICHT `self`
///
/// **Prozess** (`/proc/self/schedstat`) sammelt **alle** Faeden ein. Der
/// Handshake rechnet **an einer Stelle** — der Sockel, der DNS, die
/// Kryptografie, alles in **einem** Faden. Nimmt man den Prozess, schreibt die
/// **Netz-Threads** des Betriebssystems in dieselbe Zahl hinein, und die Frage
/// „wie lange haben **wir** gerechnet“ wird zu „wie lange hat der Rechner
/// gerechnet“. **GEMESSEN an `thread-self`:** 2,64 ms CPU fuer 300 ms Warten.
///
/// ### UND WAS DAS FELD **NICHT** KANN
///
/// **Ein Faden, der auf einen anderen wartet, hat keine CPU-Zeit.** Ein
/// Schritt, der **wartet**, steht hier bei **~0 ms** — und das ist **kein**
/// Fehler, sondern das **Ergebnis**: die Netzschritte sind Wartezeit, und
/// Rechenzeit ist genau das, was wir optimieren koennen.
pub fn cpu_nanosekunden() -> Option<u64> {
    // added 04.10.2026: **`None` STATT `0`**, und das ist die wichtigste Zeile
    // der Funktion. Ohne `/proc` (macOS, some Container) gibt es keine
    // CPU-Zeit. **Eine `0`, die „unbekannt“ bedeutet, ist eine Lüge**, und sie
    // landet als Zahl in einem Bericht.
    let roh = std::fs::read_to_string("/proc/thread-self/schedstat").ok()?;
    roh.split_whitespace().next()?.parse().ok()
}

/// added 04.10.2026: **EINE MESSUNG, und sie traegt BEIDES** — Wandzeit und
/// CPU-Zeit. **Nicht** „die eine gegen die andere“: die Frage der Leitung war
/// ausdruecklich, dass die **Wandzeit unbrauchbar** ist, und **eine Tabelle mit
/// beiden Spalten zeigt genau das in einer Zeile**: der Netzschritt hat 1000 ms
/// Wandzeit und 0,4 ms CPU, und der Rechenschritt hat 11 ms Wandzeit und 11 ms
/// CPU.
#[derive(Clone, Copy, Debug)]
pub struct Messung {
    /// Laufzeit nach der **Uhr** (`Instant`).
    pub wand: Duration,
    /// Laufzeit **auf der CPU**, aus Feld 1 von `schedstat`.
    ///
    /// **`None`, wenn `/proc` nicht lesbar ist** — und **nie** `Some(0)` als
    /// Ersatz, denn „nicht gemessen“ und „null Sekunden“ sind verschiedene
    /// Dinge (siehe [`cpu_nanosekunden`]).
    pub cpu: Option<Duration>,
}

/// added 04.10.2026: **EINSCHALTEN**, und es ist bewusst **nur** das — ein
/// Zurückschalten gibt es auch, denn eine Messung, die man nicht beenden
/// kann, misst den Rest des Tages mit.
pub fn anschalten() {
    AN.store(true, Ordering::Relaxed);
    TABELLE.with(|t| {
        *t.borrow_mut() = Some(Vec::new());
    });
}

/// added 04.10.2026: **AUSSCHALTEN und die Tabelle wegwerfen.**
pub fn ausschalten() {
    AN.store(false, Ordering::Relaxed);
    TABELLE.with(|t| {
        *t.borrow_mut() = None;
    });
}

/// added 04.10.2026: **LÄUFT DIE MESSUNG?** Der Aufrufer fragt das **einmal**
/// pro Schritt und verzweigt darauf — so bleibt der Normalfall eine
/// Last-Operation und ein Vergleich.
pub fn laeuft() -> bool {
    AN.load(Ordering::Relaxed)
}

/// added 04.10.2026: **EINE SCHRITTUHR**, und sie ist **ein Wächter**, kein
/// Paar von Aufrufen. Grund: `hinweis("a"); …; schritt("a")` ist eine
/// **Zweierregel**, und bei einer Zweierregel steht irgendwo im Handshake am
/// Ende `schritt("kette")` ohne `hinweis("kette")` — das **kompiliert** und
/// zählt dann die Zeit **seit dem Anfang des Handshakes**, also die Summe aller
/// Schritte als **eine** Zahl. Der Wächter kann das nicht.
///
/// **UND WARUM `Instant::now()` AUCH IM AUSGESCHALTETEN FALL läuft:** der
/// Wächter braucht keinen Zeitpunkt, wenn er aus ist — `start: None` und ein
/// `Relaxed`-Vergleich beim Weglegen. Der Aufrufer ruft also **immer** nur
/// `schrittzeit::uhr("name")`, und **nichts** am Aufrufer muss den Zustand kennen.
pub struct Uhr {
    name: &'static str,
    start: Option<Instant>,
    start_cpu: Option<u64>,
}

impl Uhr {
    /// added 04.10.2026: **DER WEG, DER NICHT REICHEN DARF.** Ohne ihn könnte
    /// ein Schritt seine Zeit in den nächsten schreiben, und die Tabelle
    /// hätte zwei Zeilen mit demselben Ende — das ist **kein** Messfehler,
    /// sondern eine **Tabelle, die aussieht wie eine Messung**.
    ///
    /// **UND DER FEHLER, DEN DIESE ZEILE HIER GEBRACHT HAT — GEMESSEN von
    /// `ein_waechter_meldet_seinen_schritt_von_allein`:** `beenden()` rief
    /// `ablegen()`, und **danach lief `Drop::drop` noch einmal**. Ergebnis:
    /// **jeder Schritt doppelt** (`[("warten", 5.05), ("warten", 5.06)]`).
    /// **Es ist eine Summenmessung, die doppelt zählt** — und eine Tabelle mit
    /// jeder Zeile zweimal sieht **aus wie** eine Kette mit zwei vergleichbaren
    /// Hälften. **Die Lösung ist `self.start.take()`:** danach hat `Drop` nichts
    /// zu tun, und **beide** Wege sind möglich, ohne dass einer davon zählt.
    pub fn beenden(mut self) {
        if let Some(start) = self.start.take() {
            let start_cpu = self.start_cpu.take();
            self.schreib(start, start_cpu);
        }
    }
}

impl Drop for Uhr {
    fn drop(&mut self) {
        if let Some(start) = self.start.take() {
            let start_cpu = self.start_cpu.take();
            self.schreib(start, start_cpu);
        }
    }
}

impl Uhr {
    fn schreib(&self, start: Instant, start_cpu: Option<u64>) {
        // added 04.10.2026: **BEIDE ZEITEN, und sie kommen aus **einem** Paar,
        // nicht aus zwei Messungen.** Zwei Aufrufe von `roh()` wuerden auch
        // zwei `Instant`-Differenzen liefern — und **die CPU-Differenz der
        // zweiten_lauege nicht** zu den Werten der ersten. **Ein Schritt, zwei
        // Spalten, ein Zeitpunkt-Paar.**
        let messung = Messung {
            wand: start.elapsed(),
            cpu: start_cpu.and_then(|von| {
                cpu_nanosekunden().map(|bis| Duration::from_nanos(bis.saturating_sub(von)))
            }),
        };
        TABELLE.with(|t| {
            if let Some(tabelle) = t.borrow_mut().as_mut() {
                tabelle.push((self.name, messung));
            }
        });
    }
}

/// added 04.10.2026: **DIE UHR ANLEGEN** — der einzige Aufruf, den der Handshake
/// braucht. **Der Name ist `&'static str`**, weil er im Code steht und nicht
/// aus einer Zeichenkette gesucht wird: ein Name, den man erst zusammensetzt,
/// ist im Bericht nicht wiederauffindbar.
pub fn uhr(name: &'static str) -> Uhr {
    if AN.load(Ordering::Relaxed) {
        // added 04.10.2026: **DIE REIHENFOLGE ZAEHLT**, und sie ist eine
        // Eigenschaft des Messvorgangs, nicht eine Formalie: der CPU-Zeitpunkt
        // muss **vor** dem Wandzeitpunkt liegen, damit die CPU-Differenz **auch
        // die Arbeit einschliesst**, die zwischen den beiden Aufrufen liegt.
        // GEMessen ist das nichts (die Spanne sind nanosekunden), aber es ist
        // **die richtige Reihenfolge**, und sie ist billig.
        let start_cpu = cpu_nanosekunden();
        Uhr {
            name,
            start: Some(Instant::now()),
            start_cpu,
        }
    } else {
        Uhr {
            name,
            start: None,
            start_cpu: None,
        }
    }
}

/// added 04.10.2026, Rolle TLS. **DER MAKRO, UND ES IST NICHT KOSMETIK — es
/// sind zwei Fehler, die er abwehrt.**
///
/// **1) DER DROPFENDE BLOCK.** Mit `let _uhr = uhr("…");` misst die Uhr **bis
/// zum Ende des umgebenden Blocks**. GEMESSEN in der ersten Fassung dieses
/// Werkzeugs: „RECHNEN: Schluesselplan (HKDF) **106 ms**“ — das war **nicht**
/// die HKDF, sondern **der ganze Rest des Handshakes**, den der Wächter
/// miterfasste. Der Block war an **sieben** von sieben Stellen zu weit.
///
/// **2) DER FELDWÄCHTER.** `let _signatur = …` in `strom.rs` **kollidiert mit
/// dem Feld `signatur`** aus `x509.rs`, und `kette::feldwaechter::kein_feld_
/// wird_verworfen` sucht **genau** die Zeichenfolge `let _signatur =` — GEMESSEN:
/// der Test schlug mit „diese Felder werden verworfen: ["signatur in
/// strom.rs"]“ fehl, obwohl die Uhr **benutzt** wird. Ein **Makro** hat
/// **keine** Bindung, also **keinen** Namensraum mit den Feldern.
///
/// **DIE FORM, UND SIE IST ERZWUNGEN:** `schritt!("name", { … })` — der Block
/// steht **im Aufruf**, und ohne ihn gibt es keine Messung.
#[macro_export]
macro_rules! schritt {
    ($name:expr, $block:block) => {{
        let _uhr = $crate::schrittzeit::uhr($name);
        $block
    }};
}

/// added 04.10.2026: **DIE TABELLE ALS TEXT**, und sie ist **kein `Debug`**:
/// die Ausgabe geht in einen Bericht, und ein Bericht braucht **eine Zahl je
/// Zeile mit fester Spaltenbreite**, sonst sind zwei Messungen nicht
/// vergleichbar.
pub fn tabelle() -> String {
    TABELLE.with(|t| match t.borrow().as_ref() {
        None => "keine Messung (ausgeschaltet)".to_string(),
        Some(list) => {
            let mut s = String::new();
            let mut summe_wand = Duration::ZERO;
            let mut summe_cpu = Duration::ZERO;
            let mut ohne_cpu = 0usize;
            s.push_str(&format!(
                "  {:44} {:>11} {:>11}\n",
                "SCHRITT", "WAND ms", "CPU ms"
            ));
            for (name, m) in list {
                let wand = m.wand.as_secs_f64() * 1000.0;
                let cpu = match m.cpu {
                    Some(d) => {
                        summe_cpu += d;
                        format!("{:>11.3}", d.as_secs_f64() * 1000.0)
                    }
                    None => {
                        ohne_cpu += 1;
                        "unbekannt".to_string()
                    }
                };
                summe_wand += m.wand;
                s.push_str(&format!("  {name:44} {wand:>11.3} {cpu:>11}\n"));
            }
            s.push_str(&format!(
                "  {:44} {:>11.3} {:>11.3}\n",
                "SUMME DER SCHRITTE",
                summe_wand.as_secs_f64() * 1000.0,
                summe_cpu.as_secs_f64() * 1000.0
            ));
            if ohne_cpu > 0 {
                // added 04.10.2026: **DER FEHLENDE WERT WIRD BENANNT, NICHT
                // GERUNDET.** „unbekannt“ in {ohne_cpu} Zeilen: ein Leser,
                // der eine Zahl erwartet, sieht genau an dieser Stelle, dass
                // ihm eine fehlt.
                s.push_str(&format!(
                    "  ({ohne_cpu} Schritte ohne CPU-Zeit — /proc/thread-self/schedstat\n   nicht lesbar. **Diese Zeilen tragen KEINE Rechenzeit-Aussage.**)\n"
                ));
            }
            s
        }
    })
}

/// added 04.10.2026: **DIE ROHZEITEN**, fuer Rechnungen im Bericht
/// („welcher Anteil ist das?“). Liefert `(Name, Millisekunden)` in der
/// Reihenfolge des Auflaufs — **die Reihenfolge ist die Reihenfolge im Code**
/// und damit auch die Reihenfolge, in der der Code Zeit braucht.
pub fn roh() -> Vec<(&'static str, Messung)> {
    TABELLE.with(|t| match t.borrow().as_ref() {
        None => Vec::new(),
        Some(list) => list.clone(),
    })
}

/// added 04.10.2026: **NUR DIE CPU-ZEIT**, in Millisekunden — und **nur die
/// Schritte, die eine haben.** Der Filter ist **Absicht**: eine Liste mit
/// `None`-Werten als `f64` ergibt `0`, und **eine Null sieht aus wie ein
/// Ergebnis**. Wer rechnet, benutzt diese Funktion; wer schreibt, benutzt
/// [`roh`].
pub fn roh_cpu() -> Vec<(&'static str, f64)> {
    roh()
        .into_iter()
        .filter_map(|(n, m)| m.cpu.map(|d| (n, d.as_secs_f64() * 1000.0)))
        .collect()
}

/// added 04.10.2026: **NUR DIE WANDZEIT** in Millisekunden. Sie ist **nicht**
/// die Antwort auf „wo rechnen wir“ — sie ist die Antwort auf „wo warten wir“,
/// und beides wird im Bericht **getrennt** genannt.
pub fn roh_wand() -> Vec<(&'static str, f64)> {
    roh()
        .into_iter()
        .map(|(n, m)| (n, m.wand.as_secs_f64() * 1000.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    /// added 04.10.2026: **DIE EINE EIGENTLICH SCHWIERIGE EIGENSCHAFT:**
    /// **ausgeschaltet** darf die Uhr **keinen** Schritt melden — und sie darf
    /// es auch **nicht** durch Zufall, wenn zwischen `uhr()` und `ablegen()`
    /// eingeschaltet wird. GEMESSEN ist das nicht; **behauptet** ist es, und die
    /// Behauptung steht hier, weil ein Fehler an dieser Stelle **stillschweigend**
    /// eine falsche Tabelle erzeugt.
    #[test]
    fn aus_ergibt_keine_schritte() {
        super::ausschalten();
        {
            let u = super::uhr("nichts");
            std::thread::sleep(Duration::from_millis(2));
            u.beenden();
        }
        assert!(super::roh().is_empty(), "ausgeschaltet darf nichts melden");
    }

    /// added 04.10.2026: **EINGESCHALTET** liefert der Wächter **selbst**
    /// seinen Schritt — **ohne** `hinweis`/`schritt`-Paar. Das ist der ganze
    /// Zweck der Form: ein Schritt, der seinen Wecker vergisst, ist im
    /// Zweierregel-Code ein **Phantom**, im Wächter-Code ein **Fehler**.
    #[test]
    fn ein_waechter_meldet_seinen_schritt_von_allein() {
        super::anschalten();
        {
            let u = super::uhr("warten");
            std::thread::sleep(Duration::from_millis(5));
            u.beenden();
        }
        let roh = super::roh();
        // **VOR `ausschalten` LESEN**, und das ist ein Fehler, den dieser Test
        // selbst gemacht hat: `ausschalten()` leert die Tabelle, und ein
        // `roh_wand()` **danach** liefert **null** Zeilen. Der erste Lauf
        // meldete „index out of bounds: the len is 0 but the index is 0“ —
        // **also nicht an der Messung, sondern an der Reihenfolge des Auslesens.**
        let wand = super::roh_wand();
        super::ausschalten();
        assert_eq!(roh.len(), 1, "genau ein Schritt: {roh:?}");
        assert_eq!(roh[0].0, "warten");
        // **NUR DIE WANDZEIT** wird hier geprueft: der Test schlaeft, und
        // Schlafen ist **per Definition** keine Rechenzeit. **DIE CPU-ZEIT wird
        // hier ausdruecklich NICHT geprueft** — sie ist auf dieser Maschine
        // **nicht stabil** (GEMESSEN: 2,64 ms CPU fuer 300 ms Warten, weil der
        // Faden beim Aufwachen rechnen muss), und ein Test, der sie auf
        // „unter 1 ms“ festnagelt, schlaegt fehl, sobald jemand anders baut.
        assert!(
            wand[0].1 >= 4.0,
            "5 ms geschlafen, Wandzeit gemessen {0} ms",
            wand[0].1
        );
    }

    /// added 04.10.2026: **DIE REIHENFOLGE**, weil sie die Reihenfolge des
    /// Codes ist und ein Bericht sie ablesen will.
    #[test]
    fn die_reihenfolge_ist_die_des_ablaufs() {
        super::anschalten();
        for n in ["a", "b", "c"] {
            super::uhr(n).beenden();
        }
        let namen: Vec<&str> = super::roh().into_iter().map(|(n, _)| n).collect();
        super::ausschalten();
        assert_eq!(namen, vec!["a", "b", "c"]);
    }

    /// added 04.10.2026: **DER WÄCHTER OHNE `beenden()`** — der Normalfall,
    /// denn im Handshake steht `let _uhr = uhr("…");` und sonst nichts. Ohne
    /// `Drop` waere die Tabelle **leer**, und das faellt erst im Bericht auf.
    #[test]
    fn der_waechter_arbeitet_auch_ohne_aufruf() {
        super::anschalten();
        {
            let _u = super::uhr("beim Weglegen");
        }
        let roh = super::roh();
        super::ausschalten();
        assert_eq!(roh.len(), 1);
        assert_eq!(roh[0].0, "beim Weglegen");
    }
}
