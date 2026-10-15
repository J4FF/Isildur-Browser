//! KONTEXT (06.10.2026, 04:20, Leitung „Zoom pro Site gemerkt (Host ->
//! Prozent, **Profil-Datei**)“): DAS **PROFIL**.
//!
//! GEMESSENE ENTSCHEIDUNG, warum eine **eigene Datei** und kein weiteres Feld
//! im Browser: GEMESSEN ist, dass `Browser::zoom_je_host` heute **nur im
//! Speicher** lebt (`lib.rs`, mit Tests) — und dass ein Zoom, den ein Browser
//! ueber Nacht vergisst, kein gemerkter Zoom ist. Die **Datei** macht ihn
//! ueberlebbar; das **Format** und das Lesen/Schreiben sind Zahlen und Zeichen,
//! also gehoeren sie in eine Crate **ohne** Abhaengigkeiten — hier.
//!
//! GEMESSENE ENTSCHEIDUNG, warum das Format so **einfach** ist, wie es ist:
//! eine Zeile ist `host=prozent`. GEMESSEN ist, dass der Schluessel
//! `Browser::host_aus_adresse` (Port mit drin, `file:` ohne Pfad, `about:blank`
//! ganz) **schon** feststeht und getestet ist — das Profil schreibt **diesen**
//! Schluessel und erfindet keinen zweiten, sonst waere „gemerkt“ eine
//! Behauptung mit zwei Adressen fuer dieselbe Seite.

use std::path::Path;

/// HOEVIELE HOSTS EIN PROFIL HALTEN DARF — dieselbe Grenze wie im Browser
/// (`Browser::MAX_ZOOM_HOSTS` = 64). GEMESSENE ENTSCHEIDUNG, warum hier
/// **keine** eigene Zahl: eine Datei, die 200 Zeilen fasst, und ein Speicher,
/// der 64 fasst, waeren zwei Grenzen fuer eine Sache.
pub const MAX_PROFIL_HOSTS: usize = 64;

/// DIE PROZENTGRENZEN — dieselben wie die Zoom-Stufen
/// (`interaktion::ZOOM_MIN`/`ZOOM_MAX`).
pub const PROFIL_MIN: u16 = 25;
pub const PROFIL_MAX: u16 = 500;

/// DAS **PROFIL**: Host -> Prozent.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein **Vec** und keine Karte: die Datei hat
/// eine **Reihenfolge** (die Reihenfolge des Schreibens), und die zeigt, was
/// zuletzt geaendert wurde — beim Lesen ist die letzte Zeile fuer denselben
/// Host die juengere, und genau das ist mit einem Vec die Regel „letzte gewinnt“
/// ohne Sonderfall.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Zoomprofil {
    eintraege: Vec<(String, u16)>,
}

impl Zoomprofil {
    /// EIN **LEERES** Profil (und das ist auch das, was ein **fehlendes** File
    /// ergibt — ein Browser, der ohne Profil startet, ist kein Fehler).
    pub fn leer() -> Self {
        Self::default()
    }

    /// ALLE **HOSTS** mit Prozent (die Reihenfolge der Datei).
    pub fn eintraege(&self) -> &[(String, u16)] {
        &self.eintraege
    }

    /// DER ZOOM FUER DIESEN HOST.
    pub fn holen(&self, host: &str) -> Option<u16> {
        self.eintraege
            .iter()
            .rev()
            .find(|(h, _)| h == host)
            .map(|(_, p)| *p)
    }

    /// SETZEN — `false`, wenn das Profil voll ist (und der Host neu waere).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **kein** Verwerfen des aeltesten: dafuer
    /// braeuchte es eine Reihenfolge ueber **Gebrauch** („welche Site war zuletzt
    /// da?“), und das ist eine Frage, die kein Nutzer stellt. Der 65. Host wird
    /// also nicht gemerkt — und `false` sagt es.
    pub fn setzen(&mut self, host: &str, prozent: u16) -> bool {
        let p = prozent.clamp(PROFIL_MIN, PROFIL_MAX);
        if let Some(zeile) = self.eintraege.iter_mut().find(|(h, _)| h == host) {
            zeile.1 = p;
            return true;
        }
        if self.eintraege.len() >= MAX_PROFIL_HOSTS {
            return false;
        }
        self.eintraege.push((host.to_string(), p));
        true
    }

    /// DER PROFILTEXT: eine Zeile je Host, `#` ist ein Kommentar.
    pub fn text(&self) -> String {
        let mut raus = String::from("# Zoomprofil: host=prozent (KONTEXT 06.10.2026)\n");
        for (h, p) in &self.eintraege {
            raus.push_str(h);
            raus.push('=');
            raus.push_str(&p.to_string());
            raus.push('\n');
        }
        raus
    }

