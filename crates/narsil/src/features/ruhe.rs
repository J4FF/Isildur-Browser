//! Release 08.10.2026: FEATURE 9 („Bitte nicht stören") und FEATURE 11
//! (Cookie-Autopilot).
//!
//! Drei Wege, alle in der eigenen Engine:
//!
//! 1. **Kosmetik**: die Selektoren der Ruhe-Listen aus `narsil-adblock`
//!    (gleicher Leser, gleicher Pruefer wie der Werbeblocker) werden mit
//!    `narsil_css::kosmetik` zu einem Blatt und vor dem Parsen in die Seite
//!    gesetzt. Dazu wird ein von einem Overlay gesperrtes Scrollen wieder
//!    freigegeben.
//! 2. **Netz**: Chat- und Push-Skripte sperrt die Stoerer-Liste ueber
//!    `narsil_net::haken::sperre_setzen`; der Autopilot sendet `Sec-GPC`/`DNT`.
//! 3. **Klick** (Feature 11): nach dem Laden sucht der Autopilot im
//!    Einwilligungsfenster den Knopf „Ablehnen“ / „Nur notwendige“ und loest
//!    auf ihm ein echtes `click`-Ereignis aus — derselbe Weg wie ein
//!    Mausklick (Hoerer der Seite und `onclick`). Mehrmals in den ersten
//!    Sekunden, weil viele Banner spaet kommen. Er klickt nie „Akzeptieren“.
//!    (Das DOM-`element.click()` der eigenen Bruecke loest kein Ereignis
//!    aus — darum klickt der Browser selbst und kein Seitenskript.)

use super::{Feature, Schalter};
use narsil_fmt_html::HtmlNode;

/// Die Kennung an allen eingesetzten Knoten (damit Schnappschuss und
/// Element-Waechter sie erkennen und auslassen).
pub const KENNUNG: &str = "data-narsil-feature";

/// Das Skript des Ruhemodus: Benachrichtigungs-Fragen verneinen (wirkt,
/// sobald die Engine `Notification` anbietet).
pub const RUHE_JS: &str = r#"(function(){
try{if(typeof Notification!=="undefined"){Notification.requestPermission=function(){return Promise.resolve("denied");};}}catch(e){}
})();"#;

/// Gesperrtes Scrollen wieder freigeben (Overlays setzen `overflow:hidden`).
pub const SCROLL_FREI: &str = "html,body{overflow:auto !important}";

/// Knopftexte, die ablehnen (klein, Anfang des Textes).
pub const ABLEHNEN: &[&str] = &[
    "alle ablehnen", "ablehnen", "nur notwendige", "nur essenzielle", "nur essentielle", "nur erforderliche",
    "nur technisch notwendige", "notwendige cookies", "weiter ohne einwilligung", "nicht einverstanden",
    "optionale cookies ablehnen", "reject all", "reject", "decline", "decline all", "only necessary",
    "necessary only", "use necessary cookies only", "refuse", "deny", "tout refuser", "refuser",
    "rifiuta", "rechazar", "weigeren",
];

/// Woerter, die eine Zustimmung bedeuten — solche Knoepfe werden nie geklickt.
pub const ZUSTIMMEN: &[&str] = &["akzeptier", "zustimm", "accept", "agree", "allow", "erlauben", "einverstanden", "ok", "verstanden", "got it"];

/// Bekannte Ablehnen-Knoepfe: (Attribut, Wert); `class` = enthaelt die Klasse.
pub const DIREKT: &[(&str, &str)] = &[
    ("id", "onetrust-reject-all-handler"),
    ("id", "CybotCookiebotDialogBodyButtonDecline"),
    ("id", "CybotCookiebotDialogBodyLevelButtonLevelOptinDeclineAll"),
    ("data-testid", "uc-deny-all-button"),
    ("id", "didomi-notice-disagree-button"),
    ("class", "cky-btn-reject"),
    ("class", "osano-cm-denyAll"),
    ("id", "truste-consent-required"),
    ("class", "cmpboxbtnno"),
    ("data-cookiefirst-action", "reject"),
    ("class", "cc-deny"),
    ("class", "cmplz-deny"),
    ("id", "BorlabsCookieBtnRefuse"),
];

/// Woran man ein Einwilligungsfenster erkennt (in `id` oder `class`).
pub const BEHAELTER: &[&str] = &[
    "cookie", "consent", "gdpr", "onetrust", "cybot", "usercentrics", "didomi", "truste", "borlabs", "cmplz",
    "cky-", "osano", "klaro", "iubenda", "sp_message", "cmpbox", "privacy-banner", "datenschutz",
];

