//! Release 08.10.2026: DIE ZUSATZFUNKTIONEN aus dem Tab „Features".
//!
//! `narsil-chrome::features` kennt Namen, Schalter und Geometrie; hier steht,
//! was die Funktionen in der eigenen Engine **tun**. Jede Funktion hat ein
//! eigenes Untermodul; gemeinsam sind nur der Profilordner, ein kleines
//! Zeilenformat fuer die Listen, Datumsrechnung, HTML-Escape und der
//! Zustand je Fenster ([`Zustand`]).
//!
//! Alle Daten bleiben lokal im Profil (`<profil>/features/`), im Testlauf
//! im Temp-Profil (siehe `narsil_chrome::einstellungen::pfad`).

pub mod baum;
pub mod beobachter;
pub mod belege;
pub mod formulare;
pub mod fristen;
pub mod kopfregeln;
pub mod netz;
pub mod ruhe;
pub mod schnappschuss;
pub mod seiten;
pub mod url;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub use narsil_chrome::features::{Feature, Schalter};

/// Der Ordner der Feature-Daten im Profil.
pub fn ordner() -> PathBuf {
    if let Some(eigen) = ORDNER_FUER_TESTS.with(|o| o.borrow().clone()) {
        return eigen;
    }
    narsil_chrome::einstellungen::ordner().join("features")
}

thread_local! {
    /// Tests geben jedem Faden einen eigenen Ordner, damit parallele Tests
    /// sich die Listen nicht gegenseitig ueberschreiben.
    static ORDNER_FUER_TESTS: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Einen eigenen Datenordner fuer diesen Faden setzen (Tests).
pub fn ordner_fuer_faden(p: Option<PathBuf>) {
    ORDNER_FUER_TESTS.with(|o| *o.borrow_mut() = p);
}

/// Eine Datei atomar schreiben (temporaer + Umbenennen), Ordner anlegen.
pub fn schreiben(pfad: &Path, daten: &[u8]) -> Result<(), String> {
    if let Some(e) = pfad.parent() {
        std::fs::create_dir_all(e).map_err(|f| format!("{}: {f}", e.display()))?;
    }
    let tmp = pfad.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, daten).map_err(|f| format!("{}: {f}", tmp.display()))?;
    std::fs::rename(&tmp, pfad).map_err(|f| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {f}", pfad.display())
    })
}

/// M4: die groesste Listendatei, die gelesen wird.
pub const MAX_LISTE: u64 = 4 * 1024 * 1024;

/// Eine Textdatei lesen (fehlend = leer, zu gross = leer).
pub fn lesen_text(pfad: &Path) -> String {
    match std::fs::metadata(pfad) {
        Ok(m) if m.len() <= MAX_LISTE => std::fs::read_to_string(pfad).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Ein Feld fuer eine Tabulatorzeile maskieren (`\t`, `\n`, `\\`).
pub fn maske(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => r.push_str("\\\\"),
            '\t' => r.push_str("\\t"),
            '\n' => r.push_str("\\n"),
            '\r' => {}
            c => r.push(c),
        }
    }
    r
}

/// Die Umkehrung von [`maske`].
pub fn entmaske(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => r.push('\t'),
                Some('n') => r.push('\n'),
                Some('\\') => r.push('\\'),
                Some(o) => {
                    r.push('\\');
                    r.push(o);
                }
                None => r.push('\\'),
            }
        } else {
            r.push(c);
        }
    }
    r
}

/// Eine Tabulatorzeile aus Feldern.
pub fn zeile(felder: &[&str]) -> String {
    felder.iter().map(|f| maske(f)).collect::<Vec<_>>().join("\t")
}

/// Die Felder einer Tabulatorzeile.
pub fn felder(zeile: &str) -> Vec<String> {
    zeile.split('\t').map(entmaske).collect()
}

/// HTML-Escape fuer die eingebauten Seiten.
pub fn html(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => r.push_str("&amp;"),
            '<' => r.push_str("&lt;"),
            '>' => r.push_str("&gt;"),
            '"' => r.push_str("&quot;"),
            '\'' => r.push_str("&#39;"),
            c => r.push(c),
        }
    }
    r
}

