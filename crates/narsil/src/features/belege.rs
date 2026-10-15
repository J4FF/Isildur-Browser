//! Release 08.10.2026: FEATURE 10 — BELEG- & GARANTIE-TRESOR.
//!
//! Eine Seite, die eine **Bestellung bestaetigt** (Bestellbestaetigung,
//! „Vielen Dank fuer Ihre Bestellung", Bestellnummer, Rechnung …), wird
//! erkannt; Shop, Bestellnummer, Datum, Betrag und Garantiedauer kommen in
//! `<profil>/features/belege.tsv`. Der Quelltext der Seite wird als Beleg
//! abgelegt, eine verlinkte PDF-Rechnung im Hintergrund heruntergeladen.
//! 30 Tage vor Ende der Garantie (Vorgabe: 2 Jahre gesetzliche
//! Gewaehrleistung) erinnert Narsil.

use super::fristen::{daten, normal, preis};
use super::{felder, lesen_text, ordner, schreiben, zeile};

/// Wie viele Tage vor dem Garantie-Ende erinnert wird.
pub const VORLAUF_TAGE: i64 = 30;
/// M3: hoechstens so viele Belege.
pub const MAX_BELEGE: usize = 500;
/// M4: die groesste Rechnung, die geladen wird.
pub const MAX_PDF: usize = 10 * 1024 * 1024;

/// Ein gespeicherter Beleg.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beleg {
    pub id: u64,
    pub shop: String,
    pub adresse: String,
    pub bestellnummer: String,
    pub datum: i64,
    pub betrag: String,
    pub garantie_bis: i64,
    pub seite: String,
    pub rechnung: String,
}

fn pfad() -> std::path::PathBuf {
    ordner().join("belege.tsv")
}

fn ablage() -> std::path::PathBuf {
    ordner().join("belege")
}

pub fn laden() -> Vec<Beleg> {
    lesen_text(&pfad())
        .lines()
        .take(MAX_BELEGE)
        .filter_map(|z| {
            let f = felder(z);
            if f.len() < 9 {
                return None;
            }
            Some(Beleg {
                id: f[0].parse().ok()?,
                shop: f[1].clone(),
                adresse: f[2].clone(),
                bestellnummer: f[3].clone(),
                datum: f[4].parse().ok()?,
                betrag: f[5].clone(),
                garantie_bis: f[6].parse().ok()?,
                seite: f[7].clone(),
                rechnung: f[8].clone(),
            })
        })
        .collect()
}

pub fn speichern(l: &[Beleg]) -> Result<(), String> {
    let t: Vec<String> = l
        .iter()
        .take(MAX_BELEGE)
        .map(|b| {
            zeile(&[
                &b.id.to_string(),
                &b.shop,
                &b.adresse,
                &b.bestellnummer,
                &b.datum.to_string(),
                &b.betrag,
                &b.garantie_bis.to_string(),
                &b.seite,
                &b.rechnung,
            ])
        })
        .collect();
    schreiben(&pfad(), t.join("\n").as_bytes())
}

/// Das Ergebnis der Erkennung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Erkannt {
    pub bestellnummer: String,
    pub datum: i64,
    pub betrag: String,
    pub garantie_jahre: i64,
}

const BESTELLT: &[&str] = &[
    "bestellbestaetigung", "vielen dank fuer ihre bestellung", "danke fuer deine bestellung",
    "danke fuer ihre bestellung", "ihre bestellung ist eingegangen", "bestellung wurde aufgegeben",
    "bestellung erfolgreich", "kaufbestaetigung", "zahlungsbestaetigung", "order confirmation",
    "thank you for your order", "thanks for your order", "your order has been placed", "order confirmed",
    "auftragsbestaetigung", "wir haben ihre bestellung erhalten",
];

/// Die Bestellnummer hinter einer Marke („Bestellnummer: 304-1234567").
pub fn bestellnummer(t: &str) -> Option<String> {
    for marke in ["bestellnummer", "bestell-nr", "bestellnr", "auftragsnummer", "order number", "order no", "order #", "order id", "rechnungsnummer"] {
        if let Some(i) = t.find(marke) {
            let rest = t[i + marke.len()..].trim_start_matches(|c: char| c == ':' || c == '.' || c == '#' || c.is_whitespace());
            let nr: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '/').take(40).collect();
            if nr.chars().filter(|c| c.is_ascii_digit()).count() >= 3 {
                return Some(nr.to_uppercase());
            }
        }
    }
    None
}