fn normal(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Ist der Knopftext ein Ablehnen?
pub fn lehnt_ab(t: &str) -> bool {
    let t = normal(t);
    if t.is_empty() || t.chars().count() > 60 {
        return false;
    }
    let verneint = t.contains("nicht") || t.contains("ablehn") || t.contains("reject") || t.contains("only")
        || t.contains("nur ") || t.contains("decline") || t.contains("deny") || t.contains("refus");
    if !verneint && ZUSTIMMEN.iter().any(|z| t.contains(z)) {
        return false;
    }
    ABLEHNEN.iter().any(|a| t == *a || t.starts_with(a))
}

fn attr<'a>(k: &'a HtmlNode, n: &str) -> Option<&'a str> {
    match k {
        HtmlNode::Element { attributes, .. } => attributes.get(n).map(String::as_str),
        HtmlNode::Text(_) => None,
    }
}

fn ist_knopf(k: &HtmlNode) -> bool {
    let HtmlNode::Element { tag, .. } = k else { return false };
    let t = tag.to_ascii_lowercase();
    t == "button"
        || t == "a"
        || attr(k, "role") == Some("button")
        || (t == "input" && matches!(attr(k, "type").map(str::to_ascii_lowercase).as_deref(), Some("button" | "submit")))
}

/// Die Vorlauf-Nummer des Ablehnen-Knopfs, falls die Seite einen zeigt.
pub fn ablehnen_knopf(html: &[HtmlNode], patch: &narsil_page::Patch) -> Option<usize> {
    let st = super::baum::stellen(html);
    // 1. Bekannte Knoepfe.
    for s in &st {
        for (n, w) in DIREKT {
            let treffer = match *n {
                "class" => attr(s.knoten, "class").is_some_and(|c| c.split_whitespace().any(|x| x == *w)),
                _ => attr(s.knoten, n) == Some(*w),
            };
            if treffer {
                return Some(s.ord);
            }
        }
    }
    // 2. Knoepfe mit Ablehnen-Text in einem Einwilligungsfenster.
    for s in &st {
        let kennung = format!(
            "{} {}",
            attr(s.knoten, "id").unwrap_or(""),
            attr(s.knoten, "class").unwrap_or("")
        )
        .to_ascii_lowercase();
        if !BEHAELTER.iter().any(|b| kennung.contains(b)) {
            continue;
        }
        for x in &st[s.ord + 1..s.ende.min(st.len())] {
            if !ist_knopf(x.knoten) {
                continue;
            }
            // Der Text eines `<button>` wird nicht mitgezaehlt (Steuerelement),
            // also kommt er aus dem Knoten selbst.
            let mut text = super::baum::text_von(&st, x.ord, Some(patch));
            if text.is_empty() {
                text = super::baum::text_des_knotens(x.knoten);
            }
            if text.is_empty() {
                text = attr(x.knoten, "value").or(attr(x.knoten, "aria-label")).unwrap_or("").to_string();
            }
            if lehnt_ab(&text) {
                return Some(x.ord);
            }
        }
    }
    None
}

/// Das CSS des Ruhemodus fuer eine Seite (leer, wenn er aus ist).
///
/// Die Cookie-Banner stehen hier NICHT: ein von Anfang an ausgeblendetes
/// Fenster bekommt in der Engine keine `onclick`-Hoerer, und der Autopilot
/// koennte nicht mehr ablehnen. Sie blendet [`cookie_css`] nach dem Klick aus.
pub fn css_fuer(host: &str, f: &Schalter) -> String {
    if !f.an(Feature::Ruhemodus) {
        return String::new();
    }
    let selektoren = narsil_adblock::ruhe::selektoren(host, false, true);
    let refs: Vec<&str> = selektoren.iter().map(String::as_str).collect();
    let mut css = narsil_css::kosmetik::kosmetik_als_css(&refs);
    css.push_str(SCROLL_FREI);
    css
}

/// Das CSS, das die Einwilligungsfenster ausblendet (nach dem Ablehnen).
pub fn cookie_css(host: &str) -> String {
    let selektoren = narsil_adblock::ruhe::selektoren(host, true, false);
    let refs: Vec<&str> = selektoren.iter().map(String::as_str).collect();
    let mut css = narsil_css::kosmetik::kosmetik_als_css(&refs);
    css.push_str(SCROLL_FREI);
    css
}

/// Blatt (und beim Ruhemodus das Skript) in den Quelltext einer Seite
/// setzen — vor `</body>` (sonst ans Ende). `None`, wenn nichts einzusetzen
/// ist.
pub fn einspritzen(html: &str, adresse: &str, f: &Schalter) -> Option<String> {
    if !(adresse.starts_with("http://") || adresse.starts_with("https://")) {
        return None;
    }
    let css = css_fuer(&super::host_von(adresse), f);
    if css.is_empty() {
        return None;
    }
    let mut zusatz = format!("<style {KENNUNG}=\"ruhe\">{css}</style>");
    if f.an(Feature::Ruhemodus) {
        zusatz.push_str(&format!("<script {KENNUNG}=\"ruhe\">{RUHE_JS}</script>"));
    }
    let klein = html.to_ascii_lowercase();
    let mut r = String::with_capacity(html.len() + zusatz.len());
    match klein.rfind("</body") {
        Some(i) => {
            r.push_str(&html[..i]);
            r.push_str(&zusatz);
            r.push_str(&html[i..]);
        }
        None => {
            r.push_str(html);
            r.push_str(&zusatz);
        }
    }
    Some(r)
}