/// Jetzt in Sekunden seit 1970.
pub fn jetzt_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Tage seit 1970-01-01 fuer ein Datum (proleptisch gregorianisch).
pub fn tage_aus_datum(j: i64, m: u32, t: u32) -> i64 {
    let (j, m) = if m <= 2 { (j - 1, m + 12) } else { (j, m) };
    let era = j.div_euclid(400);
    let yoe = j - era * 400;
    let doy = (153 * (m as i64 - 3) + 2) / 5 + t as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Das Datum zu Tagen seit 1970.
pub fn datum_aus_tagen(tage: i64) -> (i64, u32, u32) {
    let z = tage + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let j = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let t = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { j + 1 } else { j }, m, t)
}

/// `TT.MM.JJJJ` fuer Sekunden seit 1970 (UTC).
pub fn datum_text(sekunden: i64) -> String {
    let (j, m, t) = datum_aus_tagen(sekunden.div_euclid(86_400));
    // year-month-day in English, day.month.year in German
    narsil_i18n::trf!("{0}-{1}-{2}", j, format!("{m:02}"), format!("{t:02}"))
}

/// `TT.MM.JJJJ hh:mm` fuer Sekunden seit 1970 (UTC).
pub fn zeit_text(sekunden: i64) -> String {
    let rest = sekunden.rem_euclid(86_400);
    format!("{} {:02}:{:02}", datum_text(sekunden), rest / 3600, (rest % 3600) / 60)
}

/// Ein Datum `TT.MM.JJJJ` (oder `JJJJ-MM-TT`) lesen — Sekunden seit 1970.
pub fn datum_lesen(s: &str) -> Option<i64> {
    let s = s.trim();
    let (j, m, t) = if let Some((a, rest)) = s.split_once('-') {
        let (b, c) = rest.split_once('-')?;
        (a.parse().ok()?, b.parse().ok()?, c.parse().ok()?)
    } else {
        let mut it = s.split('.');
        let t = it.next()?.trim().parse().ok()?;
        let m = it.next()?.trim().parse().ok()?;
        let j: i64 = it.next()?.trim().parse().ok()?;
        (if j < 100 { j + 2000 } else { j }, m, t)
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&t) || !(1970..=2200).contains(&j) {
        return None;
    }
    Some(tage_aus_datum(j, m, t) * 86_400)
}

/// Der Host einer Adresse (klein, ohne Port).
pub fn host_von(adresse: &str) -> String {
    let rest = adresse.split_once("://").map_or(adresse, |(_, r)| r);
    let autoritaet = rest.split(['/', '?', '#']).next().unwrap_or("");
    let ohne_nutzer = autoritaet.rsplit('@').next().unwrap_or(autoritaet);
    let host = if ohne_nutzer.starts_with('[') {
        ohne_nutzer.split(']').next().map(|h| format!("{h}]")).unwrap_or_default()
    } else {
        ohne_nutzer.split(':').next().unwrap_or("").to_string()
    };
    host.to_ascii_lowercase()
}

/// Die Abfrage einer eingebauten Adresse als Paare (prozent-dekodiert).
pub fn abfrage(adresse: &str) -> Vec<(String, String)> {
    let Some((_, q)) = adresse.split_once('?') else {
        return Vec::new();
    };
    let q = q.split('#').next().unwrap_or("");
    q.split('&')
        .filter(|t| !t.is_empty())
        .take(64)
        .map(|t| {
            let (k, v) = t.split_once('=').unwrap_or((t, ""));
            (dekodiere(k), dekodiere(v))
        })
        .collect()
}

/// `application/x-www-form-urlencoded` dekodieren.
pub fn dekodiere(s: &str) -> String {
    let plus = s.replace('+', " ");
    narsil_url::prozent_dekodieren(&plus).als_text().map(str::to_string).unwrap_or(plus)
}