/// Die Garantiedauer in Jahren („3 Jahre Garantie", „Garantie: 5 Jahre").
pub fn garantie_jahre(t: &str) -> Option<i64> {
    for (i, _) in t.match_indices("garantie").chain(t.match_indices("warranty")) {
        let fenster: String = t[i.saturating_sub(25)..].chars().take(60).collect();
        let worte: Vec<&str> = fenster.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        for (j, w) in worte.iter().enumerate() {
            if let Ok(n) = w.parse::<i64>()
                && (1..=30).contains(&n)
                && worte.get(j + 1).is_some_and(|e| e.starts_with("jahr") || e.starts_with("year"))
            {
                return Some(n);
            }
        }
    }
    None
}

/// Die Heuristik: ist die Seite eine Bestell-/Kaufbestaetigung?
pub fn erkennen(titel: &str, text: &str, jetzt: i64) -> Option<Erkannt> {
    let t = normal(&format!("{titel} {text}"));
    if !BESTELLT.iter().any(|w| t.contains(w)) {
        return None;
    }
    let nr = bestellnummer(&t);
    let betrag = preis(&t).unwrap_or_default();
    // Ohne Bestellnummer und ohne Betrag ist es eher eine Info-Seite.
    if nr.is_none() && betrag.is_empty() {
        return None;
    }
    let datum = daten(&t).into_iter().find(|d| (*d - jetzt).abs() < 60 * 86_400).unwrap_or(jetzt);
    Some(Erkannt {
        bestellnummer: nr.unwrap_or_default(),
        datum,
        betrag,
        garantie_jahre: garantie_jahre(&t).unwrap_or(2),
    })
}

/// Links auf Rechnungen (PDF oder „Rechnung"/„Invoice" im Text).
pub fn rechnungs_links(baum: &[narsil_fmt_html::HtmlNode], basis: &str) -> Vec<String> {
    use narsil_fmt_html::HtmlNode;
    let mut raus = Vec::new();
    fn lauf(k: &[HtmlNode], basis: &str, raus: &mut Vec<String>) {
        for n in k {
            if let HtmlNode::Element { tag, attributes, children, .. } = n {
                if tag.eq_ignore_ascii_case("a")
                    && let Some(href) = attributes.get("href")
                {
                    let text = normal(&super::baum::text_des_knotens(n));
                    let h = href.to_ascii_lowercase();
                    let ist_pdf = h.split(['?', '#']).next().is_some_and(|p| p.ends_with(".pdf"));
                    if ist_pdf || ((text.contains("rechnung") || text.contains("invoice")) && h.contains("pdf")) {
                        let ziel = narsil_page::loese_link(href, basis);
                        if ziel.starts_with("http") && !raus.contains(&ziel) && raus.len() < 3 {
                            raus.push(ziel);
                        }
                    }
                }
                lauf(children, basis, raus);
            }
        }
    }
    lauf(baum, basis, &mut raus);
    raus
}

/// Einen Beleg eintragen; legt die Seite ab und laedt eine Rechnung im
/// Hintergrund. Dieselbe Bestellnummer wird nicht doppelt eingetragen.
pub fn eintragen(
    adresse: &str,
    titel: &str,
    e: &Erkannt,
    rohtext: Option<&str>,
    baum: &[narsil_fmt_html::HtmlNode],
) -> Result<Beleg, String> {
    let mut l = laden();
    let host = super::host_von(adresse);
    if let Some(b) = l.iter().find(|b| {
        super::host_von(&b.adresse) == host
            && ((!e.bestellnummer.is_empty() && b.bestellnummer == e.bestellnummer)
                || (e.bestellnummer.is_empty() && b.adresse == adresse))
    }) {
        return Ok(b.clone());
    }
    let id = l.iter().map(|b| b.id).max().unwrap_or(0) + 1;
    let seite = match rohtext {
        Some(t) => {
            let p = ablage().join(format!("beleg_{id}.html"));
            schreiben(&p, t.as_bytes())?;
            p.display().to_string()
        }
        None => String::new(),
    };
    let (j, m, t) = super::datum_aus_tagen(e.datum.div_euclid(86_400));
    let garantie_bis = super::tage_aus_datum(j + e.garantie_jahre, m, t.min(28)) * 86_400;
    let shop = {
        let h = host.strip_prefix("www.").unwrap_or(&host).to_string();
        let t = titel.split(['|', '–', '-']).map(str::trim).filter(|x| !x.is_empty()).last().unwrap_or("");
        if t.is_empty() || t.chars().count() > 30 || normal(t).contains("bestell") { h } else { format!("{t} ({h})") }
    };
    let mut b = Beleg {
        id,
        shop,
        adresse: adresse.to_string(),
        bestellnummer: e.bestellnummer.clone(),
        datum: e.datum,
        betrag: e.betrag.clone(),
        garantie_bis,
        seite,
        rechnung: String::new(),
    };
    if let Some(link) = rechnungs_links(baum, adresse).into_iter().next() {
        let ziel = ablage().join(format!("beleg_{id}_rechnung.pdf"));
        b.rechnung = ziel.display().to_string();
        rechnung_laden(link, ziel);
    }
    l.push(b.clone());
    speichern(&l)?;
    Ok(b)
}

