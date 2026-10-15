//! Release 08.10.2026: FEATURE 8 — ABO- & FRISTEN-ASSISTENT.
//!
//! Nach jedem Laden wird der Text der Seite heuristisch geprueft: steht dort
//! eine **Bestaetigung** (Danke, erfolgreich, aktiviert …) zusammen mit einem
//! **Abo** oder einer **Testphase** und einer **Laufzeit** („30 Tage",
//! „1 Monat") oder einem **Enddatum** („endet am 12.11.2026")? Dann kommt der
//! Dienst mit Beginn, Ende und Erinnerung in `<profil>/features/fristen.tsv`.
//! Drei Tage vor dem Ende erinnert ein Hinweis, und `narsil://fristen/vorlage`
//! erzeugt eine Kuendigungsvorlage. Es wird nichts gesendet.

use super::{felder, lesen_text, ordner, schreiben, zeile};

/// Wie viele Tage vor dem Ende erinnert wird.
pub const VORLAUF_TAGE: i64 = 3;
/// M3: hoechstens so viele Eintraege.
pub const MAX_EINTRAEGE: usize = 200;
/// M4: so viel Seitentext wird gelesen.
pub const MAX_TEXT: usize = 40_000;

/// Ein erkanntes Abo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frist {
    pub id: u64,
    pub dienst: String,
    pub adresse: String,
    pub beginn: i64,
    pub ende: i64,
    pub probe: bool,
    pub preis: String,
    pub erledigt: bool,
}

fn pfad() -> std::path::PathBuf {
    ordner().join("fristen.tsv")
}

pub fn laden() -> Vec<Frist> {
    lesen_text(&pfad())
        .lines()
        .take(MAX_EINTRAEGE)
        .filter_map(|z| {
            let f = felder(z);
            if f.len() < 8 {
                return None;
            }
            Some(Frist {
                id: f[0].parse().ok()?,
                dienst: f[1].clone(),
                adresse: f[2].clone(),
                beginn: f[3].parse().ok()?,
                ende: f[4].parse().ok()?,
                probe: f[5] == "probe",
                preis: f[6].clone(),
                erledigt: f[7] == "erledigt",
            })
        })
        .collect()
}

pub fn speichern(l: &[Frist]) -> Result<(), String> {
    let t: Vec<String> = l
        .iter()
        .take(MAX_EINTRAEGE)
        .map(|f| {
            zeile(&[
                &f.id.to_string(),
                &f.dienst,
                &f.adresse,
                &f.beginn.to_string(),
                &f.ende.to_string(),
                if f.probe { "probe" } else { "abo" },
                &f.preis,
                if f.erledigt { "erledigt" } else { "offen" },
            ])
        })
        .collect();
    schreiben(&pfad(), t.join("\n").as_bytes())
}

// ---------------------------------------------------------------------------
// DIE ERKENNUNG (rein, testbar)
// ---------------------------------------------------------------------------

/// Text fuer die Suche: klein, Umlaute ersetzt, Leerraum zusammengezogen.
pub fn normal(t: &str) -> String {
    let k: String = t.chars().take(MAX_TEXT).collect::<String>().to_lowercase();
    super::baum::zusammen(&k.replace('ä', "ae").replace('ö', "oe").replace('ü', "ue").replace('ß', "ss"))
}

fn enthaelt(t: &str, worte: &[&str]) -> bool {
    worte.iter().any(|w| t.contains(w))
}

const MONATE: [(&str, u32); 24] = [
    ("januar", 1), ("februar", 2), ("maerz", 3), ("april", 4), ("mai", 5), ("juni", 6),
    ("juli", 7), ("august", 8), ("september", 9), ("oktober", 10), ("november", 11), ("dezember", 12),
    ("january", 1), ("february", 2), ("march", 3), ("april", 4), ("may", 5), ("june", 6),
    ("july", 7), ("august", 8), ("september", 9), ("october", 10), ("november", 11), ("december", 12),
];

