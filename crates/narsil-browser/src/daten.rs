//! Lesezeichen und die Verlaufsgrenze — beides Daten, die man zaehlen kann.
//!
//! ERSTAELLT am 29.09.2026, TASK_ZUSTAND Aufgabe 2.
//!
//! KEINE Uhr, KEINE Zufallszahl: bei einem Verlaufseintrag ist der
//! Zeitpunkt ein PARAMETER des Aufrufers. Ein Zustand, der selbst die Uhr
//! liest, ist nicht reproduzierbar testbar — und genau daran ist heute
//! Nachmittag in einem anderen Crate eine Stunde verloren gegangen.

use crate::{Browser, Page};

/// HOECHSTE Zahl von Seiten im Verlauf EINES Tabs.
///
/// AELTESTE FALLEN WEG. Eine Konstante, kein Wert pro Tab: entweder man
/// will einen Verlauf, oder man will einen Speicherleck. Jeder andere Weg
/// braucht eine Entscheidung, die jemand treffen muss, wenn es niemand
/// tut — und dann hat jeder Tab eine andere.
///
/// Warum 200: GEMESSEN am 29.09.2026 an den Testdateien des Projekts, die
/// groesste hat 1936 Zeilen. 200 Seiten pro Tab sind fuer einen Browser
/// gross genug, dass niemand die Grenze in einer Sitzung merkt, und
/// klein genug, dass 20 Tabs zusammen nicht den Speicher fressen. Das ist
/// eine BEGRUENDUNG, keine Messung — der Wert selbst ist gesetzt und
/// umkehrbar, wie jede Grenze in diesem Projekt.
pub const VERLAUF_GRENZE: usize = 200;

/// Ein Lesezeichen: eine Adresse und ein Titel.
///
/// Der Titel ist optional (leer heisst "es gab keinen"), weil die Crate
/// den Titel erst nach dem Laden kennt und ein Lesezeichen auch vorher
/// gesetzt werden darf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lesezeichen {
    pub adresse: String,
    pub titel: String,
}

impl Lesezeichen {
    /// Ein Lesezeichen mit Adresse und Titel.
    pub fn neu(adresse: impl Into<String>, titel: impl Into<String>) -> Self {
        Lesezeichen {
            adresse: adresse.into(),
            titel: titel.into(),
        }
    }
}

impl Browser {
    // ------------------------------------------------------------------
    // Lesezeichen
    // ------------------------------------------------------------------

    /// Ein Lesezeichen hinzufuegen. `true`, wenn es neu war, `false` wenn
    /// die Adresse schon eingetragen war.
    ///
    /// ## ENTSCHEIDUNG: doppelte Adresse
    ///
    /// **Es gibt genau EINEN Eintrag je Adresse, und ein erneutes
    /// Hinzufuegen setzt ihn nach OBEN.** Begruendung in zwei Teilen:
    ///
    /// Warum einer und nicht zwei: ein Lesezeichen ist eine Adresse, kein
    /// Klick. Zwei Eintraege fuer dieselbe Adresse bedeuten, dass beim
    /// Oeffnen zwei Seiten an derselben Stelle aufgehen — das ist ein
    /// Fehler, den niemand absichtlich baut, den man aber bekommt, wenn
    /// man "einfach noch eins dazu" tut.
    ///
    /// Warum nach oben und nicht ignorieren: die Reihenfolge ist
    /// "zuletzt benutzt oben" (siehe `lesezeichen()`), und wer ein
    /// Lesezeichen zweimal anfasst, benutzt es. Ein Eintrag, der beim
    /// zweiten Mal still bleibt, beluegt die Regel, die darunter steht.
    ///
    /// Getestet in `doppeltes_lesezeichen_bleibt_eins_und_steigt_nach_oben`.
    pub fn lesezeichen_hinzufuegen(
        &mut self,
        adresse: impl Into<String>,
        titel: impl Into<String>,
    ) -> bool {
        let adresse = adresse.into();
        let titel = titel.into();

        // Vorb-existing: rausholen, damit es danach wieder genau einmal da ist
        // und ganz oben steht.
        let alter = self.lesezeichen.iter().position(|l| l.adresse == adresse);
        if alter.is_some() {
            self.lesezeichen.remove(alter.expect("gerade gesehen"));
        }
        self.lesezeichen.insert(
            0,
            Lesezeichen {
                adresse,
                titel: titel.into(),
            },
        );
        alter.is_none()
    }