/// Eine Rechnung im Hintergrund laden (nur wenn es wirklich ein PDF ist).
fn rechnung_laden(link: String, ziel: std::path::PathBuf) {
    let _ = std::thread::Builder::new().name("narsil-beleg".into()).spawn(move || {
        let Ok(bytes) = crate::laden::lade_bytes(&link) else { return };
        if bytes.len() <= MAX_PDF && bytes.starts_with(b"%PDF") {
            let _ = schreiben(&ziel, &bytes);
        }
    });
}

/// Die faelligen Garantie-Erinnerungen.
pub fn faellig(l: &[Beleg], jetzt: i64) -> Vec<&Beleg> {
    l.iter()
        .filter(|b| b.garantie_bis >= jetzt && b.garantie_bis - jetzt <= VORLAUF_TAGE * 86_400)
        .collect()
}

/// Die Garantie eines Belegs aendern (Jahre ab Kaufdatum).
pub fn garantie_setzen(id: u64, jahre: i64) -> Result<(), String> {
    let mut l = laden();
    if let Some(b) = l.iter_mut().find(|b| b.id == id) {
        let (j, m, t) = super::datum_aus_tagen(b.datum.div_euclid(86_400));
        b.garantie_bis = super::tage_aus_datum(j + jahre.clamp(0, 30), m, t.min(28)) * 86_400;
    }
    speichern(&l)
}

