//! Release 12.10.2026: der Blattschrank — die geholten externen Stylesheets
//! (`<link rel="stylesheet">`) je aufgeloester Adresse, prozessweit.
//!
//! GEMESSEN vorher: `narsil::blaetter` ersetzte jedes geholte `<link>` im
//! Baum durch ein `<style>` mit dem Text. Fuer Skripte gab es den Link
//! danach nicht mehr — `document.querySelectorAll('link')` fand 0,
//! `link.onload`/`onerror` kamen nie (WPT HTMLLinkElement-load-event,
//! link-load-error-events, HTMLLinkElement-disabled-*), und ein per Skript
//! eingefuegter Link wurde nie geholt.
//!
//! DER WEG: der Link bleibt ein Link. Der Abrufer legt das CSS hier ab, und
//! JEDER Seitenbau fuellt die Link-Bloecke daraus (siehe
//! `Bauer::kaskade_einrichten`). Fehlende Adressen holt der Takt nach.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Hoechstzahl Eintraege (M3).
const MAX_EINTRAEGE: usize = 1024;
/// Hoechstens so viele Bytes CSS im Schrank (M3); aeltere fallen heraus.
const MAX_BYTES: usize = 64 * 1024 * 1024;

/// Der Stand einer Adresse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stand {
    /// Geholt; der Text (mit eingesetzten `@import`, siehe `narsil::blaetter`).
    Geladen(Arc<str>),
    /// Der Abruf scheiterte (Netz, Status, Typ, Groesse).
    Fehler,
}

struct Schrank {
    eintraege: HashMap<String, (Stand, u64)>,
    bytes: usize,
    uhr: u64,
}

static SCHRANK: Mutex<Option<Schrank>> = Mutex::new(None);

fn mit<T>(f: impl FnOnce(&mut Schrank) -> T) -> T {
    let mut g = SCHRANK.lock().unwrap_or_else(|e| e.into_inner());
    let s = g.get_or_insert_with(|| Schrank { eintraege: HashMap::new(), bytes: 0, uhr: 0 });
    f(s)
}

fn groesse(st: &Stand) -> usize {
    match st {
        Stand::Geladen(t) => t.len(),
        Stand::Fehler => 0,
    }
}

fn eintragen(url: &str, st: Stand) {
    if url.is_empty() || url.len() > 8192 {
        return;
    }
    mit(|s| {
        s.uhr += 1;
        let uhr = s.uhr;
        if let Some((alt, _)) = s.eintraege.remove(url) {
            s.bytes = s.bytes.saturating_sub(groesse(&alt));
        }
        s.bytes += groesse(&st);
        s.eintraege.insert(url.to_string(), (st, uhr));
        while (s.bytes > MAX_BYTES || s.eintraege.len() > MAX_EINTRAEGE) && s.eintraege.len() > 1 {
            let Some(aeltester) = s.eintraege.iter().min_by_key(|(_, (_, u))| *u).map(|(k, _)| k.clone()) else { break };
            if let Some((weg, _)) = s.eintraege.remove(&aeltester) {
                s.bytes = s.bytes.saturating_sub(groesse(&weg));
            }
        }
    });
}

/// Ein geholtes Blatt ablegen.
pub fn ablegen(url: &str, css: &str) {
    eintragen(url, Stand::Geladen(Arc::from(css)));
}

/// Ein gescheitertes Blatt vermerken.
pub fn fehler(url: &str) {
    eintragen(url, Stand::Fehler);
}

/// Der Stand einer Adresse (`None` = nie angefragt).
pub fn stand(url: &str) -> Option<Stand> {
    mit(|s| s.eintraege.get(url).map(|(st, _)| st.clone()))
}

/// Der Text eines geholten Blatts.
pub fn inhalt(url: &str) -> Option<Arc<str>> {
    match stand(url) {
        Some(Stand::Geladen(t)) => Some(t),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ablegen_und_lesen() {
        ablegen("https://x.test/schrank-a.css", "p{color:red}");
        fehler("https://x.test/schrank-b.css");
        assert_eq!(inhalt("https://x.test/schrank-a.css").as_deref(), Some("p{color:red}"));
        assert_eq!(stand("https://x.test/schrank-b.css"), Some(Stand::Fehler));
        assert_eq!(stand("https://x.test/schrank-c.css"), None);
    }
}