/// Alle Daten im Text (Sekunden), in Fundreihenfolge: `TT.MM.JJJJ`,
/// `JJJJ-MM-TT`, `12. November 2026`, `November 12, 2026`.
pub fn daten(t: &str) -> Vec<i64> {
    let mut raus = Vec::new();
    let b = t.as_bytes();
    let mut i = 0usize;
    while i < b.len() && raus.len() < 32 {
        if b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_digit()) {
            let rest = &t[i..];
            // TT.MM.JJJJ
            let kandidat: String = rest.chars().take(10).collect();
            if let Some(s) = super::datum_lesen(&kandidat).filter(|_| kandidat.matches('.').count() == 2 && kandidat.len() == 10) {
                raus.push(s);
                i += 10;
                continue;
            }
            if let Some(s) = super::datum_lesen(&kandidat).filter(|_| kandidat.matches('-').count() == 2 && kandidat.len() == 10) {
                raus.push(s);
                i += 10;
                continue;
            }
            // 12. november 2026
            let worte: Vec<&str> = rest.split(' ').take(3).collect();
            if worte.len() == 3 {
                let tag = worte[0].trim_end_matches('.').parse::<u32>().ok();
                let monat = MONATE.iter().find(|(n, _)| worte[1].trim_end_matches(',') == *n).map(|(_, m)| *m);
                let jahr = worte[2].trim_end_matches(['.', ',']).get(..4).and_then(|j| j.parse::<i64>().ok());
                if let (Some(t), Some(m), Some(j)) = (tag, monat, jahr)
                    && (1..=31).contains(&t)
                    && (1970..=2200).contains(&j)
                {
                    raus.push(super::tage_aus_datum(j, m, t) * 86_400);
                }
            }
        }
        if b[i].is_ascii_alphabetic() && (i == 0 || b[i - 1] == b' ') {
            // november 12, 2026
            let rest = &t[i..];
            let worte: Vec<&str> = rest.split(' ').take(3).collect();
            if worte.len() == 3
                && let Some((_, m)) = MONATE.iter().find(|(n, _)| worte[0] == *n)
                && let Ok(tag) = worte[1].trim_end_matches(',').parse::<u32>()
                && let Some(j) = worte[2].trim_end_matches(['.', ',']).get(..4).and_then(|j| j.parse::<i64>().ok())
                && (1..=31).contains(&tag)
                && (1970..=2200).contains(&j)
            {
                raus.push(super::tage_aus_datum(j, *m, tag) * 86_400);
            }
        }
        i += t[i..].chars().next().map_or(1, char::len_utf8);
    }
    raus
}

/// Eine Laufzeit im Text in Tagen („30 Tage", „14-tägig", „1 Monat",
/// „2 weeks", „ein Jahr").
pub fn laufzeit_tage(t: &str) -> Option<i64> {
    let worte: Vec<&str> = t.split([' ', '-']).collect();
    for (i, w) in worte.iter().enumerate() {
        let zahl = match *w {
            "ein" | "einen" | "eine" | "one" | "a" => Some(1),
            "zwei" | "two" => Some(2),
            "drei" | "three" => Some(3),
            "sieben" | "seven" => Some(7),
            "vierzehn" | "fourteen" => Some(14),
            "dreissig" | "thirty" => Some(30),
            x => x.trim_matches(|c: char| !c.is_ascii_digit()).parse::<i64>().ok().filter(|_| x.chars().next().is_some_and(|c| c.is_ascii_digit())),
        };
        let Some(n) = zahl.filter(|n| (1..=1000).contains(n)) else { continue };
        let Some(einheit) = worte.get(i + 1) else { continue };
        let tage = if einheit.starts_with("tag") || einheit.starts_with("day") || einheit.starts_with("taegig") {
            1
        } else if einheit.starts_with("woche") || einheit.starts_with("week") || einheit.starts_with("woechig") {
            7
        } else if einheit.starts_with("monat") || einheit.starts_with("month") {
            30
        } else if einheit.starts_with("jahr") || einheit.starts_with("year") {
            365
        } else {
            continue;
        };
        return Some(n * tage);
    }
    None
}

