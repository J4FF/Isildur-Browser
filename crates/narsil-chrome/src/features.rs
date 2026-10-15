//! Release 08.10.2026: DER TAB „FEATURES" in den Einstellungen.
//!
//! Jede Zusatzfunktion des Browsers hat hier
//!
//! * einen deutschen Namen ([`Feature::name`]),
//! * einen An/Aus-Schalter, der wie die uebrigen Werte in
//!   `einstellungen.txt` steht (`feature_<schluessel>=true|false`),
//! * ein „(i)", das eine kurze Erklaerung mit den ehrlichen Grenzen der
//!   Umsetzung einblendet ([`Feature::erklaerung`]),
//! * und, wo die Funktion Daten oder Regeln verwaltet, einen Knopf, der die
//!   zugehoerige eingebaute Seite (`narsil://…`) oeffnet.
//!
//! Die Funktionen selbst stehen in `narsil/src/features/` (Engine) und in
//! `narsil-net/src/haken.rs` (Netzschicht); diese Datei kennt nur Zustand,
//! Texte und Geometrie — dieselbe Teilung wie bei `einstellungen.rs`.

use super::thema::Palette;
use super::{Chrome, Rechteck};
use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect};
use narsil_i18n::{tr, tr_key};
use narsil_display::{DisplayItem, DisplayList};

/// Die Funktionen in der Nummerierung der Feature-Liste des Nutzers (0–13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    /// 0: das vorhandene „Konvertiert herunterladen" im Kontextmenue.
    Konvertieren,
    /// 1: Schnappschuss von DOM-Stand, Eingaben und Scrollstand; Wiederherstellung nach Absturz.
    Schnappschuss,
    /// 2: markierte Elemente im Hintergrund beobachten.
    Beobachter,
    /// 3: Netzregel je Tab und Canvas-Verrauschen.
    NetzSandbox,
    /// 4: verschluesseltes Formular-Gedaechtnis.
    FormularGedaechtnis,
    /// 5: Hintergrund-Tabs auf die Platte auslagern.
    TabEinfrieren,
    /// 6: Kopfzeilen-Regeln je Domain.
    Kopfregeln,
    /// 7: Tracking-Parameter entfernen, Shortener aufloesen, Umschreiben.
    UrlReinigung,
    /// 8: Abo- und Probeabo-Fristen.
    Fristen,
    /// 9: Pop-ups, Newsletter, Chats und Benachrichtigungsfragen ausblenden.
    Ruhemodus,
    /// 10: Kauf- und Bestellbestaetigungen mit Garantiefrist.
    Belege,
    /// 11: Cookie-Banner mit der datenschutzfreundlichsten Wahl beantworten.
    CookieAutopilot,
    /// 12: Ziel von Kurz- und Tracking-Links beim Darueberfahren zeigen.
    LinkZiel,
    /// 13 (Release 08.10.2026): Bilder auf Anzeigegroesse, Sichtfenster,
    /// spaeter laden — weniger Speicher und Netz.
    BildDiaet,
}

/// Alle Funktionen in der Nummerierung der Liste.
pub const ALLE: [Feature; 14] = [
    Feature::Konvertieren,
    Feature::Schnappschuss,
    Feature::Beobachter,
    Feature::NetzSandbox,
    Feature::FormularGedaechtnis,
    Feature::TabEinfrieren,
    Feature::Kopfregeln,
    Feature::UrlReinigung,
    Feature::Fristen,
    Feature::Ruhemodus,
    Feature::Belege,
    Feature::CookieAutopilot,
    Feature::LinkZiel,
    Feature::BildDiaet,
];

impl Feature {
    /// Die Nummer aus der Feature-Liste (0–13).
    pub fn nummer(self) -> u8 {
        ALLE.iter().position(|f| *f == self).unwrap_or(0) as u8
    }

