//! Download-Verwaltung: Ziel, Fortschritt, Groessengrenze, sicherer Dateiname.
//!
//! Diese Crate laedt NICHTS selbst (kein Netz, kein Dateisystem — wie
//! `sitzung.rs` schreibt sie nur auf, statt zu speichern). Der Aufrufer
//! meldet empfangene Bytes; die Liste prueft Stand, Grenze und Namen.
//! So bleibt jeder Schritt testbar, auch ohne Server und Platte.
//!
//! Grenzen (M4/M6): hoechstens [`MAX_DOWNLOADS`] gleichzeitige Eintraege,
//! kein Download ueber [`MAX_DOWNLOAD_BYTES`], kein Dateiname ueber
//! [`MAX_DATEINAME_BYTES`]. Alles darueber sind benannte Fehler.

// ---------------------------------------------------------------------------
// Modul-Intern
// ---------------------------------------------------------------------------

/// Hoechstens so viele Downloads gleichzeitig in der Liste.
///
/// BEGRUENDUNG (M6): Jeder Eintrag haelt URL + Namen im Speicher; 32
/// parallele Downloads sind mehr, als je ein Fenster anzeigt — alles
/// darueber ist ein Leck durch Anklicken, kein Gebrauch.
pub const MAX_DOWNLOADS: usize = 32;
/// Absolute Obergrenze EINER Download-Groesse in Bytes (2 GiB).
///
/// BEGRUENDUNG (M4): Die Anfrage-Grenze (`grenze`) kommt vom Aufrufer
/// (Header/Content-Length: Eingabe!). Sie wird an dieser Decke
/// gekappt — `u64::MAX` aus einem gelogenen Header darf nie als
/// Erlaubnis durchgehen.
pub const MAX_DOWNLOAD_BYTES: u64 = 2_147_483_648;
/// Hoechste Dateinamen-Laenge in Bytes (128: passt auf jedes
/// Dateisystem, gekuerzt an Zeichengrenzen, nie mitten im Umlaut).
pub const MAX_DATEINAME_BYTES: usize = 128;
/// Ersatzname, wenn vom Vorschlag nichts uebrig bleibt.
pub const ERSATZ_DATEINAME: &str = "index";

/// Was ein Download gerade tut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadStand {
    /// Laeuft noch: Bytes duerfen gemeldet werden.
    Aktiv,
    /// Abgeschlossen: `empfangen` ist die endgueltige Groesse.
    Fertig,
    /// Angehalten — mit Grund statt Schweigen (Grenze? Abbruch?).
    Abgebrochen(String),
}

/// EIN Download: Ziel, Stand und Zaehlung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// Fortlaufende Nummer, nie zweimal (wie `TabId`).
    pub id: u64,
    /// Woher die Bytes kommen (nur notiert, nie aufgerufen).
    pub url: String,
    /// Wohin sie sollen: bereinigter Dateiname (siehe [`dateiname_sichern`]).
    pub dateiname: String,
    /// Angekuendigte Groesse (Content-Length), falls bekannt. Nur
    /// Anzeige — durchgesetzt wird `grenze`, Server luegen.
    pub erwartet: Option<u64>,
    /// Bisher gemeldete Bytes.
    pub empfangen: u64,
    /// Obergrenze in Bytes (Anfrage, gedeckelt auf [`MAX_DOWNLOAD_BYTES`]).
    pub grenze: u64,
    /// Der Stand (aktiv/fertig/abgebrochen mit Grund).
    pub stand: DownloadStand,
}

/// Fehler der Download-Verwaltung, alle benannt (M7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadFehler {
    /// Unbekannte Download-Nummer.
    Unbekannt(u64),
    /// Mehr als [`MAX_DOWNLOADS`] gleichzeitig.
    ZuVieleDownloads { aktuell: usize, maximum: usize },
    /// Groessengrenze ueberschritten — der Eintrag steht danach auf
    /// `Abgebrochen`, die gezaehlten Bytes bleiben lesbar.
    ZuGross { empfangen: u64, grenze: u64 },
    /// Melden oder Fertigmelden auf beendetem Eintrag (fertig/abgebrochen).
    Beendet(u64),
}

