//! Die **Spur** — ein Ringpuffer der letzten Trace-Zeilen, unabhaengig davon,
//! ob jemand sie ausgegeben hat.
//!
//! DER ANLASS IST EIN GEMESSENER FEHLER (N46). `cloudflare_quic_laden` — der
//! wichtigste Test der Rolle — fiel **einmal von 15** Laeufen aus:
//!
//! ```text
//! Abruf fehlgeschlagen: Zeitueberschreitung: keine Antwort nach dem
//! Idle-Zeitlimit
//! ```
//!
//! Und **genau fuer diesen Lauf** gab es keine Spur, weil die Spur nur bei
//! `NARSIL_QUIC_TRACE=1` geschrieben wird und der normale Testlauf sie nicht
//! einschaltet. Ein Fehler, der nur manchmal auftritt, ist beim zweiten
//! Auftreten **wiederlost**, wenn man ihn nicht sichert.
//!
//! DIE LOESUNG, und sie ist bewusst klein: die Spur wird **immer** in einen
//! Ringpuffer geschrieben, und wer sie braucht, holt sie sich danach. Zwei
//! Folgen hat das, beide positiv:
//!
//!  1. Ein Fehler ist ab jetzt **immer** nachvollziehbar — der Aufrufer ruft
//!     `spur_letzte(n)` und druckt sie.
//!  2. `trace()` liest die Umgebungsvariable **nicht** mehr bei jedem Aufruf.
//!     `std::env::var_os` war der teuerste Teil jeder Trace-Zeile — bei 1 169
//!     Paketen mit mehreren Zeilen je Paket ist das ein spuerbarer Posten.
//!
//! M7: Der Puffer ist **begrenzt**, und die Grenze heisst `MAX_SPURZEILEN`.
//! Eine Spur, die nichts begrenzt, ist ein Speicherleck mit Umweg.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// Wie viele Zeilen die Spur haelt (M7).
///
/// DER WERT: ein Abruf von 1,3 MB schreibt ueber 5 000 Zeilen (Empfang,
/// Entschluesseln, Rahmen, Senden). 4 000 Zeilen sind damit der komplette
/// groesste Abruf, und der Speicher bleibt bei rund 0,5 MiB - auch im
/// schlimmsten Fall.
pub const MAX_SPURZEILEN: usize = 4096;

/// Der Zaehler der nicht behaltenen Zeilen (N46).
static VERWORFEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Zeigt der Trace auf der Konsole? Einmal gelesen, dann gemerkt.
fn trace_an() -> bool {
    static AN: OnceLock<bool> = OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("NARSIL_QUIC_TRACE").is_some())
}

/// Der Ringpuffer. Ein `Mutex` ist hier richtig und nicht uebertrieben: es
/// schreiben nur die Client-Pfade, und der Zeitraum ist kurz.
fn spur() -> &'static Mutex<VecDeque<String>> {
    static S: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// Eine Zeile in die Spur schreiben — **immer**, nicht nur bei aktivem Trace.
///
/// DER UNTERSCHIED ZUR ALTEN FASSUNG, und er ist der ganze Punkt: vorher
/// gab es diese Funktion gar nicht; `trace()` pruefte die Umgebung und
/// sonst nichts. Jetzt wird die Zeile **immer** behalten, und nur die
/// **Ausgabe** haengt an der Umgebung.
pub fn spur_zeile(phase: &str, text: &str) {
    let zeile = format!("[quic {phase}] {text}");
    if trace_an() {
        eprintln!("{zeile}");
    }
    // M7: Der Puffer ist begrenzt; ein vollgeschriebener gibt die aelteste
    // Zeile ab. `lock()`-Fehler werden **nicht** weitergegeben: die Spur ist
    // eine Diagnose, und ein Diagnosefehler darf keinen Abruf zerlegen.
    //
    // DER ZAEHLER ist nicht Kosmetik: `tests/n46_spur.rs` erwartete am Ende
    // `MAX_SPURZEILEN` Zeilen und fand 3 732. Entweder hat die Sperre nicht
    // gehalten (dann waeren Zeilen ueberschrieben worden) oder `lock()`
    // schlug fehl und die Zeilen sind stillschweigend weg (dann ist es ein
    // vergiftetes Mutex). Der Zaehler sagt es einem, statt dass man es rät.
    if let Ok(mut s) = spur().lock() {
        if s.len() >= MAX_SPURZEILEN {
            s.pop_front();
            VERWORFEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        s.push_back(zeile);
    } else {
        VERWORFEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Wie viele Zeilen der Ring **nicht** behalten hat (N46, M7).
///
/// ZWEI GRUENDE, und beide zaehlt dieser Zaehler zusammen:
///  * der Ring war voll — dann ist es der normale Fall, und die Zahl sagt,
///    wie weit man zurueckblicken kann;
///  * die Sperre schlug fehl (vergiftetes Mutex) — dann ist es ein **Fehler**,
///    und die Spur ist unzuverlaessig.
pub fn spur_verworfen() -> u64 {
    VERWORFEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Die letzten `n` Zeilen, **aelteste zuerst**.
///
/// DER GRUND FUER DIE REIHENFOLGE: eine Spur, die verkehrt herum laeuft, liest
/// sich wie ein rueckwaerts laufender Abruf. `VecDeque` gibt sie von vorn.
pub fn spur_letzte(n: usize) -> Vec<String> {
    spur()
        .lock()
        .map(|s| s.iter().rev().take(n).rev().cloned().collect())
        .unwrap_or_default()
}

/// Die Spur als eine Zeile mit Zeilenumbruechen — zum direkten Ausgeben.
pub fn spur_text(n: usize) -> String {
    spur_letzte(n).join("\n")
}

/// Wie viele Zeilen die Spur gerade haelt.
pub fn spur_anzahl() -> usize {
    spur().lock().map(|s| s.len()).unwrap_or(0)
}

/// Die Spur **leeren**.
///
/// WOFUER: wenn ein Test mehrere Abrufe macht, will jeder seine eigene Spur
/// sehen. Ohne das leeren sieht man am Ende nur die letzten `n` Zeilen des
/// **letzten** Abrufs — und bei einem Fehlschlag im ersten sieht man
/// ueberhaupt nichts.
pub fn spur_leeren() {
    if let Ok(mut s) = spur().lock() {
        s.clear();
    }
    VERWORFEN.store(0, std::sync::atomic::Ordering::Relaxed);
}