    /// Der Schluessel in `einstellungen.txt` (`feature_<schluessel>`).
    pub fn schluessel(self) -> &'static str {
        match self {
            Feature::Konvertieren => "konvertieren",
            Feature::Schnappschuss => "schnappschuss",
            Feature::Beobachter => "beobachter",
            Feature::NetzSandbox => "netz_sandbox",
            Feature::FormularGedaechtnis => "formular_gedaechtnis",
            Feature::TabEinfrieren => "tab_einfrieren",
            Feature::Kopfregeln => "kopfregeln",
            Feature::UrlReinigung => "url_reinigung",
            Feature::Fristen => "fristen",
            Feature::Ruhemodus => "ruhemodus",
            Feature::Belege => "belege",
            Feature::CookieAutopilot => "cookie_autopilot",
            Feature::LinkZiel => "link_ziel",
            Feature::BildDiaet => "bild_diaet",
        }
    }

    /// Der Name, der im Tab steht.
    pub fn name(self) -> &'static str {
        tr(self.name_key())
    }

    /// The English name (the translation key of [`Self::name`]).
    pub fn name_key(self) -> &'static str {
        match self {
            Feature::Konvertieren => tr_key("Download converted"),
            Feature::Schnappschuss => tr_key("Session snapshot & crash protection"),
            Feature::Beobachter => tr_key("Element watcher"),
            Feature::NetzSandbox => tr_key("Network sandbox per tab"),
            Feature::FormularGedaechtnis => tr_key("Form memory"),
            Feature::TabEinfrieren => tr_key("Freeze tabs"),
            Feature::Kopfregeln => tr_key("Header rules per domain"),
            Feature::UrlReinigung => tr_key("Link cleaner"),
            Feature::Fristen => tr_key("Subscription & deadline assistant"),
            Feature::Ruhemodus => tr_key("Do not disturb"),
            Feature::Belege => tr_key("Receipt & warranty vault"),
            Feature::CookieAutopilot => tr_key("Cookie autopilot"),
            Feature::LinkZiel => tr_key("Link target preview"),
            Feature::BildDiaet => tr_key("Image diet"),
        }
    }

    /// Die Erklaerung hinter dem (i) — was die Funktion tut und wo ihre
    /// Grenze liegt. Absaetze sind durch `\n` getrennt.
    pub fn erklaerung(self) -> &'static str {
        tr(self.erklaerung_key())
    }

    /// The English explanation (the translation key of [`Self::erklaerung`]).
    pub fn erklaerung_key(self) -> &'static str {
        match self {
            Feature::Konvertieren => tr_key(
                "Adds the entry “Download converted >” to the context menu: images, sounds and videos are converted in Narsil's own engine (e.g. PNG → WebP/JPEG/BMP, WAV → FLAC, video → still image/GIF) and saved.\nOff: the entry disappears from the menu.",
            ),
            Feature::Schnappschuss => tr_key(
                "Every 20 seconds (only after changes) saves every tab: address, title, scroll position, form input and the current DOM state including script changes – compressed, with a checksum, in its own file in the profile. After a crash Narsil restores all tabs from the snapshot on the next start without fetching the pages again (also by hand under “Manage”).\nLimit: JavaScript variables, timers and open WebSockets cannot be saved out of Narsil's own JS engine. The page appears as a saved state without running scripts, images come from the cache or the network; F5 reloads it live.",
            ),
            Feature::Beobachter => tr_key(
                "Right-click a value (price, status, version number) → “Watch element”. Narsil checks the element in the background (every 5 minutes by default, adjustable) and every few seconds on the open page, and reports a change quietly in the status bar. Everything is local, without any service.\nLimit: In the background the HTML is checked without JavaScript; values that a script loads later are only seen by the watcher while the page is open.",
            ),
            Feature::NetzSandbox => tr_key(
                "Right-click a tab → “Network”: open, local only (localhost, 127.x, ::1) or blocked. Narsil's own network layer enforces the rule for every request – page, images, scripts, fetch and every redirect. In addition, canvas readouts (toDataURL, getImageData) get slight noise per page, so canvas fingerprints are not stable, without changing what is shown.\nLimit: There are no operating system namespaces and no VPN; the rule applies to the active tab. The engine has no WebRTC, so there is no WebRTC leak either.",
            ),
            Feature::FormularGedaechtnis => tr_key(
                "Remembers submitted forms encrypted (ChaCha20-Poly1305) in the profile and recognizes the field types: name, address, parcel locker/post number, postal code, city, e-mail, phone, IBAN (with check digit), BIC. Ctrl+Shift+F fills all matching fields on a new page at once, even with unusual field names.\nLimit: Passwords, card numbers and CVC are never stored. The key is its own file in the profile – it protects against someone reading the file, not against someone with full access to the profile.",
            ),
            Feature::TabEinfrieren => tr_key(
                "Tabs that stayed in the background for 10 minutes (or right-click the tab → “Freeze tab”) are cleared from memory: DOM state including script changes, input and scroll position go to disk compressed. When you click the tab, Narsil rebuilds it from there without fetching the page again, at the same position.\nLimit: Narsil's own engine cannot save the JavaScript context itself – the thawed tab shows the state without running scripts (F5 reloads live), images come from the cache or the network.",
            ),
            Feature::Kopfregeln => tr_key(
                "Set or remove your own rules per domain for request headers (e.g. User-Agent, Accept-Language, Referer, custom headers), and remove response headers. The rules apply in Narsil's own network layer to every request and every redirect. Edit them under “Rules”.\nOn purpose, no presets for getting around paywalls are included.",
            ),
            Feature::UrlReinigung => tr_key(
                "Removes tracking parameters (utm_*, fbclid, gclid, msclkid, mc_eid …) before loading and when copying, resolves short links (bit.ly, t.co, tinyurl …) with a HEAD request without an intermediate page, and can rewrite addresses: Reddit → Redlib, YouTube → Invidious (instance freely selectable under “Rules”).\nLimit: Shorteners that only redirect via JavaScript cannot be resolved this way.",
            ),
            Feature::Fristen => tr_key(
                "Recognizes pages that confirm a subscription or a trial (keywords, terms like “30 days”, dates like “ends on …”) and adds the service, start and cancellation deadline to a local list. Three days before it expires, Narsil reminds you and offers a ready-made cancellation template.\nLimit: The recognition is a heuristic – entries can be checked, changed and deleted in the list. Nothing is sent.",
            ),
            Feature::Ruhemodus => tr_key(
                "When you open a page, hides newsletter pop-ups, chat bubbles, “Allow notifications?” prompts and surveys, unlocks scrolling that an overlay blocked, and blocks the scripts of known chat and push services in the network. Uses the reader and checker of the built-in ad blocker with its own list of disturbers.\nLimit: Unusually built pop-ups without recognizable identifiers can slip through.",
            ),
            Feature::Belege => tr_key(
                "Recognizes order and purchase confirmations, reads shop, order number, date, amount and warranty period (default: 2 years statutory warranty) and stores the page together with a linked PDF invoice locally in the profile. 30 days before the warranty ends, Narsil reminds you.\nLimit: Heuristic – entries can be corrected in the list. The browser does not see invoices that only arrive by e-mail.",
            ),
            Feature::CookieAutopilot => tr_key(
                "Answers cookie banners automatically with the most privacy-friendly choice: looks in known consent dialogs (OneTrust, Cookiebot, Usercentrics, Didomi, Quantcast, Borlabs …) for the “Reject” or “Only necessary” button, clicks it (a real click event) and hides the dialog. In addition, “Sec-GPC: 1” and “DNT: 1” go out with every request.\nLimit: Dialogs without a reject button are only hidden; “Accept” is never clicked.",
            ),
            Feature::LinkZiel => tr_key(
                "When the mouse moves over a short or tracking link (bit.ly, t.co, google.com/url?q=, l.facebook.com …), Narsil resolves the target in the background with a HEAD request (Location header, without fetching the page) and shows the bare, cleaned target address as a hint at the pointer.\nLimit: Chains are followed up to 5 hops; redirects via JavaScript stay invisible.",
            ),
            Feature::BildDiaet => tr_key(
                "Keeps images in memory only as large as they are drawn (box filter, JPEG directly in the DCT at 1/2, 1/4, 1/8; with object-fit: cover the cover size). Images more than two screen heights out of view give up their pixels, the original data stays – when you scroll back they are decoded again in the background, and the space stays reserved. Narsil loads images with a fixed box only when they come close. The card shows the memory saved, “Manage” the state per tab and exceptions per domain.\nNever changed: SVG, graphics under 64 pixels, background images; with pixelated only by whole numbers. “Save image” and “Download converted” deliver the original.\nLimit: The network saving counts images, not bytes (what is never loaded has no known size); no Save-Data header.",
            ),
        }
    }

    /// Die Vorgabe beim ersten Start. Das vorhandene Konvertieren ist an —
    /// alle neuen Funktionen schaltet der Nutzer selbst ein. Release
    /// 08.10.2026: dazu die Bild-Diaet (Auftrag: Vorgabe AN; sie aendert
    /// nichts Sichtbares, sie spart nur).
    pub fn vorgabe(self) -> bool {
        matches!(self, Feature::Konvertieren | Feature::BildDiaet)
    }

    /// Der Knopf neben dem Schalter: Beschriftung und eingebaute Seite.
    /// The label is a translation key (show it with `tr`).
    pub fn verwalten(self) -> Option<(&'static str, &'static str)> {
        match self {
            Feature::Schnappschuss => Some((tr_key("Manage"), "narsil://schnappschuss")),
            Feature::Beobachter => Some((tr_key("List"), "narsil://beobachter")),
            Feature::FormularGedaechtnis => Some((tr_key("Data"), "narsil://formulare")),
            Feature::Kopfregeln => Some((tr_key("Rules"), "narsil://kopfregeln")),
            Feature::UrlReinigung => Some((tr_key("Rules"), "narsil://umleitungen")),
            Feature::Fristen => Some((tr_key("List"), "narsil://fristen")),
            Feature::Belege => Some((tr_key("List"), "narsil://belege")),
            Feature::BildDiaet => Some((tr_key("Manage"), "narsil://bild-diaet")),
            _ => None,
        }
    }
}

