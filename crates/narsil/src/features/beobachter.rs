//! Release 08.10.2026: FEATURE 2 — ELEMENT-WAECHTER.
//!
//! Rechtsklick → „Element beobachten" merkt sich Adresse, Pfad des Elements
//! (`baum::pfad_zu`, mit `id`-Anker wo moeglich) und den sichtbaren Wert.
//!
//! * **Offene Seite**: alle paar Sekunden wird der Wert im aktuellen
//!   DOM-Stand gelesen (Baum plus Patch — also auch Aenderungen durch
//!   Skripte der Seite).
//! * **Hintergrund**: im eingestellten Takt holt ein eigener Faden die Seite
//!   (mit den Cookies des Browsers), liest das HTML und vergleicht.
//!
//! Eine Aenderung erscheint als dezenter Hinweis am unteren Rand und in der
//! Liste `narsil://beobachter`. Alles lokal.

use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{felder, lesen_text, ordner, schreiben, zeile};

/// M3: hoechstens so viele beobachtete Elemente.
pub const MAX_EINTRAEGE: usize = 64;
/// M4: hoechstens so viele Zeichen je Wert.
pub const MAX_WERT: usize = 300;
/// Der Takt fuer die offene Seite.
pub const TAKT_OFFEN: std::time::Duration = std::time::Duration::from_secs(3);

/// Ein beobachtetes Element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eintrag {
    pub id: u64,
    pub adresse: String,
    pub pfad: String,
    pub name: String,
    pub wert: String,
    pub alter_wert: String,
    pub geprueft: i64,
    pub geaendert: i64,
    pub fehler: String,
}

fn pfad() -> std::path::PathBuf {
    ordner().join("beobachter.tsv")
}

fn takt_pfad() -> std::path::PathBuf {
    ordner().join("beobachter_takt.txt")
}

/// Release 08.10.2026: der Takt wird in jedem Tick gebraucht — einmal je
/// Profilordner lesen und merken, statt jede Runde die Datei zu oeffnen.
static TAKT: Mutex<Option<(std::path::PathBuf, u64)>> = Mutex::new(None);

/// Der Hintergrund-Takt in Minuten (1 bis 1440, Vorgabe 5).
pub fn takt_minuten() -> u64 {
    let p = takt_pfad();
    let mut g = TAKT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((q, m)) = g.as_ref() {
        if *q == p {
            return *m;
        }
    }
    let m = lesen_text(&p).trim().parse::<u64>().unwrap_or(5).clamp(1, 1440);
    *g = Some((p, m));
    m
}

/// Den Hintergrund-Takt setzen.
pub fn takt_setzen(min: u64) -> Result<(), String> {
    let m = min.clamp(1, 1440);
    schreiben(&takt_pfad(), m.to_string().as_bytes())?;
    *TAKT.lock().unwrap_or_else(|e| e.into_inner()) = Some((takt_pfad(), m));
    Ok(())
}

/// Die Liste lesen.
pub fn laden() -> Vec<Eintrag> {
    lesen_text(&pfad())
        .lines()
        .take(MAX_EINTRAEGE)
        .filter_map(|z| {
            let f = felder(z);
            if f.len() < 8 {
                return None;
            }
            Some(Eintrag {
                id: f[0].parse().ok()?,
                adresse: f[1].clone(),
                pfad: f[2].clone(),
                name: f[3].clone(),
                wert: f[4].clone(),
                alter_wert: f[5].clone(),
                geprueft: f[6].parse().unwrap_or(0),
                geaendert: f[7].parse().unwrap_or(0),
                fehler: f.get(8).cloned().unwrap_or_default(),
            })
        })
        .collect()
}

/// Die Liste schreiben.
pub fn speichern(l: &[Eintrag]) -> Result<(), String> {
    let text: Vec<String> = l
        .iter()
        .take(MAX_EINTRAEGE)
        .map(|e| {
            zeile(&[
                &e.id.to_string(),
                &e.adresse,
                &e.pfad,
                &e.name,
                &e.wert,
                &e.alter_wert,
                &e.geprueft.to_string(),
                &e.geaendert.to_string(),
                &e.fehler,
            ])
        })
        .collect();
    schreiben(&pfad(), text.join("\n").as_bytes())
}

fn kuerzen(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n { format!("{t}…") } else { t }
}

