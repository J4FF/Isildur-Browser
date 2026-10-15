//! Release 08.10.2026: FEATURE 1 (Sitzungs-Schnappschuss & Absturzschutz)
//! und FEATURE 5 (Tabs einfrieren).
//!
//! # Was gesichert wird
//!
//! Je Tab: Adresse, Titel, Scrollposition und der **DOM-Stand** — der
//! geparste Baum mit allem, was Skripte und Eingaben geaendert haben
//! (`baum::gepatcht`: Texte, Attribute, Klassen, Stile, eingetippte Werte,
//! entfernte und neue Knoten), als HTML ohne `<script>`. Externe
//! Stylesheets stehen darin schon als `<style>`, die Seite kommt also ohne
//! Abruf des Dokuments und ohne Stylesheet-Abrufe zurueck.
//!
//! # Das Dateiformat (`RWSS1`)
//!
//! ```text
//! RWSS1 <crc32 hex> <laenge>\n<deflate(nutzlast)>
//! nutzlast = "zeit\t<s>\naktiv\t<n>\n" + je Tab
//!            "tab\t<adresse>\t<titel>\t<versatz>\t<bytes>\n" <html> "\n"
//! ```
//!
//! Die Pruefsumme schuetzt vor einer halb geschriebenen Datei (Absturz
//! waehrend des Schreibens); geschrieben wird ohnehin atomar.
//!
//! # Was nicht geht (ehrlich)
//!
//! Der JavaScript-Heap der eigenen Engine (Variablen, Closures, Timer) und
//! offene Verbindungen lassen sich nicht serialisieren. Der wiederhergestellte
//! Tab zeigt den gesicherten Stand ohne laufende Skripte; F5 laedt live.

use std::path::PathBuf;

use super::{lesen_text, ordner, schreiben};

/// M4: hoechstens so viele Bytes HTML je Tab.
pub const MAX_HTML_TAB: usize = 6 * 1024 * 1024;
/// M4: hoechstens so viele Bytes Nutzlast insgesamt (entpackt).
pub const MAX_NUTZLAST: usize = 64 * 1024 * 1024;
/// Wie oft gesichert wird (nur bei Aenderung).
pub const TAKT: std::time::Duration = std::time::Duration::from_secs(20);
/// Feature 5: nach so langer Zeit im Hintergrund wird ein Tab eingefroren.
pub const FROST_NACH: std::time::Duration = std::time::Duration::from_secs(600);

/// Der gesicherte Stand eines Tabs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TabStand {
    pub adresse: String,
    pub titel: String,
    pub versatz: u32,
    /// Der DOM-Stand als HTML (leer = nur die Adresse).
    pub html: String,
}

/// Ein Schnappschuss aller Tabs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Schnappschuss {
    pub zeit: i64,
    pub aktiv: usize,
    pub tabs: Vec<TabStand>,
}

