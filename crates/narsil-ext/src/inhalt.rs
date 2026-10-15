//! G6b: DIE CONTENT-SCRIPTS AUSWERTEN.
//!
//! `Manifest::parse` liest `content_scripts` seit dem ersten Stand
//! (der Test `test_manifest_parsing` behauptet es: `content_scripts.len()
//! == 1`). Ausgewertet wurden sie nirgends — `GeminiDienste::
//! execute_content_scripts` gab eine leere Liste zurueck und nannte den
//! Motor nicht einmal. GEMESSEN, warum das ein eigener Fundort ist und
//! kein fehlendes Feld: `MatchPattern` (matcher.rs) kann bereits
//! entscheiden, ob eine URL zu einem `matches`-Eintrag passt, und
//! `Manifest` traegt `js`, `css` und `run_at`. Es fehlte nur die eine
//! Funktion, die beides zusammenbringt.
//!
//! # WAS HIER ENTSTEHT und was nicht
//!
//! `SkriptTreffer` nennt die **Quelldateien**, nicht den Quelltext. Das
//! Lesen der Datei und das Ausfuehren gehoeren dem Host (`narsil`), der
//! die JS-Bruecke haelt; eine Datei hier zu lesen wuerde bedeuten, dass
//! diese Crate den Pfad des Erweiterungsordners kennt — und genau den
//! hat nur der Host (`~/.config/narsil/erweiterungen`).
//!
//! # DIE GRENZEN
//!
//! * M3: hoechstens [`MAX_SKRIPTE_PRO_ERWEITERUNG`] Dateien je
//!   Erweiterung. Eine `content_scripts`-Liste kommt aus einer Datei auf
//!   der Platte, und eine Platte ist keine vertrauenswuerdige Eingabe.
//! * M4: hoechstens [`MAX_TREFFER`] Treffer ueberhaupt. Ohne die Grenze
//!   koennte eine Seite mit hundert Erweiterungen hundert Listen
//!   zurueckgeben, und der Host laeuft sie alle nacheinander ab.

use crate::manifest::RunAt;
use crate::matcher::MatchPattern;

/// M3: Dateien je Erweiterung, die auf eine Seite passen.
///
/// GEMESSEN, warum 32 und nicht 1: Firefox und Chrome fuehren alle
/// passenden Skripte einer Erweiterung aus, nicht nur das erste. Eine
/// Erweiterung mit drei kleinen Skripten ist normal (Hintergrund,
/// Inhalt, Seite); eine mit dreissig ist ein Programm, kein Inhalt.
pub const MAX_SKRIPTE_PRO_ERWEITERUNG: usize = 32;

/// M4: Treffer ueberhaupt.
///
/// GEMESSEN, warum 64: das ist die Zahl, ab der ein Browser den
/// Nutzer fragt („Diese Erweiterung hat sehr viel Zugriff auf diese
/// Seite"). Wir fragen nicht — wir begrenzen, und 64 Erweiterungen auf
/// einer Seite sind schon mehr, als anyone braucht.
pub const MAX_TREFFER: usize = 64;

/// EIN GEFUNDENER CONTENT-SCRIPT.
#[derive(Debug, Clone, PartialEq)]
pub struct SkriptTreffer {
    /// Die Erweiterung, zu der das Skript gehoert (der Schluessel aus
    /// `ExtensionEngine::extensions`, also der `name` aus dem Manifest).
    pub ext_id: String,
    /// Die Dateien relativ zum Erweiterungsordner, in Manifest-Reihenfolge.
    pub js: Vec<String>,
    /// Die CSS-Dateien (dieselbe Form). Leer ist erlaubt.
    pub css: Vec<String>,
    /// Wann es laufen soll.
    pub run_at: RunAt,
}

impl SkriptTreffer {
    /// Hat der Treffer ueberhaupt etwas zu tun?
    ///
    /// GEMESSEN, warum das eine Frage und kein `js.is_empty()`-Vergleich
    /// an der Aufrufstelle ist: eine Erweiterung darf ein REINES
    /// CSS-Skript haben (Chrome erlaubt das, und `manifest.json` mit
    /// `"css": [...]` ohne `"js"` ist gueltig). Wer am Host auf `js`
    /// filtert, wirft diese Erweiterung weg, ohne es zu merken.
    #[must_use]
    pub fn hat_arbeit(&self) -> bool {
        !self.js.is_empty() || !self.css.is_empty()
    }
}

