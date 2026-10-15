//! DAS ABNAHMEBILD AN ZWEI ORTEN — eine Regel, eine Stelle.
//!
//! # DER BEFUND, DER DIESE DATEI AUSGELOEST HAT (03.10.2026, 3:0x)
//!
//! `werkzeuge/platz_halten.sh` raeumt Platz, und Z. 39 verschiebt dabei
//! jede Datei `*.png` und `*.txt` unter `team/log` nach 180 Minuten auf
//! die externe Platte:
//!
//! ```text
//! find team/log -maxdepth 1 -type f \( -name '*.png' -o -name '*.txt' \)
//!      -mmin +180 -exec mv -t "$ZIEL/team_log" {} +
//! ```
//!
//! GEMESSEN (03.10., 3:0x): die Zeile nennt genau ZWEI Wurzeln —
//! `team/log` (Z. 39) und `probe/` (Z. 40). `team/bilder` kommt im
//! ganzen Skript NULL Mal vor (`grep -c team/bilder` = 0). Und die
//! Abnahmebilder lagen zu sieben Stueck genau in `team/log`
//! (`devtools_D1.png` … `devtools_D7.png`).
//!
//! GEMESSENE FOLGE: die Koordination verlangt den Beleg als Bild
//! (`team/log/devtools_*.png`, `team/log/ui_vorlage_*.png`) — und nach
//! drei Stunden waere er weg, ohne dass jemand etwas bemerkt hat. Die
//! Beobachtung steht in `team/PROBLEME.txt` (23:49) gegen dieselbe
//! Regel an anderer Stelle: nicht der Datenverlust ist das Problem,
//! sondern dass niemand es bemerkt hat.
//!
//! # DIE ENTSCHEIDUNG
//!
//! GEMESSENE ENTSCHEIDUNG, warum BEIDE Orte beschrieben werden und
//! nicht einer: `team/log` ist der Ort, an dem die Koordination sucht
//! (und dort bleibt die Abnahme, solange der Lauf frisch ist);
//! `team/bilder` ist der einzige Ordner im Baum, den der
//! Auslagerungsbefehl nicht beruehrt — dort ueberlebt sie den Sweep.
//!
//! GEMESSENE ENTSCHEIDUNG, warum das eine MODUL und nicht eine Zeile
//! in jedem Beispiel: fuenf Beispiele (`d3`, `d6`, `d7`, `d9`, `d11`)
//! schrieben bisher auf eigene Faust an ihre eigene Pfadzeile. Eine
//! Regel, die an fuenf Stellen steht, wird an vier Stellen falsch —
//! die Abnahme der UI-VORLAGE hat genau das gezeigt: dort stand die
//! Regel schon an zwei Stellen, und eine davon fehlte.
//!
//! # M9 (kein /tmp)
//!
//! Der Pfad liegt im Baum des Projekts, nicht in `/tmp` (tmpfs = RAM).
//! GEMESSENE ENTSCHEIDUNG, warum das Schreiben kein zweites Bild im
//! Speicher braucht: `schreiben` nimmt die fertigen PNG-Byte und
//! schreibt DIESELBEN Bytes zweimal — es entsteht keine Kopie, und die
//! Spitze bleibt die des Rasterers (GEMESSEN: 957 Items, 1920x1080).

use std::path::PathBuf;

/// Der Projektordner (`Browser`), aus dem Crate-Pfad bestimmt.
///
/// REPARIERT 05.10.2026: Vorher standen hier die **festen** Linux-Pfade
/// `/home/jaff/Bilder/Browser/team/log` und `…/team/bilder`. Unter Windows
/// ist `/home/…` kein Projektpfad, sondern der laufwerksrelative Pfad
/// `C:\home\jaff\…` — `--bild` legte sein Bild also **neben** dem Projekt
/// an, auf jedem anderen Rechner an einer Stelle, die niemand sucht.
/// `CARGO_MANIFEST_DIR` ist der Pfad des Crates (`crates/narsil`), zwei
/// Ebenen darueber liegt der Projektordner. Damit ist der Ort auf jeder
/// Maschine richtig, und die Begruendung unten (kein relatives `team/log`
/// neben dem Binary) bleibt erfuellt: der Pfad ist **absolut**.
fn projekt_wurzel() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// DIE ZWEI ORDNER, in die jedes Abnahmebild geht.
///
/// GEMESSENE ENTSCHEIDUNG, warum absolute Pfade und keine
/// Verzeichnisnamen: die Beispiele werden aus `target/debug/examples`
/// aufgerufen, also aus einem anderen Arbeitsverzeichnis als das
/// Projekt. Ein relatives `team/log` haette neben dem Binary gelandet.
pub fn ordner() -> [PathBuf; 2] {
    let wurzel = projekt_wurzel();
    [wurzel.join("team").join("log"), wurzel.join("team").join("bilder")]
}

/// Schreibt die Bytes an BEIDE Orte und nennt die geschriebenen Pfade.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Liste zurueckkommt und nicht nur
/// `()`: ein Beispiel, das den Pfad nicht kennt, kann ihn auch nicht
/// ausgeben — und ein Bild, dessen Pfad niemand nennt, ist ein Bild,
/// dessen Alter niemand prueft.
pub fn schreiben(name: &str, bytes: &[u8]) -> std::io::Result<Vec<PathBuf>> {
    let ordner = ordner();
    let mut pfade = Vec::with_capacity(ordner.len());
    for ordner in ordner {
        std::fs::create_dir_all(&ordner)?;
        let pfad = ordner.join(name);
        std::fs::write(&pfad, bytes)?;
        pfade.push(pfad);
    }
    Ok(pfade)
}

/// Schreibt die Bytes und sagt (fuer `eprintln!`) beide Pfade.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge nicht wechselt: das
/// erste Ziel ist der Ort, an dem die Koordination sucht.
pub fn pfade_text(pfade: &[PathBuf]) -> String {
    let mut text = String::new();
    for (i, p) in pfade.iter().enumerate() {
        if i == 0 {
            text.push_str(&p.display().to_string());
        } else {
            text.push_str(" | auch ");
            text.push_str(&p.display().to_string());
        }
    }
    text
}