/// Einen Beleg loeschen (mit den abgelegten Dateien).
pub fn entfernen(id: u64) -> Result<(), String> {
    let mut l = laden();
    if let Some(b) = l.iter().find(|b| b.id == id) {
        for p in [&b.seite, &b.rechnung] {
            if !p.is_empty() {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    l.retain(|b| b.id != id);
    speichern(&l)
}

fn datei_adresse(p: &str) -> String {
    format!("file:///{}", p.replace('\\', "/").trim_start_matches('/'))
}

/// Die Garantiedauer eines Belegs in ganzen Jahren (Kaufdatum bis Ende).
pub fn garantie_jahre_von(b: &Beleg) -> i64 {
    let (j0, m0, t0) = super::datum_aus_tagen(b.datum.div_euclid(86_400));
    let (j1, m1, t1) = super::datum_aus_tagen(b.garantie_bis.div_euclid(86_400));
    let mut jahre = j1 - j0;
    if (m1, t1) < (m0, t0.min(28)) {
        jahre -= 1;
    }
    jahre.max(0)
}

/// Die Liste fuer `narsil://belege`.
pub fn uebersicht(jetzt: i64) -> String {
    let mut l = laden();
    l.sort_by(|a, b| b.datum.cmp(&a.datum));
    let mut h = format!(
        "<p>{}</p>",
        super::html(narsil_i18n::tr("Detected purchases with a warranty period — stored locally, Narsil reminds you 30 days before it ends."))
    );
    if l.is_empty() {
        h.push_str(&format!("<p>{}</p>", super::html(narsil_i18n::tr("No receipts yet."))));
        return h;
    }
    h.push_str(&format!(
        "<table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th></th></tr>",
        super::html(narsil_i18n::tr("Shop")),
        super::html(narsil_i18n::tr("Order")),
        super::html(narsil_i18n::tr("Date")),
        super::html(narsil_i18n::tr("Amount")),
        super::html(narsil_i18n::tr("Warranty until")),
        super::html(narsil_i18n::tr("Stored"))
    ));
    for b in &l {
        let rest = (b.garantie_bis - jetzt).div_euclid(86_400);
        // Release 09.10.2026: das Feld zeigt die erkannte Garantiedauer (vorher
        // stand dort immer 2 — auch bei „3 Jahre Herstellergarantie").
        let jahre = garantie_jahre_von(b);
        h.push_str(&format!(
            "<tr><td><a href=\"{}\">{}</a></td><td>{}</td><td>{}</td><td>{}</td><td><b>{}</b> <small>{}</small></td><td>{}{}</td>\
             <td><form action=\"narsil://belege\" method=\"get\"><input type=\"hidden\" name=\"aktion\" value=\"garantie\">\
             <input type=\"hidden\" name=\"id\" value=\"{}\"><input name=\"jahre\" size=\"2\" value=\"{}\"> {} \
             <button type=\"submit\">{}</button></form> <a href=\"narsil://belege?aktion=weg&amp;id={}\">{}</a></td></tr>",
            super::html(&b.adresse),
            super::html(&b.shop),
            super::html(&b.bestellnummer),
            super::datum_text(b.datum),
            super::html(&b.betrag),
            super::datum_text(b.garantie_bis),
            if rest < 0 { narsil_i18n::tr("expired").to_string() } else { narsil_i18n::trf!("{} days left", rest) },
            if b.seite.is_empty() { String::new() } else { format!("<a href=\"{}\">{}</a>", super::html(&datei_adresse(&b.seite)), super::html(narsil_i18n::tr("Page"))) },
            if b.rechnung.is_empty() || !std::path::Path::new(&b.rechnung).exists() {
                String::new()
            } else {
                format!(" · <a href=\"{}\">{}</a>", super::html(&datei_adresse(&b.rechnung)), super::html(narsil_i18n::tr("Invoice (PDF)")))
            },
            b.id,
            jahre,
            super::html(narsil_i18n::tr("years")),
            super::html(narsil_i18n::tr("Set")),
            b.id,
            super::html(narsil_i18n::tr("Delete"))
        ));
    }
    h.push_str("</table>");
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    const JETZT: i64 = 1_791_417_600;

    #[test]
    fn bestellbestaetigung_wird_erkannt() {
        let e = erkennen(
            "Bestellbestätigung – Technikladen",
            "Vielen Dank für Ihre Bestellung! Bestellnummer: 304-1234567-89. Gesamtsumme 249,00 € inkl. MwSt. \
             Bestelldatum 07.10.2026. Für Geräte gilt 3 Jahre Herstellergarantie.",
            JETZT,
        )
        .expect("erkannt");
        assert_eq!(e.bestellnummer, "304-1234567-89");
        assert_eq!(e.betrag, "249,00 €");
        assert_eq!(super::super::datum_text(e.datum), "2026-10-07");
        assert_eq!(e.garantie_jahre, 3);
        // Ohne Bestaetigung: nichts.
        assert_eq!(erkennen("Warenkorb", "Bestellnummer folgt. 249,00 €", JETZT), None);
        assert_eq!(erkennen("Danke", "Vielen Dank für Ihre Bestellung", JETZT), None, "ohne Nummer und Betrag");
    }

    #[test]
    fn beleg_ablage_garantie_und_erinnerung() {
        let dir = std::env::temp_dir().join(format!("narsil-test-beleg-{}-{:?}", std::process::id(), std::thread::current().id()));
        super::super::ordner_fuer_faden(Some(dir.clone()));
        let html = "<html><body><p>Danke</p><a href=\"/konto/rechnung_42.pdf\">Rechnung herunterladen</a></body></html>";
        let baum = narsil_page::html_lesen(html);
        assert_eq!(rechnungs_links(&baum, "https://shop.example/danke"), vec!["https://shop.example/konto/rechnung_42.pdf".to_string()]);
        let e = Erkannt { bestellnummer: "A-1001".into(), datum: JETZT, betrag: "19,99 €".into(), garantie_jahre: 2 };
        // Ohne Netz: der Link ist lokal nicht erreichbar, die Seite wird trotzdem abgelegt.
        let b = eintragen("https://shop.example/danke", "Danke – Shop", &e, Some(html), &[]).expect("eintragen");
        assert!(std::path::Path::new(&b.seite).exists());
        assert_eq!(super::super::datum_text(b.garantie_bis), "2028-10-08");
        // Doppelt: kein zweiter Eintrag.
        eintragen("https://shop.example/danke2", "Danke", &e, Some(html), &[]).expect("nochmal");
        assert_eq!(laden().len(), 1);
        assert!(faellig(&laden(), JETZT).is_empty());
        assert_eq!(faellig(&laden(), b.garantie_bis - 10 * 86_400).len(), 1);
        // Release 09.10.2026: das Jahresfeld zeigt die Dauer des Belegs.
        assert_eq!(garantie_jahre_von(&laden()[0]), 2);
        assert!(uebersicht(JETZT).contains("name=\"jahre\" size=\"2\" value=\"2\""));
        garantie_setzen(b.id, 5).expect("garantie");
        assert_eq!(super::super::datum_text(laden()[0].garantie_bis), "2031-10-08");
        assert!(uebersicht(JETZT).contains("A-1001"));
        assert_eq!(garantie_jahre_von(&laden()[0]), 5);
        assert!(uebersicht(JETZT).contains("name=\"jahre\" size=\"2\" value=\"5\""));
        entfernen(b.id).expect("weg");
        assert!(laden().is_empty());
        assert!(!std::path::Path::new(&b.seite).exists());
        super::super::ordner_fuer_faden(None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