/// Prozent-Kodierung fuer einen Abfragewert.
pub fn kodiere(s: &str) -> String {
    let mut r = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            r.push(b as char);
        } else {
            r.push_str(&format!("%{b:02X}"));
        }
    }
    r
}

/// Eine dezente Meldung (Toast) am unteren rechten Rand.
#[derive(Debug, Clone)]
pub struct Hinweis {
    pub text: String,
    /// Optional: eine Adresse, die ein Klick auf den Hinweis oeffnet.
    pub ziel: Option<String>,
    pub seit: Instant,
}

/// Wie lange ein Hinweis steht.
pub const HINWEIS_DAUER: std::time::Duration = std::time::Duration::from_secs(9);

/// Der Zustand der Funktionen je Fenster (Sitzung).
#[derive(Default)]
pub struct Zustand {
    /// Feature 3: die Netzregel je Tab (`TabId.0` -> 0/1/2).
    pub netz: HashMap<u64, u8>,
    /// Feature 5: wann ein Tab zuletzt aktiv war.
    pub aktiv_seit: HashMap<u64, Instant>,
    /// Feature 5: eingefrorene Tabs und ihre Dateien.
    pub eingefroren: HashMap<u64, PathBuf>,
    /// Feature 5: wann zuletzt nach einzufrierenden Tabs gesehen wurde.
    pub frost_pruefung: Option<Instant>,
    /// Feature 1: wann zuletzt gesichert wurde, und die Pruefsumme des Stands.
    pub schnappschuss_zeit: Option<Instant>,
    pub schnappschuss_summe: u32,
    /// Feature 2: wann zuletzt die offene Seite / der Hintergrund geprueft wurde.
    pub beobachtung_offen: Option<Instant>,
    pub beobachtung_hintergrund: Option<Instant>,
    /// Feature 8/10: wann zuletzt erinnert wurde.
    pub erinnerung: Option<Instant>,
    /// Feature 4: der zuletzt eingesetzte Datensatz (fuer das Weiterschalten).
    pub formular_runde: Option<(usize, Instant)>,
    /// Feature 12: der Link unter dem Zeiger und wo der Hinweis steht.
    pub link_tipp: Option<(String, narsil_core::geometry::Point)>,
    /// Feature 12: der zuletzt gezeichnete Text der Vorschau.
    pub link_tipp_stand: Option<(String, String)>,
    /// Die dezente Meldung.
    pub hinweis: Option<Hinweis>,
    /// Feature 8/10: die zuletzt ausgewertete Adresse (nur einmal je Seite).
    pub ausgewertet: String,
    /// Feature 11: Seite, Runde und Ladezeit des Autopiloten.
    pub autopilot: Option<(String, usize, Instant)>,
}

impl Zustand {
    /// Einen Hinweis zeigen.
    pub fn hinweis(&mut self, text: impl Into<String>, ziel: Option<String>) {
        self.hinweis = Some(Hinweis {
            text: text.into(),
            ziel,
            seit: Instant::now(),
        });
    }
}

// ---------------------------------------------------------------------------
// FEATURE 12: DIE LINK-ZIEL-VORSCHAU (Zustand der Aufloesungen)
// ---------------------------------------------------------------------------

/// Der Stand einer Aufloesung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkZiel {
    Laeuft,
    Fertig(String),
    Fehler(String),
}

fn link_ziele() -> &'static std::sync::Mutex<HashMap<String, LinkZiel>> {
    static Z: std::sync::OnceLock<std::sync::Mutex<HashMap<String, LinkZiel>>> = std::sync::OnceLock::new();
    Z.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// M3: hoechstens so viele Aufloesungen im Speicher und gleichzeitig.
pub const MAX_LINKZIELE: usize = 512;
pub const MAX_GLEICHZEITIG: usize = 4;