/// Ein Preis im Text („9,99 €", „€ 4.99", „12,00 eur").
pub fn preis(t: &str) -> Option<String> {
    for (i, _) in t.match_indices(|c: char| c == '€' || c == '$') {
        // davor: „9,99 €"
        let vorher = t[..i].trim_end();
        let zahl: String = vorher.chars().rev().take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.').collect::<Vec<_>>().into_iter().rev().collect();
        if zahl.chars().any(|c| c.is_ascii_digit()) && (zahl.contains(',') || zahl.contains('.')) {
            return Some(format!("{zahl} {}", &t[i..i + t[i..].chars().next().map_or(1, char::len_utf8)]));
        }
        // danach: „€ 4.99"
        let nachher = t[i..].chars().skip(1).collect::<String>();
        let zahl: String = nachher.trim_start().chars().take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.').collect();
        if zahl.chars().any(|c| c.is_ascii_digit()) {
            return Some(format!("{} {zahl}", &t[i..i + t[i..].chars().next().map_or(1, char::len_utf8)]));
        }
    }
    None
}

/// Das Ergebnis der Erkennung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Erkannt {
    pub probe: bool,
    pub ende: i64,
    pub preis: String,
}

const BESTAETIGUNG: &[&str] = &[
    "vielen dank", "danke fuer", "erfolgreich", "willkommen", "ist jetzt aktiv", "wurde aktiviert",
    "bestaetig", "thank you", "thanks for", "welcome to", "is now active", "confirmed", "you're all set",
    "you are all set", "subscription is active", "abgeschlossen",
];
const PROBE: &[&str] = &[
    "probeabo", "probe-abo", "probemonat", "testphase", "testzeitraum", "test-zeitraum", "kostenlos testen",
    "gratis testen", "kostenlose testversion", "free trial", "trial period", "your trial", "ihr test",
    "probezeitraum", "probe abo",
];
const ABO: &[&str] = &[
    "abonnement", "ihr abo", "dein abo", "abo wurde", "mitgliedschaft", "membership", "subscription",
    "subscribed", "verlaengert sich automatisch", "renews automatically", "auto-renew", "premium-abo",
    "monatlich kuendbar", "jederzeit kuendbar",
];

/// Die Heuristik: ist die Seite eine Abo-/Probeabo-Bestaetigung?
pub fn erkennen(titel: &str, text: &str, jetzt: i64) -> Option<Erkannt> {
    let t = normal(&format!("{titel} {text}"));
    let probe = enthaelt(&t, PROBE);
    let abo = probe || enthaelt(&t, ABO);
    if !abo || !enthaelt(&t, BESTAETIGUNG) {
        return None;
    }
    // Ein ausdrueckliches Ende („endet am", „bis zum", „ends on") gewinnt.
    let mut ende = None;
    for marke in ["endet am", "bis zum", "bis einschliesslich", "laeuft bis", "ends on", "until", "kuendigung bis", "verlaengert sich am", "renews on", "naechste zahlung am", "next payment"] {
        if let Some(i) = t.find(marke) {
            let rest: String = t[i + marke.len()..].chars().take(40).collect();
            if let Some(d) = daten(rest.trim()).into_iter().find(|d| *d > jetzt - 86_400) {
                ende = Some(d);
                break;
            }
        }
    }
    if ende.is_none() {
        let d = laufzeit_tage(&t)?;
        ende = Some(jetzt + d * 86_400);
    }
    Some(Erkannt { probe, ende: ende?, preis: preis(&t).unwrap_or_default() })
}