/// Die Netz-Sperre der Stoerer-Liste und die Datenschutz-Signale setzen.
pub fn anwenden(f: &Schalter) {
    if f.an(Feature::Ruhemodus) {
        narsil_net::haken::sperre_setzen(Some(std::sync::Arc::new(|url: &str| {
            narsil_adblock::ruhe::stoerer_gesperrt(url)
        })));
    } else {
        narsil_net::haken::sperre_setzen(None);
    }
    narsil_net::haken::gpc_setzen(f.an(Feature::CookieAutopilot));
}

/// Nach so vielen Millisekunden nach dem Laden sucht der Autopilot erneut.
pub const RUNDEN_MS: [u64; 4] = [0, 600, 2500, 6000];
/// Ab dieser Runde wird ausgeblendet, auch ohne gefundenen Knopf.
pub const AUSBLENDEN_AB: usize = 1;

impl crate::Sitzung {
    /// Feature 11: die Einwilligungsfenster ausblenden — ein `<style>` als
    /// letzter Knoten des Baums (die Nummern aller anderen Knoten bleiben,
    /// also auch Patch, DOM-Stand und Hoerer), dann neu bauen.
    pub fn cookie_banner_ausblenden(&mut self) {
        let adresse = self.browser.adresse().to_string();
        let Some(a) = self.angezeigt.as_mut() else { return };
        let schon = a.html.iter().any(|k| matches!(k, HtmlNode::Element { attributes, .. } if attributes.get(KENNUNG).map(String::as_str) == Some("autopilot")));
        if schon {
            return;
        }
        let mut baum: Vec<HtmlNode> = a.html.as_ref().clone();
        let mut attributes = std::collections::HashMap::new();
        attributes.insert(KENNUNG.to_string(), "autopilot".to_string());
        baum.push(HtmlNode::Element {
            tag: "style".into(),
            attributes,
            children: vec![HtmlNode::Text(cookie_css(&super::host_von(&adresse)))],
            shadow_root: None,
        });
        a.html = std::sync::Arc::new(baum);
        self.seite_neu_bauen();
        self.schmutzig = true;
    }

    /// Feature 11: den Ablehnen-Knopf der Seite klicken. `true`, wenn geklickt.
    pub fn cookie_autopilot(&mut self) -> bool {
        let Some(a) = self.angezeigt.as_ref() else { return false };
        let Some(ord) = ablehnen_knopf(&a.html, &a.patch) else { return false };
        let ziel = a.stand.lock().ok().and_then(|w| w.ids.get(&ord).copied());
        let Some(ziel) = ziel else { return false };
        let vorfahren = self.ereignis_vorfahren(ord);
        self.ereignis_ausloesen(ziel, &vorfahren, "click", true, true, 0.0, 0.0);
        self.meldung = Some(narsil_i18n::tr("Cookie autopilot: consent rejected (only necessary cookies)").into());
        self.schmutzig = true;
        true
    }