impl std::fmt::Display for DownloadFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadFehler::Unbekannt(id) => write!(f, "unbekannter Download {id}"),
            DownloadFehler::ZuVieleDownloads { aktuell, maximum } => {
                write!(f, "zu viele Downloads: {aktuell} von {maximum}")
            }
            DownloadFehler::ZuGross { empfangen, grenze } => {
                write!(
                    f,
                    "Download mit {empfangen} Bytes ueber der Grenze {grenze}"
                )
            }
            DownloadFehler::Beendet(id) => {
                write!(f, "Download {id} ist schon beendet")
            }
        }
    }
}

impl std::error::Error for DownloadFehler {}

/// Die Liste aller Downloads dieses Fensters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadListe {
    /// Alle Eintraege, auch beendete (Verlauf statt Vergessen).
    downloads: Vec<Download>,
    /// Naechste freie Nummer — faellt nie zurueck (wie `naechste_tab_id`).
    naechste_id: u64,
}

impl DownloadListe {
    /// Eine leere Downloadliste. Nummern beginnen bei 1.
    pub fn neu() -> Self {
        DownloadListe {
            downloads: Vec::new(),
            naechste_id: 1,
        }
    }

    /// Startet einen Download: URL notieren, Dateinamen bereinigen,
    /// Grenze deckeln. Gibt die Nummer zurueck, unter der gemeldet wird.
    ///
    /// `erwartet` ist Anzeige (darf luegen), `grenze` ist Gesetz
    /// (`None` heisst: absolute Decke [`MAX_DOWNLOAD_BYTES`]).
    pub fn starten(
        &mut self,
        url: &str,
        vorschlag: &str,
        erwartet: Option<u64>,
        grenze: Option<u64>,
    ) -> Result<u64, DownloadFehler> {
        if self.downloads.len() >= MAX_DOWNLOADS {
            return Err(DownloadFehler::ZuVieleDownloads {
                aktuell: self.downloads.len(),
                maximum: MAX_DOWNLOADS,
            });
        }
        let id = self.naechste_id;
        self.naechste_id = self.naechste_id.saturating_add(1);
        self.downloads.push(Download {
            id,
            url: url.to_string(),
            dateiname: dateiname_sichern(vorschlag),
            erwartet,
            empfangen: 0,
            grenze: grenze.unwrap_or(MAX_DOWNLOAD_BYTES).min(MAX_DOWNLOAD_BYTES),
            stand: DownloadStand::Aktiv,
        });
        Ok(id)
    }

    /// Meldet `bytes` neue Bytes. Zaehlt saettigend (kein Wickeln bei
    /// gelogenen Chunks) und bricht bei Grenz-Ueberschreitung mit
    /// benanntem Fehler AB — der Eintrag bleibt lesbar stehen.
    pub fn melden(&mut self, id: u64, bytes: u64) -> Result<(u64, Option<u64>), DownloadFehler> {
        let eintrag = self
            .downloads
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(DownloadFehler::Unbekannt(id))?;
        if !matches!(eintrag.stand, DownloadStand::Aktiv) {
            return Err(DownloadFehler::Beendet(id));
        }
        eintrag.empfangen = eintrag.empfangen.saturating_add(bytes);
        if eintrag.empfangen > eintrag.grenze {
            eintrag.stand = DownloadStand::Abgebrochen(narsil_i18n::tr("Size limit exceeded").to_string());
            return Err(DownloadFehler::ZuGross {
                empfangen: eintrag.empfangen,
                grenze: eintrag.grenze,
            });
        }
        Ok((eintrag.empfangen, eintrag.erwartet))
    }