/// Grussformeln vor dem Dienstnamen im Seitentitel (klein, ASCII-Laenge =
/// Byte-Laenge des Originals, weil nur ASCII-Zeichen darin stehen).
const GRUSS: &[&str] = &[
    "willkommen bei ",
    "willkommen auf ",
    "willkommen im ",
    "willkommen in ",
    "willkommen ",
    "welcome to ",
    "welcome ",
    "thanks for joining ",
];

/// Die Restzeit als Text: „heute", „in 1 Tag", „in 30 Tagen" (aufgerundet
/// auf ganze Tage — eine Frist, die in 29 Tagen und 23 Stunden endet, endet
/// „in 30 Tagen").
pub fn rest_text(ende: i64, jetzt: i64) -> String {
    let sekunden = ende - jetzt;
    let tage = if sekunden <= 0 { 0 } else { (sekunden + 86_399) / 86_400 };
    match tage {
        0 => narsil_i18n::tr("today").to_string(),
        1 => narsil_i18n::tr("in 1 day").to_string(),
        n => narsil_i18n::trf!("in {} days", n),
    }
}

/// Den Namen des Dienstes aus Titel und Host.
pub fn dienst_name(titel: &str, adresse: &str) -> String {
    let host = super::host_von(adresse);
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let t = titel.split(['|', '–', '-', '·', ':']).map(str::trim).filter(|x| !x.is_empty()).last().unwrap_or("");
    // Release 09.10.2026: Grussformeln gehoeren nicht zum Namen (gemessen:
    // „Willkommen bei StreamFlix" stand als Dienst in der Liste).
    let klein = t.to_lowercase();
    let t = GRUSS
        .iter()
        .find(|g| klein.starts_with(*g))
        .and_then(|g| t.get(g.len()..))
        .map_or(t, str::trim);
    if t.is_empty() || t.chars().count() > 40 {
        host
    } else {
        format!("{t} ({host})")
    }
}

/// Eine erkannte Frist eintragen (dieselbe Seite innerhalb von 7 Tagen
/// aktualisiert den Eintrag statt eines zweiten).
pub fn eintragen(dienst: &str, adresse: &str, e: &Erkannt, jetzt: i64) -> Result<Frist, String> {
    let mut l = laden();
    let host = super::host_von(adresse);
    let id = l.iter().map(|f| f.id).max().unwrap_or(0) + 1;
    let f = Frist {
        id,
        dienst: dienst.to_string(),
        adresse: adresse.to_string(),
        beginn: jetzt,
        ende: e.ende,
        probe: e.probe,
        preis: e.preis.clone(),
        erledigt: false,
    };
    if let Some(alt) = l.iter_mut().find(|x| super::host_von(&x.adresse) == host && jetzt - x.beginn < 7 * 86_400) {
        let id = alt.id;
        *alt = Frist { id, ..f };
        let neu = alt.clone();
        speichern(&l)?;
        return Ok(neu);
    }
    l.push(f.clone());
    speichern(&l)?;
    Ok(f)
}

/// Die faelligen Erinnerungen (offen, Ende in hoechstens `VORLAUF_TAGE`).
pub fn faellig(l: &[Frist], jetzt: i64) -> Vec<&Frist> {
    l.iter()
        .filter(|f| !f.erledigt && f.ende >= jetzt - 86_400 && f.ende - jetzt <= VORLAUF_TAGE * 86_400)
        .collect()
}

/// Die Kuendigungsvorlage.
pub fn vorlage(f: &Frist) -> String {
    // The letter is one translation unit: {0} service, {1} trial or
    // subscription, {2} end date, {3} start date, {4} address.
    let art = if f.probe { narsil_i18n::tr("trial subscription") } else { narsil_i18n::tr("subscription") };
    narsil_i18n::trf!(
        "<h2>Cancellation: {0}</h2><pre>To: {0}\nSubject: Cancellation of my {1}\n\nDear Sir or Madam,\n\nI hereby cancel my {1} with {0} at the next possible date,\nat the latest on {2}. Please confirm the cancellation and the end of the contract\nin writing. I object to any further renewal and any further debits.\n\nCustomer number / e-mail of the account: ____________________\n\nKind regards\n\n____________________\nPlace, date: ____________________</pre><p><small>Started on {3} at {4}. Detected by the deadline assistant — please check the data.</small></p>",
        super::html(&f.dienst),
        art,
        super::datum_text(f.ende),
        super::datum_text(f.beginn),
        super::html(&f.adresse)
    )
}