    /// Der Takt des Autopiloten (aus `features_tick`).
    pub fn autopilot_tick(&mut self, jetzt: std::time::Instant) {
        let adresse = self.browser.adresse().to_string();
        if !(adresse.starts_with("http://") || adresse.starts_with("https://")) || self.angezeigt.is_none() {
            return;
        }
        let (runde, seit) = match &self.features.autopilot {
            Some((a, r, t)) if *a == adresse => (*r, *t),
            _ => {
                self.features.autopilot = Some((adresse.clone(), 0, jetzt));
                (0, jetzt)
            }
        };
        let Some(&faellig) = RUNDEN_MS.get(runde) else { return };
        if jetzt.duration_since(seit).as_millis() < u128::from(faellig) {
            return;
        }
        let fertig = self.cookie_autopilot();
        if fertig || runde >= AUSBLENDEN_AB {
            self.cookie_banner_ausblenden();
        }
        self.features.autopilot = Some((adresse, if fertig { RUNDEN_MS.len() } else { runde + 1 }, seit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schalter(cookie: bool, ruhe: bool) -> Schalter {
        let mut s = Schalter::default();
        s.setzen(Feature::CookieAutopilot, cookie);
        s.setzen(Feature::Ruhemodus, ruhe);
        s
    }

    #[test]
    fn ohne_funktion_bleibt_die_seite_unberuehrt() {
        assert_eq!(einspritzen("<html><body>x</body></html>", "https://a.example/", &schalter(false, false)), None);
        assert_eq!(einspritzen("<p>x</p>", "file:///x.html", &schalter(true, true)), None);
    }

    #[test]
    fn blatt_steht_vor_dem_body_ende() {
        let s = einspritzen("<html><body><p>Inhalt</p></BODY></html>", "https://a.example/", &schalter(true, true))
            .expect("eingesetzt");
        let i = s.find("<style").expect("style");
        assert!(i < s.find("</BODY>").expect("ende"));
        assert!(s.contains("intercom-container"));
        assert!(s.contains("display:none") || s.contains("display: none"), "{s}");
        assert!(s.contains("Notification.requestPermission"));
        // Die Cookie-Fenster blendet erst der Autopilot nach dem Klick aus.
        assert!(!s.contains("onetrust-banner-sdk"), "{s}");
        assert!(cookie_css("a.example").contains("onetrust-banner-sdk"));
        assert_eq!(einspritzen("<p>x</p>", "https://a.example/", &schalter(true, false)), None);
        let t = einspritzen("<p>ohne body</p>", "https://a.example/", &schalter(false, true)).expect("ruhe");
        assert!(t.ends_with("</script>"));
    }

    #[test]
    fn ablehnen_wird_erkannt_akzeptieren_nie() {
        for t in ["Alle ablehnen", "Nur notwendige Cookies", "Reject all", "  Decline  ", "Nur essenzielle Cookies akzeptieren"] {
            assert!(lehnt_ab(t), "{t}");
        }
        for t in ["Alle akzeptieren", "Accept all", "OK", "Einstellungen", "Cookies zulassen", ""] {
            assert!(!lehnt_ab(t), "{t}");
        }
        let html = narsil_page::html_lesen(
            "<div class=\"news\"><button>Ablehnen</button></div>\
             <div id=\"cookie-layer\"><button>Alle akzeptieren</button><a href=\"#\">Nur notwendige</a></div>",
        );
        let ord = ablehnen_knopf(&html, &narsil_page::Patch::default()).expect("Knopf");
        let st = super::super::baum::stellen(&html);
        assert_eq!(super::super::baum::text_von(&st, ord, None), "Nur notwendige", "nicht der Knopf ausserhalb des Banners");
        let ot = narsil_page::html_lesen("<div id=\"x\"><button id=\"onetrust-reject-all-handler\">Alle ablehnen</button></div>");
        assert!(ablehnen_knopf(&ot, &narsil_page::Patch::default()).is_some());
        assert_eq!(ablehnen_knopf(&narsil_page::html_lesen("<p>keine Banner</p>"), &narsil_page::Patch::default()), None);
    }

    /// Ende zu Ende: der Autopilot klickt in der eigenen Engine wirklich
    /// „Alle ablehnen" (der `onclick` der Seite laeuft) und nie „Alle
    /// akzeptieren"; das Banner ist ausgeblendet.
    #[test]
    fn der_autopilot_klickt_ablehnen_in_der_eigenen_engine() {
        let seite = r#"<html><body>
<div id="cookie-banner"><p>Wir nutzen Cookies.</p>
<button id="ja" onclick="document.getElementById('erg').textContent='AKZEPTIERT'">Alle akzeptieren</button>
<button id="nein" onclick="document.getElementById('erg').textContent='ABGELEHNT'">Alle ablehnen</button>
</div><p id="erg">offen</p></body></html>"#;
        let mut s = crate::Sitzung::neu(800, 600);
        s.einstellungen.features.setzen(Feature::CookieAutopilot, true);
        let a = s.aus_html(seite, "https://a.example/", 800, 600).expect("Seite");
        s.browser.anwenden(&narsil_browser::Navigation::To("https://a.example/".into()));
        s.angezeigt = Some(a);
        let sichtbar = |s: &crate::Sitzung| {
            s.angezeigt.as_ref().expect("Seite").seite.document.nodes().iter()
                .any(|n| n.text.as_deref().is_some_and(|t| t.contains("Wir nutzen")))
        };
        assert!(sichtbar(&s), "das Banner ist vor dem Autopiloten da");
        // Runde 0 sofort: klicken und ausblenden.
        s.autopilot_tick(std::time::Instant::now());
        let a = s.angezeigt.as_ref().expect("Seite");
        let st = super::super::baum::stellen(&a.html);
        let alles = super::super::baum::text_von(&st, 0, Some(&a.patch));
        assert!(alles.contains("ABGELEHNT"), "nicht abgelehnt: {alles} / {:?}", a.konsole);
        assert!(!alles.contains("AKZEPTIERT"), "akzeptiert!");
        assert!(!sichtbar(&s), "das Banner steht noch im Bild");
    }
}