/// Einen Eintrag hinzufuegen (doppelte Adresse+Pfad ersetzt den alten).
pub fn hinzufuegen(adresse: &str, pfad: &str, wert: &str) -> Result<Eintrag, String> {
    let mut l = laden();
    l.retain(|e| !(e.adresse == adresse && e.pfad == pfad));
    if l.len() >= MAX_EINTRAEGE {
        return Err(narsil_i18n::trf!("At most {} elements", MAX_EINTRAEGE));
    }
    let id = l.iter().map(|e| e.id).max().unwrap_or(0) + 1;
    let e = Eintrag {
        id,
        adresse: adresse.to_string(),
        pfad: pfad.to_string(),
        name: kuerzen(wert, 40),
        wert: kuerzen(wert, MAX_WERT),
        alter_wert: String::new(),
        geprueft: super::jetzt_s(),
        geaendert: 0,
        fehler: String::new(),
    };
    l.push(e.clone());
    speichern(&l)?;
    Ok(e)
}

/// Einen Eintrag entfernen.
pub fn entfernen(id: u64) -> Result<(), String> {
    let mut l = laden();
    l.retain(|e| e.id != id);
    speichern(&l)
}

/// Ohne Anker vergleichen (`…/seite#abschnitt` ist dieselbe Seite).
pub fn gleiche_seite(a: &str, b: &str) -> bool {
    a.split('#').next() == b.split('#').next()
}

/// Einen neuen Wert eintragen; `Some(alt)`, wenn er sich geaendert hat.
pub fn wert_melden(l: &mut [Eintrag], id: u64, neu: Result<String, String>) -> Option<(String, String)> {
    let e = l.iter_mut().find(|e| e.id == id)?;
    e.geprueft = super::jetzt_s();
    match neu {
        Ok(w) => {
            e.fehler.clear();
            let w = kuerzen(&w, MAX_WERT);
            if w != e.wert {
                let alt = std::mem::replace(&mut e.wert, w.clone());
                e.alter_wert = alt.clone();
                e.geaendert = super::jetzt_s();
                return Some((alt, w));
            }
        }
        Err(f) => e.fehler = kuerzen(&f, 120),
    }
    None
}

/// Den Wert eines Elements in einem HTML-Text lesen (Hintergrund).
pub fn wert_im_html(html: &str, pfad: &str) -> Result<String, String> {
    let baum = narsil_page::html_lesen(html);
    let k = super::baum::finde(&baum, pfad).ok_or_else(|| narsil_i18n::tr("Element no longer found").to_string())?;
    Ok(super::baum::text_des_knotens(k))
}

/// Die Seite holen (mit Cookies, Faden-Netzregel frei) und den Wert lesen.
pub fn wert_holen(adresse: &str, pfad: &str) -> Result<String, String> {
    narsil_net::haken::regel_im_faden(Some(narsil_net::haken::Netzregel::Frei));
    let https = adresse.starts_with("https://");
    let rest = adresse.split_once("://").map(|(_, r)| r).ok_or_else(|| narsil_i18n::tr("not a web address").to_string())?;
    let (autoritaet, weg) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let weg = weg.split('#').next().unwrap_or("/");
    let weg = if weg.starts_with('?') { format!("/{weg}") } else { weg.to_string() };
    let standard = if https { 443 } else { 80 };
    let (host, port) = match autoritaet.rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() => (h.to_string(), p.parse().unwrap_or(standard)),
        _ => (autoritaet.to_string(), standard),
    };
    let mut anfrage = narsil_net::Request::get(&host, port, &weg);
    if https {
        anfrage = anfrage.https();
    }
    crate::keks::anfrage_bei_kekse(&mut anfrage, adresse, &host);
    let antwort = narsil_net::fetch(&anfrage).map_err(|e| e.to_string())?;
    if antwort.status.code >= 400 {
        return Err(format!("HTTP {}", antwort.status.code));
    }
    let text = String::from_utf8_lossy(&antwort.body).into_owned();
    wert_im_html(&text, pfad)
}

/// Ergebnisse des Hintergrund-Fadens: (id, Wert oder Fehler).
fn ergebnisse() -> &'static Mutex<Vec<(u64, Result<String, String>)>> {
    static E: OnceLock<Mutex<Vec<(u64, Result<String, String>)>>> = OnceLock::new();
    E.get_or_init(|| Mutex::new(Vec::new()))
}

static LAEUFT: AtomicBool = AtomicBool::new(false);
static JETZT_PRUEFEN: AtomicBool = AtomicBool::new(false);

