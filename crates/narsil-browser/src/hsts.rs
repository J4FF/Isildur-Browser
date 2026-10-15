//! HSTS- und Tracker-Sperrliste.
//! Implementierung nach RFC 6265 und Ergänzungen fuer Browser-Sicherheit.

/// Der HSTS-Eintrag (HTTP Strict Transport Security).
///
/// Speichert, dass eine Domain fuer HTTPS gezwungen werden soll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HSTSEintrag {
    /// Die Domain, die HSTS aktiv hat.
    domain: String,
    /// Wann der HSTS-Eintrag gesetzt wurde (Unix-Timestamp).
    gesetzt_am: u64,
    /// Die maximale Alterungszeit in Sekunden (typischerweise 1 Jahr = 31536000).
    max_alter: u64,
}

/// Der Zustand des HSTS-Speichers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HSTSSpeicher {
    /// Alle HSTS-Eintraege.
    einsaetze: Vec<HSTSEintrag>,
}

impl HSTSSpeicher {
    /// Einen neuen, leeren HSTS-Speicher erzeugen.
    pub fn neu() -> Self {
        Self {
            einsaetze: Vec::new(),
        }
    }

    /// Pruft, ob eine Domain HSTS aktiv hat.
    ///
    /// Rueckgabe: (ist_hsts, soll_auf_https_umleiten)
    /// - ist_hsts: Die Domain hat HSTS aktiv und der Eintrag ist nicht abgelaufen.
    /// - soll_auf_https_umleiten: Wenn der Ursprungsprotokoll http:// war,
    ///   sollte auf https:// umgelenkt werden.
    pub fn prüfen(&self, domain: &str, jetzt: u64) -> (bool, bool) {
        let domain_lower = domain.to_ascii_lowercase();
        for eintrag in &self.einsaetze {
            // Abgelaufene Eintraege zaehlen nicht (sonst wuerde eine
            // Wochen alte HSTS-Regel ewig auf HTTPS zwingen).
            if eintrag.domain == domain_lower
                && jetzt < eintrag.gesetzt_am.saturating_add(eintrag.max_alter)
            {
                return (true, true); // HSTS aktiv, https erzwingen
            }
        }
        (false, false) // Kein (gueltiger) HSTS Eintrag gefunden
    }

    /// Fuegt einen HSTS-Eintrag fuer eine Domain hinzu.
    ///
    /// Ersetzt vorhandenen Eintrag wenn schon vorhanden. `jetzt` sind die
    /// Unix-Sekunden vom Aufrufer (keine Uhr in der Crate: gleiche Eingabe,
    /// gleiches Ergebnis). Höchstens 1024 Eintraege (M6); voll heisst:
    /// der aelteste fliegt, damit HSTS nie den Speicher frisst.
    pub fn hinzufuegen(&mut self, domain: &str, jetzt: u64) {
        let domain_lower = domain.to_ascii_lowercase();

        // Vorhandenen Eintrag entfernen falls vorhanden
        self.einsaetze.retain(|e| e.domain != domain_lower);

        // Neuen Eintrag hinzufuegen (Grenze: 1024, der aelteste fliegt).
        if self.einsaetze.len() >= 1024 {
            if let Some(aeltester) = self
                .einsaetze
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.gesetzt_am)
                .map(|(i, _)| i)
            {
                self.einsaetze.remove(aeltester);
            }
        }
        self.einsaetze.push(HSTSEintrag {
            domain: domain_lower,
            gesetzt_am: jetzt,
            max_alter: 31536000, // 1 Jahr in Sekunden
        });
    }

    /// Entfernt einen HSTS-Eintrag.
    pub fn entfernen(&mut self, domain: &str) {
        let domain_lower = domain.to_ascii_lowercase();
        self.einsaetze.retain(|e| e.domain != domain_lower);
    }

    /// Gibt alle HSTS-Domains zurueck.
    pub fn domains(&self) -> Vec<String> {
        self.einsaetze.iter().map(|e| e.domain.clone()).collect()
    }
}

/// Tracker-Sperrliste.
///
/// Eine einfache Liste von Domains, die als Tracker gesperrt sind.
/// Drittanbieter-Cookies von diesen Domains werden blockiert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerSperrliste {
    /// Gesperrte Domains.
    gesperrt: Vec<String>,
    /// Maximale Anzahl an gesperrten Domains (Standard: 1000).
    max_domains: usize,
}

impl TrackerSperrliste {
    /// Einen neuen Tracker-Sperrlisten-Standard erzeugen.
    ///
    /// Die Standardgrenze betraegt 1000 Domains.
    pub fn neu() -> Self {
        Self {
            gesperrt: Vec::new(),
            max_domains: 1000,
        }
    }

    ///Fuegt eine Domain zur Sperrliste hinzu.
    ///
    /// Gibt true zurueck, wenn die Domain hinzugefuegt wurde.
    /// Fuegt nicht hinzu, wenn die Liste bereits voll ist.
    pub fn domein_sperren(&mut self, domain: &str) -> bool {
        let domain_lower = domain.to_ascii_lowercase();

        // Duplikat pruefen
        if self.gesperrt.iter().any(|d| d == &domain_lower) {
            return false;
        }

        // Grenze pruefen
        if self.gesperrt.len() >= self.max_domains {
            return false;
        }

        self.gesperrt.push(domain_lower);
        true
    }

    ///Entfernt eine Domain aus der Sperrliste.
    pub fn domein_entfernen(&mut self, domain: &str) -> bool {
        let domain_lower = domain.to_ascii_lowercase();
        if let Some(pos) = self.gesperrt.iter().position(|d| d == &domain_lower) {
            self.gesperrt.remove(pos);
            true
        } else {
            false
        }
    }

    ///Prueft, ob eine Domain gesperrt ist.
    pub fn ist_gesperrt(&self, domain: &str) -> bool {
        let domain_lower = domain.to_ascii_lowercase();
        self.gesperrt.iter().any(|d| d == &domain_lower)
    }

    ///Gibt alle gesperrten Domains zurueck.
    pub fn gesperrte_domains(&self) -> &[String] {
        &self.gesperrt
    }
}