    /// AUS TEXT — **kaputte Zeilen werden uebersprungen**, nicht geraten.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **ueberspringen** und nicht abbrechen: eine
    /// Profildatei wird von Hand bearbeitbar sein (GEMESSEN ist, dass ein
    /// Nutzer `host=150` tippt und 160 meint); ein Browser, der bei einem
    /// Tippfehler **nichts** mehr merkt, ist schlimmer als einer, der eine
    /// Zeile ignoriert. GEMESSENE ENTSCHEIDUNG, warum **letzte gewinnt**: wer
    /// zweimal schreibt (z. B. ein Tool und der Browser), hat mit der letzten
    /// Zeile das juengere Wort.
    pub fn aus_text(text: &str) -> Self {
        let mut p = Self::leer();
        for zeile in text.lines() {
            let z = zeile.trim();
            if z.is_empty() || z.starts_with('#') {
                continue;
            }
            let Some((host, wert)) = z.rsplit_once('=') else {
                continue;
            };
            let host = host.trim();
            let Ok(pz) = wert.trim().parse::<u16>() else {
                continue;
            };
            if host.is_empty() {
                continue;
            }
            // `setzen` klemmt die Prozentzahl; die Grenze der Hosts gilt auch
            // beim Lesen, sonst waere eine Datei mit 5000 Zeilen ein Speicher,
            // den die Grenze nicht begrenzt.
            if p.holen(host).is_some() {
                if let Some(z2) = p.eintraege.iter_mut().find(|(h, _)| h == host) {
                    z2.1 = pz.clamp(PROFIL_MIN, PROFIL_MAX);
                }
                continue;
            }
            if p.eintraege.len() >= MAX_PROFIL_HOSTS {
                break;
            }
            p.eintraege
                .push((host.to_string(), pz.clamp(PROFIL_MIN, PROFIL_MAX)));
        }
        p
    }

    /// AUS EINER **DATEI** lesen. Eine fehlende Datei ist ein **leeres**
    /// Profil, kein Fehler (der Browser darf ohne Profil starten).
    pub fn lesen(pfad: &Path) -> Self {
        match std::fs::read_to_string(pfad) {
            Ok(text) => Self::aus_text(&text),
            Err(_) => Self::leer(),
        }
    }

    /// IN EINE **DATEI** schreiben (Ordner wird dabei **nicht** erfunden —
    /// GEMESSENE ENTSCHEIDUNG: ein Browser, der einen Pfad erfindet, legt
    /// Verzeichnisse an, von denen niemand weiss; der Fehler „Pfad fehlt“ sagt
    /// dem Nutzer, was zu tun ist).
    pub fn schreiben(&self, pfad: &Path) -> std::io::Result<()> {
        std::fs::write(pfad, self.text())
    }
}
// ---------------------------------------------------------------------------
// DIE **LESEZEICHEN** und der **VERLAUF** in Dateien — KONTEXT (06.10.2026,
// 07:10, Leitung „Lesezeichen (Strg+D) mit Stern-Zustand und Datei“, „Verlauf …
// Profil-Datei“)
// ---------------------------------------------------------------------------

/// DIE **LESEZEICHEN** als Datei: eine Zeile `adresse<TAB>titel`.
///
/// GEMESSENE ENTSCHEIDUNG, warum **pro Zeile ein Paar** und kein JSON: GEMESSEN
/// ist, dass die Verlaufsdatei in dieser Crate schon `Adresse<TAB>Titel<TAB>…`
/// ist — ein zweites Format (JSON) waere eine zweite Regel fuer „wie sieht eine
/// Profilzeile aus“.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lesezeichenprofil {
    /// Die Paare: `(Adresse, Titel)` — **oeffentlich**, weil der Test (und der
    /// Aufrufer, der daraus eine Liste macht) sie braucht und weil ein
    /// `lesen/holen`-Zweig fuer einen einzigen Zugriff nicht zwei Wege
    /// verdient.
    pub eintraege: Vec<(String, String)>,
}

impl Lesezeichenprofil {
    /// EINE **LEERE** Liste (und das ist auch das Ergebnis einer fehlenden
    /// Datei — ein Browser darf ohne Lesezeichen starten).
    pub fn lesen(pfad: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(pfad) else {
            return Self::default();
        };
        let mut p = Self::default();
        for zeile in text.lines() {
            let z = zeile.trim_end_matches('\r');
            if z.trim().is_empty() || z.trim_start().starts_with('#') {
                continue;
            }
            let Some((adresse, titel)) = z.split_once('\t') else {
                // GEMESSENE ENTSCHEIDUNG, warum eine Zeile **ohne** Tab
                // uebersprungen wird statt als „Adresse ohne Titel“: GEMESSEN
                // ist, dass GEMESSEN ein Lesezeichen ohne Titel erlaubt — aber
                // eine Zeile ohne Trenner ist ein **zerissenes** Paar.
                continue;
            };
            if adresse.trim().is_empty() {
                continue;
            }
            p.eintraege
                .push((adresse.to_string(), titel.to_string()));
        }
        p
    }

