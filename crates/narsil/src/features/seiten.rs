//! Release 08.10.2026: DIE EINGEBAUTEN SEITEN der Features (`narsil://…`).
//!
//! Die Seiten werden von der eigenen Engine gezeichnet; Aenderungen laufen
//! ueber gewoehnliche GET-Formulare und Links auf dieselbe Adresse
//! (`narsil://kopfregeln?aktion=neu&…`). Jede Aktion ist so gebaut, dass ein
//! erneutes Laden nichts verdoppelt (gleiche Regel ersetzt, Loeschen per id).

use super::{abfrage, html, Feature};
use narsil_i18n::{tr, trf};

// Release 09.10.2026: Formulare als Flex-Zeilen (Beschriftung, Feld, Knopf
// nebeneinander, umbrechend) — gueltiges CSS, das auch nach einer Korrektur
// der Inline-Formularelemente in der Engine dasselbe Bild ergibt. Checkboxen
// ohne Polster und Rahmen (sie waren schwarze Quadrate).
const STIL: &str = "body{font-family:sans-serif;margin:0;background:#f6f7f9;color:#1c1c24}\
main{max-width:920px;margin:0 auto;padding:24px 28px}\
h1{font-size:24px;margin:8px 0 4px}h2{font-size:18px;margin:20px 0 8px}h3{font-size:15px;margin:16px 0 4px}\
.unter{color:#606070;margin:0 0 16px}\
.karte{background:#ffffff;border:1px solid #d8d8e0;border-radius:10px;padding:14px 18px;margin:12px 0}\
.hinweis{background:#e8f1fd;border:1px solid #9cc1f2;border-radius:8px;padding:10px 14px;margin:12px 0}\
.aus{background:#fdf1e6;border:1px solid #f0b37e;border-radius:8px;padding:10px 14px;margin:12px 0}\
table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:6px 8px;border-bottom:1px solid #e4e4ea;vertical-align:top}\
th{font-size:13px;color:#606070}a{color:#1a63c8}small{color:#606070}\
input:not([type=checkbox]):not([type=radio]),select{padding:4px 6px;border:1px solid #b8b8c4;border-radius:5px;background:#ffffff}\
input[type=checkbox],input[type=radio]{margin:0;accent-color:#1a63c8}input[type=hidden]{display:none}\
form{display:flex;flex-wrap:wrap;align-items:center;gap:8px;margin:8px 0}\
form p{flex-basis:100%;margin:0;display:flex;flex-wrap:wrap;align-items:center;gap:8px}\
label{display:inline-flex;align-items:center;gap:6px}\
button{padding:5px 12px;border:1px solid #1a63c8;background:#1a63c8;color:#ffffff;border-radius:6px}\
pre{background:#ffffff;border:1px solid #d8d8e0;border-radius:8px;padding:14px;white-space:pre-wrap}\
nav a{margin-right:14px}";

fn rahmen(titel: &str, unter: &str, f: Option<Feature>, meldung: &str, inhalt: &str) -> String {
    let aus = match f {
        Some(f) if !schalter().an(f) => format!(
            "<div class=\"aus\">{} <a href=\"about:einstellungen#features\">{}</a>.</div>",
            html(&trf!("The feature “{}” is switched off. Switch it on under", f.name())),
            html(tr("Settings › Features"))
        ),
        _ => String::new(),
    };
    let meldung = if meldung.is_empty() { String::new() } else { format!("<div class=\"hinweis\">{}</div>", html(meldung)) };
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{t}</title><style>{STIL}</style></head><body><main>\
         <nav><a href=\"about:einstellungen#features\">‹ {zurueck}</a></nav>\
         <h1>{t}</h1><p class=\"unter\">{u}</p>{aus}{meldung}<div class=\"karte\">{inhalt}</div></main></body></html>",
        t = html(titel),
        u = html(unter),
        zurueck = html(tr("Settings › Features")),
    )
}

/// Die gespeicherten Schalter (die Seiten zeigen, ob die Funktion an ist).
fn schalter() -> super::Schalter {
    narsil_chrome::einstellungen::lesen_von(&narsil_chrome::einstellungen::pfad())
        .map(|e| e.features)
        .unwrap_or_default()
}