/// Die naechste Runde sofort starten (Knopf „Jetzt prüfen").
pub fn sofort_pruefen() {
    JETZT_PRUEFEN.store(true, Ordering::SeqCst);
}

/// Den Hintergrund-Faden fuer die Eintraege starten.
pub fn hintergrund_starten(eintraege: Vec<Eintrag>) {
    if eintraege.is_empty() || LAEUFT.swap(true, Ordering::SeqCst) {
        return;
    }
    let ok = std::thread::Builder::new()
        .name("narsil-beobachter".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            for e in eintraege {
                if !(e.adresse.starts_with("http://") || e.adresse.starts_with("https://")) {
                    continue;
                }
                let w = wert_holen(&e.adresse, &e.pfad);
                if let Ok(mut g) = ergebnisse().lock() {
                    g.push((e.id, w));
                }
            }
            LAEUFT.store(false, Ordering::SeqCst);
        })
        .is_ok();
    if !ok {
        LAEUFT.store(false, Ordering::SeqCst);
    }
}

/// Die fertigen Ergebnisse abholen.
pub fn ergebnisse_abholen() -> Vec<(u64, Result<String, String>)> {
    match ergebnisse().lock() {
        Ok(mut g) => std::mem::take(&mut *g),
        Err(_) => Vec::new(),
    }
}

impl crate::Sitzung {
    /// Das Element unter einem Fensterpunkt: (Pfad, Wert).
    pub fn element_unter(&self, x: f32, y: f32) -> Option<(String, String)> {
        let a = self.angezeigt.as_ref()?;
        let y_seite = y - self.chrome.seiten_oberkante() as f32 + a.versatz as f32;
        fn lauf(bx: &narsil_layout::LayoutBox, x: f32, y: f32) -> Option<narsil_dom::NodeId> {
            let r = bx.border_box;
            let drin = x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height;
            let mut t = drin.then_some(bx.node);
            for k in &bx.children {
                if let Some(n) = lauf(k, x, y) {
                    t = Some(n);
                }
            }
            t
        }
        let knoten = lauf(&a.baum.root, x, y_seite)?;
        let ord = *a.seite.dom_zu_ord.get(&(knoten.index() as u32))?;
        let st = super::baum::stellen(&a.html);
        let mut e = super::baum::element_von(&st, ord)?;
        // Ein Element ohne Text (Bildrahmen, leere Zelle): eine Ebene hoeher.
        for _ in 0..3 {
            if !super::baum::text_von(&st, e, Some(&a.patch)).is_empty() {
                break;
            }
            e = st[e].eltern.and_then(|p| super::baum::element_von(&st, p))?;
        }
        let wert = super::baum::text_von(&st, e, Some(&a.patch));
        if wert.is_empty() {
            return None;
        }
        let pfad = super::baum::pfad_zu(&a.html, st[e].knoten)?;
        Some((pfad, wert))
    }

    /// Aus dem Rechtsklick-Menue: das Element unter (x, y) beobachten.
    pub fn element_beobachten(&mut self, x: f32, y: f32) {
        let adresse = self.browser.adresse().to_string();
        let Some((pfad, wert)) = self.element_unter(x, y) else {
            self.meldung = Some(narsil_i18n::tr("Element watcher: there is no text under the pointer").into());
            self.schmutzig = true;
            return;
        };
        self.meldung = Some(match hinzufuegen(&adresse, &pfad, &wert) {
            Ok(e) => narsil_i18n::trf!("Element watcher: “{}” is being watched (list: narsil://beobachter)", e.name),
            Err(f) => narsil_i18n::trf!("Element watcher: {}", f),
        });
        self.schmutzig = true;
    }

    /// Der Wert eines Eintrags in der offenen Seite (Baum plus Patch).
    fn wert_offen(&self, pfad: &str) -> Option<String> {
        let a = self.angezeigt.as_ref()?;
        let k = super::baum::finde(&a.html, pfad)?;
        let st = super::baum::stellen(&a.html);
        let s = st.iter().find(|s| std::ptr::eq(s.knoten, k))?;
        Some(super::baum::text_von(&st, s.ord, Some(&a.patch)))
    }