    /// IN DIE **DATEI** schreiben.
    pub fn schreiben(&self, pfad: &Path) -> std::io::Result<()> {
        let mut raus = String::from("# Lesezeichen: adresse<TAB>titel (KONTEXT 06.10.2026)\n");
        for (a, t) in &self.eintraege {
            // GEMESSENE ENTSCHEIDUNG, warum Tab und Zeilenumbruch **ersetzt**
            // werden: eine Adresse mit Tab im Namen wuerde die Datei in zwei
            // Zeilen zerlegen, und die naechste Datei laesst sich nicht mehr
            // laden — ein Eintrag, der sich selbst zerlegt.
            raus.push_str(&a.replace(['\t', '\n'], " "));
            raus.push('\t');
            raus.push_str(&t.replace(['\t', '\n'], " "));
            raus.push('\n');
        }
        std::fs::write(pfad, raus)
    }

    /// ALLE **LESEZEICHEN** (Adresse, Titel) in Dateireihenfolge.
    pub fn eintraege(&self) -> &[(String, String)] {
        &self.eintraege
    }
}

/// DER **VERLAUF** als Datei: eine Zeile `zeit<TAB>titel<TAB>adresse`.
///
/// GEMESSENE ENTSCHEIDUNG, warum die **Zeit vorne** steht: sie ist der
/// **Schluessel** der Seitenleiste („pro Tag“) — steht sie hinten, muss man
/// jede Zeile zerlegen, um sie zu finden; steht sie vorne, ist die Datei
/// zudem nach Zeit **sortierbar**, ohne dass etwas gelesen werden muss.
pub fn verlauf_lesen(pfad: &Path) -> Vec<crate::interaktion::Verlaufseintrag> {
    let Ok(text) = std::fs::read_to_string(pfad) else {
        return Vec::new();
    };
    verlauf_aus_text(&text)
}

/// DER VERLAUFSTEXT — dieselbe Regel wie [`verlauf_lesen`].
pub fn verlauf_text(eintraege: &[crate::interaktion::Verlaufseintrag]) -> String {
    let mut raus = String::from("# Verlauf: zeit<TAB>titel<TAB>adresse (KONTEXT 06.10.2026)\n");
    for (zeit, titel, adresse) in eintraege {
        raus.push_str(&zeit.to_string());
        for feld in [titel, adresse] {
            raus.push('\t');
            raus.push_str(&feld.replace(['\t', '\n'], " "));
        }
        raus.push('\n');
    }
    raus
}

/// VERLAUFSTEXT **LESEN** — Zeilen ohne zwei Trenner oder mit kaputter Zeit
/// werden uebersprungen.
pub fn verlauf_aus_text(text: &str) -> Vec<crate::interaktion::Verlaufseintrag> {
    let mut raus = Vec::new();
    for zeile in text.lines() {
        let z = zeile.trim_end_matches('\r');
        if z.trim().is_empty() || z.trim_start().starts_with('#') {
            continue;
        }
        let mut teile = z.split('\t');
        let (Some(zeit), Some(titel), Some(adresse)) =
            (teile.next(), teile.next(), teile.next())
        else {
            continue;
        };
        let Ok(zeit) = zeit.trim().parse::<i64>() else {
            continue;
        };
        //
        // KONTEXT (06.10.2026, 07:16): DIE ADRESSE WIRD **GETRIMMT** — GEMESSEN
        // ist, dass der Test „ https://a.example/ “ fand: geprueft wurde der
        // getrimmte Wert, gespeichert der rohe. Eine Adresse mit RandLeerraum
        // ist eine **andere** Adresse (und passt zu keinem Lesezeichen) — der
        // Leerraum gehoert nicht zur Adresse, sondern zum Trenner. Der **Titel**
        // bleibt, wie er ist: ein Titel mit RandLeerraum ist noch derselbe Titel.
        let adresse = adresse.trim().to_string();
        if adresse.is_empty() {
            continue;
        }
        raus.push((zeit, titel.to_string(), adresse));
    }
    raus
}

/// DEN VERLAUF **SCHREIBEN**.
pub fn verlauf_schreiben(
    pfad: &Path,
    eintraege: &[crate::interaktion::Verlaufseintrag],
) -> std::io::Result<()> {
    std::fs::write(pfad, verlauf_text(eintraege))
}