/// Die Liste fuer `narsil://fristen`.
pub fn uebersicht(jetzt: i64) -> String {
    let mut l = laden();
    l.sort_by_key(|f| (f.erledigt, f.ende));
    let mut h = format!(
        "<p>{}</p>\
         <form action=\"narsil://fristen\" method=\"get\"><input type=\"hidden\" name=\"aktion\" value=\"neu\">\
         {} <input name=\"dienst\" size=\"18\"> {} <input name=\"ende\" size=\"10\"> \
         <label><input type=\"checkbox\" name=\"probe\" value=\"1\"> {}</label> \
         <button type=\"submit\">{}</button></form>",
        super::html(narsil_i18n::tr("Detected subscriptions and trials — stored only locally. Narsil reminds you three days before they expire.")),
        super::html(narsil_i18n::tr("Service")),
        super::html(narsil_i18n::tr("End (YYYY-MM-DD)")),
        super::html(narsil_i18n::tr("Trial")),
        super::html(narsil_i18n::tr("Add")),
    );
    if l.is_empty() {
        h.push_str(&format!("<p>{}</p>", super::html(narsil_i18n::tr("No deadlines yet."))));
        return h;
    }
    h.push_str(&format!(
        "<table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th></th></tr>",
        super::html(narsil_i18n::tr("Service")),
        super::html(narsil_i18n::tr("Kind")),
        super::html(narsil_i18n::tr("Start")),
        super::html(narsil_i18n::tr("End")),
        super::html(narsil_i18n::tr("Price"))
    ));
    for f in &l {
        let wann = if f.erledigt {
            narsil_i18n::tr("done").to_string()
        } else if f.ende < jetzt {
            narsil_i18n::tr("expired").to_string()
        } else {
            rest_text(f.ende, jetzt)
        };
        h.push_str(&format!(
            "<tr><td><a href=\"{}\">{}</a></td><td>{}</td><td>{}</td><td><b>{}</b> <small>{}</small></td><td>{}</td>\
             <td><a href=\"narsil://fristen/vorlage?id={}\">{}</a> · \
             <a href=\"narsil://fristen?aktion=erledigt&amp;id={}\">{}</a> · \
             <a href=\"narsil://fristen?aktion=weg&amp;id={}\">{}</a></td></tr>",
            super::html(&f.adresse),
            super::html(&f.dienst),
            if f.probe { narsil_i18n::tr("Trial") } else { narsil_i18n::tr("Subscription") },
            super::datum_text(f.beginn),
            super::datum_text(f.ende),
            wann,
            super::html(&f.preis),
            f.id,
            super::html(narsil_i18n::tr("Cancellation template")),
            f.id,
            super::html(narsil_i18n::tr("Mark as done")),
            f.id,
            super::html(narsil_i18n::tr("Delete"))
        ));
    }
    h.push_str("</table>");
    h
}