fn reinigen(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

impl Schnappschuss {
    /// Die unkomprimierte Nutzlast.
    pub fn nutzlast(&self) -> Vec<u8> {
        let mut p = format!("zeit\t{}\naktiv\t{}\n", self.zeit, self.aktiv).into_bytes();
        for t in &self.tabs {
            let html = if t.html.len() > MAX_HTML_TAB { "" } else { t.html.as_str() };
            p.extend_from_slice(
                format!(
                    "tab\t{}\t{}\t{}\t{}\n",
                    reinigen(&t.adresse),
                    reinigen(&t.titel),
                    t.versatz,
                    html.len()
                )
                .as_bytes(),
            );
            p.extend_from_slice(html.as_bytes());
            p.push(b'\n');
        }
        p
    }

    /// Die Datei: Kopf mit Pruefsumme und Laenge, dann deflate.
    pub fn kodieren(&self) -> Vec<u8> {
        let p = self.nutzlast();
        let crc = narsil_compress::pruefsummen::crc32(&p);
        let mut d = format!("RWSS1 {crc:08x} {}\n", p.len()).into_bytes();
        d.extend_from_slice(&narsil_compress::deflate(&p));
        d
    }

    /// Eine Datei lesen und pruefen.
    pub fn dekodieren(d: &[u8]) -> Result<Schnappschuss, String> {
        let kopf_ende = d.iter().position(|b| *b == b'\n').ok_or("kein Kopf")?;
        let kopf = std::str::from_utf8(&d[..kopf_ende]).map_err(|_| "Kopf kein Text")?;
        let mut teile = kopf.split(' ');
        if teile.next() != Some("RWSS1") {
            return Err("kein Narsil-Schnappschuss".into());
        }
        let crc = u32::from_str_radix(teile.next().ok_or("ohne Pruefsumme")?, 16).map_err(|_| "Pruefsumme kaputt")?;
        let laenge: usize = teile.next().ok_or("ohne Laenge")?.parse().map_err(|_| "Laenge kaputt")?;
        if laenge > MAX_NUTZLAST {
            return Err("Schnappschuss zu gross".into());
        }
        let mut p = Vec::new();
        narsil_compress::deflate_entpacke(&d[kopf_ende + 1..], &mut p, MAX_NUTZLAST)
            .map_err(|e| format!("entpacken: {e:?}"))?;
        if p.len() != laenge || narsil_compress::pruefsummen::crc32(&p) != crc {
            return Err("Pruefsumme stimmt nicht (Datei unvollstaendig)".into());
        }
        Self::aus_nutzlast(&p)
    }

    fn aus_nutzlast(p: &[u8]) -> Result<Schnappschuss, String> {
        let mut s = Schnappschuss::default();
        let mut i = 0usize;
        let zeile = |i: &mut usize| -> Option<String> {
            let rest = p.get(*i..)?;
            let e = rest.iter().position(|b| *b == b'\n')?;
            let z = String::from_utf8_lossy(&rest[..e]).into_owned();
            *i += e + 1;
            Some(z)
        };
        while let Some(z) = zeile(&mut i) {
            let f: Vec<&str> = z.split('\t').collect();
            match f.first().copied() {
                Some("zeit") => s.zeit = f.get(1).and_then(|x| x.parse().ok()).unwrap_or(0),
                Some("aktiv") => s.aktiv = f.get(1).and_then(|x| x.parse().ok()).unwrap_or(0),
                Some("tab") if f.len() >= 5 => {
                    let n: usize = f[4].parse().map_err(|_| "Tablaenge kaputt")?;
                    let html = p.get(i..i + n).ok_or("Tab abgeschnitten")?;
                    i += n + 1;
                    s.tabs.push(TabStand {
                        adresse: f[1].to_string(),
                        titel: f[2].to_string(),
                        versatz: f[3].parse().unwrap_or(0),
                        html: String::from_utf8_lossy(html).into_owned(),
                    });
                    if s.tabs.len() > narsil_browser::MAX_TABS {
                        break;
                    }
                }
                _ => {}
            }
        }
        Ok(s)
    }
}

/// Die Rohdaten eines Tabs, die im Hauptfaden billig zu greifen sind.
pub struct Roh {
    pub adresse: String,
    pub titel: String,
    pub versatz: u32,
    pub html: Option<(std::sync::Arc<Vec<narsil_fmt_html::HtmlNode>>, narsil_page::Patch)>,
}

impl Roh {
    /// Den Stand bauen (teuer: Baum durchgehen, serialisieren).
    pub fn stand(&self) -> TabStand {
        let html = match &self.html {
            Some((baum, patch)) => {
                let t = narsil_fmt_html::serialisiere(&super::baum::gepatcht(baum, patch));
                if t.len() > MAX_HTML_TAB { String::new() } else { t }
            }
            None => String::new(),
        };
        TabStand { adresse: self.adresse.clone(), titel: self.titel.clone(), versatz: self.versatz, html }
    }
}

/// Die Datei des Schnappschusses.
pub fn pfad() -> PathBuf {
    ordner().join("schnappschuss.rwss")
}

/// Die Marke „der Browser laeuft" (bleibt nach einem Absturz liegen).
pub fn marke() -> PathBuf {
    ordner().join("laeuft.marke")
}

/// Beim Start: lag die Marke noch da (= letzter Lauf ist abgestuerzt)?
/// Danach wird sie neu gesetzt.
pub fn start_marke() -> bool {
    let m = marke();
    let abgestuerzt = m.exists();
    let _ = schreiben(&m, format!("{}\n", std::process::id()).as_bytes());
    abgestuerzt
}

/// Beim sauberen Beenden: die Marke entfernen.
pub fn marke_loeschen() {
    let _ = std::fs::remove_file(marke());
}

/// Den gesicherten Schnappschuss lesen.
pub fn lesen() -> Result<Schnappschuss, String> {
    let d = std::fs::read(pfad()).map_err(|e| format!("kein Schnappschuss: {e}"))?;
    Schnappschuss::dekodieren(&d)
}

/// Die Datei eines eingefrorenen Tabs.
pub fn frost_pfad(tab: u64) -> PathBuf {
    ordner().join("eingefroren").join(format!("tab_{tab}.rwss"))
}

/// Ob gerade ein Sichern im Hintergrund laeuft.
static SICHERT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl crate::Sitzung {
    fn roh_von_tab(&self, nr: usize) -> Roh {
        let a = if nr == self.browser.aktiver_tab_nr() {
            self.angezeigt.as_ref()
        } else {
            self.tabs.get(nr).and_then(|t| t.as_ref())
        };
        Roh {
            adresse: self.browser.adresse_von_tab(nr).to_string(),
            titel: a.map(|a| a.seite.titel.clone()).unwrap_or_default(),
            versatz: a.map_or(0, |a| a.versatz),
            html: a
                .filter(|a| a.content_type.is_empty() || a.content_type.starts_with("text/html"))
                .map(|a| (a.html.clone(), a.patch.clone())),
        }
    }

    /// Eine billige Pruefsumme ueber das, was sich aendern kann.
    fn stand_summe(&self) -> u32 {
        let mut s = format!("{}|", self.browser.aktiver_tab_nr());
        for nr in 0..self.browser.tabs() {
            let r = self.roh_von_tab(nr);
            s.push_str(&r.adresse);
            s.push('|');
            s.push_str(&r.versatz.to_string());
            if let Some((baum, p)) = &r.html {
                s.push_str(&format!(
                    "|{}|{}|{}|{}|{}",
                    std::sync::Arc::as_ptr(baum) as usize,
                    p.text.len(),
                    p.attribut.len(),
                    p.weg.len(),
                    p.neu.len()
                ));
                let mut werte: Vec<String> = p.attribut.iter().map(|((o, n), w)| format!("{o}{n}{w}")).collect();
                werte.sort();
                s.push_str(&werte.join(","));
            }
        }
        narsil_compress::pruefsummen::crc32(s.as_bytes())
    }

    /// Feature 1: einen Schnappschuss im Hintergrund sichern, wenn sich
    /// seit dem letzten etwas geaendert hat. `zwingend` sichert immer und
    /// wartet (beim Beenden).
    pub fn schnappschuss_sichern(&mut self, zwingend: bool) {
        let summe = self.stand_summe();
        if !zwingend && summe == self.features.schnappschuss_summe {
            return;
        }
        if !zwingend && SICHERT.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        self.features.schnappschuss_summe = summe;
        let roh: Vec<Roh> = (0..self.browser.tabs()).map(|nr| self.roh_von_tab(nr)).collect();
        let eingefroren: Vec<Option<PathBuf>> = (0..self.browser.tabs())
            .map(|nr| {
                self.browser
                    .tab_id_an_position(nr)
                    .and_then(|t| self.features.eingefroren.get(&t.0).cloned())
            })
            .collect();
        let aktiv = self.browser.aktiver_tab_nr();
        let ziel = pfad();
        let arbeit = move || {
            let tabs: Vec<TabStand> = roh
                .iter()
                .zip(eingefroren.iter())
                .map(|(r, frost)| match frost {
                    Some(p) if r.html.is_none() => std::fs::read(p)
                        .ok()
                        .and_then(|d| Schnappschuss::dekodieren(&d).ok())
                        .and_then(|s| s.tabs.into_iter().next())
                        .unwrap_or_else(|| r.stand()),
                    _ => r.stand(),
                })
                .collect();
            let s = Schnappschuss { zeit: super::jetzt_s(), aktiv, tabs };
            let _ = schreiben(&ziel, &s.kodieren());
            SICHERT.store(false, std::sync::atomic::Ordering::SeqCst);
        };
        if zwingend {
            arbeit();
        } else if std::thread::Builder::new()
            .name("narsil-schnappschuss".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(arbeit)
            .is_err()
        {
            SICHERT.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Aus einem gesicherten Stand eine Seite bauen (ohne Netz fuer das
    /// Dokument, ohne Skripte).
    pub fn stand_anzeigen(&self, t: &TabStand) -> Option<crate::sitzung::Angezeigt> {
        if t.html.is_empty() {
            return None;
        }
        let mut a = self.aus_html(&t.html, &t.adresse, self.breite.max(1), self.sicht_hoehe()).ok()?;
        a.versatz = t.versatz.min(a.max_versatz(self.sicht_hoehe()));
        a.meldungen.push(narsil_i18n::tr("Restored state — scripts paused, F5 reloads live").into());
        Some(a)
    }

    /// Feature 1: den gesicherten Schnappschuss als Tabs wiederherstellen.
    pub fn schnappschuss_wiederherstellen(&mut self) -> Result<usize, String> {
        let s = lesen()?;
        if s.tabs.is_empty() {
            return Err(narsil_i18n::tr("the snapshot is empty").into());
        }
        // Der erste Tab nimmt den aktuellen Platz ein, die weiteren kommen dazu.
        let mut erster = true;
        for t in s.tabs.iter().take(narsil_browser::MAX_TABS) {
            if !erster && !self.tab_neu() {
                break;
            }
            erster = false;
            self.browser.anwenden(&narsil_browser::Navigation::To(t.adresse.clone()));
            self.chrome.setze_adresse(&t.adresse);
            self.angezeigt = self.stand_anzeigen(t);
        }
        let ziel = s.aktiv.min(self.browser.tabs().saturating_sub(1));
        self.tab_wechseln_zu(ziel as u32);
        if self.angezeigt.is_none() && !self.browser.adresse().is_empty() {
            let a = self.browser.adresse().to_string();
            let _ = self.gehe_zu(&a);
        }
        self.tabs_anzeigen();
        let n = s.tabs.len();
        self.features.hinweis(
            narsil_i18n::trf!("Snapshot of {} restored: {} tab(s)", super::zeit_text(s.zeit), n),
            None,
        );
        self.schmutzig = true;
        Ok(n)
    }

    /// Feature 5: den Tab an Position `nr` einfrieren (nicht den aktiven).
    pub fn tab_einfrieren(&mut self, nr: usize) -> bool {
        if nr == self.browser.aktiver_tab_nr() {
            return false;
        }
        let Some(id) = self.browser.tab_id_an_position(nr) else {
            return false;
        };
        let Some(a) = self.tabs.get_mut(nr).and_then(Option::take) else {
            return false;
        };
        let r = Roh {
            adresse: self.browser.adresse_von_tab(nr).to_string(),
            titel: a.seite.titel.clone(),
            versatz: a.versatz,
            html: Some((a.html.clone(), a.patch.clone())),
        };
        let stand = r.stand();
        if stand.html.is_empty() {
            self.tabs[nr] = Some(a);
            return false;
        }
        let s = Schnappschuss { zeit: super::jetzt_s(), aktiv: 0, tabs: vec![stand] };
        let p = frost_pfad(id.0);
        match schreiben(&p, &s.kodieren()) {
            Ok(()) => {
                self.features.eingefroren.insert(id.0, p);
                drop(a);
                true
            }
            Err(_) => {
                self.tabs[nr] = Some(a);
                false
            }
        }
    }

    /// Feature 5: den aktiven Tab sofort einfrieren (aus dem Tabmenue): er
    /// wird dazu verlassen, wenn es einen anderen gibt.
    pub fn aktiven_tab_einfrieren(&mut self) {
        let nr = self.browser.aktiver_tab_nr();
        if self.browser.tabs() < 2 {
            self.meldung = Some(narsil_i18n::tr("Freeze tabs: the only tab stays active").into());
            return;
        }
        let ziel = if nr + 1 < self.browser.tabs() { nr + 1 } else { nr - 1 };
        self.tab_wechseln_zu(ziel as u32);
        self.meldung = Some(if self.tab_einfrieren(nr) {
            narsil_i18n::trf!("Tab {} frozen — it comes back without reloading", nr + 1)
        } else {
            narsil_i18n::tr("This tab cannot be frozen (not an HTML page)").to_string()
        });
        self.tabs_anzeigen();
        self.schmutzig = true;
    }

    /// Feature 5: einen eingefrorenen Tab beim Aktivieren auftauen.
    pub fn auftauen(&mut self, nr: usize) -> Option<crate::sitzung::Angezeigt> {
        let id = self.browser.tab_id_an_position(nr)?;
        let p = self.features.eingefroren.remove(&id.0)?;
        let d = std::fs::read(&p).ok();
        let _ = std::fs::remove_file(&p);
        let s = Schnappschuss::dekodieren(&d?).ok()?;
        let t = s.tabs.into_iter().next()?;
        // Die Adresse muss noch stimmen (der Tab koennte navigiert sein).
        if t.adresse != self.browser.adresse_von_tab(nr) {
            return None;
        }
        self.stand_anzeigen(&t)
    }

    /// Feature 5: Tabs einfrieren, die lange im Hintergrund lagen.
    pub fn frost_pruefen(&mut self, jetzt: std::time::Instant) {
        let aktiv = self.browser.aktiver_tab_nr();
        for nr in 0..self.browser.tabs() {
            if nr == aktiv || self.tabs.get(nr).is_none_or(Option::is_none) {
                continue;
            }
            let Some(id) = self.browser.tab_id_an_position(nr) else { continue };
            let seit = *self.features.aktiv_seit.entry(id.0).or_insert(jetzt);
            if jetzt.duration_since(seit) >= FROST_NACH {
                self.tab_einfrieren(nr);
            }
        }
    }
}

/// Die Uebersicht fuer `narsil://schnappschuss`.
pub fn uebersicht() -> String {
    let stand = match lesen() {
        Ok(s) => {
            let mut h = format!(
                "<p>{}</p><ul>",
                narsil_i18n::trf!(
                    "Last snapshot: <b>{}</b> (UTC), {} tab(s), {} KB on disk.",
                    super::zeit_text(s.zeit),
                    s.tabs.len(),
                    std::fs::metadata(pfad()).map(|m| m.len() / 1024).unwrap_or(0)
                )
            );
            for t in &s.tabs {
                h.push_str(&format!(
                    "<li>{} — <small>{}</small>{}</li>",
                    super::html(if t.titel.is_empty() { &t.adresse } else { &t.titel }),
                    super::html(&t.adresse),
                    if t.html.is_empty() { format!(" <small>{}</small>", super::html(narsil_i18n::tr("(address only)"))) } else { String::new() }
                ));
            }
            h.push_str(&format!(
                "</ul><p><a href=\"narsil://schnappschuss?aktion=wiederherstellen\">{}</a> {}</p>",
                super::html(narsil_i18n::tr("Restore this state now")),
                super::html(narsil_i18n::tr("(opens the tabs in addition)"))
            ));
            h
        }
        // Release 09.10.2026: fehlt die Datei, ist das der Normalfall (noch
        // nichts gesichert) und kein Fehler — vorher stand die rohe
        // Betriebssystem-Meldung („os error 3") auf der Seite.
        Err(_) if !pfad().exists() => {
            format!(
                "<p>{}</p>",
                super::html(narsil_i18n::tr("No snapshot yet. Narsil saves every 20 seconds as soon as a tab changes."))
            )
        }
        Err(e) => format!("<p>{}</p>", super::html(&narsil_i18n::trf!("The snapshot cannot be read ({}).", e))),
    };
    let marke = if lesen_text(&marke()).is_empty() {
        String::new()
    } else {
        format!("<p><small>{}</small></p>", super::html(narsil_i18n::tr("The browser is running (crash marker set).")))
    };
    // Ein Pfad mit EINER Sorte Trenner (vorher `C:/…/profil\features\…`).
    let datei = pfad().display().to_string();
    let datei = if cfg!(windows) { datei.replace('/', "\\") } else { datei };
    format!("{stand}<p><small>{}</small></p>{marke}", super::html(&narsil_i18n::trf!("File: {}", datei)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn das_format_ueberlebt_den_weg_und_merkt_kaputte_dateien() {
        let s = Schnappschuss {
            zeit: 1_790_000_000,
            aktiv: 1,
            tabs: vec![
                TabStand { adresse: "https://a.example/".into(), titel: "A\tTitel".into(), versatz: 120, html: "<p>Hallo\nWelt ä</p>".into() },
                TabStand { adresse: "about:einstellungen".into(), titel: String::new(), versatz: 0, html: String::new() },
            ],
        };
        let d = s.kodieren();
        assert!(d.starts_with(b"RWSS1 "));
        let z = Schnappschuss::dekodieren(&d).expect("dekodieren");
        assert_eq!(z.tabs[0].html, s.tabs[0].html);
        assert_eq!(z.tabs[0].titel, "A Titel");
        assert_eq!(z.tabs[0].versatz, 120);
        assert_eq!(z.aktiv, 1);
        assert_eq!(z.tabs[1].adresse, "about:einstellungen");
        // Ein abgeschnittener Schnappschuss wird erkannt.
        assert!(Schnappschuss::dekodieren(&d[..d.len() - 3]).is_err());
        let mut kaputt = d.clone();
        let n = kaputt.len();
        kaputt[n - 1] ^= 0x55;
        assert!(Schnappschuss::dekodieren(&kaputt).is_err());
        assert!(Schnappschuss::dekodieren(b"etwas anderes\n").is_err());
        // Komprimiert: eine wiederholte Seite wird deutlich kleiner.
        let gross = Schnappschuss { zeit: 1, aktiv: 0, tabs: vec![TabStand { adresse: "https://a/".into(), titel: "t".into(), versatz: 0, html: "<p>abc</p>".repeat(5000) }] };
        assert!(gross.kodieren().len() < 50_000 / 4);
    }

    /// Ende zu Ende: Seite mit Skript und Eingabe → Schnappschuss →
    /// Wiederherstellung zeigt Skript-Stand und Eingabe, ohne Skripte.
    #[test]
    fn der_dom_stand_mit_skript_und_eingabe_kommt_zurueck() {
        let dir = std::env::temp_dir().join(format!("narsil-test-schnapp-{}-{:?}", std::process::id(), std::thread::current().id()));
        super::super::ordner_fuer_faden(Some(dir.clone()));
        // Release 09.10.2026: ohne Datei ein ruhiger Satz, keine OS-Meldung.
        let leer = uebersicht();
        assert!(leer.contains("No snapshot yet."), "{leer}");
        assert!(!leer.contains("os error"), "{leer}");
        let mut s = crate::Sitzung::neu(900, 700);
        let html = "<html><head><title>Formular</title></head><body><p id=\"z\">vorher</p>\
                    <input name=\"q\" value=\"\"><script>document.getElementById('z').textContent='vom Skript';</script></body></html>";
        let mut a = s.aus_html(html, "https://a.example/form", 900, 700).expect("Seite");
        // Die Eingabe des Nutzers steht im Patch (wie `werte_schreiben`).
        let ord_input = super::super::baum::stellen(&a.html)
            .iter()
            .find(|x| matches!(x.knoten, narsil_fmt_html::HtmlNode::Element { tag, .. } if tag == "input"))
            .expect("input")
            .ord;
        a.patch.attribut.insert((ord_input, "value".into()), "meine Eingabe".into());
        a.versatz = 0;
        s.browser.anwenden(&narsil_browser::Navigation::To("https://a.example/form".into()));
        s.angezeigt = Some(a);
        s.schnappschuss_sichern(true);
        let gelesen = lesen().expect("lesen");
        assert_eq!(gelesen.tabs.len(), 1);
        let t = &gelesen.tabs[0];
        assert!(t.html.contains("vom Skript"), "{}", t.html);
        assert!(t.html.contains("meine Eingabe"), "{}", t.html);
        assert!(!t.html.contains("<script"), "{}", t.html);
        // Wiederherstellen in einer frischen Sitzung.
        let mut neu = crate::Sitzung::neu(900, 700);
        neu.schnappschuss_wiederherstellen().expect("wiederherstellen");
        assert_eq!(neu.browser.adresse(), "https://a.example/form");
        let a = neu.angezeigt.as_ref().expect("angezeigt");
        let texte = narsil_fmt_html::extract_text(&a.html);
        assert!(texte.contains("vom Skript"), "{texte}");
        super::super::ordner_fuer_faden(None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