    /// Der Takt des Waechters (aus `features_tick`).
    pub fn beobachter_tick(&mut self, jetzt: std::time::Instant) -> bool {
        let mut geaendert = false;
        // 1. Ergebnisse aus dem Hintergrund.
        let fertig = ergebnisse_abholen();
        if !fertig.is_empty() {
            let mut l = laden();
            for (id, w) in fertig {
                if let Some((alt, neu)) = wert_melden(&mut l, id, w) {
                    self.waechter_meldung(&l, id, &alt, &neu);
                    geaendert = true;
                }
            }
            let _ = speichern(&l);
        }
        // 2. Die offene Seite.
        if self.features.beobachtung_offen.is_none_or(|t| jetzt.duration_since(t) >= TAKT_OFFEN) {
            self.features.beobachtung_offen = Some(jetzt);
            let adresse = self.browser.adresse().to_string();
            let mut l = laden();
            let passende: Vec<(u64, String)> = l
                .iter()
                .filter(|e| gleiche_seite(&e.adresse, &adresse))
                .map(|e| (e.id, e.pfad.clone()))
                .collect();
            let mut aenderung = false;
            for (id, p) in passende {
                if let Some(w) = self.wert_offen(&p) {
                    if w.is_empty() {
                        continue;
                    }
                    if let Some((alt, neu)) = wert_melden(&mut l, id, Ok(w)) {
                        self.waechter_meldung(&l, id, &alt, &neu);
                        aenderung = true;
                    }
                }
            }
            if aenderung {
                let _ = speichern(&l);
                geaendert = true;
            }
        }
        // 3. Den Hintergrund anstossen.
        let takt = std::time::Duration::from_secs(takt_minuten() * 60);
        let faellig = JETZT_PRUEFEN.swap(false, Ordering::SeqCst)
            || self.features.beobachtung_hintergrund.is_none_or(|t| jetzt.duration_since(t) >= takt);
        if faellig {
            self.features.beobachtung_hintergrund = Some(jetzt);
            hintergrund_starten(laden());
        }
        geaendert
    }

    fn waechter_meldung(&mut self, l: &[Eintrag], id: u64, alt: &str, neu: &str) {
        let Some(e) = l.iter().find(|e| e.id == id) else { return };
        self.features.hinweis(
            narsil_i18n::trf!(
                "Element watcher: {} on {} — “{}” → “{}”",
                kuerzen(&e.name, 30),
                super::host_von(&e.adresse),
                kuerzen(alt, 40),
                kuerzen(neu, 40)
            ),
            Some("narsil://beobachter".into()),
        );
        self.schmutzig = true;
    }
}