/// Das Ziel eines Links fuer die Vorschau — sofort, wenn es ohne Netz geht,
/// sonst aus dem Speicher; startet die Aufloesung per HEAD im Hintergrund.
pub fn link_ziel(link: &str, regel: u8) -> LinkZiel {
    let r = url::Regeln::default();
    if !url::ist_kurzlink(link) {
        return LinkZiel::Fertig(url::reinigen(link, &r));
    }
    let Ok(mut g) = link_ziele().lock() else {
        return LinkZiel::Fehler("gesperrt".into());
    };
    if let Some(z) = g.get(link) {
        return z.clone();
    }
    if g.values().filter(|z| **z == LinkZiel::Laeuft).count() >= MAX_GLEICHZEITIG {
        return LinkZiel::Laeuft;
    }
    if g.len() >= MAX_LINKZIELE {
        g.clear();
    }
    g.insert(link.to_string(), LinkZiel::Laeuft);
    drop(g);
    let l = link.to_string();
    let ok = std::thread::Builder::new()
        .name("narsil-linkziel".into())
        .spawn(move || {
            narsil_net::haken::regel_im_faden(Some(narsil_net::haken::Netzregel::aus_zahl(regel)));
            let z = match url::per_head_aufloesen(&l) {
                Ok(ziel) => LinkZiel::Fertig(url::reinigen(&ziel, &url::Regeln::default())),
                Err(e) => LinkZiel::Fehler(e),
            };
            if let Ok(mut g) = link_ziele().lock() {
                g.insert(l, z);
            }
        })
        .is_ok();
    if !ok {
        if let Ok(mut g) = link_ziele().lock() {
            g.remove(link);
        }
    }
    LinkZiel::Laeuft
}

// ---------------------------------------------------------------------------
// DIE EINHAENGUNG IN DIE SITZUNG
// ---------------------------------------------------------------------------

impl crate::Sitzung {
    /// Die prozessweiten Haken nach den Schaltern setzen (Start und nach
    /// jeder Aenderung im Tab „Features").
    pub fn features_anwenden(&mut self) {
        let f = self.einstellungen.features;
        kopfregeln::anwenden(f.an(Feature::Kopfregeln));
        ruhe::anwenden(&f);
        self.netz_anwenden();
    }

    /// Vor einer Navigation: interne Aktionen und Feature 7 (bereinigen,
    /// umschreiben). `None` heisst „erledigt, nicht laden".
    pub fn features_adresse(&mut self, adresse: &str) -> Option<String> {
        let a = adresse.trim();
        if a.starts_with("narsil://schnappschuss") {
            let q = abfrage(a);
            if q.iter().any(|(k, v)| k == "aktion" && v == "wiederherstellen") {
                return Some(match self.schnappschuss_wiederherstellen() {
                    Ok(_) => "narsil://schnappschuss?aktion=fertig".into(),
                    Err(e) => {
                        self.meldung = Some(narsil_i18n::trf!("Snapshot: {}", e));
                        "narsil://schnappschuss".into()
                    }
                });
            }
        }
        if a.split('#').next() == Some("about:einstellungen") && a.ends_with("#features") {
            self.einstell_seite.wechsle(narsil_chrome::einstellungen::Kategorie::Features);
            return Some("about:einstellungen".into());
        }
        if self.einstellungen.features.an(Feature::UrlReinigung)
            && (a.starts_with("http://") || a.starts_with("https://"))
        {
            let sauber = url::reinigen(a, &url::Regeln::laden());
            if sauber != a {
                self.meldung = Some(narsil_i18n::trf!("Link cleaner: {}", sauber));
            }
            return Some(sauber);
        }
        Some(a.to_string())
    }

    /// Feature 7 beim Teilen/Kopieren einer Adresse.
    pub fn features_teilen(&self, adresse: &str) -> String {
        if self.einstellungen.features.an(Feature::UrlReinigung) {
            url::reinigen(adresse, &url::Regeln::laden())
        } else {
            adresse.to_string()
        }
    }

    /// Ein Tab wurde aktiv: Netzregel, Zeitstempel fuer das Einfrieren.
    pub fn features_tab_aktiviert(&mut self) {
        if let Some(id) = self.browser.aktive_tab_id() {
            self.features.aktiv_seit.insert(id.0, Instant::now());
        }
        self.netz_anwenden();
    }