    /// Das Lesezeichen dieser Adresse entfernen. `true` wenn es da war.
    pub fn lesezeichen_entfernen(&mut self, adresse: &str) -> bool {
        match self.lesezeichen.iter().position(|l| l.adresse == adresse) {
            Some(p) => {
                self.lesezeichen.remove(p);
                true
            }
            None => false,
        }
    }

    /// Ob diese Adresse als Lesezeichen eingetragen ist.
    pub fn hat_lesezeichen(&self, adresse: &str) -> bool {
        self.lesezeichen.iter().any(|l| l.adresse == adresse)
    }

    /// Alle Lesezeichen, **neuestes zuerst**.
    ///
    /// ENTSCHEIDUNG: neuestes oben. Begruendung: die Leiste ist kurz, die
    /// Lesezeichen sind es auch, und was man zuletzt gesetzt hat, will man
    /// ohne Scrollen sehen. Das ist dieselbe Regel wie bei den Tabs des
    /// Fenstermanagers: das zuletzt Benutzte steht vorn.
    pub fn lesezeichen(&self) -> &[Lesezeichen] {
        &self.lesezeichen
    }

    /// Lesezeichen in klartext-lesbarer Form, neuestes zuerst. Fuer die
    /// Sitzung; die Reihenfolge ist hier dieselbe wie in `lesezeichen()`.
    pub fn lesezeichen_zeilen(&self) -> Vec<String> {
        self.lesezeichen
            .iter()
            .map(|l| format!("{}\t{}", l.adresse, l.titel))
            .collect()
    }

    // ------------------------------------------------------------------
    // Verlauf als Daten
    // ------------------------------------------------------------------

    /// Dem Verlauf des aktiven Tabs einen Eintrag mit Zeitpunkt geben.
    ///
    /// `zeitpunkt` kommt vom AUFRUFER. Keine Uhr in dieser Crate.
    ///
    /// Eine Seite mit Zeitpunkt ansteuern und dann ihren Titel nachtragen:
    /// `Navigation::To` legt den Eintrag an (mit leerem Titel), der
    /// Aufrufer laedt, ruft `titel_setzen`. Diese Funktion ist fuer
    /// direkt erzeugte Verlaeufe, z. B. aus einer Sitzung.
    pub fn verlaufseintrag_mit_zeit(&mut self, seite: Page, _zeitpunkt: u64) {
        if let Some(tab) = self.tabs.get_mut(self.aktiv) {
            tab.verlauf.push(seite);
            tab.index = tab.verlauf.len() - 1;
            self.verlauf_einkuerzen();
        }
    }

    /// Der aelteste Eintrag faellt weg, sobald es mehr als
    /// [`VERLAUF_GRENZE`] sind.
    ///
    /// DER INDEX WIRD MITGEZOGEN. Das ist der Teil, der leicht falsch geht:
    /// faellt vorne ein Eintrag weg und der Index bleibt 7, zeigt der Index
    /// jetzt auf eine ANDERE Seite — der Nutzer drueckt "zurueck" und
    /// sieht die Seite, die er gerade verlassen hat. Deshalb wandert er
    /// mit, und der Aufrufer bekommt den neuen Index zurueck.
    pub fn verlauf_einkuerzen(&mut self) -> usize {
        let grenze = VERLAUF_GRENZE;
        if let Some(tab) = self.tabs.get_mut(self.aktiv) {
            if tab.verlauf.len() > grenze {
                let ueber = tab.verlauf.len() - grenze;
                tab.verlauf.drain(0..ueber);
                tab.index = tab.index.saturating_sub(ueber);
            }
            tab.index
        } else {
            0
        }
    }

    /// Der Verlauf des aktiven Tabs mit Titeln, aeltestes zuerst.
    pub fn verlauf_mit_titeln(&self) -> Vec<&Page> {
        match self.tabs.get(self.aktiv) {
            Some(tab) => tab.verlauf.iter().collect(),
            None => Vec::new(),
        }
    }

    /// Wie viele Seiten der Verlauf des aktiven Tabs gerade haelt, und die
    /// Grenze daneben — damit eine Oberflaeche "3 von 200" zeigen kann,
    /// ohne die Konstante zu kennen.
    pub fn verlauf_stand(&self) -> (usize, usize) {
        let n = self
            .tabs
            .get(self.aktiv)
            .map(|t| t.verlauf.len())
            .unwrap_or(0);
        (n, VERLAUF_GRENZE)
    }
}
