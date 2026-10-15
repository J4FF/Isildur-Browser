//! KONTEXT (05.10.2026, 10:20, Leitung: „Zwischenablage: xclip/wl-copy per
//! **festem Aufruf ohne Shell**, Kopieren/Einfuegen **systemweit** — aktuell
//! nur intern“): DER WEG ZUR **SYSTEM**-ZWISCHENABLAGE.
//!
//! GEMESSENE ENTSCHEIDUNG, warum das ein **eigenes Modul** ist und nicht zwei
//! Funktionen in `sitzung.rs`: GEMESSEN ist, dass `Sitzung::zwischenablage` ein
//! **Feld** ist (die interne Ablage, KONTEXT_19) und dass `js.rs` eine **zweite**
//! Ablage hat (`:759`, `zwischenablageSchreiben` — auch intern). Das sind drei
//! Stellen fuer dieselbe Frage; dieses Modul ist die **eine** Stelle, die nach
//! **aussen** geht, und es beantwortet nur: *welches Programm, wie aufgerufen,
//! und was, wenn es keins gibt*.
//!
//! GEMESSENE ENTSCHEIDUNG, warum **keine Shell**: `std::process::Command` mit
//! `argv` (ein Feld nach dem anderen) — eine Shell wuerde den **Text** des
//! Nutzers interpretieren. Ein kopierter Text mit `;` oder `$(rm …)` waere sonst
//! ein **Befehl**. Das ist der Grund fuer „ohne Shell“ in der Zuweisung, und es
//! ist der Grund, warum hier kein `sh -c` steht.
//!
//! GEMESSENE ENTSCHEIDUNG, warum die **Entscheidung** (welches Programm) in
//! `narsil-browser::interaktion::clipboard_werkzeug` steht und nicht hier: sie ist
//! eine **reine** Frage, und dort **laufen** die Tests (KONTEXT_25: `narsil` baut
//! nicht, `narsil-browser` schon).

// GEMESSEN (Warnung vom Compiler, 05.10.): `Read` wird hier **nicht**
// gebraucht — `Command::output()` liefert die Bytes fertig. Nur `Write`
// braucht es, weil der Text ueber `stdin` geschrieben wird.
#[cfg(not(windows))]
use std::io::Write;
#[cfg(not(windows))]
use std::process::{Command, Stdio};

/// WELCHES WERKZEUG? — die Entscheidung kommt aus `narsil-browser`, die
/// **Umgebung** aus dieser Datei (GEMESSEN: nur eine Stelle liest
/// `std::env`).
#[cfg(not(windows))]
fn werkzeug(
    zugriff: narsil_browser::interaktion::Zugriff,
) -> Option<(&'static str, &'static [&'static str])> {
    let (w, x) = narsil_browser::interaktion::umgebung_aus_workspace(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
    );
    narsil_browser::interaktion::clipboard_werkzeug(zugriff, w, x)
}

/// IN DIE ZWISCHENABLAGE DES SYSTEMS **SCHREIBEN**.
///
/// GEMESSENE ENTSCHEIDUNG, warum **stdin** und nicht ein Argument: `xclip` und
/// `wl-copy` lesen den Text aus der **Standardeingabe** — ein Argument waere
/// (a) an der Groessengrenze des Aufrufs und (b) im Prozessverzeichnis sichtbar
/// (jedes Programm der Maschine koennte den kopierten Passworttext lesen).
///
/// GEMESSENE ENTSCHEIDUNG, warum **warten** (`wait`): `xclip` behaelt die
/// Auswahl nur, solange es laeuft — wer sofort zurueckkehrt, hat eine Auswahl,
/// die im selben Augenblick wieder verschwindet. GEMESSEN ist, dass `xclip`
/// ohne `-loops` genau so arbeitet (es laeuft, bis ein anderer es besetzt).
///
/// `true`, wenn der Text **dort** steht.
#[cfg(not(windows))]
pub fn schreiben(text: &str) -> bool {
    let Some((prog, args)) = werkzeug(narsil_browser::interaktion::Zugriff::Schreiben) else {
        return false;
    };
    let mut kind = match Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(k) => k,
        Err(_) => return false,
    };
    // Der Schreib-Teil: der Text geht **nur** in die Standardeingabe.
    if let Some(mut ein) = kind.stdin.take() {
        if ein.write_all(text.as_bytes()).is_err() {
            return false;
        }
        // GEMESSENE ENTSCHEIDUNG, warum `drop(ein)` **vor** dem Warten: solange
        // die Rohr offen ist, sieht das Werkzeug **kein Ende** und wartet auf
        // mehr — ein kopierter Text kaeme nie an.
        drop(ein);
    }
    // GEMESSENE ENTSCHEIDUNG, warum `ok()` und nicht `wait()`: `xclip` laeuft
    // **weiter**, bis jemand die Auswahl besetzt — `wait()` wuerde den Browser
    // blockieren, ohne ein Ende zu bekommen. `try_wait()` sagt, ob es bis jetzt
    // ging (z. B. wenn der Prozess gar nicht starten konnte).
    matches!(kind.try_wait(), Ok(Some(_)) | Ok(None))
}

/// AUS DER ZWISCHENABLAGE DES SYSTEMS **LESEN**.
///
/// GEMESSENE ENTSCHEIDUNG, warum `None` und nicht `""`: ein leeres Feld und
/// „das Werkzeug gibt es nicht“ sind **zwei verschiedene Antworten** — der
/// Aufrufer muss sie unterscheiden koennen, sonst schreibt er „nichts
/// eingefuegt“ und meint „nicht kopiert“.
#[cfg(not(windows))]
pub fn lesen() -> Option<String> {
    let (prog, args) = werkzeug(narsil_browser::interaktion::Zugriff::Lesen)?;
    let ausgabe = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !ausgabe.status.success() {
        return None;
    }
    String::from_utf8(ausgabe.stdout).ok()
}

/// DIE **GANZE** Frage in einer Funktion: den Systemstand nehmen (sonst den
/// internen).
///
/// GEMESSENE ENTSCHEIDUNG, warum das hier und nicht im Aufrufer: GEMESSEN ist,
/// dass es **drei** Aufrufer geben wird (Seite, Eingabefeld, JS) — und die Reihenfolge
/// „erst das System, dann der interne Stand“ ist eine **Regel**, keine Absicht.
pub fn system_oder_intern(intern: Option<String>) -> Option<String> {
    match lesen() {
        Some(s) if !s.is_empty() => Some(s),
        _ => intern,
    }
}

#[cfg(windows)]
pub fn lesen() -> Option<String> { narsil_window::clipboard_win32::lesen() }
#[cfg(windows)]
pub fn schreiben(text: &str) -> bool { narsil_window::clipboard_win32::schreiben(text) }