fn wert<'a>(q: &'a [(String, String)], k: &str) -> &'a str {
    q.iter().find(|(a, _)| a == k).map_or("", |(_, v)| v.as_str())
}

/// Die Seite zu einer Adresse `narsil://…` — `None`, wenn es keine ist.
pub fn interne_seite(adresse: &str) -> Option<String> {
    let rest = adresse.strip_prefix("narsil://")?;
    let name = rest.split(['?', '#']).next().unwrap_or("");
    let q = abfrage(adresse);
    let aktion = wert(&q, "aktion");
    let jetzt = super::jetzt_s();
    Some(match name {
        "kopfregeln" => {
            let mut meldung = String::new();
            let mut regeln = super::kopfregeln::laden();
            match aktion {
                "neu" => match super::kopfregeln::aus_formular(wert(&q, "domain"), wert(&q, "richtung"), wert(&q, "name"), wert(&q, "art"), wert(&q, "wert")) {
                    Ok(r) => {
                        regeln.retain(|x| !(x.domain == r.domain && x.richtung == r.richtung && x.name.eq_ignore_ascii_case(&r.name)));
                        regeln.push(r);
                        meldung = match super::kopfregeln::speichern(&regeln) {
                            Ok(()) => tr("Rule saved.").into(),
                            Err(e) => trf!("Not saved: {}", e),
                        };
                    }
                    Err(e) => meldung = e,
                },
                "weg" => {
                    if let Ok(nr) = wert(&q, "nr").parse::<usize>()
                        && nr < regeln.len()
                    {
                        regeln.remove(nr);
                        let _ = super::kopfregeln::speichern(&regeln);
                        meldung = tr("Rule removed.").into();
                    }
                }
                _ => {}
            }
            super::kopfregeln::anwenden(schalter().an(Feature::Kopfregeln));
            let mut h = format!(
                "<form action=\"narsil://kopfregeln\" method=\"get\"><input type=\"hidden\" name=\"aktion\" value=\"neu\">\
                 {} <input name=\"domain\" size=\"16\" placeholder=\"{}\"> \
                 <select name=\"richtung\"><option value=\"anfrage\">{}</option><option value=\"antwort\">{}</option></select> \
                 {} <input name=\"name\" size=\"16\" placeholder=\"User-Agent\"> \
                 <select name=\"art\"><option value=\"setzen\">{}</option><option value=\"entfernen\">{}</option></select> \
                 {} <input name=\"wert\" size=\"22\"> <button type=\"submit\">{}</button></form>\
                 <p><small>{}</small></p>",
                html(tr("Domain")),
                html(tr("example.com or *")),
                html(tr("Request")),
                html(tr("Response")),
                html(tr("Header")),
                html(tr("set")),
                html(tr("remove")),
                html(tr("Value")),
                html(tr("Add")),
                html(tr("Examples: Accept-Language “en-US,en” for one site, remove Referer, a custom X header. On purpose there are no presets for getting around paywalls.")),
            );
            if regeln.is_empty() {
                h.push_str(&format!("<p>{}</p>", html(tr("No rules."))));
            } else {
                h.push_str(&format!(
                    "<table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th></th></tr>",
                    html(tr("Domain")),
                    html(tr("Direction")),
                    html(tr("Header")),
                    html(tr("Effect"))
                ));
                for (i, r) in regeln.iter().enumerate() {
                    h.push_str(&format!(
                        "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><a href=\"narsil://kopfregeln?aktion=weg&amp;nr={i}\">{}</a></td></tr>",
                        html(&r.domain),
                        if r.richtung == narsil_net::haken::Richtung::Antwort { tr("Response") } else { tr("Request") },
                        html(&r.name),
                        match &r.wert { Some(w) => trf!("set: {}", html(w)), None => tr("remove").into() },
                        html(tr("Remove"))
                    ));
                }
                h.push_str("</table>");
            }
            rahmen(tr("Header rules"), tr("Request and response headers per domain — they apply to every request and redirect."), Some(Feature::Kopfregeln), &meldung, &h)
        }
        "umleitungen" => {
            let mut r = super::url::Regeln::laden();
            let mut meldung = String::new();
            if aktion == "speichern" {
                r.tracking = wert(&q, "tracking") == "1";
                r.kurzlinks = wert(&q, "kurzlinks") == "1";
                r.reddit = wert(&q, "reddit") == "1";
                r.youtube = wert(&q, "youtube") == "1";
                for (feld, ziel) in [("reddit_instanz", &mut r.reddit_instanz), ("youtube_instanz", &mut r.youtube_instanz)] {
                    let v = wert(&q, feld).trim();
                    if super::url::instanz_gueltig(v) {
                        *ziel = v.trim_end_matches('/').to_string();
                    } else if !v.is_empty() {
                        meldung = trf!("“{}” is not a valid instance address (https://…)", v);
                    }
                }
                r.eigene_parameter = wert(&q, "parameter")
                    .split(',')
                    .map(|p| p.trim().to_ascii_lowercase())
                    .filter(|p| !p.is_empty() && p.len() <= 64)
                    .take(64)
                    .collect();
                if meldung.is_empty() {
                    meldung = match r.speichern() {
                        Ok(()) => tr("Saved.").into(),
                        Err(e) => trf!("Not saved: {}", e),
                    };
                }
            }
            let test = wert(&q, "pruefen");
            let ergebnis = if test.is_empty() {
                String::new()
            } else {
                let sauber = super::url::reinigen(test, &r);
                format!("<p>{} <b>{}</b></p>", html(tr("Cleaned without network:")), html(&sauber))
            };
            let haken = |b: bool| if b { " checked" } else { "" };
            let h = format!(
                "<form action=\"narsil://umleitungen\" method=\"get\"><input type=\"hidden\" name=\"aktion\" value=\"speichern\">\
                 <p><label><input type=\"checkbox\" name=\"tracking\" value=\"1\"{}> {}</label></p>\
                 <p><label><input type=\"checkbox\" name=\"kurzlinks\" value=\"1\"{}> {}</label></p>\
                 <p><label><input type=\"checkbox\" name=\"reddit\" value=\"1\"{}> Reddit → Redlib</label> {} <input name=\"reddit_instanz\" size=\"32\" value=\"{}\"></p>\
                 <p><label><input type=\"checkbox\" name=\"youtube\" value=\"1\"{}> YouTube → Invidious</label> {} <input name=\"youtube_instanz\" size=\"32\" value=\"{}\"></p>\
                 <p>{} <input name=\"parameter\" size=\"40\" value=\"{}\"></p>\
                 <button type=\"submit\">{}</button></form>\
                 <h3>{}</h3><form action=\"narsil://umleitungen\" method=\"get\"><input name=\"pruefen\" size=\"60\" value=\"{}\"> <button type=\"submit\">{}</button></form>{}",
                haken(r.tracking),
                html(tr("Remove tracking parameters (utm_*, fbclid, gclid …)")),
                haken(r.kurzlinks),
                html(tr("Resolve short links with HEAD (bit.ly, t.co …)")),
                haken(r.reddit),
                html(tr("Instance")),
                html(&r.reddit_instanz),
                haken(r.youtube),
                html(tr("Instance")),
                html(&r.youtube_instanz),
                html(tr("Custom parameters (comma, * = prefix)")),
                html(&r.eigene_parameter.join(",")),
                html(tr("Save")),
                html(tr("Check an address")),
                html(test),
                html(tr("Check")),
                ergebnis
            );
            rahmen(tr("Link cleaner"), tr("Clean and rewrite addresses before loading and copying."), Some(Feature::UrlReinigung), &meldung, &h)
        }
        "beobachter" => {
            let mut meldung = String::new();
            match aktion {
                "weg" => {
                    if let Ok(id) = wert(&q, "id").parse::<u64>() {
                        let _ = super::beobachter::entfernen(id);
                        meldung = tr("Removed.").into();
                    }
                }
                "takt" => {
                    if let Ok(m) = wert(&q, "minuten").trim().parse::<u64>() {
                        let _ = super::beobachter::takt_setzen(m);
                        meldung = trf!("Background check every {} minutes.", super::beobachter::takt_minuten());
                    }
                }
                "jetzt" => {
                    super::beobachter::sofort_pruefen();
                    meldung = tr("The check runs in the background — changes appear as a notice.").into();
                }
                _ => {}
            }
            rahmen(tr("Element watcher"), tr("Watched values — right-click a value on a page → “Watch element”."), Some(Feature::Beobachter), &meldung, &super::beobachter::uebersicht())
        }
        "formulare" => {
            let mut meldung = String::new();
            match aktion {
                "weg" => {
                    if let Ok(nr) = wert(&q, "nr").parse::<usize>() {
                        let _ = super::formulare::entfernen(nr);
                        meldung = tr("Record deleted.").into();
                    }
                }
                "alle_weg" => {
                    super::formulare::alles_loeschen();
                    meldung = tr("All form data and the key are deleted.").into();
                }
                _ => {}
            }
            rahmen(tr("Form memory"), tr("Form data remembered in encrypted form."), Some(Feature::FormularGedaechtnis), &meldung, &super::formulare::uebersicht())
        }
        "fristen/vorlage" => {
            let id: u64 = wert(&q, "id").parse().unwrap_or(0);
            match super::fristen::laden().into_iter().find(|f| f.id == id) {
                Some(f) => rahmen(tr("Cancellation template"), tr("To copy into an e-mail or a letter."), None, "", &super::fristen::vorlage(&f)),
                None => rahmen(
                    tr("Cancellation template"),
                    "",
                    None,
                    tr("This entry no longer exists."),
                    &format!("<p><a href=\"narsil://fristen\">{}</a></p>", html(tr("To the list"))),
                ),
            }
        }
        "fristen" => {
            let mut meldung = String::new();
            let mut l = super::fristen::laden();
            match aktion {
                "weg" | "erledigt" => {
                    if let Ok(id) = wert(&q, "id").parse::<u64>() {
                        if aktion == "weg" {
                            l.retain(|f| f.id != id);
                        } else if let Some(f) = l.iter_mut().find(|f| f.id == id) {
                            f.erledigt = true;
                        }
                        let _ = super::fristen::speichern(&l);
                        meldung = tr("Saved.").into();
                    }
                }
                "neu" => match super::datum_lesen(wert(&q, "ende")) {
                    Some(ende) if !wert(&q, "dienst").trim().is_empty() => {
                        let e = super::fristen::Erkannt { probe: wert(&q, "probe") == "1", ende, preis: String::new() };
                        let dienst = wert(&q, "dienst").trim().to_string();
                        if !l.iter().any(|f| f.dienst == dienst && f.ende == ende) {
                            let id = l.iter().map(|f| f.id).max().unwrap_or(0) + 1;
                            l.push(super::fristen::Frist { id, dienst, adresse: String::new(), beginn: jetzt, ende, probe: e.probe, preis: String::new(), erledigt: false });
                            let _ = super::fristen::speichern(&l);
                        }
                        meldung = tr("Deadline added.").into();
                    }
                    _ => meldung = tr("Please enter the service and the end date (YYYY-MM-DD).").into(),
                },
                _ => {}
            }
            rahmen(tr("Subscription & deadline assistant"), tr("Detected subscriptions and trials with their cancellation deadline."), Some(Feature::Fristen), &meldung, &super::fristen::uebersicht(jetzt))
        }
        "belege" => {
            let mut meldung = String::new();
            match aktion {
                "weg" => {
                    if let Ok(id) = wert(&q, "id").parse::<u64>() {
                        let _ = super::belege::entfernen(id);
                        meldung = tr("Receipt deleted.").into();
                    }
                }
                "garantie" => {
                    if let (Ok(id), Ok(j)) = (wert(&q, "id").parse::<u64>(), wert(&q, "jahre").trim().parse::<i64>()) {
                        let _ = super::belege::garantie_setzen(id, j);
                        meldung = trf!("Warranty set to {} years.", j);
                    }
                }
                _ => {}
            }
            rahmen(tr("Receipt & warranty vault"), tr("Detected purchases, stored confirmations and invoices."), Some(Feature::Belege), &meldung, &super::belege::uebersicht(jetzt))
        }
        "schnappschuss" => rahmen(
            tr("Session snapshot"),
            tr("DOM state, input and scroll position of all tabs — to restore them after a crash."),
            Some(Feature::Schnappschuss),
            if aktion == "fertig" { tr("Restored.") } else { "" },
            &super::schnappschuss::uebersicht(),
        ),
        // Release 08.10.2026: BILD-DIAET — Stand je Tab und Ausnahmen je Domain.
        "bild-diaet" => {
            use crate::bild_diaet as bd;
            let mut liste = bd::ausnahmen();
            let mut meldung = String::new();
            match aktion {
                "neu" => match bd::domain_aus(wert(&q, "domain")) {
                    Some(d) => {
                        if !liste.contains(&d) {
                            liste.push(d.clone());
                        }
                        meldung = match bd::ausnahmen_speichern(&liste) {
                            Ok(()) => trf!("“{}” stays untouched from now on (the next time the page loads).", d),
                            Err(e) => trf!("Not saved: {}", e),
                        };
                    }
                    None => meldung = tr("That is not a domain (example: spiegel.de).").into(),
                },
                "weg" => {
                    if let Ok(nr) = wert(&q, "nr").parse::<usize>()
                        && nr < liste.len()
                    {
                        let d = liste.remove(nr);
                        let _ = bd::ausnahmen_speichern(&liste);
                        meldung = trf!("“{}” is no longer on the exception list.", d);
                    }
                }
                _ => {}
            }
            let b = bd::bericht();
            let jetzt_gespart: usize = b.tabs.values().map(bd::Seitenstand::gespart).sum();
            let aufgeschoben: usize = b.tabs.values().map(|s| s.aufgeschoben).sum();
            let mut h = format!(
                "<h2>{}</h2><table>\
                 <tr><td>{}</td><td><b>{}</b></td></tr>\
                 <tr><td>{}</td><td>{}</td></tr>\
                 <tr><td>{}</td><td>{}</td></tr>\
                 <tr><td>{}</td><td>{} / {} / {}</td></tr>\
                 <tr><td>{}</td><td>{aufgeschoben}</td></tr></table>\
                 <p><small>{}</small></p>",
                html(tr("This session")),
                html(tr("Memory saved now (all tabs)")),
                bd::mb(jetzt_gespart as u64),
                html(tr("Released since the start")),
                bd::mb(b.zaehler.freigegeben_summe),
                html(tr("Largest saving on one page")),
                bd::mb(b.zaehler.gespart_spitze as u64),
                html(tr("Downscaled / dropped / decoded again")),
                b.zaehler.verkleinert,
                b.zaehler.verworfen,
                b.zaehler.nachdekodiert,
                html(tr("Images loaded later (not fetched yet)")),
                html(tr("The network saving counts images, not bytes: what is never loaded has no known size.")),
            );
            h.push_str(&format!("<h2>{}</h2>", html(tr("Per tab"))));
            if b.tabs.is_empty() {
                h.push_str(&format!("<p>{}</p>", html(tr("No page with images yet."))));
            } else {
                h.push_str(&format!(
                    "<table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th></th></tr>",
                    html(tr("Page")),
                    html(tr("Images")),
                    html(tr("downscaled / dropped")),
                    html(tr("Memory (without diet)")),
                    html(tr("later"))
                ));
                let mut tabs: Vec<_> = b.tabs.iter().collect();
                tabs.sort_by_key(|(t, _)| **t);
                for (_, s) in tabs {
                    let domain = bd::domain_aus(&s.adresse).unwrap_or_default();
                    let knopf = if s.ausnahme {
                        format!("<small>{}</small>", html(tr("Exception")))
                    } else if domain.is_empty() {
                        String::new()
                    } else {
                        format!("<a href=\"narsil://bild-diaet?aktion=neu&amp;domain={}\">{}</a>", html(&domain), html(tr("Exception")))
                    };
                    // Lange Adressen gekuerzt (Anfang … Ende), damit die
                    // Tabelle in die Karte passt.
                    let zeichen: Vec<char> = s.adresse.chars().collect();
                    let kurz: String = if zeichen.len() > 46 {
                        let anfang: String = zeichen[..20].iter().collect();
                        let ende: String = zeichen[zeichen.len() - 22..].iter().collect();
                        format!("{anfang}…{ende}")
                    } else {
                        s.adresse.clone()
                    };
                    h.push_str(&format!(
                        "<tr><td>{}</td><td>{}</td><td>{} / {}</td><td>{} ({})</td><td>{}</td><td>{knopf}</td></tr>",
                        html(&kurz),
                        s.bilder,
                        s.verkleinert,
                        s.verworfen,
                        bd::mb(s.puffer_bytes as u64),
                        bd::mb(s.natur_bytes as u64),
                        s.aufgeschoben,
                    ));
                }
                h.push_str("</table>");
            }
            h.push_str(&format!(
                "<h2>{}</h2><p><small>{}</small></p>\
                 <form action=\"narsil://bild-diaet\" method=\"get\"><input type=\"hidden\" name=\"aktion\" value=\"neu\">\
                 {} <input name=\"domain\" size=\"24\" placeholder=\"spiegel.de\"> <button type=\"submit\">{}</button></form>",
                html(tr("Exceptions per domain")),
                html(tr("On these sites (and their subdomains) all images stay at full size and are loaded as without the image diet.")),
                html(tr("Domain")),
                html(tr("Add")),
            ));
            if liste.is_empty() {
                h.push_str(&format!("<p>{}</p>", html(tr("No exceptions."))));
            } else {
                h.push_str("<table>");
                for (i, d) in liste.iter().enumerate() {
                    h.push_str(&format!(
                        "<tr><td>{}</td><td><a href=\"narsil://bild-diaet?aktion=weg&amp;nr={i}\">{}</a></td></tr>",
                        html(d),
                        html(tr("Remove"))
                    ));
                }
                h.push_str("</table>");
            }
            rahmen(
                tr("Image diet"),
                tr("Images at display size, pixels only in the viewport, load later — state of this session."),
                Some(Feature::BildDiaet),
                &meldung,
                &h,
            )
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alle_seiten_gibt_es_und_sie_escapen() {
        let dir = std::env::temp_dir().join(format!("narsil-test-seiten-{}-{:?}", std::process::id(), std::thread::current().id()));
        super::super::ordner_fuer_faden(Some(dir.clone()));
        for n in ["kopfregeln", "umleitungen", "beobachter", "formulare", "fristen", "belege", "schnappschuss", "fristen/vorlage?id=1", "bild-diaet"] {
            let s = interne_seite(&format!("narsil://{n}")).unwrap_or_else(|| panic!("{n}"));
            assert!(s.contains("<h1>") && s.contains("Settings › Features"), "{n}");
        }
        assert_eq!(interne_seite("narsil://gibtsnicht"), None);
        assert_eq!(interne_seite("narsil://start"), None);
        // Eine Regel ueber das Formular anlegen, dann wieder entfernen.
        let s = interne_seite("narsil://kopfregeln?aktion=neu&domain=beispiel.de&richtung=anfrage&name=X-Test&art=setzen&wert=%3Cb%3E").expect("seite");
        assert!(s.contains("Rule saved"), "{s}");
        assert!(s.contains("&lt;b&gt;") && !s.contains("<b>X"), "nicht escaped");
        assert_eq!(super::super::kopfregeln::laden().len(), 1);
        // Dieselbe Regel noch einmal (Neuladen der Adresse): keine zweite.
        interne_seite("narsil://kopfregeln?aktion=neu&domain=beispiel.de&richtung=anfrage&name=X-Test&art=setzen&wert=2").expect("seite");
        assert_eq!(super::super::kopfregeln::laden().len(), 1);
        interne_seite("narsil://kopfregeln?aktion=weg&nr=0").expect("seite");
        assert!(super::super::kopfregeln::laden().is_empty());
        narsil_net::haken::kopfregeln_setzen(Vec::new());
        // Umleitungen speichern und pruefen.
        let s = interne_seite("narsil://umleitungen?aktion=speichern&tracking=1&reddit=1&reddit_instanz=https%3A%2F%2Fredlib.example").expect("um");
        assert!(s.contains("Saved"), "{s}");
        let s = interne_seite("narsil://umleitungen?pruefen=https%3A%2F%2Fwww.reddit.com%2Fr%2Frust%3Futm_source%3Dx").expect("pruefen");
        assert!(s.contains("https://redlib.example/r/rust"), "{s}");
        // Eine Frist von Hand.
        let s = interne_seite("narsil://fristen?aktion=neu&dienst=Zeitung&ende=01.12.2026&probe=1").expect("frist");
        assert!(s.contains("Zeitung") && s.contains("2026-12-01"), "{s}");
        super::super::ordner_fuer_faden(None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