/// Release 08.10.2026: eine Zeile LIVE-STAND je Funktion (die Karte zeigt sie
/// neben dem Namen) — die Engine schreibt sie (`narsil/src/bild_diaet.rs`),
/// der Tab liest sie beim Zeichnen.
static STATUS: std::sync::Mutex<Option<std::collections::HashMap<Feature, String>>> = std::sync::Mutex::new(None);

/// Den Live-Stand einer Funktion setzen (leer = keiner).
pub fn status_setzen(f: Feature, text: impl Into<String>) {
    if let Ok(mut s) = STATUS.lock() {
        let text = text.into();
        let karte = s.get_or_insert_with(std::collections::HashMap::new);
        if text.is_empty() {
            karte.remove(&f);
        } else {
            karte.insert(f, text);
        }
    }
}

/// Der Live-Stand einer Funktion.
#[must_use]
pub fn status(f: Feature) -> Option<String> {
    STATUS.lock().ok().and_then(|s| s.as_ref().and_then(|k| k.get(&f).cloned()))
}

/// Die Schalter aller Funktionen — ein Bit je [`Feature::nummer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schalter {
    bits: u16,
}

impl Default for Schalter {
    fn default() -> Self {
        let mut s = Schalter { bits: 0 };
        for f in ALLE {
            s.setzen(f, f.vorgabe());
        }
        s
    }
}

