//! Release 08.10.2026: DIE RUHE-LISTEN fuer den Tab „Features".
//!
//! Zwei Filterlisten in derselben Schreibweise wie die Grundliste
//! (Adblock Plus 2.0), damit derselbe Leser ([`crate::liste_lesen`]) und
//! derselbe Pruefer ([`crate::Pruefer`]) sie verarbeiten:
//!
//! * [`COOKIE_BANNER`] — die Fenster der Einwilligungs-Plattformen
//!   (Feature 11, Cookie-Autopilot);
//! * [`STOERER`] — Chat-Widgets, Push-Fragen, Newsletter-Fenster und
//!   Umfragen (Feature 9, „Bitte nicht stoeren").

use crate::{Pruefer, liste_lesen};
use std::sync::OnceLock;

/// Die Cookie-Banner-Liste.
pub const COOKIE_BANNER: &str = include_str!("../daten/cookie_banner.txt");
/// Die Stoerer-Liste.
pub const STOERER: &str = include_str!("../daten/stoerer.txt");

/// Der Pruefer fuer die Cookie-Banner-Liste (einmal gebaut).
pub fn cookie_pruefer() -> &'static Pruefer {
    static P: OnceLock<Pruefer> = OnceLock::new();
    P.get_or_init(|| Pruefer::aus(&liste_lesen(COOKIE_BANNER)))
}

/// Der Pruefer fuer die Stoerer-Liste (einmal gebaut).
pub fn stoerer_pruefer() -> &'static Pruefer {
    static P: OnceLock<Pruefer> = OnceLock::new();
    P.get_or_init(|| Pruefer::aus(&liste_lesen(STOERER)))
}

/// Die kosmetischen Selektoren fuer einen Host aus den gewaehlten Listen.
pub fn selektoren(host: &str, cookie: bool, stoerer: bool) -> Vec<String> {
    let mut v = Vec::new();
    if cookie {
        v.extend(cookie_pruefer().kosmetisch_fuer(host).into_iter().map(|k| k.selektor.clone()));
    }
    if stoerer {
        v.extend(stoerer_pruefer().kosmetisch_fuer(host).into_iter().map(|k| k.selektor.clone()));
    }
    v
}

/// Wird die Adresse von der Stoerer-Liste gesperrt (Chat-/Push-Skripte)?
pub fn stoerer_gesperrt(url: &str) -> bool {
    matches!(
        stoerer_pruefer().pruefe(&crate::Anfrage::neu(url)),
        crate::Urteil::Blockiert(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_listen_werden_ohne_reste_gelesen() {
        for (name, text) in [("cookie", COOKIE_BANNER), ("stoerer", STOERER)] {
            let a = liste_lesen(text);
            assert_eq!(a.unerkannt, 0, "{name}: unerkannte Zeilen");
            assert!(a.regeln.len() > 20, "{name}: {} Regeln", a.regeln.len());
        }
    }

    #[test]
    fn die_banner_kennungen_gelten_auf_jeder_seite() {
        let s = selektoren("news.example", true, false);
        assert!(s.iter().any(|x| x.contains("onetrust-banner-sdk")), "{s:?}");
        assert!(s.iter().any(|x| x.contains("CybotCookiebotDialog")));
        let beide = selektoren("news.example", true, true);
        assert!(beide.len() > s.len());
        assert!(beide.iter().any(|x| x.contains("intercom-container")));
        assert!(selektoren("news.example", false, false).is_empty());
    }

    #[test]
    fn chat_skripte_werden_gesperrt_normale_nicht() {
        assert!(stoerer_gesperrt("https://widget.intercom.io/widget/abc"));
        assert!(stoerer_gesperrt("https://embed.tawk.to/123/default"));
        assert!(stoerer_gesperrt("https://cdn.onesignal.com/sdks/OneSignalSDK.js"));
        assert!(!stoerer_gesperrt("https://www.example.com/app.js"));
        assert!(!stoerer_gesperrt("https://intercom.io.example.org/x.js"));
    }
}