impl crate::Sitzung {
    /// Nach dem Laden einer Seite: Fristen (8) und Belege (10) suchen.
    pub fn fristen_und_belege_pruefen(&mut self) {
        let f8 = self.einstellungen.features.an(super::Feature::Fristen);
        let f10 = self.einstellungen.features.an(super::Feature::Belege);
        if !f8 && !f10 {
            return;
        }
        let adresse = self.browser.adresse().to_string();
        if !(adresse.starts_with("http://") || adresse.starts_with("https://")) || self.features.ausgewertet == adresse {
            return;
        }
        let Some(a) = self.angezeigt.as_ref() else { return };
        self.features.ausgewertet = adresse.clone();
        let titel = a.seite.titel.clone();
        let st = super::baum::stellen(&a.html);
        let text = if st.is_empty() { String::new() } else { super::baum::text_von(&st, 0, Some(&a.patch)) };
        let jetzt = super::jetzt_s();
        if f8 && let Some(e) = erkennen(&titel, &text, jetzt) {
            let dienst = dienst_name(&titel, &adresse);
            if let Ok(f) = eintragen(&dienst, &adresse, &e, jetzt) {
                self.features.hinweis(
                    narsil_i18n::trf!(
                        "{} detected: {} — ends on {} (list: narsil://fristen)",
                        if f.probe { narsil_i18n::tr("Trial") } else { narsil_i18n::tr("Subscription") },
                        f.dienst,
                        super::datum_text(f.ende)
                    ),
                    Some("narsil://fristen".into()),
                );
                self.schmutzig = true;
            }
        }
        if f10 {
            let rohtext = a.rohtext.clone();
            let html = a.html.clone();
            if let Some(b) = super::belege::erkennen(&titel, &text, jetzt) {
                if let Ok(eintrag) = super::belege::eintragen(&adresse, &titel, &b, rohtext.as_deref(), &html) {
                    self.features.hinweis(
                        narsil_i18n::trf!(
                            "Receipt saved: {}{} — warranty until {} (narsil://belege)",
                            eintrag.shop,
                            if eintrag.bestellnummer.is_empty() { String::new() } else { narsil_i18n::trf!(", order {}", eintrag.bestellnummer) },
                            super::datum_text(eintrag.garantie_bis)
                        ),
                        Some("narsil://belege".into()),
                    );
                    self.schmutzig = true;
                }
            }
        }
    }