    /// Ein Tab wird geschlossen: seine Feature-Daten aufraeumen.
    pub fn features_tab_weg(&mut self, nr: usize) {
        if let Some(id) = self.browser.tab_id_an_position(nr) {
            self.features.netz.remove(&id.0);
            self.features.aktiv_seit.remove(&id.0);
            if let Some(p) = self.features.eingefroren.remove(&id.0) {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    /// Der Takt aller Features (aus `Sitzung::tick`). `true` = neu zeichnen.
    pub fn features_tick(&mut self, jetzt: Instant) -> bool {
        let mut neu = false;
        if self.features.hinweis.as_ref().is_some_and(|h| jetzt.duration_since(h.seit) >= HINWEIS_DAUER) {
            self.features.hinweis = None;
            neu = true;
        }
        let f = self.einstellungen.features;
        if let Some(id) = self.browser.aktive_tab_id() {
            self.features.aktiv_seit.insert(id.0, jetzt);
        }
        if f.an(Feature::Schnappschuss)
            && !self.laedt
            && self.features.schnappschuss_zeit.is_none_or(|t| jetzt.duration_since(t) >= schnappschuss::TAKT)
        {
            self.features.schnappschuss_zeit = Some(jetzt);
            self.schnappschuss_sichern(false);
        }
        if f.an(Feature::TabEinfrieren)
            && self.features.frost_pruefung.is_none_or(|t| jetzt.duration_since(t) >= std::time::Duration::from_secs(30))
        {
            self.features.frost_pruefung = Some(jetzt);
            self.frost_pruefen(jetzt);
        }
        if f.an(Feature::Beobachter) && !self.laedt && self.beobachter_tick(jetzt) {
            neu = true;
        }
        if f.an(Feature::CookieAutopilot) && !self.laedt {
            self.autopilot_tick(jetzt);
        }
        if (f.an(Feature::Fristen) || f.an(Feature::Belege)) && !self.laedt {
            let vorher = self.features.hinweis.as_ref().map(|h| h.seit);
            self.fristen_und_belege_pruefen();
            self.erinnerungen_pruefen(jetzt);
            if self.features.hinweis.as_ref().map(|h| h.seit) != vorher {
                neu = true;
            }
        }
        if f.an(Feature::LinkZiel) {
            // Neu zeichnen, wenn eine Aufloesung fertig geworden ist.
            let stand = self.link_tipp_text();
            if stand != self.features.link_tipp_stand {
                self.features.link_tipp_stand = stand;
                neu = true;
            }
        }
        neu
    }

    /// Tasten der Features. `true` = verbraucht.
    pub fn features_taste(&mut self, k: narsil_input::KeyEvent) -> bool {
        if k.pressed
            && k.modifiers.ctrl
            && k.modifiers.shift
            && !k.modifiers.alt
            && matches!(k.key, narsil_input::Key::Char('f') | narsil_input::Key::Char('F'))
        {
            self.formular_ausfuellen();
            return true;
        }
        false
    }

    /// Feature 12: der Zeiger bewegt sich ueber der Seite.
    pub fn features_zeiger(&mut self, p: narsil_core::geometry::Point) -> bool {
        if !self.einstellungen.features.an(Feature::LinkZiel) {
            return false;
        }
        let vorher = self.features.link_tipp.as_ref().map(|(l, _)| l.clone());
        let oben = self.chrome.seiten_oberkante() as f32;
        let link = if p.y >= oben && self.menue.is_none() && self.tabmenue.is_none() {
            self.angezeigt.as_ref().and_then(|a| {
                narsil_page::link_unter(&a.seite, &a.baum, (p.x, p.y - oben + a.versatz as f32)).map(|l| l.to_string())
            })
        } else {
            None
        };
        self.features.link_tipp = match link {
            // Derselbe Link: der Hinweis bleibt, wo er aufging.
            Some(l) if vorher.as_deref() == Some(l.as_str()) => self.features.link_tipp.take(),
            Some(l) if url::ist_verdaechtig(&l) => Some((l, p)),
            _ => None,
        };
        let jetzt = self.features.link_tipp.as_ref().map(|(l, _)| l.clone());
        jetzt != vorher
    }

    /// Der Text der Link-Vorschau (zwei Zeilen) — `None` ohne Vorschau.
    pub fn link_tipp_text(&self) -> Option<(String, String)> {
        let (link, _) = self.features.link_tipp.as_ref()?;
        let regel = self.netzregel_aktiver_tab();
        Some(match link_ziel(link, regel) {
            LinkZiel::Fertig(z) if url::ist_kurzlink(link) => (format!("→ {z}"), narsil_i18n::tr("Short link resolved with HEAD (Location header)").into()),
            LinkZiel::Fertig(z) => (format!("→ {z}"), narsil_i18n::tr("Tracking redirect unpacked or cleaned without network").into()),
            LinkZiel::Laeuft => (format!("{link}"), narsil_i18n::tr("The target is being checked with HEAD …").into()),
            LinkZiel::Fehler(e) => (format!("{link}"), narsil_i18n::trf!("Target cannot be resolved: {}", e)),
        })
    }

    /// Ein Klick auf den Hinweis oeffnet sein Ziel. `true` = verbraucht.
    pub fn features_klick(&mut self, p: narsil_core::geometry::Point) -> bool {
        let Some(r) = self.hinweis_rect() else { return false };
        if p.x < r.x || p.x > r.x + r.width || p.y < r.y || p.y > r.y + r.height {
            return false;
        }
        let ziel = self.features.hinweis.take().and_then(|h| h.ziel);
        if let Some(z) = ziel
            && self.tab_neu()
            && let Err(e) = self.gehe_zu(&z)
        {
            self.meldung = Some(e.to_string());
        }
        self.schmutzig = true;
        true
    }

    fn hinweis_rect(&self) -> Option<narsil_core::geometry::Rect> {
        let h = self.features.hinweis.as_ref()?;
        let text: String = h.text.chars().take(MAX_HINWEIS_ZEICHEN).collect();
        let b = (narsil_chrome::features::textbreite(&text, 13.0) + 40).min(self.breite.saturating_sub(24)) as f32;
        let hoehe = 40.0;
        let x = self.breite as f32 - b - 16.0;
        let y = self.hoehe.saturating_sub(narsil_chrome::STATUSLEISTE_HOEHE) as f32 - hoehe - 14.0;
        Some(narsil_core::geometry::Rect::new(x.max(8.0), y.max(0.0), b, hoehe))
    }

    /// Hinweis und Link-Vorschau zeichnen (ganz oben in der Liste).
    pub fn features_zeichnen(&self, liste: &mut narsil_display::DisplayList) {
        use narsil_core::color::Color;
        use narsil_core::geometry::{Point, Rect};
        use narsil_display::DisplayItem;
        let pal = narsil_chrome::thema::Palette::fuer(self.chrome.thema);
        let text = |x: f32, y: f32, t: String, farbe: Color, grad: f32, gewicht: u16| DisplayItem::Text {
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(x, y),
            text: t,
            color: farbe,
            font_size: grad,
            gewicht,
            kursiv: false,
        };
        if let (Some(h), Some(r)) = (self.features.hinweis.as_ref(), self.hinweis_rect()) {
            // Release 08.10.2026 (Glas-Stil): der Hinweis ist ein Glasfeld wie
            // das Kontextmenue, mit einem Akzentpunkt statt des Streifens.
            narsil_chrome::glas::feld(liste, &glas_rechteck(r), narsil_chrome::glas::prozent_von(&self.chrome));
            liste.push(DisplayItem::Rect {
                rect: Rect::new(r.x + 9.0, r.y + r.height / 2.0 - 3.0, 6.0, 6.0),
                color: pal.akzent,
                radius: [3.0; 4],
                deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
            });
            let t: String = h.text.chars().take(MAX_HINWEIS_ZEICHEN).collect();
            let platz = ((r.width - 32.0) / 6.6).max(4.0) as usize;
            let t: String = if t.chars().count() > platz { format!("{}…", t.chars().take(platz.saturating_sub(1)).collect::<String>()) } else { t };
            liste.push(text(r.x + 22.0, r.y + 11.0, t, narsil_chrome::glas::GLAS_TEXT, 13.0, 400));
        }
        if let (Some((_, p)), Some((z1, z2))) = (self.features.link_tipp.as_ref(), self.link_tipp_text()) {
            let z1: String = z1.chars().take(110).collect();
            let b = (narsil_chrome::features::textbreite(&z1, 13.0).max(narsil_chrome::features::textbreite(&z2, 11.0)) + 24)
                .min(self.breite.saturating_sub(16)) as f32;
            let hoehe = 46.0;
            let mut x = p.x + 14.0;
            let mut y = p.y + 20.0;
            if x + b > self.breite as f32 - 8.0 {
                x = (self.breite as f32 - 8.0 - b).max(8.0);
            }
            if y + hoehe > self.hoehe as f32 - narsil_chrome::STATUSLEISTE_HOEHE as f32 {
                y = (p.y - hoehe - 8.0).max(0.0);
            }
            // Release 08.10.2026 (Glas-Stil): der Tooltip ist ein Glasfeld.
            narsil_chrome::glas::feld(
                liste,
                &glas_rechteck(Rect::new(x, y, b, hoehe)),
                narsil_chrome::glas::prozent_von(&self.chrome),
            );
            liste.push(text(x + 12.0, y + 7.0, z1, narsil_chrome::glas::GLAS_TEXT, 13.0, 600));
            liste.push(text(x + 12.0, y + 27.0, z2, narsil_chrome::glas::GLAS_GRAU, 11.0, 400));
        }
    }
}

/// M4: so viele Zeichen hat ein Hinweis hoechstens.
pub const MAX_HINWEIS_ZEICHEN: usize = 160;

/// Release 08.10.2026: ein Fliesskomma-Rechteck als `Rechteck` der Chrome.
fn glas_rechteck(r: narsil_core::geometry::Rect) -> narsil_chrome::Rechteck {
    narsil_chrome::Rechteck {
        x: r.x.max(0.0) as u32,
        y: r.y.max(0.0) as u32,
        breite: r.width.max(0.0) as u32,
        hoehe: r.height.max(0.0) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datum_hin_und_zurueck() {
        for (j, m, t) in [(1970, 1, 1), (2026, 10, 8), (2024, 2, 29), (2100, 12, 31)] {
            let tage = tage_aus_datum(j, m, t);
            assert_eq!(datum_aus_tagen(tage), (j, m, t));
        }
        assert_eq!(tage_aus_datum(1970, 1, 2), 1);
        assert_eq!(datum_lesen("08.10.2026"), datum_lesen("2026-10-08"));
        assert_eq!(datum_text(datum_lesen("31.12.2026").unwrap()), "2026-12-31");
        {
            let _de = narsil_i18n::scoped(narsil_i18n::Language::German);
            assert_eq!(datum_text(datum_lesen("2026-12-31").unwrap()), "31.12.2026");
        }
        assert_eq!(datum_lesen("32.01.2026"), None);
    }

    #[test]
    fn maske_und_html() {
        let s = "a\tb\nc\\d";
        assert_eq!(entmaske(&maske(s)), s);
        assert_eq!(felder(&zeile(&["x\ty", "z"])), vec!["x\ty".to_string(), "z".to_string()]);
        assert_eq!(html("<a href=\"x\">&"), "&lt;a href=&quot;x&quot;&gt;&amp;");
        assert_eq!(host_von("https://User@Www.Beispiel.de:8080/p?q"), "www.beispiel.de");
        assert_eq!(abfrage("narsil://x?a=1&b=h%C3%A4+u"), vec![("a".into(), "1".into()), ("b".into(), "hä u".into())]);
        assert_eq!(dekodiere(&kodiere("ä &=?")), "ä &=?");
    }
}