impl Schalter {
    /// Ist die Funktion eingeschaltet?
    pub fn an(&self, f: Feature) -> bool {
        self.bits & (1 << f.nummer()) != 0
    }

    /// Eine Funktion schalten.
    pub fn setzen(&mut self, f: Feature, an: bool) {
        if an {
            self.bits |= 1 << f.nummer();
        } else {
            self.bits &= !(1 << f.nummer());
        }
    }

    /// Die Zeilen fuer `einstellungen.txt`.
    pub fn als_zeilen(&self) -> String {
        let mut s = String::new();
        for f in ALLE {
            s.push_str(&format!(
                "feature_{}={}\n",
                f.schluessel(),
                if self.an(f) { "true" } else { "false" }
            ));
        }
        s
    }

    /// Eine Zeile `feature_<schluessel>=<wert>` lesen; `false`, wenn der
    /// Schluessel keine Funktion ist.
    pub fn zeile_lesen(&mut self, schluessel: &str, wert: &str) -> bool {
        let Some(name) = schluessel.strip_prefix("feature_") else {
            return false;
        };
        match ALLE.iter().find(|f| f.schluessel() == name) {
            Some(f) => {
                self.setzen(*f, wert == "true");
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// DIE GEOMETRIE DES TABS
// ---------------------------------------------------------------------------

/// Der Takt einer Feature-Zeile (Karte plus Luft).
pub const ZEILE_FEATURE: u32 = 40;
/// Die Hoehe der Karte einer Zeile.
pub const HOEHE_KARTE: u32 = 34;
/// Breite und Hoehe des Schalters (Pille).
pub const SCHALTER_B: u32 = 38;
pub const SCHALTER_H: u32 = 20;
/// Kante des (i)-Symbols.
pub const INFO_KANTE: u32 = 16;
/// Schrift des Namens.
pub const SCHRIFT_NAME: f32 = 13.0;
/// Schrift der Erklaerung.
pub const SCHRIFT_ERKLAERUNG: f32 = 12.0;
/// Zeilenhoehe der Erklaerung.
pub const ZEILE_ERKLAERUNG: u32 = 17;
/// Breite des Verwalten-Knopfs.
pub const KNOPF_B: u32 = 82;
/// Hoehe des Verwalten-Knopfs.
pub const KNOPF_H: u32 = 24;
/// Innenrand der Karte.
pub const RAND: u32 = 12;
/// Release 08.10.2026 (Glas-Stil): der Radius der Feature-Karten.
pub const RADIUS_KARTE_GLAS: f32 = 12.0;
/// Hoechstens so viele Zeilen hat die Erklaerung (M3).
pub const MAX_ERKLAERUNG_ZEILEN: usize = 16;

/// Die geschaetzte Breite eines Textes in einer serifenlosen Schrift.
///
/// `narsil-chrome` misst keine Schrift (das macht der Rasterer). Die Schaetzung
/// arbeitet mit Breitenklassen (schmal i/l/t, breit m/w, Versalien) und liegt
/// absichtlich etwas ueber der echten Breite, damit das (i) den Namen nie
/// ueberdeckt.
pub fn textbreite(text: &str, grad: f32) -> u32 {
    let mut einheiten = 0.0f32;
    for c in text.chars() {
        einheiten += match c {
            'i' | 'l' | 'j' | '.' | ',' | ':' | ';' | '\'' | '|' | '!' => 0.28,
            'f' | 't' | 'r' | 'I' | ' ' | '(' | ')' | '-' => 0.36,
            'm' | 'w' | 'M' | 'W' | '&' | '@' => 0.86,
            'A'..='Z' | 'Ä' | 'Ö' | 'Ü' => 0.68,
            '0'..='9' => 0.56,
            _ => 0.55,
        };
    }
    (einheiten * grad * 1.04).ceil() as u32
}

/// Die Karte der Zeile mit dem Feld-Rechteck `zeile` (aus `einstellungen::feld`).
pub fn karte(zeile: &Rechteck) -> Rechteck {
    Rechteck {
        x: zeile.x,
        y: zeile.y,
        breite: zeile.breite,
        hoehe: HOEHE_KARTE,
    }
}

/// Der Schalter am rechten Rand der Karte.
pub fn schalter_rect(zeile: &Rechteck) -> Rechteck {
    let k = karte(zeile);
    Rechteck {
        x: k.rechts().saturating_sub(RAND + SCHALTER_B),
        y: k.y + (HOEHE_KARTE - SCHALTER_H) / 2,
        breite: SCHALTER_B,
        hoehe: SCHALTER_H,
    }
}

/// Das (i) in einer festen Spalte links vom Knopf-Platz.
///
/// Release 08.10.2026: GEMESSEN am Bild (1600 px, Segoe UI): direkt hinter
/// dem geschaetzten Namen lag das (i) mal 6, mal 25 px vom Text entfernt —
/// die Schaetzung kennt die Schrift nicht. Eine feste Spalte steht in
/// jeder Karte an derselben Stelle, unabhaengig von der Schrift und ohne
/// den Knopf zu beruehren.
pub fn info_rect(_f: Feature, zeile: &Rechteck) -> Rechteck {
    let k = karte(zeile);
    let spalte = k.rechts().saturating_sub(RAND + SCHALTER_B + KNOPF_B + 2 * 12 + INFO_KANTE);
    Rechteck {
        x: spalte.max(k.x + RAND),
        y: k.y + (HOEHE_KARTE - INFO_KANTE) / 2,
        breite: INFO_KANTE,
        hoehe: INFO_KANTE,
    }
}

/// Der Verwalten-Knopf links vom Schalter (nur bei Funktionen mit Seite).
pub fn knopf_rect(f: Feature, zeile: &Rechteck) -> Option<Rechteck> {
    f.verwalten()?;
    let s = schalter_rect(zeile);
    let k = karte(zeile);
    Some(Rechteck {
        x: s.x.saturating_sub(12 + KNOPF_B),
        y: k.y + (HOEHE_KARTE - KNOPF_H) / 2,
        breite: KNOPF_B,
        hoehe: KNOPF_H,
    })
}

/// Bricht die Erklaerung in Zeilen fuer eine Breite um (Absaetze bei `\n`).
pub fn umbrechen(text: &str, breite: u32, grad: f32) -> Vec<String> {
    let mut zeilen = Vec::new();
    for absatz in text.split('\n') {
        let mut zeile = String::new();
        for wort in absatz.split_whitespace() {
            let kandidat = if zeile.is_empty() {
                wort.to_string()
            } else {
                format!("{zeile} {wort}")
            };
            if textbreite(&kandidat, grad) > breite && !zeile.is_empty() {
                zeilen.push(std::mem::take(&mut zeile));
                zeile = wort.to_string();
            } else {
                zeile = kandidat;
            }
            if zeilen.len() >= MAX_ERKLAERUNG_ZEILEN {
                return zeilen;
            }
        }
        if !zeile.is_empty() {
            zeilen.push(zeile);
        }
    }
    zeilen.truncate(MAX_ERKLAERUNG_ZEILEN);
    zeilen
}

/// Das Rechteck der eingeblendeten Erklaerung (unter dem (i), in der
/// Breite der Karte, nach oben geklappt, wenn unten kein Platz ist).
pub fn erklaerung_rect(f: Feature, zeile: &Rechteck, fensterhoehe: u32) -> Rechteck {
    let k = karte(zeile);
    let innen = k.breite.saturating_sub(2 * 14);
    let zeilen = umbrechen(f.erklaerung(), innen, SCHRIFT_ERKLAERUNG).len() as u32;
    let hoehe = 14 + 20 + zeilen * ZEILE_ERKLAERUNG + 12;
    let unten = k.unten() + 6;
    let y = if unten + hoehe + super::STATUSLEISTE_HOEHE > fensterhoehe && k.y > hoehe + 6 {
        k.y - hoehe - 6
    } else {
        unten
    };
    Rechteck {
        x: k.x,
        y,
        breite: k.breite,
        hoehe,
    }
}

fn liegt_in(p: &Point, r: &Rechteck) -> bool {
    p.x >= r.x as f32 && p.x < r.rechts() as f32 && p.y >= r.y as f32 && p.y < r.unten() as f32
}

/// Was unter dem Punkt in einer Zeile liegt (ohne den Schalter — den trifft
/// der allgemeine Weg ueber `einstellungen::element`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeilenTreffer {
    Info,
    Verwalten,
}

/// Der Treffer in der Zeile `zeile` einer Funktion.
pub fn zeilen_treffer(f: Feature, zeile: &Rechteck, p: &Point) -> Option<ZeilenTreffer> {
    let i = info_rect(f, zeile);
    // Etwas groesser als das Symbol: 16 px sind ein kleines Ziel.
    let ziel = Rechteck {
        x: i.x.saturating_sub(4),
        y: i.y.saturating_sub(4),
        breite: i.breite + 8,
        hoehe: i.hoehe + 8,
    };
    if liegt_in(p, &ziel) {
        return Some(ZeilenTreffer::Info);
    }
    if let Some(k) = knopf_rect(f, zeile)
        && liegt_in(p, &k)
    {
        return Some(ZeilenTreffer::Verwalten);
    }
    None
}

/// Ob der Punkt in der eingeblendeten Erklaerung liegt.
pub fn in_erklaerung(f: Feature, zeile: &Rechteck, fensterhoehe: u32, p: &Point) -> bool {
    liegt_in(p, &erklaerung_rect(f, zeile, fensterhoehe))
}

fn rect(r: &Rechteck, farbe: Color, radius: f32, deckkraft: f32) -> DisplayItem {
    DisplayItem::Rect {
        rect: Rect::new(r.x as f32, r.y as f32, r.breite as f32, r.hoehe as f32),
        color: farbe,
        radius: [radius; 4],
        deckkraft,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    }
}

fn text(x: u32, y: u32, t: &str, farbe: Color, grad: f32, gewicht: u16) -> DisplayItem {
    DisplayItem::Text {
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        origin: Point::new(x as f32, y as f32),
        text: t.to_string(),
        color: farbe,
        font_size: grad,
        gewicht,
        kursiv: false,
    }
}

/// Eine Zeile zeichnen: Karte, Name, (i), ggf. Knopf, Schalter.
pub fn zeile_zeichnen(
    liste: &mut DisplayList,
    f: Feature,
    an: bool,
    info_offen: bool,
    zeile: &Rechteck,
    pal: &Palette,
) {
    let k = karte(zeile);
    // Die Karte: eine leicht abgehobene Flaeche mit feinem Rand.
    // Release 08.10.2026 (Glas-Stil): die Karte ist eine Glasflaeche — Grund,
    // Glanz und Lichtkante wie die Felder des Kontextmenues.
    let kr = Rect::new(k.x as f32, k.y as f32, k.breite as f32, k.hoehe as f32);
    // Release 09.10.2026 (Nutzer: „Glas billig"): flaches Glas wie die
    // Kategorienliste und die Karten der uebrigen Tabs — vorher eine
    // DECKENDE dunkle Flaeche (`panel_grund`), die auf dem Verlauf wie ein
    // schwarzes Loch stand.
    super::glas::flaeche(liste, kr, [RADIUS_KARTE_GLAS; 4], 100);
    // Der Name, senkrecht mittig.
    liste.push(text(
        k.x + RAND,
        k.y + (HOEHE_KARTE - 16) / 2,
        f.name(),
        pal.text,
        SCHRIFT_NAME,
        600,
    ));
    // Release 08.10.2026: der Live-Stand (z. B. Bild-Diaet: gesparter
    // Speicher) leise neben dem Namen, gekuerzt bis vor das (i).
    if an && let Some(stand) = status(f) {
        let x = k.x + RAND + textbreite(f.name(), SCHRIFT_NAME) + 14;
        let platz = info_rect(f, zeile).x.saturating_sub(x + 12);
        let mut t = stand;
        if textbreite(&t, 12.0) > platz {
            while !t.is_empty() && textbreite(&format!("{t}…"), 12.0) > platz {
                t.pop();
            }
            let gekuerzt = t.trim_end().to_string();
            t = if gekuerzt.is_empty() { gekuerzt } else { format!("{gekuerzt}…") };
        }
        if !t.is_empty() && platz > 40 {
            liste.push(text(x, k.y + (HOEHE_KARTE - 15) / 2 + 1, &t, pal.text_schwach, 12.0, 400));
        }
    }
    // Das (i).
    let i = info_rect(f, zeile);
    super::symbole::zeichne(
        liste,
        super::symbole::Symbol::Info,
        i.x,
        i.y,
        INFO_KANTE,
        if info_offen { pal.akzent } else { pal.text_schwach },
    );
    // Der Knopf zur eingebauten Seite.
    if let (Some(r), Some((beschriftung, _))) = (knopf_rect(f, zeile), f.verwalten()) {
        let beschriftung = tr(beschriftung);
        // Release 08.10.2026 (Glas-Stil): eine Pille mit Glanz und Lichtkante.
        let rund = r.hoehe as f32 / 2.0;
        liste.push(rect(&r, pal.adresse, rund, 1.0));
        super::glas::veredeln(
            liste,
            Rect::new(r.x as f32, r.y as f32, r.breite as f32, r.hoehe as f32),
            [rund; 4],
            100,
            super::glas::GLANZ_LEISE,
            super::glas::KANTE_HELL,
        );
        let tb = textbreite(beschriftung, 12.0).min(r.breite);
        liste.push(text(
            r.x + (r.breite - tb) / 2,
            r.y + (KNOPF_H - 15) / 2,
            beschriftung,
            pal.text,
            12.0,
            400,
        ));
    }
    // Der Schalter: Pille mit Knauf.
    let s = schalter_rect(zeile);
    liste.push(rect(
        &s,
        if an { pal.akzent } else { pal.trennlinie_tab },
        SCHALTER_H as f32 / 2.0,
        1.0,
    ));
    let knauf = SCHALTER_H - 6;
    let kx = if an { s.rechts() - 3 - knauf } else { s.x + 3 };
    liste.push(rect(
        &Rechteck {
            x: kx,
            y: s.y + 3,
            breite: knauf,
            hoehe: knauf,
        },
        Color::rgb(0xff, 0xff, 0xff),
        knauf as f32 / 2.0,
        1.0,
    ));
}

/// Die eingeblendete Erklaerung zeichnen (liegt ueber allem).
pub fn erklaerung_zeichnen(
    liste: &mut DisplayList,
    f: Feature,
    zeile: &Rechteck,
    fensterhoehe: u32,
    pal: &Palette,
) {
    let r = erklaerung_rect(f, zeile, fensterhoehe);
    // Release 08.10.2026 (Glas-Stil): die Erklaerung ist ein Glasfeld wie das
    // Kontextmenue (Ringschatten, dunkles Glas, Glanz, Lichtkante).
    super::glas::feld(liste, &r, 100);
    super::symbole::zeichne(
        liste,
        super::symbole::Symbol::Info,
        r.x + 14,
        r.y + 12,
        INFO_KANTE,
        pal.akzent,
    );
    liste.push(text(r.x + 14 + INFO_KANTE + 8, r.y + 12, f.name(), super::glas::GLAS_TEXT, 13.0, 700));
    let mut y = r.y + 14 + 20;
    for z in umbrechen(f.erklaerung(), r.breite.saturating_sub(28), SCHRIFT_ERKLAERUNG) {
        liste.push(text(r.x + 14, y, &z, super::glas::GLAS_TEXT, SCHRIFT_ERKLAERUNG, 400));
        y += ZEILE_ERKLAERUNG;
    }
}

/// Die Abschnitte des Tabs als (Ueberschrift, Funktionen).
pub const GRUPPEN: [(&str, &[Feature]); 5] = [
    (
        tr_key("Privacy & network"),
        &[
            Feature::NetzSandbox,
            Feature::Kopfregeln,
            Feature::UrlReinigung,
            Feature::LinkZiel,
        ],
    ),
    (
        tr_key("Quiet browsing"),
        &[Feature::Ruhemodus, Feature::CookieAutopilot],
    ),
    (
        tr_key("Session & tabs"),
        &[Feature::Schnappschuss, Feature::TabEinfrieren],
    ),
    (
        tr_key("Assistants"),
        &[
            Feature::Beobachter,
            Feature::FormularGedaechtnis,
            Feature::Fristen,
            Feature::Belege,
        ],
    ),
    (tr_key("Downloads & storage"), &[Feature::Konvertieren, Feature::BildDiaet]),
];

/// Die Kopfzeile ueber den Abschnitten.
/// A translation key (show it with `tr`).
pub const HINWEIS: &str =
    tr_key("Extra features of Narsil's own engine – each one can be switched on its own. (i) explains details and limits.");

/// Damit `Chrome` hier nicht ungenutzt importiert ist, wenn die Geometrie
/// spaeter die Leisten braucht: die Seite beginnt wie die anderen
/// Kategorien unter der Ueberschrift.
pub fn hinweis_y(chrome: &Chrome) -> u32 {
    chrome.inhalt_oberkante() + 6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vierzehn_funktionen_mit_namen_und_erklaerung() {
        assert_eq!(ALLE.len(), 14);
        for (i, f) in ALLE.iter().enumerate() {
            assert_eq!(f.nummer() as usize, i);
            assert!(!f.name().is_empty());
            assert!(f.erklaerung().len() > 80, "{f:?}: Erklaerung zu kurz");
            assert!(!f.erklaerung().contains("ae ") && !f.name().contains("oe"), "{f:?}");
            // Jede Erklaerung nennt ihre Grenze oder ihre Abschaltung.
            assert!(
                f.erklaerung_key().contains("Limit") || f.erklaerung_key().contains("Off:")
                    || f.erklaerung_key().contains("On purpose"),
                "{f:?}: keine Grenze genannt"
            );
        }
        // Schluessel eindeutig.
        for (i, a) in ALLE.iter().enumerate() {
            for b in &ALLE[i + 1..] {
                assert_ne!(a.schluessel(), b.schluessel());
                assert_ne!(a.name(), b.name());
            }
        }
    }

    #[test]
    fn jede_funktion_steht_in_genau_einer_gruppe() {
        for f in ALLE {
            let n = GRUPPEN.iter().filter(|(_, l)| l.contains(&f)).count();
            assert_eq!(n, 1, "{f:?} steht {n} mal in den Gruppen");
        }
    }

    #[test]
    fn schalter_vorgabe_und_datei() {
        let s = Schalter::default();
        assert!(s.an(Feature::Konvertieren), "Konvertieren war schon da und bleibt an");
        // Release 08.10.2026: die Bild-Diaet ist ab Werk an (Auftrag).
        assert!(s.an(Feature::BildDiaet), "Bild-Diät ist ab Werk an");
        for f in ALLE.iter().skip(1).filter(|f| **f != Feature::BildDiaet) {
            assert!(!s.an(*f), "{f:?} ist nicht aus");
        }
        let mut t = Schalter::default();
        t.setzen(Feature::Beobachter, true);
        t.setzen(Feature::Konvertieren, false);
        let mut gelesen = Schalter::default();
        for zeile in t.als_zeilen().lines() {
            let (k, w) = zeile.split_once('=').expect("k=v");
            assert!(gelesen.zeile_lesen(k, w), "{zeile}");
        }
        assert_eq!(gelesen, t);
        assert!(!gelesen.zeile_lesen("feature_gibtsnicht", "true"));
        assert!(!gelesen.zeile_lesen("zoom", "true"));
    }

    #[test]
    fn info_und_knopf_liegen_in_der_karte_und_ueberlappen_nicht() {
        let zeile = Rechteck { x: 300, y: 200, breite: 640, hoehe: 30 };
        for f in ALLE {
            let k = karte(&zeile);
            let i = info_rect(f, &zeile);
            let s = schalter_rect(&zeile);
            assert!(i.x >= k.x && i.rechts() <= k.rechts(), "{f:?}");
            assert!(!i.ueberlappt(&s), "{f:?}: (i) liegt auf dem Schalter");
            // Das (i) steht hinter dem Namen und nicht darauf.
            assert!(i.x >= k.x + RAND + textbreite(f.name(), SCHRIFT_NAME), "{f:?}");
            if let Some(b) = knopf_rect(f, &zeile) {
                assert!(!b.ueberlappt(&s) && !b.ueberlappt(&i), "{f:?}");
            }
            assert_eq!(
                zeilen_treffer(f, &zeile, &Point::new(i.x as f32 + 8.0, i.y as f32 + 8.0)),
                Some(ZeilenTreffer::Info)
            );
        }
    }

    #[test]
    fn die_erklaerung_bricht_um_und_passt() {
        let zeile = Rechteck { x: 300, y: 200, breite: 640, hoehe: 30 };
        for f in ALLE {
            let zeilen = umbrechen(f.erklaerung(), 612, SCHRIFT_ERKLAERUNG);
            assert!(!zeilen.is_empty() && zeilen.len() <= MAX_ERKLAERUNG_ZEILEN);
            for z in &zeilen {
                assert!(textbreite(z, SCHRIFT_ERKLAERUNG) <= 612 || !z.contains(' '), "{z}");
            }
            let r = erklaerung_rect(f, &zeile, 1080);
            assert!(r.unten() <= 1080, "{f:?}");
        }
    }
}
