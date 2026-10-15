//! Release 08.10.2026: FEATURE 6 — KOPFZEILEN-REGELN JE DOMAIN.
//!
//! Die Regeln stehen in `<profil>/features/kopfregeln.tsv` (eine Regel je
//! Zeile: Domain, Richtung, Name, Aktion, Wert) und wirken ueber
//! `narsil_net::haken` auf jede Anfrage und jede Umleitung. Es gibt bewusst
//! keine mitgelieferten Regeln — insbesondere keine, die Bezahlschranken
//! umgehen (z. B. einen Suchmaschinen-Bot vortaeuschen).

use super::{felder, lesen_text, ordner, schreiben, zeile};
use narsil_net::haken::{KopfRegel, Richtung};

fn pfad() -> std::path::PathBuf {
    ordner().join("kopfregeln.tsv")
}

/// Die gespeicherten Regeln lesen (ungueltige Zeilen fallen weg).
pub fn laden() -> Vec<KopfRegel> {
    lesen_text(&pfad())
        .lines()
        .take(narsil_net::haken::MAX_REGELN)
        .filter_map(|z| {
            let f = felder(z);
            if f.len() < 4 {
                return None;
            }
            let richtung = if f[1] == "antwort" { Richtung::Antwort } else { Richtung::Anfrage };
            let wert = if f[3] == "entfernen" { None } else { Some(f.get(4).cloned().unwrap_or_default()) };
            let r = KopfRegel { domain: f[0].clone(), richtung, name: f[2].clone(), wert };
            r.ist_gueltig().then_some(r)
        })
        .collect()
}

/// Die Regeln speichern.
pub fn speichern(regeln: &[KopfRegel]) -> Result<(), String> {
    let text: Vec<String> = regeln
        .iter()
        .take(narsil_net::haken::MAX_REGELN)
        .map(|r| {
            zeile(&[
                &r.domain,
                if r.richtung == Richtung::Antwort { "antwort" } else { "anfrage" },
                &r.name,
                if r.wert.is_some() { "setzen" } else { "entfernen" },
                r.wert.as_deref().unwrap_or(""),
            ])
        })
        .collect();
    schreiben(&pfad(), text.join("\n").as_bytes())
}

/// Eine Regel aus Formularwerten bauen und pruefen.
pub fn aus_formular(domain: &str, richtung: &str, name: &str, aktion: &str, wert: &str) -> Result<KopfRegel, String> {
    let domain = domain.trim().trim_start_matches("https://").trim_start_matches("http://");
    let domain = domain.split('/').next().unwrap_or("").to_ascii_lowercase();
    if domain.is_empty() || domain.contains(char::is_whitespace) {
        return Err(narsil_i18n::tr("Please enter a domain (e.g. example.com or *)").into());
    }
    let r = KopfRegel {
        domain,
        richtung: if richtung == "antwort" { Richtung::Antwort } else { Richtung::Anfrage },
        name: name.trim().to_string(),
        wert: if aktion == "entfernen" { None } else { Some(wert.trim().to_string()) },
    };
    if !r.ist_gueltig() {
        return Err(narsil_i18n::trf!("The header “{}” is not allowed or invalid", r.name));
    }
    Ok(r)
}

/// Die Regeln an die Netzschicht geben (leer, wenn die Funktion aus ist).
pub fn anwenden(an: bool) {
    narsil_net::haken::kopfregeln_setzen(if an { laden() } else { Vec::new() });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regeln_ueberleben_die_datei_und_ungueltige_fallen_weg() {
        let dir = std::env::temp_dir().join(format!("narsil-test-kopf-{}-{:?}", std::process::id(), std::thread::current().id()));
        super::super::ordner_fuer_faden(Some(dir.clone()));
        let a = aus_formular("https://Beispiel.de/pfad", "anfrage", "User-Agent", "setzen", "Mobil/1.0").expect("a");
        assert_eq!(a.domain, "beispiel.de");
        let b = aus_formular("*", "antwort", "X-Tracking", "entfernen", "").expect("b");
        assert!(aus_formular("x.de", "anfrage", "Host", "setzen", "y").is_err());
        assert!(aus_formular("", "anfrage", "X-A", "setzen", "y").is_err());
        speichern(&[a.clone(), b.clone()]).expect("speichern");
        assert_eq!(laden(), vec![a, b]);
        super::super::ordner_fuer_faden(None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