    /// Erinnerungen fuer Fristen (8) und Garantien (10) — beim Start und
    /// danach stuendlich.
    pub fn erinnerungen_pruefen(&mut self, jetzt_i: std::time::Instant) {
        if self.features.erinnerung.is_some_and(|t| jetzt_i.duration_since(t) < std::time::Duration::from_secs(3600)) {
            return;
        }
        self.features.erinnerung = Some(jetzt_i);
        let jetzt = super::jetzt_s();
        if self.einstellungen.features.an(super::Feature::Fristen) {
            let l = laden();
            if let Some(f) = faellig(&l, jetzt).first() {
                self.features.hinweis(
                    narsil_i18n::trf!(
                        "Your {} with {} ends {} — open the cancellation template?",
                        if f.probe { narsil_i18n::tr("trial subscription") } else { narsil_i18n::tr("subscription") },
                        f.dienst,
                        rest_text(f.ende, jetzt)
                    ),
                    Some(format!("narsil://fristen/vorlage?id={}", f.id)),
                );
                self.schmutzig = true;
                return;
            }
        }
        if self.einstellungen.features.an(super::Feature::Belege) {
            let l = super::belege::laden();
            if let Some(b) = super::belege::faellig(&l, jetzt).first() {
                self.features.hinweis(
                    narsil_i18n::trf!("The warranty for the purchase at {} ends on {}", b.shop, super::datum_text(b.garantie_bis)),
                    Some("narsil://belege".into()),
                );
                self.schmutzig = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JETZT: i64 = 1_791_417_600; // 08.10.2026

    #[test]
    fn daten_und_laufzeiten_werden_gelesen() {
        assert_eq!(daten("endet am 12.11.2026 um"), vec![super::super::datum_lesen("12.11.2026").unwrap()]);
        assert_eq!(daten("bis 2026-11-12"), vec![super::super::datum_lesen("12.11.2026").unwrap()]);
        assert_eq!(daten("am 12. november 2026"), vec![super::super::datum_lesen("12.11.2026").unwrap()]);
        assert_eq!(daten("on november 12, 2026."), vec![super::super::datum_lesen("12.11.2026").unwrap()]);
        assert_eq!(laufzeit_tage("ihr 30 tage probeabo"), Some(30));
        assert_eq!(laufzeit_tage("14-taegige testphase"), Some(14));
        assert_eq!(laufzeit_tage("einen monat gratis"), Some(30));
        assert_eq!(laufzeit_tage("your 2 weeks trial"), Some(14));
        assert_eq!(laufzeit_tage("ohne frist"), None);
        assert_eq!(preis("danach 9,99 € monatlich").as_deref(), Some("9,99 €"));
        assert_eq!(preis("then $ 4.99 per month").as_deref(), Some("$ 4.99"));
    }

    #[test]
    fn probeabo_bestaetigung_wird_erkannt_werbung_nicht() {
        let e = erkennen(
            "Willkommen bei StreamFlix",
            "Vielen Dank! Ihr 30-Tage-Probeabo ist jetzt aktiv. Danach 9,99 € pro Monat. Jederzeit kündbar.",
            JETZT,
        )
        .expect("erkannt");
        assert!(e.probe);
        assert_eq!(e.ende, JETZT + 30 * 86_400);
        assert_eq!(e.preis, "9,99 €");
        let e2 = erkennen("Bestätigung", "Ihre Mitgliedschaft wurde aktiviert. Sie endet am 01.12.2026.", JETZT).expect("e2");
        assert_eq!(super::super::datum_text(e2.ende), "2026-12-01");
        // Eine Werbeseite ohne Bestaetigung: nichts.
        assert_eq!(erkennen("Jetzt testen", "Starte dein 30-Tage-Probeabo jetzt!", JETZT), None);
        // Eine Nachricht ohne Abo: nichts.
        assert_eq!(erkennen("Danke", "Vielen Dank für Ihre Nachricht.", JETZT), None);
    }

    #[test]
    fn eintragen_erinnern_und_vorlage() {
        let dir = std::env::temp_dir().join(format!("narsil-test-frist-{}-{:?}", std::process::id(), std::thread::current().id()));
        super::super::ordner_fuer_faden(Some(dir.clone()));
        let e = Erkannt { probe: true, ende: JETZT + 2 * 86_400, preis: "9,99 €".into() };
        let f = eintragen("StreamFlix (streamflix.example)", "https://streamflix.example/willkommen", &e, JETZT).expect("eintragen");
        // Dieselbe Seite noch einmal: kein zweiter Eintrag.
        eintragen("StreamFlix", "https://streamflix.example/danke", &e, JETZT + 10).expect("nochmal");
        let l = laden();
        assert_eq!(l.len(), 1);
        assert_eq!(faellig(&l, JETZT).len(), 1);
        assert!(faellig(&l, JETZT - 10 * 86_400).is_empty());
        let v = vorlage(&f);
        assert!(v.contains("Cancellation") && v.contains("trial subscription") && v.contains(&super::super::datum_text(f.ende)));
        assert!(uebersicht(JETZT).contains("in 2 days"));
        // Release 09.10.2026: aufgerundet und mit richtiger Mehrzahl.
        assert_eq!(rest_text(JETZT + 30 * 86_400 - 5, JETZT), "in 30 days");
        assert_eq!(rest_text(JETZT + 3_600, JETZT), "in 1 day");
        assert_eq!(rest_text(JETZT, JETZT), "today");
        {
            // The German letter keeps its wording.
            let _de = narsil_i18n::scoped(narsil_i18n::Language::German);
            let v = vorlage(&f);
            assert!(v.contains("Kündigung") && v.contains("Probeabos") && v.contains(&super::super::datum_text(f.ende)), "{v}");
            assert_eq!(rest_text(JETZT + 30 * 86_400 - 5, JETZT), "in 30 Tagen");
        }
        assert_eq!(dienst_name("Willkommen bei StreamFlix", "https://www.streamflix.example/x"), "StreamFlix (streamflix.example)");
        assert_eq!(dienst_name("Danke", "https://shop.example/"), "Danke (shop.example)");
        super::super::ordner_fuer_faden(None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
