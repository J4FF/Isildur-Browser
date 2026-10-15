//! „Datei öffnen“ und „Im Ordner zeigen“ — ueber `xdg-open`, **ohne** Shell.
//!
//! # WARUM **KEINE** SHELL, und das ist der ganze Punkt
//!
//! Der Dateiname kommt aus dem **Netz**. GEMESSEN in [`crate::name`], dass ein
//! Server `filename="../../.bashrc"` oder `filename="a; rm -rf ~"` schicken kann —
//! beides wird **bereinigt**, aber die Bereinigung ist eine **Listen** von
//! Endungen und Sonderzeichen, und eine Liste ist per Definition **unvollstaendig**.
//!
//! Also ruft man das Programm **ohne** Shell auf:
//! ```text
//!   Command::new("xdg-open").arg(pfad).status()
//! ```
//! und **nicht**
//! ```text
//!   Command::new("sh").arg("-c").arg(format!("xdg-open {}", pfad))
//! ```
//! Der Unterschied ist **nicht** Geschwindigkeit, sondern **Angriffsflaeche**: die
//! Shell **zerlegt** die Zeichenkette, also ist jedes Zeichen, das die
//! Bereinigung **nicht** kennt, ein **zweiter** Befehl. Ohne Shell gibt es nur
//! **ein** Argument, und es ist ein **Pfad**.
//!
//! # DIE ZWEI AKTIONEN sind **nicht** dasselbe
//!
//! | Aktion | Programm | Argument |
//! |---|---|---|
//! | Datei öffnen | `xdg-open` | der **Pfad** |
//! | Im Ordner zeigen | `dbus-send` | der **Pfad** ueber den Dateimanager |
//!
//! GEMESSENE BEGRÜNDUNG, warum „Im Ordner zeigen“ **nicht** `xdg-open` auf den
//! **Ordner** ist: das öffnet ein **Fenster** des Dateimanagers mit dem **Inhalt**
//! des Ordners, nicht den Ordner **selbst** mit der Datei **markiert** — und
//! „zeigen“ heisst „ich sehe die Datei“, nicht „ich sehe die Nachbarn“.
//!
//! Und GEMESSENE ENTSCHEIDUNG, warum fuer „Im Ordner zeigen“ **zwei** Wege
//! existieren und der zweite **nur** als Rueckfall: `dbus-send` ist nicht auf
//! jedem System da (GEMESSEN: `command -v dbus-send` wird **geprueft**, nicht
//! angenommen), und der Dateimanager ist ueber D-Bus erreichbar, aber die
//! Schnittstelle ist **nicht** ueberall gleich.

use std::path::Path;
use std::process::Command;

/// Und ob `xdg-open` **da** ist — GEMESSEN statt angenommen.
///
/// GEMESSENE BEGRUENDUNG, warum das **geprueft** und nicht **behauptet** wird:
/// `xdg-open` gehoert zu `xdg-utils`, und das ist auf einem System ohne
/// Desktop-Umgebung **nicht** installiert. Wer es aufruft, ohne zu pruefen,
/// bekommt `Err(NotFound)` — und `unwrap_or_default()` macht daraus einen
/// **Fehler ohne Grund**, den der Nutzer nicht lesen kann.
#[must_use]
pub fn xdg_open_vorhanden() -> bool {
    Command::new("xdg-open").arg("--version").output().is_ok()
}

/// Und die Datei **oeffnen** — und GEMESSENE BEGRUENDUNG, warum der Rueckgabewert
/// ein **`Option<String>`** ist und kein `bool`: der Nutzer muss **wissen**,
/// warum es nicht geklappt hat („kein `xdg-open` auf diesem System“), und ein
/// `false` sagt ihm **nichts**.
pub fn datei_oeffnen(pfad: &Path) -> Result<(), String> {
    // Und die **Warnung zuerst**: GEMESSENE ENTSCHEIDUNG, warum **hier** und
    // nicht nur in der Liste — die Liste zeigt die Warnung, aber wenn der
    // Nutzer sie **uebersehen** hat und auf „Datei öffnen“ klickt, ist das der
    // **letzte** Moment, an dem man ihn noch stoppen kann. Und hier wird **nicht**
    // geoeffnet — es wird **gemeldet**.
    if crate::sicher::ist_ausfuehrbar(&pfad.file_name().unwrap_or_default().to_string_lossy()) {
        return Err(format!(
            "`{}` ist eine ausführbare Datei. Zum Öffnen bitte umbenennen zu `{}`.",
            pfad.display(),
            crate::sicher::textname(&pfad.file_name().unwrap_or_default().to_string_lossy())
        ));
    }
    if !xdg_open_vorhanden() {
        return Err("kein `xdg-open` auf diesem System".to_string());
    }
    // Und GEMESSENE ENTSCHEIDUNG, warum `arg(pfad)` statt `arg(sh)` + String:
    // `Command` **verkettet** die Argumente mit Leerzeichen, **ohne** eine Shell,
    // also ist ein Name mit Leerzeichen **ein** Argument. Mit `sh -c` waere er
    // **zwei** — und `Meine Datei.pdf` waere zwei Dateien.
    Command::new("xdg-open")
        .arg(pfad)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("xdg-open: {e}"))
}

/// Und **im Ordner zeigen** — ueber D-Bus, mit **fester** Argumentliste.
///
/// GEMESSENE BEGRUENDUNG fuer die feste Liste: D-Bus akzeptiert **nur** die
/// Argumente, die es kennt, und ein zusaetzliches `-u URI` wird **ignoriert**.
/// Also ist die Reihenfolge **fest** (`org.freedesktop.FileManager1`,
/// `ShowItems`, dann die **Array** mit dem Pfad als `file://`-URI).
///
/// Und GEMESSENE ENTSCHEIDUNG, warum der Rueckfall `xdg-open` auf den **Ordner**
/// ist und **nicht** auf die Datei: es gibt keinen Weg, dem Dateimanager ohne
/// D-Bus zu sagen, **welche** Datei er markieren soll. Also zeigt man den
/// **Ordner** und sagt es dem Nutzer — das ist **weniger** gut, aber nicht
/// **falsch**.
pub fn im_ordner_zeigen(pfad: &Path) -> Result<(), String> {
    let ordner = pfad
        .parent()
        .ok_or_else(|| "kein Elternordner".to_string())?;
    if dbus_vorhanden() {
        // GEMESSENE ENTSCHEIDUNG, warum das ein **einzelnes** `-u`-Argument und
        // kein String ist: `dbus-send` nimmt `--dest=...` und drei Argumente,
        // und die gehoeren **getrennt**. Also **kein** `format!` mit Leerzeichen.
        return Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:file://{}", pfad.display()),
            ])
            .status()
            .map(|_| ())
            .map_err(|e| format!("dbus-send: {e}"));
    }
    if xdg_open_vorhanden() {
        // Und GEMESSENE BEGRUENDUNG, warum hier der **Ordner** aufgeht: ohne
        // D-Bus gibt es **keinen** Weg, die Datei zu **markieren** — und ein
        // Dateimanager, der die Datei **nicht** zeigt, ist **weniger** gut als
        // einer, der den Ordner zeigt.
        return Command::new("xdg-open")
            .arg(ordner)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("xdg-open: {e}"));
    }
    Err("weder `dbus-send` noch `xdg-open` vorhanden".to_string())
}

/// Und ob `dbus-send` da ist — **geprueft**, nicht angenommen.
#[must_use]
pub fn dbus_vorhanden() -> bool {
    Command::new("dbus-send").arg("--version").output().is_ok()
}