    /// Schliesst einen Download erfolgreich ab.
    pub fn fertig(&mut self, id: u64) -> Result<(), DownloadFehler> {
        let eintrag = self
            .downloads
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(DownloadFehler::Unbekannt(id))?;
        if !matches!(eintrag.stand, DownloadStand::Aktiv) {
            return Err(DownloadFehler::Beendet(id));
        }
        eintrag.stand = DownloadStand::Fertig;
        Ok(())
    }

    /// Bricht einen Download mit Grund ab. Der Grund wird auf 256 Zeichen
    /// gekuerzt (M4: Fehlertexte aus Eingaben duerfen nicht wachsen).
    pub fn abbrechen(&mut self, id: u64, grund: &str) -> Result<(), DownloadFehler> {
        let eintrag = self
            .downloads
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(DownloadFehler::Unbekannt(id))?;
        if !matches!(eintrag.stand, DownloadStand::Aktiv) {
            return Err(DownloadFehler::Beendet(id));
        }
        let kurz: String = grund.chars().take(256).collect();
        eintrag.stand = DownloadStand::Abgebrochen(kurz);
        Ok(())
    }

    /// Fortschritt `(empfangen, erwartet)`, wenn es den Eintrag gibt.
    pub fn fortschritt(&self, id: u64) -> Option<(u64, Option<u64>)> {
        self.downloads
            .iter()
            .find(|d| d.id == id)
            .map(|d| (d.empfangen, d.erwartet))
    }

    /// Der Eintrag mit dieser Nummer, wenn es ihn gibt.
    pub fn eintrag(&self, id: u64) -> Option<&Download> {
        self.downloads.iter().find(|d| d.id == id)
    }

    /// Wie viele Eintraege die Liste haelt (inklusive beendeter).
    pub fn anzahl(&self) -> usize {
        self.downloads.len()
    }
}

impl Default for DownloadListe {
    fn default() -> Self {
        Self::neu()
    }
}

/// Macht aus einem Vorschlag einen sicheren Dateinamen.
///
/// Entfernt alles, womit ein Name aus seinem Ordner ausbricht oder
/// unsichtbar wird: Pfad-Trenner (`/` und `\` — es zaehlt nur das
/// letzte Segment), Steuerzeichen, fuehrende Punkte (`..`, `.versteckt`),
/// Punkte/Leerzeichen am Ende. Umlaute bleiben (gueltige Namen).
/// Leer danach heisst [`ERSATZ_DATEINAME`]; Laenge gedeckelt auf
/// [`MAX_DATEINAME_BYTES`] an Zeichengrenzen (nie mitten im Umlaut).
///
/// KEIN Prozent-Dekodieren hier: Das gehoert zu narsil-url, und eine neue
/// Crate-Abhaengigkeit fuer eine Zeichenklasse waere die falsche
/// Richtung. Der Aufrufer dekodiert vorher, wenn der Name aus einer URL
/// kommt.
pub fn dateiname_sichern(vorschlag: &str) -> String {
    // Nur das letzte Segment: `a/../b` endet als `b`, nie als Ausbruch.
    let mut rest = vorschlag;
    if let Some(i) = rest.rfind('/') {
        rest = &rest[i + 1..];
    }
    if let Some(i) = rest.rfind('\\') {
        rest = &rest[i + 1..];
    }
    // Steuerzeichen raus, Rest behalten (inklusive Umlaute).
    let mut name: String = rest.chars().filter(|c| !c.is_control()).collect();
    // Fuehrende Punkte weg (`..`, `.profil`), End-Punkte/Leerzeichen weg.
    while name.starts_with('.') {
        name.remove(0);
    }
    while name.ends_with('.') || name.ends_with(' ') {
        name.pop();
    }
    // M4: Laenge deckeln, aber nur an Zeichengrenzen schneiden.
    while name.len() > MAX_DATEINAME_BYTES {
        name.pop();
    }
    // Nach dem Kuerzen kann wieder ein Punkt am Ende stehen.
    while name.ends_with('.') || name.ends_with(' ') {
        name.pop();
    }
    if name.is_empty() {
        return ERSATZ_DATEINAME.to_string();
    }
    name
}