/// Die Liste fuer `narsil://beobachter`.
pub fn uebersicht() -> String {
    let l = laden();
    let mut h = format!(
        "<form action=\"narsil://beobachter\" method=\"get\"><input type=\"hidden\" name=\"aktion\" value=\"takt\">\
         <label>{} <input name=\"minuten\" value=\"{}\" size=\"4\"> {}</label> \
         <button type=\"submit\">{}</button></form>\
         <p><a href=\"narsil://beobachter?aktion=jetzt\">{}</a></p>",
        super::html(narsil_i18n::tr("Check in the background every")),
        takt_minuten(),
        super::html(narsil_i18n::tr("minutes")),
        super::html(narsil_i18n::tr("Apply")),
        super::html(narsil_i18n::tr("Check all now"))
    );
    if l.is_empty() {
        h.push_str(&format!(
            "<p>{}</p>",
            super::html(narsil_i18n::tr("Nothing watched yet. Right-click a value on a page → “Watch element”."))
        ));
        return h;
    }
    h.push_str(&format!(
        "<table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th></th></tr>",
        super::html(narsil_i18n::tr("Element")),
        super::html(narsil_i18n::tr("Page")),
        super::html(narsil_i18n::tr("Value")),
        super::html(narsil_i18n::tr("Last checked"))
    ));
    for e in &l {
        let neu = if e.geaendert > 0 {
            format!(
                "<b>{}</b><br><small>{}</small>",
                super::html(&e.wert),
                super::html(&narsil_i18n::trf!("before: {} · changed {}", e.alter_wert, super::zeit_text(e.geaendert)))
            )
        } else {
            super::html(&e.wert)
        };
        h.push_str(&format!(
            "<tr><td>{}<br><small>{}</small></td><td><a href=\"{}\">{}</a></td><td>{}{}</td><td>{}</td>\
             <td><a href=\"narsil://beobachter?aktion=weg&amp;id={}\">{}</a></td></tr>",
            super::html(&e.name),
            super::html(&e.pfad),
            super::html(&e.adresse),
            super::html(&super::host_von(&e.adresse)),
            neu,
            if e.fehler.is_empty() { String::new() } else { format!("<br><small>{}</small>", super::html(&narsil_i18n::trf!("Error: {}", e.fehler))) },
            super::zeit_text(e.geprueft),
            e.id,
            super::html(narsil_i18n::tr("Remove"))
        ));
    }
    h.push_str("</table>");
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn testordner() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("narsil-test-beob-{}-{:?}", std::process::id(), std::thread::current().id()));
        super::super::ordner_fuer_faden(Some(d.clone()));
        d
    }

    #[test]
    fn die_liste_merkt_aenderungen() {
        let d = testordner();
        let e = hinzufuegen("https://shop.example/p", "#preis", "19,99 €").expect("dazu");
        let mut l = laden();
        assert_eq!(l.len(), 1);
        assert_eq!(wert_melden(&mut l, e.id, Ok("19,99 €".into())), None);
        assert_eq!(
            wert_melden(&mut l, e.id, Ok("17,49 €".into())),
            Some(("19,99 €".into(), "17,49 €".into()))
        );
        speichern(&l).expect("speichern");
        let g = laden();
        assert_eq!(g[0].wert, "17,49 €");
        assert_eq!(g[0].alter_wert, "19,99 €");
        assert!(g[0].geaendert > 0);
        assert!(uebersicht().contains("17,49 €"));
        entfernen(e.id).expect("weg");
        assert!(laden().is_empty());
        super::super::ordner_fuer_faden(None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn der_wert_wird_im_neu_geladenen_html_gefunden() {
        let alt = "<html><body><h1>Shop</h1><div id=\"preis\"><span>19,99</span> €</div></body></html>";
        let neu = "<html><body><h1>Shop</h1><p>Neu im Angebot!</p><div id=\"preis\"><span>17,49</span> €</div></body></html>";
        assert_eq!(wert_im_html(alt, "#preis").as_deref(), Ok("19,99 €"));
        assert_eq!(wert_im_html(neu, "#preis").as_deref(), Ok("17,49 €"));
        assert!(wert_im_html(neu, "#gibtsnicht").is_err());
    }

    /// Ende zu Ende im Browser: Element unter dem Zeiger waehlen, die Seite
    /// aendert sich per Skript-Patch, der Waechter meldet es.
    #[test]
    fn rechtsklick_beobachten_und_aenderung_melden() {
        let d = testordner();
        let mut s = crate::Sitzung::neu(900, 700);
        let html = "<html><body><h1>Ticket</h1><p id=\"status\">Wartend</p></body></html>";
        let a = s.aus_html(html, "https://tickets.example/42", 900, 700).expect("Seite");
        s.browser.anwenden(&narsil_browser::Navigation::To("https://tickets.example/42".into()));
        s.angezeigt = Some(a);
        // Den Punkt des Textes „Wartend" suchen.
        let a = s.angezeigt.as_ref().unwrap();
        let mut punkt = None;
        fn suche(bx: &narsil_layout::LayoutBox, a: &crate::sitzung::Angezeigt, raus: &mut Option<(f32, f32)>) {
            if let Some(n) = a.seite.document.get(bx.node)
                && n.text.as_deref().is_some_and(|t| t.contains("Wartend"))
            {
                *raus = Some((bx.border_box.x + 2.0, bx.border_box.y + 2.0));
            }
            for k in &bx.children {
                suche(k, a, raus);
            }
        }
        suche(&a.baum.root, a, &mut punkt);
        let (x, y) = punkt.expect("Text im Layout");
        let y_fenster = y + s.chrome.seiten_oberkante() as f32;
        s.element_beobachten(x, y_fenster);
        let l = laden();
        assert_eq!(l.len(), 1, "{:?}", s.meldung);
        assert_eq!(l[0].wert, "Wartend");
        assert_eq!(l[0].pfad, "#status");
        // Ein Skript aendert den Text (Patch).
        let st = super::super::baum::stellen(&s.angezeigt.as_ref().unwrap().html);
        let ord = st.iter().find(|x| matches!(x.knoten, narsil_fmt_html::HtmlNode::Text(t) if t.contains("Wartend"))).expect("Text").ord;
        s.angezeigt.as_mut().unwrap().patch.text.insert(ord, "Du bist dran".into());
        s.beobachter_tick(std::time::Instant::now());
        let l = laden();
        assert_eq!(l[0].wert, "Du bist dran");
        let h = s.features.hinweis.as_ref().expect("Hinweis");
        assert!(h.text.contains("Du bist dran"), "{}", h.text);
        super::super::ordner_fuer_faden(None);
        let _ = std::fs::remove_dir_all(d);
    }
}