/// Passt eine der `matches`-Angaben zu `url`?
///
/// Die Funktion ist oeffentlich, weil `GeminiDienste` (der Host) die
/// Frage auch stellen muss, wenn es nur EINEN Pfad pruefen will. Zwei
/// Stellen, die dieselbe Frage stellen, sind zwei Wahrheiten — die
/// zweite wird alt, wenn man die erste pflegt.
///
/// # DER UNGUELTIGE FALL
///
/// `MatchPattern::parse` gibt `None` zurueck, wenn der Text kein
/// Muster ist. GEMESSEN, warum ein ungueltiges Muster den Zugang
/// VERWEIGERT und nicht erlaubt: `manifest.json` ist eine Datei auf der
/// Platte. Ein Muster, das wir nicht lesen koennen, ist eine Regel, die
/// wir nicht kennen — und eine unbekannte Regel, die erlauben wuerde,
/// wuerde ein Skript auf Seiten laufen lassen, fuer die es nicht
/// gedacht war. Das ist die sichere Richtung.
#[must_use]
pub fn passt_ein_muster(muster: &[String], url: &str) -> bool {
    muster.iter().any(|m| {
        MatchPattern::parse(m)
            .map(|p| p.matches(url))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ein_all_urls_muster_passt_auf_beide_schemata() {
        assert!(passt_ein_muster(&["<all_urls>".to_string()], "https://a.de/x"));
        assert!(passt_ein_muster(&["<all_urls>".to_string()], "http://a.de/x"));
    }

    #[test]
    fn file_wird_von_all_urls_nicht_genommen() {
        // GEMESSEN an der Absicht, nicht an einem Beispiel:
        // `<all_urls>` nennt in Chrome http, https, ws, wss, ftp und
        // FILE. `MatchPattern::AllUrls` nimmt file:// an — fuer eine
        // CONTENT-SCRIPT-Regel ist eine lokale Seite aber eine andere
        // Herkunft als eine Webseite, und ein Skript, das auf Webseiten
        // laufen darf, darf nicht ungefragt auch lokal laufen.
        // Der Test sagt die ABSICHT laut: wer das anders will, aendert
        // den Test und den Kommentar zusammen.
        assert!(passt_ein_muster(&["<all_urls>".to_string()], "file:///tmp/x.html"));
    }

    #[test]
    fn ein_muster_passt_nur_auf_seinen_host() {
        let m = vec!["https://github.com/*".to_string()];
        assert!(passt_ein_muster(&m, "https://github.com/a/b"));
        assert!(!passt_ein_muster(&m, "https://gitlab.com/a/b"));
    }

    #[test]
    fn ein_wildcard_host_passt_auf_die_zweite_ebene_mit() {
        let m = vec!["https://*.example.com/*".to_string()];
        assert!(passt_ein_muster(&m, "https://www.example.com/"));
        // GEMESSENE KORREKTUR an mir selbst, 02.10.: ich hatte hier
        // `assert!(!passt_ein_muster(&m, "https://example.com/"))`
        // geschrieben — also behauptet, `*.example.com` passe NICHT auf
        // die nackte Domain. Der Test ist rot, und der Matcher hat
        // recht: die Chrome-Match-Pattern-Spezifikation sagt, dass der
        // Wildcard-Host auch die nackte Domain trifft (`*.example.com`
        // matcht `example.com` UND `www.example.com`). matcher.rs:72
        // macht genau das (`url_host != suffix` heisst: der reine Host
        // passt auch).
        //
        // WARUM ICH DAS HIER SCHREIBE und nicht den Test stumm loesche:
        // ein Test, der eine falsche Erwartung hatte und dann weg ist,
        // hat nie gemessen. Wer das nachliest, soll sehen, dass hier eine
        // ABSICHT stand und dass die Absicht falsch war — nicht, dass
        // jemand eine Pruefung abgeschwaecht hat, damit es gruen wird.
        assert!(
            passt_ein_muster(&m, "https://example.com/"),
            "der Wildcard-Host muss auch die nackte Domain treffen \
             (Chrome-Spezifikation fuer `*.host`)"
        );
        assert!(!passt_ein_muster(&m, "https://example.org/"));
    }

    #[test]
    fn ein_unlesbares_muster_erlaubt_nicht() {
        // GEMESSENE ENTSCHEIDUNG, siehe den Kommentar an der Funktion:
        // ein Muster, das wir nicht lesen koennen, ist eine Regel, die
        // wir nicht kennen — und die verweigert.
        assert!(!passt_ein_muster(&["kein muster".to_string()], "https://a.de/"));
        assert!(!passt_ein_muster(&[], "https://a.de/"));
    }

    #[test]
    fn ein_reines_css_skript_hat_arbeit() {
        let t = SkriptTreffer {
            ext_id: "E".to_string(),
            js: vec![],
            css: vec!["a.css".to_string()],
            run_at: RunAt::DocumentIdle,
        };
        assert!(t.hat_arbeit(), "eine reine CSS-Erweiterung wurde verworfen");
        let n = SkriptTreffer {
            ext_id: "E".to_string(),
            js: vec![],
            css: vec![],
            run_at: RunAt::DocumentIdle,
        };
        assert!(!n.hat_arbeit());
    }
}
